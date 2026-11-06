//! #375 step 2: your opens read on a worker (`opening.rs`). `read_inline` off in each: the worker's path is the subject.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

/// Reads held by a test (the path, read only once its sender sends or goes): `read` waits there before it reads.
static HELD: Mutex<Vec<(PathBuf, Receiver<()>)>> = Mutex::new(Vec::new());

/// The read of `path` (as asked) waits for the sender: dropped or sent, it goes on.
fn hold(path: &Path) -> Sender<()> {
    let (tx, rx) = channel();
    HELD.lock().unwrap().push((path.to_path_buf(), rx));
    tx
}

/// `read`'s hook: a read a test holds waits until it's let go (at most the shared patience).
pub(super) fn held(path: &Path) {
    let rx = {
        let mut h = HELD.lock().unwrap();
        let Some(i) = h.iter().position(|(p, _)| p == path) else { return };
        h.remove(i).1
    };
    let _ = rx.recv_timeout(crate::test_results::PATIENCE);
}

use crate::editor::Editor;
use crate::scratch::Scratch;
use std::time::{Duration, Instant};

const A: &str = "// alpha's own line\nfn alpha() {}\n";
const B: &str = "fn beta() -> u32 {\n    let x = 1;\n    x + 1\n}\n";

/// a.rs open and in front (read right there), c.rs open behind it, b.rs and d.rs on disk only; `read_inline` off.
fn scene(name: &str) -> (Scratch, PathBuf, Editor) {
    let s = Scratch::new(name);
    let root = std::fs::canonicalize(s.path()).unwrap();
    std::fs::write(root.join("a.rs"), A).unwrap();
    std::fs::write(root.join("b.rs"), B).unwrap();
    std::fs::write(root.join("c.rs"), "// gamma\n").unwrap();
    std::fs::write(root.join("d.rs"), "// delta\n").unwrap();
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    ed.open(&root.join("c.rs")).unwrap();
    ed.open(&root.join("a.rs")).unwrap();
    ed.read_inline = false;
    (s, root, ed)
}

fn frame(ed: &mut Editor) -> vt100::Parser {
    crossterm::style::force_color_output(true);
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, 100, 24).unwrap();
    let mut p = vt100::Parser::new(24, 100, 0);
    p.process(&bytes);
    p
}

fn rows(p: &vt100::Parser) -> Vec<String> {
    p.screen().rows(0, 100).collect()
}

/// Results handled as the loop does, until `done` (at most the shared patience).
fn until(ed: &mut Editor, done: impl Fn(&Editor) -> bool) {
    let end = Instant::now() + crate::test_results::PATIENCE;
    while !done(ed) {
        assert!(Instant::now() < end, "timed out waiting");
        if let Some(ev) = ed.events.recv_timeout(Duration::from_millis(20)) {
            ed.handle_event(ev);
        }
    }
}

/// Results handled for `d` of real time (what a test lets land, a dropped one too).
fn for_a_while(ed: &mut Editor, d: Duration) {
    let end = Instant::now() + d;
    while Instant::now() < end {
        if let Some(ev) = ed.events.recv_timeout(Duration::from_millis(20)) {
            ed.handle_event(ev);
        }
    }
}

