//! The pointer resting on a name in the chat (#389 step 6, `hoverrest.rs` and `chatlink::rest`), simulated on
//! `sim::chatlink`'s answer — a cited line, its line quoted in a block, a name in prose on it, one no cited place holds,
//! a word that isn't code: the pointer wandering over the chat (onto those, anywhere, onto the card), time passing by
//! the rest's own scale, the wheel, a key, the file edited under it, its buffer closed, frames — the stand-in answering
//! each hover with where it was asked. After every step: the file closed open in no buffer again and no card raised (a
//! rest is a reveal); a hover asked only for the rest there now, `REST` after it came — at once
//! where a card was up as it came (#391's hand-off) —, once; a card answered only saying a place in the file that holds
//! the name rested on, as the file was then — a cited place's its own lines, a word that isn't code none; a card drawn
//! only with the pointer on its name or on the card; no timer left armed once all's quiet.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use unicode_width::UnicodeWidthStr;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::event::{Event, Mouse, MouseKind};
use crate::hoverrest::{In, REST};
use crate::lsp_editor::Kind;

const NAME: &str = "the_pointer_resting_on_a_name_in_the_chat_asks_only_there";

const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5, 6, 7];

const INPUTS: usize = 120;

const LIMIT: Duration = crate::test_results::PATIENCE;

const SIZE: (u16, u16) = (120, 30);

/// Where the pointer is put: what it finds there (a code span drawn without its backticks) — the quoted name, the
/// prose one on the cited line, the place, a word that isn't code, a name no cited place holds.
const AT: &[&str] = &["helper(2)", "total", "main.rs:6", "Nothing", "helper alone"];

#[test]
fn the_pointer_resting_on_a_name_in_the_chat_asks_only_there() {
    super::seeds(NAME, SEEDS, run);
}

