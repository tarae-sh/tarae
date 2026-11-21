//! A watch over whole trees: which of the files worth keeping below some folders were made, written or removed,
//! said in batches — what a language server asks its client to watch. Core and language-neutral: it's given the
//! roots, the folders left out and what to keep, and knows nothing else.
//!
//! macOS: FSEvents streams with file events, by direct FFI to CoreServices — as many as the folders left out need
//! (`groups`: one a root with folders left out, its first `EXCLUDED_MAX` its exclusion paths, and one shared by the
//! roots below a folder left out that leave out none of their own — a build script's out dir); an exclusion path
//! may be one not there yet (a fresh clone's `target`): FSEvents honors it once it's made (W1b step 3's second card,
//! macOS 27) — where one wouldn't, what's below it is dropped by prefix on the worker until the next roots change
//! starts the stream with it there;
//! their callbacks only hand the events to the one worker (`tarae-treewatch`). As in `fswatch`, an event only says
//! "look now": the worker stats the path and decides by a table of the kept files below the roots, each with its stamp — gone and
//! held: deleted; there and not held: created; there and held: changed, whatever the stamp says (a direct event is
//! never talked out of by a stamp) — but the first event to name a file a compare put in the table, the stamp the
//! same, within `MARKED_FOR` of that compare, takes the mark and says nothing (`Held::unnamed`): it's the make the
//! compare already said, its event landed in a later burst (a checkout's, split at `BURST_MAX`). A look naming a
//! file is a compare of it: changed only where its stamp moved. But an event FSEvents says is of a plain file, whose
//! path no server keeps and the table holds nothing at or below, costs no stat: dropped, counted (`Tree::unwanted` — a build's `.o`s in a
//! folder nothing leaves out). A folder's event, one flagged "must scan below", or the flags for dropped
//! events: that folder (or every root) is walked and compared with the table by stamp — a folder moved in with
//! files in it is one event, for the folder; one moved away is known only from the table. A file is said changed
//! only for a reason the tree can name (`Why`): an event named it, its stamp moved, or events were lost over it
//! while its stamp was too fresh to tell (`disk::sure`) — and once, for a file no event ever stood for, when its
//! stamp can first be judged (`Held::owed`). So a write that keeps the very stamp (the same size, inside the clock's
//! grain of the last one seen) and whose event FSEvents lost without a flag isn't seen, by a look either: the known
//! rest — as one whose own event comes first, the make's lost, which the mark takes. The streams start before the
//! table's first walk, so a change made while it walks is said after (armed before the look: its stamp too fresh to
//! be sure; the fill marks nothing). Said twice is harmless (a server reads the file again); missed leaves it
//! wrong, silently.
//!
//! A path is watched when the deepest folder above it, among the roots and the folders left out, is a root — so a
//! root below a folder left out (a build script's output below `target`, a checkout nested in one) is watched.
//! What reaches the worker below a folder left out is dropped there by prefix — the last line, past the streams'
//! exclusion paths; only a root with more than `EXCLUDED_MAX` folders left out below it should need it.
//! `TreeWatch::look` is the backstop's seam: paths looked at whatever the streams said, through the same burst.
//! A rename that only changes case, split across two bursts, is accepted: the old name is said changed and stays
//! until a compare of its folder — the LSP side's looks compare the root and put it right.
//!
//! Links to folders aren't followed — by the walk, as by FSEvents, which names the link and never what's below its
//! target: a tree reached only through one isn't watched. A link to a file is kept under the link's path, but
//! FSEvents names its target: a change to a target outside the tree isn't said for the link — only a compare
//! catches it (its stamp is the target's). Elsewhere than macOS the start says no.

// (the worker is macOS's; elsewhere only its decisions are built, for their tests, and the start says no)
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant, SystemTime};

use crate::disk::Stamp;
use crate::fswatch::{BURST_MAX, QUIET};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Created,
    Changed,
    Deleted,
}

/// The files worth remembering, by path (W1b: the union of the servers' globs).
pub type Keep = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

/// What a batch says: kept files only, each path once.
pub type Batch = Vec<(PathBuf, Kind)>;

/// A settled burst: its batch, never empty. (What a look's compare found changed with no event naming it, its stamp
/// older than `MISSED_AFTER` — the stream missed it, or a copy kept its mtime: `cp -p`, `tar x` — is the worker's
/// own running total, `Tree.missed`, logged as it doubles.)
pub struct Said {
    pub batch: Batch,
    /// Tests: what each word of `batch` stood on, in its order (#502: a changed is judged by its reason).
    #[cfg(test)]
    pub(crate) record: Vec<Option<Record>>,
}

/// A change a look's compare finds with a stamp at least this old is one the stream should have said by then.
const MISSED_AFTER: Duration = Duration::from_secs(5);

/// How long a mark (`Held::unnamed`) is honored after the compare that set it: a few bursts (`BURST_MAX`) — long
/// enough for a make's own event split into a later one, short enough that where none is coming (events lost, a
/// root come back with files in it, a look's find) it can't swallow a later rewrite that keeps the stamp.
const MARKED_FOR: Duration = Duration::from_secs(5);

/// What's watched. Paths are real ones: FSEvents names `/private/tmp`, never `/tmp`.
pub struct Given {
    pub roots: Vec<PathBuf>,
    /// Folders below the roots left out (`.git`, the builds' outputs, the checkouts inside), there or not, in the
    /// order that decides a stream's places: exclusion paths of the streams (`groups`) — past a root's first
    /// `EXCLUDED_MAX`, dropped by prefix on the worker — as is what's below one made after its stream started, where
    /// FSEvents wouldn't honor it.
    pub leave_out: Vec<PathBuf>,
    pub keep: Keep,
}

/// FSEvents takes at most this many exclusion paths, a stream.
const EXCLUDED_MAX: usize = 8;

/// FSEvents' event flags read here (`kFSEventStreamEventFlag…`); the kind of a file's change is never read from
/// them, only from the stat and the table — the item's type only to skip a stat no server wants (`Tree::unwanted`).
mod flag {
    pub const MUST_SCAN_SUB_DIRS: u32 = 0x1;
    pub const USER_DROPPED: u32 = 0x2;
    pub const KERNEL_DROPPED: u32 = 0x4;
    pub const ROOT_CHANGED: u32 = 0x20;
    pub const MOUNT: u32 = 0x40;
    pub const UNMOUNT: u32 = 0x80;
    pub const ITEM_IS_FILE: u32 = 0x10000;
    pub const ITEM_IS_DIR: u32 = 0x20000;
    pub const ITEM_IS_SYMLINK: u32 = 0x40000;
    /// The flags that ask for more than one path's look: a scan, every root, a root's own change, a volume.
    pub const SCAN: u32 = MUST_SCAN_SUB_DIRS | USER_DROPPED | KERNEL_DROPPED | ROOT_CHANGED | MOUNT | UNMOUNT;
    /// tarae's own, above every flag FSEvents defines: a path `TreeWatch::look` named.
    pub const LOOKED: u32 = 1 << 31;
}

const GONE: &str = "the tree watch's worker is gone";

/// A watch running: dropped, its worker stops the streams and ends.
pub struct TreeWatch {
    tx: Sender<Msg>,
}

enum Msg {
    /// From the stream's callback: `(path, flags)` as FSEvents gave them.
    Events(Vec<(PathBuf, u32)>),
    /// The roots and the folders left out, together: a new stream for each group that changed, the rest go on.
    Set(Vec<PathBuf>, Vec<PathBuf>, Sender<Result<(), String>>),
    Keep(Keep),
    /// A test's question: the streams running and what was dropped by prefix.
    #[cfg(test)]
    Seen(Sender<Seen>),
    Stop,
}

/// What a test sees of a running watch: each stream running, by its number (the worker's count of the streams it
/// started) and its group; the events dropped by prefix (`Tree.unwatched`, and their paths) and with no stat
/// (`Tree.unkept`).
#[cfg(test)]
struct Seen {
    streams: Vec<(u64, Group)>,
    unwatched: usize,
    unwatched_paths: Vec<PathBuf>,
    unkept: usize,
}

