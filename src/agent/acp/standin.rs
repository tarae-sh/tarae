//! The stand-in ACP agent (step 2's plan §5): the test binary re-run on one `#[ignore]`d test (`run`), started by
//! the stand-in's row (`agent::STAND_IN`) as `sh -c 'exec "$@" 3>&1 >/dev/null' sh <test binary> --exact
//! agent::acp::standin::run --ignored --nocapture`: libtest's own lines go to /dev/null, and it speaks ACP on
//! descriptor 3, tarae's pipe. It reads its script from `SCRIPT` (a file in the test's scratch folder) and writes
//! what it heard to `HEARD`, a JSON line each, for the test to read. It waits only on what it reads: no network,
//! no real agent, no wait on time.
//!
//! A script is a JSON array of steps, each an object:
//! - `{"expect": method}` waits for that request or notification (what came before it is heard, kept for a later
//!   step); a request is answered with the step's `result`, or its `error` (a code) and `message` — one with neither
//!   is held as the turn, for `end_turn`.
//! - `{"say": update}` a `session/update`; `{"ask": toolCall, "id": id, "options": [...]}` a permission ask (agy's
//!   two options unless given); `{"await": id}` waits for the answer to it.
//! - `{"end_turn": stopReason, "usage": {...}}` answers the turn held; `{"answer": result}` any request held.
//!   ("Held": the last request an `expect` didn't answer.)
//! - `{"send": message}` any message as it is (a request of the agent's own).
//! - `{"stderr": line}`, `{"exit": code}`, `{"junk": text}` (a line that isn't JSON), `{"eof": true}` (reads until
//!   stdin closes, heard as `{"eof": true}`).
//! - `{"mcp": message}` sends an MCP message to tarae's server — the first of the `mcpServers` its session was opened
//!   with, started at the first such step as an agent starts one (its command, arguments and `env`), heard as
//!   `{"mcp_pid": pid}` — and, a request, waits for its answer, heard as `{"mcp_answer": answer}` (anything else
//!   the server said meanwhile as `{"mcp_other": message}`).
//! - `{"linger": true}` leaves a process of its own in its group that outlives it (a stand-in for an agent's child
//!   that never ends by itself), heard as `{"linger_pid": pid}`.
//! - `{"tty": true}` reads its session and group and tries to open `/dev/tty`, heard as `{"pid": pid, "pgid":
//!   pgid, "sid": sid, "tty": "opened" or why not}`.
//! - `{"own": {"name": …, "args": …, "id": …}}` runs a tool as agy 1.5.9 does (`own`): reported, its folder's hooks
//!   asked on agy's input (heard as `{"hook": their answer}`), then `failed` where they refused or failed, else the
//!   ask (`pending`, `session/request_permission`, its answer waited for) and, on `allow_once`, the call and
//!   `completed` — with `"mcp": message`, one of tarae's: the message sent to tarae's server (as an `mcp` step) and its
//!   answer waited for, as agy calls a server's tool. With `NO_ASK` set, no ask: agy 1.5.4 and 1.5.7.
//!
//! It plays agy's part, so its row takes agy's recipe (`agent::AGY_TOOLS_OFF`): its `--gemini-dir <folder>`, which
//! libtest wouldn't take, is moved by its `sh` into `DIR` before the test binary starts.
//!
//! What it heard: first `{"env": [its environment's names], "nofile": its soft descriptor limit, "dir": its folder}`,
//! then each line tarae wrote, as it came.

use std::collections::VecDeque;
use std::io::{BufRead, Write};

use serde_json::{Value, json};

/// The stand-in's row's word for the test binary's path (`acp::command` puts it there).
pub const SELF: &str = "@self";

/// The test the stand-in is (`run`).
pub const TEST: &str = "agent::acp::standin::run";

/// The stand-in's row's arguments to `sh`: `--gemini-dir <folder>` taken out of what follows and put in `DIR`, the
/// rest exec'd.
pub const ARGS: &[&str] = &[
    "-c",
    concat!(
        "d=; for a in \"$@\"; do shift; ",
        "if [ \"$d\" = 1 ]; then TARAE_ACP_DIR=$a; export TARAE_ACP_DIR; d=; ",
        "elif [ \"$a\" = --gemini-dir ]; then d=1; else set -- \"$@\" \"$a\"; fi; done; ",
        "exec \"$@\" 3>&1 >/dev/null"
    ),
    "sh",
    SELF,
    "--exact",
    TEST,
    "--ignored",
    "--nocapture",
];

