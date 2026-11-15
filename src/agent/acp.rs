//! The Agent Client Protocol's wire: JSON-RPC 2.0 over the agent's stdin and stdout, a message a line — protocol
//! version 1, as agentclientprotocol.com/protocol read on 2026-11-14 (initialization, session setup, the prompt
//! turn, errors). What tarae writes and reads, and what each becomes, is step 2's plan §2; what it drops is said
//! where it's dropped. Each message the real agy sent is read in agy's shape (the design session's probe, agy 1.5.4).
//!
//! One thread holds the conversation (`run`): the handshake (`initialize`, then `session/new`, `resume` or `load`),
//! the prompts held until the session is known and sent one turn at a time, the permission asks still open, the
//! cancel. The handle (`Proc`) only hands it what the main loop says; stdin is written on a thread of its own and
//! stdout read on another, so nothing here waits on the main loop.
//!
//! tarae's tools reach it through the bridge (`bridge.rs`): an MCP server offered in `session/new`, a relay whose
//! messages come up the same thread as stdout's, so a tool's call is told once and in order (finding C). Its process
//! leads a session of its own, with no terminal, and so a group of its own, which ends with it — the relay and
//! whatever else it started (condition 4).
//!
//! Step 3's guards that live on this thread: the row's mode kept (guard 2 — set at the start, set back if it moves,
//! the start or the session ended where it won't be), and the tripwire (guard 3): one of its own tools reported
//! `completed`, or one of tarae's the bridge never carried, stops it at once, its group ended (`Wire::trip`). This
//! thread sees the agent's reports and the bridge's calls in one order, so the match needs no clock.
//!
//! Test-only in step 2 (the design session's answer, §9.4): no config reaches it, and a release build carries none
//! of it.

pub mod bridge;
pub mod standin;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, PipeWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};

use serde_json::{Value, json};

use super::{
    Agent, Args, Caps, Ended, Event, Exit, First, Permission, Place, Prompt, SpawnError, Start, Started,
};
use crate::event::Apply;
use crate::llm::Env;
use crate::spawn::Spawned;

/// The protocol version tarae speaks.
pub const VERSION: u64 = 1;

/// What an ACP agent can do: its turn stopped (`session/cancel`), a conversation continued (`session/resume`,
/// else `session/load` — the wire starts a new one where neither is there, said), its thinking as text. Not asked
/// of it: a fork, per-request usage (a turn's `usage` only), a cost, the plan's limits, models and effort (step
/// 4), a compaction on demand, who pays (ACP has no word for it), its permission asks read by tool name (an ask's
/// name is the agent's own, its title the model's text: every ask refused, condition 1), a helper of Claude's for
/// its chat's tools (`web_search`'s: a session's every model is its agent's, condition 3). Its own tools off: by its
/// row's recipe, which a row on these caps must have (`every_acp_row_keeps_the_floor`) — tarae's hook refuses them
/// (guard 1, `agentgate.rs`).
pub const CAPS: Caps = Caps {
    interrupt: true,
    resume: true,
    fork: false,
    thinking_text: true,
    tokens_per_request: false,
    cost: false,
    limits: false,
    models: false,
    effort: false,
    compaction: false,
    own_tools_off: true,
    billing: false,
    asks: false,
    helpers: false,
};

/// The prefix of tarae's own tools as agy names them in its `tool_call`s (`mcp_<server>_<tool>`): the bridge tells
/// those (landing 4), so the agent's report of one is dropped, and its updates with it, by id.
pub const OWN: &str = "mcp_tarae_";

/// An own tool's call that failed (its hook refused it, guard 1), as its row's result says it.
pub const REFUSED: &str = "refused — tarae's tools only";

/// The tool `name` is of tarae's (`mcp_tarae_<tool>`, something after the prefix, as the hook allows it), or None: one
/// of the agent's own — an empty name, and the prefix alone, included.
fn taraes(name: &str) -> Option<&str> {
    name.strip_prefix(OWN).filter(|tool| !tool.is_empty())
}

// ── Its environment ─────────────────────────────────────────────────────

/// tarae's own sockets, named in its environment: the UI socket (`TARAE_UI`, the terminal panel's and your tasks'),
/// a sandboxed run's pty broker (`pty::BROKER`), and your SSH agent's (as no claude gets it, threat model G20).
pub const SOCKETS: &[&str] = &["TARAE_UI", crate::pty::BROKER, "SSH_AUTH_SOCK"];

/// Whether an agent that isn't Claude never gets `name` from tarae's environment (condition 2): Claude Code's keys
/// and settings (`ANTHROPIC_*`, `CLAUDE*` — `llm::claudes_own` —, and what a Claude Code session sets for what it
/// starts — `llm::judged`'s, `AI_AGENT` and its trace context among them), the wikis' tokens (`web.signed-in`'s
/// `token-env`), and tarae's own sockets (`SOCKETS`). The rest of tarae's environment is its, as it would be any
/// program's: removed by name, never cleared.
pub fn left_out(name: &str, signed_in: &[crate::web::signed::Wiki]) -> bool {
    crate::llm::claudes_own(name)
        || crate::llm::judged(name)
        || SOCKETS.contains(&name)
        || signed_in.iter().any(|w| w.token_env == name)
}

/// `s`'s arguments: its row's, as they are (no setting reaches them), then its folder's where its row turns its own
/// tools off (`folder_arg <folder>`), none after.
pub fn args(s: &Start) -> Args {
    let folder = folder(s).map(|(arg, f)| [arg.to_string(), f.to_string_lossy().into_owned()]);
    let args = s.row.args.iter().map(|a| a.to_string()).chain(folder.into_iter().flatten()).collect();
    Args { args, extra: Vec::new() }
}

/// The argument naming `s`'s folder, and the folder (`agentgate::prepare`'s), where its row has a recipe.
fn folder<'a>(s: &Start<'a>) -> Option<(&'static str, &'a Path)> {
    let recipe = s.row.own_tools.as_ref()?;
    Some((recipe.folder_arg, s.prepared?.folder()))
}

/// `s`'s process, not started: its row's program on its row's arguments, run in `dir`, with tarae's environment
/// less what `left_out` says (`given`: a test's stand-in for it, laid over the process's). A plain `Command`: `spawn`
/// starts it, and the shim for the descriptor limit is that start's. A row that turns its own tools off is refused
/// without its folder made (`Start.prepared`): no start goes around guard 1. Its folder after its arguments, and
/// tarae's own path for its hook to ask (`hook::ENV`).
pub fn command(s: &Start, dir: &Path, given: Option<&Env>) -> Result<Command, SpawnError> {
    let program = s.row.command;
    // (tests never start a real agent — the stand-in answers for one; nor its help)
    #[cfg(test)]
    {
        let name = Path::new(program).file_name().map(|n| n.to_string_lossy().into_owned());
        assert!(
            name.is_none_or(|n| !["agy", "gemini", "claude"].contains(&n.as_str())),
            "a test started a real agent ({program}) — give it the stand-in's row (`agent::STAND_IN`)"
        );
    }
    let mut cmd = Command::new(program);
    cmd.current_dir(dir);
    for a in s.row.args {
        // (the stand-in's: the test binary itself, re-run on its one test)
        #[cfg(test)]
        if *a == standin::SELF {
            cmd.arg(std::env::current_exe().map_err(SpawnError::Start)?);
            continue;
        }
        cmd.arg(a);
    }
    if s.row.own_tools.is_some() && s.prepared.is_none() {
        return Err(SpawnError::Args(format!("{}: not started without tarae's folder", s.row.name)));
    }
    if let Some((arg, folder)) = folder(s) {
        cmd.arg(arg).arg(folder);
    }
    let mut names = crate::llm::process_names();
    if let Some(given) = given {
        cmd.envs(given);
        names.extend(given.keys().cloned());
    }
    for name in names.iter().filter(|n| left_out(n, s.signed_in)) {
        cmd.env_remove(name);
    }
    if let Some(p) = s.prepared {
        cmd.env(super::hook::ENV, p.program());
    }
    Ok(cmd)
}

