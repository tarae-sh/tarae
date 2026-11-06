//! The language server's hover where the pointer rests (#233 — CLAUDE.md, the mouse's bullet; the user, 2026-10-29, at
//! work, reversing 2026-10-26): the pointer still ~0.5 s on a name in the code shows `K`'s card at that name, the
//! problems underlined there first, as the cursor's card says them; gone as the pointer leaves the name and its card,
//! never the keys — the wheel over it scrolls it, a press there is the code's under it, as on `K`'s (the card is a
//! reveal: it shows, it has no act of its own). `editor.mouse-hover` turns it off; `K` and ctrl+press stay as they are.
//!
//! Idle stays idle: one timer at a time — a new name moves the rest's `due`, the timer sleeping on for what's left
//! (a sweep across the code starts one thread, not one a word); the request goes on the hover's route
//! (`Editor::lsp_hover_at`, `server_for_doc`), its answer kept only for the rest it was asked for (`n`), at the
//! version it was asked of (`on_response`'s rule) — a newer rest's replaces it. One card at a time, the mouse's first:
//! `K`'s up, or a cut `…` under the pointer, no rest; while it's shown the cursor's diagnostic card steps aside.
//!
//! **Every card the mouse brings waits the same rest** (#391, the user, 2026-11-05: "give it a moment, as show docs
//! does"): a cut `…`'s and a flag's `¶`'s too, by one rule (`bring`, `due`) on this one timer — while one is up, the
//! next target's shows at once; off them all, it goes at once. The row in hand's card (keys) and the lit grounds under
//! the pointer are no such card: they stay immediate. The docs' hover too: with any of the mouse's cards up as the
//! pointer comes onto a name (`State.warm`), its server is asked at once, the card up staying until the answer comes
//! (`Rest.before`) — never a gap between two cards, never a card asked for a name the pointer only crossed cold.
//!
//! **A name in the chat alike** (#389 step 6, `.sb/chatlink-plan.md` §2.4): the pointer resting on a name in the
//! conversation shows the server's hover there, by the same rest, hand-off and leaving — asked where `space k` asks it
//! (`chatlink::rest`: a cited place's lines, a quoted block's spot in its file, else by name), the card in the chat at
//! the name. What can't be asked there shows nothing and says nothing.

use std::time::{Duration, Instant};

use unicode_width::UnicodeWidthStr;

use crate::document::DocId;
use crate::editor::Editor;
use crate::event::Event;
use crate::mouse::{Part, Surface, Under};
use crate::split::PaneId;

/// How long the pointer rests on a name before its hover is asked.
pub const REST: Duration = Duration::from_millis(500);

#[derive(Default)]
pub struct State {
    pub rest: Option<Rest>,
    /// The latest rest's number — an answer for another is dropped.
    n: u64,
    /// A timer sleeping (one at a time).
    armed: bool,
    /// One of the mouse's cards was up as this hover came (`card_up`, taken before any of them moves): the next
    /// target's comes at once — the warm hand-off, one rule for every card.
    pub warm: bool,
}

#[cfg(test)]
impl State {
    /// Whether a timer sleeps.
    pub fn armed(&self) -> bool {
        self.armed
    }
}

/// What a rest is on: a code pane's text, or the chat's conversation — a name there by its text (what streams in after
/// it moves the conversation's version on, never the name).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum In {
    Code(PaneId),
    Chat(std::sync::Arc<str>),
}

/// Where the pointer rests: a name (or a problem's underline) of a code pane, or a name in the chat, as it was drawn
/// then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spot {
    pub pane: In,
    pub doc: DocId,
    /// The document's version (the chat's: 0, `In::Chat` holds its name).
    pub version: u64,
    /// The name's bytes (a problem's spot off a name: its one character).
    pub word: std::ops::Range<usize>,
    /// Whether it's a name — the server is asked only of one.
    pub name: bool,
    /// The name's first cell on screen.
    pub cell: (usize, usize),
    /// The pane's rect and scroll as drawn (the chat's: its conversation's rect and first row) — another (a resize,
    /// follow moving the view, the wheel) leaves the card unshown.
    pub view: (crate::split::Rect, usize, usize),
}

