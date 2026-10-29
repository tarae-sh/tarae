//! The mouse's rules every surface shares (`.sb/mouse-plan.md`; CLAUDE.md, "The mouse, one way on every surface"): a
//! press gives the keys to the surface it lands on. Before any surface takes a press, what holds the keys over them all
//! lets go: a prompt or a picker open closes as `esc` closes it (a press on it is its own), and a card focused gives
//! its keys back as `esc` does (it stays, waiting) — then the press acts where it landed.
//!
//! What's under the pointer is one truth (M0, §3): `Editor::mouse_under` — `handle_mouse`'s precedence as data, read
//! from `Screen`'s records — and `handle_mouse` dispatches on its answer, each branch saying it took the event
//! (`took_mouse`, the routing test's). What each act does on each surface is one table, `expect` — `docs/mouse.md` is
//! printed from it — and the walk (`tests::no_press_is_lost_in_silence`) holds every scene to it.

use crate::editor::{Drag, Editor};
use crate::event::MouseKind;
use crate::float::Grip;
use crate::pane::GroupKind;
use crate::selection::{Held, Selection, Unit};
use crate::sizes::{Border, Slot};
use crate::split::PaneId;
use crate::workspace::Spot;

fn inside((x0, y0, w, h): (usize, usize, usize, usize), x: usize, y: usize) -> bool {
    (x0..x0 + w).contains(&x) && (y0..y0 + h).contains(&y)
}

/// On a span of row `row`, columns `from..to`.
fn on(row: usize, from: usize, to: usize, x: usize, y: usize) -> bool {
    row == y && (from..to).contains(&x)
}

/// What's under the pointer: the surface, and the part of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Under {
    pub surface: Surface,
    pub part: Part,
}

/// Every surface the mouse lands on (the plan's §1 rows), in `mouse_under`'s order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// Pointing's labels up (`C-o` in the chat, `space @`): a press picks the element under it, or stops.
    Pointing,
    /// The prompt's card (`:`, its completions) — and, a press outside it, its `Outside`.
    Prompt,
    /// A picker, the palette: its list (and a press outside it, `Outside`).
    Picker,
    /// A picker's preview.
    PickerPreview,
    /// The notification card.
    Toast,
    /// A plugin's overlay (`plugin/overlay.rs`): a press is its plugin's (`pressed`, the row from 0) — news,
    /// never the keys; the wheel and a hover pass to what's under it.
    Overlay,
    /// The front card and its stack's rows, the grammar download's buttons.
    Card,
    /// The ask's waiting card (`space i`, a commit draft — `term::draw_ask_stream`): a press gives the keys to where its
    /// one key is typed, the code (its `esc stop` — `llm::esc_stops`), as a press focuses a card that waits.
    Ask,
    /// A line between two things with sizes (`Part::Border`).
    Edge,
    /// A floating pane's frame and what's in it but a list's rows.
    Float(GroupKind),
    /// A floating list (the session list, `:messages`) — or one docked.
    Floatlist(GroupKind),
    /// The status line.
    Status,
    /// The bottom slot's line: its tabs, a group's members' names.
    SlotLine,
    /// Hover text, docs (`K`, `space g p`) — the wheel's only.
    Popup,
    /// The code review's file rail.
    Rail,
    /// The terminal, the program's.
    Terminal,
    /// The terminal's view mode.
    TerminalView,
    /// A run panel's output: the tests' detail, the task's, a preflight's, `:procs`' detail.
    Output(Slot),
    /// A linked group's list: the tests', a task's places, a preflight's steps, `:procs`'.
    List(GroupKind),
    /// A plugin's panel.
    Plugin,
    /// The side panel (files, outline, list, git, a plugin's tab).
    Side,
    /// The follow card, minimized chat: its trail rows, its strip's marks.
    FollowCard,
    /// The chat's top line, its gaps and edge cells.
    Chat,
    /// The chat's conversation.
    Transcript,
    /// The chat's input.
    Input,
    /// Claude's edit review in the buffer — and, a press off it while it's up, `Outside`.
    EditReview,
    /// The start screen.
    Welcome,
    /// A code pane's top line (its path, buffers).
    PaneTop(PaneId),
    /// A code pane's text (files, pages, a woven view, the verdict).
    Code(PaneId),
    /// Which-key: a row presses its key (`Part::Row`: its index in `Screen.which_key_rows`).
    WhichKey,
    /// The completion menu and its docs: a row accepts its item (`Part::Row`: its index among those shown).
    Completion,
    /// The note card: a press replies (`space n`'s).
    NoteCard,
    /// Nothing drawn there that takes the mouse.
    Nothing,
}

/// The part of a surface under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// A character of text: its line (a document's, a run's output row, the conversation's row) and its column — the
    /// code's read past the hints its pane drew before it on that row (`Screen::text_x`, #150).
    Text {
        line: usize,
        col: usize,
    },
    /// A row of a list (its index, as the surface counts them; a toast's: the screen row).
    Row(usize),
    /// A word on a line that does its key (motion's W builds on it).
    Word(Word),
    /// A card's answer, by its key.
    Answer(char),
    /// A place in the chat that goes somewhere.
    Link(Link),
    /// The rail's viewed mark of file `i`.
    Mark(usize),
    /// The rail's `↑/↓ N more`: rows to scroll by.
    More(isize),
    Border(Border),
    /// A floating frame's top line (it moves) or edges (they size it).
    Frame(Grip),
    /// Outside the surface that holds the keys over every other (a prompt, a picker, an edit's review).
    Outside,
    /// The rest of it.
    Blank,
}

/// A word on a line that does its key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Word {
    /// The status line's word of what Claude is doing now (`now::press`).
    Now,
    /// A session's tab, the dropdown, the status line's chip and counts, a presence tag.
    Session(Spot),
    /// A bottom slot's tab.
    Tab(Slot),
    /// A group member's name on the slot's line.
    Member(PaneId),
    /// A preflight's step chip (its index).
    Step(usize),
    /// A mark of the follow card's strip (its index in `Screen.stop_marks`).
    Stop(usize),
    /// A word that names a state, a key hint's pair: its key (its index in `Screen.word_spots` — `wordspot.rs`).
    Does(usize),
    /// A chip over the chat's input, of what goes with the message (`whole::Cut::With`: the `n`th, or None for `N
    /// things`): what goes, on a card over it; pressed, it whole (`quote::pressed`).
    With(Option<usize>),
}

/// A place in the chat that goes somewhere, by its index in its record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// Where a command ran, at its row's end (`Screen.chat_ran`).
    Ran(usize),
    /// A UI element's id in an answer (`Screen.chat_ids`).
    Id(usize),
    /// A link, a tool call's row, a trail row (`Screen.chat_links`).
    Chat(usize),
}

/// The surface under the pointer, whose lines light half while it's there (zellij's `mouse_hover_effects` —
/// CLAUDE.md, the mouse's bullet): the lines the keys light where it has them (`Viewer.at`'s) — a code pane, the side
/// panel or the rail, the chat, the bottom slot, a group docked or floating — and the floating frames that wait for
/// them (#189): the front card's, the follow card's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lights {
    Code(PaneId),
    Side,
    Chat,
    Slot,
    Dock(GroupKind),
    /// The front card's frame (its peeks above it have none).
    Card,
    Follow,
}

/// How a floating frame's lines are lit (#189): the faint wash, the accent half over it while the pointer is on it, the
/// accent while it has the keys — one rule for every frame (`term::draw_frame`, a floating pane's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lit {
    Wash,
    Half,
    Keys,
}

/// A floating frame as drawn (`Screen.frames`): where, the surface whose pointer lights it (None: one that's up only
/// while it has the keys — a picker, a prompt, which-key) and how it was lit — the walk reads it, the hover finds the
/// front card's there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Framed {
    pub rect: (usize, usize, usize, usize),
    pub of: Option<Lights>,
    pub lit: Lit,
}

impl Under {
    fn new(surface: Surface, part: Part) -> Under {
        Under { surface, part }
    }

    /// Whether `handle_mouse` reaches the terminal's press rule for it (a press elsewhere ends its selection and takes
    /// the keys from the program): everything past the overlays, the held drags, the lines' words and the rail.
    pub fn reaches_terminal(&self) -> bool {
        use Surface as S;
        let early = matches!(
            self.surface,
            S::Pointing
                | S::Prompt
                | S::Picker
                | S::PickerPreview
                | S::Toast
                | S::Overlay
                | S::Card
                | S::Edge
                | S::Floatlist(_)
                | S::Status
                | S::SlotLine
                | S::Popup
                | S::Rail
                | S::WhichKey
                | S::Completion
                | S::NoteCard
        );
        let word = matches!(
            self.part,
            Part::Frame(_)
                | Part::Word(
                    Word::Now
                        | Word::Session(_)
                        | Word::Tab(_)
                        | Word::Member(_)
                        | Word::Does(_)
                        | Word::With(_)
                )
        );
        !early && !word
    }
}

/// A card's way in pressed (`Part::Answer`): `C-q answer` while it waits — focused, as a press anywhere on it; once
/// focused `esc back` in its place — the keys back, as `esc` (#200: every pair on its bottom line does its key).
pub const WAY: char = '\u{1b}';

/// What a press on the prompt's card off its line and its completions says.
pub const PROMPT_ROWS: &str = "a completion's row fills it in — enter runs the line · esc close";
/// What the middle button says where there's no text to paste into.
pub const MIDDLE_ELSEWHERE: &str =
    "the middle button pastes the clipboard into text — the code, the chat's input, a prompt's line";
