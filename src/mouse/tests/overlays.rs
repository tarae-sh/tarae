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

/// The chat's slash list (#232): a press on a row fills its name in, as `tab` going round to it — the list stays, the
/// input keeps the keys, nothing sent; a press on its air does nothing under it.
#[test]
fn a_press_on_a_slash_row_fills_its_name_in() {
    let mut ed = editor();
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    feed(&mut ed, "/c");
    frame(&mut ed);
    let (x, y, ..) = ed.screen.slash_card.get().expect("the list drawn");
    let row = ed.screen.slash_rows.borrow().iter().find(|r| r.1 == 1).map(|r| r.0).expect("row 1");
    press(&mut ed, x + 4, row);
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert_eq!(c.input.text(), "/compact", "filled in");
    assert!(c.msgs.is_empty() && !c.busy(), "nothing sent");
    assert!(ed.workspace.is_at(crate::workspace::At::Chat), "the input keeps the keys");
    assert!(ed.screen.slash_card.get().is_some(), "the list stays");
    // (the round goes on from it: the next `tab` the name after it)
    feed(&mut ed, "<tab>");
    assert_eq!(ed.workspace.claude.chat.as_ref().unwrap().input.text(), "/cost");
    // (its air: nothing under it pressed)
    press(&mut ed, x + 4, y);
    assert_eq!(ed.workspace.claude.chat.as_ref().unwrap().input.text(), "/cost");
}

/// A picker opened over the slash list: the list stays up under it, and a press where the picker covers one of its rows
/// is the picker's, never the hidden row's — the chat's input as it was (#232's review, round 2). A picker floats left of
/// the chat, so it reaches the list only where the code's area is narrower than a picker's least width: the chat zoomed
/// (`C-w z`), the picker one that keeps the screen's height (a preview's).
#[test]
fn a_picker_over_the_slash_list_takes_the_press_on_it() {
    let mut ed = editor();
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    feed(&mut ed, "<C-w>z/m");
    let items: Vec<crate::picker::Item> = (0..60)
        .map(|i| crate::picker::Item {
            action: crate::picker::Action::Open(std::path::PathBuf::from(format!("/nowhere/f{i}.rs"))),
            label: format!("f{i}.rs"),
            hint: String::new(),
            glyph: None,
        })
        .collect();
    ed.picker = Some(crate::picker::Picker::new("files", items, true));
    frame(&mut ed);
    let card = ed.screen.slash_card.get().expect("the list up under the picker");
    let b = ed.screen.picker_box.get().expect("the picker drawn");
    let rows = ed.screen.slash_rows.borrow().clone();
    let inside = |(rx, ry, rw, rh): (usize, usize, usize, usize), x: usize, y: usize| {
        (rx..rx + rw).contains(&x) && (ry..ry + rh).contains(&y)
    };
    let (x, y) = (rows.iter())
        .flat_map(|r| (card.0 + 2..card.0 + card.2).map(move |x| (x, r.0)))
        .find(|&(x, y)| inside(b, x, y))
        .unwrap_or_else(|| panic!("no row under the picker: {b:?} {card:?} {rows:?}\n{}", frame(&mut ed)));
    assert_eq!(ed.mouse_at(x, y).surface, crate::mouse::Surface::Picker, "the picker's, over the list");
    press(&mut ed, x, y);
    assert_eq!(ed.workspace.claude.chat.as_ref().unwrap().input.text(), "/m", "no name filled in under it");
    assert!(ed.picker.is_some(), "the picker stays: a press inside it");
}

/// The list's keys are its frame's bottom line (#234), typed as they're pressed — in insert mode, where the list is: `tab`
/// selects the next, `enter` accepts it.
#[test]
fn a_press_on_the_completion_lists_keys_does_them_while_typing() {
    let mut ed = editor();
    completing(&mut ed);
    let spot = |ed: &Editor, key: &str| {
        let spots = ed.screen.word_spots.borrow();
        let press = crate::wordspot::Press::Keys(key.into());
        let s = spots.iter().find(|s| s.on == crate::mouse::Surface::Completion && s.press == press);
        s.map(|s| (s.from, s.y)).unwrap_or_else(|| panic!("{key} on the list's bottom line: {spots:?}"))
    };
    let (x, y, _, h) = ed.screen.completion_card.get().expect("the list drawn");
    let (tx, ty) = spot(&ed, "tab");
    assert_eq!(ty, y + h - 1, "on its bottom line");
    assert!(tx > x);
    press(&mut ed, tx, ty);
    assert_eq!(ed.completion.as_ref().and_then(|c| c.selected), Some(1), "`tab` selected the next");
    let label = ed.completion.as_ref().map(|c| c.item(1).label.clone()).unwrap_or_default();
    let (ex, ey) = spot(&ed, "enter");
    press(&mut ed, ex, ey);
    assert!(ed.completion.is_none(), "accepted, closed");
    assert!(ed.doc().text.line(0).to_string().ends_with(&format!("{label}\n")), "{}", ed.doc().text.line(0));
    assert_eq!(ed.mode, crate::editor::Mode::Insert, "still typing");
}

