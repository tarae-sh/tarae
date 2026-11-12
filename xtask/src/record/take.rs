//! The full video's take and its dry run (V3, main's `.sb/full-video-v3-brief.md`).
//!
//! - `cargo xtask record take --out DIR --cwd FOLDER --tarae BIN [--rust-cloned] [--size 166x32] [--config FILE]` —
//!   with `--rust-cloned`, the Rust toolchain `record rust-clone` cloned into `TAKE_RUST` before (`toolchain::reuse`,
//!   #525: Spotlight indexes a fresh clone for minutes); without it, cloned at its start — the take (`BIN`
//!   by its path, never a name `PATH` answers: `tarae_bin`), with its surroundings (`surroundings`) in a home made
//!   new for it under `/tmp` (`/tmp/tarae-take-<uid>-<n>`, 0700): a copy of your tarae config (`FILE`, else
//!   `$XDG_CONFIG_HOME/tarae/config.toml` or `~/.config/tarae/config.toml`) with `llm.cost = false`,
//!   `llm.auto-cards = "off"` and `workspace.worktrees` outside the home (`TAKE_WORKTREES`, made where it isn't,
//!   refused where it holds anything), the rest of that folder (themes, plugins) linked beside it; a state and a data
//!   home of its own (`homes_of_its_own`: nothing of yours listed — no recent files, no sessions, no grammar you
//!   downloaded — and a UI socket's path well under `sun_path`) and a cache home outside the home and `/tmp`
//!   (`TAKE_CACHE`, made where it isn't, refused where it holds anything: the preflight's base copy is there); its
//!   tarae's `--version` said first; git's name and mail the stand-ins (`GIT_NAME`, `GIT_MAIL`) by `GIT_CONFIG_GLOBAL`
//!   and the author and committer variables — the floor every run of tarae's sandbox keeps (`src/sandbox/runenv.rs`),
//!   so tarae's git, `:sh` and Claude's runs all commit as them — and in every repository's own config, by git's
//!   template (Claude Code's own git leaves `GIT_CONFIG_GLOBAL` out, #520); your Rust toolchain cloned outside the home (`toolchain.rs`, `TAKE_RUST`: made where it isn't,
//!   refused where it holds anything) as its `RUSTUP_HOME` and `CARGO_HOME`, rustup's proxies first on its `PATH`,
//!   your `~/.cargo/bin` last, `RUSTUP_AUTO_INSTALL=0`; fish's prompt the take's own (`FISH_PROMPT`: the folder's name
//!   and git's branch, `halil (main)> `, no greeting — its config home is the take's, where fish would write its
//!   default, login and machine and all; zsh and bash read your own rc files from `HOME`, so only fish's prompt is the
//!   take's). Its environment is a list of what it keeps (`KEPT`: `HOME`, `USER`, `LOGNAME`, `SHELL`, `TERM`,
//!   `COLORTERM`, the locale, `TMPDIR`) and what it sets, never a list of what it drops: the same from any shell, a
//!   Claude Code one's plumbing included (`takes_env`); what it left out said first, by prefix and count (`left_out`,
//!   names only — some are tokens), and its `SHELL` beside it. `FOLDER` (the take's `/Users/Shared/halil`) is made
//!   where it isn't, refused where it holds anything. Your files are read, never written. Its claude is your config's:
//!   run it only as the take, never in a test.
//! - `cargo xtask record dry [OUT]` — V3's dry run (default `target/rec/record-dry`, made new): the take's motions end
//!   to end at the take's size by the hands (`hands.rs`), on a scene's tarae with the stand-in Claude (`scenes::live`,
//!   `nine`: the real one refused, #247) and the same surroundings; each step and its answer in `dry.txt`, the take in
//!   `take/`, its check in `take/check.txt`.

use std::ffi::OsString;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::hands::{self, code};
use super::toolchain;
use super::{Outer, Spec, check, parse_size, record};
use crate::scenes::{self, docshots::Bg};
use crate::{Result, root, shot};

/// The take's size: 166×32 (the design session's, 2026-11-11).
pub(crate) const TAKE_SIZE: (u16, u16) = (166, 32);

