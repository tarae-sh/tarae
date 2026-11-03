//! `cargo xtask kernels [--dry-run] [--repeat N] [--keep-going] [--no-build] [--keep-vm] [kernel …]` — tarae's sandbox tests on
//! several kernels (`.sb/kernels-plan.md`): the set is data, `xtask/kernels.toml` (each kernel's VM, its sysctls,
//! the modes it runs and the tier each is expected to reach). Run by you, outside tarae's sandbox (Lima and colima
//! keep their VMs in your home) — before a publish, and after a landing that touches the sandbox's Linux code.
//!
//! **Built once, run everywhere**: tarae's own tests are built once in the `build` colima VM (`cargo xtask linux
//! test --keep --bin tarae`), and every kernel runs that one binary (`test --kept [--as-user]`) in its own Docker —
//! a container uses its VM's kernel, so one VM a kernel. A colima kernel's Docker is its profile's
//! (`DOCKER_CONTEXT`); a Lima kernel's VM is made from Lima's own template for its cloud image (`_images/<image>`)
//! or from the place its row names (`download`, its digest checked), Docker put in by its `install`, the checkout and its repository's git folder mounted read-only (the checks'
//! clone reads them, nothing else of home), and what the binary needs carried in from colima's Docker with no
//! download: the `rust:<pin>` image (`docker save | docker load`), the kept binary (its sha256 then checked against
//! the build's), the rustup volume (the pinned toolchain a test's own cargo asks for) — into a colima profile's
//! other than the build's too (Y2's `tarae-yama2`). No kept binary in the build's Docker: nothing runs, said with
//! the way. One VM at a time: made, run, stopped and deleted (`--keep-vm` leaves it;
//! a VM a stopped run left is deleted first, said).
//!
//! Each mode runs `repeat` times in a row (the `sandbox::` tests, the must-be-refused suite among them), stopping
//! at the first failure — `--keep-going` runs every one and says each test that failed once, with the runs it
//! failed in, after the mode and again at the end (`failures`); then tarae's own tests `repeat` times in the
//! kernel's `whole` mode. Each kernel's facts
//! first (uname, arch, Landlock's ABI read by the kernel, Yama, the user-namespace restriction, the LSMs).
//! Results: `.sb/kernels/<date>-<commit>/<kernel>-<mode>.txt` (the facts, every run's output and time), the tables
//! a run printed for a snapshot it had none of or that differed in `tables/` (to bless from), sizes in
//! `sizes.txt`, one row a mode in `.sb/kernels/history.tsv` — never `target/`. A VM that didn't come up is
//! **not measured**, never a pass. `--dry-run` prints every command in order, and each Lima VM's file, running
//! nothing.

use std::collections::BTreeMap;
use std::io::{BufRead as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{Result, root};

const TABLE: &str = "xtask/kernels.toml";
const OUT: &str = ".sb/kernels";
const HISTORY_HEAD: &str =
    "date\tcommit\tkernel\tuname\tarch\tabi\tyama\tmode\texpected\tfound\tpassed\tcompared\tminutes\n";
const USAGE: &str =
    "usage: cargo xtask kernels [--dry-run] [--repeat N] [--keep-going] [--no-build] [--keep-vm] [kernel …]";

/// The tests a mode runs: the sandbox's, the must-be-refused suite among them.
const FILTER: &str = "sandbox::";

/// How a mode's container runs (`cargo xtask linux`'s options) — the tier it reaches is the kernel's to say.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Opts {
    /// `--userns`
    Own,
    /// Docker's profiles, `CAP_SYS_PTRACE` added
    Host,
    /// `--docker-default`
    Offline,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Mode {
    opts: Opts,
    /// `--as-user`
    user: bool,
}

impl Mode {
    fn parse(s: &str) -> Result<Mode> {
        let (base, user) = match s.strip_suffix("-user") {
            Some(b) => (b, true),
            None => (s, false),
        };
        let opts = match base {
            "own" => Opts::Own,
            "host" => Opts::Host,
            "offline" => Opts::Offline,
            _ => return Err(format!("{s}: a mode is own, host or offline, `-user` after it as a user")),
        };
        Ok(Mode { opts, user })
    }

    fn name(self) -> String {
        let base = match self.opts {
            Opts::Own => "own",
            Opts::Host => "host",
            Opts::Offline => "offline",
        };
        format!("{base}{}", if self.user { "-user" } else { "" })
    }
}

#[derive(Clone, Debug, PartialEq)]
enum At {
    /// A colima profile, and `colima start`'s options for it.
    Colima { profile: String, start: Vec<String> },
    /// A Lima VM from `_images/<image>` — or from `download`, where the table names its image's place — Docker put
    /// in by `install` (a shell line, as root).
    Lima { image: String, install: String, download: Option<Download> },
}

/// A Lima kernel's image from a place the table names (`download`) instead of its template's: the template's own
/// place can be a redirector that sends a plain client to a mirror refusing it (K6's first run, 2026-11-03: Fedora's
/// to TUNA's 403). Lima checks the digest as it checks a template's.
#[derive(Clone, Debug, PartialEq)]
struct Download {
    location: String,
    arch: String,
    digest: String,
}

/// The table's `download = { location, arch, digest }` of a kernel, checked: an https address, an arch Lima
/// names, a sha256 digest — nothing else in it.
fn download(t: &toml::Table, of: &str) -> Result<Option<Download>> {
    let Some(d) = t.get("download") else {
        return Ok(None);
    };
    let d = d.as_table().ok_or(format!("{of}: download: a table of location, arch and digest"))?;
    if let Some(k) = d.keys().find(|k| !["location", "arch", "digest"].contains(&k.as_str())) {
        return Err(format!("{of}: download.{k}: only location, arch and digest"));
    }
    let of = format!("{of}: download");
    let get = |key: &str| string(d, key, &of)?.ok_or(format!("{of}: no {key}"));
    let (location, arch, digest) = (get("location")?, get("arch")?, get("digest")?);
    if !location.starts_with("https://") || !plain(&location) || host(&location).is_empty() {
        return Err(format!(
            "{of}: location {location}: an https:// address, letters, digits and `._-/=:` only"
        ));
    }
    if !["aarch64", "x86_64"].contains(&arch.as_str()) {
        return Err(format!("{of}: arch {arch}: aarch64 or x86_64"));
    }
    let hex = digest.strip_prefix("sha256:").unwrap_or_default();
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
        return Err(format!("{of}: digest {digest}: sha256: and 64 lowercase hex digits"));
    }
    Ok(Some(Download { location, arch, digest }))
}

/// An address's host (`https://dl.fedoraproject.org/pub/…` → `dl.fedoraproject.org`).
fn host(url: &str) -> &str {
    url.split_once("://").map_or("", |(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default())
}

#[derive(Clone, Debug)]
struct Kernel {
    name: String,
    says: String,
    at: At,
    sysctl: Vec<String>,
    env: Vec<String>,
    modes: Vec<Mode>,
    /// The tier each mode reaches, by the mode's name.
    expect: BTreeMap<String, String>,
    whole: Mode,
}

struct Table {
    build: String,
    repeat: u32,
    kernels: Vec<Kernel>,
}

fn strings(t: &toml::Table, key: &str, of: &str) -> Result<Vec<String>> {
    match t.get(key) {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(a)) => a
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or(format!("{of}: {key}: strings only")))
            .collect(),
        Some(_) => Err(format!("{of}: {key}: a list of strings")),
    }
}

fn string(t: &toml::Table, key: &str, of: &str) -> Result<Option<String>> {
    match t.get(key) {
        None => Ok(None),
        Some(v) => v.as_str().map(|s| Some(s.to_string())).ok_or(format!("{of}: {key}: a string")),
    }
}

/// A word that goes into a shell line or a file Lima reads as it is: letters, digits and `._-/=:` only.
fn plain(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-/=:".contains(c))
}

fn kernel(t: &toml::Table) -> Result<Kernel> {
    let name = string(t, "name", "a kernel")?.ok_or("a kernel without a name")?;
    if !plain(&name) || name.contains(['/', '=', ':']) {
        return Err(format!("{name}: a kernel's name is letters, digits, `.`, `_` and `-`"));
    }
    let of = name.as_str();
    let at = match (string(t, "colima", of)?, string(t, "image", of)?) {
        (Some(_), None) if t.contains_key("download") => {
            return Err(format!("{of}: download is a Lima image's: with image, not colima"));
        }
        (Some(profile), None) if plain(&profile) => At::Colima { profile, start: strings(t, "start", of)? },
        (None, Some(image)) if plain(&image) => {
            let install = string(t, "install", of)?.ok_or(format!("{of}: an image needs its install"))?;
            At::Lima { image, install, download: download(t, of)? }
        }
        _ => return Err(format!("{of}: one of colima (a profile) or image (Lima's template)")),
    };
    let sysctl = strings(t, "sysctl", of)?;
    if let Some(s) = sysctl.iter().find(|s| !plain(s) || !s.contains('=')) {
        return Err(format!("{of}: sysctl {s}: name=value"));
    }
    let env = strings(t, "env", of)?;
    if let Some(e) = env.iter().find(|e| {
        let n = e.split_once('=').map(|(n, _)| n).unwrap_or_default();
        n.is_empty() || !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }) {
        return Err(format!("{of}: env {e}: NAME=VALUE"));
    }
    let modes = strings(t, "modes", of)?.iter().map(|m| Mode::parse(m)).collect::<Result<Vec<_>>>()?;
    if modes.is_empty() {
        return Err(format!("{of}: no modes"));
    }
    let whole = Mode::parse(&string(t, "whole", of)?.ok_or(format!("{of}: no whole"))?)?;
    let mut expect = BTreeMap::new();
    if let Some(e) = t.get("expect") {
        let e = e.as_table().ok_or(format!("{of}: expect: a table of mode = tier"))?;
        for (m, tier) in e {
            let mode = Mode::parse(m)?;
            let tier = tier.as_str().unwrap_or_default();
            if !["own", "host", "offline"].contains(&tier) {
                return Err(format!("{of}: expect.{m}: own, host or offline"));
            }
            if !modes.contains(&mode) && mode != whole {
                return Err(format!("{of}: expect.{m}: not one of its modes"));
            }
            expect.insert(mode.name(), tier.to_string());
        }
    }
    let says = string(t, "says", of)?.unwrap_or_default();
    Ok(Kernel { name, says, at, sysctl, env, modes, expect, whole })
}

