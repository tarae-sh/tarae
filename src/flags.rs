//! An answer's places in the code (#388, chatlink step 1): the chat keeps the whole answer, the code a flag — a `¶` at
//! the end of each line an answer names (`chatnotes.rs` reads the places), no words: the paragraph is the chat's.
//!
//! - **Lit** in the accent while its paragraph is in the column's view (`lit_sync`, after each frame: the rows the chat
//!   showed, each paragraph by the places its rows name), faint otherwise — the paragraph you read and the code it names
//!   linked without a word.
//! - **A press** on it, `enter` on its line and `space L` there: the column at the newest answer's paragraph about that
//!   line, the keys staying where they are (`to_paragraph`; the chat shown if hidden, opened on its conversation if
//!   closed).
//! - **Its hover** (`hovered`, `term::flag_card`): a borderless card under the `¶`, a row per answer that named the line,
//!   newest first — F0's doubt, the question, the sentence that named the place —, each row a button to that answer's
//!   paragraph. The card stays while the pointer is on the `¶` or on it.
//! - **`]n`/`[n`** walk flags with the notes (`notes::step`): on a flag the cursor moves there, no card, and with the
//!   column in sight its paragraph comes into view.
//! - Kept with the conversation (`chatnotes::places_json`, `restore`), a new conversation clearing them (`forget`).

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::chatdoc::Reading;
use crate::chatnotes::Origin;
use crate::document::DocId;
use crate::editor::Editor;

/// A place an answer named, as the code shows it.
#[derive(Clone, Debug)]
pub struct Flag {
    pub id: u64,
    pub path: PathBuf,
    /// The mark in the open document (follows edits), as a note's.
    pub at: Option<(DocId, u64)>,
    /// 0-based lines, as of the last event.
    pub lines: Range<usize>,
    /// Restored with its conversation: its first line's text then (re-anchors once, as the file opens).
    pub anchor: Option<String>,
    /// Which answer it's of, and where in it.
    pub origin: Origin,
    /// What tarae saw of the place in its turn (focus F0) — None after a continue: nothing said.
    pub check: Option<crate::turnmap::Judged>,
    /// The question its answer answered, its first line (the hover's row).
    pub asked: Option<String>,
    /// The sentence that named the place (the hover's row).
    pub words: String,
}

impl Flag {
    /// Its paragraph in the chat.
    pub fn reading(&self) -> Reading {
        Reading { turn: self.origin.turn, name: self.origin.name.clone(), said: self.origin.said }
    }
}

/// A session's flags, and what the chat's answers leave to place them.
#[derive(Default)]
pub struct Flags {
    pub list: Vec<Flag>,
    next: u64,
    /// The answers this chat has placed flags for — the newest's number (`Origin.n`).
    pub answers: u64,
    /// How many there were as the conversation last kept them (`chatnotes::kept_sync`).
    pub kept: usize,
    /// The chat closed, its answers' flags still here: their conversation as it stood then and how many messages of yours
    /// it holds (`chat::keep_places`), until the chat opens again.
    pub left: Option<(Box<crate::chats::Saved>, usize)>,
    /// The last answer's places, waiting for what their names come to (`chatnotes::sync`).
    pub answer: Option<crate::chatnotes::Waiting>,
    /// The paragraphs the column showed in the last frame (`lit_sync`): their flags lit.
    pub lit: Vec<Reading>,
    /// The flag `]n`/`[n` stepped to: where the walk goes on from while the cursor is on its line.
    pub focus: Option<u64>,
}

/// A flag's `¶` as a frame drew it (`Screen.flag_ends`): its row, its column, the flag, its pane's left and right columns.
pub type End = (usize, usize, u64, usize, usize);

/// The `¶` the mouse is over, or the card it opened (`hovered`): that flag's line, where its `¶` was drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hover {
    pub id: u64,
    pub x: usize,
    pub y: usize,
}

/// A flag on `lines` of `path`, of the answer `origin`.
pub fn add(
    ed: &mut Editor,
    (path, lines): (&Path, Range<usize>),
    origin: Origin,
    check: Option<crate::turnmap::Judged>,
    (asked, words): (Option<String>, String),
) -> u64 {
    let flags = &mut ed.workspace.claude.flags;
    flags.next += 1;
    flags.answers = flags.answers.max(origin.n);
    let anchor = origin.text.clone();
    let id = flags.next;
    flags.list.push(Flag {
        id,
        path: path.to_path_buf(),
        at: None,
        lines,
        anchor,
        origin,
        check,
        asked,
        words,
    });
    id
}

