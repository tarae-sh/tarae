//! Focus A1b step 3: the chat folds an older answer's placed paragraphs — one stub row each, every word kept in the
//! transcript's document, folded under any selection (#271), going to the code's card by `enter` or a press.

use std::path::{Path, PathBuf};

use super::*;
use crate::chat::{Chat, Msg, Role, State};
use crate::event::{Mouse, MouseKind};

const W: u16 = 140;
const H: u16 = 34;

thread_local! {
    /// When this test began (`project`): what `frame` holds to the shared patience.
    static BEGAN: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

/// A project of the test's own with `src/a.rs` and `src/b.rs`, ten lines each, and a chat with no claude behind it,
/// up beside the code.
fn project(name: &str) -> (crate::scratch::Scratch, PathBuf, Editor) {
    let scratch = crate::scratch::Scratch::new(name);
    let root = std::fs::canonicalize(&*scratch).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    for f in ["src/a.rs", "src/b.rs"] {
        let text: String = (1..=10).map(|i| format!("let line_{i} = {i};\n")).collect();
        std::fs::write(root.join(f), text).unwrap();
    }
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    ed.workspace.claude.chat = Some(Chat::new());
    BEGAN.set(Some(std::time::Instant::now()));
    (scratch, root, ed)
}

/// A frame, as the screen shows it — within the shared patience: a loop of keys and frames that never gets what it
/// waits for fails its test, it doesn't spin.
#[track_caller]
fn frame(ed: &mut Editor, w: u16) -> String {
    let began = BEGAN.get().expect("`project` first");
    assert!(began.elapsed() < crate::test_results::PATIENCE, "still drawing frames past the patience");
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, w, H).unwrap();
    let mut p = vt100::Parser::new(H, w, 0);
    p.process(&bytes);
    p.screen().contents()
}

/// A turn answering `question` with `text`, its notes placed, a frame after.
fn turn(ed: &mut Editor, question: &str, text: &str) {
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::User, question, ""));
    c.follow.new_turn();
    c.msgs.push(Msg::new(Role::Assistant, text, ""));
    crate::chatnotes::turn_over(ed, true);
    frame(ed, W);
    crate::test_results::settle_until(ed, |ed| {
        crate::places::answered(ed) && ed.workspace.claude.flags.answer.is_none()
    });
    frame(ed, W);
}

/// The chat's rows as the last frame laid them out (the frame's own left out): each row's text as drawn.
fn rows(ed: &Editor) -> Vec<String> {
    let laid = ed.screen.chat_laid.borrow();
    let drawn = |r: &ChatRow| r.spans.iter().map(|s| s.text.as_str()).collect::<String>();
    let of = laid.iter().filter(|(of, _)| *of != Of::Own);
    of.flat_map(|(_, rows)| rows.iter().map(drawn)).collect()
}

fn stubs(ed: &Editor) -> Vec<String> {
    rows(ed).into_iter().filter(|r| r.starts_with('◂')).collect()
}

/// The stub row starting with `head`.
fn stub_row(ed: &Editor, head: &str) -> ChatRow {
    let laid = ed.screen.chat_laid.borrow();
    let mut rows = laid.iter().flat_map(|(_, rows)| rows.iter());
    rows.find(|r| r.spans.first().is_some_and(|s| s.text.starts_with(head))).expect("the stub").clone()
}

fn text(ed: &Editor) -> String {
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    c.transcript.as_ref().unwrap().pane.doc_in().unwrap().text.to_string()
}

#[track_caller]
fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(key);
        frame(ed, W);
    }
}

/// The chat's view mode, its cursor on the first stub.
fn onto_a_stub(ed: &mut Editor) {
    feed(ed, "<esc>gg");
    for _ in 0..rows(ed).len() {
        if stub_here(ed).is_some() {
            return;
        }
        feed(ed, "j");
    }
    panic!("no stub to go down to: {:#?}", rows(ed));
}

const FIRST: &str = "The loop at src/a.rs:3 prints each name, how many knots it has and the whole length of its \
                     thread as the map hands them over.\n\nThe map is filled at src/b.rs:2, before the loop starts.\n\n\
                     It prints them in the map's own order, which changes from run to run.";
const SECOND: &str = "A hash map at src/a.rs:3 keeps one entry a name.\n\nA Vec would keep the order.";

fn two_turns(ed: &mut Editor) {
    turn(ed, "what does the loop print?", FIRST);
    turn(ed, "why a hash map?", SECOND);
}

