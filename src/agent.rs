/// The Agent Client Protocol's wire (step 2): test-only until step 4 makes it reachable (the design session's §9.4).
#[cfg(test)]
pub mod acp;
pub mod claude;
/// `tarae agent-hook`: guard 1's hook, under the gate with the ACP wire.
#[cfg(test)]
pub mod hook;
mod process;

#[cfg(test)]
mod argv_tests;

/// A running agent: what tarae says to it. Its kind's lines are its wire's (`claude.rs`); everything above the
/// wire asks this handle, and none of it names the kind.
pub enum Agent {
    Claude(claude::Lines),
    #[cfg(test)]
    Acp(acp::Proc),
}

impl Agent {
    /// Tests: an agent with no process — what it's told goes to `tx` (a stand-in's lines).
    #[cfg(test)]
    pub fn stand_in(tx: std::sync::mpsc::Sender<String>) -> Agent {
        Agent::Claude(claude::Lines::to(tx))
    }

    /// tarae's MCP server offered: the first thing it's told (its messages come back as `Event::Mcp`).
    /// (ACP: the bridge is offered in `session/new` — nothing to say here.)
    pub fn initialize(&self) -> std::io::Result<()> {
        match self {
            Agent::Claude(l) => l.initialize(),
            #[cfg(test)]
            Agent::Acp(_) => Ok(()),
        }
    }

    /// A message to it. Err: it doesn't read anymore.
    pub fn prompt(&self, p: Prompt) -> std::io::Result<()> {
        match self {
            Agent::Claude(l) => l.prompt(p),
            #[cfg(test)]
            Agent::Acp(a) => a.prompt(p),
        }
    }

    /// Its turn stopped — the process and its conversation stay. Whether it was told.
    pub fn stop_turn(&self) -> bool {
        match self {
            Agent::Claude(l) => l.stop_turn(),
            #[cfg(test)]
            Agent::Acp(a) => a.stop_turn(),
        }
    }

    /// tarae's MCP server's answer (`response` its JSON-RPC message) to ask `id` (ACP: back through the bridge).
    pub fn answer_mcp(&self, id: &str, response: serde_json::Value) {
        match self {
            Agent::Claude(l) => l.answer_mcp(id, response),
            #[cfg(test)]
            Agent::Acp(a) => a.answer_mcp(id, response),
        }
    }

    /// The answer to its permission ask `id`.
    pub fn answer_permission(&self, id: &str, p: Permission) {
        match self {
            Agent::Claude(l) => l.answer_permission(id, p),
            #[cfg(test)]
            Agent::Acp(a) => a.answer_permission(id, p),
        }
    }

    /// The conversation goes on on `model` (an alias or id, `default` its own). Whether it was told — never where
    /// its capabilities say no (`Caps::models`).
    pub fn set_model(&self, model: &str) -> bool {
        match self {
            Agent::Claude(l) => l.set_model(model),
            #[cfg(test)]
            Agent::Acp(_) => false,
        }
    }

    /// The conversation goes on at `effort` (None: the model's own). Whether it was told (`Caps::effort`).
    pub fn set_effort(&self, effort: Option<&str>) -> bool {
        match self {
            Agent::Claude(l) => l.set_effort(effort),
            #[cfg(test)]
            Agent::Acp(_) => false,
        }
    }

    /// The account's limits asked (`limits.rs`; the answer comes as `Event::Limits`). Whether it was asked
    /// (`Caps::limits`).
    pub fn ask_limits(&self) -> bool {
        match self {
            Agent::Claude(l) => l.ask_limits(),
            #[cfg(test)]
            Agent::Acp(_) => false,
        }
    }

