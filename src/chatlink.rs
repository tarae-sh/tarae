//! A name in the chat answers the code's language keys (#389, `.sb/chatlink-plan.md` §2): on a `path:line` an answer
//! cites, in a code block that quotes a file, or backticked in a paragraph that cites places (step 4, by place), a key on
//! the transcript (`gd` `gD` `gy` `gi` `gr`, ctrl+press, `K`) asks the language server at that spot in the file, exact
//! as in the code itself; `K` on a cited place shows its lines. A backticked name no cited line holds that's a local of
//! the answer's own files, else of those its turn read (`locals_of`: the syntax tree's `locals.scm`), is asked at its
//! definition there. Any other name written as code — backticked in prose, or
//! in a block that quotes no file (step 5, by name) — is asked of the workspace (`workspace/symbol`, the exact name, a
//! qualified one's container first): one definition goes there, several are a list of places; the other keys ask at the
//! one found. What each message's blocks quote is found on a worker after the frame that drew it (`after_frame`, one
//! search at a time, as `places.rs` finds its names): a key only looks up what's held (`target`) and asks. Every miss is
//! said where you look; nothing reaches Claude (0 tokens).

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
/// An answer's own files looked through for a local at most — the ones it cites, quotes or its turn changed —, and the
/// files its turn only read parsed at most, those whose text holds the name (`locals_of`).
const OWN_MAX: usize = 8;
const READ_MAX: usize = 16;

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
    /// The definitions of each name its prose writes as code that are locals in the answer's own files, else in those
    /// its turn only read (`locals_of`): one, or several none of which the answer's focus picks.
    locals: HashMap<Arc<str>, Vec<Spot>>,
    /// The open buffers it read, at their versions then — one edited since, it's made again after the next frame.
    versions: Vec<(PathBuf, u64)>,
    /// A buffer it read edited since (or a key found a line moved): being made again, and read as it is until the new
    /// one lands — the answer's names drawn as they were, a key asking at its places (nearest still beats by name; a
    /// line that moved is said, `ask`, and the new map comes within the same wait).
    stale: bool,
    /// The files whose locals it read (the tests' check that a file read without the name isn't).
    #[cfg(test)]
    parsed: Vec<PathBuf>,
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

/// A name asked of the workspace (§2.1-4): its number, the name and the container a qualified one names, what the key
/// asks, what's said first (why no cited place held it), the transcript's cursor as it was asked, the servers yet to
/// answer and the names of all asked, what they found (the server, the file, the position, its container), those that
/// answered while loading (asked again as their work ends, `again`) and their errors.
#[derive(Clone, Debug)]
struct Named {
    n: u64,
    word: String,
    container: Option<String>,
    goto: Option<Goto>,
    why: Option<String>,
    head: usize,
    waiting: Vec<ClientId>,
    servers: Vec<String>,
    found: Vec<(ClientId, PathBuf, Value, String)>,
    busy: Vec<ClientId>,
    errors: Vec<String>,
    tries: u8,
    again: bool,
    /// The pointer's rest it's asked for (`hoverrest.rs`'s number): its answer the rest's card, a miss nothing said.
    rest: Option<u64>,
}

/// The chat's links (`Chat.links`): each message's map, the files' listing they were made for, the search under way,
/// and the last question asked — at a spot, or by name.
#[derive(Debug, Default)]
pub struct Links {
    maps: HashMap<usize, Map>,
    listing: u64,
    generation: u64,
    asking: bool,
    asked: Option<Asked>,
    named: Option<Named>,
    /// The pointer's rest's question by name, apart from a key's (neither takes the other's place).
    resting: Option<Named>,
    /// The underlines (§2.3, Q6): what each name written as code came to by name (true: a definition or several), for
    /// the conversation and the checkout's files as they are — asked once each.
    words: HashMap<Arc<str>, bool>,
    /// The names drawn not known yet, in the order drawn: asked after the frame (`ask_names`).
    wanted: std::cell::RefCell<Vec<Arc<str>>>,
    /// The batch asked, each name's question until every server asked has answered.
    batch: Vec<Underline>,
    /// Names a loading server answered empty: asked again as its work ends (`work_ended`).
    later: Vec<Arc<str>>,
    /// The servers' count when none could be asked by name: nothing asked again until it changes.
    none_at: Option<usize>,
    /// Each file's locals as last parsed, for the maps' searches (in the one under way's hands meanwhile).
    defs: Defs,
    /// Since when a search has waited for an open buffer's parse (`after_frame`).
    parse_wait: Option<std::time::Instant>,
}

/// How long a search waits for an open buffer's parse under way, to read its tree, before parsing the file itself.
const PARSE_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

/// The local definitions of the files the maps' searches parsed (`syntax::local_defs`), each at the text it was parsed
/// of: a file is parsed once for every answer that reads it, and again only once it changes. Cleared with the maps'
/// `generation`.
#[derive(Debug, Default)]
struct Defs {
    files: HashMap<PathBuf, (Stamp, FileDefs)>,
    /// The files parsed, in order (the tests' check that a file is parsed once).
    #[cfg(test)]
    parses: Vec<PathBuf>,
}

/// A file's local definitions (None: its language has no `locals.scm`).
type FileDefs = Option<Arc<Vec<crate::syntax::LocalDef>>>;

/// A file's lines as read (None: not read — named like a key, too big, gone).
type Lines = Option<Arc<Vec<String>>>;

/// Which text of a file was read: an open buffer's at its version, else the disk's by its modified time and length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stamp {
    Open(DocId, u64),
    Disk(std::time::SystemTime, u64),
}

/// A name of the underlines' batch asked by name: its question's number, the servers yet to answer, whether one found
/// it, whether one was loading.
#[derive(Debug)]
struct Underline {
    n: u64,
    word: Arc<str>,
    waiting: Vec<ClientId>,
    found: bool,
    busy: bool,
}

/// Names asked at most in one batch (§2.4): the next batch once this one's answers are all in.
const BATCH: usize = 16;

/// Where a name written as code stands on a chat row (`ChatRow.names`): the bytes of the row's spans it covers, the name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameAt {
    pub bytes: Range<usize>,
    pub word: Arc<str>,
}

impl Links {
    /// Whether `word` is underlined: it came to a definition by name. Not known yet: asked after this frame.
    pub fn underlined(&self, word: &Arc<str>) -> bool {
        if let Some(&found) = self.words.get(word) {
            return found;
        }
        let asked = self.batch.iter().any(|a| a.word == *word) || self.later.contains(word);
        let mut wanted = self.wanted.borrow_mut();
        if !asked && !wanted.contains(word) {
            wanted.push(word.clone());
        }
        false
    }
}

/// Whether `span` (a code span's text) is a name: an identifier or a path of them (`a::b`, `ui.accent`, a call's `()`
/// after it), ≤ 80 characters — not a file's name (`main.rs`), a flag, a key, a number or words.
fn name_shaped(span: &str) -> bool {
    let s = span.strip_suffix("()").unwrap_or(span);
    let ident = |w: &str| {
        w.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
            && w.chars().all(|c| c.is_alphanumeric() || c == '_')
    };
    !s.is_empty()
        && s.len() <= 80
        && s.split("::").all(|p| p.split('.').all(ident))
        && !(s.contains('.') && crate::syntax::detect(Path::new(s)).is_some())
}

