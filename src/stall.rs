//! The freeze watcher: where the main loop is, and a thread that says so when it stays there (the polish week's
//! landing 0 — #288's 5½ minutes were found by hand with `sample`). The loop stamps each handler it begins and each
//! phase of a frame (`Stamp::begin`) with what it is — the event's kind, then the key's command or the job's site as
//! the handler learns it (`Stamp::refine`) — and goes `idle` as it waits for the next event. A label is `&'static`
//! words, a site, or a typed command's first word copied in place: nothing allocated on the key path.
//!
//! The watcher (`watch`, thread `tarae-stall`) parks while the loop is idle; the first stamp after unparks it. It
//! wakes `SLOW` after a stamp began and, if that stamp still stands, writes one log line (what, how long so far);
//! past `STACK` it takes the main thread's stack into the log's folder (macOS: `/usr/bin/sample` on tarae's own pid;
//! Linux: the call the main thread waits in, no stack) and logs where; once the loop moves on, the stall's whole time.
//! A stall past `STACK` is said on screen once a run (a calm toast pointing at the log); later ones go to
//! `:messages` only. What it decides is `Watcher::look`, pure over the stamp and a time — the thread and the
//! simulation (`sim/stall.rs`) both drive it.

use std::panic::Location;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long one handler or frame phase may stand before the log says so: fifteen frames (a batch is capped at a
/// frame's 16 ms, the key-to-frame budgets hold a frame), a pause anyone feels — and rare enough that the log stays
/// readable (the plan's 100 ms would log a start's first frame and a big file's first parse).
pub const SLOW: Duration = Duration::from_millis(250);

/// Past this a stall gets the main thread's stack and is said on screen: the plan's 2 s — a freeze, not a slow frame.
pub const STACK: Duration = Duration::from_secs(2);

/// The stacks kept in the log's folder (`stalls/`), newest first.
#[cfg(target_os = "macos")]
const STACKS_KEPT: usize = 5;

/// The bytes of a typed command's first word kept (`Label::Typed`).
const WORD: usize = 23;

/// What the loop is doing — the stamp's label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Label {
    /// Input or the terminal's: `key`, `mouse`, `paste`, `focus`, `resize`, `hover` (`DrawnFor::of`'s words).
    Event(&'static str),
    /// A key's command (`Editor::run`).
    Command(&'static str),
    /// A typed command, its first word (`typed::execute`).
    Typed(Word),
    /// A job's result, by where it was sent from.
    Job(&'static Location<'static>),
    /// A job that names itself (`Editor::drawn_for`), or a step of the loop's own.
    Named(&'static str),
    /// A frame's phase (`term::event_loop`).
    Frame(&'static str),
}

/// A typed command's first word, copied in place (cut on a character's boundary at `WORD` bytes).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Word {
    bytes: [u8; WORD],
    len: u8,
}

impl Word {
    pub fn of(s: &str) -> Word {
        let mut len = s.len().min(WORD);
        while !s.is_char_boundary(len) {
            len -= 1;
        }
        let mut bytes = [0; WORD];
        bytes[..len].copy_from_slice(&s.as_bytes()[..len]);
        Word { bytes, len: len as u8 }
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

impl std::fmt::Debug for Word {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl std::fmt::Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Label::Event(what) | Label::Named(what) => f.write_str(what),
            Label::Command(name) => write!(f, "command `{name}`"),
            Label::Typed(word) => write!(f, "`:{}`", word.as_str()),
            Label::Job(at) => write!(f, "job {}:{}", at.file(), at.line()),
            Label::Frame(phase) => write!(f, "frame: {phase}"),
        }
    }
}

/// The watcher's state (`Stamp.waiting`).
const RUNNING: u8 = 0;
/// Timing a stamp: woken at its next limit, or by `idle`.
const TIMED: u8 = 1;
/// Nothing to time (the loop idle, or a stall past every limit): woken by the next stamp.
const PARKED: u8 = 2;

/// Where the main loop is — one an editor (`Editor.stall`), shared with its watcher.
pub struct Stamp {
    /// Stamps begun, twice, plus one while one stands: a stamp's own number is odd, the idle after it the next.
    seq: AtomicU64,
    now: Mutex<Now>,
    waiting: AtomicU8,
    watcher: OnceLock<std::thread::Thread>,
    stop: AtomicBool,
    /// Stacks taken (tests wait on it).
    stacks: AtomicU32,
}

struct Now {
    since: Option<Instant>,
    label: Label,
    /// The stamp the watcher said is stalling — its end is timed exactly (`ended`).
    watched: u64,
    /// That stamp's number and how long it stood, once it's over.
    ended: Option<(u64, Duration)>,
}

/// What the watcher read of the stamp.
#[derive(Clone, Copy, Debug)]
pub struct Seen {
    pub seq: u64,
    pub since: Option<Instant>,
    pub label: Label,
    pub ended: Option<(u64, Duration)>,
}

impl Default for Stamp {
    fn default() -> Self {
        let now = Now { since: None, label: Label::Named("the start"), watched: 0, ended: None };
        Stamp {
            seq: AtomicU64::new(0),
            now: Mutex::new(now),
            waiting: AtomicU8::new(RUNNING),
            watcher: OnceLock::new(),
            stop: AtomicBool::new(false),
            stacks: AtomicU32::new(0),
        }
    }
}

impl Stamp {
    fn lock(&self) -> std::sync::MutexGuard<'_, Now> {
        self.now.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The loop begins `label`: the stamp before it, if one stood, is over.
    pub fn begin(&self, label: Label) {
        let at = crate::clock::now();
        let mut n = self.lock();
        let was = self.seq.load(SeqCst);
        let ended = n.ended(was, at);
        (n.since, n.label) = (Some(at), label);
        self.seq.store((was / 2 + 1) * 2 + 1, SeqCst);
        drop(n);
        // (from idle the watcher is parked; a stall it watched is over)
        let waiting = self.waiting.load(SeqCst);
        if waiting == PARKED || (ended && waiting != RUNNING) {
            self.wake();
        }
    }

    /// What the stamp under way is doing, now that its handler knows (a key's command, a job's site) — the same
    /// stamp, its time running on.
    pub fn refine(&self, label: Label) {
        let mut n = self.lock();
        if self.seq.load(SeqCst) & 1 == 1 {
            n.label = label;
        }
    }

    /// The loop waits for the next event: the stamp under way is over.
    pub fn idle(&self) {
        let at = crate::clock::now();
        let mut n = self.lock();
        let was = self.seq.load(SeqCst);
        if was & 1 == 0 {
            return;
        }
        n.ended(was, at);
        self.seq.store(was + 1, SeqCst);
        drop(n);
        // (a watcher timing it needn't wake later, with the loop idle by then: it parks now)
        if self.waiting.load(SeqCst) != RUNNING {
            self.wake();
        }
    }

    fn wake(&self) {
        if let Some(t) = self.watcher.get() {
            t.unpark();
        }
    }

    pub fn seen(&self) -> Seen {
        let n = self.lock();
        Seen { seq: self.seq.load(SeqCst), since: n.since, label: n.label, ended: n.ended }
    }

    /// The watcher said stamp `seq` is stalling: its end is timed.
    fn watch(&self, seq: u64) {
        let mut n = self.lock();
        (n.watched, n.ended) = (seq, None);
    }

    /// Stacks taken so far.
    #[cfg(test)]
    pub fn stacks(&self) -> u32 {
        self.stacks.load(SeqCst)
    }

    /// Its watcher ends (tests).
    #[cfg(test)]
    pub fn stop(&self) {
        self.stop.store(true, SeqCst);
        self.wake();
    }
}

impl Now {
    /// Stamp `was` ends at `at`: timed, if it's the one watched — whether it was.
    fn ended(&mut self, was: u64, at: Instant) -> bool {
        let watched = was & 1 == 1 && was == self.watched;
        if watched && let Some(since) = self.since {
            self.ended = Some((was, at.saturating_duration_since(since)));
        }
        watched
    }
}

/// The limits a watcher goes by (`SLOW`, `STACK`; tests' own smaller).
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub slow: Duration,
    pub stack: Duration,
}

pub const LIMITS: Limits = Limits { slow: SLOW, stack: STACK };

/// What a look found to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// A stamp has stood past `slow` — logged once.
    Slow { label: Label, so_far: Duration },
    /// Past `stack` — the main thread's stack taken, once.
    Stack { label: Label, so_far: Duration },
    /// A stall said is over, its whole time (`label`: its last).
    Over { label: Label, took: Duration, tell: Tell },
}

/// How an end is said on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tell {
    /// Under `stack`: the log only.
    No,
    /// The run's first past `stack`: a calm toast.
    Toast,
    /// Later ones: `:messages` only.
    Note,
}