pub fn find(ed: &Editor, id: u64) -> Option<&Flag> {
    ed.workspace.claude.flags.list.iter().find(|f| f.id == id)
}

/// A new conversation, or another continued: the flags of the one left go.
pub fn forget(ed: &mut Editor) {
    let flags = &mut ed.workspace.claude.flags;
    (flags.list, flags.answer, flags.left, flags.focus) = (Vec::new(), None, None, None);
    (flags.answers, flags.kept) = (0, 0);
    ed.flag_hover = None;
}

/// Every event: open documents get marks for their flags, and every flag's lines follow its mark (a note's rule,
/// `notes::follow_mark`).
pub fn attach(ed: &mut Editor) {
    let Editor { workspace, docs, .. } = ed;
    for f in workspace.claude.flags.list.iter_mut() {
        crate::notes::follow_mark((&mut f.at, &mut f.lines, &mut f.anchor), &f.path, docs);
    }
}

/// Marks flags point at (the jump list's pruning keeps them).
pub fn marks(ed: &Editor) -> Vec<(DocId, u64)> {
    ed.workspace.claude.flags.list.iter().filter_map(|f| f.at).collect()
}

/// The flags starting on `line` of `path`, newest answer first — one `¶`, the first's.
pub fn on_line<'a>(flags: &'a Flags, path: &Path, line: usize) -> Vec<&'a Flag> {
    let mut v: Vec<&Flag> = flags.list.iter().filter(|f| f.path == path && f.lines.start == line).collect();
    crate::sorting::by_key(&mut v, |f| (std::cmp::Reverse(f.origin.n), f.id));
    v
}

/// The flag at your cursor's line: the newest answer's there.
pub fn here(ed: &Editor) -> Option<&Flag> {
    let path = ed.doc().path.as_deref()?;
    on_line(&ed.workspace.claude.flags, path, ed.cursor_line()).first().copied()
}

/// The answer's paragraph about your line, for `space L`: the newest answer's flag there that names a place (a
/// walk-through's stop names none), else the newest's. None: no flag on your line.
pub fn answer_here(ed: &Editor) -> Option<Reading> {
    let path = ed.doc().path.as_deref()?;
    let all = on_line(&ed.workspace.claude.flags, path, ed.cursor_line());
    all.iter().find(|f| !f.origin.name.is_empty()).or(all.first()).map(|f| f.reading())
}

/// Whether flag `f`'s paragraph was in the column's view as the last frame drew it.
pub fn lit(flags: &Flags, f: &Flag) -> bool {
    flags.lit.iter().any(|r| *r == f.reading())
}

/// The column at flag `id`'s paragraph, the keys where they are: a press on its `¶` or a row of its hover, `enter` on its
/// line. Hidden, the chat is shown; closed, opened on the conversation it left.
pub fn to_paragraph(ed: &mut Editor, id: u64) {
    let Some(at) = find(ed, id).map(Flag::reading) else { return };
    ed.workspace.claude.flags.focus = Some(id);
    ed.flag_hover = None;
    if !crate::chat::reopen_left(ed) {
        return ed.set_status("this answer's conversation isn't kept — C-p lists the ones that are");
    }
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    // (hidden by `space L`: shown — #270 — as the conversation, never the ask line)
    (c.hidden, c.asking) = (false, false);
    c.reading = Some((at, false));
    ed.unchanged = false;
}

/// `enter` on a line with a flag and no note (`notes::card_key`): the column at its newest answer's paragraph.
pub fn enter(ed: &mut Editor) -> bool {
    let Some(id) = here(ed).map(|f| f.id) else { return false };
    crate::follow::your_view(ed);
    to_paragraph(ed, id);
    true
}

/// `]n`/`[n` landed on flag `id`: the cursor at its line (no card — its words are the chat's), and with the column in
/// sight its paragraph shown there, the keys staying in the code.
pub fn stepped(ed: &mut Editor, id: u64) {
    let Some((path, line, at)) = find(ed, id).map(|f| (f.path.clone(), f.lines.start, f.reading())) else {
        return;
    };
    ed.workspace.claude.flags.focus = Some(id);
    if let Err(e) = ed.open_at(&path, line, 0) {
        return ed.set_error(format!("{e:#}"));
    }
    if crate::chat::column_in_sight(ed)
        && let Some(c) = ed.workspace.claude.chat.as_mut()
    {
        c.reading = Some((at, false));
    }
}

