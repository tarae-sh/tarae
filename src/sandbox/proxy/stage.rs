//! A terminated tunnel's stage (P1b of `.sb/credential-masking-plan.md`; `.sb/tls-p1b-step1.md` as the design session
//! answered it, 2026-11-08): the HTTP layer between the run's leg and the host's, on the connection's own thread
//! between their two bridges (`terminate::serve`). Every request the run sends is read — its head in origin form
//! (`parse::origin`), decided on the CONNECT target (`decide`), its body followed to its end — and every answer the
//! host sends is followed to its end by what was asked (`Exchange`). The bytes go on as they came (P2 swaps there).
//! A request refused goes nowhere, nor anything the run sent behind it, and is answered once the answers owed have
//! gone, then the tunnel closes. Keep-alive with srt's idle limits (`Limits::head_time`, `keep_alive`, `stalled`).

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use super::bridge::{Ending, Out};
use super::masked::{self, Stopped, View};
use super::parse::{self, Body, Framing};
use super::swap::Scan;
use super::{Limits, MASKED_OP, OP, answer_bytes};

/// What one read takes.
const CHUNK: usize = 16 * 1024;
/// The most bytes waiting for one side before the other isn't read any more.
const PENDING_MAX: usize = 64 * 1024;
/// How long a closed tunnel waits for each side's own close — its close_notify back through its bridge — before it
/// lets go: a bridge let go at once can end before its own close_notify went.
const CLOSING: Duration = Duration::from_secs(2);

/// The rules for each request inside a terminated tunnel, decided on its CONNECT target `name:port`, never on the
/// request's own words: its `Host`, where it has one, names that host and port (no port: 443) — else 421, srt's
/// `requireHostMatch`, here always on; TRACE and TRACK never go (they echo the request back, its credentials with it:
/// D3); its method one of plain http's (`parse::METHODS`, the user's list) — a CONNECT inside the tunnel neither; no
/// upgrade (WebSocket is P4). Err: the status, and why. P2's swap and D1's later rules plug in here.
pub(super) fn decide(o: &parse::Origin, name: &str, port: u16) -> Result<(), (u16, String)> {
    if let Some((host, p)) = &o.host {
        let p = p.unwrap_or(443);
        if !matches!(host, parse::Host::Name(n) if n == name) || p != port {
            return Err((421, format!("a request's Host names {}", host.header(p, 443))));
        }
    }
    let m = o.method.as_str();
    if m == "TRACE" || m == "TRACK" {
        return Err((
            405,
            format!("a {m} request — refused on a host tarae terminates: it echoes the request back"),
        ));
    }
    if m == "CONNECT" {
        return Err((405, "a CONNECT inside a tunnel".into()));
    }
    if !parse::METHODS.contains(&m) {
        return Err((405, format!("a method other than {}", parse::METHODS.join(" "))));
    }
    if o.upgrade {
        return Err((
            501,
            "an Upgrade — WebSocket and other upgrades aren't taken on a host tarae terminates (yet)".into(),
        ));
    }
    Ok(())
}

/// How a tunnel's exchange stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Stop {
    /// A request refused before any byte of it went on: its status and why — answered once the answers owed have
    /// gone, then the tunnel closes.
    Refused(u16, String),
    /// The host's answer unreadable before any byte of it went on: a 502 to the run, then the tunnel closes.
    Failed(String),
    /// A framing that can't be followed part-way through it (some of it went on): the tunnel closes now.
    Cut(String),
    /// A masked value toward a host it doesn't go to (P2, D5): why — found in a head, refused 403 as `Refused`;
    /// part-way through a body (`cut`), the tunnel closes now. Said as an attempt (`MASKED_OP`).
    Masked { why: String, cut: bool },
}

/// Where an exchange is, for its idle limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    /// No request taken whole yet: `head_time`.
    First,
    /// Every answer whole, the next request not begun: `keep_alive`.
    Idle,
    /// Whole requests waiting for their answers, nothing part-way: no limit (the run's side is still read).
    Waiting,
    /// A head or a body part-way either side, or bytes waiting for one: `stalled`.
    UnderWay,
}

