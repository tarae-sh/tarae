//! Unsaved-buffer recovery — a copy of every buffer with unsaved edits, so a crash, a kill, a power cut or a
//! terminal closed under tarae doesn't take what was typed with it.
//!
//! `$XDG_STATE_HOME/tarae/recovery/<pid>-<start ms>/` is one running tarae's (0700): `lock`, an flock held
//! while it runs — the kernel lets go when the process dies, however it dies, and `exec` closes it — and
//! `<doc id>.txt` per buffer: a JSON header line (its file, that file's stamp as the buffer last read or
//! wrote it, the root tarae started in, when), then the whole text. Not the edits since the save: those
//! need the saved file as it was, and a copy matters most when it isn't.
//!
//! One thread writes them (`Keeper`): a buffer's copy once it's been quiet for `QUIET`, or at the latest
//! `MOST` after its first change not yet kept — never on a key, and nothing comes back to the main loop (no
//! event, no frame). The main loop only tells it what changed (`Editor::recovery_sync`, every event: a few
//! compares per buffer, a rope clone when one moved). A buffer saved, undone to its file, reloaded or
//! closed: its copy goes. A quit removes the folder; a quit by a signal (the terminal went) and
//! `:restart`'s exec keep it, written up to the last key first (`Editor::recovery_end`).
//!
//! Beside them `<session>.chat`: the conversation each session's chat is in, one JSON line, told as it changes
//! (the event `chat::keep` runs in, a continue, a new conversation, a chat closed: `moved`, so an event where none moved
//! touches no session). `:restart`'s exec keeps them as it
//! keeps the copies, for its failure: the tarae it starts brings each chat back itself and passes those over
//! (`Recovery.back`), so only a tarae that truly went leaves one to offer.
//!
//! At startup a worker looks at the folders whose lock is free — a tarae that's gone — and each copy of
//! this project's that differs from its file is a card (`What::Recover`): `y` restore · `c` compare · `D`
//! discard · `n` not now (asked again at the next start). Never kept (`never`): the places that hold keys,
//! files named like keys, `editor.recovery-skip`; nor files over `MAX_BYTES`.
//!
//! The conversations it left (`Left`, what the chats' store still holds, the newest per session) ride on the first of
//! those cards, each said with its size and whether it's past its cache: `y` brings them back too, by their ids
//! (`chat::left_back`), `D` starts fresh. None to ride on: a card of their own (`What::RecoverChats`). What comes
//! back is shown, and acts on your key.

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ropey::Rope;
use serde_json::{Value, json};

use crate::disk::Stamp;
use crate::document::{DocId, Document};
use crate::editor::Editor;
use crate::event::WorkspaceId;
use crate::offer::{Offer, What};

/// A buffer quiet this long has its copy written…
const QUIET: Duration = Duration::from_secs(1);
/// …and one typed into without a pause, this long after its first change not yet kept.
const MOST: Duration = Duration::from_secs(10);
/// Buffers larger than this aren't kept (persistent undo's limit too).
pub const MAX_BYTES: usize = 8 << 20;
/// How long the end of the process waits for the copies to be written, or removed.
const EXIT_WAIT: Duration = Duration::from_secs(5);
const LOCK: &str = "lock";
const COPY: &str = "txt";
const CHAT: &str = "chat";
/// What a conversation's `read anew` means, on a card that says one.
const READ_ANEW: &str = "Read anew: past its cache, its next message sends it all again.";
/// A compare buffer's title: this, then the file.
const COMPARE: &str = "recovered · ";

/// What decides, besides names like keys' (`secretnames`) and the places that hold keys, that a file is never kept: the user's
/// `editor.recovery-skip`, the sessions' roots its relative entries are from, and home (`~/` entries, and
/// where the key places are).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rules {
    pub home: Option<PathBuf>,
    pub skip: Vec<String>,
    pub roots: Vec<PathBuf>,
}

/// Whether the file at `path` is never kept: in a place that holds keys (`places`, real paths — a link into
/// `~/.ssh` counts), named like a key, or in `editor.recovery-skip`. On the keeper (it resolves the path).
fn never(path: &Path, rules: &Rules, places: &[PathBuf]) -> bool {
    let real = crate::sandbox::profile_path(path);
    let named = crate::secretnames::named_secret;
    places.iter().any(|p| real.starts_with(p)) || named(path) || named(&real) || skipped(&real, rules)
}

/// The places that hold keys, as real paths: what Claude's runs never read (`sandbox::kept_reads` —
/// `allow::SECRETS`, Claude's own, the toolchains' credentials wherever their homes are, tarae's state).
fn key_places(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else { return Vec::new() };
    crate::sandbox::kept_reads(home, &crate::sandbox::vars_in(home, &crate::sandbox::Here::now()))
}

/// `editor.recovery-skip`: a name (one `*` at most) matches any part of the path in any case; an entry
/// with a `/` is a path — from a session's root, or from `/` or `~/`.
fn skipped(real: &Path, rules: &Rules) -> bool {
    // (a `/` anywhere makes it a path — `private/` too)
    let entries = rules.skip.iter().map(|s| (s.trim().contains('/'), s.trim().trim_end_matches('/')));
    entries.filter(|(_, s)| !s.is_empty()).any(|(path, s)| {
        if let Some(rest) = s.strip_prefix("~/") {
            let at = rules.home.as_ref().map(|h| crate::sandbox::profile_path(&h.join(rest)));
            return at.is_some_and(|at| real.starts_with(at));
        }
        if s.starts_with('/') {
            return real.starts_with(crate::sandbox::profile_path(Path::new(s)));
        }
        if path {
            return rules.roots.iter().any(|r| real.strip_prefix(r).is_ok_and(|rel| rel.starts_with(s)));
        }
        let s = s.to_lowercase();
        real.iter().any(|p| crate::secretnames::wild(&s, &p.to_string_lossy().to_lowercase()))
    })
}

