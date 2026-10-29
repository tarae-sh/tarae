//! #214's second piece: the experience budget. With many sessions open, an event that changes nothing shared — the
//! mouse moving over the code, the working sign's tick, a behind session's stream delta — and the frame after it do the
//! same work with 17 sessions open as with 34 (the same busy ones, review and notes; the 17 more idle, each with its
//! buffers): paths compared, hashed, joined or looked at on disk (`docpaths::worked`) and allocations
//! (`crate::allocs`) counted, never growing with the sessions that didn't change.

use super::*;
use crate::event::Event;

/// What a stretch took: paths (`docpaths::work`) and allocations (`allocs::count`), on this thread.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Took {
    paths: usize,
    allocs: usize,
}

fn took(f: impl FnOnce()) -> Took {
    let (p, a) = (work(), crate::allocs::count());
    f();
    Took { paths: work() - p, allocs: crate::allocs::count() - a }
}

/// The kinds of event that change nothing shared, each made anew (`n`: which of a pair — the mouse goes back and
/// forth, the delta's words differ).
#[derive(Clone, Copy, Debug)]
enum Kind {
    /// The mouse moving over the code (`Event::Hover`): what's under it asked, none of the per-event syncs.
    Hover,
    /// The working sign's frame (`tick_landed`): every per-event sync.
    Tick,
    /// A word of a busy session's turn behind (`Event::For`): swapped in and out, every per-event sync.
    Delta,
}

const KINDS: [Kind; 3] = [Kind::Hover, Kind::Tick, Kind::Delta];

fn event(ed: &Editor, kind: Kind, n: usize) -> Event {
    match kind {
        Kind::Hover => Event::Hover(100 + 7 * (n % 2) as u16, 12 + (n % 2) as u16),
        Kind::Tick => {
            let at = crate::clock::now();
            Event::Job(Box::new(move |ed: &mut Editor| ed.tick_landed(at)))
        }
        Kind::Delta => {
            // (the first busy session, behind)
            let w = ed.workspaces[0].id;
            let g = ed.workspaces[0].claude.chat.as_ref().map_or(0, crate::chat::Chat::generation);
            let words = if n.is_multiple_of(2) { "Let me look " } else { "at the map. " };
            let apply = move |ed: &mut Editor| {
                crate::chat::stream_event(ed, g, crate::llm::StreamEv::Text(words.into()));
            };
            Event::For(w, ed.claude_of(w), Box::new(apply))
        }
    }
}

/// Each kind's event and the frame after it, at its most over a few of each (after a few to settle).
fn per_kind(ed: &mut Editor) -> Vec<(Kind, Took, Took)> {
    let mut buf = Vec::with_capacity(1 << 16);
    KINDS
        .iter()
        .map(|&kind| {
            let (mut ev_most, mut frame_most) = (Took::default(), Took::default());
            for n in 0..8 {
                let e = event(ed, kind, n);
                let ev = took(|| ed.handle_event(e));
                buf.clear();
                let frame = took(|| crate::term::render(ed, &mut buf, W, H).unwrap());
                if n >= 4 {
                    ev_most =
                        Took { paths: ev.paths.max(ev_most.paths), allocs: ev.allocs.max(ev_most.allocs) };
                    frame_most = Took {
                        paths: frame.paths.max(frame_most.paths),
                        allocs: frame.allocs.max(frame_most.allocs),
                    };
                }
            }
            (kind, ev_most, frame_most)
        })
        .collect()
}

/// The experience budget timed for `cargo perf`: each kind's event and frame, medians, at 17 sessions and at 34.
#[test]
fn perf_many_sessions_still() {
    if crate::test_results::unoptimized("docpaths::tests::still::perf_many_sessions_still") {
        return;
    }
    for n in [SESSIONS, 2 * SESSIONS] {
        let (mut ed, _dir) = many_of(&format!("docpaths-still-perf-{n}"), n);
        let mut buf = Vec::with_capacity(1 << 16);
        for kind in KINDS {
            let (mut events, mut frames) = (Vec::new(), Vec::new());
            for i in 0..200 {
                let e = event(&ed, kind, i);
                let t = std::time::Instant::now();
                ed.handle_event(e);
                events.push(t.elapsed());
                buf.clear();
                let t = std::time::Instant::now();
                crate::term::render(&mut ed, &mut buf, W, H).unwrap();
                frames.push(t.elapsed());
            }
            let kind = format!("{kind:?}").to_lowercase();
            for (what, mut all) in [("event", events), ("frame", frames)] {
                all.sort_unstable();
                let us = all[all.len() / 2].as_secs_f64() * 1e6;
                let line = format!("PERF many_sessions_{n}_{kind}_{what}_us {us:.1} budget=16000 noise=30\n");
                let _ = std::io::Write::write_all(&mut std::io::stderr(), line.as_bytes());
            }
        }
    }
}

/// What may differ between 17 sessions and 34 for one event or frame: a few paths and allocations, never one a session.
const SLACK: usize = 4;

/// The experience budget: a hover, a tick and a behind session's delta, and each one's frame, take the same paths and
/// allocations with 34 sessions open as with 17 (within `SLACK`) — what didn't change isn't walked.
#[test]
fn an_event_of_nothing_shared_is_the_same_work_at_17_sessions_and_34() {
    let (mut few, _a) = many_of("docpaths-still-17", SESSIONS);
    let (mut more, _b) = many_of("docpaths-still-34", 2 * SESSIONS);
    let at_few = per_kind(&mut few);
    let at_more = per_kind(&mut more);
    let mut over = Vec::new();
    for ((kind, ev, frame), (_, ev2, frame2)) in at_few.iter().zip(&at_more) {
        eprintln!("STILL {kind:?}: event {ev:?} → {ev2:?}; frame {frame:?} → {frame2:?}");
        for (what, a, b) in [("event", ev, ev2), ("frame", frame, frame2)] {
            if b.paths > a.paths + SLACK || b.allocs > a.allocs + SLACK {
                over.push(format!(
                    "{kind:?}'s {what}: {a:?} at {SESSIONS} sessions, {b:?} at {}",
                    2 * SESSIONS
                ));
            }
        }
    }
    assert!(over.is_empty(), "growing with the sessions that didn't change:\n{}", over.join("\n"));
}