fn table(text: &str) -> Result<Table> {
    let t: toml::Table = text.parse().map_err(|e| format!("{TABLE}: {e}"))?;
    let build =
        string(&t, "build", TABLE)?.filter(|b| plain(b)).ok_or(format!("{TABLE}: build: a profile"))?;
    let repeat = t.get("repeat").and_then(|r| r.as_integer()).filter(|r| *r > 0).unwrap_or(3) as u32;
    let kernels = match t.get("kernel") {
        Some(toml::Value::Array(a)) => a
            .iter()
            .map(|k| k.as_table().ok_or(format!("{TABLE}: [[kernel]]: a table")).and_then(kernel))
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(format!("{TABLE}: no [[kernel]]")),
    };
    let mut names: Vec<&str> = kernels.iter().map(|k| k.name.as_str()).collect();
    names.sort_unstable();
    if let Some(w) = names.windows(2).find(|w| w[0] == w[1]) {
        return Err(format!("{TABLE}: {} twice", w[0]));
    }
    Ok(Table { build, repeat, kernels })
}

/// Which Docker a command talks to.
#[derive(Clone, Debug, PartialEq)]
enum Docker {
    /// colima's profile: its context.
    Context(String),
    /// A Lima VM: its forwarded socket.
    Host(String),
}

impl Docker {
    /// Its name as said: colima's profile, a Lima VM's.
    fn name(&self) -> &str {
        match self {
            Docker::Context(c) if c == "colima" => "default",
            Docker::Context(c) => c.strip_prefix("colima-").unwrap_or(c),
            Docker::Host(h) => {
                (h.strip_suffix("/sock/docker.sock")).and_then(|p| p.rsplit('/').next()).unwrap_or(h)
            }
        }
    }
}

fn colima_context(profile: &str) -> String {
    match profile {
        "default" => "colima".into(),
        p => format!("colima-{p}"),
    }
}

/// A command as it runs: its program and words, and the Docker it talks to.
#[derive(Clone, Debug, PartialEq)]
struct Cmd {
    docker: Option<Docker>,
    argv: Vec<String>,
}

impl Cmd {
    fn new(argv: &[&str]) -> Cmd {
        Cmd { docker: None, argv: argv.iter().map(|a| a.to_string()).collect() }
    }

    fn on(mut self, docker: &Docker) -> Cmd {
        self.docker = Some(docker.clone());
        self
    }

    fn args<S: AsRef<str>>(mut self, more: impl IntoIterator<Item = S>) -> Cmd {
        self.argv.extend(more.into_iter().map(|s| s.as_ref().to_string()));
        self
    }

    /// As a shell line, for `--dry-run` and the results.
    fn line(&self) -> String {
        let env = match &self.docker {
            None => String::new(),
            Some(Docker::Context(c)) => format!("DOCKER_CONTEXT={c} "),
            Some(Docker::Host(h)) => format!("DOCKER_HOST={h} "),
        };
        let words: Vec<String> = self.argv.iter().map(|w| shown(w)).collect();
        format!("{env}{}", words.join(" "))
    }

    fn command(&self, xtask: &Path) -> Command {
        let program = match self.argv[0].as_str() {
            "xtask" => xtask.as_os_str().to_owned(),
            p => p.into(),
        };
        let mut c = Command::new(program);
        c.args(&self.argv[1..]).current_dir(root());
        match &self.docker {
            None => {}
            Some(Docker::Context(x)) => {
                c.env("DOCKER_CONTEXT", x).env_remove("DOCKER_HOST");
            }
            Some(Docker::Host(h)) => {
                c.env("DOCKER_HOST", h).env_remove("DOCKER_CONTEXT");
            }
        }
        c
    }
}

/// A word as a shell line shows it: as it is when plain, else quoted.
fn shown(w: &str) -> String {
    match !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || "._-/=:@,+%".contains(c)) {
        true => w.to_string(),
        false => crate::linux::quoted(w),
    }
}

/// The tier `mode` is expected to reach on `k`: the table's, else offline under `--docker-default` (as `cargo xtask
/// linux` expects unless told), else none — said on the command and in the history row alike.
fn expected(k: &Kernel, mode: Mode) -> Option<&str> {
    match k.expect.get(&mode.name()) {
        Some(t) => Some(t),
        None => (mode.opts == Opts::Offline).then_some("offline"),
    }
}

/// `xtask` is this program (`cargo xtask …`).
fn xtask(args: &[&str]) -> Cmd {
    Cmd::new(&["xtask"]).args(args)
}

/// The one build: tarae's own tests, kept in the checkout's target volume.
fn build_cmd(docker: &Docker) -> Cmd {
    xtask(&["linux", "test", "--keep", "--bin", "tarae"]).on(docker)
}

/// `cargo xtask linux` for `mode` on `k`: its `env`, the tier expected, the container's options, the kept binary
/// — `filter`'s tests, or all of them.
fn suite_cmd(k: &Kernel, mode: Mode, filter: Option<&str>, docker: &Docker) -> Cmd {
    let mut a = vec!["linux".to_string()];
    for e in &k.env {
        a.extend(["--env".into(), e.clone()]);
    }
    if let Some(t) = expected(k, mode) {
        a.extend(["--env".into(), format!("TARAE_EXPECT_TIER={t}")]);
    }
    match mode.opts {
        Opts::Own => a.push("--userns".into()),
        Opts::Host => {}
        Opts::Offline => a.push("--docker-default".into()),
    }
    a.extend(["test".into(), "--kept".into()]);
    if mode.user {
        a.push("--as-user".into());
    }
    a.extend(filter.map(str::to_string));
    Cmd::new(&["xtask"]).args(a).on(docker)
}

/// What a kernel's Docker is asked for its facts: uname, Landlock's ABI as the kernel answers it (a program built
/// in the image, seccomp lifted so Docker's filter doesn't answer for the kernel), Yama, the restriction — and its
/// disk (`df -P /`'s row: where its volumes are too), for `room`.
const FACTS: &str = r#"echo "uname: $(uname -r)"
echo "disk: $(df -P / | sed -n 2p)"
echo "arch: $(uname -m)"
echo "yama: $(cat /proc/sys/kernel/yama/ptrace_scope 2>/dev/null || echo none)"
echo "userns restriction: $(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns 2>/dev/null || echo none)"
echo "max user namespaces: $(cat /proc/sys/user/max_user_namespaces 2>/dev/null || echo none)"
cat > /tmp/abi.c <<'EOF'
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/syscall.h>
#include <unistd.h>
int main(void) {
    long v = syscall(444, NULL, 0, 1UL); /* landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION) */
    if (v < 0) printf("landlock abi: none (%s)\n", strerror(errno));
    else printf("landlock abi: %ld\n", v);
    return 0;
}
EOF
cc -o /tmp/abi /tmp/abi.c && /tmp/abi"#;

/// What the VM itself is asked: its LSMs and its system.
const VM_FACTS: &str = r#"echo "lsm: $(cat /sys/kernel/security/lsm 2>/dev/null || echo unreadable)"; . /etc/os-release; echo "system: $PRETTY_NAME""#;

fn facts_cmd(docker: &Docker, image: &str) -> Cmd {
    Cmd::new(&["docker", "run", "--rm", "--security-opt", "seccomp=unconfined", image, "sh", "-c", FACTS])
        .on(docker)
}

fn vm_shell(k: &Kernel, script: &str) -> Cmd {
    match &k.at {
        At::Colima { profile, .. } => Cmd::new(&["colima", "ssh", "-p", profile, "--", "sh", "-c", script]),
        At::Lima { .. } => Cmd::new(&["limactl", "shell", "--workdir", "/", &vm_name(k), "sh", "-c", script]),
    }
}

/// A Lima kernel's VM.
fn vm_name(k: &Kernel) -> String {
    format!("tarae-{}", k.name.to_ascii_lowercase())
}

/// Lima's folder (`LIMA_HOME`, else `~/.lima`).
fn lima_home() -> PathBuf {
    match std::env::var_os("LIMA_HOME") {
        Some(h) => PathBuf::from(h),
        None => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".lima"),
    }
}

fn docker_of(k: &Kernel) -> Docker {
    match &k.at {
        At::Colima { profile, .. } => Docker::Context(colima_context(profile)),
        At::Lima { .. } => {
            Docker::Host(format!("unix://{}/{}/sock/docker.sock", lima_home().display(), vm_name(k)))
        }
    }
}

/// The first words of the Lima VM's file this tool makes (`lima_yaml`) — what tells its VM from a user's by the name.
const MADE: &str = "# made by `cargo xtask kernels`";

fn made_here(yaml: &str) -> bool {
    yaml.starts_with(MADE)
}

