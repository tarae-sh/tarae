//! Turn checkpoints — what Claude changed in one chat turn, across files, undone (and redone) as one.
//!
//! - Every edit of Claude's that the user accepts (`edits.rs`) is recorded against the turn: per file, the
//!   text before Claude's first change this turn and after its latest. The turn ending makes a checkpoint
//!   (the chat says so, with `C-z`).
//! - `space u` (`C-z` in the chat) · `:undo-turn` takes the latest turn still applied back; `space U` ·
//!   `:redo-turn` brings the latest undone one back. Per file it's a three-way step over what the file is now: Claude's changes go,
//!   the user's own edits since stay; where the user edited on or next to one of Claude's changes, that
//!   change stays too, and the toast says how many.
//! - Each file changes as one transaction (`u` there takes it back); a file that had no unsaved edits is saved
//!   again, so the disk — what Claude, tests and git read — matches. A file the turn created goes (deleted,
//!   its buffer closed) when nothing else is in it; redo writes it again.
//! - Claude is told on the next message (its memory still has its edits).
//! - Review at the end (`edits.rs`): edits Claude makes unseen keep only the text before; what it is after is
//!   read when the turn ends (`gather`), and the turn opens as a review (`codereview::start_turn`).
//! - A command of Claude's that isn't a check: what it changed in the checkout joins the turn the same way
//!   (`record_run`, `snapshot.rs`) — a file it deleted comes back with undo, one that isn't text by its git
//!   blob. One a run keeps by its blobs that is text at both of the turn's ends (past `snapshot::TEXT_MAX`) is
//!   kept as text from the turn's end (`settle_blobs`), Claude's edits around the run with it.
//! - Kept on disk per project (`$XDG_STATE_HOME/tarae/turns/<project hash>/`, and the turn in flight), written on
//!   a worker at each change: a new tarae (`:restart`, a crash) still has them. A turn cut short comes back as one
//!   of its own, its files as they are now.
//! - Kept as an editor keeps its history, by time (user, 2026-10-29): every turn while its conversation is — Claude
//!   Code's `cleanupPeriodDays`, read where the chat's claude reads it (`idlecompact::Places`) — no count; and a
//!   project's whole store within `llm.turns-max-mb`, its oldest turns going first, said once (`Room`, `save`).
//! - A text is a file of its own in `texts/`, named by its content's digest (`Text`): written once however many
//!   turns hold it (one turn's text after is the next one's before), and gone once no kept turn names it.
//!   `index.json` says each turn's question, paths, time and texts by name.
//! - Kept turns come back as their questions and paths only (`Files::Kept`, #218: 26 sessions' texts held
//!   525 MB), their texts read on a worker when a step or a review needs them (`with_texts`). A turn's texts in
//!   their files are let go where its session is behind, and in front past the newest `KEEP` (`shed`, #214).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ropey::Rope;
use serde_json::{Value, json};

use crate::editor::Editor;
use crate::transaction::{Change, Transaction};

/// One file a turn changed.
#[derive(Clone, Debug, Default)]
pub struct Touched {
    pub path: PathBuf,
    /// Before Claude's first change this turn — None: the turn created the file, or (`bytes`) it's kept by its
    /// blob only (`was`).
    pub before: Option<Rope>,
    /// After its latest.
    pub after: Rope,
    /// Written without the user seeing it (review at the end, `edits.rs`; a run's change, `snapshot.rs`) —
    /// `after` is read from disk when the turn ends, and the turn is reviewed then.
    pub unseen: bool,
    /// After it there's no file (a run deleted it): undo writes it again, redo deletes it.
    pub gone: bool,
    /// Not text (a run's change): what it was and is as git's blobs — taken back by writing a blob out.
    pub bytes: Option<Bytes>,
}

/// A file a run changed that isn't text, by its git blobs (None: no file).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bytes {
    pub before: Option<String>,
    pub after: Option<String>,
    /// What it was before the turn can't be had (its blob gone from the repository by the turn's end, `settle_blobs`):
    /// why — a step leaves the file as it is and says so.
    pub lost: Option<String>,
}

/// A file kept by its blobs during a turn is kept as text at its end (`settle_blobs`) up to this, either end — past it,
/// by its blobs still.
const WHOLE_MAX: u64 = 32 << 20;

/// A file kept by its blobs, as the turn's end takes it to its worker (`gather`): the root whose repository has its
/// blobs, the text an edit of Claude's kept before the run, its blobs.
type Blobbed = (PathBuf, Option<Rope>, Bytes);

/// A file the turn wrote unseen, as its end reads it (`gather`): its text (None: unreadable or no text), its
/// fingerprint, whether it's gone, and — one kept by its blobs — how it's kept from now on.
type Gathered = (Option<Rope>, Option<crate::disk::Stamp>, bool, Option<Settled>);

/// A file the turn kept by its blobs, as its end finds it (`settle_blobs`).
#[derive(Debug)]
enum Settled {
    /// Text at both ends: kept as any text file is — what it was before the turn (None: the turn made it).
    Text(Option<Rope>),
    /// By its blobs still, both the turn's ends.
    Blobs(Bytes),
}

/// One turn's changes.
#[derive(Clone, Debug)]
pub struct Checkpoint {
    /// What the user asked (its first line).
    pub question: String,
    pub files: Files,
    pub undone: bool,
    /// Its changes have been looked at: each was reviewed as it came, or the user ended its review (`space
    /// v q`), or a git review showing all its files — not one written unseen whose review a later turn's
    /// replaced (`space v t` says so).
    pub seen: bool,
    /// Which turn it is, among the kept ones — a new one whenever its files change (`own`).
    pub id: u64,
    /// When it was made (seconds since the epoch, `clock::system`): kept as long as its conversation would be.
    pub at: u64,
    /// Its conversation (claude's id; None: the chat had none yet, or a turn cut short) — the turn goes as it does,
    /// counted from its last activity (`save`).
    pub conv: Option<String>,
    /// Each file's texts as kept, in `files`' order — empty until a save has written them (`filed`); whatever changes
    /// its files empties it, with a new `id`.
    pub texts: Vec<Texts>,
    /// HEAD's moves in its runs — the files they brought aren't its own (said at its review's head).
    pub head: Vec<crate::snapshot::HeadMove>,
}

/// A turn's files: with their texts, or as kept — paths only, the texts in the turn's file.
#[derive(Clone, Debug)]
pub enum Files {
    Here(Vec<Touched>),
    Kept(Vec<Kept>),
}

/// A text as kept: a file of its own in the project's `texts/`, named by its content's digest — written once,
/// whichever turns hold it — and its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Text {
    pub hash: [u8; 32],
    pub len: u64,
}

/// A file's texts as kept: before (None: the turn made it) and after.
pub type Texts = (Option<Text>, Text);

impl Text {
    fn of(text: &str) -> Text {
        Text { hash: crate::sha256::digest(text.as_bytes()), len: text.len() as u64 }
    }

    /// Its file's name in `texts/`.
    fn name(&self) -> String {
        self.hash.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn json(&self) -> Value {
        json!({ "hash": self.name(), "len": self.len })
    }

    fn from_json(v: &Value) -> Option<Text> {
        let hex = v["hash"].as_str().filter(|h| h.len() == 64)?;
        let mut hash = [0; 32];
        for (i, b) in hash.iter_mut().enumerate() {
            *b = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
        }
        Some(Text { hash, len: v["len"].as_u64()? })
    }
}

/// A file of a turn kept on disk, without its texts.
#[derive(Clone, Debug, PartialEq)]
pub struct Kept {
    pub path: PathBuf,
    pub unseen: bool,
    pub gone: bool,
    pub bytes: Option<Bytes>,
}

impl Files {
    /// The files with their texts — None: they're on disk only (`with_texts`).
    pub fn here(&self) -> Option<&[Touched]> {
        match self {
            Files::Here(v) => Some(v),
            Files::Kept(_) => None,
        }
    }