/// After a frame (`term::render_drawing`): the paragraphs the column showed — each a source line of an answer, by the
/// places its rows name (`chatdoc::in_view`). True where that changed: drawn again, the flags lit as it is.
pub fn lit_sync(ed: &mut Editor) -> bool {
    let now = match (ed.workspace.claude.chat.as_ref(), ed.screen.chat_view.get()) {
        (Some(c), Some((_, _, area, from))) if !c.hidden && !ed.workspace.claude.flags.list.is_empty() => {
            crate::chatdoc::in_view(c, &ed.screen.chat_laid.borrow(), from, area)
        }
        _ => Vec::new(),
    };
    let flags = &mut ed.workspace.claude.flags;
    let changed = now != flags.lit;
    flags.lit = now;
    changed
}

/// The mouse at (`x`, `y`) (`Event::Hover`): on a flag's `¶` its card opens; on the card it stays; elsewhere it goes.
/// Whether that changed — a move that changes nothing draws nothing.
pub fn hovered(ed: &mut Editor, x: usize, y: usize) -> bool {
    let s = &ed.screen;
    let on_card = s
        .flag_card
        .get()
        .is_some_and(|(cx, cy, w, h)| (cx..cx + w).contains(&x) && (cy..cy + h).contains(&y));
    // (a `¶` the press would reach: none under a card or a picker)
    let end = match ed.mouse_under(crate::event::MouseKind::Down, x, y).part {
        crate::mouse::Part::Word(crate::mouse::Word::Does(i)) => {
            let spot = s.word_spots.borrow().get(i).map(|w| (w.y, w.from, w.press.clone()));
            let ends = s.flag_ends.borrow();
            spot.and_then(|(y, x, p)| match p {
                crate::wordspot::Press::Flag(id) if ends.iter().any(|e| (e.0, e.1, e.2) == (y, x, id)) => {
                    Some(Hover { id, x, y })
                }
                _ => None,
            })
        }
        _ => None,
    };
    let now = end.or(ed.flag_hover.filter(|_| on_card));
    let changed = now != ed.flag_hover;
    ed.flag_hover = now;
    changed
}

/// A frame drawn while a flag's card is up: its `¶` still where it was? A scroll, an edit, the flag gone take it from
/// under a mouse that didn't move — then it goes. Whether it changed (drawn again).
pub fn hover_drawn(ed: &mut Editor) -> bool {
    let Some(h) = ed.flag_hover else { return false };
    let here = ed.screen.flag_ends.borrow().iter().any(|e| (e.0, e.1, e.2) == (h.y, h.x, h.id));
    if !here {
        ed.flag_hover = None;
    }
    !here
}

/// What flag `f`'s hover row says: what tarae saw of the place, every verdict (`doc` its file's buffer — focus F0; the
/// chat beside the place says only the doubts), the question's first line, the sentence that named the place.
pub fn row_of(f: &Flag, doc: Option<&crate::document::Document>) -> (Option<&'static str>, String, String) {
    let saw = f.check.as_ref().zip(doc).map(|(c, d)| c.word(d));
    let asked = f.asked.as_deref().map_or("an answer".to_string(), crate::chatnotes::plain_words);
    (saw, asked, crate::chatnotes::plain_words(&f.words))
}

/// F0's doubt about a place an answer's row names (chatlink Q3): the flag of that answer (`turn`, its question's place
/// among yours) at that name and line, where tarae has one to say — `doc` finds the file's buffer.
pub fn doubt_of<'a>(
    flags: &Flags,
    (turn, name, said): (usize, &str, usize),
    doc: impl Fn(&Path) -> Option<&'a crate::document::Document>,
) -> Option<&'static str> {
    let f = flags
        .list
        .iter()
        .find(|f| f.origin.turn == turn && &*f.origin.name == name && f.origin.said == said)?;
    f.check.as_ref().zip(doc(&f.path)).and_then(|(c, d)| c.doubt(d))
}

#[cfg(test)]
mod tests;
