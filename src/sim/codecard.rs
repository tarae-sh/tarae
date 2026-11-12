//! The code tool's batch card, simulated (`codetool::tool::hold`, piece c): a program hands out two reads outside the
//! project and one inside together; each read is judged on its own job, and the bound (`JUDGED_WITHIN`, a job sleeping
//! on the batch's `Wake`) races them — the seed picks which lands first, and the user's answer (`y` or `n`) to each
//! card that comes.
//!
//! - `a_batchs_card_never_grows_and_each_place_asks_once`: after every step, a card once shown never changes (no place
//!   joins it a moment before `y`, granted unread), and nothing is held once none of the batch is left to judge (the
//!   bound or the last verdict let it go); at the end every place outside was on exactly one card, the program ended,
//!   and what it was given for the last call is the verdict of the card that listed that place — its words on `y`,
//!   the user's refusal on `n`. The bound's sleeper never outlives the batch (the simulation fails a thread that
//!   does). Seeds land both ways: one card for both places, and a late place asking alone.
//! - `a_hung_calls_batch_card_comes_at_the_bound`: the same with a fourth call in the batch, a `references` its
//!   language server never answers (a stand-in that stays silent; tarae's own fallback answers it with the text
//!   matches after `nav::REFS_WAIT`, 20 s) — the case the bound is for: no verdict of the batch can let the held reads
//!   go, the bound has to, and their card still comes. Seeds show it: a card comes while the hung call is still out.
//! - `a_place_named_like_a_key_never_rides_in_a_batch`: the same with a fourth read outside, a file named like a key
//!   (the design session's call, 2026-11-12) — after every step, a card listing it lists no other place and is today's
//!   single key card; and the key's read counts as judged for the batch (it asks alone): once the rest is judged too
//!   — both plain places held, the read inside answered — their card is up at that step, never kept for the bound.
//!   Seeds show it: a batch card comes before the bound. At the end the key was on exactly one card, and the
//!   program's last call (its read) got what that card said, whatever the batch's card said.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;

use super::{Choices, Sim};
use crate::codetool;
use crate::codetool::tests::{script, stand_in};
use crate::editor::Editor;

const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5, 7, 10, 11, 12];
/// The hung scenario's: 16, 18, 19 and 22 have a place alone; in 3 the clock passes the fallback before the card.
const HUNG_SEEDS: &[u64] = &[0, 1, 2, 3, 4, 16, 18, 19, 22];

/// By scenario (`Kind`): seeds whose two plain places came on one card, seeds where one came alone; and seeds where a
/// card came while the hung call was still out.
static ONE_CARD: [AtomicUsize; 3] = [AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0)];
static ALONE: [AtomicUsize; 3] = [AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0)];
static WHILE_HUNG: AtomicUsize = AtomicUsize::new(0);
/// The warned scenario's seeds whose batch card came before the bound: the last verdict let it go, the key's read
/// (asking alone) already counted as judged.
static BEFORE_BOUND: AtomicUsize = AtomicUsize::new(0);

/// What the batch holds besides its two places outside and its read inside.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Plain,
    /// A `references` its server never answers.
    Hung,
    /// A read of a file named like a key, outside.
    Warned,
}

#[test]
fn a_batchs_card_never_grows_and_each_place_asks_once() {
    let name = "a_batchs_card_never_grows_and_each_place_asks_once";
    super::seeds(name, SEEDS, |c| batch_card(name, c, Kind::Plain));
    let (one, alone) = (ONE_CARD[0].load(Ordering::SeqCst), ALONE[0].load(Ordering::SeqCst));
    assert!(one > 0 && alone > 0, "seeds landed one way only: {one} on one card, {alone} with a place alone");
}

#[test]
fn a_place_named_like_a_key_never_rides_in_a_batch() {
    let name = "a_place_named_like_a_key_never_rides_in_a_batch";
    super::seeds(name, SEEDS, |c| batch_card(name, c, Kind::Warned));
    let (one, alone) = (ONE_CARD[2].load(Ordering::SeqCst), ALONE[2].load(Ordering::SeqCst));
    assert!(one > 0 && alone > 0, "seeds landed one way only: {one} on one card, {alone} with a place alone");
    let before = BEFORE_BOUND.load(Ordering::SeqCst);
    assert!(before > 0, "no seed's batch card came before the bound");
}

#[test]
fn a_hung_calls_batch_card_comes_at_the_bound() {
    let name = "a_hung_calls_batch_card_comes_at_the_bound";
    super::seeds(name, HUNG_SEEDS, |c| batch_card(name, c, Kind::Hung));
    let (one, alone) = (ONE_CARD[1].load(Ordering::SeqCst), ALONE[1].load(Ordering::SeqCst));
    assert!(one > 0 && alone > 0, "seeds landed one way only: {one} on one card, {alone} with a place alone");
    let hung = WHILE_HUNG.load(Ordering::SeqCst);
    assert!(hung > 0, "no seed's card came while the hung call was out");
}

