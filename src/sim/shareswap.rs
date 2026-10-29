//! The shared server's swap when the new server exits before it takes over (#213), simulated: a repository's main
//! checkout served by a stand-in rust-analyzer (`Client::own` through `Shares.start` — no process), its three
//! worktrees' files opened at random among the steps, each new set of folders swapped in by a new stand-in that
//! loads for `LOAD` and — the first time that set is tried — exits before it takes over, as rust-analyzer panicked
//! loading 10 checkouts; tried again, it loads and takes over. Time passes only by the simulation's wakes, so a
//! checkout joins mid-gather, mid-swap or while a set waits. After every step: a swap for folders whose new server
//! exited never begins before that exit + `RETRY` (the same load isn't tried again at once); any other swap begins
//! within `PROMPT` of the last checkout that joined (folders that changed don't wait for the retry); each exit is
//! said on screen once, as a warning — never an error, never the server marked failed. At the end, an event every
//! `POKE` until it's done: one server serves every checkout, nothing under way.
//!
//! Again with one worktree whose set exits `EXITS` times in a row: each wait is twice the last, up to an hour
//! (10, 20, 40, 60, 60 min) — the next try neither before it nor more than `POKE` after; on screen the first exit
//! only; the waiting file's status line says when it's tried again.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde_json::json;

use super::{Choices, Sim};
use crate::editor::{Editor, ToastKind};
use crate::lsp::{Client, ClientId, ServerSpec};
use crate::share::{Pressure, RETRY, RETRY_MAX, Room};

const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5];

const LIMIT: Duration = crate::test_results::PATIENCE;

/// How long a stand-in takes to load its folders.
const LOAD: Duration = Duration::from_secs(2);

/// A swap that isn't a retry begins within this of the last checkout that joined.
const PROMPT: Duration = Duration::from_secs(30);

const WORKTREES: [&str; 3] = ["wt1", "wt2", "wt3"];

/// How often an event comes once the checkouts are open (`share_sync` runs on every one; a hold has no wake).
const POKE: Duration = Duration::from_secs(5 * 60);

/// Exits in a row for one set in the second scenario: past the hour's cap, and once at it.
const EXITS: usize = 5;

#[test]
fn a_new_server_that_exits_isnt_tried_again_for_the_same_folders() {
    let name = "a_new_server_that_exits_isnt_tried_again_for_the_same_folders";
    super::seeds(name, SEEDS, |choices| run(choices, name, &WORKTREES, 1, false));
}

#[test]
fn the_same_folders_exiting_again_wait_twice_as_long_up_to_an_hour() {
    let name = "the_same_folders_exiting_again_wait_twice_as_long_up_to_an_hour";
    super::seeds(name, SEEDS, |choices| run(choices, name, &WORKTREES[..1], EXITS, true));
}

/// The wait after a set's `k`th exit in a row.
fn wait_after(k: usize) -> Duration {
    match k {
        1 => RETRY,
        2 => 2 * RETRY,
        3 => 4 * RETRY,
        _ => RETRY_MAX,
    }
}

/// What the model keeps, on the driver's thread (the stand-ins' loads land there, the check runs there): how many
/// times each set's new server exits before one loads, the folder sets whose new server exited and when (the
/// simulation's clock and the wall's), when the last checkout joined, the swaps already looked at.
#[derive(Default)]
struct Model {
    exits: usize,
    exited: Vec<(Vec<PathBuf>, Instant, SystemTime)>,
    joined: Option<Instant>,
    seen: Vec<ClientId>,
}

thread_local! {
    static MODEL: RefCell<Model> = RefCell::new(Model::default());
}

fn set(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut v = roots.to_vec();
    v.sort();
    v
}

