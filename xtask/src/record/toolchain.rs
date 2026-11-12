//! The take's Rust toolchain outside the home (main's showcase decisions: "The take's checkouts and its Rust toolchain
//! also sit outside the home"). rustc prints its own files' paths into what it says — a note into `std` as
//! `~/.rustup/toolchains/…/lib/rustlib/src/rust/library/…`, one into a dependency as `~/.cargo/registry/src/…`, and
//! through a symlink the real path all the same — so the take's `RUSTUP_HOME` and `CARGO_HOME` are clones in
//! `TAKE_RUST`, made by `cp -c` (the same blocks: no 4 GB copy, a few seconds), of yours:
//!
//! - `rustup/settings.toml` and `rustup/toolchains/<stable>` (your rustup's default, a `stable` one) — no other
//!   toolchain, no downloads or update hashes;
//! - `cargo/registry/` — never `credentials.toml`, your cargo `config.toml`, `git/`, `build/` or `bin/`'s programs;
//! - where a run would find cargo or rustc under your home (`which cargo` prints `~/.cargo/bin/cargo`, and Claude runs
//!   `which` on screen), rustup's proxies — `cargo/bin/rustup` and the names that are it (a link to it, or the same
//!   file) for a tool the cloned toolchain holds — and that folder first on the take's `PATH`.
//!
//! The take's `PATH` is yours with your cargo's `bin` moved last (the programs you installed there, an old `tarae`
//! among them, behind everything else; the take's tarae is named by its path) and nothing under your rustup. Last, not
//! gone: rustup's `~/.cargo/env`, which a shell's startup sources (zsh's `.zshenv`, read by every `zsh -c` — `:sh`,
//! the terminal pane), puts that folder first wherever it's missing (the try, 2026-11-11: `:sh which cargo` printed
//! `~/.cargo/bin/cargo` with it dropped). `RUSTUP_AUTO_INSTALL=0`: rustup never downloads a toolchain on
//! camera — a `rust-toolchain.toml` naming one the clone doesn't hold says `toolchain '…' is not installed` instead
//! (measured, rustup 1.29.1: it syncs that channel otherwise). Where none names one, cargo, rustc, rustdoc, rustfmt,
//! clippy and rust-analyzer all run from the clone with no network (measured).
//!
//! tarae's sandbox follows the two variables (the rust pack's `[sandbox.homes]`, and `RUST*`/`CARGO_*` passed to
//! every run). Refused where the target isn't on your homes' volume — a clone needs the same one, and a plain copy at
//! the take's start is another wait — and, as the take's other folders, where it holds anything. Yours are only read.
//!
//! `cargo xtask record rust-clone DIR` — the same clone into `DIR` (the runbook's try, and its time), the variables
//! said. Made into `TAKE_RUST` before a take, `record take --rust-cloned` takes it as it is (`reuse`, #525): Spotlight
//! indexes a clone's 4 GB for about 8 minutes at ~300 % CPU, and that happens before the take, not in it. A clone says
//! what it made in `MADE` beside its two homes; a take marks it taken, and a clone taken once is never taken again (the
//! last take's downloads are in its registry).

use std::ffi::OsString;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::Result;

/// Where the take's toolchain is cloned: outside the home, beside its folder and its checkouts. Absent or empty before
/// a take (`record take` refuses one that holds anything); removed after it, with the checkouts (the runbook).
pub(crate) const TAKE_RUST: &str = "/Users/Shared/halil-rust";

/// What a clone writes beside its two homes: the toolchain and the proxies it made, and once a take took it, `taken`.
pub(crate) const MADE: &str = ".record-rust-clone";

/// Yours: the homes the take's are cloned from, your home, and the `PATH` a take's run would find cargo on.
pub(crate) struct Yours {
    pub(crate) rustup: PathBuf,
    pub(crate) cargo: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) path: OsString,
}

