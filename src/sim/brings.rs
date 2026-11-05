//! Brought one after another, simulated (frictions29 24): the world of `sim/sessions.rs` — two sessions with
//! commits of their own — and at random a bring asked (`:workspace-bring`, its card graded on a worker), one brought
//! as its card's `y` does (`workspace_bring_now`, fast-forward only), a new commit in a session, a frame, the user
//! taking their time. A landing grades the other sessions' waiting bring cards again (`bring_regrade`) — its jobs
//! among the rest, a bring asked meanwhile, another landing. And the bring queue (24 a, `bringqueue.rs`): both
//! marked, `B`, its card's `y` — the rebase and the quick preflight it starts as tasks stood in for by the
//! scenario (no task is modelled): it rebases the checkout itself (`git rebase main`, aborted at a conflict —
//! the two sessions' `a.rs` clash) and says a preflight passed or failed. And the one bring card's `y` (polish landing
//! 4): its rebase stood in for as the queue's, the rest of its way the real one (`bring_go_on`: graded, its preflight
//! stood in for, brought). And the project's `.tarae.toml` read asking trust for its preflight steps, its card answered
//! `y` or `n` later (#323: a new checkout asks again). After every step: one bring card a session, the queue holds each
//! session once, one under way at most, a bring going by itself has no card of its own up and says where it is in its
//! session's words, and one whose preflight the trust card holds back says it waits on it, that card up.
//!
//! Every bring waits its turn (#371): a plain bring card answered `y` (as a key), the one card's `y` while another bring
//! is under way, a file left in no commit in a session or taken away again, a commit on `main` from outside the queue.
//! After every step besides: one bring under way at a time, whoever asked it; a session in line has no bring card of
//! its own, each card up says the place its `y` would take (and none while nothing is under way), each row waiting says
//! its place; those in line go in the order asked; no preflight starts on a tree with changes in no commit, nor for
//! `risky`, whose high-risk change no one looked at (#379: its card waits, its `h`).

use std::time::Duration;

use super::sessions::{World, git, git_out, one_bring_card, print};
use super::{Choices, Sim};
use crate::bringqueue::Stage;
use crate::editor::Editor;
use crate::key::{Code, Key};

/// The seeds every `cargo test` runs, each twice — git's processes are most of a step (~40 steps a second). 2: a
/// bring waits on the trust card (#323).
const SEEDS: &[u64] = &[0, 2];

