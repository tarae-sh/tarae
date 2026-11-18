use super::*;
use crate::config::Config;
use crate::event::{Event, Mouse, MouseKind};
use crate::uistate::Region;

const W: u16 = 120;
const H: u16 = 40;

/// An editor on a file of three lines, in a project of its own (`dir`).
fn editor(dir: &crate::scratch::Scratch) -> Editor {
    let mut ed = Editor::new(Config::default());
    ed.workspace.root = std::fs::canonicalize(dir.as_ref()).unwrap();
    let a = ed.workspace.root.join("a.txt");
    std::fs::write(&a, "one\ntwo\nthree\n").unwrap();
    ed.open(&a).unwrap();
    frame(&mut ed);
    ed
}

/// A frame as the main loop draws one, as text.
fn frame(ed: &mut Editor) -> Vec<String> {
    ed.focus_sync();
    let mut buf = Vec::new();
    crate::term::render(ed, &mut buf, W, H).unwrap();
    crate::screen::text(&buf, W, H).unwrap().lines().map(str::to_string).collect()
}

/// Keys as typed: plain characters, `<name>` for the rest, a frame after each.
fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(key);
        frame(ed);
    }
}

fn text(ed: &Editor) -> String {
    ed.workspace.memo.as_ref().map(Memo::text).unwrap_or_default()
}

/// The memo's frame as last drawn, None while it isn't.
fn drawn(ed: &Editor) -> Option<Rect> {
    ed.screen.floating.borrow().iter().find(|f| f.0 == GroupKind::Memo).map(|f| f.1)
}

fn press(ed: &mut Editor, kind: MouseKind, x: usize, y: usize) {
    ed.handle_event(Event::Mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false }));
    frame(ed);
}

/// `space M` brings the memo up with the keys, in insert mode at the end of its text — 40 cells wide, as tall as its
/// rows (three at least), centred, its top at a quarter; `memo` in its top line, its keys in its bottom line as its
/// mode has them; what's typed is its, never the code's; the status line says `insert · memo`.
#[test]
fn space_m_brings_the_memo_up_typed_into_at_its_end() {
    let dir = crate::scratch::Scratch::new("memo-up");
    let mut ed = editor(&dir);
    let code = ed.doc().text.to_string();
    feed(&mut ed, " M");
    assert!(ed.memo_has_keys(), "the keys its");
    let k = ed.keys_to();
    assert_eq!((k.region, k.takes), (Region::Memo, crate::uistate::Takes::Text), "typing in it");
    feed(&mut ed, "Next: the memo<ret>then the clip");
    assert_eq!(text(&ed), "Next: the memo\nthen the clip");
    assert_eq!(ed.doc().text.to_string(), code, "the code untouched");
    let rows = frame(&mut ed);
    let r = drawn(&ed).expect("drawn");
    assert_eq!(
        r,
        Rect { x: 37, y: 10, w: WIDTH, h: 5 },
        "its measure wide, centred, its top at a quarter, three rows"
    );
    assert!(rows[r.y][r.x..].starts_with("╭─ memo ─"), "{}", rows[r.y]);
    assert!(rows[r.y + 1].contains("│ Next: the memo"), "its text a blank in: {}", rows[r.y + 1]);
    assert!(rows[r.y + 2].contains("│ then the clip"), "{}", rows[r.y + 2]);
    let bottom = &rows[r.y + r.h - 1];
    assert!(bottom.contains("esc normal"), "{bottom}");
    assert!(!bottom.contains("space M"), "space types a space here: {bottom}");
    assert!(!bottom.contains("C-w"), "C-w deletes a word here: {bottom}");
    assert!(rows[H as usize - 1].to_lowercase().contains("insert · memo"), "{}", rows[H as usize - 1]);
    // `esc`: normal mode there, its keys said for it
    feed(&mut ed, "<esc>");
    assert!(ed.memo_has_keys(), "esc is normal mode, the memo stays");
    assert_eq!(ed.workspace.memo.as_ref().map(Memo::mode), Some(Mode::Normal));
    let rows = frame(&mut ed);
    let bottom = &rows[r.y + r.h - 1];
    assert!(
        bottom.contains("space M put away")
            && bottom.contains("C-w w code")
            && !bottom.contains("esc normal"),
        "{bottom}"
    );
    assert!(rows[H as usize - 1].to_lowercase().contains("normal · memo"), "{}", rows[H as usize - 1]);
}

