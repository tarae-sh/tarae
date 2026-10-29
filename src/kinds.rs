//! Every element tarae draws is one of five kinds, and its look says which (CLAUDE.md, the mouse's bullet — the user,
//! 2026-10-28, #215/#216): a **button** (a key hint, a buffer's or session's name, a list row, a mark, a card's answer)
//! — under the pointer its ground lightens, a press does its act; a **reveal** (a cut `…` with no act of its own: blame,
//! a diagnostic at a line's end, a card's body) — its whole text on its card; **text** (code, the transcript, output) — a
//! press puts the cursor, a drag selects; a **handle** (a line between sizes) — half under the pointer, full while
//! dragged; **none**.
//!
//! What a cell is: `Editor::kind_at` — `mouse_under`'s answer there (the one truth of what a press lands on) by
//! `Under::kind`, a reveal where a cut is seen and no press acts. What's drawn to light is recorded where it's drawn, its
//! kind with it (`Screen.elements`, one `record`): the light (`wordspot::lit`) is the element whose act is the press's
//! there, so a word can't light without being pressed nor be pressed unlit. Every key hint printed is recorded too
//! (`hint`, `Screen.hints`): `term::hint_spans` marks its keys (`markdown::Span.hint`) and what prints them records
//! them, as a cut is carried to where it's printed; the rows that draw their hints by hand record theirs. The walk holds
//! each to its kind (`mouse::tests::walk::every_element_reacts_as_its_kind`).

use crate::editor::{Editor, Screen};
use crate::mouse::{Part, Under};
use crate::theme::Style;
use crate::workspace::At;

/// What an element is (the five kinds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A press acts: what it does, as `mouse_under` names it (a word, a row, an answer, a link, a mark).
    Button(Part),
    /// Cut text with no act of its own: its whole on its card.
    Reveal,
    /// A press puts the cursor, a drag selects.
    Text,
    /// A line between sizes, or a floating frame's: it drags.
    Handle,
    None,
}

impl Kind {
    /// Its word in the state (`pointer.kind`) and the walk's lists.
    pub fn word(&self) -> &'static str {
        match self {
            Kind::Button(_) => "button",
            Kind::Reveal => "reveal",
            Kind::Text => "text",
            Kind::Handle => "handle",
            Kind::None => "none",
        }
    }
}

/// An element as drawn: its row, its columns, its kind, and what's drawn there — lit span by span under the pointer.
#[derive(Clone, Debug)]
pub struct Element {
    pub y: usize,
    pub from: usize,
    pub to: usize,
    pub kind: Kind,
    pub spans: Vec<(String, Style)>,
}

/// A key hint as printed: its key's row and columns, the key as written, and the cells of what it says after it (` copy`:
/// pressed too).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub y: usize,
    pub from: usize,
    pub to: usize,
    pub key: String,
    pub what: usize,
    /// What it says it does (`copy`) — a pair whose key costs saying it is no button (`wordspot::costs`).
    pub said: String,
}

