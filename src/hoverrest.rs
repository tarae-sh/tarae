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
}

#[cfg(test)]
impl State {
    /// Whether a timer sleeps.
    pub fn armed(&self) -> bool {
        self.armed
    }
}

/// Where the pointer rests: a name (or a problem's underline) of a code pane, as it was drawn then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spot {
    pub pane: PaneId,
    pub doc: DocId,
    pub version: u64,
    /// The name's bytes (a problem's spot off a name: its one character).
    pub word: std::ops::Range<usize>,
    /// Whether it's a name — the server is asked only of one.
    pub name: bool,
    /// The name's first cell on screen.
    pub cell: (usize, usize),
    /// The pane's rect and scroll as drawn — another (a resize, follow moving the view) leaves the card unshown.
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

    /// When the pointer came to rest there (asked again, the server busy: when that was put off).
    pub fn since(&self) -> Instant {
        self.due - REST
    }
}

impl Rest {
    /// The card to draw, if one: answered, and its document and pane as they were.
    pub fn shown<'a>(&'a self, ed: &Editor) -> Option<&'a Card> {
        let card = self.card.as_ref()?;
        let doc = ed.docs.iter().find(|d| d.id == self.spot.doc)?;
        (doc.version() == self.spot.version && view_of(ed, self.spot.pane) == Some(self.spot.view))
            .then_some(card)
    }
}

/// The pane's rect and scroll in the last frame.
fn view_of(ed: &Editor, pane: PaneId) -> Option<(crate::split::Rect, usize, usize)> {
    let rect = ed.screen.panes.iter().find(|p| p.0 == pane)?.1;
    let (top, left) = match ed.workspace.views.iter().find(|v| v.id == pane) {
        Some(v) if pane != ed.workspace.viewer.focus => (v.top, v.left),
        _ => (ed.doc().top, ed.doc().left),
    };
    Some((rect, top, left))
}

/// The name (or underlined problem) at (`x`, `y`) of a code pane (`u`: `mouse_under`'s answer there), if one rests there.
fn spot_at(ed: &Editor, u: Under, x: usize, y: usize) -> Option<Spot> {
    let (Surface::Code(pane), Part::Text { line, col }) = (u.surface, u.part) else { return None };
    // (one card at a time: `K`'s up, or a cut `…`'s card under the pointer, stays theirs)
    if !ed.config.mouse_hover || ed.popup.is_some() || ed.cut_hover.is_some() {
        return None;
    }
    let id = match pane == ed.workspace.viewer.focus {
        true => ed.doc().id,
        false => ed.workspace.views.iter().find(|v| v.id == pane)?.doc,
    };
    let doc = ed.docs.iter().find(|d| d.id == id)?;
    let text = &doc.text;
    let line = line.min(crate::movement::last_line(text));
    let pos = crate::movement::pos_at_col(text, line, col, ed.config.tab_width);
    let c = text.get_byte_slice(pos..).and_then(|s| s.chars().next())?;
    let name = crate::completion::is_word(c);
    let problem = doc.lsp.diagnostics.iter().any(|d| crate::term::underlines(d, pos));
    if !name && !problem {
        return None;
    }
    let word = if name { crate::completion::word_at(text, pos) } else { pos..pos + c.len_utf8() };
    // (its first cell: back from the pointer by what's before it on the row — a name is never wrapped inside)
    let before = text.byte_slice(word.start..pos).to_string().width();
    let x0 = ed.screen.panes.iter().find(|p| p.0 == pane).map_or(0, |p| p.1.x + p.2);
    Some(Spot {
        pane,
        doc: id,
        version: doc.version(),
        word,
        name,
        cell: (x.saturating_sub(before).max(x0), y),
        view: view_of(ed, pane)?,
    })
}

/// The pointer moved to (`x`, `y`) (`Event::Hover`): a new name rests from now, the card of the one left goes — over the
/// card itself it stays. Whether what's drawn changed.
pub fn hovered(ed: &mut Editor, u: Under, x: usize, y: usize) -> bool {
    if over_card(ed, x, y) {
        return false;
    }
    let spot = spot_at(ed, u, x, y);
    if ed.hover_rest.rest.as_ref().is_some_and(|r| Some(&r.spot) == spot.as_ref()) {
        return false;
    }
    let drawn = ed.hover_rest.rest.take().is_some() && ed.screen.rest_card.get().is_some();
    if let Some(spot) = spot {
        let st = &mut ed.hover_rest;
        st.n += 1;
        let due = crate::clock::now() + REST;
        st.rest = Some(Rest { spot, n: st.n, due, landed: false, again: false, card: None });
        arm(ed, REST);
    }
    drawn
}

/// Input came (a key, a press, a paste — your hands elsewhere): the rest and its card go, but the wheel over the card,
/// which scrolls it.
pub fn input(ed: &mut Editor, ev: &Event) {
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
    if let Some(c) = ed.hover_rest.rest.as_mut().and_then(|r| r.card.as_mut()) {
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

/// The timer woke: the rest asked of its server once it's been there `REST` — or slept on for what's left. Nothing drawn
/// unless there's no server to ask and a problem shows alone.
fn tick(ed: &mut Editor) {
    ed.hover_rest.armed = false;
    ed.unchanged = true;
    let Some(r) = ed.hover_rest.rest.as_ref().filter(|r| !r.landed) else { return };
    let left = r.due.saturating_duration_since(crate::clock::now());
    if !left.is_zero() {
        return arm(ed, left);
    }
    land(ed);
}

/// The rest come: its hover asked — with no server ready that does it (or off a name), the problems there alone.
fn land(ed: &mut Editor) {
    let Some(r) = ed.hover_rest.rest.as_mut() else { return };
    r.landed = true;
    let (doc, pos, n, name) = (r.spot.doc, r.spot.word.start, r.n, r.spot.name);
    if !(name && ed.lsp_hover_at(doc, pos, n)) {
        failed(ed, n);
        ed.unchanged = false;
    }
}

/// The rest landed now, its time not waited (tests outside a simulation).
#[cfg(test)]
pub fn rested(ed: &mut Editor) {
    if ed.hover_rest.rest.as_ref().is_some_and(|r| !r.landed) {
        land(ed);
    }
}

/// The server's answer for rest `n` (empty: none) — at the version asked of, `on_response` saw to that.
pub fn answered(ed: &mut Editor, n: u64, text: &str) {
    let Some(r) = ed.hover_rest.rest.as_ref().filter(|r| r.n == n) else { return };
    let doc = ed.docs.iter().find(|d| d.id == r.spot.doc);
    let lang = doc.and_then(|d| d.syntax.as_ref()).map(|s| s.lang.name.clone());
    let docs = match text.is_empty() {
        true => Vec::new(),
        false => crate::markdown::render(text, &ed.theme, lang.as_deref()),
    };
    if let Some(r) = ed.hover_rest.rest.as_mut() {
        r.card = Some(Card { docs, scroll: 0 });
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
        r.card = Some(Card::default());
    }
}