    /// What it can do.
    pub fn caps(&self) -> &'static Caps {
        match self {
            Agent::Claude(_) => &CLAUDE.caps,
            #[cfg(test)]
            Agent::Acp(_) => &acp::CAPS,
        }
    }

    /// Tests: its stdin as it is, for a line of the test's own (a stand-in's: Claude's lines).
    #[cfg(test)]
    pub fn lines(&self) -> &claude::Lines {
        match self {
            Agent::Claude(l) => l,
            Agent::Acp(_) => panic!("an ACP agent's stdin is its wire's"),
        }
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
    /// tarae chooses who pays for it (`billing.rs`: a plan's login or an API key) and checks the credential it says it
    /// runs on. One that can't: no credential is asked for it, nor its word checked.
    pub billing: bool,
    /// Its permission asks name its tools in words tarae answers by (Claude Code's: a `Read` goes through `reads.rs`,
    /// `edits::request`). One whose don't: the chat refuses every ask it makes before anything reads its name —
    /// the user's rule for own tools, early and fail-closed (step 2's condition 1: an ACP ask's name is the agent's
    /// own, its title the model's text).
    pub asks: bool,
    /// The helpers tarae starts for its chat's tools are of its own models (`web_search`'s search runs on Claude's
    /// Haiku). One whose aren't: its chat doesn't list such a tool — a session's every model is its agent's (the
    /// user's rule; step 2's condition 3, `tools::chat_gets`).
    pub helpers: bool,
}

/// An agent tarae can start: data, as a language pack is a row ("agents are rows of data", 2026-11-13).
#[derive(Debug)]
pub struct Row {
    /// What a kept conversation records (`chats::Saved.agent`): `claude`.
    pub name: &'static str,
    pub wire: Wire,
    /// Its program and arguments. Claude's are `llm.command`/`llm.args`' defaults (the user's own override them).
    pub command: &'static str,
    pub args: &'static [&'static str],
    /// What it can do, before it runs too.
    pub caps: Caps,
    /// How its own tools go off (guard 1). None: it has none of its own to turn off (Claude: `--tools ""`), or it
    /// can't — and then an ACP row's `caps.own_tools_off` is false (`every_acp_row_keeps_the_floor`), and `llm::held`
    /// refuses to start it.
    #[cfg(test)]
    pub own_tools: Option<Recipe>,
}

/// A config folder of tarae's own (`$state/agent-homes/<row>/`, `agentgate.rs`), and the files tarae writes there
/// before every start: the agent's own tools turned off by tarae's hook (guard 1).
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub struct Recipe {
    /// The argument that names the folder, its absolute path after it, after the row's `args`: `--gemini-dir`.
    pub folder_arg: &'static str,
    /// The hook file, from the folder: `config/hooks.json`. The program it names is beside it (`hook::SCRIPT_NAME`).
    pub hook_file: &'static str,
    /// What the hook file says, and how the hook's input and answer are shaped.
    pub hook: Hook,
}

/// A hook's file and speech, by the agent family that reads it.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub enum Hook {
    /// agy 1.5.4 and 1.5.7, as the design session's probe saw them take it: one `PreToolUse` group, `matcher` `*`,
    /// one hook of `type` `command` — a bare absolute path, no arguments — and its `timeout` (its unit unmeasured:
    /// step 4's probe). Its stdin is a JSON object with `toolCall.name`; its answer `{"decision":"allow"}` or
    /// `{"decision":"deny","reason":…}`.
    Agy { timeout: u32 },
}

/// agy's recipe (the design session's probe, Q1): `--gemini-dir <folder>`, its `config/hooks.json`. The stand-in
/// plays agy's part, so it takes it too.
#[cfg(test)]
pub const AGY_TOOLS_OFF: Recipe =
    Recipe { folder_arg: "--gemini-dir", hook_file: "config/hooks.json", hook: Hook::Agy { timeout: 10 } };

/// The protocol an agent speaks, its wire's: each its own module below `agent`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wire {
    /// Claude Code's stream-json (`agent/claude.rs`).
    Claude,
    /// The Agent Client Protocol (`agent/acp.rs`): test-only in step 2, no config reaches it.
    #[cfg(test)]
    Acp,
}

/// Claude Code's row.
pub static CLAUDE: Row = Row {
    name: "claude",
    wire: Wire::Claude,
    command: "claude",
    args: claude::DEFAULT_ARGS,
    caps: claude::CAPS,
    #[cfg(test)]
    own_tools: None,
};

/// The stand-in ACP agent's row (`acp::standin`): the test binary re-run on its one test, speaking ACP on
/// descriptor 3. Tests' only: no config reaches it, nor is a kept conversation's name looked up in it.
#[cfg(test)]
pub static STAND_IN: Row = Row {
    name: "stand-in",
    wire: Wire::Acp,
    command: "sh",
    args: acp::standin::ARGS,
    caps: acp::CAPS,
    own_tools: Some(AGY_TOOLS_OFF),
};

