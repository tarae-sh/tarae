//! Landing R of the mouse audit (`.sb/mouse-plan.md` §4): every line between two things with sizes drags — a divider
//! over and under too, the review rail's edge (its width kept, `C-w < >` with its keys), a name on the slot's line as
//! the line — and a press on one without a drag gives the keys to what it heads (Q9). (Lit under the pointer, Q7:
//! `term_snapshots::a_line_that_drags_lights_under_the_pointer`, where colours are drawn.)

use super::{code_review, editor, feed, frame, press, said};
use crate::editor::Editor;
use crate::event::{Event, Mouse, MouseKind as K};
use crate::sizes::Border;
use crate::split::{Dir, PaneId, Rect};
use crate::uistate::Region;
use crate::workspace::At;

fn mouse(ed: &mut Editor, kind: K, x: usize, y: usize) {
    ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
    frame(ed);
}

/// A press at (`x`, `y`), dragged to (`x`, `to`) or (`to_x`, `y`), let go there.
fn drag(ed: &mut Editor, (x, y): (usize, usize), (to_x, to_y): (usize, usize)) {
    mouse(ed, K::Down, x, y);
    mouse(ed, K::Drag, to_x, to_y);
    mouse(ed, K::Up, to_x, to_y);
}

fn rect(ed: &Editor, id: PaneId) -> Rect {
    ed.screen.leaves.iter().find(|l| l.0 == id).expect("its leaf").1
}

/// The code's two panes, in order along the screen (left/top first).
fn two(ed: &Editor) -> (PaneId, PaneId) {
    let mut ids = ed.code_leaves();
    ids.sort_by_key(|&id| (rect(ed, id).y, rect(ed, id).x));
    (ids[0], ids[1])
}

/// Over and under, the lower pane's top line is the line between them: it drags the upper one's rows, said and kept in
/// the tree as `C-w - +` keep them; a press there without a drag gives the lower pane the keys, a drag never moves them.
#[test]
fn a_divider_over_and_under_drags_and_a_press_on_it_gives_its_pane_the_keys() {
    let mut ed = editor();
    feed(&mut ed, "<C-w>s");
    let (up, low) = two(&ed);
    let (a, b) = (rect(&ed, up), rect(&ed, low));
    assert_eq!(a.y + a.h, b.y, "the lower one's line right under the upper one");
    let (x, y) = (b.x + b.w / 2, b.y);
    assert_eq!(ed.border_at(x, y), Some(Border::Divider { sized: up, heads: low, dir: Dir::Horizontal }));
    assert_eq!(ed.border_at(x, y + 1), None, "its text under it: not the line");
    // A press, let go there: the keys to the pane whose line it is
    ed.focus_view(up);
    frame(&mut ed);
    press(&mut ed, x, y);
    assert_eq!((ed.keys_to().region, ed.workspace.viewer.focus), (Region::Code, low));
    // Dragged 4 rows up: the upper pane 4 shorter, the lower 4 taller — the keys stay where they were
    ed.focus_view(up);
    frame(&mut ed);
    drag(&mut ed, (x, y), (x, y - 4));
    assert_eq!((rect(&ed, up).h, rect(&ed, low).h), (a.h - 4, b.h + 4), "follows the mouse");
    assert_eq!(ed.workspace.viewer.focus, up, "a drag gives no keys");
    assert_eq!(said(&ed), format!("pane {} rows · C-w = automatic", a.h - 4));
    assert!(ed.workspace.split.sized(), "kept in the tree");
    feed(&mut ed, "<C-w>=");
    assert_eq!(rect(&ed, up).h, a.h, "even again");
}

