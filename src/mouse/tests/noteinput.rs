//! Focus C1: `space n`'s line in the note's card, at the lines it's about — the card's last row between the lines, a new
//! question's card holding only it; the prompt's `Input`, keys and presses as ever; on the status line only where no card
//! can stand.

use super::overlays::{note_card_keys, note_card_rows};
use super::{editor, feed, frame, press, said};
use crate::editor::{Editor, PromptKind};
use crate::event::{Mouse, MouseKind};
use crate::notes::By;
use crate::scratch::Scratch;

/// A file of 200 lines (`line 0`, the first), in a folder of the test's.
fn project(name: &str) -> (Scratch, std::path::PathBuf, Editor) {
    let scratch = Scratch::new(name);
    let mut ed = editor();
    ed.config.llm.command = "sh".into();
    let path = scratch.join("a.rs");
    let text: String = (0..200).map(|i| format!("line {i}\n")).collect();
    ed.docs[0].text = ropey::Rope::from_str(&text);
    ed.docs[0].path.set(Some(path.clone()));
    frame(&mut ed);
    (scratch, path, ed)
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

/// `space n` on two lines, typing: a card under them holding only the line, named by them, its keys its bottom line — the
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
    assert!(rows[1].contains("ask claude: why this"), "{rows:#?}");
    assert!(rows[2].contains("enter send · esc close"), "{rows:#?}");
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
    assert_eq!(shown.matches("ask claude").count(), 1, "nothing bottom left: {shown}");
    let status = shown.lines().last().unwrap_or("");
    assert!(!status.contains("enter send"), "said once, on the card: {status}");
    let (bytes, cols, rows) = ed.last_frame.clone().unwrap();
    let mut p = vt100::Parser::new(rows, cols, 0);
    p.process(&bytes);
    assert_eq!(
        p.screen().cursor_position(),
        ((y + 1) as u16, (d.x + "why this".len()) as u16),
        "the cursor in it"
    );
    feed(&mut ed, "<esc>");
    assert!(ed.prompt.is_none() && ed.workspace.claude.notes.list.is_empty(), "nothing made");
    assert_eq!(ed.screen.note_card.get(), None, "its card gone");
    assert_eq!(frame(&mut ed), before, "the code as it was");
}

/// `enter`: what you typed stays where you typed it, the card a note's now, its line gone and its own keys back.
#[test]
fn sent_the_question_stays_where_it_was_typed() {
    let (_s, _path, mut ed) = project("c1-sent");
    feed(&mut ed, "4G nwhy this<ret>");
    assert!(ed.prompt.is_none(), "the line closed");
    let id = ed.workspace.claude.notes.list[0].id;
    assert_eq!(ed.workspace.claude.notes.open, Some(id), "its card open");
    let rows = note_card_rows(&ed);
    assert!(rows.iter().any(|r| r.contains("why this")), "{rows:#?}");
    assert!(!rows.iter().any(|r| r.contains("ask claude")), "{rows:#?}");
    assert_eq!(ed.screen.note_input.get(), None);
}

