//! Landing S of the mouse audit (`.sb/mouse-plan.md` §4): a drag selects in every text — the run panels' output and
//! the terminal's view mode too — a double press takes the word and a triple the line, a drag from them going by
//! words or lines; a drag held past the text's edge scrolls it a row a tick; the wheel moves the view only.

use super::{H, code_cell, editor, feed, frame, press};
use crate::editor::{Drag, Editor};
use crate::event::{Mouse, MouseKind as K};

fn mouse(ed: &mut Editor, kind: K, (x, y): (usize, usize)) {
    ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
    frame(ed);
}

/// The cell where `needle` is drawn (its first character), on the frame as text.
fn cell_of(ed: &mut Editor, needle: &str) -> (usize, usize) {
    let text = frame(ed);
    for (y, row) in text.lines().enumerate() {
        if let Some(b) = row.find(needle) {
            return (row[..b].chars().count(), y);
        }
    }
    panic!("{needle:?} on screen:\n{text}");
}

/// What's selected in `on`'s text (the primary range).
fn selected(ed: &mut Editor, on: Drag) -> String {
    let d = crate::mouse::text_doc(ed, on).expect("its document");
    let r = d.selection().primary();
    d.text.byte_slice(r.from()..r.to()).to_string()
}

fn presses(ed: &mut Editor, at: (usize, usize), n: usize) {
    for _ in 0..n {
        press(ed, at.0, at.1);
    }
}

fn with_code(ed: &mut Editor, lines: usize) {
    let text: String = (0..lines).map(|i| format!("let alpha_{i} = beta + gamma;\n")).collect();
    ed.docs[0].text = ropey::Rope::from_str(&text);
    frame(ed);
}

/// In the code: two presses at once on a word take it, three the line, a fourth starts again; a drag from a double
/// takes whole words, back over the press too.
#[test]
fn a_double_press_in_the_code_takes_the_word_and_a_triple_the_line() {
    crate::clock::hold();
    let mut ed = editor();
    with_code(&mut ed, 30);
    let at = cell_of(&mut ed, "beta + gamma");
    presses(&mut ed, at, 2);
    assert_eq!(selected(&mut ed, Drag::Code), "beta", "a double: the word");
    press(&mut ed, at.0, at.1);
    assert_eq!(selected(&mut ed, Drag::Code), "let alpha_0 = beta + gamma;\n", "a triple: the line");
    press(&mut ed, at.0, at.1);
    assert!(ed.doc().selection().primary().is_empty(), "a fourth: the cursor only");
    // (the double again, then a drag onto `gamma` and back onto `alpha_0`: whole words)
    let mut ed = editor();
    with_code(&mut ed, 30);
    let gamma = cell_of(&mut ed, "amma;");
    mouse(&mut ed, K::Down, at);
    mouse(&mut ed, K::Up, at);
    mouse(&mut ed, K::Down, at);
    mouse(&mut ed, K::Drag, gamma);
    assert_eq!(selected(&mut ed, Drag::Code), "beta + gamma", "by words");
    let alpha = cell_of(&mut ed, "pha_0");
    mouse(&mut ed, K::Drag, alpha);
    assert_eq!(selected(&mut ed, Drag::Code), "alpha_0 = beta", "back over the press: its word kept");
    mouse(&mut ed, K::Up, alpha);
    // (presses apart in time are presses, not a double)
    press(&mut ed, at.0, at.1);
    crate::clock::advance(crate::editor::DOUBLE_CLICK);
    press(&mut ed, at.0, at.1);
    assert!(ed.doc().selection().primary().is_empty(), "two presses apart: the cursor only");
    crate::clock::release();
}

