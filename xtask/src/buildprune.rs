//! `cargo xtask build-prune` (#168): what's old in cargo's build folders, pruned — `<place>/xx/yyyy`, the
//! `{workspace-path-hash}` layout this repository's checkouts build in (`$CARGO_HOME/build`, `~/.cargo/build`, or
//! `--in`). It lists first; `--yes` removes. A dev tool: tarae itself only says the disk's room
//! (`src/diskroom.rs`) and removes nothing.
//!
//! ```text
//! cargo xtask build-prune [--age 2d] [--whole [--others]] [--yes] [--in <folder>]
//! ```
//!
//! Each profile folder (`<build>/<profile>`, or `<build>/<triple>/<profile>`: one with `deps/`) only while this
//! holds cargo's own lock there, `<profile>/.cargo-build-lock` (`flock`, as cargo 1.99 takes it: a build in flight
//! holds it, and one that starts meanwhile waits a moment — the lock is let go every `SLICE`; checked by hand,
//! `.sb/buildcache/lock.sh`). A retake that fails loses the hold for good: nothing more is removed there, the
//! profile counted busy. A profile with no lock file isn't the layout it knows: left alone.
//!
//! - always, **orphaned objects**: a `deps/*.rcgu.o` whose every link is a name in `deps/`, in a unit that links
//!   its objects from `incremental/` (another object of its has such a link) or whose output is gone from `deps/`.
//!   macOS's `unpacked` debug info keeps a binary's objects as its debug info: rustc links each incremental
//!   session's objects into `deps/` under new names and never removes the old ones (#168: 1.5 GB a relink of
//!   tarae's tests, 9.3 GB in a morning in one checkout). The ones a binary names are linked from its session
//!   too (checked against the debug map by hand: `.sb/buildcache/oso.sh`). A unit none of whose objects is linked
//!   from elsewhere is left whole — nothing says which of them a binary names (a dependency built without
//!   incremental). One written in the last `GRACE` is left: a build may have only just let the lock go.
//! - `--age`: **incremental caches**, `incremental/<crate>-<hash>/` whose newest session is older (a configuration
//!   not built since); **old test binaries**, `deps/<name>-<16 hex>` (and its `.d`) older, another of that name
//!   newer.
//! - `--whole` (with `--age`): a **build folder no checkout of this repository builds in** (each asked `cargo
//!   metadata`), untouched for the age, that built this repository's own package (its `.fingerprint/<name>-<16
//!   hex>`) — another project's (the global build-dir holds every project's, and none of them is a checkout here)
//!   only with `--others`, the listing saying which is which as far as the fingerprints tell. Only a folder named
//!   as cargo names one (`{workspace-path-hash}`: `xx/` two hex digits, then fourteen), holding nothing cargo never
//!   makes (`FOREIGN`: `.git`, `Cargo.toml`, `src/` … — a project's root under `--in` has a `target/debug` that
//!   looks like a profile), and only while every one of its profiles' locks is held (one with no lock file refuses
//!   it); each refusal said. Renamed aside in its own folder (one `renameat`, to `ASIDE`…), the locks let go, then
//!   removed. A build that starts there waits only for the rename; one interrupted is finished by the next pass,
//!   and one that won't go is said. None is removed whole when a checkout didn't say where it builds.
//!
//! Nothing else: fingerprints, build scripts' output, libraries (a proc macro's may be loaded), `tmp/`. All of it
//! is cache: what's gone is built again when it's needed, never built wrong.
//!
//! **Through no link.** A build folder is written by sandboxed runs, and this isn't sandboxed. std's
//! `remove_dir_all` (1.58.1 on, CVE-2022-21658) goes through no link below the path it's given, but it follows
//! that path's own folders — and `debug/`, `deps/`, `incremental/` are a run's to swap. So every step here is on
//! descriptors (`Dir`): each folder opened from the one before with `O_NOFOLLOW | O_DIRECTORY`, every name read
//! with `AT_SYMLINK_NOFOLLOW`, every removal an `unlinkat` and the aside a `renameat` on those descriptors, the
//! lock opened `O_NOFOLLOW`; nothing re-walked by path after a check. A link where a folder is looked for is never
//! gone through; a link inside what's removed goes as itself.

use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use crate::Result;

const USAGE: &str =
    "usage: cargo xtask build-prune [--age 30m|6h|2d] [--whole [--others]] [--yes] [--in <folder>]";
/// How long a pass holds a profile's lock at a time: a build that starts waits no longer.
const SLICE: Duration = Duration::from_millis(200);
/// The pause between two holds, for a build that waits on the lock to take it.
const PAUSE: Duration = Duration::from_millis(50);
/// An object written this recently is left: a build may have only just let the lock go.
const GRACE: Duration = Duration::from_secs(10 * 60);
/// The name a build folder removed whole goes by between its rename and its end (`.xtask-pruning-<pid>-<n>`) —
/// one found by a later pass is finished.
const ASIDE: &str = ".xtask-pruning-";
/// Cargo's lock in a profile folder.
const LOCK: &str = ".cargo-build-lock";
/// A build folder is this many folders below the place (`{workspace-path-hash}`: `xx/yyyy`).
const DEPTH: usize = 2;
/// How deep a removal goes before it stops, saying so.
const TREE_DEPTH: usize = 32;
/// What cargo never makes in a build folder or one below it — a project's root, or a checkout: never removed whole.
const FOREIGN: &[&str] =
    &[".git", ".hg", ".jj", ".svn", "Cargo.toml", "Cargo.lock", ".cargo", "src", "target"];

