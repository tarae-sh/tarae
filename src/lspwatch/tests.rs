//! The LSP side of the tree watch: its rules on their own (globs, bases, registrations, the split), the gate a
//! server's writer thread waits on, and the editor with a fake tree watch (batches handed in, looks recorded) and a
//! stand-in server (what it's sent, recorded).

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::lsp::{Client, ServerSpec};
use crate::test_results::settle_until;

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

/// A server on `/r` (given and real alike), leaving out what the tree leaves out and `extra`.
fn server(extra: &[&str]) -> Server {
    let mut left_out = tree_left_out(&[p("/r")]);
    left_out.extend(extra.iter().map(|e| p(e)));
    Server::new(vec![(p("/r"), p("/r"))], left_out, vec![])
}

fn relative(base: &str, pattern: &str) -> Value {
    json!({ "globPattern": { "baseUri": crate::lsp::uri(&p(base)), "pattern": pattern } })
}

fn plain(pattern: &str) -> Value {
    json!({ "globPattern": pattern })
}

fn watchers(list: &[Value]) -> Value {
    json!({ "watchers": list })
}

fn table(servers: Vec<(ClientId, Server)>) -> Table {
    Table { servers: servers.into_iter().collect() }
}

/// Each server's changes for `batch`, as (server, path, LSP type).
fn split(t: &Table, batch: &[(&str, Kind)]) -> Vec<(ClientId, String, u64)> {
    let batch: Batch = batch.iter().map(|(s, k)| (p(s), *k)).collect();
    let mut out = Vec::new();
    for (cid, changes) in t.split(&batch) {
        for c in changes {
            let path = crate::lsp::path_from_uri(c["uri"].as_str().unwrap()).unwrap();
            out.push((cid, path.display().to_string(), c["type"].as_u64().unwrap()));
        }
    }
    out
}

/// LSP's glob grammar: `*` and `?` stay in one segment, `**` crosses them, braces and classes, a plain absolute
/// path a pattern like any other.
#[test]
fn globs_follow_the_lsp_grammar() {
    let rows: &[(&str, &str, bool)] = &[
        ("**/*.rs", "a.rs", true),
        ("**/*.rs", "src/deep/a.rs", true),
        ("**/*.rs", "/r/src/a.rs", true),
        ("**/*.rs", "src/a.rsx", false),
        ("**/Cargo.{lock,toml}", "Cargo.lock", true),
        ("**/Cargo.{lock,toml}", "crates/x/Cargo.toml", true),
        ("**/Cargo.{lock,toml}", "Cargo.json", false),
        ("*.rs", "a.rs", true),
        ("*.rs", "src/a.rs", false),
        ("src/*.rs", "src/x/a.rs", false),
        ("?.rs", "a.rs", true),
        ("?.rs", "/.rs", false),
        ("[!a]*.rs", "b.rs", true),
        ("[!a]*.rs", "a.rs", false),
        ("[a-c].rs", "b.rs", true),
        ("[a-c].rs", "d.rs", false),
        ("/r/Cargo.toml", "/r/Cargo.toml", true),
        ("/r/Cargo.toml", "/r/x/Cargo.toml", false),
    ];
    for (pattern, path, want) in rows {
        assert_eq!(glob(pattern).unwrap().is_match(path), *want, "{pattern} on {path}");
    }
}

/// Is this base served? Inside a root: by its watch. Below a folder left out: a root of its own. Outside: below
/// one of the project's output folders (the user's answer, 2026-11-11, for now); else refused — and refused when
/// the project names no output folder.
#[test]
fn is_this_base_served() {
    let roots = [p("/r")];
    let left_out = [p("/r/target"), p("/r/.git"), p("/r/wt")];
    let outputs = [p("/home/u/.cargo/build/2d")];
    let rows: &[(&str, &[PathBuf], Base)] = &[
        ("/r", &outputs, Base::InRoot),
        ("/r/crates/x", &outputs, Base::InRoot),
        ("/r/target/debug/build/x-1/out", &outputs, Base::OwnRoot),
        ("/r/target", &outputs, Base::OwnRoot),
        ("/r/wt/src", &outputs, Base::OwnRoot),
        ("/home/u/.cargo/build/2d/debug/build/x-1/out", &outputs, Base::OwnRoot),
        ("/home/u/.cargo/build/2d/debug/build/x-1/out", &[], Base::Refused),
        ("/home/u/Library/Application Support/x", &outputs, Base::Refused),
        ("/home/u/.cargo/build/3e", &outputs, Base::Refused),
        ("/rr", &outputs, Base::Refused),
    ];
    for (base, outputs, want) in rows {
        assert_eq!(served(&p(base), &roots, &left_out, outputs), *want, "{base} ({} outputs)", outputs.len());
    }
    // An output folder that can't be one isn't counted: `/`, home or above it, one holding the server's folder
    let home = crate::allow::home().unwrap();
    let above_home = home.parent().unwrap().to_path_buf();
    let implausible: &[(PathBuf, PathBuf)] = &[
        (p("/"), p("/home/u/.cargo/build/2d/out")),
        (home.clone(), home.join("Library/Application Support/x")),
        (above_home.clone(), above_home.join("other/x")),
        (p("/w"), p("/w/build/out")),
    ];
    for (output, base) in implausible {
        let roots = [p("/w/r"), p("/r")];
        let got = served(base, &roots, &left_out, std::slice::from_ref(output));
        assert_eq!(got, Base::Refused, "{} as an output folder", output.display());
    }
    let below_home = home.join(".cargo/build/2d");
    let got = served(&below_home.join("debug/out"), &roots, &left_out, std::slice::from_ref(&below_home));
    assert_eq!(got, Base::OwnRoot, "a folder below home is one");
}

