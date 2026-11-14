pub mod flags;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
#[cfg(test)]
use std::process::Stdio;
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::time::Duration;

use crate::agent::{Agent, Caps, Event, First, ModelInfo, Permission, Prompt, SpawnError, Started};
use crate::billing::Credential;
use crate::event::Apply;
#[cfg(test)]
use crate::llm::LlmConfig;
use crate::llm::{ClaudeEnv, Env, judged, process_names};

/// Speed options: no tools (pure transform), skip MCP·user settings, no conversation kept by claude (keeps /resume clean).
pub(crate) const DEFAULT_ARGS: &[&str] = &[
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--include-partial-messages",
    "--tools",
    "",
    "--no-session-persistence",
    "--strict-mcp-config",
    "--setting-sources",
    "",
];

impl ModelInfo {
    pub fn list(v: &serde_json::Value) -> Vec<ModelInfo> {
        let s = |m: &serde_json::Value, k: &str| m[k].as_str().unwrap_or_default().to_string();
        v.as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["value"].is_string())
            .map(|m| ModelInfo {
                value: s(m, "value"),
                name: s(m, "displayName"),
                about: s(m, "description"),
                resolved: s(m, "resolvedModel"),
                efforts: m["supportedEffortLevels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.as_str().map(str::to_string))
                    .collect(),
            })
            .collect()
    }
}

/// Claude, started in `dir` (the project root — its relative paths, its tools and its kept conversations are there),
/// with tarae's environment less what `claude_env` leaves out, on `cred` (`billing.rs`: an API session's key helper
/// laid into its `--settings`). `given`: `Editor::claude_env` — a test's stand-in for tarae's environment, laid
/// over the process's as if inherited (None: the process's own); what's left out is judged on it alone, and the
/// process's names those rules judge aren't inherited (`judged`). As you, as every other tool runs claude: no
/// sandbox of its own (user decision 2026-10-17) — what it does through its tools goes through tarae. Tests'
/// (a start is `agent::start`, on its role's flags): `cfg`'s program on its own `llm.args`, `extra` after.
#[cfg(test)]
pub fn spawn(
    cfg: &LlmConfig,
    extra: &[&str],
    dir: &Path,
    given: Option<&Env>,
    cred: &Credential,
) -> Result<Child, SpawnError> {
    let s = crate::agent::Start::new(crate::agent::Role::Draft, cfg);
    let mut cmd = command(&s, &cfg.args, extra, dir, given, cred)?;
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.spawn().map_err(SpawnError::Start)
}

/// `s`'s command, not started: its program (`llm.command`, itself) on `args` (its role's, in place of `llm.args`)
/// and `extra`, and its environment.
pub fn command(
    s: &crate::agent::Start,
    args: &[String],
    extra: &[&str],
    dir: &Path,
    given: Option<&Env>,
    cred: &Credential,
) -> Result<Command, SpawnError> {
    let program = s.claude.command;
    // (tests never start the real claude — a stand-in in `sh` answers for it; card model P2 found a test
    // whose claude read ~/.claude.json and the keychain)
    #[cfg(test)]
    assert!(
        Path::new(program).file_name().is_none_or(|n| n != "claude"),
        "a test started the real claude — give llm.command a stand-in (`sh -c 'cat > /dev/null'`)"
    );
    let args = flags::billing::args(args, cred).map_err(SpawnError::Billing)?;
    let mut cmd = Command::new(program);
    cmd.current_dir(dir).args(&args).args(extra);
    if let Some(given) = given {
        // (a stand-in is the whole of what the rules judge — `ClaudeEnv::of`: the process's own such names stay out)
        for name in process_names().into_iter().filter(|n| judged(n) && !given.contains_key(n)) {
            cmd.env_remove(name);
        }
        cmd.envs(given);
    }
    // claude doesn't update itself from tarae (decision A): the user updates it (`:claude-update`), never a
    // program swapped under a chat that's running — `DISABLE_UPDATES` stops the background updater and
    // `claude update`/`install` alike (its env-vars reference). And its `git status` takes no index lock under
    // you (`.git/index.lock`)
    cmd.env("DISABLE_UPDATES", "1").env("GIT_OPTIONAL_LOCKS", "0");
    // Nor does it read Claude Code's auto memory (threat model G21): its folder is the repository's (every
    // checkout's), and what's there is another tool's notes — instructions not meant for it, paid for in every
    // request; a note this claude wrote there would later be obeyed by your own Claude Code. The variable
    // outranks the setting `autoMemoryEnabled` (2.1.296), so a project's settings can't turn it back on. Team
    // memory (`CLAUDE_MEMORY_STORES`) is memory too: never passed
    cmd.env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1").env_remove("CLAUDE_MEMORY_STORES");
    // Nor does it reach your SSH agent (threat model G20): through its socket the agent signs with your keys for
    // whoever asks, no key read. claude's own git only reads status, it never needs one; Claude's runs never got
    // it (`runenv`). A path it finds elsewhere is reached: claude runs as you (threat model G20)
    cmd.env_remove("SSH_AUTH_SOCK");
    // Nor your wiki's token (`web.signed-in`): it can write the wiki, and only tarae's own GET sends it — never a
    // claude's, its children's, or what they print
    for w in s.signed_in {
        cmd.env_remove(&w.token_env);
    }
    // (removed by name, never `env_clear`: the rest of tarae's environment is claude's as it always was. An API
    // session never keeps a key from it: one there outranks its helper — Claude Code's credential order)
    let keep_keys = s.claude.keep_env_key && *cred == Credential::Subscription;
    for name in ClaudeEnv::of(given, keep_keys).removed {
        cmd.env_remove(name);
    }
    if let Some(m) = s.model {
        cmd.args(["--model", m]);
    }
    if !s.effort.is_empty() {
        cmd.args(["--effort", s.effort]);
    }
    Ok(cmd)
}

/// One user message line (stream-json input).
pub fn user_message(content: &str) -> String {
    user_content(serde_json::json!(content))
}

/// One user message line whose content is a string or a list of content blocks.
pub fn user_content(content: serde_json::Value) -> String {
    serde_json::json!({"type": "user", "message": {"role": "user", "content": content}}).to_string()
}

/// The first line to a chat process: registers tarae's own MCP server (`sdkMcpServers` — its messages come
/// back as `mcp_message` control requests).
pub fn initialize_message(servers: &[&str]) -> String {
    serde_json::json!({"type": "control_request", "request_id": "tarae-init",
        "request": {"subtype": "initialize", "sdkMcpServers": servers}})
    .to_string()
}

