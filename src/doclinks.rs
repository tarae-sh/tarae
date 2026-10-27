//! Every relative link in the docs and the README goes somewhere (frictions #131): its file is there, and its
//! `#anchor` is a heading of that file as GitHub names it — so a heading renamed can't leave a link behind.
//! Links in code (fences, code spans) aren't links; a link with a scheme (`https:`, `mailto:`) isn't checked.

use std::collections::BTreeSet;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

/// A heading's anchor as GitHub makes it (github-slugger): the text as shown, lowercased, every character but
/// letters, digits, `-`, `_` and spaces taken out, each space a `-`.
fn slug(shown: &str) -> String {
    shown
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' '))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// A text cut into its pieces and its code spans (`true`, a span's content as written), as byte ranges.
fn spans(text: &str) -> Vec<(bool, Range<usize>)> {
    let mut out = Vec::new();
    let (mut at, mut from) = (0, 0);
    while let Some(i) = text[at..].find('`').map(|i| at + i) {
        let n = text[i..].len() - text[i..].trim_start_matches('`').len();
        let fence = &text[i..i + n];
        // The span closes at the next run of exactly as many backticks; none: the backticks are text.
        let mut close = None;
        let mut j = i + n;
        while let Some(k) = text[j..].find(fence).map(|k| j + k) {
            let m = text[k..].len() - text[k..].trim_start_matches('`').len();
            if m == n {
                close = Some(k);
                break;
            }
            j = k + m;
        }
        match close {
            Some(k) => {
                out.push((false, from..i));
                out.push((true, i + n..k));
                (at, from) = (k + n, k + n);
            }
            None => at = i + n,
        }
    }
    out.push((false, from..text.len()));
    out
}

/// An ATX heading's text after its `#`s, as written (its closing run too); None: the line isn't one.
fn heading(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let level = t.len() - t.trim_start_matches('#').len();
    let is = line.len() - t.len() <= 3 && (1..=6).contains(&level) && t[level..].starts_with([' ', '\t']);
    is.then(|| &t[level..])
}

/// Whether `line` starts a block of its own, ending the paragraph before it: a heading, a list item, a table's row.
fn opens(line: &str) -> bool {
    let t = line.trim_start();
    let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    heading(line).is_some()
        || t.starts_with('|')
        || ["- ", "* ", "+ "].iter().any(|m| t.starts_with(m))
        || (digits > 0 && [". ", ") "].iter().any(|m| t[digits..].starts_with(m)))
}

/// A file's prose (`prose`), each line with its pieces outside code spans. A span is paired across a paragraph's
/// lines, as the docs are wrapped: a backtick that opens on one line may close on the next. A paragraph ends at a blank
/// line, a fence and a heading, and before a block that `opens`.
fn outside_code(text: &str) -> Vec<(usize, &str, Vec<&str>)> {
    let lines = prose(text);
    let at = |s: &str| s.as_ptr() as usize - text.as_ptr() as usize;
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let mut j = i + 1;
        while j < lines.len()
            && lines[j].0 == lines[j - 1].0 + 1
            && !lines[j - 1].1.trim().is_empty()
            && heading(lines[j - 1].1).is_none()
            && !lines[j].1.trim().is_empty()
            && !opens(lines[j].1)
        {
            j += 1;
        }
        let (from, to) = (at(lines[i].1), at(lines[j - 1].1) + lines[j - 1].1.len());
        let pieces: Vec<Range<usize>> = spans(&text[from..to])
            .into_iter()
            .filter(|(code, _)| !code)
            .map(|(_, r)| from + r.start..from + r.end)
            .collect();
        for &(n, line) in &lines[i..j] {
            let (a, b) = (at(line), at(line) + line.len());
            let mine = pieces
                .iter()
                .map(|p| p.start.max(a)..p.end.min(b))
                .filter(|p| p.start < p.end)
                .map(|p| &text[p])
                .collect();
            out.push((n, line, mine));
        }
        i = j;
    }
    out
}