    pub fn paths(&self) -> Vec<&Path> {
        match self {
            Files::Here(v) => v.iter().map(|t| t.path.as_path()).collect(),
            Files::Kept(v) => v.iter().map(|k| k.path.as_path()).collect(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Files::Here(v) => v.len(),
            Files::Kept(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Without their texts.
    fn kept(&self) -> Vec<Kept> {
        match self {
            Files::Here(v) => v
                .iter()
                .map(|t| Kept {
                    path: t.path.clone(),
                    unseen: t.unseen,
                    gone: t.gone,
                    bytes: t.bytes.clone(),
                })
                .collect(),
            Files::Kept(v) => v.clone(),
        }
    }
}

/// What a turn's file was before it, as its review diffs it (`Touched::was`, `codereview::turn_before`).
#[derive(Clone, Debug)]
pub enum Was {
    Text(Rope),
    /// Kept by its git blob only: a run's change of a file that isn't text, or is past what a run keeps as
    /// text (`snapshot::TEXT_MAX`) — read from git when the review asks.
    Blob(String),
    /// No file: the turn made it.
    None,
}

impl Touched {
    /// What it was before the turn: its text where the turn kept it (an edit's, or a run's of a text file),
    /// else its blob before.
    pub fn was(&self) -> Was {
        match (&self.before, &self.bytes) {
            (Some(t), _) => Was::Text(t.clone()),
            (None, Some(Bytes { before: Some(id), .. })) => Was::Blob(id.clone()),
            (None, _) => Was::None,
        }
    }

    /// It's a change (a file changed and changed back isn't).
    fn changed(&self) -> bool {
        match (&self.bytes, &self.before) {
            (Some(b), _) => b.before != b.after,
            (None, None) => true,
            (None, Some(b)) => *b != self.after || self.gone,
        }
    }
}

impl Checkpoint {
    pub fn new(question: String, files: Vec<Touched>) -> Self {
        let seen = !files.iter().any(|t| t.unseen);
        let (files, id, at) = (Files::Here(files), new_id(), now_s());
        let (texts, head, conv) = (Vec::new(), Vec::new(), None);
        Checkpoint { question, files, undone: false, seen, id, at, conv, texts, head }
    }

    /// Its texts are in their files — written by a save, or read back from them: they may go (`shed`).
    pub fn filed(&self) -> bool {
        !self.texts.is_empty()
    }
}

/// The conversation session `l`'s chat is in (claude's id) — a turn made now is its.
fn conversation(l: &crate::workspace::Workspace) -> Option<String> {
    l.claude.chat.as_ref()?.conversation.clone()
}

/// Now, in seconds since the epoch, as logic reads it.
fn now_s() -> u64 {
    crate::clock::system().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A turn's id: when it was made (ns), one past the last given when that's later — never the same twice.
fn new_id() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = crate::chats::seq().max(1);
    let next = |l: u64| l.max(now - 1) + 1;
    next(LAST.try_update(Ordering::Relaxed, Ordering::Relaxed, |l| Some(next(l))).unwrap_or_else(|l| l))
}

#[derive(Default)]
pub struct Checkpoints {
    pub list: Vec<Checkpoint>,
    /// The turn in flight.
    pub turn: Vec<Touched>,
    /// HEAD's moves in the turn in flight's runs (`Checkpoint.head`).
    pub head: Vec<crate::snapshot::HeadMove>,
    /// Where a move or copy of Claude's put a file it wrote unseen this turn, when that place held something
    /// that isn't text before (no `before` to keep for it in `turn`): named like the unseen ones all the same
    /// (`tools::moved`). Gone with the turn.
    pub carried: Vec<PathBuf>,
    /// The roots whose standing a run's report said this turn (`snapshot::FolderLine`: a folder that's no git
    /// checkout) — not said again by its next runs. Gone with the turn.
    pub told: Vec<PathBuf>,
    /// What Claude hears with the next message (it undid or redid a turn).
    pub tell: Vec<String>,
    /// The turn that just ended wrote files unseen, being read (`gather`): its checkpoint comes after.
    pub gathering: bool,
    /// Where they're kept (the state dir's `turns/`; None: not kept — tests).
    pub dir: Option<PathBuf>,
    /// How many of `list`, from its start, came back as kept (`restore_from`) — not the turn tarae ended in
    /// the middle of, put after them, nor those made since: a turn's review carried over `:restart` counts
    /// back from the last of these (`codereview::reopen_turn`).
    pub restored: usize,
    /// Its kept turns are being read back (`restore_from`): tarae's memory isn't judged meanwhile
    /// (`Editor::restoring`).
    pub restoring: bool,
    /// A turn's texts are being read (`with_texts`): a step waits.
    pub reading: bool,
    /// A step (`undo`) waits for its big files' buffers to load, these (`step`, `loaded`).
    pub loading: Option<(bool, Vec<(crate::document::DocId, PathBuf)>)>,
    /// A step's files that aren't text are being written (`put_blobs`): the next step waits.
    pub putting: bool,
    /// A save let its oldest turns go past `llm.turns-max-mb`, said (`saved`): since then in `:messages` only.
    pub told_full: bool,
}

/// An accepted edit of Claude's landed in document `id`: `before` was its text just before (None = no such
/// file), the buffer holds the text after.
pub fn record(ed: &mut Editor, id: crate::document::DocId, before: Option<Rope>) {
    let Some((path, after)) =
        ed.docs.iter().find(|d| d.id == id).and_then(|d| Some((d.path.clone()?, d.text.clone())))
    else {
        return;
    };
    let turn = &mut ed.workspace.claude.checkpoints.turn;
    match turn.iter_mut().find(|t| t.path == path) {
        Some(t) => t.after = after,
        None => {
            turn.push(Touched { path, before, after, ..Default::default() });
            keep(ed);
        }
    }
}

/// An edit of Claude's let in unasked (review at the end) to `path`, which was `before` (None = no such
/// file) — kept the first time this turn; what it is after is read when the turn ends. `then` (what goes on
/// to write it) runs once that's on disk: a crash before the write still has what it was.
pub fn record_unseen(
    ed: &mut Editor,
    path: PathBuf,
    before: Option<Rope>,
    then: impl FnOnce(&mut Editor) + Send + 'static,
) {
    let turn = &mut ed.workspace.claude.checkpoints.turn;
    match turn.iter_mut().find(|t| t.path == path) {
        Some(t) if t.unseen => then(ed),
        Some(t) => {
            t.unseen = true;
            keep_then(ed, then);
        }
        None => {
            let after = before.clone().unwrap_or_default();
            turn.push(Touched { path, before, after, unseen: true, ..Default::default() });
            keep_then(ed, then);
        }
    }
}

/// `record_unseen` for many files at once (a rename, replace or edit of Claude's, `refactor.rs`): each
/// `(path, before)` (None = no such file) — `then` once what each was is on disk, kept once for them all.
pub fn record_unseen_all(
    ed: &mut Editor,
    files: Vec<(PathBuf, Option<Rope>)>,
    then: impl FnOnce(&mut Editor) + Send + 'static,
) {
    let turn = &mut ed.workspace.claude.checkpoints.turn;
    let mut new = false;
    for (path, before) in files {
        match turn.iter_mut().find(|t| t.path == path) {
            Some(t) if t.unseen => {}
            Some(t) => (t.unseen, new) = (true, true),
            None => {
                let after = before.clone().unwrap_or_default();
                turn.push(Touched { path, before, after, unseen: true, ..Default::default() });
                new = true;
            }
        }
    }
    match new {
        true => keep_then(ed, then),
        false => then(ed),
    }
}

/// Accepted changes of Claude's to many files at once (a change set accepted whole on its card,
/// `refactor.rs`): each `(path, before, after)` — before None: no such file (one it makes); after None: no
/// file after (deleted, or moved from there: undo writes it again) — `then` once what each was is on disk,
/// before any is written.
pub fn record_all(
    ed: &mut Editor,
    files: Vec<(PathBuf, Option<Rope>, Option<Rope>)>,
    then: impl FnOnce(&mut Editor) + Send + 'static,
) {
    let turn = &mut ed.workspace.claude.checkpoints.turn;
    for (path, before, after) in files {
        let gone = after.is_none();
        let after = after.unwrap_or_default();
        match turn.iter_mut().find(|t| t.path == path) {
            Some(t) => (t.after, t.gone) = (after, gone),
            None => turn.push(Touched { path, before, after, gone, ..Default::default() }),
        }
    }
    keep_then(ed, then);
}

/// A change of the user's own across files (`:workspace-apply`, `refactor::land_yours`): a step of its own for
/// `space u`, `question` its title — its files join `step` when that's the one it made already (else a new one).
/// Each `(path, before, after)` as `record_all`'s; `then` once what each was is on disk, before any is written.
/// The step's id.
pub fn own(
    ed: &mut Editor,
    step: Option<u64>,
    question: &str,
    files: Vec<(PathBuf, Option<Rope>, Option<Rope>)>,
    then: impl FnOnce(&mut Editor) + Send + 'static,
) -> u64 {
    let touched = files.into_iter().map(|(path, before, after)| Touched {
        path,
        before,
        gone: after.is_none(),
        after: after.unwrap_or_default(),
        ..Default::default()
    });
    let conv = conversation(&ed.workspace);
    let list = &mut ed.workspace.claude.checkpoints.list;
    let id = match list.iter_mut().find(|c| Some(c.id) == step) {
        Some(c) => {
            if let Files::Here(fs) = &mut c.files {
                // (its texts change: written again — a save under way for the old ones finds no such turn)
                (c.id, c.texts) = (new_id(), Vec::new());
                for t in touched {
                    match fs.iter_mut().find(|f| f.path == t.path) {
                        Some(f) => (f.after, f.gone) = (t.after, t.gone),
                        None => fs.push(t),
                    }
                }
            }
            c.id
        }
        None => {
            let c = Checkpoint { conv, ..Checkpoint::new(question.to_string(), touched.collect()) };
            let id = c.id;
            list.push(c);
            id
        }
    };
    keep_then(ed, then);
    id
}

/// `paths` of the user's own step `step` weren't written after all: out of it (and the step, left with none).
pub fn own_drop(ed: &mut Editor, step: u64, paths: &[PathBuf]) {
    if paths.is_empty() {
        return;
    }
    let list = &mut ed.workspace.claude.checkpoints.list;
    if let Some(c) = list.iter_mut().find(|c| c.id == step)
        && let Files::Here(fs) = &mut c.files
    {
        fs.retain(|t| !paths.contains(&t.path));
        // (written again: a save under way for its texts as they were has more files than it now — `filed`)
        c.texts.clear();
    }
    list.retain(|c| c.id != step || !c.files.is_empty());
    keep(ed);
}

/// What a run of Claude's changed in the checkout (`snapshot.rs`) joins the turn, as written unseen: each
/// file's text before this turn kept the first time (one the turn touched already keeps its own), a deleted
/// one marked gone, one that isn't text kept by its blobs. What they are after is read when the turn ends.
pub fn record_run(ed: &mut Editor, changes: Vec<crate::snapshot::Change>) {
    add_run(&mut ed.workspace.claude.checkpoints.turn, changes);
    keep(ed);
}

/// `record_run` when the turn it ran in is over (stopped meanwhile): a checkpoint of its own, called
/// `question`, with HEAD's move in it said (`head`). The files row (`checkpoint::finish`).
pub fn record_run_alone(
    ed: &mut Editor,
    question: &str,
    changes: Vec<crate::snapshot::Change>,
    head: Vec<crate::snapshot::HeadMove>,
) -> Option<String> {
    let mut files = Vec::new();
    add_run(&mut files, changes);
    finish(ed, question, files, head)
}

fn add_run(turn: &mut Vec<Touched>, changes: Vec<crate::snapshot::Change>) {
    for c in changes {
        let text = |b: &Option<crate::snapshot::Blob>| {
            b.as_ref().and_then(|b| b.text.as_deref()).map(Rope::from_str).unwrap_or_default()
        };
        let blobs = Bytes {
            before: c.before.as_ref().map(|b| b.id.clone()),
            after: c.after.as_ref().map(|b| b.id.clone()),
            lost: None,
        };
        let (after, gone, is_text) = (text(&c.after), c.after.is_none(), c.text());
        match turn.iter_mut().find(|t| t.path == c.path) {
            Some(t) => {
                (t.unseen, t.gone, t.after) = (true, gone, after);
                // (its blob before stays this turn's first; one that was text before the run: from the run's — its
                // text before the turn kept beside it, what the turn's end settles it by: `settle_blobs`)
                match (&mut t.bytes, is_text) {
                    (Some(b), _) => b.after = blobs.after,
                    (None, false) => t.bytes = Some(blobs),
                    (None, true) => {}
                }
            }
            None => {
                // (one that isn't text has no text before — never an empty one: its blob says what it was)
                let before = c.before.as_ref().filter(|_| is_text).map(|_| text(&c.before));
                let bytes = (!is_text).then_some(blobs);
                turn.push(Touched { path: c.path, before, after, unseen: true, gone, bytes });
            }
        }
    }
}

/// A change of its own accepted in document `id` (a `space i` answer): one step.
pub fn single(ed: &mut Editor, id: crate::document::DocId, before: Rope, what: &str) {
    let Some((path, after)) =
        ed.docs.iter().find(|d| d.id == id).and_then(|d| Some((d.path.clone()?, d.text.clone())))
    else {
        return;
    };
    if before == after {
        return;
    }
    let question = what.lines().next().unwrap_or_default().to_string();
    let files = vec![Touched { path, before: Some(before), after, ..Default::default() }];
    let conv = conversation(&ed.workspace);
    ed.workspace.claude.checkpoints.list.push(Checkpoint { conv, ..Checkpoint::new(question, files) });
    keep(ed);
}

/// The chat turn ended: its changes become a checkpoint (a chat row says so). The files it changed, as
/// the row names them (`lib.rs, sub.rs` · `4 files`), if any.
pub fn turn_over(ed: &mut Editor, question: &str) -> Option<String> {
    ed.workspace.claude.checkpoints.carried.clear();
    ed.workspace.claude.checkpoints.told.clear();
    let files = std::mem::take(&mut ed.workspace.claude.checkpoints.turn);
    let head = std::mem::take(&mut ed.workspace.claude.checkpoints.head);
    finish(ed, question, files, head)
}

/// The chat turn ended having written files unseen (review at the end): what they are now is read on a
/// worker, then the checkpoint is made and `chat::turn_gathered` takes it on. False: nothing unseen —
/// `turn_over` it is.
pub fn gather(ed: &mut Editor, question: &str) -> bool {
    if !ed.workspace.claude.checkpoints.turn.iter().any(|t| t.unseen) {
        return false;
    }
    ed.workspace.claude.checkpoints.carried.clear();
    ed.workspace.claude.checkpoints.told.clear();
    let files = std::mem::take(&mut ed.workspace.claude.checkpoints.turn);
    let head = std::mem::take(&mut ed.workspace.claude.checkpoints.head);
    ed.workspace.claude.checkpoints.gathering = true;
    let question = question.to_string();
    // (one kept by its blobs: with its root's repository, its text before the turn if an edit kept one, its blobs)
    let roots = ed.workspace.roots();
    let seen: Vec<(PathBuf, Option<Blobbed>)> = files
        .iter()
        .filter(|t| t.unseen)
        .map(|t| {
            let blobs = t.bytes.as_ref().map(|b| {
                let root = roots.all().find(|r| crate::paths::written_inside(r, &t.path));
                (root.unwrap_or(&ed.workspace.root).to_path_buf(), t.before.clone(), b.clone())
            });
            (t.path.clone(), blobs)
        })
        .collect();
    ed.events.jobs().spawn(move || {
        // (gone: nothing in it now · unreadable: as it was kept) — with its fingerprint, for open buffers
        let now: Vec<Gathered> = seen
            .iter()
            .map(|(p, blobs)| {
                let stamp = crate::disk::stamp(p);
                let read = std::fs::read(p);
                let settled = match (&read, blobs) {
                    (_, None) => None,
                    (Ok(b), Some((root, before, bytes))) => {
                        Some(settle_blobs(root, p, before.as_ref(), bytes, Some(b)))
                    }
                    (Err(e), Some((root, before, bytes))) if e.kind() == std::io::ErrorKind::NotFound => {
                        Some(settle_blobs(root, p, before.as_ref(), bytes, None))
                    }
                    (Err(_), Some(_)) => None,
                };
                let (text, gone) = match read {
                    Ok(b) => (String::from_utf8(b).ok().map(|s| Rope::from_str(&s)), false),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Some(Rope::new()), true),
                    Err(_) => (None, false),
                };
                (text, stamp, gone, settled)
            })
            .collect();
        move |ed: &mut Editor| {
            let mut files = files;
            for ((p, _), (now, stamp, gone, settled)) in seen.iter().zip(now) {
                let t = files.iter_mut().find(|t| &t.path == p);
                // (kept by its blobs: as text now where both its ends are, else by the turn's ends' blobs)
                if let (Some(t), Some(s)) = (t, settled) {
                    match s {
                        Settled::Text(before) => (t.before, t.bytes) = (before, None),
                        Settled::Blobs(b) => t.bytes = Some(b),
                    }
                }
                let Some(now) = now else { continue };
                // An open buffer shows what Claude wrote before the review diffs it
                ed.disk_take(p, &now, stamp);
                // (one that isn't text keeps its blobs: `after` says nothing of it)
                if let Some(t) = files.iter_mut().find(|t| &t.path == p && t.bytes.is_none()) {
                    (t.after, t.gone) = (now, gone);
                }
            }
            ed.workspace.claude.checkpoints.gathering = false;
            let n = finish(ed, &question, files, head);
            crate::chat::turn_gathered(ed, n);
        }
    });
    true
}

/// A file the turn kept by its blobs (a run's change of a file that isn't text, or past `snapshot::TEXT_MAX`), as
/// the turn ends — a worker's (git). What it was before the turn: the text an edit of Claude's kept before the run
/// (`before`), else its blob before; what it is: `now` (None: gone). Text at both ends (up to `WHOLE_MAX`): kept as
/// any text file is — undo takes it back to the turn's first text over your edits since, Claude's edits after the
/// run the turn's too (#281). Else by its blobs, both the turn's ends: the text an edit kept written as one, what it
/// is now hashed from the file. A blob before the repository no longer has is `lost`: a step says so, never writes.
fn settle_blobs(
    root: &Path,
    path: &Path,
    before: Option<&Rope>,
    bytes: &Bytes,
    now: Option<&[u8]>,
) -> Settled {
    let text = |b: &[u8]| -> Option<Rope> {
        (b.len() as u64 <= WHOLE_MAX && !b.contains(&0))
            .then(|| std::str::from_utf8(b).ok().map(Rope::from_str))?
    };
    // (Ok(None): the turn made it · Err(None): no text · Err(Some): can't be had, why)
    let was = match (before, &bytes.before) {
        (Some(t), _) => Ok(Some(t.clone())),
        (None, None) => Ok(None),
        (None, Some(id)) => match crate::snapshot::blob(root, id, WHOLE_MAX) {
            Ok(b) => b.as_deref().and_then(text).map(Some).ok_or(None),
            Err(None) => Err(Some("what it was before the turn isn't kept (git no longer has it)".into())),
            // (git couldn't say just now: by its blobs, the one before still the run's — never lost for it)
            Err(Some(e)) => {
                let shown = crate::chat::shown(root, path);
                crate::log::line(
                    "turns",
                    "info",
                    &format!("{shown}: its blob before not read ({e}) — kept by blobs"),
                );
                Err(None)
            }
        },
    };
    let is_text = now.is_none_or(|b| text(b).is_some());
    let lost = match was {
        Ok(was) if is_text => return Settled::Text(was),
        Ok(_) | Err(None) => None,
        Err(Some(why)) => Some(why),
    };
    let mut b = Bytes { lost, ..bytes.clone() };
    if let Some(t) = before {
        match crate::snapshot::hash(root, path, t.to_string().as_bytes()) {
            Ok(id) => b.before = Some(id),
            Err(e) => b.lost = Some(format!("what it was before the turn isn't kept ({e})")),
        }
    }
    // (its blob now unknown: the run's — a step finds it changed since and leaves it, said)
    match now {
        None => b.after = None,
        Some(now) => b.after = crate::snapshot::hash(root, path, now).ok().or(b.after),
    }
    Settled::Blobs(b)
}

/// The turn's `files` as a checkpoint, with HEAD's moves in it said (`head`) — the files row (`lib.rs,
/// sub.rs` · `4 files`), None if none changed.
fn finish(
    ed: &mut Editor,
    question: &str,
    files: Vec<Touched>,
    head: Vec<crate::snapshot::HeadMove>,
) -> Option<String> {
    // (a file changed and changed back isn't a change)
    let files: Vec<Touched> = files.into_iter().filter(Touched::changed).collect();
    if files.is_empty() {
        // (the turn kept as in flight goes)
        keep(ed);
        return None;
    }
    let names: Vec<String> = files
        .iter()
        .map(|f| f.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
        .collect();
    let n = if names.len() <= 2 { names.join(", ") } else { format!("{} files", names.len()) };
    let question = question.lines().next().unwrap_or_default().to_string();
    let conv = conversation(&ed.workspace);
    ed.workspace.claude.checkpoints.list.push(Checkpoint { head, conv, ..Checkpoint::new(question, files) });
    keep(ed);
    Some(n)
}

/// Whether there's a turn to undo (the chat's `C-z` hint).
pub fn can_undo(ed: &Editor) -> bool {
    ed.workspace.claude.checkpoints.list.iter().any(|c| !c.undone)
}

/// `C-z` · `:undo-turn` (`undo`) · `:redo-turn`.
pub fn step(ed: &mut Editor, undo: bool) -> Result<(), String> {
    if !ed.workspace.claude.checkpoints.turn.is_empty()
        || ed.workspace.claude.chat.as_ref().is_some_and(|c| c.busy())
    {
        return Err("Claude is still working — stop it (C-c in the chat) or wait for the turn to end".into());
    }
    if ed.workspace.claude.checkpoints.gathering {
        return Err("reading what Claude wrote this turn — a moment".into());
    }
    if ed.workspace.claude.checkpoints.reading {
        return Err("reading that turn's changes — a moment".into());
    }
    if ed.workspace.claude.checkpoints.loading.is_some() {
        return Err("reading that turn's big files — a moment".into());
    }
    if ed.workspace.claude.checkpoints.putting {
        return Err("writing that turn's files back — a moment".into());
    }
    let list = &ed.workspace.claude.checkpoints.list;
    let i = if undo {
        list.iter().rposition(|c| !c.undone).ok_or("no turn of Claude's changed files — nothing to undo")?
    } else {
        list.iter().rposition(|c| c.undone).ok_or("no undone turn to bring back")?
    };
    // (kept on disk only: read first, then this step again — the turn it finds then)
    let Some(files) = list[i].files.here().map(<[Touched]>::to_vec) else {
        with_texts(ed, i, move |ed, _| {
            if let Err(e) = step(ed, undo) {
                ed.set_warning(e);
            }
        });
        return Ok(());
    };
    // (its checkout being moved onto its base: git's rebase would stop on a write, or its abort put the old text
    // back — refused, nothing queued)
    let then = if undo { "take it back" } else { "bring it back" };
    if let Some(why) = crate::keepclose::refused(ed, files.iter().map(|f| f.path.as_path()), then) {
        return Err(why);
    }
    let question = list[i].question.clone();
    // (a big file's buffer loads in the background: every one the step writes as text is in first, then this step
    // again — never the turn stepped with one of them left as it was, "still loading")
    let mut waits = Vec::new();
    for f in files.iter().filter(|f| f.bytes.is_none()) {
        let open = match ed.docs.by_path(&f.path).map(|d| (d.id, d.loading)) {
            Some(d) => Some(d),
            None if f.path.exists() && ed.open(&f.path).is_ok() => Some((ed.doc().id, ed.doc().loading)),
            None => None,
        };
        if let Some((id, true)) = open {
            waits.push((id, f.path.clone()));
        }
    }
    if !waits.is_empty() {
        ed.workspace.claude.checkpoints.loading = Some((undo, waits));
        ed.set_status(format!("reading “{}”'s big files — a moment", quote(&question)));
        return Ok(());
    }
    let mut changed = Vec::new();
    let mut kept = 0;
    let mut problems = Vec::new();
    let mut blobs = Vec::new();
    for f in &files {
        // (not text: its blob written out on a worker — said there if it can't be)
        if let Some(b) = &f.bytes {
            if let Some(why) = &b.lost {
                problems.push(format!(
                    "{}: {why} — left as it is",
                    crate::chat::shown(&ed.workspace.root, &f.path)
                ));
                continue;
            }
            let (expect, put) = if undo { (&b.after, &b.before) } else { (&b.before, &b.after) };
            blobs.push((f.path.clone(), expect.clone(), put.clone()));
            changed.push(crate::chat::shown(&ed.workspace.root, &f.path));
            continue;
        }
        let after = (!f.gone).then_some(&f.after);
        let (from, to) = if undo { (after, f.before.as_ref()) } else { (f.before.as_ref(), after) };
        match file(ed, &f.path, from, to) {
            Ok((true, k)) => {
                changed.push(crate::chat::shown(&ed.workspace.root, &f.path));
                kept += k;
            }
            Ok((false, k)) => kept += k,
            Err(e) => problems.push(e),
        }
    }
    ed.workspace.claude.checkpoints.list[i].undone = undo;
    keep(ed);
    // (a turn read back for this step: its texts go again where its session is behind — the step needed them)
    shed(&mut ed.workspace, ed.behind);
    put_blobs(ed, blobs);
    let what = if undo { "undid" } else { "brought back" };
    let files = match changed.as_slice() {
        [one] => one.clone(),
        many => format!("{} files", many.len()),
    };
    let mut msg = format!("{what} “{}” — {files}{}", quote(&question), if undo { " as before" } else { "" });
    if kept > 0 {
        msg.push_str(&format!(" · {kept} change(s) left as they are: you edited there since"));
    }
    ed.workspace.claude.checkpoints.tell.push(format!(
        "The user {} your changes from the turn \"{}\" in {} — {}.",
        if undo { "undid" } else { "brought back" },
        question,
        if changed.is_empty() { "no file".to_string() } else { changed.join(", ") },
        if undo { "those files are as they were before that turn" } else { "they're applied again" },
    ));
    // The chat has the question right above: its row says only what happened
    if let Some(c) = &mut ed.workspace.claude.chat {
        let row = match (undo, changed.as_slice()) {
            (_, []) => format!("{what} the turn — nothing to change"),
            (true, _) => format!("undid the turn ({files}) · space U brings it back"),
            (false, _) => format!("brought the turn back ({files})"),
        };
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Note, row, ""));
    }
    if problems.is_empty() {
        ed.set_success(format!("{msg}{}", if undo { " · space U brings it back" } else { "" }));
    } else {
        ed.set_warning(format!("{msg} · {}", problems.join("; ")));
    }
    Ok(())
}

/// Document `id`'s text came in (`Editor::finish_loading`; `read`: it was, not an error): a step waiting for its
/// turn's big files (`step`) goes on once the last of them is in — one of them that couldn't be read leaves the turn
/// as it is, said; another buffer's load, read or not, isn't the step's.
pub fn loaded(ed: &mut Editor, id: crate::document::DocId, read: bool) {
    let Some((undo, waits)) = &mut ed.workspace.claude.checkpoints.loading else { return };
    let Some(at) = waits.iter().position(|w| w.0 == id) else { return };
    let (undo, (_, path), left) = (*undo, waits.remove(at), waits.len());
    if read && left > 0 {
        return;
    }
    ed.workspace.claude.checkpoints.loading = None;
    if !read {
        let shown = crate::chat::shown(&ed.workspace.root, &path);
        return ed.set_warning(format!("{shown} couldn't be read — the turn left as it is"));
    }
    if let Err(e) = step(ed, undo) {
        ed.set_warning(e);
    }
}

/// Files that aren't text, each `(path, what it should be now, what it goes to)` as git's blobs (None: no
/// file) — written out on a worker; one changed since is left as it is, and said.
fn put_blobs(ed: &mut Editor, blobs: Vec<(PathBuf, Option<String>, Option<String>)>) {
    if blobs.is_empty() {
        return;
    }
    let root = ed.workspace.root.clone();
    ed.workspace.claude.checkpoints.putting = true;
    ed.events.jobs().spawn(move || {
        let problems: Vec<String> = blobs
            .iter()
            .filter_map(|(p, now, to)| crate::snapshot::put(&root, p, now.as_deref(), to.as_deref()).err())
            .collect();
        move |ed: &mut Editor| {
            ed.workspace.claude.checkpoints.putting = false;
            if !problems.is_empty() {
                ed.set_warning(format!("not taken back: {}", problems.join("; ")));
            }
        }
    });
}

/// The question, short enough for a toast (`src/sub.rs 새 파일에 sub(a…`).
fn quote(question: &str) -> String {
    const MAX: usize = 24;
    let q: String = question.chars().take(MAX).collect();
    if question.chars().count() > MAX { format!("{}…", q.trim_end()) } else { q }
}

/// One file from `from` to `to` over what it is now (None = no file). (changed, changes kept).
pub(crate) fn file(
    ed: &mut Editor,
    path: &Path,
    from: Option<&Rope>,
    to: Option<&Rope>,
) -> Result<(bool, usize), String> {
    let shown = crate::chat::shown(&ed.workspace.root, path);
    let open = ed.docs.iter().any(|d| d.path.as_deref() == Some(path));
    if !open && !path.exists() && from.is_some() {
        return Err(format!("{shown} is gone"));
    }
    ed.open(path).map_err(|e| format!("{shown}: {e:#}"))?;
    if ed.doc().loading {
        return Err(format!("{shown} is still loading"));
    }
    let empty = Rope::new();
    let now = ed.doc().text.to_string();
    let (target, kept) = rebase(&from.unwrap_or(&empty).to_string(), &to.unwrap_or(&empty).to_string(), &now);
    // In the project or one of its folders, written through no link from that root (`beneath.rs`): a run of
    // Claude's may have swapped a folder for a link to outside since the turn. A file outside them (accepted on its
    // review) by its path.
    let roots = ed.workspace.roots();
    let root = roots.all().find(|r| crate::paths::written_inside(r, path)).map(Path::to_path_buf);
    // A file the turn made, with nothing else in it now: it goes
    if to.is_none() && kept == 0 && target.is_empty() {
        let id = ed.doc().id;
        match &root {
            Some(r) => remove_beneath(r, path),
            None => std::fs::remove_file(path),
        }
        .map_err(|e| format!("{shown}: {e}"))?;
        ed.close_doc(id);
        return Ok((true, 0));
    }
    if target == now {
        return Ok((false, kept));
    }
    let clean = !ed.doc().is_modified();
    let changes: Vec<Change> = crate::editdiff::line_changes(&now, &target)
        .into_iter()
        .map(|(from, to, _, insert)| Change { from, to, insert })
        .collect();
    let tx = Transaction::new(changes);
    ed.with_group(|cx| {
        let doc = cx.editor.doc_mut();
        let sel = tx.map_selection(doc.selection());
        doc.apply_with(&tx, sel);
        // (a turn stepped back on your key — not your edit)
        doc.own.not_yours();
    });
    // The disk matches again (unsaved edits of the user's stay unsaved)
    if clean {
        let id = ed.doc().id;
        ed.flush_saves(); // (written here — a background save of it finishes first)
        match root {
            // (a folder a run deleted made again on the way)
            Some(r) => {
                let doc = ed.doc_mut();
                let at = doc.check_save(false).map_err(|e| format!("{shown}: {e:#}"))?;
                let stamp = crate::saver::write_beneath(&r, &at, &doc.text, &|| {})?;
                let version = doc.version();
                doc.saved_at(version, stamp);
            }
            None => {
                // (a file a run deleted, with its folder: the folder first)
                if let Some(dir) = path.parent().filter(|d| !d.exists()) {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{shown}: {e}"))?;
                }
                ed.doc_mut().save().map_err(|e| format!("{shown}: {e:#}"))?;
            }
        }
        ed.lsp_did_save(id);
        ed.undo_persist(id);
    }
    Ok((true, kept))
}

/// `path` removed from its folder, opened from `root`'s real path through no link (`beneath.rs`) — a link
/// in the way is refused, never gone through.
fn remove_beneath(root: &Path, path: &Path) -> std::io::Result<()> {
    let base = crate::beneath::Dir::root(root)?;
    let rel = path.strip_prefix(&base.at).map_err(|_| std::io::Error::other("not in the project"))?;
    let name = rel.file_name().ok_or_else(|| std::io::Error::other("no file named"))?;
    base.folder(rel.parent().unwrap_or(Path::new("")), false)?.remove(name)
}

// ── Kept on disk ────────────────────────────────────────────────────────────

/// The newest turns whose texts the session in front holds once their files have them (`shed`) — the rest are
/// read back as a step or a review asks.
pub(crate) const KEEP: usize = 10;

/// What a project's kept turns may hold on disk: now (seconds since the epoch, `clock::system`, read on the loop),
/// how many days a conversation is kept (`cleanupPeriodDays` as the chat's claude reads it; None: not known — no
/// claude started for it — so none goes by age), each conversation's last activity tarae kept (`chats::Saved`'s
/// `updated`, by claude's id) and the bytes their texts may take (`llm.turns-max-mb`).
#[derive(Clone, Debug)]
pub struct Room {
    pub now: u64,
    pub days: Option<u64>,
    pub active: Vec<(String, u64)>,
    pub max: u64,
}

impl Room {
    /// Every turn kept, whatever its age and size (a project's turns forgotten; tests).
    pub const ANY: Room = Room { now: 0, days: None, active: Vec::new(), max: u64::MAX };
}

/// What a save did: written (false: a later one was, or the disk said no), the texts it wrote for turns that had
/// none written (by id), and the turns it let go — older than their conversation's days (`old`), the oldest past
/// `Room.max` (`big`).
#[derive(Debug, Default)]
pub struct Saved {
    pub written: bool,
    pub texts: Vec<(u64, Vec<Texts>)>,
    pub old: Vec<u64>,
    pub big: Vec<u64>,
}

/// The question of a turn tarae ended in the middle of, when it didn't say.
const CUT_SHORT: &str = "a turn cut short";

pub fn default_dir() -> Option<PathBuf> {
    Some(crate::config::state_dir()?.join("turns"))
}

/// A project's turns as kept: the checkpoints, and the turn in flight with its question.
#[derive(Debug, Default)]
pub struct Stored {
    pub list: Vec<Checkpoint>,
    pub turn: Vec<Touched>,
    /// The turn in flight's HEAD moves (`Checkpoints.head`) — a turn cut short keeps them (review finding).
    pub head: Vec<crate::snapshot::HeadMove>,
    pub question: String,
    /// When it was taken (`chats::seq`) — a slower save of an older one doesn't replace it.
    pub seq: u64,
}

/// Saves read, compare and write the file one at a time.
static SAVING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Holds every save back while it's kept (tests: what waits for one).
#[cfg(test)]
pub fn hold_saves() -> std::sync::MutexGuard<'static, ()> {
    SAVING.lock().unwrap_or_else(|e| e.into_inner())
}

/// `t` as kept — None: its path is no text, a file no store holds (a turn kept comes back without it).
fn touched_json(t: &Touched) -> Option<Value> {
    let bytes = t.bytes.as_ref().map(bytes_json);
    Some(
        json!({ "path": t.path.to_str()?, "before": t.before.as_ref().map(Rope::to_string), "after": t.after.to_string(),
            "unseen": t.unseen, "gone": t.gone, "bytes": bytes }),
    )
}

fn bytes_json(b: &Bytes) -> Value {
    json!({ "before": b.before, "after": b.after, "lost": b.lost })
}

fn bytes_of(v: &Value) -> Option<Bytes> {
    let s = |b: &Value| b.as_str().map(String::from);
    v.is_object().then(|| Bytes { before: s(&v["before"]), after: s(&v["after"]), lost: s(&v["lost"]) })
}

fn touched_of(v: &Value) -> Option<Touched> {
    Some(Touched {
        path: PathBuf::from(v["path"].as_str()?),
        before: v["before"].as_str().map(Rope::from_str),
        after: Rope::from_str(v["after"].as_str()?),
        unseen: v["unseen"].as_bool().unwrap_or(false),
        gone: v["gone"].as_bool().unwrap_or(false),
        bytes: bytes_of(&v["bytes"]),
    })
}

fn files_of(v: &Value) -> Vec<Touched> {
    v.as_array().into_iter().flatten().filter_map(touched_of).collect()
}

/// `k` as kept — None as `touched_json`.
fn kept_json(k: &Kept, (before, after): &Texts) -> Option<Value> {
    let bytes = k.bytes.as_ref().map(bytes_json);
    Some(json!({ "path": k.path.to_str()?, "unseen": k.unseen, "gone": k.gone, "bytes": bytes,
            "before": before.as_ref().map(Text::json), "after": after.json() }))
}

fn texts_of(v: &Value) -> Option<Texts> {
    let before = match &v["before"] {
        Value::Null => None,
        b => Some(Text::from_json(b)?),
    };
    Some((before, Text::from_json(&v["after"])?))
}

fn kept_of(v: &Value) -> Option<Kept> {
    Some(Kept {
        path: PathBuf::from(v["path"].as_str()?),
        unseen: v["unseen"].as_bool().unwrap_or(false),
        gone: v["gone"].as_bool().unwrap_or(false),
        bytes: bytes_of(&v["bytes"]),
    })
}

/// The folder the project at `root`'s turns are kept in (`<dir>/<project hash>/`).
fn folder(dir: &Path, root: &Path) -> PathBuf {
    crate::config::state_file(dir, root).with_extension("")
}

/// The index of the project's kept turns.
fn index_file(dir: &Path, root: &Path) -> PathBuf {
    folder(dir, root).join("index.json")
}

/// The folder of a project's texts, each a file by its digest (`Text::name`).
fn texts_dir(folder: &Path) -> PathBuf {
    folder.join("texts")
}

/// `files`' texts (as kept, `texts`) from the project's `folder` — Err: whether they're gone for good (no such
/// file, or it isn't the text: written whole or not at all, `write_atomic`), and why. Any other failure to read
/// one (too many files open, a permission) passes: the turn stays, asked again later.
fn read_texts(folder: &Path, files: &[Kept], texts: &[Texts]) -> Result<Vec<Touched>, (bool, String)> {
    if texts.len() != files.len() {
        return Err((true, "not its files' texts".to_string()));
    }
    let dir = texts_dir(folder);
    let read = |t: &Text| -> Result<Rope, (bool, String)> {
        let gone = |e: &std::io::Error| {
            matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidData)
        };
        let body = std::fs::read_to_string(dir.join(t.name())).map_err(|e| (gone(&e), e.to_string()))?;
        match body.len() as u64 == t.len {
            true => Ok(Rope::from_str(&body)),
            false => Err((true, "a text of another size".to_string())),
        }
    };
    files
        .iter()
        .zip(texts)
        .map(|(k, (before, after))| {
            Ok(Touched {
                path: k.path.clone(),
                before: before.as_ref().map(read).transpose()?,
                after: read(after)?,
                unseen: k.unseen,
                gone: k.gone,
                bytes: k.bytes.clone(),
            })
        })
        .collect()
}

/// `text` in `dir` by its digest (`known`: already taken) — written only where no file has it yet. None: the disk
/// said no.
fn put_text(dir: &Path, text: &Rope, known: Option<Text>) -> Option<Text> {
    if let Some(t) = known
        && std::fs::symlink_metadata(dir.join(t.name())).is_ok()
    {
        return Some(t);
    }
    let body = text.to_string();
    let t = known.unwrap_or_else(|| Text::of(&body));
    let path = dir.join(t.name());
    if std::fs::symlink_metadata(&path).is_err() {
        crate::disk::write_atomic(&path, |w| std::io::Write::write_all(w, body.as_bytes())).ok()?;
    }
    Some(t)
}

/// The index's `seq` — it starts the file (`save`), so only its first bytes are read. 0: no index, or not one.
fn seq_of(index: &Path) -> u64 {
    use std::io::Read as _;
    let mut head = Vec::new();
    let Ok(f) = std::fs::File::open(index) else { return 0 };
    let _ = f.take(40).read_to_end(&mut head);
    let head = String::from_utf8_lossy(&head);
    let digits: String =
        head.strip_prefix("{\"seq\":").unwrap_or_default().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

/// The index: its turns as kept (paths only), the turn in flight (texts and all).
fn read_index(index: &Path) -> Stored {
    let Ok(body) = std::fs::read_to_string(index) else { return Stored::default() };
    let Ok(v) = serde_json::from_str::<Value>(&body) else { return Stored::default() };
    let list = v["list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            // (a turn with a file it can't say whole is left out — its texts couldn't be found)
            let files = c["files"].as_array()?;
            Some(Checkpoint {
                question: c["question"].as_str().unwrap_or_default().to_string(),
                files: Files::Kept(files.iter().map(kept_of).collect::<Option<_>>()?),
                undone: c["undone"].as_bool().unwrap_or(false),
                seen: c["seen"].as_bool().unwrap_or(true),
                id: c["id"].as_u64()?,
                at: c["at"].as_u64()?,
                conv: c["conv"].as_str().map(String::from),
                texts: files.iter().map(texts_of).collect::<Option<_>>()?,
                head: heads_of(&c["head"]),
            })
        })
        .filter(|c| !c.files.is_empty())
        .collect();
    Stored {
        list,
        turn: files_of(&v["turn"]),
        head: heads_of(&v["head"]),
        question: v["question"].as_str().unwrap_or_default().to_string(),
        seq: v["seq"].as_u64().unwrap_or(0),
    }
}

/// HEAD moves as kept (`HeadMove::to_json`) — one unreadable is left out.
fn heads_of(v: &Value) -> Vec<crate::snapshot::HeadMove> {
    v.as_array().into_iter().flatten().filter_map(crate::snapshot::HeadMove::from_json).collect()
}

/// The project at `root`'s turns kept in `dir`: the index's (paths only) — nothing if none, or unreadable.
pub(crate) fn load(dir: &Path, root: &Path) -> Stored {
    read_index(&index_file(dir, root))
}

/// `load` with every turn's texts read too (tests: what's kept, whole).
#[cfg(test)]
pub fn load_whole(dir: &Path, root: &Path) -> Stored {
    let mut s = load(dir, root);
    for c in &mut s.list {
        // (one a save removed meanwhile stays kept)
        if let Files::Kept(k) = &c.files
            && let Ok(texts) = read_texts(&folder(dir, root), k, &c.texts)
        {
            c.files = Files::Here(texts);
        }
    }
    s
}

/// Written — unless what's there was taken later (on a worker): each text a turn holds in memory a file of its own
/// by content where none has it yet, the turns `room` doesn't hold let go (those whose conversation was last active
/// longer ago than its days, then the oldest while the texts the rest name take more than its bytes — the newest
/// always stays), the index, then every text no turn left names goes, and the old format's turns (`old_turn`).
fn save(dir: &Path, root: &Path, mut s: Stored, room: Room) -> Saved {
    let _one = SAVING.lock().unwrap_or_else(|e| e.into_inner());
    let (at, index) = (folder(dir, root), index_file(dir, root));
    let mut out = Saved::default();
    let texts = texts_dir(&at);
    if seq_of(&index) > s.seq || std::fs::create_dir_all(&texts).is_err() {
        return out;
    }
    let write = |path: &Path, body: String| {
        crate::disk::write_atomic(path, |w| std::io::Write::write_all(w, body.as_bytes())).is_ok()
    };
    for c in &mut s.list {
        let Files::Here(files) = &c.files else { continue };
        let fresh = c.texts.len() != files.len();
        let known = |i: usize| c.texts.get(i).filter(|_| !fresh);
        let mut put = Vec::with_capacity(files.len());
        for (i, t) in files.iter().enumerate() {
            let before = match &t.before {
                Some(b) => match put_text(&texts, b, known(i).and_then(|k| k.0)) {
                    Some(b) => Some(b),
                    None => return out,
                },
                None => None,
            };
            let Some(after) = put_text(&texts, &t.after, known(i).map(|k| k.1)) else { return out };
            put.push((before, after));
        }
        if fresh {
            out.texts.push((c.id, put.clone()));
            c.texts = put;
        }
    }
    // (a kept turn whose texts aren't known can't be found again: out)
    s.list.retain(|c| c.texts.len() == c.files.len());
    // By age: as its conversation goes, counted from its last activity (claude's `cleanupPeriodDays` counts from a
    // conversation's last write) — its newest turn, or later what tarae kept of it; a turn of none as the project's
    // latest. A day at least, and the newest turn always stays.
    if let Some(days) = room.days {
        let from = room.now.saturating_sub(days.max(1).saturating_mul(86_400));
        let mut last: std::collections::HashMap<Option<&str>, u64> = std::collections::HashMap::new();
        for c in &s.list {
            let e = last.entry(c.conv.as_deref()).or_default();
            *e = (*e).max(c.at);
        }
        for (id, at) in &room.active {
            if let Some(e) = last.get_mut(&Some(id.as_str())) {
                *e = (*e).max(*at);
            }
        }
        let project = last.values().chain(room.active.iter().map(|(_, at)| at)).max().copied().unwrap_or(0);
        let newest = s.list.last().map(|c| c.id);
        let old: Vec<u64> = s
            .list
            .iter()
            .filter(|c| Some(c.id) != newest)
            .filter(|c| c.conv.as_deref().map_or(project, |id| last[&Some(id)]) < from)
            .map(|c| c.id)
            .collect();
        s.list.retain(|c| !old.contains(&c.id));
        out.old = old;
    }
    // By size, oldest first: a text several turns name counted once, gone with the last of them
    let names = |c: &Checkpoint| -> Vec<Text> {
        c.texts.iter().flat_map(|(b, a)| b.iter().chain([a])).copied().collect()
    };
    let mut held: std::collections::HashMap<[u8; 32], (u64, usize)> = std::collections::HashMap::new();
    for t in s.list.iter().flat_map(names) {
        held.entry(t.hash).or_insert((t.len, 0)).1 += 1;
    }
    let mut total: u64 = held.values().map(|(len, _)| len).sum();
    let mut cut = 0;
    while total > room.max && cut + 1 < s.list.len() {
        for t in names(&s.list[cut]) {
            if let Some(e) = held.get_mut(&t.hash) {
                e.1 -= 1;
                if e.1 == 0 {
                    total -= e.0;
                    held.remove(&t.hash);
                }
            }
        }
        out.big.push(s.list[cut].id);
        cut += 1;
    }
    s.list.drain(..cut);
    let list: Vec<Value> = s
        .list
        .iter()
        .map(|c| {
            let files: Vec<Value> =
                c.files.kept().iter().zip(&c.texts).filter_map(|(k, t)| kept_json(k, t)).collect();
            let head: Vec<Value> = c.head.iter().map(crate::snapshot::HeadMove::to_json).collect();
            let (id, q, seen) = (c.id, &c.question, c.seen);
            json!({ "id": id, "at": c.at, "conv": c.conv, "question": q, "files": files, "undone": c.undone,
                    "seen": seen, "head": head })
        })
        .collect();
    let turn: Vec<Value> = s.turn.iter().filter_map(touched_json).collect();
    let head: Vec<Value> = s.head.iter().map(crate::snapshot::HeadMove::to_json).collect();
    // (`root` null where it's no text: nothing reads it, the folder's name is the key)
    let rest =
        json!({ "root": root.to_str(), "list": list, "turn": turn, "head": head, "question": s.question })
            .to_string();
    // (`seq` first: a save reads only it — `seq_of`)
    if !write(&index, format!("{{\"seq\":{},{}", s.seq, &rest[1..])) {
        return out;
    }
    // The texts no turn names (all under `SAVING`: an older save that comes after this one writes nothing —
    // `seq_of` above — so it never removes a newer turn's texts, nor names one removed)
    let named: std::collections::HashSet<String> =
        held.keys().map(|hash| Text { hash: *hash, len: 0 }.name()).collect();
    for e in std::fs::read_dir(&texts).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        // (a text's digest: not a file being written)
        let a_text = name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit());
        if a_text && !named.contains(&name) {
            let _ = std::fs::remove_file(e.path());
        }
    }
    // The old format's turns beside the index — no reader of them: gone, through no link, only what's named so
    if let Ok(d) = crate::beneath::Dir::root(&at) {
        for (name, kind) in d.entries().unwrap_or_default() {
            if matches!(kind, crate::beneath::Kind::File) && old_turn(&name) {
                let _ = d.remove(&name);
            }
        }
    }
    out.written = true;
    out
}

