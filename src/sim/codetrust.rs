//! G13's gate, simulated: a scratch git repository with two cargo roots in it and `main.rs` open, the trust store a
//! scratch file — and at random its file asking for a server that runs one root's code (`code_trusted`, as
//! `attach_lsp` asks — no server is started: the simulation models no server's threads, so `editor.lsp` is off and
//! the gate is asked directly), the card answered either way, `:code-trust`, `:code-untrust` (a yes's write under
//! way or not: the take-back's waits its turn in the store's line — `sim::blocked`), time passing; the store's read
//! and write jobs among them. After every step: the gate never said yes while the root wasn't trusted, no root is
//! trusted but by a yes no take-back came after, a trusted root has no file waiting, no root is trusted while its
//! repository is held back or asked, and the repository is one answer (#226): one card at most, every root read the
//! same state. The second scenario adds another tarae's yes written to the same store (#226's review: a yes there
//! outranked this tarae's `n` or card for a root read after it).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::trust::State;

// (3: a root's store read lands after `:code-untrust`, begun after the yes landed and before the take-back)
const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 30;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn servers_wait_for_the_codes_trust() {
    super::seeds("servers_wait_for_the_codes_trust", SEEDS, |c| {
        run("servers_wait_for_the_codes_trust", c, false)
    });
}

// (10: another tarae's yes lands, then here `n`, `:code-trust` and `n`, b's read begun after them all landing last)
const SEEDS_ELSEWHERE: &[u64] = &[0, 1, 2, 10];

#[test]
fn another_taraes_yes_never_outranks_this_ones_answer() {
    let name = "another_taraes_yes_never_outranks_this_ones_answer";
    super::seeds(name, SEEDS_ELSEWHERE, |c| run(name, c, true));
}

