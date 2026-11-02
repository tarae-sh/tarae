//! The chat's transcript as a document (panes P2c, `.sb/panes-plan.md` §2.1e): the conversation's text, a
//! read-only `Document` that follows what each frame lays out (`sync`) — one line a paragraph, as the code's lines
//! under soft wrap: the chat's layout wraps it into its rows, and those rows are the document's (`wrap::Given`), so
//! `j`/`k`, the pages and the `z` moves step by what's drawn, while search, `w` and `x` cross a wrap and a resize
//! moves no cursor.
//!
//! What the drawer adds is never text: the user's bar, a thought's bar, a note's `· `/`≈ `, a code well's edge, a
//! wrapped row's hanging indent (`ChatRow.skip`), a tool row's knot, a change's stamps, the writing caret
//! (`ChatRow.tail`), the bleeding head (a style). The air before a question or a run of tool rows is an empty line; the
//! frame's own rows — what a live run waits on, `thinking…`, the empty chat's words (`Of::Own`) — aren't in it,
//! never under the cursor. A message's lines are made again only when its rows are (`Msg.cache`: the `Arc` the
//! layout keeps) — the one being written, mostly — and replace only its own lines when their text changed.
//!
//! It's kept as the chat's own drawer's (`term::draw_chat` paints its rows' spans, the selections over them by the
//! rows' places here), not `draw_text`'s: its rows are markdown's spans.
//!
//! The chat's view mode is normal mode on it (`esc` from the input, `space >`, a press on it; `i` back, `esc` again to
//! the code — the wheel only scrolls: never the keys, CLAUDE.md's mouse bullet): the keymap's chat table — normal mode whole, `[view]` and `[view.chat]` over it — run with the
//! document swapped in (`docpane::swap`); an edit put back and said. While it has the keys it holds still — what
//! comes is appended below what's in sight, said (`● normal · new text after`), `G` catches up; without them the chat
//! shows its newest rows, `Chat.scroll` up from the bottom. Its selection is where you were: the next view lands
//! there, in this conversation.

use std::sync::Arc;

use unicode_width::UnicodeWidthStr;

use crate::chat::{Chat, ChatRow};
use crate::docpane::{After, Pane, Which};
use crate::document::Document;
use crate::editor::{Editor, Mode};
use crate::key::{Code, Key};
use crate::keymap::MappableCommand;
use crate::movement as mv;
use crate::selection::{Range, Selection};
use crate::theme::Style;
use crate::transaction::{Assoc, Change, Transaction};
use crate::view::Surface;
use crate::workspace::At;
use crate::wrap::{Given, Part, Row};

/// What a run of the chat's rows is (`Laid`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Of {
    /// Message `i`'s rows (`Chat.msgs`).
    Msg(usize),
    /// The air before a question or a run of tool rows — where a message is, not what it is: an empty line.
    Air,
    /// The message waiting for this turn's end (`Chat.queued`): its words, the transcript's last — drawn after the
    /// frame's own rows (what a run waits on, `thinking…`), right over the input.
    Queued,
    /// The frame's own: no text of the document.
    Own,
}

/// The chat's rows as a frame laid them out, in order: each message's kept rows, the air between them, the frame's
/// own (`term::chat_laid`, then `draw_chat`'s; `Screen.chat_laid`).
pub type Laid = Vec<(Of, Arc<[ChatRow]>)>;

/// A row's place in its piece's text: its line (from the piece's first), where its text starts on the line and
/// its display column there, the bytes of it drawn (a cut code row's `…` stands for more), the cells before it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Place {
    pub line: usize,
    pub from: usize,
    pub col: usize,
    pub len: usize,
    pub x0: usize,
}

/// A row's text as the document has it: its characters past `skip` and before `tail`, line breaks and tabs as
/// spaces (one byte each — the painter counts the same bytes in its spans), blanks at its end off; and what a cut
/// code row's `…` stands for (`rest`), the `…` itself not the text's.
fn body(r: &ChatRow) -> (String, Option<&str>) {
    let n: usize = r.spans.iter().map(|s| s.text.chars().count()).sum();
    let keep = n.saturating_sub(r.tail);
    let chars = r.spans.iter().flat_map(|s| s.text.chars()).take(keep).skip(r.skip);
    let mut s: String = chars.map(|c| if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c }).collect();
    // (a code well's padding after a cut row's `…` isn't the text either)
    s.truncate(s.trim_end().len());
    match r.rest.as_deref() {
        Some(rest) => {
            if s.ends_with('…') {
                s.truncate(s.len() - '…'.len_utf8());
            }
            (s, Some(rest))
        }
        None => (s, None),
    }
}

/// A row drawn cut whose text is `whole` (`whole.rs`: what's cut is whole where you look at it): the document keeps
/// `whole` — the drawn text's start that is `whole`'s is the row's text, the rest of `whole` what it stands for
/// (`ChatRow.rest`), and what's drawn past that start (the `…`, a kept end, what follows it) the drawer's
/// (`ChatRow.tail`): never painted, never under the cursor — a cursor on the row shows it whole on its card. Not cut: as
/// it is.
pub fn keep_whole(r: &mut ChatRow, whole: &str) {
    if body(r).0 == whole {
        return;
    }
    let n: usize = r.spans.iter().map(|s| s.text.chars().count()).sum();
    let drawn: Vec<char> =
        r.spans.iter().flat_map(|s| s.text.chars()).take(n.saturating_sub(r.tail)).collect();
    let drawn = drawn.get(r.skip.min(drawn.len())..).unwrap_or_default();
    let mut same = drawn.iter().zip(whole.chars()).take_while(|(a, b)| **a == *b).count();
    // (never ending in blanks or a `…`: `body` trims the one and takes the other for a cut's)
    while same > 0 && (drawn[same - 1].is_whitespace() || drawn[same - 1] == '…') {
        same -= 1;
    }
    let at: usize = whole.chars().take(same).map(char::len_utf8).sum();
    r.rest = Some(whole[at..].into());
    r.tail = n - (r.skip + same).min(n);
}

/// The cells of `r`'s first `skip` characters — what the drawer put before its text.
fn lead(r: &ChatRow) -> usize {
    let s: String = r.spans.iter().flat_map(|s| s.text.chars()).take(r.skip).collect();
    s.width()
}

/// A piece's rows as the document's text — a row that goes on from the one before (`ChatRow.join`) on its line,
/// the space its break took put back — each line ending in a line break; and each row's place in it.
pub fn lines_of(rows: &[ChatRow]) -> (String, Vec<Place>) {
    let mut text = String::new();
    let mut at = Vec::with_capacity(rows.len());
    let (mut line, mut start) = (0, 0);
    for (k, r) in rows.iter().enumerate() {
        match r.join.filter(|_| k > 0) {
            Some(space) => text.push_str(if space { " " } else { "" }),
            None if k > 0 => {
                text.push('\n');
                (line, start) = (line + 1, text.len());
            }
            None => {}
        }
        let (shown, rest) = body(r);
        let from = text.len() - start;
        let col = text[start..].width();
        text.push_str(&shown);
        at.push(Place { line, from, col, len: shown.len(), x0: lead(r) });
        text.push_str(rest.unwrap_or_default());
    }
    if !rows.is_empty() {
        text.push('\n');
    }
    (text, at)
}

/// A note's card read on in the chat (focus A1c, `notes::read_on`): its answer's paragraph, by the question's place among
/// your messages (`Origin.turn`) and the place as written with its first line (`Origin.name`, `Origin.said`) — held
/// unfolded, the chat taken to it once a frame laid it out (`reading`). A flow at a stop has no name: its answer's last
/// message.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Reading {
    pub turn: usize,
    pub name: Arc<str>,
    pub said: usize,
}

