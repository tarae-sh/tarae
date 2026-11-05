//! What an answer names of the code (#239, #388): the last answer's `path:line` places are flags at their lines
//! (`flags.rs`) — the chat keeps the whole answer, the code a `¶` leading to it. No tool call and no token: tarae reads
//! Claude's prose for them.
//!
//! - At a turn's end, its answer (the turn's prose, every message of it) is cut by its source text, never the rows it
//!   wrapped to (`cut`): blocks — a paragraph, a list item, a heading, a table row; fenced code is no prose — then a
//!   block's sentences. A walk-through's stop (`<show at=…>why</show>`) is its place, `why` its sentence. `a.rs:120-140`
//!   covers those lines.
//! - Where a name is goes by the chat's one answer (`places::Known::place`, the answers drawn): a name that comes to
//!   no file, or to several, is no place; one outside the project and its folders neither. Found on the worker the
//!   frame asks (`places::after_frame`), so the flags are placed the event its answers land (`sync`).
//! - Kept for the conversation (focus A1b): every answer's flags stay while its conversation does — a new one (`C-l`,
//!   `:chat-new`, `/clear`) clears them, a continue (`--resume` after `:restart`, `C-p`) brings them back (`restore`).
//!   Kept with the conversation (`places_json`): places only — the name as written, the path, the lines, the line's
//!   text —, the words cut again from its rows.
//! - An answer with words after its last call shows the column by itself at its turn's end, unfocused, never while you
//!   type (`chat::read_by_itself`, chatlink Q5) — unless you hid the column during that turn: then `◂ new` says it.
//! - A line where the same answer's own `note` sits gets no flag there (Claude said it there already).

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::chat::{Msg, Role};
use crate::editor::Editor;

/// Places an answer gives flags at most.
const PLACES_MAX: usize = 40;

/// The most of a sentence (the flag's hover) kept, in characters.
const SENTENCE_MAX: usize = 400;

/// A place an answer names, with what it says of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Said {
    /// The name as written (`src/a.rs`, `a.rs`).
    pub name: Arc<str>,
    /// Its lines, from 0, the end out.
    pub lines: Range<usize>,
    /// The sentence that names it.
    pub sentence: String,
}

/// An answer's places waiting for what their names come to (`Flags.answer`), and the notes its own `note` pinned.
#[derive(Clone, Debug, Default)]
pub struct Waiting {
    pub places: Vec<Said>,
    pub own: Vec<u64>,
    /// The question the answer answered: its first line that says something (`Flag.asked`).
    pub asked: Option<String>,
    /// Its question's place among the conversation's messages of yours, from 0 (`Origin.turn`).
    pub turn: usize,
    /// Its turn's stops, as it ended — what its places are judged by (`turnmap::Judge`, focus F0).
    pub judge: crate::turnmap::Judge,
}

/// Which answer a flag is of, and where in it (focus A1b): what the conversation keeps of it (`places_json`) and finds
/// again in its words on a continue (`restore`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    /// The answer's number in this chat, from 1 — the newest the highest.
    pub n: u64,
    /// When it was given, unix seconds.
    pub at: u64,
    /// Its question's place among the conversation's messages of yours, from 0.
    pub turn: usize,
    /// The place as written (`src/a.rs`) and its first line as written, from 0.
    pub name: Arc<str>,
    pub said: usize,
    /// Its line's text as placed — what finds it again once the file moved it.
    pub text: Option<String>,
}

/// The places `text` (Claude's answer as written) names, each with its sentence — the first time each place is named.
#[cfg(test)]
fn cut(text: &str) -> Vec<Said> {
    let mut out: Vec<Said> = Vec::new();
    for s in cut_blocks(&blocks(text)) {
        if out.len() < PLACES_MAX && !out.iter().any(|o| o.name == s.name && o.lines.start == s.lines.start) {
            out.push(s);
        }
    }
    out
}

