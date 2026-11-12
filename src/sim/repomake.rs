//! `:session-new` in a folder in no repository, simulated (#513): at random `:session-new dates` (looked at on a
//! worker — the card that makes a repository), `y` or `n` on the card in front (drawn and focused, as yours is), `:w` or
//! `:q` in the first commit's message, a file written in the folder (one named like a key among them), a frame, the
//! user taking their time. After every step: at most one such card; a repository there only after a `y` took it; a
//! file named like a key never staged nor committed; a message being written is an open buffer.

use std::path::Path;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::key::{Code, Key};
use crate::offer::What;

const SEEDS: &[u64] = &[0, 1];

const INPUTS: usize = 30;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_new_session_in_a_folder_with_no_repository() {
    super::seeds("a_new_session_in_a_folder_with_no_repository", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-repomake");
    let dir = scratch.to_path_buf();
    let root = dir.join("folder");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "a\n").unwrap();
    std::fs::write(root.join(".env"), "X=1\n").unwrap();
    let user = dir.join("gitconfig");
    std::fs::write(&user, "[user]\n\tname = t\n\temail = t@t\n[commit]\n\tgpgsign = false\n").unwrap();
    crate::gitrepo::global_config_for(&dir, &user);
    let mut sim = Sim::new("a_new_session_in_a_folder_with_no_repository", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (root.clone(), "folder".into());
    ed.config.workspace_worktrees = dir.join("wt").display().to_string();
    // (whether a `y` was taken on the card: a repository only after one)
    let taken = std::cell::Cell::new(false);
    let shown = dir.display().to_string();
    let step = |sim: &mut Sim, ed: &mut Editor| input(sim, ed, &root, &taken);
    let check = |ed: &Editor| held(ed, &root, taken.get());
    sim.walk(&mut ed, INPUTS, step, check, |ed| super::sessions::print(ed, &shown));
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn input(sim: &mut Sim, ed: &mut Editor, root: &Path, taken: &std::cell::Cell<bool>) -> String {
    let c = &mut sim.choices;
    let card_up = |ed: &Editor| ed.offers.first().is_some_and(|o| matches!(o.what, What::RepoMake(_)));
    // (by arm: `:session-new` 3, `y` on the card 3, `n` 1, `:w` in the message 2, `:q` 1, a file 1, a frame 2, time
    // passing 2)
    match c.weighted(&[3, 3, 1, 2, 1, 1, 2, 2]) {
        0 => {
            ed.session_new("dates", None);
            ":session-new dates".into()
        }
        // (drawn and focused — yours, it is at once; then the letter as a key)
        1 | 2 if !card_up(ed) => "(no repository card in front)".into(),
        k @ (1 | 2) => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            ed.card_focus_front();
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            let letter = if k == 1 { 'y' } else { 'n' };
            ed.handle_key(Key::plain(Code::Char(letter)));
            // (taken: the card at work on it, or gone)
            if letter == 'y' && (!card_up(ed) || ed.offers[0].installing()) {
                taken.set(true);
            }
            letter.to_string()
        }
        k @ (3 | 4) if ed.in_commit_message() => {
            let line = if k == 3 { "w" } else { "q" };
            ed.handle_key(Key::plain(Code::Char(':')));
            for ch in line.chars() {
                ed.handle_key(Key::plain(Code::Char(ch)));
            }
            ed.handle_key(Key::plain(Code::Enter));
            format!(":{line} in the message")
        }
        3 | 4 => "(no commit message in front)".into(),
        5 => {
            let name = *c.pick(&["b.txt", "id_rsa", "c.txt"]);
            std::fs::write(root.join(name), format!("{}\n", sim.steps)).unwrap();
            format!("{name} written")
        }
        6 => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            "a frame".into()
        }
        _ => {
            let ms = *c.pick(&[50, 600]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

/// At most one repository card; a repository only after its `y`; nothing named like a key in the index or a commit; a
/// commit message being written is an open buffer.
fn held(ed: &Editor, root: &Path, taken: bool) -> Result<(), String> {
    let parked = std::iter::once(&ed.workspace).chain(&ed.workspaces).flat_map(|l| &l.parked);
    let cards = ed.offers.iter().chain(parked).filter(|o| matches!(o.what, What::RepoMake(_))).count();
    if cards > 1 {
        return Err(format!("{cards} repository cards up"));
    }
    if !root.join(".git").exists() {
        return Ok(());
    }
    if !taken {
        return Err("a repository made with no y on its card".into());
    }
    let git = |args: &[&str]| crate::gitrepo::git(root, args, None).unwrap_or_default();
    let keyed = |list: String| list.lines().find(|f| [".env", "id_rsa"].contains(f)).map(str::to_string);
    let committed = match git(&["rev-parse", "-q", "--verify", "HEAD"]).is_empty() {
        true => None,
        false => keyed(git(&["ls-tree", "-r", "--name-only", "HEAD"])),
    };
    if let Some(f) = keyed(git(&["ls-files"])).or(committed) {
        return Err(format!("{f}, named like a key, staged or committed"));
    }
    for l in std::iter::once(&ed.workspace).chain(&ed.workspaces) {
        if let Some(c) = &l.committing
            && !ed.docs.iter().any(|d| d.id == c.doc)
        {
            return Err(format!("session {}'s commit message is in buffer {:?}, not open", l.id, c.doc));
        }
    }
    Ok(())
}
