//! A name in the chat answers the code's language keys (#389, `.sb/chatlink-plan.md` §2 — step 4, by place): on a
//! `path:line` an answer cites, in a code block that quotes a file, or backticked in a paragraph that cites places, a
//! key on the transcript (`gd` `gD` `gy` `gi` `gr`, ctrl+press, `K`) asks the language server at that spot in the file,
//! exact as in the code itself; `K` on a cited place shows its lines. What each message's blocks quote is found on a
//! worker after the frame that drew it (`after_frame`, one search at a time, as `places.rs` finds its names): a key only
//! looks up what's held (`target`) and asks. Every miss is said where you look; nothing reaches Claude (0 tokens).

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use crate::chat::{Chat, Role};
use crate::chatdoc::Of;
use crate::document::DocId;
use crate::editor::Editor;
use crate::lsp::ClientId;
use crate::lsp_editor::{Goto, Kind};

/// The lines read around a cited line, each side — a name in prose is looked for there (§2.1-3).
const NEAR: usize = 3;
/// A cited range's lines read at most (`K`'s peek of `a.rs:120-140`).
const RANGE_MAX: usize = 40;
/// How far from a cited line a code block's first line may be when no paragraph just before it cites its file (§2.1-2).
const WINDOW: usize = 20;
/// A quoted line looked for this far past the last one found (after an elision, `SKIP_MAX`).
const STEP_MAX: usize = 3;
const SKIP_MAX: usize = 2000;
/// A file past this size isn't read for a map.
const FILE_MAX: u64 = 4 << 20;
/// A busy server asked again at most this often for one question.
const TRIES_MAX: u8 = 20;

/// A message's markdown as the transcript draws it: prose (a paragraph, a heading, an item, a table row) and fenced code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// Its text as written, code spans' backticks kept.
    Prose(String),
    /// Its language and lines.
    Code { lang: String, lines: Vec<String> },
}

/// `text`'s parts, in order (a fence still open — an answer being written — is code to its end).
pub(crate) fn parts(text: &str) -> Vec<Part> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    let mut fence: Option<(&str, String, Vec<String>)> = None;
    let flush = |cur: &mut Option<String>, out: &mut Vec<Part>| {
        if let Some(p) = cur.take().filter(|p| !p.trim().is_empty()) {
            out.push(Part::Prose(p));
        }
    };
    for line in text.lines() {
        let t = line.trim_start();
        if let Some((f, ..)) = fence {
            if t.starts_with(f) && t[f.len()..].trim().is_empty() {
                let (_, lang, lines) = fence.take().expect("in a fence");
                out.push(Part::Code { lang, lines });
            } else if let Some((.., lines)) = fence.as_mut() {
                lines.push(line.to_string());
            }
            continue;
        }
        if let Some(f) = ["```", "~~~"].into_iter().find(|f| t.starts_with(f)) {
            flush(&mut cur, &mut out);
            let lang = t[f.len()..].split_whitespace().next().unwrap_or_default().to_string();
            fence = Some((f, lang, Vec::new()));
            continue;
        }
        if t.is_empty() {
            flush(&mut cur, &mut out);
            continue;
        }
        // (a heading and a table row are parts of their own; a list item starts one)
        if t.starts_with('#') || t.starts_with('|') {
            flush(&mut cur, &mut out);
            out.push(Part::Prose(t.to_string()));
            continue;
        }
        let digits = t.bytes().take_while(u8::is_ascii_digit).count();
        let item = ["- ", "* ", "+ "].iter().any(|m| t.starts_with(m))
            || (digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")));
        match cur.as_mut() {
            Some(c) if !item => {
                c.push(' ');
                c.push_str(t.trim_end());
            }
            _ => {
                flush(&mut cur, &mut out);
                cur = Some(t.trim_end().to_string());
            }
        }
    }
    flush(&mut cur, &mut out);
    if let Some((_, lang, lines)) = fence {
        out.push(Part::Code { lang, lines });
    }
    out
}

/// A line as the transcript compares it: tabs as spaces (`chatdoc::body`), the blanks at its ends off.
fn norm(s: &str) -> String {
    s.replace('\t', " ").trim().to_string()
}

