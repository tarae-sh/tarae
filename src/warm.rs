//! A new session's first build starts warm (gitcrate B6, the user's reversal of Q6 on 2026-11-07: Google's Agent
//! Compute Engine starts each sandbox from a pre-warmed snapshot cloned copy-on-write; this is its local form): where
//! the new checkout's build folder is its own (one per checkout, or inside it), the main checkout's profile folder
//! its pack names (`[builds.warm]`) is cloned into it — only what no checkout builds.
//!
//! - **Only the main checkout's, only into a new checkout's**: the source is the repository's main checkout, never a
//!   session's (its runs write there); the destination is never the main checkout's build folder nor in it.
//! - **Under the tool's lock**, held for the clone alone (`clonefile` of a folder: a fraction of a second) — a unit
//!   half written is never copied. Busy past `TRIES`: nothing copied, said.
//! - **The checkout's own units go before the tool sees them** (B3's hazard: a path package's freshness rests on file
//!   times alone, and a checkout whose files are older than the main checkout's units would use its binary — its
//!   snapshots, its fixtures, its repository). Its own packages are the lock file's entries with no source (members
//!   and path dependencies outside the workspace) and the members the tool's ask names; each has an entry
//!   `<package>-<hash>` in the `units` folder, and the hash ends the name of every entry of that unit in every folder
//!   (a custom library name too) — those go from the staged copy.
//! - **Into place only where nothing is**: the staged profile (`<profile>.tarae-warm-<pid>-<n>`) renamed in
//!   exclusively; a build that started first keeps its cold folder and the copy goes. A staged copy a gone tarae left
//!   is removed by the next start into that build folder — none comes to a checkout's own, so there it lives until
//!   the checkout goes (inside it) or its session's end removes the folder (per checkout); `build-prune` leaves it
//!   (no lock in it: not a profile it knows).
//! - **Clones only**: macOS's `clonefile` (APFS), Linux's `FICLONE` a file at a time, its times kept (btrfs, XFS); a
//!   file system that clones nothing gets nothing — a full copy would be disk for seconds saved.
//!
//! On a worker, after the checkout is made: the opening never waits for it. Said once in the log, never a toast.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::builds::{Builds, Tool, Warm};

/// How many times the tool's lock is asked for, `PAUSE` apart, before the copy is let go: 2 s — a set's other
/// children's clones (each holding it a fraction of a second) waited out, a build of the home's not.
const TRIES: usize = 40;
const PAUSE: Duration = Duration::from_millis(50);

/// What a staged copy's name has after the profile's: `debug.tarae-warm-<pid>-<n>`.
const STAGED: &str = ".tarae-warm-";

/// A staged copy's name is unique in this process with this.
static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The main checkout's side: its top, its build folder and the profile folder in it (real paths), the tool's answer
/// there.
struct Source {
    home: PathBuf,
    profile: PathBuf,
    answer: Value,
}

/// The main checkout of the repository `any` is a checkout of, and where it builds — Err: why there's nothing to start
/// from. `new` is the new checkout (never the home).
fn source(b: &Builds, tool: &Tool, warm: &Warm, any: &Path, new: &Path) -> Result<Source, String> {
    let common = crate::worktree::common_dir(any)?;
    let home = crate::worktree::main_checkout(&common, new).ok_or("no main checkout to start from")?;
    let ((_, build), answer) =
        b.ask_whole(tool, &home).ok_or("the main checkout's build folder isn't known")?;
    let build =
        std::fs::canonicalize(&build).map_err(|_| "the main checkout has built nothing".to_string())?;
    let profile = build.join(&warm.profile);
    if !profile.join(&warm.lock).is_file() {
        return Err("the main checkout has built nothing".into());
    }
    Ok(Source { home, profile, answer })
}