/// The older answer reads as stubs and its place-less paragraph, the newest whole; the two stubs have no air between
/// them, and the document holds every word of both answers.
#[test]
fn an_older_answers_placed_paragraphs_fold_to_one_row_each_and_the_document_keeps_them() {
    let (_s, _root, mut ed) = project("fold-two");
    two_turns(&mut ed);
    let rows = rows(&ed);
    let stubs: Vec<&String> = rows.iter().filter(|r| r.starts_with('◂')).collect();
    assert_eq!(stubs.len(), 2, "{rows:#?}");
    assert!(stubs[0].starts_with("◂ a.rs:3  The loop at src/a.rs:3 prints"), "{}", stubs[0]);
    assert!(stubs[1].starts_with("◂ b.rs:2  The map is filled"), "{}", stubs[1]);
    let at = rows.iter().position(|r| r.starts_with("◂ a.rs")).unwrap();
    assert!(rows[at + 1].starts_with("◂ b.rs"), "no air between two stubs: {rows:#?}");
    assert!(rows[at + 2].trim().is_empty(), "air before the whole paragraph: {rows:#?}");
    assert!(rows.iter().any(|r| r.starts_with("It prints them")), "the place-less whole: {rows:#?}");
    assert!(rows.iter().any(|r| r.starts_with("A hash map at src/a.rs:3")), "the newest whole: {rows:#?}");
    let doc = text(&ed).replace('\n', " ");
    for word in FIRST.split_whitespace().chain(SECOND.split_whitespace()) {
        assert!(doc.contains(word), "{word:?} isn't in {doc:?}");
    }
}

/// A stub stands for a paragraph that goes on: `…` on the row, the whole on its card from the row's first cell in the
/// card's own ink, every byte of it in a piece (a card draws only its pieces — the space a wrap took was dropped).
#[test]
fn a_stubs_card_has_its_paragraph_whole() {
    let (_s, _root, mut ed) = project("fold-card");
    two_turns(&mut ed);
    let row = stub_row(&ed, "◂ a.rs:3");
    let drawn: String = row.spans.iter().map(|s| s.text.as_str()).collect();
    assert!(drawn.ends_with('…'), "{drawn}");
    let (from, to, whole) = row.cut.clone().expect("its cut");
    assert_eq!((from, to), (0, drawn.width()));
    assert_eq!(whole.text, FIRST.split("\n\n").next().unwrap());
    assert!(whole.style.fg.is_some() || whole.style.bg.is_some(), "a card's own ink: {:?}", whole.style);
    let mut at = 0;
    for (r, _) in &whole.pieces {
        assert_eq!(r.start, at, "a gap in the pieces: {:?}", whole.pieces);
        at = r.end;
    }
    assert_eq!(at, whole.text.len(), "{:?}", whole.pieces);
    assert!(whole.pieces.iter().all(|(_, s)| s.fg.is_some() || s.bg.is_some()), "{:?}", whole.pieces);
}

/// A card's pieces are bytes of its text, wherever the layout left blanks: two spaces at a wrap leave one at the row's
/// end, which the document's line doesn't have (`body`) — counted, every later piece stands past its letters, and
/// with Hangul after it the card slices inside a character.
#[test]
fn a_stubs_card_pieces_are_its_texts_bytes_at_a_double_space() {
    let (_s, _root, mut ed) = project("fold-card-blanks");
    let long = format!("The loop at src/a.rs:3  {}", "실타래는  이어진  실  한  가닥이다  ".repeat(12));
    turn(&mut ed, "where?", long.trim_end());
    turn(&mut ed, "and?", "Nothing more.");
    let (_, _, whole) = stub_row(&ed, "◂ a.rs:3").cut.expect("its cut");
    let mut at = 0;
    for (r, _) in &whole.pieces {
        assert_eq!(r.start, at, "a gap in the pieces");
        assert!(whole.text.get(r.clone()).is_some(), "{r:?} isn't in {:?}", whole.text);
        at = r.end;
    }
    assert_eq!(at, whole.text.len());
}

/// The newest answer stays whole while the next is under way — until that turn ends, then it folds.
#[test]
fn the_newest_answer_stays_whole_until_the_next_turn_ends() {
    let (_s, _root, mut ed) = project("fold-newest");
    turn(&mut ed, "what does the loop print?", FIRST);
    assert_eq!(stubs(&ed), Vec::<String>::new(), "one answer: whole");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::User, "why a hash map?", ""));
    c.state = State::Thinking;
    frame(&mut ed, W);
    assert_eq!(stubs(&ed), Vec::<String>::new(), "its question sent: whole");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::Assistant, "A partial", ""));
    frame(&mut ed, W);
    assert_eq!(stubs(&ed), Vec::<String>::new(), "the next answer being written: whole");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.state = State::Idle;
    frame(&mut ed, W);
    assert_eq!(stubs(&ed).len(), 2, "its turn ended: {:#?}", rows(&ed));
}

/// A turn with no answer (it failed before a word) changes nothing: the last answer stays the newest.
#[test]
fn a_question_with_no_answer_leaves_the_newest_whole() {
    let (_s, _root, mut ed) = project("fold-no-answer");
    turn(&mut ed, "what does the loop print?", FIRST);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::User, "and then?", ""));
    c.msgs.push(Msg::new(Role::Note, "stopped", ""));
    frame(&mut ed, W);
    assert_eq!(stubs(&ed), Vec::<String>::new());
}