/// A review's comment goes as it does outside the card: a draft, its card not left open, so `space n` on the next line
/// is the next comment and never a reply to the last — its label in the card.
#[test]
fn a_reviews_comment_is_typed_there_and_leaves_no_card() {
    let (_s, path, mut ed) = project("c1-comment");
    crate::notes::add(&mut ed, &path, 100..101, By::You, "a draft");
    ed.workspace.claude.notes.list[0].thread[0].draft = true;
    feed(&mut ed, "5G na word");
    let rows = note_card_rows(&ed);
    assert!(rows[1].contains("comment on line 5: a word"), "{rows:#?}");
    feed(&mut ed, "<ret>");
    assert_eq!(ed.workspace.claude.notes.open, None, "no card left");
    feed(&mut ed, "9G n");
    assert!(matches!(ed.prompt.as_ref().map(|p| p.kind), Some(PromptKind::Note(None))), "the next comment");
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
    // Sent, what you wrote stays in sight, your cursor far above it, until a key moves that
    feed(&mut ed, " nwhy<ret>");
    let rows = note_card_rows(&ed);
    assert!(rows.iter().any(|r| r.contains("why")), "{rows:#?}");
    feed(&mut ed, "j");
    assert_eq!(ed.screen.note_card.get(), None, "the next move: the view at the cursor");
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
    assert!(ed.workspace.claude.notes.list.is_empty(), "nothing made");
    // A review's comment leaves no card: as `esc` too
    let path = ed.doc().path.clone().unwrap();
    crate::notes::add(&mut ed, &path, 150..151, By::You, "a draft");
    ed.workspace.claude.notes.list[0].thread[0].draft = true;
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

/// `space n` on an open card: its line under the newest entry, a blank row over it, the card's last row; lit — the keys are
/// there; to the mouse its row alone is the prompt's. The card grows by those rows, and shrinks back.
#[test]
fn a_reply_stands_under_the_newest_entry_of_its_card() {
    let (_s, path, mut ed) = project("c1-reply");
    let id = crate::notes::add(&mut ed, &path, 9..10, By::Author, "A word about this line.");
    ed.workspace.claude.notes.list[0].thread.push(crate::notes::Entry {
        by: By::You,
        text: "and the next?".into(),
        draft: false,
    });
    crate::notes::open(&mut ed, id);
    feed(&mut ed, "10G");
    let h = ed.screen.note_card.get().unwrap().3;
    feed(&mut ed, " nsure");
    let (x, y, w, grown) = ed.screen.note_card.get().unwrap();
    assert_eq!(grown, h + 2, "a blank row and the line");
    assert_eq!(ed.screen.prompt_card.get(), Some((x, y + grown - 2, w, 1)));
    let rows = note_card_rows(&ed);
    assert!(rows[grown - 2].contains("reply: sure"), "{rows:#?}");
    assert!(rows[grown - 3].trim_matches(['│', ' ']).is_empty(), "{rows:#?}");
    assert!(rows[grown - 4].contains("and the next?"), "under the newest: {rows:#?}");
    assert!(rows[grown - 1].contains("enter send · esc close"), "{rows:#?}");
    assert_eq!(note_card_keys(&ed).into_iter().map(|k| k.0).collect::<Vec<_>>(), ["enter", "esc"]);
    let lit = ed
        .screen
        .frames
        .borrow()
        .iter()
        .any(|f| f.of == Some(crate::mouse::Lights::Note) && f.lit != crate::mouse::Lit::Wash);
    assert!(lit, "its frame lit: it has the keys");
    feed(&mut ed, "<esc>");
    assert_eq!(ed.screen.note_card.get().unwrap().3, h, "as it was");
}

/// A reply on a card the wheel left out of sight: the card whole in sight with its line, not its foot alone.
#[test]
fn a_reply_brings_its_whole_card_into_sight() {
    let (_s, path, mut ed) = project("c1-far");
    let tall = "A paragraph of its own.\n\n".repeat(7);
    let id = crate::notes::add(&mut ed, &path, 9..10, By::Author, &tall);
    crate::notes::open(&mut ed, id);
    feed(&mut ed, "10G");
    for _ in 0..30 {
        mouse(&mut ed, MouseKind::ScrollDown, 20, 5);
    }
    assert_eq!(ed.screen.note_card.get(), None, "out of sight");
    let top = ed.doc().top;
    feed(&mut ed, " n");
    let rows = note_card_rows(&ed);
    assert!(rows[0].contains("╭─ note") && rows.iter().any(|r| r.contains("reply:")), "{rows:#?}");
    feed(&mut ed, "<esc>");
    assert_eq!(ed.doc().top, top, "`esc`: back where the wheel left it, its hold standing");
    assert_eq!(ed.screen.note_card.get(), None);
}

/// A card the wheel put in sight, your cursor far from it, pressed: the line opens on it, and neither `esc` nor what you
/// send takes the view to your cursor — the next key that moves it does, as after the wheel alone.
#[test]
fn a_pressed_card_stays_in_sight_as_its_line_ends() {
    let (_s, path, mut ed) = project("c1-press-view");
    let id = crate::notes::add(&mut ed, &path, 99..100, By::Author, "A word about this line.");
    crate::notes::open(&mut ed, id);
    feed(&mut ed, "10G");
    assert_eq!(ed.screen.note_card.get(), None, "out of sight");
    while !ed.screen.note_card.get().is_some_and(|c| c.1 < 20) {
        assert!(ed.doc().top < 120, "never in sight");
        mouse(&mut ed, MouseKind::ScrollDown, 20, 5);
    }
    let top = ed.doc().top;
    // (`at`: another cell each time, or it's the same press's double)
    let reply = |ed: &mut Editor, at: usize| {
        let (x, y, ..) = ed.screen.note_card.get().expect("the card in sight");
        press(ed, x + at, y + 1);
        assert_eq!(ed.prompt.as_ref().map(|p| p.kind), Some(PromptKind::Note(Some(id))));
    };
    reply(&mut ed, 6);
    feed(&mut ed, "<esc>");
    assert_eq!(ed.doc().top, top, "`esc`: the view as it was");
    assert!(ed.screen.note_card.get().is_some(), "the card still in sight");
    reply(&mut ed, 9);
    feed(&mut ed, "sure<ret>");
    assert_eq!(ed.doc().top, top, "sent: the view stays");
    let rows = note_card_rows(&ed);
    assert!(rows.iter().any(|r| r.contains("sure")), "what you wrote in sight: {rows:#?}");
    feed(&mut ed, "j");
    assert_eq!(ed.screen.note_card.get(), None, "the next move: the view at the cursor");
}

/// `:` with a card open out of sight: the view stays where it is — only `space n`'s line brings it there.
#[test]
fn another_prompt_leaves_the_view_alone() {
    let (_s, path, mut ed) = project("c1-other-prompt");
    let id = crate::notes::add(&mut ed, &path, 9..10, By::Author, "A word about this line.");
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
        let last = shown.lines().enumerate().filter(|(_, l)| l.contains("ask claude")).last();
        assert!(last.is_some_and(|(y, _)| y < rect.y + rect.h), "top {top}: {shown}");
        assert_eq!(typed(&ed), "words");
    }
    // (its line's row above that pane's first: the top inside the card, past the line)
    let other = ed.workspace.views.iter_mut().find(|v| v.id != focus).unwrap();
    (other.top, other.top_rows) = (99, Some((99, 3)));
    let shown = frame(&mut ed);
    assert_eq!(shown.matches("ask claude").count(), 1, "the focused pane's alone: {shown}");
}