/// Every place an answer's `blocks` name, each time it's named; the flags take each place's first (`read_answer`).
/// `PLACES_MAX` places at most, counted once each: a repeat of one kept is kept (the answer's length bounds them), a new
/// one past the cap left out.
fn cut_blocks(blocks: &[String]) -> Vec<Said> {
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
        // (a walk-through's stop: its `why` the sentence — one that says none, the first of what follows it: the rest of
        // its block, else the block after it)
        let stops = crate::chat::show_tags(b);
        let prose = without(b, stops.iter().map(|s| s.0.clone()));
        let prose = prose.trim();
        for (_, at, why) in &stops {
            let Some((whole, path, line)) = crate::places::names_in(at).into_iter().next() else { continue };
            let next = blocks.get(i + 1).map(String::as_str).filter(|n| !n.contains("<show"));
            let after = Some(prose).filter(|p| !p.is_empty()).or(next).unwrap_or_default();
            let first =
                |p: &str| sentences(p).first().map_or(String::new(), |s| p[s.clone()].trim().to_string());
            let sentence = if why.is_empty() { first(after) } else { why.clone() };
            let said = Said {
                name: Arc::from(&at[path]),
                lines: line..range_end(at, whole.end).max(line + 1),
                sentence: capped(&plain(&sentence), SENTENCE_MAX),
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
            };
            push(said, &mut out);
        }
    }
    out
}