/// The variable its `sh` puts its folder in (`ARGS`).
pub const DIR: &str = "TARAE_ACP_DIR";

/// The variable naming its script.
pub const SCRIPT: &str = "TARAE_ACP_SCRIPT";
/// The variable naming the file it writes what it heard to.
pub const HEARD: &str = "TARAE_ACP_HEARD";
/// The variable that, set (to anything), makes it play agy 1.5.4 and 1.5.7, which never asked for a call its hooks
/// let through (`own`) — a switch of the stand-in's, as `SCRIPT` and `HEARD` are: its row's `ARGS` are every test's.
pub const NO_ASK: &str = "TARAE_ACP_NO_ASK";

/// How long a hook may take where its entry names no `timeout` (agy's own default, unmeasured).
const HOOK_TIMEOUT: f64 = 30.0;

/// agy's two options for a permission ask, as the probe saw them.
pub fn agy_options() -> Value {
    json!([
        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
        {"optionId": "deny", "name": "Deny", "kind": "reject_once"},
    ])
}

struct Agent {
    out: std::fs::File,
    heard: std::fs::File,
    lines: std::io::Lines<std::io::StdinLock<'static>>,
    /// Heard and not yet taken by a step.
    unread: VecDeque<Value>,
    session: String,
    turn: Option<Value>,
    /// The `mcpServers` its session was opened with.
    servers: Value,
    /// tarae's server once started: its stdin and its stdout's lines.
    server: Option<(std::process::ChildStdin, std::io::Lines<std::io::BufReader<std::process::ChildStdout>>)>,
    /// What it started, never waited for: its group's end is tarae's to make (condition 4).
    children: Vec<std::process::Child>,
    /// Its session's folder, and the last message's text (a hook's `lastUserInput`).
    cwd: String,
    last_input: String,
    /// The asks it made of its own (`own`), for their ids.
    asks: u64,
}

impl Agent {
    fn say(&mut self, v: Value) {
        let _ = writeln!(self.out, "{v}").and_then(|_| self.out.flush());
    }

    fn heard(&mut self, v: &Value) {
        let _ = writeln!(self.heard, "{v}").and_then(|_| self.heard.flush());
    }

    /// The next line from tarae, heard. None: stdin closed.
    fn hear(&mut self) -> Option<Value> {
        loop {
            let line = self.lines.next()?.ok()?;
            match serde_json::from_str::<Value>(&line) {
                Ok(v) => {
                    self.heard(&v);
                    return Some(v);
                }
                Err(_) => self.heard(&json!({"junk": line})),
            }
        }
    }

    /// The first message heard that `f` takes, waiting for it. None: stdin closed first.
    fn wait(&mut self, f: impl Fn(&Value) -> bool) -> Option<Value> {
        if let Some(at) = self.unread.iter().position(&f) {
            return self.unread.remove(at);
        }
        loop {
            let v = self.hear()?;
            if f(&v) {
                return Some(v);
            }
            self.unread.push_back(v);
        }
    }

    fn missed(&mut self, what: &str) -> ! {
        self.heard(&json!({"missed": what}));
        eprintln!("stand-in: stdin closed before {what}");
        std::process::exit(3);
    }

