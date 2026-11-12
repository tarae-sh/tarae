//! Every way in names where `C-q` goes (#544), simulated: Claude at work, its chat holding the keys at first; at random
//! a card of Claude's comes (up to three), Claude's edit review comes, `C-q`, an answer (`n`), `esc`, `C-w w`, the keys
//! put on the code or back on the chat, a frame at another size. Each thing that comes has waited longer than what comes
//! after it (minutes apart, so the inbox's order is the order they came, whichever clock each is on).
//!
//! After every step, a frame: every `C-q answer` pair drawn (a card's bottom line, the review's head or peek, the chat's
//! foot) names the one `C-q` goes to (`next_up`), and no other. At each `C-q`: what it gave the keys to is what the frame
//! before said, and the status line said `C-q` only where it gave them somewhere.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::inbox::Next;
use crate::uistate::Region;
use crate::uistate::tests::Names;

const NAME: &str = "every_way_in_names_where_c_q_goes";

const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 50;

const LIMIT: Duration = crate::test_results::PATIENCE;

const SIZES: &[(u16, u16)] = &[(120, 40), (160, 40), (100, 30)];

#[test]
fn every_way_in_names_where_c_q_goes() {
    super::seeds(NAME, SEEDS, run);
}

/// What the scenario keeps from its own steps.
struct World {
    /// Things come so far (each older than the next by a minute: the first came 100 minutes ago).
    came: u64,
    /// The frame's size.
    size: (u16, u16),
}

fn run(choices: Choices) -> Vec<String> {
    use crate::chat::{Msg, Role};
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let mut ed = crate::uistate::tests::editor();
    let mut c = crate::chat::Chat::new();
    c.state = crate::chat::State::Thinking;
    c.msgs.push(Msg::new(Role::User, "hi", ""));
    ed.workspace.claude.chat = Some(c);
    ed.workspace.set_at(crate::workspace::Here::Chat);
    let w = RefCell::new(World { came: 0, size: SIZES[0] });
    let wrong = Cell::new(None);
    let check = |_: &Editor| wrong.take().map_or(Ok(()), Err);
    let input = |sim: &mut Sim, ed: &mut Editor| {
        let said = step(sim, ed, &mut w.borrow_mut(), &wrong);
        if let Err(why) = judge(ed, w.borrow().size) {
            wrong.set(Some(why));
        }
        said
    };
    sim.walk(&mut ed, INPUTS, input, check, print);
    sim.trace().to_vec()
}

/// What `C-q` reaches here now, as a drawn way in names it (a card holding the keys: nothing — its `C-q` gives them back).
fn reaches(ed: &Editor) -> Option<Names> {
    let front = ed.offers.first().map(|o| o.id);
    match ed.next_up() {
        _ if ed.card_has_keys() => None,
        _ if ed.offer_reading() => Some(Names::Card),
        Next::Card(_) => Some(Names::Card),
        Next::Review => Some(Names::Review),
        Next::Go(e) if e.card().is_some() && e.card() == front => Some(Names::Card),
        _ => None,
    }
}

/// A frame: every pair drawn names what `C-q` reaches, and no other.
fn judge(ed: &mut Editor, size: (u16, u16)) -> Result<Vec<(Names, String)>, String> {
    let ways = crate::uistate::tests::ways_in(ed, "C-q", size)?;
    let reached = reaches(ed);
    if let Some((n, row)) = ways.iter().find(|(n, _)| *n != Names::Next && Some(*n) != reached) {
        return Err(format!("`{row}` names the {n:?}; C-q reaches {reached:?}"));
    }
    Ok(ways)
}

/// `t` set back by what came's age: the `n`th thing come is 100 − `n` minutes old.
fn aged(t: Instant, n: u64) -> Instant {
    t.checked_sub(Duration::from_secs(60 * (100 - n))).unwrap_or(t)
}

