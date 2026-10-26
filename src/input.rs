//! A one-place input as a buffer (panes P2c, `.sb/panes-plan.md` §2.1e; the prompt's since #153): a small document in
//! insert mode, with its own keys' state (`docpane::Pane`) — the chat's input (`chatinput.rs`) and the prompt's line
//! (`prompt.rs`) are each one. What a key does to it runs with its document swapped in (`docpane::swap`), so the
//! editor's own insert-mode commands edit it; what's drawn of it and where a press lands are its own.

use crate::docpane::Pane;
use crate::document::{DocId, Document};
use crate::editor::Mode;
use crate::key::Key;
use crate::selection::Selection;
use crate::transaction::{Change, Transaction};
use unicode_width::UnicodeWidthChar as _;

/// An input: its document pane, always in insert mode.
pub struct Input {
    pub pane: Pane,
}

/// The chat's input (`document::CHAT_INPUT`) — the prompt's is `Input::new(document::PROMPT_INPUT)`.
impl Default for Input {
    fn default() -> Input {
        Input::new(crate::document::CHAT_INPUT)
    }
}

/// The chat's input holding `text`, the cursor at its end.
impl From<&str> for Input {
    fn from(text: &str) -> Input {
        let mut i = Input::default();
        i.set(text);
        i
    }
}

impl From<String> for Input {
    fn from(text: String) -> Input {
        Input::from(text.as_str())
    }
}

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.text())
    }
}

impl Clone for Input {
    /// The same text and selection, nothing under way.
    fn clone(&self) -> Input {
        let d = self.pane.doc();
        let mut i = Input::new(d.id);
        i.set(&self.text());
        i.pane.doc_mut().set_selection(d.selection().clone());
        i
    }
}

impl PartialEq<&str> for Input {
    fn eq(&self, other: &&str) -> bool {
        self.pane.doc().text == *other
    }
}

impl Input {
    /// An empty input, its document numbered `id` (one of `document`'s inputs).
    pub fn new(id: DocId) -> Input {
        let mut doc = Document::from_str(id, "");
        doc.throwaway = true;
        // (its lines as written: `↑`/`↓` move by them, the drawer wraps them)
        doc.wrap = Some(false);
        let mut pane = Pane::new(doc);
        pane.keys.mode = Mode::Insert;
        Input { pane }
    }

    /// What's typed.
    pub fn text(&self) -> String {
        self.pane.doc().text.to_string()
    }

    pub fn is_empty(&self) -> bool {
        self.pane.doc().text.len_bytes() == 0
    }

    /// Nothing typed but blanks — nothing to send.
    pub fn blank(&self) -> bool {
        self.pane.doc().text.chars().all(char::is_whitespace)
    }

    /// Where the cursor is (bytes).
    pub fn cursor(&self) -> usize {
        self.pane.doc().selection().primary().head
    }

    /// `text` in its place, the cursor at `cursor` (on a character boundary, else its end). A sequence of
    /// `[insert]`'s under way goes with what it replaces.
    pub fn set_at(&mut self, text: &str, cursor: usize) {
        self.pane.keys.pending.clear();
        let d = self.pane.doc_mut();
        let tx = Transaction::new(vec![Change { from: 0, to: d.text.len_bytes(), insert: text.to_string() }]);
        d.apply(&tx);
        let at = if text.is_char_boundary(cursor) { cursor.min(text.len()) } else { text.len() };
        d.set_selection(Selection::point(at));
    }

    /// `text` in its place, the cursor at its end.
    pub fn set(&mut self, text: &str) {
        self.set_at(text, text.len());
    }

    pub fn clear(&mut self) {
        self.set("");
    }

    /// `text` typed at the cursor, the cursor after it (a paste, a key that brought the chat back) — after what a
    /// sequence under way had typed (`j` of `j k`: a paste breaks it off, as a key does).
    pub fn insert_str(&mut self, text: &str) {
        let text = self.held() + text;
        if text.is_empty() {
            return;
        }
        let d = self.pane.doc_mut();
        let at = d.selection().primary().head;
        d.apply(&Transaction::new(vec![Change::insert(at, text.clone())]));
        d.set_selection(Selection::point(at + text.len()));
    }

