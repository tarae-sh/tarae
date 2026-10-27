//! A run's change in a workspace's folder (Folders C), simulated: a folder added to the workspace, a turn begins, a
//! run of Claude's is taken before it runs (`snapshot::Before::take_with`, the project and the folder, on a worker),
//! the stand-in run rewrites the folder's file, it ends (`tools::run_changes`, as a run's end does) — and the turn
//! ends, `space u` is pressed, frames are drawn and time passes, in any order. After every step: what the turn or a
//! checkpoint holds of the folder's file has its text before the run as it was; at the end, a run that ended has its
//! change held by the turn or a checkpoint, and a checkpoint holding it taken back by `space u` left the file as it was
//! before the run, one not taken back the run's.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::key::{Code, Key};

const NAME: &str = "a_runs_change_in_a_folder_is_the_turns";

const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 24;

const LIMIT: Duration = crate::test_results::PATIENCE;

const BEFORE: &str = "resource \"a\" {}\n";
const RUN: &str = "resource \"a\" {}\nresource \"b\" {}\n";

#[test]
fn a_runs_change_in_a_folder_is_the_turns() {
    super::seeds(NAME, SEEDS, run);
}

/// Where the scenario is: the turn going, the run (none · taken · written · ended), `space u` pressed.
#[derive(Default)]
struct World {
    turn: bool,
    turns: u64,
    run: u8,
    /// The snapshot before the run, as its job lands it.
    snap: Arc<Mutex<Option<crate::snapshot::Before>>>,
    undos: usize,
}