/// Where the take's other checkouts go (`[workspace] worktrees` in its config copy): outside the home, as its folder is
/// (the user's choice) — else `dates`' and `board`'s builds print a path under it, cargo's every `Compiling`. Absent or
/// empty before a take (`record take` refuses one that holds anything); removed after it (the runbook).
pub(crate) const TAKE_WORKTREES: &str = "/Users/Shared/halil-checkouts";

/// The take's cache home (`XDG_CACHE_HOME`, #519): tarae's preflight works in a base copy there (`basetree.rs`), and
/// fmt's diff printed its path whole — under your home, `~/.cache/tarae/base/…`, before. Outside the home, as the take's
/// folder and checkouts are, and never under `/tmp` with the take's other homes: every run may write the temp folders,
/// and tarae makes no run's folder in a cache a run could write (`sandbox::runs_written`) — every run would be refused.
/// Absent or empty before a take (`record take` refuses one that holds anything); removed after it (the runbook).
pub(crate) const TAKE_CACHE: &str = "/Users/Shared/halil-cache";

/// Git's name and mail in a take: stand-ins, never yours.
pub(crate) const GIT_NAME: &str = "halil dev";
pub(crate) const GIT_MAIL: &str = "dev@halil.invalid";

/// The take's fish prompt (`$XDG_CONFIG_HOME/fish/functions/fish_prompt.fish`): the folder's name and git's branch —
/// `halil (main)> `, `halil-564b6228 (try2)> ` in a checkout. `/U/S/halil` reads as a puzzle to a viewer, and the
/// whole path is in what cargo prints (the design session, 2026-11-11).
pub(crate) const FISH_PROMPT: &str = "# The take's prompt (cargo xtask record take): the folder's name and git's branch.\n\
                                       function fish_prompt; printf '%s%s> ' (basename $PWD) (fish_vcs_prompt); end\n";

/// The take's fish greeting (`$XDG_CONFIG_HOME/fish/conf.d/greeting.fish`): none.
pub(crate) const FISH_GREETING: &str =
    "# The take's greeting (cargo xtask record take): none.\nset -g fish_greeting\n";

/// The names a take keeps from the environment it's started in (and the locale's `LC_*`): everything else is left out,
/// whatever shell started it (`takes_env`).
pub(crate) const KEPT: [&str; 8] =
    ["HOME", "USER", "LOGNAME", "SHELL", "TERM", "COLORTERM", "LANG", "TMPDIR"];

/// How what's left out is counted (`left_out`), the first that a name starts with, in this order; the rest "others".
const LEFT_OUT_BY: [&str; 13] = [
    "CLAUDE_CODE_",
    "CLAUDE_AGENT_SDK_",
    "CLAUDE",
    "ANTHROPIC_",
    "MCP_",
    "OTEL_",
    "CARGO_",
    "RUSTUP_",
    "GIT_",
    "XDG_",
    "TARAE_",
    "ZELLIJ",
    "SSH_",
];

const USAGE: &str = "usage: cargo xtask record take --out DIR --cwd FOLDER --tarae BIN [--rust-cloned] [--size 166x32] [--config FILE] | record dry [OUT]";