pub struct Rest {
    pub spot: Spot,
    n: u64,
    due: Instant,
    /// The rest came: asked of its server, or there was none to ask.
    landed: bool,
    /// Asked again once already, the server having been busy (`busy`).
    again: bool,
    /// What it shows once answered (docs empty: the problems alone, else nothing).
    pub card: Option<Card>,
    /// The card up as the pointer came here from it (the hand-off): shown, where it was, until this one's answer.
    pub before: Option<Box<(Spot, Card)>>,
}

#[derive(Default)]
pub struct Card {
    pub docs: Vec<crate::markdown::Line>,
    /// Rows scrolled down (the wheel over it).
    pub scroll: usize,
}

#[cfg(test)]
impl Rest {
    /// Its number — what its request carries (`Kind::PointerHover`).
    pub fn n(&self) -> u64 {
        self.n
    }
}

impl Rest {
    /// The card to draw and the spot it's of, if one: answered — else the one it took over from, until it is — and its
    /// document and pane as they were.
    pub fn shown<'a>(&'a self, ed: &Editor) -> Option<(&'a Spot, &'a Card)> {
        let (spot, card) = match (&self.card, &self.before) {
            (Some(c), _) => (&self.spot, c),
            (None, Some(b)) => (&b.0, &b.1),
            (None, None) => return None,
        };
        still(ed, spot).then_some((spot, card))
    }

    /// What it showed, to stand on as the next rest's `before`.
    fn into_shown(self, ed: &Editor) -> Option<Box<(Spot, Card)>> {
        match self.card {
            Some(card) => still(ed, &self.spot).then(|| Box::new((self.spot, card))),
            None => self.before.filter(|b| still(ed, &b.0)),
        }
    }
}

/// Whether `spot`'s document and pane are as they were — in the chat, its name where it was.
fn still(ed: &Editor, spot: &Spot) -> bool {
    let same = doc_of(ed, spot).is_some_and(|d| match &spot.pane {
        In::Code(_) => d.version() == spot.version,
        In::Chat(name) => d.text.get_byte_slice(spot.word.clone()).is_some_and(|s| s == **name),
    });
    same && view_of(ed, &spot.pane) == Some(spot.view)
}

/// The document `spot` is in: a code pane's, or the chat's conversation.
pub fn doc_of<'a>(ed: &'a Editor, spot: &Spot) -> Option<&'a crate::document::Document> {
    match spot.pane {
        In::Code(_) => ed.docs.iter().find(|d| d.id == spot.doc),
        In::Chat(_) => crate::chatdoc::transcript(ed)?.pane.doc_in().filter(|d| d.id == spot.doc),
    }
}

/// Whether one of the mouse's cards is up, as last drawn: a cut `…`'s, a flag's, the docs' hover.
pub fn card_up(ed: &Editor) -> bool {
    let docs = ed.screen.rest_card.get().is_some() && ed.hover_rest.rest.is_some();
    ed.cut_hover.is_some() || ed.flag_hover.is_some() || docs
}

/// The pane's rect and scroll in the last frame — the chat's: its conversation's rect and first row.
fn view_of(ed: &Editor, pane: &In) -> Option<(crate::split::Rect, usize, usize)> {
    let pane = match pane {
        In::Code(p) => *p,
        In::Chat(_) => {
            let (y, w, h, first) = ed.screen.chat_view.get()?;
            return Some((crate::split::Rect { x: ed.screen.chat_x()?, y, w, h }, first, 0));
        }
    };
    let rect = ed.screen.panes.iter().find(|p| p.0 == pane)?.1;
    let (top, left) = match ed.workspace.views.iter().find(|v| v.id == pane) {
        Some(v) if pane != ed.workspace.viewer.focus => (v.top, v.left),
        _ => (ed.doc().top, ed.doc().left),
    };
    Some((rect, top, left))
}