/// Side by side, the divider is the right pane's left edge: a press on it without a drag gives that pane the keys.
#[test]
fn a_press_on_a_divider_side_by_side_gives_the_pane_right_of_it_the_keys() {
    let mut ed = editor();
    feed(&mut ed, "<C-w>v");
    let (left, right) = two(&ed);
    let l = rect(&ed, left);
    let (x, y) = (l.x + l.w, l.y + 3);
    assert_eq!(ed.border_at(x, y), Some(Border::Divider { sized: left, heads: right, dir: Dir::Vertical }));
    ed.focus_view(left);
    frame(&mut ed);
    press(&mut ed, x, y);
    assert_eq!(ed.workspace.viewer.focus, right);
    ed.focus_view(left);
    frame(&mut ed);
    drag(&mut ed, (x, y), (x - 6, y));
    assert_eq!(
        (rect(&ed, left).w, ed.workspace.viewer.focus),
        (l.w - 6, left),
        "sized, the keys where they were"
    );
}

/// The review's rail: its edge drags its width (kept: `sizes.json`'s `rail`), a press there gives it the keys, `C-w > <`
/// with its keys size it as the side panel's do, `C-w =` gives it back its rows' need.
#[test]
fn the_review_rails_edge_drags_and_its_keys_size_it() {
    let mut ed = editor();
    code_review(&mut ed);
    let w0 = ed.screen.rail_w;
    assert_eq!(ed.border_at(w0 - 1, 5), Some(Border::Rail), "its last column");
    assert_eq!(ed.keys_to().region, Region::Code);
    press(&mut ed, w0 - 1, 5);
    assert_eq!(ed.keys_to().region, Region::Rail, "a press there: the rail's keys");
    drag(&mut ed, (w0 - 1, 5), (w0 + 5, 5));
    assert_eq!((ed.screen.rail_w, ed.sizes.rail), (w0 + 6, Some(w0 as u16 + 6)), "follows the mouse, yours");
    assert_eq!(said(&ed), format!("review rail {} columns · C-w = automatic", w0 + 6));
    feed(&mut ed, "<C-w>>");
    assert_eq!(ed.screen.rail_w, w0 + 6 + crate::sizes::STEP_COLS, "C-w > with its keys");
    feed(&mut ed, "<C-w><lt>");
    assert_eq!(ed.screen.rail_w, w0 + 6);
    // From the code, `C-w >` isn't the rail's
    ed.workspace.viewer.at = At::Code;
    feed(&mut ed, "<C-w>>");
    assert_eq!(ed.screen.rail_w, w0 + 6, "the code's keys: not the rail's size");
    feed(&mut ed, "<C-w>=");
    assert_eq!((ed.screen.rail_w, ed.sizes.rail), (w0, None), "its rows' need again");
}

/// The chat's edge and the bottom slot's line: a press without a drag gives the chat and the slot the keys; a drag
/// sizes them and leaves the keys where they were. A name on the slot's line is the line too: dragged, the slot is sized.
#[test]
fn a_press_on_a_panels_line_gives_it_the_keys_and_a_name_there_drags_the_line() {
    let mut ed = editor();
    crate::uistate::tests::chat(&mut ed, "");
    let lines = (0..20).map(|i| format!("output line {i}")).collect();
    ed.workspace.test_run = Some(crate::testing::TestRun::finished("t", Vec::new(), lines));
    ed.workspace.viewer.at = At::Code;
    frame(&mut ed);
    let chat = super::W as usize - ed.screen.chat_w;
    assert_eq!(ed.border_at(chat, 10), Some(Border::Chat));
    press(&mut ed, chat, 10);
    assert_eq!(ed.keys_to().region, Region::Chat, "the chat's edge: the chat's keys");
    ed.workspace.viewer.at = At::Code;
    frame(&mut ed);
    let w = ed.screen.chat_w;
    drag(&mut ed, (chat, 10), (chat - 4, 10));
    assert_eq!(
        (ed.screen.chat_w, ed.keys_to().region),
        (w + 4, Region::Code),
        "sized, the keys where they were"
    );
    // The slot's line, off its names
    let s = &ed.screen;
    let line = s.text_top + s.body_h - s.bottom_h;
    let x =
        (s.slot_x..s.slot_end()).find(|&x| ed.border_at(x, line) == Some(Border::Bottom)).expect("its line");
    press(&mut ed, x, line);
    assert!(ed.bottom_has_keys(), "the slot's line: the slot's keys");
    // A tab's name, dragged up 3: the slot 3 taller; let go where it was pressed, nothing but the tab's act
    let (y, tab) = ed.screen.slot_tabs.borrow().first().map(|t| (t.0, t.1)).expect("a tab");
    let h = ed.screen.bottom_h;
    mouse(&mut ed, K::Down, tab, y);
    mouse(&mut ed, K::Up, tab, y);
    assert_eq!((ed.screen.bottom_h, ed.sizes.bottom, ed.dragging), (h, None, None), "a press is the tab's");
    drag(&mut ed, (tab, y), (tab, y - 3));
    assert_eq!((ed.screen.bottom_h, ed.sizes.bottom), (h + 3, Some(h as u16 + 3)), "dragged from its name");
}

