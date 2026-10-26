//! Claude's writing heard as it appears, simulated (`sound.rs`, `sound::typist`): an edit's text streamed in pieces
//! of any size at any gap, the next edit streaming once it's whole (on through its landing), frames ticking, keys of yours, its buffer leaving the pane and coming back, `:sound` flipped,
//! the edit landing unseen, the turn's end. After every step: a sound came only for what that step newly showed in a
//! buffer in sight while sound was on — a return only when that held a line's end, else a key or the space bar or
//! nothing; never two in a step; no two sounds closer than `GAP_MIN`, no two returns closer than `RETURN_GAP`, never
//! more than 16 keys in a second (on the simulation's clock, as each is heard); the landing heard once, only in sight;
//! nothing once the turn is over.

use std::cell::RefCell;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::llm::StreamEv;
use crate::sound::Sink;
use crate::sound::said::Kind;
use crate::writing::{Delta, Place, REVEAL_EVERY, Scan};

const SEEDS: &[u64] = &[0, 1, 2, 3, 4];

const INPUTS: usize = 120;

const LIMIT: Duration = crate::test_results::PATIENCE;

/// A sound's delay is an f32 of seconds: its time is that precise.
const EPS: Duration = Duration::from_micros(10);

const TEXT: &str = "fn main() {\n    let a = 1;\n    let b = 2;\n    println!(\"{a} {b}\");\n}\n";

const OLD: &str = "    let b = 2;\n";

const NEW: &str = "    let b = 2;\n    // a skein: one thread for each name\n    let skein = Skein::new(names);\n    for name in skein.names() {\n        skein.wind(name);\n    }\n";

/// The next edit Claude streams once the first is whole — still coming as the first lands.
const OLD2: &str = "    println!(\"{a} {b}\");\n";

const NEW2: &str =
    "    println!(\"{a} {b}\");\n    // every name wound: the skein is whole\n    skein.tie();\n";

#[test]
fn claudes_writing_is_heard_as_it_appears() {
    super::seeds("claudes_writing_is_heard_as_it_appears", SEEDS, run);
}

