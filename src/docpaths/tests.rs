//! Documents by path (`docpaths.rs`), and the work #214 found every event and frame doing with many sessions open:
//! presence's gather (`presence::gather`), a review rail's lookups (`codereview::doc_of`), the workspaces' syncs
//! (`workspaces_sync`, `share_sync`), the bases kept close (`keepclose::sync`) and the notes' buffers
//! (`notes::attach`).

use super::*;
use crate::codereview::{Base, CodeReview, File};
use crate::config::Config;
use crate::replay::{Hunk, Stop};
use crate::scratch::Scratch;
use std::path::PathBuf;

/// Sessions open (yours in front and 16 behind, each a checkout of one repository), the busy ones among those behind,
/// each one's own buffers, its turn's stops (every other one an edit of two hunks, over `TURN_FILES` files, `OPEN` of
/// them open), the review's files (each open) and your notes on files not open — #214's driver: 17 sessions, four
/// Claudes working, a 49-file review.
const SESSIONS: usize = 17;
const BUSY: usize = 4;
const BUFFERS: usize = 15;
const STOPS: usize = 300;
const TURN_FILES: usize = 40;
const OPEN: usize = 10;
const REVIEWED: usize = 49;
const NOTES: usize = 200;

/// A repository and its 16 linked checkouts (`w0` the first): their real paths.
fn checkouts(dir: &Scratch) -> Vec<PathBuf> {
    let git = |at: &std::path::Path, args: &[&str]| crate::gitrepo::fixture_git(at, args);
    let first = dir.join("w0");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::write(first.join("a.rs"), "fn a() {}\n").unwrap();
    git(&first, &["init", "-q"]);
    git(&first, &["add", "-A"]);
    git(&first, &["commit", "-qm", "first"]);
    for k in 1..SESSIONS {
        git(&first, &["worktree", "add", "-q", &format!("../w{k}")]);
    }
    (0..SESSIONS).map(|k| std::fs::canonicalize(dir.join(format!("w{k}"))).unwrap()).collect()
}

/// A buffer of `path` in session `k`'s workspace (0: the one in front).
fn buffer(ed: &mut Editor, k: usize, path: PathBuf) {
    let id = ed.alloc_id();
    let mut d = Document::from_str(id, "fn a() {}\n");
    d.path = Some(path);
    ed.docs.push(d);
    match k {
        0 => ed.workspace_keeps(id),
        k => ed.workspaces[k - 1].docs.push(id),
    }
}

/// A Claude at work in a turn of `STOPS` stops, looking at one of its files.
fn busy() -> crate::chat::Chat {
    let mut c = crate::chat::Chat::new();
    let _ = crate::chat::stand_in(&mut c);
    c.state = crate::chat::State::Thinking;
    let look = |verb| crate::chat::Look {
        verb,
        what: String::new(),
        pattern: false,
        range: String::new(),
        link: None,
    };
    let stops: Vec<Stop> = (0..STOPS)
        .map(|k| {
            let edit = k % 2 == 0;
            let hunks =
                |at: usize| Hunk { lines: at..at + 3, removed: vec!["a".into()], added: vec!["b".into()] };
            Stop {
                call: format!("c{k}"),
                look: look(if edit { "edit" } else { "read" }),
                path: Some(PathBuf::from(format!("src/m{}.rs", k % TURN_FILES))),
                lines: Some(k..k + 3),
                hunks: if edit { vec![hunks(k), hunks(k + 10)] } else { Vec::new() },
                why: String::new(),
                page: None,
            }
        })
        .collect();
    c.follow.replay.begin();
    for s in stops {
        c.follow.replay.add(s);
    }
    c.follow.gaze =
        Some(crate::follow::Gaze::new(look("read"), Some(PathBuf::from("src/m1.rs")), Some(4..9)));
    c
}

