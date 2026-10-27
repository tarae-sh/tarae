//! Landing O of the mouse audit (`.sb/mouse-plan.md` §4): which-key's rows press as their keys, completion's accept,
//! its wheel moves through it; the note card's press replies; the wheel over a cut card scrolls its body, a drag from a
//! card says why it selects nothing; the start screen's rows press their keys; a press ends a key sequence under way;
//! the middle button pastes where it lands, the wheel sideways scrolls the code's columns.

use super::{code_cell, editor, feed, frame, press, said};
use crate::editor::Editor;
use crate::event::{Mouse, MouseKind};
use crate::uistate::Region;

fn mouse(ed: &mut Editor, kind: MouseKind, x: usize, y: usize) {
    ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
    frame(ed);
}

/// A which-key row's cell, by the key it presses.
fn which_key_row(ed: &Editor, key: &str) -> (usize, usize) {
    let rows = ed.screen.which_key_rows.borrow();
    let r = rows.iter().find(|r| r.3.to_string() == key).unwrap_or_else(|| panic!("{key} drawn: {rows:?}"));
    (r.1 + 1, r.0)
}

#[test]
fn a_press_on_a_which_key_row_presses_its_key() {
    let mut ed = editor();
    feed(&mut ed, "<C-w>");
    assert!(ed.screen.which_key.get().is_some(), "which-key up");
    let (x, y) = which_key_row(&ed, "v");
    press(&mut ed, x, y);
    assert_eq!(ed.screen.panes.len(), 2, "`C-w v` split the pane");
    assert!(ed.pending.is_empty(), "the sequence done");
    // (the same place again at once: that one press — no second split, nothing of the code now under it pressed)
    let after = (ed.workspace.viewer.focus, ed.doc().selection().clone());
    press(&mut ed, x, y);
    assert_eq!(ed.screen.panes.len(), 2, "a double is one press");
    assert_eq!(
        (ed.workspace.viewer.focus, ed.doc().selection().clone()),
        after,
        "the code under it unpressed"
    );
}

#[test]
fn a_press_off_which_key_ends_the_sequence_then_acts_there() {
    let mut ed = editor();
    feed(&mut ed, "<C-w>");
    let (x, y) = code_cell(&ed, 5);
    press(&mut ed, x, y);
    assert!(ed.pending.is_empty(), "the sequence ended");
    assert!(ed.screen.which_key.get().is_none(), "which-key gone");
    assert_eq!(ed.doc().selection().primary().head, crate::movement::line_start(&ed.doc().text, 5) + 2);
    // (the next key is a key of its own, not the prefix's)
    feed(&mut ed, "v");
    assert_eq!(ed.screen.panes.len(), 1, "`v` after the press isn't `C-w v`");
}

fn completing(ed: &mut Editor) {
    feed(ed, "A");
    let items: Vec<serde_json::Value> =
        (0..30).map(|i| serde_json::json!({"label": format!("word_{i}"), "detail": "i32"})).collect();
    let at = ed.doc().selection().primary().head;
    let mut c = crate::completion::Completion::new(0, ed.doc().id, at, serde_json::Value::from(items));
    c.filter("");
    c.selected = Some(0);
    ed.completion = Some(c);
    frame(ed);
}

#[test]
fn a_press_on_a_completion_row_accepts_it_and_the_wheel_moves_through_it() {
    let mut ed = editor();
    completing(&mut ed);
    let top = ed.doc().top;
    let (x, y, ..) = ed.screen.completion_card.get().expect("the menu drawn");
    mouse(&mut ed, MouseKind::ScrollDown, x + 4, y + 2);
    assert_eq!(ed.completion.as_ref().and_then(|c| c.selected), Some(3), "three rows on");
    mouse(&mut ed, MouseKind::ScrollUp, x + 4, y + 2);
    mouse(&mut ed, MouseKind::ScrollUp, x + 4, y + 2);
    assert_eq!(ed.completion.as_ref().and_then(|c| c.selected), Some(0), "stopped at the first, never round");
    assert_eq!(ed.doc().top, top, "the code under it unscrolled");
    let row = ed.screen.completion_rows.borrow().iter().find(|r| r.1 == 2).map(|r| r.0).expect("row 2");
    let label = ed.completion.as_ref().map(|c| c.item(2).label.clone()).unwrap_or_default();
    press(&mut ed, x + 4, row);
    assert!(ed.completion.is_none(), "accepted, closed");
    assert!(ed.doc().text.line(0).to_string().ends_with(&format!("{label}\n")), "{}", ed.doc().text.line(0));
    assert_eq!(ed.mode, crate::editor::Mode::Insert, "still typing");
}

