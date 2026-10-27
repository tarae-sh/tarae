//! A press while a key sequence is under way ends it, then acts where it lands (landing O, one rule with W's words) —
//! wherever the sequence is kept: the code's count and awaited character, a run panel's own keys, the chat's view mode.
//! Insert mode's held keys (the `j` of a `j k` binding) aren't a sequence but text: a press breaks them off, typed
//! where they were typed — as a key that breaks the sequence off types them — never dropped, never left to land where
//! the press put the cursor.

use super::{code_cell, editor, feed, frame, press};
use crate::editor::{Editor, Mode};
use crate::workspace::At;

#[test]
fn a_press_after_a_count_keeps_no_count() {
    let mut ed = editor();
    feed(&mut ed, "2");
    assert_eq!(ed.pending_display(), "2");
    let (x, y) = code_cell(&ed, 5);
    press(&mut ed, x, y);
    assert_eq!(ed.pending_display(), "", "the count ended");
    feed(&mut ed, "j");
    assert_eq!(ed.cursor_line(), 6, "`j` alone: one line, not two");
}

#[test]
fn a_press_while_a_character_is_awaited_ends_the_wait() {
    let mut ed = editor();
    feed(&mut ed, "f");
    assert_eq!(ed.pending_display(), "f…");
    let (x, y) = code_cell(&ed, 5);
    press(&mut ed, x, y);
    assert_eq!(ed.pending_display(), "", "no character awaited");
    let at = ed.doc().selection().primary().head;
    feed(&mut ed, "l");
    assert_eq!(ed.doc().selection().primary().head, at + 1, "`l` a move of its own, not `f`'s character");
}

/// (a press on the code would take the keys from it, which drops what's under way anyway — `Pane::dropped`: a press
/// on its own text keeps them there, and must end it)
#[test]
fn a_press_on_a_run_panels_text_ends_its_own_sequence() {
    let mut ed = editor();
    crate::uistate::tests::task_panel(&mut ed);
    ed.bottom_take_keys();
    feed(&mut ed, "g");
    assert_eq!(ed.pending_display(), "g", "the output's own `g` under way");
    let (_, y0, x0, _) = ed.screen.run_rows.of(crate::sizes::Slot::Task).expect("its rows drawn");
    press(&mut ed, x0 + 1, y0);
    assert!(!ed.workspace.is_at(At::Code), "the keys still the output's");
    assert_eq!(ed.pending_display(), "", "no `g` left waiting for its second key");
}

#[test]
fn a_press_on_the_conversation_ends_the_chats_view_mode_sequence() {
    let mut ed = editor();
    crate::uistate::tests::chat_view(&mut ed);
    feed(&mut ed, "g");
    assert_eq!(ed.pending_display(), "g");
    let y = (0..40).find(|&y| ed.screen.chat_row(y).is_some()).expect("a row of the conversation drawn");
    let x = ed.screen.chat_x.expect("the chat drawn") + crate::chrome::GAP + crate::chrome::PAD + 1;
    press(&mut ed, x, y);
    let keys = crate::chatdoc::transcript(&ed).map(|t| t.pane.keys.pending_display());
    assert_eq!(keys.as_deref(), Some(""), "the view mode's `g` ended, the view mode kept");
}

#[test]
fn a_press_on_the_code_ends_the_terminals_view_mode_sequence() {
    if crate::sandbox::no_pty_here(
        "mouse::tests::sequences::a_press_on_the_code_ends_the_terminals_view_mode_sequence",
    ) {
        return;
    }
    let mut ed = editor();
    crate::uistate::tests::terminal(&mut ed);
    feed(&mut ed, "<C-\\>g");
    let keys = |ed: &Editor| crate::view::terminal_view_of(ed).map(|v| v.keys.pending_display());
    assert_eq!(keys(&ed).as_deref(), Some("g"), "its view mode's `g` under way");
    // (a press on its own text: the view mode stays — the code's would leave it, `g` and all)
    let (y0, _) = ed.screen.terminal_rows.expect("the terminal drawn");
    let (x, y) = (ed.screen.slot_x + crate::chrome::PAD + 1, y0 + crate::chrome::SLOT_HEAD + 1);
    press(&mut ed, x, y);
    assert_eq!(keys(&ed).as_deref(), Some(""), "ended, the view mode kept");
}

/// Insert's `C-r` waiting for a register's name ends on a press, as every line's: the next key typed, never taken as a
/// register — the code's insert mode and the chat's input (where `C-r` is the chat's own, a code block applied: a
/// register's wait there is a binding of yours, `A-g` here).
#[test]
fn a_press_ends_inserts_register_wait_in_the_code_and_the_chats_input() {
    let (config, w) = crate::config::parse("[keys.insert]\nA-g = \"insert_register\"\n");
    assert!(w.is_empty(), "{w:?}");
    let mut ed = Editor::new(config);
    ed.docs[0].text = ropey::Rope::from_str(&(0..30).map(|i| format!("line {i}\n")).collect::<String>());
    frame(&mut ed);
    feed(&mut ed, "i<C-r>");
    assert!(ed.on_next_char.is_some(), "`C-r` waits");
    let (x, y) = code_cell(&ed, 3);
    press(&mut ed, x, y);
    assert!(ed.on_next_char.is_none(), "the wait ended");
    feed(&mut ed, "a");
    assert_eq!(ed.doc().text.line(3).to_string(), "liane 3\n", "`a` typed where the press put the cursor");
    // (the chat's input: a press on it)
    feed(&mut ed, "<esc>");
    crate::uistate::tests::chat(&mut ed, "he");
    ed.workspace.viewer.at = At::Chat;
    frame(&mut ed);
    feed(&mut ed, "<A-g>");
    let idle = |ed: &Editor| ed.workspace.claude.chat.as_ref().unwrap().input.pane.keys.idle();
    assert!(!idle(&ed), "its register's wait");
    let y = (0..40).find(|&y| ed.screen.chat_input_row(y).is_some()).expect("the input drawn");
    let x0 = ed.screen.chat_input_row(y).map(|r| r.1).unwrap();
    press(&mut ed, x0, y);
    assert!(idle(&ed), "the wait ended");
    feed(&mut ed, "a");
    let input = ed.workspace.claude.chat.as_ref().unwrap().input.pane.doc().text.to_string();
    assert_eq!(input, "ahe", "`a` typed where the press put the cursor");
}

