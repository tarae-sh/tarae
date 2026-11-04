//! The way back to a host over ssh (the remote plan's S-D, and its bar: a sleeping screen never loses the session) —
//! the rule, pure: `client.rs` drives it with its clock and runs the tries it asks for; the tests drive it with theirs.
//!
//! A link is taken for down when it ends with no `Gone` (ssh exited, its pipe's end) or when a `Ping` sent on a wake
//! (the wall clock ahead of the monotonic one, which counts no sleep) or a focus back has no `Pong` within
//! `PONG_WITHIN`. Down, a try starts at once, asking nothing (`BatchMode`); the last frame stays up and nothing is said
//! — unless a try fails, or the first one takes past `SAY_AFTER`: then one line, `q` quits and `r` tries now. A failed
//! try is tried again after 1, 2, 4 … `BACKOFF_MAX` s (a wake sooner), but one the host refused (a `Gone`: another
//! protocol after a restart) isn't. Never a clock while up: an idle client sends nothing.

use std::time::{Duration, Instant, SystemTime};

/// A try with no `Welcome` by this, and the line is said.
pub const SAY_AFTER: Duration = Duration::from_secs(3);
/// A `Ping` with no `Pong` by this, and the link is taken for gone.
pub const PONG_WITHIN: Duration = Duration::from_secs(3);
/// The wall clock this far ahead of the monotonic one since the last look: the machine slept.
pub const WAKE_GAP: Duration = Duration::from_secs(5);
/// The longest wait between two tries.
pub const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// What the client does now.
#[derive(Debug, PartialEq, Eq)]
pub enum Do {
    /// A try: ssh run again asking nothing (`wait`: the relay waiting for a restarted host's socket).
    Try { wait: bool },
    /// The link taken for gone: ssh cut, then a try.
    Cut,
    /// The line drawn over the last frame.
    Say(Line),
}

/// The client's line: what happened, why (may be empty), and its keys.
#[derive(Debug, PartialEq, Eq)]
pub struct Line {
    pub head: String,
    pub why: String,
    pub keys: String,
}

impl Line {
    /// As drawn in `w` cells: the head and the keys whole, the why cut to the room between them.
    pub fn fit(&self, w: usize) -> String {
        use unicode_width::UnicodeWidthStr;
        let (head, keys) = (&self.head, &self.keys);
        match (head.is_empty(), self.why.is_empty()) {
            (_, true) => format!(" {head} — {keys}"),
            (true, false) => {
                let room = w.saturating_sub(format!("  — {keys}").width());
                format!(" {} — {keys}", crate::cut::fit_ellipsis(&self.why, room))
            }
            (false, false) => {
                let room = w.saturating_sub(format!(" {head}:  — {keys}").width());
                format!(" {head}: {} — {keys}", crate::cut::fit_ellipsis(&self.why, room))
            }
        }
    }
}

fn line(head: String, why: &str, keys: &str) -> Do {
    // (one row: the relay's no lists its hosts on the lines after)
    let why = why.lines().next().unwrap_or_default().into();
    Do::Say(Line { head, why, keys: keys.into() })
}

#[derive(Debug)]
enum State {
    /// The link is there; a `Ping` out, and when it went.
    Up {
        ping: Option<(u32, Instant)>,
    },
    Down(Down),
}

#[derive(Debug)]
struct Down {
    /// Since when — the wake seen (the `Ping`'s), or the link's end: what "back in" is measured from.
    since: Instant,
    /// The host restarts (`Gone::Restarting`), not a drop.
    restart: bool,
    /// A try under way since then.
    trying: Option<Instant>,
    /// Tries failed in a row, and when the next one goes.
    failed: u32,
    next: Option<Instant>,
    /// The line is drawn: `r` and `q` are taken.
    said: bool,
    /// The host let this go (a `Gone`): no try by itself.
    given_up: bool,
}

#[derive(Debug)]
pub struct Back {
    /// The machine, as the line says it.
    host: String,
    state: State,
    /// The clocks at the last look.
    looked: (Instant, SystemTime),
    pings: u32,
}

impl Back {
    pub fn new(host: &str, now: Instant, wall: SystemTime) -> Back {
        Back { host: host.into(), state: State::Up { ping: None }, looked: (now, wall), pings: 0 }
    }

    /// Whether the machine slept since the last look — the wall clock gone on past the monotonic one. Looked at as the
    /// client wakes for anything (a key, a focus, a message, a timer); a wake while down tries at once.
    pub fn woke(&mut self, now: Instant, wall: SystemTime) -> bool {
        let (then, was) = std::mem::replace(&mut self.looked, (now, wall));
        let mono = now.saturating_duration_since(then);
        let walked = wall.duration_since(was).unwrap_or_default();
        let woke = walked > mono + WAKE_GAP;
        if woke
            && let State::Down(d) = &mut self.state
            && d.trying.is_none()
            && !d.given_up
        {
            d.next = Some(now);
        }
        woke
    }

