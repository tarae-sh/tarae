//! The start's jobs on the simulated clock (#191, #201, `event::Starts`): what an idle check waits out is the start's
//! work only, never a timer. Two scenarios. A `:restart` with a warm conversation: the chat's restore (a job of the
//! start's) lands and arms the usage watch — a job asleep until idle compaction is due, 54 minutes on, then until the
//! cache goes cold; after every step nothing of the start's is out past `START_LATE` of the simulated clock, and at
//! the end none is out and the watch was armed. A job of the start's left out past `START_LATE`: the state's
//! `starting` says how long; landed, it's gone. And a poll of the start's that respawns from its own result (a
//! rebase's, the shared server's swap): its rounds age as one job, said late once past `START_LATE`. And a start's
//! chats (projects step 2): the start's chat opened, its conversations read on a job, continues nothing and offers the
//! latest; a session the list brought back continues its own — unless a message of yours went before its read landed,
//! then that message begins a new one, never the old rows under a new conversation. And a crash's (projects step 3): a
//! gone tarae's folder, looked at on a job, its card up and nothing back until `y`; then the conversation it was in by
//! its id, not the one the store has first — unless a message of yours went before the read landed; which of the two
//! it came to said only once it has.

use std::cell::Cell;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;

const SEEDS: &[u64] = &[0, 1, 2];

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_restore_with_a_warm_conversation_leaves_nothing_of_the_start_out() {
    super::seeds("a_restore_with_a_warm_conversation_leaves_nothing_of_the_start_out", SEEDS, restore);
}

#[test]
fn a_start_job_out_past_its_time_is_said_with_it() {
    super::seeds("a_start_job_out_past_its_time_is_said_with_it", SEEDS, late);
}

#[test]
fn a_start_offers_its_last_conversation_and_a_session_brought_back_continues_its_own() {
    super::seeds(
        "a_start_offers_its_last_conversation_and_a_session_brought_back_continues_its_own",
        SEEDS,
        brought_back,
    );
}

#[test]
fn a_crashs_restore_continues_the_conversation_it_left() {
    super::seeds("a_crashs_restore_continues_the_conversation_it_left", SEEDS, |c| crash(c, 0));
}

#[test]
fn a_message_during_a_crashs_restore_is_never_under_the_old_rows() {
    super::seeds("a_message_during_a_crashs_restore_is_never_under_the_old_rows", SEEDS, |c| crash(c, 1));
}

#[test]
fn a_poll_that_respawns_ages_as_one_job() {
    super::seeds("a_poll_that_respawns_ages_as_one_job", SEEDS, respawns);
}

/// The start's jobs out past `START_LATE` — Err names them.
fn none_late(ed: &Editor) -> Result<(), String> {
    let late: Vec<String> = ed
        .events
        .starts_out()
        .into_iter()
        .filter(|(_, out)| *out >= crate::event::START_LATE)
        .map(|(at, out)| format!("{at} out {}", crate::term::run_duration(out)))
        .collect();
    match late.is_empty() {
        true => Ok(()),
        false => Err(format!("the start's still out past {:?}: {late:?}", crate::event::START_LATE)),
    }
}

fn print(ed: &Editor) -> String {
    let out: Vec<String> = ed.events.starts_out().iter().map(|(at, _)| at.to_string()).collect();
    format!("starting {out:?} · usage watch {:?}", ed.workspace.claude.usage.watch)
}

