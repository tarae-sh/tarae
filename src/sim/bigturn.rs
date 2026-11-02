//! A big file's turn stepped (#281), simulated: a repository of the scenario's own with `big.txt` past
//! `snapshot::TEXT_MAX` (its buffer loads in the background) and `b.bin`; a turn in which Claude edited big.txt and a
//! run then changed it and wrote b.bin, its end gathered on a worker. At random: `space u`, `space U` (each taken, or
//! refused while the turn is gathered, the last step's buffers load or its blobs are written), a big buffer of the user's
//! opened or closed, time passing — the gathering, the loads and the blobs' writes landing in the seed's order. After
//! every step: each file is as the turn found it or as it left it, never between (big.txt with Claude's edit and not the
//! run's: the text just before the run); a step waiting waits only on buffers still loading. Once quiet: both files as
//! the turn's last step left it.

use std::path::Path;
use std::time::Duration;

use super::sessions::git;
use super::{Choices, Sim};
use crate::editor::Editor;

const NAME: &str = "a_big_files_turn_steps_whole";

const SEEDS: &[u64] = &[0, 1, 2, 3];

const INPUTS: usize = 16;

const LIMIT: Duration = crate::test_results::PATIENCE;

const BIN_BEFORE: &[u8] = b"\x00bin-before\n";
const BIN_AFTER: &[u8] = b"\x00bin-after\n";

#[test]
fn a_big_files_turn_steps_whole() {
    super::seeds(NAME, SEEDS, run);
}

/// big.txt as the turn found it and as it left it.
struct Texts {
    first: String,
    last: String,
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-bigturn");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut first = String::new();
    for i in 0.. {
        if first.len() as u64 > crate::snapshot::TEXT_MAX {
            break;
        }
        first.push_str(&format!("line {i}\n"));
    }
    std::fs::write(root.join("big.txt"), &first).unwrap();
    std::fs::write(root.join("b.bin"), BIN_BEFORE).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "-m", "base"]);
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    // Claude's edit of big.txt, unseen; then a run changing it again and b.bin
    let path = root.join("big.txt");
    let edited = first.replacen("line 7\n", "line 7 claude\n", 1);
    crate::checkpoint::record_unseen(&mut ed, path.clone(), Some(ropey::Rope::from_str(&first)), |_| {});
    std::fs::write(&path, &edited).unwrap();
    let before = crate::snapshot::Before::take(&root);
    let last = edited.replacen("line 9\n", "line 9 run\n", 1);
    std::fs::write(&path, &last).unwrap();
    std::fs::write(root.join("b.bin"), BIN_AFTER).unwrap();
    crate::checkpoint::record_run(&mut ed, before.since().unwrap().files);
    assert!(crate::checkpoint::gather(&mut ed, "fix"), "written unseen");
    let texts = Texts { first, last };
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &root), |ed| check(ed, &root, &texts), print);
    let end = check(&ed, &root, &texts).and_then(|()| {
        let undone = ed.workspace.claude.checkpoints.list.first().is_some_and(|c| c.undone);
        let (text, bin) = files(&root);
        let want = if undone { (&texts.first, BIN_BEFORE) } else { (&texts.last, BIN_AFTER) };
        match (text == *want.0, bin == want.1) {
            (true, true) => Ok(()),
            _ => Err(format!(
                "undone {undone}, yet big.txt the turn's {} and b.bin {bin:?}",
                which(&text, &texts)
            )),
        }
    });
    sim.at_end(|| end);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn input(sim: &mut Sim, ed: &mut Editor, root: &Path) -> String {
    let c = &mut sim.choices;
    let other = root.join("other.txt");
    // (by arm: `space u` 3, `space U` 3, the user's big buffer opened 1, closed 1, time 1)
    match c.weighted(&[3, 3, 1, 1, 1]) {
        k @ (0 | 1) => {
            let key = if k == 0 { "space u" } else { "space U" };
            match crate::checkpoint::step(ed, k == 0) {
                Ok(()) => key.into(),
                Err(e) => format!("{key} refused: {e}"),
            }
        }
        2 => {
            if !other.exists() {
                std::fs::write(&other, std::fs::read(root.join("big.txt")).unwrap()).unwrap();
            }
            ed.open(&other).unwrap();
            "other.txt opened".into()
        }
        3 => match ed.docs.by_path(&other).map(|d| d.id) {
            Some(id) => {
                ed.close_doc(id);
                "other.txt closed".into()
            }
            None => "(other.txt not open)".into(),
        },
        _ => {
            let ms = *c.pick(&[10, 100, 500]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

fn files(root: &Path) -> (String, Vec<u8>) {
    let text = std::fs::read_to_string(root.join("big.txt")).unwrap_or_default();
    (text, std::fs::read(root.join("b.bin")).unwrap_or_default())
}

fn which(text: &str, texts: &Texts) -> &'static str {
    match text {
        t if t == texts.first => "first",
        t if t == texts.last => "last",
        _ => "neither",
    }
}

/// Each file as the turn found it or left it; a step waiting waits only on buffers still loading.
fn check(ed: &Editor, root: &Path, texts: &Texts) -> Result<(), String> {
    let (text, bin) = files(root);
    if which(&text, texts) == "neither" {
        return Err(format!("big.txt is neither the turn's first text nor its last ({} bytes)", text.len()));
    }
    if bin != BIN_BEFORE && bin != BIN_AFTER {
        return Err(format!("b.bin is {bin:?}"));
    }
    for (id, p) in ed.workspace.claude.checkpoints.loading.iter().flat_map(|w| &w.1) {
        if !ed.docs.iter().any(|d| d.id == *id && d.loading) {
            return Err(format!("a step waits on {}, which isn't loading", p.display()));
        }
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let cps = &ed.workspace.claude.checkpoints;
    let undone = cps.list.first().map(|c| c.undone);
    let waits = cps.loading.as_ref().map(|w| w.1.len());
    format!("undone {undone:?} · waits {waits:?} · putting {} · gathering {}", cps.putting, cps.gathering)
}
