//! Whether a quiet run is stuck is judged over its minutes, simulated (#641: a recorder at 0.7 % of a core on average
//! was never said in 45 minutes — judged look by look, a look over 1 % now and then started its quiet time over). A
//! run of Claude's going (the scenario's own `sleep`, in a session of its own); the sampling thread's looks every 30 s
//! at a share of a core the stream picks — next to none, a recorder's, one over the line, a build's — the run printing
//! now and then, frames. After every look, against the scenario's own count of what it fed in: it's
//! found stuck only when it printed nothing and its looks averaged under `IDLE_PCT` over `llm.stuck-after`'s 10
//! minutes, and one that did is never missed.

use std::cell::RefCell;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::procs::{Os, Sample};

const SEEDS: &[u64] = &[0, 1, 2];

const INPUTS: usize = 160;

const LIMIT: Duration = crate::test_results::PATIENCE;

/// The sampling thread's looks while `:procs` is closed.
const EVERY: Duration = crate::procs::QUIET_EVERY;

/// `llm.stuck-after`'s 10 minutes, in those looks.
const LOOKS: usize = 20;

/// What a look's share of a core is picked from, in tenths of a percent: next to none, a recorder over a sleeping
/// tarae, one over the line, a build's.
const TENTHS: &[u32] = &[3, 7, 13, 450];

#[test]
fn a_quiet_run_is_judged_over_its_minutes() {
    super::seeds("a_quiet_run_is_judged_over_its_minutes", SEEDS, run);
}

/// What the scenario fed in.
#[derive(Default)]
struct Ledger {
    /// Each look's share of a core in tenths of a percent, the first at the run's start.
    looks: Vec<u32>,
    /// The look the run last printed after (its start: the first).
    printed: usize,
    /// What went wrong, for `held`.
    wrong: Option<String>,
}

impl Ledger {
    /// Whether the run is stuck at its last look by the rule — the `LOOKS` looks since one at or before the window's
    /// start, nothing printed in them, under 1 % (200 tenths over 20) all told. None exactly on the line: either way.
    fn due(&self) -> Option<bool> {
        let k = self.looks.len().checked_sub(1)?;
        if k < LOOKS || k - self.printed < LOOKS {
            return Some(false);
        }
        let sum: u32 = self.looks[k + 1 - LOOKS..].iter().sum();
        (sum != 200).then_some(sum < 200)
    }
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-stuckwindow");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    // (the run's process: the scenario's own, in a session of its own as a run's is)
    let mut c = std::process::Command::new("sleep");
    c.arg("600").current_dir(&root);
    let pid = crate::spawn::setsid(&c).unwrap().pid;
    let mut sim = Sim::new("a_quiet_run_is_judged_over_its_minutes", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (root.clone(), "proj".into());
    let task = crate::task::Task { name: "records".into(), cmd: "sleep 600".into(), cwd: root };
    let mut r = crate::task::Run::for_test(task).running_as(pid);
    (r.by_claude, r.last_output) = (true, crate::clock::now());
    ed.workspace.task_run = Some(r);
    // (its first look, at its start)
    let ledger = RefCell::new(Ledger { looks: vec![TENTHS[0]], ..Ledger::default() });
    ed.procs_sampled(sample(pid, TENTHS[0]));
    let check = |ed: &Editor| held(ed).and_then(|()| ledger.borrow_mut().wrong.take().map_or(Ok(()), Err));
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, pid, &ledger), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    // SAFETY: a KILL to the scenario's own process, never reaped before this: still its own.
    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    let _ = crate::spawn::wait(pid);
    trace
}

/// A look at the processes as the sampling thread sends one: tarae (the test) and the run at `tenths` of a percent.
fn sample(pid: u32, tenths: u32) -> Sample {
    let me = std::process::id();
    let os = |pid, ppid, name: &str| Os { pid, ppid, name: name.into(), started: 1, ..Default::default() };
    Sample::of(vec![(os(me, 1, "tarae"), Some(1.0)), (os(pid, me, "sh"), Some(tenths as f32 / 10.0))])
}

fn wait(ed: &Editor) -> Option<&crate::waits::Waiting> {
    ed.workspace.task_run.as_ref().map(|r| &r.wait)
}

fn input(sim: &mut Sim, ed: &mut Editor, pid: u32, ledger: &RefCell<Ledger>) -> String {
    let c = &mut sim.choices;
    // (by arm: a look 30, the run prints 1, a frame 1)
    match c.weighted(&[30, 1, 1]) {
        0 => {
            let tenths = TENTHS[c.weighted(&[8, 8, 4, 1])];
            sim.wait(EVERY);
            let before = wait(ed).and_then(|w| w.stuck_asked);
            ed.procs_sampled(sample(pid, tenths));
            let mut l = ledger.borrow_mut();
            l.looks.push(tenths);
            let (now, k) = (crate::clock::now(), l.looks.len() - 1);
            let w = wait(ed).expect("the run is there");
            let found = w.stuck_asked != before && w.stuck_asked == Some(now);
            // (asked within the 10 minutes, or said stuck still: not asked again, rightly)
            let standing =
                w.stuck.is_some() || before.is_some_and(|a| now.duration_since(a) < LOOKS as u32 * EVERY);
            l.wrong = match (l.due(), found) {
                (Some(false), true) => Some(format!("found stuck at look {k} against the rule")),
                (Some(true), false) if !standing => Some(format!("missed at look {k}: stuck by the rule")),
                _ => None,
            };
            format!("a look at {:.1}%{}", tenths as f32 / 10.0, if found { " — found stuck" } else { "" })
        }
        1 => {
            if let Some(r) = ed.workspace.task_run.as_mut() {
                r.last_output = crate::clock::now();
            }
            let mut l = ledger.borrow_mut();
            l.printed = l.looks.len() - 1;
            crate::stuck::sync(ed);
            "the run prints".into()
        }
        _ => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            "a frame".into()
        }
    }
}

/// Never a card.
fn held(ed: &Editor) -> Result<(), String> {
    match ed.offers.is_empty() && ed.workspace.parked.is_empty() {
        true => Ok(()),
        false => Err("a card came".into()),
    }
}

/// What the trace says after a step.
fn print(ed: &Editor) -> String {
    let w = wait(ed);
    format!(
        "said stuck {} · asked {}",
        w.is_some_and(|w| w.stuck.is_some()),
        w.and_then(|w| w.stuck_asked).is_some()
    )
}