/// Our answer to a control request from claude.
pub fn control_response(request_id: &str, response: serde_json::Value) -> String {
    serde_json::json!({"type": "control_response",
        "response": {"subtype": "success", "request_id": request_id, "response": response}})
    .to_string()
}

/// Control request that stops the running turn — the process and conversation memory remain.
pub fn interrupt_message() -> String {
    serde_json::json!({"type": "control_request", "request_id": "tarae-stop", "request": {"subtype": "interrupt"}})
        .to_string()
}

/// What a claude can do: everything tarae asks of an agent.
pub const CAPS: Caps = Caps {
    interrupt: true,
    resume: true,
    fork: true,
    thinking_text: true,
    tokens_per_request: true,
    cost: true,
    limits: true,
    models: true,
    effort: true,
    compaction: true,
    own_tools_off: true,
    billing: true,
    asks: true,
    helpers: true,
};

/// Whether a claude that ended saying `err` is started again, and how: one whose last words refuse
/// `--thinking-display` (an older CLI), without it — once.
pub(super) fn again(err: &str, now: super::Again) -> Option<super::Again> {
    (err.contains("thinking-display") && !now.thinking_plain).then_some(super::Again { thinking_plain: true })
}

/// The request id tarae asks `get_usage` under — its answer comes back with it (`parse_line`).
pub const REQUEST_ID: &str = "tarae-usage";

/// How long a prompt's part kept an hour stays in the cache (`PromptPart::cache_hour`) — the reviewer's
/// `promptCacheTtl` says the same, so Claude Code's own breakpoints keep it as long.
pub const CACHE_HOUR: &str = "1h";

/// A prompt as one user message line: its text, or its parts as text blocks.
fn prompt_line(p: Prompt) -> String {
    let block = |text: String, hour: bool| match hour {
        true => serde_json::json!({ "type": "text", "text": text,
            "cache_control": { "type": "ephemeral", "ttl": CACHE_HOUR } }),
        false => serde_json::json!({ "type": "text", "text": text }),
    };
    match p {
        Prompt::Text(t) => user_message(&t),
        Prompt::Parts(parts) => {
            user_content(parts.into_iter().map(|p| block(p.text, p.cache_hour)).collect())
        }
    }
}

/// A permission's answer as Claude Code's permission prompt takes it.
fn permission(p: Permission) -> serde_json::Value {
    match p {
        Permission::Allow { input } => serde_json::json!({ "behavior": "allow", "updatedInput": input }),
        Permission::Deny { message } => serde_json::json!({ "behavior": "deny", "message": message }),
    }
}

/// What the `Agent` handle says to a claude, each a line of its stream-json.
impl Lines {
    pub(super) fn initialize(&self) -> std::io::Result<()> {
        self.send(initialize_message(&["tarae"]))
    }

    pub(super) fn prompt(&self, p: Prompt) -> std::io::Result<()> {
        self.send(prompt_line(p))
    }

    pub(super) fn stop_turn(&self) -> bool {
        self.send(interrupt_message()).is_ok()
    }

    pub(super) fn answer_mcp(&self, id: &str, response: serde_json::Value) {
        let _ = self.send(control_response(id, serde_json::json!({ "mcp_response": response })));
    }

    pub(super) fn answer_permission(&self, id: &str, p: Permission) {
        let _ = self.send(control_response(id, permission(p)));
    }

    pub(super) fn set_model(&self, model: &str) -> bool {
        self.request(serde_json::json!({ "subtype": "set_model", "model": model }))
    }

    /// (null: the model's own — measured: thinking back at the untouched default's)
    pub(super) fn set_effort(&self, effort: Option<&str>) -> bool {
        self.request(
            serde_json::json!({ "subtype": "apply_flag_settings", "settings": { "effortLevel": effort } }),
        )
    }

    /// The `get_usage` question (no transcript scan), under `REQUEST_ID`.
    pub(super) fn ask_limits(&self) -> bool {
        let line = serde_json::json!({"type": "control_request", "request_id": REQUEST_ID,
               "request": {"subtype": "get_usage", "skip_behaviors": true}});
        self.send(line.to_string()).is_ok()
    }

    /// A control request of ours whose answer isn't needed (a model or effort chosen), under an id of its own.
    fn request(&self, request: serde_json::Value) -> bool {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = format!("tarae-{}", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        let line = serde_json::json!({ "type": "control_request", "request_id": id, "request": request });
        self.send(line.to_string()).is_ok()
    }
}

/// A claude started: its first lines written (tarae's MCP server offered when `first.tools`, then the prompt if
/// there is one) — off the main loop, through its `Lines` — and its stream read by `pump` into `route`. Err: those
/// lines couldn't go (no stdin, or it stopped reading), the child ended and reaped off the main loop.
pub(super) fn begin(
    mut child: Child,
    first: First,
    once: bool,
    tx: crate::event::Tx,
    route: impl Fn(Event) -> Option<Apply> + Send + 'static,
) -> Result<Started, SpawnError> {
    let pid = child.id();
    let sent = match child.stdin.take() {
        Some(stdin) => {
            let lines = Lines::new(stdin);
            let own = first.tools.then(|| lines.weak());
            let offered = if first.tools { lines.initialize() } else { Ok(()) };
            let sent = offered.and_then(|_| first.prompt.map_or(Ok(()), |p| lines.prompt(p)));
            sent.map(|_| (lines, own))
        }
        None => Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "no stdin")),
    };
    let (lines, own) = match sent {
        Ok(sent) => sent,
        Err(e) => {
            super::process::end(child);
            return Err(SpawnError::Write(e));
        }
    };
    pump(child, own, once, tx, route);
    Ok(Started { agent: Agent::Claude(lines), pid })
}

// ── Writing to it ───────────────────────────────────────────────────────

/// A claude's stdin, written on a thread of its own, lines in the order sent. A claude still starting (a
/// cold start is 8–10 s) reads nothing yet, and a pipe takes 64 KB before a write waits — one message can
/// carry the file, other unsaved buffers and a screen: measured, a 200 KB message held the main loop as long
/// as the reader didn't read (2.0 s for a stand-in sleeping 2 s). Dropped: what's queued still goes, then
/// stdin closes — its pump's `Weak` doesn't keep it open.
pub struct Lines(Arc<Sender<String>>);

/// A claude's stdin as its `pump` holds it: the wire's own answers go through it while a `Lines` is kept.
pub struct Weak(std::sync::Weak<Sender<String>>);

