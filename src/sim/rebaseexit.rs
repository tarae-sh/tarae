//! A rebase's abort whose git is closed while it runs, simulated (#447): a session stopped at a conflict, and at random
//! an abort started (its git stood in for by a run in the task panel — no process), that run closed (`space m c`,
//! `task_close`), its git's exit told (as the run's end tells it once the run is gone: `rebase::ran`), the abort's work
//! done on disk meanwhile (`git rebase --abort` — its exit reads it ended), a frame, the user taking their time (past
//! the wait's bound too: `EXIT_WAIT`), and a new rebase stopped where one ended. Each job — the said step's wake, the
//! bound's, the reads — interleaved by seed. After every step: no abort's end asks its Claude (no chat opens); a
//! rebase stopped after an abort ran says the abort didn't finish; an exit told after the end was read changes
//! nothing; the wait is on the status line only once said, with frames asked for its timer. At the end every wait
//! is over.

use std::cell::RefCell;
use std::path::Path;
use std::time::Duration;

use super::sessions::{World, git};
use super::{Choices, Sim};
use crate::editor::Editor;
use crate::rebase::Stage;

const SEEDS: &[u64] = &[0, 1, 2];

const INPUTS: usize = 60;

const LIMIT: Duration = crate::test_results::PATIENCE;

/// What the scenario remembers: the next run's generation — runs closed whose exit isn't told yet — whether an
/// abort ran since the rebase was taken up — what an invariant found wrong in a step.
#[derive(Default)]
struct Model {
    next: u64,
    closed: Vec<u64>,
    aborted: bool,
    wrong: Option<String>,
}

