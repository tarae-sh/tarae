//! Netlink's wire, read and written in one place for the sandbox: a message's header (`struct nlmsghdr`), the walk over
//! a buffer of messages — a run's batch (`wire::netlink_reads`) and the kernel's answers (`owners::diag`) —, an
//! `NLMSG_ERROR`'s code, a message's attributes (`struct rtattr`), and the one request tarae writes whole by hand, a
//! device up (`keeper`'s `lo`). Pure over bytes, no allocation and no panic — a read past the end is `None`, never an
//! index: the keeper's birth child uses it between `clone3` and exec. It walks and reads; what a message may be is its
//! caller's to judge.
//!
//! The numbers are Linux's (`uapi/linux/netlink.h`, `rtnetlink.h`, `if.h`), written out so it builds and its walk is
//! tested on every system, as `wire`'s are; `the_numbers_are_linuxs` checks them against libc's there.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

/// `struct nlmsghdr`'s size: a message's payload follows it.
pub const HDR_LEN: usize = 16;
pub const NLMSG_ERROR: u16 = 2;
pub const NLMSG_DONE: u16 = 3;
pub const RTM_NEWLINK: u16 = 16;
pub const NLM_F_REQUEST: u16 = 1;
pub const NLM_F_ACK: u16 = 4;
pub const NLM_F_DUMP: u16 = 0x300;
pub const IFF_UP: u32 = 1;
const AF_UNSPEC: u8 = 0;
/// `struct rtattr`'s size: an attribute's value follows it.
const ATTR_LEN: usize = 4;

/// Where the next message or attribute starts after one of `n` bytes (`NLMSG_ALIGN`, `RTA_ALIGN`).
pub const fn align(n: usize) -> usize {
    n.saturating_add(3) & !3
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..)?.first_chunk().map(|w| u16::from_ne_bytes(*w))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..)?.first_chunk().map(|w| u32::from_ne_bytes(*w))
}

fn put(to: &mut [u8], at: usize, b: &[u8]) -> Option<()> {
    to.get_mut(at..at.checked_add(b.len())?)?.copy_from_slice(b);
    Some(())
}

/// A message's header: its length (the header's own counted), type, flags, sequence number and port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub len: u32,
    pub kind: u16,
    pub flags: u16,
    pub seq: u32,
    pub pid: u32,
}

impl Header {
    /// The header at `b`'s start; None where `b` is shorter.
    pub fn read(b: &[u8]) -> Option<Header> {
        Some(Header {
            len: u32_at(b, 0)?,
            kind: u16_at(b, 4)?,
            flags: u16_at(b, 6)?,
            seq: u32_at(b, 8)?,
            pid: u32_at(b, 12)?,
        })
    }

    pub fn bytes(&self) -> [u8; HDR_LEN] {
        let mut b = [0; HDR_LEN];
        let _ = self.write(&mut b);
        b
    }

    /// Written at `to`'s start; None where `to` is shorter.
    pub fn write(&self, to: &mut [u8]) -> Option<()> {
        put(to, 0, &self.len.to_ne_bytes())?;
        put(to, 4, &self.kind.to_ne_bytes())?;
        put(to, 6, &self.flags.to_ne_bytes())?;
        put(to, 8, &self.seq.to_ne_bytes())?;
        put(to, 12, &self.pid.to_ne_bytes())
    }
}

/// What's wrong with the record a walk stopped at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    /// Fewer bytes left than a header's.
    Header,
    /// A length under its own header's.
    Under,
    /// A length past what's left.
    Past,
}

impl std::fmt::Display for Cut {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            Cut::Header => "a header cut short",
            Cut::Under => "a length under its header's",
            Cut::Past => "a length past what's left",
        })
    }
}

/// The record at `rest`'s start, `len` long by its header of `head` bytes (None: `rest` holds no whole header), and
/// what follows it at the 4-byte alignment — the last one's padding may be missing.
fn split(rest: &[u8], head: usize, len: Option<usize>) -> Result<(&[u8], &[u8]), Cut> {
    let len = len.filter(|_| rest.len() >= head).ok_or(Cut::Header)?;
    if len < head {
        return Err(Cut::Under);
    }
    let record = rest.get(..len).ok_or(Cut::Past)?;
    Ok((record, rest.get(align(len)..).unwrap_or(&[])))
}

/// The messages in `buf`, each its header and payload, in order; one cut short or too long ends the walk with what's
/// wrong — nothing after it.
pub fn messages(buf: &[u8]) -> Messages<'_> {
    Messages { rest: Some(buf) }
}

pub struct Messages<'a> {
    /// None once the walk said a record was wrong.
    rest: Option<&'a [u8]>,
}

