//! A plugin build installed on its card (PD1's H2), simulated: Claude's ask (its look a job), the build rebuilt, the
//! card's `y` or `n` (the write a job), the plugins folder made unwritable and writable again, time passing, in any
//! order. After every step: what's in the plugins folder is bytes a card showed and its `y` took, and what memory
//! approves is such bytes — never a rebuild nobody saw, never a dev approval. At the end (all quiet): memory approves
//! exactly what's in the folder, or nothing when nothing is.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;

const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 24;

const LIMIT: Duration = crate::test_results::PATIENCE;

const NAME: &str = "an_installed_plugin_is_the_bytes_its_card_showed";

const WASM: &str = "plug/demo/target/demo.wasm";

#[test]
fn an_installed_plugin_is_the_bytes_its_card_showed() {
    super::seeds(NAME, SEEDS, run);
}

/// Build `n`: the demo hearing keys alone, a custom section telling the builds apart.
fn build(n: u8) -> Vec<u8> {
    let mut b = crate::plugin::demo_asking_for_sim(b'k');
    b.extend([0, 3, 1, b'v', n]);
    b
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-plugininstall");
    let root = dir.join("p");
    std::fs::create_dir_all(root.join("plug/demo/target")).unwrap();
    std::fs::write(root.join("plug/demo/Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    std::fs::write(root.join(WASM), build(0)).unwrap();
    let plugins = dir.join("plugins");
    crate::plugin::install_stand_in(&plugins);
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let mut ed = Editor::with_root(crate::config::Config::default(), root.clone());
    ed.plugins_detached();
    // (the bytes each `y` took, by sha256; the builds made so far)
    let taken = RefCell::new(BTreeSet::<String>::new());
    let builds = RefCell::new(0u8);
    let installed = |p: &Path| std::fs::read(p.join("demo.wasm")).ok().map(|b| crate::sha256::hex(&b));
    let approved = |ed: &Editor| ed.plugin_install_approval("demo");
    let check = |ed: &Editor| {
        let (now, a, taken) = (installed(&plugins), approved(ed), taken.borrow());
        if let Some(sha) = now.as_ref().filter(|s| !taken.contains(*s)) {
            return Err(format!("the folder holds {sha}, which no card's yes took"));
        }
        match a {
            Some((sha, dev)) if dev || !taken.contains(&sha) => {
                Err(format!("memory approves {sha} (dev: {dev})"))
            }
            _ => Ok(()),
        }
    };
    let print = |ed: &Editor| format!("folder {:?} · approved {:?}", installed(&plugins), approved(ed));
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, (&root, &plugins), &taken, &builds), check, print);
    sim.at_end(|| match (installed(&plugins), approved(&ed)) {
        (now, a) if now == a.as_ref().map(|a| a.0.clone()) => Ok(()),
        (now, a) => Err(format!("at the end the folder holds {now:?}, memory approves {a:?}")),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn input(
    sim: &mut Sim,
    ed: &mut Editor,
    (root, plugins): (&Path, &Path),
    taken: &RefCell<BTreeSet<String>>,
    builds: &RefCell<u8>,
) -> String {
    let c = &mut sim.choices;
    let up =
        ed.offers.first().and_then(|o| ed.plugin_install_shown(o)).filter(|_| !ed.offers[0].installing());
    // (by arm: an ask 3, a rebuild 2, a yes 3, a no 1, the folder blocked or freed 1, time 2)
    match c.weighted(&[3, 2, 3, 1, 1, 2]) {
        0 => {
            let n = c.pick(&[1u64, 2, 3]).to_owned();
            let r = crate::plugin::tool_call(
                ed,
                crate::tools::Call::test(n),
                &serde_json::json!({ "wasm": WASM }),
            );
            format!("Claude asks ({r:?})")
        }
        1 => {
            *builds.borrow_mut() += 1;
            std::fs::write(root.join(WASM), build(*builds.borrow())).unwrap();
            format!("rebuilt: build {}", builds.borrow())
        }
        2 => match up {
            Some(sha) => {
                taken.borrow_mut().insert(sha.clone());
                ed.offer_accept();
                format!("y on the card of {}", &sha[..8])
            }
            None => "no card to answer".into(),
        },
        3 => match up {
            Some(sha) => {
                ed.offer_dismiss();
                format!("n on the card of {}", &sha[..8])
            }
            None => "no card to answer".into(),
        },
        4 => match (plugins.is_file(), plugins.exists()) {
            (true, _) => {
                std::fs::remove_file(plugins).unwrap();
                "the plugins folder writable again".into()
            }
            (false, false) => {
                std::fs::write(plugins, "").unwrap();
                "the plugins folder blocked".into()
            }
            (false, true) => "the plugins folder is there: left".into(),
        },
        _ => {
            let ms = *c.pick(&[100, 1500]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}
