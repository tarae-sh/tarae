//! What came on its own drying as it lands (`motion.rs` §6, #401/#402), simulated: Claude's cards coming and answered,
//! the session list opened and closed, a session behind starting and ending its turn and its news coming, children
//! coming, an answer said out of sight (`◂ new`), a run beginning and failing under its tab, typing begun and ended,
//! frames, ticks and time passing in any order — every place of §6 landing beside the others. After every
//! step: each place in one landing at most, each landing drying on its own clock (its places as fresh as its own age
//! says); one a frame started is whole on that frame, none starts while you type, and it cuts no older one short — a
//! place of it leaves the older only by landing again; the wake is the soonest of their own steps (`born + k·TICK`, 2 ms
//! past), now for one that's over — and none with none left: every wake ends.

use std::cell::Cell;
use std::time::{Duration, Instant};

use super::{Choices, Sim};
use crate::editor::{Editor, TICK};
use crate::key::Key;
use crate::motion::{Place, What, shown_for, strength};

const SEEDS: &[u64] = &[0, 1, 2, 7];

const INPUTS: usize = 60;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn what_came_on_its_own_dries_once_each_on_its_own_clock() {
    super::seeds("what_came_on_its_own_dries_once_each_on_its_own_clock", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("what_came_on_its_own_dries_once_each_on_its_own_clock", choices, LIMIT);
    // (the frames the scenario draws itself: each one's landing judged against the one before it)
    sim.frames = None;
    let dir = crate::scratch::Scratch::new("sim-arrivals");
    let mut ed = Editor::with_root(crate::config::Config::default(), dir.path().to_path_buf());
    ed.docs[0].text = ropey::Rope::from_str("alpha\nbeta\n");
    ed.motion_drawn = true;
    // (a second session, behind: the strip drawn, its word and news changing on their own)
    ed.workspace.name = "main".into();
    let docs = ed.workspace_add(dir.path().join("docs"));
    ed.workspace_by_id(docs).unwrap().name = "docs".into();
    // (main's conversation out of sight: `◂ new` where an answer comes)
    let mut c = crate::chat::Chat::new();
    c.hidden = true;
    ed.workspace.claude.chat = Some(c);
    let wrong = Cell::new(None::<String>);
    let check = |ed: &Editor| held(ed, &wrong);
    let root = dir.path().to_path_buf();
    sim.walk(&mut ed, INPUTS, |sim, ed| step(sim, ed, &wrong, &root), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn keys(sim: &Sim, ed: &mut Editor, keys: &[&str]) {
    for k in keys {
        sim.batch(ed, crate::event::Event::Key(k.parse::<Key>().unwrap()));
    }
}

/// The landings drying now, oldest first: when each began, and its places.
fn landings(ed: &Editor) -> Vec<(Instant, Vec<Place>)> {
    (ed.motions.iter())
        .filter_map(|m| match &m.what {
            What::Arrived(at) => Some((m.born, at.clone())),
            _ => None,
        })
        .collect()
}

/// A frame, as the main loop draws one — and what it started: whole on it, never while you type, cutting no older one
/// short (a place leaves it only by landing again).
fn frame(ed: &mut Editor, wrong: &Cell<Option<String>>) {
    let (before, typing) = (landings(ed), ed.typing());
    ed.focus_sync();
    crate::term::render(ed, &mut Vec::new(), 100, 30).unwrap();
    let after = landings(ed);
    // (the older ones in their order, each what it kept of its places — one emptied gone; a new one is pushed last. Two
    // born at one instant are told apart by that order, never by the time)
    let mut kept: Vec<&[Place]> = vec![&[]; before.len()];
    let mut j = 0;
    for (i, b) in before.iter().enumerate() {
        if let Some(a) = after.get(j).filter(|a| a.0 == b.0 && a.1.iter().all(|p| b.1.contains(p))) {
            (kept[i], j) = (&a.1, j + 1);
        }
    }
    let new = &after[j..];
    if new.len() > 1 {
        return wrong.set(Some(format!("{} landings began in one frame: {new:?}", new.len())));
    }
    let again: &[Place] = new.first().map_or(&[], |l| &l.1);
    for (i, (born, at)) in before.iter().enumerate() {
        let age = crate::clock::since(*born);
        if age >= shown_for(crate::motion::ARRIVED) {
            continue;
        }
        if let Some(p) = at.iter().find(|p| !kept[i].contains(p) && !again.contains(p)) {
            return wrong.set(Some(format!("{p:?} cut short {age:?} into its drying")));
        }
    }
    let Some((born, at)) = new.first() else { return };
    if typing {
        wrong.set(Some(format!("{at:?} began to dry while you typed")));
    } else if at.iter().any(|p| ed.arrival(p) < 1.0) || crate::clock::since(*born) > Duration::ZERO {
        wrong.set(Some(format!("{at:?} not whole on the frame it landed")));
    }
}

fn step(sim: &mut Sim, ed: &mut Editor, wrong: &Cell<Option<String>>, root: &std::path::Path) -> String {
    let c = &mut sim.choices;
    // (by arm: a card comes 4, one answered 2, the list opened 2, closed 1, typing 2, typing ends 2, a frame 3, a tick
    // 4, time passing 4, docs's turn begins or ends 2, its files again from disk 1, a child comes 1, an answer unseen 1, a
    // run begins or ends 2)
    match c.weighted(&[4, 2, 2, 1, 2, 2, 3, 4, 4, 2, 1, 1, 1, 2]) {
        9 => {
            let l = ed.workspaces.iter_mut().find(|l| l.name == "docs");
            let Some(l) = l else { return "docs is in front".into() };
            match l.claude.chat.take() {
                Some(_) => "docs's turn ends".into(),
                None => {
                    let mut c = crate::chat::Chat::new();
                    c.state = crate::chat::State::Thinking;
                    l.claude.chat = Some(c);
                    "docs's turn begins".into()
                }
            }
        }
        10 => match ed.workspaces.iter_mut().find(|l| l.name == "docs") {
            Some(l) => {
                l.reloaded ^= 1;
                format!("docs's news {}", if l.reloaded > 0 { "comes" } else { "seen" })
            }
            None => "docs is in front".into(),
        },
        11 => {
            let n = ed.workspaces.len();
            if n > 4 {
                return "children enough".into();
            }
            let main = ed.workspace.claude.id;
            let id = ed.workspace_add(root.join(format!("child-{n}")));
            let l = ed.workspace_by_id(id).unwrap();
            (l.name, l.claude.parent) = (format!("child-{n}"), Some(main));
            format!("child-{n} comes")
        }
        12 => match ed.workspace.claude.chat.as_mut() {
            Some(c) => {
                c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Assistant, "done", ""));
                c.news = Some(c.msgs.len() - 1);
                "an answer comes, unseen".into()
            }
            None => "no conversation".into(),
        },
        13 => {
            use crate::task::{Run, State, Task};
            match ed.workspace.task_run.as_mut() {
                Some(r) if r.state == State::Running => {
                    r.state = State::Failed(Some(1));
                    "the run fails".into()
                }
                Some(r) => {
                    r.state = State::Running;
                    "the run again".into()
                }
                None => {
                    let t = Task { name: "make".into(), cmd: "make".into(), cwd: root.to_path_buf() };
                    let mut r = Run::for_test(t);
                    r.state = State::Running;
                    ed.workspace.task_run = Some(r);
                    "a run begins".into()
                }
            }
        }
        0 => {
            let n = ed.offer_seq + 1;
            let what = crate::offer::What::Asks { call: crate::tools::Call::test(n) };
            let o = crate::offer::Offer::new(format!("Claude asks {n}"), "to see", "", String::new(), what);
            ed.push_offer(o);
            format!("card {n} comes")
        }
        1 => match ed.offers.is_empty() {
            true => "no card to answer".into(),
            false => {
                let o = ed.offers.remove(0);
                format!("card {} answered", o.id)
            }
        },
        2 => {
            keys(sim, ed, &["esc", "esc", "space", "W"]);
            "the session list opened".into()
        }
        3 => {
            keys(sim, ed, &["esc", "esc", "space", "W", "q"]);
            "the session list closed".into()
        }
        4 => {
            keys(sim, ed, &["esc", "esc", "i", "x"]);
            "typing".into()
        }
        5 => {
            keys(sim, ed, &["esc", "esc"]);
            "typing ends".into()
        }
        6 => {
            frame(ed, wrong);
            "a frame".into()
        }
        7 => {
            // (as the main loop's wake: its tick, then a frame — one past its time lets it go)
            let over = ed.motions.iter().any(|m| crate::clock::since(m.born) >= shown_for(m.life));
            ed.schedule_tick();
            if over && ed.unchanged {
                wrong.set(Some("a landing let go, and the tick said nothing changed".into()));
            }
            frame(ed, wrong);
            if let Some(m) = ed.motions.iter().find(|m| crate::clock::since(m.born) >= shown_for(m.life)) {
                wrong.set(Some(format!("a tick left a landing {:?} old", crate::clock::since(m.born))));
            }
            "a tick".into()
        }
        _ => {
            let ms = *c.pick(&[30, 100, 102, 250, 700, 1000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

/// Each place in one landing at most, as fresh as its own landing's age says; the wake the soonest of their own steps,
/// at most a step away (now for one over); none with none left.
fn held(ed: &Editor, wrong: &Cell<Option<String>>) -> Result<(), String> {
    if let Some(s) = wrong.take() {
        return Err(s);
    }
    let all = landings(ed);
    for (i, (born, at)) in all.iter().enumerate() {
        if let Some(p) = at.iter().find(|p| all[i + 1..].iter().any(|l| l.1.contains(p))) {
            return Err(format!("{p:?} in two landings at once"));
        }
        let k = strength(crate::clock::since(*born), crate::motion::ARRIVED);
        if let Some(p) = at.iter().find(|p| ed.moving() && ed.arrival(p) != k) {
            return Err(format!("{p:?} drawn at {} where its own clock says {k}", ed.arrival(p)));
        }
    }
    let wake = ed.motion_wake();
    let slack = Duration::from_millis(2);
    // (each motion's next step on its own clock: now for one over)
    let due = |m: &crate::motion::Moving| {
        let age = crate::clock::since(m.born);
        match age >= shown_for(m.life) {
            true => Duration::ZERO,
            false => {
                let next = (age.as_nanos() / TICK.as_nanos() + 1) * TICK.as_nanos();
                Duration::from_nanos((next - age.as_nanos()) as u64) + slack
            }
        }
    };
    let soonest = ed.motions.iter().map(due).min();
    match (wake, soonest) {
        (None, None) => Ok(()),
        (Some(w), None) => Err(format!("nothing drying, a wake asked in {w:?}")),
        (None, Some(_)) => Err("a landing drying, and no wake asked".into()),
        (Some(w), Some(d)) if w != d || w > TICK + slack => {
            Err(format!("a wake in {w:?}, the soonest step of what dries in {d:?}"))
        }
        _ => Ok(()),
    }
}

fn print(ed: &Editor) -> String {
    let at: Vec<String> = (landings(ed).iter())
        .map(|(born, at)| format!("{at:?} {} ms old", crate::clock::since(*born).as_millis()))
        .collect();
    format!(
        "cards {} · drying {} · typing {} · wake {:?}",
        ed.offers.len(),
        if at.is_empty() { "nothing".into() } else { at.join(", ") },
        ed.typing(),
        ed.motion_wake()
    )
}
