//! What's cut is whole where you look at it — one way (the user, 2026-10-27, frictions #187): every `…` tarae draws —
//! a tool's row in the chat (a note's, a reply's), the queued line, the code's line ends (blame, a diagnostic), a list's
//! row — stays as it is (nothing unfolds, nothing moves) and shows whole on a borderless card over it (`term::cut_card`),
//! its first row on the cut one: while the mouse is over it, and while it's the row in hand (the chat's view mode's
//! cursor on it, a list's selected row while the list has the keys) — one card at a time, the mouse's first. What's kept
//! is the whole text, never the cut one (the transcript's `/` and `y` find and copy all of it). The one way in:
//! `cut_at` where it's drawn, which records it only when it was cut. Past `term::CUT_CARD_MAX` rows the card says
//! `↓ N more lines`, a press there opening it whole in a read-only buffer (`open`, Q's way for a quote).
//!
//! Three cards say more than the text (`term::run_card`, `now_card`, `with_card`): a run panel's command over its
//! slot's line, the status line's word of what Claude is doing, what goes with a message.

use crate::editor::Editor;
use crate::sizes::Slot;
use crate::theme::Style;

/// A row that may be cut, by what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cut {
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

/// A text cut where it's drawn: the whole of it, as it was drawn (`pieces`: byte ranges of it in their own colours — a
/// tool row's target, range and where it ran; none, `style` all of it), and the columns its card may take (`[from, to)`
/// — the code's pane for a line end, the chat's for its rows; the screen for a list's row, its card reaching right over
/// what's beside it). `state`: where a row's trailing state starts (the dim column after a list row's name, its gap
/// before it included) — kept whole on one row of the card, never split (the user, 2026-10-27).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Whole {
    pub on: On,
    pub text: String,
    pub style: Style,
    pub pieces: Vec<(std::ops::Range<usize>, Style)>,
    pub cols: (usize, usize),
    pub state: Option<usize>,
}

/// Where the row in hand's card goes — never over another row of its own list nor that list's frame (the user,
/// 2026-10-27, on U1's review). The mouse's card is always over its row: it's there only while you point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hand {
    /// Over its row, wrapped under it in its columns — the chat's view-mode cursor's row (the transcript isn't a list).
    Over,
    /// A list with what's beside it free (the side panel, the review rail — the code at their right): along its own
    /// row, on over what's beside it; the rest wrapped under it from `edge` (the list's right edge) rightwards only.
    Beside(usize),
    /// A list in a frame (x, y, width, height — a picker's: a card's choices, `:granted`; the prompt's or
    /// completion's card; a floating list): just outside the frame, below it, else above, in the free ground's
    /// columns there (`[from, to)`: a picker's ground left of the chat, a floating list's the screen — never just the
    /// frame's, the user, 2026-10-27), one row where the text fits them.
    Outside((usize, usize, usize, usize), (usize, usize)),
}

/// The card as drawn (`Screen.cut_card`): its cells (x, y, width, height) — its rows after the first from `from`
/// (beside a list: its edge, the list's rows left of it not the card's) —, its `↓ N more lines` row (a press there
/// opens it whole), and which cut it shows — the mouse over it keeps it. `under`: drawn under what floats (a docked
/// surface's row in hand), so a cell where something floats isn't its. `outside`: a framed list's row in hand, drawn
/// outside its frame (`Hand::Outside`) — a press on it is a press outside, as on the ground it covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub rect: (usize, usize, usize, usize),
    pub from: usize,
    pub under: bool,
    pub outside: bool,
    pub more: Option<usize>,
    pub cut: Cut,
}

impl Card {
    /// Whether the cell (`x`, `y`) is the card's as seen.
    fn holds(&self, ed: &Editor, x: usize, y: usize) -> bool {
        let (cx, cy, cw, ch) = self.rect;
        let mine = (cx..cx + cw).contains(&x) && (cy..cy + ch).contains(&y) && (y == cy || x >= self.from);
        mine && !(self.under && floats(ed.mouse_under(crate::event::MouseKind::Down, x, y).surface))
    }
}

