//! Text cut to a number of cells — by display width, for whatever lays text out in cells, the terminal's
//! drawing (`term.rs`) first.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The start of `s` that fits in `width` cells — bare: for code and output lines.
pub fn fit(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > width {
            break;
        }
        w += cw;
        out.push(ch);
    }
    out
}

/// Truncate to width, ending with '…' if cut — nothing when there's no room at all (the `…` is a cell too).
pub fn fit_ellipsis(s: &str, width: usize) -> String {
    if s.width() <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = fit(s, width.saturating_sub(1));
    out.push('…');
    out
}

/// Keeps the end (file name) of a path that doesn't fit: `…/editor/mod.rs`. No room at all: nothing (a
/// lone `…` would still take a cell).
pub fn fit_left(s: &str, width: usize) -> String {
    if s.width() <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut w = 1;
    let mut tail: Vec<char> = Vec::new();
    for ch in s.chars().rev() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > width {
            break;
        }
        w += cw;
        tail.push(ch);
    }
    std::iter::once('…').chain(tail.into_iter().rev()).collect()
}

/// A place (a folder's path) that doesn't fit, folded from its start at a folder's boundary where one is left —
/// `…/ZCTJ/infra`, never `…hot.ZCTJ/infra`; its last part alone cut as `fit_left` cuts it. `:folders`' place and a
/// workspace's folder in the file picker.
pub fn fit_place(s: &str, width: usize) -> String {
    let at = fit_left(s, width);
    match at.strip_prefix('…').filter(|r| !r.starts_with('/')).and_then(|r| r.find('/').map(|i| &r[i..])) {
        Some(rest) => format!("…{rest}"),
        None => at,
    }
}

/// `fit_place` with a row's match marks (characters of `s`) moved along: whole, they stay; folded, those cut off go and
/// the rest follow the `…`; nothing left, none.
pub fn fit_place_marked(s: &str, marks: &[u32], width: usize) -> (String, Vec<u32>) {
    let cut = fit_place(s, width);
    let marks = match cut.strip_prefix('…') {
        _ if cut == s => marks.to_vec(),
        Some(rest) => {
            let gone = (s.chars().count() - rest.chars().count()) as u32;
            marks.iter().filter(|&&m| m >= gone).map(|&m| m - gone + 1).collect()
        }
        None => Vec::new(),
    };
    (cut, marks)
}

/// One row of `fit_in_hand`'s: its text, and whether the break before it took a space (a line break of `s` counts as
/// one — a paragraph keeps it as a space).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wrapped {
    pub text: String,
    pub space: bool,
}