/// After a frame (`term::render`): the chat taken to the paragraph a note's card read on to (`Chat.reading`), once its
/// rows are laid out — a row of air above it; its view mode's rows where it's on, else the chat's own from the bottom.
/// The keys stay where they are. True when it moved (drawn again).
pub fn reading(ed: &mut Editor) -> bool {
    use crate::chat::Role;
    let Some(c) = ed.workspace.claude.chat.as_ref() else { return false };
    let Some((at, false)) = c.reading.clone() else { return false };
    let laid = ed.screen.chat_laid.borrow().clone();
    if ed.screen.chat_view.get().is_none() || laid.is_empty() {
        return false;
    }
    // (each message's question's place among yours, as `Fold::of` counts it)
    let mut users = 0;
    let turn: Vec<usize> = (c.msgs.iter())
        .map(|m| {
            users += usize::from(m.role == Role::User);
            users.saturating_sub(1)
        })
        .collect();
    let of =
        |i: usize| c.msgs.get(i).is_some_and(|m| m.role == Role::Assistant) && turn.get(i) == Some(&at.turn);
    let last = (0..c.msgs.len()).rev().find(|&i| of(i));
    let (mut row, mut found) = (0, None);
    for (o, rows) in &laid {
        if let Of::Msg(i) = o
            && of(*i)
        {
            let named =
                |r: &crate::chat::ChatRow| r.places.iter().any(|p| *p.name == *at.name && p.line == at.said);
            let hit = match at.name.is_empty() {
                true => (Some(*i) == last).then_some(0),
                false => rows.iter().position(named),
            };
            if let Some(k) = hit {
                // (its paragraph's first row: a wrapped row goes on from the one above)
                let k = (0..=k).rev().find(|&j| rows[j].join.is_none()).unwrap_or(k);
                found = Some(row + k);
                break;
            }
        }
        row += rows.len();
    }
    let Some(target) = found.map(|r| r.saturating_sub(1)) else { return false };
    let area = ed.screen.chat_view.get().map_or(20, |a| a.2.max(1));
    let total: usize = laid.iter().map(|(_, r)| r.len()).sum();
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return false };
    // (done once it moved: the view mode's rows not laid yet, the next frame tries again)
    match c.transcript.as_mut().filter(|t| t.keyed) {
        Some(t) => {
            let Some(d) = t.pane.doc_in_mut() else { return false };
            let Some(g) = d.rows.clone() else { return false };
            let Some(r) = doc_row(&laid, target) else { return false };
            let (line, k) = g.place(r.min(g.len().saturating_sub(1)));
            d.set_top(line);
            d.top_rows = (k > 0).then_some((line, k));
            // (held there as the wheel holds it: its cursor stays where it is, out of sight, until a key moves it)
            d.wheel_held = Some((d.selection().primary(), d.version()));
        }
        None => {
            c.scroll = total.saturating_sub(area).saturating_sub(target);
            // (follow reading along the chat goes from here: not a scroll of yours — `follow::reading`)
            if c.follow.read_at.is_some() {
                c.follow.read_at = Some(c.scroll);
            }
        }
    }
    c.reading = Some((at, true));
    true
}

/// What the chat folds this frame (focus A1b step 3): the conversation reads as one — the newest answer whole until the
/// next one has come, an older answer's paragraphs whose words a note holds at the code folded to one stub row
/// (`◂ main.rs:11  first words…`), whole on its cut card under the mouse or the view mode's cursor. The document keeps
/// every word: the row's text is the paragraph's start, what it folds its `rest` (`keep_whole`), so search and `y` find
/// and copy all of it. A selection never opens it (#271, #269's rule): a drag onto it takes its paragraph whole
/// (`drag`), a selection wider than a character on its line lights its words (`term::chat_paint`); it opens only by its
/// own acts — `enter` and a press go to its place, `notes::read_on` holds it open (`reading`).
#[derive(Debug, Default)]
pub struct Fold {
    /// The messages before it are older answers': the question of the newest answer that's done.
    upto: usize,
    /// Each message's question's place among your messages, while there's something to fold.
    turns: Vec<usize>,
    stubs: Vec<crate::chatnotes::Stub>,
    /// Each turn's stubs hashed, by turn: what a message's rows depend on, worked out once a frame.
    keys: Vec<(usize, u64)>,
    /// The paragraph a note's card read on to (`Chat.reading`): its stub held open.
    reading: Option<Reading>,
}

impl Fold {
    pub fn of(c: &Chat, notes: &crate::notes::Notes) -> Fold {
        use crate::chat::Role;
        // (the turn under way isn't an answer yet: the one before it stays whole until it's done)
        let end = match c.busy() {
            true => c.msgs.iter().rposition(|m| m.role == Role::User).unwrap_or(c.msgs.len()),
            false => c.msgs.len(),
        };
        let answer = c.msgs[..end].iter().rposition(|m| m.role == Role::Assistant);
        let upto = answer.and_then(|a| c.msgs[..a].iter().rposition(|m| m.role == Role::User)).unwrap_or(0);
        let stubs = crate::chatnotes::stubs(notes);
        if upto == 0 || stubs.is_empty() {
            return Fold::default();
        }
        let mut users = 0;
        let turns = (c.msgs.iter())
            .map(|m| {
                users += usize::from(m.role == Role::User);
                users.saturating_sub(1)
            })
            .collect();
        let mut keys: Vec<(usize, u64)> = Vec::new();
        for chunk in stubs.chunk_by(|a, b| a.turn == b.turn) {
            use std::hash::{Hash, Hasher};
            let mut h = std::hash::DefaultHasher::new();
            chunk.hash(&mut h);
            keys.push((chunk[0].turn, h.finish()));
        }
        let reading = c.reading.as_ref().map(|r| r.0.clone());
        Fold { upto, turns, stubs, keys, reading }
    }

    /// What of message `i` folds, its stubs; None: it stays whole.
    fn of_msg(&self, i: usize) -> Option<&[crate::chatnotes::Stub]> {
        let turn = *self.turns.get(i).filter(|_| i < self.upto)?;
        let from = self.stubs.partition_point(|s| s.turn < turn);
        let to = self.stubs.partition_point(|s| s.turn <= turn);
        (from < to).then(|| &self.stubs[from..to])
    }

    /// What message `i`'s rows depend on here (`LaidKey.state`) — the notes that hold its paragraphs, the one read on to;
    /// 0: it stays whole.
    pub fn key(&self, i: usize) -> u64 {
        if self.of_msg(i).is_none() {
            return 0;
        }
        let turn = self.turns[i];
        let at = self.keys.partition_point(|k| k.0 < turn);
        let mut key = self.keys[at].1;
        // (a paragraph read on to from its note's card, held open)
        if let Some(r) = self.reading.as_ref().filter(|r| r.turn == turn) {
            use std::hash::{Hash, Hasher};
            let mut h = std::hash::DefaultHasher::new();
            r.hash(&mut h);
            key = key.rotate_left(11) ^ h.finish();
        }
        key | 1
    }