/// What's drawn over the docked surfaces — a docked row in hand's card is under it (`term::cut_card`'s first pass).
fn floats(s: crate::mouse::Surface) -> bool {
    use crate::mouse::Surface as S;
    matches!(
        s,
        S::Pointing
            | S::Prompt
            | S::Picker
            | S::PickerPreview
            | S::Toast
            | S::Card
            | S::Float(_)
            | S::Floatlist(_)
            | S::Popup
            | S::FollowCard
            | S::WhichKey
            | S::Completion
            | S::NoteCard
    )
}

/// A press that landed on a cut text's card, until its release (`Editor.cut_press`): its drags and its release are the
/// row's press too — or, its act done (`↓ N more lines`), taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    Row(usize, usize),
    Taken,
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
    Transcript,
    Completion,
    Prompt,
}

impl On {
    /// Drawn with what's docked (its row in hand's card under what floats — `term::cut_card`'s first pass).
    pub(crate) fn docked(self) -> bool {
        matches!(self, On::Code | On::Side | On::Rail | On::Transcript)
    }

    pub(crate) fn under(self, s: crate::mouse::Surface) -> bool {
        use crate::mouse::Surface as S;
        matches!(
            (self, s),
            (On::Code, S::Code(_))
                | (On::Picker, S::Picker)
                | (On::Side, S::Side)
                | (On::Rail, S::Rail)
                | (On::List, S::Floatlist(_))
                | (On::Transcript, S::Transcript)
                | (On::Completion, S::Completion)
                | (On::Prompt, S::Prompt)
        )
    }

    /// What a buffer opened from its card is titled after (`whole · chat`).
    fn word(self) -> &'static str {
        match self {
            On::Code => "code",
            On::Picker => "picker",
            On::Side => "side panel",
            On::Rail => "review",
            On::List => "list",
            On::Transcript => "chat",
            On::Completion => "completion",
            On::Prompt => "prompt",
        }
    }
}

/// `shown` drawn on `on` at (`x`, `y`) for `whole`: recorded for the mouse when it was cut (else nothing — no card for a
/// text that's whole already), its card in `cols`; `hand`, it's the row in hand (a list's selected row while the list
/// has the keys), its card placed so — it stands while no other is under the mouse.
pub fn cut_at(
    ed: &Editor,
    on: On,
    (x, y): (usize, usize),
    (shown, whole): (&str, &str),
    style: Style,
    cols: (usize, usize),
    hand: Option<Hand>,
) {
    use unicode_width::UnicodeWidthStr;
    if shown == whole || shown.is_empty() {
        return;
    }
    let w = Whole { on, text: whole.to_string(), style, pieces: Vec::new(), cols, state: None };
    record(ed, (y, x, x + shown.width()), w, hand);
}

/// A cut text's spot (row, from, to) — its card's text starts at `from` — for one that worked out its whole itself (the
/// chat's rows: their pieces' colours).
pub fn record(ed: &Editor, (y, a, b): (usize, usize, usize), w: Whole, hand: Option<Hand>) {
    let mut spots = ed.screen.cut_spots.borrow_mut();
    if let Some(hand) = hand {
        ed.screen.cut_hand.set(Some((spots.len(), hand)));
    }
    spots.push((y, a, b, Cut::Text(Box::new(w))));
}

/// The cut whose card shows now, and where it goes if it's the row in hand: the mouse's, else this frame's row in hand
/// — only where its surface is what's at its cell (not under a picker, a card) and nothing else is under the mouse.
pub fn shown(ed: &Editor) -> Option<(Cut, Option<Hand>)> {
    let spots = ed.screen.cut_spots.borrow();
    let hand =
        ed.screen.cut_hand.get().and_then(|(i, h)| Some((spots.get(i)?, h))).filter(|(s, _)| match &s.3 {
            Cut::Text(w) => w.on.under(ed.mouse_under(crate::event::MouseKind::Down, s.1, s.0).surface),
            _ => false,
        });
    match &ed.cut_hover {
        Some(cut) => Some((cut.clone(), hand.filter(|(s, _)| s.3 == *cut).map(|h| h.1))),
        None => hand.map(|(s, h)| (s.3.clone(), Some(h))),
    }
}

