//! The turn's map: the run-over rule first, then its roles, then what's drawn and what a press does.

use std::ops::Range;
use std::path::{Path, PathBuf};

use super::*;
use crate::chat::Look;
use crate::config::Config;
use crate::replay::{Hunk, Mapped};
use crate::sidebar::Sidebar;

const ROOT: &str = "/project";

fn at(rel: &str) -> PathBuf {
    Path::new(ROOT).join(rel)
}

fn look(verb: &'static str, what: &str, range: &str) -> Look {
    Look { verb, what: what.into(), pattern: false, range: range.into(), link: None }
}

fn stop(verb: &'static str, path: Option<PathBuf>, lines: Option<Range<usize>>) -> Stop {
    let (call, why, page, hunks) = (String::new(), String::new(), None, Vec::new());
    Stop { call, look: look(verb, "", ""), path, lines, hunks, why, page, map: Mapped::default() }
}

fn read(rel: &str, lines: Range<usize>) -> Stop {
    stop("read", Some(at(rel)), Some(lines))
}

fn search(rel: &str, line: usize) -> Stop {
    stop("search", Some(at(rel)), Some(line..line + 1))
}

fn edit(rel: &str, lines: Range<usize>) -> Stop {
    let hunks = vec![Hunk { lines: lines.clone(), removed: Vec::new(), added: Vec::new() }];
    Stop { hunks, ..stop("edit", Some(at(rel)), Some(lines)) }
}

/// A `run_tests` of `rel`, `holds` what its target holds (`None`: it never started), `verdict` its row's.
fn test(rel: &str, holds: Option<Holds>, verdict: &str) -> Stop {
    let map = Mapped { holds, ..Mapped::default() };
    Stop { look: look("test", rel, verdict), map, ..stop("test", Some(at(rel)), None) }
}

fn map(stops: &[Stop]) -> HashMap<PathBuf, File> {
    fold(stops, Path::new(ROOT)).0
}

/// A file's marks by weight, as its row says them.
fn said(stops: &[Stop], rel: &str) -> Vec<String> {
    map(stops).get(Path::new(rel)).map_or(Vec::new(), |f| f.roles.iter().map(Role::said).collect())
}

// ── Run over: the one rule ──────────────────────────────────────────────────────────────────

/// A plain `run` never marks `▸`: no coverage, tarae can't know what a command exercised — even one that says its tests
/// passed, even one naming the file.
#[test]
fn a_plain_run_never_marks_run_over() {
    let run =
        |what: &str, verdict: &str| Stop { look: look("run", what, verdict), ..stop("run", None, None) };
    let turn = [edit("a.rs", 3..4), run("cargo test -q", "ok"), run("cargo test a.rs", "12 passed")];
    assert_eq!(said(&turn, "a.rs"), ["●"]);
    // (given a place and what it holds as a test run has, still a `run`)
    let mut run = run("cargo test", "12 passed");
    (run.path, run.map.holds) = (Some(at("a.rs")), Some(Holds::File));
    assert_eq!(said(&[edit("a.rs", 3..4), run], "a.rs"), ["●"]);
}

/// A `run_tests` of another file doesn't: `tests.rs` passing says nothing tarae knows of `a.rs` — and an unchanged file a
/// run went over has no change to be run over.
#[test]
fn a_run_tests_of_another_file_doesnt() {
    let turn = [edit("a.rs", 3..4), test("tests.rs", Some(Holds::File), "12 passed")];
    assert_eq!(said(&turn, "a.rs"), ["●"]);
    assert_eq!(said(&turn, "tests.rs"), Vec::<String>::new());
}

/// A `run_tests` of the changed file, the whole of it or a target holding the change, marks it — one whose lines miss
/// every change there doesn't.
#[test]
fn a_run_tests_holding_the_change_marks_it() {
    let whole = [edit("a.rs", 3..4), test("a.rs", Some(Holds::File), "12 passed")];
    assert_eq!(said(&whole, "a.rs"), ["●", "▸"]);
    let on = [edit("a.rs", 3..4), test("a.rs", Some(Holds::Lines(2..6)), "passed")];
    assert_eq!(said(&on, "a.rs"), ["●", "▸"]);
    let off = [edit("a.rs", 3..4), test("a.rs", Some(Holds::Lines(10..20)), "passed")];
    assert_eq!(said(&off, "a.rs"), ["●"]);
}

/// "Ran after the last change": a change after the run takes `▸` back, the run before it never gave it.
#[test]
fn a_change_after_the_run_takes_it_back() {
    let run = || test("a.rs", Some(Holds::File), "12 passed");
    let turn = [edit("a.rs", 3..4), run(), edit("a.rs", 9..10)];
    assert_eq!(said(&turn, "a.rs"), ["●"]);
    assert_eq!(said(&[run(), edit("a.rs", 3..4)], "a.rs"), ["●"]);
    let again = [edit("a.rs", 3..4), run(), edit("a.rs", 9..10), run()];
    assert_eq!(said(&again, "a.rs"), ["●", "▸"]);
}

/// Only a pass claims it: a run under way, one that ran nothing, one never started.
#[test]
fn only_a_pass_marks_it() {
    for verdict in ["", "no tests ran", "not run"] {
        let turn = [edit("a.rs", 3..4), test("a.rs", Some(Holds::File), verdict)];
        assert_eq!(said(&turn, "a.rs"), ["●"], "{verdict:?}");
    }
    assert_eq!(said(&[edit("a.rs", 3..4), test("a.rs", None, "12 passed")], "a.rs"), ["●"]);
}