impl<'a> Iterator for Messages<'a> {
    type Item = Result<(Header, &'a [u8]), Cut>;

    fn next(&mut self) -> Option<Self::Item> {
        let rest = self.rest.take().filter(|r| !r.is_empty())?;
        let header = Header::read(rest);
        let (record, next) = match split(rest, HDR_LEN, header.map(|h| h.len as usize)) {
            Ok(split) => split,
            Err(cut) => return Some(Err(cut)),
        };
        self.rest = Some(next);
        Some(header.map(|h| (h, record.get(HDR_LEN..).unwrap_or(&[]))).ok_or(Cut::Header))
    }
}

/// An `NLMSG_ERROR`'s payload (its code, then the request it answers) read: 0 an acknowledgement, else the errno the
/// kernel negated. A payload too short for its code, or a code that's no negated errno, `EPROTO`. A dump's
/// `NLMSG_DONE` carries its code the same way (`netlink_dump`'s `dump_done_errno`): read by this too.
pub fn ack(payload: &[u8]) -> Result<(), i32> {
    match u32_at(payload, 0).map(|c| c as i32) {
        Some(0) => Ok(()),
        Some(code) if code < 0 => Err(code.checked_neg().unwrap_or(libc::EPROTO)),
        _ => Err(libc::EPROTO),
    }
}

/// The attributes in a message's payload past its fixed part, each its type and value, in order; as `messages`.
pub fn attrs(buf: &[u8]) -> Attrs<'_> {
    Attrs { rest: Some(buf) }
}

pub struct Attrs<'a> {
    rest: Option<&'a [u8]>,
}

impl<'a> Iterator for Attrs<'a> {
    type Item = Result<(u16, &'a [u8]), Cut>;

    fn next(&mut self) -> Option<Self::Item> {
        let rest = self.rest.take().filter(|r| !r.is_empty())?;
        let (record, next) = match split(rest, ATTR_LEN, u16_at(rest, 0).map(usize::from)) {
            Ok(split) => split,
            Err(cut) => return Some(Err(cut)),
        };
        self.rest = Some(next);
        let kind = u16_at(record, 2).ok_or(Cut::Header);
        Some(kind.map(|k| (k, record.get(ATTR_LEN..).unwrap_or(&[]))))
    }
}

/// The request that brings device `index` up (`RTM_NEWLINK`, acknowledged, sequence 1): its header, then `struct
/// ifinfomsg` — family (unspecified), padding, type, index, flags and their change mask, `IFF_UP` in both.
pub fn link_up(buf: &mut [u8; 32], index: i32) {
    *buf = [0; 32];
    let header = Header { len: 32, kind: RTM_NEWLINK, flags: NLM_F_REQUEST | NLM_F_ACK, seq: 1, pid: 0 };
    let _ = (|| {
        header.write(buf)?;
        put(buf, HDR_LEN, &[AF_UNSPEC])?;
        put(buf, HDR_LEN + 4, &index.to_ne_bytes())?;
        put(buf, HDR_LEN + 8, &IFF_UP.to_ne_bytes())?;
        put(buf, HDR_LEN + 12, &IFF_UP.to_ne_bytes())
    })();
}

/// The walk's properties over any bytes — the in-tree loop's and the `netlink` fuzz target's (`fuzz/parsers`).
#[cfg(any(test, fuzzing))]
pub mod check {
    use super::*;