#[test]
fn a_press_on_the_note_card_replies_and_nothing_under_it_is_pressed() {
    let mut ed = editor();
    let path = std::env::temp_dir().join("tarae-mouse-overlays-note.rs");
    ed.docs[0].path = Some(path.clone());
    crate::notes::add(&mut ed, &path, 0..1, crate::notes::By::Author, "A word about this line.");
    frame(&mut ed);
    let before = ed.doc().selection().primary();
    let (x, y, w, h) = ed.screen.note_card.get().expect("the note card drawn");
    press(&mut ed, x + w / 2, y + h / 2);
    assert!(
        matches!(ed.prompt.as_ref().map(|p| p.kind), Some(crate::editor::PromptKind::Note(_))),
        "its reply's line open"
    );
    assert_eq!(ed.doc().selection().primary(), before, "the code under it unpressed");
    // (pressed again, later — not the double's second press — replying: said, the line kept)
    ed.pressed_through = None;
    let (x, y, w, h) = ed.screen.note_card.get().expect("the note card drawn beside its reply's line");
    mouse(&mut ed, MouseKind::Down, x + w / 2, y + h / 2);
    mouse(&mut ed, MouseKind::Up, x + w / 2, y + h / 2);
    assert!(ed.prompt.is_some(), "the reply's line kept");
    assert!(said(&ed).starts_with("replying"), "said: {}", said(&ed));
}

/// Another prompt open (`:` typed, the card last drawn under it): a press where the note card was is outside that prompt
/// — it closes as `esc`, never replaced by the note's reply.
#[test]
fn a_press_where_the_note_card_was_closes_another_prompt_and_never_replies() {
    let mut ed = editor();
    let path = std::env::temp_dir().join("tarae-mouse-overlays-note-prompt.rs");
    ed.docs[0].path = Some(path.clone());
    crate::notes::add(&mut ed, &path, 0..1, crate::notes::By::Author, "A word about this line.");
    frame(&mut ed);
    let (x, y, w, h) = ed.screen.note_card.get().expect("the note card drawn");
    ed.handle_key(crate::key::Key::plain(crate::key::Code::Char(':')));
    for kind in [MouseKind::Down, MouseKind::Up] {
        ed.handle_mouse(Mouse {
            kind,
            x: (x + w / 2) as u16,
            y: (y + h / 2) as u16,
            alt: false,
            ctrl: false,
        });
    }
    assert!(ed.prompt.is_none(), "the `:` line closed as esc, not replaced by the note's reply");
}

fn long_card(ed: &mut Editor) {
    let body: Vec<String> = (0..80).map(|i| format!("line {i} of what it asks")).collect();
    let what = crate::offer::What::ReviewAtEnd;
    ed.push_offer(crate::offer::Offer::new("A long card".into(), &body.join("\n"), "", String::new(), what));
    frame(ed);
}

