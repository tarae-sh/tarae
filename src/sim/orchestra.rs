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
//! told to the new one, nor read as its report (the review of P1b, 2026-10-28). P2: a send asks to `wake` the parent
//! half the time; a card of Claude's comes in the parent or a child, the user answers one by its key; an hour passes;
//! the user ends a child. And: a card goes only by the user's key, none of a child's words reach the parent's claude; a
//! wake's news names a turn that asked, never comes while the parent's own card waits, never past `llm.parent-wakes`
//! in an hour.

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
    /// Ended by the user (its checkout closed): no step reaches it any more.
    ended: bool,
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
    /// The cards put up (in the parent or a child): where, its id, its token (in its title and text), whether the
    /// user answered it — by a key — and whether a turn of that session ended since (its Claude's own end: what it
    /// asked goes with it). Nothing else takes one away — no session tool, no step of the parent's.
    cards: Vec<(WorkspaceId, u64, String, bool, bool)>,
    /// The reply tokens of the turns whose message asked to wake the parent (`wake`) — a wake's news names one.
    wakers: Vec<String>,
    /// When each wake was seen (the simulation's clock, ms): never more than `CAP` in an hour.
    woke_at: Vec<u64>,
    /// P3: the parent's cards its Claude proposed (`Offer.proposed`), by card id: the token its news names (in the
    /// proposal's child name) and the conversation that proposed it — answered by your key, news once.
    proposed: HashMap<u64, (String, usize)>,
}

/// `llm.parent-wakes` here: small, so the walk meets it.
const CAP: usize = 2;

/// What a wake's message says (`orchestra::WOKE`), as its line holds it.
const WOKE: &str = "a turn you asked to be woken for ended";

