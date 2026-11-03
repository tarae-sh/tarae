//! #300: a chat's streamed events applied with #214's sessions open and a long conversation in front — what each event
//! costs the main loop (the job at `llm::pump`'s send, then every per-event sync), measured before anything is changed.

use super::*;
use crate::chat::{Look, Msg, Role};
use crate::event::Event;
use crate::llm::StreamEv;
use std::time::{Duration, Instant};

/// The front conversation's messages: a question, a read's row, an answer naming places, its time — turn after turn.
const MSGS: usize = 2000;

/// #214's scene with the front's chat docked, `MSGS` into its conversation, a turn under way.
fn scene(name: &str) -> (Editor, Held) {
    let (mut ed, held) = many(name);
    let mut c = crate::chat::Chat::new();
    let _ = crate::chat::stand_in_own(&mut c);
    let answer = |i: usize| {
        format!(
            "The reader keeps its lines in src/m3.rs:{} and the writer in src/m{}.rs:4 — **every** one it can.\n\n\
             ```rust\nfn parse(s: &str) -> u32 {{\n    s.lines().count() as u32\n}}\n```\n\n\
             - the caller hands it the whole text\n- nothing is copied twice\n\nThat's all, as far as the tests go.",
            1 + i % 9,
            i % TURN_FILES
        )
    };
    for i in 0..MSGS {
        c.msgs.push(match i % 4 {
            0 => Msg::new(Role::User, format!("Question {i}: how does parse handle a long input?"), ""),
            1 => Msg::tool(
                Look {
                    verb: "read",
                    what: format!("src/m{}.rs", i % TURN_FILES),
                    pattern: false,
                    range: "L10–49".into(),
                    link: None,
                },
                &format!("old{i}"),
            ),
            2 => Msg::new(Role::Assistant, answer(i), ""),
            _ => Msg::new(Role::Note, "4.6s", ""),
        });
    }
    c.msgs.push(Msg::new(Role::User, "And the writer?", ""));
    c.state = crate::chat::State::Thinking;
    c.hidden = false;
    ed.workspace.claude.chat = Some(c);
    // (laid out once, as the driver's had been)
    let mut buf = Vec::with_capacity(1 << 16);
    crate::term::render(&mut ed, &mut buf, W, H).unwrap();
    (ed, held)
}

/// The kinds of streamed event a turn sends, in the front chat or a busy one's behind.
#[derive(Clone, Copy, Debug)]
enum Kind {
    /// Answer text, 25 ms of it.
    Text,
    /// A read's call (its row, follow's stop).
    Tool,
    /// The read's result.
    Result,
    /// A word of a busy session's turn behind.
    Behind,
}

const KINDS: [Kind; 4] = [Kind::Text, Kind::Tool, Kind::Result, Kind::Behind];

fn event(ed: &Editor, kind: Kind, n: usize) -> Event {
    let (w, chat) = match kind {
        Kind::Behind => (ed.workspaces[0].id, ed.workspaces[0].claude.chat.as_ref()),
        _ => (ed.workspace.id, ed.workspace.claude.chat.as_ref()),
    };
    let g = chat.map_or(0, crate::chat::Chat::generation);
    let ev = match kind {
        Kind::Text | Kind::Behind => {
            StreamEv::Text(format!("word{n} and src/m{}.rs:{} ", n % TURN_FILES, n % 30))
        }
        Kind::Tool => StreamEv::Tool {
            id: format!("call{n}"),
            name: "mcp__tarae__read".into(),
            input: serde_json::json!({ "files": [format!("src/m{}.rs:1-40", n % TURN_FILES)] }),
        },
        Kind::Result => {
            StreamEv::ToolResult { id: format!("call{n}"), text: "1: fn a() {}".into(), error: false }
        }
    };
    let apply = move |ed: &mut Editor| crate::chat::stream_event(ed, g, ev);
    Event::For(w, ed.claude_of(w), Box::new(apply))
}

/// Each kind's event and the frame after it, timed — medians and the worst, said for `cargo perf`. `F300_EVENTS=n`
/// applies only that many of each, untimed, for a profiler.
#[test]
fn perf_stream_events_with_many_sessions() {
    if crate::test_results::unoptimized("docpaths::tests::stream::perf_stream_events_with_many_sessions") {
        return;
    }
    let (mut ed, _held) = scene("docpaths-stream-perf");
    let mut buf = Vec::with_capacity(1 << 16);
    let profiling = std::env::var("F300_EVENTS").ok().and_then(|n| n.parse::<usize>().ok());
    let rounds = profiling.unwrap_or(100);
    let mut took: Vec<(Kind, Vec<Duration>, Vec<Duration>)> =
        KINDS.iter().map(|&k| (k, Vec::new(), Vec::new())).collect();
    for n in 0..rounds {
        for (kind, events, frames) in &mut took {
            let e = event(&ed, *kind, n);
            let t = Instant::now();
            ed.handle_event(e);
            events.push(t.elapsed());
            buf.clear();
            let t = Instant::now();
            crate::term::render(&mut ed, &mut buf, W, H).unwrap();
            frames.push(t.elapsed());
        }
    }
    if profiling.is_some() {
        return;
    }
    for (kind, events, frames) in took {
        let kind = format!("{kind:?}").to_lowercase();
        for (what, mut all) in [("event", events), ("frame", frames)] {
            all.sort_unstable();
            let (med, worst) = (all[all.len() / 2], all[all.len() - 1]);
            let line = format!(
                "PERF stream_{kind}_{what}_us {:.1} budget=16000 noise=30\nWORST stream_{kind}_{what} {worst:?}\n",
                med.as_secs_f64() * 1e6
            );
            let _ = std::io::Write::write_all(&mut std::io::stderr(), line.as_bytes());
        }
    }
}
