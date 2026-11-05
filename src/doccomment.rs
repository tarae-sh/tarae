//! Doc comment rendering (like IntelliJ's "Rendered documentation comments") — detection, unwrapping (pure).
//!
//! Strips the markers its language's pack names (`DocComments`: `///` `//!` for Rust, `/** … */` for Java·Kotlin·
//! JS·TS·C++ …) and shows them as markdown, **folded**: a block's text regrouped into chunks (`chunks`) and flowed
//! to the width, its rows on its first line and none on the rest (`codeview::Measure`; `j`/`k` step over it as one
//! line) — the document's lines are untouched, so line numbers hold. A block whose text is HTML (Javadoc's) and the
//! inline tags a pack lists (`{@code …}`) are read as Markdown first (`html.rs`), so one renderer draws them all.
//! Where it's selected or edited, that whole block shows as source (`Editor::doc_block_folded`, the one rule).
//! Drawing: term.rs.

mod html;

/// A language's doc comments (its pack's `doc-comments`): which comments are docs and how their text reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocComments {
    /// Line markers whose text is a doc, in Markdown (`///`).
    pub line: Vec<String>,
    /// A block doc's opener and closer (`/**`, `*/` — the language's block comment end); its lines' leading mark
    /// (the opener's last character, `*`) dropped.
    pub block: Option<(String, String)>,
    /// A block's text is HTML (Javadoc's), not Markdown — line docs stay Markdown.
    pub html: bool,
    /// Inline tags (`{@code …}`) and how each is drawn; one not listed stays as written.
    pub inline: Vec<(String, InlineKind)>,
    /// Block tags drawn as a labelled section (`@param` → `Params`); one not listed is drawn `@name …`.
    pub tags: Vec<TagSpec>,
}

/// How an inline tag `{@name body}` is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineKind {
    /// Its body as code.
    Code,
    /// A reference (`split_ref`) as a link set as code — its label when it has one, else the reference shortened.
    Link,
    /// The same as plain link text.
    PlainLink,
    /// Its body as text.
    Text,
}

/// A block tag drawn as a section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagSpec {
    pub name: String,
    /// The section's name (`Params`).
    pub label: String,
    pub arg: TagArg,
}

/// What a block tag's first word is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagArg {
    /// Nothing apart: all of it is the description.
    None,
    /// A name, drawn as code (`@param key`) — a `{Type}` before it too (JSDoc's).
    Name,
    /// A reference, drawn as a link (`@throws java.io.IOException` → `IOException`).
    Ref,
}

impl DocComments {
    /// Whether a line whose text starts with `c` can start a doc comment.
    fn starts(&self, c: char) -> bool {
        self.line.iter().chain(self.block.as_ref().map(|b| &b.0)).any(|m| m.starts_with(c))
    }

    /// The tag `name`'s spec, when it's drawn as a section.
    pub fn tag(&self, name: &str) -> Option<&TagSpec> {
        self.tags.iter().find(|t| t.name == name)
    }
}

/// This language's doc comments (its pack's `doc-comments`) — none in an unknown file, nor in a language without
/// them (`///` inside a markdown code block is left as is).
pub fn syntax(lang: Option<&str>) -> Option<&'static DocComments> {
    lang.and_then(crate::syntax::spec).and_then(|s| s.doc_comments.as_ref())
}

/// Whether a change shown on screen is in or right next to lines `a..=b` — a code review's or `:diff`'s hunk
/// (or an edit of Claude's that follow shows): a changed or added line among them or on either side, or removed
/// rows inside them or right against them; or a merge conflict's lines. Such a block stays raw, so a diff is
/// always read against the text it changes.
pub fn touches_change(doc: &crate::document::Document, a: usize, b: usize) -> bool {
    crate::codeview::Changes::of(doc).near(doc, a, b)
        || doc.conflicts.iter().any(|c| c.start <= b + 1 && c.end + 1 >= a)
}

/// One doc comment line, unwrapped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    /// One markdown line (including blank lines).
    Text(String),
    /// ```` ```lang ```` — code block start (language name, empty string if none).
    FenceOpen(String),
    FenceClose,
    /// One line inside a code block.
    Code(String),
}

