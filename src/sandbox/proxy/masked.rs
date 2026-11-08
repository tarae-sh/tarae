//! Masked credentials (P2 of `.sb/credential-masking-plan.md`; `.sb/masking-p2a-step1.md` as the design session
//! answered it, 2026-11-08): a variable `llm.sandbox-secrets` names reaches a run as a sentinel, and tarae's proxy
//! swaps the real value in only toward the hosts named with it, inside the tunnels it terminates (`stage`).
//!
//! The registry: per session (the proxy's, the CA's key), in tarae's memory only — each name's sentinel, kept across
//! the session's runs (a daemon lives across them) and forgotten with it (`forget`); the real value read again from
//! tarae's own environment (`Here.env`) each time a run's policy is made. A name tarae's environment lacks gets
//! nothing, said. It rides each run's `Egress` by `Arc`, as the CA does: never in a run's token, its environment, the
//! keeper or the wire. `Debug` gives names and counts, never a value.
//!
//! Sentinels (U1, decided 2026-11-08): a value of 20 bytes or more gets one exactly as long — `fake_value_` and
//! random characters of srt's alphabet, so a body's length and an answer's never change; a shorter one srt's 47-byte
//! `fake_value_<uuid4>`, swapped in heads only (a body or an answer holding it is left, said). Unique in the session,
//! none inside another. A value of fewer than 8 bytes, or with a byte past visible ASCII, isn't masked (`maskable`).

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use super::egress::Pattern;
use super::parse::{self, Body};
use super::swap::{self, Does, Scan};

/// The shortest value masked: a shorter one would scrub common text from every answer's head (an ETag, a date).
pub const FLOOR: usize = 8;
/// From this length on a value's sentinel is exactly as long (U1): swapped in bodies and scrubbed from answers too.
pub const SAME_LENGTH: usize = 20;
/// Every sentinel's start (srt's).
const PREFIX: &str = "fake_value_";
/// srt's base sentinel's length: `fake_value_` and a UUID.
const BASE: usize = 47;
/// What follows the prefix in a sentinel as long as its value: URL-unreserved, so never percent-encoded.
const ALPHABET: &[u8; 38] = b"abcdefghijklmnopqrstuvwxyz0123456789_-";

/// Whether `value` can be masked — Err: why not, in a report's words. Visible ASCII only (e: swapped into a header,
/// any other byte could start another line), and at least `FLOOR` bytes (m).
pub fn maskable(value: &[u8]) -> Result<(), &'static str> {
    if !value.iter().all(|c| (0x21..=0x7e).contains(c)) {
        return Err(
            "a byte that isn't visible ASCII (a space, a control character, past 0x7E) — it can't go in a \
                    header as it is, so it isn't masked",
        );
    }
    if value.len() < FLOOR {
        return Err("too short to mask safely — its card can pass the real value");
    }
    Ok(())
}

/// One masked variable: its name, its sentinel, its real value, the hosts it goes to.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret {
    pub name: String,
    pub sentinel: String,
    real: Vec<u8>,
    pub hosts: Vec<Pattern>,
}

/// Its name and its hosts' count — never a value.
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret({}, {} hosts)", self.name, self.hosts.len())
    }
}

impl Secret {
    /// Whether it goes to `host:port` (a CONNECT's, 443 its default).
    fn goes_to(&self, host: &str, port: u16) -> bool {
        self.hosts.iter().any(|p| p.matches(host, port, 443))
    }
}

/// A session's masked variables, as a run's policy made them (by name).
#[derive(Clone, PartialEq, Eq)]
pub struct Masked {
    secrets: Vec<Secret>,
}

/// Names and hosts' counts — never a value.
impl std::fmt::Debug for Masked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(&self.secrets).finish()
    }
}

impl Masked {
    /// Each name and its sentinel — what the run's environment holds.
    pub fn sentinels(&self) -> impl Iterator<Item = (&str, &str)> {
        self.secrets.iter().map(|s| (s.name.as_str(), s.sentinel.as_str()))
    }

    pub fn secrets(&self) -> &[Secret] {
        &self.secrets
    }

    /// Every host a secret goes to, each once: what the run's proxy terminates (D1).
    pub fn hosts(&self) -> Vec<Pattern> {
        let mut all: Vec<Pattern> = Vec::new();
        for p in self.secrets.iter().flat_map(|s| &s.hosts) {
            if !all.contains(p) {
                all.push(p.clone());
            }
        }
        all
    }