/// A turn as an earlier tarae kept it, texts and all: `<id>.json` beside the index (no digests in its index).
fn old_turn(name: &std::ffi::OsStr) -> bool {
    let id = name.to_str().and_then(|n| n.strip_suffix(".json")).unwrap_or_default();
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
}

/// The turns as they are now, kept (the texts are written out on a worker).
pub(crate) fn keep(ed: &Editor) {
    keep_of(ed, &ed.workspace);
}

/// `keep` for session `l` — one that's ending (out of the editor already) too.
pub(crate) fn keep_of(ed: &Editor, l: &crate::workspace::Workspace) {
    if let Some(save) = saving(ed, l) {
        ed.events.jobs().spawn_kept(move || {
            let s = save();
            move |ed: &mut Editor| {
                if saved(ed, s) {
                    ed.unchanged = true;
                }
            }
        });
    }
}

/// Tests: session `l`'s turns written now, on this thread (`keep_of` writes on a worker, unseen by events).
#[cfg(test)]
pub(crate) fn keep_now(ed: &Editor, l: &crate::workspace::Workspace) {
    if let Some(save) = saving(ed, l) {
        save();
    }
}

/// The turns kept for the checkout in `root` forgotten (its session ended, the checkout removed) — written
/// over with none, later than any save of its still on its way (on a worker).
pub(crate) fn forget(ed: &Editor, root: PathBuf) {
    let Some(dir) = ed.turns_dir.clone() else { return };
    let none = Stored { seq: crate::chats::seq(), ..Default::default() };
    ed.events.jobs().background(move || _ = save(&dir, &root, none, Room::ANY));
}