/// The Lima VM's file for `k`: Lima's own template for its image — or the table's `download`, alone (a template's
/// images would be tried after it, its refusing place again) —, 4 CPUs and 8 GiB as colima's test VMs, only the
/// checkout and its git folder mounted (read-only), no containerd of Lima's, Docker put in as root (its socket
/// the user's, forwarded to the Mac), the sysctls set at each boot.
fn lima_yaml(k: &Kernel, mounts: &[&Path]) -> Result<String> {
    let At::Lima { image, install, download } = &k.at else {
        return Err(format!("{}: not a Lima kernel", k.name));
    };
    let from = match download {
        None => format!("base:\n- template:_images/{image}\n"),
        Some(d) => format!(
            "# ({image}'s image from {}, not its template's place: the table's download)\n\
             images:\n- location: \"{}\"\n  arch: {}\n  digest: \"{}\"\n",
            host(&d.location),
            d.location,
            d.arch,
            d.digest
        ),
    };
    let mut y = format!(
        "{MADE} for {} — deleted after its run\n\
         minimumLimaVersion: 2.0.0\n\
         {from}\
         vmType: vz\ncpus: 4\nmemory: 8GiB\ndisk: 40GiB\n\
         # (the checks' clone reads the checkout and its git folder: nothing else of home)\n\
         mounts:\n",
        k.name
    );
    for m in mounts {
        let m = m.to_string_lossy();
        if m.contains(['"', '\\', '\n']) {
            return Err(format!("{m}: a path Lima's file can't hold as it is"));
        }
        y.push_str(&format!("- location: \"{m}\"\n  writable: false\n"));
    }
    y.push_str("containerd:\n  system: false\n  user: false\nprovision:\n- mode: system\n  script: |\n");
    let mut script = vec!["#!/bin/bash".to_string(), "set -eux -o pipefail".into()];
    script.extend(k.sysctl.iter().map(|s| format!("sysctl -w {s}")));
    script.push("if ! command -v docker >/dev/null 2>&1; then".into());
    script.push(format!("  {}", install.replace('\n', "\n  ")));
    script.push("fi".into());
    // (the socket the user's, so Lima's forward reaches it; restarted after the override)
    script.extend(
        [
            "mkdir -p /etc/systemd/system/docker.socket.d",
            "printf '[Socket]\\nSocketUser=%s\\n' \"$LIMA_CIDATA_USER\" > /etc/systemd/system/docker.socket.d/override.conf",
            "systemctl stop docker.service docker.socket || true",
            "systemctl daemon-reload",
            "systemctl enable docker.socket docker.service",
            "systemctl start docker.socket docker.service",
        ]
        .map(String::from),
    );
    for l in script.join("\n").lines() {
        y.push_str(&format!("    {l}\n"));
    }
    y.push_str(
        "probes:\n- script: |\n\
         \x20   #!/bin/bash\n\
         \x20   set -eux -o pipefail\n\
         \x20   timeout 900s bash -c \"until docker info >/dev/null 2>&1; do sleep 3; done\"\n\
         \x20 hint: See /var/log/cloud-init-output.log in the guest\n\
         portForwards:\n\
         - guestSocket: \"/var/run/docker.sock\"\n\
         \x20 hostSocket: \"{{.Dir}}/sock/docker.sock\"\n",
    );
    Ok(y)
}

/// What a kernel's runs take from the build's Docker: the `rust:<pin>` image, the checkout's target volume with the
/// kept binary in it — made first as `cargo xtask linux` makes it, labelled (`make`: `docker volume create`'s
/// words), so its `prune` judges it —, the rustup volume.
#[derive(Clone, Debug)]
struct Needs {
    image: String,
    target: String,
    make: Vec<String>,
    rustup: String,
}

/// The Docker `k`'s runs need the kept binary carried into — its own, where it isn't the build's (`from`): every
/// Lima VM's, and a colima profile's other than the build's (Y2's `tarae-yama2`). None: the build's own Docker.
fn carried_into(k: &Kernel, from: &Docker) -> Option<Docker> {
    Some(docker_of(k)).filter(|d| d != from)
}

/// What's carried from the build's Docker into another, each a command or a pipe: the image, the target volume
/// made, the kept binary into it, the rustup volume.
fn carry(from: &Docker, to: &Docker, n: &Needs) -> Vec<(Cmd, Option<Cmd>)> {
    let (kept, image) = (crate::linux::KEPT, n.image.as_str());
    let vol = |v: &str, at: &str| format!("{v}:{at}");
    let (target, rustup) = (vol(&n.target, "/target"), vol(&n.rustup, "/r"));
    vec![
        (Cmd::new(&["docker", "save", image]).on(from), Some(Cmd::new(&["docker", "load"]).on(to))),
        (Cmd::new(&["docker"]).args(&n.make).on(to), None),
        (
            Cmd::new(&["docker", "run", "--rm", "-v", &target, image, "cat", kept]).on(from),
            Some(
                Cmd::new(&["docker", "run", "--rm", "-i", "-v", &target, image, "sh", "-ec"])
                    .args([format!(
                        "mkdir -p \"$(dirname {kept})\" && cat > {kept}.new && chmod 0755 {kept}.new && mv {kept}.new {kept}"
                    )])
                    .on(to),
            ),
        ),
        (
            Cmd::new(&["docker", "run", "--rm", "-v", &rustup, image, "tar", "-C", "/r", "-cf", "-", "."]).on(from),
            Some(
                Cmd::new(&["docker", "run", "--rm", "-i", "-v", &rustup, image, "tar", "-C", "/r", "-xf", "-"])
                    .on(to),
            ),
        ),
    ]
}

/// The kept binary's sha256 in `d`, as `sha256sum` prints it.
fn kept_sum(d: &Docker, n: &Needs) -> Cmd {
    let target = format!("{}:/target", n.target);
    Cmd::new(&["docker", "run", "--rm", "-v", &target, &n.image, "sha256sum", crate::linux::KEPT]).on(d)
}

/// `sha256sum`'s hash, its first word — None: it printed none.
fn sum_of(said: &str) -> Option<&str> {
    said.split_whitespace().next().filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// A Docker with no kept test binary, said with the way: the build's is kept by the one build; another's is carried
/// from the build's before a kernel's runs (`carry_in`), its hash checked.
fn no_kept(at: &Docker, build: &Docker, k: Option<&str>) -> String {
    let keep = build_cmd(build).line().replacen(" xtask ", " cargo xtask ", 1);
    match (at == build, k) {
        (true, _) | (false, None) => format!(
            "no kept binary in {}'s Docker — `cargo xtask kernels` keeps it there (not with `--no-build`), or `{keep}`",
            build.name()
        ),
        (false, Some(k)) => format!(
            "no kept binary in {}'s Docker — it's carried there from {}'s before {k}'s runs, its sha256 checked: \
             `cargo xtask kernels --no-build {k}` carries it again",
            at.name(),
            build.name()
        ),
    }
}

/// Carries what `to`'s runs need from `from` (`carry`), the kept binary then checked against the build's `sum`.
fn carry_in(from: &Docker, to: &Docker, n: &Needs, sum: &str, x: &Path) -> Result<()> {
    for (a, b) in carry(from, to, n) {
        let ok = match &b {
            Some(b) => {
                eprintln!("$ {} | {}", a.line(), b.line());
                piped(&a, b, x)
            }
            None => said_status(&a, x),
        };
        if !ok {
            return Err(format!("carrying failed: {}", a.line()));
        }
    }
    let c = kept_sum(to, n);
    eprintln!("$ {}", c.line());
    let said = quiet(c.command(x)).unwrap_or_default();
    match sum_of(&said) {
        Some(s) if s == sum => Ok(()),
        Some(s) => Err(format!(
            "the kept binary carried into {}'s Docker isn't the build's: sha256 {s} there, {sum} in {}'s",
            to.name(),
            from.name()
        )),
        None => Err(format!(
            "no kept binary in {}'s Docker after carrying it from {}'s (`{}` printed no sha256)",
            to.name(),
            from.name(),
            c.line()
        )),
    }
}

/// The lines `--dry-run` prints for `k`, in order (a Lima VM's file after them).
fn plan(k: &Kernel, repeat: u32, from: &Docker, n: &Needs) -> Vec<String> {
    let docker = docker_of(k);
    let mut l = vec![format!("# {} — {}", k.name, k.says)];
    match &k.at {
        At::Colima { profile, start } => {
            l.push(format!(
                "{}    # only if it isn't running; stopped after",
                Cmd::new(&["colima", "start", "-p", profile, "--activate=false"]).args(start).line()
            ));
            for s in &k.sysctl {
                l.push(Cmd::new(&["colima", "ssh", "-p", profile, "--", "sudo", "sysctl", "-w", s]).line());
            }
        }
        At::Lima { image: img, download, .. } => {
            let name = vm_name(k);
            l.push(format!(
                "{}    # only if a stopped run left it",
                Cmd::new(&["limactl", "delete", "--force", &name]).line()
            ));
            let source = match download {
                None => format!("Lima's _images/{img}"),
                Some(d) => format!("{img} from {} (its digest checked)", host(&d.location)),
            };
            l.push(format!(
                "{}    # downloads {source} once; Docker's packages inside",
                Cmd::new(&[
                    "limactl",
                    "start",
                    &format!("--name={name}"),
                    "--tty=false",
                    &format!("<{name}.yaml>")
                ])
                .line()
            ));
        }
    }
    if let Some(to) = carried_into(k, from) {
        for (a, b) in carry(from, &to, n) {
            match b {
                Some(b) => l.push(format!("{} | {}", a.line(), b.line())),
                None => l.push(a.line()),
            }
        }
        l.push(format!("{}    # the build's sha256, else not measured", kept_sum(&to, n).line()));
    }
    l.push(format!("{}    # the facts", facts_cmd(&docker, &n.image).line()));
    l.push(vm_shell(k, VM_FACTS).line());
    for m in &k.modes {
        l.push(format!("{}    # ×{repeat}", suite_cmd(k, *m, Some(FILTER), &docker).line()));
    }
    l.push(format!("{}    # ×{repeat}, the whole", suite_cmd(k, k.whole, None, &docker).line()));
    match &k.at {
        At::Colima { profile, .. } => l.push(format!(
            "{}    # only if this run started it",
            Cmd::new(&["colima", "stop", "-p", profile]).line()
        )),
        At::Lima { .. } => {
            let name = vm_name(k);
            l.push(Cmd::new(&["limactl", "stop", &name]).line());
            l.push(Cmd::new(&["limactl", "delete", "--force", &name]).line());
        }
    }
    l
}

/// A run's output, as libtest and the suite say it: the tier the probe found, whether the must-be-refused table
/// was held against a snapshot (or why not), and the tables printed for a snapshot it had none of or that
/// differed, by name.
#[derive(Debug, Default, PartialEq)]
struct Read {
    found: Option<String>,
    compared: Option<String>,
    tables: BTreeMap<String, String>,
}

const UNCOMPARED: &str = "tarae-test: must-be-refused snapshot not compared — ";
const TABLE_HEAD: &str = "# must-be-refused suite (";

fn read(out: &str) -> Read {
    let mut r = Read::default();
    let mut pending: Option<String> = None;
    let mut lines = out.lines().peekable();
    while let Some(l) = lines.next() {
        if let Some(rest) = l.strip_prefix("tier here: ") {
            r.found = rest.split_whitespace().next().map(str::to_string);
        } else if let Some(why) = l.strip_prefix(UNCOMPARED) {
            r.compared = Some(format!("not compared: {why}"));
        } else if l.contains("must_be_refused") && l.trim_end().ends_with("... ok") {
            r.compared.get_or_insert_with(|| "compared".into());
        } else if l.contains("must_be_refused") && l.trim_end().ends_with("... FAILED") {
            r.compared = Some("failed".into());
        }
        // (`no snapshot …/NAME.txt yet`, `snapshot NAME differs`: the table after it is that snapshot's)
        if let Some(at) = l.find("no snapshot ") {
            let path = l[at + "no snapshot ".len()..].split_whitespace().next().unwrap_or_default();
            pending = Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned());
        } else if let Some(at) = l.find("snapshot ").filter(|_| l.contains(" differs (")) {
            pending = l[at + "snapshot ".len()..].split_whitespace().next().map(str::to_string);
        }
        if let Some(start) = l.find(TABLE_HEAD)
            && let Some(name) = pending.take()
        {
            let mut t = format!("{}\n", &l[start..]);
            while let Some(next) = lines.peek() {
                if !(next.starts_with("# claim") || next.contains(" | ")) {
                    break;
                }
                t.push_str(next);
                t.push('\n');
                lines.next();
            }
            r.tables.insert(name, t);
        }
    }
    r
}