/// A drag held past the code's last row scrolls it a row a tick, the selection reaching the row now at the edge; the
/// view stays where the drag left it; back inside, or let go, it stops.
#[test]
fn a_drag_past_the_codes_edge_scrolls_it_a_row_a_tick() {
    crate::clock::hold();
    let mut ed = editor();
    with_code(&mut ed, 200);
    let at = code_cell(&ed, 3);
    mouse(&mut ed, K::Down, at);
    mouse(&mut ed, K::Drag, (at.0, H as usize - 1));
    let (top, head) = (ed.doc().top, ed.doc().selection().primary().head);
    assert!(ed.mouse_edge.is_some(), "held past its edge");
    assert!(!crate::mouse::edge_tick(&mut ed), "not before its time");
    for n in 1..=3 {
        crate::clock::advance(crate::mouse::EDGE_EVERY);
        assert!(crate::mouse::edge_tick(&mut ed), "a row on");
        frame(&mut ed);
        assert_eq!(ed.doc().top, top + n, "a row a tick");
    }
    assert!(ed.doc().selection().primary().head > head, "the selection reached the rows that came");
    assert_eq!(ed.doc().selection().primary().anchor, ed.doc().text.line_to_byte(3) + 2, "from the press");
    // (back inside: no more)
    let inside = code_cell(&ed, 6);
    mouse(&mut ed, K::Drag, inside);
    assert!(ed.mouse_edge.is_none(), "back inside, held no longer");
    // (past the top, let go: no more)
    mouse(&mut ed, K::Drag, (at.0, 0));
    assert!(ed.mouse_edge.is_some(), "past the top");
    crate::clock::advance(crate::mouse::EDGE_EVERY);
    assert!(crate::mouse::edge_tick(&mut ed));
    frame(&mut ed);
    assert_eq!(ed.doc().top, top + 2, "up a row");
    mouse(&mut ed, K::Up, (at.0, 0));
    // (let go there: to the row drawn at the edge now — a tick's drag read the frame before its scroll)
    let first = ed.screen.panes[0].3.first().map(|l| l.0).expect("its rows");
    let head = ed.doc().selection().primary().head;
    assert_eq!(crate::movement::line_of(&ed.doc().text, head), first, "to the first row drawn");
    crate::clock::advance(crate::mouse::EDGE_EVERY);
    assert!(!crate::mouse::edge_tick(&mut ed), "let go: still");
    assert!(ed.tick_wanted().is_none_or(|d| d > crate::mouse::EDGE_EVERY), "no tick asked for it");
    crate::clock::release();
}

fn with_output(ed: &mut Editor, lines: usize) {
    let lines = (0..lines).map(|i| format!("output line {i} of the run")).collect();
    ed.workspace.test_run = Some(crate::testing::TestRun::finished("t", Vec::new(), lines));
    ed.workspace.viewer.slot_front = Some(crate::sizes::Slot::Test);
    frame(ed);
}

/// A run panel's output: a drag selects what's under it, a double its word, a drag held past its last row scrolls it;
/// the wheel moves its view and never its cursor or selection.
#[test]
fn a_drag_over_run_output_selects_and_the_wheel_keeps_it() {
    crate::clock::hold();
    let on = Drag::Output(crate::sizes::Slot::Test);
    let mut ed = editor();
    with_output(&mut ed, 200);
    let from = cell_of(&mut ed, "line 192 of");
    let to = cell_of(&mut ed, "line 193 of");
    let to = (to.0 + "line 193 o".len(), to.1);
    mouse(&mut ed, K::Down, from);
    mouse(&mut ed, K::Drag, to);
    mouse(&mut ed, K::Up, to);
    assert_eq!(selected(&mut ed, on), "line 192 of the run\noutput line 193 of", "dragged over");
    // (the wheel: the view, never the selection)
    let sel = crate::mouse::text_doc(&mut ed, on).unwrap().selection().clone();
    let top = crate::mouse::text_doc(&mut ed, on).unwrap().top;
    for _ in 0..4 {
        mouse(&mut ed, K::ScrollUp, from);
    }
    let d = crate::mouse::text_doc(&mut ed, on).unwrap();
    assert_eq!((d.top, d.selection()), (top - 12, &sel), "the view moved, the selection where it was");
    // (a double: its word)
    let word = cell_of(&mut ed, "line 185 ");
    presses(&mut ed, (word.0 + 1, word.1), 2);
    assert_eq!(selected(&mut ed, on), "line", "a double: the word");
    // (held past its first row: it scrolls up a row a tick)
    let rows = ed.screen.run_rows.of(crate::sizes::Slot::Test).expect("drawn");
    let top = crate::mouse::text_doc(&mut ed, on).unwrap().top;
    mouse(&mut ed, K::Down, word);
    mouse(&mut ed, K::Drag, (word.0, rows.1 - 1));
    crate::clock::advance(crate::mouse::EDGE_EVERY);
    assert!(crate::mouse::edge_tick(&mut ed), "a row on");
    frame(&mut ed);
    assert_eq!(crate::mouse::text_doc(&mut ed, on).unwrap().top, top - 1, "scrolled a row");
    let first = crate::mouse::text_doc(&mut ed, on).unwrap().text.line_to_byte(top - 1);
    let head = crate::mouse::text_doc(&mut ed, on).unwrap().selection().primary().head;
    assert_eq!(head, first + "output ".len(), "selected up to the pointer's column on the row now first");
    crate::clock::release();
}