/// One doc comment line: (indent bytes, text after the marker, whether it's a block doc's line).
pub type DocLine = (usize, String, bool);

/// Per line: if a doc comment in `syn`'s markers, its `DocLine`. `in_block` = continuing line in `/**`. The
/// returned bool = whether the next line is also inside the block.
pub fn classify(line: &str, in_block: bool, syn: &DocComments) -> (Option<DocLine>, bool) {
    let line = line.trim_end_matches(['\n', '\r']);
    let t = line.trim_start();
    let indent = line.len() - t.len();
    let one_space = |s: &str| s.strip_prefix(' ').unwrap_or(s).to_string();
    let last = |m: &str| m.chars().last().unwrap_or(' ');
    if let Some((open, close)) = &syn.block {
        // Code after the closer (`/** @type {number} */ let x = 1;`) = a code line, not a doc line
        let code_after = |s: &str, i: usize| !s[i + close.len()..].trim().is_empty();
        let mark = last(open);
        if in_block {
            if let Some(i) = t.find(close.as_str()) {
                if code_after(t, i) {
                    return (None, false);
                }
                let before = t[..i].trim_end();
                let body = before.strip_prefix(mark).unwrap_or(before);
                return (Some((indent, one_space(body), true)), false);
            }
            let body = t.strip_prefix(mark).unwrap_or(t);
            return (Some((indent, one_space(body), true)), true);
        }
        // (`/**/` and `/***` aren't docs)
        let opens = |r: &&str| !r.starts_with(mark) && !r.starts_with(last(close));
        if let Some(rest) = t.strip_prefix(open.as_str()).filter(opens) {
            return match rest.find(close.as_str()) {
                Some(i) if code_after(rest, i) => (None, false),
                Some(i) => (Some((indent, one_space(rest[..i].trim_end()), true)), false),
                None => (Some((indent, one_space(rest), true)), true),
            };
        }
    }
    // (`////` isn't a `///` doc — a marker of one character repeated is no doc with one more of it, a rule line;
    // `//!!` is a `//!` doc, as Rust's grammar has it)
    let more = |m: &str, r: &str| m.chars().all(|c| c == last(m)) && r.starts_with(last(m));
    for m in &syn.line {
        if let Some(rest) = t.strip_prefix(m.as_str()).filter(|r| !more(m, r)) {
            return (Some((indent, one_space(rest), false)), false);
        }
    }
    (None, false)
}

/// `classify` over `lines`, in order. Only a line that can be one is copied out of the rope — outside a
/// block doc, one whose text starts as a marker does — so a screenful of code costs no allocation (drawing and
/// `j`/`k` scan hundreds of lines around the view each time).
pub fn scan<'a>(
    text: &'a ropey::Rope,
    lines: std::ops::RangeInclusive<usize>,
    syn: &'a DocComments,
) -> impl Iterator<Item = Option<DocLine>> + 'a {
    let (mut in_block, mut buf) = (false, String::new());
    text.lines_at(*lines.start()).take(lines.count()).map(move |line| {
        let head = line.chars().take(2000);
        if !in_block && !head.clone().find(|c| !c.is_whitespace()).is_some_and(|c| syn.starts(c)) {
            return None;
        }
        buf.clear();
        buf.extend(head);
        let (c, next) = classify(&buf, in_block, syn);
        in_block = next;
        c
    })
}

/// A run of doc comment lines: lines `start..=end`, the first one's indent, each line's text after its marker.
pub struct Block {
    pub start: usize,
    pub end: usize,
    /// The display column its marker starts at (tabs to their stops) — where its folded rows are drawn.
    pub indent: usize,
    pub bodies: Vec<String>,
    /// Its language's doc comments.
    pub syn: &'static DocComments,
    /// Its text is HTML: a block doc (it starts with one) in a language whose are (`DocComments.html`).
    pub html: bool,
}

