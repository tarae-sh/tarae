use crate::config::Config;
use crate::editor::Editor;
use crate::key::{Code, Key};
use crate::pane::{GroupKind, Place, Shows, Side};
use crate::sizes::Slot;
use crate::workspace::At;

const W: u16 = 160;
const H: u16 = 40;

fn feed(ed: &mut Editor, keys: &str) {
    feed_in(ed, keys, W, H);
}

/// Keys, each with a frame `w` × `h` after it.
fn feed_in(ed: &mut Editor, keys: &str, w: u16, h: u16) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(key);
        frame_in(ed, w, h);
    }
}

/// A frame `w` wide, as the main loop draws one after an event.
fn frame(ed: &mut Editor, w: u16) {
    frame_in(ed, w, H);
}

fn frame_in(ed: &mut Editor, w: u16, h: u16) {
    ed.focus_sync();
    crate::term::render(ed, &mut Vec::new(), w, h).unwrap();
}

/// The frame as text.
fn shown(ed: &mut Editor, w: u16) -> String {
    frame(ed, w);
    crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), w, H).unwrap()
}

/// The code, a finished test run of two tests (one failed, at a place) and a finished task behind it in the slot, the
/// tests' list with the keys.
fn tests_and_a_task() -> (crate::scratch::Scratch, Editor) {
    use crate::test_results::{Case, Status};
    let dir = crate::scratch::Scratch::new("dock");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/lib.rs"), "a\nb\nc\nd\n").unwrap();
    let mut ed = Editor::new(Config::default());
    ed.workspace.root = dir.to_path_buf();
    ed.docs[0].text = ropey::Rope::from_str("fn main() {}\n");
    let t = crate::task::Task { name: "check".into(), cmd: "cargo check".into(), cwd: dir.to_path_buf() };
    let mut r = crate::task::Run::for_test(t);
    r.state = crate::task::State::Passed;
    ed.workspace.task_run = Some(r);
    let case = |name: &str, status, at: bool| Case {
        name: name.into(),
        status,
        secs: None,
        message: if at { vec!["left != right".into()] } else { Vec::new() },
        at: at.then(|| (dir.join("src/lib.rs"), 2)),
    };
    let cases = vec![case("adds", Status::Passed, false), case("subtracts", Status::Failed, true)];
    let mut run = crate::testing::TestRun::finished("math", cases, vec!["test result: FAILED".into()]);
    // (started after the task: the slot's front)
    run.started = crate::clock::now();
    ed.workspace.test_run = Some(run);
    frame(&mut ed, W);
    ed.list_focus(GroupKind::Tests);
    frame(&mut ed, W);
    (dir, ed)
}

/// A task group docked over the code (`C-w K`) has its line on the screen's first row: the mouse over its cut command
/// there has no room above for the run card (`term::run_card`) — none, nothing drawn past the screen's top, the frame
/// as it was; `K` in the panel still says the command whole.
#[test]
fn a_task_docked_at_the_top_has_no_run_card_over_its_line() {
    let dir = crate::scratch::Scratch::new("dock-card");
    let mut ed = Editor::new(Config::default());
    ed.workspace.root = dir.to_path_buf();
    ed.docs[0].text = ropey::Rope::from_str("fn main() {}\n");
    let cmd = "CARGO_TARGET_DIR=target/whole cargo clippy --all-targets -q 2>&1 | grep -E 'warning|error'";
    let t = crate::task::Task { name: cmd.into(), cmd: cmd.into(), cwd: dir.to_path_buf() };
    let mut r = crate::task::Run::for_test(t);
    r.state = crate::task::State::Passed;
    let a = dir.join("a.rs");
    for (line, severity) in [(0, 2), (2, 1)] {
        let it = crate::quickfix::Item {
            path: a.clone(),
            line,
            col: 0,
            message: "m".into(),
            severity,
            mark: None,
        };
        r.push_line(true, format!("{}:{}: m", a.display(), line + 1), false);
        r.place(it, None);
    }
    ed.workspace.task_run = Some(r);
    frame(&mut ed, W);
    ed.list_focus(GroupKind::Task);
    feed(&mut ed, "<C-w>K");
    assert_eq!(ed.workspace.group(GroupKind::Task).map(|g| g.place), Some(Place::Docked(Side::Top)));
    ed.workspace.viewer.at = At::Code;
    let before = shown(&mut ed, W);
    let spot =
        ed.screen.cut_spots.borrow().iter().find(|s| s.3 == crate::whole::Cut::Run(Slot::Task)).cloned();
    let (y, x) = spot.map(|s| (s.0, s.1)).expect("its cut label in the docked line");
    assert_eq!(y, 0, "its line the screen's first row");
    ed.handle_event(crate::event::Event::Hover(x as u16, y as u16));
    assert_eq!(shown(&mut ed, W), before, "no card: no room over the line");
}