/// Claude's edit review: a drag selects what it shows — the file's lines and the ones a change proposes alike — kept
/// as the button is let go (and copied); a key of yours ends it, and `y` still answers the change, never copies.
#[test]
fn a_drag_over_an_edit_review_selects_its_text() {
    let mut ed = editor();
    ed.docs[0].text = ropey::Rope::from_str("let a = 1;\nlet b = 2;\nlet c = 3;\n");
    let doc = ed.doc().id;
    let change = vec![(11, 22, "let b = 2;\n".into(), "let b = 20;\n".into())];
    crate::llm::review_with(&mut ed, doc, "change it", change, None);
    frame(&mut ed);
    let from = cell_of(&mut ed, "a = 1");
    let to = cell_of(&mut ed, "b = 20");
    mouse(&mut ed, K::Down, from);
    assert_eq!(crate::mouse::review_marked(&ed), None, "a press selects nothing");
    mouse(&mut ed, K::Drag, (to.0 + 4, to.1));
    let marked = crate::mouse::review_marked(&ed).expect("selected");
    assert!(marked.starts_with("a = 1;\n") && marked.ends_with("let b = 2"), "{marked:?}");
    assert!(marked.contains("let b = 2;\n"), "the line it takes out too: {marked:?}");
    mouse(&mut ed, K::Up, (to.0 + 4, to.1));
    assert_eq!(crate::mouse::review_marked(&ed).as_ref(), Some(&marked), "kept as it's let go");
    // (and copied, as the terminal's selection is — the copy's toast once the clipboard took it)
    let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the copy's result");
    ed.handle_event(ev);
    assert!(crate::clipboard::written(&marked), "copied as it's let go: {marked:?}");
    assert_eq!(ed.keys_to().region, crate::uistate::Region::EditReview, "its keys");
    feed(&mut ed, "<C-e>");
    assert_eq!(crate::mouse::review_marked(&ed), None, "a key ends it");
    // (a press alone, let go: nothing; `y` answers)
    press(&mut ed, from.0, from.1);
    assert_eq!(crate::mouse::review_marked(&ed), None);
    feed(&mut ed, "y");
    assert!(ed.workspace.claude.review.is_none(), "y accepted the change");
    assert_eq!(ed.doc().text.to_string(), "let a = 1;\nlet b = 20;\nlet c = 3;\n");
}

/// The chat's conversation and its input: a double takes the word.
#[test]
fn a_double_press_in_the_chat_takes_the_word() {
    crate::clock::hold();
    let mut ed = editor();
    crate::uistate::tests::chat(&mut ed, "a draft to send");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, "a question", ""));
    c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Assistant, "An answer, in several words.", ""));
    let at = cell_of(&mut ed, "several");
    presses(&mut ed, (at.0 + 2, at.1), 2);
    assert_eq!(selected(&mut ed, Drag::Transcript), "several", "the conversation's word");
    let at = cell_of(&mut ed, "draft");
    presses(&mut ed, (at.0 + 1, at.1), 2);
    assert_eq!(selected(&mut ed, Drag::Input), "draft", "the input's word");
    press(&mut ed, at.0 + 1, at.1);
    assert_eq!(selected(&mut ed, Drag::Input), "a draft to send", "a triple: its line");
    crate::clock::release();
}