/// The places `text` cites: the name as written, its line (from 0) and a range's last (`a.rs:120-140`).
fn cited(text: &str) -> Vec<(Arc<str>, usize, usize)> {
    let found = crate::places::names_in(text).into_iter();
    found
        .map(|(whole, path, line)| {
            let rest = &text[whole.end..];
            let digits =
                rest.strip_prefix('-').map_or(0, |r| r.bytes().take_while(u8::is_ascii_digit).count());
            let end = match digits {
                0 => line,
                n => rest[1..1 + n].parse::<usize>().map_or(line, |e| e.saturating_sub(1).max(line)),
            };
            (Arc::from(&text[path]), line, end.min(line + RANGE_MAX))
        })
        .collect()
}

/// The word (letters, digits, `_`) at byte `at` of `line`, or just before it.
fn word_at(line: &str, at: usize) -> Option<Range<usize>> {
    let w = |c: char| c.is_alphanumeric() || c == '_';
    let at = at.min(line.len());
    let on = line[at..].chars().next().is_some_and(w);
    let at = match on {
        true => at,
        false => line[..at].char_indices().next_back().filter(|(_, c)| w(*c)).map(|(i, _)| i)?,
    };
    let start = line[..at].char_indices().rev().find(|&(_, c)| !w(c)).map_or(0, |(i, c)| i + c.len_utf8());
    let end = line[at..].find(|c: char| !w(c)).map_or(line.len(), |i| at + i);
    (start < end).then_some(start..end)
}

/// Where `word` stands whole in `line`: each start.
fn whole_in(line: &str, word: &str) -> Vec<usize> {
    let w = |c: char| c.is_alphanumeric() || c == '_';
    line.match_indices(word)
        .filter(|(i, _)| {
            !line[..*i].chars().next_back().is_some_and(w)
                && !line[i + word.len()..].chars().next().is_some_and(w)
        })
        .map(|(i, _)| i)
        .collect()
}

/// `word` is written as code in `text`'s prose: whole inside one of its code spans.
fn backticked(parts: &[Part], word: &str) -> bool {
    parts.iter().any(|p| match p {
        Part::Prose(t) => t.split('`').skip(1).step_by(2).any(|span| !whole_in(span, word).is_empty()),
        Part::Code { .. } => false,
    })
}

// ── The maps, found after the frame ───────────────────────────────────────────────────────────────────────────

/// What a message's places came to in their files, read on a worker: each code block (in order) the file it quotes and
/// each of its lines' line there; each cited place the lines around it. Made for the message as it was (`key`).
#[derive(Clone, Debug, Default)]
pub struct Map {
    key: (u64, usize),
    quoted: Vec<Option<Quoted>>,
    near: Vec<Near>,
    /// The open buffers it read, at their versions then — one edited since, it's made again after the next frame.
    versions: Vec<(PathBuf, u64)>,
}

/// A code block that quotes a file: the file, and each of the block's lines' line there with its text as read.
#[derive(Clone, Debug)]
struct Quoted {
    path: PathBuf,
    at: Vec<Option<(usize, String)>>,
}

/// A cited place's lines as read: from `first`, its own (`line` to `end`) with `NEAR` each side.
#[derive(Clone, Debug)]
struct Near {
    name: Arc<str>,
    line: usize,
    end: usize,
    path: PathBuf,
    first: usize,
    lines: Vec<String>,
}

/// A question a key asked of a server: its number, where (`doc`, `byte`), what about, the transcript's cursor as it was
/// asked (moved since: the answer's dropped, as the code's `gd`'s), and how often it was put off for a busy server.
#[derive(Clone, Debug)]
struct Asked {
    n: u64,
    cid: ClientId,
    doc: DocId,
    byte: usize,
    goto: Option<Goto>,
    word: String,
    head: usize,
    again: bool,
    tries: u8,
}

/// The chat's links (`Chat.links`): each message's map, the files' listing they were made for, the search under way,
/// and the last question asked.
#[derive(Debug, Default)]
pub struct Links {
    maps: HashMap<usize, Map>,
    listing: u64,
    generation: u64,
    asking: bool,
    asked: Option<Asked>,
}

/// A message's key: its version and its text's length (an append the version didn't see).
fn key_of(m: &crate::chat::Msg) -> (u64, usize) {
    (m.version(), m.text.len())
}

