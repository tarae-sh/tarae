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
    /// A chat note's on a place its answer named: what tarae saw of that place in its turn (focus F0). None where its
    /// stops weren't kept (a continue brought it back): nothing said.
    pub check: Option<crate::turnmap::Judged>,
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
        check: None,
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
        check: None,
    });
    notes.next
}

/// What note `n` says tarae saw of its place, `doc` the file's buffer — nothing for a note no answer's check is on.
pub fn check_word(n: &Note, doc: &crate::document::Document) -> Option<&'static str> {
    n.check.as_ref().map(|c| c.word(doc))
}

/// `check_word` where it's a doubt: what its line end says (`Judged::doubt`).
pub fn check_doubt(n: &Note, doc: &crate::document::Document) -> Option<&'static str> {
    n.check.as_ref().and_then(|c| c.doubt(doc))
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
            check: None,
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

/// The lines a new note `space n` writes is about: the selection's; a walk-through's stop's, the cursor on it (never a
/// review's comment) — what `submit` pins and what the card under them says while it's typed.
fn asked_lines(ed: &Editor) -> Range<usize> {
    let doc = ed.doc();
    let r = doc.selection().primary();
    let (a, b) =
        (mv::line_of(&doc.text, r.from()), mv::line_of(&doc.text, r.to().saturating_sub(1).max(r.from())));
    match crate::follow::stop_yours(ed).filter(|_| a == b && !crate::codereview::drafting(ed)) {
        Some((_, lines, _)) => lines,
        None => a..b + 1,
    }
}

/// The lines of `doc` a new note's question is being typed about (focus C1): `space n`'s line open with no note to reply
/// to — none, or one gone since, which `submit` makes a new one too. A buffer with no file has no notes: none.
pub fn asking(ed: &Editor, doc: &crate::document::Document) -> Option<Range<usize>> {
    let new = matches!(ed.prompt.as_ref().map(|p| p.kind), Some(crate::editor::PromptKind::Note(None)));
    (new && doc.path.is_some() && doc.id == ed.doc().id).then(|| asked_lines(ed))
}

/// A reply whose note went while it's typed is a new question: `submit` makes a note on your lines, so the line says that
/// (`ask claude:`, a review's comment) — asked each frame, whichever of the ways a note goes took it.
pub fn prompt_sync(ed: &mut Editor) {
    let Some(crate::editor::PromptKind::Note(Some(id))) = ed.prompt.as_ref().map(|p| p.kind) else { return };
    if find(ed, id).is_some() {
        return;
    }
    let label = comment_label(ed);
    if let Some(p) = ed.prompt.as_mut() {
        (p.kind, p.label) = (crate::editor::PromptKind::Note(None), label);
    }
}

/// `space n`'s word for a new note in a review: a comment, kept as a draft — said so, on its line (#134: "ask claude:"
/// wasn't taken for the review's comment input the rail's `space n comment` promised).
fn comment_label(ed: &Editor) -> Option<String> {
    crate::codereview::drafting(ed).then(|| format!("comment on line {}:", ed.cursor_line() + 1))
}

/// The note `space n`'s line replies to, if there's one.
fn replying_to(ed: &Editor) -> Option<&Note> {
    let Some(crate::editor::PromptKind::Note(Some(id))) = ed.prompt.as_ref().map(|p| p.kind) else {
        return None;
    };
    find(ed, id)
}

/// Whether `space n`'s line is a reply on the card that shows `n` (focus C1): the card holds it.
pub fn replying(ed: &Editor, n: &Note) -> bool {
    replying_to(ed).is_some_and(|r| group(&ed.workspace.claude.notes, n).iter().any(|g| g.id == r.id))
}

/// The note whose card is drawn in `doc`: the one `space n`'s line replies to while it's open (focus C1) — the line
/// stands on its card whether that was open or not, and `esc` leaves `Notes.open` as it was —, else the open one. None
/// while a question's card stands in its place (`asking`): one card at a time.
pub fn carded<'a>(ed: &'a Editor, doc: &crate::document::Document) -> Option<&'a Note> {
    let path = doc.path.as_deref();
    let card = replying_to(ed).filter(|n| path == Some(n.path.as_path())).or_else(|| opened(ed, doc));
    card.filter(|_| asking(ed, doc).is_none())
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

/// The note `enter` acts on: the one here, else the open card's in this file — a card a turn opened leaves your cursor
/// where it was, and the card's keys are its own (`term::draw_note_card` says them only where they are). While follow
/// holds the view: your cursor's, and the open card only where it isn't Claude's place (`opened_yours`). `space n` has
/// no such second place: `here`, else a new note on your line (focus C1, the user on its shots).
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
        check: None,
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

/// `space n`: reply to the note here, or start one (asking Claude about the line or selection) — at your cursor, a card
/// open on another line staying open: to reply there, go to its line or press its words. Follow, holding the view, pauses
/// and the view comes back to your cursor (`follow::your_view`).
pub fn start(ed: &mut Editor) {
    let at = here(ed).map(|n| n.id);
    crate::follow::your_view(ed);
    start_on(ed, at);
}

/// A press on the open card's words: a reply to that card, wherever your cursor is.
pub fn start_card(ed: &mut Editor) {
    let notes = &ed.workspace.claude.notes;
    let at = notes.open.and_then(|o| find(ed, o)).map(|n| shown(notes, n).id);
    // (what you type stands in this card: follow, holding the view, doesn't take it away — nor does your cursor once
    // the line ends: the card you pressed is what's in sight)
    crate::follow::pause_at_card(ed);
    hold_view(ed);
    start_on(ed, at);
}