/// Every row tarae has: what a kept conversation's name is looked up in (`row_named`).
static ROWS: &[&Row] = &[&CLAUDE];

/// The row a start from `cfg` runs: Claude's, whatever `cfg` says — no setting names another (step 2).
pub fn row_of(_cfg: &crate::llm::LlmConfig) -> &'static Row {
    &CLAUDE
}

/// The row named `name` (a kept conversation's), or None: no such row here.
pub fn row_named(name: &str) -> Option<&'static Row> {
    ROWS.iter().copied().find(|r| r.name == name)
}

/// Which start it is — its wire turns it, with the rest of its `Start`, into its own arguments.
#[derive(Clone, Copy, Debug)]
pub enum Role {
    Chat,
    /// `space i`'s.
    Ask,
    /// A commit draft's.
    Draft,
    Reviewer(crate::reviewer::Role),
    /// The web search's helper.
    WebSearch,
    /// Auto cards' reviewer, its spend's backstop.
    CardReview {
        max_usd: f64,
    },
}

/// What a start asks for; its wire says it in its own arguments (`args`).
pub struct Start<'a> {
    /// The agent started.
    pub row: &'static Row,
    pub role: Role,
    /// Its model (`--model`; None: its own).
    pub model: Option<&'a str>,
    /// Its effort ("": the model's own).
    pub effort: &'a str,
    /// The wikis whose tokens no agent's environment keeps (`web.signed-in`).
    pub signed_in: &'a [crate::web::signed::Wiki],
    /// tarae's words before any message: beside the agent's own (the chat's, the ask's, the reviewer's), or in
    /// their place (the web search's, a card review's).
    pub system: Option<&'a str>,
    /// A conversation continued (the chat's).
    pub resume: Option<Resume<'a>>,
    /// Its thinking said as text (the chat's, where its agent can — `Caps::thinking_text`).
    pub thinking_text: bool,
    /// How it differs from what's asked, as its wire said when the last start ended (`again`).
    pub again: Option<Again>,
    /// What only Claude's wire reads.
    pub claude: ClaudeStart<'a>,
    /// Its folder made and its hook written (`agentgate::prepare`), where its row turns its own tools off: none, and
    /// such a row isn't started (`acp::command`).
    #[cfg(test)]
    pub prepared: Option<&'a crate::agentgate::Prepared>,
}

impl<'a> Start<'a> {
    /// `role`'s start on `cfg`, on its row (`row_of`): no words of tarae's, no conversation continued, no thinking
    /// text, no settings.
    pub fn new(role: Role, cfg: &'a crate::llm::LlmConfig) -> Start<'a> {
        Start::on(row_of(cfg), role, cfg)
    }

    /// `new` on `row` (a chat's: its conversation's).
    pub fn on(row: &'static Row, role: Role, cfg: &'a crate::llm::LlmConfig) -> Start<'a> {
        Start {
            row,
            role,
            model: cfg.model.as_deref(),
            effort: &cfg.effort,
            signed_in: &cfg.signed_in,
            system: None,
            resume: None,
            thinking_text: false,
            again: None,
            claude: ClaudeStart {
                command: &cfg.command,
                args: &cfg.args,
                keep_env_key: cfg.keep_env_key,
                settings: None,
            },
            #[cfg(test)]
            prepared: None,
        }
    }
}

/// How a start is tried again, as its wire asked (`again`): kept by whoever starts it and given back in its next
/// `Start`, what it means its wire's alone.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Again {
    /// Its thinking not asked for as text (an older claude refuses `--thinking-display`).
    thinking_plain: bool,
}

/// Whether a process that ended saying `err` is to be started again, and how — `now` how the last start already
/// differed. None: nothing to try again (its end is said as any end is).
pub fn again(row: &Row, err: &str, now: Option<Again>) -> Option<Again> {
    match row.wire {
        Wire::Claude => claude::again(err, now.unwrap_or_default()),
        #[cfg(test)]
        Wire::Acp => None,
    }
}

/// A conversation continued: its id, and whether as a fork (copied under a new id, the source's left as it was).
pub struct Resume<'a> {
    pub conversation: &'a str,
    pub fork: bool,
}