fn generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A message to map: its index and key, its parts, and each cited place that came to a file (its part, name, lines, file).
struct Want {
    msg: usize,
    key: (u64, usize),
    parts: Vec<Part>,
    places: Vec<(usize, Arc<str>, usize, usize, PathBuf)>,
}

/// After each frame (`term::render_drawing`, before `places::after_frame` asks the names it wants): the messages it
/// drew with no map for them as they are, mapped on a worker — one search at a time. A message whose places aren't
/// answered yet waits for a frame that has them; one that cites none is mapped empty here. The checkout's files
/// changed (another `listing`): every map made again.
pub fn after_frame(ed: &mut Editor) {
    let listing = ed.workspace.listing;
    let docs = &ed.docs;
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    if c.links.listing != listing || c.links.generation == 0 {
        c.links.maps.clear();
        (c.links.listing, c.links.generation, c.links.asking) = (listing, generation(), false);
    }
    if c.links.asking {
        return;
    }
    // (a buffer a map read edited since: made again — its lines may have moved)
    let edited = |(p, v): &(PathBuf, u64)| docs.by_path(p).is_some_and(|d| d.version() != *v);
    c.links.maps.retain(|_, mp| !mp.versions.iter().any(edited));
    let laid = ed.screen.chat_laid.borrow();
    let writing = c.state == crate::chat::State::Writing;
    let mut want = Vec::new();
    for i in laid.iter().filter_map(|(of, _)| if let Of::Msg(i) = of { Some(*i) } else { None }) {
        let Some(m) = c.msgs.get(i).filter(|m| matches!(m.role, Role::User | Role::Assistant)) else {
            continue;
        };
        let key = key_of(m);
        if (writing && i + 1 == c.msgs.len()) || c.links.maps.get(&i).is_some_and(|mp| mp.key == key) {
            continue;
        }
        let parts = parts(&crate::chat::displayed(&m.text));
        let mut places = Vec::new();
        let mut answered = true;
        for (k, p) in parts.iter().enumerate() {
            let Part::Prose(t) = p else { continue };
            for (name, line, end) in cited(t) {
                match c.places.place(&name) {
                    None => answered = false,
                    Some(Some(path)) => places.push((k, name, line, end, path)),
                    Some(None) => {}
                }
            }
        }
        match (places.is_empty(), answered) {
            (_, false) => {}
            (true, true) => {
                c.links.maps.insert(i, Map { key, ..Map::default() });
            }
            (false, true) => want.push(Want { msg: i, key, parts, places }),
        }
    }
    drop(laid);
    if want.is_empty() {
        return;
    }
    c.links.asking = true;
    let generation = c.links.generation;
    let paths: Vec<&PathBuf> = want.iter().flat_map(|w| w.places.iter().map(|p| &p.4)).collect();
    let open: HashMap<PathBuf, (ropey::Rope, u64)> = (paths.iter())
        .filter_map(|p| ed.docs.by_path(p).map(|d| ((*p).clone(), (d.text.clone(), d.version()))))
        .collect();
    let root = ed.workspace.root.clone();
    ed.events.jobs().spawn(move || {
        let mut files = Files { open, read: HashMap::new(), root, used: Vec::new() };
        let maps: Vec<(usize, Map)> = want.into_iter().map(|w| (w.msg, map_of(&w, &mut files))).collect();
        move |ed: &mut Editor| landed(ed, generation, maps)
    });
}

/// A search's maps: kept when they're for the files as they are (another `listing` since: dropped, asked again). Nothing
/// on screen changes (a frame only for the next search, when one waits).
fn landed(ed: &mut Editor, generation: u64, maps: Vec<(usize, Map)>) {
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    if c.links.generation != generation {
        return;
    }
    c.links.asking = false;
    c.links.maps.extend(maps);
    ed.unchanged = true;
}

/// The files a search reads: an open buffer's text, else the disk's — never a file named like a key, nor a huge one.
struct Files {
    open: HashMap<PathBuf, (ropey::Rope, u64)>,
    read: HashMap<PathBuf, Option<Arc<Vec<String>>>>,
    root: PathBuf,
    /// The open buffers the message being mapped read, at their versions.
    used: Vec<(PathBuf, u64)>,
}

