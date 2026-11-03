//! "Drawn" means presented (the remote plan's §4), simulated: an interface that presents late — tarae draws frames for
//! it, it presents them when it gets to them (`interface::Facts::presents_late`), and each key it sends says the last
//! frame it presented (`Event::Input`'s `seen`). Weighty cards and light ones come (stacked, none yours), frames are
//! drawn, the interface presents the newest now and then, and keys come with what it had presented: `y`, `y y` in one
//! batch, `C-q`, `esc`, the user taking their time.
//!
//! The scenario keeps its own reading of the frames: from which frame the front card has been drawn as it is (what
//! each frame drew of it), and the frame the keys were handed on in (an answered card's next, at the first frame that
//! draws it). Each `y` is judged by it: one that answers a weighty card answers only a card drawn since a frame the
//! interface had presented, one that answers a card the keys were handed to only once that frame was presented —
//! else it's refused, said on the card as typed before it was in sight; one on a focused card drawn since a frame it
//! presented answers it. The editor never counts a frame the interface didn't present.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::event::Event;
use crate::key::Key;
use crate::offer::{Offer, What};

const SEEDS: &[u64] = &[0, 1, 2, 7, 10];

const INPUTS: usize = 60;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_card_is_answered_only_from_a_frame_the_interface_presented() {
    super::seeds("a_card_is_answered_only_from_a_frame_the_interface_presented", SEEDS, run);
}

type Card = (u64, u64);

/// What the scenario knows from its own steps and the frames it saw drawn.
#[derive(Default)]
struct Model {
    /// The last frame the interface presented — what its keys say.
    shown: u64,
    /// The last frame read.
    frames: u64,
    /// The front card as the frames drew it, and the first of them.
    since: Option<(Card, u64)>,
    /// The card an answer handed the keys on to, not drawn yet · the one they reached, and the frame that drew it.
    next: Option<Card>,
    handed: Option<(Card, u64)>,
}

impl Model {
    /// The frames drawn since the last look: what the newest drew of the front card.
    fn sync(&mut self, ed: &Editor) {
        if ed.frames == self.frames {
            return;
        }
        self.frames = ed.frames;
        let front = front(ed).map(|f| (f.0, f.1)).filter(|_| ed.screen.offer_card.is_some());
        self.since = match (front, self.since) {
            (Some(f), Some((s, at))) if s == f => Some((s, at)),
            (Some(f), _) => Some((f, ed.frames)),
            (None, _) => None,
        };
        if let Some(n) = self.next.take()
            && front == Some(n)
        {
            self.handed = Some((n, ed.frames));
        }
    }

    /// Whether card `c` stands in a frame the interface presented: drawn as it is since one it presented, and — when
    /// the keys were handed to it — that frame presented too.
    fn presented(&self, c: Card, weighty: bool) -> bool {
        let drawn = self.since.is_some_and(|(s, at)| s == c && at <= self.shown);
        let handed = self.handed.is_none_or(|(h, at)| h != c || at <= self.shown);
        (!weighty || drawn) && handed
    }

