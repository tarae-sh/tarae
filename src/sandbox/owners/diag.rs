//! The sockets on one local port of tarae's network namespace, as the kernel lists them (`NETLINK_SOCK_DIAG`,
//! `inet_diag`): asked per call that may land on another session's (step 8). The port is filtered in the kernel
//! (`INET_DIAG_REQ_BYTECODE`), and a TCP ask lists listeners only — the kernel walks its listening table, not every
//! connection (M8.1: `/proc/net/tcp` walks them all — 1.4 ms with 100 sockets in the namespace, 12 ms with
//! 10 000). Each socket comes with its cookie (`idiag_cookie`, its `SO_COOKIE`: 64-bit, never reused — what
//! `owners` knows it by) and, an IPv6 one, its `IPV6_V6ONLY` (`INET_DIAG_SKV6ONLY`); `/proc/net` says neither.
//!
//! A protocol the kernel can't list is an error, never an empty list (#694): a kernel with `inet_diag` but not the
//! protocol's module (`udp_diag`) ends the dump `NLMSG_DONE` carrying `-ENOENT` (`__inet_diag_dump`, `netlink_dump`),
//! read as an ack. And before a protocol's first answer is believed, a socket of tarae's own is looked for in it
//! (`told`): a dump that leaves it out, however cleanly it ends, sends every lookup of that protocol to tarae's
//! tables from then on (`owners::rows_from`).

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::{Mutex, OnceLock};

use super::Row;
use crate::sandbox::netlink::{self, Header, NLM_F_DUMP, NLM_F_REQUEST, NLMSG_DONE, NLMSG_ERROR};

/// `SOCK_DIAG_BY_FAMILY` (`linux/sock_diag.h`).
const BY_FAMILY: u16 = 20;
/// `INET_DIAG_REQ_BYTECODE`, `INET_DIAG_SKV6ONLY` (`linux/inet_diag.h`).
const REQ_BYTECODE: u16 = 1;
const SKV6ONLY: u16 = 11;
/// `INET_DIAG_BC_S_GE`, `INET_DIAG_BC_S_LE`: the local port at least, at most the next op's `no`; `D_`: the
/// remote port.
const BC_S_GE: u8 = 2;
const BC_S_LE: u8 = 3;
const BC_D_GE: u8 = 4;
const BC_D_LE: u8 = 5;
/// The states asked: a TCP listener (`TCP_LISTEN`); a UDP socket connected to no one (`TCP_CLOSE`).
const LISTEN: u32 = 1 << 10;
const UNCONNECTED: u32 = 1 << 7;
/// `struct inet_diag_msg`'s size: its attributes follow.
const MSG: usize = 72;
/// `NLM_F_DUMP_INTR` (`linux/netlink.h`): the dump's sockets changed while it was made.
const NLM_F_DUMP_INTR: u16 = 0x10;

/// The sockets on local `port` of tarae's network namespace a call may land on — TCP listeners (`tcp`), or UDP
/// sockets connected to no one — IPv4 and IPv6. An answer that doesn't come within a second is an error.
pub fn on_port(tcp: bool, port: u16) -> io::Result<Vec<Row>> {
    checked(told(tcp), || dump(tcp, Some((port, Local)), open(tcp)))
}

/// The states a call lands on: a TCP listener, a UDP socket connected to no one.
fn open(tcp: bool) -> u32 {
    if tcp { LISTEN } else { UNCONNECTED }
}

/// What `ask` answers — where the known-answer check found the dump tells this protocol here (`told`); else an
/// error saying why, and `ask` isn't made.
pub(super) fn checked<T>(told: Result<(), String>, ask: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    told.map_err(|why| io::Error::other(format!("not told by sock_diag here: {why}")))?;
    ask()
}

/// Whether the dump tells the protocol's sockets (`tcp`, else UDP) here: `knows` asked before the first answer is
/// believed, and kept once it's sure (once per tarae and protocol); one it isn't sure of is asked again next time.
fn told(tcp: bool) -> Result<(), String> {
    static TOLD: [Check; 2] = [Check::new(), Check::new()];
    told_by(&TOLD[usize::from(tcp)], tcp, |port| dump(tcp, Some((port, Local)), open(tcp)))
}

