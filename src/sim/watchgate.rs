//! The tree watch's yes and a server's start, in either order, simulated (W1b, `lspwatch.rs`): a stand-in
//! rust-analyzer (its pack says `watched-files`) starts for a cargo project while the fake tree watch's start takes
//! `delay` — none (the two orders are the stream's to pick), under `WAIT`, past it. The stand-in's writer-thread part
//! (`lspwatch::gated`) runs on a job, waiting on the gate by the simulation's clock. A key is typed among the steps:
//! each lands at once (the main loop never waits for the gate). After every step: a decision is never the wrong one
//! for the delay. At the end: told tarae watches iff the yes came within `WAIT`; past it, the late yes changes
//! nothing for it and no watch is left running for no one.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::lspwatch::WAIT;
use crate::lspwatch::tests::{CONFIG, Fake, stand_in};

const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5, 6, 7];

const LIMIT: Duration = crate::test_results::PATIENCE;

const DELAYS: &[Duration] = &[Duration::ZERO, Duration::from_secs(3), Duration::from_secs(15)];

#[test]
fn the_watchs_yes_and_a_servers_start_in_either_order() {
    let name = "the_watchs_yes_and_a_servers_start_in_either_order";
    super::seeds(name, SEEDS, |choices| run(choices, name));
}

fn decided(ed: &Editor) -> Option<bool> {
    ed.lsp.clients.first().and_then(|c| c.watched.get()).map(|w| w.declared)
}

fn run(choices: Choices, name: &str) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-watch-gate");
    let t: PathBuf = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::create_dir_all(t.join("src")).unwrap();
    std::fs::write(t.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
    std::fs::write(t.join("src/main.rs"), "fn main() {}\n").unwrap();
    let mut sim = Sim::new(name, choices, LIMIT);
    let delay = *sim.choices.pick(DELAYS);
    let fake = Arc::new(Fake::taking(delay));
    let (config, _) = crate::config::parse(CONFIG);
    let mut ed = Editor::with_root(config, t.clone());
    ed.lsp.share.start = Some(stand_in);
    ed.lsp.watch.start = Some(fake.start());
    ed.open(&t.join("src/main.rs")).unwrap();
    let within = delay < WAIT;
    let input = |sim: &mut Sim, ed: &mut Editor| -> String {
        let before = ed.doc().text.len_chars();
        for k in ["i", "x", "esc"] {
            sim.batch(ed, crate::event::Event::Key(k.parse().unwrap()));
        }
        assert_eq!(ed.doc().text.len_chars(), before + 1, "a key waited");
        "x typed".to_string()
    };
    let check = |ed: &Editor| match decided(ed) {
        Some(d) if d != within => {
            Err(format!("told {d} with the yes {} after its start", crate::term::run_duration(delay)))
        }
        _ => Ok(()),
    };
    let print = |ed: &Editor| format!("decided {:?}, watch running {}", decided(ed), ed.lsp.watch.running());
    sim.walk(&mut ed, 3, input, check, print);
    sim.at_end(|| {
        if decided(&ed) != Some(within) {
            return Err(format!(
                "decided {:?} with the yes {} late",
                decided(&ed),
                crate::term::run_duration(delay)
            ));
        }
        if ed.lsp.watch.running() != within {
            return Err(format!("a watch running: {} (told: {within})", ed.lsp.watch.running()));
        }
        Ok(())
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}
