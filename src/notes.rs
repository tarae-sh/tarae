//! Notes — drafts at lines, waiting to be sent (#388, chatlink step 3): a code review's comments (yours) and the
//! reviewer's findings (`reviewer.rs`, `finding`). Every other word about the code is the chat's: `space n` outside a
//! review is the ask line with the lines quoted (`ask_about`), their flag the answer's (`flags.rs`); the chat's Claude
//! has no `note` tool.
//!
//! - A note sits on a document mark (it follows edits); a file that isn't open keeps the line numbers and
//!   gets its mark when it opens (`attach`, every event). The line end shows `¶ first words`, its card the words whole
//!   (`term::note_lines`).
//! - `space n` in a review (`codereview::drafting`): a comment on this line, a draft. `space v s` sends them all in one
//!   message (`codereview::send`), and a sent one leaves the code: its lines are a flag of the answer (Q9).
//!   `]n`/`[n` walk notes and flags, `space N` lists them, `:note-close` drops the one here.
//! - Kept per session, under its checkout, in the restore file (`restore.rs`, written on quit — `to_json`, read
//!   as the session is made — `restore_in`). A note remembers its first line's text: if the file changed while
//!   tarae was closed, it re-anchors on the nearest line with that text when the file opens.

use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::document::DocId;
use crate::editor::Editor;
use crate::movement as mv;
use crate::picker::Item;
use crate::selection::{Range as SelRange, Selection};

/// Who wrote a draft: you (a review's comment) · the reviewer (a finding).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    You,
    /// The reviewer (`reviewer.rs`) — a second Claude, apart from the chat's.
    Reviewer,
}

impl By {
    /// How it's shown: on the note's card, in the notes list and the review rail.
    pub fn label(self) -> &'static str {
        self.word()
    }

    /// How the review's message names its author for Claude, and in the restore file.
    pub fn word(self) -> &'static str {
        match self {
            By::You => "you",
            By::Reviewer => "reviewer",
        }
    }

    fn from_word(w: &str) -> Option<By> {
        match w {
            "you" => Some(By::You),
            "reviewer" => Some(By::Reviewer),
            _ => None,
        }
    }
}