fn run(choices: super::Choices) -> Vec<String> {
    // (the calls' ids start again each seed: the answers of the last one go)
    crate::tools::ANSWERED.with(|a| a.borrow_mut().clear());
    let dir = crate::scratch::Scratch::new("sim-orchestra");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut sim = Sim::new("children_and_their_sends", choices, LIMIT);
    let mut ed = Editor::with_root(crate::config::Config::default(), root.join("home"));
    ed.config.llm.parent_wakes = CAP;
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
        let ended = false;
        children.push(Child {
            id,
            name,
            got: Vec::new(),
            conv: Vec::new(),
            draft,
            report: None,
            _claude,
            ended,
        });
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
        cards: Vec::new(),
        wakers: Vec::new(),
        woke_at: Vec::new(),
        proposed: HashMap::new(),
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
    let mut k = c.below(2);
    let mut w = world.borrow_mut();
    // (a turn's end or a claude's exit in a session lets the cards its Claude asked there go: marked before the arm —
    // the arms that end one say so by `ends`)
    let ends =
        |w: &mut World, at: WorkspaceId| w.cards.iter_mut().filter(|x| x.0 == at).for_each(|x| x.4 = true);
    // (a child ended: the other; both ended, time passes)
    if w.children[k].ended {
        k = 1 - k;
    }
    if w.children[k].ended {
        drop(w);
        sim.wait(Duration::from_millis(50));
        return "both children ended: 50 ms pass".into();
    }
    let (child, name, draft) = (w.children[k].id, w.children[k].name, w.children[k].draft.clone());
    // (by arm: the parent sends 4, the user types in a child 3, a child's turn ends 4, a switch 2, time 1, a child's turn
    // refused for money 1 — what waited stays, the child idle; the next end stands in for its billing's change —, the
    // parent reads `sessions` 2, the parent's own message or its turn's end 2, a child's claude exits or starts 2, the
    // parent's chat anew 1)
    // (P2: a card up in the parent or a child 2, the user answers one 2, an hour passes 1, the user ends a child 1)
    // (P3: a review the parent started in a child ends 1 — and half the parent's cards its Claude's proposals)
    match c.weighted(&[4, 3, 4, 2, 1, 1, 2, 2, 2, 1, 2, 2, 1, 1, 1]) {
        0 => {
            w.n += 1;
            let token = format!("[p{}]", w.n);
            let fresh = c.below(3) == 0;
            let wake = c.below(2) == 0;
            let users = chat_of(ed, child).and_then(|c| c.queued.as_deref()).map(tokens).unwrap_or_default();
            let (home, n) = (w.home, w.n as u64);
            w.sent.push((k, token.clone(), users, n));
            let conv = w.conv;
            w.conv_of.insert(token.clone(), conv);
            drop(w);
            let args = serde_json::json!({ "session": name, "message": token, "fresh": fresh, "wake": wake });
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
            let how =
                [(fresh, " fresh"), (wake, " wake")].iter().filter(|h| h.0).map(|h| h.1).collect::<String>();
            format!("the parent sends {name} {token}{how}")
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
                    // (its message asked to wake the parent: a wake may name it)
                    let last = chat_of(ed, child)
                        .and_then(|c| c.msgs.iter().rfind(|m| m.role == crate::chat::Role::User));
                    if last.is_some_and(|m| m.wake) {
                        w.wakers.push(token.clone());
                    }
                    // (its report only where the conversation now started it)
                    if conv == w.conv {
                        w.children[k].report = Some(Some(token.clone()));
                    }
                }
                Some(false) => w.users.push(token.clone()),
                None => {}
            }
            ends(&mut w, child);
            drop(w);
            // (through the stream's own end: its reply in, the parent's news made where every turn's end passes)
            ed.with_workspace(child, |ed| {
                let g = ed.workspace.claude.chat.as_ref().map_or(0, |c| c.generation());
                crate::chat::stream_event(ed, g, crate::llm::StreamEv::Done(Ok(format!("{token} done"))));
            });
            format!("{name}'s turn ends ({token})")
        }
        3 => {
            let living: Vec<WorkspaceId> = std::iter::once(w.home)
                .chain(w.children.iter().filter(|ch| !ch.ended).map(|ch| ch.id))
                .collect();
            let to = *c.pick(&living);
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
            ends(&mut w, child);
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
            if chat_of(ed, home).is_some_and(crate::chat::Chat::busy) {
                ends(&mut w, home);
            }
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
            ends(&mut w, child);
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
                    // (a turn that asked to wake the parent, cut off: a wake may name it as failed)
                    let last = chat_of(ed, child)
                        .and_then(|c| c.msgs.iter().rfind(|m| m.role == crate::chat::Role::User));
                    if last.is_some_and(|m| m.wake) {
                        w.wakers.push(token.clone());
                    }
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
            ends(&mut w, home);
            drop(w);
            ed.with_workspace(home, |ed| {
                crate::chat::reset(ed);
                if let Some(c) = ed.workspace.claude.chat.as_mut() {
                    c.conversation = Some(conv);
                }
            });
            format!("the parent's chat starts anew ({at_work} child turns of the last one at work)")
        }
        10 => {
            // (a card of Claude's comes, in the parent or a child — its token in its title and its text: never in what
            // the parent is told)
            w.n += 1;
            let token = format!("[card{}]", w.n);
            let at = if c.below(2) == 0 { w.home } else { child };
            let what = crate::offer::What::Asks { call: crate::tools::Call::test(1_000_000 + w.n as u64) };
            let o = crate::offer::Offer::new(
                format!("Claude asks {token}"),
                &format!("to see {token}"),
                "",
                String::new(),
                what,
            );
            // (P3: one in the parent its Claude proposed — its answer news for the conversation that proposed it, the
            // news's token in the proposal's child name: the key alone is told, never the card's words)
            // (carried as the act's own command carries it to the card it raises: `orchestra::carry`)
            let pc = (at == w.home && c.below(2) == 0).then(|| format!("[pc{}]", w.n));
            let proposal = pc.as_ref().map(|pc| {
                let parent = std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == w.home);
                crate::orchestra::Proposal {
                    act: crate::orchestra::Act::Bring,
                    parent: parent.map(|l| l.claude.id).expect("the parent"),
                    workspace: w.home,
                    by: "home".into(),
                    child: format!("{name}{pc}"),
                    why: String::new(),
                    conversation: Some(conversation(w.conv)),
                }
            });
            let o = crate::orchestra::carry(o, proposal);
            let conv = w.conv;
            drop(w);
            ed.with_workspace(at, |ed| ed.push_offer(o));
            let mut w = world.borrow_mut();
            w.cards.push((at, ed.offer_seq, token.clone(), false, false));
            if let Some(pc) = pc.clone() {
                w.proposed.insert(ed.offer_seq, (pc, conv));
            }
            format!(
                "a card {token} comes in {at}{}",
                pc.map(|p| format!(", proposed {p}")).unwrap_or_default()
            )
        }
        11 => {
            // (the user answers one: its session in front, the card focused, `n` — a key, the only way one goes)
            let gone = |at: WorkspaceId| w.children.iter().any(|ch| ch.ended && ch.id == at);
            let open: Vec<usize> = (0..w.cards.len())
                .filter(|&i| !w.cards[i].3 && !w.cards[i].4 && !gone(w.cards[i].0))
                .collect();
            let Some(&i) = open.get(c.below(open.len().max(1))) else {
                drop(w);
                sim.wait(Duration::from_millis(50));
                return "no card to answer: 50 ms pass".into();
            };
            let (at, id) = (w.cards[i].0, w.cards[i].1);
            w.front = at;
            drop(w);
            ed.workspace_switch(at);
            // (its cards up in arrival order: the ones before it answered first, each by its key)
            let mut said = Vec::new();
            while let Some(front) = ed.offers.first().map(|o| o.id) {
                ed.card_focus_front();
                ed.offer_key(crate::key::Key::plain(crate::key::Code::Char('n')));
                let mut w = world.borrow_mut();
                if let Some(card) = w.cards.iter_mut().find(|x| x.1 == front) {
                    card.3 = true;
                    said.push(card.2.clone());
                }
                // (a proposal answered by your key: news for the conversation that proposed it)
                if let Some(news) = w.proposed.remove(&front) {
                    w.news.push(news);
                }
                if front == id || ed.offers.first().map(|o| o.id) == Some(front) {
                    break;
                }
            }
            format!("the user answers {} in {at}", said.join(", "))
        }
        12 => {
            drop(w);
            sim.wait(Duration::from_secs(3_600));
            "an hour passes".into()
        }
        13 => {
            // (the user ends a child behind with nothing of the parent's on its way there — its checkout closed)
            let on_way = w.sent.iter().any(|(j, _, _, n)| {
                *j == k
                    && crate::tools::ANSWERED
                        .with(|a| a.borrow().iter().all(|(id, _)| *id != serde_json::json!(n)))
            });
            if child == w.front || on_way {
                drop(w);
                sim.wait(Duration::from_millis(50));
                return format!("{name} isn't ended (in front, or a message on its way): 50 ms pass");
            }
            w.children[k].ended = true;
            drop(w);
            let root = std::iter::once(&ed.workspace)
                .chain(&ed.workspaces)
                .find(|l| l.id == child)
                .map(|l| l.root.clone());
            if let Some(root) = root {
                ed.workspace_close(&root);
            }
            format!("the user ends {name}")
        }
        14 => {
            // (P3: a review the parent started in a child stops — its why its token —, told as it stops and again as its
            // end passes: news once, for the conversation that asked)
            w.n += 1;
            let token = format!("[rv{}]", w.n);
            let conv = w.conv;
            w.news.push((token.clone(), conv));
            let home = w.home;
            drop(w);
            let parent = std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == home);
            let asked = crate::orchestra::ForParent {
                parent: parent.map(|l| l.claude.id).expect("the parent"),
                conversation: Some(conversation(conv)),
                wake: false,
                told: false,
            };
            ed.with_workspace(child, |ed| {
                let state = crate::reviewer::State::Failed(token.clone());
                let mut r =
                    crate::reviewer::Reviewer::for_test(ed.workspace.root.clone(), state, Vec::new(), "");
                r.parent = Some(asked);
                ed.workspace.reviewer = Some(r);
                crate::orchestra::review_ended(ed);
                crate::orchestra::review_ended(ed);
            });
            format!("a review the parent started in {name} stops ({token})")
        }
        _ => unreachable!("one arm a weight"),
    }
}

