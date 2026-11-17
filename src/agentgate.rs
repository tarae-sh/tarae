//! Guard 1's folder (agents beside Claude, step 3): an agent whose own tools tarae turns off (its row's `Recipe`) is
//! started on a config folder of tarae's own, `$state/agent-homes/<row>/<key>/`, made and checked before every start,
//! on a worker, and its two files written there again — the hook file and the program it names (`agent::hook`), the
//! same bytes from every tarae and every start (the design session's condition 2).
//!
//! - The folder holds the agent's conversations, so it outlives a session; it's in tarae's state, which no run and no
//!   `read` of Claude's reaches (`sandbox::taraes_own`), and never the user's own `~/.gemini`.
//! - One folder a project (step 4's L-B6): `<key>` is the project's — its repository's, which its checkouts share, else
//!   the folder's real path (`home_key`) — so nothing the agent remembers (its summaries, read back at every start)
//!   crosses projects.
//! - `$state`, `agent-homes/`, `<row>/`, `<key>/` and its `config/` are each this user's, a real folder (never a link),
//!   and nobody else's to write (condition 4) — `agent-homes/` and below 0700, as made. One that isn't ends the start,
//!   said; nothing is repaired in place.
//! - Each file goes to a new name in `config/` and is renamed over its own: never seen half written, and a link
//!   put in its place is replaced, never written through. One that was there and differs is said once.
//! - The rest of the folder is the agent's (its conversations, its own `config/` files): tarae doesn't remove what
//!   it wrote (Q3). But a server of `config/` named as tarae's would pass the hook as tarae's tools, so the agent
//!   isn't started while one is there, said (condition 3).
//!
//! The chat's start waits for it (`Gate`, after `claudetrust::Gate`): its result, on the main loop, starts the agent
//! (`chat::gate_opened`) or says why not. The same job reads the project's files the row holds until trusted (guard 4,
//! `agenttrust.rs`): the start waits for both, and for the card's answer when those files ask one.
//!
//! While the agent runs, one watch (`running`, an `fswatch` thread as `claudetrust::watch`) looks at tarae's two files
//! and the project's held files whenever they may have changed, and at once as it starts — so a change made between
//! the gate's read and the agent loading the files is seen too. It judges bytes, never events: every start of every
//! tarae renames the same bytes over the two files, and that's no change (two taraes never stop each other's agents).
//! - tarae's files not the bytes and mode tarae writes (`ours`), or not a plain file: the agent is stopped, said.
//! - The held files' digest not the one the start was trusted for (a file changed, added — a `git pull` — or gone),
//!   or past what's ever trusted: stopped, said; for another digest its card asks again as its end lands (`ended`).
//! - The stop is the tripwire's (`acp::Wire::stop`): its group ended at once, its turn ended with the words, nothing it
//!   says after read. What's sent meanwhile waits for the next start.
//! - The window that stays: from a write until the watch has read it (a burst settles: ≥ 50 ms, at most about 0.5 s).

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::agent::Row;
use crate::editor::Editor;
use crate::trust::Digest;

/// Where the agents' folders are, in tarae's state folder.
pub const HOMES: &str = "agent-homes";

/// The most of a file of the agent's own `config/` read for its servers.
pub const READ_MAX: u64 = 1 << 20;

/// An agent's folder made and its files written, for one start: only `prepare` makes one.
#[derive(Debug)]
pub struct Prepared {
    folder: PathBuf,
    program: PathBuf,
}

impl Prepared {
    /// The folder, absolute: what its row's `folder_arg` names.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// tarae's own path as the hook asks it (`hook::ENV` in the agent's environment).
    pub fn program(&self) -> &Path {
        &self.program
    }
}

/// What `prepare` made: the folder, and each file of tarae's that was there and differed (its path from the folder,
/// its size) — written again.
#[derive(Debug)]
pub struct Made {
    pub prepared: Prepared,
    pub changed: Vec<(String, u64)>,
}

