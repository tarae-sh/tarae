//! A drag over a folded doc comment taller than the screen (#385, #386), simulated: presses, drags (inside the text
//! and past its edges), releases, the edge's ticks, the wheel and `y`, in any order, a frame after each as the main
//! loop draws one. After every step: a press or a drag inside the text leaves the view where it was (only the edge's
//! ticks and the wheel move it), the block stays folded (nothing edits it), and nothing is copied but by `y` — whose
//! register holds the selection's source, never the words as shown.

use std::cell::RefCell;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::event::{Mouse, MouseKind as K};
use crate::key::{Code, Key};

const NAME: &str = "a_drag_over_a_folded_doc_comment_moves_nothing_but_the_selection";

const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5];

const INPUTS: usize = 40;

const LIMIT: Duration = crate::test_results::PATIENCE;

const SIZE: (u16, u16) = (100, 30);

#[test]
fn a_drag_over_a_folded_doc_comment_moves_nothing_but_the_selection() {
    super::seeds(NAME, SEEDS, run);
}

#[derive(Default)]
struct World {
    wrong: Option<String>,
}

fn run(choices: Choices) -> Vec<String> {
    let world = RefCell::new(World::default());
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    let paras: String =
        (0..40).map(|i| format!("/// Paragraph {i}: the spool keeps its order.\n///\n")).collect();
    ed.docs[0].text = ropey::Rope::from_str(&format!("fn a() {{}}\n{paras}pub struct Spool;\nfn b() {{}}\n"));
    ed.docs[0].path.set(Some("/tmp/sim-docdrag.rs".into()));
    frame(&mut ed);
    // (its first rows wheeled above the screen)
    ed.view_by(30);
    frame(&mut ed);
    let check = |ed: &Editor| {
        if let Some(w) = world.borrow_mut().wrong.take() {
            return Err(w);
        }
        if !ed.view.iter().any(|r| matches!(r, crate::codeview::Row::Doc { .. })) && on_block(ed) {
            return Err("the block opened — nothing edited it".into());
        }
        let copied = ed.messages.iter().find(|m| m.2.contains("as shown"));
        copied.map_or(Ok(()), |m| Err(format!("copied as shown: {:?}", m.2)))
    };
    let input = |sim: &mut Sim, ed: &mut Editor| input(sim, ed, &mut world.borrow_mut());
    sim.walk(&mut ed, INPUTS, input, check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// Whether the view starts on or above the block's lines (1 to 80), so it's drawn — folded or not.
fn on_block(ed: &Editor) -> bool {
    ed.doc().top <= 80
}

fn frame(ed: &mut Editor) {
    crate::term::render(ed, &mut Vec::new(), SIZE.0, SIZE.1).unwrap();
}

fn view(ed: &Editor) -> (usize, usize) {
    (ed.doc().top, ed.doc().top_skip())
}

/// The code's rows as the last frame drew them, their first 60 cells (its top line and the status line left out: they
/// say where the cursor is; the right's toasts and scrollbar too) — the view as seen, however its top is kept (a line
/// inside a folded block, or its first with rows skipped).
fn drawn(ed: &Editor) -> Vec<String> {
    let (bytes, ..) = ed.last_frame.as_ref().expect("a frame");
    let text = crate::screen::text(bytes.as_slice(), SIZE.0, SIZE.1).unwrap();
    text.lines()
        .skip(1)
        .take(SIZE.1 as usize - 2)
        .map(|l| l.chars().take(60).collect::<String>().trim_end().to_string())
        .collect()
}

fn input(sim: &mut Sim, ed: &mut Editor, w: &mut World) -> String {
    let (_, rect, gutter, _) = ed.screen.panes.first().cloned().expect("a code pane");
    let c = &mut sim.choices;
    // (a cell of the text, or — a drag — past its top or bottom edge)
    let inside =
        |c: &mut Choices| (rect.x + gutter + c.below(40), rect.y + 1 + c.below(rect.h.saturating_sub(2)));
    let mouse = |ed: &mut Editor, kind, (x, y): (usize, usize)| {
        ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
        frame(ed);
    };
    // (by arm: a press 3, a drag inside 4, a drag past an edge 1, a release 2, the edge's tick 2, the wheel 1, `y` 1)
    let before = drawn(ed);
    let said = match c.weighted(&[3, 4, 1, 2, 2, 1, 1]) {
        0 => {
            let at = inside(c);
            mouse(ed, K::Down, at);
            format!("a press at {at:?}")
        }
        1 => {
            let at = inside(c);
            mouse(ed, K::Drag, at);
            format!("a drag to {at:?}")
        }
        2 => {
            let y = if c.below(2) == 0 { rect.y.saturating_sub(1) } else { rect.y + rect.h };
            mouse(ed, K::Drag, (rect.x + gutter + 4, y));
            return format!("a drag past the edge, row {y}");
        }
        3 => {
            let at = inside(c);
            mouse(ed, K::Up, at);
            format!("let go at {at:?}")
        }
        4 => {
            sim.wait(crate::mouse::EDGE_EVERY);
            let moved = crate::mouse::edge_tick(ed);
            frame(ed);
            return format!("the edge's tick{}", if moved { ", a row on" } else { "" });
        }
        5 => {
            let by = *c.pick(&[-3isize, 3]);
            ed.view_by(by);
            frame(ed);
            return format!("the wheel {by}");
        }
        _ => {
            ed.handle_key(Key::plain(Code::Char('y')));
            frame(ed);
            let d = ed.doc();
            let r = d.selection().primary();
            // (a cursor alone: the character under it, as Helix yanks it)
            let to = r.to().max(crate::graphemes::next_boundary(&d.text, r.from()));
            let source = d.text.byte_slice(r.from()..to).to_string();
            if ed.registers.get(&'"').is_some_and(|y| y.first() != Some(&source)) {
                w.wrong = Some(format!("y yanked {:?}, not the source {source:?}", ed.registers.get(&'"')));
            }
            return "y".into();
        }
    };
    // (a press or a drag inside the text: the view where it was)
    // (a press or a drag inside the text: the view where it was — but the scroll margin's rows, as on code: one in the
    // margin keeps it)
    let now = drawn(ed);
    let so = ed.config.scrolloff as isize;
    match shift(&before, &now) {
        Some(k) if k.abs() <= so => {}
        k => w.wrong = Some(format!("{said}: the view moved {k:?} rows, past the margin's {so}")),
    }
    said
}

/// How many rows `now` is `before` moved up (negative: down) — the rows they share the same; None: none of them.
fn shift(before: &[String], now: &[String]) -> Option<isize> {
    let n = before.len() as isize;
    (0..n).flat_map(|k| [k, -k]).find(|&k| {
        (0..n).filter(|i| (0..n).contains(&(i + k))).all(|i| before[(i + k) as usize] == now[i as usize])
    })
}

/// What a trace line ends with: the view, the selection's lines, whether the block is drawn folded.
fn print(ed: &Editor) -> String {
    let d = ed.doc();
    let r = d.selection().primary();
    let (a, b) = (d.text.byte_to_line(r.from()), d.text.byte_to_line(r.to()));
    let folded = ed.view.iter().any(|r| matches!(r, crate::codeview::Row::Doc { .. }));
    format!("view {:?} · selected {}..{} · {}", view(ed), a + 1, b + 1, if folded { "folded" } else { "raw" })
}