/// The take's surroundings in `home` (a folder of the take's own): tarae's config there — `config` (your config's
/// text; None: none) with `llm.cost = false`, `llm.auto-cards = "off"` and, given one, `workspace.worktrees =
/// worktrees` (None: the config's own), and every other entry of `linked` (your config's folder) linked beside it — and
/// a git config of the stand-ins'. The variables that give them to the take.
pub(crate) fn surroundings(
    home: &Path,
    config: Option<&str>,
    linked: Option<&Path>,
    worktrees: Option<&Path>,
) -> Result<Vec<(&'static str, OsString)>> {
    let mut t: toml::Table = match config {
        Some(text) => text.parse().map_err(|e| format!("your tarae config isn't TOML tarae reads: {e}"))?,
        None => toml::Table::new(),
    };
    let llm = t.entry("llm").or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let llm = llm.as_table_mut().ok_or("your tarae config's llm isn't a table")?;
    llm.insert("cost".into(), toml::Value::Boolean(false));
    // (no second claude reading each card on camera: yours may be "shadow", whose record is your state home's anyway)
    llm.insert("auto-cards".into(), toml::Value::String("off".into()));
    if let Some(at) = worktrees {
        let ws = t.entry("workspace").or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let ws = ws.as_table_mut().ok_or("your tarae config's workspace isn't a table")?;
        ws.insert("worktrees".into(), toml::Value::String(at.to_string_lossy().into_owned()));
    }
    let dir = home.join("config/tarae");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = toml::to_string(&t).map_err(|e| format!("the take's config: {e}"))?;
    let file = dir.join("config.toml");
    let head = match worktrees {
        Some(_) => {
            "# The take's copy of your tarae config (cargo xtask record take): llm.cost = false, llm.auto-cards = \
                    \"off\" and workspace.worktrees outside the home, nothing else changed.\n"
        }
        None => {
            "# The take's copy of your tarae config (cargo xtask record take): llm.cost = false and llm.auto-cards = \
                 \"off\", nothing else changed.\n"
        }
    };
    std::fs::write(&file, format!("{head}{text}")).map_err(|e| format!("{}: {e}", file.display()))?;
    if let Some(from) = linked
        && let Ok(entries) = std::fs::read_dir(from)
    {
        for e in entries.flatten() {
            let name = e.file_name();
            if name != "config.toml" {
                let to = dir.join(&name);
                std::os::unix::fs::symlink(e.path(), &to)
                    .map_err(|err| format!("{}: {err}", to.display()))?;
            }
        }
    }
    // (#520: Claude Code's own git — its context's "Git user: …" — leaves GIT_CONFIG_GLOBAL out of its environment
    // (2.1.322, with GIT_CONFIG_SYSTEM, GIT_CONFIG_COUNT/KEY_n/VALUE_n and GIT_CONFIG_PARAMETERS) and reads your
    // ~/.gitconfig: a repository's own config outranks that, so every repository the take makes carries the stand-ins
    // in its .git/config, from git's template — its worktrees, the take's other checkouts, share it)
    let template = home.join("git-template");
    let user = format!("[user]\n\tname = {GIT_NAME}\n\temail = {GIT_MAIL}\n");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&template)
        .map_err(|e| format!("{}: {e}", template.display()))?;
    let local = template.join("config");
    std::fs::write(&local, &user).map_err(|e| format!("{}: {e}", local.display()))?;
    let git = home.join("gitconfig");
    let gitconfig = format!(
        "# The take's git config (cargo xtask record take): the stand-ins' name and mail, and a template that puts them in \
         every repository's own config.\n{user}[init]\n\tdefaultBranch = main\n\ttemplateDir = {}\n[commit]\n\tgpgsign = false\n",
        template.display()
    );
    std::fs::write(&git, gitconfig).map_err(|e| format!("{}: {e}", git.display()))?;
    let fish = home.join("config/fish");
    for (file, text) in [("functions/fish_prompt.fish", FISH_PROMPT), ("conf.d/greeting.fish", FISH_GREETING)]
    {
        let p = fish.join(file);
        let dir = p.parent().unwrap_or(&fish);
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(vec![
        // (cargo's build folder in each checkout's own `target`: a shared one set in your cargo config is a path under
        // your home, and `cargo test` says it on every run — "Running unittests src/main.rs (/Users/…)")
        ("CARGO_BUILD_BUILD_DIR", "{workspace-root}/target".into()),
        ("XDG_CONFIG_HOME", home.join("config").into()),
        ("GIT_CONFIG_GLOBAL", git.into()),
        ("GIT_AUTHOR_NAME", GIT_NAME.into()),
        ("GIT_AUTHOR_EMAIL", GIT_MAIL.into()),
        ("GIT_COMMITTER_NAME", GIT_NAME.into()),
        ("GIT_COMMITTER_EMAIL", GIT_MAIL.into()),
    ])
}

