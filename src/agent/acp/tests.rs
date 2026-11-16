//! The ACP wire's tests: each message read, line → `Event`, in the real agy's shape where agy sent it (the design
//! session's probe, condition 7), the dropped ones included; then the wire over the stand-in agent (`standin`).

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, channel};

use serde_json::{Value, json};

use super::standin::{HEARD, SCRIPT};
use super::*;
use crate::agent::hook;
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

/// A session answer's `modes`: in `now`, agy's two offered.
fn modes(now: &str) -> Value {
    json!({"currentModeId": now, "availableModes": [{"id": "default", "name": "Default"}, {"id": "plan", "name": "Plan"}]})
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
        Event::Exit(Exit { said, kind: Ended::Tripped { tool } }) => format!("exit tripped {tool}: {said}"),
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

/// A turn's answer: agy's `usage` counted (`thoughtTokens` apart), its model from `configOptions`, and each
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

/// The turns agy 1.5.9 reported usage for in step 4's two probes (their `summary.md`s' #3) — in, out, cache read,
/// thought, `totalTokens`: probe 1's uncached turns 1, 4, 8 and 13, its cached 2, 3, 5, 6, 7, 9 and 10, then probe
/// 2's cached 1, 2 and 4; last probe 1's turns 11 (uncached) and 12 (cached), whose usage is on its `wire.jsonl`
/// though its `turns.jsonl` kept them as attempts with no answer (L-B6). Every cache write was 0.
const AGY_1_5_9_TURNS: [(u64, u64, u64, u64, u64); 16] = [
    (10630, 73, 0, 38, 10703),
    (12416, 360, 0, 251, 12776),
    (10680, 156, 0, 122, 10836),
    (10758, 36, 0, 31, 10794),
    (2423, 146, 8576, 36, 11145),
    (2400, 81, 8950, 47, 11431),
    (2805, 70, 9301, 29, 12176),
    (2689, 173, 10465, 136, 13327),
    (2479, 64, 11806, 27, 14349),
    (9013, 292, 36161, 152, 45466),
    (13043, 419, 8731, 245, 22193),
    (15154, 263, 17404, 186, 32821),
    (16287, 508, 19460, 352, 36255),
    (17412, 311, 26799, 200, 44522),
    (10775, 109, 0, 72, 10884),
    (2363, 635, 8721, 548, 11719),
];

/// One of `AGY_1_5_9_TURNS` as agy's `usage`.
fn agy_1_5_9_usage((input, output, read, thought, total): (u64, u64, u64, u64, u64)) -> Value {
    json!({"inputTokens": input, "outputTokens": output, "cachedReadTokens": read, "cachedWriteTokens": 0,
        "thoughtTokens": thought, "totalTokens": total})
}

/// `:usage`'s text for `u`, at a fixed time.
fn usage_said(u: &crate::usage::Usage) -> String {
    crate::usage::report("s", u, &Default::default(), &[], 100, crate::log::Zone::Here)
}

/// L-B5 (#3): agy's usage counted as 1.5.9 reported it. On each of the probes' turns, cached and uncached, tarae's
/// total (its context and its output) is `totalTokens`, and the thinking stays apart: `Spent.thought`, never added in
/// (its output count holds it, L-B6). Over the 16 turns the tally's total is their totals' sum, its thinking theirs,
/// kept with the conversation, and `:usage`'s line on cost says it is part of the output.
#[test]
fn agys_usage_counts_as_1_5_9_reported() {
    let mut u = crate::usage::Usage::of(&CAPS);
    let (mut total, mut thought) = (0, 0);
    for (i, turn) in AGY_1_5_9_TURNS.into_iter().enumerate() {
        let usage = agy_1_5_9_usage(turn);
        let s = spent(&usage, "s-1".into(), i as u64 + 1).unwrap();
        assert_eq!(s.counts.context().unwrap() + s.counts.output.unwrap(), turn.4, "{usage}");
        assert_eq!(s.thought, Some(turn.3), "{usage}");
        u.spent(s);
        (total, thought) = (total + turn.4, thought + turn.3);
    }
    let t = u.totals();
    assert_eq!((t.counts.context().unwrap() + t.counts.output.unwrap(), t.thought), (total, Some(thought)));
    let mut back = crate::usage::Usage::of(&CAPS);
    back.restore("s-1", &u.conv_json("s-1").unwrap());
    assert_eq!(back.totals().thought, Some(thought), "kept with the conversation");
    let line = format!(
        "\nThis agent doesn't report what its turns cost. Its thinking — {} tokens — is part of the output count \
         below.\n",
        crate::usage::tokens(thought)
    );
    let said = usage_said(&u);
    assert!(said.contains(&line), "{said}");
    let none = usage_said(&crate::usage::Usage::of(&CAPS));
    assert!(none.contains("\nThis agent doesn't report what its turns cost.\n"), "no thinking yet: {none}");
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
    /// A hook file laid over tarae's once its folder is made (`config/hooks.json`): the stand-in reads it.
    hooks: Option<String>,
}

fn start(script: Value, o: Opts) -> Run {
    let dir = crate::scratch::Scratch::new("acp-stand-in");
    std::fs::write(dir.join("script.json"), script.to_string()).unwrap();
    let mut env = o.env;
    env.insert(SCRIPT.into(), dir.join("script.json").to_string_lossy().into_owned());
    env.insert(HEARD.into(), dir.join("heard").to_string_lossy().into_owned());
    // (where a test's bridge binds and its agent's folder is: its own state folder)
    env.insert("XDG_STATE_HOME".into(), dir.join("state").to_string_lossy().into_owned());
    // (its folder made as the chat's gate makes it, on this thread)
    let made = crate::agentgate::prepare(&STAND_IN, state(Some(&env)).as_deref(), dir.path())
        .expect("its folder made");
    if let Some(hooks) = &o.hooks {
        std::fs::write(made.prepared.folder().join("config/hooks.json"), hooks).unwrap();
    }
    let s = Start {
        system: o.system,
        resume: o.resume.map(|conversation| Resume { conversation, fork: false }),
        prepared: Some(&made.prepared),
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

/// L-B5: a turn with no `usage` — an answer without one, or a turn cut off (probe 2's turn 3: its stdin ended
/// mid-turn, no answer at all) — sends no usage, so nothing is said of it, and the tally (`:usage`'s turns, totals and
/// text) stays as the turn before left it, the cut's end (`Usage::cut_off`, the chat's on an exit) included.
#[test]
fn a_turn_with_no_usage_leaves_the_tally_and_says_nothing() {
    let usage = agy_1_5_9_usage(AGY_1_5_9_TURNS[11]);
    let text = |t: &str| json!({"say": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": t}}});
    let mut script = handshake();
    script.extend([
        json!({"expect": "session/prompt"}),
        text("One"),
        json!({"end_turn": "end_turn", "usage": usage}),
        json!({"expect": "session/prompt"}),
        text("Two"),
        json!({"end_turn": "end_turn"}),
        json!({"expect": "session/prompt"}),
        json!({"eof": true}),
    ]);
    let mut run = start(json!(script), Opts { prompt: Some("one"), ..Opts::default() });
    let spent_said = |got: &[String]| got.iter().filter(|g| g.starts_with("spent ")).count();
    let first = run.until("done");
    assert_eq!(spent_said(&first), 1, "the turn with usage: {first:?}");
    run.agent().prompt(Prompt::Text("two".into())).unwrap();
    let second = run.until("done");
    assert_eq!((spent_said(&second), second.last().unwrap().as_str()), (0, "done Two"), "{second:?}");
    run.agent().prompt(Prompt::Text("cut".into())).unwrap();
    run.heard_until(|v| v["method"] == "session/prompt" && v["params"]["prompt"][0]["text"] == "cut");
    // (its stdin ended mid-turn, as probe 2's run C's)
    run.started = None;
    let cut = run.until("exit");
    assert_eq!(spent_said(&cut), 0, "the turn cut off: {cut:?}");
    // The chat's side: the first turn's usage, then the cut's end — the tally as it was, `:usage` saying the same
    let mut u = crate::usage::Usage::of(&CAPS);
    u.spent(spent(&usage, "s-1".into(), 1).unwrap());
    let before = (u.turns.clone(), u.totals(), usage_said(&u));
    assert!(
        spent(&json!({"stopReason": "end_turn"})["usage"], "s-1".into(), 2).is_none(),
        "no usage, none sent"
    );
    u.cut_off(3);
    assert_eq!((u.turns.clone(), u.totals(), usage_said(&u)), before);
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
    let script = json!([expect("initialize", agy_initialize()),
        expect("session/resume", json!({"configOptions": [], "modes": modes("default")})),
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
        {"answer": {"configOptions": [], "modes": modes("default")}},
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

/// The design session's note 1 on L-B2: an agent that isn't Claude gets none of the names that move git's discovery
/// (`GIT_DISCOVERY`), so a git it runs finds its repository from its cwd, as `agenttrust::chain` does — the rest as it
/// is. (Their values point nowhere: the stand-in runs no git.) The names are the test's own list, so one taken out of
/// `GIT_DISCOVERY` reaches the agent here: `GIT_OBJECT_DIRECTORY` among them (the design session's item on L-B3:
/// git's `is_git_directory` takes it in place of `objects/`).
#[test]
fn an_agent_gets_none_of_the_names_that_move_gits_discovery() {
    let moving = [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    ];
    let mut env: Env = moving.iter().map(|n| (n.to_string(), "tarae-fake-nowhere".to_string())).collect();
    env.insert("GIT_AUTHOR_NAME".into(), "kept".into());
    let mut script = handshake();
    script.push(json!({"eof": true}));
    let run = start(json!(script), Opts { env, ..Opts::default() });
    run.until("ready");
    let heard = run.heard();
    let names: Vec<&str> = heard[0]["env"].as_array().unwrap().iter().filter_map(|n| n.as_str()).collect();
    assert!(names.contains(&"GIT_AUTHOR_NAME"), "the rest as it is: {names:?}");
    let got: Vec<&&str> = moving.iter().filter(|n| names.contains(n)).collect();
    assert!(got.is_empty(), "passed on: {got:?}");
    assert_eq!(GIT_DISCOVERY, moving, "the list as the test's");
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
            own_tools: None,
            mode: None,
            held: &[],
        }));
        let s = Start::on(row, Role::Chat, &cfg);
        let refused = std::panic::catch_unwind(|| command(&s, &std::env::temp_dir(), None).map(|_| ()));
        let why = refused.expect_err(program);
        let why = why.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(why.contains("a test started a real agent"), "{program}: {why}");
    }
    // (and the stand-in's own isn't refused, its folder made)
    let dir = crate::scratch::Scratch::new("acp-real-agent");
    let made = crate::agentgate::prepare(&STAND_IN, Some(dir.path()), dir.path()).unwrap();
    let s = Start { prepared: Some(&made.prepared), ..Start::on(&STAND_IN, Role::Chat, &cfg) };
    assert!(command(&s, &std::env::temp_dir(), None).is_ok());
}

// ── Guard 1: its own tools off ─────────────────────────────────────────────────────

/// Step 3's guard 1, end to end on the stand-in: a start makes its folder (`$state/agent-homes/<row>/<key>/`, the
/// project's — here a folder in no repository, keyed by its real path (L-B6) —, 0700 down to `config/`), writes tarae's two files there to the byte — the hook file (0600) naming the program beside it
/// (0700) — and names it with `--gemini-dir`, tarae's path for the hook in its environment. The stand-in runs its
/// own `run_command` as agy does: the hook it reads there asks tarae (the program, then the test binary's
/// `hook::child`), which refuses it, and the call comes up as a refused row (step 3's guard 3: no trip at `failed`),
/// nothing asked; `mcp_tarae_search` is let through, asked for as agy 1.5.9 asks (answered `allow_once` here, as the
/// chat's rule would), and called through the bridge, as agy calls it (its report dropped, as tarae's own tools' are,
/// and matched with the bridge's call: no trip at the turn's end).
#[test]
fn a_start_makes_the_folder_writes_the_hook_and_an_own_tool_is_denied() {
    use std::os::unix::fs::PermissionsExt;
    let own = json!({"own": {"name": "run_command", "id": "t1", "args": {"CommandLine": "echo probe-ran"}}});
    // (the model's `1.0`, which the stand-in writes `1` on both sides, as 1.5.9 did: L-B4)
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "search", "arguments": {"pattern": "fn beta", "context": 1.0}}});
    let taraes = json!({"own": {"name": "mcp_tarae_search", "id": "t2",
        "args": {"pattern": "fn beta", "context": 1.0, "toolSummary": "Search fn beta"}, "mcp": call}});
    let mut script = handshake();
    script.extend([
        json!({"expect": "session/prompt"}),
        own,
        taraes,
        json!({"end_turn": "end_turn"}),
        json!({"eof": true}),
    ]);
    let run = start(json!(script), Opts { prompt: Some("run echo"), tools: true, ..Opts::default() });
    let mut got = run.until("ask ask-1 mcp_tarae_search");
    assert_eq!(
        got.iter().filter(|g| g.starts_with("ask ")).count(),
        1,
        "its own tool, refused, asks nothing"
    );
    run.agent().answer_permission("ask-1", Permission::Allow { input: Value::Null });
    got.extend(run.until("mcp bridge-1"));
    run.agent().answer_mcp("bridge-1", json!({"jsonrpc": "2.0", "id": 1, "result": {"content": []}}));
    got.extend(run.until("done"));
    let place = crate::trust::Place::File(std::fs::canonicalize(run.dir.path()).unwrap());
    let key = &crate::sha256::hex(crate::trust::key(&place).as_bytes())[..16];
    let row = run.dir.join("state/tarae/agent-homes/stand-in");
    let folder = row.join(key);
    let mode = |p: &std::path::Path| std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777;
    for dir in [row.parent().unwrap(), &row, &folder, &folder.join("config")] {
        assert_eq!(mode(dir), 0o700, "{}", dir.display());
    }
    let (hooks, program) = (folder.join("config/hooks.json"), folder.join("config").join(hook::SCRIPT_NAME));
    assert_eq!((mode(&hooks), mode(&program)), (0o600, 0o700));
    let want = hook::file(crate::agent::AGY_TOOLS_OFF.hook, program.to_str().unwrap());
    assert_eq!(std::fs::read_to_string(&hooks).unwrap(), want);
    assert_eq!(std::fs::read_to_string(&program).unwrap(), hook::SCRIPT);
    let heard = run.heard();
    assert_eq!(heard[0]["dir"], json!(folder.to_str().unwrap()), "--gemini-dir names it");
    assert!(heard[0]["env"].as_array().unwrap().contains(&json!(hook::ENV)), "{}", heard[0]);
    let hooks: Vec<&Value> = heard.iter().filter_map(|v| v.get("hook")).collect();
    assert_eq!(
        hooks,
        [&json!({"decision": "deny", "reason": hook::DENY}), &json!({"decision": "allow"})],
        "{heard:?}"
    );
    assert!(got.contains(&r#"tool t1 run_command {"CommandLine":"echo probe-ran"}"#.to_string()), "{got:?}");
    assert!(got.contains(&format!("result t1 true {REFUSED}")), "a refused row: {got:?}");
    let bridged = r#"tool bridge-1 mcp__tarae__search {"context":1,"pattern":"fn beta"}"#;
    assert!(got.contains(&bridged.to_string()), "the server got the rewritten number: {got:?}");
    assert!(!got.iter().any(|g| g.contains("t2")), "tarae's own, dropped: {got:?}");
    assert_eq!(got.last().unwrap(), "done ", "no trip: {got:?}");
}

/// A row that turns its own tools off isn't started without its folder made: the start ends with its words, and
/// no process was started (the stand-in heard nothing).
#[test]
fn a_row_with_a_recipe_isnt_started_without_its_folder() {
    let dir = crate::scratch::Scratch::new("acp-no-folder");
    let env = Env::from([
        (SCRIPT.to_string(), dir.join("script.json").to_string_lossy().into_owned()),
        (HEARD.to_string(), dir.join("heard").to_string_lossy().into_owned()),
    ]);
    std::fs::write(dir.join("script.json"), json!([{"eof": true}]).to_string()).unwrap();
    let cfg = LlmConfig::default();
    let s = Start::on(&STAND_IN, Role::Chat, &cfg);
    let at = crate::agent::Place { dir: dir.path(), given: Some(&env), cred: None };
    let first = First { tools: false, prompt: None };
    let ed = crate::editor::Editor::new(crate::config::Config::default());
    let started = crate::agent::start(&s, at, first, false, ed.events.sender(), |_| None);
    let Err(e) = started else { panic!("started without its folder") };
    assert_eq!(e.said("sh", true), "stand-in: not started without tarae's folder");
    assert!(!dir.join("heard").exists(), "a process started");
}

// ── The bridge ───────────────────────────────────────────────────────────────────────

/// An MCP message of the stand-in's to tarae's server, through the bridge.
fn mcp(message: Value) -> Value {
    json!({"mcp": message})
}

/// Whether process `pid` is gone, once it is. The agent's group's processes are orphans once it's reaped, gone when
/// whoever adopts them reaps them — a moment's wait: in a real tarae on Linux tarae itself, a child subreaper
/// (`reaper::at_start`) that takes them as adopted (in a session neither its own nor tarae's) and reaps them
/// (`Reaper::round`); on macOS launchd; in a test, which makes no subreaper, init — under `cargo xtask linux` the
/// container's pid 1, its `sh`, which reaps every child it's given.
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
    // (agy's report: its arguments the call's, and its `toolSummary` the server never gets)
    let args = json!({"files": ["a.rs"], "toolSummary": "Read a.rs"});
    let reported = json!({"say": {"sessionUpdate": "tool_call", "toolCallId": "agy-1", "name": "mcp_tarae_read",
        "title": "Run mcp_tarae_read", "kind": "other", "status": "pending", "locations": null, "args": args,
        "rawInput": args}});
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

// ── With no process (`Staged`) ───────────────────────────────────────────────────────────

/// A wire on the stand-in's row with no process, its handshake answered in agy's shapes (`initialize` 0, the session
/// 1, its answer `session`): what came up, said.
fn staged(session: Value) -> (Staged, Agent, Vec<String>) {
    let (mut st, agent) = Staged::new(&std::env::temp_dir());
    let mut got = st.line(&json!({"jsonrpc": "2.0", "id": 0, "result": agy_initialize()}));
    got.extend(st.line(&json!({"jsonrpc": "2.0", "id": 1, "result": session})));
    (st, agent, got.into_iter().filter_map(said).collect())
}

/// A `session/update` line.
fn update_line(u: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s-1", "update": u}})
}

/// What `events` say.
fn says(events: Vec<Event>) -> Vec<String> {
    events.into_iter().filter_map(said).collect()
}

/// A prompt sent from the chat's handle and taken by the wire: what it wrote.
fn prompted(st: &mut Staged, agent: &Agent) -> Vec<Value> {
    agent.prompt(Prompt::Text("go".into())).unwrap();
    st.take();
    st.written()
}

/// The turn's answer (`session/prompt` was request `id`): what came up.
fn turn_over(st: &mut Staged, id: u64) -> Vec<String> {
    says(st.line(&json!({"jsonrpc": "2.0", "id": id, "result": {"stopReason": "end_turn"}})))
}

/// agy's report of tarae's `tool` (`mcp_tarae_<tool>`) as `id`, on `args` with its `toolSummary`, at `status`.
fn taraes_report(id: &str, tool: &str, args: Value, status: &str) -> Value {
    let mut args = args;
    args["toolSummary"] = json!(format!("Run {tool}"));
    update_line(json!({"sessionUpdate": "tool_call", "toolCallId": id, "name": format!("mcp_tarae_{tool}"),
        "title": format!("Run mcp_tarae_{tool}"), "kind": "other", "status": status, "locations": null,
        "args": args, "rawInput": args}))
}

/// The bridge's `tools/call` of `tool` on `arguments`.
fn bridge_call(n: u64, tool: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": n, "method": "tools/call", "params": {"name": tool, "arguments": arguments}})
}

/// Guard 2: a session that opens in another mode it offers is set to the row's (`session/set_mode`) before anything
/// else — the session isn't said (`Conversation`, `Ready`) and the first prompt isn't written until its answer.
#[test]
fn a_session_in_another_mode_is_set_to_the_rows() {
    let (mut st, agent, got) = staged(json!({"sessionId": "s-1", "modes": modes("plan")}));
    assert!(got.is_empty(), "nothing said before the mode's set: {got:?}");
    let set = json!({"jsonrpc": "2.0", "id": 2, "method": "session/set_mode",
        "params": {"sessionId": "s-1", "modeId": "default"}});
    let written = prompted(&mut st, &agent);
    assert_eq!(written.last(), Some(&set), "{written:?}");
    assert!(!written.iter().any(|w| w["method"] == "session/prompt"), "the prompt held: {written:?}");
    let got = says(st.line(&json!({"jsonrpc": "2.0", "id": 2, "result": {}})));
    assert_eq!(got, ["conversation s-1  None None", "ready 0 None false"]);
    let written = st.written();
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0]["method"], "session/prompt", "the prompt once the mode's set");
    // (and in the row's mode already: none set)
    let (mut st, agent, got) = staged(json!({"sessionId": "s-1", "modes": modes("default")}));
    assert_eq!(got, ["conversation s-1  None None", "ready 0 None false"]);
    let written = prompted(&mut st, &agent);
    assert!(!written.iter().any(|w| w["method"] == "session/set_mode"), "{written:?}");
}

/// Guard 2, fail-closed: a session that won't take the row's mode (an error at `session/set_mode`, on the stand-in),
/// doesn't offer it, or says no mode at all ends the start, said; no prompt is written.
#[test]
fn a_mode_it_wont_take_ends_the_start() {
    let session = |modes: Value| expect("session/new", json!({"sessionId": "s-1", "modes": modes}));
    let refused = json!({"expect": "session/set_mode", "error": -32603, "message": "not now"});
    let only_plan = json!({"currentModeId": "plan", "availableModes": [{"id": "plan"}]});
    for (script, words) in [
        (
            json!([expect("initialize", agy_initialize()), session(modes("plan")), refused, {"eof": true}]),
            "failed stand-in won't take mode default: not now",
        ),
        (
            json!([expect("initialize", agy_initialize()), session(only_plan), {"eof": true}]),
            "failed stand-in offers no mode default",
        ),
        (
            json!([expect("initialize", agy_initialize()), session(Value::Null), {"eof": true}]),
            "failed stand-in offers no mode default",
        ),
    ] {
        let run = start(script, Opts { prompt: Some("go"), ..Opts::default() });
        let got = run.until("exit");
        assert!(got.contains(&words.to_string()), "{words}: {got:?}");
        assert!(!got.iter().any(|g| g.starts_with("conversation")), "{got:?}");
        assert!(run.of("session/prompt").is_empty(), "{words}");
    }
}

/// Guard 2 mid-session, on the stand-in: a `current_mode_update` to another mode is set back to the row's; one it
/// won't take back stops it — said as the turn's end, its end an exit (no trip).
#[test]
fn a_mode_left_midway_is_set_back() {
    let moved = json!({"say": {"sessionUpdate": "current_mode_update", "currentModeId": "plan"}});
    let text =
        json!({"say": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "ok"}}});
    let mut script = handshake();
    script.extend([
        json!({"expect": "session/prompt"}),
        moved.clone(),
        expect("session/set_mode", json!({})),
        text,
        json!({"end_turn": "end_turn"}),
        json!({"expect": "session/prompt"}),
        moved,
        json!({"expect": "session/set_mode", "error": -32603, "message": "stuck"}),
        json!({"eof": true}),
    ]);
    let run = start(json!(script), Opts { prompt: Some("go"), ..Opts::default() });
    assert_eq!(run.until("done").last().unwrap(), "done ok");
    let set = run.of("session/set_mode");
    assert_eq!(set[0]["params"], json!({"sessionId": "s-1", "modeId": "default"}));
    run.agent().prompt(Prompt::Text("again".into())).unwrap();
    let got = run.until("exit");
    let words = "stand-in left mode default and won't go back — stopped";
    assert_eq!(got[got.len() - 2..], [format!("failed {words}"), format!("exit {words}")], "{got:?}");
}