/// `keep`, then `then` on the main loop once it's written (at once when nothing is kept — tests, or
/// before `restore` has them back). Drawn after, whatever the save said: `then` goes on with what was asked.
fn keep_then(ed: &mut Editor, then: impl FnOnce(&mut Editor) + Send + 'static) {
    let Some(save) = saving(ed, &ed.workspace) else { return then(ed) };
    ed.events.jobs().spawn_kept(move || {
        let s = save();
        move |ed: &mut Editor| {
            _ = saved(ed, s);
            then(ed);
        }
    });
}

/// Writing session `l`'s turns as they are now, within what a project may keep (for a worker) — None when they
/// aren't kept. How long a conversation is kept is read there, as the chat's claude reads it — where one was
/// started (else none goes by age: a project's own settings, maybe held back, aren't what it runs with).
fn saving(ed: &Editor, l: &crate::workspace::Workspace) -> Option<impl FnOnce() -> Saved + Send + 'static> {
    let dir = l.claude.checkpoints.dir.clone()?;
    let started = l.claude.chat.as_ref().is_some_and(|c| c.launched.is_some());
    let places = crate::idlecompact::Places::of_in(ed, l).filter(|_| started);
    let max = ed.config.llm.turns_max_mb.saturating_mul(1 << 20);
    let room = Room { now: now_s(), days: None, active: Vec::new(), max };
    let root = l.root.clone();
    let chats = ed.chats_dir.clone();
    let list = &l.claude.checkpoints.list;
    let question = l
        .claude
        .chat
        .as_ref()
        .and_then(|c| c.msgs.iter().rev().find(|m| m.role == crate::chat::Role::User))
        .map(|m| m.text.lines().next().unwrap_or_default().to_string())
        .unwrap_or_default();
    let s = Stored {
        list: list.to_vec(),
        turn: l.claude.checkpoints.turn.clone(),
        head: l.claude.checkpoints.head.clone(),
        question,
        seq: crate::chats::seq(),
    };
    Some(move || {
        let days = places.map(|p| p.cleanup_days().unwrap_or(crate::claudetrust::CLEANUP_DEFAULT));
        // (each conversation's last activity as tarae kept it — only where age decides; its times alone, never the
        // conversations' rows)
        let active = match (days, chats) {
            (Some(_), Some(chats)) => crate::chats::times(&chats, &root),
            _ => Vec::new(),
        };
        save(&dir, &root, s, Room { days, active, ..room })
    })
}

/// A save's word back: the texts it wrote are the turns' (`filed`), and the turns it let go leave their session —
/// past `llm.turns-max-mb` said, the first time since tarae started as a warning, after that in `:messages`. True:
/// it had nothing to say, nothing changed on screen (what runs after it may still change something).
fn saved(ed: &mut Editor, s: Saved) -> bool {
    if s.texts.is_empty() && s.old.is_empty() && s.big.is_empty() {
        return true;
    }
    if !s.texts.is_empty() {
        filed(ed, &s.texts);
    }
    let mut big = 0;
    for l in std::iter::once(&mut ed.workspace).chain(&mut ed.workspaces) {
        for id in s.old.iter().chain(&s.big) {
            let Some(at) = l.claude.checkpoints.list.iter().position(|c| c.id == *id) else { continue };
            match drop_turn(l, at) {
                true => big += usize::from(s.big.contains(id)),
                // (its review is open: kept, but its texts are gone from the disk — not filed until the next save
                // writes them again, so nothing lets them go meanwhile: `shed`)
                false => l.claude.checkpoints.list[at].texts.clear(),
            }
        }
    }
    if !s.old.is_empty() {
        crate::log::line(
            "turns",
            "info",
            &format!("{} turns past their conversations' days let go", s.old.len()),
        );
    }
    if big > 0 {
        let mb = ed.config.llm.turns_max_mb;
        let said = format!(
            "the oldest {big} of Claude's turns let go — this project's kept turns passed llm.turns-max-mb ({mb} MB); \
             space u reaches back as far as what's kept"
        );
        match std::mem::replace(&mut ed.workspace.claude.checkpoints.told_full, true) {
            false => ed.set_warning(said),
            true => ed.recorded(crate::editor::ToastKind::Warning, said),
        }
    }
    false
}

