//! The driver's hands on a take (the full video's V3, main's `.sb/full-video-v3-brief.md`): a way into a running
//! `record` for another process of the same user, so the driver types the written asks and answers what tarae asks by
//! tarae's own state — never blind, never by words on the screen. Keys typed straight into the pane pass as ever: the
//! user can take over at any point.
//!
//! - **The way in** (`WayIn`): a socket, `in.sock`, in a folder made new for the take in the take's own folder
//!   (`DIR/tarae-rec-<uid>-<n>`, 0700 and checked so: only its user's processes reach it, and each connection's peer is
//!   checked to be that user too) — inside the project, where tarae's sandbox lets a session's runs connect, so the
//!   driver's commands reach it with no card; a path past `sun_path` (`SUN_PATH`) bound and reached from its folder
//!   (`tarae_sock`). Said when `record` starts and kept in the take's `way-in`; both gone when `record` ends.
//! - `record type --to WHERE [--send] WORDS` — the words typed with the typist's rhythm (`shot::typed`), only where
//!   tarae has the keys at WHERE; `--send` leaves them standing `ASK_STANDS` (the same constant `cut init` keeps them
//!   by) and sends the return, WHERE checked again first.
//! - `record key [--to WHERE] KEY …` — keys in the scenes' words (`space l`, `C-q`, `y`, `<ret>`, `up`, `bs`;
//!   `key_bytes`: a word that's no key's name is refused, never typed as text), a key's pause after each; with `--to`,
//!   the same guard.
//! - `record wait WHAT [--for SECS] [--session S]` — until it holds by tarae's state and events (`Wait`): `turn` (a
//!   turn ended since the hands' last ask, one no `wait turn` returned yet — keys pressed since or not; the ask line's
//!   end too, `space i`'s, which is no turn of the chat's), `card [KIND]` (one in front, or an edit's review up: `edit`),
//!   `keys WHERE`, `run` (one ended since the hands' last key), `bring` (one ended since). A card that comes in front
//!   first (or an edit's review) ends any other wait, said: it's answered at once, never waited past. tarae's state is
//!   looked at again at each event and every `RELOOK` with none (not every change sends one). Gives up at its patience
//!   (`PATIENCE` unless `--for`); what's in front said either way.
//! - `record state` — tarae's state as its text form, a fact a line (`tarae state`'s).
//! - `record mark WORDS` — a landmark `mark` at that moment, the words with it (`mark N` in an edit list): a note.
//! - `record chapter WORDS` — a landmark `chapter`: a chapter starts here, the words its title (`cut init` puts it).
//!
//! Each finds the way in by `--via` (the socket, or the take's folder) or `$TARAE_RECORD`, and prints the line it's
//! answered; its exit is `code`'s: 0 held, 2 refused (the keys elsewhere), 3 a card came first, 4 gave up, 1 else.
//!
//! WHERE is where the keys are by tarae's state (`keys.region`, `docs/design/ui-state.md`, `place`), in these words:
//! `chat` (the chat's input), `ask` (the ask line, `space i`), `command` (`:`), `comment` (a review's), `session-name`
//! (a new session's), `prompt` (any prompt), or a region's own name (`code`, `card`, `picker`, `review`,
//! `edit-review`, `list:sessions` …) or its family's (`side` for `side:git`, `view` for `view:chat`); a side panel's
//! tab also as `state` names it (`git` for `side:git`).

use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::keys::AFTER_KEY;
use super::{Log, SCRIPT, Seen};
use crate::cut::init::ASK_STANDS;
use crate::{Result, shot};

#[cfg(test)]
mod tests;

/// The way in's socket, in its folder; the take's file that names it.
const SOCK: &str = "in.sock";
pub(crate) const WAY_IN: &str = "way-in";

/// `sun_path`'s room on macOS (Linux's is 108): a socket path under it is reached as it is, by anyone.
pub(crate) const SUN_PATH: usize = 104;

/// A wait's patience unless `--for` says: the runbook's "a turn that hasn't moved for ten minutes".
const PATIENCE: Duration = Duration::from_secs(600);

/// How long a connection's request is waited for once it's made.
const ASKED_IN: Duration = Duration::from_secs(10);

/// How often a wait looks at tarae's state again with no news: a change that sends no event (`:`'s prompt turning into
/// the session prompt keeps the region `prompt`, #522) is still seen.
const RELOOK: Duration = Duration::from_millis(250);

/// What the ask line's end (`space i`: no turn of the chat's, no `turn` event) says, as a toast: its change up for
/// review (`ask::reviewing`), none made (`ask::ended`); an error starting `claude: ` is its end too.
const ASK_ENDS: [&str; 2] = ["claude answered in ", "claude: no changes"];

