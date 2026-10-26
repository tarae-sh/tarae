//! What's cut is whole where you look at it (the user, 2026-10-25): a row cut to one line — a tool's row in the chat
//! (a note's, a reply's), the queued line — unfolds whole, wrapped, while it's the row in hand: the chat's view mode's
//! cursor on it, or the mouse over it; moving on folds it back. A run panel's command, cut to its tab's label in the
//! slot's line, doesn't unfold — that line is one row of structure, and the panel never shrinks mid-run — it shows
//! whole on a card over the line while the mouse is over it (`term::run_card`). What's kept is the whole text, never
//! the cut one (the transcript's `/` and `y` find and copy all of it, `K` says the command whole); one helper cuts and
//! wraps it, `cut::fit_in_hand`, so a fourth surface takes it the same way: its rows from the helper, in hand by
//! `held`, its spots recorded in `Screen.cut_spots` where it's drawn.

use crate::editor::Editor;
use crate::sizes::Slot;

/// A row that may be cut, by what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cut {
    /// A tool call's row in the chat (its call's id).
    Call(String),
    /// The chat's queued line — the message waiting for the turn's end.
    Queued,
    /// A run panel's command: its tab's label in the slot's line (whole on a card over it).
    Run(Slot),
    /// The status line's word of what Claude is doing now (`now.rs`): its step, sentence and plan's step whole on a
    /// card over it (`term::now_card`).
    Now,
}

/// What's in hand now — drawn whole: the row the mouse is over, the chat row its view mode's cursor is on. Worked out
/// once a frame by whoever lays rows out (`term::chat_laid`), never per row.
pub fn held(ed: &Editor) -> [Option<Cut>; 2] {
    [ed.cut_hover.clone(), crate::chatdoc::cursor_on(ed)]
}

/// The cut row at (`x`, `y`) on the frame last drawn.
fn under(ed: &Editor, x: usize, y: usize) -> Option<Cut> {
    ed.screen.cut_spots.borrow().iter().find(|s| s.0 == y && (s.1..s.2).contains(&x)).map(|s| s.3.clone())
}

/// The mouse at (`x`, `y`) (`Event::Hover`): over a cut row, it's in hand. Whether that changed — a move that changes
/// nothing draws nothing.
pub fn hovered(ed: &mut Editor, x: usize, y: usize) -> bool {
    let over = under(ed, x, y);
    let changed = over != ed.cut_hover;
    ed.cut_hover = over;
    changed
}

/// A frame drawn while the mouse holds a row in hand (`term::render_drawn`): still under it? What came, a scroll, a
/// run ending can take it from under a mouse that didn't move — then it folds. Whether it changed (drawn again).
pub fn drawn(ed: &mut Editor) -> bool {
    if ed.cut_hover.is_none() {
        return false;
    }
    let over = ed.presence.hover_at.and_then(|(x, y)| under(ed, x, y));
    let changed = over != ed.cut_hover;
    ed.cut_hover = over;
    changed
}

#[cfg(test)]
mod tests;
