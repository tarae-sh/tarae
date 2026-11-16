//! A bring's commit first beside a code review, simulated (#535–#537, the full video's take 2, chapter 4): `board`, a
//! session with changes in no commit and no commits of its own (two high-risk changes among them), and at random
//! `:workspace-bring board` (looked at on a worker), `c` on its held card, `y` or `n` on the review's end card, the
//! review opened or its end asked in `board` (`space v v`, `space v q` — its bases jobs), a high-risk change looked at
//! (`]h`, a frame, `space v h`) or taken back by an edit (graded again once its diff is), a commit on main from
//! outside, `:q` in the commit's message, a frame, the user taking their time. No claude starts: the config holds it
//! (`llm::held_for` — a draft asked is said held, the message buffer open).
//!
//! After every step: a commit first waits only behind its review's end card, up or parked (#535: never set aside in
//! silence); a bring's held card says a review is open in its checkout exactly while one is (#535); the review's end
//! card in front says the review's own count of high-risk changes not looked at, its yes held exactly while there are
//! some (#537); and no bring of `board` was only refused for main having moved (#536: the held card instead).

use std::path::Path;
use std::time::Duration;

use super::sessions::{git, print};
use super::{Choices, Sim};
use crate::editor::Editor;
use crate::event::WorkspaceId;
use crate::offer::{Offer, What};

const SEEDS: &[u64] = &[0, 1, 2];

const INPUTS: usize = 40;

const LIMIT: Duration = crate::test_results::PATIENCE;

const BASE_RS: &str = "fn a() {}\nfn x() {}\nfn y() {}\nfn z() {}\n";
const BOARD_RS: &str =
    "fn a() {}\nfn b() { unsafe { c() } }\nfn x() {}\nfn y() {}\nfn z() {}\nfn d() { unsafe { e() } }\n";

#[test]
fn a_brings_commit_first_beside_a_review() {
    super::seeds("a_brings_commit_first_beside_a_review", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-commitfirst");
    let repo = dir.join("proj");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("a.txt"), "one\n").unwrap();
    std::fs::write(repo.join("src/a.rs"), BASE_RS).unwrap();
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
    let wt = dir.join("wt").join("board");
    git(&repo, &["worktree", "add", "-q", "-b", "board", &wt.display().to_string(), "main"]);
    std::fs::write(wt.join("a.txt"), "mine\n").unwrap();
    std::fs::write(wt.join("src/a.rs"), BOARD_RS).unwrap();
    // (main moved past it already, as take 2's: #536's case from the first bring)
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "main moves"]);
    let mut sim = Sim::new("a_brings_commit_first_beside_a_review", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (repo.clone(), "proj".into());
    ed.workspace.git_branch = Some("main".into());
    ed.config.workspace_worktrees = dir.join("wt").display().to_string();
    // (no claude: a stand-in by name, and held by a config that doesn't read — the draft is said held)
    ed.config.llm.command = "sh".into();
    ed.config.llm.args = vec!["-c".into(), "cat > /dev/null".into()];
    ed.config_unread = vec!["sim: no claude here".into()];
    let board = ed.workspace_open(&wt).unwrap();
    if let Some(l) = ed.workspace_by_id(board) {
        (l.name, l.branch, l.base) = ("board".into(), Some("board".into()), Some("main".into()));
    }
    ed.workspace_switch(WorkspaceId::HOME);
    let shown = dir.display().to_string();
    // (a held card's `c` taken with board's review open, not yet answered on the end card it asked)
    let asked = std::cell::Cell::new(false);
    let step = |sim: &mut Sim, ed: &mut Editor| input(sim, ed, board, &repo, &wt, &asked);
    let check = |ed: &Editor| held(ed, board, &asked);
    sim.walk(&mut ed, INPUTS, step, check, |ed| format!("{} · {}", print(ed, &shown), state(ed, board)));
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// Keys as the main loop hands them in (`<name>` a named key), each an event.
fn keys(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key: crate::key::Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => crate::key::Key::plain(crate::key::Code::Char(c)),
        };
        ed.handle_event(crate::event::Event::Key(key));
    }
}

fn frame(ed: &mut Editor) {
    crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
}

/// The front card is `is`: drawn, focused, drawn again, then `letter` as a key.
fn answer(ed: &mut Editor, is: impl Fn(&Offer) -> bool, letter: char, none: &str) -> String {
    if !ed.offers.first().is_some_and(is) {
        return format!("(no {none} in front)");
    }
    frame(ed);
    ed.card_focus_front();
    frame(ed);
    keys(ed, &letter.to_string());
    format!("{letter} on the {none}")
}

