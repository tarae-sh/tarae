//! A body's payload searched as it streams (masking P2, `.sb/masking-p2a-step1.md`): a run's masked values swapped
//! in toward their hosts, real values scrubbed back to their sentinels in answers, another secret's sentinel found
//! before any of it goes — each pattern replaced in place by one as long, so a body's framing (its length, its
//! chunks' size lines) never changes. Pure: bytes in, bytes out, no I/O and no crate of its own (the fuzz crate
//! takes it by `#[path]`).
//!
//! What's held back is only the longest suffix of the payload so far that is a proper prefix of some pattern — a
//! sentinel's start on the way out, a real value's on the way back — never a fixed length: values look random, so
//! almost always nothing is held, and a streamed answer's events go on as each comes. Framing bytes behind a held
//! suffix wait with it (order), untouched.

/// What a pattern found does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Does {
    /// Replaced by its `to`, as long as it.
    Swap,
    /// Stops the scan before any of it goes: another secret's sentinel.
    Stray,
    /// Left as it is, and said (`Scan::left`): a pattern whose replacement isn't as long (a value under 20 bytes).
    Note,
}

/// A pattern: what's found, what replaces it (`Swap`: as long), what it does, and the name it's said by.
#[derive(Clone, PartialEq, Eq)]
pub struct Pattern {
    pub from: Vec<u8>,
    pub to: Vec<u8>,
    pub does: Does,
    pub name: String,
}

/// The name and what it does only: a pattern is a secret's value or its sentinel.
impl std::fmt::Debug for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pattern({}, {:?}, {} bytes)", self.name, self.does, self.from.len())
    }
}

/// The first match at or after `from` of the patterns `pats` names (`at(i)`: pattern i's bytes) — the leftmost, and of
/// those starting there the longest: its start and which.
fn first(
    from: usize,
    n: usize,
    at: impl Fn(usize) -> usize,
    next: &[Option<usize>],
) -> Option<(usize, usize)> {
    (0..n)
        .filter_map(|i| next[i].filter(|&s| s >= from).map(|s| (s, i)))
        .min_by(|a, b| a.0.cmp(&b.0).then(at(b.1).cmp(&at(a.1))))
}

/// Where `needle` first is in `hay` at or after `from`.
pub fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let (Some(&c), true) = (needle.first(), hay.len() >= needle.len()) else { return None };
    let last = hay.len() - needle.len();
    let mut i = from;
    while i <= last {
        i += hay[i..=last].iter().position(|&b| b == c)?;
        if &hay[i..i + needle.len()] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `hay` with each `(from, to)` found replaced, leftmost first and the longest of those starting at one place
/// (lengths may differ: a head's text, not a body's) — None: none found.
pub fn replaced(hay: &[u8], pats: &[(&[u8], &[u8])]) -> Option<Vec<u8>> {
    let pats: Vec<&(&[u8], &[u8])> = pats.iter().filter(|(f, _)| !f.is_empty()).collect();
    let mut next: Vec<Option<usize>> = pats.iter().map(|(f, _)| find(hay, f, 0)).collect();
    let (mut out, mut at) = (Vec::new(), 0);
    while let Some((s, i)) = first(at, pats.len(), |i| pats[i].0.len(), &next) {
        out.extend_from_slice(&hay[at..s]);
        out.extend_from_slice(pats[i].1);
        at = s + pats[i].0.len();
        for (j, n) in next.iter_mut().enumerate() {
            if n.is_some_and(|n| n < at) {
                *n = find(hay, pats[j].0, at);
            }
        }
    }
    (at > 0).then(|| [&out[..], &hay[at..]].concat())
}

/// A body's payload searched as it streams, its framing passed through around it: `frame` for what isn't the
/// payload's (a chunk's size line, its CR LF), `data` for what is, `end` when the body ends. Holding (`new`'s
/// `hold`): a pattern's start is held until it's known; watching (not holding): every byte goes at once, what's
/// gone only matched (plain http, where what went before is fake anyway).
pub struct Scan {
    pats: Vec<Pattern>,
    /// The longest pattern.
    longest: usize,
    hold: bool,
    /// The payload's last bytes that may begin a pattern.
    partial: Vec<u8>,
    /// How many of `partial`'s last bytes are still held in `raw` (the rest went: a release, or watching).
    kept: usize,
    /// What's held, from `partial`'s first kept byte on — framing behind it too.
    raw: Vec<u8>,
    /// Where each kept byte of `partial` is in `raw`.
    at: Vec<usize>,
    /// The patterns found and left as they were (a `Note`, or a `Swap` part of which had gone) — once each, in order;
    /// the first `told` given already (`left`).
    left: Vec<(Does, String)>,
    told: usize,
    /// How many were swapped.
    pub swapped: usize,
}

impl std::fmt::Debug for Scan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Scan({:?}, {} held)", self.pats, self.raw.len())
    }
}

