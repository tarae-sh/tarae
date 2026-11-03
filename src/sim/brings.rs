//! Brought one after another, simulated (frictions29 24): the world of `sim/sessions.rs` — two sessions with
//! commits of their own — and at random a bring asked (`:workspace-bring`, its card graded on a worker), one brought
//! as its card's `y` does (`workspace_bring_now`, fast-forward only), a new commit in a session, a frame, the user
//! taking their time. A landing grades the other sessions' waiting bring cards again (`bring_regrade`) — its jobs
//! among the rest, a bring asked meanwhile, another landing. And the bring queue (24 a, `bringqueue.rs`): both
//! marked, `B`, its card's `y` — the rebase and the quick preflight it starts as tasks stood in for by the
//! scenario (no task is modelled): it rebases the checkout itself (`git rebase main`, aborted at a conflict —
//! the two sessions' `a.rs` clash) and says a preflight passed or failed. And the one bring card's `y` (polish landing
//! 4): its rebase stood in for as the queue's, the rest of its way the real one (`bring_go_on`: graded, its preflight
//! stood in for, brought). And the project's `.tarae.toml` read asking trust for its preflight steps, its card answered
//! `y` or `n` later (#323: a new checkout asks again). After every step: one bring card a session, the queue holds each
//! session once, one under way at most, a bring going by itself has no card of its own up and says where it is in its
//! session's words, and one whose preflight the trust card holds back says it waits on it, that card up.

use std::time::Duration;

use super::sessions::{World, git, git_out, one_bring_card, print};
use super::{Choices, Sim};
use crate::bringqueue::Stage;
use crate::editor::Editor;
use crate::key::{Code, Key};

/// The seeds every `cargo test` runs, each twice — git's processes are most of a step (~40 steps a second). 2: a
/// bring waits on the trust card (#323).
const SEEDS: &[u64] = &[0, 2];

