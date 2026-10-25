//! Per-pane follow, simulated (panes P3 step 8, plan §4): two code panes on one file, follow leading the left one (A)
//! while your keys are in the right one (B) — moves, typing, undo, a selection, half pages; Claude's edit streamed over
//! a line in pieces of any size (its place looked for on a worker, landing among the steps), its reads elsewhere in the
//! file, frames at two widths, the turn's end, the next edit, time passing. After every step: the keys stay in B and
//! follow stays A's, never paused by them; a step that wasn't a key never moved B's selection, nor its view but by a
//! frame; a piece that moved follow's line moved it onto the line a drawn block rewrites; and after a frame A's view
//! holds follow's line — the writing in sight in the pane that follows it, whatever came between.

use std::cell::RefCell;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::key::{Code, Key};
use crate::llm::StreamEv;
use crate::split::PaneId;
use crate::writing::{Delta, Scan};

const SEEDS: &[u64] = &[0, 1, 2, 3, 4];

const INPUTS: usize = 60;

const LIMIT: Duration = crate::test_results::PATIENCE;

const NAME: &str = "your_keys_in_one_pane_never_move_while_follow_leads_the_other";

#[test]
fn your_keys_in_one_pane_never_move_while_follow_leads_the_other() {
    super::seeds(NAME, SEEDS, run);
}