/// Follow's hold on the view goes with the chat (`chat::close`): where it held the open card's line — the turn's card it
/// brought in sight — the view stays there, the card with its line, until a key of yours moves the cursor or the text
/// (`hold_view`); never back at your cursor with the card's line at the foot and the card below it (focus A1c, `space L`
/// in the 80-column shot). Holding anything else, the view comes back to your cursor as before.
pub fn keep_card_in_sight(ed: &mut Editor) {
    let line = crate::follow::view_line(ed);
    let open = opened(ed, ed.doc()).map(|n| n.lines.start);
    if line.is_some() && line == open {
        hold_view(ed);
    }
}

/// The view stays where it is until a key moves the cursor or the text — the wheel's hold (`Document.wheel_held`).
fn hold_view(ed: &mut Editor) {
    let d = ed.doc_mut();
    d.wheel_held = Some((d.selection().primary(), d.version()));
}

/// The note prompt open on note `target`, none: a new note on the cursor's lines.
fn start_on(ed: &mut Editor, target: Option<u64>) {
    // On a walk-through's stop: it holds still while you ask
    crate::follow::hold(ed);
    let comment = comment_label(ed).filter(|_| target.is_none());
    ed.open_prompt(crate::editor::PromptKind::Note(target), "");
    if let Some(p) = ed.prompt.as_mut().filter(|_| comment.is_some()) {
        p.label = comment;
    }
    // (where the view stands before the line moves it: put back if the line ends with no card to show)
    let d = ed.doc();
    let view = (d.top, d.left, d.top_rows);
    if let Some(p) = ed.prompt.as_mut() {
        p.view = Some(view);
    }
    // (a reply is written on its card — `carded`: in sight with its line, as a card that opens is)
    let path = ed.doc().path.as_deref();
    let lines = target.and_then(|id| find(ed, id)).filter(|n| path == Some(&n.path)).map(|n| n.lines.clone());
    if let Some(lines) = lines {
        ed.show_range(lines);
    }
}

/// How many notes hold a draft — what `space v s` sends.
pub fn drafts(ed: &Editor) -> usize {
    ed.workspace.claude.notes.list.iter().filter(|n| is_draft(n)).count()
}

/// The status line's count of what `space v s` sends (`2 drafts · space v s`) this frame, `rail_w` its rail's width: none
/// where a review's rail counts them, nor where the reviewer's own segment says the same, its findings alone waiting.
pub fn drafts_said(ed: &Editor, rail_w: usize) -> Option<usize> {
    let mine = |n: &Note| n.thread.iter().any(|e| e.draft && e.by == By::You);
    let rail = ed.review_rail_up() && rail_w > 0;
    let findings =
        !ed.workspace.claude.notes.list.iter().any(mine) && crate::reviewer::status_shows(ed, rail_w);
    Some(drafts(ed)).filter(|&k| k > 0 && !rail && !findings)
}

/// A reply on its way to Claude (`chat::Later::Reply`, `reads::Own::Replies`): `text` on note `id` — `fresh`: the note came
/// with it, gone with it if Claude never sees it — and `why`, what the user said at a walk-through's stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub id: u64,
    pub text: String,
    pub fresh: bool,
    pub why: Option<String>,
}

/// The prompt was sent. Whether its card stays: false with nothing to send, and for a review's comment.
pub fn submit(ed: &mut Editor, target: Option<u64>, text: &str) -> bool {
    write(ed, target, text, false)
}

/// `space n`'s line kept for later (its `tab`): a draft, as a review's comment is, whatever `drafting` says till now — then
/// it does, until `space v s` sends them all. Whether its card stays, as `submit`'s.
pub fn keep(ed: &mut Editor, target: Option<u64>, text: &str) -> bool {
    write(ed, target, text, true)
}

/// Whether a reply written now goes behind what's waiting: Claude answering, or replies in line before it — the order they
/// were written. (A card holding some: `send` puts it behind them.)
fn in_line(ed: &Editor) -> bool {
    let queued =
        |c: &crate::chat::Chat| c.sends.iter().any(|l| matches!(l, crate::chat::Later::Reply { .. }));
    ed.workspace.claude.chat.as_ref().is_some_and(|c| c.busy() || queued(c))
}

