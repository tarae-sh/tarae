//! A chat turn on an ACP agent, simulated (agents beside Claude, step 2's plan §5; the wire with no process,
//! `agent::acp::Staged`): the agent's own permission ask, its call of tarae's `read` through the bridge — of a file
//! outside the project, which waits on the read card — and its words, in any order; each event the wire makes landing
//! on the loop when the stream says, the wire taking what the chat handed its handle when the stream says, the stop key
//! once, the card answered, frames. The path's one ordering rule is the stop while a permission ask is open: the
//! chat's refusal of the ask and the stop reach the wire in either order, and the ask is answered once — `cancelled`
//! when the stop came first (the spec's MUST), the chat's refusal otherwise.
//!
//! After every step: the ask is answered at most once and never allowed (condition 1) — open when `session/cancel`
//! went, before it; a bridged request is answered at most once. At the end (the agent's turn over: `cancelled` after
//! a stop, else `end_turn`): the chat is idle, the ask answered once, and the read's answer — the file's text, the
//! card said yes to — went back through the bridge once, and came up as the call's result.
//!
//! A second scenario, `an_own_tool_trips_the_session` (step 3's guard 3): the agent runs one of its own tools to its
//! end (`in_progress`, then `completed`), calls tarae's `read` of the file outside (its card), and the stop key and a
//! second message of yours come, in any order. After every step: nothing is written to the agent after the trip, nor
//! sent back through the bridge; the trip is said once in the transcript; once its end lands, no read card is up. At
//! the end: the chat is idle, the trip said once, and its row held until you quit.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use super::{Choices, Sim};
use crate::agent::Event;
use crate::agent::acp::Staged;
use crate::editor::Editor;

const NAME: &str = "a_turn_on_an_acp_agent_with_a_stop_and_an_ask";

const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5, 6, 7];

const INPUTS: usize = 30;

/// The inputs after them that end the turn (most of them settle rounds while a job runs between).
const ENDING: usize = 40;

const LIMIT: Duration = crate::test_results::PATIENCE;

/// What the file outside the project says.
const WORDS: &str = "the notes outside";

/// How the fixed seeds' asks were answered: the stop's `cancelled` first, the chat's refusal first.
static CANCELLED: AtomicUsize = AtomicUsize::new(0);
static REFUSED: AtomicUsize = AtomicUsize::new(0);

#[test]
fn a_turn_on_an_acp_agent_with_a_stop_and_an_ask() {
    super::seeds(NAME, SEEDS, run);
    let (cancelled, refused) = (CANCELLED.load(Ordering::SeqCst), REFUSED.load(Ordering::SeqCst));
    assert!(
        cancelled > 0 && refused > 0,
        "the ask answered one way only: {cancelled} cancelled, {refused} refused"
    );
}

struct World {
    staged: RefCell<Staged>,
    /// What went back through the bridge.
    bridge: std::sync::mpsc::Receiver<String>,
    back: RefCell<Vec<Value>>,
    /// Every line written to the agent, in order.
    to_agent: RefCell<Vec<Value>>,
    /// The events the wire made, not landed yet.
    pending: RefCell<VecDeque<Event>>,
    /// The agent's ask, its call and its words: said yet.
    said: Cell<[bool; 3]>,
    stopped: Cell<bool>,
    /// The file outside the project its call reads.
    outside: std::path::PathBuf,
    /// The call's `ToolResult` landed.
    result: Cell<bool>,
    /// How many lines had gone to the agent when its ask was read: one written after is open to a stop.
    asked_at: Cell<Option<usize>>,
}

impl World {
    /// What was written to the agent and back through the bridge, read up.
    fn read_up(&self) {
        self.to_agent.borrow_mut().extend(self.staged.borrow().written());
        let back = self.bridge.try_iter().filter_map(|l| serde_json::from_str::<Value>(&l).ok());
        self.back.borrow_mut().extend(back);
    }

    fn made(&self, events: Vec<Event>) {
        self.pending.borrow_mut().extend(events);
    }
}