/// `▲` claims less — a test failed there: a failed run at its file and the failure it went to, whatever changed, until
/// the same run passes or the file is changed; it takes `▸` from what ran before it.
#[test]
fn a_failure_stands_until_its_run_passes_or_its_file_changes() {
    let held = || Some(Holds::File);
    let failure = stop("failed", Some(at("b.rs")), Some(7..8));
    let failed = [edit("a.rs", 3..4), test("a.rs", held(), "1 failed"), failure.clone()];
    assert_eq!(said(&failed, "a.rs"), ["▲", "●"]);
    assert_eq!(said(&failed, "b.rs"), ["▲"], "the failure it went to");
    let mut fixed = failed.to_vec();
    fixed.push(edit("b.rs", 7..8));
    assert_eq!(said(&fixed, "b.rs"), ["●"], "its file changed");
    assert_eq!(said(&fixed, "a.rs"), ["▲", "●"], "the run's own stands");
    fixed.push(test("a.rs", held(), "12 passed"));
    assert_eq!(said(&fixed, "a.rs"), ["●", "▸"], "passed again");
    let mut gone = failed.to_vec();
    gone.push(test("a.rs", held(), "12 passed"));
    assert_eq!(said(&gone, "b.rs"), Vec::<String>::new(), "its run passed: the failure went with it");
    let broke = [edit("a.rs", 3..4), test("a.rs", held(), "12 passed"), test("a.rs", held(), "1 failed")];
    assert_eq!(said(&broke, "a.rs"), ["▲", "●"], "it passed, then failed: not run over");
    // (again, but nothing ran: it stands — under way, never started, no test in it)
    for verdict in ["", "not run", "no tests ran"] {
        let mut again = failed.to_vec();
        again.push(test("a.rs", held(), verdict));
        assert_eq!(said(&again, "a.rs"), ["▲", "●"], "{verdict:?}");
        assert_eq!(said(&again, "b.rs"), ["▲"], "{verdict:?}");
    }
    // (a command that ended well is over it too, its `ok` no test's `passed`)
    let run = |verdict: &str| Stop { look: look("run", "cargo test", verdict), ..stop("run", None, None) };
    let failed = [run("exit 101"), failure.clone()];
    assert_eq!(said(&failed, "b.rs"), ["▲"]);
    let again = |verdict: &str| [failed.to_vec(), vec![run(verdict)]].concat();
    assert_eq!(said(&again("not run"), "b.rs"), ["▲"]);
    assert_eq!(said(&again("ok"), "b.rs"), Vec::<String>::new());
}

/// A call that came back an error — failed, or refused on its card — did nothing: no read, no search, on the map or in a
/// count; a change that landed stands whatever its call said after.
#[test]
fn a_call_that_failed_claims_nothing() {
    let failed = |s: Stop| Stop { map: Mapped { errored: true, ..s.map.clone() }, ..s };
    let turn = [failed(read("a.rs", 0..1)), failed(search("a.rs", 3)), read("b.rs", 0..1)];
    assert!(!map(&turn).contains_key(Path::new("a.rs")), "{:?}", map(&turn));
    assert_eq!(said(&turn, "b.rs"), ["◦"]);
    let reads = [read("a.rs", 0..1), failed(read("a.rs", 4..5))];
    assert_eq!(said(&reads, "a.rs"), ["◦"], "not counted");
    assert_eq!(said(&[failed(edit("a.rs", 3..4))], "a.rs"), ["●"]);
}

// ── Roles ─────────────────────────────────────────────────────────────────────────────────────────

/// Weight, not time: changed before read, a read's count when it was read more than once, past nine `9+`.
#[test]
fn roles_go_by_weight_and_reads_are_counted() {
    let turn = [read("a.rs", 0..2), search("a.rs", 1), read("a.rs", 5..6), edit("a.rs", 3..4)];
    let turn = [&turn[..], &[test("a.rs", Some(Holds::File), "passed")]].concat();
    assert_eq!(said(&turn, "a.rs"), ["●", "▸", "•", "◦ 2"]);
    assert_eq!(said(&[read("a.rs", 0..2)], "a.rs"), ["◦"], "read once: no count");
    let many: Vec<Stop> = (0..12).map(|i| read("a.rs", i..i + 1)).collect();
    assert_eq!(said(&many, "a.rs"), ["◦ 9+"]);
}

/// A read with no lines of its own is of the file: its row says it, no line does — not the first, what a read of no
/// range was recorded as before the map read it.
#[test]
fn a_whole_read_marks_the_file_not_a_line() {
    let m = map(&[stop("read", Some(at("a.rs")), None)]);
    let f = &m[Path::new("a.rs")];
    assert_eq!(f.roles[0].said(), "◦");
    assert!(f.lines.is_empty(), "{:?}", f.lines);
}

/// What isn't a place in the project: outside it, counted and never marked; a note, a plan and a page, no mark at all.
#[test]
fn what_has_no_place_on_the_map() {
    let mut page = read("a.rs", 0..1);
    page.page = Some(crate::replay::PageAt { url: "https://a.test".into(), copy: None });
    let turn = [
        stop("read", Some(PathBuf::from("/elsewhere/x.rs")), Some(0..1)),
        stop("note", Some(at("a.rs")), Some(0..1)),
        stop("plan", None, None),
        page,
    ];
    let (files, outside) = fold(&turn, Path::new(ROOT));
    assert!(files.is_empty(), "{files:?}");
    assert_eq!(outside, 1);
}

/// A file the turn removed or moved: changed, with no lines to say so.
#[test]
fn a_file_gone_is_changed_with_no_lines() {
    let mut gone = stop("edit", Some(at("a.rs")), None);
    gone.map.whole = true;
    let m = map(&[gone]);
    assert_eq!(m[Path::new("a.rs")].roles[0].mark, Mark::Edit);
    assert!(m[Path::new("a.rs")].lines.is_empty());
    // (an edit's stop before it landed — a rename's, refused — changed nothing)
    assert!(map(&[stop("edit", Some(at("a.rs")), None)]).is_empty());
}

/// A press on a mark goes to its role's next stop after the one in hand, then round to its first.
#[test]
fn a_press_goes_round_a_roles_stops() {
    let r = Role { mark: Mark::Read, stops: vec![1, 4, 6] };
    assert_eq!([r.press(None), r.press(Some(1)), r.press(Some(4)), r.press(Some(6))], [1, 4, 6, 1]);
    assert_eq!(r.press(Some(2)), 4, "the one in hand another role's");
}

/// A definition's roles are its own lines': a method's read marks the method, not the `impl` around it.
#[test]
fn the_outline_marks_a_lines_innermost_definition() {
    let src = "impl A {\n    fn one() {\n    }\n    fn two() {\n    }\n}\nfn free() {\n}\n";
    let text = ropey::Rope::from_str(src);
    let def = |name: &str, end: &str, depth: usize| {
        let start = src.find(name).unwrap();
        let end = start + src[start..].find(end).unwrap() + end.len();
        crate::syntax::Def { depth, kind: 'f', name: name.into(), range: start..end, at: start }
    };
    let defs =
        [def("impl A", "\n}", 0), def("fn one", "}", 1), def("fn two", "}", 1), def("fn free", "}", 0)];
    let mut state = State::default();
    (state.files, _) = fold(&[read("a.rs", 1..3), edit("a.rs", 3..4)], Path::new(ROOT));
    let roles = state.outline(Path::new("a.rs"), &defs, &text);
    let said: Vec<Vec<String>> = roles.iter().map(|r| r.iter().map(Role::said).collect()).collect();
    let none = Vec::<String>::new();
    assert_eq!(said, [none.clone(), vec!["◦".to_string()], vec!["●".to_string()], none]);
}

