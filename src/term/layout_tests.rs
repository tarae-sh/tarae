//! The layout matrix (onetree S0, `.sb/onetree-plan.md` §4): today's layout pinned before the one-tree steps move it
//! into one rule. Every scene of `SCENES` at every size of `WIDTHS` × `HEIGHTS`, one frame each: the tree's leaves as
//! laid out (kind and rect), `Screen`'s home scalars, and the lines drawn — each row's `─` runs and joins, each
//! column's `│` runs. Printed as text into `src/snapshots/layout_matrix.txt`; `TARAE_BLESS=1 cargo test layout_matrix`
//! takes a change that's meant. S1–S4 change nothing seen: the file stays as it is through them.
//!
//! The terminal's panel isn't a scene: it needs a pty, which a sandboxed run has none of (`snapshot_terminal_panel`
//! draws it where it can).
//!
//! `perf_layout_frame` times what the layout costs a frame over the same matrix: `Layout::new` and the code's edges.

use super::{Layout, code_edges, render};
use crate::config::Config;
use crate::editor::Editor;
use crate::key::{Code, Key};
use crate::pane::GroupKind;
use crate::workspace::At;

const WIDTHS: [u16; 6] = [54, 80, 100, 120, 160, 200];
const HEIGHTS: [u16; 3] = [20, 34, 50];

/// A scene: its name and what sets it up on a fresh editor (`editor`), keys fed at the frame's size.
type Scene = (&'static str, fn(&mut Editor, u16, u16));

const SCENES: &[Scene] = &[
    ("code", |_, _, _| {}),
    ("code beside code", |ed, w, h| keys(ed, "<C-w>v", w, h)),
    ("code over code", |ed, w, h| keys(ed, "<C-w>s", w, h)),
    ("chat", |ed, _, _| chat(ed, At::Code)),
    ("chat with the keys", |ed, _, _| chat(ed, At::Chat)),
    ("chat hidden", |ed, _, _| {
        chat(ed, At::Code);
        ed.workspace.claude.chat.as_mut().unwrap().hidden = true;
    }),
    ("side panel", |ed, _, _| side(ed)),
    ("side panel and chat", |ed, _, _| {
        chat(ed, At::Code);
        side(ed);
    }),
    ("review", |ed, _, _| review(ed)),
    ("review and chat", |ed, _, _| {
        chat(ed, At::Code);
        review(ed);
    }),
    ("slot: tests", |ed, _, _| tests(ed)),
    ("slot: task", |ed, _, _| task(ed)),
    ("slot: tests, side panel and chat", |ed, _, _| {
        chat(ed, At::Code);
        side(ed);
        tests(ed);
        ed.workspace.viewer.at = At::Code;
    }),
    ("tests docked left, chat", |ed, w, h| docked(ed, "H", w, h)),
    ("tests docked right, chat", |ed, w, h| docked(ed, "L", w, h)),
    ("tests docked over the code, chat", |ed, w, h| docked(ed, "K", w, h)),
    ("code zoomed, chat", |ed, w, h| {
        chat(ed, At::Code);
        keys(ed, "<C-w>z", w, h);
    }),
    ("chat zoomed", |ed, w, h| {
        chat(ed, At::Chat);
        keys(ed, "<C-w>z", w, h);
    }),
];

/// A fresh editor on 30 lines of plain text, its name pinned (a line that names it lays out the same anywhere) and a
/// stand-in for claude, never the real one.
fn editor() -> Editor {
    let mut ed = Editor::new(Config::default());
    ed.docs[0].text = ropey::Rope::from_str(&(0..30).map(|i| format!("line {i}\n")).collect::<String>());
    ed.workspace.root = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    (ed.workspace.name, ed.workspace.git_branch) = ("main".into(), None);
    ed.config.llm.command = "sh".into();
    ed.config.llm.args = vec!["-c".into(), "cat > /dev/null".into()];
    ed
}

/// A frame `w` × `h`, as the main loop draws one after an event.
fn frame(ed: &mut Editor, w: u16, h: u16) {
    ed.focus_sync();
    render(ed, &mut Vec::new(), w, h).unwrap();
}

/// Keys, a frame after each (`<C-w>` as written).
fn keys(ed: &mut Editor, keys: &str, w: u16, h: u16) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(key);
        frame(ed, w, h);
    }
}

