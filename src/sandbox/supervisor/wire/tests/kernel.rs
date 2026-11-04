//! The kernel's own reading of an address against `parse`'s (V2 step 4, `.sb/v2-fuzz-plan.md` §3): the bytes
//! `parse` read handed to the kernel as the supervisor hands it tarae's copy — a UDP socket's `connect` (no packet
//! goes anywhere) and `getpeername`; a unix socket's `bind` in the test's own scratch folder and the file it makes;
//! an abstract or made-up name's `getsockname`. The kernel reads what tarae read, or refuses. Linux only: the
//! numbers and the reading are Linux's.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::os::fd::{AsFd, FromRawFd};
use std::os::unix::fs::{FileExt, FileTypeExt, MetadataExt};

use super::*;
use crate::sandbox::route::this_machine;
use crate::sandbox::supervisor::{read_mem, send_built};
use crate::scratch::Scratch;

/// Cases each test draws.
const CASES: usize = 10_000;
/// Where a case's address lies in the stand-in memory `address` reads it from.
const AT: u64 = 0x1000;
/// Disagreements listed at most, after the loop (all of them counted).
const LISTED: usize = 20;

fn socket(family: i32, kind: i32) -> Result<OwnedFd, i32> {
    // SAFETY: plain arguments; the descriptor is owned at once.
    let fd = unsafe { libc::socket(family, kind | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(errno());
    }
    // SAFETY: just made, ours alone.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// `connect(s, b, len b)`, the address as a run's call names it.
fn connect(s: &OwnedFd, b: &[u8]) -> Result<(), i32> {
    // SAFETY: `b` lives for the call; the kernel reads at most `b.len()` bytes of it.
    match unsafe { libc::connect(s.as_raw_fd(), b.as_ptr().cast(), b.len() as libc::socklen_t) } {
        0 => Ok(()),
        _ => Err(errno()),
    }
}

/// `bind(s, b, len b)`.
fn bind(s: &OwnedFd, b: &[u8]) -> Result<(), i32> {
    // SAFETY: as `connect`'s.
    match unsafe { libc::bind(s.as_raw_fd(), b.as_ptr().cast(), b.len() as libc::socklen_t) } {
        0 => Ok(()),
        _ => Err(errno()),
    }
}

/// The address the kernel says the socket is connected to (`peer`) or bound at, as it wrote it.
fn name(s: &OwnedFd, peer: bool) -> Result<Vec<u8>, i32> {
    let mut a = [0u8; ADDR_MAX];
    let mut len = a.len() as libc::socklen_t;
    // SAFETY: room for `len` bytes, the most the kernel writes.
    let r = unsafe {
        match peer {
            true => libc::getpeername(s.as_raw_fd(), a.as_mut_ptr().cast(), &mut len),
            false => libc::getsockname(s.as_raw_fd(), a.as_mut_ptr().cast(), &mut len),
        }
    };
    match r {
        0 => Ok(a[..(len as usize).min(a.len())].to_vec()),
        _ => Err(errno()),
    }
}

/// An internet address as the kernel writes one (`struct sockaddr_in`, `sockaddr_in6`), read here by hand — not by
/// `parse`, which is what's checked.
fn kernels(a: &[u8]) -> Option<SocketAddr> {
    let port = u16::from_be_bytes(a.get(2..4)?.try_into().ok()?);
    let ip: IpAddr = match i32::from(u16::from_ne_bytes(a.get(..2)?.try_into().ok()?)) {
        libc::AF_INET => Ipv4Addr::from(<[u8; 4]>::try_from(a.get(4..8)?).ok()?).into(),
        libc::AF_INET6 => Ipv6Addr::from(<[u8; 16]>::try_from(a.get(8..24)?).ok()?).into(),
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

/// What tarae reads of `b` as a call's address: read once (`address`, at most 128 bytes), then `parse`.
fn tarae_reads(b: &[u8]) -> Result<Addr, i32> {
    address(&Fake::default().at(AT, b.to_vec()), AT, b.len() as u64).and_then(|a| parse(&a))
}

/// A `struct sockaddr_in6`: port, no flow label, the address, `scope` (its last four bytes).
fn v6(ip: Ipv6Addr, port: u16, scope: u32) -> Vec<u8> {
    let rest = [&port.to_be_bytes()[..], &[0; 4], &ip.octets(), &scope.to_ne_bytes()].concat();
    sockaddr(AF_INET6, &rest)
}

/// The counts by kind and the disagreements, the first `LISTED` of them in words.
#[derive(Default)]
struct Tally {
    seen: BTreeMap<String, usize>,
    wrong: Vec<String>,
    wrongs: usize,
}

impl Tally {
    fn wrong(&mut self, what: String) {
        self.wrongs += 1;
        if self.wrong.len() < LISTED {
            self.wrong.push(what);
        }
    }

    /// No disagreement, and each of `kinds` seen at least `floor` times — so the loop reached what it checks.
    fn check(&self, kinds: &[&str], floor: usize) {
        assert!(
            self.wrongs == 0,
            "{} disagreements, the first:\n{}\nseen: {:?}",
            self.wrongs,
            self.wrong.join("\n"),
            self.seen
        );
        let short: Vec<&&str> =
            kinds.iter().filter(|k| self.seen.get(**k).is_none_or(|&n| n < floor)).collect();
        assert!(short.is_empty(), "under {floor} of {short:?}: {:?}", self.seen);
    }
}

/// Random addresses as a run's `connect` names them on a UDP socket of each family (nothing is sent): where the
/// kernel connects is what tarae read — the same port and address, an IPv4 one on an IPv6 socket as the mapped one
/// it is (`route::plain`), `0.0.0.0` and `::` (mapped too) as loopback, which `this_machine` takes them for — or
/// it connects nowhere (`getpeername`'s `ENOTCONN`) where tarae read `AF_UNSPEC` or port 0, or it refuses. What
/// tarae refuses (too short for its family, past 128 bytes) the kernel refuses too. Seeded, 10,000 addresses on
/// each socket; IPv6 where the kernel has it (else said).
#[test]
fn the_kernel_connects_where_tarae_read_the_address() {
    let seeds = [
        v4([127, 0, 0, 1], 9),
        v4([127, 0, 0, 1], 0),
        v4([127, 1, 2, 3], 53),
        v4([0, 0, 0, 0], 9),
        v4([224, 0, 0, 1], 5353),
        v4([192, 0, 2, 1], 443),
        [v4([127, 0, 0, 1], 9), vec![0; 110]].concat(),
        v6(Ipv6Addr::LOCALHOST, 9, 0),
        v6(Ipv6Addr::LOCALHOST, 0, 0),
        v6(Ipv6Addr::UNSPECIFIED, 9, 0),
        v6(Ipv4Addr::LOCALHOST.to_ipv6_mapped(), 9, 0),
        v6(Ipv4Addr::UNSPECIFIED.to_ipv6_mapped(), 9, 0),
        v6("fe80::1".parse().unwrap(), 9, 1),
        v6("2001:db8::1".parse().unwrap(), 9, 0),
        v6(Ipv6Addr::LOCALHOST, 9, 0)[..24].to_vec(),
        sockaddr(AF_UNSPEC, &[0; 14]),
        sockaddr(AF_UNIX, b"/x\0"),
        sockaddr(AF_NETLINK, &[0; 10]),
    ];
    let mut families = vec![(libc::AF_INET, "v4")];
    match socket(libc::AF_INET6, libc::SOCK_DGRAM).and_then(|s| connect(&s, &v6(Ipv6Addr::LOCALHOST, 9, 0))) {
        Ok(()) => families.push((libc::AF_INET6, "v6")),
        Err(e) => eprintln!("no IPv6 loopback here (errno {e}): IPv6 sockets left out"),
    }
    let mut rng = Rng(0x5eed_0add_7e55_0004);
    let mut t = Tally::default();
    for _ in 0..CASES {
        let mut b = seeds[rng.below(seeds.len())].clone();
        mutate(&mut rng, &mut b, &[]);
        let read = tarae_reads(&b);
        for &(family, on) in &families {
            let s = socket(family, libc::SOCK_DGRAM).unwrap();
            let what = match (connect(&s, &b), &read) {
                (Err(_), Err(_)) => "both refuse",
                (Err(_), Ok(_)) => "the kernel refuses",
                (Ok(()), Err(e)) => {
                    t.wrong(format!("{on}: tarae refused ({e}) what the kernel took: {b:?}"));
                    continue;
                }
                (Ok(()), Ok(ours)) => match (ours, name(&s, true)) {
                    (Addr::Unspec, Err(libc::ENOTCONN)) => "disconnected",
                    (Addr::Ip(a), Err(libc::ENOTCONN)) if a.port() == 0 => "port 0",
                    (Addr::Ip(a), Ok(peer)) => {
                        let Some(peer) = kernels(&peer) else {
                            t.wrong(format!("{on}: tarae read {a}, the kernel's peer {peer:?}: {b:?}"));
                            continue;
                        };
                        let (mine, its) = (a.ip().to_canonical(), peer.ip().to_canonical());
                        match a.port() == peer.port() {
                            true if mine == its => "the same",
                            true if mine.is_unspecified() && its.is_loopback() && this_machine(a.ip()) => {
                                "this machine"
                            }
                            _ => {
                                t.wrong(format!(
                                    "{on}: tarae read {a}, the kernel connected to {peer}: {b:?}"
                                ));
                                continue;
                            }
                        }
                    }
                    (ours, peer) => {
                        t.wrong(format!(
                            "{on}: tarae read {ours:?}, the kernel connected, its peer {peer:?}: {b:?}"
                        ));
                        continue;
                    }
                },
            };
            *t.seen.entry(format!("{what} {on}")).or_default() += 1;
        }
    }
    let kinds = ["both refuse", "the kernel refuses", "disconnected", "port 0", "the same", "this machine"];
    let kinds: Vec<String> = families.iter().flat_map(|(_, on)| kinds.map(|k| format!("{k} {on}"))).collect();
    t.check(&kinds.iter().map(String::as_str).collect::<Vec<_>>(), 50);
}

/// Random unix addresses as a run's `bind` names them: a path is where the kernel makes the socket — the bytes up
/// to the first NUL, in the test's own scratch folder —, an abstract name (under a prefix of the test's own) is the
/// one `getsockname` says, the family alone is the kernel's own made-up name (a NUL and five hex digits); else the
/// kernel refuses and makes nothing. Seeded, 10,000 addresses. Only a name's bytes are mutated, never the folder's
/// or the prefix, and a `/` in them becomes `_`: nothing is made outside the folder, nor at a path relative to the
/// test's.
#[test]
fn the_kernel_binds_a_unix_socket_where_tarae_read_it() {
    let dir = Scratch::new("wire-kernel");
    let folder = [dir.as_os_str().as_bytes(), b"/"].concat();
    assert!(folder.len() <= 60, "{} leaves too little of a unix name's 108 bytes", dir.display());
    let prefix = format!("\0tarae-wire-{}-", std::process::id()).into_bytes();
    let names: [&[u8]; 6] = [b"s.sock", b"s.sock\0junk", b"\xff\xfe.sock", b"a", &[b'n'; 40], &[b'n'; 107]];
    let mut rng = Rng(0x5eed_0add_7e55_0005);
    let mut t = Tally::default();
    for _ in 0..CASES {
        let mut n = names[rng.below(names.len())].to_vec();
        mutate(&mut rng, &mut n, &[]);
        n.iter_mut().filter(|c| **c == b'/').for_each(|c| *c = b'_');
        let b = match rng.below(5) {
            0 => sockaddr(AF_UNIX, b""),
            1 | 2 => sockaddr(AF_UNIX, &[&prefix[..], &n].concat()),
            _ => sockaddr(AF_UNIX, &[&folder[..], &n].concat()),
        };
        let s = socket(libc::AF_UNIX, libc::SOCK_STREAM).unwrap();
        let bound = bind(&s, &b);
        let made: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().path()).collect();
        let what = match (bound, tarae_reads(&b)) {
            (Err(_), _) if made.is_empty() => "refused",
            (Err(e), _) => {
                t.wrong(format!("the kernel refused ({e}) and made {made:?}: {b:?}"));
                ""
            }
            (Ok(()), Ok(Addr::Path(p))) => match &made[..] {
                [one]
                    if *one == p
                        && std::fs::symlink_metadata(one).is_ok_and(|m| m.file_type().is_socket()) =>
                {
                    "path"
                }
                _ => {
                    t.wrong(format!("tarae read {p:?}, the kernel made {made:?}: {b:?}"));
                    ""
                }
            },
            (Ok(()), Ok(Addr::Abstract(ours))) => match name(&s, false) {
                Ok(its) if made.is_empty() && its.get(2) == Some(&0) && its[3..] == ours => "abstract",
                its => {
                    t.wrong(format!("tarae read @{ours:?}, the kernel bound {its:?}, made {made:?}: {b:?}"));
                    ""
                }
            },
            (Ok(()), Ok(Addr::Unnamed)) => match name(&s, false) {
                Ok(its)
                    if its.len() == 8 && its[2] == 0 && its[3..].iter().all(|c| c.is_ascii_hexdigit()) =>
                {
                    "made up"
                }
                its => {
                    t.wrong(format!("tarae read no name, the kernel bound {its:?}: {b:?}"));
                    ""
                }
            },
            (Ok(()), ours) => {
                t.wrong(format!("tarae read {ours:?}, the kernel bound, made {made:?}: {b:?}"));
                ""
            }
        };
        drop(s);
        // (each case starts from an empty folder: what the kernel made is the test's own, in it)
        made.iter().for_each(|m| std::fs::remove_file(m).unwrap());
        if !what.is_empty() {
            *t.seen.entry(what.into()).or_default() += 1;
        }
    }
    t.check(&["refused", "path", "abstract", "made up"], 300);
}

/// V2-4: the kernel reads a message's control wherever `msg_control` points once `msg_controllen` has a length — a
/// null pointer too, which faults (nothing is mapped at 0 for a run: `vm.mmap_min_addr`) — and `read_msg` reads the
/// same message so: `EFAULT` from both; with no length, both take it. A one-byte datagram over a socket pair of the
/// test's own.
#[test]
fn a_control_length_with_a_null_pointer_faults_in_the_kernel_as_in_tarae() {
    let mut fds = [0; 2];
    // SAFETY: room for the two descriptors `socketpair` writes.
    let made = unsafe {
        libc::socketpair(libc::AF_UNIX, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0, fds.as_mut_ptr())
    };
    assert_eq!(made, 0, "socketpair: {}", errno());
    // SAFETY: just made, ours alone.
    let (s, _peer) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    let byte = [7u8];
    for len in [16usize, 0] {
        let mut iov = libc::iovec { iov_base: byte.as_ptr() as *mut libc::c_void, iov_len: 1 };
        // SAFETY: all zeros is an empty `struct msghdr`.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = std::ptr::null_mut();
        msg.msg_controllen = len as _;
        // SAFETY: the header and its one buffer are ours and live for the call; the null control is what's asked.
        let kernel = match unsafe { libc::sendmsg(s.as_raw_fd(), &msg, libc::MSG_DONTWAIT) } {
            -1 => Err(errno()),
            _ => Ok(()),
        };
        let m = Fake::default()
            .at(HEAD, msghdr((0, 0), (IOV, 1), (0, len as u64)))
            .at(IOV, iovecs(&[(BUFS, 1)]))
            .at(BUFS, byte);
        let tarae = read_msg(&m, passer(&mut Vec::new()), HEAD, true).map(|_| ()).map_err(|u| u.errno());
        assert_eq!(
            kernel,
            if len > 0 { Err(libc::EFAULT) } else { Ok(()) },
            "the kernel, control length {len}"
        );
        assert_eq!(tarae, kernel, "tarae, control length {len}");
    }
}

/// (V2-6) A buffer reaching past where a process's memory ends — an empty one at the top of the address space, one
/// that wraps — faults in the kernel before any buffer is read (`__import_iovec`'s `access_ok`), and `read_msg` says
/// so too: `EFAULT`, where it sent the message (an empty one) or read the others first; a tagged pointer (arm64's top
/// byte) faults in the kernel and in tarae's reads (`read_mem`) — tarae refuses one itself, whatever a kernel's
/// `/proc/<pid>/mem` reads there (arm64's drops the tag: the kernel's, said in the output, never asserted). A
/// one-byte datagram over a socket pair of the test's own, the buffer past user space second. (Past 2^63 only:
/// x86_64's 6.6–6.11 take an empty buffer below it.)
#[test]
fn a_buffer_past_user_space_faults_in_the_kernel_as_in_tarae() {
    let o = Oracle::new();
    let byte = [7u8];
    for (base, len) in [(u64::MAX - 7, 0), (1 << 63, 0), (u64::MAX - 7, 16)] {
        let mut v = [(byte.as_ptr() as u64, 1), (base, len)]
            .map(|(b, l)| libc::iovec { iov_base: b as *mut libc::c_void, iov_len: l as usize });
        // SAFETY: all zeros is an empty `struct msghdr`.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        (msg.msg_iov, msg.msg_iovlen) = (v.as_mut_ptr(), 2);
        // SAFETY: the header and its array are ours and live for the call; the second buffer is what's asked.
        let kernel = match unsafe { libc::sendmsg(o.kernels.0.as_raw_fd(), &msg, libc::MSG_DONTWAIT) } {
            -1 => Err(errno()),
            _ => Ok(()),
        };
        let m = Fake::default()
            .at(HEAD, msghdr((0, 0), (IOV, 2), (0, 0)))
            .at(IOV, iovecs(&[(BUFS, 1), (base, len)]))
            .at(BUFS, byte);
        let tarae = read_msg(&m, passer(&mut Vec::new()), HEAD, true).map(|_| ()).map_err(|u| u.errno());
        assert_eq!(
            (kernel, tarae),
            (Err(libc::EFAULT), Err(libc::EFAULT)),
            "a buffer at {base:#x}, {len} bytes"
        );
        assert_eq!(m.reads.borrow().len(), 2, "no buffer read: {:?}", m.reads.borrow());
    }
    // A lone buffer's reach is cut to `MAX_RW_COUNT` before it's judged (`import_ubuf`, from 6.4's 3b2deb0e46da): a
    // datagram far past the send buffer, the protocol's `EMSGSIZE` (tarae's past 1 MiB), not a fault. Before 6.4
    // `__import_iovec` judged each buffer's whole reach first: the kernel's `EFAULT`, where tarae says its own limit
    // first — both refuse. Its byte in the binary's own image, low in user space, where the cut reach ends inside it —
    // a thread's stack lies where ASLR put it, near the top, and its cut reach may pass the end (`EFAULT` again).
    static LOW: [u8; 1] = [7];
    let low = LOW.as_ptr() as u64;
    assert!(low + MAX_RW_COUNT <= USER_MAX, "a byte at {low:#x}: its cut reach past user space");
    let mut v = [libc::iovec { iov_base: low as *mut libc::c_void, iov_len: 1 << 60 }];
    // SAFETY: as above; the long buffer is what's asked (nothing of it is read).
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    (msg.msg_iov, msg.msg_iovlen) = (v.as_mut_ptr(), 1);
    // SAFETY: as above; the call only reads its buffer.
    let kernel = match unsafe { libc::sendmsg(o.kernels.0.as_raw_fd(), &msg, libc::MSG_DONTWAIT) } {
        -1 => Err(errno()),
        sent => Ok(sent),
    };
    let m = Fake::default().at(HEAD, msghdr((0, 0), (IOV, 1), (0, 0))).at(IOV, iovecs(&[(BUFS, 1 << 60)]));
    let tarae = read_msg(&m, passer(&mut Vec::new()), HEAD, true).map(|_| ()).map_err(|u| u.errno());
    let cut = if release() >= (6, 4) { libc::EMSGSIZE } else { libc::EFAULT };
    assert_eq!((kernel, tarae), (Err(cut), Err(libc::EMSGSIZE)), "a lone long buffer at {low:#x}");
    // A pointer to that byte with a tag in its top byte: the kernel's `EFAULT` (no run here turned the tagged ABI
    // on), and tarae's reads of a run's memory say `EFAULT` too — refused by tarae itself (`USER_MAX`), so what this
    // kernel's `/proc/<pid>/mem` does there (arm64's reads the byte) is only said.
    let tagged = byte.as_ptr() as u64 | 0x2a << 56;
    let mut v = [libc::iovec { iov_base: tagged as *mut libc::c_void, iov_len: 1 }];
    // SAFETY: as above.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    (msg.msg_iov, msg.msg_iovlen) = (v.as_mut_ptr(), 1);
    // SAFETY: as above; the tagged buffer is what's asked.
    let kernel = match unsafe { libc::sendmsg(o.kernels.0.as_raw_fd(), &msg, libc::MSG_DONTWAIT) } {
        -1 => Err(errno()),
        sent => Ok(sent),
    };
    assert_eq!(kernel, Err(libc::EFAULT), "the kernel, a tagged pointer");
    let mut raw = [0u8];
    let proc_reads = o.mem.read_exact_at(&mut raw, tagged);
    eprintln!("/proc/self/mem at {tagged:#x}, tagged: {proc_reads:?} (the kernel's, not asserted)");
    assert_eq!(read_mem(&o.mem, tagged, 1), Err(libc::EFAULT), "tarae's read of it");
    assert_eq!(read_mem(&o.mem, byte.as_ptr() as u64, 1), Ok(byte.to_vec()), "and of the byte itself");
}

/// Marks `the_kernel_takes_what_tarae_sends`' own process.
const MESSAGES_SCENE: &str = "TARAE_WIRE_MESSAGES_SCENE";

/// The kernel as the oracle for a whole message, where `check::kernel`'s model may be wrong (`.sb/cmsg-fuzz-brief.md`
/// item 3): fuzzed messages (`fuzzed`, no name) laid out in real memory at the addresses they hold (`Laid`), each
/// handed to the kernel as it is — a `sendmsg` of the run's own header on a datagram socket pair — and read by
/// `read_msg` from the same memory through `/proc/self/mem` (`read_mem`, as a run's is read), its descriptors taken as
/// copies of tarae's, then sent as tarae sends it (`supervisor::send_built`) on a pair of its own. What arrives on each
/// must be the same — the data, and descriptors of the same files in the same order — or both refused with the same
/// errno, but where they differ by a rule `Oracle::apart` names. In a process of its own: it maps memory at fixed low
/// addresses, and the numbers of its descriptors must stay still while it runs (the kernel looks them up).
#[test]
fn the_kernel_takes_what_tarae_sends() {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "sandbox::wire::tests::kernel::messages_scene",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(MESSAGES_SCENE, "1")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && said.contains("1 passed"), "the scene: {said}");
    eprintln!("{}", said.lines().find(|l| l.starts_with("the kernel and tarae")).unwrap_or_default());
}

/// `the_kernel_takes_what_tarae_sends`' scene, in a process with no other test's threads: 10,000 messages, each the
/// same on both sides or apart by a named rule, and each way of coming out seen at least 100 times.
#[test]
#[ignore = "run by the_kernel_takes_what_tarae_sends in a process of its own"]
fn messages_scene() {
    if std::env::var_os(MESSAGES_SCENE).is_none() {
        return;
    }
    let s = Scratch::new("wire-messages");
    // (each descriptor passed known by the file it is: eight stand-ins of the test's own; then one no process can
    // hold, and one past any table)
    let held: Vec<std::fs::File> =
        (0..8).map(|k| std::fs::File::create(s.join(format!("held-{k}"))).unwrap()).collect();
    let mut fds = [-1; 10];
    fds[9] = i32::MAX;
    for (k, f) in held.iter().enumerate() {
        fds[k] = f.as_raw_fd();
    }
    let o = Oracle::new();
    let mut rng = Rng(0x5eed_0add_7e55_0007);
    let mut t = Tally::default();
    for _ in 0..CASES {
        let (m, at) = fuzzed(&mut rng, &fds, false);
        let laid = Laid::out(&o.mem, &m);
        let came = o.case(at);
        drop(laid);
        match came {
            Ok(what) => *t.seen.entry(what.to_string()).or_default() += 1,
            Err(why) => t.wrong(why),
        }
    }
    let kinds =
        ["sent alike", "sent alike with descriptors", "refused alike", "a control message tarae refuses"];
    t.check(&kinds, 100);
    eprintln!("the kernel and tarae: {:?}", t.seen);
}

/// What came of a message on one side: what arrived (its data; the files of the descriptors it passed, by device
/// and inode) or the errno that refused it.
#[derive(Debug, PartialEq)]
enum Came {
    Sent { data: Vec<u8>, files: Vec<(u64, u64)> },
    Refused(i32),
}

impl std::fmt::Display for Came {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Came::Sent { data, files } => {
                write!(f, "sent {} bytes {:?}…, files {files:?}", data.len(), &data[..data.len().min(16)])
            }
            Came::Refused(e) => write!(f, "refused, errno {e}"),
        }
    }
}