/// When the watcher looks again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    /// When the stamp moves (the next stamp, or the loop going idle).
    Park,
    /// At this time, or when the stamp moves before it.
    At(Instant),
}

/// One look's findings.
#[derive(Debug)]
pub struct Look {
    pub acts: Vec<Act>,
    pub wake: Wake,
    /// The stamp it read: a wait is skipped if it moved since.
    pub seq: u64,
}

/// The stall a watcher follows.
struct Stall {
    seq: u64,
    since: Instant,
    label: Label,
    said: bool,
    stacked: bool,
}

/// What a watcher knows between looks.
pub struct Watcher {
    limits: Limits,
    stall: Option<Stall>,
    /// A stall was said on screen this run.
    told: bool,
}

impl Watcher {
    pub fn new(limits: Limits) -> Watcher {
        Watcher { limits, stall: None, told: false }
    }

    /// What the stamp says at `at`: a stall's lines due, and when to look again.
    pub fn look(&mut self, stamp: &Stamp, at: Instant) -> Look {
        let seen = stamp.seen();
        let mut acts = Vec::new();
        if let Some(s) = self.stall.take_if(|s| s.seq != seen.seq)
            && s.said
        {
            // (timed by the stamp that ended it; else — it ended while a look was being made — by now)
            let took = (seen.ended.filter(|(q, _)| *q == s.seq).map(|(_, d)| d))
                .unwrap_or_else(|| at.saturating_duration_since(s.since));
            let tell = match took >= self.limits.stack {
                false => Tell::No,
                true if std::mem::replace(&mut self.told, true) => Tell::Note,
                true => Tell::Toast,
            };
            acts.push(Act::Over { label: s.label, took, tell });
        }
        let (seq, Some(since)) = (seen.seq, seen.since) else {
            return Look { acts, wake: Wake::Park, seq: seen.seq };
        };
        if seq & 1 == 0 {
            return Look { acts, wake: Wake::Park, seq };
        }
        let s =
            self.stall.get_or_insert(Stall { seq, since, label: seen.label, said: false, stacked: false });
        s.label = seen.label;
        let so_far = at.saturating_duration_since(since);
        if !s.said && so_far >= self.limits.slow {
            s.said = true;
            stamp.watch(seq);
            acts.push(Act::Slow { label: s.label, so_far });
        }
        if s.said && !s.stacked && so_far >= self.limits.stack {
            s.stacked = true;
            acts.push(Act::Stack { label: s.label, so_far });
        }
        let wake = match (s.said, s.stacked) {
            (false, _) => Wake::At(since + self.limits.slow),
            (true, false) => Wake::At(since + self.limits.stack),
            (true, true) => Wake::Park,
        };
        Look { acts, wake, seq }
    }
}