/// `row`'s folder under `state` (`$state`) for the project at `root` made and checked, its own `config/` looked
/// through, and its files written. Err: why it isn't started, in the chat's words. It reads the disk (the project's
/// git files too, never waited on): a worker's.
pub fn prepare(row: &Row, state: Option<&Path>, root: &Path) -> Result<Made, String> {
    let name = row.name;
    let off = |why: String| format!("{name}: its own tools can't be turned off — {why}");
    let Some(recipe) = row.own_tools else { return Err(off("its row has no recipe for it".into())) };
    let state = state.ok_or_else(|| off("no state folder (neither XDG_STATE_HOME nor HOME)".into()))?;
    let say = |what: &str, p: &Path, e: std::io::Error| off(format!("{what} {}: {e}", p.display()));
    std::fs::create_dir_all(state).map_err(|e| say("can't make", state, e))?;
    alone(name, state, 0o022)?;
    let folder = state.join(HOMES).join(name).join(home_key(root));
    let hook_file = folder.join(recipe.hook_file);
    let config = hook_file.parent().unwrap_or(&folder).to_path_buf();
    let mut dir = state.to_path_buf();
    for part in config.strip_prefix(state).unwrap_or(Path::new("")).components() {
        use std::os::unix::fs::DirBuilderExt;
        dir.push(part);
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => {
                return Err(say("can't make", &dir, e));
            }
            _ => {}
        }
        alone(name, &dir, 0o077)?;
    }
    let files = ours(&recipe, &folder).map_err(|script| off(format!("{} isn't text", script.display())))?;
    let paths = [files[0].0.as_path(), files[1].0.as_path()];
    servers_named_as_taraes(name, &folder, &config, &paths)?;
    let mut changed = Vec::new();
    for (path, bytes, mode) in &files {
        let rel = path.strip_prefix(&folder).unwrap_or(path).display().to_string();
        match put(path, bytes, *mode) {
            Ok(Some(size)) => changed.push((rel, size)),
            Ok(None) => {}
            Err(e) => return Err(off(format!("can't write {rel}: {e}"))),
        }
    }
    let program = program(state).map_err(|e| off(format!("can't tell where this tarae is: {e}")))?;
    Ok(Made { prepared: Prepared { folder, program }, changed })
}

/// The name of the project's folder under its row's (L-B6): the first 16 hex of the SHA-256 of its place's key
/// (`trust::key`) — a fixed length that says no path. The place is the trust's (`trust::place_of`'s walk): the
/// repository's git common folder, which its checkouts share, for every folder in it; with no repository, the
/// folder's real path, so a link to it gets the same folder. Walked from the folder itself, not a file in it: no name
/// a project puts in its root is followed.
fn home_key(root: &Path) -> String {
    use crate::trust::Place;
    let real = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let checkout = real.ancestors().find(|a| a.join(".git").exists());
    let place = match checkout.and_then(crate::allow::common_dir) {
        Some(common) => Place::Repo { common, file: PathBuf::new() },
        None => Place::File(real),
    };
    crate::sha256::hex(crate::trust::key(&place).as_bytes())[..16].to_string()
}

/// tarae's two files in the agent's `folder` for `recipe`: the program, then the hook file naming it, each with the
/// bytes and mode tarae writes — the same from every tarae and every start. Err: the program's path, which isn't text.
fn ours(recipe: &crate::agent::Recipe, folder: &Path) -> Result<[(PathBuf, Vec<u8>, u32); 2], PathBuf> {
    let hook_file = folder.join(recipe.hook_file);
    let script = hook_file.parent().unwrap_or(folder).join(crate::agent::hook::SCRIPT_NAME);
    let Some(text) = script.to_str() else { return Err(script) };
    let file = crate::agent::hook::file(recipe.hook, text).into_bytes();
    let program = crate::agent::hook::SCRIPT.as_bytes().to_vec();
    Ok([(script, program, 0o700), (hook_file, file, 0o600)])
}

/// Err unless `dir` is this user's, a real folder (not a link), with none of `mask`'s mode bits.
fn alone(name: &str, dir: &Path, mask: u32) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid only reads.
    let me = unsafe { libc::geteuid() };
    let ok =
        std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && m.uid() == me && m.mode() & mask == 0);
    match ok {
        true => Ok(()),
        false => Err(format!("{name}: {} isn't yours alone — not started", dir.display())),
    }
}