/// A task group floating (`C-w e`): the mouse over its cut command on its frame's top line shows the run card over that
/// line, inside its frame's columns — placed by the panel whose line it is, not the slot's — and nothing else on the
/// frame changes.
#[test]
fn a_floating_tasks_run_card_is_over_its_own_line_inside_its_columns() {
    let dir = crate::scratch::Scratch::new("float-card");
    let mut ed = Editor::new(Config::default());
    ed.workspace.root = dir.to_path_buf();
    ed.docs[0].text = ropey::Rope::from_str(&"fn main() {}\n".repeat(40));
    let cmd = "CARGO_TARGET_DIR=target/whole cargo clippy --all-targets -q 2>&1 | grep -E 'warning|error' \
               | head -20 && echo farewell";
    let t = crate::task::Task { name: cmd.into(), cmd: cmd.into(), cwd: dir.to_path_buf() };
    let mut r = crate::task::Run::for_test(t);
    r.state = crate::task::State::Passed;
    let a = dir.join("a.rs");
    for (line, severity) in [(0, 2), (2, 1)] {
        let it = crate::quickfix::Item {
            path: a.clone(),
            line,
            col: 0,
            message: "m".into(),
            severity,
            mark: None,
        };
        r.push_line(true, format!("{}:{}: m", a.display(), line + 1), false);
        r.place(it, None);
    }
    ed.workspace.task_run = Some(r);
    frame(&mut ed, W);
    ed.list_focus(GroupKind::Task);
    feed(&mut ed, "<C-w>e");
    assert!(
        matches!(ed.workspace.group(GroupKind::Task).map(|g| g.place), Some(Place::Floating(_))),
        "floating"
    );
    ed.workspace.viewer.at = At::Code;
    let before = shown(&mut ed, W);
    let f =
        ed.screen.floating.borrow().iter().find(|f| f.0 == GroupKind::Task).map(|f| f.1).expect("floating");
    let spot =
        ed.screen.cut_spots.borrow().iter().find(|s| s.3 == crate::whole::Cut::Run(Slot::Task)).cloned();
    let (y, x) = spot.map(|s| (s.0, s.1)).expect("its cut label on its frame's line");
    assert_eq!(y, f.y, "its frame's top line");
    ed.handle_event(crate::event::Event::Hover(x as u16, y as u16));
    let screen = shown(&mut ed, W);
    let (was, now): (Vec<&str>, Vec<&str>) = (before.lines().collect(), screen.lines().collect());
    let top = (0..y)
        .find(|&i| now[i].contains("$ CARGO"))
        .unwrap_or_else(|| panic!("its card over the line:\n{screen}"));
    assert!((top..y).any(|i| now[i].contains("farewell")), "whole:\n{screen}");
    // (inside the frame's columns; every cell outside the card's rows and columns as it was)
    let cols = |l: &str, a: usize, b: usize| l.chars().skip(a).take(b - a).collect::<String>();
    for i in 0..was.len() {
        if (top..y).contains(&i) {
            let (l, r) = (f.x, f.x + f.w);
            assert_eq!(cols(was[i], 0, l), cols(now[i], 0, l), "row {i} left of the frame:\n{screen}");
            assert_eq!(
                cols(was[i], r, W as usize),
                cols(now[i], r, W as usize),
                "row {i} right of it:\n{screen}"
            );
            assert!(now[i].chars().skip(l).take(r - l).any(|c| c != ' '), "row {i}: the card");
        } else {
            assert_eq!(was[i], now[i], "row {i} changed:\n{screen}");
        }
    }
}

fn rect(ed: &Editor, id: crate::split::PaneId) -> crate::split::Rect {
    ed.screen.leaves.iter().find(|l| l.0 == id).map(|l| l.1).unwrap_or_default()
}

fn tests(ed: &Editor) -> crate::pane::Group {
    ed.workspace.group(GroupKind::Tests).unwrap()
}

/// P3 step 6 (§3.1–3.3): `C-w L` from the tests' list docks the group right of the code — a leaf of the tree of its own
/// (`group:tests`), left of the chat, the whole height; the slot shows the task, its tabs without the tests; the keys
/// stay in the list, which moves as it did, `tab` to its detail beside it. `C-w J` brings it back to the slot, in
/// front, the keys with the member that had them.
#[test]
fn c_w_l_docks_the_tests_group_and_c_w_j_brings_it_back() {
    let (_dir, mut ed) = tests_and_a_task();
    let g = tests(&ed);
    assert_eq!(ed.workspace.viewer.at, At::List(g.list));
    assert_eq!(ed.slot_shown(), Some(Slot::Test), "the tests in front");
    feed(&mut ed, "<C-w>L");
    let g = tests(&ed);
    assert_eq!(g.place, Place::Docked(Side::Right));
    assert_eq!(ed.workspace.shows(g.host), Some(Shows::Group(GroupKind::Tests)), "a leaf of its own");
    assert!(ed.workspace.split.holds(g.host));
    let (host, code) = (ed.dock_rect(GroupKind::Tests).unwrap().1, rect(&ed, ed.workspace.viewer.focus));
    assert!(
        host.x == code.x + code.w && host.x + host.w == W as usize,
        "right of the code: {host:?} {code:?}"
    );
    // (its members on screen in its place)
    let list = rect(&ed, g.list);
    assert!(
        list.w > 0 && list.x > code.x + code.w && list.y == 1,
        "its list beside the code, under its line: {list:?}"
    );
    assert_eq!(ed.slot_shown(), Some(Slot::Task), "the slot shows the task");
    assert!(!ed.slot_tabs().iter().any(|t| t.slot == Slot::Test), "the slot's tabs leave the tests out");
    assert_eq!(ed.workspace.viewer.at, At::List(g.list), "the keys stay in the list");
    assert_eq!(ed.keys_to().region, crate::uistate::Region::List(GroupKind::Tests));
    assert_eq!(ed.slot(), Some(Slot::Test), "the panel in hand");
    assert!(!ed.bottom_has_keys(), "not the slot's");
    feed(&mut ed, "j");
    assert_eq!(ed.workspace.test_run.as_ref().unwrap().selected, 1, "the list moves");
    feed(&mut ed, "<tab>");
    assert_eq!(
        (ed.workspace.viewer.at, ed.workspace.viewer.hand),
        (At::Slot, Some(GroupKind::Tests)),
        "tab: its detail"
    );
    assert_eq!(crate::rundoc::has_keys(&ed), Some(Slot::Test), "the tests' output has them, not the task's");
    assert_eq!(ed.at_pane(), Some(g.host));
    let text = shown(&mut ed, W);
    let top = text.lines().next().unwrap();
    assert!(top.contains("tests") && top.contains("results"), "its line at the top names it: {top}");
    let slot_line = text.lines().nth(ed.screen.text_top + ed.screen.body_h - ed.screen.bottom_h).unwrap();
    assert!(slot_line.contains("check") && !slot_line.contains("tests"), "the slot's line: {slot_line}");
    // (back: in the slot, in front — the task put in front of it meanwhile —, the detail with the keys)
    ed.slot_show(Slot::Task);
    ed.detail_focus(GroupKind::Tests);
    frame(&mut ed, W);
    assert_eq!(ed.slot_shown(), Some(Slot::Task));
    feed(&mut ed, "<C-w>J");
    let g = tests(&ed);
    assert_eq!(g.place, Place::Slot);
    assert_eq!(Some(g.host), ed.workspace.leaf(Shows::Slot));
    assert!(!ed.workspace.panes.iter().any(|p| p.shows == Shows::Group(GroupKind::Tests)), "its leaf gone");
    assert_eq!(ed.slot_shown(), Some(Slot::Test), "in front");
    assert_eq!(
        (ed.workspace.viewer.at, ed.workspace.viewer.hand, ed.slot()),
        (At::Slot, None, Some(Slot::Test))
    );
    feed(&mut ed, "<C-w>J");
    assert!(ed.toasts.iter().any(|t| t.text.starts_with("tests is in the slot")), "said");
}