/// The user's answer: a base outside the server's folders below one of its project's output folders (a shared
/// `build-dir`'s build-script output, as `[builds]` prints it) is watched as a root of its own — read in its real
/// form, told in the printed one; the rest of that folder isn't heard; any other outside base stays refused.
#[test]
fn a_base_below_an_output_folder_outside_is_watched() {
    let outputs = vec![(p("/b/2d"), p("/real/b/2d"))];
    let mut s = Server::new(vec![(p("/r"), p("/r"))], tree_left_out(&[p("/r")]), outputs);
    let out = "/b/2d/debug/build/x-1/out";
    let options =
        watchers(&[relative(out, "**/*.rs"), relative("/home/u/Library/Application Support/x", "*")]);
    let read = s.register("w", &options);
    assert_eq!(read.refused, ["/home/u/Library/Application Support/x"]);
    let t = table(vec![(1, s)]);
    assert_eq!(t.own_roots(), [p("/real/b/2d/debug/build/x-1/out")], "the tree is asked for it, real");
    let got = split(
        &t,
        &[
            ("/real/b/2d/debug/build/x-1/out/gen.rs", Kind::Changed),
            ("/real/b/2d/debug/other.rs", Kind::Changed),
        ],
    );
    assert_eq!(got, [(1, "/b/2d/debug/build/x-1/out/gen.rs".into(), 2)]);
}

/// Each root's output folders are asked once, whichever server asks (a process each time otherwise).
#[test]
fn each_roots_output_folders_are_asked_once() {
    let count = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&count);
    let ask: Outputs = Arc::new(move |r: &Path| {
        lock(&seen).push(r.to_path_buf());
        vec![(r.join("target"), r.join("target"))]
    });
    let asked: Asked = Default::default();
    let first = outputs_of(&ask, &asked, &[p("/a"), p("/b")]);
    let again = outputs_of(&ask, &asked, &[p("/b"), p("/a")]);
    assert_eq!(*lock(&count), [p("/a"), p("/b")]);
    assert_eq!(first.len(), 2);
    assert_eq!(again, [(p("/b/target"), p("/b/target")), (p("/a/target"), p("/a/target"))]);
}

/// A relative pattern's base by a URI and by a workspace folder object; a plain pattern below the server's
/// folders only.
#[test]
fn a_relative_pattern_by_uri_and_by_folder_object() {
    let mut s = server(&[]);
    let folder = json!({ "globPattern": { "baseUri": { "uri": crate::lsp::uri(&p("/r/b")), "name": "b" },
        "pattern": "*.toml" } });
    let read = s.register("1", &watchers(&[relative("/r/a", "**/*.rs"), folder, plain("**/*.md")]));
    assert_eq!(read, Read::default());
    let t = table(vec![(1, s)]);
    let got = split(
        &t,
        &[
            ("/r/a/x/m.rs", Kind::Changed),
            ("/r/c/m.rs", Kind::Changed),
            ("/r/b/c.toml", Kind::Changed),
            ("/r/b/x/c.toml", Kind::Changed),
            ("/r/d/r.md", Kind::Changed),
            ("/elsewhere/r.md", Kind::Changed),
        ],
    );
    let paths: Vec<&str> = got.iter().map(|g| g.1.as_str()).collect();
    assert_eq!(paths, ["/r/a/x/m.rs", "/r/b/c.toml", "/r/d/r.md"]);
}

/// The same id again replaces (rust-analyzer's second registration, no unregister between); unregistered, it
/// stops; a new id then the old one's unregister (pyright's) holds both between, a path once a notification.
#[test]
fn registrations_by_id() {
    let mut s = server(&[]);
    s.register("w", &watchers(&[relative("/r", "**/*.rs")]));
    s.register("w", &watchers(&[relative("/r", "**/*.toml")]));
    let t = table(vec![(1, s.clone())]);
    let batch = [("/r/a.rs", Kind::Changed), ("/r/Cargo.toml", Kind::Changed)];
    assert_eq!(split(&t, &batch), [(1, "/r/Cargo.toml".into(), 2)], "the same id again replaces");
    s.unregister("w");
    assert_eq!(split(&table(vec![(1, s.clone())]), &batch), [], "unregistered");

    s.register("old", &watchers(&[relative("/r", "**/*.rs")]));
    s.register("new", &watchers(&[relative("/r", "**/*.rs"), plain("**/*.toml")]));
    let both = split(&table(vec![(1, s.clone())]), &batch);
    assert_eq!(both, [(1, "/r/a.rs".into(), 2), (1, "/r/Cargo.toml".into(), 2)], "a path once");
    s.unregister("old");
    assert_eq!(split(&table(vec![(1, s)]), &batch), both, "the new one holds it");
}

/// The kind mask: a watcher for creates only hears no change nor delete; absent means all three.
#[test]
fn the_kind_mask() {
    let mut s = server(&[]);
    let creates = json!({ "globPattern": "**/*.rs", "kind": 1 });
    s.register("1", &watchers(&[creates]));
    let t = table(vec![(1, s)]);
    let batch = [("/r/a.rs", Kind::Created), ("/r/b.rs", Kind::Changed), ("/r/c.rs", Kind::Deleted)];
    assert_eq!(split(&t, &batch), [(1, "/r/a.rs".into(), 1)]);
    let mut s = server(&[]);
    s.register("1", &watchers(&[plain("**/*.rs")]));
    let types: Vec<u64> = split(&table(vec![(1, s)]), &batch).iter().map(|g| g.2).collect();
    assert_eq!(types, [1, 2, 3], "created, changed, deleted as LSP numbers them");
}

/// Two servers, one batch: each hears its own. A nested checkout one leaves out and the other serves as its root
/// goes to the second only; what the tree leaves out goes to nobody.
#[test]
fn two_servers_one_batch_and_a_nested_checkout() {
    let mut outer = server(&["/r/wt"]);
    outer.register("1", &watchers(&[relative("/r", "**/*.rs")]));
    let mut inner = Server::new(vec![(p("/r/wt"), p("/r/wt"))], tree_left_out(&[p("/r/wt")]), vec![]);
    inner.register("1", &watchers(&[relative("/r/wt", "**/*.rs")]));
    let t = table(vec![(1, outer), (2, inner)]);
    let got = split(
        &t,
        &[("/r/a.rs", Kind::Changed), ("/r/wt/b.rs", Kind::Created), ("/r/target/c.rs", Kind::Changed)],
    );
    assert_eq!(got, [(1, "/r/a.rs".into(), 2), (2, "/r/wt/b.rs".into(), 1)]);
    assert!(t.keeps(&p("/r/wt/b.rs")) && !t.keeps(&p("/r/target/c.rs")) && !t.keeps(&p("/r/a.md")));
}

