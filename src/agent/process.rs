//! The process an agent runs as, whatever its wire: ended by its pid only while its reader still holds it
//! (`kill`), reaped once it ends (`reap`), and one no reader took ended off the main loop (`end`).

use std::process::Child;

/// The claude processes `pump` reads that aren't reaped yet — the only pids `kill` signals. A reaped one's pid
/// is the system's to give anyone: `pump` reaped it, then waited for stderr before its exit's event cleared the
/// pid where it's kept, and a kill meanwhile (a chat reset, an editor dropped) went to whoever had it by then
/// (frictions28 T1: an e2e test's `tarae state` got a SIGKILL from nowhere in it).
pub(super) static UNREAPED: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// Force-kills the process (cancel) — the reader thread ends on EOF. Only one `pump` hasn't reaped.
pub fn kill(pid: u32) {
    let unreaped = UNREAPED.lock().unwrap_or_else(|e| e.into_inner());
    if unreaped.contains(&pid) {
        // SAFETY: a child of tarae's not reaped yet — `pump` reaps it only under this lock: the pid is still its.
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
    }
}

/// `child` reaped once it has ended: waited for without reaping (`WNOWAIT`), then taken out of `UNREAPED` and
/// reaped under its lock — so a `kill` either reaches it or sends nothing.
pub(super) fn reap(child: &mut Child) {
    let pid = child.id();
    // SAFETY: a zeroed siginfo_t is a valid one to fill; waitid only waits for our own child here.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let flags = libc::WEXITED | libc::WNOWAIT;
    // SAFETY: as above.
    while unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, flags) } != 0
        && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
    {}
    let mut unreaped = UNREAPED.lock().unwrap_or_else(|e| e.into_inner());
    unreaped.retain(|&p| p != pid);
    let _ = child.wait();
}

/// A child no reader took (its first lines not written): ended and reaped off the main loop — no `pump` has it,
/// so `kill` can't reach it, and dropped it would stay a zombie.
pub(super) fn end(mut child: Child) {
    crate::event::thread("tarae-reap", move || {
        let _ = child.kill();
        let _ = child.wait();
    });
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    use super::*;
    use crate::agent::claude::pump;

    /// frictions28 T1: `kill` signals only a claude `pump` still holds unreaped — a stand-in of the test's own
    /// that no `pump` has is left running; one `pump` reads is ended, and once reaped its pid is out of the list
    /// (a kill then sends nothing: the pid may be anyone's).
    #[test]
    fn kill_reaches_only_a_child_pump_has_not_reaped() {
        let stand_in = || {
            Command::new("/bin/sh")
                .args(["-c", "read x"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        };
        let mut theirs = stand_in();
        kill(theirs.id());
        std::thread::sleep(Duration::from_millis(100));
        assert!(matches!(theirs.try_wait(), Ok(None)), "no pump has it: not signalled");
        let _ = theirs.kill();
        let _ = theirs.wait();
        let ed = crate::editor::Editor::new(crate::config::Config::default());
        let mut ours = stand_in();
        let (pid, stdin) = (ours.id(), ours.stdin.take());
        pump(ours, None, false, ed.events.sender(), |_| None);
        kill(pid);
        let end = std::time::Instant::now() + crate::test_results::PATIENCE;
        while UNREAPED.lock().unwrap().contains(&pid) {
            assert!(std::time::Instant::now() < end, "killed and reaped");
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(stdin);
    }
}