/// Guard 2's bound (L3 note 4): a mode left and taken back is set back `MODE_BACKS` times in a turn; left once more, the
/// session stops, said — its turn's end and its end — and nothing more is written. A new turn counts from none.
#[test]
fn a_mode_left_again_and_again_in_a_turn_stops_it() {
    let left = update_line(json!({"sessionUpdate": "current_mode_update", "currentModeId": "plan"}));
    let back = |st: &mut Staged, times: usize| {
        for _ in 0..times {
            assert!(says(st.line(&left)).is_empty());
            let w = st.written();
            assert_eq!((w.len(), &w[0]["method"]), (1, &json!("session/set_mode")), "{w:?}");
            assert!(says(st.line(&json!({"jsonrpc": "2.0", "id": w[0]["id"], "result": {}}))).is_empty());
        }
    };
    let (mut st, agent, _) = staged(agy_session("s-1"));
    prompted(&mut st, &agent);
    back(&mut st, MODE_BACKS);
    assert_eq!(turn_over(&mut st, 2), ["done "]);
    // (a new turn: set back as many times again)
    let written = prompted(&mut st, &agent);
    assert_eq!(written[0]["method"], "session/prompt");
    back(&mut st, MODE_BACKS);
    let words = "stand-in keeps leaving mode default — set back 3 times this turn, then stopped";
    assert_eq!(says(st.line(&left)), [format!("failed {words}"), format!("exit {words}")]);
    assert!(st.written().is_empty(), "nothing written after the stop");
}