// ── On screen ────────────────────────────────────────────────────────────────────────────────

const CODE: &str = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\nfn f() {}\n";

/// An editor on a project of `src/a.rs`, `src/b.rs` and `src/deep/c.rs` (`rels` for others) with the side panel up,
/// `src/a.rs` open and a chat whose turn 14 asked `why`.
fn project_of(s: &crate::scratch::Scratch, rels: &[&str]) -> Editor {
    let rels: Vec<String> = rels.iter().map(|r| r.to_string()).collect();
    for r in &rels {
        std::fs::create_dir_all(s.join(r).parent().unwrap()).unwrap();
        std::fs::write(s.join(r), CODE).unwrap();
    }
    let mut ed = Editor::new(Config::default());
    ed.workspace.root = s.to_path_buf();
    ed.open_for_claude(&s.join("src/a.rs")).unwrap();
    let mut panel = Sidebar::with_files(s.to_path_buf(), rels);
    panel.set_open(["src"], true);
    ed.workspace.viewer.sidebar = Some(panel);
    let mut c = crate::chat::Chat::new();
    c.follow.new_turn();
    c.follow.replay.asked(14, "why does the map stay?");
    ed.workspace.claude.chat = Some(c);
    ed
}

fn project(s: &crate::scratch::Scratch) -> Editor {
    project_of(s, &["src/a.rs", "src/b.rs", "src/deep/c.rs"])
}

fn add(ed: &mut Editor, stops: Vec<Stop>) {
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    stops.into_iter().for_each(|s| c.follow.replay.add(s));
    sync(ed);
    ed.sidebar_map();
}

/// The frame at `w`×40, as text a row each.
fn frame(ed: &mut Editor, w: u16) -> Vec<String> {
    let mut buf = Vec::new();
    crate::term::render(ed, &mut buf, w, 40).unwrap();
    let mut vt = vt100::Parser::new(40, w, 0);
    vt.process(&buf);
    (0..40).map(|y| vt.screen().contents_between(y, 0, y, w)).collect()
}

fn row<'a>(rows: &'a [String], name: &str) -> &'a str {
    // (past the top lines: the code's says the path too)
    rows.iter().skip(1).find(|r| r.contains(name)).unwrap_or_else(|| panic!("no row {name}: {rows:#?}"))
}

fn press(ed: &mut Editor, x: usize, y: usize) {
    use crate::event::{Mouse, MouseKind};
    for kind in [MouseKind::Down, MouseKind::Up] {
        ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
    }
}

/// The turn on the code's structure: the tree's marks in weight order, the sign column's on their lines, the top line
/// naming the turn — a press on a mark goes to its stop, `]r`/`[r` step the same stops, and the state lists each.
#[test]
fn a_turn_reads_as_a_map_and_a_press_is_its_stop() {
    let s = crate::scratch::Scratch::new("turnmap-keys");
    let mut ed = project(&s);
    // (room beside the tabs for the turn: `the_tabs_stay_whole_and_the_turn_folds_first`)
    ed.sizes.side = Some(38);
    let in_s = |st: Stop, rel: &str| Stop { path: Some(s.join(rel)), ..st };
    add(
        &mut ed,
        vec![
            in_s(read("", 0..2), "src/a.rs"),
            in_s(search("", 1), "src/b.rs"),
            in_s(read("", 4..5), "src/a.rs"),
            in_s(edit("", 2..3), "src/a.rs"),
            in_s(test("", Some(Holds::File), "12 passed"), "src/a.rs"),
        ],
    );
    let rows = frame(&mut ed, 150);
    assert!(row(&rows, "a.rs ").contains("● ▸ ◦ 2"), "{}", row(&rows, "a.rs "));
    assert!(row(&rows, "b.rs").contains('•'), "{}", row(&rows, "b.rs"));
    assert!(rows[0].contains("turn 14"), "{}", rows[0]);
    // The sign column: `◦` on what it read, `●` on what it changed
    assert!(row(&rows, "fn a()").contains('◦'), "{}", row(&rows, "fn a()"));
    assert!(row(&rows, "fn c()").contains('●'), "{}", row(&rows, "fn c()"));
    assert!(!row(&rows, "fn d()").contains(['◦', '●']), "{}", row(&rows, "fn d()"));
    // Each mark a button, listed for the state: a press on the file's `●` is the edit's stop
    let id = "map:src/a.rs:●";
    let cell = |ed: &Editor, id: &str| {
        let marks = ed.screen.map_marks.borrow();
        marks.iter().find(|m| m.2 == id).map(|m| (m.1, m.0)).unwrap_or_else(|| panic!("no {id}: {marks:?}"))
    };
    let (x, y) = cell(&ed, id);
    assert!(matches!(ed.kind_at(x, y), crate::kinds::Kind::Button(_)), "{:?}", ed.kind_at(x, y));
    // (a button: under the pointer its ground lightens, its ink stays)
    crossterm::style::force_color_output(true);
    let painted = |ed: &mut Editor| {
        let mut buf = Vec::new();
        crate::term::render(ed, &mut buf, 150, 40).unwrap();
        let mut vt = vt100::Parser::new(40, 150, 0);
        vt.process(&buf);
        let c = vt.screen().cell(y as u16, x as u16).unwrap().clone();
        (c.bgcolor(), c.fgcolor())
    };
    let (ground, ink) = painted(&mut ed);
    ed.handle_event(crate::event::Event::Hover(x as u16, y as u16));
    let lit = painted(&mut ed);
    assert!(lit.0 != ground && lit.1 == ink, "{ground:?} {ink:?} → {lit:?}");
    let listed =
        |ed: &Editor| ed.ui_state().panels.lists.iter().find(|l| l.0 == "map").map(|l| l.1.clone()).unwrap();
    let list = listed(&ed);
    assert!(list.iter().any(|r| r.id == id && r.label.contains("changed")), "{list:?}");
    // (pointed at: said as what it is, never as gone)
    let said = crate::uistate::claude::pointed_said(&ed, id);
    assert!(said.contains("changed") && said.contains("last turn"), "{said}");
    press(&mut ed, x, y);
    let hand = |ed: &Editor| ed.workspace.claude.chat.as_ref().unwrap().follow.replay.at;
    assert_eq!(hand(&ed), Some((ed.turnmap.turn, 3)), "the edit's stop");
    sync(&mut ed);
    // The stop in hand lit, in the state too; `]r` from there the next, as a press would step
    frame(&mut ed, 150);
    assert_eq!(ed.turnmap.hand, Some(3));
    let list = listed(&ed);
    assert!(list.iter().any(|r| r.id == id && r.selected), "lit: {list:?}");
    crate::follow::replay_step(&mut ed, true);
    assert_eq!(hand(&ed), Some((ed.turnmap.turn, 4)), "`]r`: the same places, in time order");
    // A press on a line's sign: that line's stop
    crate::follow::replay_step(&mut ed, false);
    sync(&mut ed);
    frame(&mut ed, 150);
    let (x, y) = cell(&ed, "map:src/a.rs@1:◦");
    press(&mut ed, x, y);
    assert_eq!(hand(&ed), Some((ed.turnmap.turn, 0)), "the read's stop");
}

