//! orchestra P1a–P1b, simulated (`.sb/orchestra-plan.md` §8): a parent — the checkout tarae started in — and two
//! children its Claude started, every chat's claude a stand-in. At random the parent sends a child a message
//! (`session_send`, through the tool's own path — now and then `fresh`, a new conversation there), reads `sessions`, or
//! gets a message of its own (its turn ending at another step); the user types in a child (sent, or queued while it
//! works); a child's turn ends (its reply through the stream's own `Done`), or its claude exits — mid-turn too — and
//! starts again at another step; a child's turn is refused for money; the user switches sessions; time passes. After
//! every step: the front session is the one the user's last switch chose — nothing of the parent's moves it; every
//! parent message reaches its child as exactly one turn's input, in send order, never before a message of the user's
//! queued there earlier nor one of its own held there, and waits (once) until it does — but a fresh one refused (the
//! child at work, something waiting there), which never does; a child's draft is never read or changed; every turn the
//! parent started that ended is news told it exactly once — in a session tool's answer or its next message — or kept
//! for it still, and a turn the user started never is; `sessions` reads each child's last report as the reply of the
//! last turn the parent started there that ended in its conversation. Now and then the parent's own chat starts anew
//! (`chat::reset`) — a child at work on what its last conversation sent, maybe: what that conversation started is never
//! told to the new one, nor read as its report (the review of P1b, 2026-10-28).

use std::collections::HashMap;

use std::cell::RefCell;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use super::Sim;
use crate::editor::Editor;
use crate::event::WorkspaceId;

/// (38: a send made at once while one sent behind was on its way — a later message landing first, found by 53 before
/// the money arm moved every walk, by 124 since P1b's arms did, by 199 since the parent's chat anew did; 5: a send at
/// once past one of the parent's held by a turn refused for money — 0 finds it now)
const SEEDS: &[u64] = &[0, 1, 2, 5, 38, 124, 199];

const INPUTS: usize = 60;

const LIMIT: Duration = crate::test_results::PATIENCE;

/// The header of the news block (`orchestra::take_news`): what follows it, to its frame's end, is news.
const NEWS: &str = "news of the sessions you started:";

#[test]
fn children_and_their_sends() {
    super::seeds("children_and_their_sends", SEEDS, run);
}

struct Child {
    id: WorkspaceId,
    name: &'static str,
    /// Its turns' inputs as its chat shows them, every conversation's in order — and this conversation's (a fresh send
    /// starts another: its rows go).
    got: Vec<String>,
    conv: Vec<String>,
    draft: String,
    /// What its last report is: None — no turn the parent started has ended in this conversation; Some(None) — one
    /// did, with no reply; Some(token) — its reply's.
    report: Option<Option<String>>,
    /// Its first claude's lines (kept, so its sends go): what it got is read off its chat — a claude started again is
    /// the shared stand-in.
    _claude: Receiver<String>,
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
    /// The parent's claude's lines.
    parent: Receiver<String>,
    /// The news blocks the parent was told — in its lines and its `sessions` answers —, each with the parent's
    /// conversation it was told to.
    told: Vec<(String, usize)>,
    /// The tokens of the turns the parent started that ended, each with the parent's conversation that started it (news
    /// told once to that one, never to another) · of those the user started.
    news: Vec<(String, usize)>,
    users: Vec<String>,
    /// The parent's conversation now (its chat started anew: the next) · each parent message's, by its token.
    conv: usize,
    conv_of: HashMap<String, usize>,
    /// `sessions` answers not checked yet, and the report each child had as it was asked.
    reads: Vec<(String, Vec<Option<Option<String>>>)>,
    /// Every chat's claude restarted is a stand-in whose lines come here — the parent's after its chat started anew
    /// too: read for its news blocks (a child's never holds one).
    shared: Receiver<String>,
}

fn run(choices: super::Choices) -> Vec<String> {
    // (the calls' ids start again each seed: the answers of the last one go)
    crate::tools::ANSWERED.with(|a| a.borrow_mut().clear());
    let dir = crate::scratch::Scratch::new("sim-orchestra");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut sim = Sim::new("children_and_their_sends", choices, LIMIT);
    let mut ed = Editor::with_root(crate::config::Config::default(), root.join("home"));
    // (a claude started again — a fresh send's new conversation, one that exited — is a stand-in too: never a process)
    let (tx, shared) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    let mut c = crate::chat::Chat::new();
    let parent = crate::chat::stand_in_own(&mut c);
    c.session_tools = true;
    c.conversation = Some(conversation(0));
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
        let _claude = claude.expect("its stand-in");
        children.push(Child { id, name, got: Vec::new(), conv: Vec::new(), draft, report: None, _claude });
    }
    let home = ed.workspace.id;
    let world = RefCell::new(World {
        home,
        front: home,
        children,
        sent: Vec::new(),
        n: 0,
        parent,
        told: Vec::new(),
        news: Vec::new(),
        users: Vec::new(),
        conv: 0,
        conv_of: HashMap::new(),
        reads: Vec::new(),
        shared,
    });
    sim.walk(
        &mut ed,
        INPUTS,
        |sim, ed| input(sim, ed, &world),
        |ed| held(ed, &mut world.borrow_mut()),
        print,
    );
    let trace = sim.trace().to_vec();
    drop((sim, ed, world));
    trace
}