pub fn run(args: &[String]) -> Result<()> {
    let a = Args::read(args)?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let at =
        a.at.or_else(|| std::env::var_os("CARGO_HOME").map(|h| PathBuf::from(h).join("build")))
            .or_else(|| home.as_ref().map(|h| h.join(".cargo/build")))
            .ok_or("no CARGO_HOME nor HOME — name the folder with --in")?;
    let place = place(&at, home.as_deref())?;
    let claimed = match a.whole {
        true => claimed(&crate::root()),
        false => Ok(Claims::default()),
    };
    let opts = Opts { age: a.age, whole: a.whole, others: a.others, claimed, remove: a.yes };
    let p = pass(&place, &opts, SystemTime::now());
    print!("{}", report(&p, a.yes, home.as_deref()));
    Ok(())
}

/// The command line.
#[derive(Debug, Default, PartialEq)]
struct Args {
    age: Option<Duration>,
    whole: bool,
    others: bool,
    yes: bool,
    at: Option<PathBuf>,
}

impl Args {
    fn read(args: &[String]) -> Result<Args> {
        let mut a = Args::default();
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--age" => a.age = Some(age(it.next().ok_or("--age needs a time: 30m, 6h, 2d")?)?),
                "--whole" => a.whole = true,
                "--others" => a.others = true,
                "--yes" => a.yes = true,
                "--in" => a.at = Some(PathBuf::from(it.next().ok_or("--in needs a folder")?)),
                other => return Err(format!("{other}? {USAGE}")),
            }
        }
        if a.whole && a.age.is_none() {
            return Err(
                "--whole needs --age: how long a folder no checkout builds in is left untouched first (--age 3d)"
                    .into(),
            );
        }
        if a.others && !a.whole {
            return Err(
                "--others goes with --whole: it lets other projects' build folders go whole too".into()
            );
        }
        Ok(a)
    }
}

/// `30m`, `6h`, `2d`.
fn age(s: &str) -> Result<Duration> {
    let bad = || format!("--age {s}: a number and m, h or d (30m, 6h, 2d)");
    let Some((last, _)) = s.char_indices().last() else { return Err(bad()) };
    let (n, unit) = s.split_at(last);
    let n: u64 = n.parse().map_err(|_| bad())?;
    let secs = match unit {
        "m" => 60,
        "h" => 3600,
        "d" => 24 * 3600,
        _ => return Err(bad()),
    };
    Ok(Duration::from_secs(n * secs))
}

