//! The ACP wire's tests: each message read, line → `Event`, in the real agy's shape where agy sent it (the design
//! session's probe, condition 7), the dropped ones included; then the wire over the stand-in agent (`standin`).

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, channel};

use serde_json::{Value, json};

use super::standin::{HEARD, SCRIPT};
use super::*;
use crate::agent::{Resume, Role, STAND_IN};
use crate::llm::LlmConfig;

// ── agy's shapes ─────────────────────────────────────────────────────────────

/// agy's `initialize` answer: `loadSession`, `sessionCapabilities` (and again in snake_case), `auth.logout`, its
/// `_meta` system prompt — no `sessionCapabilities.systemPrompt`.
fn agy_initialize() -> Value {
    let session = json!({"resume": {}, "close": {}, "load": {}, "list": {}, "additionalDirectories": {}});
    json!({"protocolVersion": 1, "agentCapabilities": {
        "loadSession": true,
        "sessionCapabilities": session,
        "session_capabilities": session,
        "auth": {"logout": {}},
        "_meta": {"antigravity": {"systemPrompt": {"append": true}}},
    }, "authMethods": []})
}

/// agy's `session/new` answer: its id, `configOptions` with a `model` select, `models`, `modes`, `_meta`.
fn agy_session(id: &str) -> Value {
    json!({
        "sessionId": id,
        "configOptions": [{"id": "model", "name": "Model", "type": "select", "currentValue": "gemini-pro",
            "options": [{"value": "gemini-pro", "name": "Gemini Pro"}]}],
        "models": {"currentModelId": "gemini-pro", "availableModels": []},
        "modes": {"currentModeId": "default", "availableModes": []},
        "_meta": {},
    })
}

/// agy's `tool_call` for a command: `name` and `args` beyond the spec, its title the command line itself.
fn agy_command_call() -> Value {
    json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "echo probe-ran", "kind": "execute",
        "status": "pending", "name": "run_command", "args": {"CommandLine": "echo probe-ran"},
        "rawInput": {"CommandLine": "echo probe-ran"}, "locations": null})
}

/// agy's permission ask's params: `toolCall` with `name`, `title`, `kind`, `status: pending`, `rawInput`; its two
/// options.
fn agy_ask(name: Option<&str>, title: &str, kind: &str) -> Value {
    let mut call = json!({"toolCallId": "t1", "title": title, "kind": kind, "status": "pending",
        "rawInput": {"CommandLine": "echo probe-ran"}});
    if let Some(name) = name {
        call["name"] = json!(name);
    }
    json!({"sessionId": "s-1", "toolCall": call, "options": standin::agy_options()})
}

/// agy's `session/prompt` answer's `usage`.
fn agy_usage() -> Value {
    json!({"inputTokens": 1200, "outputTokens": 80, "cachedReadTokens": 900, "cachedWriteTokens": 30,
        "thoughtTokens": 400, "totalTokens": 2210})
}

fn up(update: Value) -> Option<Event> {
    super::update(&update, &mut HashSet::new())
}

fn said(ev: Event) -> Option<String> {
    Some(match ev {
        Event::Thinking => "thinking".into(),
        Event::ThinkingBlock(open) => format!("block {}", if open { "open" } else { "closed" }),
        Event::Thought(t) => format!("thought {t}"),
        Event::Text(t) => format!("text {t}"),
        Event::Tool { id, name, input } => format!("tool {id} {name} {input}"),
        Event::ToolResult { id, text, error } => format!("result {id} {error} {text}"),
        Event::Permission { id, tool, input } => format!("ask {id} {tool} {input}"),
        Event::Conversation { id, model, version, key_source } => {
            format!("conversation {id} {model} {version:?} {key_source:?}")
        }
        Event::Ready { models, key_source, account } => {
            format!("ready {} {key_source:?} {}", models.len(), account.is_some())
        }
        Event::Spent(s) => format!("spent {} {:?} {:?}", s.conversation, s.counts, s.total_cost),
        Event::Compaction(c) => format!("compaction {c:?}"),
        Event::Done(Ok(t)) => format!("done {t}"),
        Event::Done(Err(e)) => format!("failed {e}"),
        Event::Exit(Exit { said, kind: Ended::Exited }) => format!("exit {said}"),
        Event::Exit(Exit { said, kind: Ended::Gone }) => format!("exit gone: {said}"),
        Event::Mcp { id, message } => format!("mcp {id} {message}"),
        _ => return None,
    })
}

// ── Reading ──────────────────────────────────────────────────────────────────