    /// `rows` of message `i`, its places marked, each paragraph whose note holds its words one stub row `w` wide. Only the
    /// paragraph that first names a place: a later naming's words are on no card, so they stay here. Two stubs in a
    /// row have no air between them: its blank line goes from the document with its row (every line has a row).
    pub fn rows(
        &self,
        c: &Chat,
        i: usize,
        rows: Vec<ChatRow>,
        w: usize,
        styles: (Style, Style),
    ) -> Vec<ChatRow> {
        use crate::chat::Role;
        let Some(stubs) = self.of_msg(i) else { return rows };
        let start = c.msgs[..i].iter().rposition(|m| m.role == Role::User).map_or(0, |u| u + 1);
        let words = c.msgs[start..i].iter().filter(|m| m.role == Role::Assistant);
        let mut seen: Vec<(Arc<str>, usize)> = words
            .flat_map(|m| {
                let names = crate::places::names_in(&m.text);
                let name = |n: (std::ops::Range<usize>, std::ops::Range<usize>, usize)| {
                    (Arc::from(&m.text[n.1]), n.2)
                };
                names.into_iter().map(name).collect::<Vec<_>>()
            })
            .collect();
        let mut out: Vec<ChatRow> = Vec::with_capacity(rows.len());
        let mut k = 0;
        while k < rows.len() {
            let end = k + 1 + rows[k + 1..].iter().take_while(|r| r.join.is_some()).count();
            let para = &rows[k..end];
            k = end;
            let text = lines_of(para).0;
            let text = text.trim_end_matches('\n');
            if text.trim().is_empty() {
                out.extend_from_slice(para);
                continue;
            }
            let names = || para.iter().flat_map(|r| &r.places);
            let place = names().find_map(|n| {
                let new = !seen.iter().any(|s| *s.0 == *n.name && s.1 == n.line);
                stubs.iter().find(|s| new && *s.name == *n.name && s.said == n.line)
            });
            let read = |s: &&crate::chatnotes::Stub| {
                self.reading
                    .as_ref()
                    .is_some_and(|r| (r.turn, &*r.name, r.said) == (s.turn, &*s.name, s.said))
            };
            let held = place.is_some_and(|s| read(&s));
            seen.extend(names().map(|n| (n.name.clone(), n.line)));
            match place.filter(|_| !held && para.iter().all(|r| r.prose)) {
                Some(s) => {
                    // (only air: a list's item between two stubs has none around it, and is words)
                    let air = out.last().is_some_and(|r| r.spans.is_empty());
                    if air && out.len() >= 2 && out[out.len() - 2].stub.is_some() {
                        out.pop();
                    }
                    out.push(stub_row(para, text, s, w, styles));
                }
                None => out.extend_from_slice(para),
            }
        }
        out
    }
}

/// The least of a paragraph's first words a stub shows: less room, its place alone.
const STUB_WORDS_MIN: usize = 8;

/// `para`'s one row, `w` wide: `◂ main.rs:11  first words…`, faint, a link to its place, `whole` its text — cut first by
/// its words, then its place — and whole on its card in the answer's own colours, `card` under them.
fn stub_row(
    para: &[ChatRow],
    whole: &str,
    s: &crate::chatnotes::Stub,
    w: usize,
    (faint, card): (Style, Style),
) -> ChatRow {
    use crate::markdown::Span;
    let file = s.path.file_name().map_or_else(String::new, |f| f.to_string_lossy().into_owned());
    let place = format!("{file}:{}", s.line + 1);
    let room = w.saturating_sub("◂ ".width() + place.width() + "  ".width());
    let (place, words) = match room >= STUB_WORDS_MIN {
        true => (place, crate::cut::fit_ellipsis(whole, room)),
        false => (crate::cut::fit_ellipsis(&place, w.saturating_sub("◂ ".width())), String::new()),
    };
    let head = format!("◂ {place}  ");
    let skip = head.chars().count();
    let span = |text: String| Span { cut: None, hint: false, text, style: faint };
    let to = (head.width() + words.width()).min(w);
    let mut row = ChatRow {
        spans: vec![span(head), span(words)],
        skip,
        link: Some(crate::chat::Link { path: s.path.clone(), line: s.line }),
        stub: Some(s.note),
        ..ChatRow::default()
    };
    keep_whole(&mut row, whole);
    if row.rest.is_some() {
        let whole = crate::whole::Whole {
            key: None,
            on: crate::whole::On::Transcript,
            text: whole.to_string(),
            style: card,
            pieces: pieces_of(para, card),
            cols: (0, 0),
            state: None,
            float: None,
        };
        // (from the row's first cell: the paragraph where it would stand, nothing of the stub beside it)
        row.cut = Some((0, to, whole));
    }
    row
}

/// The styles of `para`'s text over `card`, each on its bytes of the line it makes (`lines_of`).
fn pieces_of(para: &[ChatRow], card: Style) -> Vec<(std::ops::Range<usize>, Style)> {
    let (mut out, mut at) = (Vec::new(), 0);
    for r in para {
        // (the space a wrap took: a card draws only its pieces)
        if r.join == Some(true) {
            let s = r.spans.iter().find(|s| !s.text.trim().is_empty());
            out.push((at..at + 1, s.map_or(card, |s| card.patch(s.style))));
            at += 1;
        }
        // (as the document has it: blanks at a row's end aren't the line's — `body`)
        let (mut left, mut skip) = (body(r).0.len(), r.skip);
        for s in &r.spans {
            let chars = s.text.chars().count();
            let len = s.text.chars().skip(skip).map(char::len_utf8).sum::<usize>().min(left);
            skip = skip.saturating_sub(chars);
            if len > 0 {
                out.push((at..at + len, card.patch(s.style)));
                (at, left) = (at + len, left - len);
            }
        }
    }
    out
}

/// A piece of the document: what it was made of (the layout's own rows — the same `Arc` is the same rows), its
/// bytes and lines, its rows' places, and its part of `Document.rows` (`given`).
struct Piece {
    of: Of,
    rows: Arc<[ChatRow]>,
    len: usize,
    lines: usize,
    at: Vec<Place>,
    part: Option<Arc<Part>>,
}

impl Piece {
    fn new(of: Of, rows: &Arc<[ChatRow]>) -> (Piece, String) {
        let (text, at) = lines_of(rows);
        let lines = at.last().map_or(0, |a| a.line + 1);
        (Piece { of, rows: rows.clone(), len: text.len(), lines, at, part: None }, text)
    }

    /// Whether `rows` laid out as `of` are this piece as it is: the same rows (`Arc`) — or air, always the one
    /// empty row whichever `Arc` a frame made for it (`term::chat_laid`).
    fn is(&self, of: Of, rows: &Arc<[ChatRow]>) -> bool {
        self.of == of && (of == Of::Air || Arc::ptr_eq(&self.rows, rows))
    }
}

/// The transcript: its document pane, the pieces it was made of (their rows `Document.rows`, a part each), and its
/// view mode's state.
pub struct Transcript {
    pub pane: Pane,
    pieces: Vec<Piece>,
    /// Its view mode is on (`esc` from the input, `space >`, a press) — the keys its while the chat has them.
    pub keyed: bool,
    /// What the conversation held when the view came or caught up (`seen`) — another now: something came after.
    seen: u64,
    /// Its cursor was placed in this conversation: the selection is where you were.
    visited: bool,
}

impl Transcript {
    pub(crate) fn new(id: crate::document::DocId) -> Transcript {
        let mut doc = Document::from_str(id, "");
        doc.wrap = Some(true);
        doc.throwaway = true;
        doc.rows = Some(Arc::new(Given::default()));
        Transcript { pane: Pane::new(doc), pieces: Vec::new(), keyed: false, seen: 0, visited: false }
    }

    /// Its rows (the document's, frame rows not counted).
    pub fn rows(&self) -> usize {
        self.pane.doc_in().and_then(|d| d.rows.as_deref()).map_or(0, Given::len)
    }

    /// Row `i` as drawn: its line, where its text starts in the document, the bytes of it drawn, the cells before
    /// it, and what the cell after its text stands for (`Shown.end`).
    pub fn row(&self, i: usize) -> Option<Shown> {
        let d = self.pane.doc_in()?;
        let g = d.rows.as_deref()?;
        let r = g.get(i)?;
        let (line, k) = g.place(i);
        // (its drawn length: its piece's row's — a part each, in order; an empty conversation's row, none)
        let len = g.part_of(i).and_then(|(p, k)| self.pieces.get(p)?.at.get(k)).map_or(0, |a| a.len);
        let drawn_end = r.start + len;
        let end = match g.of(line).get(k + 1) {
            // (a row the layout broke at a space: the space its break took)
            Some(next) => (next.start > drawn_end).then_some(drawn_end),
            // (its line's last: the line break — a cut code row's `…` stands for more, none drawn)
            None => (drawn_end == mv::line_end(&d.text, line)).then_some(drawn_end),
        };
        Some(Shown { line, start: r.start, len, x0: r.x0, end })
    }

    /// The stub the document's `line` is: where it goes, and the note that holds its words.
    fn stub_at(&self, line: usize) -> Option<(crate::chat::Link, u64)> {
        let mut base = 0;
        for p in &self.pieces {
            if line < base + p.lines {
                let r = &p.rows[p.at.iter().position(|a| a.line == line - base)?];
                return Some((r.link.clone()?, r.stub?));
            }
            base += p.lines;
        }
        None
    }