impl TreeWatch {
    /// The watch started: the streams made and started — an answer from FSEvents' service, up to seconds when it's
    /// slow, so never on the main thread —, then the table filled on the worker. Each settled burst's batch goes to
    /// `out`, on the worker. No: why, in words for the log, and nothing left running.
    pub fn start(given: Given, out: impl FnMut(Said) + Send + 'static) -> Result<TreeWatch, String> {
        #[cfg(target_os = "macos")]
        return mac::start(given, Box::new(out));
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (given, out);
            Err(format!("no tree watch on {} yet", std::env::consts::OS))
        }
    }

    /// Other roots and folders left out, in one go: a new stream for each group that changed (`groups`), all started
    /// before the old ones stop — the others go on. A root may not be there yet. No: the old streams, roots and
    /// folders stay, all of them.
    pub fn set(&self, roots: Vec<PathBuf>, leave_out: Vec<PathBuf>) -> Result<(), String> {
        self.ask(|r| Msg::Set(roots, leave_out, r))
    }

    /// Other files kept: the table filled for the new matches on the worker, nothing said for them. A send, never
    /// a wait — a server's registration lands on the main loop.
    pub fn keep(&self, keep: Keep) -> Result<(), String> {
        self.tx.send(Msg::Keep(keep)).map_err(|_| GONE.to_string())
    }

    /// Paths looked at whatever the stream said (tarae's own writes, a run's end, git, focus regained): each goes
    /// through the burst as an event marked `flag::LOOKED` — a file compared by its stamp, a folder compared.
    pub fn look(&self, paths: Vec<PathBuf>) -> Result<(), String> {
        let events = paths.into_iter().map(|p| (p, flag::LOOKED)).collect();
        self.tx.send(Msg::Events(events)).map_err(|_| GONE.to_string())
    }

    fn ask(&self, msg: impl FnOnce(Sender<Result<(), String>>) -> Msg) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.tx.send(msg(tx)).map_err(|_| GONE.to_string())?;
        rx.recv().map_err(|_| GONE.to_string())?
    }

    /// The streams running and what was dropped by prefix, as the worker has them once the messages before are
    /// handled (a burst still under way not yet settled: its drops not counted).
    #[cfg(test)]
    fn seen(&self) -> Seen {
        let (tx, rx) = mpsc::channel();
        self.tx.send(Msg::Seen(tx)).expect(GONE);
        rx.recv().expect(GONE)
    }
}

impl Drop for TreeWatch {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
    }
}

/// A burst of events under way: settled after `QUIET` with none, or `BURST_MAX` after its first (`fswatch`'s).
#[derive(Default)]
struct Burst {
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Burst {
    fn add(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// How long until it settles — zero: it has; none: no burst is under way.
    fn left(&self, now: Instant) -> Option<Duration> {
        let end = (self.last? + QUIET).min(self.first? + BURST_MAX);
        Some(end.saturating_duration_since(now))
    }
}

/// A kept file as last seen: its stamp, whether that stamp tells every later write apart (`disk::sure`), and whether
/// it's owed a word — no event stood for it (the start's fill, a compare's own find) and it was too fresh to judge:
/// said changed once, at the first compare that can judge it (#286's care, once — `compare`).
/// And the mark (`unnamed`): a compare put this stamp in the table and no event has named the file since: the server
/// was already told, and the event for it is still on its way — the first event that does, while the stamp is the
/// same, is the make or write the compare already said, landed in a later burst; it takes the mark and says nothing
/// (W1b step 3, round 7; `look`). Held as when the compare set it, and honored only for `MARKED_FOR` after: where
/// the event never comes (events lost, a root come back, a look's find), a later event with the stamp the same is a
/// rewrite that keeps mtime and size (a reproducible build's), said changed (calls 7).
/// The fill marks nothing, sure or not: it told the server nothing, and a sure entry has no event coming — an event
/// naming one is a write (a reproducible build's rewrite keeps the very mtime and size), said changed.
#[derive(Clone, Copy, Debug)]
struct Held {
    stamp: Stamp,
    sure: bool,
    owed: bool,
    unnamed: Option<Instant>,
    /// Tests: who put this stamp in the table (a compare that saw it unmoved keeps the one before).
    #[cfg(test)]
    put: Option<Put>,
}

/// Tests: who put a held stamp in the table — an event, a compare (a look's too), the fill —, when, and in which
/// step (`Tree.steps`).
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Put {
    by: By,
    at: Instant,
    step: u64,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum By {
    Event,
    Compare,
    Fill,
}

/// Tests: what a said path's word stood on (#502) — the step's `Why`, the entry as held before (its stamp, sureness,
/// debt, mark and who put it), the stamp now (none: gone), the stamp's age at the stat and the mark's then.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Record {
    why: Why,
    step: u64,
    held: Option<Held>,
    stamp: Option<Stamp>,
    age: Option<Duration>,
    mark: Option<Duration>,
}

/// Tests: the reasons a file may be said changed, each one the record proves (#502's calls).
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    /// 1. A lost-events compare (`Why::Lost`), the held stamp not sure.
    Lost,
    /// 2. An event past its mark's `MARKED_FOR`, the stamp the same.
    PastMark,
    /// 3. A look's compare judging a debt (`Held::owed`) past `disk::RACY`, put by no event.
    Debt,
    /// 4. An event on an entry an event put in an earlier step, the stamp the same: a make split by a burst's end —
    ///    or a rewrite that kept size and mtime, which only an event tells (never talked out of by a stamp).
    Named,
    /// 5. The stamp moved since the entry was put.
    Moved,
}

#[cfg(test)]
impl Record {
    /// Why a changed word was said, if the record proves a reason — none: round 5's bug, a changed with no reason.
    pub(crate) fn reason(&self) -> Option<Reason> {
        let (h, stamp) = (self.held?, self.stamp?);
        let by = |by: By| h.put.is_some_and(|p| p.by == by && p.step < self.step);
        match self.why {
            _ if h.stamp != stamp => Some(Reason::Moved),
            Why::Lost if !h.sure => Some(Reason::Lost),
            Why::Event if h.unnamed.is_some() && self.mark.is_some_and(|m| m >= MARKED_FOR) => {
                Some(Reason::PastMark)
            }
            Why::Event if by(By::Event) => Some(Reason::Named),
            Why::Look
                if h.owed
                    && (by(By::Compare) || by(By::Fill))
                    && self.age.is_some_and(|a| a >= crate::disk::RACY) =>
            {
                Some(Reason::Debt)
            }
            _ => None,
        }
    }

    /// For a failure's message: the reason, or "no reason", and what it stood on.
    pub(crate) fn line(&self) -> String {
        let reason = self.reason().map_or("no reason".to_string(), |r| format!("{r:?}"));
        let ms = |d: Option<Duration>| d.map_or("—".to_string(), |d| format!("{}ms", d.as_millis()));
        let size = |s: Option<Stamp>| s.map_or("—".to_string(), |s| s.1.to_string());
        let held = self.held;
        let put = held.and_then(|h| h.put);
        format!(
            "{reason}: by {:?} in step {}; put by {:?} in step {:?}; stamp {} (size {} → {}), {} old at the stat; \
             held sure {:?}, owed {:?}; mark {} old",
            self.why,
            self.step,
            put.map(|p| p.by),
            put.map(|p| p.step),
            if held.map(|h| h.stamp) == self.stamp { "the same" } else { "moved" },
            size(held.map(|h| h.stamp)),
            size(self.stamp),
            ms(self.age),
            held.map(|h| h.sure),
            held.map(|h| h.owed),
            ms(self.mark),
        )
    }
}

impl Held {
    /// Marked, and the mark still honored at `now`.
    fn marked(&self, now: Instant) -> bool {
        self.unnamed.is_some_and(|at| now.saturating_duration_since(at) < MARKED_FOR)
    }
}

/// Why a path is said — what decides whether a compare says a file changed whose stamp didn't move, and where a
/// batch came from in its log line (`said_files`). A file is said changed only for a reason the tree can name: an
/// event named it, its stamp moved, or events were lost over it while its stamp was too fresh to tell (W1b step 3,
/// round 5: never because a second pass came by within `disk::RACY`). In the order a folder's compares fold into
/// the outermost (`step`): the most careful first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Why {
    /// Events were lost over it ("must scan below", the dropped flags): a held file whose stamp can't tell a write
    /// of the same size apart (not `sure`) is said changed.
    Lost,
    /// A folder's own event, a root's change, a case rename, a root come: changed only where the stamp moved.
    Folder,
    /// A file's own event: the stat and the table decide (`look`) — never a compare's.
    Event,
    /// `TreeWatch::look`: as a compare for a file (changed only where its stamp moved), as a folder's for a folder;
    /// what its compare finds that no event said, old, is counted missed.
    Look,
}

