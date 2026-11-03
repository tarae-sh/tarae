//! What the answer says of a place shows at it (#239): the last answer's `path:line` places are notes in the code —
//! the sentence that names one at its line end, the paragraph it's in (a list item: that item) on the note's card,
//! `chat` where a note says its author. No tool call and no token: tarae reads Claude's prose for them.
//!
//! - At a turn's end, its answer (the turn's prose, every message of it) is cut by its source text, never the rows it
//!   wrapped to (`cut`): blocks — a paragraph, a list item, a heading, a table row; fenced code is no prose — then a
//!   block's sentences. A walk-through's stop (`<show at=…>why</show>`) is its place, `why` its sentence, the block
//!   after it its paragraph. `a.rs:120-140` covers those lines.
//! - Where a name is goes by the chat's one answer (`places::Known::place`, the answers drawn): a name that comes to
//!   no file, or to several, is no place; one outside the project and its folders neither. Found on the worker the
//!   frame asks (`places::after_frame`), so the notes are placed the event its answers land (`sync`).
//! - The answer's place holds its flow (focus A1): where its last words (after its last call: before one it's the
//!   turn's narration) say something no place takes, the note at the answer's place — the first place of the newest of
//!   its messages that names one (where follow takes the view, `follow::answer_place`), else the turn's last edit, else
//!   its last read in the project — is one card of those words in the order written (`Part`): the paragraph about that
//!   line marked, every place-less one whole, one placed elsewhere a line with its place. One note there, never two:
//!   that line's own place note is it. Its line end is that line's sentence (at a stop, the first place-less one's).
//! - Kept for the conversation (focus A1b, reversing #239's "the last answer's, replaced by the next"): every answer's
//!   notes stay while its conversation does — a new one (`C-l`, `:chat-new`, `/clear`) clears them, a continue
//!   (`--resume` after `:restart`, `C-p`) brings them back (`restore`). Kept with the conversation (`places_json`):
//!   places only — the name as written, the path, the lines, the line's text —, the words cut again from its rows. One
//!   line's answers are one card, newest first (`notes::group`); the line end says the newest answer's places only.
//! - The newest answer opens its own card at the turn's end (`notes::open_by_turn`), never while you type; it stays
//!   until the next answer opens its own or you close it. An answer no place holds — none of its names a file here,
//!   no stop for its words — opens the reader instead (`chat::read_by_itself`, the flow plan's Q7). One you answered with `space n` is a thread — kept, as
//!   every note is.
//! - A line where the same answer's own `note` sits gets no chat note there (Claude said it there already); a turn that
//!   wrote at a note at all (a new one or a reply) gets no flow — its answer is at the code already.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::chat::{Msg, Role};
use crate::editor::Editor;

/// Places an answer gives notes at most.
const PLACES_MAX: usize = 40;

/// The most of a sentence (the line end) and a paragraph (the card) kept, in characters.
const SENTENCE_MAX: usize = 400;
const PARAGRAPH_MAX: usize = 1_500;

/// A place an answer names, with what it says of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Said {
    /// The name as written (`src/a.rs`, `a.rs`).
    pub name: Arc<str>,
    /// Its lines, from 0, the end out.
    pub lines: Range<usize>,
    pub sentence: String,
    pub paragraph: String,
    /// The answer's blocks its paragraph is (`blocks`, numbered through the turn's messages): a note of it placed,
    /// they're said.
    pub blocks: Range<usize>,
}

/// An answer's places waiting for what their names come to (`Notes.answer`), and the notes its own `note` pinned.
#[derive(Clone, Debug, Default)]
pub struct Waiting {
    pub places: Vec<Said>,
    pub own: Vec<u64>,
    /// The turn wrote at a note (`Notes.noted_now`): no flow — the answer is there.
    pub noted: bool,
    /// The answer's blocks, every message of the turn's in order — those from `last` on that no placed note covers are
    /// the answer's note.
    pub blocks: Vec<Block>,
    /// The first block of the turn's last words, after its last call: what it said before one is the turn's narration
    /// (`Let me read…`), never the answer's note.
    pub last: usize,
    /// Every place named — its name, first line and blocks — the newest message's first, a place named again too: a
    /// block naming one that's placed is said; where that note goes, the first that came to a file, else the first of
    /// `stops` in the project.
    pub named: Vec<(Arc<str>, usize, Range<usize>)>,
    /// Where the turn stopped in a file (`stops`): its edits, newest first, then its reads.
    pub stops: Vec<(PathBuf, usize)>,
    /// The question the answer answered: its first line that says something (`Note.asked`).
    pub asked: Option<String>,
    /// Its question's place among the conversation's messages of yours, from 0 (`Origin.turn`).
    pub turn: usize,
    /// Its turn's stops, as it ended — what its places are judged by (`turnmap::Judge`, focus F0).
    pub judge: crate::turnmap::Judge,
}