/// Claude's own part of a start.
pub struct ClaudeStart<'a> {
    /// Its program (`llm.command`).
    pub command: &'a str,
    /// What it's given in place of its row's arguments (`llm.args`).
    pub args: &'a [String],
    /// An API key in tarae's environment goes on to it (`llm.environment-key`, `llm::keeps_key`).
    pub keep_env_key: bool,
    /// The chat's `--settings` and the sources it loads (`claudetrust::ready`).
    pub settings: Option<(&'a str, &'a str)>,
}

/// A start's arguments as its wire gives them: those in place of `llm.args`, and those after (`Chat.launched` keeps
/// both).
pub struct Args {
    pub args: Vec<String>,
    pub extra: Vec<String>,
}

/// `s`'s arguments, its wire's. Err: `llm.args` can't take what it must add (a board's `--settings` given as a file).
pub fn args(s: &Start) -> Result<Args, String> {
    match s.row.wire {
        Wire::Claude => claude::flags::of(s),
        #[cfg(test)]
        Wire::Acp => Ok(acp::args(s)),
    }
}

/// `s`'s process, not started: its program, arguments and environment — run in `dir`, `given` a test's stand-in for
/// tarae's environment, paid for by `cred` (given where its row says `Caps::billing`).
pub fn command(
    s: &Start,
    dir: &std::path::Path,
    given: Option<&crate::llm::Env>,
    cred: Option<&crate::billing::Credential>,
) -> Result<std::process::Command, SpawnError> {
    match s.row.wire {
        Wire::Claude => claude::flags::command(s, dir, given, cred),
        #[cfg(test)]
        Wire::Acp => acp::command(s, dir, given),
    }
}

/// Claude's `command`, started in a session of its own (`spawn::setsid_fed`, as every child of tarae's — the user's
/// decision, 2026-11-14): its own process group, which its end ends with it (`process::reap_group`, `kill`), and no
/// controlling terminal — it and what it starts don't have tarae's terminal as `/dev/tty`, nor are in its session or
/// foreground group. Its stdin a pipe (the writing end returned), stdout and stderr pipes. It keeps tarae's raised
/// descriptor limit, the one measured for claude (`nofile`). No filter on it (`Acts::User`'s way): it runs as you.
fn spawn(
    s: &Start,
    dir: &std::path::Path,
    given: Option<&crate::llm::Env>,
    cred: Option<&crate::billing::Credential>,
) -> Result<(crate::spawn::Spawned, std::io::PipeWriter), SpawnError> {
    let cmd = command(s, dir, given, cred)?;
    crate::spawn::setsid_fed(&cmd).map_err(SpawnError::Start)
}

/// Where an agent runs and on whose account: its folder (the project's root, a helper's own), `given` a test's
/// stand-in for tarae's environment, `cred` who pays (asked only where its row says `Caps::billing`).
pub struct Place<'a> {
    pub dir: &'a std::path::Path,
    pub given: Option<&'a crate::llm::Env>,
    pub cred: Option<&'a crate::billing::Credential>,
}

/// What an agent is told first: tarae's tools offered (its MCP server — their messages come back as `Event::Mcp`),
/// then a prompt.
pub struct First {
    pub tools: bool,
    pub prompt: Option<Prompt>,
}

/// An agent started: its handle (dropped, its input closes) and its process's pid (`kill`).
pub struct Started {
    pub agent: Agent,
    pub pid: u32,
}

/// `s` started at `at`: its process spawned, its first lines written (`first`, off the main loop) and its stream
/// read on a thread of its own, each event handed to `route` and what it makes applied on the main loop. `once`:
/// one turn, the process ended and reaped after it. Err: it didn't start, or its first lines couldn't be written —
/// its child then ended and reaped off the main loop; `SpawnError::said` says it.
pub fn start(
    s: &Start,
    at: Place,
    first: First,
    once: bool,
    tx: crate::event::Tx,
    route: impl Fn(Event) -> Option<crate::event::Apply> + Send + 'static,
) -> Result<Started, SpawnError> {
    match s.row.wire {
        Wire::Claude => {
            let started = spawn(s, at.dir, at.given, at.cred)?;
            claude::begin(started, first, once, s.resume.is_some(), tx, route)
        }
        #[cfg(test)]
        Wire::Acp => acp::begin(acp::spawn(s, at.dir, at.given)?, s, &at, first, once, tx, route),
    }
}

