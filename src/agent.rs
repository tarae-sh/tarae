/// The Agent Client Protocol's wire (step 2; reachable since step 4's L2: `llm.agent`).
pub mod acp;
pub mod claude;
/// `tarae agent-hook`: guard 1's hook.
pub mod hook;
mod process;

#[cfg(test)]
mod argv_tests;

/// A running agent: what tarae says to it. Its kind's lines are its wire's (`claude.rs`); everything above the
/// wire asks this handle, and none of it names the kind.
pub enum Agent {
    Claude(claude::Lines),
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
            Agent::Acp(_) => Ok(()),
        }
    }

    /// A message to it. Err: it doesn't read anymore.
    pub fn prompt(&self, p: Prompt) -> std::io::Result<()> {
        match self {
            Agent::Claude(l) => l.prompt(p),
            Agent::Acp(a) => a.prompt(p),
        }
    }

    /// Its turn stopped — the process and its conversation stay. Whether it was told.
    pub fn stop_turn(&self) -> bool {
        match self {
            Agent::Claude(l) => l.stop_turn(),
            Agent::Acp(a) => a.stop_turn(),
        }
    }

    /// tarae's MCP server's answer (`response` its JSON-RPC message) to ask `id` (ACP: back through the bridge).
    pub fn answer_mcp(&self, id: &str, response: serde_json::Value) {
        match self {
            Agent::Claude(l) => l.answer_mcp(id, response),
            Agent::Acp(a) => a.answer_mcp(id, response),
        }
    }

    /// The answer to its permission ask `id`.
    pub fn answer_permission(&self, id: &str, p: Permission) {
        match self {
            Agent::Claude(l) => l.answer_permission(id, p),
            Agent::Acp(a) => a.answer_permission(id, p),
        }
    }

    /// Stopped by tarae with `said` (an agent's watch, `agentgate`), as the tripwire stops one: its group ended at once,
    /// its turn ended with those words, nothing it says after read, its end an `Exited` saying them. False: its wire
    /// can't (Claude's) or it's gone already — the caller ends its process.
    pub fn stop_with(&self, said: String) -> bool {
        match self {
            Agent::Claude(_) => false,
            Agent::Acp(a) => a.stop_with(said),
        }
    }

    /// The conversation goes on on `model` (an alias or id, `default` its own). Whether it was told — never where
    /// its capabilities say no (`Caps::models`).
    pub fn set_model(&self, model: &str) -> bool {
        match self {
            Agent::Claude(l) => l.set_model(model),
            Agent::Acp(_) => false,
        }
    }

    /// The conversation goes on at `effort` (None: the model's own). Whether it was told (`Caps::effort`).
    pub fn set_effort(&self, effort: Option<&str>) -> bool {
        match self {
            Agent::Claude(l) => l.set_effort(effort),
            Agent::Acp(_) => false,
        }
    }

    /// The account's limits asked (`limits.rs`; the answer comes as `Event::Limits`). Whether it was asked
    /// (`Caps::limits`).
    pub fn ask_limits(&self) -> bool {
        match self {
            Agent::Claude(l) => l.ask_limits(),
            Agent::Acp(_) => false,
        }
    }

    /// What it can do.
    pub fn caps(&self) -> &'static Caps {
        match self {
            Agent::Claude(_) => &CLAUDE.caps,
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
    /// (`llm::held_for`).
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
    /// The starts it can be, by kind: the chat's always (`every_row_chats`); a role it can't be is held where it would
    /// start, with its words (`row_for`, `llm::held_for`), and `start` refuses it.
    pub roles: &'static [RoleKind],
}

/// An agent tarae can start: data, as a language pack is a row ("agents are rows of data", 2026-11-13).
#[derive(Debug)]
pub struct Row {
    /// What a kept conversation records (`chats::Saved.agent`): `claude`. On screen it's the word for the agent's
    /// program and process (`claude exited`, `▲ no claude`, `this session stays on claude`).
    pub name: &'static str,
    /// The agent's name on screen where it's the one at work (`Claude is still answering`, `Ask Claude…`, `Claude's
    /// change`) — step 4's C.3. Claude's capitalized; agy's is its own lowercase name, the same as `name`.
    pub shown: &'static str,
    pub wire: Wire,
    /// Its program and arguments. Claude's are `llm.command`/`llm.args`' defaults (the user's own override them).
    pub command: &'static str,
    pub args: &'static [&'static str],
    /// What it can do, before it runs too.
    pub caps: Caps,
    /// How its own tools go off (guard 1). None: it has none of its own to turn off (Claude: `--tools ""`), or it
    /// can't — and then an ACP row's `caps.own_tools_off` is false (`every_acp_row_keeps_the_floor`), and `llm::held_for`
    /// refuses to start it.
    pub own_tools: Option<Recipe>,
    /// The session mode tarae keeps it in (guard 2): set at the start where its session says another, and set back if
    /// it moves (`acp::Wire`'s mode). None: it has no modes.
    pub mode: Option<&'static str>,
    /// What a project can make it load that runs as you, in every folder from the project's root up to its git top
    /// (guard 4, `agenttrust.rs`): held until you trust it on a card, the agent not started before. A path ending in `/` is a folder: every file under it.
    pub held: &'static [&'static str],
    /// The version of it tarae's guards were measured on: another one it says at `initialize` is said once a version,
    /// never refused (`version_said`). None: nothing to compare (Claude's own update says its version).
    pub measured: Option<&'static str>,
}

