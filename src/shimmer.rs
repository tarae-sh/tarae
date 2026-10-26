//! The working sign (CLAUDE.md, "One working sign, one cell that rises"; #159, motion's `.sb/wave-plan.md` C) — what a
//! turn is doing as its stream shows it: one state, `pace`, and one cell drawn from it (`term::working`) wherever
//! Claude or a run works: the top lines, the follow card, the now line, a tab's badge, an ask's peek, the step list.
//! The cell keeps its place for the whole turn; only how it moves changes.
//!
//! - `Pace::Wave` — working, not streaming: a thinking block open (summarized thinking sends nothing until it's done,
//!   then lands whole — measured 2026-10-25, `cargo xtask stream --think`: 1.2–1.9 s silent in short turns), a run of
//!   Claude's under way, the turn's start before its first event, a request out with nothing of its answer yet (a
//!   tool's result gone back: 2.75 s measured before the next message's first event), a compaction. The cell breathes
//!   on the animation's clock (`breath`): an even rise and fall, `BREATH` long.
//! - `Pace::Shimmer` — deltas arriving (text, thinking's summary, an edit's input: what tarae's reader sends,
//!   `llm::pump`): the cell's height is the stream's level (`Band::level`, its bytes of about the last `RATE_TAU`,
//!   `BYTES_A_STEP` a step) — uneven, as the stream is: high while it pours, lower as it thins. No frames of its own:
//!   each delta is an event and a frame already.
//! - `Pace::Still` — neither, no delta for `STILL_AFTER`: the cell at the floor and still, at once — never the level
//!   sinking after a big lump; `◦ stuck?` as before (`Doing::Stuck`).
//!
//! The latest sign wins: a delta within `STILL_AFTER` is `Shimmer` unless something since said the stream paused for work
//! (a thinking block opening, a tool call, a request out — each lets the delta before it go); thinking's summary lands
//! after its block opened, while it's still open: the shimmer. Past `STILL_AFTER`, what works without streaming breathes
//! again. At `calm` and `none` the cell holds `CELLS[STILL]`; the level is drawn only where motion is
//! (`Editor.motion_drawn` — off in tests, where the cell breathes on the tests' still clock).

use std::time::{Duration, Instant};

use crate::workspace::Workspace;

/// The working sign's heights, low to high — never below `▃` (`▁ ▂` all but vanished on hanji in the plan's shots,
/// and `▁` read as `_`), never above `▆` (`▇` in the accent read as the code's block cursor: the user, 2026-10-27, on
/// the ceiling's shots).
pub const CELLS: [char; 4] = ['▃', '▄', '▅', '▆'];

/// The height held where nothing moves on a clock (`calm`, `none`).
pub const STILL: usize = 1;

/// How fast the stream's level falls back: its bytes decay with this time constant, in seconds.
pub const RATE_TAU: f32 = 0.3;

/// Bytes of the level a step of the cell: prose at its measured 328 characters a second keeps about 98 bytes in the
/// level — the top.
pub const BYTES_A_STEP: f32 = 20.0;

/// One breath of the cell where nothing streams (Claude Code's spinner breathes on the same period).
pub const BREATH: Duration = Duration::from_secs(2);

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
    /// The stream's bytes as of `rate_at`, decaying with `RATE_TAU` (`level`).
    pub rate: f32,
    pub rate_at: Option<Instant>,
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
    /// A batch of `bytes` arrived: the level rises by them.
    pub fn delta(&mut self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        self.rate = self.level() + bytes as f32;
        self.rate_at = Some(crate::clock::now());
        self.last = Some(crate::clock::now());
        self.begun = true;
        self.asked = false;
    }

    /// The stream's bytes of about the last `RATE_TAU`, decayed to now: 0 once it has stood still a while.
    pub fn level(&self) -> f32 {
        let since = self.rate_at.map_or(f32::INFINITY, |t| crate::clock::since(t).as_secs_f32());
        self.rate * (-since / RATE_TAU).exp()
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

    /// A request went out: the breath until something of its answer comes.
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

/// The cell's height for a stream's `level` (`Band::level`): a step each `BYTES_A_STEP`, the floor at none.
pub fn height(level: f32) -> usize {
    ((level / BYTES_A_STEP) as usize).min(CELLS.len() - 1)
}

/// The cell's height at `step` of the animation's clock (`editor::anim_step`, a `TICK` a step): an even rise from the
/// floor to the top and back, one `BREATH` long — a cosine, so it lingers at both ends.
pub fn breath(step: u128) -> usize {
    let period = (BREATH.as_millis() / crate::editor::TICK.as_millis()).max(1);
    let t = (step % period) as f32 / period as f32;
    let k = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * t).cos());
    (k * (CELLS.len() - 1) as f32).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_raises_the_level_by_its_bytes_and_a_quiet_lets_it_fall() {
        let mut b = Band::default();
        assert_eq!(height(b.level()), 0, "nothing came: the floor");
        b.delta(8);
        assert!((b.level() - 8.0).abs() < 0.5, "{}", b.level());
        b.delta(16);
        assert!((b.level() - 24.0).abs() < 1.0, "{}", b.level());
        assert_eq!(height(b.level()), 1);
        // (a lump: 419 bytes of a thinking summary at once — the top, no higher)
        b.delta(419);
        assert_eq!(height(b.level()), CELLS.len() - 1);
        b.delta(0);
        assert!(b.begun);
        // (a quiet of a few time constants: back at the floor)
        b.rate_at = b.rate_at.map(|t| t - Duration::from_secs(2));
        assert_eq!(height(b.level()), 0, "{}", b.level());
    }

    #[test]
    fn the_breath_rises_from_the_floor_to_the_top_and_back_in_one_period() {
        let period = BREATH.as_millis() / crate::editor::TICK.as_millis();
        let top = CELLS.len() - 1;
        assert_eq!(breath(0), 0);
        assert_eq!(breath(period / 2), top);
        assert_eq!(breath(period), 0);
        let rise: Vec<usize> = (0..=period / 2).map(breath).collect();
        assert!(rise.windows(2).all(|w| w[0] <= w[1]), "even: {rise:?}");
        assert!((0..period).all(|s| breath(s) == breath(period - s) || s == 0), "and back the same way");
    }
}
