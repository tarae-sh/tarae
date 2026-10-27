//! What's cut is whole where you look at it — one way (the user, 2026-10-27, frictions #187): every `…` tarae draws —
//! a tool's row in the chat (a note's, a reply's), the queued line, the code's line ends (blame, a diagnostic), a list's
//! row — stays as it is (nothing unfolds, nothing moves) and shows whole on a borderless card over it (`term::cut_card`),
//! its first row on the cut one: while the mouse is over it, and while it's the row in hand (the chat's view mode's
//! cursor on it, a list's selected row while the list has the keys) — one card at a time, the mouse's first. What's kept
//! is the whole text, never the cut one (the transcript's `/` and `y` find and copy all of it). The one way in: a cut
//! carries its whole (`Fit`, a span's `cut`) to where it's printed, which records it there (`at`, `cut_at`) — only
//! when it was cut, never matched by its text (U2's review). Past `term::CUT_CARD_MAX` rows the card says
//! `↓ N more lines`, a press there opening it whole in a read-only buffer (`open`, Q's way for a quote).
//!
//! Three cards say more than the text (`term::run_card`, `now_card`, `with_card`): a run panel's command over its
//! slot's line, the status line's word of what Claude is doing, what goes with a message.

use crate::editor::Editor;
use crate::pane::GroupKind;
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
/// before it included) — kept whole on one row of the card, never split (the user, 2026-10-27). `float`: the floating pane
/// it's drawn in (`drawn_in`), None for what's docked or floats over the panes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Whole {
    pub on: On,
    pub text: String,
    pub style: Style,
    pub pieces: Vec<(std::ops::Range<usize>, Style)>,
    pub cols: (usize, usize),
    pub state: Option<usize>,
    pub float: Option<GroupKind>,
}

impl Whole {
    /// Whether it's what's seen at (`x`, `y`), `surface` what the pointer is over there: on that surface, and in the
    /// floating pane it's drawn in — the topmost there —, or, docked, under none (U2's third review: a docked line's cut
    /// under a floating pane, the pane's own surface one a docked line's could be, opened its card).
    pub(crate) fn seen(&self, ed: &Editor, surface: crate::mouse::Surface, (x, y): (usize, usize)) -> bool {
        let floating = || ed.float_at(x, y).map(|f| f.0).filter(|_| ed.picker.is_none());
        // (a frame's line: that frame the topmost drawn at the cell — a picker, which-key or the prompt drawn over a card
        // is what's seen there, whatever `mouse_under` answers first: U2's fourth review)
        let on_top = match self.on {
            On::Frame(_, rect) => {
                let frames = ed.screen.frames.borrow();
                let inside = |(fx, fy, fw, fh): (usize, usize, usize, usize)| {
                    (fx..fx + fw).contains(&x) && (fy..fy + fh).contains(&y)
                };
                frames.iter().rev().find(|f| inside(f.rect)).map(|f| f.rect) == Some(rect)
            }
            _ => true,
        };
        self.on.under(surface)
            && on_top
            && ((self.float.is_none() && !self.on.paned()) || floating() == self.float)
    }
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
    /// Each row's end, its first column past it — a row reaching on to the end of a word under it (`term::cut_card`).
    pub ends: Vec<usize>,
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
        let end = self.ends.get(y.wrapping_sub(cy)).copied().unwrap_or(cx + cw);
        let mine = (cx..end).contains(&x) && (cy..cy + ch).contains(&y) && (y == cy || x >= self.from);
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
    /// A floating list (the session list, `:messages`, `:folders`) — or a group floating: its list, its detail's own
    /// rows (the tests' note).
    List,
    Transcript,
    Completion,
    Prompt,
    /// A run panel docked: its lists' rows (`:procs`, the tests', a task's or preflight's steps), its own rows (what's
    /// queued, a sandbox's line), a plugin's panel.
    Slot,
    /// A slot's line, a pane's, the chat's top — every docked `top_line`.
    Top,
    /// A floating frame's top or bottom line (`term::draw_frame`): a card's, a picker's, the follow card's, which-key's,
    /// the prompt's — the surface the pointer is over on it and the frame's cells (x, y, width, height), so a cut there is
    /// seen only where that frame is the topmost drawn (`Whole::seen`; U2's fourth review: any floating surface opened it,
    /// another card over it too).
    Frame(crate::mouse::Surface, (usize, usize, usize, usize)),
    /// The chat's rows that aren't the transcript's: its intro, a chip, a run's wait, its input's placeholder.
    Chat,
    Status,
    Toast,
    /// A card: its title, a peek's, its rows.
    Card,
    /// The waiting card of `space i`'s ask (`term::draw_ask_stream`): its instruction. It has no surface of its own —
    /// the pointer over it is over its review not ready yet (`mouse_under`'s `EditReview`, outside), so it's seen
    /// there, floating over the panes (U2's fourth review: as `Card` it was never under the mouse).
    Ask,
    /// Claude's edit review in the buffer: its head.
    Review,
    WhichKey,
    Follow,
    Welcome,
    /// A picker's preview: its folder, its note.
    Preview,
}