impl Lines {
    /// Tests: lines that go to `tx`, no process, no thread (`Editor.claude_lines`).
    #[cfg(test)]
    pub fn to(tx: Sender<String>) -> Lines {
        Lines(Arc::new(tx))
    }

    /// What its `pump` holds.
    pub fn weak(&self) -> Weak {
        Weak(Arc::downgrade(&self.0))
    }

    pub fn new(mut stdin: std::process::ChildStdin) -> Lines {
        let (tx, rx) = channel::<String>();
        crate::event::thread("tarae-llm-write", move || {
            while let Ok(line) = rx.recv() {
                // (a failed write: the process is gone — `pump` tells its end)
                if writeln!(stdin, "{line}").and_then(|_| stdin.flush()).is_err() {
                    break;
                }
            }
        });
        Lines(Arc::new(tx))
    }

    /// Err: an earlier write failed — the process doesn't read anymore.
    pub fn send(&self, line: String) -> std::io::Result<()> {
        self.0.send(line).map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }
}

// ── Reading the streamed answer ─────────────────────────────────────────

/// How long streamed text and an edit's input are held to go with what comes after them (`pump`): a batch goes out
/// this long after the last one, whether another line came or not — a pause in the stream never holds what came
/// before it (measured, `cargo xtask stream`: the old rule, sent with the next line, held a delta 120 ms at the p90).
pub const FLUSH: Duration = Duration::from_millis(25);

/// Reader thread body: parses stdout line by line and sends closures built by `route` to the main loop.
/// With `once`, reaps the process after the first turn (ask). Otherwise runs until the process exits (chat).
/// stderr is drained separately (so a full pipe doesn't stall the child) — its last line is the error text.
/// stdout is read on a thread of its own (`tarae-llm-lines`), its lines handed over through a channel this one
/// waits on no longer than what it holds may wait (`FLUSH`). `own`: its stdin, where a control request no one
/// above the wire answers gets its `{}` (None: it asks none — a claude without tarae's tools).
pub fn pump(
    mut child: Child,
    own: Option<Weak>,
    once: bool,
    tx: crate::event::Tx,
    route: impl Fn(Event) -> Option<Apply> + Send + 'static,
) {
    super::process::UNREAPED.lock().unwrap_or_else(|e| e.into_inner()).push(child.id());
    crate::event::thread("tarae-llm-read", move || {
        // stderr's last line arrives when the pipe closes
        let (err_tx, err_rx) = channel::<String>();
        if let Some(err) = child.stderr.take() {
            crate::event::thread("tarae-llm-err", move || {
                let mut last = String::new();
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    if !line.trim().is_empty() {
                        last = line;
                    }
                }
                let _ = err_tx.send(last);
            });
        }
        let send = |ev: Event| route(ev).is_none_or(|apply| tx.send(crate::event::Event::Job(apply)).is_ok());
        // Streamed text not yet sent — answer text or (`pending_thought`) thinking summary — and an edit's
        // input as it's written (`writing`), sent in the same batches
        let mut pending = String::new();
        let mut pending_thought = false;
        let writing = std::cell::RefCell::new(crate::writing::Reading::default());
        let flush = |pending: &mut String, thought: bool| {
            let t = std::mem::take(pending);
            (t.is_empty() || send(if thought { Event::Thought(t) } else { Event::Text(t) }))
                && writing.borrow_mut().take().into_iter().all(|d| send(Event::Writing(d)))
        };
        let mut last_flush = crate::clock::now();
        let mut thinking_sent = false;
        // (the thinking block open in the message under way: its index)
        let mut thinking_block = None;
        let mut done = false;
        let mut model = String::new();
        let (line_tx, lines) = channel::<String>();
        match child.stdout.take() {
            Some(out) => {
                crate::event::thread("tarae-llm-lines", move || {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        if line_tx.send(line).is_err() {
                            break;
                        }
                    }
                });
            }
            // (no stdout: no lines — its end at once, the process reaped and its end told)
            None => drop(line_tx),
        }
        loop {
            // What's held goes out when its time is up, whether another line came or not
            let held = !pending.is_empty() || writing.borrow().holds();
            let left = FLUSH.saturating_sub(crate::clock::since(last_flush));
            if held && left.is_zero() {
                last_flush = crate::clock::now();
                if !flush(&mut pending, pending_thought) {
                    break;
                }
                continue;
            }
            let line = match held {
                true => lines.recv_timeout(left).map_err(|e| e == RecvTimeoutError::Disconnected),
                false => lines.recv().map_err(|_| true),
            };
            let line = match line {
                Ok(line) => line,
                Err(false) => continue,
                // (stdout closed: what it held goes, then its end is told)
                Err(true) => {
                    flush(&mut pending, pending_thought);
                    break;
                }
            };
            let Ok(ev) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
            // (a control request no one above the wire answers: `{}` from here, so claude never waits on it —
            // the only line written off the main loop)
            if let Some(id) = unanswered(&ev) {
                if let Some(stdin) = own.as_ref().and_then(|w| w.0.upgrade()) {
                    let _ = stdin.send(control_response(id, serde_json::json!({})));
                }
                continue;
            }
            // (what it costs — the same line may carry a tool call or the turn's end too)
            if let Some(u) = usage_line(&ev, crate::usage::now_ms())
                && !send(u)
            {
                break;
            }
            if let Some(m) = crate::billing::refusal(&ev)
                && !send(Event::Money(m))
            {
                break;
            }
            if let Some(m) = answered_by(&ev).filter(|m| *m != model) {
                model = m.to_string();
                if !send(Event::Answered(model.clone())) {
                    break;
                }
            }
            // An edit's input: read as it comes, sent at the batch's time — at once when its block ends
            if writing.borrow_mut().line(&ev) {
                let ended = ev["event"]["type"] == "content_block_stop";
                if ended || crate::clock::since(last_flush) >= FLUSH {
                    last_flush = crate::clock::now();
                    if !flush(&mut pending, pending_thought) {
                        break;
                    }
                }
                continue;
            }
            // Tool calls arrive whole in the assistant message — text streamed before them goes first
            let tools = tool_events(&ev);
            if !tools.is_empty() {
                if !flush(&mut pending, pending_thought) {
                    break;
                }
                thinking_sent = false;
                if !tools.into_iter().all(&send) {
                    break;
                }
                continue;
            }
            // A thinking block's start and end — its summary, held, goes before its end
            if let Some(open) = thinking_edge(&ev, &mut thinking_block) {
                if !flush(&mut pending, pending_thought) || !send(Event::ThinkingBlock(open)) {
                    break;
                }
                continue;
            }
            match parse_line(&ev) {
                Some(ev @ (Event::Text(_) | Event::Thought(_))) => {
                    let (t, thought) = match ev {
                        Event::Thought(t) => (t, true),
                        Event::Text(t) => (t, false),
                        _ => unreachable!(),
                    };
                    if thought != pending_thought {
                        if !flush(&mut pending, pending_thought) {
                            break;
                        }
                        pending_thought = thought;
                    }
                    pending.push_str(&t);
                    thinking_sent = thought;
                    if crate::clock::since(last_flush) >= FLUSH {
                        last_flush = crate::clock::now();
                        if !flush(&mut pending, pending_thought) {
                            break;
                        }
                    }
                }
                // Answered on the main loop · said where it happened (text streamed before it goes first)
                Some(c @ (Event::Mcp { .. } | Event::Permission { .. } | Event::Compaction(_))) => {
                    if !flush(&mut pending, pending_thought) || !send(c) {
                        break;
                    }
                }
                Some(e @ (Event::Conversation { .. } | Event::Ready { .. } | Event::Limits(_))) => {
                    if !send(e) {
                        break;
                    }
                }
                Some(Event::Thinking) if !thinking_sent => {
                    thinking_sent = true;
                    send(Event::Thinking);
                }
                Some(d @ Event::Done(_)) => {
                    flush(&mut pending, pending_thought);
                    thinking_sent = false;
                    let ok = send(d);
                    if once || !ok {
                        done = true;
                        break;
                    }
                }
                _ => {}
            }
        }
        if once {
            let _ = child.kill();
        }
        super::process::reap(&mut child);
        // Let stderr drain after exit (else the real error is lost) — bounded, in case a grandchild holds it
        let err = err_rx.recv_timeout(Duration::from_millis(500)).unwrap_or_default();
        if once && !done {
            let msg = if err.is_empty() { "exited without a result".to_string() } else { err };
            send(Event::Done(Err(msg)));
        } else if !once {
            send(Event::Exit(err));
        }
    });
}

