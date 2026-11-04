//! `tarae attach --stdio [--wait S] [--pid N | PLACE]` (the remote plan's §8d, step 5): the relay, the remote command
//! `tarae attach ssh://…` runs on the host's machine through ssh. It says a text preamble on its stdout — `MAGIC`, then
//! `ok PID PROJECT` or `no WHY` with the hosts it found (a line each, exit 2) — and then copies bytes both ways between
//! its stdin/stdout and the host's attach socket, parsing none (the client's `Hello`, the host's frames: protocol v0 as
//! it is). To the host it's a local attach (this user, its ancestry sshd's). Its number is the preamble's own, never
//! `wire::V`: any tarae from step 5 on relays to any host.
//!
//! Which host: `--pid`'s; else the one whose project holds PLACE (`~`, `~/x` the home's, `/x`), the deepest; else, no
//! PLACE, the only one running. `--wait S`: a restarted host's socket retried that long while it isn't bound yet.

use std::fs::File;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ssh::MAGIC;

const USAGE: &str = "usage: tarae attach --stdio [--wait S] [--pid PID | PLACE]   relay a host for ssh";

/// The relay: its preamble, then bytes both ways until either side ends. Never returns.
pub fn main(args: &[String]) -> ! {
    let mut out = stdio(1);
    // (first, whatever comes after: every refusal reaches the client in the relay's words)
    let _ = out.write_all(MAGIC).and_then(|_| out.flush());
    let (stream, pid, project) = match found(args) {
        Ok(f) => f,
        Err(No { why, hosts }) => {
            let mut said = format!("no {why}\n");
            for h in hosts {
                said += &format!("{h}\n");
            }
            let _ = out.write_all(said.as_bytes()).and_then(|_| out.flush());
            std::process::exit(2)
        }
    };
    if out.write_all(format!("ok {pid} {project}\n").as_bytes()).and_then(|_| out.flush()).is_err() {
        std::process::exit(1)
    }
    relay(stdio(0), out, stream);
    std::process::exit(0)
}

/// Descriptor `fd` (0 or 1) as a file — no buffer between: a frame goes on as it comes.
fn stdio(fd: i32) -> File {
    // SAFETY: 0 and 1 are the process's, each taken once here; the relay is all the process does.
    unsafe { File::from_raw_fd(fd) }
}

/// Bytes both ways until either side ends: the client's (stdin's end — ssh gone) is the host's EOF (its detach); the
/// host's (its socket closed) ends the relay, and ssh's channel with it.
fn relay(mut from: impl Read + Send + 'static, mut to: impl Write, stream: std::os::unix::net::UnixStream) {
    let Ok(mut up) = stream.try_clone() else { return };
    crate::event::thread("tarae-relay-up", move || {
        let _ = copy(&mut from, &mut up);
        let _ = up.shutdown(Shutdown::Write);
    });
    let _ = copy(&mut &stream, &mut to);
}

/// `from` into `to` until `from` ends, each read written and flushed as it comes.
fn copy(from: &mut impl Read, to: &mut impl Write) -> io::Result<()> {
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        to.write_all(&buf[..n])?;
        to.flush()?;
    }
}

/// No host: why, and the hosts there (a line each).
struct No {
    why: String,
    hosts: Vec<String>,
}

impl No {
    fn just(why: impl Into<String>) -> No {
        No { why: why.into(), hosts: Vec::new() }
    }
}

