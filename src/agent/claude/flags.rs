//! Claude Code's flags, each start's (`agent::Start`): what a role asks for, in Claude's words — its tools, the
//! conversation it continues, its system prompt, its settings, its limits. Every claude tarae starts is made by
//! `command` (`agent::command`, `agent::spawn`); `agent::argv_tests` holds each start's argv.

use std::path::Path;
use std::process::Command;

use crate::agent::{Args, Role, SpawnError, Start};
use crate::billing::Credential;
use crate::llm::{Env, LlmConfig};

/// `s`'s arguments: in place of `llm.args`, and after them. Err: `llm.args` can't take what it must add.
pub fn of(s: &Start) -> Result<Args, String> {
    let base = &s.cfg.args;
    let system = s.system.unwrap_or_default();
    // (tarae's words beside Claude Code's own, and its permission asks on stdin — answered by tarae)
    let beside =
        || ["--append-system-prompt", system, "--permission-prompt-tool", "stdio"].map(String::from).to_vec();
    Ok(match s.role {
        Role::Chat => chat::args(s),
        Role::Ask => Args { args: ask::args(base), extra: beside() },
        Role::Reviewer(role) => Args { args: reviewer::args_for(base, role)?, extra: beside() },
        Role::Draft => Args { args: base.clone(), extra: Vec::new() },
        Role::WebSearch => Args { args: web_search::args(base, system), extra: Vec::new() },
        Role::CardReview { max_usd } => Args { args: card_review::args(system, max_usd), extra: Vec::new() },
    })
}

/// `s`'s process, not started: Claude's `command` on its arguments — a board's member without what would set its
/// cache's time over its own (`reviewer::CACHE_TTL_ENV`).
pub fn command(s: &Start, dir: &Path, given: Option<&Env>, cred: &Credential) -> Result<Command, SpawnError> {
    let Args { args, extra } = of(s).map_err(SpawnError::Args)?;
    let cfg = LlmConfig { args, ..s.cfg.clone() };
    let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
    let mut cmd = super::command(&cfg, &extra, dir, given, cred)?;
    if let Role::Reviewer(role) = s.role
        && !matches!(role, crate::reviewer::Role::One)
    {
        for name in reviewer::CACHE_TTL_ENV {
            cmd.env_remove(name);
        }
    }
    Ok(cmd)
}

/// The chat's.
pub mod chat {
    use super::*;
    use crate::agent::claude;

    /// The tools the chat process gets. Reading and tarae's own (`note`, `run_tests`, `run` — `tools.rs`) are
    /// allowed up front: the tests are the project's own, run the way the user runs them, in view, and any code
    /// Claude wrote for them was reviewed; `run` asks the user itself for anything but the checks. No Grep or
    /// Glob: tarae's `search` and `read` do what they did — 54 exploration turns on three builds called Grep never
    /// and Glob 3 times, and leaving them out takes 1,570 tokens off every request (tests/live.rs `chat_tools_ab`,
    /// 2026-10-13).
    /// No Edit or Write: tarae's `edit` makes every change (`ALLOWED`'s; the A/B that decided it: tests/live.rs
    /// `chat_edit_ab`, 2026-10-13 — as correct, 31 % fewer tokens), so the chat has no tool of Claude Code's at all.
    /// No Read: tarae's `read` reads images too (readimg, 2026-10-20 — `media.rs`; the A/B: tests/live.rs
    /// `chat_reads_a_screenshot_ab`).
    pub const TOOLS: &str = "";
    /// Not Read: allowed by name it reads anywhere unasked (measured — `reads.rs` answers it).
    /// (`references`, `symbols`, `definition` and `diagnostics` read inside the project only — `nav.rs`; `search`
    /// and `read` outside it only as Read would, on its card — `reads::Gate`; `rename` and `replace` land as Claude's
    /// edits do, in the review — `refactor.rs`, and `edit` too — `edittool.rs`; none of them reads or changes a file
    /// named like a key before Claude was let read it — `reads::Keys`; `ui` the screen's structure at Claude's level
    /// — `uistate/claude.rs`; `git` only git's reads, on the guarded path — `gittool.rs`; `sessions_start` asks on its
    /// own card, `session_send` and `sessions` reach only the chat's own children, and only a parent's chat lists them —
    /// `orchestra.rs`; `plugin` asks on its own card — `plugin/install.rs`; `web_search` asks on its own card, a chat's
    /// first — `web/search.rs` — and is listed only with `llm.web-search` on)
    pub const ALLOWED: &str = "mcp__tarae__run_tests,mcp__tarae__run,mcp__tarae__plan,\
mcp__tarae__references,mcp__tarae__search,mcp__tarae__read,mcp__tarae__symbols,mcp__tarae__definition,\
mcp__tarae__diagnostics,mcp__tarae__rename,mcp__tarae__replace,mcp__tarae__ui,mcp__tarae__edit,mcp__tarae__git,\
mcp__tarae__plugin,mcp__tarae__sessions_start,mcp__tarae__session_send,mcp__tarae__sessions,mcp__tarae__session_act,\
mcp__tarae__web_search,mcp__tarae__code";