/// One protocol's known-answer check: its answer once sure, and what runs it one at a time.
pub(super) struct Check {
    kept: OnceLock<Result<(), String>>,
    running: Mutex<()>,
}

impl Check {
    pub(super) const fn new() -> Check {
        Check { kept: OnceLock::new(), running: Mutex::new(()) }
    }
}

/// `told` by `c`: its kept answer, else `knows` asked (`ask`: the dump of a port) — one at a time: a second thread
/// waits for the first's answer (the check takes about 0.1 ms; its dump's second at most per try) and takes it if
/// that one was sure, else asks itself. Waiting, not the tables meanwhile: it's a worker's wait (a served call's, a
/// sweep's), never the main loop's, and the answer is in a moment — at worst, a kernel answering `EAGAIN` every time,
/// served calls queue a second each behind one another (never seen; fails closed; friction #699).
pub(super) fn told_by(
    c: &Check,
    tcp: bool,
    ask: impl FnOnce(u16) -> io::Result<Vec<Row>>,
) -> Result<(), String> {
    if let Some(t) = c.kept.get() {
        return t.clone();
    }
    let _one = c.running.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = c.kept.get() {
        return t.clone();
    }
    match knows(tcp, ask) {
        Known::Told => c.kept.get_or_init(|| Ok(())).clone(),
        Known::NotTold(why) => c.kept.get_or_init(|| Err(why)).clone(),
        Known::Unsure(why) => Err(why),
    }
}

/// The errnos the check can meet that can't change while tarae runs: the answer "not told", kept.
const FOR_GOOD: &[(i32, &str)] = &[
    (libc::ENOENT, "a dump's end: the kernel has no diag module for the protocol, and couldn't load one"),
    (libc::EPROTO, "a dump's end with no code: how this kernel ends one"),
    (libc::EPROTONOSUPPORT, "no NETLINK_SOCK_DIAG in this kernel"),
    (libc::EAFNOSUPPORT, "no AF_NETLINK, or no IPv4 for its own socket, in this kernel"),
    (libc::ENOPROTOOPT, "no SO_COOKIE (a kernel before 4.13)"),
    (libc::EPERM, "a seccomp filter or an LSM refuses it: its policy is the process's, for its life"),
    (libc::EACCES, "the same, as an LSM says it"),
];

/// The errnos that pass: asked again next time, the tables meanwhile. So is any errno on neither list, a datagram
/// cut short and a dump interrupted twice — retrying costs a bind and a dump; keeping a wrong "not told" would cost
/// the tables for good.
const PASSING: &[(i32, &str)] = &[
    (libc::EMFILE, "tarae's descriptors all open: some close"),
    (libc::ENFILE, "the system's descriptors all open: the same"),
    (libc::ENOBUFS, "the kernel short of memory for the socket or the dump"),
    (libc::ENOMEM, "the same"),
    (libc::EINTR, "a signal came during the wait"),
    (libc::EAGAIN, "no answer within the second (SO_RCVTIMEO)"),
];

/// Whether `e` says the check's answer holds for tarae's life (`FOR_GOOD`, and not `PASSING`).
fn for_good(e: &io::Error) -> bool {
    let on = |list: &[(i32, &str)]| e.raw_os_error().is_some_and(|n| list.iter().any(|(f, _)| *f == n));
    on(FOR_GOOD) && !on(PASSING)
}

/// The known-answer check's answer.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Known {
    /// The dump listed tarae's own socket.
    Told,
    /// It can't, for good: it ended clean without it, or an errno that holds for tarae's life (`FOR_GOOD`: no diag
    /// module, no sock_diag, no `SO_COOKIE`, a seccomp filter's or an LSM's refusal).
    NotTold(String),
    /// Not known this time (`PASSING`: its socket not made — `EMFILE` —, no answer within the second; a dump
    /// interrupted twice): not kept, asked again; meanwhile the tables.
    Unsure(String),
}