impl Why {
    /// How a folder an event (or a look) named is compared.
    fn folded(self) -> Why {
        match self {
            Why::Event => Why::Folder,
            why => why,
        }
    }
}

/// What a step has said so far: each path once, its kind and why (the first said wins).
type Saying = BTreeMap<PathBuf, (Kind, Why)>;

/// A root watched, and which folder it is: one renamed away and another made in its place isn't it. None: not
/// there (yet, or any more) — still a root, compared when it's there again.
struct Root {
    path: PathBuf,
    id: Option<(u64, u64)>,
}

/// The roots given, each a local folder or not there — or why not, for the first that isn't (all or nothing).
fn roots_of(paths: &[PathBuf]) -> Result<Vec<Root>, String> {
    paths.iter().map(|p| root_of(p)).collect()
}

/// A root: a folder, or nothing there yet (a build script's out dir before the build, after `cargo clean`) —
/// refused when what's there isn't a folder, or on a network file system, judged at the nearest folder that is.
fn root_of(p: &Path) -> Result<Root, String> {
    let id = folder_id(p);
    if id.is_none() && std::fs::symlink_metadata(p).is_ok() {
        return Err(format!("{} isn't a folder", p.display()));
    }
    let at = p.ancestors().find(|a| std::fs::symlink_metadata(a).is_ok()).unwrap_or(p);
    if !crate::fswatch::is_local(at) {
        return Err(format!(
            "{} is on a network file system: other machines' writes there aren't told",
            p.display()
        ));
    }
    Ok(Root { path: p.to_path_buf(), id })
}

/// Whether `root` can be watched (a local folder, there now: a server's own) — or why not: one bad root left
/// out, the others kept.
pub fn watchable(root: &Path) -> Result<(), String> {
    folder_id(root).ok_or_else(|| format!("{} isn't a folder", root.display()))?;
    root_of(root).map(|_| ())
}

/// Whether the watch would take these roots (a test's fake refuses what the tree refuses).
#[cfg(test)]
pub(crate) fn roots_taken(roots: &[PathBuf]) -> Result<(), String> {
    roots_of(roots).map(|_| ())
}

fn folder_id(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::symlink_metadata(path).ok().filter(|m| m.is_dir())?;
    Some((m.dev(), m.ino()))
}

/// One FSEvents stream's share: its roots, and the folders below them it leaves out (its exclusion paths).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Group {
    roots: Vec<PathBuf>,
    exclude: Vec<PathBuf>,
}

/// The streams the roots need: one a root with folders left out below it, and one for all the nested roots with
/// none — so a root's stream depends on that root alone, and `set` restarts only the streams of the roots that
/// changed (the design session, 2026-11-11: no packing, which no order keeps stable):
/// - a root's stream leaves out the folders left out below it, but one below another of them (it leaves out nothing
///   more), its first `EXCLUDED_MAX` — past them, dropped by prefix on the worker (its path in the second list, said
///   once). A root below a folder left out (a checkout nested in `worktrees`) with folders left out of its own is
///   such a root too: its own `target` is its stream's exclusion, never a flood dropped by prefix;
/// - a root below a folder left out with none of its own (a build script's out dir below `target`) is *shared*:
///   all of them share one stream, with no exclusions — an exclusion path above a root silences it, even named to
///   the same stream (W1a §8) — so the folder stays an exclusion of the stream of the root it's below, and nothing
///   below it reaches tarae; when the shared roots change, that one stream restarts.
fn groups(roots: &[PathBuf], leave_out: &[PathBuf]) -> (Vec<Group>, Vec<PathBuf>) {
    let nested = |r: &PathBuf| {
        leave_out.iter().any(|l| r.starts_with(l) && roots.iter().any(|q| l.starts_with(q) && l != q))
    };
    let leaves_out = |r: &PathBuf| leave_out.iter().any(|l| l.starts_with(r) && l != r);
    let (own, plain): (Vec<&PathBuf>, Vec<&PathBuf>) =
        roots.iter().partition(|r| nested(r) && !leaves_out(r));
    let mut over = Vec::new();
    let mut groups: Vec<Group> = (plain.into_iter())
        .map(|r| {
            // (a nested root's own folders lie below the folder the outer root leaves out: they take none of the
            // outer stream's places)
            let below: Vec<&PathBuf> = leave_out.iter().filter(|l| l.starts_with(r) && *l != r).collect();
            let mut ex: Vec<&PathBuf> =
                below.iter().copied().filter(|l| !below.iter().any(|m| m != l && l.starts_with(m))).collect();
            if ex.len() > EXCLUDED_MAX {
                over.push(r.clone());
                ex.truncate(EXCLUDED_MAX);
            }
            Group { roots: vec![r.clone()], exclude: ex.into_iter().cloned().collect() }
        })
        .collect();
    if !own.is_empty() {
        groups.push(Group { roots: own.into_iter().cloned().collect(), exclude: Vec::new() });
    }
    (groups, over)
}

/// One path's two events as one: their flags together, a look's mark kept only when both were looks — a path the
/// stream named too is the stream's.
fn merged(a: u32, b: u32) -> u32 {
    ((a | b) & !flag::LOOKED) | (a & b & flag::LOOKED)
}

/// The folder two paths share, by their components as written: for two that differ only by case, the one above
/// where they part.
fn shared_folder(a: &Path, b: &Path) -> PathBuf {
    a.components().zip(b.components()).take_while(|(x, y)| x == y).map(|(x, _)| x).collect()
}

/// The worker's state — what's watched and the table of kept files — and every decision, with no stream: its
/// tests hand events in.
struct Tree {
    roots: Vec<Root>,
    leave_out: Vec<PathBuf>,
    keep: Keep,
    table: BTreeMap<PathBuf, Held>,
    /// Files a look's compare found changed with no event naming them, and their stamps old (`Said::missed`): the
    /// running total, logged each time it doubles.
    missed: usize,
    /// Events for paths not watched, dropped by prefix — below a folder no stream leaves out (past a root's first
    /// `EXCLUDED_MAX`), or of a root no longer watched: the running total, logged each time it doubles.
    unwatched: usize,
    /// Events for plain files no server keeps, the table holding nothing at or below them, dropped with no stat
    /// (`unwanted`): the running total, logged with `unwatched` each time their sum doubles.
    unkept: usize,
    /// Whether a `keep` change made the table pass `MANY` files — said once.
    many_said: bool,
    /// The last step's batch, how many said for each `Why` (by its order): the batch line's "said by".
    by: [usize; 4],
    /// The last step's events that took the mark (`Held::unnamed`), their flags: said in the log (`marked_line`).
    marked: Vec<u32>,
    /// The last step's files an event said changed with the stamp the same, no mark honored: past its mark's
    /// `MARKED_FOR`, and with none (an entry an event or the fill put: a make split by a burst's end, a rewrite that
    /// kept size and mtime — a build's generated files) — the batch line's count, before any rule moves (#502).
    unmoved: [usize; 2],
    /// Tests: the steps so far — the record's (`Put`, `Record`) "which step".
    #[cfg(test)]
    steps: u64,
    /// Tests: the last step's record, by path (the first said wins, as in `Saying`).
    #[cfg(test)]
    record: BTreeMap<PathBuf, Record>,
    /// Tests: the stats `look` made.
    #[cfg(test)]
    stats: usize,
    /// Tests: the paths checked against the roots and what's left out (`watched`).
    #[cfg(test)]
    checked: std::cell::Cell<usize>,
    /// Tests: the paths dropped by prefix (`unwatched`), one per dropped event.
    #[cfg(test)]
    unwatched_paths: Vec<PathBuf>,
}

/// A table past this many files after a `keep` change is said in the log, once: a glob that keeps everything
/// isn't capped, only seen.
const MANY: usize = 100_000;