/// Turn `at` out of session `l` (its file gone, or let go by a save), the indexes into its list kept right — not
/// while a review is open on it (false: it stays, and the next save lets it go again).
fn drop_turn(l: &mut crate::workspace::Workspace, at: usize) -> bool {
    if let Some(crate::codereview::Base::Turn { at: open, .. }) = l.code_review.as_mut().map(|r| &mut r.base)
    {
        match (*open).cmp(&at) {
            std::cmp::Ordering::Equal => return false,
            std::cmp::Ordering::Greater => *open -= 1,
            std::cmp::Ordering::Less => {}
        }
    }
    let cps = &mut l.claude.checkpoints;
    cps.list.remove(at);
    cps.restored -= usize::from(at < cps.restored);
    true
}

/// Turn `at`'s texts in memory, then `then(ed, at)` — at once when they are; else read from its file on a
/// worker first, the turn found again by its id (gone meanwhile: nothing more). Its file gone for good
/// (`read_texts`): said, and the turn left out — nothing of it can be stepped or shown; one that can't be read
/// just now: said, the turn kept as it is.
pub fn with_texts(ed: &mut Editor, at: usize, then: impl FnOnce(&mut Editor, usize) + Send + 'static) {
    texts_then(ed, at, false, then);
}

/// `with_texts` for what no one asked for (a review's files graded again at a save, a turn's end, a HEAD move —
/// `codereview::grades_sync`): a read under way, or one that fails just now, says nothing — `then` doesn't run,
/// and the next time it's asked for tries again. A turn whose file is gone for good is still left out — a note
/// in `:messages`, never a toast.
pub fn with_texts_quietly(
    ed: &mut Editor,
    at: usize,
    then: impl FnOnce(&mut Editor, usize) + Send + 'static,
) {
    texts_then(ed, at, true, then);
}

/// `with_texts`, `quiet` as `with_texts_quietly` is.
fn texts_then(
    ed: &mut Editor,
    at: usize,
    quiet: bool,
    then: impl FnOnce(&mut Editor, usize) + Send + 'static,
) {
    let cps = &ed.workspace.claude.checkpoints;
    let Some(cp) = cps.list.get(at) else { return };
    let Files::Kept(files) = &cp.files else { return then(ed, at) };
    if cps.reading {
        if !quiet {
            ed.set_status("reading that turn's changes — a moment");
        }
        return;
    }
    // (a kept turn came from `dir`: it's set as they come back)
    let Some(dir) = cps.dir.clone() else { return };
    let (id, files, kept) = (cp.id, files.clone(), cp.texts.clone());
    let at_folder = folder(&dir, &ed.workspace.root);
    ed.workspace.claude.checkpoints.reading = true;
    ed.events.jobs().spawn(move || {
        let texts = read_texts(&at_folder, &files, &kept);
        move |ed: &mut Editor| {
            let cps = &mut ed.workspace.claude.checkpoints;
            cps.reading = false;
            let Some(at) = cps.list.iter().position(|c| c.id == id) else { return };
            match texts {
                Ok(texts) => {
                    let c = &mut cps.list[at];
                    if matches!(c.files, Files::Kept(_)) {
                        c.files = Files::Here(texts);
                    }
                    then(ed, at);
                }
                Err((false, e)) => {
                    let question = quote(&cps.list[at].question);
                    let said = format!("couldn't read the turn “{question}”'s changes just now ({e})");
                    match quiet {
                        true => crate::log::line(
                            "turns",
                            "info",
                            &format!("{said} — tried again when next asked"),
                        ),
                        false => ed.set_warning(format!("{said} — try again")),
                    }
                }
                Err((true, e)) => {
                    // (a review on it — its texts are held while one is open, so none is: closed all the same)
                    let on_it = |ed: &Editor| {
                        matches!(ed.workspace.code_review.as_ref().map(|r| &r.base),
                            Some(crate::codereview::Base::Turn { at: open, .. }) if *open == at)
                    };
                    if on_it(ed) {
                        crate::codereview::close(ed);
                    }
                    let question = ed.workspace.claude.checkpoints.list[at].question.clone();
                    drop_turn(&mut ed.workspace, at);
                    keep(ed);
                    let said =
                        format!("the turn “{}” isn't kept any more ({e}) — left out", quote(&question));
                    // (no one asked for it — a review's files graded again at a save: in `:messages`, no card over
                    // the work; the review it closed is in sight)
                    match quiet {
                        true => ed.recorded(crate::editor::ToastKind::Warning, said),
                        false => ed.set_warning(said),
                    }
                }
            }
        }
    });
}

/// Session `l`'s turns' texts let go where their files have them (#214: every turn made since tarae started kept
/// its texts — a whole file's twice for each it changed): all of them `behind`, and in front those past the newest
/// `KEEP` — they're read again as a step or a review needs them. Not the turn its review is open on (that reads
/// them as it shows), nor any where nothing is kept (no folder: tests), nor one no save has written yet.
pub(crate) fn shed(l: &mut crate::workspace::Workspace, behind: bool) {
    if l.claude.checkpoints.dir.is_none() {
        return;
    }
    let open = match l.code_review.as_ref().map(|r| &r.base) {
        Some(crate::codereview::Base::Turn { at, .. }) => Some(*at),
        _ => None,
    };
    let from = l.claude.checkpoints.list.len().saturating_sub(KEEP);
    for (i, c) in l.claude.checkpoints.list.iter_mut().enumerate() {
        if Some(i) != open && matches!(c.files, Files::Here(_)) && c.filed() && (behind || i < from) {
            c.files = Files::Kept(c.files.kept());
        }
    }
}

/// The turns `written` are in their files now (a save wrote their texts): `filed`, wherever they are — not one
/// whose files changed since (`own_drop`: fewer) — and let go in the sessions behind.
fn filed(ed: &mut Editor, written: &[(u64, Vec<Texts>)]) {
    // (the one in front is behind while its result is applied for it — `Editor.behind` — and then the session in
    // front is among those behind: it's left as it is)
    let swapped = ed.behind;
    for (i, l) in std::iter::once(&mut ed.workspace).chain(&mut ed.workspaces).enumerate() {
        let mut any = false;
        for c in l.claude.checkpoints.list.iter_mut().filter(|c| c.texts.is_empty()) {
            if let Some((_, texts)) = written.iter().find(|(id, t)| *id == c.id && t.len() == c.files.len()) {
                (c.texts, any) = (texts.clone(), true);
            }
        }
        match (any, i == 0) {
            (true, true) => shed(l, swapped),
            (true, false) if !swapped => shed(l, true),
            _ => {}
        }
    }
}

/// Tests: a project's store as it is on disk.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct StoreSeen {
    pub turns: usize,
    /// What the texts its turns name take, each once.
    pub bytes: u64,
    /// Texts a turn names that aren't there.
    pub missing: usize,
    /// Texts there that no turn names.
    pub unnamed: usize,
}

#[cfg(test)]
pub(crate) fn store_seen(dir: &Path, root: &Path) -> StoreSeen {
    let s = load(dir, root);
    let named: std::collections::HashMap<String, u64> = s
        .list
        .iter()
        .flat_map(|c| c.texts.iter().flat_map(|(b, a)| b.iter().chain([a])))
        .map(|t| (t.name(), t.len))
        .collect();
    let there: std::collections::HashSet<String> = std::fs::read_dir(texts_dir(&folder(dir, root)))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.len() == 64 && n.bytes().all(|b| b.is_ascii_hexdigit()))
        .collect();
    StoreSeen {
        turns: s.list.len(),
        bytes: named.values().sum(),
        missing: named.keys().filter(|n| !there.contains(*n)).count(),
        unnamed: there.iter().filter(|n| !named.contains_key(*n)).count(),
    }
}

/// `restore_from` the front session's own `dir` (tests set one; nothing is kept until they're back).
#[cfg(test)]
pub fn restore(ed: &mut Editor) {
    let Some(dir) = ed.workspace.claude.checkpoints.dir.take() else { return };
    ed.workspace.claude.checkpoints.restoring = true;
    restore_from(ed, dir, ed.workspace.root.clone());
}