fn run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-acp");
    let root = std::fs::canonicalize(scratch.path()).unwrap();
    let (project, outside) = (root.join("project"), root.join("outside"));
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("notes.txt"), format!("{WORDS}\n")).unwrap();
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = project.clone();
    let (mut staged, agent) = Staged::new(&project);
    let mut c = crate::chat::Chat::new();
    c.agent = &crate::agent::STAND_IN;
    c.hidden = false;
    crate::chat::stand_in_agent(&mut c, agent);
    ed.workspace.claude.chat = Some(c);
    // The handshake: the agent's answers to `initialize` and `session/new`, their events landed
    let mut events = staged.line(&json!({"jsonrpc": "2.0", "id": 0, "result": {"protocolVersion": 1}}));
    let session = json!({"sessionId": "s-1", "modes": {"currentModeId": "default"}});
    events.extend(staged.line(&json!({"jsonrpc": "2.0", "id": 1, "result": session})));
    for ev in events {
        land(&mut ed, ev);
    }
    let bridge = staged.link();
    assert!(crate::chat::submit(&mut ed, "read the notes outside", None, None), "the turn asked");
    let world = World {
        staged: RefCell::new(staged),
        bridge,
        back: RefCell::new(Vec::new()),
        to_agent: RefCell::new(Vec::new()),
        pending: RefCell::new(VecDeque::new()),
        said: Cell::new([false; 3]),
        stopped: Cell::new(false),
        outside: outside.join("notes.txt"),
        result: Cell::new(false),
        asked_at: Cell::new(None),
    };
    let check = |ed: &Editor| held(ed, &world);
    let print = |ed: &Editor| print(ed, &world);
    // (the end, once the scenario's inputs are spent — the read card answered within them, so the walk goes quiet: a
    // call waiting on a card is watched, and its watch wakes for as long as it waits: what the agent hasn't said, said;
    // everything handed and landed and the card answered, until all is in; then the agent's turn over)
    let (n, over) = (Cell::new(0), Cell::new(false));
    let step = |sim: &mut Sim, ed: &mut Editor| {
        n.set(n.get() + 1);
        if n.get() <= INPUTS {
            return input(sim, ed, &world);
        }
        if world.said.get().contains(&false) {
            return say(&world, world.said.get().iter().position(|s| !s).unwrap());
        }
        if over.get() || !all_in(ed, &world) {
            return settle(ed, &world);
        }
        over.set(true);
        let prompt =
            world.to_agent.borrow().iter().find(|v| v["method"] == "session/prompt").map(|v| v["id"].clone());
        let reason = if world.stopped.get() { "cancelled" } else { "end_turn" };
        let line = json!({"jsonrpc": "2.0", "id": prompt, "result": {"stopReason": reason}});
        world.made(world.staged.borrow_mut().line(&line));
        format!("the agent's turn over: {reason} · {}", settle(ed, &world))
    };
    sim.walk(&mut ed, INPUTS + ENDING, step, check, print);
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let idle = !c.busy();
    let to_agent = world.to_agent.borrow().clone();
    let back = world.back.borrow().clone();
    let result = world.result.get();
    sim.at_end(|| {
        if !idle {
            return Err("the turn's end left the chat busy".into());
        }
        let answers: Vec<&Value> =
            to_agent.iter().filter(|v| v["id"] == 5 && v.get("method").is_none()).collect();
        if answers.len() != 1 {
            return Err(format!("the ask answered {} times", answers.len()));
        }
        let read: Vec<&Value> = back.iter().filter(|v| v["id"] == 9).collect();
        if read.len() != 1 || !read[0].to_string().contains(WORDS) {
            return Err(format!("the read's answer back through the bridge: {read:?}"));
        }
        if !result {
            return Err("the call's result never came up".into());
        }
        match answers[0]["result"]["outcome"]["outcome"].as_str() {
            Some("cancelled") => CANCELLED.fetch_add(1, Ordering::SeqCst),
            _ => REFUSED.fetch_add(1, Ordering::SeqCst),
        };
        Ok(())
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    ed.workspace.claude.chat = None;
    drop(ed);
    trace
}

/// An event of the wire's, landed as the loop lands the reader's.
fn land(ed: &mut Editor, ev: Event) {
    let g = ed.workspace.claude.chat.as_ref().map_or(0, crate::chat::Chat::generation);
    ed.handle_event(crate::event::Event::Job(Box::new(move |ed: &mut Editor| {
        crate::chat::stream_event(ed, g, ev)
    })));
}

/// The read card in front, answered `y`. Whether one was.
fn answer_card(ed: &mut Editor) -> bool {
    if !ed.offers.first().is_some_and(|o| matches!(o.what, crate::offer::What::Read { .. })) {
        return false;
    }
    crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
    ed.offer_key(crate::key::Key::plain(crate::key::Code::Char('y')))
}

/// One round of the end: every event landed, every message the chat handed taken, the card answered.
fn settle(ed: &mut Editor, w: &World) -> String {
    let mut n = 0;
    loop {
        let ev = w.pending.borrow_mut().pop_front();
        let moved = match ev {
            Some(ev) => {
                note(w, &ev);
                land(ed, ev);
                true
            }
            None => match w.staged.borrow_mut().take() {
                Some((_, events)) => {
                    w.made(events);
                    true
                }
                None => answer_card(ed),
            },
        };
        if !moved {
            break;
        }
        n += 1;
    }
    format!("settled ({n} moves)")
}

/// Whether everything of the turn is in: nothing waits to land or to be taken, no card is up, the read answered
/// back through the bridge and its result landed, the ask answered.
fn all_in(ed: &Editor, w: &World) -> bool {
    w.read_up();
    let reading = ed.workspace.claude.chat.as_ref().is_some_and(|c| !c.reads.is_empty());
    let asked = w.to_agent.borrow().iter().any(|v| v["id"] == 5 && v.get("method").is_none());
    w.pending.borrow().is_empty() && !reading && ed.offers.is_empty() && w.result.get() && asked
}

/// The agent says its `i`th: its own ask, the read through the bridge, its words.
fn say(w: &World, i: usize) -> String {
    let mut said = w.said.get();
    said[i] = true;
    w.said.set(said);
    let mut staged = w.staged.borrow_mut();
    let (events, what) = match i {
        0 => {
            let call = json!({"toolCallId": "t1", "name": "write_file", "title": "Write a.txt",
                "kind": "edit", "status": "pending", "rawInput": {"path": "a.txt"}});
            let params = json!({"sessionId": "s-1", "toolCall": call,
                "options": crate::agent::acp::standin::agy_options()});
            let ask =
                json!({"jsonrpc": "2.0", "id": 5, "method": "session/request_permission", "params": params});
            (staged.line(&ask), "its own ask (write_file)")
        }
        1 => {
            let files = [w.outside.to_string_lossy()];
            let call = json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
                "params": {"name": "read", "arguments": {"files": files}}});
            (staged.bridged(call), "a read through the bridge")
        }
        _ => {
            let text = json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Reading."}});
            let update = json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s-1", "update": text}});
            (staged.line(&update), "its words")
        }
    };
    drop(staged);
    w.made(events);
    if i == 0 {
        w.read_up();
        w.asked_at.set(Some(w.to_agent.borrow().len()));
    }
    format!("the agent says {what}")
}