/// `tool_use` blocks of a complete assistant message · `tool_result` blocks of the user message that
/// answers them.
fn tool_events(ev: &serde_json::Value) -> Vec<Event> {
    let Some(content) = ev["message"]["content"].as_array() else { return Vec::new() };
    let id = |b: &serde_json::Value, k: &str| b[k].as_str().unwrap_or_default().to_string();
    match ev["type"].as_str() {
        Some("assistant") => content
            .iter()
            .filter(|b| b["type"] == "tool_use")
            .map(|b| Event::Tool { id: id(b, "id"), name: id(b, "name"), input: b["input"].clone() })
            .collect(),
        Some("user") => (content.iter())
            .filter(|b| b["type"] == "tool_result")
            .map(|b| {
                // A string, or text blocks
                let mut text = match &b["content"] {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Array(parts) => {
                        parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n")
                    }
                    _ => String::new(),
                };
                if text.len() > 4096 {
                    let cut = (0..=4096).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
                    text.truncate(cut);
                }
                let error = b["is_error"].as_bool() == Some(true);
                Event::ToolResult { id: id(b, "tool_use_id"), text, error }
            })
            // (and a WebSearch's list, whole — its own line's `tool_use_result`)
            .chain(crate::web::search::found(ev).map(|f| Event::Searched(Box::new(f))))
            .collect(),
        _ => Vec::new(),
    }
}

/// The model that wrote this answer, if the line says — not claude's own stand-ins (`<synthetic>`, for an
/// answer it made up after an error).
fn answered_by(ev: &serde_json::Value) -> Option<&str> {
    let m = match ev["type"].as_str()? {
        "assistant" => &ev["message"]["model"],
        "stream_event" if ev["event"]["type"] == "message_start" => &ev["event"]["message"]["model"],
        _ => return None,
    };
    m.as_str().filter(|m| !m.is_empty() && !m.starts_with('<'))
}

/// What a line says about usage (`usage.rs` has what's where), the plan's limits or a retry (`limits.rs`), read
/// at `at_ms` — None: nothing.
pub fn usage_line(ev: &serde_json::Value, at_ms: u64) -> Option<Event> {
    use crate::usage::{Counts, Seen, Spent, Ttl};
    let seen = |id: Option<&serde_json::Value>, model: &serde_json::Value, u: &serde_json::Value| {
        Some(Event::Usage(Seen {
            id: match id {
                Some(id) => Some(id.as_str()?.to_string()),
                None => None,
            },
            model: model.as_str().unwrap_or_default().to_string(),
            counts: Some(Counts::of(u)).filter(Counts::any)?,
            ttl: Ttl::of(u),
            at_ms,
        }))
    };
    match ev["type"].as_str()? {
        "system" if ev["subtype"] == "status" && ev["status"] == "requesting" => {
            Some(Event::Requesting(at_ms))
        }
        "system" if ev["subtype"] == "api_retry" => crate::limits::of_retry(ev).map(Event::Retry),
        "rate_limit_event" => crate::limits::of_event(ev, at_ms).map(Event::Limits),
        "stream_event" => {
            let e = &ev["event"];
            match e["type"].as_str()? {
                "message_start" => {
                    seen(Some(&e["message"]["id"]), &e["message"]["model"], &e["message"]["usage"])
                }
                "message_delta" => seen(None, &serde_json::Value::Null, &e["usage"]),
                _ => None,
            }
        }
        // (claude's own stand-in after an error, `<synthetic>`, cost nothing)
        "assistant" if !ev["message"]["model"].as_str().unwrap_or_default().starts_with('<') => {
            let m = &ev["message"];
            seen(Some(&m["id"]), &m["model"], &m["usage"])
        }
        "result" => {
            let models = ev["modelUsage"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, m)| (name.clone(), m["contextWindow"].as_u64()))
                .collect();
            Some(Event::Spent(Spent {
                conversation: ev["session_id"].as_str().unwrap_or_default().to_string(),
                counts: Counts::of(&ev["usage"]),
                total_cost: ev["total_cost_usd"].as_f64(),
                models,
                ms: ev["duration_ms"].as_u64(),
                at_ms,
            }))
        }
        _ => None,
    }
}