/// The take's environment: of `caller`'s (the environment `record take` was started in), only the names it keeps
/// (`KEPT`, `LC_*`), then `set` (the take's own, over a kept one of the same name) — and the names left out, sorted.
/// A list of what's kept, never of what's dropped: a Claude Code shell's `CLAUDE_CODE_*` (a messaging socket and its
/// token, a session id), `ANTHROPIC_BASE_URL` or `MCP_*`, what `cargo run` sets (`CARGO_PKG_*`, rustup's
/// `RUSTUP_TOOLCHAIN` pin), whatever comes next — none reaches the take's real, billed claude or its builds.
pub(crate) fn takes_env(
    caller: impl IntoIterator<Item = (OsString, OsString)>,
    set: &[(OsString, OsString)],
) -> (Vec<(OsString, OsString)>, Vec<String>) {
    let is_set = |k: &OsString| set.iter().any(|(s, _)| s == k);
    let (mut env, mut left) = (Vec::new(), Vec::new());
    for (k, v) in caller {
        let n = k.to_string_lossy();
        if KEPT.contains(&&*n) || n.starts_with("LC_") {
            if !is_set(&k) {
                env.push((k, v));
            }
        } else if !is_set(&k) {
            left.push(n.into_owned());
        }
    }
    env.extend(set.iter().cloned());
    left.sort();
    (env, left)
}

/// What `takes_env` left out, said by prefix and count (`LEFT_OUT_BY`) — names only, never a value (some are tokens):
/// `left out of the take: CLAUDE_CODE_… ×28, ANTHROPIC_… ×2, CARGO_… ×9, 14 others`.
pub(crate) fn left_out(names: &[String]) -> String {
    let mut counts = [0usize; LEFT_OUT_BY.len()];
    let mut others = 0;
    for n in names {
        match LEFT_OUT_BY.iter().position(|p| n.starts_with(p)) {
            Some(i) => counts[i] += 1,
            None => others += 1,
        }
    }
    let mut said: Vec<String> =
        LEFT_OUT_BY.iter().zip(counts).filter(|(_, c)| *c > 0).map(|(p, c)| format!("{p}… ×{c}")).collect();
    match others {
        0 => {}
        1 => said.push("1 other".into()),
        n => said.push(format!("{n} others")),
    }
    if said.is_empty() {
        said.push("nothing".into());
    }
    format!("left out of the take: {}", said.join(", "))
}

/// `cmd`'s environment made the take's (`takes_env`): `set` and `cmd`'s own explicit ones over the kept names of
/// this process's, nothing else. What it left out, said (`left_out`).
fn the_takes_env(cmd: &mut Command, set: Vec<(&'static str, OsString)>) -> String {
    let mut own: Vec<(OsString, OsString)> =
        cmd.get_envs().filter_map(|(k, v)| Some((k.to_os_string(), v?.to_os_string()))).collect();
    own.retain(|(k, _)| !set.iter().any(|(s, _)| k == s));
    own.extend(set.into_iter().map(|(k, v)| (OsString::from(k), v)));
    let (env, left) = takes_env(std::env::vars_os(), &own);
    cmd.env_clear().envs(env);
    left_out(&left)
}

/// Your tarae config's folder, as tarae finds it: `$XDG_CONFIG_HOME/tarae`, else `~/.config/tarae`.
fn your_config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|c| c.join("tarae"))
}