/// `C-w H` docks left of the code (right of the side panel), `C-w K` over it in its column — the path bar under it;
/// from the code `C-w H J K L` still trade the code's panes; the terminal stays in the slot, said.
#[test]
fn c_w_h_and_k_dock_left_and_over_the_code() {
    let (_dir, mut ed) = tests_and_a_task();
    let root = ed.workspace.root.clone();
    ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::new(crate::sidebar::Mode::Files, root));
    ed.list_focus(GroupKind::Tests);
    feed(&mut ed, "<C-w>H");
    let g = tests(&ed);
    assert_eq!(g.place, Place::Docked(Side::Left));
    let (host, code, side) =
        (ed.dock_rect(GroupKind::Tests).unwrap().1, rect(&ed, ed.workspace.viewer.focus), ed.screen.side_w);
    assert!(
        host.x >= side && host.x + host.w <= code.x,
        "between the side panel and the code: {host:?} {code:?}"
    );
    feed(&mut ed, "<C-w>K");
    let g = tests(&ed);
    assert_eq!(g.place, Place::Docked(Side::Top));
    let (host, code) = (ed.dock_rect(GroupKind::Tests).unwrap().1, rect(&ed, ed.workspace.viewer.focus));
    assert!(
        host.y == 0 && host.h > 0 && code.y > host.y + host.h,
        "over the code, its path bar between: {host:?} {code:?}"
    );
    let text = shown(&mut ed, W);
    let bar = text.lines().nth(host.h).unwrap();
    assert!(bar.contains("tarae"), "the path bar under it: {bar}");
    // (from the code: the code's panes trade places, nothing docks)
    feed(&mut ed, "<esc><C-w>v<C-w>L");
    assert_eq!(tests(&ed).place, Place::Docked(Side::Top));
    assert_eq!(ed.workspace.views.len(), 2);
    // (the task in the slot: the task's group docks too; the terminal stays, said)
    ed.bottom_take_keys();
    feed(&mut ed, "<C-w>L");
    assert_eq!(ed.workspace.group(GroupKind::Task).map(|g| g.place), Some(Place::Docked(Side::Right)));
    assert!(ed.docked_in_sight(GroupKind::Task) && ed.docked_in_sight(GroupKind::Tests), "both docked");
    assert_eq!(ed.slot_shown(), None, "the slot has nothing left to show");
    let _sent = crate::terminal::stand_in(&mut ed, 10, 80, 100);
    frame(&mut ed, W);
    ed.bottom_take_keys();
    feed(&mut ed, "<C-w>L");
    assert!(ed.toasts.iter().any(|t| t.text.starts_with("the terminal stays in the slot")), "said");
}

/// §3.4: closed, a docked group's leaf goes (the code takes the room) and its place is kept — the next run of its kind
/// lands there, docked, without the keys; `C-w q` from it closes it as `q` does, and on the code closes a code pane,
/// the docked group staying.
#[test]
fn a_new_run_lands_where_its_kind_is_docked() {
    let (_dir, mut ed) = tests_and_a_task();
    feed(&mut ed, "<C-w>L");
    let host = tests(&ed).host;
    feed(&mut ed, "q");
    assert!(ed.workspace.test_run.is_none(), "closed");
    assert_eq!(ed.workspace.viewer.at, At::Code);
    let g = tests(&ed);
    assert_eq!(g.place, Place::Docked(Side::Right), "its place kept");
    assert!(!ed.workspace.split.holds(host), "its leaf gone");
    assert_eq!(Some(g.host), ed.workspace.leaf(Shows::Slot), "its host the slot's meanwhile");
    let code = rect(&ed, ed.workspace.viewer.focus);
    // (a new run: docked again, the keys where they were)
    let run = crate::testing::TestRun::finished("math", Vec::new(), vec!["running".into()]);
    ed.workspace.test_run = Some(run);
    frame(&mut ed, W);
    assert!(ed.docked_in_sight(GroupKind::Tests), "landed docked");
    assert!(
        ed.dock_rect(GroupKind::Tests).is_some() && rect(&ed, ed.workspace.viewer.focus).w < code.w,
        "beside the code"
    );
    assert_eq!(ed.slot_shown(), Some(Slot::Task));
    assert_eq!(ed.workspace.viewer.at, At::Code);
    // (its output alone has the keys by a click on it; `C-w q` there closes it)
    ed.panel_take_keys(Slot::Test);
    frame(&mut ed, W);
    assert_eq!((ed.workspace.viewer.at, ed.slot()), (At::Slot, Some(Slot::Test)));
    // (on the code: a code pane closes, the docked group stays)
    feed(&mut ed, "<esc><C-w>v<C-w>q");
    assert_eq!(ed.workspace.views.len(), 1);
    assert!(ed.docked_in_sight(GroupKind::Tests), "the docked group stays");
    feed(&mut ed, "<C-w>q");
    assert!(ed.toasts.iter().any(|t| t.text.starts_with("the code keeps one pane")), "said");
    assert!(ed.docked_in_sight(GroupKind::Tests));
    ed.panel_take_keys(Slot::Test);
    feed(&mut ed, "<C-w>q");
    assert!(ed.workspace.test_run.is_none(), "C-w q closed it, as q");
}

