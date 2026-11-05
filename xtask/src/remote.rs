//! `cargo xtask remote [--dry-run] [--keep-vm] [--keys N] [--cuts N]` — step 5 measured over a real ssh (the remote
//! plan's §8d, S-J): this Mac's `tarae attach ssh://…` to a host in a Lima VM, through the VM's sshd. Run by you, in a
//! terminal of your own: Lima keeps its VMs and its ssh key in your home, and `tarae attach` refuses to run under a run
//! of Claude's (`TARAE_RUN`, S-E) — from one, everything up to the attach runs and the attach is said not measured.
//!
//! One Lima VM, made as `cargo xtask kernels` makes its own (Lima's template for Debian 12, `vz`, the checkout and its
//! git folder mounted read-only, nothing else of home), deleted after (`--keep-vm` leaves it; one a stopped run left is
//! deleted first, only one this tool made). In it: the build's tools and rustup, tarae built from this tree (`shot`
//! profile: the toolchain `rust-toolchain.toml` pins, into `/var/tmp`), a host (`tarae --host`) on a file of 2,000
//! lines. Here: tarae built (`shot`), then `tarae attach --stats` on a pty of its own, its ssh Lima's `ssh.config`
//! with no shared connection (a full handshake each time) — put first on its `PATH` as `ssh`, nothing of yours
//! changed. Driven: `--keys` keys (`j`, 150 ms apart), `--cuts` links cut (its ssh killed) each to a live frame, one
//! link frozen (its ssh stopped) and a focus back (the `Ping` unanswered for 3 s, then cut), more keys, `:detach`.
//!
//! What it says is the client's own measure (`--stats`): the first frame from its start, a key to its frame (median,
//! p90, bytes), each way back, the link's end to the new link's first frame presented. Results in
//! `.sb/remote/<date>-<commit>.txt` (the setup's output, the client's), a row in `.sb/remote/history.tsv`. A VM that
//! didn't come up, a build that failed, a host that didn't start: not measured, never a pass.