    /// The chat's claude: tarae's tools, the conversation kept and maybe continued, tarae's MCP server only, the
    /// project's settings as the chat loads them, tarae's words beside its own, its thinking as text.
    pub fn args(s: &Start) -> Args {
        let mut args = claude::with_tools(&s.cfg.args, TOOLS, ALLOWED);
        // The conversation is kept by claude too, so it can be continued (`chats.rs`)
        args.retain(|a| a != "--no-session-persistence");
        // Only tarae's own MCP server, whatever `llm.args` says: under `-p` a project's `.mcp.json` servers connect
        // unasked
        if !args.iter().any(|a| a == "--strict-mcp-config") {
            args.push("--strict-mcp-config".into());
        }
        if let Some(r) = &s.resume {
            args.extend(["--resume".to_string(), r.conversation.to_string()]);
            // (a fork's first claude: the source's conversation copied under a new id, the source's untouched)
            if r.fork {
                args.push("--fork-session".into());
            }
        }
        if let Some((settings, sources)) = s.claude.settings {
            // CLAUDE.md files and settings as `claude` loads them — but hooks don't run, and edits always come
            // here as a review (an `ask` rule and the default mode outrank any allow rule or accept-edits mode)
            args = claude::with_flag(&args, "--setting-sources", sources);
            args = claude::with_flag(&args, "--settings", settings);
            args = claude::with_flag(&args, "--permission-mode", "default");
        }
        let system = s.system.unwrap_or_default();
        let mut extra: Vec<String> =
            ["--append-system-prompt", system, "--permission-prompt-tool", "stdio"].map(String::from).into();
        // (an older claude refused it: started again without — `again`)
        if s.thinking_text && !s.again.is_some_and(|a| a.thinking_plain) {
            extra.extend(["--thinking-display", "summarized"].map(String::from));
        }
        Args { args, extra }
    }
}

/// `space i`'s.
pub mod ask {
    use crate::agent::claude;

    /// Its tools: tarae's two, none of Claude Code's.
    pub const ALLOWED: &str = "mcp__tarae__read,mcp__tarae__edit";
    pub const DISALLOWED: &str = "Read,Grep,Glob,Edit,Write,NotebookEdit,Bash,WebFetch,WebSearch,Task";

    /// The most requests it makes: it has tools now, and a few reads and an edit's retry fit.
    pub const MAX_TURNS: &str = "10";