/// The place the build folders are in, by its real path: a folder, not a link itself, neither `/` nor the home
/// folder nor above it.
fn place(at: &Path, home: Option<&Path>) -> Result<PathBuf> {
    let meta = std::fs::symlink_metadata(at).map_err(|e| format!("{}: {e}", at.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{} is a link — name the folder it leads to with --in", at.display()));
    }
    if !meta.is_dir() {
        return Err(format!("{} isn't a folder", at.display()));
    }
    let real = std::fs::canonicalize(at).map_err(|e| format!("{}: {e}", at.display()))?;
    let home = home.map(|h| std::fs::canonicalize(h).unwrap_or_else(|_| h.to_path_buf()));
    if real.parent().is_none() || home.is_some_and(|h| h.starts_with(&real)) {
        return Err(format!("{} holds the home folder — not a place of build folders", real.display()));
    }
    Ok(real)
}

/// What the repository's checkouts said (`claimed`).
#[derive(Clone, Debug, Default, PartialEq)]
struct Claims {
    /// Where each builds (target and build folders, real paths).
    dirs: Vec<PathBuf>,
    /// The repository's own package (its workspace root's — else every member): a build folder that built one
    /// is this repository's.
    own: Vec<String>,
}

/// Where each checkout of the repository at `root` builds (`git worktree list`, each asked `cargo metadata`) and
/// its own package — Err: the first that didn't say (then no folder can be told unclaimed).
fn claimed(root: &Path) -> std::result::Result<Claims, PathBuf> {
    let list = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .ok_or_else(|| root.to_path_buf())?;
    let checkouts = String::from_utf8_lossy(&list.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .map(PathBuf::from)
        .filter(|c| c.join("Cargo.toml").is_file())
        .collect::<Vec<_>>();
    let mut out = Claims::default();
    for c in checkouts {
        let mut cargo = Command::new("cargo");
        cargo.args(["metadata", "--no-deps", "--offline", "--format-version", "1"]).current_dir(&c);
        // (a test's stand-in says its own folders, whatever the run around it sets)
        #[cfg(test)]
        cargo
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR")
            .env_remove("CARGO_BUILD_BUILD_DIR");
        let said = cargo
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok());
        let Some(said) = said else { return Err(c) };
        for key in ["target_directory", "build_directory"] {
            if let Some(d) = said.get(key).and_then(|d| d.as_str()) {
                out.dirs.push(std::fs::canonicalize(d).unwrap_or_else(|_| PathBuf::from(d)));
            }
        }
        let root_manifest =
            said.get("workspace_root").and_then(|r| r.as_str()).map(|r| Path::new(r).join("Cargo.toml"));
        let packages: Vec<(&str, &str)> = said
            .get("packages")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
            .filter_map(|p| Some((p.get("name")?.as_str()?, p.get("manifest_path")?.as_str()?)))
            .collect();
        let root_pkg: Vec<&str> = packages
            .iter()
            .filter(|(_, m)| root_manifest.as_deref() == Some(Path::new(m)))
            .map(|p| p.0)
            .collect();
        let own = if root_pkg.is_empty() { packages.iter().map(|p| p.0).collect() } else { root_pkg };
        out.own.extend(own.into_iter().map(String::from));
    }
    out.dirs.sort();
    out.dirs.dedup();
    out.own.sort();
    out.own.dedup();
    Ok(out)
}

/// What a pass prunes besides orphaned objects, and whether it removes or only lists.
#[derive(Clone, Debug)]
struct Opts {
    /// Incremental caches and old test binaries not built for this long (None: orphaned objects only).
    age: Option<Duration>,
    /// Build folders no checkout claims, untouched for `age`, that built this repository's package, removed whole.
    whole: bool,
    /// With `whole`: another project's too (none of this repository's packages built there).
    others: bool,
    /// What every checkout said — Err: the checkout that didn't say.
    claimed: std::result::Result<Claims, PathBuf>,
    /// Removed; else only counted, as it would be.
    remove: bool,
}

/// What a pass did (or, listing, would do).
#[derive(Clone, Debug, Default, PartialEq)]
struct Pruned {
    /// Bytes freed (allocated blocks of what had no other link).
    freed: u64,
    objects: usize,
    caches: usize,
    binaries: usize,
    folders: usize,
    /// Profile folders left alone, or left partway: a build held their lock.
    busy: usize,
    /// Why no folder was removed whole though asked (a checkout that didn't say where it builds).
    unsure: Option<String>,
    /// What it meant to remove and left, each with why (a folder not cargo's or with no lock file, a removal that
    /// failed) — never dropped in silence.
    left: Vec<String>,
    /// Each build folder it pruned and what of it.
    each: Vec<(PathBuf, String)>,
}

impl Pruned {
    fn add(&mut self, o: Pruned) {
        self.freed += o.freed;
        self.objects += o.objects;
        self.caches += o.caches;
        self.binaries += o.binaries;
        self.folders += o.folders;
        self.busy += o.busy;
        self.left.extend(o.left);
        self.each.extend(o.each);
    }

    /// What it did, in a few words: `9.3 GB — 7,334 old objects, 2 old test binaries` — None when nothing.
    fn said(&self) -> Option<String> {
        let n = |k: usize, one: &str, many: &str| match k {
            0 => None,
            1 => Some(format!("1 {one}")),
            k => Some(format!("{} {many}", thousands(k))),
        };
        let parts: Vec<String> = [
            n(self.objects, "old object", "old objects"),
            n(self.caches, "stale incremental cache", "stale incremental caches"),
            n(self.binaries, "old test binary", "old test binaries"),
            n(self.folders, "folder no checkout builds in", "folders no checkout builds in"),
        ]
        .into_iter()
        .flatten()
        .collect();
        if parts.is_empty() {
            return None;
        }
        Some(format!("{} — {}", mem(self.freed), parts.join(", ")))
    }
}

/// `7334` → `7,334`.
fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Bytes as tarae says them (`procs::mem`): `9.3 GB`, `512 MB`, `4 KB`.
fn mem(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= (1u64 << 30) as f64 {
        format!("{:.1} GB", b / (1u64 << 30) as f64)
    } else if b >= (1u64 << 20) as f64 {
        format!("{:.0} MB", b / (1u64 << 20) as f64)
    } else {
        format!("{:.0} KB", b / 1024.0)
    }
}

/// A time by its largest unit, as tarae says an age: `3d`, `5h`, `12m`.
fn ago(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        s if s >= 24 * 3600 => format!("{}d", s / (24 * 3600)),
        s if s >= 3600 => format!("{}h", s / 3600),
        s => format!("{}m", s / 60),
    }
}

/// `path` with the home folder as `~`.
fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// What a pass did or would do, a line a build folder, then the sum.
fn report(p: &Pruned, removed: bool, home: Option<&Path>) -> String {
    let mut out = String::new();
    for (at, said) in &p.each {
        out += &format!("{}: {said}\n", tilde(at, home));
    }
    for l in &p.left {
        out += &format!("left: {l}\n");
    }
    if let Some(u) = &p.unsure {
        out += &format!("{u}\n");
    }
    match p.busy {
        0 => {}
        1 => out += "1 profile left while a build holds its lock\n",
        n => out += &format!("{n} profiles left while builds hold their locks\n"),
    }
    out += &match (p.said(), removed) {
        (Some(s), true) => format!("Freed {s}\n"),
        (Some(s), false) => format!("Would free {s} — --yes removes it\n"),
        (None, _) => "Nothing old to prune\n".to_string(),
    };
    out
}