/// What the scenario keeps of an event as it lands.
fn note(w: &World, ev: &Event) {
    if let Event::ToolResult { id, .. } = ev
        && id.starts_with("bridge-")
    {
        w.result.set(true);
    }
}

fn input(sim: &mut Sim, ed: &mut Editor, w: &World) -> String {
    let c = &mut sim.choices;
    // (by arm: the agent says its next 3, an event lands 4, the wire takes what the chat handed 4, the stop key 1,
    // the read card answered 2, a frame 1)
    match c.weighted(&[3, 4, 4, 1, 2, 1]) {
        0 => {
            let left: Vec<usize> = (0..3).filter(|&i| !w.said.get()[i]).collect();
            if left.is_empty() {
                return "(the agent has said all)".into();
            }
            let i = *c.pick(&left);
            say(w, i)
        }
        1 => {
            let Some(ev) = w.pending.borrow_mut().pop_front() else { return "(no event to land)".into() };
            note(w, &ev);
            land(ed, ev);
            "an event lands".into()
        }
        2 => {
            let Some((what, events)) = w.staged.borrow_mut().take() else {
                return "(nothing handed to take)".into();
            };
            w.made(events);
            format!("the wire takes {what}")
        }
        3 if !w.stopped.get() && ed.workspace.claude.chat.as_ref().is_some_and(|c| c.busy()) => {
            w.stopped.set(true);
            crate::chat::stop(ed);
            "the stop key".into()
        }
        3 => "(stopped already)".into(),
        4 => match answer_card(ed) {
            true => "y on the read card".into(),
            false => "(no read card)".into(),
        },
        _ => {
            crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
            "a frame".into()
        }
    }
}

