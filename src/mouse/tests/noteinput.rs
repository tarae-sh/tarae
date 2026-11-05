//! Focus C1: a review's comment typed in a card at the lines it's about — `space n` while drafts wait (#388: outside a
//! review it's the ask line, `notes::ask_about`); the prompt's `Input`, keys and presses as ever; on the status line only
//! where no card can stand.

use super::overlays::{note_card_keys, note_card_rows};
use super::{editor, feed, frame, press, said};
use crate::editor::{Editor, PromptKind};
use crate::event::{Mouse, MouseKind};
use crate::notes::By;
use crate::scratch::Scratch;

/// A file of 200 lines (`line 0`, the first), in a folder of the test's — a draft of yours waiting on its last line, so
/// `space n` writes comments (`codereview::drafting`).
fn project(name: &str) -> (Scratch, std::path::PathBuf, Editor) {
    let scratch = Scratch::new(name);
    let mut ed = editor();
    ed.config.llm.command = "sh".into();
    let path = scratch.join("a.rs");
    let text: String = (0..200).map(|i| format!("line {i}\n")).collect();
    ed.docs[0].text = ropey::Rope::from_str(&text);
    ed.docs[0].path.set(Some(path.clone()));
    crate::notes::add(&mut ed, &path, 199..200, By::You, "far off");
    frame(&mut ed);
    (scratch, path, ed)
}

/// A finding of the reviewer's on line `line` of the file, its id.
fn noted(ed: &mut Editor, path: &std::path::Path, line: usize, text: &str) -> u64 {
    crate::notes::add(ed, path, line..line + 1, By::Reviewer, text)
}

fn mouse(ed: &mut Editor, kind: MouseKind, x: usize, y: usize) {
    ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
    frame(ed);
}

fn typed(ed: &Editor) -> String {
    ed.prompt.as_ref().map(|p| p.text()).unwrap_or_default()
}

fn row(ed: &mut Editor, y: usize) -> String {
    frame(ed).lines().nth(y).unwrap_or_default().to_string()
}

/// `space n` on two lines while drafts wait, typing a comment: a card under them holding only the line, named by them, its keys its bottom line — the
/// code below moved down, nothing covered, nothing on the status line. The terminal's cursor in it, the prompt's rect the
/// input's row. `esc`: as it was.
#[test]
fn a_question_is_typed_in_a_card_under_its_lines() {
    let (_s, _path, mut ed) = project("c1-new");
    feed(&mut ed, "2Gxx");
    let before = frame(&mut ed);
    feed(&mut ed, " nwhy this");
    let (x, y, w, h) = ed.screen.note_card.get().expect("its card drawn");
    let rows = note_card_rows(&ed);
    assert_eq!(h, 3, "its line in its frame: {rows:#?}");
    assert!(rows[0].contains("note ── lines 2–3"), "{rows:#?}");
    assert!(rows[1].contains("comment on line 3: why this"), "{rows:#?}");
    assert!(rows[2].contains("enter later · esc close"), "{rows:#?}");
    assert!(row(&mut ed, y - 1).contains("line 2"), "under the selection's end");
    assert!(row(&mut ed, y + h).contains("line 3"), "the next line under the card");
    let d = ed.screen.prompt_input.get().expect("the line drawn");
    assert_eq!(d.y, y + 1, "its row");
    assert_eq!(
        ed.screen.prompt_card.get(),
        Some((x, y, w, h)),
        "its card the prompt's: it holds nothing else"
    );
    let shown = frame(&mut ed);
    assert_eq!(shown.matches("comment on line").count(), 1, "nothing bottom left: {shown}");
    let status = shown.lines().last().unwrap_or("");
    assert!(!status.contains("enter later"), "said once, on the card: {status}");
    let (bytes, cols, rows) = ed.last_frame.clone().unwrap();
    let mut p = vt100::Parser::new(rows, cols, 0);
    p.process(&bytes);
    assert_eq!(
        p.screen().cursor_position(),
        ((y + 1) as u16, (d.x + "why this".len()) as u16),
        "the cursor in it"
    );
    feed(&mut ed, "<esc>");
    assert!(ed.prompt.is_none() && ed.workspace.claude.notes.list.len() == 1, "nothing made");
    assert_eq!(ed.screen.note_card.get(), None, "its card gone");
    assert_eq!(frame(&mut ed), before, "the code as it was");
}