/// A tab pressed on the slot's line (#216, `Editor::slot_press`): the one in front already gives the slot the keys, as
/// one behind does; with the keys there already they stay as they were, the tab in front; a docked group's panel gives
/// them to its own pane, never to the slot's other tab.
#[test]
fn a_press_on_the_tab_in_front_gives_the_slot_the_keys() {
    use crate::sizes::Slot;
    let two_tabs = || {
        let mut ed = editor();
        crate::uistate::tests::task_panel(&mut ed);
        let lines = (0..20).map(|i| format!("output line {i}")).collect();
        ed.workspace.test_run = Some(crate::testing::TestRun::finished("t", Vec::new(), lines));
        ed.workspace.viewer.at = At::Code;
        frame(&mut ed);
        ed
    };
    let front_tab = |ed: &Editor| {
        let front = ed.slot_shown().expect("a panel in front");
        let tabs = ed.screen.slot_tabs.borrow();
        (front, tabs.iter().find(|t| t.3 == front).map(|t| (t.1, t.0)).expect("its tab"))
    };
    // The tab in front, the keys in the code
    let mut ed = two_tabs();
    assert_eq!(ed.screen.slot_tabs.borrow().len(), 2, "two tabs");
    let (front, (x, y)) = front_tab(&ed);
    press(&mut ed, x, y);
    assert!(ed.bottom_has_keys(), "the slot's keys — {:?}", ed.workspace.viewer.at);
    assert_eq!(ed.slot_shown(), Some(front), "its tab still in front");
    // The keys there already: they stay
    let mut ed = two_tabs();
    ed.bottom_take_keys();
    frame(&mut ed);
    let at = ed.workspace.viewer.at;
    let (front, (x, y)) = front_tab(&ed);
    press(&mut ed, x, y);
    assert_eq!((ed.workspace.viewer.at, ed.slot_shown()), (at, Some(front)), "as they were");
    // A docked group: its own pane's keys, not the task's tab left in the slot
    let mut ed = editor();
    crate::uistate::tests::task_panel(&mut ed);
    crate::uistate::tests::tests_group_list(&mut ed);
    frame(&mut ed);
    ed.dock(crate::pane::GroupKind::Tests, crate::pane::Side::Right);
    ed.workspace.viewer.at = At::Code;
    frame(&mut ed);
    assert!(ed.slot_docked(Slot::Test), "docked");
    ed.slot_press(Slot::Test);
    frame(&mut ed);
    assert!(
        ed.slot_in_hand().is_some() && !ed.workspace.is_at(At::Slot),
        "the docked group's keys — {:?}",
        ed.workspace.viewer.at
    );
}