/// `bytes` written to `path` with `mode`: to a new name beside it, then renamed over it. Some: what was there
/// differed (its size; a link, a folder or another file's mode are a difference too).
fn put(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<Option<u64>> {
    use std::io::{Read, Write};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let before = match std::fs::symlink_metadata(path) {
        Err(_) => None,
        Ok(m) if !m.is_file() || m.mode() & 0o7777 != mode => Some(m.len()),
        Ok(m) => {
            let mut was = Vec::new();
            let read =
                std::fs::File::open(path).and_then(|f| f.take(bytes.len() as u64 + 1).read_to_end(&mut was));
            (read.is_err() || was != bytes).then_some(m.len())
        }
    };
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temp = path.with_file_name(format!(".{name}.{}-{n}.tarae", std::process::id()));
    let written = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(&temp)?;
        f.write_all(bytes)?;
        f.set_permissions(std::fs::Permissions::from_mode(mode))?;
        std::fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written.map(|_| before)
}

/// The agent's own `config/` looked through for a server named as tarae's (condition 3): its tools would be
/// `mcp_tarae_<tool>`, which the hook lets through as tarae's. Each `.json` file there but tarae's own (`ours`), and
/// any other that reads as JSON, read by key (`taraes_server`); a `.json` file that isn't JSON counts if it holds
/// the word anywhere, and one that can't be read (a link, past `READ_MAX`) ends the start too — at worst a false
/// refusal, said.
fn servers_named_as_taraes(name: &str, folder: &Path, config: &Path, ours: &[&Path]) -> Result<(), String> {
    use std::io::Read;
    let Ok(entries) = std::fs::read_dir(config) else { return Ok(()) };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    crate::sorting::sort(&mut paths);
    for path in paths.iter().filter(|p| !ours.contains(&p.as_path())) {
        let rel = path.strip_prefix(folder).unwrap_or(path).display().to_string();
        let json = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"));
        let unread =
            |why: String| format!("{name}: {rel} can't be read for its MCP servers — {why}; not started");
        let Ok(meta) = std::fs::symlink_metadata(path) else { continue };
        if meta.is_dir() || (!json && !meta.is_file()) {
            continue;
        }
        if !meta.is_file() {
            return Err(unread("not a plain file (a link is never read through)".into()));
        }
        let mut text = Vec::new();
        let read = std::fs::File::open(path).and_then(|f| f.take(READ_MAX + 1).read_to_end(&mut text));
        match read {
            Err(e) if json => return Err(unread(e.to_string())),
            Ok(n) if json && n as u64 > READ_MAX => return Err(unread("too big to read".into())),
            Err(_) => continue,
            Ok(_) => {}
        }
        if let Some(server) = named_in(json, &text) {
            return Err(format!(
                "{name}: {rel} names an MCP server \"{server}\" — its tools would pass for tarae's; not started"
            ));
        }
    }
    Ok(())
}

/// The server `text` names as tarae's: by key where it reads as JSON (`taraes_server`); a `.json` file (`json`) that
/// doesn't, by the word anywhere in it — at worst a false refusal.
pub fn named_in(json: bool, text: &[u8]) -> Option<String> {
    match serde_json::from_slice::<Value>(text) {
        Ok(v) => taraes_server(&v),
        Err(_) if json => {
            String::from_utf8_lossy(text).to_lowercase().contains("tarae").then(|| "tarae".into())
        }
        Err(_) => None,
    }
}

/// A server's name that would pass for tarae's: `tarae`, any case, alone or before a mark that isn't a letter or a
/// digit (`tarae_x`'s tools are `mcp_tarae_x_<tool>`).
fn as_taraes(server: &str) -> bool {
    let lower = server.to_lowercase();
    lower.strip_prefix("tarae").is_some_and(|rest| rest.chars().next().is_none_or(|c| !c.is_alphanumeric()))
}

/// The first server `v` names as tarae's: under any key that says `mcp` or `server` (any case), an object's keys or
/// an array's `name`s, wherever it is.
pub fn taraes_server(v: &Value) -> Option<String> {
    match v {
        Value::Object(m) => m.iter().find_map(|(k, v)| {
            let lower = k.to_lowercase();
            let names: Vec<&str> = match v {
                _ if !lower.contains("mcp") && !lower.contains("server") => Vec::new(),
                Value::Object(servers) => servers.keys().map(String::as_str).collect(),
                Value::Array(servers) => servers.iter().filter_map(|s| s["name"].as_str()).collect(),
                _ => Vec::new(),
            };
            names.into_iter().find(|n| as_taraes(n)).map(String::from).or_else(|| taraes_server(v))
        }),
        Value::Array(a) => a.iter().find_map(taraes_server),
        _ => None,
    }
}

/// tarae's own path as the hook asks it (`hook::ENV`): the binary as it was started (`restart::exe` — once a build
/// replaces it, Linux names the running one "… (deleted)", which no hook could run). A test build's: a program in its
/// state folder that re-runs the test binary on `hook::child`, answering on descriptor 3 as the stand-in and the
/// bridge's relay do.
fn program(state: &Path) -> std::io::Result<PathBuf> {
    if !cfg!(test) {
        let unknown = || std::io::Error::new(std::io::ErrorKind::NotFound, "can't tell where this tarae is");
        return crate::restart::exe().ok_or_else(unknown);
    }
    let exe = std::env::current_exe()?;
    let path = state.join("agent-hook-test");
    let exe = crate::packs::sh_quote(&exe.to_string_lossy());
    let text = format!(
        "#!/bin/sh\nexec {exe} --exact agent::hook::child --ignored --nocapture \"$@\" 3>&1 >/dev/null\n"
    );
    put(&path, text.as_bytes(), 0o700)?;
    Ok(path)
}

// ── The chat's gate ──────────────────────────────────────────────────────────────

/// A chat's gate for an agent whose own tools tarae turns off: it starts once its folder is made (`prepare`) and the
/// project's files it holds are trusted (`agenttrust`).
pub struct Gate {
    /// The folder being made and the files read: since when (the chat says it's starting meanwhile).
    reading: Option<std::time::Instant>,
    /// Made, for the next start — taken by it: each start makes it again.
    prepared: Option<Prepared>,
    /// This gate's: a closed chat's late result isn't another chat's.
    pub(crate) id: u64,
    /// The project's held files as last read, and the answer in hand for them (the store may not have it yet).
    pub(crate) files: Option<crate::agenttrust::Seen>,
    /// Their card up, or its yes being kept: since when — what's sent meanwhile waits for the start.
    pub(crate) asked: Option<std::time::Instant>,
    /// Its running agent's watch (`running`).
    watch: Option<Watching>,
    /// Its agent stopped by the watch, its end not landed yet (`changed_now`): what's sent meanwhile waits.
    stopping: Option<Stopping>,
    /// A simulation's starts: staged handles in place of processes (`sim/acp.rs`); None: real ones.
    #[cfg(test)]
    pub(crate) stage: Option<Stage>,
}

/// A running agent's watch: what it compares, and its thread's waker (None: a staged start's, no thread).
struct Watching {
    /// (a staged start's scenario looks again with it — `look_again`)
    #[cfg_attr(not(test), allow(dead_code))]
    watched: Watched,
    waker: Option<crate::fswatch::Waker>,
}

/// An agent the watch stopped: its words, whether the held files changed (its gate opens again as its end lands, so
/// their card asks), and since when.
struct Stopping {
    said: String,
    again: bool,
    since: std::time::Instant,
}

/// A simulation's starts (`chat::ensure_proc`): each takes the next handle, counted, its process's generation kept.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Stage {
    pub agents: std::collections::VecDeque<crate::agent::Agent>,
    pub starts: usize,
    pub generations: Vec<u64>,
}

