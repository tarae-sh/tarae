//! The chat's places found (#196), simulated: an answer naming files alone, drawn in frames, its names found on a worker
//! (`places::after_frame`); a file made and removed in the project (`files_moved`, as a change set's create or a run's
//! end says), a file's text written (`wrote`), a name pressed (`places::open_named`) and time passing, in any order —
//! the answers landing as the main loop takes them, a frame when one is due. After every step: the answers kept for the
//! checkout's files as they are say what each name comes to on disk now (one found before a file was made or removed
//! never lands as theirs — its generation passed), and a file a press opened is one of the project's its name ends
//! (found the one way, never the root joined to it). At the end, quiet
//! after a last frame: every name the answer gives is answered, as the disk has it.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Choices, Sim};
use crate::chat::{Chat, Link, Msg, Role};
use crate::editor::Editor;
use crate::places::Found;

const NAME: &str = "the_chats_names_are_found_for_the_files_as_they_are";

/// (8: an answer found before `c/Device.java` was removed lands after the frame that asked again — the generation's)
const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5, 8];

const INPUTS: usize = 24;

const LIMIT: Duration = crate::test_results::PATIENCE;

/// The names the answer gives — one file's, two's (or one's, as `c/Device.java` comes and goes), none's.
const NAMES: [&str; 3] = ["Device.java", "Util.java", "Gone.java"];

#[test]
fn the_chats_names_are_found_for_the_files_as_they_are() {
    super::seeds(NAME, SEEDS, run);
}

/// Where the scenario is: whether the second `Device.java` is there, the names pressed.
#[derive(Default)]
struct World {
    second: bool,
    pressed: Vec<&'static str>,
}

fn run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-places");
    let root = std::fs::canonicalize(scratch.path()).unwrap();
    for f in ["src/a/Device.java", "a/Util.java", "b/Util.java"] {
        std::fs::create_dir_all(root.join(f).parent().unwrap()).unwrap();
        std::fs::write(root.join(f), "one\ntwo\nthree\n").unwrap();
    }
    let world = RefCell::new(World::default());
    let mut sim = Sim::new(NAME, choices, LIMIT);
    sim.frames = Some((120, 30));
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    let mut c = Chat::new();
    let answer = "See Device.java:2 and Util.java:3, not Gone.java:1.";
    c.msgs = vec![Msg::new(Role::User, "where?", ""), Msg::new(Role::Assistant, answer, "")];
    ed.workspace.claude.chat = Some(c);
    let check = |ed: &Editor| {
        crate::places::check(ed)?;
        opened(ed, &root, &world.borrow().pressed)
    };
    let print = |ed: &Editor| print(ed, &root);
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &root, &mut world.borrow_mut()), check, print);
    // (a last frame, its names found: every one answered for the files as they are)
    crate::term::render(&mut ed, &mut Vec::new(), 120, 30).unwrap();
    sim.walk(&mut ed, 0, |_, _| String::new(), check, print);
    let answered: Vec<String> = crate::places::answers(&ed).into_iter().map(|a| a.0).collect();
    let key_now = crate::places::check(&ed);
    sim.at_end(|| match NAMES.iter().all(|n| answered.iter().any(|a| a == n)) {
        true => key_now,
        false => Err(format!("quiet after a frame, the names answered are {answered:?}")),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn input(sim: &mut Sim, ed: &mut Editor, root: &Path, w: &mut World) -> String {
    let c = &mut sim.choices;
    // (by arm: a frame 3, a file made or removed 2, a text written 2, a press 2, time 1)
    match c.weighted(&[3, 2, 2, 2, 1]) {
        0 => {
            crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
            "a frame".into()
        }
        1 => {
            let second = root.join("c/Device.java");
            w.second = !w.second;
            if w.second {
                std::fs::create_dir_all(second.parent().unwrap()).unwrap();
                std::fs::write(&second, "x\n").unwrap();
            } else {
                std::fs::remove_file(&second).unwrap();
            }
            ed.files_moved(&second);
            format!("c/Device.java {}", if w.second { "made" } else { "removed" })
        }
        2 => {
            let util = root.join("a/Util.java");
            std::fs::write(&util, "one\ntwo\nthree\nfour\n").unwrap();
            ed.wrote(&util);
            "a/Util.java's text written".into()
        }
        3 => {
            let name = *c.pick(&NAMES);
            w.pressed.push(name);
            ed.picker = None;
            crate::chat::jump(ed, Link { path: root.join(name), line: 1 });
            format!("{name}:2 pressed")
        }
        _ => {
            let ms = *c.pick(&[1, 30, 300]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

/// A file in front is one of the project's that a pressed name ends with — never the root joined to the name.
fn opened(ed: &Editor, root: &Path, pressed: &[&str]) -> Result<(), String> {
    let ours = ["src/a/Device.java", "a/Util.java", "b/Util.java", "c/Device.java"];
    match ed.doc().path.as_deref() {
        Some(p) if !pressed.iter().any(|n| p.ends_with(n)) || !ours.iter().any(|f| root.join(f) == p) => {
            Err(format!("{} opened for {pressed:?}", p.display()))
        }
        _ => Ok(()),
    }
}

/// What a trace line ends with: each name's answer (its files from the root), whether a search is under way, the
/// picker's title.
fn print(ed: &Editor, root: &Path) -> String {
    let rel = |p: &PathBuf| p.strip_prefix(root).unwrap_or(p).display().to_string();
    let said: Vec<String> = crate::places::answers(ed)
        .into_iter()
        .map(|(n, f)| match f {
            Found::File(p) => format!("{n}={}", rel(&p)),
            Found::Several(_, k) => format!("{n}={k} files"),
            Found::Not => format!("{n}=none"),
        })
        .collect();
    let asking = ed.workspace.claude.chat.as_ref().is_some_and(|c| c.places.waiting());
    let picker = ed.picker.as_ref().map(|p| p.title.replace(&root.display().to_string(), ""));
    format!("[{}]{} · {:?}", said.join(" "), if asking { " asking" } else { "" }, picker)
}
