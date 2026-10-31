//! Notes — Claude's comments pinned to lines of code, each a thread you answer in place.
//!
//! - Claude writes them with the `note` tool of the chat process (an MCP server living in tarae — the chat
//!   answers `mcp_message` control requests, `chat.rs`). `reply_to` answers a thread instead.
//! - Each entry says who wrote it (`By`): the author (the chat's Claude), you, or the reviewer (a second
//!   Claude, `reviewer.rs` — its findings wait like your drafts, `finding`).
//! - A note sits on a document mark (it follows edits); a file that isn't open keeps the line numbers and
//!   gets its mark when it opens (`attach`, every event). The line end shows `¶ first words +N`, and with
//!   the cursor on the lines a card shows the whole thread (`term::note_lines`).
//! - `space n`: reply to the note here, or ask Claude about this line (a new note starting with you).
//!   The message goes to the chat with the lines and the thread; Claude answers with `reply_to` — if it
//!   answers in plain text, that text goes into the thread. `]n`/`[n` walk notes, `space N` lists them,
//!   `:note-close` drops the one here.
//! - Kept per session, under its checkout, in the restore file (`restore.rs`, written on quit — `to_json`, read
//!   as the session is made — `restore_in`). A note remembers its first line's text: if the file changed while
//!   tarae was closed, it re-anchors on the nearest line with that text when the file opens.

use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::document::DocId;
use crate::editor::Editor;
use crate::movement as mv;
use crate::selection::{Range as SelRange, Selection};

/// Who wrote an entry: the chat's Claude (the author of the changes) · you · the reviewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    /// The chat's Claude — what it changed is what's reviewed.
    Author,
    You,
    /// The reviewer (`reviewer.rs`) — a second Claude, apart from the chat's.
    Reviewer,
    /// What the chat's last answer said of a place (`chatnotes.rs`) — the chat's Claude's words, not a note it
    /// pinned.
    Chat,
}

impl By {
    /// How it's shown: on the note's card, in the notes list and the review rail.
    pub fn label(self) -> &'static str {
        match self {
            By::Author => "claude",
            By::You => "you",
            By::Reviewer => "reviewer",
            By::Chat => "chat",
        }
    }

    /// How a thread line names its author for Claude, and in the restore file (the author's `claude`, as
    /// files written before there was a reviewer have it).
    pub fn word(self) -> &'static str {
        match self {
            By::Author => "claude",
            By::You => "you",
            By::Reviewer => "reviewer",
            By::Chat => "chat",
        }
    }

    fn from_word(w: &str) -> By {
        match w {
            "you" => By::You,
            "reviewer" => By::Reviewer,
            "chat" => By::Chat,
            _ => By::Author,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub by: By,
    pub text: String,
    /// Written during a review — goes to Claude with the rest of it (`codereview::send`).
    pub draft: bool,
}

#[derive(Clone, Debug)]
pub struct Note {
    pub id: u64,
    pub path: PathBuf,
    /// The mark in the open document (follows edits).
    pub at: Option<(DocId, u64)>,
    /// 0-based lines, as of the last event.
    pub lines: Range<usize>,
    pub thread: Vec<Entry>,
    /// You wrote last and Claude hasn't answered yet.
    pub waiting: bool,
    /// Restored from last time: the first line's text then (re-anchors once, when the file opens).
    pub anchor: Option<String>,
    /// Its lines' text as it was pinned — a SHA-256 of it (`lines_digest`): a draft whose lines changed since
    /// is offered to close at a turn's end (#197). None until known (a file not open: read on a worker).
    pub pinned: Option<[u8; 32]>,
    /// A finding of the board's (`board.rs`): the angles that found it, what its skeptic made of it, its trace.
    pub board: Option<Mark>,
    /// A finding still being placed on the lines now from the snapshot its reviewer read (`pin_finding`): no
    /// mark yet.
    pub placing: bool,
    /// A chat note's (`chatnotes.rs`): the sentence that names its place — the line end's words; its first entry
    /// is the paragraph, the card's.
    pub sentence: Option<String>,
    /// A chat note at the answer's place (`chatnotes::Part`, focus A1): the answer's flow its card draws — its first
    /// entry the same as one text. Empty for every other note.
    pub flow: Vec<crate::chatnotes::Part>,
    /// A chat note's: the question its answer answered, its first line — the card's head says which answer (focus A1b).
    pub asked: Option<String>,
    /// A chat note's: which answer it's of and where in it (`chatnotes::Origin`) — one line's answers one card, newest
    /// first; kept with the conversation.
    pub answer: Option<crate::chatnotes::Origin>,
    /// A finding's: the branch its round's changes were since (`Reviewer.since`; None: uncommitted, or not a
    /// finding) — what its change lands in, in the checkout tarae started in (`landed_check`, #248).
    pub since: Option<String>,
}

/// What a finding of the board's carries beside its text: the angles that found it (the first pinned it),
/// confirmed or unchecked, and the skeptic's trace (or why no one checked it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub angles: Vec<String>,
    pub verdict: crate::board::Verdict,
    pub trace: String,
}

impl Mark {
    fn json(&self) -> Value {
        json!({ "angles": self.angles, "verdict": self.verdict.word(), "trace": self.trace })
    }

    fn of(v: &Value) -> Option<Mark> {
        let angles: Vec<String> =
            v["angles"].as_array()?.iter().filter_map(|a| a.as_str().map(str::to_string)).collect();
        let verdict = crate::board::Verdict::named(v["verdict"].as_str()?)?;
        (!angles.is_empty()).then(|| Mark {
            angles,
            verdict,
            trace: v["trace"].as_str().unwrap_or_default().into(),
        })
    }
}

/// Whether note `n` hasn't gone to Claude yet: it began as a draft (yours, or the reviewer's finding) that
/// `space v s` hasn't sent.
pub fn unsent(n: &Note) -> bool {
    n.thread.first().is_some_and(|e| e.draft)
}

impl Note {
    /// The first thing said — what the line end shows (a chat note's, the sentence that names its place).
    pub fn gist(&self) -> &str {
        if let Some(s) = self.sentence.as_deref() {
            return s;
        }
        self.thread.first().map_or("", |e| e.text.lines().find(|l| !l.trim().is_empty()).unwrap_or(""))
    }
}

/// Whether `n` is what one of the conversation's answers said of a place, nothing more (`chatnotes.rs`): kept with the
/// conversation (never in the restore file), never told to Claude. Answered with `space n`, it's a thread.
pub fn bare_chat(n: &Note) -> bool {
    n.thread.len() == 1 && n.thread[0].by == By::Chat
}

#[derive(Default)]
pub struct Notes {
    pub list: Vec<Note>,
    next: u64,
    /// The notes the chat turn in flight is about (one: its answer lands there if Claude doesn't use the tool).
    pub turn: Vec<u64>,
    /// The note shown when several share a line — the one just pinned, or the one `]n`/`[n` stepped to.
    pub focus: Option<u64>,
    /// The note whose card is open between the lines, under its last one, in every pane that shows its file (focus
    /// A1b): opened by an act only (`]n`/`[n`, `enter` on its line, a press on its `¶`), never by the cursor passing;
    /// open until closed (`q`, `esc`, `enter` again) or another opens — leaving the file keeps it.
    pub open: Option<u64>,
    /// The open card is the one a turn's end opened (`open_by_turn`): the next answer's replaces it; one you opened
    /// stays.
    pub turned: bool,
    /// A turn's card waiting for you to stop typing (`opening_sync`).
    pub opening: Option<u64>,
    /// The open card shows its line's earlier answers too (`enter`, a press on its `… N earlier`).
    pub earlier: bool,
    /// The answers this chat has placed notes for — the newest's number (`chatnotes::Origin.n`).
    pub answers: u64,
    /// The chat notes that are bare as the conversation last kept them (`chatnotes::kept_sync`).
    pub kept_bare: usize,
    /// The chat closed, its answers' notes still here: their conversation as it stood then and how many messages of yours
    /// it holds (`chat::keep_places`), until the chat opens again.
    pub left: Option<(Box<crate::chats::Saved>, usize)>,
    /// Drafts the stale card asked about (`stale_card`) — not asked again.
    stale_asked: Vec<u64>,
    /// Findings placed on lines changed since their reviewer read them (`pin_finding`) — the stale card says so.
    since_read: Vec<u64>,
    /// The notes the chat's Claude pinned in the turn under way — its answer's places take none on their lines
    /// (`chatnotes::turn_over`).
    pub pinned_now: Vec<u64>,
    /// The chat's Claude wrote at a note in the turn under way — pinned one or replied on one: its answer is at the code,
    /// so it gets no flow at its place (`chatnotes::turn_over`).
    pub noted_now: bool,
    /// The last answer's places, waiting for what their names come to (`chatnotes::sync`).
    pub answer: Option<crate::chatnotes::Waiting>,
    /// A note on a path that's no text was said not to last (`unkept_said`) — once.
    unkept_said: bool,
}

/// The `note` tool as the MCP server lists it.
pub fn tool_schema() -> Value {
    json!({
        "name": "note",
        "description": "Pin a short note to lines of code in the user's editor — it shows beside that code and \
    the user can reply there. Use it for observations tied to specific lines (a bug, a risk, a question, an idea); \
    one or two sentences each, a few per answer at most. A path:line in your answer already shows at that line \
    with its sentence — don't repeat it as a note. To answer the user on an existing note, pass reply_to with its \
    id (path and line aren't needed then).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path, absolute or relative to the project root" },
                "line": { "type": "integer", "description": "First line, 1-based" },
                "end_line": { "type": "integer", "description": "Last line, 1-based (optional)" },
                "text": { "type": "string", "description": "The note — Markdown, one or two sentences" },
                "reply_to": { "type": "integer", "description": "Id of the note you're answering" }
            },
            "required": ["text"]
        }
    })
}

fn find(ed: &Editor, id: u64) -> Option<&Note> {
    ed.workspace.claude.notes.list.iter().find(|n| n.id == id)
}

pub(crate) fn find_mut(ed: &mut Editor, id: u64) -> Option<&mut Note> {
    ed.workspace.claude.notes.list.iter_mut().find(|n| n.id == id)
}

/// A note on `path` is about to count for the restore file: said, the first time, where its path is no text and
/// `to_json` will leave it out — at the save, on quit, nobody's left to read it.
fn unkept_said(ed: &mut Editor, path: &Path) {
    if path.to_str().is_none() && !std::mem::replace(&mut ed.workspace.claude.notes.unkept_said, true) {
        let said = format!("A note on {} stays only until you quit: its path isn't text", path.display());
        ed.set_warning(said);
    }
}

/// A new note on `lines` of `path`.
pub fn add(ed: &mut Editor, path: &Path, lines: Range<usize>, by: By, text: &str) -> u64 {
    unkept_said(ed, path);
    ed.workspace.claude.notes.next += 1;
    let id = ed.workspace.claude.notes.next;
    ed.workspace.claude.notes.list.push(Note {
        id,
        path: path.to_path_buf(),
        at: None,
        lines: lines.clone(),
        thread: vec![Entry { by, text: text.trim().to_string(), draft: false }],
        waiting: by == By::You,
        anchor: None,
        pinned: None,
        board: None,
        placing: false,
        sentence: None,
        flow: Vec::new(),
        asked: None,
        answer: None,
        since: None,
    });
    ed.workspace.claude.notes.focus = Some(id);
    attach(ed);
    // (a file not open — a finding of the reviewer's: its lines as they are now, read on a worker)
    if find(ed, id).is_some_and(|n| n.pinned.is_none()) {
        let path = path.to_path_buf();
        ed.events.jobs().spawn(move || {
            let text = std::fs::read_to_string(&path).ok().map(|t| ropey::Rope::from_str(&t));
            let pinned = text.and_then(|t| lines_digest(&t, &lines));
            move |ed: &mut Editor| {
                if let Some(n) = find_mut(ed, id).filter(|n| n.pinned.is_none()) {
                    n.pinned = pinned;
                }
                ed.unchanged = true;
            }
        });
    }
    id
}

/// A chat note on `lines` of `path` (`chatnotes::sync`): `sentence` at the line end, `paragraph` on its card — or, at
/// the answer's place, its `flow`. Not focused (a crowded line shows the note you stepped to) and no lines kept as pinned
/// (no draft's: never stale). `asked`: the question the answer answered.
pub fn add_said(
    ed: &mut Editor,
    path: &Path,
    lines: Range<usize>,
    (sentence, paragraph): (String, String),
    flow: Vec<crate::chatnotes::Part>,
    asked: Option<String>,
) -> u64 {
    let notes = &mut ed.workspace.claude.notes;
    notes.next += 1;
    notes.list.push(Note {
        id: notes.next,
        path: path.to_path_buf(),
        at: None,
        lines,
        thread: vec![Entry { by: By::Chat, text: paragraph, draft: false }],
        waiting: false,
        anchor: None,
        pinned: None,
        board: None,
        placing: false,
        sentence: Some(sentence),
        flow,
        asked,
        answer: None,
        since: None,
    });
    notes.next
}

/// What a note keeps of its lines' text to tell a change (`Note.pinned`): a SHA-256 of them. None past the
/// end.
pub(crate) fn lines_digest(text: &ropey::Rope, lines: &Range<usize>) -> Option<[u8; 32]> {
    let n = text.len_lines();
    if lines.start >= n {
        return None;
    }
    let end = lines.end.clamp(lines.start + 1, n);
    let s = text.slice(text.line_to_char(lines.start)..text.line_to_char(end)).to_string();
    Some(crate::sha256::digest(s.as_bytes()))
}

