//! The bridge's socket and its checks, with no agent: the test's own process is the peer, under an "agent" that is
//! itself (its ancestry holds it) or isn't (a pid no process has).

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc::{Receiver, channel};

use super::*;
use crate::test_results::PATIENCE;

/// A pid no process has: no peer runs under it.
const NOBODY: u32 = 999_999_999;

/// A connection to `b`, its reads given the patience (`timed`).
fn connect(b: &Bridge) -> UnixStream {
    let s = crate::uisocket::at(b.path(), |p: &Path| UnixStream::connect(p)).expect("connected");
    timed(&s);
    s
}

/// `s`'s reads given the patience — where it can be set: on a connection the bridge refused and closed first, macOS
/// refuses the option (EINVAL), and a read there ends at once anyway.
fn timed(s: &UnixStream) {
    let _ = s.set_read_timeout(Some(PATIENCE));
}

/// Whether the bridge closed `s` without a word (its read ends: nothing, or the reset of a close that left what was
/// written to it unread) — never a read that ran out of patience.
fn closed(s: &mut UnixStream) -> bool {
    let mut buf = [0u8; 1];
    match s.read(&mut buf) {
        Ok(0) => true,
        Err(e) => e.kind() == std::io::ErrorKind::ConnectionReset,
        Ok(_) => false,
    }
}

/// The next message the bridge handed the wire's thread, but `Linked`'s writer as its number.
fn next(rx: &Receiver<In>) -> String {
    match rx.recv_timeout(PATIENCE).expect("a message to the wire") {
        In::Linked(n, _) => format!("linked {n}"),
        In::Unlinked(n) => format!("unlinked {n}"),
        In::Bridged(v) => format!("bridged {v}"),
        _ => "another".into(),
    }
}

/// The socket is `agent/<tarae pid>/<n>.sock` in the state folder — the folder 0700, the socket 0600, bound past
/// `sun_path` from its folder (a scratch path is longer) — and goes with the bridge, its folder with it; each start
/// its own name and token, which `session/new`'s server entry carries in its environment, never in its arguments.
#[test]
fn the_socket_is_closed_its_own_and_goes_with_the_bridge() {
    let dir = crate::scratch::Scratch::new("acp-bridge-socket");
    // (a state folder whose sockets' paths are past `sun_path` on every system: 104 bytes on macOS, 108 on Linux)
    let state = dir.join("s".repeat(110));
    let (tx, _rx) = channel();
    let (a, b) =
        (Bridge::listen(&state, NOBODY, tx.clone()).unwrap(), Bridge::listen(&state, NOBODY, tx).unwrap());
    let folder = state.join("agent").join(std::process::id().to_string());
    assert_eq!(a.path().parent(), Some(folder.as_path()));
    assert!(a.path().as_os_str().len() > 110, "past sun_path: {}", a.path().display());
    let reached = crate::uisocket::at(a.path(), |p: &Path| UnixStream::connect(p));
    assert!(reached.is_ok(), "bound and reached from its folder: {reached:?}");
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!((mode(&folder), mode(a.path())), (0o700, 0o600));
    assert_ne!(a.path(), b.path());
    assert_ne!(a.token(), b.token());
    assert_eq!(a.token().len(), 64);
    let server = a.server();
    assert_eq!(server["env"], json!([{"name": TOKEN, "value": a.token()}]));
    assert!(
        !server["args"].to_string().contains(a.token()) && !server["command"].to_string().contains(a.token())
    );
    assert_eq!(server["args"].as_array().unwrap().last().unwrap(), &json!(a.path().to_string_lossy()));
    drop(a);
    assert!(folder.exists(), "another's socket still there: the folder stays");
    let b_path = b.path().to_path_buf();
    drop(b);
    assert!(!b_path.exists() && !folder.exists(), "the socket and its folder gone with the bridge");
}

/// A dead tarae's folder (its pid no process's) loses its sockets, and itself once empty; a live one's is left.
#[test]
fn a_dead_taraes_folder_is_cleared_at_the_next_bind() {
    let dir = crate::scratch::Scratch::new("acp-bridge-dead");
    let (tx, _rx) = channel();
    let dead = dir.join("agent").join(NOBODY.to_string());
    let held = crate::uisocket::bind_named(&dead, "0.sock").unwrap();
    let live = dir.join("agent").join(std::os::unix::process::parent_id().to_string());
    std::fs::create_dir_all(&live).unwrap();
    let b = Bridge::listen(dir.path(), NOBODY, tx).unwrap();
    assert!(!dead.exists(), "a dead tarae's folder cleared");
    assert!(live.exists(), "a live process's left");
    drop((b, held));
}

/// The peer's checks, each its own refusal: another user's; one whose ancestry, read to the first process, doesn't
/// hold the agent; one whose ancestry stops short of it (who started it can't be told — refused too); one whose
/// process can't be read at all.
#[test]
fn a_peer_is_judged_by_its_user_and_its_ancestry() {
    let me = 501;
    assert_eq!(judged(501, me, &[900, 800, 1], 800), Ok(()));
    assert_eq!(judged(501, me, &[800, 1], 800), Ok(()), "the agent itself");
    assert_eq!(judged(502, me, &[900, 800, 1], 800), Err(Refused::AnotherUser));
    assert_eq!(judged(501, me, &[900, 700, 1], 800), Err(Refused::NotUnder));
    assert_eq!(judged(501, me, &[900, 700], 800), Err(Refused::Unreadable), "ancestry unread: refused");
    assert_eq!(judged(501, me, &[], 800), Err(Refused::Unknown));
    assert_eq!(judged(501, me, &[0], 800), Err(Refused::Unknown));
    assert!(same(b"abc", b"abc") && !same(b"abc", b"abd") && !same(b"abc", b"ab"));
}