fn is_read(o: &crate::offer::Offer) -> bool {
    matches!(o.what, crate::offer::What::Read { .. })
}

/// The id of the hung scenario's `references`, and of the warned one's read of a key: the batch's last.
const HUNG: u32 = 3;

fn batch_card(name: &str, choices: Choices, kind: Kind) -> Vec<String> {
    let hung = kind == Kind::Hung;
    let mut sim = Sim::new(name, choices, crate::test_results::PATIENCE);
    crate::tools::ANSWERED.with(|a| a.borrow_mut().clear());
    let s = crate::scratch::Scratch::new("sim-codetool-card");
    let base = crate::paths::real(&s).unwrap();
    let dir = base.join("p");
    let places = [base.join("one"), base.join("two")];
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "alpha\n").unwrap();
    for (p, word) in places.iter().zip(["one marker", "two marker"]) {
        std::fs::create_dir_all(p).unwrap();
        std::fs::write(p.join("f.txt"), format!("{word}\n")).unwrap();
    }
    let key = base.join("keyed/.env");
    std::fs::create_dir_all(base.join("keyed")).unwrap();
    std::fs::write(&key, "key marker\n").unwrap();
    let key = key.display().to_string();
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = dir.clone();
    if hung {
        // (a server for the project's file that answers everything but `references`, up before the program asks it)
        let lib = dir.join("lib.rs");
        std::fs::write(&lib, "fn helper() {}\n").unwrap();
        ed.open(&lib).unwrap();
        static TRAITS: std::sync::LazyLock<crate::lsp::ServerTraits> =
            std::sync::LazyLock::new(crate::lsp::ServerTraits::default);
        let serve: crate::lsp::Serve = Box::new(|method, _| match method {
            "initialize" => {
                Some(json!({ "capabilities": { "positionEncoding": "utf-8", "referencesProvider": true } }))
            }
            "textDocument/references" => Some(json!(crate::lsp::NO_REPLY)),
            _ => None,
        });
        let cid = ed.lsp_own("silent", &TRAITS, serve);
        let id = ed.doc().id;
        if let Some(d) = ed.docs.iter_mut().find(|d| d.id == id) {
            d.lsp.attach(cid, false, true);
        }
        sim.walk(&mut ed, 0, |_, _| String::new(), |_| Ok(()), |_| "the server up".into());
    }
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.claude.chat.as_mut().unwrap().code = true;
    codetool::tool::ENGINE.with(|e| *e.borrow_mut() = Some(stand_in()));
    codetool::tool::BARE.with(|b| b.set(true));
    let file = |i: usize| places[i].join("f.txt").display().to_string();
    let read = |id: u32, f: String| json!({ "id": id, "name": "read", "arg": { "files": [f] } });
    let mut calls = vec![read(0, file(0)), read(1, "a.txt".into()), read(2, file(1))];
    match kind {
        Kind::Plain => {}
        Kind::Hung => calls.push(
            json!({ "id": HUNG, "name": "references", "arg": { "path": "lib.rs", "symbol": "helper" } }),
        ),
        Kind::Warned => calls.push(read(HUNG, key.clone())),
    }
    let batch = json!({ "pending": calls });
    let waits = "pending\n{\"pending\":[]}";
    let generation = ed.workspace.claude.chat.as_ref().unwrap().generation();
    let call = crate::tools::Call::of(generation, "", json!(70)).named("code");
    let started = crate::clock::now();
    // (the warned batch's program ends on its fourth answer, the key's: the others' on their third)
    let src = match kind {
        Kind::Warned => script(&[&format!("pending\n{batch}"), waits, waits, waits, "@"]),
        _ => script(&[&format!("pending\n{batch}"), waits, waits, "@"]),
    };
    codetool::tool::call(&mut ed, call, &json!({ "src": src })).unwrap();

    // (each card as first seen, by its id; the user's answer to each)
    let seen: RefCell<BTreeMap<u64, String>> = RefCell::default();
    let said: RefCell<BTreeMap<u64, char>> = RefCell::default();
    let while_hung = std::cell::Cell::new(false);
    let before_bound = std::cell::Cell::new(false);
    let answer = |sim: &mut Sim, ed: &mut Editor| {
        let Some(card) = ed.offers.first().filter(|o| is_read(o)) else {
            return "(no card to answer)".into();
        };
        let (id, key) = (card.id, *sim.choices.pick(&['y', 'n']));
        said.borrow_mut().insert(id, key);
        match key {
            'y' => ed.offer_accept_by('y'),
            _ => ed.offer_dismiss(),
        }
        format!("card {id} answered {key}")
    };
    let check = |ed: &Editor| {
        let program = ed.workspace.claude.programs.first();
        let out = program.is_some_and(|r| r.waiting().any(|id| id == HUNG));
        let mut seen = seen.borrow_mut();
        for o in ed.offers.iter().filter(|o| is_read(o)) {
            if !seen.contains_key(&o.id) && out {
                while_hung.set(true);
            }
            let first = seen.entry(o.id).or_insert_with(|| o.question.clone());
            if *first != o.question {
                return Err(format!("card {} grew after it was shown:\n{first}\n→\n{}", o.id, o.question));
            }
            // (a place named like a key: on today's single key card, by itself)
            let others = o.question.contains(&file(0)) || o.question.contains(&file(1));
            if o.question.contains(&key) && (others || o.title != crate::reads::SCRIPT_KEYS_TITLE) {
                return Err(format!(
                    "a key rode with others, or not on its own card: {}\n{}",
                    o.title, o.question
                ));
            }
        }
        // (none of the batch left to judge: what was held is on its card — the bound's release, or the last verdict's)
        if let Some((held, unjudged)) = program.map(|r| r.holding())
            && unjudged == 0
            && held > 0
        {
            return Err(format!(
                "{held} read(s) still held with none of the batch left to judge: no card for them"
            ));
        }
        // (the key's read asks alone and counts as judged: the last of the rest judged — both plain places held, the
        // read inside answered — its card is up at that step, never kept for the bound)
        if kind == Kind::Warned
            && let Some(r) = program
        {
            let at = std::path::Path::new(&key);
            let reads = ed.workspace.claude.chat.as_ref().map(|c| &c.reads);
            let asking = ed.offers.iter().any(|o| is_read(o) && o.question.contains(&key))
                || reads.is_some_and(|q| q.iter().any(|p| p.look.at.as_deref() == Some(at)));
            let key_judged = asking || !r.waiting().any(|id| id == HUNG);
            let inside_answered = !r.waiting().any(|id| id == 1);
            if key_judged && inside_answered && r.holding().0 == 2 {
                return Err(
                    "the batch is judged, the key's read asking alone, and its two places still held: \
                            no card till the bound"
                        .into(),
                );
            }
        }
        let both = ed
            .offers
            .iter()
            .any(|o| is_read(o) && o.question.contains(&file(0)) && o.question.contains(&file(1)));
        if both && crate::clock::since(started) < codetool::tool::JUDGED_WITHIN {
            before_bound.set(true);
        }
        Ok(())
    };
    let print = |ed: &Editor| {
        let cards: Vec<u64> = ed.offers.iter().filter(|o| is_read(o)).map(|o| o.id).collect();
        let ended = crate::tools::ANSWERED.with(|a| a.borrow().iter().any(|(k, _)| *k == json!(70)));
        format!("cards up {cards:?} · ended {ended}")
    };
    // (the user's answers among the jobs; then each card still up answered, the jobs it starts run out)
    sim.walk(&mut ed, 3, answer, check, print);
    for _ in 0..4 {
        if !ed.offers.iter().any(is_read) {
            break;
        }
        sim.walk(&mut ed, 1, |sim, ed| answer(sim, ed), check, print);
    }
    let (seen, said) = (seen.into_inner(), said.into_inner());
    let on =
        |i: usize| seen.iter().filter(|(_, q)| q.contains(&file(i))).map(|(id, _)| *id).collect::<Vec<_>>();
    let (one, two) = (on(0), on(1));
    let keyed: Vec<u64> = seen.iter().filter(|(_, q)| q.contains(&key)).map(|(id, _)| *id).collect();
    sim.at_end(|| {
        if one.len() != 1 || two.len() != 1 {
            return Err(format!("each place on one card: {one:?} {two:?} of {seen:#?}"));
        }
        if keyed.len() != (kind == Kind::Warned) as usize {
            return Err(format!("the key's place on one card: {keyed:?} of {seen:#?}"));
        }
        if seen.values().any(|q| q.contains("a.txt")) {
            return Err("a read in the project asked".into());
        }
        let answer = crate::tools::ANSWERED.with(|a| a.borrow().iter().find(|(k, _)| *k == json!(70)).cloned());
        let Some((_, Ok(answer))) = answer else { return Err(format!("the program didn't end: {answer:?}")) };
        let refused = answer.contains("the user chose not to let");
        // (the last call's place: the key's, its own card's verdict — else the second place's)
        let (last, marker) = match kind {
            Kind::Warned => (keyed[0], "key marker"),
            _ => (two[0], "two marker"),
        };
        match (said.get(&last), answer.contains(marker), refused) {
            (Some('y'), true, false) | (Some('n'), false, true) => Ok(()),
            (verdict, ..) => Err(format!(
                "the last call's place answered {verdict:?} (cards {seen:#?}, answers {said:?}), the program given: {answer}"
            )),
        }
    });
    match one == two {
        true => ONE_CARD[kind as usize].fetch_add(1, Ordering::SeqCst),
        false => ALONE[kind as usize].fetch_add(1, Ordering::SeqCst),
    };
    if hung && while_hung.get() {
        WHILE_HUNG.fetch_add(1, Ordering::SeqCst);
    }
    if kind == Kind::Warned && before_bound.get() {
        BEFORE_BOUND.fetch_add(1, Ordering::SeqCst);
    }
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}