/// A line is an answer to a request of ours (a result, an error), a permission ask, another request of the
/// agent's, an update, or nothing tarae reads.
#[test]
fn each_line_is_told_by_its_kind() {
    let line = |v: Value| read(&v);
    assert_eq!(
        line(json!({"jsonrpc": "2.0", "id": 3, "result": {"stopReason": "end_turn"}})),
        Read::Answer { id: 3, result: Ok(json!({"stopReason": "end_turn"})) }
    );
    assert_eq!(
        line(
            json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32000, "message": "Authentication required"}})
        ),
        Read::Answer { id: 1, result: Err((-32000, "Authentication required".into())) }
    );
    let ask = json!({"jsonrpc": "2.0", "id": 7, "method": "session/request_permission", "params": {"x": 1}});
    assert_eq!(line(ask), Read::Ask { id: json!(7), params: json!({"x": 1}) });
    for method in ["fs/read_text_file", "terminal/create", "elicitation/create"] {
        let req = json!({"jsonrpc": "2.0", "id": "r1", "method": method, "params": {}});
        assert_eq!(line(req), Read::Request { id: json!("r1") }, "{method}");
    }
    let update = json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s",
        "update": {"sessionUpdate": "agent_message_chunk"}}});
    assert_eq!(line(update), Read::Update(json!({"sessionUpdate": "agent_message_chunk"})));
    assert_eq!(line(json!({"jsonrpc": "2.0", "method": "$/progress", "params": {}})), Read::Other);
    assert_eq!(line(json!({"jsonrpc": "2.0", "id": "not-ours", "result": {}})), Read::Other);
}

/// Text and thought chunks are their text; a chunk's other content, the prompt echoed, and the updates step 2
/// doesn't read are dropped — each in agy's shape where agy sent it.
#[test]
fn chunks_are_text_and_the_rest_is_dropped() {
    let chunk = |kind: &str, content: Value| json!({"sessionUpdate": kind, "content": content});
    let text = |t: &str| json!({"type": "text", "text": t});
    assert!(matches!(up(chunk("agent_message_chunk", text("Hi"))), Some(Event::Text(t)) if t == "Hi"));
    assert!(
        matches!(up(chunk("agent_thought_chunk", text("Checking"))), Some(Event::Thought(t)) if t == "Checking")
    );
    let image = json!({"type": "image", "mimeType": "image/png", "data": ""});
    assert!(up(chunk("agent_message_chunk", image)).is_none(), "a chunk's non-text content");
    assert!(up(chunk("user_message_chunk", text("hello"))).is_none(), "the prompt echoed");
    let dropped = [
        json!({"sessionUpdate": "available_commands_update", "availableCommands": []}),
        json!({"sessionUpdate": "session_info_update", "title": "A probe"}),
        json!({"sessionUpdate": "current_mode_update", "currentModeId": "default"}),
        json!({"sessionUpdate": "config_option_update", "configOptions": []}),
        json!({"sessionUpdate": "usage_update", "used": 1000, "size": 200000, "cost": null}),
        json!({"sessionUpdate": "compaction_summary_chunk", "content": text("so far")}),
        json!({"sessionUpdate": "notice", "message": "hi"}),
    ];
    for u in dropped {
        assert!(up(u.clone()).is_none(), "{u}");
    }
    let compacting = |status: &str| up(json!({"sessionUpdate": "compaction_update", "status": status}));
    assert!(matches!(compacting("in_progress"), Some(Event::Compaction(crate::compaction::Ev::Started))));
    for status in ["completed", "failed", "cancelled"] {
        assert!(
            matches!(compacting(status), Some(Event::Compaction(crate::compaction::Ev::Ended))),
            "{status}"
        );
    }
}

/// ACP's `plan` is dropped (the design session's §9.2: tarae's `plan` tool is the plan, one writer).
#[test]
fn an_acp_plan_is_dropped() {
    let plan = json!({"sessionUpdate": "plan", "entries": [
        {"content": "Read the map", "priority": "high", "status": "in_progress"},
        {"content": "Fix it", "priority": "medium", "status": "pending"},
    ]});
    assert!(up(plan).is_none());
}

/// A call's `locations` are dropped (§9.1: the places come from tarae's tools' inputs): the call's event is the
/// same with them as with agy's null.
#[test]
fn a_calls_locations_are_dropped() {
    let mut placed = agy_command_call();
    placed["locations"] = json!([{"path": "/p/src/a.rs", "line": 12}]);
    let (a, b) = (up(agy_command_call()).and_then(said), up(placed).and_then(said));
    assert_eq!(a, b);
    assert_eq!(a.as_deref(), Some(r#"tool t1 run_command {"CommandLine":"echo probe-ran"}"#));
}

/// A call is named by the agent's `name`, else its `kind` — never its `title`, the model's text (agy's for a
/// command is the command line); its input is `rawInput`, else agy's `args`.
#[test]
fn a_call_is_named_by_its_name_never_its_title() {
    let named = |call: Value| match up(call) {
        Some(Event::Tool { name, input, .. }) => (name, input),
        _ => panic!("a call"),
    };
    assert_eq!(named(agy_command_call()).0, "run_command");
    let mut kind_only = agy_command_call();
    kind_only.as_object_mut().unwrap().remove("name");
    assert_eq!(named(kind_only.clone()).0, "execute");
    kind_only.as_object_mut().unwrap().remove("kind");
    assert_eq!(named(kind_only).0, "", "no name nor kind: none, never the title");
    let mut args_only = agy_command_call();
    args_only.as_object_mut().unwrap().remove("rawInput");
    assert_eq!(named(args_only).1, json!({"CommandLine": "echo probe-ran"}));
}

/// A finished call: a command's text content (its first 4 KB), a write's `diff` and a read's null content as no
/// text, `failed` as an error; one still running is nothing yet.
#[test]
fn a_finished_call_is_its_result() {
    let done = |status: &str, content: Value| {
        up(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": status,
            "content": content, "rawOutput": {"output": "probe-ran\n"}}))
        .and_then(said)
    };
    let command = json!([{"type": "content", "content": {"type": "text", "text": "probe-ran\n"}}]);
    assert_eq!(done("completed", command.clone()).as_deref(), Some("result t1 false probe-ran\n"));
    assert_eq!(done("failed", command).as_deref(), Some("result t1 true probe-ran\n"));
    let write = json!([{"type": "diff", "path": "/p/a.txt", "oldText": null, "newText": "x"}]);
    assert_eq!(done("completed", write).as_deref(), Some("result t1 false "));
    assert_eq!(done("completed", Value::Null).as_deref(), Some("result t1 false "), "a read's");
    assert_eq!(done("in_progress", Value::Null), None);
    let long = json!([{"type": "content", "content": {"type": "text", "text": "é".repeat(3000)}}]);
    match up(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed",
        "content": long}))
    {
        Some(Event::ToolResult { text, .. }) => assert_eq!(text.len(), 4096),
        _ => panic!("a result"),
    }
}