/// One pass over every build folder in `place` (a real path).
fn pass(place: &Path, opts: &Opts, now: SystemTime) -> Pruned {
    let mut out = Pruned::default();
    if opts.whole
        && let Err(dir) = &opts.claimed
    {
        out.unsure = Some(format!("{} didn't say where it builds — no folder removed whole", dir.display()));
    }
    // (its real path; every step below it through no link)
    if let Ok(under) = Dir::root(place) {
        walk(&under, Path::new(""), DEPTH, opts, now, &mut out);
    }
    out
}

/// Down `left` more folders from `dir` (`rel` below the place) to the build folders, each pruned (`one`) — and a
/// folder an earlier pass set aside, finished (or said, when it won't go).
fn walk(dir: &Dir, rel: &Path, left: usize, opts: &Opts, now: SystemTime, out: &mut Pruned) {
    for name in dir.folders() {
        if left > 1 {
            if let Ok(sub) = dir.sub(&name) {
                walk(&sub, &rel.join(&name), left - 1, opts, now, out);
            }
        } else if name.to_string_lossy().starts_with(ASIDE) {
            let at = dir.at.join(&name);
            match dir.tree(&name, opts.remove) {
                Ok(freed) => {
                    out.freed += freed;
                    out.each.push((at, format!("{} — set aside by an earlier pass", mem(freed))));
                }
                Err(e) => {
                    out.left.push(format!("{}: set aside by an earlier pass, won't go — {e}", at.display()))
                }
            }
        } else {
            out.add(one(dir, &rel.join(&name), &name, opts, now));
        }
    }
}

/// `rel` is named as cargo names a build folder under its `{workspace-path-hash}`: two hex digits, then fourteen.
fn cargo_named(rel: &Path) -> bool {
    let hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'));
    let parts: Vec<Option<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
    matches!(parts.as_slice(), [Some(a), Some(b)] if hex(a, 2) && hex(b, 14))
}

/// The first of `FOREIGN` in the build folder `b` or a folder of it (`.git`, `debug/Cargo.toml`), as named — not
/// followed: a link by that name counts.
fn foreign(b: &Dir) -> Option<String> {
    let in_dir = |d: &Dir| FOREIGN.iter().find(|f| d.stat(OsStr::new(f)).is_ok()).map(|f| f.to_string());
    in_dir(b).or_else(|| {
        b.folders().into_iter().find_map(|n| {
            let sub = b.sub(&n).ok()?;
            in_dir(&sub).map(|f| format!("{}/{f}", n.to_string_lossy()))
        })
    })
}

/// The first of `own` packages the build folder `b` built: a profile's `.fingerprint/<name>-<16 hex>`.
fn built(b: &Dir, own: &[String]) -> Option<String> {
    profiles(b).into_iter().find_map(|(_, p)| {
        let names = p.sub(OsStr::new(".fingerprint")).ok()?.names().ok()?;
        own.iter()
            .find(|pkg| {
                names.iter().any(|n| {
                    n.to_str()
                        .and_then(|n| n.strip_prefix(pkg.as_str()))
                        .and_then(|rest| rest.strip_prefix('-'))
                        .is_some_and(|h| h.len() == 16 && h.bytes().all(|c| c.is_ascii_hexdigit()))
                })
            })
            .cloned()
    })
}

/// Tests: a step run at a place (a lock's path, a folder set aside).
#[cfg(test)]
type Hook = std::cell::RefCell<Option<Box<dyn FnMut(&Path)>>>;

#[cfg(test)]
thread_local! {
    /// Tests: each hold's slice (zero: let go before every removal).
    static SLICE_FOR: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
    /// Tests: run while a hold is let go, before it's taken again (the lock's path) — a build that takes it then.
    static BETWEEN: Hook = const { std::cell::RefCell::new(None) };
    /// Tests: run once a folder is set aside and its locks let go, before it's removed (its path there).
    static ASIDE_THEN: Hook = const { std::cell::RefCell::new(None) };
}

fn slice() -> Duration {
    #[cfg(test)]
    if let Some(s) = SLICE_FOR.get() {
        return s;
    }
    SLICE
}

/// A profile folder's lock, held — let go and taken again every `SLICE`; lost for good once a retake fails.
struct Hold {
    file: File,
    since: Instant,
    lost: bool,
    #[cfg_attr(not(test), allow(dead_code))]
    at: PathBuf,
}

/// Let go explicitly: closing our descriptor alone leaves the lock with any copy a child forked meanwhile holds
/// until it execs (`cargo metadata`, or another test's child) — `LOCK_UN` frees it for every copy.
impl Drop for Hold {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// What taking a profile's lock found.
enum Took {
    Held(Hold),
    /// A build holds it (or it can't be taken).
    Busy,
    /// No lock file there: not the layout it knows.
    None,
}

impl Hold {
    /// The lock file in `profile` taken, through no link, if no build holds it.
    fn take(profile: &Dir) -> Took {
        let Ok(file) = profile.open_file(OsStr::new(LOCK)) else { return Took::None };
        match file.try_lock() {
            Ok(()) => {
                Took::Held(Hold { file, since: Instant::now(), lost: false, at: profile.at.join(LOCK) })
            }
            Err(_) => Took::Busy,
        }
    }