    /// Whether card `c` is drawn, but in a frame the interface hasn't presented: weighty and drawn since such a frame,
    /// or handed the keys in one — what's refused as typed before it was in sight.
    fn drawn_unseen(&self, c: Card, weighty: bool) -> bool {
        let drawn = weighty && self.since.is_some_and(|(s, at)| s == c && at > self.shown);
        drawn || self.handed.is_some_and(|(h, at)| h == c && at > self.shown)
    }
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("a_card_is_answered_only_from_a_frame_the_interface_presented", choices, LIMIT);
    sim.frames = Some((100, 30));
    let mut ed = Editor::new(crate::config::Config::default());
    ed.interface.presents_late = true;
    let model = RefCell::new(Model::default());
    let wrong = Cell::new(None);
    let check = |ed: &Editor| {
        model.borrow_mut().sync(ed);
        held(ed, &model.borrow(), &wrong)
    };
    sim.walk(&mut ed, INPUTS, |sim, ed| step(sim, ed, &model, &wrong), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// The front card: (id, rev, weighty).
fn front(ed: &Editor) -> Option<(u64, u64, bool)> {
    ed.offers.first().map(|o| (o.id, o.rev, o.weighty()))
}

/// Key `k` as the interface sends it — with the last frame it presented — a batch of its own, a frame after it when
/// one is due (`framed`), else the first of a batch the next key ends; a `y` judged by the model.
fn key(
    sim: &Sim,
    ed: &mut Editor,
    model: &RefCell<Model>,
    wrong: &Cell<Option<String>>,
    k: &str,
    framed: bool,
) {
    model.borrow_mut().sync(ed);
    let (before, had) = (front(ed), ed.card_has_keys());
    let seen = model.borrow().shown;
    let event = Event::Input { seen, seq: 0, event: Box::new(Event::Key(k.parse::<Key>().unwrap())) };
    match framed {
        true => sim.batch(ed, event),
        false => ed.handle_event(event),
    }
    let mut m = model.borrow_mut();
    let Some(f) = before.filter(|_| k == "y") else { return };
    let answered = front(ed).map(|o| o.0) != Some(f.0);
    let stands = m.presented((f.0, f.1), f.2);
    let said = ed.offer_said_now().unwrap_or_default().to_string();
    match (answered, had && stands) {
        (true, _) if !stands => wrong.set(Some(format!(
            "y answered card#{} (weighty {}) before the frame that drew it was presented — drawn since {:?}, handed \
             {:?}, the interface had presented frame {seen}",
            f.0, f.2, m.since, m.handed
        ))),
        (false, true) => wrong.set(Some(format!("y left card#{}, focused and presented, up — {said:?}", f.0))),
        (false, false) if had && m.drawn_unseen((f.0, f.1), f.2) && !said.contains("came before this card was in sight") => {
            wrong.set(Some(format!("y on card#{}, drawn but not presented yet, refused unsaid: {said:?}", f.0)))
        }
        _ => {}
    }
    // (answered: the keys on to the next — going through them, none yours — at the frame that draws it)
    if answered {
        m.next = front(ed).filter(|_| had).map(|n| (n.0, n.1));
    }
}

fn step(sim: &mut Sim, ed: &mut Editor, model: &RefCell<Model>, wrong: &Cell<Option<String>>) -> String {
    let c = &mut sim.choices;
    // (by arm: a card 3, a frame drawn 2, the interface presenting the newest 2, `y` 4, `C-q` 2, `esc` 1, `y y` in
    // one batch 1, time passing 1)
    match c.weighted(&[3, 2, 2, 4, 2, 1, 1, 1]) {
        0 => {
            let weighty = *c.pick(&[true, true, false]);
            let what = match weighty {
                // (weighty, and its `y` brings back no conversation: nothing starts)
                true => What::RecoverChats(Vec::new()),
                false => {
                    What::ReviewEnd { session: crate::event::WorkspaceId::HOME, started: crate::clock::now() }
                }
            };
            // (a title of its own: one with the same title would be merged into the card up)
            let title = format!("Card {}?", ed.offer_seq + 1);
            ed.push_offer(Offer::new(title, "One line of what it asks.", "", String::new(), what));
            format!("card#{} comes{}", ed.offer_seq, if weighty { ", weighty" } else { "" })
        }
        1 => {
            crate::term::render(ed, &mut Vec::new(), 100, 30).unwrap();
            format!("frame {} drawn", ed.frames)
        }
        2 => {
            model.borrow_mut().shown = ed.frames;
            format!("the interface presents frame {}", ed.frames)
        }
        3 => {
            key(sim, ed, model, wrong, "y", true);
            "key y".into()
        }
        4 => {
            key(sim, ed, model, wrong, "C-q", true);
            "key C-q".into()
        }
        5 => {
            key(sim, ed, model, wrong, "esc", true);
            "key esc".into()
        }
        6 => {
            // (a repeat, or a yank typed on after an answer: both keys before any frame)
            key(sim, ed, model, wrong, "y", false);
            key(sim, ed, model, wrong, "y", true);
            "keys y y in one batch".into()
        }
        _ => {
            let ms = *c.pick(&[50, 300, 1200]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

/// No `y` judged wrong by the model, and the editor counts no frame the interface didn't present.
fn held(ed: &Editor, m: &Model, wrong: &Cell<Option<String>>) -> Result<(), String> {
    if let Some(why) = wrong.take() {
        return Err(why);
    }
    if ed.presented > m.shown {
        return Err(format!(
            "frame {} counted as presented, the interface presented {}",
            ed.presented, m.shown
        ));
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    format!(
        "cards {} · frame {} · presented {} · card has the keys {}",
        ed.offers.len(),
        ed.frames,
        ed.presented,
        ed.card_has_keys()
    )
}