/// `text`'s blocks as they read: a paragraph's lines joined, a list item (its marker off, the lines under it
/// joined), a heading, a table row, a quote's text; fenced code left out.
fn blocks(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
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

/// A sentence as a card shows it: markdown's marks off (`code`, **strong**).
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

/// A text as a card's row shows it: its words, markdown's marks off.
pub fn plain_words(text: &str) -> String {
    plain(text)
}

/// A chat turn ended (`ok`: whole, neither stopped nor failed): the places its answer names wait for what their names
/// come to (`sync`) — with the notes it pinned itself, whose lines take none. Words of its own after its last call show
/// the column by itself (chatlink Q5: the answer is the chat's alone now) — not one that wrote at a note (its answer is
/// there), nor where you hid the column during the turn. A turn cut short (a partial answer) and one that began with no
/// message of the user's (`Chat.turn_bare`: idle compaction's `/compact` — its answer isn't an answer, and the last user
/// message's would be the previous turn's, placed again) place nothing.
pub fn turn_over(ed: &mut Editor, ok: bool) {
    let notes = &mut ed.workspace.claude.notes;
    let own = std::mem::take(&mut notes.pinned_now);
    let noted = std::mem::take(&mut notes.noted_now);
    let Some(c) = ed.workspace.claude.chat.as_ref().filter(|c| ok && !c.turn_bare) else { return };
    let turn = c.msgs.iter().rposition(|m| m.role == Role::User).map_or(0, |i| i + 1);
    let (mut said, words) = read_answer(&c.msgs[turn..]);
    let users = c.msgs[..turn].iter().filter(|m| m.role == Role::User).count();
    let hid = c.hid_at == Some(users);
    if words && !noted && !hid {
        crate::chat::read_by_itself(ed);
    }
    let Some(c) = ed.workspace.claude.chat.as_ref() else { return };
    if said.places.is_empty() {
        return;
    }
    // (asked after this frame — a turn's end draws one)
    for s in &said.places {
        let _ = c.places.place(&s.name);
    }
    said.asked = asked(&c.msgs[..turn]);
    said.turn = users.saturating_sub(1);
    said.own = own;
    let turn = c.follow.replay.turns().back().map_or(&[][..], |t| &t.stops[..]);
    said.judge = crate::turnmap::Judge::of(turn, &ed.workspace.root, &c.edits.seen);
    ed.workspace.claude.flags.answer = Some(said);
    ed.unchanged = false;
    sync(ed);
}

/// The question a turn answered, from the messages up to its answer: its first line that says something.
fn asked(before: &[Msg]) -> Option<String> {
    let q = before.last().filter(|m| m.role == Role::User)?;
    q.text.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}

/// An answer as it reads, from `msgs` — one turn's, after its question: its places (each the first time it's named), and
/// whether it says anything after its last call (before one it's the turn's narration). What `turn_over` reads at its end
/// and `restore` again from the conversation's rows.
fn read_answer(msgs: &[Msg]) -> (Waiting, bool) {
    let mut w = Waiting::default();
    let called = msgs.iter().rposition(|m| m.role == Role::Tool).map_or(0, |i| i + 1);
    let mut words = false;
    for (i, m) in msgs.iter().enumerate().filter(|(_, m)| m.role == Role::Assistant) {
        let these = blocks(&m.text);
        words |= i >= called && !these.is_empty();
        for s in cut_blocks(&these) {
            if w.places.len() < PLACES_MAX
                && !w.places.iter().any(|p| p.name == s.name && p.lines.start == s.lines.start)
            {
                w.places.push(s);
            }
        }
    }
    (w, words)
}

/// Every event: the answer's places, once every name is answered for the files as they are, are its flags — kept with
/// the answers before it — or, none a file here, nothing changes.
pub fn sync(ed: &mut Editor) {
    kept_sync(ed);
    if ed.workspace.claude.flags.answer.is_none() {
        return;
    }
    let Some(c) = ed.workspace.claude.chat.as_ref() else {
        ed.workspace.claude.flags.answer = None;
        return;
    };
    let said = ed.workspace.claude.flags.answer.as_ref().expect("checked");
    // (its names being found: their answers wait to land — asked here meanwhile, each event would put a name in flight
    // back on the list, one search more after it lands, and a settle with no frame never ends. Only its own: another
    // name waiting — one `follow::answer_place` asked as it looked — holds none of them)
    if said.places.iter().any(|s| c.places.pending(&s.name)) {
        return;
    }
    // (a file made or removed since they were found: asked again after the next frame — never placed by an answer for
    // the files as they were)
    let roots: Vec<PathBuf> = ed.workspace.roots().all().map(|r| r.to_path_buf()).collect();
    if !c.places.is_for(ed.workspace.listing, &roots) {
        ed.unchanged = false;
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
    let notes = &ed.workspace.claude.notes;
    let own: Vec<(&PathBuf, &Range<usize>)> =
        notes.list.iter().filter(|n| said.own.contains(&n.id)).map(|n| (&n.path, &n.lines)).collect();
    let inside = |p: &PathBuf| roots.iter().any(|r| crate::paths::inside(r, p).is_some());
    let placed: Vec<(PathBuf, Said)> = (found.into_iter())
        .filter(|(p, s)| inside(p) && !own.iter().any(|(op, ol)| *op == p && ol.contains(&s.lines.start)))
        .map(|(p, s)| (p, s.clone()))
        .collect();
    let Some(w) = ed.workspace.claude.flags.answer.take() else { return };
    if placed.is_empty() {
        return;
    }
    let n = ed.workspace.claude.flags.answers + 1;
    let at = crate::clock::system().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    for (path, s) in placed {
        // (an open buffer's line; else the store reads it from the file as it keeps them — never a key-named file's)
        let text = (ed.docs.iter())
            .find(|d| d.path.as_deref() == Some(path.as_path()))
            .filter(|d| s.lines.start < d.text.len_lines())
            .filter(|_| !crate::sandbox::key_named(&ed.workspace.root, &path))
            .map(|d| d.text.line(s.lines.start).to_string().trim().to_string());
        let doc = crate::docpaths::find(ed, &path);
        let check = w.judge.judged(&path, &s.lines, doc);
        let origin = Origin { n, at, turn: w.turn, name: s.name, said: s.lines.start, text };
        crate::flags::add(ed, (&path, s.lines), origin, check, (w.asked.clone(), s.sentence));
    }
    crate::flags::attach(ed);
    // (kept with its conversation now — the turn's own keep went before its places were known)
    kept_sync(ed);
    ed.unchanged = false;
}

/// Every event: the conversation kept again when its answers' flags changed in number — placed, or gone — so what it
/// keeps is what's there, however it's left next: another conversation, `C-l`, a plain quit (none of which keeps it),
/// the chat closed too (`chat::keep_places`).
pub fn kept_sync(ed: &mut Editor) {
    let flags = &mut ed.workspace.claude.flags;
    let now = flags.list.len();
    if now != std::mem::replace(&mut flags.kept, now) {
        crate::chat::keep_places(ed);
    }
}

/// The conversation's answers' places as it keeps them (focus A1b, `chats::Saved.places`): each answer by its question
/// counted back from the newest of `msgs`' messages of yours, each place's name and line as written, its path, its
/// lines now and its line's text — places only, the words are the conversation's. None: no answer placed any.
pub fn places_json(flags: &crate::flags::Flags, msgs: &[Msg]) -> Option<Value> {
    places_of(flags, msgs.iter().filter(|m| m.role == Role::User).count())
}

/// `places_json` of a conversation with `users` messages of yours — what's left of it once the chat closed.
pub fn places_of(flags: &crate::flags::Flags, users: usize) -> Option<Value> {
    let mut answers: Vec<(u64, Value)> = Vec::new();
    for f in &flags.list {
        let o = &f.origin;
        let Some(back) = users.checked_sub(o.turn + 1) else { continue };
        // (a path that's no text — under a folder named so, `json!` panics on it — is one no store holds)
        let Some(path) = f.path.to_str() else { continue };
        let place = json!({
            "name": &*o.name, "said": o.said, "path": path, "lines": [f.lines.start, f.lines.end], "text": o.text,
        });
        let a = match answers.iter().position(|(k, _)| *k == o.n) {
            Some(i) => &mut answers[i].1,
            None => {
                answers.push((o.n, json!({ "back": back, "asked": f.asked, "at": o.at, "places": [] })));
                &mut answers.last_mut().expect("just pushed").1
            }
        };
        a["places"].as_array_mut().expect("made so").push(place);
    }
    crate::sorting::by_key(&mut answers, |a| a.0);
    (!answers.is_empty()).then(|| Value::Array(answers.into_iter().map(|a| a.1).collect()))
}

/// A place as the conversation kept it (`places_json`): its name and line as written, its path, its lines, its line's
/// text.
type Kept = (Arc<str>, usize, PathBuf, Range<usize>, Option<String>);

/// A conversation continued (`chat::continue_from`: `--resume` after `:restart`, `C-p`): the flags of the one before go
/// and this one's answers' places come back (`places_json`), their words cut again from its rows. A place whose file is
/// gone, its words no longer give, or whose question isn't in the rows kept, is left out, said once in `:messages`.
pub fn restore(ed: &mut Editor, places: Option<&Value>) {
    crate::flags::forget(ed);
    let Some(answers) = places.and_then(Value::as_array) else { return };
    let Some(c) = ed.workspace.claude.chat.as_ref() else { return };
    let users: Vec<usize> =
        (c.msgs.iter().enumerate()).filter(|(_, m)| m.role == Role::User).map(|p| p.0).collect();
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
        let (said, _) = read_answer(&c.msgs[i + 1..end]);
        let asked = asked(&c.msgs[..i + 1]);
        if asked.as_deref() != a["asked"].as_str() {
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
        let n = k as u64 + 1;
        let when = a["at"].as_u64().unwrap_or(0);
        for k in kept {
            // (its words as its answer says them now: one they no longer name is lost)
            let Some(s) = said.places.iter().find(|s| s.name == k.0 && s.lines.start == k.1) else {
                lost += 1;
                continue;
            };
            let origin = Origin { n, at: when, turn, name: k.0, said: k.1, text: k.4 };
            back.push((k.2, k.3, origin, (asked.clone(), s.sentence.clone())));
        }
    }
    for (path, lines, origin, words) in back {
        crate::flags::add(ed, (&path, lines), origin, None, words);
    }
    crate::flags::attach(ed);
    // (as kept: a continue changes nothing to keep)
    let flags = &mut ed.workspace.claude.flags;
    flags.kept = flags.list.len();
    if lost > 0 {
        let s = if lost == 1 { "" } else { "s" };
        let said = format!("{lost} place{s} of the conversation's answers no longer found");
        ed.recorded(crate::editor::ToastKind::Info, said);
    }
    ed.unchanged = false;
}

#[cfg(test)]
pub(crate) mod tests;
