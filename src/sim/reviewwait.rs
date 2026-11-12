//! An edit review Claude raised takes no key before it's focused (#528), simulated: a sentence typed into the chat's
//! input a key a step — it holds `a`, `y`, `n`, `q`, `j`, `tab` and a space —, Claude's edit of a protected file
//! landing mid-sentence (`refactor::land`: `show`'s path, as #528's `Cargo.toml`), frames at two sizes (the chat beside
//! the code, and a screen too narrow for both, the chat taking it whole), time passing, `C-q`, `esc`, a press on the
//! review, `y` / `a` / `n` / `j` once it's focused, and now and then `enter`, the sentence sent (queued while Claude
//! works — then the sentence's `tab`, the steer key, steers it: the input stays empty). And the reviewed file's
//! grammar (#534/#538): its language loaded with the grammar missing (a stand-in language whose grammar is never here),
//! and an install of it landing (`Editor::grammars_here`).
//!
//! The scenario keeps its own model from its own steps: whether it focused the review, and whether a frame has drawn
//! the review in the code area since (`C-q` on the narrow screen gives the code the screen, a frame later). After every
//! step: no change is decided unless the model focused the review and a frame drew it before the answer; while the
//! model hasn't focused it, every key typed lands in the chat's input, after what was there, in order — and none does
//! while it has; a sentence sent arrives whole as the chat's message; the review has the keys exactly when the model
//! says. The grammar's: while the review is drawn in the code area its file holds no offer back, a load that failed for
//! the missing grammar there was offered (offers are a status line under `cfg(test)`: the decision, not the card), and
//! an install landing leaves no file holding that grammar's offer.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::key::Key;

const NAME: &str = "an_edit_review_claude_raised_takes_no_key_before_its_focused";

const SEEDS: &[u64] = &[0, 1, 2, 7];

const INPUTS: usize = 60;

const LIMIT: Duration = crate::test_results::PATIENCE;

/// The chat beside the code, and a screen too narrow for both.
const WIDE: (u16, u16) = (140, 30);
const NARROW: (u16, u16) = (60, 30);

/// The sentence, a key at a time (the keys an answer or a move would be, among its letters).
const SENTENCE: &[&str] = &[
    "S", "a", "y", "space", "y", "e", "s", "space", "a", "n", "d", "space", "q", "u", "i", "t", "tab", "j",
    "o", "g", "space", "o", "n", ".",
];

#[test]
fn an_edit_review_claude_raised_takes_no_key_before_its_focused() {
    super::seeds(NAME, SEEDS, run);
}

/// What the scenario knows from its own steps.
#[derive(Default)]
struct Model {
    /// It focused the review (`C-q`, a press) — let go by `esc` or its answer.
    focused: bool,
    /// The last frame drew the review in the code area.
    drawn: bool,
    /// A frame drew it in the code area since the model focused it, or just before (read there before `C-q`).
    seen: bool,
    frames: u64,
    /// The next key of the sentence.
    at: usize,
    /// Landings made, for each one's text.
    landings: usize,
}

impl Model {
    /// A frame drawn since: whether it drew the review in the code area.
    fn sync(&mut self, ed: &Editor) {
        if ed.frames != self.frames {
            self.frames = ed.frames;
            self.drawn = ed.screen.review_area.get().is_some();
            if self.focused && self.drawn {
                self.seen = true;
            }
        }
    }
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-reviewwait");
    std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(dir.join("aside.txt"), "x\n").unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut sim = Sim::new(NAME, choices, LIMIT);
    sim.frames = Some(WIDE);
    let mut ed = Editor::with_root(crate::config::Config::default(), root.clone());
    let mut c = crate::chat::Chat::new();
    let _claude = crate::chat::stand_in(&mut c);
    ed.workspace.claude.chat = Some(c);
    ed.workspace.set_at(crate::workspace::Here::Chat);
    crate::term::render(&mut ed, &mut Vec::new(), WIDE.0, WIDE.1).unwrap();
    let model = RefCell::new(Model { frames: ed.frames, ..Model::default() });
    let wrong = Cell::new(None);
    let check = |ed: &Editor| held(ed, &model.borrow(), &wrong);
    let input = |sim: &mut Sim, ed: &mut Editor| {
        let said = step(sim, ed, &root, &model, &wrong);
        model.borrow_mut().sync(ed);
        said
    };
    sim.walk(&mut ed, INPUTS, input, check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// A language whose grammar is never here — the reviewed file loaded in it fails as a missing grammar does.
fn missing() -> &'static crate::syntax::LangSpec {
    static SPEC: std::sync::OnceLock<crate::syntax::LangSpec> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| {
        let mut lang = crate::syntax::spec("lua").expect("lua's pack").clone();
        (lang.name, lang.grammar) = ("tarae-sim-lang".into(), "tarae-sim-no-such-grammar".into());
        lang
    })
}