/// A request sent on, owed its answer.
#[derive(Clone, Copy, Debug)]
struct Asked {
    /// A HEAD: its answer has no body.
    head: bool,
    /// The connection's last.
    last: bool,
}

#[derive(Debug)]
enum RunSide {
    /// A head, gathered in `Exchange.head`.
    Head,
    /// A request's body; then the next head, or nothing (`last`) — its data through `scan` where masking looks at it.
    Body { body: Body, last: bool, scan: Option<Box<Scan>> },
    /// After the connection's last request: nothing more goes (what comes is dropped and counted).
    Shut,
    /// After a refusal: nothing more goes (what comes is dropped).
    Refused,
}

#[derive(Debug)]
enum HostSide {
    /// An answer's head, gathered in `Exchange.answer`.
    Head,
    /// An answer's body — `close`: until the host closes; its data through `scan` where masking scrubs it.
    Body { body: Body, close: bool, scan: Option<Box<Scan>> },
}

/// A terminated tunnel's HTTP, both ways, as a pure machine: bytes in from either side, the bytes that go on out —
/// as they came but for masking's named edits (`View`: the swaps, the scrubs, `Accept-Encoding`, `TE`) — or why it
/// stops.
#[derive(Debug)]
pub(super) struct Exchange {
    name: String,
    port: u16,
    /// What masking swaps, refuses and scrubs here — None: nothing masked in the session.
    view: Option<View>,
    /// What the report says of masking here, each once (`said`).
    notes: Vec<String>,
    told: usize,
    run: RunSide,
    head: Vec<u8>,
    host: HostSide,
    answer: Vec<u8>,
    owed: VecDeque<Asked>,
    /// The answer under way ends the connection: its request was the last, or it says so.
    closes: bool,
    /// The last answer is whole: nothing more comes either way.
    over: bool,
    /// A request was taken whole: the first head's limit is past.
    taken: bool,
    /// The run's bytes after its last request: dropped.
    pub(super) dropped: usize,
}

impl Exchange {
    pub(super) fn new(name: &str, port: u16, view: Option<View>) -> Exchange {
        Exchange {
            name: name.into(),
            port,
            view,
            notes: Vec::new(),
            told: 0,
            run: RunSide::Head,
            head: Vec::new(),
            host: HostSide::Head,
            answer: Vec::new(),
            owed: VecDeque::new(),
            closes: false,
            over: false,
            taken: false,
            dropped: 0,
        }
    }