    /// Its claude's arguments from `base` (`llm.args`): never a conversation to continue, no settings, tarae's two
    /// tools only, a request cap.
    pub fn args(base: &[String]) -> Vec<String> {
        let mut args: Vec<String> = Vec::with_capacity(base.len() + 12);
        let mut it = base.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--resume" | "--session-id" => {
                    it.next();
                }
                "--continue" | "--fork-session" => {}
                _ => args.push(a.clone()),
            }
        }
        let mut args = claude::with_tools(&args, "", ALLOWED);
        if !args.iter().any(|a| a == "--no-session-persistence") {
            args.push("--no-session-persistence".into());
        }
        for (flag, value) in [
            ("--setting-sources", ""),
            ("--disallowedTools", DISALLOWED),
            ("--permission-mode", "default"),
            ("--max-turns", MAX_TURNS),
        ] {
            args = claude::with_flag(&args, flag, value);
        }
        args
    }

    #[cfg(test)]
    mod tests {
        /// Its arguments from `llm.args` with a conversation's flags in them: none of those survives, and it has no
        /// settings, Claude Code's tools disallowed, tarae's two allowed, its request cap.
        #[test]
        fn its_args_carry_no_conversation_and_its_own_limits() {
            let mut base = crate::llm::LlmConfig::default().args;
            base.extend(
                ["--resume", "x", "--continue", "--session-id", "y", "--fork-session"].map(String::from),
            );
            let args = super::args(&base);
            for gone in ["--resume", "x", "--continue", "--session-id", "y", "--fork-session"] {
                assert!(!args.iter().any(|a| a == gone), "{gone} in {args:?}");
            }
            let value = |flag: &str| {
                let at: Vec<usize> =
                    args.iter().enumerate().filter(|(_, a)| *a == flag).map(|(i, _)| i).collect();
                assert_eq!(at.len(), 1, "{flag} once: {args:?}");
                args[at[0] + 1].as_str()
            };
            assert_eq!(value("--setting-sources"), "");
            assert_eq!(value("--disallowedTools"), super::DISALLOWED);
            assert_eq!(value("--max-turns"), "10");
            assert_eq!(value("--tools"), "");
            assert_eq!(value("--allowedTools"), "mcp__tarae__read,mcp__tarae__edit");
            assert_eq!(value("--permission-mode"), "default");
            assert!(args.iter().any(|a| a == "--no-session-persistence") && args.iter().any(|a| a == "-p"));
        }
    }
}

/// The reviewer's, each member's.
pub mod reviewer {
    use serde_json::{Value, json};

    use crate::agent::claude;
    use crate::reviewer::Role;

    /// Its tools: none of Claude Code's — tarae's own, allowed up front: its reading of the snapshot and its findings.
    pub const TOOLS: &str = "";
    pub const ALLOWED: &str = "mcp__tarae__read,mcp__tarae__search,mcp__tarae__note";
    /// A skeptic's: its reading and its verdicts.
    pub const JUDGE_ALLOWED: &str = "mcp__tarae__read,mcp__tarae__search,mcp__tarae__judge";
    /// What it may never use, named too (`--tools` already leaves them out).
    pub const DISALLOWED: &str = "Read,Grep,Glob,Edit,Write,NotebookEdit,Bash,WebFetch,WebSearch,Task";
    /// Its turns at most (a read is one): a review that wanders stops.
    pub const MAX_TURNS: &str = "60";
    /// A skeptic's turns at most: it traces a few claims, it doesn't read the change whole.
    pub const SKEPTIC_TURNS: &str = "25";