/// tarae's own tools' calls, by agy's `name` (`mcp_tarae_*`), are dropped — the bridge tells them (landing 4) —
/// and their updates with them, by id; another MCP server's are an own tool's.
#[test]
fn tarae_s_own_calls_are_dropped_with_their_updates() {
    let mut dropped = HashSet::new();
    let call = |name: &str, id: &str| {
        json!({"sessionUpdate": "tool_call", "toolCallId": id, "name": name, "title": format!("Run {name}"),
            "kind": "other", "status": "pending", "locations": null, "rawInput": {}})
    };
    let finished = |id: &str| json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "completed", "content": null});
    assert!(update(&call("mcp_tarae_read", "m1"), &mut dropped).is_none());
    assert!(update(&finished("m1"), &mut dropped).is_none(), "its update by id");
    let other = update(&call("mcp_other_read", "m2"), &mut dropped).and_then(said);
    assert_eq!(other.as_deref(), Some("tool m2 mcp_other_read {}"));
    assert!(update(&finished("m2"), &mut dropped).is_some());
}

/// A permission ask names the agent's `name`, else its `kind` — never the title (condition 1): an ask titled
/// `Read` with no name is `read`'s; its id is the JSON-RPC id, a number or a string.
#[test]
fn a_permission_ask_never_names_its_title() {
    let ask = |params: Value| said(permission("7", &params)).unwrap();
    assert_eq!(
        ask(agy_ask(Some("run_command"), "echo probe-ran", "execute")),
        r#"ask 7 run_command {"CommandLine":"echo probe-ran"}"#
    );
    assert!(ask(agy_ask(None, "Read", "read")).starts_with("ask 7 read "));
    assert!(ask(agy_ask(None, "Read", "")).starts_with("ask 7  "), "no name nor kind: none");
    assert_eq!((id_key(&json!(7)), id_key(&json!("p-1"))), ("7".to_string(), "p-1".to_string()));
}

/// A permission's answer selects agy's `allow_once` or `reject_once` option by its kind — `cancelled` where
/// there's none, never `allow_always`.
#[test]
fn a_permission_answer_selects_its_option_by_kind() {
    let allow = Permission::Allow { input: json!({}) };
    let deny = Permission::Deny { message: "no".into() };
    let options = standin::agy_options();
    assert_eq!(outcome(&options, &allow), json!({"outcome": {"outcome": "selected", "optionId": "allow"}}));
    assert_eq!(outcome(&options, &deny), json!({"outcome": {"outcome": "selected", "optionId": "deny"}}));
    let always = json!([{"optionId": "always", "kind": "allow_always"}]);
    assert_eq!(outcome(&always, &allow), json!({"outcome": {"outcome": "cancelled"}}));
    assert_eq!(outcome(&Value::Null, &deny), json!({"outcome": {"outcome": "cancelled"}}));
}

/// A turn's answer: agy's `usage` counted (`thoughtTokens` dropped), its model from `configOptions`, and each
/// `stopReason` said.
#[test]
fn a_turns_answer_is_its_usage_and_its_end() {
    let s = spent(&agy_usage(), "s-1".into(), 5).unwrap();
    let want = crate::usage::Counts { input: Some(1200), write: Some(30), read: Some(900), output: Some(80) };
    assert_eq!((s.counts, s.total_cost, s.models.len(), s.ms), (want, None, 0, None));
    assert!(spent(&Value::Null, "s-1".into(), 5).is_none());
    assert_eq!(model_of(&agy_session("s-1")), "gemini-pro");
    assert_eq!(model_of(&json!({"sessionId": "s-1"})), "");
    assert_eq!(stopped("agy", "end_turn", "Hi"), Ok("Hi".into()));
    assert_eq!(stopped("agy", "cancelled", "Hi"), Err("stopped".into()));
    for (reason, words) in [
        ("max_tokens", "agy stopped: the answer reached its length limit"),
        ("max_turn_requests", "agy stopped: the turn reached its limit of requests"),
        ("refusal", "agy refused to go on"),
    ] {
        assert_eq!(stopped("agy", reason, ""), Err(words.into()));
    }
}

// ── Over the stand-in ───────────────────────────────────────────────────────────

/// A stand-in running: its scratch folder, what came up from its wire, its handle.
struct Run {
    dir: crate::scratch::Scratch,
    rx: Receiver<String>,
    started: Option<Started>,
    _ed: crate::editor::Editor,
}

