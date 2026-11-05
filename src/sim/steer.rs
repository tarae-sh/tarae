//! Steering a turn, simulated (the flow plan's piece 5, Q6 a): a chat at work, its claude a stand-in. At random a turn
//! begins, Claude calls a tool that waits (its answer a job's result, landing when the stream picks), one answered at
//! once (`plan`), the user sends (`enter`: queued while Claude works) and maybe steers it (`tab`), the turn ends, a frame. After every step: every word the
//! user steered is in exactly one place — waiting to steer, queued (the turn ended first), or sent to claude once, in
//! the order typed; a turn over leaves nothing steering; what an answer carried has its row in the conversation, what
//! went as a message none.

use std::cell::RefCell;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use super::{Choices, Sim};
use crate::chat::{Chat, State};
use crate::editor::Editor;
use crate::key::{Code, Key};

const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 40;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_steer_meets_the_answers() {
    super::seeds("a_steer_meets_the_answers", SEEDS, run);
}

struct World {
    n: usize,
    /// The words steered, in order (`[sN]`), and those queued (`[qN]`).
    steered: Vec<String>,
    queued: Vec<String>,
    lines: Receiver<String>,
    /// Every line the stand-in got so far.
    got: Vec<String>,
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("a_steer_meets_the_answers", choices, LIMIT);
    let mut ed = crate::uistate::tests::editor();
    let mut c = Chat::new();
    let lines = crate::chat::stand_in(&mut c);
    ed.workspace.claude.chat = Some(c);
    ed.workspace.set_at(crate::workspace::Here::Chat);
    let w = RefCell::new(World { n: 0, steered: Vec::new(), queued: Vec::new(), lines, got: Vec::new() });
    let check = |ed: &Editor| held(ed, &mut w.borrow_mut());
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &mut w.borrow_mut()), check, print);
    sim.trace().to_vec()
}

fn chat(ed: &Editor) -> &Chat {
    ed.workspace.claude.chat.as_ref().unwrap()
}

fn input(sim: &mut Sim, ed: &mut Editor, w: &mut World) -> String {
    let c = &mut sim.choices;
    let busy = chat(ed).busy();
    let generation = chat(ed).generation();
    // (by arm: a turn begins 2, a tool that waits 3, one answered at once 2, a steer 3, a queued message 2, the turn
    // ends 2, a frame 1)
    let arm = c.weighted(&[2, 3, 2, 3, 2, 2, 1]);
    match arm {
        0 if !busy => {
            let c = ed.workspace.claude.chat.as_mut().unwrap();
            (c.state, c.started) = (State::Thinking, crate::clock::now());
            "a turn begins".into()
        }
        0 => "(a turn goes)".into(),
        // (its answer the job's result: the stream decides when it lands, steers before or after it)
        1 if busy => {
            w.n += 1;
            let n = w.n as u64;
            ed.events.jobs().spawn(move || {
                move |ed: &mut Editor| {
                    let call = crate::tools::Call::of(generation, &format!("r{n}"), serde_json::json!(n));
                    crate::tools::answer(ed, call, Ok(format!("[a{n}]")));
                }
            });
            format!("Claude calls a tool that waits, [a{n}]")
        }
        1 => "(no turn: no call)".into(),
        2 if busy => {
            w.n += 1;
            let n = w.n;
            let msg = serde_json::json!({ "jsonrpc": "2.0", "id": n, "method": "tools/call",
                "params": { "name": "plan", "arguments": { "steps": [{ "text": format!("[p{n}]"), "status": "doing" }] } } });
            let r = crate::tools::mcp(ed, generation, &format!("r{n}"), &msg).expect("answered at once");
            w.got.push(r.to_string());
            format!("Claude's plan [p{n}], answered at once")
        }
        2 => "(no turn: no plan)".into(),
        3 | 4 => {
            let steer = arm == 3;
            w.n += 1;
            let token = format!("[{}{}]", if steer { 's' } else { 'q' }, w.n);
            // (sent — queued while Claude works — then, to steer, `tab`: what's queued, with any queued before it)
            ed.workspace.claude.chat.as_mut().unwrap().input = token.as_str().into();
            ed.handle_key(Key::plain(Code::Enter));
            if steer {
                ed.handle_key(Key::plain(Code::Tab));
                // (nothing to steer — it went at once: a tab typed in the empty input, cleared)
                ed.workspace.claude.chat.as_mut().unwrap().input.clear();
            }
            match steer && busy {
                true => w.steered.push(token.clone()),
                false => w.queued.push(token.clone()),
            }
            format!("the user {} {token}", if steer { "steers" } else { "sends" })
        }
        5 if busy => {
            crate::chat::stand_in_done(ed);
            "the turn ends".into()
        }
        5 => "(no turn to end)".into(),
        _ => {
            crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
            "a frame".into()
        }
    }
}

/// Every steered word in one place, once, in order; nothing steering past a turn's end; a row for each answer's.
fn held(ed: &Editor, w: &mut World) -> Result<(), String> {
    let got: Vec<String> = w.lines.try_iter().collect();
    w.got.extend(got);
    let c = chat(ed);
    if !c.busy() && c.steer.is_some() {
        return Err("no turn under way, and words still wait to steer it".into());
    }
    let waits = |t: &str| c.steer.as_deref().is_some_and(|s| s.contains(t));
    let queued = |t: &str| c.queued.as_deref().is_some_and(|s| s.contains(t));
    let sent = |t: &str| w.got.iter().filter(|l| l.contains(t)).count();
    let mut last = 0;
    for t in w.steered.iter().chain(&w.queued) {
        let places = usize::from(waits(t)) + usize::from(queued(t)) + sent(t);
        if places != 1 {
            return Err(format!(
                "{t} is in {places} places (steering {}, queued {}, sent {})",
                waits(t),
                queued(t),
                sent(t)
            ));
        }
    }
    for t in &w.steered {
        let Some(at) = w.got.iter().position(|l| l.contains(t.as_str())) else { continue };
        if at < last {
            return Err(format!("{t} went before a steer typed ahead of it"));
        }
        last = at;
        let line = &w.got[at];
        let framed = line.contains("while you work");
        let row = c.msgs.iter().any(|m| crate::chat::is_steered(&m.text) && m.text.contains(t.as_str()));
        if framed != row {
            return Err(format!("{t}: framed in an answer {framed}, its row in the conversation {row}"));
        }
        // (in the one frame of its process's id — as its line has it, JSON's)
        let id = format!("<editor-note id=\\\"{}\\\">", crate::quote::frame_id(c.generation()));
        if framed && !line.contains(&id) {
            return Err(format!("{t}: framed with no frame of its process's id: {line}"));
        }
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let c = chat(ed);
    format!("busy {} · steering {:?} · queued {:?}", c.busy(), c.steer, c.queued)
}
