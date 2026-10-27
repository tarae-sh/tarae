//! Landing F of the mouse audit (`.sb/mouse-plan.md` §4): a press gives the keys to the surface it lands on — the
//! code from the side panel, the rail and a focused card; the side panel and the rail off their rows; an edit's review
//! anywhere on it — and a prompt or picker open closes on a press outside it, nothing under it pressed.

use crate::editor::Editor;
use crate::event::{Mouse, MouseKind};
use crate::key::{Code, Key};
use crate::offer::{Offer, What};
use crate::uistate::Region;
use crate::workspace::At;

mod overlays;
mod resize;
mod select;
mod sequences;
mod walk;

const W: u16 = 160;
const H: u16 = 40;

fn editor() -> Editor {
    let mut ed = Editor::new(crate::config::Config::default());
    ed.docs[0].text = ropey::Rope::from_str(&(0..30).map(|i| format!("line {i}\n")).collect::<String>());
    frame(&mut ed);
    ed
}

fn frame(ed: &mut Editor) -> String {
    ed.focus_sync();
    crate::term::render(ed, &mut Vec::new(), W, H).unwrap();
    crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), W, H).unwrap()
}

/// Keys, a frame after each (`<C-q>`, `<esc>` as written).
fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let k: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(k);
        frame(ed);
    }
}

/// A press and its release at (`x`, `y`), a frame after.
fn press(ed: &mut Editor, x: usize, y: usize) {
    for kind in [MouseKind::Down, MouseKind::Up] {
        ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
    }
    frame(ed);
}

/// The cell of the code's line `line` (the one pane, from its top), a few columns into its text.
fn code_cell(ed: &Editor, line: usize) -> (usize, usize) {
    let (_, rect, gutter, _) = ed.screen.panes.first().expect("a code pane");
    (rect.x + gutter + 2, rect.y + line)
}

fn said(ed: &Editor) -> String {
    ed.messages.last().map(|m| m.2.clone()).unwrap_or_default()
}

/// A press in the code takes the keys from the side panel: the code's keys move the code (they moved the panel's
/// selection before, the press only placing the cursor).
#[test]
fn a_press_in_the_code_takes_the_keys_from_the_side_panel() {
    let mut ed = editor();
    let side = crate::sidebar::Sidebar::with_files(std::env::temp_dir(), vec!["a.rs".into(), "b.rs".into()]);
    ed.workspace.viewer.sidebar = Some(side);
    ed.workspace.viewer.at = At::Side;
    frame(&mut ed);
    assert!(matches!(ed.keys_to().region, Region::Side(_)));
    let (x, y) = code_cell(&ed, 4);
    press(&mut ed, x, y);
    assert_eq!(ed.keys_to().region, Region::Code, "the code has the keys");
    assert_eq!(ed.cursor_line(), 4, "the cursor where pressed");
    feed(&mut ed, "j");
    assert_eq!(ed.cursor_line(), 5, "j moves the code");
}

/// The wheel over the code scrolls the pane under the pointer and nothing else (the mouse rule): the keys stay where
/// they were (the side panel's here), the focus too, the cursor and the selection; the view stays where the wheel left
/// it, frame after frame, until the cursor moves — then it's back at the cursor. Over another split, that pane's view
/// scrolls, the focus stays.
#[test]
fn the_wheel_scrolls_the_view_and_nothing_else() {
    let mut ed = editor();
    ed.docs[0].text = ropey::Rope::from_str(&(0..200).map(|i| format!("line {i}\n")).collect::<String>());
    feed(&mut ed, "jjxx");
    let sel = ed.doc().selection().clone();
    assert!(!sel.primary().is_empty(), "a selection: {sel:?}");
    let side = crate::sidebar::Sidebar::with_files(std::env::temp_dir(), vec!["a.rs".into()]);
    ed.workspace.viewer.sidebar = Some(side);
    ed.workspace.viewer.at = At::Side;
    frame(&mut ed);
    let (x, y) = code_cell(&ed, 5);
    let wheel = |ed: &mut Editor, x: usize, y: usize| {
        ed.handle_mouse(Mouse {
            kind: MouseKind::ScrollDown,
            x: x as u16,
            y: y as u16,
            alt: false,
            ctrl: false,
        });
        frame(ed);
    };
    for _ in 0..4 {
        wheel(&mut ed, x, y);
    }
    frame(&mut ed);
    assert_eq!(ed.doc().top, 12, "the view scrolled, and stays");
    assert_eq!(ed.doc().selection(), &sel, "the cursor and the selection where they were");
    assert!(matches!(ed.keys_to().region, Region::Side(_)), "the keys still the side panel's");
    // A move brings the view back to the cursor
    ed.workspace.viewer.at = At::Code;
    feed(&mut ed, "k");
    assert!(ed.doc().top <= ed.cursor_line(), "back at the cursor: top {}", ed.doc().top);
    // Another split: its own view, the focus stays
    feed(&mut ed, "<C-w>v");
    let focus = ed.workspace.viewer.focus;
    let code = |id: crate::split::PaneId| ed.workspace.views.iter().any(|v| v.id == id);
    let (other, rect) = *ed.screen.leaves.iter().find(|l| l.0 != focus && code(l.0)).expect("the other pane");
    let before = ed.workspace.views.iter().find(|v| v.id == other).map(|v| v.top).unwrap();
    wheel(&mut ed, rect.x + rect.w / 2, rect.y + 4);
    assert_eq!(ed.workspace.viewer.focus, focus, "the focus where it was");
    let after = ed.workspace.views.iter().find(|v| v.id == other).map(|v| v.top).unwrap();
    assert_eq!(after, before + 3, "that pane's view scrolled");
}

