//! Claude's environment: what of tarae's environment a claude it starts never gets (`llm::command`). std only —
//! xtask includes this file by `#[path]` (auto cards' eval, `xtask/src/cardeval.rs`), so its claude leaves out the
//! same names, from this one list.
//!
//! tarae's environment can hold another Claude Code session's variables (apibill L0: tarae started from a shell a
//! desktop-app session started — `CLAUDECODE`, `CLAUDE_CODE_CHILD_SESSION`, its session id, entrypoint, effort, pid,
//! its host's account hints), and an exported API key. Passed on, a top-level chat is taken for a nested one —
//! "transcript persistence, prompt history, and `claude agents` registration" off (Claude Code's env-vars
//! reference, `CLAUDE_CODE_FORCE_SESSION_PERSISTENCE`) — and a key in the environment bills in your login's place.
//! A named list, not a `CLAUDE*` prefix: most such names are settings you may export on purpose
//! (`CLAUDE_CODE_MAX_OUTPUT_TOKENS`, `MCP_TIMEOUT`, `ANTHROPIC_BASE_URL`) — those stay.

use std::collections::BTreeSet;

/// Credentials Claude Code takes from its environment over your login (its authentication docs: under `-p` the
/// key "is always used when present"): not passed — unless you keep them (`llm.environment-key = "keep"`: a
/// login by key alone, a gateway's `ANTHROPIC_BASE_URL` with its token), and never inside a Claude Code session,
/// where they're the host's. `CLAUDE_CODE_OAUTH_TOKEN` is the host's there too (`THE_HOSTS`), else a subscription
/// login you chose (`claude setup-token`) — passed.
pub const CREDENTIALS: &[&str] = &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"];

/// What Claude Code (or its Agent SDK) sets in the processes it starts — inherited, always a parent session's,
/// never passed. Claude Code's env-vars reference says so of each, but where noted.
pub const SET_FOR_CHILDREN: &[&str] = &[
    // (the SDK drops it from what it inherits as well)
    "CLAUDECODE",
    // ("reliably distinguishes a nested session")
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_REMOTE",
    "CLAUDE_CODE_REMOTE_SESSION_ID",
    "CLAUDE_EFFORT",
    "CLAUDE_PID",
    "CLAUDE_JOB_DIR",
    // (hooks' and file suggestions' — the settings reference; apibill L5 saw claude set it for its children)
    "CLAUDE_PROJECT_DIR",
    // (the SDK's transport sets both: how its host started claude — L5 again for the entrypoint)
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_AGENT_SDK_VERSION",
    // (the changelog: "for subprocesses so `gh` can attribute traffic to Claude Code")
    "AI_AGENT",
];

/// …and whole families of them: Remote Control's, and the session inbox's ("Set by Claude Code, not by you").
pub const SET_FOR_CHILDREN_FAMILIES: &[&str] = &["CLAUDE_CODE_BRIDGE_", "CLAUDE_CODE_MESSAGING_"];

/// A Claude Code host's own, seen in tarae's environment (L0) or named by the SDK as what its host passes — no doc
/// lists them as yours to set. Left out only when tarae itself runs inside Claude Code (`ClaudeEnv::nested`).
pub const THE_HOSTS: &[&str] = &[
    // (the SDK: what a host passes the claude it starts — its login among them)
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_HOST_SESSION_ID",
    "CLAUDE_CODE_SESSION_ACCESS_TOKEN",
    "CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING",
    // (L0's, undocumented)
    "CLAUDE_CODE_SDK_HAS_HOST_AUTH_REFRESH",
    "CLAUDE_CODE_DESKTOP_APP_VERSION",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_WORKER_EPOCH",
    // (the SDK passes its span's W3C context, and a `-p` claude parents its own spans under it — the
    // monitoring docs)
    "TRACEPARENT",
    "TRACESTATE",
];

/// What tells that tarae itself runs inside a Claude Code session: a parent session's own marks, set only by
/// Claude Code (or its host) in what it starts — `CLAUDE_CODE_CHILD_SESSION` ("only set by Claude Code itself …
/// and not by IDE extensions") and `CLAUDE_PID` in its tools' and hooks' processes, `CLAUDE_CODE_SESSION_ID` in
/// those and its stdio MCP servers', `CLAUDE_CODE_HOST_SESSION_ID` what an SDK host passes the claude it starts
/// (L0 found it in tarae's). Not `CLAUDECODE`: an IDE's terminal sets it too, and a shell there is no session's —
/// your own `CLAUDE_CODE_OAUTH_TOKEN` stays.
pub const NESTED_BY: &[&str] =
    &["CLAUDE_CODE_CHILD_SESSION", "CLAUDE_PID", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_HOST_SESSION_ID"];

/// The OAuth scopes go with their refresh token ("Required when `CLAUDE_CODE_OAUTH_REFRESH_TOKEN` is set"): with
/// it, they're yours; without it, inside Claude Code, the host's.
pub const SCOPES: (&str, &str) = ("CLAUDE_CODE_OAUTH_SCOPES", "CLAUDE_CODE_OAUTH_REFRESH_TOKEN");

/// What claude gets of tarae's environment: the names left out, and why — names only, never a value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClaudeEnv {
    /// Left out (`env_remove`), sorted.
    pub removed: Vec<String>,
    /// tarae itself runs inside a Claude Code session (`NESTED_BY`).
    pub nested: bool,
    /// Nested: the `CLAUDE*`/`ANTHROPIC_*` names passed as they are — your settings, or the host's under a name
    /// tarae doesn't know yet; said, so the lists can grow. Sorted.
    pub passed: Vec<String>,
}

/// The rules over `names` (tarae's environment's — names, not values: none is needed, so none is read).
/// `keep_keys`: `llm.environment-key = "keep"`.
pub fn claude_env<'a>(names: impl IntoIterator<Item = &'a str>, keep_keys: bool) -> ClaudeEnv {
    // (a set: sorted and once each, with no sort of its own — xtask has no `sorting`)
    let names: BTreeSet<&str> = names.into_iter().collect();
    let has = |n: &str| names.contains(n);
    let nested = NESTED_BY.iter().any(|n| has(n));
    let mut out = ClaudeEnv { nested, ..Default::default() };
    for &name in &names {
        let removed = CREDENTIALS.contains(&name) && (nested || !keep_keys)
            || SET_FOR_CHILDREN.contains(&name)
            || SET_FOR_CHILDREN_FAMILIES.iter().any(|f| name.starts_with(f))
            || nested && THE_HOSTS.contains(&name)
            || nested && name == SCOPES.0 && !has(SCOPES.1);
        if removed {
            out.removed.push(name.to_string());
        } else if nested && (name.starts_with("CLAUDE") || name.starts_with("ANTHROPIC_")) {
            out.passed.push(name.to_string());
        }
    }
    out
}

/// Whether a rule of `claude_env` judges `name` — what a stand-in's claude never inherits from the process
/// (`llm::command`): the process's own such names (tarae's tests run inside a Claude Code session too) would be
/// judged on nothing.
pub fn judged(name: &str) -> bool {
    CREDENTIALS.contains(&name)
        || SET_FOR_CHILDREN.contains(&name)
        || SET_FOR_CHILDREN_FAMILIES.iter().any(|f| name.starts_with(f))
        || THE_HOSTS.contains(&name)
        || NESTED_BY.contains(&name)
        || name == SCOPES.0
        || name == SCOPES.1
}
