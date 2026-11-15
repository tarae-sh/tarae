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

/// What `f` returns — a wait with no patience of its own (a pipe's read, a thread's join) — on a thread of its own,
/// waited for with the patience: a wait that runs out names `what` (the thread is left to the process's end).
fn within<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(PATIENCE) {
        Ok(v) => v,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("waited {}s for {what}", PATIENCE.as_secs())
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("{what}: its thread panicked"),
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

/// Step 3, L1: a first line read to `FIRST_MAX` and no further — a peer under the agent sending 1 MB with no newline
/// is closed once the bound is read, nothing of it reaches the wire, and the bridge isn't held up by it: a right
/// connection after it is linked.
#[test]
fn a_first_line_past_its_bound_is_refused_and_holds_nothing_up() {
    let dir = crate::scratch::Scratch::new("acp-bridge-first-bound");
    let (tx, rx) = channel();
    let b = Bridge::listen(dir.path(), std::process::id(), tx).unwrap();
    let mut long = connect(&b);
    // (closed once its bound is read: the rest of the write may fail, EPIPE — what's held is the close)
    let _ = long.write_all(&vec![b'a'; 1 << 20]);
    assert!(closed(&mut long), "a first line past its bound: closed");
    let mut live = connect(&b);
    writeln!(live, "{}", b.token()).unwrap();
    let In::Linked(..) = rx.recv_timeout(PATIENCE).expect("linked") else {
        panic!("linked first: nothing of the long one")
    };
}

/// Step 3, L1: the live connection's lines read to `LINE_MAX` and no further — one past it ends the connection,
/// unlinked, nothing of it handed on; the next right connection is the live one.
#[test]
fn a_live_line_past_its_bound_ends_the_connection() {
    let dir = crate::scratch::Scratch::new("acp-bridge-line-bound");
    let (tx, rx) = channel();
    let b = Bridge::listen(dir.path(), std::process::id(), tx).unwrap();
    let mut live = connect(&b);
    writeln!(live, "{}", b.token()).unwrap();
    let In::Linked(n, _) = rx.recv_timeout(PATIENCE).expect("linked") else { panic!("linked first") };
    writeln!(live, r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#).unwrap();
    assert_eq!(next(&rx), r#"bridged {"id":1,"jsonrpc":"2.0","method":"tools/list"}"#);
    let _ = live.write_all(&vec![b' '; LINE_MAX]);
    assert_eq!(next(&rx), format!("unlinked {n}"), "a line past its bound: ended, nothing handed on");
    let mut again = connect(&b);
    writeln!(again, "{}", b.token()).unwrap();
    assert_eq!(next(&rx), format!("linked {}", n + 1), "the next is the live one");
}

/// The relay: the token first, then the agent's side to the socket and the socket to the agent's side — a line from
/// the socket reaches the agent's pipe while the socket stays open (`pass`: on Linux `std::io::copy` held it back) —
/// and it ends when either side does.
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
    let mut agent_out = BufReader::new(agent_out);
    // (`agent_out` kept: its end dropped would end the relay's down side, not the agent's side closing)
    let (back, _agent_out) = within("the answer at the agent's side", move || {
        let mut back = String::new();
        agent_out.read_line(&mut back).unwrap();
        (back, agent_out)
    });
    assert_eq!(back.trim_end(), r#"{"jsonrpc":"2.0","id":7,"result":{}}"#);
    drop(agent_in);
    assert_eq!(within("the relay's end, the agent's side closed", move || relayed.join().unwrap()), Ok(()));
    assert_eq!(next(&rx), format!("unlinked {n}"));
    assert!(relay(Path::new("/nonexistent/0.sock"), "t", std::io::empty(), std::io::sink()).is_err());
}

/// A socket's reader that sleeps through its peer's end, as macOS can (#390/#392, attach's test's model): a read that
/// began before `ended` and found the end is woken only by its socket's timeout — untimed, it sleeps past the
/// patience — and then says nothing came; a read begun after it sees the end.
struct Asleep {
    s: UnixStream,
    ended: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Read for Asleep {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let (before, began) = (self.ended.load(std::sync::atomic::Ordering::SeqCst), crate::clock::now());
        let got = self.s.read(buf);
        if before || !matches!(got, Ok(0)) {
            return got;
        }
        let woken = self.s.read_timeout().unwrap().unwrap_or(PATIENCE * 2);
        crate::event::sleep(woken.saturating_sub(began.elapsed()));
        Err(std::io::ErrorKind::WouldBlock.into())
    }
}

/// `s` asleep through its end, and the flag that says the end is coming.
fn asleep(s: &UnixStream) -> (Asleep, std::sync::Arc<std::sync::atomic::AtomicBool>) {
    let ended = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    (Asleep { s: s.try_clone().unwrap(), ended: ended.clone() }, ended)
}

/// Past a timeout of `readagain`'s, as attach's test waits.
const QUIET: std::time::Duration =
    crate::readagain::EVERY.saturating_add(std::time::Duration::from_millis(500));

/// Item 6 of the L2 notes: tarae's side of a connection has its reads timed and reads again at each timeout — a quiet
/// live connection stays live past a timeout or two, and a relay's end that a read slept through is seen, the
/// connection unlinked (else it stays the live one, and a restarted MCP server is refused as a second).
#[test]
fn a_connections_end_is_seen_through_a_sleeping_read_and_a_quiet_one_stays() {
    let (shared, rx) = shared();
    let (bridge_side, mut relay_side) = UnixStream::pair().unwrap();
    let (reader, ended) = asleep(&bridge_side);
    let s = shared.clone();
    std::thread::spawn(move || connection(1, &bridge_side, reader, &s));
    writeln!(relay_side, "t0ken").unwrap();
    assert_eq!(next(&rx), "linked 1");
    assert!(rx.recv_timeout(QUIET).is_err(), "a quiet connection stays live past a timeout");
    assert!(shared.live.lock().unwrap().is_some());
    ended.store(true, std::sync::atomic::Ordering::SeqCst);
    drop(relay_side);
    assert_eq!(next(&rx), "unlinked 1", "the relay's end seen through the sleeping read");
    assert!(shared.live.lock().unwrap().is_none(), "no longer the live one");
}

/// A bridge's shared state with no socket, its agent the test's own process and its token `t0ken`, and what it hands
/// the wire's thread.
fn shared() -> (std::sync::Arc<Shared>, Receiver<In>) {
    let (tx, rx) = channel();
    let shared = std::sync::Arc::new(Shared {
        agent: std::process::id(),
        token: "t0ken".into(),
        stop: std::sync::atomic::AtomicBool::new(false),
        live: std::sync::Mutex::new(None),
        open: std::sync::Mutex::new(Vec::new()),
        logged: std::sync::Mutex::new(std::collections::HashSet::new()),
        to: std::sync::Mutex::new(tx),
    });
    (shared, rx)
}

/// A socket's reader whose thread's clock moves past `TOKEN_WITHIN` as its `at`-th read begins.
struct Late {
    s: UnixStream,
    at: usize,
    reads: usize,
}

impl Read for Late {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reads += 1;
        if self.reads == self.at {
            crate::clock::advance(TOKEN_WITHIN * 2);
        }
        self.s.read(buf)
    }
}

/// The bridge landing's leftover: a peer that passed the user and ancestry checks and never sends its token is closed
/// once `TOKEN_WITHIN` has passed (its read bounded in time, as attach's `Hello`), nothing reaching the wire and the
/// live slot left free — and once the token came, the live connection has no such bound: quiet past it, it stays.
#[test]
fn a_token_that_never_comes_holds_no_thread_and_the_live_one_has_no_bound() {
    let (shared, rx) = shared();
    let (bridge_side, mut peer) = UnixStream::pair().unwrap();
    timed(&peer);
    let s = shared.clone();
    let reader = Late { s: bridge_side.try_clone().unwrap(), at: 1, reads: 0 };
    let (done, ended) = channel();
    std::thread::spawn(move || {
        connection(1, &bridge_side, reader, &s);
        let _ = done.send(());
    });
    ended.recv_timeout(PATIENCE).expect("a silent peer's thread ended once its time passed");
    assert!(closed(&mut peer), "closed");
    assert!(rx.try_recv().is_err(), "nothing reached the wire");
    assert!(shared.live.lock().unwrap().is_none(), "the live slot free");
    let (bridge_side, mut peer) = UnixStream::pair().unwrap();
    writeln!(peer, "t0ken").unwrap();
    let s = shared.clone();
    let reader = Late { s: bridge_side.try_clone().unwrap(), at: 2, reads: 0 };
    std::thread::spawn(move || connection(2, &bridge_side, reader, &s));
    assert_eq!(next(&rx), "linked 2");
    assert!(rx.recv_timeout(QUIET).is_err(), "quiet past the token's time: still live");
    drop(peer);
    assert_eq!(next(&rx), "unlinked 2");
}

/// A peer under the agent that connects and says nothing never takes the live slot: while it waits for its token (its
/// thread's clock held, so its time can't pass while the test looks), a right connection is linked and live; the
/// silent one's end unlinks nothing and leaves the live one live.
#[test]
fn the_live_slot_stays_free_while_a_peer_says_nothing() {
    let (shared, rx) = shared();
    let (silent_side, silent) = UnixStream::pair().unwrap();
    let s = shared.clone();
    let (done, ended) = channel();
    let (begun, waits) = channel();
    let reader = Told { s: silent_side.try_clone().unwrap(), told: Some(begun) };
    std::thread::spawn(move || {
        crate::clock::hold();
        connection(1, &silent_side, reader, &s);
        let _ = done.send(());
    });
    waits.recv_timeout(PATIENCE).expect("the silent one's read begun");
    assert!(shared.live.lock().unwrap().is_none(), "the slot free while it waits");
    let (live_side, mut live) = UnixStream::pair().unwrap();
    writeln!(live, "t0ken").unwrap();
    let s = shared.clone();
    std::thread::spawn(move || connection(2, &live_side, &live_side, &s));
    assert_eq!(next(&rx), "linked 2", "linked while the silent one waits");
    assert!(ended.try_recv().is_err(), "the silent one still waiting");
    drop(silent);
    ended.recv_timeout(PATIENCE).expect("the silent one's thread ended with its peer");
    assert!(rx.try_recv().is_err(), "its end unlinked nothing");
    assert!(shared.live.lock().unwrap().is_some(), "the live one still live");
    drop(live);
    assert_eq!(next(&rx), "unlinked 2");
}

/// A socket's reader that says when its first read begins.
struct Told {
    s: UnixStream,
    told: Option<std::sync::mpsc::Sender<()>>,
}

impl Read for Told {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if let Some(t) = self.told.take() {
            let _ = t.send(());
        }
        self.s.read(buf)
    }
}

/// A socket's receive timeout, as `readagain::timed` sets it.
fn read_timeout_of(s: &UnixStream) -> std::time::Duration {
    use std::os::fd::AsRawFd;
    let mut tv = libc::timeval { tv_sec: 0, tv_usec: 0 };
    let mut len = std::mem::size_of::<libc::timeval>() as libc::socklen_t;
    // SAFETY: one timeval of ours and its size; getsockopt writes at most that.
    let r = unsafe {
        libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_RCVTIMEO, (&raw mut tv).cast(), &mut len)
    };
    assert_eq!(r, 0, "{}", std::io::Error::last_os_error());
    std::time::Duration::from_secs(tv.tv_sec as u64) + std::time::Duration::from_micros(tv.tv_usec as u64)
}