/// What a drag from pointing's press says (`Editor.drag_says`).
pub const POINTING_DRAG: &str = "pointing takes a press — a drag picks nothing";
/// What a press says while `C-w m` moves a floating pane (its top line still drags it).
pub const MOVING_PRESS: &str = "moving this pane — h j k l move it, enter done, its top line drags it";
/// What a drag from a card says (Q11: a card is a question, not a document).
pub const CARD_DRAG: &str = "a card's text isn't selected — v shows it whole, to copy";
/// What a drag from the ask's waiting card says: what it writes comes to the buffer, reviewed there.
pub const ASK_DRAG: &str = "the ask's card isn't selected — its change comes to the buffer, reviewed there";
/// What a press or a drag off an edit's review says while it waits.
pub const REVIEW_OFF: &str = "the review waits for its answer — y accepts the change, n rejects it";
/// What a drag from a word that does its key says, a key hint's or a name's, wherever it's drawn: it's a button, a drag
/// starts beside it — text selects, a line sizes (piece B).
pub const HINT_DRAG: &str = "this takes a press — a drag starts beside it";
/// What a drag on the start screen says.
pub const WELCOME_DRAG: &str = "the start screen has nothing to select — a press on a row presses its keys";

thread_local!(pub static TOOK: std::cell::Cell<Option<Surface>> = const { std::cell::Cell::new(None) });