/// Kept until the next turn starts — the turn over, a single session, it stays; a new one clears it; an older turn `[r`
/// steps into shows that turn's.
#[test]
fn it_stays_until_the_next_turn_and_follows_replay_back() {
    let s = crate::scratch::Scratch::new("turnmap-kept");
    let mut ed = project(&s);
    add(&mut ed, vec![Stop { path: Some(s.join("src/a.rs")), ..read("", 0..1) }]);
    assert!(ed.workspace.claude.chat.as_ref().is_some_and(|c| !c.busy()), "no turn under way");
    assert_eq!(ed.turnmap.files.len(), 1);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.follow.new_turn();
    c.follow.replay.asked(15, "next");
    add(&mut ed, Vec::new());
    assert!(ed.turnmap.is_empty(), "the next turn started");
    assert!(ed.turnmap.said().is_empty(), "nothing on it: the top line says none");
    add(&mut ed, vec![Stop { path: Some(s.join("src/b.rs")), ..read("", 0..1) }]);
    assert!(ed.turnmap.files.contains_key(Path::new("src/b.rs")));
    crate::follow::replay_step(&mut ed, false);
    crate::follow::replay_step(&mut ed, false);
    sync(&mut ed);
    assert!(ed.turnmap.files.contains_key(Path::new("src/a.rs")), "back in the last turn: its map");
    assert_eq!(ed.turnmap.said(), ["turn 14"]);
}

/// Closing the chat keeps the map, as its answers stay at the code: the code wide is where a map reads best. A press and
/// `]r` still go to the places, the chat takes its stops back as it opens, and the next turn clears them.
#[test]
fn closing_the_chat_keeps_the_map() {
    let s = crate::scratch::Scratch::new("turnmap-closed");
    let mut ed = project(&s);
    let stops = vec![
        Stop { path: Some(s.join("src/a.rs")), ..read("", 0..1) },
        Stop { path: Some(s.join("src/b.rs")), ..read("", 4..5) },
    ];
    add(&mut ed, stops);
    crate::chat::close(&mut ed);
    sync(&mut ed);
    assert_eq!(ed.turnmap.files.len(), 2, "closed, it stays");
    let (turn, rows) = (ed.turnmap.turn, frame(&mut ed, 120));
    assert!(row(&rows, "b.rs").contains('◦'), "{}", row(&rows, "b.rs"));
    crate::follow::replay_at(&mut ed, (turn, 1));
    assert_eq!(ed.doc().path.as_deref(), Some(s.join("src/b.rs").as_path()), "a press goes to its place");
    assert_eq!(ed.doc().text.byte_to_line(ed.doc().selection().primary().head), 4);
    crate::follow::replay_step(&mut ed, false);
    assert_eq!(ed.doc().path.as_deref(), Some(s.join("src/a.rs").as_path()), "`[r` too");
    // (as the open chat: at the oldest the hand stays and it says so, past the newest it's back to live)
    let said = |ed: &Editor| ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
    let hand = |ed: &Editor| ed.workspace.claude.stops_left.as_ref().unwrap().at;
    crate::follow::replay_step(&mut ed, false);
    assert_eq!((hand(&ed), said(&ed).as_str()), (Some((turn, 0)), "that was the first stop kept"));
    crate::follow::replay_step(&mut ed, true);
    assert_eq!(hand(&ed), Some((turn, 1)));
    crate::follow::replay_step(&mut ed, true);
    assert_eq!((hand(&ed), said(&ed).as_str()), (None, "that was the newest stop"));
    sync(&mut ed);
    assert!(!ed.turnmap.is_empty());
    // (a stand-in claude: opening starts one)
    ed.config.llm.command = "sh".into();
    ed.config.llm.args = vec!["-c".into(), "cat > /dev/null".into()];
    crate::chat::open(&mut ed);
    sync(&mut ed);
    assert_eq!(ed.turnmap.files.len(), 2, "the chat opened: not a turn");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    assert_eq!(c.follow.replay.turns().len(), 1, "it took its stops back");
    c.follow.new_turn();
    sync(&mut ed);
    assert!(ed.turnmap.is_empty(), "the next turn started");
}

/// A closed chat that left no stops says so, either way, never that a stop was reached.
#[test]
fn no_stops_kept_is_said_with_the_chat_closed() {
    let s = crate::scratch::Scratch::new("turnmap-none");
    let mut ed = project(&s);
    crate::chat::close(&mut ed);
    ed.workspace.claude.stops_left = Some(crate::replay::Replay::default());
    for forward in [false, true] {
        ed.status = None;
        crate::follow::replay_step(&mut ed, forward);
        let said = ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
        assert!(said.starts_with("nothing to replay"), "{said}");
    }
}

/// With the chat closed no card says what a stop was: one with no place to go to — a plain run, a file the turn removed —
/// says it where you look, a press or `]r` never silent.
#[test]
fn a_stop_with_no_place_is_said_with_the_chat_closed() {
    let s = crate::scratch::Scratch::new("turnmap-said");
    let mut ed = project(&s);
    let run = Stop { look: look("run", "cargo test -q", "ok"), ..stop("run", None, None) };
    let map = Mapped { whole: true, ..Mapped::default() };
    let gone =
        Stop { look: look("edit", "src/x.rs", ""), map, ..stop("edit", Some(s.join("src/x.rs")), None) };
    add(&mut ed, vec![run, gone]);
    crate::chat::close(&mut ed);
    sync(&mut ed);
    let turn = ed.turnmap.turn;
    let said = |ed: &Editor| ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
    crate::follow::replay_at(&mut ed, (turn, 0));
    assert!(said(&ed).contains("cargo test -q") && said(&ed).contains("ok"), "{}", said(&ed));
    ed.status = None;
    crate::follow::replay_step(&mut ed, true);
    assert!(said(&ed).contains("src/x.rs") && said(&ed).contains("gone"), "`]r`: {}", said(&ed));
}