/// The terminal's view mode: a drag selects (it said it didn't before), a double its word.
#[test]
fn a_drag_in_the_terminals_view_selects() {
    crate::clock::hold();
    let mut ed = editor();
    let _sent = crate::terminal::stand_in(&mut ed, 10, 60, 100);
    let t = ed.workspace.terminal.as_mut().unwrap();
    t.parser.process(b"$ cargo build\r\nCompiling tarae v0.1.0\r\nFinished in 4.2s\r\n");
    frame(&mut ed);
    feed(&mut ed, "<C-\\>");
    assert!(crate::view::terminal_viewed(&ed), "its view mode");
    let from = cell_of(&mut ed, "tarae v0");
    let to = cell_of(&mut ed, "in 4.2s");
    mouse(&mut ed, K::Down, from);
    mouse(&mut ed, K::Drag, to);
    mouse(&mut ed, K::Up, to);
    let on = Drag::TerminalView;
    assert_eq!(selected(&mut ed, on), "tarae v0.1.0\nFinished i", "dragged over");
    assert!(!ed.messages.iter().any(|m| m.2.contains("doesn't select")), "no longer said");
    presses(&mut ed, (from.0 + 1, from.1), 2);
    assert_eq!(selected(&mut ed, on), "tarae", "a double: the word");
    crate::clock::release();
}

/// A terminal of `lines` lines printed, in its view mode — the receiver keeps its stand-in's other end.
fn with_terminal(ed: &mut Editor, lines: usize) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let sent = crate::terminal::stand_in(ed, 10, 60, 200);
    let t = ed.workspace.terminal.as_mut().unwrap();
    for i in 0..lines {
        t.parser.process(format!("out {i:03} here\r\n").as_bytes());
    }
    frame(ed);
    feed(ed, "<C-\\>");
    assert!(crate::view::terminal_viewed(ed), "its view mode");
    sent
}

/// The terminal's view: a press near its edge (inside the scrolloff margin) doesn't move its rows, so a double there
/// takes the word pressed; the wheel moves the view and never the selection; a drag held past its top scrolls it.
#[test]
fn the_terminals_view_holds_still_under_a_press_and_the_wheel() {
    crate::clock::hold();
    let on = Drag::TerminalView;
    let mut ed = editor();
    let _sent = with_terminal(&mut ed, 60);
    let top = crate::mouse::text_doc(&mut ed, on).unwrap().top;
    let rows = ed.screen.terminal_rows.expect("drawn");
    let first = rows.0 + crate::chrome::SLOT_HEAD;
    // (its first row: inside the margin — on its number, `out 051 here`)
    let x = ed.screen.slot_x + crate::chrome::PAD + "out 0".len();
    presses(&mut ed, (x, first), 2);
    let d = crate::mouse::text_doc(&mut ed, on).unwrap();
    assert_eq!(d.top, top, "the rows under the pointer didn't move");
    let word = format!("{:03}", top);
    assert_eq!(selected(&mut ed, on), word, "the word pressed");
    // (the wheel: the view only)
    let sel = crate::mouse::text_doc(&mut ed, on).unwrap().selection().clone();
    crate::clock::advance(crate::editor::DOUBLE_CLICK);
    for _ in 0..2 {
        mouse(&mut ed, K::ScrollUp, (x, first + 3));
    }
    let d = crate::mouse::text_doc(&mut ed, on).unwrap();
    assert_eq!((d.top, d.selection()), (top - 6, &sel), "the view moved, the selection where it was");
    // (held past its top: a row a tick)
    let top = d.top;
    mouse(&mut ed, K::Down, (x, first + 2));
    mouse(&mut ed, K::Drag, (x, first - 1));
    crate::clock::advance(crate::mouse::EDGE_EVERY);
    assert!(crate::mouse::edge_tick(&mut ed), "a row on");
    assert_eq!(crate::mouse::text_doc(&mut ed, on).unwrap().top, top - 1, "scrolled a row");
    crate::clock::release();
}

