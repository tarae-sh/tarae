//! Every way in names where `C-q` goes (#544), simulated: Claude at work, its chat holding the keys at first; at random
//! a card of Claude's comes (up to three), Claude's edit review comes, `C-q`, an answer (`n`), `esc`, `C-w w`, the keys
//! put on the code or back on the chat, a frame at another size. Each thing that comes has waited longer than what comes
//! after it (minutes apart, so the inbox's order is the order they came, whichever clock each is on).
//!
//! After every step, a frame: every `C-q answer` pair drawn (a card's bottom line, the review's head or peek, the chat's
//! foot) names the one `C-q` goes to (`c_q_next`), and no other — the rest say how many presses reach them (`C-q ×2`).
//! At each `C-q`: what it gave the keys to is what the frame before said one press reaches, the status line said `C-q`
//! only where it gave them somewhere, and what said `×2` before says `C-q` after (#544 (2): each press steps on).

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::inbox::Waiting;
use crate::uistate::Region;
use crate::uistate::tests::{Names, Way};

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

/// Where the keys go once the next `C-q` reaches the card in front or the review (`c_q_next`), as a drawn way names it.
fn reaches(ed: &Editor) -> Option<Region> {
    match ed.c_q_next()? {
        Waiting::Card(id) => Some(Region::Card(id)),
        Waiting::Review => Some(Region::EditReview),
    }
}

/// A frame: every pair drawn saying one press names what `C-q` reaches, and no other says one press.
fn judge(ed: &mut Editor, size: (u16, u16)) -> Result<Vec<Way>, String> {
    let ways = crate::uistate::tests::ways_in(ed, "C-q", size)?;
    let reached = reaches(ed);
    if let Some(w) = ways.iter().find(|w| w.names != Names::Next && w.n == 1 && w.at != reached) {
        return Err(format!("`{}` names the {:?}; C-q reaches {reached:?}", w.row, w.names));
    }
    if let Some(w) = ways.iter().find(|w| w.n > 1 && w.at == reached) {
        return Err(format!("`{}` says ×{}; one C-q reaches it", w.row, w.n));
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
            let shown = (ed.last_frame.as_ref())
                .and_then(|f| crate::screen::text(f.0.as_slice(), w.size.0, w.size.1))
                .unwrap_or_default();
            let before = ed.keys_to().region;
            // (reached: what waited took the keys — a focused one's `C-q` steps on, or gives them back)
            ed.handle_key("C-q".parse().unwrap());
            ed.focus_sync();
            crate::term::render(ed, &mut Vec::new(), w.size.0, w.size.1).unwrap();
            let after = ed.keys_to().region;
            let reached =
                Some(after).filter(|a| *a != before && matches!(a, Region::Card(_) | Region::EditReview));
            for way in ways.iter().filter(|way| way.n == 1) {
                let right = match way.names {
                    Names::Next => reached.is_some(),
                    _ => way.at == reached,
                };
                if !right {
                    let row = &way.row;
                    wrong.set(Some(format!(
                        "`{row}` said C-q, it gave the keys to {after:?} (from {before:?})"
                    )));
                }
            }
            if reached.is_some() && ways.is_empty() {
                wrong.set(Some(format!("C-q gave the keys to {after:?}, nothing drawn said so:\n{shown}")));
            }
            // (what said ×2 says C-q now, wherever it's still drawn)
            let now = crate::uistate::tests::ways_in(ed, "C-q", w.size).unwrap_or_default();
            for two in ways.iter().filter(|way| way.n == 2) {
                if let Some(was) = now.iter().find(|n| n.at == two.at && n.n != 1) {
                    wrong.set(Some(format!("`{}` said ×2; after one C-q it says `{}`", two.row, was.row)));
                }
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
        ed.c_q_next(),
        ed.keys_to().region
    )
}
