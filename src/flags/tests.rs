//! #388's flags at the code: drawn, lit, pressed, walked, hovered — and the column a turn's end shows (chatlink Q5).

use crate::chatnotes::tests::{answer, answers_to, feed, project, reads, screen};
use crate::editor::Editor;
use crate::event::{Mouse, MouseKind};

/// A project with two files and two answers: the first names `src/a.rs:3` and `src/b.rs:2`, the second `src/a.rs:7` and
/// then `src/a.rs:3` again (two answers on one line; written out of the file's order); `src/a.rs` open.
fn two_answers(name: &str) -> (crate::scratch::Scratch, std::path::PathBuf, Editor) {
    let (s, root, mut ed) = project(name, &["src/a.rs", "src/b.rs"]);
    answers_to(
        &mut ed,
        "what reads the count?",
        &["The count is read in src/a.rs:3. It is set in src/b.rs:2."],
        vec![],
    );
    answers_to(
        &mut ed,
        "and the guard?",
        &["The loop ends at src/a.rs:7. The guard is at src/a.rs:3 too."],
        vec![],
    );
    ed.open(&root.join("src/a.rs")).unwrap();
    (s, root, ed)
}

/// The flag at `line` (from 0) of the open file standing for its line: the newest answer's.
fn flag_at(ed: &Editor, line: usize) -> u64 {
    let path = ed.doc().path.clone().unwrap();
    crate::flags::on_line(&ed.workspace.claude.flags, &path, line)
        .first()
        .map(|f| f.id)
        .expect("a flag there")
}

/// Where the flag at `line` drew its `¶` in the last frame.
fn pilcrow(ed: &Editor, id: u64) -> (u16, u16) {
    let ends = ed.screen.flag_ends.borrow();
    let e = ends.iter().find(|e| e.2 == id).expect("its ¶ drawn");
    (e.1 as u16, e.0 as u16)
}

fn press(ed: &mut Editor, (x, y): (u16, u16)) {
    ed.handle_mouse(Mouse { kind: MouseKind::Down, x, y, alt: false, ctrl: false });
    ed.handle_mouse(Mouse { kind: MouseKind::Up, x, y, alt: false, ctrl: false });
}

/// A press on a flag's `¶` takes the hidden column to its line's newest answer's paragraph, the keys staying in the
/// code; no card opens. (Its button pressing a note's act instead, nothing reads.)
#[test]
fn a_press_on_a_flag_takes_the_column_to_its_paragraph() {
    let (_s, _root, mut ed) = two_answers("flags-press");
    screen(&mut ed);
    let id = flag_at(&ed, 2);
    assert_eq!(crate::flags::find(&ed, id).unwrap().origin.n, 2, "the newest answer's stands for the line");
    let at = pilcrow(&ed, id);
    press(&mut ed, at);
    assert!(reads(&ed, id), "the column at its paragraph");
    assert_eq!(ed.workspace.viewer.at, crate::workspace::At::Code, "the keys in the code");
}

/// `enter` on a flag's line does what its press does; `space L` there opens the column at that paragraph too.
#[test]
fn enter_and_space_l_on_a_flags_line_go_to_its_paragraph() {
    let (_s, _root, mut ed) = two_answers("flags-enter");
    // (`space L` opens the chat: a stand-in claude, never the real one)
    ed.config.llm.command = "sh -c 'cat > /dev/null'".into();
    screen(&mut ed);
    feed(&mut ed, "7G<ret>");
    let id = flag_at(&ed, 6);
    assert!(reads(&ed, id), "enter: the column at its paragraph");
    assert_eq!(ed.workspace.viewer.at, crate::workspace::At::Code);
    // (hidden again, then `space L` on the line: shown at the same paragraph, the keys in it as `space L` gives them)
    crate::chat::hide(&mut ed);
    ed.workspace.viewer.at = crate::workspace::At::Code;
    ed.workspace.claude.chat.as_mut().unwrap().reading = None;
    feed(&mut ed, " L");
    let at = ed.workspace.claude.chat.as_ref().unwrap().reading.as_ref().map(|r| r.0.clone());
    assert_eq!(at, crate::flags::find(&ed, id).map(|f| f.reading()));
}

