//! A failed run's report with the lines its output points at (code mode (a)'s a6, `llm.compound`): the places every
//! run finds already (`Run.places` — the packs' `[[places]]` and the form every tool shares, `task::Matcher`), the
//! first `POINTS_MAX` of them in the project, a few lines round each, read on a worker once the run ended — the read
//! that would follow, in the report. No form of place is known here: those are the packs' data.

use std::path::{Path, PathBuf};

use crate::editor::Editor;

/// Places whose lines a report brings, at most — the first ones in the project, in the output's order.
pub const POINTS_MAX: usize = 8;
/// Lines brought each side of a place's — so a report brings `POINTS_MAX × (2 × AROUND + 1)` lines at most (56).
pub const AROUND: usize = 3;
/// Places looked at for those, at most (the run's first ones: a long list's tail is the same errors again).
const LOOKED_MAX: usize = 64;

/// What a report adds: its text (empty: nothing — or `\nLines it points at:\n…`) and what it showed.
#[derive(Debug, Default)]
pub struct Pointed {
    pub text: String,
    pub showed: crate::nav::Showed,
}

/// The places of `places` a report may bring the lines of (path as found, line from 0) — judged on the worker.
pub fn wanted(places: &[crate::quickfix::Item]) -> Vec<(PathBuf, usize)> {
    places.iter().take(LOOKED_MAX).map(|it| (it.path.clone(), it.line)).collect()
}

/// `then` with what a failed run's report brings of `wanted`: read on a worker (`keys`: the chat's, which files
/// named like a key it hasn't been let read); at once with nothing when there's nothing to read or no `keys` (off,
/// or a run that didn't fail).
pub fn then(
    ed: &mut Editor,
    keys: Option<crate::reads::Keys>,
    wanted: Vec<(PathBuf, usize)>,
    then: impl FnOnce(&mut Editor, Pointed) + Send + 'static,
) {
    let Some(keys) = keys.filter(|_| !wanted.is_empty()) else { return then(ed, Pointed::default()) };
    let root = ed.workspace.root.clone();
    ed.events.jobs().spawn(move || {
        let p = read(&root, &wanted, &keys);
        move |ed: &mut Editor| then(ed, p)
    });
}

/// The lines `wanted` points at (a worker's): the first `POINTS_MAX` places inside `root` by their real paths, but a
/// file named like a key not open to Claude (`keys`), `AROUND` lines each side, merged — each file's stamp taken
/// before it's read.
pub fn read(root: &Path, wanted: &[(PathBuf, usize)], keys: &crate::reads::Keys) -> Pointed {
    let Ok(root) = std::fs::canonicalize(root) else { return Pointed::default() };
    let mut files: Vec<(PathBuf, Vec<usize>)> = Vec::new();
    let mut taken = 0;
    for (path, line) in wanted {
        let Ok(real) = std::fs::canonicalize(path) else { continue };
        if !real.starts_with(&root) || !real.is_file() || keys.shut(&real) {
            continue;
        }
        if taken == POINTS_MAX {
            break;
        }
        taken += 1;
        match files.iter_mut().find(|f| f.0 == real) {
            Some(f) => f.1.push(*line),
            None => files.push((real, vec![*line])),
        }
    }
    let mut p = Pointed::default();
    let mut out = String::new();
    for (real, mut lines) in files {
        // (taken before it's read: a change while it reads counts as one since)
        let stamp = crate::disk::stamp(&real);
        crate::sorting::sort(&mut lines);
        let Some(text) = text_of(&real, lines.last().map_or(0, |l| l + AROUND + 1)) else { continue };
        let all: Vec<&str> = text.lines().collect();
        lines.dedup();
        // (a place past the file's end: it changed since the run printed it)
        lines.retain(|l| *l < all.len());
        let mut spans: Vec<(usize, usize)> = Vec::new();
        for &l in &lines {
            let (a, b) = (l.saturating_sub(AROUND), (l + AROUND).min(all.len() - 1));
            match spans.last_mut() {
                Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
                _ => spans.push((a, b)),
            }
        }
        let Some(&(_, end)) = spans.last() else { continue };
        let w = (end + 1).to_string().len();
        out.push_str(&crate::chat::shown(&root, &real));
        out.push('\n');
        for (k, &(a, b)) in spans.iter().enumerate() {
            if k > 0 {
                out.push_str("  --\n");
            }
            for (i, line) in all.iter().enumerate().take(b + 1).skip(a) {
                let sep = if lines.binary_search(&i).is_ok() { ':' } else { '-' };
                out.push_str(&format!(
                    "  {:>w$}{sep} {}\n",
                    i + 1,
                    crate::nav::clip(line, crate::nav::LINE_MAX)
                ));
            }
            p.showed.lines.push(crate::replay::Shown { path: real.clone(), lines: a..b + 1, old: false });
        }
        p.showed.stamps.push((real, stamp));
    }
    if !out.is_empty() {
        p.text = format!("\nLines it points at:\n{out}");
    }
    p
}

