//! A crash said and held (#470): a panic while a pane was laid out or drawn — the release build aborts on one — ended
//! tarae at every start once, as the start brought that session back in front and drew the same pane again.
//!
//! Each surface's layout and drawing says which it is while it runs (`drawing`: the front session's checkout and name,
//! the chat or a code pane's file — a thread-local the hook reads). The panic hook (`begin`) writes a marker into tarae's
//! state, `crashed/<the start root's>.json` (`config::state_file`): where, the message's head (`cut` — never the text it
//! sliced), that session and pane, the build. Any thread's panic writes one when it ends tarae (`panic = "abort"`); in a
//! build that unwinds, only one in a drawing (the main thread's).
//!
//! The next start in that folder reads it beside the restore file, before the first frame (`take`), deletes it, says it
//! (`said`) and holds that pane (`Hold.held`, matched by the session's checkout — sessions come back on a worker): none
//! of its last content laid out or drawn, its row in its place (`HELD`; `SAME` under it) until `enter` on it or a press on it (`show`).
//! Shown and crashing again, the next start says so (`Drawing.again`). Still held at a quit or `:restart`, the hold is
//! written back to the marker (`keep`, `Crashed.held`): the next start holds it again, said quietly.
//!
//! A panic that ends tarae (#541, #542) goes through one hook, in a fixed order (`hook`): only the first panic does
//! the work (`ENDING`; a second one waits for its end); what it needs is read on the panicking thread (where, the
//! message, the frames); the terminal is given back (`terminal`, term.rs's `restore`); then a helper thread, waited
//! for `END_WAIT` at most, asks the recovery keeper for what waits (`recovery::crash_flush`), writes the report —
//! frames as addresses first (`report.rs`) — and the marker; three lines of tarae's own go to stderr (`lines`), and
//! the frames are symbolized in what's left of the wait, the report replaced if that ends in time (else the next
//! start does it). The release build prints no default message. From `ENDING` on, the main loop applies no event
//! and draws nothing, and the input thread stops: what's typed at what looks like a shell never reaches the editor.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::editor::Editor;

/// The folder of the markers in tarae's state.
const DIR: &str = "crashed";
/// A panic's message kept at most this many characters (a toast can land in a recording).
const MESSAGE: usize = 120;
/// How long a panic's hook waits at most — the keeper's copies, the report, its frames symbolized — before tarae ends.
pub const END_WAIT: Duration = Duration::from_secs(2);
/// …of which the keeper's flush at most: the report is written after it, within the rest.
const FLUSH_WAIT: Duration = Duration::from_millis(1500);
/// `tarae --crash-test`'s panic: its message, and what a start says of a crash that was one (never taken for a real one).
pub const ON_PURPOSE: &str = "crash-test: a panic on purpose";
/// …and how a start's words begin for it, where a real crash's say `tarae crashed`.
const ON_PURPOSE_SAID: &str = "tarae crash-test, a panic on purpose";
/// A held pane's row.
pub const HELD: &str = "held after a crash";
/// …and the row under it, a warning, when this is the build that crashed.
pub const SAME: &str = "same build — may crash again";

/// Which surface of a session: the chat (its transcript), or a code pane on a file (its path; none for a scratch).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pane {
    Chat,
    Code(PathBuf),
}

/// What a thread lays out or draws now: the session (its checkout, its name) and its surface; `again`, a pane a crash
/// held that the user's key showed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drawing {
    pub root: PathBuf,
    pub session: String,
    pub pane: Pane,
    pub again: bool,
}

thread_local!(static DRAWING: RefCell<Option<Drawing>> = const { RefCell::new(None) });

/// A drawing said while it lives (`drawing`); the one before it back at its end.
pub struct Note(Option<Drawing>);

impl Drop for Note {
    fn drop(&mut self) {
        let was = self.0.take();
        DRAWING.with(|d| *d.borrow_mut() = was);
    }
}

/// The front session's `pane` is laid out or drawn until the note returned ends.
pub fn drawing(ed: &Editor, pane: Pane) -> Note {
    let w = &ed.workspace;
    let again = ed.crash.shown.as_ref().is_some_and(|h| h.root == w.root && h.pane == pane);
    let d = Drawing { root: w.root.clone(), session: w.label().to_string(), pane, again };
    Note(DRAWING.with(|cell| cell.replace(Some(d))))
}