/// `space n` on a note whose card is closed: the card holds the line, what the line end said left to it — and `esc` leaves
/// it closed, the line end as it was, no trace; sent, it stays.
#[test]
fn a_reply_shows_a_closed_card_and_esc_leaves_it_closed() {
    let (_s, path, mut ed) = project("c1-closed");
    let id = crate::notes::add(&mut ed, &path, 9..10, By::Author, "A word about this line.");
    feed(&mut ed, "10G");
    assert_eq!(ed.screen.note_card.get(), None);
    feed(&mut ed, " n");
    let (_, y, ..) = ed.screen.note_card.get().expect("its card");
    let rows = note_card_rows(&ed);
    assert!(
        rows.iter().any(|r| r.contains("A word about")) && rows.iter().any(|r| r.contains("reply:")),
        "{rows:#?}"
    );
    assert_eq!(ed.workspace.claude.notes.open, None, "nothing opened for good");
    let line = row(&mut ed, y - 1);
    assert!(line.contains("line 9") && !line.contains("A word"), "said once, on the card: {line}");
    feed(&mut ed, "<esc>");
    assert_eq!(ed.screen.note_card.get(), None, "closed as it was");
    let line = row(&mut ed, y - 1);
    assert!(line.contains("line 9") && line.contains("A word"), "its line end back: {line}");
    feed(&mut ed, " nsure<ret>");
    assert_eq!(ed.workspace.claude.notes.open, Some(id), "sent: it stays open");
}

