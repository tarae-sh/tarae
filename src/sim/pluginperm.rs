//! A plugin's approvals written behind memory (#461), simulated: a yes on its card, a file allowed, a revoke, the store
//! read by the config thread's watch, a write made to fail and time passing, in any order — each write a job, its turn
//! in the store's line, its result landing in the simulation's order, a failed one's retry on the simulated clock.
//! After every step: memory is what the answers given say, in the order given — no read and no write landing undoes
//! one. At the end (all quiet): the store says the same, and nothing is left unwritten.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::plugin::{ApprovalsWatch, PluginAsk};

// (4: a read older than a write already applied, laid over it — when the guard on `served` is gone; 175: a later write
// failed while an earlier one was on its way, which didn't carry it — left unwritten, no retry)
const SEEDS: &[u64] = &[0, 1, 2, 4, 175];

const INPUTS: usize = 30;

const LIMIT: Duration = crate::test_results::PATIENCE;

const NAME: &str = "approvals_hold_in_memory_and_land_in_the_order_given";

/// What the answers given say of `demo`: None, not approved; else the files allowed beyond its patterns.
type Model = Option<Vec<String>>;

#[test]
fn approvals_hold_in_memory_and_land_in_the_order_given() {
    super::seeds(NAME, SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-pluginperm");
    let store = dir.join("plugin-permissions.json");
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.plugins_perm_stand_in(store.clone());
    let model = RefCell::new(None::<Vec<String>>);
    let watch = RefCell::new(ApprovalsWatch::new(Some(store.clone())));
    let check = |ed: &Editor| match ed.plugin_perm_memory().0 == *model.borrow() {
        true => Ok(()),
        false => {
            Err(format!("memory {:?}, the answers given {:?}", ed.plugin_perm_memory().0, model.borrow()))
        }
    };
    let print = |ed: &Editor| {
        let (memory, unwritten) = ed.plugin_perm_memory();
        let store = crate::plugin::approvals_kept_files(&store);
        format!("memory {memory:?} · {unwritten} unwritten · store {store:?} · {}", ed.plugin_perm_writes())
    };
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &store, &model, &watch), check, print);
    sim.at_end(|| {
        let (kept, unwritten) = (crate::plugin::approvals_kept_files(&store), ed.plugin_perm_memory().1);
        match (kept == *model.borrow(), unwritten) {
            (true, 0) => Ok(()),
            _ => Err(format!(
                "the store {kept:?}, {unwritten} unwritten, the answers given {:?}",
                model.borrow()
            )),
        }
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn input(
    sim: &mut Sim,
    ed: &mut Editor,
    store: &Path,
    model: &RefCell<Model>,
    watch: &RefCell<ApprovalsWatch>,
) -> String {
    let c = &mut sim.choices;
    // (by arm: a yes 3, a file allowed 2, a revoke 2, the watch's read 3, a write to fail 1, time 2)
    match c.weighted(&[3, 2, 2, 3, 1, 2]) {
        0 => {
            ed.plugin_perm_yes();
            model.borrow_mut().get_or_insert_with(Vec::new);
            "yes".into()
        }
        1 => {
            let file = *c.pick(&["/tmp/a", "/tmp/b"]);
            ed.plugin_ask_answer("demo".into(), vec![PluginAsk::Read(10, PathBuf::from(file))], true);
            // (asked only while approved: a card from before a revoke grants nothing)
            if let Some(files) = model.borrow_mut().as_mut().filter(|f| !f.iter().any(|f| f == file)) {
                files.push(file.into());
            }
            format!("{file} allowed")
        }
        2 => {
            let r = ed.plugin_revoke("demo");
            assert_eq!(r.is_ok(), model.borrow_mut().take().is_some(), "{r:?}");
            "revoke".into()
        }
        3 => match watch.borrow_mut().changed() {
            Some(kept) => {
                ed.plugin_approvals_read(kept);
                "the store read".into()
            }
            None => "the store unchanged".into(),
        },
        4 => {
            crate::storewrite::fail_next(store);
            "the next write fails".into()
        }
        _ => {
            let ms = *c.pick(&[100, 1500, 6000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}