impl Default for Gate {
    fn default() -> Gate {
        static IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Gate {
            reading: None,
            prepared: None,
            id,
            files: None,
            asked: None,
            watch: None,
            stopping: None,
            #[cfg(test)]
            stage: None,
        }
    }
}

impl Drop for Gate {
    /// The chat gone: its agent's watch ends.
    fn drop(&mut self) {
        if let Some(w) = self.watch.take().and_then(|w| w.waker) {
            w.wake();
        }
    }
}

impl Gate {
    /// Since when its agent has waited for the folder and the files, or for their card's answer, or for its stop to
    /// land.
    pub fn waiting_since(&self) -> Option<std::time::Instant> {
        self.reading.or(self.asked).or(self.stopping.as_ref().map(|s| s.since))
    }

    /// Its agent is being stopped by the watch: nothing is sent to it.
    pub(crate) fn stopping(&self) -> bool {
        self.stopping.is_some()
    }
}

/// The chat's agent may start (`chat::ensure_proc`): its folder made and the project's files it holds trusted — None
/// while that's done on a worker (it starts once it is, `made_now`) or their card waits.
pub fn ready(ed: &mut Editor) -> Option<Prepared> {
    let state = crate::agent::state_of(ed.claude_env.as_ref());
    let (root, store) = (ed.workspace.root.clone(), ed.trust_path.clone());
    let c = ed.workspace.claude.chat.as_mut()?;
    if let Some(p) = c.agent_gate.prepared.take() {
        return Some(p);
    }
    if c.agent_gate.reading.is_none() && c.agent_gate.asked.is_none() {
        c.agent_gate.reading = Some(crate::clock::now());
        let (id, row) = (c.agent_gate.id, c.agent);
        ed.events.jobs().spawn(move || {
            let (looked, since, carried) = crate::agenttrust::look_since(row, &root, store.as_deref());
            let made = prepare(row, state.as_deref(), &root);
            move |ed: &mut Editor| {
                ed.trust_carried(carried);
                made_now(ed, id, made, looked, since)
            }
        });
    }
    None
}