/// A press at (`x`, `y`): on the card's `↓ N more lines`, its text whole in a read-only buffer (None: taken); elsewhere
/// on the card over its row, the press on the row it's about — it covers what's under it, and a press mustn't act on a
/// row out of sight; elsewhere on a card outside its list's frame, or off it, where it was — outside a picker, the
/// prompt or completion it closes them, nothing picked or accepted (frictions29 #187's review: a press under the
/// sandbox's blocked card chose a grant). Which it was is held until the release (`Editor.cut_press`: `held`).
pub fn pressed(ed: &mut Editor, x: usize, y: usize) -> Option<(usize, usize)> {
    ed.cut_press = None;
    let card = ed.screen.cut_card.borrow().clone();
    let Some(card) = card.filter(|c| c.holds(ed, x, y)) else { return Some((x, y)) };
    let spot = ed.screen.cut_spots.borrow().iter().find(|s| s.3 == card.cut).map(|s| (s.0, s.1, s.2));
    let Some((sy, a, b)) = spot else { return Some((x, y)) };
    if card.more == Some(y) {
        if let Cut::Text(w) = card.cut {
            open(ed, &format!("whole · {}", w.on.word()), &w.text, Vec::new());
        }
        // (its act done: the card goes, until the mouse moves onto a cut again)
        ed.cut_hover = None;
        ed.cut_press = Some(Held::Taken);
        return None;
    }
    if card.outside {
        return Some((x, y));
    }
    let row = (x.clamp(a, b.saturating_sub(1).max(a)), sy);
    ed.cut_press = Some(Held::Row(row.0, row.1));
    Some(row)
}

/// A drag or the release after a press on the card (`pressed`): the row's press goes on where it landed — a drag over the
/// card or out of it selects nothing under it, the release lands on that row — or, its act done, nothing (None). The
/// release lets it go. A drag or release with no press on the card: where it is.
pub fn held(ed: &mut Editor, x: usize, y: usize, up: bool) -> Option<(usize, usize)> {
    let held = ed.cut_press;
    if up {
        ed.cut_press = None;
    }
    match held {
        None => Some((x, y)),
        Some(Held::Row(x, y)) => Some((x, y)),
        Some(Held::Taken) => None,
    }
}

/// `text` whole in a read-only buffer titled `title`, the code with the keys — one buffer a title: the same text is the
/// one open (pressed again, it's shown); another takes its place.
pub fn open(ed: &mut Editor, title: &str, text: &str, paint: Vec<(usize, usize, u32)>) {
    let open = ed
        .docs
        .iter()
        .position(|d| d.throwaway && d.read_only.is_some() && d.title.as_deref() == Some(title));
    match open {
        Some(at) if ed.docs[at].text == text => ed.current = at,
        _ => {
            if let Some(at) = open {
                ed.close_doc(ed.docs[at].id);
            }
            let id = ed.open_virtual(title, text);
            if let Some(d) = ed.docs.iter_mut().find(|d| d.id == id) {
                d.paint = paint;
                d.read_only = Some(d.snapshot());
            }
        }
    }
    ed.workspace.viewer.at = crate::workspace::At::Code;
    ed.sync_focus_view();
}

/// The cut row at (`x`, `y`) on the frame last drawn — or its card, the mouse gone onto it.
fn under(ed: &Editor, x: usize, y: usize) -> Option<Cut> {
    if let Some(c) = ed.screen.cut_card.borrow().as_ref()
        && c.holds(ed, x, y)
    {
        return Some(c.cut.clone());
    }
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
