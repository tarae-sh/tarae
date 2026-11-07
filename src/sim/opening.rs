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
        arm => other(sim, ed, root, arm),
    }
}

/// The steps besides an open (`input`'s arms from 1): a frame, a key, a switch, a file changed or removed.
fn other(sim: &mut Sim, ed: &mut Editor, root: &Path, arm: usize) -> String {
    let c = &mut sim.choices;
    match arm {
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

const PASSES: &str = "a_pass_lands_in_front_only_if_newest_and_never_grows_the_buffers";

/// #375 step 3: the git tab's passes over files (`open_preview`, as `git_show_file` asks it), simulated among your own
/// opens, keys, switches, files changed or removed and frames, the reads landing in any order. At every step: never a
/// document in front without its text; the front moved by no input only to the newest open or pass asked, from what
/// was in front then; at most one preview among the buffers; a pass's read never on the loop; only one read at a time.
#[test]
fn a_pass_lands_in_front_only_if_newest_and_never_grows_the_buffers() {
    super::seeds(PASSES, SEEDS, run_passes);
}

fn run_passes(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-opening-passes");
    let root = std::fs::canonicalize(scratch.path()).unwrap();
    std::fs::write(root.join("a.rs"), "fn alpha() {}\n").unwrap();
    for f in FILES {
        std::fs::write(root.join(f), text(f, 0)).unwrap();
    }
    let world = RefCell::new(World::default());
    let mut sim = Sim::new(PASSES, choices, LIMIT);
    sim.frames = Some(SIZE);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    ed.open(&root.join("a.rs")).unwrap();
    ed.read_inline = false;
    crate::term::render(&mut ed, &mut Vec::new(), SIZE.0, SIZE.1).unwrap();
    world.borrow_mut().front = Some(ed.doc().id);
    let check = |ed: &Editor| check_passes(ed, &mut world.borrow_mut());
    let input = |sim: &mut Sim, ed: &mut Editor| pass_input(sim, ed, &root, &mut world.borrow_mut());
    sim.walk(&mut ed, INPUTS, input, check, print);
    let left = ed.opening.is_some() || ed.showing.is_some() || ed.show_reading.is_some();
    sim.at_end(|| match left {
        true => Err("an open or a pass still under way once all is quiet".into()),
        false => Ok(()),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn pass_input(sim: &mut Sim, ed: &mut Editor, root: &Path, w: &mut World) -> String {
    // (by arm: a pass 5, an open of yours 1, everything else as the first scenario)
    match sim.choices.weighted(&[5, 1, 4]) {
        0 => {
            w.input = true;
            let name = *sim.choices.pick(FILES);
            let path = root.join(name);
            let open = ed.docs.iter().any(|d| d.path.as_deref() == Some(path.as_path()));
            let (from, reading) = (ed.doc().id, ed.show_reading);
            ed.open_preview(&path, |ed, r| {
                if let Err(e) = r {
                    ed.open_failed(e)
                }
            });
            if !open && ed.docs.iter().any(|d| d.path.as_deref() == Some(path.as_path())) {
                w.wrong = Some(format!("{name} was read on the loop as the pass asked it"));
            }
            // (one read at a time: one under way stays the one read)
            if reading.is_some() && ed.show_reading != reading {
                w.wrong = Some(format!("{name}'s read began while another was under way"));
            }
            w.asked = (!open).then_some((path, from));
            format!("pass over {name}{}", if open { " (open already)" } else { "" })
        }
        1 => {
            w.input = true;
            let name = *sim.choices.pick(FILES);
            let path = root.join(name);
            let open = ed.docs.iter().any(|d| d.path.as_deref() == Some(path.as_path()));
            let from = ed.doc().id;
            ed.open_file(&path);
            w.asked = (!open).then_some((path, from));
            format!("open {name}{}", if open { " (open already)" } else { "" })
        }
        // (a frame, a key, a switch, a file changed or removed — as the first scenario weighs them)
        _ => {
            w.input = true;
            let arm = 1 + sim.choices.weighted(&[2, 2, 2, 1, 1]);
            other(sim, ed, root, arm)
        }
    }
}

/// `check`, and at most one preview among the buffers.
fn check_passes(ed: &Editor, w: &mut World) -> Result<(), String> {
    check(ed, w)?;
    let previews = ed.buffers().into_iter().filter(|&i| ed.docs[i].preview).count();
    match previews {
        0 | 1 => Ok(()),
        n => Err(format!("{n} previews among the buffers")),
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