/// #271, #269's rule: a stub stays folded under any selection — a search hit past what it shows, `x` on it, which takes
/// its paragraph whole — nothing moving, and `y` copies every word of it (the document keeps them).
#[test]
fn a_stub_stays_folded_under_any_selection() {
    let (_s, _root, mut ed) = project("fold-stays");
    two_turns(&mut ed);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    frame(&mut ed, W);
    feed(&mut ed, "<esc>gg");
    let folded = rows(&ed);
    assert_eq!(stubs(&ed).len(), 2);
    // (a word only the card shows)
    feed(&mut ed, "/hands<ret>");
    assert_eq!(at(&ed), "hands");
    assert_eq!(rows(&ed), folded, "a search hit in it: folded, nothing moved");
    feed(&mut ed, "<esc>;x");
    assert_eq!(rows(&ed), folded, "`x`");
    let first = FIRST.split("\n\n").next().unwrap();
    assert_eq!(at(&ed).trim_end(), first, "`x` takes the paragraph whole");
    feed(&mut ed, "y");
    let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the copy's result");
    ed.handle_event(ev);
    let copied = ed.registers.get(&'"').and_then(|v| v.first()).cloned().unwrap_or_default();
    assert_eq!(copied.trim_end(), first, "`y` copies every word of it");
    // (the second of two: the first stays as it is too)
    feed(&mut ed, "<esc>gg/filled<ret>");
    assert_eq!(rows(&ed), folded);
    assert_eq!(at(&ed), "filled");
}

/// #271: a selection wider than a character anywhere on a stub's line lights all its words (#269's folded rows), the
/// primary's ground; one that isn't on it, or a cursor alone, lights nothing.
#[test]
fn a_selection_on_a_stubs_line_lights_its_words() {
    let (_s, _root, mut ed) = project("fold-lit");
    two_turns(&mut ed);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    frame(&mut ed, W);
    feed(&mut ed, "<esc>gg");
    assert_eq!((lit(&mut ed, 0), lit(&mut ed, 1)), (false, false), "a cursor alone");
    // (from the question above into the first stub's hidden words, the cursor off its row)
    let line = |ed: &Editor, starts: &str| {
        let d = transcript(ed).unwrap().pane.doc_in().unwrap();
        (0..d.text.len_lines()).find(|&l| d.text.line(l).to_string().starts_with(starts)).unwrap()
    };
    let d = transcript(&ed).unwrap().pane.doc_in().unwrap();
    let hands = d.text.to_string().find("hands").unwrap();
    let q = mv::line_start(&d.text, line(&ed, "what does the loop"));
    let d = transcript_mut(&mut ed).unwrap().pane.doc_in_mut().unwrap();
    d.set_selection(Selection::single(Range::new(hands + 2, q)));
    frame(&mut ed, W);
    assert_eq!((lit(&mut ed, 0), lit(&mut ed, 1)), (true, false), "the first one's words, the second's not");
    // (a selection only in what it folds — not the primary, whose cursor would bring its card over it — lights its words
    // all the same, on the other selections' ground)
    let d = transcript_mut(&mut ed).unwrap().pane.doc_in_mut().unwrap();
    let ranges = vec![Range::new(q, q + "what".len()), Range::new(hands, hands + "hands".len())];
    d.set_selection(Selection::new(ranges, 0));
    frame(&mut ed, W);
    assert_eq!((lit(&mut ed, 0), lit(&mut ed, 1)), (true, false), "a selection in its folded words only");
    let primary = ground_at(&mut ed, "what does the loop");
    assert_ne!(stub_ground(&mut ed, 0), primary, "the other selections' ground, not the primary's");
    feed(&mut ed, "%");
    assert_eq!((lit(&mut ed, 0), lit(&mut ed, 1)), (true, true), "all of it");
    feed(&mut ed, ";");
    assert_eq!((lit(&mut ed, 0), lit(&mut ed, 1)), (false, false), "let go");
}

/// Whether stub `k`'s words are drawn on another ground than its place (`◂ a.rs:3`), which a selection never lights.
fn lit(ed: &mut Editor, k: usize) -> bool {
    let (words, place) = stub_grounds(ed, k);
    words != place
}

/// The ground of stub `k`'s words.
fn stub_ground(ed: &mut Editor, k: usize) -> vt100::Color {
    stub_grounds(ed, k).0
}

/// The grounds of stub `k`'s words and of its place, as a frame draws them.
fn stub_grounds(ed: &mut Editor, k: usize) -> (vt100::Color, vt100::Color) {
    let p = coloured(ed);
    let (y, _) = ed.screen.chat_stubs.borrow()[k];
    let x = ed.screen.chat_links.borrow().iter().find(|l| l.0 == y).map(|l| l.1).unwrap();
    let head = "◂ a.rs:3  ".chars().count();
    let bg = |x: usize| p.screen().cell(y as u16, x as u16).map(|c| c.bgcolor()).unwrap();
    (bg(x + head + 1), bg(x))
}

