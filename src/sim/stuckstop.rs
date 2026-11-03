//! A stuck run stopped by its key, simulated (frictions29 35; no card since polish landing 4): a run of Claude's going
//! (the scenario's own `sleep`, in a session of its own), at random found stuck (said on its panel), a frame, its stop
//! key (`space m c`) while it's said stuck, the run printing (said no more), the user taking their time — the stop's
//! job (its tree read, TERM, the wait, KILL, the wait, what outlived it) among the rest; one stop a run. After every
//! step: never a card, the panel kept after a stop, and the stop said once for its key.

use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::key::{Code, Key};
use crate::sizes::Slot;

const SEEDS: &[u64] = &[0, 1];

const INPUTS: usize = 30;

const LIMIT: Duration = crate::test_results::PATIENCE;

/// The line a stop puts in the run's output first.
const STOPPED: &str = "── stopped by you";

#[test]
fn a_stuck_run_stopped_by_its_key() {
    super::seeds("a_stuck_run_stopped_by_its_key", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-stuckstop");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    // (the run's process: the scenario's own, in a session of its own as a run's is)
    let mut c = std::process::Command::new("sleep");
    c.arg("60").current_dir(&root);
    let pid = crate::spawn::setsid(&c).unwrap().pid;
    let mut sim = Sim::new("a_stuck_run_stopped_by_its_key", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (root.clone(), "proj".into());
    let task = crate::task::Task { name: "hangs".into(), cmd: "sleep 60".into(), cwd: root };
    let mut r = crate::task::Run::for_test(task).running_as(pid);
    r.by_claude = true;
    ed.workspace.task_run = Some(r);
    let ys = std::cell::Cell::new(0);
    let check = |ed: &Editor| held(ed, ys.get());
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, pid, &ys), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    // (ended whatever the steps did; reaped)
    // SAFETY: a KILL to the scenario's own process, never reaped before this: still its own.
    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    let _ = crate::spawn::wait(pid);
    trace
}

/// Whether the run is said stuck.
fn said(ed: &Editor) -> bool {
    ed.workspace.task_run.as_ref().is_some_and(|r| r.wait.stuck.is_some())
}

fn stops_said(ed: &Editor) -> usize {
    let Some(r) = ed.workspace.task_run.as_ref() else { return 0 };
    r.output.iter().filter(|(_, l)| l.starts_with(STOPPED)).count()
}

fn input(sim: &mut Sim, ed: &mut Editor, pid: u32, ys: &std::cell::Cell<usize>) -> String {
    let c = &mut sim.choices;
    // (by arm: found stuck 3, a frame 2, its stop key 3, the run prints 1, time passing 1)
    match c.weighted(&[3, 2, 3, 1, 1]) {
        0 if !said(ed) => {
            let Some(r) = ed.workspace.task_run.as_mut().filter(|r| r.state == crate::task::State::Running)
            else {
                return "(no run going)".into();
            };
            let s = crate::stuck::Stuck {
                slot: Slot::Task,
                run: r.trace.id(),
                pid,
                asked: crate::clock::now(),
                quiet: Duration::from_secs(11 * 60),
                cpu: 0.0,
                at: 0,
                reach: None,
                quiet_there: false,
            };
            if let Some(t) = crate::clock::now().checked_sub(Duration::from_secs(11 * 60)) {
                r.last_output = t;
            }
            r.wait.stuck = Some(Box::new(s));
            "found stuck".into()
        }
        0 => "(said stuck already)".into(),
        1 => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            "a frame".into()
        }
        // (one stop a run of the scenario: a second one's read of the tree would race the kernel ending what the first
        // signalled — and the key on a run not said stuck closes the panel, which isn't this scenario's)
        2 if said(ed) && ys.get() == 0 => {
            for k in [' ', 'm', 'c'] {
                ed.handle_key(Key::plain(Code::Char(k)));
            }
            ys.set(1);
            "space m c on the stuck run".into()
        }
        2 => "(not said stuck, or stopped already)".into(),
        3 => {
            if let Some(r) = ed.workspace.task_run.as_mut() {
                r.last_output = crate::clock::now();
            }
            crate::stuck::sync(ed);
            "the run prints".into()
        }
        _ => {
            let ms = *c.pick(&[100, 4000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

/// Never a card; the panel kept after its stop; the stop said in the run's output no more than once per key.
fn held(ed: &Editor, ys: usize) -> Result<(), String> {
    if !ed.offers.is_empty() || !ed.workspace.parked.is_empty() {
        return Err("a card came".into());
    }
    if ys > 0 && ed.workspace.task_run.is_none() {
        return Err("the stop closed the panel".into());
    }
    if stops_said(ed) > ys {
        return Err(format!("the stop said {} times for {ys} key", stops_said(ed)));
    }
    Ok(())
}

/// What the trace says after a step (the processes' own end isn't in it: the kernel's timing).
fn print(ed: &Editor) -> String {
    format!("said stuck {} · stops said {}", said(ed), stops_said(ed))
}