    /// The name of a sentinel `b` holds, if any — what tarae reads in plaintext outside a terminated tunnel (a
    /// CONNECT's target, a plain request's host and head) is refused with it (k, D5).
    pub fn sentinel_in(&self, b: &[u8]) -> Option<&str> {
        let s = self.secrets.iter().find(|s| swap::find(b, s.sentinel.as_bytes(), 0).is_some())?;
        Some(&s.name)
    }

    /// A plain request's body watched for any sentinel (nothing held: what went before is fake).
    pub fn watch(&self) -> Scan {
        let strays = self.secrets.iter().map(|s| swap::Pattern {
            from: s.sentinel.clone().into_bytes(),
            to: Vec::new(),
            does: Does::Stray,
            name: s.name.clone(),
        });
        Scan::new(strays.collect(), false)
    }

    /// What a tunnel to `host:port` swaps, refuses and scrubs — made once for it.
    pub fn toward(&self, host: &str, port: u16) -> View {
        let (own, others): (Vec<&Secret>, Vec<&Secret>) =
            self.secrets.iter().partition(|s| s.goes_to(host, port));
        // (l: each real value once, scrubbed to the sentinel of the secret that goes here, else the first by name)
        let mut reals: Vec<(Vec<u8>, Vec<u8>, String)> = Vec::new();
        for s in &self.secrets {
            if reals.iter().any(|(r, ..)| *r == s.real) {
                continue;
            }
            let same = self.secrets.iter().filter(|o| o.real == s.real);
            let chosen = same.clone().find(|o| o.goes_to(host, port)).unwrap_or(s);
            reals.push((s.real.clone(), chosen.sentinel.clone().into_bytes(), chosen.name.clone()));
        }
        crate::sorting::by_key(&mut reals, |(r, ..)| std::cmp::Reverse(r.len()));
        View {
            at: format!("{host}:{port}"),
            own: own
                .iter()
                .map(|s| (s.name.clone(), s.sentinel.clone().into_bytes(), s.real.clone()))
                .collect(),
            strays: others.iter().map(|s| (s.name.clone(), s.sentinel.clone().into_bytes())).collect(),
            reals,
        }
    }

    /// Tests: a registry of these (name, sentinel, real value, hosts), by name.
    #[cfg(test)]
    pub fn of(list: &[(&str, &str, &str, &[&str])]) -> Masked {
        let mut secrets: Vec<Secret> = list
            .iter()
            .map(|(name, sentinel, real, hosts)| Secret {
                name: name.to_string(),
                sentinel: sentinel.to_string(),
                real: real.as_bytes().to_vec(),
                hosts: hosts.iter().map(|h| Pattern::parse(h).unwrap()).collect(),
            })
            .collect();
        secrets.sort_by(|a, b| a.name.cmp(&b.name));
        Masked { secrets }
    }
}

/// The sessions' sentinels: (session, name, sentinel).
static SENTINELS: Mutex<Vec<(u64, String, String)>> = Mutex::new(Vec::new());

/// Session `session`'s masked variables for a run: each of `table`'s (`llm.sandbox-secrets`: name → host rules) that
/// `names` holds (`RunEnv.masked`: in tarae's environment, `maskable`), its real value `env`'s, its sentinel the
/// session's (minted the first time, again where the value's length calls for another) — None: none. What the
/// report says besides: a name `env` lacks, a sentinel that couldn't be made. A worker's.
pub fn of_session(
    session: u64,
    table: &BTreeMap<String, Vec<String>>,
    names: &[String],
    env: &HashMap<String, OsString>,
) -> (Option<Arc<Masked>>, Vec<String>) {
    let mut said = Vec::new();
    let mut kept = SENTINELS.lock().unwrap_or_else(|e| e.into_inner());
    let mut secrets: Vec<Secret> = Vec::new();
    for (name, hosts) in table {
        let Some(value) = env.get(name) else {
            said.push(format!(
                "{name} (llm.sandbox-secrets) isn't in tarae's environment — your commands don't get it"
            ));
            continue;
        };
        // (one `maskable` refused is said where the run's environment was made: `runenv::of`)
        if !names.contains(name) {
            continue;
        }
        let real = value.as_encoded_bytes().to_vec();
        let len = if real.len() >= SAME_LENGTH { real.len() } else { BASE };
        let hosts: Vec<Pattern> = hosts.iter().filter_map(|h| Pattern::parse(h).ok()).collect();
        let theirs =
            |n: &String| kept.iter().find(|(s, k, _)| *s == session && k == n).map(|(.., t)| t.clone());
        let sentinel = match theirs(name).filter(|t| t.len() == len) {
            Some(t) => t,
            None => {
                // (unique in the session, none inside another, nor inside a real value)
                let taken: Vec<Vec<u8>> = (kept.iter().filter(|(s, k, _)| *s == session && k != name))
                    .map(|(.., t)| t.clone().into_bytes())
                    .chain(table.keys().filter_map(|n| env.get(n)).map(|v| v.as_encoded_bytes().to_vec()))
                    .collect();
                let free = |t: &str| {
                    taken.iter().all(|o| {
                        swap::find(o, t.as_bytes(), 0).is_none() && swap::find(t.as_bytes(), o, 0).is_none()
                    })
                };
                match (0..8).filter_map(|_| mint(len)).find(|t| free(t)) {
                    Some(t) => t,
                    None => {
                        said.push(format!(
                            "{name} (llm.sandbox-secrets): a masked value couldn't be made — your commands don't \
                             get it"
                        ));
                        continue;
                    }
                }
            }
        };
        kept.retain(|(s, k, _)| !(*s == session && k == name));
        kept.push((session, name.clone(), sentinel.clone()));
        secrets.push(Secret { name: name.clone(), sentinel, real, hosts });
    }
    let masked = (!secrets.is_empty()).then(|| Arc::new(Masked { secrets }));
    (masked, said)
}