/// A base below a folder left out (a build script's output below `target`) is a root of its own: what's below it
/// is heard, the rest of `target` isn't, and the tree is asked for it as a root.
#[test]
fn a_base_below_a_left_out_folder_is_a_root_of_its_own() {
    let mut s = server(&[]);
    let out = "/r/target/debug/build/x-1/out";
    let read = s.register("w", &watchers(&[relative("/r", "**/*.rs"), relative(out, "**/*.rs")]));
    assert_eq!(read, Read::default());
    let t = table(vec![(1, s)]);
    let got = split(
        &t,
        &[("/r/target/debug/build/x-1/out/gen.rs", Kind::Changed), ("/r/target/debug/y.rs", Kind::Changed)],
    );
    assert_eq!(got, [(1, "/r/target/debug/build/x-1/out/gen.rs".into(), 2)]);
    assert_eq!(t.own_roots(), [p(out)]);
    assert_eq!(
        minimal(vec![p("/r"), p(out), p("/r/src")], &lists(&[("/r", &[])])),
        [p("/r"), p(out)],
        "below target: not covered by /r"
    );
}

/// Each root's whole list: what the tree leaves out at it (`tree_left_out`) and `more`.
fn lists(roots: &[(&str, &[&str])]) -> BTreeMap<PathBuf, Vec<PathBuf>> {
    (roots.iter())
        .map(|(r, more)| {
            let mut list = tree_left_out(&[p(r)]);
            list.extend(more.iter().map(|m| p(m)));
            (p(r), list)
        })
        .collect()
}

/// `minimal` reads a root's whole list: a server's folder inside a checkout the outer root leaves out (the walk's
/// `.claude/worktrees`, a clone with its own `.git`) isn't covered by it — else dropped, then silenced by the outer
/// stream's exclusion. A folder below nothing left out is.
#[test]
fn minimal_reads_the_whole_list_of_what_is_left_out() {
    let wt = "/r/.claude/worktrees/x";
    let walked = lists(&[("/r", &["/r/.claude/worktrees", "/r/clone"]), (wt, &[])]);
    assert_eq!(
        minimal(vec![p("/r"), p(wt), p("/r/clone/sub"), p("/r/crates/m")], &walked),
        [p("/r"), p(wt), p("/r/clone/sub")],
        "the worktree and the clone's folder are roots of their own; the member is covered"
    );
}

/// A scratch repository: `files` written (a name ending in `/` a folder), `.gitignore` as given, all committed.
fn repository(name: &str, ignore: &str, files: &[&str]) -> (crate::scratch::Scratch, PathBuf) {
    let dir = crate::scratch::Scratch::new(name);
    let t = std::fs::canonicalize(dir.path()).unwrap();
    for f in files {
        let path = t.join(f.trim_end_matches('/'));
        if f.ends_with('/') {
            std::fs::create_dir_all(&path).unwrap();
        } else {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        }
    }
    std::fs::write(t.join(".gitignore"), ignore).unwrap();
    crate::gitrepo::fixture_git(&t, &["init", "-q"]);
    crate::gitrepo::fixture_git(&t, &["add", "-A"]);
    crate::gitrepo::fixture_git(&t, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "t"]);
    (dir, t)
}

/// The walk's list, in its order: the root's `.git` and outputs there or not (no `target` yet: a fresh clone's
/// comes with the first build), `.claude/worktrees` and a clone inside it (every watched server's, its pack's
/// `leave-out` or not), then the output-named folders deeper down that git ignores — a member's own `target` — but
/// not a tracked `src/target` with a kept file in it, nor one git doesn't ignore.
#[test]
fn the_walk_leaves_out_the_roots_own_the_checkouts_inside_and_what_git_ignores() {
    let files = [
        ".claude/worktrees/x/.git/",
        "clone/.git/",
        "crates/m/target/debug/m",
        "crates/n/target/debug/n",
        "src/target/mod.rs",
        "src/main.rs",
    ];
    let (_d, t) = repository("lspwatch-walk", "/target\n/crates/m/target\n", &files);
    let mut want = tree_left_out(std::slice::from_ref(&t));
    want.extend([t.join(".claude/worktrees"), t.join("clone"), t.join("crates/m/target")]);
    assert!(!t.join("target").exists());
    assert_eq!(walk_left_out(&t), want);
}

/// Past `DEEP_MAX` output-named folders, git is asked about the shallowest, then by path; not a repository, none
/// is left out.
#[test]
fn the_walk_asks_git_about_the_first_deep_max_and_none_outside_a_repository() {
    let names: Vec<String> = (0..DEEP_MAX + 6).map(|i| format!("p{i:02}/target/")).collect();
    let mut files: Vec<&str> = names.iter().map(String::as_str).collect();
    files.push("a/b/target/");
    let (_d, t) = repository("lspwatch-walk-cap", "target\n", &files);
    let deep: Vec<PathBuf> = walk_left_out(&t).split_off(tree_left_out(std::slice::from_ref(&t)).len());
    let first: Vec<PathBuf> = (0..DEEP_MAX).map(|i| t.join(format!("p{i:02}/target"))).collect();
    assert_eq!(deep, first, "the shallow ones, by path; a/b/target is deeper");

    let plain = crate::scratch::Scratch::new("lspwatch-walk-plain");
    let o = std::fs::canonicalize(plain.path()).unwrap();
    std::fs::create_dir_all(o.join("crates/m/target")).unwrap();
    assert_eq!(walk_left_out(&o), tree_left_out(std::slice::from_ref(&o)), "no repository: none join");
}