impl Tree {
    fn new(given: Given) -> Result<Tree, String> {
        let roots = roots_of(&given.roots)?;
        let (leave_out, keep) = (given.leave_out, given.keep);
        Ok(Tree {
            roots,
            leave_out,
            keep,
            table: BTreeMap::new(),
            missed: 0,
            unwatched: 0,
            unkept: 0,
            many_said: false,
            by: [0; 4],
            marked: Vec::new(),
            unmoved: [0; 2],
            #[cfg(test)]
            steps: 0,
            #[cfg(test)]
            record: BTreeMap::new(),
            #[cfg(test)]
            stats: 0,
            #[cfg(test)]
            checked: Default::default(),
            #[cfg(test)]
            unwatched_paths: Vec::new(),
        })
    }

    fn root_paths(&self) -> Vec<PathBuf> {
        self.roots.iter().map(|r| r.path.clone()).collect()
    }

    /// The deepest folder above `path` (or it), among the roots and the folders left out, is a root — a tie a
    /// root's.
    fn watched(&self, path: &Path) -> bool {
        #[cfg(test)]
        self.checked.set(self.checked.get() + 1);
        let deepest = |folders: &mut dyn Iterator<Item = &PathBuf>| {
            folders.filter(|f| path.starts_with(f)).map(|f| f.components().count()).max()
        };
        let root = deepest(&mut self.roots.iter().map(|r| &r.path));
        let out = deepest(&mut self.leave_out.iter());
        root.is_some_and(|r| out.is_none_or(|o| r >= o))
    }

    /// The roots strictly below `dir` that a walk from it wouldn't reach: the folder above each isn't watched.
    fn roots_within(&self, dir: &Path) -> Vec<PathBuf> {
        (self.roots.iter().map(|r| &r.path))
            .filter(|r| r.starts_with(dir) && *r != dir && !r.parent().is_some_and(|p| self.watched(p)))
            .cloned()
            .collect()
    }

    /// The table's paths no longer watched, dropped unsaid (not deleted, only no longer watched).
    fn drop_unwatched(&mut self) {
        let gone: Vec<PathBuf> = self.table.keys().filter(|p| !self.watched(p)).cloned().collect();
        for p in gone {
            self.table.remove(&p);
        }
    }

    /// The kept files below `dirs` that the table doesn't hold yet, put in it unsaid — what's there as the watch
    /// starts, or as a root, a folder or a match comes in; one too fresh to judge is owed (`Held::owed`); none is
    /// marked (`Held::unnamed`). The links to folders met (not followed).
    fn fill(&mut self, dirs: &[PathBuf]) -> usize {
        let taken = crate::clock::system();
        #[cfg(test)]
        let put = Some(Put { by: By::Fill, at: crate::clock::now(), step: self.steps });
        let (mut found, mut links) = (BTreeMap::new(), 0);
        for d in dirs {
            self.walk(d, &mut found, &mut links);
        }
        for (p, stamp) in found {
            let sure = crate::disk::sure(&stamp, taken);
            // (not marked: the fill told the server nothing — an event naming the file says it changed)
            let held = Held {
                stamp,
                sure,
                owed: !sure,
                unnamed: None,
                #[cfg(test)]
                put,
            };
            self.table.entry(p).or_insert(held);
        }
        links
    }