    /// The row the view starts at (its top line's rows above the screen counted).
    pub fn top_row(&self) -> usize {
        let Some(d) = self.pane.doc_in() else { return 0 };
        d.rows.as_deref().map_or(0, |g| g.index(d.top, d.top_skip()))
    }

    /// The message whose words the document's `line` is (the air before one: the message before it).
    fn msg_at(&self, line: usize) -> Option<usize> {
        let (mut base, mut last) = (0, None);
        for p in &self.pieces {
            if let Of::Msg(i) = p.of {
                if line < base + p.lines {
                    return Some(i);
                }
                last = Some(i);
            } else if line < base + p.lines {
                return last;
            }
            base += p.lines;
        }
        last
    }

    /// The row of message `i`'s row `k` (its last, past its end).
    fn row_of_msg(&self, i: usize, k: usize) -> Option<usize> {
        let mut at = 0;
        for p in &self.pieces {
            if p.of == Of::Msg(i) {
                return Some(at + k.min(p.at.len().saturating_sub(1)));
            }
            at += p.at.len();
        }
        None
    }
}

/// A row of the transcript as it's drawn (`Transcript::row`): its line, where its text starts in the document and
/// the bytes of it drawn, the cells the drawer put before it, and the byte the cell after its text stands for —
/// its line's break, or the space a wrap took — where a cursor there is drawn (None: the next row starts right
/// there).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub line: usize,
    pub start: usize,
    pub len: usize,
    pub x0: usize,
    pub end: Option<usize>,
}

/// Before a frame draws the chat (`term::render_frame`): its transcript follows `laid`, the conversation as this
/// frame laid it out — each message whose rows were laid out again, its lines replaced when their text changed;
/// from the first piece that isn't what it was (another conversation, a message gone), the rest made again. One
/// transaction: selections and the view's top follow it. Never inside a key's command (its document swapped in).
pub fn sync(ed: &mut Editor, laid: &Laid) {
    if ed.workspace.claude.chat.as_ref().is_some_and(|c| c.transcript.is_none()) {
        let t = Transcript::new(ed.alloc_id());
        ed.workspace.claude.chat.as_mut().expect("a chat, above").transcript = Some(t);
    }
    if let Some(t) = ed.workspace.claude.chat.as_mut().and_then(|c| c.transcript.as_mut()) {
        follow(t, laid);
    }
}

/// `t` made to hold `laid`'s pieces (`sync`).
fn follow(t: &mut Transcript, laid: &Laid) {
    let Some(d) = t.pane.doc_in_mut() else { return };
    let new: Vec<(Of, &Arc<[ChatRow]>)> =
        laid.iter().filter(|(of, _)| *of != Of::Own).map(|(of, rows)| (*of, rows)).collect();
    // (the pieces as they were up to the first laid out anew stay where they are — a frame while Claude writes looks
    // at the answer being written, not the conversation's every piece again)
    let same_to = t.pieces.iter().zip(&new).take_while(|(p, (of, rows))| p.is(*of, rows)).count();
    if same_to == t.pieces.len() && same_to == new.len() {
        return;
    }
    // (the pieces of the same kinds in the same places are each looked at; past the first that differs, made anew)
    let kept =
        same_to + t.pieces[same_to..].iter().zip(&new[same_to..]).take_while(|(p, n)| p.of == n.0).count();
    let mut changes = Vec::new();
    let mut at: usize = t.pieces[..same_to].iter().map(|p| p.len).sum();
    let rest = t.pieces.split_off(same_to);
    let mut pieces = std::mem::take(&mut t.pieces);
    for (i, old) in rest.into_iter().enumerate().map(|(j, old)| (same_to + j, old)) {
        if i >= kept {
            let end = d.text.len_bytes();
            let text: String = new[kept..].iter().map(|(of, rows)| piece(&mut pieces, *of, rows)).collect();
            changes.push(Change { from: at, to: end, insert: text });
            break;
        }
        if old.is(new[i].0, new[i].1) {
            at += old.len;
            pieces.push(old);
            continue;
        }
        let (p, text) = Piece::new(new[i].0, new[i].1);
        let was = d.text.byte_slice(at..at + old.len).to_string();
        if was != text {
            // (only what differs: a message being written grows at its end — the cursor and the view on its
            // earlier lines stay where they are)
            match blank_changes(&was, &text, at) {
                Some(blanks) => changes.extend(blanks),
                None => {
                    let (pre, suf) = same_ends(&was, &text);
                    let insert = text[pre..text.len() - suf].to_string();
                    changes.push(Change { from: at + pre, to: at + old.len - suf, insert });
                }
            }
        }
        at += old.len;
        pieces.push(p);
    }
    // (more pieces than it had: what came after, at its end)
    if pieces.len() < new.len() {
        let text: String =
            new[pieces.len()..].iter().map(|(of, rows)| piece(&mut pieces, *of, rows)).collect();
        changes.push(Change::insert(d.text.len_bytes(), text));
    }
    if !changes.is_empty() {
        let top = mv::line_start(&d.text, d.top.min(mv::last_line(&d.text)));
        let skip = d.top_skip();
        let tx = Transaction::new(changes);
        let top = tx.map_pos(top, Assoc::Before);
        // (what arrives never takes a selection along: its start and its last character's start stay before the
        // text that comes there — a cursor on the line break of the paragraph being written keeps its cell, the
        // words going on under and after it; `x` on the last line keeps that line when a message comes after it)
        let ends: Vec<(usize, Option<usize>, bool)> = d
            .selection()
            .ranges()
            .iter()
            .map(|r| {
                let last = (r.to() > r.from())
                    .then(|| tx.map_pos(crate::graphemes::prev_boundary(&d.text, r.to()), Assoc::Before));
                (tx.map_pos(r.from(), Assoc::Before), last, r.head < r.anchor)
            })
            .collect();
        let primary = d.selection().primary_index();
        let wheeled = d.wheeled();
        d.apply(&tx);
        let ranges = ends
            .into_iter()
            .map(|(from, last, back)| {
                let to = last.map_or(from, |l| crate::graphemes::next_boundary(&d.text, l).max(from));
                if back { Range::new(to, from) } else { Range::new(from, to) }
            })
            .collect();
        d.set_selection(Selection::new(ranges, primary));
        // (the wheel's hold goes along with the cursor: what arrives is no key of yours)
        if wheeled {
            d.wheel_held = Some((d.selection().primary(), d.version()));
        }
        let line = mv::line_of(&d.text, top);
        d.set_top(line);
        d.top_rows = (skip > 0).then_some((line, skip));
    }
    t.pieces = pieces;
    given(d, &mut t.pieces);
}

/// The changes from `was` to `text`, the piece at `at`, where only blank lines differ — a stub opening or closing, one
/// between two others bringing two at once: each line its own change, so the words between them are never replaced and
/// what's selected there stays. One that comes goes before the line break above it, not at the next paragraph's first
/// byte, where `Assoc::Before` would leave what starts there behind it. None: words differ.
fn blank_changes(was: &str, text: &str, at: usize) -> Option<Vec<Change>> {
    let (mut old, mut new) = (was.split_inclusive('\n').peekable(), text.split_inclusive('\n').peekable());
    let (mut pos, mut out): (usize, Vec<Change>) = (at, Vec::new());
    loop {
        match (old.peek().copied(), new.peek().copied()) {
            (None, None) => return Some(out),
            (Some(a), Some(b)) if a == b => {
                pos += a.len();
                old.next();
                new.next();
            }
            (_, Some("\n")) => {
                out.push(Change::insert(if pos > at { pos - 1 } else { pos }, "\n"));
                new.next();
            }
            (Some("\n"), _) => {
                out.push(Change::delete(pos, pos + 1));
                pos += 1;
                old.next();
            }
            _ => return None,
        }
    }
}