const INPUTS: usize = 30;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn brought_one_after_another() {
    super::seeds("brought_one_after_another", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    // (a third, clashing with neither: two can wait in line behind a third)
    let world = World::new().and("third", "t.rs", "fn t() {}\n");
    let mut sim = Sim::new("brought_one_after_another", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (world.repo.clone(), "proj".into());
    ed.workspace.git_branch = Some("main".into());
    ed.config.workspace_worktrees = world.dir.join("wt").display().to_string();
    let mut names = Vec::new();
    for (name, wt) in &world.sessions {
        let id = ed.workspace_open(wt).unwrap();
        if let Some(l) = ed.workspace_by_id(id) {
            (l.name, l.branch, l.base) = (name.clone(), Some(name.clone()), Some("main".into()));
        }
        names.push((name.clone(), wt.clone()));
    }
    ed.workspace_switch(crate::event::WorkspaceId::HOME);
    ed.bring_queue.stand_in = true;
    let dir = world.dir.display().to_string();
    // (each `y` that asked a bring, in order: those in line go as asked)
    let asked = std::cell::RefCell::new(Vec::new());
    let check = |ed: &Editor| {
        one_bring_card(ed)
            .and_then(|()| queue_held(ed))
            .and_then(|()| failed_said(ed))
            .and_then(|()| going_alone(ed))
            .and_then(|()| trust_said(ed))
            .and_then(|()| one_at_a_time(ed))
            .and_then(|()| line_said(ed))
            .and_then(|()| first_asked_first(ed, &asked.borrow()))
            .and_then(|()| preflights_checked(ed, &names))
    };
    let said = |ed: &Editor| format!("{} · {}{}", print(ed, &dir), queue_said(ed), trust_waits(ed));
    let step = |sim: &mut Sim, ed: &mut Editor| {
        let s = input(sim, ed, &names, &mut asked.borrow_mut(), &world.repo);
        // (an input is an event: what every event runs after it, the line's part — `handle_event`)
        ed.bring_queue_sync();
        s
    };
    sim.walk(&mut ed, INPUTS, step, check, said);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

type Asked = Vec<crate::event::WorkspaceId>;

fn input(
    sim: &mut Sim,
    ed: &mut Editor,
    names: &[(String, std::path::PathBuf)],
    asked: &mut Asked,
    repo: &std::path::Path,
) -> String {
    let c = &mut sim.choices;
    let (name, root) = c.pick(names).clone();
    // (by arm: a bring asked 4, one brought 2, a commit 2, a frame 1, time passing 1, the queue asked 1, the
    // queue's task or a single bring's preflight stood in for 3, a bring card's failed preflight 1, the one bring
    // card's `y` 2, the project's trust asked or answered 1, a plain bring card's `y` 2, a file in no commit 1, a
    // commit on main 1)
    match c.weighted(&[4, 2, 2, 1, 1, 1, 3, 1, 2, 1, 2, 1, 1]) {
        0 => match ed.workspace_bring(&name) {
            Ok(()) => format!(":workspace-bring {name}"),
            Err(e) => format!(":workspace-bring {name}: {e}"),
        },
        // (its card answered: gone as its `y` takes it — none while its own bring goes; a held one has no `y`)
        1 if !take_card(ed, &root) => format!("({name}: no bring card to answer)"),
        1 => match ed.workspace_bring_now(&name) {
            Ok(()) => format!("{name} brought (its card's y)"),
            Err(e) => format!("{name} brought: {e}"),
        },
        2 => {
            let file = format!("c{}.txt", sim.steps);
            std::fs::write(root.join(&file), "x\n").unwrap();
            git(&root, &["add", &file]);
            git(&root, &["commit", "-q", "-m", &file]);
            format!("{name}: a commit")
        }
        3 => {
            crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
            "a frame".into()
        }
        4 => {
            let ms = *c.pick(&[50, 600]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        5 => queue_asked(sim, ed, names, asked),
        6 => {
            let passes = *c.pick(&[true, false]);
            stand_in(ed, names, passes)
        }
        7 => preflight_failed(ed, &name, &root),
        8 => bring_go(ed, &name, &root, asked),
        9 => {
            let yes = *c.pick(&[true, false]);
            trust(ed, sim.steps, yes)
        }
        10 => card_y(ed, asked),
        11 if name == "risky" && c.weighted(&[1, 1]) == 0 => looked(ed, &root),
        11 => unclean(ed, &name, &root),
        _ => {
            let file = format!("m{}.txt", sim.steps);
            std::fs::write(repo.join(&file), "m\n").unwrap();
            git(repo, &["add", &file]);
            git(repo, &["commit", "-q", "-m", &file]);
            "main: a commit (from outside the queue)".into()
        }
    }
}

/// The bring card of the checkout at `root` taken away, as its `y` takes it — false when none is up (a held one has
/// no `y`).
fn take_card(ed: &mut Editor, root: &std::path::Path) -> bool {
    let up = |o: &crate::offer::Offer| o.bring_root() == Some(root) && o.held.is_none();
    let parked = std::iter::once(&ed.workspace).chain(&ed.workspaces).flat_map(|l| &l.parked);
    if !ed.offers.iter().chain(parked).any(up) {
        return false;
    }
    ed.offers.retain(|o| !up(o));
    for l in std::iter::once(&mut ed.workspace).chain(&mut ed.workspaces) {
        l.parked.retain(|o| !up(o));
    }
    true
}

/// `risky`'s high-risk change looked at (`space v h`): its bring goes on as any other's from here.
fn looked(ed: &mut Editor, root: &std::path::Path) -> String {
    let rules = crate::risk::Rules::of(&ed.config);
    let highs = crate::codereview::highs_between(root, "main", "HEAD", &rules).unwrap_or_default();
    let keys: Vec<crate::risk::Key> = highs.iter().flat_map(|h| h.keys.iter().copied()).collect();
    let n = keys.len();
    let all = std::iter::once(&mut ed.workspace).chain(&mut ed.workspaces);
    if let Some(l) = all.into_iter().find(|l| l.root == root) {
        l.looked.extend(keys);
    }
    format!("risky: its high-risk change looked at ({n} keys)")
}

/// The file a session's checkout has in no commit, while it's there (`unclean`).
const UNCLEAN: &str = "unclean.txt";

/// A file left in no commit in session `name`'s checkout — or, there, taken away — while no bring of it is under way
/// (one under way checked its tree as it began; the bring checks again as it lands).
fn unclean(ed: &mut Editor, name: &str, root: &std::path::Path) -> String {
    let Ok((id, _)) = ed.session_named(name) else { return format!("({name}: no session)") };
    if ed.bring_queued(id) || session(ed, id).is_some_and(|l| ed.bring_going(l) || l.rebase.is_some()) {
        return format!("({name}: its bring under way)");
    }
    let file = root.join(UNCLEAN);
    match file.exists() {
        true => {
            std::fs::remove_file(&file).unwrap();
            format!("{name}: its file in no commit taken away")
        }
        false => {
            std::fs::write(&file, "x\n").unwrap();
            format!("{name}: a file left in no commit")
        }
    }
}

/// The plain bring card in front (no high-risk hold, no failed preflight) answered `y` as a key, once drawn and
/// focused: the preflight then the bring — or, another bring under way, in line behind it.
fn card_y(ed: &mut Editor, asked: &mut Asked) -> String {
    let plain = |o: &crate::offer::Offer| {
        matches!(o.what, crate::offer::What::Rebased(_)) && o.held.is_none() && o.warnings.is_empty()
    };
    let Some(root) =
        ed.offers.first().filter(|o| plain(o)).and_then(|o| o.bring_root()).map(|r| r.to_path_buf())
    else {
        return "(no plain bring card in front)".into();
    };
    let Some(id) = ed.session_at(&root) else { return "(its session is gone)".into() };
    crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
    ed.card_focus_front();
    ed.handle_key(Key::plain(Code::Char('y')));
    asked.push(id);
    let name = session(ed, id).map(|l| l.label().to_string()).unwrap_or_default();
    match ed.bring_queued(id) {
        true => format!("{name}'s bring card: y — in line, {}", ed.bring_said(id).unwrap_or_default()),
        false => format!("{name}'s bring card: y"),
    }
}

/// Whether session `l`'s bring, going by itself, holds the line — not one stopped at its rebase's conflict or
/// resolved there (that waits on a person).
fn holds(ed: &Editor, l: &crate::workspace::Workspace) -> bool {
    use crate::rebase::Stage as R;
    let waits_on_you = l.rebase.as_ref().is_some_and(|r| match r.stage {
        R::Running { .. } => false,
        R::Resolved => true,
        R::Stopped => r.stop != Default::default(),
    });
    ed.bring_going(l) && !waits_on_you && !ed.bring_queued(l.id)
}

/// The one bring under way at a time (#371): the queue's, or a bring going by itself — never two at once, whoever
/// asked them.
fn one_at_a_time(ed: &Editor) -> Result<(), String> {
    let all = || std::iter::once(&ed.workspace).chain(&ed.workspaces);
    let mut going: Vec<String> =
        all().filter(|l| holds(ed, l)).map(|l| format!("{} (by itself)", l.label())).collect();
    if let Some((id, stage)) = ed.bring_queue.run.as_ref().and_then(|r| r.now) {
        going.extend(session(ed, id).map(|l| format!("{} (the queue's, {stage:?})", l.label())));
    }
    match going.len() {
        0 | 1 => Ok(()),
        _ => Err(format!("two brings under way at once: {}", going.join(", "))),
    }
}

/// A session in line has no bring card of its own; each plain or rebase card up says the place its `y` would take —
/// after the one under way and those waiting — and none says one while its `y` would bring at once; each row waiting
/// says its place, the one under way first.
fn line_said(ed: &Editor) -> Result<(), String> {
    let run = ed.bring_queue.run.as_ref();
    let in_line: Vec<crate::event::WorkspaceId> =
        run.iter().flat_map(|r| r.left.iter().copied().chain(r.now.map(|(id, _)| id))).collect();
    let going = run.is_some_and(|r| r.now.is_some())
        || std::iter::once(&ed.workspace).chain(&ed.workspaces).any(|l| holds(ed, l));
    let ahead = usize::from(going) + run.map_or(0, |r| r.left.len());
    for o in all_cards(ed) {
        let Some(root) = o.bring_root() else { continue };
        let Some(id) = ed.session_at(root) else { continue };
        if in_line.contains(&id) {
            return Err(format!("in line, and card#{} {:?} asks", o.id, o.title));
        }
        let plain = matches!(o.what, crate::offer::What::Rebase(_) | crate::offer::What::Rebased(_));
        if !plain || !o.warnings.is_empty() {
            continue;
        }
        let place = o.question.rsplit_once(": ").and_then(|(_, p)| p.strip_suffix(" in line."));
        let says = o.question.contains("\n\nAnother bring is under way (");
        let want = nth(ahead + 1);
        if says != ed.bring_waits(id) || (says && place != Some(want.as_str())) {
            return Err(format!(
                "card#{} {:?} says {place:?} ({says}), {ahead} ahead: {:?}",
                o.id, o.title, o.question
            ));
        }
    }
    for (i, id) in run.iter().flat_map(|r| r.left.iter().copied()).enumerate() {
        let want = format!("bring {}", nth(i + 1 + usize::from(going)));
        if ed.bring_said(id).as_deref() != Some(want.as_str()) {
            return Err(format!("session {id} waits {}: its row {:?}", nth(i + 1), ed.bring_said(id)));
        }
    }
    Ok(())
}

/// `1st` … as the cards and rows say it (the scenario's own).
fn nth(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Those waiting in line go in the order their `y`s asked (`asked`, each one's last).
fn first_asked_first(ed: &Editor, asked: &[crate::event::WorkspaceId]) -> Result<(), String> {
    let Some(run) = ed.bring_queue.run.as_ref() else { return Ok(()) };
    let when = |id: &crate::event::WorkspaceId| asked.iter().rposition(|a| a == id);
    let order: Vec<Option<usize>> = run.left.iter().map(when).collect();
    match order.windows(2).all(|w| w[0] < w[1]) {
        true => Ok(()),
        false => Err(format!("in line {:?}, asked {asked:?}", run.left)),
    }
}

/// No preflight starts on a tree with a file in no commit, nor for `risky`, its high-risk change not looked at (#379).
fn preflights_checked(ed: &Editor, names: &[(String, std::path::PathBuf)]) -> Result<(), String> {
    for l in std::iter::once(&ed.workspace).chain(&ed.workspaces) {
        let queued = ed.bring_queue_at(l.id) == Some(Stage::Preflight);
        if !queued && !(l.bring_after_preflight && !l.bring_trust) {
            continue;
        }
        let Some((name, root)) = names.iter().find(|(_, r)| *r == l.root) else { continue };
        if root.join(UNCLEAN).exists() {
            return Err(format!("{name}'s preflight runs with a file in no commit"));
        }
        if name == "risky" && l.looked.is_empty() {
            return Err("risky's preflight runs, its high-risk change not looked at".into());
        }
    }
    Ok(())
}

/// The project's `.tarae.toml` read again, asking trust for its preflight steps (#323) — while no preflight of a
/// bring has started (one under way runs what it was given) — or, asking, its card answered (`yes`).
fn trust(ed: &mut Editor, step: usize, yes: bool) -> String {
    if let Some(p) = ed.project_trust.as_ref().filter(|p| p.state == crate::trust::State::Ask) {
        let hash = p.hash;
        let id = all_cards(ed)
            .find(|o| matches!(o.what, crate::offer::What::ProjectTrust { .. }))
            .map(|o| o.id)
            .unwrap_or_default();
        ed.project_trust_answer(id, hash, yes);
        return format!("its trust card: {}", if yes { "y" } else { "n" });
    }
    let all = || std::iter::once(&ed.workspace).chain(&ed.workspaces);
    if all().any(|l| l.bring_after_preflight && !l.bring_trust) {
        return "(a preflight under way: its file read later)".into();
    }
    let both = Box::new((ed.config.clone(), ed.keymaps.clone()));
    let file = ed.workspace.root.join(".tarae.toml");
    let project = crate::trust::Project {
        place: crate::trust::place_of(&file),
        file,
        hash: [step as u8; 32],
        items: vec![("preflight.check".into(), "true".into())],
        state: crate::trust::State::Ask,
        with: both.clone(),
        without: both,
        read_at: 0,
    };
    ed.project_trust_set(Some(project));
    ".tarae.toml read: its preflight steps wait on its trust card".into()
}

/// Every card, wherever it is: in front, stacked, parked with its session.
fn all_cards(ed: &Editor) -> impl Iterator<Item = &crate::offer::Offer> {
    let parked = std::iter::once(&ed.workspace).chain(&ed.workspaces).flat_map(|l| &l.parked);
    ed.offers.iter().chain(parked)
}

/// The sessions whose bring waits on the trust card, said.
fn trust_waits(ed: &Editor) -> String {
    let all = || std::iter::once(&ed.workspace).chain(&ed.workspaces);
    let names: Vec<&str> = all().filter(|l| ed.bring_waits_trust(l)).map(|l| l.label()).collect();
    match names.is_empty() {
        true => String::new(),
        false => format!(" · waits on trust: {}", names.join(", ")),
    }
}

/// A bring whose preflight the project's trust holds back waits on its card, says so in its session's words, and the
/// card is up — never stopped unsaid (#323).
fn trust_said(ed: &Editor) -> Result<(), String> {
    let held = ed.config_project_held("preflight.");
    let up = all_cards(ed).any(|o| match &o.what {
        crate::offer::What::ProjectTrust { .. } => true,
        crate::offer::What::RepoTrust { parts, .. } => {
            parts.iter().any(|p| matches!(p.what, crate::offer::What::ProjectTrust { .. }))
        }
        _ => false,
    });
    let all = || std::iter::once(&ed.workspace).chain(&ed.workspaces);
    for l in all().filter(|l| l.bring_after_preflight && ed.bring_queue_at(l.id).is_none()) {
        let words = ed.bring_progress(l).unwrap_or_default();
        match (held, l.bring_trust) {
            (true, false) => {
                return Err(format!("{} stalls unsaid: its preflight held back, {words:?}", l.label()));
            }
            (true, true) if !words.ends_with("waits on trust") || !up => {
                return Err(format!("{} waits on trust: its words {words:?}, its card up {up}", l.label()));
            }
            _ => {}
        }
    }
    Ok(())
}

/// The one bring card's `y` on session `name` (`rebase_start`'s `bring_go`), its rebase stood in for — run here,
/// aborted at a conflict, as the queue's — and its end as `rebase_ended`'s goes on: graded on a worker, its quick
/// preflight (stood in for by arm 6), brought.
/// Another bring under way: its `y` lines it up instead (`rebase_start`, as the card's `y` — no rebase runs then).
fn bring_go(ed: &mut Editor, name: &str, root: &std::path::Path, asked: &mut Asked) -> String {
    let Ok((id, _)) = ed.session_named(name) else { return format!("({name}: no session)") };
    let free =
        |l: &crate::workspace::Workspace| l.rebase.is_none() && !l.bring_go && !l.bring_after_preflight;
    if ed.bring_queued(id) || !session(ed, id).is_some_and(free) {
        return format!("({name}: its bring under way)");
    }
    // (its card, answered: gone as its `y` takes it — a held one has no `y`)
    let up = |o: &crate::offer::Offer| o.bring_root() == Some(root) && o.held.is_none();
    let parked = std::iter::once(&ed.workspace).chain(&ed.workspaces).flat_map(|l| &l.parked);
    let Some(card) = ed.offers.iter().chain(parked).find(|o| up(o)).map(|o| o.id) else {
        return format!("({name}: no bring card to answer)");
    };
    asked.push(id);
    if ed.bring_waits(id) {
        let m = crate::rebase::Moved {
            root: root.to_path_buf(),
            name: name.into(),
            branch: name.into(),
            base: "main".into(),
            moved: 1,
            ahead: 1,
        };
        ed.rebase_start(card, m);
        ed.offers.retain(|o| o.id != card);
        for l in std::iter::once(&mut ed.workspace).chain(&mut ed.workspaces) {
            l.parked.retain(|o| o.id != card);
        }
        return format!(
            "{name}: y — another bring under way, in line: {}",
            ed.bring_said(id).unwrap_or_default()
        );
    }
    ed.offers.retain(|o| !up(o));
    for l in std::iter::once(&mut ed.workspace).chain(&mut ed.workspaces) {
        l.parked.retain(|o| !up(o));
    }
    if !rebase(root) {
        return format!("{name}: y, its rebase stopped at a conflict — aborted, nothing goes on");
    }
    let ahead = git_out(root, &["rev-list", "--count", "main..HEAD"]).trim().parse().unwrap_or(0);
    let m = crate::rebase::Moved {
        root: root.to_path_buf(),
        name: name.into(),
        branch: name.into(),
        base: "main".into(),
        moved: 0,
        ahead,
    };
    ed.with_workspace(id, |ed| {
        (ed.workspace.bring_go, ed.workspace.bring_rebased) = (true, true);
        let quick = ed.preflight_quick();
        ed.bring_go_on(m, quick);
    });
    format!("{name}: y, rebased — on to its grade, preflight and bring")
}

/// Session `id`, wherever it is.
fn session(ed: &Editor, id: crate::event::WorkspaceId) -> Option<&crate::workspace::Workspace> {
    std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == id)
}

/// A bring going by itself (its card's `y`) has no bring card of its own up anywhere, and its session's words say it.
fn going_alone(ed: &Editor) -> Result<(), String> {
    let all = || std::iter::once(&ed.workspace).chain(&ed.workspaces);
    let cards: Vec<&crate::offer::Offer> = all().flat_map(|l| &l.parked).chain(&ed.offers).collect();
    for l in all().filter(|l| ed.bring_going(l)) {
        if let Some(o) = cards.iter().find(|o| o.bring_root() == Some(l.root.as_path())) {
            return Err(format!("{} is being brought, and card#{} {:?} asks", l.label(), o.id, o.title));
        }
        let words = ed.bring_progress(l).unwrap_or_default();
        if !words.starts_with("bringing") {
            return Err(format!("{} is being brought, its words {words:?}", l.label()));
        }
    }
    Ok(())
}

/// A session's bring card answered `p` (the card gone, as its `p` takes it), the preflight it ran stood in for and
/// failing (no task is modelled): the card is back saying so (frictions34 42) — one bring card a session still,
/// whatever was asked meanwhile.
fn preflight_failed(ed: &mut Editor, name: &str, root: &std::path::Path) -> String {
    let Ok((id, _)) = ed.session_named(name) else { return format!("({name}: no session)") };
    if ed.bring_queue_at(id).is_some() {
        return format!("({name} is the queue's)");
    }
    let m = crate::rebase::Moved {
        root: root.to_path_buf(),
        name: name.into(),
        branch: name.into(),
        base: "main".into(),
        moved: 0,
        ahead: 1,
    };
    ed.with_workspace(id, |ed| {
        (ed.workspace.bring_after_preflight, ed.workspace.bring_card_after) = (true, Some(Box::new(m)));
        ed.rebase_preflight_done(false, "quick preflight");
    });
    format!("{name}'s bring card: p, its preflight failed")
}

/// `space W`'s `m` on each session, `B`, and the card's `y` once it's drawn and focused — behind a queue going, they
/// line up after it.
fn queue_asked(
    _sim: &mut Sim,
    ed: &mut Editor,
    names: &[(String, std::path::PathBuf)],
    asked: &mut Asked,
) -> String {
    ed.bring_queue.marks.clear();
    for (name, _) in names {
        if let Ok((id, _)) = ed.session_named(name) {
            ed.bring_mark(id);
        }
    }
    let marks = ed.bring_queue.marks.clone();
    if let Err(e) = ed.bring_queue_ask() {
        return format!("B: {e}");
    }
    let front = |ed: &Editor| ed.offers.first().is_some_and(|o| o.kind() == "bring-queue");
    if !front(ed) {
        return "B: its card behind another".into();
    }
    crate::term::render(ed, &mut Vec::new(), 112, 30).unwrap();
    // (focused — a press on it — then its `y`)
    ed.card_focus_front();
    ed.handle_key(Key::plain(Code::Char('y')));
    asked.extend(marks.into_iter().filter(|id| ed.bring_queued(*id)));
    "B, and y on its card".into()
}

/// What the queue's one under way waits on, ended by the scenario: its rebase — run here, aborted at a conflict —
/// or its preflight (`passes`).
fn stand_in(ed: &mut Editor, names: &[(String, std::path::PathBuf)], passes: bool) -> String {
    let Some((id, stage)) = ed.bring_queue.run.as_ref().and_then(|r| r.now) else {
        return single_preflight(ed, names, passes);
    };
    let Some((name, root)) = names.iter().find(|(n, _)| ed.session_named(n).is_ok_and(|(i, _)| i == id))
    else {
        return "(the queue's one isn't the scenario's)".into();
    };
    match stage {
        Stage::Rebasing => {
            let rebased = rebase(root);
            let done = if rebased { Ok(()) } else { Err("its rebase stopped at a conflict".into()) };
            ed.with_workspace(id, |ed| {
                ed.bring_queue_rebased(done);
            });
            format!("{name}'s rebase {}", if rebased { "done" } else { "stopped: aborted" })
        }
        Stage::Preflight => {
            ed.with_workspace(id, |ed| ed.rebase_preflight_done(passes, "quick preflight"));
            format!("{name}'s preflight {}", if passes { "passed" } else { "failed" })
        }
        s => format!("({name}: {s:?}, nothing to stand in for)"),
    }
}

/// A single bring's preflight (`bring_go`'s, once graded) ended by the scenario (`passes`).
fn single_preflight(ed: &mut Editor, names: &[(String, std::path::PathBuf)], passes: bool) -> String {
    let waiting = |ed: &Editor, n: &str| {
        let id = ed.session_named(n).ok()?.0;
        // (one waiting on its trust card hasn't started)
        session(ed, id).filter(|l| l.bring_after_preflight && !l.bring_trust).map(|_| id)
    };
    let Some((name, id)) = names.iter().find_map(|(n, _)| waiting(ed, n).map(|id| (n.clone(), id))) else {
        return "(no preflight under way)".into();
    };
    ed.with_workspace(id, |ed| ed.rebase_preflight_done(passes, "quick preflight"));
    format!("{name}'s preflight {}", if passes { "passed" } else { "failed" })
}

/// `git rebase main` in `root` as the scenario's own git runs — aborted at a conflict: whether it went through.
fn rebase(root: &std::path::Path) -> bool {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(["-c", "core.hooksPath=/dev/null", "-C"])
            .arg(root)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_CONFIG_HOME", "/dev/null")
            .env("GIT_AUTHOR_DATE", "2026-10-18T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-10-18T00:00:00Z")
            .output()
            .unwrap()
            .status
            .success()
    };
    run(&["rebase", "-q", "main"]) || !run(&["rebase", "--abort"])
}

/// A bring card back from a failed preflight reads as one wherever it is — its risk warned, its `y` a bring anyway —
/// and none says the plain card's words with the failed one's output behind `v` (frictions29 91: a bring asked
/// again put them over it).
fn failed_said(ed: &Editor) -> Result<(), String> {
    let parked = std::iter::once(&ed.workspace).chain(&ed.workspaces).flat_map(|l| &l.parked);
    for o in ed.offers.iter().chain(parked) {
        if !matches!(o.what, crate::offer::What::Rebased(_)) {
            continue;
        }
        let failed = o.question.contains("preflight failed");
        // (a held one has no `y` — `codereview::push_bring_card`)
        let y = o.answers(true).first().filter(|a| a.0 == 'y').map(|a| a.1);
        let anyway = o.held.is_some() || y == Some("Bring anyway");
        if failed != !o.warnings.is_empty() || (failed && !anyway) || (o.output.is_some() && !failed) {
            return Err(format!("card#{} {:?} reads as {y:?}: {:?}", o.id, o.title, o.question));
        }
    }
    Ok(())
}

/// The queue holds each session once, and one under way at most.
fn queue_held(ed: &Editor) -> Result<(), String> {
    let Some(run) = ed.bring_queue.run.as_ref() else { return Ok(()) };
    let mut all: Vec<crate::event::WorkspaceId> =
        run.left.iter().copied().chain(run.now.map(|(id, _)| id)).collect();
    let n = all.len();
    all.sort_unstable();
    all.dedup();
    match all.len() == n {
        true => Ok(()),
        false => Err(format!("a session twice in the queue: {:?} · {:?}", run.left, run.now)),
    }
}

/// The queue as it is: the one under way and its stage, those left, brought and out.
fn queue_said(ed: &Editor) -> String {
    let Some(run) = ed.bring_queue.run.as_ref() else { return "no queue".into() };
    let brought: Vec<&str> = run.brought.iter().map(|(n, _)| n.as_str()).collect();
    let out: Vec<String> = run.out.iter().map(|(n, why)| format!("{n}: {why}")).collect();
    format!("queue now {:?} left {:?} brought {brought:?} out {out:?}", run.now, run.left)
}