fn front(ed: &Editor) -> String {
    ed.doc()
        .path
        .as_deref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn listed(ed: &Editor, name: &str) -> bool {
    ed.buffers().iter().any(|&i| ed.docs[i].path.as_deref().is_some_and(|p| p.ends_with(name)))
}

/// The cell of `text`'s first character on the screen.
fn cell_of(p: &vt100::Parser, text: &str) -> (u16, u16) {
    let r = rows(p);
    let row = r.iter().position(|l| l.contains(text)).unwrap_or_else(|| panic!("{text}: {r:#?}"));
    let col = r[row][..r[row].find(text).unwrap()].chars().count();
    (row as u16, col as u16)
}

/// `:open b.rs`: the frame right after the key is still a.rs — its lines, no b.rs named anywhere — and b.rs isn't a
/// buffer yet.
#[test]
fn the_code_in_front_stays_until_the_file_lands() {
    let (_s, root, mut ed) = scene("open-stays");
    let a = ed.doc().id;
    let held = hold(&root.join("b.rs"));
    crate::typed::execute(&mut ed, &format!("open {}", root.join("b.rs").display())).unwrap();
    assert_eq!(ed.doc().id, a, "a.rs stays in front");
    assert!(!listed(&ed, "b.rs"), "no buffer before its text");
    let r = rows(&frame(&mut ed));
    assert!(r.iter().any(|l| l.contains("alpha's own line")), "a.rs's lines: {r:#?}");
    assert!(!r.iter().any(|l| l.contains("b.rs") || l.contains("fn beta")), "nothing of b.rs: {r:#?}");
    drop(held);
}

/// Once its read lands b.rs is in front with its tree in, so its first frame is coloured (its `u32` in another colour
/// than with no tree).
#[test]
fn a_file_lands_in_front_with_its_tree_its_first_frame_coloured() {
    let (_s, root, mut ed) = scene("open-coloured");
    crate::typed::execute(&mut ed, &format!("open {}", root.join("b.rs").display())).unwrap();
    until(&mut ed, |ed| ed.opening.is_none());
    assert_eq!(front(&ed), "b.rs");
    let tree = ed.doc().syntax.as_ref().is_some_and(|s| s.tree.is_some() && !s.dirty);
    assert!(tree, "its tree came with its text");
    let p = frame(&mut ed);
    // (`u32`, a type: never under the cursor, which stands at the text's start)
    let (row, col) = cell_of(&p, "u32");
    let coloured = p.screen().cell(row, col).unwrap().fgcolor();
    ed.doc_mut().syntax = None;
    let plain = frame(&mut ed).screen().cell(row, col).unwrap().fgcolor();
    assert_ne!(coloured, plain, "its first frame is coloured");
}

/// Another file in front before b.rs lands: b.rs joins the buffers, said, and c.rs stays in front.
#[test]
fn a_file_landing_after_you_went_elsewhere_joins_the_buffers_said() {
    let (_s, root, mut ed) = scene("open-behind");
    let held = hold(&root.join("b.rs"));
    ed.open_file(&root.join("b.rs"));
    ed.cycle_buffer(-1);
    assert_eq!(front(&ed), "c.rs");
    drop(held);
    until(&mut ed, |ed| ed.opening.is_none());
    assert_eq!(front(&ed), "c.rs", "never put in front of you");
    assert!(listed(&ed, "b.rs"), "among your buffers");
    let said = ed.messages.last().map(|m| m.2.clone()).unwrap_or_default();
    assert!(said.contains("b.rs is ready — space b"), "{said}");
}

/// A second open under way before the first lands: only the second comes; the first lands as nothing.
#[test]
fn of_two_opens_only_the_second_lands() {
    let (_s, root, mut ed) = scene("open-two");
    let held = hold(&root.join("b.rs"));
    ed.open_file(&root.join("b.rs"));
    ed.open_file(&root.join("d.rs"));
    until(&mut ed, |ed| ed.opening.is_none());
    assert_eq!(front(&ed), "d.rs");
    drop(held);
    for_a_while(&mut ed, Duration::from_millis(300));
    assert_eq!(front(&ed), "d.rs");
    assert!(
        !ed.docs.iter().any(|d| d.path.as_deref().is_some_and(|p| p.ends_with("b.rs"))),
        "b.rs made nothing"
    );
}

/// A read held past `SAY_AFTER`: the step said on the status line with its time, gone as the file lands.
#[test]
fn an_open_past_its_moment_is_said_with_its_time_then_gone() {
    let (_s, root, mut ed) = scene("open-said");
    let held = hold(&root.join("b.rs"));
    ed.open_file(&root.join("b.rs"));
    let status = |ed: &mut Editor| rows(&frame(ed)).last().cloned().unwrap_or_default();
    assert!(!status(&mut ed).contains("opening"), "unsaid at first");
    until(&mut ed, |ed| ed.opening.as_ref().is_some_and(|o| o.said));
    let line = status(&mut ed);
    assert!(line.contains("opening b.rs…"), "{line}");
    assert!(
        line.split("opening b.rs…")
            .nth(1)
            .is_some_and(|t| t.trim_start().starts_with(['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'])),
        "its time: {line}"
    );
    drop(held);
    until(&mut ed, |ed| ed.opening.is_none());
    let line = status(&mut ed);
    assert!(!line.contains("opening"), "gone as it lands: {line}");
    assert_eq!(front(&ed), "b.rs");
}

/// `gd` into a file not open: the cursor at the definition in the very event its read lands.
#[test]
fn a_go_to_into_a_file_not_open_lands_at_its_place() {
    let (_s, root, mut ed) = scene("open-gd");
    let pos = serde_json::json!({ "line": 2, "character": 4 });
    ed.lsp_jump(&root.join("b.rs"), &pos, crate::lsp::Encoding::Utf8);
    assert_eq!(front(&ed), "a.rs", "not yet");
    until(&mut ed, |ed| ed.opening.is_none());
    assert_eq!(front(&ed), "b.rs");
    let d = ed.doc();
    let at = d.selection().primary().cursor(&d.text);
    assert_eq!(at, d.text.line_to_byte(2) + 4, "at `x + 1`'s `x`");
    assert!(d.jumped, "a jump: the view centres it if it's out of sight");
}

/// A project replace over two files not open reads both first, then applies both.
#[test]
fn a_replace_over_files_not_open_applies_to_each() {
    let (_s, root, mut ed) = scene("open-replace");
    let (b, d) = (root.join("b.rs"), root.join("d.rs"));
    std::fs::write(&d, "// delta beta\n").unwrap();
    let targets = vec![(b.clone(), vec![0]), (d.clone(), vec![0])];
    ed.replace_plan = crate::replace::plan("beta", "gamma", &targets, &Default::default()).unwrap();
    ed.replace_apply(&[0, 1]);
    assert!(!listed(&ed, "b.rs") && !listed(&ed, "d.rs"), "read on a worker first");
    until(&mut ed, |ed| listed(ed, "b.rs") && listed(ed, "d.rs"));
    let text = |p: &Path| ed.docs.iter().find(|x| x.path.as_deref() == Some(p)).map(|x| x.text.to_string());
    assert_eq!(text(&b).unwrap().lines().next(), Some("fn gamma() -> u32 {"));
    assert_eq!(text(&d).unwrap(), "// delta gamma\n");
    assert_eq!(front(&ed), "a.rs", "what was in front stays");
}
