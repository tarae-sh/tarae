//! A toast is news, never a pane with the keys (`toast.rs`), simulated: messages said (routine ones and errors), `C-w w`,
//! presses on the card and elsewhere, keys (`y`, `j`, `k`, `x`, `esc`, `q`), a session switch, frames, ticks and the
//! user taking their time — a toast's reading time running out as keys go by, an error read and let go by a key.
//!
//! After every step: a press on a card drawn opened `:messages` with the keys and let the toasts go; a card up asks for a wake while a
//! message on it runs (none waits forever on the screen), and none once it's gone; an error read past `ERROR_MIN` that a
//! key of the person's went by is fading.

use std::cell::RefCell;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::key::Key;

const SEEDS: &[u64] = &[0, 1, 2, 7, 10];

const INPUTS: usize = 60;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_toast_never_takes_a_key() {
    super::seeds("a_toast_never_takes_a_key", SEEDS, run);
}

/// What the scenario knows from its own steps: the two sessions, and what the last step should have left.
#[derive(Default)]
struct Model {
    sessions: (crate::event::SessionId, crate::event::SessionId),
    /// The last step pressed a card that was drawn, after message N was said: `:messages` should be up, the toasts
    /// up then gone (one said after it — a job landing — is news of its own).
    pressed: Option<u64>,
    /// The last step was a key past an error's `ERROR_MIN`: that error should be fading.
    read: Option<u64>,
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-toasts");
    let mut sim = Sim::new("a_toast_never_takes_a_key", choices, LIMIT);
    sim.frames = Some((100, 24));
    let mut ed = Editor::new(crate::config::Config::default());
    ed.docs[0].text = ropey::Rope::from_str("one\ntwo\n");
    let main = ed.session.id;
    let other = ed.session_open(dir.path()).unwrap();
    ed.session_switch(main);
    let model = RefCell::new(Model { sessions: (main, other), ..Model::default() });
    let check = |ed: &Editor| held(ed, &model.borrow());
    sim.walk(&mut ed, INPUTS, |sim, ed| step(sim, ed, &model), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn key(sim: &Sim, ed: &mut Editor, k: &str) {
    sim.batch(ed, crate::event::Event::Key(k.parse::<Key>().unwrap()));
}

fn press(sim: &Sim, ed: &mut Editor, x: usize, y: usize) {
    let kind = crate::event::MouseKind::Down;
    let press = crate::event::Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false };
    sim.batch(ed, crate::event::Event::Mouse(press));
}

fn step(sim: &mut Sim, ed: &mut Editor, model: &RefCell<Model>) -> String {
    let c = &mut sim.choices;
    let mut m = model.borrow_mut();
    (m.pressed, m.read) = (None, None);
    // (by arm: a message 3, an error 1, C-w w 2, a press on the card 1, a key 4, a press elsewhere 1, a switch 1, a
    // frame 1, a tick 2, time passing 3)
    match c.weighted(&[3, 1, 2, 1, 4, 1, 1, 1, 2, 3]) {
        0 => {
            let n = ed.said + 1;
            ed.set_status(format!("saved file {n}"));
            format!("message {n}")
        }
        1 => {
            ed.set_error("it broke");
            "an error".into()
        }
        2 => {
            key(sim, ed, "C-w");
            key(sim, ed, "w");
            "C-w w".into()
        }
        3 => {
            let Some((x, y, ..)) = ed.screen.toast_card.get().filter(|_| !ed.card_toasts().is_empty()) else {
                return "no card to press".into();
            };
            // (a prompt's press is its own — none opens in this scenario)
            m.pressed = (ed.prompt.is_none() && ed.picker.is_none()).then_some(ed.said);
            press(sim, ed, x + 2, y);
            "a press on the card".into()
        }
        4 => {
            let k = *c.pick(&["y", "j", "k", "x", "esc", "q"]);
            // (an error read: a person's key lets it go)
            m.read = (ed.toasts.iter())
                .find(|t| t.kind == crate::editor::ToastKind::Error && t.age() >= crate::editor::ERROR_MIN)
                .map(|t| t.id);
            key(sim, ed, k);
            format!("key {k}")
        }
        5 => {
            press(sim, ed, 1, 1);
            "a press on the code".into()
        }
        6 => {
            let (main, other) = m.sessions;
            let to = if ed.session.id == main { other } else { main };
            ed.session_switch(to);
            crate::term::render(ed, &mut Vec::new(), 100, 24).unwrap();
            format!("to session {to}")
        }
        7 => {
            crate::term::render(ed, &mut Vec::new(), 100, 24).unwrap();
            "a frame".into()
        }
        8 => {
            // (as the main loop's wake: its tick, then a frame)
            ed.schedule_tick();
            ed.focus_sync();
            crate::term::render(ed, &mut Vec::new(), 100, 24).unwrap();
            "a tick".into()
        }
        _ => {
            let ms = *c.pick(&[50, 120, 400, 5000, 70_000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

/// A press on it opened its home; a card that runs asks for a wake, none when gone; an error
/// read and passed by a key fades.
fn held(ed: &Editor, m: &Model) -> Result<(), String> {
    use crate::uistate::Region;
    let region = ed.keys_to().region;
    let list = Region::List(crate::pane::GroupKind::Messages);
    if let Some(said) = m.pressed {
        if region != list || ed.messages_pane.is_none() {
            return Err(format!("a press on the card: the keys at {region:?}, not :messages"));
        }
        if let Some(t) = ed.toasts.iter().find(|t| t.id <= said) {
            return Err(format!("a press opened :messages, yet toast#{} said before it stays", t.id));
        }
    }
    let running = ed.toasts.iter().any(|t| t.alive() && !t.waiting());
    match (ed.toast_wake(), running) {
        (None, true) => return Err("a toast's time runs, yet no wake is asked to end it".into()),
        (Some(w), false) if ed.toasts.is_empty() => return Err(format!("no toast, yet a wake in {w:?}")),
        _ => {}
    }
    if let Some(t) = m.read.and_then(|id| ed.toasts.iter().find(|t| t.id == id))
        && t.life > t.age() + Duration::from_millis(400)
    {
        return Err(format!("toast#{} read, a key went by — still up for {:?}", t.id, t.life - t.age()));
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let list = ed.messages_pane.is_some();
    format!("toasts {} · drawn {} · :messages {list}", ed.toasts.len(), ed.screen.toast_card.get().is_some())
}