/// The two socket pairs, the memory both sides read, and what the kernel's own limits are here.
struct Oracle {
    /// This process's memory (`/proc/self/mem`): read as a run's is, written to lay a message out.
    mem: std::fs::File,
    kernels: (OwnedFd, OwnedFd),
    taraes: (OwnedFd, OwnedFd),
    /// The most a datagram on these sockets carries (`unix_dgram_sendmsg`: `sk_sndbuf` less 32).
    sndbuf: u64,
    /// The most control the kernel copies (`sock_kmalloc`: `net.core.optmem_max` — `optmem_of`).
    optmem: u64,
    /// Where tarae's copies are numbered from: none of the run's (as in a supervisor, whose table isn't the run's).
    ours_from: i32,
}

impl Oracle {
    fn new() -> Oracle {
        let pair = || {
            let mut fds = [0; 2];
            // SAFETY: room for the two descriptors `socketpair` writes.
            let made = unsafe {
                libc::socketpair(libc::AF_UNIX, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0, fds.as_mut_ptr())
            };
            assert_eq!(made, 0, "socketpair: {}", errno());
            // SAFETY: just made, ours alone.
            unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
        };
        let kernels = pair();
        let sndbuf = sockopt_of(&kernels.0, libc::SO_SNDBUF) - 32;
        assert!(sndbuf < 1 << 18, "a send buffer of {sndbuf}: past what `received` reads");
        let optmem = optmem_of(&kernels.0);
        // SAFETY: a struct the call fills.
        let mut limit: libc::rlimit = unsafe { std::mem::zeroed() };
        // SAFETY: as above.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) }, 0);
        let ours_from = (limit.rlim_cur.min(4096 + 300) - 300) as i32;
        assert!(ours_from > 64, "a descriptor limit of {}", limit.rlim_cur);
        let mem = std::fs::OpenOptions::new().read(true).write(true).open("/proc/self/mem").unwrap();
        Oracle { mem, kernels, taraes: pair(), sndbuf, optmem, ours_from }
    }