/// The name (or underlined problem) at (`x`, `y`) of a code pane, or the name there in the chat (`u`: `mouse_under`'s
/// answer there), if one rests there.
fn spot_at(ed: &Editor, u: Under, x: usize, y: usize) -> Option<Spot> {
    // (one card at a time: `K`'s up, or a cut `…`'s card under the pointer, stays theirs)
    if !ed.config.mouse_hover || ed.popup.is_some() || ed.cut_hover.is_some() || ed.cut_wait.is_some() {
        return None;
    }
    if u.surface == Surface::Transcript {
        return chat_spot(ed, u.part, x, y);
    }
    let (Surface::Code(pane), Part::Text { line, col }) = (u.surface, u.part) else { return None };
    let id = match pane == ed.workspace.viewer.focus {
        true => ed.doc().id,
        false => ed.workspace.views.iter().find(|v| v.id == pane)?.doc,
    };
    let doc = ed.docs.iter().find(|d| d.id == id)?;
    let text = &doc.text;
    let line = line.min(crate::movement::last_line(text));
    let pos = crate::movement::pos_at_col(text, line, col, ed.config.tab_width);
    // (the character there from its chunk — no slice to the file's end at every cell the pointer crosses, #406)
    let c = match (pos <= text.len_bytes()).then(|| text.chunk_at_byte(pos)) {
        Some((chunk, at, ..)) if pos < at + chunk.len() => chunk[pos - at..].chars().next(),
        _ => text.get_byte_slice(pos..).and_then(|s| s.chars().next()),
    }?;
    let name = crate::completion::is_word(c);
    let problem = doc.lsp.diagnostics.iter().any(|d| crate::term::underlines(d, pos));
    if !name && !problem {
        return None;
    }
    let word = if name { crate::completion::word_at(text, pos) } else { pos..pos + c.len_utf8() };
    // (its first cell: back from the pointer by what's before it on the row — a name is never wrapped inside)
    let before = std::borrow::Cow::<str>::from(text.byte_slice(word.start..pos)).width();
    let x0 = ed.screen.panes.iter().find(|p| p.0 == pane).map_or(0, |p| p.1.x + p.2);
    Some(Spot {
        pane: In::Code(pane),
        doc: id,
        version: doc.version(),
        word,
        name,
        cell: (x.saturating_sub(before).max(x0), y),
        view: view_of(ed, &In::Code(pane))?,
    })
}

/// The name in the chat's conversation at (`x`, `y`) (`part`: what's there) — a cited place whole, or a word on its own
/// letters — if one rests there. A run's row and an id are buttons of their own: none.
fn chat_spot(ed: &Editor, part: Part, x: usize, y: usize) -> Option<Spot> {
    use crate::mouse::Link;
    if matches!(part, Part::Link(Link::Ran(_) | Link::Id(_))) {
        return None;
    }
    let Part::Text { line: row, col } = ed.transcript_text(x, y) else { return None };
    let (at, row_start) = crate::chatdoc::char_at(ed, row, col)?;
    let word = crate::chatlink::rest_word(ed, at)?;
    let d = crate::chatdoc::transcript(ed)?.pane.doc_in()?;
    // (its first cell: back from the pointer by what's before it on this row — a place may wrap)
    let before = d.text.byte_slice(word.start.max(row_start)..at).to_string().width();
    let pane = In::Chat(d.text.byte_slice(word.clone()).to_string().into());
    let view = view_of(ed, &pane)?;
    Some(Spot { pane, doc: d.id, version: 0, word, name: true, cell: (x.saturating_sub(before), y), view })
}

/// The rest's number, while one is there.
pub fn resting(ed: &Editor) -> Option<u64> {
    ed.hover_rest.rest.as_ref().map(|r| r.n)
}

/// The pointer moved to (`x`, `y`) (`Event::Hover`): a new name rests from now, the card of the one left goes — over the
/// card itself it stays. With a mouse's card up as it came (`State.warm`), the name is asked at once and the docs' card
/// up stays until its answer. Whether what's drawn changed.
pub fn hovered(ed: &mut Editor, u: Under, x: usize, y: usize) -> bool {
    if over_card(ed, x, y) {
        return false;
    }
    let spot = spot_at(ed, u, x, y);
    if ed.hover_rest.rest.as_ref().is_some_and(|r| Some(&r.spot) == spot.as_ref()) {
        return false;
    }
    let old = ed.hover_rest.rest.take();
    let drawn = old.is_some() && ed.screen.rest_card.get().is_some();
    let Some(spot) = spot else { return drawn };
    let warm = ed.hover_rest.warm;
    let before = old.filter(|_| warm).and_then(|o| o.into_shown(ed));
    let st = &mut ed.hover_rest;
    st.n += 1;
    let now = crate::clock::now();
    let due = if warm { now } else { now + REST };
    st.rest = Some(Rest { spot, n: st.n, due, landed: false, again: false, card: None, before });
    match warm {
        true => land(ed),
        false => arm(ed, REST),
    }
    // (the card drawn goes unless it stands on; one that came at once — no server, the problems alone — is drawn)
    let r = ed.hover_rest.rest.as_ref();
    (drawn && r.is_some_and(|r| r.before.is_none())) || r.is_some_and(|r| r.card.is_some())
}