/// Lines you moved since aren't the stops': your edit takes the file's signs and leaves its row — Claude's own doesn't.
#[test]
fn your_edit_takes_the_signs_and_leaves_the_row() {
    let s = crate::scratch::Scratch::new("turnmap-yours");
    let mut ed = project(&s);
    add(&mut ed, vec![Stop { path: Some(s.join("src/a.rs")), ..read("", 0..2) }]);
    let drawn = |ed: &mut Editor| row(&frame(ed, 120), "fn a()").contains('◦');
    assert!(drawn(&mut ed), "the sign");
    // (a new version of the text, whoever's)
    let changed = |ed: &mut Editor| {
        use crate::key::{Code, Key};
        [Code::Char('i'), Code::Char('x'), Code::Esc].into_iter().for_each(|c| ed.handle_key(Key::plain(c)));
    };
    changed(&mut ed);
    landed(&mut ed, &s.join("src/a.rs"));
    sync(&mut ed);
    assert!(drawn(&mut ed), "Claude's landing keeps it");
    changed(&mut ed);
    sync(&mut ed);
    let rows = frame(&mut ed, 120);
    assert!(!row(&rows, "fn a()").contains('◦'), "yours takes it: {}", row(&rows, "fn a()"));
    assert!(row(&rows, "a.rs ").contains('◦'), "its row stays: {}", row(&rows, "a.rs "));
}

/// The outline's marks are the signs': your edit takes them too.
#[test]
fn your_edit_takes_the_outlines_marks() {
    use crate::key::{Code, Key};
    let s = crate::scratch::Scratch::new("turnmap-outline");
    let mut ed = project(&s);
    let panel = ed.workspace.viewer.sidebar.as_mut().unwrap();
    panel.mode = crate::sidebar::Mode::Outline;
    panel.defs = vec![crate::syntax::Def { depth: 0, kind: 'f', name: "fn a".into(), range: 0..9, at: 0 }];
    let a = || Stop { path: Some(s.join("src/a.rs")), ..read("", 0..1) };
    let marks = |ed: &Editor| ed.workspace.viewer.sidebar.as_ref().unwrap().def_roles(0).len();
    add(&mut ed, vec![a()]);
    assert_eq!(marks(&ed), 1);
    [Code::Char('i'), Code::Char('x'), Code::Esc].into_iter().for_each(|c| ed.handle_key(Key::plain(c)));
    sync(&mut ed);
    add(&mut ed, vec![a()]);
    assert_eq!(marks(&ed), 0);
}

/// Your edit is the session's: another session's map keeps its signs on a file at the same path, a session with no map
/// between leaves it as it was, and only that session's next turn lets it go.
#[test]
fn your_edit_stays_with_its_session() {
    let s = crate::scratch::Scratch::new("turnmap-sessions");
    let mut ed = project(&s);
    let main = ed.workspace.id;
    add(&mut ed, vec![Stop { path: Some(s.join("src/a.rs")), ..read("", 0..2) }]);
    let checkout = |name: &str| {
        let root = s.join(name);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), CODE).unwrap();
        std::fs::canonicalize(root).unwrap()
    };
    let (fix_root, empty_root) = (checkout("fix"), checkout("empty"));
    let fix = ed.workspace_add(fix_root.clone());
    let empty = ed.workspace_add(empty_root.clone());
    assert!(ed.workspace_switch(fix).is_in());
    ed.open(&fix_root.join("src/a.rs")).unwrap();
    let mut c = crate::chat::Chat::new();
    c.follow.new_turn();
    ed.workspace.claude.chat = Some(c);
    add(&mut ed, vec![Stop { path: Some(fix_root.join("src/a.rs")), ..read("", 0..2) }]);
    let signed = |ed: &mut Editor| {
        sync(ed);
        row(&frame(ed, 120), "fn a()").contains('◦')
    };
    assert!(signed(&mut ed), "fix's own");
    use crate::key::{Code, Key};
    [Code::Char('i'), Code::Char('x'), Code::Esc].into_iter().for_each(|c| ed.handle_key(Key::plain(c)));
    assert!(!signed(&mut ed), "yours takes them");
    assert!(ed.workspace_switch(main).is_in());
    assert!(signed(&mut ed), "main's src/a.rs isn't the one you changed");
    assert!(ed.workspace_switch(empty).is_in());
    ed.open(&empty_root.join("src/a.rs")).unwrap();
    sync(&mut ed);
    assert!(ed.turnmap.is_empty());
    assert!(ed.workspace_switch(fix).is_in());
    assert!(!signed(&mut ed), "a session with no map between changes nothing");
    ed.workspace.claude.chat.as_mut().unwrap().follow.new_turn();
    add(&mut ed, vec![Stop { path: Some(fix_root.join("src/a.rs")), ..read("", 0..2) }]);
    assert!(signed(&mut ed), "its next turn's stops are where they say");
}

/// Your edit moves lines under every turn begun before it, on the map in sight or not: `[r` into one shows that file no
/// signs, its row staying, and a turn begun after it keeps its own — one begun anew after sitting empty too.
#[test]
fn your_edit_holds_for_every_turn_before_it() {
    use crate::key::{Code, Key};
    let s = crate::scratch::Scratch::new("turnmap-older");
    let mut ed = project(&s);
    let (a, b) = (s.join("src/a.rs"), s.join("src/b.rs"));
    let at = |p: &PathBuf| Stop { path: Some(p.clone()), ..read("", 0..2) };
    let next = |ed: &mut Editor| ed.workspace.claude.chat.as_mut().unwrap().follow.new_turn();
    let changed = |ed: &mut Editor| {
        [Code::Char('i'), Code::Char('x'), Code::Esc].into_iter().for_each(|c| ed.handle_key(Key::plain(c)));
        sync(ed);
    };
    let signed = |ed: &mut Editor, stop: Option<(usize, usize)>| {
        match stop {
            Some(stop) => crate::follow::replay_at(ed, stop),
            None => ed.workspace.claude.chat.as_mut().unwrap().follow.replay.at = None,
        }
        sync(ed);
        row(&frame(ed, 120), "fn a()").contains('◦')
    };
    // A file only the turn before touched: not on the map in sight as you change it
    add(&mut ed, vec![at(&a)]);
    assert!(signed(&mut ed, Some((0, 0))), "the first turn's own");
    next(&mut ed);
    add(&mut ed, vec![at(&b)]);
    assert!(!ed.turnmap.files.contains_key(Path::new("src/a.rs")));
    changed(&mut ed);
    assert!(!signed(&mut ed, Some((0, 0))), "`[r` into it: its lines moved");
    assert!(ed.turnmap.files.contains_key(Path::new("src/a.rs")), "its row stays");
    // Then a new turn: it reads the file as it is now, the first stays as it was
    next(&mut ed);
    add(&mut ed, vec![at(&a)]);
    assert!(signed(&mut ed, None), "a turn begun after it");
    assert!(!signed(&mut ed, Some((0, 0))), "edit, new turn, `[r`");
    assert!(signed(&mut ed, Some((2, 0))));
    // A turn that did nothing, begun again by the next question: your edit between is before it
    next(&mut ed);
    changed(&mut ed);
    next(&mut ed);
    add(&mut ed, vec![at(&a)]);
    assert!(signed(&mut ed, None), "the reused turn began after it");
    assert!(!signed(&mut ed, Some((2, 0))), "the turn before it didn't");
}