    /// The message whose header is at `at`, both ways: alike (how), apart by a rule (which), or what's wrong.
    fn case(&self, at: u64) -> Result<&'static str, String> {
        // SAFETY: `at` is the run's header as it's laid out — wherever it points, the kernel only reads it and
        // what it points to (a fault is its `EFAULT`).
        let sent = unsafe {
            libc::sendmsg(
                self.kernels.0.as_raw_fd(),
                at as *const libc::msghdr,
                libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
            )
        };
        let kernel = if sent < 0 { Came::Refused(errno()) } else { received(&self.kernels.1)? };
        let reads = Reads { mem: &self.mem, reads: std::cell::RefCell::new(Vec::new()) };
        let pass = |n: i32| match n {
            n if n >= self.ours_from => Err(libc::EBADF),
            // SAFETY: a plain call; the copy is owned at once.
            n => match unsafe { libc::fcntl(n, libc::F_DUPFD_CLOEXEC, self.ours_from) } {
                -1 => Err(errno()),
                // SAFETY: just made, ours alone.
                ours => Ok(unsafe { OwnedFd::from_raw_fd(ours) }),
            },
        };
        let tarae = match read_msg(&reads, pass, at, true) {
            Err(Unsent::Control { .. }) => return Ok("a control message tarae refuses"),
            Err(Unsent::Errno(e)) => Came::Refused(e),
            Ok(msg) => {
                let sent = send_built(self.taraes.0.as_fd(), &msg, &[], libc::MSG_DONTWAIT);
                let e = errno();
                drop(msg);
                if sent < 0 { Came::Refused(e) } else { received(&self.taraes.1)? }
            }
        };
        match (&kernel, &tarae) {
            (k, t) if k == t => Ok(match k {
                Came::Sent { files, .. } if files.is_empty() => "sent alike",
                Came::Sent { .. } => "sent alike with descriptors",
                Came::Refused(_) => "refused alike",
            }),
            (k, t) => {
                let failed = reads.failed();
                self.apart(at, k, t, failed).ok_or_else(|| {
                    format!("at {at:#x}: the kernel {k}, tarae {t} ({failed:?}): {}", self.header(at))
                })
            }
        }
    }

    /// The header at `at` as words (name, its length, iovecs, how many, control, its length) and its iovecs, for a
    /// disagreement's line.
    fn header(&self, at: u64) -> String {
        let Ok(h) = read_mem(&self.mem, at, MSGHDR) else { return "no header".into() };
        let long = |o: usize| u64::from_ne_bytes(h[o..o + 8].try_into().unwrap());
        let iov: Vec<String> = (0..long(24).min(4))
            .filter_map(|k| read_mem(&self.mem, long(16).checked_add(16 * k)?, 16).ok())
            .map(|v| {
                format!(
                    "{:#x}+{:#x}",
                    u64::from_ne_bytes(v[..8].try_into().unwrap()),
                    u64::from_ne_bytes(v[8..].try_into().unwrap())
                )
            })
            .collect();
        format!("{:x?} iov {iov:?}", [0, 8, 16, 24, 32, 40, 48].map(long))
    }

    /// Where tarae's answer is known to differ from the kernel's, and why — a rule each, none of them a message
    /// sent apart: (1) the header names an address (mutated into one: where it goes is the supervisor's —
    /// `destination` — and an address's reading is checked on its own above); (2) control past `optmem_max` but within
    /// tarae's 64 KiB: the kernel's `ENOBUFS` before anything of it is read, tarae reads it — and past 64 KiB within
    /// `optmem_max` (128 KiB from 6.8), tarae's `ENOBUFS` where the kernel reads it; (3) a buffer reaching past
    /// `USER_MAX` that this kernel takes (x86_64's 6.6–6.11 check only the top bit: an empty one there), tarae's
    /// `EFAULT`; (4) a datagram past the socket's send buffer: the protocol's `EMSGSIZE` before it copies the data,
    /// tarae reads the data first (up to 1 MiB) and faults; (5) the control is wrong, or isn't there: the kernel copies
    /// it before the protocol takes the data (`____sys_sendmsg`) and checks it before it copies the data
    /// (`scm_send`), tarae reads the data first — and says its own limit on a datagram (1 MiB, `EMSGSIZE`) before
    /// either; (6) the iovec array cut short after a length that isn't an `ssize_t`: the kernel meets the
    /// length, tarae reads the array whole (`check::apart`'s "iovec array cut"). `failed`: the step of tarae's read
    /// that faulted.
    fn apart(&self, at: u64, kernel: &Came, tarae: &Came, failed: Option<Step>) -> Option<&'static str> {
        let h = read_mem(&self.mem, at, MSGHDR).ok()?;
        let long = |o: usize| u64::from_ne_bytes(h[o..o + 8].try_into().unwrap());
        let namelen = u32::from_ne_bytes(h[8..12].try_into().unwrap());
        let (iov, segs, control_len) = (long(16), long(24), long(40));
        let word = |b: &[u8]| u64::from_ne_bytes(b.try_into().unwrap());
        let iovecs: Vec<(u64, u64)> = (0..segs.min(1024))
            .filter_map(|k| read_mem(&self.mem, iov.checked_add(16 * k)?, 16).ok())
            .map(|v| (word(&v[..8]), word(&v[8..])))
            .collect();
        let total: u128 = iovecs.iter().map(|v| u128::from(v.1)).sum();
        let past = iovecs.iter().any(|&(base, len)| base.checked_add(len).is_none_or(|end| end > USER_MAX));
        let control_errno = [libc::EINVAL, libc::EBADF, libc::EPERM, libc::ENOBUFS];
        match (kernel, tarae) {
            _ if long(0) != 0 && namelen != 0 => Some("a name, the supervisor's to follow"),
            (Came::Refused(libc::ENOBUFS), _) if control_len > self.optmem && control_len <= 1 << 16 => {
                Some("control past optmem_max")
            }
            (_, Came::Refused(libc::ENOBUFS)) if control_len > 1 << 16 && control_len <= self.optmem => {
                Some("control past tarae's 64 KiB")
            }
            (_, Came::Refused(libc::EFAULT)) if past => {
                Some("a buffer past USER_MAX, which this kernel takes")
            }
            (Came::Refused(libc::EMSGSIZE), Came::Refused(libc::EFAULT))
                if failed == Some(Step::Data) && total > u128::from(self.sndbuf) =>
            {
                Some("a datagram past the send buffer")
            }
            (Came::Refused(k), Came::Refused(libc::EFAULT))
                if failed == Some(Step::Data) && control_errno.contains(k) =>
            {
                Some("control checked before data")
            }
            (Came::Refused(k), Came::Refused(libc::EMSGSIZE))
                if failed.is_none() && (control_errno.contains(k) || *k == libc::EFAULT) =>
            {
                Some("control checked before data")
            }
            (Came::Refused(libc::EINVAL), Came::Refused(libc::EFAULT)) if failed == Some(Step::Iovecs) => {
                Some("iovec array cut")
            }
            _ => None,
        }
    }
}