#[cfg(test)]
impl Known {
    /// As `checked` takes it: Err, why the dump isn't believed (tests: `told` keeps it so).
    pub(super) fn told(self) -> Result<(), String> {
        match self {
            Known::Told => Ok(()),
            Known::NotTold(why) | Known::Unsure(why) => Err(why),
        }
    }
}

/// The known-answer check: a socket of tarae's own on loopback — a TCP listener, a UDP socket — expected in what
/// `ask` (the dump of its port) lists, by its cookie.
pub(super) fn knows(tcp: bool, ask: impl FnOnce(u16) -> io::Result<Vec<Row>>) -> Known {
    match knows_why(tcp, ask) {
        Ok(()) => Known::Told,
        Err((why, true)) => Known::NotTold(why),
        Err((why, false)) => Known::Unsure(why),
    }
}

/// `knows`, its Err why and whether that's for good.
fn knows_why(tcp: bool, ask: impl FnOnce(u16) -> io::Result<Vec<Row>>) -> Result<(), (String, bool)> {
    let proto = if tcp { "TCP" } else { "UDP" };
    let own = |e: io::Error| (format!("{proto}: a socket of tarae's own to look for: {e}"), for_good(&e));
    let lo = (Ipv4Addr::LOCALHOST, 0);
    let (port, fd) = match tcp {
        true => std::net::TcpListener::bind(lo).and_then(|s| Ok((s.local_addr()?.port(), OwnedFd::from(s)))),
        false => std::net::UdpSocket::bind(lo).and_then(|s| Ok((s.local_addr()?.port(), OwnedFd::from(s)))),
    }
    .map_err(own)?;
    let cookie = cookie(&fd).map_err(own)?;
    let rows = ask(port).map_err(|e| (format!("{proto}: {e}"), for_good(&e)))?;
    match rows.iter().any(|r| r.cookie == Some(cookie)) {
        true => Ok(()),
        false => Err((format!("{proto}: the dump ended without tarae's own socket on port {port}"), true)),
    }
}

/// `fd`'s `SO_COOKIE`.
fn cookie(fd: &OwnedFd) -> io::Result<u64> {
    let (mut v, mut len) = (0u64, std::mem::size_of::<u64>() as libc::socklen_t);
    // SAFETY: a u64 and its length, live for the call, on a descriptor of ours.
    let r = unsafe {
        libc::getsockopt(fd.as_raw_fd(), libc::SOL_SOCKET, libc::SO_COOKIE, (&raw mut v).cast(), &mut len)
    };
    if r != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(v)
}

/// The sockets a bind or a listen on local `port` could take from (`Owners::another_on`): TCP listeners there — the
/// kernel lists a bound TCP socket only once it listens —; UDP sockets there in any state, and UDP sockets
/// connected to that port (`Row.peer`): their plain sends, never served, land on what's bound there.
pub fn bound_on(tcp: bool, port: u16) -> io::Result<Vec<Row>> {
    match tcp {
        true => on_port(true, port),
        false => checked(told(false), || {
            let mut rows = dump(false, Some((port, Local)), !0)?;
            rows.extend(dump(false, Some((port, Remote)), !0)?);
            Ok(rows)
        }),
    }
}

/// Which end of a socket a port filter asks about.
#[derive(Clone, Copy)]
enum End {
    Local,
    Remote,
}
use End::*;

/// The cookies of every socket of the protocol (`tcp`, else UDP) in tarae's network namespace the kernel lists —
/// bound, listening or connected, in any state (an unbound one isn't listed): what a sweep keeps.
pub fn every(tcp: bool) -> io::Result<std::collections::HashSet<u64>> {
    checked(told(tcp), || Ok(dump(tcp, None, !0)?.into_iter().filter_map(|r| r.cookie).collect()))
}