/// `s`'s process started (`command`) in a session of its own (`spawn::setsid_fed_as`): its own process group, which
/// `run` ends with it (step 2's condition 4: the session leader's pgid is its pid), and no controlling terminal — it
/// and what it starts can't open `/dev/tty`, nor are reached by a signal to tarae's group. Its stdin a pipe, stdout
/// and stderr pipes. It gets the descriptor limit tarae started with, as every program of the user's does (the
/// shim, `spawn::Limit::Original`) — not Claude's raised one, which was measured for claude alone. `Acts::User`:
/// what acts for Claude or a plugin gets the filter that keeps it from new user namespaces, for runs; the agent's
/// own process isn't a run (it runs as you, unsandboxed: the user's decision) — as Claude's own process, which
/// `spawn::setsid_fed` starts with no filter, isn't.
pub fn spawn(s: &Start, dir: &Path, given: Option<&Env>) -> Result<(Spawned, PipeWriter), SpawnError> {
    let cmd = command(s, dir, given)?;
    crate::spawn::setsid_fed_as(&cmd, crate::spawn::Acts::User).map_err(SpawnError::Start)
}

// ── The handle ──────────────────────────────────────────────────────────

/// What the main loop hands the conversation's thread.
enum In {
    Line(String),
    Eof,
    Prompt(Prompt),
    Cancel,
    Answer(String, Permission),
    /// The handle dropped: what's queued for stdin still goes, then it closes.
    Closed,
    /// The bridge's connection `n` is the live one: its writes.
    Linked(u64, Sender<String>),
    /// Connection `n` ended.
    Unlinked(u64),
    /// A message of the live connection's: an MCP request or notification.
    Bridged(Value),
    /// tarae's MCP server's answer to bridged message `key` (`Agent::answer_mcp`).
    Mcp(String, Value),
}

/// A running ACP agent, as `Agent::Acp` holds it: each call a message to its conversation's thread, never a wait.
pub struct Proc(Sender<In>);

impl Proc {
    /// Held until the session is known and the turn before it answered. Err: the agent's gone.
    pub(super) fn prompt(&self, p: Prompt) -> std::io::Result<()> {
        self.0.send(In::Prompt(p)).map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }

    /// `session/cancel`, and every ask still open answered `cancelled` (the spec's MUST).
    pub(super) fn stop_turn(&self) -> bool {
        self.0.send(In::Cancel).is_ok()
    }

    pub(super) fn answer_permission(&self, id: &str, p: Permission) {
        let _ = self.0.send(In::Answer(id.to_string(), p));
    }

    /// Back through the bridge, and — a call's — its `ToolResult`.
    pub(super) fn answer_mcp(&self, key: &str, response: Value) {
        let _ = self.0.send(In::Mcp(key.to_string(), response));
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.send(In::Closed);
    }
}

// ── Starting ────────────────────────────────────────────────────────────

/// What the conversation's thread takes from a start.
struct Begin {
    /// The row's name and program: what its words say (`<name> isn't signed in — sign in with `<command>``).
    name: &'static str,
    command: &'static str,
    cwd: PathBuf,
    resume: Option<String>,
    system: Option<String>,
    /// tarae's tools offered (`First.tools`): where its bridge binds (`$state`; None: nowhere — the start ends
    /// saying so), its agent's pid, and the way back to this thread its connections take.
    tools: Option<(Option<PathBuf>, u32, Sender<In>)>,
    /// Its process, whose group a trip ends at once (None: the simulation's, which has none).
    pid: Option<u32>,
    /// The mode its row keeps it in (guard 2, `Row.mode`).
    mode: Option<&'static str>,
}

/// Where a bridge binds, and an agent's folder is (`agentgate`): `$state` (`config::state_dir`) — a test's own, its `given` `XDG_STATE_HOME` (a folder of its
/// scratch's, as the real one is outside the sandbox).
pub(crate) fn state(given: Option<&Env>) -> Option<PathBuf> {
    match cfg!(test) {
        true => given.and_then(|g| g.get("XDG_STATE_HOME")).map(|s| PathBuf::from(s).join("tarae")),
        false => crate::config::state_dir(),
    }
}

/// An ACP agent started (`spawn`: the process and its stdin's writing end): its stdout and stderr read, and its
/// conversation's thread begun — `initialize` its first line, written there. `first`'s prompt goes once the session
/// is known; its `tools`, the bridge, is bound at the handshake and offered in the session's `mcpServers`.
pub(super) fn begin(
    (mut child, stdin): (Spawned, PipeWriter),
    s: &Start,
    at: &Place,
    first: First,
    once: bool,
    tx: crate::event::Tx,
    route: impl Fn(Event) -> Option<Apply> + Send + 'static,
) -> Result<Started, SpawnError> {
    let pid = child.pid;
    let (in_tx, in_rx) = channel::<In>();
    if let Some(p) = first.prompt {
        let _ = in_tx.send(In::Prompt(p));
    }
    let begin = Begin {
        name: s.row.name,
        command: s.row.command,
        cwd: at.dir.to_path_buf(),
        resume: s.resume.as_ref().map(|r| r.conversation.to_string()),
        system: s.system.map(String::from),
        tools: first.tools.then(|| (state(at.given), pid, in_tx.clone())),
        pid: Some(pid),
        mode: s.row.mode,
    };
    match child.stdout.take() {
        Some(out) => {
            let lines = in_tx.clone();
            crate::event::thread("tarae-acp-lines", move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    if lines.send(In::Line(line)).is_err() {
                        return;
                    }
                }
                let _ = lines.send(In::Eof);
            });
        }
        None => {
            let _ = in_tx.send(In::Eof);
        }
    }
    super::process::UNREAPED.lock().unwrap_or_else(|e| e.into_inner()).push(pid);
    let out = writer(stdin);
    crate::event::thread("tarae-acp-read", move || {
        let send = |ev: Event| route(ev).is_none_or(|apply| tx.send(crate::event::Event::Job(apply)).is_ok());
        run(child, Wire::new(begin, out), in_rx, once, &send);
    });
    Ok(Started { agent: Agent::Acp(Proc(in_tx)), pid })
}

/// The agent's stdin, written on a thread of its own, lines in the order sent (a pipe takes 64 KB before a write
/// waits, and a prompt can carry more). The sender dropped: what's queued goes, then stdin closes.
fn writer(mut stdin: PipeWriter) -> Sender<String> {
    let (tx, rx) = channel::<String>();
    crate::event::thread("tarae-acp-write", move || {
        while let Ok(line) = rx.recv() {
            // (a failed write: the process is gone — its stdout's end says so)
            if writeln!(stdin, "{line}").and_then(|_| stdin.flush()).is_err() {
                break;
            }
        }
    });
    tx
}