/// `tarae agent-tools <socket>`: the bridge's relay an ACP agent starts as tarae's MCP server (`acp::bridge`) —
/// under the gate with the rest of the wire until step 4.
#[cfg(test)]
pub fn tools_relay(args: &[String]) -> anyhow::Result<()> {
    acp::bridge::cli(args)
}

/// An agent's process and its group ended by its pid, while its reader still holds it (else nothing is sent: the
/// pid may be anyone's by then).
pub use process::kill;

/// Why an agent didn't start.
#[derive(Debug)]
pub enum SpawnError {
    /// Its program didn't start (`spawn_failed` says it).
    Start(std::io::Error),
    /// Its billing can't be laid into its arguments (Claude's: `flags::billing::args`).
    Billing(String),
    /// Its arguments can't be made (`agent::args`).
    Args(String),
    /// Its first lines couldn't be written: it has no input, or stopped reading it.
    Write(std::io::Error),
}

impl SpawnError {
    /// What the chat (`whole`: every line) or a toast says of a start of `command` (its program).
    pub fn said(&self, command: &str, whole: bool) -> String {
        match self {
            SpawnError::Start(e) => crate::llm::spawn_failed(command, e, whole),
            SpawnError::Billing(why) | SpawnError::Args(why) => why.clone(),
            SpawnError::Write(e) => format!("{command}: {e}"),
        }
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
pub enum Event {
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
    /// The process exited: what it said and how it ended, its wire's reading.
    Exit(Exit),
}

/// An agent's process's end (`Event::Exit`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exit {
    /// What its end says in the chat (stderr's last line, or its wire's words); empty: nothing.
    pub said: String,
    pub kind: Ended,
}

/// How an agent's process ended, as its wire tells it — never read from its words above the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// It exited, or was ended.
    Exited,
    /// The conversation it was started to continue is gone from the agent (Claude's own cleanup took it, it was
    /// deleted, the agent can't continue one): the chat starts a new one and sends again what it cut off.
    Gone,
}

#[cfg(test)]
impl Exit {
    /// An end that says `said` (ACP's wire and tests: Claude's reads its kind from its words, `claude::exit`).
    pub fn exited(said: impl Into<String>) -> Exit {
        Exit { said: said.into(), kind: Ended::Exited }
    }
}

#[cfg(test)]
mod tests {
    /// A file of the wire's: what alone speaks Claude's protocol.
    fn in_the_wire(key: &str) -> bool {
        key.starts_with("agent/claude")
    }