impl Files {
    fn lines(&mut self, path: &Path) -> Option<Arc<Vec<String>>> {
        if let Some((_, v)) = self.open.get(path)
            && !self.used.iter().any(|u| u.0 == path)
        {
            self.used.push((path.to_path_buf(), *v));
        }
        if let Some(l) = self.read.get(path) {
            return l.clone();
        }
        let text = match self.open.get(path) {
            Some((rope, _)) => Some(rope.to_string()),
            None if crate::sandbox::key_named(&self.root, path) => None,
            None if std::fs::metadata(path).is_ok_and(|m| m.len() > FILE_MAX) => None,
            None => std::fs::read_to_string(path).ok(),
        };
        let lines = text.map(|t| Arc::new(t.lines().map(|l| l.trim_end_matches('\r').to_string()).collect()));
        self.read.insert(path.to_path_buf(), lines.clone());
        lines
    }
}

/// One message's map (on a worker).
fn map_of(w: &Want, files: &mut Files) -> Map {
    files.used.clear();
    let mut near = Vec::new();
    for (_, name, line, end, path) in &w.places {
        let Some(lines) = files.lines(path) else { continue };
        let first = line.saturating_sub(NEAR);
        let last = (end + NEAR + 1).min(lines.len());
        let got = lines.get(first..last).map(<[String]>::to_vec).unwrap_or_default();
        near.push(Near { name: name.clone(), line: *line, end: *end, path: path.clone(), first, lines: got });
    }
    let mut quoted = Vec::new();
    for (k, p) in w.parts.iter().enumerate() {
        let Part::Code { lang, lines: code } = p else { continue };
        // (a paragraph just before it citing a file: the block anywhere in it, nearest the line cited; else near any
        // place the message cites)
        let before: Vec<_> = w.places.iter().filter(|p| k > 0 && p.0 == k - 1).collect();
        let others = w.places.iter().filter(|p| !(k > 0 && p.0 == k - 1));
        let tries = before.iter().map(|p| (*p, None)).chain(others.map(|p| (p, Some(WINDOW))));
        let diff = lang == "diff";
        let found = tries.into_iter().find_map(|((_, _, line, _, path), window)| {
            let lines = files.lines(path)?;
            let at = map_block(&lines, code, *line, window, diff)?;
            let at = at.into_iter().map(|a| a.map(|l| (l, lines[l].clone()))).collect();
            Some(Quoted { path: path.clone(), at })
        });
        quoted.push(found);
    }
    Map { key: w.key, quoted, near, versions: std::mem::take(&mut files.used) }
}

/// A quoted line as a block has it.
#[derive(Debug, PartialEq, Eq)]
enum Quote {
    Line(String),
    Blank,
    /// `…`, `// …`, `...` — lines left out: the next line is looked for further on.
    Elided,
    /// A diff's removed line: not in the file now.
    Gone,
}

fn quote_of(line: &str, diff: bool) -> Quote {
    let body = match (diff, line.chars().next()) {
        (true, Some('-')) => return Quote::Gone,
        (true, Some('@')) => return Quote::Elided,
        (true, Some('+' | ' ')) => &line[1..],
        _ => line,
    };
    let n = norm(body);
    if n.is_empty() {
        return Quote::Blank;
    }
    let bare = n.trim_start_matches(|c: char| "/#*-;<!".contains(c) || c == ' ');
    match bare.starts_with('…') || bare.starts_with("...") {
        true => Quote::Elided,
        false => Quote::Line(n),
    }
}

