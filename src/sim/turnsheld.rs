//! Turns' texts let go (#214's fourth piece), simulated: two sessions, each with its turns kept in a folder; a turn ends
//! in either — its file written unseen, gathered — the front switches, `space u` takes the front's back, frames are drawn
//! and time passes, the saves and reads landing in any order. After every step: no session holds more than `KEEP`
//! turns' texts, and none behind more than one its file has (a step's read landing after it went behind). Once quiet:
//! the same, and each session's file is what its newest turn not taken back wrote — a turn let go is still taken
//! back, its texts read from its file.
//!
//! Each turn writes ~300 KB and a project keeps 1 MB (`llm.turns-max-mb`): the saves let the oldest turns go (#214,
//! round 2), among the steps, reads and switches. After every step each project's store is within it (or one turn),
//! names no text that isn't there and holds none it doesn't name; once quiet, a session whose kept turns are all
//! taken back has its file as the oldest kept one found it.

use std::path::PathBuf;
use std::time::Duration;

use super::{Choices, Sim};
use crate::checkpoint::Checkpoint;
use crate::editor::Editor;
use crate::event::WorkspaceId;
use crate::key::{Code, Key};

const NAME: &str = "turns_let_go_are_still_taken_back";

const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5];

const INPUTS: usize = 40;

const LIMIT: Duration = crate::test_results::PATIENCE;

const FIRST: &str = "fn a() {}\n";

/// A turn's file past its first lines: ~300 KB.
const PAD: &str = "// a line of a big source file, as long as most of them are in a file this size\n";
const PAD_LINES: usize = 3700;

/// What a project keeps, in MB.
const MAX_MB: u64 = 1;

#[test]
fn turns_let_go_are_still_taken_back() {
    super::seeds(NAME, SEEDS, run);
}

/// Each session's id, root, turns made, and whether `space u` was pressed there (no turn after: a turn's text before
/// is read from disk as it starts, and a step's write may not have landed yet).
struct World {
    ids: [WorkspaceId; 2],
    roots: [PathBuf; 2],
    turns: [usize; 2],
    stepped: [bool; 2],
}

fn run(choices: Choices) -> Vec<String> {
    let scratch = crate::scratch::Scratch::new("sim-turnsheld");
    let t = std::fs::canonicalize(scratch.path()).unwrap();
    let roots = [t.join("front"), t.join("back")];
    for r in &roots {
        std::fs::create_dir_all(r).unwrap();
        std::fs::write(r.join("a.rs"), FIRST).unwrap();
    }
    let mut sim = Sim::new(NAME, choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.turns_dir) = (roots[0].clone(), Some(t.join("turns")));
    ed.config.llm.turns_max_mb = MAX_MB;
    ed.workspace_kept_back(ed.workspace.id);
    let ids = [ed.workspace.id, ed.workspace_add(roots[1].clone())];
    // (a git review open in each: no turn's review opens — the turn one is open on keeps its texts)
    for (id, root) in ids.into_iter().zip(&roots) {
        let review =
            crate::codereview::CodeReview::for_test(crate::codereview::Base::Head, root.clone(), vec![]);
        ed.with_workspace(id, |ed| ed.workspace.code_review = Some(review));
    }
    let mut w = World { ids, roots: roots.clone(), turns: [0, 0], stepped: [false, false] };
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &mut w), check, print);
    if let Err(e) = check(&ed) {
        sim.fail_now(format!("once quiet: {e}"));
    }
    for (k, root) in roots.iter().enumerate() {
        let l = session(&ed, ids[k]);
        let behind = l.id != ed.workspace.id;
        let held = l.claude.checkpoints.list.iter().filter(|c| c.files.here().is_some()).count();
        if behind && held > 1 {
            sim.fail_now(format!("once quiet, session {k} behind holds {held} turns' texts"));
        }
        // (none applied: as the oldest kept found it — the turn before it let go, its change still there)
        let list = &l.claude.checkpoints.list;
        let want = match (list.iter().rev().find(|c| !c.undone), list.first()) {
            (Some(c), _) => text(k, &c.question),
            (None, Some(oldest)) => before(k, &oldest.question),
            (None, None) => FIRST.to_string(),
        };
        let on_disk = std::fs::read_to_string(root.join("a.rs")).unwrap_or_default();
        if on_disk != want {
            let turns: Vec<String> = l
                .claude
                .checkpoints
                .list
                .iter()
                .map(|c| format!("{}{}", c.question, if c.undone { " u" } else { "" }))
                .collect();
            sim.fail_now(format!("session {k}'s a.rs once quiet: {on_disk:?}, want {want:?} — {turns:?}"));
        }
    }
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// What session `k`'s turn named `question` wrote.
fn text(k: usize, question: &str) -> String {
    format!("fn a() {{}}\n// {k}: {question}\n{}", PAD.repeat(PAD_LINES))
}