/// `record take …` (the module's doc).
pub(super) fn take(args: &[String]) -> Result<()> {
    let (mut out, mut cwd, mut size, mut config, mut bin) = (None, None, TAKE_SIZE, None, None);
    let mut rust_cloned = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{a} wants a value — {USAGE}"));
        match a.as_str() {
            "--out" => out = Some(PathBuf::from(value()?)),
            "--cwd" => cwd = Some(PathBuf::from(value()?)),
            "--size" => size = parse_size(value()?)?,
            "--config" => config = Some(PathBuf::from(value()?)),
            "--tarae" => bin = Some(PathBuf::from(value()?)),
            // (#525: the clone `record rust-clone` made in TAKE_RUST before the take, Spotlight done with it)
            "--rust-cloned" => rust_cloned = true,
            other => return Err(format!("{other}: {USAGE}")),
        }
    }
    let out = out.ok_or_else(|| format!("--out DIR: where the take goes — {USAGE}"))?;
    let cwd =
        cwd.ok_or_else(|| format!("--cwd FOLDER: the take's folder (/Users/Shared/halil) — {USAGE}"))?;
    let bin = tarae_bin(bin.as_deref())?;
    // (the build it is, said before anything is made: the runbook's "check once it's up" compares it with main's tip)
    let version =
        Command::new(&bin).arg("--version").output().map_err(|e| format!("{}: {e}", bin.display()))?;
    if !version.status.success() {
        return Err(format!(
            "{} --version: {}",
            bin.display(),
            String::from_utf8_lossy(&version.stderr).trim()
        ));
    }
    eprintln!("record take: {} — {}", String::from_utf8_lossy(&version.stdout).trim(), bin.display());
    folder_for_a_take(&cwd)?;
    let worktrees = Path::new(TAKE_WORKTREES);
    folder_for_a_take(worktrees)?;
    let cache = Path::new(TAKE_CACHE);
    folder_for_a_take(cache)?;
    let (yours, at) = (toolchain::Yours::here()?, Path::new(toolchain::TAKE_RUST));
    let rust = match rust_cloned {
        true => toolchain::reuse(&yours, at)?,
        false => {
            eprintln!("record take: your Rust toolchain cloned into {} …", toolchain::TAKE_RUST);
            toolchain::clone(&yours, at)?
        }
    };
    // (taken: never taken by another take, whose registry would hold this one's downloads)
    toolchain::mark_taken(at)?;
    let config = config.or_else(|| your_config_dir().map(|d| d.join("config.toml")));
    let text = match &config {
        Some(f) => match std::fs::read_to_string(f) {
            Ok(t) => Some(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("{}: {e}", f.display())),
        },
        None => None,
    };
    let home = hands::fresh_folder(Path::new("/tmp"), "tarae-take")?;
    let linked = config.as_deref().and_then(Path::parent);
    let mut env = surroundings(&home, text.as_deref(), linked, Some(worktrees))?;
    let own = homes_of_its_own(&home, cache)?;
    let state = home.join("state");
    env.extend(own);
    env.extend(rust.env);
    let mut cmd = Command::new(&bin);
    eprintln!("record take: {}", the_takes_env(&mut cmd, env));
    // (fish against zsh changes what's on screen: the terminal pane's prompt, `:sh`'s)
    match std::env::var_os("SHELL") {
        Some(s) => eprintln!("record take: its shell {} (the one that started it)", s.to_string_lossy()),
        None => eprintln!("record take: no SHELL where it was started — tarae's own default"),
    }
    eprintln!(
        "record take: its home {} — your config copied with llm.cost = false, auto-cards off and the checkouts in \
         {TAKE_WORKTREES}, git as {GIT_NAME} <{GIT_MAIL}> (its repositories' own config too), fish's prompt its own, a \
         state and data home of its own, its cache home {TAKE_CACHE}, Rust's {} and registry {}{}; tarae in {}",
        home.display(),
        rust.toolchain,
        match rust_cloned {
            true => format!("as record rust-clone made them in {} before the take", toolchain::TAKE_RUST),
            false =>
                format!("cloned into {} in {}", toolchain::TAKE_RUST, crate::cut::run_duration(rust.took)),
        },
        if rust.proxies.is_empty() { "" } else { " (rustup's proxies first on PATH)" },
        cwd.display()
    );
    let outer = Outer::stdio()?;
    let spec = Spec {
        size,
        out: out.clone(),
        outer: Some(outer),
        keys: Vec::new(),
        quit_after_keys: false,
        ui: Some(state),
        cwd: Some(cwd),
        hands: Some(out.clone()),
    };
    let taken = record(spec, cmd)?;
    eprintln!("{}", taken.said(&out));
    Ok(())
}