/// The pid of a stack being taken (`:procs` names it).
static SAMPLING: AtomicU32 = AtomicU32::new(0);

/// The process taking the main thread's stack, while it runs.
pub fn sampling() -> Option<u32> {
    Some(SAMPLING.load(SeqCst)).filter(|&p| p > 0)
}

/// Starts the watcher of `stamp` (the main loop's — `term::event_loop`); what it says on screen goes through `tx`.
pub fn watch(stamp: Arc<Stamp>, tx: crate::event::Tx, limits: Limits) {
    crate::event::thread("tarae-stall", move || {
        let _ = stamp.watcher.set(std::thread::current());
        let mut w = Watcher::new(limits);
        while !stamp.stop.load(SeqCst) {
            let look = w.look(&stamp, crate::clock::now());
            for &act in &look.acts {
                said(act, &stamp, &tx);
            }
            wait(&stamp, &look);
        }
    });
}

/// Parks until the next look (`look.wake`) — not at all if the stamp moved since it was read: whoever moved it saw
/// `waiting` as it was before, or it's set first and they wake it (the two stores, then the two loads, `SeqCst`).
fn wait(stamp: &Stamp, look: &Look) {
    stamp.waiting.store(if look.wake == Wake::Park { PARKED } else { TIMED }, SeqCst);
    if stamp.seq.load(SeqCst) == look.seq && !stamp.stop.load(SeqCst) {
        match look.wake {
            Wake::Park => std::thread::park(),
            Wake::At(t) => std::thread::park_timeout(t.saturating_duration_since(crate::clock::now())),
        }
    }
    stamp.waiting.store(RUNNING, SeqCst);
}

