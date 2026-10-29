//! G13's gate, simulated: a scratch git repository with two cargo roots in it and `main.rs` open, the trust store a
//! scratch file — and at random its file asking for a server that runs one root's code (`code_trusted`, as
//! `attach_lsp` asks — no server is started: the simulation models no server's threads, so `editor.lsp` is off and
//! the gate is asked directly) — rust-analyzer, or jdtls, whose pack's agents are looked for once trusted
//! (`lspagent.rs`) —, the card answered either way, `:code-trust`, `:code-untrust` (a yes's write under way or not:
//! the take-back's waits its turn in the store's line — `sim::blocked`; and with its file asking for jdtls again at
//! once, so a look begun under the trust taken back lands under the next), time passing; the store's read and write
//! jobs and the agents' looks among them. After every step: the gate never said yes while the root wasn't trusted,
//! nor to jdtls before its agents were known; no root is trusted but by a yes no take-back came after, a trusted root
//! has no file waiting for its trust, a file waits for a server's look only while that server's own look is under
//! way, and what a server would start with is its latest look's finding, never an older one's; no root is trusted
//! while its repository is held back or asked, and the repository is one answer (#226): one card at most, every
//! root read the same state. The second scenario adds another tarae's yes written to the same store (#226's review:
//! a yes there outranked this tarae's `n` or card for a root read after it). The third asks for jdtls alone and
//! answers every card yes, so a look outlives a take-back and the next trust (#228: in the first, where an `n` holds
//! the repository back until a `:code-trust`, 16,000 seeds never got there).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::trust::State;

// (104: a root's store read lands after `:code-untrust`, begun after the yes landed and before the take-back)
const SEEDS: &[u64] = &[0, 1, 2, 104];

const INPUTS: usize = 30;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn servers_wait_for_the_codes_trust() {
    super::seeds("servers_wait_for_the_codes_trust", SEEDS, |c| {
        run("servers_wait_for_the_codes_trust", c, Mode::Plain)
    });
}

// (2: here `n`, then another tarae's yes, a's read landing after both — held back; 10: another tarae's yes, here `n`
// and `:code-trust`, b's read landing last — asked)
const SEEDS_ELSEWHERE: &[u64] = &[0, 1, 2, 10];

#[test]
fn another_taraes_yes_never_outranks_this_ones_answer() {
    let name = "another_taraes_yes_never_outranks_this_ones_answer";
    super::seeds(name, SEEDS_ELSEWHERE, |c| run(name, c, Mode::Elsewhere));
}

// (360: a look begun before `:code-untrust`, landing after the next trust's look began — its finding dropped)
const SEEDS_LOOKS: &[u64] = &[0, 1, 2, 360];

#[test]
fn a_look_begun_before_a_take_back_never_starts_its_server() {
    let name = "a_look_begun_before_a_take_back_never_starts_its_server";
    super::seeds(name, SEEDS_LOOKS, |c| run(name, c, Mode::Looks));
}

/// Which scenario a run is.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Plain,
    /// Another tarae's yes is among the inputs.
    Elsewhere,
    /// jdtls alone asked for, every card answered yes.
    Looks,
}