/// The bridge landing's leftover: `cli`'s stdin timed (`relay_from`, which `cli` runs on stdin), so an agent whose stdio
/// is a socketpair has the relay's end read again for. The input's socket has no timeout before, and
/// `readagain::EVERY` once the relay runs; the relay links and ends with its input.
#[test]
fn the_relays_input_is_timed_as_cli_starts_it() {
    let dir = crate::scratch::Scratch::new("acp-bridge-cli-input");
    let (tx, rx) = channel();
    let b = Bridge::listen(dir.path(), std::process::id(), tx).unwrap();
    let (input, agent_in) = UnixStream::pair().unwrap();
    let kept = input.try_clone().unwrap();
    assert_eq!(read_timeout_of(&kept), std::time::Duration::ZERO, "untimed before");
    let (path, token) = (b.path().to_path_buf(), b.token().to_string());
    let relayed = std::thread::spawn(move || relay_from(&path, &token, input, std::io::sink()));
    let In::Linked(n, _) = rx.recv_timeout(PATIENCE).expect("linked") else { panic!("linked first") };
    assert_eq!(read_timeout_of(&kept), crate::readagain::EVERY, "its input timed");
    drop((agent_in, kept));
    assert_eq!(within("the relay's end, its input closed", move || relayed.join().unwrap()), Ok(()));
    assert_eq!(next(&rx), format!("unlinked {n}"));
}