/// Which answer a chat note is of, and where in it (focus A1b): what the conversation keeps of it (`places_json`) and
/// finds again in its words on a continue (`restore`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    /// The answer's number in this chat, from 1 — the newest the highest.
    pub n: u64,
    /// When it was given, unix seconds.
    pub at: u64,
    /// Its question's place among the conversation's messages of yours, from 0.
    pub turn: usize,
    /// The place as written (`src/a.rs`) and its first line as written, from 0 — a flow at a stop: no name.
    pub name: Arc<str>,
    pub said: usize,
    /// Its line's text as placed — what finds it again once the file moved it.
    pub text: Option<String>,
}

/// A place of an answer whose note holds its paragraph at the code (focus A1b step 3): what the chat folds that
/// paragraph to once a newer answer came (`chatdoc::Fold`).
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Stub {
    /// Its answer's question's place among the conversation's messages of yours (`Origin.turn`).
    pub turn: usize,
    /// The place as written (`Origin.name`, `Origin.said`).
    pub name: Arc<str>,
    pub said: usize,
    pub path: PathBuf,
    /// Its line now, from 0.
    pub line: usize,
    pub note: u64,
}

/// Every place the conversation's answers have a note at, by answer (a flow at a stop has no name, so no paragraph
/// comes to it). A note closed is no stub: its paragraph is whole again.
pub fn stubs(notes: &crate::notes::Notes) -> Vec<Stub> {
    let mut out: Vec<Stub> = (notes.list.iter())
        .filter_map(|n| {
            let o = n.answer.as_ref()?;
            Some(Stub {
                turn: o.turn,
                name: o.name.clone(),
                said: o.said,
                path: n.path.clone(),
                line: n.lines.start,
                note: n.id,
            })
        })
        .collect();
    crate::sorting::by_key(&mut out, |s| (s.turn, s.note));
    out
}

/// A block of an answer as it reads (`blocks`): its text, whether it's a list item, and the fenced code that follows it
/// where it ends with `:` (focus A1c, #259: that code is what it says, so its note's card shows it) — as written, its
/// fences too, `CODE_LINES_MAX` lines of it at most.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub text: String,
    pub item: bool,
    pub code: Option<String>,
}

impl Block {
    /// Its text, its code after it (`Block.code`) — what its note's card renders.
    pub fn whole(&self) -> String {
        with_code(self.text.clone(), Some(self))
    }
}

/// The most lines of a block's code its note keeps (the chat has it all).
const CODE_LINES_MAX: usize = 30;

/// `paragraph`, `b`'s code after it where it has some.
fn with_code(paragraph: String, b: Option<&Block>) -> String {
    match b.and_then(|b| b.code.as_deref()) {
        Some(code) => format!("{paragraph}\n\n{code}"),
        None => paragraph,
    }
}

/// A block of the answer's flow on its place's card (`Note.flow`): the one about this line (marked), one no place takes
/// (whole), one placed elsewhere — its place as shown (`src/thread.rs:7`), its words one line there, whole at its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Here(Block),
    Whole(Block),
    Elsewhere(String, Block),
}

impl Part {
    pub fn block(&self) -> &Block {
        match self {
            Part::Here(b) | Part::Whole(b) | Part::Elsewhere(_, b) => b,
        }
    }
}

/// The places `text` (Claude's answer as written) names, each with its sentence and paragraph — the first time each
/// place is named.
#[cfg(test)]
fn cut(text: &str) -> Vec<Said> {
    let mut out: Vec<Said> = Vec::new();
    for s in cut_blocks(&blocks(text), 0) {
        if out.len() < PLACES_MAX && !out.iter().any(|o| o.name == s.name && o.lines.start == s.lines.start) {
            out.push(s);
        }
    }
    out
}