/// #214's scene: the sessions, their buffers, the busy ones' turns, the review in front with its files open and marked
/// viewed (what's changed since is asked of each), its rail drawn, your notes, one shared server.
fn many(name: &str) -> (Editor, Scratch) {
    let dir = Scratch::new(name);
    let roots = checkouts(&dir);
    let root = |k: usize| roots[k].clone();
    let mut ed = Editor::new(Config::default());
    ed.workspace.root = root(0);
    ed.workspace.name = "w0".into();
    // (each checkout's git watch as tarae starts them — the front's at the start, the others' as they're made — their
    // first rounds reading each one's repository (`git_common`), applied in it behind; the rest of tarae's git off)
    ed.git_auto = true;
    crate::git::watch(ed.events.sender(), root(0));
    for k in 1..SESSIONS {
        let id = ed.workspace_add(root(k));
        ed.workspace_by_id(id).unwrap().name = format!("w{k}");
    }
    ed.git_auto = false;
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    while !std::iter::once(&ed.workspace).chain(&ed.workspaces).all(|l| l.git_common.is_some()) {
        let left = end.saturating_duration_since(std::time::Instant::now());
        let ev = ed.events.recv_timeout(left).expect("every checkout's repository read by its git watch");
        ed.handle_event(ev);
    }
    for k in 1..SESSIONS {
        for j in 0..BUFFERS {
            buffer(&mut ed, k, root(k).join(format!("lib/b{j}.rs")));
        }
    }
    for k in 1..=BUSY {
        ed.workspaces[k - 1].claude.chat = Some(busy());
        for j in 0..OPEN {
            buffer(&mut ed, k, root(k).join(format!("src/m{j}.rs")));
        }
    }
    let files: Vec<File> = (0..REVIEWED)
        .map(|i| {
            let rel = format!("src/r{}/f{i}.rs", i / 8);
            File {
                path: root(0).join(&rel),
                rel,
                added: 20,
                removed: 4,
                new: false,
                gone: false,
                binary: false,
            }
        })
        .collect();
    for f in &files {
        buffer(&mut ed, 0, f.path.clone());
        ed.workspace.viewed.insert(f.path.clone(), crate::codereview::Viewed::stand_in());
    }
    ed.workspace.code_review = Some(CodeReview::for_test(Base::Head, root(0), files));
    // (in front, your own copy of a file the busy ones write in theirs — their places there asked of it, `ask`)
    let watched = root(0).join("src/m3.rs");
    std::fs::create_dir_all(watched.parent().unwrap()).unwrap();
    std::fs::write(&watched, "fn a() {}\n").unwrap();
    ed.open(&watched).unwrap();
    // (one rust-analyzer for every checkout, every buffer its — `share.rs`: a stand-in, no process)
    static TRAITS: std::sync::LazyLock<crate::lsp::ServerTraits> =
        std::sync::LazyLock::new(crate::lsp::ServerTraits::default);
    let cid = ed.lsp_own("rust-analyzer", &TRAITS, Box::new(|_, _| None));
    let c = ed.lsp.clients.iter_mut().find(|c| c.id == cid).unwrap();
    // (the checkouts in the order they joined: yours last, the idle ones first — each asked whether it rests)
    (c.root, c.roots) = (root(SESSIONS - 1), (0..SESSIONS).rev().map(root).collect());
    c.share = Some(crate::share::Key { repo: "/nowhere/.git".into(), toolchain: String::new() });
    for d in ed.docs.iter_mut().filter(|d| d.path.is_some()) {
        d.lsp.attach(cid, false, true);
    }
    // (each checkout's repository kept resolved, as the driver's were: a resolve within `RACY` of its files' making
    // isn't kept, so resolved again until it is)
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    while !roots.iter().all(|r| crate::gitrepo::cached(r).is_some()) {
        assert!(std::time::Instant::now() < end, "the checkouts' repositories never kept");
        crate::event::sleep(std::time::Duration::from_millis(50));
        for r in &roots {
            crate::gitrepo::repo(r).unwrap();
        }
    }
    ed.workspace.git_branch = Some("main".into());
    // (your notes: half of them drafts on the review's files — its rail counts them —, half on files not open)
    for j in 0..NOTES {
        let draft = j % 2 == 0;
        let path = match draft {
            true => root(0).join(format!("src/r{}/f{}.rs", (j / 2 % REVIEWED) / 8, j / 2 % REVIEWED)),
            false => root(0).join(format!("docs/n{}.md", j % 40)),
        };
        crate::notes::add(&mut ed, &path, 0..1, crate::notes::By::You, "a note");
        ed.workspace.claude.notes.list.last_mut().unwrap().thread[0].draft = draft;
    }
    (ed, dir)
}