/// The language server's popups (#234) — `K`'s docs, the signature — never have the keys, and light their frame's lines
/// half while the pointer is on them, as every floating frame (#189): washed again once it's off.
#[test]
fn the_language_servers_popups_light_half_under_the_pointer() {
    use crate::event::Event;
    use crate::mouse::{Lights, Lit};
    let lit = |ed: &Editor| {
        let frames = ed.screen.frames.borrow();
        frames.iter().find(|f| f.of == Some(Lights::Popup)).map(|f| (f.rect, f.lit)).expect("a popup's frame")
    };
    let hover = |ed: &mut Editor, x: usize, y: usize| {
        ed.handle_event(Event::Hover(x as u16, y as u16));
        frame(ed);
    };
    for which in ["K's docs", "the signature"] {
        let mut ed = editor();
        feed(&mut ed, "jj");
        if which == "K's docs" {
            ed.popup = Some(crate::markdown::render("Some hover text.", &ed.theme, None));
            ed.popup_name = "word".into();
        } else {
            ed.signature = Some(crate::signature::Signature {
                doc: ed.doc().id,
                label: "fn get(k: &str) -> usize".into(),
                active: None,
                index: 0,
                count: 1,
                docs: String::new(),
            });
        }
        frame(&mut ed);
        let ((x, y, w, h), at_first) = lit(&ed);
        assert_eq!(at_first, Lit::Wash, "{which}: the faint wash, never the keys'");
        hover(&mut ed, x + w / 2, y + h / 2);
        assert_eq!(lit(&ed).1, Lit::Half, "{which}: half lit with the pointer on it");
        hover(&mut ed, x + w / 2, y + h + 2);
        assert_eq!(lit(&ed).1, Lit::Wash, "{which}: washed again off it");
    }
}