/// Where code block `code`'s lines are in `file`: its first line with text found (nearest `near`, within `window` of it
/// when given), each next one at most `STEP_MAX` lines on (`SKIP_MAX` past an elision), in order, by their text without
/// the blanks at their ends. None: its first line isn't there.
fn map_block(
    file: &[String],
    code: &[String],
    near: usize,
    window: Option<usize>,
    diff: bool,
) -> Option<Vec<Option<usize>>> {
    let quotes: Vec<Quote> = code.iter().map(|l| quote_of(l, diff)).collect();
    let (a, first) = quotes.iter().enumerate().find_map(|(i, q)| match q {
        Quote::Line(s) => Some((i, s)),
        _ => None,
    })?;
    let j = (0..file.len())
        .filter(|&j| window.is_none_or(|w| j.abs_diff(near) <= w))
        .filter(|&j| norm(&file[j]) == *first)
        .min_by_key(|&j| j.abs_diff(near))?;
    let mut at = vec![None; code.len()];
    at[a] = Some(j);
    let (mut pos, mut skip) = (j, false);
    for (k, q) in quotes.iter().enumerate().skip(a + 1) {
        match q {
            Quote::Blank => {
                if file.get(pos + 1).is_some_and(|l| l.trim().is_empty()) {
                    pos += 1;
                    at[k] = Some(pos);
                }
            }
            Quote::Elided => skip = true,
            Quote::Gone => {}
            Quote::Line(s) => {
                let reach = if skip { SKIP_MAX } else { STEP_MAX };
                if let Some(d) = (1..=reach).find(|d| file.get(pos + d).is_some_and(|l| norm(l) == *s)) {
                    pos += d;
                    at[k] = Some(pos);
                    skip = false;
                }
            }
        }
    }
    Some(at)
}

// ── A key on the transcript ─────────────────────────────────────────────────────────────────────────────────

/// A spot in a file a name in the chat stands for: the file, the line and the byte of the name in it, the name, and the
/// line as it was read (another there now: the file changed since — said, never asked at a guess).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Spot {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub word: String,
    pub text: String,
}

/// What the transcript's cursor is on (§2.1): a cited place (its file and line, and the lines read there for `K`'s peek)
/// or a name at a spot in a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Place { name: String, link: crate::chat::Link, peek: Option<Vec<String>> },
    At(Spot),
}

/// A language key on the chat's transcript at its cursor — `goto`, or the hover (None). Inside a key's command (its
/// document lent) the act waits until the documents are back (`docpane::swap`'s `after_swap`).
pub fn key(ed: &mut Editor, goto: Option<Goto>) {
    let t = target(ed);
    let act = move |ed: &mut Editor| act(ed, t, goto);
    match crate::docpane::swapped(ed) {
        Some(_) => ed.after_swap.push(Box::new(act)),
        None => act(ed),
    }
}

/// The front chat's transcript, its cursor's byte — the document lent to a key's command or kept in its pane.
fn transcript(ed: &Editor) -> Option<(&Chat, &crate::document::Document)> {
    let c = ed.workspace.claude.chat.as_ref()?;
    let t = c.transcript.as_ref()?;
    let d = match crate::docpane::swapped(ed) {
        Some(crate::docpane::Which::Chat) => ed.doc(),
        _ => t.pane.doc_in()?,
    };
    Some((c, d))
}

const NOT_CODE: &str = "not a name in the code";

