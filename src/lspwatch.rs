//! tarae as its language servers' file watcher (W1b). A server whose pack says `watched-files = true` is told at
//! `initialize` that tarae watches (`workspace.didChangeWatchedFiles`: dynamic, relative patterns) once the tree
//! watch (`treewatch.rs`) said yes for its folders — waited for on its writer thread (`gated`), never past `WAIT`;
//! a no, or no answer by then, and it starts without and watches by itself, as before. Told, it hears of every
//! change to a file one of its registrations asks about, below the folders it serves.
//!
//! - One tree watch for the editor (`State`): started by a job with the first watched server's folders, its roots
//!   following the watched servers' (`Editor::lsp_watch_request`), gone with the last. It comes through a seam
//!   (`Start`), so tests and the simulation hand batches in.
//! - Registrations (`client/registerCapability` for `workspace/didChangeWatchedFiles`) are kept per server and id:
//!   the same id again replaces, an unregistered one goes, a server gone takes its own. The tree keeps the union of
//!   the servers' globs (`Table::keeps`).
//! - A path goes to a server when the nearest folder above it — among the server's folders, the folders the tree
//!   leaves out below them (`walk_left_out`: each folder's `.git` and the builds' outputs, the checkouts inside it,
//!   the output folders deeper down that git ignores) and the bases watched as roots of their own — is one of its
//!   folders or such a base, and one of its watchers matches the path and its kind.
//! - Which bases are watched is one rule, `served`.
//! - A batch is split per server on the tree watch's worker (`Table::split`); the main loop pushes one
//!   notification a server, serialized on its writer thread.
//! - Looks at known moments (`Editor::lsp_look`): a write of tarae's own, a run's or a test's end, git's, focus
//!   regained — the tree watch compares what its stream may have missed.
//! - Paths are compared as real ones (FSEvents names `/private/tmp`); a server is told them in the form of the
//!   folders it was given.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::editor::Editor;
use crate::event::Event;
use crate::lsp::ClientId;
use crate::treewatch::{Batch, Given, Keep, Kind, Said, TreeWatch};

/// How long a server's `initialize` waits for the tree watch's yes for its folders.
pub const WAIT: Duration = Duration::from_secs(10);

/// A wait past this is said in the log, with its time.
const WAIT_SAID: Duration = Duration::from_secs(1);

/// How long a request of the tree watch's waits for the one before it to land (a stream's start: seconds on a slow
/// fseventsd) — past it, it goes on. The wait is on the request's job, a thread of its own (`tarae-job`: one a job,
/// no pool): a hung fseventsd holds one such thread a request waiting, each for at most this long.
const BEFORE_WAIT: Duration = Duration::from_secs(60);

pub const METHOD: &str = "workspace/didChangeWatchedFiles";

/// A watcher's `kind` when it says none: create, change and delete.
const KIND_ALL: u64 = 7;

/// Where the project at a root builds, as its pack's `[builds]` says (`Builds::ask`: its target and build folders)
/// — each as printed and real. A process: on a worker (the server's writer thread), once a root (`outputs_of`).
pub type Outputs = Arc<dyn Fn(&Path) -> Vec<(PathBuf, PathBuf)> + Send + Sync>;

fn builds_outputs(b: crate::builds::Builds) -> Outputs {
    Arc::new(move |root: &Path| {
        let Some(Ok(tool)) = b.tool_for(root) else { return Vec::new() };
        let Some((target, build)) = b.ask(tool, root) else { return Vec::new() };
        let (real_target, real_build) = crate::builds::real_dirs((target.clone(), build.clone()));
        vec![(target, real_target), (build, real_build)]
    })
}

/// The answers asked so far, by root.
type Asked = Arc<Mutex<BTreeMap<PathBuf, Vec<(PathBuf, PathBuf)>>>>;

/// `roots`' output folders: each root asked once (`asked` keeps the answers, until its last watched server goes),
/// on the calling worker. One that can't be an output folder (`plausible`) is said in the log as it's asked, and
/// `served` doesn't count it.
fn outputs_of(ask: &Outputs, asked: &Asked, roots: &[PathBuf]) -> Vec<(PathBuf, PathBuf)> {
    let mut out = Vec::new();
    for r in roots {
        let known = lock(asked).get(r).cloned();
        let these = known.unwrap_or_else(|| {
            let these = ask(r);
            for (printed, real) in these.iter().filter(|(_, real)| !plausible(real, &[self::real(r)])) {
                let said = format!(
                    "{}'s builds name {} ({}) as an output folder: not counted (/, home or above it, or holding the \
                     project)",
                    r.display(),
                    printed.display(),
                    real.display()
                );
                crate::log::line("lsp", "watch", &said);
            }
            lock(asked).insert(r.clone(), these.clone());
            these
        });
        out.extend(these);
    }
    out
}

/// Whether a folder the project's builds name can be an output folder: not `/`, not home or a folder above it,
/// holding none of the server's folders (`roots`, real) — a cargo config may say `target-dir = ".."`.
fn plausible(output: &Path, roots: &[PathBuf]) -> bool {
    let home = crate::allow::home();
    // (`/` is above home, and holds every root)
    !home.is_some_and(|h| h.starts_with(output)) && !roots.iter().any(|r| r.starts_with(output))
}