/// The environment variable that names the way in.
pub(crate) const ENV: &str = "TARAE_RECORD";

/// What an answer's exit says.
pub(crate) mod code {
    pub(crate) const HELD: i32 = 0;
    pub(crate) const ERROR: i32 = 1;
    pub(crate) const REFUSED: i32 = 2;
    pub(crate) const CARD: i32 = 3;
    pub(crate) const GAVE_UP: i32 = 4;
}

// ── The way in ─────────────────────────────────────────────────────────────────────────────────────────────────

/// The take's way in: its folder, its socket, listening. Dropped, both go — only what it made.
pub(crate) struct WayIn {
    pub(crate) dir: PathBuf,
    pub(crate) sock: PathBuf,
    pub(crate) listener: UnixListener,
}

impl WayIn {
    /// A folder made new under `base` (the take's folder), 0700 and ours — refused where it isn't — and its socket
    /// bound in it.
    pub(crate) fn open(base: &Path) -> Result<WayIn> {
        let dir = fresh_folder(base, "tarae-rec").map_err(|e| format!("the way in: {e}"))?;
        let gone = |why: String| {
            let _ = std::fs::remove_dir(&dir);
            Err(format!("the way in: {}: {why}", dir.display()))
        };
        // (a path past `sun_path` — a checkout deep in a home, a test's scratch folder — bound from its folder, and
        // reached so by `ask`)
        let sock = dir.join(SOCK);
        match tarae_sock::at(&sock, |p: &Path| UnixListener::bind(p)) {
            Ok(listener) => Ok(WayIn { dir, sock, listener }),
            Err(e) => gone(format!("{SOCK}: {e}")),
        }
    }
}

/// A folder made new under `base`, `<prefix>-<uid>-<n>` (the next `n` where one's there — never one that was), 0700
/// and checked so: a folder, this user's, nobody else's bits.
pub(crate) fn fresh_folder(base: &Path, prefix: &str) -> Result<PathBuf> {
    // SAFETY: getuid can't fail.
    let uid = unsafe { libc::getuid() };
    for n in 1..1000 {
        let dir = base.join(format!("{prefix}-{uid}-{n}"));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", dir.display())),
        }
        // (made new just now, so ours — checked all the same)
        return match std::fs::symlink_metadata(&dir) {
            Ok(m) if m.is_dir() && m.uid() == uid && m.mode() & 0o077 == 0 => Ok(dir),
            Ok(m) => {
                let _ = std::fs::remove_dir(&dir);
                Err(format!(
                    "{}: mode {:o}, owner {}: not 0700 and yours",
                    dir.display(),
                    m.mode() & 0o7777,
                    m.uid()
                ))
            }
            Err(e) => {
                let _ = std::fs::remove_dir(&dir);
                Err(format!("{}: {e}", dir.display()))
            }
        };
    }
    Err(format!("no free name under {}", base.display()))
}

impl Drop for WayIn {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.sock);
        let _ = std::fs::remove_dir(&self.dir);
    }
}

/// Whether the peer of `s` is this process's user.
fn ours(s: &UnixStream) -> bool {
    // SAFETY: geteuid can't fail.
    let me = unsafe { libc::geteuid() };
    peer_uid(s).is_some_and(|u| u == me)
}

#[cfg(target_os = "macos")]
fn peer_uid(s: &UnixStream) -> Option<u32> {
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: getpeereid into two locals, on a socket held here.
    (unsafe { libc::getpeereid(s.as_raw_fd(), &mut uid, &mut gid) } == 0).then_some(uid)
}

#[cfg(not(target_os = "macos"))]
fn peer_uid(s: &UnixStream) -> Option<u32> {
    // SAFETY: a ucred for getsockopt to fill, its length.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: SO_PEERCRED into that ucred, on a socket held here.
    let r = unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (r == 0).then_some(cred.uid)
}

// ── What the socket said ───────────────────────────────────────────────────────────────────────────────────────

/// The take's news for the hands' waits: the events that end one (a turn's, a run's, a bring's), each with when it
/// came; how many came in all (a wait looks again at each); when the hands last sent a key; whether the take ended.
#[derive(Default)]
pub(crate) struct Hub {
    now: Mutex<HubNow>,
    changed: Condvar,
}

#[derive(Default)]
struct HubNow {
    told: Vec<(Instant, Value)>,
    news: u64,
    sent: Option<Instant>,
    /// When the hands last sent an ask (`type --send`: the chat's, the ask line's).
    asked: Option<Instant>,
    /// For each `wait turn [SESSION]`, when it last held: a turn's end is returned once.
    turns_taken: Vec<(Option<String>, Instant)>,
    ended: bool,
}