    /// The run's next bytes: what goes on to the host added to `out` — a head only once it's whole and decided, a
    /// body as it comes, counted to its end. A refusal: nothing of that request nor after it went.
    pub(super) fn run_sent(&mut self, mut b: &[u8], out: &mut Vec<u8>) -> Result<(), Stop> {
        while !b.is_empty() {
            match &mut self.run {
                RunSide::Shut => {
                    self.dropped += b.len();
                    return Ok(());
                }
                RunSide::Refused => return Ok(()),
                RunSide::Body { body, last, scan } => {
                    let n = match scan {
                        Some(scan) => match masked::through(body, scan, b, out) {
                            Ok(n) => n,
                            Err(Stopped::Framing(why)) => {
                                return Err(Stop::Cut(format!("a request's body: {why}")));
                            }
                            Err(Stopped::Stray(name)) => {
                                let why = self.view.as_ref().map(|v| v.refused(&name)).unwrap_or_default();
                                self.run = RunSide::Refused;
                                return Err(Stop::Masked { why, cut: true });
                            }
                        },
                        None => {
                            let n =
                                body.take(b).map_err(|why| Stop::Cut(format!("a request's body: {why}")))?;
                            out.extend_from_slice(&b[..n]);
                            n
                        }
                    };
                    b = &b[n..];
                    if body.done() {
                        let left = scan.as_mut().map(|s| s.left()).unwrap_or_default();
                        self.run = if *last { RunSide::Shut } else { RunSide::Head };
                        self.left(true, left);
                    }
                }
                RunSide::Head => {
                    // (no more of it kept than a head can be: past that, `origin` has answered)
                    let before = self.head.len();
                    let k = b.len().min(parse::HEAD_MAX.saturating_sub(before).max(1));
                    self.head.extend_from_slice(&b[..k]);
                    let o = match parse::origin(&self.head) {
                        Ok(None) => {
                            b = &b[k..];
                            continue;
                        }
                        Ok(Some(o)) => o,
                        Err(r) => {
                            self.run = RunSide::Refused;
                            return Err(Stop::Refused(r.status(), r.inside()));
                        }
                    };
                    if let Err((status, why)) = decide(&o, &self.name, self.port) {
                        self.run = RunSide::Refused;
                        return Err(Stop::Refused(status, why));
                    }
                    // (masking's edits: a sentinel that doesn't go here refused, a secret's own swapped in)
                    let mut scan = None;
                    match self.view.as_ref().map(|v| v.request_head(&self.head[..o.len])) {
                        None => out.extend_from_slice(&self.head[..o.len]),
                        Some(Err(why)) => {
                            self.run = RunSide::Refused;
                            return Err(Stop::Masked { why, cut: false });
                        }
                        Some(Ok(h)) => {
                            out.extend_from_slice(&h.bytes);
                            h.left.into_iter().for_each(|said| self.note(said));
                            let view = self.view.as_ref().expect("a view");
                            match h.encoded {
                                // (an encoded body isn't searched: only toward a secret's host is that said)
                                Some(c) if view.edits() && o.body != Framing::Empty => self.note(format!(
                                    "a request body with Content-Encoding: {c} isn't searched — a masked value in \
                                     it went as its fake"
                                )),
                                Some(_) => {}
                                None => scan = view.request_scan().map(Box::new),
                            }
                        }
                    }
                    b = &b[o.len.saturating_sub(before)..];
                    self.head.clear();
                    self.owed.push_back(Asked { head: o.method == "HEAD", last: o.last });
                    self.taken = true;
                    let body = Body::new(o.body);
                    self.run = match (body.done(), o.last) {
                        (false, last) => RunSide::Body { body, last, scan },
                        (true, true) => RunSide::Shut,
                        (true, false) => RunSide::Head,
                    };
                }
            }
        }
        Ok(())
    }

