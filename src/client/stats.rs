//! `tarae attach --stats` (the remote plan's §8d, "Tests and what's measured"): what the client measured, said at its
//! end on its terminal, once it's restored (the way back's times are in its log as they happen too). Pure: `client.rs` tells it what was sent and presented,
//! by its clock; the tests by theirs. One clock, the client's: nothing to agree with the host's.
//!
//! - **A key's round trip**: from an `Input`'s send to the first presented `Frame` whose `input` is at least its
//!   `seq`; that frame's bytes shared among the inputs it answered. Only an input sent on a link that stayed up: one
//!   waiting as a link goes down is let go (`down`), its frame never coming.
//! - **The first frame**: from the client's start to its first frame presented, ssh's handshake in it.
//! - **Back**: each way back, from the link down (or the wake seen) to the new link's first frame presented.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// The words every line of it starts with — what `cargo xtask remote` finds them by in the client's output.
pub const SAID: &str = "tarae attach --stats:";

pub struct Stats {
    start: Instant,
    first: Option<Duration>,
    /// Inputs sent and not yet answered by a presented frame, oldest first: their `seq` and when.
    waiting: VecDeque<(u64, Instant)>,
    keys: Vec<Duration>,
    /// The bytes of the frames that answered an input.
    bytes: u64,
    back: Vec<Duration>,
}

impl Stats {
    pub fn new(start: Instant) -> Stats {
        Stats { start, first: None, waiting: VecDeque::new(), keys: Vec::new(), bytes: 0, back: Vec::new() }
    }

    /// Input `seq` sent at `at`.
    pub fn sent(&mut self, seq: u64, at: Instant) {
        self.waiting.push_back((seq, at));
    }

    /// A frame presented at `at` — drawn once input `input` was handled, `bytes` long.
    pub fn presented(&mut self, input: u64, bytes: usize, at: Instant) {
        self.first.get_or_insert(at.saturating_duration_since(self.start));
        let mut answered = false;
        while let Some(&(seq, sent)) = self.waiting.front()
            && seq <= input
        {
            self.waiting.pop_front();
            self.keys.push(at.saturating_duration_since(sent));
            answered = true;
        }
        if answered {
            self.bytes += bytes as u64;
        }
    }

    /// The link went down: what waits on it has no frame coming.
    pub fn down(&mut self) {
        self.waiting.clear();
    }

    /// A way back took `took`.
    pub fn back(&mut self, took: Duration) {
        self.back.push(took);
    }

    /// What it says, a line each.
    pub fn said(&self) -> Vec<String> {
        let mut out = Vec::new();
        match self.first {
            Some(f) => {
                out.push(format!("{SAID} the first frame {} from the start (ssh's handshake in it)", ms(f)))
            }
            None => out.push(format!("{SAID} no frame presented")),
        }
        match self.keys.len() {
            0 => out.push(format!("{SAID} no key answered")),
            n => out.push(format!(
                "{SAID} {n} keys, each to its frame: median {}, p90 {}, {} B a key",
                ms(rank(&self.keys, 0.5)),
                ms(rank(&self.keys, 0.9)),
                self.bytes / n as u64
            )),
        }
        if !self.back.is_empty() {
            let each: Vec<String> = self.back.iter().map(|d| ms(*d)).collect();
            out.push(format!(
                "{SAID} back {} times, the link down to a frame presented: {}",
                self.back.len(),
                each.join(", ")
            ));
        }
        out
    }
}

/// A measure as it's said: milliseconds, two places (a key over a local link is under one).
fn ms(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1000.0)
}

/// The nearest-rank `q` quantile of `of` (not empty).
fn rank(of: &[Duration], q: f64) -> Duration {
    let mut v = of.to_vec();
    crate::sorting::sort(&mut v);
    let i = ((q * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1;
    v[i]
}

#[cfg(test)]
mod tests;