impl Scan {
    /// A scan for `pats` — a `Swap` whose `to` isn't as long its `from` is a `Note`; an empty pattern none.
    pub fn new(pats: Vec<Pattern>, hold: bool) -> Scan {
        let pats: Vec<Pattern> = pats
            .into_iter()
            .filter(|p| !p.from.is_empty())
            .map(|p| match p.does {
                Does::Swap if p.to.len() != p.from.len() => Pattern { does: Does::Note, ..p },
                _ => p,
            })
            .collect();
        let longest = pats.iter().map(|p| p.from.len()).max().unwrap_or(0);
        Scan {
            pats,
            longest,
            hold,
            partial: Vec::new(),
            kept: 0,
            raw: Vec::new(),
            at: Vec::new(),
            left: Vec::new(),
            told: 0,
            swapped: 0,
        }
    }

    /// Bytes that aren't the payload's: on at once, or behind what's held.
    pub fn frame(&mut self, b: &[u8], out: &mut Vec<u8>) {
        match self.raw.is_empty() {
            true => out.extend_from_slice(b),
            false => self.raw.extend_from_slice(b),
        }
    }

    /// The payload's next bytes: what's known goes to `out`, a pattern swapped in place as it goes. Err: a `Stray`'s
    /// name — none of it went (bytes before it in this call may not have either), and the scan is over.
    pub fn data(&mut self, b: &[u8], out: &mut Vec<u8>) -> Result<(), String> {
        self.scan(b, out, true)
    }

    /// `data`'s work — `more`: more may come, so a match a longer pattern could still grow from waits, and the
    /// hold holds; else every match is decided and nothing is held (the partial bytes are kept for matching).
    fn scan(&mut self, b: &[u8], out: &mut Vec<u8>, more: bool) -> Result<(), String> {
        if self.pats.is_empty() {
            out.extend_from_slice(b);
            return Ok(());
        }
        // buf: the partial bytes then `b`; its first `gone` went already; byte j's place in `raw` is `pos(j)`
        let held = self.partial.len();
        let gone = held - self.kept;
        let r0 = self.raw.len();
        self.raw.extend_from_slice(b);
        let mut buf = std::mem::take(&mut self.partial);
        buf.extend_from_slice(b);
        let at = std::mem::take(&mut self.at);
        let pos = |j: usize| if j < held { at[j - gone] } else { r0 + (j - held) };
        let pats = &self.pats;
        let mut next: Vec<Option<usize>> = pats.iter().map(|p| find(&buf, &p.from, 0)).collect();
        let mut last = 0;
        while let Some((s, i)) = first(last, pats.len(), |i| pats[i].from.len(), &next) {
            let p = &pats[i];
            // (a longer pattern starting here may still come: undecided — held from here, below)
            let rest = &buf[s..];
            if more && pats.iter().any(|q| q.from.len() > rest.len() && q.from.starts_with(rest)) {
                break;
            }
            match p.does {
                // (the caller stops: nothing of it went, nor anything after)
                Does::Stray => return Err(p.name.clone()),
                Does::Swap if s >= gone => {
                    for (k, &c) in p.to.iter().enumerate() {
                        self.raw[pos(s + k)] = c;
                        buf[s + k] = c;
                    }
                    self.swapped += 1;
                }
                does => {
                    if !self.left.iter().any(|(d, n)| *d == does && *n == p.name) {
                        self.left.push((does, p.name.clone()));
                    }
                }
            }
            last = s + p.from.len();
            for (j, n) in next.iter_mut().enumerate() {
                if n.is_some_and(|n| n < last) {
                    *n = find(&buf, &pats[j].from, last);
                }
            }
        }
        // The hold: the longest suffix past the last match that a pattern starts with, and is shorter than
        let from = last.max(buf.len().saturating_sub(self.longest.saturating_sub(1)));
        let h = (from..buf.len())
            .find(|&s| pats.iter().any(|p| p.from.len() > buf.len() - s && p.from.starts_with(&buf[s..])))
            .unwrap_or(buf.len());
        let start = h.max(gone);
        let hold = self.hold && more;
        let cut = match hold {
            true if start < buf.len() => pos(start),
            _ => self.raw.len(),
        };
        out.extend_from_slice(&self.raw[..cut]);
        self.raw.drain(..cut);
        if hold {
            self.at = (start..buf.len()).map(|j| pos(j) - cut).collect();
        }
        self.kept = self.at.len();
        buf.drain(..h);
        self.partial = buf;
        Ok(())
    }