/// §3.7: too narrow for the code beside it, a docked group folds back into the slot's tabs for as long — its place
/// kept, its keys the slot's there (the task's tab picked meanwhile: the folded group in front, its list keeping the
/// keys); wider, it's beside the code again.
#[test]
fn a_docked_group_folds_into_the_slot_when_narrow() {
    let (_dir, mut ed) = tests_and_a_task();
    feed(&mut ed, "<C-w>L");
    assert!(ed.docked_in_sight(GroupKind::Tests));
    ed.slot_show(Slot::Task);
    frame(&mut ed, W);
    let list = At::List(tests(&ed).list);
    assert_eq!((ed.workspace.viewer.slot_front, ed.workspace.viewer.at), (Some(Slot::Task), list));
    // (folded by an event: the side panel and the chat open leave the code too little beside it)
    let root = ed.workspace.root.clone();
    ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::new(crate::sidebar::Mode::Files, root));
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    frame(&mut ed, W);
    assert!(!ed.docked_in_sight(GroupKind::Tests), "folded");
    assert_eq!(
        (ed.workspace.viewer.slot_front, ed.workspace.viewer.at),
        (Some(Slot::Test), list),
        "in front, the keys in it"
    );
    assert!(ed.bottom_has_keys());
    (ed.workspace.viewer.sidebar, ed.workspace.claude.chat) = (None, None);
    frame(&mut ed, W);
    assert!(ed.docked_in_sight(GroupKind::Tests), "docked again");
    // (folded by a narrower frame)
    ed.slot_show(Slot::Task);
    frame(&mut ed, W);
    frame(&mut ed, 60);
    assert!(!ed.docked_in_sight(GroupKind::Tests), "60 columns: folded");
    assert_eq!(tests(&ed).place, Place::Docked(Side::Right), "not moved");
    assert!(ed.slot_tabs().iter().any(|t| t.slot == Slot::Test), "a tab of the slot again");
    assert_eq!(ed.workspace.viewer.at, At::List(tests(&ed).list), "the keys with it, in the slot");
    assert!(ed.bottom_has_keys());
    let text = shown(&mut ed, 60);
    assert!(text.contains("tests"), "in the slot's line");
    feed(&mut ed, "j");
    assert_eq!(ed.workspace.test_run.as_ref().unwrap().selected, 1, "its keys there");
    frame(&mut ed, 80);
    assert!(!ed.docked_in_sight(GroupKind::Tests), "80: still folded");
    frame(&mut ed, W);
    assert!(ed.docked_in_sight(GroupKind::Tests), "wide: docked again");
    assert_eq!(
        (ed.workspace.viewer.at, ed.workspace.viewer.hand),
        (list, Some(GroupKind::Tests)),
        "in hand again"
    );
    assert!(!ed.slot_tabs().iter().any(|t| t.slot == Slot::Test));
}

/// Folded into the slot with its detail in hand, the keys are the slot's there — not its docked leaf's, which has no
/// room: `C-w l` with nothing that way stays (said), `C-w w` goes on from the detail, the state's `at` is the detail;
/// wide again, its own leaf's.
#[test]
fn the_window_keys_hop_from_a_folded_groups_output() {
    let (_dir, mut ed) = tests_and_a_task();
    feed(&mut ed, "<C-w>L<tab>");
    let g = tests(&ed);
    assert_eq!(
        (ed.workspace.viewer.at, ed.workspace.viewer.hand, ed.at_pane()),
        (At::Slot, Some(GroupKind::Tests), Some(g.host))
    );
    frame(&mut ed, 60);
    assert!(!ed.docked_in_sight(GroupKind::Tests), "folded");
    let slot = ed.workspace.leaf(Shows::Slot);
    assert_eq!((ed.workspace.viewer.at, ed.at_pane()), (At::Slot, slot), "the slot's");
    let detail = g.detail_pane().unwrap();
    let state = ed.ui_state().json();
    let panes = state["workspaces"][0]["panes"].as_array().cloned().unwrap_or_default();
    let at = panes.iter().find(|p| p["at"] == true).expect("a pane with the keys");
    let slot_or_detail = [format!("pane#{}", slot.unwrap()), format!("pane#{detail}")];
    assert!(
        slot_or_detail.iter().any(|id| at["id"] == *id),
        "the state's at: the slot's, not its host's — {at}"
    );
    feed_in(&mut ed, "<C-w>l", 60, H);
    assert_eq!(ed.workspace.viewer.at, At::Slot, "nothing right of it: the keys stay");
    assert!(ed.toasts.iter().any(|t| t.text.starts_with("nothing")), "said");
    let order: Vec<_> = ed.screen.leaves.iter().map(|l| l.0).collect();
    let i = order.iter().position(|&id| id == detail).expect("its detail laid out in the slot");
    let next = order[(i + 1) % order.len()];
    feed_in(&mut ed, "<C-w>w", 60, H);
    let landed = if ed.workspace.is_at(At::Code) { Some(ed.workspace.viewer.focus) } else { ed.at_pane() };
    assert_eq!(landed, Some(next), "C-w w: the pane after its detail");
    // (wide again, its detail in hand: its own leaf's)
    ed.detail_focus(GroupKind::Tests);
    frame(&mut ed, W);
    assert!(ed.docked_in_sight(GroupKind::Tests));
    assert_eq!((ed.workspace.viewer.hand, ed.at_pane()), (Some(GroupKind::Tests), Some(g.host)));
}