    /// `messages` over `b`: each message it yields lies whole in `b` where the one before it ended (aligned), its
    /// header the bytes there and its payload its length less the header; it covers `b` to its end, or stops with
    /// what's wrong at the record after the last whole one — and says nothing after. The messages and how it ended.
    pub fn walk(b: &[u8]) -> (usize, Option<Cut>) {
        let (mut at, mut n, mut cut) = (0, 0, None);
        let mut walk = messages(b);
        for m in walk.by_ref() {
            match m {
                Ok((h, payload)) => {
                    let len = h.len as usize;
                    assert_eq!(Header::read(&b[at..]), Some(h), "{b:?}");
                    assert_eq!(payload, &b[at + HDR_LEN..at + len], "{b:?}");
                    at = (at + align(len)).min(b.len());
                    n += 1;
                }
                Err(c) => {
                    cut = Some(c);
                    break;
                }
            }
        }
        assert!(walk.next().is_none(), "a walk goes on after it said {cut:?}: {b:?}");
        let left = &b[at..];
        let len = || u32::from_ne_bytes(left[..4].try_into().unwrap()) as usize;
        match cut {
            None => assert!(left.is_empty(), "{b:?}"),
            Some(Cut::Header) => assert!(!left.is_empty() && left.len() < HDR_LEN, "{b:?}"),
            Some(Cut::Under) => assert!(left.len() >= HDR_LEN && len() < HDR_LEN, "{b:?}"),
            Some(Cut::Past) => assert!(left.len() >= HDR_LEN && len() > left.len(), "{b:?}"),
        }
        (n, cut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mutate::{self, Rng};

    /// A message of `kind` with `payload`, its padding where `padded`.
    fn message(kind: u16, seq: u32, payload: &[u8], padded: bool) -> Vec<u8> {
        let len = (HDR_LEN + payload.len()) as u32;
        let mut m = Header { len, kind, flags: NLM_F_REQUEST, seq, pid: 7 }.bytes().to_vec();
        m.extend(payload);
        if padded {
            m.resize(align(m.len()), 0);
        }
        m
    }

    fn walked(b: &[u8]) -> Result<Vec<(Header, Vec<u8>)>, Cut> {
        messages(b).map(|m| m.map(|(h, p)| (h, p.to_vec()))).collect()
    }

    /// Messages written read back as written — payloads of 0 to 24 bytes, so every padding, and the last one's (21)
    /// missing too —, and every way a record is wrong ends the walk with its words, nothing read after it: cut inside its
    /// header, a length under 16, a length past the buffer. An `NLMSG_ERROR` of 16 bytes reads, its code `EPROTO`.
    #[test]
    fn the_walk_reads_what_was_written_and_says_what_is_cut() {
        let payload = |n: usize| (0..n as u8).collect::<Vec<u8>>();
        for last_padded in [true, false] {
            let (mut b, mut want) = (Vec::new(), Vec::new());
            for (i, n) in (0..25).chain([21]).enumerate() {
                let m = message(16 + i as u16, i as u32, &payload(n), i < 25 || last_padded);
                want.push((Header::read(&m).unwrap(), payload(n)));
                b.extend(m);
            }
            assert_eq!(walked(&b), Ok(want.clone()), "the last one padded: {last_padded}");
            assert_eq!(check::walk(&b), (26, None));
        }
        let good = message(18, 1, &[1, 2, 3], true);
        for cut in 1..HDR_LEN {
            let mut b = good.clone();
            b.extend(&good[..cut]);
            assert_eq!(check::walk(&b), (1, Some(Cut::Header)), "cut at {cut}");
        }
        for len in 0..HDR_LEN as u32 {
            let mut b = good.clone();
            b[..4].copy_from_slice(&len.to_ne_bytes());
            b.extend(&good);
            assert_eq!(check::walk(&b), (0, Some(Cut::Under)), "length {len}");
        }
        let mut b = good.clone();
        b[..4].copy_from_slice(&(good.len() as u32 + 1).to_ne_bytes());
        assert_eq!(check::walk(&b), (0, Some(Cut::Past)));
        assert_eq!(
            [Cut::Header, Cut::Under, Cut::Past].map(|c| c.to_string()),
            ["a header cut short", "a length under its header's", "a length past what's left"]
        );
        assert_eq!(walked(&[]), Ok(vec![]));
        let error = message(NLMSG_ERROR, 1, &[], false);
        let read = walked(&error).unwrap();
        assert_eq!((read[0].0.kind, read[0].1.len()), (NLMSG_ERROR, 0));
        assert_eq!(ack(&read[0].1), Err(libc::EPROTO));
    }

    /// Any bytes walk without a panic, holding the walk's properties (`check::walk`): batches of one to four
    /// messages, mutated 100,000 times (a length past the end, under its header, a header cut short).
    #[test]
    fn any_bytes_walk_without_a_panic() {
        let mut rng = Rng(0x5eed_0add_7e55_0b1d);
        let mut ends = [0usize; 4];
        let words: [&[u8]; 3] = [&[0, 0, 0, 0], &[0xff, 0xff, 0, 0], &[15, 0, 0, 0]];
        for _ in 0..100_000 {
            let mut b = Vec::new();
            for _ in 0..1 + rng.below(4) {
                let payload: Vec<u8> = (0..rng.below(25)).map(|_| rng.next() as u8).collect();
                b.extend(message(rng.next() as u16, 0, &payload, rng.below(4) != 0));
            }
            mutate::bytes(&mut rng, &mut b, &words);
            let end = match check::walk(&b).1 {
                None => 0,
                Some(Cut::Header) => 1,
                Some(Cut::Under) => 2,
                Some(Cut::Past) => 3,
            };
            ends[end] += 1;
        }
        assert!(ends.iter().all(|&n| n > 1000), "{ends:?}");
    }

    /// An `NLMSG_ERROR`'s code: 0 an acknowledgement, a negated errno that errno; a payload too short for it, a
    /// positive code or one with no negation, `EPROTO`.
    #[test]
    fn an_ack_reads_the_kernels_code() {
        let code = |c: i32| c.to_ne_bytes();
        assert_eq!(ack(&code(0)), Ok(()));
        assert_eq!(ack(&code(-libc::EPERM)), Err(libc::EPERM));
        assert_eq!(ack(&[code(-libc::ENODEV), code(0)].concat()), Err(libc::ENODEV));
        for bad in [&code(1)[..], &code(i32::MIN), &[0, 0, 0], &[]] {
            assert_eq!(ack(bad), Err(libc::EPROTO), "{bad:?}");
        }
    }

    /// Attributes written read back as written, each value padded, the last one not; one cut short or too long ends
    /// the walk with what's wrong.
    #[test]
    fn attributes_read_back_and_say_what_is_cut() {
        let attr = |kind: u16, v: &[u8], padded: bool| {
            let mut a = [((ATTR_LEN + v.len()) as u16).to_ne_bytes(), kind.to_ne_bytes()].concat();
            a.extend(v);
            if padded {
                a.resize(align(a.len()), 0);
            }
            a
        };
        let b = [attr(11, &[1], true), attr(2, &[], true), attr(3, &[9, 8, 7, 6, 5], false)].concat();
        let read: Vec<_> = attrs(&b).map(|a| a.map(|(k, v)| (k, v.to_vec()))).collect();
        assert_eq!(read, [Ok((11, vec![1])), Ok((2, vec![])), Ok((3, vec![9, 8, 7, 6, 5]))]);
        let cut = |b: &[u8]| attrs(b).find_map(Result::err);
        assert_eq!(cut(&[attr(11, &[1], true), vec![5, 0]].concat()), Some(Cut::Header));
        assert_eq!(cut(&[3, 0, 11, 0, 0, 0, 0, 0]), Some(Cut::Under));
        assert_eq!(cut(&[9, 0, 11, 0, 1, 0, 0, 0]), Some(Cut::Past));
    }

    /// Linux's numbers are libc's there.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_numbers_are_linuxs() {
        assert_eq!(HDR_LEN, size_of::<libc::nlmsghdr>());
        assert_eq!(ATTR_LEN, size_of::<libc::rtattr>());
        assert_eq!(
            [NLMSG_ERROR, NLMSG_DONE, RTM_NEWLINK].map(i32::from),
            [libc::NLMSG_ERROR, libc::NLMSG_DONE, i32::from(libc::RTM_NEWLINK)]
        );
        assert_eq!(
            [NLM_F_REQUEST, NLM_F_ACK, NLM_F_DUMP].map(i32::from),
            [libc::NLM_F_REQUEST, libc::NLM_F_ACK, libc::NLM_F_DUMP]
        );
        assert_eq!((IFF_UP as i32, i32::from(AF_UNSPEC)), (libc::IFF_UP, libc::AF_UNSPEC));
    }

    /// `link_up`'s request held to Linux's layout, field by field at libc's offsets: `struct nlmsghdr` (its length the
    /// whole request's, `RTM_NEWLINK`, a request asking an acknowledgement, sequence 1, port 0), then `struct
    /// ifinfomsg` (unspecified family, type 0, the index, `IFF_UP` in the flags and their change mask).
    #[cfg(target_os = "linux")]
    #[test]
    fn link_up_is_held_to_linuxs_layout() {
        use std::mem::offset_of;
        let mut b = [0xaa; 32];
        link_up(&mut b, 0x0102_0304);
        let h = |at: usize, n: usize| &b[at..at + n];
        let i = |at: usize, n: usize| &b[HDR_LEN + at..HDR_LEN + at + n];
        assert_eq!(HDR_LEN + size_of::<libc::ifinfomsg>(), b.len());
        assert_eq!(h(offset_of!(libc::nlmsghdr, nlmsg_len), 4), 32u32.to_ne_bytes());
        assert_eq!(h(offset_of!(libc::nlmsghdr, nlmsg_type), 2), libc::RTM_NEWLINK.to_ne_bytes());
        let flags = (libc::NLM_F_REQUEST | libc::NLM_F_ACK) as u16;
        assert_eq!(h(offset_of!(libc::nlmsghdr, nlmsg_flags), 2), flags.to_ne_bytes());
        assert_eq!(h(offset_of!(libc::nlmsghdr, nlmsg_seq), 4), 1u32.to_ne_bytes());
        assert_eq!(h(offset_of!(libc::nlmsghdr, nlmsg_pid), 4), 0u32.to_ne_bytes());
        assert_eq!(i(offset_of!(libc::ifinfomsg, ifi_family), 1), [libc::AF_UNSPEC as u8]);
        assert_eq!(i(1, 1), [0], "the padding");
        assert_eq!(i(offset_of!(libc::ifinfomsg, ifi_type), 2), [0, 0]);
        assert_eq!(i(offset_of!(libc::ifinfomsg, ifi_index), 4), 0x0102_0304i32.to_ne_bytes());
        let up = (libc::IFF_UP as u32).to_ne_bytes();
        assert_eq!(i(offset_of!(libc::ifinfomsg, ifi_flags), 4), up);
        assert_eq!(i(offset_of!(libc::ifinfomsg, ifi_change), 4), up);
    }
}