use std::io::{Read as _, Write as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::kernels::{lima_home, minutes, quiet, teed, when};
use crate::{Result, root};

const USAGE: &str = "usage: cargo xtask remote [--dry-run] [--keep-vm] [--keys N] [--cuts N]";
const VM: &str = "tarae-remote";
/// The first words of the VM's file this tool makes — what tells its VM from a user's by the name.
const MADE: &str = "# made by `cargo xtask remote`";
const OUT: &str = ".sb/remote";
const HISTORY_HEAD: &str =
    "date\tcommit\tlink\tfirst_ms\tkeys\tmedian_ms\tp90_ms\tbytes_a_key\tcut_back_ms\tfrozen_back_ms\n";
/// Where the VM builds tarae: its own disk, the checkout being read-only.
const TARGET: &str = "/var/tmp/tarae-target";
/// The host's folder in the VM, its home's.
const PROJECT: &str = "tarae-remote";
/// What the client's lines start with (`src/client/stats.rs`'s `SAID`).
const SAID: &str = "tarae attach --stats:";
/// The client's screen.
const COLS: u16 = 120;
const ROWS: u16 = 40;
/// Between two keys: each frame in before the next, as a person types.
const KEY_GAP: Duration = Duration::from_millis(150);
/// A wait that never comes by this is said, the run not measured.
const PATIENCE: Duration = Duration::from_secs(60);

#[derive(Debug, PartialEq)]
struct Asked {
    dry: bool,
    keep_vm: bool,
    keys: u32,
    cuts: u32,
}

fn ask(args: &[String]) -> Result<Asked> {
    let mut a = Asked { dry: false, keep_vm: false, keys: 40, cuts: 3 };
    let mut it = args.iter();
    while let Some(w) = it.next() {
        let mut n = || -> Result<u32> {
            it.next().and_then(|n| n.parse().ok()).filter(|n| *n > 0).ok_or_else(|| USAGE.to_string())
        };
        match w.as_str() {
            "--dry-run" => a.dry = true,
            "--keep-vm" => a.keep_vm = true,
            "--keys" => a.keys = n()?,
            "--cuts" => a.cuts = n()?,
            _ => return Err(USAGE.into()),
        }
    }
    Ok(a)
}

/// The VM's file: Lima's own template for Debian 12, 4 CPUs and 8 GiB as `kernels`' VMs, only `mounts` (read-only),
/// no containerd.
fn lima_yaml(mounts: &[&Path]) -> Result<String> {
    let mut y = format!(
        "{MADE} — deleted after its run\n\
         minimumLimaVersion: 2.0.0\n\
         base:\n- template:_images/debian-12\n\
         vmType: vz\ncpus: 4\nmemory: 8GiB\ndisk: 30GiB\n\
         # (the build reads the checkout and its git folder: nothing else of home)\n\
         mounts:\n"
    );
    for m in mounts {
        let m = m.to_string_lossy();
        if m.contains(['"', '\\', '\n']) {
            return Err(format!("{m}: a path Lima's file can't hold as it is"));
        }
        y.push_str(&format!("- location: \"{m}\"\n  writable: false\n"));
    }
    y.push_str("containerd:\n  system: false\n  user: false\n");
    Ok(y)
}

/// The VM made ready: the build's tools, rustup (no toolchain: the checkout's file pins it), tarae built.
fn setup_script(root: &Path) -> String {
    let root = crate::linux::quoted(&root.to_string_lossy());
    format!(
        "set -eux -o pipefail\n\
         export DEBIAN_FRONTEND=noninteractive\n\
         sudo -E apt-get update -q\n\
         sudo -E apt-get install -yq build-essential pkg-config curl git ca-certificates\n\
         [ -x ~/.cargo/bin/rustup ] || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
         | sh -s -- -y --profile minimal --default-toolchain none --no-modify-path\n\
         cd {root}\n\
         CARGO_TARGET_DIR={TARGET} ~/.cargo/bin/cargo build --locked --profile shot --bin tarae\n"
    )
}

/// A host started in the VM, on its own (no terminal, its own session), on a file of 2,000 lines — until its attach
/// socket is there.
fn host_script() -> String {
    format!(
        "set -eu\n\
         mkdir -p ~/{PROJECT} && cd ~/{PROJECT}\n\
         seq -f 'line %g' 1 2000 > a.txt\n\
         setsid nohup {TARGET}/shot/tarae --host a.txt </dev/null >/dev/null 2>&1 &\n\
         for i in $(seq 100); do\n\
         \x20 ls \"${{XDG_STATE_HOME:-$HOME/.local/state}}\"/tarae/ui/*.attach >/dev/null 2>&1 && exit 0\n\
         \x20 sleep 0.1\n\
         done\n\
         echo \"no host's attach socket after 10 s\" >&2\n\
         exit 1\n"
    )
}

/// The `ssh` the client runs: the real one, on Lima's `ssh.config`, with no shared connection (a cut is a cut, each
/// attach its whole handshake) — each run's pid written down first (`exec` keeps it), so a link can be cut.
fn ssh_stand_in(config: &Path, pids: &Path) -> String {
    let q = |p: &Path| crate::linux::quoted(&p.to_string_lossy());
    format!(
        "#!/bin/sh\necho $$ >> {}\nexec /usr/bin/ssh -F {} -o ControlMaster=no -o ControlPath=none \"$@\"\n",
        q(pids),
        q(config)
    )
}

/// The address the client attaches to: Lima's host name for the VM, the host's folder in its home.
fn url() -> String {
    format!("ssh://lima-{VM}/~/{PROJECT}")
}

/// What the client said it measured (`--stats`).
#[derive(Debug, Default, PartialEq)]
struct Said {
    first: Option<f64>,
    /// keys, median, p90 (ms), bytes a key
    keys: Option<(u32, f64, f64, u64)>,
    back: Vec<f64>,
}

/// The number right after `head` in `s` (a `,` after it let go).
fn after<T: std::str::FromStr>(s: &str, head: &str) -> Option<T> {
    let rest = &s[s.find(head)? + head.len()..];
    rest.split_whitespace().next()?.trim_end_matches(',').parse().ok()
}

fn said(out: &str) -> Said {
    let mut s = Said::default();
    for l in out.lines().filter_map(|l| Some(l[l.find(SAID)? + SAID.len()..].trim())) {
        if l.starts_with("the first frame ") {
            s.first = after(l, "the first frame ");
        } else if l.contains(" keys, each to its frame: ") {
            let bytes = l.rsplit(", ").next().and_then(|b| after(b, ""));
            s.keys = (|| Some((after(l, "")?, after(l, "median ")?, after(l, "p90 ")?, bytes?)))();
        } else if let Some(each) = l.split("the link down to a frame presented: ").nth(1) {
            s.back = each.split(", ").filter_map(|b| b.trim().strip_suffix(" ms")?.parse().ok()).collect();
        }
    }
    s
}

fn median(of: &[f64]) -> Option<f64> {
    let mut v = of.to_vec();
    v.sort_by(f64::total_cmp);
    (!v.is_empty()).then(|| v[(v.len() - 1) / 2])
}

/// The history's row: the cuts' backs first in `back`, then the frozen link's.
fn row(date: &str, commit: &str, link: &str, s: &Said, cuts: usize) -> String {
    let ms = |v: Option<f64>| v.map_or("-".into(), |v| format!("{v:.2}"));
    let (n, med, p90, bytes) = match s.keys {
        Some((n, m, p, b)) => (n.to_string(), ms(Some(m)), ms(Some(p)), b.to_string()),
        None => ("0".into(), "-".into(), "-".into(), "-".into()),
    };
    let cut = median(&s.back[..cuts.min(s.back.len())]);
    let frozen = s.back.get(cuts).copied();
    format!(
        "{date}\t{commit}\t{link}\t{}\t{n}\t{med}\t{p90}\t{bytes}\t{}\t{}",
        ms(s.first),
        ms(cut),
        ms(frozen)
    )
}

/// A command in the VM, as its user, from `/`.
fn in_vm(script: &str) -> Command {
    let mut c = Command::new("limactl");
    c.args(["shell", "--workdir", "/", VM, "bash", "-c", script]);
    c
}

fn said_status(c: &mut Command) -> bool {
    eprintln!("$ {c:?}");
    c.stdin(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn down(keep_vm: bool) {
    if keep_vm {
        eprintln!("--keep-vm: {VM} left as it is (`limactl delete --force {VM}` after)");
        return;
    }
    said_status(Command::new("limactl").args(["stop", VM]));
    said_status(Command::new("limactl").args(["delete", "--force", VM]));
}

pub fn run(args: &[String]) -> Result<()> {
    let a = ask(args)?;
    let root = root();
    let git = crate::linux::common_git_dir(&root);
    let mounts: Vec<&Path> = std::iter::once(root.as_path()).chain(git.as_deref()).collect();
    let yaml = lima_yaml(&mounts)?;
    if a.dry {
        println!("# {VM}.yaml:\n{yaml}");
        println!("$ limactl start --name={VM} --tty=false {VM}.yaml");
        println!("# in the VM:\n{}", setup_script(&root));
        println!("# the host, in the VM:\n{}", host_script());
        println!("$ cargo build --profile shot --bin tarae    # here");
        println!(
            "# ssh, first on the client's PATH:\n{}",
            ssh_stand_in(Path::new("<lima>/ssh.config"), Path::new("<pids>"))
        );
        println!("$ tarae attach {} --remote-tarae {TARGET}/shot/tarae --stats    # on a pty, driven", url());
        println!("$ limactl stop {VM}; limactl delete --force {VM}");
        return Ok(());
    }
    if quiet({
        let mut c = Command::new("limactl");
        c.arg("--version");
        c
    })
    .is_none()
    {
        return Err("no limactl here — Lima comes with colima (`brew install lima`)".into());
    }
    let (date, commit) = (when(std::time::SystemTime::now()), crate::perf::commit(&root));
    let dir = root.join(OUT);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let out = dir.join(format!("{}-{commit}.txt", &date[..10]));
    let started = Instant::now();
    let listed = quiet({
        let mut c = Command::new("limactl");
        c.args(["list", "--format", "{{.Name}}"]);
        c
    });
    if listed.is_some_and(|l| l.lines().any(|n| n.trim() == VM)) {
        let file = lima_home().join(VM).join("lima.yaml");
        if !std::fs::read_to_string(&file).is_ok_and(|y| y.starts_with(MADE)) {
            return Err(format!(
                "a Lima VM named {VM} that `cargo xtask remote` didn't make — left as it is: rename or delete it"
            ));
        }
        eprintln!("{VM}: a VM a stopped run left — deleted first");
        said_status(Command::new("limactl").args(["delete", "--force", VM]));
    }
    let file = std::env::temp_dir().join(format!("{VM}-{}.yaml", std::process::id()));
    std::fs::write(&file, &yaml).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut start = Command::new("limactl");
    start.arg("start").args([format!("--name={VM}"), "--tty=false".into()]).arg(&file);
    eprintln!("$ {start:?}");
    let (came_up, said_up) = teed(start);
    let _ = std::fs::remove_file(&file);
    let mut kept = format!("$ limactl start\n{said_up}");
    let result = match came_up {
        false => Err("its VM didn't come up (limactl start, above)".to_string()),
        true => measured(&a, &root, &mut kept),
    };
    down(a.keep_vm);
    let _ = std::fs::write(&out, &kept);
    let link = format!("lima {VM} debian-12 vz, ssh -T");
    let history = dir.join("history.tsv");
    let mut rows = std::fs::read_to_string(&history).unwrap_or_else(|_| HISTORY_HEAD.into());
    let s = match result {
        Ok(s) => s,
        Err(why) => {
            rows.push_str(&format!("{date}\t{commit}\t{link}\tnot measured: {why}\n"));
            let _ = std::fs::write(&history, rows);
            return Err(format!("not measured: {why} — {}", out.display()));
        }
    };
    let r = row(&date, &commit, &link, &s, a.cuts as usize);
    rows.push_str(&r);
    rows.push('\n');
    std::fs::write(&history, rows).map_err(|e| format!("{}: {e}", history.display()))?;
    println!("\n{HISTORY_HEAD}{r}");
    println!("{} min in all — {}, a row in {}", minutes(started.elapsed()), out.display(), history.display());
    Ok(())
}

/// The VM up: tarae built there and its host started, tarae built here, the client driven — what it said.
fn measured(a: &Asked, root: &Path, kept: &mut String) -> std::result::Result<Said, String> {
    for (what, script) in [("the setup and the build", setup_script(root)), ("the host", host_script())] {
        eprintln!("# {what}, in {VM}");
        let (ok, said) = teed(in_vm(&script));
        kept.push_str(&format!("\n# {what}\n{said}"));
        if !ok {
            return Err(format!("{what} failed in the VM"));
        }
    }
    let bin = crate::shot::build_shot(root)?;
    // (S-E: an interface is the person's — the attach is never made from a run of Claude's, this tool's neither)
    if std::env::var_os("TARAE_RUN").is_some() {
        return Err(
            "TARAE_RUN is set (a run of Claude's) and tarae attach refuses under it — run `cargo xtask \
                    remote` in a terminal of your own"
                .into(),
        );
    }
    let config = lima_home().join(VM).join("ssh.config");
    let scratch = std::env::temp_dir().join(format!("{VM}-client-{}", std::process::id()));
    std::fs::create_dir(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
    let r = drive(a, &bin, &config, &scratch);
    // (only what this run made, in its own new folder)
    let _ = std::fs::remove_dir_all(&scratch);
    let (out, s) = r?;
    kept.push_str(&format!("\n# the client\n{out}"));
    Ok(s)
}

/// The client on a pty of its own, driven: keys, cuts, a frozen link and a focus back, keys, `:detach`.
fn drive(
    a: &Asked,
    bin: &Path,
    config: &Path,
    scratch: &Path,
) -> std::result::Result<(String, Said), String> {
    use std::os::unix::fs::PermissionsExt as _;
    let (bin_dir, state, home_config) = (scratch.join("bin"), scratch.join("state"), scratch.join("config"));
    for d in [&bin_dir, &state, &home_config] {
        std::fs::create_dir(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let pids = scratch.join("ssh.pids");
    let ssh = bin_dir.join("ssh");
    std::fs::write(&ssh, ssh_stand_in(config, &pids)).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let (master, slave) = crate::pty::open(ROWS, COLS).map_err(|e| format!("pty: {e}"))?;
    let mut master = std::fs::File::from(master);
    let stdio = || slave.try_clone().map(Stdio::from).map_err(|e| e.to_string());
    let path = format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default());
    let mut cmd = Command::new(bin);
    cmd.args(["attach", &url(), "--remote-tarae", &format!("{TARGET}/shot/tarae"), "--stats"])
        .env("PATH", path)
        .env("XDG_STATE_HOME", &state)
        .env("XDG_CONFIG_HOME", &home_config)
        .env("TERM", "xterm-256color")
        .env("TARAE_BACKGROUND", "dark")
        .env_remove("TARAE_UI")
        .stdin(stdio()?)
        .stdout(stdio()?)
        .stderr(stdio()?);
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("ZELLIJ") {
            cmd.env_remove(k);
        }
    }
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            // (`setsid` + the pty as its controlling terminal, as a terminal starts it: ssh may read /dev/tty)
            if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", bin.display()))?;
    drop(slave);
    let (tx, rx) = mpsc::channel();
    let mut reader = master.try_clone().map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let mut buf = [0u8; 64 * 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut d = Driven {
        vt: vt100::Parser::new(ROWS, COLS, 0),
        raw: Vec::new(),
        rx,
        log: state.join("tarae/attach.log"),
    };
    let r = d.script(a, &mut master, &pids);
    if r.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    d.pump(Duration::from_millis(500));
    let out = String::from_utf8_lossy(&d.raw).into_owned();
    r.map_err(|why| format!("{why}\n{}", d.vt.screen().contents()))?;
    let s = said(&out);
    match s.keys {
        Some(_) => Ok((out, s)),
        None => Err(format!("the client said no measure of a key:\n{out}")),
    }
}

struct Driven {
    vt: vt100::Parser,
    raw: Vec<u8>,
    rx: mpsc::Receiver<Vec<u8>>,
    /// The client's log: its "back in" lines as each way back's first frame is presented.
    log: PathBuf,
}

impl Driven {
    fn pump(&mut self, for_: Duration) {
        let end = Instant::now() + for_;
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            match self.rx.recv_timeout(left) {
                Ok(b) => {
                    self.vt.process(&b);
                    self.raw.extend_from_slice(&b);
                }
                Err(_) => break,
            }
        }
    }

    fn until(&mut self, what: &str, done: &dyn Fn(&mut Self) -> bool) -> std::result::Result<(), String> {
        let end = Instant::now() + PATIENCE;
        while !done(self) {
            if Instant::now() > end {
                return Err(format!("{what}: not in {} s", PATIENCE.as_secs()));
            }
            self.pump(Duration::from_millis(50));
        }
        Ok(())
    }

    fn backs(&self) -> usize {
        std::fs::read_to_string(&self.log)
            .map_or(0, |l| l.lines().filter(|l| l.contains(" back in ")).count())
    }

    fn keys(&mut self, to: &mut std::fs::File, key: &str, n: u32) -> std::result::Result<(), String> {
        for _ in 0..n {
            to.write_all(key.as_bytes()).map_err(|e| e.to_string())?;
            self.pump(KEY_GAP);
        }
        Ok(())
    }

    fn script(&mut self, a: &Asked, to: &mut std::fs::File, pids: &Path) -> std::result::Result<(), String> {
        let last_ssh = || -> std::result::Result<i32, String> {
            let p = std::fs::read_to_string(pids).unwrap_or_default();
            p.lines().last().and_then(|p| p.trim().parse().ok()).filter(|p| *p > 1).ok_or("no ssh run".into())
        };
        self.until("the host's first frame", &|d| d.vt.screen().contents().contains("line 1"))?;
        self.keys(to, "j", a.keys)?;
        for i in 1..=a.cuts {
            let ssh = last_ssh()?;
            // SAFETY: kill only sends a signal, to the ssh this run's client started (its stand-in wrote its pid)
            unsafe { libc::kill(ssh, libc::SIGKILL) };
            let n = i as usize;
            self.until(&format!("back after cut {i}"), &|d| d.backs() >= n)?;
            self.keys(to, "k", 3)?;
        }
        let frozen = last_ssh()?;
        // SAFETY: as above — the link held still, as a network gone while the laptop slept
        unsafe { libc::kill(frozen, libc::SIGSTOP) };
        to.write_all(b"\x1b[I").map_err(|e| e.to_string())?;
        let n = a.cuts as usize + 1;
        let back = self.until("back after the frozen link", &|d| d.backs() >= n);
        // SAFETY: as above — the stopped ssh ended, whatever came of it
        unsafe { libc::kill(frozen, libc::SIGKILL) };
        back?;
        self.keys(to, "j", a.keys)?;
        to.write_all(b"\x1b").map_err(|e| e.to_string())?;
        self.pump(KEY_GAP);
        to.write_all(b":detach\r").map_err(|e| e.to_string())?;
        self.until("the client's end", &|d| String::from_utf8_lossy(&d.raw).contains("detached — "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vm_mounts_only_the_checkout_read_only_and_says_its_maker() {
        let y = lima_yaml(&[Path::new("/w/tarae"), Path::new("/w/tarae/.git")]).unwrap();
        assert!(y.starts_with(MADE), "{y}");
        assert_eq!(y.matches("- location: ").count(), 2, "{y}");
        assert_eq!(y.matches("writable: false").count(), 2, "{y}");
        assert!(!y.contains("writable: true") && y.contains("_images/debian-12"), "{y}");
        assert!(lima_yaml(&[Path::new("/w/a\"b")]).is_err());
    }

    #[test]
    fn the_ssh_is_the_real_one_on_limas_config_with_no_shared_connection() {
        let s = ssh_stand_in(Path::new("/l/tarae-remote/ssh.config"), Path::new("/t/ssh.pids"));
        assert_eq!(
            s,
            "#!/bin/sh\necho $$ >> '/t/ssh.pids'\nexec /usr/bin/ssh -F '/l/tarae-remote/ssh.config' \
             -o ControlMaster=no -o ControlPath=none \"$@\"\n"
        );
        assert_eq!(url(), "ssh://lima-tarae-remote/~/tarae-remote");
    }

    #[test]
    fn the_build_is_the_checkouts_toolchain_into_the_vms_own_disk() {
        let s = setup_script(Path::new("/w/my tarae"));
        assert!(s.contains("--default-toolchain none") && s.contains("cd '/w/my tarae'"), "{s}");
        assert!(
            s.contains("CARGO_TARGET_DIR=/var/tmp/tarae-target ") && s.contains("--locked --profile shot"),
            "{s}"
        );
        assert!(host_script().contains("setsid nohup /var/tmp/tarae-target/shot/tarae --host a.txt"));
    }

    /// The client's lines as `src/client/stats.rs` writes them, on a terminal (`\r\n`), read back.
    #[test]
    fn the_clients_measures_are_read_from_its_lines() {
        let out = "\x1b[?1049ldetached — tarae attach ssh://x comes back\r\n\
                   tarae attach --stats: the first frame 412.31 ms from the start (ssh's handshake in it)\r\n\
                   tarae attach --stats: 89 keys, each to its frame: median 0.68 ms, p90 1.06 ms, 340 B a key\r\n\
                   tarae attach --stats: back 4 times, the link down to a frame presented: 61.25 ms, 70.00 ms, \
                   65.50 ms, 3100.40 ms\r\n";
        let s = said(out);
        assert_eq!(
            s,
            Said {
                first: Some(412.31),
                keys: Some((89, 0.68, 1.06, 340)),
                back: vec![61.25, 70.0, 65.5, 3100.4]
            }
        );
        assert_eq!(
            row("2026-11-05 09:00", "abc", "lima", &s, 3),
            "2026-11-05 09:00\tabc\tlima\t412.31\t89\t0.68\t1.06\t340\t65.50\t3100.40"
        );
        assert_eq!(said("no stats here"), Said::default());
    }

    #[test]
    fn its_words_are_asked_once_each() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            ask(&args(&["--keys", "10", "--cuts", "1", "--keep-vm"])).unwrap(),
            Asked { dry: false, keep_vm: true, keys: 10, cuts: 1 }
        );
        for bad in [&["--keys"][..], &["--keys", "0"], &["--cuts", "x"], &["--vm"]] {
            assert!(ask(&args(bad)).is_err(), "{bad:?}");
        }
    }
}