fn chat_of(ed: &Editor, id: WorkspaceId) -> Option<&crate::chat::Chat> {
    std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == id)?.claude.chat.as_ref()
}

/// The tokens (`[u3]`, `[p4]`) in `text`.
fn tokens(text: &str) -> Vec<String> {
    text.split('[').skip(1).filter_map(|t| t.split_once(']')).map(|t| format!("[{}]", t.0)).collect()
}

/// The news blocks in `text`: from each header to its `<editor-note>`'s end, else the text's.
fn news_in(text: &str) -> Vec<String> {
    text.match_indices(NEWS)
        .map(|(i, _)| {
            let rest = &text[i..];
            rest[..rest.find("</editor-note>").unwrap_or(rest.len())].to_string()
        })
        .collect()
}

/// Whose the turn under way in `id`'s chat is — None: none under way; Some(true): the parent's.
fn turn_of(ed: &Editor, id: WorkspaceId) -> Option<bool> {
    let c = chat_of(ed, id).filter(|c| c.busy())?;
    let last = c.msgs.iter().rfind(|m| m.role == crate::chat::Role::User)?;
    Some(last.from.is_some())
}

/// The parent's conversation `n` as its claude would name it.
fn conversation(n: usize) -> String {
    format!("conversation-{n}")
}

/// The parent's conversation that sent the message of the turn under way in `id` — by its token, as the world kept it
/// when it was sent (a brief, sent at the start: the first).
fn conv_of_turn(ed: &Editor, id: WorkspaceId, w: &World) -> usize {
    let c = chat_of(ed, id);
    let last = c.and_then(|c| c.msgs.iter().rfind(|m| m.role == crate::chat::Role::User));
    let token = last.map(|m| tokens(&m.text)).unwrap_or_default();
    token.iter().find_map(|t| w.conv_of.get(t).copied()).unwrap_or(0)
}