/// The tripwire's bound (L3 note 4): a call of its own still running keeps no input and its name only as far as it's
/// said, `OWN_OPEN` of them at most — the oldest let go. Its `completed` still trips, as one with no name.
#[test]
fn an_own_call_that_never_ends_keeps_no_input_and_so_many_at_most() {
    let name = format!("run_command{}", "x".repeat(4096));
    let call = |id: String| {
        update_line(json!({"sessionUpdate": "tool_call", "toolCallId": id, "name": name, "kind": "execute",
            "status": "in_progress", "rawInput": {"CommandLine": "y".repeat(1 << 16)}}))
    };
    let (mut st, agent, _) = staged(agy_session("s-1"));
    prompted(&mut st, &agent);
    for n in 0..=OWN_OPEN {
        st.line(&call(format!("t{n}")));
    }
    assert_eq!(st.w.reports.len(), OWN_OPEN, "so many at most");
    assert!(!st.w.reports.contains_key("t0"), "the oldest let go");
    let kept = |r: &Report| r.input.is_null() && r.name.chars().count() == NAME_SHOWN + 1;
    assert!(st.w.reports.values().all(kept), "no input, the name as far as it's said");
    let done =
        update_line(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t0", "status": "completed"}));
    let words = "stand-in ran a tool of its own, one with no name — stopped: it and everything it started ended. Only \
                 tarae's tools may run; its hook should have refused this.";
    assert_eq!(says(st.line(&done)).last(), Some(&format!("exit tripped one with no name: {words}")));
}

/// The design session's note on L-B1: the tools whose ask's answer the log was told are the agent's names, so
/// `ASKS_SAID_MAX` at most — past it nothing more is kept (or said); each ask is still answered.
#[test]
fn the_asks_said_are_so_many_at_most() {
    let (mut st, agent, _) = staged(agy_session("s-1"));
    prompted(&mut st, &agent);
    for n in 0..ASKS_SAID_MAX + 8 {
        let tool = format!("tool_{n}");
        let params = agy_ask(Some(&tool), &format!("Run {tool}"), "execute");
        let ask =
            json!({"jsonrpc": "2.0", "id": n, "method": "session/request_permission", "params": params});
        st.line(&ask);
        agent.answer_permission(&n.to_string(), Permission::Deny { message: String::new() });
        st.take();
        let deny = json!({"outcome": {"outcome": "selected", "optionId": "deny"}});
        assert_eq!(st.written(), [json!({"jsonrpc": "2.0", "id": n, "result": deny})], "answered {n}");
    }
    assert_eq!(st.w.asks_said.len(), ASKS_SAID_MAX, "so many at most");
    assert!(!st.w.asks_said.contains(&format!("tool_{ASKS_SAID_MAX}")), "past it, not kept");
}

/// Guard 3 on the stand-in: one of its own tools reported `completed` stops the session at once — never at its
/// `in_progress` before (Q5) — its group ended (a process of its own that would outlive it included), the turn
/// ended in tarae's words, its end a trip naming the tool; a prompt after it is never written.
#[test]
fn a_completed_own_tool_stops_the_session_and_says_it() {
    let call = |status: &str| {
        json!({"sessionUpdate": if status == "in_progress" { "tool_call" } else { "tool_call_update" },
        "toolCallId": "t1", "name": "run_command", "title": "echo ran", "kind": "execute", "status": status,
        "args": {"CommandLine": "echo ran"}, "rawInput": {"CommandLine": "echo ran"}})
    };
    let mut script = handshake();
    script.extend([
        json!({"expect": "session/prompt"}),
        json!({"say": call("in_progress")}),
        json!({"linger": true}),
        json!({"say": call("completed")}),
        json!({"eof": true}),
    ]);
    let run = start(json!(script), Opts { prompt: Some("go"), ..Opts::default() });
    let got = run.until("exit");
    let words = "stand-in ran a tool of its own, run_command — stopped: it and everything it started ended. Only \
                 tarae's tools may run; its hook should have refused this.";
    let n = got.len();
    assert_eq!(got[n - 3], r#"tool t1 run_command {"CommandLine":"echo ran"}"#, "{got:?}");
    assert_eq!(got[n - 2..], [format!("failed {words}"), format!("exit tripped run_command: {words}")]);
    let linger = run.heard().iter().find_map(|v| v["linger_pid"].as_u64()).expect("its process");
    assert!(gone(linger), "a process of its group outlived the trip");
    let _ = run.agent().prompt(Prompt::Text("again".into()));
    assert_eq!(run.of("session/prompt").len(), 1, "nothing written after the trip");
}

/// Guard 3: a report of tarae's tool that the bridge never carried, `completed`, trips at the turn's end — its row
/// first (it never had one), then the turn's end in tarae's words, then its end; a prompt waiting is never written.
/// As agy 1.5.7 reports it (no ask), and as 1.5.9 does (L-B1): `in_progress`, an ask for it allowed as the chat's
/// rule allows one for an offered tool (`allowed_ask`), then `completed` — the ask's allow is no call of the bridge's.
#[test]
fn an_unbridged_taraes_report_trips_at_the_turns_end() {
    for asked in [false, true] {
        let (mut st, agent, _) = staged(agy_session("s-1"));
        prompted(&mut st, &agent);
        let _bridge = st.link();
        let report = taraes_report("m1", "search", json!({"pattern": "x"}), "completed");
        if asked {
            let first = taraes_report("m1", "search", json!({"pattern": "x"}), "in_progress");
            assert!(says(st.line(&first)).is_empty(), "held until the turn's end");
            let mut params = agy_ask(Some("mcp_tarae_search"), "Run mcp_tarae_search", "other");
            params["toolCall"]["rawInput"] = json!({"pattern": "x"});
            let ask =
                json!({"jsonrpc": "2.0", "id": 7, "method": "session/request_permission", "params": params});
            assert_eq!(says(st.line(&ask)), [r#"ask 7 mcp_tarae_search {"pattern":"x"}"#]);
            assert!(allowed_ask("mcp_tarae_search", &["search".to_string()]), "the chat's rule allows it");
            agent.answer_permission("7", Permission::Allow { input: Value::Null });
            st.take();
            let allow = json!({"outcome": {"outcome": "selected", "optionId": "allow"}});
            assert_eq!(st.written(), [json!({"jsonrpc": "2.0", "id": 7, "result": allow})]);
            let done = update_line(json!({"sessionUpdate": "tool_call_update", "toolCallId": "m1",
                "status": "completed", "content": null}));
            assert!(says(st.line(&done)).is_empty(), "held until the turn's end");
        } else {
            assert!(says(st.line(&report)).is_empty(), "held until the turn's end");
        }
        agent.prompt(Prompt::Text("next".into())).unwrap();
        st.take();
        let words = "stand-in said it ran tarae's search, which tarae never ran — stopped: it and everything it \
                     started ended.";
        assert_eq!(
            turn_over(&mut st, 2),
            [
                r#"tool m1 mcp_tarae_search {"pattern":"x","toolSummary":"Run search"}"#.to_string(),
                format!("failed {words}"),
                format!("exit tripped mcp_tarae_search: {words}"),
            ],
            "asked {asked}"
        );
        assert!(st.written().is_empty(), "the prompt waiting, never written");
        assert!(says(st.line(&report)).is_empty(), "nothing read after it");
    }
}

/// The stand-in as agy 1.5.9 was measured (#5 and #13, the second probe's run A): a hook that doesn't answer within
/// its timeout is killed and the call `failed` — it fails closed: nothing asked, the call never made. Here a hook
/// that sleeps past a timeout of 0.3 s, in front of one of its own tools: a refused row, no ask, the hook (the pid
/// the stand-in started) gone.
#[test]
fn a_hook_past_its_timeout_fails_the_call_and_nothing_asks() {
    use std::os::unix::fs::PermissionsExt;
    let hooks = crate::scratch::Scratch::new("acp-slow-hook");
    let script = hooks.join("slow");
    std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let one = json!({"type": "command", "command": hook::quoted(script.to_str().unwrap()), "timeout": 0.3});
    let file = json!({"t": {"PreToolUse": [{"matcher": "*", "hooks": [one]}]}});
    let own = json!({"own": {"name": "run_command", "id": "t1", "args": {"CommandLine": "echo ran"}}});
    let mut script = handshake();
    script.extend([
        json!({"expect": "session/prompt"}),
        own,
        json!({"end_turn": "end_turn"}),
        json!({"eof": true}),
    ]);
    let opts = Opts { prompt: Some("go"), hooks: Some(file.to_string()), ..Opts::default() };
    let run = start(json!(script), opts);
    let got = run.until("done");
    assert!(got.contains(&format!("result t1 true {REFUSED}")), "a refused row: {got:?}");
    assert!(!got.iter().any(|g| g.starts_with("ask ")), "nothing asked: {got:?}");
    let heard = run.heard();
    let hook = heard.iter().find_map(|v| v.get("hook")).expect("its hook asked");
    assert_eq!(hook["failed"], "timeout", "{hook}");
    let pid = hook["pid"].as_u64().filter(|&p| p > 0).expect("the hook's pid");
    assert!(gone(pid), "the hook outlived its timeout");
}

/// Guard 3's match (the design session's condition 1), in agy's real shape: a report of tarae's matches a bridge call
/// of the turn by its tool and the call's every argument, the same — its `toolSummary`, which the server never gets,
/// beside them — whichever comes first, and no trip. A report whose value differs isn't the call's, nor is a second
/// report of one call: each trips at the turn's end.
#[test]
fn a_bridged_call_doesnt_trip_whichever_comes_first() {
    let args = json!({"pattern": "fn beta"});
    let report = |id: &str, args: Value| taraes_report(id, "search", args, "completed");
    let turn = |first_report: bool, reports: &[Value]| {
        let (mut st, agent, _) = staged(agy_session("s-1"));
        prompted(&mut st, &agent);
        let _bridge = st.link();
        let mut got = Vec::new();
        if !first_report {
            got.extend(says(st.bridged(bridge_call(1, "search", args.clone()))));
        }
        for r in reports {
            got.extend(says(st.line(r)));
        }
        if first_report {
            got.extend(says(st.bridged(bridge_call(1, "search", args.clone()))));
        }
        got.extend(turn_over(&mut st, 2));
        got
    };
    for first_report in [true, false] {
        let got = turn(first_report, &[report("m1", args.clone())]);
        assert_eq!(got.last().unwrap(), "done ", "report first {first_report}: {got:?}");
        assert!(!got.iter().any(|g| g.contains("m1")), "its report dropped: {got:?}");
    }
    let tripped = |got: &[String]| got.last().is_some_and(|g| g.starts_with("exit tripped mcp_tarae_search"));
    let other = report("m1", json!({"pattern": "fn alpha"}));
    let got = turn(true, &[other]);
    assert!(tripped(&got), "another value: {got:?}");
    let got = turn(false, &[report("m1", args.clone()), report("m2", args.clone())]);
    assert!(tripped(&got) && got.iter().any(|g| g.starts_with("tool m2 ")), "one call, two reports: {got:?}");
    // (the rule itself: every key of the call's, the same; the report may hold more)
    assert!(carried(&json!({"a": 1}), &json!({"a": 1, "toolSummary": "s"})));
    assert!(!carried(&json!({"a": 1, "b": 2}), &json!({"a": 1})));
    assert!(carried(&Value::Null, &json!({"a": 1})), "a call with no arguments");
}

/// Whether a turn trips with the bridge's `search` call on `call` and agy's report of it on `report` (its
/// `toolSummary` beside), the report first and last: the same answer both ways, or the test fails.
fn trips_on(call: &Value, report: &Value) -> bool {
    let turn = |report_first: bool| {
        let (mut st, agent, _) = staged(agy_session("s-1"));
        prompted(&mut st, &agent);
        let _bridge = st.link();
        let line = taraes_report("m1", "search", report.clone(), "completed");
        let mut got = Vec::new();
        if report_first {
            got.extend(says(st.line(&line)));
        }
        got.extend(says(st.bridged(bridge_call(1, "search", call.clone()))));
        if !report_first {
            got.extend(says(st.line(&line)));
        }
        got.extend(turn_over(&mut st, 2));
        got.last().is_some_and(|g| g.starts_with("exit tripped mcp_tarae_search"))
    };
    let first = turn(true);
    assert_eq!(first, turn(false), "either order: {call} against {report}");
    first
}

/// L-B4 (#15): agy 1.5.9 rewrites the model's numbers — `1.0` as `1`, a 23-digit integer as
/// `1.2345678901234568e+22` (the second probe, measured) — and a report so written still carries the bridge's call:
/// numbers by value, nested ones too, whichever side was rewritten.
#[test]
fn a_report_with_its_numbers_rewritten_matches_its_bridge_call() {
    let read = |s: &str| serde_json::from_str::<Value>(s).unwrap();
    let (big, its_float) = (read("12345678901234567890123"), read("1.2345678901234568e+22"));
    let nested = |one: Value, two: Value| json!({"o": {"a": {"b": [one, two, "x"]}, "z": null}, "s": ""});
    for (call, report) in [
        (json!({"n": 1.0}), json!({"n": 1})),
        (json!({"n": 1}), json!({"n": 1.0})),
        (json!({"n": big}), json!({"n": its_float})),
        (json!({"n": 0.1}), json!({"n": 0.1})),
        (json!({"n": -7.0}), json!({"n": -7})),
        (nested(json!(1.0), json!(2)), nested(json!(1), json!(2.0))),
    ] {
        assert!(!trips_on(&call, &report), "{call} against {report}");
    }
}

/// L-B4's other side: a report whose number is another still trips at the turn's end — at the top, nested, or an
/// array or object of another shape around the same numbers.
#[test]
fn a_report_with_another_number_still_trips() {
    let nested = |b: Value| json!({"o": {"a": {"b": b}}});
    for (call, report) in [
        (json!({"n": 1}), json!({"n": 2})),
        (json!({"n": 0.1}), json!({"n": 0.2})),
        (json!({"n": 7}), json!({"n": 7.5})),
        (json!({"n": 1}), json!({"n": "1"})),
        (nested(json!([1, 2])), nested(json!([1, 2.5]))),
        (nested(json!([1, 2])), nested(json!([1, 2, 3]))),
        (nested(json!({"x": 1})), nested(json!({"x": 1, "y": 2}))),
    ] {
        assert!(trips_on(&call, &report), "{call} against {report}");
    }
}

/// L-B4's strict side: two integers are compared exactly, so one that differs only past f64's precision — 2^53 + 1
/// against 2^53, `u64::MAX` against one less, `i64::MIN` against one more, integers on both sides — still trips. (A
/// 23-digit integer can't be held so: tarae's JSON, serde_json without `arbitrary_precision`, reads it as its nearest
/// f64, so two that differ in their last digit are one value before any compare.)
#[test]
fn a_report_with_an_integer_off_past_f64s_precision_still_trips() {
    for (call, report) in [
        (json!({"n": 9007199254740993u64}), json!({"n": 9007199254740992u64})),
        (json!({"n": u64::MAX}), json!({"n": u64::MAX - 1})),
        (json!({"n": i64::MIN}), json!({"n": i64::MIN + 1})),
        (json!({"o": [u64::MAX]}), json!({"o": [u64::MAX - 1]})),
    ] {
        let f64_of = |v: &Value| v.pointer("/n").or(v.pointer("/o/0")).and_then(Value::as_f64);
        assert_eq!(f64_of(&call), f64_of(&report), "the same as f64s: {call} against {report}");
        assert!(trips_on(&call, &report), "{call} against {report}");
    }
    let read = |s: &str| serde_json::from_str::<Value>(s).unwrap();
    assert_eq!(read("12345678901234567890123"), read("12345678901234567890124"), "read as one f64");
}

/// Guard 3: one of its own tools that failed — its hook refused it: `in_progress`, then `failed` (Q5's measure) — is
/// a refused row in tarae's words, never a trip; the turn ends as ever.
#[test]
fn a_failed_own_tool_is_a_refused_row_not_a_trip() {
    let (mut st, agent, _) = staged(agy_session("s-1"));
    prompted(&mut st, &agent);
    let running = update_line(
        json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "name": "run_command",
        "title": "echo ran", "kind": "execute", "status": "in_progress", "rawInput": {"CommandLine": "echo ran"}}),
    );
    let reason = json!({"type": "content", "content": {"type": "text", "text": hook::DENY}});
    let failed =
        update_line(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "failed",
        "content": [reason]}));
    assert_eq!(says(st.line(&running)), [r#"tool t1 run_command {"CommandLine":"echo ran"}"#]);
    assert_eq!(says(st.line(&failed)), [format!("result t1 true {REFUSED}")]);
    assert_eq!(turn_over(&mut st, 2), ["done "]);
}

/// The L3 notes' 3: one of its own refused in its first report (`tool_call` already `failed`, the other shape of
/// agy's two) gets its row and the refused result in tarae's words, as one refused in its update does.
#[test]
fn a_call_refused_in_its_first_report_gets_its_row_and_the_refused_result() {
    let (mut st, agent, _) = staged(agy_session("s-1"));
    prompted(&mut st, &agent);
    let reason = json!({"type": "content", "content": {"type": "text", "text": hook::DENY}});
    let refused = update_line(
        json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "name": "run_command", "title": "echo ran",
        "kind": "execute", "status": "failed", "rawInput": {"CommandLine": "echo ran"}, "content": [reason]}),
    );
    assert_eq!(
        says(st.line(&refused)),
        [
            r#"tool t1 run_command {"CommandLine":"echo ran"}"#.to_string(),
            format!("result t1 true {REFUSED}")
        ]
    );
    assert_eq!(turn_over(&mut st, 2), ["done "]);
}

