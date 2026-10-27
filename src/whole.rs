//! What's cut is whole where you look at it (the user, 2026-10-25): a row cut to one line — a tool's row in the chat
//! (a note's, a reply's), the queued line — unfolds whole, wrapped, while it's the row in hand: the chat's view mode's
//! cursor on it, or the mouse over it; moving on folds it back. A run panel's command, cut to its tab's label in the
//! slot's line, doesn't unfold — that line is one row of structure, and the panel never shrinks mid-run — it shows
//! whole on a card over the line while the mouse is over it (`term::run_card`). What's kept is the whole text, never
//! the cut one (the transcript's `/` and `y` find and copy all of it, `K` says the command whole); one helper cuts and
//! wraps it, `cut::fit_in_hand`, so a fourth surface takes it the same way: its rows from the helper, in hand by
//! `held`, its spots recorded in `Screen.cut_spots` where it's drawn.
//!
//! Every other `…` tarae draws (#178, #179: the code's line ends — blame, a diagnostic —, a list's row) is a text cut
//! where it stands: it doesn't unfold (nothing moves) — the mouse over it shows it whole on a borderless card over it,
//! its first row on the cut one (`term::cut_card`). The one way in: `cut_at` where it's drawn, which records it only
//! when it was cut.

use crate::editor::Editor;
use crate::sizes::Slot;
use crate::theme::Style;

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
    /// A text cut to `…` where it stands (`cut_at`): whole on a card over it (`term::cut_card`).
    Text(Box<Whole>),
    /// A chip over the chat's input of what goes with the message (#185) — the `n`th of them, or None: `N things`, all
    /// of them folded — what goes on a card over it (`term::with_card`).
    With(Option<usize>),
}

/// A text cut where it's drawn: the whole of it, as it was drawn, and the columns its card may take (`[from, to)` — the
/// code's pane for a line end; the screen for a list's row, its card reaching right over what's beside it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Whole {
    pub on: On,
    pub text: String,
    pub style: Style,
    pub cols: (usize, usize),
}

/// The surface a cut text is drawn on — in hand only where that surface is what's under the pointer (`mouse_under`), so
/// one under a picker, a card or a floating pane isn't.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum On {
    Code,
    Picker,
    Side,
    Rail,
    List,
}

impl On {
    pub(crate) fn under(self, s: crate::mouse::Surface) -> bool {
        use crate::mouse::Surface as S;
        matches!(
            (self, s),
            (On::Code, S::Code(_))
                | (On::Picker, S::Picker)
                | (On::Side, S::Side)
                | (On::Rail, S::Rail)
                | (On::List, S::Floatlist(_))
        )
    }
}

/// `shown` drawn on `on` at (`x`, `y`) for `whole`: recorded for the mouse when it was cut (else nothing — no card for a
/// text that's whole already), its card in `cols`.
pub fn cut_at(
    ed: &Editor,
    on: On,
    (x, y): (usize, usize),
    (shown, whole): (&str, &str),
    style: Style,
    cols: (usize, usize),
) {
    use unicode_width::UnicodeWidthStr;
    if shown == whole || shown.is_empty() {
        return;
    }
    let w = Whole { on, text: whole.to_string(), style, cols };
    ed.screen.cut_spots.borrow_mut().push((y, x, x + shown.width(), Cut::Text(Box::new(w))));
}

/// What's in hand now — drawn whole: the row the mouse is over, the chat row its view mode's cursor is on. Worked out
/// once a frame by whoever lays rows out (`term::chat_laid`), never per row.
pub fn held(ed: &Editor) -> [Option<Cut>; 2] {
    [ed.cut_hover.clone(), crate::chatdoc::cursor_on(ed)]
}

/// The cut row at (`x`, `y`) on the frame last drawn.
fn under(ed: &Editor, x: usize, y: usize) -> Option<Cut> {
    // (the last drawn first; a cut text only where what it's drawn on is what the pointer is over, a chip only where the
    // chat is)
    let mut surface = None;
    let mut at =
        || *surface.get_or_insert_with(|| ed.mouse_under(crate::event::MouseKind::Down, x, y).surface);
    let spots = ed.screen.cut_spots.borrow();
    let mut here = spots.iter().rev().filter(|s| s.0 == y && (s.1..s.2).contains(&x));
    here.find(|s| match &s.3 {
        Cut::Text(w) => w.on.under(at()),
        Cut::With(_) => at() == crate::mouse::Surface::Chat,
        _ => true,
    })
    .map(|s| s.3.clone())
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