/// Narrow, a row folds its marks to the strongest, its name giving way — never the mark. Folded, a folder says its
/// strongest and how many of its files the turn touched, of how many it holds.
#[test]
fn narrow_a_row_says_its_strongest_and_a_folded_folder_its_count() {
    let s = crate::scratch::Scratch::new("turnmap-narrow");
    let long = "src/a_name_that_fills_the_panels_whole_row_and_a_good_deal_more.rs";
    let mut ed = project_of(&s, &["src/a.rs", long, "src/deep/c.rs", "src/deep/d.rs"]);
    let at = |rel: &str, st: Stop| Stop { path: Some(s.join(rel)), ..st };
    add(
        &mut ed,
        vec![
            at("src/a.rs", read("", 0..1)),
            at("src/a.rs", search("", 0)),
            at("src/a.rs", edit("", 0..1)),
            at(long, read("", 0..1)),
            at(long, edit("", 0..1)),
            at("src/deep/c.rs", edit("", 0..1)),
        ],
    );
    let rows = frame(&mut ed, 120);
    assert!(row(&rows, "a.rs ").contains("● • ◦"), "{}", row(&rows, "a.rs "));
    assert!(row(&rows, "deep/").contains("● 1/2"), "{}", row(&rows, "deep/"));
    let cut = row(&rows, "a_name");
    assert!(cut.contains('●') && !cut.contains('◦'), "the strongest alone: {cut}");
}

/// What a fit leaves of a button: all of it, its glyph with its count cut — the end inside the `…` — or nothing with its
/// glyph gone, by the end of the text or under a `…` at the same bytes.
#[test]
fn a_buttons_range_is_checked_against_the_fit() {
    let (whole, glyph, press) = ("◦ 9+  M", 0..3, 0..6);
    assert_eq!(press_kept(whole, whole, &glyph, &press), Some(0..6));
    assert_eq!(press_kept(whole, "◦ 9+ …", &glyph, &press), Some(0..6));
    assert_eq!(press_kept(whole, "◦ 9…", &glyph, &press), Some(0..3), "its end inside the `…`");
    assert_eq!(press_kept(whole, "", &glyph, &press), None);
    let (whole, glyph, press) = ("● ◦ 2", 4..7, 4..9);
    assert_eq!(press_kept(whole, "● …", &glyph, &press), None, "a `…` where its glyph was");
    assert_eq!(press_kept(whole, "●…", &glyph, &press), None, "past the end");
}

/// A press's range is of the whole hint, the row draws its fit: a deep folded folder with ten reads, `◦ 9+ 1/1`, cut to
/// `◦ 9…` has the range's end inside the `…` — sliced there, a panic, and release aborts on one. At every width the panel
/// takes, what's left of a mark is a button on its glyph and goes to its stop; the `…` no button.
#[test]
fn a_mark_cut_short_stays_a_button_on_its_glyph() {
    let s = crate::scratch::Scratch::new("turnmap-cut");
    let dirs = ["src", "src/p", "src/p/q", "src/p/q/r", "src/p/q/r/s", "src/p/q/r/s/t", "src/p/q/r/s/t/u"];
    let deep = "src/p/q/r/s/t/u/v/x.rs";
    let mut ed = project_of(&s, &["src/a.rs", deep]);
    ed.workspace.viewer.sidebar.as_mut().unwrap().set_open(dirs, true);
    add(&mut ed, (0..10).map(|l| Stop { path: Some(s.join(deep)), ..read("", l..l + 1) }).collect());
    let mut cut = 0;
    for side in crate::sizes::SIDE_MIN..=crate::sizes::SIDE_MAX {
        ed.sizes.side = Some(side as u16);
        let rows = frame(&mut ed, 120);
        let Some((y, _)) = rows.iter().enumerate().find(|(_, r)| r.contains("◦ 9…")) else { continue };
        cut += 1;
        let marks = ed.screen.map_marks.borrow().clone();
        let m = marks.iter().find(|m| m.0 == y).unwrap_or_else(|| panic!("no button at {side}: {marks:?}"));
        let glyph = rows[y].chars().nth(m.1).unwrap();
        assert_eq!(glyph, '◦', "{}", rows[y]);
        assert!(matches!(ed.kind_at(m.1, y), crate::kinds::Kind::Button(_)), "{:?}", ed.kind_at(m.1, y));
        let spots = ed.screen.word_spots.borrow();
        let on = spots.iter().find(|s| s.y == y && s.from == m.1).unwrap();
        assert_eq!(on.to, m.1 + 1, "its glyph alone, never the `…`: {}", rows[y]);
        drop(spots);
        press(&mut ed, m.1, y);
        let hand = ed.workspace.claude.chat.as_ref().unwrap().follow.replay.at;
        assert_eq!(hand, Some((ed.turnmap.turn, 0)), "its first stop");
        crate::follow::replay_step(&mut ed, true);
        ed.workspace.claude.chat.as_mut().unwrap().follow.replay.at = None;
    }
    assert!(cut > 0, "no width cut it inside its count");
    // (wide, its count is part of it)
    ed.sizes.side = Some(crate::sizes::SIDE_MAX as u16);
    let rows = frame(&mut ed, 240);
    let y = rows.iter().position(|r| r.contains("◦ 9+")).unwrap();
    let spots = ed.screen.word_spots.borrow();
    let on = spots.iter().find(|s| s.y == y && s.on == crate::mouse::Surface::Side).unwrap();
    assert_eq!(on.to - on.from, 4, "{}", rows[y]);
}