/// A kernel's facts, as `FACTS` and `VM_FACTS` say them.
#[derive(Debug, Default)]
struct Facts {
    text: String,
}

impl Facts {
    fn get(&self, key: &str) -> String {
        let head = format!("{key}: ");
        (self.text.lines().find_map(|l| l.strip_prefix(&head)))
            .map(|v| v.trim().to_string())
            .unwrap_or_else(|| "-".into())
    }
}

/// A command's output to its end, passed on as it comes and kept (stdout and stderr one stream) — with whether it
/// succeeded.
fn teed(mut c: Command) -> (bool, String) {
    let Ok((reader, writer)) = std::io::pipe() else {
        return (false, "no pipe".into());
    };
    let Ok(err) = writer.try_clone() else {
        return (false, "no pipe".into());
    };
    c.stdin(Stdio::null()).stdout(writer).stderr(err);
    let child = c.spawn();
    // (the command holds the pipe's writing ends until it's dropped: the read would never end)
    drop(c);
    let mut child = match child {
        Ok(ch) => ch,
        Err(e) => return (false, format!("couldn't start it: {e}\n")),
    };
    let mut kept = String::new();
    for line in std::io::BufReader::new(reader).lines().map_while(std::result::Result::ok) {
        println!("{line}");
        kept.push_str(&line);
        kept.push('\n');
    }
    (child.wait().is_ok_and(|s| s.success()), kept)
}

fn quiet(mut c: Command) -> Option<String> {
    let out = c.stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `a | b` — whether both succeeded.
fn piped(a: &Cmd, b: &Cmd, x: &Path) -> bool {
    let mut first = a.command(x);
    let Ok(mut from) = first.stdin(Stdio::null()).stdout(Stdio::piped()).spawn() else {
        return false;
    };
    let mut second = b.command(x);
    let out = from.stdout.take().expect("piped");
    let to = second.stdin(Stdio::from(out)).status();
    // (the pipe's read end let go before waiting: a second that didn't start or ended early mustn't leave the first
    // — gigabytes of `docker save` — blocked on a full pipe)
    drop(second);
    let from = from.wait();
    to.is_ok_and(|s| s.success()) && from.is_ok_and(|s| s.success())
}

fn minutes(d: Duration) -> String {
    format!("{:.1}", d.as_secs_f64() / 60.0)
}

struct Run {
    dir: PathBuf,
    /// The Docker the one build keeps the binary in (`no_kept`'s way).
    build: Docker,
    date: String,
    commit: String,
    rows: Vec<String>,
    failed: Vec<String>,
    /// `--keep-going`: every run of a mode, its failing tests counted.
    keep_going: bool,
    /// Each mode's count of its failing tests, said again at the end (`--keep-going`).
    tallies: Vec<String>,
}

impl Run {
    fn row(&mut self, k: &Kernel, facts: &Facts, mode: &str, expected: &str, rest: [&str; 4]) {
        let [found, passed, compared, mins] = rest;
        self.rows.push(
            [
                self.date.as_str(),
                &self.commit,
                &k.name,
                &facts.get("uname"),
                &facts.get("arch"),
                &facts.get("landlock abi"),
                &facts.get("yama"),
                mode,
                expected,
                found,
                passed,
                compared,
                mins,
            ]
            .join("\t"),
        );
    }

    /// `mode`'s runs on `k`, `repeat` in a row, stopping at the first failure (`--keep-going`: every one, each
    /// failing test counted) — into its file and a row.
    fn mode(
        &mut self,
        k: &Kernel,
        facts: &Facts,
        (label, m): (&str, Mode),
        cmd: &Cmd,
        repeat: u32,
        x: &Path,
    ) {
        let file = self.dir.join(format!("{}-{label}.txt", k.name));
        let mut text = format!("# {} — {}\n{}\n$ {}\n", k.name, k.says, facts.text.trim_end(), cmd.line());
        let started = Instant::now();
        let mut passed = 0;
        let mut seen = Read::default();
        let mut tally = crate::failures::Tally::default();
        let mut none_kept = None;
        for n in 1..=repeat {
            tally.run_starts();
            eprintln!("── {} {label}: run {n} of {repeat}", k.name);
            let t = Instant::now();
            let (ok, out) = teed(cmd.command(x));
            let r = read(&out);
            text.push_str(&format!(
                "\n── run {n} of {repeat} · {} min · {} ──\n{out}",
                minutes(t.elapsed()),
                if ok { "ok" } else { "failed" }
            ));
            // (no binary there: no run of it can pass — said where it is and the way, never as a crash)
            if !ok && out.lines().any(|l| l.trim() == crate::linux::NONE_KEPT) {
                none_kept = Some(no_kept(&docker_of(k), &self.build, Some(&k.name)));
                break;
            }
            for (name, table) in &r.tables {
                let tables = self.dir.join("tables");
                let _ = std::fs::create_dir_all(&tables);
                let _ = std::fs::write(tables.join(format!("{name}.txt")), table);
            }
            seen = Read { found: r.found.or(seen.found), compared: r.compared.or(seen.compared), ..r };
            out.lines().for_each(|l| tally.line(l));
            if !ok {
                tally.run_failed();
                if !self.keep_going {
                    break;
                }
                continue;
            }
            passed += 1;
        }
        if self.keep_going && tally.any_failed() {
            let said = tally.said(repeat).join("\n");
            eprintln!("── {} {label}: {said}", k.name);
            text.push_str(&format!("\n── every run ──\n{said}\n"));
            self.tallies.push(format!("{} {label}: {said}", k.name));
        }
        if let Some(why) = &none_kept {
            eprintln!("── {} {label}: {why}", k.name);
            text.push_str(&format!("\n── {why}\n"));
            self.failed.push(format!("{} {label} ({why})", k.name));
        }
        let _ = std::fs::write(&file, text);
        let passed = match (passed == repeat, self.keep_going) {
            _ if none_kept.is_some() => "no kept binary".into(),
            (true, _) => format!("ok {repeat}/{repeat}"),
            (false, keep_going) => {
                self.failed.push(format!("{} {label}", k.name));
                match keep_going {
                    true => format!("failed {} of {repeat}", repeat - passed),
                    false => format!("failed on run {} of {repeat}", passed + 1),
                }
            }
        };
        let expected = expected(k, m).unwrap_or("-").to_string();
        let found = seen.found.unwrap_or_else(|| "-".into());
        let compared = seen.compared.unwrap_or_else(|| "-".into());
        self.row(k, facts, label, &expected, [&found, &passed, &compared, &minutes(started.elapsed())]);
    }

    fn not_measured(&mut self, k: &Kernel, why: &str) {
        eprintln!("{}: not measured — {why}", k.name);
        self.failed.push(format!("{} not measured ({why})", k.name));
        let facts = Facts::default();
        self.row(k, &facts, "-", "-", ["-", "not measured", why, "-"]);
    }
}

struct Asked {
    dry: bool,
    repeat: Option<u32>,
    keep_going: bool,
    build: bool,
    keep_vm: bool,
    names: Vec<String>,
}

fn ask(args: &[String]) -> Result<Asked> {
    let mut a =
        Asked { dry: false, repeat: None, keep_going: false, build: true, keep_vm: false, names: Vec::new() };
    let mut it = args.iter();
    while let Some(w) = it.next() {
        match w.as_str() {
            "--dry-run" => a.dry = true,
            "--no-build" => a.build = false,
            "--keep-vm" => a.keep_vm = true,
            "--keep-going" => a.keep_going = true,
            "--repeat" => {
                let n = it.next().and_then(|n| n.parse().ok()).filter(|n| *n > 0);
                a.repeat = Some(n.ok_or(format!("--repeat: a count — {USAGE}"))?);
            }
            w if w.starts_with('-') => return Err(format!("{w}? — {USAGE}")),
            name => a.names.push(name.to_string()),
        }
    }
    Ok(a)
}

/// The kernels asked, in the table's order — all of them when none is named.
fn chosen<'a>(t: &'a Table, names: &[String]) -> Result<Vec<&'a Kernel>> {
    if let Some(n) = names.iter().find(|n| !t.kernels.iter().any(|k| &k.name == *n)) {
        let all: Vec<&str> = t.kernels.iter().map(|k| k.name.as_str()).collect();
        return Err(format!("{n}: not in {TABLE} ({})", all.join(", ")));
    }
    Ok(t.kernels.iter().filter(|k| names.is_empty() || names.contains(&k.name)).collect())
}

