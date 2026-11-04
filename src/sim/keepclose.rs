//! Workspaces kept close, simulated (collab C3, `keepclose.rs`): a repository on `main` and two workspaces on it —
//! `fix`, whose Claude (a stand-in) takes messages and has runs, and `up` — while `main` moves again and again, the
//! card is answered `y` or `n` or left, fix's turns start and end and its runs come and go, time passes and frames are
//! drawn: the overlap rounds, looks and moves on their workers landing among the steps.
//!
//! Once, fix changes `a.txt` in no commit — a file main's every move rewrites too: a move judged before that conflicts
//! after all, and must leave fix as it was.
//!
//! fix's turns end behind (`done` until looked at), and its turn's own review opens and closes (review at the end opens
//! one as a turn ends) — each arriving as a result of fix's does, the loop's every-event round after it.
//!
//! After every step: at most one card a base, on a tip read; while fix moves its Claude isn't at work and nothing runs
//! there (a message or a run asked meanwhile waits); a message waiting goes once nothing holds it; fix, not moving,
//! holds no conflict marker, nothing left set aside, and its change as written. Friction #360: with nothing moving, no
//! workspace idle by the scenario's own terms (a turn ended and not looked at is) still waits; the card says what each
//! of its workspaces waits for as it is now; a `y` leaves each workspace it moves waiting or moving, an `n` none of its
//! card's waiting; and a workspace a `y` was given for — carried onto every newer tip while it waits — is on that tip
//! once nothing holds it (fix but with its change, which can't go onto any later tip).

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::event::{Event, WorkspaceId};
use crate::key::{Code, Key};
use crate::offer::What;
use crate::workspace::Workspace;

/// Two seeds, ~7 s each at opt-level 0 (2026-10-23, macOS): seed 2 asks a run in fix while it moves, and moves fix
/// after its change in no commit, a conflict after all (2026-10-25: of seeds 0–9 the one that does either — a move
/// is in flight one job's time).
const SEEDS: &[u64] = &[0, 2];

const INPUTS: usize = 40;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn workspaces_follow_their_base_only_when_idle() {
    super::seeds("workspaces_follow_their_base_only_when_idle", SEEDS, run);
}

