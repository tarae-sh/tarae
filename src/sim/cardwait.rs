//! A card takes the keys only once focused (P3 §3.8), simulated: keys (`j`, `k`, `y`, `esc`, `C-q`), light cards
//! coming between them — some asked by the key just before (the user's own, focused as they come), some rewritten
//! as they're up (asked again: a new `rev`) — a press on the card, a second session switched to and back (a card
//! parked, then up again), frames, and the user taking their time.
//!
//! The scenario keeps its own model of which card is focused — by id and `rev`, from what it did alone: `C-q` or a
//! press focuses the front (`C-q` again steps on to the next card waiting, else lets go — #544; `esc`, a switch, the
//! card's text changing let go), your own card coming to
//! the front is focused, and an answer hands the keys on only while going through them (the answered card not
//! yours) or to one of yours — and only at a frame that draws it: `y y` in one batch (a repeat, a yank typed on) never
//! answers the next card unseen (the review of 7b). Each `y` is judged by it — one that reaches a card the model
//! hasn't focused must leave it up (the code's yank, or refused and said on it), one that reaches the focused card
//! must answer it (light cards: `y` ends what never began, the card goes). After every step the editor's focus is
//! the model's, and a card standing whole frame after frame kept its top row (#561) — its text rewritten taller or
//! shorter as it's up: risen only to stand on the floor where it grew past it, never down; and over the chat's input
//! (the chat open beside, the keys taken there as `space l` takes them) only while the card has the keys — without them
//! never over the input nor its rule, its top unmoved.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::key::Key;
use crate::offer::{Offer, What};

/// (13: #566, a press on a frame that drew only another session's peek; 15: that model's break-once found it too)
const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

const INPUTS: usize = 60;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_card_takes_the_keys_only_once_focused() {
    super::seeds("a_card_takes_the_keys_only_once_focused", SEEDS, run);
}

/// What the scenario knows from its own steps: the card it focused (id, rev), the one an answer handed the keys on to
/// (focused at the next frame that draws it), the frames seen, the two sessions.
#[derive(Default)]
struct Model {
    focused: Option<(u64, u64)>,
    next: Option<(u64, u64)>,
    frames: u64,
    sessions: (crate::event::WorkspaceId, crate::event::WorkspaceId),
    /// The front card standing whole at the last check: the frame it was drawn in, its id, its top row, and whether it
    /// stood over the chat's input without the keys (held).
    drawn: Cell<Option<(u64, u64, usize, bool)>>,
}

impl Model {
    /// A frame drawn since: the card handed on, in front as it was, is focused.
    fn sync(&mut self, ed: &Editor) {
        if ed.frames != self.frames {
            self.frames = ed.frames;
            if self.next.is_some() && front(ed).map(|f| (f.0, f.1)) == self.next {
                self.focused = self.next.take();
            }
        }
    }
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-cardwait");
    let mut sim = Sim::new("a_card_takes_the_keys_only_once_focused", choices, LIMIT);
    sim.frames = Some((100, 24));
    let mut ed = Editor::new(crate::config::Config::default());
    // (the chat open beside: its input below the asks, where the keys go by `space l` — a stand-in claude behind it)
    ed.config.llm.command = "sh".into();
    ed.config.llm.args = vec!["-c".into(), "cat > /dev/null".into()];
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    let main = ed.workspace.id;
    let other = ed.workspace_open(dir.path()).unwrap();
    ed.workspace_switch(main);
    let model = RefCell::new(Model { sessions: (main, other), ..Model::default() });
    let wrong = Cell::new(None);
    let check = |ed: &Editor| held(ed, &model.borrow(), &wrong);
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &model, &wrong), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// The front card up: (id, rev, yours).
fn front(ed: &Editor) -> Option<(u64, u64, bool)> {
    ed.offers.first().map(|o| (o.id, o.rev, o.yours))
}