/// The ground of the first cell of `text` in the chat, as a frame draws it.
fn ground_at(ed: &mut Editor, text: &str) -> vt100::Color {
    let p = coloured(ed);
    let chat_x = ed.screen.chat_x.unwrap();
    let screen = p.screen().contents();
    let (x, y) = (screen.lines().enumerate())
        .find_map(|(y, l)| {
            let l: String = l.chars().skip(chat_x).collect();
            Some((chat_x + l[..l.find(text)?].chars().count(), y))
        })
        .expect(text);
    p.screen().cell(y as u16, x as u16).map(|c| c.bgcolor()).unwrap()
}

/// A frame with its colours.
fn coloured(ed: &mut Editor) -> vt100::Parser {
    // (crossterm drops every colour under NO_COLOR, which tarae's test runner sets — as `term_snapshots`' `Shot`)
    crossterm::style::force_color_output(true);
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, W, H).unwrap();
    let mut p = vt100::Parser::new(H, W, 0);
    p.process(&bytes);
    p
}

/// #271: a drag from the question onto a stub takes its paragraph whole — it stays folded, nothing moves under the
/// pointer, its card not over the rows the drag goes on to — on to the next stub both, back up the first again; let go,
/// its card comes (the view mode's cursor on it), and `y` copies every word. Up from the newest answer onto it, the same:
/// from its paragraph's start, the character pressed still in it.
#[test]
fn a_drag_onto_a_stub_takes_its_paragraph_whole_and_moves_nothing() {
    use crate::event::MouseKind as K;
    let (_s, _root, mut ed) = project("fold-drag");
    two_turns(&mut ed);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    frame(&mut ed, W);
    feed(&mut ed, "<esc>gg<esc>");
    let screen = frame(&mut ed, W);
    let folded = rows(&ed);
    let chat_x = ed.screen.chat_x.unwrap();
    let cell = |word: &str| {
        screen.lines().enumerate().find_map(|(y, l)| {
            let l: String = l.chars().skip(chat_x).collect();
            let b = l.find(word)?;
            Some((chat_x + l[..b].chars().count(), y))
        })
    };
    let mouse = |ed: &mut Editor, kind: K, (x, y): (usize, usize)| {
        ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
        frame(ed, W)
    };
    let (qx, qy) = cell("what does the loop").unwrap();
    let (sx, sy) = cell("◂ a.rs:3").unwrap();
    let (tx, ty) = cell("◂ b.rs:2").unwrap();
    mouse(&mut ed, K::Down, (qx, qy));
    let held = mouse(&mut ed, K::Drag, (sx + 12, sy));
    let first = FIRST.split("\n\n").next().unwrap();
    assert_eq!(rows(&ed), folded, "folded:\n{held}");
    assert!(at(&ed).trim_end().ends_with(first), "its paragraph whole: {:?}", at(&ed));
    assert!(at(&ed).starts_with("what does the loop"), "from the press: {:?}", at(&ed));
    assert_eq!(held.lines().nth(sy), screen.lines().nth(sy), "nothing over the row under the pointer");
    assert_eq!(held.lines().nth(ty), screen.lines().nth(ty), "nor the row after it");
    assert!(lit(&mut ed, 0) && !lit(&mut ed, 1), "its words lit");
    mouse(&mut ed, K::Drag, (tx + 3, ty));
    assert_eq!(rows(&ed), folded);
    assert!(at(&ed).trim_end().ends_with("before the loop starts."), "both: {:?}", at(&ed));
    assert!(lit(&mut ed, 0) && lit(&mut ed, 1));
    mouse(&mut ed, K::Drag, (sx, sy));
    assert!(at(&ed).trim_end().ends_with(first), "back to the first: {:?}", at(&ed));
    mouse(&mut ed, K::Up, (sx, sy));
    assert_eq!(rows(&ed), folded, "let go");
    assert!(ed.screen.cut_card.borrow().is_some(), "its card, the cursor on it");
    feed(&mut ed, "y");
    let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the copy's result");
    ed.handle_event(ev);
    let copied = ed.registers.get(&'"').and_then(|v| v.first()).cloned().unwrap_or_default();
    assert!(copied.trim_end().ends_with(first), "`y` copies every word: {copied:?}");
    // (up: pressed on the newest answer, dragged onto the second stub, then the first)
    let (vx, vy) = cell("A Vec would").unwrap();
    mouse(&mut ed, K::Down, (vx, vy));
    let held = mouse(&mut ed, K::Drag, (tx + 12, ty));
    assert_eq!(rows(&ed), folded, "folded, dragged up:\n{held}");
    let second = FIRST.split("\n\n").nth(1).unwrap();
    assert!(at(&ed).starts_with(second), "from its paragraph's start: {:?}", at(&ed));
    assert!(at(&ed).ends_with("A"), "the character pressed still in it: {:?}", at(&ed));
    assert!(!lit(&mut ed, 0) && lit(&mut ed, 1), "its words lit");
    mouse(&mut ed, K::Drag, (sx + 3, sy));
    assert_eq!(rows(&ed), folded);
    assert!(at(&ed).starts_with(first) && at(&ed).ends_with("A"), "both, up: {:?}", at(&ed));
    assert!(lit(&mut ed, 0) && lit(&mut ed, 1));
    mouse(&mut ed, K::Up, (sx + 3, sy));
}