    /// The reviewer's claude: its arguments from `args` (llm.args), the speed flags kept — never a conversation
    /// to continue, no settings, its tools only.
    /// A skeptic's one tool of tarae's is `judge`, and its turns are fewer. Err: a board's member, and llm.args
    /// passes `--settings` as a file — tarae can't lay its cache's TTL into it unread (as `billing::args`).
    pub fn args_for(base: &[String], role: Role) -> Result<Vec<String>, String> {
        let mut args: Vec<String> = Vec::with_capacity(base.len() + 12);
        let mut it = base.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                // (another conversation — the chat's — is never its own)
                "--resume" | "--session-id" => {
                    it.next();
                }
                "--continue" | "--fork-session" => {}
                _ => args.push(a.clone()),
            }
        }
        let skeptic = matches!(role, Role::Skeptic(_));
        let mut args = claude::with_tools(&args, TOOLS, if skeptic { JUDGE_ALLOWED } else { ALLOWED });
        if !args.iter().any(|a| a == "--no-session-persistence") {
            args.push("--no-session-persistence".into());
        }
        args = claude::with_flag(&args, "--setting-sources", "");
        args = claude::with_flag(&args, "--disallowedTools", DISALLOWED);
        args = claude::with_flag(&args, "--permission-mode", "default");
        args = claude::with_flag(&args, "--max-turns", if skeptic { SKEPTIC_TURNS } else { MAX_TURNS });
        if !matches!(role, Role::One) {
            // A board's members cache an hour, on a key too (its default is five minutes): the prefix's block is an
            // hour's (`content`), and Claude Code's own breakpoints must be the same (`member_command`). Laid into a
            // `--settings` object given (the key's helper is laid in after, `billing::args`)
            let given = args.iter().rposition(|a| a == "--settings").and_then(|i| args.get(i + 1));
            let mut settings = match given.map(|v| serde_json::from_str::<Value>(v)) {
                None => serde_json::Map::new(),
                Some(Ok(Value::Object(m))) => m,
                Some(_) => return Err(SETTINGS_FILE.into()),
            };
            settings.insert("promptCacheTtl".into(), json!(CACHE_TTL));
            args = claude::with_flag(&args, "--settings", &Value::Object(settings).to_string());
        }
        Ok(args)
    }

    pub const SETTINGS_FILE: &str = "llm.args passes --settings as a file — tarae can't add the board's cache time to it (give it as JSON)";

    /// The TTL of a board's cache: the prefix's block and Claude Code's own breakpoints alike — the API refuses a
    /// longer one after a shorter one (measured: a 400 for an hour's block after Claude Code's 5-minute ones on a key).
    const CACHE_TTL: &str = super::super::CACHE_HOUR;

    /// What sets Claude Code's cache TTL over `promptCacheTtl` (its env-vars reference) — a board's members never
    /// get them: theirs must be `CACHE_TTL`.
    pub const CACHE_TTL_ENV: &[&str] = &["FORCE_PROMPT_CACHING_5M", "CLAUDE_CODE_PROMPT_CACHE_TTL"];
}

/// The web search's helper.
pub mod web_search {
    use crate::agent::claude;

    /// Its one tool, and Claude Code's others disallowed (tarae's MCP server isn't there: `--strict-mcp-config` and no
    /// `initialize`).
    pub const TOOLS: &str = "WebSearch";
    pub const DISALLOWED: &str = "Read,Grep,Glob,Edit,Write,NotebookEdit,Bash,WebFetch,Task";

    /// One request: the tool runs and its result comes, then the `result` with the cost — no closing words (probe C).
    pub const MAX_TURNS: &str = "1";

    /// The helper's arguments from `base` (`llm.args`): never a conversation to continue, no settings, WebSearch
    /// alone, one request, `system` in place of Claude Code's own prompt.
    pub fn args(base: &[String], system: &str) -> Vec<String> {
        let mut args: Vec<String> = Vec::with_capacity(base.len() + 12);
        let mut it = base.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--resume" | "--session-id" | "--system-prompt" | "--append-system-prompt" => {
                    it.next();
                }
                "--continue" | "--fork-session" => {}
                _ => args.push(a.clone()),
            }
        }
        let mut args = claude::with_tools(&args, TOOLS, TOOLS);
        for flag in ["--no-session-persistence", "--strict-mcp-config"] {
            if !args.iter().any(|a| a == flag) {
                args.push(flag.into());
            }
        }
        for (flag, value) in [
            ("--setting-sources", ""),
            ("--disallowedTools", DISALLOWED),
            ("--permission-mode", "default"),
            ("--max-turns", MAX_TURNS),
            ("--system-prompt", system),
        ] {
            args = claude::with_flag(&args, flag, value);
        }
        args
    }
}

/// Who pays (`billing.rs`): an API session's key helper, laid into every start's `--settings` (`claude::command`).
pub mod billing {
    use serde_json::{Map, Value as Json};

    use crate::agent::claude;
    use crate::billing::Credential;