/// A press on the side panel off its rows (under its last one) gives it the keys, as `space e` does — it wasn't
/// even said before.
#[test]
fn a_press_off_the_side_panels_rows_gives_it_the_keys() {
    let mut ed = editor();
    let side = crate::sidebar::Sidebar::with_files(std::env::temp_dir(), vec!["a.rs".into()]);
    ed.workspace.viewer.sidebar = Some(side);
    frame(&mut ed);
    assert_eq!(ed.keys_to().region, Region::Code);
    let last = ed.screen.sidebar.borrow().iter().map(|r| r.0).max().expect("its rows");
    assert!(ed.screen.side_w > 4);
    press(&mut ed, 3, last + 3);
    assert!(matches!(ed.keys_to().region, Region::Side(_)), "{:?}", ed.keys_to().region);
    // And the code's press takes them back
    let (x, y) = code_cell(&ed, 2);
    press(&mut ed, x, y);
    assert_eq!(ed.keys_to().region, Region::Code);
}

/// A code review of one changed file, its rail up.
fn code_review(ed: &mut Editor) {
    use crate::codereview::{Base, CodeReview, File};
    let root = ed.workspace.root.clone();
    let file = File {
        rel: "src/a.rs".into(),
        path: root.join("src/a.rs"),
        added: 1,
        removed: 0,
        new: false,
        gone: false,
        binary: false,
    };
    ed.workspace.code_review = Some(CodeReview::for_test(Base::Head, root, vec![file]));
    frame(ed);
    assert!(ed.screen.rail_w > 0 && !ed.screen.review_rail.borrow().is_empty(), "the rail drawn");
}

/// A press in the code during a code review takes the keys from its rail — a drag and `y` after it are the code's,
/// never the rail's.
#[test]
fn a_press_in_the_code_takes_the_keys_from_the_review_rail() {
    let mut ed = editor();
    code_review(&mut ed);
    crate::codereview::rail_focus(&mut ed);
    frame(&mut ed);
    assert_eq!(ed.keys_to().region, Region::Rail, "the rail had them");
    let (x, y) = code_cell(&ed, 3);
    press(&mut ed, x, y);
    assert_eq!(ed.keys_to().region, Region::Code, "{:?}", ed.workspace.viewer.at);
    assert_eq!(ed.cursor_line(), 3);
}

/// A press on the rail off its rows (under its last one, within its width) gives it the keys, as `space e` does —
/// it was silent; past its width the press is the code's.
#[test]
fn a_press_off_the_rails_rows_gives_it_the_keys() {
    let mut ed = editor();
    code_review(&mut ed);
    assert_eq!(ed.keys_to().region, Region::Code);
    let last = ed.screen.review_rail.borrow().iter().map(|r| r.0).max().expect("its rows");
    press(&mut ed, 2, last + 3);
    assert_eq!(ed.keys_to().region, Region::Rail, "its keys");
    let (x, y) = code_cell(&ed, 2);
    press(&mut ed, x, y);
    assert_eq!(ed.keys_to().region, Region::Code, "the code's again");
}