fn at(ed: &Editor) -> String {
    let d = transcript(ed).unwrap().pane.doc_in().unwrap();
    let r = d.selection().primary();
    d.text.byte_slice(r.from()..r.to()).to_string()
}

/// A selection on the second stub's first words stays those words, and a view whose top is that line keeps it.
#[test]
fn a_stubs_first_word_and_the_views_top_stay_where_they_were() {
    let (_s, _root, mut ed) = project("fold-first-word");
    two_turns(&mut ed);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    frame(&mut ed, W);
    feed(&mut ed, "<esc>gg/The map<ret>");
    assert_eq!(stubs(&ed).len(), 2, "{:#?}", rows(&ed));
    assert_eq!(at(&ed), "The map");
    // (again with the stub at the view's top: selected there, no key to bring anything in sight)
    feed(&mut ed, ";");
    assert_eq!(stubs(&ed).len(), 2, "{:#?}", rows(&ed));
    let d = transcript_mut(&mut ed).unwrap().pane.doc_in_mut().unwrap();
    let line = (0..d.text.len_lines()).find(|&l| d.text.line(l).to_string().starts_with("The map")).unwrap();
    let start = mv::line_start(&d.text, line);
    d.set_top(line);
    d.set_selection(Selection::single(Range::new(start, start + "The map".len())));
    frame(&mut ed, W);
    assert_eq!(stubs(&ed).len(), 2, "{:#?}", rows(&ed));
    let d = transcript(&ed).unwrap().pane.doc_in().unwrap();
    assert!(d.text.line(d.top).to_string().starts_with("The map"), "the top moved to line {}", d.top);
    assert_eq!(at(&ed), "The map");
}

const THREE: &str =
    "The loop is at src/a.rs:3.\n\nThe map is filled at src/b.rs:2.\n\nThe knots are counted at src/a.rs:5.";

/// A search hit in the middle of three stubs stays as it is — `n` the same hit again, the stubs as they were — and a
/// cursor left there stays on its paragraph.
#[test]
fn the_middle_of_three_stubs_keeps_what_is_selected_in_it() {
    let (_s, _root, mut ed) = project("fold-three");
    turn(&mut ed, "where?", THREE);
    turn(&mut ed, "and?", "Nothing more.");
    assert_eq!(stubs(&ed).len(), 3, "{:#?}", rows(&ed));
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    frame(&mut ed, W);
    feed(&mut ed, "<esc>gg/filled<ret>");
    assert_eq!(at(&ed), "filled");
    assert_eq!(stubs(&ed).len(), 3, "{:#?}", rows(&ed));
    feed(&mut ed, "n");
    assert_eq!(at(&ed), "filled", "the one hit, again");
    feed(&mut ed, ";");
    assert_eq!(stubs(&ed).len(), 3, "let go: {:#?}", rows(&ed));
    let d = transcript(&ed).unwrap().pane.doc_in().unwrap();
    let line = mv::line_of(&d.text, d.selection().primary().cursor(&d.text));
    assert!(d.text.line(line).to_string().starts_with("The map"), "the cursor left line {line}");
}

/// Every selection settles in one frame: wherever it stands and however wide — a stub opening, the blank lines beside it
/// coming back, its words staying selected — the next frame lays nothing out and changes no text. A fold's rows that
/// changed each frame would be laid out and drawn for ever with no key pressed.
#[test]
fn every_selection_settles_in_one_frame() {
    settles("fold-settles-two", FIRST);
    settles("fold-settles-three", THREE);
}

/// `answer` older than the next, every selection of it tried.
fn settles(name: &str, answer: &str) {
    let (_s, _root, mut ed) = project(name);
    turn(&mut ed, "what does the loop print?", answer);
    turn(&mut ed, "why a hash map?", SECOND);
    assert!(stubs(&ed).len() >= 2, "{:#?}", rows(&ed));
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    frame(&mut ed, W);
    feed(&mut ed, "<esc>");
    let state = |ed: &Editor| {
        let d = transcript(ed).unwrap().pane.doc_in().unwrap();
        let laid = ed.screen.chat_laid.borrow();
        // (a message's rows: the air between them is a row made anew each frame)
        let msgs = laid.iter().filter(|(of, _)| matches!(of, Of::Msg(_)));
        let rows: Vec<*const ChatRow> = msgs.map(|(_, r)| r.as_ptr()).collect();
        (d.version(), d.selection().clone(), rows)
    };
    let mut tried = 0;
    let mut from = 0;
    while from < text(&ed).len() {
        for wide in [0, 1, 2, 6, 60, 400] {
            let t = transcript_mut(&mut ed).unwrap().pane.doc_in_mut().unwrap();
            let len = t.text.len_bytes();
            let a = crate::graphemes::prev_boundary(&t.text, from.min(len));
            let b = crate::graphemes::next_boundary(&t.text, (a + wide).min(len));
            t.set_selection(Selection::single(Range::new(a, b.max(a))));
            let selected = at(&ed).replace('\n', "");
            frame(&mut ed, W);
            let once = state(&ed);
            frame(&mut ed, W);
            assert_eq!(state(&ed), once, "from {a}, {wide} wide, in {:?}", text(&ed));
            // (a plain cursor on a blank line that goes moves off it; what's wider keeps its words)
            if wide > 0 {
                assert_eq!(at(&ed).replace('\n', ""), selected, "from {a}, {wide} wide");
            }
            tried += 1;
        }
        from += 7;
    }
    assert!(tried > 100, "{tried}");
}