#[cfg(test)]
thread_local! {
    /// `mouse_under` asked since the test set it to 0 — a work budget's count
    /// (`term::tests::a_row_under_the_pointer_reads_no_frame_again`).
    pub static ASKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `handle_mouse`'s branch for `s` took the event — the routing test reads it.
#[inline]
pub fn took_mouse(s: Surface) {
    TOOK.set(Some(s));
}

impl Editor {
    /// What a press at (`x`, `y`) lands on — `mouse_under`'s answer for a press.
    #[cfg(test)]
    pub fn mouse_at(&self, x: usize, y: usize) -> Under {
        self.mouse_under(MouseKind::Down, x, y)
    }

    /// What a mouse event of kind `kind` at (`x`, `y`) goes to, as the last frame drew the screen — `handle_mouse`'s
    /// precedence, first match wins: pointing; the prompt; a press outside a prompt or picker; the notification card
    /// and the cards (a press); what a drag holds (a border, a frame, the chat's text, the terminal's); floating frames
    /// and lists; the lines' borders and words (a press); the picker; hover text (the wheel); the rail; the bottom slot's
    /// panels; the side panel; the follow card; the chat; Claude's edit review; the start screen; the code. The act
    /// matters where today's rules make it: a press closes what's open, a drag goes where its press was, the wheel
    /// alone scrolls hover text.
    pub fn mouse_under(&self, kind: MouseKind, x: usize, y: usize) -> Under {
        #[cfg(test)]
        ASKED.set(ASKED.get() + 1);
        use MouseKind as K;
        use Surface as S;
        let s = &self.screen;
        let at = Under::new;
        let down = kind == K::Down;
        let wheel = matches!(kind, K::ScrollUp | K::ScrollDown);
        // (a press whose drag does nothing: its drag and release that press's — `Editor.drag_says`)
        if matches!(kind, K::Drag | K::Up)
            && let Some((u, ..)) = self.drag_says
        {
            return u;
        }
        if self.pointing.is_some() {
            return at(S::Pointing, Part::Blank);
        }
        // (a press on the prompt's line dragged: its line's wherever the pointer goes)
        let prompt_text = |x: usize| {
            let line = s.prompt_input.get();
            line.map_or(Part::Blank, |d| Part::Text { line: 0, col: x.saturating_sub(d.x) })
        };
        if matches!(kind, K::Drag | K::Up) && matches!(self.mouse_anchor, Some((Drag::Prompt, _))) {
            return at(S::Prompt, prompt_text(x));
        }
        // (and a press on a picker's query: its line's too)
        let query_text = |x: usize| {
            let line = s.picker_input.get();
            line.map_or(Part::Blank, |d| Part::Text { line: 0, col: x.saturating_sub(d.x) })
        };
        if matches!(kind, K::Drag | K::Up) && matches!(self.mouse_anchor, Some((Drag::Picker, _))) {
            return at(S::Picker, query_text(x));
        }
        // (a key hint that presses its key on what holds every press — the status line's keys of a prompt or a picker, the
        // note card's, the ask's, a floating frame's lines': its key, never the press that closes it or a drag of the frame)
        let hint_on = |surface: Surface| {
            let spots = s.word_spots.borrow();
            spots.iter().rposition(|w| w.on == surface && on(w.y, w.from, w.to, x, y))
        };
        if down
            && (self.prompt.is_some() || self.picker.is_some())
            && s.height == Some(y)
            && let Some(i) = hint_on(S::Status)
        {
            return at(S::Status, Part::Word(Word::Does(i)));
        }
        // The prompt's card: a completion row, its line where it was drawn (#153); what's under it isn't pressed through
        // it
        if let Some(card) = s.prompt_card.get()
            && inside(card, x, y)
        {
            let row = s.prompt_rows.borrow().iter().find(|r| r.0 == y).map(|r| r.1);
            let part = match row {
                Some(i) => Part::Row(i),
                None if s.prompt_input.get().is_some_and(|d| d.y == y) => prompt_text(x),
                None => Part::Blank,
            };
            return at(S::Prompt, part);
        }
        // The note card: a press is its own, while its reply's line is open too — the wheel too, nothing under it (another
        // prompt open: a press there is outside that prompt, which closes it)
        let replying =
            matches!(self.prompt.as_ref().map(|p| p.kind), Some(crate::editor::PromptKind::Note(_)));
        if (down || wheel)
            && (self.prompt.is_none() || replying)
            && s.note_card.get().is_some_and(|r| inside(r, x, y))
        {
            let hint = hint_on(S::NoteCard).filter(|_| down);
            return at(S::NoteCard, hint.map_or(Part::Blank, |i| Part::Word(Word::Does(i))));
        }
        if down && self.prompt.is_some() {
            return at(S::Prompt, Part::Outside);
        }
        if down && self.picker.is_some() && !s.picker_box.get().is_some_and(|b| inside(b, x, y)) {
            return at(S::Picker, Part::Outside);
        }
        let way = s.offer_way.filter(|w| on(w.0, w.1, w.2, x, y)).map(|_| WAY);
        let button = s.offer_buttons.iter().find(|b| on(b.0, b.1, b.2, x, y)).map(|b| b.3).or(way);
        let on_card = s.offer_card.is_some_and(|c| inside(c, x, y));
        let on_toast = s.toast_card.get().is_some_and(|t| inside(t, x, y));
        if down && on_toast && !on_card && button.is_none() && self.picker.is_none() {
            return at(S::Toast, Part::Row(y));
        }
        if down && !on_card && button.is_none() && self.picker.is_none() {
            let drawn = s.overlays.borrow();
            if let Some((_, (_, oy, ..))) = drawn.iter().rev().find(|o| inside(o.1, x, y)) {
                return at(S::Overlay, Part::Row(y - oy));
            }
        }
        if down && (button.is_some() || on_card) {
            return at(S::Card, button.map_or(Part::Blank, Part::Answer));
        }
        // (the wheel over a card: its body, cut to fit, scrolls)
        if wheel && on_card {
            return at(S::Card, Part::Blank);
        }
        // Which-key and the completion menu: a row presses its key, accepts its item — the wheel theirs, nothing under
        // them pressed through them
        if down || wheel {
            let rows = s.which_key_rows.borrow();
            if let Some(i) = rows.iter().position(|r| on(r.0, r.1, r.2, x, y)) {
                return at(S::WhichKey, Part::Row(i));
            }
            if s.which_key.get().is_some_and(|r| inside(r, x, y)) {
                return at(S::WhichKey, Part::Blank);
            }
            if let Some(card) = s.completion_card.get()
                && inside(card, x, y)
            {
                let row = s.completion_rows.borrow().iter().find(|r| r.0 == y).map(|r| r.1);
                return at(S::Completion, row.map_or(Part::Blank, Part::Row));
            }
            if s.completion_docs.get().is_some_and(|r| inside(r, x, y)) {
                return at(S::Completion, Part::Blank);
            }
        }
        // What a drag holds, wherever the pointer goes (a press starts afresh: one held whose release was lost lets go)
        if let Some(b) = self.dragging.filter(|_| !down) {
            return at(S::Edge, Part::Border(b));
        }
        if let Some((kind, grip)) = self.float_dragging.filter(|_| !down) {
            return at(S::Float(kind), Part::Frame(grip));
        }
        // (a name pressed on a line nothing drags: its drag and release that line's)
        if matches!(kind, K::Drag | K::Up) && self.name_held.is_some() {
            return at(S::SlotLine, Part::Blank);
        }
        if matches!(kind, K::Drag | K::Up) {
            // (a drag from text is that text's wherever the pointer goes — past its edge, its row there: it scrolls,
            // `edge`)
            let rows = |y0: usize, n: usize| y.clamp(y0, y0 + n.max(1) - 1);
            match self.mouse_anchor {
                Some((Drag::Transcript, _)) => {
                    let y = s.chat_view.get().map_or(y, |(y0, _, n, _)| rows(y0, n));
                    return at(S::Transcript, self.transcript_text(x, y));
                }
                Some((Drag::Input, _)) => return at(S::Input, self.input_text(x, y)),
                Some((Drag::Review, _)) => return at(S::EditReview, Part::Blank),
                Some((Drag::Code, _)) => {
                    let pid = self.workspace.viewer.focus;
                    if let Some(part) = self.code_text(pid, x, y, true) {
                        return at(S::Code(pid), part);
                    }
                }
                Some((Drag::Output(slot), _)) => {
                    if let Some(r) = s.run_rows.of(slot) {
                        let line = rows(r.1, r.3) - r.1;
                        return at(S::Output(slot), Part::Text { line, col: x.saturating_sub(r.2) });
                    }
                }
                _ => {}
            }
            // (a program that has the mouse always gets its drag and release, clamped to its edge)
            if self.mouse_in_terminal {
                return at(self.terminal_surface(), self.terminal_cell(x, y, true));
            }
        }
        // The ask's waiting card: over the panes, the floating ones too — under what's drawn after it (the follow card,
        // hover text, a picker); nothing under it pressed through it
        if let Some(card) = s.ask_card.get()
            && inside(card, x, y)
            && self.picker.is_none()
            && !s.thought_card.get().or(s.chat_mini.get()).is_some_and(|r| inside(r, x, y))
            && !(wheel && self.popup.is_some() && s.popup.get().is_some_and(|r| inside(r, x, y)))
        {
            let hint = hint_on(S::Ask).filter(|_| down);
            return at(S::Ask, hint.map_or(Part::Blank, |i| Part::Word(Word::Does(i))));
        }
        // Floating panes: the topmost there takes it, nothing under it pressed through it
        let floated = self.float_at(x, y).map(|f| f.0).filter(|_| self.picker.is_none());
        // (a floating group's header is the slot's drawn in its frame: its keys there, on its top line)
        let header = |kind: GroupKind| {
            let top = self.float_at(x, y).filter(|f| f.1.y == y).map(|f| f.0) == Some(kind);
            hint_on(S::Float(kind)).or_else(|| hint_on(S::SlotLine).filter(|_| top))
        };
        if down
            && let Some(kind) = floated
            && let Some(i) = header(kind)
        {
            return at(S::Float(kind), Part::Word(Word::Does(i)));
        }
        if down
            && floated.is_some()
            && let Some((kind, grip)) = self.float_grip(x, y)
        {
            return at(S::Float(kind), Part::Frame(grip));
        }
        let docked_list = crate::floatlist::KINDS.into_iter().find(|&k| {
            self.dock_rect(k)
                .is_some_and(|(_, r)| (r.x..r.x + r.w).contains(&x) && (r.y..r.y + r.h).contains(&y))
        });
        let list = floated.filter(|&k| crate::floatlist::is_list(k));
        if let Some(kind) = list.or(docked_list.filter(|_| floated.is_none() && self.picker.is_none())) {
            let row = s.list(kind).filter(|l| (l.0..l.0 + l.1).contains(&y)).map(|l| l.2 + y - l.0);
            return at(S::Floatlist(kind), row.map_or(Part::Blank, Part::Row));
        }
        // The lines' borders and words
        let free = self.picker.is_none() && floated.is_none();
        // (a line's words before the line: the slot's line drags off its tabs and its keys, as off its tabs; the last
        // drawn first, a card's line over a pane's — and under the follow card, the minimized chat, only the card's own:
        // a word it covers is neither pressed nor lit through it)
        let carded = s.thought_card.get().or(s.chat_mini.get()).is_some_and(|r| inside(r, x, y));
        let shown = |w: &&crate::wordspot::Spot| !carded || w.on == S::FollowCard;
        if down
            && free
            && let Some(i) =
                s.word_spots.borrow().iter().rposition(|w| on(w.y, w.from, w.to, x, y) && shown(&w))
        {
            return at(s.word_spots.borrow()[i].on, Part::Word(Word::Does(i)));
        }
        if down
            && free
            && let Some(b) = self.border_at(x, y)
        {
            return at(S::Edge, Part::Border(b));
        }
        let status = s.height == Some(y);
        if down
            && free
            && s.cut_spots.borrow().iter().any(|l| l.3 == crate::whole::Cut::Now && on(l.0, l.1, l.2, x, y))
        {
            return at(S::Status, Part::Word(Word::Now));
        }
        if down
            && free
            && let Some(&(.., spot)) = s.session_spots.borrow().iter().find(|l| on(l.0, l.1, l.2, x, y))
        {
            let pane = s
                .panes
                .iter()
                .find(|p| (p.1.x..p.1.x + p.1.w).contains(&x) && (p.1.y..p.1.y + p.1.h).contains(&y));
            let surface = match (status, spot, pane) {
                (true, ..) => S::Status,
                (_, Spot::Follow(_), Some(p)) => S::Code(p.0),
                _ => S::Chat,
            };
            return at(surface, Part::Word(Word::Session(spot)));
        }
        if status && free {
            return at(S::Status, Part::Blank);
        }
        // (the last drawn first: a floating frame's names over what's under it)
        if down
            && self.picker.is_none()
            && let Some(&(.., slot)) = s.slot_tabs.borrow().iter().rev().find(|t| on(t.0, t.1, t.2, x, y))
        {
            return at(S::SlotLine, Part::Word(Word::Tab(slot)));
        }
        if down
            && self.picker.is_none()
            && let Some(&(.., id)) = s.member_names.borrow().iter().rev().find(|t| on(t.0, t.1, t.2, x, y))
        {
            return at(S::SlotLine, Part::Word(Word::Member(id)));
        }
        if let Some(p) = self.picker.as_ref() {
            if down && let Some(i) = hint_on(S::Picker) {
                return at(S::Picker, Part::Word(Word::Does(i)));
            }
            if s.picker_preview_x.is_some_and(|px| x >= px) {
                return at(S::PickerPreview, Part::Blank);
            }
            if s.picker_input.get().is_some_and(|d| d.y == y) {
                return at(S::Picker, query_text(x));
            }
            let row = s.picker_list.filter(|l| (l.0..l.0 + l.1).contains(&y)).and_then(|l| p.row_at(y - l.0));
            return at(S::Picker, row.map_or(Part::Blank, Part::Row));
        }
        if wheel && self.popup.is_some() && s.popup.get().is_some_and(|r| inside(r, x, y)) {
            return at(S::Popup, Part::Blank);
        }
        // The review's file rail: its columns, left of where the code and the slot under it start (#73)
        if self.review_rail_up() && x < s.slot_x && (x < s.rail_w || !s.review_rail.borrow().is_empty()) {
            if wheel {
                return at(S::Rail, Part::Blank);
            }
            if down {
                let more = s.review_rail_more.borrow().iter().find(|r| r.0 == y).map_or(0, |r| r.1);
                let mark = s
                    .review_rail_marks
                    .borrow()
                    .iter()
                    .find(|r| r.0 == y && (r.2..r.2 + 2).contains(&x))
                    .map(|r| r.1);
                let row = s.review_rail.borrow().iter().find(|r| r.0 == y).map(|r| r.1);
                match (more, mark, row) {
                    (by, ..) if by != 0 => return at(S::Rail, Part::More(by)),
                    (_, Some(i), _) => return at(S::Rail, Part::Mark(i)),
                    (.., Some(i)) => return at(S::Rail, Part::Row(i)),
                    _ if x < s.rail_w => return at(S::Rail, Part::Blank),
                    _ => {}
                }
            }
        }
        // A group docked beside the code or floating: what's under the pointer in its rect is its panel's; else the
        // bottom slot's columns, under the code, between the side panel and the chat
        let docked = floated.or_else(|| {
            (s.docked.borrow().iter())
                .find(|d| (d.1.x..d.1.x + d.1.w).contains(&x) && (d.1.y..d.1.y + d.1.h).contains(&y))
                .map(|d| d.0)
        });
        let in_slot = docked.is_some() || (x >= s.slot_x && x < s.slot_end());
        let slot = match docked {
            Some(k) => k.slot(),
            None => self.slot_shown(),
        };
        let in_terminal =
            s.terminal_rows.is_some_and(|(y0, y1)| (y0..=y1).contains(&y)) && in_slot && docked.is_none();
        if in_terminal {
            let part = if down || wheel { self.terminal_cell(x, y, false) } else { Part::Blank };
            return at(self.terminal_surface(), part);
        }
        if let Some(a) = s.procs_panel.get()
            && (a.top..=a.bottom).contains(&y)
            && in_slot
            && slot == Some(Slot::Procs)
        {
            let detail = (s.run_rows.of(Slot::Procs))
                .filter(|r| (r.1..r.1 + r.3).contains(&y) && x + 1 >= r.2)
                .filter(|_| !(a.x..a.x + a.w).contains(&x));
            if let Some(r) = detail {
                return at(S::Output(Slot::Procs), Part::Text { line: y - r.1, col: x.saturating_sub(r.2) });
            }
            let row = (a.first..a.first + a.shown).contains(&y) && (a.x..a.x + a.w).contains(&x);
            let part = if row { Part::Row(a.scroll + y - a.first) } else { Part::Blank };
            return at(S::List(GroupKind::Procs), part);
        }
        if let Some((y0, y1, first, scroll)) = s.plugin_panel.get()
            && (y0..=y1).contains(&y)
            && in_slot
            && self.plugin_panel_in_sight()
        {
            // (the rows: not the header, not the status line under them)
            let part = if (first..y1).contains(&y) { Part::Row(scroll + y - first) } else { Part::Blank };
            return at(S::Plugin, part);
        }
        // (the row of a run's output under the pointer, and its column — as the panel drew them)
        let run_row = |slot: Slot| {
            (s.run_rows.of(slot))
                .filter(|r| (r.1..r.1 + r.3).contains(&y) && x >= r.2)
                .map(|r| Part::Text { line: y - r.1, col: x - r.2 })
        };
        if let Some((y0, y1, _)) = s.test_panel.get()
            && slot == Some(Slot::Test)
            && (y0..=y1).contains(&y)
            && in_slot
        {
            let output = at(S::Output(Slot::Test), Part::Blank);
            if wheel {
                return output;
            }
            if let Some(text) = run_row(Slot::Test) {
                return at(S::Output(Slot::Test), text);
            }
            return match self.list_row(GroupKind::Tests, x, y) {
                Some(i) => at(S::List(GroupKind::Tests), Part::Row(i)),
                None => output,
            };
        }
        if let Some((y0, y1, _)) = s.task_panel.get()
            && slot == Some(Slot::Task)
            && (y0..=y1).contains(&y)
            && in_slot
        {
            // (a row of its list — its places, a preflight's steps — laid out beside the output)
            let listed = self.kind_of(Slot::Task).and_then(|kind| Some((kind, self.list_row(kind, x, y)?)));
            if down && let Some((kind, i)) = listed {
                return at(S::List(kind), Part::Row(i));
            }
            let output = at(S::Output(Slot::Task), Part::Blank);
            if wheel {
                return output;
            }
            // (a preflight's step chip: its output from its heading — #72)
            let chip = s.task_steps.borrow().iter().find(|c| on(c.0, c.1, c.2, x, y)).map(|c| c.3);
            let step = chip.filter(|&k| {
                self.workspace.task_run.is_some()
                    && self.workspace.preflight.as_ref().is_some_and(|p| p.steps.get(k).is_some())
            });
            if down && let Some(k) = step {
                return at(S::Output(Slot::Task), Part::Word(Word::Step(k)));
            }
            return run_row(Slot::Task).map_or(output, |text| at(S::Output(Slot::Task), text));
        }
        if let Some(kind) = floated {
            return at(S::Float(kind), Part::Blank);
        }
        if self.workspace.viewer.sidebar.is_some()
            && !self.review_rail_up()
            && x < s.panes.iter().map(|p| p.1.x).min().unwrap_or(0)
        {
            let row = s.sidebar.borrow().iter().find(|l| l.0 == y).map(|l| l.1);
            return at(S::Side, row.map_or(Part::Blank, Part::Row));
        }
        if let Some(i) = s.stop_marks.borrow().iter().position(|m| m.0 == y && m.1 == x) {
            return at(S::FollowCard, Part::Word(Word::Stop(i)));
        }
        let links = |y: usize| s.chat_links.borrow().iter().position(|l| on(l.0, l.1, l.2, x, y));
        // (minimized, or floating over the code while it follows)
        let follow_card = s.chat_mini.get().or(s.thought_card.get());
        if self.workspace.claude.chat.is_some() && follow_card.is_some_and(|r| inside(r, x, y)) {
            return at(S::FollowCard, links(y).map_or(Part::Blank, |i| Part::Link(Link::Chat(i))));
        }
        if let Some(cx) = s.chat_x
            && self.workspace.claude.chat.is_some()
            && x >= cx
        {
            if wheel {
                return at(S::Transcript, self.transcript_text(x, y));
            }
            if let Some(i) = s.chat_ran.borrow().iter().position(|l| on(l.0, l.1, l.2, x, y)) {
                return at(S::Transcript, Part::Link(Link::Ran(i)));
            }
            if let Some(i) = s.chat_ids.borrow().iter().position(|l| on(l.0, l.1, l.2, x, y)) {
                return at(S::Transcript, Part::Link(Link::Id(i)));
            }
            if let Some(i) = links(y) {
                return at(S::Transcript, Part::Link(Link::Chat(i)));
            }
            let chip = |l: &(usize, usize, usize, crate::whole::Cut)| match l.3 {
                crate::whole::Cut::With(which) if on(l.0, l.1, l.2, x, y) => Some(which),
                _ => None,
            };
            if let Some(which) = s.cut_spots.borrow().iter().find_map(chip) {
                return at(S::Chat, Part::Word(Word::With(which)));
            }
            if s.chat_input_row(y).is_some() {
                return at(S::Input, self.input_text(x, y));
            }
            if s.chat_row(y).is_some() {
                return at(S::Transcript, self.transcript_text(x, y));
            }
            return at(S::Chat, Part::Blank);
        }
        // (Claude's edit review in place of the code — an answer still being written leaves the code the code's)
        if self.workspace.claude.review.as_ref().is_some_and(|r| r.ready) {
            if s.review_area.get().is_some_and(|a| inside(a, x, y)) {
                let hunk = s.review_rows.borrow().iter().find(|c| c.0 == y).map(|c| c.1);
                return at(S::EditReview, hunk.map_or(Part::Blank, Part::Row));
            }
            return at(S::EditReview, Part::Outside);
        }
        if self.welcome() {
            let row = s.welcome_rows.borrow().iter().position(|r| on(r.0, r.1, r.2, x, y));
            return at(S::Welcome, row.map_or(Part::Blank, Part::Row));
        }
        let Some((pid, ..)) =
            s.panes.iter().find(|(_, r, ..)| (r.x..r.x + r.w).contains(&x) && (r.y..r.y + r.h).contains(&y))
        else {
            // (a pane at the column's top has its top line over its rect — `chrome::line_in_pane` —, its `┬` over a
            // divider the line's too)
            let over = s.panes.iter().find(|(_, r, ..)| {
                !crate::chrome::line_in_pane(*r, s.text_top) && y + 1 == r.y && (r.x..=r.x + r.w).contains(&x)
            });
            return match over {
                Some(p) if !wheel => at(S::PaneTop(p.0), Part::Blank),
                Some(p) => at(S::Code(p.0), Part::Blank),
                None => at(S::Nothing, Part::Blank),
            };
        };
        if wheel {
            return at(S::Code(*pid), Part::Blank);
        }
        match self.code_text(*pid, x, y, false) {
            Some(part) => at(S::Code(*pid), part),
            None => at(S::PaneTop(*pid), Part::Blank),
        }
    }

    /// Whose lines light half with the pointer at (`x`, `y`), `u` under it (`Lights`): a run panel's, a list's, the
    /// terminal's are their group's where it's docked or floating, else the slot's; a card's or the follow card's, the
    /// frame drawn there (`Screen.frames` — a card's peek has none). None on a line (a line that drags lights itself —
    /// `border_hover`), a picker, the status line — what has no lines of its own to light.
    pub fn lights_at(&self, u: Under, x: usize, y: usize) -> Option<Lights> {
        use Surface as S;
        let group = || {
            self.float_at(x, y).map(|f| f.0).or_else(|| {
                let docked = self.screen.docked.borrow();
                docked.iter().find(|d| inside((d.1.x, d.1.y, d.1.w, d.1.h), x, y)).map(|d| d.0)
            })
        };
        match u.surface {
            S::Code(p) | S::PaneTop(p) => Some(Lights::Code(p)),
            S::Side | S::Rail => Some(Lights::Side),
            S::Chat | S::Transcript | S::Input => Some(Lights::Chat),
            S::Float(k) | S::Floatlist(k) => Some(Lights::Dock(k)),
            S::Output(_) | S::List(_) | S::Terminal | S::TerminalView | S::Plugin | S::SlotLine => {
                Some(group().map_or(Lights::Slot, Lights::Dock))
            }
            // (the ask's card records no frame — it has none, as a toast: nothing lit, unless one is drawn there)
            S::Card | S::FollowCard | S::Ask => {
                let frames = self.screen.frames.borrow();
                frames.iter().rev().find(|f| inside(f.rect, x, y)).and_then(|f| f.of)
            }
            _ => None,
        }
    }

    /// Pane `pid`'s character under (`x`, `y`) — None on its top line. `held` (a drag from a press there): the row
    /// clamped to its text's rows, the column to its own.
    fn code_text(&self, pid: PaneId, x: usize, y: usize, held: bool) -> Option<Part> {
        let s = &self.screen;
        let (_, rect, gutter, lines) = s.panes.iter().find(|p| p.0 == pid)?;
        // (its rows of text as the drawer lays them out: a pane under another has its top line in its first row —
        // `chrome::pane_text`)
        let text = crate::chrome::pane_text(*rect, s.text_top);
        let y = if held { y.clamp(text.y, (text.y + text.h).max(text.y + 1) - 1) } else { y };
        let x = if held { x.clamp(rect.x, (rect.x + rect.w).max(rect.x + 1) - 1) } else { x };
        if y < text.y {
            return None;
        }
        let row = y - text.y;
        let scroll = || match self.workspace.views.iter().find(|v| v.id == pid) {
            Some(v) if pid != self.workspace.viewer.focus => (v.top, v.left),
            _ => (self.doc().top, self.doc().left),
        };
        let (line, c0, x0, end) = lines.get(row).copied().unwrap_or_else(|| {
            let (top, left) = scroll();
            (top + row, left, 0, usize::MAX)
        });
        // A wrapped row: cells after its indent, never past its end (that's the next row's) — the hints drawn before the
        // pointer on that row, in this pane, not counted (frictions #150)
        let x = s.text_x(x, y, rect.x);
        let col = crate::wrap::clamp_col(c0 + x.saturating_sub(rect.x + gutter).saturating_sub(x0), end);
        Some(Part::Text { line, col })
    }

    /// The conversation's row and column under (`x`, `y`), if a row of it was drawn there.
    fn transcript_text(&self, x: usize, y: usize) -> Part {
        let text_x = self.screen.chat_x.unwrap_or(0) + crate::chrome::GAP + crate::chrome::PAD;
        self.screen.chat_row(y).map_or(Part::Blank, |line| Part::Text { line, col: x.saturating_sub(text_x) })
    }

    /// The chat's input's row and column under (`x`, `y`), if a row of it was drawn there.
    fn input_text(&self, x: usize, y: usize) -> Part {
        let row = self.screen.chat_input_row(y);
        row.map_or(Part::Blank, |(line, x0, _)| Part::Text { line, col: x.saturating_sub(x0) })
    }

    fn terminal_surface(&self) -> Surface {
        if crate::view::terminal_viewed(self) { Surface::TerminalView } else { Surface::Terminal }
    }

    /// The terminal's cell under (`x`, `y`): its screen starts under the header (`chrome::SLOT_HEAD`), the padding in
    /// from the slot's edge — clamped to it while a press there holds the mouse (`held`).
    fn terminal_cell(&self, x: usize, y: usize, held: bool) -> Part {
        let s = &self.screen;
        let (head, x0) = (crate::chrome::SLOT_HEAD, s.slot_x + crate::chrome::PAD);
        let Some((y0, y1)) = s.terminal_rows else { return Part::Blank };
        if !held && (y < y0 + head || x < x0) {
            return Part::Blank;
        }
        Part::Text { line: y.clamp(y0 + head, y1.max(y0 + head)) - y0 - head, col: x.saturating_sub(x0) }
    }
}

/// A press at (`x`, `y`) landing on `u`, before its surface takes it: a focused card pressed outside lets go of the
/// keys (the press goes on) — said, unless what's pressed says its own (a toast opens `:messages` at the message
/// pressed, a session's spot goes to the card, the list, a session). A prompt or picker pressed outside closes as its
/// surface's own act; a press on a card is the card's.
pub fn let_go(ed: &mut Editor, u: Under) {
    let modal = matches!(u.surface, Surface::Prompt | Surface::Card)
        || (u.surface == Surface::Picker && u.part == Part::Outside);
    if modal || !ed.card_has_keys() {
        return;
    }
    ed.card_focus = None;
    if u.surface != Surface::Toast && !matches!(u.part, Part::Word(Word::Session(_))) {
        let key = ed.key_said("C-q");
        ed.set_status(format!("keys back — the card waits, {key} answers it"));
    }
}

/// The document a press or drag on `on`'s text selects in: the code's in front, the conversation's, the input's, a run
/// panel's output, the terminal's view (the prompt's line and a picker's query are their own, `prompt::drag`).
pub(crate) fn text_doc(ed: &mut Editor, on: Drag) -> Option<&mut crate::document::Document> {
    match on {
        Drag::Code => Some(ed.doc_mut()),
        Drag::Transcript => crate::chatdoc::doc_mut(ed),
        Drag::Input => ed.workspace.claude.chat.as_mut().map(|c| c.input.pane.doc_mut()),
        Drag::Output(slot) => ed.workspace.outputs.get_mut(slot).and_then(|o| o.doc_in_mut()),
        Drag::TerminalView => crate::view::terminal_doc_mut(ed),
        Drag::Prompt | Drag::Picker | Drag::Review => None,
    }
}

/// The edit review's text under (`x`, `y`): which of its rows, the display column in that row's text. A press on a
/// row that isn't text (a change's head, the title): the text row under it, the one above at the end. `held` (a drag
/// from a press there): over a head the row above it; above its first row of text that row's start.
pub fn review_at(ed: &Editor, x: usize, y: usize, kind: MouseKind) -> Option<(usize, usize)> {
    let rows = ed.screen.review_text.borrow();
    let first = *rows.first()?;
    if kind != MouseKind::Down && y < first.0 {
        return Some((first.1, first.2));
    }
    let &(_, i, col, x0) = match kind {
        MouseKind::Down => rows.iter().find(|r| r.0 >= y).or(rows.last())?,
        _ => rows.iter().rev().find(|r| r.0 <= y)?,
    };
    Some((i, col + x.saturating_sub(x0)))
}

/// What a drag selected over the edit review (`Review.marked`): its rows' text from the press's column to the
/// pointer's, both included — the file's lines and the ones a change proposes alike, a change's head not text.
pub fn review_marked(ed: &Editor) -> Option<String> {
    use crate::codeview::Row;
    let rev = ed.workspace.claude.review.as_ref()?;
    let (a, b) = rev.marked.filter(|(a, b)| a != b)?;
    let (lo, hi) = (a.min(b), a.max(b));
    let rows = &rev.kept.as_ref()?.rows;
    let doc = ed.docs.iter().find(|d| d.id == rev.doc_id)?;
    let tab = ed.config.tab_width;
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate().take(hi.0 + 1).skip(lo.0) {
        let text = match row {
            Row::Text { line, .. } => {
                doc.text.line(*line).to_string().trim_end_matches(['\n', '\r']).to_string()
            }
            Row::Other { text, .. } => text.clone(),
            Row::Doc { .. } | Row::Header { .. } => continue,
        };
        let byte = |col: usize| {
            crate::graphemes::cells(&text, 0, tab)
                .find(|c| c.col >= col)
                .map_or(text.len(), |c| c.bytes.start)
        };
        let from = if i == lo.0 { lo.1 } else { 0 };
        let to = if i == hi.0 { hi.1 + 1 } else { usize::MAX };
        out.push(text[byte(from)..byte(to)].to_string());
    }
    Some(out.join("\n"))
}

/// A drag over the edit review let go: what it selected copied, as the terminal's selection is — the review's own
/// keys stay its answers (`y` accepts the change; it never copies there). A press not dragged: nothing selected.
fn review_copy(ed: &mut Editor) {
    let Some(text) = review_marked(ed) else {
        if let Some(r) = ed.workspace.claude.review.as_mut() {
            r.marked = None;
        }
        return;
    };
    let what = match text.lines().count() {
        0 | 1 => format!("{} characters", text.chars().count()),
        n => format!("{n} lines"),
    };
    crate::clipboard::copy(&ed.events.jobs(), text, what);
}

/// A press, drag or release on the edit review at (`x`, `y`), on a change's row (`hunk`) or off one: a press takes the
/// change in hand (one answered already, to change the answer) and gives the review the keys; a drag from it selects
/// its text, past its edge it scrolls; let go, what it selected is copied. Whether it took the event (a drag or release
/// not from a press there: no).
pub fn review_mouse(ed: &mut Editor, kind: MouseKind, x: usize, y: usize, hunk: Option<usize>) -> bool {
    let held = matches!(ed.mouse_anchor, Some((Drag::Review, _)));
    let at = review_at(ed, x, y, kind);
    let Some(r) = ed.workspace.claude.review.as_mut() else { return false };
    match kind {
        MouseKind::Drag if held => {
            if let (Some(at), Some(from)) = (at, r.pressed) {
                r.marked = Some((from, at));
            }
            edge(ed, x, y);
        }
        MouseKind::Up if held => {
            ed.mouse_anchor = None;
            review_copy(ed);
        }
        // (a press anywhere on it: the keys to it, as `C-q` brings them — from the chat, the code, a panel)
        MouseKind::Down => {
            if let Some(i) = hunk.filter(|&i| r.hunks.get(i).is_some_and(|h| !h.conflict)) {
                (r.current, r.follow) = (i, false);
            }
            r.away = false;
            (r.marked, r.pressed) = (None, at);
            if at.is_some() {
                ed.mouse_anchor = Some((Drag::Review, Held { from: 0, to: 0, unit: Unit::Char }));
            }
            ed.plugin_panel_unfocus();
            ed.procs_unfocus();
            ed.workspace.viewer.at = crate::workspace::At::Code;
        }
        _ => return false,
    }
    true
}

/// A press on `on`'s text, its surface's click having put the cursor on `pos`: the second at once on that character
/// selects its word, the third its line, a fourth starts again (`Unit::of`) — what it took is what a drag from it
/// holds, the drag going by the same unit.
pub fn text_press(ed: &mut Editor, on: Drag, pos: usize) {
    let pane = ed.workspace.viewer.focus;
    let Some(d) = text_doc(ed, on) else { return };
    // (the same character of the same document in the same pane: another split's, another buffer's isn't a double)
    let here = (on, d.id, pane, pos);
    let n = match ed.mouse_presses {
        Some((was, at, n)) if was == here && crate::clock::since(at) < crate::editor::DOUBLE_CLICK => n + 1,
        _ => 1,
    };
    ed.mouse_presses = Some((here, crate::clock::now(), n));
    let unit = Unit::of(n);
    let Some(d) = text_doc(ed, on) else { return };
    let held = Held::at(&d.text, pos, unit);
    if unit != Unit::Char {
        d.set_selection(Selection::single(held.range()));
    }
    ed.mouse_anchor = Some((on, held));
}

/// A drag from a press on `on`'s text to its character `pos` under the pointer: what's between selected, by the
/// press's unit (`Held::dragged`). A drag whose press wasn't on that text: nothing.
pub fn text_drag(ed: &mut Editor, on: Drag, pos: usize) {
    let Some((_, held)) = ed.mouse_anchor.filter(|a| a.0 == on) else { return };
    let Some(d) = text_doc(ed, on) else { return };
    let r = held.dragged(&d.text, pos);
    d.set_selection(Selection::single(r));
}

/// How often a drag held past its text's edge scrolls it a row toward the pointer.
pub const EDGE_EVERY: std::time::Duration = std::time::Duration::from_millis(50);

/// The screen rows `on`'s text was drawn on in the last frame (first, past the last) — where a drag past them scrolls
/// it. The input, the prompt's line and a picker's query: none (they're a few rows, their own).
fn text_rows(ed: &Editor, on: Drag) -> Option<(usize, usize)> {
    let s = &ed.screen;
    match on {
        Drag::Code => {
            let rect = s.panes.iter().find(|p| p.0 == ed.workspace.viewer.focus)?.1;
            let text = crate::chrome::pane_text(rect, s.text_top);
            Some((text.y, text.y + text.h))
        }
        Drag::Transcript => s.chat_view.get().map(|(y0, _, rows, _)| (y0, y0 + rows)),
        Drag::Output(slot) => s.run_rows.of(slot).map(|r| (r.1, r.1 + r.3)),
        Drag::TerminalView => s.terminal_rows.map(|(y0, y1)| (y0 + crate::chrome::SLOT_HEAD, y1 + 1)),
        Drag::Review => {
            let rows = s.review_text.borrow();
            Some((rows.first()?.0, rows.last()?.0 + 1))
        }
        Drag::Input | Drag::Prompt | Drag::Picker => None,
    }
}

/// The row a drag at screen row `y` is past its text by: up (−1), down (1), or inside it (None).
fn past(ed: &Editor, on: Drag, y: usize) -> Option<isize> {
    let (first, end) = text_rows(ed, on)?;
    match y {
        _ if y < first => Some(-1),
        _ if y >= end && end > first => Some(1),
        _ => None,
    }
}

/// After a drag at (`x`, `y`) on text: past its first or last row it's held there (`Editor.mouse_edge`), its text
/// scrolling a row toward the pointer each `EDGE_EVERY` (`edge_tick`) and the selection reaching the row now at the
/// edge; back inside, no longer.
pub fn edge(ed: &mut Editor, x: usize, y: usize) {
    let past = ed.mouse_anchor.and_then(|(on, _)| past(ed, on, y));
    ed.mouse_edge = match (past, ed.mouse_edge) {
        (None, _) => None,
        (Some(_), Some((.., at))) => Some((x as u16, y as u16, at)),
        (Some(_), None) => Some((x as u16, y as u16, crate::clock::now())),
    };
    // (the code's view held where the drag left it: the frame's scroll to the cursor would jump its scrolloff margin)
    if ed.mouse_edge.is_some() && matches!(ed.mouse_anchor, Some((Drag::Code, _))) {
        let d = ed.doc_mut();
        d.wheel_held = Some((d.selection().primary(), d.version()));
    }
}

/// Where `on`'s text is scrolled to: its top line and the rows of it above the view (a transcript's paragraph), the
/// edit review's first row.
fn edge_top(ed: &mut Editor, on: Drag) -> Option<(usize, usize)> {
    match on {
        Drag::Review => ed.workspace.claude.review.as_ref().map(|r| (r.scroll, 0)),
        _ => text_doc(ed, on).map(|d| (d.top, d.top_rows.map_or(0, |r| r.1))),
    }
}

/// When the next row of a drag held past its text's edge is due (`Editor::tick_wanted`).
pub fn edge_wake(ed: &Editor) -> Option<std::time::Duration> {
    let (.., at) = ed.mouse_edge?;
    Some(EDGE_EVERY.saturating_sub(crate::clock::since(at)))
}

/// A tick while a drag is held past its text's edge: its text a row toward the pointer, the drag again where the
/// pointer is (the selection to the row now at the edge). Whether it moved.
pub fn edge_tick(ed: &mut Editor) -> bool {
    let (Some((x, y, at)), Some((on, _))) = (ed.mouse_edge, ed.mouse_anchor) else { return false };
    if crate::clock::since(at) < EDGE_EVERY {
        return false;
    }
    let Some(by) = past(ed, on, y.into()) else {
        ed.mouse_edge = None;
        return false;
    };
    let was = edge_top(ed, on);
    // (toward the end, no further than its last line on the bottom row: a drag selects what's there, it never moves
    // the text out of sight — a file shorter than its rows doesn't move; one scrolled past it already, `zj` or the
    // wheel, stays there, never pulled back up)
    let rows = text_rows(ed, on).map_or(1, |(first, end)| end - first);
    let at_end = by > 0
        && match on {
            // (by the rows the pane lays out — a soft-wrapped line takes several)
            Drag::Code => ed.shows_end(rows),
            // (a line a row)
            Drag::Output(_) => text_doc(ed, on)
                .is_some_and(|d| d.top >= (crate::movement::last_line(&d.text) + 1).saturating_sub(rows)),
            // (its last row drawn, the last frame's)
            Drag::Review => ed.workspace.claude.review.as_ref().is_some_and(|r| {
                let last = r.kept.as_ref().map_or(0, |k| k.rows.len()).saturating_sub(1);
                ed.screen.review_text.borrow().iter().any(|t| t.1 == last)
            }),
            _ => false,
        };
    if at_end {
        ed.mouse_edge = None;
        return false;
    }
    match on {
        Drag::Code => {
            let d = ed.doc_mut();
            d.set_top(d.top.saturating_add_signed(by));
        }
        Drag::Transcript => crate::chatdoc::view_by(ed, by),
        Drag::Output(slot) => crate::rundoc::view_by(ed, slot, by),
        Drag::TerminalView => crate::view::terminal_view_by(ed, by),
        Drag::Review => {
            let Some(r) = ed.workspace.claude.review.as_mut() else { return false };
            let rows = r.kept.as_ref().map_or(0, |k| k.rows.len());
            (r.scroll, r.follow) = (r.scroll.saturating_add_signed(by).min(rows.saturating_sub(1)), false);
        }
        Drag::Input | Drag::Prompt | Drag::Picker => return false,
    }
    // (at its end already: nothing more to scroll, no more ticks — a drag back there arms it again)
    if edge_top(ed, on) == was {
        ed.mouse_edge = None;
        return false;
    }
    ed.mouse_edge = Some((x, y, crate::clock::now()));
    ed.handle_mouse(crate::event::Mouse { kind: MouseKind::Drag, x, y, alt: false, ctrl: false });
    true
}

// The table and its doc are the tests' (the walk holds every scene to it, `docs/mouse.md` is printed from it).

/// What a mouse act is, for the table and the walk: a press · the release after it · a drag from a press · a
/// second press at once (a double) · the wheel, up and down · a move with no button.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Press,
    Release,
    Drag,
    Double,
    Wheel,
    Hover,
}