/// A drag held past its text's end, where nothing more scrolls: the ticks stop (no frame 20 a second, nor forever
/// when the release is lost) — back to the edge again, they start again.
#[test]
fn a_drag_held_past_the_end_stops_ticking() {
    crate::clock::hold();
    // (held below the last row until it stops: the ticks it took, the view's top, the line on its bottom row)
    let held = |lines: usize| {
        let mut ed = editor();
        with_code(&mut ed, lines);
        let at = code_cell(&ed, 1);
        mouse(&mut ed, K::Down, at);
        mouse(&mut ed, K::Drag, (at.0, H as usize - 1));
        let mut ticks = 0;
        for _ in 0..100 {
            crate::clock::advance(crate::mouse::EDGE_EVERY);
            if !crate::mouse::edge_tick(&mut ed) {
                break;
            }
            frame(&mut ed);
            ticks += 1;
        }
        assert!(ed.mouse_edge.is_none(), "held no longer");
        assert!(ed.tick_wanted().is_none_or(|d| d > crate::mouse::EDGE_EVERY), "no tick asked for it");
        let bottom = ed.screen.panes[0].3.iter().map(|l| l.0).max().expect("its rows");
        (ticks, ed.doc().top, bottom)
    };
    // (a file shorter than the pane: nothing to scroll, it stays in sight)
    assert_eq!(held(5), (0, 0, 4), "a short file doesn't move");
    // (a long one: until its last line is on the bottom row, no further)
    let (ticks, top, bottom) = held(60);
    assert_eq!(bottom, 59, "its last line on the bottom row");
    assert_eq!(ticks, top, "a row a tick, then still");
    assert!(top > 0, "it scrolled");
    crate::clock::release();
}

/// A run's output held past its last row: it scrolls toward its end and stops with its last line on the bottom row —
/// never moved out of sight (the wheel and `zj` keep their own limit).
#[test]
fn a_drag_past_the_outputs_end_stops_at_its_last_row() {
    crate::clock::hold();
    let on = Drag::Output(crate::sizes::Slot::Test);
    let mut ed = editor();
    with_output(&mut ed, 200);
    let at = cell_of(&mut ed, "line 195 ");
    mouse(&mut ed, K::Down, at);
    let end = crate::mouse::text_doc(&mut ed, on).unwrap().top;
    for _ in 0..4 {
        mouse(&mut ed, K::ScrollUp, at);
    }
    assert_eq!(crate::mouse::text_doc(&mut ed, on).unwrap().top, end - 12, "the wheel took it back");
    let rows = ed.screen.run_rows.of(crate::sizes::Slot::Test).expect("drawn");
    let first = cell_of(&mut ed, "output line");
    mouse(&mut ed, K::Down, first);
    mouse(&mut ed, K::Drag, (first.0, rows.1 + rows.3));
    let mut ticks = 0;
    for _ in 0..40 {
        crate::clock::advance(crate::mouse::EDGE_EVERY);
        if !crate::mouse::edge_tick(&mut ed) {
            break;
        }
        frame(&mut ed);
        ticks += 1;
    }
    assert!(ed.mouse_edge.is_none(), "held no longer");
    let d = crate::mouse::text_doc(&mut ed, on).unwrap();
    assert_eq!((ticks, d.top), (12, end), "a row a tick, until its last line is on the bottom row");
    let last = crate::movement::last_line(&d.text);
    assert_eq!(d.top + rows.3, last + 1, "its last line on the bottom row");
    crate::clock::release();
}