/// What a heading's text shows: code spans as written, a link its text, no tags, no emphasis marks.
fn shown(heading: &str) -> String {
    let mut out = String::new();
    for (code, r) in spans(heading) {
        let s = &heading[r];
        if code {
            out.push_str(s);
            continue;
        }
        let mut rest = s;
        // A link's target isn't shown: `](…)` goes.
        let mut text = String::new();
        while let Some(i) = rest.find("](") {
            text.push_str(&rest[..i]);
            rest = rest[i + 2..].split_once(')').map_or("", |(_, after)| after);
        }
        text.push_str(rest);
        // Tags go, and the `_` that marks emphasis (never one inside a word: `snake_case` keeps it).
        let chars: Vec<char> = text.chars().collect();
        let mut tag = false;
        for (i, &c) in chars.iter().enumerate() {
            match c {
                '<' if chars[i + 1..].contains(&'>') => tag = true,
                '>' if tag => tag = false,
                _ if tag => {}
                '_' => {
                    let word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric());
                    if word(i.checked_sub(1).and_then(|p| chars.get(p))) && word(chars.get(i + 1)) {
                        out.push(c);
                    }
                }
                c => out.push(c),
            }
        }
    }
    out
}

/// A Markdown file's lines outside fenced code, numbered from 1.
fn prose(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (i, line) in text.lines().enumerate() {
        let t = line.trim_start();
        let mark = t.chars().next().filter(|c| matches!(c, '`' | '~'));
        let run = mark.map_or(0, |m| t.len() - t.trim_start_matches(m).len());
        match fence {
            Some((m, n)) => {
                if mark == Some(m) && run >= n && t[run..].trim().is_empty() {
                    fence = None;
                }
            }
            None if run >= 3 => fence = Some((mark.unwrap(), run)),
            None => out.push((i + 1, line)),
        }
    }
    out
}

/// The anchors a Markdown file has: its headings' (a repeated one `-1`, `-2`, … as GitHub numbers them) and every
/// `id="…"` or `name="…"` of its HTML.
fn anchors(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (_, line) in prose(text) {
        if let Some(text) = heading(line) {
            // A closing run of `#`s isn't the heading's.
            let text = text.trim();
            let text = match text.trim_end_matches('#') {
                cut if cut.is_empty() || cut.ends_with([' ', '\t']) => cut,
                _ => text,
            };
            let base = slug(&shown(text));
            let mut name = base.clone();
            let mut n = 0;
            while out.contains(&name) {
                n += 1;
                name = format!("{base}-{n}");
            }
            out.insert(name);
        }
        for attr in ["id=\"", "name=\""] {
            out.extend(attrs(line, attr).map(str::to_string));
        }
    }
    out
}

/// Each value of `attr` (`href="`) on a line, word-bounded (`data-href` isn't `href`).
fn attrs<'a>(line: &'a str, attr: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    line.match_indices(attr).filter_map(move |(i, _)| {
        let bounded = line[..i].ends_with(|c: char| c.is_whitespace());
        let value = &line[i + attr.len()..];
        bounded.then(|| value.split('"').next()).flatten()
    })
}

/// Every link target on a line, in its `pieces` outside code spans (`outside_code`): inline links and images
/// (`](target "title")`), a reference's definition (`[name]: target`, a line that doesn't start in code), HTML's
/// `href`, `src` and `srcset`.
fn targets<'a>(line: &'a str, pieces: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let t = line.trim_start();
    if line.len() - t.len() <= 3
        && pieces.first().is_some_and(|p| p.as_ptr() == line.as_ptr())
        && t.starts_with('[')
        && let Some((_, def)) = t.split_once("]:")
        && let Some(target) = def.split_whitespace().next()
    {
        out.push(target.trim_start_matches('<').trim_end_matches('>'));
    }
    for s in pieces {
        for (i, _) in s.match_indices("](") {
            let rest = s[i + 2..].trim_start();
            let target = match rest.strip_prefix('<') {
                Some(r) => r.split('>').next().unwrap_or(""),
                None => {
                    // Up to a space or the `)` that closes it (a target's own parentheses balanced).
                    let mut depth = 0;
                    let end = rest
                        .char_indices()
                        .find(|&(_, c)| match c {
                            '(' => {
                                depth += 1;
                                false
                            }
                            ')' if depth > 0 => {
                                depth -= 1;
                                false
                            }
                            ')' => true,
                            c => c.is_whitespace(),
                        })
                        .map_or(rest.len(), |(e, _)| e);
                    &rest[..end]
                }
            };
            out.push(target);
        }
        for attr in ["href=\"", "src=\"", "srcset=\""] {
            out.extend(attrs(s, attr).map(|v| v.split_whitespace().next().unwrap_or("")));
        }
    }
    out
}

