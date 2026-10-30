//! The reviewer's findings leaving with a landed change, simulated (#248): a repository with changed files, and at
//! random a finding pinned on a changed file, one of them answered by you, a file committed (its change in the base
//! now — the checkout tarae started in: HEAD), a file changed again, the check a bring or a new round makes
//! (`notes::landed_check`), the user taking their time — its git and the findings' digests jobs among them. After
//! every step: a finding gone was on a file committed since it was pinned, never one you answered. Once it's all
//! quiet and checked again: no finding of the reviewer's alone left on a file with no change, and each one closed
//! said once.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use super::sessions::{git, git_out};
use super::{Choices, Sim};
use crate::editor::Editor;
use crate::notes::By;

/// (2: a finding you answered on a file committed, the check after — closed with your words, unguarded)
const SEEDS: &[u64] = &[0, 1, 2];

const INPUTS: usize = 30;

const LIMIT: Duration = crate::test_results::PATIENCE;

const FILES: &[&str] = &["a.txt", "b.txt", "c.txt"];

#[test]
fn findings_leave_with_their_landed_change() {
    super::seeds("findings_leave_with_their_landed_change", SEEDS, run);
}

/// What the checks keep: each finding's file and the input it was pinned at, those you answered, each file's last
/// commit's input, the files changed now, the inputs so far.
#[derive(Default)]
struct Seen {
    pinned: BTreeMap<u64, (&'static str, usize)>,
    yours: BTreeSet<u64>,
    committed: BTreeMap<&'static str, usize>,
    changed: BTreeSet<&'static str>,
    n: usize,
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-landed");
    std::fs::create_dir_all(dir.join("proj")).unwrap();
    let repo = std::fs::canonicalize(dir.join("proj")).unwrap();
    for f in FILES {
        std::fs::write(repo.join(f), "one\n").unwrap();
    }
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.email", "t@t"],
        &["config", "user.name", "t"],
        &["config", "commit.gpgsign", "false"],
        &["add", "-A"],
        &["commit", "-q", "-m", "base"],
    ] {
        git(&repo, args);
    }
    for f in FILES {
        std::fs::write(repo.join(f), "one\ntwo\n").unwrap();
    }
    let mut sim = Sim::new("findings_leave_with_their_landed_change", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = repo.clone();
    let seen = RefCell::new(Seen { changed: FILES.iter().copied().collect(), ..Seen::default() });
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &repo, &seen), |ed| held(ed, &seen.borrow()), print);
    // (the check once more, nothing else under way: what it leaves is what's still on a change)
    let last = |_: &mut Sim, ed: &mut Editor| {
        crate::notes::landed_check(ed);
        "the check, last".to_string()
    };
    sim.walk(&mut ed, 1, last, |ed| held(ed, &seen.borrow()), print);
    sim.at_end(|| ended(&ed, &seen.borrow()));
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn input(sim: &mut Sim, ed: &mut Editor, repo: &Path, seen: &RefCell<Seen>) -> String {
    let c = &mut sim.choices;
    seen.borrow_mut().n += 1;
    // (by arm: a finding pinned 4, answered by you 2, a file committed 2, changed again 2, the check 3, time 1)
    match c.weighted(&[4, 2, 2, 2, 3, 1]) {
        0 => {
            let changed: Vec<&'static str> = seen.borrow().changed.iter().copied().collect();
            if changed.is_empty() {
                return "(nothing changed to find on)".into();
            }
            let f = *c.pick(&changed);
            let id = crate::notes::pin_finding(ed, &repo.join(f), 0..1, &format!("on {f}"), None, None);
            let mut s = seen.borrow_mut();
            let n = s.n;
            s.pinned.insert(id, (f, n));
            format!("finding #{id} on {f}")
        }
        1 => {
            let ids: Vec<u64> = ed.workspace.claude.notes.list.iter().map(|n| n.id).collect();
            if ids.is_empty() {
                return "(no finding to answer)".into();
            }
            let id = *c.pick(&ids);
            if let Some(n) = ed.workspace.claude.notes.list.iter_mut().find(|n| n.id == id) {
                n.thread.push(crate::notes::Entry { by: By::You, text: "why?".into(), draft: true });
            }
            seen.borrow_mut().yours.insert(id);
            format!("#{id} answered by you")
        }
        2 => {
            let f = *c.pick(FILES);
            git(repo, &["add", f]);
            git(repo, &["commit", "-q", "--allow-empty", "-m", f]);
            let mut s = seen.borrow_mut();
            let n = s.n;
            s.committed.insert(f, n);
            s.changed.remove(f);
            format!("{f} committed")
        }
        3 => {
            let f = *c.pick(FILES);
            // (a size of its own each time: the simulated clock stamps every write alike, and `diff-index` trusts an
            // index entry whose size and time match — a real write's time is its own)
            let n = seen.borrow().n;
            super::write(&repo.join(f), format!("one\n{}", "fix\n".repeat(n)));
            seen.borrow_mut().changed.insert(f);
            format!("{f} changed again")
        }
        4 => {
            crate::notes::landed_check(ed);
            "the check".into()
        }
        5 => {
            let ms = *c.pick(&[50, 300, 2000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => unreachable!("one arm a weight"),
    }
}

/// A finding gone was on a file committed since it was pinned, and never one you answered.
fn held(ed: &Editor, seen: &Seen) -> Result<(), String> {
    let here = |id: u64| ed.workspace.claude.notes.list.iter().any(|n| n.id == id);
    for (&id, &(f, at)) in seen.pinned.iter().filter(|(id, _)| !here(**id)) {
        if seen.yours.contains(&id) {
            return Err(format!("#{id} closed with your words in it"));
        }
        if seen.committed.get(f).is_none_or(|&c| c < at) {
            return Err(format!("#{id} on {f} closed, nothing committed since it was pinned"));
        }
    }
    Ok(())
}

/// None of the reviewer's alone is left on a file with no change, and the findings closed are those said.
fn ended(ed: &Editor, seen: &Seen) -> Result<(), String> {
    let still = |f: &str| !git_out(&ed.workspace.root, &["status", "--porcelain", "--", f]).trim().is_empty();
    for n in &ed.workspace.claude.notes.list {
        let Some(&(f, _)) = seen.pinned.get(&n.id) else { continue };
        if !seen.yours.contains(&n.id) && !still(f) {
            return Err(format!("#{} left on {f}, whose change is committed", n.id));
        }
    }
    let said: usize = (ed.messages.iter())
        .filter_map(|m| m.2.strip_prefix("closed ")?.split(' ').next()?.parse::<usize>().ok())
        .sum();
    let gone = seen.pinned.len() - ed.workspace.claude.notes.list.len();
    if said != gone {
        return Err(format!("{gone} closed, {said} said"));
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    format!("{} findings", ed.workspace.claude.notes.list.len())
}