/// Its frame whole at 80 × 34, as `:messages`' is — its corners, its sides, its lines on both sides of its name and keys —
/// and lit, the accent, while it has the keys: on the terminal, each frame sent as what changed since the last
/// (`Screen::present`), as the loop sends it.
#[test]
fn the_memos_frame_is_whole_and_lit_on_a_narrow_screen() {
    let dir = crate::scratch::Scratch::new("memo-narrow");
    let mut ed = editor(&dir);
    crossterm::style::force_color_output(true);
    let (mut screen, mut sent) = (crate::screen::Screen::new(), Vec::new());
    let typed = " MNext: the memo docks beside the code\nthen the clip";
    let keys = typed.chars().map(|c| match c {
        '\n' => "ret".parse().unwrap(),
        c => Key::plain(Code::Char(c)),
    });
    for k in std::iter::once(None).chain(keys.map(Some)) {
        if let Some(k) = k {
            ed.handle_key(k);
        }
        ed.focus_sync();
        let mut buf = Vec::new();
        crate::term::render(&mut ed, &mut buf, 80, 34).unwrap();
        sent.extend_from_slice(screen.present(&buf, 80, 34));
    }
    let mut p = vt100::Parser::new(34, 80, 0);
    p.process(&sent);
    let s = p.screen();
    let r = drawn(&ed).unwrap();
    let cells = |y: usize| (r.x..r.x + r.w).map(|x| s.cell(y as u16, x as u16).unwrap()).collect::<Vec<_>>();
    let text = |y: usize| cells(y).iter().map(|c| c.contents().to_string()).collect::<String>();
    assert!(text(r.y).starts_with("╭─ memo ─") && text(r.y).ends_with('╮'), "{}", text(r.y));
    for y in r.y + 1..r.y + r.h - 1 {
        assert!(text(y).starts_with('│') && text(y).ends_with('│'), "{}", text(y));
    }
    let bottom = text(r.y + r.h - 1);
    assert!(bottom.starts_with("╰─") && bottom.ends_with('╯'), "{bottom}");
    // (lit: its lines the accent, the status line's dot's)
    let accent = s.cell(33, 1).unwrap().fgcolor();
    let lines =
        [(r.y, r.x), (r.y, r.x + 1), (r.y + 1, r.x), (r.y + 1, r.x + r.w - 1), (r.y + r.h - 1, r.x + 1)];
    for (y, x) in lines {
        assert_eq!(s.cell(y as u16, x as u16).unwrap().fgcolor(), accent, "its line at {y},{x} lit");
    }
}

/// In both themes the cell under the insert cursor at its text's end carries the text's colour — a terminal that draws
/// its bar in its cell's colour (and the shots) shows a bar on paper as on ink, as the code's and the chat's input do.
#[test]
fn the_memos_insert_cursor_shows_in_both_themes() {
    let dir = crate::scratch::Scratch::new("memo-cursor");
    crossterm::style::force_color_output(true);
    for theme in ["meok", "hanji"] {
        let mut ed = editor(&dir);
        ed.theme = crate::theme::load(theme).unwrap();
        feed(&mut ed, " Mthen the clip");
        let mut buf = Vec::new();
        crate::term::render(&mut ed, &mut buf, W, H).unwrap();
        let mut p = vt100::Parser::new(H, W, 0);
        p.process(&buf);
        let s = p.screen();
        let r = drawn(&ed).unwrap();
        let (x, y) = ((r.x + PAD) as u16, (r.y + 1) as u16);
        let text = s.cell(y, x).unwrap();
        assert_eq!(text.contents(), "t");
        let at = s.cell(y, x + "then the clip".len() as u16).unwrap();
        assert_eq!(at.fgcolor(), text.fgcolor(), "{theme}: the cursor's cell in the text's colour");
    }
}