/// `path`'s first `lines` lines (`usize::MAX`: all of it), read from that very file as opened (`paths::open_read`,
/// as `outlines::of` reads one) — no further: a big generated file isn't read whole for a few lines at its top.
pub(crate) fn text_of(path: &Path, lines: usize) -> Option<String> {
    use std::io::BufRead;
    let o = crate::paths::open_read(path).ok()?;
    if o.real != path || !o.meta.is_file() {
        return None;
    }
    let mut r = std::io::BufReader::new(o.file);
    let mut bytes = Vec::new();
    for _ in 0..lines {
        if r.read_until(b'\n', &mut bytes).ok()? == 0 {
            break;
        }
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `line 1` … `line n`.
    fn numbered(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}\n")).collect()
    }

    /// The places in the project bring `AROUND` lines each side, merged, their own lines marked; a place outside the
    /// project, in a file named like a key or past a file's end brings nothing. What it showed: each file's stamp
    /// (once) and the lines.
    #[test]
    fn a_report_brings_the_lines_its_places_point_at() {
        let scratch = crate::scratch::Scratch::new("pointed");
        let dir = std::fs::canonicalize(&scratch).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let a = dir.join("src/a.rs");
        std::fs::write(&a, numbered(30)).unwrap();
        std::fs::write(dir.join(".env"), "A=tarae-fake-token\n").unwrap();
        let outside = crate::scratch::Scratch::new("pointed-outside");
        std::fs::write(outside.join("b.rs"), numbered(30)).unwrap();
        let keys = crate::reads::Keys::none_opened(&dir);
        let wanted = [
            (outside.join("b.rs"), 4),
            (dir.join(".env"), 0),
            (a.clone(), 9),
            (a.clone(), 11),
            (a.clone(), 99),
        ];
        let p = read(&dir, &[&wanted[..], &[(a.clone(), 25)]].concat(), &keys);
        let mut want = String::from("\nLines it points at:\nsrc/a.rs\n");
        for (k, (lines, marked)) in [(7..=15, &[10, 12][..]), (23..=29, &[26][..])].into_iter().enumerate() {
            if k > 0 {
                want.push_str("  --\n");
            }
            for i in lines {
                let sep = if marked.contains(&i) { ':' } else { '-' };
                want.push_str(&format!("  {i:>2}{sep} line {i}\n"));
            }
        }
        assert_eq!(p.text, want);
        assert_eq!(p.showed.stamps.iter().map(|s| (&s.0, s.1.is_some())).collect::<Vec<_>>(), [(&a, true)]);
        assert_eq!(p.showed.lines.iter().map(|l| l.lines.clone()).collect::<Vec<_>>(), [6..15, 22..29]);
        // Nothing in the project: nothing at all
        let p = read(&dir, &wanted[..2], &keys);
        assert!(p.text.is_empty() && p.showed.stamps.is_empty(), "{}", p.text);
    }

    /// A file is read only as far as it's needed: its first `lines` lines, all of it with `usize::MAX`.
    #[test]
    fn a_files_text_is_read_only_as_far_as_its_lines() {
        let scratch = crate::scratch::Scratch::new("pointed-text");
        let a = std::fs::canonicalize(&scratch).unwrap().join("a.rs");
        std::fs::write(&a, numbered(400)).unwrap();
        assert_eq!(text_of(&a, 3).as_deref(), Some("line 1\nline 2\nline 3\n"));
        assert_eq!(text_of(&a, 0).as_deref(), Some(""));
        assert_eq!(text_of(&a, usize::MAX), Some(numbered(400)));
    }

    /// At most `POINTS_MAX` places — the first, in the output's order — and `wanted` looks at the run's first ones.
    #[test]
    fn a_report_brings_the_first_places_only() {
        let scratch = crate::scratch::Scratch::new("pointed-caps");
        let dir = std::fs::canonicalize(&scratch).unwrap();
        let a = dir.join("a.rs");
        std::fs::write(&a, numbered(400)).unwrap();
        let wanted: Vec<(PathBuf, usize)> = (0..12).map(|i| (a.clone(), 20 * i + 10)).collect();
        let p = read(&dir, &wanted, &crate::reads::Keys::none_opened(&dir));
        assert_eq!(p.showed.lines.len(), POINTS_MAX);
        assert!(p.text.contains(&format!("{}: line", 20 * (POINTS_MAX - 1) + 11)), "{}", p.text);
        assert!(!p.text.contains(&format!("{}: line", 20 * POINTS_MAX + 11)), "{}", p.text);
        let item = |line| crate::quickfix::Item {
            path: a.clone(),
            line,
            col: 0,
            message: String::new(),
            severity: 1,
            mark: None,
        };
        let places: Vec<_> = (0..100).map(item).collect();
        assert_eq!(super::wanted(&places).len(), LOOKED_MAX);
    }

    /// a6's code holds no form of place and names no tool or language: those are the packs' data (`[[places]]`).
    #[test]
    fn the_lines_pointed_at_know_no_form_of_place() {
        let names = [
            "regex",
            "rust",
            "rustc",
            "cargo",
            "python",
            "pytest",
            "java",
            "javac",
            "maven",
            "gradle",
            "kotlin",
            "kotlinc",
            "scala",
            "sbt",
            "node",
            "jest",
            "vitest",
            "typescript",
            "tsc",
            "javascript",
            "elixir",
            "php",
            "lua",
            "ruby",
            "golang",
            "swift",
            "panic",
            "panicked",
            "backtrace",
            "traceback",
        ];
        let code = include_str!("pointed.rs").split("#[cfg(test)]").next().unwrap();
        for w in code.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
            assert!(!names.contains(&w.to_ascii_lowercase().as_str()), "`{w}` in pointed.rs");
        }
    }
}