/// The ask answered at most once and never allowed — and, open when `session/cancel` went, answered before it (the
/// stop's `cancelled`); a bridged request answered at most once.
fn held(_: &Editor, w: &World) -> Result<(), String> {
    w.read_up();
    let to_agent = w.to_agent.borrow();
    let answers: Vec<(usize, &Value)> =
        to_agent.iter().enumerate().filter(|(_, v)| v["id"] == 5 && v.get("method").is_none()).collect();
    if answers.len() > 1 {
        return Err(format!("the ask answered {} times", answers.len()));
    }
    if let Some((_, a)) = answers.first()
        && a["result"]["outcome"]["optionId"] == "allow"
    {
        return Err("the agent's own ask allowed".into());
    }
    if let Some(cancel) = to_agent.iter().position(|v| v["method"] == "session/cancel")
        && w.asked_at.get().is_some_and(|asked| asked <= cancel)
    {
        match answers.first() {
            Some((at, _)) if *at > cancel => {
                return Err(format!("the ask, open at the cancel ({cancel}), answered after it ({at})"));
            }
            None => return Err(format!("the ask, open at the cancel ({cancel}), left unanswered")),
            Some(_) => {}
        }
    }
    let back = w.back.borrow();
    if back.iter().filter(|v| v["id"] == 9).count() > 1 {
        return Err("the read answered twice through the bridge".into());
    }
    Ok(())
}

// ── An own tool trips the session ──────────────────────────────────────────────────────────────────

const TRIP: &str = "an_own_tool_trips_the_session";

const TRIP_INPUTS: usize = 40;

/// The inputs after them: what the agent hasn't said, then every event landed and the turn's answer.
const TRIP_ENDING: usize = 40;

/// The trip's words in the chat (the turn's end, or its end's).
const TRIPPED: &str = "stand-in ran a tool of its own, run_command — stopped: it and everything it started ended. Only \
                       tarae's tools may run; its hook should have refused this.";

/// How the fixed seeds' reads went: answered back through the bridge before the trip, or cut off by it.
static READ_BEFORE: AtomicUsize = AtomicUsize::new(0);
static READ_CUT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn an_own_tool_trips_the_session() {
    super::seeds(TRIP, SEEDS, trip_run);
    let (before, cut) = (READ_BEFORE.load(Ordering::SeqCst), READ_CUT.load(Ordering::SeqCst));
    assert!(before > 0 && cut > 0, "the read one way only: {before} answered before the trip, {cut} cut off");
}

struct Trip {
    staged: RefCell<Staged>,
    bridge: std::sync::mpsc::Receiver<String>,
    back: RefCell<Vec<Value>>,
    to_agent: RefCell<Vec<Value>>,
    pending: RefCell<VecDeque<Event>>,
    /// Its own tool running, its own tool done, the read through the bridge: said yet.
    said: Cell<[bool; 3]>,
    stopped: Cell<bool>,
    again: Cell<bool>,
    outside: std::path::PathBuf,
    /// How many lines had gone to the agent, and back through the bridge, when the wire tripped.
    tripped_at: Cell<Option<(usize, usize)>>,
    /// The trip's end has landed (its row held), as a check saw.
    end_seen: Cell<bool>,
}