const W: u16 = 180;
const H: u16 = 50;

/// An event (a key: every per-event sync runs) and a frame of the scene: the paths compared, hashed or joined by
/// each (`docpaths::work`).
fn counted(ed: &mut Editor, buf: &mut Vec<u8>, key: &str) -> (usize, usize) {
    let w0 = work();
    ed.handle_event(crate::event::Event::Key(key.parse().unwrap()));
    let w1 = work();
    buf.clear();
    crate::term::render(ed, buf, W, H).unwrap();
    (w1 - w0, work() - w1)
}

/// Budget of work: with #214's sessions, shared server, review and notes, an event touches what changed — presence a
/// few paths a buffer and a file of each busy turn (its places made only when the turn or a file moved), the
/// workspaces' syncs and the bases a few a session, the notes a few a buffer and a note — never each place, folder or
/// note against every buffer, stop or session; a frame's rail, its marks and the state's review a few a buffer, a file
/// and a place, never each file against every buffer or note. Measured 2026-10-29 (paths compared, hashed, joined or
/// looked at on disk): an event 1,160 (presence 666, the workspaces' syncs 64, the notes 430), a frame 1,970, the state
/// 688, the bases 16 — and each fix taken out alone (`.sb/f214-traps.sh`): presence 27,684 (no lookup by path) and
/// 2,466 (made anew every event), the workspaces' syncs 2,261 and 2,433 (as at e5317216), the notes 35,030, a frame
/// 162,366 (no lookup by path) and 8,370 (the rail's notes walked a row), the state 57,366, the bases 254.
#[test]
fn many_sessions_compare_paths_once_a_place() {
    let (mut ed, _dir) = many("docpaths-budget");
    let mut buf = Vec::with_capacity(1 << 16);
    counted(&mut ed, &mut buf, "j");
    let (event, frame) = counted(&mut ed, &mut buf, "k");
    let of = |ed: &mut Editor, f: fn(&mut Editor)| {
        let w = work();
        f(ed);
        work() - w
    };
    let presence = of(&mut ed, crate::presence::sync);
    let sessions = of(&mut ed, Editor::workspaces_sync);
    let state = of(&mut ed, |ed| drop(std::hint::black_box(ed.ui_state())));
    let notes = of(&mut ed, crate::notes::attach);
    // (the bases' watch is off in tests: what its every-event pass asks, asked)
    let bases = of(&mut ed, |ed| assert_eq!(crate::keepclose::unwatched(ed).len(), 1, "main, once"));
    eprintln!(
        "WORK event {event} frame {frame} — presence {presence}, workspaces {sessions}, state {state}, notes \
         {notes}, bases {bases}"
    );
    assert!(
        ed.presence.now.len() > BUSY * STOPS,
        "the busy sessions' places gathered: {}",
        ed.presence.now.len()
    );
    assert!(buf.windows(6).any(|w| w == b"review"), "the rail drawn");
    let docs = ed.docs.len();
    let files = BUSY * TURN_FILES;
    assert!(
        presence <= 2 * (docs + files),
        "presence: {presence} paths for {docs} buffers, {files} turn files"
    );
    assert!(sessions <= 8 * SESSIONS, "the workspaces' syncs: {sessions} paths for {SESSIONS} sessions");
    assert!(event <= 2 * (docs + files + NOTES + 4 * SESSIONS), "an event: {event} paths");
    assert!(frame <= 4 * (docs + 8 * REVIEWED), "a frame: {frame} paths for {docs} buffers");
    assert!(state <= 4 * (docs + 4 * REVIEWED), "the state: {state} paths for {docs} buffers");
    assert!(notes <= 4 * (docs + NOTES), "the notes: {notes} paths for {docs} buffers");
    assert!(bases <= 2 * SESSIONS, "the bases: {bases} paths for {SESSIONS} sessions");
}