fn run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-folders");
    let t = std::fs::canonicalize(scratch.path()).unwrap();
    let (root, infra) = (t.join("project"), t.join("infra"));
    for (dir, file, text) in [(&root, "a.rs", "fn a() {}\n"), (&infra, "main.tf", BEFORE)] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(file), text).unwrap();
        for args in [&["init", "-q"][..], &["add", "-A"], &["commit", "-q", "-m", "base"]] {
            git(dir, args);
        }
    }
    let world = RefCell::new(World::default());
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    let folder = crate::folders::Folder { at: infra.clone(), name: "infra".into(), instructions: Vec::new() };
    ed.workspace.folders.push(folder);
    let mut c = crate::chat::Chat::new();
    let _claude = crate::chat::stand_in(&mut c);
    ed.workspace.claude.chat = Some(c);
    ed.workspace.viewer.at = crate::workspace::At::Code;
    let file = infra.join("main.tf");
    let check = |ed: &Editor| held(ed, &file);
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &root, &infra, &mut world.borrow_mut()), check, print);
    // (quiet: the checkpoint holding the run's change, taken back or not, left the file as it says)
    let w = world.borrow();
    let cps = &ed.workspace.claude.checkpoints;
    let holding = cps.list.iter().find(|c| c.files.paths().contains(&file.as_path()));
    // (a run that ended: its change in the folder is the turn's, or a checkpoint's — never lost)
    if w.run == 3 && holding.is_none() && !cps.turn.iter().any(|t| t.path == file) {
        sim.fail_now("the run ended and nothing holds its change to infra/main.tf".into());
    }
    let on_disk = std::fs::read_to_string(&file).unwrap_or_default();
    let want = match (holding.map(|c| c.undone), w.run) {
        (Some(true), _) => BEFORE,
        (Some(false), _) | (None, 2..) => RUN,
        (None, _) => BEFORE,
    };
    if on_disk != want {
        sim.fail_now(format!("main.tf at the end: {on_disk:?}, want {want:?} ({} undos)", w.undos));
    }
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// git in the scenario's own repositories — no hooks, no signing, a name of its own.
fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"])
        .args(["-c", "user.email=t@example.invalid", "-c", "user.name=t"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn input(sim: &mut Sim, ed: &mut Editor, root: &Path, infra: &Path, w: &mut World) -> String {
    let c = &mut sim.choices;
    // (by arm: a turn begins 2, the run goes on 4, the turn ends 2, `space u` 2, a frame 1, time 1)
    match c.weighted(&[2, 4, 2, 2, 1, 1]) {
        0 if !w.turn => {
            w.turns += 1;
            w.turn = true;
            sim.wait(Duration::from_millis(1));
            if let Some(c) = ed.workspace.claude.chat.as_mut() {
                (c.state, c.started) = (crate::chat::State::Thinking, crate::clock::now());
            }
            format!("turn {} begins", w.turns)
        }
        0 => "(a turn goes)".into(),
        1 => match w.run {
            // (taken before it runs: the project and its folder, on a worker — as `tools::go`)
            0 if w.turn => {
                w.run = 1;
                let (root, folders, slot) = (root.to_path_buf(), vec![infra.to_path_buf()], w.snap.clone());
                ed.events.jobs().spawn(move || {
                    let snap = crate::snapshot::Before::take_with(&root, &folders);
                    move |_: &mut Editor| *slot.lock().unwrap() = Some(snap)
                });
                "Claude's run is taken before it runs".into()
            }
            0 => "(no turn: no run)".into(),
            1 if w.snap.lock().unwrap().is_some() => {
                w.run = 2;
                std::fs::write(infra.join("main.tf"), RUN).unwrap();
                "the run rewrites infra/main.tf".into()
            }
            1 => "(its snapshot isn't taken yet)".into(),
            2 => {
                w.run = 3;
                let snap = w.snap.lock().unwrap().take().unwrap();
                let generation = ed.workspace.claude.chat.as_ref().map_or(0, |c| c.generation());
                crate::tools::run_changes(
                    ed,
                    snap,
                    generation,
                    "terraform fmt".into(),
                    |_: &mut Editor, _| {},
                );
                "the run ends".into()
            }
            _ => "(the run is over)".into(),
        },
        2 if w.turn => {
            w.turn = false;
            crate::chat::stand_in_done(ed);
            let q = format!("turn {}", w.turns);
            if !crate::checkpoint::gather(ed, &q) {
                crate::checkpoint::turn_over(ed, &q);
            }
            format!("turn {} ends", w.turns)
        }
        2 => "(no turn to end)".into(),
        3 => {
            w.undos += 1;
            ed.workspace.viewer.at = crate::workspace::At::Code;
            for k in [Code::Char(' '), Code::Char('u')] {
                ed.handle_key(Key::plain(k));
            }
            "space u".into()
        }
        4 => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            "a frame".into()
        }
        _ => {
            let ms = *c.pick(&[1, 30, 300]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

/// What the turn and the checkpoints hold of the folder's file: its text before is the one before the run.
fn held(ed: &Editor, file: &Path) -> Result<(), String> {
    let cps = &ed.workspace.claude.checkpoints;
    let lists = cps.list.iter().filter_map(|c| c.files.here()).chain(std::iter::once(cps.turn.as_slice()));
    for t in lists.flatten().filter(|t| t.path.starts_with(file.parent().unwrap())) {
        if t.path != file {
            return Err(format!("{} in a turn: nothing but main.tf changed in the folder", t.path.display()));
        }
        if t.before.as_ref().map(ToString::to_string).as_deref() != Some(BEFORE) {
            return Err(format!("main.tf's text before the run held as {:?}", t.before));
        }
    }
    Ok(())
}

/// What a trace line ends with: the turn, the checkpoints (undone or not) holding the folder's file.
fn print(ed: &Editor) -> String {
    let busy = ed.workspace.claude.chat.as_ref().is_some_and(crate::chat::Chat::busy);
    let cps = &ed.workspace.claude.checkpoints;
    let of = |c: &crate::checkpoint::Checkpoint| {
        let n: Vec<PathBuf> =
            c.files.paths().iter().filter_map(|p| p.file_name().map(PathBuf::from)).collect();
        format!("{}{n:?}", if c.undone { "undone " } else { "" })
    };
    let list: Vec<String> = cps.list.iter().map(of).collect();
    format!(
        "{} · turn {} files · checkpoints [{}]",
        if busy { "a turn goes" } else { "between turns" },
        cps.turn.len(),
        list.join(", ")
    )
}