impl Hub {
    fn lock(&self) -> std::sync::MutexGuard<'_, HubNow> {
        self.now.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// An event of tarae's, as it came.
    pub(crate) fn told(&self, at: Instant, ev: &Value) {
        let mut now = self.lock();
        if matches!(ev["event"].as_str(), Some("turn" | "run" | "bring" | "toast")) {
            now.told.push((at, ev.clone()));
        }
        now.news += 1;
        self.changed.notify_all();
    }

    pub(crate) fn end(&self) {
        self.lock().ended = true;
        self.changed.notify_all();
    }

    fn ended(&self) -> bool {
        self.lock().ended
    }

    fn sent(&self, at: Instant) {
        self.lock().sent = Some(at);
    }

    /// An ask sent at `at`.
    pub(crate) fn asked(&self, at: Instant) {
        self.lock().asked = Some(at);
    }

    /// Where a wait's events start: for a turn, the last ask (else the last key) — a turn that ended between the ask
    /// and the wait, keys pressed or not, still counts — but past the end a `wait turn` for the same session already
    /// returned; for the rest, the hands' last key. Now where neither was.
    pub(crate) fn since_for(&self, w: &Wait) -> Instant {
        let now = self.lock();
        let base = match w {
            Wait::Turn(_) => now.asked.or(now.sent),
            _ => now.sent,
        };
        let taken = match w {
            Wait::Turn(who) => now.turns_taken.iter().find(|(w, _)| w == who).map(|(_, at)| *at),
            _ => None,
        };
        base.into_iter().chain(taken).max().unwrap_or_else(Instant::now)
    }

    /// A `wait turn` for `who` held at `at`: the ends before it are taken.
    fn turn_taken(&self, who: &Option<String>, at: Instant) {
        let mut now = self.lock();
        now.turns_taken.retain(|(w, _)| w != who);
        now.turns_taken.push((who.clone(), at));
    }

    fn news(&self) -> u64 {
        self.lock().news
    }

    /// The events kept that came at `since` or after.
    fn since(&self, since: Instant) -> Vec<Value> {
        self.lock().told.iter().filter(|(at, _)| *at >= since).map(|(_, v)| v.clone()).collect()
    }

    /// Until news past `seen` came (true), or `until` passed or the take ended (false) — the deadline first: news that
    /// keeps coming (tarae sends events all the time) never keeps a wait past its patience.
    fn wait_news(&self, seen: u64, until: Instant) -> bool {
        let mut now = self.lock();
        loop {
            let Some(left) = until.checked_duration_since(Instant::now()).filter(|l| !l.is_zero()) else {
                return false;
            };
            if now.ended {
                return false;
            }
            if now.news != seen {
                return true;
            }
            now = self.changed.wait_timeout(now, left).unwrap_or_else(|p| p.into_inner()).0;
        }
    }
}

// ── Serving ────────────────────────────────────────────────────────────────────────────────────────────────────

/// What the hands act on: the take's terminal (each write handed to the log as the key script's), the screen's end
/// (the typist's pauses stop there), the socket's news, tarae's UI socket (None: none watched), the take's start, and
/// one typist at a time.
pub(crate) struct Ctx {
    pub(crate) to: File,
    pub(crate) tx: Sender<Log>,
    pub(crate) seen: Arc<Seen>,
    pub(crate) hub: Arc<Hub>,
    pub(crate) ui: Option<PathBuf>,
    pub(crate) started: Instant,
    pub(crate) typing: Mutex<()>,
}

impl Ctx {
    fn write(&self, bytes: &[u8]) {
        let at = Instant::now();
        let _ = (&self.to).write_all(bytes);
        let _ = self.tx.send(Log::Key(at, SCRIPT, bytes.to_vec()));
        self.hub.sent(at);
    }

    fn state(&self) -> Result<Value> {
        ui_ask(self.ui.as_deref(), json!({ "get": "state" }))
    }
}

/// One request to tarae's UI socket.
fn ui_ask(ui: Option<&Path>, req: Value) -> Result<Value> {
    let sock = ui.ok_or("no UI socket watched (record --state)")?;
    let s = shot::ui_connect(sock).map_err(|e| format!("{}: {e}", sock.display()))?;
    shot::ui_ask_on(s, sock, &req)
}

/// The way in served until `stop` is readable: each connection of this user's on a thread of its own, answered once;
/// every one of those joined before it returns (a wait ends at the hub's end, a typist at the screen's).
pub(crate) fn serve(listener: &UnixListener, stop: &OwnedFd, ctx: &Arc<Ctx>) {
    let mut hands = Vec::new();
    loop {
        let mut fds = [
            libc::pollfd { fd: listener.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: stop.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        ];
        // SAFETY: two pollfds of descriptors held here.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if n < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if n < 0 || fds[1].revents != 0 {
            break;
        }
        let Ok((s, _)) = listener.accept() else { continue };
        if !ours(&s) {
            continue;
        }
        let ctx = ctx.clone();
        match std::thread::Builder::new().name("record-hand".into()).spawn(move || hand(&s, &ctx)) {
            Ok(t) => hands.push(t),
            Err(_) => continue,
        }
        hands.retain(|t| !t.is_finished());
    }
    for t in hands {
        let _ = t.join();
    }
}