    /// A `Ping` to send (its `n`) — while up and none is out.
    pub fn ping(&mut self, now: Instant) -> Option<u32> {
        let State::Up { ping: ping @ None } = &mut self.state else { return None };
        self.pings = self.pings.wrapping_add(1);
        *ping = Some((self.pings, now));
        Some(self.pings)
    }

    pub fn pong(&mut self, n: u32) {
        if let State::Up { ping } = &mut self.state
            && ping.is_some_and(|(p, _)| p == n)
        {
            *ping = None;
        }
    }

    /// The link ended with no `Gone` — a try now (None: it was down already).
    pub fn dropped(&mut self, now: Instant) -> Option<Do> {
        self.down(now, false).then_some(Do::Try { wait: false })
    }

    /// `Gone::Restarting`: a try now, the relay waiting for the new host.
    pub fn restarting(&mut self, now: Instant) -> Option<Do> {
        self.down(now, true).then_some(Do::Try { wait: true })
    }

    fn down(&mut self, now: Instant, restart: bool) -> bool {
        let State::Up { ping } = self.state else { return false };
        let since = ping.map_or(now, |(_, at)| at);
        self.state = State::Down(Down {
            since,
            restart,
            trying: Some(now),
            failed: 0,
            next: None,
            said: false,
            given_up: false,
        });
        true
    }

    /// The try came back with the host's `Welcome`: up again, and since when it was down.
    pub fn up(&mut self) -> Option<Instant> {
        let State::Down(d) = std::mem::replace(&mut self.state, State::Up { ping: None }) else {
            return None;
        };
        Some(d.since)
    }

    /// The try failed (`why`; `gone`: the host refused it — trying again won't help): the line, and the next try's
    /// time unless given up.
    pub fn failed(&mut self, now: Instant, why: &str, gone: bool) -> Option<Do> {
        let State::Down(d) = &mut self.state else { return None };
        d.trying = None;
        d.said = true;
        d.failed += 1;
        let host = &self.host;
        if gone {
            d.given_up = true;
            d.next = None;
            return Some(line(String::new(), why, "r again · q quit"));
        }
        let wait = backoff(d.failed);
        d.next = Some(now + wait);
        let what = match d.restart {
            true => format!("the host on {host} restarted, but"),
            false => format!("the link to {host} dropped"),
        };
        let keys = format!("again in {} · r now · q quit", crate::term::run_duration(wait));
        Some(line(what, why, &keys))
    }

    /// `r` on the line: a try now (false: one is under way, or nothing's said).
    pub fn now(&mut self, now: Instant) -> bool {
        match &mut self.state {
            State::Down(d) if d.said && d.trying.is_none() => {
                (d.trying, d.next) = (Some(now), None);
                true
            }
            _ => false,
        }
    }

    /// What's due at `now`: a `Ping` overdue cuts the link, a try past `SAY_AFTER` says the line, the next try goes.
    pub fn tick(&mut self, now: Instant) -> Option<Do> {
        if let State::Up { ping: Some((_, at)) } = self.state
            && now >= at + PONG_WITHIN
        {
            self.down(now, false);
            return Some(Do::Cut);
        }
        let host = &self.host;
        let State::Down(d) = &mut self.state else { return None };
        match d.trying {
            Some(at) if !d.said && now >= at + SAY_AFTER => {
                d.said = true;
                let head = match d.restart {
                    true => format!("the host on {host} restarts"),
                    false => format!("the link to {host} dropped"),
                };
                let keys =
                    if d.restart { "attaching again… · q quit" } else { "reconnecting… · q quit" };
                Some(line(head, "", keys))
            }
            Some(_) => None,
            None => {
                let due = d.next.is_some_and(|n| now >= n);
                if due {
                    (d.trying, d.next) = (Some(now), None);
                }
                due.then_some(Do::Try { wait: false })
            }
        }
    }

    /// When `tick` has something next — None: nothing (idle).
    pub fn due(&self) -> Option<Instant> {
        match &self.state {
            State::Up { ping } => ping.map(|(_, at)| at + PONG_WITHIN),
            State::Down(d) => match d.trying {
                Some(at) if !d.said => Some(at + SAY_AFTER),
                Some(_) => None,
                None => d.next,
            },
        }
    }

    pub fn is_down(&self) -> bool {
        matches!(self.state, State::Down(_))
    }

    /// The line is drawn: `r` and `q` are the client's.
    pub fn said(&self) -> bool {
        matches!(&self.state, State::Down(d) if d.said)
    }
}

/// The wait after `failed` tries failed in a row: 1, 2, 4 … s, at most `BACKOFF_MAX`.
fn backoff(failed: u32) -> Duration {
    Duration::from_secs(1u64 << failed.saturating_sub(1).min(5)).min(BACKOFF_MAX)
}

#[cfg(test)]
mod tests;