/// The checkout's own packages: the lock file's entries with no source in each of `tops`, and the members each of
/// `answers` names (`members`).
fn own_names(warm: &Warm, tops: &[&Path], answers: &[&Value]) -> HashSet<String> {
    let (file, table, key, without) = &warm.lock_file;
    let mut names = HashSet::new();
    for top in tops {
        let Ok(text) = std::fs::read_to_string(top.join(file)) else { continue };
        let Ok(v) = toml::from_str::<toml::Table>(&text) else { continue };
        let entries = v.get(table).and_then(toml::Value::as_array).into_iter().flatten();
        for e in entries.filter(|e| e.get(without).is_none()) {
            names.extend(e.get(key).and_then(toml::Value::as_str).map(str::to_string));
        }
    }
    if let Some((list, key)) = &warm.members {
        for a in answers {
            let listed = a[list.as_str()].as_array().into_iter().flatten();
            names.extend(listed.filter_map(|p| p[key.as_str()].as_str().map(str::to_string)));
        }
    }
    names
}

/// A unit entry's hash: what follows its name's last `-` before its first `.` (`libfoo_bar-1a2b.rmeta` → `1a2b`),
/// hex digits only — and the name before it.
fn unit_of(entry: &str) -> Option<(&str, &str)> {
    let stem = entry.split('.').next()?;
    let (name, hash) = stem.rsplit_once('-')?;
    (!name.is_empty() && !hash.is_empty() && hash.bytes().all(|c| c.is_ascii_hexdigit()))
        .then_some((name, hash))
}

/// The hashes of `names`' units in `profile` (its `units` folder's entries `<package>-<hash>`).
fn own_hashes(profile: &Path, warm: &Warm, names: &HashSet<String>) -> HashSet<String> {
    let entries = std::fs::read_dir(profile.join(&warm.units)).into_iter().flatten().flatten();
    entries
        .filter_map(|e| {
            let n = e.file_name().into_string().ok()?;
            let (name, hash) = unit_of(&n)?;
            names.contains(name).then(|| hash.to_string())
        })
        .collect()
}

/// Whether entry `name` (in any of the folders) is one of `hashes`' units.
fn is_own(name: &std::ffi::OsStr, hashes: &HashSet<String>) -> bool {
    name.to_str().and_then(unit_of).is_some_and(|(_, h)| hashes.contains(h))
}

/// `names`' units taken out of the profile folder `profile` (a staged copy): every entry of theirs in each folder,
/// a folder whole (no link followed). How many went.
fn drop_own(profile: &Path, warm: &Warm, names: &HashSet<String>) -> Result<usize, String> {
    let hashes = own_hashes(profile, warm, names);
    let mut gone = 0;
    for f in &warm.folders {
        let Ok(entries) = std::fs::read_dir(profile.join(f)) else { continue };
        for e in entries.flatten().filter(|e| is_own(&e.file_name(), &hashes)) {
            let p = e.path();
            let removed = match e.file_type() {
                Ok(t) if t.is_dir() => std::fs::remove_dir_all(&p),
                _ => std::fs::remove_file(&p),
            };
            removed.map_err(|err| format!("{}: {err}", p.display()))?;
            gone += 1;
        }
    }
    Ok(gone)
}

/// What the profile's folders hold but `names`' units, on disk — measured until `until` (false: not all of it).
fn shared_size(profile: &Path, warm: &Warm, names: &HashSet<String>, until: Instant) -> (u64, bool) {
    let hashes = own_hashes(profile, warm, names);
    let (mut total, mut whole) = (0, true);
    for f in &warm.folders {
        let Ok(entries) = std::fs::read_dir(profile.join(f)) else { continue };
        for e in entries.flatten().filter(|e| !is_own(&e.file_name(), &hashes)) {
            if crate::clock::now() >= until {
                return (total, false);
            }
            let (size, all) = crate::worktree::disk_size(&e.path(), until);
            (total, whole) = (total + size, whole && all);
        }
    }
    (total, whole)
}

/// The tool's lock, held while this lives — let go by `LOCK_UN` as it drops, never by the close alone: a lock is its
/// open file's, and a child forked meanwhile anywhere in tarae holds a copy of the descriptor until it execs
/// (`O_CLOEXEC` closes it only then), the lock with it — the home's build would wait on that copy.
pub(crate) struct Held(pub(crate) std::fs::File);

impl Drop for Held {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd as _;
        // SAFETY: flock on a descriptor of ours.
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// The tool's lock at `path` taken (`flock`, exclusive) — held while the `Held` lives. Err: another holds it still
/// after `TRIES`, or it isn't there.
fn locked(path: &Path) -> Result<Held, String> {
    use std::os::fd::AsRawFd as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| format!("its lock: {e}"))?;
    for n in 0..TRIES {
        if n > 0 {
            crate::event::pause(PAUSE);
        }
        // SAFETY: flock on a descriptor of ours, let go as `Held` drops.
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(Held(f));
        }
    }
    Err("the main checkout was building — nothing copied".into())
}

