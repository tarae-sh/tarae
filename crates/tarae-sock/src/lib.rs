//! A unix socket by a path past `sun_path` (#383: a scene's state folder made its socket's path 106 bytes, and the
//! driver's connect failed for 20 s unsaid). The kernel takes a socket's path in `sockaddr_un.sun_path` — 104 bytes on
//! macOS, 108 on Linux, the closing NUL included — so a longer one is named from its folder: a relative name, its
//! folder the working folder of the thread that binds or connects.
//!
//! A process's working folder is every thread's, so a `chdir` around the call moves it under the others (a client's
//! reconnect, a test binary's other tests). Neither way here moves it:
//! - Linux names the folder by a descriptor it holds: `/proc/self/fd/N/<name>`.
//! - macOS has no such name (no `connectat`; its `/dev/fd` names no folder's entries), so the call runs on a thread of
//!   its own whose working folder alone is the socket's folder — `pthread_chdir_np`, libpthread's per-thread folder
//!   (exported since 10.12, the one fts's `FTS_THREAD_FCHDIR` uses) — joined before `at` returns.
//!
//! No file is made and no folder or link of anyone's is trusted: the name is looked up from its folder as the kernel
//! would look the whole path up, under the same permissions.

#[cfg(target_os = "macos")]
use std::ffi::CString;
use std::io;
#[cfg(target_os = "macos")]
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::Path;

/// `sun_path`'s room, its closing NUL included: a path at least this long is named from its folder.
#[cfg(target_os = "macos")]
pub const SUN_PATH: usize = 104;
#[cfg(not(target_os = "macos"))]
pub const SUN_PATH: usize = 108;

/// Connects to the socket at `path`, however long (a long one from its folder, `at`).
pub fn connect(path: &Path) -> io::Result<UnixStream> {
    at(path, |p: &Path| UnixStream::connect(p))
}

/// `f` on `path` — a path that fits `sun_path` as it is, a longer one as its name from its folder (`from_folder`): the
/// caller's working folder and every other thread's stay. Err where it still can't: a name alone past `sun_path`, or
/// the folder not one to name it from (the OS's reason, with it).
pub fn at<T: Send>(path: &Path, f: impl FnOnce(&Path) -> io::Result<T> + Send) -> io::Result<T> {
    if path.as_os_str().len() < SUN_PATH {
        return f(path);
    }
    let past =
        |why: &str| io::Error::new(io::ErrorKind::InvalidInput, format!("past sun_path ({SUN_PATH}): {why}"));
    let Some(name) = path.file_name() else { return Err(past("it names no socket")) };
    if name.len() >= SUN_PATH {
        return Err(past("its name alone is that long"));
    }
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    from_folder(dir, Path::new(name), f)
}

/// The folder's reason it can't be named from.
fn from_said(dir: &Path, e: io::Error) -> io::Error {
    io::Error::new(e.kind(), format!("can't name a socket from {}: {e}", dir.display()))
}

/// macOS: `f` on `name`, on a thread of its own whose working folder alone is `dir` (`pthread_chdir_np`), joined
/// before it returns.
#[cfg(target_os = "macos")]
fn from_folder<T: Send>(
    dir: &Path,
    name: &Path,
    f: impl FnOnce(&Path) -> io::Result<T> + Send,
) -> io::Result<T> {
    let folder = CString::new(dir.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a NUL in a socket's folder"))?;
    std::thread::scope(|s| {
        let thread = std::thread::Builder::new().name("tarae-sock".into()).spawn_scoped(s, || {
            own_folder(&folder).map_err(|e| from_said(dir, e))?;
            let r = f(name);
            back();
            r
        })?;
        thread.join().unwrap_or_else(|p| std::panic::resume_unwind(p))
    })
}

/// Linux: `f` on `/proc/self/fd/N/name`, `N` a descriptor of `dir` held meanwhile — the kernel walks the folder the
/// descriptor holds. No thread and no `unshare(CLONE_FS)`: Docker's default seccomp profile refuses `unshare` without
/// `CAP_SYS_ADMIN` (measured: EPERM in `cargo xtask linux`), as other filters may.
#[cfg(target_os = "linux")]
fn from_folder<T: Send>(
    dir: &Path,
    name: &Path,
    f: impl FnOnce(&Path) -> io::Result<T> + Send,
) -> io::Result<T> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let folder = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_DIRECTORY)
        .open(dir)
        .map_err(|e| from_said(dir, e))?;
    let via = Path::new("/proc/self/fd").join(folder.as_raw_fd().to_string()).join(name);
    if via.as_os_str().len() >= SUN_PATH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("past sun_path ({SUN_PATH}) even as {}", via.display()),
        ));
    }
    let r = f(&via);
    drop(folder);
    r
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn from_folder<T>(dir: &Path, _: &Path, _: impl FnOnce(&Path) -> io::Result<T>) -> io::Result<T> {
    Err(from_said(dir, io::Error::new(io::ErrorKind::Unsupported, "no way here to name one from its folder")))
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    /// This thread's own working folder (libpthread; `-1` to `pthread_fchdir_np`: the process's again).
    fn pthread_chdir_np(path: *const libc::c_char) -> libc::c_int;
    fn pthread_fchdir_np(fd: libc::c_int) -> libc::c_int;
}

/// This thread's working folder `folder`, no other's.
#[cfg(target_os = "macos")]
fn own_folder(folder: &CString) -> io::Result<()> {
    // SAFETY: a NUL-ended path the call only reads; it sets this thread's folder alone.
    match unsafe { pthread_chdir_np(folder.as_ptr()) } {
        0 => Ok(()),
        _ => Err(io::Error::last_os_error()),
    }
}

/// This thread back on the process's folder (it ends next: the folder is let go either way).
#[cfg(target_os = "macos")]
fn back() {
    // SAFETY: -1 names no descriptor: the thread's own folder dropped.
    unsafe { pthread_fchdir_np(-1) };
}

#[cfg(test)]
mod tests;
