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
//! - The last answer that names places has them: the next one that does replaces them, one whose names come to
//!   nothing leaves them. One you answered with `space n` is a thread — kept, as every note is.
//! - A line where the same answer's own `note` sits gets no chat note there (Claude said it there already).
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
}

/// An answer's places waiting for what their names come to (`Notes.answer`), and the notes its own `note` pinned.
#[derive(Clone, Debug, Default)]
pub struct Waiting {
    pub places: Vec<Said>,
    pub own: Vec<u64>,
}

/// The places `text` (Claude's answer as written) names, each with its sentence and paragraph — the first time each
/// place is named.
pub fn cut(text: &str) -> Vec<Said> {
    let blocks = blocks(text);
    let mut out: Vec<Said> = Vec::new();
    let push = |s: Said, out: &mut Vec<Said>| {
        if out.len() < PLACES_MAX && !out.iter().any(|o| o.name == s.name && o.lines.start == s.lines.start) {
            out.push(s);
        }
    };
    for (i, b) in blocks.iter().enumerate() {
        // (a walk-through's stop: its `why` the sentence, what follows it the paragraph — the rest of its block, else
        // the block after it)
        let stops = crate::chat::show_tags(b);
        let prose = without(b, stops.iter().map(|s| s.0.clone()));
        let prose = prose.trim();
        for (_, at, why) in &stops {
            let Some((whole, path, line)) = crate::places::names_in(at).into_iter().next() else { continue };
            let next = || blocks.get(i + 1).filter(|n| !n.contains("<show")).map(String::as_str);
            let after = Some(prose).filter(|p| !p.is_empty()).or_else(next);
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
            };
            push(said, &mut out);
        }
    }
    out
}

/// `text`'s blocks as they read: a paragraph's lines joined, a list item (its marker off, the lines under it
/// joined), a heading, a table row, a quote's text; fenced code left out.
fn blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    let mut fence: Option<&str> = None;
    let flush = |cur: &mut Option<String>, out: &mut Vec<String>| {
        if let Some(b) = cur.take().filter(|b| !b.trim().is_empty()) {
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
            out.push(t.trim_start_matches('#').trim().to_string());
            continue;
        }
        match item(t) {
            Some(body) => {
                flush(&mut cur, &mut out);
                cur = Some(body.to_string());
            }
            None => match cur.as_mut() {
                Some(c) => {
                    c.push(' ');
                    c.push_str(t.trim_end());
                }
                None => cur = Some(t.trim_end().to_string()),
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
/// come to (`sync`) — with the notes it pinned itself, whose lines take none. An answer that names none leaves the
/// last one's, and so do a turn cut short (a partial answer, its last sentence cut) and one that began with no
/// message of the user's (`Chat.turn_bare`: idle compaction's `/compact` — its answer isn't an answer, and the last
/// user message's would be the previous turn's, placed again).
pub fn turn_over(ed: &mut Editor, ok: bool) {
    let notes = &mut ed.workspace.claude.notes;
    let own = std::mem::take(&mut notes.pinned_now);
    let Some(c) = ed.workspace.claude.chat.as_ref().filter(|c| ok && !c.turn_bare) else { return };
    let turn = c.msgs.iter().rposition(|m| m.role == crate::chat::Role::User).map_or(0, |i| i + 1);
    let mut places: Vec<Said> = Vec::new();
    for m in c.msgs[turn..].iter().filter(|m| m.role == crate::chat::Role::Assistant) {
        for s in cut(&m.text) {
            if places.len() < PLACES_MAX
                && !places.iter().any(|p| p.name == s.name && p.lines.start == s.lines.start)
            {
                places.push(s);
            }
        }
    }
    if places.is_empty() {
        return;
    }
    // (asked after this frame — a turn's end draws one)
    for s in &places {
        let _ = c.places.place(&s.name);
    }
    ed.workspace.claude.notes.answer = Some(Waiting { places, own });
    ed.unchanged = false;
    sync(ed);
}

/// Every event: the answer's places, once every name is answered for the files as they are, are its notes — the
/// last answer's going — or, none a file, nothing changes.
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
    let placed: Vec<(PathBuf, Range<usize>, String, String)> = found
        .into_iter()
        .filter(|(p, _)| roots.iter().any(|r| crate::paths::inside(r, p).is_some()))
        .filter(|(p, s)| !own.iter().any(|(op, ol)| *op == p && ol.contains(&s.lines.start)))
        .map(|(p, s)| (p, s.lines.clone(), s.sentence.clone(), s.paragraph.clone()))
        .collect();
    ed.workspace.claude.notes.answer = None;
    if placed.is_empty() {
        return;
    }
    ed.workspace.claude.notes.list.retain(|n| !crate::notes::bare_chat(n));
    for (path, lines, sentence, paragraph) in placed {
        crate::notes::add_said(ed, &path, lines, sentence, paragraph);
    }
    crate::notes::attach(ed);
    ed.unchanged = false;
}

#[cfg(test)]
mod tests;
