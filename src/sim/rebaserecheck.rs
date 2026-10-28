//! A resolved rebase's card read again, simulated (#206): two sessions' rebases resolved at a stop, a tracked file
//! changed besides the conflict holding each card's `y` — and at random the card put up (as `:workspace-bring`
//! does), focused, answered (`y` held and refused, read again; `X`; `n`), a frame, that file staged, unstaged,
//! undone or changed again in its checkout, the git tab's status read (read again too), the rebase going on and
//! resolved at its next stop (whose conflicted file is the one that held the stop before), a switch, the user
//! taking their time — each read again's job and its landing interleaved by seed. `y` and `X` start no git: a run
//! in each task panel stands in, so the answer's rebase stays where it was as its line waits. After every step:
//! a resolved card is up only as `rebase_offer` put it (an answered one never comes back); what a rebase says
//! changed besides its conflict is never its stop's own conflicted file; a card with the keys says what it said
//! when drawn.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::sessions::{World, git, print};
use super::{Choices, Sim};
use crate::editor::Editor;
use crate::event::WorkspaceId;
use crate::key::{Code, Key};
use crate::offer::{Offer, What};
use crate::rebase::{Broken, Stage};

const SEEDS: &[u64] = &[0, 1];

const INPUTS: usize = 40;

const LIMIT: Duration = crate::test_results::PATIENCE;

/// What the scenario remembers between steps: the checkouts whose card `rebase_offer` put up and no answer or
/// move took away · those whose card `y` or `X` took away since · the front card as the last frame drew it (id,
/// rev, question, held).
#[derive(Default)]
struct Seen {
    up: BTreeSet<PathBuf>,
    answered: BTreeSet<PathBuf>,
    drawn: Option<(u64, u64, String, Option<String>)>,
}