impl On {
    /// Drawn with what's docked (its row in hand's card under what floats — `term::cut_card`'s first pass).
    pub(crate) fn docked(self) -> bool {
        matches!(self, On::Code | On::Side | On::Rail | On::Transcript | On::Slot)
    }

    /// A pane's — docked, or a floating pane's (`Whole.float`) —, so under a floating pane it's hidden; the rest floats
    /// over the panes (a picker, a card, a toast, the follow card — `mouse_under` answers for them first).
    fn paned(self) -> bool {
        matches!(
            self,
            On::Code
                | On::Side
                | On::Rail
                | On::List
                | On::Transcript
                | On::Slot
                | On::Top
                | On::Chat
                | On::Review
                | On::Welcome
        )
    }

    pub(crate) fn under(self, s: crate::mouse::Surface) -> bool {
        use crate::mouse::Surface as S;
        if let On::Frame(on, _) = self {
            return s == on;
        }
        matches!(
            (self, s),
            (On::Code, S::Code(_))
                | (On::Picker, S::Picker)
                | (On::Side, S::Side)
                | (On::Rail, S::Rail)
                | (On::List, S::Floatlist(_) | S::Float(_) | S::List(_) | S::Output(_))
                | (On::Transcript, S::Transcript)
                | (On::Completion, S::Completion)
                | (On::Prompt, S::Prompt)
                | (On::Slot, S::List(_) | S::Output(_) | S::SlotLine | S::Plugin | S::Floatlist(_))
                | (
                    On::Top,
                    S::PaneTop(_)
                        | S::SlotLine
                        | S::Chat
                        | S::Side
                        | S::Rail
                        | S::Float(_)
                        | S::Floatlist(_)
                        | S::List(_)
                        | S::Output(_)
                        | S::Plugin
                        | S::Edge
                )
                | (On::Chat, S::Chat | S::Transcript | S::Input)
                | (On::Status, S::Status)
                | (On::Toast, S::Toast)
                | (On::Card, S::Card)
                | (On::Ask, S::EditReview)
                | (On::Review, S::EditReview)
                | (On::WhichKey, S::WhichKey)
                | (On::Follow, S::FollowCard)
                | (On::Welcome, S::Welcome)
                | (On::Preview, S::PickerPreview)
        )
    }

    /// What a buffer opened from its card is titled after (`whole · chat`).
    fn word(self) -> &'static str {
        match self {
            On::Code => "code",
            On::Picker => "picker",
            On::Side => "side panel",
            On::Rail | On::Review => "review",
            On::List => "list",
            On::Transcript | On::Chat => "chat",
            On::Completion => "completion",
            On::Prompt => "prompt",
            On::Slot => "run",
            On::Top | On::Frame(..) => "line",
            On::Status => "status line",
            On::Toast => "message",
            On::Card => "card",
            On::Ask => "ask",
            On::WhichKey => "keys",
            On::Follow => "follow",
            On::Welcome => "start",
            On::Preview => "preview",
        }
    }
}

thread_local! {
    /// The cuts `at` placed, until `settle` hands them to the frame's spots (the printers that place them — a top line,
    /// a card's rows — don't hold the editor).
    static PLACED: std::cell::RefCell<Vec<Placed>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The floating pane being drawn (`Within`), what's printed now drawn in it.
    static IN: std::cell::Cell<Option<GroupKind>> = const { std::cell::Cell::new(None) };
}

/// While it lives, what's printed is drawn in the floating pane `kind` (`term::draw_floating`): a cut recorded then is
/// seen only where that pane is the topmost floating one (`Whole::seen`).
pub struct Within(Option<GroupKind>);

impl Within {
    pub fn new(kind: GroupKind) -> Within {
        Within(IN.replace(Some(kind)))
    }
}

impl Drop for Within {
    fn drop(&mut self) {
        IN.set(self.0);
    }
}

/// The floating pane what's printed now is drawn in (`Within`) — None: docked, or over the panes.
pub fn drawn_in() -> Option<GroupKind> {
    IN.get()
}

/// A cut placed where it was printed: its spot (row, from, to), what it is, whether it's the row in hand.
type Placed = ((usize, usize, usize), Whole, Option<Hand>);

