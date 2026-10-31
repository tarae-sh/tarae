//! Focus A1b, simulated: a conversation's answers kept at the code and brought back with it. Turns end with answers
//! naming lines of `src/a.rs` and `src/b.rs` (notes once their names are found on a worker after a frame, the
//! conversation then kept again with its places on a job — `chat::keep_places`), frames, time and `:chat-resume` of the
//! same conversation (its store read on a job, `chatnotes::restore` from its rows), a note closed or turned a thread by
//! a reply (kept again on a job, `chatnotes::kept_sync`; one always left, so the store keeps a place), a note closed with
//! the chat closed (kept as the conversation stood then, `Notes.left`), in any order — a resume may read the store before
//! a keep lands. After every step: every chat note is of an answer the conversation's rows
//! hold (its question where its `Origin` says), none of one answer twice on a line, the newest's number the highest.
//! At the end, nothing racing: two answers and a note closed, then the store holds exactly the notes that are there, the
//! close with it, and a resume brings back exactly what it keeps: the places it writes again are the ones read; and the
//! chat, its older answers folded to stubs, still has every word of every answer in its document.

use std::path::Path;
use std::time::Duration;

use super::{Choices, Sim};
use crate::chat::{Msg, Role};
use crate::editor::Editor;
use crate::notes::{By, Entry};
use serde_json::Value;

const NAME: &str = "a_conversations_answers_come_back_as_it_keeps_them";

const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5];

const INPUTS: usize = 24;

const LIMIT: Duration = crate::test_results::PATIENCE;

const ID: &str = "conv-1";