// ── The conversation ────────────────────────────────────────────────────

/// A request of ours waiting for its answer.
enum Asked {
    Initialize,
    New,
    Resume(String),
    Load(String),
    Prompt,
    /// `session/set_mode` to the row's (guard 2): at the start, the session's id and model held for its answer (the
    /// session known, `Ready` and the first prompt, only then); None: set back mid-session.
    Mode(Option<(String, String)>),
}

/// What the agent reported of one tool call (`tool_call`, then its `tool_call_update`s), for the tripwire (guard 3).
#[derive(Default)]
struct Report {
    /// Its name as `tool_name` reads it ("": an update with no `tool_call` before it — one of its own).
    name: String,
    /// Its arguments as it says them (`input_of`): agy's carry a key the server never gets (`toolSummary`).
    input: Value,
    /// Where it ended: `completed` (true), `failed` (false), or not yet.
    done: Option<bool>,
    /// One of tarae's matched with a call of the bridge's.
    matched: bool,
    /// Its place among the reports, for the order they're matched in.
    seq: u64,
}

/// A call of tarae's tool through the bridge this turn, for a report of it to be matched with.
struct Called {
    tool: String,
    arguments: Value,
    matched: bool,
}

/// What the tripwire makes of one report.
enum Watch {
    /// Nothing yet: tarae's (judged at the turn's end), or one of its own still running.
    Pass,
    /// One of its own failed: its hook refused it (guard 1) — a refused row.
    Refused,
    /// One of its own completed: it ran it — its name, and whether it has no row yet (completed in its first report).
    Trip(String, bool),
}

/// What a bridged message is, for its answer.
#[derive(Clone, Copy, PartialEq)]
enum Bridged {
    /// A `tools/call`: its answer is its `ToolResult` too.
    Call,
    Request,
    /// No answer goes back (`tools::mcp` gives one, as Claude's SDK wants).
    Notification,
}

/// What the conversation's thread keeps.
struct Wire {
    begin: Begin,
    /// To stdin (None: the handle dropped — stdin closes).
    out: Option<Sender<String>>,
    next_id: u64,
    asked: HashMap<u64, Asked>,
    /// What its `initialize` said it can do.
    load: bool,
    resume: bool,
    system_prompt: bool,
    version: Option<String>,
    session: Option<String>,
    /// tarae's words before the first message, as its first text block — once a conversation, where the agent
    /// takes no system prompt (the design session's answer, §9.3): never on a resume or a load, never in the
    /// transcript.
    system_first: Option<String>,
    /// `session/load`'s replay of the conversation, dropped until its answer.
    replaying: bool,
    queue: VecDeque<Prompt>,
    turn: bool,
    /// The turn's answer so far (its `Done`).
    text: String,
    /// The permission asks still open: each its id as `Event::Permission` gives it, its JSON-RPC id and options.
    asks: HashMap<String, (Value, Value)>,
    /// tarae's own tools' calls, whose updates are dropped too.
    dropped: HashSet<String>,
    /// Streamed text or thought not yet sent, and when the last batch went (`claude::FLUSH`'s rule).
    pending: String,
    pending_thought: bool,
    last_flush: std::time::Instant,
    thinking: bool,
    /// The process is to end: signed out, another version, an error at its start, a conversation gone, a mode it
    /// won't keep, a trip.
    end: bool,
    /// How it ended where the wire ended it, in place of stderr's last line: the conversation it was to continue gone
    /// (`Ended::Gone`, its words in the log only), the mode it left (`Exited`), a trip (`Tripped`).
    ended_as: Option<Exit>,
    junk_logged: bool,
    /// Guard 3, the tripwire: the agent's reports of its calls by id (tarae's until their turn's end, its own until
    /// they end), the bridge's calls this turn, and how many reports there have been.
    reports: HashMap<String, Report>,
    calls: Vec<Called>,
    reports_n: u64,
    /// It tripped: nothing after it is read, no prompt written.
    tripped: bool,
    /// tarae's tools' bridge, once bound (dropped: its socket and folder go).
    bridge: Option<bridge::Bridge>,
    /// Its live connection: its number and its writes.
    link: Option<(u64, Sender<String>)>,
    /// Its messages the chat hasn't answered: each by its key (`Event::Mcp`'s id), with its connection and kind.
    bridged: HashMap<String, (u64, Bridged)>,
    bridged_n: u64,
}

impl Wire {
    fn new(begin: Begin, out: Sender<String>) -> Wire {
        Wire {
            begin,
            out: Some(out),
            next_id: 0,
            asked: HashMap::new(),
            load: false,
            resume: false,
            system_prompt: false,
            version: None,
            session: None,
            system_first: None,
            replaying: false,
            queue: VecDeque::new(),
            turn: false,
            text: String::new(),
            asks: HashMap::new(),
            dropped: HashSet::new(),
            pending: String::new(),
            pending_thought: false,
            last_flush: crate::clock::now(),
            thinking: false,
            end: false,
            ended_as: None,
            junk_logged: false,
            reports: HashMap::new(),
            calls: Vec::new(),
            reports_n: 0,
            tripped: false,
            bridge: None,
            link: None,
            bridged: HashMap::new(),
            bridged_n: 0,
        }
    }

    /// The session's `mcpServers`: the bridge, where tarae's tools are offered.
    fn servers(&self) -> Value {
        Value::Array(self.bridge.iter().map(bridge::Bridge::server).collect())
    }

    /// The bridge bound, where tarae's tools are offered. Err: why it can't be.
    fn bind(&mut self) -> Result<(), String> {
        let Some((state, agent, to)) = self.begin.tools.take() else { return Ok(()) };
        let state = state.ok_or("no state folder (neither XDG_STATE_HOME nor HOME)")?;
        self.bridge = Some(bridge::Bridge::listen(&state, agent, to)?);
        Ok(())
    }

    fn write(&self, v: Value) {
        if let Some(out) = &self.out {
            let _ = out.send(v.to_string());
        }
    }