/// `]n` walks the newest answer's flags first, in the order written, then the rest by file and line — a line's answers
/// one stop — and wraps; on a flag the cursor moves, no card opens, and with the column in sight its paragraph shows.
/// (From your line the first stop is the nearest after it, as for notes: `src/a.rs:3`, then on in the walk's order — by
/// file order alone it would go to `src/a.rs:7` next.)
#[test]
fn next_note_walks_the_newest_answers_flags_first() {
    let (_s, root, mut ed) = two_answers("flags-walk");
    let at = |ed: &Editor| (crate::chat::shown(&root, ed.doc().path.as_ref().unwrap()), ed.cursor_line() + 1);
    let mut been = Vec::new();
    for _ in 0..4 {
        feed(&mut ed, "]n");
        been.push(at(&ed));
        assert_eq!(ed.workspace.claude.notes.open, None, "no card");
    }
    let want = [("src/a.rs", 3), ("src/b.rs", 2), ("src/a.rs", 7), ("src/a.rs", 3)];
    assert_eq!(been, want.map(|(p, l)| (p.to_string(), l)));
    // With the column in sight: the paragraph of the flag stepped to, the keys in the code
    ed.workspace.claude.chat.as_mut().unwrap().hidden = false;
    screen(&mut ed);
    feed(&mut ed, "]n");
    let id = flag_at(&ed, 1);
    assert!(reads(&ed, id), "its paragraph shown: src/b.rs:2's");
    assert_eq!(ed.workspace.viewer.at, crate::workspace::At::Code);
}

/// The `¶` lit while its paragraph is in the column's view, faint once it scrolled out — in the frame that moved it
/// there (a press: lit by the view it drew, never the one before).
#[test]
fn a_flag_is_lit_while_its_paragraph_is_in_view() {
    let (_s, _root, mut ed) = project("flags-lit", &["src/a.rs"]);
    answer(&mut ed, "The count is read in src/a.rs:3.");
    // (a long answer after it: the first scrolls out of the column)
    let long: String = (0..60).map(|i| format!("Paragraph {i} says nothing of the code.\n\n")).collect();
    answers_to(&mut ed, "more?", &[&long], vec![]);
    ed.open(&_root.join("src/a.rs")).unwrap();
    let id = flag_at(&ed, 2);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.hidden = false;
    c.scroll = 0;
    screen(&mut ed);
    let lit = |ed: &Editor| {
        let f = crate::flags::find(ed, id).unwrap();
        crate::flags::lit(&ed.workspace.claude.flags, f)
    };
    assert!(!lit(&ed), "its paragraph far above the conversation's end");
    // (the column at its paragraph, as a press takes it: lit once that frame is drawn)
    crate::flags::to_paragraph(&mut ed, id);
    screen(&mut ed);
    assert!(lit(&ed), "its paragraph in view, in the frame that took it there");
    let s = ed.workspace.claude.flags.lit.clone();
    assert!(s.iter().any(|r| *r == crate::flags::find(&ed, id).unwrap().reading()), "{s:?}");
    // (the state says it)
    let state = ed.ui_state();
    assert!(state.flags.iter().any(|f| f.id == "flag:src/a.rs:3" && f.lit), "{:?}", state.flags);
    // Scrolled back to the end: faint
    ed.workspace.claude.chat.as_mut().unwrap().scroll = 0;
    screen(&mut ed);
    assert!(!lit(&ed), "scrolled out");
}