    /// `args` with the credential laid into their `--settings` object (one made if there's none) — merged, never
    /// replacing what's there. Err: a `--settings` that names a file (tarae can't add to it unread).
    pub fn args(args: &[String], cred: &Credential) -> Result<Vec<String>, String> {
        let Credential::Api { helper, hour_cache } = cred else { return Ok(args.to_vec()) };
        let given = args.iter().rposition(|a| a == "--settings").and_then(|i| args.get(i + 1));
        let mut json = match given.map(|v| serde_json::from_str::<Json>(v)) {
            None => Map::new(),
            Some(Ok(Json::Object(m))) => m,
            Some(_) => {
                return Err(
                    "llm.args passes --settings as a file — tarae can't add the API key's helper to it"
                        .into(),
                );
            }
        };
        json.insert("apiKeyHelper".into(), Json::String(helper.clone()));
        if *hour_cache {
            json.insert("promptCacheTtl".into(), Json::String("1h".into()));
        }
        Ok(claude::with_flag(args, "--settings", &Json::Object(json).to_string()))
    }

    #[cfg(test)]
    mod tests {
        use std::path::PathBuf;

        use serde_json::Value;

        use super::*;

        /// The helper joins the one `--settings` object the chat passes: `claudetrust` holds every command key up front
        /// as `""` — an API session's helper takes `apiKeyHelper`'s place, the rest stays as held; a subscription's
        /// arguments are untouched. No `--settings` yet: one made. One naming a file: refused (tarae can't add to it
        /// unread).
        #[test]
        fn the_helper_is_merged_into_the_chats_settings_never_beside_them() {
            let seen = crate::claudetrust::Seen {
                scan: None,
                state: crate::trust::State::Trusted,
                file: PathBuf::new(),
                base: Default::default(),
            };
            let (held, _) = crate::claudetrust::settings(crate::chat::CHAT_SETTINGS, &seen);
            let held_json: Value = serde_json::from_str(&held).unwrap();
            assert_eq!(held_json["apiKeyHelper"], "", "held up front");
            let given: Vec<String> = vec!["-p".into(), "--settings".into(), held.clone()];
            let api = |hour_cache| Credential::Api { helper: "the-helper".into(), hour_cache };
            let out = args(&given, &api(true)).unwrap();
            assert_eq!(out.iter().filter(|a| *a == "--settings").count(), 1, "one --settings: {out:?}");
            let got: Value = serde_json::from_str(out.last().unwrap()).unwrap();
            assert_eq!(got["apiKeyHelper"], "the-helper");
            assert_eq!(got["promptCacheTtl"], "1h");
            for (k, v) in held_json.as_object().unwrap().iter().filter(|(k, _)| *k != "apiKeyHelper") {
                assert_eq!(&got[k], v, "{k} as held");
            }
            let one_turn: Value =
                serde_json::from_str(args(&given, &api(false)).unwrap().last().unwrap()).unwrap();
            assert!(one_turn.get("promptCacheTtl").is_none());
            assert_eq!(args(&given, &Credential::Subscription).unwrap(), given, "the plan: untouched");
            let made = args(&["-p".into()], &api(false)).unwrap();
            assert_eq!(made[1..], ["--settings".to_string(), r#"{"apiKeyHelper":"the-helper"}"#.to_string()]);
            let file: Vec<String> = vec!["--settings".into(), "/some/settings.json".into()];
            assert!(args(&file, &api(true)).unwrap_err().contains("as a file"));
        }
    }
}

/// Auto cards' reviewer.
pub mod card_review {
    /// Its flags: `cardreview::args`' (no tools, no MCP, no settings, one turn, `system` its policy, `max_usd` its
    /// spend's backstop), its stream read by `pump` — the model said by its config (`command` adds it).
    pub fn args(system: &str, max_usd: f64) -> Vec<String> {
        let mut out = Vec::new();
        let mut it = crate::cardreview::args("", system, Some(max_usd)).into_iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--model" => {
                    it.next();
                }
                "--output-format" => {
                    it.next();
                    out.extend(
                        ["--input-format", "stream-json", "--output-format", "stream-json", "--verbose"]
                            .map(String::from),
                    );
                }
                _ => out.push(a),
            }
        }
        out
    }
}