/// The L3 notes' 2: a tool named the prefix alone (`mcp_tarae_`) is one of its own everywhere — its row shows, and
/// at `failed` its refused result; `update` asks `taraes`, as the tripwire does.
#[test]
fn a_tool_named_the_prefix_alone_is_its_own_and_its_refused_row_shows() {
    let (mut st, agent, _) = staged(agy_session("s-1"));
    prompted(&mut st, &agent);
    let running = update_line(json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "name": OWN,
        "title": "Run", "kind": "other", "status": "in_progress", "rawInput": {}}));
    let failed =
        update_line(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "failed"}));
    assert_eq!(says(st.line(&running)), ["tool t1 mcp_tarae_ {}"]);
    assert_eq!(says(st.line(&failed)), [format!("result t1 true {REFUSED}")]);
    assert_eq!(turn_over(&mut st, 2), ["done "]);
    let mut dropped = HashSet::new();
    assert!(update(&running["params"]["update"], &mut dropped).is_some() && dropped.is_empty());
}

/// The L3 notes' 1: a tripping tool's name is the agent's string, as long as it likes — where the trip is made it's
/// cut to `NAME_SHOWN` characters and made plain (a newline, an escape, a bidi mark: `?`), in its words (the turn's
/// end, so the chat's transcript, toast and status line, which say `Ended::Tripped`'s) and its end.
#[test]
fn a_tripping_tools_name_is_cut_and_made_plain() {
    let (mut st, agent, _) = staged(agy_session("s-1"));
    prompted(&mut st, &agent);
    let long = format!("run\u{1b}[2J\ncommand{}\n\u{202e}", "x".repeat(4096));
    assert!(long.len() > 4096);
    let ran = update_line(json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "name": long,
        "title": "Run", "kind": "execute", "status": "completed", "rawInput": {}}));
    let events = st.line(&ran);
    let shown = format!("run?[2J?command{}…", "x".repeat(24));
    assert_eq!(shown.chars().count(), NAME_SHOWN);
    let mut words = Vec::new();
    for ev in events {
        match ev {
            Event::Done(Err(w)) => words.push(w),
            Event::Exit(Exit { said, kind: Ended::Tripped { tool } }) => {
                assert_eq!(tool, shown, "the end's tool");
                words.push(said);
            }
            _ => {}
        }
    }
    assert_eq!(words.len(), 2, "the turn's end and the exit: {words:?}");
    for w in &words {
        assert!(w.contains(&format!("a tool of its own, {shown} — stopped")), "{w}");
        assert!(w.chars().all(|c| !c.is_control() && c != '\u{202e}'), "plain: {w:?}");
        assert!(w.len() < 300, "cut: {} bytes", w.len());
    }
    assert_eq!(super::shown("run_command"), "run_command", "a plain short name as it is");
    assert_eq!(super::shown("é\t"), "??");
}