/// Key `k` — a batch of its own, a frame after it when one is due (`framed`), else the first of a batch the next key
/// ends — judged and followed by the model.
fn key(
    sim: &Sim,
    ed: &mut Editor,
    model: &RefCell<Model>,
    wrong: &Cell<Option<String>>,
    k: &str,
    framed: bool,
) {
    let before = front(ed);
    let session = ed.workspace.id;
    // (another card waiting, here behind it or in a session behind — a focused one's `C-q` steps on to it, #544)
    let others = !ed.c_q_line().is_empty();
    let event = crate::event::Event::Key(k.parse::<Key>().unwrap());
    match framed {
        true => sim.batch(ed, event),
        false => ed.handle_event(event),
    }
    let mut m = model.borrow_mut();
    let had = before.is_some_and(|f| m.focused == Some((f.0, f.1)));
    match k {
        "y" => {
            let Some(f) = before else { return };
            let answered = front(ed).map(|o| o.0) != Some(f.0);
            match (had, answered) {
                (false, true) => wrong.set(Some(format!("y answered card#{}, never focused or drawn", f.0))),
                (true, false) => wrong.set(Some(format!("y left card#{}, focused, up", f.0))),
                _ => {}
            }
            // (answered: on to the next only while going through them, or to one of yours — at the frame that draws
            // it)
            if answered {
                m.focused = None;
                m.next = front(ed).filter(|n| !f.2 || n.2).map(|n| (n.0, n.1));
            }
        }
        "esc" if had => m.focused = None,
        // (focused: on to the next card waiting — brought to the front, its session's too — none: let go; else to this
        // session's card, or — none here — the next up anywhere)
        "C-q" if had => {
            m.focused = front(ed).filter(|f| others && Some(f.0) != before.map(|b| b.0)).map(|f| (f.0, f.1))
        }
        "C-q" => {
            let here = before.is_some();
            m.focused = match front(ed) {
                Some(f) if here || ed.workspace.id != session => Some((f.0, f.1)),
                _ => None,
            };
        }
        _ => {}
    }
    m.sync(ed);
}

fn input(sim: &mut Sim, ed: &mut Editor, model: &RefCell<Model>, wrong: &Cell<Option<String>>) -> String {
    let said = step(sim, ed, model, wrong);
    model.borrow_mut().sync(ed);
    said
}

