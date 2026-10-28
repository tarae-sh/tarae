//! The proxy's way out as the relay sees it (`Out`): a socket to the host or to the upstream — or, through an
//! https:// upstream, one end of a pair whose other end a bridge holds: the TLS to that upstream on a thread of its
//! own, what the relay writes encrypted to it and what it answers decrypted back. The relay (its `poll`, Linux's
//! `splice`) works on the pair's end as on any socket; one thread and a copy more, only through an https://
//! upstream.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;

/// What one read takes, either way.
const CHUNK: usize = 16 * 1024;
/// The most decrypted bytes waiting for the relay before the upstream isn't read any more.
const PENDING_MAX: usize = 64 * 1024;

/// The way out: a connection made, or the bridge's end.
#[derive(Debug)]
pub enum Out {
    Tcp(TcpStream),
    Bridged(UnixStream),
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
            Out::Bridged(s) => s.set_nonblocking(on),
        }
    }

    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        match self {
            Out::Tcp(s) => s.shutdown(how),
            Out::Bridged(s) => s.shutdown(how),
        }
    }

    /// Bytes there without taking them.
    pub fn peek(&self, b: &mut [u8]) -> io::Result<usize> {
        match self {
            Out::Tcp(s) => s.peek(b),
            // SAFETY: a socket of ours, a buffer of `b.len()` bytes.
            Out::Bridged(s) => {
                match unsafe { libc::recv(s.as_raw_fd(), b.as_mut_ptr().cast(), b.len(), libc::MSG_PEEK) } {
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
            Out::Bridged(s) => s.as_raw_fd(),
        }
    }
}

impl Read for &Out {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        match self {
            Out::Tcp(s) => (&*s).read(b),
            Out::Bridged(s) => (&*s).read(b),
        }
    }
}

impl Write for &Out {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        match self {
            Out::Tcp(s) => (&*s).write(b),
            Out::Bridged(s) => send(s.as_raw_fd(), b),
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

/// The bridge for `conn` — TLS to the upstream on `tcp`, its handshake done — started: the relay's end returned,
/// the thread holding the other until either side is done.
pub fn start(conn: rustls::ClientConnection, tcp: TcpStream) -> io::Result<Out> {
    let (relay, mine) = pair()?;
    std::thread::Builder::new().name("tarae-proxy-tls".into()).spawn(move || run(conn, tcp, mine))?;
    Ok(Out::Bridged(relay))
}

/// The bridge's loop (`poll` on both): what the relay writes goes to the upstream encrypted, what the upstream
/// sends to the relay decrypted. The relay's end closed → close_notify, then the upstream's writing shut down; the
/// upstream's close_notify (or its end) → the relay's reading end shut down once what's decrypted is written.
/// Anything else that fails ends both: the relay sees its side end. A record that doesn't decrypt is never passed on.
fn run(mut conn: rustls::ClientConnection, tcp: TcpStream, pair: UnixStream) {
    if tcp.set_nonblocking(true).is_err() || pair.set_nonblocking(true).is_err() {
        return;
    }
    // (to the relay: decrypted, not yet written; the relay's end read to its end; the upstream's TLS ended)
    let (mut out, mut relay_done, mut tls_done) = (Vec::new(), false, false);
    let (mut out_shut, mut tcp_shut) = (false, false);
    // hup[i]: that side hung up — left out of `poll` while it's neither read nor written to, so it can't spin the
    // loop (as the relay's)
    let mut hup = [false; 2];
    let mut buf = vec![0u8; CHUNK];
    let end = || {
        let _ = tcp.shutdown(Shutdown::Both);
        let _ = pair.shutdown(Shutdown::Both);
    };
    loop {
        // (what's decrypted taken out of rustls, up to what the relay may wait for)
        while !tls_done && out.len() < PENDING_MAX {
            match conn.reader().read(&mut buf) {
                Ok(0) => tls_done = true,
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                // (its end with no close_notify: what came is all there is — the tunnel inside is TLS of its own)
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
        let wants_tls_out = conn.wants_write();
        let read_tcp = !tls_done && out.len() < PENDING_MAX;
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
        ];
        // SAFETY: two pollfds of ours, their count.
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return end();
        }
        if fds.iter().any(|f| f.revents & (libc::POLLERR | libc::POLLNVAL) != 0) {
            return end();
        }
        for (h, f) in hup.iter_mut().zip(&fds) {
            *h |= f.revents & libc::POLLHUP != 0;
        }
        let ready = libc::POLLIN | libc::POLLHUP;
        if read_tcp && fds[0].revents & ready != 0 {
            match conn.read_tls(&mut &tcp) {
                Ok(0) => tls_done = true,
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
