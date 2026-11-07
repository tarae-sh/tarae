//! A fork while another session streams, simulated (`fork.rs`): a repository on `main` and a session `plain` beside it,
//! each Claude a stand-in (`Editor.claude_lines`), and at random the checkout tarae started in forked (its card asked,
//! asked again, `y`, `n`), `plain`'s Claude streaming (a message, text, its turn's end), the source's own turn begun and
//! ended (D2: a `y` then waits for it), a switch of sessions, a frame, the user taking their time — the fork's measure and
//! its checkout's making jobs among them. After every step: at most one fork card a source; a card of a session's own
//! (one that parks with it) stays in that session's stack, never another's; a card of nobody's stays up; `plain`'s
//! stream is whole — every word streamed in its chat, its turn its own — and a fork is made only while its source isn't
//! answering.

use std::cell::RefCell;
use std::time::Duration;

use super::sessions::{World, print};
use super::{Choices, Sim};
use crate::editor::Editor;
use crate::event::WorkspaceId;
use crate::llm::StreamEv;
use crate::offer::{Offer, What};

const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 30;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_fork_while_another_session_streams() {
    super::seeds("a_fork_while_another_session_streams", SEEDS, run);
}

/// What the checks keep.
#[derive(Default)]
struct Seen {
    /// The cards of a session's own put up, by id, with the session they're of.
    own: Vec<(u64, WorkspaceId)>,
    /// The card of nobody's.
    nobodys: u64,
    /// The words `plain`'s Claude streamed in its turn under way, the turns it ended.
    words: Vec<String>,
    turns: usize,
    /// The fork cards seen being made (their `y` gone on to the worker).
    started: Vec<u64>,
}