#[derive(Default)]
struct World {
    input: String,
    sent: usize,
    scan: Option<Scan>,
    whole: bool,
    /// The next edit's input, as the first's.
    input2: String,
    sent2: usize,
    scan2: Option<Scan>,
    landed: bool,
    /// What each item (its call, its place in it) had shown, and the sounds kept, at the last check.
    shown: Vec<((String, usize), usize)>,
    heard: usize,
    /// The step just taken was the landing, and whether its buffer was in sight then.
    landing: Option<bool>,
    inputs: usize,
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("claudes_writing_is_heard_as_it_appears", choices, LIMIT);
    let dir = crate::scratch::Scratch::new("sim-sound");
    std::fs::write(dir.join("a.rs"), TEXT).unwrap();
    std::fs::write(dir.join("b.rs"), "// another file\n").unwrap();
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = dir.to_path_buf();
    ed.open(&dir.join("a.rs")).unwrap();
    ed.config.sound = true;
    ed.sound.sink = Sink::Record(Vec::new());
    let mut c = crate::chat::Chat::new();
    let _claude = crate::chat::stand_in(&mut c);
    // (no follow: what's in sight is the steps' to choose)
    c.follow.on = false;
    c.state = crate::chat::State::Thinking;
    c.writing.paced = true;
    ed.workspace.claude.chat = Some(c);
    crate::term::render(&mut ed, &mut Vec::new(), 100, 32).unwrap();
    let s = |t: &str| serde_json::to_string(t).unwrap();
    let edit = |old: &str, new: &str| {
        format!(r#"{{"edits":[{{"file_path":"a.rs","old_string":{},"new_string":{}}}]}}"#, s(old), s(new))
    };
    let world = RefCell::new(World {
        input: edit(OLD, NEW),
        scan: Some(Scan::new()),
        input2: edit(OLD2, NEW2),
        scan2: Some(Scan::new()),
        ..World::default()
    });
    let root = dir.to_path_buf();
    let check = |ed: &Editor| held(ed, &mut world.borrow_mut());
    sim.walk(&mut ed, INPUTS, |sim, ed| step(sim, ed, &mut world.borrow_mut(), &root), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn busy(ed: &Editor) -> bool {
    ed.workspace.claude.chat.as_ref().is_some_and(|c| c.state != crate::chat::State::Idle)
}

fn a_rs(ed: &Editor) -> Option<&crate::document::Document> {
    ed.docs.iter().find(|d| d.path.as_ref().is_some_and(|p| p.ends_with("a.rs")))
}

fn step(sim: &mut Sim, ed: &mut Editor, w: &mut World, root: &std::path::Path) -> String {
    w.inputs += 1;
    w.landing = None;
    let c = &mut sim.choices;
    // (by arm: a piece 5, a frame's tick 10, time 2, a key of yours 2, the buffer out of the pane or back 1, `:sound`
    // 1, the edit landing 1, the turn's end 1, a piece of the next edit 4)
    let arm = if w.inputs == INPUTS && busy(ed) { 7 } else { c.weighted(&[5, 10, 2, 2, 1, 1, 1, 1, 4]) };
    let said = act(sim, ed, w, root, arm);
    // (a frame after the step's events, as the main loop draws: what's in sight is what it drew)
    crate::term::render(ed, &mut Vec::new(), 100, 32).unwrap();
    said
}

fn act(sim: &mut Sim, ed: &mut Editor, w: &mut World, root: &std::path::Path, arm: usize) -> String {
    let g = ed.workspace.claude.chat.as_ref().map_or(0, crate::chat::Chat::generation);
    let c = &mut sim.choices;
    match arm {
        0 => {
            let chars: Vec<char> = w.input.chars().collect();
            if !busy(ed) || w.sent >= chars.len() {
                return "(no input to stream)".into();
            }
            let n = *c.pick(&[1, 7, 16, 30, 190]);
            let end = (w.sent + n).min(chars.len());
            let piece: String = chars[w.sent..end].iter().collect();
            w.sent = end;
            let Some(scan) = w.scan.as_mut() else { return "(no reader)".into() };
            scan.feed(&piece);
            let done = end == chars.len();
            let d = Delta { call: "t1".into(), pieces: scan.take(), done };
            ed.handle_event(crate::event::Event::Job(Box::new(move |ed: &mut Editor| {
                crate::chat::stream_event(ed, g, StreamEv::Writing(d));
            })));
            w.whole |= done;
            format!("{n} characters of the input ({end} of {})", chars.len())
        }
        1 => {
            sim.wait(REVEAL_EVERY);
            ed.handle_event(crate::event::Event::Job(Box::new(|_: &mut Editor| {})));
            "a frame's tick".into()
        }
        2 => {
            let ms = *c.pick(&[5, 40, 160, 640, 1500]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        3 => {
            // (a key of yours that changes nothing of Claude's: the reveal still runs after it, as after any event)
            let key = *c.pick(&["j", "k", "l"]);
            ed.handle_event(crate::event::Event::Key(key.parse::<crate::key::Key>().unwrap()));
            format!("your {key}")
        }
        4 => {
            let here = ed.doc().path.as_ref().is_some_and(|p| p.ends_with("a.rs"));
            let to = if here { "b.rs" } else { "a.rs" };
            ed.open(&root.join(to)).unwrap();
            format!("{to} in the pane")
        }
        5 => {
            let _ = crate::typed::execute(ed, "sound");
            format!("sound {}", if ed.config.sound { "on" } else { "off" })
        }
        6 => {
            if w.landed || !w.whole || !busy(ed) {
                return "(nothing whole to land)".into();
            }
            w.landed = true;
            let Some(path) = a_rs(ed).and_then(|d| d.path.clone()) else { return "(a.rs is gone)".into() };
            let (before, after) = (TEXT.to_string(), TEXT.replace(OLD, NEW));
            std::fs::write(&path, &after).unwrap();
            let _ = crate::typed::execute(ed, "reload");
            // (as review at the end lands it — `refactor::write`)
            let f = crate::refactor::FileChange {
                path: path.clone(),
                rel: "a.rs".into(),
                before: before.as_str().into(),
                after: after.as_str().into(),
                places: 1,
                op: crate::refactor::Op::Text,
                protected: false,
            };
            let shown = a_rs(ed).is_some_and(|d| crate::sound::sight(ed).contains(&d.id));
            w.landing = Some(shown && ed.config.sound);
            crate::sound::landed(ed, &f);
            "the edit lands unseen".into()
        }
        8 => {
            // (the next edit, once the first is whole: its keys go on as the first lands)
            let chars: Vec<char> = w.input2.chars().collect();
            if !busy(ed) || !w.whole || w.sent2 >= chars.len() {
                return "(no next edit to stream)".into();
            }
            let n = *c.pick(&[1, 7, 16, 30, 190]);
            let end = (w.sent2 + n).min(chars.len());
            let piece: String = chars[w.sent2..end].iter().collect();
            w.sent2 = end;
            let Some(scan) = w.scan2.as_mut() else { return "(no reader)".into() };
            scan.feed(&piece);
            let done = end == chars.len();
            let d = Delta { call: "t2".into(), pieces: scan.take(), done };
            ed.handle_event(crate::event::Event::Job(Box::new(move |ed: &mut Editor| {
                crate::chat::stream_event(ed, g, StreamEv::Writing(d));
            })));
            format!("{n} characters of the next edit ({end} of {})", chars.len())
        }
        _ => {
            // (after both edits are whole — the last step ends it anyway)
            let next_whole = w.sent2 >= w.input2.chars().count();
            if !busy(ed) || !w.whole || (!next_whole && w.inputs < INPUTS) {
                return "(no turn to end)".into();
            }
            crate::chat::stream_event(ed, g, StreamEv::Done(Ok(String::new())));
            "the turn ends".into()
        }
    }
}

fn held(ed: &Editor, w: &mut World) -> Result<(), String> {
    let Sink::Record(kept) = &ed.sound.sink else { return Err("the record went".into()) };
    let fresh = &kept[w.heard..];
    // (what every item newly showed since the last check, of those in sight)
    let sight = crate::sound::sight(ed);
    let (mut new, mut in_sight, mut shown) = (String::new(), false, Vec::new());
    for c in ed.workspace.claude.chat.iter().flat_map(|c| &c.writing.calls) {
        for (i, it) in c.items.iter().enumerate() {
            let key = (c.id.clone(), i);
            let before = w.shown.iter().find(|(k, _)| *k == key).map_or(0, |(_, n)| *n);
            let now = it.pace.shown;
            if matches!(&it.place, Place::Found(f) if f.bound.is_some_and(|b| sight.contains(&b.0))) {
                in_sight = true;
                new.push_str(it.new.s.get(before.min(now)..now).unwrap_or_default());
            }
            shown.push((key, now));
        }
    }
    // (the landing's check once: a job the simulation runs next is checked as any step)
    match w.landing.take() {
        Some(heard) => {
            let kinds: Vec<Kind> = fresh.iter().map(|(_, s)| s.kind).collect();
            if kinds != if heard { vec![Kind::Settle] } else { vec![] } {
                return Err(format!("the landing (in sight and on: {heard}) sounded {kinds:?}"));
            }
        }
        None => {
            if fresh.len() > 1 {
                return Err(format!("{} sounds in one step", fresh.len()));
            }
            if let Some((_, s)) = fresh.first() {
                if !ed.config.sound || !in_sight || new.is_empty() {
                    return Err(format!(
                        "{:?} with sound {} and {:?} newly shown (in sight: {in_sight})",
                        s.kind, ed.config.sound, new
                    ));
                }
                if s.kind == Kind::Return && !new.contains('\n') {
                    return Err(format!("a return for {new:?}"));
                }
            }
        }
    }
    // As each is heard (to its f32 delay's precision): never two within GAP_MIN, never 17 keys in a second
    let at: Vec<std::time::Instant> =
        kept.iter().map(|(t, s)| *t + Duration::from_secs_f32(s.delay) + EPS).collect();
    if let Some(p) = at.windows(2).position(|p| p[1] + EPS < p[0] + crate::sound::typist::GAP_MIN) {
        return Err(format!(
            "sounds {p} and {} {:?} apart",
            p + 1,
            at[p + 1].saturating_duration_since(at[p])
        ));
    }
    let returns: Vec<std::time::Instant> =
        kept.iter().zip(&at).filter(|((_, s), _)| s.kind == Kind::Return).map(|(_, t)| *t).collect();
    if returns.windows(2).any(|r| r[1] + EPS < r[0] + crate::sound::typist::RETURN_GAP) {
        return Err("two returns within RETURN_GAP".into());
    }
    let keys: Vec<std::time::Instant> = kept
        .iter()
        .zip(&at)
        .filter(|((_, s), _)| matches!(s.kind, Kind::Key | Kind::Space))
        .map(|(_, t)| *t)
        .collect();
    for (i, t) in keys.iter().enumerate() {
        let within =
            keys[i..].iter().take_while(|k| k.saturating_duration_since(*t) < Duration::from_secs(1)).count();
        if within > 16 {
            return Err(format!("{within} keys within a second"));
        }
    }
    if !busy(ed) && fresh.iter().any(|(_, s)| s.kind != Kind::Settle) {
        return Err("a sound after the turn ended".into());
    }
    (w.shown, w.heard) = (shown, kept.len());
    Ok(())
}

fn print(ed: &Editor) -> String {
    let heard = match &ed.sound.sink {
        Sink::Record(k) => k.iter().map(|(_, s)| format!("{:?}", s.kind)).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    };
    format!(
        "sound {} · in front {:?} · heard {heard}",
        ed.config.sound,
        ed.doc().path.as_ref().and_then(|p| p.file_name())
    )
}