/// The chat open, empty, the keys `at`.
fn chat(ed: &mut Editor, at: At) {
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.viewer.at = at;
}

fn side(ed: &mut Editor) {
    ed.sidebar_show(crate::sidebar::Mode::Outline);
}

/// A code review of one file, its rail up with the keys.
fn review(ed: &mut Editor) {
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
    ed.workspace.viewer.at = At::Rail;
}

/// A finished run of two tests, one failed, in the slot.
fn tests(ed: &mut Editor) {
    use crate::test_results::{Case, Status};
    let case = |name: &str, status, failed: bool| Case {
        name: name.into(),
        status,
        secs: None,
        message: if failed { vec!["left != right".into()] } else { Vec::new() },
        at: None,
    };
    let cases = vec![case("adds", Status::Passed, false), case("subtracts", Status::Failed, true)];
    let run = crate::testing::TestRun::finished("math", cases, vec!["test result: FAILED".into()]);
    ed.workspace.test_run = Some(run);
}

/// A task that passed, in the slot.
fn task(ed: &mut Editor) {
    let t = crate::task::Task { name: "check".into(), cmd: "true".into(), cwd: ed.workspace.root.clone() };
    let mut r = crate::task::Run::for_test(t);
    r.state = crate::task::State::Passed;
    ed.workspace.task_run = Some(r);
}

/// The tests' group docked by `C-w <side>` from its list, a task left in the slot, the chat open.
fn docked(ed: &mut Editor, side: &str, w: u16, h: u16) {
    chat(ed, At::Code);
    task(ed);
    frame(ed, w, h);
    tests(ed);
    frame(ed, w, h);
    ed.list_focus(GroupKind::Tests);
    frame(ed, w, h);
    keys(ed, &format!("<C-w>{side}"), w, h);
}

/// The editor of scene `scene` at `w` × `h`, its last frame drawn.
fn scene(scene: &Scene, w: u16, h: u16) -> Editor {
    let mut ed = editor();
    frame(&mut ed, w, h);
    (scene.1)(&mut ed, w, h);
    frame(&mut ed, w, h);
    ed
}

/// What a pane shows, by its name in the state (`code`, `chat`, `list:tests`).
fn kind(ed: &Editor, id: crate::split::PaneId) -> String {
    ed.workspace.shows(id).map_or("?".into(), |s| s.name())
}

/// Box drawing's lines: `─` runs along a row, the rest joins or corners (`│` is a column's).
fn line_glyph(c: char) -> bool {
    matches!(c, '─' | '┬' | '┴' | '├' | '┤' | '┼' | '╭' | '╮' | '╰' | '╯' | '┌' | '┐' | '└' | '┘')
}

/// The frame's lines: each row's `─` runs (`─a-b`) and its joins where they are (`┬72`), then each column's `│` runs
/// (with the joins in them, so a line through a `┤` is one run).
fn lines(ed: &Editor, w: u16, h: u16) -> Vec<String> {
    let frame = &ed.last_frame.as_ref().expect("a frame").0;
    let text = crate::screen::text(frame, w, h).expect("a frame that reads");
    let grid: Vec<Vec<char>> = text.lines().map(|l| l.chars().collect()).collect();
    let at = |x: usize, y: usize| grid.get(y).and_then(|r| r.get(x)).copied().unwrap_or(' ');
    let mut out = Vec::new();
    for (y, row) in grid.iter().enumerate() {
        let mut parts = Vec::new();
        let mut x = 0;
        while x < row.len() {
            match row[x] {
                '─' => {
                    let start = x;
                    while x + 1 < row.len() && row[x + 1] == '─' {
                        x += 1;
                    }
                    parts.push(format!("─{start}-{x}"));
                }
                c if line_glyph(c) => parts.push(format!("{c}{x}")),
                _ => {}
            }
            x += 1;
        }
        if !parts.is_empty() {
            out.push(format!("row {y:>2}  {}", parts.join(" ")));
        }
    }
    let width = grid.iter().map(Vec::len).max().unwrap_or(0);
    let vertical = |c: char| c == '│' || (line_glyph(c) && c != '─');
    for x in 0..width {
        let mut parts = Vec::new();
        let mut y = 0;
        while y < grid.len() {
            // (a run of line cells down the column, the joins it hangs from and ends at in it — one with a `│` in it)
            let start = y;
            while y < grid.len() && vertical(at(x, y)) {
                y += 1;
            }
            if (start..y).any(|y| at(x, y) == '│') {
                parts.push(format!("│{start}-{}", y - 1));
            }
            y += 1;
        }
        if !parts.is_empty() {
            out.push(format!("col {x:>3}  {}", parts.join(" ")));
        }
    }
    out
}