/// Held below a soft-wrapped pane's last row: it stops when its last line is on the bottom row, counted in the rows
/// the pane lays out (a wrapped line takes several), never with its last lines left below.
#[test]
fn a_drag_past_a_wrapped_panes_end_stops_with_its_last_line_in_sight() {
    crate::clock::hold();
    let mut ed = editor();
    let long = "word ".repeat(70);
    let text: String =
        (0..60).map(|i| if i % 3 == 0 { format!("{i} {long}\n") } else { format!("{i} short\n") }).collect();
    ed.docs[0].text = ropey::Rope::from_str(&text);
    ed.docs[0].wrap = Some(true);
    frame(&mut ed);
    assert!(ed.screen.panes[0].3.iter().filter(|l| l.0 == 0).count() > 1, "its long lines wrap");
    let at = code_cell(&ed, 1);
    mouse(&mut ed, K::Down, at);
    mouse(&mut ed, K::Drag, (at.0, H as usize - 1));
    for _ in 0..100 {
        crate::clock::advance(crate::mouse::EDGE_EVERY);
        if !crate::mouse::edge_tick(&mut ed) {
            break;
        }
        frame(&mut ed);
    }
    assert!(ed.mouse_edge.is_none(), "it stopped");
    assert!(ed.doc().top > 0, "it scrolled");
    let last = ed.screen.panes[0].3.last().map(|l| l.0);
    assert_eq!(last, Some(59), "its last line drawn, on the rows it has");
    crate::clock::release();
}

/// Claude's edit review held below its last row: it scrolls down a row a tick and stops with the file's last row on
/// its bottom row — never on to its last row alone.
#[test]
fn a_drag_past_the_edit_reviews_end_stops_at_its_bottom_row() {
    crate::clock::hold();
    let mut ed = editor();
    let text: String = (0..120).map(|i| format!("let v{i} = {i};\n")).collect();
    ed.docs[0].text = ropey::Rope::from_str(&text);
    let doc = ed.doc().id;
    let at = ed.doc().text.line_to_byte(60);
    let old = ed.doc().text.line(60).to_string();
    crate::llm::review_with(
        &mut ed,
        doc,
        "change it",
        vec![(at, at + old.len(), old, "let w = 0;\n".into())],
        None,
    );
    frame(&mut ed);
    let scroll = ed.workspace.claude.review.as_ref().unwrap().scroll;
    let first = ed.screen.review_text.borrow().first().map(|r| r.0).expect("its rows");
    mouse(&mut ed, K::Down, (8, first + 2));
    mouse(&mut ed, K::Drag, (8, H as usize - 1));
    let mut ticks = 0;
    for _ in 0..200 {
        crate::clock::advance(crate::mouse::EDGE_EVERY);
        if !crate::mouse::edge_tick(&mut ed) {
            break;
        }
        frame(&mut ed);
        ticks += 1;
    }
    assert!(ticks > 0 && ed.mouse_edge.is_none(), "it scrolled, then stopped ({ticks})");
    assert!(ed.workspace.claude.review.as_ref().unwrap().scroll > scroll);
    let (_, top, _, h) = ed.screen.review_area.get().expect("drawn");
    let rows = ed.workspace.claude.review.as_ref().unwrap().kept.as_ref().unwrap().rows.len();
    let last = *ed.screen.review_text.borrow().last().expect("its rows");
    assert_eq!((last.0, last.1), (top + h - 1, rows - 1), "the file's last row on its bottom row");
    crate::clock::release();
}

