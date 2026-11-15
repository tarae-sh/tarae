//! The proxy's way out as the relay sees it (`Out`): a socket to the host or to the upstream — or, through an
//! https:// upstream, one end of a pair whose other end a bridge holds: the TLS to that upstream on a thread of its
//! own, what the relay writes encrypted to it and what it answers decrypted back. The relay (its `poll`, Linux's
//! `splice`) works on the pair's end as on any socket; one thread and a copy more, only through an https://
//! upstream.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// What one read takes, either way.
const CHUNK: usize = 16 * 1024;
/// The most decrypted bytes waiting for the relay before the upstream isn't read any more.
const PENDING_MAX: usize = 64 * 1024;

/// The way out: a connection made, or the bridge's end — with its life line (`Line`: the pair alone can't tell a
/// relay gone from one only half-closed). A bridge's end let go is a truncation (no close_notify) unless the relay
/// shut its writing first and waited (the stage's `close`): `line` before `pair`, fields dropping in their order —
/// the line's byte is there before the pair's end can read as the relay's close.
#[derive(Debug)]
pub enum Out {
    Tcp(TcpStream),
    Bridged { line: Line, pair: UnixStream },
}

/// A bridge's reading end of its life line, held while its thread runs — None once it's over.
type Reader = Arc<Mutex<Option<io::PipeReader>>>;

fn lock(r: &Reader) -> MutexGuard<'_, Option<io::PipeReader>> {
    r.lock().unwrap_or_else(|e| e.into_inner())
}

/// The relay's half of a bridge's life line: the writing end of a pipe the bridge's thread watches. Let go — by
/// `abandon`, an early return, a panic's unwind, or after the stage's `close` — it writes one byte, then closes: the
/// bridge's thread ends both sides with no close_notify, the TLS's reader seeing a truncation, whatever else holds a
/// copy of its ends (a child forked that moment by any thread's spawn holds them until its exec closes them, in an
/// order of its own — the pair's end closed first read as the relay's close, a clean end to the run's TLS, #605).
/// So a clean end is only ever the relay's own: its writing shut while the line lives. The byte only while the bridge
/// still reads it (under its lock, `Alive`): never into a pipe nobody reads (`EPIPE`, and a SIGPIPE tarae lives
/// through only while it's ignored — a restart's window resets it, `restart::Signals`).
#[derive(Debug)]
pub struct Line {
    w: io::PipeWriter,
    alive: Reader,
}

impl Drop for Line {
    fn drop(&mut self) {
        let alive = lock(&self.alive);
        if alive.is_some() {
            // (one byte into a pipe nothing else writes: never blocks)
            let _ = (&self.w).write_all(&[0]);
        }
    }
}

/// The bridge's half: the reading end, let go under its lock as the bridge's thread ends (any return).
struct Alive(Reader);

impl Drop for Alive {
    fn drop(&mut self) {
        lock(&self.0).take();
    }
}

impl From<TcpStream> for Out {
    fn from(s: TcpStream) -> Out {
        Out::Tcp(s)
    }
}

impl Out {
    pub fn set_nonblocking(&self, on: bool) -> io::Result<()> {
        match self {
            Out::Tcp(s) => s.set_nonblocking(on),
            Out::Bridged { pair, .. } => pair.set_nonblocking(on),
        }
    }

    /// Its reads' and writes' time limit (None: none) — for a blocking handshake over it (`terminate`).
    pub fn set_timeouts(&self, t: Option<std::time::Duration>) -> io::Result<()> {
        match self {
            Out::Tcp(s) => s.set_read_timeout(t).and_then(|()| s.set_write_timeout(t)),
            Out::Bridged { pair, .. } => pair.set_read_timeout(t).and_then(|()| pair.set_write_timeout(t)),
        }
    }