/// Item 6: the bridge's relay reads its socket (`down`) and its input timed, read again at each timeout — quiet, it
/// keeps relaying past a timeout or two; tarae's end, slept through by its socket's read, ends it.
#[test]
fn the_relay_sees_taraes_end_through_a_sleeping_read_and_stays_while_quiet() {
    let (relay_side, tarae) = UnixStream::pair().unwrap();
    let (input, mut agent_in) = UnixStream::pair().unwrap();
    // (the relay's input timed as `cli` times its stdin)
    crate::readagain::timed(&input).unwrap();
    let (agent_out, output) = std::io::pipe().unwrap();
    let (down, ended) = asleep(&relay_side);
    let (input, _input_ended) = asleep(&input);
    let (done, end) = channel();
    std::thread::spawn(move || {
        let r = carry(relay_side, down, "t0ken", input, output);
        let _ = done.send(r);
    });
    tarae.set_read_timeout(Some(PATIENCE)).unwrap();
    let mut first = String::new();
    BufReader::new(&tarae).read_line(&mut first).unwrap();
    assert_eq!(first, "t0ken\n");
    assert!(end.recv_timeout(QUIET).is_err(), "a quiet relay keeps relaying past a timeout");
    writeln!(agent_in, "up").unwrap();
    let mut got = String::new();
    BufReader::new(&tarae).read_line(&mut got).unwrap();
    assert_eq!(got, "up\n", "still carrying after its timeouts");
    ended.store(true, std::sync::atomic::Ordering::SeqCst);
    drop(tarae);
    let ended = end.recv_timeout(PATIENCE).expect("tarae's end, slept through, ends the relay");
    assert_eq!(ended, Ok(()));
    drop(agent_out);
}