/// A question opened with a card open elsewhere in the file: the question's is the card here, one at a time, the open one's
/// line end saying what it says with no card under it; `esc`, the open card back as it was, its line end the `¶` alone.
#[test]
fn a_question_stands_in_for_an_open_card_until_it_ends() {
    let (_s, path, mut ed) = project("c1-other");
    let id = crate::notes::add(&mut ed, &path, 9..10, By::Author, "A word about this line.");
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

/// `space n` on a line with no note, a sent question's card open on another: a question of its own on your line, never a
/// reply to that card, whose bottom line doesn't offer it — a key of yours acts at your cursor (#250). `esc`: that card as it
/// was. Sent: a note of its own, the first untouched. To reply there: its line. `]n`, `q` and `esc` still act on the open
/// card from anywhere.
#[test]
fn space_n_on_a_line_with_no_note_asks_there_whatever_card_is_open() {
    let (_s, _path, mut ed) = project("c1-yours");
    let kind = |ed: &Editor| ed.prompt.as_ref().map(|p| p.kind);
    feed(&mut ed, "4G nfirst<ret>");
    let first = ed.workspace.claude.notes.list[0].id;
    assert_eq!(ed.workspace.claude.notes.open, Some(first));
    assert!(note_card_rows(&ed).last().unwrap().contains("space n"), "on its line: its key");
    feed(&mut ed, "9G");
    let rows = note_card_rows(&ed);
    assert!(!rows.last().unwrap().contains("space n"), "elsewhere it isn't its key: {rows:#?}");
    feed(&mut ed, " n");
    assert_eq!(kind(&ed), Some(PromptKind::Note(None)), "a question");
    let rows = note_card_rows(&ed);
    assert!(rows[0].contains("line 9") && rows.len() == 3, "on your line: {rows:#?}");
    feed(&mut ed, "<esc>");
    assert_eq!(ed.workspace.claude.notes.open, Some(first), "it stays open");
    assert!(note_card_rows(&ed).iter().any(|r| r.contains("first")), "its card back");
    feed(&mut ed, " nsecond<ret>");
    let notes = &ed.workspace.claude.notes;
    assert_eq!(notes.list.len(), 2, "a note of its own");
    assert_eq!(notes.list[0].thread.len(), 1, "no reply on the first");
    assert_eq!(notes.list[1].lines, 8..9, "on your line");
    let second = notes.list[1].id;
    assert_eq!(notes.open, Some(second));
    // To reply on the first: its line
    feed(&mut ed, "4G n");
    assert_eq!(kind(&ed), Some(PromptKind::Note(Some(first))));
    // The open card's own keys, from a line with no note
    feed(&mut ed, "<esc>6G");
    assert_eq!(ed.workspace.claude.notes.open, Some(second));
    feed(&mut ed, "]n");
    assert_eq!(ed.workspace.claude.notes.open, Some(first), "`]n` on from the open card's, past line 9");
    feed(&mut ed, "30Gq");
    assert_eq!(ed.workspace.claude.notes.open, None, "`q` closes it from anywhere");
    feed(&mut ed, "]n30G");
    assert!(ed.workspace.claude.notes.open.is_some());
    feed(&mut ed, "<esc>");
    assert_eq!(ed.workspace.claude.notes.open, None, "`esc` too");
}

/// A note gone while its reply is typed: `submit` makes a new note at your lines, and the card says so as it's typed —
/// `ask claude:`, never `reply:` —, a review's comment in its own words.
#[test]
fn a_reply_whose_note_went_is_a_new_question_in_its_card() {
    let (_s, path, mut ed) = project("c1-gone");
    let id = crate::notes::add(&mut ed, &path, 9..10, By::Author, "A word about this line.");
    feed(&mut ed, "10G nsure");
    assert!(note_card_rows(&ed).iter().any(|r| r.contains("reply: sure")), "a reply while it's there");
    crate::notes::close(&mut ed, id);
    frame(&mut ed);
    let rows = note_card_rows(&ed);
    assert!(rows[0].contains("line 10") && rows.len() == 3, "{rows:#?}");
    assert!(rows[1].contains("ask claude: sure"), "what's typed kept, a question now: {rows:#?}");
    assert_eq!(ed.prompt.as_ref().map(|p| p.kind), Some(PromptKind::Note(None)));
    // In a review: a comment
    feed(&mut ed, "<esc>");
    let id = crate::notes::add(&mut ed, &path, 9..10, By::Author, "A word about this line.");
    crate::notes::add(&mut ed, &path, 100..101, By::You, "a draft");
    let draft = ed.workspace.claude.notes.list.last_mut().unwrap();
    draft.thread[0].draft = true;
    feed(&mut ed, " nsure");
    crate::notes::close(&mut ed, id);
    frame(&mut ed);
    let rows = note_card_rows(&ed);
    assert!(rows[1].contains("comment on line 10: sure"), "{rows:#?}");
}

/// Where no card can stand the line is the prompt it was, on the status line, its keys there: a buffer with no file, a
/// code too narrow for a card.
#[test]
fn where_no_card_can_stand_it_stands_on_the_status_line() {
    let mut ed = editor();
    feed(&mut ed, " n");
    assert_eq!(ed.screen.note_card.get(), None, "no file, no notes");
    assert_eq!(ed.screen.note_input.get(), None);
    let (_, y, ..) = ed.screen.prompt_card.get().expect("its card");
    assert!(y > 30, "bottom left: row {y}");
    let shown = frame(&mut ed);
    assert!(shown.lines().last().unwrap_or("").contains("enter send"), "{shown}");
    let (_s, _path, mut ed) = project("c1-narrow");
    feed(&mut ed, " n");
    crate::term::render(&mut ed, &mut Vec::new(), 16, 24).unwrap();
    assert_eq!(ed.screen.note_input.get(), None, "too narrow");
    assert_eq!(ed.screen.note_card.get(), None, "too narrow");
    let (_, y, _, h) = ed.screen.prompt_card.get().expect("its card");
    assert!(h >= 3 && y > 15, "bottom left: {y}, {h}");
}

/// Where no card can stand a note's line end goes on saying what no card says: a reply being typed, a card left open.
#[test]
fn a_reply_with_no_card_keeps_its_line_end() {
    let (_s, path, mut ed) = project("c1-narrow-reply");
    ed.docs[0].text = ropey::Rope::from_str("\n\n\n\n");
    let id = crate::notes::add(&mut ed, &path, 1..2, By::Author, "Aword");
    feed(&mut ed, "2G n");
    let (w, h) = (17, 24);
    let said = |ed: &mut Editor| {
        let mut buf = Vec::new();
        crate::term::render(ed, &mut buf, w, h).unwrap();
        assert_eq!(ed.screen.note_card.get(), None, "too narrow");
        let shown = crate::screen::text(&buf, w, h).unwrap();
        assert!(shown.contains("¶ Aw"), "{shown}");
    };
    said(&mut ed);
    feed(&mut ed, "<esc>");
    crate::notes::open(&mut ed, id);
    said(&mut ed);
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
    let (_s, path, mut ed) = project("c1-head");
    crate::notes::start(&mut ed);
    ed.prompt.as_mut().unwrap().set("words that run on past the card");
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
    let shown = narrow(&mut ed, 26);
    held(&ed, &shown, "16 cells");
    narrow(&mut ed, 25);
    bottom_left(&ed, "15 cells");
    feed(&mut ed, "<esc>");
    // A comment's head: `comment on line 1: ` and four
    crate::notes::add(&mut ed, &path, 100..101, By::You, "a draft");
    ed.workspace.claude.notes.list[0].thread[0].draft = true;
    crate::notes::start(&mut ed);
    let shown = narrow(&mut ed, 33);
    held(&ed, &shown, "a comment's 23 cells");
    narrow(&mut ed, 32);
    bottom_left(&ed, "a comment's 22 cells");
}

/// A reply where a card can stand but can't hold it: a closed card stays closed and an open one as it was, the line on the
/// status line, the view not going to a line that isn't there.
#[test]
fn a_card_too_narrow_for_its_reply_leaves_the_line_on_the_status_line() {
    let (_s, path, mut ed) = project("c1-reply-narrow");
    let id = crate::notes::add(&mut ed, &path, 9..10, By::Author, "A word.");
    feed(&mut ed, "10G");
    crate::notes::start(&mut ed);
    narrow(&mut ed, 21);
    assert!(ed.screen.note_input.get().is_some(), "`reply: ` and four cells fit: 11");
    feed(&mut ed, "<esc>");
    crate::notes::start(&mut ed);
    narrow(&mut ed, 20);
    assert_eq!(ed.screen.note_input.get(), None, "10 cells");
    assert_eq!(ed.screen.note_card.get(), None, "a closed card stays closed");
    feed(&mut ed, "<esc>");
    crate::notes::open(&mut ed, id);
    crate::notes::start(&mut ed);
    narrow(&mut ed, 20);
    assert_eq!(ed.screen.note_input.get(), None, "10 cells");
    let rows = note_card_rows(&ed);
    assert!(rows.iter().any(|r| r.contains("A word")), "the open card stays: {rows:#?}");
    assert!(!rows.iter().any(|r| r.contains("reply:")), "with no line in it: {rows:#?}");
    let (_, y, ..) = ed.screen.prompt_card.get().expect("the line's card");
    assert!(y > 15, "bottom left: row {y}");
    // An open card far below your cursor, under its note's end: a line in it would take the view there
    feed(&mut ed, "<esc>");
    crate::notes::close(&mut ed, id);
    let id = crate::notes::add(&mut ed, &path, 9..120, By::Author, "A word.");
    crate::notes::open(&mut ed, id);
    feed(&mut ed, "10G");
    crate::notes::start(&mut ed);
    narrow(&mut ed, 20);
    assert!(ed.doc().top < 20, "the view stays at your cursor: top {}", ed.doc().top);
}

/// A question too wide for its card, an open card elsewhere: that card stays as it was — nothing stands in for it, the code
/// under it doesn't move, its line end stays the `¶` alone — and the line stands on the status line. A comment's longer head
/// the same.
#[test]
fn a_question_with_no_room_leaves_the_open_card_as_it_was() {
    let (_s, path, mut ed) = project("c1-question-narrow");
    let id = crate::notes::add(&mut ed, &path, 9..10, By::Author, "A word.");
    crate::notes::open(&mut ed, id);
    feed(&mut ed, "16G");
    // (one column under what each head needs: `a_card_holds_the_line_only_where_its_head_fits`)
    for (w, why) in [(25, "a question"), (32, "a comment")] {
        let before = narrow(&mut ed, w);
        let top = ed.doc().top;
        let card = ed.screen.note_card.get().expect(why);
        crate::notes::start(&mut ed);
        let shown = narrow(&mut ed, w);
        assert_eq!(ed.prompt.as_ref().map(|p| p.kind), Some(PromptKind::Note(None)), "{why}");
        assert_eq!(ed.screen.note_input.get(), None, "{why}: no room in a card");
        assert_eq!(ed.screen.note_card.get(), Some(card), "{why}: the open card as it was");
        assert_eq!(ed.doc().top, top, "{why}: the view as it was");
        let under: Vec<_> = shown.lines().zip(before.lines()).take(card.1 + card.3).collect();
        assert!(under.iter().all(|(a, b)| a == b), "{why}: what's above the prompt as it was:\n{shown}");
        let (_, y, ..) = ed.screen.prompt_card.get().expect(why);
        assert!(y > 15, "{why}: bottom left, row {y}");
        feed(&mut ed, "<esc>");
        assert_eq!(ed.workspace.claude.notes.open, Some(id), "{why}");
        // (the comment's round: drafts waiting, so a new note is one)
        crate::notes::add(&mut ed, &path, 150..151, By::You, "a draft");
        ed.workspace.claude.notes.list.last_mut().unwrap().thread[0].draft = true;
    }
}

/// Two panes on a file: the card in each, the line the one with the keys' — its presses and its cursor — shown in both.
#[test]
fn two_panes_show_the_line_the_focused_one_holds_it() {
    let (_s, _path, mut ed) = project("c1-panes");
    feed(&mut ed, "5G<C-w>v");
    // (from either pane: whichever is drawn last)
    for _ in 0..2 {
        feed(&mut ed, " nwords");
        assert_eq!(frame(&mut ed).matches("ask claude: words").count(), 2, "in both");
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

/// A drag from a reply's card's words over its line stays the card's: copied as you let go, nothing held after it, so a
/// later drag isn't the card's — the line left as it was.
#[test]
fn a_drag_from_the_cards_words_onto_its_line_stays_the_cards() {
    let (_s, path, mut ed) = project("c1-drag-line");
    crate::notes::add(&mut ed, &path, 4..5, By::Author, "A word about this line.");
    feed(&mut ed, "5G nhello");
    let (x, y, ..) = ed.screen.note_card.get().unwrap();
    let d = ed.screen.prompt_input.get().unwrap();
    mouse(&mut ed, MouseKind::Down, x + 4, y + 1);
    mouse(&mut ed, MouseKind::Drag, x + 9, y + 1);
    mouse(&mut ed, MouseKind::Drag, d.x + 2, d.y);
    assert_eq!(ed.mouse_under(MouseKind::Up, d.x + 2, d.y).surface, crate::mouse::Surface::NoteCard);
    assert!(crate::term::note_marked_text(&ed).is_some(), "its words still marked");
    mouse(&mut ed, MouseKind::Up, d.x + 2, d.y);
    assert_eq!(ed.note_marked, None, "the gesture over: let go");
    assert_eq!(typed(&ed), "hello", "the line as it was");
    let sel = ed.prompt.as_ref().unwrap().input.pane.doc().selection().primary();
    assert_eq!(sel.from(), sel.to(), "nothing of it selected");
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
    assert!(ed.workspace.claude.notes.list.is_empty(), "nothing sent");
    feed(&mut ed, " nhello");
    let keys = note_card_keys(&ed);
    let (_, (x, y)) = keys.iter().find(|k| k.0 == "enter").unwrap();
    press(&mut ed, *x, *y);
    assert_eq!(ed.workspace.claude.notes.list.len(), 1, "sent: {}", said(&ed));
}

/// A press on the question's frame is its own, what's typed kept; one outside the card closes it, as `esc`. On a reply's card
/// a drag still selects the card's words, copied as you let go — the line left as it was; a press not dragged says it's open.
#[test]
fn a_press_on_the_card_keeps_the_line_and_one_outside_closes_it() {
    let (_s, path, mut ed) = project("c1-off");
    feed(&mut ed, "5G nhello");
    let (x, y, ..) = ed.screen.note_card.get().unwrap();
    press(&mut ed, x + 20, y);
    assert_eq!(typed(&ed), "hello", "kept");
    assert_eq!(ed.keys_to().region, crate::uistate::Region::Prompt);
    press(&mut ed, x + 20, y + 12);
    assert!(ed.prompt.is_none() && ed.workspace.claude.notes.list.is_empty(), "closed, nothing sent");
    crate::notes::add(&mut ed, &path, 4..5, By::Author, "A word about this line.");
    feed(&mut ed, " nhello");
    let (x, y, ..) = ed.screen.note_card.get().unwrap();
    press(&mut ed, x + 6, y + 2);
    assert_eq!(said(&ed), "replying — enter sends it · esc close");
    assert_eq!(typed(&ed), "hello", "the line as it was, never opened anew");
    mouse(&mut ed, MouseKind::Down, x + 4, y + 2);
    mouse(&mut ed, MouseKind::Drag, x + 9, y + 2);
    assert!(ed.note_marked.is_some_and(|(a, b)| a != b), "its words selected");
    mouse(&mut ed, MouseKind::Up, x + 9, y + 2);
    assert_eq!(typed(&ed), "hello", "the line as it was");
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