/// Narrow, the panel's tabs stay whole: `turn 14` is a detail of its top line, folded before them as details are — whole
/// or gone, what it's of and `· 1 outside` going first. What the turn asked is on its card; a turn with nothing on the map
/// names none, and the question stays off what the panel is of.
#[test]
fn the_tabs_stay_whole_and_the_turn_folds_first() {
    // (a short name: what the panel is of beside the whole turn at its widest)
    let s = crate::scratch::Scratch::new("t");
    let mut ed = project(&s);
    let asked = |ed: &Editor| {
        let spots = ed.screen.cut_spots.borrow();
        spots.iter().any(|c| matches!(&c.3, crate::whole::Cut::Text(w) if w.text.contains("why does")))
    };
    sync(&mut ed);
    ed.sizes.side = Some(crate::sizes::SIDE_MAX as u16);
    frame(&mut ed, 240);
    assert!(ed.turnmap.asked.is_some() && !asked(&ed), "no stop yet: nothing of the turn on the line");
    let top = |ed: &mut Editor, side: usize| {
        ed.sizes.side = Some(side as u16);
        // (wide enough for the panel at every width it takes, with the chat open)
        let line = frame(ed, 240).swap_remove(0);
        line.chars().take(side - 1).collect::<String>()
    };
    let outside = stop("read", Some("/elsewhere/x.rs".into()), Some(0..1));
    add(&mut ed, vec![Stop { path: Some(s.join("src/a.rs")), ..read("", 0..1) }, outside]);
    // (the 31 of a screen with the chat open: nothing beside the tabs)
    let line = top(&mut ed, 31);
    assert!(line.contains("files · outline · git") && !line.contains("turn"), "{line}");
    for side in 26..=crate::sizes::SIDE_MAX {
        let line = top(&mut ed, side);
        assert!(line.contains("files · outline · git"), "at {side}: {line}");
        assert!(!line.contains("turn") || line.contains("turn 14"), "whole or gone, at {side}: {line}");
    }
    let narrow = top(&mut ed, 38);
    assert!(narrow.contains("turn 14") && !narrow.contains("outside"), "a part folds first: {narrow}");
    assert!(asked(&ed), "what it asked, on its card");
    let wide = top(&mut ed, crate::sizes::SIDE_MAX);
    assert!(wide.contains("turn 14 · 1 outside"), "{wide}");
}

/// The map is folded when its stops or the stop in hand change, never by a frame or a sync — the budget of work every
/// frame and event leans on: `made` counts the folds.
#[test]
fn the_map_is_folded_once_a_change() {
    let s = crate::scratch::Scratch::new("turnmap-work");
    let mut ed = project(&s);
    add(&mut ed, vec![Stop { path: Some(s.join("src/a.rs")), ..read("", 0..1) }]);
    let made = ed.turnmap.made;
    for _ in 0..3 {
        frame(&mut ed, 120);
        sync(&mut ed);
    }
    assert_eq!(ed.turnmap.made, made, "nothing changed");
    add(&mut ed, vec![Stop { path: Some(s.join("src/b.rs")), ..read("", 0..1) }]);
    assert_eq!(ed.turnmap.made, made + 1, "one stop, one fold");
    crate::follow::replay_step(&mut ed, false);
    sync(&mut ed);
    assert_eq!(ed.turnmap.made, made + 2, "the stop in hand");
    frame(&mut ed, 120);
    sync(&mut ed);
    assert_eq!(ed.turnmap.made, made + 2, "its frame folds nothing");
}

// ── Each call's every place is a stop ───────────────────────────────────────────────────────────

fn places(ed: &Editor) -> Vec<(String, Option<Range<usize>>)> {
    let replay = &ed.workspace.claude.chat.as_ref().unwrap().follow.replay;
    let rel = |s: &Stop| {
        let p = s.path.as_deref().and_then(|p| p.strip_prefix(&ed.workspace.root).ok());
        p.unwrap().display().to_string()
    };
    let stops = replay.turns().back().map_or(&[][..], |t| &t.stops[..]);
    stops.iter().map(|s| (rel(s), s.lines.clone())).collect()
}

/// A batched read is a stop a range: the map and `]r` have every place it read, the strip's mark stays one, a press on
/// its row goes to its first.
#[test]
fn a_batched_read_is_a_stop_a_place() {
    let s = crate::scratch::Scratch::new("turnmap-read");
    let mut ed = project(&s);
    let input = serde_json::json!({ "files": ["src/a.rs:1-2,5-6", "src/b.rs", "https://a.test"] });
    let look = crate::chat::look(&ed.workspace.root, "mcp__tarae__read", &input);
    crate::follow::on_tool(&mut ed, "r1", "mcp__tarae__read", &input, &look);
    let want =
        [("src/a.rs".to_string(), Some(0..2)), ("src/a.rs".into(), Some(4..6)), ("src/b.rs".into(), None)];
    assert_eq!(places(&ed), want);
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert_eq!(crate::follow::strip(c).len(), 1, "one mark a call");
    assert_eq!(c.follow.replay.place_of("r1").map(|p| p.1), Some(0), "its row goes to its first");
    sync(&mut ed);
    let f = &ed.turnmap.files;
    assert_eq!(f[Path::new("src/a.rs")].roles[0].said(), "◦ 2");
    assert_eq!(f[Path::new("src/b.rs")].roles[0].said(), "◦");
}

/// A read by name has its lines from its answer: the outline's mark needs them.
#[test]
fn a_read_by_name_takes_its_lines_from_the_answer() {
    let s = crate::scratch::Scratch::new("turnmap-named");
    let mut ed = project(&s);
    let input = serde_json::json!({ "files": ["src/a.rs:run", "src/b.rs:1-2"] });
    let look = crate::chat::look(&ed.workspace.root, "mcp__tarae__read", &input);
    crate::follow::on_tool(&mut ed, "r1", "mcp__tarae__read", &input, &look);
    let answer = "== src/a.rs:run — fn run (of 6 lines)\n4: fn d() {}\n5: fn e() {}\n== src/b.rs:1-2 (of 6 lines)\n1: x\n2: y\n";
    crate::follow::on_result(&mut ed, "r1", answer, false);
    assert_eq!(places(&ed)[0], ("src/a.rs".to_string(), Some(3..5)));
    // (a whole file, through the same way: no lines of its own, not its first)
    let input = serde_json::json!({ "files": ["src/b.rs"] });
    let look = crate::chat::look(&ed.workspace.root, "mcp__tarae__read", &input);
    crate::follow::on_tool(&mut ed, "r2", "mcp__tarae__read", &input, &look);
    assert_eq!(places(&ed).last(), Some(&("src/b.rs".to_string(), None)));
}

