//! Text cut to a number of cells — by display width, for whatever lays text out in cells, the terminal's
//! drawing (`term.rs`) first.

// What draws slices text with `cut::slice` (#470: a slice off a character's boundary panicked the main thread)
#![cfg_attr(not(test), deny(clippy::string_slice))]

use std::borrow::Cow;
use std::ops::{Bound, RangeBounds};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// `s[range]` for what draws: an end off a character's boundary (or past the end) fails a test or a simulation at
/// once, and in a release build snaps to the boundary before it — a character drawn more or less, never the session
/// ended (#470: an offset added up from wrapped rows landed inside a `—`, and the main thread panicked at each start).
/// Both ends snap the same way, so two slices that meet at a wrong offset still meet.
pub fn slice(s: &str, range: impl RangeBounds<usize>) -> &str {
    let from = match range.start_bound() {
        Bound::Included(&a) => a,
        Bound::Excluded(&a) => a + 1,
        Bound::Unbounded => 0,
    };
    let to = match range.end_bound() {
        Bound::Included(&b) => b + 1,
        Bound::Excluded(&b) => b,
        Bound::Unbounded => s.len(),
    };
    debug_assert!(
        s.is_char_boundary(from) && s.is_char_boundary(to) && from <= to,
        "a slice off a character's boundary: {from}..{to} of {} bytes",
        s.len()
    );
    s.get(snapped(s, from, to)).unwrap_or_default()
}

/// `s` with its first letter upper-cased, taken as a character (#479: `s[..1]` of a `실` or a `👍` panicked): `go` →
/// `Go`.
pub fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// `from..to` in `s` with each end snapped to the boundary at or before it (past the end: the end), never backwards.
fn snapped(s: &str, from: usize, to: usize) -> std::ops::Range<usize> {
    let to = s.floor_char_boundary(to);
    s.floor_char_boundary(from).min(to)..to
}

/// `s` as a cut shows it, each character in the cells `width()` gives it. A control character — which a terminal
/// acts on: a tab to its next stop, a line's end down a row, an escape eating what follows — is one cell (#478): a
/// space for a tab or a line's end (a row's lines one paragraph, as `fit_in_hand` and the chat's document join them;
/// a row has no tab stops of its own), `�` for the rest (as the code view draws one). A `\r\n` is one space.
pub fn drawn(s: &str) -> Cow<'_, str> {
    if !s.chars().any(char::is_control) {
        return Cow::Borrowed(s);
    }
    // (a control character is a character of its own — `\r\n` the one pair)
    let mut out = String::with_capacity(s.len());
    for g in s.graphemes(true) {
        match stand_in(g) {
            Some(c) => out.push(c),
            None => out.push_str(g),
        }
    }
    Cow::Owned(out)
}

/// What `drawn` draws for a character (a grapheme cluster) that's a control character — None for the rest, drawn as
/// they are.
fn stand_in(g: &str) -> Option<char> {
    let c = g.chars().next().filter(|c| c.is_control())?;
    Some(if c.is_whitespace() { ' ' } else { '\u{fffd}' })
}

/// Byte `at` of `s` as a byte of `drawn(s)` — an offset taken on the text (a search's hit, a painted word, where it
/// was cut) read in what's drawn of it (#491: a `\r\n` is one byte there, another control character `�`'s three).
/// Between a `\r\n`'s two: after its space; past the end: the end.
pub fn drawn_at(s: &str, at: usize) -> usize {
    let head = s.get(..s.floor_char_boundary(at)).unwrap_or(s);
    if !head.chars().any(char::is_control) {
        return head.len();
    }
    head.graphemes(true).map(|g| stand_in(g).map_or(g.len(), char::len_utf8)).sum()
}

/// The byte of `s` that `drawn(s)`'s byte `at` is drawn from — `drawn_at` the other way: inside a character, its
/// start; past the end, the end.
pub fn drawn_from(s: &str, at: usize) -> usize {
    if !s.chars().any(char::is_control) {
        return s.floor_char_boundary(at);
    }
    let mut d = 0;
    for (i, g) in s.grapheme_indices(true) {
        d += stand_in(g).map_or(g.len(), char::len_utf8);
        if d > at {
            return i;
        }
    }
    s.len()
}