/// Blank lines that are all that differ are each their own change, one that comes before the line break above it, the
/// changes making the text they were asked to; words that differ are no such change.
#[test]
fn only_blank_lines_are_changed_one_by_one() {
    let made = |was: &str, text: &str| {
        let mut rope = ropey::Rope::from_str(was);
        Transaction::new(blank_changes(was, text, 0).expect("blank lines only")).apply(&mut rope, false);
        rope.to_string()
    };
    for (was, text) in [
        ("A\nB\n", "A\n\nB\n"),
        ("A\n\nB\n", "A\nB\n"),
        ("A\nB\nC\n", "A\n\nB\n\nC\n"),
        ("A\n\nB\n\nC\n", "A\nB\nC\n"),
        ("A\nB\n", "A\n\n\nB\n"),
        ("A\n\nB\nC\n", "A\nB\n\nC\n"),
        ("\nB\n", "\n\nB\n"),
        ("A\n", "A\n\n"),
    ] {
        assert_eq!(made(was, text), text, "{was:?} → {text:?}");
    }
    let at = |from, to, insert: &str| Change { from, to, insert: insert.into() };
    let three = blank_changes("A\nB\nC\n", "A\n\nB\n\nC\n", 10);
    assert_eq!(three, Some(vec![at(11, 11, "\n"), at(13, 13, "\n")]), "before the breaks above them");
    let back = blank_changes("A\n\nB\n\nC\n", "A\nB\nC\n", 10);
    assert_eq!(back, Some(vec![at(12, 13, ""), at(15, 16, "")]));
    assert_eq!(blank_changes("A\nxyz\nB", "A\n\n\nB", 0), None, "a rewrite");
    assert_eq!(blank_changes("A\n", "A\nmore\n", 0), None, "words that came");
}

/// `y` over the older answer copies every word: the selection opens what it's on, and the document has it all anyway.
#[test]
fn a_copy_across_stubs_has_their_words() {
    let (_s, _root, mut ed) = project("fold-copy");
    two_turns(&mut ed);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    frame(&mut ed, W);
    feed(&mut ed, "<esc>%");
    let selected = at(&ed);
    for word in FIRST.split_whitespace() {
        assert!(selected.contains(word), "{word:?} isn't in {selected:?}");
    }
    assert_eq!(stubs(&ed).len(), 2, "folded as ever: {:#?}", rows(&ed));
}

/// A search hit far into a stub's words keeps its row in sight, by the rows drawn now.
#[test]
fn a_hit_in_a_stub_keeps_its_row_in_sight() {
    let (_s, _root, mut ed) = project("fold-sight");
    // (the hit far down a paragraph taller than the view: its stub in sight, its row past the foot once it's open)
    let long = format!("The loop at src/a.rs:3 {}needle.\n\nThe last word.", "goes on and on ".repeat(200));
    turn(&mut ed, "a question", &long);
    turn(&mut ed, "the last question", "Nothing here.");
    assert_eq!(stubs(&ed).len(), 1, "{:#?}", rows(&ed));
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    frame(&mut ed, W);
    feed(&mut ed, "<esc>gg/needle<ret>");
    assert_eq!(stubs(&ed).len(), 1);
    let t = transcript(&ed).unwrap();
    let d = t.pane.doc_in().unwrap();
    let g = d.rows.as_deref().unwrap();
    let c = d.selection().primary().cursor(&d.text);
    let line = mv::line_of(&d.text, c);
    let row = g.index(line, crate::wrap::row_of(g.of(line), c));
    let (top, size) = (t.top_row(), t.pane.size.unwrap().0);
    assert!((top..top + size).contains(&row), "row {row} past {top}..+{size}");
}

/// `enter` on a stub goes to its flag's line, no card (#388: its words are the chat's), the chat keeping the keys; with a
/// selection it quotes, as everywhere; on any other row with none, the input.
#[test]
fn enter_on_a_stub_goes_to_its_flags_line() {
    let (_s, root, mut ed) = project("fold-enter");
    two_turns(&mut ed);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    frame(&mut ed, W);
    onto_a_stub(&mut ed);
    let (link, flag) = stub_here(&ed).unwrap();
    assert_eq!(link.path, root.join("src/a.rs"));
    let said = frame(&mut ed, W).matches("enter go there").count();
    assert_eq!(said, 1, "its hint, at the chat's foot — never the status line too (polish landing 5)");
    feed(&mut ed, "<ret>");
    assert_eq!(ed.doc().path.as_deref(), Some(root.join("src/a.rs").as_path()));
    assert_eq!(ed.cursor_line(), 2);
    assert_eq!((ed.workspace.claude.notes.open, ed.workspace.claude.flags.focus), (None, Some(flag)));
    assert!(has_keys(&ed), "the keys stay the chat's, as a link's");
    // (any other row, nothing selected: the input)
    feed(&mut ed, "gg<ret>");
    assert!(!has_keys(&ed) && ed.workspace.is_at(crate::workspace::At::Chat), "the input has the keys");
    // (a selection on a stub: quoted, its paragraph whole, as everywhere)
    onto_a_stub(&mut ed);
    feed(&mut ed, "x<ret>");
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert_eq!(c.quotes.len(), 1, "quoted");
    assert_eq!(c.quotes[0].text, FIRST.split("\n\n").next().unwrap());
}