/// The names written as code on a message's rows (`ChatRow.names`), each word of a name-shaped code span its own (both
/// of `Editor::root`): a span drawn in the inline code's style (`code`) whose text is one of the message's code spans.
/// Laid out once with the message's rows; what's underlined is decided each frame (`Links::underlined`).
pub fn mark_names(rows: &mut [crate::chat::ChatRow], text: &str, code: crate::theme::Style) {
    if code.fg.is_none() && code.bg.is_none() || !text.contains('`') {
        return;
    }
    let parts = parts(&crate::chat::displayed(text));
    let spans: Vec<&str> = (parts.iter())
        .filter_map(|p| if let Part::Prose(t) = p { Some(t) } else { None })
        .flat_map(|t| t.split('`').skip(1).step_by(2).map(str::trim))
        .filter(|s| name_shaped(s))
        .collect();
    if spans.is_empty() {
        return;
    }
    let styled = |s: &crate::theme::Style| {
        code.fg.is_none_or(|f| s.fg == Some(f)) && code.bg.is_none_or(|b| s.bg == Some(b))
    };
    let w = |c: char| c.is_alphanumeric() || c == '_';
    for r in rows.iter_mut().filter(|r| r.link.is_none()) {
        let mut at = 0;
        for s in &r.spans {
            let t = s.text.trim();
            if styled(&s.style) && spans.contains(&t) {
                let lead = at + (s.text.len() - s.text.trim_start().len());
                let mut i = 0;
                while i < t.len() {
                    let start = i + t[i..].find(w).unwrap_or(t.len() - i);
                    let end = start + t[start..].find(|c| !w(c)).unwrap_or(t.len() - start);
                    if start < end {
                        let word = Arc::from(&t[start..end]);
                        r.names.push(NameAt { bytes: lead + start..lead + end, word });
                    }
                    i = end.max(start + 1);
                }
            }
            at += s.text.len();
        }
    }
}

/// After the frame (`after_frame`): the names it drew not known yet, asked by name — the newest answer's first, one
/// batch at a time (`BATCH`), each name once. A keyword is never asked (`syntax::keyword_in`: none underlined); with no
/// server that searches by name, nothing is asked until one starts. The main thread only sends: the answers land as
/// events (`named_answered`).
fn ask_names(ed: &mut Editor) {
    let servers = ed.lsp.clients.len();
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    let l = &mut c.links;
    if !l.batch.is_empty() || l.wanted.get_mut().is_empty() {
        return;
    }
    if l.none_at == Some(servers) {
        l.wanted.get_mut().clear();
        return;
    }
    l.none_at = None;
    // (drawn top to bottom: the newest answer's last)
    let mut wanted = std::mem::take(l.wanted.get_mut());
    wanted.reverse();
    let rest = wanted.split_off(wanted.len().min(BATCH));
    *l.wanted.get_mut() = rest;
    let root = ed.workspace.root.clone();
    for word in wanted {
        if crate::syntax::keyword_in([], &word).is_some() {
            if let Some(c) = ed.workspace.claude.chat.as_mut() {
                c.links.words.insert(word, false);
            }
            continue;
        }
        let n = next(ed);
        let asked = ed.ask_named(&word, Kind::ChatNamed { n }, &root);
        let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
        match asked {
            Ok(asked) => {
                let waiting = asked.into_iter().map(|a| a.0).collect();
                c.links.batch.push(Underline { n, word, waiting, found: false, busy: false });
            }
            // (a server starting: asked again when a frame draws it once it's ready)
            Err((_, true)) => {}
            Err((_, false)) => {
                c.links.none_at = Some(servers);
                c.links.wanted.get_mut().clear();
                return;
            }
        }
    }
}

/// A server's answer to an underline's question (`i` in the batch): found by its exact name, a shared server's for this
/// checkout only. Once every server asked has answered, what it came to is kept — a loading server's empty answer asked
/// again as its work ends — and once the batch is in, the next asked; a frame only where an underline came.
fn underline_answered(ed: &mut Editor, cid: ClientId, i: usize, result: Result<&Value, &Value>) {
    let loading = ed.client(cid).is_some_and(|c| !c.progress.is_empty());
    let roots = ed.client(cid).map(|c| c.roots.clone()).unwrap_or_default();
    let mine = ed.workspace.root.clone();
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    let l = &mut c.links;
    let a = &mut l.batch[i];
    let Some(k) = a.waiting.iter().position(|&w| w == cid) else { return };
    a.waiting.remove(k);
    match result {
        Ok(v) => {
            let exact = |s: &&Value| {
                let name = s["name"].as_str().unwrap_or_default();
                let path = s["location"]["uri"].as_str().and_then(crate::lsp::path_from_uri);
                name.split('(').next() == Some(&*a.word)
                    && path.is_some_and(|p| !crate::share::foreign(&roots, &mine, &p))
            };
            let found = v.as_array().is_some_and(|s| s.iter().any(|s| exact(&s)));
            a.found |= found;
            a.busy |= !found && loading;
        }
        Err(e) if matches!(e["code"].as_i64(), Some(-32801 | -32802)) => a.busy = true,
        Err(_) => {}
    }
    if !a.waiting.is_empty() {
        ed.unchanged = true;
        return;
    }
    let a = l.batch.remove(i);
    let came = a.found;
    match (a.found, a.busy) {
        (false, true) => l.later.push(a.word),
        (found, _) => {
            l.words.insert(a.word, found);
        }
    }
    if l.batch.is_empty() {
        ask_names(ed);
    }
    ed.unchanged = !came;
}

/// A message's key: its version and its text's length (an append the version didn't see).
fn key_of(m: &crate::chat::Msg) -> (u64, usize) {
    (m.version(), m.text.len())
}

fn generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A message to map: its index and key, its parts, each cited place that came to a file (its part, name, lines, file),
/// the names its prose writes as code that may be locals (`local_words`), and what its turn did to files (`turned`).
struct Want {
    msg: usize,
    key: (u64, usize),
    parts: Vec<Part>,
    places: Vec<(usize, Arc<str>, usize, usize, PathBuf)>,
    words: Vec<Arc<str>>,
    turned: Turned,
}

/// The names a message's prose writes as code that may be locals: a code span that's one identifier (a call's `()`
/// after it) — a qualified one names an item, never a local.
fn local_words(parts: &[Part]) -> Vec<Arc<str>> {
    let mut out: Vec<Arc<str>> = Vec::new();
    for p in parts {
        let Part::Prose(t) = p else { continue };
        for span in t.split('`').skip(1).step_by(2) {
            let s = span.trim();
            let s = s.strip_suffix("()").unwrap_or(s);
            if name_shaped(s) && !s.contains(['.', ':']) && !out.iter().any(|w| &**w == s) {
                out.push(Arc::from(s));
            }
        }
    }
    out
}

/// What an answer's turn did to files: those its `edit`, `replace` and `rename` calls changed, each with the lines they
/// left changed where the replay has them, and the project's files its `read` calls read.
#[derive(Clone, Debug, Default)]
struct Turned {
    changed: Vec<(PathBuf, Vec<Range<usize>>)>,
    read: Vec<PathBuf>,
}

impl Turned {
    fn note(&mut self, look: &crate::chat::Look, path: &Path, hunks: &[crate::replay::Hunk], root: &Path) {
        if look.changes() {
            let lines = hunks.iter().map(|h| h.lines.start..h.lines.end.max(h.lines.start + 1));
            match self.changed.iter_mut().find(|c| c.0 == path) {
                Some(c) => c.1.extend(lines),
                None => self.changed.push((path.to_path_buf(), lines.collect())),
            }
        } else if look.verb == "read" && path.starts_with(root) && !self.read.iter().any(|r| r == path) {
            self.read.push(path.to_path_buf());
        }
    }
}