/// `%XX` taken back to its bytes.
fn decoded(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok());
        match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
            Some(byte) if b[i] == b'%' => {
                out.push(byte);
                i += 3;
            }
            _ => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What's wrong with the links of `files` (paths from `root`), one line each: `docs/a.md:14: #x — …`.
fn broken(root: &Path, files: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(root.join(file)).unwrap();
        for (n, line, pieces) in outside_code(&text) {
            for target in targets(line, &pieces) {
                let scheme = target.split_once(':').is_some_and(|(s, _)| {
                    s.starts_with(|c: char| c.is_ascii_alphabetic())
                        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
                });
                if target.is_empty() || scheme {
                    continue;
                }
                let (path, anchor) = match target.split_once('#') {
                    Some((p, a)) => (p, Some(a)),
                    None => (target, None),
                };
                let say = |why: String| format!("{}:{n}: {target} — {why}", file.display());
                // A path from the file's folder, or from the repository's root with a leading `/`, as GitHub reads
                // it; never above the root.
                let to = match path {
                    "" => Some(file.clone()),
                    p => {
                        let p = decoded(p);
                        let from =
                            if p.starts_with('/') { PathBuf::new() } else { file.parent().unwrap().into() };
                        Path::new(&p).components().try_fold(from, |mut at, c| match c {
                            Component::ParentDir => at.pop().then_some(at),
                            Component::Normal(c) => Some(at.join(c)),
                            _ => Some(at),
                        })
                    }
                };
                let Some(to) = to else {
                    out.push(say("above the repository".into()));
                    continue;
                };
                if !there(root, &to) {
                    out.push(say(format!("no {}", to.display())));
                    continue;
                }
                if let Some(anchor) = anchor.filter(|a| !a.is_empty())
                    && to.extension().is_some_and(|e| e == "md")
                {
                    let theirs = anchors(&std::fs::read_to_string(root.join(&to)).unwrap());
                    if !theirs.contains(&decoded(anchor)) {
                        out.push(say(format!("no such heading in {}", to.display())));
                    }
                }
            }
        }
    }
    out
}

/// Whether `rel` (normal parts only) is under `root` as written, each part's case too: GitHub's paths are
/// case-sensitive, and this Mac's disk isn't — `exists` would take `Pic.png` for `pic.png`.
fn there(root: &Path, rel: &Path) -> bool {
    let mut at = root.to_path_buf();
    for part in rel.components() {
        let Component::Normal(name) = part else { return false };
        let Ok(entries) = std::fs::read_dir(&at) else { return false };
        if !entries.flatten().any(|e| e.file_name() == name) {
            return false;
        }
        at.push(name);
    }
    true
}