/// A block laid out folded: its lines, its first line's indent, its rows (spans, is example code).
pub struct Folded {
    pub start: usize,
    pub end: usize,
    pub indent: usize,
    pub rows: Vec<(crate::codeview::Spans, bool)>,
    /// Each row's source bytes (`row_spans`).
    pub at: Vec<(usize, usize)>,
}

pub type FoldsKey = (crate::document::DocId, u64, (usize, usize), String, Option<String>);

/// Every doc comment block of a document laid out folded, for `key` = (document, version, width and tab width,
/// theme, the language its grammar parses — example code is coloured by it: a grammar loading lays it out again).
pub struct Folds {
    pub key: FoldsKey,
    /// In order of their lines.
    pub blocks: Vec<Folded>,
}

/// The doc comment blocks among `lines` (`scan` over them: start it far enough above to join a `/** */`), tabs
/// `tab` wide.
pub fn blocks(
    text: &ropey::Rope,
    lines: std::ops::RangeInclusive<usize>,
    tab: usize,
    syn: &'static DocComments,
) -> Vec<Block> {
    let w0 = *lines.start();
    let info: Vec<Option<DocLine>> = scan(text, lines, syn).collect();
    runs(&info, syn)
        .into_iter()
        .filter_map(|r| {
            let (indent, _, in_block) = info[r.start].as_ref()?;
            let bodies =
                info[r.clone()].iter().map(|x| x.as_ref().map(|v| v.1.clone()).unwrap_or_default()).collect();
            let indent = crate::movement::visual_col(text, text.line_to_byte(w0 + r.start) + indent, tab);
            let html = syn.html && *in_block;
            Some(Block { start: w0 + r.start, end: w0 + r.end - 1, indent, bodies, syn, html })
        })
        .collect()
}

/// The blocks among `scan`'s lines, as index ranges: runs of doc lines — split where a block doc's lines meet
/// line docs in a language whose block docs are HTML (Java's `/** */` beside its `///`), as each reads apart.
fn runs(info: &[Option<DocLine>], syn: &DocComments) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < info.len() {
        let Some((_, _, kind)) = info[i] else {
            i += 1;
            continue;
        };
        let start = i;
        while i < info.len() && info[i].as_ref().is_some_and(|d| !syn.html || d.2 == kind) {
            i += 1;
        }
        out.push(start..i);
    }
    out
}

/// Whether range `from..to` of `text` is on lines `a..=b`: its lines are those of its first and last characters (a line
/// selected above, its newline its last character, isn't the next one).
pub fn on_lines(text: &ropey::Rope, (from, to): (usize, usize), (a, b): (usize, usize)) -> bool {
    let last = if to > from { crate::graphemes::prev_boundary(text, to) } else { from };
    text.byte_to_line(from.min(text.len_bytes())) <= b && text.byte_to_line(last.min(text.len_bytes())) >= a
}

/// Whether line `l` of a doc comment holds no words: blank once its markers are off (`///`, ` * `, `/**`, `#`) — what
/// parts its paragraphs, as a blank row parts them folded.
pub fn blank_line(text: &ropey::Rope, l: usize) -> bool {
    text.line(l).to_string().trim().trim_start_matches(['/', '*', '!', '#']).trim().is_empty()
}

/// The lines of block `a..=b` each of its folded `rows`' words come from: its paragraph's — a run of blank lines after
/// words parts two, in the comment as in its rows —, the first from the block's first line, the last to its last; a
/// blank row between two paragraphs the blank lines between them (`row_spans` parts a paragraph's among its rows).
fn row_lines(
    text: &ropey::Rope,
    (a, b): (usize, usize),
    rows: &[(crate::codeview::Spans, bool)],
) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut open = false;
    for l in a..=b {
        match (blank_line(text, l), open) {
            (true, _) => open = false,
            (false, true) => runs.last_mut().expect("open").1 = l,
            (false, false) => {
                runs.push((l, l));
                open = true;
            }
        }
    }
    let n = runs.len();
    let para = |k: usize| {
        if n == 0 {
            return (a, b);
        }
        let k = k.min(n - 1);
        (if k == 0 { a } else { runs[k].0 }, if k + 1 == n { b } else { runs[k].1 })
    };
    // (the blank lines between paragraphs `k` and `k + 1`, else `k`'s own when none are)
    let gap = |k: usize| match (runs.get(k), runs.get(k + 1)) {
        (Some(x), Some(y)) if y.0 > x.1 + 1 => (x.1 + 1, y.0 - 1),
        _ => para(k),
    };
    let (mut p, mut words) = (0, false);
    rows.iter()
        .map(|(spans, _)| match (spans.iter().all(|s| s.text.trim().is_empty()), words) {
            (false, _) => {
                words = true;
                para(p)
            }
            (true, true) => {
                (p, words) = (p + 1, false);
                gap(p - 1)
            }
            (true, false) => p.checked_sub(1).map_or_else(|| para(0), gap),
        })
        .collect()
}