/// What answer `i`'s turn did to files, from the chat's record: its tool rows between the question and this answer —
/// each row's first file (`Look.link`), for the whole conversation — and the replay's stops of those same calls, every
/// file each touched with the lines an edit left (`replay::Turn.asked`: the last `replay::TURNS` turns). A read of a page
/// is no file.
fn turned(c: &Chat, i: usize, root: &Path) -> Turned {
    let mut t = Turned::default();
    let Some(q) = c.msgs[..i].iter().rposition(|m| m.role == Role::User) else { return t };
    let mut calls = Vec::new();
    for m in c.msgs[q + 1..i].iter().filter(|m| m.role == Role::Tool) {
        calls.push(m.call.as_str());
        if let Some(l) = &m.look
            && let Some(link) = &l.link
        {
            t.note(l, &link.path, &[], root);
        }
    }
    // (the replay's turn of this question: by its place among yours)
    let n = c.msgs[..=q].iter().filter(|m| m.role == Role::User).count();
    let turn = c.follow.replay.turns().iter().find(|t| t.asked.as_ref().is_some_and(|a| a.0 == n));
    let stops = turn.into_iter().flat_map(|t| &t.stops);
    for s in stops.filter(|s| s.page.is_none() && calls.contains(&s.call.as_str())) {
        if let Some(p) = &s.path {
            t.note(&s.look, p, &s.hunks, root);
        }
    }
    t
}