/// What a crash left: where (`file:line`), the message's head, the thread, the build, and the pane being drawn; `held`,
/// a hold carried over a quit or `:restart` (`keep`) — no new crash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crashed {
    pub at: String,
    pub message: String,
    pub thread: String,
    pub build: String,
    pub drawing: Option<Drawing>,
    pub held: bool,
    /// The report this crash left (`report.rs`), when one was written.
    pub report: Option<PathBuf>,
}

/// A pane held since the start: the session's checkout, its name, its surface, and where and in which build the crash
/// that held it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub root: PathBuf,
    pub session: String,
    pub pane: Pane,
    pub at: String,
    pub build: String,
}

impl Held {
    /// The build that crashed drawing it is this one.
    pub fn same_build(&self) -> bool {
        self.build == tarae_wire::BUILD
    }
}

/// The editor's: the pane held, and the one shown since (a crash drawing it is said as `again`).
#[derive(Debug, Default)]
pub struct Hold {
    pub held: Option<Held>,
    pub shown: Option<Held>,
}

/// The marker of a tarae started in `root`, in state folder `state`.
pub fn file(state: &Path, root: &Path) -> PathBuf {
    crate::config::state_file(&state.join(DIR), root)
}

/// What a crash knows of the tarae it ends, set at the start (`begin`): its marker and its reports' folder (none
/// without a state folder), where it started (the copies are offered there again), home (said as `~`).
pub struct Ends {
    pub marker: Option<PathBuf>,
    pub reports: Option<PathBuf>,
    pub root: PathBuf,
    pub home: Option<PathBuf>,
}

static ENDS: OnceLock<Ends> = OnceLock::new();
/// The terminal given back (term.rs's `restore`), registered by the link that took it (`terminal`).
static TERMINAL: OnceLock<fn() -> std::io::Result<()>> = OnceLock::new();
static HOOK: std::sync::Once = std::sync::Once::new();
/// A panic is ending tarae: the first one's hook runs; the main loop applies no event, the input thread stops.
static ENDING: AtomicBool = AtomicBool::new(false);
/// The running build's symbols kept (`dsyms::keep` at the start): where a report's frames are symbolized from.
static SYMBOLS: OnceLock<PathBuf> = OnceLock::new();

/// The reports' folder in state folder `state`.
pub fn reports(state: &Path) -> PathBuf {
    state.join(DIR).join(report::DIR)
}

/// At the start, before the first frame: a panic from now on that ends tarae leaves its marker at `file(state, root)`
/// and a report in `reports(state)` (none without a state folder), says itself in three lines, and has the keeper
/// write what waits.
pub fn begin(state: Option<&Path>, root: PathBuf) {
    let ends = Ends {
        marker: state.map(|s| file(s, &root)),
        reports: state.map(reports),
        root,
        home: crate::allow::home(),
    };
    let _ = ENDS.set(ends);
    install();
}

/// The terminal was taken (raw mode, the alternate screen): a panic gives it back with `restore` before anything else
/// — this process's editor's, or an attached client's.
pub fn terminal(restore: fn() -> std::io::Result<()>) {
    let _ = TERMINAL.set(restore);
    install();
}

/// The running build's symbols are kept at `at` (`dsyms::keep`, the start's worker) — only when they're this image's.
pub fn symbols_kept(at: PathBuf) {
    let _ = SYMBOLS.set(at);
}

/// A panic is ending tarae: nothing more is applied or drawn (`hook`).
pub fn ending() -> bool {
    ENDING.load(Ordering::SeqCst)
}

/// At the main loop's end: a panic ending tarae waits here for its hook's end — never the quit's way out, which
/// removes the copies of unsaved buffers.
pub fn hold_if_ending() {
    if ending() {
        loop {
            std::thread::park();
        }
    }
}

fn install() {
    HOOK.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| hook(info, &prev)));
    });
}

/// The release build's way: tarae ends with this panic, no default message after tarae's lines.
fn aborts() -> bool {
    #[cfg(test)]
    if AS_ABORT.load(Ordering::SeqCst) {
        return true;
    }
    cfg!(panic = "abort")
}