/// The sockets of the protocol (`tcp`, else UDP) in `states`, IPv4 and IPv6, with `port` at that end where given.
fn dump(tcp: bool, port: Option<(u16, End)>, states: u32) -> io::Result<Vec<Row>> {
    // SAFETY: plain call; a descriptor it makes is ours.
    let fd = unsafe {
        libc::socket(libc::AF_NETLINK, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, libc::NETLINK_SOCK_DIAG)
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: just made.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let second = libc::timeval { tv_sec: 1, tv_usec: 0 };
    // SAFETY: a timeval and its size, live for the call.
    let r = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (&raw const second).cast(),
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        )
    };
    if r != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut rows = Vec::new();
    for (first, family) in [(1, libc::AF_INET), (3, libc::AF_INET6)] {
        rows.extend(retried(|again| {
            let seq = first + u32::from(again);
            let req = request(family as u8, tcp, port, states, seq);
            // SAFETY: the request's bytes, live for the call.
            if unsafe { libc::send(fd.as_raw_fd(), req.as_ptr().cast(), req.len(), 0) } < 0 {
                return Err(io::Error::last_os_error());
            }
            let mut got = Vec::new();
            dumped(&fd, seq, &mut got).map(|()| got)
        })?);
    }
    Ok(rows)
}

/// A dump the kernel marked interrupted (`NLM_F_DUMP_INTR`: the sockets changed while it was made) — its rows may
/// be short of some, so it isn't an answer.
#[derive(Debug)]
pub(super) struct Interrupted;

impl std::fmt::Display for Interrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("sock_diag: the dump was interrupted (NLM_F_DUMP_INTR)")
    }
}

impl std::error::Error for Interrupted {}

/// Whether `e` is a dump the kernel marked interrupted.
pub(super) fn interrupted(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|i| i.is::<Interrupted>())
}

/// `ask` (`again`: the second time), asked once more where its dump was interrupted — interrupted again, Err (the
/// tables are read).
pub(super) fn retried(mut ask: impl FnMut(bool) -> io::Result<Vec<Row>>) -> io::Result<Vec<Row>> {
    match ask(false) {
        Err(e) if interrupted(&e) => ask(true),
        r => r,
    }
}

/// A dump of `family`'s sockets of the protocol (`tcp`, else UDP) in `states` — with `port` at that end where given.
fn request(family: u8, tcp: bool, port: Option<(u16, End)>, states: u32, seq: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(92);
    let len: u32 = 16 + 56 + if port.is_some() { 4 + 16 } else { 0 };
    b.extend(Header { len, kind: BY_FAMILY, flags: NLM_F_REQUEST | NLM_F_DUMP, seq, pid: 0 }.bytes());
    // inet_diag_req_v2: family, protocol, ext, pad, states, then an empty inet_diag_sockid (48 bytes)
    let protocol = if tcp { libc::IPPROTO_TCP } else { libc::IPPROTO_UDP } as u8;
    b.extend([family, protocol, 0, 0]);
    b.extend(states.to_ne_bytes());
    b.extend([0u8; 48]);
    let Some((port, end)) = port else { return b };
    // The filter: the port ≥ `port` (else past the end: rejected), then ≤ it (else rejected) — each op
    // `{code, yes, no}`, the port in the next op's `no`; a socket is listed when the program ends exactly at its end
    b.extend((4u16 + 16).to_ne_bytes());
    b.extend(REQ_BYTECODE.to_ne_bytes());
    let (ge, le) = match end {
        Local => (BC_S_GE, BC_S_LE),
        Remote => (BC_D_GE, BC_D_LE),
    };
    for (code, no) in [(ge, 16u16 + 4), (le, 8 + 4)] {
        b.extend([code, 8]);
        b.extend(no.to_ne_bytes());
        b.extend([0, 0]);
        b.extend(port.to_ne_bytes());
    }
    debug_assert_eq!(b.len(), len as usize);
    b
}