#[test]
fn the_wheel_over_a_cut_card_scrolls_its_body_to_its_end() {
    let mut ed = editor();
    long_card(&mut ed);
    let shown = |ed: &mut Editor| frame(ed);
    assert!(shown(&mut ed).contains("line 0 of what it asks"), "its top");
    assert!(!ed.offer_seen_whole(), "cut: not read whole");
    let (x, y, w, h) = ed.screen.offer_card.expect("the card drawn");
    let (cx, cy) = (x + w / 2, y + h / 2);
    mouse(&mut ed, MouseKind::ScrollDown, cx, cy);
    let text = shown(&mut ed);
    assert!(!text.contains("line 0 of what it asks") && text.contains("line 3 of what it asks"), "{text}");
    assert!(text.contains("A long card"), "its title stays");
    for _ in 0..40 {
        mouse(&mut ed, MouseKind::ScrollDown, cx, cy);
    }
    let text = shown(&mut ed);
    assert!(text.contains("line 79 of what it asks") && text.contains("lines above"), "{text}");
    assert!(ed.offer_seen_whole(), "scrolled to its end: read whole");
    mouse(&mut ed, MouseKind::ScrollUp, cx, cy);
    assert!(shown(&mut ed).contains("more lines"), "back up: its end cut again");
}

/// Read whole only once every row of the body was drawn (#84): the wheel stepped past rows between frames — its end in
/// sight, the rows it skipped never drawn — isn't read whole until they are.
#[test]
fn a_cut_card_wheeled_past_rows_between_frames_is_not_read_whole_until_each_was_drawn() {
    let mut ed = editor();
    long_card(&mut ed);
    let (x, y, w, h) = ed.screen.offer_card.expect("the card drawn");
    let (cx, cy) = (x + w / 2, y + h / 2);
    for _ in 0..40 {
        ed.handle_mouse(Mouse {
            kind: MouseKind::ScrollDown,
            x: cx as u16,
            y: cy as u16,
            alt: false,
            ctrl: false,
        });
    }
    let text = frame(&mut ed);
    assert!(text.contains("line 79 of what it asks") && text.contains("lines above"), "its end: {text}");
    assert!(!ed.offer_seen_whole(), "its end in sight, the rows between never drawn: not read whole");
    for _ in 0..40 {
        mouse(&mut ed, MouseKind::ScrollUp, cx, cy);
    }
    assert!(frame(&mut ed).contains("line 0 of what it asks"), "back at its top");
    assert!(ed.offer_seen_whole(), "every row drawn on the way back: read whole");
}

/// A double-click on a row whose press made what was under it go: the second press is that one press — the start
/// screen's `space ?` opens the palette and picks nothing in it, a completion accepted leaves the cursor where the
/// accept put it.
#[test]
fn a_double_click_where_the_first_press_opened_or_closed_something_is_one_press() {
    let mut ed = editor();
    ed.docs[0].text = ropey::Rope::new();
    frame(&mut ed);
    let row = ed.screen.welcome_rows.borrow().iter().find(|r| r.3 == "space ?").cloned().expect("its row");
    press(&mut ed, row.1 + 12, row.0);
    assert_eq!(ed.keys_to().region, Region::Picker, "the palette open");
    let at = |ed: &Editor| ed.picker.as_ref().and_then(|p| p.current()).map(|i| i.label.clone());
    let before = at(&ed);
    press(&mut ed, row.1 + 12, row.0);
    assert_eq!(ed.keys_to().region, Region::Picker, "still open");
    assert_eq!(at(&ed), before, "nothing picked in it");
    assert!(ed.picker_click.is_none(), "no row of it pressed");
    let mut ed = editor();
    completing(&mut ed);
    let (x, ..) = ed.screen.completion_card.get().expect("the menu drawn");
    let row = ed.screen.completion_rows.borrow().iter().find(|r| r.1 == 2).map(|r| r.0).expect("row 2");
    press(&mut ed, x + 4, row);
    assert!(ed.completion.is_none(), "accepted");
    let (sel, text) = (ed.doc().selection().clone(), ed.doc().text.clone());
    press(&mut ed, x + 4, row);
    assert_eq!((ed.doc().selection(), &ed.doc().text), (&sel, &text), "the code under it unpressed");
    assert_eq!(ed.mode, crate::editor::Mode::Insert);
}