/// A draft at lines — a review's comment or a finding of the reviewer's — until `space v s` sends it with the rest of the
/// review (`codereview::send`), when it leaves the code (#388 step 3: no thread, no reply).
#[derive(Clone, Debug)]
pub struct Note {
    pub id: u64,
    pub path: PathBuf,
    /// The mark in the open document (follows edits).
    pub at: Option<(DocId, u64)>,
    /// 0-based lines, as of the last event.
    pub lines: Range<usize>,
    /// Who wrote it, and what.
    pub by: By,
    pub text: String,
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

impl Note {
    /// Its first line said — what the line end shows.
    pub fn gist(&self) -> &str {
        self.text.lines().find(|l| !l.trim().is_empty()).unwrap_or("")
    }
}

#[derive(Default)]
pub struct Notes {
    pub list: Vec<Note>,
    next: u64,
    /// The note shown when several share a line — the one just pinned, or the one `]n`/`[n` stepped to.
    pub focus: Option<u64>,
    /// The note whose card is open between the lines, under its last one, in every pane that shows its file (focus
    /// A1b): opened by an act only (`]n`/`[n`, `enter` on its line, a press on its `¶`), never by the cursor passing;
    /// open until closed (`q`, `esc`, `enter` again) or another opens — leaving the file keeps it.
    pub open: Option<u64>,
    /// Drafts whose lines changed since they were pinned (`stale_check`, `stale_mark`): each says `stale` on itself —
    /// its line end, its card (`R` re-places it there), the review rail's count — and the notes list's top line counts
    /// them, `C-l` there closing them all (`close_stale`). No card (polish landing 4: 266 of its 294 answers were `y`).
    stale: Vec<u64>,
    /// Findings placed on lines changed since their reviewer read them (`pin_finding`) — what made them stale.
    since_read: Vec<u64>,
    /// A note on a path that's no text was said not to last (`unkept_said`) — once.
    unkept_said: bool,
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

/// A new draft on `lines` of `path`.
pub fn add(ed: &mut Editor, path: &Path, lines: Range<usize>, by: By, text: &str) -> u64 {
    unkept_said(ed, path);
    ed.workspace.claude.notes.next += 1;
    let id = ed.workspace.claude.notes.next;
    ed.workspace.claude.notes.list.push(Note {
        id,
        path: path.to_path_buf(),
        at: None,
        lines: lines.clone(),
        by,
        text: text.trim().to_string(),
        anchor: None,
        pinned: None,
        board: None,
        placing: false,
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
        let made = follow_mark((&mut n.at, &mut n.lines, &mut n.anchor), &n.path, docs);
        // (its lines as pinned, when not known yet — a note restored from an earlier build)
        if let Some(i) = made.filter(|_| n.pinned.is_none()) {
            n.pinned = lines_digest(&docs[i].text, &n.lines);
        }
    }
}

/// One mark's lines followed — a note's or a flag's (`flags::attach`): attached, its lines are its mark's; not yet, its
/// file's buffer once open gets one (moved first to its first line's text as kept, `anchor`, if the file moved it). The
/// buffer's index where it was just made.
pub(crate) fn follow_mark(
    (at, lines, anchor): (&mut Option<(DocId, u64)>, &mut Range<usize>, &mut Option<String>),
    path: &Path,
    docs: &mut crate::docpaths::Docs,
) -> Option<usize> {
    // (its mark as stored, its ends snapped as `Document::mark` does — never a selection made for it each event:
    // #214's third piece, an allocation a note)
    match at.and_then(|(d, m)| {
        let doc = &docs[docs.index_of(d)?];
        Some((doc, doc.mark_raw(m)?.primary()))
    }) {
        Some((doc, r)) => {
            let line = |p| mv::line_of(&doc.text, crate::graphemes::snap(&doc.text, p));
            let (a, b) = (line(r.anchor), line(r.head));
            *lines = a.min(b)..a.max(b) + 1;
            None
        }
        None => {
            *at = None;
            let i = docs.first(path, |d| !d.loading)?;
            let doc = &mut docs[i];
            // Saved last time: follow its first line if the file moved it meanwhile
            if let Some(want) = anchor.take() {
                let shift = nearest_line(&doc.text, lines.start, &want);
                *lines = lines.start.saturating_add_signed(shift)..lines.end.saturating_add_signed(shift);
            }
            // (the file may have shrunk: both ends within it, the first not after the last)
            let last = mv::last_line(&doc.text);
            let a = lines.start.min(last);
            let b = lines.end.saturating_sub(1).max(a).min(last);
            let sel =
                Selection::single(SelRange::new(mv::line_start(&doc.text, a), mv::line_start(&doc.text, b)));
            *at = Some((doc.id, doc.add_mark(sel)));
            *lines = a..b + 1;
            Some(i)
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

/// A session's notes as saved in the restore file: path, lines, the first line's text, who wrote it and what. One on a path that's
/// no text is left out (`json!` panics on it, and a lossy path could be another file): it lasts until you quit, said the
/// first time (`unkept_said`: as a note is made, or a chat note answered into a thread).
pub fn to_json(notes: &Notes, docs: &[crate::document::Document]) -> Vec<Value> {
    notes
        .list
        .iter()
        // (a finding still being placed has the snapshot's lines and a copy that won't outlive a restart: not
        // kept on the wrong lines — its placing lands within the moment)
        .filter(|n| !n.placing && n.path.to_str().is_some())
        .map(|n| {
            let anchor = n.anchor.clone().or_else(|| {
                let doc = docs.iter().find(|d| d.path.as_deref() == Some(n.path.as_path()))?;
                (n.lines.start < doc.text.len_lines())
                    .then(|| doc.text.line(n.lines.start).to_string().trim().to_string())
            });
            let pinned = n.pinned.as_ref().map(hex);
            json!({
                "path": n.path, "lines": [n.lines.start, n.lines.end], "anchor": anchor, "by": n.by.word(),
                "text": n.text,
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
        let (Some(by), Some(text)) =
            (By::from_word(v["by"].as_str().unwrap_or_default()), v["text"].as_str())
        else {
            continue;
        };
        if !Path::new(path).is_file() {
            continue;
        }
        notes.next += 1;
        notes.list.push(Note {
            id: notes.next,
            path: PathBuf::from(path),
            at: None,
            lines: a as usize..(b as usize).max(a as usize + 1),
            by,
            text: text.to_string(),
            anchor: v["anchor"].as_str().map(str::to_string),
            pinned: v["pinned"].as_str().and_then(of_hex),
            board: Mark::of(&v["board"]),
            placing: false,
            since: v["since"].as_str().map(str::to_string),
        });
    }
    attach_in(notes, docs);
}

/// Marks notes and flags point at (the jump list's pruning keeps them).
pub fn marks(ed: &Editor) -> Vec<(DocId, u64)> {
    let mut v: Vec<(DocId, u64)> = ed.workspace.claude.notes.list.iter().filter_map(|n| n.at).collect();
    v.extend(crate::flags::marks(ed));
    v
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

/// The lines `space n` is about: the selection's; a walk-through's stop's, the cursor on it (never a review's comment) —
/// what the ask line quotes (`ask_about`), what a comment pins (`submit`) and what its card under them says while typed.
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

/// The lines of `doc` a review's comment is being typed about (focus C1): `space n`'s line open in a review. A buffer with
/// no file has no notes: none.
pub fn asking(ed: &Editor, doc: &crate::document::Document) -> Option<Range<usize>> {
    let new = matches!(ed.prompt.as_ref().map(|p| p.kind), Some(crate::editor::PromptKind::Note));
    (new && doc.path.is_some() && doc.id == ed.doc().id).then(|| asked_lines(ed))
}

/// `space n`'s word in a review: a comment, kept as a draft — said so, on its line (#134).
fn comment_label(ed: &Editor) -> String {
    format!("comment on line {}:", ed.cursor_line() + 1)
}

/// The note whose card is drawn in `doc`: the open one — none while a comment's card stands in its place (`asking`): one
/// card at a time.
pub fn carded<'a>(ed: &'a Editor, doc: &crate::document::Document) -> Option<&'a Note> {
    opened(ed, doc).filter(|_| asking(ed, doc).is_none())
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

/// Of the notes over `line`: the one its line end shows — starting there, the focused one, else the first — so a key on
/// a line takes its `¶`, never a range running through it; none starting there, the focused one, else the first. As the
/// card shows it.
fn first_of<'a>(ed: &'a Editor, v: &[&'a Note], line: usize) -> Option<&'a Note> {
    let notes = &ed.workspace.claude.notes;
    let focused = |n: &&&'a Note| Some(n.id) == notes.focus;
    let starts = || v.iter().filter(|n| n.lines.start == line);
    (starts().find(focused).or_else(|| starts().next()))
        .or_else(|| v.iter().find(focused))
        .or(v.first())
        .copied()
}

/// The note `enter` acts on: the one here, else the open card's in this file — a card a turn opened leaves your cursor
/// where it was, and the card's keys are its own (`term::draw_note_card` says them only where they are). While follow
/// holds the view: your cursor's, and the open card only where it isn't Claude's place (`opened_yours`). `space n` has
/// no such second place: `here`, else a new note on your line (focus C1, the user on its shots).
pub fn target(ed: &Editor) -> Option<&Note> {
    here(ed).or_else(|| opened_yours(ed))
}

// ── The reviewer's findings ────────────────────────────────────────────────────────────────

/// Where a new note goes: `path` (relative to `root`) and its lines, 0-based. In `root` only, by its real path,
/// asked before anything about the file: a path outside gets one answer whether or not it's there — a finding's
/// lines go to the author once sent, and only the Read card lets Claude see outside (auditfix2 F12).
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
        by: By::Reviewer,
        text: text.trim().to_string(),
        anchor: None,
        pinned: None,
        board: mark,
        placing: true,
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
/// lines changed since they were pinned (fixed, maybe, by the author asked in the chat — #197) are marked stale
/// (`stale_mark`). Compared on a worker: an open file's text as it is (its notes follow their marks), a file not
/// open read from disk.
pub(crate) fn stale_check(ed: &mut Editor) {
    type Wanted = (u64, PathBuf, Range<usize>, [u8; 32], Option<(ropey::Rope, bool)>);
    let (notes, docs) = (&ed.workspace.claude.notes, &ed.docs);
    let wanted: Vec<Wanted> = notes
        .list
        .iter()
        .filter(|n| !notes.stale.contains(&n.id))
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
        move |ed: &mut Editor| stale_mark(ed, stale)
    });
}

/// The drafts a card names at most.
const STALE_SHOWN: usize = 6;

/// Drafts `ids` whose lines changed since they were pinned are stale — said on each, never a card or a toast: the
/// notes say it where they're drawn and the notes list counts them (polish landing 4).
pub(crate) fn stale_mark(ed: &mut Editor, ids: Vec<u64>) {
    let notes = &ed.workspace.claude.notes;
    let new: Vec<u64> = notes
        .list
        .iter()
        .filter(|n| ids.contains(&n.id) && !notes.stale.contains(&n.id))
        .map(|n| n.id)
        .collect();
    if new.is_empty() {
        ed.unchanged = true;
        return;
    }
    ed.workspace.claude.notes.stale.extend(new);
}

/// The stale drafts — each says `stale` on itself (its line end, its card, the review rail's count).
pub fn stale_shown(ed: &Editor) -> &[u64] {
    &ed.workspace.claude.notes.stale
}

/// Every event: the stale set follows its drafts — one closed or sent since (`C-x` in the notes list, `space v s`,
/// `:note-close`) leaves it.
pub fn stale_sync(ed: &mut Editor) {
    if ed.workspace.claude.notes.stale.is_empty() {
        return;
    }
    let notes = &ed.workspace.claude.notes;
    let left: Vec<u64> =
        (notes.stale.iter().copied()).filter(|&id| notes.list.iter().any(|n| n.id == id)).collect();
    if left.len() != notes.stale.len() {
        ed.workspace.claude.notes.stale = left;
    }
}

/// The key in the notes list (`space N`) closing every stale draft (`close_stale`), said on its top line.
/// A ctrl key, never an Alt one: macOS Terminal and iTerm send no Alt from Option by default, zellij takes several —
/// and the list's query takes the plain letters. `C-l` clears, as in a shell and the chat (a conversation there).
pub const CLOSE_STALE_KEY: &str = "C-l";

/// The notes list's top line beside its name, while drafts are stale: `3 stale`, and the key closing them all — a
/// button too (`:notes-close-stale`).
pub fn stale_said(ed: &Editor) -> Option<String> {
    match ed.workspace.claude.notes.stale.len() {
        0 => None,
        k => Some(format!("{k} stale")),
    }
}

/// `C-l` in the notes list, `:notes-close-stale`: every stale draft closed, said — no card (polish landing 4: the
/// stale card's `y` was 266 of its 294 answers). None stale: said so.
pub fn close_stale(ed: &mut Editor) {
    let ids = std::mem::take(&mut ed.workspace.claude.notes.stale);
    if ids.is_empty() {
        return ed.set_status("no stale drafts — none's lines changed since it was pinned");
    }
    let before = ed.workspace.claude.notes.list.len();
    ed.workspace.claude.notes.list.retain(|n| !ids.contains(&n.id));
    ed.workspace.claude.notes.since_read.retain(|id| !ids.contains(id));
    let k = before - ed.workspace.claude.notes.list.len();
    ed.set_status(format!("{} closed — their lines had changed", crate::nav::count(k, "stale draft")));
}

/// The key on a stale draft's card re-placing it (`re_place`), while that card is open — Helix's `R` (replace with
/// what's yanked) one `esc` away, as `q`'s macro is.
pub const RE_PLACE_KEY: char = 'R';

/// `R` on a stale draft's open card: it stays where it is, pinned to its lines as they are now — stale no more, until
/// they change again.
pub fn re_place(ed: &mut Editor, id: u64) {
    let Some(n) = find(ed, id) else { return };
    let (path, lines) = (n.path.clone(), n.lines.clone());
    // (its card is open where its file is: the text as it is, never the disk read on a key)
    let Some((.., text)) = open_text(ed, &path) else { return };
    let pinned = lines_digest(&text, &lines);
    if let Some(n) = find_mut(ed, id) {
        n.pinned = pinned;
    }
    let notes = &mut ed.workspace.claude.notes;
    notes.stale.retain(|&s| s != id);
    notes.since_read.retain(|&s| s != id);
    let at = format!("{}:{}", crate::chat::shown(&ed.workspace.root, &path), lines.start + 1);
    ed.set_status(format!("re-placed at {at} — stale again only if its lines change"));
}

/// Whether the notes list is titled `title`.
pub fn is_list(title: &str) -> bool {
    title == NOTES
}

/// The rows of the notes list: every note.
pub fn list_items(ed: &Editor) -> Vec<crate::picker::Item> {
    picker_items(ed)
}

/// Whether note `n` is a finding of the reviewer's that leaves with its change (#248): placed (your words never go
/// unasked — `close_by`).
fn landable(n: &Note) -> bool {
    n.by == By::Reviewer && !n.placing
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

/// `landed_check`'s findings closed, each with its base — those still there — said once.
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
    let whose = |n: &Note| by.is_none() || Some(n.by) == by;
    let ids: Vec<u64> = ed.workspace.claude.notes.list.iter().filter(|n| whose(n)).map(|n| n.id).collect();
    let drafts: Vec<&Note> =
        ed.workspace.claude.notes.list.iter().filter(|n| whose(n) && n.by == By::You).collect();
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
    find(ed, id).map(|n| n.by)
}

/// Who `word` names, as `:notes-clear` takes it — the words the cards head entries with: `you`, `reviewer`.
pub fn by_word(word: &str) -> Option<By> {
    By::from_word(word)
}

// ── You ───────────────────────────────────────────────────────────────────

/// `space n` at your cursor — follow, holding the view, pauses and the view comes back to it (`follow::your_view`): in a
/// review, a comment on the line or selection (`comment`); else the ask line with those lines quoted (`ask_about`).
pub fn start(ed: &mut Editor) {
    crate::follow::your_view(ed);
    match crate::codereview::drafting(ed) {
        true => comment(ed),
        false => ask_about(ed),
    }
}

/// `space n` outside a review (#388, chatlink §1.1): the ask line, the lines asked about quoted whole (`asked_lines`) — a
/// place of the answer whether or not it cites them (`Quote.asked`: their flag, `chatnotes::turn_over`). A buffer with no
/// file, or one named like a key Claude may not read, is said and nothing opens.
fn ask_about(ed: &mut Editor) {
    let Some(path) = ed.doc().path.clone() else {
        return ed.set_status("save the file first — a question at lines is about a file's lines");
    };
    // On a walk-through's stop: it holds still while you ask
    crate::follow::hold(ed);
    let lines = asked_lines(ed);
    let d = ed.doc();
    let last = mv::last_line(&d.text);
    let (a, b) = (lines.start.min(last), lines.end.saturating_sub(1).min(last));
    let (from, to) = (mv::line_start(&d.text, a), mv::line_end(&d.text, b));
    let taken = match crate::quote::taken(ed, &[(from, to.max(from))], None) {
        Ok(t) => t,
        Err(e) => return ed.set_status(e),
    };
    crate::quote::add(ed, taken);
    // (the quote just taken — none: the message's limits refused it, said)
    let at = (path, a..b + 1);
    if let Some(q) = ed.workspace.claude.chat.as_mut().and_then(|c| c.quotes.last_mut()) {
        q.asked = Some(at);
    }
}

/// Follow's hold on the view goes with the chat (`chat::close`): where it held the open card's line — the turn's card it
/// brought in sight — the view stays there, the card with its line, until a key of yours moves the cursor or the text
/// (`hold_view`); never back at your cursor with the card's line at the foot and the card below it (focus A1c, the chat
/// ended in the 80-column shot — `:chat-close` now; `space L` hides it, follow and its hold going on). Holding anything
/// else, the view comes back to your cursor as before.
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

/// A review's comment prompt open on the cursor's lines (its card at them — focus C1).
fn comment(ed: &mut Editor) {
    let label = comment_label(ed);
    ed.open_prompt(crate::editor::PromptKind::Note, "");
    // (where the view stands before the line moves it: put back as the line ends — a comment shows no card)
    let d = ed.doc();
    let view = (d.top, d.left, d.top_rows);
    if let Some(p) = ed.prompt.as_mut() {
        (p.label, p.view) = (Some(label), Some(view));
    }
}

/// How many notes hold a draft — what `space v s` sends.
pub fn drafts(ed: &Editor) -> usize {
    ed.workspace.claude.notes.list.len()
}

/// The status line's count of what `space v s` sends (`2 drafts · space v s`) this frame, `rail_w` its rail's width: none
/// where a review's rail counts them, nor where the reviewer's own segment says the same, its findings alone waiting, nor
/// where a note's card says `space v s send N` this frame (#329).
pub fn drafts_said(ed: &Editor, rail_w: usize) -> Option<usize> {
    let mine = |n: &Note| n.by == By::You;
    let rail = ed.review_rail_up() && rail_w > 0 || ed.screen.drafts_drawn.get();
    let findings =
        !ed.workspace.claude.notes.list.iter().any(mine) && crate::reviewer::status_shows(ed, rail_w);
    Some(drafts(ed)).filter(|&k| k > 0 && !rail && !findings)
}

/// A review's comment sent from its line: a draft on the cursor's lines, said — `space v s` sends them all. False: no
/// card to show (the prompt's view put back).
pub fn submit(ed: &mut Editor, text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    let Some(path) = ed.doc().path.clone() else {
        ed.set_status("save the file first — notes live in files");
        return false;
    };
    let lines = asked_lines(ed);
    add(ed, &path, lines, By::You, text);
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
    false
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
    card.or(first_of(ed, &mine, ed.cursor_line())).or(opened_yours(ed))
}

/// `:note-close` — the note `close_target` names goes, for good; follow, holding the view, pauses and the view comes back
/// to your cursor (`follow::your_view`). A refusal moves nothing.
pub fn close_here(ed: &mut Editor) -> Result<(), String> {
    let id = close_target(ed).map(|n| n.id).ok_or("no note on this line")?;
    crate::follow::your_view(ed);
    close(ed, id);
    Ok(())
}

/// Which of its own `n` is, and of how many (focus A1c, #260): the reviewer's finding among the review's, any other note
/// among the others, in `]n`'s order (file, line, age). None where it's the only one.
pub fn position(notes: &Notes, n: &Note) -> Option<(usize, usize)> {
    let reviewer = |m: &Note| m.by == By::Reviewer;
    let mut all: Vec<&Note> = notes.list.iter().filter(|m| reviewer(n) == reviewer(m)).collect();
    crate::sorting::by_key(&mut all, |m| (m.path.clone(), m.lines.start, m.id));
    let at = all.iter().position(|m| m.id == n.id)?;
    (all.len() > 1).then_some((at + 1, all.len()))
}

/// A stop of `]n`'s walk: a note, or a line's flags (its newest answer's flag stands for them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stop {
    Note(u64),
    Flag(u64),
}

/// `]n`'s stops in its order (chatlink Q8): the newest answer's flags in the order written, then every other note and
/// line of flags by (file, line, age) — one stop a line's answers.
fn walk(ed: &Editor) -> Vec<(PathBuf, usize, Stop)> {
    let (notes, flags) = (&ed.workspace.claude.notes, &ed.workspace.claude.flags);
    let lead = |f: &crate::flags::Flag| {
        crate::flags::on_line(flags, &f.path, f.lines.start).first().is_some_and(|g| g.id == f.id)
    };
    let (mut first, mut rest): (Vec<_>, Vec<_>) =
        flags.list.iter().filter(|f| lead(f)).partition(|f| f.origin.n == flags.answers);
    crate::sorting::by_key(&mut first, |f| f.id);
    let mut rest: Vec<(PathBuf, usize, Stop)> =
        rest.drain(..).map(|f| (f.path.clone(), f.lines.start, Stop::Flag(f.id))).collect();
    rest.extend(notes.list.iter().map(|n| (n.path.clone(), n.lines.start, Stop::Note(n.id))));
    crate::sorting::by_key(&mut rest, |s| s.clone());
    let first = first.into_iter().map(|f| (f.path.clone(), f.lines.start, Stop::Flag(f.id)));
    first.chain(rest).collect()
}

/// Whether `]n`/`[n` have another note to go to — a card's `]n next` drawn only then (a key hinted does its key).
pub fn walks_on(ed: &Editor) -> bool {
    walk(ed).len() > 1
}

/// `]n`/`[n` — the next/previous note or flag across every file, wrapping around: after a turn the newest answer's
/// flags in the order written, then the rest in (file, line, age) order (`walk`); notes sharing a line are visited one
/// by one (the card shows the one stepped to), a line's flags one stop.
pub fn step(ed: &mut Editor, forward: bool) {
    let notes = &ed.workspace.claude.notes;
    let all = walk(ed);
    // From the note shown here, else the flag stepped to there (else the line's), else the open card's (not one at
    // Claude's place: you aren't there), else from the cursor line
    let card = notes.open.and_then(|id| find(ed, id)).filter(|n| !at_claudes(ed, n));
    let flag = crate::flags::here(ed).map(|f| {
        let flags = &ed.workspace.claude.flags;
        let lead = crate::flags::on_line(flags, &f.path, f.lines.start).first().map_or(f.id, |g| g.id);
        Stop::Flag(lead)
    });
    let from = here(ed).map(|n| Stop::Note(n.id)).or(flag).or(card.map(|n| Stop::Note(n.id)));
    let target = match from.and_then(|id| all.iter().position(|t| t.2 == id)) {
        Some(k) if forward => all.get(k + 1).or(all.first()),
        Some(k) => k.checked_sub(1).and_then(|k| all.get(k)).or(all.last()),
        None => {
            let mut sorted = all.clone();
            crate::sorting::by_key(&mut sorted, |s| s.clone());
            let here = (ed.doc().path.clone().unwrap_or_default(), ed.cursor_line(), Stop::Note(0));
            let t = if forward {
                sorted.iter().find(|t| **t > here).or(sorted.first())
            } else {
                sorted.iter().rev().find(|t| **t < here).or(sorted.last())
            };
            t.and_then(|t| all.iter().find(|a| a.2 == t.2))
        }
    };
    // (from the one note there is: round to itself — said, never silent: #265)
    let lone = all.len() == 1 && from == Some(all[0].2);
    match target.cloned() {
        Some((p, l, stop)) => {
            // (a key of yours: follow, holding the view, pauses — said once, never left to notice a move, as a note on
            // your own line moves nothing)
            crate::follow::your_view(ed);
            match stop {
                Stop::Note(id) => {
                    ed.workspace.claude.notes.focus = Some(id);
                    if let Err(e) = ed.open_at(&p, l, 0) {
                        ed.set_error(format!("{e:#}"));
                    }
                    open(ed, id);
                }
                Stop::Flag(id) => crate::flags::stepped(ed, id),
            }
            if lone {
                ed.set_status(match stop {
                    Stop::Note(_) => "the only note — 1 of 1",
                    Stop::Flag(_) => "the only flag — 1 of 1",
                });
            }
        }
        None => ed.set_status(NONE_YET),
    }
}

// ── The card between the lines (focus A1b) ──────────────────────────────────────────────────────

/// Note `id`'s card open (`Notes.open`, any other closed): its lines and the card under them put in sight as one block
/// where its file is in front. A code too narrow for a card says so (its line end stays).
pub fn open(ed: &mut Editor, id: u64) {
    ed.workspace.claude.notes.open = Some(id);
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

/// The keys the card layer takes now (`card_key` — the layer `keys_chain` names `Region::NoteCard`): in normal mode
/// with nothing under way, on a file's code (a woven view has no notes) — `enter` on a note's line (its card opens, or
/// closes when it's the one open) or a flag's (the column at its paragraph, `flags::enter`), `q` and `esc` while a card
/// is open in this file (it closes). Helix's `q` (`replay_macro`) is one `esc` away.
pub fn card_keys(ed: &Editor) -> Vec<crate::key::Key> {
    use crate::key::{Code, Key};
    let free = ed.mode == crate::editor::Mode::Normal
        && ed.pending.is_empty()
        && ed.prompt.is_none()
        && ed.on_next_char.is_none()
        && !ed.doc().loading
        && crate::weave::of(ed.doc()).is_none();
    let mut keys = Vec::new();
    if free && (here(ed).is_some() || crate::flags::here(ed).is_some()) {
        keys.push(Key::plain(Code::Enter));
    }
    if free && let Some(o) = opened(ed, ed.doc()) {
        keys.extend([Key::plain(Code::Char('q')), Key::plain(Code::Esc)]);
        // (a stale draft's card: `R` re-places it — polish landing 4)
        if ed.workspace.claude.notes.stale.contains(&o.id) {
            keys.push(Key::plain(Code::Char(RE_PLACE_KEY)));
        }
    }
    keys
}

/// A key of the card layer's (`card_keys`); false: not its.
pub fn card_key(ed: &mut Editor, key: crate::key::Key) -> bool {
    if !card_keys(ed).contains(&key) {
        return false;
    }
    ed.count = None;
    if key.code == crate::key::Code::Char(RE_PLACE_KEY) {
        if let Some(id) = opened(ed, ed.doc()).map(|o| o.id) {
            re_place(ed, id);
        }
        return true;
    }
    // (a flag's line, no note on it: the column at its paragraph, no card between the lines — a note's line is its
    // card's, the flag's `¶` after it the way to the chat)
    if key.code == crate::key::Code::Enter && here(ed).is_none() && crate::flags::enter(ed) {
        return true;
    }
    let notes = &ed.workspace.claude.notes;
    let id = target(ed).map(|n| n.id);
    let open_here = id.is_some() && notes.open == id;
    // (`enter` acts at your cursor: follow, holding the view, pauses and the view comes back to it)
    if key.code == crate::key::Code::Enter {
        crate::follow::your_view(ed);
    }
    match (key.code, id) {
        (crate::key::Code::Enter, Some(id)) if !open_here => open(ed, id),
        _ => close_card(ed),
    }
    true
}

/// The open card closed (`q`, `esc`, `enter` again, a press on its `¶`).
fn close_card(ed: &mut Editor) {
    ed.workspace.claude.notes.open = None;
}

/// A press on note `id`'s line end (`¶`): its card between the lines opens, or closes when it's the one open.
pub fn pressed(ed: &mut Editor, id: u64) {
    match ed.workspace.claude.notes.open == Some(id) {
        true => close_card(ed),
        false => {
            ed.workspace.claude.notes.focus = Some(id);
            open(ed, id);
        }
    }
}

/// What `]n` and `space N` say with neither a note nor a flag to go to.
const NONE_YET: &str =
    "no notes or flags yet — an answer's places are flagged at the code, space n asks about a line";

/// `space N` — every note and flag (#388): `]n`'s lead first, the newest answer's flags in the order written, then the
/// notes newest first, then the older answers' flags, newest answer first. `C-x` closes the note on its row, `A-x` every
/// note its author began (#197); a flag's `enter` goes to its line with the chat at its paragraph.
pub fn picker(ed: &mut Editor) {
    if ed.workspace.claude.notes.list.is_empty() && ed.workspace.claude.flags.list.is_empty() {
        return ed.set_status(NONE_YET);
    }
    let items = picker_items(ed);
    let mut p = crate::picker::Picker::of("notes", NOTES, items, false);
    p.paints = flag_paints(ed);
    ed.open_picker(p, None);
}

/// The flags' rows' cards (`Paint.more`): the sentence that named the place, under the row's place and question.
fn flag_paints(ed: &Editor) -> Vec<(crate::picker::Action, crate::picker::Paint)> {
    let flags = ed.workspace.claude.flags.list.iter();
    flags
        .filter(|f| f.asked.is_some() && !f.words.trim().is_empty())
        .map(|f| {
            let more = Some(crate::chatnotes::plain_words(&f.words));
            let action = crate::picker::Action::Flag { id: f.id, path: f.path.clone(), line: f.lines.start };
            (action, crate::picker::Paint { more, ..Default::default() })
        })
        .collect()
}

/// The notes list's rows (`picker`'s order).
pub fn picker_items(ed: &Editor) -> Vec<crate::picker::Item> {
    let flags = &ed.workspace.claude.flags;
    let (newest, older): (Vec<Item>, Vec<Item>) = {
        let mut all: Vec<&crate::flags::Flag> = flags.list.iter().collect();
        crate::sorting::by_key(&mut all, |f| (std::cmp::Reverse(f.origin.n), f.id));
        let (a, b): (Vec<_>, Vec<_>) = all.into_iter().partition(|f| f.origin.n == flags.answers);
        (a.into_iter().map(|f| flag_row(ed, f)).collect(), b.into_iter().map(|f| flag_row(ed, f)).collect())
    };
    newest.into_iter().chain(note_rows(ed)).chain(older).collect()
}

/// A flag's row in the notes list: where it is and the question its answer answered (as its hover's row — two answers on
/// one line told apart in a few cells; #388 step 3), the sentence that named the place on its card (`flag_paints`) —
/// none asked (a continue's lost question), the sentence itself. F0's doubt after `chat` where tarae has one.
fn flag_row(ed: &Editor, f: &crate::flags::Flag) -> Item {
    let doc = ed.docs.by_path(&f.path);
    let doubt = f.check.as_ref().zip(doc).and_then(|(c, d)| c.doubt(d));
    let at = format!("{}:{}", crate::chat::shown(&ed.workspace.root, &f.path), f.lines.start + 1);
    let said = f.asked.as_deref().unwrap_or(&f.words);
    Item {
        label: format!("{at}  {}", crate::chatnotes::plain_words(said)),
        action: crate::picker::Action::Flag { id: f.id, path: f.path.clone(), line: f.lines.start },
        hint: doubt.map_or("chat".to_string(), |d| format!("chat · {d}")),
        glyph: Some(("¶", "ui.virtual")),
    }
}

/// The notes' rows, newest first.
fn note_rows(ed: &Editor) -> impl Iterator<Item = Item> + '_ {
    use crate::picker::Action;
    ed.workspace.claude.notes.list.iter().rev().map(|n| Item {
        label: row_label(ed, n),
        action: Action::Note { id: n.id, path: n.path.clone(), line: n.lines.start },
        // (whose draft)
        hint: n.by.label().to_string(),
        glyph: Some(match n.by {
            By::Reviewer => ("¶", "type"),
            By::You => ("¶", "ui.accent"),
        }),
    })
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

    /// #251's review: what doesn't come through `add` is said too — a reviewer's finding placed from its snapshot
    /// (`to_json` leaves it out).
    #[test]
    fn a_finding_on_such_a_path_is_said() {
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
    }

    /// Each draft says who wrote it — the reviewer's `finding` the reviewer, a review's comment you — and keeps it through
    /// the restore file; an entry that isn't a draft (a note an older tarae kept from the chat's Claude) left the code
    /// (#388) and isn't brought back.
    #[test]
    fn notes_carry_their_author() {
        let scratch = crate::scratch::Scratch::new("notes-by");
        let dir = std::fs::canonicalize(scratch.path()).unwrap();
        let f = dir.join("a.rs");
        std::fs::write(&f, "one\ntwo\nthree\n").unwrap();
        let mut ed = editor();
        ed.workspace.root = dir.clone();
        ed.open(&f).unwrap();
        let (found, _) =
            finding(&mut ed, &dir, &json!({"path": "a.rs", "line": 2, "text": "the reviewer's"})).unwrap();
        ed.goto_line(2);
        submit(&mut ed, "mine");
        let by: Vec<(By, &str)> =
            ed.workspace.claude.notes.list.iter().map(|n| (n.by, n.text.as_str())).collect();
        assert_eq!(by, [(By::Reviewer, "the reviewer's"), (By::You, "mine")]);
        assert_eq!(ed.workspace.claude.notes.list[0].id, found);
        let mut saved = to_json(&ed.workspace.claude.notes, &ed.docs);
        let words: Vec<&str> = saved.iter().map(|n| n["by"].as_str().unwrap()).collect();
        assert_eq!(words, ["reviewer", "you"]);
        saved.push(json!({"path": f, "lines": [0, 1], "by": "claude", "text": "no one's now"}));
        let mut ed = editor();
        restore_from(&mut ed.workspace.claude.notes, &mut ed.docs, saved);
        let by: Vec<By> = ed.workspace.claude.notes.list.iter().map(|n| n.by).collect();
        assert_eq!(by, [By::Reviewer, By::You]);
        // The reviewer pins findings in the checkout only, and answers no thread
        let away = crate::scratch::Scratch::new("notes-by-out");
        let outside = away.join("out.rs");
        std::fs::write(&outside, "x\n").unwrap();
        assert!(finding(&mut ed, &dir, &json!({"path": outside, "line": 1, "text": "out"})).is_err());
        assert!(finding(&mut ed, &dir, &json!({"reply_to": 1, "text": "re"})).is_err());
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

    /// #197: the author fixed what the reviewer found (asked in the chat, not with `space v s`) — the
    /// findings stayed drafts. A turn's end marks the drafts whose lines changed since they were pinned stale —
    /// no card (polish landing 4); `:notes-close-stale` closes them all. A draft whose lines are as they were,
    /// and a note that isn't a draft, stay unmarked — and a finding in a file not open counts its lines as the
    /// file had them then.
    #[test]
    fn drafts_on_lines_changed_since_are_marked_stale() {
        let scratch = crate::scratch::Scratch::new("notes-stale");
        let dir = scratch.to_path_buf();
        let (f, g) = (dir.join("a.rs"), dir.join("b.rs"));
        std::fs::write(&f, "one\ntwo\nthree\nfour\n").unwrap();
        std::fs::write(&g, "alpha\nbeta\n").unwrap();
        let mut ed = editor();
        ed.workspace.root = dir.clone();
        ed.open(&f).unwrap();
        add(&mut ed, &f, 0..1, By::You, "the author's");
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
        stale_check(&mut ed);
        settle_until(&mut ed, |ed| stale_shown(ed).len() == 2);
        assert!(ed.offers.is_empty(), "no card");
        let stale: Vec<&str> =
            stale_shown(&ed).iter().map(|&id| super::find(&ed, id).unwrap().gist()).collect();
        assert_eq!(stale, ["two is wrong", "beta"], "its lines as they were: four too stays");
        crate::typed::execute(&mut ed, "notes-close-stale").unwrap();
        assert_eq!(gists(&ed), ["the author's", "four too"]);
        assert!(stale_shown(&ed).is_empty());
        // Nothing changed since: nothing marked
        stale_check(&mut ed);
        settle_for(&mut ed, 300);
        assert!(stale_shown(&ed).is_empty() && ed.offers.is_empty());
        // Line 4 changes: marked, still no card
        ed.goto_line(3);
        feed(&mut ed, "Afixed<esc>");
        stale_check(&mut ed);
        settle_until(&mut ed, |ed| stale_shown(ed).len() == 1);
        assert!(ed.offers.is_empty());
        assert_eq!(gists(&ed), ["the author's", "four too"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Polish landing 4: a stale draft says so on itself, never on a card — `stale ·` at its line end, its card's top
    /// line, `R re-place` first on its bottom line; `R` there pins it to its lines as they are (stale no more, until
    /// they change again). The notes list (`space N`) counts them on its top line with `C-l close them`; `C-x` closing
    /// one, the count follows; `C-l` closes the rest. (The files are the test's own.)
    #[test]
    fn a_stale_draft_says_so_on_itself_and_the_notes_list_closes_them() {
        let scratch = crate::scratch::Scratch::new("notes-stale-one");
        let dir = scratch.to_path_buf();
        let f = dir.join("a.rs");
        std::fs::write(&f, "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let mut ed = editor();
        ed.open(&f).unwrap();
        for (line, text) in [(2, "two is wrong"), (3, "three too"), (5, "five")] {
            finding(&mut ed, &dir, &json!({"path": "a.rs", "line": line, "text": text})).unwrap();
        }
        let fix = |ed: &mut Editor, line: usize| {
            ed.goto_line(line);
            feed(ed, "Afixed<esc>");
            stale_check(ed);
        };
        fix(&mut ed, 1);
        settle_until(&mut ed, |ed| !stale_shown(ed).is_empty());
        assert!(ed.offers.is_empty(), "no card");
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
        assert!(screen.contains("╭─ review · 1/3 ── stale · its lines changed ─"), "its top line: {screen}");
        assert!(screen.contains("╰─ R re-place"), "its key first: {screen}");
        crate::screen::frames_sized(&mut ed, "cards4-stale-card", crate::screen::SHOT_SIZES);
        // `R`: re-placed where it is, no longer stale — said
        feed(&mut ed, "R");
        assert!(stale_shown(&ed).is_empty(), "re-placed");
        assert_eq!(gists(&ed), ["two is wrong", "three too", "five"], "nothing closed");
        assert!(!rows(&mut ed).contains("stale · two is wrong"));
        // Its lines change again, and another's: two stale
        feed(&mut ed, "q");
        fix(&mut ed, 1);
        fix(&mut ed, 2);
        settle_until(&mut ed, |ed| stale_shown(ed).len() == 2);
        // The notes list counts them on its top line, with the key that closes them
        feed(&mut ed, "<space>N");
        assert!(ed.picker.as_ref().is_some_and(|p| is_list(&p.title)));
        assert!(rows(&mut ed).contains("─ notes ── 2 stale · C-l close them ─"), "{}", rows(&mut ed));
        crate::screen::frames_sized(&mut ed, "cards4-stale-list", crate::screen::SHOT_SIZES);
        // `C-x` on a stale row: the count follows
        let at =
            ed.picker.as_ref().unwrap().items().iter().position(|i| i.label.contains("three too")).unwrap();
        ed.picker.as_mut().unwrap().selected = at;
        feed(&mut ed, "<C-x>");
        assert_eq!(stale_shown(&ed).len(), 1);
        assert!(rows(&mut ed).contains("─ notes ── 1 stale · C-l close them ─"));
        // `C-l`: the rest, the list following
        feed(&mut ed, "<C-l>");
        assert_eq!(gists(&ed), ["five"]);
        assert_eq!(ed.picker.as_ref().map(|p| p.items().len()), Some(1), "the list follows");
        let screen = rows(&mut ed);
        assert!(!screen.contains("stale ·") && screen.contains("1 stale draft closed"), "{screen}");
        assert!(ed.offers.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// #197: closing notes by who wrote them — in the notes list (`space N`) `C-x` closes the note on its row,
    /// `A-x` every note its author began (the reviewer's findings, all at once); `:notes-clear reviewer`
    /// does the same from the command line.
    #[test]
    fn notes_close_by_row_or_by_author() {
        let scratch = crate::scratch::Scratch::new("notes-close");
        let dir = std::fs::canonicalize(scratch.path()).unwrap();
        let f = dir.join("a.rs");
        std::fs::write(&f, "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let mut ed = editor();
        ed.workspace.root = dir.clone();
        ed.open(&f).unwrap();
        finding(&mut ed, &dir, &json!({"path": "a.rs", "line": 2, "text": "found one"})).unwrap();
        finding(&mut ed, &dir, &json!({"path": "a.rs", "line": 3, "text": "found two"})).unwrap();
        ed.goto_line(3);
        submit(&mut ed, "mine");
        feed(&mut ed, "<space>Nfound two<A-x>");
        assert_eq!(gists(&ed), ["mine"], "the reviewer's, all");
        let rows = ed.picker.as_ref().map(|p| (p.items().len(), p.shown().count()));
        assert_eq!(rows, Some((1, 0)), "the list follows (narrowed to `found two`: none left)");
        assert!(ed.toasts.iter().any(|t| t.text.contains("2 notes reviewer began closed")), "said");
        feed(&mut ed, "<esc><space>Nmine<C-x>");
        assert!(gists(&ed).is_empty());
        feed(&mut ed, "<esc>");
        finding(&mut ed, &dir, &json!({"path": "a.rs", "line": 4, "text": "found three"})).unwrap();
        crate::typed::execute(&mut ed, "notes-clear reviewer").unwrap();
        assert!(ed.workspace.claude.notes.list.is_empty() && ed.offers.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Closing notes when some of them are your unsent drafts (a review's comments): a card names them first — `n` keeps
    /// every one, `y` closes them; from the notes list the card takes the keys (the list goes). None of yours among them:
    /// closed at once.
    #[test]
    fn closing_by_author_asks_first_over_your_unsent_drafts() {
        let scratch = crate::scratch::Scratch::new("notes-drafts");
        let dir = std::fs::canonicalize(scratch.path()).unwrap();
        let f = dir.join("a.rs");
        std::fs::write(&f, "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let mut ed = editor();
        ed.workspace.root = dir.clone();
        ed.open(&f).unwrap();
        finding(&mut ed, &dir, &json!({"path": "a.rs", "line": 2, "text": "found one"})).unwrap();
        finding(&mut ed, &dir, &json!({"path": "a.rs", "line": 3, "text": "found two"})).unwrap();
        ed.goto_line(3);
        submit(&mut ed, "mine");
        // A bare `:notes-clear` (every note): asked, naming each of yours; `n` keeps them all
        crate::typed::execute(&mut ed, "notes-clear").unwrap();
        let o = ed.offers.first().expect("the card");
        assert_eq!(o.title, "Close all 3 notes?");
        assert!(o.question.contains("a.rs:4 · mine") && !o.question.contains("found one"), "{}", o.question);
        feed(&mut ed, "<C-q>n");
        assert!(ed.offers.is_empty());
        assert_eq!(gists(&ed), ["found one", "found two", "mine"], "kept, every one");
        // The notes list's `A-x` on yours: the card has the keys, `y` closes them
        feed(&mut ed, "<space>Nmine<A-x>");
        assert!(ed.picker.is_none() && !ed.offers.is_empty(), "the card, not the list");
        feed(&mut ed, "y");
        assert_eq!(gists(&ed), ["found one", "found two"]);
        // None of yours: at once
        crate::typed::execute(&mut ed, "notes-clear").unwrap();
        assert!(ed.offers.is_empty() && ed.workspace.claude.notes.list.is_empty());
        assert!(ed.toasts.iter().any(|t| t.text.contains("2 notes closed")), "said");
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
        add(&mut ed, &f, 1..2, By::Reviewer, "same");
        add(&mut ed, &f, 1..2, By::Reviewer, "same");
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
        add(&mut ed, &f, 2..3, By::You, "look here");
        let mut saved = to_json(&ed.workspace.claude.notes, &ed.docs);
        assert_eq!(saved[0]["anchor"], "target();");
        // Edited outside tarae: two lines above
        std::fs::write(&f, "x\ny\na\nb\ntarget();\nc\n").unwrap();
        saved.push(
            json!({"path": f, "lines": [50, 52], "anchor": "gone", "by": "you", "text": "past the end"}),
        );
        let mut ed = editor();
        restore_from(&mut ed.workspace.claude.notes, &mut ed.docs, saved);
        assert_eq!(ed.workspace.claude.notes.list.len(), 2);
        ed.open(&f).unwrap();
        attach(&mut ed);
        let n = &ed.workspace.claude.notes.list[0];
        assert_eq!(n.lines, 4..5, "re-anchored on its line's text");
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
        let saved = vec![json!({"path": f, "lines": [1, 2], "by": "reviewer", "text": "here"})];
        restore_from(&mut notes, &mut docs, saved);
        let n = &notes.list[0];
        assert_eq!(n.at.map(|(d, _)| d), Some(4), "the loaded buffer of its file, past the loading one");
        assert_eq!(n.lines, 1..2);
    }
}