fn names(roots: &[PathBuf]) -> String {
    let names: Vec<String> =
        roots.iter().filter_map(|r| r.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
    names.join(",")
}

fn plenty() -> Room {
    Room { pressure: Pressure::Normal, headroom: Some(64 << 30) }
}

/// A stand-in server: it answers `initialize`, and after `LOAD` it's loaded (`loaded`).
fn stand_in(ed: &mut Editor, cid: ClientId, spec: &ServerSpec, roots: &[PathBuf]) -> Client {
    let serve: crate::lsp::Serve = Box::new(|method, _| match method {
        "initialize" => Some(json!({ "capabilities": { "positionEncoding": "utf-8",
            "workspace": { "workspaceFolders": { "changeNotifications": true } } } })),
        _ => None,
    });
    let mut c = Client::own(cid, &spec.name, crate::lsp::traits(spec), serve);
    (c.spec, c.root, c.roots) = (spec.clone(), roots[0].clone(), roots.to_vec());
    ed.events.shared_jobs().spawn(move || {
        crate::event::sleep(LOAD);
        move |ed: &mut Editor| loaded(ed, cid)
    });
    c
}

/// Server `cid` has loaded: quiescent — but a swap's new one for folders tried fewer than `Model.exits` times exits
/// before it takes over (rust-analyzer's panic).
fn loaded(ed: &mut Editor, cid: ClientId) {
    let Some(folders) = ed.client(cid).map(|c| set(&c.roots)) else { return };
    let again = MODEL.with(|m| {
        let m = m.borrow();
        m.exited.iter().filter(|e| e.0 == folders).count() < m.exits
    });
    if ed.lsp.share.incoming(cid) && again {
        let now = (crate::clock::now(), crate::clock::system());
        MODEL.with(|m| m.borrow_mut().exited.push((folders, now.0, now.1)));
        return ed.lsp_exited(cid);
    }
    let status = json!({ "jsonrpc": "2.0", "method": "experimental/serverStatus",
        "params": { "health": "ok", "quiescent": true } });
    ed.on_lsp_message(cid, status);
}

/// A repository `main` and its worktrees (`.git` files naming their folder, and its `commondir`, as git makes them
/// — no git run), each a cargo project with `src/main.rs`.
fn fixture(t: &Path) {
    std::fs::create_dir_all(t.join("main/.git")).unwrap();
    for c in std::iter::once(&"main").chain(&WORKTREES) {
        std::fs::create_dir_all(t.join(c).join("src")).unwrap();
        std::fs::write(t.join(c).join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(t.join(c).join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
    }
    for w in WORKTREES {
        let dir = t.join("main/.git/worktrees").join(w);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("commondir"), "../..\n").unwrap();
        std::fs::write(t.join(w).join(".git"), format!("gitdir: {}\n", dir.display())).unwrap();
    }
}

/// The main checkout's file open (`main_first`: its server up before any other opens — else both folders' keys may
/// be read before it starts, one server for both, no swap), then `worktrees`' at random among the steps, each
/// folder set's new server exiting `exits` times before one loads; then an event every `POKE` until one server
/// serves them all.
fn run(
    choices: Choices,
    name: &str,
    worktrees: &[&'static str],
    exits: usize,
    main_first: bool,
) -> Vec<String> {
    MODEL.with(|m| *m.borrow_mut() = Model { exits, ..Model::default() });
    let dir = crate::scratch::Scratch::new("sim-share-swap");
    let t = std::fs::canonicalize(dir.path()).unwrap();
    fixture(&t);
    let (config, _) = crate::config::parse(
        "[lsp.rust-analyzer]\ncommand = \"sh\"\n[lang.rust]\nlsp = [\"rust-analyzer\"]\n",
    );
    let mut sim = Sim::new(name, choices, LIMIT);
    let mut ed = Editor::with_root(config, t.join("main"));
    ed.lsp.share.room = Some(plenty);
    ed.lsp.share.start = Some(stand_in);
    ed.open(&t.join("main/src/main.rs")).unwrap();
    if main_first {
        sim.walk(&mut ed, 0, |_, _| String::new(), held, print);
    }
    let mut opened: Vec<&str> = Vec::new();
    let input = |sim: &mut Sim, ed: &mut Editor| -> String {
        let left: Vec<&str> = worktrees.iter().copied().filter(|w| !opened.contains(w)).collect();
        let w = *sim.choices.pick(&left);
        opened.push(w);
        MODEL.with(|m| m.borrow_mut().joined = Some(crate::clock::now()));
        ed.open(&t.join(w).join("src/main.rs")).unwrap();
        format!("{w}'s file opens")
    };
    sim.walk(&mut ed, worktrees.len(), input, held, print);
    let all = set(&std::iter::once(&"main").chain(worktrees).map(|c| t.join(c)).collect::<Vec<_>>());
    let done = |ed: &Editor| match ed.lsp.clients.as_slice() {
        [c] => set(&c.roots) == all && ed.lsp.share.swaps.is_empty(),
        _ => false,
    };
    // (folders held after an exit: their wait over, the next event tries them again — 5 h at most)
    for _ in 0..60 {
        if done(&ed) {
            break;
        }
        sim.wait(POKE);
        ed.share_sync();
        sim.walk(&mut ed, 0, |_, _| String::new(), held, print);
    }
    sim.at_end(|| {
        if !done(&ed) {
            return Err(format!("not one server for every checkout: {}", print(&ed)));
        }
        if !ed.docs.iter().filter(|d| d.path.is_some()).all(|d| d.lsp.client == Some(ed.lsp.clients[0].id)) {
            return Err("a file without the server".into());
        }
        MODEL.with(|m| {
            let m = m.borrow();
            match m.exited.iter().find(|e| m.exited.iter().filter(|x| x.0 == e.0).count() != exits) {
                Some(e) => Err(format!("{} exited fewer times than {exits} before it loaded", names(&e.0))),
                None if main_first && m.exited.is_empty() => Err("no swap's new server exited".into()),
                None => Ok(()),
            }
        })
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// A swap for folders whose new server exited waits out `wait_after` from its last exit, and begins within `POKE`
/// of it; any other begins within `PROMPT` of the last join; each set's first exit said once, as a warning; a
/// waiting file's status line says when it's tried again.
fn held(ed: &Editor) -> Result<(), String> {
    MODEL.with(|m| {
        let mut m = m.borrow_mut();
        for s in &ed.lsp.share.swaps {
            if m.seen.contains(&s.b) {
                continue;
            }
            m.seen.push(s.b);
            let folders = ed.client(s.b).map(|c| set(&c.roots)).unwrap_or_default();
            let after = |at: Instant| crate::term::run_duration(s.started.saturating_duration_since(at));
            let k = m.exited.iter().filter(|e| e.0 == folders).count();
            match m.exited.iter().rev().find(|e| e.0 == folders).map(|e| e.1) {
                Some(at)
                    if s.started < at + wait_after(k) || s.started > at + wait_after(k) + POKE + PROMPT =>
                {
                    return Err(format!(
                        "{} swapped in again {} after its new server's exit {k} (its wait {})",
                        names(&folders),
                        after(at),
                        crate::term::run_duration(wait_after(k))
                    ));
                }
                Some(_) => {}
                None => {
                    let joined = m.joined.unwrap_or(s.started);
                    if s.started.saturating_duration_since(joined) > PROMPT {
                        return Err(format!(
                            "{} swapped in only {} after the last checkout joined",
                            names(&folders),
                            after(joined)
                        ));
                    }
                }
            }
        }
        let said =
            |kind: ToastKind| ed.messages.iter().filter(|x| x.1 == kind && x.2.contains("exited")).count();
        let mut sets: Vec<&Vec<PathBuf>> = Vec::new();
        for e in &m.exited {
            if !sets.contains(&&e.0) {
                sets.push(&e.0);
            }
        }
        if said(ToastKind::Warning) != sets.len() || said(ToastKind::Error) > 0 {
            return Err(format!(
                "{} exits of {} folder sets, said {} times as a warning, {} as an error",
                m.exited.len(),
                sets.len(),
                said(ToastKind::Warning),
                said(ToastKind::Error)
            ));
        }
        if ed.lsp.failed.contains_key("rust-analyzer") {
            return Err("a swap's new server exiting marked the server failed".into());
        }
        status(ed, &m)
    })
}

/// The file in front, waiting for its server while its folders' wait runs: its status line says when they're tried
/// again — the last exit's wall clock + `wait_after` (`lsp_progress`, as the status line reads it).
fn status(ed: &Editor, m: &Model) -> Result<(), String> {
    let doc = ed.doc();
    let (Some(path), None) = (doc.path.as_deref(), doc.lsp.client) else { return Ok(()) };
    let Some((folders, at, wall)) = m.exited.iter().rev().find(|e| e.0.iter().any(|r| path.starts_with(r)))
    else {
        return Ok(());
    };
    let k = m.exited.iter().filter(|e| e.0 == *folders).count();
    if crate::clock::now() >= *at + wait_after(k) {
        return Ok(());
    }
    let want = format!(
        "rust-analyzer: the last swap exited — again after {}",
        crate::log::clock(*wall + wait_after(k))
    );
    match ed.lsp_progress() {
        Some(s) if s == want => Ok(()),
        s => Err(format!(
            "{}'s file waiting after exit {k}: the status line says {s:?}, not {want:?}",
            names(folders)
        )),
    }
}

fn print(ed: &Editor) -> String {
    let one = |c: &Client| {
        let incoming = if ed.lsp.share.incoming(c.id) { " incoming" } else { "" };
        format!("{}[{}]{incoming}", c.id, names(&c.roots))
    };
    ed.lsp.clients.iter().map(one).collect::<Vec<_>>().join(" ")
}