fn hex(d: &[u8; 32]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn of_hex(s: &str) -> Option<[u8; 32]> {
    let bytes: Option<Vec<u8>> = (0..s.len())
        .step_by(2)
        .map(|i| s.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok()))
        .collect();
    bytes?.try_into().ok()
}

/// Every event: open documents get marks for their notes, and every note's lines follow its mark.
pub fn attach(ed: &mut Editor) {
    let Editor { workspace: session, docs, .. } = ed;
    attach_in(&mut session.claude.notes, docs);
}

/// `attach` for one session's notes.
fn attach_in(notes: &mut Notes, docs: &mut crate::docpaths::Docs) {
    // (a note looks for its buffer every event — by id once attached, else by its path: the buffers laid out once,
    // kept while none moves (`Docs`; a mark added moves none). #214: each such note searched every buffer)
    for n in notes.list.iter_mut().filter(|n| !n.placing) {
        // (its mark as stored, its ends snapped as `Document::mark` does — never a selection made for it each event:
        // #214's third piece, an allocation a note)
        match n.at.and_then(|(d, m)| {
            let doc = &docs[docs.index_of(d)?];
            Some((doc, doc.mark_raw(m)?.primary()))
        }) {
            Some((doc, r)) => {
                let line = |p| mv::line_of(&doc.text, crate::graphemes::snap(&doc.text, p));
                let (a, b) = (line(r.anchor), line(r.head));
                n.lines = a.min(b)..a.max(b) + 1;
            }
            None => {
                n.at = None;
                if let Some(doc) = docs.first(&n.path, |d| !d.loading).map(|i| &mut docs[i]) {
                    // Saved last time: follow its first line if the file moved it meanwhile
                    if let Some(want) = n.anchor.take() {
                        let shift = nearest_line(&doc.text, n.lines.start, &want);
                        n.lines = n.lines.start.saturating_add_signed(shift)
                            ..n.lines.end.saturating_add_signed(shift);
                    }
                    // (the file may have shrunk: both ends within it, the first not after the last)
                    let last = mv::last_line(&doc.text);
                    let a = n.lines.start.min(last);
                    let b = n.lines.end.saturating_sub(1).max(a).min(last);
                    let sel = Selection::single(SelRange::new(
                        mv::line_start(&doc.text, a),
                        mv::line_start(&doc.text, b),
                    ));
                    n.at = Some((doc.id, doc.add_mark(sel)));
                    n.lines = a..b + 1;
                    // (its lines as pinned, when not known yet — a note restored from an earlier build)
                    if n.pinned.is_none() {
                        n.pinned = lines_digest(&doc.text, &n.lines);
                    }
                }
            }
        }
    }
}

/// Offset from `line` to the nearest line (within 500) whose text is `want`, trimmed — 0 if none.
pub(crate) fn nearest_line(text: &ropey::Rope, line: usize, want: &str) -> isize {
    let is = |l: usize| l < text.len_lines() && text.line(l).to_string().trim() == want;
    if is(line) {
        return 0;
    }
    for d in 1..=500isize {
        for off in [-d, d] {
            if line.checked_add_signed(off).is_some_and(is) {
                return off;
            }
        }
    }
    0
}

/// A session's notes as saved in the restore file: path, lines, the first line's text, the thread. One on a path that's
/// no text is left out (`json!` panics on it, and a lossy path could be another file): it lasts until you quit, said the
/// first time (`unkept_said`: as a note is made, or a chat note answered into a thread).
pub fn to_json(notes: &Notes, docs: &[crate::document::Document]) -> Vec<Value> {
    notes
        .list
        .iter()
        // (a finding still being placed has the snapshot's lines and a copy that won't outlive a restart: not
        // kept on the wrong lines — its placing lands within the moment)
        // (and the chat's answers' places: kept with their conversation, never here — focus A1b)
        .filter(|n| !n.placing && !bare_chat(n) && n.path.to_str().is_some())
        .map(|n| {
            let anchor = n.anchor.clone().or_else(|| {
                let doc = docs.iter().find(|d| d.path.as_deref() == Some(n.path.as_path()))?;
                (n.lines.start < doc.text.len_lines())
                    .then(|| doc.text.line(n.lines.start).to_string().trim().to_string())
            });
            let thread: Vec<Value> = n.thread.iter().map(|e| json!([e.by.word(), e.text, e.draft])).collect();
            let pinned = n.pinned.as_ref().map(hex);
            json!({
                "path": n.path, "lines": [n.lines.start, n.lines.end], "anchor": anchor, "thread": thread,
                "pinned": pinned, "board": n.board.as_ref().map(Mark::json), "since": n.since,
            })
        })
        .collect()
}

/// Session `id`'s notes from last time, kept for its checkout — session 0's at start, each other one's as
/// it's made (`workspace_add`: back at a start or a restart, or opened again).
pub fn restore_in(ed: &mut Editor, id: crate::event::WorkspaceId) {
    let Editor { workspace: session, workspaces: sessions, docs, restore, .. } = ed;
    let Some(s) = std::iter::once(session).chain(sessions.iter_mut()).find(|l| l.id == id) else { return };
    let saved = restore.notes(&s.root);
    restore_from(&mut s.claude.notes, docs, saved);
    // (and the lines looked at in its reviews — `risk.rs` — with the added ones' text)
    s.looked.extend(restore.looked(&s.root));
    // (and its viewed marks — one made since, in this run, stays: frictions29 #73)
    for (path, v) in restore.viewed(&s.root) {
        s.viewed.entry(path).or_insert(v);
    }
    for (rel, lines) in restore.looked_lines(&s.root) {
        let kept = s.looked_lines.entry(rel).or_default();
        for l in lines {
            if !kept.iter().any(|k| k.added == l.added) {
                kept.push(l);
            }
        }
    }
    // (each once: restored again in the same run — a session ended and opened again — adds none twice)
    for (rel, orders) in restore.looked_orders(&s.root) {
        let kept = s.looked_orders.entry(rel).or_default();
        for o in orders {
            if !kept.contains(&o) {
                kept.push(o);
            }
        }
    }
}

fn restore_from(notes: &mut Notes, docs: &mut crate::docpaths::Docs, saved: Vec<Value>) {
    for v in saved {
        let (Some(path), Some(lines)) = (v["path"].as_str(), v["lines"].as_array()) else { continue };
        let (Some(a), Some(b)) =
            (lines.first().and_then(Value::as_u64), lines.get(1).and_then(Value::as_u64))
        else {
            continue;
        };
        let thread: Vec<Entry> = v["thread"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                let by = By::from_word(e[0].as_str().unwrap_or_default());
                Some(Entry { by, text: e[1].as_str()?.to_string(), draft: e[2] == true })
            })
            .collect();
        if thread.is_empty() || !Path::new(path).is_file() {
            continue;
        }
        notes.next += 1;
        notes.list.push(Note {
            id: notes.next,
            path: PathBuf::from(path),
            at: None,
            lines: a as usize..(b as usize).max(a as usize + 1),
            thread,
            waiting: false,
            anchor: v["anchor"].as_str().map(str::to_string),
            pinned: v["pinned"].as_str().and_then(of_hex),
            board: Mark::of(&v["board"]),
            placing: false,
            sentence: None,
            flow: Vec::new(),
            asked: None,
            answer: None,
            since: v["since"].as_str().map(str::to_string),
        });
    }
    attach_in(notes, docs);
}

/// Marks notes point at (the jump list's pruning keeps them).
pub fn marks(ed: &Editor) -> Vec<(DocId, u64)> {
    ed.workspace.claude.notes.list.iter().filter_map(|n| n.at).collect()
}

/// Notes over `line` in the current document, in (first line, age) order.
fn all_at(ed: &Editor, line: usize) -> Vec<&Note> {
    let Some(path) = ed.doc().path.as_deref() else { return Vec::new() };
    let mut v: Vec<&Note> =
        ed.workspace.claude.notes.list.iter().filter(|n| n.path == path && n.lines.contains(&line)).collect();
    crate::sorting::by_key(&mut v, |n| (n.lines.start, n.id));
    v
}

/// The note whose card is open in `doc` (`Notes.open`).
pub fn opened<'a>(ed: &'a Editor, doc: &crate::document::Document) -> Option<&'a Note> {
    let id = ed.workspace.claude.notes.open?;
    let path = doc.path.as_deref()?;
    ed.workspace.claude.notes.list.iter().find(|n| n.id == id && n.path == path)
}

/// The note under the cursor: the focused one (just pinned, or stepped to) if it's here, else the first —
/// so `]n` reads a crowded line in order. Your cursor's even while follow holds the view at Claude's place: a key of
/// yours acts at your cursor, never at what follow shows (#250).
pub fn here(ed: &Editor) -> Option<&Note> {
    let line = ed.cursor_line();
    first_of(ed, &all_at(ed, line), line)
}

/// Whether follow holds the view on note `n`'s lines — Claude's place, in sight but not where you are: a key of yours
/// never takes its card (#250).
fn at_claudes(ed: &Editor, n: &Note) -> bool {
    crate::follow::view_line(ed)
        .is_some_and(|l| ed.doc().path.as_deref() == Some(n.path.as_path()) && n.lines.contains(&l))
}

/// The card open in this file a key of yours may take: not one at Claude's place while follow holds the view there.
fn opened_yours(ed: &Editor) -> Option<&Note> {
    opened(ed, ed.doc()).filter(|n| !at_claudes(ed, n))
}

/// Of the notes over `line`: the one its line end shows — starting there, the focused one, else the first that is its
/// line's own (a line's answers one, `shown`) — so a key on a line takes its `¶`, never an older answer's range running
/// through it; none starting there, the focused one, else the first. As the card shows it.
fn first_of<'a>(ed: &'a Editor, v: &[&'a Note], line: usize) -> Option<&'a Note> {
    let notes = &ed.workspace.claude.notes;
    let focused = |n: &&&'a Note| Some(n.id) == notes.focus;
    let starts = || v.iter().filter(|n| n.lines.start == line);
    (starts().find(focused).or_else(|| starts().find(|n| shown(notes, n).id == n.id)))
        .or_else(|| v.iter().find(focused))
        .or(v.first())
        .map(|n| shown(notes, n))
}

/// The note `space n` and `enter` act on: the one here, else the open card's in this file — a card a turn opened leaves
/// your cursor where it was, and the card's keys are its own (`term::draw_note_card` says them only where they are).
/// While follow holds the view: your cursor's, and the open card only where it isn't Claude's place (`opened_yours`).
pub fn target(ed: &Editor) -> Option<&Note> {
    let notes = &ed.workspace.claude.notes;
    here(ed).or_else(|| opened_yours(ed).map(|n| shown(notes, n)))
}

/// Note `n` as the card and the line end show it: a chat note its line's newest answer (`group`), so what a key acts on
/// is what's in sight, never an older answer folded under it; else `n`.
fn shown<'a>(notes: &'a Notes, n: &'a Note) -> &'a Note {
    group(notes, n).first().copied().unwrap_or(n)
}

// ── The tool ────────────────────────────────────────────────────────────────

/// `note` called by Claude → the text it gets back (Err = a tool error it can correct).
pub fn tool_call(ed: &mut Editor, args: &Value) -> Result<String, String> {
    let text = args["text"].as_str().map(str::trim).filter(|t| !t.is_empty()).ok_or("text is required")?;
    if let Some(id) = args["reply_to"].as_u64() {
        let n = find_mut(ed, id).ok_or(format!("there's no note #{id}"))?;
        // (a draft — yours, or the reviewer's finding — is the user's until sent: a reply would go out with it)
        if unsent(n) {
            return Err(format!("note #{id} isn't sent to you yet — the user sends it with space v s"));
        }
        n.thread.push(Entry { by: By::Author, text: text.to_string(), draft: false });
        n.waiting = false;
        ed.workspace.claude.notes.noted_now = true;
        return Ok(format!("replied on note #{id}"));
    }
    let (path, lines) = placed(&ed.workspace.root, args)?;
    let line = lines.start;
    let id = add(ed, &path, lines, By::Author, text);
    ed.workspace.claude.notes.noted_now = true;
    ed.workspace.claude.notes.pinned_now.push(id);
    Ok(format!("note #{id} pinned at {}:{}", crate::chat::shown(&ed.workspace.root, &path), line + 1))
}

/// Where a new note goes: `path` (relative to `root`) and its lines, 0-based. In `root` only, by its real path,
/// asked before anything about the file: a path outside gets one answer whether or not it's there — a note's
/// lines go to Claude on the user's reply, and only the Read card lets Claude see outside (auditfix2 F12).
fn placed(root: &Path, args: &Value) -> Result<(PathBuf, Range<usize>), String> {
    let path =
        args["path"].as_str().filter(|p| !p.is_empty()).ok_or("path and line are required for a new note")?;
    let path = crate::chat::resolve(root, path);
    if crate::paths::inside(root, &path).is_none() {
        return Err("outside the project — notes go on its files only".into());
    }
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    if !path.is_file() {
        return Err(format!("{} isn't a file", path.display()));
    }
    Ok((path, lines_of(args)?))
}

/// The lines a note's `line` and `end_line` name (from 0, the end out).
fn lines_of(args: &Value) -> Result<Range<usize>, String> {
    let line = args["line"].as_u64().ok_or("path and line are required for a new note")?.max(1) as usize - 1;
    let end = args["end_line"].as_u64().map_or(line, |e| (e.max(1) as usize - 1).max(line));
    Ok(line..end + 1)
}

/// The most a finding of the reviewer's says (it's read by you, then maybe sent to the author).
const FINDING_MAX: usize = 1_000;

/// A finding's place and text as the reviewer's `note` gives them, checked — a file inside `root`, no
/// `reply_to`, the text cut at `FINDING_MAX` — not pinned (the board's angles record it as a claim first).
pub fn finding_of(root: &Path, args: &Value) -> Result<(PathBuf, Range<usize>, String), String> {
    let text = finding_text(args)?;
    let (path, lines) = placed(root, args)?;
    let shown = crate::chat::shown(root, &path);
    crate::paths::existing_inside(root, &path, &shown, "the reviewer pins findings in the checkout only")?;
    Ok((path, lines, text))
}

/// `finding_of` in snapshot `snap` (a review run's, bgreview): the place a file of the snapshot (its path from
/// the root — one it left out too: its change is in the diff), its lines the snapshot's. Never one named like a
/// key or a link: placing reads the file in the checkout (`place`), and that would read a key, or follow the link
/// wherever it goes.
pub fn finding_in(
    snap: &crate::snapread::Snap,
    args: &Value,
) -> Result<(String, Range<usize>, String), String> {
    use crate::snapread::{Found, Left};
    let text = finding_text(args)?;
    let given =
        args["path"].as_str().filter(|p| !p.is_empty()).ok_or("path and line are required for a new note")?;
    let rel =
        snap.rel_of(given).ok_or("outside the checkout — the reviewer pins findings in the checkout only")?;
    match snap.found(&rel) {
        Found::Left(why @ (Left::Key | Left::Link)) => {
            Err(format!("{rel}: {} — no finding on it: say it in the verdict", why.said()))
        }
        Found::File | Found::Left(_) => Ok((rel, lines_of(args)?, text)),
        Found::Folder => Err(format!("{rel} is a folder — a finding goes on a file's lines")),
        Found::Nothing => Err(format!("{given}: no such file in what the reviewer reads")),
    }
}

/// A finding's text, checked: no `reply_to`, cut at `FINDING_MAX`.
fn finding_text(args: &Value) -> Result<String, String> {
    if args.get("reply_to").is_some_and(|r| !r.is_null()) {
        return Err("the reviewer pins new findings only — reply_to isn't one of its arguments".into());
    }
    let text = args["text"].as_str().map(str::trim).filter(|t| !t.is_empty()).ok_or("text is required")?;
    Ok(match text.char_indices().nth(FINDING_MAX) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    })
}