    fn request(&mut self, method: &str, params: Value, what: Asked) {
        let id = self.next_id;
        self.next_id += 1;
        self.asked.insert(id, what);
        self.write(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    }

    fn initialize(&mut self) {
        let params = json!({
            "protocolVersion": VERSION,
            "clientCapabilities": {
                "fs": {"readTextFile": false, "writeTextFile": false},
                "terminal": false,
                "session": {"compaction": {}},
            },
            "clientInfo": {"name": "tarae", "version": env!("CARGO_PKG_VERSION")},
        });
        self.request("initialize", params, Asked::Initialize);
    }

    /// The session, once `initialize` answered: the conversation continued where it can be, else a new one.
    fn open(&mut self) {
        let cwd = self.begin.cwd.to_string_lossy().into_owned();
        let system = self.begin.system.take();
        let servers = self.servers();
        match self.begin.resume.clone() {
            Some(id) if self.resume => {
                let params = json!({"sessionId": id, "cwd": cwd, "mcpServers": servers});
                self.request("session/resume", params, Asked::Resume(id));
            }
            Some(id) if self.load => {
                self.replaying = true;
                let params = json!({"sessionId": id, "cwd": cwd, "mcpServers": servers});
                self.request("session/load", params, Asked::Load(id));
            }
            Some(id) => self.gone(&id),
            None => {
                let mut params = json!({"cwd": cwd, "mcpServers": servers});
                match (system, self.system_prompt) {
                    (Some(text), true) => {
                        params["systemPrompt"] = json!({"mode": "append", "content": [text_block(text)]});
                    }
                    (system, _) => self.system_first = system,
                }
                self.request("session/new", params, Asked::New);
            }
        }
    }

    /// A conversation that can't be continued: the process ends, its end's kind `Gone` (the chat's path for one: a
    /// new conversation started, said, the message sent again).
    fn gone(&mut self, id: &str) {
        let said = format!("{} has no conversation {id}", self.begin.name);
        self.ended_as = Some(Exit { said, kind: Ended::Gone });
        self.end = true;
    }

    /// The session known, in the row's mode: said (`Conversation`, `Ready`), and the first prompt goes.
    fn opened(&mut self, id: String, model: String, send: &dyn Fn(Event) -> bool) -> bool {
        self.session = Some(id.clone());
        let ev = Event::Conversation { id, model, version: self.version.clone(), key_source: None };
        if !send(ev) || !send(Event::Ready { models: Vec::new(), key_source: None, account: None }) {
            return false;
        }
        self.next();
        true
    }

    /// Guard 2 at the start: the session's answer `r` in the row's mode — opened at once; in another it offers, set to
    /// the row's first (`session/set_mode`, the session held until its answer); with no mode said, or the row's not
    /// offered, the start ends, said (fail-closed).
    fn mode_at_start(&mut self, id: String, r: &Value, send: &dyn Fn(Event) -> bool) -> bool {
        let (name, model) = (self.begin.name, model_of(r));
        let Some(mode) = self.begin.mode else { return self.opened(id, model, send) };
        let modes = &r["modes"];
        let offered = modes["availableModes"].as_array().into_iter().flatten().any(|m| m["id"] == mode);
        match modes["currentModeId"].as_str() {
            Some(now) if now == mode => self.opened(id, model, send),
            Some(_) if offered => {
                let params = json!({"sessionId": id, "modeId": mode});
                self.request("session/set_mode", params, Asked::Mode(Some((id, model))));
                true
            }
            _ => {
                self.end = true;
                send(Event::Done(Err(format!("{name} offers no mode {mode}"))))
            }
        }
    }

    /// Guard 2 mid-session: a `current_mode_update` to a mode not the row's is set back (`session/set_mode`); one it
    /// won't take back stops it (`Asked::Mode(None)`'s error).
    fn mode_moved(&mut self, u: &Value) {
        let Some(mode) = self.begin.mode else { return };
        let Some(session) = self.session.clone() else { return };
        if u["currentModeId"].as_str() != Some(mode) {
            self.request(
                "session/set_mode",
                json!({"sessionId": session, "modeId": mode}),
                Asked::Mode(None),
            );
        }
    }

    /// Stopped by tarae (a trip, a mode it won't keep): its group ended at once, before anything is said — whatever it
    /// runs ends with it; the bridge let go (a bridged call still open gets no answer), nothing queued written; what
    /// streamed sent, `row` (a call's row it never had), and `said` ending the turn if one runs. Its end's kind and
    /// words are `ended_as`, told once it's reaped.
    fn stop(&mut self, said: String, kind: Ended, row: Option<Event>, send: &dyn Fn(Event) -> bool) -> bool {
        self.end = true;
        if let Some(pid) = self.begin.pid {
            super::process::end_group(pid);
        }
        self.bridge = None;
        self.link = None;
        self.queue.clear();
        let mut ok = self.thought_over(send) && self.flush(send);
        if let Some(row) = row {
            ok = ok && send(row);
        }
        if std::mem::replace(&mut self.turn, false) {
            ok = ok && send(Event::Done(Err(said.clone())));
        }
        self.ended_as = Some(Exit { said, kind });
        ok
    }

    /// Guard 3's trip (plan §6): it ran `tool` itself — `own`: one of its own it reported completed; else one of
    /// tarae's that the bridge never carried. Stopped (`stop`), the log told its name and the agent's version, never
    /// its input.
    fn trip(&mut self, tool: String, own: bool, row: Option<Event>, send: &dyn Fn(Event) -> bool) -> bool {
        let name = self.begin.name;
        let version = self.version.as_deref().unwrap_or("its version unsaid");
        crate::log::line(name, "warn", &format!("tripped: it ran {tool:?} itself ({version}) — stopped"));
        let tool = match tool.is_empty() {
            true => "one with no name".to_string(),
            false => tool,
        };
        let said = match own {
            true => format!(
                "{name} ran a tool of its own, {tool} — stopped: it and everything it started ended. Only tarae's \
                 tools may run; its hook should have refused this."
            ),
            false => format!(
                "{name} said it ran tarae's {}, which tarae never ran — stopped: it and everything it started ended.",
                taraes(&tool).unwrap_or(&tool)
            ),
        };
        self.tripped = true;
        self.stop(said, Ended::Tripped { tool }, row, send)
    }

    /// The tripwire's reading of one `tool_call` or `tool_call_update` (plan §6, the design session's Q5 and
    /// condition 1): kept by id, its name from its `tool_call`; one of its own trips at `completed`, never before (a
    /// refused call goes `in_progress`, then `failed`), and is a refused row at `failed`; one of tarae's is matched
    /// with a bridge call and judged at the turn's end (`judge`). A report of tarae's already judged is passed by.
    fn watch(&mut self, u: &Value) -> Watch {
        let id = u["toolCallId"].as_str().unwrap_or_default().to_string();
        let first = u["sessionUpdate"] == "tool_call";
        if !first && !self.reports.contains_key(&id) && self.dropped.contains(&id) {
            return Watch::Pass;
        }
        let n = self.reports_n;
        let r = self.reports.entry(id.clone()).or_insert_with(|| Report { seq: n, ..Report::default() });
        self.reports_n += 1;
        if first {
            r.name = tool_name(u);
        }
        match input_of(u) {
            Value::Null => {}
            input => r.input = input,
        }
        match u["status"].as_str() {
            Some("completed") => r.done = Some(true),
            Some("failed") => r.done = Some(false),
            _ => {}
        }
        if taraes(&r.name).is_some() {
            if !r.matched {
                self.match_report(&id);
            }
            return Watch::Pass;
        }
        match r.done {
            Some(true) => Watch::Trip(r.name.clone(), first),
            Some(false) => Watch::Refused,
            None => Watch::Pass,
        }
    }

    /// Report `id` of tarae's matched with the first call this turn of its tool that it says: every argument the call
    /// had is in the report's, the same (it may hold more — the design session's condition 1). Each matched once.
    fn match_report(&mut self, id: &str) {
        let Some(r) = self.reports.get_mut(id) else { return };
        let Some(tool) = taraes(&r.name) else { return };
        let call =
            self.calls.iter_mut().find(|c| !c.matched && c.tool == tool && carried(&c.arguments, &r.input));
        if let Some(c) = call {
            (c.matched, r.matched) = (true, true);
        }
    }

    /// A call of the bridge's (`tool`, on `arguments`) kept for the turn, matched with the first report of tarae's not
    /// matched yet that says it.
    fn called(&mut self, tool: &str, arguments: Value) {
        let report = (self.reports.values_mut())
            .filter(|r| !r.matched && taraes(&r.name) == Some(tool) && carried(&arguments, &r.input))
            .min_by_key(|r| r.seq);
        let matched = report.map(|r| r.matched = true).is_some();
        self.calls.push(Called { tool: tool.to_string(), arguments, matched });
    }

    /// The turn's end for the tripwire: each report of tarae's the bridge never carried, judged — one that completed
    /// trips (its row first, as it never had one); one that failed is a warning row; one still running (a stop cut it
    /// off) neither. They and the turn's calls go, and its own that ended. Some: it tripped (whether the editor's still
    /// there).
    fn judge(&mut self, send: &dyn Fn(Event) -> bool) -> Option<bool> {
        self.calls.clear();
        let judged: Vec<String> = (self.reports.iter())
            .filter(|(_, r)| taraes(&r.name).is_some() || r.done.is_some())
            .map(|(id, _)| id.clone())
            .collect();
        let mut left: Vec<(String, Report)> = judged
            .into_iter()
            .filter_map(|id| self.reports.remove(&id).map(|r| (id, r)))
            .filter(|(_, r)| taraes(&r.name).is_some() && !r.matched && r.done.is_some())
            .collect();
        left.sort_by_key(|(_, r)| r.seq);
        let name = self.begin.name;
        for (id, r) in left {
            let row = Event::Tool { id: id.clone(), name: r.name.clone(), input: r.input };
            if r.done == Some(true) {
                return Some(self.trip(r.name, false, Some(row), send));
            }
            let tool = taraes(&r.name).unwrap_or_default();
            let text = format!("{name} said tarae's {tool} failed, which tarae never ran");
            if !(self.flush(send) && send(row) && send(Event::ToolResult { id, text, error: true })) {
                return Some(false);
            }
        }
        None
    }

    /// Signed out (`-32000`): said, and the process ends — never `authenticate` (the brief's §5: you sign in with
    /// the agent's own program).
    fn signed_out(&mut self, send: &dyn Fn(Event) -> bool) {
        let (name, command) = (self.begin.name, self.begin.command);
        send(Event::Done(Err(format!(
            "{name} isn't signed in — sign in with `{command}` itself, then send again"
        ))));
        self.end = true;
    }

    /// The next prompt, if the session is known and no turn runs.
    fn next(&mut self) {
        let Some(session) = self.session.clone().filter(|_| !self.turn && !self.end) else { return };
        let Some(p) = self.queue.pop_front() else { return };
        let mut blocks: Vec<Value> = self.system_first.take().map(text_block).into_iter().collect();
        match p {
            Prompt::Text(t) => blocks.push(text_block(t)),
            // (`cache_hour`: ACP has no word for it)
            Prompt::Parts(parts) => blocks.extend(parts.into_iter().map(|p| text_block(p.text))),
        }
        self.turn = true;
        self.text.clear();
        self.request("session/prompt", json!({"sessionId": session, "prompt": blocks}), Asked::Prompt);
    }

    fn cancel(&mut self) {
        for (_, (id, _)) in self.asks.drain().collect::<Vec<_>>() {
            self.write(response(&id, json!({"outcome": {"outcome": "cancelled"}})));
        }
        if let Some(session) = self.session.clone().filter(|_| self.turn) {
            self.write(
                json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": session}}),
            );
        }
    }

    fn answer(&mut self, key: &str, p: Permission) {
        let Some((id, options)) = self.asks.remove(key) else { return };
        self.write(response(&id, outcome(&options, &p)));
    }

    /// A message of the bridge's: a `tools/call` comes up as its call (`Tool`, named as Claude's MCP client names
    /// tarae's tools) right before its `Mcp` — the bridge, not the agent's own report, tells a tarae tool's call
    /// (finding C); anything else is its `Mcp` alone. Each its key, `bridge-N`.
    fn bridged(&mut self, msg: Value, send: &dyn Fn(Event) -> bool) -> bool {
        // (none after a trip: it let the bridge go)
        let Some((link, _)) = self.link else { return true };
        if !(self.thought_over(send) && self.flush(send)) {
            return false;
        }
        self.bridged_n += 1;
        let key = format!("bridge-{}", self.bridged_n);
        let kind = match (msg.get("id").filter(|i| !i.is_null()), msg["method"].as_str()) {
            (None, _) => Bridged::Notification,
            (Some(_), Some("tools/call")) => Bridged::Call,
            (Some(_), _) => Bridged::Request,
        };
        self.bridged.insert(key.clone(), (link, kind));
        if kind == Bridged::Call {
            let tool = msg["params"]["name"].as_str().unwrap_or_default();
            let name = format!("mcp__{}__{tool}", bridge::NAME);
            let input = msg["params"]["arguments"].clone();
            self.called(tool, input.clone());
            if !send(Event::Tool { id: key.clone(), name, input }) {
                return false;
            }
        }
        send(Event::Mcp { id: key, message: msg })
    }

    /// tarae's MCP server's answer to bridged message `key`: back through the connection that asked, if it's still
    /// the live one (a notification's: nowhere); a call's is its `ToolResult` too, wherever the answer went.
    fn mcp_answered(&mut self, key: &str, answer: Value, send: &dyn Fn(Event) -> bool) -> bool {
        let Some((link, kind)) = self.bridged.remove(key) else { return true };
        if let Some((_, out)) =
            self.link.as_ref().filter(|(now, _)| *now == link && kind != Bridged::Notification)
        {
            let _ = out.send(answer.to_string());
        }
        if kind != Bridged::Call {
            return true;
        }
        let (text, error) = match answer.get("error") {
            Some(e) => (e["message"].as_str().unwrap_or("an error").to_string(), true),
            None => {
                let content = answer["result"]["content"].as_array().into_iter().flatten();
                let text = content.filter_map(text_of).collect::<Vec<_>>().join("\n");
                (text, answer["result"]["isError"] == true)
            }
        };
        send(Event::ToolResult { id: key.to_string(), text: cut(text), error })
    }

    /// One message the handle or the bridge handed the conversation's thread, but stdout's end (`run`; the
    /// simulation's driver, `sim/acp.rs`). False: the editor's gone.
    fn take(&mut self, m: In, send: &dyn Fn(Event) -> bool) -> bool {
        // (tripped: nothing after it is read — the agent's lines, the bridge's, a prompt, an answer; but the handle's end)
        if self.tripped {
            if let In::Closed = m {
                self.out = None;
            }
            return true;
        }
        match m {
            In::Line(line) => self.line(&line, send),
            In::Eof => true,
            In::Prompt(p) => {
                self.queue.push_back(p);
                self.next();
                true
            }
            In::Cancel => {
                self.cancel();
                true
            }
            In::Answer(id, p) => {
                self.answer(&id, p);
                true
            }
            In::Closed => {
                self.out = None;
                true
            }
            In::Linked(n, out) => {
                self.link = Some((n, out));
                true
            }
            In::Unlinked(n) => {
                self.link = self.link.take().filter(|(now, _)| *now != n);
                true
            }
            In::Bridged(msg) => self.bridged(msg, send),
            In::Mcp(key, answer) => self.mcp_answered(&key, answer, send),
        }
    }

    /// What streamed and wasn't sent yet, sent. False: the editor's gone.
    fn flush(&mut self, send: &dyn Fn(Event) -> bool) -> bool {
        self.last_flush = crate::clock::now();
        let t = std::mem::take(&mut self.pending);
        t.is_empty() || send(if self.pending_thought { Event::Thought(t) } else { Event::Text(t) })
    }

    /// Streamed text (`thought`: thinking) held to go with what comes within `FLUSH`.
    fn stream(&mut self, t: String, thought: bool, send: &dyn Fn(Event) -> bool) -> bool {
        if thought != self.pending_thought && !self.flush(send) {
            return false;
        }
        self.pending_thought = thought;
        self.pending.push_str(&t);
        crate::clock::since(self.last_flush) < super::claude::FLUSH || self.flush(send)
    }

    /// A thinking run over: its summary, then its end.
    fn thought_over(&mut self, send: &dyn Fn(Event) -> bool) -> bool {
        if !self.thinking {
            return true;
        }
        self.thinking = false;
        self.flush(send) && send(Event::ThinkingBlock(false))
    }

    /// One line of stdout. False: the editor's gone.
    fn line(&mut self, line: &str, send: &dyn Fn(Event) -> bool) -> bool {
        // (tripped: nothing after it is read as the turn's)
        if self.tripped {
            return true;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            if !std::mem::replace(&mut self.junk_logged, true) {
                crate::log::line(
                    self.begin.name,
                    "warn",
                    "a line that isn't JSON on its stdout (once a process)",
                );
            }
            return true;
        };
        match read(&v) {
            Read::Answer { id, result } => self.answered(id, result, send),
            Read::Ask { id, params } => {
                let key = id_key(&id);
                let ev = permission(&key, &params);
                self.asks.insert(key, (id, params["options"].clone()));
                self.thought_over(send) && self.flush(send) && send(ev)
            }
            Read::Request { id } => {
                // (`fs/*` and `terminal/*` aren't offered, `elicitation/create` nor any other is served)
                self.write(json!({"jsonrpc": "2.0", "id": id,
                    "error": {"code": -32601, "message": "Method not found"}}));
                true
            }
            Read::Update(_) if self.replaying => true,
            Read::Update(u) => self.update(&u, send),
            Read::Other => true,
        }
    }

    fn update(&mut self, u: &Value, send: &dyn Fn(Event) -> bool) -> bool {
        let kind = u["sessionUpdate"].as_str().unwrap_or_default();
        let thought = kind == "agent_thought_chunk";
        if !thought && !self.thought_over(send) {
            return false;
        }
        if kind == "current_mode_update" {
            self.mode_moved(u);
            return true;
        }
        let refused = match kind {
            "tool_call" | "tool_call_update" => match self.watch(u) {
                Watch::Trip(tool, unseen) => {
                    let id = u["toolCallId"].as_str().unwrap_or_default().to_string();
                    let row = unseen.then(|| Event::Tool { id, name: tool.clone(), input: input_of(u) });
                    return self.trip(tool, true, row, send);
                }
                Watch::Refused => true,
                Watch::Pass => false,
            },
            _ => false,
        };
        match update(u, &mut self.dropped) {
            Some(Event::ToolResult { id, error: true, .. }) if refused => {
                self.flush(send) && send(Event::ToolResult { id, text: REFUSED.into(), error: true })
            }
            Some(Event::Text(t)) => {
                self.text.push_str(&t);
                self.stream(t, false, send)
            }
            Some(Event::Thought(t)) => {
                if !self.thinking {
                    self.thinking = true;
                    if !(self.flush(send) && send(Event::Thinking) && send(Event::ThinkingBlock(true))) {
                        return false;
                    }
                }
                self.stream(t, true, send)
            }
            Some(ev) => self.flush(send) && send(ev),
            None => true,
        }
    }

    fn answered(
        &mut self,
        id: u64,
        result: Result<Value, (i64, String)>,
        send: &dyn Fn(Event) -> bool,
    ) -> bool {
        let Some(what) = self.asked.remove(&id) else { return true };
        let name = self.begin.name;
        match (what, result) {
            (_, Err((AUTH_REQUIRED, _))) => {
                self.turn = false;
                let _ = self.thought_over(send) && self.flush(send);
                self.signed_out(send);
            }
            (Asked::Initialize, Ok(r)) => {
                let version = r["protocolVersion"].as_u64().unwrap_or(0);
                if version != VERSION {
                    send(Event::Done(Err(format!(
                        "{name} speaks ACP version {version}; tarae speaks {VERSION}"
                    ))));
                    self.end = true;
                    return true;
                }
                let caps = &r["agentCapabilities"];
                let has = |v: &Value| !v.is_null() && *v != false;
                self.load = caps["loadSession"] == true;
                self.resume = has(&caps["sessionCapabilities"]["resume"]);
                self.system_prompt = has(&caps["sessionCapabilities"]["systemPrompt"]);
                self.version = r["agentInfo"]["version"].as_str().filter(|v| !v.is_empty()).map(String::from);
                // (tarae's tools: the bridge bound here, on this thread, before the session that offers it)
                if let Err(e) = self.bind() {
                    send(Event::Done(Err(format!("{name}: tarae's tools can't be offered: {e}"))));
                    self.end = true;
                    return true;
                }
                self.open();
            }
            (Asked::Resume(id) | Asked::Load(id), Err((UNKNOWN_SESSION, _))) => self.gone(&id),
            (Asked::New | Asked::Resume(_) | Asked::Load(_) | Asked::Initialize, Err((_, message))) => {
                send(Event::Done(Err(format!("{name}: {message}"))));
                self.end = true;
            }
            (Asked::Mode(Some((id, model))), Ok(_)) => return self.opened(id, model, send),
            (Asked::Mode(Some(_)), Err((_, message))) => {
                let mode = self.begin.mode.unwrap_or_default();
                send(Event::Done(Err(format!("{name} won't take mode {mode}: {message}"))));
                self.end = true;
            }
            (Asked::Mode(None), Ok(_)) => {}
            (Asked::Mode(None), Err(_)) => {
                let mode = self.begin.mode.unwrap_or_default();
                let said = format!("{name} left mode {mode} and won't go back — stopped");
                return self.stop(said, Ended::Exited, None, send);
            }
            (what @ (Asked::New | Asked::Resume(_) | Asked::Load(_)), Ok(r)) => {
                self.replaying = false;
                let asked = match what {
                    Asked::Resume(id) | Asked::Load(id) => Some(id),
                    _ => None,
                };
                let Some(id) = r["sessionId"].as_str().map(String::from).or(asked) else {
                    send(Event::Done(Err(format!("{name} named no session"))));
                    self.end = true;
                    return true;
                };
                return self.mode_at_start(id, &r, send);
            }
            (Asked::Prompt, result) => {
                // (the tripwire's turn's end: a report of tarae's the bridge never carried trips here, its words the
                // turn's end)
                if let Some(ok) = self.judge(send) {
                    return ok;
                }
                self.turn = false;
                if !(self.thought_over(send) && self.flush(send)) {
                    return false;
                }
                let done = match result {
                    Ok(r) => {
                        let conversation = self.session.clone().unwrap_or_default();
                        if let Some(spent) = spent(&r["usage"], conversation, crate::usage::now_ms())
                            && !send(Event::Spent(spent))
                        {
                            return false;
                        }
                        stopped(name, r["stopReason"].as_str().unwrap_or_default(), &self.text)
                    }
                    Err((_, message)) => Err(message),
                };
                if !send(Event::Done(done)) {
                    return false;
                }
                self.next();
            }
        }
        true
    }
}

/// The conversation's thread: what stdout says and what the handle and the bridge hand it, in their order; text
/// batched as Claude's is (`claude::FLUSH`). Ends when stdout does: the bridge let go, the process reaped and its
/// group ended with it (`process::reap_group`), then its end told (stderr's last line) — `once`: after its first
/// turn, the process ended.
fn run(mut child: Spawned, mut w: Wire, rx: Receiver<In>, once: bool, send: &dyn Fn(Event) -> bool) {
    let (err_tx, err_rx) = channel::<String>();
    if let Some(err) = child.stderr.take() {
        crate::event::thread("tarae-acp-err", move || {
            let mut last = String::new();
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    last = line;
                }
            }
            let _ = err_tx.send(last);
        });
    }
    w.initialize();
    let mut ended = false;
    let mut done = false;
    loop {
        if w.end && !ended {
            ended = true;
            super::process::end_group(child.pid);
        }
        let held = !w.pending.is_empty();
        let left = super::claude::FLUSH.saturating_sub(crate::clock::since(w.last_flush));
        if held && left.is_zero() {
            if !w.flush(send) {
                break;
            }
            continue;
        }
        let got = match held {
            true => rx.recv_timeout(left),
            false => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let ok = match got {
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) | Ok(In::Eof) => {
                // (stdout's end mid-turn is the turn's end for the tripwire too)
                if w.turn && !w.tripped && w.judge(send).is_some() {
                    break;
                }
                let _ = w.thought_over(send) && w.flush(send);
                break;
            }
            Ok(m) => {
                let turns = w.turn;
                let ok = w.take(m, send);
                done |= turns && !w.turn;
                ok
            }
        };
        if !ok || (once && done) {
            break;
        }
    }
    if once || !ended && w.end {
        super::process::end_group(child.pid);
    }
    drop(w.out.take());
    drop(w.bridge.take());
    super::process::reap_group(child.pid);
    let err = err_rx.recv_timeout(std::time::Duration::from_millis(500)).unwrap_or_default();
    match once {
        true if !done => {
            send(Event::Done(Err(if err.is_empty() { "exited without a result".into() } else { err })));
        }
        true => {}
        false => {
            send(Event::Exit(w.ended_as.take().unwrap_or_else(|| Exit::exited(err))));
        }
    }
}