/// A text cut to its cells, carrying the whole it was cut from (U2's review, 2026-10-28): through whatever holds it — a
/// span (`markdown::Span.cut`: a chat row, cached with its message), a status line's piece, a top line's name — to
/// where it's printed, which records it there (`at`, `term::chat_spans_at`); never matched by its text. Its text leaves
/// only by `shown` (no `Display`): a site that cuts says where the cut goes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fit {
    text: String,
    cut: Option<Of>,
}

/// Where a text's cut part is (its bytes `[from, to)`) and the whole it was cut from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Of {
    pub from: usize,
    pub to: usize,
    pub whole: String,
}

impl Fit {
    /// A text that isn't cut.
    pub fn plain(text: impl Into<String>) -> Fit {
        Fit { text: text.into(), cut: None }
    }

    pub fn shown(&self) -> &str {
        &self.text
    }

    pub fn width(&self) -> usize {
        unicode_width::UnicodeWidthStr::width(self.text.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn is_cut(&self) -> bool {
        self.cut.is_some()
    }

    /// `lead` before it (its cut moved along).
    pub fn lead(mut self, lead: &str) -> Fit {
        self.text.insert_str(0, lead);
        if let Some(of) = &mut self.cut {
            (of.from, of.to) = (of.from + lead.len(), of.to + lead.len());
        }
        self
    }

    /// `tail` after it.
    pub fn then(mut self, tail: &str) -> Fit {
        self.text.push_str(tail);
        self
    }

    /// It in two at byte `at` (a char boundary), each part carrying the cut of what it holds.
    pub fn split_at(self, at: usize) -> (Fit, Fit) {
        let (a, b) = self.text.split_at(at);
        let part = |text: &str, lo: usize| {
            let cut = self.cut.as_ref().and_then(|of| {
                let (from, to) = (of.from.max(lo) - lo, of.to.min(lo + text.len()).saturating_sub(lo));
                (from < to).then(|| Of { from, to, whole: of.whole.clone() })
            });
            Fit { text: text.to_string(), cut }
        };
        (part(a, 0), part(b, at))
    }

    /// It as a span in `style`, its cut carried.
    pub fn span(self, style: Style) -> crate::markdown::Span {
        crate::markdown::Span { text: self.text, style, cut: self.cut.map(Box::new) }
    }

    /// Its text and its cut apart (a holder of its own: a status line's piece).
    pub fn into_parts(self) -> (String, Option<Of>) {
        (self.text, self.cut)
    }

    /// Its text and its cut together again.
    pub fn from_parts(text: String, cut: Option<Of>) -> Fit {
        Fit { text, cut }
    }

    /// It cut again to `width` cells — its whole kept, or itself the whole where it wasn't cut.
    pub fn fit(&self, width: usize) -> Fit {
        self.again(crate::cut::fit_ellipsis(&self.text, width))
    }

    /// `shown`, its text cut again (`fit_ellipsis`: a start of it and `…`): the cut from where it was cut, or from this
    /// cut's `…` where that comes first — the whole widened by what's cut this time before the old cut (its lead) and
    /// after it (its tail), so the card says all the cut cells stood for (U2's third review: a lead cut into kept the old
    /// cut's start, inside the new `…`, and was placed nowhere).
    fn again(&self, shown: String) -> Fit {
        if shown == self.text {
            return self.clone();
        }
        let Some(was) = &self.cut else { return of(shown, &self.text) };
        let Some(kept) = shown.strip_suffix('…').filter(|k| self.text.starts_with(k)) else {
            // (nothing left to draw: no cut to see)
            return Fit::plain(shown);
        };
        let from = was.from.min(kept.len());
        let (lead, tail) =
            (self.text.get(from..was.from).unwrap_or(""), self.text.get(was.to..).unwrap_or(""));
        let whole = format!("{lead}{}{tail}", was.whole);
        Fit { cut: Some(Of { from, to: shown.len(), whole }), text: shown }
    }
}

impl From<String> for Fit {
    fn from(text: String) -> Fit {
        Fit::plain(text)
    }
}

impl From<&str> for Fit {
    fn from(text: &str) -> Fit {
        Fit::plain(text)
    }
}

impl From<&String> for Fit {
    fn from(text: &String) -> Fit {
        Fit::plain(text.as_str())
    }
}

impl From<&Fit> for Fit {
    fn from(fit: &Fit) -> Fit {
        fit.clone()
    }
}

/// `text` in `width` cells, `…` at its end where it's cut — the one way `term.rs` cuts a text (with `fit_left`): the
/// cut carries its whole to its print (`at`), which records it for the mouse (`every_cut_tarae_draws_is_recorded`).
pub fn fit(text: &str, width: usize) -> Fit {
    of(crate::cut::fit_ellipsis(text, width), text)
}

/// `fit`, its end kept (`…/editor/mod.rs`).
pub fn fit_left(text: &str, width: usize) -> Fit {
    of(crate::cut::fit_left(text, width), text)
}

/// `fit` for a part of `whole` (a message's first line, its third row): its card says all of it — cut or not, a part.
pub fn fit_part(text: &str, width: usize, whole: &str) -> Fit {
    of(crate::cut::fit_ellipsis(text, width), whole)
}

/// `fit` for a text whose cut is recorded another way — a card of its own (a run's command: `Cut::Run`; the now line),
/// or painted again over a cut recorded where it was printed (a lit word). A plain string, nothing carried: each use is
/// named with its why, a count a drawer that only goes down (`tests::OWN`).
pub fn fit_own(text: &str, width: usize) -> String {
    crate::cut::fit_ellipsis(text, width)
}

/// `shown`, a cut of `whole` made another way (a list row's middle) — or `whole` itself, not cut.
pub(crate) fn of(shown: String, whole: &str) -> Fit {
    let cut =
        (shown != whole && !shown.is_empty()).then(|| Of { from: 0, to: shown.len(), whole: whole.into() });
    Fit { text: shown, cut }
}

/// `fit` put on `on` at (`x`, `y`) in `style`: its cut, if any, recorded where it stands, its card in `cols`.
pub fn at(on: On, fit: &Fit, (x, y): (usize, usize), style: Style, cols: (usize, usize)) {
    at_in_hand(on, fit, (x, y), style, cols, None);
}

/// `at` for the row in hand (a list's selected row while its list has the keys): its card stands while the mouse is on
/// no other, placed by `hand`.
pub fn at_in_hand(
    on: On,
    fit: &Fit,
    (x, y): (usize, usize),
    style: Style,
    cols: (usize, usize),
    hand: Option<Hand>,
) {
    placed(on, (&fit.text, fit.cut.as_ref()), (x, y), style, cols, hand);
}

/// `printed` put on `on` at (`x`, `y`) in `style` with the cut it carries (`cut`, a span's): recorded where its cut part
/// stands, its card in `cols`.
pub fn placed(
    on: On,
    (printed, cut): (&str, Option<&Of>),
    (x, y): (usize, usize),
    style: Style,
    cols: (usize, usize),
    hand: Option<Hand>,
) {
    let Some((of, (a, b))) = cut.and_then(|of| Some((of, part_at(printed, of, x)?))) else { return };
    let w =
        Whole { on, text: of.whole.clone(), style, pieces: Vec::new(), cols, state: None, float: drawn_in() };
    PLACED.with_borrow_mut(|p| p.push(((y, a, b), w, hand)));
}

/// The columns of `printed`'s cut part `of`, printed from column `x` — None where nothing of it is drawn.
fn part_at(printed: &str, of: &Of, x: usize) -> Option<(usize, usize)> {
    use unicode_width::UnicodeWidthStr;
    // (printed shorter than it was cut — cut again where it's drawn: up to its end)
    let to = of.to.min(printed.len());
    let (before, part) = (printed.get(..of.from)?, printed.get(of.from..to)?);
    let a = x + before.width();
    (!part.is_empty()).then(|| (a, a + part.width()))
}

/// A new frame: nothing placed yet.
pub fn begin() {
    PLACED.with_borrow_mut(Vec::clear);
}

/// The cuts placed since the last settle, into the frame's spots — before a card is drawn from them (`term::cut_card`).
pub fn settle(ed: &Editor) {
    for (spot, w, hand) in PLACED.take() {
        record(ed, spot, w, hand);
    }
}

/// `at_in_hand`, recorded at once (a printer that holds the editor): `fit` drawn on `on` at (`x`, `y`) — its cut, if any
/// (else nothing: no card for a text that's whole already), recorded where it stands, its card in `cols`; `hand`, it's
/// the row in hand (a list's selected row while the list has the keys), its card placed so — it stands while no other is
/// under the mouse.
pub fn cut_at(
    ed: &Editor,
    on: On,
    (x, y): (usize, usize),
    fit: &Fit,
    style: Style,
    cols: (usize, usize),
    hand: Option<Hand>,
) {
    let Some((of, (a, b))) = fit.cut.as_ref().and_then(|of| Some((of, part_at(&fit.text, of, x)?))) else {
        return;
    };
    let float = drawn_in();
    let w = Whole { on, text: of.whole.clone(), style, pieces: Vec::new(), cols, state: None, float };
    record(ed, (y, a, b), w, hand);
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
            Cut::Text(w) => {
                w.seen(ed, ed.mouse_under(crate::event::MouseKind::Down, s.1, s.0).surface, (s.1, s.0))
            }
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
        Cut::Text(w) => w.seen(ed, at(), (x, y)),
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