    /// The host's next bytes: what goes on to the run added to `out` — an answer's head only once it's whole, its
    /// body as it comes, counted to its end by what was asked: a HEAD's answer has no body, nor a 204's or a 304's
    /// (`parse::response`), a 1xx owes the next answer, one with neither length nor chunks ends with the host's close.
    pub(super) fn host_sent(&mut self, mut b: &[u8], out: &mut Vec<u8>) -> Result<(), Stop> {
        while !b.is_empty() && !self.over {
            match &mut self.host {
                HostSide::Body { body, scan, .. } => {
                    let cut = |why| Stop::Cut(format!("its answer's body: {why}"));
                    let n = match scan {
                        Some(scan) => match masked::through(body, scan, b, out) {
                            Ok(n) => n,
                            Err(Stopped::Framing(why)) => return Err(cut(why)),
                            // (an answer's scan has no stray: every pattern is swapped)
                            Err(Stopped::Stray(_)) => return Err(cut("a value it can't scrub")),
                        },
                        None => {
                            let n = body.take(b).map_err(cut)?;
                            out.extend_from_slice(&b[..n]);
                            n
                        }
                    };
                    b = &b[n..];
                    if body.done() {
                        let left = scan.as_mut().map(|s| s.left()).unwrap_or_default();
                        self.answered();
                        self.left(false, left);
                    }
                }
                HostSide::Head => {
                    let Some(&asked) = self.owed.front() else {
                        return Err(Stop::Cut("it sent an answer nothing asked for".into()));
                    };
                    let before = self.answer.len();
                    let k = b.len().min(parse::RESPONSE_HEAD_MAX.saturating_sub(before).max(1));
                    self.answer.extend_from_slice(&b[..k]);
                    let r = match parse::response(&self.answer) {
                        Ok(None) => {
                            b = &b[k..];
                            continue;
                        }
                        Ok(Some(r)) => r,
                        Err(e) => return Err(Stop::Failed(format!("its answer: {}", e.inside()))),
                    };
                    if r.status == 101 {
                        return Err(Stop::Cut(
                            "it switched protocols (101), which the run never asked".into(),
                        ));
                    }
                    // (masking's scrub: every real value back to its sentinel, the head's and the body's)
                    let mut scan = None;
                    match self.view.as_ref().map(|v| v.answer_head(&self.answer[..r.len])) {
                        None => out.extend_from_slice(&self.answer[..r.len]),
                        Some(h) => {
                            out.extend_from_slice(&h.bytes);
                            let view = self.view.as_ref().expect("a view");
                            match h.encoded {
                                Some(c) if r.body != Framing::Empty && !asked.head => self.note(format!(
                                    "an answer with Content-Encoding: {c} passed unscrubbed — a real value in \
                                     its body would reach the run"
                                )),
                                Some(_) => {}
                                None => scan = view.answer_scan().map(Box::new),
                            }
                        }
                    }
                    b = &b[r.len.saturating_sub(before)..];
                    let old = self.answer.starts_with(b"HTTP/1.0");
                    self.answer.clear();
                    // (an interim answer: the request still owes its final one)
                    if r.status < 200 {
                        continue;
                    }
                    let says = |t: &str| {
                        r.fields
                            .iter()
                            .filter(|(k, _)| k.eq_ignore_ascii_case("connection"))
                            .any(|(_, v)| v.split(',').any(|x| x.trim().eq_ignore_ascii_case(t)))
                    };
                    self.closes = asked.last || says("close") || (old && !says("keep-alive"));
                    let framing = if asked.head { Framing::Empty } else { r.body };
                    let body = Body::new(framing);
                    match body.done() {
                        true => self.answered(),
                        false => self.host = HostSide::Body { body, close: framing == Framing::Close, scan },
                    }
                }
            }
        }
        Ok(())
    }

    /// An answer whole: its request no longer owed — the exchange over where it closes the connection.
    fn answered(&mut self) {
        self.owed.pop_front();
        self.host = HostSide::Head;
        self.over |= self.closes;
    }

    /// The host's side ended — `clean`: its TLS with close_notify: the exchange is over — Ok where that's an end
    /// (between answers: a request still owed sees the close, and its client may try again; an answer that ends with
    /// the close, only where it ended clean: what its scrub held goes to `out`), else why not.
    pub(super) fn host_ended(&mut self, clean: bool, out: &mut Vec<u8>) -> Result<(), Stop> {
        let was = std::mem::replace(&mut self.over, true);
        match &mut self.host {
            _ if was => Ok(()),
            HostSide::Head if self.answer.is_empty() => Ok(()),
            HostSide::Head => Err(Stop::Failed("it closed part-way through an answer's head".into())),
            HostSide::Body { close: true, scan, .. } if clean => {
                let left = match scan {
                    Some(s) => {
                        let _ = s.end(out);
                        s.left()
                    }
                    None => Vec::new(),
                };
                self.owed.pop_front();
                self.host = HostSide::Head;
                self.left(false, left);
                Ok(())
            }
            HostSide::Body { close: true, .. } => Err(Stop::Cut(
                "its TLS ended without close_notify in an answer that runs to its close (it may be cut short)".into(),
            )),
            HostSide::Body { .. } => Err(Stop::Cut("it closed part-way through an answer's body".into())),
        }
    }

    /// A note of masking's, said once a tunnel.
    fn note(&mut self, said: String) {
        if !self.notes.contains(&said) {
            self.notes.push(said);
        }
    }

    /// What a body's scan left as it was — `out`: a request's — noted.
    fn left(&mut self, out: bool, left: Vec<(super::swap::Does, String)>) {
        let said: Vec<String> = match &self.view {
            Some(v) => left.iter().map(|(does, name)| v.left_said(out, *does, name)).collect(),
            None => Vec::new(),
        };
        said.into_iter().for_each(|s| self.note(s));
    }