/// As a session is made (`Editor::workspace_kept_back`): the kept turns of the checkout in `root` come back
/// from `dir` (read on a worker), before any made since, into the session the editor's results are for now
/// — its `dir` set once they're back (a save before would write over them). They come back as kept (paths
/// only). A turn tarae ended in the middle of becomes a checkpoint of its own, its files as they are now.
pub fn restore_from(ed: &Editor, dir: PathBuf, root: PathBuf) {
    ed.events.jobs().spawn(move || {
        let back = dir.clone();
        let mut s = load(&dir, &root);
        let mut cut = std::mem::take(&mut s.turn);
        for t in cut.iter_mut().filter(|t| t.bytes.is_none()) {
            // (gone: nothing in it now · unreadable: as it was kept)
            match std::fs::read(&t.path) {
                Ok(b) => {
                    if let Ok(now) = String::from_utf8(b) {
                        (t.after, t.gone) = (Rope::from_str(&now), false);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (t.after, t.gone) = (Rope::new(), true),
                Err(_) => {}
            }
        }
        cut.retain(Touched::changed);
        let kept = s.list.len();
        if !cut.is_empty() {
            let question = if s.question.is_empty() { CUT_SHORT.to_string() } else { s.question.clone() };
            let head = std::mem::take(&mut s.head);
            s.list.push(Checkpoint { head, ..Checkpoint::new(question, cut) });
        }
        move |ed: &mut Editor| {
            ed.workspace.claude.checkpoints.dir = Some(back);
            ed.workspace.claude.checkpoints.restored = kept;
            ed.workspace.claude.checkpoints.restoring = false;
            let n = s.list.len();
            if n > 0 {
                let since = std::mem::take(&mut ed.workspace.claude.checkpoints.list);
                ed.workspace.claude.checkpoints.list = s.list.into_iter().chain(since).collect();
                // (a turn's review open already points past the ones put before it)
                if let Some(crate::codereview::Base::Turn { at, .. }) =
                    ed.workspace.code_review.as_mut().map(|r| &mut r.base)
                {
                    *at += n;
                }
            }
            // (a turn's review `:restart` carried: open again now its turn is back)
            if let Some(v) = ed.workspace.turn_review_back.take() {
                crate::codereview::reopen_turn(ed, &v);
            }
            keep(ed);
        }
    });
}

/// `base` taken to `theirs`, over `ours` (= `base` with the user's edits): the lines `base` → `theirs`
/// changes, except where `ours` changed on or next to them — those stay as `ours` has them. (text, how many
/// were left).
pub fn rebase(base: &str, theirs: &str, ours: &str) -> (String, usize) {
    if base == ours {
        return (theirs.to_string(), 0);
    }
    let mine = crate::editdiff::line_changes(base, ours);
    let wanted = crate::editdiff::line_changes(base, theirs);
    // Touching counts: two edits on neighboring lines are one place
    let near =
        |a: &(usize, usize, String, String), b: &(usize, usize, String, String)| a.0 <= b.1 && b.0 <= a.1;
    let (take, left): (Vec<_>, Vec<_>) = wanted.into_iter().partition(|w| !mine.iter().any(|m| near(w, m)));
    // Both sets are disjoint on `base` now: apply them in order
    let mut all: Vec<(usize, usize, String, String)> = mine.into_iter().chain(take).collect();
    crate::sorting::by_key(&mut all, |c| (c.0, c.1));
    let mut out = String::with_capacity(ours.len());
    let mut at = 0;
    for (from, to, _, insert) in all {
        out.push_str(&base[at..from]);
        out.push_str(&insert);
        at = to;
    }
    out.push_str(&base[at..]);
    (out, left.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebase_keeps_the_users_edits_and_what_they_touched() {
        let before = "a\nb\nc\nd\ne\nf\ng\n";
        let after = "a\nB\nc\nd\ne\nF\ng\n"; // Claude changed b and f
        // Untouched since: straight back
        assert_eq!(rebase(after, before, after), (before.to_string(), 0));
        // The user edited d (away from both): d stays theirs, b and f go back
        let ours = "a\nB\nc\nD!\ne\nF\ng\n";
        assert_eq!(rebase(after, before, ours), ("a\nb\nc\nD!\ne\nf\ng\n".to_string(), 0));
        // The user edited the line after B: that change of Claude's stays
        let ours = "a\nB\nC!\nd\ne\nF\ng\n";
        assert_eq!(rebase(after, before, ours), ("a\nB\nC!\nd\ne\nf\ng\n".to_string(), 1));
        // Lines added by the user at the end
        let ours = "a\nB\nc\nd\ne\nF\ng\nh\n";
        assert_eq!(rebase(after, before, ours), ("a\nb\nc\nd\ne\nf\ng\nh\n".to_string(), 0));
        // A created file emptied back; the user's own addition keeps it
        assert_eq!(rebase("new\n", "", "new\n"), (String::new(), 0));
        // (the user's line right after them touches them: all stays)
        assert_eq!(rebase("x\ny\nz\n", "", "x\ny\nz\nmine\n"), ("x\ny\nz\nmine\n".to_string(), 1));
    }

    /// #218: what restoring sessions' turns costs — each session's kept turns (`KEEP` of them, every file
    /// a turn touched with its whole text before and after) read as a restore reads them (`load`: the index,
    /// paths only — an earlier tarae's one file was read whole, 525 MB held for these), 26 sessions four at a
    /// time as the workers take them. The process's footprint before, at its peak while loading, with the
    /// turns held, and once they're dropped. Stand-in turns: `TARAE_TURN_FILES` files of `TARAE_TURN_KB` KB
    /// each (3 × 150 KB).
    #[test]
    fn perf_restored_turns_memory() {
        if crate::test_results::unoptimized("checkpoint::tests::perf_restored_turns_memory") {
            return;
        }
        use super::*;
        let env = |k: &str, d: usize| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        let (files, kb, sessions) = (env("TARAE_TURN_FILES", 3), env("TARAE_TURN_KB", 150), 26);
        let scratch = crate::scratch::Scratch::new("perf-turns");
        let dir = scratch.to_path_buf();
        let text = |seed: usize| -> Rope {
            let line =
                format!("    let x{seed} = compute(a, b, c); // a line of a source file, as long as most\n");
            Rope::from_str(&line.repeat(kb * 1024 / line.len()))
        };
        let roots: Vec<PathBuf> = (0..sessions).map(|i| PathBuf::from(format!("/checkouts/s{i}"))).collect();
        for root in &roots {
            let list = (0..KEEP)
                .map(|t| {
                    let touched = (0..files)
                        .map(|f| Touched {
                            path: root.join(format!("src/f{f}.rs")),
                            before: Some(text(t * 10 + f)),
                            after: text(t * 10 + f + 1),
                            ..Default::default()
                        })
                        .collect();
                    Checkpoint::new(format!("turn {t}"), touched)
                })
                .collect();
            put(&dir, root, Stored { list, seq: 1, ..Default::default() });
        }
        // (each project's folder: its index and its turns' texts)
        let on_disk: u64 = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .flat_map(|e| std::fs::read_dir(e.path()).into_iter().flatten().flatten())
            .map(|e| e.metadata().unwrap().len())
            .sum();
        let me = std::process::id();
        let footprint = || {
            let os = crate::procs::tree(me).into_iter().find(|o| o.pid == me).unwrap();
            os.footprint.unwrap_or(os.rss)
        };
        let mb = |b: u64| b as f64 / (1 << 20) as f64;
        let before = footprint();
        let (stop, peak) = (std::sync::atomic::AtomicBool::new(false), std::sync::atomic::AtomicU64::new(0));
        let held: Vec<Stored> = std::thread::scope(|s| {
            crate::event::scoped(s, "tarae-peak", || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    peak.fetch_max(footprint(), std::sync::atomic::Ordering::Relaxed);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            });
            let mut held = Vec::new();
            for four in roots.chunks(4) {
                let loads: Vec<_> =
                    four.iter().map(|r| crate::event::scoped(s, "tarae-load", || load(&dir, r))).collect();
                held.extend(loads.into_iter().map(|l| l.join().unwrap()));
            }
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            held
        });
        let holding = footprint();
        let turns: usize = held.iter().map(|s| s.list.len()).sum();
        drop(held);
        let after = footprint();
        let peak = peak.into_inner().max(holding);
        eprintln!(
            "restored turns: {sessions} sessions × {KEEP} turns × {files} files × {kb} KB ({turns} turns, {:.0} MB \
             on disk) — footprint before {:.0} MB, peak {:.0} MB, holding {:.0} MB, dropped {:.0} MB",
            mb(on_disk),
            mb(before),
            mb(peak),
            mb(holding),
            mb(after)
        );
        assert_eq!(turns, sessions * KEEP);
    }

    /// frictions19: `space u` putting a file back in the project writes through its folders by descriptor —
    /// `sub/` swapped for a link to outside since the turn (what a sandboxed run can do) and the file there
    /// is untouched, the step says why; a file the turn made is removed the same way, never the one the
    /// link leads to. (By its real path the file was "outside" now, and written by path through the link.)
    #[test]
    fn putting_a_file_back_never_goes_through_a_link_swapped_in() {
        use super::*;
        let base = crate::scratch::Scratch::new("undo-beneath");
        let (dir, outside) = (base.join("p"), base.join("outside"));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let (dir, outside) = (std::fs::canonicalize(&dir).unwrap(), std::fs::canonicalize(&outside).unwrap());
        let mut ed = Editor::new(crate::config::Config::default());
        ed.workspace.root = dir.clone();
        for f in ["a.txt", "new.txt"] {
            std::fs::write(dir.join("sub").join(f), "after\n").unwrap();
            std::fs::write(outside.join(f), "after\n").unwrap();
            ed.open(&dir.join("sub").join(f)).unwrap();
        }
        std::fs::rename(dir.join("sub"), dir.join("sub.aside")).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("sub")).unwrap();
        let (before, after) = (Rope::from_str("before\n"), Rope::from_str("after\n"));
        // (undoing: from the turn's text back to what was before it — none, for the file it made)
        // (opened again it's where the link leads now: refused as not the project's, or as the link it is)
        let e = file(&mut ed, &dir.join("sub/a.txt"), Some(&after), Some(&before)).unwrap_err();
        assert!(e.contains("a link") || e.contains("not in"), "{e}");
        let e = file(&mut ed, &dir.join("sub/new.txt"), Some(&after), None).unwrap_err();
        assert!(e.contains("a link"), "{e}");
        for f in ["a.txt", "new.txt"] {
            assert_eq!(
                std::fs::read_to_string(outside.join(f)).unwrap(),
                "after\n",
                "{f}: never through the link"
            );
        }
    }

    /// Events handled until `until`.
    #[track_caller]
    fn settle(ed: &mut Editor, what: &str, until: impl Fn(&Editor) -> bool) {
        crate::test_results::settle_saying(ed, until, |_| what.to_string());
    }

    /// `save` whatever the turns' age and size — written.
    fn put(dir: &Path, root: &Path, s: Stored) -> bool {
        save(dir, root, s, Room::ANY).written
    }

    /// The texts kept in the project's folder `at`, by name.
    fn texts_in(at: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(texts_dir(at))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        crate::sorting::sort(&mut names);
        names
    }

    /// A temp folder of the test's own (real path), empty — removed as it's dropped, so a test whose editor keeps
    /// its turns lets their saves land first (`landed`).
    fn temp(name: &str) -> crate::scratch::Scratch {
        crate::scratch::Scratch::new(name)
    }

    /// The turns' saves under way on workers (`keep`'s) landed — before the test's folder goes, or one lands after
    /// it and makes its `turns/` again (#399: 541 left in `$TMPDIR`).
    #[track_caller]
    fn landed(ed: &Editor) {
        assert!(ed.events.writes().wait(crate::test_results::PATIENCE), "the turns' saves landed");
    }

    /// A turn that made `root`'s `a.txt` "two\n" from "one\n".
    fn one_to_two(root: &Path, question: &str) -> Checkpoint {
        let (before, after) = (Some(Rope::from_str("one\n")), Rope::from_str("two\n"));
        Checkpoint::new(
            question.into(),
            vec![Touched { path: root.join("a.txt"), before, after, ..Default::default() }],
        )
    }

    /// #251: a file whose path is no text is left out of its kept turn, the rest of the turn kept with each text still
    /// its own file's (`json!` panics on such a path, a release build aborts); a project named so is no panic either.
    #[test]
    fn a_file_whose_path_is_no_text_is_left_out_of_its_kept_turn() {
        use std::os::unix::ffi::OsStrExt as _;
        let dir = temp("turns-no-text");
        let (root, store) = (dir.join("p"), dir.join("turns"));
        std::fs::create_dir_all(&root).unwrap();
        let bad = PathBuf::from(std::ffi::OsStr::from_bytes(b"/p/caf\xe9.rs"));
        let file = |path: PathBuf| Touched { path, after: Rope::from_str("bad\n"), ..Default::default() };
        let mut kept = one_to_two(&root, "q");
        let Files::Here(files) = &mut kept.files else { panic!("a new turn's are here") };
        files.insert(0, file(bad.clone()));
        let turn = vec![file(bad.clone()), file(root.join("b.txt"))];
        let list = vec![kept];
        assert!(put(&store, &root, Stored { list, turn, seq: 1, ..Default::default() }));
        let back = load_whole(&store, &root);
        let Files::Here(files) = &back.list[0].files else { panic!("its texts read") };
        let kept: Vec<_> = files.iter().map(|t| (t.path.clone(), t.after.to_string())).collect();
        assert_eq!(kept, [(root.join("a.txt"), "two\n".to_string())]);
        assert_eq!(back.turn.iter().map(|t| &t.path).collect::<Vec<_>>(), [&root.join("b.txt")]);
        assert!(put(&store, &bad, Stored { seq: 1, ..Default::default() }));
    }

    /// #218: kept turns come back as their paths only, in every session — the one in front and those behind
    /// — and `space u` / `space U` still take each back and bring it again across the start, its texts read
    /// from their files as the step asks. A session behind lets them go once the step is done, and one going
    /// behind lets go what it read (their files still have them); in front they're held.
    #[test]
    fn every_sessions_turn_undoes_across_a_start_its_texts_read_as_asked() {
        use super::*;
        let dir = temp("turns-every");
        let names = ["front", "b1", "b2"];
        let store = dir.join("turns");
        for n in names {
            std::fs::create_dir_all(dir.join(n)).unwrap();
            std::fs::write(dir.join(n).join("a.txt"), "two\n").unwrap();
            let list = vec![one_to_two(&dir.join(n), &format!("turn in {n}"))];
            assert!(put(
                &store,
                &dir.join(n),
                Stored { list, seq: crate::chats::seq(), ..Default::default() }
            ));
        }
        // A new tarae: the session in front and two behind, each with its kept turn
        let mut ed = Editor::new(crate::config::Config::default());
        (ed.workspace.root, ed.turns_dir) = (dir.join("front"), Some(store.clone()));
        ed.workspace_kept_back(ed.workspace.id);
        let ids = [ed.workspace.id, ed.workspace_add(dir.join("b1")), ed.workspace_add(dir.join("b2"))];
        assert!(ed.restoring(), "reading them back");
        let turns = |ed: &Editor, id| {
            let l = std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == id).unwrap();
            l.claude.checkpoints.list.clone()
        };
        settle(&mut ed, "every session's turn back", |ed| {
            !ed.restoring() && ids.iter().all(|&id| turns(ed, id).len() == 1)
        });
        let held = |ed: &Editor, id| turns(ed, id)[0].files.here().is_some();
        for id in ids {
            assert!(!held(&ed, id), "paths only: {:?}", turns(&ed, id));
        }
        for (id, n) in ids.into_iter().zip(names) {
            let a = dir.join(n).join("a.txt");
            let now = |want: &'static str| {
                let a = a.clone();
                move |_: &Editor| std::fs::read_to_string(&a).unwrap() == want
            };
            ed.with_workspace(id, |ed| step(ed, true).unwrap());
            settle(&mut ed, &format!("{n}: undone"), now("one\n"));
            ed.with_workspace(id, |ed| step(ed, false).unwrap());
            settle(&mut ed, &format!("{n}: brought back"), now("two\n"));
            assert_eq!(held(&ed, id), id == ids[0], "{n}: held in front only");
        }
        // The front one goes behind: what it read goes
        assert!(ed.workspace_switch(ids[1]).is_in());
        assert!(!held(&ed, ids[0]) && !held(&ed, ids[1]));
        // …and is read again when asked
        ed.with_workspace(ids[0], |ed| step(ed, true).unwrap());
        let a = dir.join("front/a.txt");
        settle(&mut ed, "front: undone again", |_| std::fs::read_to_string(&a).unwrap() == "one\n");
        landed(&ed);
    }

    /// #214: turns made since tarae started hold no texts their files have once their session is behind, and none
    /// past the newest `KEEP` anywhere — each kept its own, a whole file twice for each it changed, for as long as
    /// tarae ran (the driver's 17 sessions grew to ~537 MB). Turns in front and behind, each writing a file of
    /// ~220 KB unseen, a git review open in each (so no turn's opens — the turn a review is open on keeps its texts,
    /// one a session): the texts held stay flat in the turns made — none behind, the newest `KEEP` in front — while
    /// the disk keeps every turn, each text once (a turn's before is the after of the turn three back: 14 texts for
    /// 14 turns). The newest behind is still undone, its texts read back from their files; and after a restart
    /// `space u` reaches back through all 14, the oldest's files made by it gone again.
    #[test]
    fn turns_made_hold_no_texts_their_files_have_behind_nor_past_keep() {
        use super::*;
        let dir = temp("turns-held");
        let store = dir.join("turns");
        let (front, back) = (dir.join("front"), dir.join("back"));
        for r in [&front, &back] {
            std::fs::create_dir_all(r).unwrap();
        }
        let mut ed = Editor::new(crate::config::Config::default());
        (ed.workspace.root, ed.turns_dir) = (front.clone(), Some(store.clone()));
        ed.workspace_kept_back(ed.workspace.id);
        let ids = [ed.workspace.id, ed.workspace_add(back.clone())];
        settle(&mut ed, "both sessions' turns read back", |ed| !ed.restoring());
        for (id, root) in ids.into_iter().zip([&front, &back]) {
            let review =
                crate::codereview::CodeReview::for_test(crate::codereview::Base::Head, root.clone(), vec![]);
            ed.with_workspace(id, |ed| ed.workspace.code_review = Some(review));
        }
        let list = |ed: &Editor, id| {
            let mut all = std::iter::once(&ed.workspace).chain(&ed.workspaces);
            all.find(|l| l.id == id).map(|l| l.claude.checkpoints.list.clone()).unwrap_or_default()
        };
        // (bytes of text held: before and after, every file of every turn with its texts here)
        let held = |ed: &Editor, id| -> (usize, usize) {
            let list = list(ed, id);
            let here: Vec<&[Touched]> = list.iter().filter_map(|c| c.files.here()).collect();
            let bytes = here.iter().flat_map(|f| f.iter());
            (
                here.len(),
                bytes.map(|t| t.before.as_ref().map_or(0, Rope::len_bytes) + t.after.len_bytes()).sum(),
            )
        };
        let big = "    let x = compute(a, b, c); // a line of a source file, as long as most\n".repeat(3000);
        let mut most = Vec::new();
        for t in 0..14 {
            for (k, root) in [&front, &back].into_iter().enumerate() {
                let path = root.join(format!("f{}.rs", t % 3));
                let before = std::fs::read_to_string(&path).ok().map(|s| Rope::from_str(&s));
                ed.with_workspace(ids[k], |ed| record_unseen(ed, path.clone(), before, |_| {}));
                std::fs::write(&path, format!("{big}// turn {t}\n")).unwrap();
                ed.with_workspace(ids[k], |ed| assert!(gather(ed, &format!("turn {t}")), "written unseen"));
                // (its texts in its file: the save's word back)
                settle(&mut ed, &format!("turn {t} of session {k} made and in its file"), |ed| {
                    let l = list(ed, ids[k]);
                    l.len() == t + 1 && l.last().is_some_and(Checkpoint::filed)
                });
            }
            let (front_held, back_held) = (held(&ed, ids[0]), held(&ed, ids[1]));
            assert_eq!(back_held, (0, 0), "turn {t}: behind, no texts held");
            assert_eq!(front_held.0, (t + 1).min(KEEP), "turn {t}: in front, the newest KEEP turns' texts");
            most.push(front_held.1);
        }
        // (at most `KEEP` turns' two texts of one file, at every turn)
        let bound = KEEP * 2 * (big.len() + 16);
        assert!(most.iter().all(|&b| b <= bound), "held in front past {bound} B: {most:?}");
        // The newest turn behind, undone: its texts read back from its file
        let f = back.join("f1.rs");
        ed.with_workspace(ids[1], crate::codereview::close);
        ed.with_workspace(ids[1], |ed| step(ed, true).unwrap());
        settle(&mut ed, "behind: its newest turn undone", |_| {
            std::fs::read_to_string(&f).unwrap().ends_with("// turn 10\n")
        });
        // Every turn on disk, each text once
        for root in [&front, &back] {
            assert_eq!(load(&store, root).list.len(), 14, "every turn kept: {}", root.display());
            assert_eq!(texts_in(&folder(&store, root)).len(), 14, "each text once: {}", root.display());
        }
        settle(&mut ed, "its undo kept", |_| load(&store, &back).list.last().is_some_and(|c| c.undone));
        // (as a quit flushes them)
        landed(&ed);
        drop(ed);
        // A restart: `space u` behind reaches its oldest turn, each read back from its files as asked
        let mut ed = Editor::new(crate::config::Config::default());
        (ed.workspace.root, ed.turns_dir) = (front.clone(), Some(store.clone()));
        ed.workspace_kept_back(ed.workspace.id);
        let ids = [ed.workspace.id, ed.workspace_add(back.clone())];
        settle(&mut ed, "both sessions' turns back", |ed| {
            !ed.restoring() && ids.iter().all(|&id| list(ed, id).len() == 14)
        });
        for n in 2..=14 {
            ed.with_workspace(ids[1], |ed| step(ed, true).unwrap());
            settle(&mut ed, &format!("behind: {n} turns undone"), |ed| {
                list(ed, ids[1]).iter().filter(|c| c.undone).count() == n
            });
            assert_eq!(held(&ed, ids[1]).0, 0, "behind again: the turn read for it let go");
        }
        for f in 0..3 {
            assert!(!back.join(format!("f{f}.rs")).exists(), "f{f}.rs, made by the oldest turns, gone");
        }
        landed(&ed);
    }

    /// A project keeps its turns as an editor keeps its history, by time (user, 2026-10-29): a turn goes as its
    /// conversation would — `cleanupPeriodDays` as the chat's claude reads it (here its `--settings`), the time read
    /// through `clock` — and the project's texts stay within `llm.turns-max-mb`, its oldest turns going first, a text a
    /// kept turn still names staying and the newest turn staying whatever its size. Said once, a warning; after that
    /// in `:messages` only.
    #[test]
    fn turns_go_with_their_conversations_and_the_oldest_past_the_size_limit() {
        use super::*;
        let dir = temp("turns-room");
        let (root, store) = (dir.join("p"), dir.join("turns"));
        std::fs::create_dir_all(&root).unwrap();
        let at = folder(&store, &root);
        let mut ed = Editor::new(crate::config::Config::default());
        (ed.workspace.root, ed.turns_dir) = (root.clone(), Some(store.clone()));
        ed.workspace_kept_back(ed.workspace.id);
        settle(&mut ed, "read back", |ed| !ed.restoring());
        let mut chat = crate::chat::Chat::new();
        let args = vec!["--settings".to_string(), r#"{"cleanupPeriodDays":5}"#.to_string()];
        chat.launched = Some((crate::llm::LlmConfig { args, ..Default::default() }, Vec::new()));
        ed.workspace.claude.chat = Some(chat);
        crate::clock::hold();
        // (a turn that made `<q>.txt` from `from` — kept, its texts in their files)
        let turn = |ed: &mut Editor, q: &str, from: Option<&str>, to: &str| {
            let t = Touched {
                path: root.join(format!("{q}.txt")),
                before: from.map(Rope::from_str),
                after: Rope::from_str(to),
                ..Default::default()
            };
            let c = Checkpoint { conv: conversation(&ed.workspace), ..Checkpoint::new(q.into(), vec![t]) };
            ed.workspace.claude.checkpoints.list.push(c);
            keep(ed);
            settle(ed, &format!("turn {q} kept"), |ed| {
                let l = &ed.workspace.claude.checkpoints.list;
                l.iter().any(|c| c.question == q) && l.iter().all(Checkpoint::filed)
            });
        };
        let questions = |ed: &Editor| -> Vec<String> {
            ed.workspace.claude.checkpoints.list.iter().map(|c| c.question.clone()).collect()
        };
        let on_disk =
            || -> Vec<String> { load(&store, &root).list.into_iter().map(|c| c.question).collect() };
        let day = std::time::Duration::from_secs(86_400);
        // By age: a's conversation was last active 6 days before the third save, b's 3
        let talk = |ed: &mut Editor, id: &str| {
            ed.workspace.claude.chat.as_mut().unwrap().conversation = Some(id.into())
        };
        talk(&mut ed, "c1");
        turn(&mut ed, "a", None, "a\n");
        crate::clock::advance(3 * day);
        talk(&mut ed, "c2");
        turn(&mut ed, "b", None, "b\n");
        assert_eq!(questions(&ed), ["a", "b"]);
        crate::clock::advance(3 * day);
        keep(&ed);
        settle(&mut ed, "a, past its conversation's days, let go", |ed| questions(ed) == ["b"]);
        assert_eq!(on_disk(), ["b"]);
        assert_eq!(texts_in(&at), [Text::of("b\n").name()], "a's text gone with it");
        // By size (1 MiB): c's C and d's D are 1,000,000 B; e's E makes it 1.3 MB — b goes, c, then d (C is gone
        // with it), and D stays: e's text before
        ed.config.llm.turns_max_mb = 1;
        let (big_c, big_d, big_e) = ("c".repeat(500_000), "d".repeat(500_000), "e".repeat(300_000));
        turn(&mut ed, "c", None, &big_c);
        turn(&mut ed, "d", Some(&big_c), &big_d);
        assert_eq!(questions(&ed), ["b", "c", "d"], "within it");
        turn(&mut ed, "e", Some(&big_d), &big_e);
        assert_eq!(questions(&ed), ["e"]);
        assert_eq!(on_disk(), ["e"]);
        let mut de = vec![Text::of(&big_d).name(), Text::of(&big_e).name()];
        crate::sorting::sort(&mut de);
        assert_eq!(texts_in(&at), de, "D kept for e, C gone");
        let full = |s: &str| s.contains("llm.turns-max-mb");
        let toasts = |ed: &Editor| ed.toasts.iter().filter(|t| full(&t.text)).count();
        assert_eq!(toasts(&ed), 1, "said: {:?}", ed.toasts);
        // The newest stays past it, alone — and said again only in `:messages`
        // (F under a MiB: a bigger file opens on a worker — the step would say it's still loading)
        turn(&mut ed, "f", Some(&big_e), &"f".repeat(900_000));
        assert_eq!(questions(&ed), ["f"]);
        assert_eq!(toasts(&ed), 1, "said once: {:?}", ed.toasts);
        assert_eq!(ed.messages.iter().filter(|m| full(&m.2)).count(), 2, "{:?}", ed.messages);
        // Its undo, its texts read back from their files
        shed(&mut ed.workspace, true);
        assert!(ed.workspace.claude.checkpoints.list[0].files.here().is_none(), "let go");
        std::fs::write(root.join("f.txt"), "f".repeat(900_000)).unwrap();
        step(&mut ed, true).unwrap();
        let f = root.join("f.txt");
        settle(&mut ed, "f undone", |_| std::fs::read_to_string(&f).is_ok_and(|s| s == big_e));
        crate::clock::release();
        landed(&ed);
    }

    /// An editor keeping its turns in `dir`'s `turns/` for the project `dir/p`, read back, its chat's claude started
    /// with `cleanupPeriodDays` 5 in conversation `c1` (review round 1).
    fn kept_in(dir: &Path) -> (Editor, PathBuf, PathBuf) {
        let (root, store) = (dir.join("p"), dir.join("turns"));
        std::fs::create_dir_all(&root).unwrap();
        let mut ed = Editor::new(crate::config::Config::default());
        (ed.workspace.root, ed.turns_dir) = (root.clone(), Some(store.clone()));
        ed.workspace_kept_back(ed.workspace.id);
        settle(&mut ed, "read back", |ed| !ed.restoring());
        let mut chat = crate::chat::Chat::new();
        let args = vec!["--settings".to_string(), r#"{"cleanupPeriodDays":5}"#.to_string()];
        chat.launched = Some((crate::llm::LlmConfig { args, ..Default::default() }, Vec::new()));
        chat.conversation = Some("c1".into());
        ed.workspace.claude.chat = Some(chat);
        (ed, root, store)
    }

    /// A turn that made `root`'s `<q>.txt` `to`, kept — settled once its texts are in their files.
    fn kept_turn(ed: &mut Editor, q: &str, to: &str) {
        let path = ed.workspace.root.join(format!("{q}.txt"));
        let t = Touched { path, after: Rope::from_str(to), ..Default::default() };
        let c = Checkpoint { conv: conversation(&ed.workspace), ..Checkpoint::new(q.into(), vec![t]) };
        ed.workspace.claude.checkpoints.list.push(c);
        keep(ed);
        settle(ed, &format!("turn {q} kept"), |ed| {
            ed.workspace.claude.checkpoints.list.iter().any(|c| c.question == q && c.filed())
        });
    }

    fn questions(ed: &Editor) -> Vec<String> {
        ed.workspace.claude.checkpoints.list.iter().map(|c| c.question.clone()).collect()
    }

    /// Review round 1: a turn goes as its conversation does — counted from its last activity, never from when the
    /// turn was made: a conversation in use keeps its oldest turns; idle past its days, its turns go but the newest,
    /// which a save never lets go (a `space u` step's after an idle week let every turn go).
    #[test]
    fn a_turn_goes_as_its_conversation_does_from_its_last_activity_and_the_newest_never() {
        use super::*;
        let dir = temp("turns-last-active");
        let (mut ed, root, store) = kept_in(&dir);
        let chats = dir.join("chats");
        ed.chats_dir = Some(chats.clone());
        crate::clock::hold();
        let day = std::time::Duration::from_secs(86_400);
        let active = |at: u64| {
            let rows = Vec::new();
            let c = crate::chats::Saved {
                id: "c1".into(),
                title: "t".into(),
                updated: at,
                rows,
                plan: None,
                usage: None,
                places: None,
                seq: crate::chats::seq(),
            };
            crate::chats::save(&chats, &root, c);
        };
        let on_disk =
            || -> Vec<String> { load(&store, &root).list.into_iter().map(|c| c.question).collect() };
        // a on day 0, b on day 3, the conversation last active on day 4 (a message with no edit)
        kept_turn(&mut ed, "a", "a\n");
        crate::clock::advance(3 * day);
        kept_turn(&mut ed, "b", "b\n");
        active(now_s() + 86_400);
        // (the conversations' rows are never what a save reads: their file gone, the times stay)
        std::fs::write(crate::config::state_file(&chats, &root), "not read").unwrap();
        // Day 8½: a made 8½ days ago, b 5½, the conversation active 4½ — kept by that alone
        crate::clock::advance(5 * day + day / 2);
        keep_now(&ed, &ed.workspace);
        assert_eq!(on_disk(), ["a", "b"], "a conversation in use keeps its turns");
        // Day 10: idle 6 days — a goes, b the newest stays
        crate::clock::advance(2 * day);
        keep(&ed);
        settle(&mut ed, "a let go", |ed| questions(ed) == ["b"]);
        assert_eq!(on_disk(), ["b"]);
        // A year on, a step's save: the newest still there, undoable
        crate::clock::advance(365 * day);
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        step(&mut ed, true).unwrap();
        settle(&mut ed, "b undone", |_| !root.join("b.txt").exists());
        keep(&ed);
        settle(&mut ed, "saved", |_| load(&store, &root).list.first().is_some_and(|c| c.undone));
        assert_eq!((questions(&ed), on_disk()), (vec!["b".to_string()], vec!["b".to_string()]));
        crate::clock::release();
        landed(&ed);
    }

    /// Review round 1: a save with nothing to say leaves the frame to what runs after it — `keep_then`'s `then`
    /// changed the screen in the same event, and the frame waited for the next key.
    #[test]
    fn a_quiet_save_draws_what_its_then_changed() {
        use super::*;
        let dir = temp("turns-quiet-then");
        let (mut ed, root, _) = kept_in(&dir);
        use std::cell::Cell;
        thread_local! {
            /// Whether the event that said "written" changed nothing on screen.
            static WRITTEN: Cell<Option<bool>> = const { Cell::new(None) };
            /// An event said it changed nothing.
            static QUIET: Cell<bool> = const { Cell::new(false) };
        }
        // (each event's word taken, then cleared for the next — as the loop does)
        crate::editor::AFTER_EVENT.set(Some(|ed: &mut Editor| {
            if WRITTEN.get().is_none() && ed.toasts.iter().any(|t| t.text == "written") {
                WRITTEN.set(Some(ed.unchanged));
            }
            QUIET.set(QUIET.get() || ed.unchanged);
            ed.unchanged = false;
        }));
        let files = vec![(root.join("a.txt"), Some(Rope::from_str("one\n")), Some(Rope::from_str("two\n")))];
        ed.unchanged = false;
        record_all(&mut ed, files, |ed| ed.set_status("written"));
        settle(&mut ed, "then ran", |_| WRITTEN.get().is_some());
        assert_eq!(WRITTEN.get(), Some(false), "the status `then` set is drawn");
        // One with nothing after it: no frame
        QUIET.set(false);
        keep(&ed);
        settle(&mut ed, "a save with nothing to say draws nothing", |_| QUIET.get());
        crate::editor::AFTER_EVENT.set(None);
        landed(&ed);
    }

    /// Review round 1: the turn a review is open on stays when a save lets it go — its texts in memory, the review
    /// whole — and the next save that has room for it writes it again, index and texts; once the review is closed,
    /// the next save lets it go.
    #[test]
    fn a_turn_let_go_with_its_review_open_stays_and_is_written_again() {
        use super::*;
        let dir = temp("turns-review-open");
        let (mut ed, root, store) = kept_in(&dir);
        let at = folder(&store, &root);
        ed.config.llm.turns_max_mb = 1;
        let (big_a, big_b) = ("a".repeat(600_000), "b".repeat(600_000));
        kept_turn(&mut ed, "a", &big_a);
        let turn = crate::codereview::Base::Turn { at: 0, question: "a".into() };
        ed.workspace.code_review =
            Some(crate::codereview::CodeReview::for_test(turn, root.clone(), Vec::new()));
        // b past the limit: a would go, but its review is open
        kept_turn(&mut ed, "b", &big_b);
        assert_eq!(questions(&ed), ["a", "b"], "kept in memory");
        let on_disk =
            || -> Vec<String> { load(&store, &root).list.into_iter().map(|c| c.question).collect() };
        assert_eq!(on_disk(), ["b"]);
        assert_eq!(texts_in(&at), [Text::of(&big_b).name()], "a's text gone from the disk");
        let a = &ed.workspace.claude.checkpoints.list[0];
        assert!(a.files.here().is_some_and(|f| f[0].after == big_a.as_str()), "its texts in memory");
        // Room again: written back, readable from its files
        ed.config.llm.turns_max_mb = 4;
        keep_now(&ed, &ed.workspace);
        assert_eq!(on_disk(), ["a", "b"]);
        let whole = load_whole(&store, &root);
        assert!(
            whole.list[0].files.here().is_some_and(|f| f[0].after == big_a.as_str()),
            "a read back whole"
        );
        // No room, the review closed: let go
        ed.config.llm.turns_max_mb = 1;
        ed.workspace.code_review = None;
        keep(&ed);
        settle(&mut ed, "a let go", |ed| questions(ed) == ["b"]);
        assert_eq!(on_disk(), ["b"]);
        landed(&ed);
    }

    /// Review round 2: a drop refused for its open review leaves the turn unfiled — the save took its texts off the
    /// disk — so once the review closes and the session goes behind, nothing lets its texts go before the next save
    /// writes them again, and `space u` still takes it back (it was let go as Kept, its file gone: "left out").
    #[test]
    fn a_turn_kept_for_its_review_keeps_its_texts_until_they_are_written_again() {
        use super::*;
        let dir = temp("turns-refused-drop");
        let (mut ed, root, _) = kept_in(&dir);
        ed.config.llm.turns_max_mb = 1;
        let (big_a, big_b) = ("a".repeat(600_000), "b".repeat(600_000));
        kept_turn(&mut ed, "a", &big_a);
        let turn = crate::codereview::Base::Turn { at: 0, question: "a".into() };
        ed.workspace.code_review =
            Some(crate::codereview::CodeReview::for_test(turn, root.clone(), Vec::new()));
        kept_turn(&mut ed, "b", &big_b);
        assert_eq!(questions(&ed), ["a", "b"], "a refused: its review is open");
        assert!(!ed.workspace.claude.checkpoints.list[0].filed(), "its texts gone from the disk: not filed");
        // The review closed, the session behind: a's texts stay in memory
        ed.workspace.code_review = None;
        shed(&mut ed.workspace, true);
        assert!(ed.workspace.claude.checkpoints.list[0].files.here().is_some(), "not let go unwritten");
        // Room again, both taken back: a.txt goes with a's undo
        ed.config.llm.turns_max_mb = 4;
        std::fs::write(root.join("a.txt"), &big_a).unwrap();
        std::fs::write(root.join("b.txt"), &big_b).unwrap();
        step(&mut ed, true).unwrap();
        settle(&mut ed, "b undone", |_| !root.join("b.txt").exists());
        step(&mut ed, true).unwrap();
        settle(&mut ed, "a undone", |_| !root.join("a.txt").exists());
        assert_eq!(questions(&ed), ["a", "b"]);
        landed(&ed);
    }

    /// Review round 2: a save reads each conversation's last activity, never its rows (it read the whole chats file
    /// at every change to a turn) — the bytes it reads the same with 1 row a conversation as with 200.
    #[test]
    fn a_save_reads_the_conversations_times_whatever_their_rows() {
        let dir = temp("turns-chat-times");
        let root = dir.join("p");
        let read = |rows: usize| -> (usize, u64) {
            let store = dir.join(format!("chats-{rows}"));
            for i in 0..20 {
                let rows = vec![json!({ "role": "user", "text": "x".repeat(100) }); rows];
                let c = crate::chats::Saved {
                    id: format!("c{i:02}"),
                    title: "t".into(),
                    updated: 1_800_000_000,
                    rows,
                    plan: None,
                    usage: None,
                    places: None,
                    seq: crate::chats::seq(),
                };
                crate::chats::save(&store, &root, c);
            }
            let times = crate::chats::times(&store, &root);
            (times.len(), std::fs::metadata(crate::chats::times_file(&store, &root)).unwrap().len())
        };
        let (few, many) = (read(1), read(200));
        assert_eq!(few.0, 20, "every conversation's time");
        assert_eq!(few, many, "the same bytes whatever the rows");
        assert!(many.1 < 1024, "{} bytes", many.1);
    }

    /// Review round 1: an earlier tarae's turns (`<id>.json` beside the index — no reader of them) go at the next
    /// save, through no link and only what's named so: another name, a folder, a link stay, and what a link leads to.
    #[test]
    fn the_old_formats_turn_files_go_at_a_save() {
        use super::*;
        let dir = temp("turns-old-format");
        let (root, store) = (dir.join("p"), dir.join("turns"));
        let at = folder(&store, &root);
        std::fs::create_dir_all(at.join("456.json")).unwrap();
        let outside = dir.join("kept.json");
        std::fs::write(&outside, "mine").unwrap();
        std::os::unix::fs::symlink(&outside, at.join("789.json")).unwrap();
        for name in ["123.json", "1730000000000000000.json", "12a.json", ".json", "123.json.tmp", "notes.txt"]
        {
            std::fs::write(at.join(name), "{}").unwrap();
        }
        assert!(put(
            &store,
            &root,
            Stored { list: vec![one_to_two(&root, "q")], seq: 1, ..Default::default() }
        ));
        let mut left: Vec<String> = std::fs::read_dir(&at)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        crate::sorting::sort(&mut left);
        let stay =
            [".json", "123.json.tmp", "12a.json", "456.json", "789.json", "index.json", "notes.txt", "texts"];
        assert_eq!(left, stay);
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "mine");
    }

    /// HEAD's moves are kept with their turn (#225, review finding): a checkpoint's come back as they were, and
    /// the turn in flight's go with it when tarae ends mid-turn — its checkpoint, made at the next start, still
    /// says what came with HEAD (the files it left out stay out).
    #[test]
    fn head_moves_are_kept_with_their_turn_and_a_turn_cut_short() {
        let dir = temp("turns-head");
        let (root, store) = (dir.join("p"), dir.join("turns"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "two\n").unwrap();
        let h = |short: &str, files: usize| crate::snapshot::HeadMove {
            from: "1".repeat(40),
            to: "2".repeat(40),
            short: short.into(),
            came: true,
            forward: true,
            files,
            own: 0,
        };
        let (done, flying) = (h("1a2b3c4", 90), h("5d6e7f8", 3));
        let list = vec![Checkpoint { head: vec![done.clone()], ..one_to_two(&root, "q") }];
        let t = Touched {
            path: root.join("a.txt"),
            before: Some(Rope::from_str("one\n")),
            after: Rope::from_str("two\n"),
            unseen: true,
            ..Default::default()
        };
        let (turn, head, question) = (vec![t], vec![flying.clone()], "mid-turn".to_string());
        let seq = crate::chats::seq();
        assert!(put(&store, &root, Stored { list, turn, head, question, seq }));
        // Read back as kept
        let s = load(&store, &root);
        assert_eq!((&s.list[0].head, &s.head), (&vec![done.clone()], &vec![flying.clone()]));
        // A start: the turn cut short is a checkpoint, its HEAD move with it
        let mut ed = Editor::new(crate::config::Config::default());
        (ed.workspace.root, ed.turns_dir) = (root.clone(), Some(store.clone()));
        let hold = hold_saves();
        ed.workspace_kept_back(ed.workspace.id);
        settle(&mut ed, "its turns back", |ed| !ed.restoring());
        let list = &ed.workspace.claude.checkpoints.list;
        let got: Vec<_> = list.iter().map(|c| (c.question.as_str(), c.head.clone())).collect();
        assert_eq!(got, [("q", vec![done]), ("mid-turn", vec![flying])]);
        // #399: the save their coming back asks is a write a quit (and this test's end) waits for
        assert!(!ed.events.writes().wait(std::time::Duration::ZERO), "its save counted as under way");
        drop(hold);
        landed(&ed);
    }

    /// `space v t` on a turn kept from before the start: its texts read first, then its review opens on them —
    /// the file against its text before the turn — and they stay while that review is open, its session
    /// behind or not (the review's diff and its files read them as they're shown).
    #[test]
    fn a_kept_turns_review_reads_its_texts_first() {
        let dir = temp("turns-review");
        let (root, store) = (dir.join("p"), dir.join("turns"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "two\n").unwrap();
        let list = vec![one_to_two(&root, "q")];
        assert!(put(&store, &root, Stored { list, seq: crate::chats::seq(), ..Default::default() }));
        let mut ed = Editor::new(crate::config::Config::default());
        (ed.workspace.root, ed.turns_dir) = (root.clone(), Some(store.clone()));
        ed.workspace_kept_back(ed.workspace.id);
        settle(&mut ed, "its turn back", |ed| !ed.restoring());
        assert!(ed.workspace.claude.checkpoints.list[0].files.here().is_none());
        crate::codereview::review_turn(&mut ed);
        settle(&mut ed, "its review open", |ed| ed.workspace.code_review.is_some());
        let r = ed.workspace.code_review.as_ref().unwrap();
        let files: Vec<_> = r.files.iter().map(|f| (f.rel.as_str(), f.added, f.removed, f.new)).collect();
        assert_eq!(files, [("a.txt", 1, 1, false)]);
        shed(&mut ed.workspace, true);
        assert!(
            ed.workspace.claude.checkpoints.list[0].files.here().is_some(),
            "held while its review is open"
        );
        landed(&ed);
    }

    /// A turn's texts that can't be read just now (here a folder where its file is — a stand-in for too many
    /// files open, as root too) leave the turn and its file as they are, said; its file gone for good leaves
    /// the turn out, said, and the next undo takes the turn before it back.
    #[test]
    fn a_turn_whose_texts_cant_be_read_now_stays_and_one_gone_is_left_out() {
        let dir = temp("turns-unread");
        let (root, store) = (dir.join("p"), dir.join("turns"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "two\n").unwrap();
        // (second's texts its own: b.txt from "x" to "y")
        let first = one_to_two(&root, "first");
        let b = Touched {
            path: root.join("b.txt"),
            before: Some(Rope::from_str("x\n")),
            after: Rope::from_str("y\n"),
            ..Default::default()
        };
        let second = Checkpoint::new("second".into(), vec![b]);
        let texts = texts_dir(&folder(&store, &root)).join(Text::of("y\n").name());
        let list = vec![first, second];
        assert!(put(&store, &root, Stored { list, seq: crate::chats::seq(), ..Default::default() }));
        let mut ed = Editor::new(crate::config::Config::default());
        (ed.workspace.root, ed.turns_dir) = (root.clone(), Some(store.clone()));
        ed.workspace_kept_back(ed.workspace.id);
        settle(&mut ed, "its turns back", |ed| !ed.restoring());
        let questions = |ed: &Editor| {
            ed.workspace.claude.checkpoints.list.iter().map(|c| c.question.clone()).collect::<Vec<_>>()
        };
        let said = |ed: &Editor| ed.toasts.iter().map(|t| t.text.clone()).collect::<Vec<_>>().join(" | ");
        // Can't be read just now
        std::fs::remove_file(&texts).unwrap();
        std::fs::create_dir(&texts).unwrap();
        step(&mut ed, true).unwrap();
        settle(&mut ed, "read", |ed| !ed.workspace.claude.checkpoints.reading);
        assert_eq!(questions(&ed), ["first", "second"], "{}", said(&ed));
        assert!(said(&ed).contains("couldn't read the turn “second”'s changes just now"), "{}", said(&ed));
        assert!(texts.is_dir(), "left as it is");
        // Gone for good
        std::fs::remove_dir(&texts).unwrap();
        step(&mut ed, true).unwrap();
        settle(&mut ed, "read again", |ed| !ed.workspace.claude.checkpoints.reading);
        assert_eq!(questions(&ed), ["first"]);
        assert!(said(&ed).contains("the turn “second” isn't kept any more"), "{}", said(&ed));
        step(&mut ed, true).unwrap();
        let a = root.join("a.txt");
        settle(&mut ed, "the turn before undone", |_| std::fs::read_to_string(&a).unwrap() == "one\n");
        landed(&ed);
    }

    /// A save reads only the index's `seq` (it starts the file — an index whose rest isn't JSON still says it):
    /// an older save writes nothing. A text is one file however many turns hold it — a turn's text after is the
    /// next one's before, and two turns may make a file the same — written once; one no turn the index names any
    /// more holds goes; forgetting leaves no texts.
    #[test]
    fn a_text_is_one_file_whatever_holds_it_and_goes_with_the_last() {
        use super::*;
        let dir = temp("turns-seq");
        let (root, store) = (dir.join("p"), dir.join("turns"));
        let turn = |q: &str, from: &str, to: &str| {
            let t = Touched {
                path: root.join("a.txt"),
                before: Some(Rope::from_str(from)),
                after: Rope::from_str(to),
                ..Default::default()
            };
            Checkpoint::new(q.into(), vec![t])
        };
        // (one → two, two → three, three → two: four texts held, three of them)
        let (a, b, c) =
            (turn("a", "one\n", "two\n"), turn("b", "two\n", "three\n"), turn("c", "three\n", "two\n"));
        let name = |s: &str| Text::of(s).name();
        let (at, index) = (folder(&store, &root), index_file(&store, &root));
        let seq = crate::chats::seq();
        let stored = |list: Vec<Checkpoint>, seq| Stored { list, seq, ..Default::default() };
        let saved = save(&store, &root, stored(vec![a.clone(), b.clone(), c.clone()], seq), Room::ANY);
        assert!(saved.written);
        assert_eq!(seq_of(&index), seq);
        let mut three = vec![name("one\n"), name("two\n"), name("three\n")];
        crate::sorting::sort(&mut three);
        assert_eq!(texts_in(&at), three, "each text once");
        assert_eq!(
            saved.texts.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [a.id, b.id, c.id],
            "all written now"
        );
        assert!(!put(&store, &root, stored(vec![c.clone()], seq - 1)), "an older one");
        assert_eq!(texts_in(&at).len(), 3);
        std::fs::write(&index, format!("{{\"seq\":{},not json", seq + 1)).unwrap();
        assert_eq!(seq_of(&index), seq + 1, "only its head read");
        assert!(!put(&store, &root, stored(vec![c.clone()], seq)));
        // A later one without a: its "one" goes, "two" stays as written — b and c still hold it
        let two = texts_dir(&at).join(name("two\n"));
        let two_was = std::fs::metadata(&two).unwrap().modified().unwrap();
        assert!(put(&store, &root, stored(vec![b.clone(), c.clone()], seq + 2)));
        assert!(!texts_dir(&at).join(name("one\n")).exists());
        assert_eq!(std::fs::metadata(&two).unwrap().modified().unwrap(), two_was, "written once");
        let kept: Vec<Vec<Texts>> = load(&store, &root).list.into_iter().map(|c| c.texts).collect();
        let t = Text::of;
        assert_eq!(kept, [vec![(Some(t("two\n")), t("three\n"))], vec![(Some(t("three\n")), t("two\n"))]]);
        // Forgotten: nothing left
        let mut ed = Editor::new(crate::config::Config::default());
        ed.turns_dir = Some(store.clone());
        forget(&ed, root.clone());
        settle(&mut ed, "forgotten", |_| !two.exists());
        assert!(load(&store, &root).list.is_empty() && texts_in(&at).is_empty());
        landed(&ed);
    }

    /// Wave 3, gap 2: a session behind (back at a start, or opened again) kept its turns nowhere it read —
    /// its first turn saved a list of one over the ones kept for its checkout. Its kept turns come back into
    /// it first, and a turn made meanwhile goes after them, on disk too.
    #[test]
    fn a_session_behind_keeps_its_turns_across_a_start() {
        use super::*;
        let scratch = crate::scratch::Scratch::new("turns-behind");
        let dir = scratch.to_path_buf();
        for p in ["front", "back"] {
            std::fs::create_dir_all(dir.join(p)).unwrap();
        }
        let (front, back, store) = (dir.join("front"), dir.join("back"), dir.join("turns"));
        let turn = |q: &str| {
            let path = back.join("a.txt");
            let (before, after) = (Some(Rope::from_str("one\n")), Rope::from_str("two\n"));
            Checkpoint::new(q.into(), vec![Touched { path, before, after, ..Default::default() }])
        };
        // Kept by an earlier tarae for the checkout behind
        let earlier = Stored {
            list: vec![turn("first"), turn("second")],
            seq: crate::chats::seq(),
            ..Default::default()
        };
        put(&store, &back, earlier);
        let mut ed = Editor::new(crate::config::Config::default());
        (ed.workspace.root, ed.turns_dir) = (front, Some(store.clone()));
        let id = ed.workspace_add(back.clone());
        // A turn there right away, before anything else came
        ed.with_workspace(id, |ed| {
            ed.workspace.claude.checkpoints.list.push(turn("third"));
            keep(ed);
        });
        let questions = |l: &[Checkpoint]| l.iter().map(|c| c.question.clone()).collect::<Vec<_>>();
        let want = ["first", "second", "third"];
        let end = std::time::Instant::now() + crate::test_results::PATIENCE;
        loop {
            let here = questions(&ed.workspace_by_id(id).unwrap().claude.checkpoints.list);
            let kept = questions(&load(&store, &back).list);
            if here == want && kept == want {
                break;
            }
            assert!(std::time::Instant::now() < end, "in the session {here:?} · kept {kept:?}");
            if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(10)) {
                ed.handle_event(ev);
            }
        }
        landed(&ed);
    }
}