/// What session `k`'s turn named `question` (`turn N`) found: what the turn before wrote (turns follow one another
/// — none after a step).
fn before(k: usize, question: &str) -> String {
    match question.strip_prefix("turn ").and_then(|n| n.parse::<usize>().ok()) {
        Some(n) if n > 1 => text(k, &format!("turn {}", n - 1)),
        _ => FIRST.to_string(),
    }
}

fn session(ed: &Editor, id: WorkspaceId) -> &crate::workspace::Workspace {
    std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == id).unwrap()
}

fn input(sim: &mut Sim, ed: &mut Editor, w: &mut World) -> String {
    let c = &mut sim.choices;
    // (by arm: a turn ends 4, the front switches 2, `space u` 2, a frame 1, time 1)
    match c.weighted(&[4, 2, 2, 1, 1]) {
        0 => {
            let k = c.below(2);
            if w.stepped[k] {
                return format!("(session {k} is taking turns back)");
            }
            // (one turn at a time: the last one's end gathered first)
            if session(ed, w.ids[k]).claude.checkpoints.gathering {
                return format!("(session {k}'s last turn still gathered)");
            }
            w.turns[k] += 1;
            let (path, q) = (w.roots[k].join("a.rs"), format!("turn {}", w.turns[k]));
            let before = std::fs::read_to_string(&path).ok().map(|s| ropey::Rope::from_str(&s));
            let (p, written) = (path.clone(), text(k, &q));
            ed.with_workspace(w.ids[k], |ed| {
                crate::checkpoint::record_unseen(ed, p, before, |_| {});
                std::fs::write(&path, &written).unwrap();
                assert!(crate::checkpoint::gather(ed, &q), "written unseen");
            });
            format!("session {k}'s {q} ends")
        }
        1 => {
            let to = if ed.workspace.id == w.ids[0] { w.ids[1] } else { w.ids[0] };
            ed.workspace_switch(to);
            format!("the front switches to {}", if to == w.ids[0] { 0 } else { 1 })
        }
        2 => {
            let k = usize::from(ed.workspace.id == w.ids[1]);
            w.stepped[k] = true;
            ed.workspace.viewer.at = crate::workspace::At::Code;
            for k in [Code::Char(' '), Code::Char('u')] {
                ed.handle_key(Key::plain(k));
            }
            format!("space u in session {k}")
        }
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

/// No session holds more than `KEEP` turns' texts; none behind the texts of more than one turn its file has (`filed`):
/// the one a step read, its result landing after the session went behind (#218: held while the session stays where it
/// is — a session reads one at a time).
fn check(ed: &Editor) -> Result<(), String> {
    for l in std::iter::once(&ed.workspace).chain(&ed.workspaces) {
        let cps = &l.claude.checkpoints;
        let held: Vec<&Checkpoint> = cps.list.iter().filter(|c| c.files.here().is_some()).collect();
        if held.len() > crate::checkpoint::KEEP {
            return Err(format!("{} holds {} turns' texts", l.name, held.len()));
        }
        let behind = l.id != ed.workspace.id;
        let filed = held.iter().filter(|c| c.filed()).count();
        if behind && filed > 1 {
            return Err(format!("{} behind holds {filed} turns' texts their files have", l.name));
        }
        // Its project's store: within what it keeps (or its one turn), every text it names there, none it doesn't
        let Some(dir) = &ed.turns_dir else { continue };
        let s = crate::checkpoint::store_seen(dir, &l.root);
        if (s.bytes > MAX_MB << 20 && s.turns > 1) || s.missing > 0 || s.unnamed > 0 {
            return Err(format!("{}'s store: {s:?}", l.name));
        }
    }
    Ok(())
}

/// Each session's turns: `+` its texts held, `-` let go, `u` taken back.
fn print(ed: &Editor) -> String {
    let of = |l: &crate::workspace::Workspace| {
        let marks: String = l
            .claude
            .checkpoints
            .list
            .iter()
            .map(|c| match (c.undone, c.files.here().is_some()) {
                (true, _) => 'u',
                (false, true) => '+',
                (false, false) => '-',
            })
            .collect();
        format!("{}{} [{marks}]", l.name, if l.id == ed.workspace.id { " (front)" } else { "" })
    };
    let all: Vec<String> = std::iter::once(&ed.workspace).chain(&ed.workspaces).map(of).collect();
    all.join(" · ")
}