/// `args` with `flag`'s value replaced (the flag appended if it wasn't there).
fn with_flag(args: &[String], flag: &str, value: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len() + 2);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == flag {
            it.next();
        } else {
            out.push(a.clone());
        }
    }
    out.extend([flag.to_string(), value.to_string()]);
    out
}

/// `args` with its `--tools <list>` swapped for `tools` (read-only ones for the chat panel), and `allowed`
/// (those + tarae's own MCP tools) allowed up front, so `-p` never stops to ask.
pub fn with_tools(args: &[String], tools: &str, allowed: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len() + 4);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--tools" {
            it.next();
        } else {
            out.push(a.clone());
        }
    }
    out.extend(["--tools", tools, "--allowedTools", allowed].map(String::from));
    out
}

/// Whether a stream line starts a thinking block (true) or ends the one open (`open`: its index, kept here).
fn thinking_edge(ev: &serde_json::Value, open: &mut Option<u64>) -> Option<bool> {
    if ev["type"] != "stream_event" {
        return None;
    }
    let e = &ev["event"];
    let index = e["index"].as_u64().unwrap_or(0);
    match e["type"].as_str()? {
        "content_block_start"
            if matches!(e["content_block"]["type"].as_str(), Some("thinking" | "redacted_thinking")) =>
        {
            *open = Some(index);
            Some(true)
        }
        "content_block_stop" if *open == Some(index) => {
            *open = None;
            Some(false)
        }
        _ => None,
    }
}

/// A control request of claude's that comes up as no event (another subtype, another server's message): its id,
/// for the wire's own `{}`.
fn unanswered(ev: &serde_json::Value) -> Option<&str> {
    (ev["type"] == "control_request" && parse_line(ev).is_none()).then(|| ev["request_id"].as_str()).flatten()
}

pub(crate) fn parse_line(ev: &serde_json::Value) -> Option<Event> {
    match ev["type"].as_str()? {
        // tarae's MCP server's messages and permission asks — any other is the wire's (`unanswered`)
        "control_request" => {
            let (id, req) = (ev["request_id"].as_str()?.to_string(), &ev["request"]);
            match req["subtype"].as_str()? {
                "can_use_tool" => Some(Event::Permission {
                    id,
                    tool: req["tool_name"].as_str().unwrap_or_default().to_string(),
                    input: req["input"].clone(),
                }),
                "mcp_message" if req["server_name"] == "tarae" => {
                    Some(Event::Mcp { id, message: req["message"].clone() })
                }
                _ => None,
            }
        }
        "stream_event" => {
            let delta = &ev["event"]["delta"];
            match delta["type"].as_str()? {
                "text_delta" => Some(Event::Text(delta["text"].as_str()?.to_string())),
                "thinking_delta" => Some(match delta["thinking"].as_str().filter(|t| !t.is_empty()) {
                    Some(t) => Event::Thought(t.to_string()),
                    None => Event::Thinking,
                }),
                _ => None,
            }
        }
        "system" if ev["subtype"] == "thinking_tokens" => Some(Event::Thinking),
        // (`status: "requesting"` is usage's — `usage_line`)
        "system" if ev["subtype"] == "compact_boundary" || ev["subtype"] == "status" => {
            crate::compaction::Ev::of(ev, crate::usage::now_ms()).map(Event::Compaction)
        }
        "system" if ev["subtype"] == "init" => Some(Event::Conversation {
            id: ev["session_id"].as_str()?.to_string(),
            model: ev["model"].as_str().unwrap_or_default().to_string(),
            version: ev["claude_code_version"].as_str().filter(|v| !v.is_empty()).map(String::from),
            key_source: ev["apiKeySource"].as_str().map(String::from),
        }),
        "control_response" if ev["response"]["request_id"] == "tarae-init" => {
            let r = &ev["response"]["response"];
            Some(Event::Ready {
                models: ModelInfo::list(&r["models"]),
                key_source: r["account"]["apiKeySource"].as_str().map(String::from),
                account: crate::usage::Account::of(&r["account"]),
            })
        }
        "control_response" if ev["response"]["request_id"] == REQUEST_ID => {
            Some(Event::Limits(crate::limits::of_answer(&ev["response"], crate::usage::now_ms())))
        }
        "result" => {
            let text = ev["result"].as_str().unwrap_or_default().to_string();
            let failed = ev["is_error"].as_bool() == Some(true)
                || ev["subtype"].as_str().is_some_and(|s| s.starts_with("error"));
            Some(Event::Done(if failed {
                Err(if text.is_empty() { "stopped".into() } else { text })
            } else {
                Ok(text)
            }))
        }
        _ => None,
    }
}

/// Tests: claude's control request (`request` its body — an `mcp_message`, a `can_use_tool`) as the wire hands
/// it up, decoded from its line as one from claude is.
#[cfg(test)]
pub mod test {
    pub fn control(request_id: &str, request: serde_json::Value) -> crate::agent::Event {
        let line =
            serde_json::json!({"type": "control_request", "request_id": request_id, "request": request});
        super::parse_line(&line).expect("a control request the wire hands up")
    }