/// A focused card pressed outside lets go of the keys, as `esc` does — it waits, unanswered — and the press acts
/// where it landed: a letter typed after it is the code's, never the card's answer.
#[test]
fn a_press_outside_a_focused_card_gives_its_keys_back() {
    let mut ed = editor();
    let what =
        What::ReviewEnd { session: crate::event::WorkspaceId::HOME, started: std::time::Instant::now() };
    ed.push_offer(Offer::new("End the review?".into(), "2 files not viewed.", "", String::new(), what));
    // (a second behind it: the status line counts what waits — the one card in sight alone it leaves to the card)
    ed.push_offer(Offer::new("Review at the end?".into(), "?", "", String::new(), What::ReviewAtEnd));
    frame(&mut ed);
    feed(&mut ed, "<C-q>");
    assert!(ed.card_has_keys(), "focused");
    let (x, y) = code_cell(&ed, 2);
    press(&mut ed, x, y);
    assert!(!ed.card_has_keys() && ed.offers.len() == 2, "let go, still waiting");
    assert!(said(&ed).starts_with("keys back — the card waits"), "{}", said(&ed));
    assert_eq!(ed.cursor_line(), 2, "the press acted where it landed");
    feed(&mut ed, "n");
    assert_eq!(ed.offers.len(), 2, "n is the code's, not the card's answer");
    // A press on the card itself still focuses it
    let (cx, cy, ..) = ed.screen.offer_card.expect("drawn");
    press(&mut ed, cx + 2, cy + 1);
    assert!(ed.card_has_keys(), "a press on it focuses it");
    // What says its own lets go unsaid: the status line's `▲1 waiting`, a toast (its row picked, no word added first)
    let keys_back = |ed: &Editor| ed.messages.iter().filter(|m| m.2.starts_with("keys back")).count();
    let before = keys_back(&ed);
    let inbox =
        ed.screen.session_spots.borrow().iter().find(|s| s.3 == crate::workspace::Spot::Inbox).copied();
    let (y, x0, ..) = inbox.expect("the status line's waiting count");
    press(&mut ed, x0, y);
    assert_eq!(keys_back(&ed), before, "the inbox's press: unsaid");
    // (the cursor off the top rows: the toast takes the top right)
    feed(&mut ed, "<esc>jjjjjjjjjj");
    ed.set_status("the newest");
    frame(&mut ed);
    if !ed.card_has_keys() {
        feed(&mut ed, "<C-q>");
    }
    assert!(ed.card_has_keys());
    let newest = ed.toasts.iter().find(|t| t.text == "the newest").map(|t| t.id).expect("its toast");
    let row = ed.screen.toast_rows.borrow().iter().find(|r| r.1 == newest).map(|r| r.0).expect("its row");
    let (tx, ..) = ed.screen.toast_card.get().expect("a toast beside it");
    press(&mut ed, tx + 3, row);
    assert!(!ed.card_has_keys(), "let go");
    assert_eq!(keys_back(&ed), before, "the toast's press: unsaid");
    let at = ed.float_list_here(crate::pane::GroupKind::Messages);
    assert_eq!(at, Some(newest), "the message pressed is the one picked");
}

/// A press anywhere on an edit's review gives it the keys, as `C-q` does — after `esc` left it for the code; a press
/// on a change takes that one in hand too.
#[test]
fn a_press_on_an_edit_review_gives_it_the_keys() {
    let mut ed = editor();
    ed.docs[0].text = ropey::Rope::from_str("a\nb\nc\n");
    let doc = ed.doc().id;
    crate::llm::review_with(&mut ed, doc, "change it", vec![(0, 1, "a".into(), "x".into())], None);
    frame(&mut ed);
    assert_eq!(ed.keys_to().region, Region::EditReview);
    feed(&mut ed, "<esc>");
    assert_eq!(ed.keys_to().region, Region::Code, "left for the code, unanswered");
    // Off it — the status line, the top line over it (no split is drawn while it's up: it takes the code's whole
    // area) — the press goes where it lands, never to the review
    let (_, ry, ..) = ed.screen.review_area.get().expect("its area");
    for y in [H as usize - 1, ry.saturating_sub(1)] {
        press(&mut ed, 20, y);
        assert_eq!(ed.keys_to().region, Region::Code, "row {y}: not the review's");
        assert!(ed.workspace.claude.review.as_ref().is_some_and(|r| r.away));
    }
    let y = ed.screen.review_rows.borrow().first().map(|r| r.0).expect("its change's rows");
    press(&mut ed, 20, y);
    assert_eq!(ed.keys_to().region, Region::EditReview, "back in it by the press");
    assert!(ed.workspace.claude.review.as_ref().is_some_and(|r| !r.away));
}