/// The source bytes each of block `a..=b`'s folded `rows` shows: a paragraph's (`row_lines`) parted among its rows where
/// each row's first word is found (`places`) — its first row from its first line's start, its last to its last line's
/// end; a blank row between two paragraphs the blank lines between them. What a selection over a folded block lights
/// (#269) and the row its cursor is kept in sight on (`codeview::Measure::row_at`, #386).
pub fn row_spans(
    text: &ropey::Rope,
    (a, b): (usize, usize),
    rows: &[(crate::codeview::Spans, bool)],
) -> Vec<(usize, usize)> {
    let lines = row_lines(text, (a, b), rows);
    let mut out = Vec::with_capacity(rows.len());
    let mut k = 0;
    while k < rows.len() {
        // (the run of rows its paragraph takes)
        let e = k + lines[k..].iter().take_while(|l| **l == lines[k]).count();
        let from = crate::movement::line_start(text, lines[k].0);
        let to = crate::movement::line_full_end(text, lines[k].1);
        let mut starts = vec![from];
        let mut p = from;
        for (r, (spans, _)) in rows.iter().enumerate().take(e).skip(k) {
            let found = places(text, (p, to), spans);
            if r > k {
                starts.push(found.first().map_or(p, |w| w.at).max(p));
            }
            p = found.last().map_or(p, |w| w.at + w.word.len());
        }
        starts.push(to);
        out.extend(starts.windows(2).map(|w| (w[0], w[1].max(w[0]))));
        k = e;
    }
    out
}

/// A word of a folded row found in its source: the cell it starts at among the row's words, its byte there, its text.
#[derive(Debug, PartialEq, Eq)]
pub struct Place {
    pub col: usize,
    pub at: usize,
    pub word: String,
}

/// How far past the last word found the next is looked for: a link's address, a tag's markup between two words.
const NEAR: usize = 400;

/// The words of a folded row (`spans`) found in its source `from..to`, in order — each where it starts a word there (no
/// letter or digit just before it: `wind` in `` `wind` ``), the first within `NEAR` bytes of the last one found, else
/// its letters and digits alone (`IOException` of `IOException.`); a word drawn but not written (a section's label, a
/// bullet, an entity's character) left out. What a press on the row lands on (`place_at`) and where `row_spans` parts a
/// paragraph.
pub fn places(text: &ropey::Rope, (from, to): (usize, usize), spans: &[crate::markdown::Span]) -> Vec<Place> {
    use unicode_width::UnicodeWidthStr;
    let (from, to) = (from.min(text.len_bytes()), to.min(text.len_bytes()));
    let src = text.byte_slice(from.min(to)..to).to_string();
    let shown: String = spans.iter().map(|s| s.text.as_str()).collect();
    let mut out = Vec::new();
    let mut p = 0;
    let mut col = 0;
    for part in shown.split_inclusive(char::is_whitespace) {
        let word = part.trim_end();
        let at_col = col;
        col += part.width();
        // (the word whole, else its letters and digits — the cells before them counted)
        let bare = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        let tries = [(word, 0), (bare, word.find(bare).map_or(0, |i| word[..i].width()))];
        let end = (p..=src.len().min(p + NEAR)).rev().find(|&i| src.is_char_boundary(i)).unwrap_or(p);
        let window = &src[p..end];
        let found = tries.iter().filter(|(w, _)| !w.is_empty()).find_map(|&(w, skip)| {
            let starts = |i: &usize| !src[..p + i].chars().next_back().is_some_and(|c| c.is_alphanumeric());
            window.match_indices(w).map(|(i, _)| i).find(starts).map(|i| (p + i, w, skip))
        });
        if let Some((i, w, skip)) = found {
            out.push(Place { col: at_col + skip, at: from + i, word: w.to_string() });
            p = i + w.len();
        }
    }
    out
}