/// A finding of the reviewer's pinned on `lines` of `path` (checked by `finding_of`/`finding_in`), a draft — the
/// board's with its `mark`. `from`: the file as its reviewer read it (bgreview: its path in the snapshot and the
/// snapshot), `lines` that text's — placed on the lines now on a worker (`place`), unmarked until then; None:
/// `lines` are the file's now.
pub fn pin_finding(
    ed: &mut Editor,
    path: &Path,
    lines: Range<usize>,
    text: &str,
    mark: Option<Mark>,
    from: Option<(String, std::sync::Arc<crate::snapread::Snap>)>,
) -> u64 {
    // (what its round was since: its change lands there — #248)
    let since = ed.workspace.reviewer.as_ref().and_then(|r| r.since.clone());
    let Some((from, snap)) = from else {
        let id = add(ed, path, lines, By::Reviewer, text);
        if let Some(n) = find_mut(ed, id) {
            n.thread[0].draft = true;
            (n.board, n.since) = (mark, since);
        }
        return id;
    };
    unkept_said(ed, path);
    ed.workspace.claude.notes.next += 1;
    let id = ed.workspace.claude.notes.next;
    ed.workspace.claude.notes.list.push(Note {
        id,
        path: path.to_path_buf(),
        at: None,
        lines: lines.clone(),
        thread: vec![Entry { by: By::Reviewer, text: text.trim().to_string(), draft: true }],
        waiting: false,
        anchor: None,
        pinned: None,
        board: mark,
        placing: true,
        sentence: None,
        flow: Vec::new(),
        asked: None,
        answer: None,
        since,
    });
    place(ed, id, from, snap, lines);
    id
}

/// Finding `id`, `lines` of the text its reviewer read (file `from` of `snap`), placed on its file's lines now —
/// the open document's text, else the file on disk — on a worker (`placed_lines`); `pinned` the lines as read, so
/// a finding on lines changed since is a stale draft (`stale_check`) and its reviewer says so at its end
/// (`reviewer::placed`). The document edited meanwhile: placed again on its text then.
fn place(
    ed: &mut Editor,
    id: u64,
    from: String,
    snap: std::sync::Arc<crate::snapread::Snap>,
    lines: Range<usize>,
) {
    let Some(path) = find(ed, id).map(|n| n.path.clone()) else { return };
    let seen = open_text(ed, &path);
    let (at, read) = (seen.as_ref().map(|s| (s.0, s.1)), seen.map(|s| s.2));
    let file = path.clone();
    ed.events.jobs().spawn(move || {
        // (one it left out — past its size, a link — has no text of its own here: the checkout's as it is)
        // (never through a link, nor waiting on a FIFO a run put there since: `checkout_bytes`)
        let checkout =
            || crate::codereview::checkout_bytes(&file).map(|b| String::from_utf8_lossy(&b).into_owned());
        let then = match snap.text(&from) {
            Some(t) => Some(String::from_utf8_lossy(&t).into_owned()),
            None => checkout(),
        };
        let then = then.map(|t| ropey::Rope::from_str(&t));
        let now = read.or_else(|| checkout().map(|t| ropey::Rope::from_str(&t)));
        let (to, changed) = match (&then, &now) {
            (Some(then), Some(now)) => placed_lines(&then.to_string(), &now.to_string(), &lines),
            // (the file gone since: on its path, changed)
            _ => (lines.clone(), true),
        };
        let pinned = then.as_ref().and_then(|t| lines_digest(t, &lines));
        move |ed: &mut Editor| {
            // (its file opened or edited since it was read: placed again on what's there now)
            if find(ed, id).is_some() && open_text(ed, &path).map(|o| (o.0, o.1)) != at {
                return place(ed, id, from, snap, lines);
            }
            // (the last hold on its texts may be this one: freed on a worker)
            crate::event::drop_later(snap);
            // (closed meanwhile: nothing to place, but its reviewer counts it placed)
            if find(ed, id).is_none() {
                return crate::reviewer::placed(ed, id, false);
            }
            if let Some(n) = find_mut(ed, id) {
                (n.lines, n.pinned, n.placing) = (to, pinned, false);
            }
            if changed {
                ed.workspace.claude.notes.since_read.push(id);
            }
            attach(ed);
            crate::reviewer::placed(ed, id, changed);
        }
    });
}

/// Whether finding `id` was placed on lines changed since its reviewer read them (`place`).
#[cfg(test)]
pub fn changed_since_read(ed: &Editor, id: u64) -> bool {
    ed.workspace.claude.notes.since_read.contains(&id)
}

/// The open document of `path` (loaded): its id, version and text.
fn open_text(ed: &Editor, path: &Path) -> Option<(DocId, u64, ropey::Rope)> {
    let doc = ed.docs.iter().find(|d| !d.loading && d.path.as_deref() == Some(path))?;
    Some((doc.id, doc.version(), doc.text.clone()))
}

#[cfg(test)]
#[test]
fn a_finding_is_placed_on_the_lines_now() {
    let then = "a\nb\nc\nd\ne\n";
    // (two lines put in above: moved down by two)
    assert_eq!(placed_lines(then, "x\ny\na\nb\nc\nd\ne\n", &(2..3)), (4..5, false));
    // (a line taken out above: up by one)
    assert_eq!(placed_lines(then, "b\nc\nd\ne\n", &(3..5)), (2..4, false));
    // (its own line changed: placed there, changed)
    assert_eq!(placed_lines(then, "a\nb\nC\nd\ne\n", &(2..3)), (2..3, true));
    // (a line put in between its lines: changed)
    assert_eq!(placed_lines(then, "a\nb\nc\nnew\nd\ne\n", &(2..4)), (2..5, true));
    // (a change below it: nothing)
    assert_eq!(placed_lines(then, "a\nb\nc\nd\nE\n", &(1..2)), (1..2, false));
    // (its lines taken out: where they were, changed)
    assert_eq!(placed_lines(then, "a\nd\ne\n", &(1..3)), (1..2, true));
}

/// The reviewer's `note` as tests send it: a finding of the checkout at `root` pinned there (`reviewer::note`
/// does it for a review run, in what it reads).
#[cfg(test)]
pub fn finding(ed: &mut Editor, root: &Path, args: &Value) -> Result<(u64, String), String> {
    let (path, lines, text) = finding_of(root, args)?;
    let (shown, line) = (crate::chat::shown(root, &path), lines.start);
    let id = pin_finding(ed, &path, lines, &text, None, None);
    Ok((id, format!("finding #{id} pinned at {shown}:{}", line + 1)))
}

/// Lines `lines` of text `then` placed in text `now` (tarae's own diff, `git::hunks`): moved by what was added or
/// taken out above them; one in a change since — `changed` — placed where that change is now.
pub fn placed_lines(then: &str, now: &str, lines: &Range<usize>) -> (Range<usize>, bool) {
    let hunks = crate::git::hunks(then, now);
    let at = |x: usize| {
        let mut delta: isize = 0;
        for h in &hunks {
            // (in order: one after it moves nothing; lines put in at its place push it down)
            if x < h.base.start {
                break;
            }
            if x < h.base.end {
                return h.lines.start;
            }
            delta += h.lines.len() as isize - h.base.len() as isize;
        }
        x.saturating_add_signed(delta)
    };
    let (a, b) = (lines.start, lines.end.max(lines.start + 1));
    // (a change over its lines, or lines put in between them)
    let changed = hunks.iter().any(|h| match h.base.is_empty() {
        true => a < h.base.start && h.base.start < b,
        false => h.base.start < b && a < h.base.end,
    });
    let (first, last) = (at(a), at(b - 1));
    (first..last.max(first) + 1, changed)
}

/// The chat turn ended: notes still waiting stop waiting — a single one gets Claude's plain answer (it didn't
/// use `reply_to`); a review's are answered in the chat.
pub fn turn_done(ed: &mut Editor, answer: Option<String>) {
    let ids = std::mem::take(&mut ed.workspace.claude.notes.turn);
    let single = ids.len() == 1;
    for id in ids {
        let Some(n) = find_mut(ed, id) else { continue };
        if !n.waiting {
            continue;
        }
        n.waiting = false;
        if single && let Some(a) = answer.as_ref().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()) {
            n.thread.push(Entry { by: By::Author, text: a, draft: false });
        }
    }
    stale_check(ed);
}

/// Whether a draft's lines are still as they were pinned: at `lines` in `text`, or moved within 500 lines
/// (a file not open has no mark to follow lines added above).
fn as_pinned(text: &ropey::Rope, lines: &Range<usize>, pinned: &[u8; 32]) -> bool {
    let at = |off: isize| {
        let (Some(a), Some(b)) = (lines.start.checked_add_signed(off), lines.end.checked_add_signed(off))
        else {
            return false;
        };
        lines_digest(text, &(a..b)).as_ref() == Some(pinned)
    };
    at(0) || (1..=500isize).any(|d| at(-d) || at(d))
}

/// A chat turn ended: the drafts — your comments, the reviewer's findings, waiting for `space v s` — whose
/// lines changed since they were pinned (fixed, maybe, by the author asked in the chat — #197) are offered to
/// close, on a card (`stale_card`); each is asked about once. Compared on a worker: an open file's text as
/// it is (its notes follow their marks), a file not open read from disk.
pub fn stale_check(ed: &mut Editor) {
    type Wanted = (u64, PathBuf, Range<usize>, [u8; 32], Option<(ropey::Rope, bool)>);
    let (notes, docs) = (&ed.workspace.claude.notes, &ed.docs);
    let wanted: Vec<Wanted> = notes
        .list
        .iter()
        .filter(|n| n.thread.iter().any(|e| e.draft) && !notes.stale_asked.contains(&n.id))
        .filter_map(|n| {
            let pinned = n.pinned?;
            let doc = docs.iter().find(|d| !d.loading && d.path.as_deref() == Some(n.path.as_path()));
            Some((
                n.id,
                n.path.clone(),
                n.lines.clone(),
                pinned,
                doc.map(|d| (d.text.clone(), n.at.is_some())),
            ))
        })
        .collect();
    if wanted.is_empty() {
        return;
    }
    ed.events.jobs().spawn(move || {
        let stale: Vec<u64> = wanted
            .into_iter()
            .filter(|(_, path, lines, pinned, open)| match open {
                // (on its mark: where its lines are now)
                Some((text, true)) => lines_digest(text, lines).as_ref() != Some(pinned),
                Some((text, false)) => !as_pinned(text, lines, pinned),
                None => std::fs::read_to_string(path)
                    .map_or(true, |t| !as_pinned(&ropey::Rope::from_str(&t), lines, pinned)),
            })
            .map(|w| w.0)
            .collect();
        move |ed: &mut Editor| stale_card(ed, stale)
    });
}

/// The drafts a card names at most.
const STALE_SHOWN: usize = 6;