/// A review's comment goes as it does outside the card: a draft, its card not left open, so `space n` on the next line
/// is the next comment and never a reply to the last — its label in the card.
#[test]
fn a_reviews_comment_is_typed_there_and_leaves_no_card() {
    let (_s, _path, mut ed) = project("c1-comment");
    feed(&mut ed, "5G na word");
    let rows = note_card_rows(&ed);
    assert!(rows[1].contains("comment on line 5: a word"), "{rows:#?}");
    feed(&mut ed, "<ret>");
    assert_eq!(ed.workspace.claude.notes.open, None, "no card left");
    feed(&mut ed, "9G n");
    assert!(matches!(ed.prompt.as_ref().map(|p| p.kind), Some(PromptKind::Note)), "the next comment");
}

/// The status line's piece that sends the drafts, as drawn.
fn drafts_word(ed: &mut Editor) -> Option<crate::wordspot::Spot> {
    frame(ed);
    let spots = ed.screen.word_spots.borrow().clone();
    spots.into_iter().find(|s| s.press == crate::wordspot::Press::Command("review_send"))
}

/// C1b: while drafts wait the status line says how many and the key, calm — no `▲`, not the cards' `waiting` — past the
/// toast, and the whole of it is one button: its ground lightens under the mouse, a press sends them from wherever the keys
/// are, and with none left it's gone. A press while Claude works queues them, as the key does.
#[test]
fn the_status_line_counts_the_drafts_and_a_press_sends_them() {
    let (_s, path, mut ed) = project("c1b-drafts-word");
    ed.workspace.claude.notes.list.clear();
    let (tx, rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    assert!(drafts_word(&mut ed).is_none(), "none waiting: nothing said");
    crate::notes::add(&mut ed, &path, 3..4, By::You, "one");
    ed.toasts.clear();
    let line = |ed: &mut Editor| frame(ed).lines().last().unwrap_or_default().to_string();
    let shown = line(&mut ed);
    assert!(shown.contains("1 draft · space v s"), "{shown}");
    assert!(!shown.contains('▲') && !shown.contains("waiting"), "it reads apart from the cards': {shown}");
    feed(&mut ed, "10G ntwo<ret>");
    assert!(line(&mut ed).contains("2 drafts · space v s"), "{}", line(&mut ed));
    let s = drafts_word(&mut ed).expect("a word");
    let word: String = s.spans.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(word, "2 drafts · space v s", "count and key one button");
    assert_eq!(s.on, crate::mouse::Surface::Status);
    assert!(crate::wordspot::lit(&ed, s.from, s.y).is_some(), "its first cell");
    assert!(crate::wordspot::lit(&ed, s.to - 1, s.y).is_some(), "its last");
    press(&mut ed, s.from + 3, s.y);
    let sent: Vec<String> = rx.try_iter().filter(|l| l.contains("\"user\"")).collect();
    assert_eq!(sent.len(), 1, "one message: {sent:#?}");
    assert!(sent[0].contains("one") && sent[0].contains("two"), "{sent:#?}");
    assert!(drafts_word(&mut ed).is_none(), "none left: {}", line(&mut ed));
    // Claude at work: a press queues them
    crate::notes::add(&mut ed, &path, 19..20, By::You, "three");
    let s = drafts_word(&mut ed).expect("a word");
    // (a press of its own, not the first one's double: the word stands where it stood — nothing waves before it)
    ed.list_click = None;
    press(&mut ed, s.from, s.y);
    assert_eq!(rx.try_iter().filter(|l| l.contains("\"user\"")).count(), 0, "Claude's turn first");
    assert_eq!(said(&ed), "review queued — its 1 comment goes when Claude's turn ends");
}

/// C1b: the reviewer's findings alone are drafts too, and its own segment says them (`¶ 2 findings · space v r`): not said
/// twice, nor offered to a press unread. A draft of yours beside them counts them all, what `space v s` sends; its report
/// read, its segment gone, they're counted.
#[test]
fn the_reviewers_findings_alone_are_its_own_segments() {
    use crate::reviewer::{Reviewer, State};
    let (_s, path, mut ed) = project("c1b-findings");
    ed.workspace.claude.notes.list.clear();
    let finding =
        |ed: &mut Editor, line, text| crate::notes::add(ed, &path, line..line + 1, By::Reviewer, text);
    let ids = vec![finding(&mut ed, 3, "one"), finding(&mut ed, 9, "two")];
    let root = ed.workspace.root.clone();
    ed.workspace.reviewer = Some(Reviewer::for_test(root, State::Done, ids, "[verdict] Looks right."));
    assert!(crate::reviewer::status_shows(&ed, 0), "its segment says them");
    assert_eq!(crate::notes::drafts_said(&ed, 0), None, "not said twice");
    let a = crate::notes::add(&mut ed, &path, 20..21, By::You, "mine");
    assert_eq!(crate::notes::drafts_said(&ed, 0), Some(3), "yours among them: all of what goes");
    crate::notes::close(&mut ed, a);
    assert_eq!(crate::notes::drafts_said(&ed, 0), None);
    ed.workspace.reviewer.as_mut().unwrap().report_seen = true;
    assert!(!crate::reviewer::status_shows(&ed, 0));
    assert_eq!(crate::notes::drafts_said(&ed, 0), Some(2), "nothing else says them now");
}

/// C1b: the review's rail has the count, so the status line doesn't say it twice; the rail out of sight, it does.
#[test]
fn the_rail_counts_the_drafts_where_it_stands() {
    use crate::codereview::{Base, CodeReview};
    let (_s, path, mut ed) = project("c1b-drafts-rail");
    ed.workspace.claude.notes.list.clear();
    crate::notes::add(&mut ed, &path, 3..4, By::You, "kept");
    assert_eq!(crate::notes::drafts_said(&ed, 0), Some(1), "no rail");
    assert_eq!(crate::notes::drafts_said(&ed, 30), Some(1), "a side panel isn't the rail");
    ed.workspace.code_review = Some(CodeReview::for_test(Base::Head, ed.workspace.root.clone(), vec![]));
    assert_eq!(crate::notes::drafts_said(&ed, 30), None, "the rail says it");
    assert_eq!(crate::notes::drafts_said(&ed, 0), Some(1), "the rail folded away");
}

/// Many lines selected, the cursor at their start: the card under their end — and the view there, the line in sight
/// whatever the selection's size; `esc` brings the view back to the cursor.
#[test]
fn a_selection_of_many_lines_has_its_card_under_its_end() {
    let (_s, _path, mut ed) = project("c1-many");
    let (a, b) = (ed.doc().text.line_to_byte(9), ed.doc().text.line_to_byte(120));
    ed.doc_mut()
        .set_selection(crate::selection::Selection::single(crate::selection::Range { anchor: b, head: a }));
    frame(&mut ed);
    let top = ed.doc().top;
    feed(&mut ed, " n");
    let (_, y, ..) = ed.screen.note_card.get().expect("its card drawn");
    let rows = note_card_rows(&ed);
    assert!(rows[0].contains("lines 10–120"), "{rows:#?}");
    assert!(row(&mut ed, y - 1).contains("line 119"), "under the selection's end");
    assert!(ed.screen.prompt_input.get().is_some(), "the line in sight");
    feed(&mut ed, "<esc>");
    assert_eq!(ed.doc().top, top, "the view back where it was");
    assert!(
        frame(&mut ed).contains("line 9\n") || frame(&mut ed).contains("line 9 "),
        "the cursor in sight again"
    );
}

/// The cursor's line at the pane's foot: the view moves as far as the card needs, the line and its frame's foot in sight.
#[test]
fn a_line_at_the_panes_foot_brings_its_card_into_sight() {
    let (_s, _path, mut ed) = project("c1-foot");
    feed(&mut ed, "100Gzb");
    let top = ed.doc().top;
    let (_, rect, ..) = *ed.screen.panes.first().unwrap();
    feed(&mut ed, " n");
    assert!(ed.doc().top > top, "the view moved: {} from {top}", ed.doc().top);
    let (_, y, _, h) = ed.screen.note_card.get().expect("its card drawn");
    assert_eq!(h, 3, "whole");
    assert!(y + h <= rect.y + rect.h, "the frame's foot on screen: {y}+{h} in {rect:?}");
    assert!(row(&mut ed, y - 1).contains("line 99"), "under its line");
    feed(&mut ed, "<esc>");
    assert_eq!(ed.doc().top, top, "`esc`: the view as it was");
    feed(&mut ed, " n<ret>");
    assert_eq!(ed.doc().top, top, "nothing typed: as `esc`");
    assert_eq!(ed.workspace.claude.notes.list.len(), 1, "nothing made");
    // A review's comment leaves no card: as `esc` too
    feed(&mut ed, " na word");
    assert_ne!(ed.doc().top, top, "its card moved it");
    feed(&mut ed, "<ret>");
    assert_eq!(ed.workspace.claude.notes.list.len(), 2, "the comment made");
    assert_eq!(ed.doc().top, top, "no card to show");
}

/// `esc` puts the whole view back: the column it stood at and the rows of its top line above the screen, not only its line.
#[test]
fn esc_puts_the_whole_view_back() {
    let (_s, _path, mut ed) = project("c1-whole-view");
    let text: String = (0..200).map(|i| format!("line {i} {}\n", "word ".repeat(60))).collect();
    ed.docs[0].text = ropey::Rope::from_str(&text);
    for wrap in [false, true] {
        ed.doc_mut().wrap = Some(wrap);
        feed(&mut ed, "100Gzb");
        let at = ed.doc().text.line_to_byte(99) + 200;
        ed.doc_mut().set_selection(crate::selection::Selection::point(at));
        frame(&mut ed);
        let d = ed.doc_mut();
        match wrap {
            true => d.top_rows = Some((d.top, 1)),
            false => d.left += 30,
        }
        let view = (d.top, d.left, d.top_rows);
        frame(&mut ed);
        assert_eq!((ed.doc().top, ed.doc().left, ed.doc().top_rows), view, "the view as set");
        feed(&mut ed, " n");
        assert_ne!(ed.doc().top, view.0, "its card moved it");
        feed(&mut ed, "<esc>");
        assert_eq!((ed.doc().top, ed.doc().left, ed.doc().top_rows), view, "as it was");
    }
}

/// The wheel, over the code or the line itself, scrolls the code it's in and the line stays in sight — what's typed kept,
/// the keys its; never out of sight, however far.
#[test]
fn the_wheel_never_takes_the_line_out_of_sight() {
    let (_s, _path, mut ed) = project("c1-wheel");
    feed(&mut ed, "100Gzz nwords");
    let (x, y, ..) = ed.screen.note_card.get().unwrap();
    let top = ed.doc().top;
    for kind in [MouseKind::ScrollDown, MouseKind::ScrollUp, MouseKind::ScrollUp] {
        for _ in 0..30 {
            mouse(&mut ed, kind, x + 4, 2);
            assert!(ed.screen.prompt_input.get().is_some(), "in sight");
            assert!(ed.screen.note_input.get().is_some(), "in its card, never gone to the status line");
        }
    }
    assert_eq!(typed(&ed), "words");
    let d = ed.screen.prompt_input.get().unwrap();
    let moved = ed.doc().top;
    assert_ne!(moved, top, "it scrolled");
    mouse(&mut ed, MouseKind::ScrollDown, d.x + 2, d.y);
    assert_ne!(ed.doc().top, moved, "the wheel over the line scrolls the code too (top {moved}, row {y})");
}

/// `:` with a card open out of sight: the view stays where it is — only `space n`'s line brings it there.
#[test]
fn another_prompt_leaves_the_view_alone() {
    let (_s, path, mut ed) = project("c1-other-prompt");
    let id = noted(&mut ed, &path, 9, "A word about this line.");
    crate::notes::open(&mut ed, id);
    feed(&mut ed, "100G");
    let top = ed.doc().top;
    feed(&mut ed, ":");
    assert_eq!(ed.doc().top, top);
}

/// A pane without the keys, the file scrolled there so a screen's edge cuts the card under its line: the line's row isn't
/// drawn past the pane, nor taken for the prompt's.
#[test]
fn a_pane_cutting_the_card_draws_no_line_past_its_edge() {
    let (_s, _path, mut ed) = project("c1-cut");
    feed(&mut ed, "100G<C-w>v nwords");
    let focus = ed.workspace.viewer.focus;
    let (_, rect, ..) = *ed.screen.panes.iter().find(|p| p.0 != focus).unwrap();
    for top in 100usize.saturating_sub(rect.h + 3)..=100 {
        ed.workspace.views.iter_mut().find(|v| v.id != focus).unwrap().top = top;
        let shown = frame(&mut ed);
        let last = shown.lines().enumerate().filter(|(_, l)| l.contains("comment on line")).last();
        assert!(last.is_some_and(|(y, _)| y < rect.y + rect.h), "top {top}: {shown}");
        assert_eq!(typed(&ed), "words");
    }
    // (its line's row above that pane's first: the top inside the card, past the line)
    let other = ed.workspace.views.iter_mut().find(|v| v.id != focus).unwrap();
    (other.top, other.top_rows) = (99, Some((99, 3)));
    let shown = frame(&mut ed);
    assert_eq!(shown.matches("comment on line").count(), 1, "the focused pane's alone: {shown}");
}

/// A question opened with a card open elsewhere in the file: the question's is the card here, one at a time, the open one's
/// line end saying what it says with no card under it; `esc`, the open card back as it was, its line end the `¶` alone.
#[test]
fn a_question_stands_in_for_an_open_card_until_it_ends() {
    let (_s, path, mut ed) = project("c1-other");
    let id = noted(&mut ed, &path, 9, "A word about this line.");
    crate::notes::open(&mut ed, id);
    feed(&mut ed, "20G");
    let line_end = |ed: &mut Editor| {
        let shown = frame(ed);
        shown.lines().find(|l| l.contains("line 9 ")).expect("its line in sight").to_string()
    };
    assert!(!line_end(&mut ed).contains("A word"), "its card says it");
    feed(&mut ed, " n");
    let rows = note_card_rows(&ed);
    assert!(rows[0].contains("line 20") && rows.len() == 3, "{rows:#?}");
    assert!(line_end(&mut ed).contains("A word"), "no card under it now");
    feed(&mut ed, "<esc>");
    assert_eq!(ed.workspace.claude.notes.open, Some(id));
    assert!(ed.screen.note_card.get().is_some(), "its card back");
    assert!(!line_end(&mut ed).contains("A word"), "its card says it again");
}

/// Where no card can stand the line is the prompt it was, on the status line, its keys there: a buffer with no file, a
/// code too narrow for a card.
#[test]
fn where_no_card_can_stand_it_stands_on_the_status_line() {
    let (_s, _path, mut ed) = project("c1-no-file");
    ed.docs[0].path.set(None);
    frame(&mut ed);
    feed(&mut ed, " n");
    assert_eq!(ed.screen.note_card.get(), None, "no file, no notes");
    assert_eq!(ed.screen.note_input.get(), None);
    let (_, y, ..) = ed.screen.prompt_card.get().expect("its card");
    assert!(y > 30, "bottom left: row {y}");
    let shown = frame(&mut ed);
    assert!(shown.lines().last().unwrap_or("").contains("enter later"), "{shown}");
    let (_s, _path, mut ed) = project("c1-narrow");
    feed(&mut ed, " n");
    crate::term::render(&mut ed, &mut Vec::new(), 16, 24).unwrap();
    assert_eq!(ed.screen.note_input.get(), None, "too narrow");
    assert_eq!(ed.screen.note_card.get(), None, "too narrow");
    let (_, y, _, h) = ed.screen.prompt_card.get().expect("its card");
    assert!(h >= 3 && y > 15, "bottom left: {y}, {h}");
}

/// `w` columns wide: 5 of them the gutter and 1 the scrollbar (`project`'s 200 lines), 4 more the card's frame and padding.
fn narrow(ed: &mut Editor, w: u16) -> String {
    let mut buf = Vec::new();
    crate::term::render(ed, &mut buf, w, 24).unwrap();
    crate::screen::text(&buf, w, 24).unwrap()
}

/// A card holds the line only where its head and four cells fit inside its frame — never over its edge into what's beside
/// it: narrower, the line stands on the status line. `ask claude: ` needs 16 cells, a comment's head more.
#[test]
fn a_card_holds_the_line_only_where_its_head_fits() {
    let (_s, _path, mut ed) = project("c1-head");
    let bottom_left = |ed: &Editor, why: &str| {
        assert_eq!(ed.screen.note_input.get(), None, "{why}");
        assert_eq!(ed.screen.note_card.get(), None, "{why}");
        let (_, y, ..) = ed.screen.prompt_card.get().expect("its card");
        assert!(y > 15, "{why}: bottom left, row {y}");
    };
    let held = |ed: &Editor, shown: &str, why: &str| {
        let (cursor, y) = ed.screen.note_input.get().expect(why);
        let (x, _, w, _) = ed.screen.note_card.get().expect(why);
        let row = shown.lines().nth(y.into()).unwrap_or_default();
        assert!(usize::from(cursor) < x + w - 2, "{why}: the cursor at {cursor}, card {x}+{w}");
        assert_eq!(row.chars().nth(x + w - 1), Some('│'), "{why}: its frame's edge whole: {row}");
    };
    // A comment's head: `comment on line 1: ` and four
    crate::notes::start(&mut ed);
    ed.prompt.as_mut().unwrap().set("words that run on past the card");
    let shown = narrow(&mut ed, 33);
    held(&ed, &shown, "a comment's 23 cells");
    narrow(&mut ed, 32);
    bottom_left(&ed, "a comment's 22 cells");
}

/// A comment too wide for its card, an open card elsewhere: that card stays as it was — nothing stands in for it, the code
/// under it doesn't move, its line end stays the `¶` alone — and the line stands on the status line.
#[test]
fn a_comment_with_no_room_leaves_the_open_card_as_it_was() {
    let (_s, path, mut ed) = project("c1-question-narrow");
    let id = noted(&mut ed, &path, 9, "A word.");
    crate::notes::open(&mut ed, id);
    feed(&mut ed, "16G");
    // (one column under what its head needs: `a_card_holds_the_line_only_where_its_head_fits`)
    let before = narrow(&mut ed, 32);
    let top = ed.doc().top;
    let card = ed.screen.note_card.get().expect("the open card");
    crate::notes::start(&mut ed);
    let shown = narrow(&mut ed, 32);
    assert_eq!(ed.prompt.as_ref().map(|p| p.kind), Some(PromptKind::Note));
    assert_eq!(ed.screen.note_input.get(), None, "no room in a card");
    assert_eq!(ed.screen.note_card.get(), Some(card), "the open card as it was");
    assert_eq!(ed.doc().top, top, "the view as it was");
    let under: Vec<_> = shown.lines().zip(before.lines()).take(card.1 + card.3).collect();
    assert!(under.iter().all(|(a, b)| a == b), "what's above the prompt as it was:\n{shown}");
    let (_, y, ..) = ed.screen.prompt_card.get().expect("the prompt's card");
    assert!(y > 15, "bottom left, row {y}");
    feed(&mut ed, "<esc>");
    assert_eq!(ed.workspace.claude.notes.open, Some(id));
}

/// Two panes on a file: the card in each, the line the one with the keys' — its presses and its cursor — shown in both.
#[test]
fn two_panes_show_the_line_the_focused_one_holds_it() {
    let (_s, _path, mut ed) = project("c1-panes");
    feed(&mut ed, "5G<C-w>v");
    // (from either pane: whichever is drawn last)
    for _ in 0..2 {
        feed(&mut ed, " nwords");
        assert_eq!(frame(&mut ed).matches("comment on line 5: words").count(), 2, "in both");
        let focus = ed.workspace.viewer.focus;
        let (_, rect, ..) = *ed.screen.panes.iter().find(|p| p.0 == focus).unwrap();
        let (x, y) = ed.screen.note_input.get().unwrap();
        assert!((rect.x..rect.x + rect.w).contains(&(x as usize)), "the focused pane's: {x}, {rect:?}");
        assert_eq!(ed.screen.prompt_input.get().unwrap().y, y as usize);
        feed(&mut ed, "<esc><C-w>w");
    }
}

/// A press on the line puts its cursor, a drag selects — the prompt's `Input`, in the card; the card's words stay the card's.
#[test]
fn a_press_and_a_drag_on_the_cards_line_are_the_prompts() {
    let (_s, _path, mut ed) = project("c1-press");
    feed(&mut ed, "5G nhello there");
    let d = ed.screen.prompt_input.get().unwrap();
    press(&mut ed, d.x + 2, d.y);
    assert_eq!(ed.prompt.as_ref().unwrap().cursor(), 2, "on the character pressed");
    assert_eq!(ed.keys_to().region, crate::uistate::Region::Prompt, "the keys its");
    feed(&mut ed, "X");
    assert_eq!(typed(&ed), "heXllo there", "typed there");
    let d = ed.screen.prompt_input.get().unwrap();
    mouse(&mut ed, MouseKind::Down, d.x + 6, d.y);
    mouse(&mut ed, MouseKind::Drag, d.x + 8, d.y);
    mouse(&mut ed, MouseKind::Up, d.x + 8, d.y);
    let sel = ed.prompt.as_ref().unwrap().input.pane.doc().selection().primary();
    assert_eq!((sel.from(), sel.to()), (6, 9), "what's dragged over");
    assert_eq!(typed(&ed), "heXllo there", "nothing typed by it");
}

/// The card's keys press where the keys are, whatever the code's mode: `esc` closes, `enter` sends — never "esc first".
#[test]
fn the_cards_keys_are_the_lines_in_any_mode() {
    let (_s, _path, mut ed) = project("c1-keys");
    feed(&mut ed, "5Gv nhello");
    let keys = note_card_keys(&ed);
    let (_, (x, y)) = keys.iter().find(|k| k.0 == "esc").unwrap();
    press(&mut ed, *x, *y);
    assert!(ed.prompt.is_none(), "closed: {}", said(&ed));
    assert_eq!(ed.workspace.claude.notes.list.len(), 1, "nothing kept");
    feed(&mut ed, " nhello");
    let keys = note_card_keys(&ed);
    let (_, (x, y)) = keys.iter().find(|k| k.0 == "enter").unwrap();
    press(&mut ed, *x, *y);
    assert_eq!(ed.workspace.claude.notes.list.len(), 2, "kept: {}", said(&ed));
}

/// A press on the comment's frame is its own, what's typed kept; one outside the card closes it, as `esc`.
#[test]
fn a_press_on_the_card_keeps_the_line_and_one_outside_closes_it() {
    let (_s, _path, mut ed) = project("c1-off");
    feed(&mut ed, "5G nhello");
    let (x, y, ..) = ed.screen.note_card.get().unwrap();
    press(&mut ed, x + 20, y);
    assert_eq!(typed(&ed), "hello", "kept");
    assert_eq!(ed.keys_to().region, crate::uistate::Region::Prompt);
    press(&mut ed, x + 20, y + 12);
    assert!(ed.prompt.is_none() && ed.workspace.claude.notes.list.len() == 1, "closed, nothing kept");
}

/// The line in the card edits as the prompt does: `C-w` a word, never the window prefix; on nothing, said; its history none,
/// said; `backspace` on nothing closes it.
#[test]
fn the_cards_line_edits_as_the_prompt_does() {
    let (_s, _path, mut ed) = project("c1-edit");
    feed(&mut ed, "5G ntwo words<C-w>");
    assert_eq!(typed(&ed), "two ", "a word gone");
    feed(&mut ed, "<C-w>");
    assert_eq!(typed(&ed), "", "to its start");
    let panes = ed.screen.panes.len();
    feed(&mut ed, "<C-w>v");
    assert_eq!(ed.screen.panes.len(), panes, "never the window prefix");
    assert_eq!(typed(&ed), "v");
    feed(&mut ed, "<backspace><up>");
    assert!(said(&ed).contains("keeps no history"), "{}", said(&ed));
    feed(&mut ed, "<backspace>");
    assert!(ed.prompt.is_none(), "closed");
    assert_eq!(ed.screen.note_card.get(), None, "its card with it");
}
