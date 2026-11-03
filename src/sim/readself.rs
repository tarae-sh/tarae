//! The flow plan's piece 3, simulated: an answer no place holds opens the reader — the docked column — by itself. Turns end with answers
//! naming a line of `src/a.rs` (a note once its name is found on a worker after a frame), a file that isn't there (no
//! place once that search lands), or no file at all (none to search: at once); frames, time, typing begun and ended, the
//! reader hidden, in any order — a search may land while you type, after the next turn, or never before the end. After
//! every step: an open waits only while the chat is out of sight and you type, and the keys are never the chat's by it.
//! At the end, typing over and every search landed: the newest answer's way is taken — its card at its line, or the
//! reader in sight (unless you hid it since).

use std::time::Duration;

use super::{Choices, Sim};
use crate::chat::{Msg, Role};
use crate::editor::{Editor, Mode};
use crate::workspace::At;

const NAME: &str = "an_answer_no_place_holds_opens_the_reader_by_itself";

const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5];

const INPUTS: usize = 24;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn an_answer_no_place_holds_opens_the_reader_by_itself() {
    super::seeds(NAME, SEEDS, run);
}

/// The newest turn's answer: where its words are held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Said {
    /// `src/a.rs:N` — a note there.
    Placed,
    /// A file that isn't there, or no file at all — no place.
    Nowhere,
}

fn run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-read-self");
    let root = std::fs::canonicalize(scratch.path()).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    let text: String = (1..=10).map(|i| format!("let line_{i} = {i};\n")).collect();
    std::fs::write(root.join("src/a.rs"), text).unwrap();
    let mut sim = Sim::new(NAME, choices, LIMIT);
    sim.frames = Some((120, 30));
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    // (claude a stand-in: what it's sent, never a process)
    let (tx, _rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    ed.open(&root.join("src/a.rs")).unwrap();
    crate::chat::open(&mut ed);
    // (out of sight, the keys in the code — as the ask line leaves it once its message went)
    crate::chat::hide(&mut ed);
    ed.workspace.viewer.at = At::Code;
    let mut last = None;
    let input = |sim: &mut Sim, ed: &mut Editor| {
        let said = input(sim, ed, &mut last);
        // (the column in sight after the newest turn: its card never opens by itself — its answer is there)
        if ed.workspace.claude.chat.as_ref().is_some_and(|c| !c.hidden)
            && let Some((_, seen)) = last.as_mut()
        {
            *seen = true;
        }
        said
    };
    sim.walk(&mut ed, INPUTS, input, held, print);
    // The end: typing over, the searches landed — the newest answer's way taken
    let end = |_: &mut Sim, ed: &mut Editor| {
        if ed.mode == Mode::Insert {
            key(ed, crate::key::Code::Esc);
        }
        crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
        "typing over, a frame".to_string()
    };
    sim.walk(&mut ed, 1, end, held, print);
    let settled = |ed: &Editor| ed.workspace.claude.notes.answer.is_none();
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let (wanted, hidden) = (c.read_wanted, c.hidden);
    let card = ed.workspace.claude.notes.open.is_some();
    sim.at_end(|| match (last, settled(&ed), wanted, hidden, card) {
        (_, false, ..) => Ok(()),
        (_, true, true, ..) => Err("typing over, the reader still waits".into()),
        (Some((Said::Nowhere, false)), true, false, true, _) => {
            Err("an answer no place holds, the reader out of sight".into())
        }
        (Some((Said::Placed, false)), true, false, _, false) => {
            Err("an answer with a place, no card at it".into())
        }
        _ => Ok(()),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// A key as the loop has it: an event, every event's syncs after it.
fn key(ed: &mut Editor, code: crate::key::Code) {
    ed.handle_event(crate::event::Event::Key(crate::key::Key::plain(code)));
}

/// `last`: the newest turn's answer, and whether the conversation was hidden by you or in sight since.
fn input(sim: &mut Sim, ed: &mut Editor, last: &mut Option<(Said, bool)>) -> String {
    let c = &mut sim.choices;
    // (by arm: a turn's end 3, a frame 3, time 1, typing begun or ended 2, the reader hidden 1)
    match c.weighted(&[3, 3, 1, 2, 1]) {
        0 => {
            let text = match *c.pick(&[0, 1, 2]) {
                0 => "It is read in src/a.rs:3, once.".to_string(),
                1 => "It is read in src/gone.rs:3, once.".to_string(),
                _ => "Nothing reads it twice; that is all.".to_string(),
            };
            // (the second: hidden by you since — or the column in sight since, where no card opens by itself)
            *last = Some((if text.contains("src/a.rs:") { Said::Placed } else { Said::Nowhere }, false));
            let chat = ed.workspace.claude.chat.as_mut().unwrap();
            // (a message of yours: the last one's wait let go — `submit_as`'s)
            (chat.new_at, chat.read_wanted) = (None, false);
            chat.msgs.push(Msg::new(Role::User, "and?", ""));
            chat.follow.new_turn();
            chat.msgs.push(Msg::new(Role::Assistant, &text, ""));
            chat.news = Some(chat.msgs.len() - 1);
            crate::chatnotes::turn_over(ed, true);
            // (what the loop does after the event)
            crate::chatnotes::sync(ed);
            crate::chat::read_sync(ed);
            format!("a turn's end: {text:?}")
        }
        1 => {
            crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
            "a frame".into()
        }
        2 => {
            let ms = *c.pick(&[1, 30, 300]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        3 => match ed.mode {
            Mode::Insert => {
                key(ed, crate::key::Code::Esc);
                "typing ended".into()
            }
            _ => {
                key(ed, crate::key::Code::Char('i'));
                "typing begun".into()
            }
        },
        _ => {
            crate::chat::hide(ed);
            if let Some((_, hid)) = last.as_mut() {
                *hid = true;
            }
            "the reader hidden".into()
        }
    }
}

/// A reader waits only while it's out of sight and you type; it never takes the keys.
fn held(ed: &Editor) -> Result<(), String> {
    let Some(c) = ed.workspace.claude.chat.as_ref() else { return Err("the chat closed".into()) };
    if c.read_wanted && !c.hidden {
        return Err("waiting to open, in sight".into());
    }
    if c.read_wanted && ed.mode != Mode::Insert {
        return Err("waiting to open, nobody typing".into());
    }
    if ed.workspace.is_at(At::Chat) {
        return Err("the keys the chat's: it opened with them".into());
    }
    // (the conversation is the docked column — the user, 2026-11-03: never the ask line by itself)
    if !c.hidden && c.asking {
        return Err("opened as the ask line, not the column".into());
    }
    Ok(())
}

/// What a trace line ends with: the chat in sight or not, waiting, the mode, the card open, an answer's places waiting.
fn print(ed: &Editor) -> String {
    let c = ed.workspace.claude.chat.as_ref();
    let shown = c.is_some_and(|c| !c.hidden);
    let wanted = c.is_some_and(|c| c.read_wanted);
    let notes = &ed.workspace.claude.notes;
    let waits = if notes.answer.is_some() { " · places asked" } else { "" };
    format!("shown {shown} · wants {wanted} · {:?} · open {:?}{waits}", ed.mode, notes.open)
}