/// The host asked for, connected: its socket, pid and project.
fn found(args: &[String]) -> Result<(std::os::unix::net::UnixStream, u32, String), No> {
    let (mut wait, mut pid, mut place) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--stdio" => {}
            "--wait" => {
                let s = it.next().and_then(|s| s.parse::<u64>().ok());
                wait = Some(Duration::from_secs(s.ok_or_else(|| No::just(USAGE))?));
            }
            "--pid" => {
                pid = Some(it.next().and_then(|p| p.parse::<u32>().ok()).ok_or_else(|| No::just(USAGE))?)
            }
            p if !p.starts_with('-') && place.is_none() => place = Some(p.to_string()),
            other => return Err(No::just(format!("unknown: {other} — {USAGE}"))),
        }
    }
    let here = machine();
    let state = crate::config::state_dir()
        .ok_or_else(|| No::just("no state folder (neither XDG_STATE_HOME nor HOME)"))?;
    let ui = state.join("ui");
    let hosts = || hosts_in(&ui);
    let listed = |hosts: &[(u32, Vec<PathBuf>)]| hosts.iter().map(|(p, roots)| line(*p, roots)).collect();
    let (pid, project) = match (pid, &place) {
        (Some(p), _) => (p, String::new()),
        (None, Some(place)) => {
            let at = resolve(place).map_err(|e| No::just(format!("no folder {place} on {here}: {e}")))?;
            let hosts = hosts();
            match pick(&hosts, &at) {
                Some((p, root)) => (p, root.display().to_string()),
                None => {
                    let why = no_host(Some(place), &here, &ui, graphic_access());
                    return Err(No { why, hosts: listed(&hosts) });
                }
            }
        }
        (None, None) => {
            let hosts = hosts();
            match hosts.as_slice() {
                [(p, roots)] => (*p, roots.first().map(|r| r.display().to_string()).unwrap_or_default()),
                [] => return Err(No::just(no_host(None, &here, &ui, graphic_access()))),
                _ => {
                    let why = format!("several tarae hosts run on {here} — name one: ssh://…/PATH or ?pid=N");
                    return Err(No { why, hosts: listed(&hosts) });
                }
            }
        }
    };
    let path = crate::attach::path_for(pid).ok_or_else(|| No::just("no state folder"))?;
    let until = crate::clock::now() + wait.unwrap_or_default();
    let stream = loop {
        match crate::attach::connect(&path) {
            Ok(s) => break s,
            Err(e)
                if crate::clock::now() < until
                    && matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) =>
            {
                crate::event::sleep(Duration::from_millis(50));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let why = format!("no tarae host {pid} on {here} (no {})", path.display());
                return Err(No { why, hosts: listed(&hosts()) });
            }
            Err(e) => return Err(No::just(format!("host {pid} on {here}: {}: {e}", path.display()))),
        }
    };
    Ok((stream, pid, project))
}

/// PLACE as a real path: `~` and `~/x` the home's, `/x` as written, else relative to the home (where ssh starts).
fn resolve(place: &str) -> io::Result<PathBuf> {
    let home = || std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| io::Error::other("no HOME"));
    let p = match place {
        "~" => home()?,
        p if p.starts_with("~/") => home()?.join(&p[2..]),
        p if p.starts_with('/') => PathBuf::from(p),
        p => home()?.join(p),
    };
    std::fs::canonicalize(p)
}

/// The hosts in `ui` (a `<pid>.attach` socket, and its state socket answering): pid and projects, by pid.
fn hosts_in(ui: &Path) -> Vec<(u32, Vec<PathBuf>)> {
    let mut pids: Vec<u32> = std::fs::read_dir(ui)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "attach"))
        .filter_map(|p| p.file_stem()?.to_str()?.parse().ok())
        .collect();
    crate::sorting::sort(&mut pids);
    pids.into_iter()
        .filter_map(|pid| Some((pid, crate::uisocket::roots_of(&ui.join(format!("{pid}.sock")))?)))
        .collect()
}

/// The host whose project holds `at` — the deepest such project.
fn pick<'a>(hosts: &'a [(u32, Vec<PathBuf>)], at: &Path) -> Option<(u32, &'a Path)> {
    hosts
        .iter()
        .flat_map(|(p, roots)| roots.iter().map(move |r| (*p, r.as_path())))
        .filter(|(_, r)| at.starts_with(r))
        .max_by_key(|(_, r)| r.as_os_str().len())
}

/// A host's line in a list: its pid, its projects (the front one first).
fn line(pid: u32, roots: &[PathBuf]) -> String {
    let roots: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
    format!("{pid:>7}  {}", roots.join(", "))
}

/// No host for `place` (None: none at all) on `machine`: the way to start one, and — where this session can't reach
/// the login keychain (S-B: an ssh session on macOS) — why not from here.
fn no_host(place: Option<&str>, machine: &str, ui: &Path, gui: bool) -> String {
    let what = place.map_or_else(|| "no tarae host runs".to_string(), |p| format!("no tarae host for {p}"));
    let why = if gui {
        ""
    } else {
        " (one started from this ssh session would have no Claude sign-in: the login keychain isn't reachable here)"
    };
    format!(
        "{what} on {machine} (looked in {}) — start one there: tarae --host in a zellij pane at the machine{why}",
        ui.display()
    )
}

