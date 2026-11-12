//! An install's "colors are on" said only with the colours (#551), simulated: two Rust files and a text file, an install of
//! the Rust grammar landing (`Editor::grammars_installed`, a stand-in for the build: the Rust files without colours
//! first, as before a grammar), switches between the files, a key typed, a file's load failing, frames and time passing
//! (the wait's said step, past `opening::SAY_AFTER`) — the loads and parses landing in any order among them. After every
//! step: in a step that said the words, every file the install found in sight without colours (and whose load didn't
//! fail) has its tree; the wait is on the status line only once said, with frames asked for its timer. At the end the
//! words of the last install were said and nothing waits.

use std::cell::RefCell;
use std::path::Path;
use std::time::Duration;

use super::{Choices, Sim};
use crate::document::DocId;
use crate::editor::Editor;
use crate::key::Key;

const NAME: &str = "a_grammars_colors_are_said_only_with_them";

const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 40;

const LIMIT: Duration = crate::test_results::PATIENCE;

const SIZE: (u16, u16) = (100, 24);

const FILES: &[&str] = &["a.rs", "b.rs", "notes.txt"];

const WORDS: &str = "colors are on";

#[test]
fn a_grammars_colors_are_said_only_with_them() {
    if crate::syntax::Loader::global().load(rust()).is_err() {
        return;
    }
    super::seeds(NAME, SEEDS, run);
}

fn rust() -> &'static crate::syntax::LangSpec {
    crate::syntax::spec("rust").expect("rust's pack")
}

/// The scenario's own account: the files the last install found in sight without colours, less those whose load failed;
/// whether its words are still to come; how many times they were said before the step; what a step found wrong.
#[derive(Default)]
struct Model {
    waiting: Vec<DocId>,
    owed: bool,
    said: usize,
    wrong: Option<String>,
}

fn run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-grammar-colours");
    let root = std::fs::canonicalize(scratch.path()).unwrap();
    for f in FILES {
        std::fs::write(root.join(f), format!("// {f}\nfn {}() {{}}\n", f.replace('.', "_"))).unwrap();
    }
    let model = RefCell::new(Model::default());
    let mut sim = Sim::new(NAME, choices, LIMIT);
    sim.frames = Some(SIZE);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    for f in FILES.iter().rev() {
        ed.open(&root.join(f)).unwrap();
    }
    let check = |ed: &Editor| check(ed, &mut model.borrow_mut());
    // (each input an event's, as the loop takes it: the work every event ends with after it — parses started, the wait
    // looked at — and a frame when one is due)
    let input = |sim: &mut Sim, ed: &mut Editor| {
        let said = input(sim, ed, &root, &mut model.borrow_mut());
        sim.batch(ed, crate::event::Event::Job(Box::new(|_| {})));
        said
    };
    sim.walk(&mut ed, INPUTS, input, check, print);
    let (owed, left) = (model.borrow().owed, ed.colouring.is_some());
    sim.at_end(|| match (owed, left) {
        (_, true) => Err("all quiet, and an install's words still wait".into()),
        (true, _) => Err("all quiet, and the last install's words never said".into()),
        _ => Ok(()),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn said(ed: &Editor) -> usize {
    ed.messages.iter().filter(|m| m.2.contains(WORDS)).count()
}

fn in_sight(ed: &Editor, id: DocId) -> bool {
    id == ed.doc().id || ed.workspace.views.iter().any(|v| v.doc == id)
}

fn input(sim: &mut Sim, ed: &mut Editor, root: &Path, m: &mut Model) -> String {
    let c = &mut sim.choices;
    m.said = said(ed);
    // (by arm: an install landing 2, a switch 3, a key 1, a load failing 1, a frame 1, time passing 2)
    match c.weighted(&[2, 3, 1, 1, 1, 2]) {
        0 => {
            // (before the grammar: the Rust files without colours)
            let rs: Vec<DocId> = ed
                .docs
                .iter()
                .filter(|d| d.path.as_deref().is_some_and(|p| p.extension().is_some_and(|x| x == "rs")))
                .map(|d| d.id)
                .collect();
            for d in ed.docs.iter_mut().filter(|d| rs.contains(&d.id)) {
                d.syntax = None;
            }
            // (what's claimed of them: those in sight — one out of sight the stand-in took the colours of is no file the
            // words speak of)
            m.waiting.retain(|d| !rs.contains(d));
            m.waiting.extend(rs.into_iter().filter(|&id| in_sight(ed, id)));
            m.owed = true;
            let built =
                [crate::grammar::Progress::Done { name: rust().grammar.clone(), took: Duration::ZERO }];
            ed.grammars_installed(&built, Ok(()), None);
            "the Rust grammar's install lands".into()
        }
        1 => {
            let f = *c.pick(FILES);
            ed.open(&root.join(f)).unwrap();
            format!("to {f}")
        }
        2 => {
            for k in ["i", "x", "esc"] {
                ed.handle_key(k.parse::<Key>().unwrap());
            }
            "x typed".into()
        }
        3 => {
            let id = ed.doc().id;
            ed.language_loaded(id, rust(), Err("the grammar's library didn't load".into()));
            m.waiting.retain(|&d| d != id);
            format!("{}'s load fails", ed.doc().display_name_short())
        }
        4 => {
            crate::term::render(ed, &mut Vec::new(), SIZE.0, SIZE.1).unwrap();
            "a frame".into()
        }
        _ => {
            let ms = *c.pick(&[100, 400, 2_000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

fn check(ed: &Editor, m: &mut Model) -> Result<(), String> {
    if let Some(w) = m.wrong.take() {
        return Err(w);
    }
    let now = said(ed);
    if now > m.said {
        m.owed = false;
        for &id in &m.waiting {
            let Some(d) = ed.docs.iter().find(|d| d.id == id) else { continue };
            if d.syntax.as_ref().is_none_or(|s| s.tree.is_none()) {
                return Err(format!("'{WORDS}' said, {} not coloured yet", d.display_name_short()));
            }
        }
        m.waiting.clear();
    }
    m.said = now;
    if ed.colouring_said().is_some() && ed.tick_wanted().is_none() {
        return Err("the wait said with its timer, and no frame asked for it".into());
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let trees: Vec<String> = ed
        .docs
        .iter()
        .map(|d| {
            let s = match &d.syntax {
                None => "-",
                Some(s) if s.tree.is_some() => "tree",
                Some(s) if s.in_flight => "parsing",
                Some(_) => "loaded",
            };
            format!("{}:{s}", d.display_name_short())
        })
        .collect();
    let wait = ed.colouring_said().map_or(String::new(), |(w, _)| format!(" · {w}"));
    format!("{}{wait} · said {}", trees.join(" "), said(ed))
}