    /// The kept files below `dir` with their stamps, the folders left out skipped — but the roots below them.
    fn walk(&self, dir: &Path, found: &mut BTreeMap<PathBuf, Stamp>, links: &mut usize) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let (path, Ok(ty)) = (e.path(), e.file_type()) else { continue };
            if ty.is_dir() {
                if self.watched(&path) {
                    self.walk(&path, found, links);
                } else {
                    for r in self.roots_within(&path) {
                        self.walk(&r, found, links);
                    }
                }
                continue;
            }
            if !(self.keep)(&path) && !ty.is_symlink() {
                continue;
            }
            // (a link: what it names when that's a file — a link to a folder isn't followed)
            let m = if ty.is_symlink() { std::fs::metadata(&path) } else { e.metadata() };
            let Ok(m) = m else { continue };
            if m.is_dir() {
                *links += 1;
            } else if (self.keep)(&path) {
                found.insert(path, stamp_of(&m));
            }
        }
    }

    /// The table's paths strictly below `dir`.
    fn below(&self, dir: &Path) -> Vec<PathBuf> {
        use std::ops::Bound::{Excluded, Unbounded};
        (self.table.range::<Path, _>((Excluded(dir), Unbounded)))
            .map(|(p, _)| p)
            .take_while(|p| p.starts_with(dir))
            .cloned()
            .collect()
    }

    /// Whether the table holds a path strictly below `dir`.
    fn holds_below(&self, dir: &Path) -> bool {
        use std::ops::Bound::{Excluded, Unbounded};
        (self.table.range::<Path, _>((Excluded(dir), Unbounded)))
            .next()
            .is_some_and(|(p, _)| p.starts_with(dir))
    }

    /// An event FSEvents says is of a plain file — `ITEM_IS_FILE`, and nothing that says a folder or a link, asks
    /// for a scan or marks a look — whose path no server keeps, the table holding nothing at or below it: nothing
    /// a stat could change (a folder would be flagged one; a file no server keeps is never said), so it isn't made.
    fn unwanted(&self, path: &Path, flags: u32) -> bool {
        flags & flag::ITEM_IS_FILE != 0
            && flags & (flag::ITEM_IS_DIR | flag::ITEM_IS_SYMLINK | flag::SCAN | flag::LOOKED) == 0
            && !(self.keep)(path)
            && !self.table.contains_key(path)
            && !self.holds_below(path)
    }

    /// One settled burst's events, each `(path, flags)` as FSEvents gave them, looked at against the file system as
    /// it is now: what changed among the kept files, each path once.
    /// A settled burst said — none when nothing changed: its batch, and what a look found the stream missed, its
    /// running total logged the first time and each time it doubles (no toast: a copy that kept its mtime reads as
    /// missed too).
    fn settle(&mut self, events: &[(PathBuf, u32)]) -> Option<Said> {
        let (before, dropped) = (self.missed, self.unwatched + self.unkept);
        let batch = self.step(events);
        if !self.marked.is_empty() {
            crate::log::line("tree-watch", "info", &marked_line(&self.marked));
        }
        if doubled(dropped, self.unwatched + self.unkept) {
            let what = format!(
                "{} events dropped unlooked at: {} by prefix (below a folder no stream leaves out, or of a root no \
                 longer watched), {} of files no server keeps (said again when the total doubles)",
                self.unwatched + self.unkept,
                self.unwatched,
                self.unkept
            );
            crate::log::line("tree-watch", "info", &what);
        }
        if doubled(before, self.missed) {
            let what = format!(
                "looks have found {} files changed that no event named, their stamps over {} old: FSEvents \
                 missed them, or a copy kept its mtime (said again when the total doubles)",
                self.missed,
                crate::term::run_duration(MISSED_AFTER)
            );
            crate::log::line("tree-watch", "warn", &what);
        }
        #[cfg(test)]
        let record = batch.iter().map(|(p, _)| self.record.remove(p)).collect();
        (!batch.is_empty()).then_some(Said {
            batch,
            #[cfg(test)]
            record,
        })
    }

    /// The batch; what it said by each `Why` left in `by`.
    fn step(&mut self, events: &[(PathBuf, u32)]) -> Batch {
        let mut said = Saying::new();
        self.marked.clear();
        self.unmoved = [0; 2];
        #[cfg(test)]
        {
            self.steps += 1;
            self.record.clear();
        }
        let (mut files, mut scans, mut whole) = (Vec::new(), Vec::new(), false);
        let by_look = |looked: bool| if looked { Why::Look } else { Why::Folder };
        // (a root that wasn't there and is now, named by any event or look at or below it: compared, its files created)
        for r in self.roots.iter_mut().filter(|r| r.id.is_none()) {
            let named = events.iter().filter(|(p, _)| p.starts_with(&r.path));
            let looked = named.clone().all(|(_, f)| f & flag::LOOKED != 0);
            if named.count() > 0
                && let Some(id) = folder_id(&r.path)
            {
                r.id = Some(id);
                scans.push((r.path.clone(), by_look(looked)));
            }
        }
        for (path, flags) in events {
            if flags & (flag::USER_DROPPED | flag::KERNEL_DROPPED) != 0 {
                whole = true;
            } else if flags & flag::ROOT_CHANGED != 0 {
                self.root_changed(path, &mut said);
            } else if flags & flag::MUST_SCAN_SUB_DIRS != 0 {
                scans.push((path.clone(), Why::Lost));
            } else {
                files.push((path.clone(), *flags));
            }
        }
        // (one path's events as one: `merged`)
        crate::sorting::sort(&mut files);
        files.dedup_by(|b, a| {
            let same = b.0 == a.0;
            if same {
                a.1 = merged(a.1, b.1);
            }
            same
        });
        // (an `unwanted` file first: dropped the same wherever it lies, so never checked against the roots and what's
        // left out — kept in the list until the case renames have seen it, a partner of a kept one; the rest below a
        // root — one that went in this burst takes its events with it — and not left out: the prefix drop)
        let named = files.len();
        let mut files: Vec<(PathBuf, u32, bool)> = (files.into_iter())
            .filter_map(|(p, f)| {
                let unwanted = self.unwanted(&p, f);
                if unwanted || self.watched(&p) {
                    Some((p, f, unwanted))
                } else {
                    #[cfg(test)]
                    self.unwatched_paths.push(p);
                    None
                }
            })
            .collect();
        self.unwatched += named - files.len();
        self.case_renames(&mut files, &mut scans);
        for (path, flags, unwanted) in files {
            if unwanted {
                self.unkept += 1;
            } else {
                let why = if flags & flag::LOOKED != 0 { Why::Look } else { Why::Event };
                if self.look(&path, why, &mut said, &mut scans) {
                    self.marked.push(flags);
                }
            }
        }
        if whole {
            scans = self.root_paths().into_iter().map(|r| (r, Why::Lost)).collect();
        }
        // (one not watched: the roots below it, else nothing — a root's own or the old stream's)
        let mut scans: Vec<(PathBuf, Why)> = (scans.into_iter())
            .flat_map(|(p, why)| match self.watched(&p) {
                true => vec![(p, why)],
                false => self.roots_within(&p).into_iter().map(|r| (r, why)).collect(),
            })
            .collect();
        // (sorted, a folder's own come right after it: walked once, by the outermost — as careful as the most careful
        // folded into it, the stream's before a look's)
        crate::sorting::sort(&mut scans);
        scans.dedup_by(|inner, outer| {
            let within = inner.0.starts_with(&outer.0);
            if within {
                outer.1 = outer.1.min(inner.1);
            }
            within
        });
        for (dir, why) in scans {
            self.compare(&dir, why, &mut said);
        }
        self.by = [0; 4];
        for (_, why) in said.values() {
            self.by[*why as usize] += 1;
        }
        said.into_iter().map(|(p, (kind, _))| (p, kind)).collect()
    }

    /// Two names in one burst that differ only by case — a rename on a case-insensitive volume, where one stat finds
    /// the file under both — leave the files for the folder above where they part, compared.
    fn case_renames(&self, files: &mut Vec<(PathBuf, u32, bool)>, scans: &mut Vec<(PathBuf, Why)>) {
        let mut by_case: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, (p, _, _)) in files.iter().enumerate() {
            by_case.entry(p.to_string_lossy().to_lowercase()).or_default().push(i);
        }
        let mut cased = vec![false; files.len()];
        for group in by_case.values().filter(|g| g.len() > 1) {
            let first = files[group[0]].0.clone();
            let folder = group[1..].iter().fold(first, |f, &i| shared_folder(&f, &files[i].0));
            let looked = group.iter().all(|&i| files[i].1 & flag::LOOKED != 0);
            scans.push((folder, if looked { Why::Look } else { Why::Folder }));
            group.iter().for_each(|&i| cased[i] = true);
        }
        let mut cased = cased.into_iter();
        files.retain(|_| !cased.next().unwrap_or(false));
    }

    /// A path an event named (`Why::Event`; `Why::Look` a look's; a compare's own when its folder is gone), by one
    /// stat: a file's change decided by the table — an event's: a held file said changed whatever its stamp says, but
    /// at the first event naming one still marked with its stamp the same (`Held::marked`): nothing, the mark taken
    /// (true); any other `why`'s as a compare of that one file (`judged`: changed only where its stamp moved, or for
    /// the reasons a compare has — calls 7: a save's look after its event, in another burst, says nothing) —, a
    /// folder left to `compare` — one `unwanted` never comes here (`step` drops it). The mark is an event's only.
    fn look(&mut self, path: &Path, why: Why, said: &mut Saying, scans: &mut Vec<(PathBuf, Why)>) -> bool {
        let (taken, now) = (crate::clock::system(), crate::clock::now());
        #[cfg(test)]
        {
            self.stats += 1;
        }
        let (folder, file) = match std::fs::symlink_metadata(path) {
            Ok(m) if m.is_dir() => (true, None),
            // (a link to a folder isn't followed: not a file)
            Ok(m) if m.is_symlink() => (false, std::fs::metadata(path).ok().filter(|m| !m.is_dir())),
            Ok(m) => (false, Some(m)),
            Err(_) => (false, None),
        };
        if folder {
            scans.push((path.to_path_buf(), why.folded()));
        } else {
            // (a folder gone, or replaced by a file: what it held is gone)
            for p in self.below(path) {
                let held = self.table.remove(&p);
                self.say(said, &p, (Kind::Deleted, why), held, None, (taken, now));
            }
        }
        match file.filter(|_| (self.keep)(path)) {
            None => {
                if let Some(held) = self.table.remove(path) {
                    self.say(said, path, (Kind::Deleted, why), Some(held), None, (taken, now));
                }
            }
            Some(m) => {
                let stamp = stamp_of(&m);
                let sure = crate::disk::sure(&stamp, taken);
                if why != Why::Event {
                    let held = self.table.get(path).copied();
                    let (kind, new) = judged(held, stamp, sure, why, now);
                    self.put(path, new, now);
                    if let Some(kind) = kind {
                        self.say(said, path, (kind, why), held, Some(stamp), (taken, now));
                    }
                    return false;
                }
                // (an event stood for it: owed nothing, named)
                let new = Held {
                    stamp,
                    sure,
                    owed: false,
                    unnamed: None,
                    #[cfg(test)]
                    put: Some(Put { by: By::Event, at: now, step: self.steps }),
                };
                let held = self.table.insert(path.to_path_buf(), new);
                let kind = match held {
                    // (the make a compare already said, its own event landed late: the mark taken)
                    Some(h) if h.marked(now) && h.stamp == stamp => return true,
                    Some(_) => Kind::Changed,
                    None => Kind::Created,
                };
                self.say(said, path, (kind, why), held, Some(stamp), (taken, now));
            }
        }
        false
    }

    /// A compare's entry (`judged`) in the table — in tests, put by this compare where it has no one's yet (made or
    /// moved here).
    fn put(&mut self, path: &Path, new: Held, now: Instant) {
        #[cfg(test)]
        let new = Held { put: new.put.or(Some(Put { by: By::Compare, at: now, step: self.steps })), ..new };
        #[cfg(not(test))]
        let _ = now;
        self.table.insert(path.to_path_buf(), new);
    }

    /// `p` said with its kind and why, unless said already this step (the first said wins): `held` its entry
    /// before, `stamp` now (none: gone), `at` the clocks read before the stat. An event's changed with the stamp the
    /// same is counted (`unmoved`); in tests, what the word stood on is recorded (`Record`).
    fn say(
        &mut self,
        said: &mut Saying,
        p: &Path,
        (kind, why): (Kind, Why),
        held: Option<Held>,
        stamp: Option<Stamp>,
        at: (SystemTime, Instant),
    ) {
        if said.contains_key(p) {
            return;
        }
        said.insert(p.to_path_buf(), (kind, why));
        if let Some(h) = held.filter(|h| why == Why::Event && kind == Kind::Changed && Some(h.stamp) == stamp)
        {
            self.unmoved[usize::from(h.unnamed.is_none())] += 1;
        }
        #[cfg(test)]
        {
            let age = stamp.and_then(|s| at.0.duration_since(s.0).ok());
            let mark = held.and_then(|h| h.unnamed).map(|m| at.1.saturating_duration_since(m));
            self.record.insert(p.to_path_buf(), Record { why, step: self.steps, held, stamp, age, mark });
        }
        #[cfg(not(test))]
        let _ = at;
    }

    /// A folder walked and compared with the table by stamp: a file not held is created, one held and not found
    /// deleted, one whose stamp moved changed. A held file whose stamp didn't move is said changed only for a reason
    /// (`Why`): events were lost over it (`Why::Lost`) and its held stamp couldn't tell a write apart (`disk::sure`);
    /// or it's owed (`Held::owed`) and its stamp can be judged now — once. A stamp this compare found that no event
    /// stood for, too fresh to judge, is owed in turn. Asked by a look only: one made or whose stamp moved, unsaid by
    /// any event and older than `MISSED_AFTER`, is counted missed (a deletion has no stamp to tell its age by).
    fn compare(&mut self, dir: &Path, why: Why, said: &mut Saying) {
        if !std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir()) {
            self.look(dir, why, said, &mut Vec::new());
            return;
        }
        let (taken, at) = (crate::clock::system(), crate::clock::now());
        let mut found = BTreeMap::new();
        self.walk(dir, &mut found, &mut 0);
        for p in self.below(dir) {
            if !found.contains_key(&p) {
                let held = self.table.remove(&p);
                self.say(said, &p, (Kind::Deleted, why), held, None, (taken, at));
            }
        }
        for (p, stamp) in found {
            let held = self.table.get(&p).copied();
            let (kind, new) = judged(held, stamp, crate::disk::sure(&stamp, taken), why, at);
            self.put(&p, new, at);
            let moved = held.is_none_or(|h| h.stamp != stamp);
            let Some(kind) = kind else { continue };
            // (one an event named was looked at first: its stamp is held already, so it never comes here)
            let old = taken.duration_since(stamp.0).is_ok_and(|age| age >= MISSED_AFTER);
            self.missed += usize::from(why == Why::Look && moved && old);
            self.say(said, &p, (kind, why), held, Some(stamp), (taken, at));
        }
    }

    /// FSEvents' word that a root itself moved, went or came: still the same folder (or still none), nothing; else
    /// it's compared as it is now — gone, its files deleted; another folder there or back, its files created — and
    /// it stays a root (a `cargo clean` takes an out dir, the next build makes it again).
    fn root_changed(&mut self, path: &Path, said: &mut Saying) {
        let Some(i) = self.roots.iter().position(|r| r.path == path) else { return };
        let now = folder_id(path);
        if now == self.roots[i].id {
            return;
        }
        self.roots[i].id = now;
        if now.is_none() {
            let what = format!("{} was removed or renamed: watched for its return", path.display());
            crate::log::line("tree-watch", "info", &what);
        }
        self.compare(path, Why::Folder, said);
    }

    /// Other roots and folders left out: what's no longer watched leaves the table unsaid (it isn't deleted, only
    /// no longer watched); a new root, and a folder no longer left out, are walked.
    fn set(&mut self, roots: Vec<Root>, folders: Vec<PathBuf>) {
        let old = std::mem::replace(&mut self.roots, roots);
        let was = std::mem::replace(&mut self.leave_out, folders);
        self.drop_unwatched();
        let mut fresh: Vec<PathBuf> = (self.roots.iter())
            .filter(|r| !old.iter().any(|o| o.path == r.path && o.id == r.id))
            .map(|r| r.path.clone())
            .collect();
        fresh.extend(was.into_iter().filter(|w| self.watched(w)));
        self.fill(&fresh);
    }

    /// Other files kept: what's no longer kept leaves the table, the new matches come in — both unsaid.
    fn set_keep(&mut self, keep: Keep) {
        self.keep = keep;
        let keep = &self.keep;
        self.table.retain(|p, _| keep(p));
        self.fill(&self.root_paths());
        if self.table.len() > MANY && !std::mem::replace(&mut self.many_said, true) {
            let said = format!("what the servers asked to watch keeps {} files", self.table.len());
            crate::log::line("lsp", "watch", &said);
        }
    }
}

