//! Idle compaction on the simulated clock (audit 2.C, `idlecompact.rs`): a chat whose last turn wrote 250k tokens to
//! the 1h cache, its claude a stand-in whose lines are kept (`Editor.claude_lines`), the usage watch asleep (a job,
//! `usage::sleep_until`) and the rule read (a job) among the steps. Three scenarios: left idle, it sends one
//! `/compact`, 54 minutes after the request; a message at 53 minutes (its turn ended at random) holds it until 54
//! minutes after that turn's request; a card coming and going at random holds it while it's up. After every step: a
//! `/compact` only while idle, at least 54 minutes after the last request and within a minute of it, one at most a
//! request.

use std::cell::RefCell;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::llm::StreamEv;

const SEEDS: &[u64] = &[0, 1, 2];

const LIMIT: Duration = crate::test_results::PATIENCE;

const MIN: u64 = 60_000;

#[test]
fn idle_54_minutes_compacts_once() {
    super::seeds("idle_54_minutes_compacts_once", SEEDS, |c| {
        run("idle_54_minutes_compacts_once", c, Arm::Idle)
    });
}

#[test]
fn a_message_at_53_minutes_holds_it() {
    super::seeds("a_message_at_53_minutes_holds_it", SEEDS, |c| {
        run("a_message_at_53_minutes_holds_it", c, Arm::Message)
    });
}

#[test]
fn a_card_waiting_holds_it() {
    super::seeds("a_card_waiting_holds_it", SEEDS, |c| run("a_card_waiting_holds_it", c, Arm::Card));
}

#[derive(Clone, Copy, PartialEq)]
enum Arm {
    Idle,
    Message,
    Card,
}

/// What the checks keep: the stand-in's lines, when each `/compact` came, the last request's time.
#[derive(Default)]
struct Seen {
    lines: Vec<String>,
    compacts: Vec<u64>,
    request_ms: u64,
    /// Requests answered so far (a turn the scenario ended).
    requests: usize,
    /// The scenario's first input taken (the message at 53 minutes).
    began: bool,
}