/// What the transcript's cursor is on, from what's held (no disk, no request) — Err: why there's nothing to ask,
/// in words to say.
pub(crate) fn target(ed: &Editor) -> Result<Target, String> {
    let Some((c, d)) = transcript(ed) else { return Err(NOT_CODE.into()) };
    let t = c.transcript.as_ref().expect("a transcript, above");
    let head = d.selection().primary().cursor(&d.text);
    let line = d.text.byte_to_line(head);
    let Some((i, lines)) = t.msg_lines(line) else { return Err(NOT_CODE.into()) };
    let Some(m) = c.msgs.get(i).filter(|m| matches!(m.role, Role::User | Role::Assistant)) else {
        return Err(NOT_CODE.into());
    };
    let text_of = |l: usize| d.text.line(l).to_string().trim_end_matches(['\n', '\r']).to_string();
    let here = text_of(line);
    let at = head - d.text.line_to_byte(line);
    let map = c.links.maps.get(&i).filter(|mp| mp.key == key_of(m));
    const WAIT: &str = "still reading the files this answer cites — ask again in a moment";
    // A cited place: the file at its line
    let cites = cited(&here);
    let names = crate::places::names_in(&here);
    if let Some(k) = names.iter().position(|(whole, ..)| whole.start <= at && at < whole.end) {
        let (name, l) = (&cites[k].0, &cites[k].1);
        let Some(path) = c.places.link(&ed.workspace.root, name) else {
            return Err(match c.places.pending(name) {
                true => WAIT.into(),
                false => format!("{name} — no such file in the project or its folders"),
            });
        };
        let peek = map.and_then(|mp| mp.near.iter().find(|n| n.name == *name && n.line == *l)).map(|n| {
            let from = n.line.saturating_sub(n.first);
            let to = match n.end > n.line {
                true => (n.end + 1 - n.first).min(n.lines.len()),
                false => (from + NEAR + 1).min(n.lines.len()),
            };
            let from = if n.end > n.line { from } else { from.saturating_sub(NEAR) };
            n.lines[from.min(to)..to].to_vec()
        });
        let name = here[names[k].0.clone()].to_string();
        return Ok(Target::Place { name, link: crate::chat::Link { path, line: *l }, peek });
    }
    let Some(w) = word_at(&here, at) else { return Err(NOT_CODE.into()) };
    let word = here[w.clone()].to_string();
    if word.chars().all(|c| c.is_ascii_digit()) {
        return Err(NOT_CODE.into());
    }
    let parts = parts(&crate::chat::displayed(&m.text));
    // In a code block: its run of lines in the message's, found in order
    let drawn: Vec<String> = lines.clone().map(text_of).collect();
    let k = line - lines.start;
    let (mut from, mut b) = (0, 0);
    for (j, p) in parts.iter().enumerate() {
        let Part::Code { lines: code, .. } = p else { continue };
        if let Some(s) = run_of(&drawn, code, from) {
            if (s..s + code.len()).contains(&k) {
                let Some(map) = map else { return Err(WAIT.into()) };
                let Some(q) = map.quoted.get(b).and_then(Option::as_ref) else {
                    // (the paragraph before it cites a file: the lines aren't there, as quoted)
                    let before = match j.checked_sub(1).map(|i| &parts[i]) {
                        Some(Part::Prose(t)) => cited(t),
                        _ => Vec::new(),
                    };
                    let file = before
                        .iter()
                        .find_map(|(n, l, _)| map.near.iter().find(|x| x.name == *n && x.line == *l));
                    return Err(match file {
                        Some(n) => format!("these lines aren't in {} as quoted", shown(ed, &n.path)),
                        None => "this code block quotes no file the answer cites".into(),
                    });
                };
                let shown = shown(ed, &q.path);
                let Some((fl, raw)) = q.at.get(k - s).cloned().flatten() else {
                    return Err(format!("this line isn't in {shown} as quoted"));
                };
                // (the name's place in the line by characters from its text's start: the quote's indent may differ)
                let lead = here.len() - here.trim_start().len();
                let chars = here[lead.min(w.start)..w.start].chars().count();
                let raw_lead = raw.len() - raw.trim_start().len();
                let guess =
                    raw[raw_lead..].char_indices().nth(chars).map_or(raw.len(), |(i, _)| raw_lead + i);
                let col = whole_in(&raw, &word).into_iter().min_by_key(|c| c.abs_diff(guess));
                let Some(col) = col else { return Err(format!("this line isn't in {shown} as quoted")) };
                return Ok(Target::At(Spot { path: q.path.clone(), line: fl, col, word, text: raw }));
            }
            from = s + code.len();
        }
        b += 1;
    }
    // In prose: written as code, on the lines its paragraph cites
    if !backticked(&parts, &word) {
        return Err(NOT_CODE.into());
    }
    if cites.is_empty() {
        return Err(format!("this paragraph cites no file to find `{word}` in"));
    }
    let Some(map) = map else { return Err(WAIT.into()) };
    let mut best: Option<(usize, Spot)> = None;
    for (name, l, end) in &cites {
        let Some(n) = map.near.iter().find(|n| n.name == *name && n.line == *l) else { continue };
        for (j, text) in n.lines.iter().enumerate() {
            let fl = n.first + j;
            let off = if fl < *l { l - fl } else { fl.saturating_sub(*end) };
            if let Some(col) = whole_in(text, &word).first().copied()
                && best.as_ref().is_none_or(|(o, _)| off < *o)
            {
                let spot =
                    Spot { path: n.path.clone(), line: fl, col, word: word.clone(), text: text.clone() };
                best = Some((off, spot));
            }
        }
    }
    match best {
        Some((_, spot)) => Ok(Target::At(spot)),
        None => {
            let places: Vec<&str> = cites.iter().map(|c| &*c.0).collect();
            Err(format!("`{word}` isn't on the lines this paragraph cites ({})", places.join(", ")))
        }
    }
}