/// The byte a press `cell` cells into a folded row's words lands on, its source `from..to`: the character under it in
/// the word there, the end of the word before it past one (as a press past a line's end on code), the first word's
/// start before them all; no word found, `from`.
pub fn place_at(
    text: &ropey::Rope,
    (from, to): (usize, usize),
    spans: &[crate::markdown::Span],
    cell: usize,
) -> usize {
    use unicode_width::UnicodeWidthChar;
    let found = places(text, (from, to), spans);
    let Some(w) = found.iter().rev().find(|w| w.col <= cell).or(found.first()) else { return from };
    let mut c = w.col;
    for (i, ch) in w.word.char_indices() {
        c += ch.width().unwrap_or(0);
        if c > cell {
            return w.at + i;
        }
    }
    w.at + w.word.len()
}

/// Line range [start, end] of the doc comment block containing `line` (within 200 lines up/down — joins
/// a `/** */` that starts above).
pub fn block_at(text: &ropey::Rope, line: usize, syn: &DocComments) -> Option<(usize, usize)> {
    let last = text.len_lines().saturating_sub(1);
    if line > last {
        return None;
    }
    let (w0, w1) = (line.saturating_sub(200), (line + 200).min(last));
    let info: Vec<Option<DocLine>> = scan(text, w0..=w1, syn).collect();
    let i = line - w0;
    let r = runs(&info, syn).into_iter().find(|r| r.contains(&i))?;
    Some((w0 + r.start, w0 + r.end - 1))
}

/// What a block draws per line: its text as Markdown (`parts`) — a block of HTML read as Markdown first, a
/// Markdown one with its pack's inline tags and tag arguments rewritten (`html.rs`).
pub fn parts_of(b: &Block) -> Vec<Part> {
    if b.html {
        return parts(&html::markdown(&b.bodies, b.syn));
    }
    let mut out = parts(&b.bodies);
    if !b.syn.inline.is_empty() || !b.syn.tags.is_empty() {
        for p in &mut out {
            if let Part::Text(s) = p {
                *s = html::markdown_line(s, b.syn);
            }
        }
    }
    out
}

/// Text of one block (consecutive doc comment lines) → what to draw per line (inside/outside code blocks).
pub fn parts(bodies: &[String]) -> Vec<Part> {
    let mut in_fence = false;
    bodies
        .iter()
        .map(|b| {
            let t = b.trim_start();
            if let Some(info) = t.strip_prefix("```") {
                in_fence = !in_fence;
                if in_fence {
                    return Part::FenceOpen(info.split([',', ' ']).next().unwrap_or_default().to_string());
                }
                return Part::FenceClose;
            }
            if in_fence { Part::Code(b.clone()) } else { Part::Text(b.clone()) }
        })
        .collect()
}

/// Chunks of a folded (compacted) doc comment — regrouped by meaning, not by line breaks.
/// Consecutive text lines are merged into one paragraph and rewrapped to the width (markdown rules — same as
/// rustdoc), blank lines become one line between paragraphs (one even if several), code fence lines
/// are dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chunk {
    Para(String),
    Heading(String),
    Item(String),
    /// `@param x description` — (tag name, rest).
    Tag(String, String),
    /// One line of example code (verbatim).
    Code(String),
    /// Blank line (between paragraphs — several collapse into one, none at start/end).
    Gap,
}