/// One scene at one size, as the matrix's file prints it.
fn printed(name: &str, ed: &Editor, w: u16, h: u16) -> String {
    let s = &ed.screen;
    let leaves: Vec<String> = (s.leaves.iter())
        .map(|(id, r)| format!("{}#{id} {},{} {}x{}", kind(ed, *id), r.x, r.y, r.w, r.h))
        .collect();
    let chat_x = s.chat_x().map_or("-".into(), |x| x.to_string());
    // (the home scalars as the matrix has always printed them, from the frame's rects by kind (onetree S1g, S1h))
    use crate::pane::Shows;
    let slot = s.slot();
    let mut out = format!(
        "## {w}x{h} · {name}\nleaves  {}\nscreen  chat_x {chat_x} · chat_w {} · side_w {} · rail_w {} · bottom_h {} · \
         body_h {} · text_top {} · slot_x {}\n",
        leaves.join(" · "),
        s.drawn_w(Shows::Chat),
        s.drawn_w(Shows::Side),
        s.drawn_w(Shows::Rail),
        slot.h,
        s.body_end() - s.text_top,
        s.text_top,
        slot.x,
    );
    for l in lines(ed, w, h) {
        out.push_str(&format!("  {l}\n"));
    }
    out
}

/// The whole matrix, every scene at every size.
fn matrix() -> String {
    let mut out = String::from(
        "# The layout matrix (onetree S0): every scene at every size — leaves (kind#id x,y wxh), Screen's home scalars,\n\
         # the lines drawn (a row's ─ runs and joins, a column's │ runs). TARAE_BLESS=1 cargo test layout_matrix\n\n",
    );
    for sc in SCENES {
        for &w in &WIDTHS {
            for &h in &HEIGHTS {
                let ed = scene(sc, w, h);
                out.push_str(&printed(sc.0, &ed, w, h));
                out.push('\n');
            }
        }
    }
    out
}

/// Today's layout, pinned (onetree S0): every scene at every size lays out its leaves, scalars and lines as
/// `src/snapshots/layout_matrix.txt` says — the gate S1–S4 keep (zero diffs).
#[test]
fn layout_matrix() {
    // (colours aren't read here, but the frame is drawn as the snapshots' are)
    crossterm::style::force_color_output(true);
    let actual = matrix();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/snapshots/layout_matrix.txt");
    if std::env::var_os("TARAE_BLESS").is_some_and(|v| v == "1") {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("no {} yet — TARAE_BLESS=1 cargo test layout_matrix", path.display()));
    if expected != actual {
        // (the first scene that differs, whole on both sides: a matrix's diff is read scene by scene)
        let (e, a): (Vec<&str>, Vec<&str>) =
            (expected.split("\n\n").collect(), actual.split("\n\n").collect());
        let at = e.iter().zip(&a).position(|(e, a)| e != a).unwrap_or(e.len().min(a.len()));
        let differ = e.iter().zip(&a).filter(|(e, a)| e != a).count() + e.len().abs_diff(a.len());
        panic!(
            "the layout matrix differs in {differ} scene(s) — the first, expected:\n{}\n\nactual:\n{}\n\n\
             If the change is meant: TARAE_BLESS=1 cargo test layout_matrix",
            e.get(at).unwrap_or(&"(none)"),
            a.get(at).unwrap_or(&"(none)"),
        );
    }
}