/// The tree's start is given each watched server's whole list: the root's `target` though it isn't there yet, and
/// the checkouts inside it.
#[test]
fn the_tree_is_given_the_whole_list_there_or_not() {
    let fake = Arc::new(Fake::default());
    let (_d, t, mut ed) = editor_with("lspwatch-whole-list", &fake, &[".claude/worktrees/x", "clone/.git"]);
    settle_until(&mut ed, |ed| decided(ed) == Some(true) && ed.lsp.watch.running());
    let given = lock(&fake.left_out).clone();
    assert!(!t.join("target").exists());
    for want in [t.join("target"), t.join(".claude/worktrees"), t.join("clone")] {
        assert!(given.contains(&want), "{} in {given:?}", want.display());
    }
    let w = ed.lsp.clients[0].watched.get().cloned().expect("decided");
    assert!(w.left_out.contains(&t.join("clone")), "the server's own split leaves it out too");
}

/// A base outside the served folders, with no output folder of its project over it, is refused (and said:
/// `Read.refused`); a plain absolute pattern outside them too; a pattern that doesn't read is said apart.
#[test]
fn a_base_outside_is_refused() {
    let mut s = server(&[]);
    let options = watchers(&[
        relative("/home/u/.cargo/build/2d/debug/build/x-1/out", "**/*.rs"),
        plain("/home/u/Library/Application Support/x"),
        plain("/r/Cargo.toml"),
        plain("**/[.rs"),
    ]);
    let read = s.register("w", &options);
    assert_eq!(
        read.refused,
        ["/home/u/.cargo/build/2d/debug/build/x-1/out", "/home/u/Library/Application Support/x"]
    );
    assert_eq!(read.bad, ["**/[.rs"]);
    let t = table(vec![(1, s)]);
    assert_eq!(split(&t, &[("/r/Cargo.toml", Kind::Changed)]), [(1, "/r/Cargo.toml".into(), 2)]);
}

/// A server knows its folders by the paths it was given (`/tmp/x`); the tree names real ones (`/private/tmp/x`):
/// its bases are read in the real form, its changes told in its own.
#[test]
fn paths_are_told_in_the_form_the_server_was_given() {
    let mut s =
        Server::new(vec![(p("/tmp/x"), p("/private/tmp/x"))], tree_left_out(&[p("/private/tmp/x")]), vec![]);
    s.register("w", &watchers(&[relative("/tmp/x/src", "**/*.rs")]));
    // (a plain absolute pattern in the server's form: matched in that form, not refused)
    let read = s.register("p", &watchers(&[plain("/tmp/x/Cargo.toml")]));
    assert_eq!(read, Read::default(), "a plain pattern in the server's form isn't refused");
    let t = table(vec![(1, s)]);
    assert_eq!(split(&t, &[("/private/tmp/x/src/a.rs", Kind::Changed)]), [(1, "/tmp/x/src/a.rs".into(), 2)]);
    assert_eq!(
        split(&t, &[("/private/tmp/x/Cargo.toml", Kind::Changed)]),
        [(1, "/tmp/x/Cargo.toml".into(), 2)]
    );
}

/// A batch of 5,000 paths is split off the main thread: one list a server.
#[test]
fn a_batch_of_5000_is_one_list_a_server() {
    let mut s = server(&[]);
    s.register("w", &watchers(&[plain("**/*.rs")]));
    let t = table(vec![(1, s)]);
    let batch: Batch = (0..5000).map(|i| (p(&format!("/r/src/m{i}.rs")), Kind::Changed)).collect();
    let per = t.split(&batch);
    assert_eq!(per.len(), 1);
    assert_eq!(per[0].1.len(), 5000);
}

// ── The gate ────────────────────────────────────────────────────────────────────────

/// A yes that comes late: the writer thread waits for it (nothing came back before it), and declares.
#[test]
fn the_writer_waits_for_a_late_yes() {
    let dir = crate::scratch::Scratch::new("lspwatch-late-yes");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let gate = Arc::new(Gate::default());
    let (tx, rx) = std::sync::mpsc::channel();
    let waiting = Arc::clone(&gate);
    let roots = vec![root];
    let writer = std::thread::spawn(move || {
        let _ = tx.send(declared(&waiting, &roots, "t", crate::clock::now(), crate::test_results::PATIENCE));
    });
    assert!(rx.try_recv().is_err(), "no answer yet: it waits");
    gate.fill(Ok(()));
    writer.join().unwrap();
    assert_eq!(rx.recv().ok(), Some(true));
}

/// Past the bound or on a no it starts without: a yes after its bound changes nothing for it — even one already
/// there when its writer looks (the writer began late) — only for a server started after it.
#[test]
fn past_the_bound_or_on_a_no_it_watches_by_itself() {
    let dir = crate::scratch::Scratch::new("lspwatch-bound");
    let roots = vec![std::fs::canonicalize(dir.path()).unwrap()];
    let first = crate::clock::now();
    let gate = Gate::default();
    assert!(!declared(&gate, &roots, "t", first, Duration::ZERO), "no answer within the bound");
    gate.fill(Ok(()));
    assert!(!declared(&gate, &roots, "t", first, Duration::ZERO), "the yes came after its bound");
    assert!(
        declared(&gate, &roots, "t", crate::clock::now(), Duration::ZERO),
        "the next server takes the yes"
    );
    let no = Gate::default();
    no.fill(Err("no tree watch here".into()));
    assert!(!declared(&no, &roots, "t", crate::clock::now(), crate::test_results::PATIENCE), "a no");
    let gone = vec![roots[0].join("missing")];
    let yes = Gate::default();
    yes.fill(Ok(()));
    assert!(!declared(&yes, &gone, "t", crate::clock::now(), Duration::ZERO), "a root that isn't watchable");
}

// ── The editor, with a fake tree watch and a stand-in server ───────────────────────────────────