/// `space n`'s line sent, or (`later`) kept: its thread takes it either way, `submit` and `keep`.
fn write(ed: &mut Editor, target: Option<u64>, text: &str, later: bool) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    // During a review it's a draft: it goes with the rest (`space v s`), nothing is sent now
    let draft = later || crate::codereview::drafting(ed);
    // (Claude answering: the reply is in its thread now and goes at the turn's end — frictions29 18)
    let queues = in_line(ed) && !draft;
    let fresh = target.filter(|&id| find(ed, id).is_some()).is_none();
    // A question at a walk-through's stop is about the stop: its lines, and what Claude said they do
    let stop = (fresh && !draft).then(|| crate::follow::stop_yours(ed)).flatten();
    let id = match target.filter(|&id| find(ed, id).is_some()) {
        Some(id) => {
            let n = find_mut(ed, id).expect("checked");
            n.thread.push(Entry { by: By::You, text: text.to_string(), draft });
            // (a draft leaves what it waits for as it was: a reply sent or in line still has its answer to come)
            n.waiting |= !draft;
            // (a chat note answered is a thread now, the restore file's)
            let path = n.path.clone();
            unkept_said(ed, &path);
            id
        }
        None => {
            let Some(path) = ed.doc().path.clone() else {
                ed.set_status("save the file first — notes live in files");
                return false;
            };
            let lines = asked_lines(ed);
            let id = add(ed, &path, lines, By::You, text);
            if draft && let Some(n) = find_mut(ed, id) {
                n.waiting = false;
                n.thread[0].draft = true;
            }
            id
        }
    };
    if draft {
        let k = drafts(ed);
        // (outside a review too, while drafts wait — a background reviewer's findings among them: bgreview)
        // (in a review the rail counts them; outside one it's said where you look, the user's decision 2026-10-19)
        match ed.workspace.code_review.is_some() {
            true => ed.note(format!("draft saved — {k} in this review · space v s sends them")),
            false => {
                ed.toasts.retain(|t| !t.text.contains("draft saved —"));
                let k = crate::nav::count(k, "draft");
                ed.set_status(format!("draft saved — {k} · space v s sends them"))
            }
        };
        return false;
    }
    // (sent, it stays where you typed it: the card that held the line shows the thread now, the view on it wherever your
    // cursor is — a review's comments go on to the next line, each a draft of its own, above)
    let notes = &mut ed.workspace.claude.notes;
    (notes.open, notes.turned, notes.opening) = (Some(id), false, None);
    // (its lines past a screen, the card stands under their first, not where the line was typed: in sight there —
    // focus A1c, #263)
    let lines = find(ed, id).map(|n| n.lines.clone());
    let size = (ed.viewport.1, ed.viewport.0);
    let under = crate::term::note_rows(ed, ed.doc(), size).map(|r| r.0);
    if let Some(lines) = lines.filter(|l| under.is_some_and(|u| u + 1 < l.end)) {
        ed.show_range(lines);
    }
    hold_view(ed);
    let why = stop.map(|(_, _, why)| why.to_string());
    let reply = Reply { id, text: text.to_string(), fresh, why };
    if queues && let Some(c) = ed.workspace.claude.chat.as_mut() {
        c.sends.push(crate::chat::Later::Reply { reply, lines: None });
        // (nothing holding them any more — a turn cut off left them there: they go now, this one with them)
        crate::chat::send_queued(ed);
        queued_said(ed);
        return true;
    }
    send(ed, vec![reply], None);
    true
}

/// The replies in line, said: how many, what they wait for, and how they'll go — with none left (they went, or their notes
/// closed), what was said of them goes.
fn queued_said(ed: &mut Editor) {
    let asked = crate::reads::replies_ask(ed);
    // (a count that changes is one message that changes, never a stack of them: `say_size`)
    ed.toasts.retain(|t| !t.text.contains("reply queued —") && !t.text.contains("replies queued —"));
    let Some(c) = ed.workspace.claude.chat.as_ref() else { return };
    let k = c.sends.iter().filter(|l| matches!(l, crate::chat::Later::Reply { .. })).count();
    let (queued, go) = match k {
        0 => return,
        1 => ("reply queued".to_string(), "it goes"),
        k => (format!("{k} replies queued"), "they go as one message"),
    };
    let when = match () {
        _ if c.settings.waiting() && !c.running() => format!("claude is starting; {go} once it's up"),
        _ if asked => format!("{go} once the card is answered"),
        _ => format!("{go} when Claude's turn ends"),
    };
    ed.set_status(format!("{queued} — {when}"));
}

/// `replies` back in line, first, as they were: a card's `lines` kept for when they go.
fn back_in_line(ed: &mut Editor, replies: Vec<Reply>, lines: Option<bool>) {
    if let Some(c) = ed.workspace.claude.chat.as_mut() {
        c.sends.splice(0..0, replies.into_iter().map(|reply| crate::chat::Later::Reply { reply, lines }));
    }
    queued_said(ed);
}

/// Every event: a reply whose note went while it waited — closed, whichever of the ways a note goes took it — is taken back:
/// the line counts what will go. A card holding one goes, what it asks no longer so (its `a` would open a key for nothing,
/// and it holds the line): what's left of its batch is first in line again, a card of its own for the keys still named.
pub fn queue_sync(ed: &mut Editor) {
    let notes = &ed.workspace.claude.notes;
    let mut dropped = false;
    if let Some(c) = ed.workspace.claude.chat.as_mut() {
        let there = |l: &crate::chat::Later| match l {
            crate::chat::Later::Reply { reply, .. } => notes.list.iter().any(|n| n.id == reply.id),
            _ => true,
        };
        if !c.sends.iter().all(there) {
            c.sends.retain(there);
            dropped = true;
        }
    }
    let carded = crate::reads::replies_carded(ed).filter(|r| r.iter().any(|r| find(ed, r.id).is_none()));
    let Some(replies) = carded.cloned() else {
        // (the words count what will go: one fewer, or none)
        if dropped {
            queued_said(ed);
        }
        return;
    };
    crate::reads::replies_card_gone(ed);
    let left: Vec<Reply> = replies.into_iter().filter(|r| find(ed, r.id).is_some()).collect();
    if let Some(c) = ed.workspace.claude.chat.as_mut() {
        c.sends.splice(0..0, left.into_iter().map(|reply| crate::chat::Later::Reply { reply, lines: None }));
    }
    crate::chat::send_queued(ed);
    queued_said(ed);
}

/// `k` replies, as a person counts them.
fn replies_count(k: usize) -> String {
    if k == 1 { "1 reply".into() } else { format!("{k} replies") }
}

/// What a message of several replies says of itself, after their notes: which words are whose, and where each is answered
/// (a lone reply has its note's id before it and only one to answer: `send` says nothing).
const REPLIES_SAID: &str = "(my replies follow, each after its note's id: answer each on its note)";