    fn step(&mut self, step: &Value) {
        if let Some(method) = step["expect"].as_str() {
            let Some(v) = self.wait(|v| v["method"] == method) else { self.missed(method) };
            if v["params"]["mcpServers"].is_array() {
                self.servers = v["params"]["mcpServers"].clone();
            }
            if let Some(cwd) = v["params"]["cwd"].as_str() {
                self.cwd = cwd.to_string();
            }
            if let Some(text) = v["params"]["prompt"].as_array().and_then(|p| p.last()?["text"].as_str()) {
                self.last_input = text.to_string();
            }
            if let Some(id) = v.get("id").filter(|i| !i.is_null()) {
                if let Some(s) = v["params"]["sessionId"].as_str() {
                    self.session = s.to_string();
                }
                if let Some(code) = step["error"].as_i64() {
                    let message = step["message"].as_str().unwrap_or("refused");
                    let error = json!({"code": code, "message": message});
                    self.say(json!({"jsonrpc": "2.0", "id": id, "error": error}));
                } else if let Some(result) = step.get("result") {
                    if let Some(s) = result["sessionId"].as_str() {
                        self.session = s.to_string();
                    }
                    self.say(json!({"jsonrpc": "2.0", "id": id, "result": result}));
                } else {
                    self.turn = Some(id.clone());
                }
            }
        } else if let Some(update) = step.get("say") {
            self.update(update.clone());
        } else if let Some(call) = step.get("own") {
            self.own(call);
        } else if let Some(call) = step.get("ask") {
            let options = step.get("options").cloned().unwrap_or_else(agy_options);
            let params = json!({"sessionId": self.session, "toolCall": call, "options": options});
            let id = &step["id"];
            self.say(
                json!({"jsonrpc": "2.0", "id": id, "method": "session/request_permission", "params": params}),
            );
        } else if let Some(id) = step.get("await") {
            let answered = |v: &Value| v["id"] == *id && v.get("method").is_none();
            if self.wait(answered).is_none() {
                self.missed(&format!("the answer to {id}"));
            }
        } else if let Some(reason) = step.get("end_turn") {
            let Some(id) = self.turn.take() else { self.missed("a turn to end") };
            let mut result = json!({"stopReason": reason});
            if let Some(usage) = step.get("usage") {
                result["usage"] = usage.clone();
            }
            self.say(json!({"jsonrpc": "2.0", "id": id, "result": result}));
        } else if let Some(result) = step.get("answer") {
            let Some(id) = self.turn.take() else { self.missed("a request to answer") };
            self.say(json!({"jsonrpc": "2.0", "id": id, "result": result}));
        } else if let Some(v) = step.get("send") {
            self.say(v.clone());
        } else if let Some(line) = step["stderr"].as_str() {
            eprintln!("{line}");
        } else if let Some(code) = step["exit"].as_i64() {
            std::process::exit(code as i32);
        } else if let Some(text) = step["junk"].as_str() {
            let _ = writeln!(self.out, "{text}").and_then(|_| self.out.flush());
        } else if step["eof"] == true {
            while self.hear().is_some() {}
            self.heard(&json!({"eof": true}));
        } else if let Some(message) = step.get("mcp") {
            self.mcp(message);
        } else if step["linger"] == true {
            // (a child of its own that outlives it: `sh` waits to open a FIFO nobody writes to)
            let child = std::process::Command::new("/bin/sh")
                .args(["-c", "exec 0<&-; mkfifo \"$1\" && read x < \"$1\"", "sh"])
                .arg(format!("{}.fifo", std::env::var(HEARD).unwrap_or_default()))
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("a lingering child");
            self.heard(&json!({"linger_pid": child.id()}));
            self.children.push(child);
        } else if step["tty"] == true {
            // SAFETY: plain calls about this process.
            let (pid, pgid, sid) = unsafe { (libc::getpid(), libc::getpgid(0), libc::getsid(0)) };
            let tty = match std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty") {
                Ok(_) => "opened".to_string(),
                Err(e) => e.to_string(),
            };
            self.heard(&json!({"pid": pid, "pgid": pgid, "sid": sid, "tty": tty}));
        }
    }

    fn update(&mut self, update: Value) {
        let params = json!({"sessionId": self.session, "update": update});
        self.say(json!({"jsonrpc": "2.0", "method": "session/update", "params": params}));
    }

    /// A tool call, its own or tarae's, as agy 1.5.9 runs one (the second probe's, `.sb/agent-p4-probe/
    /// out-1794767239/`): reported `in_progress`, its hooks asked first (`hook`); where they refused or one failed (a
    /// refusal, past its timeout, an exit other than 0, what isn't JSON) `failed` with the reason, and nothing asked.
    /// Else — one allowed, or none said anything (no decision, #8) — `pending` and a permission ask offering
    /// `allow_once` and `reject_once` only, in every mode; on `allow_once` `in_progress`, the call (one of tarae's:
    /// its `mcp` message to tarae's server, the answer waited for), then `completed`; any other answer `failed`. With
    /// `NO_ASK` set, the call is made after the hooks, unasked (agy 1.5.4 and 1.5.7).
    fn own(&mut self, call: &Value) {
        let (id, name, args) = (&call["id"], &call["name"], &call["args"]);
        let kind = if name.as_str().is_some_and(|n| n.starts_with("mcp_")) { "other" } else { "execute" };
        self.update(json!({"sessionUpdate": "tool_call", "toolCallId": id, "title": name, "kind": kind,
            "status": "in_progress", "name": name, "args": args, "rawInput": args, "locations": null}));
        let dir = std::env::var(DIR).unwrap_or_default();
        let brain = format!("{dir}/antigravity-cli/brain/{}", self.session);
        let input = json!({
            "artifactDirectoryPath": brain,
            "conversationId": self.session,
            "cwd": self.cwd,
            "hookEventName": "PreToolUse",
            "lastUserInput": self.last_input,
            "modelName": "stand-in",
            "stepIdx": 1,
            "toolCall": {"args": args, "id": id, "name": name},
            "transcriptPath": format!("{brain}/.system_generated/logs/transcript_full.jsonl"),
            "workspacePaths": [self.cwd],
        });
        let answer = hook(&dir, &input);
        self.heard(&json!({"hook": answer}));
        let failed = |reason: &Value| {
            let content = match reason.is_null() {
                true => Value::Null,
                false => json!([{"type": "content", "content": {"type": "text", "text": reason}}]),
            };
            json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "failed", "content": content})
        };
        if answer["decision"] == "deny" {
            self.update(failed(&answer["reason"]));
            return;
        }
        if std::env::var_os(NO_ASK).is_none() {
            self.update(json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "pending"}));
            self.asks += 1;
            let ask = json!(format!("ask-{}", self.asks));
            let asked = json!({"toolCallId": id, "title": format!("Run {}", name.as_str().unwrap_or_default()),
                "kind": kind, "status": "pending", "name": name, "args": args, "rawInput": args});
            let params = json!({"sessionId": self.session, "toolCall": asked, "options": agy_options()});
            self.say(
                json!({"jsonrpc": "2.0", "id": ask, "method": "session/request_permission", "params": params}),
            );
            let answered = |v: &Value| v["id"] == ask && v.get("method").is_none();
            let Some(a) = self.wait(answered) else { self.missed(&format!("the answer to {ask}")) };
            let allowed =
                agy_options().as_array().unwrap().iter().any(|o| {
                    o["kind"] == "allow_once" && a["result"]["outcome"]["optionId"] == o["optionId"]
                }) && a["result"]["outcome"]["outcome"] == "selected";
            if !allowed {
                self.update(failed(&Value::Null));
                return;
            }
            self.update(
                json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "in_progress"}),
            );
        }
        if let Some(message) = call.get("mcp") {
            self.mcp(message);
        }
        self.update(json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "completed", "content": null}));
    }

    /// `message` to tarae's server, started first if it isn't; a request's answer waited for and heard.
    fn mcp(&mut self, message: &Value) {
        if self.server.is_none() {
            let s = &self.servers[0];
            let mut cmd = std::process::Command::new(s["command"].as_str().expect("a server's command"));
            cmd.args(s["args"].as_array().into_iter().flatten().filter_map(Value::as_str));
            for e in s["env"].as_array().into_iter().flatten() {
                cmd.env(e["name"].as_str().unwrap_or_default(), e["value"].as_str().unwrap_or_default());
            }
            // (its stderr isn't tarae's: the stand-in's end is told by its own)
            cmd.stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            let mut child = cmd.spawn().expect("tarae's server started");
            self.heard(&json!({"mcp_pid": child.id()}));
            let (stdin, stdout) = (child.stdin.take().unwrap(), child.stdout.take().unwrap());
            self.server = Some((stdin, std::io::BufReader::new(stdout).lines()));
            self.children.push(child);
        }
        let Some((stdin, lines)) = self.server.as_mut() else { return };
        let _ = writeln!(stdin, "{message}").and_then(|_| stdin.flush());
        let Some(id) = message.get("id") else { return };
        let mut others = Vec::new();
        let answer =
            lines.map_while(Result::ok).filter_map(|l| serde_json::from_str::<Value>(&l).ok()).find(|v| {
                match v["id"] == *id && v.get("method").is_none() {
                    true => true,
                    false => {
                        others.push(v.clone());
                        false
                    }
                }
            });
        for v in others {
            self.heard(&json!({"mcp_other": v}));
        }
        match answer {
            Some(a) => self.heard(&json!({"mcp_answer": a})),
            None => self.missed(&format!("tarae's server's answer to {id}")),
        }
    }
}