/// Every row's held paths (`Row.held`): what a folder's answer of `trust::Kind::Agent` is about (`trust::carried_rel`).
pub fn held_paths() -> impl Iterator<Item = &'static str> {
    every_row().flat_map(|r| r.held.iter().copied())
}

/// A config folder of tarae's own (`$state/agent-homes/<row>/<key>/`, one a project, `agentgate.rs`), and the files tarae writes there
/// before every start: the agent's own tools turned off by tarae's hook (guard 1).
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
#[derive(Debug, Clone, Copy)]
pub enum Hook {
    /// agy 1.5.4 and 1.5.7, as the design session's probe saw them take it: one `PreToolUse` group, `matcher` `*`,
    /// one hook of `type` `command` — an absolute path, shell-quoted, no arguments: 1.5.9 runs it through a shell
    /// (step 4's probe, #6) — and its `timeout` (in seconds, enforced: probe 2, run A). Its stdin is a JSON object with `toolCall.name`; its answer `{"decision":"allow"}` or
    /// `{"decision":"deny","reason":…}`.
    Agy { timeout: u32 },
}

/// agy's recipe (the design session's probe, Q1): `--gemini-dir <folder>`, its `config/hooks.json`. The stand-in
/// plays agy's part, so it takes it too. Its hook's timeout, 10 s: probe 2 (run A) measured agy enforcing it in seconds,
/// the hook killed and its call failed closed — a long one costs nothing in safety, a short one would fail tarae's own
/// tools on a loaded machine.
pub const AGY_TOOLS_OFF: Recipe =
    Recipe { folder_arg: "--gemini-dir", hook_file: "config/hooks.json", hook: Hook::Agy { timeout: 10 } };

/// The protocol an agent speaks, its wire's: each its own module below `agent`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wire {
    /// Claude Code's stream-json (`agent/claude.rs`).
    Claude,
    /// The Agent Client Protocol (`agent/acp.rs`): `llm.agent`'s `agy`.
    Acp,
}

/// Claude Code's row.
pub static CLAUDE: Row = Row {
    name: "claude",
    shown: "Claude",
    wire: Wire::Claude,
    command: "claude",
    args: claude::DEFAULT_ARGS,
    caps: claude::CAPS,
    own_tools: None,
    mode: None,
    // (its own gate is `claudetrust.rs`)
    held: &[],
    measured: None,
};

/// agy's row (Google's Antigravity CLI, `agy acp` — the structure plan §1): its own tools off by tarae's hook (guard 1),
/// kept in its `default` mode (guard 2), a project's `.agents/` held until trusted (guard 4 — `.gemini/` wasn't loaded
/// in the three shapes probe 1 tried, #17). Its program is `agy` on PATH, its arguments its own: no setting reaches
/// them (`acp::args`). Chosen by `llm.agent` for a session that has no row yet (`llm::session_row`).
pub static AGY: Row = Row {
    name: "agy",
    shown: "agy",
    wire: Wire::Acp,
    command: "agy",
    args: &["acp"],
    caps: acp::CAPS,
    own_tools: Some(AGY_TOOLS_OFF),
    mode: Some("default"),
    held: &[".agents/"],
    measured: Some("1.5.9"),
};

/// The stand-in ACP agent's row (`acp::standin`): the test binary re-run on its one test, speaking ACP on
/// descriptor 3. Tests' only: no config reaches it, nor is a kept conversation's name looked up in it.
#[cfg(test)]
pub static STAND_IN: Row = Row {
    name: "stand-in",
    shown: "stand-in",
    wire: Wire::Acp,
    command: "sh",
    args: acp::standin::ARGS,
    caps: acp::CAPS,
    own_tools: Some(AGY_TOOLS_OFF),
    mode: Some("default"),
    held: &[".stand-in/"],
    measured: None,
};

/// Every row tarae has: what a kept conversation's name is looked up in (`row_named`), and `llm.agent`'s choices.
static ROWS: &[&Row] = &[&CLAUDE, &AGY];

/// Every row, the stand-in's too in tests.
fn every_row() -> impl Iterator<Item = &'static Row> {
    #[cfg(test)]
    let rows = ROWS.iter().copied().chain([&STAND_IN]);
    #[cfg(not(test))]
    let rows = ROWS.iter().copied();
    rows
}

/// Whether `s` is a row's name on screen (`Row.shown`): what reads a row tarae wrote back (`orchestra::is_told`).
pub fn is_shown(s: &str) -> bool {
    every_row().any(|r| r.shown == s)
}

/// The name on screen (`Row.shown`) of the row named `name` (`Row.name`) — `name` itself when no row is: what a
/// process's row, titled by its program's word, says of it in words (`procs::detail`).
pub fn shown_of(name: &str) -> &str {
    every_row().find(|r| r.name == name).map_or(name, |r| r.shown)
}

/// `llm.agent`'s choices: each row's name, in `ROWS`' order (`every_row_is_a_choice`).
pub const NAMES: &[&str] = &["claude", "agy"];

/// The config's row (`llm.agent`): what a session that has no row yet starts on — read by `llm::session_row` and where
/// a session's row is picked, never by a start itself (`every_start_takes_the_sessions_row`).
pub fn row_of(cfg: &crate::llm::LlmConfig) -> &'static Row {
    cfg.agent
}

/// The version an agent of `row` said at `initialize` (`version`; None: unsaid), as said once a version where it
/// isn't the one its guards were measured on (`Row.measured`) — never refused. The version is the agent's text: cut
/// and made plain as a tool's name is (`acp::shown`). None: nothing to say.
pub fn version_said(row: &Row, version: Option<&str>) -> Option<String> {
    let measured = row.measured?;
    match version {
        Some(v) if v == measured => None,
        Some(v) => {
            Some(format!("{} {}: tarae's guards were measured on {measured}", row.name, acp::shown(v)))
        }
        None => Some(format!("{}, its version unsaid: tarae's guards were measured on {measured}", row.name)),
    }
}

