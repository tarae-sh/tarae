//! The memo kept, simulated (main's `.sb/memo-plan.md`, step (a)): brought up (`space M`), typed into, `esc`, undone,
//! put away every way (`space M`, `C-w w`, a press on the code), the terminal's focus lost, idle auto-save's wait,
//! frames at several widths — each save a worker's write (`memo_keep` on `layouts::save`), the writes run in any order.
//! After every step: the memo is drawn exactly while the keys are in it, and the file holds words the editor asked to
//! keep, never older than what it held before; once quiet, the newest — the memo's words as they are.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::event::{Event, Mouse, MouseKind};
use crate::key::{Code, Key};

const SEEDS: &[u64] = &[0, 1, 2];

const INPUTS: usize = 40;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn the_memos_newest_words_are_kept() {
    super::seeds("the_memos_newest_words_are_kept", SEEDS, run);
}

/// The words the editor asked to have written, in order, and the newest of them the file was seen holding.
#[derive(Default)]
struct World {
    asked: RefCell<Vec<String>>,
    read: Cell<usize>,
    /// A frame that drew the memo put away, or didn't draw it up, said.
    drawn: RefCell<Option<String>>,
    /// The workspace the memo's words are watched in, and another's folder to switch to and back.
    home: Cell<Option<crate::event::WorkspaceId>>,
    other: RefCell<std::path::PathBuf>,
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("the_memos_newest_words_are_kept", choices, LIMIT);
    let dir = crate::scratch::Scratch::new("sim-memo");
    let root = dir.to_path_buf();
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    let file = root.join("state").join("memos.json");
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    ed.memos_path = Some(file.clone());
    // (idle auto-save: its wait a job's sleep, the simulation's clock)
    ed.config.auto_save = "idle".into();
    ed.open(&root.join("a.txt")).unwrap();
    crate::term::render(&mut ed, &mut Vec::new(), 120, 40).unwrap();
    let world = World::default();
    let other = crate::scratch::Scratch::new("sim-memo-other");
    world.home.set(Some(ed.workspace.id));
    *world.other.borrow_mut() = other.to_path_buf();
    let key = root.to_string_lossy().into_owned();
    let check = |ed: &Editor| held(ed, &world, &file, &key);
    sim.walk(&mut ed, INPUTS, |sim, ed| step(sim, ed, &world, &key), check, print);
    // Quiet: home again, put away, its words are what's there
    if let Some(home) = world.home.get() {
        ed.workspace_switch(home);
    }
    if ed.memo_up() {
        keys(&mut sim, &mut ed, &world, &key, "<esc><C-w>w");
    }
    sim.wait(Duration::from_secs(5));
    let words = ed.workspace.memo.as_ref().map(crate::memo::Memo::text);
    let kept = crate::memo::read(Some(&file)).get(&key).and_then(|v| v["text"].as_str().map(str::to_string));
    match (words, kept) {
        // (never typed into: nothing to keep)
        (Some(w), None) if w.is_empty() => {}
        (None, None) => {}
        (w, k) => assert_eq!(k, w, "once quiet, the file holds the memo's words"),
    }
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// The words the editor asked to keep last, if new — after each event the scenario sends.
fn record(ed: &Editor, world: &World, key: &str) {
    let Some(now) = ed.memos.get(key).and_then(|v| v["text"].as_str()) else { return };
    if world.asked.borrow().last().map(String::as_str) != Some(now) {
        world.asked.borrow_mut().push(now.to_string());
    }
}

fn keys(sim: &mut Sim, ed: &mut Editor, world: &World, key: &str, keys: &str) {
    let mut chars = keys.chars();
    while let Some(ch) = chars.next() {
        let k: Key = match ch {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            ch => Key::plain(Code::Char(ch)),
        };
        sim.batch(ed, Event::Key(k));
        record(ed, world, key);
    }
}

fn step(sim: &mut Sim, ed: &mut Editor, world: &World, key: &str) -> String {
    // (by arm: keys 6, a press on the code 1, the terminal's focus lost 1, a frame 1, time passing 2, a workspace
    // switch 1)
    match sim.choices.weighted(&[6, 1, 1, 1, 2, 1]) {
        5 => {
            let home = world.home.get();
            let said = match home.filter(|&h| h == ed.workspace.id) {
                Some(_) => {
                    let other = world.other.borrow().clone();
                    let _ = ed.workspace_open(&other);
                    "a switch to another workspace"
                }
                None => {
                    if let Some(h) = home {
                        ed.workspace_switch(h);
                    }
                    "a switch back"
                }
            };
            sim.batch(ed, Event::Resize);
            record(ed, world, key);
            said.into()
        }
        0 => {
            let k =
                *sim.choices.pick(&[" M", " M", "next:", "the clip<ret>", "<esc>", "<C-w>w", "u", "i", "x"]);
            keys(sim, ed, world, key, k);
            format!("keys {k:?}")
        }
        1 => {
            // (the code's right end: never under the memo, centred 40 wide)
            let Some(r) = ed.screen.panes.first().map(|p| p.1) else { return "no pane".into() };
            let (x, y) = ((r.x + r.w).saturating_sub(3) as u16, (r.y + 2) as u16);
            for kind in [MouseKind::Down, MouseKind::Up] {
                sim.batch(ed, Event::Mouse(Mouse { kind, x, y, alt: false, ctrl: false }));
                record(ed, world, key);
            }
            "a press on the code".into()
        }
        2 => {
            sim.batch(ed, Event::Focus(false));
            record(ed, world, key);
            "the terminal's focus lost".into()
        }
        3 => {
            let w = *sim.choices.pick(&[60, 100, 160]);
            crate::term::render(ed, &mut Vec::new(), w, 40).unwrap();
            let drawn = ed.screen.floating.borrow().iter().any(|f| f.0 == crate::pane::GroupKind::Memo);
            if drawn != ed.memo_floating() {
                *world.drawn.borrow_mut() =
                    Some(format!("a frame drew the memo floating {drawn}, floating {}", ed.memo_floating()));
            }
            format!("a frame, {w} columns")
        }
        _ => {
            let ms = *sim.choices.pick(&[100, 3000]);
            sim.wait(Duration::from_millis(ms));
            record(ed, world, key);
            "time passes".into()
        }
    }
}

/// The memo drawn floating and up exactly while its keys are in it — docked in sight, up whoever has them; the file holds
/// words asked for, never older than what it held before.
fn held(ed: &Editor, world: &World, file: &std::path::Path, key: &str) -> Result<(), String> {
    record(ed, world, key);
    let at = ed.workspace.is_at(crate::workspace::Here::Memo);
    if ed.memo_up() != at && !ed.memo_docked_shown() {
        return Err(format!("the memo up {} with the keys {at}", ed.memo_up()));
    }
    if let Some(said) = world.drawn.borrow_mut().take() {
        return Err(said);
    }
    // (a workspace behind never keeps its memo up but docked: put away as it went, its words kept)
    let docked = |w: &crate::workspace::Workspace| w.group(crate::pane::GroupKind::Memo).is_some();
    if let Some(w) = ed.workspaces.iter().find(|w| w.memo.as_ref().is_some_and(|m| m.up) && !docked(w)) {
        return Err(format!("workspace {:?} went behind with its memo up", w.id));
    }
    let Some(kept) =
        crate::memo::read(Some(file)).get(key).and_then(|v| v["text"].as_str().map(str::to_string))
    else {
        return Ok(());
    };
    let asked = world.asked.borrow();
    // (the same words asked again later are several places: the latest no older than what it held before)
    let Some(i) = asked.iter().rposition(|a| *a == kept) else {
        return Err(format!("the file holds words never asked for: {kept:?}"));
    };
    if i < world.read.get() && !asked[world.read.get()..].contains(&kept) {
        return Err(format!("the file went back to {kept:?}, older than it held"));
    }
    world.read.set(i);
    Ok(())
}

fn print(ed: &Editor) -> String {
    let m = ed.workspace.memo.as_ref();
    let words = m.map(crate::memo::Memo::text).unwrap_or_default();
    format!("memo up {} · {:?} · {words:?}", ed.memo_up(), m.map(crate::memo::Memo::mode))
}
