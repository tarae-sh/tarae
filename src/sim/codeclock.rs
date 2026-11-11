//! The code tool's clock, simulated (`codetool.rs`'s `Watch`): a program's steps and the waits between them, each
//! step's watchdog a job sleeping in the simulation's time. A simulation runs one thread at a time, so the engine's
//! real work can't race its clock: how the clock moves while a step runs is modelled (`Watch::modelled`).
//!
//! - `a_programs_clock_counts_its_running_only`: on the stand-in engine, under the JIT's limit or the
//!   interpreter's, each of a program's three steps runs at least a time the seed picks (in tenths of the limit:
//!   1.5, 3.5 or 6.5 — a simulated sleep wakes at its time or later), and the user takes 30 s now and then before
//!   answering. After every step its running time is its steps' own (each measured where it ran), never a wait;
//!   it's never stopped before the limit; once two steps reach it, it ends by the clock, saying so.
//! - `work_the_steps_dont_see_ends_by_the_clock`: on the real engine (where `cargo xtask quickjs` built it), each
//!   of the re-pin's three shapes under the JIT and under the interpreter: its second step runs for real, on a
//!   thread of its own, while the simulated clock moves a tick at a time; its watchdog sets the flag at the limit
//!   (10 s, 60 s of simulated running), which stops the work inside the engine's builtin, and the program ends
//!   with the clock's words.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use super::{Choices, Sim};
use crate::codetool::tests::{HEAVY, real, script, stand_in, test_log};
use crate::codetool::{self, Ended, LIMITS, Limits, Model, Next, Program, Watch, Why, time_limit};
use crate::editor::Editor;

const SEEDS: &[u64] = &[0, 1, 2, 3, 7, 10];

const INPUTS: usize = 12;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_programs_clock_counts_its_running_only() {
    super::seeds("a_programs_clock_counts_its_running_only", SEEDS, counted);
}

#[test]
fn work_the_steps_dont_see_ends_by_the_clock() {
    super::seeds("work_the_steps_dont_see_ends_by_the_clock", &[0, 1], heavy);
}

/// What a step brought back, for the scenario to take.
type Landed = Arc<Mutex<Option<Next>>>;

fn lands(to: &Landed) -> impl FnOnce(&mut Editor, Next) + Send + 'static {
    let to = to.clone();
    move |_, next| *to.lock().unwrap() = Some(next)
}

/// A step runs `runs[n - 1]` of simulated time (or more: a sleep wakes at its time or later), then the engine
/// runs it (at once, in the simulation's time) — its flag set meanwhile, it stops at its first check. Each step's
/// span, from its clock's first stamp to its last (nothing yields between those and this), goes to `spans`.
fn sleeps(runs: Vec<Duration>, spans: Arc<Mutex<Vec<Duration>>>) -> Model {
    Arc::new(move |n: u32, _: &Watch, run: &mut (dyn FnMut() + Send)| {
        let t = crate::clock::now();
        crate::event::sleep(runs[n as usize - 1]);
        run();
        spans.lock().unwrap().push(crate::clock::since(t));
    })
}

/// The first step runs as any (to its call); the next runs for real on a thread of its own while the simulated
/// clock moves a tick at a time, until the flag is set — then it's waited for: the flag ends it.
fn races(tick: Duration) -> Model {
    Arc::new(move |n: u32, w: &Watch, run: &mut (dyn FnMut() + Send)| {
        if n == 1 {
            return run();
        }
        std::thread::scope(|s| {
            let t = std::thread::Builder::new().name("tarae-heavy".into()).spawn_scoped(s, run).unwrap();
            while w.stopped().is_none() {
                crate::event::sleep(tick);
            }
            t.join().unwrap();
        });
    })
}

struct World {
    watch: Arc<Watch>,
    /// Each step's running, at least, in order.
    runs: Vec<Duration>,
    /// Each step's running, as it ran.
    spans: Arc<Mutex<Vec<Duration>>>,
    limit: Duration,
    landed: Landed,
    waiting: RefCell<Option<(Box<Program>, u32)>>,
    ended: RefCell<Option<Ended>>,
}

fn counted(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("a_programs_clock_counts_its_running_only", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    let limits = Limits { jit: sim.choices.below(2) == 0, fuel: 1 << 30, ..LIMITS };
    let (limit, tenth) = (limits.clock(), limits.clock() / 10);
    let runs: Vec<Duration> = (0..3).map(|_| tenth * *sim.choices.pick(&[3u32, 7, 13]) / 2).collect();
    let spans = Arc::<Mutex<Vec<Duration>>>::default();
    let watch = Watch::modelled(&limits, sleeps(runs.clone(), spans.clone()));
    let pending = |id: u32| format!("pending\n{{\"pending\":[{{\"id\":{id},\"name\":\"x\"}}]}}");
    let src = script(&[&pending(0), &pending(1), "@"]);
    let landed = Landed::default();
    codetool::start_job(&ed.events.jobs(), || Ok(stand_in()), src, limits, watch.clone(), lands(&landed));
    let w =
        World { watch, runs, spans, limit, landed, waiting: RefCell::default(), ended: RefCell::default() };
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &w), |_| held(&w), |_| print(&w));
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// What a step brought back, taken: a call to answer, or the end.
fn take(w: &World) {
    match w.landed.lock().unwrap().take() {
        Some(Next::Calls(p, calls)) => {
            let id = calls.first().map(|c| c.id).or_else(|| p.waiting().next()).expect("a call out");
            *w.waiting.borrow_mut() = Some((p, id));
        }
        Some(Next::Ended(e)) => *w.ended.borrow_mut() = Some(e),
        None => {}
    }
}

