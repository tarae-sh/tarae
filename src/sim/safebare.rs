//! #610, landing 3: the offer to make git use a bare repository only when named, simulated.
//!
//! `the_offer_comes_once_and_its_answer_holds`: git's setting read as the git watch's first round reads it (on a
//! scratch global, never yours), turns ending with or without a command run in them, the card answered `y`, `n` or
//! `esc` by keys, with frames and time between, in any order. After every step: one such card at most; none once `y` or
//! `n` was given; the scratch global is git's own `explicit` exactly, and only after `y`. At the end the store holds
//! `yes` when `y` was given, `no` when `n` was, nothing else.

use std::cell::RefCell;
use std::path::Path;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::gitrepo::GitHome;

const NAME: &str = "the_offer_comes_once_and_its_answer_holds";

const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 20;

const LIMIT: Duration = crate::test_results::PATIENCE;

const WRITTEN: &str = "[safe]\n\tbareRepository = explicit\n";

#[test]
fn the_offer_comes_once_and_its_answer_holds() {
    super::seeds(NAME, SEEDS, run);
}

/// The answers given, in order.
#[derive(Default)]
struct World {
    answers: Vec<&'static str>,
}

fn editor(dir: &Path) -> Editor {
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = dir.to_path_buf();
    ed.workspace.viewer.keys_to_code();
    ed.safe_bare.home = Some(GitHome::Scratch(dir.to_path_buf()));
    ed.safe_bare.store = Some(dir.join("state/safe-bare"));
    ed
}

fn run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-safebare");
    let dir = std::fs::canonicalize(scratch.path()).unwrap();
    let world = RefCell::new(World::default());
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let mut ed = editor(&dir);
    let check = |ed: &Editor| held(ed, &dir, &world.borrow());
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &mut world.borrow_mut()), check, print);
    let w = world.borrow();
    let (yes, no) = (w.answers.contains(&"y"), w.answers.contains(&"n"));
    let store = dir.join("state/safe-bare");
    sim.at_end(|| {
        let kept = std::fs::read_to_string(&store).unwrap_or_default();
        let words: Vec<&str> = kept.lines().filter_map(|l| l.split_whitespace().next()).collect();
        match (yes, no, words.as_slice()) {
            (false, false, []) | (true, false, ["yes"]) | (false, true, ["no"]) => Ok(()),
            (yes, no, w) => Err(format!("y given {yes}, n given {no}, the store says {w:?}")),
        }
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn up(ed: &Editor) -> usize {
    ed.offers.iter().filter(|o| o.kind() == "safe-bare").count()
}

fn input(sim: &mut Sim, ed: &mut Editor, w: &mut World) -> String {
    let c = &mut sim.choices;
    // (by arm: the read 1, a turn 3, an answer 3, a frame 1, time 1)
    match c.weighted(&[1, 3, 3, 1, 1]) {
        0 => {
            crate::safebare::read_once(ed);
            "git's setting read (the git watch's first round)".into()
        }
        1 => {
            let ran = *c.pick(&[true, false]);
            ed.workspace.claude.ran_in_turn = ran;
            crate::safebare::turn_over(ed);
            format!("a turn ends, a command ran in it: {ran}")
        }
        2 => match ed.offers.first().filter(|o| o.kind() == "safe-bare").map(|o| (o.id, o.rev)) {
            Some((id, rev)) => {
                let k = *c.pick(&["y", "n", "esc"]);
                crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
                ed.offer_read_whole((id, rev));
                ed.card_focus = Some((id, rev));
                ed.handle_key(k.parse().unwrap());
                w.answers.push(k);
                format!("{k} on the card")
            }
            None => "(no card to answer)".into(),
        },
        3 => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            "a frame".into()
        }
        _ => {
            let ms = *c.pick(&[1, 30, 300]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

/// One card at most; none once answered; the scratch global git's `explicit` exactly, and only after `y`.
fn held(ed: &Editor, dir: &Path, w: &World) -> Result<(), String> {
    if up(ed) > 1 {
        return Err("two safe-bare cards up".into());
    }
    let answered = w.answers.iter().any(|a| *a != "esc");
    if answered && up(ed) > 0 {
        return Err("answered, and its card is up".into());
    }
    match std::fs::read_to_string(dir.join(".gitconfig")) {
        Ok(t) if t != WRITTEN => Err(format!("the scratch global: {t:?}")),
        Ok(_) if !w.answers.contains(&"y") => Err("written with no y".into()),
        _ => Ok(()),
    }
}

fn print(ed: &Editor) -> String {
    format!("card {} · setting {:?} · offered {}", up(ed), ed.safe_bare.setting, ed.safe_bare.offered)
}