/// `read_msg`'s memory here: this process's own, read as a run's is (`read_mem`) — and each read, whether it was
/// all there.
struct Reads<'a> {
    mem: &'a std::fs::File,
    reads: std::cell::RefCell<Vec<(u64, usize, bool)>>,
}

/// Which of tarae's reads of a message (no name) faulted, by its place: the header, the iovec array, a buffer, the
/// control (the last, after every buffer).
#[derive(Debug, PartialEq, Clone, Copy)]
enum Step {
    Header,
    Iovecs,
    Data,
    Control,
}

impl Reads<'_> {
    fn failed(&self) -> Option<Step> {
        let reads = self.reads.borrow();
        let (k, &(at, len, _)) = reads.iter().enumerate().find(|r| !r.1.2)?;
        let control = read_mem(self.mem, reads[0].0, MSGHDR).ok().map(|h| {
            let long = |o: usize| u64::from_ne_bytes(h[o..o + 8].try_into().unwrap());
            (long(32), long(40) as usize)
        });
        Some(match k {
            0 => Step::Header,
            1 => Step::Iovecs,
            _ if control == Some((at, len)) => Step::Control,
            _ => Step::Data,
        })
    }
}

impl Mem for Reads<'_> {
    fn read(&self, at: u64, len: usize) -> Result<Vec<u8>, i32> {
        let r = read_mem(self.mem, at, len);
        self.reads.borrow_mut().push((at, len, r.is_ok()));
        r
    }
}