/// Each kind is sized along the split it sits in (onetree S2b): the conversation stacked under the code takes a third
/// of the column's rows, the slot beside the code 2/5 of the columns — trees today's layout never makes (S5's moves).
#[test]
fn each_kind_is_sized_along_the_split_it_sits_in() {
    use crate::pane::Shows;
    use crate::split::{Dir, Share, Split};
    let mut ed = editor();
    frame(&mut ed, 120, 50);
    chat(&mut ed, At::Code);
    let leaf = |ed: &Editor, s: Shows| Split::Leaf(ed.workspace.leaf(s).unwrap());
    let code = Split::Leaf(ed.workspace.views[0].id);
    let (side, rail, slot, conv) =
        (leaf(&ed, Shows::Side), leaf(&ed, Shows::Rail), leaf(&ed, Shows::Slot), leaf(&ed, Shows::Chat));
    // (the conversation under the code in its column: 48 rows under the path bar, no panel in the slot)
    let column = Split::Node(
        Dir::Horizontal,
        vec![(Share::Even, code.clone()), (Share::Auto, slot.clone()), (Share::Auto, conv.clone())],
    );
    ed.workspace.split = Split::Node(
        Dir::Vertical,
        vec![(Share::Auto, side.clone()), (Share::Auto, rail.clone()), (Share::Even, column)],
    );
    let rect = |ed: &Editor, s: Shows| {
        let id = ed.workspace.leaf(s).unwrap();
        Layout::new(ed, 120, 50).leaves.into_iter().find(|l| l.0 == id).unwrap().1
    };
    let c = rect(&ed, Shows::Chat);
    assert_eq!((c.w, c.h), (120, crate::sizes::across(crate::sizes::Across::Chat, 48)));
    assert_eq!(c.h, 16);
    // (the slot beside the code, the conversation as its column)
    tests(&mut ed);
    let column = Split::Node(Dir::Horizontal, vec![(Share::Even, code)]);
    ed.workspace.split = Split::Node(
        Dir::Vertical,
        vec![
            (Share::Auto, side),
            (Share::Auto, rail),
            (Share::Even, column),
            (Share::Auto, slot),
            (Share::Auto, conv),
        ],
    );
    assert_eq!(rect(&ed, Shows::Slot).w, 120 * 2 / 5);
    assert_eq!(rect(&ed, Shows::Chat).w, 48, "its home's way: the width the table gives it");
}

/// The layout costs a frame little (onetree S0's measure, S1 and S3 judged by it): `Layout::new` and the code's edges,
/// over every scene at 80, 120 and 200 × 34 — each scene's median of its runs, the median of those said for
/// `cargo perf`.
#[test]
fn perf_layout_frame() {
    if crate::test_results::unoptimized("term::layout_tests::perf_layout_frame") {
        return;
    }
    const RUNS: usize = 200;
    let mut medians = Vec::new();
    for sc in SCENES {
        for w in [80, 120, 200] {
            let h = 34;
            let ed = scene(sc, w, h);
            let mut times: Vec<std::time::Duration> = (0..RUNS)
                .map(|_| {
                    let t = std::time::Instant::now();
                    let lay = Layout::new(&ed, w, h);
                    std::hint::black_box(code_edges(&ed, &lay));
                    t.elapsed()
                })
                .collect();
            times.sort();
            medians.push(times[RUNS / 2]);
        }
    }
    medians.sort();
    let us = medians[medians.len() / 2].as_secs_f64() * 1e6;
    let line = format!("PERF layout_frame_us {us:.2} budget=1000 noise=2\n");
    let _ = std::io::Write::write_all(&mut std::io::stderr(), line.as_bytes());
}
