//! A jump's view (#387), simulated: jumps into a file (`open_at` at a line, as a picker's, `]q`'s or the chat's) and
//! into a big one still loading (its place taken as its text lands, a job), `C-o`, plain motions and the wheel, and
//! frames, in any order. At every frame drawn while a jump waits on it: a target the view already had in sight leaves
//! the view as it is (but the scroll margin's few rows), one out of sight is centred — and the jump is spent by that
//! frame, never left for a later motion's.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::key::{Code, Key};

const NAME: &str = "a_jump_is_centred_only_when_out_of_sight";

const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5];

const INPUTS: usize = 30;

const LIMIT: Duration = crate::test_results::PATIENCE;

const SIZE: (u16, u16) = (100, 30);

/// The small file's lines and the big one's (past `ASYNC_LOAD_BYTES`: it loads on a worker).
const SMALL: usize = 400;
const BIG: usize = 120_000;

#[test]
fn a_jump_is_centred_only_when_out_of_sight() {
    super::seeds(NAME, SEEDS, run);
}

/// What a frame found wrong, kept for the step's check; the file the last jump went into, until a frame spends it —
/// the scenario's own account, never the editor's mark.
#[derive(Default)]
struct World {
    wrong: Option<String>,
    jump: Option<PathBuf>,
}

fn run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-jumps");
    let root = std::fs::canonicalize(scratch.path()).unwrap();
    let lines = |n: usize| (0..n).map(|i| format!("line {i}\n")).collect::<String>();
    std::fs::write(root.join("small.txt"), lines(SMALL)).unwrap();
    std::fs::write(root.join("big.txt"), lines(BIG)).unwrap();
    let world = RefCell::new(World::default());
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    ed.open(&root.join("small.txt")).unwrap();
    crate::term::render(&mut ed, &mut Vec::new(), SIZE.0, SIZE.1).unwrap();
    let check = |_: &Editor| world.borrow_mut().wrong.take().map_or(Ok(()), Err);
    let input = |sim: &mut Sim, ed: &mut Editor| input(sim, ed, &root, &mut world.borrow_mut());
    sim.walk(&mut ed, INPUTS, input, check, print);
    // (quiet, a last frame: no jump is left waiting)
    frame(&mut ed, &mut world.borrow_mut());
    let left = ed.docs.iter().any(|d| d.jumped);
    let wrong = world.borrow_mut().wrong.take();
    sim.at_end(|| match (wrong, left) {
        (Some(w), _) => Err(w),
        (None, true) => Err("a jump still waits after a frame".into()),
        (None, false) => Ok(()),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn input(sim: &mut Sim, ed: &mut Editor, root: &Path, w: &mut World) -> String {
    let c = &mut sim.choices;
    // (by arm: a frame 4, a jump in the small file 3, into the big one 1, a plain motion 2, the wheel 1, C-o 1)
    match c.weighted(&[4, 3, 1, 2, 1, 1]) {
        0 => {
            frame(ed, w);
            "a frame".into()
        }
        1 | 2 => {
            let (file, n) = if c.below(4) > 0 { ("small.txt", SMALL) } else { ("big.txt", BIG) };
            // (near where the view is — in sight or just past it — or anywhere)
            let top = ed.doc().top;
            let line = match c.below(2) {
                0 => (top + *c.pick(&[0, 3, 10, 20, 27, 35, 60])).min(n - 1),
                _ => c.below(n),
            };
            let path: PathBuf = root.join(file);
            ed.open_at(&path, line, 0).unwrap();
            w.jump = Some(path);
            format!("a jump to {file}:{}", line + 1)
        }
        3 => {
            let keys = *c.pick(&["j", "k", "<C-d>", "<C-u>", "40j"]);
            feed(ed, keys);
            format!("{keys} pressed")
        }
        4 => {
            let by = *c.pick(&[-30isize, -3, 3, 30]);
            ed.view_by(by);
            format!("the wheel {by}")
        }
        _ => {
            let before = (ed.doc().id, ed.doc().selection().clone());
            ed.jump_backward(1);
            if (ed.doc().id, ed.doc().selection().clone()) != before {
                w.jump = ed.doc().path.clone();
            }
            "C-o".into()
        }
    }
}

fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let k: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(k);
    }
}

/// A frame, judged where a jump waits on the document in front: its target in sight in the view before (the view's
/// rows from its top — a plain file, a row a line) leaves the view within the scroll margin's rows of where it was; out
/// of sight, the target's line is the middle row (or the view at the text's top, a line above the middle); the wheel
/// turned since, the view where the wheel left it. Either way every jump is spent.
fn frame(ed: &mut Editor, w: &mut World) {
    let rows = ed.viewport.0.max(1);
    let (half, so) = (rows.saturating_sub(1) / 2, ed.config.scrolloff.min(rows.saturating_sub(1) / 2));
    let d = ed.doc();
    // (a jump into the file in front, its text there — one still loading waits for it)
    let here = w.jump.as_deref().is_some_and(|p| d.path.as_deref() == Some(p));
    let jumped = here && !d.loading;
    if !d.loading || !here {
        w.jump = None;
    }
    let top = d.top;
    let wheeled = d.wheel_held.is_some_and(|h| h == (d.selection().primary(), d.version()));
    let line = ed.cursor_line();
    crate::term::render(ed, &mut Vec::new(), SIZE.0, SIZE.1).unwrap();
    let now = ed.doc().top;
    let wrong = match jumped {
        _ if ed.docs.iter().any(|d| d.jumped) => Some("a jump still waits after a frame".to_string()),
        false => None,
        true if wheeled && now != top => {
            Some(format!("the wheel left the view at {}, the jump moved it", top + 1))
        }
        true if wheeled => None,
        true if (top..top + rows).contains(&line) && now.abs_diff(top) > so => {
            Some(format!("line {} was in sight from {} — the view moved to {}", line + 1, top + 1, now + 1))
        }
        true if !(top..top + rows).contains(&line) && now != line.saturating_sub(half) => {
            Some(format!("line {} was out of sight — the view at {}, not centred", line + 1, now + 1))
        }
        true => None,
    };
    w.wrong = w.wrong.take().or(wrong);
}

/// What a trace line ends with: the file in front, its cursor's line, its view's top, whether a jump waits on it.
fn print(ed: &Editor) -> String {
    let d = ed.doc();
    let name = d.path.as_deref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
    let waits = if d.jumped { " · a jump waits" } else { "" };
    let loading = if d.loading { " · loading" } else { "" };
    format!("{} :{} top {}{waits}{loading}", name.unwrap_or_default(), ed.cursor_line() + 1, d.top + 1)
}