/// The take's state and data homes, new in `home` (0700, empty): no recent files, sessions or trust of yours, and no
/// grammar you downloaded (`$XDG_DATA_HOME/tarae/grammars`) — a release build embeds only the core grammars, so the
/// first `.rs` file raises the Rust grammar's card, as a first user meets it (the design session, 2026-11-11). And its
/// cache home, `cache` (`TAKE_CACHE`): made where it isn't, refused where it holds anything, 0700. The variables that
/// give them.
pub(crate) fn homes_of_its_own(home: &Path, cache: &Path) -> Result<Vec<(&'static str, OsString)>> {
    let mut env = Vec::new();
    for (var, name) in [("XDG_STATE_HOME", "state"), ("XDG_DATA_HOME", "data")] {
        let at = home.join(name);
        std::fs::DirBuilder::new().mode(0o700).create(&at).map_err(|e| format!("{}: {e}", at.display()))?;
        env.push((var, at.into_os_string()));
    }
    folder_for_a_take(cache)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(cache, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("{}: {e}", cache.display()))?;
    env.push(("XDG_CACHE_HOME", cache.as_os_str().to_owned()));
    Ok(env)
}

/// The tarae a take runs: `--tarae BIN`, a path to an executable file (made absolute: tarae starts in the take's
/// folder) — never a bare name, which `PATH` would answer with whatever tarae is installed there (your
/// `~/.cargo/bin/tarae` may be weeks old).
pub(crate) fn tarae_bin(given: Option<&Path>) -> Result<PathBuf> {
    let say = "--tarae BIN: the tarae the take runs, by its path — the release build of main's tip, as a first user \
               gets it (`cargo build --release --bin tarae`: target/release/tarae), never one found on PATH";
    let p = given.ok_or_else(|| format!("{say} — {USAGE}"))?;
    if p.components().count() < 2 {
        return Err(format!("{}: a name, not a path — {say}", p.display()));
    }
    use std::os::unix::fs::PermissionsExt;
    let m = std::fs::metadata(p).map_err(|e| format!("{}: {e} — {say}", p.display()))?;
    if !m.is_file() || m.permissions().mode() & 0o111 == 0 {
        return Err(format!("{}: not a program — {say}", p.display()));
    }
    std::fs::canonicalize(p).map_err(|e| format!("{}: {e}", p.display()))
}

/// The take's folder: made where it isn't; refused where it holds anything (a take starts from an empty one).
pub(super) fn folder_for_a_take(cwd: &Path) -> Result<()> {
    match std::fs::read_dir(cwd) {
        Ok(mut d) => match d.next() {
            None => Ok(()),
            Some(_) => Err(format!(
                "{}: not empty — a take starts in an empty folder (move the last take's away)",
                cwd.display()
            )),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(cwd).map_err(|e| format!("{}: {e}", cwd.display()))
        }
        Err(e) => Err(format!("{}: {e}", cwd.display())),
    }
}

// ── The dry run ─────────────────────────────────────────────────────────────────────────────────────────────

/// The dry run's hands: each step a command line as the driver types it (`record …`), made a request as the client
/// makes it (`hands::request`), its answer kept with the take's clock.
struct Driver {
    sock: PathBuf,
    started: Instant,
    said: Vec<String>,
}

impl Driver {
    fn run(&mut self, line: &str) -> (i32, String) {
        let words: Vec<String> = line.split(' ').map(String::from).collect();
        let at = self.started.elapsed().as_millis() as u64;
        let answer = hands::request(&words[0], &words[1..]).and_then(|(req, _)| hands::ask(&self.sock, &req));
        let (c, said) = answer.unwrap_or_else(|e| (code::ERROR, e));
        let line = format!("{:>9}  record {line}\n           {c} · {said}", crate::cut::clock(at));
        eprintln!("{line}");
        self.said.push(line);
        (c, said)
    }

    fn note(&mut self, words: &str) {
        eprintln!("# {words}");
        self.said.push(format!("# {words}"));
    }

    /// `wait` until it holds, each card (or edit's review) that comes first answered `y`: focused first where it waits.
    fn through(&mut self, wait: &str) -> (i32, String) {
        for _ in 0..8 {
            let (c, said) = self.run(wait);
            if c != code::CARD {
                return (c, said);
            }
            self.answer(&said);
        }
        (code::ERROR, "eight cards in a row".into())
    }