    /// The body ended: what's held goes — a match a longer pattern could have grown from decided, a pattern's start
    /// that never ended no pattern. Err: a stray, as `data`'s.
    pub fn end(&mut self, out: &mut Vec<u8>) -> Result<(), String> {
        self.scan(&[], out, false)?;
        (self.partial, self.kept, self.at) = (Vec::new(), 0, Vec::new());
        Ok(())
    }

    /// What's held goes now (a pause, on the way out), what's whole in it decided — what follows still matched with
    /// it: a pattern it began is left, a stray still stops. Whether anything was held; Err: a stray, as `data`'s.
    pub fn release(&mut self, out: &mut Vec<u8>) -> Result<bool, String> {
        let any = !self.raw.is_empty();
        self.scan(&[], out, false)?;
        Ok(any)
    }

    /// Whether bytes are held.
    pub fn holding(&self) -> bool {
        !self.raw.is_empty()
    }

    /// The patterns found and left as they were since the last call, by name (each once a scan).
    pub fn left(&mut self) -> Vec<(Does, String)> {
        let new = self.left[self.told..].to_vec();
        self.told = self.left.len();
        new
    }
}

/// The property its loops and the fuzz target hold it to (`fuzz/parsers`: `--cfg fuzzing`).
#[cfg(any(test, fuzzing))]
pub mod check {
    use super::*;

    /// Patterns sharing a long start (srt's prefix), the worst case for the hold: two swapped, one a `Note` (its
    /// replacement longer), one a stray, one a real value inside another's.
    pub fn patterns() -> Vec<Pattern> {
        // (each swap's replacement as long as it: `Scan::new` makes one that isn't a `Note`)
        let p = |from: &str, to: &str, does, name: &str| Pattern {
            from: from.into(),
            to: to.into(),
            does,
            name: name.into(),
        };
        vec![
            p("fake_value_a1b2c3d4e5", "REAL-VALUE-ONE-ABCDEF", Does::Swap, "ONE"),
            p("fake_value_a1b2xyz", "REAL-VALUE-TWO-XYZ", Does::Swap, "TWO"),
            p("fake_value_aaaa", "a value much longer than it", Does::Note, "SHORT"),
            p("fake_value_zz9", "", Does::Stray, "OTHER"),
            p("fake_value_a1b2c3d4e5f6g7", "REAL-VALUE-LONGER-OF-THEM", Does::Swap, "LONG"),
        ]
    }

