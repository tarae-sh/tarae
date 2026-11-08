//! Ended workspaces saved and brought back, simulated (`endsave.rs`, SM2): a repository on `main` and a session `plain`
//! beside it (a commit of its own), and at random a file changed in `plain`, its end asked (the card always), that card
//! answered (`D`, `R`, `y`, `n`), `:workspace-restore plain` asked, its card answered (`y`, `n`), a switch, a frame, the
//! user taking their time — the end's removal, the restore's look and its making among the jobs. After every step: no
//! two sessions share a checkout, and no more come back than were saved. Once quiet: every save is still a ref or came
//! back — refs = saved − brought back — and every session's checkout is there.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::sessions::{World, print};
use super::{Choices, Sim};
use crate::editor::Editor;
use crate::offer::What;

const SEEDS: &[u64] = &[0, 1];

const INPUTS: usize = 40;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn ended_workspaces_saved_and_brought_back() {
    super::seeds("ended_workspaces_saved_and_brought_back", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let world = World::new();
    let mut sim = Sim::new("ended_workspaces_saved_and_brought_back", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (world.repo.clone(), "proj".into());
    ed.workspace.git_branch = Some("main".into());
    ed.config.workspace_worktrees = world.dir.join("wt").display().to_string();
    // (a session brought back opens its chat: its Claude a stand-in, as the fork's scenario has it)
    let (tx, _rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    let (_, plain) = &world.sessions[1];
    let id = ed.workspace_open(plain).unwrap();
    if let Some(l) = ed.workspace_by_id(id) {
        (l.name, l.branch, l.base) = ("plain".into(), Some("plain".into()), Some("main".into()));
    }
    ed.workspace_switch(crate::event::WorkspaceId::HOME);
    let dir = world.dir.display().to_string();
    let repo = world.repo.clone();
    // (a card's failure in the trace too: a removal's or a restore's lands there, not in what's said)
    let shown = |ed: &Editor| {
        let failed = ed.offers.iter().find_map(|o| match &o.state {
            crate::offer::OfferState::Failed(why) => Some(format!(" · #{} failed: {why}", o.id)),
            _ => None,
        });
        format!("{}{}", print(ed, &dir), failed.unwrap_or_default().replace(&dir, "<dir>"))
    };
    sim.walk(&mut ed, INPUTS, input, |ed| held(ed, &repo), shown);
    let roots: Vec<PathBuf> = ed.sessions_in_order().iter().map(|l| l.root.clone()).collect();
    let (saved, back, refs) =
        (counted(&ed, " · saved at "), counted(&ed, "brought back as it was"), refs(&repo));
    sim.at_end(|| {
        if refs != saved - back {
            return Err(format!("{refs} saves left, {saved} saved and {back} brought back"));
        }
        match roots.iter().find(|r| !r.is_dir()) {
            Some(r) => Err(format!("a session's checkout isn't there: {}", r.display())),
            None => Ok(()),
        }
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// How many messages said `what`.
fn counted(ed: &Editor, what: &str) -> usize {
    ed.messages.iter().filter(|m| m.2.contains(what)).count()
}

/// The repository's saves.
fn refs(repo: &Path) -> usize {
    crate::endsave::list(&repo.join(".git")).map_or(0, |l| l.len())
}

/// `plain`'s session, open: its root.
fn plain(ed: &Editor) -> Option<PathBuf> {
    ed.sessions_in_order().iter().find(|l| l.name == "plain").map(|l| l.root.clone())
}

/// The card of kind `of` brought to the front — false: none up.
fn to_front(ed: &mut Editor, of: fn(&What) -> bool) -> bool {
    let Some(i) = ed.offers.iter().position(|o| of(&o.what)) else { return false };
    let o = ed.offers.remove(i);
    ed.offers.insert(0, o);
    true
}

fn input(sim: &mut Sim, ed: &mut Editor) -> String {
    let said = act(sim, ed);
    ed.handle_event(crate::event::Event::Job(Box::new(|_| {})));
    said
}

fn act(sim: &mut Sim, ed: &mut Editor) -> String {
    let c = &mut sim.choices;
    // (by arm: a change 1 — one written after its card makes git refuse the removal, its save taken back —, the end asked
    // 3, the end card answered 4, the restore asked 3, its card answered 3, a switch 1, a frame 1, time 2)
    match c.weighted(&[1, 3, 4, 3, 3, 1, 1, 2]) {
        0 => match plain(ed).filter(|r| r.is_dir()) {
            Some(root) => {
                let n = sim.steps;
                std::fs::write(root.join(format!("n{n}.txt")), format!("{n}\n")).unwrap();
                format!("plain: n{n}.txt written")
            }
            None => "(no plain to change)".into(),
        },
        1 => match ed.session_end_asking("plain", true) {
            Ok(()) => "plain's end asked".into(),
            Err(e) => format!("plain's end: {e}"),
        },
        2 => {
            if !to_front(ed, |w| matches!(w, What::WorkspaceEnd(_))) {
                return "(no end card)".into();
            }
            match *c.pick(&['D', 'R', 'y', 'n']) {
                'D' => ed.offer_remove(true),
                'R' => ed.offer_remove(false),
                'y' => ed.offer_accept(),
                _ => ed.offer_dismiss(),
            }
            "the end card answered".into()
        }
        3 => {
            crate::endsave::ask(ed, "plain");
            ":workspace-restore plain".into()
        }
        4 => {
            if !to_front(ed, |w| matches!(w, What::WorkspaceRestore(_))) {
                return "(no restore card)".into();
            }
            match c.weighted(&[3, 1]) {
                0 => {
                    ed.offer_accept();
                    "y on the restore card".into()
                }
                _ => {
                    ed.offer_dismiss();
                    "n on the restore card".into()
                }
            }
        }
        5 => {
            let ids: Vec<_> = ed.sessions_in_order().iter().map(|l| l.id).collect();
            let to = *c.pick(&ids);
            ed.workspace_switch(to);
            format!("session {to} in front")
        }
        6 => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            "a frame".into()
        }
        7 => {
            let ms = *c.pick(&[50, 300, 2000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => unreachable!("one arm a weight"),
    }
}

fn held(ed: &Editor, _repo: &Path) -> Result<(), String> {
    let mut roots: Vec<&Path> = ed.sessions_in_order().into_iter().map(|l| l.root.as_path()).collect();
    roots.sort();
    if let Some(w) = roots.windows(2).find(|w| w[0] == w[1]) {
        return Err(format!("two sessions in {}", w[0].display()));
    }
    let (saved, back) = (counted(ed, " · saved at "), counted(ed, "brought back as it was"));
    if back > saved {
        return Err(format!("{back} brought back, only {saved} saved"));
    }
    Ok(())
}