/// The folder made and the files read: what differed said, and the agent started (`chat::gate_opened`) where its files
/// are trusted — else their card up, or why not said, nothing started (a message that waited stays, for the start).
fn made_now(
    ed: &mut Editor,
    id: u64,
    made: Result<Made, String>,
    looked: Result<crate::agenttrust::Seen, String>,
    (key, at): (String, u64),
) {
    use crate::trust::State;
    let log = ed.workspace.log();
    // (read before a take-back there given since: not trusted, whatever the store said then — F11)
    let taken = ed.trust_path.as_deref().is_some_and(|t| crate::storewrite::since(t, &key, at).1);
    let held = looked.as_ref().is_ok_and(|s| ed.agent_files_held.contains(&s.place));
    let Some(c) = ed.workspace.claude.chat.as_mut().filter(|c| c.agent_gate.id == id) else { return };
    c.agent_gate.reading = None;
    let name = c.agent.name;
    let refused = |ed: &mut Editor, said: &str| {
        log.line("agent", "warn", said);
        let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
        c.note(said);
        let at = c.msgs.len() - 1;
        crate::chat::said_here(ed, at);
    };
    let prepared = match made {
        Ok(Made { prepared, changed }) => {
            for (rel, size) in changed {
                log.line(
                    "agent",
                    "warn",
                    &format!("{name}'s {rel} was changed outside tarae ({size} bytes)"),
                );
                c.note(&format!("{name}'s {rel} was changed outside tarae — written again"));
            }
            prepared
        }
        Err(said) => return refused(ed, &said),
    };
    let mut seen = match looked {
        Ok(seen) => seen,
        Err(said) => return refused(ed, &said),
    };
    // (the answer in hand: a yes the store may not have yet — for these very files)
    let in_hand = c.agent_gate.files.as_ref().is_some_and(|s| s.hash == seen.hash && s.place == seen.place);
    if in_hand && c.agent_gate.files.as_ref().is_some_and(|s| s.state == State::Trusted) {
        seen.state = State::Trusted;
    }
    if taken {
        seen.state = State::Ask;
    }
    if seen.files.is_empty() || seen.state == State::Trusted {
        c.agent_gate.files = Some(seen);
        c.agent_gate.prepared = Some(prepared);
        return crate::chat::gate_opened(ed);
    }
    if held {
        seen.state = State::Ignored;
        c.agent_gate.files = Some(seen);
        return refused(ed, &format!("Held back — {name} isn't started in this project until you quit"));
    }
    seen.state = State::Ask;
    c.agent_gate.asked = Some(crate::clock::now());
    c.note(&format!("{name} waits for your answer on the card — your message goes once it starts"));
    let card = crate::agenttrust::card(name, &seen);
    c.agent_gate.files = Some(seen);
    ed.trust_offer(card);
}

/// `:granted`'s `d` on an agent's files at `place`: an answer in hand there asks again (the store's is being taken
/// back) — in every session.
pub(crate) fn forgotten(ed: &mut Editor, place: &crate::trust::Place) {
    let workspaces = std::iter::once(&mut ed.workspace).chain(ed.workspaces.iter_mut());
    for c in workspaces.filter_map(|w| w.claude.chat.as_mut()) {
        if let Some(s) = c.agent_gate.files.as_mut().filter(|s| s.place == *place) {
            s.state = crate::trust::State::Ask;
        }
    }
}

// ── The watch while it runs ──────────────────────────────────────────────────────

