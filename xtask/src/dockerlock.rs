//! One `cargo xtask linux` at a time on a Docker, machine-wide (#352: five at once — two bring preflights' Linux
//! clippy, a session's, a kernels run's build, a session's tests — each a rustc or clippy-driver of 1–4 GB, ran
//! colima's 12 GB VM out of memory; every Docker command hung, two ran OOM-killed past an hour). `queued::admit` keeps
//! one session's runs in line, not the machine's: this lock is the machine's — tarae's sessions, the user's own runs
//! and a shell outside tarae alike.
//!
//! An `flock` on a file of its own per Docker endpoint (`DOCKER_HOST`, else the context's — `endpoint`), so colima's
//! `tarae-test` and a Lima VM's Docker never wait on each other. The kernel lets it go when its holder ends, however
//! it ends (a kill too); the descriptor is close-on-exec, so no `docker run` it starts outlives it holding the lock.
//! The holder writes its pid, since when and its checkout into the file; one waiting reads them and says so —
//! `waits for another Linux check (fixes, 4m)` — as it starts waiting, when the holder changes and every `SAY_EVERY`
//! (the rust pack's `[[waits]]` makes it the run's state in tarae's panels: `waiting for another Linux check
//! (fixes) · 4:12`), then `waited 4m for another Linux check` as its turn comes.
//!
//! **Where** (`PLACE`): the shared temp folder — `/private/tmp` on macOS, `/tmp` on Linux — which every sandboxed
//! run writes on both systems (`sandbox.rs`'s `TMP`, in every policy's writes but a project's it holds), the user's
//! runs and any shell too. Never `$TMPDIR`: a sandboxed run's is its own folder. Its name carries the uid, and a
//! file there that isn't this user's own plain file is never used: said, the check runs without waiting its turn.

