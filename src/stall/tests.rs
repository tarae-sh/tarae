use std::time::Duration;

use super::*;
use crate::config::Config;
use crate::editor::Editor;
use crate::event::Event;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn look(w: &mut Watcher, stamp: &Stamp) -> Look {
    w.look(stamp, crate::clock::now())
}

/// The decision, on this thread's clock: nothing to time parks; a stamp is timed to `SLOW`, said once past it, its
/// stack once past `STACK`, its end with the whole time it stood however late the look — on screen the run's first
/// past `STACK` only, a later one a note, a short one never.
#[test]
fn a_stamp_past_slow_is_said_once_its_stack_once_and_its_end_with_its_whole_time() {
    let stamp = Stamp::default();
    let mut w = Watcher::new(LIMITS);
    let t0 = crate::clock::hold();
    assert_eq!(look(&mut w, &stamp).wake, Wake::Park, "nothing begun: parked");
    let label = Label::Command("goto_definition");
    stamp.begin(label);
    let l = look(&mut w, &stamp);
    assert!(l.acts.is_empty(), "{l:?}");
    assert_eq!(l.wake, Wake::At(t0 + SLOW));
    crate::clock::advance(SLOW);
    let l = look(&mut w, &stamp);
    assert_eq!(l.acts, [Act::Slow { label, so_far: SLOW }]);
    assert_eq!(l.wake, Wake::At(t0 + STACK));
    crate::clock::advance(ms(10));
    assert!(look(&mut w, &stamp).acts.is_empty(), "said once");
    crate::clock::advance(STACK);
    let l = look(&mut w, &stamp);
    assert_eq!(l.acts, [Act::Stack { label, so_far: SLOW + ms(10) + STACK }]);
    assert_eq!(l.wake, Wake::Park, "past every limit: parked until the stamp moves");
    crate::clock::advance(ms(500));
    stamp.idle();
    // (the look comes late: the time is the stamp's own)
    crate::clock::advance(ms(700));
    let took = SLOW + ms(510) + STACK;
    let l = look(&mut w, &stamp);
    assert_eq!(l.acts, [Act::Over { label, took, tell: Tell::Toast }]);
    assert_eq!(l.wake, Wake::Park, "idle");

    // A second one past `STACK`, ended by the next handler: a note, the next one timed
    stamp.begin(Label::Frame("draw"));
    crate::clock::advance(STACK);
    let l = look(&mut w, &stamp);
    assert_eq!(l.acts.len(), 2, "slow and its stack at once: {l:?}");
    stamp.begin(Label::Event("key"));
    let l = look(&mut w, &stamp);
    assert_eq!(l.acts, [Act::Over { label: Label::Frame("draw"), took: STACK, tell: Tell::Note }]);
    assert_eq!(l.wake, Wake::At(crate::clock::now() + SLOW));

    // A short one: never a word
    crate::clock::advance(ms(100));
    stamp.idle();
    let l = look(&mut w, &stamp);
    assert!(l.acts.is_empty(), "{l:?}");
    // Past SLOW, under STACK: the log only
    stamp.begin(Label::Named("a job"));
    crate::clock::advance(ms(300));
    assert_eq!(look(&mut w, &stamp).acts.len(), 1);
    stamp.idle();
    let l = look(&mut w, &stamp);
    assert_eq!(l.acts, [Act::Over { label: Label::Named("a job"), took: ms(300), tell: Tell::No }]);
    crate::clock::release();
}

/// A handler names what it does as it learns it — the same stamp, its time running on; idle, nothing is named.
#[test]
fn a_refined_label_is_the_stamps_own_and_an_idle_loop_takes_none() {
    let stamp = Stamp::default();
    let t0 = crate::clock::hold();
    stamp.begin(Label::Event("key"));
    let seq = stamp.seen().seq;
    crate::clock::advance(ms(40));
    stamp.refine(Label::Command("save"));
    let seen = stamp.seen();
    assert_eq!((seen.seq, seen.since, seen.label), (seq, Some(t0), Label::Command("save")));
    stamp.idle();
    stamp.refine(Label::Command("later"));
    assert_eq!(stamp.seen().label, Label::Command("save"), "idle: no stamp to name");
    crate::clock::release();
    // (a typed command's word copied whole, or cut on a character's boundary)
    assert_eq!(Label::Typed(Word::of("w")).to_string(), "`:w`");
    let long = Word::of("타래타래타래타래타래");
    assert_eq!(long.as_str(), "타래타래타래타", "23 bytes: seven syllables of three");
}

