//! The freeze watcher (`stall.rs`), simulated: handlers begin and stand for the time the seed picks, keys go through
//! the main loop's batch, the loop goes idle, and the watcher looks when the seed says.
//!
//! After every look: a stall said only past `SLOW` and once, its stack only past `STACK` and once, a said stall's end
//! with the time it stood however late the look, on screen once a run (a toast, then notes); every stamp past a limit
//! at a look said by it; the watcher parked only where nothing is left to time, else woken at the next limit.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::stall::{Act, LIMITS, Label, SLOW, STACK, Tell, Wake, Watcher};

const SEEDS: &[u64] = &[0, 1, 2, 3, 9];

const INPUTS: usize = 80;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn the_watcher_says_every_stall_once_and_its_whole_time() {
    super::seeds("the_watcher_says_every_stall_once_and_its_whole_time", SEEDS, run);
}

/// The stamp under way as the scenario made it.
#[derive(Clone, Copy)]
struct Cur {
    since: Instant,
    said: bool,
    stacked: bool,
}

struct Model {
    watcher: Watcher,
    /// The stamp last seen, and the one standing (None: the loop idle) — followed after every step, a job's result
    /// landing too (`sync`).
    seq: u64,
    cur: Option<Cur>,
    /// A said stall that ended, and how long it stood — its end due at the next look.
    ended: Option<Duration>,
    /// A stall was said on screen this run.
    told: bool,
    wrong: Option<String>,
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("the_watcher_says_every_stall_once_and_its_whole_time", choices, LIMIT);
    sim.frames = Some((80, 24));
    let mut ed = Editor::new(crate::config::Config::default());
    ed.docs[0].text = ropey::Rope::from_str("one\ntwo\nthree\n");
    let model = RefCell::new(Model {
        watcher: Watcher::new(LIMITS),
        seq: 0,
        cur: None,
        ended: None,
        told: false,
        wrong: None,
    });
    let check = |ed: &Editor| {
        let mut m = model.borrow_mut();
        sync(&mut m, ed);
        m.wrong.take().map_or(Ok(()), Err)
    };
    sim.walk(&mut ed, INPUTS, |sim, ed| step(sim, ed, &model), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// The stamp moved in this step (a new one, or idle — the clock still where it was): the one before ended now, a
/// said one's end due at the next look.
fn sync(m: &mut Model, ed: &Editor) {
    let seen = ed.stall.seen();
    if seen.seq == m.seq {
        return;
    }
    if let Some(c) = m.cur.take().filter(|c| c.said) {
        m.ended = Some(crate::clock::since(c.since));
    }
    m.seq = seen.seq;
    if seen.seq & 1 == 1 {
        let since = seen.since.unwrap_or_else(crate::clock::now);
        m.cur = Some(Cur { since, said: false, stacked: false });
    }
}

fn step(sim: &mut Sim, ed: &mut Editor, model: &RefCell<Model>) -> String {
    let mut m = model.borrow_mut();
    // (by arm: a handler begins 2, a key through the loop 2, idle 2, time passing 4, the watcher looks 4)
    match sim.choices.weighted(&[2, 2, 2, 4, 4]) {
        0 => {
            ed.stall.begin(Label::Named("a step"));
            "a handler begins".into()
        }
        1 => {
            let k = *sim.choices.pick(&["j", "k", "w", "x"]);
            sim.batch(ed, crate::event::Event::Key(k.parse().unwrap()));
            // (left standing at the batch's end, the frame drawn in the same instant)
            let label = ed.stall.seen().label;
            if label != Label::Named("the batch's end") {
                m.wrong = Some(format!("after a batch the stamp says {label}"));
            }
            format!("key {k}")
        }
        2 => {
            ed.stall.idle();
            "the loop goes idle".into()
        }
        3 => {
            let ms = *sim.choices.pick(&[5, 100, 240, 260, 1900, 2100, 6000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => {
            let at = crate::clock::now();
            let look = m.watcher.look(&ed.stall, at);
            m.wrong = judged(&mut m, &look.acts, look.wake, at).err();
            let wake = match look.wake {
                Wake::Park => "parks".to_string(),
                Wake::At(t) => format!("wakes in {:?}", t - at),
            };
            format!("the watcher looks: {:?} · {wake}", look.acts)
        }
    }
}

/// A look's acts and wake against what the scenario made.
fn judged(m: &mut Model, acts: &[Act], wake: Wake, at: Instant) -> Result<(), String> {
    let mut over = m.ended.take();
    for act in acts {
        match *act {
            Act::Over { took, tell, .. } => {
                let Some(want) = over.take() else { return Err(format!("an end said, none due: {act:?}")) };
                if took != want {
                    return Err(format!("an end said as {took:?}, it stood {want:?}"));
                }
                let should = match took >= STACK {
                    false => Tell::No,
                    true if std::mem::replace(&mut m.told, true) => Tell::Note,
                    true => Tell::Toast,
                };
                if tell != should {
                    return Err(format!("{took:?} said {tell:?}, not {should:?}"));
                }
            }
            Act::Slow { so_far, .. } => {
                let c = m.cur.as_mut().ok_or("a stall said with the loop idle")?;
                if c.said || so_far < SLOW || so_far != at - c.since {
                    return Err(format!("said at {so_far:?}, said before: {}", c.said));
                }
                c.said = true;
            }
            Act::Stack { so_far, .. } => {
                let c = m.cur.as_mut().ok_or("a stack with the loop idle")?;
                if !c.said || c.stacked || so_far < STACK {
                    return Err(format!("a stack at {so_far:?}, taken before: {}", c.stacked));
                }
                c.stacked = true;
            }
        }
    }
    if let Some(d) = over {
        return Err(format!("a said stall ended after {d:?}, its end unsaid"));
    }
    let want = match m.cur {
        None => Wake::Park,
        Some(c) if at - c.since >= SLOW && !c.said => return Err("past SLOW at a look, unsaid".into()),
        Some(c) if at - c.since >= STACK && !c.stacked => return Err("past STACK at a look, no stack".into()),
        Some(c) if !c.said => Wake::At(c.since + SLOW),
        Some(c) if !c.stacked => Wake::At(c.since + STACK),
        Some(_) => Wake::Park,
    };
    match wake == want {
        true => Ok(()),
        false => Err(format!("woken {wake:?}, due {want:?}")),
    }
}

fn print(ed: &Editor) -> String {
    let s = ed.stall.seen();
    format!("stamp {} · {}", s.seq, s.label)
}