/// After each frame (`term::render_drawing`, before `places::after_frame` asks the names it wants): the messages it
/// drew with no map for them as they are, mapped on a worker — one search at a time. A message whose places aren't
/// answered yet waits for a frame that has them; one that cites none is mapped empty here. The checkout's files
/// changed (another `listing`): every map made again.
pub fn after_frame(ed: &mut Editor) {
    let listing = ed.workspace.listing;
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    if c.links.listing != listing || c.links.generation == 0 {
        c.links.maps.clear();
        crate::event::drop_later(std::mem::take(&mut c.links.defs));
        (c.links.listing, c.links.generation, c.links.asking) = (listing, generation(), false);
        // (what the names came to, asked again for the files as they are — an answer in flight lands on nothing)
        (c.links.words, c.links.batch, c.links.later) = Default::default();
    }
    ask_names(ed);
    let docs = &ed.docs;
    let root = ed.workspace.root.clone();
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    if c.links.asking {
        return;
    }
    // (a buffer a map read edited since: made again — its lines may have moved —, read as it is meanwhile)
    for mp in c.links.maps.values_mut().filter(|mp| !mp.stale) {
        mp.stale = behind(docs, mp);
    }
    let laid = ed.screen.chat_laid.borrow();
    let writing = c.state == crate::chat::State::Writing;
    let mut want = Vec::new();
    let mut wake = false;
    for i in laid.iter().filter_map(|(of, _)| if let Of::Msg(i) = of { Some(*i) } else { None }) {
        let Some(m) = c.msgs.get(i).filter(|m| matches!(m.role, Role::User | Role::Assistant)) else {
            continue;
        };
        let key = key_of(m);
        if (writing && i + 1 == c.msgs.len())
            || c.links.maps.get(&i).is_some_and(|mp| mp.key == key && !mp.stale)
        {
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
        // (an answer's names that may be locals, looked for in its own files and its turn's)
        let words = if m.role == Role::Assistant { local_words(&parts) } else { Vec::new() };
        let turned = if words.is_empty() { Turned::default() } else { turned(c, i, &root) };
        let files = !places.is_empty() || !turned.changed.is_empty() || !turned.read.is_empty();
        match (files && (!places.is_empty() || !words.is_empty()), answered) {
            (_, false) => {}
            // (its names drawn held this frame: one known by name asks the frame that shows it — `drawn` asks the
            // others by name)
            (false, true) => {
                let was = drawn(c, i);
                c.links.maps.insert(i, Map { key, ..Map::default() });
                wake |= drawn(c, i) != was;
            }
            (true, true) => want.push(Want { msg: i, key, parts, places, words, turned }),
        }
    }
    drop(laid);
    if wake {
        // (run inside the frame, it asks the next by an event: an empty job's landing draws)
        ed.events.jobs().spawn(|| |_: &mut Editor| {});
    }
    if want.is_empty() {
        c.links.parse_wait = None;
        return ask_names(ed);
    }
    let paths: Vec<&PathBuf> = (want.iter())
        .flat_map(|w| {
            let changed = w.turned.changed.iter().map(|c| &c.0);
            w.places.iter().map(|p| &p.4).chain(changed).chain(&w.turned.read)
        })
        .collect();
    // (an open buffer whose locals aren't kept at its version, its parse under way: waited for — its tree, the query
    // alone, beats parsing it whole on the worker (term.rs: 357 ms) —, the stale maps drawn meanwhile; `PARSE_WAIT` at
    // most, then parsed there)
    let parsing = paths.iter().filter_map(|p| Some((*p, docs.by_path(p)?))).any(|(p, d)| {
        let kept = c.links.defs.files.get(p).is_some_and(|f| f.0 == Stamp::Open(d.id, d.version()));
        !kept && d.syntax.as_ref().is_some_and(|s| s.in_flight)
    });
    let now = crate::clock::now();
    if parsing && now.saturating_duration_since(*c.links.parse_wait.get_or_insert(now)) < PARSE_WAIT {
        return ask_names(ed);
    }
    (c.links.parse_wait, c.links.asking) = (None, true);
    let generation = c.links.generation;
    let defs = std::mem::take(&mut c.links.defs);
    let open: HashMap<PathBuf, Open> = (paths.iter())
        .filter_map(|p| {
            let d = docs.by_path(p)?;
            // (its tree only where it's of this text: no edit since its parse started, none under way)
            let s = d.syntax.as_ref().filter(|s| !s.dirty && !s.in_flight);
            let tree = s.and_then(|s| Some((s.tree.clone()?, s.lang.clone())));
            Some(((*p).clone(), Open { text: d.text.clone(), version: d.version(), id: d.id, tree }))
        })
        .collect();
    ed.events.jobs().spawn(move || {
        let mut files = Files::new(open, root, defs);
        let maps: Vec<(usize, Map)> = want.into_iter().map(|w| (w.msg, map_of(&w, &mut files))).collect();
        let defs = files.defs;
        move |ed: &mut Editor| landed(ed, generation, maps, defs)
    });
    ask_names(ed);
}

/// Whether a buffer `mp` read was edited since.
fn behind(docs: &crate::docpaths::Docs, mp: &Map) -> bool {
    mp.versions.iter().any(|(p, v)| docs.by_path(p).is_some_and(|d| d.version() != *v))
}

/// A search's maps and the files' locals it parsed: kept when they're for the files as they are (another `listing`
/// since: dropped, asked again), their messages' names not locals asked by name (`drawn`'s second pass, through
/// `Links::underlined`). A frame where a message's names draw otherwise than they did — a local underlined, a held
/// answer's names known by name now, a stale map's local gone —, and where a map landed behind its buffer: an edit made
/// while it was searched marked nothing (`after_frame` waits on `asking`), so that frame marks it and makes it again.
fn landed(ed: &mut Editor, generation: u64, maps: Vec<(usize, Map)>, defs: Defs) {
    let docs = &ed.docs;
    let Some(c) = ed.workspace.claude.chat.as_mut().filter(|c| c.links.generation == generation) else {
        return crate::event::drop_later((maps, defs));
    };
    (c.links.asking, c.links.defs) = (false, defs);
    let late = maps.iter().any(|(_, mp)| behind(docs, mp));
    let before: Vec<(usize, Vec<bool>)> = maps.iter().map(|(i, _)| (*i, drawn(c, *i))).collect();
    c.links.maps.extend(maps);
    let changed = before.iter().any(|(i, was)| drawn(c, *i) != *was);
    ask_names(ed);
    ed.unchanged = !changed && !late;
}

/// Whether each name written as code in message `i`, as laid out, draws underlined (`name_underlined`).
fn drawn(c: &Chat, i: usize) -> Vec<bool> {
    let Some(laid) = c.msgs.get(i).map(|m| m.cache.borrow()) else { return Vec::new() };
    let names = laid.iter().flat_map(|l| l.rows.iter()).flat_map(|r| &r.names);
    names.map(|n| name_underlined(c, Some(i), &n.word)).collect()
}

/// Whether `word`, a name written as code in message `msg`, draws underlined: a local its map has; held while an
/// answer's map isn't made yet; else by name (`Links::underlined`, which asks it after the frame when it isn't known).
pub fn name_underlined(c: &Chat, msg: Option<usize>, word: &Arc<str>) -> bool {
    match msg.map(|i| local_underlined(c, i, word)) {
        Some(Some(true)) => true,
        Some(None) => false,
        _ => c.links.underlined(word),
    }
}

/// An open buffer a search reads: its text, version and id, and its syntax tree with its language where the tree is of
/// that text.
struct Open {
    text: ropey::Rope,
    version: u64,
    id: DocId,
    tree: Option<(tree_sitter::Tree, Arc<crate::syntax::LangData>)>,
}

/// The files a search reads: an open buffer's text, else the disk's — never a file named like a key, nor a huge one —,
/// each with the text's stamp; and their locals, parsed once a text (`Defs`).
struct Files {
    open: HashMap<PathBuf, Open>,
    read: HashMap<PathBuf, (Lines, Option<Stamp>)>,
    root: PathBuf,
    /// The open buffers the message being mapped read, at their versions.
    used: Vec<(PathBuf, u64)>,
    defs: Defs,
}

impl Files {
    fn new(open: HashMap<PathBuf, Open>, root: PathBuf, defs: Defs) -> Files {
        Files { open, read: HashMap::new(), root, used: Vec::new(), defs }
    }

    fn lines(&mut self, path: &Path) -> Lines {
        if let Some(o) = self.open.get(path)
            && !self.used.iter().any(|u| u.0 == path)
        {
            self.used.push((path.to_path_buf(), o.version));
        }
        if let Some(l) = self.read.get(path) {
            return l.0.clone();
        }
        // (the disk's stamp taken before its text: a change between the two parses it again next time, never the reverse)
        let disk = || {
            let m = std::fs::metadata(path).ok()?;
            Some((m.len(), m.modified().ok()))
        };
        let (text, stamp) = match self.open.get(path) {
            Some(o) => (Some(o.text.to_string()), Some(Stamp::Open(o.id, o.version))),
            None if crate::sandbox::key_named(&self.root, path) => (None, None),
            None => match disk() {
                Some((len, _)) if len > FILE_MAX => (None, None),
                stat => {
                    let stamp = stat.and_then(|(len, at)| Some(Stamp::Disk(at?, len)));
                    (std::fs::read_to_string(path).ok(), stamp)
                }
            },
        };
        let lines = text.map(|t| Arc::new(t.lines().map(|l| l.trim_end_matches('\r').to_string()).collect()));
        self.read.insert(path.to_path_buf(), (lines.clone(), stamp));
        lines
    }

    /// `path`'s local definitions (`syntax::local_defs`) of its text as read (`lines`): parsed once a text — an open
    /// buffer's tree of it queried instead. None: its language isn't known or loaded, or has no `locals.scm`.
    fn defs(&mut self, path: &Path, lines: &[String]) -> FileDefs {
        let stamp = self.read.get(path).and_then(|r| r.1);
        if let Some((at, defs)) = self.defs.files.get(path)
            && Some(*at) == stamp
        {
            return defs.clone();
        }
        let defs = match self.open.get(path).and_then(|o| Some((&o.text, o.tree.as_ref()?))) {
            Some((text, (tree, lang))) => crate::syntax::local_defs_in(lang, tree, &text.to_string()),
            None => {
                let spec = crate::syntax::detect(path)?;
                let lang = crate::syntax::Loader::global().load(spec).ok()?;
                #[cfg(test)]
                self.defs.parses.push(path.to_path_buf());
                crate::syntax::local_defs(&lang, &lines.join("\n"))
            }
        };
        let defs = defs.map(Arc::new);
        if let Some(stamp) = stamp {
            self.defs.files.insert(path.to_path_buf(), (stamp, defs.clone()));
        }
        defs
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
    let (locals, _parsed) = locals_of(w, &quoted, files);
    Map {
        key: w.key,
        quoted,
        near,
        locals,
        versions: std::mem::take(&mut files.used),
        stale: false,
        #[cfg(test)]
        parsed: _parsed,
    }
}

/// A definition a ring found, with the lines of the scopes around it (innermost first).
type Def = (Spot, Vec<Range<usize>>);

/// The definitions of each of `w`'s names (`local_words`) that are locals by the syntax tree (`syntax::local_defs`),
/// nearest ring first, the first that holds one deciding: the answer's own files — those it cites, its blocks quote
/// (`quoted`) and its turn changed, `OWN_MAX` at most —, then those its turn only read whose text holds the name
/// (`READ_MAX`). Several in a ring: the one whose scope holds a line the answer cites or its turn changed, where exactly
/// one's does (`settle`). A language with no `locals.scm` adds none. Each file parsed once a text (`Files::defs`). And
/// the files whose locals it read.
fn locals_of(
    w: &Want,
    quoted: &[Option<Quoted>],
    files: &mut Files,
) -> (HashMap<Arc<str>, Vec<Spot>>, Vec<PathBuf>) {
    let (mut out, mut parsed) = (HashMap::new(), Vec::new());
    if w.words.is_empty() {
        return (out, parsed);
    }
    let mut own: Vec<&PathBuf> = Vec::new();
    let cited = w.places.iter().map(|p| &p.4);
    let changed = w.turned.changed.iter().map(|c| &c.0);
    for p in cited.chain(quoted.iter().flatten().map(|q| &q.path)).chain(changed) {
        if !own.contains(&p) && own.len() < OWN_MAX {
            own.push(p);
        }
    }
    // (the answer's focus: the lines it cites, the lines its turn changed)
    let mut focus: Vec<(&Path, Range<usize>)> = w.places.iter().map(|p| (&*p.4, p.2..p.3 + 1)).collect();
    focus.extend(w.turned.changed.iter().flat_map(|(p, lines)| lines.iter().map(move |l| (&**p, l.clone()))));
    let mut left = w.words.clone();
    let read: Vec<&PathBuf> = w.turned.read.iter().filter(|p| !own.contains(p)).collect();
    for (ring, max) in [(own, OWN_MAX), (read, READ_MAX)] {
        let mut found: HashMap<Arc<str>, Vec<Def>> = HashMap::new();
        let mut looked = 0;
        for path in ring {
            if left.is_empty() || looked == max {
                break;
            }
            let Some(lines) = files.lines(path) else { continue };
            // (a file whose text holds none of the names left isn't parsed)
            if !lines.iter().any(|l| left.iter().any(|w| !whole_in(l, w).is_empty())) {
                continue;
            }
            looked += 1;
            let Some(defs) = files.defs(path, &lines) else { continue };
            parsed.push(path.clone());
            for d in defs.iter() {
                let Some(word) = left.iter().find(|w| ***w == *d.name) else { continue };
                let text = lines.get(d.line).cloned().unwrap_or_default();
                let spot = Spot { path: path.clone(), line: d.line, col: d.col, word: d.name.clone(), text };
                found.entry(word.clone()).or_default().push((spot, d.scopes.clone()));
            }
        }
        left.retain(|w| !found.contains_key(w));
        out.extend(found.into_iter().map(|(word, defs)| (word, settle(defs, &focus))));
    }
    (out, parsed)
}

/// A ring's definitions of one name: one is the answer; of several, the one whose scope holds a line in the answer's
/// `focus` wins where exactly one's does — its scope the widest around it that holds no other of them (an `i` in three
/// functions: each its function's) —, else all of them.
fn settle(defs: Vec<Def>, focus: &[(&Path, Range<usize>)]) -> Vec<Spot> {
    if defs.len() < 2 {
        return defs.into_iter().map(|d| d.0).collect();
    }
    // (each file's definitions' lines in order: how many a scope holds by two searches — an `ed` that's a parameter in
    // a thousand functions isn't a million comparisons)
    let mut lines: HashMap<&Path, Vec<usize>> = HashMap::new();
    for (s, _) in &defs {
        lines.entry(s.path.as_path()).or_default().push(s.line);
    }
    lines.values_mut().for_each(|l| crate::sorting::sort(l));
    let count = |p: &Path, sc: &Range<usize>| {
        let l = lines.get(p).map_or(&[][..], |l| &l[..]);
        l.partition_point(|&x| x < sc.end) - l.partition_point(|&x| x < sc.start)
    };
    // (a scope holds its own definition: another's there too when it holds two)
    let territory = |k: usize| {
        let (s, scopes) = &defs[k];
        scopes.iter().take_while(|sc| count(&s.path, sc) < 2).last().cloned()
    };
    let held = |k: usize| {
        territory(k).is_some_and(|t| {
            (focus.iter()).any(|(p, r)| *p == defs[k].0.path && r.start < t.end && t.start < r.end)
        })
    };
    let picked: Vec<usize> = (0..defs.len()).filter(|&k| held(k)).collect();
    match picked.as_slice() {
        &[k] => vec![defs[k].0.clone()],
        _ => defs.into_iter().map(|d| d.0).collect(),
    }
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

/// What the transcript's cursor is on (§2.1): a cited place (its file and line, and the lines read there for `K`'s peek),
/// a name at a spot in a file, or a name no cited place holds — to ask by name, the container a qualified one names
/// (`Thread` of `Thread::length`), and why no place held it where the answer has places (said with the answer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Place {
        name: String,
        link: crate::chat::Link,
        peek: Option<Vec<String>>,
    },
    At(Spot),
    Named {
        word: String,
        container: Option<String>,
        why: Option<String>,
    },
    /// Several locals of that name in the answer's files, none in its focus (`settle`): `gd` lists them.
    Locals {
        word: String,
        spots: Vec<Spot>,
    },
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
/// The files an answer cites not read yet (`after_frame`): there's nothing to look up in yet.
const WAIT: &str = "still reading the files this answer cites — ask again in a moment";

/// What the transcript's cursor is on, from what's held (no disk, no request) — Err: why there's nothing to ask,
/// in words to say.
pub(crate) fn target(ed: &Editor) -> Result<Target, String> {
    let Some(head) = head(ed) else { return Err(NOT_CODE.into()) };
    target_at(ed, head)
}

/// What the transcript's byte `head` is on (`target`'s rule — the cursor's, or the pointer's rest).
fn target_at(ed: &Editor, head: usize) -> Result<Target, String> {
    let Some((c, d)) = transcript(ed) else { return Err(NOT_CODE.into()) };
    let t = c.transcript.as_ref().expect("a transcript, above");
    let head = head.min(d.text.len_bytes());
    let line = d.text.byte_to_line(head);
    let Some((i, lines)) = t.msg_lines(line) else { return Err(NOT_CODE.into()) };
    let Some(m) = c.msgs.get(i).filter(|m| matches!(m.role, Role::User | Role::Assistant)) else {
        return Err(NOT_CODE.into());
    };
    let text_of = |l: usize| d.text.line(l).to_string().trim_end_matches(['\n', '\r']).to_string();
    let here = text_of(line);
    let at = head - d.text.line_to_byte(line);
    // (a stale map too, while its file's edit is mapped again: its place still beats asking by name — a line that moved
    // is said as it's asked, `ask`)
    let map = c.links.maps.get(&i).filter(|mp| mp.key == key_of(m));
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
    let named = |why: Option<String>| {
        Ok(Target::Named { word: word.clone(), container: qualifier(&here, w.start), why })
    };
    let parts = parts(&crate::chat::displayed(&m.text));
    // (a keyword of the language it's written in is never asked: said at once)
    let keyword = |langs: Vec<&str>| {
        crate::syntax::keyword_in(langs, &word)
            .map(|l| format!("`{word}` is a {l} keyword — nothing to look up"))
    };
    // In a code block: its run of lines in the message's, found in order
    let drawn: Vec<String> = lines.clone().map(text_of).collect();
    let k = line - lines.start;
    let (mut from, mut b) = (0, 0);
    for (j, p) in parts.iter().enumerate() {
        let Part::Code { lines: code, lang } = p else { continue };
        if let Some(s) = run_of(&drawn, code, from) {
            if (s..s + code.len()).contains(&k) {
                let langs = match crate::syntax::resolve(lang) {
                    Some(l) => vec![l.name.as_str()],
                    None => langs_of(&parts),
                };
                if let Some(said) = keyword(langs) {
                    return Err(said);
                }
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
                    // (by name, then — a block that quotes nothing is Claude's own code, a proposal)
                    return named(
                        file.map(|n| format!("these lines aren't in {} as quoted", shown(ed, &n.path))),
                    );
                };
                let shown = shown(ed, &q.path);
                let Some((fl, raw)) = q.at.get(k - s).cloned().flatten() else {
                    return named(Some(format!("this line isn't in {shown} as quoted")));
                };
                // (the name's place in the line by characters from its text's start: the quote's indent may differ)
                let lead = here.len() - here.trim_start().len();
                let chars = here[lead.min(w.start)..w.start].chars().count();
                let raw_lead = raw.len() - raw.trim_start().len();
                let guess =
                    raw[raw_lead..].char_indices().nth(chars).map_or(raw.len(), |(i, _)| raw_lead + i);
                let col = whole_in(&raw, &word).into_iter().min_by_key(|c| c.abs_diff(guess));
                let Some(col) = col else {
                    return named(Some(format!("this line isn't in {shown} as quoted")));
                };
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
    if let Some(said) = keyword(langs_of(&parts)) {
        return Err(said);
    }
    // (then a local of that name in the answer's own files, or those its turn read: `locals_of`)
    let local = |map: &Map, why: Option<String>| match map.locals.get(word.as_str()).map(Vec::as_slice) {
        Some([one]) if qualifier(&here, w.start).is_none() => Ok(Target::At(one.clone())),
        Some(spots) if qualifier(&here, w.start).is_none() && !spots.is_empty() => {
            Ok(Target::Locals { word: word.clone(), spots: spots.to_vec() })
        }
        _ => named(why),
    };
    if cites.is_empty() {
        let writing = c.state == crate::chat::State::Writing && i + 1 == c.msgs.len();
        return match map {
            Some(map) => local(map, None),
            // (the answer's files not read yet, where it has names that may be locals)
            None if !writing && local_words(&parts).iter().any(|l| **l == *word) => Err(WAIT.into()),
            None => named(None),
        };
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
            local(
                map,
                Some(format!("`{word}` isn't on the lines this paragraph cites ({})", places.join(", "))),
            )
        }
    }
}

/// Whether message `i`'s `word` is a local its files define (`Map.locals`, for the message as it is — a stale map too,
/// until the one being made lands: its names never blink as a file beside the chat is typed in): underlined. None: an
/// answer whose map isn't made yet — nothing underlined nor asked by name until it is (a local is never asked).
pub fn local_underlined(c: &Chat, i: usize, word: &str) -> Option<bool> {
    let m = c.msgs.get(i)?;
    match c.links.maps.get(&i).filter(|mp| mp.key == key_of(m)) {
        Some(mp) => Some(mp.locals.get(word).is_some_and(|s| !s.is_empty())),
        // (the answer being written isn't mapped until it ends: asked by name meanwhile)
        None if m.role != Role::Assistant
            || (c.state == crate::chat::State::Writing && i + 1 == c.msgs.len()) =>
        {
            Some(false)
        }
        None => None,
    }
}

/// The languages a message's words may be written in: its code blocks' and the files it cites' (none: every one loaded,
/// `syntax::keyword_in`).
fn langs_of(parts: &[Part]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for p in parts {
        let found: Vec<&'static crate::languages::LangSpec> = match p {
            Part::Code { lang, .. } => crate::syntax::resolve(lang).into_iter().collect(),
            Part::Prose(t) => {
                cited(t).iter().filter_map(|(n, ..)| crate::syntax::detect(Path::new(&**n))).collect()
            }
        };
        for l in found {
            if !out.contains(&l.name.as_str()) {
                out.push(l.name.as_str());
            }
        }
    }
    out
}

/// The container a qualified name at byte `at` of `line` names: `Thread` of `Thread::length`, `ui` of `ui.accent` — the
/// name right before a `::` or `.` just before it.
fn qualifier(line: &str, at: usize) -> Option<String> {
    let before = &line[..at];
    let before = before.strip_suffix("::").or_else(|| before.strip_suffix('.'))?;
    let w = word_at(before, before.len())?;
    (w.end == before.len()).then(|| before[w].to_string())
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
    // (a key's question takes the place of any before it)
    if let Some(c) = ed.workspace.claude.chat.as_mut() {
        (c.links.asked, c.links.named) = (None, None);
    }
    match t {
        Err(why) => ed.set_status(why),
        Ok(Target::Place { link, .. }) if goto.is_some() => crate::chat::jump(ed, link),
        Ok(Target::Place { name, link, peek }) => match peek {
            Some(lines) if !lines.is_empty() => {
                let (md, lang) = peek_md(&link, &lines);
                show(ed, &md, lang.as_deref(), name);
            }
            _ => ed.set_status(format!("{name} — still reading its lines: ask again in a moment")),
        },
        Ok(Target::At(spot)) => ask(ed, spot, goto),
        Ok(Target::Named { word, container, why }) => by_name(ed, Named::new(word, container, goto, why)),
        Ok(Target::Locals { word, spots }) => {
            let k = spots.len();
            match goto {
                Some(Goto::Definition | Goto::Declaration) => {
                    let found = spots.into_iter().map(|s| (s.path, s.line, s.col)).collect();
                    ed.refs_anew(word);
                    ed.list_places(Goto::Definition, found);
                    ed.set_status(format!("{k} found in the answer's files"));
                }
                _ => ed
                    .set_status(format!("{k} definitions of `{word}` in the answer's files — gd lists them")),
            }
        }
    }
}

/// A cited place's lines as a card's markdown, in its file's language.
fn peek_md(link: &crate::chat::Link, lines: &[String]) -> (String, Option<String>) {
    let lang = crate::syntax::detect(&link.path).map(|l| l.name.clone());
    (format!("```{}\n{}\n```", lang.as_deref().unwrap_or(""), lines.join("\n")), lang)
}

/// The transcript's cursor now.
fn head(ed: &Editor) -> Option<usize> {
    transcript(ed).map(|(_, d)| d.selection().primary().cursor(&d.text))
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
        // (read again after the next frame: a second press finds it where it is now, if it's still there — the maps
        // drawn as they are meanwhile)
        if let Some(c) = ed.workspace.claude.chat.as_mut() {
            c.links.maps.values_mut().for_each(|mp| mp.stale = true);
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

/// A server's work ended: the chat's question it was busy with asked again — at its spot, or by name (while the
/// transcript's cursor is where it was asked).
pub fn work_ended(ed: &mut Editor, cid: ClientId) {
    // (the underlines a loading server answered empty: asked again)
    if let Some(c) = ed.workspace.claude.chat.as_mut()
        && !c.links.later.is_empty()
    {
        let later = std::mem::take(&mut c.links.later);
        c.links.wanted.get_mut().extend(later);
        ask_names(ed);
    }
    let head = head(ed);
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    if let Some(q) = c.links.named.take_if(|q| q.again && q.busy.contains(&cid)) {
        if Some(q.head) == head {
            let tries = q.tries;
            by_name(ed, Named { tries, ..Named::new(q.word, q.container, q.goto, q.why) });
        }
        return;
    }
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

// ── By name (§2.1-4) ────────────────────────────────────────────────────────────────────────────────────────────

impl Named {
    fn new(word: String, container: Option<String>, goto: Option<Goto>, why: Option<String>) -> Named {
        Named {
            n: 0,
            word,
            container,
            goto,
            why,
            head: 0,
            waiting: Vec::new(),
            servers: Vec::new(),
            found: Vec::new(),
            busy: Vec::new(),
            errors: Vec::new(),
            tries: 0,
            again: false,
            rest: None,
        }
    }

    /// `text` said of this question: after why no cited place held its name, where one didn't.
    fn said(&self, text: String) -> String {
        match &self.why {
            Some(why) => format!("{why} — {text}"),
            None => text,
        }
    }
}

/// `q`'s name asked of the project's servers (`workspace/symbol`) — none that can be, and a go-to is the syntax tree's
/// stand-in, said; another key, why.
fn by_name(ed: &mut Editor, mut q: Named) {
    (q.n, q.head) = (next(ed), head(ed).unwrap_or(0));
    let root = ed.workspace.root.clone();
    match ed.ask_named(&q.word, Kind::ChatNamed { n: q.n }, &root) {
        Err((why, later)) => {
            // (the reason in the chat's words: Claude's asks it for a file's path to start a server)
            let why =
                if later { why } else { "no language server that searches by name is running here".into() };
            match q.goto {
                Some(Goto::Definition | Goto::Declaration) => stand_in(ed, q, why),
                _ => {
                    let text = q.said(format!("{why}, so nothing is known of `{}`", q.word));
                    ed.set_status(text)
                }
            }
        }
        Ok(asked) => {
            (q.waiting, q.servers) = asked.into_iter().unzip();
            if let Some(c) = ed.workspace.claude.chat.as_mut() {
                c.links.named = Some(q);
            }
        }
    }
}

/// No server to ask: the definitions the syntax tree finds among the name's text matches, on a worker (`nav::tree_found`,
/// Claude's `definition`'s stand-in) — said as the tree's, `why` with it.
fn stand_in(ed: &mut Editor, q: Named, why: String) {
    let (root, keys, word, n) = (ed.workspace.root.clone(), crate::reads::Keys::of(ed), q.word.clone(), q.n);
    // (a buffer with edits read as it is — a rope's clone, its nodes shared)
    let edited = crate::nav::edited(ed);
    if let Some(c) = ed.workspace.claude.chat.as_mut() {
        c.links.named = Some(q);
    }
    ed.events.jobs().spawn(move || {
        let found = crate::nav::tree_found(&root, &word, Goto::Definition, &keys, &edited);
        move |ed: &mut Editor| {
            let head = head(ed);
            let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
            let Some(q) = c.links.named.take_if(|q| q.n == n) else { return };
            if Some(q.head) != head {
                return;
            }
            let places = match found {
                Err(e) => return ed.set_status(q.said(format!("{why}, and the text search failed: {e}"))),
                Ok((places, _)) => places,
            };
            let by = format!("found by the syntax tree ({why})");
            match places.as_slice() {
                [] => ed.set_status(
                    q.said(format!("{why}, and the syntax tree finds no definition of `{word}`")),
                ),
                [(p, _)] => {
                    go_to_name(ed, &p.path, p.line, &word);
                    ed.set_status(q.said(by));
                }
                _ => {
                    let found = places.iter().map(|(p, _)| (p.path.clone(), p.line, 0)).collect();
                    ed.refs_anew(word);
                    ed.list_places(Goto::Definition, found);
                    ed.set_status(q.said(format!("{} {by}", places.len())));
                }
            }
        }
    });
}

/// `path` opened at `line`, the cursor on `word` there (its line's start where it isn't) — placed once its text is in.
fn go_to_name(ed: &mut Editor, path: &Path, line: usize, word: &str) {
    let word = word.to_string();
    ed.open_at_then(path, line, 0, move |ed| {
        let d = ed.doc();
        let Some(col) = d.text.get_line(line).and_then(|l| whole_in(&l.to_string(), &word).first().copied())
        else {
            return;
        };
        crate::editor::place_cursor(ed.doc_mut(), line, col);
    });
}

/// A server's answer to the chat's question `n` by name (Err: its error): the exact name's symbols kept — a shared
/// server's for this checkout only — and once every server asked has answered, what they came to (`finish`). Another
/// asked since, or the transcript's cursor moved: dropped.
pub fn named_answered(ed: &mut Editor, cid: ClientId, n: u64, result: Result<&Value, &Value>) {
    let batch = ed.workspace.claude.chat.as_ref().and_then(|c| c.links.batch.iter().position(|a| a.n == n));
    if let Some(i) = batch {
        return underline_answered(ed, cid, i, result);
    }
    let head = head(ed);
    let (server, loading) = match ed.client(cid) {
        Some(c) => (c.name.clone(), !c.progress.is_empty()),
        None => (String::new(), false),
    };
    let roots = ed.client(cid).map(|c| c.roots.clone()).unwrap_or_default();
    let mine = ed.workspace.root.clone();
    let rest = crate::hoverrest::resting(ed);
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    // (the pointer's rest's: while it rests there still)
    if c.links.resting.as_ref().is_some_and(|q| q.n == n) {
        let Some(q) = c.links.resting.as_mut().filter(|q| q.rest.is_some() && q.rest == rest) else {
            c.links.resting = None;
            return;
        };
        if !keep(q, cid, result, &server, loading, &roots, &mine) {
            return;
        }
        if let Some(q) = c.links.resting.take_if(|q| q.waiting.is_empty()) {
            rest_found(ed, q);
        }
        return;
    }
    let Some(q) = c.links.named.as_mut().filter(|q| q.n == n) else { return };
    if Some(q.head) != head {
        c.links.named = None;
        return;
    }
    if !keep(q, cid, result, &server, loading, &roots, &mine) {
        return;
    }
    if q.waiting.is_empty()
        && let Some(q) = c.links.named.take()
    {
        finish(ed, q);
    }
}

/// Server `cid`'s answer to `q` kept (Err: its error): the exact name's symbols — a shared server's (`roots`) for this
/// checkout (`mine`) only —, a busy or loading one that found none noted, an error kept. False: `q` didn't wait on it.
#[allow(clippy::too_many_arguments)]
fn keep(
    q: &mut Named,
    cid: ClientId,
    result: Result<&Value, &Value>,
    server: &str,
    loading: bool,
    roots: &[PathBuf],
    mine: &Path,
) -> bool {
    let Some(i) = q.waiting.iter().position(|&w| w == cid) else { return false };
    q.waiting.remove(i);
    let before = q.found.len();
    match result {
        Ok(v) => {
            for s in v.as_array().into_iter().flatten() {
                // (the exact name, case and all — a server's `length()` too — never a close one)
                let name = s["name"].as_str().unwrap_or_default();
                if name.split('(').next() != Some(q.word.as_str()) {
                    continue;
                }
                let Some(path) = s["location"]["uri"].as_str().and_then(crate::lsp::path_from_uri) else {
                    continue;
                };
                if crate::share::foreign(roots, mine, &path) {
                    continue;
                }
                let pos = match &s["location"]["range"]["start"] {
                    Value::Null => serde_json::json!({ "line": 0, "character": 0 }),
                    p => p.clone(),
                };
                let container = s["containerName"].as_str().unwrap_or_default().to_string();
                q.found.push((cid, path, pos, container));
            }
        }
        Err(e) if matches!(e["code"].as_i64(), Some(-32801 | -32802)) => q.busy.push(cid),
        Err(e) => {
            q.errors.push(format!("{server}: {}", e["message"].as_str().unwrap_or("the request failed")))
        }
    }
    if q.found.len() == before && loading && !q.busy.contains(&cid) {
        q.busy.push(cid);
    }
    true
}

/// Every server asked has answered `q`: one definition gone to (or asked about, for the other keys), several listed,
/// none said — put off while a server that found none is loading, asked again as its work ends.
fn finish(ed: &mut Editor, mut q: Named) {
    let mut found = defs(&mut q);
    let word = q.word.clone();
    match found.len() {
        0 if !q.busy.is_empty() => {
            let server = ed.client(q.busy[0]).map(|c| c.name.clone()).unwrap_or_default();
            q.tries += 1;
            if q.tries > TRIES_MAX {
                return ed.set_status(
                    q.said(format!("{server} is still loading the project — try again in a moment")),
                );
            }
            let then = match q.goto {
                None => "showing it",
                Some(Goto::References) => "listing them",
                Some(_) => "going there",
            };
            ed.set_status(q.said(format!("{server} is loading — {then} when ready")));
            q.again = true;
            if let Some(c) = ed.workspace.claude.chat.as_mut() {
                c.links.named = Some(q);
            }
        }
        0 => {
            let servers = q.servers.join(" and ");
            let text = match q.errors.is_empty() {
                true => format!("{servers} found nothing named `{word}`"),
                false => q.errors.join("; "),
            };
            ed.set_status(q.said(text));
        }
        1 => {
            let (cid, path, pos, _) = found.pop().expect("one");
            let enc = ed.encoding_of(cid);
            match q.goto {
                Some(Goto::Definition | Goto::Declaration) => {
                    ed.lsp_jump(&path, &pos, enc);
                    if q.why.is_some() {
                        ed.set_status(q.said("found by name".into()));
                    }
                }
                // (the hover, the type, the implementations and the uses: asked at the one definition)
                goto => ask_there(ed, q, &path, &pos, enc, goto),
            }
        }
        k => match q.goto {
            Some(Goto::Definition | Goto::Declaration) => {
                let at = |pos: &Value, key: &str| pos[key].as_u64().unwrap_or(0) as usize;
                let found =
                    found.into_iter().map(|(_, p, pos, _)| (p, at(&pos, "line"), at(&pos, "character")));
                ed.refs_anew(word);
                ed.list_places(Goto::Definition, found.collect());
                if q.why.is_some() {
                    ed.set_status(q.said(format!("{k} found by name")));
                }
            }
            _ => ed.set_status(q.said(format!("{k} definitions of `{word}` — gd lists them"))),
        },
    }
}

/// The definitions `q` found, taken: a qualified name's those in its container where there are any, one a line.
fn defs(q: &mut Named) -> Vec<(ClientId, PathBuf, Value, String)> {
    let mut found = std::mem::take(&mut q.found);
    if let Some(c) = &q.container
        && found.iter().any(|f| !whole_in(&f.3, c).is_empty())
    {
        found.retain(|f| !whole_in(&f.3, c).is_empty());
    }
    let line = |pos: &Value| pos["line"].as_u64().unwrap_or(0) as usize;
    found.dedup_by(|a, b| a.1 == b.1 && line(&a.2) == line(&b.2));
    found
}

/// Where `word` stands whole on the line of byte `b` of `text`, the nearest at or after `b` first (a symbol's place may
/// be where its item starts, `pub fn`): its byte — None, never a guess.
fn on_line(text: &ropey::Rope, b: usize, word: &str) -> Option<usize> {
    let l = text.byte_to_line(b);
    let start = text.line_to_byte(l);
    let col = b - start;
    let at =
        whole_in(&text.line(l).to_string(), word).into_iter().min_by_key(|&c| (c < col, c.abs_diff(col)))?;
    Some(start + at)
}

/// `goto` (or the hover) asked at the definition `q`'s name came to: its file opened aside, the name on that line.
fn ask_there(
    ed: &mut Editor,
    q: Named,
    path: &Path,
    pos: &Value,
    enc: crate::lsp::Encoding,
    goto: Option<Goto>,
) {
    let shown = shown(ed, path);
    let id = match crate::nav::buffer_for(ed, path) {
        Ok(id) => id,
        Err(e) => return ed.set_status(q.said(format!("{shown}: {e}"))),
    };
    let Some(doc) = ed.docs.iter().find(|d| d.id == id) else { return };
    if doc.loading {
        return ed.set_status(q.said(format!("{shown} is still loading — ask again in a moment")));
    }
    let Some(b) = crate::lsp::from_position(&doc.text, pos, enc) else { return };
    let Some(byte) = on_line(&doc.text, b, &q.word) else {
        // (never asked at a guess)
        let line = doc.text.byte_to_line(b) + 1;
        return ed.set_status(q.said(format!("`{}` isn't on {shown}:{line}, where it was found", q.word)));
    };
    let a = Asked {
        n: next(ed),
        cid: 0,
        doc: id,
        byte,
        goto,
        word: q.word,
        head: q.head,
        again: false,
        tries: 0,
    };
    send(ed, a);
}

// ── The pointer's rest (step 6, §2.4) ───────────────────────────────────────────────────────────────────────────────

/// What the pointer at the transcript's byte `at` rests on: a cited place whole, else a word — on its own letters only,
/// never a number — its bytes (`hoverrest::chat_spot`). Whether it's a name to ask about waits for the rest (`rest`).
pub(crate) fn rest_word(ed: &Editor, at: usize) -> Option<Range<usize>> {
    let (_, d) = transcript(ed)?;
    if at >= d.text.len_bytes() {
        return None;
    }
    let l = d.text.byte_to_line(at);
    let start = d.text.line_to_byte(l);
    let here = d.text.line(l).to_string();
    let k = at - start;
    let place = crate::places::names_in(&here).into_iter().find(|(whole, ..)| whole.contains(&k));
    if let Some((whole, ..)) = place {
        return Some(start + whole.start..start + whole.end);
    }
    let on = here[k..].chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_');
    let w = word_at(&here, k).filter(|_| on)?;
    let number = here[w.clone()].chars().all(|c| c.is_ascii_digit());
    (!number).then(|| start + w.start..start + w.end)
}

/// The pointer's rest `n` came at the transcript's byte `at` (`hoverrest::land`): asked as `space k` asks it there — a
/// cited place's lines shown, a name asked at its spot in its file or by name — its answer the rest's card. What can't
/// be asked shows nothing and says nothing (the pointer asked nothing); what's still being read is tried once more.
pub fn rest(ed: &mut Editor, at: usize, n: u64) {
    use crate::hoverrest::{busy, failed};
    match target_at(ed, at) {
        Err(why) if why == WAIT => busy(ed, n),
        Err(_) | Ok(Target::Locals { .. }) => failed(ed, n),
        Ok(Target::Place { link, peek, .. }) => match peek {
            Some(lines) if !lines.is_empty() => {
                let (md, lang) = peek_md(&link, &lines);
                crate::hoverrest::shows(ed, n, &md, lang.as_deref());
            }
            _ => busy(ed, n),
        },
        Ok(Target::At(spot)) => rest_in(ed, n, &spot.path, |text| {
            let now = text.get_line(spot.line).map(|l| l.to_string());
            let same = now.as_deref().map(|l| l.trim_end_matches(['\n', '\r'])) == Some(spot.text.as_str());
            same.then(|| text.line_to_byte(spot.line) + spot.col)
        }),
        Ok(Target::Named { word, container, .. }) => {
            let q = Named { n: next(ed), rest: Some(n), ..Named::new(word, container, None, None) };
            let root = ed.workspace.root.clone();
            match ed.ask_named(&q.word, Kind::ChatNamed { n: q.n }, &root) {
                Err(_) => failed(ed, n),
                Ok(asked) => {
                    let (waiting, servers) = asked.into_iter().unzip();
                    if let Some(c) = ed.workspace.claude.chat.as_mut() {
                        c.links.resting = Some(Named { waiting, servers, ..q });
                    }
                }
            }
        }
    }
}

/// Every server asked has answered the rest's question by name `q`: one definition, its hover asked there; still
/// loading, tried once more; none or several (`gd` lists them), nothing.
fn rest_found(ed: &mut Editor, mut q: Named) {
    let Some(n) = q.rest else { return };
    let found = defs(&mut q);
    match found.as_slice() {
        [] if !q.busy.is_empty() => crate::hoverrest::busy(ed, n),
        [(cid, path, pos, _)] => {
            let (enc, pos, word) = (ed.encoding_of(*cid), pos.clone(), q.word);
            rest_in(ed, n, path, |text| on_line(text, crate::lsp::from_position(text, &pos, enc)?, &word));
        }
        _ => crate::hoverrest::failed(ed, n),
    }
}

/// Rest `n`'s hover asked in `path`'s buffer at the byte `at` finds in its text: the file still loading, tried once
/// more; nothing there to ask at, or no server ready, nothing shown. Only a buffer open already, any session's — a rest
/// is a reveal: it never opens or lists one, reads the disk, raises a card or starts a server (`nav::buffer_for` does,
/// for a key); a file no buffer holds shows nothing.
fn rest_in(ed: &mut Editor, n: u64, path: &Path, at: impl FnOnce(&ropey::Rope) -> Option<usize>) {
    let Some(doc) = ed.docs.by_path(path) else { return crate::hoverrest::failed(ed, n) };
    let id = doc.id;
    if doc.loading {
        return crate::hoverrest::busy(ed, n);
    }
    match at(&doc.text) {
        Some(byte) if ed.lsp_hover_at(id, byte, n) => {}
        _ => crate::hoverrest::failed(ed, n),
    }
}

/// The front chat's underlines as kept: the files' listing they're for, and each name asked with whether it came to a
/// definition (the simulation's check).
#[cfg(test)]
pub(crate) fn underlines(ed: &Editor) -> Option<(u64, Vec<(String, bool)>)> {
    let l = &ed.workspace.claude.chat.as_ref()?.links;
    let mut words: Vec<(String, bool)> = l.words.iter().map(|(w, &f)| (w.to_string(), f)).collect();
    crate::sorting::by_key(&mut words, |w| w.0.clone());
    Some((l.listing, words))
}

/// The front chat's maps, each message's: whether it's stale, and whether a buffer it read was edited since (the
/// simulation's check).
#[cfg(test)]
pub(crate) fn maps(ed: &Editor) -> Vec<(usize, bool, bool)> {
    let Some(c) = ed.workspace.claude.chat.as_ref() else { return Vec::new() };
    let mut out: Vec<_> = c.links.maps.iter().map(|(i, mp)| (*i, mp.stale, behind(&ed.docs, mp))).collect();
    out.sort_unstable();
    out
}

/// The front chat's question by name in flight or waiting: its number, the name, its container, and what it asks.
#[cfg(test)]
pub(crate) fn named(ed: &Editor) -> Option<(u64, String, Option<String>, Option<Goto>)> {
    let q = ed.workspace.claude.chat.as_ref()?.links.named.as_ref()?;
    Some((q.n, q.word.clone(), q.container.clone(), q.goto))
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