/// The wheel over a card drawn whole does nothing and keeps nothing: cut later (a shorter screen), it opens at its top.
#[test]
fn the_wheel_over_a_card_drawn_whole_keeps_nothing_for_when_its_cut() {
    let mut ed = editor();
    let body: Vec<String> = (0..20).map(|i| format!("line {i} of what it asks")).collect();
    let what = crate::offer::What::ReviewAtEnd;
    ed.push_offer(crate::offer::Offer::new("A card".into(), &body.join("\n"), "", String::new(), what));
    let text = frame(&mut ed);
    assert!(text.contains("line 19 of what it asks") && !text.contains("more lines"), "drawn whole: {text}");
    let (x, y, w, h) = ed.screen.offer_card.expect("the card drawn");
    for _ in 0..5 {
        mouse(&mut ed, MouseKind::ScrollDown, x + w / 2, y + h / 2);
    }
    assert!(frame(&mut ed).contains("line 0 of what it asks"), "whole, unmoved");
    // (a shorter screen: the card cut — at its top)
    ed.focus_sync();
    crate::term::render(&mut ed, &mut Vec::new(), super::W, 20).unwrap();
    let short = crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), super::W, 20).unwrap();
    assert!(short.contains("more lines"), "cut now: {short}");
    assert!(short.contains("line 0 of what it asks"), "cut, it opens at its top: {short}");
}

#[test]
fn a_drag_from_a_card_says_it_selects_nothing() {
    let mut ed = editor();
    long_card(&mut ed);
    let (x, y, w, h) = ed.screen.offer_card.expect("the card drawn");
    mouse(&mut ed, MouseKind::Down, x + 3, y + h / 2);
    mouse(&mut ed, MouseKind::Drag, x + w - 3, y + h / 2);
    assert_eq!(said(&ed), crate::mouse::CARD_DRAG);
}

#[test]
fn a_press_on_a_start_screen_row_presses_its_keys() {
    let mut ed = editor();
    ed.docs[0].text = ropey::Rope::new();
    frame(&mut ed);
    let row = ed.screen.welcome_rows.borrow().iter().find(|r| r.3 == "space ?").cloned().expect("its row");
    press(&mut ed, row.1 + 12, row.0);
    assert_eq!(ed.keys_to().region, Region::Picker, "the palette open");
    // (off its rows: said)
    ed.picker_close();
    frame(&mut ed);
    press(&mut ed, 1, 1);
    assert!(said(&ed).contains("presses its keys"), "said: {}", said(&ed));
}

/// The middle button where there's no text to paste into: nothing pressed — a card's answer unanswered, which-key's
/// row unpressed, the start screen's `:q` not run — said.
#[test]
fn the_middle_button_presses_nothing_where_there_is_no_text() {
    let mut ed = editor();
    long_card(&mut ed);
    let &(y, x0, ..) = ed.screen.offer_buttons.iter().find(|b| b.3 == 'n').expect("its n drawn");
    mouse(&mut ed, MouseKind::Middle, x0, y);
    assert_eq!(ed.offers.len(), 1, "the card unanswered");
    assert!(!ed.card_has_keys(), "nor focused");
    assert_eq!(said(&ed), crate::mouse::MIDDLE_ELSEWHERE);
    let mut ed = editor();
    feed(&mut ed, "<C-w>");
    let (x, y) = which_key_row(&ed, "v");
    mouse(&mut ed, MouseKind::Middle, x, y);
    assert_eq!(ed.screen.panes.len(), 1, "`v` not pressed");
    assert_eq!(said(&ed), crate::mouse::MIDDLE_ELSEWHERE);
    let mut ed = editor();
    ed.docs[0].text = ropey::Rope::new();
    frame(&mut ed);
    let row = ed.screen.welcome_rows.borrow().iter().find(|r| r.3 == ":q").cloned().expect("its row");
    mouse(&mut ed, MouseKind::Middle, row.1 + 12, row.0);
    assert!(!ed.should_quit && ed.prompt.is_none(), "`:q` not run");
    assert_eq!(said(&ed), crate::mouse::MIDDLE_ELSEWHERE);
}