    /// What masking noted since the last call, for the report.
    pub(super) fn said(&mut self) -> Vec<String> {
        let new = self.notes[self.told..].to_vec();
        self.told = self.notes.len();
        new
    }

    /// Whether a request body's start of what may be a masked value is held.
    pub(super) fn run_holds(&self) -> bool {
        matches!(&self.run, RunSide::Body { scan: Some(s), .. } if s.holding())
    }

    /// A pause: what the request body's scan holds goes to `out` as it is (the fake goes, said where a value went so).
    /// Err: a stray completed in it, as `run_sent`'s.
    pub(super) fn release_run(&mut self, out: &mut Vec<u8>) -> Result<(), Stop> {
        let RunSide::Body { scan: Some(s), .. } = &mut self.run else { return Ok(()) };
        if let Err(name) = s.release(out) {
            let why = self.view.as_ref().map(|v| v.refused(&name)).unwrap_or_default();
            self.run = RunSide::Refused;
            return Err(Stop::Masked { why, cut: true });
        }
        let left = s.left();
        self.left(true, left);
        Ok(())
    }

    /// Requests sent on, owed their answers.
    pub(super) fn owed(&self) -> usize {
        self.owed.len()
    }

    /// The last answer is whole (its request was the connection's last, it said so, or it ended with the close).
    pub(super) fn over(&self) -> bool {
        self.over
    }

    /// Where it is, for the idle limits (bytes waiting in the loop make any phase but `First` `UnderWay`).
    pub(super) fn phase(&self) -> Phase {
        let run_rests = match self.run {
            RunSide::Head => self.head.is_empty(),
            RunSide::Shut | RunSide::Refused => true,
            RunSide::Body { .. } => false,
        };
        let host_rests = matches!(self.host, HostSide::Head) && self.answer.is_empty();
        match (self.taken, run_rests && host_rests, self.owed.is_empty()) {
            (false, ..) => Phase::First,
            (true, false, _) => Phase::UnderWay,
            (true, true, true) => Phase::Idle,
            (true, true, false) => Phase::Waiting,
        }
    }
}

/// When the idle limit for `phase` closes the tunnel: a first head within `head_time` of the run's handshake
/// (`started`), the next within `keep_alive` of the last byte moved, anything part-way within `stalled` of it — never
/// while whole requests wait for their answers.
fn due(phase: Phase, started: Instant, moved: Instant, l: &Limits) -> Option<Instant> {
    match phase {
        Phase::First => Some(started + l.head_time),
        Phase::Idle => Some(moved + l.keep_alive),
        Phase::Waiting => None,
        Phase::UnderWay => Some(moved + l.stalled),
    }
}

/// Whether the tunnel closes now: its limit past — but never while the run has bytes unread where a head is waited
/// for (`First`, `Idle`): a head that came in the same round as the limit wins (the design session's tie rule).
fn expired(phase: Phase, due: Option<Instant>, now: Instant, unread: impl FnOnce() -> bool) -> bool {
    let past = due.is_some_and(|d| now >= d);
    past && !(matches!(phase, Phase::First | Phase::Idle) && unread())
}