// ── The simulation's door ─────────────────────────────────────────────

/// The conversation's thread's work with no process and no thread (`sim/acp.rs`): the agent's lines and the bridge's
/// messages handed in by the scenario, what the chat handed the handle taken a message at a time when it says, what
/// went to the agent read back, and every event given back for the scenario to land — as `run` does, its order the
/// scenario's choice.
pub struct Staged {
    w: Wire,
    rx: Receiver<In>,
    written: Receiver<String>,
    /// Its end told: where the wire ends the process (a trip), its `Exit` comes once, after the events that ended it,
    /// as `run`'s after the reap.
    exited: bool,
}

impl Staged {
    /// A wire begun in `dir` (its `initialize` written), and the handle the chat holds.
    pub fn new(dir: &Path) -> (Staged, Agent) {
        let (in_tx, rx) = channel();
        let (out, written) = channel();
        let row = &super::STAND_IN;
        let begin = Begin {
            name: row.name,
            command: row.command,
            cwd: dir.into(),
            resume: None,
            system: None,
            tools: None,
            pid: None,
            mode: row.mode,
        };
        let mut w = Wire::new(begin, out);
        w.initialize();
        (Staged { w, rx, written, exited: false }, Agent::Acp(Proc(in_tx)))
    }

    /// `f`'s work, its events kept — and what streamed sent at once (the scenario's own clock has no `FLUSH` wait); the
    /// process's end after them, once, where the wire ended it.
    fn run(&mut self, f: impl FnOnce(&mut Wire, &dyn Fn(Event) -> bool)) -> Vec<Event> {
        let events = std::cell::RefCell::new(Vec::new());
        let send = |ev| {
            events.borrow_mut().push(ev);
            true
        };
        f(&mut self.w, &send);
        self.w.flush(&send);
        let mut events = events.into_inner();
        if let Some(exit) = self.w.ended_as.take().filter(|_| !self.exited) {
            self.exited = true;
            events.push(Event::Exit(exit));
        }
        events
    }