/// Where code block `code` is drawn among a message's `drawn` lines, from line `from` on: the line it starts at.
fn run_of(drawn: &[String], code: &[String], from: usize) -> Option<usize> {
    if code.is_empty() || code.len() > drawn.len() {
        return None;
    }
    (from..=drawn.len() - code.len()).find(|&s| code.iter().zip(&drawn[s..]).all(|(c, d)| norm(c) == norm(d)))
}

/// `path` as it's said: from the project's root.
fn shown(ed: &Editor, path: &Path) -> String {
    path.strip_prefix(&ed.workspace.root).unwrap_or(path).display().to_string()
}

/// What a key on the transcript comes to: a place gone to or peeked at, a question asked, or why not — said.
fn act(ed: &mut Editor, t: Result<Target, String>, goto: Option<Goto>) {
    match t {
        Err(why) => ed.set_status(why),
        Ok(Target::Place { link, .. }) if goto.is_some() => crate::chat::jump(ed, link),
        Ok(Target::Place { name, link, peek }) => match peek {
            Some(lines) if !lines.is_empty() => {
                let lang = crate::syntax::detect(&link.path).map(|l| l.name.clone());
                let md = format!("```{}\n{}\n```", lang.as_deref().unwrap_or(""), lines.join("\n"));
                show(ed, &md, lang.as_deref(), name);
            }
            _ => ed.set_status(format!("{name} — still reading its lines: ask again in a moment")),
        },
        Ok(Target::At(spot)) => ask(ed, spot, goto),
    }
}

/// `md` as `K`'s card, drawn in the chat at its cursor's name (`Editor.popup_chat`).
fn show(ed: &mut Editor, md: &str, lang: Option<&str>, name: String) {
    ed.popup = Some(crate::markdown::render(md, &ed.theme, lang));
    (ed.popup_name, ed.popup_scroll, ed.popup_chat) = (name, 0, true);
}

/// The question at `spot`: its file's buffer (opened aside), the line checked as it was read, the request sent.
fn ask(ed: &mut Editor, spot: Spot, goto: Option<Goto>) {
    let shown = shown(ed, &spot.path);
    let id = match crate::nav::buffer_for(ed, &spot.path) {
        Ok(id) => id,
        Err(e) => return ed.set_status(format!("{shown}: {e}")),
    };
    let Some(doc) = ed.docs.iter().find(|d| d.id == id) else { return };
    if doc.loading {
        return ed.set_status(format!("{shown} is still loading — ask again in a moment"));
    }
    let now = doc.text.get_line(spot.line).map(|l| l.to_string());
    if now.as_deref().map(|l| l.trim_end_matches(['\n', '\r'])) != Some(spot.text.as_str()) {
        // (read again after the next frame: a second press finds it where it is now, if it's still there)
        if let Some(c) = ed.workspace.claude.chat.as_mut() {
            c.links.maps.clear();
        }
        return ed
            .set_status(format!("{shown} changed since — `{}` isn't where the answer has it", spot.word));
    }
    let byte = doc.text.line_to_byte(spot.line) + spot.col;
    let head = transcript(ed).map_or(0, |(_, d)| d.selection().primary().cursor(&d.text));
    let n = next(ed);
    let asked = Asked { n, cid: 0, doc: id, byte, goto, word: spot.word, head, again: false, tries: 0 };
    send(ed, asked);
}

fn next(ed: &mut Editor) -> u64 {
    let n = ed.lsp.tool_ask_next;
    ed.lsp.tool_ask_next += 1;
    n
}