#[test]
fn a_resolved_rebases_card_read_again() {
    super::seeds("a_resolved_rebases_card_read_again", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let world = World::new();
    let mut sim = Sim::new("a_resolved_rebases_card_read_again", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (world.repo.clone(), "proj".into());
    let mut ids = Vec::new();
    for (name, wt) in &world.sessions {
        // (a tracked file changed besides stop 1's conflict: its `y` held)
        std::fs::write(wt.join("a.rs"), "fn a() { changed() }\n").unwrap();
        let id = ed.workspace_open(wt).unwrap();
        ed.workspace_switch(id);
        ed.workspace.name = name.clone();
        let task = crate::task::Task { name: "build".into(), cmd: "true".into(), cwd: wt.clone() };
        let mut r = crate::task::Run::for_test(task);
        r.state = crate::task::State::Running;
        ed.workspace.task_run = Some(r);
        crate::rebase::resolved_at_for_test(&mut ed, 1, &["a.txt"]);
        ids.push((id, wt.clone()));
    }
    ed.workspace_switch(WorkspaceId::HOME);
    let dir = world.dir.display().to_string();
    let seen = RefCell::new(Seen::default());
    let step = |sim: &mut Sim, ed: &mut Editor| {
        let said = input(sim, ed, &ids, &mut seen.borrow_mut());
        let mut s = seen.borrow_mut();
        // (an answer, a move on: the card taken away — only `rebase_offer` puts one up again)
        s.up.retain(|root| cards(ed).any(|o| resolved_of(o) == Some(root.as_path())));
        said
    };
    sim.walk(&mut ed, INPUTS, step, |ed| held(ed, &seen.borrow()), |ed| print(ed, &dir));
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn input(sim: &mut Sim, ed: &mut Editor, ids: &[(WorkspaceId, PathBuf)], seen: &mut Seen) -> String {
    let c = &mut sim.choices;
    let (sid, root) = c.pick(ids).clone();
    // (by arm: its card up 3, the card in front focused 2, answered 3, a frame 2, the checkout's file 3, the git
    // tab's read 2, the rebase on 1, a switch 1, time passing 1)
    match c.weighted(&[3, 2, 3, 2, 3, 2, 1, 1, 1]) {
        0 => match ed.rebase_offer(sid) {
            Ok(()) => {
                seen.answered.remove(&root);
                seen.up.insert(root);
                format!("session {sid}'s card up")
            }
            Err(e) => format!("session {sid}'s card: {e}"),
        },
        // (drawn and focused — a press on it: a card that came on its own takes no key until then)
        1 => {
            frame(ed, seen);
            ed.card_focus_front();
            frame(ed, seen);
            "the card in front focused".into()
        }
        2 => {
            let key = *c.pick(&['y', 'y', 'X', 'n']);
            let Some(front) = ed.offers.first().map(|o| (o.id, resolved_of(o).map(Path::to_path_buf))) else {
                return "(no card up)".into();
            };
            if !ed.card_has_keys() {
                frame(ed, seen);
                ed.card_focus_front();
                frame(ed, seen);
            }
            ed.handle_key(Key::plain(Code::Char(key)));
            if let (id, Some(root)) = front
                && matches!(key, 'y' | 'X')
                && !cards(ed).any(|o| o.id == id)
            {
                seen.answered.insert(root);
            }
            key.to_string()
        }
        3 => {
            frame(ed, seen);
            "a frame".into()
        }
        // The file that holds stop 1's `y`, as you'd treat it in the git tab or the code
        4 => {
            let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match c.below(4) {
                0 => {
                    git(&root, &["add", "a.rs"]);
                    format!("{name}: a.rs staged")
                }
                1 => {
                    git(&root, &["reset", "-q", "--", "a.rs"]);
                    format!("{name}: a.rs unstaged")
                }
                2 => {
                    git(&root, &["checkout", "--", "a.rs"]);
                    format!("{name}: a.rs undone")
                }
                _ => {
                    std::fs::write(root.join("a.rs"), format!("fn a() {{ again({}) }}\n", sim.steps))
                        .unwrap();
                    format!("{name}: a.rs changed again")
                }
            }
        }
        5 => {
            ed.with_workspace(sid, Editor::git_refresh);
            format!("session {sid}'s git tab reads")
        }
        // Its git at work, then resolved at the next stop — or straight there (a run between two frames)
        6 => {
            let straight = c.below(2) == 0;
            let mut said = String::new();
            ed.with_workspace(sid, |ed| {
                let Some(r) = ed.workspace.rebase.as_ref() else { return };
                let n = r.stop.step.map_or(1, |s| s.0);
                if r.stage == Stage::Resolved && !straight {
                    crate::rebase::going_on_for_test(ed);
                    said = format!("session {sid}'s rebase goes on from stop {n}");
                } else {
                    // (stop 1's conflict is a.txt, the next a.rs — the file that held the one before)
                    let files: &[&str] = if n % 2 == 0 { &["a.txt"] } else { &["a.rs"] };
                    crate::rebase::resolved_at_for_test(ed, n + 1, files);
                    said = format!("session {sid}'s rebase resolved at stop {}", n + 1);
                }
            });
            said
        }
        7 => {
            let to = *c.pick(&[WorkspaceId::HOME, ids[0].0, ids[1].0]);
            ed.workspace_switch(to);
            format!("session {to} in front")
        }
        8 => {
            let ms = *c.pick(&[50, 300, 2000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => unreachable!("one arm a weight"),
    }
}

/// A frame, the front card as it drew it remembered.
fn frame(ed: &mut Editor, seen: &mut Seen) {
    crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
    seen.drawn = ed.offers.first().map(|o| (o.id, o.rev, o.question.clone(), o.held.clone()));
}

/// The cards up and parked, by every session.
fn cards(ed: &Editor) -> impl Iterator<Item = &Offer> {
    let parked = std::iter::once(&ed.workspace).chain(&ed.workspaces).flat_map(|l| &l.parked);
    ed.offers.iter().chain(parked)
}

/// The checkout a resolved card is about.
fn resolved_of(o: &Offer) -> Option<&Path> {
    match &o.what {
        What::RebaseResolved { root, .. } => Some(root),
        _ => None,
    }
}

/// A resolved card is up only as `rebase_offer` put it · what a rebase says changed besides its conflict is never
/// its stop's own conflicted file · a card with the keys says what it said when drawn.
fn held(ed: &Editor, seen: &Seen) -> Result<(), String> {
    if let Some(o) = cards(ed).find(|o| resolved_of(o).is_some_and(|r| !seen.up.contains(r))) {
        let root = resolved_of(o).unwrap_or(Path::new(""));
        let how = if seen.answered.contains(root) { "is back after its answer" } else { "is up" };
        return Err(format!("card#{} {:?} {how}, and no `rebase_offer` put it up since", o.id, o.title));
    }
    for l in std::iter::once(&ed.workspace).chain(&ed.workspaces) {
        let Some(r) = l.rebase.as_ref() else { continue };
        if let Some(f) = r.also.tracked.iter().chain(&r.also.made).find(|f| r.stop.files.contains(f)) {
            return Err(format!(
                "session {}'s rebase at {:?} names {f} as changed besides its conflict — the stop's own conflicted file",
                l.id, r.stop.step
            ));
        }
    }
    if let (true, Some(o), Some((id, _, question, held))) =
        (ed.card_has_keys(), ed.offers.first(), &seen.drawn)
        && o.id == *id
        && (&o.question, &o.held) != (question, held)
    {
        return Err(format!("card#{id} has the keys, saying other than it said when drawn"));
    }
    Ok(())
}

/// The scenario run with `broken`'s guard out: some seed of `seeds` (found by a run of 0..40, the first that did)
/// fails, saying `said`.
fn sees(broken: Broken, seeds: &[u64], said: &str) {
    let _back = broken.set();
    let mut failures = Vec::new();
    for &seed in seeds {
        let failed = std::panic::catch_unwind(|| run(Choices::seeded(seed)));
        let Err(p) = failed else { continue };
        let p = super::panic_said(&*p);
        if p.contains(said) {
            return;
        }
        failures.push(p);
    }
    panic!("with {broken:?}, no seed of {seeds:?} failed saying {said:?}: {failures:#?}");
}

/// …and it sees a read again that lands after the card was answered, `y` or `X`, put it back up.
#[test]
fn the_read_again_simulation_sees_an_answered_card_brought_back() {
    sees(Broken::BringsBack, &[7], "is back after its answer");
}

/// …and it sees a read asked at one stop landing after the rebase went on and was resolved at the next: what it
/// read laid on that stop (the stage alone, looked at before the stop was, let it).
#[test]
fn the_read_again_simulation_sees_a_read_landing_at_the_next_stop() {
    sees(Broken::AnyStop, &[16], "the stop's own conflicted file");
}

/// …and it sees a card said again under the focus at the same `rev`, keeping the keys.
#[test]
fn the_read_again_simulation_sees_a_card_changed_under_the_focus() {
    sees(Broken::SameRev, &[0], "has the keys, saying other than it said when drawn");
}