/// A connection's one request, answered.
fn hand(s: &UnixStream, ctx: &Ctx) {
    let _ = s.set_read_timeout(Some(ASKED_IN));
    let mut line = String::new();
    if BufReader::new(s).read_line(&mut line).is_err() {
        return;
    }
    let (code, said) = match serde_json::from_str::<Value>(&line) {
        Ok(req) => act(&req, ctx),
        Err(e) => (code::ERROR, format!("not a request: {e}")),
    };
    let _ = writeln!(&mut &*s, "{}", json!({ "code": code, "said": said }));
}

/// A request done: its exit and its line.
fn act(req: &Value, ctx: &Ctx) -> (i32, String) {
    let text = req["text"].as_str().unwrap_or_default();
    let to = req["to"].as_str();
    match req["do"].as_str().unwrap_or_default() {
        "type" => match to {
            Some(to) => typed(ctx, to, text, req["send"] == true),
            None => (code::ERROR, "type wants --to WHERE: words are never typed blind".into()),
        },
        "key" => {
            let keys: Vec<String> = (req["keys"].as_array().into_iter().flatten())
                .filter_map(|k| k.as_str().map(String::from))
                .collect();
            keyed(ctx, to, &keys)
        }
        "wait" => match Wait::parse(req["what"].as_str().unwrap_or_default(), req["arg"].as_str()) {
            Ok(w) => {
                let patience = req["for_ms"].as_u64().map_or(PATIENCE, Duration::from_millis);
                let since = ctx.hub.since_for(&w);
                wait(&w, patience, &mut || ctx.state(), &ctx.hub, since)
            }
            Err(e) => (code::ERROR, e),
        },
        "state" => match ui_ask(ctx.ui.as_deref(), json!({ "get": "state", "format": "text" })) {
            Ok(v) => (code::HELD, v["text"].as_str().unwrap_or_default().trim_end().to_string()),
            Err(e) => (code::ERROR, format!("tarae's state: {e}")),
        },
        what @ ("mark" | "chapter") => {
            let at = Instant::now();
            let what = if what == "chapter" { "chapter" } else { "mark" };
            let _ = ctx.tx.send(Log::Mark(at, what, text.to_string()));
            let ms = at.saturating_duration_since(ctx.started).as_millis() as u64;
            let said = if what == "chapter" { "a chapter from" } else { "marked at" };
            (code::HELD, format!("{said} {} — {text}", crate::cut::clock(ms)))
        }
        other => (code::ERROR, format!("{other:?}: type, key, wait, state, mark or chapter")),
    }
}

/// `text` typed where the keys are at `to` — else nothing, said — and sent once it stood `ASK_STANDS` where `send`.
fn typed(ctx: &Ctx, to: &str, text: &str, send: bool) -> (i32, String) {
    let _one = ctx.typing.lock().unwrap_or_else(|p| p.into_inner());
    let state = match ctx.state() {
        Ok(s) => s,
        Err(e) => return (code::ERROR, format!("tarae's state: {e} — nothing typed")),
    };
    if let Err(e) = guard(to, &state) {
        return (code::REFUSED, format!("{e} — nothing typed"));
    }
    for (i, (c, gap)) in shot::typed(text).into_iter().enumerate() {
        if i > 0 && !ctx.seen.pause(Duration::from_secs_f64(gap.max(0.0))) {
            return (code::ERROR, "the take ended while typing".into());
        }
        ctx.write(c.to_string().as_bytes());
    }
    let n = text.chars().count();
    if !send {
        ctx.seen.pause(AFTER_KEY);
        return (code::HELD, format!("typed {n} characters into {to}"));
    }
    ctx.seen.pause(Duration::from_millis(ASK_STANDS));
    let state = match ctx.state() {
        Ok(s) => s,
        Err(e) => return (code::ERROR, format!("typed {n} characters, not sent: tarae's state: {e}")),
    };
    if let Err(e) = guard(to, &state) {
        return (code::REFUSED, format!("typed {n} characters, not sent: {e}"));
    }
    ctx.write(b"\r");
    ctx.hub.asked(Instant::now());
    ctx.seen.pause(AFTER_KEY);
    (
        code::HELD,
        format!("typed {n} characters into {to}, sent after {:.1} s standing", ASK_STANDS as f64 / 1e3),
    )
}

