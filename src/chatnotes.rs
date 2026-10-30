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
//! - The last answer that places a note has them: the next one that does replaces them, one with neither a place nor
//!   a stop leaves them. One you answered with `space n` is a thread — kept, as every note is.
//! - A line where the same answer's own `note` sits gets no chat note there (Claude said it there already); a turn that
//!   wrote at a note at all (a new one or a reply) gets no flow — its answer is at the code already.
//! - Never kept across a restart (they're the last answer's); a thread one is.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

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
}

/// A block of an answer as it reads (`blocks`): its text, and whether it's a list item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub text: String,
    pub item: bool,
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
            // (a stop that says no why: what follows it begins the line end)
            let first =
                |p: &str| sentences(p).first().map_or(String::new(), |s| p[s.clone()].trim().to_string());
            let sentence = if why.is_empty() { first(&paragraph) } else { why.clone() };
            let said = Said {
                name: Arc::from(&at[path]),
                lines: line..range_end(at, whole.end).max(line + 1),
                sentence: capped(&plain(&sentence), SENTENCE_MAX),
                paragraph: capped(&paragraph, PARAGRAPH_MAX),
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
                paragraph: capped(prose, PARAGRAPH_MAX),
                blocks: base + i..base + i + 1,
            };
            push(said, &mut out);
        }
    }
    out
}

/// `text`'s blocks as they read: a paragraph's lines joined, a list item (its marker off, the lines under it
/// joined), a heading, a table row, a quote's text; fenced code left out.
fn blocks(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut cur: Option<Block> = None;
    let mut fence: Option<&str> = None;
    let flush = |cur: &mut Option<Block>, out: &mut Vec<Block>| {
        if let Some(b) = cur.take().filter(|b| !b.text.trim().is_empty()) {
            out.push(b);
        }
    };
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(f) = fence {
            if t.starts_with(f) {
                fence = None;
            }
            continue;
        }
        if let Some(f) = ["```", "~~~"].into_iter().find(|f| t.starts_with(f)) {
            flush(&mut cur, &mut out);
            fence = Some(f);
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
            out.push(Block { text: t.trim_start_matches('#').trim().to_string(), item: false });
            continue;
        }
        match item(t) {
            Some(body) => {
                flush(&mut cur, &mut out);
                cur = Some(Block { text: body.to_string(), item: true });
            }
            None => match cur.as_mut() {
                Some(c) => {
                    c.text.push(' ');
                    c.text.push_str(t.trim_end());
                }
                None => cur = Some(Block { text: t.trim_end().to_string(), item: false }),
            },
        }
    }
    flush(&mut cur, &mut out);
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
    let turn = c.msgs.iter().rposition(|m| m.role == crate::chat::Role::User).map_or(0, |i| i + 1);
    let (mut places, mut all, mut named): (Vec<Said>, Vec<Block>, Vec<_>) = Default::default();
    let called = c.msgs[turn..].iter().rposition(|m| m.role == crate::chat::Role::Tool).map_or(0, |i| i + 1);
    let mut last = 0;
    for (i, m) in c.msgs[turn..].iter().enumerate().filter(|(_, m)| m.role == crate::chat::Role::Assistant) {
        let these = blocks(&m.text);
        let said = cut_blocks(&these, all.len());
        all.extend(these);
        // (said before a call: the turn's narration)
        if i < called {
            last = all.len();
        }
        // (the newest message's places first, each message's in order)
        named.splice(0..0, said.iter().map(|s| (s.name.clone(), s.lines.start, s.blocks.clone())));
        for s in said {
            if places.len() < PLACES_MAX
                && !places.iter().any(|p| p.name == s.name && p.lines.start == s.lines.start)
            {
                places.push(s);
            }
        }
    }
    let stops = stops(c);
    let asked =
        turn.checked_sub(1).and_then(|i| c.msgs[i].text.lines().map(str::trim).find(|l| !l.is_empty()));
    let asked = asked.map(str::to_string);
    if places.is_empty() && (noted || all.len() <= last || stops.is_empty()) {
        return;
    }
    // (asked after this frame — a turn's end draws one)
    for s in &places {
        let _ = c.places.place(&s.name);
    }
    ed.workspace.claude.notes.answer =
        Some(Waiting { places, own, noted, blocks: all, last, named, stops, asked });
    ed.unchanged = false;
    sync(ed);
}

