//! The turn's word, simulated (motion §3, `shimmer.rs`): turns asked through `chat::submit` and their streams — text,
//! thinking's summary and an edit's input in small batches and in lumps, thinking blocks opening and closing, tool calls
//! (a read, a run), their results and claude's requests — frames and time passing in any order.
//!
//! Fixed cases, each in a step of its own: a whole think (open and silent → the wave, its summary landing → the shimmer
//! and two cells, closed and silent → still); a read answered right after words (the wave at once and while Claude's
//! next answer is on its way, past `STILL_AFTER` too; its first words → the shimmer; silent → still); a turn ended and the next asked (the band back
//! at 0, the wave); a lump moves the band two cells; a wait never moves it. After every step: the band where the batches
//! sent it, and a frame draws the wave exactly when the pace is the wave (one sign).
//!
//! Besides, `World` keeps the stream as the scenario sent it and states the rule over it (`World::pace`) — the rule
//! again, so it can't tell a wrong rule from a right one: what it checks is that every event reaches the turn's state
//! (`chat::on_event` → `Chat.pace`), whatever the order they come in.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use super::{Choices, Sim};
use crate::chat::{Msg, Role};
use crate::editor::Editor;
use crate::llm::StreamEv;
use crate::shimmer::{CELLS_MAX, Pace, STILL_AFTER};

const SEEDS: &[u64] = &[0, 1, 2, 7, 10];

const INPUTS: usize = 60;

const LIMIT: Duration = crate::test_results::PATIENCE;

const SIZE: (u16, u16) = (110, 30);

#[test]
fn the_turns_word_moves_only_as_its_stream_arrives() {
    super::seeds("the_turns_word_moves_only_as_its_stream_arrives", SEEDS, run);
}

/// The stream as the scenario sent it, this turn.
#[derive(Default)]
struct World {
    at: Cell<f32>,
    last: Cell<Option<Instant>>,
    thinking: Cell<bool>,
    begun: Cell<bool>,
    /// A result went back or a request went out, and nothing of the answer came since.
    asked: Cell<bool>,
    /// The tool calls without their result yet: (id, a run).
    calls: RefCell<Vec<(String, bool)>>,
    calls_made: Cell<usize>,
    /// A frame drawn this step.
    framed: Cell<bool>,
    /// What a step found wrong (the next check says it).
    wrong: RefCell<Option<String>>,
}

impl World {
    /// The rule over the stream as sent (see the module's words: the wiring's check, not the rule's).
    fn pace(&self) -> Pace {
        let runs = self.calls.borrow().iter().any(|c| c.1);
        match () {
            _ if self.last.get().is_some_and(|t| crate::clock::since(t) < STILL_AFTER) => Pace::Shimmer,
            _ if self.thinking.get() || runs || self.asked.get() || !self.begun.get() => Pace::Wave,
            _ => Pace::Still,
        }
    }

    fn delta(&self, bytes: usize) {
        self.at.set(self.at.get() + (bytes as f32 / 16.0).min(CELLS_MAX));
        self.last.set(Some(crate::clock::now()));
        self.begun.set(true);
        self.asked.set(false);
    }

    /// A new turn: nothing of it yet.
    fn turn(&self) {
        self.at.set(0.0);
        self.last.set(None);
        self.thinking.set(false);
        self.begun.set(false);
        self.asked.set(false);
        self.calls.borrow_mut().clear();
    }