fn colima_running(profile: &str) -> bool {
    quiet({
        let mut c = Command::new("colima");
        c.args(["status", "-p", profile]);
        c
    })
    .is_some()
}

/// Starts a colima profile unless it's running — whether this run started it.
fn colima_up(profile: &str, start: &[String], x: &Path) -> Result<bool> {
    if colima_running(profile) {
        return Ok(false);
    }
    let c = Cmd::new(&["colima", "start", "-p", profile, "--activate=false"]).args(start);
    eprintln!("$ {}", c.line());
    match c.command(x).status() {
        Ok(s) if s.success() => Ok(true),
        _ => Err(format!("colima start -p {profile} failed")),
    }
}

fn colima_down(profile: &str, x: &Path) {
    let c = Cmd::new(&["colima", "stop", "-p", profile]);
    eprintln!("$ {}", c.line());
    let _ = c.command(x).status();
}

fn said_status(c: &Cmd, x: &Path) -> bool {
    eprintln!("$ {}", c.line());
    c.command(x).status().is_ok_and(|s| s.success())
}

/// What step 0 measures besides time: the image's, the kept binary's and the rustup volume's sizes.
fn sizes(from: &Docker, n: &Needs, x: &Path) -> String {
    let (image, target, rustup) = (n.image.as_str(), n.target.as_str(), n.rustup.as_str());
    let ask = |c: Cmd| quiet(c.on(from).command(x)).unwrap_or_else(|| "not measured".into());
    let image_size = ask(Cmd::new(&["docker", "image", "inspect", image, "--format", "{{.Size}}"]));
    let vol = |v: &str, at: &str| format!("{v}:{at}");
    let kept =
        ask(Cmd::new(&["docker", "run", "--rm", "-v", &vol(target, "/target"), image, "stat", "-c", "%s"])
            .args([crate::linux::KEPT]));
    let rustup =
        ask(Cmd::new(&["docker", "run", "--rm", "-v", &vol(rustup, "/r"), image, "du", "-sb", "/r"]));
    let rustup = rustup.split_whitespace().next().unwrap_or_default().to_string();
    let mb = |s: &str| s.parse::<f64>().map_or(s.to_string(), |b| format!("{:.0} MB", b / 1e6));
    format!(
        "image {image}: {}\nkept test binary: {}\nrustup volume: {}\n",
        mb(&image_size),
        mb(&kept),
        mb(&rustup)
    )
}

fn one_kernel(
    run: &mut Run,
    k: &Kernel,
    repeat: u32,
    from: &Docker,
    (n, sum): (&Needs, &str),
    keep_vm: bool,
    x: &Path,
) {
    let started = Instant::now();
    let docker = docker_of(k);
    let mut stop_colima = None;
    match &k.at {
        At::Colima { profile, start } => {
            match colima_up(profile, start, x) {
                Ok(started) => stop_colima = started.then(|| profile.clone()),
                Err(why) => return run.not_measured(k, &why),
            }
            for s in &k.sysctl {
                if !said_status(
                    &Cmd::new(&["colima", "ssh", "-p", profile, "--", "sudo", "sysctl", "-w", s]),
                    x,
                ) {
                    return run.not_measured(k, &format!("sysctl {s} failed"));
                }
            }
        }
        At::Lima { .. } => {
            let name = vm_name(k);
            let listed = quiet({
                let mut c = Command::new("limactl");
                c.args(["list", "--format", "{{.Name}}"]);
                c
            });
            if listed.is_some_and(|l| l.lines().any(|n| n.trim() == name)) {
                // (only one this tool made: its file's first line says so — a VM of the user's by that name is theirs)
                let file = lima_home().join(&name).join("lima.yaml");
                if !std::fs::read_to_string(&file).is_ok_and(|y| made_here(&y)) {
                    return run.not_measured(
                        k,
                        &format!("a Lima VM named {name} that `cargo xtask kernels` didn't make — left as it is: rename or delete it"),
                    );
                }
                eprintln!("{name}: a VM a stopped run left — deleted first");
                said_status(&Cmd::new(&["limactl", "delete", "--force", &name]), x);
            }
            let root = root();
            let git = crate::linux::common_git_dir(&root);
            let mounts: Vec<&Path> = std::iter::once(root.as_path()).chain(git.as_deref()).collect();
            let yaml = match lima_yaml(k, &mounts) {
                Ok(y) => y,
                Err(why) => return run.not_measured(k, &why),
            };
            let file = run.dir.join(format!("{name}.yaml"));
            if let Err(e) = std::fs::write(&file, yaml) {
                return run.not_measured(k, &format!("{}: {e}", file.display()));
            }
            let up = Cmd::new(&["limactl", "start", &format!("--name={name}"), "--tty=false"])
                .args([file.to_string_lossy()]);
            eprintln!("$ {}", up.line());
            let (came_up, said) = teed(up.command(x));
            let start_file = format!("{name}-start.txt");
            let _ = std::fs::write(run.dir.join(&start_file), &said);
            if !came_up {
                lima_down(k, keep_vm, x);
                let why = match download_refused(&said) {
                    Some((url, status)) => refused_why(&url, &status, asked_again(&url).as_deref()),
                    None => format!("its VM didn't come up (limactl start, its output in {start_file})"),
                };
                return run.not_measured(k, &why);
            }
        }
    }
    let down = |x: &Path| match &k.at {
        At::Colima { .. } => stop_colima.iter().for_each(|p| colima_down(p, x)),
        At::Lima { .. } => lima_down(k, keep_vm, x),
    };
    // (the build's binary, in a Docker of its own: a Lima VM's, another colima profile's — Y2's)
    if let Some(to) = carried_into(k, from)
        && let Err(why) = carry_in(from, &to, n, sum, x)
    {
        down(x);
        return run.not_measured(k, &why);
    }
    let mut facts = Facts::default();
    for c in [facts_cmd(&docker, &n.image), vm_shell(k, VM_FACTS)] {
        eprintln!("$ {}", c.line());
        facts.text.push_str(&quiet(c.command(x)).unwrap_or_else(|| format!("(not read: {})", c.line())));
        facts.text.push('\n');
    }
    eprint!("{}", facts.text);
    let _ = std::fs::write(run.dir.join(format!("{}-facts.txt", k.name)), &facts.text);
    // (#280: past 90 % its runs would fail by the thousand with ENOSPC — not measured, said before any runs)
    if let Err(why) = room(k, &facts) {
        down(x);
        return run.not_measured(k, &why);
    }
    for m in &k.modes {
        run.mode(k, &facts, (&m.name(), *m), &suite_cmd(k, *m, Some(FILTER), &docker), repeat, x);
    }
    let whole = format!("whole-{}", k.whole.name());
    run.mode(k, &facts, (&whole, k.whole), &suite_cmd(k, k.whole, None, &docker), repeat, x);
    down(x);
    eprintln!("{}: {} min", k.name, minutes(started.elapsed()));
}

/// Err: the kernel's Docker disk (its facts' `disk`) past `linux::DISK_STOP`, said with what frees it — colima's
/// profile by `cargo xtask linux prune` on its Docker; a Lima VM's disk is made for the run, holding what was carried
/// in. A disk not read: Ok, its runs say the rest.
fn room(k: &Kernel, facts: &Facts) -> std::result::Result<(), String> {
    let Some(full) = crate::linux::disk(&facts.get("disk")).and_then(crate::linux::too_full) else {
        return Ok(());
    };
    let frees = match &k.at {
        At::Colima { profile, .. } => {
            format!("`DOCKER_CONTEXT={} cargo xtask linux prune` frees it", colima_context(profile))
        }
        At::Lima { .. } => {
            "its VM's disk is made for the run (`disk:` in its file, `lima_yaml`): a bigger one".into()
        }
    };
    Err(format!("its Docker: {full} — {frees}"))
}

/// The image's address and the status a refused download got, from what `limactl start` said: Lima's downloader
/// says `failed to download <url>: unexpected HTTP status 403 Forbidden, body=…` on one line (the address quoted
/// as Go's `%#q` quotes it, inside a log line's quotes).
fn download_refused(said: &str) -> Option<(String, String)> {
    said.lines().find_map(|l| {
        let rest = &l[l.find("failed to download ")?..];
        let url = rest[rest.find("https://")?..].split(|c: char| "`\"\\' ".contains(c)).next()?;
        let status = l.split("unexpected HTTP status ").nth(1)?.split(", body=").next()?.trim();
        Some((url.to_string(), status.to_string()))
    })
}

/// Who answers `url` now as Lima's download asks it — redirects followed, one byte: curl's `<status> <redirects>
/// <last address>`, or None where curl couldn't ask. A redirector's mirror is only seen here: Lima names the
/// address it was given.
fn asked_again(url: &str) -> Option<String> {
    let mut c = Command::new("curl");
    c.args(["-sS", "-o", "/dev/null", "-r", "0-0", "-L", "--max-time", "60"]).args([
        "-w",
        "%{http_code} %{num_redirects} %{url_effective}",
        url,
    ]);
    quiet(c)
}

