//! The working sign, simulated (#159, `shimmer.rs`): turns asked through `chat::submit` and their streams — text,
//! thinking's summary and an edit's input in small batches and in lumps, thinking blocks opening and closing, tool calls
//! (a read, a run), their results and claude's requests — frames and time passing in any order.
//!
//! Fixed cases, each in a step of its own: a delta, a few bytes or a lump, draws the turn's cell as it was (the breath
//! goes on; out of a stall, the floor); a whole think (open and silent → breathing, frame after frame, every height
//! reached; its summary landing moves nothing; closed and silent → still, the cell at the floor and held a breath long;
//! the stream again → its breath from the floor, frame by frame half a breath up); a read answered right after words
//! (breathing at once and while Claude's next answer is on its way, past `STILL_AFTER` too; silent after its first
//! words → still); a turn ended and the next asked (breathing from the floor). After every step the turn's pace is the
//! stream's as sent, and a frame draws the turn's cell as it says — the floor while still, else the breath from where
//! the turn's own started (its start, the last stall it left) — and no other height (one sign).
//!
//! Besides, `World` keeps the stream as the scenario sent it and states the rule over it (`World::pace`, `World::from`)
//! — the rule again, so it can't tell a wrong rule from a right one: what it checks is that every event reaches the
//! turn's state (`chat::on_event` → `Chat.pace`), whatever the order they come in.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use super::{Choices, Sim};
use crate::chat::{Msg, Role};
use crate::editor::Editor;
use crate::llm::StreamEv;
use crate::shimmer::{CELLS, Pace, STILL, STILL_AFTER, breath};
use crate::workspace::Doing;

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
    /// The animation step the turn's breath counts from: its start, the last stall it left.
    from: Cell<u128>,
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
        let flowing = self.last.get().is_some_and(|t| crate::clock::since(t) < STILL_AFTER);
        match flowing || self.thinking.get() || runs || self.asked.get() || !self.begun.get() {
            true => Pace::Breath,
            false => Pace::Still,
        }
    }

    fn delta(&self) {
        self.last.set(Some(crate::clock::now()));
        self.begun.set(true);
        self.asked.set(false);
    }

    /// A new turn: nothing of it yet, its breath from now.
    fn turn(&self) {
        self.from.set(crate::editor::anim_step());
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
    world.turn();
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

/// `ev` as the stream sends it, the world told before (`World`'s state for it already set): a turn that was still and
/// breathes again breathes from now.
fn send(ed: &mut Editor, world: &World, ev: StreamEv) {
    let before = pace(ed);
    let g = ed.workspace.claude.chat.as_ref().map_or(0, crate::chat::Chat::generation);
    crate::chat::stream_event(ed, g, ev);
    if before == Some(Pace::Still) && world.pace() == Pace::Breath {
        world.from.set(crate::editor::anim_step());
    }
}

/// The working sign's heights on the last frame, each once.
fn drawn(ed: &Editor) -> Vec<char> {
    let Some((frame, w, h)) = ed.last_frame.as_ref() else { return Vec::new() };
    let screen = crate::screen::text(frame, *w, *h).unwrap_or_default();
    let mut seen: Vec<char> = screen.chars().filter(|c| CELLS.contains(c)).collect();
    seen.sort_unstable();
    seen.dedup();
    seen
}

/// The turn's cell as the world says (`term::working`): the floor while the stream stands still, else the breath from
/// where the turn's own started; `▄` where nothing moves on a clock.
fn want(ed: &Editor, world: &World) -> char {
    match pace(ed) {
        _ if !ed.moving() => CELLS[STILL],
        Some(Pace::Still) => CELLS[0],
        _ => CELLS[breath(crate::editor::anim_step().saturating_sub(world.from.get()))],
    }
}

/// The turn's cell says `▲` or `◦` instead: it waits on you, or a call may be stuck.
fn signed(ed: &Editor) -> bool {
    matches!(ed.doing(&ed.workspace), Doing::Waiting | Doing::Stuck { .. })
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
    send(ed, world, StreamEv::Tool { id: id.clone(), name: name.into(), input });
    format!("{id}: {name}")
}

/// Its result, back to Claude.
fn answer(ed: &mut Editor, world: &World, id: &str) {
    world.calls.borrow_mut().retain(|c| c.0 != id);
    world.asked.set(true);
    world.last.set(None);
    send(ed, world, StreamEv::ToolResult { id: id.into(), text: "ok".into(), error: false });
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
            // (a delta draws the cell as it was — the breath goes on, whatever its size; out of a stall, the floor)
            let was = pace(ed);
            frame(ed, world);
            let before = drawn(ed);
            world.delta();
            send(
                ed,
                world,
                if k == 0 { StreamEv::Text(text.into()) } else { StreamEv::Thought(text.into()) },
            );
            frame(ed, world);
            let now = drawn(ed);
            let floor = ed.moving().then_some(CELLS[0]).into_iter().collect::<Vec<_>>();
            match was {
                _ if before.is_empty() || now.is_empty() => {}
                Some(Pace::Breath) if now != before => world.wrong(format!(
                    "a delta of {} bytes took the cell from {before:?} to {now:?}",
                    text.len()
                )),
                Some(Pace::Still) if now != floor => {
                    world.wrong(format!("out of a stall: {now:?}, not from the floor"))
                }
                _ => {}
            }
            if pace(ed) != Some(Pace::Breath) {
                world.wrong(format!("a delta came: {:?}, not breathing", pace(ed)));
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
            send(ed, world, StreamEv::ThinkingBlock(open));
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
            send(ed, world, StreamEv::ThinkingBlock(true));
            sim.wait(STILL_AFTER + Duration::from_millis(200));
            if pace(ed) != Some(Pace::Breath) {
                world.wrong(format!("a thinking block open and silent: {:?}, not breathing", pace(ed)));
            }
            // (silent, it breathes: a breath's frames, each the breath's height, rising and falling)
            let tick = crate::editor::TICK;
            let mut breath = Vec::new();
            for _ in 0..crate::shimmer::BREATH.as_millis() / tick.as_millis() {
                sim.wait(tick);
                frame(ed, world);
                breath.extend(drawn(ed));
                if drawn(ed) != [want(ed, world)] {
                    world.wrong(format!(
                        "a silent think drew {:?}, its breath {:?}",
                        drawn(ed),
                        want(ed, world)
                    ));
                }
            }
            breath.sort_unstable();
            breath.dedup();
            if ed.moving() && breath.len() < CELLS.len() {
                world.wrong(format!("a silent think's breath reached only {breath:?}"));
            }
            let before = drawn(ed);
            let summary = "Checking how the map keeps its keys, then the clone. ".repeat(3);
            world.delta();
            send(ed, world, StreamEv::Thought(summary));
            frame(ed, world);
            if pace(ed) != Some(Pace::Breath) || drawn(ed) != before {
                world.wrong(format!("its summary landed: {:?}, {before:?} became {:?}", pace(ed), drawn(ed)));
            }
            world.thinking.set(false);
            send(ed, world, StreamEv::ThinkingBlock(false));
            sim.wait(STILL_AFTER);
            // (a quiet: the cell at the floor, and held there a breath long)
            let floor = ed.moving().then_some(CELLS[0]).into_iter().collect::<Vec<_>>();
            for _ in 0..3 {
                frame(ed, world);
                if pace(ed) != Some(Pace::Still) || drawn(ed) != floor && !signed(ed) {
                    world.wrong(format!("closed and silent: {:?}, the cell {:?}", pace(ed), drawn(ed)));
                }
                sim.wait(crate::shimmer::BREATH / 3);
            }
            // (the stream again: its breath from the floor, frame by frame up to the top — never a jump to where the
            // clock went meanwhile; short of `STILL_AFTER`, past which one delta's stream is still again)
            world.delta();
            send(ed, world, StreamEv::Text("Then the clone. ".into()));
            let mut up = Vec::new();
            for k in 0..crate::shimmer::BREATH.as_millis() / tick.as_millis() / 2 {
                if k > 0 {
                    sim.wait(tick);
                }
                frame(ed, world);
                up.extend(drawn(ed));
            }
            let breath: Vec<char> = (0..up.len() as u128).map(|k| CELLS[crate::shimmer::breath(k)]).collect();
            if ed.moving() && !signed(ed) && up != breath {
                world.wrong(format!("out of the stall it drew {up:?}, a breath from the floor {breath:?}"));
            }
            "a whole think, a stall, the stream again".into()
        }
        6 => {
            frame(ed, world);
            "a frame".into()
        }
        8 => {
            // (an edit's input as the reader sends it: its new text's bytes raise the level, as text's do)
            let lump = "let x = 1;\n".repeat(40);
            let add = *c.pick(&["x", "    let first = Thread::new(name);\n", lump.as_str()]);
            world.delta();
            let n = world.calls_made.get();
            let piece = crate::writing::Piece::Text {
                item: 0,
                field: crate::writing::Field::New,
                add: add.into(),
                closed: false,
            };
            send(
                ed,
                world,
                StreamEv::Writing(crate::writing::Delta {
                    call: format!("e{n}"),
                    pieces: vec![piece],
                    done: false,
                }),
            );
            if pace(ed) != Some(Pace::Breath) {
                world.wrong(format!("{} bytes of an edit's input: {:?}, not breathing", add.len(), pace(ed)));
            }
            format!("an edit's input of {} bytes", add.len())
        }
        9 => {
            world.asked.set(true);
            world.last.set(None);
            send(ed, world, StreamEv::Requesting(0));
            "a request goes out".into()
        }
        10 => {
            // (a read answered: Claude's next answer on its way — breathing, however long; its first words, then
            // silent: still)
            if world.thinking.get() || world.calls.borrow().iter().any(|c| c.1) {
                return "(a read waits)".into();
            }
            // (a few words first: the answer's wait breathes at once, never the words' stall)
            world.delta();
            send(ed, world, StreamEv::Text("Let me look. ".into()));
            let said = call(ed, world, false);
            let id = world.calls.borrow().last().map(|c| c.0.clone()).unwrap_or_default();
            answer(ed, world, &id);
            if pace(ed) != Some(Pace::Breath) {
                world.wrong(format!(
                    "a read answered right after words: {:?}, not breathing at once",
                    pace(ed)
                ));
            }
            sim.wait(STILL_AFTER + Duration::from_millis(1500));
            if pace(ed) != Some(Pace::Breath) {
                world.wrong(format!(
                    "a read answered, the next answer on its way: {:?}, not breathing",
                    pace(ed)
                ));
            }
            world.delta();
            send(ed, world, StreamEv::Text("So: ".into()));
            let first = pace(ed);
            sim.wait(STILL_AFTER);
            if (first, pace(ed)) != (Some(Pace::Breath), Some(Pace::Still)) {
                world.wrong(format!("the answer's first words, then silent: {first:?}, {:?}", pace(ed)));
            }
            format!("{said}, answered, the next answer")
        }
        11 => {
            // (the turn over, the next asked through `submit`: nothing of the last one carried over)
            send(ed, world, StreamEv::Done(Ok("done".into())));
            if !crate::chat::submit(ed, "and the third?", None, None) {
                world.wrong("the next turn wasn't asked".into());
            }
            world.turn();
            // (its breath from the floor, as it starts)
            frame(ed, world);
            let floor = ed.moving().then_some(CELLS[0]).into_iter().collect::<Vec<_>>();
            if pace(ed) != Some(Pace::Breath) || drawn(ed) != floor && !signed(ed) {
                world.wrong(format!(
                    "a new turn: {:?}, its cell {:?}, not breathing from the floor",
                    pace(ed),
                    drawn(ed)
                ));
            }
            "the turn ends, the next is asked".into()
        }
        _ => {
            let ms = *c.pick(&[30, 250, 999, 1000, 2000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

fn held(ed: &Editor, world: &World) -> Result<(), String> {
    if let Some(w) = world.wrong.borrow_mut().take() {
        return Err(w);
    }
    let pace = pace(ed);
    if pace != Some(world.pace()) {
        return Err(format!("the turn's pace {pace:?}, the stream as sent says {:?}", world.pace()));
    }
    // (a frame this step: the turn's cell on it as its pace says, and no other height — one sign; none at all only
    // while the turn waits on you or a call may be stuck, its cell then `▲` or `◦`)
    if !world.framed.get() {
        return Ok(());
    }
    let (seen, want) = (drawn(ed), want(ed, world));
    // (no cell only where the turn waits on you or a call may be stuck — `▲`, `◦` take it — or no turn is under way)
    match seen.as_slice() {
        [c] if *c == want => Ok(()),
        [] if signed(ed) || pace.is_none() => Ok(()),
        _ => Err(format!("the frame draws {seen:?}, the turn's cell at {pace:?} is {want:?}")),
    }
}

fn print(ed: &Editor) -> String {
    let c = ed.workspace.claude.chat.as_ref();
    format!("{:?} · {:?}", pace(ed), c.map(|c| c.state))
}