/// `keys` sent one by one, a key's pause after each — where `to` says, only with the keys there.
fn keyed(ctx: &Ctx, to: Option<&str>, keys: &[String]) -> (i32, String) {
    let bytes = match keys.iter().map(|k| key_bytes(k)).collect::<Result<Vec<_>>>() {
        Ok(b) if !b.is_empty() => b,
        Ok(_) => return (code::ERROR, "key wants a key or more".into()),
        Err(e) => return (code::ERROR, e),
    };
    let _one = ctx.typing.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(to) = to {
        let state = match ctx.state() {
            Ok(s) => s,
            Err(e) => return (code::ERROR, format!("tarae's state: {e} — nothing sent")),
        };
        if let Err(e) = guard(to, &state) {
            return (code::REFUSED, format!("{e} — nothing sent"));
        }
    }
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && !ctx.seen.pause(AFTER_KEY) {
            return (code::ERROR, "the take ended while keying".into());
        }
        ctx.write(b);
    }
    ctx.seen.pause(AFTER_KEY);
    (code::HELD, format!("sent {}", keys.join(" ")))
}

/// A key in the scenes' words as the bytes a terminal sends for it: `space`, `<ret>` (`ret`, `enter`), `<esc>`,
/// `<tab>`, `<bs>` (`bs`, `backspace`), the arrows (`<up>`, `up` …), `C-x`, `A-x`; one character as itself; the
/// shots' escapes (`\r`, `\x1b`). Any other word refused, never typed as text (`key up` typed "up" on camera, #522):
/// words go with `type`. An unknown `<name>` refused.
pub(crate) fn key_bytes(k: &str) -> Result<Vec<u8>> {
    let named = |s: &str| -> Option<&'static [u8]> {
        Some(match s {
            "space" | "<space>" => b" ",
            "ret" | "<ret>" | "enter" | "<enter>" => b"\r",
            "esc" | "<esc>" => b"\x1b",
            "tab" | "<tab>" => b"\t",
            "bs" | "<bs>" | "backspace" | "<backspace>" => b"\x7f",
            "up" | "<up>" => b"\x1b[A",
            "down" | "<down>" => b"\x1b[B",
            "right" | "<right>" => b"\x1b[C",
            "left" | "<left>" => b"\x1b[D",
            "<lt>" => b"<",
            _ => return None,
        })
    };
    if let Some(b) = named(k) {
        return Ok(b.to_vec());
    }
    if let Some(rest) = k.strip_prefix("C-") {
        let mut cs = rest.chars();
        return match (cs.next(), cs.next(), rest) {
            (_, _, "space") => Ok(vec![0]),
            (Some(c), None, _) if c.is_ascii_alphabetic() || "@[\\]^_".contains(c) => {
                Ok(vec![c.to_ascii_lowercase() as u8 & 0x1f])
            }
            _ => Err(format!("{k}: C- takes a letter (or @ [ \\ ] ^ _ space)")),
        };
    }
    if let Some(rest) = k.strip_prefix("A-").filter(|r| !r.is_empty()) {
        let mut b = vec![0x1b];
        b.extend(key_bytes(rest)?);
        return Ok(b);
    }
    if k.len() > 2 && k.starts_with('<') && k.ends_with('>') {
        return Err(format!(
            "{k}: not a key's name — <ret> <esc> <tab> <bs> <space> <up> <down> <left> <right> <lt>"
        ));
    }
    if k.is_empty() {
        return Err("an empty key".into());
    }
    if k.chars().count() > 1 && !k.starts_with('\\') {
        return Err(format!(
            "{k}: not a key — a word goes with `type --to WHERE`; a key is one character or a name: space ret esc tab \
             bs up down left right, C-x, A-x"
        ));
    }
    Ok(shot::unescape(k))
}

// ── Where the keys are ─────────────────────────────────────────────────────────────────────────────────────────

/// Where the keys are in a state, in WHERE's words (the module's doc).
pub(crate) fn place(s: &Value) -> String {
    let region = s["keys"]["region"].as_str().unwrap_or("nowhere said");
    if region != "prompt" {
        return region.to_string();
    }
    match s["prompt"]["kind"].as_str() {
        Some("change:") => "ask".into(),
        Some(":") => "command".into(),
        Some("comment:") => "comment".into(),
        Some(l) if l.starts_with("new session") => "session-name".into(),
        Some(label) => format!("prompt {label}"),
        None => "prompt".into(),
    }
}