impl Yours {
    /// This machine's: `RUSTUP_HOME` and `CARGO_HOME` (rustup's proxy sets both for `cargo xtask`), else under `HOME`.
    pub(crate) fn here() -> Result<Yours> {
        let home =
            std::env::var_os("HOME").map(PathBuf::from).ok_or("no HOME to find your Rust toolchain in")?;
        let var = |n: &str, d: &str| std::env::var_os(n).map(PathBuf::from).unwrap_or_else(|| home.join(d));
        Ok(Yours {
            rustup: var("RUSTUP_HOME", ".rustup"),
            cargo: var("CARGO_HOME", ".cargo"),
            path: std::env::var_os("PATH").unwrap_or_default(),
            home,
        })
    }
}

/// What a clone made: the variables that give it to the take, the toolchain's name, the proxies cloned (none: a run
/// finds cargo outside your home), and how long it took.
pub(crate) struct Cloned {
    pub(crate) env: Vec<(&'static str, OsString)>,
    pub(crate) toolchain: String,
    pub(crate) proxies: Vec<String>,
    pub(crate) took: Duration,
}

/// Your toolchain cloned into `to` (the module's doc): refused off your homes' volume or where `to` holds anything;
/// made where it isn't.
pub(crate) fn clone(yours: &Yours, to: &Path) -> Result<Cloned> {
    for from in [&yours.rustup, &yours.cargo] {
        same_volume(to, from)?;
    }
    super::take::folder_for_a_take(to)?;
    let started = Instant::now();
    let settings = yours.rustup.join("settings.toml");
    let text = std::fs::read_to_string(&settings).map_err(|e| format!("{}: {e}", settings.display()))?;
    let toolchain = stable(&text, &yours.rustup.join("toolchains"))?;
    let (rustup, cargo) = (to.join("rustup"), to.join("cargo"));
    mkdir(&rustup.join("toolchains"))?;
    cp(&settings, &rustup.join("settings.toml"))?;
    cp(&yours.rustup.join("toolchains").join(&toolchain), &rustup.join("toolchains").join(&toolchain))?;
    mkdir(&cargo)?;
    let registry = yours.cargo.join("registry");
    if registry.is_dir() {
        cp(&registry, &cargo.join("registry"))?;
    }
    let under_home =
        ["cargo", "rustc"].iter().any(|n| which(n, &yours.path).is_some_and(|p| p.starts_with(&yours.home)));
    let (bin, yours_bin) = (cargo.join("bin"), yours.cargo.join("bin"));
    let mut proxies = Vec::new();
    if under_home {
        let holds = rustup.join("toolchains").join(&toolchain).join("bin");
        proxies = clone_proxies(&yours_bin, &holds, &bin)?;
    }
    let mut made = toml::Table::new();
    made.insert("toolchain".into(), toolchain.clone().into());
    made.insert("proxies".into(), proxies.clone().into());
    let marker = to.join(MADE);
    let text = format!(
        "# What cargo xtask record rust-clone made here (record take --rust-cloned takes it).\n{made}"
    );
    std::fs::write(&marker, text).map_err(|e| format!("{}: {e}", marker.display()))?;
    let env = env_of(yours, to, !proxies.is_empty())?;
    Ok(Cloned { env, toolchain, proxies, took: started.elapsed() })
}

/// A clone's `cargo/bin`, where its proxies are.
fn bin_of(to: &Path) -> PathBuf {
    to.join("cargo/bin")
}

/// The variables that give the clone in `to` to a take: its two homes, rustup never installing, and a `PATH` of yours
/// without rustup's folders, your cargo's `bin` last — the clone's proxies first where it has them.
fn env_of(yours: &Yours, to: &Path, proxies: bool) -> Result<Vec<(&'static str, OsString)>> {
    let (bin, yours_bin) = (bin_of(to), yours.cargo.join("bin"));
    let yours_path: Vec<PathBuf> = std::env::split_paths(&yours.path).collect();
    let path = proxies
        .then(|| bin.clone())
        .into_iter()
        .chain(yours_path.iter().filter(|p| **p != yours_bin && !p.starts_with(&yours.rustup)).cloned())
        .chain(yours_path.contains(&yours_bin).then(|| yours_bin.clone()));
    let path = std::env::join_paths(path).map_err(|e| format!("the take's PATH: {e}"))?;
    Ok(vec![
        ("RUSTUP_HOME", to.join("rustup").into_os_string()),
        ("CARGO_HOME", to.join("cargo").into_os_string()),
        ("RUSTUP_AUTO_INSTALL", "0".into()),
        ("PATH", path),
    ])
}

/// The clone `record rust-clone` made in `to` earlier, taken as it is (#525) — refused where `to` is a link, another
/// user's or others may write in it (`take::real_and_yours`), where none made it (no `MADE`),
/// where `to` holds anything else (a take's checkout, a build), or where a take took it already. What it made, as
/// `clone` said it (no time: it's there).
pub(crate) fn reuse(yours: &Yours, to: &Path) -> Result<Cloned> {
    let again =
        "make it again: empty it, then `cargo xtask record rust-clone DIR` (or leave --rust-cloned out)";
    // (`/Users/Shared` is everyone's: a link, or a folder another user made — or others may write in, whatever it
    // holds — is never taken, as `record take` takes no such folder)
    let m = std::fs::symlink_metadata(to).map_err(|e| format!("{}: {e} — {again}", to.display()))?;
    super::take::real_and_yours(to, &m, super::take::my_uid()).map_err(|e| format!("{e}; {again}"))?;
    if m.mode() & 0o022 != 0 {
        return Err(format!(
            "{}: mode {:o}, others may write in it — not only what record rust-clone made; {again}",
            to.display(),
            m.mode() & 0o7777
        ));
    }
    let marker = to.join(MADE);
    let text = std::fs::read_to_string(&marker)
        .map_err(|e| format!("{}: {e} — not a clone record rust-clone made; {again}", marker.display()))?;
    let made: toml::Table = text.parse().map_err(|e| format!("{}: {e} — {again}", marker.display()))?;
    if made.contains_key("taken") {
        return Err(format!(
            "{}: a take took it already (its downloads are in its registry) — {again}",
            to.display()
        ));
    }
    let mut names: Vec<String> = std::fs::read_dir(to)
        .map_err(|e| format!("{}: {e}", to.display()))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != ".DS_Store")
        .collect();
    names.sort();
    if names != [MADE, "cargo", "rustup"] {
        return Err(format!(
            "{} holds {} — not only what record rust-clone made; {again}",
            to.display(),
            names.join(" ")
        ));
    }
    let toolchain = made.get("toolchain").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    if toolchain.is_empty() || !to.join("rustup/toolchains").join(&toolchain).is_dir() {
        return Err(format!("{}: its toolchain {toolchain:?} isn't there — {again}", to.display()));
    }
    let proxies: Vec<String> = (made.get("proxies").and_then(|v| v.as_array()).into_iter().flatten())
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let env = env_of(yours, to, !proxies.is_empty())?;
    Ok(Cloned { env, toolchain, proxies, took: Duration::ZERO })
}