/// What presence keeps from event to event (`presence::Built`) is what gathering it anew gives, after each change
/// that can move it: nothing, an edit landing in a turn, a stop's hunks changed in place, a file the turn wrote opened,
/// an open one edited, a session renamed, a turn ended — and an event that changes nothing makes nothing again.
#[test]
fn presence_kept_is_presence_gathered_anew() {
    let (mut ed, _dir) = many("docpaths-kept");
    let check = |ed: &mut Editor, what: &str| {
        crate::presence::sync(ed);
        assert_eq!(ed.presence.now, crate::presence::gathered_anew(ed), "{what}");
    };
    check(&mut ed, "first");
    let before = ed.presence.now.len();
    let w = work();
    crate::presence::sync(&mut ed);
    let still = work() - w;
    check(&mut ed, "nothing changed");
    assert_eq!(ed.presence.now.len(), before);
    let root1 = ed.workspaces[0].root.clone();
    fn replay(ed: &mut Editor, k: usize) -> &mut crate::replay::Replay {
        &mut ed.workspaces[k].claude.chat.as_mut().unwrap().follow.replay
    }
    let hunk = Hunk { lines: 3..5, removed: Vec::new(), added: vec!["x".into()] };
    let look = crate::chat::Look {
        verb: "edit",
        what: String::new(),
        pattern: false,
        range: String::new(),
        link: None,
    };
    replay(&mut ed, 0).add(Stop {
        call: "new".into(),
        look,
        path: Some("src/new.rs".into()),
        lines: Some(3..5),
        hunks: vec![hunk],
        why: String::new(),
        page: None,
    });
    check(&mut ed, "an edit landed");
    assert_eq!(ed.presence.now.len(), before + 1);
    replay(&mut ed, 1).by_call("c0").unwrap().hunks.clear();
    check(&mut ed, "a stop's hunks changed in place");
    buffer(&mut ed, 1, root1.join("src/m20.rs"));
    check(&mut ed, "a file the turn wrote opened");
    let open = ed.docs.iter_mut().find(|d| d.path.as_deref() == Some(&root1.join("src/m1.rs"))).unwrap();
    open.apply(&crate::transaction::Transaction::new(vec![crate::transaction::Change::insert(0, "// x\n")]));
    check(&mut ed, "an open file edited");
    ed.workspaces[2].name = "renamed".into();
    check(&mut ed, "a session renamed");
    assert!(ed.presence.now.iter().any(|p| p.name == "renamed"));
    ed.workspaces[3].claude.chat.as_mut().unwrap().state = crate::chat::State::Idle;
    check(&mut ed, "a turn ended");
    assert!(!ed.presence.now.iter().any(|p| p.session == ed.workspaces[3].id));
    // (nothing new: each busy session's turn files looked up, nothing made — a few paths a file, never a stop's)
    assert!(still <= 4 * (ed.docs.len() + BUSY * TURN_FILES), "an event of nothing new: {still} paths");
}

/// A lookup of a turn's stops that finds nothing keeps what presence made of them (review round 1: `by_call` and a page
/// read gave the stops a new version whatever they found, and each busy session's presence was made anew): a call no
/// stop has, a page read none waits for, an edit landing in a file no stop is in.
#[test]
fn a_lookup_finding_nothing_keeps_presence() {
    let (mut ed, _dir) = many("docpaths-nothing");
    // (a busy Claude in front too: a page read lands in the front session's follow)
    ed.workspace.claude.chat = Some(busy());
    crate::presence::sync(&mut ed);
    let kept = ed.presence.kept();
    assert_eq!(kept.len(), BUSY + 1, "every busy session's presence kept");
    let w = work();
    for k in 0..BUSY {
        let r = &mut ed.workspaces[k].claude.chat.as_mut().unwrap().follow.replay;
        assert!(r.by_call("no-such-call").is_none());
        r.edited(std::path::Path::new("src/never.rs"), &[(0..1, 0..2)]);
    }
    crate::follow::page_read(
        &mut ed,
        "https://example.invalid/",
        "https://example.invalid/",
        crate::clock::now(),
        None,
    );
    crate::presence::sync(&mut ed);
    let still = work() - w;
    assert_eq!(ed.presence.kept(), kept, "presence kept, nothing made again");
    assert!(still <= 4 * (ed.docs.len() + (BUSY + 1) * TURN_FILES), "nothing made: {still} paths");
}

/// Every search of the documents by path goes through `docpaths` (`find`, `Index`) — and what's left of them only shrinks
/// (review round 1: the work budget counts only what reports itself, so a plain search added anywhere counted as nothing):
/// a file's count is its searches over a `docs` that compare a path (`searches`), in what a non-test build compiles. One
/// added fails here; one turned into a lookup fails too until its count goes down (the message prints the table now).
#[test]
fn every_search_of_the_documents_by_path_is_counted() {
    let now = crate::sim::lint::counts_by(&["docpaths.rs"], &searches);
    crate::sim::lint::only_shrinks("searches of the documents by path", &now, SEARCHES);
}

