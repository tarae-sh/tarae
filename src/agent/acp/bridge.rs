//! The bridge (step 2's plan §4, landing 4): tarae's tools reach an ACP agent as an MCP server of its own, a stdio
//! program it starts from `session/new`'s `mcpServers` — `tarae agent-tools <socket>`, a relay (`relay`) from its
//! stdin and stdout to a Unix socket the agent's wire listens on (`Bridge`). The wire tells each `tools/call` that
//! comes through it, so the chat's tools and cards are Claude's own (`tools::mcp`).
//!
//! The socket is `$state/agent/<tarae pid>/<n>.sock`, bound by `uisocket::bind_named`'s guards — its folder 0700 and
//! this user's, the socket 0600 and checked, a path past `sun_path` bound from its folder — outside the project, so a
//! sandboxed run can't connect to it (Seatbelt lets a run connect only to sockets in the project and its own run
//! folder; `sandbox::tests::claudes_runs_cant_reach_an_agents_bridge`). Each connection passes three checks, each
//! stopping its own:
//! 1. **The peer is this user** (`getpeereid`, `SO_PEERCRED`): another user's processes — the folder and the
//!    socket's modes already keep them out; this is the second lock.
//! 2. **The peer runs under this agent**: its ancestry includes the agent's pid. This is what stops this user's
//!    other processes (another session's agent, a terminal, a browser): the token below passes through the agent
//!    and sits in the relay's environment, which any process of this user can read, so it can't. A peer whose
//!    ancestry can't be read is refused.
//! 3. **The token**: 32 random bytes made for this start, handed to the relay only in `session/new`'s
//!    `mcpServers[].env` (`TOKEN` — never on a command line), the connection's first line, compared in constant
//!    time. It's the second lock where ancestry can't be told apart: a process of this user's that the agent
//!    started for something else (its hooks, its other MCP servers) and that never read the relay's environment.
//!
//! One connection is live at a time; a second while one is live is refused. The listener stays while the agent runs
//! (an MCP server it restarts comes back), and goes with it: the socket and its folder removed (`Bridge::drop`). A
//! dead tarae's folder is cleared at the next bind. Every refusal is logged, once a bridge and kind.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::In;

/// The relay's variable holding its token (`session/new`'s `mcpServers[].env`).
pub const TOKEN: &str = "TARAE_AGENT_TOOLS_TOKEN";

/// The name the agent knows tarae's server by (agy names its tools `mcp_tarae_<tool>` by it: `acp::OWN`).
pub const NAME: &str = "tarae";

/// The sockets bound by this tarae so far: each start's its own name.
static BOUND: AtomicU64 = AtomicU64::new(0);

/// The most a connection's first line, the token, is read to, its newline in it: 64 hex digits, the newline and
/// slack. Past it the connection is refused — a peer that never sends a newline holds nothing of the bridge's.
pub const FIRST_MAX: usize = 128;

/// The most a message of the live connection's is read to, its newline in it. Past it the connection ends: the peer
/// passed the checks, so this is the agent's relay, but one line never takes all the memory there is.
pub const LINE_MAX: usize = 16 << 20;

/// Why a connection was refused (logged once a bridge and kind).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refused {
    /// Its user and process couldn't be read.
    Unknown,
    AnotherUser,
    /// Its ancestry, read to its first process, doesn't hold the agent.
    NotUnder,
    /// Its ancestry stops before the first process without the agent: who started it can't be told.
    Unreadable,
    /// Its first line isn't the token.
    Token,
    /// Its first line is past `FIRST_MAX`.
    FirstLong,
    /// Another connection is live.
    Second,
    /// The live connection sent a line past `LINE_MAX`: ended.
    LineLong,
}

impl Refused {
    fn said(self) -> &'static str {
        match self {
            Refused::Unknown => "a connection refused: who it is can't be read",
            Refused::AnotherUser => "a connection refused: another user's",
            Refused::NotUnder => "a connection refused: not a process of the agent's",
            Refused::Unreadable => "a connection refused: whose process it is can't be read",
            Refused::Token => "a connection refused: not the token",
            Refused::FirstLong => "a connection refused: a first line past 128 bytes",
            Refused::Second => "a connection refused: another is live",
            Refused::LineLong => "a connection ended: a line past 16 MB",
        }
    }
}

/// Whether a peer of user `uid`, its ancestry `line` (itself first, as `uisocket::ancestors` reads it), may reach
/// agent `agent`'s bridge, `me` this user.
pub fn judged(uid: u32, me: u32, line: &[u32], agent: u32) -> Result<(), Refused> {
    if uid != me {
        return Err(Refused::AnotherUser);
    }
    match line {
        [] | [0, ..] => Err(Refused::Unknown),
        _ if line.contains(&agent) => Ok(()),
        _ if line.last() == Some(&1) => Err(Refused::NotUnder),
        _ => Err(Refused::Unreadable),
    }
}