    /// Before each removal: past its slice, the lock let go for a moment and taken again — false once a build
    /// took it meanwhile, and from then on (what's left there is the next pass's).
    fn breathe(&mut self) -> bool {
        if self.lost {
            return false;
        }
        if self.since.elapsed() < slice() {
            return true;
        }
        let _ = self.file.unlock();
        std::thread::sleep(PAUSE);
        #[cfg(test)]
        BETWEEN.with(|b| {
            if let Some(f) = b.borrow_mut().as_mut() {
                f(&self.at)
            }
        });
        self.lost = self.file.try_lock().is_err();
        self.since = Instant::now();
        !self.lost
    }
}

/// Older than `age` at `now` (a time to come counts as now).
fn older(t: SystemTime, now: SystemTime, age: Duration) -> bool {
    now.duration_since(t).is_ok_and(|d| d >= age)
}

/// The profile folders of the build folder `b` (their names below it): its folders with a `deps/` folder in them,
/// and theirs one down (a target triple's). Links are never gone through.
fn profiles(b: &Dir) -> Vec<(PathBuf, Dir)> {
    let is = |d: &Dir| d.stat(OsStr::new("deps")).is_ok_and(|m| m.kind == Kind::Folder);
    let mut out = Vec::new();
    for name in b.folders() {
        let Ok(p) = b.sub(&name) else { continue };
        if is(&p) {
            out.push((PathBuf::from(&name), p));
            continue;
        }
        for inner in p.folders() {
            if let Ok(q) = p.sub(&inner)
                && is(&q)
            {
                out.push((Path::new(&name).join(&inner), q));
            }
        }
    }
    out
}

/// A name for a folder set aside, new on this machine.
fn aside_name() -> OsString {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    OsString::from(format!("{ASIDE}{}-{n}", std::process::id()))
}

/// The build folder `name` in `parent` (`rel` below the place), each profile folder pruned while its lock is held
/// — removed whole only while it holds every one (`Opts.whole`), and only as the module's doc says.
fn one(parent: &Dir, rel: &Path, name: &OsStr, opts: &Opts, now: SystemTime) -> Pruned {
    let mut out = Pruned::default();
    let Ok(b) = parent.sub(name) else { return out };
    let mut held = Vec::new();
    let mut lockless = Vec::new();
    let mut busy = 0;
    for (prel, p) in profiles(&b) {
        match Hold::take(&p) {
            Took::Held(h) => held.push((p, h)),
            Took::Busy => busy += 1,
            Took::None => lockless.push(prel),
        }
    }
    out.busy = busy;
    if let (true, Some(age), Ok(claims)) = (opts.whole, opts.age, &opts.claimed) {
        let claimed = claims.dirs.iter().any(|c| c.starts_with(&b.at) || b.at.starts_with(c));
        let mut times: Vec<Meta> = b.stat_self().into_iter().collect();
        times.extend(b.folders().iter().filter_map(|n| b.stat(n).ok()));
        for (p, _) in &held {
            times.extend(
                ["deps", "incremental", ".fingerprint"].iter().filter_map(|n| p.stat(OsStr::new(n)).ok()),
            );
        }
        let newest = times.iter().map(|m| m.modified).max();
        if !claimed && newest.is_some_and(|t| older(t, now, age)) {
            let at = b.at.display();
            let untouched = newest.and_then(|t| now.duration_since(t).ok()).map(ago).unwrap_or_default();
            let ours = built(&b, &claims.own);
            if !cargo_named(rel) {
                out.left.push(format!(
                    "{at}: not named as cargo names a build folder (xx/ and 14 hex digits) — not removed whole"
                ));
            } else if let Some(f) = foreign(&b) {
                out.left.push(format!("{at}: holds {f}, which cargo never makes — not removed whole"));
            } else if let Some(p) = lockless.first() {
                out.left.push(format!("{at}: {} has no lock file — not removed whole", p.display()));
            } else if ours.is_none() && !opts.others {
                out.each.push((
                    b.at.clone(),
                    format!(
                        "kept whole — no checkout of this repository builds here, and it built none of its \
                         packages: another project's, as far as this can tell (--others removes it), untouched \
                         {untouched}"
                    ),
                ));
            } else if busy == 0 && !held.is_empty() {
                let whose = match &ours {
                    Some(pkg) => format!("this repository's (it built {pkg})"),
                    None => "another project's, as far as this can tell".to_string(),
                };
                let line = |freed| {
                    format!(
                        "{} — no checkout of this repository builds here; {whose}, untouched {untouched}",
                        mem(freed)
                    )
                };
                if !opts.remove {
                    match parent.tree(name, false) {
                        Ok(freed) => {
                            (out.freed, out.folders) = (freed, 1);
                            out.each.push((b.at.clone(), line(freed)));
                        }
                        Err(e) => out.left.push(format!("{at}: {e}")),
                    }
                    return out;
                }
                // (aside while every lock is held — one rename in its own folder — then the locks let go: a build
                // starting here makes a new folder, never waits for the removal)
                let aside = aside_name();
                if let Err(e) = parent.rename(name, &aside) {
                    out.left.push(format!("{at}: not set aside — {e}"));
                } else {
                    drop(held);
                    #[cfg(test)]
                    ASIDE_THEN.with(|t| {
                        if let Some(f) = t.borrow_mut().as_mut() {
                            f(&parent.at.join(&aside))
                        }
                    });
                    match parent.tree(&aside, true) {
                        Ok(freed) => {
                            (out.freed, out.folders) = (freed, 1);
                            out.each.push((b.at.clone(), line(freed)));
                        }
                        Err(e) => out.left.push(format!("{at}: removing it failed partway — {e}")),
                    }
                    return out;
                }
            }
        }
    }
    for (p, hold) in &mut held {
        let deps = p.sub(OsStr::new("deps")).ok();
        if let (Some(age), Some(d)) = (opts.age, &deps) {
            binaries(d, age, now, opts.remove, hold, &mut out);
        }
        if let Some(d) = &deps {
            objects(d, now, opts.remove, hold, &mut out);
        }
        if let (Some(age), Ok(i)) = (opts.age, p.sub(OsStr::new("incremental"))) {
            caches(&i, age, now, opts.remove, hold, &mut out);
        }
        if hold.lost {
            out.busy += 1;
        }
    }
    if let Some(said) = out.said() {
        out.each.push((b.at.clone(), said));
    }
    out
}

/// `name` in `dir` removed — or, listing, taken as removed.
fn gone(dir: &Dir, name: &OsStr, remove: bool) -> bool {
    !remove || dir.remove(name).is_ok()
}

/// A file in `deps/` as the pass reads it.
struct Entry {
    name: OsString,
    text: String,
    meta: Meta,
}

/// The plain files in `dir` (links left out).
fn files_in(dir: &Dir) -> Vec<Entry> {
    dir.entries()
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, m)| m.kind == Kind::File)
        .filter_map(|(name, meta)| Some(Entry { text: name.to_str()?.to_string(), name, meta }))
        .collect()
}

