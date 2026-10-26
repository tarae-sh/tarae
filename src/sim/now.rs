//! What Claude is doing now, simulated (`now.rs`): a turn's calls (a run, a read), their results, its words and its
//! thinking arriving, frames and time passing, in any order — every stream event applied as the main loop applies it
//! (`now::sync` after it).
//!
//! After every step: while the step holds, its time is the clock's since the last look plus what it said then — never
//! the turn's; another step starts again from no more than the time since; and a frame drawn says that time on the
//! status line, after the step's words.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use super::{Choices, Sim};
use crate::chat::{Msg, Role};
use crate::editor::Editor;
use crate::event::Event;
use crate::llm::StreamEv;

const SEEDS: &[u64] = &[0, 1, 2, 7, 10];

const INPUTS: usize = 50;

const LIMIT: Duration = crate::test_results::PATIENCE;

const SIZE: (u16, u16) = (140, 30);

#[test]
fn the_steps_time_is_its_own() {
    super::seeds("the_steps_time_is_its_own", SEEDS, run);
}

#[derive(Default)]
struct World {
    /// The step at the last look: its key, its time then, and when that was.
    last: RefCell<Option<(String, Duration, Instant)>>,
    /// The calls without their result yet.
    calls: RefCell<Vec<String>>,
    made: std::cell::Cell<usize>,
    framed: std::cell::Cell<bool>,
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("the_steps_time_is_its_own", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.docs[0].text = ropey::Rope::from_str("fn main() {}\n");
    let mut c = crate::chat::Chat::new();
    c.msgs = vec![Msg::new(Role::User, "earlier", "")];
    let claude = crate::chat::stand_in(&mut c);
    c.follow.on = false;
    ed.workspace.claude.chat = Some(c);
    assert!(crate::chat::submit(&mut ed, "keep the first strand", None, None), "asked");
    // (as after the key that sent it: every event ends in `now::sync`)
    crate::now::sync(&mut ed);
    let world = World::default();
    let check = |ed: &Editor| {
        while claude.try_recv().is_ok() {}
        held(ed, &world)
    };
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &world), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// An event of the turn's stream, as the main loop applies it.
fn send(ed: &mut Editor, ev: StreamEv) {
    let g = ed.workspace.claude.chat.as_ref().map_or(0, crate::chat::Chat::generation);
    ed.handle_event(Event::Job(Box::new(move |ed: &mut Editor| crate::chat::stream_event(ed, g, ev))));
}

fn input(sim: &mut Sim, ed: &mut Editor, world: &World) -> String {
    world.framed.set(false);
    let c = &mut sim.choices;
    // (by arm: a call 3, its result 2, words 2, a thinking block 1, a frame 2, time passing 4)
    match c.weighted(&[3, 2, 2, 1, 2, 4]) {
        0 => {
            let n = world.made.get() + 1;
            world.made.set(n);
            let id = format!("t{n}");
            let (name, input) = match c.below(2) {
                0 => ("mcp__tarae__run", serde_json::json!({"command": "cargo test -q"})),
                _ => ("mcp__tarae__read", serde_json::json!({"files": ["src/main.rs"]})),
            };
            world.calls.borrow_mut().push(id.clone());
            send(ed, StreamEv::Tool { id: id.clone(), name: name.into(), input });
            format!("{id}: {name}")
        }
        1 => {
            let Some(id) = world.calls.borrow_mut().pop() else { return "(no call waits)".into() };
            send(ed, StreamEv::ToolResult { id: id.clone(), text: "ok".into(), error: false });
            format!("{id} answered")
        }
        2 => {
            let words = *c.pick(&["Let me look", " at the map.", " I will run the tests. "]);
            send(ed, StreamEv::Text(words.into()));
            format!("words {words:?}")
        }
        3 => {
            send(ed, StreamEv::ThinkingBlock(true));
            sim.wait(Duration::from_millis(1500));
            send(ed, StreamEv::ThinkingBlock(false));
            "a think".into()
        }
        4 => {
            crate::term::render(ed, &mut Vec::new(), SIZE.0, SIZE.1).unwrap();
            world.framed.set(true);
            "a frame".into()
        }
        _ => {
            let ms = *c.pick(&[300, 1000, 2500, 61_000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

fn held(ed: &Editor, world: &World) -> Result<(), String> {
    let Some(step) = crate::now::step(ed, &ed.workspace) else { return Ok(()) };
    let took = crate::now::took(&ed.workspace, &step);
    let now = crate::clock::now();
    let last = world.last.replace(Some((step.key.clone(), took, now)));
    if let Some((key, then, at)) = last {
        let since = now.saturating_duration_since(at);
        match key == step.key {
            true if took != then + since => {
                return Err(format!("{} held: {took:?}, not {then:?} + {since:?}", step.key));
            }
            false if took > since => {
                return Err(format!("{} after {key}: {took:?}, more than the {since:?} since", step.key));
            }
            _ => {}
        }
    }
    let turn = ed.workspace.claude.chat.as_ref().map(|c| crate::clock::since(c.started)).unwrap_or_default();
    if took > turn {
        return Err(format!("the step's {took:?} past the turn's {turn:?}"));
    }
    // (a frame this step: the status line says that time after the step's words)
    let Some((frame, w, h)) = ed.last_frame.as_ref().filter(|_| world.framed.get()) else { return Ok(()) };
    let screen = crate::screen::text(frame, *w, *h).unwrap_or_default();
    let line = screen.lines().last().unwrap_or_default();
    let said = format!(" {}", crate::term::running(took, !ed.moving()));
    match line.contains(step.verb) && line.contains(&said) {
        true => Ok(()),
        false => Err(format!("the status line doesn't say {} {said}: {line}", step.verb)),
    }
}

fn print(ed: &Editor) -> String {
    match crate::now::step(ed, &ed.workspace) {
        Some(s) => format!("{} {} · {:?}", s.verb, s.what, crate::now::took(&ed.workspace, &s)),
        None => "no turn".into(),
    }
}