/// The session ended: its sentinels go (runs still holding them keep theirs).
pub fn forget(session: u64) {
    SENTINELS.lock().unwrap_or_else(|e| e.into_inner()).retain(|(s, ..)| *s != session);
}

/// A new sentinel `len` bytes long: srt's `fake_value_<uuid4>` at `BASE`, else `fake_value_` and random characters
/// of `ALPHABET` (a uniform draw: bytes past its last whole multiple dropped). None: no randomness to be had.
fn mint(len: usize) -> Option<String> {
    use ring::rand::SecureRandom;
    let rng = ring::rand::SystemRandom::new();
    let mut out = String::from(PREFIX);
    if len == BASE {
        let mut b = [0u8; 16];
        rng.fill(&mut b).ok()?;
        (b[6], b[8]) = ((b[6] & 0x0f) | 0x40, (b[8] & 0x3f) | 0x80);
        for (i, c) in b.iter().enumerate() {
            if [4, 6, 8, 10].contains(&i) {
                out.push('-');
            }
            out.push_str(&format!("{c:02x}"));
        }
        return Some(out);
    }
    let whole = 256 - 256 % ALPHABET.len();
    while out.len() < len {
        let mut b = [0u8; 64];
        rng.fill(&mut b).ok()?;
        let drawn =
            b.iter().filter(|&&c| usize::from(c) < whole).map(|&c| ALPHABET[usize::from(c) % ALPHABET.len()]);
        out.extend(drawn.take(len - out.len()).map(char::from));
    }
    Some(out)
}

/// What a tunnel to one host swaps, refuses and scrubs (`Masked::toward`).
pub struct View {
    /// `host:port`, as its report says it.
    at: String,
    /// The host's own secrets: name, sentinel, real value — swapped in.
    own: Vec<(String, Vec<u8>, Vec<u8>)>,
    /// The others' sentinels, by name: refused toward it (D5).
    strays: Vec<(String, Vec<u8>)>,
    /// Every real value once, longest first, with the sentinel it's scrubbed back to and that one's name (l).
    reals: Vec<(Vec<u8>, Vec<u8>, String)>,
}

/// Names only.
impl std::fmt::Debug for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let own: Vec<&str> = self.own.iter().map(|(n, ..)| n.as_str()).collect();
        write!(f, "View({}, own {own:?}, {} others)", self.at, self.strays.len())
    }
}

/// A head as it goes on (`View::request_head`, `answer_head`), and its body's `Content-Encoding` where it has one
/// other than `identity`: the body then isn't searched.
#[derive(Debug)]
pub struct Head {
    pub bytes: Vec<u8>,
    pub encoded: Option<String>,
}

/// How a body's scan stopped (`through`).
#[derive(Debug, PartialEq, Eq)]
pub enum Stopped {
    /// A framing it can't follow.
    Framing(&'static str),
    /// A sentinel that doesn't go here: its name.
    Stray(String),
}

impl View {
    /// Whether a secret goes here: its requests are edited (the swaps, `Accept-Encoding`, `TE`).
    pub fn edits(&self) -> bool {
        !self.own.is_empty()
    }

