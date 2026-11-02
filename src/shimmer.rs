//! The working sign (CLAUDE.md, "One working sign, three cells that wave"; #159, motion's `.sb/wave-plan.md` C, the
//! breath on the clock — the user, 2026-10-28, on `cargo xtask sign`'s animations: the stream's level read as stutters —
//! then the old wave's three cells again, #277: "one cell feels too plain") — what a turn is doing: one state, `pace`, and
//! `WIDTH` cells drawn from it (`term::working`) wherever Claude or a run works: the top lines, the follow card, the now
//! line, a tab's badge, an ask's peek, the step list. The cells keep their place for the whole turn — a mark standing in
//! for them (`▲`, `◦`, a step's) takes the last, by its word, the ones before it blank (`mark`); they move one way, the
//! wave on the clock, a step every `STEP` (the homepage's, the user's pick of `cargo xtask sign`'s 5b), and stop when
//! the stream does.
//!
//! - `Pace::Breath` — working: deltas arriving (text, thinking's summary, an edit's input), a thinking block open
//!   (summarized thinking sends nothing until it's done — measured 2026-10-25, `cargo xtask stream --think`: 1.2–1.9 s
//!   silent in short turns), a run of Claude's under way, the turn's start before its first event, a request out with
//!   nothing of its answer yet (a tool's result gone back: 2.75 s measured before the next message's first event), a
//!   compaction. The cells wave (`wave`): each rises and falls, `WAVE` long, `APART` steps behind the one after it,
//!   whatever the stream does — a delta changes nothing. A turn's wave counts from its own start (`Band::from`): it
//!   rises from the floor as the turn starts, and again as it leaves `Still` (`woke`), never faster than the wave moves.
//! - `Pace::Still` — the stream stopped: no delta for `STILL_AFTER` and nothing else at work. The cells at the floor
//!   and still, at once; `◦ stuck?` as before (`Doing::Stuck`).
//!
//! A delta within `STILL_AFTER` waves unless something since said the stream paused for work (a thinking block
//! opening, a tool call, a request out — each lets the delta before it go, and waves for itself). At `calm` and `none`
//! the cells hold a still wave (`held`); still and the turn's own wave are drawn only where motion is
//! (`Editor.motion_drawn` — off in tests, where the cells wave on the tests' still clock).

use std::time::{Duration, Instant};

use crate::workspace::Workspace;

/// The working sign's heights, low to high: the old wave's (6b23970a's `term::wave`, the homepage's recording) — `▁`
/// never (it read as `_`); `▇` at the top, which alone in one cell read as the code's block cursor (the user,
/// 2026-10-27), among two lower cells reads as a wave's crest.
pub const CELLS: [char; 6] = ['▂', '▃', '▄', '▅', '▆', '▇'];

/// The cells the sign takes, wherever it shows, for the whole turn.
pub const WIDTH: usize = 3;

/// One cell's rise and fall, low to high and back (the old wave's).
pub const WAVE: Duration = Duration::from_secs(1);

/// How long each of the wave's frames stands: every other step of its cycle, as the homepage's recording shows it
/// (2026-10-25, a frame every ~0.2 s) — the user's pick (#277, 5b over 5's step a `TICK`): its redraws half of 5's.
pub const STEP: Duration = Duration::from_millis(200);

/// How many steps of its cycle a cell runs behind the next one (the old wave's: 3 of 10).
pub const APART: usize = 3;

/// The wave's step held where nothing moves on a clock (`calm`, `none`): `▂▅▆`, rising to the right.
pub const STILL: u128 = 10;

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

/// The turn's cells' heights now: the floor while still, else its wave from `Band::from` — None: no turn under way.
pub fn turn_heights(l: &Workspace) -> Option<[usize; WIDTH]> {
    Some(match pace(l)? {
        Pace::Still => [0; WIDTH],
        Pace::Breath => {
            let from = l.claude.chat.as_ref().map_or(0, |c| c.pace.from);
            wave(crate::editor::anim_step().saturating_sub(from))
        }
    })
}

/// The cells' heights `step` steps of the animation's clock (`editor::anim_step`, a `TICK` a step) after the wave
/// began: each a triangle from the floor to the top and back, `WAVE` long, `APART` of its steps behind the next, a frame
/// standing `STEP` — and none higher than the steps so far, so it rises from the floor as it begins, never faster than
/// the wave itself moves.
pub fn wave(step: u128) -> [usize; WIDTH] {
    let top = CELLS.len() - 1;
    let cycle = 2 * top;
    let tick = crate::editor::TICK.as_millis();
    let per = (WAVE.as_millis() / tick / cycle as u128).max(1);
    let hold = (STEP.as_millis() / tick / per).max(1);
    let at = step / per;
    let at = at - at % hold;
    std::array::from_fn(|k| {
        let p = ((at + (k * APART) as u128) % cycle as u128) as usize;
        (if p <= top { p } else { cycle - p }).min(at.min(top as u128) as usize)
    })
}

/// The cells drawn for `heights`.
pub fn cells(heights: [usize; WIDTH]) -> String {
    heights.iter().map(|&h| CELLS[h]).collect()
}

/// A mark standing where the sign stands (`▲` waiting on you, `◦` maybe stuck, a step's): in its last cell, by its
/// word, the ones before it blank — an indent before the mark, never a hole between it and its word — so the word stays
/// where it was (the user, #277).
pub fn mark(glyph: &str) -> String {
    use unicode_width::UnicodeWidthStr;
    format!("{}{glyph}", " ".repeat(WIDTH.saturating_sub(glyph.width())))
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
    fn the_wave_rises_from_the_floor_and_runs_as_the_old_one() {
        let period = WAVE.as_millis() / crate::editor::TICK.as_millis();
        let s = |step: u128| cells(wave(step));
        assert_eq!(s(0), "▂▂▂", "from the floor");
        // (the old wave's frames, 6b23970a's `term::wave`, once it has risen — every other one, each standing two ticks:
        // the homepage's)
        let old = |step: usize| -> String {
            (0..3).map(|k| ['▂', '▃', '▄', '▅', '▆', '▇', '▆', '▅', '▄', '▃'][(step + k * 3) % 10]).collect()
        };
        for step in 6..40 {
            assert_eq!(s(step as u128), old(step - step % 2), "step {step}");
        }
        assert_eq!(s(period), s(2 * period), "one period");
        assert_eq!(s(STILL), "▂▅▆", "the still wave");
        let changes = (0..period).filter(|&step| wave(step) != wave(step + 1)).count();
        assert_eq!(changes as u128, period / 2, "a frame every other tick");
        for step in 0..3 * period {
            let (a, b) = (wave(step), wave(step + 1));
            assert!((0..WIDTH).all(|k| a[k].abs_diff(b[k]) <= 2), "as the wave moves, at most: {step}");
        }
        assert_eq!(mark("▲"), "  ▲");
    }
}
