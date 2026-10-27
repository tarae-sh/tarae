//! A kubectl run outside the sandbox and its card's cluster line (#182), simulated: Claude asks to run `kubectl
//! --kubeconfig <a stand-in> get pods` outside while its turn goes, the kubeconfig's context changes, the turn ends
//! and another begins, a frame is drawn, the user takes their time, the card in front is answered `n` or `y` (a
//! stand-in run holds the slot: a `y` taken only queues the run, nothing starts) — the card's kubeconfig read on a
//! worker, and read again at `y`, landing among them in any order. After every step: a card comes only for a call
//! whose turn still goes as its read lands, its line right under the command names a context the file held since
//! the call (never one from before it), and — the stand-in in a folder Claude's runs write — that line is said so,
//! in the warning color, never as decided; a run is taken only on a `y` for a card whose context, as last drawn,
//! the file held since that `y`.

use std::cell::RefCell;
use std::path::Path;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::key::{Code, Key};

const NAME: &str = "a_kubectl_card_names_a_context_its_file_held";

/// The seeds every `cargo test` runs, each twice (no process: the card's read is a file's) — 14: a read landing in
/// the next turn, which a guard on the process alone let through (a card for a stopped turn's call).
const SEEDS: &[u64] = &[0, 1, 2, 3, 14];

const INPUTS: usize = 40;

const LIMIT: Duration = crate::test_results::PATIENCE;

const CONTEXTS: [&str; 3] = ["standin-a", "standin-b", "standin-c"];

#[test]
fn a_kubectl_card_names_a_context_its_file_held() {
    super::seeds(NAME, SEEDS, run);
}

/// What the scenario did: the contexts the file held in order, each call's (id, the context's index then, its
/// turn), the turn going (None: between turns), the turns over, the cards seen so far.
#[derive(Default)]
struct World {
    held: Vec<&'static str>,
    calls: Vec<(u64, usize, u64)>,
    turn: Option<u64>,
    turns: u64,
    over: Vec<u64>,
    seen: Vec<u64>,
    /// Each card's context as last checked, by call.
    named: Vec<(u64, String)>,
    /// Each `y`: its call, the context's index then.
    ys: Vec<(u64, usize)>,
    /// The runs taken before this run of the scenario (`tools::ADMITTED` is the thread's).
    admitted: usize,
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-kubecard");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let kc = root.join("kc.yaml");
    let admitted = crate::tools::ADMITTED.with(|a| a.borrow().len());
    let world = RefCell::new(World { admitted, ..World::default() });
    write(&kc, &mut world.borrow_mut(), 0);
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    let mut c = crate::chat::Chat::new();
    let _claude = crate::chat::stand_in(&mut c);
    ed.workspace.claude.chat = Some(c);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    // (the run slot held by a stand-in for the whole run: a `y` taken queues the run, never starts it)
    let task = crate::task::Task { name: "stand-in".into(), cmd: "true".into(), cwd: root.clone() };
    let mut r = crate::task::Run::for_test(task);
    r.state = crate::task::State::Running;
    ed.workspace.task_run = Some(r);
    let check = |ed: &Editor| held(ed, &kc, &mut world.borrow_mut());
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &kc, &mut world.borrow_mut()), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// The stand-in kubeconfig set to `CONTEXTS[i]` (no cluster, no credential).
fn write(kc: &Path, w: &mut World, i: usize) {
    std::fs::write(kc, format!("apiVersion: v1\nkind: Config\ncurrent-context: {}\n", CONTEXTS[i])).unwrap();
    w.held.push(CONTEXTS[i]);
}

fn input(sim: &mut Sim, ed: &mut Editor, kc: &Path, w: &mut World) -> String {
    let c = &mut sim.choices;
    // (by arm: a turn begins 2, Claude asks 4, the context changes 3, the turn ends 2, a frame 2, time 1, `n` 2)
    match c.weighted(&[2, 4, 3, 2, 2, 1, 2]) {
        0 if w.turn.is_none() => {
            w.turns += 1;
            w.turn = Some(w.turns);
            // (a message's turn, as `chat::submit_as` starts one: never at the instant the last began)
            sim.wait(Duration::from_millis(1));
            if let Some(c) = ed.workspace.claude.chat.as_mut() {
                (c.state, c.started) = (crate::chat::State::Thinking, crate::clock::now());
            }
            format!("turn {} begins", w.turns)
        }
        0 => "(a turn goes)".into(),
        1 => {
            let (Some(turn), Some(chat)) = (w.turn, ed.workspace.claude.chat.as_ref()) else {
                return "(no turn: Claude asks nothing)".into();
            };
            let n = w.calls.len() as u64 + 1;
            let call = crate::tools::Call::of(chat.generation(), "", serde_json::json!(n));
            // (each call's command its own: its card found by it)
            let cmd = format!("kubectl --kubeconfig {} get pods -l call={n}", kc.display());
            let args =
                serde_json::json!({ "command": cmd, "outside_sandbox": true, "reason": "its sign-in" });
            w.calls.push((n, w.held.len() - 1, turn));
            match crate::tools::run(ed, call, &args) {
                Ok(()) => format!("Claude asks to run kubectl outside (call {n})"),
                Err(e) => format!("Claude asks to run kubectl outside (call {n}) — {e}"),
            }
        }
        2 => {
            let i = *c.pick(&[0, 1, 2]);
            write(kc, w, i);
            format!("the context becomes {}", CONTEXTS[i])
        }
        3 => match w.turn.take() {
            Some(t) => {
                w.over.push(t);
                crate::chat::stand_in_done(ed);
                format!("turn {t} ends")
            }
            None => "(no turn to end)".into(),
        },
        4 => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            "a frame".into()
        }
        5 => {
            let ms = *c.pick(&[1, 30, 300]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ if ed.offers.is_empty() => "(no card to answer)".into(),
        _ => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            ed.card_focus_front();
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            let key = *sim.choices.pick(&['n', 'y', 'y']);
            if let Some((n, _)) = cards(ed).next().filter(|_| key == 'y') {
                w.ys.push((n, w.held.len() - 1));
            }
            ed.handle_key(Key::plain(Code::Char(key)));
            format!("{key} on the card")
        }
    }
}