/// The review up: (its generation, its changes still undecided).
fn review(ed: &Editor) -> Option<(u64, usize)> {
    let r = ed.workspace.claude.review.as_ref().filter(|r| r.ready)?;
    Some((r.generation, r.hunks.iter().filter(|h| h.decision == crate::llm::Decision::Undecided).count()))
}

fn input_of(ed: &Editor) -> String {
    ed.workspace.claude.chat.as_ref().map(|c| c.input.text().to_string()).unwrap_or_default()
}

/// Key `k` as a batch of its own (a frame after it when one is due), judged and followed by the model.
fn key(sim: &Sim, ed: &mut Editor, model: &RefCell<Model>, wrong: &Cell<Option<String>>, k: &str) {
    key_framed(sim, ed, model, wrong, k, true);
}

/// Key `k` — a batch of its own when `framed`, else the first of a batch the next key ends (no frame between).
fn key_framed(
    sim: &Sim,
    ed: &mut Editor,
    model: &RefCell<Model>,
    wrong: &Cell<Option<String>>,
    k: &str,
    framed: bool,
) {
    let (before, typed) = (review(ed), input_of(ed));
    let busy = ed.workspace.claude.chat.as_ref().is_some_and(|c| c.busy());
    // (the message `enter` queued while Claude works, the input empty: `tab` — the steer key — steers it)
    let queued = ed.workspace.claude.chat.as_ref().and_then(|c| {
        let steers = crate::chat::steerable(c, ed.chat_asking()) == Some(crate::chat::Steerable::Queued);
        c.queued.clone().filter(|_| steers)
    });
    let event = crate::event::Event::Key(k.parse::<Key>().unwrap());
    match framed {
        true => sim.batch(ed, event),
        false => ed.handle_event(event),
    }
    let mut m = model.borrow_mut();
    let after = review(ed);
    let decided =
        before.is_some_and(|(g, left)| left > 0 && after.is_none_or(|(g2, l2)| g2 != g || l2 < left));
    if decided && !(m.focused && m.seen) {
        let why = if m.focused { "focused, but no frame drew it" } else { "never focused" };
        wrong.set(Some(format!("{k} decided a change — {why}")));
    }
    let now = input_of(ed);
    match (m.focused, k) {
        // (its answer: the review ends, the keys back in the chat)
        (true, _) if decided && after.is_none() => m.focused = false,
        (true, "esc") => m.focused = false,
        (true, _) if now != typed => {
            wrong.set(Some(format!("{k}, the review focused, typed into the chat: {now:?}")))
        }
        (true, _) => {}
        (false, "C-q") if before.is_some() => (m.focused, m.seen) = (true, m.drawn),
        (false, "enter") if !busy && !typed.trim().is_empty() => {
            let sent = ed.workspace.claude.chat.as_ref().and_then(|c| {
                c.msgs.iter().rev().find(|m| m.role == crate::chat::Role::User).map(|m| m.text.clone())
            });
            // (sent as the chat sends any message: its ends trimmed)
            if sent.as_deref() != Some(typed.trim()) || !now.is_empty() {
                let said = ed.messages.last().map(|m| m.2.clone());
                wrong.set(Some(format!("enter sent {sent:?} of {typed:?}, left {now:?} — said {said:?}")));
            }
        }
        (false, "enter" | "C-q" | "esc") => {}
        // (a queued sentence steered: it leaves the queue for the turn under way, the input left empty)
        (false, "tab") if let Some(q) = queued => {
            let c = ed.workspace.claude.chat.as_ref().unwrap();
            if !now.is_empty() || c.queued.is_some() {
                wrong.set(Some(format!("tab, {q:?} queued, left the input {now:?}, queued {:?}", c.queued)));
            }
        }
        // (a letter at the input's end; a space and `tab` too, not in sight as text — what was there kept)
        (false, k) => {
            let grew = now.len() > typed.len() && now.starts_with(&typed);
            let letter = k.chars().count() == 1 && !now.ends_with(k);
            if !grew || letter {
                wrong.set(Some(format!("{k} typed: the input {typed:?} became {now:?}")));
            }
        }
    }
    m.sync(ed);
}