/// An older answer's stub goes to its flag's line, the walk going on from it (`]n`). No card: its words are the chat's.
#[test]
fn a_stubs_link_goes_to_its_flags_line() {
    let (_s, root, mut ed) = project("fold-earlier");
    two_turns(&mut ed);
    let flag = stub_row(&ed, "◂ a.rs:3").stub.unwrap();
    let link = stub_row(&ed, "◂ a.rs:3").link.unwrap();
    go(&mut ed, link, flag);
    assert_eq!(ed.doc().path.as_deref(), Some(root.join("src/a.rs").as_path()));
    assert_eq!((ed.cursor_line(), ed.workspace.claude.flags.focus), (2, Some(flag)));
    assert_eq!(ed.workspace.claude.notes.open, None, "no card");
}

/// A stub whose file is gone goes nowhere, said.
#[test]
fn a_stub_whose_file_is_gone_goes_nowhere() {
    let (_s, root, mut ed) = project("fold-file-gone");
    two_turns(&mut ed);
    let flag = stub_row(&ed, "◂ b.rs:2").stub.unwrap();
    let link = stub_row(&ed, "◂ b.rs:2").link.unwrap();
    std::fs::remove_file(root.join("src/b.rs")).unwrap();
    go(&mut ed, link, flag);
    assert_ne!(ed.doc().path.as_deref(), Some(root.join("src/b.rs").as_path()));
}

/// A press on a stub does what its `enter` does.
#[test]
fn a_press_on_a_stub_goes_to_its_flags_line() {
    let (_s, root, mut ed) = project("fold-press");
    two_turns(&mut ed);
    // (the second: the turn's own card is open on the first's line)
    let (row, note) = ed.screen.chat_stubs.borrow()[1];
    let x = ed.screen.chat_links.borrow().iter().find(|l| l.0 == row).map(|l| l.1).unwrap();
    let (x, y) = (x as u16 + 1, row as u16);
    ed.handle_mouse(Mouse { kind: MouseKind::Down, x, y, alt: false, ctrl: false });
    assert_eq!(ed.doc().path.as_deref(), Some(root.join("src/b.rs").as_path()));
    assert_eq!(ed.workspace.claude.flags.focus, Some(note));
}

/// An answer all placed is its stubs; nothing else of it stays but its air.
#[test]
fn an_answer_all_placed_is_its_stubs() {
    let (_s, _root, mut ed) = project("fold-all");
    turn(&mut ed, "where?", "The loop is at src/a.rs:3.\n\nThe map is at src/b.rs:2.");
    turn(&mut ed, "and?", "Nothing more.");
    let rows = rows(&ed);
    assert_eq!(stubs(&ed).len(), 2, "{rows:#?}");
    let words: Vec<&String> = rows.iter().filter(|r| !r.trim().is_empty()).collect();
    assert_eq!(words.len(), 5, "the two questions, two stubs, the answer: {words:#?}");
    // (short enough to stand whole: no `…`, no card)
    let row = stub_row(&ed, "◂ a.rs:3");
    assert!(row.cut.is_none() && row.rest.is_none(), "{row:?}");
}

/// Only the paragraph that first names a place folds — a later naming's words are on no card — across an answer's
/// messages too; a table's row and a fence stay as they are.
#[test]
fn only_the_paragraph_a_card_holds_folds() {
    let (_s, _root, mut ed) = project("fold-first");
    let first = "The start is at src/a.rs:3.\n\nBack at src/a.rs:3 again.\n\n| where | what |\n|---|---|\n| src/b.rs:2 | the map |\n\n```\nsrc/b.rs:5\n```";
    turn(&mut ed, "where?", first);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::Tool, "call", ""));
    c.msgs.push(Msg::new(Role::Assistant, "Once more src/a.rs:3 here.", ""));
    turn(&mut ed, "and?", "Nothing more.");
    let rows = rows(&ed);
    let stubs: Vec<&String> = rows.iter().filter(|r| r.starts_with('◂')).collect();
    assert_eq!(stubs.len(), 1, "{rows:#?}");
    assert!(stubs[0].contains("The start"), "{stubs:?}");
    assert!(rows.iter().any(|r| r.starts_with("Back at src/a.rs:3")), "{rows:#?}");
    assert!(rows.iter().any(|r| r.starts_with("Once more")), "{rows:#?}");
    assert!(rows.iter().any(|r| r.contains("the map")), "the table's row: {rows:#?}");
    assert!(rows.iter().any(|r| r.contains("src/b.rs:5")), "the fence: {rows:#?}");
}