/// The answer of the hooks `<dir>/config/hooks.json` names for every tool (agy's shape, `hook::file`) — its groups by
/// name (serde_json's map is sorted; tarae's file has one), each group's hooks in the file's order — each run as
/// `run_hook` runs one: the first refusal or failure (a refusal of the stand-in's own words, with
/// `failed` saying why: agy 1.5.9 fails the call, #5 #8), which ends the chain; else the last answer naming a
/// decision; else `"none"` (no decision: the call goes on to its ask, #8).
fn hook(dir: &str, input: &Value) -> Value {
    let file = std::fs::read_to_string(format!("{dir}/config/hooks.json")).unwrap_or_default();
    let file: Value = serde_json::from_str(&file).unwrap_or_default();
    let groups = file.as_object().into_iter().flat_map(|g| g.values());
    let entries = groups.flat_map(|g| g["PreToolUse"].as_array().into_iter().flatten());
    let hooks =
        entries.filter(|e| e["matcher"] == "*").flat_map(|e| e["hooks"].as_array().into_iter().flatten());
    let mut answer = json!("none");
    for h in hooks.filter(|h| h["type"] == "command") {
        let Some(command) = h["command"].as_str() else { continue };
        let timeout =
            std::time::Duration::from_secs_f64(h["timeout"].as_f64().unwrap_or(HOOK_TIMEOUT).max(0.0));
        let said = match run_hook(command, timeout, input) {
            Ok(said) => said,
            Err((why, pid)) => {
                let reason = format!("hook failed: {why}");
                return json!({"decision": "deny", "reason": reason, "failed": why, "pid": pid});
            }
        };
        if said["decision"] == "deny" {
            return said;
        }
        if said["decision"].is_string() {
            answer = said;
        }
    }
    answer
}

