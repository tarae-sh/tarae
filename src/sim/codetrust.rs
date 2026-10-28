//! G13's gate, simulated: a scratch cargo project with `main.rs` open, the trust store a scratch file — and at
//! random its file asking for a server that runs the project's code (`code_trusted`, as `attach_lsp` asks — no
//! server is started: the simulation models no server's threads, so `editor.lsp` is off and the gate is asked
//! directly), the card answered either way, `:code-trust`, `:code-untrust`, time passing; the store's read and
//! write jobs among them. After every step: the gate never said yes while the root wasn't trusted, and a trusted
//! root has no file waiting.

use std::cell::RefCell;
use std::path::Path;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::trust::State;

const SEEDS: &[u64] = &[0, 1, 2];

const INPUTS: usize = 30;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn servers_wait_for_the_codes_trust() {
    super::seeds("servers_wait_for_the_codes_trust", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-code-trust");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
    let (config, _) = crate::config::parse("[editor]\nlsp = false\n");
    let mut sim = Sim::new("servers_wait_for_the_codes_trust", choices, LIMIT);
    let mut ed = Editor::with_root(config, root.clone());
    ed.code_gate = true;
    ed.trust_path = Some(root.join("trust.json"));
    ed.open(&root.join("main.rs")).unwrap();
    // (a yes the gate gave while the root wasn't trusted)
    let wrong = RefCell::new(None);
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &root, &wrong), |ed| held(ed, &root, &wrong), print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn card(ed: &Editor) -> Option<u64> {
    ed.offers.iter().find(|o| matches!(o.what, crate::offer::What::CodeTrust { .. })).map(|o| o.id)
}

fn trusted(ed: &Editor, root: &Path) -> bool {
    ed.code_trust.get(root).is_some_and(|r| r.state == Some(State::Trusted))
}

fn input(sim: &mut Sim, ed: &mut Editor, root: &Path, wrong: &RefCell<Option<String>>) -> String {
    let c = &mut sim.choices;
    // (by arm: its file asks for its server 3, the card answered 2, :code-trust 1, :code-untrust 1, time 1)
    match c.weighted(&[3, 2, 1, 1, 1]) {
        0 => {
            let server = crate::lsp::ServerSpec {
                name: "rust-analyzer".into(),
                command: "rust-analyzer".into(),
                args: Vec::new(),
                except: Vec::new(),
            };
            let was = trusted(ed, root);
            let id = ed.doc().id;
            let yes = ed.code_trusted(&server, root, id);
            if yes && !was {
                *wrong.borrow_mut() = Some("the gate let rust-analyzer start, the root untrusted".into());
            }
            format!("main.rs asks for rust-analyzer: {}", if yes { "starts" } else { "waits" })
        }
        1 => match card(ed) {
            None => "(no card)".into(),
            Some(id) => {
                let yes = *c.pick(&[true, false]);
                ed.code_trust_answer(id, root, yes);
                format!("the card answered {}", if yes { "y" } else { "n" })
            }
        },
        2 => match ed.code_trust_ask() {
            Ok(()) => ":code-trust".into(),
            Err(e) => format!(":code-trust: {e}"),
        },
        3 => match ed.code_untrust() {
            Ok(()) => ":code-untrust".into(),
            Err(e) => format!(":code-untrust: {e}"),
        },
        4 => {
            let ms = *c.pick(&[20, 300]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => unreachable!("one arm a weight"),
    }
}

/// The gate never said yes untrusted; a trusted root's files wait for nothing.
fn held(ed: &Editor, root: &Path, wrong: &RefCell<Option<String>>) -> Result<(), String> {
    if let Some(w) = wrong.borrow().as_ref() {
        return Err(w.clone());
    }
    match ed.code_trust.get(root) {
        Some(r) if r.state == Some(State::Trusted) && !r.held.is_empty() => {
            Err(format!("trusted, yet {} files wait", r.held.len()))
        }
        _ => Ok(()),
    }
}

fn print(ed: &Editor) -> String {
    let states: Vec<String> =
        ed.code_trust.values().map(|r| format!("{:?} ({} held)", r.state, r.held.len())).collect();
    format!("trust: [{}]{}", states.join(", "), if card(ed).is_some() { ", card up" } else { "" })
}