/// What's cut is whole where you look at it (the user, 2026-10-25) — the one way a row that may be cut is shown:
/// folded, one row of `width` cells, its first line cut with `…` at its end (`fit_ellipsis`) or, `keep_end`, at its
/// start (`fit_left`) — `…` after it when more lines follow; `whole` (the row in hand: a cursor on it, the mouse over
/// it), every row it takes, each line of it broken after a space where the row has one (that space the break's), else
/// mid-word, blank lines left out. The caller hangs the rows after the first under its text and records where a cut
/// one is (`whole.rs`), so the mouse can take it in hand.
pub fn fit_in_hand(s: &str, width: usize, keep_end: bool, whole: bool) -> Vec<Wrapped> {
    let width = width.max(1);
    let mut lines = s.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.trim().is_empty());
    if !whole {
        let first = lines.next().unwrap_or_default();
        let text = match (lines.next().is_some(), keep_end) {
            (true, _) => fit_ellipsis(&format!("{first} …"), width),
            (false, true) => fit_left(first, width),
            (false, false) => fit_ellipsis(first, width),
        };
        return vec![Wrapped { text, space: false }];
    }
    let mut rows = Vec::new();
    for (k, line) in lines.enumerate() {
        let mut space = k > 0;
        let mut rest = line;
        while !rest.is_empty() {
            // (the start that fits; past it, back to the row's last space — a word isn't cut while one is there)
            let fits = fit(rest, width).len().max(rest.chars().next().map_or(0, char::len_utf8));
            if fits == rest.len() {
                rows.push(Wrapped { text: rest.to_string(), space });
                break;
            }
            let next = fits + rest[fits..].chars().next().map_or(0, char::len_utf8);
            // (at the first of a run of spaces: the row ends on a word, the rest of the run starts the next — the
            // text again, whatever the width)
            match rest[..next]
                .rfind(' ')
                .map(|at| rest[..at].trim_end_matches(' ').len())
                .filter(|&at| at > 0)
            {
                Some(at) => {
                    rows.push(Wrapped { text: rest[..at].to_string(), space });
                    (rest, space) = (&rest[at + 1..], true);
                }
                None => {
                    rows.push(Wrapped { text: rest[..fits].to_string(), space });
                    (rest, space) = (&rest[fits..], false);
                }
            }
        }
    }
    if rows.is_empty() {
        rows.push(Wrapped { text: String::new(), space: false });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(rows: &[Wrapped]) -> Vec<(&str, bool)> {
        rows.iter().map(|r| (r.text.as_str(), r.space)).collect()
    }

    #[test]
    fn a_place_folds_at_a_folders_boundary() {
        let p = "/private/tmp/tarae-shot-folders/infra";
        assert_eq!(fit_place(p, 40), p, "fits: whole");
        assert_eq!(fit_place(p, 30), "…/tmp/tarae-shot-folders/infra");
        assert_eq!(fit_place(p, 28), "…/tarae-shot-folders/infra", "`…mp/` folded to the boundary");
        assert_eq!(fit_place(p, 20), "…/infra", "never mid-name while a boundary is left");
        assert_eq!(fit_place("/a/infrastructure", 8), "…ructure", "its last part alone: cut");
        assert_eq!(fit_place(p, 0), "");
    }

    /// A place's match marks: whole, each where it was (the leading `/` too); folded, those cut off gone and the rest
    /// after the `…` on the same characters; nothing left, none.
    #[test]
    fn a_places_marks_follow_its_fold() {
        let p = "/x/ZCTJ/infra";
        let marked = |(s, m): (String, Vec<u32>)| -> String {
            s.chars().enumerate().filter(|(i, _)| m.contains(&(*i as u32))).map(|(_, c)| c).collect()
        };
        assert_eq!(
            fit_place_marked(p, &[0, 1, 8], 20),
            (p.to_string(), vec![0, 1, 8]),
            "whole: as they were"
        );
        assert_eq!(marked(fit_place_marked(p, &[0, 1, 8], 20)), "/xi");
        let folded = fit_place_marked(p, &[0, 1, 7, 8], 10);
        assert_eq!(folded.0, "…/infra");
        assert_eq!(marked(folded), "/i", "the `/` and `i` kept, `x` and the first `/` cut off");
        assert_eq!(fit_place_marked(p, &[0, 8], 0), (String::new(), Vec::new()));
    }

    #[test]
    fn a_row_in_hand_is_whole_folded_its_cut() {
        let s = "cargo fmt --all && git diff --stat";
        assert_eq!(texts(&fit_in_hand(s, 12, false, false)), [("cargo fmt -…", false)]);
        assert_eq!(texts(&fit_in_hand(s, 12, true, false)), [("…diff --stat", false)]);
        assert_eq!(
            texts(&fit_in_hand(s, 12, false, true)),
            [("cargo fmt", false), ("--all && git", true), ("diff --stat", true)]
        );
        // (whole, the rows' text and the spaces their breaks took are the text again)
        let joined = fit_in_hand(s, 12, false, true)
            .iter()
            .map(|r| format!("{}{}", if r.space { " " } else { "" }, r.text))
            .collect::<String>();
        assert_eq!(joined, s);
    }

    #[test]
    fn a_word_wider_than_the_row_breaks_mid_word_and_lines_break_rows() {
        assert_eq!(
            texts(&fit_in_hand("abcdefgh ij", 4, false, true)),
            [("abcd", false), ("efgh", false), ("ij", true)]
        );
        assert_eq!(texts(&fit_in_hand("one\n\ntwo", 10, false, true)), [("one", false), ("two", true)]);
        assert_eq!(
            texts(&fit_in_hand("ab  cd", 4, false, true)),
            [("ab", false), (" cd", true)],
            "a run of spaces"
        );
        assert_eq!(texts(&fit_in_hand("one\ntwo", 10, false, false)), [("one …", false)]);
    }
}