    /// Tests: a connection's own address (a bridge's end has none).
    #[cfg(test)]
    fn local_addr(&self) -> Option<std::net::SocketAddr> {
        match self {
            Out::Tcp(s) => s.local_addr().ok(),
            Out::Bridged { .. } => None,
        }
    }

    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        match self {
            Out::Tcp(s) => s.shutdown(how),
            Out::Bridged { pair, .. } => pair.shutdown(how),
        }
    }

    /// A bridge's end: its life line's writing end — `poll` on it with no events says (`POLLHUP`, `POLLERR`) once the
    /// bridge's thread is over, its reading end gone with it. None: a connection.
    pub fn life_line(&self) -> Option<RawFd> {
        match self {
            Out::Tcp(_) => None,
            Out::Bridged { line, .. } => Some(line.w.as_raw_fd()),
        }
    }

    /// Let go with no end said: a connection shut both ways (whatever holds a copy of it); a bridge's end as any
    /// letting go of one is, by its `Line` — its thread ends both sides with no close_notify, the TLS's reader seeing
    /// a truncation.
    pub fn abandon(self) {
        if let Out::Tcp(s) = &self {
            let _ = s.shutdown(Shutdown::Both);
        }
    }

    /// Bytes there without taking them.
    pub fn peek(&self, b: &mut [u8]) -> io::Result<usize> {
        match self {
            Out::Tcp(s) => s.peek(b),
            // SAFETY: a socket of ours, a buffer of `b.len()` bytes.
            Out::Bridged { pair, .. } => {
                match unsafe { libc::recv(pair.as_raw_fd(), b.as_mut_ptr().cast(), b.len(), libc::MSG_PEEK) }
                {
                    n if n >= 0 => Ok(n as usize),
                    _ => Err(io::Error::last_os_error()),
                }
            }
        }
    }
}

impl AsRawFd for Out {
    fn as_raw_fd(&self) -> RawFd {
        match self {
            Out::Tcp(s) => s.as_raw_fd(),
            Out::Bridged { pair, .. } => pair.as_raw_fd(),
        }
    }
}

impl Read for &Out {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        match self {
            Out::Tcp(s) => (&*s).read(b),
            Out::Bridged { pair, .. } => (&*pair).read(b),
        }
    }
}

