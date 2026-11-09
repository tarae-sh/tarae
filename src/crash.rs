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

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::{Value, json};

use crate::editor::Editor;

/// The folder of the markers in tarae's state.
const DIR: &str = "crashed";
/// A panic's message kept at most this many characters (a toast can land in a recording).
const MESSAGE: usize = 120;
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

static MARKER: OnceLock<PathBuf> = OnceLock::new();

/// At the start, before the first frame: a panic from now on leaves its marker at `marker` (`file`).
pub fn begin(marker: PathBuf) {
    if MARKER.set(marker).is_err() {
        return;
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        left(info);
        prev(info);
    }));
}

/// The hook's: the marker written, when this panic ends tarae.
fn left(info: &std::panic::PanicHookInfo) {
    let drawing = DRAWING.try_with(|d| d.try_borrow().ok().and_then(|d| d.clone())).ok().flatten();
    if !cfg!(panic = "abort") && drawing.is_none() {
        return;
    }
    let Some(path) = MARKER.get() else { return };
    let payload = info.payload();
    let message = (payload.downcast_ref::<&str>().copied())
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or_default();
    let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
    let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
    let build = tarae_wire::BUILD.to_string();
    let c = Crashed { at, message: cut(message), thread, build, drawing, held: false };
    let _ = write(path, &c);
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

/// The start's words for `c`: what happened first, where last (`· file:line, build …` — what a narrow toast cuts).
pub fn said(c: &Crashed) -> String {
    let place = format!("{}, build {}", c.at, build_said(&c.build));
    match &c.drawing {
        Some(d) => {
            let what = match &d.pane {
                Pane::Chat => format!("{}'s chat", d.session),
                Pane::Code(p) => {
                    format!("{}'s {}", d.session, p.strip_prefix(&d.root).unwrap_or(p).display())
                }
            };
            if c.held {
                return format!("{what} still held after a crash · {place}");
            }
            let verb = if d.again { "crashed again showing" } else { "crashed drawing" };
            format!("tarae {verb} {what} — it opens held · {place}")
        }
        None => format!("tarae crashed · {place}"),
    }
}

/// The start, a crash's marker read: said at once — the whole of it in the log, the message too —, its pane held; a
/// hold carried over (`keep`), held again and said quietly.
pub fn start(ed: &mut Editor, c: Crashed) {
    let words = said(&c);
    if c.held {
        crate::log::line("tarae", "info", &words);
        ed.set_status(words);
    } else {
        crate::log::line("tarae", "error", &format!("{words} — {} (thread {})", c.message, c.thread));
        ed.set_warning(words);
    }
    let (at, build) = (c.at, c.build);
    ed.crash.held = c.drawing.map(|d| Held { root: d.root, session: d.session, pane: d.pane, at, build });
}

/// At a quit or `:restart`'s exec: a hold still standing written back to the start's marker (`begin`), so the next
/// start holds it again — without it, the same build would crash drawing it again. A crash after this writes its own.
pub fn keep(ed: &Editor) {
    if let Some(path) = MARKER.get() {
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

#[cfg(test)]
mod tests;