/// Tests (a child process of their own): this process goes as the release build does — no default message after the
/// lines, a second panic waits for the first's end.
#[cfg(test)]
pub static AS_ABORT: AtomicBool = AtomicBool::new(false);
/// Every panic here ends tarae (in a build that unwinds, only a drawing's does otherwise): `tarae --crash-test`'s, and
/// tests' children.
pub static ENDS_TARAE: AtomicBool = AtomicBool::new(false);
/// Tests: what the last ending's hook came to.
#[cfg(test)]
pub static WENT: std::sync::Mutex<Option<Went>> = std::sync::Mutex::new(None);
/// Tests: second panics that waited for the first's end.
#[cfg(test)]
pub static WAITED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Tests: the abort's stand-in — the process exits where the release build aborts, at the end of the first panic's hook
/// (a second panic parked meanwhile ends with it, as in release).
#[cfg(test)]
pub static EXITS: AtomicBool = AtomicBool::new(false);
/// Tests: run inside the first panic's hook, once it's the one ending tarae — where a second panic comes meanwhile.
#[cfg(test)]
pub static DURING: OnceLock<fn()> = OnceLock::new();

/// What an ending's hook came to: the keeper's answer, whether the report was written and symbolized in time, the
/// lines said.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Went {
    pub flushed: crate::recovery::Flushed,
    pub written: bool,
    pub symbolized: bool,
    pub lines: Vec<String>,
}

/// The one panic hook, in its order (the module's doc). Nothing here takes a lock the panicking thread may hold:
/// stdout and stderr's are reentrant, `DRAWING` is only tried, the keeper's sender is read from a `OnceLock`, the log
/// isn't written; every wait is a channel's, bounded by `END_WAIT`.
fn hook(info: &std::panic::PanicHookInfo, prev: &(dyn Fn(&std::panic::PanicHookInfo) + Send + Sync)) {
    let drawing = DRAWING.try_with(|d| d.try_borrow().ok().and_then(|d| d.clone())).ok().flatten();
    let marked = ENDS_TARAE.load(Ordering::SeqCst);
    let ends = ENDS.get().filter(|_| aborts() || drawing.is_some() || marked);
    let Some(ends) = ends else {
        give_back_terminal();
        prev(info);
        return;
    };
    if ENDING.swap(true, Ordering::SeqCst) {
        // (a second panic while the first ends tarae: the first's abort ends this one too)
        if aborts() {
            #[cfg(test)]
            WAITED.fetch_add(1, Ordering::SeqCst);
            loop {
                std::thread::park();
            }
        }
        return prev(info);
    }
    #[cfg(test)]
    if let Some(during) = DURING.get() {
        during();
    }
    let start = crate::clock::now();
    let payload = info.payload();
    let message = (payload.downcast_ref::<&str>().copied())
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or_default();
    let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
    let keeper_itself = thread == crate::recovery::THREAD;
    let c = Crashed {
        at: info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default(),
        message: crate::tokens::redacted(&cut(message)),
        thread,
        build: tarae_wire::BUILD.to_string(),
        drawing,
        held: false,
        report: None,
    };
    let frames = report::addresses();
    let backtrace = std::backtrace::Backtrace::force_capture();
    give_back_terminal();
    let r = report::Report {
        crashed: c.clone(),
        when: crate::clock::system(),
        pid: std::process::id(),
        running: crate::dsyms::running(),
        symbols: SYMBOLS.get().cloned(),
        root: ends.root.clone(),
        unsaved: crate::recovery::Flushed::Nothing,
        frames: report::Frames::Addresses(frames),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    // (taken by the helper — or, no thread to be had, by this one: the same here, its flush's wait still bounded)
    let job = std::sync::Arc::new(std::sync::Mutex::new(Some((r, backtrace, tx))));
    let theirs = job.clone();
    let helper = std::thread::Builder::new().name("tarae-crash".into()).spawn(move || {
        let taken = theirs.lock().ok().and_then(|mut j| j.take());
        if let Some((r, backtrace, tx)) = taken {
            ended(ends, r, backtrace, keeper_itself, &tx);
        }
    });
    if helper.is_err()
        && let Some((r, backtrace, tx)) = job.lock().ok().and_then(|mut j| j.take())
    {
        ended(ends, r, backtrace, keeper_itself, &tx);
    }
    let left = || END_WAIT.saturating_sub(crate::clock::since(start));
    let written = match rx.recv_timeout(left()) {
        Ok(Step::Written(flushed, report)) => Some((flushed, report)),
        _ => None,
    };
    let (flushed, report) = match written.clone() {
        Some(w) => w,
        None => {
            let f = match crate::recovery::keeping() {
                true => crate::recovery::Flushed::Unanswered,
                false => crate::recovery::Flushed::Nothing,
            };
            let report = match ends.reports {
                Some(_) => Written::Late,
                None => Written::NoState,
            };
            (f, report)
        }
    };
    let said = lines(&c.at, &c.build, flushed, &ends.root, &report, ends.home.as_deref());
    {
        use std::io::Write as _;
        let mut err = std::io::stderr().lock();
        for l in &said {
            let _ = writeln!(err, "{l}");
        }
        let _ = err.flush();
    }
    let symbolized = written.is_some() && matches!(rx.recv_timeout(left()), Ok(Step::Symbolized(true)));
    #[cfg(test)]
    {
        let went = Went { flushed, written: written.is_some(), symbolized, lines: said };
        *WENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(went);
    }
    #[cfg(not(test))]
    let _ = (symbolized, said);
    #[cfg(test)]
    if EXITS.load(Ordering::SeqCst) {
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }
    if !aborts() {
        prev(info);
    }
}

/// `tarae --crash-test`: a panic on purpose, after the start, as one ends tarae — every panic in a build that unwinds
/// too (a debug build's). Never returns.
pub fn on_purpose(ed: &mut Editor) -> ! {
    // (the start's workers — the symbols kept — have a moment, as a real crash comes after them)
    let end = crate::clock::now() + Duration::from_secs(3);
    while SYMBOLS.get().is_none() && crate::clock::now() < end {
        if let Ok(Some(ev)) = ed.events.recv_within(Duration::from_millis(50)) {
            ed.handle_event(ev);
        }
    }
    ENDS_TARAE.store(true, Ordering::SeqCst);
    panic!("{ON_PURPOSE}");
}

/// The terminal given back, when one was taken.
fn give_back_terminal() {
    if let Some(restore) = TERMINAL.get() {
        let _ = restore();
    }
}

/// A step of the helper's, as the hook hears it.
enum Step {
    /// The keeper's answer, and the report as written — frames as addresses.
    Written(crate::recovery::Flushed, Written),
    /// Its frames symbolized in time, the report replaced — or not (nothing came of it).
    Symbolized(bool),
}

/// What became of a crash's report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Written {
    At(PathBuf),
    Failed(String),
    /// Not written within `END_WAIT`.
    Late,
    /// No state folder to write it in.
    NoState,
}