    /// A file of any wire's (Claude's, ACP's): what may tell an agent's kind.
    fn in_a_wire(key: &str) -> bool {
        in_the_wire(key) || key.starts_with("agent/acp")
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
    /// (`agent/claude.rs`); everything above it speaks `Event` and the `Agent` handle, so another agent's wire
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
        let outside = sources.iter().filter(|(key, _)| key != "agent.rs" && !in_a_wire(key));
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

    /// Paths into a wire outside the agent's own files (`agent.rs`, `agent/`), as `file`: `agent::claude` or
    /// `agent::acp` itself, or either taken from an `agent::{…}`.
    fn reaching_in(sources: &[(String, String)]) -> Vec<String> {
        let ident = |c: char| c.is_alphanumeric() || c == '_';
        let reaches = |code: &str| {
            let path = ["agent::claude", "agent::acp"]
                .iter()
                .any(|wire| code.match_indices(wire).any(|(at, w)| !code[at + w.len()..].starts_with(ident)));
            let braced = code.match_indices("agent::{").any(|(at, w)| {
                let inside = &code[at + w.len()..];
                let inside = &inside[..inside.find('}').unwrap_or(inside.len())];
                inside.split(|c: char| !ident(c)).any(|word| word == "claude" || word == "acp")
            });
            path || braced
        };
        let outside = sources.iter().filter(|(key, _)| key != "agent.rs" && !key.starts_with("agent/"));
        outside.filter(|(_, code)| reaches(code)).map(|(key, _)| key.clone()).collect()
    }

    /// What `agent.rs` takes from the one wire for the code above it: nothing (step 2, L1: every name but one went
    /// behind `agent::start`, or is `agent`'s own; L2: `llm.args`' default is the Claude row's arguments).
    const REEXPORTED: &[&str] = &[];

    /// The names `code` takes from `claude` by a `use` (one, or a `{…}` of them).
    fn reexported(code: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (at, w) in code.match_indices("use claude::") {
            let rest = &code[at + w.len()..];
            let items = match rest.strip_prefix('{') {
                Some(inside) => &inside[..inside.find('}').unwrap_or(inside.len())],
                None => &rest[..rest.find(';').unwrap_or(rest.len())],
            };
            out.extend(items.split(',').map(str::trim).filter(|n| !n.is_empty()).map(String::from));
        }
        out
    }

    /// Agents beside Claude, landing 5: the code above the wire reaches it through `agent` alone (its handle,
    /// `Start` and the one wire's few items `agent` names), never by `agent::claude::` — so step 2's second wire
    /// is a change in `agent` and below. Test code is left out on purpose: tests keep their doors into the wire
    /// (`claude::test::{control, answer}`, the flags' moved constants, `REQUEST_ID`, the line builders), as a
    /// stand-in claude speaks its protocol.
    #[test]
    fn nothing_above_the_wire_names_claudes() {
        let sources = crate::sim::lint::sources();
        let reaching = reaching_in(&sources);
        assert!(reaching.is_empty(), "Claude's wire named above it: {reaching:?}");
        // (nor through `agent`: what it names of the one wire is a list that only shrinks — step 2, L1)
        let agent = sources.iter().find(|(key, _)| key == "agent.rs").map(|(_, code)| code.as_str());
        let named = reexported(agent.expect("agent.rs read"));
        assert!(named.iter().all(|n| REEXPORTED.contains(&n.as_str())), "named through `agent`: {named:?}");
        assert!(REEXPORTED.is_empty(), "this list only shrinks");
        // (and it sees one: a reader put back above the wire)
        assert_eq!(reexported("pub(crate) use claude::pump;"), ["pump"]);
        assert_eq!(reexported("pub use claude::{DEFAULT_ARGS, Lines};"), ["DEFAULT_ARGS", "Lines"]);
        // (and it sees one: a path into the wire, a `claude` taken from `agent`; the agent's own files may)
        let code = |key: &str, c: &str| (key.to_string(), c.to_string());
        let made_up = [
            code("chat.rs", "crate::agent::claude::pump(child, None, true, tx, route);"),
            code("ask.rs", "use crate::agent::{Agent, claude};"),
            code("llm.rs", "use crate::agent::claude as wire;"),
            code("usage.rs", "crate::agent::pump(child, None, true, tx, route);"),
            code("agent/acp.rs", "use crate::agent::claude::Lines;"),
            // (step 2, L3: ACP's wire alike — never named above it, not even once the gate is lifted)
            code("term.rs", "crate::agent::acp::begin(child, s, dir, first, false, tx, route);"),
            code("reviewer.rs", "use crate::agent::{acp, Agent};"),
            code("models.rs", "let caps = &crate::agent::acpx::CAPS;"),
        ];
        assert_eq!(reaching_in(&made_up), ["chat.rs", "ask.rs", "llm.rs", "term.rs", "reviewer.rs"]);
    }

    /// Agents beside Claude, step 2 (L2): no config chooses an agent — no setting names one or a wire, and the row
    /// of any config is Claude's, whatever its program and arguments say (completed by L3 once the second wire is
    /// there: a config can't start it).
    #[test]
    fn no_config_chooses_another_row() {
        use super::{CLAUDE, Role, STAND_IN, Start, Wire, row_named, row_of};
        let words = ["agent", "wire", "acp", "backend", "row"];
        let naming: Vec<&str> = (crate::settings::SETTINGS.iter().map(|s| s.path))
            .filter(|p| p.split(['.', '-']).any(|w| words.contains(&w)))
            .collect();
        assert!(naming.is_empty(), "a setting names an agent: {naming:?}");
        let default = crate::llm::LlmConfig::default();
        let other = |command: &str, args: &[&str]| crate::llm::LlmConfig {
            command: command.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            ..crate::llm::LlmConfig::default()
        };
        // (L3: a config can't start the ACP wire — not one naming an ACP agent, nor the stand-in's own program and
        // arguments, the one row on that wire there is to miss)
        assert_eq!(STAND_IN.wire, Wire::Acp);
        for cfg in [
            default.clone(),
            other("agy", &["--acp"]),
            other("/usr/bin/gemini", &["--experimental-acp"]),
            other(STAND_IN.command, STAND_IN.args),
        ] {
            assert!(std::ptr::eq(row_of(&cfg), &CLAUDE), "{} {:?}", cfg.command, cfg.args);
            assert_eq!(row_of(&cfg).wire, Wire::Claude);
            assert!(std::ptr::eq(Start::new(Role::Chat, &cfg).row, &CLAUDE));
        }
        // (`llm.command` and `llm.args` default to the row's own; a row is found by its kept name alone, and a kept
        // conversation never names the stand-in's)
        assert_eq!((default.command.as_str(), default.args.len()), (CLAUDE.command, CLAUDE.args.len()));
        assert!(row_named("claude").is_some_and(|r| std::ptr::eq(r, &CLAUDE)));
        assert!(row_named("acp").is_none() && row_named("").is_none());
        assert!(row_named(STAND_IN.name).is_none());
    }

    /// Agents beside Claude, condition 5: nothing above the wire matches on which agent it has — what one can do
    /// is read from it (its capabilities, step 1's landing 3), so another wire needs no change up there.
    #[test]
    fn no_match_on_the_agent_outside_the_wire() {
        let named = matching(&crate::sim::lint::sources());
        assert!(named.is_empty(), "an agent's kind named outside its wire: {named:?}");
        // (and it sees one; a stand-in's handle made is no match)
        let code = |c: &str| [("chat.rs".to_string(), c.to_string())];
        assert_eq!(matching(&code("let Agent::Claude(l) = a;")), ["chat.rs"]);
        assert_eq!(matching(&code("if let Agent::Acp(p) = a {}")), ["chat.rs"]);
        assert!(matching(&code("let a = Agent::stand_in(tx);")).is_empty());
        // (the ACP wire's own files may: they make its handle)
        let acp = [("agent/acp.rs".to_string(), "Agent::Acp(Proc(in_tx))".to_string())];
        assert!(matching(&acp).is_empty());
        assert_eq!(matching(&code("std::ptr::eq(a.caps(), &crate::agent::claude::CAPS)")), ["chat.rs"]);
    }

    /// Whether `row` keeps the floor every ACP row stands on, as what it breaks: every permission ask refused
    /// (guard 2 — the rule, not a row's choice), and its own tools said off only where a recipe turns them off
    /// (guard 1).
    fn floor(row: &super::Row) -> Vec<&'static str> {
        let mut broken = Vec::new();
        if row.wire != super::Wire::Acp {
            return broken;
        }
        if row.caps.asks {
            broken.push("its asks read");
        }
        if row.caps.own_tools_off != row.own_tools.is_some() {
            broken.push("its own tools said off with no recipe, or a recipe said not off");
        }
        broken
    }

    /// Agents beside Claude, step 3 (plan §1): every row on the ACP wire — `ROWS` and the stand-in's — keeps the
    /// floor: its asks refused, and its own tools off only by a recipe (`acp::CAPS` says off; a row copying it with
    /// no recipe would pass `llm::held` with nothing turned off).
    #[test]
    fn every_acp_row_keeps_the_floor() {
        use super::{ROWS, Row, STAND_IN, Wire};
        for row in ROWS.iter().copied().chain([&STAND_IN]) {
            assert!(floor(row).is_empty(), "{}: {:?}", row.name, floor(row));
        }
        // (and it sees each: a row on `acp::CAPS` with no recipe, and one whose asks are read)
        let row =
            |caps, own_tools| Row { name: "x", wire: Wire::Acp, command: "x", args: &[], caps, own_tools };
        let asks = super::Caps { asks: true, ..super::acp::CAPS };
        assert_eq!(floor(&row(super::acp::CAPS, None)).len(), 1);
        assert_eq!(floor(&row(asks, Some(super::AGY_TOOLS_OFF))), ["its asks read"]);
    }
}