/// Every place an answer's `blocks` name, each time it's named — a place named again too: each naming says its
/// block is about a place (the flow's `here`/`at`); the notes take each place's first (`turn_over`). `PLACES_MAX`
/// places at most, counted once each: a repeat of one kept is kept (the answer's length bounds them), a new one past
/// the cap left out. `base` the number of the first block among the turn's.
fn cut_blocks(blocks: &[Block], base: usize) -> Vec<Said> {
    let mut out: Vec<Said> = Vec::new();
    let mut kept: Vec<(Arc<str>, usize)> = Vec::new();
    let mut push = |s: Said, out: &mut Vec<Said>| {
        let place = (s.name.clone(), s.lines.start);
        if !kept.contains(&place) {
            if kept.len() >= PLACES_MAX {
                return;
            }
            kept.push(place);
        }
        out.push(s);
    };
    for (i, b) in blocks.iter().enumerate() {
        let b = &b.text;
        // (a walk-through's stop: its `why` the sentence, what follows it the paragraph — the rest of its block, else
        // the block after it)
        let stops = crate::chat::show_tags(b);
        let prose = without(b, stops.iter().map(|s| s.0.clone()));
        let prose = prose.trim();
        for (_, at, why) in &stops {
            let Some((whole, path, line)) = crate::places::names_in(at).into_iter().next() else { continue };
            let next = blocks.get(i + 1).map(|n| n.text.as_str()).filter(|n| !n.contains("<show"));
            let (after, upto) = match (Some(prose).filter(|p| !p.is_empty()), next) {
                (Some(p), _) => (Some(p), i + 1),
                (None, Some(n)) => (Some(n), i + 2),
                (None, None) => (None, i + 1),
            };
            let paragraph = match after {
                Some(n) if !why.is_empty() => format!("{why}\n\n{n}"),
                Some(n) => n.to_string(),
                None => why.clone(),
            };
            // (the code its last block ends on, `:`, after it)
            let code = |p: String| with_code(p, blocks.get(upto - 1));
            // (a stop that says no why: what follows it begins the line end)
            let first =
                |p: &str| sentences(p).first().map_or(String::new(), |s| p[s.clone()].trim().to_string());
            let sentence = if why.is_empty() { first(&paragraph) } else { why.clone() };
            let said = Said {
                name: Arc::from(&at[path]),
                lines: line..range_end(at, whole.end).max(line + 1),
                sentence: capped(&plain(&sentence), SENTENCE_MAX),
                paragraph: code(capped(&paragraph, PARAGRAPH_MAX)),
                blocks: base + i..base + upto,
            };
            push(said, &mut out);
        }
        let sentences = sentences(prose);
        for (whole, path, line) in crate::places::names_in(prose) {
            let s = sentences.iter().find(|s| s.contains(&whole.start)).cloned().unwrap_or(0..prose.len());
            let said = Said {
                name: Arc::from(&prose[path]),
                lines: line..range_end(prose, whole.end).max(line + 1),
                sentence: capped(&plain(prose[s].trim()), SENTENCE_MAX),
                paragraph: with_code(capped(prose, PARAGRAPH_MAX), blocks.get(i)),
                blocks: base + i..base + i + 1,
            };
            push(said, &mut out);
        }
    }
    out
}

/// `text`'s blocks as they read: a paragraph's lines joined, a list item (its marker off, the lines under it
/// joined), a heading, a table row, a quote's text; fenced code no block of its own — the code of the block before it
/// where that one ends with `:` (`Block.code`), else left out.
fn blocks(text: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut cur: Option<Block> = None;
    let mut fence: Option<&str> = None;
    // (the code of a block ending `:` as it's read, `CODE_LINES_MAX` lines and its closing fence)
    let mut code: Option<String> = None;
    let flush = |cur: &mut Option<Block>, out: &mut Vec<Block>| {
        if let Some(b) = cur.take().filter(|b| !b.text.trim().is_empty()) {
            out.push(b);
        }
    };
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(f) = fence {
            let end = t.starts_with(f);
            if let Some(c) = code.as_mut().filter(|c| end || c.lines().count() <= CODE_LINES_MAX) {
                c.push_str(line.trim_end());
                c.push('\n');
            }
            if end {
                fence = None;
                if let (Some(c), Some(b)) = (code.take(), out.last_mut()) {
                    b.code = Some(c.trim_end().to_string());
                }
            }
            continue;
        }
        if let Some(f) = ["```", "~~~"].into_iter().find(|f| t.starts_with(f)) {
            flush(&mut cur, &mut out);
            fence = Some(f);
            let said = out.last().is_some_and(|b| b.code.is_none() && b.text.trim_end().ends_with(':'));
            code = said.then(|| format!("{}\n", line.trim_end()));
            continue;
        }
        let t = t.strip_prefix('>').map_or(t, str::trim_start);
        if t.is_empty() {
            flush(&mut cur, &mut out);
            continue;
        }
        // (a heading and a table row are blocks of their own; a list item starts one)
        if t.starts_with('#') || t.starts_with('|') {
            flush(&mut cur, &mut out);
            out.push(Block { text: t.trim_start_matches('#').trim().to_string(), item: false, code: None });
            continue;
        }
        match item(t) {
            Some(body) => {
                flush(&mut cur, &mut out);
                cur = Some(Block { text: body.to_string(), item: true, code: None });
            }
            None => match cur.as_mut() {
                Some(c) => {
                    c.text.push(' ');
                    c.text.push_str(t.trim_end());
                }
                None => cur = Some(Block { text: t.trim_end().to_string(), item: false, code: None }),
            },
        }
    }
    flush(&mut cur, &mut out);
    // (an answer cut inside its code: what came of it)
    if let (Some(c), Some(b)) = (code, out.last_mut()) {
        b.code = Some(c.trim_end().to_string());
    }
    out
}