#[test]
fn a_conversations_answers_come_back_as_it_keeps_them() {
    super::seeds(NAME, SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-answers-kept");
    let root = std::fs::canonicalize(scratch.path()).unwrap();
    for f in ["src/a.rs", "src/b.rs"] {
        std::fs::create_dir_all(root.join(f).parent().unwrap()).unwrap();
        let text: String = (1..=10).map(|i| format!("let line_{i} = {i};\n")).collect();
        std::fs::write(root.join(f), text).unwrap();
    }
    let mut sim = Sim::new(NAME, choices, LIMIT);
    sim.frames = Some((120, 30));
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    ed.chats_dir = Some(scratch.join("chats"));
    // (claude a stand-in: what it's sent, never a process)
    let (tx, _rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    crate::chat::open(&mut ed);
    ed.workspace.claude.chat.as_mut().unwrap().conversation = Some(ID.into());
    let mut asked = 0;
    let input = |sim: &mut Sim, ed: &mut Editor| input(sim, ed, &mut asked);
    let print = |ed: &Editor| print(ed, &root);
    sim.walk(&mut ed, INPUTS, input, held, print);
    // The end, nothing racing: two more answers and a note closed — the store then holds what's there, the close with it
    // (a resume above may have left it newer than the rows: these keeps overwrite it)
    for k in 0..3 {
        let last = |_: &mut Sim, ed: &mut Editor| match k {
            2 => closed(ed),
            _ => {
                let said = turn(ed, &mut asked, 2, 5);
                crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
                said
            }
        };
        sim.walk(&mut ed, 1, last, held, print);
    }
    let dir = ed.chats_dir.clone().unwrap();
    let kept =
        |root: &Path| crate::chats::load(&dir, root).into_iter().find(|s| s.id == ID).and_then(|s| s.places);
    let before = kept(&root);
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let here = crate::chatnotes::places_json(&ed.workspace.claude.notes, &c.msgs);
    let (notes, store) = (where_they_are(&here), where_they_are(&before));
    sim.at_end(|| match notes == store {
        true => Ok(()),
        false => Err(format!("the notes are at {notes:?}, the store keeps {store:?}")),
    });
    // A resume brings back what the store keeps — and keeps it as it read it
    crate::chat::resume(&mut ed, ID);
    sim.walk(&mut ed, 0, |_, _| String::new(), held, print);
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let now = crate::chatnotes::places_json(&ed.workspace.claude.notes, &c.msgs);
    sim.at_end(|| match now == before {
        true if before.is_none() => Err("notes were placed, the store keeps no place".into()),
        true => Ok(()),
        false => Err(format!("the store kept {before:?}, a resume brought back {now:?}")),
    });
    // The chat folds the older answers (focus A1b step 3): every word of every answer is still in its document
    crate::term::render(&mut ed, &mut Vec::new(), 120, 30).unwrap();
    // (an older answer folded at all: else the words below hold for nothing folded)
    let laid = ed.screen.chat_laid.borrow();
    let stubs = laid.iter().flat_map(|(_, rows)| rows.iter()).filter(|r| r.stub.is_some()).count();
    drop(laid);
    sim.at_end(|| match stubs {
        0 => Err("no older answer folded, two answers with notes kept".into()),
        _ => Ok(()),
    });
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let doc =
        c.transcript.as_ref().and_then(|t| t.pane.doc_in()).map_or(String::new(), |d| d.text.to_string());
    let answers = c.msgs.iter().filter(|m| m.role == Role::Assistant);
    let lost: Vec<String> = answers
        .flat_map(|m| m.text.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .filter(|w| !doc.contains(w.as_str()))
        .collect();
    sim.at_end(|| match lost.is_empty() {
        true => Ok(()),
        false => Err(format!("words gone from the transcript: {lost:?}")),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// `places_json`'s places as (answer's time, name, line, file) — their line texts the store's alone (`chats::texts_read`).
fn where_they_are(places: &Option<Value>) -> Vec<(Value, Value, Value, Value)> {
    let answers = places.as_ref().and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let at =
        |a: &'_ Value, p: &Value| (a["at"].clone(), p["name"].clone(), p["said"].clone(), p["path"].clone());
    answers.iter().flat_map(|a| a["places"].as_array().into_iter().flatten().map(|p| at(a, p))).collect()
}

/// A turn's end: question `asked` + 1, its answer naming `src/a.rs:a` and, past 0, `src/b.rs:b`.
fn turn(ed: &mut Editor, asked: &mut usize, a: usize, b: usize) -> String {
    *asked += 1;
    let text = match b {
        0 => format!("It is read in src/a.rs:{a}."),
        _ => format!("It is read in src/a.rs:{a}; src/b.rs:{b} checks it.\n\nNothing else."),
    };
    let chat = ed.workspace.claude.chat.as_mut().unwrap();
    chat.msgs.push(Msg::new(Role::User, format!("question {asked}"), ""));
    chat.follow.new_turn();
    chat.msgs.push(Msg::new(Role::Assistant, &text, ""));
    crate::chatnotes::turn_over(ed, true);
    format!("question {asked}: {text:?}")
}

/// The first bare note closed, then what the loop does after the key.
fn closed(ed: &mut Editor) -> String {
    let first = ed.workspace.claude.notes.list.iter().find(|n| crate::notes::bare_chat(n)).map(|n| n.id);
    let Some(id) = first else { return "no note to close".into() };
    crate::notes::close(ed, id);
    crate::chatnotes::sync(ed);
    format!("note {id} closed")
}

fn input(sim: &mut Sim, ed: &mut Editor, asked: &mut usize) -> String {
    let c = &mut sim.choices;
    // (by arm: a turn's end 3, a frame 3, a resume 2, time 1, a note closed or answered 2, the chat closed 1)
    match c.weighted(&[3, 3, 2, 1, 2, 1]) {
        0 => {
            let (a, b) = (*c.pick(&[2, 5]), *c.pick(&[0, 3, 5]));
            turn(ed, asked, a, b)
        }
        1 => {
            crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
            "a frame".into()
        }
        2 => {
            crate::chat::resume(ed, ID);
            "resumed".into()
        }
        3 => {
            let ms = *c.pick(&[1, 30, 300]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        // (kept as it stood then, on a job racing the resume that opens the chat again)
        5 => {
            crate::chat::close(ed);
            let said = closed(ed);
            crate::chat::resume(ed, ID);
            ed.workspace.claude.chat.as_mut().unwrap().conversation = Some(ID.into());
            format!("the chat closed, {said}, resumed")
        }
        _ => {
            let notes = ed.workspace.claude.notes.list.iter().filter(|n| crate::notes::bare_chat(n));
            let bare: Vec<u64> = notes.map(|n| n.id).collect();
            if bare.len() < 2 {
                return "no note to spare".into();
            }
            let (id, reply) = (*c.pick(&bare), *c.pick(&[false, true]));
            match reply {
                true => {
                    let n = crate::notes::find_mut(ed, id).expect("just listed");
                    n.thread.push(Entry { by: By::You, text: "why?".into(), draft: false });
                }
                false => crate::notes::close(ed, id),
            }
            // (what the loop does after the key)
            crate::chatnotes::sync(ed);
            format!("note {id} {}", if reply { "answered" } else { "closed" })
        }
    }
}

/// Every chat note is of an answer the conversation's rows hold: its question the `Origin.turn`th message of yours,
/// whose first line is the note's `asked`; one answer's notes on distinct lines; `Notes.answers` the highest number.
fn held(ed: &Editor) -> Result<(), String> {
    let notes = &ed.workspace.claude.notes;
    let Some(c) = ed.workspace.claude.chat.as_ref() else { return Err("the chat closed".into()) };
    let users: Vec<&Msg> = c.msgs.iter().filter(|m| m.role == Role::User).collect();
    let mut seen = Vec::new();
    for n in notes.list.iter().filter(|n| crate::notes::bare_chat(n)) {
        let Some(o) = &n.answer else {
            return Err(format!("a chat note of no answer at line {}", n.lines.start));
        };
        let q = users.get(o.turn).and_then(|m| m.text.lines().next());
        if q != n.asked.as_deref() {
            return Err(format!(
                "answer {} asked {:?}, the rows' question {} is {q:?}",
                o.n, n.asked, o.turn
            ));
        }
        if o.n > notes.answers {
            return Err(format!("answer {} past the newest number {}", o.n, notes.answers));
        }
        let key = (o.n, n.path.clone(), n.lines.start);
        if seen.contains(&key) {
            return Err(format!("answer {} twice at line {}", o.n, n.lines.start));
        }
        seen.push(key);
    }
    Ok(())
}

/// What a trace line ends with: the chat notes as (answer, file, line), the open card's, whether an answer waits.
fn print(ed: &Editor, root: &Path) -> String {
    let notes = &ed.workspace.claude.notes;
    let rel = |p: &Path| p.strip_prefix(root).unwrap_or(p).display().to_string();
    let said: Vec<String> = (notes.list.iter())
        .filter_map(|n| Some(format!("{}:{}:{}", n.answer.as_ref()?.n, rel(&n.path), n.lines.start + 1)))
        .collect();
    let rows = ed.workspace.claude.chat.as_ref().map_or(0, |c| c.msgs.len());
    let waits = if notes.answer.is_some() { " · waits" } else { "" };
    format!("{rows} rows · {said:?} · open {:?}{waits}", notes.open)
}