/// A press on the `:` menu's row fills that completion in, as `tab` cycling to it does — the prompt stays, the code
/// under it untouched; a press outside the prompt closes it as `esc`, and presses nothing there.
#[test]
fn a_prompt_takes_its_press_and_a_press_outside_closes_it() {
    let mut ed = editor();
    feed(&mut ed, ":w");
    let rows = ed.screen.prompt_rows.borrow().clone();
    assert!(rows.len() >= 2, "a menu: {rows:?}");
    let (y, i) = rows[1];
    let comp = ed.cmdline_completion().expect("its completion");
    let want = crate::cmdline::apply("w", &comp, i);
    let before = ed.doc().selection().primary();
    press(&mut ed, 6, y);
    assert_eq!(ed.prompt.as_ref().map(|p| p.text()).as_deref(), Some(want.as_str()), "filled in");
    assert_eq!(ed.doc().selection().primary(), before, "the code untouched");
    let (x, y) = code_cell(&ed, 3);
    press(&mut ed, x, y);
    assert!(ed.prompt.is_none(), "closed");
    assert_eq!(ed.doc().selection().primary(), before, "no click-through");
    assert_eq!(ed.keys_to().region, Region::Code);
}

/// A themes picker on the theme the editor isn't in, that one previewed (as moving onto its row previews it).
fn previewing_a_theme(ed: &mut Editor, compact: bool) -> String {
    let original = ed.theme.name.clone();
    let other = if original == "hanji" { "meok" } else { "hanji" };
    let item = |n: &str| crate::picker::Item {
        label: n.into(),
        action: crate::picker::Action::Theme(n.into()),
        hint: String::new(),
        glyph: None,
    };
    let mut p = crate::picker::Picker::new("themes", vec![item(other), item(&original)], false);
    (p.compact, p.preview) = (compact, !compact);
    ed.open_picker(p, None);
    ed.preview_theme();
    let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the preview read");
    ed.handle_event(ev);
    assert_eq!(ed.theme.name, other, "previewed");
    frame(ed);
    original
}

/// Line 0 `f(aa, bb)` with an inlay hint before each argument (`f(x: aa, y: bb)` as drawn), and where its text
/// starts on screen.
fn with_two_hints() -> (Editor, usize, usize) {
    let mut ed = editor();
    ed.docs[0].text = ropey::Rope::from_str("f(aa, bb)\nline 1\n");
    let hint = |pos, text: &str| crate::lsp::InlayHint { pos, text: text.into() };
    ed.docs[0].lsp.inlay = vec![hint(2, "x: "), hint(6, "y: ")];
    frame(&mut ed);
    let (_, rect, gutter, _) = ed.screen.panes.first().expect("a code pane");
    let (x, y) = (rect.x + gutter, rect.y);
    (ed, x, y)
}

/// What the code's primary selection holds.
fn selected(ed: &Editor) -> String {
    let r = ed.doc().selection().primary();
    ed.doc().text.byte_slice(r.from()..r.to()).to_string()
}

/// A press and a drag across a line with two inlay hints select the characters under the pointer (frictions #150:
/// every hint before the pointer moved them that many cells right); a press on a hint lands on the character it stands
/// before.
#[test]
fn a_press_and_a_drag_past_inlay_hints_take_the_characters_under_the_pointer() {
    let (mut ed, x, y) = with_two_hints();
    let shown = frame(&mut ed);
    assert!(shown.lines().nth(y).is_some_and(|l| l.contains("f(x: aa, y: bb)")), "the hints drawn:\n{shown}");
    // (cells: `f(` 0-1, `x: ` 2-4, `aa` 5-6, `, ` 7-8, `y: ` 9-11, `bb` 12-13)
    ed.handle_mouse(Mouse { kind: MouseKind::Down, x: (x + 6) as u16, y: y as u16, alt: false, ctrl: false });
    assert_eq!(ed.doc().selection().primary().cursor(&ed.doc().text), 3, "the second a");
    ed.handle_mouse(Mouse {
        kind: MouseKind::Drag,
        x: (x + 12) as u16,
        y: y as u16,
        alt: false,
        ctrl: false,
    });
    assert_eq!(selected(&ed), "a, b", "from the second a through the first b");
    ed.handle_mouse(Mouse { kind: MouseKind::Up, x: (x + 12) as u16, y: y as u16, alt: false, ctrl: false });
    frame(&mut ed);
    // (on `y: `'s middle: the b after it)
    press(&mut ed, x + 10, y);
    assert_eq!(ed.doc().selection().primary().cursor(&ed.doc().text), 6, "the b the hint stands before");
}

