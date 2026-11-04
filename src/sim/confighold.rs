//! A config that doesn't read, simulated (friction #339): your config file broken and fixed at random, `:config-reload`
//! (its read a job), the chat opened, a message sent, a frame, time passing — its claude a stand-in whose lines are kept
//! (`Editor.claude_lines`). After every step: no claude started while the file the editor last read didn't read; once it
//! reads, the chat a hold kept is running; every message sent reached the stand-in or waits (`Chat.queued`) — never
//! lost; a chat held says why on the frame.

use std::cell::RefCell;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;

const SEEDS: &[u64] = &[0, 1, 2];

const INPUTS: usize = 28;

const LIMIT: Duration = crate::test_results::PATIENCE;

const BROKEN: &str = "[llm]\ncommand = \"sh\n";
const READS: &str = "[llm]\ncommand = \"sh\"\n";

#[test]
fn a_config_that_doesnt_read_holds_claude() {
    super::seeds("a_config_that_doesnt_read_holds_claude", SEEDS, run);
}

/// What the checks keep.
#[derive(Default)]
struct Seen {
    sent: Vec<String>,
    lines: Vec<String>,
    /// Running as of the last check.
    running: bool,
    /// A frame of a held chat that didn't say so.
    unsaid: Option<String>,
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-config-hold");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let file = root.join("config.toml");
    std::fs::write(&file, BROKEN).unwrap();
    let mut sim = Sim::new("a_config_that_doesnt_read_holds_claude", choices, LIMIT);
    let from = crate::config::Sources { user: Some(file.clone()), trust: None };
    let mut ed = Editor::with_root(crate::config::load(&root, &from).0, root.clone());
    ed.config_file = Some(file.clone());
    let (tx, rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    let seen = RefCell::new(Seen::default());
    sim.walk(
        &mut ed,
        INPUTS,
        |sim, ed| input(sim, ed, &file, &seen),
        |ed| held(ed, &rx, &mut seen.borrow_mut()),
        print,
    );
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn input(sim: &mut Sim, ed: &mut Editor, file: &Path, seen: &RefCell<Seen>) -> String {
    let c = &mut sim.choices;
    // (by arm: the file broken 1, fixed 2, `:config-reload` 3, the chat opened 2, a message 3, a frame 1, time 1)
    match c.weighted(&[1, 2, 3, 2, 3, 1, 1]) {
        0 => write(file, BROKEN, "the file broken"),
        1 => write(file, READS, "the file fixed"),
        2 => {
            crate::typed::execute(ed, "config-reload").unwrap();
            ":config-reload".into()
        }
        3 => match ed.workspace.claude.chat.is_some() {
            true => "(the chat is open)".into(),
            false => {
                crate::chat::open(ed);
                ed.workspace.leave(crate::workspace::At::Chat);
                "the chat opened".into()
            }
        },
        4 => {
            let Some(chat) = ed.workspace.claude.chat.as_mut() else { return "(no chat to send in)".into() };
            let n = seen.borrow().sent.len() + 1;
            let text = format!("question {n}");
            chat.input.set(&text);
            crate::chat::send(ed);
            seen.borrow_mut().sent.push(text.clone());
            format!("sent: {text}")
        }
        5 => {
            let mut bytes = Vec::new();
            crate::term::render(ed, &mut bytes, 120, 34).unwrap();
            let mut vt = vt100::Parser::new(34, 120, 0);
            vt.process(&bytes);
            let held = ed.workspace.claude.chat.as_ref().is_some_and(|c| c.held && !c.running());
            if held && !vt.screen().contents().contains("no claude until your config reads") {
                seen.borrow_mut().unsaid = Some(vt.screen().contents());
            }
            "a frame".into()
        }
        6 => {
            let ms = *c.pick(&[50, 300, 2000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => unreachable!("one arm a weight"),
    }
}

fn write(file: &Path, text: &str, said: &str) -> String {
    std::fs::write(file, text).unwrap();
    said.into()
}

/// No claude started on a config that doesn't read; a held chat starts once it reads; no message lost; a held chat
/// says why.
fn held(ed: &Editor, rx: &Receiver<String>, seen: &mut Seen) -> Result<(), String> {
    seen.lines.extend(rx.try_iter());
    if let Some(frame) = seen.unsaid.take() {
        return Err(format!("held, the chat didn't say why:\n{frame}"));
    }
    let unread = !ed.config_unread.is_empty();
    let Some(c) = ed.workspace.claude.chat.as_ref() else { return Ok(()) };
    let started = c.running() && !std::mem::replace(&mut seen.running, c.running());
    if started && unread {
        return Err("a claude started while the config didn't read".into());
    }
    if !unread && (c.held || !c.running()) {
        return Err("the config reads, and the chat's claude is still held".into());
    }
    for s in &seen.sent {
        let waits = c.queued.as_deref().is_some_and(|q| q.contains(s.as_str()));
        if !waits && !seen.lines.iter().any(|l| l.contains(s.as_str())) {
            return Err(format!("\"{s}\" neither sent nor waiting"));
        }
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let config = if ed.config_unread.is_empty() { "reads" } else { "doesn't read" };
    match ed.workspace.claude.chat.as_ref() {
        None => format!("config {config}, no chat"),
        Some(c) => format!(
            "config {config}, chat {}{}, queued {:?}",
            if c.running() { "running" } else { "not running" },
            if c.held { ", held" } else { "" },
            c.queued
        ),
    }
}