#[test]
fn a_closed_aborts_git_is_waited_for_then_read() {
    super::seeds("a_closed_aborts_git_is_waited_for_then_read", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let world = World::new();
    // (main's own `a.rs`: `plain`'s rebase onto it stops there)
    std::fs::write(world.repo.join("a.rs"), "fn a() { main() }\n").unwrap();
    git(&world.repo, &["add", "a.rs"]);
    git(&world.repo, &["commit", "-q", "-m", "main's a"]);
    let wt = world.sessions.iter().find(|(n, _)| n == "plain").map(|(_, p)| p.clone()).unwrap();
    let mut sim = Sim::new("a_closed_aborts_git_is_waited_for_then_read", choices, LIMIT);
    sim.frames = Some((100, 20));
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (world.repo.clone(), "proj".into());
    let id = ed.workspace_open(&wt).unwrap();
    ed.workspace_switch(id);
    ed.workspace.name = "plain".into();
    let model = RefCell::new(Model { next: 1000, ..Model::default() });
    stopped(&mut ed, &wt, &mut model.borrow_mut());
    let dir = world.dir.display().to_string();
    sim.walk(
        &mut ed,
        INPUTS,
        |sim, ed| input(sim, ed, &wt, &mut model.borrow_mut()),
        |ed| held(ed, &mut model.borrow_mut()),
        |ed| super::sessions::print(ed, &dir),
    );
    sim.at_end(|| match ed.workspace.rebase.as_ref().filter(|r| r.stopping.is_some()) {
        Some(_) => Err("all quiet, and a closed run's exit still waited for".into()),
        None => Ok(()),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// A rebase stopped at its conflict in `wt` (git run here), taken up as tarae takes one up (`rebase_adopt`).
fn stopped(ed: &mut Editor, wt: &Path, m: &mut Model) {
    let _ = crate::gitrepo::test_git(wt)
        .args(["-c", "core.hooksPath=/dev/null", "-c", "user.email=t@t", "-c", "user.name=t"])
        .args(["rebase", "main"])
        .output()
        .unwrap();
    let stop = crate::rebase::under_way(wt).expect("stopped at its conflict");
    ed.rebase_adopt(stop);
    m.aborted = false;
}

fn input(sim: &mut Sim, ed: &mut Editor, wt: &Path, m: &mut Model) -> String {
    let c = &mut sim.choices;
    let task_running = ed.workspace.task_run.as_ref().is_some_and(|t| t.state == crate::task::State::Running);
    // (by arm: an abort started 3, its run closed 3, a closed run's exit told 1 — seldom, so the bound comes first
    // too — the abort's work done on disk 1, a frame 1, time passing 3, a rebase stopped again 2)
    match c.weighted(&[3, 3, 1, 1, 1, 3, 2]) {
        0 => {
            if !ed.workspace.rebase.as_ref().is_some_and(|r| r.stopped()) {
                return "(no rebase stopped: no abort)".into();
            }
            if task_running {
                return "(the panel runs one)".into();
            }
            m.next += 1;
            let generation = m.next;
            let task = crate::task::Task {
                name: "rebase --abort".into(),
                cmd: "true".into(),
                cwd: wt.to_path_buf(),
            };
            let mut run = crate::task::Run::for_test(task).of_generation(generation);
            run.state = crate::task::State::Running;
            ed.workspace.task_run = Some(run);
            crate::rebase::aborting_for_test(ed, generation);
            m.aborted = true;
            format!("an abort started (run {generation})")
        }
        1 => {
            if !task_running {
                return "(nothing running to close)".into();
            }
            let generation = ed.workspace.task_run.as_ref().map(|t| t.generation()).unwrap_or_default();
            ed.task_close();
            m.closed.push(generation);
            format!("run {generation} closed")
        }
        2 => {
            if m.closed.is_empty() {
                return "(no closed run to exit)".into();
            }
            let generation = m.closed.remove(c.below(m.closed.len()));
            let waits =
                ed.workspace.rebase.as_ref().is_some_and(
                    |r| matches!(r.stage, Stage::Running { generation: g, .. } if g == generation),
                );
            let said = ed.messages.len();
            let took = crate::rebase::ran(ed, generation);
            if !waits && (took || ed.messages.len() != said) {
                m.wrong =
                    Some(format!("run {generation}'s exit, told after its end was read, changed something"));
            }
            format!("run {generation}'s git exits{}", if waits { "" } else { " (after its end was read)" })
        }
        3 => {
            if crate::rebase::under_way(wt).is_none() {
                return "(no rebase on disk to abort)".into();
            }
            git(wt, &["rebase", "--abort"]);
            "the abort's work done on disk".into()
        }
        4 => {
            crate::term::render(ed, &mut Vec::new(), 100, 20).unwrap();
            "a frame".into()
        }
        5 => {
            let ms = *c.pick(&[100, 400, 2_000, 10_000, 11_000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => {
            if ed.workspace.rebase.is_some() || crate::rebase::under_way(wt).is_some() {
                return "(a rebase is there)".into();
            }
            stopped(ed, wt, m);
            "a rebase stopped again".into()
        }
    }
}

fn held(ed: &Editor, m: &mut Model) -> Result<(), String> {
    if let Some(w) = m.wrong.take() {
        return Err(w);
    }
    if ed.workspace.claude.chat.is_some() {
        return Err("an abort's end asked its Claude (a chat opened)".into());
    }
    let Some(r) = ed.workspace.rebase.as_ref() else { return Ok(()) };
    if r.stopped() && m.aborted && r.abort_failed.is_none() {
        return Err("an abort's end read as a stop, not as the abort failing".into());
    }
    if r.stopped() && r.stopping.is_some() {
        return Err("stopped, and still waiting for its git to exit".into());
    }
    let said = ed.rebase_exit_said();
    if said.is_some() != r.stopping.is_some_and(|s| s.said) {
        return Err(format!("the status line's wait {said:?}, its step {:?}", r.stopping));
    }
    if said.is_some() && ed.tick_wanted().is_none() {
        return Err("the wait said with its timer, and no frame asked for it".into());
    }
    Ok(())
}