/// This machine's name.
fn machine() -> String {
    let mut name = [0u8; 256];
    // SAFETY: gethostname writes at most the length given into the buffer.
    if unsafe { libc::gethostname(name.as_mut_ptr().cast(), name.len() - 1) } != 0 {
        return "this machine".into();
    }
    let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    String::from_utf8_lossy(&name[..end]).into_owned()
}

/// Whether this process's security session has the GUI's (macOS: a Tailscale SSH session is launchd's `Background`,
/// the login keychain not in its search list — piece 0, measured). Elsewhere always.
#[cfg(target_os = "macos")]
fn graphic_access() -> bool {
    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SessionGetInfo(session: u32, id: *mut u32, attributes: *mut u32) -> i32;
    }
    /// `callerSecuritySession`: this process's own.
    const CALLER: u32 = u32::MAX;
    /// `sessionHasGraphicAccess`.
    const GRAPHIC: u32 = 0x0010;
    let (mut id, mut attrs) = (0u32, 0u32);
    // SAFETY: SessionGetInfo only fills in the two locals it's given.
    let status = unsafe { SessionGetInfo(CALLER, &mut id, &mut attrs) };
    // (unknown: said as if it has — the parenthesis is a reason, never a guess)
    status != 0 || attrs & GRAPHIC != 0
}

#[cfg(not(target_os = "macos"))]
fn graphic_access() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No host is said with the way to start one and where it looked; the keychain's reason only where this session
    /// has no GUI's.
    #[test]
    fn no_host_says_the_way_and_the_keychain_only_without_the_gui() {
        let ui = Path::new("/s/tarae/ui");
        let gui = no_host(Some("~/x"), "mac", ui, true);
        assert_eq!(
            gui,
            "no tarae host for ~/x on mac (looked in /s/tarae/ui) — start one there: tarae --host in a zellij pane at \
             the machine"
        );
        let ssh = no_host(None, "mac", ui, false);
        assert!(ssh.starts_with("no tarae host runs on mac"), "{ssh}");
        assert!(ssh.ends_with("the login keychain isn't reachable here)"), "{ssh}");
    }

    /// The deepest project holding the place is its host's; one holding none, no host.
    #[test]
    fn the_deepest_project_holding_the_place_picks_the_host() {
        let hosts = vec![
            (10, vec![PathBuf::from("/h/a")]),
            (20, vec![PathBuf::from("/h/b"), PathBuf::from("/h/a/c")]),
        ];
        assert_eq!(pick(&hosts, Path::new("/h/a/c/d")), Some((20, Path::new("/h/a/c"))));
        assert_eq!(pick(&hosts, Path::new("/h/a/x")), Some((10, Path::new("/h/a"))));
        assert_eq!(pick(&hosts, Path::new("/h/ab")), None, "by whole parts, never a name's prefix");
        assert_eq!(line(20, &hosts[1].1), "     20  /h/b, /h/a/c");
    }

    /// The relay's two ends: the client's (its stdin ended) is the host's EOF, the host's (its socket closed) ends the
    /// relay — every byte between copied as it came.
    #[test]
    fn the_relay_copies_both_ways_and_ends_with_either_side() {
        use std::os::unix::net::UnixStream;
        // (the host's socket, and the client's two pipes)
        let (relay_side, mut host) = UnixStream::pair().unwrap();
        let (stdin_r, mut stdin_w) = UnixStream::pair().unwrap();
        let (mut stdout_r, stdout_w) = UnixStream::pair().unwrap();
        let t = std::thread::spawn(move || relay(stdin_r, stdout_w, relay_side));
        stdin_w.write_all(b"hello").unwrap();
        let mut b = [0u8; 5];
        host.read_exact(&mut b).unwrap();
        assert_eq!(&b, b"hello");
        host.write_all(b"frame").unwrap();
        stdout_r.read_exact(&mut b).unwrap();
        assert_eq!(&b, b"frame");
        // (the client's end: the host reads EOF — its detach — and the relay still carries what the host says)
        drop(stdin_w);
        assert_eq!(host.read(&mut b).unwrap(), 0, "the client's end is the host's EOF");
        host.write_all(b"bye").unwrap();
        // (the host's end: the relay ends, its stdout closed)
        drop(host);
        t.join().unwrap();
        let mut rest = Vec::new();
        stdout_r.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"bye");
    }
}