/// A list's items have no air between them: a place-less one between two stubs stays, every word of it.
#[test]
fn a_lists_item_between_two_stubs_stays() {
    let (_s, _root, mut ed) = project("fold-list");
    let list = "- The loop is at src/a.rs:3.\n- Nothing to see here.\n- The map is at src/b.rs:2.";
    turn(&mut ed, "where?", list);
    turn(&mut ed, "and?", "Nothing more.");
    let rows = rows(&ed);
    assert_eq!(stubs(&ed).len(), 2, "{rows:#?}");
    assert!(rows.iter().any(|r| r.contains("Nothing to see here.")), "{rows:#?}");
    assert!(text(&ed).contains("Nothing to see here."), "{}", text(&ed));
}

/// The chat narrow: the stub cut first by its words, then, too narrow for words, its place alone.
#[test]
fn a_narrow_stub_cuts_its_words_first_then_its_place() {
    let s = crate::chatnotes::Stub {
        turn: 0,
        name: "src/thread.rs".into(),
        said: 8,
        path: Path::new("/p/src/thread.rs").to_path_buf(),
        line: 8,
        flag: 1,
    };
    let para = [ChatRow {
        spans: vec![crate::markdown::Span {
            cut: None,
            hint: false,
            text: "The length comes from the knots".into(),
            style: Style::default(),
        }],
        ..ChatRow::default()
    }];
    let whole = "The length comes from the knots";
    let at = |w: usize| {
        let r = super::stub_row(&para, whole, &s, w, (Style::default(), Style::default()));
        let drawn: String = r.spans.iter().map(|s| s.text.as_str()).collect();
        (drawn, r)
    };
    assert_eq!(at(60).0, "◂ thread.rs:9  The length comes from the knots");
    assert_eq!(at(30).0, "◂ thread.rs:9  The length com…");
    // (under `STUB_WORDS_MIN`: no words, the place whole while it fits, then cut)
    assert_eq!(at(20).0.trim_end(), "◂ thread.rs:9");
    assert_eq!(at(9).0.trim_end(), "◂ thread…");
    for w in [60, 30, 20, 9] {
        let (_, r) = at(w);
        assert_eq!(lines_of(std::slice::from_ref(&r)).0.trim_end(), whole, "at {w} the document keeps it");
    }
    assert!(at(20).1.cut.is_some(), "its card says the words it has no room for");
}

/// A long conversation lays out the fold's rows only when what they depend on changes: a frame with nothing new keeps
/// every message's rows (the same `Arc`).
#[test]
fn a_frame_with_nothing_new_lays_nothing_out_again() {
    let (_s, _root, mut ed) = project("fold-cache");
    two_turns(&mut ed);
    let before: Vec<_> = ed.screen.chat_laid.borrow().iter().map(|(_, r)| r.clone()).collect();
    frame(&mut ed, W);
    let after: Vec<_> = ed.screen.chat_laid.borrow().iter().map(|(_, r)| r.clone()).collect();
    assert_eq!(before.len(), after.len());
    let msgs = ed.screen.chat_laid.borrow().iter().map(|(of, _)| *of).collect::<Vec<_>>();
    for (k, (a, b)) in before.iter().zip(&after).enumerate() {
        assert!(!matches!(msgs[k], Of::Msg(_)) || Arc::ptr_eq(a, b), "piece {k} laid out again");
    }
}

/// A place's line moved by an edit: the stub says where its flag is now.
#[test]
fn a_stub_says_where_its_flag_is_now() {
    let (_s, _root, mut ed) = project("fold-moved");
    two_turns(&mut ed);
    let flag = stub_row(&ed, "◂ b.rs:2").stub.unwrap();
    let flags = &mut ed.workspace.claude.flags.list;
    flags.iter_mut().find(|f| f.id == flag).unwrap().lines = 6..7;
    frame(&mut ed, W);
    assert!(stubs(&ed).iter().any(|s| s.starts_with("◂ b.rs:7")), "{:?}", stubs(&ed));
}

/// The view mode's cursor on a cut stub sits on the card's letters, which start at the row's first cell — not on the
/// row's own, past `◂ place`.
#[test]
fn the_view_cursor_on_a_stub_is_on_its_cards_letters() {
    let (_s, _root, mut ed) = project("fold-cursor");
    two_turns(&mut ed);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    frame(&mut ed, W);
    onto_a_stub(&mut ed);
    let row = ed.screen.chat_stubs.borrow().first().map(|s| s.0).expect("a stub in sight");
    let x = ed.screen.chat_links.borrow().iter().find(|l| l.0 == row).map(|l| l.1).unwrap();
    let cursors = ed.screen.cursors.borrow();
    assert!(cursors.contains(&(x, row)), "at ({x}, {row}): {cursors:?}");
}