/// Look B (main's `.sb/decisions/ux.md` "The memo's look"): every cell of its body inside the sides — its text's rows,
/// the blanks either side, the air row under the text — is on the toasts' slight ground (the cards' surface `ui.popup`
/// 70 % over the code's ground), its frame's lines on the pane's ground; a transparent theme paints neither.
#[test]
fn the_memos_body_is_on_the_toasts_slight_ground_and_its_frame_on_the_panes() {
    use crossterm::style::Color as C;
    let dir = crate::scratch::Scratch::new("memo-ground");
    crossterm::style::force_color_output(true);
    let rgb = |c: Option<C>| match c {
        Some(C::Rgb { r, g, b }) => vt100::Color::Rgb(r, g, b),
        c => panic!("a built-in theme's colour is a true colour: {c:?}"),
    };
    for theme in ["meok", "hanji", "meok-transparent", "hanji-transparent"] {
        let mut ed = editor(&dir);
        ed.theme = crate::theme::load(theme).unwrap();
        let bg = |k: &str| ed.theme.try_get(k).and_then(|s| s.bg);
        let (pane, ground) = match bg("ui.background") {
            None => (vt100::Color::Default, vt100::Color::Default),
            Some(code) => {
                let (Some(C::Rgb { r, g, b }), C::Rgb { r: r0, g: g0, b: b0 }) = (bg("ui.popup"), code)
                else {
                    panic!("{theme}: true colours")
                };
                let m = |s: u8, c: u8| (c as f32 + (s as f32 - c as f32) * 0.7).round() as u8;
                (rgb(Some(code)), vt100::Color::Rgb(m(r, r0), m(g, g0), m(b, b0)))
            }
        };
        feed(&mut ed, " MNext: the memo docks<ret>then the clip");
        let mut buf = Vec::new();
        crate::term::render(&mut ed, &mut buf, W, H).unwrap();
        let mut p = vt100::Parser::new(H, W, 0);
        p.process(&buf);
        let s = p.screen();
        let r = drawn(&ed).unwrap();
        // (two lines, three rows at least: an air row under the text)
        assert_eq!(r.h, ROWS_MIN + 2, "{theme}: {r:?}");
        let at = |x: usize, y: usize| s.cell(y as u16, x as u16).unwrap().bgcolor();
        for y in r.y + 1..r.y + r.h - 1 {
            for x in r.x + 1..r.x + r.w - 1 {
                assert_eq!(at(x, y), ground, "{theme}: its body's cell at {x},{y}");
            }
            assert_eq!(at(r.x, y), pane, "{theme}: its left side at {y}");
            assert_eq!(at(r.x + r.w - 1, y), pane, "{theme}: its right side at {y}");
        }
        for x in r.x..r.x + r.w {
            assert_eq!(at(x, r.y), pane, "{theme}: its top line at {x}");
            assert_eq!(at(x, r.y + r.h - 1), pane, "{theme}: its bottom line at {x}");
        }
        if pane != vt100::Color::Default {
            assert_ne!(ground, pane, "{theme}: a ground of its own");
        }
    }
}

/// The status line's `¶ memo` as drawn: its word (a press that runs `memo`), and the status line's row.
fn mark(ed: &mut Editor) -> (Option<crate::wordspot::Spot>, String) {
    let rows = frame(ed);
    let spots = ed.screen.word_spots.borrow().clone();
    let word = spots.into_iter().find(|s| s.press == crate::wordspot::Press::Command("memo"));
    (word, rows[H as usize - 1].clone())
}