/// The same with the hints off: the cells are the text's own again — nothing the last frame with hints recorded is
/// read past.
#[test]
fn with_inlay_hints_off_a_press_and_a_drag_take_the_texts_own_columns() {
    let (mut ed, x, y) = with_two_hints();
    ed.config.inlay_hints = false;
    let shown = frame(&mut ed);
    assert!(shown.lines().nth(y).is_some_and(|l| l.contains("f(aa, bb)")), "no hints:\n{shown}");
    // (cells: `f(` 0-1, `aa` 2-3, `, ` 4-5, `bb` 6-7)
    ed.handle_mouse(Mouse { kind: MouseKind::Down, x: (x + 3) as u16, y: y as u16, alt: false, ctrl: false });
    assert_eq!(ed.doc().selection().primary().cursor(&ed.doc().text), 3, "the second a");
    ed.handle_mouse(Mouse { kind: MouseKind::Drag, x: (x + 6) as u16, y: y as u16, alt: false, ctrl: false });
    assert_eq!(selected(&ed), "a, b", "from the second a through the first b");
}

/// A press and a drag at (`x`, `y`) — no release.
fn down_drag(ed: &mut Editor, (x, y): (usize, usize), to: usize) {
    for (kind, x) in [(MouseKind::Down, x), (MouseKind::Drag, to)] {
        ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
    }
}

/// Split side by side (`C-w v`), the same line with its hints in both panes: a press and a drag in the right pane take
/// the characters under the pointer there — the left pane's hints, drawn on the same screen row, aren't the right's.
#[test]
fn a_press_in_the_right_pane_skips_only_its_own_hints() {
    let (mut ed, ..) = with_two_hints();
    feed(&mut ed, "<C-w>v");
    let shown = frame(&mut ed);
    assert_eq!(ed.screen.panes.len(), 2, "split:\n{shown}");
    let (_, rect, gutter, _) = *ed.screen.panes.iter().max_by_key(|p| p.1.x).unwrap();
    let x = rect.x + gutter;
    let rows: Vec<Vec<char>> = shown.lines().map(|l| l.chars().collect()).collect();
    let at = |r: &Vec<char>| r.iter().skip(x).take(15).collect::<String>() == "f(x: aa, y: bb)";
    let y = rows.iter().position(at).unwrap_or_else(|| panic!("the right pane's hints drawn:\n{shown}"));
    assert!(rows[y][..x].iter().collect::<String>().contains("f(x: aa, y: bb)"), "the left's too:\n{shown}");
    down_drag(&mut ed, (x + 6, y), x + 12);
    assert_eq!(ed.workspace.viewer.focus, ed.screen.panes.iter().max_by_key(|p| p.1.x).unwrap().0);
    assert_eq!(selected(&ed), "a, b", "from the second a through the first b, in the right pane");
}

/// Split side by side (`C-w v`) and over and under (`C-w s`): a press in the second pane takes the line drawn under the
/// pointer — a pane at the column's top has its top line over its rect, one under another in its first row (a row was
/// taken off every split pane, so a press there went one line up, and none on its first).
#[test]
fn a_press_in_a_split_pane_takes_the_line_under_it() {
    for split in ["<C-w>v", "<C-w>s"] {
        let mut ed = editor();
        feed(&mut ed, split);
        let shown = frame(&mut ed);
        let (_, rect, gutter, _) = *ed.screen.panes.iter().max_by_key(|p| (p.1.y, p.1.x)).unwrap();
        let x = rect.x + gutter;
        let rows: Vec<String> = shown.lines().map(|l| l.chars().skip(x).take(6).collect()).collect();
        for line in [0, 2] {
            let y = (rect.y..).find(|&y| rows[y] == format!("line {line}")).expect("drawn");
            press(&mut ed, x + 2, y);
            assert_eq!(ed.cursor_line(), line, "{split}: the line under the pointer");
        }
    }
}