/// The program waiting on a call: answered, or the user takes 30 s first (a card). While a step runs, time moves
/// only as the simulation moves it.
fn input(sim: &mut Sim, ed: &mut Editor, w: &World) -> String {
    take(w);
    let waiting = w.waiting.borrow_mut().take();
    match waiting {
        Some((p, id)) if sim.choices.below(3) > 0 => {
            codetool::answer_job(&ed.events.jobs(), p, id, json!(id), lands(&w.landed));
            format!("call {id} answered")
        }
        Some(p) => {
            *w.waiting.borrow_mut() = Some(p);
            sim.wait(Duration::from_secs(30));
            "the user takes 30 s".into()
        }
        None => "(no call waits)".into(),
    }
}

fn held(w: &World) -> Result<(), String> {
    take(w);
    let (ran, steps) = (w.watch.ran(), w.watch.steps() as usize);
    let spans = w.spans.lock().unwrap().clone();
    let measured: Duration = spans.iter().sum();
    // (a step running: its time so far is on top of the ones that ended)
    let counted = if spans.len() == steps { ran == measured } else { ran >= measured };
    if !counted {
        return Err(format!("ran {ran:?}, its steps {spans:?}: a wait counted"));
    }
    if let Some(at) = w.watch.stopped_at().filter(|&at| at < w.limit) {
        return Err(format!("stopped at {at:?}, before the limit {:?}", w.limit));
    }
    let Some(e) = &*w.ended.borrow() else { return Ok(()) };
    let two: Duration = w.runs[..2].iter().sum();
    match &e.result {
        Ok(_) if steps == 3 && two < w.limit && w.watch.stopped().is_none() => Ok(()),
        Err(words) if *words == time_limit(w.limit) && w.watch.stopped() == Some(Why::Clock) => Ok(()),
        r => Err(format!(
            "ended {r:?} after {steps} steps of {spans:?} (at least {:?}, limit {:?})",
            w.runs, w.limit
        )),
    }
}

fn print(w: &World) -> String {
    let end = match &*w.ended.borrow() {
        Some(e) => format!(" · ended {:?}", e.result),
        None => String::new(),
    };
    format!("{} steps, ran {:?}{end}", w.watch.steps(), w.watch.ran())
}

/// One program of `heavy`'s: what it is, its limits and clock, what landed.
struct Heavy {
    what: &'static str,
    limits: Limits,
    watch: Arc<Watch>,
    landed: Landed,
}

fn heavy(choices: Choices) -> Vec<String> {
    let Some(engine) = real() else { return Vec::new() };
    let mut sim = Sim::new("work_the_steps_dont_see_ends_by_the_clock", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    let log = test_log(2_000);
    let mut all = Vec::new();
    for jit in [true, false] {
        for (what, src) in HEAVY {
            let limits = Limits { jit, ..LIMITS };
            let watch = Watch::modelled(&limits, races(limits.clock() / 10));
            let landed = Landed::default();
            // (its call answered as it lands: the second step is the work)
            let answer = {
                let (landed, log) = (landed.clone(), log.clone());
                move |ed: &mut Editor, next: Next| match next {
                    Next::Calls(p, calls) => {
                        codetool::answer_job(&ed.events.jobs(), p, calls[0].id, json!(log), lands(&landed))
                    }
                    end => *landed.lock().unwrap() = Some(end),
                }
            };
            let e = engine.clone();
            codetool::start_job(&ed.events.jobs(), move || Ok(e), src.into(), limits, watch.clone(), answer);
            all.push(Heavy { what, limits, watch, landed });
        }
    }
    let print = |_: &Editor| {
        let ended = all.iter().filter(|h| h.landed.lock().unwrap().is_some()).count();
        let ran: Vec<String> = all.iter().map(|h| format!("{:?}", h.watch.ran())).collect();
        format!("{ended} ended, ran {}", ran.join(" "))
    };
    sim.walk(&mut ed, 0, |_, _| unreachable!("no inputs"), |_| Ok(()), print);
    sim.at_end(|| {
        for h in &all {
            let (clock, engine) = (h.limits.clock(), if h.limits.jit { "JIT" } else { "interpreter" });
            let Some(Next::Ended(end)) = h.landed.lock().unwrap().take() else {
                return Err(format!("{}, {engine}: didn't end", h.what));
            };
            if end.result != Err(time_limit(clock)) || h.watch.stopped() != Some(Why::Clock) {
                return Err(format!("{}, {engine}: ended {:?}, not by the clock", h.what, end.result));
            }
            let at = h.watch.stopped_at().unwrap_or_default();
            if at < clock || h.watch.steps() != 2 {
                return Err(format!("{}, {engine}: stopped at {at:?} in {} steps", h.what, h.watch.steps()));
            }
        }
        Ok(())
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}