/// The README and every Markdown file under `docs/`, from the repository's root.
fn docs(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![PathBuf::from("README.md")];
    let mut dirs = vec![PathBuf::from("docs")];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(root.join(&dir)).unwrap() {
            let entry = entry.unwrap();
            let rel = dir.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                dirs.push(rel);
            } else if rel.extension().is_some_and(|e| e == "md") {
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_relative_link_and_anchor_in_the_docs_resolves() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let files = docs(root);
        assert!(files.len() > 10, "the docs: {files:?}");
        let broken = broken(root, &files);
        assert!(broken.is_empty(), "links that go nowhere:\n{}", broken.join("\n"));
    }

    #[test]
    fn a_headings_anchor_is_githubs() {
        let names: Vec<_> = anchors(
            "# Its panel, its line — its buffers\n\
             ## `:set` and [the list](a.md#x)\n\
             ## Sockets and routing (*built*, step 4)\n\
             ### `snake_case` and _emphasis_ and <kbd>Tab</kbd>\n\
             ## A -- B ##\n\
             ## 타래 · Thread\n\
             ## Events\n\
             ## Events\n\
             ## Events-1\n\
             <a id=\"kept\"></a>\n\
             ```\n\
             # not a heading\n\
             ```\n",
        )
        .into_iter()
        .collect();
        assert_eq!(
            names,
            [
                "a----b",
                "events",
                "events-1",
                "events-1-1",
                "its-panel-its-line--its-buffers",
                "kept",
                "set-and-the-list",
                "snake_case-and-emphasis-and-tab",
                "sockets-and-routing-built-step-4",
                "타래--thread",
            ]
        );
    }

    #[test]
    fn a_link_that_goes_nowhere_is_said_and_code_isnt_a_link() {
        let s = crate::scratch::Scratch::new("doclinks");
        std::fs::create_dir_all(s.join("docs/sub")).unwrap();
        std::fs::write(s.join("docs/sub/b.md"), "# B\n## Two words\n").unwrap();
        std::fs::write(s.join("docs/pic.png"), "").unwrap();
        std::fs::write(
            s.join("docs/a.md"),
            "# A\n\
             [ok](sub/b.md#two-words) [here](#a) ![pic](pic.png \"title\") [web](https://x.y/#z) [top](/docs/a.md)\n\
             [gone](sub/c.md) [stale](sub/b.md#one-word) [self](#nope)\n\
             `[in code](nowhere.md)` and <img src=\"missing.png\">\n\
             [ref]: sub/b.md#two\n\
             [out](../../x.md)\n\
             ```md\n[fenced](nowhere.md)\n```\n",
        )
        .unwrap();
        let said = broken(s.path(), &[PathBuf::from("docs/a.md")]);
        assert_eq!(
            said,
            [
                "docs/a.md:3: sub/c.md — no docs/sub/c.md",
                "docs/a.md:3: sub/b.md#one-word — no such heading in docs/sub/b.md",
                "docs/a.md:3: #nope — no such heading in docs/a.md",
                "docs/a.md:4: missing.png — no docs/missing.png",
                "docs/a.md:5: sub/b.md#two — no such heading in docs/sub/b.md",
                "docs/a.md:6: ../../x.md — above the repository",
            ]
        );
    }

    #[test]
    fn a_links_case_is_the_files() {
        let s = crate::scratch::Scratch::new("doclinks-case");
        std::fs::create_dir_all(s.join("docs/Sub")).unwrap();
        std::fs::write(s.join("docs/Sub/Pic.png"), "").unwrap();
        std::fs::write(
            s.join("docs/a.md"),
            "[ok](Sub/Pic.png) [file](Sub/pic.png) [folder](sub/Pic.png) [up](../Docs/a.md)\n",
        )
        .unwrap();
        assert_eq!(
            broken(s.path(), &[PathBuf::from("docs/a.md")]),
            [
                "docs/a.md:1: Sub/pic.png — no docs/Sub/pic.png",
                "docs/a.md:1: sub/Pic.png — no docs/sub/Pic.png",
                "docs/a.md:1: ../Docs/a.md — no Docs/a.md",
            ]
        );
    }

    #[test]
    fn a_code_span_wrapped_across_lines_is_code_to_its_end() {
        let s = crate::scratch::Scratch::new("doclinks-wrap");
        std::fs::create_dir_all(s.join("docs")).unwrap();
        std::fs::write(
            s.join("docs/a.md"),
            // (1–2: a span from line 1 to line 2, then a link; 4–5: per line, line 5's backticks would pair around
            // its link and hide it; 7–8: a list item ends the paragraph, its unclosed backtick pairs with nothing)
            "A span that `starts here\n\
             and [ends](nowhere.md) here` is code, and [this](gone.md) a link.\n\
             \n\
             `one` and `two\n\
             wraps` — then [real](gone2.md) and a `x` y\n\
             \n\
             - an item with `one backtick\n\
             - and [next](gone3.md) with `code`\n",
        )
        .unwrap();
        assert_eq!(
            broken(s.path(), &[PathBuf::from("docs/a.md")]),
            [
                "docs/a.md:2: gone.md — no docs/gone.md",
                "docs/a.md:5: gone2.md — no docs/gone2.md",
                "docs/a.md:8: gone3.md — no docs/gone3.md",
            ]
        );
    }
}