#[derive(Default)]
struct World {
    /// Where the pointer is.
    at: Option<(usize, usize)>,
    /// The hover requests seen out (their ids).
    seen: Vec<u64>,
    /// The rests asked of (their numbers).
    asked: Vec<u64>,
    /// Each rest as the pointer made it: its number, when, and whether a card was on the frame then.
    came: Vec<(u64, Instant, bool)>,
    /// The rests whose answer was judged.
    cards: Vec<u64>,
    /// A line added at the file's top.
    added: bool,
    /// The file's buffer closed (as `:bc` closes it): from then on no rest may open it again.
    closed: Option<std::path::PathBuf>,
    /// What a frame drew against the rules (`drawn`), said by the check after it.
    fault: Option<String>,
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new(NAME, choices, LIMIT);
    // (results land as the main loop takes them, a frame after each that changed what's drawn)
    sim.frames = Some(SIZE);
    let (_dir, mut ed, id, _) = super::chatlink::setup("sim-chatrest");
    let world = RefCell::new(World::default());
    let check = |ed: &Editor| held(ed, id, &mut world.borrow_mut());
    sim.walk(&mut ed, INPUTS, |sim, ed| step(sim, ed, id, &mut world.borrow_mut()), check, print);
    assert!(!ed.hover_rest.armed(), "a timer left armed once all's quiet");
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// The pointer to (`x`, `y`): a new rest there noted with whether a card was up as it came.
fn hover(ed: &mut Editor, w: &mut World, x: usize, y: usize) {
    let up = ed.screen.rest_card.get().is_some() && ed.hover_rest.rest.is_some();
    let was = ed.hover_rest.rest.as_ref().map(|r| r.n());
    w.at = Some((x, y));
    ed.handle_event(Event::Hover(x as u16, y as u16));
    if let Some(n) = ed.hover_rest.rest.as_ref().map(|r| r.n()).filter(|n| Some(*n) != was) {
        w.came.push((n, crate::clock::now(), up));
    }
}

/// The cell the transcript's byte `k` was drawn in by the last frame, if it was.
fn cell_of(ed: &Editor, k: usize) -> Option<(usize, usize)> {
    let (y0, _, rows, first) = ed.screen.chat_view.get()?;
    let t = crate::chatdoc::transcript(ed)?;
    let d = t.pane.doc_in()?;
    let laid = ed.screen.chat_laid.borrow();
    for y in y0..y0 + rows {
        let Some(s) = crate::chatdoc::doc_row(&laid, first + y - y0).and_then(|r| t.row(r)) else { continue };
        if (s.start..s.start + s.len).contains(&k) {
            let w = d.text.byte_slice(s.start..k).to_string().width();
            let x0 = ed.screen.chat_x()? + crate::chrome::GAP + crate::chrome::PAD;
            return Some((x0 + s.x0 + w, y));
        }
    }
    None
}

fn frame(ed: &mut Editor, w: &mut World) {
    crate::term::render(ed, &mut Vec::new(), SIZE.0, SIZE.1).unwrap();
    w.fault = w.fault.take().or_else(|| drawn(ed, w).err());
}

fn step(sim: &mut Sim, ed: &mut Editor, file: crate::document::DocId, w: &mut World) -> String {
    let c = &mut sim.choices;
    // (by arm: a frame 2, onto a name 3, anywhere in the chat 1, onto the card 2, time 6, a key 1, the wheel 1, the
    // file edited 1, its buffer closed 1 — once, most runs late)
    match c.weighted(&[2, 3, 1, 2, 6, 1, 1, 1, 1]) {
        0 => {
            frame(ed, w);
            "a frame".into()
        }
        1 => {
            let at = *c.pick(AT);
            let k = crate::chatdoc::transcript(ed)
                .and_then(|t| t.pane.doc_in())
                .and_then(|d| d.text.to_string().find(at));
            let Some(k) = k else { return format!("({at} not laid out yet)") };
            let Some((x, y)) = cell_of(ed, k + c.below(2)) else { return format!("({at} not in sight)") };
            hover(ed, w, x, y);
            format!("onto {at} at {x},{y}")
        }
        2 => {
            let (Some(x0), Some((y0, width, rows, _))) = (ed.screen.chat_x(), ed.screen.chat_view.get())
            else {
                return "(no chat drawn)".into();
            };
            let (x, y) = (x0 + c.below(width.max(1)), y0 + c.below(rows.max(1)));
            hover(ed, w, x, y);
            format!("the pointer at {x},{y}")
        }
        3 => {
            let Some(r) = ed.screen.rest_card.get() else { return "(no card to go onto)".into() };
            let (x, y) = (r.0 + c.below(r.2), r.1 + c.below(r.3));
            hover(ed, w, x, y);
            format!("onto the card at {x},{y}")
        }
        4 => {
            let ms = *c.pick(&[100, 300, 450, 600, 2000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms}ms pass")
        }
        5 => {
            let k = *c.pick(&["j", "k", "G"]);
            ed.handle_event(Event::Key(k.parse().unwrap()));
            format!("key {k}")
        }
        6 => {
            let Some((x, y)) = w.at else { return "(no pointer for the wheel)".into() };
            let kind = *c.pick(&[MouseKind::ScrollDown, MouseKind::ScrollUp]);
            ed.handle_event(Event::Mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false }));
            format!("the wheel at {x},{y}")
        }
        8 => {
            // (by you: the names' file is open in no buffer from now on — a rest on them must open none)
            let Some(d) = ed.docs.iter().find(|d| d.id == file) else { return "(no file to close)".into() };
            w.closed = d.path.clone();
            ed.close_doc(file);
            "the file's buffer closed".into()
        }
        _ => {
            // (Claude's edit, say: the file's lines move under a rest)
            let Some(d) = ed.docs.iter_mut().find(|d| d.id == file) else { return "(no file)".into() };
            let tx = match w.added {
                false => crate::transaction::Change::insert(0, "// x\n"),
                true => crate::transaction::Change::delete(0, 5),
            };
            d.apply(&crate::transaction::Transaction::new(vec![tx]));
            w.added = !w.added;
            format!("a line {} at the file's top", if w.added { "added" } else { "taken" })
        }
    }
}

/// The name a rest is on in the chat, as drawn.
fn name_of(r: &crate::hoverrest::Rest) -> &str {
    match &r.spot.pane {
        In::Chat(name) => name,
        In::Code(_) => "",
    }
}

/// A card's words, run together.
fn said(card: &crate::hoverrest::Card) -> String {
    let lines = crate::markdown::wrap(&card.docs, 80, Default::default());
    lines.iter().flat_map(|l| l.iter().map(|s| s.text.clone())).collect()
}