/// The helper's (the hook's thread waits for it, `END_WAIT` at most): the keeper's flush, the report with its frames as
/// addresses, the marker — said to the hook — then the frames symbolized and the report replaced.
fn ended(
    ends: &Ends,
    mut r: report::Report,
    backtrace: std::backtrace::Backtrace,
    keeper_itself: bool,
    tx: &std::sync::mpsc::Sender<Step>,
) {
    r.unsaved = crate::recovery::crash_flush(FLUSH_WAIT, keeper_itself);
    let written = match &ends.reports {
        None => Written::NoState,
        Some(dir) => match report::write_new(dir, &r, ends.home.as_deref()) {
            Ok(path) => Written::At(path),
            Err(e) => Written::Failed(e.to_string()),
        },
    };
    if let Some(marker) = &ends.marker {
        let report = match &written {
            Written::At(p) => Some(p.clone()),
            _ => None,
        };
        let _ = write(marker, &Crashed { report, ..r.crashed.clone() });
    }
    let _ = tx.send(Step::Written(r.unsaved, written.clone()));
    let Written::At(path) = written else { return };
    let done = report::symbolize_now(&path, &mut r, &backtrace, ends.home.as_deref());
    let _ = tx.send(Step::Symbolized(done));
}

/// The three lines a panic says on stderr once the terminal is back: where and which build; whether unsaved changes
/// are kept and where to start tarae for them (left out when none were); where the report is.
pub fn lines(
    at: &str,
    build: &str,
    flushed: crate::recovery::Flushed,
    root: &Path,
    report: &Written,
    home: Option<&Path>,
) -> Vec<String> {
    use crate::recovery::Flushed;
    let mut out = vec![format!("tarae crashed · {at} · build {}", build_said(build))];
    let start = format!("start tarae in {} and it offers them", tilde(root, home));
    match flushed {
        Flushed::Nothing => {}
        Flushed::Kept(n) => {
            let files = if n == 1 { "1 file".to_string() } else { format!("{n} files") };
            out.push(format!("unsaved changes in {files} are kept — {start}"));
        }
        Flushed::Unanswered | Flushed::KeeperItself => {
            let most = crate::recovery::MOST.as_secs();
            out.push(format!("unsaved changes are kept as they were up to {most} s before — {start}"));
        }
    }
    out.push(match report {
        Written::At(p) => format!("report: {}", tilde(p, home)),
        Written::Failed(e) => format!("report not written: {e}"),
        Written::Late => format!("report not written in {} s", END_WAIT.as_secs()),
        Written::NoState => "no report (no state folder)".to_string(),
    });
    out
}

