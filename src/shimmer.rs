//! The turn's word, honestly (motion §3) — what a turn is doing as its stream shows it: one state, `pace`, read by
//! every surface that draws the turn's word (the chat's top line, its `thinking…` row, the follow card's head).
//!
//! - `Pace::Wave` — working, not streaming: a thinking block open (summarized thinking sends nothing until it's done,
//!   then lands whole — measured 2026-10-25, `cargo xtask stream --think`: 1.2–1.9 s silent in short turns), a run of
//!   Claude's under way, the turn's start before its first event, a request out with nothing of its answer yet (a
//!   tool's result gone back: 2.75 s measured before the next message's first event), a compaction. The clock's wave
//!   `▂▅▇` beside a plain word, as before.
//! - `Pace::Shimmer` — deltas arriving (text, thinking's summary, an edit's input: what tarae's reader sends,
//!   `llm::pump`): a band over the word — the accent at its centre over the word's dim ink (the user's pick on the
//!   contact sheets, 2026-10-25: the full ink alone hardly showed on hanji) — moved by the stream alone — `min(bytes / BYTES_A_CELL, CELLS_MAX)`
//!   cells a batch (16 bytes ≈ 4 tokens: prose at its measured 328 chars/s ≈ 15 cells/s), a lump's rest dropped, never
//!   played out later (moving after a stall would be the lie). No frames of its own: each delta is an event and a
//!   frame already.
//! - `Pace::Still` — neither, no delta for `STILL_AFTER`: the band stays where the stream left it; `◦ stuck?` as
//!   before (`Doing::Stuck`).
//!
//! The latest sign wins: a delta within `STILL_AFTER` is `Shimmer` unless something since said the stream paused for work
//! (a thinking block opening, a tool call, a request out — each lets the delta before it go); thinking's summary lands
//! after its block opened, while it's still open: the shimmer. Past `STILL_AFTER`, what works without streaming is the
//! wave again. The wave is the turn's one sign where nothing
//! streams, the band where something does — never both. At `calm` and `none` the band isn't drawn (a plain word),
//! the wave as `calm` has it; drawn only where motion is (`Editor.motion_drawn` — off in tests).

use std::time::{Duration, Instant};

use crate::workspace::Workspace;

/// Bytes of the stream a cell of the band's travel.
pub const BYTES_A_CELL: usize = 16;

/// The most a batch moves the band, however much it carried.
pub const CELLS_MAX: f32 = 2.0;

/// The band's half width, in cells — and how far past each end of the word it sweeps.
pub const HALF: f32 = 3.0;

/// No delta for this long and the stream is still (the measured stream: prose's gaps ≤ 89 ms, an edit's input in clumps
/// about 0.6 s apart).
pub const STILL_AFTER: Duration = Duration::from_secs(1);

/// What the turn is doing, as its stream shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    Wave,
    Shimmer,
    Still,
}

/// The turn's stream so far (`Chat.pace`): made anew as a turn starts.
#[derive(Clone, Debug, Default)]
pub struct Band {
    /// Cells the band has travelled this turn.
    pub at: f32,
    /// When the last delta came — None once something since said the stream paused for work.
    pub last: Option<Instant>,
    /// A thinking block is open (from its start to its end in the stream).
    pub thinking: bool,
    /// Something of the turn came: a delta, a thinking block, a tool call.
    pub begun: bool,
    /// A request went out — a tool's result back to Claude, claude's `requesting` — and nothing of its answer came yet.
    pub asked: bool,
}

impl Band {
    /// A batch of `bytes` arrived: the band moves by them, `CELLS_MAX` at most (the rest dropped).
    pub fn delta(&mut self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        self.at += (bytes as f32 / BYTES_A_CELL as f32).min(CELLS_MAX);
        self.last = Some(crate::clock::now());
        self.begun = true;
        self.asked = false;
    }

    /// A thinking block began or ended.
    pub fn thinking(&mut self, open: bool) {
        self.thinking = open;
        self.begun = true;
        if open {
            (self.asked, self.last) = (false, None);
        }
    }

    /// Something of the turn came that isn't a delta (a tool call): the stream paused for it.
    pub fn event(&mut self) {
        self.begun = true;
        (self.asked, self.last) = (false, None);
    }

    /// A request went out: the wave until something of its answer comes.
    pub fn asked(&mut self) {
        (self.asked, self.last) = (true, None);
    }
}

/// What session `l`'s turn is doing — None: no turn under way.
pub fn pace(l: &Workspace) -> Option<Pace> {
    let c = l.claude.chat.as_ref().filter(|c| c.busy())?;
    let p = &c.pace;
    let compacting = crate::compaction::under_way(l);
    if !compacting && p.last.is_some_and(|t| crate::clock::since(t) < STILL_AFTER) {
        return Some(Pace::Shimmer);
    }
    let runs = c.calls.iter().any(|k| matches!(k.name.as_str(), "mcp__tarae__run" | "mcp__tarae__run_tests"));
    Some(match compacting || p.thinking || runs || p.asked || !p.begun {
        true => Pace::Wave,
        false => Pace::Still,
    })
}

/// How much of the band's ink cell `i` of a word `w` cells wide takes, 0 ‥ 1, the band having travelled `at`: a cosine
/// `HALF` cells half-wide, sweeping the word and `HALF` cells past each end, then again from the left. At 0 it stands
/// just left of the word (a turn's word starts plain).
pub fn ink(at: f32, i: usize, w: usize) -> f32 {
    let sweep = w as f32 + 2.0 * HALF;
    let centre = at.rem_euclid(sweep) - HALF;
    let d = (i as f32 - centre).abs();
    match d < HALF {
        true => 0.5 * (1.0 + (std::f32::consts::PI * d / HALF).cos()),
        false => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_moves_the_band_by_its_bytes_and_a_lump_two_cells_at_most() {
        let mut b = Band::default();
        b.delta(8);
        assert_eq!(b.at, 0.5);
        b.delta(16);
        assert_eq!(b.at, 1.5);
        // (a lump: 419 bytes of a thinking summary at once — two cells, the rest dropped)
        b.delta(419);
        assert_eq!(b.at, 3.5);
        b.delta(0);
        assert_eq!(b.at, 3.5, "nothing came: nothing moves");
        assert!(b.begun);
    }

    #[test]
    fn the_band_starts_beside_the_word_and_sweeps_past_both_ends() {
        let w = 7;
        assert!((0..w).all(|i| ink(0.0, i, w) == 0.0), "a turn's word starts plain");
        // (its centre on the word's first cell after `HALF` cells, on its last after `HALF + w - 1`)
        assert_eq!(ink(HALF, 0, w), 1.0);
        assert_eq!(ink(HALF + 6.0, 6, w), 1.0);
        assert!(ink(HALF, 1, w) > 0.5 && ink(HALF, 3, w) == 0.0);
        // (past the end, gone; a whole sweep later, beside it again)
        assert!((0..w).all(|i| ink(2.0 * HALF + w as f32, i, w) == 0.0));
        assert!((0..w).all(|i| ink(2.0 * HALF + w as f32 + HALF, i, w) == ink(HALF, i, w)));
    }
}