/// `replies` go to Claude as one message — a lone one as it always went, its note's lines and thread and then its words;
/// several, each note once with its lines and thread (`message`) and then the replies in the order written, each by its
/// note's id. One whose note went meanwhile goes no further. On files named like keys Claude may not read, one card asks
/// first (`reads::ask_own`), and its answer sends them again: `lines` Some(true) with the lines, Some(false) with those
/// files' names only (None: not asked yet); while it waits, nothing goes past them (`chat::send_queued`).
pub fn send(ed: &mut Editor, replies: Vec<Reply>, lines: Option<bool>) {
    // The chat panel carries the conversation — opened in the background if closed; the keys stay here
    if ed.workspace.claude.chat.is_none() {
        let at = ed.workspace.viewer.at;
        crate::chat::open(ed);
        ed.workspace.viewer.at = at;
        if let Some(c) = &mut ed.workspace.claude.chat {
            c.unfolded = false;
        }
    }
    let replies: Vec<Reply> = replies.into_iter().filter(|r| find(ed, r.id).is_some()).collect();
    if replies.is_empty() {
        return;
    }
    // (a card up holds the ones before these: behind them — the kept keys' read let more than one through)
    if lines.is_none() && crate::reads::replies_ask(ed) {
        return back_in_line(ed, replies, lines);
    }
    let mut ids: Vec<u64> = Vec::new();
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for r in &replies {
        if !ids.contains(&r.id) {
            ids.push(r.id);
        }
        if let Some(n) = find(ed, r.id).filter(|n| !paths.contains(&n.path)) {
            paths.push(n.path.clone());
        }
    }
    // (the chat just opened: what the project keeps open is read first — no card about one it does)
    if lines.is_none() {
        let own = crate::reads::Own::Replies(replies.clone());
        if crate::reads::wait_for_kept(ed, &paths, own) {
            return;
        }
    }
    let keys: Vec<std::path::PathBuf> =
        paths.iter().filter(|p| crate::reads::key_shut(ed, p, &[])).cloned().collect();
    if lines.is_none() && !keys.is_empty() {
        return crate::reads::ask_own(ed, keys, crate::reads::Own::Replies(replies));
    }
    let mut blocks = Vec::new();
    for id in &ids {
        let Some(n) = find(ed, *id) else { continue };
        let mine: Vec<&Reply> = replies.iter().filter(|r| r.id == *id).collect();
        let sending: Vec<&str> = mine.iter().map(|r| r.text.as_str()).collect();
        let mut about = message(ed, n, !(lines == Some(false) && keys.contains(&n.path)), &sending);
        for why in mine.iter().filter_map(|r| r.why.as_ref()) {
            let at = about.len() - "</note>".len();
            about.insert_str(at, &format!("(a stop of your walk-through — you said: {why})\n"));
        }
        blocks.push(about);
    }
    let (question, about, chip) = match replies.as_slice() {
        [r] => {
            let n = find(ed, r.id).expect("it's there: filtered above");
            let name = n.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            (r.text.clone(), blocks.join("\n"), format!("¶ #{} · {name} L{}", r.id, n.lines.start + 1))
        }
        _ => {
            let words: Vec<String> = replies.iter().map(|r| format!("#{}: {}", r.id, r.text)).collect();
            let chip =
                format!("¶ {} · {}", replies_count(replies.len()), crate::nav::count(paths.len(), "file"));
            (words.join("\n"), format!("{}\n{REPLIES_SAID}", blocks.join("\n")), chip)
        }
    };
    if crate::chat::submit(ed, &question, Some(about), Some(chip)) {
        ed.workspace.claude.notes.turn = ids;
        return;
    }
    // (its claude starting — the project's settings read first: they wait for it, frictions29 #67 — or a turn begun
    // while a card held them: at its end, the card's answer kept, never "couldn't reach claude")
    if ed.workspace.claude.chat.as_ref().is_some_and(|c| c.settings.waiting() || c.busy()) {
        return back_in_line(ed, replies, lines);
    }
    // Claude never saw them — take them back (the chat panel says why)
    for r in replies {
        if r.fresh {
            ed.workspace.claude.notes.list.retain(|n| n.id != r.id);
        } else if let Some(n) = find_mut(ed, r.id) {
            // (this reply — the last of yours with its text, not merely the last: one queued while Claude worked may
            // have others after it by now)
            if let Some(at) = n.thread.iter().rposition(|e| e.by == By::You && e.text == r.text && !e.draft) {
                n.thread.remove(at);
            }
            n.waiting = false;
        }
    }
    ed.set_warning("couldn't reach claude — the chat panel says why");
}