/// The watcher on its own thread, over the main loop's handling: a job's handler that stands past the limits is
/// logged by its site, its stack line written, and once the loop goes idle its end — then said on screen as a toast
/// through the loop. The handler waits for the watcher (its stack taken), never for a time.
#[test]
fn a_handler_that_stands_past_the_limits_is_logged_by_its_label_and_said_once_back() {
    let log = crate::log::capture();
    let mut ed = Editor::new(Config::default());
    let limits = Limits { slow: ms(30), stack: ms(60) };
    watch(ed.stall.clone(), ed.events.shared_sender(), limits);
    let stamp = ed.stall.clone();
    let s = stamp.clone();
    let stands = move |_: &mut Editor| {
        let start = crate::clock::now();
        while s.stacks() == 0 && crate::clock::since(start) < crate::test_results::PATIENCE {
            crate::event::sleep(ms(5));
        }
    };
    ed.events.shared_sender().send(Event::Job(Box::new(stands))).unwrap();
    let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the job");
    crate::term::handle_noting(&mut ed, ev);
    assert_eq!(stamp.stacks(), 1, "the handler stood until its stack was taken");
    ed.stall.idle();
    let said = |ed: &Editor| ed.toasts.iter().any(|t| t.text.starts_with("tarae stood still"));
    while !said(&ed) {
        let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the stall said over");
        crate::term::handle_noting(&mut ed, ev);
    }
    stamp.stop();
    let here = format!("job {}:", file!());
    let lines: Vec<String> =
        log.lines().into_iter().filter(|l| l.starts_with("stall") && l.contains(&here)).collect();
    let has = |what: &str| lines.iter().filter(|l| l.contains(what)).count();
    assert_eq!(has("the main loop has stood "), 1, "{lines:#?}");
    assert_eq!(has("no stack in tests"), 1, "{lines:#?}");
    assert_eq!(has("the main loop is back after "), 1, "{lines:#?}");
    let toast = ed.toasts.iter().find(|t| t.text.starts_with("tarae stood still")).unwrap();
    assert!(toast.text.contains(&here) && toast.text.ends_with("(:log-open)"), "{}", toast.text);
}

/// #300: a stall's end says the loop's CPU time from its first word on — whether it computed or waited. A handler
/// stands until its stall is said, then spins until its own thread has used `BUSY` of CPU (so a loaded machine can't
/// make it less), or sleeps as long: the spin's end says at least that much, the sleep's next to nothing.
#[test]
fn a_stalls_end_says_whether_the_loop_computed_or_waited() {
    const BUSY: Duration = Duration::from_millis(150);
    fn stand(spins: bool) -> String {
        let log = crate::log::capture();
        let mut ed = Editor::new(Config::default());
        watch(ed.stall.clone(), ed.events.shared_sender(), Limits { slow: ms(30), stack: ms(3_600_000) });
        let stamp = ed.stall.clone();
        let s = stamp.clone();
        // (named, so its lines are this stand's among every test's)
        let label = Label::Named(if spins { "the spinning stand" } else { "the sleeping stand" });
        let stands = move |_: &mut Editor| {
            s.refine(label);
            let start = crate::clock::now();
            while !s.said() && crate::clock::since(start) < crate::test_results::PATIENCE {
                crate::event::sleep(ms(5));
            }
            let here = Cpu::here();
            let cpu = || here.used().expect("this thread's cpu time");
            let from = cpu();
            match spins {
                true => {
                    while cpu() - from < BUSY {
                        std::hint::black_box((0..1000u64).sum::<u64>());
                    }
                }
                false => crate::event::sleep(BUSY),
            }
        };
        ed.events.shared_sender().send(Event::Job(Box::new(stands))).unwrap();
        let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the job");
        crate::term::handle_noting(&mut ed, ev);
        ed.stall.idle();
        let start = crate::clock::now();
        let back = || {
            let on = format!(" on {label} · ");
            log.lines().into_iter().find(|l| l.contains("the main loop is back after ") && l.contains(&on))
        };
        while back().is_none() && crate::clock::since(start) < crate::test_results::PATIENCE {
            crate::event::sleep(ms(5));
        }
        stamp.stop();
        back().expect("the stall's end logged")
    }
    // (each stand on a thread of its own, waited for with the shared patience: a lock the loop and the watcher both
    // wait on fails the test, never hangs it)
    let stand = |spins: bool| {
        let (tx, rx) = std::sync::mpsc::channel();
        crate::event::thread("tarae-stand", move || tx.send(stand(spins)));
        let what = if spins { "spinning" } else { "sleeping" };
        (rx.recv_timeout(crate::test_results::PATIENCE))
            .unwrap_or_else(|e| panic!("the {what} stand never came back ({e}): a lock waited on?"))
    };
    // (`run_duration`'s words back to a time: `152ms`, `1.2s`)
    let took = |w: &str| match w.strip_suffix("ms") {
        Some(n) => ms(n.parse().unwrap()),
        None => Duration::from_secs_f64(w.strip_suffix('s').unwrap().parse().unwrap()),
    };
    let cpu = |line: &str| {
        let (_, rest) = line.split_once(" · ").unwrap();
        match rest.split(' ').collect::<Vec<_>>()[..] {
            [used, "cpu", "of", watched, "watched"] => (took(used), took(watched)),
            _ => panic!("no cpu time: {line}"),
        }
    };
    let spun = stand(true);
    let (used, watched) = cpu(&spun);
    // (Linux counts in ticks of 10 ms: one lost each end)
    assert!(used >= BUSY - ms(20) && watched >= used - ms(20), "it computed: {spun}");
    let slept = stand(false);
    let (used, watched) = cpu(&slept);
    assert!(used < ms(50) && watched >= BUSY, "it waited: {slept}");
}