/// The middle button on the code: the cursor put on the character under the pointer, the clipboard pasted there —
/// the stand-in clipboard is every test's, so a paste of another test's copy is tried again.
#[test]
fn the_middle_button_pastes_the_clipboard_at_the_pointer_in_the_code() {
    let mut ed = editor();
    let (x, y) = code_cell(&ed, 3);
    let start = crate::movement::line_start(&ed.doc().text, 3);
    let mut pasted = false;
    for _ in 0..5 {
        crate::clipboard::stand_in("PASTED");
        let version = ed.doc().version();
        mouse(&mut ed, MouseKind::Middle, x, y);
        assert_eq!(ed.mode, crate::editor::Mode::Normal, "no mode of its own");
        while ed.doc().version() == version {
            let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the clipboard read");
            ed.handle_event(ev);
        }
        if ed.doc().text.line(3) == "liPASTEDne 3\n" {
            pasted = true;
            break;
        }
        feed(&mut ed, "u");
    }
    assert!(pasted, "pasted at the pointer: {:?}", ed.doc().text.line(3).to_string());
    assert!(ed.doc().selection().primary().from() >= start, "the cursor on that line");
}

/// The wheel sideways goes as far as the widest line's cells — tabs expanded, a wide glyph two — and pauses follow
/// over the pane it leads, as the wheel does.
#[test]
fn the_wheel_sideways_goes_by_cells_and_pauses_follow() {
    for line in ["\t".repeat(60), "가".repeat(100)] {
        let mut ed = editor();
        ed.docs[0].text = ropey::Rope::from_str(&format!("{line}\nx\n"));
        ed.docs[0].wrap = Some(false);
        frame(&mut ed);
        let (x, y) = code_cell(&ed, 1);
        mouse(&mut ed, MouseKind::ScrollRight, x, y);
        assert_eq!(ed.doc().left, 4, "{} cells wide: room to scroll", line.chars().count());
    }
    let mut ed = editor();
    ed.docs[0].text = ropey::Rope::from_str(&format!("{}\n", "x".repeat(400)));
    ed.docs[0].wrap = Some(false);
    crate::uistate::tests::chat(&mut ed, "");
    ed.workspace.viewer.at = crate::workspace::At::Code;
    ed.workspace.claude.chat.as_mut().unwrap().follow.on = true;
    frame(&mut ed);
    let (x, y) = code_cell(&ed, 0);
    mouse(&mut ed, MouseKind::ScrollRight, x, y);
    assert!(ed.workspace.claude.chat.as_ref().unwrap().follow.paused, "follow paused");
    assert_eq!(ed.doc().left, 4);
}

#[test]
fn the_wheel_sideways_scrolls_the_codes_columns() {
    let mut ed = editor();
    let long = "x".repeat(400);
    ed.docs[0].text = ropey::Rope::from_str(&format!("{long}\n{long}\n"));
    // (a buffer that wraps has nothing to scroll sideways)
    ed.docs[0].wrap = Some(true);
    frame(&mut ed);
    let (x, y) = code_cell(&ed, 1);
    mouse(&mut ed, MouseKind::ScrollRight, x, y);
    assert_eq!(ed.doc().left, 0, "wrapped: nothing sideways");
    ed.docs[0].wrap = Some(false);
    frame(&mut ed);
    let before = ed.doc().selection().primary();
    mouse(&mut ed, MouseKind::ScrollRight, x, y);
    assert_eq!(ed.doc().left, 4, "four columns on");
    assert_eq!(ed.doc().selection().primary(), before, "the cursor where it was");
    mouse(&mut ed, MouseKind::ScrollLeft, x, y);
    mouse(&mut ed, MouseKind::ScrollLeft, x, y);
    assert_eq!(ed.doc().left, 0, "back to the first column, no further");
}