/// A tree watch that's told things and records them: its looks come back as changes of the files it keeps (what
/// the worker's table would decide); `hand` gives a batch as the stream would.
#[derive(Default)]
pub(crate) struct Fake {
    pub calls: Mutex<Vec<String>>,
    pub looks: Mutex<Vec<PathBuf>>,
    out: Mutex<Option<Out>>,
    keep: Mutex<Option<Keep>>,
    /// Its start says no.
    pub no: Option<String>,
    /// Its start takes this long (a simulation's clock).
    pub delay: Duration,
    /// Its next starts that say no, then yes.
    pub fails: std::sync::atomic::AtomicUsize,
    /// What it was last told to leave out (its start's, a set's).
    pub left_out: Mutex<Vec<PathBuf>>,
}

impl Fake {
    /// One whose start takes `delay` (a simulation's clock).
    pub fn taking(delay: Duration) -> Fake {
        Fake { delay, ..Fake::default() }
    }

    pub fn start(self: &Arc<Self>) -> Start {
        let f = Arc::clone(self);
        Arc::new(move |given: Given, out: Out| {
            if !f.delay.is_zero() {
                crate::event::sleep(f.delay);
            }
            if let Some(no) = &f.no {
                return Err(no.clone());
            }
            let once = |n: usize| n.checked_sub(1);
            if f.fails.try_update(SeqCst, SeqCst, once).is_ok() {
                return Err("a start that failed".into());
            }
            crate::treewatch::roots_taken(&given.roots)?;
            let roots: Vec<String> = given.roots.iter().map(|r| r.display().to_string()).collect();
            lock(&f.calls).push(format!("start {}", roots.join(" ")));
            *lock(&f.left_out) = given.leave_out;
            *lock(&f.out) = Some(out);
            *lock(&f.keep) = Some(given.keep);
            Ok(Arc::new(FakeWatch(Arc::clone(&f))) as Arc<dyn Watch>)
        })
    }

    pub fn hand(&self, batch: Batch) {
        if let Some(out) = lock(&self.out).as_mut() {
            out(Said { batch });
        }
    }

    pub fn started(&self) -> usize {
        lock(&self.calls).iter().filter(|c| c.starts_with("start")).count()
    }
}

struct FakeWatch(Arc<Fake>);

impl Watch for FakeWatch {
    fn set(&self, roots: Vec<PathBuf>, left_out: Vec<PathBuf>) -> Result<(), String> {
        crate::treewatch::roots_taken(&roots)?;
        *lock(&self.0.left_out) = left_out;
        let roots: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
        lock(&self.0.calls).push(format!("set {}", roots.join(" ")));
        Ok(())
    }
    fn keep(&self, keep: Keep) -> Result<(), String> {
        lock(&self.0.calls).push("keep".into());
        *lock(&self.0.keep) = Some(keep);
        Ok(())
    }
    fn look(&self, paths: Vec<PathBuf>) -> Result<(), String> {
        lock(&self.0.looks).extend(paths.iter().cloned());
        let keep = lock(&self.0.keep).clone();
        let batch: Batch = (paths.into_iter())
            .filter(|p| keep.as_ref().is_some_and(|k| k(p)))
            .map(|p| (p, Kind::Changed))
            .collect();
        if !batch.is_empty() {
            self.0.hand(batch);
        }
        Ok(())
    }
}

/// What stand-in servers were sent: (its first folder, method, params).
static HEARD: Mutex<Vec<(PathBuf, String, Value)>> = Mutex::new(Vec::new());

pub(crate) fn stand_in(_: &mut Editor, cid: ClientId, spec: &ServerSpec, roots: &[PathBuf]) -> Client {
    let root = roots[0].clone();
    let serve: crate::lsp::Serve = Box::new(move |method, params| {
        lock(&HEARD).push((root.clone(), method.to_string(), params.clone()));
        (method == "initialize").then(|| json!({ "capabilities": { "positionEncoding": "utf-8" } }))
    });
    let mut c = Client::own(cid, &spec.name, crate::lsp::traits(spec), serve);
    (c.spec, c.root, c.roots) = (spec.clone(), roots[0].clone(), roots.to_vec());
    c
}

/// The paths `root`'s stand-in heard in watched-files notifications, in order.
fn heard(root: &Path) -> Vec<String> {
    let all = lock(&HEARD);
    let notes = all.iter().filter(|(r, m, _)| r == root && m == METHOD);
    let changes = notes.flat_map(|(_, _, params)| params["changes"].as_array().cloned().unwrap_or_default());
    changes
        .map(|c| crate::lsp::path_from_uri(c["uri"].as_str().unwrap()).unwrap().display().to_string())
        .collect()
}

pub(crate) const CONFIG: &str =
    "[lsp.rust-analyzer]\ncommand = \"sh\"\n[lang.rust]\nlsp = [\"rust-analyzer\"]\n";

/// A cargo project in a scratch folder, its `src/main.rs` open, its server a stand-in whose pack says
/// `watched-files`, the tree watch `fake`.
fn editor(name: &str, fake: &Arc<Fake>) -> (crate::scratch::Scratch, PathBuf, Editor) {
    editor_with(name, fake, &[])
}