    /// What a whole payload comes to, as the reference: each match leftmost-longest, a swap replaced — and where a
    /// stray stopped it, its start and name.
    pub fn whole(pats: &[Pattern], payload: &[u8]) -> (Vec<u8>, Option<(usize, String)>) {
        let mut next: Vec<Option<usize>> = pats.iter().map(|p| find(payload, &p.from, 0)).collect();
        let (mut out, mut at) = (payload.to_vec(), 0);
        while let Some((s, i)) = first(at, pats.len(), |i| pats[i].from.len(), &next) {
            let p = &pats[i];
            match p.does {
                Does::Stray => return (out, Some((s, p.name.clone()))),
                Does::Swap if p.to.len() == p.from.len() => out[s..s + p.to.len()].copy_from_slice(&p.to),
                _ => {}
            }
            at = s + p.from.len();
            for (j, n) in next.iter_mut().enumerate() {
                if n.is_some_and(|n| n < at) {
                    *n = find(payload, &pats[j].from, at);
                }
            }
        }
        (out, None)
    }

    /// `payload` streamed in pieces (`cuts`, ascending), a frame between each two (`frame`) — the bytes out are the
    /// reference's, the frames where they were; at a stray, nothing of it went and what did is the reference's start.
    /// Holding, nothing is held past the end; between pieces, at most the longest pattern less one of the payload.
    pub fn streamed(pats: &[Pattern], payload: &[u8], cuts: &[usize], frame: &[u8]) {
        let mut scan = Scan::new(pats.to_vec(), true);
        let (mut out, mut want, mut at) = (Vec::new(), Vec::new(), 0);
        let (full, reference) = whole(pats, payload);
        let mut stopped = None;
        for (k, &cut) in cuts.iter().chain([&payload.len()]).enumerate() {
            let cut = cut.clamp(at, payload.len());
            if k > 0 {
                scan.frame(frame, &mut out);
                want.extend_from_slice(frame);
            }
            if let Err(name) = scan.data(&payload[at..cut], &mut out) {
                stopped = Some(name);
                break;
            }
            want.extend_from_slice(&full[at..cut]);
            at = cut;
            let held_payload = scan.kept;
            assert!(held_payload < scan.longest.max(1), "held {held_payload} payload bytes");
        }
        if stopped.is_none()
            && let Err(name) = scan.end(&mut out)
        {
            stopped = Some(name);
        }
        match (stopped, reference) {
            (None, None) => {
                assert!(
                    out == want,
                    "streamed {:?}\nwanted   {:?}",
                    String::from_utf8_lossy(&out),
                    String::from_utf8_lossy(&want)
                );
            }
            (Some(name), Some((s, n))) => {
                assert_eq!(name, n, "the stray's name");
                // (what went is the reference's start, frames where they were, and not one byte of the stray)
                let mut upto = Vec::new();
                let mut from = 0;
                for (k, &cut) in cuts.iter().chain([&payload.len()]).enumerate() {
                    let cut = cut.clamp(from, payload.len());
                    if from > s {
                        break;
                    }
                    if k > 0 {
                        upto.extend_from_slice(frame);
                    }
                    upto.extend_from_slice(&full[from..cut.min(s)]);
                    from = cut;
                }
                assert!(
                    upto.starts_with(&out),
                    "went before the stray {:?}\nat most     {:?}",
                    String::from_utf8_lossy(&out),
                    String::from_utf8_lossy(&upto)
                );
            }
            (s, r) => panic!("streamed stopped at {s:?}, the reference at {r:?}"),
        }
    }

    /// The fuzz target's input: its first byte the largest piece (1 + its low 4 bits), then the payload — each 0xff
    /// followed by a byte planting the pattern that byte names (its index mod the patterns' count), whole.
    pub fn scan_input(b: &[u8]) {
        let Some((&size, rest)) = b.split_first() else { return };
        let pats = patterns();
        let mut payload = Vec::new();
        let mut it = rest.iter();
        while let Some(&c) = it.next() {
            match (c, it.as_slice().first()) {
                (0xff, Some(&k)) => {
                    it.next();
                    payload.extend_from_slice(&pats[usize::from(k) % pats.len()].from);
                }
                _ => payload.push(c),
            }
        }
        let step = 1 + usize::from(size & 15);
        let cuts: Vec<usize> = (1..).map(|k| k * step).take_while(|&c| c < payload.len()).collect();
        streamed(&pats, &payload, &cuts, b"\r\n3\r\n");
    }
}

#[cfg(test)]
#[path = "swap/tests.rs"]
mod tests;
