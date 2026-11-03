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

/// A side panel's file rows (`presence::file_marks`): the turns' files, one of yours no Claude is in, a review's.
fn rows() -> Vec<PathBuf> {
    let mut rels: Vec<PathBuf> = (0..TURN_FILES).map(|j| PathBuf::from(format!("src/m{j}.rs"))).collect();
    rels.extend(["lib/b0.rs", "src/r0/f0.rs", "a.rs"].map(PathBuf::from));
    rels
}

/// A side panel's file row is the work of its own marks: a lookup by its file and a few paths and allocations a mark,
/// the same with 34 sessions open as with 17 and whatever the busy turns' places (#214's third piece: each row
/// filtered and stable-sorted every presence and copied two paths for each, ~110 µs a row — ~4 ms for a 40-row
/// panel). What it says is what every presence filtered and sorted gives (`presence::in_file_anew`), and its prefixes
/// the busy sessions' names as told apart from `main` and `w1`…`w33` — written out, never worked out by the code
/// under test.
#[test]
fn a_side_panel_row_is_its_own_marks_work() {
    let mut most: Vec<(usize, Took)> = Vec::new();
    for n in [SESSIONS, 2 * SESSIONS] {
        let (ed, _dir) = many_of(&format!("docpaths-rows-{n}"), n);
        let (mut worst, mut marked) = (Took::default(), 0);
        for rel in rows() {
            let row = took(|| drop(std::hint::black_box(crate::presence::file_marks(&ed, &rel))));
            let (marks, _) = crate::presence::file_marks(&ed, &rel);
            assert_eq!(
                crate::presence::in_file(&ed, &rel),
                crate::presence::in_file_anew(&ed, &rel),
                "{rel:?}"
            );
            // (each one there by its prefix written out — the busy ones whole: `w1` all of what it shares with `w10`…,
            // `w4` one past the `w` it shares with the rest)
            let short = |n: String| match n.as_str() {
                "w1" | "w2" | "w3" | "w4" => n,
                _ => panic!("{n} on {rel:?}: not a busy one"),
            };
            let want: Vec<String> =
                crate::presence::in_file(&ed, &rel).into_iter().map(|w| short(w.0)).collect();
            assert_eq!(marks.iter().map(|m| m.prefix.clone()).collect::<Vec<_>>(), want, "{rel:?}");
            marked = marked.max(marks.len());
            worst = Took { paths: worst.paths.max(row.paths), allocs: worst.allocs.max(row.allocs) };
        }
        assert_eq!(marked, BUSY, "the busy ones' marks on a turn's file (their checkouts this repository)");
        eprintln!("ROW at {n} sessions: {worst:?}, {marked} marks");
        most.push((n, worst));
    }
    let (few, more) = (most[0].1, most[1].1);
    assert!(few.paths <= 2 + BUSY && few.allocs <= 4 + 8 * BUSY, "a row of {BUSY} marks: {few:?}");
    assert!(more.paths <= few.paths && more.allocs <= few.allocs, "17 → 34 sessions: {few:?} → {more:?}");
}

/// Memory a still event keeps: with #214's sessions open, hundreds of hovers and ticks, each with its frame, leave what
/// the main loop holds where it was — nothing kept per event or per frame (#214: the driver's tarae grew 318 → 442 MB
/// between two stalls; not these). Bytes allocated less those freed on this thread (`allocs::live`), after a round of
/// each to fill what's kept — held both ways: a block another thread made and this one freed counts against it, so
/// a count below zero would hide as much kept here.
#[test]
fn still_events_keep_no_memory() {
    let (mut ed, _dir) = many_of("docpaths-memory", SESSIONS);
    let mut buf = Vec::with_capacity(1 << 16);
    let mut round = |ed: &mut Editor, n: usize| {
        for i in 0..n {
            for kind in [Kind::Hover, Kind::Tick] {
                let e = event(ed, kind, i);
                ed.handle_event(e);
                buf.clear();
                crate::term::render(ed, &mut buf, W, H).unwrap();
            }
        }
    };
    round(&mut ed, 50);
    let at = crate::allocs::live();
    round(&mut ed, 400);
    let kept = crate::allocs::live() - at;
    eprintln!("KEPT {kept} bytes over 800 events and frames");
    // (and a block kept is seen: the count isn't blind)
    let block = std::hint::black_box(vec![0u8; 1 << 20]);
    assert!(crate::allocs::live() - at - kept >= 1 << 20, "a block kept, counted");
    drop(block);
    assert!(kept.abs() < 16 * 1024, "{kept} bytes kept over 800 still events and their frames");
}

/// What may differ between 17 sessions and 34 for one event or frame: a few paths and allocations, never one a session.
const SLACK: usize = 4;

/// The experience budget: a hover, a tick and a behind session's delta, and each one's frame, take the same paths and
/// allocations with 34 sessions open as with 17 (within `SLACK`) — what didn't change isn't walked. Measured with the
/// chat floating or hidden (the sessions' strip on the top row) and docked (the names on the chat's top line): either
/// line's work is what fits on it.
#[test]
fn an_event_of_nothing_shared_is_the_same_work_at_17_sessions_and_34() {
    let _one = one_at_a_time();
    let (mut few, _a) = laid("docpaths-still-17", SESSIONS);
    let (mut more, _b) = laid("docpaths-still-34", 2 * SESSIONS);
    let mut over = Vec::new();
    for docked in [false, true] {
        if docked {
            // (the front's chat open in its column, idle: its top line names the sessions)
            for ed in [&mut few, &mut more] {
                ed.workspace.claude.chat = Some(crate::chat::Chat::new());
            }
        }
        let at_few = per_kind(&mut few);
        let at_more = per_kind(&mut more);
        let line = if docked { "the chat's line" } else { "the strip" };
        assert_eq!(few.screen.chat_x.is_some(), docked, "{line}: the chat's column there or not");
        for ((kind, ev, frame), (_, ev2, frame2)) in at_few.iter().zip(&at_more) {
            eprintln!("STILL {line} {kind:?}: event {ev:?} → {ev2:?}; frame {frame:?} → {frame2:?}");
            for (what, a, b) in [("event", ev, ev2), ("frame", frame, frame2)] {
                if b.paths > a.paths + SLACK || b.allocs > a.allocs + SLACK {
                    over.push(format!(
                        "{line}, {kind:?}'s {what}: {a:?} at {SESSIONS} sessions, {b:?} at {}",
                        2 * SESSIONS
                    ));
                }
            }
        }
    }
    assert!(over.is_empty(), "growing with the sessions that didn't change:\n{}", over.join("\n"));
}