/// What `searches` counts and leaves: a find, a position, an any, a for over them with a path compared in its body, a
/// match on the path (`codereview::doc_of`'s gone branch), a predicate named before it — not a lookup, nor a walk that
/// compares no path.
#[test]
fn a_search_of_the_documents_by_path_counts() {
    let counted = [
        "let d = ed.docs.iter().find(|d| d.path.as_deref() == Some(p));",
        "let i = self.docs.iter().position(|d| d.path.as_ref() == Some(&path))?;",
        "if ed.docs().iter().any(|d| d.path.as_deref().is_some_and(|q| q.starts_with(dir))) {}",
        "for d in &mut self.docs { if d.path.as_deref() == Some(p) { d.dirty = true; } }",
        "ed.docs.iter_mut()\n    .filter(|d| same_file(d.path.as_deref(), p))\n    .for_each(|d| d.x = 1);",
        "ed.docs.iter().find(|d| match &d.path { Some(p) => *p == f.path, None => false })",
        "let of_it = |d: &&Document| d.path.as_ref() == Some(p);\nlet Some(d) = ed.docs.iter().find(of_it) else { return };",
    ];
    for c in counted {
        assert_eq!(searches(c), 1, "{c}");
    }
    let left = [
        "let d = crate::docpaths::find(ed, &path);",
        "let n = ed.docs.iter().filter(|d| d.modified()).count();",
        "let paths: Vec<_> = ed.docs.iter().filter_map(|d| d.path.clone()).collect(); let same = a == b;",
        "let ids = l.docs.iter().copied().collect::<Vec<_>>(); if path == other {}",
        "let codocs = x.iter().find(|d| d.path == p);",
        "let open = |d: &&Document| d.id == id;\nlet d = ed.docs.iter().find(open); let x = a.path == b;",
    ];
    for c in left {
        assert_eq!(searches(c), 0, "{c}");
    }
}

/// The searches of the documents by path in `code` (comments out, literals emptied): each walk of a `docs` —
/// `docs.iter()`, `.iter_mut()`, `.into_iter()` (`docs()` too), `for … in &…docs` — whose statement (a `for`'s body
/// with it, a predicate it names by its `let`) compares a path: `.path` there, and `==`, `!=`, a `same…` or a
/// `starts_with`.
fn searches(code: &str) -> usize {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut n = 0;
    for (at, _) in code.match_indices("docs") {
        let before = code[..at].chars().next_back();
        let rest = &code[at + 4..];
        if before.is_some_and(ident) || rest.starts_with(ident) {
            continue;
        }
        let rest = rest.strip_prefix("()").unwrap_or(rest);
        let walk = rest.trim_start();
        let iterates = [".iter()", ".iter_mut()", ".into_iter()"].iter().any(|w| walk.starts_with(w));
        // (`for d in &ed.docs {`: the statement it's in a `for`'s head)
        let head = &code[code[..at].rfind(['\n', ';', '{', '}']).map_or(0, |i| i + 1)..at];
        let looped = walk.starts_with('{') && head.trim_start().starts_with("for ") && head.contains(" in ");
        if !iterates && !looped {
            continue;
        }
        let mut span = statement(&code[at..]).to_string();
        // (a predicate named before it — `.find(of_review)`: its `let` read with it)
        for (i, _) in span.clone().match_indices('(') {
            let arg = &span[i + 1..];
            let Some(end) = arg.find(')') else { continue };
            let name = &arg[..end];
            if name.is_empty() || !name.chars().all(ident) {
                continue;
            }
            let made = [format!("let {name} ="), format!("let {name}:")];
            if let Some(at) = made.iter().filter_map(|m| code[..at].rfind(m.as_str())).max() {
                span.push_str(statement(&code[at..]));
            }
        }
        let path = span.match_indices(".path").any(|(i, _)| !span[i + 5..].starts_with(ident));
        let compared = ["==", "!=", "same", "starts_with"].iter().any(|c| span.contains(c));
        n += usize::from(path && compared);
    }
    n
}