/// What `served` says of a registration's base.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base {
    /// Below one of the server's folders, nothing left out between: that folder's watch covers it.
    InRoot,
    /// Below a folder left out, or outside below one of the project's output folders: watched as a root of its
    /// own.
    OwnRoot,
    /// Not watched: logged once a server and base.
    Refused,
}

/// Is this base served? The one place the rule lives: below one of `roots` it is — by that root's watch when the
/// nearest folder above it (itself included), among `roots` and `left_out`, is a root, else as a root of its own;
/// outside them, only below one of `outputs` — the project's own output folders, as its pack's `[builds]` names
/// them (`outputs_of`: a shared `build-dir`'s build-script output), each only if it can be one (`plausible`) —;
/// else refused (rust-analyzer's user config folder). The user, 2026-11-11, "for now": main's
/// `.sb/decisions/editor.md`.
pub fn served(base: &Path, roots: &[PathBuf], left_out: &[PathBuf], outputs: &[PathBuf]) -> Base {
    if roots.iter().any(|r| base.starts_with(r)) {
        for a in base.ancestors() {
            if roots.iter().any(|r| r == a) {
                return Base::InRoot;
            }
            if left_out.iter().any(|l| l == a) {
                return Base::OwnRoot;
            }
        }
    }
    if outputs.iter().any(|o| base.starts_with(o) && plausible(o, roots)) {
        return Base::OwnRoot;
    }
    Base::Refused
}

/// The fixed part of what the tree leaves out below `roots`: each root's `.git` and the builds' outputs
/// (`sandbox::outputs`) — the paths, there or not (a fresh clone's `target` is made by the first build, seconds
/// after the server's start). The whole list is `walk_left_out`'s.
pub fn tree_left_out(roots: &[PathBuf]) -> Vec<PathBuf> {
    let names = std::iter::once(".git").chain(crate::sandbox::outputs().iter().map(String::as_str));
    let names: Vec<&str> = names.collect();
    roots.iter().flat_map(|r| names.iter().map(move |n| r.join(n))).collect()
}

/// The output-named folders below a root's own children that git is asked about, at most — the shallowest, then
/// by path; the cut said in the log.
const DEEP_MAX: usize = 64;

/// What the tree leaves out below `root` (real), whoever serves it — in the order that decides which go to its
/// stream's `EXCLUDED_MAX` places:
/// - its `.git` and output folders, there or not (`tree_left_out`);
/// - the checkouts inside it (`lsp::nested_walk`: `.claude/worktrees`, a folder with its own `.git`): not its
///   project, whichever server serves one — that one is a root of its own, with its own stream (the design session,
///   2026-11-11, reversing "another server may serve it");
/// - the output-named folders deeper down that git ignores (a member's own `target`, `ignored_outputs`) — never by
///   name alone, which would silence a tracked `src/target` for good.
///
/// One walk and at most one git, on a worker; a checkout or folder made later waits for the next roots change.
pub fn walk_left_out(root: &Path) -> Vec<PathBuf> {
    let mut list = tree_left_out(&[root.to_path_buf()]);
    let nested = crate::lsp::nested_walk(root);
    list.extend(nested.checkouts.iter().map(|rel| root.join(rel)));
    list.extend(ignored_outputs(root, nested.outputs));
    list
}

/// Of the output-named folders `found` below `root` (with their depths), the ones git ignores there: the first
/// `DEEP_MAX` by depth then path, asked in one `git check-ignore`. Not a repository, or git fails: none, said.
fn ignored_outputs(root: &Path, mut found: Vec<(usize, PathBuf)>) -> Vec<PathBuf> {
    if found.is_empty() {
        return Vec::new();
    }
    crate::sorting::sort(&mut found);
    if found.len() > DEEP_MAX {
        let said = format!(
            "{} has {} output-named folders below its own: git is asked about the first {DEEP_MAX}, the rest \
             stay watched",
            root.display(),
            found.len()
        );
        crate::log::line("lsp", "watch", &said);
        found.truncate(DEEP_MAX);
    }
    let input: String = found.iter().map(|(_, p)| format!("{}\0", p.display())).collect();
    let asked = crate::gitrepo::repo(root).and_then(|repo| {
        let cmd = repo.command(&["check-ignore", "-z", "--stdin"]);
        crate::gitrepo::ran(&cmd, Some(input.as_bytes()))
    });
    let out = match asked {
        Ok(r) if r.ok => r.stdout,
        // (exit 1, nothing on stderr: none of them is ignored)
        Ok(r) if r.stderr.is_empty() => Vec::new(),
        Ok(r) => return not_asked(root, String::from_utf8_lossy(&r.stderr).trim()),
        Err(e) => return not_asked(root, &e),
    };
    let ignored: Vec<PathBuf> = out
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| PathBuf::from(&*String::from_utf8_lossy(s)))
        .collect();
    found.into_iter().map(|(_, p)| p).filter(|p| ignored.contains(p)).collect()
}

fn not_asked(root: &Path, why: &str) -> Vec<PathBuf> {
    let said = format!(
        "{}: git couldn't say which output-named folders below it are ignored ({why}): none is left out",
        root.display()
    );
    crate::log::line("lsp", "watch", &said);
    Vec::new()
}

