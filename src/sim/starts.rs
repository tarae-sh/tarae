//! The start's jobs on the simulated clock (#191, #201, `event::Starts`): what an idle check waits out is the start's
//! work only, never a timer. Two scenarios. A `:restart` with a warm conversation: the chat's restore (a job of the
//! start's) lands and arms the usage watch — a job asleep until idle compaction is due, 54 minutes on, then until the
//! cache goes cold; after every step nothing of the start's is out past `START_LATE` of the simulated clock, and at
//! the end none is out and the watch was armed. A job of the start's left out past `START_LATE`: the state's
//! `starting` says how long; landed, it's gone. And a poll of the start's that respawns from its own result (a
//! rebase's, the shared server's swap): its rounds age as one job, said late once past `START_LATE`.

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