/// How a test starts it.
#[derive(Default)]
struct Opts<'a> {
    resume: Option<&'a str>,
    system: Option<&'a str>,
    prompt: Option<&'a str>,
    env: Env,
    cfg: LlmConfig,
    /// tarae's tools offered (the bridge).
    tools: bool,
}

fn start(script: Value, o: Opts) -> Run {
    let dir = crate::scratch::Scratch::new("acp-stand-in");
    std::fs::write(dir.join("script.json"), script.to_string()).unwrap();
    let mut env = o.env;
    env.insert(SCRIPT.into(), dir.join("script.json").to_string_lossy().into_owned());
    env.insert(HEARD.into(), dir.join("heard").to_string_lossy().into_owned());
    // (where a test's bridge binds: its own state folder)
    env.insert("XDG_STATE_HOME".into(), dir.join("state").to_string_lossy().into_owned());
    let s = Start {
        system: o.system,
        resume: o.resume.map(|conversation| Resume { conversation, fork: false }),
        ..Start::on(&STAND_IN, Role::Chat, &o.cfg)
    };
    let at = crate::agent::Place { dir: dir.path(), given: Some(&env), cred: None };
    let first = First { tools: o.tools, prompt: o.prompt.map(|p| Prompt::Text(p.into())) };
    let ed = crate::editor::Editor::new(crate::config::Config::default());
    let (tx, rx) = channel();
    let route = move |ev| {
        if let Some(s) = said(ev) {
            let _ = tx.send(s);
        }
        None
    };
    let started = crate::agent::start(&s, at, first, false, ed.events.sender(), route).expect("started");
    Run { dir, rx, started: Some(started), _ed: ed }
}

impl Run {
    /// What came up, through the first that starts with `last`.
    fn until(&self, last: &str) -> Vec<String> {
        let mut got = Vec::new();
        loop {
            let s = self.rx.recv_timeout(crate::test_results::PATIENCE);
            let s = s.unwrap_or_else(|_| panic!("no `{last}` came up: {got:?}\nheard: {:?}", self.heard()));
            let end = s.starts_with(last);
            got.push(s);
            if end {
                return got;
            }
        }
    }