/// Two presses at once on the same character of the same document, in two splits: two presses, not a double.
#[test]
fn a_double_is_two_presses_in_one_pane() {
    crate::clock::hold();
    let mut ed = editor();
    with_code(&mut ed, 30);
    feed(&mut ed, "<C-w>v");
    let panes: Vec<_> = ed.screen.panes.iter().map(|p| (p.1, p.2)).collect();
    assert_eq!(panes.len(), 2, "two splits");
    let cell = |(r, g): (crate::split::Rect, usize)| {
        (r.x + g + 1, crate::chrome::pane_text(r, ed.screen.text_top).y + 2)
    };
    let (a, b) = (cell(panes[0]), cell(panes[1]));
    press(&mut ed, a.0, a.1);
    press(&mut ed, b.0, b.1);
    assert!(ed.doc().selection().primary().is_empty(), "the other pane's press: the cursor only");
    press(&mut ed, b.0, b.1);
    assert_eq!(selected(&mut ed, Drag::Code), "let", "in its own pane, again: the word");
    crate::clock::release();
}

/// The conversation: the wheel moves its view and never its selection; a drag held past its top scrolls it.
#[test]
fn the_conversations_wheel_keeps_its_selection_and_its_edge_scrolls() {
    crate::clock::hold();
    let on = Drag::Transcript;
    let mut ed = editor();
    crate::uistate::tests::chat(&mut ed, "");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    for i in 0..30 {
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, format!("question {i}"), ""));
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Assistant, format!("answer number {i}"), ""));
    }
    frame(&mut ed);
    let at = cell_of(&mut ed, "number 27");
    presses(&mut ed, (at.0 + 1, at.1), 2);
    assert_eq!(selected(&mut ed, on), "number");
    let d = crate::mouse::text_doc(&mut ed, on).unwrap();
    let (sel, top) = (d.selection().clone(), (d.top, d.top_rows));
    crate::clock::advance(crate::editor::DOUBLE_CLICK);
    for _ in 0..3 {
        mouse(&mut ed, K::ScrollUp, at);
    }
    let d = crate::mouse::text_doc(&mut ed, on).unwrap();
    assert_ne!((d.top, d.top_rows), top, "the view moved");
    assert_eq!(d.selection(), &sel, "the selection where it was");
    // (held past its top: a row a tick)
    let (y0, ..) = ed.screen.chat_view.get().expect("drawn");
    let top = crate::mouse::text_doc(&mut ed, on).map(|d| (d.top, d.top_rows)).unwrap();
    mouse(&mut ed, K::Down, (at.0, y0 + 2));
    mouse(&mut ed, K::Drag, (at.0, y0 - 1));
    crate::clock::advance(crate::mouse::EDGE_EVERY);
    assert!(crate::mouse::edge_tick(&mut ed), "a row on");
    let now = crate::mouse::text_doc(&mut ed, on).map(|d| (d.top, d.top_rows)).unwrap();
    assert_ne!(now, top, "scrolled a row");
    crate::clock::release();
}

/// Claude's edit review held past its first row: it scrolls a row a tick, the selection reaching it.
#[test]
fn a_drag_past_the_edit_reviews_edge_scrolls_it() {
    crate::clock::hold();
    let mut ed = editor();
    let text: String = (0..120).map(|i| format!("let v{i} = {i};\n")).collect();
    ed.docs[0].text = ropey::Rope::from_str(&text);
    let doc = ed.doc().id;
    let at = ed.doc().text.line_to_byte(80);
    let old = ed.doc().text.line(80).to_string();
    crate::llm::review_with(
        &mut ed,
        doc,
        "change it",
        vec![(at, at + old.len(), old, "let w = 0;\n".into())],
        None,
    );
    frame(&mut ed);
    let scroll = ed.workspace.claude.review.as_ref().unwrap().scroll;
    assert!(scroll > 0, "scrolled to its change");
    let first = ed.screen.review_text.borrow().first().map(|r| r.0).expect("its rows");
    mouse(&mut ed, K::Down, (8, first + 2));
    mouse(&mut ed, K::Drag, (8, first - 1));
    crate::clock::advance(crate::mouse::EDGE_EVERY);
    assert!(crate::mouse::edge_tick(&mut ed), "a row on");
    frame(&mut ed);
    assert_eq!(ed.workspace.claude.review.as_ref().unwrap().scroll, scroll - 1, "up a row");
    assert!(crate::mouse::review_marked(&ed).is_some(), "selected up to it");
    crate::clock::release();
}