/// What a compare says of a file it found with `stamp` (`sure` as taken now), the table holding `held`, and what the
/// table holds of it after: created when not held; changed when its stamp moved, when events were lost over it and
/// the held stamp couldn't tell a write apart, or when it was owed and can be judged now (once); else nothing. A
/// stamp no event stood for — made or moved here — too fresh to judge is owed; one owed and still too fresh stays so.
/// A stamp made or moved here is marked at `now` (`Held::unnamed`); a mark not yet taken stays, as when it was set.
fn judged(held: Option<Held>, stamp: Stamp, sure: bool, why: Why, now: Instant) -> (Option<Kind>, Held) {
    let kind = match held {
        None => Some(Kind::Created),
        Some(h) if h.stamp != stamp => Some(Kind::Changed),
        // (events lost over it: a write of the same size within its stamp's grain may be among them)
        Some(h) if why == Why::Lost && !h.sure => Some(Kind::Changed),
        // (no event ever stood for it, judged at last: #286's care, once)
        Some(h) if h.owed && sure => Some(Kind::Changed),
        Some(_) => None,
    };
    let moved = held.is_none_or(|h| h.stamp != stamp);
    let still = kind.is_none() && held.is_some_and(|h| h.owed);
    let unnamed = if moved { Some(now) } else { held.and_then(|h| h.unnamed) };
    let new = Held {
        stamp,
        sure,
        owed: !sure && (moved || still),
        unnamed,
        // (made or moved here: the compare puts it, `Tree::put`)
        #[cfg(test)]
        put: held.filter(|_| !moved).and_then(|h| h.put),
    };
    (kind, new)
}

/// The events a step's mark took, for the log: how many, and their flags, each set with its count, the most first —
/// what FSEvents gave a make's late event (W1b step 3, round 7: the record of round 6's split 430).
fn marked_line(flags: &[u32]) -> String {
    let mut by: BTreeMap<u32, usize> = BTreeMap::new();
    for f in flags {
        *by.entry(*f).or_default() += 1;
    }
    let mut by: Vec<(u32, usize)> = by.into_iter().collect();
    crate::sorting::by_key(&mut by, |(f, n)| (std::cmp::Reverse(*n), *f));
    let flags_of: Vec<String> = by.iter().map(|(f, n)| format!("{f:#x} ×{n}")).collect();
    format!(
        "the mark took {} events — each the first to name a file a compare had put in the table, its stamp \
         the same: nothing said — their flags {}",
        flags.len(),
        flags_of.join(", ")
    )
}

/// A batch as the log has it — how many created, deleted and changed; said by what (`Tree.by`: an event, a
/// folder's compare, a look, lost events); from how many events, those that asked for more than a look (a scan
/// below, dropped events, a root's own change); and how long after its burst's first. Every batch, changes only too
/// (a save): a checkout's whole count, and anything said again, is read from these lines (W1b step 3, rounds 5–6).
/// Of an event's, the changed with the stamp the same (`Tree.unmoved`): past a mark's window, and with no mark —
/// counted before any rule moves (#502).
fn said_files(
    batch: &[(PathBuf, Kind)],
    (by, unmoved): ([usize; 4], [usize; 2]),
    events: &[(PathBuf, u32)],
    first: Option<Instant>,
) -> String {
    let count = |k: Kind| batch.iter().filter(|(_, kind)| *kind == k).count();
    let (made, changed, gone) = (count(Kind::Created), count(Kind::Changed), count(Kind::Deleted));
    let n = |why: Why| by[why as usize];
    let flagged = |bits: u32| events.iter().filter(|(_, f)| f & bits != 0).count();
    let (scan, dropped, root) = (
        flagged(flag::MUST_SCAN_SUB_DIRS),
        flagged(flag::USER_DROPPED | flag::KERNEL_DROPPED),
        flagged(flag::ROOT_CHANGED),
    );
    let after = first.map_or_else(|| "—".to_string(), |f| crate::term::run_duration(crate::clock::since(f)));
    format!(
        "a batch said {made} created, {gone} deleted, {changed} changed, said by {} an event (changed with \
         the stamp the same: {} past a mark's window, {} with no mark), {} a folder's compare, {} a look, {} lost \
         events, from {} events ({scan} must scan below, {dropped} dropped, {root} root changed), {after} after its \
         burst's first",
        n(Why::Event),
        unmoved[0],
        unmoved[1],
        n(Why::Folder),
        n(Why::Look),
        n(Why::Lost),
        events.len()
    )
}

