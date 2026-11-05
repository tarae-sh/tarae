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

/// A `¶`'s card comes once the pointer rests on it (#391, the docs' hover's rest) — crossing it, none; a key lets the
/// wait go —; with it up, the next line's `¶` shows its own at once (the hand-off); off both, it goes at once.
#[test]
fn a_flags_card_waits_for_the_rest_then_hands_off_at_once() {
    use crate::event::Event;
    let (_s, _root, mut ed) = two_answers("flags-rest");
    ed.card_rest = true;
    screen(&mut ed);
    let (three, seven) = (flag_at(&ed, 2), flag_at(&ed, 6));
    let (a, b) = (pilcrow(&ed, three), pilcrow(&ed, seven));
    let up = |ed: &Editor| ed.flag_hover.map(|h| h.id);
    // (crossing it: its ground lights — a button's, at once — its card not yet)
    ed.handle_event(Event::Hover(a.0, a.1));
    assert!(up(&ed).is_none() && ed.flag_wait.is_some(), "its card waits for the rest");
    screen(&mut ed);
    assert!(ed.screen.flag_card.get().is_none(), "no card yet");
    ed.handle_event(Event::Key("esc".parse().unwrap()));
    assert!(ed.flag_wait.is_none(), "a key lets the wait go");
    crate::hoverrest::rested(&mut ed);
    assert!(up(&ed).is_none(), "nothing came after the key");
    // (the pointer moved again: rested, its card)
    ed.handle_event(Event::Hover(0, 0));
    ed.handle_event(Event::Hover(a.0, a.1));
    crate::hoverrest::rested(&mut ed);
    assert_eq!(up(&ed), Some(three), "rested: its card");
    screen(&mut ed);
    assert!(ed.screen.flag_card.get().is_some(), "drawn");
    // Onto the next line's `¶`: its card at once, nothing waiting
    ed.handle_event(Event::Hover(b.0, b.1));
    assert!(!ed.unchanged, "drawn");
    assert_eq!((up(&ed), ed.flag_wait.is_none()), (Some(seven), true), "handed off at once");
    screen(&mut ed);
    assert!(ed.screen.flag_card.get().is_some());
    // Off both: gone at once, and the next waits again
    ed.handle_event(Event::Hover(0, 0));
    assert!(up(&ed).is_none() && ed.flag_wait.is_none(), "gone at once");
    screen(&mut ed);
    ed.handle_event(Event::Hover(a.0, a.1));
    assert!(up(&ed).is_none(), "cold again: it waits");
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

/// The rows the last frame laid out for message `i`, their words joined as the paragraph reads (a wrap's break a space).
fn laid_words(ed: &Editor, i: usize) -> Option<String> {
    let laid = ed.screen.chat_laid.borrow();
    let rows = laid.iter().find(|(o, _)| *o == crate::chatdoc::Of::Msg(i))?.1.clone();
    assert!(rows.iter().all(|r| r.rest.is_none()), "no row stands for words it doesn't draw");
    let text: Vec<String> = rows.iter().map(|r| r.spans.iter().map(|s| s.text.as_str()).collect()).collect();
    Some(text.join(" ").split_whitespace().collect::<Vec<_>>().join(" "))
}

/// #388 step 2, the chat whole: once a newer answer's turn ended, the older answer — every place of it flagged at the code
/// — is drawn whole, every word on its rows (focus A1b step 3 folded it to `◂ a.rs:3  The count…` stubs).
#[test]
fn an_older_answers_flagged_paragraphs_are_drawn_whole() {
    let (_s, _root, mut ed) = two_answers("flags-whole");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    (c.hidden, c.scroll) = (false, 200);
    let rows = screen(&mut ed);
    assert!(!rows.iter().any(|r| r.contains('◂')), "no stub: {rows:#?}");
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let first = c.msgs.iter().position(|m| m.role == crate::chat::Role::Assistant).unwrap();
    let flagged = ed.workspace.claude.flags.list.iter().filter(|f| f.origin.n == 1).count();
    assert_eq!(flagged, 2, "both its places flagged");
    assert_eq!(
        laid_words(&ed, first).as_deref(),
        Some("The count is read in src/a.rs:3. It is set in src/b.rs:2."),
        "the older answer whole"
    );
}

/// #388 step 2: a line two answers named has one `¶`, lit while either answer's paragraph about it is in view — the
/// older one's too, now that it reads whole (the newest's alone lit it before); the state's `lit` says the same.
#[test]
fn a_lines_flag_is_lit_by_any_of_its_answers_paragraphs() {
    let (_s, root, mut ed) = project("flags-line-lit", &["src/a.rs"]);
    answer(&mut ed, "The count is read in src/a.rs:3.");
    // (a long answer between: the two paragraphs about line 3 never in view at once)
    let long: String = (0..60).map(|i| format!("Paragraph {i} says nothing of the code.\n\n")).collect();
    answers_to(&mut ed, "more?", &[&long], vec![]);
    answers_to(&mut ed, "and the guard?", &["The guard is at src/a.rs:3 too."], vec![]);
    ed.open(&root.join("src/a.rs")).unwrap();
    let path = ed.doc().path.clone().unwrap();
    let both = crate::flags::on_line(&ed.workspace.claude.flags, &path, 2);
    let (newest, older) = (both[0].id, both[1].id);
    // (the older answer's paragraph in view, the newest's not: as `]n` or a hover row takes the column there)
    ed.workspace.claude.chat.as_mut().unwrap().hidden = false;
    crate::flags::to_paragraph(&mut ed, older);
    screen(&mut ed);
    screen(&mut ed);
    let flags = &ed.workspace.claude.flags;
    let f = |id| crate::flags::find(&ed, id).unwrap();
    assert!(crate::flags::lit(flags, f(older)) && !crate::flags::lit(flags, f(newest)), "{:?}", flags.lit);
    assert!(crate::flags::line_lit(flags, f(newest)), "the line's `¶` lit by the older answer's paragraph");
    let state = ed.ui_state();
    assert!(state.flags.iter().any(|s| s.id == "flag:src/a.rs:3" && s.lit), "{:?}", state.flags);
}

/// #388 step 2, `]n` (chatlink Q8): on to the older answer's flag with the column in sight, the column at its paragraph —
/// whole, `▎` down its rows once the frame took it there (`Chat.reading` landed) — the keys in the code, no card.
#[test]
fn next_note_onto_an_older_answers_flag_shows_its_paragraph_whole() {
    let (_s, root, mut ed) = two_answers("flags-next-whole");
    ed.workspace.claude.chat.as_mut().unwrap().hidden = false;
    screen(&mut ed);
    // (the newest answer's two, then the older one's `src/a.rs:3`, `src/b.rs:2` — from line 1 the first is a.rs:3)
    feed(&mut ed, "]n]n");
    assert_eq!(crate::chat::shown(&root, ed.doc().path.as_ref().unwrap()), "src/b.rs");
    let id = flag_at(&ed, 1);
    assert_eq!(crate::flags::find(&ed, id).unwrap().origin.n, 1, "the older answer's");
    assert!(reads(&ed, id), "the column taken to its paragraph");
    screen(&mut ed);
    screen(&mut ed);
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert!(matches!(c.reading, Some((_, true))), "taken there: the `▎` beside it");
    let laid = ed.screen.chat_laid.borrow().clone();
    let at = crate::chatdoc::reading_rows(c, &laid).expect("its rows marked");
    let rows: Vec<&crate::chat::ChatRow> = laid.iter().flat_map(|(_, r)| r.iter()).collect();
    let words: Vec<String> =
        rows[at].iter().map(|r| r.spans.iter().map(|s| s.text.as_str()).collect()).collect();
    let words = words.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(words.contains("It is set in src/b.rs:2."), "the paragraph whole: {words}");
    assert_eq!(ed.workspace.viewer.at, crate::workspace::At::Code, "the keys in the code");
    assert_eq!(ed.workspace.claude.notes.open, None, "no card");
}

/// #388 step 2, `space N`'s flag rows (chatlink §1.5): every flag a row — the newest answer's first in the order written,
/// then the older's — its place and the sentence that named it (Q10), `chat` its hint, a faint `¶`; `enter` on one goes to
/// its line with the column at its paragraph, the keys in the code; `C-x` on one closes nothing and says why. With no
/// note and no flag, `space N` and `]n` say so.
#[test]
fn space_n_lists_every_flag_and_enter_goes_to_its_paragraph() {
    let (_s, _root, mut ed) = two_answers("flags-list");
    feed(&mut ed, " N");
    let p = ed.picker.as_ref().expect("the notes list");
    assert!(crate::notes::is_list(&p.title));
    let labels: Vec<String> = p.shown().map(|i| i.label.clone()).collect();
    assert_eq!(
        labels,
        [
            "src/a.rs:7  The loop ends at src/a.rs:7.",
            "src/a.rs:3  The guard is at src/a.rs:3 too.",
            "src/a.rs:3  The count is read in src/a.rs:3.",
            "src/b.rs:2  It is set in src/b.rs:2.",
        ]
    );
    assert!(p.shown().all(|i| i.hint.starts_with("chat") && i.glyph == Some(("¶", "ui.virtual"))));
    // `C-x` on a flag's row: nothing closed, said
    feed(&mut ed, "<C-x>");
    assert_eq!(ed.picker.as_ref().map(|p| p.len()), Some(4), "every row still there");
    let said = ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
    assert!(said.starts_with("a flag isn't closed"), "{said}");
    // `enter` on the last row: src/b.rs's line 2, the column at the older answer's paragraph
    feed(&mut ed, "<C-n><C-n><C-n><ret>");
    assert!(ed.picker.is_none());
    assert_eq!((ed.doc().path.as_ref().unwrap().ends_with("src/b.rs"), ed.cursor_line()), (true, 1));
    let id = flag_at(&ed, 1);
    assert!(reads(&ed, id), "the column at its paragraph");
    assert_eq!(ed.workspace.viewer.at, crate::workspace::At::Code, "the keys in the code");
    // None of either: said, no list
    let (_s2, _root2, mut ed) = project("flags-list-none", &["src/a.rs"]);
    for keys in [" N", "]n"] {
        ed.status = None;
        feed(&mut ed, keys);
        assert!(ed.picker.is_none(), "{keys}");
        let said = ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
        assert!(said.starts_with("no notes or flags yet"), "{keys}: {said}");
    }
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
