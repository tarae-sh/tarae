//! A buffer's index reads landing out of order, simulated: a file open in a repository of the scenario's own, and
//! at random the index changed then read again (a hunk staged, `space g s`: staged, then `git_load_base`) or only
//! read again (a focus regain's, a git tab action's) — each read a job, run and landed in the seed's order, so an
//! older one, which read the index before a later stage, may land after the newer. Once it's all quiet: the
//! buffer's base is the index as it is now (an older read landing last put back one that's gone, and nothing
//! asked again).

use std::path::Path;
use std::time::Duration;

use super::sessions::{git, git_out};
use super::{Choices, Sim};
use crate::editor::Editor;

/// (3: an older read, of the index before two stages, lands after the newest)
const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 12;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn the_newest_index_read_is_the_base() {
    super::seeds("the_newest_index_read_is_the_base", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-gitbase");
    std::fs::create_dir_all(dir.join("proj")).unwrap();
    let repo = std::fs::canonicalize(dir.join("proj")).unwrap();
    std::fs::write(repo.join("a.txt"), "one\n").unwrap();
    for args in [&["init", "-q", "-b", "main"][..], &["add", "a.txt"]] {
        git(&repo, args);
    }
    let mut sim = Sim::new("the_newest_index_read_is_the_base", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.session.root = repo.clone();
    ed.git_auto = true;
    ed.open(&repo.join("a.txt")).unwrap();
    let mut staged = 0;
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &repo, &mut staged), |_| Ok(()), print);
    let index = index(&repo);
    sim.at_end(|| match ed.doc().git_base.as_deref() {
        Some(base) if *base == index => Ok(()),
        base => Err(format!("the base is {base:?}, the index {index:?}")),
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn input(sim: &mut Sim, ed: &mut Editor, repo: &Path, staged: &mut usize) -> String {
    // (by arm: a hunk staged 1, a read alone 1)
    match sim.choices.weighted(&[1, 1]) {
        0 => {
            *staged += 1;
            stage(repo, &format!("one\nstaged {staged}\n"));
            ed.git_load_base(ed.doc().id);
            format!("staged {staged}, read")
        }
        _ => {
            ed.git_reload_bases();
            "read".into()
        }
    }
}

/// `text` put in the index as `a.txt`'s, the file itself left as it is (a hunk staged from the buffer).
fn stage(repo: &Path, text: &str) {
    let blob = repo.with_file_name("blob.txt");
    std::fs::write(&blob, text).unwrap();
    let blob = blob.to_str().unwrap();
    let sha = git_out(repo, &["hash-object", "-w", blob]);
    git(repo, &["update-index", "--cacheinfo", &format!("100644,{},a.txt", sha.trim())]);
}

/// `a.txt` in the index now.
fn index(repo: &Path) -> String {
    git_out(repo, &["show", ":a.txt"])
}

fn print(ed: &Editor) -> String {
    let d = ed.doc();
    format!("base {:?} · asked {}", d.git_base.as_deref(), d.git_asked)
}