/// An edit of several files is a stop each — before the map, every file after its first was lost — its row and the view
/// its first's as they were; a search, each file's first hit.
#[test]
fn an_edit_and_a_search_are_a_stop_a_file() {
    let s = crate::scratch::Scratch::new("turnmap-edit");
    let mut ed = project(&s);
    let input = serde_json::json!({ "edits": [{ "file_path": "src/a.rs" }, { "file_path": "src/b.rs" }] });
    let look = crate::chat::look(&ed.workspace.root, "mcp__tarae__edit", &input);
    crate::follow::on_tool(&mut ed, "e1", "mcp__tarae__edit", &input, &look);
    let (before, after) = (ropey::Rope::from_str(CODE), ropey::Rope::from_str(&CODE.replace("fn b", "fn B")));
    crate::follow::edit_landed(&mut ed, "e1", &s.join("src/a.rs"), &before, &after);
    crate::follow::edit_landed(&mut ed, "e1", &s.join("src/b.rs"), &before, &after);
    assert_eq!(places(&ed), [("src/a.rs".to_string(), Some(1..2)), ("src/b.rs".into(), Some(1..2))]);
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert_eq!(crate::follow::strip(c).len(), 1, "one mark a call");
    let f = |ed: &Editor, p: &str| ed.turnmap.files.get(Path::new(p)).map(|f| f.roles[0].said());
    sync(&mut ed);
    assert_eq!(f(&ed, "src/b.rs").as_deref(), Some("●"));

    let input = serde_json::json!({ "pattern": "fn" });
    let look = crate::chat::look(&ed.workspace.root, "mcp__tarae__search", &input);
    crate::follow::on_tool(&mut ed, "s1", "mcp__tarae__search", &input, &look);
    let answer = "src/deep/c.rs\n  3: fn c\n  4: fn d\nsrc/a.rs\n  5: fn e\n";
    crate::follow::on_result(&mut ed, "s1", answer, false);
    let found: Vec<_> = places(&ed).into_iter().skip(2).collect();
    assert_eq!(found, [("src/deep/c.rs".to_string(), Some(2..3)), ("src/a.rs".into(), Some(4..5))]);
}

/// `run_tests`' target reaches its stop: what "run over" reads, by the words its row shows, never another's.
#[test]
fn a_run_tests_target_reaches_its_stop() {
    let s = crate::scratch::Scratch::new("turnmap-target");
    let mut ed = project(&s);
    let input = serde_json::json!({ "path": "src/a.rs", "line": 4 });
    let look = crate::chat::look(&ed.workspace.root, "mcp__tarae__run_tests", &input);
    crate::follow::on_tool(&mut ed, "t1", "mcp__tarae__run_tests", &input, &look);
    let how = crate::testing::How::Pack { lang: "rust".into(), argv: Vec::new(), lines: None, junit: None };
    let target = crate::testing::Target { label: "t".into(), cwd: s.to_path_buf(), how, holds: Some(3..5) };
    crate::follow::test_holds(&mut ed, "src/b.rs", &target);
    let held = |ed: &Editor| {
        ed.workspace.claude.chat.as_ref().unwrap().follow.replay.turns().back().unwrap().stops[0]
            .map
            .holds
            .clone()
    };
    assert_eq!(held(&ed), None, "another's");
    crate::follow::test_holds(&mut ed, "src/a.rs:4", &target);
    assert_eq!(held(&ed), Some(Holds::Lines(3..5)));
    // (with no line it runs the file's tests: said at the call, whatever comes of finding them)
    let input = serde_json::json!({ "path": "src/a.rs" });
    let look = crate::chat::look(&ed.workspace.root, "mcp__tarae__run_tests", &input);
    crate::follow::on_tool(&mut ed, "t2", "mcp__tarae__run_tests", &input, &look);
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let stops = &c.follow.replay.turns().back().unwrap().stops;
    assert_eq!(stops.last().unwrap().map.holds, Some(Holds::File));
}

/// A file the turn removed or moved is changed on the map — `refactor::landed`'s stops, each end of a move its own, the
/// git tab's row for the one gone; a file of yours landing says nothing.
#[test]
fn a_removed_or_moved_file_lands_on_the_map() {
    use crate::refactor::{FileChange, Op, Whose};
    let s = crate::scratch::Scratch::new("turnmap-gone");
    let mut ed = project(&s);
    let input = serde_json::json!({ "edits": [{ "file_path": "src/b.rs", "delete": true }] });
    let look = crate::chat::look(&ed.workspace.root, "mcp__tarae__edit", &input);
    crate::follow::on_tool(&mut ed, "e1", "mcp__tarae__edit", &input, &look);
    crate::follow::on_tool(&mut ed, "e2", "mcp__tarae__edit", &input, &look);
    let change = |rel: &str, op: Op| {
        let (before, after) = (ropey::Rope::from_str(CODE), ropey::Rope::from_str(CODE));
        FileChange { path: s.join(rel), rel: rel.into(), before, after, places: 1, op, protected: false }
    };
    crate::refactor::landed(&mut ed, &change("src/b.rs", Op::Delete), None, Whose::Claude(1), Some("e1"));
    let moved = Op::Move { to: s.join("src/new.rs"), to_rel: "src/new.rs".into() };
    crate::refactor::landed(&mut ed, &change("src/deep/c.rs", moved), None, Whose::Claude(1), Some("e2"));
    crate::refactor::landed(&mut ed, &change("src/a.rs", Op::Delete), None, Whose::Yours, Some("e1"));
    sync(&mut ed);
    for rel in ["src/b.rs", "src/deep/c.rs", "src/new.rs"] {
        assert_eq!(ed.turnmap.files[Path::new(rel)].roles[0].mark, Mark::Edit, "{rel}");
    }
    assert!(!ed.turnmap.files.contains_key(Path::new("src/a.rs")), "yours isn't the turn's");
}

/// The stops the sign column reads are found by line, the heaviest on it first and the one in hand over all.
#[test]
fn a_lines_sign_is_its_heaviest_the_one_in_hand_over_all() {
    let sign = |mark: Mark, stop: usize, in_hand: bool| Sign { lines: 0..3, mark, at: (0, stop), in_hand };
    let signs = [sign(Mark::Read, 0, false), sign(Mark::Edit, 1, false), sign(Mark::Read, 2, false)];
    assert_eq!(sign_at(&signs, 1).map(|s| s.mark), Some(Mark::Edit));
    assert_eq!(sign_at(&signs, 9), None);
    let signs = [sign(Mark::Read, 0, true), sign(Mark::Edit, 1, false)];
    assert_eq!(sign_at(&signs, 1).map(|s| s.mark), Some(Mark::Read));
}