/// `code`'s statement from its start: to a `;` or `,` at its own depth, a bracket closing past it, or the end of a
/// block it opened (a `for`'s body).
fn statement(code: &str) -> &str {
    let mut depth = 0i32;
    let mut block = false;
    for (i, c) in code.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            '{' => {
                block |= depth == 0;
                depth += 1;
            }
            ')' | ']' | '}' => {
                depth -= 1;
                if depth < 0 || (c == '}' && depth == 0 && block) {
                    return &code[..i];
                }
            }
            ';' | ',' if depth == 0 => return &code[..i],
            _ => {}
        }
    }
    code
}

/// Searches of the documents by path left, per file (paths under `src/`, a crate's from the repository's root) —
/// `codereview.rs`'s with `doc_of`'s gone file, found by its stand-in's title.
const SEARCHES: &[(&str, usize)] = &[
    ("chat.rs", 1),
    ("checkpoint.rs", 3),
    ("codereview.rs", 4),
    ("disk.rs", 1),
    ("editor.rs", 1),
    ("follow.rs", 2),
    ("git.rs", 2),
    ("gitpanel.rs", 4),
    ("instructions.rs", 2),
    ("keepclose.rs", 3),
    ("layouts.rs", 2),
    ("lsp_editor.rs", 14),
    ("motion.rs", 2),
    ("nav.rs", 4),
    ("notes.rs", 4),
    ("offer.rs", 1),
    ("procs.rs", 1),
    ("queued.rs", 1),
    ("quickfix.rs", 1),
    ("reads.rs", 4),
    ("rebase.rs", 3),
    ("recovery.rs", 2),
    ("refactor.rs", 4),
    ("restart.rs", 1),
    ("reviewer.rs", 3),
    ("share.rs", 5),
    ("sidebar.rs", 4),
    ("sound.rs", 1),
    ("term.rs", 1),
    ("tools.rs", 1),
    ("uistate.rs", 1),
    ("weave.rs", 3),
    ("workspace.rs", 1),
    ("worktree.rs", 2),
    ("writing.rs", 3),
];

/// #214's scene timed: an event (a key, every per-event sync), a frame and a side panel's file row (`presence::
/// file_marks`), medians — said for `cargo perf` (2026-10-29, this Mac: an event 230 µs, a frame 375 µs, a row 110 µs;
/// round 1's code on this scene ~500 and ~550 µs; the first, on a smaller one at e5317216, 15.7 ms and 3.0 ms).
#[test]
fn perf_many_sessions_busy() {
    if crate::test_results::unoptimized("docpaths::tests::perf_many_sessions_busy") {
        return;
    }
    let (mut ed, _dir) = many("docpaths-perf");
    let mut buf = Vec::with_capacity(1 << 16);
    let (mut events, mut frames) = (Vec::new(), Vec::new());
    for i in 0..200 {
        let t = std::time::Instant::now();
        ed.handle_event(crate::event::Event::Key((if i % 2 == 0 { "j" } else { "k" }).parse().unwrap()));
        events.push(t.elapsed());
        buf.clear();
        let t = std::time::Instant::now();
        crate::term::render(&mut ed, &mut buf, W, H).unwrap();
        frames.push(t.elapsed());
    }
    // (the side panel's file rows each ask presence who is in them — `presence::file_marks`: a row of the turns' files)
    let rels: Vec<PathBuf> = (0..TURN_FILES).map(|j| PathBuf::from(format!("src/m{j}.rs"))).collect();
    let rows: Vec<std::time::Duration> = (0..20)
        .map(|_| {
            let t = std::time::Instant::now();
            for r in &rels {
                std::hint::black_box(crate::presence::file_marks(&ed, r));
            }
            t.elapsed() / TURN_FILES as u32
        })
        .collect();
    let all = [("many_sessions_event_us", events), ("many_sessions_frame_us", frames)];
    for (name, mut all) in all.into_iter().chain([("many_sessions_file_row_us", rows)]) {
        all.sort_unstable();
        let us = all[all.len() / 2].as_secs_f64() * 1e6;
        let line = format!("PERF {name} {us:.1} budget=16000 noise=30\n");
        let _ = std::io::Write::write_all(&mut std::io::stderr(), line.as_bytes());
    }
}
