//! The pointer's rest (#233, `hoverrest.rs`), simulated: the pointer wandering over a file's names, blanks, its gutter
//! and the card, time passing by the rest's own scale, your keys, an edit, frames at two widths — a stand-in server
//! (`Client::own`: its answers jobs, landing among the steps) answering each hover with where it was asked. After every
//! step: a hover asked only of the rest under the pointer, `REST` after it came there, and once; a card drawn only for
//! the rest under the pointer — the pointer on its name or on the card — and saying the place it was asked of (an
//! answer for a rest left never shown); no timer left armed once all's quiet.

use std::cell::RefCell;
use std::time::Duration;

use serde_json::json;
use unicode_width::UnicodeWidthStr;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::event::{Event, Mouse, MouseKind};
use crate::hoverrest::REST;
use crate::lsp_editor::Kind;

const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 80;

const LIMIT: Duration = crate::test_results::PATIENCE;

const TEXT: &str = "fn helper(a: u32) -> u32 {\n    a + 1\n}\n\nfn main() {\n    let total = helper(2);\n    println!(\"{total}\");\n}\n";

#[test]
fn the_pointers_rest_asks_once_and_shows_only_where_it_rests() {
    super::seeds("the_pointers_rest_asks_once_and_shows_only_where_it_rests", SEEDS, run);
}

#[derive(Default)]
struct World {
    /// Where the pointer is.
    at: Option<(usize, usize)>,
    /// The hover requests seen out (their ids).
    seen: Vec<u64>,
    /// The rests asked of (their numbers).
    asked: Vec<u64>,
    /// What a frame drew against the rules (`drawn`), said by the check after it.
    fault: Option<String>,
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("the_pointers_rest_asks_once_and_shows_only_where_it_rests", choices, LIMIT);
    let dir = crate::scratch::Scratch::new("sim-hoverrest");
    let file = dir.join("a.rs");
    std::fs::write(&file, TEXT).unwrap();
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = std::fs::canonicalize(dir.as_ref()).unwrap();
    ed.open(&file).unwrap();
    // (a stand-in that answers each hover with the place it was asked of)
    static TRAITS: std::sync::LazyLock<crate::lsp::ServerTraits> =
        std::sync::LazyLock::new(crate::lsp::ServerTraits::default);
    let serve: crate::lsp::Serve = Box::new(move |method, params| match method {
        "initialize" => {
            Some(json!({ "capabilities": { "positionEncoding": "utf-8", "hoverProvider": true } }))
        }
        "textDocument/hover" => {
            let p = &params["position"];
            let at = format!("asked at {}:{}", p["line"], p["character"]);
            Some(json!({ "contents": { "kind": "markdown", "value": at } }))
        }
        _ => None,
    });
    let cid = ed.lsp_own("stand-in", &TRAITS, serve);
    let id = ed.doc().id;
    if let Some(d) = ed.docs.iter_mut().find(|d| d.id == id) {
        d.lsp.attach(cid, false, true);
    }
    crate::term::render(&mut ed, &mut Vec::new(), 80, 20).unwrap();
    let world = RefCell::new(World::default());
    let check = |ed: &Editor| held(ed, &mut world.borrow_mut());
    sim.walk(&mut ed, INPUTS, |sim, ed| step(sim, ed, &mut world.borrow_mut()), check, print);
    assert!(!ed.hover_rest.armed(), "a timer left armed once all's quiet");
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn step(sim: &mut Sim, ed: &mut Editor, w: &mut World) -> String {
    let c = &mut sim.choices;
    // (by arm: the pointer anywhere 2, onto a name 4, onto the card 2, time 4, a key 1, an edit 1, a frame 3, the wheel 1)
    match c.weighted(&[2, 4, 2, 4, 1, 1, 3, 1]) {
        0 => {
            let (x, y) = (c.below(40), c.below(10));
            w.at = Some((x, y));
            ed.handle_event(Event::Hover(x as u16, y as u16));
            format!("the pointer at {x},{y}")
        }
        1 => {
            // (a name's letter as the last frame laid the text: its row under the pane's top line, after its gutter)
            let Some(&(_, rect, gutter, _)) = ed.screen.panes.first() else {
                return "(no pane drawn)".into();
            };
            let text = crate::chrome::pane_text(rect, ed.screen.text_top);
            let doc = ed.doc();
            let line = doc.top + c.below(text.h.min(8));
            let row = doc.text.get_line(line).map(|l| l.to_string()).unwrap_or_default();
            let names: Vec<usize> = row
                .char_indices()
                .filter(|&(i, ch)| {
                    crate::completion::is_word(ch) && (i == 0 || !row[..i].ends_with(char::is_alphanumeric))
                })
                .map(|(i, _)| i)
                .collect();
            if names.is_empty() {
                return format!("(no name on line {line})");
            }
            let col = *c.pick(&names) + c.below(2);
            let (x, y) = (rect.x + gutter + col, text.y + line - doc.top);
            w.at = Some((x, y));
            ed.handle_event(Event::Hover(x as u16, y as u16));
            format!("onto a name at {x},{y}")
        }
        2 => {
            let Some(r) = ed.screen.rest_card.get() else { return "(no card to go onto)".into() };
            let (x, y) = (r.0 + c.below(r.2), r.1 + c.below(r.3));
            w.at = Some((x, y));
            ed.handle_event(Event::Hover(x as u16, y as u16));
            format!("onto the card at {x},{y}")
        }
        3 => {
            let ms = *c.pick(&[100, 300, 450, 600, 2000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms}ms pass")
        }
        4 => {
            let k = *c.pick(&["j", "k", "esc"]);
            ed.handle_event(Event::Key(k.parse().unwrap()));
            format!("key {k}")
        }
        5 => {
            // (Claude's edit, say: the text moves on under a rest — not your key)
            let old = ed.doc().text.clone();
            let mut new = old.clone();
            new.insert(0, "// x\n");
            if let Some(change) = crate::disk::minimal_change(&old, &new) {
                ed.doc_mut().apply(&crate::transaction::Transaction::new(vec![change]));
            }
            "a line put at the top".into()
        }
        6 => {
            let width = *c.pick(&[80, 50]);
            crate::term::render(ed, &mut Vec::new(), width, 20).unwrap();
            w.fault = w.fault.take().or_else(|| drawn(ed, w).err());
            format!("a frame, {width} columns")
        }
        _ => {
            let Some((x, y)) = w.at else { return "(no pointer for the wheel)".into() };
            let kind = *c.pick(&[MouseKind::ScrollDown, MouseKind::ScrollUp]);
            ed.handle_event(Event::Mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false }));
            format!("the wheel at {x},{y}")
        }
    }
}