/// `row` where it can be `role` (`Caps::roles` — the chat's, always), else why not, in the words said where it would
/// start (step 4, C.2: the helpers wait for the second round).
pub fn can_be(row: &'static Row, role: Role) -> Result<&'static Row, String> {
    match row.caps.roles.contains(&role.kind()) {
        true => Ok(row),
        false => Err(not_for(row, role)),
    }
}

/// Why `row` doesn't start as `role`: none of its roles. (A debt against the user's 2026-11-13 decision — an agy
/// session's other roles are to be its own agent's — carried in the plan: honest until then.)
fn not_for(row: &Row, role: Role) -> String {
    format!("not yet with {} — {} runs on Claude's sessions only", row.name, role.said())
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

/// A `Role` without its particulars: what a row's `Caps::roles` names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoleKind {
    Chat,
    Ask,
    Draft,
    Reviewer,
    WebSearch,
    CardReview,
}

impl RoleKind {
    /// Every role: what a row that can be any (Claude's) says.
    pub const ALL: &[RoleKind] = &[
        RoleKind::Chat,
        RoleKind::Ask,
        RoleKind::Draft,
        RoleKind::Reviewer,
        RoleKind::WebSearch,
        RoleKind::CardReview,
    ];
}

impl Role {
    /// Its kind, as `Caps::roles` names it.
    pub fn kind(self) -> RoleKind {
        match self {
            Role::Chat => RoleKind::Chat,
            Role::Ask => RoleKind::Ask,
            Role::Draft => RoleKind::Draft,
            Role::Reviewer(_) => RoleKind::Reviewer,
            Role::WebSearch => RoleKind::WebSearch,
            Role::CardReview { .. } => RoleKind::CardReview,
        }
    }

    /// Its name where a hold says it (`row_for`).
    fn said(self) -> &'static str {
        match self {
            Role::Chat => "the chat",
            Role::Ask => "space i",
            Role::Draft => "a commit draft",
            Role::Reviewer(_) => "the reviewer",
            Role::WebSearch => "the web search",
            Role::CardReview { .. } => "auto cards' reviewer",
        }
    }
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
    pub prepared: Option<&'a crate::agentgate::Prepared>,
}

impl<'a> Start<'a> {
    /// `role`'s start on `row` — the session's (`llm::session_row`, `every_start_takes_the_sessions_row`) — and `cfg`:
    /// no words of tarae's, no conversation continued, no thinking text, no settings. Its start asks `llm::held_for`
    /// first (`every_start_passes_through_held_for`), which holds a role the row can't be (`can_be`); `start` refuses
    /// one all the same.
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
    // (a role its row can't be never reaches its wire: the ACP one has the chat's conversation only)
    if !s.row.caps.roles.contains(&s.role.kind()) {
        return Err(SpawnError::Role(not_for(s.row, s.role)));
    }
    match s.row.wire {
        Wire::Claude => {
            let started = spawn(s, at.dir, at.given, at.cred)?;
            claude::begin(started, first, once, s.resume.is_some(), tx, route)
        }
        Wire::Acp => acp::begin(acp::spawn(s, at.dir, at.given)?, s, &at, first, once, tx, route),
    }
}

/// The tools a `tools/list` answer offers, by name: what an ACP agent's ask may be allowed for (`acp::offered`).
pub fn tools_offered(answer: &serde_json::Value) -> Vec<String> {
    acp::offered(answer)
}

/// Guard 2's ask rule: an ACP agent's ask for `tool` allowed where it's one of tarae's that its bridge `offered`
/// (`acp::allowed_ask`).
pub fn ask_allowed(tool: &str, offered: &[String]) -> bool {
    acp::allowed_ask(tool, offered)
}

/// Where an agent's own folder is made (`agentgate`) and its bridge binds: `$state` (`acp::state`).
pub fn state_of(given: Option<&crate::llm::Env>) -> Option<std::path::PathBuf> {
    acp::state(given)
}

/// `tarae agent-tools <socket>`: the bridge's relay an ACP agent starts as tarae's MCP server (`acp::bridge`).
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
    /// Its row can't be its role (`row_for`'s words): a start that skipped `llm::held_for`.
    Role(String),
}