    /// A permission's answer as the wire sends it to claude.
    pub fn answer(p: crate::agent::Permission) -> serde_json::Value {
        super::permission(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::spawn_failed;

    /// Agents beside Claude, step 2's finding A: a start whose first lines can't be written (here: no stdin to write
    /// them to) comes back as its error, said, and its child — which no reader took — is ended and reaped, never left
    /// a zombie. The stand-in would write on and on; once its stdout is dropped unread it dies of it, but only a
    /// reap takes it out of the process table.
    #[test]
    fn a_start_whose_first_write_fails_reaps_its_child_and_says_why() {
        let child = Command::new("/bin/sh")
            .args(["-c", "exec yes"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        let ed = crate::editor::Editor::new(crate::config::Config::default());
        let first = First { tools: true, prompt: Some(Prompt::Text("hi".into())) };
        let Err(e) = begin(child, first, true, ed.events.sender(), |_| None) else { panic!("it started") };
        assert_eq!(e.said("claude", false), "claude: no stdin");
        let end = std::time::Instant::now() + crate::test_results::PATIENCE;
        // (signal 0: whether the pid is still in the process table — a zombie is, a reaped child isn't)
        // SAFETY: signal 0 sends nothing, it only asks.
        while unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
            assert!(std::time::Instant::now() < end, "ended and reaped");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A claude with no stdout to read: the reader ends at once — the process reaped, its end told — never waits
    /// on lines that can't come.
    #[test]
    fn a_claude_without_stdout_is_reaped_and_its_end_told() {
        let child = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        let (tx, rx) = channel::<bool>();
        let ed = crate::editor::Editor::new(crate::config::Config::default());
        pump(child, None, false, ed.events.sender(), move |ev| {
            let _ = tx.send(matches!(ev, Event::Exit(_)));
            None
        });
        assert_eq!(rx.recv_timeout(crate::test_results::PATIENCE), Ok(true), "its end told");
        assert!(!super::super::process::UNREAPED.lock().unwrap().contains(&pid), "reaped");
    }

    /// Agents beside Claude, condition 3: a control request no one above the wire answers (another subtype, another
    /// server's MCP message) is answered `{}` by the wire itself, through its pump's hold on stdin, and comes up as
    /// no event; and that hold doesn't keep stdin open: the handle dropped, the stand-in reads its end.
    #[test]
    fn the_wire_answers_what_no_one_above_it_does_and_a_dropped_handle_closes_stdin() {
        let dir = crate::scratch::Scratch::new("claude-wire-answers");
        let script = r#"printf '%s\n' '{"type":"control_request","request_id":"x1","request":{"subtype":"hook_callback"}}'
printf '%s\n' '{"type":"control_request","request_id":"x2","request":{"subtype":"mcp_message","server_name":"other","message":{}}}'
read -r a; read -r b; printf '%s\n%s\n' "$a" "$b" > "$1/answers"
while read -r l; do :; done
echo closed > "$1/eof""#;
        let mut child = Command::new("/bin/sh")
            .args(["-c", script, "sh"])
            .arg(dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let lines = Lines::new(child.stdin.take().unwrap());
        let (tx, rx) = channel::<String>();
        let ed = crate::editor::Editor::new(crate::config::Config::default());
        pump(child, Some(lines.weak()), false, ed.events.sender(), move |ev| {
            let said = match ev {
                Event::Mcp { id, .. } | Event::Permission { id, .. } => format!("up {id}"),
                Event::Exit(_) => "exit".to_string(),
                _ => return None,
            };
            let _ = tx.send(said);
            None
        });
        let read = |name: &str| {
            let end = std::time::Instant::now() + crate::test_results::PATIENCE;
            loop {
                if let Ok(text) = std::fs::read_to_string(dir.join(name))
                    && text.ends_with('\n')
                {
                    return text;
                }
                assert!(std::time::Instant::now() < end, "the stand-in wrote no {name}");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let answers: Vec<serde_json::Value> =
            read("answers").lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        for (a, id) in answers.iter().zip(["x1", "x2"]) {
            assert_eq!(a["type"], "control_response", "{a}");
            assert_eq!(a["response"]["request_id"], id, "{a}");
            assert_eq!(a["response"]["response"], serde_json::json!({}), "{a}");
        }
        assert!(std::fs::read_to_string(dir.join("eof")).is_err(), "stdin open while the handle is kept");
        drop(lines);
        assert_eq!(read("eof"), "closed\n", "the handle dropped: stdin closed");
        assert_eq!(rx.recv_timeout(crate::test_results::PATIENCE).as_deref(), Ok("exit"), "no event came up");
    }

    /// A delta that came within `FLUSH` of the last batch goes out when its time is up — never held for the next
    /// line, however long the stream pauses before it: text and an edit's input alike.
    #[test]
    fn a_held_delta_goes_out_by_its_time_not_with_the_next_line() {
        let delta = |index: u8, delta: serde_json::Value| {
            serde_json::json!({"type": "stream_event", "event": {"type": "content_block_delta", "index": index,
                "delta": delta}})
            .to_string()
        };
        let text = |t: &str| delta(0, serde_json::json!({"type": "text_delta", "text": t}));
        let input = |j: &str| delta(1, serde_json::json!({"type": "input_json_delta", "partial_json": j}));
        let start = serde_json::json!({"type": "stream_event", "event": {"type": "content_block_start",
            "index": 1, "content_block": {"type": "tool_use", "id": "w1", "name": "mcp__tarae__edit", "input": {}}}});
        // (four lines at once, a pause of 2 s, the next)
        let mut child = Command::new("/bin/sh")
            .args(["-c", r#"printf '%s\n' "$1" "$2" "$3" "$4" "$5"; sleep 2; printf '%s\n' "$6""#, "sh"])
            .args([
                text("one"),
                text("two"),
                start.to_string(),
                input(r#"{"edits": [{"file_path": "a.rs", "new_string": "x"#),
                input("y"),
                text("three"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (tx, rx) = channel::<(std::time::Instant, String)>();
        let ed = crate::editor::Editor::new(crate::config::Config::default());
        let _ = child.stdin.take();
        pump(child, None, false, ed.events.sender(), move |ev| {
            let said = match ev {
                Event::Text(t) => format!("text {t}"),
                Event::Writing(d) => d
                    .pieces
                    .iter()
                    .map(|p| match p {
                        crate::writing::Piece::Text { add, .. } => format!("input {add}"),
                        _ => String::new(),
                    })
                    .collect(),
                _ => return None,
            };
            let _ = tx.send((std::time::Instant::now(), said));
            None
        });
        let mut got: Vec<(std::time::Instant, String)> = Vec::new();
        while !got.iter().any(|g| g.1.contains("three")) {
            got.push(rx.recv_timeout(crate::test_results::PATIENCE).expect("the stream's lines"));
        }
        // (what came at once may go in one batch: `text onetwo`, `input xy`)
        let at = |kind: &str, end: &str| {
            let g = got.iter().find(|g| g.1.starts_with(kind) && g.1.ends_with(end));
            g.map(|g| g.0).unwrap_or_else(|| panic!("{kind} …{end}: {got:?}"))
        };
        let three = at("text", "three");
        for (kind, end) in [("text", "two"), ("input", "y")] {
            let sent = at(kind, end);
            assert!(
                sent + Duration::from_secs(1) < three,
                "{kind} …{end} went out before the pause ended: {got:?}"
            );
        }
    }

    /// A thinking block's lines as the reader sends them, in their order: its start, its summary (held, then sent)
    /// before its end — the turn's word waves until the summary lands (`shimmer.rs`).
    #[test]
    fn a_thinking_blocks_summary_goes_before_its_end() {
        let ev = |e: serde_json::Value| serde_json::json!({"type": "stream_event", "event": e}).to_string();
        let lines = [
            ev(serde_json::json!({"type": "content_block_start", "index": 0,
                "content_block": {"type": "thinking", "thinking": "", "signature": ""}})),
            ev(serde_json::json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "thinking_delta", "thinking": "Checking "}})),
            ev(serde_json::json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "thinking_delta", "thinking": "the map."}})),
            ev(serde_json::json!({"type": "content_block_stop", "index": 0})),
            ev(serde_json::json!({"type": "content_block_delta", "index": 1,
                "delta": {"type": "text_delta", "text": "It clones."}})),
        ];
        let mut child = Command::new("/bin/sh")
            .args(["-c", r#"printf '%s\n' "$@""#, "sh"])
            .args(lines)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (tx, rx) = channel::<String>();
        let ed = crate::editor::Editor::new(crate::config::Config::default());
        let _ = child.stdin.take();
        pump(child, None, false, ed.events.sender(), move |ev| {
            let said = match ev {
                Event::ThinkingBlock(open) => format!("block {}", if open { "open" } else { "closed" }),
                Event::Thought(t) => format!("thought {t}"),
                Event::Text(t) => format!("text {t}"),
                _ => return None,
            };
            let _ = tx.send(said);
            None
        });
        let mut got = Vec::new();
        while !got.iter().any(|g: &String| g.starts_with("text")) {
            got.push(rx.recv_timeout(crate::test_results::PATIENCE).expect("the stream's lines"));
        }
        // (the summary in one batch or two, as the reader's time falls)
        let mut kinds: Vec<&str> = got.iter().map(|g| g.split(' ').next().unwrap_or_default()).collect();
        kinds.dedup();
        assert_eq!(kinds, ["block", "thought", "block", "text"], "{got:?}");
        assert_eq!((got[0].as_str(), got[got.len() - 2].as_str()), ("block open", "block closed"));
        let thought: String = got.iter().filter_map(|g| g.strip_prefix("thought ")).collect();
        assert_eq!(thought, "Checking the map.");
    }

    /// Tests never start the real claude (card model P2 caught one reading ~/.claude.json and the keychain):
    /// the default command, by name or path, is refused before anything starts.
    #[test]
    #[should_panic(expected = "a test started the real claude")]
    fn a_test_never_starts_the_real_claude() {
        // (a path that isn't there: without the guard, nothing starts and nothing panics)
        let cfg = LlmConfig { command: "/nonexistent/claude".into(), ..LlmConfig::default() };
        let _ = spawn(&cfg, &[], &std::env::temp_dir(), None, &Credential::Subscription);
    }

    /// No claude: Claude Code isn't installed — how to get it and log in, as its docs say (#163); any other
    /// failure as it is.
    #[test]
    fn a_missing_claude_says_how_to_get_it() {
        let gone = std::io::Error::from(std::io::ErrorKind::NotFound);
        for command in ["claude", "/opt/x/claude"] {
            let whole = spawn_failed(command, &gone, true);
            assert!(whole.starts_with("Claude Code isn't installed"), "{whole}");
            assert!(whole.contains("curl -fsSL https://claude.ai/install.sh | bash"), "{whole}");
            assert!(whole.contains("log in by running `claude`"), "{whole}");
            assert!(!whole.contains("os error"), "{whole}");
            let one = spawn_failed(command, &gone, false);
            assert!(!one.contains('\n') && one.contains("install.sh"), "a toast's one line: {one}");
        }
        assert_eq!(spawn_failed("my-agent", &gone, true), "my-agent isn't on PATH (llm.command)");
        assert_eq!(spawn_failed("/x/my-agent", &gone, true), "/x/my-agent isn't there (llm.command)");
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(spawn_failed("claude", &denied, true).starts_with("claude: "), "not a missing one");
    }

    /// Every claude tarae starts has its own updates off (`DISABLE_UPDATES` — its updater writes a folder of programs
    /// on PATH, closed to its profile) and its git's optional locks off (`.git` isn't its to write) — over what it
    /// would inherit, an environment that says otherwise included.
    #[test]
    fn every_claude_tarae_starts_has_its_updates_off() {
        let cfg = LlmConfig {
            command: "sh".into(),
            args: vec!["-c".into(), "printf '%s %s' \"$DISABLE_UPDATES\" \"$GIT_OPTIONAL_LOCKS\"".into()],
            ..Default::default()
        };
        let given = Env::from([("DISABLE_UPDATES".to_string(), "0".to_string())]);
        let sub = Credential::Subscription;
        let child = spawn(&cfg, &[], &std::env::temp_dir(), Some(&given), &sub).unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "1 0");
    }

    /// Every claude tarae starts has Claude Code's auto memory off and no team memory stores — over what it would
    /// inherit, an environment that turns memory back on included (threat model G21).
    #[test]
    fn every_claude_tarae_starts_has_its_memory_off() {
        let script = "printf '%s %s' \"$CLAUDE_CODE_DISABLE_AUTO_MEMORY\" \"${CLAUDE_MEMORY_STORES-unset}\"";
        let cfg =
            LlmConfig { command: "sh".into(), args: vec!["-c".into(), script.into()], ..Default::default() };
        let given = Env::from([
            ("CLAUDE_CODE_DISABLE_AUTO_MEMORY".to_string(), "0".to_string()),
            ("CLAUDE_MEMORY_STORES".to_string(), "team".to_string()),
        ]);
        let sub = Credential::Subscription;
        let child = spawn(&cfg, &[], &std::env::temp_dir(), Some(&given), &sub).unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "1 unset");
    }

    /// Every claude tarae starts is without `SSH_AUTH_SOCK` — whether it would inherit it from tarae's own
    /// environment or be given it — so it can't have your SSH agent sign with your keys (threat model G20).
    #[test]
    fn every_claude_tarae_starts_has_no_ssh_agent() {
        let cfg = LlmConfig {
            command: "sh".into(),
            args: vec!["-c".into(), "printf '%s' \"${SSH_AUTH_SOCK-unset}\"".into()],
            ..Default::default()
        };
        // (given, as `given` passes tarae's environment on; the test binary's own isn't changed — a stand-in path)
        let given = Env::from([("SSH_AUTH_SOCK".to_string(), "/tarae-test/agent.sock".to_string())]);
        let sub = Credential::Subscription;
        let child = spawn(&cfg, &[], &std::env::temp_dir(), Some(&given), &sub).unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "unset");
    }

    /// Every claude tarae starts is without your wiki's token (`web.signed-in`'s variable) — inherited from tarae's
    /// environment or given — so neither it, its children nor what they print has it: the chat, the helpers and the
    /// web search's all start through `command`.
    #[test]
    fn every_claude_tarae_starts_has_no_wiki_token() {
        let t: toml::Table = toml::from_str(
            r#"v = [{ base = "https://corp.example/w", kind = "confluence-dc", token-env = "TARAE_TEST_PAT" }]"#,
        )
        .unwrap();
        let cfg = LlmConfig {
            command: "sh".into(),
            args: vec!["-c".into(), "printf '%s' \"${TARAE_TEST_PAT-unset}\"".into()],
            signed_in: crate::web::signed::wikis_of(&t["v"]).unwrap(),
            ..Default::default()
        };
        let given = Env::from([("TARAE_TEST_PAT".to_string(), "tarae-fake-token".to_string())]);
        let sub = Credential::Subscription;
        let child = spawn(&cfg, &[], &std::env::temp_dir(), Some(&given), &sub).unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "unset");
    }

    /// Every claude tarae starts is its own program, `llm.command`, as every other tool runs it — no sandbox of
    /// its own wrapped around it (user decision 2026-10-17: V1d and claudeown B reversed); what keeps it is its
    /// tools going through tarae.
    #[test]
    fn every_claude_starts_as_its_own_program() {
        let dir = std::env::temp_dir();
        let cfg = LlmConfig { command: "tarae-stand-in-claude".into(), ..Default::default() };
        let s = crate::agent::Start::new(crate::agent::Role::Draft, &cfg);
        let cmd = command(&s, &cfg.args, &[], &dir, None, &Credential::Subscription).unwrap();
        assert_eq!(cmd.get_program(), "tarae-stand-in-claude");
        let args: Vec<_> = cmd.get_args().collect();
        assert!(!args.iter().any(|a| *a == "tarae-stand-in-claude"), "nothing before it: {args:?}");
    }

    /// A thinking block's start and its own end are told (the turn's word waves meanwhile — `shimmer.rs`); another
    /// block's are not.
    #[test]
    fn a_thinking_blocks_start_and_end_are_told() {
        let line = |e: serde_json::Value| serde_json::json!({"type": "stream_event", "event": e});
        let start = |i: u64, kind: &str| {
            line(serde_json::json!({"type": "content_block_start", "index": i,
                "content_block": {"type": kind, "thinking": "", "signature": ""}}))
        };
        let stop = |i: u64| line(serde_json::json!({"type": "content_block_stop", "index": i}));
        let mut open = None;
        assert_eq!(thinking_edge(&stop(0), &mut open), None, "no block open");
        assert_eq!(thinking_edge(&start(0, "thinking"), &mut open), Some(true));
        let delta = line(serde_json::json!({"type": "content_block_delta", "index": 0,
            "delta": {"type": "thinking_delta", "thinking": "Checking"}}));
        assert_eq!(thinking_edge(&delta, &mut open), None);
        assert_eq!(thinking_edge(&stop(1), &mut open), None, "another block's end");
        assert_eq!(thinking_edge(&stop(0), &mut open), Some(false));
        assert_eq!(thinking_edge(&start(1, "text"), &mut open), None);
        assert_eq!(thinking_edge(&stop(1), &mut open), None, "a text block's end");
        assert_eq!(thinking_edge(&start(2, "redacted_thinking"), &mut open), Some(true));
        assert_eq!(open, Some(2));
    }

    #[test]
    fn tool_calls_come_from_assistant_messages() {
        let ev = serde_json::json!({"type": "assistant", "message": {"content": [
            {"type": "thinking", "thinking": ""},
            {"type": "tool_use", "name": "Read", "input": {"file_path": "/p/src/lib.rs", "offset": 10}},
            {"type": "tool_use", "name": "Grep", "input": {"pattern": "fn add"}},
        ]}});
        let got: Vec<(String, serde_json::Value)> = tool_events(&ev)
            .into_iter()
            .map(|e| match e {
                Event::Tool { name, input, .. } => (name, input),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, "Read");
        assert_eq!(got[0].1["offset"], 10);
        assert_eq!(got[1].1["pattern"], "fn add");
        let text = serde_json::json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "hi"}]}});
        assert!(tool_events(&text).is_empty());
        let result = serde_json::json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "a.rs:3:fn a"}]}]}});
        assert!(matches!(&tool_events(&result)[..],
            [Event::ToolResult { id, text, error: false }] if id == "t1" && text == "a.rs:3:fn a"));
        // (a WebSearch's: its list too, whole — the web search's helper's)
        let searched = serde_json::json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "t2", "content": "Web search results for query: \"q\""}]},
            "tool_use_result": {"query": "q", "results": [{"content": [{"title": "A", "url": "https://a.test/"}]}]}});
        assert!(matches!(&tool_events(&searched)[..],
            [Event::ToolResult { id, .. }, Event::Searched(f)] if id == "t2" && f.links[0].url == "https://a.test/"));
        // Summarized thinking carries text; the omitted kind is just a signal
        let delta = |t: &str| {
            serde_json::json!({"type": "stream_event", "event": {"type": "content_block_delta",
            "delta": {"type": "thinking_delta", "thinking": t}}})
        };
        assert!(matches!(parse_line(&delta("hmm")), Some(Event::Thought(t)) if t == "hmm"));
        assert!(matches!(parse_line(&delta("")), Some(Event::Thinking)));
    }

    #[test]
    fn flags_are_replaced_not_repeated() {
        let args = with_flag(&LlmConfig::default().args, "--setting-sources", "user,project,local");
        let at = args.iter().position(|a| a == "--setting-sources").unwrap();
        assert_eq!(args[at + 1], "user,project,local");
        assert_eq!(args.iter().filter(|a| *a == "--setting-sources").count(), 1);
        assert_eq!(with_flag(&[], "--x", "1"), ["--x", "1"]);
    }

    #[test]
    fn chat_tools_replace_the_no_tools_default() {
        let args =
            with_tools(&LlmConfig::default().args, "Read,Grep,Glob", "Read,Grep,Glob,mcp__tarae__note");
        let at = |f: &str| args.iter().position(|a| a == f).unwrap();
        assert_eq!(args.iter().filter(|a| *a == "--tools").count(), 1);
        assert_eq!(args[at("--tools") + 1], "Read,Grep,Glob");
        assert_eq!(args[at("--allowedTools") + 1], "Read,Grep,Glob,mcp__tarae__note");
        assert!(args.windows(2).all(|w| w[0] != "--setting-sources" || w[1].is_empty()), "rest kept");
    }
}
