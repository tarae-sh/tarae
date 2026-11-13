pub mod claude;

/// A running agent: what tarae says to it. Its kind's lines are its wire's (`claude.rs`); everything above the
/// wire asks this handle, and none of it names the kind.
pub enum Agent {
    Claude(claude::Lines),
}

impl From<claude::Lines> for Agent {
    fn from(lines: claude::Lines) -> Agent {
        Agent::Claude(lines)
    }
}

impl Agent {
    /// tarae's MCP server offered: the first thing it's told (its messages come back as `StreamEv::Mcp`).
    pub fn initialize(&self) -> std::io::Result<()> {
        let Agent::Claude(l) = self;
        l.initialize()
    }

    /// A message to it. Err: it doesn't read anymore.
    pub fn prompt(&self, p: Prompt) -> std::io::Result<()> {
        let Agent::Claude(l) = self;
        l.prompt(p)
    }

    /// Its turn stopped — the process and its conversation stay. Whether it was told.
    pub fn stop_turn(&self) -> bool {
        let Agent::Claude(l) = self;
        l.stop_turn()
    }

    /// tarae's MCP server's answer (`response` its JSON-RPC message) to ask `id`.
    pub fn answer_mcp(&self, id: &str, response: serde_json::Value) {
        let Agent::Claude(l) = self;
        l.answer_mcp(id, response);
    }

    /// The answer to its permission ask `id`.
    pub fn answer_permission(&self, id: &str, p: Permission) {
        let Agent::Claude(l) = self;
        l.answer_permission(id, p);
    }

    /// The conversation goes on on `model` (an alias or id, `default` its own). Whether it was told.
    pub fn set_model(&self, model: &str) -> bool {
        let Agent::Claude(l) = self;
        l.set_model(model)
    }

    /// The conversation goes on at `effort` (None: the model's own). Whether it was told.
    pub fn set_effort(&self, effort: Option<&str>) -> bool {
        let Agent::Claude(l) = self;
        l.set_effort(effort)
    }

    /// The account's limits asked (`limits.rs`; the answer comes as `StreamEv::Limits`). Whether it was asked.
    pub fn ask_limits(&self) -> bool {
        let Agent::Claude(l) = self;
        l.ask_limits()
    }

    /// What it can do.
    pub fn caps(&self) -> &'static Caps {
        let Agent::Claude(_) = self;
        &claude::CAPS
    }

    /// Tests: its stdin as it is, for a line of the test's own.
    #[cfg(test)]
    pub fn lines(&self) -> &claude::Lines {
        let Agent::Claude(l) = self;
        l
    }
}

/// What an agent can do — read where tarae relies on it, never told apart by its kind (condition 5): what one
/// can't do, tarae doesn't ask of it.
#[derive(Debug)]
pub struct Caps {
    /// Its turn can be stopped, the process and its conversation kept (`chat::stop`).
    pub interrupt: bool,
    /// A conversation it had can be continued (`--resume`, the chat's continue row).
    pub resume: bool,
    /// A conversation can be continued under a new id, the source's left as it was (a fork's chat).
    pub fork: bool,
    /// Its thinking comes as text, a summary — not only as a sign that it thinks.
    pub thinking_text: bool,
    /// It reports usage each request (the cache, the context), not only each turn.
    pub tokens_per_request: bool,
    /// It reports what a turn costs.
    pub cost: bool,
    /// The account's limits can be asked (`limits.rs`).
    pub limits: bool,
    /// Its models are listed and chosen while it runs (`models.rs`).
    pub models: bool,
    /// Its effort is chosen while it runs.
    pub effort: bool,
    /// Its conversation can be compacted (idle compaction, `idlecompact.rs`).
    pub compaction: bool,
    /// It runs with none of its own tools: every read, edit and run is tarae's. None that keeps its own starts
    /// (`llm::held`).
    pub own_tools_off: bool,
}

/// What the agent `cfg` starts can do — read before one runs. Step 1 has one wire, Claude's.
pub fn caps(_cfg: &crate::llm::LlmConfig) -> &'static Caps {
    &claude::CAPS
}

/// The capabilities a session's usage is read with until its agent starts: the one wire's.
impl Default for &'static Caps {
    fn default() -> Self {
        &claude::CAPS
    }
}

/// A message to an agent: its text, or its parts — one kept in the cache an hour (a board's shared prefix,
/// `reviewer::content`).
pub enum Prompt {
    Text(String),
    Parts(Vec<PromptPart>),
}

pub struct PromptPart {
    pub text: String,
    /// Cached an hour, not the default five minutes.
    pub cache_hour: bool,
}

/// The answer to a permission ask: its tool runs on `input`, or it doesn't, `message` saying why. `Allow`'s
/// `input` is honoured only by a wire whose answer can carry one (Claude's); another may only allow or refuse.
#[derive(Clone, Debug, PartialEq)]
pub enum Permission {
    Allow { input: serde_json::Value },
    Deny { message: String },
}