/// A line pressed whose release never came (a pane switch mid-press): the next press is its own — the code's cursor
/// where it landed, the line let go, nothing kept, said or given the keys.
#[test]
fn a_press_after_a_lost_release_is_its_own() {
    let mut ed = editor();
    crate::uistate::tests::chat(&mut ed, "");
    ed.workspace.viewer.at = At::Code;
    frame(&mut ed);
    let chat = super::W as usize - ed.screen.chat_w;
    let (w, said0) = (ed.screen.chat_w, said(&ed));
    mouse(&mut ed, K::Down, chat, 10);
    assert_eq!(ed.dragging, Some(Border::Chat), "held");
    let (x, y) = super::code_cell(&ed, 4);
    press(&mut ed, x, y);
    assert_eq!((ed.keys_to().region, ed.cursor_line()), (Region::Code, 4), "the press is the code's");
    assert_eq!((ed.dragging, ed.sizes.chat, ed.screen.chat_w), (None, None, w), "let go, unkept");
    assert_eq!(said(&ed), said0, "nothing said");
}

/// The wheel while a line is held lets it go, unkept: a drag after it sizes nothing.
#[test]
fn the_wheel_lets_go_of_a_held_line() {
    let mut ed = editor();
    crate::uistate::tests::chat(&mut ed, "");
    ed.workspace.viewer.at = At::Code;
    frame(&mut ed);
    let chat = super::W as usize - ed.screen.chat_w;
    let w = ed.screen.chat_w;
    mouse(&mut ed, K::Down, chat, 10);
    mouse(&mut ed, K::ScrollDown, chat, 10);
    assert_eq!(ed.dragging, None, "let go");
    mouse(&mut ed, K::Drag, chat - 6, 10);
    mouse(&mut ed, K::Up, chat - 6, 10);
    assert_eq!((ed.sizes.chat, ed.screen.chat_w), (None, w), "nothing sized");
}

/// A drag from a name on a line that drags nothing says why, once: a docked group's line (its edge sizes it), the
/// slot's while it fills the screen (`C-w z` first) — each its own reason.
#[test]
fn a_drag_from_a_name_on_a_line_nothing_drags_says_why_once() {
    let drag_name = |ed: &mut Editor, (x, y): (usize, usize)| {
        mouse(ed, K::Down, x, y);
        mouse(ed, K::Drag, x, y + 1);
        mouse(ed, K::Drag, x, y + 2);
        mouse(ed, K::Up, x, y + 2);
    };
    let times = |ed: &Editor, why: &str| ed.messages.iter().filter(|m| m.2 == why).count();
    // Docked right: its members' names on its own top line
    let mut ed = editor();
    crate::uistate::tests::task_panel(&mut ed);
    crate::uistate::tests::tests_group_list(&mut ed);
    frame(&mut ed);
    ed.dock(crate::pane::GroupKind::Tests, crate::pane::Side::Right);
    frame(&mut ed);
    let (r, _) = ed.dock_rect(crate::pane::GroupKind::Tests).map(|d| (d.1, d.0)).expect("docked");
    let name =
        ed.screen.member_names.borrow().iter().find(|n| n.1 >= r.x).map(|n| (n.1, n.0)).expect("a name");
    drag_name(&mut ed, name);
    assert_eq!(times(&ed, crate::editor::NAME_DOCKED), 1, "said, once");
    assert_eq!(times(&ed, crate::editor::NAME_ZOOMED), 0);
    // The slot filling the screen: its tab's name
    let mut ed = editor();
    let lines = (0..20).map(|i| format!("output line {i}")).collect();
    ed.workspace.test_run = Some(crate::testing::TestRun::finished("t", Vec::new(), lines));
    frame(&mut ed);
    ed.bottom_take_keys();
    feed(&mut ed, "<C-w>z");
    assert!(ed.zoomed().is_some(), "the slot fills the screen");
    let tab = ed.screen.slot_tabs.borrow().first().map(|t| (t.1, t.0)).expect("its tab");
    let h = ed.screen.bottom_h;
    drag_name(&mut ed, tab);
    assert_eq!(times(&ed, crate::editor::NAME_ZOOMED), 1, "said, once");
    assert_eq!(times(&ed, crate::editor::NAME_DOCKED), 0);
    assert_eq!((ed.sizes.bottom, ed.screen.bottom_h), (None, h), "nothing sized");
}