/// `a` sent (again, once its server's work ended): kept as the chat's question — or why it couldn't be, said.
fn send(ed: &mut Editor, mut a: Asked) {
    let cannot = match a.goto {
        None => "no language server here gives hover information".to_string(),
        Some(g) => format!("no language server here finds {}", g.noun()),
    };
    match ed.ask_at(a.doc, a.byte, Kind::ChatAt { n: a.n, goto: a.goto }, &cannot) {
        Err((why, _)) => {
            let file = ed.docs.index_of(a.doc).and_then(|i| ed.docs[i].path.clone());
            let shown = file.map(|p| shown(ed, &p)).unwrap_or_default();
            ed.set_status(format!("{shown}: {why}"));
        }
        Ok((cid, _)) => {
            (a.cid, a.again) = (cid, false);
            if let Some(c) = ed.workspace.claude.chat.as_mut() {
                c.links.asked = Some(a);
            }
        }
    }
}

/// A server's work ended: the chat's question it was busy with asked again.
pub fn work_ended(ed: &mut Editor, cid: ClientId) {
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    let Some(a) = c.links.asked.take_if(|a| a.again && a.cid == cid) else { return };
    send(ed, a);
}

/// Its server's answer to question `n` (Err: its error). Another asked since, or the transcript's cursor moved: dropped.
pub fn answered(ed: &mut Editor, cid: ClientId, n: u64, goto: Option<Goto>, result: Result<&Value, &Value>) {
    let head = transcript(ed).map(|(_, d)| d.selection().primary().cursor(&d.text));
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    let Some(a) = c.links.asked.take_if(|a| a.n == n && Some(a.head) == head) else { return };
    let server = ed.client(cid).map(|c| c.name.clone()).unwrap_or_default();
    let busy = ed.client(cid).is_some_and(|c| !c.progress.is_empty());
    let word = a.word.clone();
    let v = match result {
        Err(e) if matches!(e["code"].as_i64(), Some(-32801 | -32802)) => return wait(ed, a, &server),
        Err(e) => {
            return ed
                .set_status(format!("{server}: {}", e["message"].as_str().unwrap_or("the request failed")));
        }
        Ok(v) => v,
    };
    let Some(g) = goto else {
        let text = crate::lsp::hover_text(v);
        return match (text.is_empty(), busy) {
            (true, true) => wait(ed, a, &server),
            (true, false) => ed.set_status(format!("{server} knows nothing of `{word}`")),
            (false, _) => {
                let lang = ed
                    .docs
                    .iter()
                    .find(|d| d.id == a.doc)
                    .and_then(|d| d.syntax.as_ref())
                    .map(|s| s.lang.name.clone());
                show(ed, &text, lang.as_deref(), word);
            }
        };
    };
    let locs = crate::lsp::locations(v);
    match locs.as_slice() {
        [] if busy => wait(ed, a, &server),
        [] => ed.set_status(format!("no {} of `{word}` found", g.noun())),
        [(path, pos)] if g != Goto::References => {
            let enc = ed.encoding_of(cid);
            ed.lsp_jump(path, pos, enc);
        }
        _ => {
            let at = |pos: &Value, k: &str| pos[k].as_u64().unwrap_or(0) as usize;
            let found =
                locs.iter().map(|(p, pos)| (p.clone(), at(pos, "line"), at(pos, "character"))).collect();
            ed.refs_anew(word);
            ed.list_places(g, found);
        }
    }
}

/// The server answered empty while busy loading: asked again as its work ends (`work_ended`), said — at most
/// `TRIES_MAX` times, then given up, said.
fn wait(ed: &mut Editor, mut a: Asked, server: &str) {
    a.tries += 1;
    if a.tries > TRIES_MAX {
        return ed.set_status(format!("{server} is still loading the project — try again in a moment"));
    }
    let then = match a.goto {
        None => "showing it",
        Some(Goto::References) => "listing them",
        Some(_) => "going there",
    };
    ed.set_status(format!("{server} is loading — {then} when ready"));
    a.again = true;
    if let Some(c) = ed.workspace.claude.chat.as_mut() {
        c.links.asked = Some(a);
    }
}

/// The front chat's question in flight or waiting: its number, document and byte there, the name, the transcript's
/// cursor as it was asked, and what it asks (the simulation's check).
#[cfg(test)]
pub(crate) fn asked(ed: &Editor) -> Option<(u64, DocId, usize, String, usize, Option<Goto>)> {
    let a = ed.workspace.claude.chat.as_ref()?.links.asked.as_ref()?;
    Some((a.n, a.doc, a.byte, a.word.clone(), a.head, a.goto))
}

#[cfg(test)]
mod tests;