fn input(sim: &mut Sim, ed: &mut Editor, world: &RefCell<World>) -> String {
    let c = &mut sim.choices;
    let k = c.below(2);
    let mut w = world.borrow_mut();
    let (child, name, draft) = (w.children[k].id, w.children[k].name, w.children[k].draft.clone());
    // (by arm: the parent sends 4, the user types in a child 3, a child's turn ends 4, a switch 2, time 1, a child's turn
    // refused for money 1 — what waited stays, the child idle; the next end stands in for its billing's change —, the
    // parent reads `sessions` 2, the parent's own message or its turn's end 2, a child's claude exits or starts 2, the
    // parent's chat anew 1)
    match c.weighted(&[4, 3, 4, 2, 1, 1, 2, 2, 2, 1]) {
        0 => {
            w.n += 1;
            let token = format!("[p{}]", w.n);
            let fresh = c.below(3) == 0;
            let users = chat_of(ed, child).and_then(|c| c.queued.as_deref()).map(tokens).unwrap_or_default();
            let (home, n) = (w.home, w.n as u64);
            w.sent.push((k, token.clone(), users, n));
            let conv = w.conv;
            w.conv_of.insert(token.clone(), conv);
            drop(w);
            let args = serde_json::json!({ "session": name, "message": token, "fresh": fresh });
            let msg = serde_json::json!({ "jsonrpc": "2.0", "id": n, "method": "tools/call",
                "params": { "name": "session_send", "arguments": args } });
            // (the parent's call, in its own session — swapped in when the user is elsewhere)
            ed.with_workspace(home, |ed| {
                let generation = ed.workspace.claude.chat.as_ref().map_or(0, |c| c.generation());
                assert!(
                    crate::tools::mcp(ed, generation, &format!("r{n}"), &msg).is_none(),
                    "it's answered later"
                );
            });
            format!("the parent sends {name} {token}{}", if fresh { " fresh" } else { "" })
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
            w.n += 1;
            let token = format!("[r{}]", w.n);
            match turn_of(ed, child) {
                Some(true) => {
                    let conv = conv_of_turn(ed, child, &w);
                    w.news.push((token.clone(), conv));
                    // (its report only where the conversation now started it)
                    if conv == w.conv {
                        w.children[k].report = Some(Some(token.clone()));
                    }
                }
                Some(false) => w.users.push(token.clone()),
                None => {}
            }
            drop(w);
            // (through the stream's own end: its reply in, the parent's news made where every turn's end passes)
            ed.with_workspace(child, |ed| {
                let g = ed.workspace.claude.chat.as_ref().map_or(0, |c| c.generation());
                crate::chat::stream_event(ed, g, crate::llm::StreamEv::Done(Ok(format!("{token} done"))));
            });
            format!("{name}'s turn ends ({token})")
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
            // (a stand-in's end: no reply, no news — the real one's money path is `Done`'s)
            if turn_of(ed, child) == Some(true) && conv_of_turn(ed, child, &w) == w.conv {
                w.children[k].report = Some(None);
            }
            drop(w);
            ed.with_workspace(child, crate::chat::stand_in_broke);
            format!("{name}'s turn ends refused for money")
        }
        6 => {
            w.n += 1;
            let n = w.n as u64;
            let one = c.below(3);
            let args = match one {
                2 => serde_json::json!({}),
                k => serde_json::json!({ "session": w.children[k].name }),
            };
            let had: Vec<Option<Option<String>>> = w.children.iter().map(|c| c.report.clone()).collect();
            let home = w.home;
            let msg = serde_json::json!({ "jsonrpc": "2.0", "id": n, "method": "tools/call",
                "params": { "name": "sessions", "arguments": args } });
            let mut answer = String::new();
            ed.with_workspace(home, |ed| {
                let generation = ed.workspace.claude.chat.as_ref().map_or(0, |c| c.generation());
                let r = crate::tools::mcp(ed, generation, &format!("r{n}"), &msg).expect("answered at once");
                answer = r["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string();
            });
            let conv = w.conv;
            w.told.extend(news_in(&answer).into_iter().map(|b| (b, conv)));
            w.reads.push((answer, had));
            format!("the parent reads sessions {}", if one == 2 { "(all)" } else { w.children[one].name })
        }
        7 => {
            w.n += 1;
            let token = format!("[m{}]", w.n);
            let home = w.home;
            drop(w);
            let mut said = String::new();
            ed.with_workspace(home, |ed| match ed.workspace.claude.chat.as_ref().is_some_and(|c| c.busy()) {
                true => {
                    crate::chat::stand_in_done(ed);
                    said = "the parent's turn ends".into();
                }
                false => {
                    crate::chat::submit(ed, &token, None, None);
                    said = format!("the parent gets a message {token}");
                }
            });
            said
        }
        8 => {
            let running = chat_of(ed, child).is_some_and(crate::chat::Chat::running);
            if !running {
                drop(w);
                ed.with_workspace(child, crate::chat::gate_opened);
                return format!("{name}'s claude starts again");
            }
            w.n += 1;
            let token = format!("[x{}]", w.n);
            match turn_of(ed, child) {
                Some(true) => {
                    let conv = conv_of_turn(ed, child, &w);
                    w.news.push((token.clone(), conv));
                    if conv == w.conv {
                        w.children[k].report = Some(None);
                    }
                }
                Some(false) => w.users.push(token.clone()),
                None => {}
            }
            drop(w);
            ed.with_workspace(child, |ed| {
                let g = ed.workspace.claude.chat.as_ref().map_or(0, |c| c.generation());
                crate::chat::stream_event(ed, g, crate::llm::StreamEv::Exit(token.clone()));
            });
            format!("{name}'s claude exits ({token})")
        }
        9 => {
            // (the parent's chat anew, `:chat-new`'s way — its turn cut off if one ran, a child maybe at work on what
            // the last conversation sent —, its new claude's conversation said as its `init` would)
            let (home, at_work) =
                (w.home, w.children.iter().filter(|ch| turn_of(ed, ch.id) == Some(true)).count());
            w.conv += 1;
            let conv = conversation(w.conv);
            for ch in &mut w.children {
                ch.report = None;
            }
            drop(w);
            ed.with_workspace(home, |ed| {
                crate::chat::reset(ed);
                if let Some(c) = ed.workspace.claude.chat.as_mut() {
                    c.conversation = Some(conv);
                }
            });
            format!("the parent's chat starts anew ({at_work} child turns of the last one at work)")
        }
        _ => unreachable!("one arm a weight"),
    }
}

fn held(ed: &Editor, w: &mut World) -> Result<(), String> {
    if ed.workspace.id != w.front {
        return Err(format!("the front is {}, the user's last switch chose {}", ed.workspace.id, w.front));
    }
    for ch in &mut w.children {
        let c = chat_of(ed, ch.id).ok_or("a child's chat is gone")?;
        if c.input.text() != ch.draft {
            return Err(format!(
                "{}'s draft is {:?}, not the user's {:?}",
                ch.name,
                c.input.text(),
                ch.draft
            ));
        }
        // (its turns' inputs as its chat has them: this conversation's grown, or — not an extension of what was seen —
        // a new one, its report none yet)
        let now: Vec<String> =
            c.msgs.iter().filter(|m| m.role == crate::chat::Role::User).map(|m| m.text.clone()).collect();
        let new = match now.starts_with(&ch.conv) {
            true => now[ch.conv.len()..].to_vec(),
            false => {
                ch.report = None;
                now.clone()
            }
        };
        ch.got.extend(new);
        ch.conv = now;
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
        let answer = crate::tools::ANSWERED
            .with(|a| a.borrow().iter().find(|(id, _)| *id == serde_json::json!(n)).cloned());
        for (j, other) in w.children.iter().enumerate() {
            if j != *k && at(other, token)?.is_some() {
                return Err(format!("{token} for {} reached {}", ch.name, other.name));
            }
        }
        // (a fresh one refused: never reaches, and holds nothing back)
        if let Some((_, Err(why))) = &answer {
            if !why.starts_with("not sent") {
                return Err(format!("{token} answered {why:?}"));
            }
            if at(ch, token)?.is_some() {
                return Err(format!("{token} refused ({why}) and reached {} all the same", ch.name));
            }
            continue;
        }
        let waiting = chat_of(ed, ch.id).map_or(0, |c| {
            c.sends
                .iter()
                .filter(|l| matches!(l, crate::chat::Later::Parent { text, .. } if text == token))
                .count()
        });
        match (at(ch, token)?, waiting) {
            // (on its way, or waiting there: nothing the parent sent later may reach it first)
            (None, 0) if answer.is_none() => last[*k] = Some(usize::MAX),
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
    // Every turn the parent started that ended: told once to the conversation that started it, or kept for it still —
    // never to another; the user's never
    let lines: Vec<String> = w.parent.try_iter().chain(w.shared.try_iter()).collect();
    let conv = w.conv;
    w.told.extend(lines.iter().flat_map(|l| news_in(l)).map(|b| (b, conv)));
    let parent =
        std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == w.home).ok_or("no parent")?;
    let now = parent.claude.chat.as_ref().and_then(|c| c.conversation.clone());
    let kept = |t: &str| {
        let of = |n: &&crate::orchestra::News| format!("{:?}", n.ended).contains(t);
        // (one kept for an earlier conversation is never told: as good as gone)
        parent.claude.news.iter().filter(of).filter(|n| n.conversation == now).count()
    };
    let told = |t: &str| w.told.iter().filter(|(b, _)| tokens(b).iter().any(|x| x == t)).count();
    for (t, c) in &w.news {
        let ok = match *c == w.conv {
            true => told(t) + kept(t) == 1,
            // (an earlier conversation's: told to it before it went, at most once — never kept for the new one)
            false => told(t) <= 1 && kept(t) == 0,
        };
        if !ok {
            return Err(format!(
                "{t}'s news (conversation {c}) told {} times, kept {} times",
                told(t),
                kept(t)
            ));
        }
    }
    for (b, at) in &w.told {
        for t in tokens(b) {
            match w.news.iter().find(|(n, _)| *n == t) {
                None => return Err(format!("news of {t}, no turn the parent started: {b:?}")),
                Some((_, c)) if c != at => {
                    return Err(format!("news of {t}, conversation {c}'s, told to conversation {at}: {b:?}"));
                }
                Some(_) => {}
            }
        }
    }
    if let Some(t) = w.users.iter().find(|t| kept(t) > 0) {
        return Err(format!("news kept of {t}, a turn the user started"));
    }
    // `sessions`: each child's last report as it was when asked
    for (answer, had) in std::mem::take(&mut w.reads) {
        let body = answer.split(NEWS).next().unwrap_or_default();
        for (ch, report) in w.children.iter().zip(had) {
            let Some(part) = body
                .split("\n\n")
                .find(|p| p.lines().next().is_some_and(|l| l.contains(&format!(" {} · ", ch.name))))
            else {
                continue;
            };
            let ok = match &report {
                None => part.contains("no report yet"),
                Some(None) => part.contains("ended with no reply"),
                Some(Some(t)) => {
                    part.contains(&format!("<session-report session=\"{}\"", ch.name))
                        && part.contains(t.as_str())
                }
            };
            if !ok {
                return Err(format!(
                    "sessions read {}'s report as {part:?}, its last is {report:?}",
                    ch.name
                ));
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
            "{}: {}{}{} {} waiting{}",
            l.label(),
            if c.busy() { "busy" } else { "idle" },
            if c.running() { "" } else { ", stopped" },
            if c.queued.is_some() { ", the user's queued" } else { "" },
            c.sends.len(),
            match l.claude.news.len() {
                0 => String::new(),
                n => format!(", {n} news"),
            }
        ))
    });
    each.collect::<Vec<_>>().join(" · ")
}