/// Where the turn stopped in a file (this turn's stops, `replay`): its edits' first changed lines, newest first, then
/// its reads' first lines, newest first.
fn stops(c: &crate::chat::Chat) -> Vec<(PathBuf, usize)> {
    let stops = c.follow.replay.turns().back().map_or(&[][..], |t| &t.stops[..]);
    let at = |s: &crate::replay::Stop| Some((s.path.clone()?, s.lines.as_ref()?.start));
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
        let size =
            if at[k].is_some() && !here[k] { b.text.chars().count().min(60) } else { b.text.chars().count() };
        let cut = size > room;
        let b = Block { text: if cut { capped(&b.text, room) } else { b.text.clone() }, item: b.item };
        out.push(match (&at[k], here[k]) {
            (_, true) => Part::Here(b),
            (Some(p), false) => Part::Elsewhere(p.clone(), b),
            (None, false) => Part::Whole(b),
        });
        if cut {
            out.push(Part::Whole(Block { text: "The rest is in the chat.".into(), item: false }));
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

/// Every event: the answer's places, once every name is answered for the files as they are, are its notes, and what
/// none of them says one more at the turn's end place — the last answer's going — or, none a file and no stop,
/// nothing changes.
pub fn sync(ed: &mut Editor) {
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
    type Placed = (PathBuf, Range<usize>, String, String, Vec<Part>);
    let mut placed: Vec<Placed> = found
        .iter()
        .filter(|(p, s)| !own.iter().any(|(op, ol)| *op == p && ol.contains(&s.lines.start)))
        .map(|(p, s)| (p.clone(), s.lines.clone(), s.sentence.clone(), s.paragraph.clone(), Vec::new()))
        .collect();
    // The answer's flow at its place: the first place named in its newest message that came to a file here, else its
    // last stop — none where the turn wrote at a note (its answer is there)
    let end = (said.named.iter())
        .find_map(|(n, l, _)| Some((place_of(n, *l)?.0.clone(), *l)))
        .or_else(|| said.stops.iter().find(|(p, _)| inside(p)).cloned())
        .filter(|_| !said.noted);
    if let Some((path, line)) = end {
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
                    at[b] = Some(format!("{}:{}", crate::chat::shown(&ed.workspace.root, p), l + 1));
                }
            }
        }
        let parts = flow(&said.blocks, said.last, &here, &at);
        // (only where it says something no place takes; that line's own place notes are it)
        if let Some(first) = parts.iter().find(|p| matches!(p, Part::Whole(_))) {
            let first = first_sentence(&first.block().text);
            let mine = |(p, l, ..): &Placed| *p == path && l.start == line;
            let at = placed.iter().position(mine);
            let (lines, sentence) =
                at.map_or((line..line + 1, first), |k| (placed[k].1.clone(), placed[k].2.clone()));
            let note = (path.clone(), lines, sentence, flow_text(&parts), parts);
            match at {
                // (in that note's stead: the notes keep the answer's order)
                Some(k) => {
                    placed[k] = note;
                    placed = placed
                        .into_iter()
                        .enumerate()
                        .filter(|(i, p)| *i == k || !mine(p))
                        .map(|p| p.1)
                        .collect();
                }
                None => placed.push(note),
            }
        }
    }
    let asked = ed.workspace.claude.notes.answer.take().and_then(|w| w.asked);
    if placed.is_empty() {
        return;
    }
    ed.workspace.claude.notes.list.retain(|n| !crate::notes::bare_chat(n));
    for (path, lines, sentence, paragraph, flow) in placed {
        crate::notes::add_said(ed, &path, lines, (sentence, paragraph), flow, asked.clone());
    }
    crate::notes::attach(ed);
    ed.unchanged = false;
}

#[cfg(test)]
mod tests;