/// The scenario's own account: the turn's edit (its input, how much of it went, its reader), what the last step was,
/// B's selection and top as the last step left them.
#[derive(Default)]
struct World {
    turn: usize,
    input: String,
    sent: usize,
    scan: Option<Scan>,
    /// The last step was a key · a frame.
    keyed: bool,
    framed: bool,
    mine: Option<(crate::selection::Selection, usize)>,
    /// A piece moved follow's line where no drawn block rewrites.
    led_wrong: Option<String>,
    inputs: usize,
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let dir = crate::scratch::Scratch::new("sim-follow-pane");
    let text: String = (1..=60).map(|i| format!("    let x{i} = {i};\n")).collect();
    std::fs::write(dir.join("a.rs"), text).unwrap();
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = dir.to_path_buf();
    ed.open(&dir.join("a.rs")).unwrap();
    let mut c = crate::chat::Chat::new();
    let _claude = crate::chat::stand_in(&mut c);
    c.state = crate::chat::State::Thinking;
    c.follow.on = true;
    // (follow took the pane that had the code's keys as the turn began)
    let a = ed.workspace.viewer.focus;
    c.follow.pane = Some(a);
    ed.workspace.claude.chat = Some(c);
    crate::term::render(&mut ed, &mut Vec::new(), 120, 32).unwrap();
    // Your keys to a pane of their own, on the same file
    keys(&mut ed, "<C-w>v");
    let b = ed.workspace.viewer.focus;
    assert_ne!(a, b);
    crate::term::render(&mut ed, &mut Vec::new(), 120, 32).unwrap();
    let world = RefCell::new(World::default());
    begin(&mut ed, &mut world.borrow_mut(), 49);
    world.borrow_mut().mine = Some(mine(&ed));
    let check = |ed: &Editor| held(ed, a, b, &mut world.borrow_mut());
    sim.walk(&mut ed, INPUTS, |sim, ed| step(sim, ed, a, &mut world.borrow_mut()), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn generation(ed: &Editor) -> u64 {
    ed.workspace.claude.chat.as_ref().map_or(0, crate::chat::Chat::generation)
}

/// B's selection and top (B has the keys: the document in front, its scroll the document's).
fn mine(ed: &Editor) -> (crate::selection::Selection, usize) {
    (ed.doc().selection().clone(), ed.doc().top)
}

/// The next edit: line `line` kept, two written under it — the keys in the order the model writes them.
fn begin(ed: &mut Editor, w: &mut World, line: usize) {
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    c.state = crate::chat::State::Thinking;
    let doc = ed.doc();
    let old = doc.text.line(line.min(crate::movement::last_line(&doc.text))).to_string();
    let new = format!("{old}    // turn {}\n    let y{} = 0;\n", w.turn, w.turn);
    let s = |t: &str| serde_json::to_string(t).unwrap();
    w.input =
        format!(r#"{{"edits":[{{"file_path":"a.rs","old_string":{},"new_string":{}}}]}}"#, s(&old), s(&new));
    (w.sent, w.scan) = (0, Some(Scan::new()));
    w.turn += 1;
}

fn keys(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(ch) = chars.next() {
        let k: Key = match ch {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            ch => Key::plain(Code::Char(ch)),
        };
        // (through the loop, as a person's key comes: follow sees it after it — `follow::watch`)
        ed.handle_event(crate::event::Event::Key(k));
    }
}

fn step(sim: &mut Sim, ed: &mut Editor, a: PaneId, w: &mut World) -> String {
    let busy = ed.workspace.claude.chat.as_ref().is_some_and(|c| c.state != crate::chat::State::Idle);
    let g = generation(ed);
    w.inputs += 1;
    let c = &mut sim.choices;
    // (by arm: a piece of the input 6, a key 4, a frame 2, a read 1, the turn's end 1, the next edit 1, time 1; the
    // last input ends a turn under way — a turn left open keeps follow's timers going)
    let arm = if w.inputs == INPUTS && busy { 4 } else { c.weighted(&[6, 4, 2, 1, 1, 1, 1]) };
    match arm {
        0 => {
            let chars: Vec<char> = w.input.chars().collect();
            if !busy || w.sent >= chars.len() {
                return "(no input to stream)".into();
            }
            let n = *c.pick(&[1, 7, 30, 200]);
            let end = (w.sent + n).min(chars.len());
            let piece: String = chars[w.sent..end].iter().collect();
            w.sent = end;
            let call = format!("t{}", w.turn);
            let Some(scan) = w.scan.as_mut() else { return "(no reader)".into() };
            scan.feed(&piece);
            let d = Delta { call, pieces: scan.take(), done: end == chars.len() };
            let before = crate::follow::led_in(ed, a);
            crate::chat::stream_event(ed, g, StreamEv::Writing(d));
            let now = crate::follow::led_in(ed, a);
            if let Some(line) = now.filter(|_| now != before) {
                let doc = ed.docs.iter().find(|d| d.path == ed.doc().path).expect("a.rs");
                let last = crate::movement::last_line(&doc.text);
                let blocks = crate::writing::shown(doc).map(|s| s.blocks.clone()).unwrap_or_default();
                if !blocks.iter().any(|b| b.place().min(last) == line) {
                    w.led_wrong =
                        Some(format!("a piece led A's view to line {line}, no line rewritten there"));
                }
            }
            format!("{n} characters of the input ({end} of {})", chars.len())
        }
        1 => {
            let k = *c.pick(&["j", "k", "gg", "G", "ix<esc>", "u", "w", "x", "<C-d>", "<C-u>"]);
            w.keyed = true;
            keys(ed, k);
            format!("keys {k} in B")
        }
        2 => {
            let width = *c.pick(&[120, 160]);
            crate::term::render(ed, &mut Vec::new(), width, 32).unwrap();
            w.framed = true;
            format!("a frame, {width} columns")
        }
        3 => {
            let line = *c.pick(&[2, 20, 40, 58]);
            let path = ed.workspace.root.join("a.rs");
            let input = serde_json::json!({"file_path": path, "offset": line + 1, "limit": 1});
            let id = format!("r{}", w.inputs);
            crate::chat::stream_event(ed, g, StreamEv::Tool { id, name: "Read".into(), input });
            format!("a read of line {}", line + 1)
        }
        4 => {
            if !busy {
                return "(no turn to end)".into();
            }
            crate::chat::stream_event(ed, g, StreamEv::Done(Ok(String::new())));
            "the turn ends".into()
        }
        5 => {
            if busy && w.sent < w.input.chars().count() {
                return "(an edit under way)".into();
            }
            let line = *c.pick(&[5, 30, 55]);
            begin(ed, w, line);
            format!("the next edit, under line {}", line + 1)
        }
        _ => {
            let ms = *c.pick(&[30, 2000]);
            sim.wait(Duration::from_millis(ms));
            "time passes".into()
        }
    }
}

fn held(ed: &Editor, a: PaneId, b: PaneId, w: &mut World) -> Result<(), String> {
    if ed.workspace.viewer.focus != b || ed.keys_to().region.name() != "code" {
        return Err(format!(
            "the keys left B: pane {} · {}",
            ed.workspace.viewer.focus,
            ed.keys_to().region.name()
        ));
    }
    let f = &ed.workspace.claude.chat.as_ref().ok_or("the chat went")?.follow;
    if f.paused || f.pane != Some(a) || !f.on {
        return Err(format!("follow left A or paused: {:?} · paused {} · on {}", f.pane, f.paused, f.on));
    }
    if let Some(m) = w.led_wrong.take() {
        return Err(m);
    }
    // A step that wasn't a key never moved B's selection, nor its view but by a frame (keeping its cursor in sight)
    let now = mine(ed);
    if let Some(was) = w.mine.as_ref().filter(|_| !w.keyed) {
        if now.0 != was.0 {
            return Err(format!("B's selection moved with no key: {:?} → {:?}", was.0, now.0));
        }
        if !w.framed && now.1 != was.1 {
            return Err(format!("B's view moved with no key: top {} → {}", was.1, now.1));
        }
    }
    w.mine = Some(now);
    // After a frame, A's view holds follow's line
    if w.framed
        && let Some(line) = crate::follow::led_in(ed, a)
    {
        let drawn = ed.screen.panes.iter().find(|p| p.0 == a).ok_or("A not drawn")?;
        if !drawn.3.iter().any(|o| o.0 == line) {
            let shown: Vec<usize> = drawn.3.iter().map(|o| o.0).collect();
            return Err(format!("A's view lost follow's line {line}: {shown:?}"));
        }
    }
    // (the next step may be a job's — the simulation's own, not `step`: neither a key nor a frame)
    (w.keyed, w.framed) = (false, false);
    Ok(())
}

fn print(ed: &Editor) -> String {
    let c = ed.workspace.claude.chat.as_ref();
    let led = c.and_then(|c| c.follow.pane).and_then(|a| crate::follow::led_in(ed, a));
    format!(
        "{} · {:?} · A led {led:?} · B at line {} top {}",
        ed.keys_to().region.name(),
        c.map(|c| c.state),
        ed.cursor_line(),
        ed.doc().top
    )
}