fn run(choices: Choices) -> Vec<String> {
    let world = World::new();
    let mut sim = Sim::new("a_fork_while_another_session_streams", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (world.repo.clone(), "proj".into());
    ed.workspace.git_branch = Some("main".into());
    ed.config.workspace_worktrees = world.dir.join("wt").display().to_string();
    let (tx, _rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    let seen = RefCell::new(Seen::default());
    // (a card of nobody's up, and one of each session's own: they stay where they are whatever the fork does)
    let (_, plain) = &world.sessions[1];
    let plain = ed.workspace_open(plain).unwrap();
    for id in [WorkspaceId::HOME, plain] {
        ed.workspace_switch(id);
        crate::chat::open(&mut ed);
        ed.workspace.leave(crate::workspace::Here::Chat);
        let o = Offer::new(
            "Close notes".into(),
            "q",
            "…",
            "".into(),
            What::NotesClose { by: None, ids: Vec::new() },
        );
        ed.push_offer(o);
        seen.borrow_mut().own.push((ed.offer_seq, id));
    }
    ed.push_offer(Offer::new("Reset counts".into(), "q", "…", "".into(), What::FeaturesReset));
    seen.borrow_mut().nobodys = ed.offer_seq;
    ed.workspace_switch(WorkspaceId::HOME);
    let dir = world.dir.display().to_string();
    sim.walk(
        &mut ed,
        INPUTS,
        |sim, ed| input(sim, ed, plain, &seen),
        |ed| held(ed, plain, &mut seen.borrow_mut()),
        |ed| print(ed, &dir),
    );
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn fork_card(ed: &Editor) -> Option<usize> {
    ed.offers.iter().position(|o| matches!(o.what, What::SessionFork(_)))
}

/// Session `id`'s chat: its stream's generation, whether it's answering.
fn chat_of(ed: &Editor, id: WorkspaceId) -> Option<(u64, bool)> {
    let l = std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == id)?;
    l.claude.chat.as_ref().map(|c| (c.generation(), c.busy()))
}

/// A message sent in session `id`'s chat — a turn begun.
fn send(ed: &mut Editor, id: WorkspaceId, text: &str) {
    ed.with_workspace(id, |ed| {
        if let Some(c) = ed.workspace.claude.chat.as_mut() {
            c.input.set(text);
        }
        crate::chat::send(ed);
    });
}

/// An event of session `id`'s stream.
fn stream(ed: &mut Editor, id: WorkspaceId, ev: StreamEv) {
    let Some((generation, _)) = chat_of(ed, id) else { return };
    ed.with_workspace(id, |ed| crate::chat::stream_event(ed, generation, ev));
}

/// An input, then the loop's own pass as after any event (a key, a stream's line): every event's syncs — the fork card's
/// among them — run, as the main loop runs them after each.
fn input(sim: &mut Sim, ed: &mut Editor, plain: WorkspaceId, seen: &RefCell<Seen>) -> String {
    let said = act(sim, ed, plain, seen);
    ed.handle_event(crate::event::Event::Job(Box::new(|_| {})));
    said
}

fn act(sim: &mut Sim, ed: &mut Editor, plain: WorkspaceId, seen: &RefCell<Seen>) -> String {
    let c = &mut sim.choices;
    // (by arm: the fork asked 3, `y` on it 3, `plain` streams 5, the source's turn 2, `n` 1, a switch 2, a frame 1, time
    // 2)
    match c.weighted(&[3, 3, 5, 2, 1, 2, 1, 2]) {
        0 => {
            crate::fork::ask(ed, WorkspaceId::HOME, None);
            "the fork of main asked".into()
        }
        1 | 4 => {
            let Some(i) = fork_card(ed) else { return "(no fork card)".into() };
            let o = ed.offers.remove(i);
            ed.offers.insert(0, o);
            match c.weighted(&[3, 1]) {
                0 => {
                    ed.offer_accept();
                    "y on the fork card".into()
                }
                _ => {
                    ed.offer_dismiss();
                    "n on the fork card".into()
                }
            }
        }
        2 => match chat_of(ed, plain) {
            Some((_, false)) => {
                let n = seen.borrow().turns + 1;
                send(ed, plain, &format!("turn {n}"));
                seen.borrow_mut().words.clear();
                format!("plain: turn {n} asked")
            }
            Some((_, true)) => match c.weighted(&[3, 1]) {
                0 => {
                    let w = format!("w{} ", seen.borrow().words.len());
                    stream(ed, plain, StreamEv::Text(w.clone()));
                    seen.borrow_mut().words.push(w.trim().to_string());
                    format!("plain streams {w}")
                }
                _ => {
                    stream(ed, plain, StreamEv::Done(Ok("done".into())));
                    seen.borrow_mut().turns += 1;
                    "plain's turn ends".into()
                }
            },
            None => "(plain has no chat)".into(),
        },
        3 => match chat_of(ed, WorkspaceId::HOME) {
            Some((_, false)) => {
                send(ed, WorkspaceId::HOME, "a turn of main's");
                "main's turn begins".into()
            }
            Some((_, true)) => {
                stream(ed, WorkspaceId::HOME, StreamEv::Done(Ok("done".into())));
                "main's turn ends".into()
            }
            None => "(main has no chat)".into(),
        },
        5 => {
            let ids: Vec<WorkspaceId> = ed.sessions_in_order().iter().map(|l| l.id).collect();
            let to = *c.pick(&ids);
            ed.workspace_switch(to);
            format!("session {to} in front")
        }
        6 => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
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

fn held(ed: &Editor, plain: WorkspaceId, seen: &mut Seen) -> Result<(), String> {
    let all = || std::iter::once(&ed.workspace).chain(&ed.workspaces);
    let parked = || all().flat_map(|l| l.parked.iter().map(move |o| (l.id, o)));
    // At most one fork card a source
    let forks =
        ed.offers.iter().chain(parked().map(|p| p.1)).filter(|o| matches!(o.what, What::SessionFork(_)));
    if forks.count() > 1 {
        return Err("two fork cards for main".into());
    }
    // A session's own card in its stack only — up while it's in front, else parked in it
    for &(id, of) in &seen.own {
        let up = ed.offers.iter().any(|o| o.id == id);
        let at: Vec<WorkspaceId> = parked().filter(|(_, o)| o.id == id).map(|p| p.0).collect();
        match (up, &at[..]) {
            (true, []) if ed.workspace.id == of => {}
            (false, [w]) if *w == of && ed.workspace.id != of => {}
            _ => return Err(format!("card#{id}, session {of}'s, is up: {up}, parked in {at:?}")),
        }
    }
    if !ed.offers.iter().any(|o| o.id == seen.nobodys) {
        return Err(format!("card#{}, nobody's, isn't up", seen.nobodys));
    }
    // `plain`'s stream whole: every word streamed in its turn under way is in its chat
    let l = all().find(|l| l.id == plain).ok_or("plain is gone")?;
    let c = l.claude.chat.as_ref().ok_or("plain's chat is gone")?;
    let text: String = c.msgs.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join(" ");
    if let Some(w) = seen.words.iter().find(|w| !text.contains(w.as_str())) {
        return Err(format!("plain streamed {w}, its chat doesn't have it"));
    }
    // A fork is made only while its source isn't answering (D2): its `y` goes on to the worker between turns
    let home_busy = chat_of(ed, WorkspaceId::HOME).is_some_and(|c| c.1);
    for o in ed.offers.iter().filter(|o| matches!(o.what, What::SessionFork(_)) && o.installing()) {
        if !seen.started.contains(&o.id) {
            if home_busy {
                return Err(format!("card#{} forks while main is answering", o.id));
            }
            seen.started.push(o.id);
        }
    }
    Ok(())
}