/// The clone in `to` taken by a take: never taken again (`reuse`).
pub(crate) fn mark_taken(to: &Path) -> Result<()> {
    let marker = to.join(MADE);
    let mut text = std::fs::read_to_string(&marker).map_err(|e| format!("{}: {e}", marker.display()))?;
    text.push_str("taken = true\n");
    std::fs::write(&marker, text).map_err(|e| format!("{}: {e}", marker.display()))
}

/// Refused where `to` (its nearest folder that's there) isn't on `from`'s volume.
fn same_volume(to: &Path, from: &Path) -> Result<()> {
    let there = to
        .ancestors()
        .find(|a| a.exists())
        .ok_or_else(|| format!("{}: nothing of it there", to.display()))?;
    let dev = |p: &Path| std::fs::metadata(p).map(|m| m.dev()).map_err(|e| format!("{}: {e}", p.display()));
    if dev(there)? != dev(from)? {
        return Err(format!(
            "{} isn't on the volume of {}: a clone (cp -c) needs the same one, and a plain copy of the toolchain at the \
             take's start is a different thing to wait for — name a folder on that volume",
            to.display(),
            from.display()
        ));
    }
    Ok(())
}

/// The stable toolchain to clone: your rustup's default (`settings`' `default_toolchain`, its folder in `toolchains`,
/// a bare `stable` with this machine's triple), refused where it isn't a stable one.
fn stable(settings: &str, toolchains: &Path) -> Result<String> {
    let t: toml::Table = settings.parse().map_err(|e| format!("rustup's settings.toml: {e}"))?;
    let name = t
        .get("default_toolchain")
        .and_then(|v| v.as_str())
        .ok_or("rustup's settings name no default toolchain")?;
    if !name.starts_with("stable") {
        return Err(format!(
            "rustup's default toolchain is {name}: the take builds with stable (rustup default stable)"
        ));
    }
    let host = match std::env::consts::OS {
        "macos" => format!("{}-apple-darwin", std::env::consts::ARCH),
        _ => format!("{}-unknown-linux-gnu", std::env::consts::ARCH),
    };
    [name.to_string(), format!("{name}-{host}")]
        .into_iter()
        .find(|n| toolchains.join(n).is_dir())
        .ok_or_else(|| format!("{}: no {name} toolchain there", toolchains.display()))
}