/// Step (b): while the memo holds words and is put away, the status line says `¶ memo`, faint — none while it's empty,
/// none while it's up (it's in sight); a press on it brings it up as `space M` does, the keys its, insert mode at the end
/// of its words; emptied and put away, the mark is gone.
#[test]
fn the_status_line_marks_a_memo_put_away_with_words_and_a_press_brings_it_up() {
    let dir = crate::scratch::Scratch::new("memo-mark");
    let mut ed = editor(&dir);
    let said = |line: &str| line.contains(crate::term::MEMO_MARK);
    let (word, line) = mark(&mut ed);
    assert!(word.is_none() && !said(&line), "no memo: no mark — {line}");
    feed(&mut ed, " M");
    let (word, line) = mark(&mut ed);
    assert!(word.is_none() && !said(&line), "up, empty: {line}");
    feed(&mut ed, "<esc> M");
    assert!(!ed.memo_up());
    let (word, line) = mark(&mut ed);
    assert!(word.is_none() && !said(&line), "put away empty: {line}");
    feed(&mut ed, " Mone<ret>two");
    let (word, line) = mark(&mut ed);
    assert!(word.is_none() && !said(&line), "up with words: in sight — {line}");
    feed(&mut ed, "<esc> M");
    let (word, line) = mark(&mut ed);
    assert!(said(&line), "put away with words: {line}");
    let s = word.expect("its word");
    let text: String = s.spans.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(text, crate::term::MEMO_MARK, "the whole mark one button, the blanks the line's");
    assert_eq!((s.on, s.y), (crate::mouse::Surface::Status, H as usize - 1));
    // (the code's keys, moved off the end of the memo's words: the press puts its cursor back there)
    ed.workspace.memo.as_mut().unwrap().pane.doc_mut().set_selection(crate::selection::Selection::point(0));
    press(&mut ed, MouseKind::Down, s.from + 2, s.y);
    press(&mut ed, MouseKind::Up, s.from + 2, s.y);
    assert!(ed.memo_has_keys(), "the press brought it up with the keys");
    assert_eq!(ed.keys_to().region, Region::Memo);
    let m = ed.workspace.memo.as_ref().unwrap();
    assert_eq!(m.mode(), Mode::Insert, "typing");
    assert_eq!(
        m.pane.doc_in().unwrap().selection().primary().head,
        "one\ntwo".len(),
        "at the end of its words"
    );
    let (word, line) = mark(&mut ed);
    assert!(word.is_none() && !said(&line), "up again: {line}");
    // Emptied and put away (a take's hands: `esc % d space M`): no mark
    feed(&mut ed, "<esc>%d M");
    assert!(
        !ed.memo_up() && ed.workspace.memo.as_ref().unwrap().is_empty(),
        "{:?}",
        ed.workspace.memo.as_ref().map(Memo::text)
    );
    let (word, line) = mark(&mut ed);
    assert!(word.is_none() && !said(&line), "emptied: {line}");
}

/// The mark for words kept from before: a memo not brought up since tarae started, its checkout's words in the state's
/// `memos.json` — said as one put away is; an empty one kept, not.
#[test]
fn the_mark_says_words_kept_from_before() {
    let dir = crate::scratch::Scratch::new("memo-mark-kept");
    let mut ed = editor(&dir);
    let key = ed.workspace.root.to_string_lossy().into_owned();
    ed.memos.insert(key.clone(), serde_json::json!({ "text": "from yesterday" }));
    assert!(ed.workspace.memo.is_none());
    assert!(mark(&mut ed).1.contains(crate::term::MEMO_MARK), "{}", mark(&mut ed).1);
    ed.memos.insert(key, serde_json::json!({ "text": "" }));
    assert!(!mark(&mut ed).1.contains(crate::term::MEMO_MARK), "{}", mark(&mut ed).1);
}

/// Its measure is 40 characters: a line of 40 stays whole with the cursor after it on its row, the 41st wraps.
#[test]
fn a_line_of_the_measure_stays_whole_with_the_cursor_after_it() {
    let dir = crate::scratch::Scratch::new("memo-measure");
    let mut ed = editor(&dir);
    let forty = "Next: the memo docks beside the code too";
    assert_eq!(forty.chars().count(), MEASURE);
    feed(&mut ed, &format!(" M{forty}"));
    let rows = frame(&mut ed);
    let r = drawn(&ed).unwrap();
    assert!(rows[r.y + 1].contains(forty), "whole on its row: {}", rows[r.y + 1]);
    assert!(!rows[r.y + 2].contains("too"), "nothing wrapped: {}", rows[r.y + 2]);
    let mut buf = Vec::new();
    crate::term::render(&mut ed, &mut buf, W, H).unwrap();
    let mut p = vt100::Parser::new(H, W, 0);
    p.process(&buf);
    let (y, x) = p.screen().cursor_position();
    let at = (x as usize, y as usize);
    assert_eq!(at, (r.x + PAD + MEASURE, r.y + 1), "after it, on its row, inside the frame");
    assert!(at.0 < r.x + r.w - 1, "inside its side");
    feed(&mut ed, "!");
    let rows = frame(&mut ed);
    assert!(rows[r.y + 2].contains('!'), "the 41st wraps: {}", rows[r.y + 2]);
}