    /// The words a sentinel refused here is said with (D5) — its status 403, from the caller.
    pub fn refused(&self, name: &str) -> String {
        format!("a run sent {name}'s masked value toward {} — it goes only to {name}'s own hosts", self.at)
    }

    /// A request body's scan: its own sentinels swapped in, the others' stopping it — None: nothing to look for.
    pub fn request_scan(&self) -> Option<Scan> {
        let own = self.own.iter().map(|(name, sentinel, real)| swap::Pattern {
            from: sentinel.clone(),
            to: real.clone(),
            does: Does::Swap,
            name: name.clone(),
        });
        let strays = self.strays.iter().map(|(name, sentinel)| swap::Pattern {
            from: sentinel.clone(),
            to: Vec::new(),
            does: Does::Stray,
            name: name.clone(),
        });
        let pats: Vec<swap::Pattern> = own.chain(strays).collect();
        (!pats.is_empty()).then(|| Scan::new(pats, true))
    }

    /// An answer body's scan: every real value back to its sentinel — None: no secret.
    pub fn answer_scan(&self) -> Option<Scan> {
        let pats: Vec<swap::Pattern> = (self.reals.iter())
            .map(|(real, sentinel, name)| swap::Pattern {
                from: real.clone(),
                to: sentinel.clone(),
                does: Does::Swap,
                name: name.clone(),
            })
            .collect();
        (!pats.is_empty()).then(|| Scan::new(pats, true))
    }

    /// What the report says of a value left in a body as it was (`Scan::left`) — `out`: a request's, else an answer's.
    pub fn left_said(&self, out: bool, does: Does, name: &str) -> String {
        match (out, does) {
            (true, Does::Note) => format!(
                "{name}'s masked value in a request body went as its fake: the value is under {SAME_LENGTH} bytes, \
                 and a body's length can't change"
            ),
            (true, _) => format!(
                "{name}'s masked value in a request body went as its fake: the body paused part-way through it"
            ),
            (false, _) => format!(
                "{name}'s real value in an answer's body reached the run as it was: the value is under \
                 {SAME_LENGTH} bytes, and a body's length can't change"
            ),
        }
    }

    /// A request's head, decided (`stage::decide`), as it goes on: another secret's sentinel anywhere in it — inside
    /// a `Basic` credential too — refused (Err: why, 403); toward a secret's host, its own sentinels swapped in
    /// every field's value (a `Basic` credential decoded, swapped, encoded again) and in the target (the real value
    /// percent-encoded but what's unreserved, `/` too: a path's segment stays one), every `Accept-Encoding` one
    /// `identity` in the first one's place (U2: none sent, one added), `TE` dropped (n: `gzip, chunked` back is a
    /// framing no reader takes) — nothing else changes, not a space.
    pub fn request_head(&self, head: &[u8]) -> Result<Head, String> {
        let Some(s) = parse::spans(head) else { return Ok(Head { bytes: head.to_vec(), encoded: None }) };
        let named = |f: &parse::Spanned, n: &str| head[f.name.clone()].eq_ignore_ascii_case(n.as_bytes());
        let basic = |f: &parse::Spanned| match named(f, "authorization") {
            true => parse::basic_credential(&head[f.value.clone()]),
            false => None,
        };
        for (name, sentinel) in &self.strays {
            let in_basic =
                s.fields.iter().filter_map(basic).any(|raw| swap::find(&raw, sentinel, 0).is_some());
            if in_basic || swap::find(head, sentinel, 0).is_some() {
                return Err(self.refused(name));
            }
        }
        let encoded = encoding(
            s.fields.iter().filter(|f| named(f, "content-encoding")).map(|f| &head[f.value.clone()]),
        );
        if self.own.is_empty() {
            return Ok(Head { bytes: head.to_vec(), encoded });
        }
        let swaps: Vec<(&[u8], &[u8])> = self.own.iter().map(|(_, s, r)| (&s[..], &r[..])).collect();
        let encoded_reals: Vec<Vec<u8>> = self.own.iter().map(|(.., r)| percent(r)).collect();
        let in_target: Vec<(&[u8], &[u8])> =
            self.own.iter().zip(&encoded_reals).map(|((_, s, _), r)| (&s[..], &r[..])).collect();
        let mut edits: Vec<(Range<usize>, Vec<u8>)> = Vec::new();
        if let Some(t) = swap::replaced(&head[s.target.clone()], &in_target) {
            edits.push((s.target.clone(), t));
        }
        let mut accept = false;
        for f in &s.fields {
            let value = &head[f.value.clone()];
            if named(f, "accept-encoding") && !accept {
                accept = true;
                edits.push((f.value.clone(), b"identity".to_vec()));
            } else if named(f, "accept-encoding") || named(f, "te") {
                edits.push((f.line.clone(), Vec::new()));
            } else if let Some(raw) = basic(f)
                && let Some(swapped) = swap::replaced(&raw, &swaps)
            {
                let scheme = &value[..value.iter().position(|&c| c == b' ').unwrap_or(0)];
                edits.push((f.value.clone(), [scheme, b" ", parse::base64(&swapped).as_bytes()].concat()));
            } else if let Some(v) = swap::replaced(value, &swaps) {
                edits.push((f.value.clone(), v));
            }
        }
        if !accept {
            let end = head.len() - 2;
            edits.push((end..end, b"Accept-Encoding: identity\r\n".to_vec()));
        }
        Ok(Head { bytes: edited(head, edits), encoded })
    }

