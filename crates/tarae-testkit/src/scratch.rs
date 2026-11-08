//! A test's own scratch folder (f22 p). A path made of a name and the process id is one anyone on the machine
//! can guess, and a test that cleared it first (`remove_dir_all`) would delete what another user, another run or
//! a planted link put there. `Scratch::new` makes a folder that's new — `mkdir`, never `create_dir_all` over
//! what's there: 0700, and the next number when its name is taken (`EEXIST`, a link too) — so it removes only
//! what it made, when it's dropped. A path it gave is never given again in the process, gone or not (#446).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// Every path `in_dir` gave in this process, as written. A dropped folder's number isn't taken again (#446): tarae's
/// process-wide memory goes by place — `gitrepo`'s answers since tarae started, its repositories kept by real path —
/// so a test landing where an earlier one was found what that one answered (`repotrust`'s `n`: the next checkout's
/// git asked nothing); and `left_by`, which judges by path, counted a twin test on another thread holding the number
/// just freed as left behind (`reviewer`'s board).
static GIVEN: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

thread_local! {
    /// The scratch folders this thread made and hasn't dropped — a test's own (`holds`).
    static MADE: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    /// Every scratch folder this thread made, dropped or not — what `left_by` looks for on disk.
    static EVER: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

/// Whether `path` is in a scratch folder this thread made and still has (or is one): what a test hands code that
/// writes, judged where that code starts — a guard that fails a test rooted in home, the state folder or a guessable
/// temp path. As written: absolute, no `..` part. (`into_path`'s stay its own: the caller removes them.)
pub fn holds(path: &Path) -> bool {
    let plain = path.is_absolute()
        && path.components().all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
    plain && MADE.with(|m| m.borrow().iter().any(|s| path.starts_with(s)))
}

/// The scratch folders this thread made and still has — `into_path`'s too, which nothing removes for it: what a
/// check that a run leaves no folder behind compares before and after (#435).
pub fn made() -> Vec<PathBuf> {
    MADE.with(|m| m.borrow().clone())
}

/// The scratch folders this thread made while `f` ran that are still on disk once it returned: `into_path`'s
/// nothing removed, and one written again after its `Scratch` went — a save or a stand-in still at work (#441). By
/// this thread's own list, so other tests' folders made beside it never count.
pub fn left_by(f: impl FnOnce()) -> Vec<PathBuf> {
    let from = EVER.with(|e| e.borrow().len());
    f();
    EVER.with(|e| e.borrow()[from..].iter().filter(|p| p.symlink_metadata().is_ok()).cloned().collect())
}

/// A folder of the test's own under the temp folder, removed (with what's in it) when dropped.
#[derive(Debug)]
pub struct Scratch {
    path: PathBuf,
}

/// Names found taken at most before giving up (each one another's — never counting the ones this process gave).
const TRIES: usize = 1000;

impl Scratch {
    /// `tarae-<name>-<pid>-<n>` made new in the temp folder, by its real path (macOS: `/private/var/…`) — `n` one
    /// this process hasn't given before.
    pub fn new(name: &str) -> Scratch {
        Scratch::in_dir(&std::env::temp_dir(), name)
    }

    /// `new` in `dir` (a test's own temp folder or a stand-in for one).
    pub fn in_dir(dir: &Path, name: &str) -> Scratch {
        let mut given = GIVEN.lock().unwrap_or_else(|e| e.into_inner());
        let mut taken = 0;
        for n in 0.. {
            let path = dir.join(format!("tarae-{name}-{}-{n}", std::process::id()));
            if given.contains(&path) {
                continue;
            }
            if taken == TRIES {
                break;
            }
            taken += 1;
            match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => {
                    given.insert(path.clone());
                    let path = std::fs::canonicalize(&path).unwrap_or(path);
                    MADE.with(|m| m.borrow_mut().push(path.clone()));
                    EVER.with(|e| e.borrow_mut().push(path.clone()));
                    return Scratch { path };
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("a scratch folder in {}: {e}", dir.display()),
            }
        }
        panic!("no free scratch folder name in {} after {TRIES}", dir.display())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Its path, no longer removed when dropped — for a fixture that removes it itself (its own `Drop`), or a
    /// helper whose caller does: made new all the same.
    pub fn into_path(self) -> PathBuf {
        let me = std::mem::ManuallyDrop::new(self);
        me.path.clone()
    }

    /// A path in it.
    pub fn join(&self, rel: impl AsRef<Path>) -> PathBuf {
        self.path.join(rel)
    }
}

/// How long `long_socket`'s paths are: past `sun_path` on both systems (macOS 104, Linux 108, the NUL included) —
/// #383's scene socket was 106.
pub const LONG_SOCKET: usize = 110;

/// A socket's path `LONG_SOCKET` bytes long ending in `name` (`ui/7.sock` too), in `dir` (a test's own) — its folder
/// made (longer only where `dir` already is).
pub fn long_socket(dir: &Path, name: &str) -> PathBuf {
    let room = LONG_SOCKET.saturating_sub(dir.as_os_str().len() + 2 + name.len());
    let folder = if room == 0 { dir.to_path_buf() } else { dir.join("l".repeat(room)) };
    let path = folder.join(name);
    let made = path.parent().unwrap_or(&folder);
    std::fs::create_dir_all(made).unwrap_or_else(|e| panic!("{}: {e}", made.display()));
    assert!(path.as_os_str().len() >= LONG_SOCKET, "{}", path.display());
    path
}

/// A test's script at `path`, executable — written by a child `sh`, never opened for writing by the test binary:
/// Linux won't exec a file any process holds open for writing, and every child another test forks holds a copy of
/// this process's descriptors until its own exec (ETXTBSY: `stopping_claudes_command_stops_its_containers` once,
/// its stand-in `docker` busy, the engine never asked). A child's own descriptors no fork of ours copies.
pub fn write_script(path: &Path, text: &str) {
    use std::io::Write as _;
    let mut sh = std::process::Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("/bin/sh");
    sh.stdin.take().expect("its stdin").write_all(text.as_bytes()).expect("the script to sh");
    let status = sh.wait().expect("sh");
    assert!(status.success(), "{} not written: {status}", path.display());
}

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for Scratch {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

/// Removals tried at most: a stand-in still at work (a server's `cat > log`) may make a file in the folder while it's
/// removed, and the folder's own removal then fails, not empty — left in the temp folder (#441). Each try takes what
/// came meanwhile; a writer that never stops isn't waited for.
const REMOVALS: usize = 10;

impl Drop for Scratch {
    fn drop(&mut self) {
        // (`try_with`: one dropped as the thread ends finds its list gone)
        let _ = MADE.try_with(|m| m.borrow_mut().retain(|p| p != &self.path));
        for _ in 0..REMOVALS {
            match std::fs::remove_dir_all(&self.path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => continue,
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Made new, 0700, by its real path, each its own; a name taken — a folder another made there, a link — is
    /// passed over and left as it was; dropped, only what it made goes.
    #[test]
    fn a_scratch_folder_is_new_its_own_and_goes_when_dropped() {
        let outer = Scratch::new("scratch-outer");
        // (the first names taken: a folder of another's with a file in it, a link to it)
        let pid = std::process::id();
        let (theirs, link) = (outer.join(format!("tarae-t-{pid}-0")), outer.join(format!("tarae-t-{pid}-1")));
        std::fs::create_dir(&theirs).unwrap();
        std::fs::write(theirs.join("keep"), "theirs").unwrap();
        std::os::unix::fs::symlink(&theirs, &link).unwrap();
        let a = Scratch::in_dir(&outer, "t");
        let b = Scratch::in_dir(&outer, "t");
        assert_eq!(a.path(), outer.join(format!("tarae-t-{pid}-2")), "the taken ones passed over");
        assert_eq!(b.path(), outer.join(format!("tarae-t-{pid}-3")), "each its own");
        assert!(a.is_dir() && !a.is_symlink());
        let mode = std::fs::metadata(&a).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        std::fs::write(a.join("x"), "mine").unwrap();
        let (a_path, b_path) = (a.to_path_buf(), b.to_path_buf());
        drop((a, b));
        assert!(!a_path.exists() && !b_path.exists(), "gone when dropped");
        assert_eq!(std::fs::read_to_string(theirs.join("keep")).unwrap(), "theirs", "never another's");
        assert!(link.is_symlink());
    }

    /// `left_by`: what the call made that's still on disk — `into_path`'s, one made again after its `Scratch` went —
    /// never one dropped, nor one made before the call.
    #[test]
    fn left_by_is_what_a_call_made_and_left() {
        let before = Scratch::new("scratch-left-before");
        let (mut kept, mut again) = (PathBuf::new(), PathBuf::new());
        let left = left_by(|| {
            drop(Scratch::new("scratch-left-dropped"));
            kept = Scratch::new("scratch-left-kept").into_path();
            let s = Scratch::new("scratch-left-again");
            again = s.to_path_buf();
            drop(s);
            std::fs::create_dir(&again).unwrap();
        });
        assert_eq!(left, [kept.clone(), again.clone()]);
        for p in [kept, again] {
            std::fs::remove_dir_all(p).ok();
        }
        assert!(before.is_dir());
    }

    /// #446: a number dropped isn't given again in the process — not to this thread, not to another: so another
    /// thread's new folder is never at a path `left_by` made and dropped (the board's twin test, made after its
    /// first loop went).
    #[test]
    fn a_path_given_is_never_given_again_in_the_process() {
        let outer = Scratch::new("scratch-again-outer");
        let pid = std::process::id();
        let mut theirs = None;
        let left = left_by(|| {
            drop(Scratch::in_dir(&outer, "t"));
            let other = std::thread::scope(|s| s.spawn(|| Scratch::in_dir(&outer, "t")).join().unwrap());
            theirs = Some(other);
        });
        assert!(left.is_empty(), "another thread's isn't this call's: {left:?}");
        assert_eq!(theirs.unwrap().path(), outer.join(format!("tarae-t-{pid}-1")), "0 was given");
        assert_eq!(Scratch::in_dir(&outer, "t").path(), outer.join(format!("tarae-t-{pid}-2")));
    }

    /// `holds`: in a scratch folder this thread made and still has — not another thread's, not one dropped, not a
    /// path climbing out of one, never home or the temp folder itself.
    #[test]
    fn holds_is_a_scratch_folder_this_thread_has() {
        let a = Scratch::new("scratch-holds");
        assert!(holds(&a) && holds(&a.join("state/plugins")));
        assert!(!holds(&a.join("../elsewhere")) && !holds(Path::new("state")));
        assert!(!holds(&std::env::temp_dir()) && !holds(Path::new("/")));
        if let Some(home) = std::env::var_os("HOME") {
            assert!(!holds(&Path::new(&home).join(".local/state/tarae")));
        }
        let path = a.to_path_buf();
        assert!(!std::thread::scope(|s| s.spawn(|| holds(&path)).join().unwrap()), "another thread's");
        let kept = Scratch::new("scratch-holds-kept").into_path();
        assert!(holds(&kept), "`into_path`'s stays the test's");
        std::fs::remove_dir_all(&kept).ok();
        drop(a);
        assert!(!holds(&path), "dropped");
    }
}
