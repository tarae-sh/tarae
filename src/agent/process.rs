//! The process an agent runs as, whatever its wire — every one leads a session and so a process group of its own
//! (`spawn::setsid_fed`, `setsid_fed_as`): its group ended by its pid only while its reader still holds it (`kill`),
//! ended and reaped once it ends (`reap_group`), and one no reader took ended off the main loop (`end`).

/// The agents' processes their readers (`claude::pump`, `acp::run`) hold that aren't reaped yet — the only pids
/// `kill` signals. A reaped one's pid is the system's to give anyone: `pump` reaped it, then waited for stderr before
/// its exit's event cleared the pid where it's kept, and a kill meanwhile (a chat reset, an editor dropped) went to
/// whoever had it by then (frictions28 T1: an e2e test's `tarae state` got a SIGKILL from nowhere in it).
pub(super) static UNREAPED: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// Force-kills the process and its group (cancel) — what it started with it (claude's git, `security`, a shell; an
/// ACP agent's bridge relay) — and the reader thread ends on EOF. Only one its reader hasn't reaped: the group's id is
/// still its leader's.
pub fn kill(pid: u32) {
    let unreaped = UNREAPED.lock().unwrap_or_else(|e| e.into_inner());
    if unreaped.contains(&pid) {
        end_group(pid);
    }
}

/// An agent's process reaped once it has ended, by its pid: what it started and left in its group (claude's git or
/// a shell, an ACP agent's bridge relay or a server of its own) is ended too — the group signalled while its leader,
/// ended and not yet reaped, still holds the group's id — then it's taken out of `UNREAPED` and reaped under its lock,
/// so a `kill` either reaches it or sends nothing.
pub(super) fn reap_group(pid: u32) {
    ended(pid);
    end_group(pid);
    let mut unreaped = UNREAPED.lock().unwrap_or_else(|e| e.into_inner());
    unreaped.retain(|&p| p != pid);
    let _ = crate::spawn::wait(pid);
}

/// Ends the process group our child `pid` leads, itself first among it — while it's unreaped (its reader holds it):
/// the id is still its group's.
pub(super) fn end_group(pid: u32) {
    // SAFETY: a group our own child leads, not reaped yet (its reader reaps it, after this): its id is still the group's.
    unsafe { libc::killpg(pid as libc::pid_t, libc::SIGKILL) };
}

/// Waits for `pid`, a child of ours, to end, without reaping it (`WNOWAIT`).
fn ended(pid: u32) {
    // SAFETY: a zeroed siginfo_t is a valid one to fill; waitid only waits for our own child here.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let flags = libc::WEXITED | libc::WNOWAIT;
    // SAFETY: as above.
    while unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, flags) } != 0
        && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
    {}
}

/// A child no reader took (its first lines not written), by its pid: its group ended and it reaped off the main
/// loop — no reader has it, so `kill` can't reach it, and left it would stay a zombie.
pub(super) fn end(pid: u32) {
    crate::event::thread("tarae-reap", move || {
        end_group(pid);
        let _ = crate::spawn::wait(pid);
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
        let mut sh = Command::new("/bin/sh");
        sh.args(["-c", "read x"]);
        let (ours, stdin) = crate::spawn::setsid_fed(&sh).unwrap();
        let pid = ours.pid;
        pump(ours, None, false, false, ed.events.sender(), |_| None);
        kill(pid);
        let end = std::time::Instant::now() + crate::test_results::PATIENCE;
        while UNREAPED.lock().unwrap().contains(&pid) {
            assert!(std::time::Instant::now() < end, "killed and reaped");
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(stdin);
    }
}