    /// What a sequence under way had typed, taken out of it (its keys that are characters).
    fn held(&mut self) -> String {
        std::mem::take(&mut self.pane.keys.pending).iter().filter_map(Key::plain_char).collect()
    }

    /// Laid out as the chat draws it, `w` its text's width.
    pub fn laid(&self, w: usize) -> Laid {
        let text = self.text();
        let rows = rows(&text, w);
        Laid { text, rows, cursor: self.cursor() }
    }

    /// The character drawn at column `col` of row `row` (`laid`, `w` wide): the one whose cells hold it — never past a
    /// wrapped row's end, its last (the cell after it is the next row's first, as in the code); past a line's last
    /// row, its end (the line break — the cursor there is after its last character).
    pub fn at(&self, row: usize, col: usize, w: usize) -> usize {
        let laid = self.laid(w);
        let i = row.min(laid.height() - 1);
        let (text, start) = laid.row(i);
        let col = crate::wrap::clamp_col(col, laid.end_col(i));
        self.at_col(text, start, col)
    }

    /// The character whose cells hold column `col` of `text` (a piece of the input from byte `start`, drawn from its
    /// first cell), else the piece's end.
    pub fn at_col(&self, text: &str, start: usize, col: usize) -> usize {
        let mut cells = 0;
        let pos = text
            .char_indices()
            .find_map(|(b, ch)| {
                cells += ch.width().unwrap_or(0);
                (cells > col).then_some(start + b)
            })
            .unwrap_or(start + text.len());
        crate::graphemes::snap(&self.pane.doc().text, pos)
    }
}

/// The input laid out for one frame or one press: its text, read once, and its rows — byte ranges made from that text
/// (`rows`) and kept beside it, private, so they never slice another string: every one starts and ends on that
/// text's character boundaries (`char_indices`' or its end), and slicing by them can't panic.
pub struct Laid {
    text: String,
    rows: Vec<std::ops::Range<usize>>,
    /// The input's cursor in that text (a byte on its character boundary, a document's position).
    cursor: usize,
}

impl Laid {
    /// How many rows (one at least).
    pub fn height(&self) -> usize {
        self.rows.len()
    }

    /// Row `i`'s text and its first byte in the input.
    pub fn row(&self, i: usize) -> (&str, usize) {
        let r = &self.rows[i];
        (&self.text[r.clone()], r.start)
    }

    /// The row and column the cursor is drawn at — on a row's end, that row's (a wrapped row's end is the next one's
    /// start).
    pub fn cursor(&self) -> (usize, usize) {
        let at = self.rows.iter().position(|r| r.end >= self.cursor).unwrap_or(self.rows.len() - 1);
        let r = &self.rows[at];
        let upto = self.cursor.clamp(r.start, r.end);
        (at, self.text[r.start..upto].chars().map(|c| c.width().unwrap_or(0)).sum())
    }

    /// The column row `i` stops before (`wrap::Row.end_col`): a wrapped row's width, else none (`usize::MAX` — the
    /// cell after a line's text is its line break's).
    fn end_col(&self, i: usize) -> usize {
        let r = &self.rows[i];
        match self.text[r.end..].starts_with('\n') || r.end == self.text.len() {
            true => usize::MAX,
            false => self.text[r.clone()].chars().map(|c| c.width().unwrap_or(0)).sum(),
        }
    }
}

/// The input's `text` wrapped as the chat draws it, `w` its text's width (the prompt's two cells off, the caret's
/// among them): each row's bytes, without the line break that ends one.
fn rows(text: &str, w: usize) -> Vec<std::ops::Range<usize>> {
    let room = w.saturating_sub(2);
    let mut rows = Vec::new();
    let (mut start, mut col) = (0, 0);
    for (b, ch) in text.char_indices() {
        let cw = ch.width().unwrap_or(0);
        if ch == '\n' || col + cw > room {
            rows.push(start..b);
            col = 0;
            start = b;
            if ch == '\n' {
                start += 1;
                continue;
            }
        }
        col += cw;
    }
    rows.push(start..text.len());
    rows
}