/// The stage's loop (`poll` on the two bridges' ends, `sides` = [the run's, the host's]), until the exchange is over,
/// either side ends, a limit passes, or a request is refused: `Exchange` reads both ways at once (full duplex — curl
/// and git wait for a `100 Continue` before a body), a refusal is answered after the answers owed (`Refused`'s
/// status, why in its body), an answer that can't be read before any of it went is a 502 — inside the run's TLS,
/// both. Then `tell` is given what the report says (after `name:port: `; a keep-alive's end or the run's own close
/// isn't said), and the stage lets go: at a clean end — the exchange over, at rest, a refusal or a 502 answered whole
/// — each side's writing shut first (its bridge's close_notify, `close`); else (the host's TLS ended without
/// close_notify, `ending`; a framing that can't be followed either side; anything that fails part-way) with none
/// (`Out::abandon`): the run's TLS reads its end as the truncation it is.
#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    sides: [Out; 2],
    ending: &Ending,
    name: &str,
    port: u16,
    view: Option<View>,
    limits: &Limits,
    tell: impl Fn(&str, &str),
) {
    if sides.iter().any(|s| s.set_nonblocking(true).is_err()) {
        return;
    }
    let mut x = Exchange::new(name, port, view);
    // (the run's last bytes: a request body's held start goes `hold_pause` after them)
    let mut run_moved = crate::clock::now();
    // pending[0]: what goes to the run; pending[1]: to the host. eof[i]: side i read to its end; hup[i]: it hung up —
    // left out of `poll` while it's neither read nor written to (as the relay's)
    let mut pending = [Vec::new(), Vec::new()];
    let (mut eof, mut hup, mut host_shut) = ([false; 2], [false; 2], false);
    let mut buf = vec![0u8; CHUNK];
    let started = crate::clock::now();
    let mut moved = started;
    // A refusal, held until the answers owed have gone (its op: a masked value's is an attempt); then how the tunnel
    // closes
    let mut held: Option<(u16, String, &'static str)> = None;
    let mut closing: Option<Closing> = None;
    // (what a close the loop didn't plan says: the one planned's words, never clean)
    let failed = |c: &Option<Closing>| Closing {
        why: c.as_ref().and_then(|c| c.why.clone()),
        op: c.as_ref().map_or(OP, |c| c.op),
        clean: false,
    };
    let c = 'stage: loop {
        // (a clean close once what goes to the run went — not past `stalled`; an unclean one at once)
        if let Some(c) = &closing
            && (pending[0].is_empty() || !c.clean || crate::clock::now() >= moved + limits.stalled)
        {
            break Closing { clean: c.clean && pending[0].is_empty(), ..c.clone() };
        }
        let phase = match x.phase() {
            Phase::First => Phase::First,
            _ if pending.iter().any(|p| !p.is_empty()) => Phase::UnderWay,
            p => p,
        };
        let by = match closing {
            Some(_) => Some(moved + limits.stalled),
            None if x.run_holds() => due(phase, started, moved, limits)
                .map(|d| d.min(run_moved + limits.hold_pause))
                .or(Some(run_moved + limits.hold_pause)),
            None => due(phase, started, moved, limits),
        };
        let open = closing.is_none();
        let read = [
            open && !eof[0] && pending[1].len() < PENDING_MAX,
            open && !eof[1] && pending[0].len() < PENDING_MAX,
        ];
        let write = [!pending[0].is_empty(), open && !pending[1].is_empty()];
        let mut fds: [libc::pollfd; 2] = std::array::from_fn(|i| {
            let events = if read[i] { libc::POLLIN } else { 0 } | if write[i] { libc::POLLOUT } else { 0 };
            let fd = if hup[i] && events == 0 { -1 } else { sides[i].as_raw_fd() };
            libc::pollfd { fd, events, revents: 0 }
        });
        if fds.iter().all(|f| f.events == 0 || f.fd < 0) && by.is_none() {
            break failed(&closing);
        }
        let timeout = by.map_or(-1, |d| {
            let left = d.saturating_duration_since(crate::clock::now());
            // (rounded up: a poll that wakes a hair early only polls again)
            left.as_millis().saturating_add(1).min(i32::MAX as u128) as i32
        });
        // SAFETY: two pollfds of ours, their count.
        let r = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout) };
        if r < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            break failed(&closing);
        }
        if fds.iter().any(|f| f.revents & (libc::POLLERR | libc::POLLNVAL) != 0) {
            break failed(&closing);
        }
        for i in 0..2 {
            hup[i] |= fds[i].revents & libc::POLLHUP != 0;
            if !read[i] || fds[i].revents & (libc::POLLIN | libc::POLLHUP) == 0 {
                continue;
            }
            // (an end: clean where the host's TLS ended with close_notify — the run's is its close either way; a read
            // that failed is no clean end of either)
            let (got, clean) = match (&sides[i]).read(&mut buf) {
                Ok(0) => (0, i == 0 || ending.clean()),
                Ok(n) => (n, true),
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {
                    continue;
                }
                Err(_) if i == 0 => break 'stage failed(&closing),
                Err(_) => (0, false),
            };
            if got == 0 {
                eof[i] = true;
            } else {
                moved = crate::clock::now();
                if i == 0 {
                    run_moved = moved;
                }
            }
            let bytes = &buf[..got];
            let stop = match (i, got) {
                (0, 0) => continue,
                (0, _) => x.run_sent(bytes, &mut pending[1]),
                (_, 0) => x.host_ended(clean, &mut pending[0]),
                _ => x.host_sent(bytes, &mut pending[0]),
            };
            match stop {
                Ok(()) if i == 1 && got == 0 => {
                    let (why, op) = match held.take() {
                        Some((status, why, op)) => {
                            (Some(format!("{why} — refused {status}, unanswered: the host closed first")), op)
                        }
                        None => (None, OP),
                    };
                    closing = Some(Closing { why, op, clean });
                }
                Ok(()) => {}
                Err(Stop::Refused(status, why)) => held = Some((status, why, OP)),
                Err(Stop::Masked { why, cut: false }) => held = Some((403, why, MASKED_OP)),
                Err(Stop::Masked { why, cut: true }) => {
                    let why =
                        Some(format!("{why} — found part-way through a request's body: the tunnel closed"));
                    closing = Some(Closing { why, op: MASKED_OP, clean: false });
                }
                Err(Stop::Failed(why)) => {
                    pending[0].extend_from_slice(&answer_bytes(502, &format!("{name}:{port}: {why}"), ""));
                    closing = Some(Closing::clean(format!("the host's side: {why} — answered 502")));
                }
                Err(Stop::Cut(why)) => {
                    let side = if i == 0 { "the run's side" } else { "the host's side" };
                    let why = Some(format!("{side}: {why} — the tunnel closed"));
                    closing = Some(Closing { why, op: OP, clean: false });
                }
            }
        }
        // (a request body's held start, nothing more of it within the pause: it goes as it is)
        if closing.is_none()
            && x.run_holds()
            && crate::clock::now() >= run_moved + limits.hold_pause
            && let Err(Stop::Masked { why, .. }) = x.release_run(&mut pending[1])
        {
            let why = Some(format!("{why} — found part-way through a request's body: the tunnel closed"));
            closing = Some(Closing { why, op: MASKED_OP, clean: false });
        }
        for said in x.said() {
            tell(MASKED_OP, &said);
        }
        for i in 0..2 {
            if !write[i] || pending[i].is_empty() {
                continue;
            }
            match (&sides[i]).write(&pending[i]) {
                Ok(n) => {
                    pending[i].drain(..n);
                    moved = crate::clock::now();
                }
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {}
                // (its reader gone: nothing more goes either way)
                Err(_) => break 'stage failed(&closing),
            }
        }
        if closing.is_some() {
            continue;
        }
        // The answers owed have gone: the refusal held answered, then the close
        if x.owed() == 0
            && let Some((status, why, op)) = held.take()
        {
            pending[0].extend_from_slice(&answer_bytes(status, &format!("{name}:{port}: {why}"), ""));
            let said = match op {
                MASKED_OP => format!("{why} — refused {status}"),
                _ => format!("{why} — answered {status}"),
            };
            closing = Some(Closing { op, ..Closing::clean(said) });
            continue;
        }
        if x.over() {
            closing = Some(Closing { why: None, op: OP, clean: true });
            continue;
        }
        // The run's writing ended: the host's too, once what goes to it went — and with nothing owed, the end
        if eof[0] && pending[1].is_empty() && !host_shut {
            let _ = sides[1].shutdown(Shutdown::Write);
            host_shut = true;
        }
        if eof[0] && x.owed() == 0 && pending[1].is_empty() {
            closing = Some(Closing { why: None, op: OP, clean: true });
            continue;
        }
        let unread = || matches!(sides[0].peek(&mut [0u8; 1]), Ok(1..));
        if expired(phase, due(phase, started, moved, limits), crate::clock::now(), unread) {
            closing = Some(match phase {
                Phase::First => Closing::clean(format!(
                    "no whole request head within {} s of its TLS handshake — the tunnel closed",
                    limits.head_time.as_secs_f32()
                )),
                // (part-way: an answer that runs to its close isn't whole by it)
                Phase::UnderWay => Closing {
                    why: Some(format!(
                        "nothing moved for {} s part-way through a request or its answer — the tunnel closed",
                        limits.stalled.as_secs_f32()
                    )),
                    op: OP,
                    clean: false,
                },
                // (the keep-alive's end: a tunnel at rest, closed with close_notify, not said)
                Phase::Idle | Phase::Waiting => Closing { why: None, op: OP, clean: true },
            });
        }
    };
    // (said first: `close` may wait for a side's own close)
    for said in x.said() {
        tell(MASKED_OP, &said);
    }
    match (c.why, x.dropped) {
        (None, 0) => {}
        (Some(w), 0) => tell(c.op, &w),
        (None, n) => tell(OP, &format!("{n} bytes sent after its last request were dropped")),
        (Some(w), n) => tell(c.op, &format!("{w} ({n} bytes sent after its last request dropped)")),
    }
    match c.clean {
        true => close(&sides),
        false => sides.into_iter().for_each(Out::abandon),
    }
}