fn run(name: &str, choices: Choices, mode: Mode) -> Vec<String> {
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
    // (its looks in a home of its own: an empty one)
    std::fs::create_dir(repo.join("home")).unwrap();
    ed.agent_looks.home = Some(repo.join("home"));
    ed.open(&roots[0].join("main.rs")).unwrap();
    let world = RefCell::new(World::default());
    sim.walk(
        &mut ed,
        INPUTS,
        |sim, ed| input(sim, ed, &roots, &world, mode),
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

fn input(sim: &mut Sim, ed: &mut Editor, roots: &[PathBuf], world: &RefCell<World>, mode: Mode) -> String {
    let c = &mut sim.choices;
    // (by arm: its file asks for its server 3, the card answered 2, :code-trust 1, :code-untrust 1, time 1,
    // :code-untrust and its file asking for jdtls again 1 (2 for the looks) — another tarae's yes 1)
    let weights: &[usize] = match mode {
        Mode::Plain => &[3, 2, 1, 1, 1, 1],
        Mode::Elsewhere => &[3, 2, 1, 1, 1, 1, 1],
        Mode::Looks => &[3, 2, 1, 1, 1, 2],
    };
    match c.weighted(weights) {
        0 => {
            let root = c.pick(roots).clone();
            let server = match mode {
                Mode::Looks => "jdtls",
                _ => *c.pick(&["rust-analyzer", "jdtls"]),
            };
            let says = ask(ed, &root, server, world);
            format!("main.rs asks for {}'s {server}: {says}", root.file_name().unwrap().to_string_lossy())
        }
        1 => match card(ed) {
            None => "(no card)".into(),
            Some((id, root)) => {
                let yes = mode == Mode::Looks || *c.pick(&[true, false]);
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
            let root = c.pick(roots).clone();
            match ed.code_untrust() {
                Ok(()) => {
                    world.borrow_mut().yes = false;
                    let says = ask(ed, &root, "jdtls", world);
                    let name = root.file_name().unwrap().to_string_lossy().into_owned();
                    format!(":code-untrust, main.rs asks for {name}'s jdtls: {says}")
                }
                Err(e) => format!(":code-untrust: {e}"),
            }
        }
        6 => {
            // (a yes kept may trust a root read with none of its repository's read here — never one asked or
            // held back here: `held`)
            ed.code_trust_given_elsewhere(&roots[0]);
            world.borrow_mut().yes = true;
            "another tarae's y".into()
        }
        _ => unreachable!("one arm a weight"),
    }
}

/// Its file asks the gate for server `name` at `root` (as `attach_lsp` does) — what it said; a wrong yes kept in
/// `world`.
fn ask(ed: &mut Editor, root: &Path, name: &str, world: &RefCell<World>) -> &'static str {
    let server = crate::lsp::ServerSpec {
        name: name.into(),
        command: name.into(),
        args: Vec::new(),
        except: Vec::new(),
    };
    let was = trusted(ed, root);
    let id = ed.doc().id;
    let yes = ed.code_trusted(&server, root, id);
    let at = root.file_name().unwrap().to_string_lossy().into_owned();
    if yes && !was {
        world.borrow_mut().wrong = Some(format!("the gate let {name} start, {at} untrusted"));
    }
    let known =
        ed.code_trust.get(root).and_then(|r| r.agents.get(name)).is_some_and(
            |l| matches!(l.got, Some(crate::codetrust::Got::Ready { from, .. }) if from == l.begun),
        );
    if yes && name == "jdtls" && !known {
        world.borrow_mut().wrong =
            Some(format!("the gate let {at}'s jdtls start before its agents were looked for"));
    }
    if yes { "starts" } else { "waits" }
}

/// The gate never said yes untrusted, nor before an agent was known; no root trusted but by a yes no take-back came
/// after (a store read begun before it, landing after); a trusted root's files wait for nothing but a server's own
/// look under way, and what a server would start with is its latest look's; no root trusted while its repository is
/// held back or asked (its card up, a root read `n` or asking — a store's yes found later never outranks them); one
/// card at most, and the roots read agree — the repository's one answer (#226).
fn held(ed: &Editor, roots: &[PathBuf], world: &RefCell<World>) -> Result<(), String> {
    use crate::codetrust::Got;
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
        let Some(r) = ed.code_trust.get(root).filter(|r| r.state == Some(State::Trusted)) else { continue };
        for (name, l) in &r.agents {
            if !l.held.is_empty() && l.got.is_some() {
                return Err(format!(
                    "{}'s {name} look is done, yet {} files wait",
                    root.display(),
                    l.held.len()
                ));
            }
            if let Some(Got::Ready { from, .. }) = l.got
                && from != l.begun
            {
                let latest = l.begun;
                return Err(format!("{name} would start on look {from}'s finding, its latest look {latest}"));
            }
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