impl Trip {
    fn read_up(&self) {
        self.to_agent.borrow_mut().extend(self.staged.borrow().written());
        let back = self.bridge.try_iter().filter_map(|l| serde_json::from_str::<Value>(&l).ok());
        self.back.borrow_mut().extend(back);
    }

    /// What the wire made: where it tripped, noted.
    fn made(&self, events: Vec<Event>) {
        let tripped = events.iter().any(|e| {
            matches!(e, Event::Exit(crate::agent::Exit { kind: crate::agent::Ended::Tripped { .. }, .. }))
        });
        if tripped {
            self.read_up();
            self.tripped_at.set(Some((self.to_agent.borrow().len(), self.back.borrow().len())));
        }
        self.pending.borrow_mut().extend(events);
    }
}

fn trip_run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-acp-trip");
    let root = std::fs::canonicalize(scratch.path()).unwrap();
    let (project, outside) = (root.join("project"), root.join("outside"));
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("notes.txt"), format!("{WORDS}\n")).unwrap();
    let mut sim = Sim::new(TRIP, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = project.clone();
    let (mut staged, agent) = Staged::new(&project);
    let mut c = crate::chat::Chat::new();
    c.agent = &crate::agent::STAND_IN;
    c.hidden = false;
    crate::chat::stand_in_agent(&mut c, agent);
    ed.workspace.claude.chat = Some(c);
    // The handshake, in the row's mode
    let session = json!({"sessionId": "s-1", "modes": {"currentModeId": "default"}});
    let mut events = staged.line(&json!({"jsonrpc": "2.0", "id": 0, "result": {"protocolVersion": 1}}));
    events.extend(staged.line(&json!({"jsonrpc": "2.0", "id": 1, "result": session})));
    for ev in events {
        land(&mut ed, ev);
    }
    let bridge = staged.link();
    assert!(crate::chat::submit(&mut ed, "read the notes outside", None, None), "the turn asked");
    let world = Trip {
        staged: RefCell::new(staged),
        bridge,
        back: RefCell::new(Vec::new()),
        to_agent: RefCell::new(Vec::new()),
        pending: RefCell::new(VecDeque::new()),
        said: Cell::new([false; 3]),
        stopped: Cell::new(false),
        again: Cell::new(false),
        outside: outside.join("notes.txt"),
        tripped_at: Cell::new(None),
        end_seen: Cell::new(false),
    };
    let check = |ed: &Editor| trip_held(ed, &world);
    let print = |ed: &Editor| trip_print(ed, &world);
    // (the end: the read and its own tool's run, if not said yet; the read's answer back through the bridge where it
    // hadn't tripped yet — its card answered in the settling; then its own tool done, everything landed and taken, and
    // its turn's answer, which nothing reads after the trip)
    let (n, over) = (Cell::new(0), Cell::new(false));
    let step = |sim: &mut Sim, ed: &mut Editor| {
        n.set(n.get() + 1);
        if n.get() <= TRIP_INPUTS {
            return trip_input(sim, ed, &world);
        }
        let said = world.said.get();
        if let Some(i) = [2, 0].into_iter().find(|&i| !said[i]) {
            return trip_say(&world, i);
        }
        if !said[1] {
            world.read_up();
            let back = world.back.borrow().iter().any(|v| v["id"] == 9);
            return match back {
                true => trip_say(&world, 1),
                false => trip_settle(ed, &world),
            };
        }
        if over.get() || !world.pending.borrow().is_empty() {
            return trip_settle(ed, &world);
        }
        over.set(true);
        world.read_up();
        let prompt =
            world.to_agent.borrow().iter().find(|v| v["method"] == "session/prompt").map(|v| v["id"].clone());
        let reason = if world.stopped.get() { "cancelled" } else { "end_turn" };
        let line = json!({"jsonrpc": "2.0", "id": prompt, "result": {"stopReason": reason}});
        world.made(world.staged.borrow_mut().line(&line));
        format!("the agent's turn over: {reason} · {}", trip_settle(ed, &world))
    };
    sim.walk(&mut ed, TRIP_INPUTS + TRIP_ENDING, step, check, print);
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let idle = !c.busy();
    let said = c.msgs.iter().filter(|m| m.text == TRIPPED).count();
    let held = ed.agents_tripped.contains(&"stand-in");
    let tripped = world.tripped_at.get();
    let back = world.back.borrow().clone();
    sim.at_end(|| {
        let Some((_, back_at)) = tripped else { return Err("the wire never tripped".into()) };
        match back[..back_at].iter().any(|v| v["id"] == 9) {
            true => READ_BEFORE.fetch_add(1, Ordering::SeqCst),
            false => READ_CUT.fetch_add(1, Ordering::SeqCst),
        };
        if !idle {
            return Err("the trip left the chat busy".into());
        }
        if said != 1 {
            return Err(format!("the trip said {said} times"));
        }
        if !held {
            return Err("its row isn't held".into());
        }
        Ok(())
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    ed.workspace.claude.chat = None;
    drop(ed);
    trace
}

/// One round of the end: every event landed, every message the chat handed taken, a read card answered.
fn trip_settle(ed: &mut Editor, w: &Trip) -> String {
    let mut n = 0;
    loop {
        let ev = w.pending.borrow_mut().pop_front();
        let moved = match ev {
            Some(ev) => {
                land(ed, ev);
                true
            }
            None => match w.staged.borrow_mut().take() {
                Some((_, events)) => {
                    w.made(events);
                    true
                }
                None => answer_card(ed),
            },
        };
        if !moved {
            break;
        }
        n += 1;
    }
    format!("settled ({n} moves)")
}

/// The agent says its `i`th: its own tool running, then done; the read through the bridge.
fn trip_say(w: &Trip, i: usize) -> String {
    let mut said = w.said.get();
    said[i] = true;
    w.said.set(said);
    let call = |kind: &str, status: &str| {
        let u = json!({"sessionUpdate": kind, "toolCallId": "t1", "name": "run_command", "title": "echo ran",
            "kind": "execute", "status": status, "rawInput": {"CommandLine": "echo ran"}});
        json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s-1", "update": u}})
    };
    let mut staged = w.staged.borrow_mut();
    let (events, what) = match i {
        0 => (staged.line(&call("tool_call", "in_progress")), "its own tool running (run_command)"),
        1 => (staged.line(&call("tool_call_update", "completed")), "its own tool done"),
        _ => {
            let files = [w.outside.to_string_lossy()];
            let call = json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
                "params": {"name": "read", "arguments": {"files": files}}});
            (staged.bridged(call), "a read through the bridge")
        }
    };
    drop(staged);
    w.made(events);
    format!("the agent says {what}")
}