/// Input came (a key, a press, the wheel, a paste — your hands elsewhere): the rest and its card go, but the wheel over
/// the card, which scrolls it; a mouse's card still waiting never comes (one up stays while the pointer's on it).
pub fn input(ed: &mut Editor, ev: &Event) {
    (ed.cut_wait, ed.flag_wait) = (None, None);
    if ed.hover_rest.rest.is_none() {
        return;
    }
    use crate::event::MouseKind as K;
    if let Event::Mouse(m) = ev
        && matches!(m.kind, K::ScrollUp | K::ScrollDown)
        && over_card(ed, m.x as usize, m.y as usize)
    {
        return;
    }
    ed.hover_rest.rest = None;
}

/// Whether (`x`, `y`) is on the card as last drawn.
pub fn over_card(ed: &Editor, x: usize, y: usize) -> bool {
    let r = ed.screen.rest_card.get().filter(|_| ed.hover_rest.rest.is_some());
    r.is_some_and(|r| (r.0..r.0 + r.2).contains(&x) && (r.1..r.1 + r.3).contains(&y))
}

/// The pointer gone (the terminal's focus lost): nothing rests.
pub fn away(ed: &mut Editor) {
    ed.hover_rest.rest = None;
}

/// The wheel over the card: scrolled `by` rows, never past its last page as drawn.
pub fn scroll(ed: &mut Editor, by: isize) {
    let end = ed.screen.rest_end.get().unwrap_or(0);
    let shown = ed.hover_rest.rest.as_mut().and_then(|r| match r.card.as_mut() {
        Some(c) => Some(c),
        None => r.before.as_mut().map(|b| &mut b.1),
    });
    if let Some(c) = shown {
        c.scroll = c.scroll.min(end).saturating_add_signed(by).min(end);
    }
}

/// The timer for what's left of the rest, unless one sleeps already (it sleeps on for what's left when it wakes). Not
/// in tests but a simulation's (its clock moves them; others land the rest by `rested`).
fn arm(ed: &mut Editor, after: Duration) {
    #[cfg(test)]
    if crate::sim::now().is_none() {
        return;
    }
    if std::mem::replace(&mut ed.hover_rest.armed, true) {
        return;
    }
    ed.events.jobs().spawn(move || {
        crate::event::sleep(after);
        tick
    });
}

/// The timer woke: the rest asked of its server once it's been there `REST`, a mouse's card waiting up once due — or
/// slept on for what's left of the soonest. Nothing drawn unless a card came, or there's no server to ask and a
/// problem shows alone.
fn tick(ed: &mut Editor) {
    ed.hover_rest.armed = false;
    ed.unchanged = true;
    let now = crate::clock::now();
    let mut left = cards_due(ed, now);
    if let Some(r) = ed.hover_rest.rest.as_ref().filter(|r| !r.landed) {
        match r.due.saturating_duration_since(now) {
            l if l.is_zero() => land(ed),
            l => left = Some(left.map_or(l, |m| m.min(l))),
        }
    }
    if let Some(l) = left {
        arm(ed, l);
    }
}

/// The mouse's cards waiting (`bring`), at `now`: those due up — the pointer still over them —, else what's left of the
/// soonest's wait.
fn cards_due(ed: &mut Editor, now: Instant) -> Option<Duration> {
    let cut = crate::whole::rest_due(ed, now);
    let flag = crate::flags::rest_due(ed, now);
    cut.into_iter().chain(flag).min()
}

/// A mouse's card's two places: the card up, and the one waiting with when it's due.
pub type Fields<T> = fn(&mut Editor) -> (&mut Option<T>, &mut Option<(T, Instant)>);