/// The unit a file in `deps/` is of: `tarae-507f1e66af62f2c2` for its binary, its `.d`, its objects
/// (`tarae-507f1e66af62f2c2.<cgu>.rcgu.o`) and `libtarae-507f1e66af62f2c2.rlib`.
fn unit_of(name: &str) -> &str {
    let stem = name.split('.').next().unwrap_or(name);
    match name.contains('.') {
        true => stem
            .strip_prefix("lib")
            .filter(|_| !name.ends_with(".o") && !name.ends_with(".d"))
            .unwrap_or(stem),
        false => stem,
    }
}

/// Orphaned objects in `deps` (the module's doc).
fn objects(deps: &Dir, now: SystemTime, remove: bool, hold: &mut Hold, out: &mut Pruned) {
    let all = files_in(deps);
    let (objs, rest): (Vec<&Entry>, Vec<&Entry>) = all.iter().partition(|e| e.text.ends_with(".rcgu.o"));
    let outputs: HashSet<&str> =
        rest.iter().filter(|e| !e.text.ends_with(".d")).map(|e| unit_of(&e.text)).collect();
    let mut names: HashMap<u64, u64> = HashMap::new();
    for o in &objs {
        *names.entry(o.meta.ino).or_default() += 1;
    }
    let elsewhere = |o: &Entry| o.meta.links > names[&o.meta.ino];
    let linked: HashSet<&str> = objs.iter().filter(|o| elsewhere(o)).map(|o| unit_of(&o.text)).collect();
    let mut gone_names: HashMap<u64, u64> = HashMap::new();
    for o in objs {
        let unit = unit_of(&o.text);
        let orphan = !elsewhere(o) && (linked.contains(unit) || !outputs.contains(unit));
        if !orphan || !older(o.meta.modified, now, GRACE) {
            continue;
        }
        if !hold.breathe() {
            return;
        }
        if gone(deps, &o.name, remove) {
            out.objects += 1;
            let n = gone_names.entry(o.meta.ino).or_default();
            *n += 1;
            if *n == names[&o.meta.ino] {
                out.freed += o.meta.bytes;
            }
        }
    }
}

/// Old test binaries in `deps`: `<name>-<16 hex>` with no extension, older than `age`, another of that name newer.
fn binaries(deps: &Dir, age: Duration, now: SystemTime, remove: bool, hold: &mut Hold, out: &mut Pruned) {
    let all = files_in(deps);
    let stem = |n: &str| -> Option<String> {
        let (s, h) = n.rsplit_once('-')?;
        let hex = h.len() == 16 && h.bytes().all(|c| c.is_ascii_hexdigit()) && !n.contains('.');
        hex.then(|| s.to_string())
    };
    let mut newest: HashMap<String, SystemTime> = HashMap::new();
    for e in &all {
        if let Some(s) = stem(&e.text) {
            let t = newest.entry(s).or_insert(e.meta.modified);
            *t = (*t).max(e.meta.modified);
        }
    }
    for e in &all {
        let Some(s) = stem(&e.text) else { continue };
        if !older(e.meta.modified, now, age) || e.meta.modified >= newest[&s] {
            continue;
        }
        if !hold.breathe() {
            return;
        }
        if gone(deps, &e.name, remove) {
            out.binaries += 1;
            if e.meta.links == 1 {
                out.freed += e.meta.bytes;
            }
            let d = OsString::from(format!("{}.d", e.text));
            if let Ok(m) = deps.stat(&d)
                && m.kind == Kind::File
                && gone(deps, &d, remove)
            {
                out.freed += m.bytes;
            }
        }
    }
}