/// `editor`, with `folders` made in the project before its server starts.
fn editor_with(name: &str, fake: &Arc<Fake>, folders: &[&str]) -> (crate::scratch::Scratch, PathBuf, Editor) {
    let dir = crate::scratch::Scratch::new(name);
    let t = std::fs::canonicalize(dir.path()).unwrap();
    for f in folders {
        std::fs::create_dir_all(t.join(f)).unwrap();
    }
    std::fs::create_dir_all(t.join("src")).unwrap();
    std::fs::write(t.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
    std::fs::write(t.join("src/main.rs"), "fn main() {}\n").unwrap();
    let (config, _) = crate::config::parse(CONFIG);
    let mut ed = Editor::with_root(config, t.clone());
    ed.lsp.share.start = Some(stand_in);
    ed.lsp.watch.start = Some(fake.start());
    ed.open(&t.join("src/main.rs")).unwrap();
    (dir, t, ed)
}

fn decided(ed: &Editor) -> Option<bool> {
    ed.lsp.clients.first().and_then(|c| c.watched.get()).map(|w| w.declared)
}

fn register(ed: &mut Editor, id: &str, list: &[Value]) {
    let cid = ed.lsp.clients[0].id;
    let regs = json!([{ "id": id, "method": METHOD, "registerOptions": watchers(list) }]);
    let msg = json!({ "jsonrpc": "2.0", "id": 7, "method": "client/registerCapability",
        "params": { "registrations": regs } });
    ed.on_lsp_message(cid, msg);
}

fn keys(ed: &mut Editor, keys: &[&str]) {
    for k in keys {
        ed.handle_key(k.parse::<crate::key::Key>().unwrap());
    }
}

/// The pack key: a server whose pack says `watched-files` is given to the tree; one without it never is.
#[test]
fn only_a_server_whose_pack_says_so_is_given_to_the_tree() {
    let fake = Arc::new(Fake::default());
    let (_d, t, mut ed) = editor("lspwatch-pack-key", &fake);
    settle_until(&mut ed, |ed| decided(ed) == Some(true));
    assert_eq!(fake.started(), 1);
    assert_eq!(lock(&fake.calls)[0], format!("start {}", t.display()));
    let spec =
        |name: &str| ServerSpec { name: name.into(), command: "sh".into(), args: vec![], except: vec![] };
    assert!(ed.lsp_watch_gate(&spec("taplo"), &[t.join("other")]).is_none(), "its pack says nothing");
    assert!(ed.lsp_watch_gate(&spec("no-pack-knows-it"), &[t.join("other")]).is_none());
    assert_eq!(ed.lsp.watch.asked(), std::slice::from_ref(&t), "nothing asked for them");
}

/// Registered, a change handed in reaches the server; the tree keeps its globs; gone, its registrations go, the
/// tree's keep shrinks and the watch with its last server.
#[test]
fn a_registration_is_heard_and_goes_with_its_server() {
    let fake = Arc::new(Fake::default());
    let (_d, t, mut ed) = editor("lspwatch-register", &fake);
    settle_until(&mut ed, |ed| decided(ed) == Some(true));
    register(&mut ed, "w", &[relative(&t.display().to_string(), "**/*.rs")]);
    assert!(ed.lsp.watch.table().keeps(&t.join("src/lib.rs")));
    assert!(lock(&fake.calls).contains(&"keep".to_string()), "the tree told what to keep");
    fake.hand(vec![(t.join("src/lib.rs"), Kind::Created), (t.join("README.md"), Kind::Changed)]);
    let lib = t.join("src/lib.rs").display().to_string();
    settle_until(&mut ed, |_| heard(&t) == [lib.clone()]);
    let cid = ed.lsp.clients[0].id;
    ed.lsp_stopped(cid);
    assert!(ed.lsp.watch.table().servers.is_empty(), "its registrations went");
    assert!(!ed.lsp.watch.table().keeps(&t.join("src/lib.rs")));
    assert!(ed.lsp.watch.asked().is_empty() && !ed.lsp.watch.running(), "gone with its last server");
}

/// The tree watch says no: the server starts without the capability, and a later registration for it is answered
/// and kept out of the tree.
#[test]
fn the_watch_says_no_and_the_server_watches_by_itself() {
    let fake = Arc::new(Fake { no: Some("no tree watch here".into()), ..Fake::default() });
    let (_d, t, mut ed) = editor("lspwatch-no", &fake);
    settle_until(&mut ed, |ed| decided(ed) == Some(false));
    register(&mut ed, "w", &[relative(&t.display().to_string(), "**/*.rs")]);
    assert!(ed.lsp.watch.table().servers.is_empty(), "kept out of the tree");
    assert!(!ed.lsp.watch.running());
    settle_until(&mut ed, |ed| ed.lsp.watch.asked().is_empty());
}

/// A save (`:w`) with no event from the stream still reaches the server: the save's look.
#[test]
fn a_save_is_looked_at_and_reaches_the_server() {
    let fake = Arc::new(Fake::default());
    let (_d, t, mut ed) = editor("lspwatch-save", &fake);
    settle_until(&mut ed, |ed| decided(ed) == Some(true) && ed.lsp.watch.running());
    register(&mut ed, "w", &[relative(&t.display().to_string(), "**/*.rs")]);
    keys(&mut ed, &["i", "x", "esc", ":", "w", "ret"]);
    let main = t.join("src/main.rs").display().to_string();
    settle_until(&mut ed, |_| heard(&t).contains(&main));
    assert!(lock(&fake.looks).contains(&t.join("src/main.rs")));
}

/// The edit tool's change landing (a file it made, no buffer, no event from the stream) reaches the server: the
/// landing's look.
#[test]
fn an_edit_landing_is_looked_at_and_reaches_the_server() {
    let fake = Arc::new(Fake::default());
    let (_d, t, mut ed) = editor("lspwatch-landed", &fake);
    settle_until(&mut ed, |ed| decided(ed) == Some(true) && ed.lsp.watch.running());
    register(&mut ed, "w", &[relative(&t.display().to_string(), "**/*.rs")]);
    let made = t.join("src/made.rs");
    std::fs::write(&made, "pub fn made() {}\n").unwrap();
    let f = crate::refactor::FileChange {
        path: made.clone(),
        rel: "src/made.rs".into(),
        before: ropey::Rope::new(),
        after: ropey::Rope::from_str("pub fn made() {}\n"),
        places: 1,
        op: crate::refactor::Op::Create,
        protected: false,
    };
    crate::refactor::landed(&mut ed, &f, None, crate::refactor::Whose::Yours, None);
    let shown = made.display().to_string();
    settle_until(&mut ed, |_| heard(&t).contains(&shown));
}

/// Through the editor: the writer thread asks the root's output folders (the pack's `[builds]`, a stand-in here)
/// and a build-script output in a shared build folder outside the project reaches the server.
#[test]
fn a_generated_file_in_an_outside_build_folder_reaches_the_server() {
    let build = crate::scratch::Scratch::new("lspwatch-build-dir");
    let b = std::fs::canonicalize(build.path()).unwrap();
    let fake = Arc::new(Fake::default());
    let dir = crate::scratch::Scratch::new("lspwatch-outside");
    let t = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::create_dir_all(t.join("src")).unwrap();
    std::fs::write(t.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
    std::fs::write(t.join("src/main.rs"), "fn main() {}\n").unwrap();
    let (config, _) = crate::config::parse(CONFIG);
    let mut ed = Editor::with_root(config, t.clone());
    ed.lsp.share.start = Some(stand_in);
    ed.lsp.watch.start = Some(fake.start());
    let (root, at) = (t.clone(), b.clone());
    ed.lsp.watch.outputs = Some(Arc::new(move |r: &Path| {
        if r == root {
            vec![(root.join("target"), root.join("target")), (at.clone(), at.clone())]
        } else {
            vec![]
        }
    }));
    ed.open(&t.join("src/main.rs")).unwrap();
    settle_until(&mut ed, |ed| decided(ed) == Some(true) && ed.lsp.watch.running());
    let out = b.join("debug/build/x-1/out");
    register(&mut ed, "w", &[relative(&out.display().to_string(), "**/*.rs")]);
    assert!(ed.lsp.watch.table().own_roots().contains(&out), "watched as a root of its own");
    let gen_rs = out.join("gen.rs");
    fake.hand(vec![(gen_rs.clone(), Kind::Changed)]);
    let shown = gen_rs.display().to_string();
    settle_until(&mut ed, |_| heard(&t).contains(&shown));
}

/// Focus regained: every root the tree watches is looked at.
#[test]
fn focus_regained_looks_at_every_root() {
    let fake = Arc::new(Fake::default());
    let (_d, t, mut ed) = editor("lspwatch-focus", &fake);
    settle_until(&mut ed, |ed| decided(ed) == Some(true) && ed.lsp.watch.running());
    ed.handle_event(crate::event::Event::Focus(true));
    settle_until(&mut ed, |_| lock(&fake.looks).contains(&t));
}

/// A batch of 5,000 changes: the main loop's part is one push for the server.
#[test]
fn a_batch_of_5000_is_one_push_on_the_main_loop() {
    let fake = Arc::new(Fake::default());
    let (_d, t, mut ed) = editor("lspwatch-5000", &fake);
    settle_until(&mut ed, |ed| decided(ed) == Some(true) && ed.lsp.watch.running());
    register(&mut ed, "w", &[relative(&t.display().to_string(), "**/*.rs")]);
    fake.hand((0..5000).map(|i| (t.join(format!("src/m{i}.rs")), Kind::Changed)).collect());
    settle_until(&mut ed, |_| heard(&t).len() == 5000);
    assert_eq!(ed.lsp.watch.pushes, 1);
}

/// `initialize` as the writer thread writes it (a fake server's stdin, logged): the capability on a yes, none on
/// a no.
#[test]
fn initialize_declares_the_capability_only_on_a_yes() {
    for (name, answer, want) in
        [("lspwatch-init-yes", Ok(()), true), ("lspwatch-init-no", Err("no".to_string()), false)]
    {
        let dir = crate::scratch::Scratch::new(name);
        let t = std::fs::canonicalize(dir.path()).unwrap();
        let log = t.join("log");
        let spec = ServerSpec {
            name: "rust-analyzer".into(),
            command: "sh".into(),
            args: vec!["-c".into(), format!("cat > '{}'", log.display())],
            except: vec![],
        };
        let gate = Arc::new(Gate::default());
        gate.fill(answer);
        let roots = vec![t.clone()];
        let options =
            gated(crate::lsp::options_for(&spec, &roots), gate, roots.clone(), spec.name.clone(), None);
        let (config, _) = crate::config::parse("");
        let mut ed = Editor::with_root(config, t.clone());
        let c = Client::start(1, &spec, &roots, options, ed.events.shared_sender()).unwrap();
        let said = || std::fs::read_to_string(&log).unwrap_or_default();
        settle_until(&mut ed, |_| said().contains("\"initialize\"") && said().ends_with('}'));
        let caps: Value = serde_json::from_str(&said()[said().find('{').unwrap()..]).unwrap();
        let cap = &caps["params"]["capabilities"]["workspace"]["didChangeWatchedFiles"];
        assert_eq!(
            cap["relativePatternSupport"] == true && cap["dynamicRegistration"] == true,
            want,
            "{name}"
        );
        assert_eq!(c.watched.get().map(|w| w.declared), Some(want));
    }
}

/// An editor on a scratch folder with no file open: no server, the seam unset.
fn bare(name: &str) -> (crate::scratch::Scratch, PathBuf, Editor) {
    let dir = crate::scratch::Scratch::new(name);
    let t = std::fs::canonicalize(dir.path()).unwrap();
    let (config, _) = crate::config::parse(CONFIG);
    (dir, t.clone(), Editor::with_root(config, t))
}

fn ra() -> ServerSpec {
    ServerSpec { name: "rust-analyzer".into(), command: "sh".into(), args: vec![], except: vec![] }
}

/// An out dir watched as a root of its own, removed (`cargo clean`): a server starting meanwhile is still told yes,
/// though the roots asked name the absent one; made again, its file reaches the server.
#[test]
fn an_out_dir_removed_and_made_again_reaches_the_server_and_another_is_told_yes() {
    let fake = Arc::new(Fake::default());
    let (_d, t, mut ed) = editor("lspwatch-out-again", &fake);
    settle_until(&mut ed, |ed| decided(ed) == Some(true) && ed.lsp.watch.running());
    let out = t.join("target/debug/build/x-1/out");
    std::fs::create_dir_all(&out).unwrap();
    register(&mut ed, "w", &[relative(&out.display().to_string(), "**/*.rs")]);
    let set_with_out =
        |f: &Fake| lock(&f.calls).iter().any(|c| c.starts_with("set") && c.contains("x-1/out"));
    settle_until(&mut ed, |_| set_with_out(&fake));
    std::fs::remove_dir_all(t.join("target")).unwrap();
    let other = crate::scratch::Scratch::new("lspwatch-out-again-other");
    let o = std::fs::canonicalize(other.path()).unwrap();
    let gate = ed.lsp_watch_gate(&ra(), std::slice::from_ref(&o)).expect("asked");
    assert!(ed.lsp.watch.asked().contains(&out), "the absent out dir is still asked for");
    let patience = crate::test_results::PATIENCE;
    assert!(declared(&gate, std::slice::from_ref(&o), "t", crate::clock::now(), patience), "told yes");
    std::fs::create_dir_all(&out).unwrap();
    let gen_rs = out.join("gen.rs");
    std::fs::write(&gen_rs, "").unwrap();
    fake.hand(vec![(gen_rs.clone(), Kind::Created)]);
    let shown = gen_rs.display().to_string();
    settle_until(&mut ed, |_| heard(&t).contains(&shown));
}

/// A request whose start failed (a slow fseventsd past its wait) is asked again by the next with the same roots;
/// a yes is kept.
#[test]
fn a_failed_request_is_asked_again() {
    let fake = Arc::new(Fake::default());
    fake.fails.store(1, SeqCst);
    let (_d, t, mut ed) = bare("lspwatch-retry");
    ed.lsp.watch.start = Some(fake.start());
    let (roots, patience) = ([t.clone()], crate::test_results::PATIENCE);
    let first = ed.lsp_watch_request(&roots).expect("asked");
    assert!(first.wait(crate::clock::now(), patience).is_err(), "the first start fails");
    let again = ed.lsp_watch_request(&roots).expect("asked");
    assert!(!Arc::ptr_eq(&first, &again), "asked again, not the old no");
    assert_eq!(again.wait(crate::clock::now(), patience), Ok(()));
    assert_eq!(fake.started(), 1);
    let same = ed.lsp_watch_request(&roots).expect("asked");
    assert!(Arc::ptr_eq(&again, &same), "a yes is kept");
}

/// A root's output folders are asked once while a watched server has it, and again once the last one goes (a
/// restart after the build folder's setting changed sees it).
#[test]
fn output_folders_are_asked_again_once_the_last_server_on_a_root_goes() {
    let (_d, t, mut ed) = bare("lspwatch-outputs-again");
    let count = Arc::new(Mutex::new(0));
    let c = Arc::clone(&count);
    ed.lsp.watch.outputs = Some(Arc::new(move |_: &Path| {
        *lock(&c) += 1;
        vec![]
    }));
    let ask = ed.lsp_watch_ask();
    let outputs = ask.outputs.clone().expect("outputs to ask");
    let roots = [t.clone()];
    outputs_of(&outputs, &ask.asked, &roots);
    walked(&ask.walked, &roots);
    ed.lsp_watch_started(1, &roots);
    ed.lsp_watch_started(2, &roots);
    ed.lsp_watch_gone(1);
    outputs_of(&outputs, &ask.asked, &roots);
    assert_eq!(*lock(&count), 1, "another server still has the root");
    assert!(lock(&ask.walked).contains_key(&t), "its walk kept");
    ed.lsp_watch_gone(2);
    assert!(!lock(&ask.walked).contains_key(&t), "its walk gone with it");
    outputs_of(&outputs, &ask.asked, &roots);
    assert_eq!(*lock(&count), 2, "asked again");
}

/// The take-back of a file the turn made (removed, no buffer left, no event from the stream) is looked at.
#[test]
fn a_take_back_is_looked_at() {
    let fake = Arc::new(Fake::default());
    let (_d, t, mut ed) = editor("lspwatch-take-back", &fake);
    settle_until(&mut ed, |ed| decided(ed) == Some(true) && ed.lsp.watch.running());
    let made = t.join("src/made.rs");
    std::fs::write(&made, "pub fn made() {}\n").unwrap();
    let turn = ropey::Rope::from_str("pub fn made() {}\n");
    assert_eq!(crate::checkpoint::file(&mut ed, &made, Some(&turn), None), Ok((true, 0)));
    assert!(!made.exists());
    settle_until(&mut ed, |_| lock(&fake.looks).contains(&made));
}

/// A rename's edits (a server's workspace edit, applied) are looked at.
#[test]
fn a_renames_edits_are_looked_at() {
    let fake = Arc::new(Fake::default());
    let (_d, t, mut ed) = editor("lspwatch-rename-edits", &fake);
    settle_until(&mut ed, |ed| decided(ed) == Some(true) && ed.lsp.watch.running());
    let main = t.join("src/main.rs");
    let at = json!({ "line": 0, "character": 0 });
    let mut changes = serde_json::Map::new();
    changes.insert(
        crate::lsp::uri(&main),
        json!([{ "range": { "start": at, "end": at }, "newText": "// x\n" }]),
    );
    let cid = ed.lsp.clients[0].id;
    let done = Arc::new(Mutex::new(None));
    let d = Arc::clone(&done);
    ed.apply_workspace_edits_then(cid, vec![json!({ "changes": changes })], move |_, r| *lock(&d) = Some(r));
    settle_until(&mut ed, |_| lock(&done).is_some());
    assert_eq!(*lock(&done), Some(Ok(1)), "the edit applied");
    settle_until(&mut ed, |_| lock(&fake.looks).contains(&main));
}

/// A git action's end (`disk_batch_end`) looks at its checkout.
#[test]
fn a_git_actions_end_is_looked_at() {
    let fake = Arc::new(Fake::default());
    let (_d, t, mut ed) = editor("lspwatch-git-end", &fake);
    settle_until(&mut ed, |ed| decided(ed) == Some(true) && ed.lsp.watch.running());
    ed.disk_batch_told("git pull", 7);
    assert!(lock(&fake.looks).is_empty());
    ed.disk_batch_end(7);
    settle_until(&mut ed, |_| lock(&fake.looks).contains(&t));
}
