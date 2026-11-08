//! The masked card (P2b), simulated: Claude's `also {env}` for a token-like variable whose form a pack knows comes
//! through the also's worker to its card, frames are drawn, the user takes their time and answers the card in front
//! with any of its letters and some it doesn't take (`y` masked · `r` real value · `n` · `s` · `a`), turns end, the
//! session's state is written — each step's jobs and their results interleaved by seed. After every step: the name is
//! never in what `$state/sessions` would hold, a masked grant only after a `y` on a masked card and toward the
//! pack's hosts, the real value never kept past its run (no `a` or `s` grants it here), and the value on no card.

use std::time::Duration;

use super::{Choices, Sim};
use crate::allow::Grant;
use crate::editor::Editor;
use crate::key::{Code, Key};

/// The seeds every `cargo test` runs, each twice (no git, no process: the runs Claude asks for wait their turn behind
/// one that never ends).
const SEEDS: &[u64] = &[0, 1, 2, 3];

/// The scenario's own steps a run.
const INPUTS: usize = 50;

/// A run's real time at most.
const LIMIT: Duration = crate::test_results::PATIENCE;

const NAME: &str = "TARAE_SIM_GH_TOKEN";

/// A GitHub-form stand-in, in two parts (never a whole provider-format literal).
const VALUE: &str = concat!("gho_", "tarae0sim0fake0stand0in0x");

const HOSTS: &[&str] = &["api.github.com", "github.com", "uploads.github.com"];

#[test]
fn the_masked_card_through_its_answers() {
    super::seeds("the_masked_card_through_its_answers", SEEDS, run);
}

/// What the scenario saw happen: a `y` on a masked card.
#[derive(Default)]
struct Seen {
    yes: bool,
}

/// One run of the scenario; its trace.
fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-masked");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut sim = Sim::new("the_masked_card_through_its_answers", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (root.clone(), "proj".into());
    // (a system that serves its runs' calls, its environment holding the variable)
    let here = || {
        let mut h = crate::sandbox::Here { served: Some(|| Ok(())), ..crate::sandbox::Here::now() };
        h.env.insert(NAME.into(), VALUE.into());
        h
    };
    ed.sandbox_here = Some(std::sync::Arc::new(here));
    let mut chat = crate::chat::Chat::new();
    chat.state = crate::chat::State::Thinking;
    ed.workspace.claude.chat = Some(chat);
    ed.workspace.set_at(crate::workspace::Here::Chat);
    // (a run of Claude's that left the variable out, going still: what Claude asks for waits its turn behind it)
    ed.workspace.task_run = Some(going(&root));
    let seen = std::cell::RefCell::new(Seen::default());
    let check = |ed: &Editor| held(ed, &seen.borrow());
    let mut n = 0;
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &mut n, &seen), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// A run of Claude's going in the sandbox (no process), its environment leaving `NAME` out as token-like.
fn going(root: &std::path::Path) -> crate::task::Run {
    let env = crate::sandbox::Env { root: root.to_path_buf(), ..Default::default() };
    let mut p = crate::sandbox::Policy::for_project(&env);
    p.run_env = crate::sandbox::runenv::RunEnv { tokens: vec![NAME.into()], ..Default::default() };
    let task = crate::task::Task { name: "gh pr list".into(), cmd: "gh pr list".into(), cwd: root.into() };
    let mut r = crate::task::Run::for_test(task);
    (r.by_claude, r.state) = (true, crate::task::State::Running);
    r.guard = Some(crate::sandbox::Guard::In(Box::new(p)));
    r
}

/// One of the scenario's steps, said.
fn input(sim: &mut Sim, ed: &mut Editor, n: &mut u64, seen: &std::cell::RefCell<Seen>) -> String {
    let c = &mut sim.choices;
    // (by arm: Claude asks 3, a frame 2, time passing 1, an answer 4, the turn ends 1)
    match c.weighted(&[3, 2, 1, 4, 1]) {
        0 => {
            let Some(generation) = ed.workspace.claude.chat.as_ref().map(crate::chat::Chat::generation)
            else {
                return "(no chat)".into();
            };
            *n += 1;
            let args = serde_json::json!({ "command": "gh pr list", "also": { "env": NAME }, "reason": "gh reads it" });
            let call = crate::tools::Call::of(generation, &format!("m{n}"), serde_json::json!(*n));
            match crate::tools::run(ed, call, &args) {
                Ok(()) => format!("Claude asks for {NAME} (call {n})"),
                Err(e) => format!("Claude's call {n} refused: {e}"),
            }
        }
        1 => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            "a frame".into()
        }
        2 => {
            let ms = *c.pick(&[300, 2000]);
            sim.wait(Duration::from_millis(ms));
            "time passes".into()
        }
        3 => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            ed.card_focus_front();
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            let key = *sim.choices.pick(&['y', 'y', 'r', 'n', 's', 'a']);
            if key == 'y' && ed.offers.first().is_some_and(crate::offer::Offer::masks) {
                seen.borrow_mut().yes = true;
            }
            if !ed.offers.is_empty() && !ed.card_has_keys() {
                ed.handle_key("C-q".parse().unwrap());
            }
            ed.handle_key(Key::plain(Code::Char(key)));
            format!("{key} on the card")
        }
        _ => {
            // (the turn over, the next begun: its cards go, its count of `also` cards starts again)
            crate::tools::turn_over(ed);
            crate::reads::turn_over(ed);
            if let Some(c) = ed.workspace.claude.chat.as_mut() {
                c.state = crate::chat::State::Thinking;
            }
            "the turn ends, the next begins".into()
        }
    }
}

fn held(ed: &Editor, seen: &Seen) -> Result<(), String> {
    let kept = crate::worktree::Kept::of(&ed.workspace).json().to_string();
    if kept.contains(NAME) {
        return Err(format!("{NAME} in what $state/sessions holds: {kept}"));
    }
    let chat = ed.workspace.claude.chat.as_ref().map(|c| c.allowed.grants()).unwrap_or_default();
    for g in ed.workspace.claude.allowed.grants().into_iter().chain(chat) {
        match g {
            Grant::Masked(n, _) if n == NAME && !seen.yes => {
                return Err("masked with no `y` on its card".into());
            }
            Grant::Masked(n, hosts) if n == NAME && hosts != HOSTS => {
                return Err(format!("masked toward {hosts:?}"));
            }
            Grant::Env(n) if n == NAME => return Err("the real value kept past its run".into()),
            _ => {}
        }
    }
    for o in ed.offers.iter().chain(&ed.workspace.parked) {
        if format!("{:?} {} {:?}", o.what, o.question, o.asked).contains(VALUE) {
            return Err(format!("the value on card {}", o.id));
        }
    }
    Ok(())
}

/// What a trace line ends with: the masked cards up, whether the session passes it masked.
fn print(ed: &Editor) -> String {
    let cards = ed.offers.iter().filter(|o| o.masks()).count();
    let masked = ed.workspace.claude.allowed.masked.iter().any(|(n, _)| n == NAME);
    format!("{cards} masked cards · {}", if masked { "passed masked" } else { "not passed" })
}