/// What the tree leaves out below each root walked so far, by root as given (`walk_left_out`), until its last
/// watched server goes.
type Walked = Arc<Mutex<BTreeMap<PathBuf, Vec<PathBuf>>>>;

/// `roots`' (as given) lists, each walked once (`walked` keeps them), on the calling worker: real paths.
fn walked(walked: &Walked, roots: &[PathBuf]) -> Vec<Vec<PathBuf>> {
    (roots.iter())
        .map(|r| {
            let known = lock(walked).get(r).cloned();
            known.unwrap_or_else(|| {
                let list = walk_left_out(&real(r));
                lock(walked).insert(r.clone(), list.clone());
                list
            })
        })
        .collect()
}

/// A glob as LSP says it: `*` and `?` within one segment, `**` across them, `{a,b}`, `[a-z]`, `[!a-z]`.
pub fn glob(pattern: &str) -> Result<globset::GlobMatcher, String> {
    let g = globset::GlobBuilder::new(pattern).literal_separator(true).build().map_err(|e| e.to_string())?;
    Ok(g.compile_matcher())
}

#[derive(Clone)]
struct Watcher {
    /// A relative pattern's base (real), matched below it; None: a plain pattern, matched against the whole path —
    /// in the server's form (`/tmp/x/Cargo.toml` for a project it was given as `/tmp/x`), or the real one.
    base: Option<PathBuf>,
    /// Its base is a root of its own (`Base::OwnRoot`).
    own: bool,
    glob: globset::GlobMatcher,
    /// Create 1, change 2, delete 4.
    kind: u64,
}

impl Watcher {
    /// `path` real; `given`, the same in the server's form (found only for a plain pattern).
    fn matches(&self, path: &Path, given: impl FnOnce() -> PathBuf) -> bool {
        match &self.base {
            Some(b) => path.strip_prefix(b).is_ok_and(|rel| self.glob.is_match(rel)),
            None => self.glob.is_match(given()) || self.glob.is_match(path),
        }
    }
}

fn kind_bit(k: Kind) -> u64 {
    match k {
        Kind::Created => 1,
        Kind::Changed => 2,
        Kind::Deleted => 4,
    }
}

/// LSP's `FileChangeType`.
fn change_type(k: Kind) -> u64 {
    match k {
        Kind::Created => 1,
        Kind::Changed => 2,
        Kind::Deleted => 3,
    }
}

/// A watched server: its folders as it was given them and as they really are, what it leaves out (real), its
/// project's output folders (as printed and real: a base below one outside its folders is watched), its
/// registrations by id.
#[derive(Clone)]
pub struct Server {
    roots: Vec<(PathBuf, PathBuf)>,
    left_out: Vec<PathBuf>,
    outputs: Vec<(PathBuf, PathBuf)>,
    regs: BTreeMap<String, Vec<Watcher>>,
}

/// What a registration's watchers were read as: the bases refused, the patterns that didn't read.
#[derive(Default, Debug, PartialEq)]
pub struct Read {
    pub refused: Vec<String>,
    pub bad: Vec<String>,
}

impl Server {
    /// `roots`: (given, real) pairs; `left_out`: real — its nested checkouts and the tree's (`tree_left_out`);
    /// `outputs`: (printed, real) pairs.
    pub fn new(
        roots: Vec<(PathBuf, PathBuf)>,
        left_out: Vec<PathBuf>,
        outputs: Vec<(PathBuf, PathBuf)>,
    ) -> Server {
        Server { roots, left_out, outputs, regs: BTreeMap::new() }
    }

    /// Its folders, then its output folders: (as it knows them, real).
    fn pairs(&self) -> impl Iterator<Item = &(PathBuf, PathBuf)> {
        self.roots.iter().chain(&self.outputs)
    }

    fn real_roots(&self) -> Vec<PathBuf> {
        self.roots.iter().map(|(_, r)| r.clone()).collect()
    }

    /// A path the server named, in the real form (below a folder it was given or an output folder as printed:
    /// below that folder's real path).
    fn to_real(&self, p: &Path) -> PathBuf {
        for (given, real) in self.pairs() {
            if let Ok(rest) = p.strip_prefix(given) {
                return real.join(rest);
            }
        }
        p.to_path_buf()
    }

    /// A real path in the form the server knows its folders by.
    fn to_given(&self, p: &Path) -> PathBuf {
        for (given, real) in self.pairs() {
            if let Ok(rest) = p.strip_prefix(real) {
                return given.join(rest);
            }
        }
        p.to_path_buf()
    }

