//! Auto cards' shadow week (R1), simulated: Claude's cards raised and merged, the reviewer's verdict landing before or
//! after the answer, `n` or the card dropped, time passing past the review's limit (`CALL_LIMIT`, on the simulated
//! clock) — in any order. After every step: a card has one record entry at most, and the verdict's landing draws no
//! frame. At the end (all quiet): every card answered or dropped has exactly one entry, its `reviewer` in it, the
//! rest none; nothing of a verdict is in tarae.log.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::{Choices, Sim};
use crate::autocards::{self, Mode};
use crate::editor::Editor;

const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 24;

const LIMIT: Duration = crate::test_results::PATIENCE;

const NAME: &str = "each_card_keeps_one_entry_its_verdict_in_it_and_nothing_shows";

/// In every verdict's reasons: what tarae.log must never hold.
const MARKER: &str = "SIM-AUTOCARDS-REASONS";

#[test]
fn each_card_keeps_one_entry_its_verdict_in_it_and_nothing_shows() {
    super::seeds(NAME, SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-autocards");
    let log = crate::log::capture();
    let mut sim = Sim::new(NAME, choices, LIMIT);
    sim.frames = Some((100, 30));
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = PathBuf::from("/work/demo");
    ed.config.llm.auto_cards = Mode::Shadow;
    ed.cards_dir = Some(dir.path().to_path_buf());
    let mut c = crate::chat::Chat::new();
    c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, "run the tests", ""));
    ed.workspace.claude.chat = Some(c);
    // (the stand-in reviewer: no process — its verdict is the scenario's input)
    let (tx, _told) = std::sync::mpsc::channel();
    ed.auto_cards_told = Some(tx);
    // (the cards raised, and those answered or dropped)
    let raised = RefCell::new(BTreeSet::<u64>::new());
    let ended = RefCell::new(BTreeSet::<u64>::new());
    let check = |_: &Editor| {
        let entries = record(dir.path());
        match raised.borrow().iter().find(|id| entries.iter().filter(|e| e["card"] == **id).count() > 1) {
            Some(id) => Err(format!("card #{id} has more than one entry")),
            None => Ok(()),
        }
    };
    let print = |ed: &Editor| {
        let mut held: Vec<u64> = ed.auto_cards.keys().copied().collect();
        held.sort_unstable();
        let held: Vec<String> = held.iter().map(|k| format!("#{k}")).collect();
        format!("up {} · held {} · {} entries", ed.offers.len(), held.join(","), record(dir.path()).len())
    };
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &raised, &ended), check, print);
    sim.at_end(|| {
        assert!(crate::cards::wait_written(crate::test_results::PATIENCE));
        let entries = record(dir.path());
        for id in raised.borrow().iter() {
            let mine: Vec<&Value> = entries.iter().filter(|e| e["card"] == *id).collect();
            match (ended.borrow().contains(id), mine.as_slice()) {
                (true, [e]) if e["reviewer"]["verdict"].is_string() => {}
                (false, []) => {}
                (ended, mine) => return Err(format!("card #{id} (ended: {ended}): entries {mine:?}")),
            }
        }
        match log.lines().iter().find(|l| l.contains(MARKER)) {
            Some(l) => Err(format!("tarae.log holds a verdict's reasons: {l}")),
            None => Ok(()),
        }
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn record(dir: &Path) -> Vec<Value> {
    crate::cards::read_record(dir)
}

/// Claude's `also` card for a read of `~/.cargo` (kind `grant`): pushed as it lands — merged into the one up when
/// it's the same.
fn raise(ed: &mut Editor) {
    let cmd = "cargo test".to_string();
    let job = crate::tools::Job::Command(cmd.clone(), ed.workspace.root.clone());
    let run = crate::blocked::BlockedRun::of(job);
    let grant = crate::allow::Grant::Read(PathBuf::from("/Users/u/.cargo"));
    let o =
        crate::blocked::also_card("Claude", grant, "r", (&cmd, None), Box::new(run), None, Vec::new(), false);
    ed.push_offer(o);
}

/// `f` as the main loop applies an event: in a batch of its own, then its frame when one is due — so what each input
/// changed is drawn by its own batch, and a verdict's landing after owes nothing.
fn applied(sim: &Sim, ed: &mut Editor, f: impl FnOnce(&mut Editor) + Send + 'static) {
    sim.batch(ed, crate::event::Event::Job(Box::new(f)));
}

fn input(
    sim: &mut Sim,
    ed: &mut Editor,
    raised: &RefCell<BTreeSet<u64>>,
    ended: &RefCell<BTreeSet<u64>>,
) -> String {
    let c = &mut sim.choices;
    let up = ed.offers.first().map(|o| o.id);
    // (by arm: a card raised (or merged) 3, a verdict 3, `n` 2, dropped 1, time 2)
    match c.weighted(&[3, 3, 2, 1, 2]) {
        0 => {
            applied(sim, ed, |ed| {
                raise(ed);
            });
            let id = ed.offers.iter().map(|o| o.id).max().expect("its card");
            raised.borrow_mut().insert(id);
            format!("card #{id} raised (rev {})", ed.offers.iter().find(|o| o.id == id).map_or(0, |o| o.rev))
        }
        1 => {
            // (in the cards' order: the map's own is a hash's)
            let mut held: Vec<(u64, u64)> = ed.auto_cards.iter().map(|(k, p)| (*k, p.run)).collect();
            held.sort_unstable();
            let Some(&(card, run)) = (!held.is_empty()).then(|| &held[c.below(held.len())]) else {
                return "no review to answer".into();
            };
            let word = *c.pick(&["approve", "ask"]);
            // (what time owes the screen first — a toast aged out — drawn by a job that changes nothing)
            applied(sim, ed, |ed| ed.unchanged = true);
            let frames = ed.frames;
            applied(sim, ed, move |ed| {
                let text = format!("{MARKER}: the user asked to run the tests.\n{word}");
                autocards::on_event(ed, card, run, crate::agent::Event::Done(Ok(text)));
            });
            match ed.frames == frames {
                true => format!("verdict {word} on #{card}"),
                false => panic!("the verdict on #{card} drew a frame: {:?}", ed.drawn_by),
            }
        }
        2 => match up {
            Some(id) => {
                applied(sim, ed, Editor::offer_dismiss);
                ended.borrow_mut().insert(id);
                format!("n on #{id}")
            }
            None => "no card to answer".into(),
        },
        3 => match up {
            Some(id) => {
                applied(sim, ed, |ed| {
                    ed.drop_cards_for(|w| matches!(w, crate::offer::What::Grant { .. }), "its run ended")
                });
                ended.borrow_mut().insert(id);
                format!("#{id} dropped")
            }
            None => "no card to drop".into(),
        },
        _ => {
            let s = *c.pick(&[1u64, 30, 91]);
            sim.wait(Duration::from_secs(s));
            format!("{s} s pass")
        }
    }
}