    fn wrong(&self, said: String) {
        self.wrong.borrow_mut().get_or_insert(said);
    }
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("the_turns_word_moves_only_as_its_stream_arrives", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.docs[0].text = ropey::Rope::from_str("fn main() {}\n");
    let mut c = crate::chat::Chat::new();
    c.msgs = vec![Msg::new(Role::User, "why does insert clone the name?", "")];
    let claude = crate::chat::stand_in(&mut c);
    ed.workspace.claude.chat = Some(c);
    ed.motion_drawn = true;
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    let world = World::default();
    assert!(crate::chat::submit(&mut ed, "and the second?", None, None), "the first turn asked");
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

fn send(ed: &mut Editor, ev: StreamEv) {
    let g = ed.workspace.claude.chat.as_ref().map_or(0, crate::chat::Chat::generation);
    crate::chat::stream_event(ed, g, ev);
}

fn at(ed: &Editor) -> f32 {
    ed.workspace.claude.chat.as_ref().map_or(0.0, |c| c.pace.at)
}

fn pace(ed: &Editor) -> Option<Pace> {
    crate::shimmer::pace(&ed.workspace)
}

fn frame(ed: &mut Editor, world: &World) {
    crate::term::render(ed, &mut Vec::new(), SIZE.0, SIZE.1).unwrap();
    world.framed.set(true);
}

/// A call of Claude's, read or run, sent as the stream sends it.
fn call(ed: &mut Editor, world: &World, run: bool) -> String {
    let n = world.calls_made.get() + 1;
    world.calls_made.set(n);
    let id = format!("call{n}");
    let (name, input) = match run {
        true => ("mcp__tarae__run", serde_json::json!({"command": "cargo test"})),
        false => ("mcp__tarae__read", serde_json::json!({"files": ["src/main.rs"]})),
    };
    world.calls.borrow_mut().push((id.clone(), run));
    world.begun.set(true);
    world.asked.set(false);
    world.last.set(None);
    send(ed, StreamEv::Tool { id: id.clone(), name: name.into(), input });
    format!("{id}: {name}")
}

/// Its result, back to Claude.
fn answer(ed: &mut Editor, world: &World, id: &str) {
    world.calls.borrow_mut().retain(|c| c.0 != id);
    world.asked.set(true);
    world.last.set(None);
    send(ed, StreamEv::ToolResult { id: id.into(), text: "ok".into(), error: false });
}

fn input(sim: &mut Sim, ed: &mut Editor, world: &World) -> String {
    world.framed.set(false);
    let c = &mut sim.choices;
    // (by arm: text arrives 5, thinking's summary 2, a thinking block opens or closes 2, a tool call 2, its result 2,
    // a whole think 1, a frame 3, time passing 4, an edit's input 2, a request goes out 1, a read answered and the next
    // answer 1, the turn ends and the next is asked 1)
    match c.weighted(&[5, 2, 2, 2, 2, 1, 3, 4, 2, 1, 1, 1]) {
        k @ (0 | 1) => {
            let lump = "a lump of words, all at once. ".repeat(14);
            let text = *c.pick(&["word ", "another needle in the map ", lump.as_str()]);
            let before = at(ed);
            world.delta(text.len());
            send(ed, if k == 0 { StreamEv::Text(text.into()) } else { StreamEv::Thought(text.into()) });
            let moved = at(ed) - before;
            if text.len() >= 32 && moved != CELLS_MAX {
                world.wrong(format!("a lump of {} bytes moved the band {moved} cells", text.len()));
            }
            if pace(ed) != Some(Pace::Shimmer) {
                world.wrong(format!("a delta came: {:?}, not the shimmer", pace(ed)));
            }
            format!("{} of {} bytes", if k == 0 { "text" } else { "a summary" }, text.len())
        }
        2 => {
            let open = !world.thinking.get();
            world.thinking.set(open);
            world.begun.set(true);
            if open {
                world.asked.set(false);
                world.last.set(None);
            }
            send(ed, StreamEv::ThinkingBlock(open));
            format!("a thinking block {}", if open { "opens" } else { "closes" })
        }
        3 => {
            let run = c.below(2) == 0;
            call(ed, world, run)
        }
        4 => {
            let Some((id, _)) = world.calls.borrow().last().cloned() else { return "(no call waits)".into() };
            answer(ed, world, &id);
            format!("{id} answered")
        }
        5 => {
            // (a whole think, as measured: open, silent, its summary whole near its end, closed, silent)
            if world.thinking.get() || world.calls.borrow().iter().any(|c| c.1) {
                return "(a think waits)".into();
            }
            world.thinking.set(true);
            world.asked.set(false);
            world.last.set(None);
            send(ed, StreamEv::ThinkingBlock(true));
            sim.wait(STILL_AFTER + Duration::from_millis(200));
            if pace(ed) != Some(Pace::Wave) {
                world.wrong(format!("a thinking block open and silent: {:?}, not the wave", pace(ed)));
            }
            let before = at(ed);
            let summary = "Checking how the map keeps its keys, then the clone. ".repeat(3);
            world.delta(summary.len());
            send(ed, StreamEv::Thought(summary));
            if pace(ed) != Some(Pace::Shimmer) || at(ed) - before != CELLS_MAX {
                world.wrong(format!(
                    "its summary landed: {:?}, the band {} cells on",
                    pace(ed),
                    at(ed) - before
                ));
            }
            world.thinking.set(false);
            send(ed, StreamEv::ThinkingBlock(false));
            sim.wait(STILL_AFTER);
            if pace(ed) != Some(Pace::Still) || at(ed) - before != CELLS_MAX {
                world.wrong(format!(
                    "closed and silent: {:?}, the band {} cells on",
                    pace(ed),
                    at(ed) - before
                ));
            }
            "a whole think".into()
        }
        6 => {
            frame(ed, world);
            "a frame".into()
        }
        8 => {
            // (an edit's input as the reader sends it: its new text's bytes move the band, as text's do)
            let lump = "let x = 1;\n".repeat(40);
            let add = *c.pick(&["x", "    let first = Thread::new(name);\n", lump.as_str()]);
            let before = at(ed);
            world.delta(add.len());
            let n = world.calls_made.get();
            let piece = crate::writing::Piece::Text {
                item: 0,
                field: crate::writing::Field::New,
                add: add.into(),
                closed: false,
            };
            send(
                ed,
                StreamEv::Writing(crate::writing::Delta {
                    call: format!("e{n}"),
                    pieces: vec![piece],
                    done: false,
                }),
            );
            let (moved, want) = (at(ed) - before, (add.len() as f32 / 16.0).min(CELLS_MAX));
            if (moved - want).abs() > 1e-4 || pace(ed) != Some(Pace::Shimmer) {
                world.wrong(format!(
                    "{} bytes of an edit's input: the band {moved} cells on, {:?}",
                    add.len(),
                    pace(ed)
                ));
            }
            format!("an edit's input of {} bytes", add.len())
        }
        9 => {
            world.asked.set(true);
            world.last.set(None);
            send(ed, StreamEv::Requesting(0));
            "a request goes out".into()
        }
        10 => {
            // (a read answered: Claude's next answer on its way — the wave, however long; its first words the shimmer;
            // then silent, still)
            if world.thinking.get() || world.calls.borrow().iter().any(|c| c.1) {
                return "(a read waits)".into();
            }
            // (a few words first: the answer's wait is the wave at once, never the words' shimmer held on)
            let words = "Let me look. ";
            world.delta(words.len());
            send(ed, StreamEv::Text(words.into()));
            let said = call(ed, world, false);
            let id = world.calls.borrow().last().map(|c| c.0.clone()).unwrap_or_default();
            answer(ed, world, &id);
            if pace(ed) != Some(Pace::Wave) {
                world.wrong(format!(
                    "a read answered right after words: {:?}, not the wave at once",
                    pace(ed)
                ));
            }
            sim.wait(STILL_AFTER + Duration::from_millis(1500));
            if pace(ed) != Some(Pace::Wave) {
                world.wrong(format!(
                    "a read answered, the next answer on its way: {:?}, not the wave",
                    pace(ed)
                ));
            }
            let words = "So: ";
            world.delta(words.len());
            send(ed, StreamEv::Text(words.into()));
            let first = pace(ed);
            sim.wait(STILL_AFTER);
            if (first, pace(ed)) != (Some(Pace::Shimmer), Some(Pace::Still)) {
                world.wrong(format!("the answer's first words, then silent: {first:?}, {:?}", pace(ed)));
            }
            format!("{said}, answered, the next answer")
        }
        11 => {
            // (the turn over, the next asked through `submit`: nothing of the last one carried over)
            send(ed, StreamEv::Done(Ok("done".into())));
            if !crate::chat::submit(ed, "and the third?", None, None) {
                world.wrong("the next turn wasn't asked".into());
            }
            world.turn();
            if at(ed) != 0.0 || pace(ed) != Some(Pace::Wave) {
                world.wrong(format!(
                    "a new turn: the band at {}, {:?}, not the wave from 0",
                    at(ed),
                    pace(ed)
                ));
            }
            "the turn ends, the next is asked".into()
        }
        _ => {
            let ms = *c.pick(&[30, 250, 999, 1000, 2000]);
            let before = at(ed);
            sim.wait(Duration::from_millis(ms));
            if at(ed) != before {
                world.wrong(format!("{ms} ms of nothing moved the band"));
            }
            format!("{ms} ms pass")
        }
    }
}

fn held(ed: &Editor, world: &World) -> Result<(), String> {
    if let Some(w) = world.wrong.borrow_mut().take() {
        return Err(w);
    }
    let (now, want) = (at(ed), world.at.get());
    if (now - want).abs() > 1e-4 {
        return Err(format!("the band at {now}, the stream sent it to {want}"));
    }
    let pace = pace(ed);
    if pace != Some(world.pace()) {
        return Err(format!("the turn's pace {pace:?}, the stream as sent says {:?}", world.pace()));
    }
    // (a frame this step: the wave on it exactly when the pace is the wave — one sign)
    let Some((frame, w, h)) = ed.last_frame.as_ref().filter(|_| world.framed.get()) else { return Ok(()) };
    let screen = crate::screen::text(frame, *w, *h).unwrap_or_default();
    let waves = screen.contains(['▂', '▃', '▄', '▅', '▆', '▇']);
    if waves != (pace == Some(Pace::Wave)) {
        return Err(format!("the frame {} the wave at {pace:?}", if waves { "draws" } else { "lacks" }));
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let c = ed.workspace.claude.chat.as_ref();
    format!("band at {:.2} · {:?} · {:?}", at(ed), pace(ed), c.map(|c| c.state))
}