/// The kubectl run cards up: (call id, the card).
fn cards(ed: &Editor) -> impl Iterator<Item = (u64, &crate::offer::Offer)> {
    ed.offers.iter().filter_map(|o| match &o.what {
        crate::offer::What::Run(r) if r.cmd.starts_with("kubectl ") => {
            r.cmd.rsplit_once("call=").and_then(|(_, n)| n.parse().ok()).map(|n| (n, o))
        }
        _ => None,
    })
}

fn held(ed: &Editor, kc: &Path, w: &mut World) -> Result<(), String> {
    for (n, o) in cards(ed) {
        let Some(&(_, then, turn)) = w.calls.iter().find(|c| c.0 == n) else {
            return Err(format!("a card for call {n}, never asked"));
        };
        if !w.seen.contains(&n) {
            w.seen.push(n);
            if w.over.contains(&turn) {
                return Err(format!("call {n}'s card came after its turn {turn} ended"));
            }
        }
        let lines: Vec<&str> = o.question.split('\n').collect();
        let Some(at) = lines.iter().position(|l| l.starts_with("kubectl ")).map(|i| i + 1) else {
            return Err(format!("call {n}'s card has no command: {}", o.question));
        };
        let line = lines.get(at).copied().unwrap_or_default();
        let want = format!(" ({} — Claude's runs can write it)", crate::allow::tilde(kc));
        let named = line.strip_prefix("  kubectl's context: ").and_then(|l| l.strip_suffix(&want));
        let Some(named) = named else {
            return Err(format!("call {n}'s line under its command isn't its context, Claude's: {line:?}"));
        };
        if !w.held[then..].contains(&named) {
            return Err(format!("call {n}'s card names {named}, which its file didn't hold since the call"));
        }
        if o.decides(at) || !o.told.contains(&at) || !o.warnings.iter().any(|r| r.contains(&at)) {
            return Err(format!("call {n}'s context line is said as decided, or not in the warning color"));
        }
        w.named.retain(|(c, _)| *c != n);
        w.named.push((n, named.to_string()));
    }
    // (runs taken: this run's own, since it began — by its kubeconfig's path)
    let ran: Vec<u64> = crate::tools::ADMITTED.with(|a| {
        let mine = |c: &&String| c.contains(&*kc.to_string_lossy());
        a.borrow()
            .iter()
            .skip(w.admitted)
            .map(|(c, _)| c)
            .filter(mine)
            .filter_map(|c| c.rsplit_once("call=")?.1.parse().ok())
            .collect()
    });
    for n in ran {
        let Some(named) = w.named.iter().find(|(c, _)| *c == n).map(|(_, s)| s.as_str()) else {
            return Err(format!("call {n} run with no card seen"));
        };
        let Some(&(_, then)) = w.ys.iter().rev().find(|(c, _)| *c == n) else {
            return Err(format!("call {n} run with no y"));
        };
        if !w.held[then..].contains(&named) {
            return Err(format!("call {n} run on a y for {named}, which its file didn't hold since that y"));
        }
    }
    Ok(())
}

/// What a trace line ends with: the turn, the file's context, the cards' calls and the context each names.
fn print(ed: &Editor) -> String {
    let cards: Vec<String> = cards(ed)
        .map(|(n, o)| {
            let named = o.question.split("kubectl's context: ").nth(1).and_then(|l| l.split(' ').next());
            format!("{n}: {}", named.unwrap_or("?"))
        })
        .collect();
    let busy = ed.workspace.claude.chat.as_ref().is_some_and(crate::chat::Chat::busy);
    format!("{} · cards [{}]", if busy { "a turn goes" } else { "between turns" }, cards.join(", "))
}