/// How a tunnel closes: what the report says (None: nothing) and with which op, and whether cleanly — each side's
/// close_notify — or not: the run's TLS ends with none, a truncation.
#[derive(Clone, Debug)]
struct Closing {
    why: Option<String>,
    op: &'static str,
    clean: bool,
}

impl Closing {
    fn clean(why: String) -> Closing {
        Closing { why: Some(why), op: OP, clean: true }
    }
}

/// Both sides closed: each one's writing shut (its bridge sends close_notify), then what each still sends read and
/// dropped until its bridge's thread is over (`Out::life_line`) — its close_notify gone, the other end's own close come
/// — `CLOSING` at most. (Not its end read alone: a run that half-closed sent that before, and its bridge let go before
/// it read this shutdown would see only its life line gone, and end with no close_notify.)
fn close(sides: &[Out; 2]) {
    for s in sides {
        let _ = s.shutdown(Shutdown::Write);
    }
    let by = crate::clock::now() + CLOSING;
    // done[i]: side i's bridge is over (a connection: read to its end); eof[i]: read to its end
    let (mut done, mut eof, mut b) = ([false; 2], [false; 2], [0u8; 4096]);
    while done.contains(&false) {
        let left = by.saturating_duration_since(crate::clock::now());
        if left.is_zero() {
            return;
        }
        // ([side 0, its line, side 1, its line]: a line polled for its end alone)
        let mut fds: [libc::pollfd; 4] = std::array::from_fn(|k| {
            let (i, line) = (k / 2, k % 2 == 1);
            let fd = match line {
                _ if done[i] => -1,
                true => sides[i].life_line().unwrap_or(-1),
                false if eof[i] => -1,
                false => sides[i].as_raw_fd(),
            };
            libc::pollfd { fd, events: if line { 0 } else { libc::POLLIN }, revents: 0 }
        });
        let ms = left.as_millis().saturating_add(1).min(i32::MAX as u128) as i32;
        // SAFETY: four pollfds of ours, their count.
        match unsafe { libc::poll(fds.as_mut_ptr(), 4, ms) } {
            0 => return,
            r if r < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted => continue,
            r if r < 0 => return,
            _ => {}
        }
        for i in 0..2 {
            if done[i] {
                continue;
            }
            if fds[2 * i + 1].revents != 0 {
                done[i] = true;
                continue;
            }
            if fds[2 * i].revents == 0 {
                continue;
            }
            match (&sides[i]).read(&mut b) {
                Ok(n) if n > 0 => continue,
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {
                    continue;
                }
                _ => eof[i] = true,
            }
            done[i] = sides[i].life_line().is_none();
        }
    }
}

#[cfg(test)]
#[path = "stage/tests.rs"]
mod tests;