/// Whether the keys are at `to` in a state: its place, the family's name (`side` for `side:git`), any prompt for
/// `prompt`, and a side panel's tab as `state` names it (`git` for `side:git`: its `keys → git`, #522) — never the
/// chat's view for `chat`, its input.
pub(crate) fn at(to: &str, s: &Value) -> bool {
    let p = place(s);
    p == to
        || (to == "prompt" && s["keys"]["region"] == "prompt")
        || p.strip_prefix(to).is_some_and(|rest| rest.starts_with(':'))
        || p.strip_prefix("side:").is_some_and(|tab| tab == to && s["keys"]["to"] == to)
}

/// Ok where the keys are at `to`; else where they are, said.
pub(crate) fn guard(to: &str, s: &Value) -> Result<()> {
    match at(to, s) {
        true => Ok(()),
        false => Err(format!("the keys are in {}, not {to} ({})", place(s), front_line(s))),
    }
}

/// A state in one line: where the keys are, each session and what it does, the cards in front.
pub(crate) fn front_line(s: &Value) -> String {
    let k = &s["keys"];
    let mut parts = vec![format!("keys → {} ({})", place(s), k["takes"].as_str().unwrap_or("?"))];
    let front = s["front"].as_str();
    for w in s["workspaces"].as_array().into_iter().flatten() {
        let mark = if w["id"].as_str() == front { " (front)" } else { "" };
        for x in w["sessions"].as_array().into_iter().flatten() {
            let name = w["name"].as_str().unwrap_or("?");
            parts.push(format!("{} {name}{mark} {}", x["id"].as_str().unwrap_or("?"), word(&x["doing"])));
        }
    }
    parts.extend(in_front(s).map(card_line));
    parts.extend(in_review(s));
    parts.join(" · ")
}

/// Whether `kind` (a `wait card` KIND) names an edit's review.
fn is_review(kind: &str) -> bool {
    matches!(kind, "edit" | "edit-review" | "review")
}

/// A value as a word: a string as it is, anything else as its JSON.
fn word(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), String::from)
}

/// The cards in front (`up` front: taking their answers once focused).
fn in_front(s: &Value) -> impl Iterator<Item = &Value> {
    s["cards"].as_array().into_iter().flatten().filter(|c| c["up"] == "front")
}

/// What waits for an answer in front besides the cards: an edit's review up (the inbox's `up` `review`), as a line —
/// with whether it has the keys. One Claude's edit raised waits for `C-q` as a card does (#528); one your own key
/// opened (`space i`'s) has them at once.
fn in_review(s: &Value) -> impl Iterator<Item = String> + '_ {
    let keys = match place(s) == "edit-review" {
        true => "has the keys",
        false => "waits — C-q focuses it",
    };
    (s["inbox"].as_array().into_iter().flatten()).filter(|e| e["up"] == "review").map(move |e| {
        format!("{} {} · {:?} · its review up · {keys}", word(&e["id"]), word(&e["kind"]), word(&e["asks"]))
    })
}

/// A card as a line: its id, kind, title, what it asks, its answers, and whether it has the keys.
fn card_line(c: &Value) -> String {
    let answers: Vec<String> = (c["answers"].as_array().into_iter().flatten())
        .map(|a| format!("{} {}", word(&a["key"]), word(&a["does"])))
        .collect();
    let keys = match (c["focused"] == true, c["waits"] == "focus") {
        (true, _) => "has the keys",
        (_, true) => "waits — C-q focuses it",
        _ => "up",
    };
    format!(
        "{} {} · {} · {:?} · {} · {keys}",
        word(&c["id"]),
        word(&c["kind"]),
        word(&c["title"]),
        word(&c["asks"]),
        answers.join(" · ")
    )
}

// ── Waits ──────────────────────────────────────────────────────────────────────────────────────────────────────

/// What a wait is for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Wait {
    /// A turn ended since the hands' last ask (`Hub::since_for`), once — that session's (an id, `s1`, or its
    /// workspace's name) where said; with none said, the ask line's end too (`ASK_ENDS`).
    Turn(Option<String>),
    /// A card in front — of that kind where said; an edit's review up too, where none is said or the kind is `edit`
    /// (`edit-review`, `review`).
    Card(Option<String>),
    /// The keys at WHERE.
    Keys(String),
    /// A run ended since the hands' last key.
    Run,
    /// A bring ended since the hands' last key (landed, its preflight failed, stopped at conflicts, declined).
    Bring,
}

impl Wait {
    pub(crate) fn parse(what: &str, arg: Option<&str>) -> Result<Wait> {
        Ok(match (what, arg) {
            ("turn", s) => Wait::Turn(s.map(String::from)),
            ("card", k) => Wait::Card(k.map(String::from)),
            ("keys", Some(w)) => Wait::Keys(w.to_string()),
            ("keys", None) => return Err("wait keys WHERE: chat, ask, code, card …".into()),
            ("run", None) => Wait::Run,
            ("bring", None) => Wait::Bring,
            (w, _) => return Err(format!("wait {w}: turn [SESSION], card [KIND], keys WHERE, run or bring")),
        })
    }