/// Docked over the code on a short screen — the task in the slot and the code's least leave it too few rows — it folds
/// into the slot's tabs, in front, its list keeping the keys; tall again, over the code. A tall slot size of yours
/// folds it as well.
#[test]
fn a_group_docked_over_the_code_folds_on_a_short_screen() {
    let (_dir, mut ed) = tests_and_a_task();
    feed(&mut ed, "<C-w>K");
    let g = tests(&ed);
    assert_eq!((g.place, ed.workspace.viewer.at), (Place::Docked(Side::Top), At::List(g.list)));
    assert!(ed.docked_in_sight(GroupKind::Tests));
    frame_in(&mut ed, W, 12);
    let s = &ed.screen;
    let said = format!("{} {} {:?}", s.body_h, s.bottom_h, ed.dock_rect(GroupKind::Tests));
    assert!(!ed.docked_in_sight(GroupKind::Tests), "12 rows: folded — {said}");
    assert_eq!(tests(&ed).place, Place::Docked(Side::Top), "not moved");
    assert!(ed.slot_tabs().iter().any(|t| t.slot == Slot::Test), "a tab of the slot");
    assert_eq!(
        (ed.slot_shown(), ed.workspace.viewer.at),
        (Some(Slot::Test), At::List(g.list)),
        "in front, the keys in it"
    );
    assert!(ed.bottom_has_keys());
    assert!(rect(&ed, g.list).h > 0, "its list drawn in the slot");
    feed_in(&mut ed, "j", W, 12);
    assert_eq!(ed.workspace.test_run.as_ref().unwrap().selected, 1, "its keys there");
    frame_in(&mut ed, W, H);
    assert!(ed.docked_in_sight(GroupKind::Tests), "tall: over the code again");
    assert_eq!(
        (ed.workspace.viewer.at, ed.workspace.viewer.hand),
        (At::List(g.list), Some(GroupKind::Tests)),
        "in hand again"
    );
    assert!(ed.dock_rect(GroupKind::Tests).is_some_and(|d| d.1.h >= crate::dock::DOCK_MIN_ROWS));
    // (a slot size of yours that leaves it too few)
    ed.sizes.bottom = Some(H - 6);
    frame(&mut ed, W);
    assert!(!ed.docked_in_sight(GroupKind::Tests), "a tall slot: folded");
    ed.sizes.bottom = None;
    frame(&mut ed, W);
    assert!(ed.docked_in_sight(GroupKind::Tests), "back");
}

/// Its size is its kind's own (`Sizes.docked`): `C-w > <` from a member, `C-w - +` said (it's beside the code), a drag
/// of its edge; `C-w =` makes it automatic again; within what leaves the code `CODE_MIN`.
#[test]
fn a_docked_groups_size_is_its_kinds_own() {
    let (_dir, mut ed) = tests_and_a_task();
    feed(&mut ed, "<C-w>L");
    let width = |ed: &Editor| ed.dock_rect(GroupKind::Tests).map_or(0, |d| d.1.w);
    let auto = width(&ed);
    feed(&mut ed, "<C-w>>");
    let wider = width(&ed);
    assert_eq!((wider, ed.sizes.docked[0]), (auto + 4, Some((auto + 4) as u16)));
    feed(&mut ed, "<C-w>+");
    assert!(ed.toasts.iter().any(|t| t.text.contains("C-w < > size it")), "said");
    // (dragged: its edge to column 100)
    let r = ed.dock_rect(GroupKind::Tests).unwrap().1;
    assert_eq!(ed.border_at(r.x, 5), Some(crate::sizes::Border::Leaf(GroupKind::Tests)), "its edge");
    ed.border_drag(crate::sizes::Border::Leaf(GroupKind::Tests), 100, 5);
    frame(&mut ed, W);
    assert_eq!(ed.dock_rect(GroupKind::Tests).unwrap().1.x, 100, "its edge where it was let go");
    ed.border_drag(crate::sizes::Border::Leaf(GroupKind::Tests), 2, 5);
    frame(&mut ed, W);
    let code = rect(&ed, ed.workspace.viewer.focus).w;
    assert!(code >= crate::sizes::CODE_MIN, "the code keeps its least: {code}");
    feed(&mut ed, "<C-w>=");
    assert_eq!((width(&ed), ed.sizes.docked[0]), (auto, None));
}