/// Stale incremental caches in `incr`: a crate's folder whose newest entry is older than `age`.
fn caches(incr: &Dir, age: Duration, now: SystemTime, remove: bool, hold: &mut Hold, out: &mut Pruned) {
    for name in incr.folders() {
        let Ok(dir) = incr.sub(&name) else { continue };
        let mut times: Vec<SystemTime> = dir.stat_self().map(|m| m.modified).into_iter().collect();
        times.extend(dir.entries().unwrap_or_default().iter().map(|(_, m)| m.modified));
        if !times.into_iter().max().is_some_and(|t| older(t, now, age)) {
            continue;
        }
        if !hold.breathe() {
            return;
        }
        match incr.tree(&name, remove) {
            Ok(freed) => {
                out.caches += 1;
                out.freed += freed;
            }
            Err(e) => out.left.push(format!("{}: won't go — {e}", incr.at.join(&name).display())),
        }
    }
}

/// What a name is, not followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    File,
    Folder,
    Link,
    Other,
}

/// What `Dir::stat` reads of a name, not followed.
#[derive(Clone, Copy, Debug)]
struct Meta {
    kind: Kind,
    ino: u64,
    links: u64,
    /// Its allocated blocks, in bytes.
    bytes: u64,
    modified: SystemTime,
}

#[allow(clippy::unnecessary_cast)] // (the field types differ between systems)
fn meta_of(st: &libc::stat) -> Meta {
    let kind = match st.st_mode & libc::S_IFMT {
        libc::S_IFREG => Kind::File,
        libc::S_IFDIR => Kind::Folder,
        libc::S_IFLNK => Kind::Link,
        _ => Kind::Other,
    };
    let since = Duration::new(st.st_mtime.max(0) as u64, st.st_mtime_nsec.clamp(0, 999_999_999) as u32);
    Meta {
        kind,
        ino: st.st_ino as u64,
        links: st.st_nlink as u64,
        bytes: st.st_blocks.max(0) as u64 * 512,
        modified: std::time::UNIX_EPOCH + since,
    }
}

fn cstr(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a NUL in a name"))
}

/// `ret` as a result: -1 is the OS's error, said with `what`.
fn said(ret: libc::c_int, what: &Path) -> io::Result<libc::c_int> {
    match ret {
        -1 => {
            let e = io::Error::last_os_error();
            Err(io::Error::new(e.kind(), format!("{}: {e}", what.display())))
        }
        n => Ok(n),
    }
}

/// A folder opened through no link, every step from it on its descriptor (the module's doc).
struct Dir {
    fd: OwnedFd,
    /// Where it was when opened — for what's said, never walked again.
    at: PathBuf,
}

impl Dir {
    /// The folder at `path`, a real path (its last step not followed).
    fn root(path: &Path) -> io::Result<Dir> {
        let c = cstr(path.as_os_str())?;
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: a path live for the call; the descriptor is ours from here.
        let fd = said(unsafe { libc::open(c.as_ptr(), flags) }, path)?;
        Ok(Dir { fd: unsafe { OwnedFd::from_raw_fd(fd) }, at: path.to_path_buf() })
    }

    /// `name` here opened as a folder, not followed — Err for a link or anything else.
    fn sub(&self, name: &OsStr) -> io::Result<Dir> {
        let (c, at) = (cstr(name)?, self.at.join(name));
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: a descriptor of ours and a name, live for the call; the new descriptor is ours from here.
        let fd = said(unsafe { libc::openat(self.fd.as_raw_fd(), c.as_ptr(), flags) }, &at)?;
        Ok(Dir { fd: unsafe { OwnedFd::from_raw_fd(fd) }, at })
    }

    /// The names in it (`.` and `..` left out), read from its start.
    fn names(&self) -> io::Result<Vec<OsString>> {
        // (a copy for the stream: `closedir` closes what `fdopendir` was given)
        // SAFETY: a descriptor of ours; the copy is the stream's, closed with it.
        let fd = said(unsafe { libc::fcntl(self.fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) }, &self.at)?;
        let dir = unsafe { libc::fdopendir(fd) };
        if dir.is_null() {
            let e = io::Error::last_os_error();
            // SAFETY: the copy, never the stream's.
            unsafe { libc::close(fd) };
            return Err(e);
        }
        let mut out = Vec::new();
        // SAFETY: a stream of ours until `closedir`; each entry read before the next `readdir`.
        unsafe {
            // (the copy shares the offset: from the start, whatever an earlier read left)
            libc::rewinddir(dir);
            loop {
                let e = libc::readdir(dir);
                if e.is_null() {
                    break;
                }
                let name = CStr::from_ptr((*e).d_name.as_ptr()).to_bytes();
                if name != b"." && name != b".." {
                    out.push(OsStr::from_bytes(name).to_os_string());
                }
            }
            libc::closedir(dir);
        }
        Ok(out)
    }