fn nanos(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

/// A copy's first line.
fn header(path: &Path, root: &Path, disk: Option<Stamp>, conflict: bool, len: usize) -> String {
    json!({
        "v": 1,
        "path": path.to_string_lossy(),
        "root": root.to_string_lossy(),
        "disk": disk.map(|(t, n)| json!([nanos(t), n])),
        "conflict": conflict,
        "at": nanos(SystemTime::now()),
        "len": len,
    })
    .to_string()
}

/// A copy as read back.
#[derive(Debug)]
struct Copy {
    path: PathBuf,
    root: PathBuf,
    /// The file's stamp as the buffer knew it: (mtime ns, length)
    disk: Option<(u64, u64)>,
    conflict: bool,
    at: SystemTime,
    text: String,
}

/// A copy's file read back — None when it isn't one, or was cut short.
fn read_copy(file: &Path) -> Option<Copy> {
    let bytes = std::fs::read(file).ok()?;
    let nl = memchr::memchr(b'\n', &bytes)?;
    let v: Value = serde_json::from_slice(&bytes[..nl]).ok()?;
    let text = String::from_utf8(bytes[nl + 1..].to_vec()).ok()?;
    if v["v"].as_u64() != Some(1) || v["len"].as_u64()? != text.len() as u64 {
        return None;
    }
    let disk = v["disk"].as_array().and_then(|d| Some((d.first()?.as_u64()?, d.get(1)?.as_u64()?)));
    Some(Copy {
        path: PathBuf::from(v["path"].as_str()?),
        root: PathBuf::from(v["root"].as_str().unwrap_or_default()),
        disk,
        conflict: v["conflict"].as_bool().unwrap_or(false),
        at: UNIX_EPOCH + Duration::from_nanos(v["at"].as_u64().unwrap_or(0)),
        text,
    })
}

/// A session's `.chat` line.
fn chat_line(root: &Path, origin: &Path, name: &str, id: &str) -> String {
    json!({
        "v": 1,
        "root": root.to_string_lossy(),
        "origin": origin.to_string_lossy(),
        "name": name,
        "id": id,
        "at": nanos(crate::clock::system()),
    })
    .to_string()
}

/// A `.chat` file as read back.
#[derive(Debug)]
struct ChatFile {
    /// The session's checkout · where its tarae started
    root: PathBuf,
    origin: PathBuf,
    name: String,
    id: String,
    at: SystemTime,
}

/// A `.chat` file read back — None when it isn't one.
fn read_chat(file: &Path) -> Option<ChatFile> {
    let v: Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    if v["v"].as_u64() != Some(1) {
        return None;
    }
    Some(ChatFile {
        root: PathBuf::from(v["root"].as_str()?),
        origin: PathBuf::from(v["origin"].as_str()?),
        name: v["name"].as_str().unwrap_or_default().to_string(),
        id: v["id"].as_str()?.to_string(),
        at: UNIX_EPOCH + Duration::from_nanos(v["at"].as_u64().unwrap_or(0)),
    })
}

/// Tests: what a gone tarae keeps of session `session`'s chat, in its folder `dir`.
#[cfg(test)]
pub fn chat_left(dir: &Path, session: u64, root: &Path, origin: &Path, id: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    std::fs::write(dir.join(format!("{session}.{CHAT}")), chat_line(root, origin, &name, id)).unwrap();
}

/// A folder's files with extension `ext`.
fn with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == ext)).collect()
}

/// The copies in a folder.
fn copies(dir: &Path) -> Vec<PathBuf> {
    with_ext(dir, COPY)
}

/// The conversations a folder keeps.
fn chats_in(dir: &Path) -> Vec<PathBuf> {
    with_ext(dir, CHAT)
}

/// A folder with nothing left in it to ask about.
fn spent(dir: &Path) -> bool {
    copies(dir).is_empty() && chats_in(dir).is_empty()
}

/// This tarae's folder, made and locked — `run`, or `run.N` if a tarae starting up holds that one's lock
/// (it took it for a gone tarae's, and finds it empty).
fn take_folder(run: &Path) -> std::io::Result<(PathBuf, File)> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let mut last = None;
    for n in 0..4 {
        let dir = match n {
            0 => run.to_path_buf(),
            n => PathBuf::from(format!("{}.{n}", run.display())),
        };
        let made = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir).and_then(|()| {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
            std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(LOCK))
        });
        match made.map(|f| (f.try_lock(), f)) {
            Ok((Ok(()), f)) => return Ok((dir, f)),
            Ok((Err(std::fs::TryLockError::WouldBlock), _)) => {}
            Ok((Err(std::fs::TryLockError::Error(e)), _)) | Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("every folder it tried was taken")))
}

enum Msg {
    Keep {
        id: DocId,
        path: PathBuf,
        text: Rope,
        disk: Option<Stamp>,
        conflict: bool,
    },
    Drop(DocId),
    /// Session `session`'s chat is in conversation `id` · in none.
    Chat {
        session: WorkspaceId,
        root: PathBuf,
        name: String,
        id: String,
    },
    ChatGone(WorkspaceId),
    Rules(Rules),
    /// Write every copy that waits, now — then answer.
    Flush(Sender<()>),
    /// Remove every copy and the folder — then answer, and end.
    Clear(Sender<()>),
    /// A gone tarae's copy answered: removed — and with the last of its folder's cards, the folder if it's
    /// empty, then its lock let go. (Here, not on a worker of its own: the end of the process waits for the
    /// keeper, so a quit right after `y` can't leave the copy to be offered again.)
    Forget {
        file: PathBuf,
        folder: Option<(PathBuf, File)>,
    },
}

/// A buffer's text waiting for its pause.
struct Waiting {
    path: PathBuf,
    text: Rope,
    disk: Option<Stamp>,
    conflict: bool,
    changed: Instant,
    first: Instant,
}

/// The thread that writes the copies.
struct Keeper {
    run: PathBuf,
    root: PathBuf,
    quiet: Duration,
    /// The folder in use and its lock (taken with the first copy)
    folder: Option<(PathBuf, File)>,
    waiting: std::collections::HashMap<DocId, Waiting>,
    /// Buffers whose copy is on disk
    kept: Vec<DocId>,
    rules: Rules,
    /// `key_places` for `rules.home` (worked out with the first copy)
    places: Option<Vec<PathBuf>>,
    /// A failure said in the log already
    said: bool,
    #[cfg(test)]
    slow: Duration,
}

impl Keeper {
    fn due(&self, w: &Waiting) -> Instant {
        (w.changed + self.quiet).min(w.first + MOST)
    }