/// Twelve lines: as tall as half the screen, then it scrolls — the cursor's row in sight.
#[test]
fn the_memo_grows_with_its_rows_up_to_half_the_screen_then_scrolls() {
    let dir = crate::scratch::Scratch::new("memo-tall");
    let mut ed = editor(&dir);
    feed(&mut ed, " M");
    for i in 1..=30 {
        feed(&mut ed, &format!("line {i}<ret>"));
    }
    feed(&mut ed, "last");
    let rows = frame(&mut ed);
    let r = drawn(&ed).unwrap();
    // (39 rows above the status line: half is 19)
    assert_eq!(r.h, 19, "half the screen at most");
    assert_eq!(r.y, 10, "its top still at a quarter");
    assert!(rows[r.y + r.h - 2].contains("last"), "the cursor's row in sight: {}", rows[r.y + r.h - 2]);
    // (a line wider than 36 cells wraps, never cut)
    feed(&mut ed, " and a line that is longer than the memo is wide");
    let rows = frame(&mut ed);
    let r = drawn(&ed).unwrap();
    let body: String = rows[r.y + 1..r.y + r.h - 1].join("\n");
    assert!(body.contains("wide"), "wrapped in sight:\n{body}");
}

/// Leaving it puts it away — `space M` again (normal mode), `C-w w`, a press on the code: not drawn, the keys the code's,
/// its words kept; `space M` brings it back with them, the cursor at their end, typing.
#[test]
fn leaving_the_memo_puts_it_away_and_it_comes_back_with_its_words() {
    let dir = crate::scratch::Scratch::new("memo-away");
    let mut ed = editor(&dir);
    feed(&mut ed, " Mfirst words<esc> M");
    assert!(drawn(&ed).is_none(), "space M put it away");
    assert_eq!(ed.keys_to().region, Region::Code);
    feed(&mut ed, " M");
    assert!(drawn(&ed).is_some() && ed.memo_has_keys(), "back");
    assert_eq!(ed.workspace.memo.as_ref().map(Memo::mode), Some(Mode::Insert), "typing again");
    feed(&mut ed, ", more");
    assert_eq!(text(&ed), "first words, more", "at the end of its words");
    // `esc`, `C-w w`: to the code, put away
    feed(&mut ed, "<esc><C-w>w");
    assert!(drawn(&ed).is_none(), "C-w w put it away");
    assert_eq!(ed.keys_to().region, Region::Code);
    // a press on the code: put away
    feed(&mut ed, " M");
    let r = drawn(&ed).unwrap();
    let at = (r.x + r.w + 4, r.y + 1);
    press(&mut ed, MouseKind::Down, at.0, at.1);
    press(&mut ed, MouseKind::Up, at.0, at.1);
    assert_eq!(ed.keys_to().region, Region::Code, "the keys the code's: {:?}", ed.status);
    assert!(drawn(&ed).is_none(), "a press on the code put it away");
    assert_eq!(text(&ed), "first words, more", "kept");
    // `:memo` brings it, and puts it away
    feed(&mut ed, ":memo<ret>");
    assert!(ed.memo_has_keys());
    feed(&mut ed, "<esc>:memo<ret>");
    assert!(drawn(&ed).is_none(), ":memo put it away");
}

/// `C-w` in its insert mode is the chat's input's: with text it deletes a word, the memo keeping the keys (hands that
/// know it there never put the memo away mid-sentence); empty, it goes on as the window prefix; from normal mode `C-w w`
/// leaves.
#[test]
fn c_w_deletes_a_word_while_typing_and_is_the_window_prefix_when_empty() {
    let dir = crate::scratch::Scratch::new("memo-c-w");
    let mut ed = editor(&dir);
    feed(&mut ed, " Mfirst words<C-w>");
    assert_eq!(text(&ed), "first ", "the word deleted");
    assert!(ed.memo_has_keys() && drawn(&ed).is_some(), "the memo keeps the keys");
    assert_eq!(ed.workspace.memo.as_ref().map(Memo::mode), Some(Mode::Insert));
    feed(&mut ed, "w");
    assert_eq!(text(&ed), "first w", "the next letter typed, nothing moved");
    // empty: the window prefix
    feed(&mut ed, "<C-w><C-w>");
    assert_eq!(text(&ed), "", "emptied");
    feed(&mut ed, "<C-w>w");
    assert!(drawn(&ed).is_none() && !ed.memo_up(), "empty, C-w w left it");
    assert_eq!(ed.keys_to().region, Region::Code);
    // from normal mode: C-w w leaves, its words kept
    feed(&mut ed, " Mkept<esc><C-w>w");
    assert!(!ed.memo_up(), "C-w w from normal mode left it");
    assert_eq!(text(&ed), "kept");
}

