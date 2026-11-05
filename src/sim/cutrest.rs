//! The mouse's cards wait for the pointer to rest (#391, `hoverrest::bring`), simulated over a list of cut names (the
//! side panel, too narrow for them) and flags at the code's line ends (#388's `¶`): the pointer crossing them, onto
//! blanks and onto a card, time passing by the rest's own scale, your keys and the wheel between, frames. After every
//! step: a card comes only `REST` after the pointer came onto its target, with no key or wheel since — or at once in a
//! card's place, the pointer moving from one onto another of either kind (the hand-off); a card stands only while the
//! pointer's on it; one at a time; none waits while one's up. Once quiet, the target the pointer has rested on shows
//! its card, and no timer is left armed.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::event::{Event, Mouse, MouseKind};
use crate::hoverrest::REST;
use crate::whole::Cut;

const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 80;

const LIMIT: Duration = crate::test_results::PATIENCE;

const NAME: &str = "the_mouses_cards_wait_for_the_rest_and_hand_off_at_once";

const W: usize = 100;
const H: usize = 20;

#[test]
fn the_mouses_cards_wait_for_the_rest_and_hand_off_at_once() {
    super::seeds(NAME, SEEDS, run);
}

/// What a card the mouse brings is for.
#[derive(Clone, Debug, PartialEq)]
enum Target {
    Cut(Cut),
    Flag(u64),
}

#[derive(Default)]
struct World {
    /// Where the pointer is.
    at: Option<(usize, usize)>,
    /// The target the pointer came onto and when — none since a key or the wheel, until it moves again.
    onto: Option<(Target, Instant)>,
    /// The card up after the last step.
    up: Option<Target>,
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let dir = crate::scratch::Scratch::new("sim-cutrest");
    let root = std::fs::canonicalize(dir.as_ref()).unwrap();
    let file = root.join("a.rs");
    std::fs::write(&file, (1..=10).map(|i| format!("let line_{i} = {i};\n")).collect::<String>()).unwrap();
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root;
    ed.open(&file).unwrap();
    // (flags on every other line, an answer each: a `¶` at each of those lines' ends)
    for (n, line) in (1..).zip([1, 3, 5, 7]) {
        let origin = crate::chatnotes::Origin {
            n,
            at: 0,
            turn: n as usize - 1,
            name: "a.rs".into(),
            said: line,
            text: None,
        };
        let words = (format!("question {n}?").into(), format!("Line {} is read here.", line + 1));
        crate::flags::add(&mut ed, (&file, line..line + 1), origin, None, words);
    }
    let files: Vec<String> = (0..6)
        .map(|i| format!("{i}_a_long_file_name_that_never_fits_in_the_panel_number_{i}.rs"))
        .chain(["short.rs".to_string(), "b.rs".to_string()])
        .collect();
    ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::with_files("/r".into(), files));
    ed.card_rest = true;
    crate::term::render(&mut ed, &mut Vec::new(), W as u16, H as u16).unwrap();
    assert!(!ed.screen.flag_ends.borrow().is_empty(), "the flags drawn");
    let world = RefCell::new(World::default());
    let check = |ed: &Editor| held(ed, &mut world.borrow_mut());
    sim.walk(&mut ed, INPUTS, |sim, ed| step(sim, ed, &mut world.borrow_mut()), check, print);
    let w = world.borrow();
    let over = w.at.and_then(|(x, y)| under(&ed, x, y));
    if let Some((t, _)) = w.onto.as_ref().filter(|o| Some(&o.0) == over.as_ref()) {
        assert_eq!(up(&ed).as_ref(), Some(t), "rested on {}, quiet, and no card", name(t));
    }
    assert!(!ed.hover_rest.armed(), "a timer left armed once all's quiet");
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    drop(dir);
    trace
}

/// What's under (`x`, `y`) that a card is for, as the last frame drew it (the card up, on it).
fn under(ed: &Editor, x: usize, y: usize) -> Option<Target> {
    let cut = crate::whole::under(ed, x, y).map(Target::Cut);
    cut.or_else(|| crate::flags::over_at(ed, x, y).map(|h| Target::Flag(h.id)))
}

/// The card up.
fn up(ed: &Editor) -> Option<Target> {
    let cut = ed.cut_hover.clone().map(Target::Cut);
    cut.or(ed.flag_hover.map(|h| Target::Flag(h.id)))
}

/// The pointer to (`x`, `y`): onto a target it wasn't on, its rest from now; off them all, none.
fn hover(ed: &mut Editor, w: &mut World, x: usize, y: usize) {
    let over = under(ed, x, y);
    w.onto = match (over, w.onto.take()) {
        (Some(t), Some(o)) if o.0 == t => Some(o),
        (Some(t), _) => Some((t, crate::clock::now())),
        (None, _) => None,
    };
    w.at = Some((x, y));
    ed.handle_event(Event::Hover(x as u16, y as u16));
}