/// `path` with home as `~`.
pub fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// A panic's message as kept: its first line up to its first backtick — core quotes what it sliced in them, in each of
/// its slice panics' forms ("…out of bounds of `<the text>`", "…(bytes 168..171) of `<the text>`", "begin <= end
/// (4 <= 2) when slicing `<the text>`") —, at most `MESSAGE` characters.
pub fn cut(message: &str) -> String {
    let head = message.split('`').next().unwrap_or_default();
    let head = head.lines().next().unwrap_or_default().trim_end();
    match head.char_indices().nth(MESSAGE) {
        Some((at, _)) => format!("{}…", &head[..at]),
        None => head.to_string(),
    }
}

fn write(path: &Path, c: &Crashed) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let d = c.drawing.as_ref();
    let v = json!({
        "at": c.at,
        "message": c.message,
        "thread": c.thread,
        "build": c.build,
        "root": d.map(|d| d.root.to_string_lossy()),
        "session": d.map(|d| d.session.as_str()),
        "pane": d.map(|d| match &d.pane { Pane::Chat => "chat", Pane::Code(_) => "code" }),
        "file": d.and_then(|d| match &d.pane { Pane::Code(p) => Some(p.to_string_lossy()), Pane::Chat => None }),
        "again": d.is_some_and(|d| d.again),
        "held": c.held,
        "report": c.report.as_ref().map(|p| p.to_string_lossy()),
    });
    crate::disk::write_private(path, |w| w.write_all(v.to_string().as_bytes()))
}

fn read(text: &str) -> Option<Crashed> {
    let v: Value = serde_json::from_str(text).ok()?;
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    let pane = match v["pane"].as_str() {
        Some("chat") => Some(Pane::Chat),
        Some("code") => Some(Pane::Code(PathBuf::from(s("file")))),
        _ => None,
    };
    let drawing = pane.map(|pane| Drawing {
        root: PathBuf::from(s("root")),
        session: s("session"),
        pane,
        again: v["again"].as_bool().unwrap_or(false),
    });
    let held = v["held"].as_bool().unwrap_or(false);
    Some(Crashed {
        at: s("at"),
        message: s("message"),
        thread: s("thread"),
        build: s("build"),
        drawing,
        held,
        report: v["report"].as_str().map(PathBuf::from),
    })
}

/// The marker `marker` (`file`) a crash left, read and deleted — a small file, read at the start before the first
/// frame as the restore file is.
pub fn take(marker: &Path) -> Option<Crashed> {
    let text = std::fs::read_to_string(marker).ok()?;
    let _ = std::fs::remove_file(marker);
    read(&text)
}

/// The build as said: its commit (`0.1.0 32018b82` → `32018b82`), else the version alone.
fn build_said(build: &str) -> &str {
    build.rsplit(' ').next().unwrap_or(build)
}

/// The start's words for `c`: what happened first, where next, its report last (`· file:line, build … · report …` —
/// what a narrow toast cuts first).
pub fn said(c: &Crashed) -> String {
    said_home(c, crate::allow::home().as_deref())
}

/// `said`, home said as `~`.
pub fn said_home(c: &Crashed, home: Option<&Path>) -> String {
    let mut place = format!("{}, build {}", c.at, build_said(&c.build));
    if let Some(r) = &c.report {
        place += &format!(" · report {}", tilde(r, home));
    }
    match &c.drawing {
        Some(d) => {
            let what = pane_said(d);
            if c.held {
                return format!("{what} still held after a crash · {place}");
            }
            let verb = if d.again { "crashed again showing" } else { "crashed drawing" };
            format!("tarae {verb} {what} — it opens held · {place}")
        }
        None if c.message == ON_PURPOSE => format!("{ON_PURPOSE_SAID} · {place}"),
        None => format!("tarae crashed · {place}"),
    }
}

/// A start's words for a report's crash, by its message: `tarae crash-test, a panic on purpose` when it was one, else
/// `tarae crashed` (`report::words`).
pub fn crashed_said(message: &str) -> &'static str {
    if message == ON_PURPOSE { ON_PURPOSE_SAID } else { "tarae crashed" }
}