    /// An answer's head as it goes to the run: every real value back to its sentinel — in its status line and every
    /// field's value (`Location` among them) but its framing's, which the stage read already.
    pub fn answer_head(&self, head: &[u8]) -> Head {
        let Some(s) = parse::spans(head) else { return Head { bytes: head.to_vec(), encoded: None } };
        let named = |f: &parse::Spanned, n: &str| head[f.name.clone()].eq_ignore_ascii_case(n.as_bytes());
        let encoded = encoding(
            s.fields.iter().filter(|f| named(f, "content-encoding")).map(|f| &head[f.value.clone()]),
        );
        let scrubs: Vec<(&[u8], &[u8])> = self.reals.iter().map(|(r, s, _)| (&r[..], &s[..])).collect();
        let mut edits = Vec::new();
        if let Some(l) = swap::replaced(&head[s.first.clone()], &scrubs) {
            edits.push((s.first.clone(), l));
        }
        for f in s.fields.iter().filter(|f| !named(f, "content-length") && !named(f, "transfer-encoding")) {
            if let Some(v) = swap::replaced(&head[f.value.clone()], &scrubs) {
                edits.push((f.value.clone(), v));
            }
        }
        Head { bytes: edited(head, edits), encoded }
    }
}

/// A body's codings as its `Content-Encoding` fields say them, but `identity` — None: none.
fn encoding<'a>(values: impl Iterator<Item = &'a [u8]>) -> Option<String> {
    let codings: Vec<String> = values
        .flat_map(|v| v.split(|&c| c == b','))
        .map(|c| String::from_utf8_lossy(c.trim_ascii()).into_owned())
        .filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case("identity"))
        .collect();
    (!codings.is_empty()).then(|| codings.join(", "))
}

/// `b` percent-encoded but what's unreserved (RFC 3986 §2.3: letters, digits, `-._~`).
fn percent(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    for &c in b {
        match c.is_ascii_alphanumeric() || b"-._~".contains(&c) {
            true => out.push(c),
            false => out.extend_from_slice(format!("%{c:02X}").as_bytes()),
        }
    }
    out
}

/// `head` with each range replaced (none overlapping), in order.
fn edited(head: &[u8], mut edits: Vec<(Range<usize>, Vec<u8>)>) -> Vec<u8> {
    crate::sorting::by_key(&mut edits, |(r, _)| (r.start, r.end));
    let (mut out, mut at) = (Vec::with_capacity(head.len() + 64), 0);
    for (r, with) in edits {
        out.extend_from_slice(&head[at..r.start]);
        out.extend_from_slice(&with);
        at = r.end;
    }
    out.extend_from_slice(&head[at..]);
    out
}

/// `b`'s bytes of `body`, its data through `scan` and its framing around it untouched, what goes on added to `out`:
/// how many of `b` are the body's (as `Body::take`), or why it stopped. The body whole, what's held goes.
pub fn through(body: &mut Body, scan: &mut Scan, b: &[u8], out: &mut Vec<u8>) -> Result<usize, Stopped> {
    let mut data = Vec::new();
    let n = body.ranges(b, |r| data.push(r)).map_err(Stopped::Framing)?;
    let mut at = 0;
    for r in data {
        scan.frame(&b[at..r.start], out);
        scan.data(&b[r.clone()], out).map_err(Stopped::Stray)?;
        at = r.end;
    }
    scan.frame(&b[at..n], out);
    if body.done() {
        scan.end(out).map_err(Stopped::Stray)?;
    }
    Ok(n)
}

#[cfg(test)]
#[path = "masked/tests.rs"]
mod tests;