use std::fs::File;
use std::io::Write;
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::{FileExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

/// The folder the locks are in — every run may write it (the module's doc).
pub(crate) const PLACE: &str = if cfg!(target_os = "macos") { "/private/tmp" } else { "/tmp" };

/// What a waiting check's line starts with — the rust pack's `[[waits]]` matches it.
pub(crate) const WAITS: &str = "waits for another Linux check";

/// How often a waiting check tries the lock again.
const POLL: Duration = Duration::from_millis(250);

/// A waiting check says so again this often, with the holder's time — its output never goes quiet for long.
const SAY_EVERY: Duration = Duration::from_secs(300);

/// How long a waiting check gives a holder that's only just taken the lock to write who it is, before it says it
/// waits without the name.
const UNSAID: Duration = Duration::from_secs(2);

/// Docker's own when nothing names another.
const DEFAULT: &str = "unix:///var/run/docker.sock";

/// How long `docker context inspect` (it reads files, it asks no daemon) may take.
const INSPECT: Duration = Duration::from_secs(10);

/// The lock held: its descriptor, the flock with it — let go at drop or as the process ends.
pub(crate) struct Held(#[allow(dead_code, reason = "held for its flock")] File);

/// The Docker `docker` talks to: `DOCKER_HOST` (set on it, else this process's), else the context's endpoint
/// (`DOCKER_CONTEXT`, else the current one), else Docker's own socket — a socket's path as a real path, so one Docker
/// reached through a link is one lock (Docker's own too: on Debian and Ubuntu `/var/run` is a link to `/run`, and a
/// `DOCKER_HOST` naming `/run/docker.sock` is the same Docker as nothing named).
pub(crate) fn endpoint(docker: &Command) -> String {
    endpoint_or(docker, DEFAULT)
}

/// `endpoint` with `default` as Docker's own socket (its test's: one a scratch link names).
fn endpoint_or(docker: &Command, default: &str) -> String {
    let set = |name: &str| docker.get_envs().find(|(k, _)| *k == name).map(|(_, v)| v.map(|v| v.to_owned()));
    let var = |name: &str| match set(name) {
        Some(v) => v.and_then(|v| v.into_string().ok()),
        None => std::env::var(name).ok(),
    };
    let host = var("DOCKER_HOST").filter(|h| !h.is_empty()).or_else(|| {
        let mut inspect = Command::new(docker.get_program());
        for (k, v) in docker.get_envs() {
            match v {
                Some(v) => inspect.env(k, v),
                None => inspect.env_remove(k),
            };
        }
        inspect.args(["context", "inspect", "--format", "{{.Endpoints.docker.Host}}"]);
        inspect.stdin(std::process::Stdio::null());
        let out = crate::linux::output_within(&mut inspect, INSPECT).ok().flatten()?;
        let host = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (out.status.success() && !host.is_empty()).then_some(host)
    });
    let host = host.unwrap_or_else(|| default.into());
    match host.strip_prefix("unix://").map(std::fs::canonicalize) {
        Some(Ok(real)) => format!("unix://{}", real.display()),
        _ => host,
    }
}

/// The lock's file in `dir` for `endpoint`: `tarae-docker-<uid>-<16 hex>.lock`, FNV-1a of the endpoint.
pub(crate) fn path_in(dir: &Path, endpoint: &str) -> PathBuf {
    let hash = endpoint
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
    // SAFETY: getuid has no preconditions and can't fail.
    let uid = unsafe { libc::getuid() };
    dir.join(format!("tarae-docker-{uid}-{hash:016x}.lock"))
}

/// `take_in` at `PLACE`, said on stderr with the check's other lines.
pub(crate) fn take(endpoint: &str, checkout: &Path) -> Option<Held> {
    take_in(Path::new(PLACE), endpoint, checkout, &mut std::io::stderr())
}

/// The Linux checks' turn on `endpoint`'s Docker, for `checkout`: waited for while another holds it, said on `say` —
/// None: no lock to take (its file not this user's, or not made), said, and the check goes on without it.
pub(crate) fn take_in(dir: &Path, endpoint: &str, checkout: &Path, say: &mut impl Write) -> Option<Held> {
    let path = path_in(dir, endpoint);
    let file = match open(&path) {
        Ok(f) => f,
        Err(why) => {
            let _ = writeln!(
                say,
                "the Linux checks' lock {} wasn't taken ({why}): this one doesn't wait its turn on {endpoint}",
                path.display()
            );
            return None;
        }
    };
    let start = Instant::now();
    // (the holder last said, and when)
    let mut said: Option<(Option<u32>, Instant)> = None;
    loop {
        match flock(&file) {
            Ok(true) => break,
            Ok(false) => {}
            Err(e) => {
                let _ = writeln!(
                    say,
                    "the Linux checks' lock {} wasn't taken ({e}): this one doesn't wait its turn on {endpoint}",
                    path.display()
                );
                return None;
            }
        }
        let holder = Holder::read(&file);
        let pid = holder.as_ref().map(|h| h.pid);
        let due = match &said {
            None => holder.is_some() || start.elapsed() >= UNSAID,
            Some((was, at)) => pid.is_some() && *was != pid || at.elapsed() >= SAY_EVERY,
        };
        if due {
            let _ = writeln!(say, "{}", waits_line(holder.as_ref(), endpoint));
            said = Some((pid, Instant::now()));
        }
        std::thread::sleep(POLL);
    }
    if said.is_some() {
        let _ = writeln!(
            say,
            "waited {} for another Linux check — its turn now",
            crate::buildprune::ago(start.elapsed())
        );
    }
    // (who holds it, for the next one to say)
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let line = format!("{}\t{now}\t{}\n", std::process::id(), checkout.display());
    let _ = file.set_len(0).and_then(|()| file.write_all_at(line.as_bytes(), 0));
    Some(Held(file))
}

/// `waits for another Linux check (fixes, 4m) — one at a time on <endpoint>`.
fn waits_line(holder: Option<&Holder>, endpoint: &str) -> String {
    let who = match holder {
        Some(h) => {
            let name = h
                .checkout
                .file_name()
                .map_or_else(|| h.checkout.display().to_string(), |n| n.to_string_lossy().into());
            let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            format!("{name}, {}", crate::buildprune::ago(Duration::from_secs(now.saturating_sub(h.since))))
        }
        None => "a checkout not named yet".into(),
    };
    format!("{WAITS} ({who}) — one at a time on {endpoint}")
}

/// What the holder wrote: its pid, since when (seconds of the Unix epoch), its checkout.
struct Holder {
    pid: u32,
    since: u64,
    checkout: PathBuf,
}

impl Holder {
    /// None: nothing written yet (just taken), or not this layout.
    fn read(file: &File) -> Option<Holder> {
        let mut buf = vec![0u8; 4096];
        let n = file.read_at(&mut buf, 0).ok()?;
        let text = std::str::from_utf8(&buf[..n]).ok()?;
        let mut parts = text.strip_suffix('\n')?.splitn(3, '\t');
        Some(Holder {
            pid: parts.next()?.parse().ok()?,
            since: parts.next()?.parse().ok()?,
            checkout: PathBuf::from(parts.next()?),
        })
    }
}

/// The lock's file, opened through no link, made 0600 when it isn't there — refused when it isn't this user's own
/// plain file (one planted in the shared folder could be held for ever, or be anyone's).
fn open(path: &Path) -> std::result::Result<File, String> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    // SAFETY: getuid has no preconditions and can't fail.
    let uid = unsafe { libc::getuid() };
    if !meta.is_file() {
        return Err("not a plain file".into());
    }
    if meta.uid() != uid {
        return Err(format!("another user's file (uid {})", meta.uid()));
    }
    Ok(file)
}

/// The lock taken without waiting — false: another holds it.
fn flock(file: &File) -> std::io::Result<bool> {
    loop {
        // SAFETY: a descriptor the file holds open.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(true);
        }
        let e = std::io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EINTR) => continue,
            Some(libc::EWOULDBLOCK) => return Ok(false),
            _ => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests;