/// The mouse on a `¶` shows a card of every answer that named its line, newest first, a row each — the question and the
/// sentence that named it; the card stays as the pointer goes onto it, and a press on a row takes the column to that
/// answer's paragraph (the older one's here). Off both, it goes.
#[test]
fn a_flags_hover_lists_its_lines_answers_each_a_button() {
    let (_s, _root, mut ed) = two_answers("flags-hover");
    screen(&mut ed);
    let newest = flag_at(&ed, 2);
    let (x, y) = pilcrow(&ed, newest);
    ed.handle_event(crate::event::Event::Hover(x, y));
    let rows = screen(&mut ed);
    let (cx, cy, _, h) = ed.screen.flag_card.get().expect("its card drawn");
    assert_eq!(h, 2, "two answers named the line");
    let card: Vec<&String> = rows[cy..cy + h].iter().collect();
    assert!(card[0].contains("and the guard?") && card[0].contains("The guard is at"), "{card:?}");
    assert!(card[1].contains("what reads the count?") && card[1].contains("The count is read"), "{card:?}");
    // (onto its second row: it stays; pressed, the older answer's paragraph)
    ed.handle_event(crate::event::Event::Hover(cx as u16 + 2, cy as u16 + 1));
    screen(&mut ed);
    assert!(ed.flag_hover.is_some(), "the card stays under the pointer");
    press(&mut ed, (cx as u16 + 2, cy as u16 + 1));
    let older = crate::flags::on_line(&ed.workspace.claude.flags, ed.doc().path.as_ref().unwrap(), 2)[1].id;
    assert!(reads(&ed, older), "the older answer's paragraph");
    // Off both: gone
    screen(&mut ed);
    ed.handle_event(crate::event::Event::Hover(0, 0));
    screen(&mut ed);
    assert!(ed.flag_hover.is_none() && ed.screen.flag_card.get().is_none());
}

/// A flag on a folded doc comment's line stands at the end of the row its line's paragraph is on, as a note's does (focus
/// A1c, #261) — the block's other rows none — and is pressed there as at a code line's end.
#[test]
fn a_flag_on_a_folded_doc_comment_is_at_its_paragraphs_row() {
    let (_s, root, mut ed) = project("flags-doc", &["src/c.rs"]);
    let c = root.join("src/c.rs");
    std::fs::write(&c, "fn a() {}\n/// One line.\n///\n/// Two, a\n/// paragraph.\nfn b() {}\n").unwrap();
    answer(&mut ed, "The second paragraph at src/c.rs:5 says why.");
    ed.open(&c).unwrap();
    let rows = screen(&mut ed);
    if !rows.iter().any(|r| r.contains("Two, a paragraph.")) {
        // (no Rust grammar in this build: no fold to draw on)
        return;
    }
    let code = |want: &str| {
        let r = rows.iter().find(|r| r.contains(want)).unwrap();
        r.split('│').next().unwrap().trim_end().to_string()
    };
    assert!(code("Two, a paragraph.").ends_with("Two, a paragraph.   ¶"), "{rows:#?}");
    assert!(code("One line.").ends_with("One line."), "{rows:#?}");
    let id = flag_at(&ed, 4);
    ed.workspace.claude.chat.as_mut().unwrap().hidden = true;
    screen(&mut ed);
    let at = pilcrow(&ed, id);
    press(&mut ed, at);
    assert!(reads(&ed, id), "the column at its paragraph");
}

/// Chatlink Q5: an answer with words after its last call shows the hidden column at its turn's end — not one you hid
/// during that turn (`◂ new` says it). (Hidden by you before the question went, it shows.)
#[test]
fn a_turns_end_shows_the_column_unless_you_hid_it_during_the_turn() {
    let (_s, _root, mut ed) = project("flags-shows", &["src/a.rs"]);
    crate::chat::hide(&mut ed);
    answer(&mut ed, "The count is read in src/a.rs:3.");
    assert!(!ed.workspace.claude.chat.as_ref().unwrap().hidden, "shown at the turn's end");
    // Hidden during the next turn: it stays hidden
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, "and?", ""));
    crate::chat::hide(&mut ed);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.follow.new_turn();
    c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Assistant, "Nothing else reads it.", ""));
    crate::chatnotes::turn_over(&mut ed, true);
    assert!(ed.workspace.claude.chat.as_ref().unwrap().hidden, "you hid it during the turn");
}
