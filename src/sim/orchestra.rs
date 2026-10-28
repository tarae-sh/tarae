//! orchestra P1a, simulated (`.sb/orchestra-plan.md` §8): a parent — the checkout tarae started in — and two children
//! its Claude started, each chat's claude a stand-in of its own. At random the parent sends a child a message
//! (`session_send`, through the tool's own path), the user types one in a child (sent, or queued while it works), a
//! child's turn ends, the user switches sessions, time passes. After every step: the front session is the one the
//! user's last switch chose — nothing of the parent's moves it; every parent message reaches its child as exactly one
//! turn's input, in send order, never before a message of the user's queued there earlier nor one of its own held
//! there (a turn refused for money), and waits (once) until it does; a child's draft is never read or changed.

use std::cell::RefCell;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use super::Sim;
use crate::editor::Editor;
use crate::event::WorkspaceId;

/// (38: a send made at once while one sent behind was on its way — a later message landing first, found by 53 before
/// the money arm moved every walk; 5: a send at once past one of the parent's held by a turn refused for money)
const SEEDS: &[u64] = &[0, 1, 2, 5, 38];

const INPUTS: usize = 40;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn children_and_their_sends() {
    super::seeds("children_and_their_sends", SEEDS, run);
}

struct Child {
    id: WorkspaceId,
    name: &'static str,
    claude: Receiver<String>,
    /// The lines its claude got, in order.
    got: Vec<String>,
    draft: String,
}

/// What the checks keep.
struct World {
    home: WorkspaceId,
    /// The session the user's last switch chose.
    front: WorkspaceId,
    children: Vec<Child>,
    /// Each parent message: its child, its token, the user's tokens queued there as it was sent, and its call's id
    /// (unanswered: on its way — the child's own event, not yet handled).
    sent: Vec<(usize, String, Vec<String>, u64)>,
    n: usize,
}

fn run(choices: super::Choices) -> Vec<String> {
    // (the calls' ids start again each seed: the answers of the last one go)
    crate::tools::ANSWERED.with(|a| a.borrow_mut().clear());
    let dir = crate::scratch::Scratch::new("sim-orchestra");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut sim = Sim::new("children_and_their_sends", choices, LIMIT);
    let mut ed = Editor::with_root(crate::config::Config::default(), root.join("home"));
    let mut c = crate::chat::Chat::new();
    let parent = crate::chat::stand_in_own(&mut c);
    c.session_tools = true;
    ed.workspace.claude.chat = Some(c);
    let me = ed.workspace.claude.id;
    let mut children = Vec::new();
    for name in ["one", "two"] {
        std::fs::create_dir_all(root.join(name)).unwrap();
        let id = ed.workspace_add(root.join(name));
        if let Some(l) = ed.workspace_by_id(id) {
            (l.name, l.branch, l.claude.parent) = (name.into(), Some(name.into()), Some(me));
        }
        let draft = format!("{name}'s draft");
        let mut claude = None;
        ed.with_workspace(id, |ed| {
            let mut c = crate::chat::Chat::new();
            claude = Some(crate::chat::stand_in_own(&mut c));
            c.input = draft.as_str().into();
            ed.workspace.claude.chat = Some(c);
        });
        let claude = claude.expect("its stand-in");
        children.push(Child { id, name, claude, got: Vec::new(), draft });
    }
    let home = ed.workspace.id;
    let world = RefCell::new(World { home, front: home, children, sent: Vec::new(), n: 0 });
    sim.walk(
        &mut ed,
        INPUTS,
        |sim, ed| input(sim, ed, &world),
        |ed| held(ed, &mut world.borrow_mut()),
        print,
    );
    let trace = sim.trace().to_vec();
    drop((sim, ed, parent));
    trace
}

fn chat_of(ed: &Editor, id: WorkspaceId) -> Option<&crate::chat::Chat> {
    std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == id)?.claude.chat.as_ref()
}

/// The tokens (`[u3]`, `[p4]`) in `text`.
fn tokens(text: &str) -> Vec<String> {
    text.split('[').skip(1).filter_map(|t| t.split_once(']')).map(|t| format!("[{}]", t.0)).collect()
}