    fn said(&self) -> String {
        match self {
            Wait::Turn(None) => "a turn's end".into(),
            Wait::Turn(Some(s)) => format!("{s}'s turn's end"),
            Wait::Card(None) => "a card".into(),
            Wait::Card(Some(k)) => format!("a {k} card"),
            Wait::Keys(w) => format!("the keys in {w}"),
            Wait::Run => "a run's end".into(),
            Wait::Bring => "a bring's end".into(),
        }
    }
}

/// How a wait ended where it did: what it waited for held, or a card came in front first.
#[derive(Debug, PartialEq)]
pub(crate) enum Verdict {
    Held(String),
    Card(String),
}

/// Whether `w` holds in state `s` with `told` the events since its start (`Hub::since`); else whether a card in front,
/// or an edit's review up, ends it first (every wait but for a card or the keys).
pub(crate) fn judge(w: &Wait, s: &Value, told: &[Value]) -> Option<Verdict> {
    let ended = |kind: &'static str| told.iter().filter(move |e| e["event"] == kind);
    let of = |e: &Value, who: &str| {
        e["session"] == who
            || (s["workspaces"].as_array().into_iter().flatten()).any(|x| {
                x["name"] == who
                    && x["sessions"].as_array().into_iter().flatten().any(|y| y["id"] == e["session"])
            })
    };
    let held = match w {
        Wait::Card(kind) => {
            (in_front(s).find(|c| kind.as_deref().is_none_or(|k| c["kind"] == k)).map(card_line))
                .or_else(|| kind.as_deref().is_none_or(is_review).then(|| in_review(s).next()).flatten())
        }
        Wait::Keys(to) => at(to, s).then(|| format!("the keys are in {}", place(s))),
        Wait::Turn(who) => ended("turn")
            .find(|e| e["what"] == "ended" && who.as_deref().is_none_or(|who| of(e, who)))
            .map(|e| match e["said"].as_str() {
                Some(said) => said.to_string(),
                None => format!("{}'s turn ended", word(&e["session"])),
            })
            // (the ask line's end: no turn of the chat's — its toast; its session is the one in front)
            .or_else(|| {
                let text = |e: &Value| e["text"].as_str().unwrap_or_default().to_string();
                let ask_end = |t: &str| ASK_ENDS.iter().any(|a| t.starts_with(a));
                (who.is_none().then(|| ended("toast")).into_iter().flatten())
                    .map(|e| (text(e), e["kind"] == "error"))
                    .find(|(t, error)| ask_end(t) || (*error && t.starts_with("claude: ")))
                    .map(|(t, _)| format!("the ask line's end: {t}"))
            }),
        Wait::Run => ended("run").find(|e| e["ended"] == true).map(|e| {
            e["said"]
                .as_str()
                .map_or_else(|| format!("{} ended: {}", word(&e["id"]), word(&e["step"])), String::from)
        }),
        Wait::Bring => ended("bring").next().map(|e| {
            e["said"]
                .as_str()
                .map_or_else(|| format!("{} {}", word(&e["name"]), word(&e["how"])), String::from)
        }),
    };
    match (held, w) {
        (Some(l), _) => Some(Verdict::Held(l)),
        (None, Wait::Card(_) | Wait::Keys(_)) => None,
        (None, _) => in_front(s).next().map(card_line).or_else(|| in_review(s).next()).map(Verdict::Card),
    }
}

/// Until `w` holds (`judge`, a fresh state from `ask` at each piece of news the hub has), a card comes in front first,
/// or `patience` passes; the events from `since` on count. Its exit and its line, what's in front with it.
pub(crate) fn wait(
    w: &Wait,
    patience: Duration,
    ask: &mut dyn FnMut() -> Result<Value>,
    hub: &Hub,
    since: Instant,
) -> (i32, String) {
    let (begun, until) = (Instant::now(), Instant::now() + patience);
    loop {
        let news = hub.news();
        let state = ask();
        let s = state.as_ref().unwrap_or(&Value::Null);
        match judge(w, s, &hub.since(since)) {
            Some(Verdict::Held(l)) => {
                if let Wait::Turn(who) = w {
                    hub.turn_taken(who, Instant::now());
                }
                return (code::HELD, format!("{l} — {}", front_line(s)));
            }
            Some(Verdict::Card(l)) => {
                return (code::CARD, format!("a card first, waiting for {}: {l} — answer it", w.said()));
            }
            None => {}
        }
        // (news, or a look again after `RELOOK` with none: not every change sends an event)
        if !hub.wait_news(news, until.min(Instant::now() + RELOOK))
            && (hub.ended() || Instant::now() >= until)
        {
            let why = match hub.ended() {
                true => "the take ended".to_string(),
                false => format!("gave up after {:.1} s", begun.elapsed().as_secs_f64()),
            };
            let front = state.map_or_else(|e| format!("tarae's state: {e}"), |s| front_line(&s));
            return (code::GAVE_UP, format!("{why} waiting for {}: {front}", w.said()));
        }
    }
}