/// What a running agent's watch compares, and against what: tarae's two files in its folder against what tarae writes
/// (`ours`), the project's held files against the digest its start was trusted for.
#[derive(Clone)]
pub(crate) struct Watched {
    row: &'static Row,
    root: PathBuf,
    folder: PathBuf,
    ours: Vec<(PathBuf, Vec<u8>, u32)>,
    /// None: no digest at its start — any look is a change.
    hash: Option<Digest>,
    /// The chat's process it watches (`Chat.generation`): a later one isn't stopped by it.
    generation: u64,
}

/// What the watch found.
#[derive(Debug, PartialEq)]
pub(crate) enum Change {
    /// One of tarae's files isn't what tarae writes (its bytes, its mode, or not a plain file): its path from the folder.
    Ours(String),
    /// The held files aren't what the start was trusted for: another digest, or (Err) past what's ever trusted — why,
    /// in `agenttrust::look`'s words.
    Held(Result<Digest, String>),
}

/// What's changed since the start, judged by bytes: tarae's files first, then the held files. Reads the disk.
fn changed(w: &Watched) -> Option<Change> {
    for (path, bytes, mode) in &w.ours {
        if !holds(path, bytes, *mode) {
            return Some(Change::Ours(path.strip_prefix(&w.folder).unwrap_or(path).display().to_string()));
        }
    }
    match crate::agenttrust::look(w.row, &w.root, None) {
        Ok(seen) if Some(seen.hash) == w.hash => None,
        Ok(seen) => Some(Change::Held(Ok(seen.hash))),
        Err(why) => Some(Change::Held(Err(why))),
    }
}

/// `path` is a plain file with `mode` holding `bytes` — never read through a link, nor held by a FIFO.
fn holds(path: &Path, bytes: &[u8], mode: u32) -> bool {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let plain = std::fs::symlink_metadata(path)
        .is_ok_and(|m| m.is_file() && m.mode() & 0o7777 == mode && m.len() == bytes.len() as u64);
    let mut was = Vec::new();
    let opened = || {
        std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path)
    };
    plain
        && opened().and_then(|f| f.take(bytes.len() as u64 + 1).read_to_end(&mut was)).is_ok()
        && was == bytes
}

/// `w`'s files and folders watched: tarae's two, each held path in every folder from the root up to its git top
/// (`agenttrust::chain`; made, replaced or gone — not there yet: the closest folder above it), and every folder under
/// one, as deep as `agenttrust::look` reads, for its files added, removed or written. And the top's `.git` (none: the
/// root's), made, replaced, gone or written in place: the chain may have changed (the design session's note 2 on
/// L-B2), and a look keys the held files from the top, so a chain that holds more or less is another digest.
fn arm(watcher: &mut crate::fswatch::Watcher, w: &Watched) {
    use crate::agenttrust::{DEPTH_MAX, FILES_MAX};
    w.ours.iter().for_each(|(p, ..)| watcher.file(p));
    let mut folders = 0;
    let chain = crate::agenttrust::chain(&w.root);
    watcher.file(&chain.top.join(".git"));
    for h in chain.held(w.row.held) {
        let top = chain.top.join(h);
        watcher.file(&top);
        let mut next = vec![(top, 0)];
        while let Some((dir, depth)) = next.pop() {
            if folders >= FILES_MAX || !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
                continue;
            }
            watcher.folder(&dir);
            folders += 1;
            if depth < DEPTH_MAX {
                let entries = std::fs::read_dir(&dir).into_iter().flatten().flatten();
                next.extend(entries.map(|e| (e.path(), depth + 1)));
            }
        }
    }
}