/// A card the mouse brings (a cut `…`'s, a flag's `¶`'s), the pointer over `over` now (#391): onto one with no card up,
/// it waits `REST` still — the docs' hover's —, moving within it never putting it off; with one up, another's shows at
/// once — any of the mouse's cards, `State.warm` (a tooltip's warm hand-off); off them all, the card goes at once and
/// nothing waits. Whether what's drawn changed.
pub fn bring<T: PartialEq>(ed: &mut Editor, fields: Fields<T>, over: Option<T>) -> bool {
    let rests = ed.card_rest && !ed.hover_rest.warm;
    let (up, wait) = fields(ed);
    if over == *up {
        *wait = None;
        return false;
    }
    match over {
        Some(t) if rests => {
            if wait.as_ref().is_none_or(|w| w.0 != t) {
                *wait = Some((t, crate::clock::now() + REST));
                arm(ed, REST);
            }
            false
        }
        over => {
            (*up, *wait) = (over, None);
            true
        }
    }
}

/// A mouse's card waiting (`bring`), the timer awake at `now`: due, its card up if the pointer's still over it (`over`:
/// what's under it now, as the last frame drew) — else what's left of its wait.
pub fn due<T: PartialEq>(
    ed: &mut Editor,
    fields: Fields<T>,
    over: fn(&Editor) -> Option<T>,
    now: Instant,
) -> Option<Duration> {
    let left = fields(ed).1.as_ref()?.1.saturating_duration_since(now);
    if !left.is_zero() {
        return Some(left);
    }
    let (t, _) = fields(ed).1.take()?;
    if over(ed).as_ref() == Some(&t) {
        *fields(ed).0 = Some(t);
        ed.unchanged = false;
    }
    None
}

/// The rest come: its hover asked — with no server ready that does it (or off a name), the problems there alone.
fn land(ed: &mut Editor) {
    let Some(r) = ed.hover_rest.rest.as_mut() else { return };
    r.landed = true;
    let (doc, pos, n, name) = (r.spot.doc, r.spot.word.start, r.n, r.spot.name);
    // (a name in the chat: asked where its answer has it — its card now, once answered, or none)
    if let In::Chat(_) = r.spot.pane {
        crate::chatlink::rest(ed, pos, n);
        ed.unchanged = false;
        return;
    }
    if !(name && ed.lsp_hover_at(doc, pos, n)) {
        failed(ed, n);
        ed.unchanged = false;
    }
}

/// The rest landed now, its time not waited (tests outside a simulation) — the docs' hover asked, a mouse's card
/// waiting up.
#[cfg(test)]
pub fn rested(ed: &mut Editor) {
    if ed.hover_rest.rest.as_ref().is_some_and(|r| !r.landed) {
        land(ed);
    }
    cards_due(ed, crate::clock::now() + REST);
}

/// The server's answer for rest `n` (empty: none), asked of document `doc` — at the version asked of, `on_response`
/// saw to that.
pub fn answered(ed: &mut Editor, n: u64, doc: DocId, text: &str) {
    let doc = ed.docs.iter().find(|d| d.id == doc);
    let lang = doc.and_then(|d| d.syntax.as_ref()).map(|s| s.lang.name.clone());
    shows(ed, n, text, lang.as_deref());
}

/// Rest `n`'s card: markdown `text` (empty: none — the problems alone), its code in `lang`.
pub fn shows(ed: &mut Editor, n: u64, text: &str, lang: Option<&str>) {
    if ed.hover_rest.rest.as_ref().is_none_or(|r| r.n != n) {
        return;
    }
    let docs = match text.is_empty() {
        true => Vec::new(),
        false => crate::markdown::render(text, &ed.theme, lang),
    };
    if let Some(r) = ed.hover_rest.rest.as_mut() {
        (r.card, r.before) = (Some(Card { docs, scroll: 0 }), None);
    }
}

/// The server busy with rest `n` (loading the project: "content modified", as `K`'s asks again): asked once more after
/// another `REST` with the pointer still there — a second time, the problems alone (a resting pointer never wakes an
/// idle tarae on and on).
pub fn busy(ed: &mut Editor, n: u64) {
    let Some(r) = ed.hover_rest.rest.as_mut().filter(|r| r.n == n && !r.again) else { return failed(ed, n) };
    (r.again, r.landed) = (true, false);
    r.due = crate::clock::now() + REST;
    arm(ed, REST);
}

/// No answer for rest `n` (no server, an error, nothing said): the problems there alone, if any — never a word on the
/// status line (the pointer asked nothing).
pub fn failed(ed: &mut Editor, n: u64) {
    if let Some(r) = ed.hover_rest.rest.as_mut().filter(|r| r.n == n) {
        (r.card, r.before) = (Some(Card::default()), None);
    }
}