fn held(ed: &Editor, w: &mut World) -> Result<(), String> {
    if ed.workspace.id != w.front {
        return Err(format!("the front is {}, the user's last switch chose {}", ed.workspace.id, w.front));
    }
    for ch in &mut w.children {
        if ch.ended {
            if chat_of(ed, ch.id).is_some() {
                return Err(format!("{} was ended and is still there", ch.name));
            }
            continue;
        }
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
        // (one waiting in a child the user ended goes with it)
        if ch.ended && at(ch, token)?.is_none() {
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
    let (own, shared): (Vec<String>, Vec<String>) =
        (w.parent.try_iter().collect(), w.shared.try_iter().collect());
    // (the parent's: its own stand-in's, and of the shared one's those no child got — a child gets the parent's words
    // framed, or the user's tokens)
    let childs = |l: &&String| l.contains("<from-session") || tokens(l).iter().any(|t| t.starts_with("[u"));
    let parents: Vec<&String> = own.iter().chain(shared.iter().filter(|l| !childs(l))).collect();
    let lines: Vec<String> = own.iter().chain(&shared).cloned().collect();
    // A card goes only by the user's key: each not answered still up where it came; nothing of any card in what the
    // parent's claude got
    for (at, id, token, answered, released) in &w.cards {
        let front = ed.workspace.id == *at;
        let all = std::iter::once(&ed.workspace).chain(&ed.workspaces);
        let parked = all.filter(|l| l.id == *at).flat_map(|l| &l.parked).any(|o| o.id == *id);
        let up = parked || (front && ed.offers.iter().any(|o| o.id == *id));
        let gone = w.children.iter().any(|ch| ch.ended && ch.id == *at);
        if !answered && !released && !up && !gone {
            return Err(format!("card {token} in {at} went, unanswered by the user"));
        }
        if *answered && up {
            return Err(format!("card {token} in {at} answered and still up"));
        }
        // (a card of its own session's is its own to see — Claude's level keeps its own cards)
        if *at != w.home
            && let Some(l) = parents.iter().find(|l| l.contains(token.as_str()))
        {
            return Err(format!("the parent's claude got a child's card's words: {l}"));
        }
    }
    // A wake: only for a turn that asked, never while the parent's own card waits, never past `CAP` in an hour
    let now_ms = crate::usage::now_ms();
    let card_up = w.cards.iter().any(|(at, id, ..)| {
        *at == w.home && {
            let all = std::iter::once(&ed.workspace).chain(&ed.workspaces);
            let parked = all.filter(|l| l.id == *at).flat_map(|l| &l.parked).any(|o| o.id == *id);
            parked || (ed.workspace.id == *at && ed.offers.iter().any(|o| o.id == *id))
        }
    });
    for l in lines.iter().filter(|l| l.contains(WOKE)) {
        if !news_in(l).iter().flat_map(|b| tokens(b)).any(|t| w.wakers.contains(&t)) {
            return Err(format!("a wake for no turn that asked: {l}"));
        }
        if card_up {
            return Err(format!("woken while a card of the parent's waits: {l}"));
        }
        w.woke_at.push(now_ms);
        let hour = w.woke_at.iter().filter(|&&t| now_ms.saturating_sub(t) < 3_600_000).count();
        if hour > CAP {
            return Err(format!("{hour} wakes within an hour, llm.parent-wakes {CAP}"));
        }
    }
    let conv = w.conv;
    w.told.extend(lines.iter().flat_map(|l| news_in(l)).map(|b| (b, conv)));
    let parent =
        std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == w.home).ok_or("no parent")?;
    let now = parent.claude.chat.as_ref().and_then(|c| c.conversation.clone());
    let kept = |t: &str| {
        // (the whole news: a proposal's token is in its child's name)
        let of = |n: &&crate::orchestra::News| format!("{n:?}").contains(t);
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