/// Whether `a` and `b` are the same bytes, in a time that doesn't depend on where they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// A start's token: 32 bytes from `getentropy`, in hex.
fn fresh_token() -> std::io::Result<String> {
    let mut raw = [0u8; 32];
    // SAFETY: a buffer of ours, its length (≤ 256, getentropy's most).
    if unsafe { libc::getentropy(raw.as_mut_ptr().cast(), raw.len()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(raw.iter().map(|b| format!("{b:02x}")).collect())
}

/// What the listener's threads share with the bridge.
struct Shared {
    agent: u32,
    token: String,
    /// The bridge is going: what's accepted from now on is its own wake.
    stop: AtomicBool,
    /// The live connection, if one is.
    live: Mutex<Option<UnixStream>>,
    /// Every connection accepted and not yet ended (shut down with the bridge).
    open: Mutex<Vec<(u64, UnixStream)>>,
    logged: Mutex<HashSet<Refused>>,
    /// To the wire's thread.
    to: Mutex<Sender<In>>,
}

impl Shared {
    fn refused(&self, why: Refused) {
        if self.logged.lock().unwrap_or_else(|e| e.into_inner()).insert(why) {
            crate::log::line("agent-tools", "warn", why.said());
        }
    }

    fn send(&self, m: In) -> bool {
        self.to.lock().unwrap_or_else(|e| e.into_inner()).send(m).is_ok()
    }
}

/// An agent's bridge: its socket listened on while the agent runs; dropped, the socket and its folder go.
pub struct Bridge {
    path: PathBuf,
    shared: Arc<Shared>,
}

impl Bridge {
    /// The bridge of agent `agent` (its pid), bound in `state` (`$state`: `config::state_dir`) as
    /// `agent/<tarae pid>/<n>.sock`, each connection's messages handed to the wire's thread (`to`). On the wire's
    /// thread (its handshake), never the main loop's.
    pub(super) fn listen(state: &Path, agent: u32, to: Sender<In>) -> Result<Bridge, String> {
        let token = fresh_token().map_err(|e| format!("no token: {e}"))?;
        let agents = state.join("agent");
        clear_dead(&agents);
        let dir = agents.join(std::process::id().to_string());
        let name = format!("{}.sock", BOUND.fetch_add(1, Ordering::Relaxed));
        let listener = crate::uisocket::bind_named(&dir, &name)?;
        let shared = Arc::new(Shared {
            agent,
            token,
            stop: AtomicBool::new(false),
            live: Mutex::new(None),
            open: Mutex::new(Vec::new()),
            logged: Mutex::new(HashSet::new()),
            to: Mutex::new(to),
        });
        let s = shared.clone();
        crate::event::thread("tarae-bridge", move || accept(listener, s));
        Ok(Bridge { path: dir.join(name), shared })
    }

    /// Its socket.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `session/new`'s (and `resume`'s, `load`'s) `mcpServers` entry: the relay on this socket, the token only in
    /// its environment. A test build's relay is the test binary re-run on `relay`, speaking on descriptor 3 as the
    /// stand-in does (libtest's own lines go to /dev/null), the socket its last argument.
    pub fn server(&self) -> Value {
        let socket = self.path.to_string_lossy().into_owned();
        let exe = std::env::current_exe().unwrap_or_default().to_string_lossy().into_owned();
        let (command, args) = match cfg!(test) {
            true => {
                let mut args: Vec<String> =
                    super::standin::ARGS.iter().map(|a| a.replace(super::standin::SELF, &exe)).collect();
                if let Some(test) = args.iter_mut().find(|a| a.as_str() == super::standin::TEST) {
                    *test = RELAY_TEST.into();
                }
                args.push(socket);
                ("sh".to_string(), args)
            }
            false => (exe, vec!["agent-tools".to_string(), socket]),
        };
        json!({"name": NAME, "command": command, "args": args,
            "env": [{"name": TOKEN, "value": self.shared.token}]})
    }

    /// Tests: its token.
    #[cfg(test)]
    pub fn token(&self) -> &str {
        &self.shared.token
    }
}

impl Drop for Bridge {
    /// The agent ended: the listener woken and let go, every connection shut, the socket and its folder removed.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // (its own wake: `accept` sees `stop` and returns)
        let _ = crate::uisocket::at(&self.path, |p: &Path| UnixStream::connect(p));
        for (_, s) in self.shared.open.lock().unwrap_or_else(|e| e.into_inner()).drain(..) {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
        let _ = std::fs::remove_file(&self.path);
        // (another agent of this tarae's still there: its folder stays)
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

/// Tests elsewhere (the sandbox's): a bridge of the test process's own, bound in `state`, and what tells the next
/// message it handed the wire's thread within a wait — `linked N`, `bridged <message>`, `unlinked N`, in the one
/// channel's order (None: none came). macOS's only, as its one caller is.
#[cfg(target_os = "macos")]
pub fn bound(state: &Path) -> (Bridge, impl Fn(std::time::Duration) -> Option<String> + use<>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let b = Bridge::listen(state, std::process::id(), tx).expect("a bridge bound");
    let heard = move |wait| {
        Some(match rx.recv_timeout(wait).ok()? {
            In::Linked(n, _) => format!("linked {n}"),
            In::Unlinked(n) => format!("unlinked {n}"),
            In::Bridged(v) => format!("bridged {v}"),
            _ => "another".into(),
        })
    };
    (b, heard)
}

/// A dead tarae's folders under `agents` (`<pid>/`, its pid no process's now): their sockets removed, then each
/// folder if that left it empty — nothing else is touched.
fn clear_dead(agents: &Path) {
    use std::os::unix::fs::FileTypeExt;
    let Ok(entries) = std::fs::read_dir(agents) else { return };
    for e in entries.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<i32>().ok()) else { continue };
        // SAFETY: signal 0 only asks whether the process is there.
        let gone = pid > 1
            && unsafe { libc::kill(pid, 0) } != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        if !gone || !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        for s in std::fs::read_dir(e.path()).into_iter().flatten().flatten() {
            if s.file_type().is_ok_and(|t| t.is_socket()) {
                let _ = std::fs::remove_file(s.path());
            }
        }
        let _ = std::fs::remove_dir(e.path());
    }
}

/// The listener: a connection's user and ancestry checked here (quick reads), its token on a thread of its own.
fn accept(listener: UnixListener, s: Arc<Shared>) {
    let mut n = 0u64;
    for stream in listener.incoming() {
        if s.stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else { continue };
        // SAFETY: geteuid only reads.
        let me = unsafe { libc::geteuid() };
        let judged = match crate::uisocket::peer_cred(&stream) {
            Ok((uid, pid)) => judged(uid, me, &crate::uisocket::ancestors(pid), s.agent),
            Err(_) => Err(Refused::Unknown),
        };
        if let Err(why) = judged {
            s.refused(why);
            continue;
        }
        if s.live.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
            s.refused(Refused::Second);
            continue;
        }
        n += 1;
        let Ok(kept) = stream.try_clone() else { continue };
        s.open.lock().unwrap_or_else(|e| e.into_inner()).push((n, kept));
        let s = s.clone();
        crate::event::thread("tarae-bridge-in", move || {
            connection(n, &stream, &s);
            s.open.lock().unwrap_or_else(|e| e.into_inner()).retain(|(k, _)| *k != n);
        });
    }
}

/// What `line` read.
enum Line {
    /// A line, its newline taken off (the last, unended, as it is).
    Got(Vec<u8>),
    /// `max` bytes and no newline among them.
    Long,
    /// Nothing more: the input's end, or a read that failed.
    End,
}

/// `r`'s next line, read to `max` bytes, its newline in them, and never past.
fn line(r: &mut impl BufRead, max: usize) -> Line {
    let mut buf = Vec::new();
    match r.take(max as u64).read_until(b'\n', &mut buf) {
        Ok(0) | Err(_) => Line::End,
        Ok(_) if buf.last() == Some(&b'\n') => {
            buf.pop();
            Line::Got(buf)
        }
        Ok(n) if n == max => Line::Long,
        Ok(_) => Line::Got(buf),
    }
}

/// Connection `n`: its token, then — the one live — its messages to the wire's thread until it ends. Each line read
/// to its bound (`FIRST_MAX`, `LINE_MAX`), never past.
fn connection(n: u64, stream: &UnixStream, s: &Shared) {
    let mut lines = BufReader::new(stream);
    match line(&mut lines, FIRST_MAX) {
        Line::Got(first) if same(first.trim_ascii_end(), s.token.as_bytes()) => {}
        Line::Long => return s.refused(Refused::FirstLong),
        Line::Got(_) | Line::End => return s.refused(Refused::Token),
    }
    {
        let mut live = s.live.lock().unwrap_or_else(|e| e.into_inner());
        if live.is_some() {
            drop(live);
            s.refused(Refused::Second);
            return;
        }
        *live = stream.try_clone().ok();
    }
    if let Ok(out) = stream.try_clone() {
        s.send(In::Linked(n, writer(out)));
    }
    loop {
        let got = match line(&mut lines, LINE_MAX) {
            Line::Got(got) => got,
            Line::Long => break s.refused(Refused::LineLong),
            Line::End => break,
        };
        // (a line that isn't JSON: no message — the relay passes on what the agent's client wrote)
        if let Ok(v) = serde_json::from_slice::<Value>(&got)
            && !s.send(In::Bridged(v))
        {
            break;
        }
    }
    *s.live.lock().unwrap_or_else(|e| e.into_inner()) = None;
    s.send(In::Unlinked(n));
}

/// The live connection's writes, on a thread of their own, lines in the order sent (the relay may be slow to read).
fn writer(mut out: UnixStream) -> Sender<String> {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    crate::event::thread("tarae-bridge-wr", move || {
        while let Ok(line) = rx.recv() {
            if writeln!(out, "{line}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    });
    tx
}

// ── The relay ───────────────────────────────────────────────────────────

/// The relay's test: what a test build's `server` runs.
const RELAY_TEST: &str = "agent::acp::bridge::relay_test";

/// `tarae agent-tools <socket>`: connects (a path past `sun_path` from its folder), writes the token from its
/// environment, then passes `input` to the socket and the socket to `output` (`pass`), two threads, until either
/// side closes. No config read, no state touched.
pub fn relay(
    socket: &Path,
    token: &str,
    mut input: impl Read + Send + 'static,
    mut output: impl Write + Send + 'static,
) -> Result<(), String> {
    let stream = crate::uisocket::at(socket, |p: &Path| UnixStream::connect(p))
        .map_err(|e| format!("can't reach {}: {e}", socket.display()))?;
    let (mut up, mut down) = match (stream.try_clone(), stream.try_clone()) {
        (Ok(up), Ok(down)) => (up, down),
        (Err(e), _) | (_, Err(e)) => return Err(e.to_string()),
    };
    writeln!(up, "{token}").map_err(|e| e.to_string())?;
    let (ended, end) = std::sync::mpsc::channel::<()>();
    let up_ended = ended.clone();
    crate::event::thread("tarae-relay-up", move || {
        pass(&mut input, &mut up);
        let _ = up_ended.send(());
    });
    crate::event::thread("tarae-relay-dn", move || {
        pass(&mut down, &mut output);
        let _ = ended.send(());
    });
    let _ = end.recv();
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

/// `r` to `w` until `r` ends or `w` fails, each read written and flushed as it comes. Never `std::io::copy`: on Linux,
/// between a socket and a pipe it splices, and a splice from a socket into a pipe waits for the socket's next bytes
/// holding the pipe's lock — what it already put in the pipe can't be read meanwhile, so tarae's answer stayed in
/// the agent's stdout pipe until the socket said something more (the relay's test: a hang; the agent's: its MCP
/// server never answering).
fn pass(r: &mut impl Read, w: &mut impl Write) {
    let mut buf = [0u8; 8192];
    loop {
        match r.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                if w.write_all(&buf[..n]).and_then(|_| w.flush()).is_err() {
                    return;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

/// `tarae agent-tools <socket>` (`main.rs`, under the gate with the rest of the wire): `relay` on stdin and stdout.
pub fn cli(args: &[String]) -> anyhow::Result<()> {
    let [socket] = args else { anyhow::bail!("usage: tarae agent-tools <socket>") };
    let token = std::env::var(TOKEN).map_err(|_| anyhow::anyhow!("no {TOKEN}"))?;
    relay(Path::new(socket), &token, std::io::stdin(), std::io::stdout()).map_err(anyhow::Error::msg)
}

/// The relay as a test build's `server` starts it (the test binary re-run on this one test, the socket its last
/// argument): `relay` on stdin and descriptor 3.
#[test]
#[ignore = "the bridge's relay: a stand-in agent's child, never run alone"]
fn relay_test() {
    use std::os::fd::FromRawFd;
    let socket = std::env::args().next_back().filter(|a| a.ends_with(".sock"));
    let (Some(socket), Ok(token)) = (socket, std::env::var(TOKEN)) else { return };
    // SAFETY: descriptor 3 is the agent's pipe, as `server`'s `3>&1` gave it; nothing else here owns it.
    let out = unsafe { std::fs::File::from_raw_fd(3) };
    let _ = relay(Path::new(&socket), &token, std::io::stdin(), out);
}

#[cfg(test)]
mod tests;
