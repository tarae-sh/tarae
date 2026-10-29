//! #214's fourth piece: what the scene keeps as it lives. The driver's tarae grew to ~537 MB over hours with 17 sessions;
//! still events keep nothing (`still::still_events_keep_no_memory`), so this lives as the driver does — whole turns in
//! the busy sessions behind (prose, tarae's `read` called and answered on a worker, the call's result, the end),
//! frames between — and says, a round at a time, what the process holds: its resident set (Linux) and the bytes
//! allocated and not freed by every thread (`allocs::total`).

use super::*;
use crate::event::Event;
use crate::llm::StreamEv;

/// The resident set, in KB (Linux's `/proc/self/status`; elsewhere none).
fn rss_kb() -> Option<usize> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = s.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// What lands from the workers, until it's quiet a while.
fn drain(ed: &mut Editor) {
    while let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
        ed.handle_event(ev);
    }
}

/// `f` in session `w`'s workspace, as its Claude's stream reaches it behind.
fn behind(ed: &mut Editor, w: crate::event::WorkspaceId, f: impl FnOnce(&mut Editor) + Send + 'static) {
    let s = ed.claude_of(w);
    ed.handle_event(Event::For(w, s, Box::new(f)));
}

fn generation(ed: &Editor) -> u64 {
    ed.workspace.claude.chat.as_ref().map_or(0, crate::chat::Chat::generation)
}

fn stream(ed: &mut Editor, w: crate::event::WorkspaceId, ev: impl FnOnce() -> StreamEv + Send + 'static) {
    behind(ed, w, move |ed| {
        let g = generation(ed);
        crate::chat::stream_event(ed, g, ev());
    });
}

/// Calls a turn makes.
const CALLS: usize = 30;

/// The acts of a turn, and the bytes each left held (`allocs::total` across it), summed over a round.
const ACTS: [&str; 7] = ["message", "words", "call", "answered", "result", "end", "frame"];
type Acts = [isize; ACTS.len()];

fn act(acts: &mut Acts, i: usize, f: impl FnOnce()) {
    let at = crate::allocs::total();
    f();
    acts[i] += crate::allocs::total() - at;
}

/// One turn of session `w`'s Claude: a message sent, then `CALLS` times a few words and a `read` of one of its files,
/// answered, then the end — a frame every few events.
fn turn(ed: &mut Editor, buf: &mut Vec<u8>, acts: &mut Acts, w: crate::event::WorkspaceId, t: usize) {
    act(acts, 0, || {
        behind(ed, w, move |ed| {
            assert!(crate::chat::submit(ed, &format!("turn {t}: look at the map again"), None, None), "sent");
        })
    });
    let mut frame = |ed: &mut Editor, acts: &mut Acts| {
        act(acts, 6, || {
            buf.clear();
            crate::term::render(ed, buf, W, H).unwrap();
        })
    };
    for c in 0..CALLS {
        act(acts, 1, || {
            for words in ["Let me look at ", "the map once more, ", "and then the next file.\n\n"] {
                stream(ed, w, move || StreamEv::Text(words.into()));
            }
        });
        let id = format!("t{t}-{c}");
        let input = serde_json::json!({ "files": [format!("src/m{}.rs:1-80", c % TURN_FILES)] });
        let (i, inp) = (id.clone(), input.clone());
        act(acts, 2, || {
            stream(ed, w, move || StreamEv::Tool { id: i, name: "mcp__tarae__read".into(), input: inp })
        });
        let request = serde_json::json!({"subtype": "mcp_message", "server_name": "tarae", "message": {
            "jsonrpc": "2.0", "id": c, "method": "tools/call",
            "params": {"name": "read", "arguments": input}}});
        let r = format!("r{t}-{c}");
        act(acts, 3, || {
            stream(ed, w, move || StreamEv::Control { request_id: r, request });
            drain(ed);
        });
        let text = "fn a() {}\n".repeat(400);
        act(acts, 4, || stream(ed, w, move || StreamEv::ToolResult { id, text, error: false }));
        if c % 3 == 0 {
            frame(ed, acts);
        }
    }
    act(acts, 5, || {
        stream(ed, w, || StreamEv::Done(Ok("Done: the map reads as it should.".into())));
        drain(ed);
    });
    frame(ed, acts);
}

/// By hand (`cargo xtask linux test --bin tarae memory_as_the_scene_lives -- --ignored --nocapture` for the resident
/// set; `F214_ROUNDS`, `F214_SITES=k` where round k's blocks still held were made — `allocs::sites`): rounds of a turn
/// in each busy session, what the process holds after each.
#[test]
#[ignore = "by hand: #214's memory, a round at a time"]
fn memory_as_the_scene_lives() {
    let (mut ed, _dir) = many_of("docpaths-grow", SESSIONS);
    let mut buf = Vec::with_capacity(1 << 16);
    let busy: Vec<_> = (0..BUSY).map(|k| ed.workspaces[k].id).collect();
    // (fresh chats, each its own stand-in and generation: the scene's are mid-turn; the lines sent to them read each round)
    let mut sent = Vec::new();
    for k in 0..BUSY {
        let root = ed.workspaces[k].root.clone();
        for j in 0..TURN_FILES {
            let p = root.join(format!("src/m{j}.rs"));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "fn a() {\n    let x = 1;\n}\n".repeat(70)).unwrap();
        }
        let mut c = crate::chat::Chat::new();
        sent.push(crate::chat::stand_in_own(&mut c));
        ed.workspaces[k].claude.chat = Some(c);
    }
    let rounds: usize = std::env::var("F214_ROUNDS").ok().and_then(|n| n.parse().ok()).unwrap_or(12);
    let say = |round: usize| {
        eprintln!(
            "GROW round {round:>3}: rss {:>8} KB, live {:>10} B (main {:>10})",
            rss_kb().map_or("-".into(), |k| k.to_string()),
            crate::allocs::total(),
            crate::allocs::live(),
        );
    };
    say(0);
    // (`F214_SITES=k`: round k's blocks kept with their stacks, what's left of them said after the last round)
    let sites: Option<usize> = std::env::var("F214_SITES").ok().and_then(|n| n.parse().ok());
    for round in 1..=rounds {
        crate::allocs::sites::track(sites == Some(round));
        let mut acts = Acts::default();
        for (k, &w) in busy.iter().enumerate() {
            turn(&mut ed, &mut buf, &mut acts, w, round * BUSY + k);
        }
        let by: Vec<String> = ACTS.iter().zip(acts).map(|(a, b)| format!("{a} {b}")).collect();
        eprintln!("ACTS round {round:>3}: {}", by.join(", "));
        // (what was sent to the stand-ins, and the tests' own record of every answer — `tools::ANSWERED`, ~1 KB a
        // call: the test binary's, never tarae's)
        for rx in &sent {
            while rx.try_recv().is_ok() {}
        }
        crate::tools::ANSWERED.with(|a| a.borrow_mut().clear());
        say(round);
    }
    if let Some(k) = sites {
        let held = crate::allocs::sites::held(6);
        let (bytes, blocks) = held.iter().fold((0, 0), |(b, n), h| (b + h.0, n + h.1));
        eprintln!("SITES round {k}'s blocks still held after round {rounds}: {bytes} B in {blocks} blocks");
        for (b, n, at) in held.iter().take(30) {
            eprintln!("SITE {b:>9} B {n:>6} ×  {at}");
        }
    }
}