fn input(
    sim: &mut Sim,
    ed: &mut Editor,
    board: WorkspaceId,
    repo: &Path,
    wt: &Path,
    asked: &std::cell::Cell<bool>,
) -> String {
    let c = &mut sim.choices;
    let held_card = |o: &Offer| matches!(&o.what, What::RebaseHeld(h) if h.bring);
    let end_card = |o: &Offer| matches!(o.what, What::ReviewEnd { .. });
    // (by arm: a bring asked 3, `c` 3, `y` on the end card 2, `n` 1, the review opened or its end asked 3, a look 2,
    // a change taken back 1, main moves 1, `:q` in the message 1, a frame 2, time passing 1, to board or home 1)
    match c.weighted(&[3, 3, 2, 1, 3, 2, 1, 1, 1, 2, 1, 1]) {
        0 => match ed.workspace_bring("board") {
            Ok(()) => ":workspace-bring board".into(),
            Err(e) => format!(":workspace-bring board: {e}"),
        },
        1 => {
            let reviewing = std::iter::once(&ed.workspace)
                .chain(&ed.workspaces)
                .any(|l| l.id == board && l.code_review.is_some());
            let said = answer(ed, held_card, 'c', "held card");
            asked.set(asked.get() || (reviewing && said.starts_with('c')));
            said
        }
        2 => answer(ed, end_card, 'y', "end card"),
        3 => {
            let said = answer(ed, end_card, 'n', "end card");
            asked.set(asked.get() && !said.starts_with('n'));
            said
        }
        // (in board, a card in front left as it is: the code has the keys only with none)
        4 if !ed.offers.is_empty() => "(a card in front: no review key)".into(),
        4 => {
            ed.workspace_switch(board);
            match ed.workspace.code_review.is_some() {
                true => {
                    keys(ed, "<space>vq");
                    "space v q in board".into()
                }
                false => {
                    keys(ed, "<space>vv");
                    "space v v in board".into()
                }
            }
        }
        5 if ed.workspace.id != board
            || ed.workspace.code_review.is_none()
            || ed.offers.iter().any(|o| !o.aside) =>
        {
            "(no review with the keys in front)".into()
        }
        5 => {
            keys(ed, "]h");
            frame(ed);
            keys(ed, "<space>vh");
            "]h, a frame, space v h".into()
        }
        6 => {
            // (a high-risk line of the buffer taken out — one still there; graded again once its diff is)
            let Some(d) = ed.docs.iter_mut().find(|d| d.path.as_deref() == Some(&wt.join("src/a.rs"))) else {
                return "(src/a.rs not open)".into();
            };
            let text = d.text.to_string();
            let Some(at) = text.find("unsafe") else { return "(no high-risk line left)".into() };
            let line = d.text.byte_to_line(at);
            let (from, to) = (d.text.line_to_byte(line), d.text.line_to_byte(line + 1));
            d.apply(&crate::transaction::Transaction::new(vec![crate::transaction::Change::delete(
                from, to,
            )]));
            format!("line {} of src/a.rs taken out", line + 1)
        }
        7 => {
            git(repo, &["commit", "-q", "--allow-empty", "-m", &format!("main {}", sim.steps)]);
            "a commit on main".into()
        }
        8 if ed.in_commit_message() => {
            keys(ed, "<esc>:q<ret>");
            ":q in the message".into()
        }
        8 => "(no commit message in front)".into(),
        9 => {
            frame(ed);
            "a frame".into()
        }
        10 => {
            let ms = *c.pick(&[50, 600]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => {
            let to = *c.pick(&[board, WorkspaceId::HOME]);
            ed.workspace_switch(to);
            format!("to {to}")
        }
    }
}

/// Board's review, its commit first's wait, staged files — the trace line's end.
fn state(ed: &Editor, board: WorkspaceId) -> String {
    let l = std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == board);
    let Some(l) = l else { return "board gone".into() };
    let review = match &l.code_review {
        Some(_) if l.id == ed.workspace.id => {
            let r = l.code_review.as_ref().unwrap();
            format!("review ({} high left)", r.high_unseen(ed).len())
        }
        Some(_) => "review (behind)".into(),
        None => "no review".into(),
    };
    let waits = if l.commit_waits.is_some() { " · commit waits" } else { "" };
    let message = if l.committing.is_some() { " · message open" } else { "" };
    format!("{review}{waits}{message}")
}

/// The invariants (#535–#537), after every step.
fn held(ed: &Editor, board: WorkspaceId, asked: &std::cell::Cell<bool>) -> Result<(), String> {
    let all = || std::iter::once(&ed.workspace).chain(&ed.workspaces);
    let cards = || all().flat_map(|l| &l.parked).chain(&ed.offers);
    // #535: `c` with board's review open asked its end — the card's up (or parked) until answered, or the review ended
    let l = all().find(|l| l.id == board);
    if asked.get() && l.is_some_and(|l| l.code_review.is_none()) {
        asked.set(false);
    }
    let end_up = cards().any(|o| matches!(o.what, What::ReviewEnd { session, .. } if session == board));
    if asked.get() && !end_up {
        return Err("c on the held card, board's review open: nothing asked of it, nothing in sight".into());
    }
    for l in all() {
        // #535: a commit first waits only behind its review's end card — never set aside in silence
        if l.commit_waits.is_some() {
            let asked = cards().any(|o| matches!(o.what, What::ReviewEnd { session, .. } if session == l.id));
            if l.code_review.is_none() || !asked {
                return Err(format!(
                    "{}'s commit first waits with {} and no end card up",
                    l.name,
                    if l.code_review.is_some() { "its review open" } else { "no review" }
                ));
            }
        }
    }
    // #535: a held card says a review is open in its checkout exactly while one is
    for o in cards() {
        if let What::RebaseHeld(h) = &o.what
            && h.bring
        {
            let open = all().any(|l| l.root == h.moved.root && l.code_review.is_some());
            let says = o.question.contains("review first (its card asks)");
            if open != h.review || says != open {
                return Err(format!(
                    "the held card says a review is open: {says}, one is: {open} — {}",
                    o.question
                ));
            }
        }
    }
    // #537: the end card in front says the front's review's own count, held exactly while there are some
    if let Some(r) = &ed.workspace.code_review {
        let n = r.high_unseen(ed).len();
        let ours = |o: &&Offer| matches!(o.what, What::ReviewEnd { session, started } if session == ed.workspace.id && started == r.started);
        for o in ed.offers.iter().filter(ours) {
            let want = (n > 0).then(|| crate::codereview::held_said(n));
            if o.held != want {
                return Err(format!("the end card holds {:?}, the review has {n} not looked at", o.held));
            }
        }
    }
    // #536: never only refused for main having moved
    if let Some(m) = ed.messages.iter().find(|m| m.2.contains("can't move up to")) {
        return Err(format!("a bring refused in a toast: {}", m.2));
    }
    Ok(())
}