#[test]
fn a_press_on_the_note_card_replies_and_nothing_under_it_is_pressed() {
    let mut ed = editor();
    let path = std::env::temp_dir().join("tarae-mouse-overlays-note.rs");
    ed.docs[0].path.set(Some(path.clone()));
    let id = crate::notes::add(&mut ed, &path, 0..1, crate::notes::By::Author, "A word about this line.");
    crate::notes::open(&mut ed, id);
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

/// A card says `:note-close` and `space n` only where they act on it, the frame's buttons being those keys: on another
/// note's line they'd close or answer that one, a thread included, and the card would stay. On a line with none they're
/// the card's.
#[test]
fn a_cards_buttons_are_said_only_where_they_act_on_it() {
    let scratch = crate::scratch::Scratch::new("mouse-overlays-note-buttons");
    let mut ed = editor();
    let path = scratch.join("a.rs");
    ed.docs[0].path.set(Some(path.clone()));
    crate::notes::add(&mut ed, &path, 0..1, crate::notes::By::Author, "At your cursor.");
    let card = crate::notes::add(&mut ed, &path, 5..6, crate::notes::By::Author, "On the open card.");
    frame(&mut ed);
    crate::notes::open(&mut ed, card);
    let shown = frame(&mut ed);
    assert!(!shown.contains(":note-close") && !shown.contains("space n"), "{shown}");
    ed.goto_line(2);
    let shown = frame(&mut ed);
    assert!(shown.contains(":note-close") && shown.contains("space n"), "{shown}");
    crate::typed::execute(&mut ed, "note-close").unwrap();
    assert!(ed.workspace.claude.notes.list.iter().all(|n| n.id != card), "the card's, as it said");
}

/// While follow leads to a card's line, `space n` and `:note-close` go by your cursor (#250: a key of yours acts there):
/// the card says neither — with a note on your line they'd act on that one, with none `space n` starts one there and
/// `:note-close` is refused. Follow paused, there's no one's place: the card's own again.
#[test]
fn a_cards_buttons_go_by_your_cursor_while_follow_leads() {
    let scratch = crate::scratch::Scratch::new("mouse-overlays-note-follow");
    let root = std::fs::canonicalize(&*scratch).unwrap();
    let path = root.join("a.rs");
    std::fs::write(&path, (0..40).map(|i| format!("line {i}\n")).collect::<String>()).unwrap();
    let mut ed = editor();
    ed.workspace.root = root;
    ed.open(&path).unwrap();
    let mut chat = crate::chat::Chat::new();
    chat.follow.on = true;
    ed.workspace.claude.chat = Some(chat);
    frame(&mut ed);
    let yours = crate::notes::add(&mut ed, &path, 0..1, crate::notes::By::Author, "At your cursor.");
    let card = crate::notes::add(&mut ed, &path, 20..21, crate::notes::By::Author, "On the turn's card.");
    crate::notes::open_by_turn(&mut ed, card);
    let shown = frame(&mut ed);
    assert_eq!(crate::follow::view_line(&ed), Some(20), "follow leads to the card's line");
    assert_eq!(ed.cursor_line(), 0, "your cursor on the other note");
    assert!(!shown.contains("space n") && !shown.contains(":note-close"), "{shown}");
    // (none on your line: the card is at Claude's place, not yours — no button)
    crate::notes::close(&mut ed, yours);
    let shown = frame(&mut ed);
    assert!(!shown.contains("space n") && !shown.contains(":note-close"), "{shown}");
    // (paused: the open card's)
    ed.workspace.claude.chat.as_mut().unwrap().follow.paused = true;
    let shown = frame(&mut ed);
    assert!(
        shown.contains(":note-close") && shown.contains("space n"),
        "its own again, and it fits this wide: {shown}"
    );
}

/// The note card's row of `… N earlier`, pressed: only a line's folded answers unfold — a thread's own fold row reads alike
/// and is the card's words, a press there replying as anywhere on them.
#[test]
fn a_press_on_the_earlier_row_unfolds_a_lines_answers_only() {
    let scratch = crate::scratch::Scratch::new("mouse-overlays-note-earlier");
    let mut ed = editor();
    let path = scratch.join("a.rs");
    ed.docs[0].path.set(Some(path.clone()));
    let said = |ed: &mut Editor, text: &str, n: u64| {
        let words = (text.to_string(), text.to_string());
        let id = crate::notes::add_said(ed, &path, 3..4, words, Vec::new(), Some("where?".into()));
        let name: std::sync::Arc<str> = "a.rs".into();
        let origin = crate::chatnotes::Origin { n, at: 1, turn: 0, name, said: 3, text: None };
        crate::notes::find_mut(ed, id).unwrap().answer = Some(origin);
        id
    };
    said(&mut ed, "The first answer.", 1);
    let newest = said(&mut ed, "The second answer.", 2);
    let row = |ed: &Editor, words: &str| {
        let rows = ed.screen.note_text.borrow();
        let on = |r: &&(usize, usize, Vec<crate::markdown::Span>)| r.2.iter().any(|s| s.text.contains(words));
        rows.iter().find(on).map(|r| (r.1, r.0)).expect("its row recorded")
    };
    frame(&mut ed);
    crate::notes::open(&mut ed, newest);
    frame(&mut ed);
    let (x, y) = row(&ed, "… 1 earlier");
    press(&mut ed, x + 3, y);
    assert!(ed.workspace.claude.notes.earlier && ed.prompt.is_none(), "unfolded, no reply");
    assert!(frame(&mut ed).contains("The first answer."), "the earlier answer in sight");
    // A thread's fold row: its words, a reply
    let thread = crate::notes::add(&mut ed, &path, 10..11, crate::notes::By::Author, "It begins.");
    for k in 0..4 {
        let text = format!("Then {k}.");
        let e = crate::notes::Entry { by: crate::notes::By::You, text, draft: false };
        crate::notes::find_mut(&mut ed, thread).unwrap().thread.push(e);
    }
    crate::notes::open(&mut ed, thread);
    frame(&mut ed);
    let (x, y) = row(&ed, "… 2 earlier");
    ed.list_click = None;
    press(&mut ed, x + 3, y);
    assert!(!ed.workspace.claude.notes.earlier, "nothing to unfold");
    let kind = ed.prompt.as_ref().map(|p| p.kind);
    assert!(matches!(kind, Some(crate::editor::PromptKind::Note(Some(i))) if i == thread), "{kind:?}");
}

/// A press on an open card's words replies to that card, wherever your cursor is — on another note's line too: a turn's
/// card leaves your cursor where it was, and what the mouse is on is what it acts on.
#[test]
fn a_press_on_a_cards_words_replies_to_that_card() {
    let scratch = crate::scratch::Scratch::new("mouse-overlays-note-card");
    let mut ed = editor();
    let path = scratch.join("a.rs");
    ed.docs[0].path.set(Some(path.clone()));
    let yours = crate::notes::add(&mut ed, &path, 0..1, crate::notes::By::Author, "At your cursor.");
    let card = crate::notes::add(&mut ed, &path, 5..6, crate::notes::By::Author, "On the open card.");
    frame(&mut ed);
    crate::notes::open(&mut ed, card);
    frame(&mut ed);
    assert_eq!(crate::notes::here(&ed).map(|n| n.id), Some(yours), "your cursor on the other note");
    let (y, x) = {
        let rows = ed.screen.note_text.borrow();
        let on = |r: &&(usize, usize, Vec<crate::markdown::Span>)| {
            r.2.iter().any(|s| s.text.contains("On the open card"))
        };
        let row = rows.iter().find(on).expect("its words recorded");
        (row.0, row.1)
    };
    press(&mut ed, x + 3, y);
    let kind = ed.prompt.as_ref().map(|p| p.kind);
    assert!(matches!(kind, Some(crate::editor::PromptKind::Note(Some(i))) if i == card), "{kind:?}");
}

/// Focus A1b: a note's `¶` at its line's end is a button — pressed, its card opens between the lines; pressed again, it
/// closes. A drag over the card's words selects them, on the selection's ground, copied as you let go — no reply; a press
/// not dragged replies, as ever. The wheel over it scrolls the code it's in.
#[test]
fn a_notes_pilcrow_opens_its_card_and_a_drag_selects_its_words() {
    let scratch = crate::scratch::Scratch::new("mouse-overlays-note-pilcrow");
    let mut ed = editor();
    let path = scratch.join("a.rs");
    ed.docs[0].path.set(Some(path.clone()));
    let id = crate::notes::add(&mut ed, &path, 0..1, crate::notes::By::Author, "First words here, and more.");
    frame(&mut ed);
    let pilcrow = |ed: &Editor| {
        let spots = ed.screen.word_spots.borrow();
        spots.iter().find(|s| s.press == crate::wordspot::Press::Note(id)).map(|s| (s.from, s.y))
    };
    let (px, py) = pilcrow(&ed).expect("its `¶` a button");
    press(&mut ed, px, py);
    assert_eq!(ed.workspace.claude.notes.open, Some(id), "pressed: its card open");
    frame(&mut ed);
    assert!(ed.screen.note_card.get().is_some(), "drawn");
    let (px, py) = pilcrow(&ed).expect("its `¶` still a button");
    // (later — not a double's second press)
    ed.list_click = None;
    press(&mut ed, px, py);
    assert_eq!(ed.workspace.claude.notes.open, None, "pressed again: closed");
    // A drag over its words: selected, copied, no reply
    crate::notes::open(&mut ed, id);
    frame(&mut ed);
    // (the wheel over it: the code it's in scrolls)
    let (cx, cy, ..) = ed.screen.note_card.get().expect("drawn");
    let before = (ed.doc().top, ed.doc().top_skip());
    mouse(&mut ed, MouseKind::ScrollDown, cx + 2, cy + 1);
    assert_ne!((ed.doc().top, ed.doc().top_skip()), before, "the wheel scrolled the code");
    ed.doc_mut().set_top(0);
    frame(&mut ed);
    let (y, x, col) = {
        let rows = ed.screen.note_text.borrow();
        let words = |r: &&(usize, usize, Vec<crate::markdown::Span>)| -> String {
            r.2.iter().map(|s| s.text.as_str()).collect()
        };
        let row = rows.iter().find(|r| words(r).contains("First words")).expect("its words recorded");
        (row.0, row.1, words(&row).find("First").unwrap())
    };
    mouse(&mut ed, MouseKind::Down, x + col, y);
    mouse(&mut ed, MouseKind::Drag, x + col + 10, y);
    assert_eq!(crate::term::note_marked_text(&ed).as_deref(), Some("First words"), "what the drag holds");
    let marked = frame(&mut ed);
    mouse(&mut ed, MouseKind::Up, x + col + 10, y);
    assert!(ed.prompt.is_none(), "a drag: no reply");
    assert!(marked.contains("First words"), "still drawn under its ground");
    assert_eq!(ed.note_marked, None, "the gesture over: let go");
    // (a later drag elsewhere — the code under the card — is never the card's: it neither stretches nor copies it)
    frame(&mut ed);
    let (_, cy, _, ch) = ed.screen.note_card.get().expect("drawn");
    mouse(&mut ed, MouseKind::Down, 2, cy + ch + 1);
    mouse(&mut ed, MouseKind::Drag, 8, cy + ch + 1);
    assert_eq!(ed.note_marked, None, "a drag on the code isn't the card's");
    mouse(&mut ed, MouseKind::Up, 8, cy + ch + 1);
    assert!(ed.prompt.is_none(), "nor its reply");
    // A press not dragged: the reply
    mouse(&mut ed, MouseKind::Down, x + col, y);
    mouse(&mut ed, MouseKind::Up, x + col, y);
    assert!(
        matches!(ed.prompt.as_ref().map(|p| p.kind), Some(crate::editor::PromptKind::Note(_))),
        "a press: its reply's line"
    );
}

/// The note card's rows as drawn, from its top line to its bottom line.
fn note_card_rows(ed: &Editor) -> Vec<String> {
    let (x, y, w, h) = ed.screen.note_card.get().expect("the note card drawn");
    let (bytes, cols, rows) = ed.last_frame.clone().expect("a frame");
    let mut p = vt100::Parser::new(rows, cols, 0);
    p.process(&bytes);
    p.screen().rows(x as u16, w as u16).skip(y).take(h).collect()
}

/// The keys a press does on the note card, each with its cell.
fn note_card_keys(ed: &Editor) -> Vec<(String, (usize, usize))> {
    let spots = ed.screen.word_spots.borrow();
    (spots.iter().filter(|s| s.on == crate::mouse::Surface::NoteCard))
        .filter_map(|s| match &s.press {
            crate::wordspot::Press::Keys(k) => Some((k.clone(), (s.from, s.y))),
            _ => None,
        })
        .collect()
}

/// #240 (the user picked B): the note card is a docs card — a thin frame named `note`, which of how many notes start on
/// its line after the name, every entry under its author's head, its keys its bottom line and none in its body; each key
/// a button (`]n` pressed goes to the next note, `:note-close` closes it), and its frame half lit while the pointer is on
/// it, the wash again off it — never the keys': it never has them.
#[test]
fn the_note_card_is_framed_and_its_keys_are_buttons_on_its_bottom_line() {
    use crate::event::Event;
    use crate::mouse::{Lights, Lit};
    let scratch = crate::scratch::Scratch::new("mouse-overlays-note-keys");
    let mut ed = editor();
    let path = scratch.join("a.rs");
    ed.docs[0].path.set(Some(path.clone()));
    let long = "A word about this line, long enough for the card to carry every one of its keys.";
    let first = crate::notes::add(&mut ed, &path, 0..1, crate::notes::By::Author, long);
    crate::notes::add(
        &mut ed,
        &path,
        5..6,
        crate::notes::By::Author,
        &format!("Another, further down. {long}"),
    );
    crate::notes::open(&mut ed, first);
    frame(&mut ed);
    let rows = note_card_rows(&ed);
    assert!(rows[0].starts_with("╭─ note ─") && !rows[0].contains("of 2"), "{rows:#?}");
    assert!(rows[1].starts_with("│ ¶ author"), "its author's head: {rows:#?}");
    let bottom = rows.last().unwrap();
    let foot = "╰─ space n reply · ]n next · q close · space N all · :note-close ─";
    assert!(bottom.starts_with(foot), "{rows:#?}");
    assert!(!rows[1..rows.len() - 1].iter().any(|r| r.contains("space n")), "no keys in its body: {rows:#?}");
    let keys: Vec<String> = note_card_keys(&ed).into_iter().map(|k| k.0).collect();
    assert_eq!(keys, ["space n", "]n", "q", "space N", ":note-close"], "each key a button");
    // Half lit under the pointer, the wash off it
    let lit = |ed: &Editor| {
        let frames = ed.screen.frames.borrow();
        frames.iter().find(|f| f.of == Some(Lights::Note)).map(|f| f.lit).expect("the note card's frame")
    };
    assert_eq!(lit(&ed), Lit::Wash);
    let (x, y, w, h) = ed.screen.note_card.get().unwrap();
    ed.handle_event(Event::Hover((x + w / 2) as u16, (y + h / 2) as u16));
    frame(&mut ed);
    assert_eq!(lit(&ed), Lit::Half, "half lit with the pointer on it");
    ed.handle_event(Event::Hover((x + w / 2) as u16, (y + h + 3) as u16));
    frame(&mut ed);
    assert_eq!(lit(&ed), Lit::Wash, "washed again off it");
    // `]n` pressed: the next note, its card there
    let at = |ed: &Editor, key: &str| note_card_keys(ed).into_iter().find(|k| k.0 == key).expect(key).1;
    let (kx, ky) = at(&ed, "]n");
    press(&mut ed, kx, ky);
    assert_eq!(ed.cursor_line(), 5, "`]n` went to the next note");
    assert!(ed.prompt.is_none(), "its key, not a reply");
    assert_ne!(ed.workspace.claude.notes.open, Some(first), "its card open in the first's stead");
    // `:note-close` pressed: that note closed
    let (kx, ky) = at(&ed, ":note-close");
    press(&mut ed, kx, ky);
    let left: Vec<usize> = ed.workspace.claude.notes.list.iter().map(|n| n.lines.start).collect();
    assert_eq!(left, [0], "the note on line 5 closed");
    // Two notes starting on one line: which this is, on its top line
    crate::notes::add(&mut ed, &path, 0..1, crate::notes::By::You, "A second one here.");
    ed.goto_line(0);
    crate::notes::open(&mut ed, first);
    frame(&mut ed);
    let rows = note_card_rows(&ed);
    assert!(rows[0].starts_with("╭─ note ── ") && rows[0].contains(" of 2 here ─"), "{rows:#?}");
}

/// A draft's key that sends the review (`space v s`: a Claude turn started) is plain faint words on the note card's
/// bottom line — never a hint nor a button (`wordspot::COSTS`); its others stay buttons.
#[test]
fn the_note_cards_costly_key_is_plain_words() {
    let scratch = crate::scratch::Scratch::new("mouse-overlays-note-draft");
    let mut ed = editor();
    let path = scratch.join("a.rs");
    ed.docs[0].path.set(Some(path.clone()));
    let long = "A draft of mine about this line, long enough to carry the card's keys on its bottom line.";
    let id = crate::notes::add(&mut ed, &path, 0..1, crate::notes::By::You, long);
    for n in &mut ed.workspace.claude.notes.list {
        (n.thread[0].draft, n.waiting) = (true, false);
    }
    crate::notes::open(&mut ed, id);
    frame(&mut ed);
    let rows = note_card_rows(&ed);
    assert!(rows.last().unwrap().contains("space v s send the review"), "{rows:#?}");
    let keys: Vec<String> = note_card_keys(&ed).into_iter().map(|k| k.0).collect();
    assert_eq!(keys, ["space n", "]n", "q"], "the send no button");
    assert!(!ed.screen.hints.borrow().iter().any(|h| h.key == "space v s"), "nor a hint");
}

/// While a reply is written the card shows no keys (the user, 2026-10-29, on #240's reply shots): the reply's prompt has
/// them, said once on the status line (`enter send  esc close`) — no button nor hint on the card's bottom line, a press
/// there only saying how the reply goes, nothing sent; its keys come back as the reply ends.
#[test]
fn a_note_card_shows_no_keys_while_its_reply_is_written() {
    let scratch = crate::scratch::Scratch::new("mouse-overlays-note-reply");
    let mut ed = editor();
    // (the reply the key sends starts a claude: a stand-in)
    ed.config.llm.command = "sh".into();
    ed.config.llm.args = vec!["-c".into(), "cat > /dev/null".into()];
    let path = scratch.join("a.rs");
    ed.docs[0].path.set(Some(path.clone()));
    let long = "A word about this line, long enough for the card to carry every one of its keys.";
    let id = crate::notes::add(&mut ed, &path, 0..1, crate::notes::By::Author, long);
    crate::notes::open(&mut ed, id);
    let thread = |ed: &Editor| ed.workspace.claude.notes.list[0].thread.len();
    let replying =
        |ed: &Editor| matches!(ed.prompt.as_ref().map(|p| p.kind), Some(crate::editor::PromptKind::Note(_)));
    let reply = |ed: &mut Editor| {
        crate::notes::start(ed);
        feed(ed, "thanks");
        assert!(replying(ed), "the reply's line open");
    };
    let keys = |ed: &Editor| note_card_keys(ed).into_iter().map(|k| k.0).collect::<Vec<_>>();
    let walk = ["space n", "]n", "q", "space N", ":note-close"];
    frame(&mut ed);
    assert_eq!(keys(&ed), walk, "its keys before the reply");
    reply(&mut ed);
    let rows = note_card_rows(&ed);
    let bottom = rows.last().unwrap();
    assert!(bottom.starts_with("╰───") && bottom.trim_end().ends_with('╯'), "no keys: {rows:#?}");
    assert!(bottom.chars().all(|c| "╰─╯ ".contains(c)), "no keys: {rows:#?}");
    assert!(keys(&ed).is_empty(), "no button on the card");
    let (x, y, w, h) = ed.screen.note_card.get().unwrap();
    let on_card = |k: &crate::kinds::Hint| k.y == y + h - 1 && (x..x + w).contains(&k.from);
    assert!(!ed.screen.hints.borrow().iter().any(on_card), "nor a hint");
    let shown = frame(&mut ed);
    let status = shown.lines().last().unwrap_or("");
    assert!(status.contains("enter send  esc close"), "the prompt's keys, once: {status}");
    // A press where the keys were: said, nothing sent, the reply's line still open
    press(&mut ed, x + 4, y + h - 1);
    assert!(replying(&ed), "the reply's line still open");
    assert_eq!(ed.prompt.as_ref().unwrap().input.text().to_string(), "thanks", "what's typed kept");
    assert_eq!(thread(&ed), 1, "nothing sent");
    assert_eq!(said(&ed), "replying — enter sends it · esc close");
    // `esc`: the reply's line closed, nothing sent, its keys back
    feed(&mut ed, "<esc>");
    assert!(ed.prompt.is_none(), "the reply's line closed");
    assert_eq!(thread(&ed), 1, "nothing sent");
    assert_eq!(keys(&ed), walk, "its keys back");
    // (and `enter` sends it, its keys back too)
    reply(&mut ed);
    feed(&mut ed, "<ret>");
    assert_eq!(thread(&ed), 2, "sent by the key");
    assert!(ed.prompt.is_none() && !keys(&ed).is_empty(), "its keys back: {:?}", keys(&ed));
}

/// Another prompt open (`:` typed, the card last drawn under it): a press where the note card was is outside that prompt
/// — it closes as `esc`, never replaced by the note's reply.
#[test]
fn a_press_where_the_note_card_was_closes_another_prompt_and_never_replies() {
    let scratch = crate::scratch::Scratch::new("mouse-overlays-note-prompt");
    let mut ed = editor();
    let path = scratch.join("a.rs");
    ed.docs[0].path.set(Some(path.clone()));
    let id = crate::notes::add(&mut ed, &path, 0..1, crate::notes::By::Author, "A word about this line.");
    crate::notes::open(&mut ed, id);
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

/// #192: the ask's waiting card (`space i`, a commit draft) is a surface of its own — nothing under it is pressed through
/// it (a divider between panes here: a drag from it sized them), the wheel there scrolls nothing. A press gives the code
/// the keys, where its `esc stop` is typed — from the chat here — and never stops the answer; a drag from it says why it
/// selects nothing; in insert mode the press says `esc` comes first.
#[test]
fn a_press_on_the_asks_waiting_card_is_its_own() {
    let mut ed = editor();
    feed(&mut ed, "<C-w>v<C-w>v");
    let mut rev = crate::llm::Review::new(1, ed.doc().id, "name it", Vec::new());
    rev.streamed = "fn total(a: u32) -> u32 {\n    a + 1\n".into();
    ed.workspace.claude.review = Some(rev);
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    frame(&mut ed);
    assert_eq!(ed.keys_to().region, Region::Chat, "the chat had them");
    let (cx, cy, cw, ch) = ed.screen.ask_card.get().expect("the ask's card drawn");
    for (x, y) in (cy..cy + ch).flat_map(|y| (cx..cx + cw).map(move |x| (x, y))) {
        assert_eq!(ed.mouse_at(x, y).surface, crate::mouse::Surface::Ask, "its cell {x},{y}");
    }
    // (the line before a pane that starts inside the card: a divider under it)
    let starts = ed.screen.panes.iter().map(|p| p.1.x).find(|&px| px > cx + 1 && px < cx + cw);
    let x = starts.expect("a divider under the card") - 1;
    let rects = |ed: &Editor| ed.screen.panes.iter().map(|p| p.1).collect::<Vec<_>>();
    let tops = |ed: &Editor| (ed.doc().top, ed.workspace.views.iter().map(|v| v.top).collect::<Vec<_>>());
    let (before, top) = (rects(&ed), tops(&ed));
    mouse(&mut ed, MouseKind::Down, x, cy + 1);
    mouse(&mut ed, MouseKind::Drag, x + 6, cy + 1);
    mouse(&mut ed, MouseKind::Up, x + 6, cy + 1);
    assert_eq!(rects(&ed), before, "the divider under it never dragged");
    assert_eq!(said(&ed), crate::mouse::ASK_DRAG);
    assert_eq!(ed.keys_to().region, Region::Code, "the code's keys");
    assert!(crate::llm::esc_stops(&ed), "its `esc stop` over them");
    assert!(ed.workspace.claude.review.as_ref().is_some_and(|r| !r.ready), "the answer still being written");
    mouse(&mut ed, MouseKind::ScrollDown, x + 2, cy + 1);
    assert_eq!(tops(&ed), top, "no pane under it scrolled");
    // (in insert mode: the keys the code's already, its `esc` first)
    feed(&mut ed, "i");
    press(&mut ed, x + 2, cy + 1);
    assert!(said(&ed).starts_with("insert mode: esc first"), "said: {}", said(&ed));
    assert_eq!(ed.mode, crate::editor::Mode::Insert, "the mode kept");
}

/// `space i`'s answer being written (`Review` not ready), the chat's, the code's keys.
fn asking(ed: &mut Editor) {
    let mut rev = crate::llm::Review::new(1, ed.doc().id, "name it", Vec::new());
    rev.streamed = "fn total(a: u32) -> u32 {\n    a + 1\n".into();
    ed.workspace.claude.review = Some(rev);
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.viewer.at = crate::workspace::At::Code;
    frame(ed);
}

fn ask_cell(ed: &Editor) -> (usize, usize) {
    let (x, y, w, h) = ed.screen.ask_card.get().expect("the ask's card drawn");
    (x + w / 2, y + h / 2)
}

fn writing(ed: &Editor) -> bool {
    ed.workspace.claude.review.as_ref().is_some_and(|r| !r.ready)
}

/// #192: while the answer is written the code beside its card is the code's — "editing isn't blocked": a press puts
/// the cursor, a drag selects, a double a word; the answer still written, nothing said of a review.
#[test]
fn a_press_beside_the_asks_card_is_the_codes() {
    let mut ed = editor();
    asking(&mut ed);
    let (x, y) = code_cell(&ed, 3);
    let card = ed.screen.ask_card.get().expect("the ask's card drawn");
    assert!(
        !(card.0..card.0 + card.2).contains(&x) || !(card.1..card.1 + card.3).contains(&y),
        "beside the card"
    );
    assert!(matches!(ed.mouse_at(x, y).surface, crate::mouse::Surface::Code(_)), "{:?}", ed.mouse_at(x, y));
    press(&mut ed, x, y);
    let line_start = |ed: &Editor, l| crate::movement::line_start(&ed.doc().text, l);
    assert_eq!(
        ed.doc().selection().primary().head,
        line_start(&ed, 3) + 2,
        "the cursor where it was pressed"
    );
    assert_ne!(said(&ed), crate::mouse::REVIEW_OFF);
    // (elsewhere: a press there again at once would be a double)
    let ((_, y9), (_, y11)) = (code_cell(&ed, 9), code_cell(&ed, 11));
    mouse(&mut ed, MouseKind::Down, x, y9);
    mouse(&mut ed, MouseKind::Drag, x, y11);
    mouse(&mut ed, MouseKind::Up, x, y11);
    let sel = ed.doc().selection().primary();
    assert_eq!(
        (sel.from(), sel.to()),
        (line_start(&ed, 9) + 2, line_start(&ed, 11) + 3),
        "selected by the drag"
    );
    let (x7, y7) = code_cell(&ed, 7);
    press(&mut ed, x7, y7);
    press(&mut ed, x7, y7);
    let sel = ed.doc().selection().primary();
    assert_eq!(
        (sel.from(), sel.to()),
        (line_start(&ed, 7), line_start(&ed, 7) + 4),
        "a double: its word, `line`"
    );
    assert!(writing(&ed), "the answer still being written");
    assert_eq!(ed.keys_to().region, Region::Code);
}

/// #192: what's drawn after the ask's card takes the pointer over it — a picker, the follow card, the minimized chat,
/// hover text under the wheel (each laid over the card's cell as the frame would record it there).
#[test]
fn what_is_drawn_over_the_asks_card_takes_the_pointer_there() {
    use crate::mouse::Surface as S;
    let fresh = || {
        let mut ed = editor();
        asking(&mut ed);
        let at = ask_cell(&ed);
        assert_eq!(ed.mouse_at(at.0, at.1).surface, S::Ask, "the card's own, with nothing over it");
        (ed, at)
    };
    // (a picker: everything outside it is its outside)
    let (mut ed, (x, y)) = fresh();
    feed(&mut ed, "<space>?");
    assert!(ed.picker.is_some(), "the palette open");
    assert_eq!(ed.mouse_at(x, y).surface, S::Picker, "a picker's");
    // (the follow card, the minimized chat: each over it)
    for mini in [false, true] {
        let (ed, (x, y)) = fresh();
        let over = Some((x - 2, y - 1, 6, 3));
        if mini {
            ed.screen.chat_mini.set(over)
        } else {
            ed.screen.thought_card.set(over)
        }
        assert_eq!(ed.mouse_at(x, y).surface, S::FollowCard, "the follow card's (minimized: {mini})");
        assert_eq!(ed.mouse_at(x + 6, y).surface, S::Ask, "the card's beside it");
    }
    // (hover text: the wheel's, a press still the card's)
    let (mut ed, (x, y)) = fresh();
    ed.popup = Some(crate::markdown::render("Some hover text.", &ed.theme, None));
    frame(&mut ed);
    ed.screen.popup.set(Some((x - 2, y - 1, 6, 3)));
    assert_eq!(ed.mouse_under(MouseKind::ScrollDown, x, y).surface, S::Popup, "the wheel: hover text's");
    assert_eq!(ed.mouse_at(x, y).surface, S::Ask, "a press: the card's");
}

/// #192: the press's words say what stands before its `esc stop` — the mode only when it's the reason; a sequence under
/// way ended by the press says nothing of a mode; an answer done since the card's frame, nothing of stopping.
#[test]
fn a_press_on_the_asks_card_says_the_mode_only_when_its_the_reason() {
    let mut ed = editor();
    asking(&mut ed);
    // (its left end, the chat closed: off which-key, at the screen's bottom right)
    ed.workspace.claude.chat = None;
    frame(&mut ed);
    let (cx, cy, _, ch) = ed.screen.ask_card.get().expect("the ask's card drawn");
    let (x, y) = (cx + 1, cy + ch / 2);
    for keys in ["g", "3", "f"] {
        feed(&mut ed, keys);
        assert!(ed.sequence_under_way(), "{keys} under way");
        assert_eq!(
            ed.mouse_at(x, y).surface,
            crate::mouse::Surface::Ask,
            "{keys}: {:?}",
            ed.screen.which_key.get()
        );
        ed.messages.clear();
        press(&mut ed, x, y);
        assert!(!ed.sequence_under_way(), "{keys} ended by the press");
        assert!(crate::llm::esc_stops(&ed), "its `esc stop` over the code's keys after {keys}");
        assert_eq!(said(&ed), "", "nothing said after {keys}");
    }
    feed(&mut ed, "v");
    press(&mut ed, x, y);
    assert!(said(&ed).starts_with("select mode: esc first"), "said: {}", said(&ed));
    feed(&mut ed, "<esc>");
    // (the answer done since the frame the card was drawn in: nothing left to stop, no word of it)
    feed(&mut ed, "i");
    ed.workspace.claude.review.as_mut().unwrap().ready = true;
    ed.messages.clear();
    ed.handle_mouse(Mouse { kind: MouseKind::Down, x: x as u16, y: y as u16, alt: false, ctrl: false });
    assert!(!said(&ed).contains("stops the answer"), "said: {}", said(&ed));
}

/// #192: the ask's card has no frame (as a toast) — none recorded over it, nothing lit under the pointer; were one
/// drawn there, it would light as every card's frame does (`lights_at`'s arm, so the two can't drift apart).
#[test]
fn the_asks_card_lights_only_a_frame_drawn_there() {
    let mut ed = editor();
    asking(&mut ed);
    let card = ed.screen.ask_card.get().expect("the ask's card drawn");
    let (x, y) = ask_cell(&ed);
    let meets = |r: (usize, usize, usize, usize)| {
        r.0 < card.0 + card.2 && card.0 < r.0 + r.2 && r.1 < card.1 + card.3 && card.1 < r.1 + r.3
    };
    assert!(!ed.screen.frames.borrow().iter().any(|f| meets(f.rect)), "no frame recorded over it");
    let u = ed.mouse_at(x, y);
    assert_eq!(u.surface, crate::mouse::Surface::Ask);
    assert_eq!(ed.lights_at(u, x, y), None, "nothing lit");
    let framed = crate::mouse::Framed {
        rect: card,
        of: Some(crate::mouse::Lights::Card),
        lit: crate::mouse::Lit::Wash,
    };
    ed.screen.frames.borrow_mut().push(framed);
    assert_eq!(ed.lights_at(u, x, y), Some(crate::mouse::Lights::Card), "a frame drawn there lights");
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
    // (the wheel with Shift, as terminals send it: sideways over the code, down to the right)
    let top = ed.doc().top;
    mouse(&mut ed, MouseKind::ShiftScrollDown, x, y);
    assert_eq!((ed.doc().left, ed.doc().top), (4, top), "Shift's wheel: sideways");
    mouse(&mut ed, MouseKind::ShiftScrollUp, x, y);
    assert_eq!(ed.doc().left, 0);
}