/// The card for drafts `ids` whose lines changed since they were pinned: `y` closes them, `r` lists them one
/// by one, `n` keeps them. One card a session (cardburden 6 — a card each turn's end, before): a later turn's
/// drafts join the card up, its list grown in place — read again before its `y` counts (`rev`).
pub(crate) fn stale_card(ed: &mut Editor, ids: Vec<u64>) {
    let notes = &ed.workspace.claude.notes;
    let new: Vec<u64> = notes
        .list
        .iter()
        .filter(|n| ids.contains(&n.id) && is_draft(n) && !notes.stale_asked.contains(&n.id))
        .map(|n| n.id)
        .collect();
    if new.is_empty() {
        ed.unchanged = true;
        return;
    }
    ed.workspace.claude.notes.stale_asked.extend(&new);
    let up = stale_ids(ed).unwrap_or_default();
    let all: Vec<u64> =
        up.iter().chain(&new).copied().filter(|&id| find(ed, id).is_some_and(is_draft)).collect();
    let (title, q, decided) = stale_text(ed, &all);
    match stale_offer(ed) {
        Some(o) => {
            (o.title, o.question, o.decided) = (title, q, Some(decided));
            o.what = crate::offer::What::NotesStale { ids: all };
            o.rev += 1;
        }
        None => {
            let what = crate::offer::What::NotesStale { ids: all };
            let o = crate::offer::Offer::new(title, &q, "", String::new(), what).deciding(decided);
            ed.push_offer(o);
        }
    }
}

/// Whether note `n` holds a draft — it waits for `space v s`.
fn is_draft(n: &Note) -> bool {
    n.thread.iter().any(|e| e.draft)
}

/// The stale card's title, its text and the lines saying what it closes (its list), for drafts `ids`.
fn stale_text(ed: &Editor, ids: &[u64]) -> (String, String, Range<usize>) {
    let stale: Vec<&Note> = ids.iter().filter_map(|&id| find(ed, id)).collect();
    let k = stale.len();
    // (findings placed on lines changed since their reviewer read them — bgreview — or drafts since pinned)
    let read = stale.iter().filter(|n| ed.workspace.claude.notes.since_read.contains(&n.id)).count();
    let mut q = String::from(match read {
        0 => "Their lines changed since they were pinned — fixed in the chat, maybe:\n",
        r if r == k => "Their lines changed since the reviewer read them:\n",
        _ => "Their lines changed since they were pinned, or since the reviewer read them:\n",
    });
    for n in stale.iter().take(STALE_SHOWN) {
        let by = n.thread.first().map_or(By::Author, |e| e.by).label();
        let at = format!("{}:{}", crate::chat::shown(&ed.workspace.root, &n.path), n.lines.start + 1);
        q.push_str(&format!("\n  {at} · {by} · {}", n.gist()));
    }
    if k > STALE_SHOWN {
        q.push_str(&format!("\n  … {} more — r lists them all", k - STALE_SHOWN));
    }
    let listed = q.split('\n').count();
    q.push_str("\n\nKept, space v s still sends them.");
    let what = if read == k { "finding" } else { "draft" };
    (format!("Close {} on changed lines?", crate::nav::count(k, what)), q, 0..listed)
}

/// Where this session's stale card is: up in front (`offers`), or parked while the session is behind.
fn stale_offer(ed: &mut Editor) -> Option<&mut crate::offer::Offer> {
    let is = |o: &crate::offer::Offer| matches!(o.what, crate::offer::What::NotesStale { .. });
    match ed.background() {
        true => ed.workspace.parked.iter_mut().find(|o| is(o)),
        false => ed.offers.iter_mut().find(|o| is(o)),
    }
}

/// The drafts this session's stale card lists — None: no card up.
fn stale_ids(ed: &mut Editor) -> Option<Vec<u64>> {
    match &stale_offer(ed)?.what {
        crate::offer::What::NotesStale { ids } => Some(ids.clone()),
        _ => None,
    }
}