/// A name on a floating frame's top line moves the frame — the topmost first: over the slot's line it's the float's, not
/// the slot's height. Filling the screen (`C-w z` on it — nothing floats over a zoomed pane), it says its zoom's reason
/// and stays.
#[test]
fn a_name_on_a_floats_top_line_moves_the_float_over_what_is_under_it() {
    use crate::pane::GroupKind;
    let mut ed = editor();
    crate::uistate::tests::task_panel(&mut ed);
    crate::uistate::tests::tests_group_list(&mut ed);
    frame(&mut ed);
    ed.float(GroupKind::Tests);
    // (the slot tall enough that a whole float fits under its line)
    ed.sizes.bottom = Some(15);
    frame(&mut ed);
    let float =
        |ed: &Editor| ed.screen.floating.borrow().iter().find(|f| f.0 == GroupKind::Tests).map(|f| f.1);
    let name_on = |ed: &Editor, r: Rect| {
        let names = ed.screen.member_names.borrow();
        names
            .iter()
            .find(|n| n.0 == r.y && n.1 > r.x && n.1 < r.x + r.w)
            .map(|n| (n.1, n.0))
            .expect("its name")
    };
    // Its top line on the slot line's row
    let line = ed.screen.text_top + ed.screen.body_h - ed.screen.bottom_h;
    let r = float(&ed).expect("floating");
    ed.float_set(GroupKind::Tests, Rect { y: line, h: r.h.min(super::H as usize - line - 1), ..r });
    frame(&mut ed);
    let r = float(&ed).expect("floating");
    assert_eq!(r.y, line, "its top line on the slot's");
    let (h, (x, y)) = (ed.screen.bottom_h, name_on(&ed, r));
    drag(&mut ed, (x, y), (x, y - 3));
    assert_eq!(float(&ed).map(|f| f.y), Some(line - 3), "the float moved");
    assert_eq!((ed.screen.bottom_h, ed.sizes.bottom), (h, Some(15)), "the slot untouched");
    // The float filling the screen: its zoom's reason, said once, nothing moved
    ed.float_focus(GroupKind::Tests);
    feed(&mut ed, "<C-w>z");
    assert!(ed.zoomed().is_some(), "the float fills the screen");
    let r = float(&ed).expect("drawn, the screen its frame");
    let (x, y) = name_on(&ed, r);
    mouse(&mut ed, K::Down, x, y);
    mouse(&mut ed, K::Drag, x, y + 2);
    mouse(&mut ed, K::Drag, x, y + 3);
    mouse(&mut ed, K::Up, x, y + 3);
    assert_eq!(float(&ed), Some(r), "it stays");
    let said = ed.messages.iter().filter(|m| m.2 == crate::editor::NAME_ZOOMED).count();
    assert_eq!(said, 1, "its zoom's reason, once");
}

/// A floating group's frame on a screen of its own (the tests', a task in the slot) and the cell of its top line that
/// moves it, off its names.
fn floating(ed: &mut Editor) -> (Rect, (usize, usize)) {
    use crate::pane::GroupKind;
    crate::uistate::tests::task_panel(ed);
    crate::uistate::tests::tests_group_list(ed);
    frame(ed);
    ed.float(GroupKind::Tests);
    ed.workspace.viewer.at = At::Code;
    frame(ed);
    let r =
        ed.screen.floating.borrow().iter().find(|f| f.0 == GroupKind::Tests).map(|f| f.1).expect("floating");
    let moves = |x: usize| matches!(ed.float_grip(x, r.y), Some((_, crate::float::Grip::Move(..))));
    let x = (r.x + 1..r.x + r.w - 1).rev().find(|&x| moves(x)).expect("its top line, off its names");
    (r, (x, r.y))
}