fn with_j_k() -> Editor {
    let (config, w) = crate::config::parse("[keys.insert]\nj = { k = \"normal_mode\" }\n");
    assert!(w.is_empty(), "{w:?}");
    let mut ed = Editor::new(config);
    ed.docs[0].text = ropey::Rope::from_str(&(0..30).map(|i| format!("line {i}\n")).collect::<String>());
    frame(&mut ed);
    ed
}

/// The prompt's line holds a `j k` binding's `j` as the code does: a press on the line types it where it was typed,
/// then puts the cursor where the press landed.
#[test]
fn a_press_on_the_prompts_line_types_its_held_keys_where_they_were() {
    let mut ed = with_j_k();
    feed(&mut ed, ":abj");
    let line = |ed: &Editor| ed.prompt.as_ref().map(|p| p.text()).unwrap_or_default();
    assert_eq!((line(&ed).as_str(), ed.pending_display().as_str()), ("ab", "j"), "its `j` held");
    let d = ed.screen.prompt_input.get().expect("the line drawn");
    press(&mut ed, d.x, d.y);
    assert_eq!(line(&ed), "abj", "typed where it was typed, not where the press put the cursor");
    assert_eq!(ed.pending_display(), "");
    feed(&mut ed, "x");
    assert_eq!(line(&ed), "xabj", "the cursor then where the press put it");
}

/// A picker's query holds no typed key (its characters narrow at once — the user, 2026-10-27): what it can hold is
/// `C-r` waiting for a register's name, which a press ends — the next key typed, never taken as a register.
#[test]
fn a_press_on_a_pickers_query_ends_its_register_wait() {
    let mut ed = with_j_k();
    let id = ed.doc().id;
    let item = |label: &str| crate::picker::Item {
        label: label.into(),
        action: crate::picker::Action::Buffer(id),
        hint: String::new(),
        glyph: None,
    };
    ed.open_picker(
        crate::picker::Picker::of("buffers", "buffers", vec![item("one"), item("two")], false),
        None,
    );
    frame(&mut ed);
    feed(&mut ed, "onj");
    let query = |ed: &Editor| ed.picker.as_ref().map(|p| p.input.text()).unwrap_or_default();
    assert_eq!(query(&ed), "onj", "nothing held: typed at once");
    let waits = |ed: &Editor| ed.picker.as_ref().is_some_and(|p| !p.input.pane.keys.idle());
    feed(&mut ed, "<C-r>");
    assert!(waits(&ed), "`C-r` waits for a register's name");
    let d = ed.screen.picker_input.get().expect("the query drawn");
    press(&mut ed, d.x, d.y);
    assert!(!waits(&ed), "the wait ended");
    feed(&mut ed, "a");
    assert_eq!(query(&ed), "aonj", "`a` typed where the press put the cursor, not taken as a register");
}

/// Insert mode's held `j` (under `[keys.insert] j = { k = … }`): a press breaks it off — typed where it was typed, the
/// cursor then where the press put it; the chat's input's the same.
#[test]
fn a_press_types_insert_modes_held_keys_where_they_were_typed() {
    let (config, w) = crate::config::parse("[keys.insert]\nj = { k = \"normal_mode\" }\n");
    assert!(w.is_empty(), "{w:?}");
    let mut ed = Editor::new(config);
    (ed.config.llm.command, ed.config.llm.args) = ("sh".into(), vec!["-c".into(), "cat > /dev/null".into()]);
    ed.docs[0].text = ropey::Rope::from_str(&(0..30).map(|i| format!("line {i}\n")).collect::<String>());
    frame(&mut ed);
    feed(&mut ed, "ij");
    assert_eq!((ed.pending_display().as_str(), ed.doc().text.line(0).to_string()), ("j", "line 0\n".into()));
    let (x, y) = code_cell(&ed, 3);
    press(&mut ed, x, y);
    assert_eq!(ed.doc().text.line(0).to_string(), "jline 0\n", "the held `j` typed where it was typed");
    assert_eq!(ed.doc().text.line(3).to_string(), "line 3\n", "not where the press put the cursor");
    assert_eq!((ed.mode, ed.pending_display().as_str()), (Mode::Insert, ""));
    assert_eq!(ed.cursor_line(), 3, "the cursor then where the press put it");
    // (the chat's input: its held `j` typed in it before a press elsewhere)
    feed(&mut ed, "<esc>");
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.viewer.at = At::Chat;
    frame(&mut ed);
    feed(&mut ed, "hej");
    let input = |ed: &Editor| ed.workspace.claude.chat.as_ref().unwrap().input.pane.doc().text.to_string();
    assert_eq!(input(&ed), "he", "its `j` held");
    let (x, y) = code_cell(&ed, 5);
    press(&mut ed, x, y);
    assert_eq!(input(&ed), "hej", "typed in the input, not dropped");
    assert_eq!(ed.pending_display(), "");
}
