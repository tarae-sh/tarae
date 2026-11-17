//! #610: a folder git takes for a repository a run of Claude's made, held on the protected-made card, simulated: the run
//! is taken before it runs (`snapshot::Before::take` with what's remembered, on a worker), it makes `x/` one (`HEAD`,
//! `objects/`, `refs/`), it ends (`tools::run_changes`), what it held up asks (the paused servers' card), and the card is
//! answered `y`, `r` or `n` by keys — with frames and time between, in any order. After every step: the folder itself and
//! its two folders are never removed, a look-alike waiting still holds its `HEAD`, a noted one waits on nothing; at the
//! end, the run's look-alike was named and is waiting, noted, or had its `HEAD` removed — never let go unanswered.

use std::cell::RefCell;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::key::{Code, Key};

const NAME: &str = "a_look_alike_a_run_made_is_held_until_answered";

const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 24;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_look_alike_a_run_made_is_held_until_answered() {
    super::seeds(NAME, SEEDS, run);
}

/// Where the scenario is: the run (none · taken · written · ended), its snapshot as its job lands it, the answers.
#[derive(Default)]
struct World {
    run: u8,
    snap: Arc<Mutex<Option<crate::snapshot::Before>>>,
    answers: Vec<char>,
}

fn run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-lookalike");
    let root = std::fs::canonicalize(scratch.path()).unwrap();
    std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    for args in [&["init", "-q"][..], &["add", "-A"], &["commit", "-q", "-m", "base"]] {
        git(&root, args);
    }
    let world = RefCell::new(World::default());
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    let mut c = crate::chat::Chat::new();
    let _claude = crate::chat::stand_in(&mut c);
    ed.workspace.claude.chat = Some(c);
    ed.workspace.viewer.keys_to_code();
    let check = |ed: &Editor| held(ed, &root, &world.borrow());
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &root, &mut world.borrow_mut()), check, print);
    let w = world.borrow();
    let x = root.join("x");
    let waits = ed.workspace.made_protected.iter().any(|m| m.look.is_some() && m.path == "x");
    let gone = std::fs::symlink_metadata(x.join("HEAD")).is_err();
    if w.run == 3 && !waits && !ed.workspace.noted.contains(&x) && !gone {
        sim.fail_now(format!(
            "x/ was made one and is neither waiting, noted nor removed (answers {:?})",
            w.answers
        ));
    }
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// git in the scenario's own repository — no hooks, no signing, a name of its own.
fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"])
        .args(["-c", "user.email=t@example.invalid", "-c", "user.name=t"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// The front card, when it's the protected-made one and nothing is answering it.
fn made_card(ed: &Editor) -> Option<(u64, u64)> {
    let o = ed.offers.first().filter(|o| o.made() && !o.installing())?;
    Some((o.id, o.rev))
}

fn input(sim: &mut Sim, ed: &mut Editor, root: &Path, w: &mut World) -> String {
    let c = &mut sim.choices;
    // (by arm: the run goes on 4, what it held asks 1, an answer 3, a frame 1, time 1)
    match c.weighted(&[4, 1, 3, 1, 1]) {
        0 => match w.run {
            0 => {
                w.run = 1;
                let (root, slot) = (root.to_path_buf(), w.snap.clone());
                let (heads, noted) = (ed.workspace.heads.clone(), ed.workspace.noted.clone());
                ed.events.jobs().spawn(move || {
                    let snap = crate::snapshot::Before::take(&root).looked(&heads, &noted);
                    move |_: &mut Editor| *slot.lock().unwrap() = Some(snap)
                });
                "Claude's run is taken before it runs".into()
            }
            1 if w.snap.lock().unwrap().is_some() => {
                w.run = 2;
                for d in ["x/objects", "x/refs"] {
                    std::fs::create_dir_all(root.join(d)).unwrap();
                }
                std::fs::write(root.join("x/HEAD"), "ref: refs/heads/main\n").unwrap();
                "the run makes x/ a folder git takes for a repository".into()
            }
            1 => "(its snapshot isn't taken yet)".into(),
            2 => {
                w.run = 3;
                let snap = w.snap.lock().unwrap().take().unwrap();
                let generation = ed.workspace.claude.chat.as_ref().map_or(0, |c| c.generation());
                crate::tools::run_changes(ed, snap, generation, "make".into(), |_: &mut Editor, _| {});
                "the run ends".into()
            }
            _ => "(the run is over)".into(),
        },
        // (what waits on the card — here the checkout's servers — asks, as `made_hold_sync` does)
        1 if !ed.workspace.made_protected.is_empty() && ed.offers.is_empty() => {
            crate::tools::ask_made(ed, crate::tools::MadeThen::Servers);
            "what it made holds the servers: the card".into()
        }
        1 => "(nothing waits, or a card is up)".into(),
        2 => match made_card(ed) {
            Some(o) => {
                let k = *c.pick(&['y', 'r', 'n']);
                crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
                ed.offer_read_whole(o);
                ed.card_focus = Some(o);
                ed.handle_key(Key::plain(Code::Char(k)));
                w.answers.push(k);
                format!("{k} on the card")
            }
            None => "(no card to answer)".into(),
        },
        3 => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            "a frame".into()
        }
        _ => {
            let ms = *c.pick(&[1, 30, 300]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

/// The folder and its two folders stay once made (`r` never reaches them), a look-alike waiting still holds its
/// `HEAD`, and a noted one waits on nothing.
fn held(ed: &Editor, root: &Path, w: &World) -> Result<(), String> {
    let x = root.join("x");
    if w.run >= 2 && !(x.join("objects").is_dir() && x.join("refs").is_dir()) {
        return Err("x/ or its objects/ and refs/ removed — never the folder".into());
    }
    let waits = ed.workspace.made_protected.iter().any(|m| m.look.is_some());
    // (an `r` under way: its job removed it, its landing not in yet)
    let answering = ed.offers.first().is_some_and(|o| o.made() && o.installing());
    if waits && !answering && std::fs::symlink_metadata(x.join("HEAD")).is_err() {
        return Err("x/HEAD removed, and x/ still waits on the card".into());
    }
    if waits && ed.workspace.noted.contains(&x) {
        return Err("x/ noted and still waiting".into());
    }
    Ok(())
}

/// What a trace line ends with: what waits, what's noted, the card up.
fn print(ed: &Editor) -> String {
    let waits: Vec<&str> = ed.workspace.made_protected.iter().map(|m| m.path.as_str()).collect();
    let card = ed.offers.first().map_or("none", |o| if o.made() { "protected-made" } else { "another" });
    format!("waits {waits:?} · noted {} · card {card}", ed.workspace.noted.len())
}