pub fn chunks(parts: &[Part]) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();
    let mut open = false; // can we append to the last text chunk
    for p in parts {
        match p {
            Part::Text(s) => {
                let t = s.trim();
                if t.is_empty() {
                    open = false;
                    if out.last().is_some_and(|c| *c != Chunk::Gap) {
                        out.push(Chunk::Gap);
                    }
                    continue;
                }
                let hashes = t.bytes().take_while(|&b| b == b'#').count();
                if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
                    out.push(Chunk::Heading(t[hashes..].trim().to_string()));
                    open = false;
                } else if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
                    out.push(Chunk::Item(rest.to_string()));
                    open = true;
                } else if let Some(tag) = t.strip_prefix('@') {
                    let (name, rest) = tag.split_once(' ').unwrap_or((tag, ""));
                    out.push(Chunk::Tag(name.to_string(), rest.trim().to_string()));
                    open = true;
                } else if open
                    && let Some(Chunk::Para(p) | Chunk::Item(p) | Chunk::Tag(_, p)) = out.last_mut()
                {
                    if !p.is_empty() {
                        p.push(' ');
                    }
                    p.push_str(t);
                } else {
                    out.push(Chunk::Para(t.to_string()));
                    open = true;
                }
            }
            Part::Code(s) => {
                out.push(Chunk::Code(s.clone()));
                open = false;
            }
            Part::FenceOpen(_) | Part::FenceClose => open = false,
        }
    }
    if out.last() == Some(&Chunk::Gap) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(lang: &str) -> &'static DocComments {
        syntax(Some(lang)).expect("its pack has doc comments")
    }

    /// #269: each folded row's lines — its paragraph's, the first from the block's first line (a `/**`), the last to its
    /// last (a `*/`), a blank row between two the blank lines between them, every row of a flowed paragraph alike.
    #[test]
    fn each_folded_row_knows_its_paragraphs_lines() {
        let text = ropey::Rope::from_str("/**\n * One.\n *\n *\n * Two, a\n * paragraph.\n */\n");
        let row = |words: &str| {
            let span = crate::markdown::Span {
                cut: None,
                hint: false,
                text: words.into(),
                style: crate::theme::Style::default(),
            };
            (crate::codeview::Spans::from(vec![span]), false)
        };
        let rows = [row("One."), row(""), row("Two, a"), row("paragraph.")];
        assert_eq!(row_lines(&text, (0, 6), &rows), [(0, 1), (2, 3), (4, 6), (4, 6)]);
        assert_eq!(row_lines(&text, (1, 1), &rows[..1]), [(1, 1)], "one line, one row");
    }

    /// #385, #386: each folded row's source — a paragraph parted where its next row's first word is written (through
    /// its markup: `` `wind` ``, `**two**`), the first row from its first line, the last to its last — and a press on a
    /// row lands on the character under it there, past a word's end at that end, before the first word on it; a word
    /// drawn but not written (a section's label) is passed over.
    #[test]
    fn a_folded_rows_words_are_found_in_its_source() {
        let src = "/**\n * Calls `wind` on\n * **two** spools.\n *\n * Then stops.\n */\n";
        let text = ropey::Rope::from_str(src);
        let row = |words: &str| {
            let span = crate::markdown::Span {
                cut: None,
                hint: false,
                text: words.into(),
                style: crate::theme::Style::default(),
            };
            (crate::codeview::Spans::from(vec![span]), false)
        };
        let rows = [row("Calls wind on two"), row("spools."), row(""), row("Then stops.")];
        let at = |s: &str| src.find(s).expect(s);
        let line = |l: usize| text.line_to_byte(l);
        assert_eq!(
            row_spans(&text, (0, 5), &rows),
            [(0, at("spools")), (at("spools"), line(3)), (line(3), line(4)), (line(4), src.len())]
        );
        let place = |r: usize, cell: usize| {
            let spans = row_spans(&text, (0, 5), &rows);
            place_at(&text, spans[r], &rows[r].0, cell)
        };
        assert_eq!(place(0, 7), at("ind` on"), "the `i` of `wind`, in its backticks");
        assert_eq!(place(0, 14), at("two"), "`two`, inside its stars");
        assert_eq!(place(0, 17), at("two") + 3, "past the row's last word: its end");
        assert_eq!(place(1, 0), at("spools"), "a row's first word");
        assert_eq!(place(3, 5), at("stops"), "the paragraph after a gap");
        let label = [row("Params  key the key")];
        let text = ropey::Rope::from_str("/// @param key the key\n");
        let found: Vec<_> =
            places(&text, (0, text.len_bytes()), &label[0].0).into_iter().map(|p| p.word).collect();
        assert_eq!(found, ["key", "the", "key"], "the label not written: passed over");
    }

    #[test]
    fn rust_and_block_comments() {
        let rust = of("rust");
        let c = |line: &str, in_block: bool| classify(line, in_block, rust);
        assert_eq!(c("    /// Returns `x`.", false), (Some((4, "Returns `x`.".into(), false)), false));
        assert_eq!(c("//! crate docs", false).0, Some((0, "crate docs".into(), false)));
        assert_eq!(c("//// not a doc", false).0, None);
        assert_eq!(
            c("//!! inner docs", false).0,
            Some((0, "! inner docs".into(), false)),
            "Rust's grammar's"
        );
        assert_eq!(classify("//!! a module's", false, of("zig")).0, Some((0, "! a module's".into(), false)));
        assert_eq!(classify("//////////", false, of("zig")).0, None);
        assert_eq!(c("// plain", false).0, None);
        // /** … */ multi-line
        assert_eq!(c("  /**", false), (Some((2, String::new(), true)), true));
        assert_eq!(
            c("   * Adds **two** numbers.", true),
            (Some((3, "Adds **two** numbers.".into(), true)), true)
        );
        assert_eq!(c("   */", true), (Some((3, String::new(), true)), false));
        assert_eq!(c("/** one line */", false), (Some((0, "one line".into(), true)), false));
        assert_eq!(c("/**/", false).0, None);
        assert_eq!(c("/*** banner", false).0, None);
    }

    /// Which comments are docs is each language's: TypeScript's `///` is a directive, Java's `//!` a comment, Zig
    /// has no `/**`; Java's block is HTML, its `///` Markdown.
    #[test]
    fn each_language_names_its_own_doc_comments() {
        let is_doc = |lang: &str, line: &str| classify(line, false, of(lang)).0.is_some();
        assert!(!is_doc("typescript", "/// <reference path=\"a.d.ts\" />"));
        assert!(is_doc("typescript", "/** A thing. */"));
        assert!(!is_doc("java", "//! not a doc"));
        assert!(is_doc("java", "/// A **Markdown** doc"));
        assert!(!is_doc("zig", "/** no */"));
        assert!(is_doc("zig", "//! a module's doc"));
        assert_eq!(syntax(Some("python")), None);
        // (the fast path: a line copied only where a marker can start it)
        let ts = ropey::Rope::from_str("/// <reference />\nlet a;\n");
        assert!(blocks(&ts, 0..=1, 4, of("typescript")).is_empty());
    }

    /// Where a language's block docs are HTML and its line docs Markdown (Java), a block doc beside line docs is a
    /// block of its own either way round, each read as itself — and `block_at` (`j`/`k`) says the same blocks;
    /// elsewhere (Rust, both Markdown) they stay one.
    #[test]
    fn html_block_docs_and_markdown_line_docs_are_blocks_apart() {
        let spans = |text: &str, lang: &str| {
            let text = ropey::Rope::from_str(text);
            let last = text.len_lines() - 1;
            let b = blocks(&text, 0..=last, 4, of(lang));
            for b in &b {
                let at = |l| block_at(&text, l, of(lang));
                assert!(
                    (b.start..=b.end).all(|l| at(l) == Some((b.start, b.end))),
                    "block_at agrees: {lang}"
                );
            }
            b.iter().map(|b| (b.start, b.end, b.html)).collect::<Vec<_>>()
        };
        let java = "/**\n * <p>A\n */\n/// B **md**\n\nint x;\n/// C\n/** <p>D</p> */\n";
        assert_eq!(spans(java, "java"), [(0, 2, true), (3, 3, false), (6, 6, false), (7, 7, true)]);
        assert_eq!(spans("/// a\n/** b */\n/// c\n", "rust"), [(0, 2, false)]);
    }

    /// A closing `*/` followed by code ends the block on a code line — the code isn't taken as docs.
    #[test]
    fn code_after_block_close_is_not_doc() {
        let js = of("javascript");
        assert_eq!(classify("/** @type {number} */ let x = 1;", false, js), (None, false));
        assert_eq!(classify(" * foo */ bar();", true, js), (None, false));
        assert_eq!(classify(" * foo */", true, js), (Some((1, "foo".into(), true)), false));
        let text = ropey::Rope::from_str("/** @type {number} */ let x = 1;\nlet y = 2;\n");
        assert_eq!(block_at(&text, 0, js), None);
        assert_eq!(block_at(&text, 1, js), None, "the block closed on line 0");
        let text = ropey::Rope::from_str("/**\n * doc\n */ let a = 1;\nlet b = 2;\n");
        assert_eq!(block_at(&text, 1, js), Some((0, 1)));
        assert_eq!(block_at(&text, 3, js), None);
    }

    #[test]
    fn chunks_join_paragraphs_and_drop_blank_lines() {
        let b: Vec<String> = [
            "# Title",
            "",
            "First line",
            "continues here.",
            "",
            "- item one",
            "  more",
            "@param x the x",
            "```",
            "let a = 1;",
            "```",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            chunks(&parts(&b)),
            [
                Chunk::Heading("Title".into()),
                Chunk::Gap,
                Chunk::Para("First line continues here.".into()),
                Chunk::Gap,
                Chunk::Item("item one more".into()),
                Chunk::Tag("param".into(), "x the x".into()),
                Chunk::Code("let a = 1;".into()),
            ]
        );
    }

    /// "Right beside" is a change on the line just above or just below the block (added, or removed rows going
    /// there); with a line between them the block is apart from it.
    #[test]
    fn a_change_right_beside_a_block_touches_it() {
        let mut ed = crate::editor::Editor::new(crate::config::Config::default());
        let doc = &mut ed.docs[0];
        let block = "/// One.\n/// Two.\n";
        // `now` under review against `base`: does the block (its lines found in `now`) touch the change?
        let mut touches = |base: &str, now: &str| {
            doc.text = ropey::Rope::from_str(now);
            doc.review_base = Some(ropey::Rope::from_str(base));
            doc.git_hunks = crate::git::hunks(base, now);
            let a = now.lines().position(|l| l == "/// One.").unwrap();
            touches_change(doc, a, a + 1)
        };
        // Added just above / just below — and with a line between
        assert!(touches(&format!("a\n{block}b\n"), &format!("a\nnew\n{block}b\n")), "added above");
        assert!(touches(&format!("a\n{block}b\n"), &format!("a\n{block}new\nb\n")), "added below");
        assert!(!touches(&format!("a\n{block}b\n"), &format!("new\na\n{block}b\n")), "added a line above");
        assert!(!touches(&format!("a\n{block}b\n"), &format!("a\n{block}b\nnew\n")), "added a line below");
        // Removed just above / just below (its rows go right against the block) — and with a line between
        assert!(touches(&format!("a\ngone\n{block}b\n"), &format!("a\n{block}b\n")), "removed above");
        assert!(touches(&format!("a\n{block}gone\nb\n"), &format!("a\n{block}b\n")), "removed below");
        assert!(!touches(&format!("gone\na\n{block}b\n"), &format!("a\n{block}b\n")), "removed a line above");
        assert!(!touches(&format!("a\n{block}b\ngone\n"), &format!("a\n{block}b\n")), "removed a line below");
    }

    #[test]
    fn fences_split_code_from_text() {
        let b: Vec<String> = ["Example:", "```rust", "let x = 1;", "```", "done"].map(String::from).to_vec();
        assert_eq!(
            parts(&b),
            [
                Part::Text("Example:".into()),
                Part::FenceOpen("rust".into()),
                Part::Code("let x = 1;".into()),
                Part::FenceClose,
                Part::Text("done".into())
            ]
        );
    }
}