    /// Registration `id` (the same id again replaces): its `registerOptions.watchers` read, a base checked by
    /// `served`.
    pub fn register(&mut self, id: &str, options: &Value) -> Read {
        let mut read = Read::default();
        let mut watchers = Vec::new();
        let roots = self.real_roots();
        let outputs: Vec<PathBuf> = self.outputs.iter().map(|(_, real)| real.clone()).collect();
        for w in options["watchers"].as_array().into_iter().flatten() {
            let kind = w["kind"].as_u64().unwrap_or(KIND_ALL);
            let (base, pattern) = match &w["globPattern"] {
                Value::String(p) => (None, p.clone()),
                o => {
                    // (a folder's URI, or a workspace folder object)
                    let uri = o["baseUri"].as_str().or_else(|| o["baseUri"]["uri"].as_str());
                    let (Some(uri), Some(p)) = (uri, o["pattern"].as_str()) else {
                        read.bad.push(w["globPattern"].to_string());
                        continue;
                    };
                    let Some(base) = crate::lsp::path_from_uri(uri) else {
                        read.bad.push(uri.to_string());
                        continue;
                    };
                    (Some(self.to_real(&base)), p.to_string())
                }
            };
            let Ok(glob) = glob(&pattern) else {
                read.bad.push(pattern);
                continue;
            };
            let own = match &base {
                Some(b) => match served(b, &roots, &self.left_out, &outputs) {
                    Base::InRoot => false,
                    Base::OwnRoot => true,
                    Base::Refused => {
                        read.refused.push(b.display().to_string());
                        continue;
                    }
                },
                None => {
                    // (a plain absolute pattern whose fixed folder is outside every root, in the server's form or
                    // the real one, can't match: refused, so it's seen)
                    let near = |f: &Path, r: &Path| f.starts_with(r) || r.starts_with(f);
                    if let Some(fixed) = fixed_folder(&pattern)
                        && !self.roots.iter().any(|(given, real)| near(&fixed, given) || near(&fixed, real))
                    {
                        read.refused.push(pattern);
                        continue;
                    }
                    false
                }
            };
            watchers.push(Watcher { base, own, glob, kind });
        }
        self.regs.insert(id.to_string(), watchers);
        read
    }

    pub fn unregister(&mut self, id: &str) {
        self.regs.remove(id);
    }

    fn watchers(&self) -> impl Iterator<Item = &Watcher> {
        self.regs.values().flatten()
    }

    /// The bases it has watched as roots of their own.
    fn own_roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.watchers().filter(|w| w.own).filter_map(|w| w.base.as_ref())
    }

    /// The nearest folder above `path` among its folders, its own roots and what it leaves out is a folder or an
    /// own root.
    fn admitted(&self, path: &Path) -> bool {
        for a in path.ancestors().skip(1) {
            if self.roots.iter().any(|(_, r)| r == a) || self.own_roots().any(|o| o == a) {
                return true;
            }
            if self.left_out.iter().any(|l| l == a) {
                return false;
            }
        }
        false
    }

    fn keeps(&self, path: &Path) -> bool {
        self.admitted(path) && self.watchers().any(|w| w.matches(path, || self.to_given(path)))
    }

    fn wants(&self, path: &Path, kind: Kind) -> bool {
        let bit = kind_bit(kind);
        self.admitted(path)
            && self.watchers().any(|w| w.kind & bit != 0 && w.matches(path, || self.to_given(path)))
    }
}

/// A plain pattern's folder before its first glob character, when it's absolute.
fn fixed_folder(pattern: &str) -> Option<PathBuf> {
    if !pattern.starts_with('/') {
        return None;
    }
    let end = pattern.find(['*', '?', '[', '{']).unwrap_or(pattern.len());
    let fixed = &pattern[..end];
    let fixed = if end == pattern.len() { fixed } else { &fixed[..fixed.rfind('/').unwrap_or(0)] };
    Some(PathBuf::from(if fixed.is_empty() { "/" } else { fixed }))
}

/// Every watched server's registrations — what the tree keeps and how a batch is split. Shared with the tree
/// watch's worker by `Arc`; replaced whole when one changes.
#[derive(Clone, Default)]
pub struct Table {
    pub servers: BTreeMap<ClientId, Server>,
}

impl Table {
    pub fn keeps(&self, path: &Path) -> bool {
        self.servers.values().any(|s| s.keeps(path))
    }

    pub fn keep(self: &Arc<Self>) -> Keep {
        let t = Arc::clone(self);
        Arc::new(move |p: &Path| t.keeps(p))
    }

    /// The bases watched as roots of their own, every server's.
    pub fn own_roots(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.servers.values().flat_map(|s| s.own_roots().cloned()).collect();
        crate::sorting::sort(&mut v);
        v.dedup();
        v
    }

    /// A batch split per server: each server's changes (LSP's `FileEvent`s, in its folders' form), a path once.
    /// A server with none isn't listed. On the tree watch's worker.
    pub fn split(&self, batch: &Batch) -> Vec<(ClientId, Vec<Value>)> {
        let mut out = Vec::new();
        for (cid, s) in &self.servers {
            let changes: Vec<Value> = batch
                .iter()
                .filter(|(p, k)| s.wants(p, *k))
                .map(|(p, k)| json!({ "uri": crate::lsp::uri(&s.to_given(p)), "type": change_type(*k) }))
                .collect();
            if !changes.is_empty() {
                out.push((*cid, changes));
            }
        }
        out
    }
}

/// The tree watch as the LSP side uses it (`TreeWatch`, or a test's fake).
pub trait Watch: Send + Sync {
    /// The roots and what's left out, in one go (one stream start at most).
    fn set(&self, roots: Vec<PathBuf>, leave_out: Vec<PathBuf>) -> Result<(), String>;
    fn keep(&self, keep: Keep) -> Result<(), String>;
    fn look(&self, paths: Vec<PathBuf>) -> Result<(), String>;
}