/// rustup's proxies from your cargo's `bin` into `to` (made): `rustup` cloned, each name that is it and that the
/// toolchain `holds` (its `bin`; no `rls` or `cargo-miri` proxy where it has none) — a link to it made again as one
/// (`rustup`, beside it), the same file cloned. The names, sorted. bin's other programs are left.
fn clone_proxies(bin: &Path, holds: &Path, to: &Path) -> Result<Vec<String>> {
    let rustup = bin.join("rustup");
    let real = std::fs::canonicalize(&rustup).map_err(|e| format!("{}: {e}", rustup.display()))?;
    let meta = std::fs::metadata(&real).map_err(|e| format!("{}: {e}", real.display()))?;
    mkdir(to)?;
    cp(&real, &to.join("rustup"))?;
    let mut names: Vec<_> = std::fs::read_dir(bin)
        .map_err(|e| format!("{}: {e}", bin.display()))?
        .flatten()
        .map(|e| e.file_name())
        .filter(|n| n != "rustup" && holds.join(n).exists())
        .collect();
    names.sort();
    let mut proxies = vec!["rustup".to_string()];
    for name in names {
        let p = bin.join(&name);
        let link = std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink());
        let is_it = std::fs::metadata(&p).is_ok_and(|m| m.dev() == meta.dev() && m.ino() == meta.ino());
        if !is_it {
            continue;
        }
        let at = to.join(&name);
        if link {
            std::os::unix::fs::symlink("rustup", &at).map_err(|e| format!("{}: {e}", at.display()))?;
        } else {
            cp(&p, &at)?;
        }
        proxies.push(name.to_string_lossy().into_owned());
    }
    Ok(proxies)
}

/// Where `name` is found on `path`: its first executable file.
fn which(name: &str, path: &OsString) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|d| d.join(name))
        .find(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

fn mkdir(p: &Path) -> Result<()> {
    std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))
}

/// `from` cloned to `to` (not there), folders whole: `cp -c` on macOS (clonefile: the same blocks), a reflink where
/// the file system has them elsewhere (the tests' Linux).
fn cp(from: &Path, to: &Path) -> Result<()> {
    let how: &[&str] = if cfg!(target_os = "macos") { &["-c", "-pR"] } else { &["--reflink=auto", "-pR"] };
    let out = Command::new("cp").args(how).arg(from).arg(to).output().map_err(|e| format!("cp: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cp {} {}: {}",
            from.display(),
            to.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// `record rust-clone DIR` (the module's doc).
pub(super) fn run(args: &[String]) -> Result<()> {
    let [dir] = args else { return Err("usage: cargo xtask record rust-clone DIR".into()) };
    let c = clone(&Yours::here()?, Path::new(dir))?;
    for (k, v) in &c.env {
        println!("{k}={}", v.to_string_lossy());
    }
    eprintln!(
        "record rust-clone: {} and the registry cloned into {dir} in {}{}",
        c.toolchain,
        crate::cut::run_duration(c.took),
        match c.proxies.len() {
            0 => String::new(),
            _ => format!(", rustup's proxies first on PATH ({})", c.proxies.join(" ")),
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests;