/// A model claude offers (`initialize` → `models`, measured): its alias or id, what it's called, a line
/// about it, the model it stands for now, and the effort levels it takes (none: it has no such knob).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    pub value: String,
    pub name: String,
    pub about: String,
    pub resolved: String,
    pub efforts: Vec<String>,
}

/// What one stdout line means.
pub enum StreamEv {
    /// Thinking (thinking fragments·token estimate).
    Thinking,
    /// Summarized thinking text (chat with `--thinking-display summarized` — batched like `Text`).
    Thought(String),
    /// A thinking block began (true) or ended in the stream — its summary comes whole near its end, so the turn's word
    /// says it works meanwhile (`shimmer.rs`).
    ThinkingBlock(bool),
    /// Answer text fragment (batched every 25 ms).
    Text(String),
    /// An edit's input as it's written (`writing.rs`, batched as text is).
    Writing(crate::writing::Delta),
    /// A tool call (chat panel — Read·Grep·Glob with its full input).
    Tool { id: String, name: String, input: serde_json::Value },
    /// What a tool returned (its first 4 KB — enough to see where a search landed).
    ToolResult { id: String, text: String, error: bool },
    /// Claude Code's WebSearch returned, whole (`web::search::found`): the web search's helper reads its list here.
    Searched(Box<crate::web::search::Found>),
    /// A message to tarae's MCP server (its tools, `tools::mcp`), `id` the ask's — answered by
    /// `Agent::answer_mcp`, now or once its work is done.
    Mcp { id: String, message: serde_json::Value },
    /// The agent asks whether one of its own tools may run (`tool`, on `input`) — answered by
    /// `Agent::answer_permission`, now or after its card.
    Permission { id: String, tool: String, input: serde_json::Value },
    /// The process's conversation (claude's `session_id`), the model it runs, Claude Code's version and the
    /// credential it reported it runs on (`apiKeySource` — `limits.rs`) (`system init`, None: not sent) — the
    /// chat resumes it later (`--resume`).
    Conversation { id: String, model: String, version: Option<String>, key_source: Option<String> },
    /// The model that wrote an answer (an assistant message's `message.model`, or a stream's
    /// `message_start`) — sent when it changes.
    Answered(String),
    /// It's ready — its answer to our `initialize`: the models it offers (the chat's model picker), and the
    /// credential it runs on (`account.apiKeySource` — `limits.rs`; None: not said), known as it starts rather
    /// than with a turn. One event, so no frame falls between the models and the billing's check.
    Ready { models: Vec<ModelInfo>, key_source: Option<String>, account: Option<crate::usage::Account> },
    /// A request goes out (`system` `status: "requesting"`), when it was read (unix ms) — `usage.rs`.
    Requesting(u64),
    /// A request's usage, as a line reported it (`message_start`, `message_delta`, an assistant message).
    Usage(crate::usage::Seen),
    /// The turn's result as far as usage goes — sent before its `Done`.
    Spent(crate::usage::Spent),
    /// claude compacting the conversation: begun, over, the compaction itself (`compaction.rs`).
    Compaction(crate::compaction::Ev),
    /// Where the plan's limits stand — a `rate_limit_event`, or `get_usage`'s answer (`limits.rs`).
    Limits(crate::limits::Report),
    /// A request failed and claude retries it (`system/api_retry`).
    Retry(crate::limits::Retry),
    /// A request refused for money — the key's credits or spend limit (`billing::refusal`); before its `Done`.
    Money(crate::billing::Money),
    /// End of a turn — final text or error (not logged in·stopped etc.).
    Done(Result<String, String>),
    /// The process exited (for the chat panel — last stderr line).
    Exit(String),
}

#[cfg(test)]
mod tests {
    /// A file of the wire's: what alone speaks Claude's protocol.
    fn in_the_wire(key: &str) -> bool {
        key.starts_with("agent/claude")
    }

    /// How often `needle` is in `code` as a name of its own: not the end of a longer one, nor a definition.
    fn named(code: &str, needle: &str) -> usize {
        let ident = |c: char| c.is_alphanumeric() || c == '_';
        let mut n = 0;
        for (at, _) in code.match_indices(needle) {
            let before = &code[..at];
            if !before.ends_with(ident) && !before.ends_with("fn ") {
                n += 1;
            }
        }
        n
    }

    /// Claude's control words outside the wire, as `file: word`.
    fn speaking(sources: &[(String, String)]) -> Vec<String> {
        const WORDS: [&str; 6] = [
            "\"control_request\"",
            "\"control_response\"",
            "\"can_use_tool\"",
            "\"mcp_message\"",
            "\"mcp_response\"",
            "\"sdkMcpServers\"",
        ];
        let outside = sources.iter().filter(|(key, _)| !in_the_wire(key));
        outside
            .flat_map(|(key, code)| {
                WORDS.iter().filter(|w| code.contains(**w)).map(move |w| format!("{key}: {w}"))
            })
            .collect()
    }