    /// A line from the agent's stdout.
    pub fn line(&mut self, v: &Value) -> Vec<Event> {
        self.run(|w, send| {
            w.line(&v.to_string(), send);
        })
    }

    /// The bridge's connection linked: what goes back through it comes out of the receiver.
    pub fn link(&mut self) -> Receiver<String> {
        let (tx, rx) = channel();
        self.run(|w, send| {
            w.take(In::Linked(1, tx), send);
        });
        rx
    }

    /// A message of the bridge's.
    pub fn bridged(&mut self, msg: Value) -> Vec<Event> {
        self.run(|w, send| {
            w.bridged(msg, send);
        })
    }

    /// The next message the chat handed the handle, taken: what it was, and its events. None: none waits.
    pub fn take(&mut self) -> Option<(String, Vec<Event>)> {
        let m = self.rx.try_recv().ok()?;
        let said = match &m {
            In::Prompt(_) => "a prompt".to_string(),
            In::Cancel => "a stop".into(),
            In::Answer(id, _) => format!("the answer to ask {id}"),
            In::Mcp(key, _) => format!("the answer to {key}"),
            In::Closed => "the handle dropped".into(),
            _ => "another".into(),
        };
        Some((said, self.run(|w, send| _ = w.take(m, send))))
    }

    /// What was written to the agent since the last look.
    pub fn written(&self) -> Vec<Value> {
        self.written.try_iter().filter_map(|l| serde_json::from_str(&l).ok()).collect()
    }
}

