//! Landing F of the mouse audit (`.sb/mouse-plan.md` §4): a press gives the keys to the surface it lands on — the
//! code from the side panel, the rail and a focused card; the side panel and the rail off their rows; an edit's review
//! anywhere on it — and a prompt or picker open closes on a press outside it, nothing under it pressed.

use crate::editor::Editor;
use crate::event::{Mouse, MouseKind};
use crate::key::{Code, Key};
use crate::offer::{Offer, What};
use crate::uistate::Region;
use crate::workspace::At;

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
    assert_eq!(ed.prompt.as_ref().map(|p| p.text.as_str()), Some(want.as_str()), "filled in");
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