    fn run(mut self, rx: Receiver<Msg>) {
        loop {
            let due = self.waiting.values().map(|w| self.due(w)).min();
            let msg = match due {
                Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                    Ok(m) => Some(m),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                },
                None => match rx.recv() {
                    Ok(m) => Some(m),
                    Err(_) => return,
                },
            };
            match msg {
                Some(Msg::Keep { id, path, text, disk, conflict }) => {
                    let changed = Instant::now();
                    let first = self.waiting.get(&id).map_or(changed, |w| w.first);
                    self.waiting.insert(id, Waiting { path, text, disk, conflict, changed, first });
                }
                Some(Msg::Drop(id)) => {
                    self.waiting.remove(&id);
                    self.remove(id);
                }
                Some(Msg::Chat { session, root, name, id }) => self.chat(session, &root, &name, &id),
                Some(Msg::ChatGone(session)) => {
                    if let Some((dir, _)) = &self.folder {
                        let _ = std::fs::remove_file(dir.join(format!("{session}.{CHAT}")));
                    }
                }
                Some(Msg::Rules(rules)) => {
                    if rules.home != self.rules.home {
                        self.places = None;
                    }
                    self.rules = rules;
                }
                Some(Msg::Flush(done)) => {
                    let all: Vec<DocId> = self.waiting.keys().copied().collect();
                    all.into_iter().for_each(|id| self.write(id));
                    let _ = done.send(());
                }
                Some(Msg::Clear(done)) => {
                    self.waiting.clear();
                    if let Some((dir, _lock)) = self.folder.take() {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                    let _ = done.send(());
                    return;
                }
                Some(Msg::Forget { file, folder }) => {
                    let _ = std::fs::remove_file(file);
                    if let Some((dir, _lock)) = folder
                        && spent(&dir)
                    {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                }
                None => {}
            }
            let now = Instant::now();
            let due: Vec<DocId> =
                self.waiting.iter().filter(|(_, w)| self.due(w) <= now).map(|(id, _)| *id).collect();
            due.into_iter().for_each(|id| self.write(id));
        }
    }

    fn write(&mut self, id: DocId) {
        let Some(w) = self.waiting.remove(&id) else { return };
        if self.places.is_none() {
            self.places = Some(key_places(self.rules.home.as_deref()));
        }
        // (a buffer renamed to a key's name since its last copy loses that one too)
        if never(&w.path, &self.rules, self.places.as_deref().unwrap_or_default()) {
            return self.remove(id);
        }
        #[cfg(test)]
        std::thread::sleep(self.slow);
        let Some(dir) = self.folder() else { return };
        let file = dir.join(format!("{id}.{COPY}"));
        let head = header(&w.path, &self.root, w.disk, w.conflict, w.text.len_bytes());
        let written = crate::disk::write_atomic(&file, |f| {
            f.write_all(head.as_bytes())?;
            f.write_all(b"\n")?;
            w.text.write_to(f)
        });
        match written {
            Ok(()) if !self.kept.contains(&id) => self.kept.push(id),
            Ok(()) => {}
            Err(e) => self.failed(format!("{}'s copy not written: {e}", w.path.display())),
        }
    }

    /// This tarae's folder, taken with the first thing it keeps.
    fn folder(&mut self) -> Option<PathBuf> {
        if self.folder.is_none() {
            match take_folder(&self.run) {
                Ok(f) => self.folder = Some(f),
                Err(e) => {
                    self.failed(format!("no folder for the copies of unsaved buffers: {e}"));
                    return None;
                }
            }
        }
        self.folder.as_ref().map(|(dir, _)| dir.clone())
    }

    fn chat(&mut self, session: WorkspaceId, root: &Path, name: &str, id: &str) {
        let Some(dir) = self.folder() else { return };
        let line = chat_line(root, &self.root, name, id);
        let written = crate::disk::write_atomic(&dir.join(format!("{session}.{CHAT}")), |f| {
            f.write_all(line.as_bytes())
        });
        if let Err(e) = written {
            self.failed(format!("{name}'s conversation not kept: {e}"));
        }
    }

    fn remove(&mut self, id: DocId) {
        if let Some((dir, _)) = &self.folder
            && self.kept.contains(&id)
        {
            let _ = std::fs::remove_file(dir.join(format!("{id}.{COPY}")));
            self.kept.retain(|&k| k != id);
        }
    }

    /// A copy that couldn't be written — said in the log, once (a full disk would say it every pause).
    fn failed(&mut self, msg: String) {
        if !std::mem::replace(&mut self.said, true) {
            crate::log::line("recovery", "warn", &msg);
        }
    }
}

/// What the keeper was last told about a buffer.
struct Told {
    id: DocId,
    version: u64,
    disk: Option<Stamp>,
    conflict: bool,
    path: PathBuf,
}

impl Told {
    fn is(&self, d: &Document) -> bool {
        self.version == d.version()
            && self.disk == d.disk
            && self.conflict == d.disk_conflict
            && d.path.as_ref() == Some(&self.path)
    }
}

/// A gone tarae's folder, held (its lock) while cards ask about its copies.
struct Held {
    dir: PathBuf,
    lock: File,
    cards: usize,
}

pub struct Recovery {
    /// `$state/recovery`
    dir: PathBuf,
    /// This tarae's folder in it (made with the first copy)
    run: PathBuf,
    /// Where tarae started — the project a copy is offered in again
    root: PathBuf,
    tx: Option<Sender<Msg>>,
    thread: Option<std::thread::JoinHandle<()>>,
    told: Vec<Told>,
    /// The conversation told for each session, and the name it was told under
    chats: Vec<(WorkspaceId, String, String)>,
    /// Conversations this tarae has back already (`:restart`'s): nothing to offer
    back: Vec<String>,
    /// A session's conversation moved, or a session came or went, since `recovery_sync` looked (`moved`)
    moved: bool,
    /// Conversations are told (`editor.recovery`, a chats' store)
    chats_on: bool,
    /// A restore brought the front chat back: said after the next frame if it didn't draw it (`chat_drawn`)
    unseen: bool,
    rules: Rules,
    held: Vec<Held>,
    /// Restores waiting for a big file's text to load
    after_load: Vec<(DocId, Found)>,
    /// Compare buffers: (the file, the buffer)
    compares: Vec<(PathBuf, DocId)>,
    /// The process ends keeping the copies — a signal quit (the terminal went), else a quit removes them.
    pub keep: bool,
    pub quiet: Duration,
    /// Tests: a slow disk's stand-in — each copy takes this long to write · startup looks answered
    #[cfg(test)]
    pub slow: Duration,
    #[cfg(test)]
    pub scans: usize,
    /// Tests: sessions `recovery_sync` looked at
    #[cfg(test)]
    pub looks: usize,
}

/// What `recovery_sync` looks at moved: a session's chat is in another conversation or none, a session came or went, or
/// its name did (the home session's is its branch).
/// (`Editor.recovery` alone: said where a chat is in hand.)
pub fn moved(recovery: &mut Option<Recovery>) {
    if let Some(r) = recovery {
        r.moved = true;
    }
}

impl Recovery {
    /// Copies in `dir` (`$state/recovery`), for a tarae started in `root`.
    pub fn new(dir: PathBuf, root: PathBuf) -> Recovery {
        // (numbered too: two editors of one process — tests — in the same millisecond)
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let start = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        Recovery {
            run: dir.join(format!("{}-{start}-{n}", std::process::id())),
            dir,
            root,
            tx: None,
            thread: None,
            told: Vec::new(),
            chats: Vec::new(),
            back: Vec::new(),
            moved: true,
            chats_on: false,
            unseen: false,
            rules: Rules { home: crate::allow::home(), ..Default::default() },
            held: Vec::new(),
            after_load: Vec::new(),
            compares: Vec::new(),
            keep: false,
            quiet: QUIET,
            #[cfg(test)]
            slow: Duration::ZERO,
            #[cfg(test)]
            scans: 0,
            #[cfg(test)]
            looks: 0,
        }
    }