const INPUTS: usize = 30;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn brought_one_after_another() {
    super::seeds("brought_one_after_another", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let world = World::new();
    let mut sim = Sim::new("brought_one_after_another", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (world.repo.clone(), "proj".into());
    ed.workspace.git_branch = Some("main".into());
    ed.config.workspace_worktrees = world.dir.join("wt").display().to_string();
    let mut names = Vec::new();
    for (name, wt) in &world.sessions {
        let id = ed.workspace_open(wt).unwrap();
        if let Some(l) = ed.workspace_by_id(id) {
            (l.name, l.branch, l.base) = (name.clone(), Some(name.clone()), Some("main".into()));
        }
        names.push((name.clone(), wt.clone()));
    }
    ed.workspace_switch(crate::event::WorkspaceId::HOME);
    ed.bring_queue.stand_in = true;
    let dir = world.dir.display().to_string();
    let check = |ed: &Editor| {
        one_bring_card(ed)
            .and_then(|()| queue_held(ed))
            .and_then(|()| failed_said(ed))
            .and_then(|()| going_alone(ed))
            .and_then(|()| trust_said(ed))
    };
    let said = |ed: &Editor| format!("{} · {}{}", print(ed, &dir), queue_said(ed), trust_waits(ed));
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &names), check, said);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn input(sim: &mut Sim, ed: &mut Editor, names: &[(String, std::path::PathBuf)]) -> String {
    let c = &mut sim.choices;
    let (name, root) = c.pick(names).clone();
    // (by arm: a bring asked 4, one brought 2, a commit 2, a frame 1, time passing 1, the queue asked 1, the
    // queue's task or a single bring's preflight stood in for 3, a bring card's failed preflight 1, the one bring
    // card's `y` 2, the project's trust asked or answered 1)
    match c.weighted(&[4, 2, 2, 1, 1, 1, 3, 1, 2, 1]) {
        0 => match ed.workspace_bring(&name) {
            Ok(()) => format!(":workspace-bring {name}"),
            Err(e) => format!(":workspace-bring {name}: {e}"),
        },
        1 => match ed.workspace_bring_now(&name) {
            Ok(()) => format!("{name} brought (its card's y)"),
            Err(e) => format!("{name} brought: {e}"),
        },
        2 => {
            let file = format!("c{}.txt", sim.steps);
            std::fs::write(root.join(&file), "x\n").unwrap();
            git(&root, &["add", &file]);
            git(&root, &["commit", "-q", "-m", &file]);
            format!("{name}: a commit")
        }
        3 => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            "a frame".into()
        }
        4 => {
            let ms = *c.pick(&[50, 600]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        5 => queue_asked(sim, ed, names),
        6 => {
            let passes = *c.pick(&[true, false]);
            stand_in(ed, names, passes)
        }
        7 => preflight_failed(ed, &name, &root),
        8 => bring_go(ed, &name, &root),
        _ => {
            let yes = *c.pick(&[true, false]);
            trust(ed, sim.steps, yes)
        }
    }
}

/// The project's `.tarae.toml` read again, asking trust for its preflight steps (#323) — while no preflight of a
/// bring has started (one under way runs what it was given) — or, asking, its card answered (`yes`).
fn trust(ed: &mut Editor, step: usize, yes: bool) -> String {
    if let Some(p) = ed.project_trust.as_ref().filter(|p| p.state == crate::trust::State::Ask) {
        let hash = p.hash;
        let id = all_cards(ed)
            .find(|o| matches!(o.what, crate::offer::What::ProjectTrust { .. }))
            .map(|o| o.id)
            .unwrap_or_default();
        ed.project_trust_answer(id, hash, yes);
        return format!("its trust card: {}", if yes { "y" } else { "n" });
    }
    let all = || std::iter::once(&ed.workspace).chain(&ed.workspaces);
    if all().any(|l| l.bring_after_preflight && !l.bring_trust) {
        return "(a preflight under way: its file read later)".into();
    }
    let both = Box::new((ed.config.clone(), ed.keymaps.clone()));
    let file = ed.workspace.root.join(".tarae.toml");
    let project = crate::trust::Project {
        place: crate::trust::place_of(&file),
        file,
        hash: [step as u8; 32],
        items: vec![("preflight.check".into(), "true".into())],
        state: crate::trust::State::Ask,
        with: both.clone(),
        without: both,
        read_at: 0,
    };
    ed.project_trust_set(Some(project));
    ".tarae.toml read: its preflight steps wait on its trust card".into()
}

/// Every card, wherever it is: in front, stacked, parked with its session.
fn all_cards(ed: &Editor) -> impl Iterator<Item = &crate::offer::Offer> {
    let parked = std::iter::once(&ed.workspace).chain(&ed.workspaces).flat_map(|l| &l.parked);
    ed.offers.iter().chain(parked)
}

/// The sessions whose bring waits on the trust card, said.
fn trust_waits(ed: &Editor) -> String {
    let all = || std::iter::once(&ed.workspace).chain(&ed.workspaces);
    let names: Vec<&str> = all().filter(|l| ed.bring_waits_trust(l)).map(|l| l.label()).collect();
    match names.is_empty() {
        true => String::new(),
        false => format!(" · waits on trust: {}", names.join(", ")),
    }
}

/// A bring whose preflight the project's trust holds back waits on its card, says so in its session's words, and the
/// card is up — never stopped unsaid (#323).
fn trust_said(ed: &Editor) -> Result<(), String> {
    let held = ed.config_project_held("preflight.");
    let up = all_cards(ed).any(|o| match &o.what {
        crate::offer::What::ProjectTrust { .. } => true,
        crate::offer::What::RepoTrust { parts, .. } => {
            parts.iter().any(|p| matches!(p.what, crate::offer::What::ProjectTrust { .. }))
        }
        _ => false,
    });
    let all = || std::iter::once(&ed.workspace).chain(&ed.workspaces);
    for l in all().filter(|l| l.bring_after_preflight && ed.bring_queue_at(l.id).is_none()) {
        let words = ed.bring_progress(l).unwrap_or_default();
        match (held, l.bring_trust) {
            (true, false) => {
                return Err(format!("{} stalls unsaid: its preflight held back, {words:?}", l.label()));
            }
            (true, true) if !words.ends_with("waits on trust") || !up => {
                return Err(format!("{} waits on trust: its words {words:?}, its card up {up}", l.label()));
            }
            _ => {}
        }
    }
    Ok(())
}

/// The one bring card's `y` on session `name` (`rebase_start`'s `bring_go`), its rebase stood in for — run here,
/// aborted at a conflict, as the queue's — and its end as `rebase_ended`'s goes on: graded on a worker, its quick
/// preflight (stood in for by arm 6), brought.
fn bring_go(ed: &mut Editor, name: &str, root: &std::path::Path) -> String {
    let Ok((id, _)) = ed.session_named(name) else { return format!("({name}: no session)") };
    let free =
        |l: &crate::workspace::Workspace| l.rebase.is_none() && !l.bring_go && !l.bring_after_preflight;
    if ed.bring_queued(id) || !session(ed, id).is_some_and(free) {
        return format!("({name}: its bring under way)");
    }
    // (its card, answered: gone as its `y` takes it — a held one has no `y`)
    let up = |o: &crate::offer::Offer| o.bring_root() == Some(root) && o.held.is_none();
    let parked = std::iter::once(&ed.workspace).chain(&ed.workspaces).flat_map(|l| &l.parked);
    if !ed.offers.iter().chain(parked).any(up) {
        return format!("({name}: no bring card to answer)");
    }
    ed.offers.retain(|o| !up(o));
    for l in std::iter::once(&mut ed.workspace).chain(&mut ed.workspaces) {
        l.parked.retain(|o| !up(o));
    }
    if !rebase(root) {
        return format!("{name}: y, its rebase stopped at a conflict — aborted, nothing goes on");
    }
    let ahead = git_out(root, &["rev-list", "--count", "main..HEAD"]).trim().parse().unwrap_or(0);
    let m = crate::rebase::Moved {
        root: root.to_path_buf(),
        name: name.into(),
        branch: name.into(),
        base: "main".into(),
        moved: 0,
        ahead,
    };
    ed.with_workspace(id, |ed| {
        (ed.workspace.bring_go, ed.workspace.bring_rebased) = (true, true);
        let quick = ed.preflight_quick();
        ed.bring_go_on(m, quick);
    });
    format!("{name}: y, rebased — on to its grade, preflight and bring")
}

/// Session `id`, wherever it is.
fn session(ed: &Editor, id: crate::event::WorkspaceId) -> Option<&crate::workspace::Workspace> {
    std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == id)
}

/// A bring going by itself (its card's `y`) has no bring card of its own up anywhere, and its session's words say it.
fn going_alone(ed: &Editor) -> Result<(), String> {
    let all = || std::iter::once(&ed.workspace).chain(&ed.workspaces);
    let cards: Vec<&crate::offer::Offer> = all().flat_map(|l| &l.parked).chain(&ed.offers).collect();
    for l in all().filter(|l| ed.bring_going(l)) {
        if let Some(o) = cards.iter().find(|o| o.bring_root() == Some(l.root.as_path())) {
            return Err(format!("{} is being brought, and card#{} {:?} asks", l.label(), o.id, o.title));
        }
        let words = ed.bring_progress(l).unwrap_or_default();
        if !words.starts_with("bringing") {
            return Err(format!("{} is being brought, its words {words:?}", l.label()));
        }
    }
    Ok(())
}

/// A session's bring card answered `p` (the card gone, as its `p` takes it), the preflight it ran stood in for and
/// failing (no task is modelled): the card is back saying so (frictions34 42) — one bring card a session still,
/// whatever was asked meanwhile.
fn preflight_failed(ed: &mut Editor, name: &str, root: &std::path::Path) -> String {
    let Ok((id, _)) = ed.session_named(name) else { return format!("({name}: no session)") };
    if ed.bring_queue_at(id).is_some() {
        return format!("({name} is the queue's)");
    }
    let m = crate::rebase::Moved {
        root: root.to_path_buf(),
        name: name.into(),
        branch: name.into(),
        base: "main".into(),
        moved: 0,
        ahead: 1,
    };
    ed.with_workspace(id, |ed| {
        (ed.workspace.bring_after_preflight, ed.workspace.bring_card_after) = (true, Some(Box::new(m)));
        ed.rebase_preflight_done(false, "quick preflight");
    });
    format!("{name}'s bring card: p, its preflight failed")
}

/// `space W`'s `m` on each session, `B`, and the card's `y` once it's drawn and focused.
fn queue_asked(_sim: &mut Sim, ed: &mut Editor, names: &[(String, std::path::PathBuf)]) -> String {
    if ed.bring_queue.run.is_some() {
        return "(a queue is under way)".into();
    }
    ed.bring_queue.marks.clear();
    for (name, _) in names {
        if let Ok((id, _)) = ed.session_named(name) {
            ed.bring_mark(id);
        }
    }
    if let Err(e) = ed.bring_queue_ask() {
        return format!("B: {e}");
    }
    let front = |ed: &Editor| ed.offers.first().is_some_and(|o| o.kind() == "bring-queue");
    if !front(ed) {
        return "B: its card behind another".into();
    }
    crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
    // (focused — a press on it — then its `y`)
    ed.card_focus_front();
    ed.handle_key(Key::plain(Code::Char('y')));
    "B, and y on its card".into()
}

/// What the queue's one under way waits on, ended by the scenario: its rebase — run here, aborted at a conflict —
/// or its preflight (`passes`).
fn stand_in(ed: &mut Editor, names: &[(String, std::path::PathBuf)], passes: bool) -> String {
    let Some((id, stage)) = ed.bring_queue.run.as_ref().and_then(|r| r.now) else {
        return single_preflight(ed, names, passes);
    };
    let Some((name, root)) = names.iter().find(|(n, _)| ed.session_named(n).is_ok_and(|(i, _)| i == id))
    else {
        return "(the queue's one isn't the scenario's)".into();
    };
    match stage {
        Stage::Rebasing => {
            let rebased = rebase(root);
            let done = if rebased { Ok(()) } else { Err("its rebase stopped at a conflict".into()) };
            ed.with_workspace(id, |ed| {
                ed.bring_queue_rebased(done);
            });
            format!("{name}'s rebase {}", if rebased { "done" } else { "stopped: aborted" })
        }
        Stage::Preflight => {
            ed.with_workspace(id, |ed| ed.rebase_preflight_done(passes, "quick preflight"));
            format!("{name}'s preflight {}", if passes { "passed" } else { "failed" })
        }
        s => format!("({name}: {s:?}, nothing to stand in for)"),
    }
}

/// A single bring's preflight (`bring_go`'s, once graded) ended by the scenario (`passes`).
fn single_preflight(ed: &mut Editor, names: &[(String, std::path::PathBuf)], passes: bool) -> String {
    let waiting = |ed: &Editor, n: &str| {
        let id = ed.session_named(n).ok()?.0;
        // (one waiting on its trust card hasn't started)
        session(ed, id).filter(|l| l.bring_after_preflight && !l.bring_trust).map(|_| id)
    };
    let Some((name, id)) = names.iter().find_map(|(n, _)| waiting(ed, n).map(|id| (n.clone(), id))) else {
        return "(no preflight under way)".into();
    };
    ed.with_workspace(id, |ed| ed.rebase_preflight_done(passes, "quick preflight"));
    format!("{name}'s preflight {}", if passes { "passed" } else { "failed" })
}

/// `git rebase main` in `root` as the scenario's own git runs — aborted at a conflict: whether it went through.
fn rebase(root: &std::path::Path) -> bool {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(["-c", "core.hooksPath=/dev/null", "-C"])
            .arg(root)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_CONFIG_HOME", "/dev/null")
            .env("GIT_AUTHOR_DATE", "2026-10-18T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-10-18T00:00:00Z")
            .output()
            .unwrap()
            .status
            .success()
    };
    run(&["rebase", "-q", "main"]) || !run(&["rebase", "--abort"])
}

