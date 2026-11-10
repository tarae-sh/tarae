//! A failed run's lines it points at (code mode (a)'s a6, `pointed.rs`), simulated: Claude's runs and test runs end
//! failed or passed with a place in the project (a test run's: its failed case's), the setting as the chat's process
//! started flips (a new process), the file the place is in is written shorter or longer meanwhile, frames are drawn,
//! time passes — each step's jobs (the lines read on a worker) and their results interleaved by seed. After every step: each call answered once at most, a report with
//! lines only for a run that failed with the setting on, one without them answered within its own step (no worker:
//! today's way), and the file seen only by the process that was shown it.

use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;

/// The seeds every `cargo test` runs, each twice (no git, no process).
const SEEDS: &[u64] = &[0, 1, 2, 3];

/// The scenario's own steps a run.
const INPUTS: usize = 40;

/// A run's real time at most.
const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_failed_runs_lines_through_writes_and_restarts() {
    super::seeds("a_failed_runs_lines_through_writes_and_restarts", SEEDS, run);
}

/// What the scenario asked: each call's number, and whether its run failed with the setting on.
#[derive(Default)]
struct Seen {
    calls: Vec<(u64, bool)>,
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-pointed");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::write(root.join("a.rs"), lines(20)).unwrap();
    crate::tools::ANSWERED.with(|a| a.borrow_mut().clear());
    let mut sim = Sim::new("a_failed_runs_lines_through_writes_and_restarts", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    let seen = std::cell::RefCell::new(Seen::default());
    let check = |ed: &Editor| held(ed, &root, &seen.borrow());
    let mut n = 0;
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &root, &mut n, &seen), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn lines(n: usize) -> String {
    (1..=n).map(|i| format!("line {i}\n")).collect()
}

fn answers(n: u64) -> usize {
    crate::tools::ANSWERED.with(|a| a.borrow().iter().filter(|(id, _)| *id == serde_json::json!(n)).count())
}

fn input(
    sim: &mut Sim,
    ed: &mut Editor,
    root: &std::path::Path,
    n: &mut u64,
    seen: &std::cell::RefCell<Seen>,
) -> String {
    let c = &mut sim.choices;
    // (by arm: a run ends 4, the process restarts with the setting flipped 1, the file is written 2, a frame 1, time 1)
    match c.weighted(&[4, 1, 2, 1, 1]) {
        0 => {
            let Some(chat) = ed.workspace.claude.chat.as_ref() else { return "(no chat)".into() };
            let (generation, on) = (chat.generation(), chat.compound);
            let failed = *c.pick(&[true, true, false]);
            let line = *c.pick(&[2, 9, 30]);
            let tests = *c.pick(&[false, true]);
            *n += 1;
            let call = crate::tools::Call::of(generation, "", serde_json::json!(*n));
            if tests {
                use crate::test_results::{Case, Status};
                let status = if failed { Status::Failed } else { Status::Passed };
                let at = Some((root.join("a.rs"), line));
                let case = Case { name: "t".into(), status, secs: None, message: vec!["boom".into()], at };
                let mut r = crate::testing::TestRun::finished("t", vec![case], Vec::new());
                if !failed {
                    r.state = crate::testing::RunState::Passed;
                }
                r.chat = Some(call);
                ed.workspace.test_run = Some(r);
                crate::tools::tests_done(ed);
            } else {
                let task =
                    crate::task::Task { name: "check".into(), cmd: "make check".into(), cwd: root.into() };
                let mut r = crate::task::Run::for_test(task);
                r.state =
                    if failed { crate::task::State::Failed(Some(2)) } else { crate::task::State::Passed };
                r.places = vec![crate::quickfix::Item {
                    path: root.join("a.rs"),
                    line,
                    col: 0,
                    message: "boom".into(),
                    severity: 1,
                    mark: None,
                }];
                r.chat = Some(call);
                ed.workspace.task_run = Some(r);
                crate::tools::run_done(ed);
            }
            seen.borrow_mut().calls.push((*n, failed && on));
            let what = if tests { "test run" } else { "run" };
            // (no lines to bring: answered now, as before the setting)
            if !(failed && on) && answers(*n) != 1 {
                return format!("{what} {n} ended but its report waits");
            }
            format!(
                "{what} {n} ended {} at line {}, the setting {}",
                if failed { "failed" } else { "passed" },
                line + 1,
                on
            )
        }
        1 => {
            let on = !ed.workspace.claude.chat.as_ref().is_some_and(|c| c.compound);
            let mut chat = crate::chat::Chat::new();
            chat.compound = on;
            ed.workspace.claude.chat = Some(chat);
            format!("a new process, the setting {on}")
        }
        2 => {
            let k = *c.pick(&[5, 20, 40]);
            std::fs::write(root.join("a.rs"), lines(k)).unwrap();
            format!("a.rs written with {k} lines")
        }
        3 => {
            crate::term::render(ed, &mut Vec::new(), 100, 30).unwrap();
            "a frame".into()
        }
        _ => {
            let ms = *c.pick(&[50, 500]);
            sim.wait(Duration::from_millis(ms));
            "time passes".into()
        }
    }
}

fn held(ed: &Editor, root: &std::path::Path, seen: &Seen) -> Result<(), String> {
    for &(n, may) in &seen.calls {
        let said: Vec<String> = crate::tools::ANSWERED.with(|a| {
            let a = a.borrow();
            a.iter()
                .filter(|(id, _)| *id == serde_json::json!(n))
                .map(|(_, r)| r.clone().unwrap_or_else(|e| e))
                .collect()
        });
        if said.len() > 1 {
            return Err(format!("run {n} answered {} times", said.len()));
        }
        if !may && said.iter().any(|s| s.contains("Lines it points at")) {
            return Err(format!("run {n}'s report brought lines: it passed, or the setting was off"));
        }
    }
    // (a fresh process has seen nothing: what the last one was shown is its own)
    let chat = ed.workspace.claude.chat.as_ref();
    let shown = seen.calls.iter().any(|&(n, may)| may && answers(n) == 1);
    if chat.is_some_and(|c| c.edits.seen.contains_key(&root.join("a.rs"))) && !shown {
        return Err("a.rs seen with no report that brought its lines".into());
    }
    Ok(())
}

/// What a trace line ends with: calls answered of those asked, whether the chat's process has the setting.
fn print(ed: &Editor) -> String {
    let on = ed.workspace.claude.chat.as_ref().is_some_and(|c| c.compound);
    let answered = crate::tools::ANSWERED.with(|a| a.borrow().len());
    format!("{answered} answered · the setting {on}")
}