    /// Agents beside Claude, brief point 2: Claude's control protocol — its requests and answers, a permission
    /// ask, tarae's MCP server's envelope, the server offered at start — is read and written by its wire alone
    /// (`agent/claude.rs`); everything above it speaks `StreamEv` and the `Agent` handle, so another agent's wire
    /// drops in beside it. Test code (a stand-in claude speaks it) is left out.
    #[test]
    fn only_the_claude_wire_speaks_claudes_control() {
        let outside = speaking(&crate::sim::lint::sources());
        assert!(
            outside.is_empty(),
            "Claude's control spoken outside its wire (`agent/claude.rs`): {outside:?}"
        );
        // (and it sees one: a chat that built a control line of its own)
        let built = [("chat.rs".to_string(), r#"json!({"type": "control_request"})"#.to_string())];
        assert_eq!(speaking(&built), [r#"chat.rs: "control_request""#]);
    }

    /// The readers of Claude's stream lines that stay outside the wire's file on purpose (step 1 moves none):
    /// each its file, the call its own file would make (None: a method, reached only through a value of its type),
    /// and what another file writes to reach it. Their one caller is the wire. This list only shrinks: one moved
    /// into the wire leaves it, none is added.
    const STREAM_PARSERS: &[(&str, Option<&str>, &str)] = &[
        ("usage.rs", Some("Counts::of("), "Counts::of("),
        ("usage.rs", Some("Ttl::of("), "Ttl::of("),
        ("usage.rs", Some("Account::of("), "Account::of("),
        ("limits.rs", Some("of_event("), "limits::of_event("),
        ("limits.rs", Some("of_retry("), "limits::of_retry("),
        ("limits.rs", Some("of_answer("), "limits::of_answer("),
        ("compaction.rs", Some("Ev::of("), "compaction::Ev::of("),
        ("billing.rs", Some("refusal("), "billing::refusal("),
        ("web/search.rs", Some("found("), "search::found("),
        ("writing.rs", None, "writing::Reading"),
    ];

    /// Callers of `STREAM_PARSERS` outside the wire, as `file: call`.
    fn parsers_called(sources: &[(String, String)]) -> Vec<String> {
        let mut out = Vec::new();
        for (key, code) in sources.iter().filter(|(key, _)| !in_the_wire(key)) {
            for (file, own, other) in STREAM_PARSERS {
                let call = if key == file { *own } else { Some(*other) };
                if let Some(call) = call.filter(|c| named(code, c) > 0) {
                    out.push(format!("{key}: {call}"));
                }
            }
        }
        out
    }

    /// Agents beside Claude, condition 4: the readers of Claude's stream lines left in their modules are called by
    /// the wire alone — nothing new leans on Claude's shapes outside it, and the list never grows.
    #[test]
    fn the_readers_of_claudes_lines_outside_the_wire_have_it_as_their_one_caller() {
        assert!(STREAM_PARSERS.len() <= 10, "this list only shrinks");
        let sources = crate::sim::lint::sources();
        for (file, ..) in STREAM_PARSERS {
            assert!(sources.iter().any(|(key, _)| key == file), "{file} is gone: take its entries off");
        }
        let called = parsers_called(&sources);
        assert!(called.is_empty(), "a reader of Claude's lines called outside its wire: {called:?}");
        // (and it sees one: a panel reading a rate limit event itself, a parser's own file calling it)
        let leaning = [
            ("usage.rs".to_string(), "pub fn of(u: &Value) -> Counts { Counts::of(u) }".to_string()),
            ("term.rs".to_string(), "let r = crate::limits::of_event(&v, 0);".to_string()),
            ("limits.rs".to_string(), "pub fn of_event(ev: &Value, at_ms: u64) {}".to_string()),
        ];
        assert_eq!(parsers_called(&leaning), ["usage.rs: Counts::of(", "term.rs: limits::of_event("]);
    }

    /// `Agent::` variants (or the wire's own `CAPS`, an agent told apart by its capabilities) named outside the
    /// wire, as `file`.
    fn matching(sources: &[(String, String)]) -> Vec<String> {
        let outside = sources.iter().filter(|(key, _)| key != "agent.rs" && !in_the_wire(key));
        let variant = |code: &str| {
            code.match_indices("Agent::").any(|(at, w)| {
                !code[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_')
                    && code[at + w.len()..].starts_with(|c: char| c.is_ascii_uppercase())
            })
        };
        outside
            .filter(|(_, code)| variant(code) || code.contains("claude::CAPS"))
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Agents beside Claude, condition 5: nothing above the wire matches on which agent it has — what one can do
    /// is read from it (its capabilities, step 1's landing 3), so another wire needs no change up there.
    #[test]
    fn no_match_on_the_agent_outside_the_wire() {
        let named = matching(&crate::sim::lint::sources());
        assert!(named.is_empty(), "an agent's kind named outside its wire: {named:?}");
        // (and it sees one; a handle made from a claude's lines is no match)
        let code = |c: &str| [("chat.rs".to_string(), c.to_string())];
        assert_eq!(matching(&code("let Agent::Claude(l) = a;")), ["chat.rs"]);
        assert!(matching(&code("let a = Agent::from(lines);")).is_empty());
        assert_eq!(matching(&code("std::ptr::eq(a.caps(), &crate::agent::claude::CAPS)")), ["chat.rs"]);
    }
}