fn input(sim: &mut Sim, ed: &mut Editor, world: &RefCell<World>) -> String {
    let c = &mut sim.choices;
    let k = c.below(2);
    let mut w = world.borrow_mut();
    let (child, name, draft) = (w.children[k].id, w.children[k].name, w.children[k].draft.clone());
    // (by arm: the parent sends 4, the user types in a child 3, a child's turn ends 4, a switch 2, time 1, a child's turn
    // refused for money 1 — what waited stays, the child idle; the next end stands in for its billing's change)
    match c.weighted(&[4, 3, 4, 2, 1, 1]) {
        0 => {
            w.n += 1;
            let token = format!("[p{}]", w.n);
            let users = chat_of(ed, child).and_then(|c| c.queued.as_deref()).map(tokens).unwrap_or_default();
            let (home, n) = (w.home, w.n as u64);
            w.sent.push((k, token.clone(), users, n));
            drop(w);
            let msg = serde_json::json!({ "jsonrpc": "2.0", "id": n, "method": "tools/call",
                "params": { "name": "session_send", "arguments": { "session": name, "message": token } } });
            // (the parent's call, in its own session — swapped in when the user is elsewhere)
            ed.with_workspace(home, |ed| {
                let generation = ed.workspace.claude.chat.as_ref().map_or(0, |c| c.generation());
                assert!(
                    crate::tools::mcp(ed, generation, &format!("r{n}"), &msg).is_none(),
                    "it's answered later"
                );
            });
            format!("the parent sends {name} {token}")
        }
        1 => {
            w.n += 1;
            let token = format!("[u{}]", w.n);
            drop(w);
            ed.with_workspace(child, |ed| {
                if let Some(c) = ed.workspace.claude.chat.as_mut() {
                    c.input = token.as_str().into();
                }
                crate::chat::send(ed);
                // (the draft the user had, back over the input: what the checks hold the parent to)
                if let Some(c) = ed.workspace.claude.chat.as_mut() {
                    c.input = draft.as_str().into();
                }
            });
            format!("the user types {token} in {name}")
        }
        2 => {
            drop(w);
            ed.with_workspace(child, crate::chat::stand_in_done);
            format!("{name}'s turn ends")
        }
        3 => {
            let to = *c.pick(&[w.home, w.children[0].id, w.children[1].id]);
            w.front = to;
            drop(w);
            ed.workspace_switch(to);
            format!("the user switches to {to}")
        }
        4 => {
            drop(w);
            let ms = *c.pick(&[50, 500]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        5 => {
            drop(w);
            ed.with_workspace(child, crate::chat::stand_in_broke);
            format!("{name}'s turn ends refused for money")
        }
        _ => unreachable!("one arm a weight"),
    }
}

fn held(ed: &Editor, w: &mut World) -> Result<(), String> {
    if ed.workspace.id != w.front {
        return Err(format!("the front is {}, the user's last switch chose {}", ed.workspace.id, w.front));
    }
    for ch in &mut w.children {
        ch.got.extend(ch.claude.try_iter());
        let c = chat_of(ed, ch.id).ok_or("a child's chat is gone")?;
        if c.input.text() != ch.draft {
            return Err(format!(
                "{}'s draft is {:?}, not the user's {:?}",
                ch.name,
                c.input.text(),
                ch.draft
            ));
        }
    }
    let at = |ch: &Child, token: &str| -> Result<Option<usize>, String> {
        let lines: Vec<usize> =
            (0..ch.got.len()).filter(|&i| tokens(&ch.got[i]).iter().any(|t| t == token)).collect();
        match lines.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(*one)),
            _ => Err(format!("{token} reached {} in {} turns", ch.name, lines.len())),
        }
    };
    let mut last: Vec<Option<usize>> = vec![None; w.children.len()];
    for (k, token, users, n) in &w.sent {
        let ch = &w.children[*k];
        let answered =
            crate::tools::ANSWERED.with(|a| a.borrow().iter().any(|(id, _)| *id == serde_json::json!(n)));
        for (j, other) in w.children.iter().enumerate() {
            if j != *k && at(other, token)?.is_some() {
                return Err(format!("{token} for {} reached {}", ch.name, other.name));
            }
        }
        let waiting = chat_of(ed, ch.id).map_or(0, |c| {
            c.sends
                .iter()
                .filter(|l| matches!(l, crate::chat::Later::Parent { text, .. } if text == token))
                .count()
        });
        match (at(ch, token)?, waiting) {
            // (on its way, or waiting there: nothing the parent sent later may reach it first)
            (None, 0) if !answered => last[*k] = Some(usize::MAX),
            (None, 1) => last[*k] = Some(usize::MAX),
            (None, n) => {
                return Err(format!("{token} neither reached {} nor waits once there ({n})", ch.name));
            }
            (Some(_), n) if n > 0 => return Err(format!("{token} reached {} and still waits", ch.name)),
            (Some(i), _) => {
                if last[*k].is_some_and(|l| l > i) {
                    return Err(format!(
                        "{token} reached {} before a message the parent sent earlier",
                        ch.name
                    ));
                }
                last[*k] = Some(i);
                for u in users {
                    if !at(ch, u)?.is_some_and(|j| j < i) {
                        return Err(format!(
                            "{token} reached {} before the user's {u}, queued earlier",
                            ch.name
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let all = std::iter::once(&ed.workspace).chain(&ed.workspaces);
    let each = all.filter_map(|l| {
        let c = l.claude.chat.as_ref()?;
        Some(format!(
            "{}: {}{} {} waiting",
            l.label(),
            if c.busy() { "busy" } else { "idle" },
            if c.queued.is_some() { ", the user's queued" } else { "" },
            c.sends.len()
        ))
    });
    each.collect::<Vec<_>>().join(" · ")
}