// ── Reading its lines ───────────────────────────────────────────────────

/// ACP's error for a request that needs a sign-in first.
const AUTH_REQUIRED: i64 = -32000;
/// ACP's error for a session the agent doesn't have.
const UNKNOWN_SESSION: i64 = -32002;

/// What one line from the agent is.
#[derive(Debug, PartialEq)]
enum Read {
    /// The answer to a request of ours (our ids are numbers).
    Answer { id: u64, result: Result<Value, (i64, String)> },
    /// `session/request_permission`.
    Ask { id: Value, params: Value },
    /// Any other request of the agent's: not served.
    Request { id: Value },
    /// A `session/update`'s update.
    Update(Value),
    /// A notification tarae doesn't read, or an answer to no request of ours.
    Other,
}

fn read(v: &Value) -> Read {
    let id = v.get("id").filter(|i| !i.is_null());
    match (v["method"].as_str(), id) {
        (Some("session/request_permission"), Some(id)) => {
            Read::Ask { id: id.clone(), params: v["params"].clone() }
        }
        (Some(_), Some(id)) => Read::Request { id: id.clone() },
        (Some("session/update"), None) => Read::Update(v["params"]["update"].clone()),
        (Some(_), None) => Read::Other,
        (None, Some(id)) => match id.as_u64() {
            Some(id) => {
                let result = match v.get("error") {
                    Some(e) => Err((
                        e["code"].as_i64().unwrap_or_default(),
                        e["message"].as_str().unwrap_or("an error").to_string(),
                    )),
                    None => Ok(v["result"].clone()),
                };
                Read::Answer { id, result }
            }
            None => Read::Other,
        },
        (None, None) => Read::Other,
    }
}