impl Watch for TreeWatch {
    fn set(&self, roots: Vec<PathBuf>, leave_out: Vec<PathBuf>) -> Result<(), String> {
        TreeWatch::set(self, roots, leave_out)
    }
    fn keep(&self, keep: Keep) -> Result<(), String> {
        TreeWatch::keep(self, keep)
    }
    fn look(&self, paths: Vec<PathBuf>) -> Result<(), String> {
        TreeWatch::look(self, paths)
    }
}

/// Where each settled burst goes (on the tree watch's worker).
pub type Out = Box<dyn FnMut(Said) + Send>;

/// How the tree watch is started (the seam: a test's or a scenario's fake hands batches to `Out`). Blocks: a job's.
pub type Start = Arc<dyn Fn(Given, Out) -> Result<Arc<dyn Watch>, String> + Send + Sync>;

fn real_start() -> Start {
    Arc::new(|given, out| TreeWatch::start(given, out).map(|w| Arc::new(w) as Arc<dyn Watch>))
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A yes or no to come, once: the tree watch's for some folders, filled by the job that asked; waited for by a
/// server's writer thread (bounded) and by the next request's job.
#[derive(Default)]
pub struct Gate {
    /// The answer and when it came.
    slot: Mutex<Option<(Result<(), String>, Instant)>>,
    cv: Condvar,
}

impl Gate {
    /// The answer, the first one only.
    pub fn fill(&self, answer: Result<(), String>) {
        let mut s = lock(&self.slot);
        if s.is_none() {
            *s = Some((answer, crate::clock::now()));
            self.cv.notify_all();
        }
    }

    /// Whether it holds a no (a failed start or change: the next request with the same roots asks again).
    fn said_no(&self) -> bool {
        matches!(&*lock(&self.slot), Some((Err(_), _)))
    }

    /// The answer, waited for until `bound` past `since` — one that came later is a no saying so, as none by then.
    pub fn wait(&self, since: Instant, bound: Duration) -> Result<(), String> {
        let end = since + bound;
        let late =
            || Err(format!("no answer from the tree watch within {}", crate::term::run_duration(bound)));
        let within = |s: &Option<(Result<(), String>, Instant)>| match s {
            Some((a, at)) if *at <= end => a.clone(),
            _ => late(),
        };
        #[cfg(test)]
        if crate::sim::current().is_some() {
            // (a simulation's job yields to the others by its clock — a condvar would hold the run)
            loop {
                let now = crate::clock::now();
                let s = lock(&self.slot);
                if s.is_some() || now >= end {
                    return within(&s);
                }
                drop(s);
                crate::event::pause((end - now).min(Duration::from_millis(100)));
            }
        }
        let left = end.saturating_duration_since(crate::clock::now());
        let s = lock(&self.slot);
        let (s, _) = self.cv.wait_timeout_while(s, left, |s| s.is_none()).unwrap_or_else(|e| e.into_inner());
        within(&s)
    }
}

/// What a server's writer thread decided before `initialize` went (`Client.watched`): whether it was told tarae
/// watches; its folders' real paths, in their order; what the tree leaves out below them (`walk_left_out`), real.
#[derive(Clone, Debug, Default)]
pub struct Watched {
    pub declared: bool,
    pub real: Vec<PathBuf>,
    pub left_out: Vec<PathBuf>,
    /// Its project's output folders, as printed and real (`outputs_of`).
    pub outputs: Vec<(PathBuf, PathBuf)>,
}

/// `path` as it really is — one that's gone (a file, an out dir after `cargo clean`): the nearest folder above it
/// that's there, real, and the rest as written.
fn real(path: &Path) -> PathBuf {
    for a in path.ancestors() {
        if let Ok(r) = std::fs::canonicalize(a) {
            return match path.strip_prefix(a) {
                Ok(rest) if rest.as_os_str().is_empty() => r,
                Ok(rest) => r.join(rest),
                Err(_) => path.to_path_buf(),
            };
        }
    }
    path.to_path_buf()
}

/// On the server's writer thread, before `initialize` is serialized: each folder watchable, then the tree watch's
/// yes for them, come within `bound` of `since` (its start). Its log says a wait past `WAIT_SAID` and why it
/// watches by itself.
pub fn declared(gate: &Gate, roots: &[PathBuf], name: &str, since: Instant, bound: Duration) -> bool {
    for r in roots {
        if let Err(e) = crate::treewatch::watchable(r) {
            crate::log::line(name, "watch", &format!("{}: {e} — it watches its files itself", r.display()));
            return false;
        }
    }
    let began = crate::clock::now();
    let answer = gate.wait(since, bound);
    let took = crate::clock::since(began);
    if took > WAIT_SAID {
        let said = format!("waited {} for the tree watch", crate::term::run_duration(took));
        crate::log::line(name, "watch", &said);
    }
    match answer {
        Ok(()) => true,
        Err(e) => {
            crate::log::line(name, "watch", &format!("it watches its files itself: {e}"));
            false
        }
    }
}

/// What the writer thread asks before `initialize` beyond the tree watch's yes: the project's output folders (a
/// process, once a root; None: none to ask) and what the tree leaves out below its folders (a walk, once a root).
#[derive(Clone)]
pub struct Ask {
    pub outputs: Option<Outputs>,
    asked: Asked,
    walked: Walked,
}

/// `options` with the tree watch's answer: made on the writer thread (`declared`, `WAIT` from now — the server's
/// start), with the folders' real paths, what the tree leaves out below them and the project's output folders
/// (asked first: the tree watch starts meanwhile; `ask` None: none asked, the walk not kept).
pub fn gated(
    options: crate::lsp::MakeOptions,
    gate: Arc<Gate>,
    roots: Vec<PathBuf>,
    name: String,
    ask: Option<Ask>,
) -> crate::lsp::MakeOptions {
    let since = crate::clock::now();
    Box::new(move || {
        let mut o = options();
        let walked_at = ask.as_ref().map(|a| Arc::clone(&a.walked)).unwrap_or_default();
        let left_out = walked(&walked_at, &roots).into_iter().flatten().collect();
        let outputs = ask.and_then(|a| Some(outputs_of(&a.outputs?, &a.asked, &roots))).unwrap_or_default();
        o.watched = Watched {
            declared: declared(&gate, &roots, &name, since, WAIT),
            real: roots.iter().map(|r| real(r)).collect(),
            left_out,
            outputs,
        };
        o
    })
}

/// The tree watch, once a job started it, and which request it's of (a stop makes a job's late start go).
#[derive(Default)]
struct Holder {
    generation: u64,
    watch: Option<Arc<dyn Watch>>,
}

/// The LSP side's tree watch (`LspState.watch`).
pub struct State {
    /// What the tree keeps and how a batch splits — the worker reads it.
    pub table: Arc<Mutex<Arc<Table>>>,
    holder: Arc<Mutex<Holder>>,
    /// The watched servers' folders, as given (from their start to their stop).
    servers: BTreeMap<ClientId, Vec<PathBuf>>,
    /// The roots last asked for, and that request's answer to come.
    asked: Vec<PathBuf>,
    last: Option<Arc<Gate>>,
    /// What's been logged once (a refused base, a registration with no capability).
    said: HashSet<String>,
    /// The seam: None, nothing's watched (tests that don't set it).
    pub start: Option<Start>,
    /// How a root's output folders are asked (the pack's `[builds]`, from `WorkspaceStore.builds` at the first
    /// watched server's start; a test's own) and the answers so far — None: none to ask.
    pub outputs: Option<Outputs>,
    asked_outputs: Asked,
    /// What the tree leaves out below each watched server's folders, walked so far.
    walked: Walked,
    /// Tests: the notifications pushed on the main loop.
    #[cfg(test)]
    pub pushes: usize,
}

impl Default for State {
    fn default() -> State {
        State {
            table: Default::default(),
            holder: Default::default(),
            servers: BTreeMap::new(),
            asked: Vec::new(),
            last: None,
            said: HashSet::new(),
            start: if cfg!(test) { None } else { Some(real_start()) },
            outputs: None,
            asked_outputs: Default::default(),
            walked: Default::default(),
            #[cfg(test)]
            pushes: 0,
        }
    }
}

impl State {
    /// The roots asked of the tree watch (tests).
    #[cfg(test)]
    pub fn asked(&self) -> &[PathBuf] {
        &self.asked
    }

    /// Whether the tree watch is up (tests).
    #[cfg(test)]
    pub fn running(&self) -> bool {
        lock(&self.holder).watch.is_some()
    }

    fn table(&self) -> Arc<Table> {
        Arc::clone(&lock(&self.table))
    }

    fn say_once(&mut self, source: &str, text: String) {
        if self.said.insert(format!("{source}\0{text}")) {
            crate::log::line(source, "watch", &text);
        }
    }
}

/// `wanted` less the roots another one covers (below it, nothing between in that one's whole list of what the tree
/// leaves out, `left_out` — none for a root it doesn't hold: a registration's base), sorted. The whole list: a
/// server's folder inside a checkout the outer root leaves out isn't covered by it, but silenced by its stream.
fn minimal(mut wanted: Vec<PathBuf>, left_out: &BTreeMap<PathBuf, Vec<PathBuf>>) -> Vec<PathBuf> {
    crate::sorting::sort(&mut wanted);
    wanted.dedup();
    let covered = |r: &PathBuf, p: &PathBuf| {
        r != p && r.starts_with(p) && !left_out.get(p).into_iter().flatten().any(|l| r.starts_with(l))
    };
    wanted.iter().filter(|r| !wanted.iter().any(|p| covered(r, p))).cloned().collect()
}

impl Editor {
    /// A server about to start: its folders' yes or no to come, when its pack says it's told tarae watches (and
    /// there's a tree watch to have) — the tree watch asked for them if it doesn't cover them.
    pub(crate) fn lsp_watch_gate(
        &mut self,
        spec: &crate::lsp::ServerSpec,
        roots: &[PathBuf],
    ) -> Option<Arc<Gate>> {
        if !crate::lsp::traits(spec).watched_files {
            return None;
        }
        self.lsp_watch_request(roots)
    }

    /// What a watched server's writer thread asks of its roots: their builds, and what the tree leaves out (`gated`).
    pub(crate) fn lsp_watch_ask(&mut self) -> Ask {
        if self.lsp.watch.outputs.is_none() {
            self.lsp.watch.outputs = self.workspace_store.builds.clone().map(builds_outputs);
        }
        let w = &self.lsp.watch;
        Ask { outputs: w.outputs.clone(), asked: Arc::clone(&w.asked_outputs), walked: Arc::clone(&w.walked) }
    }

    /// A server told (or being told) tarae watches is up for `roots`.
    pub(crate) fn lsp_watch_started(&mut self, cid: ClientId, roots: &[PathBuf]) {
        self.lsp.watch.servers.insert(cid, roots.to_vec());
    }

    /// The tree watch's roots made what the watched servers (with `adding`) and their own roots want: a request on
    /// a job when that changed — after the last one landed, the watch started if there's none, its roots and what
    /// it leaves out set if there is. Gone when nothing's wanted. The answer to come for `adding`.
    pub(crate) fn lsp_watch_request(&mut self, adding: &[PathBuf]) -> Option<Arc<Gate>> {
        let start = self.lsp.watch.start.clone()?;
        let w = &mut self.lsp.watch;
        let mut served: Vec<PathBuf> = w.servers.values().flatten().chain(adding).cloned().collect();
        crate::sorting::sort(&mut served);
        served.dedup();
        let mut wanted = served.clone();
        wanted.extend(w.table().own_roots());
        crate::sorting::sort(&mut wanted);
        wanted.dedup();
        if wanted.is_empty() {
            let mut h = lock(&w.holder);
            h.generation += 1;
            // (its worker stops as it's dropped — a send)
            h.watch = None;
            drop(h);
            (w.asked, w.last) = (Vec::new(), None);
            return None;
        }
        // (the same roots: the answer to come, or had — but a no is asked again: one slow start isn't every later
        // server's no)
        if wanted == w.asked && !w.last.as_ref().is_some_and(|g| g.said_no()) {
            return w.last.clone();
        }
        let gate = Arc::new(Gate::default());
        let before = w.last.replace(Arc::clone(&gate));
        w.asked = wanted.clone();
        let (holder, table, tx) = (Arc::clone(&w.holder), Arc::clone(&w.table), self.events.shared_sender());
        let walked_at = Arc::clone(&w.walked);
        let generation = lock(&holder).generation;
        let answer = Arc::clone(&gate);
        self.events.shared_jobs().spawn(move || {
            if let Some(b) = before {
                let _ = b.wait(crate::clock::now(), BEFORE_WAIT);
            }
            // (the servers' folders each with its whole list, there or not — by real path, in order: a root's own
            // first; a registration's base holds none. Was `tree_left_out` filtered by `is_dir`: a fresh clone's
            // `target`, made by the first build, was left out of nothing until the next roots change.)
            let mut lists: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
            for (r, list) in served.iter().zip(walked(&walked_at, &served)) {
                lists.entry(real(r)).or_default().extend(list);
            }
            let roots = minimal(wanted.iter().map(|r| real(r)).collect(), &lists);
            let leave: Vec<PathBuf> = lists.into_values().flatten().collect();
            let running = lock(&holder).watch.clone();
            let r = match running {
                Some(watch) => watch.set(roots, leave),
                None => {
                    let keep_at = Arc::clone(&lock(&table));
                    let split = Arc::clone(&table);
                    let out: Out = Box::new(move |said: Said| {
                        let per = Arc::clone(&lock(&split)).split(&said.batch);
                        if !per.is_empty() {
                            let _ = tx.send(Event::Job(Box::new(move |ed: &mut Editor| ed.lsp_watched(per))));
                        }
                    });
                    let given = Given { roots, leave_out: leave, keep: keep_at.keep() };
                    start(given, out).map(|watch| {
                        let mut h = lock(&holder);
                        if h.generation == generation {
                            // (a registration that came while it started: kept from now on)
                            let now = Arc::clone(&lock(&table));
                            if !Arc::ptr_eq(&now, &keep_at) {
                                let _ = watch.keep(now.keep());
                            }
                            h.watch = Some(watch);
                        }
                    })
                }
            };
            answer.fill(r.clone());
            move |ed: &mut Editor| {
                if let Err(e) = r {
                    crate::log::line("lsp", "watch", &format!("the tree watch: {e}"));
                }
                ed.unchanged = true;
            }
        });
        Some(gate)
    }

    /// Server `cid` answered `initialize`: one its writer thread didn't tell tarae watches isn't the tree's.
    pub(crate) fn lsp_watch_initialized(&mut self, cid: ClientId) {
        let undeclared = self.client(cid).and_then(|c| c.watched.get()).is_some_and(|w| !w.declared);
        if undeclared && self.lsp.watch.servers.remove(&cid).is_some() {
            self.lsp_watch_request(&[]);
        }
    }

    /// `client/registerCapability` from `cid`: its watched files' registrations kept — when it was told tarae
    /// watches; else answered and kept out, said once.
    pub(crate) fn lsp_watch_register(&mut self, cid: ClientId, params: &Value) {
        let regs: Vec<&Value> = params["registrations"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["method"] == METHOD)
            .collect();
        let Some(c) = self.client(cid).filter(|_| !regs.is_empty()) else { return };
        let name = c.name.clone();
        let Some(w) = c.watched.get().filter(|w| w.declared).cloned() else {
            let said = "it registered watched files without being told tarae watches: it watches them itself";
            return self.lsp.watch.say_once(&name, said.to_string());
        };
        let roots: Vec<(PathBuf, PathBuf)> = c.roots.iter().cloned().zip(w.real.iter().cloned()).collect();
        let mut table = (*self.lsp.watch.table()).clone();
        let server = table
            .servers
            .entry(cid)
            .or_insert_with(|| Server::new(roots, w.left_out.clone(), w.outputs.clone()));
        let mut read = Read::default();
        for r in regs {
            let id = r["id"].as_str().unwrap_or_default();
            let one = server.register(id, &r["registerOptions"]);
            read.refused.extend(one.refused);
            read.bad.extend(one.bad);
        }
        for b in read.refused {
            let said =
                format!("not watched, outside the folders it serves and its project's output folders: {b}");
            self.lsp.watch.say_once(&name, said);
        }
        for b in read.bad {
            self.lsp.watch.say_once(&name, format!("a watched-files pattern that doesn't read: {b}"));
        }
        self.lsp_watch_table(table);
    }

    /// `client/unregisterCapability` from `cid` (`unregisterations`, as LSP spells it).
    pub(crate) fn lsp_watch_unregister(&mut self, cid: ClientId, params: &Value) {
        let list = params["unregisterations"].as_array().or_else(|| params["unregistrations"].as_array());
        let ids: Vec<&str> = list
            .into_iter()
            .flatten()
            .filter(|u| u["method"] == METHOD)
            .filter_map(|u| u["id"].as_str())
            .collect();
        let mut table = (*self.lsp.watch.table()).clone();
        let Some(s) = table.servers.get_mut(&cid).filter(|_| !ids.is_empty()) else { return };
        for id in ids {
            s.unregister(id);
        }
        self.lsp_watch_table(table);
    }

    /// Server `cid` gone: its registrations and its folders go — the tree's roots and what it keeps shrink; a
    /// folder no other watched server has is asked its output folders and walked again next time (the build
    /// folder's setting may have changed for a restart, a checkout been made inside it).
    pub(crate) fn lsp_watch_gone(&mut self, cid: ClientId) {
        let gone = self.lsp.watch.servers.remove(&cid);
        let had = gone.is_some();
        let w = &self.lsp.watch;
        for r in gone.iter().flatten().filter(|r| !w.servers.values().any(|s| s.contains(r))) {
            lock(&w.asked_outputs).remove(r);
            lock(&w.walked).remove(r);
        }
        let mut table = (*self.lsp.watch.table()).clone();
        if table.servers.remove(&cid).is_some() {
            self.lsp_watch_table(table);
        } else if had {
            self.lsp_watch_request(&[]);
        }
    }

    /// Server `cid` serves fewer folders (`share_leave`): what it's told and the tree's roots follow.
    pub(crate) fn lsp_watch_roots(&mut self, cid: ClientId, roots: &[PathBuf]) {
        let Some(had) = self.lsp.watch.servers.get_mut(&cid) else { return };
        *had = roots.to_vec();
        let mut table = (*self.lsp.watch.table()).clone();
        if let Some(s) = table.servers.get_mut(&cid) {
            s.roots.retain(|(given, _)| roots.contains(given));
            let real: Vec<PathBuf> = s.real_roots();
            s.left_out.retain(|l| real.iter().any(|r| l.starts_with(r)));
        }
        self.lsp_watch_table(table);
    }

    /// A new table: the worker's from now on, the tree's roots following its own roots, what it keeps sent.
    fn lsp_watch_table(&mut self, table: Table) {
        let table = Arc::new(table);
        *lock(&self.lsp.watch.table) = Arc::clone(&table);
        self.lsp_watch_request(&[]);
        let watch = lock(&self.lsp.watch.holder).watch.clone();
        if let Some(w) = watch {
            let _ = w.keep(table.keep());
        }
    }

    /// A batch split on the worker: one `workspace/didChangeWatchedFiles` a server, serialized on its writer
    /// thread — the main loop only pushes.
    pub(crate) fn lsp_watched(&mut self, per: Vec<(ClientId, Vec<Value>)>) {
        for (cid, changes) in per {
            let Some(c) = self.client_mut(cid) else { continue };
            c.notify_lazy(move || {
                json!({ "jsonrpc": "2.0", "method": METHOD, "params": { "changes": changes } }).to_string()
            });
            #[cfg(test)]
            {
                self.lsp.watch.pushes += 1;
            }
            // (tarae's own and a test's stand-in: answered on a worker)
            self.lsp_own_serve(cid);
        }
        self.unchanged = true;
    }

    /// Paths tarae knows were written (its own writes, a run's end, git's, focus regained): the tree watch looks
    /// at them whatever its stream said — a file compared by its stamp, a folder compared. Their real paths are
    /// found on a worker; the look is a send.
    pub(crate) fn lsp_look(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let Some(w) = lock(&self.lsp.watch.holder).watch.clone() else { return };
        self.events.jobs().background(move || {
            let _ = w.look(paths.iter().map(|p| real(p)).collect());
        });
    }

    /// Every root the tree watches, looked at (focus regained).
    pub(crate) fn lsp_look_roots(&self) {
        self.lsp_look(self.lsp.watch.asked.clone());
    }
}

#[cfg(test)]
pub(crate) mod tests;