/// The chat's agent started on `prepared` (`chat::ensure_proc`, a start its gate opened): watched while it runs, from
/// now — on a thread of its own (a simulation's staged start has none: its scenario takes the rounds, `look_again`).
pub(crate) fn running(ed: &mut Editor, prepared: &Prepared) {
    let (root, tx) = (ed.workspace.root.clone(), ed.events.sender());
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    let (row, folder) = (c.agent, prepared.folder.clone());
    // (`prepare` made these very files: its row has a recipe and their paths are text)
    let Some(ours) = row.own_tools.and_then(|r| ours(&r, &folder).ok()) else { return };
    let hash = c.agent_gate.files.as_ref().map(|s| s.hash);
    let w = Watched { row, root, folder, ours: ours.into(), hash, generation: c.generation() };
    #[cfg(test)]
    let staged = c.agent_gate.stage.is_some();
    #[cfg(not(test))]
    let staged = false;
    let waker = (!staged).then(crate::fswatch::Waker::new);
    let old = c.agent_gate.watch.replace(Watching { watched: w.clone(), waker: waker.clone() });
    if let Some(old) = old.and_then(|o| o.waker) {
        old.wake();
    }
    let Some(waker) = waker else { return };
    crate::event::thread("tarae-agentwch", move || {
        loop {
            // Armed before looking, so a change made while we look still ends the wait below
            let mut watcher = crate::fswatch::Watcher::with_waker(waker.clone());
            arm(&mut watcher, &w);
            if let Some(change) = changed(&w) {
                let generation = w.generation;
                let apply = move |ed: &mut Editor| changed_now(ed, generation, change);
                let _ = tx.send(crate::event::Event::Job(Box::new(apply)));
                break;
            }
            if watcher.wait() == crate::fswatch::Woke::Cancelled {
                break;
            }
        }
    });
}

/// The watch's round as a job, for a staged start's scenario (`sim/acp.rs`): what it finds applied as the thread's
/// is. Whether a watch was there to look.
#[cfg(test)]
pub(crate) fn look_again(ed: &mut Editor) -> bool {
    let c = ed.workspace.claude.chat.as_ref();
    let Some(w) = c.and_then(|c| c.agent_gate.watch.as_ref()).map(|w| w.watched.clone()) else {
        return false;
    };
    ed.events.jobs().spawn(move || {
        let change = changed(&w);
        move |ed: &mut Editor| match change {
            Some(change) => changed_now(ed, w.generation, change),
            None => ed.unchanged = true,
        }
    });
    true
}

/// The watch found `change` while process `generation` ran: it's stopped as the tripwire stops one (`Agent::stop_with`:
/// its group ended at once, its turn ended with the words), said in the log and a toast; the transcript says it once its
/// end lands (`ended`). A later process, one ended or one already being stopped: nothing.
fn changed_now(ed: &mut Editor, generation: u64, change: Change) {
    let log = ed.workspace.log();
    let chat = ed.workspace.claude.chat.as_mut();
    let ours =
        |c: &crate::chat::Chat| c.generation() == generation && c.running() && !c.agent_gate.stopping();
    let Some(c) = chat.filter(|c| ours(c)) else { return };
    let name = c.agent.name;
    let ended = "stopped: it and everything it started ended";
    let (said, ours, again) = match change {
        Change::Ours(rel) => (
            format!(
                "{name}'s {rel} changed while it ran — {ended}. tarae writes it again at its next start."
            ),
            Some(rel),
            false,
        ),
        Change::Held(Ok(_)) => (
            format!("{name}'s files in this project changed while it ran — {ended}. Their card asks again."),
            None,
            true,
        ),
        Change::Held(Err(why)) => {
            (format!("{name}'s files in this project changed while it ran — {ended}. {why}"), None, false)
        }
    };
    log.line("agent", "warn", &said);
    let since = crate::clock::now();
    c.agent_gate.stopping = Some(Stopping { said: said.clone(), again, since });
    if !c.agent().is_some_and(|a| a.stop_with(said)) {
        c.stop_process();
    }
    match ours {
        Some(rel) => ed.set_error(format!("{name} stopped — its {rel} changed")),
        None => ed.set_error(format!("{name} stopped — its files in this project changed")),
    }
}

/// The chat's agent ended (`chat`'s `Exit`): its watch with it. Stopped by the watch: its words said once in the
/// transcript (the turn's end said them, if one ran), the conversation to go on at the next start. Whether its gate
/// opens again now: the held files changed (their card asks again), or a message waits for the start.
pub(crate) fn ended(ed: &mut Editor) -> bool {
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return false };
    if let Some(w) = c.agent_gate.watch.take().and_then(|w| w.waker) {
        w.wake();
    }
    let Some(stop) = c.agent_gate.stopping.take() else { return false };
    if !c.msgs.iter().any(|m| m.text == stop.said) {
        c.note(&stop.said);
    }
    if let Some(id) = c.conversation.clone() {
        c.resume = Some(id);
    }
    stop.again || c.queued.is_some()
}

#[cfg(test)]
mod tests;