/// A drawing's pane as it's said — the start's words (`said`) and a report's `drawing` field: `panes4's chat`,
/// `docs' src/a.rs` (`workspace::possessive`).
pub fn pane_said(d: &Drawing) -> String {
    let whose = crate::workspace::possessive(&d.session);
    match &d.pane {
        Pane::Chat => format!("{whose} chat"),
        Pane::Code(p) => format!("{whose} {}", p.strip_prefix(&d.root).unwrap_or(p).display()),
    }
}

/// The start, a crash's marker read: said at once — the whole of it in the log, the message too —, its pane held; a
/// hold carried over (`keep`), held again and said quietly.
pub fn start(ed: &mut Editor, c: Crashed) {
    let words = said(&c);
    let logged = match c.held {
        true => words.clone(),
        false => format!("{words} — {} (thread {})", c.message, c.thread),
    };
    said_at_start(ed, words, &logged, c.held);
    let (at, build) = (c.at, c.build);
    ed.crash.held = c.drawing.map(|d| Held { root: d.root, session: d.session, pane: d.pane, at, build });
}

/// A crash's words at the start — the folder's marker's (`start`) or a report another start left (`report::start_job`):
/// in the log (`logged`, the whole of it), and a warning — a hold carried over (`quiet`), a status.
pub fn said_at_start(ed: &mut Editor, words: String, logged: &str, quiet: bool) {
    match quiet {
        true => {
            crate::log::line("tarae", "info", logged);
            ed.set_status(words);
        }
        false => {
            crate::log::line("tarae", "error", logged);
            ed.set_warning(words);
        }
    }
}

/// At a quit or `:restart`'s exec: a hold still standing written back to the start's marker (`begin`), so the next
/// start holds it again — without it, the same build would crash drawing it again. A crash after this writes its own.
pub fn keep(ed: &Editor) {
    if let Some(path) = ENDS.get().and_then(|e| e.marker.as_ref()) {
        keep_at(ed, path);
    }
}

/// `keep`, to marker `path`.
fn keep_at(ed: &Editor, path: &Path) {
    let Some(h) = &ed.crash.held else { return };
    let drawing =
        Drawing { root: h.root.clone(), session: h.session.clone(), pane: h.pane.clone(), again: false };
    let c = Crashed {
        at: h.at.clone(),
        message: String::new(),
        thread: String::new(),
        build: h.build.clone(),
        drawing: Some(drawing),
        held: true,
        report: None,
    };
    if let Err(e) = write(path, &c) {
        crate::log::line("tarae", "warn", &format!("{}: the crash's hold not kept: {e}", path.display()));
    }
}

/// The front session's `pane` is held: none of its content is laid out or drawn.
pub fn holds<'a>(ed: &'a Editor, pane: &Pane) -> Option<&'a Held> {
    ed.crash.held.as_ref().filter(|h| h.root == ed.workspace.root && h.pane == *pane)
}

/// The front session's held pane shown (`enter` on it, a press on it) — drawn from the next frame on. Whether one was.
pub fn show(ed: &mut Editor) -> bool {
    if !ed.crash.held.as_ref().is_some_and(|h| h.root == ed.workspace.root) {
        return false;
    }
    ed.crash.shown = ed.crash.held.take();
    true
}

/// A code pane's `enter` in normal mode, on a file a crash holds: shown.
pub fn code_key(ed: &mut Editor, key: crate::key::Key) -> bool {
    use crate::key::{Code, Key};
    if key != Key::plain(Code::Enter) || ed.mode != crate::editor::Mode::Normal || !ed.pending.is_empty() {
        return false;
    }
    let path = ed.doc().path.as_deref().map(Path::to_path_buf).unwrap_or_default();
    holds(ed, &Pane::Code(path)).is_some() && show(ed)
}

/// The chat's view mode's `enter`, its transcript held: shown.
pub fn chat_key(ed: &mut Editor) -> bool {
    use crate::workspace::Here;
    ed.workspace.is_at(Here::Chat) && holds(ed, &Pane::Chat).is_some() && show(ed)
}

#[cfg(test)]
thread_local!(pub static STAND_IN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });

/// Tests' stand-in for a drawer's panic (`STAND_IN` set on this thread): where a layout of a pane would slice wrong.
#[cfg(test)]
pub fn stand_in() {
    if STAND_IN.get() {
        panic!("byte index 170 is not a char boundary; it is inside '—' (bytes 168..171) of `a secret line`");
    }
}

pub mod report;

#[cfg(test)]
mod tests;