/// An act done: its log line, the stack, the end said where it's due.
fn said(act: Act, stamp: &Stamp, tx: &crate::event::Tx) {
    let time = crate::term::run_duration;
    match act {
        Act::Slow { label, so_far } => {
            crate::log::line("stall", "warn", &format!("the main loop has stood {} on {label}", time(so_far)))
        }
        Act::Stack { label, so_far } => {
            let stack = stack();
            stamp.stacks.fetch_add(1, SeqCst);
            crate::log::line("stall", "warn", &format!("{} into {label}: {stack}", time(so_far)));
        }
        Act::Over { label, took, tell } => {
            crate::log::line(
                "stall",
                "warn",
                &format!("the main loop is back after {} on {label}", time(took)),
            );
            if tell == Tell::No {
                return;
            }
            let msg = format!("tarae stood still {} on {label} — the log says where (:log-open)", time(took));
            let job = move |ed: &mut crate::editor::Editor| match tell {
                Tell::Toast => ed.set_status(msg),
                _ => ed.note(msg),
            };
            let _ = tx.send(crate::event::Event::Job(Box::new(job)));
        }
    }
}

/// The main thread's stack, or what can be said of it — where it was written. Tests take none.
fn stack() -> String {
    if cfg!(test) {
        return "no stack in tests".into();
    }
    stack_here()
}

/// macOS: `/usr/bin/sample` on our own pid — 1 s at 10 ms (100 samples, each a moment's suspension of tarae's
/// threads; the main one is stuck anyway), written beside the log; the release is stripped, so its frames are
/// addresses until `atos` reads them with the dSYM `:restart` kept (`dsyms.rs`).
#[cfg(target_os = "macos")]
fn stack_here() -> String {
    use std::io::Read;
    let Some(dir) = crate::config::state_dir().map(|d| d.join("stalls")) else {
        return "no stack: no state folder".into();
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return format!("no stack: {}: {e}", dir.display());
    }
    let secs = crate::clock::system().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let pid = std::process::id();
    let path = dir.join(format!("{secs}-{pid}.txt"));
    let mut cmd = std::process::Command::new("/usr/bin/sample");
    cmd.args([&pid.to_string(), "1", "10", "-file"]).arg(&path);
    let mut s = match crate::spawn::setsid(&cmd) {
        Ok(s) => s,
        Err(e) => return format!("no stack: /usr/bin/sample: {e}"),
    };
    SAMPLING.store(s.pid, SeqCst);
    // (read to their ends — a pipe left unread would stop it; what it says is its progress)
    let mut sink = Vec::new();
    for r in [s.stdout.as_mut(), s.stderr.as_mut()].into_iter().flatten() {
        let _ = r.read_to_end(&mut sink);
    }
    let ended = crate::spawn::wait(s.pid);
    SAMPLING.store(0, SeqCst);
    prune(&dir);
    match ended {
        Ok(st) if st.success() => format!("the main thread's stack in {}", path.display()),
        Ok(st) => format!("no stack: /usr/bin/sample {st}"),
        Err(e) => format!("no stack: /usr/bin/sample: {e}"),
    }
}

/// Linux: no stack (no `sample`; reading another thread's needs ptrace) — the call the main thread waits in, from
/// its `/proc` files.
#[cfg(target_os = "linux")]
fn stack_here() -> String {
    let task = format!("/proc/self/task/{}", std::process::id());
    let read = |f: &str| std::fs::read_to_string(format!("{task}/{f}")).map(|s| s.trim().to_string());
    match (read("wchan"), read("syscall")) {
        (Ok(w), Ok(c)) => format!("no stack on Linux; the main thread waits in `{w}` (syscall {c})"),
        _ => "no stack on Linux".into(),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn stack_here() -> String {
    "no stack here".into()
}

/// The newest `STACKS_KEPT` stacks stay (named by their time, so by name).
#[cfg(target_os = "macos")]
fn prune(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut names: Vec<std::ffi::OsString> = (entries.flatten())
        .map(|e| e.file_name())
        .filter(|n| n.to_str().is_some_and(|n| n.ends_with(".txt")))
        .collect();
    crate::sorting::sort(&mut names);
    let old = names.len().saturating_sub(STACKS_KEPT);
    for n in &names[..old] {
        let _ = std::fs::remove_file(dir.join(n));
    }
}

#[cfg(test)]
mod tests;