/// What goes to Claude with a note's replies `sending`: where it is, the code there (`code`; else what the context says of
/// a file named like a key — `reads::NOT_SENT`) and the thread so far, what's kept for later left out.
fn message(ed: &Editor, n: &Note, code: bool, sending: &[&str]) -> String {
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
    // The thread but the words being sent and those still in line behind them, the last of yours with each text (Claude
    // may have answered after one queued)
    let line = ed.workspace.claude.chat.iter().flat_map(|c| &c.sends);
    let behind = line.filter_map(|l| match l {
        crate::chat::Later::Reply { reply, .. } if reply.id == n.id => Some(reply.text.as_str()),
        _ => None,
    });
    let mut told: Vec<&Entry> = n.thread.iter().filter(|e| !e.draft).collect();
    for text in sending.iter().copied().chain(behind) {
        if let Some(at) = told.iter().rposition(|e| e.by == By::You && e.text == text) {
            told.remove(at);
        }
    }
    for e in told {
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

/// Which of its own `n` is, and of how many (focus A1c, #260): an answer's place among that answer's, in the order written
/// (`]n`'s after a turn); the reviewer's finding among the review's, any other note among the others, in `]n`'s order
/// (file, line, age). None where it's the only one.
pub fn position(notes: &Notes, n: &Note) -> Option<(usize, usize)> {
    let reviewer = |m: &Note| m.thread.first().is_some_and(|e| e.by == By::Reviewer);
    let kin = |m: &&Note| match (&n.answer, &m.answer) {
        (Some(a), Some(b)) => a.n == b.n,
        (Some(_), None) | (None, Some(_)) => false,
        (None, None) => reviewer(n) == reviewer(m),
    };
    let mut all: Vec<&Note> = notes.list.iter().filter(kin).collect();
    match n.answer.is_some() {
        true => crate::sorting::by_key(&mut all, |m| m.id),
        false => crate::sorting::by_key(&mut all, |m| (m.path.clone(), m.lines.start, m.id)),
    }
    let at = all.iter().position(|m| m.id == n.id)?;
    (all.len() > 1).then_some((at + 1, all.len()))
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

/// A stub of the chat's going to its note (`chatdoc::go`): `id`'s card open, its own answer in sight — an older answer's
/// on a line a newer one names too, the earlier unfolded: it's what the stub stands for.
pub fn open_answer(ed: &mut Editor, id: u64) {
    ed.workspace.claude.notes.focus = Some(id);
    open(ed, id);
    let notes = &ed.workspace.claude.notes;
    let under = find(ed, id).is_some_and(|n| group(notes, n).first().is_some_and(|f| f.id != id));
    ed.workspace.claude.notes.earlier = under;
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
    // (on a note's line — its card cut short there reads on in the chat, focus A1c; anywhere while the open card has
    // earlier answers folded: its key says so — not a card at Claude's place while follow holds the view there)
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
    // (cut short as the last frame drew it: its `…` row's — `term::draw_note_card`)
    let cut = open_here.is_some_and(|(_, o)| ed.screen.note_more.get() == Some(o.id));
    let (id, open_here) = (here.map(|n| n.id), open_here.is_some());
    // (`enter` acts at your cursor: follow, holding the view, pauses and the view comes back to it)
    if key.code == crate::key::Code::Enter {
        crate::follow::your_view(ed);
    }
    match (key.code, id) {
        (crate::key::Code::Enter, _) if folded => ed.workspace.claude.notes.earlier = true,
        (crate::key::Code::Enter, _) if cut => read_on(ed),
        (crate::key::Code::Enter, Some(id)) if !open_here => open(ed, id),
        _ => close_card(ed),
    }
    true
}

/// `enter` on a card cut short, or a press on its `…` row (focus A1c, #259): the chat at that answer's paragraph — the
/// card never grows, the chat is where the whole answer is read. The chat opened if closed (on the conversation it left,
/// its answers' notes with it, this card open again), shown if minimized, the paragraph's stub held unfolded and the
/// chat taken to it once a frame lays it out (`chatdoc::reading`) — the keys where they are, as a stub's link to its
/// card leaves them (`chatdoc::go`), in reverse.
pub fn read_on(ed: &mut Editor) {
    let notes = &ed.workspace.claude.notes;
    let Some(n) = notes.open.and_then(|o| find(ed, o)) else { return };
    let Some(o) = n.answer.as_ref() else {
        return ed.set_status("the chat has no answer of this note's — space n replies here");
    };
    let at = crate::chatdoc::Reading { turn: o.turn, name: o.name.clone(), said: o.said };
    let path = n.path.clone();
    if !crate::chat::reopen_left(ed) {
        return ed.set_status("this answer's conversation isn't kept — the card is all there is of it");
    }
    // (opened again on its conversation, its notes are made anew: this one's card open as it was)
    let notes = &mut ed.workspace.claude.notes;
    if notes.open.is_none() {
        let same = |m: &&Note| {
            m.path == path
                && m.answer
                    .as_ref()
                    .is_some_and(|a| (a.turn, &*a.name, a.said) == (at.turn, &*at.name, at.said))
        };
        notes.open = notes.list.iter().find(same).map(|m| m.id);
    }
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    c.unfolded = true;
    c.reading = Some((at, false));
    ed.unchanged = false;
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
        let reply = Reply { id, text: "queued reply".into(), fresh: false, why: None };
        send(&mut ed, vec![reply], Some(true));
        let left: Vec<&str> = find(&ed, id).unwrap().thread.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(left, ["first", "a later one"]);
    }

    /// C1b: a project's `a.rs` with Claude at work — its claude a stand-in keeping what it's sent, a first message's turn
    /// under way: (the folder, the file, the editor, what it's sent from here on).
    fn at_work(name: &str) -> (crate::scratch::Scratch, PathBuf, Editor, std::sync::mpsc::Receiver<String>) {
        let s = crate::scratch::Scratch::new(name);
        let root = std::fs::canonicalize(&*s).unwrap();
        let text: String = (0..30).map(|i| format!("line {i}\n")).collect();
        std::fs::write(root.join("a.rs"), &text).unwrap();
        let mut ed = Editor::with_root(crate::config::Config::default(), root.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        ed.claude_lines = Some(tx);
        let path = root.join("a.rs");
        ed.open(&path).unwrap();
        crate::chat::open(&mut ed);
        assert!(crate::chat::submit(&mut ed, "first", None, None));
        assert_eq!(rx.try_iter().count(), 1, "the first message");
        (s, path, ed, rx)
    }

    /// Claude's turn ends: what's in line goes.
    fn turn_ends(ed: &mut Editor) {
        ed.workspace.claude.chat.as_mut().unwrap().state = crate::chat::State::Idle;
        crate::chat::send_queued(ed);
    }

    /// The text of each message the stand-in got since.
    fn sent(rx: &std::sync::mpsc::Receiver<String>) -> Vec<String> {
        let text = |l: String| {
            let v: Value = serde_json::from_str(&l).expect("a line of json");
            let content = &v["message"]["content"];
            content.as_str().map_or_else(|| content.to_string(), str::to_string)
        };
        rx.try_iter().map(text).collect()
    }

    /// What's in the chat's line.
    fn line(ed: &Editor) -> Vec<&crate::chat::Later> {
        ed.workspace.claude.chat.as_ref().unwrap().sends.iter().collect()
    }

    fn status(ed: &Editor) -> String {
        ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default()
    }

    /// C1b: three replies written while Claude works — two on notes with their threads, a question that's a note of its
    /// own — go as one message at its turn's end: each note once with its lines and the thread so far, then the replies in
    /// the order written, each by its note's id, saying so. The words say how many, the chat's row too; the turn's notes are
    /// all three, waiting.
    #[test]
    fn replies_written_while_claude_works_go_as_one_message() {
        let (_s, path, mut ed, rx) = at_work("c1b-one");
        let a = add(&mut ed, &path, 1..2, By::Author, "this one looks odd");
        let b = add(&mut ed, &path, 4..6, By::Author, "and these lines");
        assert!(submit(&mut ed, Some(a), "why odd?"));
        assert_eq!(status(&ed), "reply queued — it goes when Claude's turn ends");
        assert!(submit(&mut ed, Some(b), "is it safe?"));
        assert_eq!(status(&ed), "2 replies queued — they go as one message when Claude's turn ends");
        ed.goto_line(9);
        assert!(submit(&mut ed, None, "and this line?"));
        assert_eq!(status(&ed), "3 replies queued — they go as one message when Claude's turn ends");
        assert_eq!(ed.toasts.iter().filter(|t| t.text.contains(" queued —")).count(), 1, "one that changes");
        let c = ed.workspace.claude.notes.list.last().unwrap().id;
        let said = ed.workspace.claude.chat.as_ref().unwrap().queued_said().map(|q| q.0);
        assert_eq!(said.as_deref(), Some("3 replies queued"));
        assert!(sent(&rx).is_empty(), "nothing while it works");
        turn_ends(&mut ed);
        let got = sent(&rx);
        assert_eq!(got.len(), 1, "one message: {got:#?}");
        let m = &got[0];
        let at = |s: &str| m.find(s).unwrap_or_else(|| panic!("{s} in {m}"));
        assert!(
            at(&format!("<note id=\"{a}\" path=\"a.rs\" lines=\"2-2\">")) < at(&format!("<note id=\"{b}\""))
        );
        assert!(
            at(&format!("<note id=\"{b}\" path=\"a.rs\" lines=\"5-6\">")) < at(&format!("<note id=\"{c}\""))
        );
        assert!(
            m.contains("<code>\nline 1\n</code>") && m.contains("<code>\nline 4\nline 5\n</code>"),
            "{m}"
        );
        assert!(m.contains("claude: this one looks odd\n</note>"), "the thread so far, not the reply: {m}");
        assert!(m.contains(REPLIES_SAID), "{m}");
        let words = format!("#{a}: why odd?\n#{b}: is it safe?\n#{c}: and this line?");
        assert!(m.ends_with(&words), "in the order written: {m}");
        assert_eq!(m.matches("why odd?").count(), 1, "said once: {m}");
        assert!(line(&ed).is_empty());
        assert_eq!(ed.workspace.claude.notes.turn, [a, b, c]);
        assert!([a, b, c].iter().all(|&id| find(&ed, id).unwrap().waiting));
        let row = ed.workspace.claude.chat.as_ref().unwrap().msgs.last().unwrap();
        assert_eq!((row.text.as_str(), row.chip.as_str()), (words.as_str(), "¶ 3 replies · 1 file"));
    }

    /// C1b, the review: a note closed takes its reply out of line and the words count what's left; the last one closed, what
    /// was said of them goes, and the turn's end sends nothing.
    #[test]
    fn a_note_closed_counts_what_is_left_in_line() {
        let (_s, path, mut ed, rx) = at_work("c1b-closed-count");
        let tick = |ed: &mut Editor| ed.handle_event(crate::event::Event::Job(Box::new(|_: &mut Editor| {})));
        let queued = |ed: &Editor| ed.toasts.iter().filter(|t| t.text.contains(" queued —")).count();
        let a = add(&mut ed, &path, 1..2, By::Author, "this one looks odd");
        let b = add(&mut ed, &path, 4..6, By::Author, "and these lines");
        assert!(submit(&mut ed, Some(a), "why odd?"));
        assert!(submit(&mut ed, Some(b), "is it safe?"));
        assert_eq!(status(&ed), "2 replies queued — they go as one message when Claude's turn ends");
        close(&mut ed, a);
        tick(&mut ed);
        assert_eq!(status(&ed), "reply queued — it goes when Claude's turn ends");
        assert_eq!(queued(&ed), 1, "one that changes");
        close(&mut ed, b);
        tick(&mut ed);
        assert!(line(&ed).is_empty());
        assert_eq!(queued(&ed), 0, "nothing in line, nothing said of it: {:?}", ed.toasts);
        turn_ends(&mut ed);
        assert!(sent(&rx).is_empty(), "{:?}", sent(&rx));
    }

    /// C1b: a reply alone in line goes as a lone reply always went, to the byte: its note, a blank line, its words, its chip —
    /// and Claude's plain answer is that note's (`turn_done`: one note).
    #[test]
    fn a_lone_reply_goes_as_it_always_did() {
        let (_s, path, mut ed, rx) = at_work("c1b-lone");
        let a = add(&mut ed, &path, 1..2, By::Author, "this one looks odd");
        submit(&mut ed, Some(a), "why odd?");
        turn_ends(&mut ed);
        let got = sent(&rx);
        let about = format!(
            "<note id=\"{a}\" path=\"a.rs\" lines=\"2-2\">\n<code>\nline 1\n</code>\nclaude: this one looks odd\n</note>\n\nwhy odd?"
        );
        assert!(got.len() == 1 && got[0].ends_with(&about), "{got:#?}");
        assert!(!got[0].contains(REPLIES_SAID), "{got:#?}");
        assert_eq!(ed.workspace.claude.notes.turn, [a]);
        let row = ed.workspace.claude.chat.as_ref().unwrap().msgs.last().unwrap();
        assert_eq!(row.chip, format!("¶ #{a} · a.rs L2"));
    }

    /// C1b: two replies on one note: it's one block and both words go, in order, its thread holding neither; a reply kept
    /// for later on it stays out. An answer that came after the first was queued stays in the thread.
    #[test]
    fn replies_on_one_note_are_one_block() {
        let (_s, path, mut ed, rx) = at_work("c1b-same");
        let (a, b) =
            (add(&mut ed, &path, 1..2, By::Author, "one"), add(&mut ed, &path, 3..4, By::Author, "two"));
        submit(&mut ed, Some(a), "first word");
        find_mut(&mut ed, a).unwrap().thread.push(Entry {
            by: By::Author,
            text: "meanwhile".into(),
            draft: false,
        });
        submit(&mut ed, Some(b), "on the other");
        submit(&mut ed, Some(a), "second word");
        find_mut(&mut ed, a).unwrap().thread.push(Entry { by: By::You, text: "not yet".into(), draft: true });
        turn_ends(&mut ed);
        let got = sent(&rx);
        assert_eq!(got.len(), 1, "{got:#?}");
        let m = &got[0];
        assert_eq!(m.matches(&format!("<note id=\"{a}\"")).count(), 1, "{m}");
        assert!(m.contains("claude: one\nclaude: meanwhile\n</note>"), "{m}");
        assert!(m.ends_with(&format!("#{a}: first word\n#{b}: on the other\n#{a}: second word")), "{m}");
        assert!(!m.contains("not yet"), "a draft stays yours: {m}");
        assert_eq!(ed.workspace.claude.notes.turn, [a, b]);
    }

    /// C1b: what stands between keeps its place: replies join only what's next to them, so a review, then a parent's
    /// message, go in the order they were put in line, one a turn, and no reply goes past either.
    #[test]
    fn a_review_and_a_parents_message_keep_their_places() {
        use crate::chat::Later;
        let (_s, path, mut ed, rx) = at_work("c1b-places");
        let ids: Vec<u64> =
            [0, 2, 4, 6].iter().map(|&l| add(&mut ed, &path, l..l + 1, By::Author, "note")).collect();
        let draft = add(&mut ed, &path, 8..9, By::You, "my comment");
        let d = find_mut(&mut ed, draft).unwrap();
        (d.thread[0].draft, d.waiting) = (true, false);
        // (the draft above makes every reply a draft: these are put in line as they were before it)
        let reply = |ed: &mut Editor, id: u64, text: &str| {
            find_mut(ed, id).unwrap().thread.push(Entry { by: By::You, text: text.into(), draft: false });
            let reply = Reply { id, text: text.into(), fresh: false, why: None };
            ed.workspace.claude.chat.as_mut().unwrap().sends.push(Later::Reply { reply, lines: None });
        };
        reply(&mut ed, ids[0], "one");
        reply(&mut ed, ids[1], "two");
        let c = ed.workspace.claude.chat.as_mut().unwrap();
        c.sends.push(Later::Review(None));
        c.sends.push(Later::Parent {
            from: "main".into(),
            conversation: None,
            wake: false,
            text: "the parent".into(),
        });
        reply(&mut ed, ids[2], "three");
        reply(&mut ed, ids[3], "four");
        let mut went = Vec::new();
        for _ in 0..4 {
            turn_ends(&mut ed);
            let got = sent(&rx);
            assert_eq!(got.len(), 1, "one a turn: {got:#?}");
            went.push(got[0].clone());
        }
        assert!(went[0].ends_with(&format!("#{}: one\n#{}: two", ids[0], ids[1])), "{went:#?}");
        assert!(went[1].contains("<review of="), "{went:#?}");
        assert!(went[2].contains("the parent"), "{went:#?}");
        assert!(went[3].ends_with(&format!("#{}: three\n#{}: four", ids[2], ids[3])), "{went:#?}");
        turn_ends(&mut ed);
        assert!(sent(&rx).is_empty() && line(&ed).is_empty());
    }

    /// C1b: a user's queued message keeps a parent's message behind it, as ever — replies go on past a parent's there, as
    /// `first` lets them, and join across it.
    #[test]
    fn with_a_message_queued_replies_join_past_a_parents() {
        use crate::chat::Later;
        let (_s, path, mut ed, rx) = at_work("c1b-parent");
        let (a, b) = (add(&mut ed, &path, 0..1, By::Author, "a"), add(&mut ed, &path, 2..3, By::Author, "b"));
        submit(&mut ed, Some(a), "one");
        let c = ed.workspace.claude.chat.as_mut().unwrap();
        c.sends.push(Later::Parent {
            from: "main".into(),
            conversation: None,
            wake: false,
            text: "the parent".into(),
        });
        submit(&mut ed, Some(b), "two");
        ed.workspace.claude.chat.as_mut().unwrap().queued = Some("yours".into());
        turn_ends(&mut ed);
        let got = sent(&rx);
        assert!(got.len() == 1 && got[0].ends_with(&format!("#{a}: one\n#{b}: two")), "{got:#?}");
        turn_ends(&mut ed);
        let got = sent(&rx);
        assert!(got.len() == 1 && got[0].contains("yours"), "the person first: {got:#?}");
        turn_ends(&mut ed);
        let got = sent(&rx);
        assert!(got.len() == 1 && got[0].contains("the parent"), "{got:#?}");
    }

    /// C1b: a note closed while its reply is queued takes the reply out of line at once — the count and the chat's row say
    /// what will go —, the rest go without it; the last one left goes as a lone reply; none left, the next in line goes on.
    #[test]
    fn a_reply_taken_back_while_queued_leaves_the_line() {
        use crate::chat::Later;
        let (_s, path, mut ed, rx) = at_work("c1b-back");
        let ids: Vec<u64> =
            [0, 2, 4].iter().map(|&l| add(&mut ed, &path, l..l + 1, By::Author, "n")).collect();
        for (id, text) in ids.iter().zip(["one", "two", "three"]) {
            submit(&mut ed, Some(*id), text);
        }
        close(&mut ed, ids[1]);
        queue_sync(&mut ed);
        let said = ed.workspace.claude.chat.as_ref().unwrap().queued_said().map(|q| q.0);
        assert_eq!(said.as_deref(), Some("2 replies queued"));
        close(&mut ed, ids[2]);
        queue_sync(&mut ed);
        assert!(matches!(line(&ed)[..], [Later::Reply { .. }]), "{:?}", line(&ed));
        turn_ends(&mut ed);
        let got = sent(&rx);
        assert!(got.len() == 1 && got[0].ends_with("</note>\n\none"), "a lone reply: {got:#?}");
        // (one gone in the same moment the turn ends, before any event: `send` leaves it out, the next goes on)
        let (a, b) = (add(&mut ed, &path, 6..7, By::Author, "a"), add(&mut ed, &path, 8..9, By::Author, "b"));
        submit(&mut ed, Some(a), "gone");
        submit(&mut ed, Some(b), "gone too");
        ed.workspace.claude.chat.as_mut().unwrap().queued = Some("the message behind".into());
        ed.workspace.claude.notes.list.retain(|n| n.id != a && n.id != b);
        turn_ends(&mut ed);
        let got = sent(&rx);
        assert!(got.len() == 1 && got[0].contains("the message behind"), "{got:#?}");
    }

    /// C1b: a reply left in line by a turn cut off (no turn's end to send it) goes with the next one written, as one message,
    /// never sitting behind it waiting for a turn that isn't under way.
    #[test]
    fn what_a_cut_turn_left_in_line_goes_with_the_next_reply() {
        let (_s, path, mut ed, rx) = at_work("c1b-cut");
        let (a, b) = (add(&mut ed, &path, 0..1, By::Author, "a"), add(&mut ed, &path, 2..3, By::Author, "b"));
        submit(&mut ed, Some(a), "left in line");
        ed.workspace.claude.chat.as_mut().unwrap().state = crate::chat::State::Idle;
        submit(&mut ed, Some(b), "the next");
        let got = sent(&rx);
        assert!(
            got.len() == 1 && got[0].ends_with(&format!("#{a}: left in line\n#{b}: the next")),
            "{got:#?}"
        );
        assert!(line(&ed).is_empty());
    }

    /// C1b: what each reply carries goes with it: `why` in its own note only, and a batch Claude never saw is taken back
    /// reply by reply — a `fresh` one's note gone, the others' words alone —, said once.
    #[test]
    fn each_reply_keeps_what_it_carries() {
        let (s, path, mut ed, rx) = at_work("c1b-carries");
        let (a, b) = (add(&mut ed, &path, 0..1, By::You, "q"), add(&mut ed, &path, 2..3, By::Author, "b"));
        find_mut(&mut ed, b).unwrap().thread.push(Entry { by: By::You, text: "two".into(), draft: false });
        let c = ed.workspace.claude.chat.as_mut().unwrap();
        let at_stop = Reply { id: a, text: "q".into(), fresh: true, why: Some("it ranks them".into()) };
        let other = Reply { id: b, text: "two".into(), fresh: false, why: None };
        for reply in [at_stop.clone(), other.clone()] {
            c.sends.push(crate::chat::Later::Reply { reply, lines: None });
        }
        turn_ends(&mut ed);
        let m = sent(&rx).remove(0);
        let said = "(a stop of your walk-through — you said: it ranks them)\n</note>";
        assert_eq!(m.matches(said).count(), 1, "{m}");
        assert!(m.find(said) < m.find(&format!("<note id=\"{b}\"")), "in its own note: {m}");
        // No claude to reach, nothing starting
        let mut ed = editor();
        ed.workspace.root = s.to_path_buf();
        ed.config.llm.command = s.join("no-claude-here").display().to_string();
        let a = add(&mut ed, &path, 0..1, By::You, "q");
        let b = add(&mut ed, &path, 2..3, By::Author, "b");
        find_mut(&mut ed, b).unwrap().thread.push(Entry { by: By::You, text: "two".into(), draft: false });
        send(&mut ed, vec![Reply { id: a, ..at_stop }, Reply { id: b, ..other }], Some(true));
        assert!(find(&ed, a).is_none(), "its note went with it");
        let n = find(&ed, b).unwrap();
        assert_eq!(n.thread.len(), 1, "the reply off its thread: {:?}", n.thread);
        assert!(!n.waiting);
        assert_eq!(ed.toasts.iter().filter(|t| t.text.contains("couldn't reach claude")).count(), 1);
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
        assert!(
            screen.contains("╭─ review · 1/3 ── stale · the card asks ─"),
            "its card's top line: {screen}"
        );
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