/// Put away with no frame since (a press, then `C-w w` before the next frame): the round's leaves, laid while it was up,
/// still list its pane — a hop never lands in it, put away; it comes up only by your key.
#[test]
fn a_hop_never_brings_the_memo_back_up() {
    let dir = crate::scratch::Scratch::new("memo-hop");
    let mut ed = editor(&dir);
    feed(&mut ed, " Mwords");
    let r = drawn(&ed).unwrap();
    let (x, y) = ((r.x + r.w + 4) as u16, (r.y + 1) as u16);
    for kind in [MouseKind::Down, MouseKind::Up] {
        ed.handle_event(Event::Mouse(Mouse { kind, x, y, alt: false, ctrl: false }));
    }
    assert!(!ed.memo_up(), "the press put it away");
    for k in ["C-w", "w"] {
        ed.handle_key(k.parse().unwrap());
    }
    assert!(!ed.memo_up(), "C-w w brought it back from the last frame's leaves");
    assert_eq!(ed.keys_to().region, Region::Code);
}

/// Inside it's a document like any: undo, `/` with `n` its own search (never the code's), the mouse's press and drag;
/// `:` is the editor's (`:w` the code's file).
#[test]
fn the_memo_is_edited_as_any_document() {
    let dir = crate::scratch::Scratch::new("memo-edit");
    let mut ed = editor(&dir);
    feed(&mut ed, " Malpha beta<ret>gamma beta<esc>");
    // undo: the whole stay in insert mode, one step
    feed(&mut ed, "u");
    assert_eq!(text(&ed), "", "undone");
    feed(&mut ed, "U");
    assert_eq!(text(&ed), "alpha beta\ngamma beta", "redone");
    // its own search
    feed(&mut ed, "gg/beta<ret>");
    let head = |ed: &Editor| ed.workspace.memo.as_ref().unwrap().pane.doc().selection().primary().head;
    assert_eq!(head(&ed), "alpha beta".len(), "found in the memo");
    assert!(ed.memo_has_keys(), "the prompt gave the keys back to it");
    feed(&mut ed, "n");
    assert_eq!(head(&ed), "alpha beta\ngamma beta".len(), "n its own");
    assert_eq!(ed.search.as_deref(), None, "the code's search untouched");
    // the mouse: a press puts the cursor there, a drag selects
    let r = drawn(&ed).unwrap();
    let (x, y) = (r.x + PAD, r.y + 1);
    press(&mut ed, MouseKind::Down, x + 6, y);
    press(&mut ed, MouseKind::Drag, x + 9, y);
    press(&mut ed, MouseKind::Up, x + 9, y);
    let sel = ed.workspace.memo.as_ref().unwrap().pane.doc().selection().primary();
    assert_eq!((sel.from(), sel.to()), (6, 10), "beta selected");
    assert!(ed.memo_has_keys());
    // `:w` is the code's
    feed(&mut ed, "<esc>:w<ret>");
    assert!(ed.memo_has_keys(), "a `:` line from the memo gives it the keys back");
    assert!(!ed.doc().is_modified());
}