/// The kernel's limit on a message's control on `s` (`net.core.optmem_max`), as it answers: the shortest control it
/// refuses — `sock_kmalloc` takes a length below the limit (`sk_omem_alloc + size < optmem_max`, none held on a fresh
/// socket), and a control at a null pointer it took faults (`EFAULT`), nothing sent. Where the sysctl is there it's
/// that, checked: one value per network namespace from 6.8 (f5769faeec36, 128 KiB as its parent 4944566706b2 made
/// it); before 6.8 one value for every namespace, shown only in the first — a container's has no file, and the limit
/// is the first namespace's (20 KiB by default on a 64-bit kernel, `sizeof(long) * (2 * UIO_MAXIOV + 512)`).
fn optmem_of(s: &OwnedFd) -> u64 {
    let refused = |len: u64| {
        // SAFETY: all zeros is an empty `struct msghdr`.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_controllen = len as _;
        // SAFETY: the header is ours; its control is a null pointer, which the kernel only tries to read.
        let sent = unsafe { libc::sendmsg(s.as_raw_fd(), &msg, libc::MSG_DONTWAIT) };
        match (sent, errno()) {
            (-1, libc::EFAULT) => false,
            (-1, libc::ENOBUFS) => true,
            (sent, e) => panic!("a control of {len} bytes at a null pointer: sent {sent}, errno {e}"),
        }
    };
    // (past the 36 bytes `____sys_sendmsg` holds on its stack, within `INT_MAX`)
    let (mut taken, mut past) = (64, 1 << 24);
    assert!(!refused(taken) && refused(past), "a control limit outside {taken}..{past}");
    while past - taken > 1 {
        let mid = (taken + past) / 2;
        if refused(mid) {
            past = mid;
        } else {
            taken = mid;
        }
    }
    match std::fs::read_to_string("/proc/sys/net/core/optmem_max") {
        Ok(t) => {
            let said: u64 = t.trim().parse().unwrap();
            assert_eq!(past, said, "the control the kernel refuses first, against optmem_max");
            said
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => past,
        Err(e) => panic!("/proc/sys/net/core/optmem_max: {e}"),
    }
}

/// This kernel's version, major and minor (`/proc/sys/kernel/osrelease`).
fn release() -> (u32, u32) {
    let said = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap();
    let mut n = said.trim().split(|c: char| !c.is_ascii_digit()).map(|n| n.parse().unwrap_or(0));
    (n.next().unwrap_or(0), n.next().unwrap_or(0))
}

/// An integer socket option at `SOL_SOCKET`.
fn sockopt_of(s: &OwnedFd, name: i32) -> u64 {
    let (mut v, mut len) = (0i32, size_of::<i32>() as libc::socklen_t);
    // SAFETY: an int and its length, live for the call.
    let r = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, name, (&raw mut v).cast(), &mut len) };
    assert_eq!(r, 0, "getsockopt: {}", errno());
    v as u64
}