/// The bytes `a` and `b` share at their start, and then at their end (not overlapping) — on character boundaries.
fn same_ends(a: &str, b: &str) -> (usize, usize) {
    let pre: usize = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).map(|(x, _)| x.len_utf8()).sum();
    let (a, b) = (&a[pre..], &b[pre..]);
    let suf =
        a.chars().rev().zip(b.chars().rev()).take_while(|(x, y)| x == y).map(|(x, _)| x.len_utf8()).sum();
    (pre, suf)
}

/// A piece made of `rows` put in `pieces`: its text.
fn piece(pieces: &mut Vec<Piece>, of: Of, rows: &Arc<[ChatRow]>) -> String {
    let (p, text) = Piece::new(of, rows);
    pieces.push(p);
    text
}

/// `d`'s rows from its pieces (`Document.rows`), a part each: the parts of the pieces before the first one laid out
/// anew kept as they were, shared (nothing before them changed, so their bytes didn't move); from it on made again
/// — a frame while Claude writes makes the rows of the answer being written, not the conversation's.
fn given(d: &mut Document, pieces: &mut [Piece]) {
    let mut g = d.rows.take().map(Arc::unwrap_or_clone).unwrap_or_default();
    let (mut kept, mut line) = (0, 0);
    for (i, p) in pieces.iter().enumerate() {
        let same = p.part.as_ref().is_some_and(|part| g.parts().get(i).is_some_and(|q| Arc::ptr_eq(q, part)));
        if !same {
            break;
        }
        (kept, line) = (kept + 1, line + p.lines);
    }
    g.keep(kept);
    for p in &mut pieces[kept..] {
        let mut part = Part::default();
        let mut start = 0;
        for (k, a) in p.at.iter().enumerate() {
            if part.first.len() <= a.line {
                part.first.push(part.rows.len());
                start = mv::line_start(&d.text, line + a.line);
            }
            let end_col = match p.at.get(k + 1) {
                Some(n) if n.line == a.line => n.col,
                _ => usize::MAX,
            };
            part.rows.push(Row { start: start + a.from, col: a.col, end_col, x0: a.x0 });
        }
        let part = Arc::new(part);
        p.part = Some(part.clone());
        g.push(part);
        line += p.lines;
    }
    // (an empty conversation: its one empty line, a row of nothing)
    if g.is_empty() {
        g.push(Arc::new(Part { rows: vec![Row::whole(0)], first: vec![0] }));
    }
    d.rows = Some(Arc::new(g));
}

/// `d`'s cursor row in sight in `rows` rows, the scrolloff margin `so` kept — by the rows it's drawn in.
pub fn in_sight(d: &mut Document, rows: usize, so: usize) {
    let Some(g) = d.rows.clone() else { return };
    let rows = rows.max(1);
    let so = so.min(rows.saturating_sub(1) / 2);
    let cursor = d.selection().primary().cursor(&d.text);
    let line = mv::line_of(&d.text, cursor);
    let at = g.index(line, crate::wrap::row_of(g.of(line), cursor));
    let mut top = g.index(d.top, d.top_skip());
    if at < top + so {
        top = at.saturating_sub(so);
    } else if at + so >= top + rows {
        top = (at + so + 1).saturating_sub(rows);
    }
    let top = top.min(g.len().saturating_sub(rows));
    let (line, k) = g.place(top);
    d.set_top(line);
    d.top_rows = (k > 0).then_some((line, k));
}

/// The frame's own rows that come before the last of the document's (`Of::Own` before the queued line): drawn among
/// its rows, not of them.
fn own_inside(laid: &Laid) -> usize {
    let last = laid.iter().rposition(|(of, _)| *of != Of::Own).unwrap_or(0);
    laid[..last].iter().filter(|(of, _)| *of == Of::Own).map(|(_, r)| r.len()).sum()
}

/// The transcript's row that the frame's row `row` is (`laid`, as `Screen.chat_laid` keeps it) — None: one of the
/// frame's own drawn among them (what a run waits on, `thinking…`, before the queued line). Past the document's last
/// piece, counted on from it (an empty conversation's cursor row).
pub fn doc_row(laid: &Laid, row: usize) -> Option<usize> {
    let (mut at, mut doc) = (0, 0);
    let last = laid.iter().rposition(|(of, _)| *of != Of::Own);
    for (k, (of, rows)) in laid.iter().enumerate() {
        if last.is_none_or(|l| k > l) {
            break;
        }
        if row < at + rows.len() {
            return (*of != Of::Own).then_some(doc + row - at);
        }
        at += rows.len();
        if *of != Of::Own {
            doc += rows.len();
        }
    }
    Some(row - (at - doc))
}

/// `doc_row`, a row of the frame's own taken as the document's next.
fn doc_row_from(laid: &Laid, row: usize) -> usize {
    (row..row + own_inside(laid) + 1).find_map(|r| doc_row(laid, r)).unwrap_or(row)
}

/// The frame's row that the transcript's row `row` is drawn in.
pub fn laid_row(laid: &Laid, row: usize) -> usize {
    let (mut at, mut doc) = (0, 0);
    for (of, rows) in laid {
        if *of != Of::Own {
            if row < doc + rows.len() {
                return at + row - doc;
            }
            doc += rows.len();
        }
        at += rows.len();
    }
    row + own_inside(laid)
}

/// The transcript's line its view mode's cursor is on while it has the keys — a cut row there is in hand, whole on its
/// card (`whole.rs`): a tool call's row, the queued line.
pub fn cursor_line(ed: &Editor) -> Option<usize> {
    if !has_keys(ed) {
        return None;
    }
    let d = transcript(ed)?.pane.doc_in()?;
    Some(mv::line_of(&d.text, d.selection().primary().cursor(&d.text)))
}

// ── Its view mode ────────────────────────────────────────────────────────────────────────────────────────

/// The chat's transcript.
pub fn transcript(ed: &Editor) -> Option<&Transcript> {
    ed.workspace.claude.chat.as_ref()?.transcript.as_ref()
}

fn transcript_mut(ed: &mut Editor) -> Option<&mut Transcript> {
    ed.workspace.claude.chat.as_mut()?.transcript.as_mut()
}

/// Its view mode is on (it has the keys whenever the chat has them).
pub fn keyed(ed: &Editor) -> bool {
    transcript(ed).is_some_and(|t| t.keyed)
}

/// The chat's view mode has the keys now: on, the chat's keys, the chat unfolded.
pub fn has_keys(ed: &Editor) -> bool {
    keyed(ed)
        && ed.workspace.is_at(At::Chat)
        && ed.workspace.claude.chat.as_ref().is_some_and(|c| !c.compact())
}

/// What the conversation holds now, as one number: what came changes it.
pub fn seen(c: &Chat) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    c.msgs.len().hash(&mut h);
    if let Some(m) = c.msgs.last() {
        (m.version(), m.text.len()).hash(&mut h);
    }
    (c.state as u8).hash(&mut h);
    h.finish()
}

/// Something came since the view came or caught up (`● normal · new text after`).
pub fn came(ed: &Editor) -> bool {
    let Some(c) = ed.workspace.claude.chat.as_ref() else { return false };
    c.transcript.as_ref().is_some_and(|t| t.keyed && t.seen != seen(c))
}

/// The rows and columns the chat shows its conversation in, as the last frame drew it.
fn viewport(ed: &Editor) -> (usize, usize) {
    // (the frame's own rows drawn among its rows take theirs)
    let own = own_inside(&ed.screen.chat_laid.borrow());
    ed.screen.chat_view.get().map_or((20, 60), |a| (a.2.saturating_sub(own).max(1), a.1.max(1)))
}

/// The chat's chords — the screen, pointing, the last code block, a new or earlier conversation, models, the
/// turn's undo, stop, follow's hops: the chat's in its view mode too (§3.4), where normal mode binds them to no
/// more than a move (`C-d` `C-u` `C-f` `C-b` page there; the chat's table leaves the rest out — `Keymaps`).
pub const CHAT_CHORDS: &[char] = &['s', 'o', 'y', 'r', 'l', 'p', 't', 'z', 'c', ']', 'g'];