/// A refused download's reason, one line for the row: the status Lima got, the host that refused (asked again: the
/// one a redirect sent it to), the address.
fn refused_why(url: &str, status: &str, again: Option<&str>) -> String {
    let from = host(url);
    let again = again.and_then(|a| {
        let mut w = a.split_whitespace();
        Some((w.next()?, w.next()?.parse::<u32>().ok()?, w.next()?))
    });
    let who = match again {
        Some((code, n, to)) if n > 0 => {
            format!("{from} sent it to {}, which answers {code} (asked again)", host(to))
        }
        Some((code, _, _)) => format!("{from}, which answers {code} (asked again)"),
        None => format!("{from} or a host it sent it to (not asked again: curl)"),
    };
    let why = format!("its image's download refused, {status}: {who} — {url}");
    why.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

fn lima_down(k: &Kernel, keep_vm: bool, x: &Path) {
    let name = vm_name(k);
    if keep_vm {
        eprintln!("--keep-vm: {name} left as it is (`limactl delete --force {name}` after)");
        return;
    }
    said_status(&Cmd::new(&["limactl", "stop", &name]), x);
    said_status(&Cmd::new(&["limactl", "delete", "--force", &name]), x);
}

pub fn run(args: &[String]) -> Result<()> {
    let a = ask(args)?;
    let root = root();
    let path = root.join(TABLE);
    let t = table(&std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?)?;
    let kernels = chosen(&t, &a.names)?;
    let repeat = a.repeat.unwrap_or(t.repeat);
    let toolchain = root.join("rust-toolchain.toml");
    let pin = crate::linux::pin(
        &std::fs::read_to_string(&toolchain).map_err(|e| format!("{}: {e}", toolchain.display()))?,
    )?;
    let machine = crate::linux::machine_id(&root).ok();
    let n = Needs {
        image: crate::linux::image(&pin),
        target: crate::linux::target_volume(&root, &pin, None),
        make: crate::linux::create_args(&root, machine.as_deref(), &pin, None),
        rustup: format!("tarae-rustup-{pin}"),
    };
    let from = Docker::Context(colima_context(&t.build));
    let x = std::env::current_exe().map_err(|e| format!("this xtask: {e}"))?;
    if a.dry {
        println!(
            "# the one build, in colima's {} (started if it isn't, stopped after if this run started it)",
            t.build
        );
        if a.build {
            println!("{}", build_cmd(&from).line());
        }
        println!(
            "{}    # the kept binary's sha256 — none: not run, said with the way",
            kept_sum(&from, &n).line()
        );
        if a.keep_going {
            println!(
                "# --keep-going: every run of each mode, each failing test said once with the runs it failed in"
            );
        }
        let git = crate::linux::common_git_dir(&root);
        let mounts: Vec<&Path> = std::iter::once(root.as_path()).chain(git.as_deref()).collect();
        for k in &kernels {
            println!();
            for l in plan(k, repeat, &from, &n) {
                println!("{l}");
            }
            if matches!(k.at, At::Lima { .. }) {
                println!("# {}.yaml:\n{}", vm_name(k), lima_yaml(k, &mounts)?);
            }
        }
        println!("(`xtask` is `cargo xtask`)");
        return Ok(());
    }
    let (date, commit) = (crate::perf::today(), crate::perf::commit(&root));
    let dir = root.join(OUT).join(format!("{date}-{commit}"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut run = Run {
        dir: dir.clone(),
        build: from.clone(),
        date,
        commit,
        rows: Vec::new(),
        failed: Vec::new(),
        keep_going: a.keep_going,
        tallies: Vec::new(),
    };
    let started = Instant::now();
    let build_profile = t.build.clone();
    let stop_build = colima_up(&build_profile, &[], &x)?;
    if a.build {
        let t0 = Instant::now();
        let c = build_cmd(&from);
        eprintln!("$ {}", c.line());
        let (ok, out) = teed(c.command(&x));
        let _ = std::fs::write(dir.join("build.txt"), format!("$ {}\n{out}", c.line()));
        if !ok {
            return Err(format!("the one build failed (above; {})", dir.join("build.txt").display()));
        }
        eprintln!("built in {} min", minutes(t0.elapsed()));
    }
    // (the binary every kernel runs, by its sha256: none, nothing runs — said with the way, never a crash)
    let c = kept_sum(&from, &n);
    eprintln!("$ {}", c.line());
    let said = quiet(c.command(&x)).unwrap_or_default();
    let Some(sum) = sum_of(&said) else {
        if stop_build {
            colima_down(&build_profile, &x);
        }
        return Err(no_kept(&from, &from, None));
    };
    let sized = sizes(&from, &n, &x);
    eprint!("{sized}");
    let _ = std::fs::write(dir.join("sizes.txt"), format!("{sized}kept test binary's sha256: {sum}\n"));
    for k in &kernels {
        // (the build's own profile stays up for the carrying; a colima kernel on it isn't stopped by its run)
        one_kernel(&mut run, k, repeat, &from, (&n, sum), a.keep_vm, &x);
    }
    if stop_build {
        colima_down(&build_profile, &x);
    }
    let history = root.join(OUT).join("history.tsv");
    let mut kept = std::fs::read_to_string(&history).unwrap_or_else(|_| HISTORY_HEAD.into());
    for r in &run.rows {
        kept.push_str(r);
        kept.push('\n');
    }
    std::fs::write(&history, kept).map_err(|e| format!("{}: {e}", history.display()))?;
    let summary: String = run.rows.iter().map(|r| format!("{r}\n")).collect();
    let _ = std::fs::write(dir.join("summary.tsv"), format!("{HISTORY_HEAD}{summary}"));
    let _ = std::io::stdout().write_all(format!("\n{HISTORY_HEAD}{summary}").as_bytes());
    // (`--keep-going`: every mode's failing tests, at the end, where a long run is read)
    for t in &run.tallies {
        println!("\n{t}");
    }
    if !run.tallies.is_empty() {
        let _ = std::fs::write(dir.join("failures.txt"), run.tallies.join("\n\n") + "\n");
    }
    println!(
        "{} min in all — written to {}, a row each in {}",
        minutes(started.elapsed()),
        dir.display(),
        history.display()
    );
    match run.failed.is_empty() {
        true => Ok(()),
        false => Err(format!("{} failed or not measured: {}", run.failed.len(), run.failed.join(", "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn the_table() -> Table {
        table(&std::fs::read_to_string(root().join(TABLE)).unwrap()).unwrap()
    }

    /// What the runs need, by stand-in names.
    fn needs() -> Needs {
        Needs {
            image: "rust:1.95".into(),
            target: "tv".into(),
            make: ["volume", "create", "--label", "tarae.checkout=/w", "tv"].map(String::from).into(),
            rustup: "rv".into(),
        }
    }

    /// The set as the plan decided it (K1–K6 and Y2), each kernel read whole: modes, expectations among them, a VM.
    #[test]
    fn the_table_is_the_plans_set_and_reads_whole() {
        let t = the_table();
        let names: Vec<&str> = t.kernels.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(names, ["K4", "K3", "K1", "K2", "K5", "K6", "Y2"], "K4 first: step 0's, colima's own");
        assert_eq!((t.build.as_str(), t.repeat), ("tarae-test", 3), "three in a row");
        let k4 = &t.kernels[0];
        assert_eq!(k4.at, At::Colima { profile: "tarae-test".into(), start: vec![] }, "K4 needs no download");
        for k in &t.kernels {
            assert!(k.expect.keys().all(|m| k.modes.iter().any(|x| &x.name() == m) || k.whole.name() == *m));
        }
        let y2 = t.kernels.iter().find(|k| k.name == "Y2").unwrap();
        assert!(y2.modes.iter().all(|m| m.user), "Y2 as a user: root's CAP_SYS_PTRACE passes Yama");
        assert!(y2.sysctl.contains(&"kernel.yama.ptrace_scope=2".to_string()));
        let k5 = t.kernels.iter().find(|k| k.name == "K5").unwrap();
        assert!(k5.sysctl.contains(&"kernel.yama.ptrace_scope=1".to_string()), "the snapshots are Yama 1's");
        assert_eq!(k5.modes.len(), 6, "every table on the kernel that compares");
        let k6 = t.kernels.iter().find(|k| k.name == "K6").unwrap();
        assert!(
            k6.sysctl.contains(&"kernel.yama.ptrace_scope=1".to_string()),
            "Fedora's 0 isn't the snapshots'"
        );
        assert!(k6.modes.iter().all(|m| k6.expect.contains_key(&m.name())), "every mode of K6's expected");
    }

    #[test]
    fn a_table_that_says_wrong_is_refused_with_why() {
        let head = "build = \"tarae-test\"\n[[kernel]]\nname = \"K\"\ncolima = \"p\"\nwhole = \"host\"\n";
        let bad = [
            ("modes = [\"guest\"]", "own, host or offline"),
            ("modes = []", "no modes"),
            ("modes = [\"host\"]\nexpect = { own = \"own\" }", "not one of its modes"),
            ("modes = [\"host\"]\nexpect = { host = \"mine\" }", "own, host or offline"),
            ("modes = [\"host\"]\nsysctl = [\"a=b; reboot\"]", "name=value"),
            ("modes = [\"host\"]\nimage = \"debian-12\"", "one of colima"),
            ("modes = [\"host\"]\ndownload = { location = \"https://a/b\" }", "with image, not colima"),
        ];
        // (a Lima row's download, each part checked)
        let lima = "build = \"tarae-test\"\n[[kernel]]\nname = \"K\"\nimage = \"i\"\ninstall = \"x\"\nwhole = \"host\"\nmodes = [\"host\"]\n";
        let digest = format!("sha256:{}", "a".repeat(64));
        let bad_download = [
            (format!("location = \"http://h/i\", arch = \"aarch64\", digest = \"{digest}\""), "an https://"),
            (
                format!("location = \"https://h/i i\", arch = \"aarch64\", digest = \"{digest}\""),
                "an https://",
            ),
            (
                format!("location = \"https://h/i\", arch = \"arm\", digest = \"{digest}\""),
                "aarch64 or x86_64",
            ),
            ("location = \"https://h/i\", arch = \"aarch64\", digest = \"sha256:ab\"".into(), "64 lowercase"),
            ("location = \"https://h/i\", arch = \"aarch64\"".into(), "no digest"),
            (
                format!("location = \"https://h/i\", arch = \"aarch64\", digest = \"{digest}\", sha = 1"),
                "only location",
            ),
        ];
        for (d, why) in bad_download {
            let got = table(&format!("{lima}download = {{ {d} }}\n"));
            assert!(got.as_ref().is_err_and(|e| e.contains(why)), "{d}: {:?}", got.err());
        }
        let good = format!(
            "{lima}download = {{ location = \"https://h/i\", arch = \"x86_64\", digest = \"{digest}\" }}\n"
        );
        assert!(table(&good).is_ok(), "{:?}", table(&good).err());
        for (rest, why) in bad {
            let got = table(&format!("{head}{rest}\n"));
            assert!(got.as_ref().is_err_and(|e| e.contains(why)), "{rest}: {:?}", got.err());
        }
        let twice = format!(
            "{head}modes = [\"host\"]\n[[kernel]]\nname = \"K\"\ncolima = \"p\"\nwhole = \"host\"\nmodes = [\"host\"]\n"
        );
        assert!(table(&twice).is_err_and(|e| e.contains("K twice")));
    }

    /// A mode is `cargo xtask linux`'s options for its container, the kept binary as root or a user, the tier
    /// expected only where the table says one.
    #[test]
    fn each_mode_is_the_kept_binary_under_its_options() {
        let t = the_table();
        let y2 = t.kernels.iter().find(|k| k.name == "Y2").unwrap();
        let d = Docker::Context("colima-tarae-yama2".into());
        let own = suite_cmd(y2, Mode::parse("own-user").unwrap(), Some(FILTER), &d);
        assert_eq!(
            own.line(),
            "DOCKER_CONTEXT=colima-tarae-yama2 xtask linux --env TARAE_EXPECT_YAMA=2 --env TARAE_EXPECT_TIER=own \
             --userns test --kept --as-user sandbox::"
        );
        // (K2: nothing in its table, its ABI in its `env`)
        let k2 = t.kernels.iter().find(|k| k.name == "K2").unwrap();
        let h = Docker::Host("unix:///l/tarae-k2/sock/docker.sock".into());
        let off = suite_cmd(k2, Mode::parse("offline").unwrap(), None, &h);
        assert_eq!(
            off.line(),
            "DOCKER_HOST=unix:///l/tarae-k2/sock/docker.sock xtask linux --env TARAE_EXPECT_LANDLOCK_ABI=1 \
             --env TARAE_EXPECT_TIER=offline --docker-default test --kept",
            "offline expected as `--docker-default` expects it, said here as in its history row; the whole suite"
        );
        assert_eq!(expected(k2, Mode::parse("offline-user").unwrap()), Some("offline"));
        assert_eq!(expected(k2, Mode::parse("host").unwrap()), None, "nothing in the table: read");
        let k1 = t.kernels.iter().find(|k| k.name == "K1").unwrap();
        assert_eq!(expected(k1, Mode::parse("own-user").unwrap()), Some("own"), "its first run's, as found");
        let k4 = t.kernels.iter().find(|k| k.name == "K4").unwrap();
        assert_eq!(expected(k4, Mode::parse("own").unwrap()), Some("host"), "the table's");
        let mut c = off.command(Path::new("/x/xtask"));
        let envs: Vec<_> = c.get_envs().map(|(k, v)| (k.to_owned(), v.map(|v| v.to_owned()))).collect();
        assert!(envs.contains(&("DOCKER_CONTEXT".into(), None)), "one Docker named: {envs:?}");
        assert_eq!(c.get_program(), "/x/xtask");
        let _ = c.stdin(Stdio::null());
        for m in ["own", "own-user", "host", "host-user", "offline", "offline-user"] {
            assert_eq!(Mode::parse(m).unwrap().name(), m);
        }
    }

    /// A Lima VM: Lima's own template for the image, only the checkout and its git folder mounted and read-only,
    /// no containerd of Lima's, Docker installed as the table says with its socket forwarded, the sysctls set.
    #[test]
    fn a_lima_vm_mounts_only_the_checkout_read_only_and_runs_docker() {
        let t = the_table();
        let k5 = t.kernels.iter().find(|k| k.name == "K5").unwrap();
        let y = lima_yaml(k5, &[Path::new("/u/w/ci"), Path::new("/u/w/tarae/.git")]).unwrap();
        // (review note 39: its file says it's this tool's — a VM of the user's by the name is never deleted)
        assert!(made_here(&y), "{y}");
        assert!(!made_here("minimumLimaVersion: 2.0.0\nbase:\n- template:_images/debian-13\n"));
        assert!(y.contains("base:\n- template:_images/debian-13\n"), "{y}");
        assert!(
            y.contains("mounts:\n- location: \"/u/w/ci\"\n  writable: false\n- location: \"/u/w/tarae/.git\"\n  writable: false\n"),
            "{y}"
        );
        assert!(y.contains("containerd:\n  system: false\n  user: false\n"), "{y}");
        assert!(y.contains("    sysctl -w kernel.yama.ptrace_scope=1\n"), "{y}");
        assert!(y.contains("      curl -fsSL https://get.docker.com | sh\n"), "{y}");
        assert!(
            y.contains("SocketUser=%s") && y.contains("hostSocket: \"{{.Dir}}/sock/docker.sock\""),
            "{y}"
        );
        // (each line of the provision's script inside its block, as YAML reads it)
        let script = y.split("  script: |\n").nth(1).unwrap().split("probes:").next().unwrap();
        assert!(script.lines().all(|l| l.starts_with("    ")), "{script}");
        assert!(lima_yaml(k5, &[Path::new("/a\"b")]).is_err());
        let k4 = &t.kernels[0];
        assert!(lima_yaml(k4, &[]).is_err(), "colima's isn't Lima's");
    }

    /// K6's image comes from the place its row names — Fedora's master, no redirector (its first run's mirror refused
    /// Lima, 403) — with the digest Lima checks, and no template whose images Lima would try after it; a row naming
    /// none keeps its template. A refused download's reason names the status and the host that refused.
    #[test]
    fn k6s_image_comes_from_fedoras_master_with_its_digest() {
        let t = the_table();
        let k6 = t.kernels.iter().find(|k| k.name == "K6").unwrap();
        let y = lima_yaml(k6, &[Path::new("/u/w/ci")]).unwrap();
        assert!(
            y.contains(
                "images:\n- location: \"https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Cloud/aarch64/images/Fedora-Cloud-Base-Generic-44-1.7.aarch64.qcow2\"\n  arch: aarch64\n  digest: \"sha256:55c60a3b80d3616a08705afd0459e75fe9f03c54aba7a46e4002a41a72fa0d5b\"\n"
            ),
            "{y}"
        );
        assert!(!y.contains("template:") && !y.contains("download.fedoraproject.org"), "{y}");
        assert!(made_here(&y) && y.contains("\nvmType: vz\n"), "{y}");
        let k3 = t.kernels.iter().find(|k| k.name == "K3").unwrap();
        let y3 = lima_yaml(k3, &[]).unwrap();
        assert!(y3.contains("base:\n- template:_images/debian-12\n") && !y3.contains("images:"), "{y3}");
        let from = Docker::Context("colima-tarae-test".into());
        let all = plan(k6, 3, &from, &needs()).join("\n");
        assert!(
            all.contains("# downloads fedora-44 from dl.fedoraproject.org (its digest checked) once"),
            "{all}"
        );

        // (Lima's line for a refused download, as its log quotes it, and the host a redirect sent it to)
        let url =
            "https://download.fedoraproject.org/pub/fedora/linux/releases/44/Cloud/aarch64/images/F.qcow2";
        let said = format!(
            "time=\"2026-11-03T14:53:10+09:00\" level=fatal msg=\"failed to download `{url}`: unexpected HTTP status 403 Forbidden, body=`<html>uncommon characteristics</html>`\"\n"
        );
        let (got, status) = download_refused(&format!("downloading\n{said}")).unwrap();
        assert_eq!((got.as_str(), status.as_str()), (url, "403 Forbidden"));
        let escaped = said.replace('`', "\\\"");
        assert_eq!(download_refused(&escaped).map(|(u, _)| u).as_deref(), Some(url), "{escaped}");
        assert_eq!(download_refused("level=fatal msg=\"exiting, status={Running:false}\""), None);
        let why = refused_why(
            url,
            "403 Forbidden",
            Some("403 1 https://mirrors.tuna.tsinghua.edu.cn/fedora/F.qcow2"),
        );
        assert_eq!(
            why,
            format!(
                "its image's download refused, 403 Forbidden: download.fedoraproject.org sent it to mirrors.tuna.tsinghua.edu.cn, which answers 403 (asked again) — {url}"
            )
        );
        let why = refused_why(url, "403 Forbidden", None);
        assert!(why.contains("403 Forbidden: download.fedoraproject.org or a host it sent it to"), "{why}");
        assert!(
            refused_why(url, "404 Not\tFound", Some("404 0 x"))
                .contains("404 Not Found: download.fedoraproject.org, which answers 404")
        );
    }

    /// What goes into a VM comes from colima's Docker with no download: the image, the kept binary, rustup.
    #[test]
    fn the_image_binary_and_toolchain_are_carried_not_fetched() {
        let (from, to) = (Docker::Context("colima-tarae-test".into()), Docker::Host("unix:///s".into()));
        let c = carry(&from, &to, &needs());
        let lines: Vec<String> =
            (c.iter()).map(|(a, b)| b.iter().fold(a.line(), |l, b| format!("{l} | {}", b.line()))).collect();
        assert_eq!(
            lines[0],
            "DOCKER_CONTEXT=colima-tarae-test docker save rust:1.95 | DOCKER_HOST=unix:///s docker load"
        );
        assert_eq!(lines[1], "DOCKER_HOST=unix:///s docker volume create --label tarae.checkout=/w tv");
        assert!(
            lines[2].contains(
                "cat /target/kept/tarae-tests | DOCKER_HOST=unix:///s docker run --rm -i -v tv:/target"
            ),
            "{}",
            lines[2]
        );
        assert!(lines[3].contains("rv:/r rust:1.95 tar -C /r -cf - ."), "{}", lines[3]);
        assert!(lines.iter().all(|l| !l.contains("pull")));
    }

    /// Y2's Docker is colima's `tarae-yama2`, not the build's `tarae-test`: the kept binary is carried there from the
    /// build's as into a Lima VM's — its volume made labelled, then its sha256 checked — after its VM is up and set,
    /// before any run; K4, on the build's own, carries nothing. A Docker with none is said by its name, with the way.
    #[test]
    fn a_colima_row_off_the_builds_docker_gets_the_kept_binary_carried() {
        let t = the_table();
        let from = Docker::Context(colima_context(&t.build));
        let row = |name: &str| t.kernels.iter().find(|k| k.name == name).unwrap();
        let yama2 = Docker::Context("colima-tarae-yama2".into());
        assert_eq!(carried_into(row("Y2"), &from), Some(yama2.clone()));
        assert_eq!(carried_into(row("K4"), &from), None, "the build's own Docker");
        assert!(matches!(carried_into(row("K3"), &from), Some(Docker::Host(_))), "a Lima VM's, as before");

        let l = plan(row("Y2"), 3, &from, &needs());
        let at = |s: &str| l.iter().position(|x| x.contains(s)).unwrap_or_else(|| panic!("{s}: {l:#?}"));
        let binary = at(
            "DOCKER_CONTEXT=colima-tarae-test docker run --rm -v tv:/target rust:1.95 cat /target/kept/tarae-tests \
             | DOCKER_CONTEXT=colima-tarae-yama2 docker run --rm -i -v tv:/target",
        );
        let made = at("DOCKER_CONTEXT=colima-tarae-yama2 docker volume create --label tarae.checkout=/w tv");
        let sum = at("DOCKER_CONTEXT=colima-tarae-yama2 docker run --rm -v tv:/target rust:1.95 sha256sum \
             /target/kept/tarae-tests    # the build's sha256");
        let up = at("sysctl -w kernel.apparmor_restrict_unprivileged_userns=0");
        let first = at("test --kept");
        assert!(up < made && made < binary && binary < sum && sum < first, "{l:#?}");
        assert!(l[first].starts_with("DOCKER_CONTEXT=colima-tarae-yama2 xtask linux"), "{}", l[first]);
        let k4 = plan(row("K4"), 3, &from, &needs()).join("\n");
        assert!(!k4.contains("sha256sum") && !k4.contains("docker load"), "{k4}");

        assert_eq!(
            no_kept(&yama2, &from, Some("Y2")),
            "no kept binary in tarae-yama2's Docker — it's carried there from tarae-test's before Y2's runs, its \
             sha256 checked: `cargo xtask kernels --no-build Y2` carries it again"
        );
        assert_eq!(
            no_kept(&from, &from, Some("K4")),
            "no kept binary in tarae-test's Docker — `cargo xtask kernels` keeps it there (not with `--no-build`), \
             or `DOCKER_CONTEXT=colima-tarae-test cargo xtask linux test --keep --bin tarae`"
        );
        assert_eq!(Docker::Host("unix:///l/tarae-k3/sock/docker.sock".into()).name(), "tarae-k3");
        assert_eq!(Docker::Context("colima".into()).name(), "default");
        let h = "0123456789abcdef".repeat(4);
        assert_eq!(sum_of(&format!("{h}  /target/kept/tarae-tests\n")), Some(h.as_str()));
        assert_eq!(sum_of("sha256sum: /target/kept/tarae-tests: No such file or directory"), None);
    }

    /// The suite's own words read back: the tier the probe found, compared or why not, and a table it printed
    /// for a snapshot it had none of, by that snapshot's name.
    #[test]
    fn a_runs_output_says_its_tier_comparison_and_tables() {
        let out = "tier here: own (user net ipc pid mnt)\n\
                   test sandbox::refused::must_be_refused ... ok\n";
        let r = read(out);
        assert_eq!((r.found.as_deref(), r.compared.as_deref()), (Some("own"), Some("compared")));
        let out = format!(
            "tier here: host (none) — why\n{UNCOMPARED}aarch64: the snapshots are x86-64's\ntest sandbox::refused::must_be_refused ... ok\n"
        );
        assert_eq!(read(&out).compared.as_deref(), Some("not compared: aarch64: the snapshots are x86-64's"));
        let out = "thread 'x' panicked at src/sandbox/refused.rs:1433:9:\n\
                   no snapshot /work/tarae/src/snapshots/refused_linux_host-aarch64.txt yet — create it\n\
                   \n\
                   # must-be-refused suite (Linux host)\n\
                   # claim | id | verdict | witness\n\
                   §1.1     | read-key                       | refused  | EACCES\n\
                   \n\
                   test sandbox::refused::must_be_refused ... FAILED\n";
        let r = read(out);
        assert_eq!(r.compared.as_deref(), Some("failed"));
        assert_eq!(
            r.tables.get("refused_linux_host-aarch64").map(String::as_str),
            Some(
                "# must-be-refused suite (Linux host)\n# claim | id | verdict | witness\n\
                 §1.1     | read-key                       | refused  | EACCES\n"
            )
        );
        let out = "snapshot refused_linux_own-aarch64 differs (/w/x.txt) — if intended\n- a\n+ b\n\nnow:\n\
                   # must-be-refused suite (Linux own)\n# claim | id | verdict | witness\nx | y | z | w\n";
        assert!(read(out).tables.contains_key("refused_linux_own-aarch64"));
        // (review note 37: aarch64's with no snapshot yet, said and not compared — its table kept all the same)
        let out = format!(
            "{UNCOMPARED}no snapshot /w/src/snapshots/refused_linux_host-aarch64.txt yet — why\n\
             # must-be-refused suite (Linux host)\n# claim | id | verdict | witness\n§1.1 | a | refused | w\n\
             test sandbox::refused::must_be_refused ... ok\n"
        );
        let r = read(&out);
        assert!(
            r.compared.as_deref().is_some_and(|c| c.starts_with("not compared: no snapshot")),
            "{:?}",
            r.compared
        );
        assert!(
            r.tables.get("refused_linux_host-aarch64").is_some_and(|t| t.contains("§1.1 | a")),
            "{:?}",
            r.tables
        );
        assert!(
            read("# must-be-refused suite (Linux host)\n").tables.is_empty(),
            "a table with no snapshot asked"
        );
    }

    #[test]
    fn the_kernels_asked_are_the_tables_in_its_order() {
        let t = the_table();
        let got: Vec<&str> =
            chosen(&t, &["Y2".into(), "K4".into()]).unwrap().iter().map(|k| k.name.as_str()).collect();
        assert_eq!(got, ["K4", "Y2"]);
        assert!(chosen(&t, &["K9".into()]).is_err_and(|e| e.contains("K9: not in")));
        let a = ask(&["--dry-run".into(), "--repeat".into(), "1".into(), "K4".into()]).unwrap();
        assert!(a.dry && a.repeat == Some(1) && a.build && !a.keep_going && a.names == ["K4"]);
        let a = ask(&["--repeat".into(), "5".into(), "--keep-going".into(), "K2".into()]).unwrap();
        assert!(a.repeat == Some(5) && a.keep_going && a.names == ["K2"], "every run of five");
        assert!(ask(&["--repeat".into(), "0".into()]).is_err());
    }

    /// K4's plan, `--dry-run`'s lines: colima's VM only, no Lima, no download — the facts, each mode, the whole.
    #[test]
    fn k4s_plan_starts_nothing_new_and_downloads_nothing() {
        let t = the_table();
        let from = Docker::Context("colima-tarae-test".into());
        let l = plan(&t.kernels[0], 3, &from, &needs());
        let all = l.join("\n");
        assert!(!all.contains("limactl") && !all.contains("docker load"), "{all}");
        assert_eq!(
            l.iter().filter(|l| l.contains("test --kept")).count(),
            7,
            "six modes and the whole: {all}"
        );
        assert!(
            all.contains("colima start -p tarae-test --activate=false    # only if it isn't running"),
            "{all}"
        );
        let k3 = t.kernels.iter().find(|k| k.name == "K3").unwrap();
        let all = plan(k3, 3, &from, &needs()).join("\n");
        assert!(
            all.contains("limactl start --name=tarae-k3") && all.contains("limactl delete --force tarae-k3"),
            "{all}"
        );
    }

    /// #280: a kernel whose Docker disk is past 90 % (its facts' `disk`) isn't measured — said before its runs, with
    /// what frees it for colima's profile or the Lima VM; under it, or not read, its runs go on.
    #[test]
    fn a_kernels_disk_past_90_percent_isnt_measured() {
        let t = the_table();
        let k3 = t.kernels.iter().find(|k| k.name == "K3").unwrap();
        let at = |pct| Facts {
            // (`df -P /`'s row as colima's tarae-test printed it, 2026-11-03)
            text: format!(
                "uname: 6.1.0\ndisk: overlay           82380556 37197472  41601788      {pct}% /\n"
            ),
        };
        assert_eq!(room(&t.kernels[0], &at(89)), Ok(()));
        assert_eq!(room(&t.kernels[0], &Facts::default()), Ok(()), "not read");
        assert_eq!(
            room(&t.kernels[0], &at(94)),
            Err("its Docker: the disk 94 % used (38 of 84 GB), past 90 % — \
                 `DOCKER_CONTEXT=colima-tarae-test cargo xtask linux prune` frees it"
                .into())
        );
        assert!(room(k3, &at(90)).unwrap_err().contains("`lima_yaml`): a bigger one"));
        assert!(FACTS.contains("echo \"disk: $(df -P / | sed -n 2p)\""));
    }
}