/// The answers to request `seq` until its `NLMSG_DONE`, each socket a row (`open`: asked in the states a call lands
/// on, for a lookup).
fn dumped(fd: &OwnedFd, seq: u32, rows: &mut Vec<Row>) -> io::Result<()> {
    let mut buf = vec![0u8; 32 * 1024];
    let mut intr = false;
    loop {
        // SAFETY: a buffer of ours and its length.
        let n = unsafe { libc::recv(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len(), 0) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        if let Some(end) = replies(&buf[..n as usize], seq, rows, &mut intr) {
            return end;
        }
    }
}

/// One datagram of the kernel's answers to request `seq`, each socket a row — Some once they end: their
/// `NLMSG_DONE` (its code read as an ack's: a dump the kernel couldn't make ends there with its errno — a protocol
/// with no diag module, `ENOENT`; a code cut short, `EPROTO`), their error, or a message cut short. One of them
/// marked `NLM_F_DUMP_INTR` (`intr`, kept across the dump's datagrams): a clean end is `Interrupted`.
fn replies(datagram: &[u8], seq: u32, rows: &mut Vec<Row>, intr: &mut bool) -> Option<io::Result<()>> {
    for m in netlink::messages(datagram) {
        let Ok((h, payload)) = m else {
            return Some(Err(io::Error::other("sock_diag: a message cut short")));
        };
        if h.seq == seq && h.flags & NLM_F_DUMP_INTR != 0 {
            *intr = true;
        }
        match h.kind {
            _ if h.seq != seq => {}
            NLMSG_DONE | NLMSG_ERROR => {
                let end = netlink::ack(payload).map_err(io::Error::from_raw_os_error);
                return Some(
                    end.and_then(|()| if *intr { Err(io::Error::other(Interrupted)) } else { Ok(()) }),
                );
            }
            BY_FAMILY => rows.extend(row(payload)),
            _ => {}
        }
    }
    None
}

/// One `inet_diag_msg` and its attributes as a row.
fn row(m: &[u8]) -> Option<Row> {
    if m.len() < MSG {
        return None;
    }
    // (family, state, timer, retrans; the sockid: sport, dport — network order —, src, dst, if, cookie; … inode)
    let (port, peer_port) = (u16::from_be_bytes([m[4], m[5]]), u16::from_be_bytes([m[6], m[7]]));
    let ip = |at: usize| match m[0] as i32 {
        libc::AF_INET => Some(IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(&m[at..at + 4]).ok()?))),
        libc::AF_INET6 => Some(IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&m[at..at + 16]).ok()?))),
        _ => None,
    };
    let (ip, peer) = (ip(8)?, ip(24)?);
    let word = |i: usize| m[i..i + 4].try_into().ok().map(u32::from_ne_bytes);
    // (`idiag_cookie`: the socket's `SO_COOKIE`, low word first — `sock_diag_save_cookie`)
    let cookie = u64::from(word(44)?) | (u64::from(word(48)?) << 32);
    let inode = word(68)?;
    let v6only = netlink::attrs(&m[MSG..])
        .map_while(Result::ok)
        .filter_map(|(kind, v)| v.first().filter(|_| kind == SKV6ONLY))
        .last()
        .is_some_and(|&b| b != 0);
    let (local, inode, cookie) = (SocketAddr::new(ip, port), u64::from(inode), Some(cookie));
    let peer = (peer_port != 0).then(|| SocketAddr::new(peer, peer_port));
    Some(Row { local, inode, cookie, open: true, v6only, peer })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mutate::{self, Rng};

    /// An `inet_diag_msg` as the kernel writes one (`inet_diag_msg_common_fill`, `sock_diag_save_cookie`): the
    /// family, the ports in network order, the addresses (an IPv4 one in the first four bytes), the cookie low word
    /// first, the inode at 68 — then `INET_DIAG_SKV6ONLY`, as an IPv6 socket's answer carries it.
    fn msg(local: SocketAddr, peer: SocketAddr, cookie: u64, inode: u32, v6only: bool) -> Vec<u8> {
        let mut m = vec![0u8; MSG];
        m[0] = if local.is_ipv4() { libc::AF_INET } else { libc::AF_INET6 } as u8;
        m[4..6].copy_from_slice(&local.port().to_be_bytes());
        m[6..8].copy_from_slice(&peer.port().to_be_bytes());
        for (at, ip) in [(8, local.ip()), (24, peer.ip())] {
            let o = match ip {
                IpAddr::V4(v4) => v4.octets().to_vec(),
                IpAddr::V6(v6) => v6.octets().to_vec(),
            };
            m[at..at + o.len()].copy_from_slice(&o);
        }
        m[44..48].copy_from_slice(&(cookie as u32).to_ne_bytes());
        m[48..52].copy_from_slice(&((cookie >> 32) as u32).to_ne_bytes());
        m[68..72].copy_from_slice(&inode.to_ne_bytes());
        if local.is_ipv6() {
            m.extend(5u16.to_ne_bytes());
            m.extend(SKV6ONLY.to_ne_bytes());
            m.extend([u8::from(v6only), 0, 0, 0]);
        }
        m
    }

    /// A datagram of answers to request 2: another request's are passed by, a socket's message is a row, and the
    /// answers end at their `NLMSG_DONE`, their error (one of 16 bytes, with no code, `EPROTO` — never a panic) or a
    /// message cut short; a datagram with no end, None (the next one is read).
    /// One netlink message: its header (`flags`, sequence `seq`), then `payload`, aligned.
    fn flagged(kind: u16, flags: u16, seq: u32, payload: &[u8]) -> Vec<u8> {
        let len = (netlink::HDR_LEN + payload.len()) as u32;
        let mut m = Header { len, kind, flags, seq, pid: 0 }.bytes().to_vec();
        m.extend(payload);
        m.resize(netlink::align(m.len()), 0);
        m
    }

    #[test]
    fn a_datagram_of_answers_ends_where_they_end() {
        let message = |kind: u16, seq: u32, payload: &[u8]| flagged(kind, 0, seq, payload);
        let local = SocketAddr::from(([127, 0, 0, 1], 8080));
        let socket = msg(local, SocketAddr::from(([0; 4], 0)), 9, 41, false);
        let ours = message(BY_FAMILY, 2, &socket);
        let (theirs, done) = (message(BY_FAMILY, 1, &socket), message(NLMSG_DONE, 2, &[0; 4]));
        let mut rows = Vec::new();
        assert!(replies(&[theirs.clone(), ours.clone()].concat(), 2, &mut rows, &mut false).is_none());
        assert_eq!(rows.iter().map(|r| (r.local, r.inode)).collect::<Vec<_>>(), [(local, 41)]);
        let end =
            |d: Vec<u8>| replies(&d, 2, &mut Vec::new(), &mut false).map(|r| r.map_err(|e| e.to_string()));
        assert_eq!(end([ours.clone(), done].concat()), Some(Ok(())));
        let errno = |e: i32| Some(Err(io::Error::from_raw_os_error(e).to_string()));
        assert_eq!(end(message(NLMSG_ERROR, 2, &(-libc::EPERM).to_ne_bytes())), errno(libc::EPERM));
        assert_eq!(end(message(NLMSG_ERROR, 2, &[])), errno(libc::EPROTO));
        // (#694: a protocol with no diag module ends its dump DONE(-ENOENT); a DONE that says nothing isn't a yes)
        assert_eq!(end(message(NLMSG_DONE, 2, &(-libc::ENOENT).to_ne_bytes())), errno(libc::ENOENT));
        assert_eq!(end(message(NLMSG_DONE, 2, &[])), errno(libc::EPROTO));
        let cut = Some(Err("sock_diag: a message cut short".to_string()));
        assert_eq!(end([ours.clone(), ours[..10].to_vec()].concat()), cut);
    }

    /// A dump the kernel marked interrupted (`NLM_F_DUMP_INTR`, on its messages and its DONE — the sockets changed
    /// while it was made) isn't an answer: its clean end reads `Interrupted`, the mark kept from an earlier datagram
    /// of the dump too; it's asked once more, and interrupted again it's an Err (the tables are read) — a second
    /// that isn't is the answer. The known-answer check keeps no "not told" from it.
    #[test]
    fn an_interrupted_dump_is_asked_again_then_refused() {
        assert_eq!(i32::from(NLM_F_DUMP_INTR), libc::NLM_F_DUMP_INTR);
        let socket =
            msg(SocketAddr::from(([127, 0, 0, 1], 8080)), SocketAddr::from(([0; 4], 0)), 9, 41, false);
        let intr = NLM_F_DUMP_INTR;
        let marked = [flagged(BY_FAMILY, intr, 2, &socket), flagged(NLMSG_DONE, intr, 2, &[0; 4])].concat();
        let read = |datagrams: &[Vec<u8>]| -> io::Result<Vec<Row>> {
            let (mut rows, mut intr) = (Vec::new(), false);
            for d in datagrams {
                if let Some(end) = replies(d, 2, &mut rows, &mut intr) {
                    return end.map(|()| rows);
                }
            }
            panic!("no end")
        };
        assert!(read(std::slice::from_ref(&marked)).is_err_and(|e| interrupted(&e)), "the DONE marked");
        // (marked on a row in one datagram, the DONE unmarked in the next)
        let split = [flagged(BY_FAMILY, intr, 2, &socket), flagged(NLMSG_DONE, 0, 2, &[0; 4])];
        assert!(read(&split).is_err_and(|e| interrupted(&e)), "marked earlier in the dump");
        let clean = [flagged(BY_FAMILY, 0, 2, &socket), flagged(NLMSG_DONE, 0, 2, &[0; 4])].concat();
        assert_eq!(read(std::slice::from_ref(&clean)).unwrap().len(), 1);
        let asked = std::cell::Cell::new(Vec::new());
        let ask = |answers: [&Vec<u8>; 2]| {
            retried(|again| {
                let mut a = asked.take();
                a.push(again);
                asked.set(a);
                read(std::slice::from_ref(answers[usize::from(again)]))
            })
        };
        let twice = ask([&marked, &marked]);
        assert!(twice.as_ref().is_err_and(interrupted), "interrupted twice: {twice:?}");
        assert_eq!(asked.take(), [false, true], "asked again once");
        assert_eq!(ask([&marked, &clean]).unwrap().len(), 1, "the second answer");
        assert_eq!(asked.take(), [false, true]);
        assert_eq!(ask([&clean, &marked]).unwrap().len(), 1, "a clean first: no second");
        assert_eq!(asked.take(), [false]);
        let unsure = knows(false, |_| read(std::slice::from_ref(&marked)));
        assert!(matches!(unsure, Known::Unsure(_)), "{unsure:?}");
    }

    /// The known-answer check keeps only what it's sure of: its own socket listed — told; a clean dump without it,
    /// or the kernel's own "can't" (`ENOENT`, `EPROTO`) — not told, for good; anything else (`EMFILE`, a second with
    /// no answer) — unsure, asked again next time.
    #[test]
    fn the_known_answer_check_keeps_only_what_its_sure_of() {
        for tcp in [true, false] {
            assert!(matches!(knows(tcp, |_| Ok(Vec::new())), Known::NotTold(_)));
            let lists =
                FOR_GOOD.iter().map(|(e, _)| (*e, true)).chain(PASSING.iter().map(|(e, _)| (*e, false)));
            // (an errno on neither list passes)
            for (e, sure) in lists.chain([(libc::EIO, false)]) {
                let k = knows(tcp, |_| Err(io::Error::from_raw_os_error(e)));
                assert_eq!(matches!(k, Known::NotTold(_)), sure, "{tcp} {e}: {k:?}");
                assert_eq!(matches!(k, Known::Unsure(_)), !sure, "{tcp} {e}: {k:?}");
            }
            let cut = knows(tcp, |_| Err(io::Error::other("sock_diag: a message cut short")));
            assert!(matches!(cut, Known::Unsure(_)), "{cut:?}");
        }
        assert!(FOR_GOOD.iter().all(|(e, _)| !PASSING.iter().any(|(p, _)| p == e)), "on both lists");
        assert_eq!(
            knows(true, |port| dump(true, Some((port, Local)), LISTEN)),
            Known::Told,
            "the kernel's TCP"
        );
    }

    /// Neon round 3: an answer for good is asked once — a lookup, a bind and a sweep after it take it kept, with no
    /// bind or dump of their own —; one that passes is asked each time, one at a time (the check runs under its
    /// lock); a sure one after a passing one is kept from then on.
    #[test]
    fn a_check_for_good_is_made_once_and_one_at_a_time() {
        /// A dump of `c`'s check failing with `e`, counted in `asks`.
        fn failing<'a>(
            c: &'a Check,
            asks: &'a std::cell::Cell<u32>,
            e: i32,
        ) -> impl FnOnce(u16) -> io::Result<Vec<Row>> + 'a {
            move |_| {
                asks.set(asks.get() + 1);
                assert!(c.running.try_lock().is_err(), "the check runs under its lock");
                Err(io::Error::from_raw_os_error(e))
            }
        }
        let (asks, good) = (std::cell::Cell::new(0), Check::new());
        for _ in 0..5 {
            assert!(told_by(&good, false, failing(&good, &asks, libc::EPROTONOSUPPORT)).is_err());
        }
        assert_eq!(asks.get(), 1, "for good: asked once");
        let (asks, passing) = (std::cell::Cell::new(0), Check::new());
        for _ in 0..3 {
            assert!(told_by(&passing, true, failing(&passing, &asks, libc::EMFILE)).is_err());
        }
        assert_eq!(asks.get(), 3, "passing: asked each time");
        assert!(told_by(&passing, true, failing(&passing, &asks, libc::EPERM)).is_err());
        let kept = told_by(&passing, true, failing(&passing, &asks, libc::EMFILE));
        assert!(kept.is_err_and(|e| e.contains("Operation not permitted")));
        assert_eq!(asks.get(), 4, "then kept");
    }

    /// A socket's message as the kernel writes it reads back as that socket — 2,000 random ones of both families —
    /// and any message, mutated 20,000 times (a length past the end, an attribute cut short), is read without a
    /// panic, its inode from the message's own bytes.
    #[test]
    fn a_diag_message_reads_back_and_any_reads_without_a_panic() {
        let mut rng = Rng(0x5eed_0add_7e55_010a);
        let mut seeds = Vec::new();
        for i in 0..2_000 {
            let ip = |rng: &mut Rng| match i % 2 {
                0 => IpAddr::V4(Ipv4Addr::from(rng.next() as u32)),
                _ => IpAddr::V6(Ipv6Addr::from(u128::from(rng.next()) << 64 | u128::from(rng.next()))),
            };
            let local = SocketAddr::new(ip(&mut rng), rng.next() as u16);
            let peer = SocketAddr::new(ip(&mut rng), [0, rng.next() as u16][rng.below(2)]);
            let (cookie, inode, v6only) =
                (rng.next(), rng.next() as u32, local.is_ipv6() && rng.below(2) == 0);
            let m = msg(local, peer, cookie, inode, v6only);
            let want = Row {
                local,
                inode: u64::from(inode),
                cookie: Some(cookie),
                open: true,
                v6only,
                peer: (peer.port() != 0).then_some(peer),
            };
            assert_eq!(row(&m), Some(want), "{m:?}");
            seeds.push(m);
        }
        let words: [&[u8]; 3] = [&[5, 0, 11, 0, 1], &[0xff, 0xff], &[4, 0]];
        for _ in 0..20_000 {
            let mut m = rng.pick(&seeds).clone();
            mutate::bytes(&mut rng, &mut m, &words);
            if let Some(r) = row(&m) {
                assert_eq!(r.inode, u64::from(u32::from_ne_bytes(m[68..72].try_into().unwrap())));
            }
        }
    }
}