fn run(name: &str, choices: Choices, arm: Arm) -> Vec<String> {
    let mut sim = Sim::new(name, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    let (tx, rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    crate::chat::open(&mut ed);
    ed.workspace.claude.usage.fresh("conv-1");
    let seen = RefCell::new(Seen::default());
    // (the turn before: its request wrote 250k tokens to the 1h cache, now)
    message(&mut ed, "hello");
    answer(&mut ed, &mut seen.borrow_mut());
    let inputs = if arm == Arm::Idle { 8 } else { 24 };
    sim.walk(
        &mut ed,
        inputs,
        |sim, ed| input(sim, ed, arm, &seen),
        |ed| held(ed, &rx, &mut seen.borrow_mut()),
        print,
    );
    // (left idle, one — unless the rule read again as it fired landed past the minute: the cache about to go, late)
    let s = seen.borrow();
    let late = ed.workspace.claude.idle.held == Some(crate::idlecompact::Why::Late);
    sim.at_end(|| match (arm, s.compacts.len()) {
        (Arm::Idle, 1) | (Arm::Message | Arm::Card, _) => Ok(()),
        (Arm::Idle, 0) if late => Ok(()),
        (Arm::Idle, n) => Err(format!("left idle: {n} /compact, not one")),
    });
    let trace = sim.trace().to_vec();
    drop(s);
    drop(sim);
    drop(ed);
    trace
}

/// A message sent from the chat's input.
fn message(ed: &mut Editor, text: &str) {
    ed.workspace.claude.chat.as_mut().unwrap().input = text.into();
    crate::chat::send(ed);
}

/// The turn under way answered, as claude's stream says it: a request now that wrote 250k tokens to the 1h cache,
/// its result, its end.
fn answer(ed: &mut Editor, seen: &mut Seen) {
    let generation = ed.workspace.claude.chat.as_ref().unwrap().generation();
    let now = crate::usage::now_ms();
    seen.requests += 1;
    seen.request_ms = now;
    let counts = crate::usage::Counts { input: Some(2), write: Some(250_000), ..Default::default() };
    let usage = crate::usage::Seen {
        id: Some(format!("r{}", seen.requests)),
        model: "m".into(),
        counts,
        ttl: Some(crate::usage::Ttl::Hour1),
        at_ms: now,
    };
    let spent = crate::usage::Spent { conversation: "conv-1".into(), at_ms: now, ..Default::default() };
    for ev in [
        StreamEv::Requesting(now),
        StreamEv::Usage(usage),
        StreamEv::Spent(spent),
        StreamEv::Done(Ok("ok".into())),
    ] {
        crate::chat::stream_event(ed, generation, ev);
    }
}

fn input(sim: &mut Sim, ed: &mut Editor, arm: Arm, seen: &RefCell<Seen>) -> String {
    let busy = ed.workspace.claude.chat.as_ref().is_some_and(|c| c.busy());
    // (the message's scenario starts with it: 53 minutes on, a message)
    if arm == Arm::Message && !std::mem::replace(&mut seen.borrow_mut().began, true) {
        sim.wait(Duration::from_millis(53 * MIN));
        message(ed, "one more thing");
        return "53 min pass, a message".into();
    }
    let c = &mut sim.choices;
    // (by arm: a frame 2, seconds passing 2, then the arm's own: the turn ends 2 · a card comes 2, it goes 2,
    // minutes passing 1)
    let weights: &[usize] = match arm {
        Arm::Idle => &[2, 2],
        Arm::Message => &[2, 2, 2],
        Arm::Card => &[2, 2, 2, 2, 1],
    };
    match (c.weighted(weights), arm) {
        (0, _) => {
            crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
            "a frame".into()
        }
        (1, _) => {
            let ms = *c.pick(&[100, 2000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        (2, Arm::Message) if busy => {
            answer(ed, &mut seen.borrow_mut());
            "the turn ends".into()
        }
        (2, Arm::Message) => "(no turn under way)".into(),
        (2, _) if ed.offers.is_empty() => {
            let what = crate::offer::What::ReviewEnd {
                session: crate::event::WorkspaceId::HOME,
                started: crate::clock::now(),
            };
            ed.push_offer(crate::offer::Offer::new(
                "End review?".into(),
                "2 files not viewed.",
                "",
                String::new(),
                what,
            ));
            "a card comes".into()
        }
        (2, _) => "(a card is up)".into(),
        (3, _) => match ed.offers.is_empty() {
            true => "(no card)".into(),
            false => {
                ed.offers.clear();
                "the card goes".into()
            }
        },
        _ => {
            let m = *c.pick(&[3, 20]);
            sim.wait(Duration::from_millis(m * MIN));
            format!("{m} min pass")
        }
    }
}

/// A `/compact` only while idle (no turn but its own, no card), 54 minutes or more after the last request and less
/// than a minute past that, and one at most a request.
fn held(ed: &Editor, rx: &Receiver<String>, seen: &mut Seen) -> Result<(), String> {
    let now = crate::usage::now_ms();
    let due = seen.request_ms + 54 * MIN;
    for line in rx.try_iter() {
        if line == crate::llm::user_message(crate::idlecompact::COMMAND) {
            if !ed.offers.is_empty() {
                return Err("/compact with a card up".into());
            }
            if now < due || now > due + crate::idlecompact::LATE_MS {
                return Err(format!("/compact {} min after the last request", (now - seen.request_ms) / MIN));
            }
            if seen.compacts.len() >= seen.requests {
                return Err(format!("{} /compact for {} requests", seen.compacts.len() + 1, seen.requests));
            }
            seen.compacts.push(now);
        }
        seen.lines.push(line);
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let c = ed.workspace.claude.chat.as_ref();
    format!(
        "{}{}{}",
        if c.is_some_and(|c| c.busy()) { "busy" } else { "idle" },
        if ed.offers.is_empty() { "" } else { " · a card" },
        match (&ed.workspace.claude.idle.sent, &ed.workspace.claude.idle.held) {
            (Some(_), _) => " · /compact sent".to_string(),
            (None, Some(why)) => format!(" · held: {}", why.name()),
            (None, None) => String::new(),
        }
    )
}