/// `key` is one of the chat's chords (`C-s`).
pub fn chord(key: Key) -> bool {
    key.ctrl && !key.alt && matches!(key.code, Code::Char(c) if CHAT_CHORDS.contains(&c))
}

/// `key` is one of the chat's chords that its view mode leaves to it: bound to nothing in its table.
pub fn chat_chord(ed: &Editor, key: Key) -> bool {
    let Some(t) = transcript(ed).filter(|t| t.keyed) else { return false };
    chord(key) && ed.keymaps.view(Surface::Chat, t.pane.keys.mode).search(&[key]).is_none()
}

/// Whether `key` goes on from its view mode with nothing under way: `C-w` (or `A-C-w`) and `:`, the editor's from
/// every pane.
fn passes_key(key: Key) -> bool {
    (key.ctrl && key.code == Code::Char('w')) || key.plain_char() == Some(':')
}

/// The keys its view mode with nothing under way lets go on down the chain (`keys_to`'s layer): `C-w`, `:`, and
/// the chat's chords.
pub fn passes(ed: &Editor) -> Vec<Key> {
    let ctrl = |c: char| Key { code: Code::Char(c), ctrl: true, alt: false, shift: false };
    let mut v = vec![ctrl('w'), Key { alt: true, ..ctrl('w') }, Key::plain(Code::Char(':'))];
    v.extend(CHAT_CHORDS.iter().map(|&c| ctrl(c)).filter(|&k| chat_chord(ed, k)));
    v
}

/// `esc` from the chat's input, `space >` from anywhere: its view mode, landed where you were in this
/// conversation, else on the last answer's first row, else the last row. Nothing to read — an empty
/// conversation — the input's `esc` goes to the code (as it did), `space >` to the input, said. The minimized
/// chat keeps its `esc` (to the code); `space >` brings it back. A chat not drawn yet (closed, minimized) is
/// viewed once the frame lays its rows out (`wanted`).
pub fn view(ed: &mut Editor) {
    let from_input = ed.workspace.is_at(At::Chat);
    let Some(c) = ed.workspace.claude.chat.as_mut() else {
        crate::chat::open(ed);
        return want(ed);
    };
    if c.compact() {
        match from_input {
            true => ed.workspace.viewer.at = At::Code,
            false => {
                c.unfold();
                want(ed);
            }
        }
        return;
    }
    if c.msgs.is_empty() {
        ed.workspace.viewer.at = if from_input { At::Code } else { At::Chat };
        if !from_input {
            ed.set_status("nothing to view yet — the conversation is empty: type a question");
        }
        return;
    }
    // (`space >` from elsewhere with the chat out of sight: once a frame lays it out; the input's `esc` has it in
    // sight — its transcript what the last frame laid; none laid yet, nothing to read: to the code)
    match (from_input, c.transcript.is_some()) {
        (true, false) => return ed.workspace.viewer.at = At::Code,
        (false, false) => return want(ed),
        (false, true) if ed.screen.chat_view.get().is_none() => return want(ed),
        _ => {}
    }
    enter(ed);
}

/// Its view mode asked for before its rows were laid out: made once a frame has (`wanted`).
fn want(ed: &mut Editor) {
    if let Some(c) = ed.workspace.claude.chat.as_mut() {
        c.view_wanted = true;
        ed.workspace.viewer.at = At::Chat;
    }
}

/// After a frame (`term::render`): the view asked for before the chat was drawn, made now — true when it was
/// (the frame is drawn again with it).
pub fn wanted(ed: &mut Editor) -> bool {
    let chat_keys = ed.workspace.is_at(At::Chat);
    let Some(c) = ed.workspace.claude.chat.as_mut().filter(|c| c.view_wanted) else { return false };
    c.view_wanted = false;
    if !chat_keys || ed.screen.chat_view.get().is_none() || c.transcript.is_none() {
        return false;
    }
    // (opened on an empty conversation: the input keeps the keys, said)
    if c.msgs.is_empty() {
        ed.set_status("nothing to view yet — the conversation is empty: type a question");
        return true;
    }
    view(ed);
    true
}

/// The last answer: the first of Claude's after your last question, else Claude's last.
fn last_answer(c: &Chat) -> Option<usize> {
    use crate::chat::Role;
    let from = c.msgs.iter().rposition(|m| m.role == Role::User).map_or(0, |i| i + 1);
    (from..c.msgs.len())
        .find(|&i| c.msgs[i].role == Role::Assistant)
        .or_else(|| c.msgs.iter().rposition(|m| m.role == Role::Assistant))
}

/// Its view mode, the keys its: the rows in sight as the chat showed them, the cursor where you were, else the last
/// answer's first row (its first is the air above it), else the last. One view at a time: the terminal's ends.
fn enter(ed: &mut Editor) {
    come(ed, None);
    in_sight_now(ed);
}

/// `enter`'s view, the cursor landed — on the row `row` (a press's, under the pointer) — the rows in sight not moved
/// for it yet.
fn come(ed: &mut Editor, row: Option<usize>) {
    crate::view::leave(ed);
    let shown = ed.screen.chat_view.get().map_or(0, |a| a.3);
    let shown = doc_row_from(&ed.screen.chat_laid.borrow(), shown);
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    let now = seen(c);
    let answer = last_answer(c);
    let Some(t) = c.transcript.as_mut() else { return };
    let last = t.rows().saturating_sub(1);
    let land = match row {
        Some(r) => Some(r.min(last)),
        None if t.visited => None,
        None => Some(answer.and_then(|i| t.row_of_msg(i, 1)).unwrap_or(last)),
    };
    (t.keyed, t.visited, t.seen) = (true, true, now);
    let Some(d) = t.pane.doc_in_mut() else { return };
    let Some(g) = d.rows.clone() else { return };
    let (line, k) = g.place(shown.min(last));
    d.set_top(line);
    d.top_rows = (k > 0).then_some((line, k));
    if let Some(r) = land.and_then(|r| g.get(r)) {
        d.set_selection(Selection::point(r.start));
    }
    ed.workspace.viewer.at = At::Chat;
}

/// A press on the conversation's row `row` (counted as `Screen.chat_view`'s) at column `col` of its text: its view
/// mode, the keys its, the cursor on that character, as a click in the code or a run's output puts it — on a bar
/// or a hanging indent, the row's first; past its text, the cell after it (its line break, the space a wrap took),
/// else its last. In sight already: the rows under the pointer don't move. Where it landed — a drag from there
/// selects (`drag`). A row of the frame's own (what a run waits on) or an empty conversation: None, nothing placed.
pub fn click(ed: &mut Editor, row: usize, col: usize) -> Option<usize> {
    if ed.workspace.claude.chat.as_ref().is_none_or(|c| c.msgs.is_empty()) {
        return None;
    }
    let row = doc_row(&ed.screen.chat_laid.borrow(), row)?;
    let s = transcript(ed).filter(|t| row < t.rows()).and_then(|t| t.row(row))?;
    if !has_keys(ed) {
        come(ed, Some(row));
    }
    let d = transcript_mut(ed).and_then(|t| t.pane.doc_in_mut())?;
    let pos = under(d, s, col);
    d.set_selection(Selection::point(pos));
    Some(pos)
}

/// A drag from the press's character `anchor` (`click`'s) to the conversation's row `row`, column `col` under the
/// pointer: what's between selected, both included, as a drag in the code selects — `y` copies it. Onto a stub, its
/// paragraph whole (#271, as a folded doc comment's row, #269): it stays folded, its words lit, nothing moving under the
/// pointer — and `y` copies every word of it. Its view no longer with the keys, or a row of the frame's own: nothing.
pub fn drag(ed: &mut Editor, held: crate::selection::Held, row: usize, col: usize) {
    if !has_keys(ed) {
        return;
    }
    let Some(row) = doc_row(&ed.screen.chat_laid.borrow(), row) else { return };
    let Some(t) = transcript(ed).filter(|t| row < t.rows()) else { return };
    let Some(s) = t.row(row) else { return };
    let stub = t.stub_at(s.line).is_some();
    let Some(d) = doc_mut(ed) else { return };
    let range = match stub {
        true => held.dragged_over(&d.text, (mv::line_start(&d.text, s.line), mv::line_end(&d.text, s.line))),
        false => held.dragged(&d.text, under(d, s, col)),
    };
    d.set_selection(Selection::new(vec![range], 0));
}

