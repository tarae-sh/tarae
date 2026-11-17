//! #610, landing 2: what a checkout already holds that git takes for a repository, simulated.
//!
//! `a_look_alike_found_at_first_trust_is_asked_once`: a checkout with one committed (`x/`), looked at as the git watch
//! and the git tab's pull do (`look_alikes_again`: the cached half, the store asked on a worker), its card answered
//! `y`, `n` or `esc` by keys, with frames and time between, in any order. After every step: one such card at most, it
//! holds nothing, and once `y` noted that version the card never comes again and `x/` stays noted. At the end the store
//! holds the note as a no exactly when `y` was given — never a trust.
//!
//! `the_untracked_scan_is_cut_and_said`: the first look's line through a stand-in git that never ends (`sleep`) — the
//! simulation's clock past `SCAN_LIMIT` stops it; the cached half alone is looked at, and the card says the cut.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::offer::{Offer, What};

const NAME: &str = "a_look_alike_found_at_first_trust_is_asked_once";

const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 20;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_look_alike_found_at_first_trust_is_asked_once() {
    super::seeds(NAME, SEEDS, run);
}

/// Where the scenario is: looks asked for, the keys given, and the version and place the card was about.
#[derive(Default)]
struct World {
    looks: usize,
    answers: Vec<&'static str>,
    about: Option<(crate::trust::Place, crate::trust::Digest)>,
    noted: bool,
}

/// git in the scenario's own repository — no user or system config, no hooks, no signing, a name of its own.
fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"])
        .args(["-c", "user.email=t@example.invalid", "-c", "user.name=t"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A repository at `base/repo` with `x/` committed, a folder git takes for a repository; `untracked` adds `u/`, one
/// not committed.
fn checkout(base: &Path, untracked: bool) -> PathBuf {
    let root = base.join("repo");
    let lay = |rel: &str| {
        let f = root.join(rel);
        std::fs::create_dir_all(f.join("objects/info")).unwrap();
        std::fs::create_dir_all(f.join("refs/heads")).unwrap();
        std::fs::write(f.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(f.join("objects/info/keep"), "").unwrap();
        std::fs::write(f.join("refs/heads/keep"), "").unwrap();
    };
    lay("x");
    for args in [&["init", "-q"][..], &["add", "-A"], &["commit", "-q", "-m", "base"]] {
        git(&root, args);
    }
    if untracked {
        lay("u");
    }
    root
}

fn editor(root: &Path, store: &Path) -> Editor {
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.to_path_buf();
    ed.trust_path = Some(store.to_path_buf());
    ed.workspace.viewer.keys_to_code();
    ed
}

fn run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-trustscan");
    let base = std::fs::canonicalize(scratch.path()).unwrap();
    let (root, store) = (checkout(&base, false), base.join("trust.json"));
    let world = RefCell::new(World::default());
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let mut ed = editor(&root, &store);
    let check = |ed: &Editor| held(ed, &root, &world.borrow());
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &mut world.borrow_mut()), check, print);
    let w = world.borrow();
    let yes = w.answers.contains(&"y");
    if let Some((place, version)) = w.about.clone() {
        sim.at_end(|| {
            let state = crate::trust::Store::read(Some(&store)).state_at(&place, version);
            match (yes, state) {
                (true, crate::trust::State::Ignored) | (false, crate::trust::State::Ask) => Ok(()),
                (yes, state) => Err(format!("y given {yes}, the store says {state:?}")),
            }
        });
    }
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// The look-alike card, in front and asking.
fn card(ed: &Editor) -> Option<&Offer> {
    ed.offers.first().filter(|o| o.kind() == "look-alike" && !o.installing())
}

fn input(sim: &mut Sim, ed: &mut Editor, w: &mut World) -> String {
    let c = &mut sim.choices;
    // (by arm: a look 2, an answer 3, a frame 1, time 1)
    match c.weighted(&[2, 3, 1, 1]) {
        0 => {
            w.looks += 1;
            ed.look_alikes_again();
            "the checkout looked at (the cached half)".into()
        }
        1 => match card(ed).map(|o| (o.id, o.rev, o.what.clone())) {
            Some((id, rev, what)) => {
                let k = *c.pick(&["y", "n", "esc"]);
                if let What::LookAlike(a) = what {
                    w.about = Some((a.place.clone(), a.version));
                }
                crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
                ed.offer_read_whole((id, rev));
                ed.card_focus = Some((id, rev));
                ed.handle_key(k.parse().unwrap());
                w.noted |= k == "y";
                w.answers.push(k);
                format!("{k} on the card")
            }
            None => "(no card to answer)".into(),
        },
        2 => {
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

/// One look-alike card at most, holding nothing; once noted, `x/` stays noted and its card doesn't come again.
fn held(ed: &Editor, root: &Path, w: &World) -> Result<(), String> {
    let cards = |l: &[Offer]| l.iter().filter(|o| o.kind() == "look-alike").count();
    if cards(&ed.offers) + cards(&ed.workspace.parked) > 1 {
        return Err("two look-alike cards up".into());
    }
    if !ed.workspace.made_protected.is_empty() {
        return Err("the card holds something".into());
    }
    if w.noted {
        if !ed.workspace.noted.contains(&root.join("x")) {
            return Err("y given, x/ not noted".into());
        }
        if ed.offers.iter().any(|o| o.kind() == "look-alike" && !o.installing()) {
            return Err("noted, and its card came again".into());
        }
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let up = ed.offers.iter().any(|o| o.kind() == "look-alike");
    format!("card {up} · noted {} · focus {}", ed.workspace.noted.len(), ed.card_focus.is_some())
}

const CUT: &str = "the_untracked_scan_is_cut_and_said";

/// #610, landing 2: the first look's line (the cached and untracked halves) through a stand-in git that never ends —
/// stopped once the simulation's clock passes `SCAN_LIMIT`; the cached half is looked at alone (`x/` named, the
/// untracked `u/` not), and the card says what wasn't looked through. Breaks if the limit goes (the run never goes
/// quiet).
#[test]
fn the_untracked_scan_is_cut_and_said() {
    super::seeds(CUT, &[0], cut_run);
}

fn cut_run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-trustscan-cut");
    let base = std::fs::canonicalize(scratch.path()).unwrap();
    let (root, store) = (checkout(&base, true), base.join("trust.json"));
    crate::snapshot::SLOW.lock().unwrap_or_else(|e| e.into_inner()).push(root.clone());
    let mut sim = Sim::new(CUT, choices, LIMIT);
    let mut ed = editor(&root, &store);
    let first = |_: &mut Sim, ed: &mut Editor| -> String {
        let root = ed.workspace.root.clone();
        ed.events.jobs().spawn(move || {
            let scan = crate::snapshot::scan(&root, None);
            move |ed: &mut Editor| ed.look_alikes_found(&root, scan.unwrap())
        });
        "the first look, its untracked half a git that never ends".into()
    };
    sim.walk(&mut ed, 1, first, |_| Ok(()), print);
    sim.at_end(|| {
        let o = card(&ed).ok_or_else(|| "no card".to_string())?;
        let q = &o.question;
        let cut = format!(
            "\nNot looked at: the untracked files took over {}.\n",
            crate::term::run_duration(crate::snapshot::SCAN_LIMIT)
        );
        match q.starts_with("x/ — ") && q.contains(&cut) && !q.contains("u/") {
            true => Ok(()),
            false => Err(format!("the card: {q}")),
        }
    });
    crate::snapshot::SLOW.lock().unwrap_or_else(|e| e.into_inner()).retain(|s| *s != root);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}