fn step(sim: &mut Sim, ed: &mut Editor, w: &mut World) -> String {
    let c = &mut sim.choices;
    // (by arm: onto a cut row 3, onto a flag 3, anywhere 2, onto the card 1, time 4, a key 1, the wheel 1, a frame 3)
    match c.weighted(&[3, 3, 2, 1, 4, 1, 1, 3]) {
        0 => {
            let rows: Vec<(usize, usize, usize)> = (ed.screen.cut_spots.borrow().iter())
                .filter(|s| matches!(s.3, Cut::Text(_)))
                .map(|s| (s.0, s.1, s.2))
                .collect();
            if rows.is_empty() {
                return "(no cut row drawn)".into();
            }
            let &(y, a, b) = c.pick(&rows);
            let x = a + c.below(b - a);
            hover(ed, w, x, y);
            format!("onto a cut row at {x},{y}")
        }
        1 => {
            let ends: Vec<(usize, usize)> = ed.screen.flag_ends.borrow().iter().map(|e| (e.1, e.0)).collect();
            if ends.is_empty() {
                return "(no flag drawn)".into();
            }
            let &(x, y) = c.pick(&ends);
            hover(ed, w, x, y);
            format!("onto a flag at {x},{y}")
        }
        2 => {
            let (x, y) = (c.below(W), c.below(H));
            hover(ed, w, x, y);
            format!("the pointer at {x},{y}")
        }
        3 => {
            let cut = ed.screen.cut_card.borrow().as_ref().map(|c| c.rect);
            let Some((x, y, cw, ch)) = cut.or(ed.screen.flag_card.get()) else {
                return "(no card to go onto)".into();
            };
            let (x, y) = (x + c.below(cw), y + c.below(ch));
            hover(ed, w, x, y);
            format!("onto the card at {x},{y}")
        }
        4 => {
            let ms = *c.pick(&[100, 300, 450, 600, 2000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms}ms pass")
        }
        5 => {
            let k = *c.pick(&["j", "k", "esc"]);
            w.onto = None;
            ed.handle_event(Event::Key(k.parse().unwrap()));
            format!("key {k}")
        }
        6 => {
            let Some((x, y)) = w.at else { return "(no pointer for the wheel)".into() };
            let kind = *c.pick(&[MouseKind::ScrollDown, MouseKind::ScrollUp]);
            w.onto = None;
            ed.handle_event(Event::Mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false }));
            format!("the wheel at {x},{y}")
        }
        _ => {
            crate::term::render(ed, &mut Vec::new(), W as u16, H as u16).unwrap();
            "a frame".into()
        }
    }
}

/// The rules, after every step.
fn held(ed: &Editor, w: &mut World) -> Result<(), String> {
    if ed.cut_hover.is_some() && ed.flag_hover.is_some() {
        return Err("two cards up at once".into());
    }
    let up = up(ed);
    let was = std::mem::replace(&mut w.up, up.clone());
    if up.is_some() && (ed.cut_wait.is_some() || ed.flag_wait.is_some()) {
        return Err("a card waits while one is up".into());
    }
    let over = w.at.and_then(|(x, y)| under(ed, x, y));
    if let Some(t) = up.as_ref().filter(|t| Some(*t) != over.as_ref()) {
        return Err(format!("{}'s card up, the pointer off it", name(t)));
    }
    match (was, up.as_ref()) {
        // (come with none up: the pointer on it `REST`, no key or wheel since)
        (None, Some(t)) => match &w.onto {
            Some((o, since)) if o == t && crate::clock::now() >= *since + REST => Ok(()),
            Some((o, since)) if o == t => Err(format!(
                "{}'s card came {:?} after the pointer did",
                name(t),
                crate::clock::now() - *since
            )),
            _ => Err(format!("{}'s card came with no rest on it since a key or the wheel", name(t))),
        },
        // (one up, the pointer onto another, of either kind: its at once)
        (Some(_), None) if over.is_some() => Err("the card gone, the pointer onto another target".into()),
        _ => Ok(()),
    }
}

fn name(t: &Target) -> String {
    match t {
        Target::Cut(Cut::Text(w)) => w.text.chars().take(12).collect(),
        Target::Cut(c) => format!("{c:?}"),
        Target::Flag(id) => format!("flag {id}"),
    }
}

fn print(ed: &Editor) -> String {
    let up = up(ed).as_ref().map(name);
    let cut = ed.cut_wait.as_ref().map(|w| name(&Target::Cut(w.0.clone())));
    let waits = cut.or(ed.flag_wait.map(|w| name(&Target::Flag(w.0.id))));
    format!("up {up:?} waits {waits:?}")
}