/// A list item's text, its marker off (`- `, `* `, `+ `, `1. `, `1) `) — None for a line that isn't one.
fn item(t: &str) -> Option<&str> {
    if let Some(rest) = ["- ", "* ", "+ "].into_iter().find_map(|m| t.strip_prefix(m)) {
        return Some(rest.trim());
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    let rest = t[digits..].strip_prefix(". ").or_else(|| t[digits..].strip_prefix(") "));
    rest.filter(|_| (1..=3).contains(&digits)).map(str::trim)
}

/// `text` without the byte ranges `cut` (a walk-through's tags).
fn without(text: &str, cut: impl Iterator<Item = Range<usize>>) -> String {
    let (mut out, mut last) = (String::new(), 0);
    for r in cut {
        out.push_str(&text[last..r.start]);
        last = r.end;
    }
    out.push_str(&text[last..]);
    out
}

/// The byte ranges of `text`'s sentences: one ends at `.`, `!` or `?` (closing marks after it) before a blank or
/// the end — never inside `code`, nor after `e.g`, `i.e`, `etc`, `vs`.
fn sentences(text: &str) -> Vec<Range<usize>> {
    let b = text.as_bytes();
    let (mut out, mut start, mut code) = (Vec::new(), 0, false);
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'`' => code = !code,
            b'.' | b'!' | b'?' if !code => {
                let mut end = i + 1;
                while end < b.len() && b")\"'*_".contains(&b[end]) {
                    end += 1;
                }
                let word =
                    text[start..i].rsplit(|c: char| c.is_whitespace() || c == '(').next().unwrap_or("");
                let short = matches!(word.to_ascii_lowercase().as_str(), "e.g" | "i.e" | "etc" | "vs" | "cf");
                if (end == b.len() || b[end].is_ascii_whitespace()) && !short {
                    out.push(start..end);
                    start = end;
                    i = end;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if start < b.len() {
        out.push(start..b.len());
    }
    out
}

/// A range's last line after `path:line` ending at `at` (`-140`, `–140`): the end out, from 0 — 0 for none.
fn range_end(text: &str, at: usize) -> usize {
    let rest = &text[at..];
    let Some(rest) = rest.strip_prefix('-').or_else(|| rest.strip_prefix('–')) else { return 0 };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    rest[..digits].parse::<usize>().unwrap_or(0)
}

/// A sentence as the line end shows it, in the italic of its note: markdown's marks off (`code`, **strong**).
fn plain(s: &str) -> String {
    s.replace("**", "").replace('`', "")
}

/// `s` within `max` characters, cut with `…`.
fn capped(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s.to_string(),
    }
}

/// A chat turn ended (`ok`: whole, neither stopped nor failed): the places its answer names wait for what their names
/// come to (`sync`) — with the notes it pinned itself, whose lines take none, and the rest of what it says, for the
/// answer's note. An answer that names none and whose turn stopped nowhere leaves the last one's, and so do a turn cut
/// short (a partial answer, its last sentence cut) and one that began with no message of the user's
/// (`Chat.turn_bare`: idle compaction's `/compact` — its answer isn't an answer, and the last user message's would be
/// the previous turn's, placed again).
pub fn turn_over(ed: &mut Editor, ok: bool) {
    let notes = &mut ed.workspace.claude.notes;
    let own = std::mem::take(&mut notes.pinned_now);
    let noted = std::mem::take(&mut notes.noted_now);
    let Some(c) = ed.workspace.claude.chat.as_ref().filter(|c| ok && !c.turn_bare) else { return };
    let turn = c.msgs.iter().rposition(|m| m.role == Role::User).map_or(0, |i| i + 1);
    let mut said = read_answer(&c.msgs[turn..]);
    let stops = stops(c);
    if said.places.is_empty() && (noted || said.blocks.len() <= said.last || stops.is_empty()) {
        // (words of its own after its last call, no place to hold them and no note it wrote at: the reader opens by
        // itself — the flow plan's Q7)
        if !noted && said.blocks.len() > said.last {
            crate::chat::read_by_itself(ed);
        }
        return;
    }
    // (asked after this frame — a turn's end draws one)
    for s in &said.places {
        let _ = c.places.place(&s.name);
    }
    said.asked = asked(&c.msgs[..turn]);
    said.turn = c.msgs[..turn].iter().filter(|m| m.role == Role::User).count().saturating_sub(1);
    (said.own, said.noted, said.stops) = (own, noted, stops);
    let turn = c.follow.replay.turns().back().map_or(&[][..], |t| &t.stops[..]);
    said.judge = crate::turnmap::Judge::of(turn, &ed.workspace.root, &c.edits.seen);
    ed.workspace.claude.notes.answer = Some(said);
    ed.unchanged = false;
    sync(ed);
}

/// The question a turn answered, from the messages up to its answer: its first line that says something.
fn asked(before: &[Msg]) -> Option<String> {
    let q = before.last().filter(|m| m.role == Role::User)?;
    q.text.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}

/// An answer as it reads, from `msgs` — one turn's, after its question: its places (each the first time it's named),
/// its blocks, every place named (the newest message's first) and where its last words begin. What `turn_over` reads
/// at its end and `restore` again from the conversation's rows.
fn read_answer(msgs: &[Msg]) -> Waiting {
    let mut w = Waiting::default();
    let called = msgs.iter().rposition(|m| m.role == Role::Tool).map_or(0, |i| i + 1);
    for (i, m) in msgs.iter().enumerate().filter(|(_, m)| m.role == Role::Assistant) {
        let these = blocks(&m.text);
        let said = cut_blocks(&these, w.blocks.len());
        w.blocks.extend(these);
        // (said before a call: the turn's narration)
        if i < called {
            w.last = w.blocks.len();
        }
        // (the newest message's places first, each message's in order)
        w.named.splice(0..0, said.iter().map(|s| (s.name.clone(), s.lines.start, s.blocks.clone())));
        for s in said {
            if w.places.len() < PLACES_MAX
                && !w.places.iter().any(|p| p.name == s.name && p.lines.start == s.lines.start)
            {
                w.places.push(s);
            }
        }
    }
    w
}

/// Where the turn stopped in a file (this turn's stops, `replay`): its edits' first changed lines, newest first, then
/// its reads' first lines, newest first.
fn stops(c: &crate::chat::Chat) -> Vec<(PathBuf, usize)> {
    let stops = c.follow.replay.turns().back().map_or(&[][..], |t| &t.stops[..]);
    let at = |s: &crate::replay::Stop| Some((s.path.clone()?, s.lines.as_ref().map_or(0, |l| l.start)));
    let read = |s: &&crate::replay::Stop| s.page.is_none() && s.look.mark() == crate::chat::Mark::Read;
    let edits = stops.iter().rev().filter(|s| !s.hunks.is_empty()).filter_map(at);
    edits.chain(stops.iter().rev().filter(read).filter_map(at)).collect()
}

/// The answer's flow for its place's card (focus A1), in the order written: its last words' blocks and any before them
/// about this line (`here`), each by where it's placed (`at`: a place elsewhere, as shown) — past `PARAGRAPH_MAX`
/// characters cut, `The rest is in the chat.` after it.
fn flow(blocks: &[Block], last: usize, here: &[bool], at: &[Option<String>]) -> Vec<Part> {
    let mut out = Vec::new();
    let mut room = PARAGRAPH_MAX;
    for (k, b) in blocks.iter().enumerate().filter(|(k, _)| *k >= last || here[*k]) {
        // (a place elsewhere is one line: what it takes of the room)
        let size = match at[k].is_some() && !here[k] {
            true => b.text.chars().count().min(60),
            false => b.text.chars().count() + b.code.as_ref().map_or(0, |c| c.chars().count()),
        };
        let cut = size > room;
        let b = match cut {
            true => Block { text: capped(&b.text, room), item: b.item, code: None },
            false => b.clone(),
        };
        out.push(match (&at[k], here[k]) {
            (_, true) => Part::Here(b),
            (Some(p), false) => Part::Elsewhere(p.clone(), b),
            (None, false) => Part::Whole(b),
        });
        if cut {
            out.push(Part::Whole(Block { text: "The rest is in the chat.".into(), item: false, code: None }));
            break;
        }
        room -= size;
    }
    out
}

/// The flow as one text — its note's first entry (what a reply carries, what's copied): a block a paragraph (a list's
/// items line by line), one placed elsewhere after its place.
pub fn flow_text(parts: &[Part]) -> String {
    let mut text = String::new();
    let mut item = false;
    for p in parts {
        let b = p.block();
        if !text.is_empty() {
            text.push_str(if item && b.item { "\n" } else { "\n\n" });
        }
        if let Part::Elsewhere(at, _) = p {
            text.push_str(&format!("› {at}  "));
        }
        if b.item {
            text.push_str("- ");
        }
        text.push_str(&b.text);
        // (a place elsewhere is its line: its code is at its own place)
        if !matches!(p, Part::Elsewhere(..))
            && let Some(code) = &b.code
        {
            text.push_str(&format!("\n\n{code}"));
        }
        item = b.item;
    }
    text
}

/// A text's first sentence as a line end says it.
fn first_sentence(text: &str) -> String {
    let s = sentences(text).first().map_or(text, |s| &text[s.clone()]);
    capped(&plain(s.trim()), SENTENCE_MAX)
}

/// A paragraph placed elsewhere, as a flow's line shows it: its words, markdown's marks off.
pub fn plain_words(text: &str) -> String {
    plain(text)
}

/// A note an answer places (`placed`): where, what it says, and where it came from in the answer.
struct Placed {
    path: PathBuf,
    lines: Range<usize>,
    sentence: String,
    paragraph: String,
    flow: Vec<Part>,
    name: Arc<str>,
    said: usize,
}

/// The notes answer `said` places, in its order: `found` its places that came to a file here, `own` the lines its own
/// `note`s took, `end` where its flow goes (focus A1), `shown` a path as a place is shown.
fn placed(
    said: &Waiting,
    found: &[(PathBuf, &Said)],
    own: &[(&PathBuf, &Range<usize>)],
    end: Option<(PathBuf, usize)>,
    shown: &dyn Fn(&Path) -> String,
) -> Vec<Placed> {
    let place_of = |n: &Arc<str>, l: usize| found.iter().find(|(_, s)| s.name == *n && s.lines.start == l);
    let mut placed: Vec<Placed> = found
        .iter()
        .filter(|(p, s)| !own.iter().any(|(op, ol)| *op == p && ol.contains(&s.lines.start)))
        .map(|(p, s)| Placed {
            path: p.clone(),
            lines: s.lines.clone(),
            sentence: s.sentence.clone(),
            paragraph: s.paragraph.clone(),
            flow: Vec::new(),
            name: s.name.clone(),
            said: s.lines.start,
        })
        .collect();
    let Some((path, line)) = end else { return placed };
    // (each block: about this line, else the first place it names that's placed — newest message first, each
    // message's in order: a block's names are in one message)
    let n = said.blocks.len();
    let (mut here, mut at): (Vec<bool>, Vec<Option<String>>) = (vec![false; n], vec![None; n]);
    for (name, l, blocks) in &said.named {
        let Some((p, _)) = place_of(name, *l) else { continue };
        for b in blocks.clone().filter(|&b| b < n) {
            if *p == path && *l == line {
                here[b] = true;
            } else if at[b].is_none() {
                at[b] = Some(format!("{}:{}", shown(p), l + 1));
            }
        }
    }
    let parts = flow(&said.blocks, said.last, &here, &at);
    // (only where it says something no place takes; that line's own place notes are it)
    let Some(first) = parts.iter().find(|p| matches!(p, Part::Whole(_))) else { return placed };
    let first = first_sentence(&first.block().text);
    let mine = |p: &Placed| p.path == path && p.said == line;
    let k = placed.iter().position(mine);
    let (lines, sentence, name) = k.map_or((line..line + 1, first, "".into()), |k| {
        (placed[k].lines.clone(), placed[k].sentence.clone(), placed[k].name.clone())
    });
    let paragraph = flow_text(&parts);
    let note = Placed { path: path.clone(), lines, sentence, paragraph, flow: parts, name, said: line };
    match k {
        // (in that note's stead: the notes keep the answer's order)
        Some(k) => {
            placed[k] = note;
            let mut i = 0;
            placed.retain(|p| {
                i += 1;
                i - 1 == k || !mine(p)
            });
        }
        None => placed.push(note),
    }
    placed
}

/// Every event: the answer's places, once every name is answered for the files as they are, are its notes, and what
/// none of them says one more at the turn's end place — kept with the answers before it, its first place's card
/// opened — or, none a file and no stop, nothing changes.
pub fn sync(ed: &mut Editor) {
    crate::notes::opening_sync(ed);
    kept_sync(ed);
    if ed.workspace.claude.notes.answer.is_none() {
        return;
    }
    let Some(c) = ed.workspace.claude.chat.as_ref() else {
        ed.workspace.claude.notes.answer = None;
        return;
    };
    let said = ed.workspace.claude.notes.answer.as_ref().expect("checked");
    // (its names being found: their answers wait to land — asked here meanwhile, each event would put a name in flight
    // back on the list, one search more after it lands, and a settle with no frame never ends. Only its own: another
    // name waiting — one `follow::answer_place` asked as it looked — holds none of them)
    if said.places.iter().any(|s| c.places.pending(&s.name)) {
        return;
    }
    let (mut found, mut unknown): (Vec<(PathBuf, &Said)>, bool) = (Vec::new(), false);
    for s in &said.places {
        match c.places.place(&s.name) {
            None => unknown = true,
            Some(Some(p)) => found.push((p, s)),
            Some(None) => {}
        }
    }
    // (one not answered for the files as they are — its answer dropped as they changed: asked now, after a frame)
    if unknown {
        ed.unchanged = false;
        return;
    }
    let roots: Vec<PathBuf> = ed.workspace.roots().all().map(|r| r.to_path_buf()).collect();
    let notes = &ed.workspace.claude.notes;
    let own: Vec<(&PathBuf, &Range<usize>)> =
        notes.list.iter().filter(|n| said.own.contains(&n.id)).map(|n| (&n.path, &n.lines)).collect();
    let inside = |p: &PathBuf| roots.iter().any(|r| crate::paths::inside(r, p).is_some());
    let found: Vec<(PathBuf, &Said)> = found.into_iter().filter(|(p, _)| inside(p)).collect();
    let place_of = |n: &Arc<str>, l: usize| found.iter().find(|(_, s)| s.name == *n && s.lines.start == l);
    // The answer's flow at its place: the first place named in its newest message that came to a file here, else its
    // last stop — none where the turn wrote at a note (its answer is there)
    let end = (said.named.iter())
        .find_map(|(n, l, _)| Some((place_of(n, *l)?.0.clone(), *l)))
        .or_else(|| said.stops.iter().find(|(p, _)| inside(p)).cloned())
        .filter(|_| !said.noted);
    let root = ed.workspace.root.clone();
    let placed = placed(said, &found, &own, end.clone(), &|p| crate::chat::shown(&root, p));
    let Some(w) = ed.workspace.claude.notes.answer.take() else { return };
    if placed.is_empty() {
        // (no name came to a file here and no stop held its words: the answer on no card — the reader opens by itself;
        // an older answer's search landing after a newer turn opens nothing)
        let users = ed
            .workspace
            .claude
            .chat
            .as_ref()
            .map_or(0, |c| c.msgs.iter().filter(|m| m.role == Role::User).count());
        if !w.noted && w.blocks.len() > w.last && w.turn + 1 == users {
            crate::chat::read_by_itself(ed);
        }
        return;
    }
    let notes = &mut ed.workspace.claude.notes;
    notes.answers += 1;
    let (n, at) = (notes.answers, crate::clock::system().duration_since(std::time::UNIX_EPOCH));
    let at = at.map_or(0, |d| d.as_secs());
    // (its first place's card opens: where follow takes the view — the flow's place, else the first)
    let mut first = None;
    for p in placed {
        // (an open buffer's line; else the store reads it from the file as it keeps them — never a key-named file's)
        let text = (ed.docs.iter())
            .find(|d| d.path.as_deref() == Some(p.path.as_path()))
            .filter(|d| p.lines.start < d.text.len_lines())
            .filter(|_| !crate::sandbox::key_named(&ed.workspace.root, &p.path))
            .map(|d| d.text.line(p.lines.start).to_string().trim().to_string());
        let lead = end.as_ref().is_some_and(|(e, l)| *e == p.path && *l == p.said);
        // (a place the answer named: a flow at a stop names none, and claims nothing of it)
        let doc = crate::docpaths::find(ed, &p.path);
        let check = w.judge.judged(&p.path, &p.lines, doc).filter(|_| !p.name.is_empty());
        let origin = Origin { n, at, turn: w.turn, name: p.name, said: p.said, text };
        let id =
            crate::notes::add_said(ed, &p.path, p.lines, (p.sentence, p.paragraph), p.flow, w.asked.clone());
        let note = crate::notes::find_mut(ed, id).expect("just added");
        (note.answer, note.check) = (Some(origin), check);
        if first.is_none() || lead && !first.is_some_and(|f: (u64, bool)| f.1) {
            first = Some((id, lead));
        }
    }
    crate::notes::attach(ed);
    if let Some((id, _)) = first {
        crate::notes::open_by_turn(ed, id);
    }
    // (kept with its conversation now — the turn's own keep went before its places were known)
    kept_sync(ed);
    ed.unchanged = false;
}

/// Every event: the conversation kept again when its answers' notes changed — one placed, closed, or turned a thread by
/// a reply — so what it keeps is what's there, however it's left next: another conversation, `C-l`, a plain quit (none
/// of which keeps it), the chat closed too (`chat::keep_places`). Never a note closed coming back on a continue, nor one
/// beside its own thread.
pub fn kept_sync(ed: &mut Editor) {
    let notes = &mut ed.workspace.claude.notes;
    let bare = notes.list.iter().filter(|n| crate::notes::bare_chat(n)).count();
    if bare != std::mem::replace(&mut notes.kept_bare, bare) {
        crate::chat::keep_places(ed);
    }
}

/// The conversation's answers' places as it keeps them (focus A1b, `chats::Saved.places`): each answer by its question
/// counted back from the newest of `msgs`' messages of yours, each place's name and line as written, its path, its
/// lines now and its line's text, and which of them holds its flow (`flow`: its path and line as written — null where
/// the turn had none, a turn that wrote at a note, or that note is gone: a thread now, or closed) — places only, the
/// words are the conversation's. None: no answer placed any.
pub fn places_json(notes: &crate::notes::Notes, msgs: &[Msg]) -> Option<Value> {
    places_of(notes, msgs.iter().filter(|m| m.role == Role::User).count())
}

/// `places_json` of a conversation with `users` messages of yours — what's left of it once the chat closed.
pub fn places_of(notes: &crate::notes::Notes, users: usize) -> Option<Value> {
    let mut answers: Vec<(u64, Value)> = Vec::new();
    for n in notes.list.iter().filter(|n| crate::notes::bare_chat(n)) {
        let Some(o) = &n.answer else { continue };
        let Some(back) = users.checked_sub(o.turn + 1) else { continue };
        // (a path that's no text — under a folder named so, `json!` panics on it — is one no store holds)
        let Some(path) = n.path.to_str() else { continue };
        let place = json!({
            "name": &*o.name, "said": o.said, "path": path, "lines": [n.lines.start, n.lines.end], "text": o.text,
        });
        let a = match answers.iter().position(|(k, _)| *k == o.n) {
            Some(i) => &mut answers[i].1,
            None => {
                let a = json!({ "back": back, "asked": n.asked, "at": o.at, "places": [], "flow": null });
                answers.push((o.n, a));
                &mut answers.last_mut().expect("just pushed").1
            }
        };
        a["places"].as_array_mut().expect("made so").push(place);
        if !n.flow.is_empty() {
            a["flow"] = json!([path, o.said]);
        }
    }
    crate::sorting::by_key(&mut answers, |a| a.0);
    (!answers.is_empty()).then(|| Value::Array(answers.into_iter().map(|a| a.1).collect()))
}

/// A place as the conversation kept it (`places_json`): its name and line as written, its path, its lines, its line's
/// text.
type Kept = (Arc<str>, usize, PathBuf, Range<usize>, Option<String>);

/// A conversation continued (`chat::continue_from`: `--resume` after `:restart`, `C-p`): the chat notes of the one
/// before go and this one's answers' places come back (`places_json`), their words cut again from its rows as they
/// were at its turn's end — no card opened (no turn ended). A place whose file is gone, its words no longer give, or
/// whose question isn't in the rows kept, is left out, said once in `:messages`.
pub fn restore(ed: &mut Editor, places: Option<&Value>) {
    crate::notes::forget_answers(ed);
    let Some(answers) = places.and_then(Value::as_array) else { return };
    let Some(c) = ed.workspace.claude.chat.as_ref() else { return };
    let users: Vec<usize> =
        (c.msgs.iter().enumerate()).filter(|(_, m)| m.role == Role::User).map(|p| p.0).collect();
    let root = ed.workspace.root.clone();
    let (mut back, mut lost) = (Vec::new(), 0);
    for (k, a) in answers.iter().enumerate() {
        let stored = a["places"].as_array().map_or(&[][..], |p| &p[..]);
        let at = |i: usize| users.get(i).copied();
        let turn = (a["back"].as_u64()).and_then(|b| users.len().checked_sub(b as usize + 1));
        let Some((turn, i)) = turn.and_then(|t| Some((t, at(t)?))) else {
            lost += stored.len();
            continue;
        };
        let end = at(turn + 1).unwrap_or(c.msgs.len());
        let mut said = read_answer(&c.msgs[i + 1..end]);
        said.asked = asked(&c.msgs[..i + 1]);
        if said.asked.as_deref() != a["asked"].as_str() {
            lost += stored.len();
            continue;
        }
        // (each place's name as written and its line: the file it came to then)
        let kept: Vec<Kept> = stored
            .iter()
            // (a gone file's: `chats::load` looked)
            .filter(|p| p["gone"] != true)
            .filter_map(|p| {
                let lines = p["lines"].as_array()?;
                let (a, b) = (lines.first()?.as_u64()? as usize, lines.get(1)?.as_u64()? as usize);
                let text = p["text"].as_str().map(str::to_string);
                let name: Arc<str> = p["name"].as_str()?.into();
                Some((
                    name,
                    p["said"].as_u64()? as usize,
                    PathBuf::from(p["path"].as_str()?),
                    a..b.max(a + 1),
                    text,
                ))
            })
            .collect();
        lost += stored.len() - kept.len();
        let found: Vec<(PathBuf, &Said)> = (said.places.iter())
            .filter_map(|s| {
                let k = kept.iter().find(|k| k.0 == s.name && k.1 == s.lines.start)?;
                Some((k.2.clone(), s))
            })
            .collect();
        // (the flow's place as kept — never worked out again: a turn that wrote at a note had none, and one whose note
        // went has none to move elsewhere)
        let flow = &a["flow"];
        let end = (flow[0].as_str()).zip(flow[1].as_u64()).map(|(p, l)| (PathBuf::from(p), l as usize));
        let n = k as u64 + 1;
        let when = a["at"].as_u64().unwrap_or(0);
        let placed = placed(&said, &found, &[], end, &|p| crate::chat::shown(&root, p));
        for k in &kept {
            let Some(p) = placed.iter().find(|p| p.path == k.2 && p.said == k.1) else {
                lost += 1;
                continue;
            };
            let origin = Origin { n, at: when, turn, name: k.0.clone(), said: k.1, text: k.4.clone() };
            let words = (p.sentence.clone(), p.paragraph.clone(), p.flow.clone());
            back.push((k.2.clone(), k.3.clone(), words, said.asked.clone(), origin));
        }
    }
    for (path, lines, (sentence, paragraph, flow), asked, origin) in back {
        let anchor = origin.text.clone();
        let id = crate::notes::add_said(ed, &path, lines, (sentence, paragraph), flow, asked);
        let notes = &mut ed.workspace.claude.notes;
        notes.answers = notes.answers.max(origin.n);
        let note = crate::notes::find_mut(ed, id).expect("just added");
        (note.answer, note.anchor) = (Some(origin), anchor);
    }
    crate::notes::attach(ed);
    // (as kept: a continue changes nothing to keep)
    let notes = &mut ed.workspace.claude.notes;
    notes.kept_bare = notes.list.iter().filter(|n| crate::notes::bare_chat(n)).count();
    if lost > 0 {
        let s = if lost == 1 { "" } else { "s" };
        let said = format!("{lost} place{s} of the conversation's answers no longer found");
        ed.recorded(crate::editor::ToastKind::Info, said);
    }
    ed.unchanged = false;
}

#[cfg(test)]
mod tests;