    /// Conversation `id` is back in a chat here, `:restart`'s.
    pub fn back(&mut self, id: String) {
        self.back.push(id);
    }

    /// To the keeper — started with the first message.
    fn send(&mut self, msg: Msg) {
        if self.tx.is_none() {
            let (tx, rx) = channel();
            let keeper = Keeper {
                run: self.run.clone(),
                root: self.root.clone(),
                quiet: self.quiet,
                folder: None,
                waiting: Default::default(),
                kept: Vec::new(),
                rules: self.rules.clone(),
                places: None,
                said: false,
                #[cfg(test)]
                slow: self.slow,
            };
            let thread = std::thread::Builder::new().name("tarae-recovery".into()).spawn(|| keeper.run(rx));
            self.thread = thread.ok();
            self.tx = Some(tx);
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(msg);
        }
    }

    /// Asks the keeper `msg` and waits for its answer, up to `within` — whether it answered.
    fn ask(&mut self, msg: impl FnOnce(Sender<()>) -> Msg, within: Duration) -> bool {
        let Some(tx) = &self.tx else { return true };
        let (done, answered) = channel();
        tx.send(msg(done)).is_err() || answered.recv_timeout(within).is_ok()
    }

    /// Tests: the process dies — the keeper stops where it is and its lock goes, its copies stay. Gone once
    /// no process has it open: a child another test forked while it was open holds it too, until it execs
    /// (a fork copies every descriptor; close-on-exec closes them only at the exec) — under load long enough
    /// for the next scan to take this tarae for a running one and pass its copies over (seen on Linux: a
    /// `queued::tests` child, not yet exec'd). A real death lets it go the same way; this waits for that.
    #[cfg(test)]
    pub fn killed(&mut self) {
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // (the folder the keeper took: `run`, or `run.N` — `take_folder`)
        let folders = (0..4).map(|n| match n {
            0 => self.run.clone(),
            n => PathBuf::from(format!("{}.{n}", self.run.display())),
        });
        let end = Instant::now() + crate::test_results::PATIENCE;
        for lock in folders.map(|d| d.join(LOCK)).filter(|l| l.exists()) {
            let free = || File::open(&lock).is_ok_and(|f| f.try_lock().is_ok());
            while !free() {
                assert!(Instant::now() < end, "{} still held 10 s after the keeper ended", lock.display());
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

    /// Tests: what waits is written now.
    #[cfg(test)]
    pub fn flush(&mut self) -> bool {
        self.ask(Msg::Flush, EXIT_WAIT)
    }

    /// This tarae's folder (made with its first copy).
    #[cfg(test)]
    pub fn folder(&self) -> &Path {
        &self.run
    }

    /// Tests: a restore brought the front chat back (`chat_back`), not said yet.
    #[cfg(test)]
    pub fn back_unsaid(&self) -> bool {
        self.unseen
    }
}

/// Whether a buffer's unsaved edits are kept: a file's (not a scratch, a commit message, a class file's
/// source), loaded, not too big. Where it is (`never`) is the keeper's to say.
fn keepable(d: &Document) -> bool {
    d.is_modified()
        && d.path.is_some()
        && !d.loading
        && !d.throwaway
        && d.virtual_uri.is_none()
        && d.text.len_bytes() <= MAX_BYTES
}

/// What became of a copy's file since.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disk {
    /// As the buffer knew it.
    Same,
    /// It had changed on disk while the buffer was being edited (a conflict, before tarae ended).
    Conflict,
    /// Changed since — at this time.
    Changed(SystemTime),
    /// Deleted since.
    Gone,
    /// It was a new file, never saved (and still isn't there).
    New,
}

/// A copy newer than its file — one card.
#[derive(Clone, Debug)]
pub struct Found {
    pub path: PathBuf,
    /// The copy's file, and its folder
    file: PathBuf,
    dir: PathBuf,
    pub text: Rope,
    pub at: SystemTime,
    pub disk: Disk,
    /// The file as it is now (empty when there's none)
    pub base: Rope,
    /// Lines the copy has that the file hasn't · the file has that the copy hasn't
    pub added: usize,
    pub removed: usize,
    /// The conversations `y` brings back with it
    pub chats: Vec<Left>,
}

/// A conversation a tarae that's gone was in.
#[derive(Clone, Debug)]
pub struct Left {
    /// Its session's checkout, and that session's name then
    pub root: PathBuf,
    name: String,
    pub past: crate::chats::Past,
    /// Past its cache's TTL: its next message reads the whole context anew
    pub cold: bool,
    at: SystemTime,
    /// The `.chat` file, and its folder
    file: PathBuf,
    dir: PathBuf,
}

/// What `scan` found: the folders held, the copies, the conversations.
#[derive(Default)]
struct Scanned {
    held: Vec<(PathBuf, File)>,
    found: Vec<Found>,
    left: Vec<Left>,
}

/// The copies left by taraes that are gone — those of this project (started in `origin`, or a file under
/// one of `roots`) that differ from their file, the newest per file, their folders held; a copy that's its
/// file already goes, and so does an older copy of a file offered. The conversations likewise, those `store` (the
/// chats' folder) still holds of a session whose checkout is there, the newest per session, but those in `back` — a `chat`
/// that's any of those has nothing to continue and goes. Another project's are left for a tarae started there. (A worker.)
fn scan(
    dir: &Path,
    own: &Path,
    origin: &Path,
    roots: &[PathBuf],
    store: Option<&Path>,
    back: &[String],
) -> Scanned {
    let Scanned { mut held, mut found, mut left } = Scanned::default();
    let Ok(rd) = std::fs::read_dir(dir) else { return Scanned::default() };
    let own = own.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    for e in rd.flatten() {
        let folder = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if !folder.is_dir() || name == own || name.starts_with(&format!("{own}.")) {
            continue;
        }
        let lock =
            std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(folder.join(LOCK));
        // (its tarae is running)
        let Some(lock) = lock.ok().filter(|l| l.try_lock().is_ok()) else { continue };
        let mut mine = 0;
        for file in copies(&folder) {
            let Some(c) = read_copy(&file) else { continue };
            if c.root != origin && !roots.iter().any(|r| c.path.starts_with(r)) {
                continue;
            }
            let now = crate::disk::stamp(&c.path);
            let base = now.and_then(|_| std::fs::read_to_string(&c.path).ok());
            if base.as_deref() == Some(c.text.as_str()) {
                let _ = std::fs::remove_file(&file);
                continue;
            }
            let disk = match (now, c.disk) {
                (None, None) => Disk::New,
                (None, Some(_)) => Disk::Gone,
                (Some((t, n)), was) if was != Some((nanos(t), n)) => Disk::Changed(t),
                _ if c.conflict => Disk::Conflict,
                _ => Disk::Same,
            };
            let base = base.unwrap_or_default();
            let hunks = crate::git::hunks(&base, &c.text);
            found.push(Found {
                path: c.path,
                file,
                dir: folder.clone(),
                added: hunks.iter().map(|h| h.lines.len()).sum(),
                removed: hunks.iter().map(|h| h.base.len()).sum(),
                text: Rope::from(c.text),
                at: c.at,
                disk,
                base: Rope::from(base),
                chats: Vec::new(),
            });
            mine += 1;
        }
        for file in chats_in(&folder) {
            let Some(c) = read_chat(&file) else { continue };
            if c.origin != origin && !roots.contains(&c.root) {
                continue;
            }
            let there = c.root.is_dir() && !back.contains(&c.id);
            let kept = store.filter(|_| there).map(|s| crate::chats::load(s, &c.root));
            let Some(saved) = kept.into_iter().flatten().find(|s| s.id == c.id) else {
                let _ = std::fs::remove_file(&file);
                continue;
            };
            let now = crate::usage::now_ms();
            let cold = saved.usage.as_ref().is_some_and(|u| crate::usage::Usage::kept_cold(u, now));
            left.push(Left {
                root: c.root,
                name: c.name,
                past: saved.past(),
                cold,
                at: c.at,
                file,
                dir: folder.clone(),
            });
            mine += 1;
        }
        if spent(&folder) {
            let _ = std::fs::remove_dir_all(&folder);
        } else if mine > 0 {
            held.push((folder, lock));
        }
    }
    // One card per file: its newest copy
    crate::sorting::by(&mut found, |a, b| a.path.cmp(&b.path).then(b.at.cmp(&a.at)));
    let mut seen: Option<PathBuf> = None;
    found.retain(|f| {
        let older = seen.as_ref() == Some(&f.path);
        if older {
            let _ = std::fs::remove_file(&f.file);
        }
        seen = Some(f.path.clone());
        !older
    });
    // One conversation per session: its newest; the start's first
    crate::sorting::by(&mut left, |a, b| a.root.cmp(&b.root).then(b.at.cmp(&a.at)));
    let mut seen: Option<PathBuf> = None;
    left.retain(|l| {
        let older = seen.as_ref() == Some(&l.root);
        if older {
            let _ = std::fs::remove_file(&l.file);
        }
        seen = Some(l.root.clone());
        !older
    });
    crate::sorting::by_key(&mut left, |l| l.root != origin);
    held.retain(|(d, _)| found.iter().any(|f| f.dir == *d) || left.iter().any(|l| l.dir == *d));
    Scanned { held, found, left }
}

/// `at 14:32`, or `on 10-03 at 14:32` before today — in the interface's zone.
fn when(t: SystemTime, zone: crate::log::Zone) -> String {
    let (_, m, d) = zone.day(t);
    match zone.day(t) == zone.day(SystemTime::now()) {
        true => format!("at {}", zone.clock(t)),
        false => format!("on {m:02}-{d:02} at {}", zone.clock(t)),
    }
}

/// `path` as the card names it: from the root, else whole.
fn shown(path: &Path, root: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

/// A conversation's row on a card — `41k · 2h ago · read anew · "why does adds() fail?"`, its session's name first where
/// there are several. The title last: a row is cut at its end (whole under the mouse), and what `y` costs stays in sight.
fn chat_row(l: &Left, named: bool, now: u64) -> String {
    let parts = named.then(|| l.name.clone()).into_iter();
    let parts = parts.chain(l.past.context.map(crate::usage::tokens));
    let parts = parts.chain([crate::chats::ago(l.past.updated, now)]);
    let parts = parts.chain(l.cold.then(|| "read anew".to_string()));
    let title = l.past.title.lines().next().unwrap_or_default();
    let parts = parts.chain([format!("\"{title}\"")]);
    parts.collect::<Vec<_>>().join(" · ")
}

/// The rows `chats_said` lists from line `from` that begin with their session's name (`Offer.named`).
fn named_rows(left: &[Left], from: usize) -> Vec<(usize, usize, PathBuf)> {
    let rows = left.iter().enumerate().filter(|_| left.len() > 1);
    rows.map(|(i, l)| (from + 1 + i, l.name.len(), l.root.clone())).collect()
}

/// The conversations as a card lists them, after `lead`: its lines from line `from` of its question, and those lines as
/// rows (`Offer.rows`).
fn chats_said(left: &[Left], lead: &str, from: usize) -> (String, Vec<(usize, String)>) {
    let now = crate::chats::now();
    let rows: Vec<String> = left.iter().map(|l| chat_row(l, left.len() > 1, now)).collect();
    let lines = format!("{lead}\n{}", rows.join("\n"));
    (lines, rows.into_iter().enumerate().map(|(i, row)| (from + 1 + i, row)).collect())
}

/// The card for a copy found.
fn card(f: Found, root: &Path, past: &str, zone: crate::log::Zone) -> Offer {
    // (when the copy was kept — tarae may have gone on a while after)
    let lines = match (f.added, f.removed) {
        (0, 0) => "spaces or the last newline".to_string(),
        (a, r) => format!("+{a} −{r} lines"),
    };
    let mut q = format!("Not saved when tarae ended: {lines}, kept {}.", when(f.at, zone));
    // (what `y` decides, at full strength with the edits: the first paragraph)
    let mut listed = Vec::new();
    if !f.chats.is_empty() {
        let lead = match f.chats.len() {
            1 => "Its conversation comes back with them:",
            _ => "The conversations come back with them:",
        };
        let (lines, rows) = chats_said(&f.chats, lead, 1);
        q.push('\n');
        q.push_str(&lines);
        listed = rows;
    }
    q.push_str("\n\n");
    let disk_line = q.split('\n').count() - 1;
    let warn = !matches!(f.disk, Disk::Same | Disk::New);
    q.push_str(&match f.disk {
        Disk::Same => "The file on disk is as it was then.".to_string(),
        Disk::New => "It was a new file, never saved.".to_string(),
        Disk::Conflict => "The file changed while they were made — they'd go over it.".to_string(),
        Disk::Changed(t) => format!("The file changed since, {} — they'd go over it.", when(t, zone)),
        Disk::Gone => "The file was deleted since — :w writes it again.".to_string(),
    });
    q.push_str("\nRestored, they stay unsaved — u goes back to the file.");
    if !f.chats.is_empty() {
        let it = if f.chats.len() == 1 { "it" } else { "them" };
        q.push_str(&format!("\nDiscarded, the chat starts fresh too; {past} still lists {it}."));
    }
    if f.chats.iter().any(|l| l.cold) {
        q.push('\n');
        q.push_str(READ_ANEW);
    }
    let title = format!("Unsaved edits · {}", shown(&f.path, root));
    let named = named_rows(&f.chats, 1);
    let mut o = Offer::new(title, &q, "Restoring…", "Not restored".into(), What::Recover(Box::new(f)));
    o.rows = listed;
    o.named = named;
    if warn { o.warning(disk_line..disk_line + 1) } else { o }
}

/// The card for conversations with no copy to ride on — `past`: the key that lists a chat's conversations, as it's said.
fn chats_card(left: Vec<Left>, past: &str) -> Offer {
    let (title, lead) = match left.len() {
        1 => ("A conversation left open".to_string(), "The chat was in it when tarae ended:"),
        n => (format!("{n} conversations left open"), "The chats were in them when tarae ended:"),
    };
    let (mut q, rows) = chats_said(&left, lead, 0);
    q.push_str(&format!(
        "\n\nRestored, the chat goes on from it. Discarded, it starts fresh; {past} still lists it."
    ));
    if left.iter().any(|l| l.cold) {
        q.push('\n');
        q.push_str(READ_ANEW);
    }
    let named = named_rows(&left, 0);
    let mut o = Offer::new(title, &q, "Restoring…", "Not restored".into(), What::RecoverChats(left));
    o.rows = rows;
    o.named = named;
    o
}

/// Tests: the card of conversations with their sessions' names and checkouts.
#[cfg(test)]
pub fn chats_card_of(sessions: &[(&str, &Path)]) -> Offer {
    chats_card(lefts_of(sessions), "C-p")
}

/// Tests: conversations of sessions with these names and checkouts.
#[cfg(test)]
fn lefts_of(sessions: &[(&str, &Path)]) -> Vec<Left> {
    let left = |&(name, root): &(&str, &Path)| Left {
        root: root.to_path_buf(),
        name: name.to_string(),
        // (a title long enough that its row is cut)
        past: crate::chats::Past {
            id: name.into(),
            title: "t".repeat(99),
            updated: 0,
            context: Some(41_000),
        },
        cold: false,
        at: UNIX_EPOCH,
        file: PathBuf::new(),
        dir: PathBuf::new(),
    };
    sessions.iter().map(left).collect()
}

/// Why `y` can't restore `f` now — an open buffer of it with edits of its own, or one still loading.
pub fn refuses(ed: &Editor, f: &Found) -> Option<String> {
    let d = ed.docs.iter().find(|d| d.path.as_ref() == Some(&f.path))?;
    match (d.loading, d.is_modified()) {
        (true, _) => Some("it's still loading — y in a moment".into()),
        (_, true) => Some(format!(
            "{} has unsaved edits of yours — save or :reload it first, then y",
            d.display_name_short()
        )),
        _ => None,
    }
}

/// `y`: the file open (or its buffer), the copy's text put in as one change — unsaved, `u` back to the file; the
/// conversations on the card back in their sessions' chats.
pub fn restore(ed: &mut Editor, card: u64, mut f: Found) {
    ed.offer_finished(card, Ok(()));
    chats_back(ed, std::mem::take(&mut f.chats));
    compare_close(ed, &f.path);
    // (not opened: the copy stays, asked about again at the next start)
    if let Err(e) = ed.open(&f.path) {
        return ed.set_error(format!("not restored — {e:#}"));
    }
    let id = ed.doc().id;
    if ed.doc().loading {
        // (a big file: once its text is in — `recovery_loaded`)
        ed.set_status(format!("restoring {} once it's loaded", ed.doc().display_name_short()));
        if let Some(r) = ed.recovery.as_mut() {
            r.after_load.push((id, f));
        }
        return;
    }
    put(ed, id, &f);
}

/// `y` on a card of conversations alone.
pub fn restore_chats(ed: &mut Editor, card: u64, left: Vec<Left>) {
    ed.offer_finished(card, Ok(()));
    chats_back(ed, left);
}

/// The session of checkout `root`, in front or behind.
fn session_of(ed: &Editor, root: &Path) -> Option<WorkspaceId> {
    std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|w| w.root == root).map(|w| w.id)
}

/// Each conversation back by its id, in its session's chat — its `.chat` file gone once that's asked.
fn chats_back(ed: &mut Editor, left: Vec<Left>) {
    for l in left {
        match session_of(ed, &l.root) {
            Some(session) => {
                let id = l.past.id.clone();
                ed.with_workspace(session, |ed| crate::chat::left_back(ed, id));
                forget_chat(ed, &l);
            }
            // (its checkout's there, its session isn't — not back yet, or no longer listed: nothing done, nothing lost)
            None => {
                drop(release(ed, &l.dir));
                ed.set_warning(format!(
                    "{}'s session isn't here — asked again when tarae next starts",
                    l.name
                ));
            }
        }
    }
}

/// `chat::loaded`: the conversation a restore asked for is in this session's chat. In front it says itself where it's
/// drawn (`chat_drawn`); behind, a toast says so, with the session's name (`said_by_session`).
pub fn chat_back(ed: &mut Editor) {
    match (ed.behind, ed.recovery.as_mut()) {
        (false, Some(r)) => r.unseen = true,
        _ => ed.set_success("Restored the chat"),
    }
}

/// After a frame: the front chat a restore just brought back, if this frame didn't draw it (too narrow a screen, a pane
/// zoomed), said; drawn, it says itself. Whether a toast was made (the frame is drawn again with it).
pub fn chat_drawn(ed: &mut Editor) -> bool {
    let Some(r) = ed.recovery.as_mut() else { return false };
    if !std::mem::take(&mut r.unseen) || ed.screen.chat_x().is_some() || ed.screen.chat_frame.is_some() {
        return false;
    }
    ed.set_success("Restored the chat — space l shows it");
    true
}

/// The copy's text into buffer `id`, as one change — and the copy goes.
fn put(ed: &mut Editor, id: DocId, f: &Found) {
    let Some(doc) = ed.docs.iter_mut().find(|d| d.id == id) else { return };
    if let Some(change) = crate::disk::minimal_change(&doc.text, &f.text) {
        let before = doc.snapshot();
        let tx = crate::transaction::Transaction::new(vec![change]);
        let sel = tx.map_selection(doc.selection());
        doc.apply_with(&tx, sel);
        doc.own.not_yours();
        doc.commit_undo(before);
    }
    let name = doc.display_name_short();
    ed.set_success(format!("Restored {name} — unsaved: :w writes it, u goes back to the file"));
    forget(ed, f);
}

/// `c`: the copy in a buffer of its own, drawn against the file on disk as `:diff` draws; the card stays.
pub fn compare(ed: &mut Editor) {
    let Some(What::Recover(f)) = ed.offers.first().map(|o| &o.what) else { return };
    let (path, text, base) = (f.path.clone(), f.text.clone(), f.base.clone());
    compare_close(ed, &path);
    let id = ed.open_virtual(&format!("{COMPARE}{}", shown(&path, &ed.workspace.root)), "");
    let Some(doc) = ed.docs.iter_mut().find(|d| d.id == id) else { return };
    doc.text = text.clone();
    let version = doc.version();
    if let Some(spec) = crate::syntax::detect(&path) {
        ed.load_language(id, spec);
    }
    if let Some(r) = ed.recovery.as_mut() {
        r.compares.push((path, id));
    }
    ed.events.jobs().spawn(move || {
        let (base, now) = (base.to_string(), text.to_string());
        let hunks = crate::git::hunks(&base, &now);
        move |ed: &mut Editor| ed.diff_show(id, version, base, "the file on disk".into(), hunks)
    });
}

/// `D`: the copy goes, and the conversations on its card.
pub fn discard(ed: &mut Editor, mut f: Found) {
    compare_close(ed, &f.path);
    forget(ed, &f);
    let left = std::mem::take(&mut f.chats);
    let chats = match left.is_empty() {
        true => String::new(),
        false => format!(" — {}", fresh_said(ed)),
    };
    chats_fresh(ed, &left);
    ed.set_status(format!("Discarded the unsaved edits of {}{chats}", shown(&f.path, &ed.workspace.root)));
}

/// `D` on a card of conversations alone.
pub fn discard_chats(ed: &mut Editor, left: Vec<Left>) {
    chats_fresh(ed, &left);
    ed.set_status(format!("Discarded — {}", fresh_said(ed)));
}

fn fresh_said(ed: &Editor) -> String {
    format!("the chat starts fresh, {} still lists what it left", ed.key_said("C-p"))
}

/// Their `.chat` files gone, and no session of theirs brought back goes on by itself: discard starts fresh.
fn chats_fresh(ed: &mut Editor, left: &[Left]) {
    for l in left {
        if let Some(w) = session_of(ed, &l.root).and_then(|s| ed.workspace_by_id(s)) {
            w.claude.brought_back = false;
        }
        forget_chat(ed, l);
    }
}

/// `n`: the copy stays, asked about at the next start — and the conversations on its card.
pub fn not_now(ed: &mut Editor, f: Found) {
    compare_close(ed, &f.path);
    drop(release(ed, &f.dir));
    f.chats.iter().for_each(|l| drop(release(ed, &l.dir)));
    let name = shown(&f.path, &ed.workspace.root);
    ed.set_status(format!("Kept the unsaved edits of {name} — asked again when tarae next starts"));
}

/// `n` on a card of conversations alone.
pub fn chats_not_now(ed: &mut Editor, left: Vec<Left>) {
    left.iter().for_each(|l| drop(release(ed, &l.dir)));
    ed.set_status("Kept — asked again when tarae next starts");
}

fn compare_close(ed: &mut Editor, path: &Path) {
    let Some(r) = ed.recovery.as_mut() else { return };
    let gone: Vec<DocId> = r.compares.iter().filter(|(p, _)| p == path).map(|(_, d)| *d).collect();
    r.compares.retain(|(p, _)| p != path);
    for d in gone {
        ed.close_doc(d);
    }
}

/// One card less for `dir` — its lock, when that was the last.
fn release(ed: &mut Editor, dir: &Path) -> Option<Held> {
    let r = ed.recovery.as_mut()?;
    let i = r.held.iter().position(|h| h.dir == dir)?;
    r.held[i].cards = r.held[i].cards.saturating_sub(1);
    (r.held[i].cards == 0).then(|| r.held.swap_remove(i))
}

/// The copy removed (by the keeper) — its folder too once it holds none.
fn forget(ed: &mut Editor, f: &Found) {
    forget_file(ed, &f.file, &f.dir);
}

fn forget_chat(ed: &mut Editor, l: &Left) {
    forget_file(ed, &l.file, &l.dir);
}

fn forget_file(ed: &mut Editor, file: &Path, dir: &Path) {
    let folder = release(ed, dir).map(|h| (h.dir, h.lock));
    if let Some(r) = ed.recovery.as_mut() {
        r.send(Msg::Forget { file: file.to_path_buf(), folder });
    }
}

impl Editor {
    /// Every event: tells the keeper which buffers changed since — a copy for each with unsaved edits worth
    /// keeping, none for the rest. No disk here, nothing waited for; a rope clone (O(1)) only for a buffer
    /// whose version moved. The sessions only at an event one moved (`moved`): an event touches what changed.
    pub(crate) fn recovery_sync(&mut self) {
        let Some(r) = self.recovery.as_mut() else { return };
        let moved = std::mem::take(&mut r.moved);
        let roots = std::iter::once(&self.workspace).chain(&self.workspaces).map(|s| &s.root);
        if (moved && !roots.clone().eq(r.rules.roots.iter())) || self.config.recovery_skip != r.rules.skip {
            r.rules.roots = roots.cloned().collect();
            r.rules.skip = self.config.recovery_skip.clone();
            if r.tx.is_some() {
                let rules = r.rules.clone();
                r.send(Msg::Rules(rules));
            }
        }
        let on = self.config.recovery;
        for d in &self.docs {
            let i = r.told.iter().position(|t| t.id == d.id);
            match (on && keepable(d), i) {
                (true, Some(i)) if r.told[i].is(d) => {}
                (true, i) => {
                    let Some(path) = d.path.clone() else { continue };
                    let told = Told {
                        id: d.id,
                        version: d.version(),
                        disk: d.disk,
                        conflict: d.disk_conflict,
                        path,
                    };
                    let (text, conflict) = (d.text.clone(), d.disk_conflict);
                    r.send(Msg::Keep { id: d.id, path: told.path.clone(), text, disk: d.disk, conflict });
                    match i {
                        Some(i) => r.told[i] = told,
                        None => r.told.push(told),
                    }
                }
                (false, Some(i)) => {
                    r.told.swap_remove(i);
                    r.send(Msg::Drop(d.id));
                }
                (false, None) => {}
            }
        }
        // Closed
        if r.told.len() > self.docs.len() || r.told.iter().any(|t| !self.docs.iter().any(|d| d.id == t.id)) {
            let gone: Vec<DocId> =
                r.told.iter().filter(|t| !self.docs.iter().any(|d| d.id == t.id)).map(|t| t.id).collect();
            for id in gone {
                r.told.retain(|t| t.id != id);
                r.send(Msg::Drop(id));
            }
        }
        // The conversation each session's chat is in — none while recovery is off or conversations aren't kept
        let on = on && self.chats_dir.is_some();
        let sessions = || std::iter::once(&self.workspace).chain(&self.workspaces);
        let conversation = |l: &crate::workspace::Workspace| -> Option<String> {
            l.claude.chat.as_ref()?.conversation.clone().filter(|_| on)
        };
        if !moved && on == r.chats_on {
            // (what no site said it moved fails here, never a record gone stale in silence)
            #[cfg(test)]
            for l in sessions() {
                let told = r.chats.iter().find(|t| t.0 == l.id).map(|t| (t.1.clone(), t.2.clone()));
                let now = conversation(l).map(|id| (id, l.label().to_string()));
                assert_eq!(now, told, "{}: moved with no `recovery::moved`", l.name);
            }
            return;
        }
        r.chats_on = on;
        #[cfg(test)]
        {
            r.looks += 1 + self.workspaces.len();
        }
        for l in sessions() {
            let id = conversation(l);
            let id = id.as_deref();
            let i = r.chats.iter().position(|t| t.0 == l.id);
            match (id, i) {
                (Some(id), Some(i)) if r.chats[i].1 == id && r.chats[i].2 == l.label() => {}
                (Some(id), i) => {
                    // (what the chrome calls it, every surface alike: the home session's its branch, not its folder)
                    let name = l.label().to_string();
                    let told = (l.id, id.to_string(), name.clone());
                    r.send(Msg::Chat { session: l.id, root: l.root.clone(), name, id: id.to_string() });
                    match i {
                        Some(i) => r.chats[i] = told,
                        None => r.chats.push(told),
                    }
                }
                (None, Some(i)) => {
                    r.chats.swap_remove(i);
                    r.send(Msg::ChatGone(l.id));
                }
                (None, None) => {}
            }
        }
        // Ended
        if r.chats.len() > 1 + self.workspaces.len()
            || r.chats.iter().any(|t| !sessions().any(|l| l.id == t.0))
        {
            let gone: Vec<WorkspaceId> =
                r.chats.iter().map(|t| t.0).filter(|&id| !sessions().any(|l| l.id == id)).collect();
            for id in gone {
                r.chats.retain(|t| t.0 != id);
                r.send(Msg::ChatGone(id));
            }
        }
    }

    /// The end of the process: the copies written up to the last key and kept (`keep` — a restart's exec, an
    /// error; or a signal said so: `Recovery.keep`), or removed with the folder (a quit). Waits up to
    /// `EXIT_WAIT` — the loop has ended, nothing is on screen to hold.
    pub fn recovery_end(&mut self, keep: bool) {
        self.recovery_sync();
        let Some(r) = self.recovery.as_mut() else { return };
        if keep || r.keep {
            r.ask(Msg::Flush, EXIT_WAIT);
        } else {
            r.ask(Msg::Clear, EXIT_WAIT);
        }
    }

    /// At startup: the copies gone taraes left for this project, looked for on a worker — a card each — and the
    /// conversations they were in.
    pub fn recovery_scan(&mut self) {
        let Some(r) = &self.recovery else { return };
        let (dir, own, origin) = (r.dir.clone(), r.run.clone(), r.root.clone());
        let roots: Vec<PathBuf> =
            std::iter::once(&self.workspace).chain(&self.workspaces).map(|s| s.root.clone()).collect();
        let (store, back) = (self.chats_dir.clone(), r.back.clone());
        self.events.shared_jobs().spawn(move || {
            let got = scan(&dir, &own, &origin, &roots, store.as_deref(), &back);
            move |ed: &mut Editor| ed.recovery_found(got)
        });
    }

    fn recovery_found(&mut self, Scanned { held, found, left }: Scanned) {
        let Some(r) = self.recovery.as_mut() else { return };
        #[cfg(test)]
        {
            r.scans += 1;
        }
        if found.is_empty() && left.is_empty() {
            self.unchanged = true;
            return;
        }
        for (dir, lock) in held {
            let copies = found.iter().filter(|f| f.dir == dir).count();
            let cards = copies + left.iter().filter(|l| l.dir == dir).count();
            r.held.push(Held { dir, lock, cards });
        }
        let root = self.workspace.root.clone();
        let past = self.key_said("C-p");
        // (the conversations ride on the first card, a restore bringing them back with its buffer)
        let mut left = left;
        for mut f in found {
            f.chats = std::mem::take(&mut left);
            self.push_offer(card(f, &root, &past, self.interface.zone));
        }
        if !left.is_empty() {
            self.push_offer(chats_card(left, &past));
        }
    }

    /// A big file's text is in: a restore that waited for it goes on.
    pub(crate) fn recovery_loaded(&mut self, id: DocId) {
        let Some(r) = self.recovery.as_mut() else { return };
        let Some(i) = r.after_load.iter().position(|(d, _)| *d == id) else { return };
        let (_, f) = r.after_load.swap_remove(i);
        put(self, id, &f);
    }
}

#[cfg(test)]
mod tests;