/// Its words are kept per workspace in the state folder — written on a worker as you leave it, keyed by the checkout's
/// real path — and come back in the next tarae.
#[test]
fn the_memos_words_are_kept_across_a_restart() {
    let dir = crate::scratch::Scratch::new("memo-kept");
    let file = dir.join("state").join("memos.json");
    let mut ed = editor(&dir);
    ed.memos_path = Some(file.clone());
    feed(&mut ed, " Mwhat comes next");
    assert!(!file.exists(), "nothing written while you type");
    feed(&mut ed, "<esc><C-w>w");
    let key = ed.workspace.root.to_string_lossy().into_owned();
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    let kept = loop {
        let kept = read(Some(&file));
        if kept.contains_key(&key) || std::time::Instant::now() > end {
            break kept;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(kept.get(&key).and_then(|v| v["text"].as_str()), Some("what comes next"), "{kept:?}");
    // the next tarae in this checkout
    let mut next = editor(&dir);
    (next.memos_path, next.memos) = (Some(file.clone()), read(Some(&file)));
    feed(&mut next, " M, then the clip");
    assert_eq!(text(&next), "what comes next, then the clip");
    // another checkout's memo is its own
    let other = crate::scratch::Scratch::new("memo-other");
    let mut elsewhere = editor(&other);
    (elsewhere.memos_path, elsewhere.memos) = (Some(file.clone()), read(Some(&file)));
    feed(&mut elsewhere, " M");
    assert_eq!(text(&elsewhere), "");
}

/// A switch to another workspace while the memo is up (a `:` line from its normal mode): it's put away there, its words
/// kept as it goes behind — quit and `:restart` keep only the front workspace's memo.
#[test]
fn a_workspace_switch_puts_the_memo_away_and_keeps_its_words() {
    let dir = crate::scratch::Scratch::new("memo-switch");
    let other = crate::scratch::Scratch::new("memo-switch-other");
    let file = dir.join("state").join("memos.json");
    let mut ed = editor(&dir);
    ed.memos_path = Some(file.clone());
    let (home, key) = (ed.workspace.id, ed.workspace.root.to_string_lossy().into_owned());
    let to = std::fs::canonicalize(other.as_ref()).unwrap();
    feed(&mut ed, &format!(" Mbefore the switch<esc>:workspace-open {}<ret>", to.display()));
    assert_ne!(ed.workspace.id, home, "in the other workspace");
    let behind = ed.workspaces.iter().find(|w| w.id == home).expect("behind");
    assert!(behind.memo.as_ref().is_some_and(|m| !m.up), "put away there");
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    let kept = loop {
        let kept = read(Some(&file));
        if kept.contains_key(&key) || std::time::Instant::now() > end {
            break kept;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(kept.get(&key).and_then(|v| v["text"].as_str()), Some("before the switch"), "{kept:?}");
    // back: down, its words there for `space M`
    assert!(ed.workspace_switch(home).is_in());
    frame(&mut ed);
    assert!(drawn(&ed).is_none() && !ed.memo_up());
    feed(&mut ed, " M");
    assert_eq!(text(&ed), "before the switch");
}

/// Its rect: 40 wide by default, as tall as its rows (three at least, half the screen at most), centred with its top at
/// a quarter; a frame you set is kept as you set it.
#[test]
fn the_memos_rect_is_its_texts_then_yours() {
    let rows = |n: usize| move |_: usize| n;
    assert_eq!(
        (WIDTH, text_w(WIDTH)),
        (45, MEASURE + 1),
        "its measure, the cursor's cell, its sides and air"
    );
    assert_eq!(rect(FRAME, 80, 33, rows(1)), Rect { x: 17, y: 8, w: 45, h: 5 });
    assert_eq!(rect(FRAME, 80, 33, rows(12)), Rect { x: 17, y: 8, w: 45, h: 14 });
    assert_eq!(rect(FRAME, 80, 33, rows(40)), Rect { x: 17, y: 8, w: 45, h: 16 });
    assert_eq!(rect(FRAME, 30, 33, rows(1)).w, 30, "a narrow screen: its whole width");
    let was = rect(FRAME, 80, 33, rows(2));
    let moved = Rect { x: 4, ..was };
    let f = frame_of(FRAME, was, moved, 80, 33);
    assert_eq!(
        (f.y, f.w, f.h),
        (Frame::PLACED, 0, 0),
        "moved across: its top, width and height by the rule still"
    );
    assert_eq!(rect(f, 80, 33, rows(2)), moved);
    assert_eq!(rect(f, 80, 33, rows(6)).h, 8, "still as tall as its rows");
}