/// The rest's rules, after every step.
fn held(ed: &Editor, file: crate::document::DocId, w: &mut World) -> Result<(), String> {
    let now = crate::clock::now();
    let rest = ed.hover_rest.rest.as_ref();
    // A rest is a reveal: the file closed stays closed, no card raised
    if let Some(p) = &w.closed
        && ed.docs.iter().any(|d| d.path.as_ref() == Some(p))
    {
        return Err(format!("{} opened again, by no one but the pointer", p.display()));
    }
    if !ed.offers.is_empty() {
        return Err(format!("a card raised: {}", ed.offers[0].title));
    }
    // A hover asked: for the rest there now, `REST` after it came — at once where a card was up as it came —, once
    for (&(_, id), p) in &ed.lsp.pending {
        let Kind::PointerHover { n } = p.kind else { continue };
        if w.seen.contains(&id) {
            continue;
        }
        w.seen.push(id);
        if !rest.is_some_and(|r| r.n() == n) {
            return Err(format!("rest {n} asked, not the one there"));
        }
        let Some(&(_, since, warm)) = w.came.iter().find(|c| c.0 == n) else {
            return Err(format!("rest {n} asked, the pointer never made it"));
        };
        if now < since + REST && !warm {
            return Err(format!("rest {n} asked {:?} after it came, no card up then", now - since));
        }
        if w.asked.contains(&n) {
            return Err(format!("rest {n} asked twice"));
        }
        w.asked.push(n);
    }
    // An answer come: a place in the file holding the name, as the file is now (an answer for another version is
    // dropped) — a cited place's its own lines, a word that isn't code none
    if let Some(r) = rest
        && let Some(card) = &r.card
        && !w.cards.contains(&r.n())
    {
        w.cards.push(r.n());
        let (name, text) = (name_of(r), said(card));
        if let Some(at) = text.split("asked at ").nth(1) {
            let (l, c) = at.split_once(':').unwrap_or_default();
            let (l, c): (usize, usize) = (l.parse().unwrap_or(usize::MAX), c.parse().unwrap_or(usize::MAX));
            // (asked before the file was closed: its answer can't be judged against a buffer gone)
            let Some(d) = ed.docs.iter().find(|d| d.id == file) else {
                return w.fault.take().map_or(Ok(()), Err);
            };
            let there = d.text.get_line(l).map(|t| t.to_string()).unwrap_or_default();
            if !there.get(c..).is_some_and(|t| t.starts_with(name)) {
                return Err(format!("rest {} on `{name}` answered at {l}:{c}, where {there:?} is", r.n()));
            }
        } else if !text.is_empty() && !name.contains(':') {
            return Err(format!("rest {} on `{name}` shows {text:?}, asking nothing", r.n()));
        }
        if name == "Nothing" && !text.is_empty() {
            return Err(format!("a word that isn't code shows {text:?}"));
        }
    }
    w.fault.take().map_or(Ok(()), Err)
}

/// A frame just drawn: a card only for the rest under the pointer — on its name or the card.
fn drawn(ed: &Editor, w: &World) -> Result<(), String> {
    let Some(rect) = ed.screen.rest_card.get() else { return Ok(()) };
    let Some(r) = ed.hover_rest.rest.as_ref() else { return Err("a card drawn with nothing resting".into()) };
    if r.card.is_none() && !w.came.iter().any(|c| c.0 == r.n() && c.2) {
        return Err("a card drawn for a rest not answered, that came with none up".into());
    }
    let inside = |(x, y): (usize, usize)| {
        (rect.0..rect.0 + rect.2).contains(&x) && (rect.1..rect.1 + rect.3).contains(&y)
    };
    let (x0, y0) = r.spot.cell;
    let wide = name_of(r).width().max(1);
    let on_name = |(x, y): (usize, usize)| y == y0 && (x0..x0 + wide).contains(&x);
    if !w.at.is_some_and(|at| inside(at) || on_name(at)) {
        return Err(format!(
            "a card up with the pointer at {:?}, off its name at {x0},{y0} and the card",
            w.at
        ));
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    match ed.hover_rest.rest.as_ref() {
        Some(r) => {
            let card = r.card.as_ref().map(|c| c.docs.len());
            let asked = ed.lsp.pending.values().any(|p| p.kind == Kind::PointerHover { n: r.n() });
            format!("rest {} on `{}` asked {asked} card {card:?}", r.n(), name_of(r))
        }
        None => "no rest".into(),
    }
}