fn restore(choices: Choices) -> Vec<String> {
    let name = "a_restore_with_a_warm_conversation_leaves_nothing_of_the_start_out";
    let dir = crate::scratch::Scratch::new("sim-starts");
    let mut sim = Sim::new(name, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.chats_dir = Some(dir.to_path_buf());
    // (the conversation as `:restart` left it: its last request wrote to the 1h cache just now — warm; under
    // `idlecompact::FLOOR`, so its wake at 54 minutes lets compaction go and sleeps on until the cache is cold)
    let mut u = crate::usage::Usage::default();
    u.fresh("conv-1");
    let now = crate::usage::now_ms();
    let counts = crate::usage::Counts { input: Some(2), write: Some(20_000), ..Default::default() };
    u.requesting(now);
    u.seen(crate::usage::Seen {
        id: Some("r1".into()),
        model: "m".into(),
        counts,
        ttl: Some(crate::usage::Ttl::Hour1),
        at_ms: now,
    });
    u.spent(crate::usage::Spent { conversation: "conv-1".into(), at_ms: now, ..Default::default() });
    let saved = crate::chats::Saved {
        id: "conv-1".into(),
        title: "before the restart".into(),
        updated: crate::chats::now(),
        rows: Vec::new(),
        plan: None,
        usage: u.conv_json("conv-1"),
        places: None,
        seq: crate::chats::seq(),
    };
    crate::chats::save(&dir, &ed.workspace.root, saved);
    // (the start: the panel as it was — lazy, no claude until a message — then the first frame)
    ed.events.set_starting(true);
    let r = crate::chat::Restored {
        conversation: Some("conv-1".into()),
        input: String::new(),
        cursor: 0,
        focused: false,
        scroll: 0,
        follow: false,
        at_end: None,
        lazy: true,
    };
    crate::chat::restore(&mut ed, r);
    ed.events.started();
    let armed = Cell::new(false);
    let check = |ed: &Editor| {
        armed.set(armed.get() || ed.workspace.claude.usage.watch.is_some());
        none_late(ed)
    };
    sim.walk(&mut ed, 0, |_, _| unreachable!("no inputs"), check, print);
    let out = ed.events.starts_out();
    sim.at_end(|| match (armed.get(), out.is_empty()) {
        (true, true) => Ok(()),
        (false, _) => Err("the usage watch was never armed: the restore's conversation wasn't warm".into()),
        (true, false) => Err(format!("the start's still out: {out:?}")),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// A conversation of `root`'s kept two hours ago, its one row `title`.
fn kept(dir: &std::path::Path, root: &std::path::Path, id: &str, title: &str) {
    let rows = crate::chats::rows_of(&[crate::chat::Msg::new(crate::chat::Role::User, title, "")]);
    let saved = crate::chats::Saved {
        id: id.into(),
        title: title.into(),
        updated: crate::chats::now() - 2 * 3_600,
        rows,
        plan: None,
        usage: None,
        places: None,
        seq: crate::chats::seq(),
    };
    crate::chats::save(dir, root, saved);
}

fn brought_back(choices: Choices) -> Vec<String> {
    let name = "a_start_offers_its_last_conversation_and_a_session_brought_back_continues_its_own";
    let dir = crate::scratch::Scratch::new("sim-brought-back");
    let mut sim = Sim::new(name, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.chats_dir = Some(dir.join("chats"));
    // (claude a stand-in: what it's sent, never a process)
    let (tx, _rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    let (home, kid) = (ed.workspace.root.clone(), dir.join("kid"));
    std::fs::create_dir_all(&kid).unwrap();
    kept(&dir.join("chats"), &home, "c-home", "the start's");
    kept(&dir.join("chats"), &kid, "c-kid", "the session's");
    // (the start: its chat opened; a session the list brought back, its chat opened too — each read on a job)
    crate::chat::open(&mut ed);
    let id = ed.workspace_add(kid.clone());
    ed.with_workspace(id, |ed| {
        ed.workspace.claude.brought_back = true;
        crate::chat::open(ed);
    });
    let chat_of = |ed: &Editor, root: &std::path::Path| {
        std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.root == root).and_then(|l| {
            let c = l.claude.chat.as_ref()?;
            let old = c.msgs.iter().any(|m| m.text == "the start's" || m.text == "the session's");
            Some((c.conversation.clone(), old, c.past.first().map(|p| p.id.clone())))
        })
    };
    // A message in the session brought back, anywhere among the reads
    let input = |_: &mut Sim, ed: &mut Editor| {
        ed.with_workspace(id, |ed| {
            ed.workspace.claude.chat.as_mut().unwrap().input = "go on".into();
            crate::chat::send(ed);
        });
        "a message in kid".to_string()
    };
    let check = |ed: &Editor| {
        let (Some(home_chat), Some(kid_chat)) = (chat_of(ed, &home), chat_of(ed, &kid)) else {
            return Err("a chat closed".into());
        };
        if home_chat.0.is_some() || home_chat.1 {
            return Err(format!("the start continued: {home_chat:?}"));
        }
        let (conv, old, ..) = kid_chat;
        match old == (conv.as_deref() == Some("c-kid")) {
            true => Ok(()),
            false => Err(format!("kid's old rows {old} under conversation {conv:?}")),
        }
    };
    let print = |ed: &Editor| format!("home {:?} · kid {:?}", chat_of(ed, &home), chat_of(ed, &kid));
    sim.walk(&mut ed, 1, input, check, print);
    let (home_chat, kid_chat) = (chat_of(&ed, &home), chat_of(&ed, &kid));
    sim.at_end(|| match (home_chat, kid_chat) {
        (Some((None, false, Some(offered))), Some(_)) if offered == "c-home" => Ok(()),
        (home, _) => Err(format!("the start's chat: {home:?} — nothing continued, c-home offered")),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// The start after a crash: a gone tarae's folder keeping `c-left`, the store holding a later one first. `messages`: one
/// sent anywhere among the restore's reads.
fn crash(choices: Choices, messages: usize) -> Vec<String> {
    let name = match messages {
        0 => "a_crashs_restore_continues_the_conversation_it_left",
        _ => "a_message_during_a_crashs_restore_is_never_under_the_old_rows",
    };
    let dir = crate::scratch::Scratch::new("sim-crash");
    let mut sim = Sim::new(name, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    let chats = dir.join("chats");
    ed.chats_dir = Some(chats.clone());
    let (tx, _rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    let home = ed.workspace.root.clone();
    kept(&chats, &home, "c-left", "the crash's");
    kept(&chats, &home, "c-later", "a later one's");
    let state = dir.join("recovery");
    crate::recovery::chat_left(&state.join("gone-1"), 1, &home, &home, "c-left");
    ed.recovery = Some(crate::recovery::Recovery::new(state, home));
    ed.recovery_scan();
    sim.walk(&mut ed, 0, |_, _| unreachable!("no inputs"), |_| Ok(()), print);
    let asked = ed.offers.iter().filter(|o| o.recovers()).count();
    let before = ed.workspace.claude.chat.is_some();
    ed.offer_accept();
    let of = |ed: &Editor| {
        ed.workspace.claude.chat.as_ref().map(|c| {
            let old = c.msgs.iter().any(|m| m.text == "the crash's");
            let later = c.msgs.iter().any(|m| m.text == "a later one's");
            (c.conversation.clone(), c.resume.clone(), old, later)
        })
    };
    let input = |_: &mut Sim, ed: &mut Editor| {
        ed.workspace.claude.chat.as_mut().unwrap().input = "go on".into();
        crate::chat::send(ed);
        "a message".to_string()
    };
    let check = |ed: &Editor| match of(ed) {
        None => Err("the chat closed".into()),
        Some((_, _, _, true)) => Err("the store's first, not the conversation it left".into()),
        Some((conv, _, old, _)) if old != (conv.as_deref() == Some("c-left")) => {
            Err(format!("its old rows {old} under conversation {conv:?}"))
        }
        Some(_) => Ok(()),
    };
    let said = |ed: &Editor| format!("chat {:?}", of(ed));
    sim.walk(&mut ed, messages, input, check, said);
    let end = of(&ed);
    // (what's said is what came of it, whichever landed first: back, or the chat's own — one of the two)
    let continued = end.as_ref().is_some_and(|e| e.0.as_deref() == Some("c-left"));
    let back = ed.recovery.as_ref().is_some_and(crate::recovery::Recovery::back_unsaid);
    let own = ed.messages.iter().any(|m| m.2.contains("of its own"));
    sim.at_end(|| match (asked, before, end) {
        (1, false, _) if messages == 1 => Ok(()),
        (1, false, Some((Some(c), Some(r), true, false))) if c == "c-left" && r == "c-left" => Ok(()),
        (asked, before, end) => {
            Err(format!("{asked} card, chat before `y` {before}, at the end {end:?} — c-left, resumed"))
        }
    });
    sim.at_end(|| match back == continued && own != continued {
        true => Ok(()),
        false => Err(format!("continued {continued}, said back {back}, said its own {own}")),
    });
    // (the keeper's folder gone before the scratch is: nothing of it left to race its removal)
    ed.recovery_end(false);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// A poll's pause (rebase's `POLL`).
const POLL: Duration = Duration::from_millis(150);

/// A stand-in poll: pauses, and its result asks again until `left` rounds are done — each round a job of its own.
fn poll(ed: &mut Editor, left: u32) {
    ed.events.jobs().spawn(move || {
        crate::event::pause(POLL);
        move |ed: &mut Editor| {
            if left > 0 {
                poll(ed, left - 1)
            }
        }
    });
}

fn respawns(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("a_poll_that_respawns_ages_as_one_job", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    // (rounds past `START_LATE` and a second, on the simulated clock)
    let rounds = ((crate::event::START_LATE + Duration::from_secs(1)).as_millis() / POLL.as_millis()) as u32;
    ed.events.set_starting(true);
    poll(&mut ed, rounds);
    ed.events.started();
    let late = std::cell::RefCell::new(None);
    let check = |ed: &Editor| {
        if late.borrow().is_none() {
            *late.borrow_mut() = ed.ui_state().starting.into_iter().find(|s| s.contains(" out "));
        }
        Ok(())
    };
    sim.walk(&mut ed, 0, |_, _| unreachable!("no inputs"), check, print);
    let (late, out) = (late.into_inner(), ed.events.starts_out());
    sim.at_end(|| match late {
        Some(said) if said.starts_with("job src/sim/starts.rs:") && out.is_empty() => Ok(()),
        Some(said) => Err(format!("said late {said:?}, still out at the end: {out:?}")),
        None => Err(format!(
            "{rounds} rounds of {POLL:?}, never said late: each round counted from its own spawn"
        )),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn late(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("a_start_job_out_past_its_time_is_said_with_it", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.events.set_starting(true);
    ed.events.jobs().spawn(|| |_: &mut Editor| {});
    ed.events.started();
    // (its worker not run yet: out, and the clock past `START_LATE`)
    let said = |ed: &Editor| ed.ui_state().starting;
    let early = said(&ed);
    sim.wait(crate::event::START_LATE + Duration::from_secs(1));
    let then = said(&ed);
    sim.walk(&mut ed, 0, |_, _| unreachable!("no inputs"), |_| Ok(()), print);
    let landed = said(&ed);
    sim.at_end(|| {
        let one = |s: &[String]| match s {
            [one] => Ok(one.clone()),
            _ => Err(format!("one of the start's out: {s:?}")),
        };
        let (early, then) = (one(&early)?, one(&then)?);
        match (
            early.starts_with("job src/sim/starts.rs:") && !early.contains(" out "),
            then.ends_with(" out 6.0s"),
        ) {
            (true, true) if landed.is_empty() => Ok(()),
            (true, true) => Err(format!("landed, still said: {landed:?}")),
            _ => Err(format!("said as it went out {early:?}, past its time {then:?}")),
        }
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}
