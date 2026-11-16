//! A chat turn on an ACP agent, simulated (agents beside Claude, step 2's plan §5; the wire with no process,
//! `agent::acp::Staged`): the agent's own permission ask, its ask for tarae's `read` (offered: tarae's tools were
//! listed through the bridge, as agy 1.5.9 asks for every call of them — step 4's L-B1), then its call of `read`
//! through the bridge — of a file outside the project, which waits on the read card — and its words, in any order but
//! the call after its ask; each event the wire makes landing on the loop when the stream says, the wire taking what
//! the chat handed its handle when the stream says, the stop key once, the card answered, frames. The path's one
//! ordering rule is the stop while a permission ask is open: the chat's answer to the ask and the stop reach the wire
//! in either order, and the ask is answered once — `cancelled` when the stop came first (the spec's MUST), the
//! chat's answer otherwise.
//!
//! After every step: each ask is answered at most once — the agent's own never allowed (condition 1), the one for
//! tarae's `read` never refused (guard 2's ask rule: `allow_once`, or `cancelled` by a stop) — and, open when
//! `session/cancel` went, before it; a bridged request is answered at most once. At the end (the agent's turn over:
//! `cancelled` after a stop, else `end_turn`): the chat is idle, each ask answered once, and the read's answer — the
//! file's text, the card said yes to — went back through the bridge once, and came up as the call's result.
//!
//! A second scenario, `an_own_tool_trips_the_session` (step 3's guard 3): the agent runs one of its own tools to its
//! end (`in_progress`, then `completed`), calls tarae's `read` of the file outside (its card), and the stop key and a
//! second message of yours come, in any order. After every step: nothing is written to the agent after the trip, nor
//! sent back through the bridge; the trip is said once in the transcript; once its end lands, no read card is up. At
//! the end: the chat is idle, the trip said once, and its row held until you quit.
//!
//! A third, `an_untrusted_projects_agent_waits_for_its_card` (step 3's guard 4 and its watch, L5): an untrusted
//! project's held files — its own and its repository top's (L-B2) — hold the agent on its gate (`agentgate.rs`), each
//! start a staged handle in place of a process (`agentgate::Stage`). The gate's read, the user's first message, the chat
//! opened and the card's answer land in any order; in a run answering `y`, once its prompt went, a held file is added
//! in the top's and the watch looks (its round a job,
//! `agentgate::look_again`), and the agent's wire takes the stop. After every step: no start before its `y` (none at
//! all in a run answering `n`), a second only after the first's end landed, the message sent once, the stop said once.
//! At the end, a `y` run: started twice, the card answered twice (it asked again), the message sent once, to the first
//! start; an `n` run: nothing started, nothing written, the hold said.

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

/// How the fixed seeds' asks were answered: its own, the stop's `cancelled` first, the chat's refusal first; tarae's
/// `read`'s, allowed.
static CANCELLED: AtomicUsize = AtomicUsize::new(0);
static REFUSED: AtomicUsize = AtomicUsize::new(0);
static ALLOWED: AtomicUsize = AtomicUsize::new(0);

/// The JSON-RPC ids of the agent's own ask and of its ask for tarae's `read`.
const OWN_ASK: u64 = 5;
const TARAES_ASK: u64 = 6;