/// The drafts the stale card in front lists — each says `stale ·` while it's up (its line end, its card, the
/// review rail's count). Empty: none up.
pub fn stale_shown(ed: &Editor) -> &[u64] {
    ed.offers
        .iter()
        .find_map(|o| match &o.what {
            crate::offer::What::NotesStale { ids } => Some(ids.as_slice()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Every event: the stale card follows its drafts — one closed or sent since (`C-x` in its list, `space v s`,
/// `:note-close`) leaves its list; none left, the card goes. Fewer, its `y` closes less than was read: not to
/// be read again.
pub fn stale_sync(ed: &mut Editor) {
    let Some(ids) = stale_ids(ed) else { return };
    let left: Vec<u64> = ids.iter().copied().filter(|&id| find(ed, id).is_some_and(is_draft)).collect();
    if left.len() == ids.len() {
        return;
    }
    if left.is_empty() {
        let is = |o: &crate::offer::Offer| matches!(o.what, crate::offer::What::NotesStale { .. });
        let gone = match ed.background() {
            true => ed.workspace.parked.iter().find(|o| is(o)).map(|o| o.id),
            false => ed.offers.iter().find(|o| is(o)).map(|o| o.id),
        };
        if let Some(id) = gone {
            ed.card_how(id, "its drafts closed or sent");
            ed.workspace.parked.retain(|o| o.id != id);
            ed.offer_finished(id, Ok(()));
        }
        return;
    }
    let (title, q, decided) = stale_text(ed, &left);
    if let Some(o) = stale_offer(ed) {
        (o.title, o.question, o.decided) = (title, q, Some(decided));
        o.what = crate::offer::What::NotesStale { ids: left };
    }
}

/// The stale drafts list's title (`r` on the card): the notes list of just those, `C-x` closing one.
pub const STALE: &str = "notes · stale";

/// Whether a picker titled `title` is a list of notes (the notes list, or the stale card's).
pub fn is_list(title: &str) -> bool {
    title == NOTES || title == STALE
}

/// `r` on the stale card: its drafts in the notes list, alone — `enter` goes to one, `C-x` closes it (the
/// card's list follows, `stale_sync`). No key of the code's changes meaning; the card stays under it.
pub fn stale_list(ed: &mut Editor) {
    let items = list_items(ed, STALE);
    ed.open_picker(crate::picker::Picker::of("notes-stale", STALE, items, false), None);
}

/// The rows of the list titled `title`: every note, or the stale card's drafts.
pub fn list_items(ed: &Editor, title: &str) -> Vec<crate::picker::Item> {
    let mut items = picker_items(ed);
    if title == STALE {
        let shown = stale_shown(ed);
        items.retain(|i| matches!(i.action, crate::picker::Action::Note { id, .. } if shown.contains(&id)));
    }
    items
}

/// The stale drafts card's answer: `y` closes notes `ids` (those still there), `n` keeps them.
pub fn stale_answer(ed: &mut Editor, card: u64, ids: Vec<u64>, yes: bool) {
    ed.offer_finished(card, Ok(()));
    if !yes {
        return ed.set_status("kept — space v s still sends them, :note-close drops one");
    }
    let before = ed.workspace.claude.notes.list.len();
    ed.workspace.claude.notes.list.retain(|n| !ids.contains(&n.id));
    let k = before - ed.workspace.claude.notes.list.len();
    ed.set_status(format!("{} closed", crate::nav::count(k, "draft")));
}

/// Whether note `n` is a finding of the reviewer's that leaves with its change (#248): unsent, placed, nothing of
/// yours in its thread (your words never go unasked — `close_by`).
fn landable(n: &Note) -> bool {
    unsent(n) && n.thread[0].by == By::Reviewer && !n.placing && n.thread.iter().all(|e| e.by != By::You)
}

/// The reviewer's unsent findings whose change the base holds now leave, said once (#248: a piece's findings left
/// on the rail went out with the next piece's at `space v s`, a turn spent answering them) — as a bring lands and as
/// a new round begins. A finding is on a landed change when its file has no change since its base (a session's own
/// base, else the start's branch; the checkout tarae started in: the branch its round was since — `Note.since`, each
/// finding its own — else HEAD, committed there being in it) — or on one undone: either way nothing of it is left to
/// review, and the words say no more. One on a file still changed stays (its lines changed since: the stale card's).
/// Looked at on a worker, each base once.
pub fn landed_check(ed: &mut Editor) {
    let session = match ed.workspace.id == crate::event::WorkspaceId::HOME {
        true => None,
        false => match ed.workspace.base.clone().or_else(|| ed.start_branch()) {
            Some(b) => Some(b),
            None => return,
        },
    };
    // (in the checkout tarae started in, reviewed since a branch: a finding there is on a commit of this branch, not
    // in that one until it's brought — rounds of the uncommitted changes between leave it so)
    let wanted: Vec<(u64, PathBuf, Option<String>)> = (ed.workspace.claude.notes.list.iter())
        .filter(|n| landable(n))
        .map(|n| (n.id, n.path.clone(), session.clone().or_else(|| n.since.clone())))
        .collect();
    if wanted.is_empty() {
        return;
    }
    let root = ed.workspace.root.clone();
    ed.events.jobs().spawn(move || {
        let mut bases: Vec<(Option<String>, Option<Changed>)> = Vec::new();
        let mut landed = Vec::new();
        for (id, path, base) in wanted {
            if !bases.iter().any(|(b, _)| *b == base) {
                let changed = crate::codereview::changed_since(&root, base.as_deref());
                if let Err(e) = &changed {
                    crate::log::line("notes", "info", &format!("landed findings not looked at: {e}"));
                }
                bases.push((base.clone(), changed.ok()));
            }
            let Some((_, Some((top, changed)))) = bases.iter().find(|(b, _)| *b == base) else { continue };
            // (a file outside the repository — a folder's: not its to say)
            if crate::paths::real(&path).is_some_and(|p| p.starts_with(top) && !changed.contains(&p)) {
                landed.push((id, base));
            }
        }
        move |ed: &mut Editor| landed_close(ed, landed)
    });
}

/// A checkout's top and the files changed there since a base (`codereview::changed_since`).
type Changed = (PathBuf, Vec<PathBuf>);

/// `landed_check`'s findings closed, each with its base — those still unsent and not answered by you — said once.
fn landed_close(ed: &mut Editor, landed: Vec<(u64, Option<String>)>) {
    let landed: Vec<(u64, Option<String>)> =
        landed.into_iter().filter(|(id, _)| find(ed, *id).is_some_and(landable)).collect();
    ed.workspace.claude.notes.list.retain(|n| !landed.iter().any(|(id, _)| *id == n.id));
    let closed: Vec<Option<String>> = landed.into_iter().map(|(_, base)| base).collect();
    let k = closed.len();
    if k == 0 {
        ed.unchanged = true;
        return;
    }
    // (landed or undone — which, the check can't tell: it says what it saw)
    let (its, has) = if k == 1 { ("its file", "has") } else { ("their files", "have") };
    let what = match &closed[0] {
        _ if closed.iter().any(|b| *b != closed[0]) => "no change since their bases".to_string(),
        Some(b) => format!("no change since {b}"),
        None => "no uncommitted change".to_string(),
    };
    ed.set_status(format!(
        "closed {} of the reviewer's — {its} {has} {what} now (landed or undone)",
        crate::nav::count(k, "unsent finding")
    ));
}

/// Every note `by` began closed — every note, `None` — said: `:notes-clear [who]`, the notes list's `A-x`
/// (#197). When some hold your unsent drafts (gone with them), a card asks first (`What::NotesClose`) — the
/// notes as they are now, one added later isn't among them. Whether it asked.
pub fn close_by(ed: &mut Editor, by: Option<By>) -> bool {
    let whose = |n: &Note| by.is_none() || n.thread.first().map(|e| e.by) == by;
    let ids: Vec<u64> = ed.workspace.claude.notes.list.iter().filter(|n| whose(n)).map(|n| n.id).collect();
    let drafts: Vec<&Note> = ed
        .workspace
        .claude
        .notes
        .list
        .iter()
        .filter(|n| whose(n) && n.thread.iter().any(|e| e.by == By::You && e.draft))
        .collect();
    if drafts.is_empty() {
        close_ids(ed, by, &ids);
        return false;
    }
    let k = drafts.len();
    let mut q = format!(
        "{} your unsent drafts — closed, they're gone:\n",
        if k == 1 { "One holds" } else { "These hold" }
    );
    for n in drafts.iter().take(STALE_SHOWN) {
        let at = format!("{}:{}", crate::chat::shown(&ed.workspace.root, &n.path), n.lines.start + 1);
        q.push_str(&format!("\n  {at} · {}", n.gist()));
    }
    if k > STALE_SHOWN {
        q.push_str(&format!("\n  … {} more", k - STALE_SHOWN));
    }
    q.push_str("\n\nKept, space v s still sends them; C-x in the notes list closes one.");
    let title = match by {
        Some(by) => format!("Close the {} {} began?", crate::nav::count(ids.len(), "note"), by.label()),
        None => format!("Close all {}?", crate::nav::count(ids.len(), "note")),
    };
    let what = crate::offer::What::NotesClose { by, ids };
    ed.push_offer(crate::offer::Offer::new(title, &q, "", String::new(), what));
    true
}

/// The card's answer (`What::NotesClose`): `y` closes notes `ids` (those still there), `n` keeps them.
pub fn close_by_answer(ed: &mut Editor, card: u64, by: Option<By>, ids: Vec<u64>, yes: bool) {
    ed.offer_finished(card, Ok(()));
    if !yes {
        return ed.set_status("kept — space v s still sends your drafts, C-x in the notes list closes one");
    }
    close_ids(ed, by, &ids);
}

/// Notes `ids` closed, said as whose they were.
fn close_ids(ed: &mut Editor, by: Option<By>, ids: &[u64]) {
    let before = ed.workspace.claude.notes.list.len();
    ed.workspace.claude.notes.list.retain(|n| !ids.contains(&n.id));
    let n = before - ed.workspace.claude.notes.list.len();
    ed.set_status(match by {
        Some(by) => format!("{} {} began closed", crate::nav::count(n, "note"), by.label()),
        None => format!("{} closed", crate::nav::count(n, "note")),
    });
}

/// Note `id` closed, said.
pub fn close(ed: &mut Editor, id: u64) {
    ed.workspace.claude.notes.list.retain(|n| n.id != id);
    ed.set_status(format!("note #{id} closed"));
}

/// The notes list's title — its keys and hints go by it.
pub const NOTES: &str = "notes";

/// A note's row in the notes list: where it is and what it says first.
fn row_label(ed: &Editor, n: &Note) -> String {
    format!("{}:{}  {}", crate::chat::shown(&ed.workspace.root, &n.path), n.lines.start + 1, n.gist())
}

/// The note on a row of the notes list (`C-x`, `A-x`): by the id the row carries — two notes saying the
/// same on the same line are two rows — if it's still there.
pub fn of_row(ed: &Editor, item: &crate::picker::Item) -> Option<u64> {
    let crate::picker::Action::Note { id, .. } = item.action else { return None };
    find(ed, id).map(|n| n.id)
}

/// The author who began note `id`.
pub fn began(ed: &Editor, id: u64) -> Option<By> {
    find(ed, id).and_then(|n| n.thread.first()).map(|e| e.by)
}

/// Who `word` names, as `:notes-clear` takes it — the words the cards head entries with: `you`, `claude`, `reviewer`,
/// `chat`.
pub fn by_word(word: &str) -> Option<By> {
    match word {
        "reviewer" => Some(By::Reviewer),
        "chat" => Some(By::Chat),
        "claude" => Some(By::Author),
        "you" => Some(By::You),
        _ => None,
    }
}

// ── You ───────────────────────────────────────────────────────────────────

/// `space n`: reply to the note here, or start one (asking Claude about the line or selection) — at your cursor: follow,
/// holding the view, pauses and the view comes back to it (`follow::your_view`).
pub fn start(ed: &mut Editor) {
    let at = target(ed).map(|n| n.id);
    crate::follow::your_view(ed);
    start_on(ed, at);
}

/// A press on the open card's words: a reply to that card, wherever your cursor is.
pub fn start_card(ed: &mut Editor) {
    let notes = &ed.workspace.claude.notes;
    let at = notes.open.and_then(|o| find(ed, o)).map(|n| shown(notes, n).id);
    start_on(ed, at);
}

/// The note prompt open on note `target`, none: a new note on the cursor's lines.
fn start_on(ed: &mut Editor, target: Option<u64>) {
    // On a walk-through's stop: it holds still while you ask
    crate::follow::hold(ed);
    // In a review a new one is a comment, kept as a draft — said so, on its line (#134: "ask claude:" wasn't
    // taken for the review's comment input the rail's `space n comment` promised)
    let comment = (target.is_none() && crate::codereview::drafting(ed))
        .then(|| format!("comment on line {}:", ed.cursor_line() + 1));
    ed.open_prompt(crate::editor::PromptKind::Note(target), "");
    if let Some(p) = ed.prompt.as_mut().filter(|_| comment.is_some()) {
        p.label = comment;
    }
}

/// The prompt was sent.
pub fn submit(ed: &mut Editor, target: Option<u64>, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    // (Claude answering: the reply is in its thread now and goes at the turn's end — frictions29 18)
    let busy =
        ed.workspace.claude.chat.as_ref().is_some_and(|c| c.busy()) && !crate::codereview::drafting(ed);
    // During a review it's a draft: it goes with the rest (`space v s`), nothing is sent now
    let draft = crate::codereview::drafting(ed);
    let fresh = target.filter(|&id| find(ed, id).is_some()).is_none();
    // A question at a walk-through's stop is about the stop: its lines, and what Claude said they do
    let stop = (fresh && !draft).then(|| crate::follow::stop_yours(ed)).flatten();
    let id = match target.filter(|&id| find(ed, id).is_some()) {
        Some(id) => {
            let n = find_mut(ed, id).expect("checked");
            n.thread.push(Entry { by: By::You, text: text.to_string(), draft });
            n.waiting = !draft;
            // (a chat note answered is a thread now, the restore file's)
            let path = n.path.clone();
            unkept_said(ed, &path);
            id
        }
        None => {
            let Some(path) = ed.doc().path.clone() else {
                return ed.set_status("save the file first — notes live in files");
            };
            let doc = ed.doc();
            let r = doc.selection().primary();
            let (a, b) = (
                mv::line_of(&doc.text, r.from()),
                mv::line_of(&doc.text, r.to().saturating_sub(1).max(r.from())),
            );
            let lines = match &stop {
                Some((_, lines, _)) if a == b => lines.clone(),
                _ => a..b + 1,
            };
            let id = add(ed, &path, lines, By::You, text);
            if draft && let Some(n) = find_mut(ed, id) {
                n.waiting = false;
                n.thread[0].draft = true;
            }
            id
        }
    };
    if draft {
        let k = ed.workspace.claude.notes.list.iter().filter(|n| n.thread.iter().any(|e| e.draft)).count();
        // (outside a review too, while drafts wait — a background reviewer's findings among them: bgreview)
        // (in a review the rail counts them; outside one it's said where you look, the user's decision 2026-10-19)
        return match ed.workspace.code_review.is_some() {
            true => ed.note(format!("draft saved — {k} in this review · space v s sends them")),
            false => ed.set_status(format!("draft saved — {k} waiting · space v s sends them")),
        };
    }
    let why = stop.map(|(_, _, why)| why.to_string());
    if busy && let Some(c) = ed.workspace.claude.chat.as_mut() {
        c.sends.push(crate::chat::Later::Reply { id, text: text.to_string(), fresh, why });
        return ed.set_status("reply queued — it goes when Claude's turn ends");
    }
    send(ed, id, text, fresh, why, None);
}

/// Note `id`'s message goes to Claude — `text` the reply just added (`fresh`: the note with it), `why` what
/// the user said at a walk-through's stop. On a file named like a key Claude may not read, the card asks
/// first (`reads::ask_own`), and its answer sends it again: `lines` Some(true) with the lines, Some(false)
/// with the file's name only (None: not asked yet).
pub fn send(ed: &mut Editor, id: u64, text: &str, fresh: bool, why: Option<String>, lines: Option<bool>) {
    // The chat panel carries the conversation — opened in the background if closed; the keys stay here
    if ed.workspace.claude.chat.is_none() {
        let at = ed.workspace.viewer.at;
        crate::chat::open(ed);
        ed.workspace.viewer.at = at;
        if let Some(c) = &mut ed.workspace.claude.chat {
            c.unfolded = false;
        }
    }
    let Some(path) = find(ed, id).map(|n| n.path.clone()) else { return };
    // (the chat just opened: what the project keeps open is read first — no card about one it does)
    let own = || crate::reads::Own::Note { id, text: text.to_string(), fresh, why: why.clone() };
    if lines.is_none() && crate::reads::wait_for_kept(ed, std::slice::from_ref(&path), own()) {
        return;
    }
    let Some(n) = find(ed, id) else { return };
    if lines.is_none() && crate::reads::key_shut(ed, &n.path, &[]) {
        let (key, text) = (n.path.clone(), text.to_string());
        return crate::reads::ask_own(ed, vec![key], crate::reads::Own::Note { id, text, fresh, why });
    }
    let chip = format!(
        "¶ #{id} · {} L{}",
        n.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
        n.lines.start + 1
    );
    let mut about = message(ed, n, lines != Some(false));
    if let Some(why) = &why {
        let at = about.len() - "</note>".len();
        about.insert_str(at, &format!("(a stop of your walk-through — you said: {why})\n"));
    }
    if crate::chat::submit(ed, text, Some(about), Some(chip)) {
        ed.workspace.claude.notes.turn = vec![id];
        return;
    }
    // (its claude starting — the project's settings read first: the reply waits for it, frictions29 #67)
    if let Some(c) = ed.workspace.claude.chat.as_mut().filter(|c| c.settings.waiting()) {
        c.sends.push(crate::chat::Later::Reply { id, text: text.to_string(), fresh, why });
        return ed.set_status("reply queued — claude is starting; it goes once it's up");
    }
    // Claude never saw it — take it back (the chat panel says why)
    if fresh {
        ed.workspace.claude.notes.list.retain(|n| n.id != id);
    } else if let Some(n) = find_mut(ed, id) {
        // (this reply — the last of yours with its text, not merely the last: one queued while Claude worked may
        // have others after it by now)
        if let Some(at) = n.thread.iter().rposition(|e| e.by == By::You && e.text == text && !e.draft) {
            n.thread.remove(at);
        }
        n.waiting = false;
    }
    ed.set_warning("couldn't reach claude — the chat panel says why");
}

/// What goes to Claude with a reply: where the note is, the code there (`code`; else what the context says of
/// a file named like a key — `reads::NOT_SENT`) and the thread so far.
fn message(ed: &Editor, n: &Note, code: bool) -> String {
    let withheld = (!code).then(|| format!("({})\n", crate::reads::NOT_SENT));
    let code = withheld.unwrap_or_else(|| {
        ed.docs
            .iter()
            .find(|d| d.path.as_deref() == Some(n.path.as_path()))
            .map(|d| {
                let last = mv::last_line(&d.text);
                let (a, b) = (n.lines.start.min(last), n.lines.end.saturating_sub(1).min(last));
                d.text
                    .slice(d.text.line_to_char(a)..d.text.line_to_char((b + 1).min(d.text.len_lines())))
                    .to_string()
            })
            .unwrap_or_default()
    });
    let mut s = format!(
        "<note id=\"{}\" path=\"{}\" lines=\"{}-{}\">\n<code>\n{}</code>\n",
        n.id,
        crate::chat::shown(&ed.workspace.root, &n.path),
        n.lines.start + 1,
        n.lines.end,
        code
    );
    // The thread up to (not including) the message being sent
    for e in &n.thread[..n.thread.len().saturating_sub(1)] {
        // (a chat note's first entry is Claude's own answer: said as its words)
        s.push_str(if e.by == By::Chat { "claude, in the chat" } else { e.by.word() });
        s.push_str(": ");
        s.push_str(&e.text);
        s.push('\n');
    }
    s.push_str("</note>");
    s
}

/// The note `:note-close` closes: the one on your cursor's line (of several, the one its open card shows); with none
/// there, the open card's in this file. A card open on another line never takes a note at your cursor's place, and one in
/// another file isn't in sight to close. While follow holds the view it's your key: at your cursor, never Claude's place —
/// not that place's open card either, with none on your line. `term::draw_note_card` asks it too: a card says
/// `:note-close` only where that closes it.
pub fn close_target(ed: &Editor) -> Option<&Note> {
    let open = opened(ed, ed.doc());
    let mine = all_at(ed, ed.cursor_line());
    let card = mine.iter().find(|n| open.is_some_and(|o| o.id == n.id)).copied();
    let at = card.or(first_of(ed, &mine, ed.cursor_line())).or(opened_yours(ed));
    at.map(|n| shown(&ed.workspace.claude.notes, n))
}

/// `:note-close` — the note `close_target` names goes, for good; follow, holding the view, pauses and the view comes back
/// to your cursor (`follow::your_view`). A refusal moves nothing.
pub fn close_here(ed: &mut Editor) -> Result<(), String> {
    let id = close_target(ed).map(|n| n.id).ok_or("no note on this line")?;
    crate::follow::your_view(ed);
    close(ed, id);
    Ok(())
}

/// The newest answer's number among the chat notes (`chatnotes::Origin.n`) — the one whose places the line ends say.
pub fn newest(notes: &Notes) -> Option<u64> {
    notes.list.iter().filter(|n| bare_chat(n)).filter_map(|n| n.answer.as_ref().map(|o| o.n)).max()
}

/// Whether chat note `n` is of an answer older than the newest (its line end a faint `¶` alone).
pub fn older(notes: &Notes, n: &Note) -> bool {
    bare_chat(n) && n.answer.as_ref().is_some_and(|o| newest(notes).is_some_and(|k| o.n < k))
}

/// The notes one card shows for `n` (focus A1b): a chat note's line's answers — every chat note starting on its line,
/// newest answer first —, else `n` alone.
pub fn group<'a>(notes: &'a Notes, n: &'a Note) -> Vec<&'a Note> {
    if !bare_chat(n) {
        return vec![n];
    }
    let mut v: Vec<&Note> = (notes.list.iter())
        .filter(|m| bare_chat(m) && m.path == n.path && m.lines.start == n.lines.start)
        .collect();
    crate::sorting::by_key(&mut v, |m| (std::cmp::Reverse(m.answer.as_ref().map_or(0, |o| o.n)), m.id));
    v
}

/// The stop `]n` makes for `n`: its line's newest answer for a chat note (one card a line), else `n`.
fn stop_of(notes: &Notes, n: &Note) -> u64 {
    shown(notes, n).id
}

/// `]n`'s stops in its order (focus A1b): the newest answer's places in the order written, then every other note by
/// (file, line, age) — one stop a line's answers.
fn walk(notes: &Notes) -> Vec<(PathBuf, usize, u64)> {
    let newest = newest(notes);
    let of_newest = |n: &Note| bare_chat(n) && n.answer.as_ref().map(|o| o.n) == newest;
    let stops = notes.list.iter().filter(|n| stop_of(notes, n) == n.id);
    let (mut first, mut rest): (Vec<&Note>, Vec<&Note>) = stops.partition(|n| of_newest(n));
    crate::sorting::by_key(&mut first, |n| n.id);
    crate::sorting::by_key(&mut rest, |n| (n.path.clone(), n.lines.start, n.id));
    first.into_iter().chain(rest).map(|n| (n.path.clone(), n.lines.start, n.id)).collect()
}

/// `]n`/`[n` — the next/previous note across every file, wrapping around: after a turn the newest answer's places in
/// the order written, then the rest in (file, line, age) order (`walk`); notes sharing a line are visited one by one
/// (the card shows the one stepped to), a line's answers one stop.
pub fn step(ed: &mut Editor, forward: bool) {
    let notes = &ed.workspace.claude.notes;
    let all = walk(notes);
    // From the note shown here, else the open card's (not one at Claude's place: you aren't there), else from the cursor
    // line
    let card = notes.open.and_then(|id| find(ed, id)).filter(|n| !at_claudes(ed, n));
    let from = here(ed).or(card).map(|n| stop_of(notes, n));
    let target = match from.and_then(|id| all.iter().position(|t| t.2 == id)) {
        Some(k) if forward => all.get(k + 1).or(all.first()),
        Some(k) => k.checked_sub(1).and_then(|k| all.get(k)).or(all.last()),
        None => {
            let key = |n: &(PathBuf, usize, u64)| (n.0.clone(), n.1, n.2);
            let mut sorted = all.clone();
            crate::sorting::by_key(&mut sorted, key);
            let here = (ed.doc().path.clone().unwrap_or_default(), ed.cursor_line(), 0);
            let t = if forward {
                sorted.iter().find(|t| **t > here).or(sorted.first())
            } else {
                sorted.iter().rev().find(|t| **t < here).or(sorted.last())
            };
            t.and_then(|t| all.iter().find(|a| a.2 == t.2))
        }
    };
    match target.cloned() {
        Some((p, l, id)) => {
            // (a key of yours: follow, holding the view, pauses — said once, never left to notice a move, as a note on
            // your own line moves nothing)
            crate::follow::your_view(ed);
            ed.workspace.claude.notes.focus = Some(id);
            if let Err(e) = ed.open_at(&p, l, 0) {
                ed.set_error(format!("{e:#}"));
            }
            open(ed, id);
        }
        None => {
            ed.set_status("no notes yet — Claude pins them while it explores, or space n asks about a line")
        }
    }
}

// ── The card between the lines (focus A1b) ──────────────────────────────────────────────────────

/// Note `id`'s card open (`Notes.open`, any other closed): its lines and the card under them put in sight as one block
/// where its file is in front. A code too narrow for a card says so (its line end stays).
pub fn open(ed: &mut Editor, id: u64) {
    let notes = &mut ed.workspace.claude.notes;
    (notes.open, notes.turned, notes.earlier, notes.opening) = (Some(id), false, false, None);
    ed.note_marked = None;
    if ed.viewport.1 < crate::term::NOTE_MIN + 4 && ed.viewport.1 > 0 {
        ed.set_status("too narrow for the note's card here — space N lists every note whole");
    }
    let lines =
        find(ed, id).filter(|n| ed.doc().path.as_deref() == Some(n.path.as_path())).map(|n| n.lines.clone());
    if let Some(lines) = lines {
        ed.show_range(lines);
    }
}

/// A turn's end: its answer's first place's card opens (focus A1b) — no keys taken, your cursor where it is; where
/// follow leads, its view takes the line and the card in sight (`follow::show_card`); not while you type (`typing`: it
/// waits, `opening_sync`), never over a card you opened yourself, which stays until you close it. It stays until the
/// next answer opens its own or you close it.
pub fn open_by_turn(ed: &mut Editor, id: u64) {
    let notes = &mut ed.workspace.claude.notes;
    // (a card of yours whose note is gone holds nothing: five paths close a note, none says so here)
    let open = notes.open.filter(|o| notes.list.iter().any(|n| n.id == *o));
    if open.is_some_and(|o| o != id) && !notes.turned {
        return;
    }
    match typing(ed) {
        true => ed.workspace.claude.notes.opening = Some(id),
        false => {
            // (the same card open — a new answer on its line: it stays as it was, the newest on top)
            let notes = &ed.workspace.claude.notes;
            let same = (notes.open.and_then(|o| find(ed, o)))
                .is_some_and(|o| group(notes, o).iter().any(|g| g.id == id));
            let notes = &mut ed.workspace.claude.notes;
            (notes.open, notes.turned, notes.opening) = (Some(id), true, None);
            notes.earlier &= same;
            if !same {
                ed.note_marked = None;
            }
            // (in sight with its card, where follow leads — its line alone was, the card's foot below the pane)
            if let Some((path, lines)) = find(ed, id).map(|n| (n.path.clone(), n.lines.clone())) {
                crate::follow::show_card(ed, &path, lines);
            }
        }
    }
}

/// Whether you're typing — in the code's insert mode, a prompt, or a message in the chat's input: a turn's card waits.
fn typing(ed: &Editor) -> bool {
    ed.mode == crate::editor::Mode::Insert
        || ed.prompt.is_some()
        || ed.workspace.claude.chat.as_ref().is_some_and(|c| !c.input.is_empty())
}

/// Every event: a turn's card that waited for you to stop typing opens — if its note is still there and no card of
/// yours opened meanwhile.
pub fn opening_sync(ed: &mut Editor) {
    let Some(id) = ed.workspace.claude.notes.opening else { return };
    if find(ed, id).is_none() {
        ed.workspace.claude.notes.opening = None;
    } else if !typing(ed) {
        ed.workspace.claude.notes.opening = None;
        open_by_turn(ed, id);
        ed.unchanged = false;
    }
}

/// A new conversation, or another continued: the chat notes of the one left go (`chatnotes.rs` — a thread you began
/// on one stays, it's yours), their card with them.
pub fn forget_answers(ed: &mut Editor) {
    let notes = &mut ed.workspace.claude.notes;
    notes.list.retain(|n| !bare_chat(n));
    notes.answer = None;
    let alive = |id: &u64| notes.list.iter().any(|n| n.id == *id);
    notes.open = notes.open.filter(alive);
    notes.opening = notes.opening.filter(alive);
    notes.turned &= notes.open.is_some();
    notes.answers = 0;
    notes.kept_bare = 0;
    notes.left = None;
}

/// How many earlier answers the card of `n` folds (`… N earlier`): its line's answers but the newest, while they're
/// folded.
pub fn earlier_folded(notes: &Notes, n: &Note) -> usize {
    if notes.earlier { 0 } else { group(notes, n).len().saturating_sub(1) }
}

/// The keys the card layer takes now (`card_key` — the layer `keys_chain` names `Region::NoteCard`): in normal mode
/// with nothing under way, on a file's code (a woven view has no notes) — `enter` on a note's line (its card opens, or
/// closes when it's the one open), `q` and `esc` while a card is open in this file (it closes). Helix's `q`
/// (`replay_macro`) is one `esc` away.
pub fn card_keys(ed: &Editor) -> Vec<crate::key::Key> {
    use crate::key::{Code, Key};
    let free = ed.mode == crate::editor::Mode::Normal
        && ed.pending.is_empty()
        && ed.prompt.is_none()
        && ed.on_next_char.is_none()
        && !ed.doc().loading
        && crate::weave::of(ed.doc()).is_none();
    let mut keys = Vec::new();
    // (on a note's line; anywhere while the open card has earlier answers folded: its key says so — not a card at
    // Claude's place while follow holds the view there)
    let notes = &ed.workspace.claude.notes;
    let folded = opened_yours(ed).is_some_and(|o| earlier_folded(notes, o) > 0);
    if free && (here(ed).is_some() || folded) {
        keys.push(Key::plain(Code::Enter));
    }
    if free && opened(ed, ed.doc()).is_some() {
        keys.extend([Key::plain(Code::Char('q')), Key::plain(Code::Esc)]);
    }
    keys
}

/// A key of the card layer's (`card_keys`); false: not its.
pub fn card_key(ed: &mut Editor, key: crate::key::Key) -> bool {
    if !card_keys(ed).contains(&key) {
        return false;
    }
    ed.count = None;
    let notes = &ed.workspace.claude.notes;
    let here = target(ed);
    // (its card's line: the open one's group — `enter` there unfolds its earlier answers first)
    let open_here = here
        .zip(notes.open.and_then(|o| find(ed, o)))
        .filter(|(h, o)| group(notes, o).iter().any(|g| g.id == h.id));
    let folded = open_here.is_some_and(|(_, o)| earlier_folded(notes, o) > 0);
    let (id, open_here) = (here.map(|n| n.id), open_here.is_some());
    // (`enter` acts at your cursor: follow, holding the view, pauses and the view comes back to it)
    if key.code == crate::key::Code::Enter {
        crate::follow::your_view(ed);
    }
    match (key.code, id) {
        (crate::key::Code::Enter, _) if folded => ed.workspace.claude.notes.earlier = true,
        (crate::key::Code::Enter, Some(id)) if !open_here => open(ed, id),
        _ => close_card(ed),
    }
    true
}

/// The open card closed (`q`, `esc`, `enter` again, a press on its `¶`).
fn close_card(ed: &mut Editor) {
    let notes = &mut ed.workspace.claude.notes;
    (notes.open, notes.turned, notes.earlier) = (None, false, false);
}

/// A press on the open card's `… N earlier`: its line's earlier answers unfolded.
pub fn unfold_earlier(ed: &mut Editor) {
    ed.workspace.claude.notes.earlier = true;
}

/// A press on note `id`'s line end (`¶`): its card opens, or closes when it's the one open.
pub fn pressed(ed: &mut Editor, id: u64) {
    let notes = &ed.workspace.claude.notes;
    let shown =
        notes.open.and_then(|o| find(ed, o)).is_some_and(|o| group(notes, o).iter().any(|g| g.id == id));
    match shown {
        true => close_card(ed),
        false => {
            ed.workspace.claude.notes.focus = Some(id);
            open(ed, id);
        }
    }
}

/// `space N` — every note, newest first. `C-x` closes the note on its row, `A-x` every note its author began
/// (#197).
pub fn picker(ed: &mut Editor) {
    if ed.workspace.claude.notes.list.is_empty() {
        return ed
            .set_status("no notes yet — Claude pins them while it explores, or space n asks about a line");
    }
    let items = picker_items(ed);
    ed.open_picker(crate::picker::Picker::of("notes", NOTES, items, false), None);
}

/// The notes list's rows, newest first.
pub fn picker_items(ed: &Editor) -> Vec<crate::picker::Item> {
    use crate::picker::{Action, Item};
    ed.workspace
        .claude
        .notes
        .list
        .iter()
        .rev()
        .map(|n| Item {
            label: row_label(ed, n),
            action: Action::Note { id: n.id, path: n.path.clone(), line: n.lines.start },
            // (who began it, and how many answered)
            hint: {
                let by = n.thread.first().map_or(By::Author, |e| e.by).label();
                match n.thread.len() {
                    1 => by.to_string(),
                    2 => format!("{by} · +1 reply"),
                    k => format!("{by} · +{} replies", k - 1),
                }
            },
            glyph: Some(match n.thread.first().map(|e| e.by) {
                Some(By::Reviewer) => ("¶", "type"),
                _ => ("¶", "ui.accent"),
            }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_results::settle_until;

    fn editor() -> Editor {
        Editor::new(crate::config::Config::default())
    }

    /// #251: a note on a path that's no text (`json!` panics on it, a release build aborts) is left out of what's saved
    /// and stays until you quit, said once as it's made — at the save, on quit, no one would read it.
    #[test]
    fn a_note_on_a_path_thats_no_text_lasts_until_you_quit_said_once() {
        use std::os::unix::ffi::OsStrExt as _;
        let bad = Path::new(std::ffi::OsStr::from_bytes(b"/p/caf\xe9.rs"));
        let mut ed = editor();
        add(&mut ed, Path::new("/p/a.rs"), 0..1, By::You, "kept");
        add(&mut ed, bad, 0..1, By::You, "not kept");
        add(&mut ed, bad, 1..2, By::You, "nor this");
        let saved = to_json(&ed.workspace.claude.notes, &[]);
        assert_eq!(saved.iter().map(|n| &n["path"]).collect::<Vec<_>>(), ["/p/a.rs"]);
        assert_eq!(ed.workspace.claude.notes.list.len(), 3, "all there until you quit");
        let said = ed.messages.iter().filter(|m| m.2.contains("its path isn't text")).count();
        assert_eq!(said, 1, "{:?}", ed.messages);
    }

    /// #251's review: what doesn't come through `add` is said too — a reviewer's finding placed from its snapshot, and
    /// a chat note answered into a thread (`to_json` leaves both out).
    #[test]
    fn a_finding_or_an_answered_chat_note_on_such_a_path_is_said() {
        use std::os::unix::ffi::OsStrExt as _;
        let bad = Path::new(std::ffi::OsStr::from_bytes(b"/p/caf\xe9.rs"));
        let said = |ed: &Editor| ed.messages.iter().any(|m| m.2.contains("its path isn't text"));
        let s = crate::scratch::Scratch::new("notes-no-text");
        let root = std::fs::canonicalize(&*s).unwrap();
        std::fs::write(root.join("a.rs"), "one\n").unwrap();
        for args in [&["init", "-q", "-b", "main"][..], &["add", "-A"]] {
            crate::snapread::tests::git_in(&root, args, b"");
        }
        let tree = crate::snapshot::snap(&root).unwrap().id;
        let snap = std::sync::Arc::new(crate::snapread::Snap::list(&root, &tree).unwrap());
        let mut ed = editor();
        pin_finding(&mut ed, bad, 0..1, "a finding", None, Some(("a.rs".into(), snap)));
        assert!(said(&ed), "{:?}", ed.messages);
        let mut ed = editor();
        ed.workspace.root = root.clone();
        ed.config.llm.command = root.join("no-claude-here").display().to_string();
        let id = add_said(&mut ed, bad, 0..1, ("s".into(), "p".into()), Vec::new(), None);
        assert!(!said(&ed), "a chat note's words are the conversation's");
        submit(&mut ed, Some(id), "why?");
        assert!(said(&ed), "{:?}", ed.messages);
    }

    /// frictions30's review (5): a reply Claude never saw is taken back by itself — a reply queued while Claude worked
    /// may have others after it by the time it goes, and the last entry isn't it. (Stand-in: a claude that isn't
    /// there, so the send fails.)
    #[test]
    fn a_reply_that_never_went_takes_back_itself_not_the_last() {
        let s = crate::scratch::Scratch::new("notes-take-back");
        let mut ed = editor();
        ed.workspace.root = s.to_path_buf();
        ed.config.llm.command = s.join("no-claude-here").display().to_string();
        let id = add(&mut ed, &s.join("a.rs"), 0..1, By::You, "first");
        for t in ["queued reply", "a later one"] {
            find_mut(&mut ed, id).unwrap().thread.push(Entry { by: By::You, text: t.into(), draft: false });
        }
        send(&mut ed, id, "queued reply", false, None, Some(true));
        let left: Vec<&str> = find(&ed, id).unwrap().thread.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(left, ["first", "a later one"]);
    }

    /// Each entry says who wrote it — the chat's `note` the author, the reviewer's `finding` the reviewer
    /// (waiting like a draft), `space n` you — and keeps it through the restore file; a file written before
    /// there was a reviewer (`claude`) reads as the author's.
    #[test]
    fn notes_carry_their_author() {
        let scratch = crate::scratch::Scratch::new("notes-by");
        let dir = std::fs::canonicalize(scratch.path()).unwrap();
        let f = dir.join("a.rs");
        std::fs::write(&f, "one\ntwo\nthree\n").unwrap();
        let mut ed = editor();
        ed.workspace.root = dir.clone();
        ed.open(&f).unwrap();
        tool_call(&mut ed, &json!({"path": f, "line": 1, "text": "the author's"})).unwrap();
        let (found, _) =
            finding(&mut ed, &dir, &json!({"path": "a.rs", "line": 2, "text": "the reviewer's"})).unwrap();
        // (`space n` on line 3: with a finding waiting it's a draft too)
        ed.goto_line(2);
        submit(&mut ed, None, "mine");
        let by: Vec<(By, bool)> =
            ed.workspace.claude.notes.list.iter().map(|n| (n.thread[0].by, n.thread[0].draft)).collect();
        assert_eq!(by, [(By::Author, false), (By::Reviewer, true), (By::You, true)]);
        assert_eq!(ed.workspace.claude.notes.list[1].id, found);
        let mut saved = to_json(&ed.workspace.claude.notes, &ed.docs);
        let words: Vec<&str> = saved.iter().map(|n| n["thread"][0][0].as_str().unwrap()).collect();
        assert_eq!(words, ["claude", "reviewer", "you"]);
        saved.push(json!({"path": f, "lines": [0, 1], "thread": [["claude", "from an older tarae"]]}));
        let mut ed = editor();
        restore_from(&mut ed.workspace.claude.notes, &mut ed.docs, saved);
        let by: Vec<(By, bool)> =
            ed.workspace.claude.notes.list.iter().map(|n| (n.thread[0].by, n.thread[0].draft)).collect();
        assert_eq!(by, [(By::Author, false), (By::Reviewer, true), (By::You, true), (By::Author, false)]);
        // The reviewer pins findings in the checkout only, and answers no thread
        let away = crate::scratch::Scratch::new("notes-by-out");
        let outside = away.join("out.rs");
        std::fs::write(&outside, "x\n").unwrap();
        assert!(finding(&mut ed, &dir, &json!({"path": outside, "line": 1, "text": "out"})).is_err());
        assert!(finding(&mut ed, &dir, &json!({"reply_to": 1, "text": "re"})).is_err());
    }

    /// auditfix2 F12: Claude's `note` pins on the project's files only — a file outside it (a stand-in in the
    /// test's own scratch folder) and a name there that isn't get one answer and no note: neither its lines (on
    /// the user's reply) nor whether it exists reach Claude without the Read card. Inside: pinned.
    #[test]
    fn a_note_outside_the_project_is_refused_alike() {
        let (inside, away) =
            (crate::scratch::Scratch::new("notes-in"), crate::scratch::Scratch::new("notes-out"));
        let root = std::fs::canonicalize(inside.path()).unwrap();
        let out = std::fs::canonicalize(away.path()).unwrap();
        std::fs::write(root.join("a.rs"), "one\n").unwrap();
        std::fs::write(out.join("hosts"), "127.0.0.1 stand-in\n").unwrap();
        let mut ed = editor();
        ed.workspace.root = root.clone();
        let pin =
            |ed: &mut Editor, path: &Path| tool_call(ed, &json!({"path": path, "line": 1, "text": "t"}));
        let there = pin(&mut ed, &out.join("hosts")).unwrap_err();
        let none = pin(&mut ed, &out.join("nothing")).unwrap_err();
        assert!(there.contains("outside the project"), "{there}");
        assert_eq!(there, none, "one answer, there or not");
        // (through a link in the project too)
        std::os::unix::fs::symlink(&out, root.join("away")).unwrap();
        assert_eq!(pin(&mut ed, Path::new("away/hosts")).unwrap_err(), there);
        assert!(ed.workspace.claude.notes.list.is_empty());
        pin(&mut ed, Path::new("a.rs")).unwrap();
        assert_eq!(ed.workspace.claude.notes.list.len(), 1);
    }

    fn feed(ed: &mut Editor, keys: &str) {
        let mut chars = keys.chars();
        while let Some(c) = chars.next() {
            let key: crate::key::Key = if c == '<' {
                chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap()
            } else {
                crate::key::Key::plain(crate::key::Code::Char(c))
            };
            ed.handle_key(key);
        }
    }

    /// Events handled for `ms` — a look that nothing comes meanwhile (`quiet_for`).
    fn settle_for(ed: &mut Editor, ms: u64) {
        let end = crate::test_results::quiet_for(std::time::Duration::from_millis(ms));
        while let Some(ev) = ed.events.recv_timeout(end.saturating_duration_since(std::time::Instant::now()))
        {
            ed.handle_event(ev);
        }
    }

    fn gists(ed: &Editor) -> Vec<&str> {
        ed.workspace.claude.notes.list.iter().map(Note::gist).collect()
    }

    /// The front card drawn and in sight past its settling time, then `keys` (a weighty card takes them only
    /// so — the stale card is one since cardburden 6).
    fn answer(ed: &mut Editor, keys: &str) {
        crate::term::render(ed, &mut Vec::new(), 120, 40).unwrap();
        let o = (ed.offers[0].id, ed.offers[0].rev);
        ed.card_focus = Some((o.0, o.1));
        feed(ed, keys);
    }

    /// #197: the author fixed what the reviewer found (asked in the chat, not with `space v s`) — the
    /// findings stayed drafts. Now a turn's end offers to close the drafts whose lines changed since they
    /// were pinned: a card naming them, `y` closes them; `n` keeps them, not asked about again. A draft whose
    /// lines are as they were, and a note that isn't a draft, stay unasked — and a finding in a file not open
    /// counts its lines as the file had them then.
    #[test]
    fn drafts_on_lines_changed_since_are_offered_to_close() {
        let scratch = crate::scratch::Scratch::new("notes-stale");
        let dir = scratch.to_path_buf();
        let (f, g) = (dir.join("a.rs"), dir.join("b.rs"));
        std::fs::write(&f, "one\ntwo\nthree\nfour\n").unwrap();
        std::fs::write(&g, "alpha\nbeta\n").unwrap();
        let mut ed = editor();
        ed.workspace.root = dir.clone();
        ed.open(&f).unwrap();
        tool_call(&mut ed, &json!({"path": f, "line": 1, "text": "the author's"})).unwrap();
        let find = |ed: &mut Editor, path: &str, line: u64, text: &str| {
            finding(ed, &dir, &json!({"path": path, "line": line, "text": text})).unwrap().0
        };
        find(&mut ed, "a.rs", 2, "two is wrong");
        find(&mut ed, "a.rs", 4, "four too");
        // (b.rs isn't open: its lines are read as the finding is pinned)
        let beta = find(&mut ed, "b.rs", 2, "beta");
        settle_until(&mut ed, |ed| {
            ed.workspace.claude.notes.list.iter().any(|n| n.id == beta && n.pinned.is_some())
        });
        // The author fixes line 2 in the buffer, and b.rs's line on disk
        ed.goto_line(1);
        feed(&mut ed, "Afixed<esc>");
        std::fs::write(&g, "alpha\nbeta fixed\n").unwrap();
        turn_done(&mut ed, None);
        settle_until(&mut ed, |ed| !ed.offers.is_empty());
        let o = &ed.offers[0];
        assert!(o.title.contains("2 drafts"), "{}", o.title);
        assert!(o.question.contains("a.rs:2") && o.question.contains("b.rs:2"), "{}", o.question);
        assert!(!o.question.contains("a.rs:4"), "its lines are as they were: {}", o.question);
        answer(&mut ed, "y");
        assert_eq!(gists(&ed), ["the author's", "four too"]);
        // Nothing changed since: nothing asked
        turn_done(&mut ed, None);
        settle_for(&mut ed, 300);
        assert!(ed.offers.is_empty());
        // Line 4 changes: asked; `n` keeps it, and it isn't asked again
        ed.goto_line(3);
        feed(&mut ed, "Afixed<esc>");
        turn_done(&mut ed, None);
        settle_until(&mut ed, |ed| !ed.offers.is_empty());
        answer(&mut ed, "n");
        assert_eq!(gists(&ed), ["the author's", "four too"]);
        turn_done(&mut ed, None);
        settle_for(&mut ed, 300);
        assert!(ed.offers.is_empty(), "asked once");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// cardburden 6: the stale card was pushed anew at every turn's end. Now one card a session: a later
    /// turn's stale drafts join it in place — the same card, its list grown, and a `y` before the new list has
    /// been in sight isn't taken. While it's up each of its drafts says `stale ·` at its line end and on its
    /// card. `r` lists just them (the notes list, no key of the code's changed); `C-x` there closes one and the
    /// card's list follows; `y` closes what's left. (The files are the test's own.)
    #[test]
    fn one_stale_card_grows_in_place_and_lists_its_drafts() {
        let scratch = crate::scratch::Scratch::new("notes-stale-one");
        let dir = scratch.to_path_buf();
        let f = dir.join("a.rs");
        std::fs::write(&f, "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let mut ed = editor();
        ed.open(&f).unwrap();
        for (line, text) in [(2, "two is wrong"), (3, "three too"), (5, "five")] {
            finding(&mut ed, &dir, &json!({"path": "a.rs", "line": line, "text": text})).unwrap();
        }
        // (the author's fix in the turn — made past the card up, which has the keys — then the turn's end)
        let fix = |ed: &mut Editor, line: usize| {
            let up = std::mem::take(&mut ed.offers);
            ed.goto_line(line);
            feed(ed, "Afixed<esc>");
            ed.offers = up;
            turn_done(ed, None);
        };
        fix(&mut ed, 1);
        settle_until(&mut ed, |ed| !ed.offers.is_empty());
        let (card, rev) = (ed.offers[0].id, ed.offers[0].rev);
        assert_eq!(ed.offers[0].title, "Close 1 draft on changed lines?");
        // Its draft says so: at its line end, and on its card (the cursor on its line)
        ed.goto_line(1);
        let rows = |ed: &mut Editor| {
            let mut bytes = Vec::new();
            crate::term::render(ed, &mut bytes, 120, 40).unwrap();
            let mut parser = vt100::Parser::new(40, 120, 0);
            parser.process(&bytes);
            parser.screen().contents()
        };
        let screen = rows(&mut ed);
        assert!(screen.contains("¶ stale · two is wrong"), "{screen}");
        let id = here(&ed).unwrap().id;
        open(&mut ed, id);
        let screen = rows(&mut ed);
        assert!(screen.contains("╭─ review ── stale · the card asks ─"), "its card's top line: {screen}");
        assert!(!screen.contains("stale · three too"), "{screen}");
        // A later turn: the same card, grown — its `y` waits for the new list to be seen
        answer(&mut ed, "");
        fix(&mut ed, 2);
        settle_until(&mut ed, |ed| ed.offers.iter().any(|o| o.title.contains("2 drafts")));
        assert_eq!(ed.offers.len(), 1, "one card");
        assert_eq!(ed.offers[0].id, card, "the same card");
        assert!(ed.offers[0].rev > rev, "to be read again");
        feed(&mut ed, "y");
        assert_eq!(
            gists(&ed),
            ["two is wrong", "three too", "five"],
            "not taken before the new list was seen"
        );
        // `r`: just its drafts, listed; `C-x` closes one — the card's list follows
        answer(&mut ed, "r");
        let p = ed.picker.as_ref().expect("its list");
        assert_eq!(p.title, STALE);
        let listed: Vec<&str> = p.items().iter().map(|i| i.label.as_str()).collect();
        assert_eq!(listed.len(), 2, "{listed:?}");
        assert!(listed.iter().all(|l| !l.contains("five")), "{listed:?}");
        feed(&mut ed, "<C-x>");
        assert_eq!(gists(&ed), ["two is wrong", "five"]);
        assert_eq!(ed.picker.as_ref().map(|p| p.items().len()), Some(1), "the list follows");
        assert_eq!(ed.offers[0].title, "Close 1 draft on changed lines?", "so does the card");
        feed(&mut ed, "<esc>");
        answer(&mut ed, "y");
        assert_eq!(gists(&ed), ["five"]);
        assert!(ed.offers.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The review of 6: a session behind keeps its stale card parked — a later turn's drafts grow that card
    /// there, in place (not a second one, nothing put up in front), and once its drafts are closed it goes from
    /// there too (`stale_sync` behind). (The files are the test's own.)
    #[test]
    fn a_stale_card_behind_grows_and_goes_where_it_is_parked() {
        let scratch = crate::scratch::Scratch::new("notes-stale-behind");
        let dir = scratch.to_path_buf();
        std::fs::create_dir_all(dir.join("back")).unwrap();
        let back_root = dir.join("back");
        std::fs::write(back_root.join("a.rs"), "one\ntwo\nthree\n").unwrap();
        let mut ed = editor();
        ed.workspace.root = dir.clone();
        let back = ed.workspace_add(back_root.clone());
        let mut ids = Vec::new();
        ed.with_workspace(back, |ed| {
            for line in [1, 2] {
                let args = json!({"path": "a.rs", "line": line, "text": format!("found {line}")});
                ids.push(finding(ed, &back_root, &args).unwrap().0);
            }
        });
        let parked = |ed: &Editor| -> Vec<(u64, u64, Vec<u64>)> {
            let l = ed.workspaces.iter().find(|l| l.id == back).expect("behind");
            l.parked
                .iter()
                .filter_map(|o| match &o.what {
                    crate::offer::What::NotesStale { ids } => Some((o.id, o.rev, ids.clone())),
                    _ => None,
                })
                .collect()
        };
        ed.with_workspace(back, |ed| {
            assert!(ed.background());
            stale_card(ed, vec![ids[0]]);
        });
        let first = parked(&ed);
        assert_eq!(first.len(), 1, "parked behind");
        assert!(ed.offers.is_empty(), "nothing in front");
        // A later turn behind: the same card, grown, to be read again
        ed.with_workspace(back, |ed| stale_card(ed, vec![ids[1]]));
        let grown = parked(&ed);
        assert_eq!(grown.len(), 1, "one card");
        assert_eq!(grown[0].0, first[0].0, "the same card");
        assert_eq!(grown[0].2, ids, "its list grown");
        assert!(grown[0].1 > first[0].1);
        assert!(ed.offers.is_empty(), "nothing in front");
        // Its drafts closed, still behind: it goes
        ed.with_workspace(back, |ed| {
            close(ed, ids[0]);
            stale_sync(ed);
        });
        assert_eq!(parked(&ed)[0].2, [ids[1]], "its list follows");
        ed.with_workspace(back, |ed| {
            close(ed, ids[1]);
            stale_sync(ed);
        });
        assert!(parked(&ed).is_empty(), "gone from where it was parked");
        assert!(ed.offers.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// #197: closing notes by who wrote them — in the notes list (`space N`) `C-x` closes the note on its row,
    /// `A-x` every note its author began (the reviewer's findings, all at once); `:notes-clear reviewer`
    /// does the same from the command line, `:notes-clear` closes every note.
    #[test]
    fn notes_close_by_row_or_by_author() {
        let scratch = crate::scratch::Scratch::new("notes-close");
        let dir = std::fs::canonicalize(scratch.path()).unwrap();
        let f = dir.join("a.rs");
        std::fs::write(&f, "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let mut ed = editor();
        ed.workspace.root = dir.clone();
        ed.open(&f).unwrap();
        tool_call(&mut ed, &json!({"path": f, "line": 1, "text": "the author's"})).unwrap();
        finding(&mut ed, &dir, &json!({"path": "a.rs", "line": 2, "text": "found one"})).unwrap();
        finding(&mut ed, &dir, &json!({"path": "a.rs", "line": 3, "text": "found two"})).unwrap();
        ed.goto_line(3);
        submit(&mut ed, None, "mine");
        tool_call(&mut ed, &json!({"path": f, "line": 5, "text": "the author's again"})).unwrap();
        feed(&mut ed, "<space>Nfound two<A-x>");
        assert_eq!(gists(&ed), ["the author's", "mine", "the author's again"], "the reviewer's, all");
        let rows = ed.picker.as_ref().map(|p| (p.items().len(), p.shown().count()));
        assert_eq!(rows, Some((3, 0)), "the list follows (narrowed to `found two`: none left)");
        assert!(ed.toasts.iter().any(|t| t.text.contains("2 notes reviewer began closed")), "said");
        feed(&mut ed, "<esc><space>Nmine<C-x>");
        assert_eq!(gists(&ed), ["the author's", "the author's again"]);
        feed(&mut ed, "<esc>");
        crate::typed::execute(&mut ed, "notes-clear claude").unwrap();
        assert!(ed.workspace.claude.notes.list.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Closing by author when some of those notes hold your unsent drafts (a reply on a finding, a comment of
    /// your own): a card names them first — `n` keeps every one, `y` closes them; from the notes list the
    /// card takes the keys (the list goes). A bare `:notes-clear` asks the same. None of yours among them:
    /// closed at once, as before.
    #[test]
    fn closing_by_author_asks_first_over_your_unsent_drafts() {
        let scratch = crate::scratch::Scratch::new("notes-drafts");
        let dir = std::fs::canonicalize(scratch.path()).unwrap();
        let f = dir.join("a.rs");
        std::fs::write(&f, "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let mut ed = editor();
        ed.workspace.root = dir.clone();
        ed.open(&f).unwrap();
        let one = finding(&mut ed, &dir, &json!({"path": "a.rs", "line": 2, "text": "found one"})).unwrap().0;
        finding(&mut ed, &dir, &json!({"path": "a.rs", "line": 3, "text": "found two"})).unwrap();
        ed.goto_line(3);
        submit(&mut ed, None, "mine");
        tool_call(&mut ed, &json!({"path": f, "line": 5, "text": "the author's"})).unwrap();
        // (your draft reply on a finding, and your own comment a draft — as a review writes them)
        let draft = |text: &str| Entry { by: By::You, text: text.into(), draft: true };
        let list = &mut ed.workspace.claude.notes.list;
        list.iter_mut().find(|n| n.id == one).unwrap().thread.push(draft("my reply"));
        list.iter_mut().find(|n| n.gist() == "mine").unwrap().thread[0].draft = true;
        // The command line: asked, `n` keeps them
        crate::typed::execute(&mut ed, "notes-clear reviewer").unwrap();
        let o = ed.offers.first().expect("the card");
        assert_eq!(o.title, "Close the 2 notes reviewer began?");
        assert!(
            o.question.contains("a.rs:2 · found one") && !o.question.contains("found two"),
            "{}",
            o.question
        );
        feed(&mut ed, "<C-q>n");
        assert_eq!(gists(&ed), ["found one", "found two", "mine", "the author's"], "kept");
        // A bare `:notes-clear` (every note): asked too, naming each that holds a draft; `n` keeps them all
        crate::typed::execute(&mut ed, "notes-clear").unwrap();
        let o = ed.offers.first().expect("the card");
        assert_eq!(o.title, "Close all 4 notes?");
        assert!(
            o.question.contains("a.rs:2 · found one")
                && o.question.contains("a.rs:4 · mine")
                && !o.question.contains("the author's"),
            "{}",
            o.question
        );
        feed(&mut ed, "<C-q>n");
        assert!(ed.offers.is_empty());
        assert_eq!(gists(&ed), ["found one", "found two", "mine", "the author's"], "kept, every one");
        // The notes list's `A-x`: the card has the keys, `y` closes them
        feed(&mut ed, "<space>Nfound two<A-x>");
        assert!(ed.picker.is_none() && !ed.offers.is_empty(), "the card, not the list");
        feed(&mut ed, "y");
        assert_eq!(gists(&ed), ["mine", "the author's"]);
        // Yours: asked too
        crate::typed::execute(&mut ed, "notes-clear you").unwrap();
        assert!(ed.offers.first().is_some_and(|o| o.question.contains("a.rs:4 · mine")));
        feed(&mut ed, "<C-q>y");
        assert_eq!(gists(&ed), ["the author's"]);
        // None of yours: at once, bare too
        crate::typed::execute(&mut ed, "notes-clear").unwrap();
        assert!(ed.offers.is_empty() && ed.workspace.claude.notes.list.is_empty());
        assert!(ed.toasts.iter().any(|t| t.text.contains("1 note closed")), "said");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The notes list's `C-x` closes the note on its row by its id: two notes saying the same on the same
    /// line are two rows, each its own.
    #[test]
    fn a_row_closes_its_own_note_when_two_say_the_same() {
        let scratch = crate::scratch::Scratch::new("notes-same");
        let dir = std::fs::canonicalize(scratch.path()).unwrap();
        let f = dir.join("a.rs");
        std::fs::write(&f, "one\ntwo\n").unwrap();
        let mut ed = editor();
        ed.workspace.root = dir.clone();
        ed.open(&f).unwrap();
        tool_call(&mut ed, &json!({"path": f, "line": 2, "text": "same"})).unwrap();
        tool_call(&mut ed, &json!({"path": f, "line": 2, "text": "same"})).unwrap();
        let ids = |ed: &Editor| ed.workspace.claude.notes.list.iter().map(|n| n.id).collect::<Vec<_>>();
        let (older, newer) = (ids(&ed)[0], ids(&ed)[1]);
        // (newest first: the second row is the older note)
        feed(&mut ed, "<space>N<C-n><C-x>");
        assert_eq!(ids(&ed), [newer], "the row's own note");
        assert!(older != newer);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Saved on quit, back at start — and if the file was edited meanwhile, the note finds its line again;
    /// a note past the end of a shrunk file lands on the last line instead of crashing.
    #[test]
    fn notes_survive_a_restart_and_outside_edits() {
        let scratch = crate::scratch::Scratch::new("notes-save");
        let dir = std::fs::canonicalize(scratch.path()).unwrap();
        let f = dir.join("a.rs");
        std::fs::write(&f, "a\nb\ntarget();\nc\n").unwrap();
        let mut ed = editor();
        ed.workspace.root = dir.clone();
        ed.open(&f).unwrap();
        tool_call(&mut ed, &json!({"path": f, "line": 3, "text": "look here"})).unwrap();
        tool_call(&mut ed, &json!({"reply_to": 1, "text": "and here"})).unwrap();
        let mut saved = to_json(&ed.workspace.claude.notes, &ed.docs);
        assert_eq!(saved[0]["anchor"], "target();");
        // Edited outside tarae: two lines above
        std::fs::write(&f, "x\ny\na\nb\ntarget();\nc\n").unwrap();
        saved.push(
            json!({"path": f, "lines": [50, 52], "anchor": "gone", "thread": [["claude", "past the end"]]}),
        );
        let mut ed = editor();
        restore_from(&mut ed.workspace.claude.notes, &mut ed.docs, saved);
        assert_eq!(ed.workspace.claude.notes.list.len(), 2);
        ed.open(&f).unwrap();
        attach(&mut ed);
        let n = &ed.workspace.claude.notes.list[0];
        assert_eq!((n.lines.clone(), n.thread.len()), (4..5, 2), "re-anchored on its line's text");
        assert_eq!(ed.workspace.claude.notes.list[1].lines, 5..6, "clamped to the last line");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// #214: a note whose file's first buffer is still loading takes the next buffer of that file that isn't — the
    /// buffers by path give the first with it, and the search goes on past one that's turned down.
    #[test]
    fn a_note_passes_over_a_buffer_still_loading() {
        use crate::document::Document;
        let scratch = crate::scratch::Scratch::new("notes-loading");
        let dir = std::fs::canonicalize(scratch.path()).unwrap();
        let (f, other) = (dir.join("a.rs"), dir.join("b.rs"));
        std::fs::write(&f, "one\ntwo\nthree\n").unwrap();
        std::fs::write(&other, "x\n").unwrap();
        let mut docs = crate::docpaths::Docs::of(vec![
            Document::open(1, &other).unwrap(),
            Document::placeholder(2, &f),
            Document::open(3, &other).unwrap(),
            Document::open(4, &f).unwrap(),
        ]);
        let mut notes = Notes::default();
        let saved = vec![json!({"path": f, "lines": [1, 2], "thread": [["claude", "here"]]})];
        restore_from(&mut notes, &mut docs, saved);
        let n = &notes.list[0];
        assert_eq!(n.at.map(|(d, _)| d), Some(4), "the loaded buffer of its file, past the loading one");
        assert_eq!(n.lines, 1..2);
    }
}
