//! The working sign (CLAUDE.md, "One working sign, one cell that rises"; #159, motion's `.sb/wave-plan.md` C, then the
//! breath alone — the user, 2026-10-28, on `cargo xtask sign`'s animations: the stream's level read as stutters) — what
//! a turn is doing: one state, `pace`, and one cell drawn from it (`term::working`) wherever Claude or a run works: the
//! top lines, the follow card, the now line, a tab's badge, an ask's peek, the step list. The cell keeps its place for
//! the whole turn; it moves one way, an even breath, and stops when the stream does.
//!
//! - `Pace::Breath` — working: deltas arriving (text, thinking's summary, an edit's input), a thinking block open
//!   (summarized thinking sends nothing until it's done — measured 2026-10-25, `cargo xtask stream --think`: 1.2–1.9 s
//!   silent in short turns), a run of Claude's under way, the turn's start before its first event, a request out with
//!   nothing of its answer yet (a tool's result gone back: 2.75 s measured before the next message's first event), a
//!   compaction. The cell breathes (`breath`): an even rise and fall, `BREATH` long, whatever the stream does — a delta
//!   changes nothing. A turn's breath counts from its own start (`Band::from`): from the floor as the turn starts, and
//!   again as it leaves `Still` (`woke`), so it never jumps a height on the way out.
//! - `Pace::Still` — the stream stopped: no delta for `STILL_AFTER` and nothing else at work. The cell at the floor and
//!   still, at once; `◦ stuck?` as before (`Doing::Stuck`).
//!
//! A delta within `STILL_AFTER` breathes unless something since said the stream paused for work (a thinking block
//! opening, a tool call, a request out — each lets the delta before it go, and breathes for itself). At `calm` and `none`
//! the cell holds `CELLS[STILL]`; still and the turn's own breath are drawn only where motion is (`Editor.motion_drawn`
//! — off in tests, where the cell breathes on the tests' still clock).

use std::time::{Duration, Instant};

use crate::workspace::Workspace;

/// The working sign's heights, low to high — never below `▃` (`▁ ▂` all but vanished on hanji in the plan's shots,
/// and `▁` read as `_`), never above `▆` (`▇` in the accent read as the code's block cursor: the user, 2026-10-27, on
/// the ceiling's shots).
pub const CELLS: [char; 4] = ['▃', '▄', '▅', '▆'];

/// The height held where nothing moves on a clock (`calm`, `none`).
pub const STILL: usize = 1;

/// One breath of the cell where nothing streams (Claude Code's spinner breathes on the same period).
pub const BREATH: Duration = Duration::from_secs(2);

/// No delta for this long and the stream is still (the measured stream: prose's gaps ≤ 89 ms, an edit's input in clumps
/// about 0.6 s apart).
pub const STILL_AFTER: Duration = Duration::from_secs(1);

/// What the turn is doing, as its stream shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    Breath,
    Still,
}

/// The turn's stream so far (`Chat.pace`): made anew as a turn starts (`Band::begin`).
#[derive(Clone, Debug, Default)]
pub struct Band {
    /// The animation step (`editor::anim_step`) the turn's breath counts from: the turn's start, and the last time it
    /// left `Pace::Still`.
    pub from: u128,
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
    /// A turn's, as it starts: its breath from the floor now.
    pub fn begin() -> Band {
        Band { from: crate::editor::anim_step(), ..Band::default() }
    }

    /// A batch of `bytes` arrived: the stream is flowing (how much never shows).
    pub fn delta(&mut self, bytes: usize) {
        if bytes == 0 {
            return;
        }
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

    /// A request went out: the breath until something of its answer comes.
    pub fn asked(&mut self) {
        (self.asked, self.last) = (true, None);
    }
}

/// What session `l`'s turn is doing — None: no turn under way.
pub fn pace(l: &Workspace) -> Option<Pace> {
    let c = l.claude.chat.as_ref().filter(|c| c.busy())?;
    let p = &c.pace;
    let flowing = p.last.is_some_and(|t| crate::clock::since(t) < STILL_AFTER);
    let runs = c.calls.iter().any(|k| matches!(k.name.as_str(), "mcp__tarae__run" | "mcp__tarae__run_tests"));
    Some(match flowing || crate::compaction::under_way(l) || p.thinking || runs || p.asked || !p.begun {
        true => Pace::Breath,
        false => Pace::Still,
    })
}

/// After an event of `l`'s turn (`chat::on_event`), its pace `before` it: a turn that was still and breathes again
/// breathes from the floor, now.
pub fn woke(l: &mut Workspace, before: Option<Pace>) {
    if before != Some(Pace::Still) || pace(l) != Some(Pace::Breath) {
        return;
    }
    if let Some(c) = l.claude.chat.as_mut() {
        c.pace.from = crate::editor::anim_step();
    }
}

/// The turn's cell's height now: the floor while still, else its breath from `Band::from` — None: no turn under way.
pub fn turn_height(l: &Workspace) -> Option<usize> {
    Some(match pace(l)? {
        Pace::Still => 0,
        Pace::Breath => {
            let from = l.claude.chat.as_ref().map_or(0, |c| c.pace.from);
            breath(crate::editor::anim_step().saturating_sub(from))
        }
    })
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
    fn a_delta_marks_the_stream_flowing_and_an_empty_one_nothing() {
        let mut b = Band::default();
        b.delta(0);
        assert!(!b.begun && b.last.is_none(), "nothing came");
        b.asked();
        b.delta(419);
        assert!(b.begun && b.last.is_some() && !b.asked, "its answer came");
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