fn c_path(p: &Path) -> std::io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt as _;
    std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(std::io::Error::other)
}

/// Folder `from` cloned at `to` (not there yet): one `clonefile` — its files' times kept.
#[cfg(target_os = "macos")]
fn clone_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    // (`<sys/clonefile.h>`'s: a link at `from` cloned as a link)
    const CLONE_NOFOLLOW: u32 = 0x0001;
    let (f, t) = (c_path(from)?, c_path(to)?);
    // SAFETY: two NUL-terminated paths that live through the call.
    match unsafe { libc::clonefile(f.as_ptr(), t.as_ptr(), CLONE_NOFOLLOW) } {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

/// Folder `from` cloned at `to` (not there yet): its folders made, each file a `FICLONE` of its own with its times
/// kept (the tool compares units' times: one cloned later than what it was built from reads as rebuilt), a link
/// made again as it reads; anything else left out. A file system that won't clone: Err, never a copy.
#[cfg(not(target_os = "macos"))]
fn clone_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::fd::AsRawFd as _;
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
    std::fs::create_dir(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let t = e.file_type()?;
        if t.is_dir() {
            clone_tree(&src, &dst)?;
        } else if t.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&src)?, &dst)?;
        } else if t.is_file() {
            let input = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(&src)?;
            let m = input.metadata()?;
            let out = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(m.permissions().mode() & 0o7777)
                .open(&dst)?;
            // SAFETY: two descriptors of ours, open for the call.
            if unsafe { libc::ioctl(out.as_raw_fd(), libc::FICLONE as _, input.as_raw_fd()) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let times = [
                libc::timespec { tv_sec: m.atime() as _, tv_nsec: m.atime_nsec() as _ },
                libc::timespec { tv_sec: m.mtime() as _, tv_nsec: m.mtime_nsec() as _ },
            ];
            // SAFETY: a descriptor of ours and two timespecs that live through the call.
            if unsafe { libc::futimens(out.as_raw_fd(), times.as_ptr()) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

/// `from` renamed to `to` only where nothing is (Err `AlreadyExists` else).
fn rename_new(from: &Path, to: &Path) -> std::io::Result<()> {
    let (f, t) = (c_path(from)?, c_path(to)?);
    #[cfg(target_os = "macos")]
    // SAFETY: two NUL-terminated paths that live through the call.
    let r = unsafe { libc::renamex_np(f.as_ptr(), t.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(not(target_os = "macos"))]
    // SAFETY: two NUL-terminated paths that live through the call, both from the working folder's (absolute).
    let r = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            f.as_ptr(),
            libc::AT_FDCWD,
            t.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } as i32;
    match r {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

/// Whether the file system holding folder `dir` clones — macOS: APFS (`clonefile` works on every APFS volume);
/// Linux: a probe, once a file system (its device) in this process: two files of ours made new there, one cloned
/// from the other, both removed whatever came of it.
fn clones_in(dir: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::sandbox::filesystem_of(dir).is_some_and(|f| f.name == "apfs")
    }
    #[cfg(not(target_os = "macos"))]
    {
        use std::os::unix::fs::MetadataExt as _;
        static PROBED: std::sync::Mutex<Vec<(u64, bool)>> = std::sync::Mutex::new(Vec::new());
        let Ok(dev) = std::fs::metadata(dir).map(|m| m.dev()) else { return false };
        if let Some((_, ok)) =
            PROBED.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(d, _)| *d == dev)
        {
            return *ok;
        }
        let ok = probe(dir);
        PROBED.lock().unwrap_or_else(|e| e.into_inner()).push((dev, ok));
        ok
    }
}

#[cfg(not(target_os = "macos"))]
fn probe(dir: &Path) -> bool {
    use std::io::Write as _;
    use std::os::fd::AsRawFd as _;
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = |end: &str| dir.join(format!(".tarae-warm-probe-{}-{seq}-{end}", std::process::id()));
    let (a, b) = (name("a"), name("b"));
    let new = |p: &Path| std::fs::OpenOptions::new().write(true).create_new(true).open(p);
    let cloned = (|| -> std::io::Result<bool> {
        // (the source open for reading too: the kernel refuses a clone from one that isn't — EBADF, the probe said
        // "clones nothing" on a reflink XFS)
        let mut from = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&a)?;
        from.write_all(b"tarae")?;
        let to = new(&b)?;
        // SAFETY: two descriptors of ours, open for the call.
        Ok(unsafe { libc::ioctl(to.as_raw_fd(), libc::FICLONE as _, from.as_raw_fd()) } == 0)
    })();
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
    cloned.unwrap_or(false)
}

/// Whether process `pid` is there (one of another user's too).
fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 sends nothing, it only asks.
    let there = unsafe { libc::kill(pid, 0) } == 0;
    there || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The staged copies a gone tarae left in build folder `build` (`<profile>.tarae-warm-<pid>-<n>`, that pid not
/// there) removed — only a folder shaped so, never through a link; anything else is left alone.
fn clear_staged(build: &Path, profile: &str) {
    for e in std::fs::read_dir(build).into_iter().flatten().flatten() {
        let name = e.file_name();
        let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(profile)?.strip_prefix(STAGED)) else {
            continue;
        };
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
        let Some((pid, _)) = rest.split_once('-').filter(|(p, n)| digits(p) && digits(n)) else { continue };
        let Ok(pid) = pid.parse::<i32>() else { continue };
        if pid as u32 == std::process::id() || alive(pid) {
            continue;
        }
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// `p` as a real path as far as it's there, the rest as written.
fn real_beyond(p: &Path) -> PathBuf {
    let Some(there) = p.ancestors().find(|a| a.exists()) else { return p.to_path_buf() };
    let rest = p.strip_prefix(there).unwrap_or(Path::new(""));
    std::fs::canonicalize(there).map_or_else(|_| p.to_path_buf(), |r| r.join(rest))
}

/// The device a path's on.
fn device(p: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata(p).ok().map(|m| m.dev())
}

/// At the create card: whether a checkout of `top`'s repository made at `path` would start warm (`copy`), and what it
/// would share (None: not measured by `until`). None: it starts cold — no home build, a build folder that isn't its
/// own, another volume, a file system that clones nothing. On a worker.
pub fn planned(b: &Builds, tool: &Tool, top: &Path, path: &Path, until: Instant) -> Option<Option<u64>> {
    planned_with(b, tool, top, path, until, &clones_in)
}

fn planned_with(
    b: &Builds,
    tool: &Tool,
    top: &Path,
    path: &Path,
    until: Instant,
    clones: &dyn Fn(&Path) -> bool,
) -> Option<Option<u64>> {
    let warm = tool.warm()?;
    let src = source(b, tool, warm, top, path).ok()?;
    let build = src.profile.parent()?;
    // (its own: inside the checkout as the home's is inside the home, or one per checkout beside the home's)
    let near = match build.strip_prefix(&src.home) {
        Ok(rel) => path.join(rel),
        Err(_) if b.setting(tool, top).is_some_and(|s| tool.per_checkout(&s)) => {
            build.parent()?.to_path_buf()
        }
        Err(_) => return None,
    };
    let near = near.ancestors().find(|a| a.is_dir())?;
    if device(near)? != device(&src.profile)? || !clones(near) {
        return None;
    }
    let names = own_names(warm, &[&src.home, top], &[&src.answer]);
    let (size, whole) = shared_size(&src.profile, warm, &names, until);
    Some(whole.then_some(size))
}

/// A clone of a folder at a place not there yet (`clone_tree`; tests: a stand-in).
type Cloner = dyn Fn(&Path, &Path) -> std::io::Result<()>;

/// The new checkout at `new` started warm from the main checkout's builds (the module's doc) — what to say of it once
/// in the log: Ok how it started, Err why it starts cold. None: not a project of a tool that starts warm. On a worker.
pub fn copy(b: &Builds, new: &Path) -> Option<Result<String, String>> {
    copy_with(b, new, &clone_tree)
}

fn copy_with(b: &Builds, new: &Path, clone: &Cloner) -> Option<Result<String, String>> {
    let tool = b.tool_for(new)?.ok()?;
    let warm = tool.warm()?;
    let shown = crate::allow::tilde(new);
    Some(match warmed(b, tool, warm, new, clone) {
        Ok((size, dropped)) => Ok(format!(
            "{}'s first build in {shown} starts warm: the dependencies the main checkout has built ({}, cloned: no \
             extra disk until it changes) — its own packages build fresh ({dropped} entries of theirs left out)",
            tool.name,
            crate::worktree::size_text(size)
        )),
        Err(why) => Err(format!("{}'s first build in {shown} starts cold: {why}", tool.name)),
    })
}

/// `copy`'s work: what the clone takes on disk (`disk_size`), how many of the checkout's own entries went.
fn warmed(
    b: &Builds,
    tool: &Tool,
    warm: &Warm,
    new: &Path,
    clone: &Cloner,
) -> Result<((u64, bool), usize), String> {
    let new = std::fs::canonicalize(new).map_err(|e| format!("{}: {e}", new.display()))?;
    let src = source(b, tool, warm, &new, &new)?;
    let ((_, build), answer) = b.ask_whole(tool, &new).ok_or("its build folder isn't known")?;
    let build = real_beyond(&build);
    let src_build = src.profile.parent().unwrap_or(&src.profile);
    // Its own: inside it, or one per checkout in the tool's home — never the home's, nor in the home
    let base = match build.starts_with(&new) {
        true => new.clone(),
        false if b.setting(tool, &new).is_some_and(|s| tool.per_checkout(&s)) => {
            let home = b.home_of(tool).and_then(|h| std::fs::canonicalize(h).ok());
            home.filter(|h| build.starts_with(h)).ok_or("its build folder isn't in the tool's home")?
        }
        false => return Err("its build folder is shared, not its own".into()),
    };
    if build.starts_with(src_build) || src_build.starts_with(&build) || build.starts_with(&src.home) {
        return Err("its build folder is the main checkout's".into());
    }
    let dst = build.join(&warm.profile);
    if std::fs::symlink_metadata(&dst).is_ok() {
        return Err("a build there started first".into());
    }
    // (another volume: said before its folder is made — no marked empty one left there)
    let near = build.ancestors().find(|a| a.is_dir()).unwrap_or(&build);
    if device(near) != device(&src.profile) {
        return Err("its build folder is on another volume than the main checkout's".into());
    }
    crate::sandbox::build_folder_made(&build, &base, tool.marks())?;
    let profile = warm.profile.to_string_lossy();
    clear_staged(&build, &profile);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let staged = build.join(format!("{profile}{STAGED}{}-{seq}", std::process::id()));
    std::fs::create_dir(&staged).map_err(|e| format!("{}: {e}", staged.display()))?;
    let done = (|| {
        {
            let _held = locked(&src.profile.join(&warm.lock))?;
            for f in warm.folders.iter().filter(|f| src.profile.join(f).is_dir()) {
                clone(&src.profile.join(f), &staged.join(f))
                    .map_err(|e| format!("its file system clones nothing ({e}) — nothing copied"))?;
            }
        }
        let names = own_names(warm, &[&src.home, &new], &[&src.answer, &answer]);
        let dropped = drop_own(&staged, warm, &names)?;
        let size = crate::worktree::disk_size(&staged, crate::clock::now() + crate::worktree::MEASURE_FOR);
        match rename_new(&staged, &dst) {
            Ok(()) => Ok((size, dropped)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                Err("a build there started first".into())
            }
            Err(e) => Err(format!("{}: {e}", dst.display())),
        }
    })();
    if done.is_err() {
        let _ = std::fs::remove_dir_all(&staged);
    }
    done
}

/// The new checkout at `root` started warm on a worker — the opening doesn't wait — and said once in the log.
pub fn begin(ed: &crate::editor::Editor, root: &Path) {
    let Some(b) = ed.workspace_store.builds.clone() else { return };
    let root = root.to_path_buf();
    ed.events.shared_jobs().background(move || {
        if let Some(said) = copy(&b, &root) {
            crate::log::line("session", "info", &said.unwrap_or_else(|why| why));
        }
    });
}

#[cfg(test)]
pub(crate) mod tests;
