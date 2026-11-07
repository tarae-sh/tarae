//! A new session's warm start, simulated (gitcrate B6): a repository whose main checkout has built, and at random a
//! new session's checkout made and its warm start begun (`warm::begin`: a background job, the stand-in cargo asked,
//! the main checkout's lock taken, its units cloned), the main checkout building (cargo's lock held) or done with it,
//! a build that starts first in the newest checkout, the user taking their time — the jobs' steps among them. After
//! every step: no checkout holds a unit of its own packages, and a build that started first keeps its folder. At the
//! end: each start said once in the log, warm or cold with its reason — a warm one holding the dependencies, a cold
//! one nothing of the copy — and no staged copy left anywhere. (Each job's threads ending within its step is the
//! simulation's own rule.)

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::warm::tests::{OWN, Repo, SERDE, entries, staged_in};

const SEEDS: &[u64] = &[0, 1, 2];

const INPUTS: usize = 12;

const LIMIT: Duration = crate::test_results::PATIENCE;

/// What a build that started first leaves in its `deps/`.
const ITS_OWN: &str = "deps/its-own";

#[test]
fn warm_starts() {
    super::seeds("warm_starts", SEEDS, run);
}

/// The scenario's own state: the checkouts made (each with whether a build started first there), the lock held.
#[derive(Default)]
struct Made {
    checkouts: Vec<(String, PathBuf, bool)>,
    held: Option<crate::warm::Held>,
}

fn run(choices: Choices) -> Vec<String> {
    let repo = Repo::new("sim-warm", false);
    repo.built(&repo.home);
    let scratch = repo.dir.file_name().unwrap().to_string_lossy().into_owned();
    let capture = crate::log::capture();
    let mut sim = Sim::new("warm_starts", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (repo.home.clone(), "proj".into());
    ed.workspace_store.builds = Some(repo.builds.clone());
    let made = RefCell::new(Made::default());
    sim.walk(
        &mut ed,
        INPUTS,
        |sim, ed| input(sim, ed, &repo, &made),
        |_| held(&made.borrow()),
        |_| String::new(),
    );
    made.borrow_mut().held = None;
    let lines: Vec<String> = capture.lines().into_iter().filter(|l| l.contains(&scratch)).collect();
    sim.at_end(|| said_once(&made.borrow(), &lines));
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn input(sim: &mut Sim, ed: &mut Editor, repo: &Repo, made: &RefCell<Made>) -> String {
    let mut m = made.borrow_mut();
    // (by arm: a checkout made and begun 3, the main checkout building or done 2, a build first 1, time 1)
    match sim.choices.weighted(&[3, 2, 1, 1]) {
        0 => {
            let name = format!("s{}", m.checkouts.len() + 1);
            let at = repo.checkout(&name);
            crate::warm::begin(ed, &at);
            m.checkouts.push((name.clone(), at, false));
            format!("{name} made, its warm start begun")
        }
        1 => match m.held.take() {
            Some(_) => "the main checkout's build done".into(),
            None => {
                use std::os::fd::AsRawFd as _;
                let lock = repo.home.join("target/debug/.cargo-build-lock");
                let f = std::fs::File::open(lock).unwrap();
                // SAFETY: flock on a descriptor of the scenario's own, let go as `Held` drops — never by its close: a
                // child another test forks meanwhile holds a copy until it execs, and a warm start found it held one
                // run in two. (Never refused: a warm start holds it only within its own step, for its clone.)
                assert_eq!(unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
                m.held = Some(crate::warm::Held(f));
                "the main checkout builds".into()
            }
        },
        2 => {
            let Some((name, at, first)) = m.checkouts.last_mut() else {
                return "(no checkout to build in)".into();
            };
            let debug = at.join("target/debug");
            if debug.exists() {
                return format!("({name} has a build already)");
            }
            std::fs::create_dir_all(debug.join("deps")).unwrap();
            std::fs::write(debug.join(ITS_OWN), "x").unwrap();
            *first = true;
            format!("a build starts first in {name}")
        }
        3 => {
            let ms = *sim.choices.pick(&[50, 500, 3000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => unreachable!("one arm a weight"),
    }
}

/// No checkout holds a unit of its own packages; a build that started first keeps what it wrote.
fn held(m: &Made) -> Result<(), String> {
    for (name, at, first) in &m.checkouts {
        let got = entries(&at.join("target/debug"));
        if let Some(e) = got.iter().find(|e| OWN.iter().any(|h| e.contains(h))) {
            return Err(format!("{name}: another checkout's unit came over: {e}"));
        }
        if *first && !got.iter().any(|e| e == ITS_OWN) {
            return Err(format!("{name}: the build that started first lost its folder: {got:?}"));
        }
    }
    Ok(())
}

/// Each start said once in the log (`lines`, this run's): warm — its checkout holding the dependency — or cold with
/// its reason, nothing of the copy there; no staged copy left beside any build folder.
fn said_once(m: &Made, lines: &[String]) -> Result<(), String> {
    for (name, at, first) in &m.checkouts {
        let place = format!("/wt/{name} ");
        let said: Vec<&String> = lines.iter().filter(|l| l.contains(&place)).collect();
        let [line] = said[..] else { return Err(format!("{name}: said {} times: {said:?}", said.len())) };
        let got = entries(&at.join("target/debug"));
        if line.contains("starts warm") {
            if !got.iter().any(|e| e.contains(SERDE)) {
                return Err(format!("{name}: said warm, yet holds {got:?}"));
            }
        } else if let Some((_, why)) = line.split_once("starts cold: ").filter(|(_, w)| !w.trim().is_empty())
        {
            let want: Vec<String> = if *first { vec![ITS_OWN.to_string()] } else { Vec::new() };
            if got != want {
                return Err(format!("{name}: cold ({why}), yet holds {got:?}"));
            }
        } else {
            return Err(format!("{name}: neither warm nor cold with a reason: {line}"));
        }
        let left = staged_in(&at.join("target"));
        if !left.is_empty() {
            return Err(format!("{name}: a staged copy left: {left:?}"));
        }
    }
    Ok(())
}