/// A connection under the agent (here: the test's own process, the agent itself) with the token is the live one:
/// linked, its lines handed on as messages, and what's written to it read back; without the token it's closed and
/// nothing reaches the wire; a second while one is live is closed too; the live one ended, unlinked. One not under
/// the agent is closed whatever it sends.
#[test]
fn a_connection_needs_the_agents_ancestry_and_the_token_and_is_the_one_live() {
    let dir = crate::scratch::Scratch::new("acp-bridge-peer");
    let (tx, rx) = channel();
    let b = Bridge::listen(dir.path(), std::process::id(), tx).unwrap();
    // (a connection the bridge refuses may be closed before any of the test's calls on it: they may fail, EPIPE —
    // what's held is that its read ends with nothing and nothing of it reaches the wire)
    let mut wrong = connect(&b);
    let _ = writeln!(wrong, "not-the-token");
    let _ = writeln!(wrong, r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#);
    assert!(closed(&mut wrong), "no token: closed");
    let mut live = connect(&b);
    writeln!(live, "{}", b.token()).unwrap();
    let linked = rx.recv_timeout(PATIENCE).expect("linked");
    let In::Linked(n, out) = linked else { panic!("linked first: nothing of the wrong one") };
    writeln!(live, r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#).unwrap();
    assert_eq!(next(&rx), r#"bridged {"id":1,"jsonrpc":"2.0","method":"tools/list"}"#);
    out.send(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#.into()).unwrap();
    let mut back = String::new();
    BufReader::new(&live).read_line(&mut back).unwrap();
    assert_eq!(back.trim_end(), r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
    let mut second = connect(&b);
    let _ = writeln!(second, "{}", b.token());
    assert!(closed(&mut second), "a second while one is live: closed");
    live.shutdown(std::net::Shutdown::Both).unwrap();
    assert_eq!(next(&rx), format!("unlinked {n}"));
    assert!(rx.try_recv().is_err(), "nothing of the refused ones");
    drop(b);
    let (tx, rx) = channel();
    let b = Bridge::listen(dir.path(), NOBODY, tx).unwrap();
    let mut outside = connect(&b);
    let _ = writeln!(outside, "{}", b.token());
    assert!(closed(&mut outside), "not under the agent, token or not: closed");
    assert!(rx.try_recv().is_err());
}

/// The order a loaded machine may give: the bridge's close of a connection it refused lands before the test's calls
/// on it (forced here: its read waited out first). Setting its patience and writing to it hold that way — what's
/// checked is only that its read ends with nothing and nothing reaches the wire.
#[test]
fn a_refused_connection_holds_whichever_way_its_close_lands() {
    let dir = crate::scratch::Scratch::new("acp-bridge-closed-first");
    let (tx, rx) = channel();
    let b = Bridge::listen(dir.path(), NOBODY, tx).unwrap();
    let mut s = crate::uisocket::at(b.path(), |p: &Path| UnixStream::connect(p)).expect("connected");
    // (not under the agent: refused at `accept`, so this read ends once the bridge closed it — no wait on time)
    assert!(closed(&mut s), "refused");
    timed(&s);
    let _ = writeln!(s, "{}", b.token());
    assert!(closed(&mut s), "its read still ends with nothing");
    assert!(rx.try_recv().is_err(), "nothing reached the wire");
}

/// The relay: the token first, then the agent's side to the socket and the socket to the agent's side; it ends when
/// either side does.
#[test]
fn the_relay_carries_both_ways_and_ends_with_either_side() {
    let dir = crate::scratch::Scratch::new("acp-bridge-relay");
    let (tx, rx) = channel();
    let b = Bridge::listen(dir.path(), std::process::id(), tx).unwrap();
    let (input, mut agent_in) = std::io::pipe().unwrap();
    let (agent_out, output) = std::io::pipe().unwrap();
    let (path, token) = (b.path().to_path_buf(), b.token().to_string());
    let relayed = std::thread::spawn(move || relay(&path, &token, input, output));
    let In::Linked(n, out) = rx.recv_timeout(PATIENCE).expect("linked") else { panic!("linked first") };
    writeln!(agent_in, r#"{{"jsonrpc":"2.0","id":7,"method":"ping"}}"#).unwrap();
    assert_eq!(next(&rx), r#"bridged {"id":7,"jsonrpc":"2.0","method":"ping"}"#);
    out.send(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#.into()).unwrap();
    let mut back = String::new();
    let mut agent_out = BufReader::new(agent_out);
    agent_out.read_line(&mut back).unwrap();
    assert_eq!(back.trim_end(), r#"{"jsonrpc":"2.0","id":7,"result":{}}"#);
    drop(agent_in);
    assert_eq!(relayed.join().unwrap(), Ok(()));
    assert_eq!(next(&rx), format!("unlinked {n}"));
    assert!(relay(Path::new("/nonexistent/0.sock"), "t", std::io::empty(), std::io::sink()).is_err());
}