/// The rest's rules, after every step.
fn held(ed: &Editor, w: &mut World) -> Result<(), String> {
    let now = crate::clock::now();
    let rest = ed.hover_rest.rest.as_ref();
    // A hover asked: for the rest there now, `REST` after it came, once
    for (&(_, id), p) in &ed.lsp.pending {
        let Kind::PointerHover { n } = p.kind else { continue };
        if w.seen.contains(&id) {
            continue;
        }
        w.seen.push(id);
        let Some(r) = rest.filter(|r| r.n() == n) else {
            return Err(format!("rest {n} asked, not the one there"));
        };
        if now < r.since() + REST {
            return Err(format!("rest {n} asked {:?} after it came", now - r.since()));
        }
        if w.asked.contains(&n) {
            return Err(format!("rest {n} asked twice"));
        }
        w.asked.push(n);
    }
    w.fault.take().map_or(Ok(()), Err)
}

/// A frame just drawn: a card only for the rest under the pointer — on its name or the card — answered, saying the
/// place it was asked of.
fn drawn(ed: &Editor, w: &World) -> Result<(), String> {
    let Some(rect) = ed.screen.rest_card.get() else { return Ok(()) };
    let Some(r) = ed.hover_rest.rest.as_ref() else { return Err("a card drawn with nothing resting".into()) };
    let Some(card) = r.card.as_ref() else { return Err("a card drawn for a rest not answered".into()) };
    let doc = ed.docs.iter().find(|d| d.id == r.spot.doc).ok_or("its document gone")?;
    if doc.version() != r.spot.version {
        return Err("a card drawn for a version gone".into());
    }
    let line = crate::movement::line_of(&doc.text, r.spot.word.start);
    let col = r.spot.word.start - crate::movement::line_start(&doc.text, line);
    let says = format!("asked at {line}:{col}");
    let text: String = crate::markdown::wrap(&card.docs, 80, Default::default())
        .iter()
        .flat_map(|l| l.iter().map(|s| s.text.clone()))
        .collect();
    if r.spot.name && !text.contains(&says) {
        return Err(format!("the card says {text:?}, not {says}"));
    }
    let inside = |(x, y): (usize, usize)| {
        (rect.0..rect.0 + rect.2).contains(&x) && (rect.1..rect.1 + rect.3).contains(&y)
    };
    let (x0, y0) = r.spot.cell;
    let wide = doc.text.byte_slice(r.spot.word.clone()).to_string().width().max(1);
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
        Some(r) => format!("rest {} at {:?} card {}", r.n(), r.spot.word, r.card.is_some()),
        None => "no rest".into(),
    }
}