/// A hint after a line's last character (the last line, no newline after it: drawn at the line's end, not before a
/// character) is recorded where it's drawn as the others are; a press before it takes its character, one on it the
/// line's end.
#[test]
fn a_hint_at_the_lines_end_is_recorded_where_its_drawn() {
    let (mut ed, x, y) = with_two_hints();
    let text = "f(aa, bb)\nn = f(aa)";
    ed.docs[0].text = ropey::Rope::from_str(text);
    ed.docs[0].lsp.inlay = vec![crate::lsp::InlayHint { pos: text.len(), text: ": i32".into() }];
    let shown = frame(&mut ed);
    assert!(shown.lines().nth(y + 1).is_some_and(|l| l.contains("n = f(aa): i32")), "drawn:\n{shown}");
    assert_eq!(*ed.screen.virtual_cells.borrow(), [(x + 9, y + 1, 5)], "recorded where it's drawn");
    press(&mut ed, x + 7, y + 1);
    assert_eq!(ed.doc().selection().primary().cursor(&ed.doc().text), 10 + 7, "the second a");
    press(&mut ed, x + 11, y + 1);
    assert_eq!(ed.doc().selection().primary().cursor(&ed.doc().text), text.len(), "the line's end");
}

/// A press on an open picker's own frame — each edge, its preview, its rows — is its own; one outside it closes it as
/// `esc` does (a previewed theme put back), nothing under it pressed. The full picker with its preview and the compact
/// one alike.
#[test]
fn a_press_outside_a_picker_closes_it_and_one_on_its_frame_never() {
    for compact in [false, true] {
        let mut ed = editor();
        let original = previewing_a_theme(&mut ed, compact);
        // (its frame as the screen shows it — the one frame up — is what's recorded)
        let shown = frame(&mut ed);
        let rows: Vec<Vec<char>> = shown.lines().map(|l| l.chars().collect()).collect();
        let y = rows.iter().position(|r| r.contains(&'╭')).expect("its top line");
        let x = rows[y].iter().position(|&c| c == '╭').unwrap();
        let w = rows[y].iter().position(|&c| c == '╮').unwrap() + 1 - x;
        let h = rows.iter().skip(y).position(|r| r.get(x) == Some(&'╰')).expect("its bottom line") + 1;
        assert_eq!(ed.screen.picker_box.get(), Some((x, y, w, h)), "compact {compact}: recorded as drawn");
        // (its sides on the query's row, its preview on its title's: a list row pressed twice is a double-click)
        let mut on = vec![(x, y + 1), (x + w - 1, y + 1), (x + w / 2, y), (x + w / 2, y + h - 1)];
        if !compact {
            let (px, ..) = ed.screen.picker_preview.expect("its preview");
            assert!(px > x + w / 3, "the preview on the right");
            on.push((px + 4, y + 1));
        }
        let (top, _) = ed.screen.picker_list.expect("its list");
        on.push((x + 4, top + 1));
        let before = ed.doc().selection().primary();
        for (cx, cy) in on {
            press(&mut ed, cx, cy);
            assert!(ed.picker.is_some(), "compact {compact}: a press at {cx},{cy} on its frame closed it");
        }
        // (right under its frame — the status line where nothing is under it)
        let below = (y + h).min(H as usize - 1);
        press(&mut ed, x + w / 2, below);
        assert!(ed.picker.is_none(), "compact {compact}: closed");
        assert_eq!(ed.theme.name, original, "compact {compact}: the theme put back");
        assert_eq!(ed.doc().selection().primary(), before, "compact {compact}: no click-through");
        assert_eq!(ed.keys_to().region, Region::Code);
    }
}

fn mouse(ed: &mut Editor, kind: MouseKind, x: usize, y: usize) {
    ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
}