/// One hook run as agy 1.5.9 was seen to run one, but by its command alone (agy's goes through a shell, #6 — step 4's
/// L-B3): `input` on stdin, its first line of output read. Err, why it failed and its pid (0: none): it couldn't be
/// run, it ran past `timeout` (killed, as agy kills it, #5), it exited other than 0, or its line isn't JSON (#8).
/// Nothing said is `Null`: no decision. Its output read on a thread of its own, so the wait for it is bounded with no
/// clock read.
fn run_hook(
    command: &str,
    timeout: std::time::Duration,
    input: &Value,
) -> Result<Value, (&'static str, u32)> {
    use std::io::Read;
    let child = std::process::Command::new(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return Err(("not run", 0)) };
    let pid = child.id();
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{input}");
    }
    let Some(mut stdout) = child.stdout.take() else { return Err(("not run", pid)) };
    let (tx, rx) = std::sync::mpsc::channel();
    crate::event::thread("tarae-standin", move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        let _ = tx.send(out);
    });
    let Ok(out) = rx.recv_timeout(timeout) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(("timeout", pid));
    };
    if !child.wait().is_ok_and(|s| s.success()) {
        return Err(("exit", pid));
    }
    match String::from_utf8_lossy(&out).lines().next().filter(|l| !l.trim().is_empty()) {
        None => Ok(Value::Null),
        Some(line) => serde_json::from_str(line).map_err(|_| ("junk", pid)),
    }
}

/// The stand-in agent itself: run only as a test's child (`SCRIPT` set), its steps in order, then its end.
#[test]
#[ignore = "the ACP stand-in agent: a test's child, never run alone"]
fn run() {
    use std::os::fd::FromRawFd;
    let (Ok(script), Ok(heard)) = (std::env::var(SCRIPT), std::env::var(HEARD)) else { return };
    let script: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(script).unwrap()).unwrap();
    let heard = std::fs::OpenOptions::new().create(true).append(true).open(heard).unwrap();
    // SAFETY: descriptor 3 is tarae's pipe, as the stand-in's row's `3>&1` gave it; nothing else here owns it. Closed
    // on exec, so no child of the stand-in's holds tarae's pipe open past it (its `server`'s `3>&1` makes its own).
    let out = unsafe {
        libc::fcntl(3, libc::F_SETFD, libc::FD_CLOEXEC);
        std::fs::File::from_raw_fd(3)
    };
    let mut a = Agent {
        out,
        heard,
        lines: std::io::stdin().lock().lines(),
        unread: VecDeque::new(),
        session: String::new(),
        turn: None,
        servers: Value::Null,
        server: None,
        children: Vec::new(),
        cwd: String::new(),
        last_input: String::new(),
        asks: 0,
    };
    let mut l = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: a plain call filling a struct of ours.
    unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut l) };
    let mut names: Vec<String> = std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()).collect();
    names.sort();
    a.heard(&json!({"env": names, "nofile": l.rlim_cur, "dir": std::env::var(DIR).ok()}));
    for step in &script {
        a.step(step);
    }
}
