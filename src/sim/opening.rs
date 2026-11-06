//! Your opens on a worker (#375), simulated: opens of small files and a big one (past 1 MB), keys, switches to another
//! buffer, a second open over one under way, the file changed or removed during its read, and frames drawn at random,
//! the reads landing in any order among them. At every step: the document in front is never one without its text (no
//! blank placeholder); an open comes in front only where what was in front as it was asked still is; and an open's read
//! never runs on the loop — the open asked, nothing of the file is there until its job lands.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Choices, Sim};
use crate::document::DocId;
use crate::editor::Editor;
use crate::key::{Code, Key};

const NAME: &str = "an_open_lands_in_front_only_where_you_left_it";

const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5, 6, 7];

const INPUTS: usize = 30;

const LIMIT: Duration = crate::test_results::PATIENCE;

const SIZE: (u16, u16) = (100, 30);

/// The files opened: small ones, and one past the old 1 MB line (once a blank placeholder in front first).
const FILES: &[&str] = &["b.rs", "c.txt", "d.txt", "big.txt"];

#[test]
fn an_open_lands_in_front_only_where_you_left_it() {
    super::seeds(NAME, SEEDS, run);
}

/// The scenario's own account: the open last asked and what was in front then; whether the step was an input; the
/// document in front after the step before; what an input found wrong.
#[derive(Default)]
struct World {
    asked: Option<(PathBuf, DocId)>,
    input: bool,
    front: Option<DocId>,
    wrong: Option<String>,
}

fn run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-opening");
    let root = std::fs::canonicalize(scratch.path()).unwrap();
    std::fs::write(root.join("a.rs"), "fn alpha() {}\n").unwrap();
    for f in FILES {
        std::fs::write(root.join(f), text(f, 0)).unwrap();
    }
    let world = RefCell::new(World::default());
    let mut sim = Sim::new(NAME, choices, LIMIT);
    sim.frames = Some(SIZE);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    ed.open(&root.join("a.rs")).unwrap();
    ed.read_inline = false;
    crate::term::render(&mut ed, &mut Vec::new(), SIZE.0, SIZE.1).unwrap();
    world.borrow_mut().front = Some(ed.doc().id);
    let check = |ed: &Editor| check(ed, &mut world.borrow_mut());
    let input = |sim: &mut Sim, ed: &mut Editor| input(sim, ed, &root, &mut world.borrow_mut());
    sim.walk(&mut ed, INPUTS, input, check, print);
    let left = ed.opening.is_some();
    sim.at_end(|| match left {
        true => Err("an open still under way once all is quiet".into()),
        false => Ok(()),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// File `name`'s text, version `v` (the big one past 1 MB).
fn text(name: &str, v: usize) -> String {
    let n = if name == "big.txt" { 40_000 } else { 30 };
    (0..n).map(|i| format!("{name} v{v} line {i} with some padding to it\n")).collect()
}

fn input(sim: &mut Sim, ed: &mut Editor, root: &Path, w: &mut World) -> String {
    w.input = true;
    let c = &mut sim.choices;
    // (by arm: an open 4, a frame 2, a key 2, a switch 2, the file changed 1, removed 1)
    match c.weighted(&[4, 2, 2, 2, 1, 1]) {
        0 => {
            let name = *c.pick(FILES);
            let path = root.join(name);
            let open = ed.docs.iter().any(|d| d.path.as_deref() == Some(path.as_path()));
            let from = ed.doc().id;
            ed.open_file(&path);
            // (not open yet: nothing of it made on the loop — its read is a job)
            if !open
                && (ed.opening.is_none() || ed.docs.iter().any(|d| d.path.as_deref() == Some(path.as_path())))
            {
                w.wrong = Some(format!("{name} was read on the loop as it was asked"));
            }
            w.asked = (!open).then_some((path, from));
            format!("open {name}{}", if open { " (open already)" } else { "" })
        }
        1 => {
            crate::term::render(ed, &mut Vec::new(), SIZE.0, SIZE.1).unwrap();
            "a frame".into()
        }
        2 => {
            let k = *c.pick(&['j', 'k', 'w']);
            ed.handle_key(Key::plain(Code::Char(k)));
            format!("{k} pressed")
        }
        3 => {
            ed.cycle_buffer(1);
            "the next buffer".into()
        }
        4 => {
            let name = *c.pick(FILES);
            let v = 1 + c.below(3);
            crate::sim::write(&root.join(name), text(name, v));
            format!("{name} changed on disk")
        }
        _ => {
            let name = *c.pick(FILES);
            let _ = std::fs::remove_file(root.join(name));
            format!("{name} removed")
        }
    }
}

/// After every step: never a document in front without its text; the front moved by no input of yours only to the open
/// asked, and only from what was in front as it was asked.
fn check(ed: &Editor, w: &mut World) -> Result<(), String> {
    let input = std::mem::take(&mut w.input);
    let (now, before) = (ed.doc().id, w.front.replace(ed.doc().id));
    if let Some(why) = w.wrong.take() {
        return Err(why);
    }
    if ed.doc().loading {
        return Err(format!("{} in front without its text", name(ed)));
    }
    if input || before == Some(now) {
        return Ok(());
    }
    match &w.asked {
        Some((path, from)) if ed.doc().path.as_deref() == Some(path) && before == Some(*from) => Ok(()),
        Some((path, from)) if ed.doc().path.as_deref() == Some(path) => Err(format!(
            "{} came in front over {:?}, not over what was there as it was asked ({from:?})",
            name(ed),
            before
        )),
        _ => Err(format!("{} came in front with no open of yours asking it", name(ed))),
    }
}

fn name(ed: &Editor) -> String {
    let d = ed.doc();
    d.path
        .as_deref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// What a trace line ends with: the file in front, the open under way and whether it's said.
fn print(ed: &Editor) -> String {
    let opening = ed.opening.as_ref().map(|o| {
        let n = o.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        format!(" · opening {n}{}", if o.said { " (said)" } else { "" })
    });
    format!("{}{}", name(ed), opening.unwrap_or_default())
}