/// The bytes of `s`'s start that fit in `width` cells: whole characters (grapheme clusters), each in the cells
/// `width()` gives it — a control character one (`drawn`), an emoji's sequence two — so a cut is never wider than its
/// room by the measure that lays it out (#478: a tab or a line's end was no cell here and one to `width()`).
fn fit_len(s: &str, width: usize) -> usize {
    // (ASCII up to the byte after the room, no `\r` — `\r\n` is one cell — a cell a byte, and a boundary there)
    let head = &s.as_bytes()[..s.len().min(width.saturating_add(1))];
    if head.is_ascii() && !head.contains(&b'\r') {
        return s.len().min(width);
    }
    let mut w = 0;
    for (i, g) in s.grapheme_indices(true) {
        w += g.width();
        if w > width {
            return i;
        }
    }
    s.len()
}

/// The start of `s` that fits in `width` cells — bare: for code and output lines.
pub fn fit(s: &str, width: usize) -> String {
    drawn(slice(s, ..fit_len(s, width))).into_owned()
}

/// Truncate to width, ending with '…' if cut — nothing when there's no room at all (the `…` is a cell too).
pub fn fit_ellipsis(s: &str, width: usize) -> String {
    if s.width() <= width {
        return drawn(s).into_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = fit(s, width - 1);
    out.push('…');
    out
}

/// Keeps the end (file name) of a path that doesn't fit: `…/editor/mod.rs`. No room at all: nothing (a
/// lone `…` would still take a cell).
pub fn fit_left(s: &str, width: usize) -> String {
    if s.width() <= width {
        return drawn(s).into_owned();
    }
    if width == 0 {
        return String::new();
    }
    let (mut w, mut from) = (1, s.len());
    for (i, g) in s.grapheme_indices(true).rev() {
        w += g.width();
        if w > width {
            break;
        }
        from = i;
    }
    format!("…{}", drawn(slice(s, from..)))
}

/// A place (a folder's path) that doesn't fit, folded from its start at a folder's boundary where one is left —
/// `…/ZCTJ/infra`, never `…hot.ZCTJ/infra`; its last part alone cut as `fit_left` cuts it. `:folders`' place and a
/// workspace's folder in the file picker.
pub fn fit_place(s: &str, width: usize) -> String {
    let at = fit_left(s, width);
    match at
        .strip_prefix('…')
        .filter(|r| !r.starts_with('/'))
        .and_then(|r| r.find('/').map(|i| slice(r, i..)))
    {
        Some(rest) => format!("…{rest}"),
        None => at,
    }
}

/// `fit_place` with a row's match marks (characters of `s`) moved along: whole, they stay; folded, those cut off go and
/// the rest follow the `…`; nothing left, none.
pub fn fit_place_marked(s: &str, marks: &[u32], width: usize) -> (String, Vec<u32>) {
    let cut = fit_place(s, width);
    let d = drawn(s);
    // (the marks read in what's drawn: a `\r\n` is one character there, #491)
    let marks: Vec<u32> = match &d {
        Cow::Borrowed(_) => marks.to_vec(),
        Cow::Owned(d) => {
            let at = |m: u32| s.char_indices().nth(m as usize).map_or(s.len(), |(b, _)| b);
            let mut on: Vec<u32> =
                marks.iter().map(|&m| slice(d, ..drawn_at(s, at(m))).chars().count() as u32).collect();
            on.dedup();
            on
        }
    };
    let marks = match cut.strip_prefix('…') {
        _ if cut == d => marks,
        Some(rest) => {
            let gone = (d.chars().count() - rest.chars().count()) as u32;
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
    if !whole {
        let mut lines = s.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.trim().is_empty());
        let first = lines.next().unwrap_or_default();
        let text = match (lines.next().is_some(), keep_end) {
            (true, _) => fit_ellipsis(&format!("{first} …"), width.max(1)),
            (false, true) => fit_left(first, width.max(1)),
            (false, false) => fit_ellipsis(first, width.max(1)),
        };
        return vec![Wrapped { text, space: false }];
    }
    fit_in_hand_at(s, width).into_iter().map(|(_, row)| row).collect()
}

/// `fit_in_hand`'s rows of `s` in hand (`whole`), each with the byte where its text starts in `s` — kept as each row
/// is cut, never added up nor searched for: a break isn't always one byte (a `\r\n`, a blank line left out), and an
/// offset added up from the rows' lengths lands inside a character (#470).
pub fn fit_in_hand_at(s: &str, width: usize) -> Vec<(usize, Wrapped)> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut start = 0;
    for raw in s.split_inclusive('\n') {
        let mut at = start;
        start += raw.len();
        let line = raw.strip_suffix('\n').unwrap_or(raw).trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let mut space = !rows.is_empty();
        let mut rest = line;
        while !rest.is_empty() {
            // (the start that fits, a character at least; past it, back to the row's last space — a word isn't cut
            // while one is there)
            let first = |s: &str| s.graphemes(true).next().map_or(0, str::len);
            let fits = fit_len(rest, width).max(first(rest));
            if fits == rest.len() {
                rows.push((at, Wrapped { text: rest.to_string(), space }));
                break;
            }
            let next = fits + first(slice(rest, fits..));
            // (at the first of a run of spaces: the row ends on a word, the rest of the run starts the next — the
            // text again, whatever the width; a space is one byte, so past it is a boundary)
            let word = slice(rest, ..next).rfind(' ').map(|sp| slice(rest, ..sp).trim_end_matches(' ').len());
            let (taken, past) = match word.filter(|&w| w > 0) {
                Some(w) => (w, w + 1),
                None => (fits, fits),
            };
            rows.push((at, Wrapped { text: slice(rest, ..taken).to_string(), space }));
            space = past > taken;
            at += past;
            rest = slice(rest, past..);
        }
    }
    if rows.is_empty() {
        rows.push((0, Wrapped { text: String::new(), space: false }));
    }
    rows
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn texts(rows: &[Wrapped]) -> Vec<(&str, bool)> {
        rows.iter().map(|r| (r.text.as_str(), r.space)).collect()
    }

    /// Each row's place is where its text is in what was wrapped, on a character's boundary, at every width — with
    /// breaks that aren't one byte: a blank line, a `\r\n`, a line of spaces, a run of spaces, `—` beside each (a tool
    /// row's words with a blank line put an added-up offset inside a `—`, and drawing the chat panicked).
    #[test]
    fn a_wrapped_rows_place_is_where_its_text_is() {
        let texts =
            ["one — two\n\nthree — four", "a\r\nb — c — d", "x   —   y — z", "line\n   \n— end —", "———"];
        for s in texts {
            for width in 1..14 {
                for (at, row) in fit_in_hand_at(s, width) {
                    let end = at + row.text.len();
                    assert!(
                        s.is_char_boundary(at) && s.is_char_boundary(end),
                        "{s:?} at {width}: {at}..{end}"
                    );
                    assert_eq!(&s[at..end], row.text, "{s:?} at {width}");
                }
            }
        }
    }

    /// Texts to wrap, the same every run (here and `term`'s tests): pieces with breaks that aren't one byte — blank
    /// lines, `\r\n`, lines and runs of spaces, tabs — beside `—`, wide and combining characters, a third of them then
    /// mutated (`mutate::text`: a NUL, a lone `\r`, a run of up to 300 of one character …).
    pub(crate) fn wrap_inputs(n: usize) -> Vec<String> {
        const PIECES: &[&str] = &[
            "one",
            "two words",
            "—",
            " — ",
            "中文字",
            "e\u{301}",
            "a\u{20dd}",
            "👍🏽",
            "\t",
            "\r\n",
            "\n",
            "\n\n",
            "\n   \n",
            "\r\n\r\n",
            "   ",
            " ",
            "abcdefghijkl",
            "\u{3000}",
            "x",
        ];
        let mut rng = crate::mutate::Rng(0x470_2026_1109);
        (0..n)
            .map(|_| {
                let mut s: String = (0..1 + rng.below(14)).map(|_| *rng.pick(PIECES)).collect();
                if rng.below(3) == 0 {
                    crate::mutate::text(&mut rng, &mut s, PIECES);
                }
                s
            })
            .collect()
    }

    /// The widths each text is wrapped at.
    pub(crate) const WIDTHS: [usize; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 13, 17, 24, 40, 80];

    /// `fit_in_hand_at`'s property: each row starts and ends on a character's boundary, after the row before, its text
    /// the text there; between rows only the space a break took, or a line break and the blank lines it left out — so
    /// the rows give back `s` without those blank lines.
    pub(crate) fn rows_give_back(s: &str, rows: &[(usize, Wrapped)]) -> Result<(), String> {
        let blank = |t: &str| t.chars().all(char::is_whitespace);
        let mut back = String::new();
        let mut end = 0;
        for (k, (at, row)) in rows.iter().enumerate() {
            let to = at + row.text.len();
            if !(end <= *at && s.is_char_boundary(*at) && s.is_char_boundary(to)) {
                return Err(format!("row {k} at {at}..{to}, the last ended at {end}"));
            }
            if s.get(*at..to) != Some(row.text.as_str()) {
                return Err(format!("row {k} at {at}: {:?} isn't its text {:?}", s.get(*at..to), row.text));
            }
            let gap = s.get(end..*at).unwrap_or_default();
            let line = gap.contains('\n') && blank(gap);
            let fits = match (k, row.space) {
                (0, _) => blank(gap) && (gap.is_empty() || gap.ends_with('\n')) && !row.space,
                (_, false) => gap.is_empty(),
                (_, true) => gap == " " || line,
            };
            if !fits {
                return Err(format!("row {k} at {at}: {gap:?} before it, its break's space {}", row.space));
            }
            // (a line's end: what's before its break — the space a break took there —, the blank lines after it gone)
            match gap.split_once('\n') {
                Some((last, _)) if k > 0 => {
                    back.push_str(last.trim_end_matches('\r'));
                    back.push('\n');
                }
                _ if k > 0 => back.push_str(gap),
                _ => {}
            }
            back.push_str(&row.text);
            end = to;
        }
        let tail = s.get(end..).unwrap_or_default();
        if !blank(tail) {
            return Err(format!("text left after the last row, from {end}"));
        }
        let lines: Vec<&str> =
            s.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.trim().is_empty()).collect();
        if !lines.is_empty() {
            back.push_str(tail.split('\n').next().unwrap_or_default().trim_end_matches('\r'));
        }
        match back == lines.join("\n") {
            true => Ok(()),
            false => Err(format!("the rows give back {back:?}")),
        }
    }

    /// #470's property, over a few thousand texts at every width: the rows `fit_in_hand_at` cuts give back the text
    /// (`rows_give_back`), and are `fit_in_hand`'s.
    #[test]
    fn wrapped_rows_give_back_the_text_at_every_width() {
        for s in wrap_inputs(2000) {
            for width in WIDTHS {
                let rows = fit_in_hand_at(&s, width);
                if let Err(e) = rows_give_back(&s, &rows) {
                    panic!("{s:?} at {width}: {e}");
                }
                let plain: Vec<Wrapped> = rows.into_iter().map(|r| r.1).collect();
                assert_eq!(fit_in_hand(&s, width, false, true), plain, "{s:?} at {width}");
            }
        }
    }

    /// Texts to cut, the same every run: pieces whose cells a sum of their chars' misses — control characters (a tab,
    /// line ends, an escape, a NUL, DEL, NEL), emoji sequences (a heart with its presentation, a keycap, a family, a
    /// skin tone, a flag), a text presentation — beside wide, half-width and combining characters and paths, a third
    /// of them then mutated (`mutate::text`).
    fn cut_inputs(n: usize) -> Vec<String> {
        const PIECES: &[&str] = &[
            "one",
            "two words",
            "—",
            "中文字",
            "실패",
            "ﾊﾝ",
            "e\u{301}",
            "\t",
            "\n",
            "\r\n",
            "\r",
            "\x1b[31m",
            "\0",
            "\x7f",
            "\u{85}",
            "❤\u{fe0f}",
            "1\u{fe0f}\u{20e3}",
            "👨\u{200d}👩\u{200d}👧",
            "👍🏽",
            "🇰🇷",
            "⌚\u{fe0e}",
            "\u{3000}",
            "src/editor/",
            "/",
            "abcdefghijkl",
            " ",
        ];
        let mut rng = crate::mutate::Rng(0x478_2026_1110);
        (0..n)
            .map(|_| {
                let mut s: String = (0..1 + rng.below(10)).map(|_| *rng.pick(PIECES)).collect();
                if rng.below(3) == 0 {
                    crate::mutate::text(&mut rng, &mut s, PIECES);
                }
                s
            })
            .collect()
    }

    /// #478's property, over a few thousand texts at every width: a cut is never wider than its room by `width()` —
    /// `fit`, `fit_ellipsis`, `fit_left`, `fit_place` and `fit_in_hand`'s folded row — and it draws as it measures, no
    /// control character left in it; `drawn` keeps a text's measure.
    #[test]
    fn a_cut_is_never_wider_than_its_room() {
        type Cut = fn(&str, usize) -> String;
        let cuts: [(&str, Cut); 6] = [
            ("fit", fit),
            ("fit_ellipsis", fit_ellipsis),
            ("fit_left", fit_left),
            ("fit_place", fit_place),
            ("fit_in_hand", |s, w| fit_in_hand(s, w, false, false).remove(0).text),
            ("fit_in_hand keep_end", |s, w| fit_in_hand(s, w, true, false).remove(0).text),
        ];
        for s in cut_inputs(2000) {
            for width in WIDTHS {
                for (name, cut) in cuts {
                    let c = cut(&s, width);
                    // (a folded row takes a cell at least)
                    let room = if name.starts_with("fit_in_hand") { width.max(1) } else { width };
                    assert!(c.width() <= room, "{name}({s:?}, {width}) = {c:?}: {} cells", c.width());
                    assert!(!c.chars().any(char::is_control), "{name}({s:?}, {width}) = {c:?}");
                }
            }
            assert_eq!(drawn(&s).width(), s.width(), "{s:?} drawn {:?}", drawn(&s));
        }
    }

    /// A control character is a cell to the measure and to the cut alike, drawn as one: a tab or a line's end a space,
    /// `\r\n` one, the rest `�` — and an emoji's sequence is two cells, cut whole or not at all.
    #[test]
    fn a_control_character_is_one_cell_drawn_as_one() {
        assert_eq!(drawn("a\tb\r\nc\nd\x1be"), "a b c d\u{fffd}e");
        assert_eq!(("a\tb".width(), fit("a\tb", 2), fit("a\tb", 3)), (3, "a ".into(), "a b".into()));
        assert_eq!(fit_ellipsis("one\ntwo", 7), "one two");
        assert_eq!(fit_ellipsis("one\ntwo", 6), "one t…");
        assert_eq!(fit_left("one\ttwo", 5), "… two");
        assert_eq!((fit("❤\u{fe0f}x", 1), fit("❤\u{fe0f}x", 2)), (String::new(), "❤\u{fe0f}".into()));
        assert_eq!(fit("1\u{fe0f}\u{20e3}", 1), "");
    }

    /// #491: a byte of a text read in what's drawn of it and back — a `\r\n` one byte there, an escape `�`'s three,
    /// every byte after them moved along; between a `\r\n`'s two, after its space; past the end, the end.
    #[test]
    fn an_offset_is_read_in_the_drawn_text_and_back() {
        let s = "a\r\nb\x1bc";
        assert_eq!(drawn(s), "a b\u{fffd}c");
        let to: Vec<usize> = (0..=7).map(|b| drawn_at(s, b)).collect();
        assert_eq!(to, [0, 1, 2, 2, 3, 6, 7, 7]);
        let back: Vec<usize> = (0..=8).map(|b| drawn_from(s, b)).collect();
        assert_eq!(back, [0, 1, 3, 4, 4, 4, 5, 6, 6], "inside the `�`: the escape");
        assert_eq!(
            (drawn_at("a—b", 9), drawn_at("a—b", 2), drawn_from("a—b", 2)),
            (5, 1, 1),
            "none: as it is"
        );
    }

    /// `slice` at boundaries is the slice; off one, a test fails at once — and a release build snaps both ends to the
    /// boundary before them, past the end to the end, never backwards.
    #[test]
    fn a_slice_off_a_boundary_fails_a_test_and_snaps_in_a_release() {
        let s = "a—b";
        assert_eq!((slice(s, 1..4), slice(s, ..1), slice(s, 4..), slice(s, ..)), ("—", "a", "b", s));
        assert_eq!(slice(s, 1..=3), "—");
        for (a, b) in [(2, 4), (1, 3), (0, 9), (4, 1)] {
            let r = std::panic::catch_unwind(|| slice(s, a..b).len());
            assert!(r.is_err(), "{a}..{b} fails a test");
        }
        assert_eq!(snapped(s, 2, 4), 1..4, "the start back to the `—`");
        assert_eq!(snapped(s, 0, 3), 0..1, "the end back before it");
        assert_eq!((snapped(s, 2, 3), snapped(s, 3, 5)), (1..1, 1..5), "two slices that met still meet");
        assert_eq!(snapped(s, 0, 99), 0..5, "past the end: the end");
        assert_eq!(snapped(s, 4, 1), 1..1, "never backwards");
    }

    /// A first letter is a character, whatever its bytes (#479): Korean, an emoji, an accent that grows when
    /// upper-cased, nothing.
    #[test]
    fn a_first_letter_is_taken_whole() {
        assert_eq!(capitalized("from its angle"), "From its angle");
        assert_eq!(capitalized("실타래 주간"), "실타래 주간");
        assert_eq!(capitalized("👍🏽 weekly"), "👍🏽 weekly");
        assert_eq!(capitalized("über"), "Über");
        assert_eq!(capitalized("ßa"), "SSa");
        assert_eq!(capitalized(""), "");
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
        // (#491: a `\r\n` before them is one character drawn — the marks after it on their own characters still)
        let p = "/x\r\nZ/infra";
        assert_eq!(marked(fit_place_marked(p, &[1, 4, 6], 20)), "xZi");
        let folded = fit_place_marked(p, &[1, 4, 6], 7);
        assert_eq!(folded.0, "…/infra");
        assert_eq!(marked(folded), "i");
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