fn step(sim: &mut Sim, ed: &mut Editor, w: &mut World, wrong: &Cell<Option<String>>) -> String {
    let c = &mut sim.choices;
    // (by arm: a card comes 2, the review comes 2, `C-q` 4, `n` 2, `esc` 1, `C-w w` 2, the keys on the code or the chat
    // 1, the screen's size 1)
    match c.weighted(&[2, 2, 4, 2, 1, 2, 1, 1]) {
        0 if ed.offers.len() >= 3 => "(three cards up: no more)".into(),
        0 => {
            w.came += 1;
            let what = crate::offer::What::Asks { call: crate::tools::Call::test(w.came) };
            let o =
                crate::offer::Offer::new("Claude asks".into(), "to see the screen", "", String::new(), what);
            ed.push_offer(o);
            let i = ed.offers.len() - 1;
            ed.offers[i].since = aged(ed.offers[i].since, w.came);
            format!("a card comes (#{})", w.came)
        }
        1 if ed.workspace.claude.review.is_some() => "(a review up: no other)".into(),
        1 => {
            w.came += 1;
            let id = ed.doc().id;
            crate::llm::review_with(ed, id, "x", vec![(0, 3, "one".into(), "uno".into())], None);
            if let Some(r) = ed.workspace.claude.review.as_mut() {
                r.asked = aged(Instant::now(), w.came);
            }
            format!("Claude's edit review comes (#{})", w.came)
        }
        2 => {
            let ways = match judge(ed, w.size) {
                Ok(ways) => ways,
                Err(why) => {
                    wrong.set(Some(why));
                    return "C-q (the frame before it wrong)".into();
                }
            };
            let before = ed.keys_to().region;
            // (reached: what waited took the keys — a focused card's `C-q` gives them back, to a review under it too)
            let review_waited = ed.workspace.claude.review.as_ref().is_some_and(crate::llm::Review::waits);
            ed.handle_key("C-q".parse().unwrap());
            ed.focus_sync();
            crate::term::render(ed, &mut Vec::new(), w.size.0, w.size.1).unwrap();
            let after = ed.keys_to().region;
            let reached = match after {
                _ if after == before => None,
                Region::Card(_) => Some(Names::Card),
                Region::EditReview if review_waited => Some(Names::Review),
                _ => None,
            };
            for (n, row) in &ways {
                let right = match n {
                    Names::Next => reached.is_some(),
                    n => Some(*n) == reached,
                };
                if !right {
                    wrong.set(Some(format!(
                        "`{row}` said C-q, it gave the keys to {after:?} (from {before:?})"
                    )));
                }
            }
            if reached.is_some() && ways.is_empty() {
                wrong.set(Some(format!("C-q gave the keys to {after:?}, nothing drawn said so")));
            }
            format!("key C-q: {before:?} → {after:?}")
        }
        3 => {
            ed.handle_key(crate::key::Key::plain(crate::key::Code::Char('n')));
            "key n".into()
        }
        4 => {
            ed.handle_key("esc".parse().unwrap());
            "key esc".into()
        }
        5 => {
            ed.handle_key("C-w".parse().unwrap());
            ed.handle_key("w".parse().unwrap());
            "keys C-w w".into()
        }
        6 if c.below(2) == 0 => {
            ed.workspace.viewer.keys_to_code();
            "the keys on the code".into()
        }
        6 => {
            ed.workspace.set_at(crate::workspace::Here::Chat);
            "the keys on the chat".into()
        }
        _ => {
            w.size = *c.pick(SIZES);
            format!("the screen {}×{}", w.size.0, w.size.1)
        }
    }
}

fn print(ed: &Editor) -> String {
    let r = ed.workspace.claude.review.as_ref().map(|r| (r.ready, r.focused, r.folded));
    format!(
        "cards {} · review {r:?} · next {:?} · keys {:?}",
        ed.offers.len(),
        ed.next_up(),
        ed.keys_to().region
    )
}