fn trip_input(sim: &mut Sim, ed: &mut Editor, w: &Trip) -> String {
    let c = &mut sim.choices;
    // (by arm: the agent says its next 3, an event lands 4, the wire takes what the chat handed 3, the stop key 1, the
    // read card answered 2, a second message 1, a frame 1)
    match c.weighted(&[3, 4, 3, 1, 2, 1, 1]) {
        0 => {
            // (its own tool done only once it ran, and least often: a read has its card, its job and its answer to go
            // through before it, in some seeds)
            let said = w.said.get();
            let left: Vec<usize> = (0..3).filter(|&i| !said[i] && (i != 1 || said[0])).collect();
            if left.is_empty() {
                return "(the agent has said all)".into();
            }
            let weights: Vec<usize> = left.iter().map(|&i| [3, 1, 4][i]).collect();
            let i = left[c.weighted(&weights)];
            trip_say(w, i)
        }
        1 => {
            let Some(ev) = w.pending.borrow_mut().pop_front() else { return "(no event to land)".into() };
            land(ed, ev);
            "an event lands".into()
        }
        2 => {
            let Some((what, events)) = w.staged.borrow_mut().take() else {
                return "(nothing handed to take)".into();
            };
            w.made(events);
            format!("the wire takes {what}")
        }
        3 if !w.stopped.get() && ed.workspace.claude.chat.as_ref().is_some_and(|c| c.busy()) => {
            w.stopped.set(true);
            crate::chat::stop(ed);
            "the stop key".into()
        }
        3 => "(stopped already, or nothing to stop)".into(),
        4 => match answer_card(ed) {
            true => "y on the read card".into(),
            false => "(no read card)".into(),
        },
        5 if !w.again.get() => {
            w.again.set(true);
            match crate::chat::submit(ed, "and then?", None, None) {
                true => "a second message, sent".into(),
                false => "a second message, not sent".into(),
            }
        }
        5 => "(a second message already)".into(),
        _ => {
            crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
            "a frame".into()
        }
    }
}