/// A script's act, taken as an input once it can be — an answer, and what the card says, once the newest tip's card is
/// up — the jobs landing among the acts in the seed's order.
#[derive(Clone, Copy, Debug)]
enum Act {
    Message,
    TurnEnds,
    MainMoves,
    /// fix's turn review opens, or closes.
    Review,
    /// fix's row on the card says it moves once that (None: says nothing of it).
    Says(Option<&'static str>),
    Answer(char),
}

/// Friction #360's cases: a `y` while fix's Claude works, its turn's end moving it; a `y` while fix is `done` (its turn
/// ended behind, not looked at), moved at once; a newer move while fix waits, its `y` going on onto the newest tip with
/// that card unanswered; fix's turn review, opened while the card is up, said on it and holding fix until it's closed;
/// an `n` on the newer card letting fix's wait go.
const CASES: &[(&str, &[Act])] = {
    use Act::*;
    &[
        ("busy", &[Message, MainMoves, Says(Some("its turn ends")), Answer('y'), TurnEnds]),
        ("done", &[Message, TurnEnds, MainMoves, Says(None), Answer('y')]),
        ("newer", &[Message, MainMoves, Answer('y'), MainMoves, Says(Some("its turn ends")), TurnEnds]),
        (
            "review",
            &[
                Message,
                TurnEnds,
                MainMoves,
                Says(None),
                Review,
                Says(Some("its turn's review is closed")),
                Answer('y'),
                Review,
            ],
        ),
        ("n", &[Message, MainMoves, Answer('y'), MainMoves, Answer('n'), TurnEnds]),
    ]
};

#[test]
fn a_yes_moves_each_workspace_once_its_idle() {
    for (name, acts) in CASES {
        let test = format!("a_yes_moves_each_workspace_once_its_idle_{name}");
        super::seeds(&test, &[0], |choices| scripted(&test, choices, acts));
    }
}

fn scripted(name: &str, choices: Choices, acts: &'static [Act]) -> Vec<String> {
    let at = std::cell::Cell::new(0);
    let input = |sim: &mut Sim, ed: &mut Editor, w: &mut World| {
        let Some(act) = acts.get(at.get()) else { return "(the script is done)".into() };
        let newest =
            card(ed).is_some_and(|o| matches!(&o.what, What::BaseMoved(m) if Some(&m.to) == w.tips.last()));
        if matches!(act, Act::Says(_) | Act::Answer(_)) && !newest {
            return "(the newest tip's card isn't up yet)".into();
        }
        at.set(at.get() + 1);
        match *act {
            Act::Message => message(sim, ed, w),
            Act::TurnEnds => turn_ends(sim, ed, w),
            Act::MainMoves => main_moves(ed, w),
            Act::Review => review(sim, ed, w),
            Act::Says(why) => {
                let Some(What::BaseMoved(m)) = card(ed).map(|o| &o.what) else { unreachable!() };
                let row = m.ws.iter().find(|x| x.id == w.fix).expect("fix on the card");
                assert_eq!(row.holds, why, "what the card says fix waits for");
                let said = card(ed).unwrap().question.lines().find(|l| l.trim_start().starts_with("fix —"));
                let said = said.expect("fix's row").to_string();
                match why {
                    Some(why) => assert!(said.ends_with(&format!("moves once {why}")), "{said}"),
                    None => assert!(!said.contains("moves once"), "{said}"),
                }
                format!("the card says {said:?}")
            }
            Act::Answer(key) => answer(ed, w, key),
        }
    };
    let end = |_: &Editor, _: &World| match acts.get(at.get()) {
        None => Ok(()),
        Some(act) => Err(format!("the script stopped at {act:?}")),
    };
    walked(name, choices, SCRIPTED, input, end)
}

/// A script's inputs at most: its acts and the waits for a card among them.
const SCRIPTED: usize = 26;

struct World {
    main: PathBuf,
    fix: WorkspaceId,
    up: WorkspaceId,
    base: crate::keepclose::Base,
    /// main's tips, oldest first — the last the newest.
    tips: Vec<String>,
    /// The workspaces a `y` was given for, each with the tip it's owed: the newest as the `y` came, carried onto a
    /// newer one while it waits; an `n` on a card naming it lets it go.
    owed: Vec<(WorkspaceId, String)>,
    /// Those seen on the tip they're owed (a move only goes forward: asked of git once).
    on: RefCell<Vec<(WorkspaceId, String)>>,
    moves: usize,
    /// fix's `a.txt` changed in no commit.
    changed: bool,
    /// The stand-in claude's lines (kept: a dropped receiver fails its writes).
    _rx: std::sync::mpsc::Receiver<String>,
}

fn run(choices: Choices) -> Vec<String> {
    walked("workspaces_follow_their_base_only_when_idle", choices, INPUTS, step, |_, _| Ok(()))
}

/// The scene — main, fix and up — walked by `input`; once all is still, `still` and `end` hold.
fn walked(
    name: &str,
    choices: Choices,
    inputs: usize,
    mut input: impl FnMut(&mut Sim, &mut Editor, &mut World) -> String,
    end: impl Fn(&Editor, &World) -> Result<(), String>,
) -> Vec<String> {
    let mut sim = Sim::new(name, choices, LIMIT);
    let world = super::sessions::World::new();
    let main = world.repo.clone();
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = std::fs::canonicalize(&main).unwrap();
    ed.workspace.name = "main".into();
    crate::git::watched(&mut ed.workspace, "main");
    ed.overlap.dir = Some(world.dir.join("overlap"));
    ed.overlap.quiet = Duration::ZERO;
    let mut ids = Vec::new();
    let mut rx = None;
    for (name, root) in &world.sessions {
        let id = ed.workspace_add(std::fs::canonicalize(root).unwrap());
        let l = ed.workspace_by_id(id).unwrap();
        l.base = Some("main".into());
        crate::git::watched(l, name);
        l.name = if ids.is_empty() { "fix" } else { "up" }.into();
        if ids.is_empty() {
            let mut c = crate::chat::Chat::new();
            rx = Some(crate::chat::stand_in(&mut c));
            l.claude.chat = Some(c);
        }
        ids.push(id);
    }
    let common = crate::gitrepo::repo(&main).unwrap().common_dir.clone();
    let base = (common, "main".to_string());
    let t = tip_of(&main);
    crate::keepclose::tip(&mut ed, base.clone(), t.clone());
    let w = RefCell::new(World {
        main,
        fix: ids[0],
        up: ids[1],
        base,
        tips: vec![t],
        owed: Vec::new(),
        on: RefCell::new(Vec::new()),
        moves: 0,
        changed: false,
        _rx: rx.unwrap(),
    });
    let check = |ed: &Editor| held(ed, &w.borrow());
    sim.walk(&mut ed, inputs, |sim, ed| input(sim, ed, &mut w.borrow_mut()), check, print);
    let trace = sim.trace().to_vec();
    if let Err(e) = still(&ed, &w.borrow()).and_then(|()| end(&ed, &w.borrow())) {
        panic!("once all was still: {e}\n{}", trace.join("\n"));
    }
    drop(sim);
    drop(ed);
    drop(world);
    trace
}

fn tip_of(dir: &std::path::Path) -> String {
    let out =
        std::process::Command::new("git").args(["rev-parse", "main"]).current_dir(dir).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn session(ed: &Editor, id: WorkspaceId) -> &Workspace {
    std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == id).unwrap()
}

fn card(ed: &Editor) -> Option<&crate::offer::Offer> {
    ed.offers.iter().find(|o| matches!(o.what, What::BaseMoved(_)))
}

/// `work` done in fix as a result of its lands there (`Event::For`: a turn's end, a message sent, a run's end) — the
/// loop's every-event round after it (`keepclose::sync`), as after a real one.
fn in_fix(sim: &Sim, ed: &mut Editor, fix: WorkspaceId, work: impl FnOnce(&mut Editor) + Send + 'static) {
    let ev = Event::For(fix, ed.claude_of(fix), Box::new(work));
    sim.batch(ed, ev);
}

fn step(sim: &mut Sim, ed: &mut Editor, w: &mut World) -> String {
    let c = &mut sim.choices;
    // (by arm: main moves 3, a message to fix 3, fix's turn ends 2, a run in fix 3, y 4, n 1, time 1, a frame 1, fix's
    // change 1, fix's turn review 2 — `y` and fix's asks weighed up: a move is in flight one job's time, and they're
    // what must meet it)
    match c.weighted(&[3, 3, 2, 3, 4, 1, 1, 1, 1, 2]) {
        0 => main_moves(ed, w),
        1 => message(sim, ed, w),
        2 => turn_ends(sim, ed, w),
        3 => {
            let l = session(ed, w.fix);
            let running = l.task_run.as_ref().is_some_and(|r| r.state == crate::task::State::Running);
            let moving = l.moving.is_some();
            let task = crate::task::Task { name: "build".into(), cmd: "true".into(), cwd: l.root.clone() };
            in_fix(sim, ed, w.fix, move |ed| match (running, moving) {
                (true, _) => {
                    if let Some(r) = ed.workspace.task_run.as_mut() {
                        r.state = crate::task::State::Passed;
                    }
                    crate::queued::run_next(ed);
                }
                // (asked while it moves: it must wait — then taken out of the line, as `:procs`' `s` does: this
                // scenario runs no process)
                (false, true) => {
                    ed.task_start(task);
                    let ran = ed
                        .workspace
                        .task_run
                        .as_ref()
                        .is_some_and(|r| r.state == crate::task::State::Running);
                    assert!(!ran, "a run started while fix moved");
                    let id = ed.workspace.runs_next.front().map(|n| n.id).expect("the run waits in line");
                    crate::queued::leave(ed, id, "the scenario's");
                }
                (false, false) => {
                    let mut r = crate::task::Run::for_test(task);
                    r.state = crate::task::State::Running;
                    ed.workspace.task_run = Some(r);
                }
            });
            match (running, moving) {
                (true, _) => "fix's run ends".into(),
                (false, true) => "a run asked in fix while it moves".into(),
                (false, false) => "a run starts in fix".into(),
            }
        }
        k @ (4 | 5) => answer(ed, w, if k == 4 { 'y' } else { 'n' }),
        6 => {
            let ms = *c.pick(&[100, 1500]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        7 => {
            crate::term::render(ed, &mut Vec::new(), 100, 30).unwrap();
            "a frame".into()
        }
        // (the person's own write, never mid-move: theirs are held then — `hold_save`)
        8 => {
            let l = session(ed, w.fix);
            if w.changed || l.moving.is_some() {
                return "(fix's change is there, or it moves)".into();
            }
            std::fs::write(l.root.join("a.txt"), FIXS).unwrap();
            w.changed = true;
            "fix changes a.txt, in no commit".into()
        }
        _ => review(sim, ed, w),
    }
}

/// main moves on: a commit there, its tip read as the watch reads it.
fn main_moves(ed: &mut Editor, w: &mut World) -> String {
    w.moves += 1;
    let file = w.main.join("main.txt");
    std::fs::write(&file, format!("move {}\n", w.moves)).unwrap();
    std::fs::write(w.main.join("a.txt"), format!("one, move {}\n", w.moves)).unwrap();
    super::sessions::git(&w.main, &["add", "main.txt", "a.txt"]);
    super::sessions::git(&w.main, &["commit", "-q", "-m", &format!("move {}", w.moves)]);
    let t = tip_of(&w.main);
    w.tips.push(t.clone());
    // (a workspace still waiting from a `y` goes onto this tip: the newest wins)
    for (id, owed) in &mut w.owed {
        if ed.keep_close.waits.iter().any(|x| x.ws.id == *id) {
            *owed = t.clone();
        }
    }
    crate::keepclose::tip(ed, w.base.clone(), t);
    format!("main moves ({})", w.moves)
}

/// A message to fix's Claude: its turn starts (or waits while fix moves).
fn message(sim: &Sim, ed: &mut Editor, w: &World) -> String {
    let moving = session(ed, w.fix).moving.is_some();
    in_fix(sim, ed, w.fix, |ed| {
        if let Some(c) = ed.workspace.claude.chat.as_mut() {
            c.input.set("go on");
        }
        crate::chat::send(ed);
    });
    format!("a message to fix{}", if moving { " (it moves)" } else { "" })
}

/// fix's turn ends, behind.
fn turn_ends(sim: &Sim, ed: &mut Editor, w: &World) -> String {
    if !session(ed, w.fix).claude.chat.as_ref().is_some_and(crate::chat::Chat::busy) {
        return "(fix's Claude is idle)".into();
    }
    in_fix(sim, ed, w.fix, |ed| {
        crate::chat::stand_in_done(ed);
        // (ended behind, as a real turn's end marks it: `done` until it's looked at — `chat::on_stream_event`)
        if ed.background() {
            ed.workspace.claude.turn_end = Some((crate::clock::now(), Ok(())));
        }
    });
    "fix's turn ends".into()
}

/// fix's turn review opens (review at the end opens the turn's own as a turn ends, behind too —
/// `chat::turn_gathered`), or the person closes it, read: a reason fix isn't moved, said on the card.
fn review(sim: &Sim, ed: &mut Editor, w: &World) -> String {
    let l = session(ed, w.fix);
    let (open, root) = (l.code_review.is_some(), l.root.clone());
    in_fix(sim, ed, w.fix, move |ed| {
        ed.workspace.code_review = match open {
            true => None,
            false => {
                let turn = crate::codereview::Base::Turn { at: 0, question: "go on".into() };
                Some(crate::codereview::CodeReview::for_test(turn, root, Vec::new()))
            }
        };
    });
    match open {
        true => "fix's turn review closed".into(),
        false => "fix's turn review opens".into(),
    }
}

/// `key` on the card up, drawn and focused first: `y` leaves each clean workspace on it waiting or moving — owed
/// the newest tip —, `n` none of them waiting.
fn answer(ed: &mut Editor, w: &mut World, key: char) -> String {
    let Some(to) = card(ed).map(|o| match &o.what {
        What::BaseMoved(m) => m.to.clone(),
        _ => unreachable!(),
    }) else {
        return "(no card)".into();
    };
    crate::term::render(ed, &mut Vec::new(), 120, 40).unwrap();
    let (id, rev, m) = card(ed)
        .map(|o| match &o.what {
            What::BaseMoved(m) => (o.id, o.rev, (**m).clone()),
            _ => unreachable!(),
        })
        .unwrap();
    ed.card_focus = Some((id, rev));
    ed.handle_key(Key::plain(Code::Char(key)));
    if ed.offers.iter().any(|o| o.id == id) {
        return format!("{key} on the card, not taken");
    }
    let waits = |ed: &Editor, id: WorkspaceId| ed.keep_close.waits.iter().any(|x| x.ws.id == id);
    let named: Vec<WorkspaceId> = m.ws.iter().map(|x| x.id).collect();
    if key == 'n' {
        // (Friction #360: `n` leaves none of them waiting, from this card or an earlier `y`)
        if let Some(id) = named.iter().find(|id| waits(ed, **id)) {
            panic!("{} still waits after the card's n", session(ed, *id).name);
        }
        w.owed.retain(|(id, _)| !named.contains(id));
        return format!("n on the card (main at {})", &to[..7]);
    }
    // (`y` moves the clean ones: each waiting, or moving — never dropped for being busy as the `y` came, #360; a
    // conflicting one goes to the resolve loop, its own card's)
    let newest = w.tips.last().unwrap().clone();
    for x in m.ws.iter().filter(|x| x.conflicts.is_empty()) {
        if session(ed, x.id).moving.is_none() && !waits(ed, x.id) {
            panic!("{} neither waits nor moves after the card's y", x.name);
        }
        w.owed.retain(|(id, _)| *id != x.id);
        w.owed.push((x.id, newest.clone()));
    }
    format!("y on the card (main at {})", &to[..7])
}

/// fix's change to `a.txt`.
const FIXS: &str = "fix's own\n";

fn held(ed: &Editor, w: &World) -> Result<(), String> {
    let cards: Vec<&crate::offer::Offer> =
        ed.offers.iter().filter(|o| matches!(o.what, What::BaseMoved(_))).collect();
    if cards.len() > 1 {
        return Err(format!("{} base-moved cards", cards.len()));
    }
    if let Some(What::BaseMoved(m)) = cards.first().map(|o| &o.what)
        && !w.tips.contains(&m.to)
    {
        return Err(format!("the card's tip {} was never read", m.to));
    }
    let fix = session(ed, w.fix);
    if fix.moving.is_some() {
        if fix.claude.chat.as_ref().is_some_and(crate::chat::Chat::busy) {
            return Err("fix moves while its Claude is at work".into());
        }
        if fix.task_run.as_ref().is_some_and(|r| r.state == crate::task::State::Running) {
            return Err("fix moves while a run is under way there".into());
        }
    }
    if fix.claude.chat.as_ref().is_some_and(|c| c.queued.is_some() && !c.busy()) && fix.moving.is_none() {
        return Err("a message waits with nothing holding it".into());
    }
    if session(ed, w.up).moving.is_some() && fix.moving.is_some() {
        return Err("two moves at once".into());
    }
    if fix.moving.is_none() {
        let text = std::fs::read_to_string(fix.root.join("a.txt")).unwrap_or_default();
        if text.contains("<<<<<<<") || text.contains(">>>>>>>") {
            return Err(format!("conflict markers in fix's a.txt: {text:?}"));
        }
        if w.changed && text != FIXS {
            return Err(format!("fix's change in no commit is gone: a.txt is {text:?}"));
        }
        if let Some(sha) = crate::aside::kept(&fix.root, crate::aside::MOVE) {
            return Err(format!("fix's changes left set aside at {sha}"));
        }
    }
    // Friction #360: nothing moving, none idle still waits — a turn ended behind (`done`) is idle
    if ed.keep_close.moving.is_none()
        && let Some(x) = ed.keep_close.waits.iter().find(|x| idle(session(ed, x.ws.id)))
    {
        return Err(format!("{} is idle and still waits to move", x.ws.name));
    }
    // … the card says what each of its workspaces waits for, as it is now
    if let Some(What::BaseMoved(m)) = cards.first().map(|o| &o.what) {
        for x in &m.ws {
            let now = crate::keepclose::busy(ed, session(ed, x.id));
            if x.holds != now {
                return Err(format!(
                    "the card says {} waits for {:?}, it waits for {now:?}",
                    x.name, x.holds
                ));
            }
        }
    }
    // … and one a `y` was given for, nothing holding it, is on the tip it's owed
    owed(ed, w)
}

/// Whether `l` is idle by the scenario's own terms: no rebase or move, nothing asked of you there, its Claude between
/// turns (a turn ended behind — `done` — too), no review and no run.
fn idle(l: &Workspace) -> bool {
    l.rebase.is_none()
        && l.moving.is_none()
        && !l.waits()
        && !l.claude.chat.as_ref().is_some_and(crate::chat::Chat::busy)
        && l.claude.review.is_none()
        && l.code_review.is_none()
        && !l.task_run.as_ref().is_some_and(|r| r.state == crate::task::State::Running)
        && l.runs_next.is_empty()
}

/// Each workspace a `y` was given for that neither waits nor moves is on the tip it's owed (fix but with its change).
fn owed(ed: &Editor, w: &World) -> Result<(), String> {
    for (id, tip) in w.owed.iter().filter(|(id, _)| !(w.changed && *id == w.fix)) {
        let l = session(ed, *id);
        let seen = w.on.borrow().iter().any(|(i, t)| i == id && t == tip);
        if seen || l.moving.is_some() || ed.keep_close.waits.iter().any(|x| x.ws.id == *id) {
            continue;
        }
        let on = std::process::Command::new("git")
            .args(["merge-base", "--is-ancestor", tip, "HEAD"])
            .current_dir(&l.root)
            .status()
            .unwrap()
            .success();
        if !on {
            return Err(format!("{} neither waits nor is on {}, the tip its y is owed", l.name, &tip[..7]));
        }
        w.on.borrow_mut().push((*id, tip.clone()));
    }
    Ok(())
}

/// Once still: nothing moving — each workspace a `y` was given for that nothing holds is on the tip it's owed.
fn still(ed: &Editor, w: &World) -> Result<(), String> {
    if ed.keep_close.moving.is_some() {
        return Ok(());
    }
    if let Some(x) =
        ed.keep_close.waits.iter().find(|x| crate::keepclose::busy(ed, session(ed, x.ws.id)).is_none())
    {
        return Err(format!("{} is idle and still waits to move", x.ws.name));
    }
    owed(ed, w)
}

fn print(ed: &Editor) -> String {
    let moving = ed.keep_close.moving.map(|id| format!(" · moving {id}")).unwrap_or_default();
    format!(
        "card {} · waits {} · moved {}{moving} · pending {} · started {}",
        card(ed).map_or("none".into(), |o| format!("#{}", o.id)),
        ed.keep_close.waits.len(),
        ed.keep_close.moved,
        ed.keep_close.said(),
        ed.overlap.started,
    )
}
