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
//! - `{"own": {"name": …, "args": …, "id": …}}` runs one of its own tools as agy does (`own`): reported, its folder's
//!   hook asked on agy's input (heard as `{"hook": its answer}`), then `failed` with the hook's reason or `completed`.
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

    /// One of its own tools, as agy runs one (the design session's probe, Q5): reported `in_progress`, its hook asked
    /// first, then `failed` with the hook's reason where it refused, else `completed`. A hook that's missing, won't
    /// run or says nothing it reads lets the tool run: it plays an agy that fails open (Q7, unmeasured).
    fn own(&mut self, call: &Value) {
        let (id, name, args) = (&call["id"], &call["name"], &call["args"]);
        self.update(json!({"sessionUpdate": "tool_call", "toolCallId": id, "title": name, "kind": "execute",
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
        let update = match answer["decision"] == "deny" {
            true => {
                let reason =
                    json!({"type": "content", "content": {"type": "text", "text": answer["reason"]}});
                json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "failed", "content": [reason]})
            }
            false => {
                json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "completed", "content": null})
            }
        };
        self.update(update);
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

/// The answer of the hooks `<dir>/config/hooks.json` names for every tool (agy's shape, `hook::file`), each run as agy
/// runs one — its command alone, `input` on stdin — the first refusal, else the last answer, else `"none"`.
fn hook(dir: &str, input: &Value) -> Value {
    let file = std::fs::read_to_string(format!("{dir}/config/hooks.json")).unwrap_or_default();
    let file: Value = serde_json::from_str(&file).unwrap_or_default();
    let groups = file.as_object().into_iter().flat_map(|g| g.values());
    let entries = groups.flat_map(|g| g["PreToolUse"].as_array().into_iter().flatten());
    let hooks =
        entries.filter(|e| e["matcher"] == "*").flat_map(|e| e["hooks"].as_array().into_iter().flatten());
    let mut answer = json!("none");
    for command in hooks.filter(|h| h["type"] == "command").filter_map(|h| h["command"].as_str()) {
        let child = std::process::Command::new(command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();
        let Ok(mut child) = child else { continue };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "{input}");
        }
        let out = child.wait_with_output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
        let said = out.ok().and_then(|o| serde_json::from_str::<Value>(o.lines().next()?).ok());
        if let Some(said) = said.filter(|s| s["decision"].is_string()) {
            if said["decision"] == "deny" {
                return said;
            }
            answer = said;
        }
    }
    answer
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