// ── The client ─────────────────────────────────────────────────────────────────────────────────────────────────

const USAGE: &str = "usage: cargo xtask record type --to WHERE [--send] WORDS | record key [--to WHERE] KEY … | record \
                     wait turn [SESSION]|card [KIND]|keys WHERE|run|bring [--for SECS] | record state | record mark \
                     WORDS (a note) | record chapter WORDS (where a chapter starts, its title) — each with --via SOCKET|TAKE, else $TARAE_RECORD";

/// `record type|key|wait|state|mark|chapter …`: the request made through the way in, its line printed, its exit the process's.
pub(crate) fn client(verb: &str, args: &[String]) -> Result<()> {
    let (req, via) = request(verb, args)?;
    let sock = way_in(via.as_deref())?;
    let (c, said) = ask(&sock, &req)?;
    println!("{said}");
    if c != code::HELD {
        std::process::exit(c);
    }
    Ok(())
}

/// A command line as its request, and its `--via`.
pub(crate) fn request(verb: &str, args: &[String]) -> Result<(Value, Option<PathBuf>)> {
    let (mut to, mut send, mut via, mut patience, mut session, mut words) =
        (None, false, None, None, None, Vec::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{a} wants a value — {USAGE}"));
        match a.as_str() {
            "--to" => to = Some(value()?),
            "--via" => via = Some(PathBuf::from(value()?)),
            "--for" => {
                let v = value()?;
                let s: f64 =
                    v.parse().ok().filter(|s: &f64| *s >= 0.0).ok_or(format!("--for {v}: seconds"))?;
                patience = Some((s * 1000.0) as u64);
            }
            "--session" => session = Some(value()?),
            "--send" => send = true,
            w => words.push(w.to_string()),
        }
    }
    let req = match verb {
        "type" if !words.is_empty() => {
            json!({ "do": "type", "to": to, "text": words.join(" "), "send": send })
        }
        "key" if !words.is_empty() => json!({ "do": "key", "to": to, "keys": words }),
        "wait" if !words.is_empty() => {
            let arg = words.get(1).cloned().or(session).or(to);
            Wait::parse(&words[0], arg.as_deref())?;
            json!({ "do": "wait", "what": words[0], "arg": arg, "for_ms": patience })
        }
        "state" if words.is_empty() => json!({ "do": "state" }),
        "mark" | "chapter" if !words.is_empty() => json!({ "do": verb, "text": words.join(" ") }),
        _ => return Err(USAGE.into()),
    };
    Ok((req, via))
}

/// The way in's socket: `via` (a socket, or a take's folder whose `way-in` names it), else `$TARAE_RECORD`.
pub(crate) fn way_in(via: Option<&Path>) -> Result<PathBuf> {
    let p = match via {
        Some(p) => p.to_path_buf(),
        None => std::env::var_os(ENV)
            .map(PathBuf::from)
            .ok_or(format!("no way in: --via SOCKET|TAKE, or ${ENV} (record says it as it starts)"))?,
    };
    match p.is_dir() {
        true => {
            let f = p.join(WAY_IN);
            let text = std::fs::read_to_string(&f)
                .map_err(|e| format!("{}: {e} — no take running there?", f.display()))?;
            Ok(PathBuf::from(text.trim()))
        }
        false => Ok(p),
    }
}

/// One request through the way in at `sock`: its exit and its line.
pub(crate) fn ask(sock: &Path, req: &Value) -> Result<(i32, String)> {
    let s = tarae_sock::connect(sock)
        .map_err(|e| format!("the way in, {}: {e} — is the take running?", sock.display()))?;
    writeln!(&mut &s, "{req}").map_err(|e| format!("the way in: {e}"))?;
    let mut line = String::new();
    BufReader::new(&s).read_line(&mut line).map_err(|e| format!("the way in: {e}"))?;
    if line.is_empty() {
        return Ok((code::ERROR, "the take ended: the way in closed before it answered".into()));
    }
    let v: Value = serde_json::from_str(&line).map_err(|e| format!("the way in's answer {line:?}: {e}"))?;
    Ok((
        v["code"].as_i64().unwrap_or(code::ERROR as i64) as i32,
        v["said"].as_str().unwrap_or_default().to_string(),
    ))
}