/// A bring card back from a failed preflight reads as one wherever it is — its risk warned, its `y` a bring anyway —
/// and none says the plain card's words with the failed one's output behind `v` (frictions29 91: a bring asked
/// again put them over it).
fn failed_said(ed: &Editor) -> Result<(), String> {
    let parked = std::iter::once(&ed.workspace).chain(&ed.workspaces).flat_map(|l| &l.parked);
    for o in ed.offers.iter().chain(parked) {
        if !matches!(o.what, crate::offer::What::Rebased(_)) {
            continue;
        }
        let failed = o.question.contains("preflight failed");
        // (a held one has no `y` — `codereview::push_bring_card`)
        let y = o.answers(true).first().filter(|a| a.0 == 'y').map(|a| a.1);
        let anyway = o.held.is_some() || y == Some("Bring anyway");
        if failed != !o.warnings.is_empty() || (failed && !anyway) || (o.output.is_some() && !failed) {
            return Err(format!("card#{} {:?} reads as {y:?}: {:?}", o.id, o.title, o.question));
        }
    }
    Ok(())
}

/// The queue holds each session once, and one under way at most.
fn queue_held(ed: &Editor) -> Result<(), String> {
    let Some(run) = ed.bring_queue.run.as_ref() else { return Ok(()) };
    let mut all: Vec<crate::event::WorkspaceId> =
        run.left.iter().copied().chain(run.now.map(|(id, _)| id)).collect();
    let n = all.len();
    all.sort_unstable();
    all.dedup();
    match all.len() == n {
        true => Ok(()),
        false => Err(format!("a session twice in the queue: {:?} · {:?}", run.left, run.now)),
    }
}

/// The queue as it is: the one under way and its stage, those left, brought and out.
fn queue_said(ed: &Editor) -> String {
    let Some(run) = ed.bring_queue.run.as_ref() else { return "no queue".into() };
    let brought: Vec<&str> = run.brought.iter().map(|(n, _)| n.as_str()).collect();
    let out: Vec<String> = run.out.iter().map(|(n, why)| format!("{n}: {why}")).collect();
    format!("queue now {:?} left {:?} brought {brought:?} out {out:?}", run.now, run.left)
}