    fn answer(&mut self, said: &str) {
        if said.contains("its review up") {
            if self.run("key --to edit-review y").0 == code::REFUSED {
                self.run("key C-q");
                self.run("key --to edit-review y");
            }
            return;
        }
        if said.contains("waits — C-q") {
            self.run("key C-q");
        }
        self.run("key --to card y");
    }

    /// The keys back in the code: `esc` until they are (three at most), each look said.
    fn back_to_code(&mut self) {
        for _ in 0..3 {
            if self.run("wait keys code --for 0").0 == code::HELD {
                return;
            }
            self.run("key esc");
        }
    }

    /// A `:` command typed and sent from the code.
    fn command(&mut self, words: &str) -> (i32, String) {
        self.back_to_code();
        self.run("key :");
        self.run(&format!("type --to command --send {words}"))
    }
}

/// `record dry [OUT]` (the module's doc).
pub(super) fn dry(args: &[String]) -> Result<()> {
    let out = match args {
        [] => super::rec_dir("record-dry"),
        [o] => PathBuf::from(o),
        _ => return Err(USAGE.into()),
    };
    if out.exists() {
        return Err(format!("{} is there already — name another, or move it away first", out.display()));
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let bin = shot::build(&root(), "shot-docs", &["bundled-grammars"])?;
    let me = std::env::current_exe().map_err(|e| format!("this xtask: {e}"))?;
    let live = scenes::live("record-dry", "nine", Bg::Dark, &bin, &me, true)?;
    let mut cmd = shot::command(&live.setup, &live.state, &live.homes)?;
    // (the take's surroundings, the stand-in's config standing for yours — its checkouts where the scene keeps them,
    // in its own folder: a dry run writes nothing in /Users/Shared)
    let home = hands::fresh_folder(Path::new("/tmp"), "tarae-take")?;
    let file = live.setup.config.join("tarae/config.toml");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let env = surroundings(&home, Some(&text), None, None)?;
    eprintln!("record dry: {}", the_takes_env(&mut cmd, env));
    let take = out.join("take");
    let spec = Spec {
        size: TAKE_SIZE,
        out: take.clone(),
        outer: None,
        keys: Vec::new(),
        quit_after_keys: false,
        ui: Some(live.state.clone()),
        cwd: Some(live.setup.cwd.clone()),
        hands: Some(take.clone()),
    };
    let started = Instant::now();
    let (done_tx, done) = mpsc::channel();
    let taking = std::thread::spawn(move || {
        let r = record(spec, cmd);
        let _ = done_tx.send(());
        r
    });
    // (the way in is made before tarae starts: its file there, or the take refused)
    let way = take.join(hands::WAY_IN);
    while !way.exists() {
        if taking.is_finished() {
            return taking.join().map_err(|_| "the take panicked".to_string())?.map(|_| ());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut d = Driver { sock: hands::way_in(Some(&take))?, started, said: Vec::new() };
    steps(&mut d);
    // (tarae quit by `:q!` in the steps; if it's still there a minute on, ended)
    if done.recv_timeout(Duration::from_secs(60)).is_err() {
        d.note("tarae still running a minute after :q! — ended (SIGTERM)");
        if let Some(pid) = ui_pid(&live.state) {
            // SAFETY: a signal to the take's own tarae, found by its socket in the take's own state home.
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }
    let taken = taking.join().map_err(|_| "the take panicked".to_string())??;
    d.note(&taken.said(&take));
    let hits = check::scan(&take, &check::Yours::here())?;
    let weak = hits.iter().filter(|h| h.weak).count();
    d.note(&format!("check: {} places, {weak} weak (take/check.txt)", hits.len() - weak));
    let p = take.join("check.txt");
    std::fs::write(&p, check::listed(&hits)).map_err(|e| format!("{}: {e}", p.display()))?;
    let p = out.join("dry.txt");
    std::fs::write(&p, d.said.join("\n") + "\n").map_err(|e| format!("{}: {e}", p.display()))?;
    let _ = std::fs::remove_dir_all(&home);
    drop(live);
    Ok(())
}

/// The pid of the one tarae whose UI socket is in `state`.
fn ui_pid(state: &Path) -> Option<i32> {
    let dir = std::fs::read_dir(state.join("tarae/ui")).ok()?;
    dir.flatten().find_map(|e| e.path().file_stem()?.to_str()?.parse().ok())
}

/// The take's motions, as the brief's dry run lists them.
fn steps(d: &mut Driver) {
    d.note("tarae up, the keys in the code");
    d.run("wait keys code --for 60");
    d.run("chapter one ask, one run");
    d.run("key space l");
    d.run("wait keys chat --for 10");
    d.run("type --to chat --send run the tests");
    d.run("wait card run --for 60");
    d.through("wait turn --for 120");
    d.note("the chat-input check: after the card's answer and the turn's end, are the keys back in the chat's input?");
    d.run("wait keys chat --for 0");
    d.run("state");
    docked_input(d);

    d.run("chapter two at once");
    d.command("session-new");
    d.run("wait keys session-name --for 10");
    d.run("type --to session-name --send dates");
    let (c, said) = d.run("wait card session-new --for 10");
    if c == code::HELD {
        d.answer(&said);
    }
    if d.run("wait keys chat --for 30").0 != code::HELD {
        d.run("key space l");
    }
    d.run("type --to chat --send explain the insert");
    d.through("wait turn dates --for 120");
    d.note("the chat-input check again, after an edit's review");
    d.run("wait keys chat --for 0");
    d.note("a commit in dates, by the stand-ins' name: the bring's, and the blame's");
    d.command("sh echo Tied-by-dates >> README.md && git commit -qam dates-readme && echo committed");
    d.run("state");

    d.note("a switch between the two, by the session list");
    d.back_to_code();
    d.run("key space W");
    d.run("state");
    d.run("key 1");
    d.run("state");
    d.back_to_code();
    d.run("key space W");
    d.run("key 2");
    d.run("state");

    d.run("chapter the bring");
    d.command("workspace-bring");
    let (_, said) = d.through("wait bring --for 120");
    if said.contains("not brought") {
        d.note("not brought: its card again, answered as it reads (y: bring anyway)");
        if let (code::HELD, said) = d.run("wait card rebased --for 10") {
            d.answer(&said);
        }
        d.through("wait bring --for 120");
    }
    d.run("state");

    d.run("chapter the blame, by the stand-ins");
    d.command("open README.md");
    d.run("key g e");
    d.run("state");

    d.run("mark the end");
    d.command("q!");
    d.through("wait card --for 5");
}

/// The chat-input check, docked (calls #1): the chat's panel shown (`space L`: the ask line closed on its `enter`, so the
/// chat is out of sight and comes back as the docked column), the keys in its input, a message sent; then where the
/// keys are at once, and again after the turn (its cards answered).
fn docked_input(d: &mut Driver) {
    d.run("mark the docked input");
    d.note("the chat-input check, docked: the chat's panel shown by space L, the keys in its input, a message sent");
    d.back_to_code();
    d.run("key space L");
    if d.run("wait keys chat --for 5").0 != code::HELD {
        d.note("space L didn't give the chat's input the keys: once more (it was in sight, and hid)");
        d.run("key space L");
        d.run("wait keys chat --for 5");
    }
    d.run("state");
    d.run("type --to chat --send explain the run");
    d.note("the docked input, the moment after its enter");
    d.run("wait keys chat --for 0");
    // (its edit turned down: main's checkout stays clean for the bring, which refuses one with changes in what it brings)
    for _ in 0..8 {
        let (c, said) = d.run("wait turn --for 120");
        match c {
            code::CARD if said.contains("its review up") => {
                if d.run("key --to edit-review n").0 == code::REFUSED {
                    d.run("key C-q");
                    d.run("key --to edit-review n");
                }
            }
            code::CARD => d.answer(&said),
            _ => break,
        }
    }
    d.note("the docked input, after the turn");
    d.run("wait keys chat --for 0");
    d.run("state");
}