impl SpawnError {
    /// What the chat (`whole`: every line) or a toast says of a start of `command` (its program).
    pub fn said(&self, command: &str, whole: bool) -> String {
        match self {
            SpawnError::Start(e) => crate::llm::spawn_failed(command, e, whole),
            SpawnError::Billing(why) | SpawnError::Args(why) | SpawnError::Role(why) => why.clone(),
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    /// It exited, or was ended.
    Exited,
    /// The conversation it was started to continue is gone from the agent (Claude's own cleanup took it, it was
    /// deleted, the agent can't continue one): the chat starts a new one and sends again what it cut off.
    Gone,
    /// Guard 3, the tripwire: it ran `tool` itself — one of its own reported done, or one of tarae's that tarae never
    /// ran — and was stopped, its group ended (`acp::Wire::trip`). The chat holds its row until you quit.
    Tripped { tool: String },
    /// Stopped at its start: it can't run as its row needs (guard 2: it offers no `mode` the row names). Said as an
    /// error, as a trip is; the chat's top line says it stopped until its next start.
    Unfit { mode: String },
}

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

    /// Agents beside Claude, step 4's L2: only `llm.agent` chooses a row — no other setting names an agent or a wire;
    /// the default and any `llm.command`/`llm.args` stay Claude's (the real `agy acp` among them, and the stand-in's
    /// own program); `llm.agent = "agy"` is agy's row, on its real arguments; a name that's no row's is said and stays
    /// Claude's, the stand-in's among them; a project's `llm.agent` waits for trust.
    #[test]
    fn only_llm_agent_chooses_a_row() {
        use super::{AGY, CLAUDE, NAMES, ROWS, STAND_IN, Wire, row_named, row_of};
        let words = ["agent", "wire", "acp", "backend", "row"];
        let naming: Vec<&str> = (crate::settings::SETTINGS.iter().map(|s| s.path))
            .filter(|p| p.split(['.', '-']).any(|w| words.contains(&w)))
            .collect();
        assert_eq!(naming, ["llm.agent"], "only llm.agent names an agent");
        let default = crate::llm::LlmConfig::default();
        let other = |command: &str, args: &[&str]| crate::llm::LlmConfig {
            command: command.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            ..crate::llm::LlmConfig::default()
        };
        for cfg in [
            default.clone(),
            other("agy", &["acp"]),
            other("/usr/bin/gemini", &["--experimental-acp"]),
            other(STAND_IN.command, STAND_IN.args),
        ] {
            assert!(std::ptr::eq(row_of(&cfg), &CLAUDE), "{} {:?}", cfg.command, cfg.args);
            assert_eq!(row_of(&cfg).wire, Wire::Claude);
        }
        // (`llm.agent`, as your config file says it)
        let (agy, warnings) = crate::config::parse("[llm]\nagent = \"agy\"\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(std::ptr::eq(row_of(&agy.editor.llm), &AGY));
        assert_eq!((AGY.wire, AGY.command, AGY.args), (Wire::Acp, "agy", &["acp"][..]));
        for name in [STAND_IN.name, "acp", "gemini"] {
            let (cfg, warnings) = crate::config::parse(&format!("[llm]\nagent = \"{name}\"\n"));
            assert!(!warnings.is_empty(), "{name}: said");
            assert!(std::ptr::eq(row_of(&cfg.editor.llm), &CLAUDE), "{name}");
        }
        assert!(crate::trust::RISKY.contains(&"llm.agent"), "a project's waits for trust");
        // (its choices are the rows' names, a row found by its kept name alone — never the stand-in's)
        assert_eq!(ROWS.iter().map(|r| r.name).collect::<Vec<_>>(), NAMES);
        assert!(row_named("agy").is_some_and(|r| std::ptr::eq(r, &AGY)));
        assert!(row_named("acp").is_none() && row_named("").is_none() && row_named(STAND_IN.name).is_none());
        // (`llm.command` and `llm.args` default to Claude's row's own)
        assert_eq!((default.command.as_str(), default.args.len()), (CLAUDE.command, CLAUDE.args.len()));
    }

    /// Step 4's L2: an agent's version other than the one its guards were measured on is said, in plain words cut as a
    /// tool's name is; none given is said too; the measured one, and Claude's any, say nothing.
    #[test]
    fn an_agents_other_version_is_said_never_refused() {
        use super::{AGY, CLAUDE, version_said};
        assert_eq!(version_said(&AGY, Some("1.5.9")), None);
        assert_eq!(
            version_said(&AGY, Some("1.6.0")).as_deref(),
            Some("agy 1.6.0: tarae's guards were measured on 1.5.9")
        );
        assert_eq!(
            version_said(&AGY, None).as_deref(),
            Some("agy, its version unsaid: tarae's guards were measured on 1.5.9")
        );
        let odd = version_said(&AGY, Some("2\u{1b}[31m\n")).unwrap();
        assert!(odd.starts_with("agy 2?[31m?:"), "{odd}");
        assert_eq!(version_said(&CLAUDE, Some("9.9.9")), None);
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
    /// (guard 2 — the rule, not a row's choice), its own tools said off only where a recipe turns them off
    /// (guard 1), and a mode it keeps named, if any (guard 2).
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
        if row.mode.is_some_and(str::is_empty) {
            broken.push("its mode named empty");
        }
        broken
    }

    /// Agents beside Claude, step 3 (plan §1): every row on the ACP wire — `ROWS` and the stand-in's — keeps the
    /// floor: its asks refused, and its own tools off only by a recipe (`acp::CAPS` says off; a row copying it with
    /// no recipe would pass `llm::held_for` with nothing turned off).
    #[test]
    fn every_acp_row_keeps_the_floor() {
        use super::{ROWS, Row, STAND_IN, Wire};
        for row in ROWS.iter().copied().chain([&STAND_IN]) {
            assert!(floor(row).is_empty(), "{}: {:?}", row.name, floor(row));
        }
        // (and it sees each: a row on `acp::CAPS` with no recipe, and one whose asks are read)
        let row = |caps, own_tools| Row {
            name: "x",
            shown: "x",
            wire: Wire::Acp,
            command: "x",
            args: &[],
            caps,
            own_tools,
            mode: Some("default"),
            held: &[],
            measured: None,
        };
        let asks = super::Caps { asks: true, ..super::acp::CAPS };
        assert_eq!(floor(&row(super::acp::CAPS, None)).len(), 1);
        assert_eq!(floor(&row(asks, Some(super::AGY_TOOLS_OFF))), ["its asks read"]);
        let empty = Row { mode: Some(""), ..row(super::acp::CAPS, Some(super::AGY_TOOLS_OFF)) };
        assert_eq!(floor(&empty), ["its mode named empty"]);
    }

    /// Every role, one of each kind.
    const ROLES: [super::Role; 6] = [
        super::Role::Chat,
        super::Role::Ask,
        super::Role::Draft,
        super::Role::Reviewer(crate::reviewer::Role::One),
        super::Role::WebSearch,
        super::Role::CardReview { max_usd: 0.05 },
    ];

    /// Agents beside Claude, step 4, C.2: every row chats — the chat's role takes the config's row, whichever — and
    /// Claude's can be every role.
    #[test]
    fn every_row_chats() {
        use super::{CLAUDE, ROWS, RoleKind, STAND_IN};
        for row in ROWS.iter().copied().chain([&STAND_IN]) {
            assert!(row.caps.roles.contains(&RoleKind::Chat), "{} can't chat", row.name);
        }
        assert!(ROLES.iter().all(|r| CLAUDE.caps.roles.contains(&r.kind())));
        assert_eq!(RoleKind::ALL.len(), ROLES.len(), "each kind once");
    }

    /// Agents beside Claude, step 4, C.2: a role's start runs its row where the row can be that role, else it's held
    /// in that role's words.
    #[test]
    fn a_role_runs_its_row_only_where_it_can_be_it() {
        use super::{CLAUDE, STAND_IN, can_be};
        for role in ROLES {
            assert!(can_be(&CLAUDE, role).is_ok_and(|r| std::ptr::eq(r, &CLAUDE)), "{role:?}");
        }
        assert!(can_be(&STAND_IN, super::Role::Chat).is_ok_and(|r| std::ptr::eq(r, &STAND_IN)));
        let said: Vec<String> = ROLES[1..].iter().map(|&r| can_be(&STAND_IN, r).unwrap_err()).collect();
        assert_eq!(
            said,
            [
                "not yet with stand-in — space i runs on Claude's sessions only",
                "not yet with stand-in — a commit draft runs on Claude's sessions only",
                "not yet with stand-in — the reviewer runs on Claude's sessions only",
                "not yet with stand-in — the web search runs on Claude's sessions only",
                "not yet with stand-in — auto cards' reviewer runs on Claude's sessions only",
            ]
        );
    }

    /// Agents beside Claude, step 4, C.2: `agent::start` refuses a role its row can't be before anything runs — a
    /// start that skipped `llm::held_for` never reaches the ACP wire, which has only the chat's conversation.
    #[test]
    fn a_role_its_row_cant_be_never_reaches_its_wire() {
        use super::{First, Place, Role, STAND_IN, SpawnError, Start, start};
        let cfg = crate::llm::LlmConfig::default();
        let s = Start::on(&STAND_IN, Role::Ask, &cfg);
        assert!(std::ptr::eq(s.row, &STAND_IN));
        let ed = crate::editor::Editor::new(crate::config::Config::default());
        let at = Place { dir: &std::env::temp_dir(), given: None, cred: None };
        let first = First { tools: false, prompt: None };
        let started = start(&s, at, first, true, ed.events.sender(), |_| None);
        let Err(SpawnError::Role(why)) = started else { panic!("not refused for its role") };
        assert_eq!(why, "not yet with stand-in — space i runs on Claude's sessions only");
    }

    /// Agents beside Claude, step 4, C.2 (L3 note 5): a tripped row holds every start of it until you quit — the
    /// chat's and each other role's it can be — and no other row's.
    #[test]
    fn a_tripped_row_holds_every_start_of_it() {
        use super::{Caps, RoleKind, Row, Wire};
        static EVERY: Row = Row {
            name: "every",
            shown: "every",
            wire: Wire::Acp,
            command: "x",
            args: &[],
            caps: Caps { roles: RoleKind::ALL, ..super::acp::CAPS },
            own_tools: Some(super::AGY_TOOLS_OFF),
            mode: Some("default"),
            held: &[],
            measured: None,
        };
        let mut ed = crate::editor::Editor::new(crate::config::Config::default());
        ed.config.llm.agent = &EVERY;
        let held = |ed: &crate::editor::Editor| ROLES.map(|r| crate::llm::held_for(ed, r));
        assert_eq!(held(&ed), ROLES.map(|_| None), "a row that can be every role, not tripped");
        ed.agents_tripped.push(EVERY.name);
        let why = "no every until you quit — it ran a tool of its own";
        assert_eq!(held(&ed), ROLES.map(|_| Some(why.to_string())));
        ed.config.llm.agent = &super::CLAUDE;
        assert_eq!(held(&ed), ROLES.map(|_| None), "Claude's starts aren't the tripped row's");
    }

    /// What builds or starts an agent's start: a function holding one asks `llm::held_for` in its own body.
    const STARTS: &[&str] = &["agent::start(", "Start::new(", "Start::on("];

    /// The functions that build a start and ask `llm::held_for` nowhere in their own body: each builds a role's
    /// `Start` for the one function that starts it, which asks. None of them starts one. This list only shrinks: a
    /// builder folded into its start leaves it, none is added.
    const BUILDERS: &[(&str, &str)] = &[
        ("ask.rs", "agent_start"),
        ("autocards.rs", "start"),
        ("llm.rs", "draft_start"),
        ("reviewer.rs", "start"),
        ("web/search.rs", "helper"),
    ];

    /// The functions of `code` (a file's, literals emptied) holding one of `STARTS` that don't ask `held_for` in their
    /// own body, by name (`(outside a fn)` for one outside any), each with whether it starts one (`agent::start`).
    fn unheld(code: &str) -> Vec<(String, bool)> {
        let fns = fns_of(code);
        // (each name once: `agent::start` is looked for first, so of two of one name the one that starts is said)
        let mut out: Vec<(String, bool)> = Vec::new();
        for pattern in STARTS {
            for (name, body) in holding(code, &fns, pattern) {
                let held = body.contains("held_for(") || body.contains("held_on(");
                if !held && !out.iter().any(|(n, _)| n == name) {
                    out.push((name.to_string(), body.contains("agent::start(")));
                }
            }
        }
        out
    }

    /// The functions of `code` (a file's, literals emptied): each one's name and its body's span.
    fn fns_of(code: &str) -> Vec<(&str, usize, usize)> {
        let ident = |c: char| c.is_alphanumeric() || c == '_';
        let mut fns: Vec<(&str, usize, usize)> = Vec::new();
        for (at, _) in code.match_indices("fn ").filter(|(at, _)| !code[..*at].ends_with(ident)) {
            let rest = &code[at + 3..];
            let name = &rest[..rest.find(|c: char| !ident(c)).unwrap_or(rest.len())];
            // (its body's brace, or a declaration's `;` — neither inside its parameters: `[u8; 4]`)
            let mut nested = 0usize;
            let open = rest.char_indices().find(|&(_, c)| {
                match c {
                    '(' | '[' => nested += 1,
                    ')' | ']' => nested = nested.saturating_sub(1),
                    _ => {}
                }
                nested == 0 && (c == '{' || c == ';')
            });
            let Some((open, '{')) = open.filter(|_| !name.is_empty()) else { continue };
            let (start, mut depth) = (at + 3 + open, 0usize);
            let end = code[start..].char_indices().find_map(|(i, c)| {
                match c {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => return None,
                }
                (depth == 0).then_some(start + i + 1)
            });
            fns.push((name, start, end.unwrap_or(code.len())));
        }
        fns
    }

    /// Each place `pattern` is in `code` (not after an identifier's character: `myagent::start(` isn't
    /// `agent::start(`), as the innermost of `fns` holding it — its name and body; `(outside a fn)` and nothing for one
    /// outside any.
    fn holding<'c>(code: &'c str, fns: &[(&'c str, usize, usize)], pattern: &str) -> Vec<(&'c str, &'c str)> {
        let ident = |c: char| c.is_alphanumeric() || c == '_';
        let mut out = Vec::new();
        for (at, _) in code.match_indices(pattern) {
            if code[..at].ends_with(ident) {
                continue;
            }
            let inner = fns.iter().filter(|f| f.1 <= at && at < f.2).max_by_key(|f| f.1);
            out.push(match inner {
                Some(&(name, start, end)) => (name, &code[start..end]),
                None => ("(outside a fn)", ""),
            });
        }
        out
    }

    /// What `unheld` finds in `codes` that isn't a builder of `BUILDERS` starting nothing, as `file: fn`.
    fn skipping(codes: &[(String, String)]) -> Vec<String> {
        let mut out = Vec::new();
        for (key, code) in codes {
            for (name, starts) in unheld(code) {
                if starts || !BUILDERS.contains(&(key.as_str(), name.as_str())) {
                    out.push(format!("{key}: {name}"));
                }
            }
        }
        out
    }

    /// Agents beside Claude, step 4, C.2 (L3 note 5): every start of an agent asks `llm::held_for` first — the config
    /// read, its row not tripped, the role one its row can be — so no start skips the hold once a config can choose a
    /// row. A function that starts one (`agent::start`) asks in its own body; one that only builds a `Start` is a
    /// builder of `BUILDERS`, a list that only shrinks.
    #[test]
    fn every_start_passes_through_held_for() {
        assert!(BUILDERS.len() <= 5, "this list only shrinks");
        let codes = crate::sim::lint::codes();
        for (file, name) in BUILDERS {
            let code = codes.iter().find(|(key, _)| key == file).map(|(_, c)| c.as_str());
            let found = code.is_some_and(|c| unheld(c).iter().any(|(n, _)| n == name));
            assert!(found, "{file}: {name} builds no start now: take it off");
        }
        let skips = skipping(&codes);
        assert!(skips.is_empty(), "a start that doesn't ask `held_for`: {skips:?}");
        // (and it sees each: a start that doesn't ask, a builder that starts, a nested one, one outside any function)
        let code = |key: &str, c: &str| (key.to_string(), c.to_string());
        let made_up = [
            code("ask.rs", "fn start(ed: &mut Editor) { agent::start(&s, at, first, true, tx, route); }"),
            code(
                "term.rs",
                "fn go(ed: &Editor) { if llm::held_for(ed, R).is_some() { return; } agent::start(&s); }",
            ),
            code("llm.rs", "fn draft_start(cfg: &C) -> R { crate::agent::start(&Start::new(R, cfg)) }"),
            code(
                "web/search.rs",
                "fn helper(cfg: &C) -> Start<'_> { Start { system: None, ..Start::new(R, cfg) } }",
            ),
            code("x.rs", "fn outer() { held_for(ed, R); fn inner() { let s = Start::on(row, R, cfg); } }"),
            code("y.rs", "const S: u8 = agent::start(&s);"),
            code("z.rs", "fn wants() { myagent::start(&s); }"),
            code("w.rs", "fn four(a: [u8; 4]) { agent::start(&s); }"),
            code("reviewer.rs", "fn start() -> S { Start::new(R, c) } fn start() { agent::start(&s); }"),
        ];
        let found = [
            "ask.rs: start",
            "llm.rs: draft_start",
            "x.rs: inner",
            "y.rs: (outside a fn)",
            "w.rs: four",
            "reviewer.rs: start",
        ];
        assert_eq!(skipping(&made_up), found);
    }

    /// What reads the config's row (`llm.agent`).
    const CONFIG_ROW_READS: &[&str] = &["agent::row_of(", "llm.agent"];

    /// The only functions that read the config's row: where a session that has none takes it (`llm::session_row`, a
    /// chat made, restored or begun anew), the config read or set (a change said — `chat::agent_moved`), a session's
    /// usage made before its chat, and the setting's own row. None of them builds or starts a start.
    const CONFIG_ROW: &[(&str, &str)] = &[
        ("llm.rs", "workspace_row"),
        ("chat.rs", "made"),
        ("chat.rs", "restore"),
        ("chat.rs", "reset_said"),
        // (its toast names the row the new conversation starts on — step 4's L3)
        ("chat.rs", "reset"),
        ("chat.rs", "agent_moved"),
        // (the set card names the row its sessions, having none, start on — step 4's L3)
        ("orchestra.rs", "card"),
        // (the welcome's brand line names the agent chosen — the user's decision, chat 2026-11-16, item 6)
        ("term.rs", "brand_line"),
        ("editor.rs", "with_root"),
        ("editor.rs", "reload_config_ours"),
        ("typed.rs", "set_value"),
        ("workspace.rs", "workspace_add"),
        ("settings.rs", "(outside a fn)"),
    ];

    /// What in `codes` reads the config's row outside `CONFIG_ROW`, or in one of them that builds or starts a start
    /// (`STARTS`), as `file: fn`.
    fn config_row_read(codes: &[(String, String)]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (key, code) in codes {
            let fns = fns_of(code);
            for (name, body) in CONFIG_ROW_READS.iter().flat_map(|p| holding(code, &fns, p)) {
                let starts = STARTS.iter().any(|s| body.contains(s));
                let said = format!("{key}: {name}");
                if (starts || !CONFIG_ROW.contains(&(key.as_str(), name))) && !out.contains(&said) {
                    out.push(said);
                }
            }
        }
        out
    }

    /// Agents beside Claude, step 4's L2 (the lint follows the row): every start takes the session's row
    /// (`llm::session_row` — its own once fixed, else the config's), so the config's row (`llm.agent`) is read only
    /// where a session that has none takes it, or a change of it is said — never by a builder or a start, which would
    /// put a session begun on one agent on another.
    #[test]
    fn every_start_takes_the_sessions_row() {
        let codes = crate::sim::lint::codes();
        for (file, name) in CONFIG_ROW {
            let code = codes.iter().find(|(key, _)| key == file).map(|(_, c)| c.as_str());
            let reads = code.is_some_and(|c| {
                let fns = fns_of(c);
                CONFIG_ROW_READS.iter().any(|p| holding(c, &fns, p).iter().any(|(n, _)| n == name))
            });
            assert!(reads, "{file}: {name} reads no config's row now: take it off");
        }
        let reads = config_row_read(&codes);
        assert!(reads.is_empty(), "the config's row read where a start is made: {reads:?}");
        // (and it sees each: a builder reading it, a start reading its field, a listed one that starts, a listed one)
        let code = |key: &str, c: &str| (key.to_string(), c.to_string());
        let made_up = [
            code("ask.rs", "fn agent_start(cfg: &C) -> S { Start::on(crate::agent::row_of(cfg), R, cfg) }"),
            code(
                "web/search.rs",
                "fn start(ed: &mut E) { let row = ed.config.llm.agent; agent::start(&s); }",
            ),
            code("chat.rs", "fn agent_moved(ed: &mut E) { let r = agent::row_of(c); agent::start(&s); }"),
            code("chat.rs", "fn made(ed: &mut E) { c.agent = crate::agent::row_of(&ed.config.llm); }"),
        ];
        let found = ["ask.rs: agent_start", "web/search.rs: start", "chat.rs: agent_moved"];
        assert_eq!(config_row_read(&made_up), found);
    }

    /// Step 4's L3: a parent's chat says what its agent was told by its row (`told stand-in: …`), and reads back as a
    /// told row whatever the row — never a note that only starts so.
    #[test]
    fn a_told_row_names_its_agent_and_reads_back() {
        use crate::agent::{AGY, CLAUDE, STAND_IN, shown_of};
        assert_eq!(crate::orchestra::told(&STAND_IN), "told stand-in: ");
        for row in [&CLAUDE, &AGY, &STAND_IN] {
            assert!(crate::orchestra::is_told(&format!("{}k's turn ended", crate::orchestra::told(row))));
        }
        assert!(!crate::orchestra::is_told("told someone: k's turn ended"));
        assert_eq!(shown_of("stand-in"), "stand-in");
        assert_eq!(shown_of("claude"), "Claude");
        assert_eq!(shown_of("cargo"), "cargo");
    }

    /// The literals of `src` (code as a non-test build compiles it, comments out) that say `Claude`, each its text.
    fn claude_literals(src: &str) -> Vec<String> {
        let (mut out, mut cs) = (Vec::new(), src.chars().peekable());
        while let Some(c) = cs.next() {
            match c {
                // (a char literal: `'"'`, `'\''` — else a lifetime, nothing to skip)
                '\'' => {
                    let rest: String = cs.clone().take(3).collect();
                    if rest.starts_with('\\') {
                        cs.by_ref().take_while(|&c| c != '\'').for_each(drop);
                    } else if rest.chars().nth(1) == Some('\'') {
                        cs.nth(1);
                    }
                }
                '"' => {
                    let mut lit = String::new();
                    while let Some(c) = cs.next() {
                        match c {
                            '\\' => lit.extend(cs.next()),
                            '"' => break,
                            c => lit.push(c),
                        }
                    }
                    if lit.contains("Claude") {
                        out.push(lit);
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Step 4's C.3, §4's check: the literals naming Claude in every file of the code's roots, counted — a file not
    /// listed is allowed none, so a new file can't skip it. One about the session's agent — its process, turn,
    /// conversation, edits — says `Row.shown`; one about Claude Code itself (its settings, login, environment,
    /// billing, usage, update), a prompt's or a tool's words to the agent stays. A new one fails here with its text.
    ///
    /// For now the list is two things in one (frozen at L3, as the walk found them): Claude Code's own literals, which
    /// stay, and those not yet moved to the row's name. It only shrinks, as the clock and sleep lists do — lower a
    /// file's count when you move one. Part C's naming landing, before the push, splits the two: the ones left moved
    /// to the row, the rest counted as Claude Code's own with their reasons — so "shrinks" has an end.
    #[test]
    fn every_literal_naming_claude_on_screen_is_claudes_own() {
        // (the command docs, `cmdline.rs`' and `commands.rs`', are written once and drawn by `llm::agent_doc`: the
        // session's row's name where it isn't Claude, but `llm::CLAUDE_ONLY_COMMANDS`')
        const CLAUDE_ONLY: &[(&str, usize)] = &[
            ("agent.rs", 2),
            ("allow.rs", 5),
            ("attach.rs", 1),
            ("billing.rs", 6),
            ("blocked.rs", 8),
            ("board.rs", 3),
            ("cardreview.rs", 1),
            ("cards.rs", 1),
            // (the system prompt)
            ("chat.rs", 1),
            ("checkpoint.rs", 4),
            ("claudetrust.rs", 18),
            ("client.rs", 1),
            ("cmdline.rs", 22),
            // (the reviewer's words in a prompt)
            ("codereview.rs", 2),
            ("commands.rs", 18),
            ("compaction.rs", 3),
            ("diskroom.rs", 4),
            ("edits.rs", 8),
            ("edittool.rs", 1),
            ("endsave.rs", 1),
            ("envkey.rs", 4),
            ("features.rs", 3),
            ("folders.rs", 8),
            ("follow.rs", 6),
            ("gitpanel.rs", 5),
            ("gitrepo/held.rs", 2),
            ("granted.rs", 3),
            ("hooks.rs", 2),
            ("hosts.rs", 3),
            ("idlecompact.rs", 2),
            ("instructions.rs", 6),
            ("keepclose.rs", 4),
            ("kubeconfig.rs", 2),
            ("limits.rs", 1),
            // (Claude Code's install, login and environment)
            ("llm.rs", 9),
            ("models.rs", 1),
            ("offer.rs", 2),
            // (billing's two, the cap's and two tools' words to the agent, a prompt's frame, the restart for the
            // project's Claude settings)
            ("orchestra.rs", 7),
            ("plugin/install.rs", 1),
            ("reads.rs", 9),
            // (the bring card's `(Claude's draft, :w)`, two)
            ("rebase.rs", 2),
            ("refactor.rs", 3),
            ("relay.rs", 1),
            ("repotrust.rs", 2),
            ("reviewer.rs", 2),
            ("rundoc.rs", 2),
            ("sandbox.rs", 5),
            ("sandbox/apparmor.rs", 2),
            ("sandbox/runenv.rs", 1),
            ("sandboxpack.rs", 1),
            ("settings.rs", 37),
            ("sound.rs", 1),
            ("testing.rs", 1),
            ("tokens.rs", 2),
            ("tools.rs", 33),
            ("uisocket.rs", 3),
            ("uistate/watch.rs", 4),
            ("usage.rs", 5),
            ("view.rs", 1),
            ("web/page.rs", 1),
            ("web/search.rs", 6),
            ("web/signed.rs", 1),
            ("web/tool.rs", 9),
            ("wordspot.rs", 1),
            // (billing's line, and the commit first's `Claude drafts it`)
            ("worktree.rs", 2),
        ];
        // (every file of the code's roots: one not listed is allowed none, so a new file can't skip it)
        let sources = crate::sim::lint::sources();
        let missing: Vec<&str> =
            CLAUDE_ONLY.iter().map(|(f, _)| *f).filter(|f| !sources.iter().any(|(k, _)| k == f)).collect();
        assert!(missing.is_empty(), "listed but not in the code: {missing:?}");
        let mut wrong = Vec::new();
        for (file, src) in &sources {
            let want = CLAUDE_ONLY.iter().find(|(f, _)| f == file).map_or(0, |(_, n)| *n);
            let found = claude_literals(src);
            if found.len() != want {
                let cut: Vec<String> = found.iter().map(|l| l.chars().take(90).collect()).collect();
                wrong.push(format!("{file}: {} (listed {want}):\n  {}", found.len(), cut.join("\n  ")));
            }
        }
        assert!(wrong.is_empty(), "literals naming Claude:\n{}", wrong.join("\n"));
        // (and it sees one, past a char literal of a quote and a lifetime)
        let made_up =
            "fn f<'a>(s: &'a str) { s.replace(['\"', '<'], \"\"); g(\"Ask Claude\\\"s\"); h('\\'', \"x\") }";
        assert_eq!(claude_literals(made_up), ["Ask Claude\"s"]);
    }
}