/// A running total that went from `before` to `after` passed a power of two: 1, 2, 4, …
fn doubled(before: usize, after: usize) -> bool {
    after >= (before + 1).next_power_of_two()
}

fn stamp_of(m: &std::fs::Metadata) -> Stamp {
    (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len())
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{CStr, CString, OsStr, c_char, c_void};
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
    use std::time::Duration;

    use super::{Burst, EXCLUDED_MAX, GONE, Given, Group, Msg, Said, Tree, TreeWatch, groups, roots_of};

    #[repr(C)]
    struct Context {
        version: isize,
        info: *mut c_void,
        retain: *const c_void,
        release: *const c_void,
        copy_description: *const c_void,
    }

    type Callback = extern "C" fn(*const c_void, *mut c_void, usize, *mut c_void, *const u32, *const u64);

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        // (a `CFArrayCallBacks`: its address is all that's used)
        static kCFTypeArrayCallBacks: [usize; 5];
        fn CFStringCreateWithFileSystemRepresentation(
            alloc: *const c_void,
            path: *const c_char,
        ) -> *const c_void;
        fn CFArrayCreate(
            alloc: *const c_void,
            values: *const *const c_void,
            n: isize,
            callbacks: *const c_void,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn FSEventStreamCreate(
            alloc: *const c_void,
            callback: Callback,
            context: *const Context,
            paths: *const c_void,
            since: u64,
            latency: f64,
            flags: u32,
        ) -> *mut c_void;
        fn FSEventStreamSetExclusionPaths(stream: *mut c_void, paths: *const c_void) -> u8;
        fn FSEventStreamSetDispatchQueue(stream: *mut c_void, queue: *mut c_void);
        fn FSEventStreamStart(stream: *mut c_void) -> u8;
        fn FSEventStreamStop(stream: *mut c_void);
        fn FSEventStreamInvalidate(stream: *mut c_void);
        fn FSEventStreamRelease(stream: *mut c_void);
    }

    // libdispatch, in libSystem
    unsafe extern "C" {
        fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> *mut c_void;
        fn dispatch_sync_f(queue: *mut c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
        fn dispatch_release(object: *mut c_void);
    }

    /// `kFSEventStreamEventIdSinceNow`.
    const SINCE_NOW: u64 = u64::MAX;
    /// `kFSEventStreamCreateFlagNoDefer | WatchRoot | FileEvents` — not `IgnoreSelf`: tarae's own writes to files
    /// nobody has open (a take-back, a rename's edits) are said too.
    const FLAGS: u32 = 0x2 | 0x4 | 0x10;
    /// How long FSEvents gathers events after it delivered some (the first of a quiet spell comes at once, `NoDefer`):
    /// under `QUIET`, so a burst's deliveries keep it one burst.
    const LATENCY: Duration = super::QUIET.checked_div(2).unwrap();

    /// One FSEvents stream, started, on a queue of its own.
    pub(super) struct Stream {
        stream: *mut c_void,
        queue: *mut c_void,
        /// What the callback's `info` points at: freed only once the stream is gone and its queue drained (`Drop`).
        tx: *mut Sender<Msg>,
    }

    // SAFETY: the stream and queue are FSEvents' and libdispatch's, both usable from any thread; `tx` is only read
    // by the callback and freed in `Drop`, after the callback can no longer run.
    unsafe impl Send for Stream {}

    /// `paths` as a CFArray of CFStrings (the caller releases it), or none if one has a NUL.
    fn cf_paths(paths: &[PathBuf]) -> Option<*const c_void> {
        let c: Vec<CString> =
            paths.iter().map(|p| CString::new(p.as_os_str().as_bytes()).ok()).collect::<Option<_>>()?;
        // SAFETY: each pointer is a NUL-terminated string alive for the call.
        let strings: Vec<*const c_void> = c
            .iter()
            .map(|s| unsafe { CFStringCreateWithFileSystemRepresentation(std::ptr::null(), s.as_ptr()) })
            .collect();
        if strings.iter().any(|s| s.is_null()) {
            // SAFETY: the ones made are ours to release.
            strings.iter().filter(|s| !s.is_null()).for_each(|&s| unsafe { CFRelease(s) });
            return None;
        }
        // SAFETY: `strings` holds `len` CFStrings; the type callbacks retain them, so ours are released after.
        let array = unsafe {
            let a = CFArrayCreate(
                std::ptr::null(),
                strings.as_ptr(),
                strings.len() as isize,
                (&raw const kCFTypeArrayCallBacks).cast(),
            );
            strings.iter().for_each(|&s| CFRelease(s));
            a
        };
        (!array.is_null()).then_some(array)
    }

    impl Stream {
        pub(super) fn start(
            roots: &[PathBuf],
            exclude: &[PathBuf],
            tx: Sender<Msg>,
        ) -> Result<Stream, String> {
            let paths = cf_paths(roots).ok_or("a root's path can't be given to FSEvents")?;
            let tx = Box::into_raw(Box::new(tx));
            let context = Context {
                version: 0,
                info: tx.cast(),
                retain: std::ptr::null(),
                release: std::ptr::null(),
                copy_description: std::ptr::null(),
            };
            // SAFETY: `paths` is a CFArray of CFStrings; the context is copied by the call; `tx` outlives the
            // stream (`Drop`).
            let stream = unsafe {
                let s = FSEventStreamCreate(
                    std::ptr::null(),
                    heard,
                    &context,
                    paths,
                    SINCE_NOW,
                    LATENCY.as_secs_f64(),
                    FLAGS,
                );
                CFRelease(paths);
                s
            };
            if stream.is_null() {
                // SAFETY: no stream holds it.
                drop(unsafe { Box::from_raw(tx) });
                return Err("FSEvents couldn't make a stream".into());
            }
            if !exclude.is_empty() {
                // (refused, the worker's prefix drop still leaves them out)
                let taken = cf_paths(exclude).is_some_and(|ex| {
                    // SAFETY: a stream not started yet, and a CFArray of CFStrings (released after).
                    unsafe {
                        let ok = FSEventStreamSetExclusionPaths(stream, ex) != 0;
                        CFRelease(ex);
                        ok
                    }
                });
                if !taken {
                    let what = format!("FSEvents refused the folders to leave out: {exclude:?}");
                    crate::log::line("tree-watch", "warn", &what);
                }
            }
            // SAFETY: a serial queue (null attributes) with a static label; the stream set on it before it starts.
            let (queue, started) = unsafe {
                let q = dispatch_queue_create(c"tarae.treewatch".as_ptr(), std::ptr::null());
                FSEventStreamSetDispatchQueue(stream, q);
                (q, FSEventStreamStart(stream) != 0)
            };
            let s = Stream { stream, queue, tx };
            if !started {
                // (Drop takes it down: stopping one that never started is a no-op)
                return Err(
                    "FSEvents' service didn't start the stream (denied here, as inside a sandbox?)".into()
                );
            }
            Ok(s)
        }
    }

    impl Drop for Stream {
        fn drop(&mut self) {
            extern "C" fn drained(_: *mut c_void) {}
            // SAFETY: the stream is stopped, unscheduled and released in the order FSEvents asks; a callback still
            // running on the (serial) queue ends before `dispatch_sync_f` returns, so none reads `tx` after.
            unsafe {
                FSEventStreamStop(self.stream);
                FSEventStreamInvalidate(self.stream);
                FSEventStreamRelease(self.stream);
                dispatch_sync_f(self.queue, std::ptr::null_mut(), drained);
                dispatch_release(self.queue);
                drop(Box::from_raw(self.tx));
            }
        }
    }

    /// The stream's callback, on its queue: the events handed to the worker, nothing more.
    extern "C" fn heard(
        _stream: *const c_void,
        info: *mut c_void,
        n: usize,
        paths: *mut c_void,
        flags: *const u32,
        _ids: *const u64,
    ) {
        // SAFETY: `info` is the stream's `tx` (alive until it's gone and this queue drained); without
        // `UseCFTypes`, `paths` is `n` C strings and `flags` `n` flags.
        let (tx, events) = unsafe {
            let tx = &*info.cast::<Sender<Msg>>();
            let paths = paths.cast::<*const c_char>();
            let events: Vec<(PathBuf, u32)> = (0..n)
                .map(|i| {
                    let p = CStr::from_ptr(*paths.add(i));
                    (PathBuf::from(OsStr::from_bytes(p.to_bytes())), *flags.add(i))
                })
                .collect();
            (tx, events)
        };
        let _ = tx.send(Msg::Events(events));
    }

    pub(super) fn start(given: Given, out: Box<dyn FnMut(Said) + Send>) -> Result<TreeWatch, String> {
        let tree = Tree::new(given)?;
        let (tx, rx) = mpsc::channel();
        let (ready, started) = mpsc::channel();
        let events = tx.clone();
        crate::event::thread("tarae-treewatch", move || run(tree, &rx, events, out, ready));
        started.recv().map_err(|_| GONE.to_string())??;
        Ok(TreeWatch { tx })
    }

    /// A stream running: the group it watches, and its number — the worker's count of the streams it started.
    struct Running {
        group: Group,
        /// Held for its drop: the stream stops with it.
        _stream: Stream,
        /// (read by a test's `seen`)
        #[cfg_attr(not(test), allow(dead_code))]
        number: u64,
    }

    /// The streams the worker runs, and how many it has started.
    #[derive(Default)]
    struct Streams {
        running: Vec<Running>,
        started: u64,
        /// The roots said once to have more than `EXCLUDED_MAX` folders left out below them.
        said_over: Vec<PathBuf>,
    }

    impl Streams {
        /// The streams for `wanted`: one started for each group not running — all of them before any old one
        /// stops, so nothing goes unheard between —, then the groups no longer wanted stopped; the rest go on. No:
        /// the ones it started stop, the running stay.
        fn set(&mut self, wanted: &[Group], over: &[PathBuf], events: &Sender<Msg>) -> Result<(), String> {
            let mut fresh = Vec::new();
            for g in wanted.iter().filter(|g| !self.running.iter().any(|r| r.group == **g)) {
                let stream = Stream::start(&g.roots, &g.exclude, events.clone())?;
                self.started += 1;
                fresh.push(Running { group: g.clone(), _stream: stream, number: self.started });
            }
            let (mut kept, stopped): (Vec<Running>, Vec<Running>) =
                std::mem::take(&mut self.running).into_iter().partition(|r| wanted.contains(&r.group));
            // (what an old stream still said is filtered by root: the prefix drop)
            drop(stopped);
            kept.append(&mut fresh);
            self.running = kept;
            for r in over {
                if self.said_over.contains(r) {
                    continue;
                }
                let what = format!(
                    "{} has more than {EXCLUDED_MAX} folders left out below it: the first {EXCLUDED_MAX} go to its \
                     stream, what's below the rest is dropped by prefix on the worker",
                    r.display()
                );
                crate::log::line("tree-watch", "info", &what);
                self.said_over.push(r.clone());
            }
            Ok(())
        }
    }

    /// About how many descriptors the streams hold, by W1a §10's measure: one a stream, and (`WatchRoot`) one for
    /// each folder on each root's path and on its volume's mount point's — 11 for one root seven folders deep on the
    /// data volume (`/System/Volumes/Data`), 53 for §10's four streams. Each stream holds its own.
    pub(super) fn descriptors(groups: &[Group]) -> usize {
        let folders = |root: &PathBuf| {
            let at = root.ancestors().find(|a| a.exists()).unwrap_or(root);
            let mount = CString::new(at.as_os_str().as_bytes()).ok().and_then(|c| {
                // SAFETY: a plain value, filled by the call for a NUL-terminated path.
                let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
                (unsafe { libc::statfs(c.as_ptr(), &mut fs) } == 0).then(|| {
                    // SAFETY: the mount point's name is NUL-terminated within the field.
                    let name = unsafe { CStr::from_ptr(fs.f_mntonname.as_ptr()) };
                    PathBuf::from(OsStr::from_bytes(name.to_bytes()))
                })
            });
            let below_slash = |p: &std::path::Path| p.components().count().saturating_sub(1);
            below_slash(root) + mount.map_or(0, |m| below_slash(&m))
        };
        groups.iter().map(|g| 1 + g.roots.iter().map(folders).sum::<usize>()).sum()
    }

    /// The worker: the streams started (said to `ready`), the table filled, then each settled burst looked at and
    /// said, and the roots, folders left out and files kept replaced as asked.
    fn run(
        mut tree: Tree,
        rx: &Receiver<Msg>,
        events: Sender<Msg>,
        mut out: Box<dyn FnMut(Said) + Send>,
        ready: Sender<Result<(), String>>,
    ) {
        let started = crate::clock::now();
        let mut streams = Streams::default();
        let (wanted, over) = groups(&tree.root_paths(), &tree.leave_out);
        if let Err(why) = streams.set(&wanted, &over, &events) {
            return drop(ready.send(Err(why)));
        }
        let _ = ready.send(Ok(()));
        let streamed = crate::clock::since(started);
        let links = tree.fill(&tree.root_paths());
        let what = format!(
            "{} streams started in {}, holding about {} descriptors, then {} files kept below {} roots in {} ({links} \
             links to folders not followed)",
            streams.running.len(),
            crate::term::run_duration(streamed),
            descriptors(&wanted),
            tree.table.len(),
            tree.roots.len(),
            crate::term::run_duration(crate::clock::since(started) - streamed),
        );
        crate::log::line("tree-watch", "info", &what);
        let (mut pending, mut burst) = (Vec::new(), Burst::default());
        loop {
            let left = burst.left(crate::clock::now());
            let msg = match left {
                // (settled: said now, even while events keep coming — the next burst holds them)
                Some(Duration::ZERO) => Err(RecvTimeoutError::Timeout),
                Some(left) => rx.recv_timeout(left),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match msg {
                Err(RecvTimeoutError::Timeout) => {
                    let first = std::mem::take(&mut burst).first;
                    let events = std::mem::take(&mut pending);
                    if let Some(said) = tree.settle(&events) {
                        let what = super::said_files(&said.batch, (tree.by, tree.unmoved), &events, first);
                        crate::log::line("tree-watch", "info", &what);
                        out(said);
                    }
                }
                Err(RecvTimeoutError::Disconnected) | Ok(Msg::Stop) => return,
                Ok(Msg::Events(e)) => {
                    burst.add(crate::clock::now());
                    pending.extend(e);
                }
                Ok(Msg::Set(paths, folders, reply)) => {
                    let (before, began) = (streams.started, crate::clock::now());
                    let new = roots_of(&paths).and_then(|roots| {
                        let (wanted, over) = groups(&paths, &folders);
                        streams.set(&wanted, &over, &events).map(|()| (roots, wanted))
                    });
                    match new {
                        Ok((roots, wanted)) => {
                            let _ = reply.send(Ok(()));
                            // (what a server's start or a registration costs in streams: said when it's any)
                            if streams.started > before {
                                let what = format!(
                                    "a set started {} streams in {}: {} running, holding about {} descriptors",
                                    streams.started - before,
                                    crate::term::run_duration(crate::clock::since(began)),
                                    streams.running.len(),
                                    descriptors(&wanted),
                                );
                                crate::log::line("tree-watch", "info", &what);
                            }
                            tree.set(roots, folders);
                        }
                        Err(why) => drop(reply.send(Err(why))),
                    }
                }
                Ok(Msg::Keep(keep)) => tree.set_keep(keep),
                #[cfg(test)]
                Ok(Msg::Seen(reply)) => {
                    let running = streams.running.iter().map(|r| (r.number, r.group.clone())).collect();
                    let seen = super::Seen {
                        streams: running,
                        unwatched: tree.unwatched,
                        unwatched_paths: tree.unwatched_paths.clone(),
                        unkept: tree.unkept,
                    };
                    let _ = reply.send(seen);
                }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn stream_for_test(
        roots: &[PathBuf],
        exclude: &[PathBuf],
    ) -> Result<(Stream, Receiver<Msg>), String> {
        let (tx, rx) = mpsc::channel();
        Ok((Stream::start(roots, exclude, tx)?, rx))
    }
}

#[cfg(test)]
mod tests;
