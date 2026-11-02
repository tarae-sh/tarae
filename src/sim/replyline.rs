//! Replies in line, simulated (focus C1b): replies written on three notes — one on a file named like a key — while turns
//! start and end, at random, among a note closed, a card answered `y` or `n`, a frame and time passing; claude a stand-in
//! whose lines are kept. A turn's end sends the replies next in line as one message, or raises one card that holds them and
//! everything behind them. After every step: nothing refused ("couldn't reach claude"); one card at most; while a card
//! stands, nothing goes but what you send yourself; no card holds a reply whose note went; every reply is in a message once,
//! in the order written, or still in line, or on the card, or taken back with its note, and one taken back goes nowhere.

use std::cell::RefCell;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::event::Event;
use crate::llm::StreamEv;
use crate::notes::By;

const SEEDS: &[u64] = &[0, 1, 2, 3, 7];

const INPUTS: usize = 48;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn replies_in_line_go_together_under_their_card() {
    super::seeds("replies_in_line_go_together_under_their_card", SEEDS, run);
}

/// What the checks keep: each reply written (its number, its note, whether it was taken back), the replies sent in the
/// order they went, and whether the card stood through the step and the step sent nothing of yours.
#[derive(Default)]
struct Seen {
    written: Vec<(usize, u64, bool)>,
    sent: Vec<usize>,
    /// How many messages of yours were sent or queued.
    messages: usize,
    /// A card stood as the step began, and the step can't have let anything go: no answer, no message sent.
    held: bool,
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-reply-line");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::write(root.join("a.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    std::fs::write(root.join(".env"), "API_KEY=tarae-fake-token\n").unwrap();
    let mut sim = Sim::new("replies_in_line_go_together_under_their_card", choices, LIMIT);
    let mut ed = Editor::with_root(crate::config::Config::default(), root.clone());
    ed.allow_path = Some(root.join("sandbox.json"));
    let (tx, rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    ed.open(&root.join("a.rs")).unwrap();
    crate::chat::open(&mut ed);
    let notes = [
        crate::notes::add(&mut ed, &root.join("a.rs"), 0..1, By::Author, "the first"),
        crate::notes::add(&mut ed, &root.join("a.rs"), 1..2, By::Author, "the second"),
        crate::notes::add(&mut ed, &root.join(".env"), 0..1, By::Author, "on the key"),
    ];
    let seen = RefCell::new(Seen::default());
    sim.walk(
        &mut ed,
        INPUTS,
        |sim, ed| input(sim, ed, &notes, &seen),
        |ed| held(ed, &rx, &mut seen.borrow_mut()),
        print,
    );
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// The card asking about replies up.
fn card_up(ed: &Editor) -> bool {
    crate::reads::replies_ask(ed)
}

/// Note `id` is there.
fn there(ed: &Editor, id: u64) -> bool {
    ed.workspace.claude.notes.list.iter().any(|n| n.id == id)
}

/// A turn's event, as the main loop applies it.
fn stream(ed: &mut Editor, ev: StreamEv) {
    let g = ed.workspace.claude.chat.as_ref().map_or(0, crate::chat::Chat::generation);
    ed.handle_event(Event::Job(Box::new(move |ed: &mut Editor| crate::chat::stream_event(ed, g, ev))));
}

fn input(sim: &mut Sim, ed: &mut Editor, notes: &[u64; 3], seen: &RefCell<Seen>) -> String {
    let c = &mut sim.choices;
    let busy = ed.workspace.claude.chat.as_ref().is_some_and(crate::chat::Chat::busy);
    seen.borrow_mut().held = card_up(ed);
    // (by arm: a reply 5, a message of yours 2, the turn's end 3, a card's answer 2, a note closed 1, a frame 1, time 1)
    match c.weighted(&[5, 2, 3, 2, 1, 1, 1]) {
        0 => {
            let left: Vec<u64> = notes.iter().copied().filter(|&n| there(ed, n)).collect();
            if left.is_empty() {
                return "(no note left)".into();
            }
            let id = *c.pick(&left);
            let n = seen.borrow().written.len() + 1;
            crate::notes::submit(ed, Some(id), &format!("r{n}"));
            seen.borrow_mut().written.push((n, id, false));
            format!("a reply r{n} on #{id}")
        }
        1 => {
            seen.borrow_mut().held = false;
            seen.borrow_mut().messages += 1;
            let n = seen.borrow().messages;
            let Some(chat) = ed.workspace.claude.chat.as_mut() else { return "(no chat)".into() };
            chat.input = format!("m{n}").into();
            crate::chat::send(ed);
            format!("a message m{n}")
        }
        2 if busy => {
            stream(ed, StreamEv::Done(Ok("ok".into())));
            "the turn ends".into()
        }
        2 => "(no turn to end)".into(),
        3 if card_up(ed) => {
            seen.borrow_mut().held = false;
            crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
            let front = ed.offers.first().map(|o| (o.id, o.rev));
            ed.card_focus = front;
            let k = *c.pick(&["y", "n"]);
            ed.offer_key(k.parse().unwrap());
            format!("the card answered {k}")
        }
        3 => "(no card)".into(),
        4 => {
            let id = *c.pick(notes);
            if !there(ed, id) {
                return "(its note is gone)".into();
            }
            crate::notes::close(ed, id);
            // (the event that takes its reply off the line and off a card: what the card held may go now)
            crate::notes::queue_sync(ed);
            let mut seen = seen.borrow_mut();
            seen.held = false;
            let sent = seen.sent.clone();
            for r in seen.written.iter_mut().filter(|r| r.1 == id && !sent.contains(&r.0)) {
                r.2 = true;
            }
            format!("note #{id} closed")
        }
        5 => {
            crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
            "a frame".into()
        }
        6 => {
            let ms = *c.pick(&[50, 300, 2000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => unreachable!("one arm a weight"),
    }
}

/// The replies a message carries as its own: what's after its last note, by their numbers.
fn carried(content: &str) -> Vec<usize> {
    let Some((_, words)) = content.rsplit_once("</note>") else { return Vec::new() };
    let number = |w: &str| w.strip_prefix('r')?.parse().ok();
    words.split(|c: char| !c.is_alphanumeric()).filter_map(number).collect()
}

fn held(ed: &Editor, rx: &Receiver<String>, seen: &mut Seen) -> Result<(), String> {
    let lines: Vec<String> = rx.try_iter().collect();
    let mut went = 0;
    for l in &lines {
        let v: serde_json::Value = serde_json::from_str(l).map_err(|e| e.to_string())?;
        let Some(content) = v["message"]["content"].as_str() else { continue };
        went += 1;
        seen.sent.extend(carried(content));
    }
    if let Some(t) = ed.toasts.iter().find(|t| t.text.contains("couldn't reach claude")) {
        return Err(format!("refused: {}", t.text));
    }
    if crate::reads::replies_carded(ed).is_some_and(|r| r.iter().any(|r| !there(ed, r.id))) {
        return Err("a card asks about a reply whose note is gone".into());
    }
    if seen.held && card_up(ed) && went > 0 {
        return Err(format!("{went} went past the card: {lines:?}"));
    }
    let cards = ed.offers.iter().filter(|o| o.title == crate::reads::POINTED_TITLE).count();
    if cards > 1 {
        return Err(format!("{cards} cards at once"));
    }
    if !seen.sent.is_sorted() {
        return Err(format!("out of the order written: {:?}", seen.sent));
    }
    let mut once = seen.sent.clone();
    once.dedup();
    if once.len() != seen.sent.len() {
        return Err(format!("sent twice: {:?}", seen.sent));
    }
    let held_back = crate::reads::replies_held(ed);
    let waits = |n: usize| {
        let text = format!("r{n}");
        let in_line = |l: &crate::chat::Later| matches!(l, crate::chat::Later::Reply { reply, .. } if reply.text == text);
        let mut line = ed.workspace.claude.chat.iter().flat_map(|c| &c.sends);
        line.any(in_line) || held_back.contains(&text)
    };
    for &(n, _, gone) in &seen.written {
        match (gone, seen.sent.contains(&n)) {
            (true, true) => return Err(format!("r{n} went, its note closed")),
            (false, false) if !waits(n) => return Err(format!("r{n} neither sent nor waiting")),
            _ => {}
        }
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let Some(c) = ed.workspace.claude.chat.as_ref() else { return "no chat".into() };
    format!(
        "{}, {} in line{}{}",
        if c.busy() { "working" } else { "idle" },
        c.sends.len(),
        if c.queued.is_some() { ", a message queued" } else { "" },
        if card_up(ed) { ", the card up" } else { "" }
    )
}