/// Nothing to the agent nor back through the bridge after the trip; the trip said at most once; as its end lands, no
/// read card up — one raised after it by a read already on a worker then (its answer goes nowhere: checked above) is
/// the read tool's, as when claude ends mid-read, and reported, not held here.
fn trip_held(ed: &Editor, w: &Trip) -> Result<(), String> {
    w.read_up();
    if let Some((agent, back)) = w.tripped_at.get() {
        let to_agent = w.to_agent.borrow();
        if let Some(v) = to_agent.get(agent..).and_then(|after| after.first()) {
            return Err(format!("written to the agent after the trip: {v}"));
        }
        if let Some(v) = w.back.borrow().get(back..).and_then(|after| after.first()) {
            return Err(format!("sent back through the bridge after the trip: {v}"));
        }
    }
    let c = ed.workspace.claude.chat.as_ref().ok_or("the chat's gone")?;
    let said = c.msgs.iter().filter(|m| m.text == TRIPPED).count();
    if said > 1 {
        return Err(format!("the trip said {said} times"));
    }
    let read_card = |o: &crate::offer::Offer| matches!(o.what, crate::offer::What::Read { .. });
    if ed.agents_tripped.contains(&"stand-in") && !w.end_seen.replace(true) {
        if ed.offers.iter().chain(&ed.workspace.parked).any(read_card) {
            return Err("a read card still up as the trip's end landed".into());
        }
        if c.running() || c.busy() {
            return Err("its process or its turn outlived the trip's end".into());
        }
    }
    Ok(())
}

fn trip_print(ed: &Editor, w: &Trip) -> String {
    let c = ed.workspace.claude.chat.as_ref();
    let busy = c.is_some_and(|c| c.busy());
    let running = c.is_some_and(crate::chat::Chat::running);
    format!(
        "busy {busy} · running {running} · card {} · to the agent {} · back {} · tripped {} · held {} · waiting {}",
        !ed.offers.is_empty(),
        w.to_agent.borrow().len(),
        w.back.borrow().len(),
        w.tripped_at.get().is_some(),
        !ed.agents_tripped.is_empty(),
        w.pending.borrow().len()
    )
}

/// What a trace line ends with: the chat busy, what was written to the agent and back, what waits to land.
fn print(ed: &Editor, w: &World) -> String {
    let busy = ed.workspace.claude.chat.as_ref().is_some_and(|c| c.busy());
    let card = !ed.offers.is_empty();
    let ask = w.to_agent.borrow().iter().rev().find(|v| v["id"] == 5 && v.get("method").is_none()).map(|v| {
        let o = &v["result"]["outcome"];
        o["optionId"].as_str().unwrap_or(o["outcome"].as_str().unwrap_or_default()).to_string()
    });
    format!(
        "busy {busy} · card {card} · to the agent {} · back {} · ask {} · waiting {}",
        w.to_agent.borrow().len(),
        w.back.borrow().len(),
        ask.as_deref().unwrap_or("open"),
        w.pending.borrow().len()
    )
}