/// Docked over the code (`C-w K`) at 40 rows, a task in the slot: `C-w +`/`-` size it from its automatic rows, a drag
/// of its bottom edge as far as leaves the code `BOTTOM_CODE` — the code's rows counted once (its own and the path bar's
/// aren't the code's twice), and drawn as far as it was let go.
#[test]
fn a_group_docked_over_the_code_is_sized_by_its_rows() {
    let (_dir, mut ed) = tests_and_a_task();
    feed(&mut ed, "<C-w>K");
    assert_eq!(ed.slot_shown(), Some(Slot::Task), "the task in the slot");
    let rows = |ed: &Editor| ed.dock_rect(GroupKind::Tests).map_or(0, |d| d.1.h);
    let auto = rows(&ed);
    assert!(auto >= 10, "a third of the rows: {auto}");
    feed(&mut ed, "<C-w>+");
    assert_eq!((rows(&ed), ed.sizes.docked[0]), (auto + 2, Some((auto + 2) as u16)), "two rows more");
    feed(&mut ed, "<C-w>-<C-w>-");
    assert_eq!(rows(&ed), auto - 2, "two less than automatic");
    feed(&mut ed, "<C-w>");
    ed.handle_key(Key::plain(Code::Char('<')));
    assert!(ed.toasts.iter().any(|t| t.text.contains("C-w - + size it")), "said");
    // (its edge is the code's top line under its last row — that row is its panel's, a click there a click on it)
    let r = ed.dock_rect(GroupKind::Tests).unwrap().1;
    let edge = r.y + r.h;
    assert_eq!(ed.border_at(10, edge), Some(crate::sizes::Border::Leaf(GroupKind::Tests)), "its edge");
    assert_eq!(ed.border_at(10, edge - 1), None, "its last row isn't an edge");
    feed(&mut ed, "<esc>");
    assert_eq!(ed.workspace.viewer.at, At::Code);
    let click = |ed: &mut Editor, x: usize, y: usize, kind| {
        ed.handle_mouse(crate::event::Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
        frame(ed, W);
    };
    let list = rect(&ed, tests(&ed).list);
    click(&mut ed, list.x + 1, edge - 1, crate::event::MouseKind::Down);
    let list_id = tests(&ed).list;
    assert_eq!(
        (ed.dragging, ed.workspace.viewer.at),
        (None, At::List(list_id)),
        "a click on its last row: its list's"
    );
    click(&mut ed, list.x + 1, edge - 1, crate::event::MouseKind::Up);
    // (dragged from the code's line: let go three rows lower, the line there)
    click(&mut ed, 10, edge, crate::event::MouseKind::Down);
    assert_eq!(ed.dragging, Some(crate::sizes::Border::Leaf(GroupKind::Tests)), "the code's line drags it");
    click(&mut ed, 10, edge + 3, crate::event::MouseKind::Drag);
    click(&mut ed, 10, edge + 3, crate::event::MouseKind::Up);
    let r = ed.dock_rect(GroupKind::Tests).unwrap().1;
    assert_eq!(r.y + r.h, edge + 3, "its bottom line where it was let go");
    // (dragged: its bottom edge down to the status line — the code keeps its least)
    ed.border_drag(crate::sizes::Border::Leaf(GroupKind::Tests), 10, H as usize - 2);
    frame(&mut ed, W);
    let n = ed.sizes.docked[0].unwrap() as usize;
    assert_eq!(rows(&ed), n, "drawn as it was set");
    assert_eq!(ed.screen.body_h - ed.screen.bottom_h, crate::sizes::BOTTOM_CODE, "the code its least");
}

/// Two groups docked on one side are stacked in one column, as wide as the wider, its width taken off the code once:
/// at 140 the tests' and the task's both right of the code, one over the other; sized as one (`C-w >`, a drag as far
/// as leaves the code `CODE_MIN`). On two sides with room for one, the kinds' order decides — the tests' — whatever
/// order the groups were made in.
#[test]
fn two_groups_docked_on_one_side_stack_in_one_column() {
    const W: u16 = 140;
    // (an even body: the stacked halves as tall as each other)
    const H: u16 = 41;
    let (_dir, mut ed) = tests_and_a_task();
    feed_in(&mut ed, "<C-w>L", W, H);
    ed.bottom_take_keys();
    feed_in(&mut ed, "<C-w>L", W, H);
    let both = |ed: &Editor| (ed.docked_in_sight(GroupKind::Tests), ed.docked_in_sight(GroupKind::Task));
    assert_eq!(both(&ed), (true, true), "both shown");
    let (t, k) = (ed.dock_rect(GroupKind::Tests).unwrap().1, ed.dock_rect(GroupKind::Task).unwrap().1);
    assert_eq!((t.x, t.w), (k.x, k.w), "one column: {t:?} {k:?}");
    assert_eq!((t.y, k.y), (0, t.h), "the tests' over the task's: {t:?} {k:?}");
    let code = rect(&ed, ed.workspace.viewer.focus);
    assert_eq!(code.x + code.w, t.x, "the code beside the column: {code:?} {t:?}");
    // (sized as one, from the task's output)
    feed_in(&mut ed, "<C-w>>", W, H);
    let w = t.w + 4;
    assert_eq!(ed.dock_rect(GroupKind::Task).unwrap().1.w, w);
    assert_eq!((ed.sizes.docked[0], ed.sizes.docked[1]), (Some(w as u16), Some(w as u16)));
    ed.border_drag(crate::sizes::Border::Leaf(GroupKind::Tests), 2, 5);
    frame_in(&mut ed, W, H);
    let n = W as usize - crate::sizes::CODE_MIN;
    assert_eq!(ed.sizes.docked[0], Some(n as u16), "as far as leaves the code its least");
    assert_eq!(ed.dock_rect(GroupKind::Task).unwrap().1.w, n);
    // (the task's on the other side, made first: the tests' come first all the same)
    feed_in(&mut ed, "<C-w>=<C-w>H", W, H);
    assert_eq!(ed.workspace.group(GroupKind::Task).map(|g| g.place), Some(Place::Docked(Side::Left)));
    ed.workspace.groups.reverse();
    frame_in(&mut ed, W, H);
    assert_eq!(both(&ed), (true, false), "room for one: the tests'");
}

/// How the tests' group was laid in the last frame.
fn shape(ed: &Editor) -> Option<crate::pane::Shape> {
    ed.screen.shapes.iter().find(|s| s.0 == GroupKind::Tests).map(|s| s.1)
}

/// Row `y` of the frame from column `x` on.
fn row_from(text: &str, y: usize, x: usize) -> String {
    text.lines().nth(y).unwrap_or_default().chars().skip(x).collect()
}

/// A group lays its members by its pane's shape (user, 2026-10-23): side by side in the slot (160 and 80 wide) and
/// docked over the code (wide and short); over and under docked right or left at 160 (tall and 64 wide) — the list as
/// tall as its two rows, the output's own top line under it, named, joined to the group's edge (`├` right, `┤` left),
/// the edge starting at the group's top line (`┌`/`┐`, nothing above it) and the code's rule running up to it (`┤`/`├`).
/// The list's keys work there; a click on the output's name gives it the keys.
#[test]
fn a_group_lays_its_members_by_its_panes_shape() {
    use crate::pane::Shape;
    let (_dir, mut ed) = tests_and_a_task();
    assert_eq!(shape(&ed), Some(Shape::Beside), "the slot at 160");
    frame(&mut ed, 80);
    assert_eq!(shape(&ed), Some(Shape::Beside), "the slot at 80");
    feed(&mut ed, "<C-w>L");
    assert_eq!(shape(&ed), Some(Shape::Over), "docked right at 160");
    let g = tests(&ed);
    let (list, detail) = (rect(&ed, g.list), rect(&ed, g.detail_pane().unwrap()));
    assert_eq!(
        (list.h, list.x, list.w),
        (2, detail.x, detail.w),
        "its two rows over the output: {list:?} {detail:?}"
    );
    assert_eq!(detail.y, list.y + list.h + 1, "the output's top line between");
    let text = shown(&mut ed, W);
    let edge = list.x - 1;
    assert!(row_from(&text, detail.y - 1, edge).starts_with("├─ output ─"), "{text}");
    assert!(
        row_from(&text, 0, edge - 2).starts_with("──┬─ tests"),
        "the edge starts where the code's top line meets its own: {text}"
    );
    assert!(row_from(&text, 1, edge).starts_with('│'), "nothing of the code's beside it under: {text}");
    feed(&mut ed, "j");
    assert_eq!(ed.workspace.test_run.as_ref().unwrap().selected, 1, "the list's keys");
    // (thirty: the list a third of the pane, the last one selected in sight there)
    let case = |i: usize| crate::test_results::Case {
        name: format!("m::case_{i:02}"),
        status: crate::test_results::Status::Passed,
        secs: None,
        message: Vec::new(),
        at: None,
    };
    let two =
        std::mem::replace(&mut ed.workspace.test_run.as_mut().unwrap().cases, (0..30).map(case).collect());
    feed(&mut ed, "G");
    let (list, text) = (rect(&ed, g.list), shown(&mut ed, W));
    assert_eq!(list.h, (H as usize - 2) / 3, "a third of the rows under its line");
    let rows: Vec<String> = (list.y..list.y + list.h).map(|y| row_from(&text, y, list.x)).collect();
    assert!(rows.last().is_some_and(|r| r.contains("case_29")), "{rows:?}");
    ed.workspace.test_run.as_mut().unwrap().cases = two;
    feed(&mut ed, "gj");
    let name = ed.screen.member_names.borrow().iter().find(|n| n.0 == detail.y - 1).copied();
    let (y, a, ..) = name.expect("the output's name on its line");
    let kind = crate::event::MouseKind::Down;
    ed.handle_mouse(crate::event::Mouse { kind, x: a as u16, y: y as u16, alt: false, ctrl: false });
    frame(&mut ed, W);
    assert_eq!(
        (ed.workspace.viewer.at, ed.slot()),
        (At::Slot, Some(Slot::Test)),
        "a click on its name: its keys"
    );
    feed(&mut ed, "<C-w>H");
    assert_eq!(shape(&ed), Some(Shape::Over), "docked left");
    let g = tests(&ed);
    let (list, detail) = (rect(&ed, g.list), rect(&ed, g.detail_pane().unwrap()));
    let text = shown(&mut ed, W);
    let edge = list.x + list.w;
    let at = |y: usize| text.lines().nth(y).unwrap_or_default().chars().nth(edge);
    assert_eq!((at(0), at(1), at(detail.y - 1)), (Some('┬'), Some('│'), Some('┤')), "{text}");
    feed(&mut ed, "<C-w>K");
    assert_eq!(shape(&ed), Some(Shape::Beside), "docked over the code at 160");
    frame(&mut ed, 80);
    frame(&mut ed, 80);
    assert_eq!(shape(&ed), Some(Shape::Beside), "…and at 80");
}

/// P3 step 6b: a pane's bottom line is the next one's top — a group docked over the code has no edge of its own under
/// it: its last row is its panel's, the code's top line right under it (never two lines); docked right, its edge starts
/// at the code's line and its own, one `┬`.
#[test]
fn a_group_docked_over_the_code_meets_its_line() {
    let (_dir, mut ed) = tests_and_a_task();
    feed(&mut ed, "<C-w>K");
    let r = ed.dock_rect(GroupKind::Tests).unwrap().1;
    let text = shown(&mut ed, W);
    let code_line = r.y + r.h;
    assert!(row_from(&text, code_line, r.x).starts_with("─ tarae › "), "the code's line under it: {text}");
    let only_line = |y: usize| row_from(&text, y, r.x).trim().chars().all(|c| c == '─');
    assert!(!only_line(code_line - 1), "no edge of the group's over it: {text}");
    assert_eq!(ed.screen.text_top, code_line + 1, "the code right under its line");
    feed(&mut ed, "<C-w>L");
    let r = ed.dock_rect(GroupKind::Tests).unwrap().1;
    let text = shown(&mut ed, W);
    assert!(row_from(&text, 0, r.x - 2).starts_with("──┬─ tests"), "{text}");
}

/// `C-w t` on a group's member turns it as it turns a split: docked right (over and under) to side by side — no room
/// at 64 columns, stacked until there is, said; wider (`C-w >`), side by side; again, over and under; `C-w =` makes it
/// automatic. In the slot (side by side), over and under once it's tall enough (`C-w +`); `C-w =` back.
#[test]
fn c_w_t_turns_a_groups_members() {
    use crate::pane::{Laid, Shape};
    let (_dir, mut ed) = tests_and_a_task();
    let said = |ed: &Editor, s: &str| ed.toasts.iter().any(|t| t.text.starts_with(s));
    feed(&mut ed, "<C-w>L");
    assert_eq!(shape(&ed), Some(Shape::Over));
    feed(&mut ed, "<C-w>t");
    assert_eq!((tests(&ed).laid, shape(&ed)), (Laid::Beside, Some(Shape::Stacked)), "no room: stacked");
    assert!(said(&ed, "tests side by side when there's room — stacked until then"), "said");
    feed(&mut ed, "<C-w>><C-w>><C-w>>");
    assert_eq!(shape(&ed), Some(Shape::Beside), "with room, as chosen");
    feed(&mut ed, "<C-w>t");
    assert_eq!((tests(&ed).laid, shape(&ed)), (Laid::Over, Some(Shape::Over)));
    assert!(said(&ed, "tests over and under — C-w t turns it back"), "said");
    feed(&mut ed, "<C-w>t");
    assert_eq!(shape(&ed), Some(Shape::Beside), "and back");
    assert_eq!(ed.workspace.views.len(), 1, "the code's split untouched");
    feed(&mut ed, "<C-w>=");
    assert_eq!((tests(&ed).laid, shape(&ed)), (Laid::Auto, Some(Shape::Over)), "automatic");
    // (in the slot)
    feed(&mut ed, "<C-w>J");
    assert_eq!(shape(&ed), Some(Shape::Beside), "the slot: side by side");
    feed(&mut ed, "<C-w>t");
    assert_eq!(shape(&ed), Some(Shape::Stacked), "too short for over and under");
    feed(&mut ed, "<C-w>+<C-w>+");
    assert_eq!(shape(&ed), Some(Shape::Over), "taller: over and under");
    feed(&mut ed, "<C-w>=");
    assert_eq!(shape(&ed), Some(Shape::Beside), "automatic again");
}

/// Docked over the code, `C-w t` says the shape it draws: the panel the whole rect under its line (its bottom the code's
/// line) — at the height where over and under starts to fit (`OVER_ROWS`), 60 wide (too narrow side by side), over and
/// under drawn and said, never "stacked until then".
#[test]
fn c_w_t_over_the_code_says_what_it_draws() {
    use crate::pane::{Laid, Shape};
    let (_dir, mut ed) = tests_and_a_task();
    feed_in(&mut ed, "<C-w>K", 60, H);
    ed.sizes.docked[GroupKind::Tests.dock_index()] =
        Some((crate::term::OVER_ROWS + crate::chrome::SLOT_HEAD) as u16);
    frame_in(&mut ed, 60, H);
    assert_eq!(shape(&ed), Some(Shape::Over), "tall enough: over and under");
    feed_in(&mut ed, "<C-w>t", 60, H);
    assert_eq!((tests(&ed).laid, shape(&ed)), (Laid::Beside, Some(Shape::Stacked)), "no room side by side");
    feed_in(&mut ed, "<C-w>t", 60, H);
    assert_eq!((tests(&ed).laid, shape(&ed)), (Laid::Over, Some(Shape::Over)));
    let said = |s: &str| ed.toasts.iter().any(|t| t.text.starts_with(s));
    assert!(said("tests over and under — C-w t turns it back"), "said as drawn");
}

/// A click on a docked group's output gives it the keys; one on the code takes them back; the state lists its host
/// with its place and its members with their host.
#[test]
fn a_docked_groups_panes_are_clicked_and_listed() {
    let (_dir, mut ed) = tests_and_a_task();
    feed(&mut ed, "<C-w>L<esc>");
    assert_eq!(ed.workspace.viewer.at, At::Code);
    let g = tests(&ed);
    let detail = rect(&ed, g.detail_pane().unwrap());
    let click = |ed: &mut Editor, x: usize, y: usize| {
        let kind = crate::event::MouseKind::Down;
        ed.handle_mouse(crate::event::Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
        frame(ed, W);
    };
    assert!(detail.w > 0 && detail.y > rect(&ed, g.list).y, "64 columns: over and under");
    // (wider: side by side)
    ed.sizes.docked[GroupKind::Tests.dock_index()] = Some(90);
    frame(&mut ed, W);
    let detail = rect(&ed, g.detail_pane().unwrap());
    assert!(detail.w > 0, "90 columns: side by side");
    click(&mut ed, detail.x + 3, detail.y + 2);
    assert_eq!((ed.workspace.viewer.at, ed.slot()), (At::Slot, Some(Slot::Test)), "its detail has the keys");
    let list = rect(&ed, g.list);
    click(&mut ed, list.x + 3, list.y + 1);
    assert_eq!(ed.workspace.viewer.at, At::List(g.list), "a row of its list");
    let state = ed.ui_state().json();
    let panes = state["workspaces"][0]["panes"].as_array().cloned().unwrap_or_default();
    let host = panes.iter().find(|p| p["shows"] == "group:tests").expect("its host listed");
    assert_eq!(host["place"], "right");
    let member = panes.iter().find(|p| p["shows"] == "list:tests").expect("its list listed");
    assert_eq!((&member["group"], &member["at"]), (&host["id"], &serde_json::json!(true)));
    let code = rect(&ed, ed.workspace.viewer.focus);
    click(&mut ed, code.x + 8, code.y + 2);
    assert_eq!(ed.workspace.viewer.at, At::Code, "the code's again");
}