/// A JSON-RPC id as `Event::Permission.id` carries it.
fn id_key(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// What a tool call is called above the wire: the agent's own `name` (agy's `run_command`, `read_file`), else its
/// `kind` — never its `title`, the model's text (agy's for a command is the command line itself; condition 1).
fn tool_name(call: &Value) -> String {
    let s = |k: &str| call[k].as_str().filter(|s| !s.is_empty());
    s("name").or_else(|| s("kind")).unwrap_or_default().to_string()
}

/// A permission ask (`session/request_permission`'s params) as the chat gets it, `id` its key.
fn permission(id: &str, params: &Value) -> Event {
    let call = &params["toolCall"];
    Event::Permission { id: id.to_string(), tool: tool_name(call), input: input_of(call) }
}

/// A permission's answer: `Allow` selects the `allow_once` option, `Deny` the `reject_once` one — never an
/// `allow_always`; with no such option, `cancelled`. `Allow`'s input and `Deny`'s message: ACP's answer carries
/// neither.
fn outcome(options: &Value, p: &Permission) -> Value {
    let kind = match p {
        Permission::Allow { .. } => "allow_once",
        Permission::Deny { .. } => "reject_once",
    };
    let chosen = options.as_array().into_iter().flatten().find(|o| o["kind"] == kind);
    match chosen.and_then(|o| o["optionId"].as_str()) {
        Some(id) => json!({"outcome": {"outcome": "selected", "optionId": id}}),
        None => json!({"outcome": {"outcome": "cancelled"}}),
    }
}

fn response(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn text_block(text: String) -> Value {
    json!({"type": "text", "text": text})
}

/// A call's input: `rawInput`, else agy's `args` (the same).
fn input_of(call: &Value) -> Value {
    match &call["rawInput"] {
        Value::Null => call["args"].clone(),
        v => v.clone(),
    }
}

/// Whether a report's arguments (`report`) carry a bridge call's (`call`): every key the call had is in the report's,
/// the same — the report may hold more (agy adds `toolSummary`, which the server never gets: the design session's
/// condition 1). A call with no arguments: any report of its tool.
fn carried(call: &Value, report: &Value) -> bool {
    match call {
        Value::Null => true,
        Value::Object(o) => o.iter().all(|(k, v)| report.get(k) == Some(v)),
        other => other == report,
    }
}

/// A content block's text — None: another kind (an image, audio, a resource: dropped).
fn text_of(content: &Value) -> Option<String> {
    (content["type"] == "text").then(|| content["text"].as_str().map(String::from)).flatten()
}

/// A finished call's text content, its first 4 KB — a write's `diff` and a read's null content are none.
fn result_text(content: &Value) -> String {
    let texts = content.as_array().into_iter().flatten().filter(|c| c["type"] == "content");
    cut(texts.filter_map(|c| text_of(&c["content"])).collect::<Vec<_>>().join("\n"))
}

/// A result's text as `Event::ToolResult` carries it: its first 4 KB.
fn cut(mut text: String) -> String {
    if text.len() > 4096 {
        let cut = (0..=4096).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
        text.truncate(cut);
    }
    text
}

/// A `session/update` as an event, or None — dropped, each said: the prompt echoed (`user_message_chunk`), a
/// chunk's non-text content, ACP's `plan` (tarae's `plan` tool is the plan, the design session's §9.2), a call's
/// `locations` (§9.1: the places come from tarae's tools' inputs), a call's `diff` and a read's null content, the
/// commands, config options and title it offers (`available_commands_update`, `config_option_update`,
/// `session_info_update`: step 4), `usage_update` (no per-request usage to read it with), a compaction's summary, a
/// `notice`. tarae's own tools' calls (`OWN`, by agy's `name`) are dropped and kept in `dropped`, so their updates go
/// too. Its mode (`current_mode_update`) and what its calls came to are the wire's to read first (`Wire::update`:
/// guards 2 and 3).
fn update(u: &Value, dropped: &mut HashSet<String>) -> Option<Event> {
    let id = || u["toolCallId"].as_str().unwrap_or_default().to_string();
    match u["sessionUpdate"].as_str()? {
        "agent_message_chunk" => text_of(&u["content"]).map(Event::Text),
        "agent_thought_chunk" => text_of(&u["content"]).map(Event::Thought),
        "tool_call" => {
            let name = tool_name(u);
            if name.starts_with(OWN) {
                dropped.insert(id());
                return None;
            }
            Some(Event::Tool { id: id(), name, input: input_of(u) })
        }
        "tool_call_update" if dropped.contains(&id()) => None,
        "tool_call_update" => {
            let error = match u["status"].as_str()? {
                "completed" => false,
                "failed" => true,
                _ => return None,
            };
            Some(Event::ToolResult { id: id(), text: result_text(&u["content"]), error })
        }
        "compaction_update" => match u["status"].as_str()? {
            "in_progress" => Some(Event::Compaction(crate::compaction::Ev::Started)),
            "completed" | "failed" | "cancelled" => Some(Event::Compaction(crate::compaction::Ev::Ended)),
            _ => None,
        },
        _ => None,
    }
}

/// The model a session answer says it runs: its `configOptions`' model select's current value, else "".
fn model_of(r: &Value) -> String {
    let mut options = r["configOptions"].as_array().into_iter().flatten();
    let model = options.find(|o| o["id"] == "model" || o["category"] == "model");
    model.and_then(|o| o["currentValue"].as_str()).unwrap_or_default().to_string()
}

/// A turn's `usage` as tarae counts it (None: not given): `thoughtTokens` dropped — not in agy's total, and what it
/// counts isn't measured yet (step 4's probe).
fn spent(usage: &Value, conversation: String, at_ms: u64) -> Option<crate::usage::Spent> {
    if !usage.is_object() {
        return None;
    }
    let n = |k: &str| usage[k].as_u64();
    let counts = crate::usage::Counts {
        input: n("inputTokens"),
        write: n("cachedWriteTokens"),
        read: n("cachedReadTokens"),
        output: n("outputTokens"),
    };
    Some(crate::usage::Spent { conversation, counts, total_cost: None, models: Vec::new(), ms: None, at_ms })
}

/// A turn's end by its `stopReason`: its answer, or why it has none.
fn stopped(name: &str, reason: &str, text: &str) -> Result<String, String> {
    match reason {
        "end_turn" => Ok(text.to_string()),
        // (what the chat gets for a turn it stopped, as from Claude)
        "cancelled" => Err("stopped".into()),
        "max_tokens" => Err(format!("{name} stopped: the answer reached its length limit")),
        "max_turn_requests" => Err(format!("{name} stopped: the turn reached its limit of requests")),
        "refusal" => Err(format!("{name} refused to go on")),
        other => Err(format!("{name} stopped: {other}")),
    }
}

#[cfg(test)]
mod tests;