/// The datagram waiting on `on` — a send that said it went must have arrived (else what's wrong): its data, and the
/// files of the descriptors it passed, each closed once looked at.
fn received(on: &OwnedFd) -> Result<Came, String> {
    let mut data = vec![0u8; 1 << 18];
    // (room for `CMSG_SPACE` of 253 descriptors)
    let mut control = [0u64; 160];
    let mut iov = libc::iovec { iov_base: data.as_mut_ptr().cast(), iov_len: data.len() };
    // SAFETY: all zeros is an empty `struct msghdr`; then its buffers, ours and live for the call.
    let mut h: libc::msghdr = unsafe { std::mem::zeroed() };
    (h.msg_iov, h.msg_iovlen) = (&raw mut iov, 1);
    (h.msg_control, h.msg_controllen) = (control.as_mut_ptr().cast(), size_of_val(&control) as _);
    // SAFETY: as above.
    let r = unsafe { libc::recvmsg(on.as_raw_fd(), &raw mut h, libc::MSG_DONTWAIT | libc::MSG_CMSG_CLOEXEC) };
    if r < 0 {
        return Err(format!("sent, and nothing arrived: errno {}", errno()));
    }
    data.truncate(r as usize);
    let mut files = Vec::new();
    // SAFETY: the header the call filled, its control within `control`; the macros walk only what it holds.
    let mut c = unsafe { libc::CMSG_FIRSTHDR(&h) };
    while !c.is_null() {
        // SAFETY: a header within the control the kernel wrote.
        let (level, kind, len) = unsafe { ((*c).cmsg_level, (*c).cmsg_type, (*c).cmsg_len) };
        if (level, kind) == (libc::SOL_SOCKET, libc::SCM_RIGHTS) {
            // SAFETY: its data, `len` less the header, within the control.
            let at = unsafe { libc::CMSG_DATA(c) };
            for k in 0..(len - check::cmsg_len(0)) / 4 {
                // SAFETY: the `k`th descriptor of its data, unaligned as it may be.
                let fd = unsafe { std::ptr::read_unaligned(at.add(4 * k).cast::<i32>()) };
                // SAFETY: a descriptor the message gave us, ours alone.
                let fd = unsafe { OwnedFd::from_raw_fd(fd) };
                let st = std::fs::File::from(fd).metadata().unwrap();
                files.push((st.dev(), st.ino()));
            }
        }
        // SAFETY: the next header the macro finds within the control, or null.
        c = unsafe { libc::CMSG_NXTHDR(&h, c) };
    }
    if h.msg_flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) != 0 {
        return Err(format!("arrived cut short: flags {:#x}", h.msg_flags));
    }
    Ok(Came::Sent { data, files })
}