    /// What the stand-in heard so far, its first line (its environment) included.
    fn heard(&self) -> Vec<Value> {
        let text = std::fs::read_to_string(self.dir.join("heard")).unwrap_or_default();
        text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    /// What it heard, once something `f` takes is among it.
    fn heard_until(&self, f: impl Fn(&Value) -> bool) -> Vec<Value> {
        let end = std::time::Instant::now() + crate::test_results::PATIENCE;
        loop {
            let heard = self.heard();
            if heard.iter().any(&f) {
                return heard;
            }
            assert!(std::time::Instant::now() < end, "never heard it: {heard:?}");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The messages it heard of `method`.
    fn of(&self, method: &str) -> Vec<Value> {
        self.heard().into_iter().filter(|v| v["method"] == method).collect()
    }

    fn agent(&self) -> &Agent {
        &self.started.as_ref().unwrap().agent
    }
}

fn expect(method: &str, result: Value) -> Value {
    json!({"expect": method, "result": result})
}

/// The handshake in agy's shapes, then a turn held.
fn handshake() -> Vec<Value> {
    vec![expect("initialize", agy_initialize()), expect("session/new", agy_session("s-1"))]
}

/// The handshake on the stand-in: `initialize` says version 1, no file system nor terminal, compaction; agy's
/// answer starts a new session with no MCP server (tarae's tools not asked) and, as agy takes no system prompt,
/// tarae's words go as the first message's first block — once: the second message has none. A turn's thought,
/// text, usage and end come up in order, a line that isn't JSON is passed over, and the handle dropped closes
/// stdin.
#[test]
fn a_turn_on_the_stand_in() {
    let thought = |t: &str| json!({"say": {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": t}}});
    let text = |t: &str| json!({"say": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": t}}});
    let echo =
        json!({"say": {"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "hello"}}});
    let mut script = handshake();
    script.extend([
        json!({"expect": "session/prompt"}),
        echo,
        thought("Checking "),
        thought("the map."),
        json!({"junk": "not json"}),
        text("Hi "),
        text("there"),
        json!({"end_turn": "end_turn", "usage": agy_usage()}),
        json!({"expect": "session/prompt"}),
        text("Again"),
        json!({"end_turn": "end_turn"}),
        json!({"eof": true}),
    ]);
    let mut run = start(
        json!(script),
        Opts { system: Some("tarae's words"), prompt: Some("hello"), ..Opts::default() },
    );
    let got = run.until("done");
    let mut kinds: Vec<&str> = got.iter().map(|g| g.split(' ').next().unwrap()).collect();
    kinds.dedup();
    assert_eq!(
        kinds,
        ["conversation", "ready", "thinking", "block", "thought", "block", "text", "spent", "done"]
    );
    assert_eq!(got[0], "conversation s-1 gemini-pro None None");
    assert_eq!(got[1], "ready 0 None false");
    let joined = |p: &str| got.iter().filter_map(|g| g.strip_prefix(p)).collect::<String>();
    assert_eq!((joined("thought "), joined("text ")), ("Checking the map.".into(), "Hi there".into()));
    let counts =
        crate::usage::Counts { input: Some(1200), write: Some(30), read: Some(900), output: Some(80) };
    assert!(got.contains(&format!("spent s-1 {counts:?} None")), "{got:?}");
    assert_eq!(got.last().unwrap(), "done Hi there");
    run.agent().prompt(Prompt::Text("again".into())).unwrap();
    assert_eq!(run.until("done").last().unwrap(), "done Again");
    let init = &run.of("initialize")[0]["params"];
    assert_eq!(init["protocolVersion"], 1);
    let caps = &init["clientCapabilities"];
    assert_eq!(caps["fs"], json!({"readTextFile": false, "writeTextFile": false}));
    assert_eq!((&caps["terminal"], &caps["session"]["compaction"]), (&json!(false), &json!({})));
    let new = &run.of("session/new")[0]["params"];
    assert_eq!(new["mcpServers"], json!([]));
    assert!(new.get("systemPrompt").is_none(), "agy takes none: {new}");
    let prompts: Vec<Value> =
        run.of("session/prompt").into_iter().map(|p| p["params"]["prompt"].clone()).collect();
    let block = |t: &str| json!({"type": "text", "text": t});
    assert_eq!(prompts, [json!([block("tarae's words"), block("hello")]), json!([block("again")])]);
    assert!(prompts.iter().all(|p| p.as_array().unwrap().iter().all(|b| b["type"] == "text")));
    run.started = None;
    run.heard_until(|v| v["eof"] == true);
    assert!(run.until("exit").last().unwrap().starts_with("exit"));
}

/// An agent whose `sessionCapabilities` take a system prompt gets tarae's words in `session/new`, appended, and
/// none in its first message.
#[test]
fn a_system_prompt_goes_where_the_agent_takes_one() {
    let init =
        json!({"protocolVersion": 1, "agentCapabilities": {"sessionCapabilities": {"systemPrompt": {}}}});
    let script = json!([expect("initialize", init), expect("session/new", agy_session("s-1")),
        {"expect": "session/prompt"}, {"end_turn": "end_turn"}]);
    let run = start(script, Opts { system: Some("tarae's words"), prompt: Some("hello"), ..Opts::default() });
    run.until("done");
    let new = &run.of("session/new")[0]["params"];
    let block = |t: &str| json!({"type": "text", "text": t});
    assert_eq!(new["systemPrompt"], json!({"mode": "append", "content": [block("tarae's words")]}));
    assert_eq!(run.of("session/prompt")[0]["params"]["prompt"], json!([block("hello")]));
}

/// A permission ask comes up named by the agent's `name`, and is answered with the option of its kind: allowed,
/// `allow_once`; refused, `reject_once`.
#[test]
fn a_permission_ask_is_answered_by_its_options_kind() {
    let call = |id: &str| {
        json!({"toolCallId": id, "name": "write_file", "title": "Write a.txt", "kind": "edit",
        "status": "pending", "rawInput": {"path": "a.txt"}})
    };
    let mut script = handshake();
    script.extend([
        json!({"ask": call("t1"), "id": 1}),
        json!({"await": 1}),
        json!({"ask": call("t2"), "id": "p-2"}),
        json!({"await": "p-2"}),
        json!({"eof": true}),
    ]);
    let run = start(json!(script), Opts::default());
    assert_eq!(run.until("ask").last().unwrap(), r#"ask 1 write_file {"path":"a.txt"}"#);
    run.agent().answer_permission("1", Permission::Allow { input: json!({}) });
    assert!(run.until("ask").last().unwrap().starts_with("ask p-2 write_file"));
    run.agent().answer_permission("p-2", Permission::Deny { message: "no".into() });
    let heard = run.heard_until(|v| v["id"] == "p-2");
    let answer = |id: Value| {
        heard.iter().find(|v| v["id"] == id && v.get("result").is_some()).unwrap()["result"].clone()
    };
    assert_eq!(answer(json!(1)), json!({"outcome": {"outcome": "selected", "optionId": "allow"}}));
    assert_eq!(answer(json!("p-2")), json!({"outcome": {"outcome": "selected", "optionId": "deny"}}));
}

/// A stop mid-turn: `session/cancel`, and the ask still open answered `cancelled` (the spec's MUST); the turn's
/// `cancelled` end is the chat's stopped turn.
#[test]
fn a_stop_cancels_the_turn_and_its_open_ask() {
    let call = json!({"toolCallId": "t1", "name": "run_command", "title": "echo probe-ran", "kind": "execute",
        "status": "pending", "rawInput": {"CommandLine": "echo probe-ran"}});
    let mut script = handshake();
    script.extend([
        json!({"expect": "session/prompt"}),
        json!({"ask": call, "id": 5}),
        json!({"expect": "session/cancel"}),
        json!({"await": 5}),
        json!({"end_turn": "cancelled"}),
        json!({"eof": true}),
    ]);
    let run = start(json!(script), Opts { prompt: Some("go"), ..Opts::default() });
    run.until("ask 5 run_command");
    assert!(run.agent().stop_turn());
    assert_eq!(run.until("failed").last().unwrap(), "failed stopped");
    let heard = run.heard();
    let answer = heard.iter().find(|v| v["id"] == 5).unwrap();
    assert_eq!(answer["result"], json!({"outcome": {"outcome": "cancelled"}}));
    assert_eq!(run.of("session/cancel")[0]["params"], json!({"sessionId": "s-1"}));
}

/// `auth_required` at `session/new`: the start ends with its words — sign in with the agent's own program — and
/// its process; `authenticate` is never sent.
#[test]
fn signed_out_at_session_new_ends_the_start_with_its_words() {
    let script = json!([expect("initialize", agy_initialize()),
        {"expect": "session/new", "error": -32000, "message": "Authentication required"}, {"eof": true}]);
    let run = start(script, Opts { prompt: Some("hello"), ..Opts::default() });
    let got = run.until("exit");
    assert!(
        got.contains(
            &"failed stand-in isn't signed in — sign in with `sh` itself, then send again".to_string()
        ),
        "{got:?}"
    );
    assert!(run.of("authenticate").is_empty() && run.of("session/prompt").is_empty());
}

/// An agent that speaks another version of ACP is refused at `initialize`, no session opened.
#[test]
fn another_protocol_version_is_refused() {
    let script =
        json!([expect("initialize", json!({"protocolVersion": 2, "agentCapabilities": {}})), {"eof": true}]);
    let run = start(script, Opts::default());
    let got = run.until("exit");
    assert!(got.contains(&"failed stand-in speaks ACP version 2; tarae speaks 1".to_string()), "{got:?}");
    assert!(run.of("session/new").is_empty());
}

/// An agent that ends mid-turn: its end says stderr's last line.
#[test]
fn an_exit_mid_turn_says_its_stderr() {
    let mut script = handshake();
    script.extend([
        json!({"expect": "session/prompt"}),
        json!({"stderr": "it fell over"}),
        json!({"exit": 1}),
    ]);
    let run = start(json!(script), Opts { prompt: Some("go"), ..Opts::default() });
    assert_eq!(run.until("exit").last().unwrap(), "exit it fell over");
}

/// A request of the agent's tarae doesn't serve (`fs/*`, `terminal/*` aren't offered) is answered `-32601`.
#[test]
fn a_request_tarae_doesnt_serve_is_answered_method_not_found() {
    let mut script = handshake();
    script.extend([
        json!({"send": {"jsonrpc": "2.0", "id": "r1", "method": "fs/read_text_file", "params": {"path": "/etc/hosts"}}}),
        json!({"await": "r1"}),
        json!({"eof": true}),
    ]);
    let run = start(json!(script), Opts::default());
    let heard = run.heard_until(|v| v["id"] == "r1");
    let answer = heard.iter().find(|v| v["id"] == "r1").unwrap();
    assert_eq!(answer["error"]["code"], -32601, "{answer}");
}

/// A conversation continued: through `session/resume` where the agent offers it (agy does), its id the chat's,
/// and none of tarae's words again (once a conversation, never on a resume).
#[test]
fn a_conversation_is_resumed_through_session_resume() {
    let script = json!([expect("initialize", agy_initialize()), expect("session/resume", json!({"configOptions": []})),
        {"expect": "session/prompt"}, {"end_turn": "end_turn"}]);
    let run = start(
        script,
        Opts {
            resume: Some("s-old"),
            system: Some("tarae's words"),
            prompt: Some("more"),
            ..Opts::default()
        },
    );
    let got = run.until("done");
    assert_eq!(got[0], "conversation s-old  None None");
    assert_eq!(run.of("session/resume")[0]["params"]["sessionId"], "s-old");
    assert!(run.of("session/new").is_empty() && run.of("session/load").is_empty());
    assert_eq!(run.of("session/prompt")[0]["params"]["prompt"], json!([{"type": "text", "text": "more"}]));
}

/// Through `session/load` where only that is offered: its replay of the conversation is dropped until its answer.
#[test]
fn a_conversation_is_loaded_its_replay_dropped() {
    let init = json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": true}});
    let replay =
        |kind: &str, t: &str| json!({"say": {"sessionUpdate": kind, "content": {"type": "text", "text": t}}});
    let script = json!([expect("initialize", init),
        // (the load held while its replay comes, then answered)
        {"expect": "session/load"},
        replay("user_message_chunk", "old question"), replay("agent_message_chunk", "old answer"),
        {"answer": {"configOptions": []}},
        {"expect": "session/prompt"}, replay("agent_message_chunk", "new answer"), {"end_turn": "end_turn"}]);
    let run = start(
        script,
        Opts {
            resume: Some("s-old"),
            system: Some("tarae's words"),
            prompt: Some("more"),
            ..Opts::default()
        },
    );
    let got = run.until("done");
    assert!(!got.iter().any(|g| g.contains("old") && g.starts_with("text")), "the replay dropped: {got:?}");
    assert_eq!(got.last().unwrap(), "done new answer");
    assert_eq!(run.of("session/load")[0]["params"]["sessionId"], "s-old");
    assert_eq!(run.of("session/prompt")[0]["params"]["prompt"], json!([{"type": "text", "text": "more"}]));
}

/// A conversation that can't be continued — none offered, or the agent doesn't have it (`-32002`) — ends the
/// process, its end's kind `Gone`, so the chat's path for one starts a new conversation and says it; its words are
/// the row's own, never Claude's.
#[test]
fn a_conversation_that_cant_be_continued_is_said_gone() {
    let none = json!({"protocolVersion": 1, "agentCapabilities": {}});
    let unknown = json!({"expect": "session/resume", "error": -32002, "message": "Resource not found"});
    for script in [
        json!([expect("initialize", none), {"eof": true}]),
        json!([expect("initialize", agy_initialize()), unknown, {"eof": true}]),
    ] {
        let run = start(script, Opts { resume: Some("s-old"), prompt: Some("more"), ..Opts::default() });
        assert_eq!(run.until("exit").last().unwrap(), "exit gone: stand-in has no conversation s-old");
        assert!(run.of("session/new").is_empty() && run.of("session/prompt").is_empty());
    }
}

/// Step 3, L1: the agent has a session of its own (`spawn::setsid_fed_as`) — its session and group its own pid, the
/// pid tarae started — and so no controlling terminal: it can't open `/dev/tty`. The session is what's checked first:
/// a run with no terminal can't open `/dev/tty` whatever the agent's start, so that alone would hold on a runner
/// without one.
#[test]
fn the_agent_has_a_session_of_its_own_and_no_terminal() {
    let run = start(json!([{"tty": true}, {"eof": true}]), Opts::default());
    let heard = run.heard_until(|v| v.get("sid").is_some());
    let me = heard.iter().find(|v| v.get("sid").is_some()).unwrap();
    let pid = run.started.as_ref().unwrap().pid;
    assert_eq!((&me["pid"], &me["pgid"], &me["sid"]), (&json!(pid), &json!(pid), &json!(pid)), "{me}");
    assert_ne!(me["tty"], "opened", "no terminal: {me}");
}

/// Condition 2: an agent that isn't Claude gets none of Claude's keys or settings from tarae's environment
/// (`ANTHROPIC_*`, `CLAUDE*`, what a Claude Code session sets for what it starts), none of the wikis' tokens and
/// none of tarae's own sockets — the rest as it is — and the descriptor limit tarae started with: what the
/// stand-in heard.
#[test]
fn an_agent_gets_none_of_claudes_keys_tokens_or_tarae_s_sockets() {
    let t: toml::Table = toml::from_str(
        r#"v = [{ base = "https://corp.example/w", kind = "confluence-dc", token-env = "TARAE_TEST_PAT" }]"#,
    )
    .unwrap();
    let cfg = LlmConfig { signed_in: crate::web::signed::wikis_of(&t["v"]).unwrap(), ..LlmConfig::default() };
    let left_out = [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDECODE",
        "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
        "AI_AGENT",
        "TRACEPARENT",
        "TARAE_TEST_PAT",
        "TARAE_UI",
        "TARAE_PTY_SOCK",
        "SSH_AUTH_SOCK",
    ];
    let mut env: Env = left_out.iter().map(|n| (n.to_string(), "tarae-fake-token".to_string())).collect();
    env.insert("TARAE_TEST_KEPT".into(), "1".into());
    let mut script = handshake();
    script.push(json!({"eof": true}));
    let run = start(json!(script), Opts { env, cfg, ..Opts::default() });
    run.until("ready");
    let heard = run.heard();
    let names: Vec<&str> = heard[0]["env"].as_array().unwrap().iter().filter_map(|n| n.as_str()).collect();
    assert!(names.contains(&"TARAE_TEST_KEPT"), "the rest as it is: {names:?}");
    let got: Vec<&&str> = left_out.iter().filter(|n| names.contains(n)).collect();
    assert!(got.is_empty(), "passed on: {got:?}");
    assert!(!names.iter().any(|n| n.starts_with("ANTHROPIC_") || n.starts_with("CLAUDE")), "{names:?}");
    let mut l = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: a plain call filling a struct of ours.
    unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut l) };
    let started_with = crate::nofile::original().unwrap_or(l.rlim_cur);
    assert_eq!(heard[0]["nofile"], started_with, "the limit tarae started with");
}

/// Tests never start a real agent: the wire's `command` refuses `agy`, `gemini` and `claude`, by name or path,
/// before anything starts.
#[test]
fn a_test_never_starts_a_real_agent() {
    let cfg = LlmConfig::default();
    for program in ["agy", "/opt/homebrew/bin/gemini", "/nonexistent/claude"] {
        let row: &'static crate::agent::Row = Box::leak(Box::new(crate::agent::Row {
            name: "real",
            wire: crate::agent::Wire::Acp,
            command: program,
            args: &["--acp"],
            caps: CAPS,
        }));
        let s = Start::on(row, Role::Chat, &cfg);
        let refused = std::panic::catch_unwind(|| command(&s, &std::env::temp_dir(), None).map(|_| ()));
        let why = refused.expect_err(program);
        let why = why.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(why.contains("a test started a real agent"), "{program}: {why}");
    }
    // (and the stand-in's own isn't refused)
    assert!(command(&Start::on(&STAND_IN, Role::Chat, &cfg), &std::env::temp_dir(), None).is_ok());
}

// ── The bridge ───────────────────────────────────────────────────────────────────────

/// An MCP message of the stand-in's to tarae's server, through the bridge.
fn mcp(message: Value) -> Value {
    json!({"mcp": message})
}

/// Whether process `pid` is gone, once it is (an orphan's reaper may take a moment).
fn gone(pid: u64) -> bool {
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    loop {
        // SAFETY: signal 0 only asks whether the process is there.
        if unsafe { libc::kill(pid as libc::pid_t, 0) } != 0 {
            return true;
        }
        if std::time::Instant::now() > end {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// tarae's tools asked: the session opens with the bridge as its one MCP server — the relay on the socket, the token
/// only in its `env` — and the stand-in's server, started as an agent starts one, reaches the wire. A `tools/call` comes
/// up once, from the bridge (finding C): its `Tool`, named as Claude's client names tarae's tools, right before its
/// `Mcp`, and its answer goes back to the agent and comes up as its `ToolResult`; the agent's own report of the call
/// (`mcp_tarae_read`, agy's name) and its update are dropped. `initialize` and a notification are `Mcp` alone, and
/// no answer to a notification goes back.
#[test]
fn a_call_through_the_bridge_comes_up_once_from_the_bridge() {
    let reported = json!({"say": {"sessionUpdate": "tool_call", "toolCallId": "agy-1", "name": "mcp_tarae_read",
        "title": "Run mcp_tarae_read", "kind": "other", "status": "pending", "locations": null, "rawInput": {}}});
    let finished = json!({"say": {"sessionUpdate": "tool_call_update", "toolCallId": "agy-1",
        "status": "completed", "content": null}});
    let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "read", "arguments": {"files": ["a.rs"]}}});
    let mut script = handshake();
    script.extend([
        json!({"expect": "session/prompt"}),
        reported,
        mcp(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}})),
        mcp(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
        mcp(call),
        finished,
        json!({"end_turn": "end_turn"}),
        json!({"eof": true}),
    ]);
    let run = start(json!(script), Opts { prompt: Some("read a.rs"), tools: true, ..Opts::default() });
    let got = run.until("mcp");
    assert_eq!(
        got.last().unwrap(),
        r#"mcp bridge-1 {"id":1,"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#
    );
    let server = &run.of("session/new")[0]["params"]["mcpServers"][0];
    assert_eq!((&server["name"], &server["env"][0]["name"]), (&json!("tarae"), &json!(bridge::TOKEN)));
    let token = server["env"][0]["value"].as_str().unwrap();
    assert!(token.len() == 64 && !server["args"].to_string().contains(token), "the token in its env only");
    run.agent().answer_mcp(
        "bridge-1",
        json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": "2025-06-18"}}),
    );
    assert!(run.until("mcp").last().unwrap().starts_with("mcp bridge-2 "));
    // (as `tools::mcp` answers a notification — kept from the agent)
    run.agent().answer_mcp("bridge-2", json!({"jsonrpc": "2.0", "result": {}, "id": 0}));
    let got = run.until("mcp");
    let n = got.len();
    assert_eq!(
        got[n - 2],
        r#"tool bridge-3 mcp__tarae__read {"files":["a.rs"]}"#,
        "its call right before it"
    );
    assert!(got[n - 1].starts_with("mcp bridge-3 "), "{got:?}");
    let text = json!({"content": [{"type": "text", "text": "1: fn a() {}"}]});
    run.agent().answer_mcp("bridge-3", json!({"jsonrpc": "2.0", "id": 2, "result": text}));
    let got = run.until("done");
    assert!(got.contains(&"result bridge-3 false 1: fn a() {}".to_string()), "{got:?}");
    let all = run.heard();
    let answers: Vec<&Value> = all.iter().filter_map(|v| v.get("mcp_answer")).collect();
    assert_eq!(answers.len(), 2, "initialize's and the call's: {answers:?}");
    assert!(
        !all.iter().any(|v| v.get("mcp_other").is_some()),
        "nothing else, the notification's none: {all:?}"
    );
    assert_eq!(answers[1]["result"], text, "the answer back to the agent");
}

/// Condition 4: an agent's process group ends with it — the bridge's relay, and a process of its own that would
/// outlive it — and the bridge's socket goes with it.
#[test]
fn the_agents_group_and_its_bridge_end_with_it() {
    let mut script = handshake();
    script.extend([
        mcp(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})),
        json!({"linger": true}),
        json!({"eof": true}),
    ]);
    let mut run = start(json!(script), Opts { tools: true, ..Opts::default() });
    run.until("mcp bridge-1");
    run.agent().answer_mcp("bridge-1", json!({"jsonrpc": "2.0", "id": 1, "result": {}}));
    let heard = run.heard_until(|v| v.get("linger_pid").is_some());
    let pid = |k: &str| heard.iter().find_map(|v| v[k].as_u64()).unwrap();
    let (relay, linger) = (pid("mcp_pid"), pid("linger_pid"));
    let args = &run.of("session/new")[0]["params"]["mcpServers"][0]["args"];
    let socket = std::path::PathBuf::from(args.as_array().unwrap().last().unwrap().as_str().unwrap());
    // SAFETY: signal 0 only asks whether the process is there.
    let alive = |pid: u64| unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
    assert!(socket.exists() && alive(relay) && alive(linger), "all there while it runs");
    run.started = None;
    assert!(run.until("exit").last().unwrap().starts_with("exit"));
    assert!(gone(relay), "the bridge's relay outlived its agent");
    assert!(gone(linger), "a process of the agent's group outlived it");
    assert!(!socket.exists(), "the bridge's socket outlived its agent");
}