/// A floating frame pressed whose release never came: the next press is its own — the code's cursor where it landed,
/// a drag from it selecting the code, the frame let go, nothing moved.
#[test]
fn a_press_after_a_lost_release_on_a_float_is_its_own() {
    let mut ed = editor();
    let (r, (x, y)) = floating(&mut ed);
    mouse(&mut ed, K::Down, x, y);
    assert!(ed.float_dragging.is_some(), "held");
    let (cx, cy) = super::code_cell(&ed, 2);
    let under = ed.mouse_at(cx, cy).surface;
    assert!(
        matches!(under, crate::mouse::Surface::Code(_)),
        "a press there is the code's, held or not: {under:?}"
    );
    mouse(&mut ed, K::Down, cx, cy);
    assert_eq!(ed.float_dragging, None, "let go at the press");
    mouse(&mut ed, K::Drag, cx + 3, cy);
    mouse(&mut ed, K::Up, cx + 3, cy);
    assert_eq!((ed.keys_to().region, ed.cursor_line()), (Region::Code, 2), "the press is the code's");
    let sel = ed.doc().selection().primary();
    assert!(sel.to() - sel.from() > 1, "the drag selected the code: {sel:?}");
    let now = ed.screen.floating.borrow().iter().find(|f| f.0 == crate::pane::GroupKind::Tests).map(|f| f.1);
    assert_eq!(now, Some(r), "nothing moved");
}

/// A click on a floating frame's top line gives it the keys and keeps nothing (no sizes file written); a drag of it
/// moves it and keeps where it is.
#[test]
fn a_click_on_a_floats_frame_keeps_nothing_a_drag_keeps_it() {
    let scratch = crate::scratch::Scratch::new("float-click");
    let mut ed = editor();
    ed.sizes_path = Some(scratch.join("sizes.json"));
    let (_, (x, y)) = floating(&mut ed);
    let seq = ed.sizes_seq;
    press(&mut ed, x, y);
    assert_eq!(ed.keys_to().region, Region::List(crate::pane::GroupKind::Tests), "the keys to it");
    assert_eq!(ed.sizes_seq, seq, "a click: nothing kept");
    drag(&mut ed, (x, y), (x - 3, y + 1));
    assert!(ed.sizes_seq > seq, "a drag: kept");
}

/// The pointer over a line that drags lights it as a drag does (`border_lit`); off it, it's let go — and a hover that
/// changes nothing draws nothing.
#[test]
fn a_line_under_the_pointer_is_lit_as_one_held() {
    let mut ed = editor();
    crate::uistate::tests::chat(&mut ed, "");
    ed.workspace.viewer.at = At::Code;
    frame(&mut ed);
    let chat = super::W as usize - ed.screen.chat_w;
    ed.handle_event(Event::Hover(chat as u16, 10));
    assert_eq!((ed.border_lit(), ed.unchanged), (Some(Border::Chat), false), "lit, drawn");
    ed.handle_event(Event::Hover(chat as u16, 11));
    assert!(ed.unchanged, "along it: nothing new to draw");
    ed.handle_event(Event::Hover(chat as u16 - 4, 11));
    assert_eq!((ed.border_lit(), ed.unchanged), (None, false), "off it: let go");
    // Under a picker nothing under it lights
    let id = ed.doc().id;
    let item = crate::picker::Item {
        label: "one".into(),
        action: crate::picker::Action::Buffer(id),
        hint: String::new(),
        glyph: None,
    };
    ed.open_picker(crate::picker::Picker::of("buffers", "buffers", vec![item], false), None);
    frame(&mut ed);
    ed.handle_event(Event::Hover(chat as u16, 10));
    assert_eq!(ed.border_lit(), None, "under a picker");
}