/// A message's pieces (`Fake`) in this process's memory at the addresses they name: each page they touch mapped
/// (`MAP_FIXED_NOREPLACE`, never over anything there), the pieces written, the rest of each page zero; unmapped
/// again when it's dropped.
struct Laid(Vec<u64>);

impl Laid {
    fn out(mem: &std::fs::File, m: &Fake) -> Laid {
        let mut pages = std::collections::BTreeSet::new();
        for (at, b) in m.pieces.iter().filter(|p| !p.1.is_empty()) {
            pages.extend((at & !0xfff..at + b.len() as u64).step_by(0x1000));
        }
        let laid = Laid(pages.into_iter().collect());
        for &p in &laid.0 {
            assert!((0x1_0000..0x1000_0000).contains(&p), "a piece at {p:#x}, not where a test lays one");
            // SAFETY: a new private page where nothing is mapped (`MAP_FIXED_NOREPLACE` refuses where something
            // is), the test's alone.
            let got = unsafe {
                libc::mmap(
                    p as *mut libc::c_void,
                    0x1000,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED_NOREPLACE,
                    -1,
                    0,
                )
            };
            assert_eq!(got as u64, p, "mmap at {p:#x} (vm.mmap_min_addr above it?): {}", errno());
        }
        // (the last laid first: where two meet, the first laid's bytes are the memory's, as `Fake` reads it)
        for (at, b) in m.pieces.iter().rev() {
            mem.write_all_at(b, *at).unwrap();
        }
        laid
    }
}

impl Drop for Laid {
    fn drop(&mut self) {
        for &p in &self.0 {
            // SAFETY: a page `out` mapped, nothing of it borrowed.
            unsafe { libc::munmap(p as *mut libc::c_void, 0x1000) };
        }
    }
}