#[cfg(test)]
pub const ACTS: [Act; 6] = [Act::Press, Act::Release, Act::Drag, Act::Double, Act::Wheel, Act::Hover];

/// The landings of the mouse plan (§4) that fill a gap.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landing {
    /// Selection: drags in every text, double a word, triple a line, the edge's scroll.
    S,
}

/// What an act on a part of a surface does.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    /// It acts: what's on screen, where the keys are, a selection, a scroll or a size changes (the wheel: when there's
    /// something to scroll).
    Acts,
    /// It says why it doesn't, where the user looks.
    Says(&'static str),
    /// A double: what one press there does, again — nothing of its own.
    Again,
    /// Nothing, by design (only a hover, a release, a wheel with nothing to scroll).
    Not(&'static str),
    /// Nothing yet — a gap the landing named fills; silent until it does.
    Gap(Landing, &'static str),
}

/// What `act` on `part` of `surface` does — the table (`docs/mouse.md` is printed from it). Every surface is a row of
/// its own: no catch-all over surfaces, so a new one doesn't compile until it's written here.
#[cfg(test)]
pub fn expect(surface: Surface, part: Part, act: Act) -> Expect {
    use Act as A;
    use Expect::{Acts, Gap, Not, Says};
    use Landing::S;
    use Surface as Su;
    const RELEASE: Expect = Not("the press did it");
    const HOVER: Expect = Not("nothing on hover");
    const AS_ONE: Expect = Expect::Again;
    const NO_SCROLL: Expect = Not("nothing to scroll");
    let text = matches!(part, Part::Text { .. });
    // (a word that does its key — a key hint, a name that names a state — takes a press: a drag from it says so, on
    // every surface it's drawn on, the slot's line's keys too; the rest of that line, its tabs' and members' names
    // included, drags the slot — piece B)
    if act == A::Drag && matches!(part, Part::Word(Word::Does(_))) && surface != Su::Pointing {
        return Says(HINT_DRAG);
    }
    match surface {
        Su::Pointing => match act {
            A::Press => Acts,
            A::Double => AS_ONE,
            A::Drag => Says(POINTING_DRAG),
            A::Wheel => NO_SCROLL,
            A::Release | A::Hover => RELEASE,
        },
        Su::Prompt => match (part, act) {
            (Part::Row(_) | Part::Outside | Part::Text { .. }, A::Press) | (Part::Text { .. }, A::Drag) => {
                Acts
            }
            (_, A::Press) => Says(PROMPT_ROWS),
            (_, A::Drag) => Gap(S, "a drag follows the pointer's row"),
            (_, A::Double) => AS_ONE,
            // (its menu is `tab`'s: what it fills in is the line's text — a wheel never types)
            (_, A::Wheel) => Not("tab moves through its completions"),
            (_, A::Release) => RELEASE,
            (_, A::Hover) => HOVER,
        },
        Su::Picker => match (part, act) {
            (Part::Outside, A::Press) => Acts,
            (Part::Row(_), A::Press | A::Double) => Acts,
            (Part::Text { .. }, A::Press | A::Drag) => Acts,
            (_, A::Press) => Not("no row there"),
            (Part::Outside, A::Double) => Not("the first press closed it"),
            (_, A::Double) => AS_ONE,
            (_, A::Drag) => Gap(S, "a drag follows the pointer's row"),
            (_, A::Wheel) => Acts,
            (_, A::Release) => RELEASE,
            (_, A::Hover) => HOVER,
        },
        Su::PickerPreview => match act {
            A::Press => Says("a press on a row of the list picks it"),
            A::Double => AS_ONE,
            A::Drag => Gap(S, "the preview's text selects"),
            A::Wheel => Acts,
            A::Release => RELEASE,
            A::Hover => HOVER,
        },
        Su::Toast => match act {
            A::Press => Acts,
            A::Double => Not("the first press opened :messages"),
            A::Drag => Not("the press opened :messages"),
            A::Wheel => Not("news isn't scrolled"),
            A::Release => RELEASE,
            A::Hover => HOVER,
        },
        // (its plugin's: `pressed` comes to it; only a press is the overlay's — the rest is what's under it)
        Su::Overlay => match act {
            A::Press => Acts,
            A::Double => AS_ONE,
            A::Drag | A::Wheel | A::Hover => Not("what's under it takes it"),
            A::Release => RELEASE,
        },
        Su::Card => match act {
            A::Press => Acts,
            A::Double => AS_ONE,
            A::Drag => Says(CARD_DRAG),
            // (a cut card's body)
            A::Wheel => Acts,
            A::Release => RELEASE,
            // (an answer on its bottom line lit, as every word that does its key — #200; a peek, its "more lines" row and
            // an answer held until it's read lights nothing: the walk's `Before.unlit`)
            A::Hover if matches!(part, Part::Answer(_)) => Acts,
            A::Hover => HOVER,
        },
        // (its press the keys to the code, where its `esc stop` is typed — never the stop itself)
        Su::Ask => match act {
            A::Press => Acts,
            A::Double => AS_ONE,
            A::Drag => Says(ASK_DRAG),
            A::Wheel => NO_SCROLL,
            A::Release => RELEASE,
            A::Hover => HOVER,
        },
        // (pressed it's held, lit; let go where it was pressed, the keys to what it heads; under the pointer, lit)
        Su::Edge => match act {
            A::Press | A::Release | A::Drag | A::Hover => Acts,
            A::Double => AS_ONE,
            A::Wheel => NO_SCROLL,
        },
        Su::Float(_) => match (part, act) {
            // (a key hint on its lines: its key, lit under the pointer — never a drag of the frame: piece B)
            (Part::Word(Word::Does(_)), A::Hover) => Acts,
            (_, A::Press) | (Part::Frame(_), A::Drag) => Acts,
            // (a click: nothing moved, nothing kept or said — a drag's release keeps and says its size)
            (Part::Frame(_), A::Release) => Not("a click keeps nothing; a drag's release keeps it"),
            (_, A::Drag) => Gap(S, "its text selects"),
            (_, A::Double) => AS_ONE,
            (_, A::Wheel) => Acts,
            (_, A::Release) => RELEASE,
            (_, A::Hover) => HOVER,
        },
        Su::Floatlist(_) => match (part, act) {
            (_, A::Press) | (Part::Row(_), A::Double) | (_, A::Wheel) => Acts,
            (_, A::Double) => AS_ONE,
            (_, A::Drag) => Gap(S, "a drag follows the pointer's row"),
            (_, A::Release) => RELEASE,
            (_, A::Hover) => HOVER,
        },
        Su::Status => match (part, act) {
            (Part::Word(_), A::Press | A::Hover) => Acts,
            (_, A::Press) => Not("no word there: the mode and the position say where the keys are"),
            (_, A::Double) => AS_ONE,
            (_, A::Drag) => Not("a line of state, no text to select"),
            (_, A::Wheel) => NO_SCROLL,
            (_, A::Release) => RELEASE,
            (_, A::Hover) => HOVER,
        },
        // (a name on the slot's line is the line too: a drag from it sizes the slot — a key hint's says so, above)
        Su::SlotLine => match (part, act) {
            (_, A::Press | A::Drag) => Acts,
            (_, A::Double) => AS_ONE,
            (_, A::Wheel) => NO_SCROLL,
            (_, A::Release) => RELEASE,
            (_, A::Hover) => Acts,
        },
        Su::Popup => match act {
            A::Wheel => Acts,
            A::Press | A::Double | A::Drag | A::Release | A::Hover => {
                Not("only the wheel is its; the rest goes under")
            }
        },
        Su::Rail => match (part, act) {
            (_, A::Press) | (_, A::Wheel) | (Part::Row(_), A::Double) => Acts,
            (_, A::Double) => AS_ONE,
            (_, A::Drag) => Gap(S, "a drag follows the pointer's row"),
            (_, A::Release) => RELEASE,
            (_, A::Hover) => HOVER,
        },
        Su::Terminal => match (part, act) {
            (_, A::Press) | (_, A::Wheel) | (Part::Text { .. }, A::Drag) => Acts,
            (_, A::Drag) => Gap(S, "a drag from its padding"),
            (_, A::Double) => Gap(S, "a word"),
            // (on its text: the press marked a selection's start, a click's release clears it — a drag's copies it)
            (Part::Text { .. }, A::Release) => Acts,
            (_, A::Release) => RELEASE,
            (_, A::Hover) => HOVER,
        },
        Su::TerminalView => match (part, act) {
            (_, A::Press) | (_, A::Wheel) => Acts,
            (_, A::Drag | A::Double) if text => Acts,
            (_, A::Drag) => Gap(S, "a drag from its padding"),
            (_, A::Double) => AS_ONE,
            (_, A::Release) => RELEASE,
            (_, A::Hover) => HOVER,
        },
        Su::Output(_) => match (part, act) {
            (_, A::Press) | (_, A::Wheel) => Acts,
            (_, A::Drag | A::Double) if text => Acts,
            (_, A::Drag) => Gap(S, "a drag follows the pointer"),
            (_, A::Double) => AS_ONE,
            (_, A::Release) => RELEASE,
            // (a preflight's step chip lights under the mouse)
            (Part::Word(_), A::Hover) => Acts,
            (_, A::Hover) => HOVER,
        },
        Su::List(_) => match act {
            A::Press | A::Wheel => Acts,
            A::Double => Gap(S, "again opens, as the session list's"),
            A::Drag => Gap(S, "a drag follows the pointer's row"),
            A::Release => RELEASE,
            A::Hover => HOVER,
        },
        Su::Plugin => match act {
            A::Press | A::Wheel => Acts,
            A::Double => AS_ONE,
            A::Drag => Gap(S, "a drag follows the pointer's row"),
            A::Release => RELEASE,
            A::Hover => HOVER,
        },
        Su::Side => match (part, act) {
            (_, A::Press) | (_, A::Wheel) => Acts,
            (Part::Row(_), A::Double) => Not("a row opens on one press — the git tab's opens on two"),
            (_, A::Double) => AS_ONE,
            (_, A::Drag) => Gap(S, "a drag follows the pointer's row"),
            (_, A::Release) => RELEASE,
            // (a tab's name on its top line lights under the mouse)
            (Part::Word(_), A::Hover) => Acts,
            (_, A::Hover) => HOVER,
        },
        Su::FollowCard => match (part, act) {
            (_, A::Press) => Acts,
            (_, A::Double) => AS_ONE,
            (_, A::Drag) => Not("placed by rule"),
            (_, A::Wheel) => NO_SCROLL,
            (_, A::Release) => RELEASE,
            (Part::Word(_), A::Hover) => Acts,
            (_, A::Hover) => HOVER,
        },
        Su::Chat => match (part, act) {
            (_, A::Press) => Acts,
            (_, A::Double) => AS_ONE,
            (_, A::Drag) => Not("tiled panes don't move — its line toward the code sizes it"),
            (_, A::Wheel) => Acts,
            (_, A::Release) => RELEASE,
            (Part::Word(_), A::Hover) => Acts,
            (_, A::Hover) => HOVER,
        },
        Su::Transcript | Su::Input => match (part, act) {
            (_, A::Press) | (_, A::Wheel) => Acts,
            (_, A::Drag | A::Double) if text => Acts,
            (_, A::Drag) => Gap(S, "a drag from a link"),
            (_, A::Double) => AS_ONE,
            (_, A::Release) => RELEASE,
            (_, A::Hover) => Not("a cut row comes whole under it"),
        },
        Su::EditReview => match (part, act) {
            (Part::Outside, A::Press | A::Drag) => Says(REVIEW_OFF),
            (Part::Outside, A::Wheel) => NO_SCROLL,
            (_, A::Press) | (_, A::Wheel) | (_, A::Drag) => Acts,
            (_, A::Double) => AS_ONE,
            (_, A::Release) => RELEASE,
            (_, A::Hover) => HOVER,
        },
        Su::Welcome => match (part, act) {
            (Part::Row(_), A::Press) => Acts,
            (_, A::Press) => Says("a row of the start screen presses its keys"),
            (_, A::Drag) => Says(WELCOME_DRAG),
            (_, A::Double) => AS_ONE,
            (_, A::Wheel) => NO_SCROLL,
            (_, A::Release) => RELEASE,
            (_, A::Hover) => HOVER,
        },
        Su::PaneTop(_) => match (part, act) {
            (_, A::Press) => Acts,
            (_, A::Double) => AS_ONE,
            (_, A::Drag) => Not("tiled panes don't move"),
            (_, A::Wheel) => Acts,
            (_, A::Release) => RELEASE,
            (Part::Word(_), A::Hover) => Acts,
            (_, A::Hover) => Not("its line between the names: nothing to press"),
        },
        Su::Code(_) => match (part, act) {
            (_, A::Press) | (_, A::Wheel) | (_, A::Drag) => Acts,
            (_, A::Double) if text => Acts,
            (_, A::Double) => AS_ONE,
            (_, A::Release) => Not("ctrl+press goes there at the release"),
            // (a presence tag shows its name under the mouse)
            (Part::Word(_), A::Hover) => Acts,
            (_, A::Hover) => Not("no hover from the language server — K"),
        },
        // (a press on a row presses its key, accepts its item, replies; the rest of the card says so)
        Su::WhichKey | Su::Completion | Su::NoteCard => match (part, act) {
            (Part::Row(_), A::Press) => Acts,
            (_, A::Press) if surface == Su::NoteCard => Acts,
            (_, A::Press) => Says("a row presses it"),
            (_, A::Double) => AS_ONE,
            (_, A::Wheel) if surface == Su::Completion => Acts,
            (_, A::Wheel) => NO_SCROLL,
            (_, A::Drag) => Not("its press did it — a drag goes to what's under it"),
            (_, A::Release) => RELEASE,
            (_, A::Hover) => HOVER,
        },
        Su::Nothing => Not("nothing there"),
    }
}

/// States that decide a press whatever surface it lands on — the table is by surface, these by state: (what the walk's
/// scene is named with, the state, what a press does there).
#[cfg(test)]
pub const STATES: &[(&str, &str, Expect)] = &[
    (
        "C-w m moving",
        "`C-w m` moving a floating pane: a press on it, not its top line",
        Expect::Says(MOVING_PRESS),
    ),
    // (which-key's rows press as their keys; the press then acts where it lands, as the table says)
    (
        "the window prefix taken",
        "a key sequence under way (`space`, `C-w`, `g`, a count…): a press ends it, then acts where it lands — which-key's \
         row presses its key, a word does its key (in normal mode; mid-insert a word's keys are refused, said); insert \
         mode's held keys (a `j k` binding's `j` — the code's, the chat's input's, the prompt's line's) are text: typed \
         where they were typed, never dropped; `C-r` waiting for a register's name, ended — in the code's insert mode and \
         every line alike",
        Expect::Acts,
    ),
];

/// The other buttons, apart from the table (its acts are the left button's and the wheel's): (the button, what it does).
#[cfg(test)]
pub const BUTTONS: &[(&str, &str)] = &[
    (
        "The middle button",
        "the clipboard pasted at the pointer, the cursor put there first, on text that takes it — the code in normal \
         or insert mode, the chat's input, a prompt's line; anywhere else nothing, said",
    ),
    (
        "The wheel sideways",
        "the code pane under the pointer, its view's columns; elsewhere nothing to scroll",
    ),
    ("The right button", "the terminal's — tarae doesn't take it"),
];

/// The table's rows as `docs/mouse.md` prints them: a surface, the part, a stand-in of each.
#[cfg(test)]
fn rows() -> Vec<(&'static str, &'static str, Surface, Part)> {
    use Surface as Su;
    let text = Part::Text { line: 0, col: 0 };
    vec![
        ("The code", "text", Su::Code(0), text),
        ("The code", "a presence tag", Su::Code(0), Part::Word(Word::Session(Spot::List))),
        ("A pane's top line", "", Su::PaneTop(0), Part::Blank),
        (
            "A pane's top line",
            "a name in its path (its row in the side panel's files), a buffer's name, a key hint (`enter go there`)",
            Su::PaneTop(0),
            Part::Word(Word::Does(0)),
        ),
        (
            "A line between panels, between your panes",
            "the chat's, the side panel's, the rail's, the slot's, a dock's, a divider",
            Su::Edge,
            Part::Border(Border::Side),
        ),
        ("Claude's edit review", "a change", Su::EditReview, Part::Row(0)),
        ("Claude's edit review", "off it", Su::EditReview, Part::Outside),
        ("The review's rail", "a file", Su::Rail, Part::Row(0)),
        ("The review's rail", "off its rows", Su::Rail, Part::Blank),
        ("The review's rail", "a key hint at its foot", Su::Rail, Part::Word(Word::Does(0))),
        ("The chat's conversation", "text", Su::Transcript, text),
        ("The chat's conversation", "a link, a tool row, an id", Su::Transcript, Part::Link(Link::Chat(0))),
        ("The chat's input", "text", Su::Input, text),
        ("The chat", "its top line, its gaps", Su::Chat, Part::Blank),
        ("The chat", "a session's name, `+N`", Su::Chat, Part::Word(Word::Session(Spot::List))),
        (
            "The chat",
            "what goes with the message over its input (a quote, `screen`, an element pointed at, `N things`)",
            Su::Chat,
            Part::Word(Word::With(Some(0))),
        ),
        (
            "The chat",
            "`follow`, `paused`, the turn's word, the model, a key hint at its foot",
            Su::Chat,
            Part::Word(Word::Does(0)),
        ),
        ("The follow card", "a trail row, the card", Su::FollowCard, Part::Blank),
        ("The follow card", "a strip mark", Su::FollowCard, Part::Word(Word::Stop(0))),
        (
            "The follow card",
            "`▲ waits`, `paused`, the turn's word, its keys",
            Su::FollowCard,
            Part::Word(Word::Does(0)),
        ),
        ("The side panel", "a row", Su::Side, Part::Row(0)),
        (
            "The side panel",
            "a tab's name on its top line, a key hint at its foot",
            Su::Side,
            Part::Word(Word::Does(0)),
        ),
        ("The side panel", "off its rows", Su::Side, Part::Blank),
        ("Run output", "text", Su::Output(Slot::Test), text),
        ("Run output", "off its rows", Su::Output(Slot::Test), Part::Blank),
        ("Run output", "a preflight's step chip", Su::Output(Slot::Task), Part::Word(Word::Step(0))),
        ("A group's list", "a row", Su::List(GroupKind::Tests), Part::Row(0)),
        ("The slot's line", "a tab, a member's name", Su::SlotLine, Part::Word(Word::Tab(Slot::Test))),
        ("The slot's line", "its keys", Su::SlotLine, Part::Word(Word::Does(0))),
        ("The terminal", "", Su::Terminal, text),
        ("The terminal's view mode", "", Su::TerminalView, text),
        ("A plugin's panel", "", Su::Plugin, Part::Row(0)),
        (
            "A floating list (sessions, `:messages`)",
            "a row",
            Su::Floatlist(GroupKind::Sessions),
            Part::Row(0),
        ),
        (
            "A floating list (sessions, `:messages`)",
            "off its rows",
            Su::Floatlist(GroupKind::Sessions),
            Part::Blank,
        ),
        ("A floating pane", "its frame", Su::Float(GroupKind::Tests), Part::Frame(Grip::Move(0, 0))),
        ("A floating pane", "inside", Su::Float(GroupKind::Tests), Part::Blank),
        (
            "A floating pane",
            "a key hint on its lines",
            Su::Float(GroupKind::Tests),
            Part::Word(Word::Does(0)),
        ),
        ("Cards", "an answer, the card", Su::Card, Part::Answer('y')),
        ("The ask's card (`space i`, a commit draft)", "", Su::Ask, Part::Blank),
        ("The ask's card (`space i`, a commit draft)", "a key hint", Su::Ask, Part::Word(Word::Does(0))),
        ("The notification card", "", Su::Toast, Part::Row(0)),
        ("A plugin's overlay", "a row", Su::Overlay, Part::Row(0)),
        ("A picker", "a row", Su::Picker, Part::Row(0)),
        ("A picker", "its query", Su::Picker, Part::Text { line: 0, col: 0 }),
        ("A picker", "its frame's other rows", Su::Picker, Part::Blank),
        ("A picker", "a key hint on its frame", Su::Picker, Part::Word(Word::Does(0))),
        ("A picker", "outside it", Su::Picker, Part::Outside),
        ("A picker's preview", "", Su::PickerPreview, Part::Blank),
        ("The prompt", "a completion", Su::Prompt, Part::Row(0)),
        ("The prompt", "its input", Su::Prompt, Part::Text { line: 0, col: 0 }),
        ("The prompt", "outside it", Su::Prompt, Part::Outside),
        ("Hover text, docs", "", Su::Popup, Part::Blank),
        ("Which-key", "a key's row", Su::WhichKey, Part::Row(0)),
        ("Which-key", "the rest of it", Su::WhichKey, Part::Blank),
        ("The completion menu", "a row", Su::Completion, Part::Row(0)),
        ("The completion menu", "the rest of it, its docs", Su::Completion, Part::Blank),
        ("The note card", "", Su::NoteCard, Part::Blank),
        ("The note card", "a key hint", Su::NoteCard, Part::Word(Word::Does(0))),
        (
            "The status line",
            "a word (`▲N waiting · C-q`, the chip, what Claude does now, the branch, `▲ N`, `full screen`)",
            Su::Status,
            Part::Word(Word::Now),
        ),
        (
            "The status line",
            "a key hint (a list's, a picker's, a prompt's)",
            Su::Status,
            Part::Word(Word::Does(0)),
        ),
        ("The status line", "the rest", Su::Status, Part::Blank),
        ("The start screen", "a row with keys", Su::Welcome, Part::Row(0)),
        ("The start screen", "the rest", Su::Welcome, Part::Blank),
        ("Pointing's labels", "", Su::Pointing, Part::Blank),
    ]
}

/// `docs/mouse.md`, as the table says it.
#[cfg(test)]
pub fn doc() -> String {
    let cell = |e: Expect| match e {
        Expect::Acts => "✓".to_string(),
        Expect::Says(why) => format!("says: {why}"),
        Expect::Again => "as one press".to_string(),
        Expect::Not(why) => format!("— {why}"),
        Expect::Gap(l, why) => format!("✗ {l:?}: {why}"),
    };
    let mut out = String::from(
        "# The mouse\n\n\
         What each act does on each surface — printed from `mouse::expect` (`src/mouse.rs`), the table the walk\n\
         (`mouse::tests::no_press_is_lost_in_silence`) holds every scene to; `TARAE_BLESS=1 cargo test mouse` writes it\n\
         again. The rules (CLAUDE.md, \"The mouse, one way on every surface\"): a press gives the keys to the surface it\n\
         lands on; a drag selects wherever there's text; the wheel scrolls what's under the pointer and nothing else;\n\
         every line between two things with sizes drags; a word that names a state does its key. A press, a drag or a\n\
         wheel that does nothing says why — nothing at all only for a hover, a release, a wheel with nothing to scroll.\n\n\
         ✓ acts · says: it says why not · — nothing, by design · ✗ a gap, and the landing of `.sb/mouse-plan.md` §4\n\
         that fills it (S selection).\n\n\
         | Surface | Part | Press | Release | Drag | Double | Wheel | Hover |\n\
         |---|---|---|---|---|---|---|---|\n",
    );
    for (surface, part, s, p) in rows() {
        let cells: Vec<String> = ACTS.iter().map(|&a| cell(expect(s, p, a))).collect();
        out.push_str(&format!("| {surface} | {part} | {} |\n", cells.join(" | ")));
    }
    out.push_str("\nStates that decide a press, on any surface:\n\n| State | Press |\n|---|---|\n");
    for &(_, state, e) in STATES {
        out.push_str(&format!("| {state} | {} |\n", cell(e)));
    }
    out.push_str("\nThe other buttons:\n\n| Button | What it does |\n|---|---|\n");
    for (button, what) in BUTTONS {
        out.push_str(&format!("| {button} | {what} |\n"));
    }
    out
}

#[cfg(test)]
mod tests;