    /// Each name in it with what it is, not followed (one gone meanwhile left out).
    fn entries(&self) -> io::Result<Vec<(OsString, Meta)>> {
        Ok(self.names()?.into_iter().filter_map(|n| self.stat(&n).ok().map(|m| (n, m))).collect())
    }

    /// The folders in it (links left out), sorted.
    fn folders(&self) -> Vec<OsString> {
        let mut out: Vec<OsString> = self
            .entries()
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, m)| m.kind == Kind::Folder)
            .map(|(n, _)| n)
            .collect();
        out.sort();
        out
    }

    /// What `name` here is, its links, size and time — not followed.
    fn stat(&self, name: &OsStr) -> io::Result<Meta> {
        let c = cstr(name)?;
        // SAFETY: zeroed out-struct, filled by the call; a descriptor of ours and a name, live for the call.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let ret =
            unsafe { libc::fstatat(self.fd.as_raw_fd(), c.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) };
        said(ret, &self.at.join(name))?;
        Ok(meta_of(&st))
    }

    /// This folder's own.
    fn stat_self(&self) -> io::Result<Meta> {
        // SAFETY: zeroed out-struct, filled by the call; a descriptor of ours.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        said(unsafe { libc::fstat(self.fd.as_raw_fd(), &mut st) }, &self.at)?;
        Ok(meta_of(&st))
    }

    /// File `name` opened to read (a lock to take on it), through no link — Err when it isn't a plain file.
    fn open_file(&self, name: &OsStr) -> io::Result<File> {
        let (c, at) = (cstr(name)?, self.at.join(name));
        let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
        // SAFETY: a descriptor of ours and a name, live for the call; the new descriptor is ours from here.
        let fd = said(unsafe { libc::openat(self.fd.as_raw_fd(), c.as_ptr(), flags) }, &at)?;
        let f = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        match f.metadata()?.file_type().is_file() {
            true => Ok(f),
            false => Err(io::Error::other(format!("{} isn't a plain file", at.display()))),
        }
    }

    /// `name` here unlinked (a link as itself) — a folder refused.
    fn remove(&self, name: &OsStr) -> io::Result<()> {
        let c = cstr(name)?;
        // SAFETY: a descriptor of ours and a name, live for the call.
        said(unsafe { libc::unlinkat(self.fd.as_raw_fd(), c.as_ptr(), 0) }, &self.at.join(name)).map(drop)
    }

    /// Empty folder `name` here removed.
    fn remove_folder(&self, name: &OsStr) -> io::Result<()> {
        let c = cstr(name)?;
        // SAFETY: a descriptor of ours and a name, live for the call.
        let ret = unsafe { libc::unlinkat(self.fd.as_raw_fd(), c.as_ptr(), libc::AT_REMOVEDIR) };
        said(ret, &self.at.join(name)).map(drop)
    }

    /// `name` here renamed `to` here (`renameat` on this one descriptor: neither is followed) — Err when anything
    /// is at `to` already.
    fn rename(&self, name: &OsStr, to: &OsStr) -> io::Result<()> {
        if self.stat(to).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{}: there already", to.display()),
            ));
        }
        let (c, t, fd) = (cstr(name)?, cstr(to)?, self.fd.as_raw_fd());
        // SAFETY: a descriptor of ours and two names, live for the call.
        said(unsafe { libc::renameat(fd, c.as_ptr(), fd, t.as_ptr()) }, &self.at.join(name)).map(drop)
    }

    /// Folder `name` here and everything in it: what it frees (the files with no link elsewhere) — removed when
    /// `remove`, each folder opened through no link from the one before and emptied on its descriptor (a link
    /// inside goes as itself, never gone through; `name` itself a link: refused, left).
    fn tree(&self, name: &OsStr, remove: bool) -> io::Result<u64> {
        let sub = self.sub(name)?;
        let mut freed = 0;
        sub.empty(0, remove, &mut freed)?;
        if remove {
            self.remove_folder(name)?;
        }
        Ok(freed)
    }

    /// Everything in this folder (`tree`).
    fn empty(&self, depth: usize, remove: bool, freed: &mut u64) -> io::Result<()> {
        if depth >= TREE_DEPTH {
            return Err(io::Error::other(format!("{}: deeper than {TREE_DEPTH} folders", self.at.display())));
        }
        for (name, m) in self.entries()? {
            if m.kind == Kind::Folder {
                self.sub(&name)?.empty(depth + 1, remove, freed)?;
                if remove {
                    self.remove_folder(&name)?;
                }
                continue;
            }
            if m.kind == Kind::File && m.links == 1 {
                *freed += m.bytes;
            }
            if remove {
                self.remove(&name)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
