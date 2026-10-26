//! `space i` beside a chat turn in flight, simulated (audit 2.B, B′ — `ask.rs`): a chat whose claude (a stand-in)
//! is mid-turn, its stream going on, its turn ending and the next asked; and at random `space i` with an
//! instruction, the ask's claude (a stand-in of its own) calling `edit`, the review answered, its turn ending, frames
//! and time. After every step: an ask asked is under way at once — never waiting behind the chat's turn, nothing of it
//! queued in the chat; nothing of it reaches the chat (its lines, its messages, what it has seen); the review up while
//! the ask's card waits is the ask's. At the end every call of the ask's has its answer in the ask's own lines.

use std::cell::RefCell;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::key::{Code, Key};
use crate::llm::StreamEv;

const SEEDS: &[u64] = &[0, 1, 2, 5];

const INPUTS: usize = 50;

const LIMIT: Duration = crate::test_results::PATIENCE;

/// What the ask is asked — a word the chat never says, so any trace of it there is the ask's.
const ASKED: &str = "uppercase-it";

#[test]
fn space_i_runs_beside_a_chat_turn() {
    super::seeds("space_i_runs_beside_a_chat_turn", SEEDS, run);
}

#[derive(Default)]
struct World {
    /// What a step found wrong (the next check says it).
    wrong: Option<String>,
    /// The ask's calls made (MCP ids) and the lines its stand-in got.
    calls: Vec<u64>,
    ask_lines: Vec<String>,
    chat_lines: Vec<String>,
    /// The ask running has called `edit` already.
    called: bool,
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-ask");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::write(root.join("a.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    let mut sim = Sim::new("space_i_runs_beside_a_chat_turn", choices, LIMIT);
    let mut ed = Editor::with_root(crate::config::Config::default(), root.clone());
    ed.open(&root.join("a.rs")).unwrap();
    let mut c = crate::chat::Chat::new();
    let chat = crate::chat::stand_in(&mut c);
    ed.workspace.claude.chat = Some(c);
    let (tx, ask) = std::sync::mpsc::channel();
    ed.ask_lines = Some(tx);
    assert!(crate::chat::submit(&mut ed, "why is b empty?", None, None), "the chat's turn asked");
    ed.workspace.viewer.at = crate::workspace::At::Code;
    let world = RefCell::new(World::default());
    let check = |ed: &Editor| held(ed, &chat, &ask, &mut world.borrow_mut());
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &root, &world), check, print);
    sim.at_end(|| {
        let w = world.borrow();
        let answered = |id: &u64| w.ask_lines.iter().any(|l| l.contains(&format!("\"id\":{id}")));
        match w.calls.iter().find(|id| !answered(id)) {
            Some(id) => Err(format!("the ask's call {id} has no answer in its lines")),
            None => Ok(()),
        }
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn keys(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(ch) = chars.next() {
        let k: Key = match ch {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            ch => Key::plain(Code::Char(ch)),
        };
        ed.handle_key(k);
    }
}

fn chat_event(ed: &mut Editor, ev: StreamEv) {
    let g = ed.workspace.claude.chat.as_ref().map_or(0, crate::chat::Chat::generation);
    crate::chat::stream_event(ed, g, ev);
}

fn ask_generation(ed: &Editor) -> Option<u64> {
    ed.workspace.claude.ask.as_ref().map(|a| a.generation)
}

fn input(sim: &mut Sim, ed: &mut Editor, root: &std::path::Path, world: &RefCell<World>) -> String {
    let c = &mut sim.choices;
    // (by arm: space i 3, the chat's stream 3, the ask's edit 3, a review answered 2, the ask's end 2, the chat's
    // turn over and the next asked 1, a frame 1, time 1)
    match c.weighted(&[3, 3, 3, 2, 2, 1, 1, 1]) {
        0 => {
            if ed.workspace.claude.ask.is_some() || ed.workspace.claude.review.is_some() {
                return "(an ask or a review is up)".into();
            }
            let busy = ed.workspace.claude.chat.as_ref().is_some_and(crate::chat::Chat::busy);
            let sends = ed.workspace.claude.chat.as_ref().map_or(0, |c| c.sends.len());
            ed.workspace.viewer.at = crate::workspace::At::Code;
            keys(ed, &format!("gg<space>i{ASKED}<ret>"));
            let mut w = world.borrow_mut();
            w.called = false;
            if ed.workspace.claude.ask.is_none() {
                w.wrong = Some(format!("space i didn't start at once (the chat busy: {busy})"));
            }
            if ed.workspace.claude.chat.as_ref().map_or(0, |c| c.sends.len()) != sends {
                w.wrong = Some("space i queued something in the chat".into());
            }
            format!("space i (the chat busy: {busy})")
        }
        1 => {
            let words = c.pick(&["b is", " empty because", " nothing"]).to_string();
            chat_event(ed, StreamEv::Text(words.clone()));
            format!("the chat's stream: {words:?}")
        }
        2 => {
            let Some(g) = ask_generation(ed).filter(|_| !world.borrow().called) else {
                return "(no ask to call edit)".into();
            };
            let id = 1000 + world.borrow().calls.len() as u64;
            let n = id - 999;
            let path = root.join("a.rs");
            let edit = serde_json::json!({"edits": [{"file_path": path, "whole": true,
                "new_string": format!("fn a() {{}} // {n}\nfn b() {{}}\n")}]});
            let request = serde_json::json!({"subtype": "mcp_message", "server_name": "tarae", "message": {
                "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": "edit", "arguments": edit}}});
            crate::ask::on_event(ed, g, StreamEv::Control { request_id: format!("ask-{id}"), request });
            let mut w = world.borrow_mut();
            w.calls.push(id);
            w.called = true;
            format!("the ask's edit (call {id})")
        }
        3 => match ed.workspace.claude.review.as_ref().filter(|r| r.ready) {
            Some(_) => {
                ed.workspace.viewer.at = crate::workspace::At::Code;
                let k = *c.pick(&["y", "n", "a"]);
                keys(ed, k);
                // (the loop's every event: the landing's next file, or its answer — `handle_event`)
                crate::refactor::next(ed);
                format!("the review answered {k}")
            }
            None => "(no review up)".into(),
        },
        4 => {
            // (its turn ends only once its call is answered — a claude waits for its tool's answer)
            let waits = {
                let w = world.borrow();
                w.called
                    && w.calls
                        .last()
                        .is_some_and(|id| !w.ask_lines.iter().any(|l| l.contains(&format!("\"id\":{id}"))))
            };
            match ask_generation(ed).filter(|_| !waits) {
                Some(g) => {
                    crate::ask::on_event(ed, g, StreamEv::Done(Ok("done".into())));
                    "the ask's turn over".into()
                }
                None => "(no ask to end)".into(),
            }
        }
        5 => {
            crate::chat::stand_in_done(ed);
            let asked = crate::chat::submit(ed, "and a?", None, None);
            ed.workspace.viewer.at = crate::workspace::At::Code;
            format!("the chat's turn over, the next asked ({asked})")
        }
        6 => {
            crate::term::render(ed, &mut Vec::new(), 110, 30).unwrap();
            "a frame".into()
        }
        7 => {
            let ms = *c.pick(&[50, 300, 2000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => unreachable!("one arm a weight"),
    }
}

fn held(ed: &Editor, chat: &Receiver<String>, ask: &Receiver<String>, w: &mut World) -> Result<(), String> {
    w.chat_lines.extend(chat.try_iter());
    w.ask_lines.extend(ask.try_iter());
    if let Some(why) = w.wrong.take() {
        return Err(why);
    }
    if let Some(l) = w.chat_lines.iter().find(|l| l.contains(ASKED) || l.contains("\"ask-")) {
        return Err(format!("the chat got a line of the ask's: {l}"));
    }
    let Some(c) = ed.workspace.claude.chat.as_ref() else { return Err("the chat is gone".into()) };
    if c.msgs.iter().any(|m| m.text.contains(ASKED)) {
        return Err("the chat's conversation holds the ask's instruction".into());
    }
    if !c.edits.seen.is_empty() {
        return Err(format!("the chat saw what the ask wrote: {:?}", c.edits.seen.keys()));
    }
    // (while the ask's card waits, the one review is it)
    if let (Some(_), Some(r)) = (ed.workspace.claude.ask.as_ref(), ed.workspace.claude.review.as_ref())
        && !r.ready
        && r.instruction != ASKED
    {
        return Err(format!("a review waits that isn't the ask's: {}", r.instruction));
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let s = &ed.workspace.claude;
    let chat = s.chat.as_ref().map_or("none", |c| if c.busy() { "busy" } else { "idle" });
    let review = match s.review.as_ref() {
        None => "no review",
        Some(r) if !r.ready => "waiting card",
        Some(_) => "review up",
    };
    format!("chat {chat} · ask {} · {review}", if s.ask.is_some() { "running" } else { "none" })
}