/// Claude's edit of `Cargo.toml` as it stands now: a line more.
fn landing(ed: &mut Editor, root: &Path, n: usize) {
    let path: PathBuf = root.join("Cargo.toml");
    let before = match ed.docs.iter().find(|d| d.path.as_deref() == Some(path.as_path())) {
        Some(d) => d.text.clone(),
        None => ropey::Rope::from_str(&std::fs::read_to_string(&path).unwrap()),
    };
    let mut after = before.clone();
    after.insert(after.len_chars(), &format!("line{n} = {n}\n"));
    let file = crate::refactor::FileChange {
        path,
        rel: "Cargo.toml".into(),
        before,
        after,
        places: 1,
        op: crate::refactor::Op::Text,
        protected: true,
    };
    let call = crate::tools::Call::test(n as u64);
    crate::refactor::land(ed, call, "edit Cargo.toml".into(), "change", vec![file], String::new());
}

fn step(
    sim: &mut Sim,
    ed: &mut Editor,
    root: &Path,
    model: &RefCell<Model>,
    wrong: &Cell<Option<String>>,
) -> String {
    let c = &mut sim.choices;
    let focused = model.borrow().focused;
    // (by arm: a key of the sentence (or, focused, of the review) 8, Claude's edit landing 2, `C-q` 2, `esc` 1, a
    // press on the review 1, the screen's size 1, a frame 1, time passing 1, `enter` 1, `C-q` and an answer in one
    // batch 1, the reviewed file's language loaded with its grammar missing 1, an install of that grammar landing 1)
    match c.weighted(&[8, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1]) {
        // (in the reviewed file, or in one Claude's turn opened aside — follow's, never in front: held, #208)
        10 if c.below(2) == 0 => {
            let Some(doc) = ed.workspace.claude.review.as_ref().map(|r| r.doc_id) else {
                return "(no review: no file to load)".into();
            };
            ed.load_language(doc, missing());
            "the reviewed file's language loads, its grammar missing".into()
        }
        10 => {
            let front = ed.current;
            ed.open_as(&root.join("aside.txt"), crate::editor::Opener::Claude).unwrap();
            let doc = ed.doc().id;
            ed.current = front;
            // (its load's result as the loader's job brings it, in this step: held now, for an install to land on)
            let no = format!("no grammar '{}' — :grammar-install", missing().grammar);
            ed.language_loaded(doc, missing(), Err(no));
            "a file Claude's turn opened aside loads, its grammar missing".into()
        }
        11 => {
            ed.grammars_here(std::slice::from_ref(&missing().grammar));
            if let Some(d) =
                ed.docs.iter().find(|d| d.grammar_held.is_some_and(|s| s.grammar == missing().grammar))
            {
                wrong.set(Some(format!(
                    "an install landed, {} still holds its offer",
                    d.display_name_short()
                )));
            }
            "an install of the missing grammar lands".into()
        }
        9 if focused => "(focused: no C-q)".into(),
        9 => {
            // (typed fast: no frame between — on the narrow screen the code, and the review, only come with the next)
            let k = *c.pick(&["y", "a", "n"]);
            key_framed(sim, ed, model, wrong, "C-q", false);
            key(sim, ed, model, wrong, k);
            format!("keys C-q {k} in one batch")
        }
        0 if focused => {
            let k = *c.pick(&["y", "a", "n", "j", "j"]);
            key(sim, ed, model, wrong, k);
            format!("key {k}, the review focused")
        }
        0 => {
            let k = {
                let mut m = model.borrow_mut();
                let k = SENTENCE[m.at % SENTENCE.len()];
                m.at += 1;
                k
            };
            key(sim, ed, model, wrong, k);
            format!("key {k} of the sentence")
        }
        1 => {
            let pending = ed.workspace.claude.chat.as_ref().is_some_and(|c| !c.edits.sets.is_empty());
            if ed.workspace.claude.review.is_some() || pending {
                return "(a review up or coming: no other edit)".into();
            }
            let n = {
                let mut m = model.borrow_mut();
                m.landings += 1;
                (m.focused, m.seen) = (false, false);
                m.landings
            };
            landing(ed, root, n);
            format!("Claude's edit {n} of Cargo.toml lands")
        }
        2 => {
            key(sim, ed, model, wrong, "C-q");
            "key C-q".into()
        }
        // (from the review only: the chat's own `esc` gives the keys to the code, out of the sentence's way)
        3 if !focused => "(esc: the review not focused)".into(),
        3 => {
            key(sim, ed, model, wrong, "esc");
            "key esc".into()
        }
        4 => {
            // (on its rows in the code area, or its peek — a cell the review is under, not a toast over it)
            let cells: Vec<(usize, usize)> = match (ed.screen.review_area.get(), ed.screen.review_peek.get())
            {
                (Some((x, y, w, h)), _) => (y..y + h).flat_map(|r| (x..x + w).map(move |c| (c, r))).collect(),
                (None, Some((x, y, w))) => (x..x + w).map(|c| (c, y)).collect(),
                _ => return "(no review drawn to press)".into(),
            };
            let on = |&(x, y): &(usize, usize)| {
                ed.mouse_under(crate::event::MouseKind::Down, x, y).surface
                    == crate::mouse::Surface::EditReview
            };
            let on: Vec<_> = cells.into_iter().filter(on).collect();
            if on.is_empty() {
                return "(the review drawn, all of it under something)".into();
            }
            let at = on[c.below(on.len())];
            for kind in [crate::event::MouseKind::Down, crate::event::MouseKind::Up] {
                let m = crate::event::Mouse { kind, x: at.0 as u16, y: at.1 as u16, alt: false, ctrl: false };
                sim.batch(ed, crate::event::Event::Mouse(m));
            }
            let mut m = model.borrow_mut();
            if !m.focused {
                (m.focused, m.seen) = (true, m.drawn);
            }
            format!("a press on the review at {},{}", at.0, at.1)
        }
        5 => {
            let size = if sim.frames == Some(WIDE) { NARROW } else { WIDE };
            sim.frames = Some(size);
            crate::term::render(ed, &mut Vec::new(), size.0, size.1).unwrap();
            format!("the screen {}×{}", size.0, size.1)
        }
        6 => {
            let (w, h) = sim.frames.unwrap_or(WIDE);
            crate::term::render(ed, &mut Vec::new(), w, h).unwrap();
            "a frame".into()
        }
        7 => {
            let ms = *c.pick(&[50, 300, 1200]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        8 if focused => "(focused: no enter)".into(),
        _ => {
            key(sim, ed, model, wrong, "enter");
            "key enter".into()
        }
    }
}

/// No key judged wrong; the review has the keys exactly when the model focused it.
fn held(ed: &Editor, m: &Model, wrong: &Cell<Option<String>>) -> Result<(), String> {
    if let Some(why) = wrong.take() {
        return Err(why);
    }
    let focused = ed.workspace.claude.review.as_ref().is_some_and(|r| r.focused);
    if focused != m.focused {
        return Err(format!("the review focused: {focused}, the model says {}", m.focused));
    }
    // (#534: a file whose code you saw holds nothing back; its code in sight — a load that failed for the grammar offered)
    if let Some(d) = ed.docs.iter().find(|d| d.grammar_held.is_some() && d.grammar_seen) {
        return Err(format!("{} holds its grammar's offer back, its code seen", d.display_name_short()));
    }
    let shown = ed.workspace.claude.review.as_ref().filter(|r| r.covers()).map(|r| r.doc_id);
    if let Some(d) = shown.and_then(|id| ed.docs.iter().find(|d| d.id == id)) {
        if d.grammar_held.is_some() {
            return Err("the review drawn in the code area, its file holds its grammar's offer back".into());
        }
        if d.language_failed && !ed.grammar_hinted.contains(&missing().grammar) {
            return Err("the review drawn in the code area, its grammar missing, nothing offered".into());
        }
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let r = ed.workspace.claude.review.as_ref().map(|r| (r.ready, r.focused, r.folded, r.seen));
    let held = ed.docs.iter().filter(|d| d.grammar_held.is_some()).count();
    let offered = ed.grammar_hinted.contains(&missing().grammar);
    format!(
        "review {r:?} · input {:?} · keys {:?} · grammar held {held}, offered {offered}",
        input_of(ed),
        ed.keys_to().region
    )
}