#[test]
fn a_turn_on_an_acp_agent_with_a_stop_and_an_ask() {
    super::seeds(NAME, SEEDS, run);
    let (cancelled, refused) = (CANCELLED.load(Ordering::SeqCst), REFUSED.load(Ordering::SeqCst));
    assert!(
        cancelled > 0 && refused > 0,
        "the ask answered one way only: {cancelled} cancelled, {refused} refused"
    );
    assert!(ALLOWED.load(Ordering::SeqCst) > 0, "tarae's read never allowed");
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
    /// The agent's ask, its call, its words and its ask for tarae's `read`: said yet.
    said: Cell<[bool; 4]>,
    stopped: Cell<bool>,
    /// The file outside the project its call reads.
    outside: std::path::PathBuf,
    /// The call's `ToolResult` landed.
    result: Cell<bool>,
    /// How many lines had gone to the agent when each ask was read (its own, tarae's `read`'s): one written after is
    /// open to a stop.
    asked_at: Cell<[Option<usize>; 2]>,
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
    // (tarae's tools listed through the bridge, as agy lists them at the session's start: what its asks may be allowed
    // for — the sandbox taken as looked at already, so the listing starts no look of its own)
    ed.sandbox = crate::sandbox::Availability::Ready;
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
    for ev in staged.bridged(list) {
        land(&mut ed, ev);
    }
    while staged.take().is_some() {}
    assert!(crate::chat::submit(&mut ed, "read the notes outside", None, None), "the turn asked");
    let world = World {
        staged: RefCell::new(staged),
        bridge,
        back: RefCell::new(Vec::new()),
        to_agent: RefCell::new(Vec::new()),
        pending: RefCell::new(VecDeque::new()),
        said: Cell::new([false; 4]),
        stopped: Cell::new(false),
        outside: outside.join("notes.txt"),
        result: Cell::new(false),
        asked_at: Cell::new([None; 2]),
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
        if let Some(&i) = sayable(&world).first() {
            return say(&world, i);
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
            to_agent.iter().filter(|v| v["id"] == OWN_ASK && v.get("method").is_none()).collect();
        if answers.len() != 1 {
            return Err(format!("the ask answered {} times", answers.len()));
        }
        let taraes: Vec<&Value> =
            to_agent.iter().filter(|v| v["id"] == TARAES_ASK && v.get("method").is_none()).collect();
        if taraes.len() != 1 {
            return Err(format!("the ask for tarae's read answered {} times", taraes.len()));
        }
        if taraes[0]["result"]["outcome"]["optionId"] == "allow" {
            ALLOWED.fetch_add(1, Ordering::SeqCst);
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
/// back through the bridge and its result landed, both asks answered.
fn all_in(ed: &Editor, w: &World) -> bool {
    w.read_up();
    let reading = ed.workspace.claude.chat.as_ref().is_some_and(|c| !c.reads.is_empty());
    let answered = |id: u64| w.to_agent.borrow().iter().any(|v| v["id"] == id && v.get("method").is_none());
    let asked = answered(OWN_ASK) && answered(TARAES_ASK);
    w.pending.borrow().is_empty() && !reading && ed.offers.is_empty() && w.result.get() && asked
}

/// What the agent may say next: what it hasn't, its call of `read` only after its ask for it (as agy 1.5.9 asks).
fn sayable(w: &World) -> Vec<usize> {
    let said = w.said.get();
    (0..4).filter(|&i| !said[i] && (i != 1 || said[3])).collect()
}

/// The agent says its `i`th: its own ask, the read through the bridge, its words, its ask for tarae's `read`.
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
            let ask = json!({"jsonrpc": "2.0", "id": OWN_ASK, "method": "session/request_permission",
                "params": params});
            (staged.line(&ask), "its own ask (write_file)")
        }
        3 => {
            let files = [w.outside.to_string_lossy()];
            let call = json!({"toolCallId": "m1", "name": "mcp_tarae_read", "title": "Run mcp_tarae_read",
                "kind": "other", "status": "pending", "rawInput": {"files": files}});
            let params = json!({"sessionId": "s-1", "toolCall": call,
                "options": crate::agent::acp::standin::agy_options()});
            let ask = json!({"jsonrpc": "2.0", "id": TARAES_ASK, "method": "session/request_permission",
                "params": params});
            (staged.line(&ask), "its ask for tarae's read")
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
    if let Some(k) = [0, 3].iter().position(|&a| a == i) {
        w.read_up();
        let mut at = w.asked_at.get();
        at[k] = Some(w.to_agent.borrow().len());
        w.asked_at.set(at);
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
            let left = sayable(w);
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

/// Each ask answered at most once — the agent's own never allowed, the one for tarae's `read` never refused — and,
/// open when `session/cancel` went, answered before it (the stop's `cancelled`); a bridged request answered at most
/// once.
fn held(_: &Editor, w: &World) -> Result<(), String> {
    w.read_up();
    let to_agent = w.to_agent.borrow();
    let asks = [(OWN_ASK, "the agent's own ask"), (TARAES_ASK, "the ask for tarae's read")];
    for (k, (id, which)) in asks.iter().enumerate() {
        let answers: Vec<(usize, &Value)> = to_agent
            .iter()
            .enumerate()
            .filter(|(_, v)| v["id"] == *id && v.get("method").is_none())
            .collect();
        if answers.len() > 1 {
            return Err(format!("{which} answered {} times", answers.len()));
        }
        if let Some((_, a)) = answers.first() {
            let chosen = &a["result"]["outcome"];
            let wrong = match *id == OWN_ASK {
                true => chosen["optionId"] == "allow",
                false => chosen["optionId"] != "allow" && chosen["outcome"] != "cancelled",
            };
            if wrong {
                return Err(format!("{which} answered {chosen}"));
            }
        }
        if let Some(cancel) = to_agent.iter().position(|v| v["method"] == "session/cancel")
            && w.asked_at.get()[k].is_some_and(|asked| asked <= cancel)
        {
            match answers.first() {
                Some((at, _)) if *at > cancel => {
                    return Err(format!("{which}, open at the cancel ({cancel}), answered after it ({at})"));
                }
                None => return Err(format!("{which}, open at the cancel ({cancel}), left unanswered")),
                Some(_) => {}
            }
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

// ── An untrusted project's agent waits for its card ──────────────────────────────────────────────────

const CARD: &str = "an_untrusted_projects_agent_waits_for_its_card";

const CARD_INPUTS: usize = 40;

/// The steps after them that see it through: the message, the card's answers, each start's handshake, the change and
/// the watch's look, every event landed (most of them frames while a job runs between).
const CARD_ENDING: usize = 80;

/// The watch's stop, in the chat.
const STOPPED: &str = "stand-in's files in this project changed while it ran — stopped: it and everything it started \
                       ended. Their card asks again.";

const HELD_BACK: &str = "Held back — stand-in isn't started in this project until you quit";

/// How the fixed seeds answered the card.
static YES_RUNS: AtomicUsize = AtomicUsize::new(0);
static NO_RUNS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn an_untrusted_projects_agent_waits_for_its_card() {
    super::seeds(CARD, SEEDS, card_run);
    let (yes, no) = (YES_RUNS.load(Ordering::SeqCst), NO_RUNS.load(Ordering::SeqCst));
    assert!(yes > 0 && no > 0, "the card answered one way only: {yes} yes, {no} no");
}

struct Card {
    /// Each start's handle, in order (`agentgate::Stage`), and what each was written.
    staged: [RefCell<Staged>; 2],
    to_agent: [RefCell<Vec<Value>>; 2],
    /// Its handshake's answers given: `initialize`, then its session.
    shook: [Cell<usize>; 2],
    /// Its turn answered.
    over: [Cell<bool>; 2],
    /// The events its wires made, each with its process's generation, not landed yet.
    pending: RefCell<VecDeque<(u64, Event)>>,
    /// This run answers the card `y` (else `n`).
    yes: bool,
    sent: Cell<bool>,
    answers: Cell<usize>,
    /// A held file added (once the first start was written its prompt), and the watch's look since.
    changed: Cell<bool>,
    looked: Cell<bool>,
    /// The first start's end has landed.
    ended: Cell<bool>,
    added: std::path::PathBuf,
}

impl Card {
    fn read_up(&self) {
        for (staged, to) in self.staged.iter().zip(&self.to_agent) {
            to.borrow_mut().extend(staged.borrow().written());
        }
    }

    /// Start `k`'s events, kept with its generation.
    fn made(&self, ed: &Editor, k: usize, events: Vec<Event>) {
        let g = generation(ed, k).unwrap_or(0);
        self.pending.borrow_mut().extend(events.into_iter().map(|e| (g, e)));
    }

    /// How many prompts carrying the message went, to start `k`.
    fn hello(&self, k: usize) -> usize {
        let to = self.to_agent[k].borrow();
        to.iter().filter(|v| v["method"] == "session/prompt" && v.to_string().contains("hello")).count()
    }
}

fn stage(ed: &Editor) -> Option<&crate::agentgate::Stage> {
    ed.workspace.claude.chat.as_ref()?.agent_gate.stage.as_ref()
}

fn starts(ed: &Editor) -> usize {
    stage(ed).map_or(0, |s| s.starts)
}

fn generation(ed: &Editor, k: usize) -> Option<u64> {
    stage(ed)?.generations.get(k).copied()
}

/// The start running now.
fn live(ed: &Editor) -> Option<usize> {
    let running = ed.workspace.claude.chat.as_ref().is_some_and(crate::chat::Chat::running);
    (running && starts(ed) > 0).then(|| starts(ed) - 1)
}

fn card_run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-acp-card");
    let root = std::fs::canonicalize(scratch.path()).unwrap();
    // (the project a folder of a repository whose top holds a file too — L-B2: the chain, one listing)
    let repo = root.join("repo");
    let project = repo.join("project");
    crate::agenttrust::repository_at(&repo);
    for folder in [&repo, &project] {
        std::fs::create_dir_all(folder.join(".stand-in")).unwrap();
        std::fs::write(folder.join(".stand-in/hooks.json"), "{}").unwrap();
    }
    let mut sim = Sim::new(CARD, choices, LIMIT);
    let yes = sim.choices.below(2) == 0;
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = project.clone();
    let state = root.join("state").to_string_lossy().into_owned();
    ed.claude_env = Some(crate::llm::Env::from([("XDG_STATE_HOME".into(), state)]));
    let ((first, a), (second, b)) = (Staged::new(&project), Staged::new(&project));
    let mut c = crate::chat::Chat::new();
    c.agent = &crate::agent::STAND_IN;
    c.hidden = false;
    let agents = [a, b].into_iter().collect();
    c.agent_gate.stage = Some(crate::agentgate::Stage { agents, ..Default::default() });
    ed.workspace.claude.chat = Some(c);
    let world = Card {
        staged: [RefCell::new(first), RefCell::new(second)],
        to_agent: Default::default(),
        shook: Default::default(),
        over: Default::default(),
        pending: RefCell::new(VecDeque::new()),
        yes,
        sent: Cell::new(false),
        answers: Cell::new(0),
        changed: Cell::new(false),
        looked: Cell::new(false),
        ended: Cell::new(false),
        added: repo.join(".stand-in/added.json"),
    };
    let check = |ed: &Editor| card_held(ed, &world);
    let print = |ed: &Editor| card_print(ed, &world);
    let n = Cell::new(0);
    let step = |sim: &mut Sim, ed: &mut Editor| {
        n.set(n.get() + 1);
        match n.get() <= CARD_INPUTS {
            true => card_input(sim, ed, &world),
            false => card_through(ed, &world),
        }
    };
    sim.walk(&mut ed, CARD_INPUTS + CARD_ENDING, step, check, print);
    world.read_up();
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let (running, busy) = (c.running(), c.busy());
    let stopped = c.msgs.iter().filter(|m| m.text == STOPPED).count();
    let (starts, answers, card) = (starts(&ed), world.answers.get(), ed.offers.iter().any(files_card));
    let hello = [world.hello(0), world.hello(1)];
    let prompts =
        world.to_agent.iter().flat_map(|t| t.borrow().clone()).filter(|v| v["method"] == "session/prompt");
    let prompts = prompts.count();
    let held_said = ed.messages.iter().any(|m| m.2 == HELD_BACK);
    sim.at_end(|| {
        if card {
            return Err("a card left up".into());
        }
        match yes {
            true => {
                YES_RUNS.fetch_add(1, Ordering::SeqCst);
                if (starts, answers) != (2, 2) {
                    return Err(format!("{starts} starts, {answers} answers: not asked again and started"));
                }
                if hello != [1, 0] {
                    return Err(format!("the message sent {hello:?} times, to each start"));
                }
                if stopped != 1 || !running || busy {
                    return Err(format!("stop said {stopped} times, running {running}, busy {busy}"));
                }
            }
            false => {
                NO_RUNS.fetch_add(1, Ordering::SeqCst);
                if (starts, answers, prompts) != (0, 1, 0) || !held_said {
                    return Err(format!(
                        "after n: {starts} starts, {answers} answers, {prompts} prompts, said {held_said}"
                    ));
                }
            }
        }
        Ok(())
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    ed.workspace.claude.chat = None;
    drop(ed);
    trace
}

fn files_card(o: &crate::offer::Offer) -> bool {
    matches!(o.what, crate::offer::What::AgentFiles { .. })
}

/// The files' card in front answered as this run answers it. Whether one was.
fn card_answer(ed: &mut Editor, w: &Card) -> bool {
    if !ed.offers.first().is_some_and(files_card) {
        return false;
    }
    crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
    let key = if w.yes { 'y' } else { 'n' };
    let answered = ed.offer_key(crate::key::Key::plain(crate::key::Code::Char(key)));
    w.answers.set(w.answers.get() + usize::from(answered));
    answered
}

/// The live start's next handshake answer: `initialize`, then its session. None: none left to give.
fn card_shake(ed: &Editor, w: &Card) -> Option<String> {
    let k = live(ed)?;
    let line = match w.shook[k].get() {
        0 => json!({"jsonrpc": "2.0", "id": 0, "result": {"protocolVersion": 1}}),
        1 => {
            json!({"jsonrpc": "2.0", "id": 1, "result": {"sessionId": "s-1", "modes": {"currentModeId": "default"}}})
        }
        _ => return None,
    };
    w.shook[k].set(w.shook[k].get() + 1);
    w.made(ed, k, w.staged[k].borrow_mut().line(&line));
    Some(format!("start {k} answers its handshake ({})", w.shook[k].get()))
}

/// What the chat handed any start's handle, taken. None: nothing waits.
fn card_take(ed: &Editor, w: &Card) -> Option<String> {
    (0..2).find_map(|k| {
        let (what, events) = w.staged[k].borrow_mut().take()?;
        w.made(ed, k, events);
        Some(format!("start {k}'s wire takes {what}"))
    })
}

/// An event landed, with its process's generation — the first start's end noted.
fn card_land(ed: &mut Editor, w: &Card) -> Option<String> {
    let (g, ev) = w.pending.borrow_mut().pop_front()?;
    if matches!(ev, Event::Exit(_)) && generation(ed, 0) == Some(g) {
        w.ended.set(true);
    }
    ed.handle_event(crate::event::Event::Job(Box::new(move |ed: &mut Editor| {
        crate::chat::stream_event(ed, g, ev)
    })));
    Some("an event lands".into())
}

fn card_send(ed: &mut Editor, w: &Card) -> String {
    w.sent.set(true);
    ed.workspace.claude.chat.as_mut().unwrap().input.set("hello");
    crate::chat::send(ed);
    "the user's message".into()
}

/// A held file added — once the first start was written its prompt.
fn card_change(w: &Card) -> Option<String> {
    w.read_up();
    if w.changed.get() || w.hello(0) == 0 {
        return None;
    }
    w.changed.set(true);
    super::write(&w.added, "{\"pulled\": true}");
    Some("a held file added (a pull)".into())
}

/// The watch looks (a job), whatever has changed.
fn card_look(ed: &mut Editor, w: &Card) -> Option<String> {
    if !crate::agentgate::look_again(ed) {
        return None;
    }
    w.looked.set(w.looked.get() || w.changed.get());
    Some("the watch looks".into())
}

/// The live start's turn answered, once its prompt went.
fn card_turn_over(ed: &Editor, w: &Card) -> Option<String> {
    let k = live(ed)?;
    w.read_up();
    let prompt =
        w.to_agent[k].borrow().iter().find(|v| v["method"] == "session/prompt").map(|v| v["id"].clone())?;
    if w.over[k].replace(true) {
        return None;
    }
    let line = json!({"jsonrpc": "2.0", "id": prompt, "result": {"stopReason": "end_turn"}});
    w.made(ed, k, w.staged[k].borrow_mut().line(&line));
    Some(format!("start {k}'s turn over"))
}

fn card_input(sim: &mut Sim, ed: &mut Editor, w: &Card) -> String {
    let skip = |what: &str| format!("({what})");
    // (by arm: the message 1, the chat opened 1, the card answered 2, a handshake 2, a take 3, a landing 4, the turn's
    // answer 1, the change 1, the watch's look 1, a frame 1)
    match sim.choices.weighted(&[1, 1, 2, 2, 3, 4, 1, 1, 1, 1]) {
        0 if !w.sent.get() => card_send(ed, w),
        0 => skip("sent already"),
        1 => {
            crate::chat::gate_opened(ed);
            "the chat opened".into()
        }
        2 => match card_answer(ed, w) {
            true => format!("{} on the card", if w.yes { 'y' } else { 'n' }),
            false => skip("no card"),
        },
        3 => card_shake(ed, w).unwrap_or_else(|| skip("no handshake to answer")),
        4 => card_take(ed, w).unwrap_or_else(|| skip("nothing handed")),
        5 => card_land(ed, w).unwrap_or_else(|| skip("no event")),
        6 => card_turn_over(ed, w).unwrap_or_else(|| skip("no turn to answer")),
        7 => card_change(w).unwrap_or_else(|| skip("no change now")),
        8 => card_look(ed, w).unwrap_or_else(|| skip("no watch")),
        _ => {
            crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
            "a frame".into()
        }
    }
}

/// The end's next step: the message, the card, the live start's handshake, what's handed and what's made, then (a `y`
/// run) the change and the watch's look — else a frame while a job runs.
fn card_through(ed: &mut Editor, w: &Card) -> String {
    if !w.sent.get() {
        return card_send(ed, w);
    }
    if card_answer(ed, w) {
        return "the card answered".into();
    }
    let next = card_shake(ed, w)
        .or_else(|| card_take(ed, w))
        .or_else(|| card_land(ed, w))
        .or_else(|| card_change(w))
        .or_else(|| {
            (w.changed.get() && !w.looked.get() && live(ed) == Some(0)).then(|| card_look(ed, w)).flatten()
        });
    next.unwrap_or_else(|| {
        crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
        "a frame".into()
    })
}

/// No start before its `y` (none after `n`); a second only after the first's end landed; the message sent once; the
/// stop said once.
fn card_held(ed: &Editor, w: &Card) -> Result<(), String> {
    w.read_up();
    let (starts, ys) = (starts(ed), if w.yes { w.answers.get() } else { 0 });
    if starts > ys {
        return Err(format!("{starts} starts after {ys} yes"));
    }
    if starts > 1 && !w.ended.get() {
        return Err("a second start before the first one's end landed".into());
    }
    let hello = w.hello(0) + w.hello(1);
    if hello > 1 {
        return Err(format!("the message sent {hello} times"));
    }
    let c = ed.workspace.claude.chat.as_ref().ok_or("the chat's gone")?;
    let stopped = c.msgs.iter().filter(|m| m.text == STOPPED).count();
    if stopped > 1 {
        return Err(format!("the stop said {stopped} times"));
    }
    Ok(())
}

fn card_print(ed: &Editor, w: &Card) -> String {
    let c = ed.workspace.claude.chat.as_ref();
    format!(
        "starts {} · running {} · busy {} · card {} · answers {} · hello {}+{} · changed {} · waiting {}",
        starts(ed),
        c.is_some_and(crate::chat::Chat::running),
        c.is_some_and(|c| c.busy()),
        ed.offers.iter().any(files_card),
        w.answers.get(),
        w.hello(0),
        w.hello(1),
        w.changed.get(),
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