impl Write for &Out {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        match self {
            Out::Tcp(s) => (&*s).write(b),
            Out::Bridged { pair, .. } => send(pair.as_raw_fd(), b),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A write to one end of the pair that never raises SIGPIPE when the other is gone (`MSG_NOSIGNAL` on Linux; the
/// pair's `SO_NOSIGPIPE` on macOS): tarae lives through SIGPIPE only while it's ignored, and a restart's window
/// resets it (`restart::Signals`).
fn send(fd: RawFd, b: &[u8]) -> io::Result<usize> {
    #[cfg(target_os = "linux")]
    let flags = libc::MSG_NOSIGNAL;
    #[cfg(not(target_os = "linux"))]
    let flags = 0;
    // SAFETY: a socket of ours, `b`'s bytes.
    match unsafe { libc::send(fd, b.as_ptr().cast(), b.len(), flags) } {
        n if n >= 0 => Ok(n as usize),
        _ => Err(io::Error::last_os_error()),
    }
}

/// A pair, each end set so a write to it never raises SIGPIPE (macOS: `SO_NOSIGPIPE`; Linux: `send`'s flag).
fn pair() -> io::Result<(UnixStream, UnixStream)> {
    let (a, b) = UnixStream::pair()?;
    #[cfg(target_os = "macos")]
    for s in [&a, &b] {
        let on: libc::c_int = 1;
        // SAFETY: a socket of ours, an int option.
        let r = unsafe {
            libc::setsockopt(
                s.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_NOSIGPIPE,
                (&on as *const libc::c_int).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if r != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok((a, b))
}

/// The bridge for `conn` — TLS on `tcp`, its handshake done: to an https:// upstream; or a terminated tunnel's
/// either leg (`terminate`: the run's as its server, over the run's connection; the host's as its client, over the
/// way out — itself a bridge's end through an https:// upstream, TLS inside TLS) — started: the relay's end
/// returned, the thread holding the other until both sides are done, or the relay has let its end go (as the relay
/// itself lives only as long as its two sides, and srt's tunnel ends one side with the other's close).
pub fn start(conn: rustls::Connection, tcp: Out) -> io::Result<Out> {
    started(conn, tcp, None)
}

/// How a terminated leg's TLS ended, as its stage reads it once its end of the pair reads to its end: with
/// close_notify — or not (no close_notify, a record that doesn't decrypt, a socket that failed: a truncation). Set by
/// the bridge before it shuts the stage's reading end.
#[derive(Clone, Debug, Default)]
pub struct Ending(Arc<AtomicBool>);

impl Ending {
    /// Its TLS ended with close_notify: what came is all there is.
    pub fn clean(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// `start` for a terminated tunnel's leg (`terminate`), whose inside is plaintext HTTP: a TLS end without
/// close_notify is no end there — a close-delimited answer is whole only by it — so how it ended goes with it.
pub fn start_leg(conn: rustls::Connection, tcp: Out) -> io::Result<(Out, Ending)> {
    let ending = Ending::default();
    Ok((started(conn, tcp, Some(ending.clone()))?, ending))
}

fn started(conn: rustls::Connection, tcp: Out, ending: Option<Ending>) -> io::Result<Out> {
    let (relay, mine) = pair()?;
    let (r, w) = io::pipe()?;
    let shared: Reader = Arc::new(Mutex::new(Some(r)));
    let (alive, line) = (Alive(shared.clone()), Line { w, alive: shared });
    std::thread::Builder::new()
        .name("tarae-proxy-tls".into())
        .spawn(move || run(conn, tcp, mine, alive, ending))?;
    Ok(Out::Bridged { line, pair: relay })
}

/// Tests: the upstream-side addresses (the bridge's own end) of the bridges whose thread still runs.
#[cfg(test)]
static LIVE: std::sync::Mutex<Vec<std::net::SocketAddr>> = std::sync::Mutex::new(Vec::new());

/// Tests: one bridge's entry in `LIVE`, taken out as its thread ends.
#[cfg(test)]
struct Live(Option<std::net::SocketAddr>);

#[cfg(test)]
impl Live {
    fn of(tcp: &Out) -> Live {
        let at = tcp.local_addr();
        if let Some(at) = at {
            LIVE.lock().unwrap_or_else(|e| e.into_inner()).push(at);
        }
        Live(at)
    }
}

#[cfg(test)]
impl Drop for Live {
    fn drop(&mut self) {
        let mut live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
        live.retain(|a| Some(*a) != self.0);
    }
}

/// Tests: whether the bridge whose upstream side is `at` (the upstream's peer) has ended, by `PATIENCE`.
#[cfg(test)]
pub fn ended(at: std::net::SocketAddr) -> bool {
    let by = std::time::Instant::now() + crate::test_results::PATIENCE;
    loop {
        if !LIVE.lock().unwrap_or_else(|e| e.into_inner()).contains(&at) {
            return true;
        }
        if std::time::Instant::now() > by {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// The bridge's loop (`poll` on both): what the relay writes goes to the upstream encrypted, what the upstream
/// sends to the relay decrypted. The relay's end closed → close_notify, then the upstream's writing shut down; the
/// upstream's close_notify (or its end) → the relay's reading end shut down once what's decrypted — all of it, what
/// rustls still holds too — is written. The relay gone (`alive` says it: its `Line`'s byte, or its end) ends both: nobody
/// reads what the upstream would still send, and an upstream that keeps its side open can't hold the thread.
/// Anything else that fails ends both: the relay sees its side end. A record that doesn't decrypt is never passed on.
/// `ending` (a terminated leg's): set clean at the upstream's close_notify alone.
fn run(mut conn: rustls::Connection, tcp: Out, pair: UnixStream, alive: Alive, ending: Option<Ending>) {
    if tcp.set_nonblocking(true).is_err() || pair.set_nonblocking(true).is_err() {
        return;
    }
    // (held until any return: `Alive` lets it go then, under its lock)
    let line = lock(&alive.0).as_ref().map_or(-1, |r| r.as_raw_fd());
    // (to the relay: decrypted, not yet written; the relay's end read to its end; the upstream's socket read to
    // its end; the upstream's TLS ended — once what rustls holds is taken out, after the socket's end)
    let (mut out, mut relay_done, mut tcp_done, mut tls_done) = (Vec::new(), false, false, false);
    let (mut out_shut, mut tcp_shut) = (false, false);
    // hup[i]: that side hung up — left out of `poll` while it's neither read nor written to, so it can't spin the
    // loop (as the relay's)
    let mut hup = [false; 2];
    let mut buf = vec![0u8; CHUNK];
    let end = || {
        let _ = tcp.shutdown(Shutdown::Both);
        let _ = pair.shutdown(Shutdown::Both);
    };
    #[cfg(test)]
    let _live = Live::of(&tcp);
    loop {
        // (what's decrypted taken out of rustls, up to what the relay may wait for — `held`: rustls still holds
        // some then, and the socket isn't read until it's out: a `read_tls` over it would pass rustls's own limit
        // and fail, or meet the socket's end and lose it)
        let mut held = false;
        while !tls_done {
            if out.len() >= PENDING_MAX {
                held = true;
                break;
            }
            match conn.reader().read(&mut buf) {
                Ok(0) => {
                    tls_done = true;
                    // (before the pair's shutdown below: the stage reads it once its read says the end)
                    if let Some(e) = &ending {
                        e.0.store(true, Ordering::Release);
                    }
                }
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                // (its end with no close_notify: through an https:// upstream what came is all there is — the tunnel
                // inside is TLS of its own; a terminated leg's `ending` stays unclean)
                Err(_) => tls_done = true,
            }
        }
        if !out.is_empty() {
            match send(pair.as_raw_fd(), &out) {
                Ok(n) => {
                    out.drain(..n);
                }
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {}
                Err(_) => return end(),
            }
        }
        if tls_done && out.is_empty() && !out_shut {
            let _ = pair.shutdown(Shutdown::Write);
            out_shut = true;
        }
        if relay_done && !conn.wants_write() && !tcp_shut {
            let _ = tcp.shutdown(Shutdown::Write);
            tcp_shut = true;
        }
        if out_shut && tcp_shut {
            return;
        }
        // (the relay took some: what rustls holds goes next, before anything waits)
        if held && out.len() < PENDING_MAX {
            continue;
        }
        let wants_tls_out = conn.wants_write();
        let read_tcp = !tcp_done && !tls_done && !held && out.len() < PENDING_MAX;
        // (the relay's next bytes only once the last are on their way: rustls's buffer stays small)
        let read_relay = !relay_done && !wants_tls_out;
        let events = [
            if read_tcp { libc::POLLIN } else { 0 } | if wants_tls_out { libc::POLLOUT } else { 0 },
            if read_relay { libc::POLLIN } else { 0 } | if out.is_empty() { 0 } else { libc::POLLOUT },
        ];
        if events == [0, 0] {
            return end();
        }
        let fd = |i: usize, s: RawFd| if hup[i] && events[i] == 0 { -1 } else { s };
        let mut fds = [
            libc::pollfd { fd: fd(0, tcp.as_raw_fd()), events: events[0], revents: 0 },
            libc::pollfd { fd: fd(1, pair.as_raw_fd()), events: events[1], revents: 0 },
            libc::pollfd { fd: line, events: libc::POLLIN, revents: 0 },
        ];
        // SAFETY: three pollfds of ours, their count.
        if unsafe { libc::poll(fds.as_mut_ptr(), 3, -1) } < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return end();
        }
        // (anything on the life line — its `Line`'s byte, or its end — says the relay let its end go)
        if fds[2].revents != 0 || fds.iter().any(|f| f.revents & (libc::POLLERR | libc::POLLNVAL) != 0) {
            return end();
        }
        for (h, f) in hup.iter_mut().zip(&fds[..2]) {
            *h |= f.revents & libc::POLLHUP != 0;
        }
        let ready = libc::POLLIN | libc::POLLHUP;
        if read_tcp && fds[0].revents & ready != 0 {
            match conn.read_tls(&mut &tcp) {
                // (what rustls decrypted before it is still taken out, to its end: `tls_done` comes from there)
                Ok(0) => tcp_done = true,
                Ok(_) => {
                    if conn.process_new_packets().is_err() {
                        // (its alert sent if it can be, then nothing more either way)
                        let _ = conn.write_tls(&mut &tcp);
                        return end();
                    }
                }
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {}
                Err(_) => return end(),
            }
        }
        if read_relay && fds[1].revents & ready != 0 {
            match (&pair).read(&mut buf) {
                Ok(0) => {
                    relay_done = true;
                    conn.send_close_notify();
                }
                Ok(n) => {
                    if conn.writer().write_all(&buf[..n]).is_err() {
                        return end();
                    }
                }
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {}
                Err(_) => return end(),
            }
        }
        while conn.wants_write() {
            match conn.write_tls(&mut &tcp) {
                Ok(_) => {}
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => break,
                Err(_) => return end(),
            }
        }
    }
}