/// The conversation's document.
pub(crate) fn doc_mut(ed: &mut Editor) -> Option<&mut Document> {
    transcript_mut(ed).and_then(|t| t.pane.doc_in_mut())
}

/// The conversation's view `by` rows and nothing else — its cursor and selection where they are, in sight or not (the
/// wheel's, a drag's held past its edge; the next key that moves the selection brings the view back to them).
pub fn view_by(ed: &mut Editor, by: isize) {
    let size = viewport(ed);
    crate::docpane::swap(ed, Which::Chat, size, |ed| ed.view_by(by));
}

/// The character row `s` draws at column `col` of the chat's text (`click`'s rule).
fn under(d: &Document, s: Shown, col: usize) -> usize {
    let text = d.text.byte_slice(s.start..s.start + s.len).to_string();
    // (the character whose cells hold the column — display width, a wide one's second cell its own)
    let col = col.saturating_sub(s.x0);
    let (mut w, mut at) = (0, None);
    for (b, g) in unicode_segmentation::UnicodeSegmentation::grapheme_indices(text.as_str(), true) {
        w += g.width();
        if w > col {
            at = Some(s.start + b);
            break;
        }
    }
    let end = s.start + s.len;
    let pos = at.or(s.end).unwrap_or_else(|| crate::graphemes::prev_boundary(&d.text, end).max(s.start));
    crate::graphemes::snap(&d.text, pos)
}

/// After a frame (`term::render`, as `rundoc::resized`): the chat drawn at another size while its view has the
/// keys — the cursor kept in sight now, not at the next key. True when the rows in sight moved (drawn again). Never
/// while the wheel left the view off the cursor (`Document::wheeled` — the mouse rule): the next key that moves the
/// selection brings it back.
pub fn resized(ed: &mut Editor) -> bool {
    if !has_keys(ed) {
        return false;
    }
    let size = viewport(ed);
    if transcript(ed).and_then(|t| t.pane.doc_in()).is_some_and(Document::wheeled) {
        if let Some(t) = transcript_mut(ed) {
            t.pane.size = Some(size);
        }
        return false;
    }
    if let Some(before) = transcript(ed).filter(|t| t.pane.size != Some(size)).map(Transcript::top_row) {
        in_sight_now(ed);
        return transcript(ed).map(Transcript::top_row) != Some(before);
    }
    // (a cursor past what arrives — on the queued line, under the answer being written — went down with it: kept in
    // sight, the rows over it moving only then; anywhere else what arrives moves nothing)
    let Some(d) = transcript_mut(ed).and_then(|t| t.pane.doc_in_mut()) else { return false };
    let Some(g) = d.rows.clone() else { return false };
    let cursor = d.selection().primary().cursor(&d.text);
    let line = mv::line_of(&d.text, cursor);
    let at = g.index(line, crate::wrap::row_of(g.of(line), cursor));
    if at < g.index(d.top, d.top_skip()) + size.0 {
        return false;
    }
    in_sight(d, size.0, 0);
    true
}

/// The cursor's row in sight in the rows the chat shows, the scrolloff margin kept.
fn in_sight_now(ed: &mut Editor) {
    let size = viewport(ed);
    let so = ed.config.scrolloff;
    let Some(t) = transcript_mut(ed) else { return };
    t.pane.size = Some(size);
    if let Some(d) = t.pane.doc_in_mut() {
        in_sight(d, size.0, so);
    }
}

/// Ends its view mode: where you were kept (its selection, down to its cursor — the next view lands there), the
/// chat showing the rows you read — `Chat.scroll` from the bottom, what came while it held below them; a prompt
/// of its goes. The keys stay where they are — the caller says where they go.
pub fn leave(ed: &mut Editor) {
    // (the rows the chat shows, its own among them: `scroll` counts the frame's rows)
    let area = ed.screen.chat_view.get().map_or(20, |a| a.2.max(1));
    // (the chat's scroll counts from the live bottom: every row the last frame laid out, its own included)
    let laid = ed.screen.chat_laid.borrow().clone();
    let total: usize = laid.iter().map(|(_, r)| r.len()).sum();
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    let Some(t) = c.transcript.as_mut().filter(|t| t.keyed && t.pane.doc_in().is_some()) else { return };
    t.keyed = false;
    c.scroll = total.max(t.rows()).saturating_sub(laid_row(&laid, t.top_row()) + area);
    if t.pane.dropped() {
        ed.prompt = None;
    }
}

/// Ends its view mode and keeps nothing of it: the conversation is another (`C-l` a new one, `C-p` an earlier one)
/// — where you were was the one that went.
pub fn forget(ed: &mut Editor) {
    leave(ed);
    if let Some(t) = transcript_mut(ed) {
        t.visited = false;
    }
    // (and a paragraph a note's card read on to: of the conversation that went)
    if let Some(c) = ed.workspace.claude.chat.as_mut() {
        c.reading = None;
    }
}

/// Every key and event (`view::sync`): its view mode ends once the chat lost the keys (a click elsewhere, the
/// chat closed or minimized, `C-w h`) — never inside a key's command (its document swapped in).
pub fn sync_keys(ed: &mut Editor) {
    if keyed(ed) && !has_keys(ed) {
        leave(ed);
    }
}

/// Caught up: the cursor on the latest line, at the bottom of what's in sight, what came taken in.
fn catch_up(ed: &mut Editor) {
    let (area, _) = viewport(ed);
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    let now = seen(c);
    let Some(t) = c.transcript.as_mut() else { return };
    t.seen = now;
    let rows = t.rows();
    let Some(d) = t.pane.doc_in_mut() else { return };
    let Some(g) = d.rows.clone() else { return };
    let last = mv::last_line(&d.text);
    d.set_selection(Selection::point(mv::line_start(&d.text, last)));
    let (line, k) = g.place(rows.saturating_sub(area));
    d.set_top(line);
    d.top_rows = (k > 0).then_some((line, k));
    in_sight_now(ed);
}

/// Runs `f` with the transcript swapped in (`docpane::swap`): an edit or typing put back is said (it's
/// read-only), the cursor kept in sight after — once the key moved the selection.
fn swap<R>(ed: &mut Editor, f: impl FnOnce(&mut Editor) -> R) -> Option<R> {
    let size = viewport(ed);
    let (r, done) = crate::docpane::swap(ed, Which::Chat, size, f)?;
    if done.refused() {
        let bound = |name, like| crate::view::bound(ed, Surface::Chat, name, like);
        let ways: Vec<String> = [("view_input", "i", "types"), ("view_leave", "esc", "code")]
            .iter()
            .filter_map(|(cmd, like, what)| Some(format!("{} {what}", bound(cmd, like)?)))
            .collect();
        let out = if ways.is_empty() { String::new() } else { format!(" — {}", ways.join(", ")) };
        ed.set_status(format!("the conversation is read-only{out}"));
    }
    // (only a key that moved the selection brings the cursor in sight: what came below while it held moves nothing —
    // `esc`, `y`, `v` leave the rows you read where they are, the scrolloff margin never taken from them: #183)
    if done.moved {
        in_sight_now(ed);
    }
    Some(r)
}

/// A command a key of its view mode runs on the editor, the document put back first: `space l`, `space I`,
/// `space ?` (`view::EDITORS`), the window table's, a `:` line bound to a key — they act on the session, never on
/// the text in sight.
fn editors(ed: &Editor, c: &MappableCommand) -> bool {
    match c {
        MappableCommand::Static(s) => {
            crate::view::EDITORS.contains(&s.name) || ed.keymaps.in_window_table(s.name)
        }
        MappableCommand::Typed(_) => true,
    }
}