thread_local! {
    /// The hints printed since `begin`, until `settle` hands them to the frame (the printers — a top line, a card's rows —
    /// don't hold the editor).
    static HINTS: std::cell::RefCell<Vec<Hint>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A key hint's key `key` printed from (`x`, `y`), by hand — its meaning (`said`, `copy`) printed apart, its cells not
/// the hint's.
pub fn hint(key: &str, said: &str, at: (usize, usize)) {
    record(key, 0, said.trim(), at);
}

/// A key hint's key `key` printed from (`x`, `y`), what it says (`what`, its blank first) right after it.
pub fn pair(key: &str, what: &str, at: (usize, usize)) {
    use unicode_width::UnicodeWidthStr;
    // (what it says: the words after its blank, up to the gap before the next pair)
    let said = what.strip_prefix(' ').map(|w| w.split("  ").next().unwrap_or_default().trim_end());
    record(key, said.map_or(0, |w| 1 + w.width()), said.unwrap_or_default(), at);
}

fn record(key: &str, what: usize, said: &str, (x, y): (usize, usize)) {
    use unicode_width::UnicodeWidthStr;
    let key = key.trim();
    // (in tests, a hint drawn whose press does nothing — or would cost — fails where it's drawn: every printer comes
    // here, a top line's, the status line's, a card's, the follow card's, a pane's line, the chat's)
    #[cfg(test)]
    assert!(
        key.is_empty() || crate::wordspot::buttons_all(key),
        "`{key} {said}` drawn as a key hint, but a press does nothing — list it in `wordspot::BUTTONS`, or draw it \
         as plain words (`COSTS`)"
    );
    #[cfg(test)]
    assert!(
        key.is_empty() || !crate::wordspot::costs(key, said),
        "`{key} {said}` drawn as a key hint, but it costs on a stray press — draw it as plain faint words (`COSTS`)"
    );
    if !key.is_empty() {
        let hint =
            Hint { y, from: x, to: x + key.width(), key: key.to_string(), what, said: said.to_string() };
        HINTS.with_borrow_mut(|h| h.push(hint));
    }
}

/// Span `i` of `spans`, a hint, printed from (`x`, `y`): its key, what the span after it says.
pub fn span_hint(spans: &[crate::markdown::Span], i: usize, at: (usize, usize)) {
    let what = spans.get(i + 1).filter(|s| !s.hint).map_or("", |s| s.text.as_str());
    pair(&spans[i].text, what, at);
}

/// The hints among `spans` (`Span.hint`), printed from (`x`, `y`) as `term::chat_spans` prints them, `w` cells at most.
pub fn hints_in(spans: &[crate::markdown::Span], (x, y): (usize, usize), w: usize) {
    use unicode_width::UnicodeWidthStr;
    let mut used = 0;
    for (i, s) in spans.iter().enumerate() {
        if used >= w {
            break;
        }
        if s.hint && used + s.text.width() <= w {
            span_hint(spans, i, (x + used + s.text.len() - s.text.trim_start().len(), y));
        }
        used += s.text.width();
    }
}

/// A new frame: nothing printed yet.
pub fn begin() {
    HINTS.with_borrow_mut(Vec::clear);
}

/// The frame's hints and its words that do their keys (`Screen.word_spots`) into its records — before the word under
/// the pointer is lit (`term::light_word`).
pub fn settle(ed: &Editor) {
    // (once each: a row printed again painted over — the chat's selection, its matches — records its hints again)
    let mut hints = ed.screen.hints.borrow_mut();
    for h in HINTS.take() {
        if !hints.contains(&h) {
            hints.push(h);
        }
    }
    drop(hints);
    buttons(ed);
    let words: Vec<Element> = (ed.screen.word_spots.borrow().iter().enumerate())
        .map(|(i, s)| Element {
            y: s.y,
            from: s.from,
            to: s.to,
            kind: Kind::Button(Part::Word(crate::mouse::Word::Does(i))),
            spans: s.spans.clone(),
        })
        .collect();
    for e in words {
        ed.screen.record(e);
    }
}

/// Every key hint printed with no word of its own a button (piece B, the user 2026-10-29: nothing drawn as a hint does
/// nothing): its key — each of several side by side (`j k`) its own — and what it says pressed as typed
/// (`wordspot::Press::Keys`), on what it's drawn on (`mouse_under`'s surface there; a modal's status line its own), typed
/// where that surface's keys are (`home`). One already a word, a row, an answer is left as it is.
fn buttons(ed: &Editor) {
    use crate::mouse::Surface as S;
    use crate::wordspot::{Press, Spot};
    use unicode_width::UnicodeWidthStr;
    // (the kinds walk's trap: every hint drawn as no button — `wordspot::UNBUTTONED`)
    #[cfg(test)]
    if crate::wordspot::UNBUTTONED.get() {
        return;
    }
    let hints = ed.screen.hints.borrow().clone();
    let mut made = Vec::new();
    for h in hints {
        let spots = ed.screen.word_spots.borrow();
        if spots.iter().any(|s| s.y == h.y && (s.from..s.to).contains(&h.from)) {
            continue;
        }
        drop(spots);
        let u = ed.mouse_under(crate::event::MouseKind::Down, h.from, h.y);
        let on = match u.surface {
            _ if ed.screen.height == Some(h.y) => S::Status,
            S::Edge => S::SlotLine,
            s => s,
        };
        if on != S::Status && matches!(u.part, Part::Word(_) | Part::Answer(_) | Part::Row(_) | Part::Link(_))
        {
            continue;
        }
        let home = match on {
            S::Side => Some(At::Side),
            S::Rail => Some(At::Rail),
            S::Chat | S::Transcript | S::Input => Some(At::Chat),
            S::SlotLine | S::Output(_) | S::List(_) => Some(At::Slot),
            S::NoteCard | S::Ask | S::Code(_) | S::PaneTop(_) | S::EditReview => Some(At::Code),
            _ => None,
        };
        let spot = |from: usize, to: usize, key: &str| Spot {
            y: h.y,
            from,
            to,
            on,
            press: Press::Keys(key.to_string()),
            spans: Vec::new(),
            home,
        };
        // (a key in no list is no button — it fails where it's drawn in tests: `wordspot::BUTTONS`; nor one whose pair
        // costs, whatever printed it: `wordspot::COSTS`)
        if crate::wordspot::keys(&h.key).is_some() {
            if crate::wordspot::buttons_all(&h.key) && !crate::wordspot::costs(&h.key, &h.said) {
                made.push(spot(h.from, h.to + h.what, &h.key));
            }
            continue;
        }
        // (keys side by side: each its own, its own cells)
        let mut at = h.from;
        for (k, w) in h.key.split(' ').enumerate() {
            let end = at + w.width();
            let button = crate::wordspot::listed(w) && !crate::wordspot::costs(w, &h.said);
            if !w.is_empty() && crate::wordspot::keys(w).is_some() && button {
                let last = k + 1 == h.key.split(' ').count();
                made.push(spot(at, if last { h.to + h.what } else { end }, w));
            }
            at = end + 1;
        }
    }
    ed.screen.word_spots.borrow_mut().extend(made);
}

impl Screen {
    /// An element drawn: its kind where it stands, what's drawn there.
    pub fn record(&self, e: Element) {
        if e.to > e.from && !e.spans.is_empty() {
            self.elements.borrow_mut().push(e);
        }
    }

    /// The element drawn at (`x`, `y`) whose kind is `kind` — the last drawn first.
    pub fn element(&self, kind: Kind, x: usize, y: usize) -> Option<Element> {
        let all = self.elements.borrow();
        all.iter().rev().find(|e| e.kind == kind && e.y == y && (e.from..e.to).contains(&x)).cloned()
    }
}

impl Under {
    /// The kind of what a press lands on.
    pub fn kind(&self) -> Kind {
        match self.part {
            Part::Text { .. } => Kind::Text,
            p @ (Part::Row(_)
            | Part::Word(_)
            | Part::Answer(_)
            | Part::Link(_)
            | Part::Mark(_)
            | Part::More(_)) => Kind::Button(p),
            Part::Border(_) | Part::Frame(_) => Kind::Handle,
            Part::Outside | Part::Blank => Kind::None,
        }
    }
}

impl Editor {
    /// What the cell (`x`, `y`) of the frame last drawn is: what a press there lands on, by its kind — a reveal where
    /// a cut is seen there and a press doesn't act.
    pub fn kind_at(&self, x: usize, y: usize) -> Kind {
        match self.mouse_under(crate::event::MouseKind::Down, x, y).kind() {
            k @ Kind::Button(_) => k,
            _ if crate::whole::under(self, x, y).is_some() => Kind::Reveal,
            k => k,
        }
    }
}