/// `elsewhere`: another tarae's yes is among the inputs.
fn run(name: &str, choices: Choices, elsewhere: bool) -> Vec<String> {
    // (a folder of its own each run: the store's line and its take-backs are the process's, kept by path — a run
    // in the folder the last one left would find its writes and marks, as no new tarae does)
    static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = crate::scratch::Scratch::new(&format!("sim-code-trust-{n}"));
    let repo = std::fs::canonicalize(dir.path()).unwrap();
    let init = std::process::Command::new("git").args(["init", "-q"]).current_dir(&repo).status().unwrap();
    assert!(init.success());
    let roots = [repo.join("a"), repo.join("b")];
    for root in &roots {
        std::fs::create_dir(root).unwrap();
        std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
    }
    let (config, _) = crate::config::parse("[editor]\nlsp = false\n");
    let mut sim = Sim::new(name, choices, LIMIT);
    let mut ed = Editor::with_root(config, repo.clone());
    ed.code_gate = true;
    ed.trust_path = Some(repo.join("trust.json"));
    ed.open(&roots[0].join("main.rs")).unwrap();
    let world = RefCell::new(World::default());
    sim.walk(
        &mut ed,
        INPUTS,
        |sim, ed| input(sim, ed, &roots, &world, elsewhere),
        |ed| held(ed, &roots, &world),
        |ed| print(ed, &roots),
    );
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// The card up: its id and root.
fn card(ed: &Editor) -> Option<(u64, PathBuf)> {
    ed.offers.iter().find_map(|o| match &o.what {
        crate::offer::What::CodeTrust { root } => Some((o.id, root.clone())),
        _ => None,
    })
}

fn trusted(ed: &Editor, root: &Path) -> bool {
    ed.code_trust.get(root).is_some_and(|r| r.state == Some(State::Trusted))
}

#[derive(Default)]
struct World {
    /// A yes the gate gave while the root wasn't trusted.
    wrong: Option<String>,
    /// The last answer was a yes, and no take-back came after it: what may be trusted.
    yes: bool,
}

fn input(
    sim: &mut Sim,
    ed: &mut Editor,
    roots: &[PathBuf],
    world: &RefCell<World>,
    elsewhere: bool,
) -> String {
    let c = &mut sim.choices;
    // (by arm: its file asks for its server 3, the card answered 2, :code-trust 1, :code-untrust 1, time 1 —
    // another tarae's yes 1)
    let weights: &[usize] = if elsewhere { &[3, 2, 1, 1, 1, 1] } else { &[3, 2, 1, 1, 1] };
    match c.weighted(weights) {
        0 => {
            let root = c.pick(roots).clone();
            let name = root.file_name().unwrap().to_string_lossy().into_owned();
            let server = crate::lsp::ServerSpec {
                name: "rust-analyzer".into(),
                command: "rust-analyzer".into(),
                args: Vec::new(),
                except: Vec::new(),
            };
            let was = trusted(ed, &root);
            let id = ed.doc().id;
            let yes = ed.code_trusted(&server, &root, id);
            if yes && !was {
                world.borrow_mut().wrong =
                    Some(format!("the gate let rust-analyzer start, {name} untrusted"));
            }
            format!("main.rs asks for {name}'s rust-analyzer: {}", if yes { "starts" } else { "waits" })
        }
        1 => match card(ed) {
            None => "(no card)".into(),
            Some((id, root)) => {
                let yes = *c.pick(&[true, false]);
                ed.code_trust_answer(id, &root, yes);
                world.borrow_mut().yes = yes;
                format!("the card answered {}", if yes { "y" } else { "n" })
            }
        },
        2 => match ed.code_trust_ask() {
            Ok(()) => ":code-trust".into(),
            Err(e) => format!(":code-trust: {e}"),
        },
        3 => {
            let writing = if ed.trust_writing > 0 { ", a write under way" } else { "" };
            match ed.code_untrust() {
                Ok(()) => {
                    world.borrow_mut().yes = false;
                    format!(":code-untrust{writing}")
                }
                Err(e) => format!(":code-untrust{writing}: {e}"),
            }
        }
        4 => {
            let ms = *c.pick(&[20, 300]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        5 => {
            // (a yes kept may trust a root read with none of its repository's read here — never one asked or
            // held back here: `held`)
            ed.code_trust_given_elsewhere(&roots[0]);
            world.borrow_mut().yes = true;
            "another tarae's y".into()
        }
        _ => unreachable!("one arm a weight"),
    }
}

/// The gate never said yes untrusted; no root trusted but by a yes no take-back came after (a store read begun
/// before it, landing after); a trusted root's files wait for nothing; no root trusted while its repository is held
/// back or asked (its card up, a root read `n` or asking — a store's yes found later never outranks them); one
/// card at most, and the roots read agree — the repository's one answer (#226).
fn held(ed: &Editor, roots: &[PathBuf], world: &RefCell<World>) -> Result<(), String> {
    let world = world.borrow();
    if let Some(w) = world.wrong.as_ref() {
        return Err(w.clone());
    }
    let cards = ed.offers.iter().filter(|o| matches!(o.what, crate::offer::What::CodeTrust { .. })).count();
    let read = |s| roots.iter().any(|r| ed.code_trust.get(r).is_some_and(|t| t.state == Some(s)));
    let waits = match () {
        _ if read(State::Ignored) => Some("held back"),
        _ if cards > 0 || read(State::Ask) => Some("asked"),
        _ => None,
    };
    if let (Some(waits), Some(root)) = (waits, roots.iter().find(|r| trusted(ed, r))) {
        return Err(format!("{} trusted, its repository {waits}", root.display()));
    }
    for root in roots {
        if !world.yes && trusted(ed, root) {
            return Err(format!("{} trusted, its trust taken back or never given", root.display()));
        }
        match ed.code_trust.get(root) {
            Some(r) if r.state == Some(State::Trusted) && !r.held.is_empty() => {
                return Err(format!("{} trusted, yet {} files wait", root.display(), r.held.len()));
            }
            _ => {}
        }
    }
    if cards > 1 {
        return Err(format!("{cards} cards for one repository"));
    }
    let states: Vec<State> = roots.iter().filter_map(|r| ed.code_trust.get(r)?.state).collect();
    match states.windows(2).any(|w| w[0] != w[1]) {
        true => Err(format!("one repository, its roots apart: {states:?}")),
        false => Ok(()),
    }
}

fn print(ed: &Editor, roots: &[PathBuf]) -> String {
    let states: Vec<String> = roots
        .iter()
        .filter_map(|p| ed.code_trust.get(p))
        .map(|r| format!("{:?} ({} held)", r.state, r.held.len()))
        .collect();
    format!("trust: [{}]{}", states.join(", "), if card(ed).is_some() { ", card up" } else { "" })
}
