//! The git tab's first read, simulated (#301): three checkouts of one repository, each brought back on the git tab
//! with the keys in the code (as `restart::restore_session` does) — then at random a frame, a switch, the tab shown
//! again, its status read again, the user taking their time, each read's job and its landing interleaved by seed.
//! After every step: a tab never says a checkout isn't a repository (every one is); once quiet, every tab has its
//! status.

use std::time::Duration;

use super::sessions::{World, print};
use super::{Choices, Sim};
use crate::editor::Editor;
use crate::event::WorkspaceId;
use crate::sidebar::Mode;

const SEEDS: &[u64] = &[0, 1];

const INPUTS: usize = 20;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_git_tab_brought_back_reads_its_status() {
    super::seeds("a_git_tab_brought_back_reads_its_status", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let world = World::new();
    let mut sim = Sim::new("a_git_tab_brought_back_reads_its_status", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (world.repo.clone(), "proj".into());
    let mut ids = vec![WorkspaceId::HOME];
    for (name, wt) in &world.sessions {
        let id = ed.workspace_open(wt).unwrap();
        ed.workspace_switch(id);
        ed.workspace.name = name.clone();
        ids.push(id);
    }
    for &id in &ids {
        ed.workspace_switch(id);
        brought_back(&mut ed);
    }
    ed.workspace_switch(WorkspaceId::HOME);
    let dir = world.dir.display().to_string();
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &ids), held, |ed| print(ed, &dir));
    sim.at_end(|| match tabs(&ed).find(|l| l.git_status.is_none()) {
        Some(l) => Err(format!("session {}'s git tab has no status, quiet", l.id)),
        None => Ok(()),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// The front workspace's side panel on the git tab, the keys where they were — as a restore brings it back.
fn brought_back(ed: &mut Editor) {
    let at = ed.workspace.viewer.at;
    ed.sidebar_show(Mode::Git);
    ed.workspace.viewer.at = at;
}

fn input(sim: &mut Sim, ed: &mut Editor, ids: &[WorkspaceId]) -> String {
    let c = &mut sim.choices;
    // (by arm: a frame 3, a switch 2, the tab shown again 2, read again 1, time passing 1)
    match c.weighted(&[3, 2, 2, 1, 1]) {
        0 => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            "a frame".into()
        }
        1 => {
            let to = *c.pick(ids);
            ed.workspace_switch(to);
            format!("session {to} in front")
        }
        2 => {
            ed.workspace.viewer.sidebar = None;
            brought_back(ed);
            "the git tab shown again".into()
        }
        3 => {
            ed.git_refresh();
            "the git tab reads again".into()
        }
        4 => {
            let ms = *c.pick(&[50, 300, 2000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => unreachable!("one arm a weight"),
    }
}

/// Every workspace whose side panel shows the git tab.
fn tabs(ed: &Editor) -> impl Iterator<Item = &crate::workspace::Workspace> {
    std::iter::once(&ed.workspace)
        .chain(&ed.workspaces)
        .filter(|l| l.viewer.sidebar.as_ref().is_some_and(|s| s.mode == Mode::Git))
}

/// A tab says `not a git repository` only once git said so — never here, where every checkout is one.
fn held(ed: &Editor) -> Result<(), String> {
    match tabs(ed).find(|l| l.git_status.is_none() && l.git_reads > 0 && l.git_landed == l.git_reads) {
        Some(l) => Err(format!("session {}'s git tab says it isn't a repository", l.id)),
        None => Ok(()),
    }
}