/// #140's fix, pinned: a press anywhere drops the code's last press — a press on a panel, a card or the terminal, then a
/// drag over the code, selects nothing from where the code was pressed before (its release lost on the status line,
/// where nothing ends it).
#[test]
fn a_drag_over_the_code_never_selects_from_its_press_before_another() {
    let side = |ed: &mut Editor| {
        let side = crate::sidebar::Sidebar::with_files(std::env::temp_dir(), vec!["a.rs".into()]);
        ed.workspace.viewer.sidebar = Some(side);
        frame(ed);
        let rows = ed.screen.sidebar.borrow().len();
        (1, ed.screen.text_top + rows + 4)
    };
    let card = |ed: &mut Editor| {
        let what =
            What::ReviewEnd { session: crate::event::WorkspaceId::HOME, started: std::time::Instant::now() };
        ed.push_offer(Offer::new("End the review?".into(), "2 files not viewed.", "", String::new(), what));
        frame(ed);
        let (x, y, ..) = ed.screen.offer_card.expect("the card drawn");
        (x + 2, y + 1)
    };
    let terminal = |ed: &mut Editor| {
        ed.terminal_run(vec!["/bin/sh".into(), "-c".into(), "sleep 30".into()], Vec::new());
        let end = std::time::Instant::now() + crate::test_results::PATIENCE;
        while ed.workspace.terminal.is_none() {
            assert!(std::time::Instant::now() < end, "the terminal started");
            if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
                ed.handle_event(ev);
            }
        }
        ed.workspace.terminal.as_mut().unwrap().visible = true;
        frame(ed);
        let (_, y1) = ed.screen.terminal_rows.expect("the terminal drawn");
        (ed.screen.slot_x + 4, y1 - 1)
    };
    let pty = !crate::sandbox::no_pty_here(
        "mouse::tests::a_drag_over_the_code_never_selects_from_its_press_before_another",
    );
    type Place = fn(&mut Editor) -> (usize, usize);
    let surfaces: [(&str, Place); 3] =
        [("the side panel", side), ("a card", card), ("the terminal", terminal)];
    for (name, at) in surfaces.into_iter().filter(|s| pty || s.0 != "the terminal") {
        let mut ed = editor();
        let (px, py) = at(&mut ed);
        let (x, y) = code_cell(&ed, 2);
        mouse(&mut ed, MouseKind::Down, x, y);
        mouse(&mut ed, MouseKind::Up, x, H as usize - 1);
        frame(&mut ed);
        let pressed = ed.doc().selection().clone();
        mouse(&mut ed, MouseKind::Down, px, py);
        frame(&mut ed);
        let (dx, dy) = code_cell(&ed, 6);
        mouse(&mut ed, MouseKind::Drag, dx, dy);
        frame(&mut ed);
        assert_eq!(ed.doc().selection(), &pressed, "{name}: nothing selected from the code's old press");
    }
}

/// A double-click on a waiting card's answer is one press — it focuses, never answers (found by the walk: the first
/// press focused, the second answered — a click meant for something else as the card came up answered it). An answer
/// takes a press of its own after that.
#[test]
fn a_double_click_on_a_waiting_cards_answer_only_focuses_it() {
    let mut ed = editor();
    let what =
        What::ReviewEnd { session: crate::event::WorkspaceId::HOME, started: std::time::Instant::now() };
    ed.push_offer(Offer::new("End the review?".into(), "2 files not viewed.", "", String::new(), what));
    frame(&mut ed);
    let n = |ed: &Editor| {
        let b = ed.screen.offer_buttons.iter().find(|b| b.3 == 'n').expect("its n drawn");
        (b.1, b.0)
    };
    crate::clock::hold();
    let (x, y) = n(&ed);
    press(&mut ed, x, y);
    assert!(ed.card_has_keys() && ed.offers.len() == 1, "one press: focused, not answered");
    let (x, y) = n(&ed);
    press(&mut ed, x, y);
    assert!(ed.card_has_keys() && ed.offers.len() == 1, "its double: still focused, nothing answered");
    crate::clock::advance(std::time::Duration::from_millis(500));
    let (x, y) = n(&ed);
    press(&mut ed, x, y);
    crate::clock::release();
    assert!(ed.offers.is_empty(), "a press of its own after: answered");
}