/// A key while its view mode has them: its prompt's (`/`), else the chat's table — normal mode's with `[view]`
/// and `[view.chat]` over it — on its document. `C-w`, `:`, the chat's chords and a sequence of the editor's under
/// way go on. Returns the handler that took it.
pub fn key(ed: &mut Editor, key: Key) -> Option<crate::uistate::Handler> {
    use crate::uistate::Handler;
    if !has_keys(ed) || !ed.pending.is_empty() {
        return None;
    }
    let t = transcript(ed)?;
    t.pane.doc_in()?;
    if ed.prompt.is_some() {
        if !t.pane.prompt {
            return None;
        }
        swap(ed, |ed| ed.handle_prompt_key(key));
        return Some(Handler::Prompt);
    }
    if t.pane.keys.idle() && (passes_key(key) || chat_chord(ed, key)) {
        return None;
    }
    swap(ed, |ed| {
        crate::docpane::key_in(
            ed,
            key,
            |ed, mode, keys| ed.keymaps.view_lookup(Surface::Chat, mode, keys),
            editors,
        )
    });
    after(ed);
    Some(Handler::ChatView)
}

/// The stub under its view mode's cursor (`ChatRow.stub`): where it goes, and the note that holds its words.
pub fn stub_here(ed: &Editor) -> Option<(crate::chat::Link, u64)> {
    let t = transcript(ed).filter(|_| has_keys(ed))?;
    let d = t.pane.doc_in()?;
    t.stub_at(mv::line_of(&d.text, d.selection().primary().cursor(&d.text)))
}

/// A stub's `enter` or a press on it: its place in the code, its note's card open with its answer in sight — the keys
/// where they are, as a link's (`chat::jump`). A file gone says so; no card opens on nothing.
pub fn go(ed: &mut Editor, link: crate::chat::Link, note: u64) {
    let path = link.path.clone();
    crate::chat::jump(ed, link);
    if ed.doc().path.as_deref() == Some(path.as_path()) {
        crate::notes::open_answer(ed, note);
    }
}

/// What a command asked for, once its key is done.
fn after(ed: &mut Editor) {
    let Some(t) = transcript_mut(ed) else { return };
    let Some((a, deferred)) = t.pane.take_after() else { return };
    match a {
        After::Leave => {
            leave(ed);
            ed.workspace.leave(At::Chat);
        }
        // (the input: the keys stay the chat's)
        After::Input => leave(ed),
        // (nothing selected: a stub goes to its place, anything else to the input)
        After::Enter => match stub_here(ed) {
            Some((link, note)) => go(ed, link, note),
            None => leave(ed),
        },
        After::Latest => catch_up(ed),
        After::Take => take(ed),
        After::ChatView => ed.set_status(
            "this is the conversation, read-only — select something, then enter or space > quotes it",
        ),
        After::Editor => ed.run(&deferred, None),
    }
}

/// `enter` (`space >`) on a selection: each range a quote over the chat's input (`quote.rs`) — the text, a
/// paragraph whole however it wrapped — the view left, the input with the keys.
fn take(ed: &mut Editor) {
    use crate::chat::Role;
    use crate::quote::{Taken, lines, text_of};
    let Some(c) = ed.workspace.claude.chat.as_ref() else { return };
    let Some(t) = c.transcript.as_ref() else { return };
    let Some(d) = t.pane.doc_in() else { return };
    let quote = |r: &Range| {
        let (text, l0, l1) = text_of(&d.text, (r.from(), r.to()), &|_| (0, None), &|_| None);
        // (blank lines around it — the air between messages — aren't what was meant)
        let text = text.trim_start_matches('\n').trim_end().to_string();
        if text.trim().is_empty() {
            return None;
        }
        let n = lines(text.lines().count());
        // (whose words: one message's role, else the chat's)
        let role = |l: usize| t.msg_at(l).and_then(|m| Some(c.msgs.get(m)?.role));
        let (role, label) = match (role(l0), role(l1)) {
            (Some(a), Some(b)) if a == b => match a {
                Role::User => ("user", "your message"),
                Role::Assistant => ("assistant", "Claude's answer"),
                Role::Thought => ("thinking", "Claude's thinking"),
                Role::Tool => ("tool calls", "Claude's tool calls"),
                Role::Note => ("note", "the chat"),
            },
            _ => ("several", "the chat"),
        };
        Some(Taken {
            attrs: vec![("from", "chat".into()), ("role", role.into())],
            label: format!("{label} {n}"),
            text,
            redact: false,
            code: None,
        })
    };
    // (only the ranges wider than a character: a plain cursor beside them quotes nothing)
    let wide = |r: &&Range| crate::graphemes::next_boundary(&d.text, r.from()) < r.to();
    let taken: Vec<Taken> = d.selection().ranges().iter().filter(wide).filter_map(quote).collect();
    if taken.is_empty() {
        return ed.set_status("only blanks are selected — nothing to quote");
    }
    // (the keys the chat's input's: `quote::add` focuses it)
    leave(ed);
    crate::quote::add(ed, taken);
}

/// Whether `enter` quotes in its view mode now: something selected.
pub fn quotes(ed: &Editor) -> bool {
    has_keys(ed) && transcript(ed).and_then(|t| t.pane.doc_in()).is_some_and(crate::docpane::wide)
}

/// Before drawing: the matches its search shows — the pattern its `/` prompt is typing, else its last search
/// while shown.
pub fn refresh_search(ed: &mut Editor) {
    let typed = ed.prompt.as_ref().filter(|p| matches!(p.kind, crate::editor::PromptKind::Search { .. }));
    let typed = typed.map(|p| p.text());
    if !has_keys(ed) {
        return;
    }
    if let Some(t) = transcript_mut(ed) {
        t.pane.refresh_search(typed);
    }
}

/// `esc` with its keys: its matches no longer shown (the code's stay as they are).
pub fn search_unshown(ed: &mut Editor) {
    if let Some(t) = transcript_mut(ed) {
        t.pane.keys.search_hl = false;
    }
}

/// The wheel over the conversation: three rows and nothing else — the keys, the focus, the cursor and the selection
/// stay where they are (the user, 2026-10-28: the wheel only scrolls). In its view mode, its document's rows (a
/// paragraph's each its own); else the chat's own, up from the bottom within what the last frame laid out — the next
/// key typed in the input brings it back down (`chat::key`). An empty conversation: nothing to read, nothing done.
pub fn wheel(ed: &mut Editor, up: bool) {
    if ed.workspace.claude.chat.as_ref().is_none_or(|c| c.msgs.is_empty() || c.transcript.is_none()) {
        return;
    }
    if has_keys(ed) {
        return view_by(ed, if up { -3 } else { 3 });
    }
    // (the rows the chat shows and every row the last frame laid out — `leave`'s measures)
    let area = ed.screen.chat_view.get().map_or(20, |a| a.2.max(1));
    let total: usize = ed.screen.chat_laid.borrow().iter().map(|(_, r)| r.len()).sum();
    if let Some(c) = ed.workspace.claude.chat.as_mut() {
        c.scroll = match up {
            true => (c.scroll + 3).min(total.saturating_sub(area)),
            false => c.scroll.saturating_sub(3),
        };
    }
}

/// The state's `view` while its view mode has the keys: the cursor's line, selections, whether more came since —
/// shapes, never text.
pub fn state(ed: &Editor) -> Option<serde_json::Value> {
    if !has_keys(ed) {
        return None;
    }
    let t = transcript(ed)?;
    let d = t.pane.doc_in()?;
    let line = mv::line_of(&d.text, d.selection().primary().cursor(&d.text));
    Some(serde_json::json!({
        "surface": Surface::Chat.name(),
        "line": line + 1,
        "lines": t.pane.lines(),
        "selections": d.selection().ranges().len(),
        "selects": t.pane.selects(),
        "mode": if t.pane.keys.mode == Mode::Select { "select" } else { "normal" },
        "held": came(ed),
    }))
}

#[cfg(test)]
mod fold_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) mod view_tests;