fn step(sim: &mut Sim, ed: &mut Editor, model: &RefCell<Model>, wrong: &Cell<Option<String>>) -> String {
    let c = &mut sim.choices;
    // (by arm: a key 6, a card 3, a card rewritten 1, a press 1, a session switch 1, a frame 1, time passing 1,
    // `y y` in one batch 2, the keys to the chat's input 1)
    match c.weighted(&[6, 3, 1, 1, 1, 1, 1, 2, 1]) {
        8 => {
            // (the keys to the chat's input, as `space l` takes them — without the claude it warms, which the simulation
            // doesn't model — only where no card has them, which every key there takes; a frame after it)
            if ed.card_has_keys() {
                return "the keys stay: the card has them".into();
            }
            ed.workspace.set_at(crate::workspace::Here::Chat);
            crate::term::render(ed, &mut Vec::new(), 100, 24).unwrap();
            model.borrow_mut().sync(ed);
            "the keys to the chat's input".into()
        }
        0 => {
            let k = *c.pick(&["j", "k", "y", "y", "y", "esc", "C-q", "C-q"]);
            key(sim, ed, model, wrong, k, true);
            format!("key {k}")
        }
        7 => {
            // (a repeat, or a yank typed on after an answer: both keys before any frame)
            key(sim, ed, model, wrong, "y", false);
            key(sim, ed, model, wrong, "y", true);
            "keys y y in one batch".into()
        }
        1 => {
            // (asked by the key just before — the user's own — or come from elsewhere)
            let yours = *c.pick(&[false, false, true]);
            let what =
                What::ReviewEnd { session: crate::event::WorkspaceId::HOME, started: crate::clock::now() };
            let was = ed.events.set_asked(yours.then_some(ed.input_seq));
            // (a title of its own: one with the same title would be merged into the card up)
            let title = format!("End review {}?", ed.offer_seq + 1);
            ed.push_offer(Offer::new(title, "2 files not viewed.", "", String::new(), what));
            ed.events.set_asked(was);
            let id = ed.offer_seq;
            // (yours, come to the front: focused as it came)
            if let Some(f) = front(ed).filter(|f| yours && f.0 == id) {
                model.borrow_mut().focused = Some((f.0, f.1));
            }
            format!("card#{id} comes{}", if yours { ", asked by the last key" } else { "" })
        }
        2 => {
            // (the front asked again from elsewhere: its text new, its `rev` up)
            let Some(title) = ed.offers.first().map(|o| o.title.clone()) else {
                return "nothing to rewrite".into();
            };
            let what =
                What::ReviewEnd { session: crate::event::WorkspaceId::HOME, started: crate::clock::now() };
            // (taller or shorter than it was, by turns — its top kept either way, #561; no choice taken for it, the
            // walk as it was)
            let text = match ed.offers[0].rev % 2 {
                0 => "3 files not viewed.\n\n  src/a.rs\n  src/b.rs\n  src/c.rs",
                _ => "3 files not viewed.",
            };
            ed.push_offer(Offer::new(title, text, "", String::new(), what));
            format!("card#{} rewritten, {} rows", ed.offers[0].id, text.lines().count())
        }
        3 => {
            // (a press where the last frame drew a card's stack — what that frame drew there takes it, as the editor
            // reads it (`mouse_under`): the stack's rows (its back cards', the air between, its own) focus the card in
            // front now, a back row's bringing that one forward; anything else there takes the keys from the card.
            // #566: the model took every press on the stack's rect for the card — one aimed from a frame that drew
            // only another session's peek (the card in front come since, undrawn) landed under that peek, on the code,
            // and the code took the keys)
            let Some((x, y, _, _)) = ed.screen.offer_card else { return "no card to press".into() };
            let (x, y) = (x + 2, y + 1);
            let under = ed.mouse_under(crate::event::MouseKind::Down, x, y);
            let on_card = under.surface == crate::mouse::Surface::Card;
            // (an answer is a key's to judge, not pressed here)
            if matches!(under.part, crate::mouse::Part::Answer(k) if on_card && !k.is_ascii_digit()) {
                return "no press: an answer there".into();
            }
            let back = matches!(under.part, crate::mouse::Part::Answer(k) if k.is_ascii_digit());
            let press = crate::event::Mouse {
                kind: crate::event::MouseKind::Down,
                x: x as u16,
                y: y as u16,
                alt: false,
                ctrl: false,
            };
            sim.batch(ed, crate::event::Event::Mouse(press));
            model.borrow_mut().focused = front(ed).filter(|_| on_card).map(|f| (f.0, f.1));
            match (on_card, back) {
                (true, true) => "a press on the card's back row".into(),
                (true, false) => "a press on the card".into(),
                (false, _) => "a press under the card's last frame, off it".into(),
            }
        }
        4 => {
            let (main, other) = model.borrow().sessions;
            let to = if ed.workspace.id == main { other } else { main };
            ed.workspace_switch(to);
            (model.borrow_mut().focused, model.borrow_mut().next) = (None, None);
            // (as the key that switches would: a frame after it)
            crate::term::render(ed, &mut Vec::new(), 100, 24).unwrap();
            format!("to session {to}")
        }
        5 => {
            crate::term::render(ed, &mut Vec::new(), 100, 24).unwrap();
            "a frame".into()
        }
        _ => {
            let ms = *c.pick(&[50, 300, 1200]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

/// No `y` judged wrong by the model; the editor's focus is the model's — the front card has the keys exactly when the
/// model focused it as it says now — and the keys' state says the one waiting otherwise.
fn held(ed: &Editor, m: &Model, wrong: &Cell<Option<String>>) -> Result<(), String> {
    if let Some(why) = wrong.take() {
        return Err(why);
    }
    let focused = front(ed).is_some_and(|f| m.focused == Some((f.0, f.1)));
    if ed.card_has_keys() != focused {
        return Err(format!("the card has the keys: {}, the model says {focused}", ed.card_has_keys()));
    }
    let waiting = ed.keys_to().waiting;
    if waiting.is_some() != (front(ed).is_some() && !focused) {
        return Err(format!("keys.waiting {waiting:?} — the model's focus {:?}", m.focused));
    }
    // (#561: the same card standing whole in a later frame kept its top — or rose to stand on the floor, grown past
    // it; and while the keys are in the chat's input, frame after frame, never moved and never over the input or its rule)
    // (a card covers the chat's input only while it has the keys: the input drawn and the card without them, held)
    let held = ed.screen.chat_input.get().is_some() && !ed.card_has_keys();
    let now = ed.screen.offer_top.map(|t| (ed.frames, t.0, t.1, held));
    let foot = ed.screen.offer_card.map(|r| r.1 + r.3 - 1);
    if let (Some((f0, id0, top0, held0)), Some((f1, id1, top1, held1))) = (m.drawn.get(), now)
        && id0 == id1
        && f1 != f0
    {
        let input = ed.screen.chat_input.get().map(|i| i.y);
        if held0 && held1 && (top1 != top0 || foot.zip(input).is_some_and(|(f, i)| f + 1 >= i)) {
            return Err(format!(
                "card#{id1} moved or covered the input without the keys: its top {top0} → {top1}, its foot {foot:?}, \
                 the input's first row {input:?}"
            ));
        }
        let floor = ed.screen.height.map(|h| h.saturating_sub(1));
        if top1 != top0 && (top1 > top0 || foot != floor) {
            return Err(format!(
                "card#{id1}'s top moved {top0} → {top1} (its foot {foot:?}, the floor {floor:?})"
            ));
        }
    }
    m.drawn.set(now);
    Ok(())
}

fn print(ed: &Editor) -> String {
    let parked = ed.workspace.parked.len() + ed.workspaces.iter().map(|l| l.parked.len()).sum::<usize>();
    format!("cards {} · parked {parked} · card has the keys {}", ed.offers.len(), ed.card_has_keys())
}