/// The prompt's line pressed where it was drawn (`Screen.prompt_input`, #153) puts its cursor on the character pressed —
/// what's under the pointer says its line (`mouse_under`), and the press acts there.
#[test]
fn a_press_on_the_prompts_drawn_line_puts_its_cursor_there() {
    let mut ed = editor();
    ed.open_prompt(crate::editor::PromptKind::Command, "write a.rs");
    frame(&mut ed);
    let d = ed.screen.prompt_input.get().expect("its line drawn");
    assert_eq!(d.first, 0, "the line whole in sight");
    let cursor = |ed: &Editor| ed.prompt.as_ref().map(|p| p.split().0.len());
    assert_eq!(cursor(&ed), Some("write a.rs".len()), "the cursor at its end as it opens");
    let (x, y) = (d.x + 2, d.y);
    assert!(
        matches!(
            ed.mouse_at(x, y),
            super::Under { surface: super::Surface::Prompt, part: super::Part::Text { .. } }
        ),
        "its line: {:?}",
        ed.mouse_at(x, y)
    );
    press(&mut ed, x, y);
    assert!(ed.prompt.is_some(), "still open");
    assert_eq!(cursor(&ed), Some(2), "the cursor on the third character, `i`");
}

/// Found by the walk (M0): a release, or a drag from a press elsewhere, over the code took the keys to the code — from
/// the side panel here, from `:messages` a toast's press opened, from a card's panel. Only a press there takes them.
#[test]
fn a_release_over_the_code_leaves_the_keys_where_the_press_gave_them() {
    let mut ed = editor();
    let side = crate::sidebar::Sidebar::with_files(std::env::temp_dir(), vec!["a.rs".into()]);
    ed.workspace.viewer.sidebar = Some(side);
    frame(&mut ed);
    let below = ed.screen.text_top + ed.screen.sidebar.borrow().len() + 4;
    mouse(&mut ed, MouseKind::Down, 1, below);
    frame(&mut ed);
    assert!(matches!(ed.keys_to().region, Region::Side(_)), "the press gave the side panel the keys");
    let (x, y) = code_cell(&ed, 3);
    mouse(&mut ed, MouseKind::Drag, x, y);
    mouse(&mut ed, MouseKind::Up, x, y);
    frame(&mut ed);
    assert!(matches!(ed.keys_to().region, Region::Side(_)), "still the side panel's");
}

/// Found by the walk (M0): a press on a code pane's top line (its path, its buffers) left the keys where they were —
/// the chat's here. It gives them to its pane's code, as a press on its text does.
#[test]
fn a_press_on_a_panes_top_line_gives_its_code_the_keys() {
    let mut ed = editor();
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.viewer.at = At::Chat;
    frame(&mut ed);
    assert_eq!(ed.keys_to().region, Region::Chat);
    let (_, rect, ..) = ed.screen.panes.first().expect("a code pane");
    let (x, y) = (rect.x + 4, rect.y - 1);
    assert!(matches!(ed.mouse_at(x, y).surface, super::Surface::PaneTop(_)), "its top line");
    let line = ed.cursor_line();
    press(&mut ed, x, y);
    assert_eq!(ed.keys_to().region, Region::Code, "the code has the keys");
    assert_eq!(ed.cursor_line(), line, "the cursor where it was");
}

/// A picker's preview at a list row's height is the preview's, never that row's (F found the press read the row and
/// not the column): the row in hand stays, the press says where a row is picked.
#[test]
fn a_press_on_a_pickers_preview_picks_no_row_and_says_so() {
    let mut ed = editor();
    let id = ed.doc().id;
    let item = |label: &str| crate::picker::Item {
        label: label.into(),
        action: crate::picker::Action::Buffer(id),
        hint: String::new(),
        glyph: None,
    };
    let items = vec![item("one"), item("two"), item("three")];
    ed.open_picker(crate::picker::Picker::of("buffers", "buffers", items, false), None);
    frame(&mut ed);
    let (top, rows) = ed.screen.picker_list.expect("its list");
    assert!(rows >= 3, "its rows: {rows}");
    let (px, ..) = ed.screen.picker_preview.expect("its preview");
    let label = |ed: &Editor| ed.picker.as_ref().and_then(|p| p.current()).map(|i| i.label.clone());
    let before = label(&ed);
    press(&mut ed, px + 4, top + 2);
    assert_eq!(label(&ed), before, "the row in hand stays");
    assert!(said(&ed).contains("a press on a row of the list picks it"), "said: {}", said(&ed));
    // (the list's own row: picked)
    let (lx, _) = ed.screen.picker_list_cols.expect("its columns");
    press(&mut ed, lx + 3, top + 2);
    assert_ne!(label(&ed), before, "a row of the list picked");
}
