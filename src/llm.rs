//! LLM — runs `claude -p` (or any CLI with the same stream-json protocol) as a subprocess.
//! No HTTP·SDK·API key inside the editor — uses the user's Claude Code login as is.
//!
//! Cold start is slow (measured 2026-09-27, Claude Code 2.1.280, a single "ok"):
//! default 8–10 s → minimal options (`--tools "" --strict-mcp-config --setting-sources ""`) 4.3 s
//! → **a process spawned early and waiting on stdin (stream-json): 2.5 s** (haiku 1.9 s).
//! A spare started as the ask's prompt opened hid that behind typing; it went with audit Q2 (2026-10-26): a
//! request starts its own process at Enter and writes its message. One process per request — so contexts don't
//! mix.
//!
//! A commit draft (`draft`): one request → its answer streamed into a preview card (`--include-partial-messages`)
//! → the in-buffer review (y/n per change) → applied as one transaction. Cancel = kill the process. `space i` is a
//! one-turn claude of its own with tarae's `read` and `edit` (`ask.rs`), whose waiting card and review are these.

use std::path::Path;
use std::time::Instant;

use ropey::Rope;

use crate::document::DocId;
use crate::editor::Editor;
use crate::key::{Code, Key};
use crate::transaction::{Change, Transaction};
use crate::workspace::Here;

use crate::agent::claude::DEFAULT_ARGS;
pub use crate::agent::claude::{
    Lines, SpawnError, command, control_response, initialize_message, interrupt_message, kill, pump, spawn,
    user_content, user_message, with_flag, with_tools,
};
#[cfg(test)]
pub(crate) use crate::agent::claude::{parse_line, usage_line};
pub use crate::agent::{ModelInfo, StreamEv};

/// `llm.sandbox-hosts`: with the network on, the hosts Claude's commands reach without asking (piece C1b).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Hosts {
    /// Every built-in pack's registries, whatever the project's languages (`"registries"`, the default).
    #[default]
    Registries,
    /// Any host (`"*"`; `"any"` read the same — configs written before C1b).
    Any,
    /// That list alone — the registries aren't added (`[]`: every host asked).
    List(Vec<crate::sandbox::proxy::egress::Pattern>),
}

impl Hosts {
    /// The rules a run's proxy holds — None: any host.
    pub fn rules(&self) -> Option<Vec<crate::sandbox::proxy::egress::Pattern>> {
        match self {
            Hosts::Any => None,
            Hosts::List(l) => Some(l.clone()),
            Hosts::Registries => {
                let mut v: Vec<crate::sandbox::proxy::egress::Pattern> = Vec::new();
                for (_, p) in &crate::sandboxpack::get().registries {
                    if !v.contains(p) {
                        v.push(p.clone());
                    }
                }
                Some(v)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub command: String,
    pub args: Vec<String>,
    pub model: Option<String>,
    /// `--effort` ("" = the model's default).
    pub effort: String,
    /// Chat: the editor follows where Claude is reading (`C-f` in the chat toggles it for that chat).
    pub follow: bool,
    /// Chat: seconds after your last key that follow takes over again while Claude works (0 = only by your key:
    /// `C-f` in the chat, `space F` in the code — `follow::again_key`).
    pub follow_resume: u64,
    /// Chat: Claude's edits go in as it makes them, the turn reviewed when it ends — every new chat starts
    /// so (`space v a` turns it on and off, and keeps it here).
    pub review_at_end: bool,
    /// Chat: load CLAUDE.md files and Claude Code settings like `claude` does (hooks off, edits always ask).
    pub chat_settings: bool,
    /// Keep every card asked about Claude's work, and every run of its refused with no card, in the record
    /// (`cards.rs`: `$state/cards/`). Only your own config sets it (`trust::YOURS_ONLY`).
    pub card_record: bool,
    /// Auto cards (`autocards.rs`): "shadow" — a model reads each of Claude's cards and its verdict is kept beside your
    /// answer, answering nothing. Only your own config sets it (`trust::YOURS_ONLY`).
    pub auto_cards: crate::autocards::Mode,
    /// How much a project's kept turns may take on disk, in MB (`checkpoint.rs`: past it the oldest go). Only your
    /// own config sets it (`trust::YOURS_ONLY`).
    pub turns_max_mb: u64,
    /// An `ANTHROPIC_API_KEY`/`ANTHROPIC_AUTH_TOKEN` in tarae's environment goes on to claude (`llm.environment-key
    /// = "keep"`) — but inside a Claude Code session, where it's the host's (`claude_env`).
    pub keep_env_key: bool,
    /// Who pays for a new session's Claude (`llm.billing` — `billing.rs`); a session keeps its own.
    pub billing: crate::billing::Billing,
    /// Who pays for the reviewer's (`llm.reviewer-billing`): None — as its session.
    pub reviewer_billing: Option<crate::billing::Billing>,
    /// Chat: commands Claude's `run` starts without asking, past the built-in checks (`tools::run_allow`) —
    /// each a prefix of whole words (`"npm test"`).
    pub run_allow: Vec<String>,
    /// Chat, Docker's card (#139): what its `a` lets vary of a command whose code tarae pins (`tools::Inert`) —
    /// `"cargo xtask linux: clippy test --userns --bin= --"`. A project's only once trusted (`trust::RISKY`).
    pub docker_inert: Vec<String>,
    /// Chat, review at the end (`edits.rs`): more files whose edits still ask first, past the protected ones
    /// (`sandbox::protected_names`, which always ask) — a name matches anywhere, a path (with `/`) from the
    /// project's root.
    pub review_ask: Vec<String>,
    /// Chat: Claude's commands run in a sandbox (`sandbox.rs`) — writes in the project, temp and the
    /// toolchain's folders; no network but this machine.
    pub sandbox: bool,
    pub sandbox_network: bool,
    /// With the network on, the hosts reached without asking (`llm.sandbox-hosts`); another is held and asked on a
    /// card (`hosts.rs`).
    pub sandbox_hosts: Hosts,
    /// With the network on, whether tarae's proxy goes on through the proxy tarae's own environment names
    /// (`HTTPS_PROXY`…, `NO_PROXY` — `proxy::upstream`), or straight out.
    pub sandbox_upstream: crate::sandbox::proxy::upstream::Setting,
    /// More folders the sandbox may write to (`~/` for home).
    pub sandbox_write: Vec<String>,
    /// PEM files whose certificates a run's trust bundle holds besides the system's and the CA variables'
    /// (`sandbox::cabundle`, #419) — and tarae's own check of an https:// upstream. Only your own config sets it
    /// (`trust::YOURS_ONLY`): it decides whom runs trust.
    pub sandbox_ca: Vec<String>,
    /// Variables a run gets masked (`llm.sandbox-secrets`, P2 — `sandbox::proxy::masked`): name → the host rules its
    /// real value goes to. Only your own config sets it (`trust::YOURS_ONLY`): where a secret goes is a capability.
    pub sandbox_secrets: std::collections::BTreeMap<String, Vec<String>>,
    /// Of `sandbox_secrets`, the names a card's masked yes put there for a run (`Grant::Masked` — `tools::with_allowed`,
    /// P2b), never a setting the user writes: swapped in only in an `Authorization` header (`proxy::masked`), said as
    /// its card's in the run's report.
    pub sandbox_secrets_card: std::collections::BTreeSet<String>,
    /// Paths in the project the sandbox keeps as they are, besides the names it always keeps
    /// (`sandbox::protected_names`).
    pub sandbox_protect: Vec<String>,
    /// Ports on this machine the sandbox lets Claude's commands connect to (none else, the network open or not).
    pub sandbox_ports: Vec<u16>,
    /// Every port on this machine open to them — the risky ones too (Seatbelt can't leave some out), and on
    /// macOS other runs' ports to tarae's proxy.
    pub sandbox_localhost: bool,
    /// More of tarae's environment their commands get (`sandbox::runenv`): names, or `PREFIX*` families — a
    /// token-like one only by its whole name.
    pub sandbox_env: Vec<String>,
    /// Days a kept run home (a toolchain's home of the runs', kept across them) stays with no run using it before
    /// tarae's start removes it (`runhomes.rs`).
    pub sandbox_kept_days: u64,
    /// Chat: the screen sent (`C-s`) and the terminal's quoted rows show the terminal panel's contents — off, its
    /// rows go blank and aren't quoted.
    pub screen_terminal: bool,
    /// Chat: minutes a run of Claude's prints nothing at next to no CPU before a card asks whether it's
    /// stuck (`stuck.rs`; 0 = never).
    pub stuck_after: u64,
    /// …and one whose work may go on elsewhere — granted a daemon's socket, run outside the sandbox, connected
    /// outside its processes — with no time limit of its own: minutes before that card (cardburden 5; 0 = as
    /// `stuck_after`).
    pub stuck_after_outside: u64,
    /// Whose chat gets the session tools (`orchestra.rs`): `any` · `home` · `off`.
    pub session_tools: String,
    /// How many sessions one parent's Claude may have open at once (`orchestra.rs`).
    pub children_max: usize,
    /// How many sessions every Claude together may have started and open at once (`orchestra.rs`).
    pub children_total: usize,
    /// How many turns of its own a parent's Claude may be woken for in an hour (`orchestra::wake`: a turn it started in
    /// a child, asked with `wake`, ended).
    pub parent_wakes: usize,
    /// Chat: Claude has `web_search` (`web::search`) — fixed for a chat's process, as the session tools are. On by
    /// default (a chat's first search asks on its card); only your own config turns it off.
    pub web_search: bool,
    /// The model its search runs on (`--model` of the helper `web::search` starts). Only your own config sets it.
    pub web_search_model: String,
    /// Chat: Claude has `code` (`codetool::tool`) — fixed for a chat's process, as `web_search` is. Off until its A/B.
    pub code: bool,
    /// `web.signed-in`: your wikis, each with the variable holding its token (`web::signed`) — here, beside what every
    /// claude is made from, so no claude tarae starts can miss leaving the variable out (`command`). Only your own
    /// config sets it.
    pub signed_in: Vec<crate::web::signed::Wiki>,
    /// `llm.cost`: money on the screen unasked — the chat's usage row's cost and the plan's weekly window, the board's
    /// estimate on its card and its start. Off (a shared screen, a recording): none of it; `:usage`, `/cost` and a
    /// board's report, which you ask for, still say it.
    pub cost: bool,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            command: "claude".into(),
            args: DEFAULT_ARGS.iter().map(|s| s.to_string()).collect(),
            model: None,
            effort: String::new(),
            follow: true,
            follow_resume: 20,
            review_at_end: false,
            chat_settings: true,
            card_record: true,
            auto_cards: crate::autocards::Mode::Off,
            turns_max_mb: 1024,
            keep_env_key: false,
            billing: Default::default(),
            reviewer_billing: None,
            run_allow: Vec::new(),
            docker_inert: Vec::new(),
            review_ask: Vec::new(),
            sandbox: true,
            sandbox_network: false,
            sandbox_hosts: Hosts::default(),
            sandbox_upstream: Default::default(),
            sandbox_write: Vec::new(),
            sandbox_ca: Vec::new(),
            sandbox_secrets: Default::default(),
            sandbox_secrets_card: Default::default(),
            sandbox_protect: Vec::new(),
            sandbox_ports: Vec::new(),
            sandbox_localhost: false,
            sandbox_env: Vec::new(),
            sandbox_kept_days: 14,
            screen_terminal: true,
            stuck_after: 10,
            stuck_after_outside: 30,
            session_tools: "any".into(),
            children_max: 3,
            children_total: 6,
            parent_wakes: 6,
            web_search: true,
            web_search_model: crate::web::search::MODEL.into(),
            code: false,
            signed_in: Vec::new(),
            cost: true,
        }
    }
}

/// What a failed start of `command` (`llm.command`) says. No `claude` at all: that Claude Code isn't installed,
/// and how to get it and log in — in its own docs' words (code.claude.com/docs/en/setup); `whole`: every line
/// (the chat), else one (a toast).
pub fn spawn_failed(command: &str, e: &std::io::Error, whole: bool) -> String {
    if e.kind() != std::io::ErrorKind::NotFound {
        return format!("{command}: {e}");
    }
    if Path::new(command).file_name().is_none_or(|n| n != "claude") {
        return match command.contains('/') {
            true => format!("{command} isn't there (llm.command)"),
            false => format!("{command} isn't on PATH (llm.command)"),
        };
    }
    if !whole {
        return "Claude Code isn't installed, or `claude` isn't on your PATH — install it with \
                `curl -fsSL https://claude.ai/install.sh | bash`, then log in by running `claude`"
            .into();
    }
    "Claude Code isn't installed, or `claude` isn't on your PATH. Install it (macOS, Linux, WSL):\n\
     curl -fsSL https://claude.ai/install.sh | bash\n\
     After installing, log in by running `claude` and following the browser prompts.\n\
     If `claude` still isn't found, ~/.local/bin isn't on your PATH yet — add it, and start tarae from a \
     new terminal.\n\
     https://code.claude.com/docs/en/setup"
        .into()
}

/// The way out of a hold, said wherever it is (`held`).
pub const HELD_WAY: &str = "fix it, then :config-reload";

/// Why no claude starts now, or None: a config file that doesn't read (friction #339). tarae runs on what's left of
/// the config then — its defaults maybe, and the default `llm.command` is the real claude on PATH: a typo would turn a
/// stand-in, another command or an API-billed setup into a live claude. So every start from the config holds alike —
/// the chat (a new session's, a continue), `space i`'s, a commit draft, the reviewer, `:claude-update` — until it reads
/// (`:config-reload`, or the watch's next load); one already running goes on. The file's own words, with its line.
pub fn held(ed: &Editor) -> Option<String> {
    (!ed.config_unread.is_empty())
        .then(|| format!("no claude until your config reads — {}: {HELD_WAY}", ed.config_unread.join("; ")))
}

/// A load's warnings as its error toast says them: and, while a file doesn't read, that no claude starts (`held`) — first,
/// so a toast cut at its third row still says it (a path takes a narrow one's rows).
pub fn config_said(ed: &Editor, warnings: &[String]) -> String {
    match ed.config_unread.is_empty() {
        true => format!("config: {}", warnings.join("; ")),
        false => format!("config: no claude until it reads — {}: {HELD_WAY}", warnings.join("; ")),
    }
}

// ── Claude's environment ────────────────────────────────────────────────
//
// The names a claude never gets, and their rules: `claudeenv.rs` (std only — auto cards' eval in xtask includes it,
// so its claude leaves out the same).

mod claudeenv;
#[cfg(test)]
use claudeenv::NESTED_BY;
pub(crate) use claudeenv::judged;
use claudeenv::{CREDENTIALS, THE_HOSTS};
pub use claudeenv::{ClaudeEnv, claude_env};

/// Whether `name` is Claude Code's own — a login, a host's, a setting (`ANTHROPIC_*`, `CLAUDE*`): never a wiki's
/// `token-env` (`web::signed::wikis_of`), which `command` would strip from every claude and send to the wiki.
pub fn claudes_own(name: &str) -> bool {
    CREDENTIALS.contains(&name)
        || THE_HOSTS.contains(&name)
        || name.starts_with("ANTHROPIC_")
        || name.starts_with("CLAUDE")
}

/// A stand-in for tarae's environment, by name (`Editor::claude_env`).
pub type Env = std::collections::HashMap<String, String>;

/// Variable `name` as claude gets it: `given`'s (`Editor::claude_env`), else tarae's own — never in a test build,
/// where no stand-in is an empty environment (tests never read the real one).
pub fn env_var(given: Option<&Env>, name: &str) -> Option<String> {
    match given {
        Some(g) => g.get(name).cloned(),
        None if cfg!(test) => None,
        None => std::env::var(name).ok(),
    }
}

/// The process's environment's names (a name that isn't UTF-8 is none of the rules').
pub(crate) fn process_names() -> Vec<String> {
    std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()).collect()
}

impl ClaudeEnv {
    /// The rules over tarae's environment — the process's, or `given`, a test's stand-in (`Editor::claude_env`),
    /// alone: what's left out, what's said and what the key card offers never depend on where the tests run, and
    /// what claude inherits of the process's names the rules judge goes (`command`), so a stand-in is the whole of
    /// it there too.
    pub fn of(given: Option<&Env>, keep_keys: bool) -> ClaudeEnv {
        let names = given.map_or_else(process_names, |g| g.keys().cloned().collect());
        claude_env(names.iter().map(String::as_str), keep_keys)
    }

    /// The keys left out (`CREDENTIALS`).
    pub(crate) fn keys(&self) -> Vec<&str> {
        self.removed.iter().map(String::as_str).filter(|n| CREDENTIALS.contains(n)).collect()
    }

    /// Why a key left out stays out, and what passes it: the setting — or, inside a Claude Code session, nothing.
    fn keys_why(&self, keys: &[&str]) -> String {
        let (it, its) = if keys.len() == 1 { ("it", "it's") } else { ("them", "they're") };
        match self.nested {
            true => {
                format!("inside a Claude Code session {its} the host's, whatever llm.environment-key says")
            }
            false => format!("llm.environment-key = \"keep\" in your config passes {it}"),
        }
    }

    /// What the chat says of it, names only — None: nothing left out.
    pub fn said(&self) -> Option<String> {
        let keys = self.keys();
        let rest: Vec<&str> = self.removed.iter().map(String::as_str).filter(|n| !keys.contains(n)).collect();
        let mut said = Vec::new();
        if !rest.is_empty() {
            let whose = match self.nested {
                true => "tarae runs inside Claude Code — that session's variables aren't passed to Claude",
                false => "Variables Claude Code sets for the processes it starts aren't passed to Claude",
            };
            said.push(format!("{whose}: {}.", rest.join(", ")));
        }
        if !keys.is_empty() {
            let verb = if keys.len() == 1 { "isn't" } else { "aren't" };
            let to = if rest.is_empty() { "to Claude" } else { "either" };
            said.push(format!(
                "{} {verb} passed {to}: Claude uses your Claude Code login ({}).",
                keys.join(" and "),
                self.keys_why(&keys)
            ));
        }
        if said.is_empty() {
            return None;
        }
        if !self.passed.is_empty() {
            said.push(format!("Passed as they are: {}.", self.passed.join(", ")));
        }
        Some(said.join(" "))
    }
}

/// Whether the session in front's chat passes a key from tarae's environment on: `llm.environment-key = "keep"`,
/// or `y` on the key card until the session ends (`envkey.rs`).
pub fn keeps_key(ed: &Editor) -> bool {
    ed.config.llm.keep_env_key || ed.workspace.claude.env_key == Some(true)
}

/// What claude's environment leaves out, said once since tarae started: one log line, and the chat's note — None:
/// nothing left out, or said already.
pub fn env_told(ed: &mut Editor) -> Option<String> {
    if ed.claude_env_told {
        return None;
    }
    ed.claude_env_told = true;
    let said = ClaudeEnv::of(ed.claude_env.as_ref(), keeps_key(ed)).said()?;
    crate::log::line("claude", "info", &said);
    Some(said)
}

/// Claude Code's words when it has no login (its errors reference, under `-p`: "Not logged in · Please run /login",
/// "Failed to authenticate…", "Please run /login · API Error: 401…").
pub const NOT_LOGGED_IN: &[&str] = &["Not logged in", "Please run /login", "Failed to authenticate"];

/// What a claude that failed with `error` for want of a login is told when tarae left a key out — None: not a
/// login failure, or none left out (claude's own words say `/login`, as your own claude's do). With the key card
/// up for it (`envkey::offered`), one line pointing there.
pub fn login_hint(ed: &mut Editor, error: &str) -> Option<String> {
    if !NOT_LOGGED_IN.iter().any(|l| error.contains(l)) {
        return None;
    }
    let env = ClaudeEnv::of(ed.claude_env.as_ref(), keeps_key(ed));
    let keys = env.keys();
    if keys.is_empty() {
        return None;
    }
    // (the key card says the rest — `envkey.rs`: the chat only points at it)
    if crate::envkey::offered(ed, &env) {
        return Some(format!(
            "A card asks whether Claude takes {} from tarae's environment.",
            keys.join(" and ")
        ));
    }
    Some(format!(
        "Claude has no login of its own, and tarae didn't pass on {} from its environment ({}). Or log in: run \
         `claude` in a terminal, then /login.",
        keys.join(" and "),
        env.keys_why(&keys)
    ))
}

// ── Prompt·response ─────────────────────────────────────────────────────

/// A draft's answer as the text it is: the one answer, a code fence around it taken off.
fn answer_of(out: &str) -> String {
    strip_fences(out.trim_matches('\n')).to_string()
}

fn strip_fences(s: &str) -> &str {
    let Some(rest) = s.strip_prefix("```") else { return s };
    let rest = rest.split_once('\n').map_or("", |(_, r)| r);
    rest.strip_suffix("```").map(|r| r.strip_suffix('\n').unwrap_or(r)).unwrap_or(rest)
}

/// If the original ended with a newline (line-wise selection), end the answer with one; otherwise strip it.
pub fn match_trailing_newline(old: &str, new: String) -> String {
    match (old.ends_with('\n'), new.ends_with('\n')) {
        (true, false) => new + "\n",
        (false, true) => new.trim_end_matches('\n').to_string(),
        _ => new,
    }
}

/// Line-level diff → (' ' | '-' | '+', line).
pub fn line_diff<'a>(old: &'a str, new: &'a str) -> Vec<(char, &'a str)> {
    use imara_diff::intern::InternedInput;
    use imara_diff::{Algorithm, diff};
    // imara's `&str` tokens = `str::lines` (terminator excluded) — indexes line up
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let input = InternedInput::new(old, new);
    let mut out = Vec::with_capacity(a.len().max(b.len()));
    let mut next = 0; // first old line not yet emitted
    diff(Algorithm::Histogram, &input, |before: std::ops::Range<u32>, after: std::ops::Range<u32>| {
        let (bs, be) = (before.start as usize, before.end as usize);
        out.extend(a[next..bs].iter().map(|l| (' ', *l)));
        out.extend(a[bs..be].iter().map(|l| ('-', *l)));
        out.extend(b[after.start as usize..after.end as usize].iter().map(|l| ('+', *l)));
        next = be;
    });
    out.extend(a[next..].iter().map(|l| (' ', *l)));
    out
}

// ── Review state ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Undecided,
    Accept,
    Reject,
}

pub struct Hunk {
    pub from: usize,
    pub to: usize,
    pub old: String,
    pub new: String,
    pub decision: Decision,
    /// The original changed while waiting and can't be located — cannot apply.
    pub conflict: bool,
}

pub struct Review {
    /// Its document's doc comments laid out folded, for a version and width (`term::review_folds`) — the review
    /// shows the whole file, and folding all of it each frame took milliseconds on a big one.
    pub folds: Option<crate::doccomment::Folds>,
    /// Its rows, folded, while what they're built from holds (`term::review_rows`).
    pub kept: Option<crate::codeview::Kept>,
    pub generation: u64,
    pub doc_id: DocId,
    pub instruction: String,
    pub hunks: Vec<Hunk>,
    pub ready: bool,
    pub current: usize,
    pub scroll: usize,
    /// Columns scrolled sideways (`h`/`l`) — lines not wrapped: a long one was cut at the edge, readable only
    /// with the chat panel closed.
    pub left: usize,
    /// Whether the view follows the current change (off when scrolling directly with j/k).
    pub follow: bool,
    pub started: Instant,
    /// When what it reviews was asked for (a chat edit waits in its queue first) — its place in the inbox.
    pub asked: Instant,
    /// Streamed answer (preview before it's done) · whether thinking.
    pub streamed: String,
    pub thinking: bool,
    /// What to call when done (applied?) — a chat edit or a change set waiting for its answer.
    pub on_done: Option<OnDone>,
    /// Why, in Claude's words: what it thought or said just before it asked for this edit (the chat's — a
    /// row or two under the title, the rest is in the chat).
    pub reason: String,
    /// What the change warns of (`cardwarn.rs`, a fact of the edit's — `edit-protected`: a file that runs later
    /// outside the sandbox): a row each under the title, in the warning color, saying whose.
    pub warnings: Vec<crate::cardwarn::Warned>,
    /// Accepted, it's a step of its own for `space u` (`space i`'s) — a chat edit is part of its turn instead
    /// (`checkpoint.rs`).
    pub own_turn: bool,
    /// The answer's lines wrapped at this width when it lands (a commit message's body at 72 — `wrap_message`).
    pub wrap: Option<usize>,
    /// Focused, it took the keys from the chat: Esc and its last answer give them back there — else to the code
    /// (#118). Set as it's focused, never as it comes.
    pub from_chat: bool,
    /// It has the keys (#528): every key is its but `space`, `:`, `C-w` and a sequence under way. Not focused, it
    /// waits, as a card that came on its own does — no key is its but the way in (`C-q`, a press on it, `C-w w`'s
    /// last stop: `review_focus`), so no stray key is ever an answer. Lost once the keys leave the code
    /// (`review_keys_sync`); Esc gives them back.
    pub focused: bool,
    /// Waiting, folded to its one-row peek: the keys were on the code as it came, or came onto it since — a waiting
    /// review never takes the place of what your keys are on. Never unfolds by itself: focusing it does.
    pub folded: bool,
    /// A frame drew it in the code area since it came or last folded (`term::render_frame`): focused before that —
    /// `C-q` on its peek, or with the code hidden on a narrow screen — an answer typed on is refused, said on it, as a
    /// card's typed before a frame drew it (a `C-q y` never answers a change no frame showed).
    pub seen: bool,
    /// Handed the keys by your answer on the one before (the next file of a set, the file after its card's answer —
    /// as `card_next` hands a card on): focused once a frame has drawn it, its answers refused until then.
    pub handed: bool,
    /// A draft's: the input that asked for it (`Events::asked` as it started — `C`, `:commit-draft`, the end card's
    /// `c`): focused as it lands while that input is still the last one, else it waits (#528, as `space i`'s).
    pub asked_by: Option<u64>,
    /// An answer of its refused (typed before it was in sight), said on its change's head until the next key.
    pub refused: Option<String>,
    /// What a mouse drag selected over it — the file's lines and the ones a change proposes alike, by its rows (the
    /// row, the display column): from the press to the pointer, copied as the button is let go.
    pub marked: Option<((usize, usize), (usize, usize))>,
    /// Where a press on it was (its row, the display column): a drag from there selects (`marked`).
    pub pressed: Option<(usize, usize)>,
    /// Claude's call it answers (its id, `chat::claim_call`): that call's row takes the keys that answered it
    /// (`chat::stamp_call`) — None for your own changes, `space i`'s, or a call not found.
    pub answers: Option<String>,
}

/// Called when the review ends (applied?).
pub type OnDone = Box<dyn FnOnce(&mut Editor, bool)>;

/// Closes the open review as rejected (before another review takes its place — so the waiter gets an answer).
/// An answer still being waited for is killed (its result would no longer find its review) — `space i`'s ends.
pub fn close_review(ed: &mut Editor) {
    let Some(mut rev) = ed.workspace.claude.review.take() else { return };
    if !rev.ready {
        if let Some(pid) = ed.llm_pid.take() {
            kill(pid);
        }
        crate::ask::closed(ed, rev.generation);
    }
    crate::reviews::edit_ended(ed, &rev, true, false);
    if let Some(cb) = rev.on_done.take() {
        cb(ed, false);
    }
}

impl Review {
    /// Waiting for an answer (`ready: false`), proposed by Claude, nobody to call when done.
    pub(crate) fn new(generation: u64, doc_id: DocId, instruction: &str, hunks: Vec<Hunk>) -> Self {
        Review {
            generation,
            folds: None,
            kept: None,
            doc_id,
            instruction: instruction.to_string(),
            hunks,
            ready: false,
            current: 0,
            scroll: 0,
            left: 0,
            follow: true,
            started: Instant::now(),
            asked: Instant::now(),
            streamed: String::new(),
            thinking: false,
            on_done: None,
            reason: String::new(),
            warnings: Vec::new(),
            own_turn: true,
            wrap: None,
            from_chat: false,
            focused: false,
            folded: false,
            seen: false,
            handed: false,
            asked_by: None,
            refused: None,
            answers: None,
            marked: None,
            pressed: None,
        }
    }

    /// It's drawn in the code area (focused, or waiting unfolded) — not its peek.
    pub fn covers(&self) -> bool {
        self.ready && !self.folded
    }

    /// Folded to its peek: what it changes is out of sight until a frame draws it again (`seen`).
    pub fn fold(&mut self) {
        (self.folded, self.seen) = (true, false);
    }

    /// It waits to be focused (`C-q`, a press, `C-w w`): up, its answer ready, without the keys.
    pub fn waits(&self) -> bool {
        self.ready && !self.focused
    }

    fn next_undecided(&self) -> Option<usize> {
        let n = self.hunks.len();
        (1..=n)
            .map(|d| (self.current + d) % n)
            .find(|&i| self.hunks[i].decision == Decision::Undecided && !self.hunks[i].conflict)
    }

    /// Rows of the in-buffer review (like Cursor): shows the whole document, expanding only the changes —
    /// unwrapped, unfolded (`term::render_frame` folds them, `draw_review` wraps the ones on screen).
    /// Undecided = both removed and added lines, accepted = new text only, rejected = original only.
    pub fn rows(&self, text: &Rope) -> Vec<crate::codeview::Row> {
        use crate::codeview::{Edit, Mark, OtherKind, Row};
        // (the empty line after a final newline isn't one: the drawer shows it only with the cursor there)
        let last = crate::movement::last_line(text);
        let line_of = |b: usize| text.byte_to_line(b.min(text.len_bytes()));
        let doc_row = |line| Row::Text { line, part: None, edit: None };
        let mut order: Vec<usize> = (0..self.hunks.len()).collect();
        crate::sorting::by_key(&mut order, |&i| self.hunks[i].from);
        let mut out = Vec::new();
        let mut next = 0; // first document line not yet emitted
        for i in order {
            let h = &self.hunks[i];
            let lf = line_of(h.from);
            let lt = if h.to > h.from { line_of(h.to - 1) } else { lf };
            out.extend((next..lf.max(next)).map(doc_row));
            out.push(Row::Header { hunk: i, at: lf });
            next = next.max(lt + 1);
            if h.conflict {
                out.extend((lf..=lt).map(doc_row));
                continue;
            }
            // The lines it takes out or keeps are the document's own rows while it holds what the change replaces;
            // edited since (the editor's keys work while the review waits — #61), the text it was asked on
            let holds = text.get_byte_slice(h.from..h.to).is_some_and(|s| s == h.old.as_str());
            // Changes line-wise: attach same-line pieces around the selection so whole lines are compared
            let ls = text.line_to_byte(lf);
            let le = text.line_to_byte((lt + 1).min(last + 1)).max(h.to);
            let (pre, post) =
                (text.byte_slice(ls..h.from.max(ls)).to_string(), text.byte_slice(h.to..le).to_string());
            let old = format!("{pre}{}{post}", h.old);
            let new = format!("{pre}{}{post}", h.new);
            // (the document line the next old line is)
            let mut old_no = lf;
            for (kind, line) in line_diff(&old, &new) {
                let mark = match (h.decision, kind) {
                    (Decision::Accept, '-') => {
                        old_no += 1;
                        continue;
                    }
                    (Decision::Reject, '+') => continue,
                    (_, '+') => Mark::Added { washed: h.decision == Decision::Undecided },
                    (Decision::Reject, '-') | (_, ' ') => Mark::Kept,
                    _ => Mark::Taken,
                };
                let edit = Edit { hunk: i, mark };
                let other =
                    |at| Row::Other { at, text: line.to_string(), kind: OtherKind::Edit(edit), part: None };
                out.push(match mark {
                    Mark::Added { .. } => other(old_no),
                    _ if holds => Row::Text { line: old_no, part: None, edit: Some(edit) },
                    _ => other(old_no),
                });
                if !matches!(mark, Mark::Added { .. }) {
                    old_no += 1;
                }
            }
        }
        out.extend((next..=last).map(doc_row));
        out
    }
}

// ── Commands ────────────────────────────────────────────────────────────

/// Claude writes what `from..to` of document `doc_id` should be, from `prompt` alone (one answer, no tools) —
/// streamed, then the in-buffer review (a commit message's draft).
pub fn draft(
    ed: &mut Editor,
    doc_id: DocId,
    label: &str,
    prompt: &str,
    from: usize,
    to: usize,
) -> Result<(), String> {
    if ed.workspace.claude.review.is_some() {
        return Err("Claude is already answering (:ask-cancel)".into());
    }
    let doc = ed.docs.iter().find(|d| d.id == doc_id).ok_or("the buffer is gone")?;
    if from > to || to > doc.text.len_bytes() {
        return Err("the buffer changed — draft again".into());
    }
    let old = doc.text.byte_slice(from..to).to_string();
    let hunk = Hunk { from, to, old, new: String::new(), decision: Decision::Undecided, conflict: false };
    start(ed, doc_id, label, prompt, hunk)?;
    let asked = ed.events.asked();
    if let Some(r) = ed.workspace.claude.review.as_mut() {
        r.asked_by = asked;
    }
    // (its time limit: a draft still being written then is stopped, and said)
    let generation = ed.llm_generation;
    ed.events.jobs().spawn(move || {
        crate::event::sleep(DRAFT_LIMIT);
        move |ed: &mut Editor| stop_waiting(ed, Some(generation), DRAFT_LIMIT_SAID)
    });
    Ok(())
}

/// The longest a draft is waited for (a commit message takes seconds): one still being written then is
/// stopped — one ran 6 minutes, its output going on, nothing on screen to stop it with.
pub const DRAFT_LIMIT: std::time::Duration = std::time::Duration::from_secs(120);
const DRAFT_LIMIT_SAID: &str = "still writing after 2 minutes — stopped, nothing changed";

/// Whether Esc now stops the answer being waited for (a draft, `space i`) — the code has the keys in normal
/// mode (`review_key`; the waiting card says `esc stop` then).
pub fn esc_stops(ed: &Editor) -> bool {
    ed.workspace.claude.review.as_ref().is_some_and(|r| !r.ready)
        && ed.mode == crate::editor::Mode::Normal
        && ed.picker.is_none()
        && ed.prompt.is_none()
        && ed.pending.is_empty()
        && !ed.workspace.chat_has_keys()
}

/// The answer being waited for (of `generation`, or any) is stopped — its process killed, nothing applied —
/// and `said` says why. Nothing waiting (it answered first): nothing happens.
pub fn stop_waiting(ed: &mut Editor, generation: Option<u64>, said: &str) {
    let waiting = ed
        .workspace
        .claude
        .review
        .as_ref()
        .filter(|r| !r.ready && generation.is_none_or(|g| r.generation == g));
    let Some(what) = waiting.map(|r| r.instruction.clone()) else {
        ed.unchanged = true;
        return;
    };
    if let Some(pid) = ed.llm_pid.take() {
        kill(pid);
    }
    close_review(ed);
    ed.llm_generation += 1;
    ed.set_warning(format!("Claude's {what}: {said}"));
}

/// A draft's answer for `hunk` of `doc_id`: the process, the review waiting for it, the stream into it — a turn
/// named `draft` in `:usage` and the call-order record.
fn start(ed: &mut Editor, doc_id: DocId, instruction: &str, prompt: &str, hunk: Hunk) -> Result<(), String> {
    if let Some(why) = held(ed) {
        return Err(why);
    }
    let what = "draft";
    let cfg = ed.config.llm.clone();
    let cred = ed.credential(ed.workspace.claude.billing, crate::billing::Role::Ask)?;
    // (its own process, started now: posix_spawn returns at once, claude's start is the child's)
    let mut child = spawn(&cfg, &[], &ed.workspace.root, ed.claude_env.as_ref(), &cred)
        .map_err(|e| e.said(&cfg.command, false))?;
    let (billing, keep) = (cred.billing(), cfg.keep_env_key);
    // Write the message and close stdin — the process exits on its own after this one turn (written off the
    // main loop: a commit draft's prompt can be 80 KB, and claude still starting reads nothing yet).
    // Before anything is installed: a process without stdin leaves no review waiting forever.
    let sent =
        child.stdin.take().ok_or_else(|| "claude: no stdin".to_string()).and_then(|stdin| {
            Lines::new(stdin).send(user_message(prompt)).map_err(|e| format!("claude: {e}"))
        });
    if let Err(e) = sent {
        // Reaped off the main thread
        crate::event::thread("tarae-reap", move || {
            let _ = child.kill();
            let _ = child.wait();
        });
        return Err(e);
    }
    ed.llm_generation += 1;
    let generation = ed.llm_generation;
    ed.workspace.claude.review = Some(Review::new(generation, doc_id, instruction, vec![hunk]));
    ed.llm_pid = Some(child.id());
    pump(child, true, ed.events.sender(), move |ev| {
        Some(match ev {
            StreamEv::Thinking | StreamEv::Thought(_) => Box::new(move |ed: &mut Editor| {
                if let Some(r) = ed.workspace.claude.review.as_mut().filter(|r| r.generation == generation) {
                    r.thinking = true;
                }
            }),
            StreamEv::Text(t) => Box::new(move |ed: &mut Editor| {
                if let Some(r) = ed.workspace.claude.review.as_mut().filter(|r| r.generation == generation) {
                    r.thinking = false;
                    r.streamed.push_str(&t);
                }
            }),
            StreamEv::Done(r) => {
                let result = r.map(|out| vec![answer_of(&out)]);
                Box::new(move |ed: &mut Editor| on_result(ed, generation, result))
            }
            // (what it cost is the session's too — a turn of its own, beside the chat's conversation; a
            // result that reports nothing has nothing to add)
            StreamEv::Spent(s) if !s.counts.any() && s.total_cost.is_none() => return None,
            StreamEv::Spent(s) => Box::new(move |ed: &mut Editor| {
                ed.workspace.claude.usage.aside(s, what);
                ed.calls_record();
                crate::usage::view_sync(ed);
            }),
            // (the account's limits, whichever claude says them)
            StreamEv::Limits(r) => Box::new(move |ed: &mut Editor| crate::limits::seen(ed, r)),
            // (the credential it says it runs on — another than its billing's stops it: `billing.rs`)
            StreamEv::Conversation { key_source, .. } => {
                let role = crate::billing::Role::Ask;
                let said = crate::billing::checked(billing, keep, role, key_source.as_deref())?;
                Box::new(move |ed: &mut Editor| {
                    crate::log::line("claude", "warn", &said);
                    stop_waiting(ed, Some(generation), &said);
                })
            }
            // a draft runs without tools, and its conversation isn't kept
            StreamEv::Requesting(_)
            | StreamEv::Retry(_)
            | StreamEv::Money(_)
            | StreamEv::Usage(_)
            | StreamEv::Answered(_)
            | StreamEv::Initialized { .. }
            | StreamEv::Tool { .. }
            | StreamEv::Writing(_)
            | StreamEv::ThinkingBlock(_)
            | StreamEv::ToolResult { .. }
            | StreamEv::Searched(_)
            | StreamEv::Control { .. }
            | StreamEv::Compaction(_)
            | StreamEv::Exit(_) => {
                return None;
            }
        })
    });
    Ok(())
}

/// Cancel: if a request is pending, kill the process (stops tokens too); if reviewing, reject all.
pub fn cancel(ed: &mut Editor) {
    if let Some(pid) = ed.llm_pid.take() {
        kill(pid);
    }
    crate::ask::cancel(ed);
    if ed.workspace.claude.review.is_some() {
        close_review(ed);
        ed.llm_generation += 1;
        ed.set_status("claude: cancelled");
    }
}

/// Opens a review directly with changes in hand (chat code apply, chat edits, change sets — same y/n flow·one undo as
/// ask) + what to call when done (applied?).
pub fn review_with(
    ed: &mut Editor,
    doc_id: DocId,
    instruction: &str,
    changes: Vec<(usize, usize, String, String)>,
    on_done: Option<OnDone>,
) {
    close_review(ed);
    ed.llm_generation += 1;
    // (what was being written there is this review's now — `writing.rs`)
    crate::writing::reviewed(ed, doc_id);
    let hunks: Vec<Hunk> = changes
        .into_iter()
        .map(|(from, to, old, new)| Hunk {
            from,
            to,
            decision: if old == new { Decision::Reject } else { Decision::Undecided },
            old,
            new,
            conflict: false,
        })
        .collect();
    if hunks.iter().all(|h| h.decision == Decision::Reject) {
        if let Some(cb) = on_done {
            cb(ed, false);
        }
        return ed.set_status("claude: no changes");
    }
    let mut rev =
        Review { ready: true, on_done, ..Review::new(ed.llm_generation, doc_id, instruction, hunks) };
    // (on the first change still to answer — a no-op change before it is decided already, and `y` there
    // answered nothing: #216)
    rev.current = rev.hunks.iter().position(|h| h.decision == Decision::Undecided).unwrap_or(0);
    ed.workspace.claude.review = Some(rev);
    // (it comes waiting — the caller focuses it when your input asked for it: the keys on the code, its peek)
    review_keys_sync(ed);
}

/// The keys an edit review answers with (`review_answer`): refused while it was handed them but not drawn yet.
pub const ANSWERS: [char; 4] = ['y', 'n', 'a', 'q'];

/// The edit review up gets the keys (#528): `C-q`, a press on it or its peek, `C-w w`'s last stop, `enter` on its
/// inbox row, a frame drawing it handed on, your own input asking for it. From wherever they were — the chat
/// remembered, so its Esc and last answer give them back there. Its file comes to the front of the code it covers
/// (waiting folded, the pane kept the one your keys were on): answered, you're where its change landed.
pub fn review_focus(ed: &mut Editor) {
    let chat = ed.workspace.chat_has_keys();
    // (in sight if the last frame drew it in the code area — waiting there, read before `C-q`; no frame drawn yet: a
    // test's editor, nothing to judge by)
    let seen = ed.frames == 0 || ed.screen.review_area.get().is_some();
    let Some(r) = ed.workspace.claude.review.as_mut().filter(|r| r.ready && !r.focused) else { return };
    (r.focused, r.folded, r.handed, r.from_chat) = (true, false, false, chat);
    r.seen = seen;
    let doc = r.doc_id;
    if let Some(i) = ed.docs.iter().position(|d| d.id == doc) {
        ed.current = i;
    }
    ed.workspace.viewer.keys_to_code();
}

/// The edit review up was handed the keys by your answer on the one before (`Review.handed`): drawn in the code
/// area, focused by the frame that draws it (`term::render_frame`).
pub fn review_hand(ed: &mut Editor) {
    if let Some(r) = ed.workspace.claude.review.as_mut().filter(|r| r.ready && !r.focused) {
        (r.handed, r.folded) = (true, false);
    }
}

/// `C-q` (or its twin) while this session's edit review waits and is the next up: focused, from wherever the keys
/// are — the terminal's program too, so beside `card_key`, first in `handle_key` (the way in the same from every
/// surface). True if the key was its.
pub fn review_way_in(ed: &mut Editor, key: Key) -> bool {
    let c_q = key.ctrl && !key.alt && key.code == Code::Char('q');
    if !c_q || ed.picker.is_some() || ed.prompt.is_some() || !ed.pending.is_empty() {
        return false;
    }
    if !ed.workspace.claude.review.as_ref().is_some_and(Review::waits)
        || ed.next_up() != crate::inbox::Next::Review
    {
        return false;
    }
    review_focus(ed);
    true
}

/// The focused edit review lets go of the keys, waiting on unanswered (its `esc`, and `C-q` stepping on — #544): back
/// to the chat if they came from it (Some(true)), else to the code, where it folds to its peek (Some(false)). None:
/// it didn't have them.
pub fn review_let_go(ed: &mut Editor) -> Option<bool> {
    let rev = ed.workspace.claude.review.as_mut().filter(|r| r.ready && r.focused)?;
    rev.focused = false;
    let chat = rev.from_chat && ed.workspace.claude.chat.is_some();
    match chat {
        true => ed.workspace.set_at(Here::Chat),
        false => {
            // (the keys on the code: it folds to its peek, the code in sight)
            rev.fold();
            ed.mode = crate::editor::Mode::Normal;
        }
    }
    Some(chat)
}

/// The focused review's `esc`: the keys back where they came from (`review_let_go`), said with its way in. True.
fn review_back(ed: &mut Editor) -> bool {
    let to = if review_let_go(ed) == Some(true) { "chat" } else { "code" };
    let key = ed.way_in(crate::inbox::Waiting::Review).unwrap_or_else(|| ed.key_said("C-q"));
    ed.set_status(format!("back to the {to} — the change waits unanswered, {key} answers it"));
    true
}

/// Every event: the edit review's focus follows the keys. Focused, it lets go once they leave the code (a hop, a
/// press elsewhere); waiting, it folds to its peek once they're on the code — and stays folded.
pub fn review_keys_sync(ed: &mut Editor) {
    let here = ed.workspace.here();
    // (which-key, a picker, a prompt, a card over the code are the editor's own layers — the `space`, `:` and `C-w` a
    // focused review lets through: the keys come back to it once they go)
    let over_code = ed.workspace.docked() == Here::Code && ed.float_front().is_some();
    let code = here == Here::Code || over_code;
    // (the side panel and the review rail stand in the code's area: drawn over, the keys there would act unseen)
    let in_area = matches!(here, Here::Code | Here::Side | Here::Rail);
    let Some(r) = ed.workspace.claude.review.as_mut().filter(|r| r.ready) else { return };
    if r.focused && !code {
        r.focused = false;
    }
    if !r.focused && !r.handed && in_area && !r.folded {
        r.fold();
    }
}

/// A commit message with its body wrapped at `width` (git's convention, 72): the subject line stays as it
/// is; a longer body line breaks between words, a list item's continuation under its text; indented lines
/// (code), trailers (`Co-Authored-By: …`) and words longer than the width stay whole.
pub fn wrap_message(text: &str, width: usize) -> String {
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let trailer = line.split_once(": ").is_some_and(|(k, _)| !k.is_empty() && !k.contains(' '));
        if i == 0 || line.chars().count() <= width || line.starts_with([' ', '\t']) || trailer {
            out.push_str(line);
            continue;
        }
        // A list item's lines go on under its text
        let hang = ["- ", "* "].iter().find(|b| line.starts_with(**b)).map_or(0, |b| b.len());
        let mut row = String::new();
        for word in line.split(' ').filter(|w| !w.is_empty()) {
            if !row.is_empty() && row.chars().count() + 1 + word.chars().count() > width {
                out.push_str(&row);
                out.push('\n');
                row = " ".repeat(hang);
            } else if !row.is_empty() && !row.ends_with(' ') {
                row.push(' ');
            }
            row.push_str(word);
        }
        out.push_str(&row);
    }
    out
}

/// Preview of the streaming answer (a draft's, `space i`'s `new_string`): its last few lines.
pub fn preview(streamed: &str, rows: usize) -> Vec<&str> {
    let lines: Vec<&str> = streamed.trim_start_matches('\n').lines().collect();
    lines[lines.len().saturating_sub(rows)..].to_vec()
}

fn on_result(ed: &mut Editor, generation: u64, result: Result<Vec<String>, String>) {
    let Some(rev) = ed.workspace.claude.review.as_mut().filter(|r| r.generation == generation) else {
        return;
    };
    ed.llm_pid = None;
    let took = rev.started.elapsed();
    let replies = match result {
        Ok(r) => r,
        Err(e) => {
            ed.workspace.claude.review = None;
            ed.set_error(format!("claude: {e}"));
            return;
        }
    };
    let Some(doc_idx) = ed.docs.iter().position(|d| d.id == rev.doc_id) else {
        ed.workspace.claude.review = None;
        ed.set_error("claude: buffer was closed");
        return;
    };
    let text = &ed.docs[doc_idx].text;
    let wrap = rev.wrap;
    for (h, new) in rev.hunks.iter_mut().zip(replies) {
        let new = match wrap {
            Some(w) => wrap_message(&new, w),
            None => new,
        };
        h.new = match_trailing_newline(&h.old, new);
        if h.new == h.old {
            h.decision = Decision::Reject;
        } else if !relocate(h, text) {
            h.conflict = true;
        }
    }
    if rev.hunks.iter().all(|h| h.decision == Decision::Reject || h.conflict) {
        let msg = if rev.hunks.iter().any(|h| h.conflict) {
            "claude: text changed while waiting — nothing applied"
        } else {
            "claude: no changes"
        };
        ed.workspace.claude.review = None;
        ed.set_status(msg);
        return;
    }
    rev.ready = true;
    rev.current =
        rev.hunks.iter().position(|h| h.decision == Decision::Undecided && !h.conflict).unwrap_or(0);
    let yours = rev.asked_by.is_some() && rev.asked_by == Some(ed.input_seq);
    ed.current = doc_idx;
    ed.set_status(format!("claude answered in {}", crate::term::run_duration(took)));
    // (asked by your key, nothing typed since: focused as it lands — else it waits for `C-q`, #528)
    match yours {
        true => review_focus(ed),
        false => review_keys_sync(ed),
    }
}

/// If edits happened while waiting, relocates the original (nearest match). False if not found.
fn relocate(h: &mut Hunk, text: &Rope) -> bool {
    // Edits while waiting may leave the old position mid-character — check boundaries before slicing.
    let boundary = |b: usize| b <= text.len_bytes() && text.char_to_byte(text.byte_to_char(b)) == b;
    if boundary(h.from) && boundary(h.to) && h.from <= h.to && text.byte_slice(h.from..h.to) == h.old.as_str()
    {
        return true;
    }
    if h.old.is_empty() {
        return false;
    }
    let s = text.to_string();
    let best = s.match_indices(&h.old).map(|(b, _)| b).min_by_key(|&b| b.abs_diff(h.from));
    match best {
        Some(from) => {
            h.to = from + h.old.len();
            h.from = from;
            true
        }
        None => false,
    }
}

/// Keys during review. True if handled. While it waits, the editor's own keys still work over it (#61): a
/// picker or a prompt open, a key sequence under way (`space W`, `space m c`), `space`, `:` and `C-w` that
/// start one — and `C-d` `C-u` `C-f` `C-b` `C-e` `C-y` (page keys too) scroll it. Esc gives the keys back
/// where they were before it took them, the change unanswered (#70, #118): to the chat if it had them,
/// else to the code — `C-q` brings them back to it.
pub fn review_key(ed: &mut Editor, key: Key) -> bool {
    if ed.picker.is_some() || ed.prompt.is_some() || !ed.pending.is_empty() {
        return false;
    }
    let page = ed.viewport.0.max(2) as isize;
    // Still waiting for the answer: Esc stops it (the card says so)
    if key == Key::plain(Code::Esc) && esc_stops(ed) {
        stop_waiting(ed, None, "stopped — nothing changed");
        return true;
    }
    let Some(rev) = ed.workspace.claude.review.as_mut().filter(|r| r.ready) else { return false };
    // (a key of yours ends what the mouse selected over it — copied already, as the button was let go)
    rev.marked = None;
    rev.refused = None;
    // Waiting: no key is its — they go where the keys are (its way in is `review_way_in`'s, beside the cards'). Handed
    // the keys by an answer but not drawn yet: an answer typed on is refused, said on it (a `y y` never answers a
    // file never in sight)
    let answer = key.plain_char().is_some_and(|c| ANSWERS.contains(&c));
    // (focused but no frame has drawn it yet — `C-q y` on its peek: the same refusal)
    if (rev.handed || rev.focused && !rev.seen) && answer {
        rev.refused = Some(format!("{key} came before this change was in sight — not taken"));
        return true;
    }
    if !rev.focused {
        return false;
    }
    // `C-q` steps on to what waits next (#544: as a card's does); nothing else waiting, the keys back as `esc`
    if key.ctrl && !key.alt && key.code == Code::Char('q') {
        return ed.c_q_step() || review_back(ed);
    }
    let scroll = |rev: &mut Review, by: isize| {
        rev.scroll = rev.scroll.saturating_add_signed(by);
        rev.follow = false;
        false
    };
    let done = match (key.code, key.ctrl, key.alt) {
        (Code::Char(' ' | ':'), false, false) | (Code::Char('w'), true, false) => return false,
        (Code::Char('d'), true, false) => scroll(rev, page / 2),
        (Code::Char('u'), true, false) => scroll(rev, -page / 2),
        (Code::Char('f'), true, false) | (Code::PageDown, false, false) => scroll(rev, page),
        (Code::Char('b'), true, false) | (Code::PageUp, false, false) => scroll(rev, -page),
        (Code::Char('e'), true, false) => scroll(rev, 1),
        (Code::Char('y'), true, false) => scroll(rev, -1),
        (Code::Esc, ..) => return review_back(ed),
        _ => return review_answer(ed, key),
    };
    if done {
        finish(ed);
    }
    true
}

/// Columns `h`/`l` scroll a review sideways.
const SIDEWAYS: usize = 8;

/// The review's own keys: `y` `n` `a` answer, `q` ends it, `j` `k` scroll, `h` `l` sideways (lines not
/// wrapped), `tab` goes to the next change.
fn review_answer(ed: &mut Editor, key: Key) -> bool {
    let doc = ed.workspace.claude.review.as_ref().and_then(|r| ed.docs.iter().find(|d| d.id == r.doc_id));
    let wraps = doc.is_some_and(|d| ed.wraps(d));
    let Some(rev) = ed.workspace.claude.review.as_mut().filter(|r| r.ready) else { return false };
    // (the change answered: its head's stamp pressed once the review moves on — §5)
    let answered = crate::motion::Place::Head { doc: rev.doc_id, change: rev.current };
    let decide = |rev: &mut Review, d: Decision| {
        rev.hunks[rev.current].decision = d;
        rev.follow = true;
        match rev.next_undecided() {
            Some(i) => {
                rev.current = i;
                false
            }
            None => true,
        }
    };
    let done = match (key.code, key.ctrl || key.alt) {
        (Code::Char('y'), false) => decide(rev, Decision::Accept),
        (Code::Char('n'), false) => decide(rev, Decision::Reject),
        (Code::Char('a'), false) => {
            rev.hunks
                .iter_mut()
                .filter(|h| h.decision == Decision::Undecided)
                .for_each(|h| h.decision = Decision::Accept);
            true
        }
        (Code::Char('q'), false) => true,
        (Code::Char('j') | Code::Down, false) => {
            rev.scroll += 1;
            rev.follow = false;
            false
        }
        (Code::Char('k') | Code::Up, false) => {
            rev.scroll = rev.scroll.saturating_sub(1);
            rev.follow = false;
            false
        }
        // (the next change waiting; none but this one, the view back on it — already there, said: #264)
        (Code::Tab, _) => match rev.next_undecided().filter(|&i| i != rev.current) {
            None if rev.follow => {
                ed.key_unused(
                    key,
                    "the review of this change",
                    "no other change waits — y takes it, n leaves it",
                );
                return true;
            }
            next => {
                rev.current = next.unwrap_or(rev.current);
                rev.follow = true;
                false
            }
        },
        // Sideways, where lines aren't wrapped (`editor.soft-wrap`, `z w`): a long one read to its end
        (Code::Char('h') | Code::Left, false)
        | (Code::Char('l') | Code::Right, false)
        | (Code::Home, false)
            if !wraps =>
        {
            rev.left = match key.code {
                Code::Char('h') | Code::Left => rev.left.saturating_sub(SIDEWAYS),
                Code::Home => 0,
                _ => rev.left + SIDEWAYS,
            };
            false
        }
        _ => {
            let out = match wraps {
                true => "y takes it, n leaves it, a takes all, q ends — esc goes back, unanswered",
                false => {
                    "y takes it, n leaves it, a takes all, q ends, h/l scroll sideways — esc goes back, unanswered"
                }
            };
            ed.key_unused(key, "the review of this change", out);
            return true;
        }
    };
    let stamped = matches!((key.code, key.ctrl || key.alt), (Code::Char('y' | 'n'), false));
    if done {
        finish(ed);
    } else if stamped {
        ed.press(answered);
    }
    true
}

/// Accepted changes as one transaction (= one undo).
fn finish(ed: &mut Editor) {
    let Some(mut rev) = ed.workspace.claude.review.take() else { return };
    let on_done = rev.on_done.take();
    let before = ed.docs.iter().find(|d| d.id == rev.doc_id).map(|d| d.text.clone());
    // (the editor's keys work while it waits — #61: a change of the text since is found again, or skipped)
    if let Some(text) = &before {
        for h in rev.hunks.iter_mut().filter(|h| h.decision == Decision::Accept && !h.conflict) {
            h.conflict = !relocate(h, text);
        }
    }
    // (Claude's change — a chat's edit, its change set's file: its call's row takes the keys that answered it)
    if let Some(id) = rev.answers.as_deref() {
        let keys: Vec<_> = rev
            .hunks
            .iter()
            .filter(|h| h.old != h.new)
            .filter_map(|h| crate::stamp::Stamp::of(h.decision))
            .collect();
        crate::chat::stamp_call(ed, id, &keys);
    }
    let applied = apply_accepted(ed, &rev);
    crate::reviews::edit_ended(ed, &rev, false, applied);
    // Answered here, the keys go back to the chat it took them from — as its Esc gives them (they stayed in
    // the code: `keys → pane#29 (commands)` after an edit review's `y`)
    if rev.from_chat && rev.focused && ed.workspace.claude.chat.is_some() {
        ed.workspace.set_at(Here::Chat);
    }
    // A step `space u` takes back on its own
    if applied
        && rev.own_turn
        && let Some(before) = before
    {
        crate::checkpoint::single(ed, rev.doc_id, before, &rev.instruction);
    }
    if let Some(cb) = on_done {
        cb(ed, applied);
    }
}

fn apply_accepted(ed: &mut Editor, rev: &Review) -> bool {
    let accepted: Vec<&Hunk> =
        rev.hunks.iter().filter(|h| h.decision == Decision::Accept && !h.conflict).collect();
    if accepted.is_empty() {
        ed.set_status("claude: nothing applied");
        return false;
    }
    let Some(idx) = ed.docs.iter().position(|d| d.id == rev.doc_id) else { return false };
    ed.current = idx;
    let tx = Transaction::new(
        accepted.iter().map(|h| Change { from: h.from, to: h.to, insert: h.new.clone() }).collect(),
    );
    let n = accepted.len();
    ed.with_group(|cx| {
        let doc = cx.editor.doc_mut();
        // (your selection, carried through Claude's lines — never theirs selected: #307 left `2 sels` you
        // never made after a review answered y n y)
        doc.apply(&tx);
        // (on your `y`, Claude's lines — not your edit)
        doc.own.not_yours();
    });
    ed.set_status(format!("claude: applied {n} change(s) — u to undo"));
    true
}

#[cfg(test)]
mod held_tests;

#[cfg(test)]
pub(crate) mod tests {
    use std::process::Command;
    use std::time::Duration;

    use super::*;
    use crate::billing::Credential;

    // ── Claude's environment ──

    /// The `ANTHROPIC_*` / `CLAUDE*` / `AI_AGENT` / `MCP_*` names apibill's L0 found in tarae's environment —
    /// tarae started from a shell a desktop-app Claude Code session started.
    const L0: &[&str] = &[
        "AI_AGENT",
        "ANTHROPIC_BASE_URL",
        "CLAUDECODE",
        "CLAUDE_AGENT_SDK_VERSION",
        "CLAUDE_CODE_BRIDGE_MCP_CARRIER",
        "CLAUDE_CODE_BRIDGE_OWNER_ACCOUNT_UUID",
        "CLAUDE_CODE_BRIDGE_OWNER_ORG_UUID",
        "CLAUDE_CODE_CHILD_SESSION",
        "CLAUDE_CODE_DESKTOP_APP_VERSION",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_EXECPATH",
        "CLAUDE_CODE_HOST_SESSION_ID",
        "CLAUDE_CODE_OAUTH_SCOPES",
        "CLAUDE_CODE_SDK_HAS_HOST_AUTH_REFRESH",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_CODE_WORKER_EPOCH",
        "CLAUDE_EFFORT",
        "CLAUDE_PID",
        "MCP_CONNECTION_NONBLOCKING",
        "MCP_SERVER_CONNECTION_BATCH_SIZE",
    ];

    /// Inside a Claude Code session, everything of that session's goes — what it sets for its children, its
    /// host's own, an exported key — and what you may have set yourself stays: documented settings
    /// (`CLAUDE_CODE_MAX_OUTPUT_TOKENS`, `MCP_*`, `ANTHROPIC_BASE_URL`), and the rest of the environment. What stays
    /// under a Claude name is listed, so a host's next undocumented variable is seen.
    #[test]
    fn inside_claude_code_its_sessions_variables_go_and_your_settings_stay() {
        let yours = ["PATH", "CLAUDE_CODE_MAX_OUTPUT_TOKENS", "ANTHROPIC_API_KEY"];
        let env = dropped(L0.iter().chain(&yours).copied());
        assert!(env.nested);
        let kept = ["ANTHROPIC_BASE_URL", "MCP_CONNECTION_NONBLOCKING", "MCP_SERVER_CONNECTION_BATCH_SIZE"];
        let mut gone: Vec<&str> = L0.iter().copied().filter(|n| !kept.contains(n)).collect();
        gone.push("ANTHROPIC_API_KEY");
        crate::sorting::sort(&mut gone);
        assert_eq!(env.removed, gone);
        assert_eq!(env.passed, ["ANTHROPIC_BASE_URL", "CLAUDE_CODE_MAX_OUTPUT_TOKENS"]);
    }

    /// The rules as tarae has them by default (`llm.environment-key = "drop"`).
    fn dropped<'a>(names: impl IntoIterator<Item = &'a str>) -> ClaudeEnv {
        claude_env(names, false)
    }

    /// Outside Claude Code, a host's names may be your own setup (a self-hosted environment's session token, your
    /// tracing, OAuth scopes) — tarae can't tell, so they stay. Only a key, which bills in your login's place,
    /// and what Claude Code sets for its children (a stray one is always a parent's) go.
    #[test]
    fn outside_claude_code_only_keys_and_what_it_sets_for_children_go() {
        let names = [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "CLAUDE_PROJECT_DIR",
            "CLAUDE_CODE_MESSAGING_SOCKET",
            "CLAUDE_CODE_SESSION_ACCESS_TOKEN",
            "CLAUDE_CODE_OAUTH_SCOPES",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDE_CODE_USE_BEDROCK",
            "TRACEPARENT",
        ];
        let env = dropped(names);
        assert!(!env.nested);
        assert_eq!(
            env.removed,
            [
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "CLAUDE_CODE_MESSAGING_SOCKET",
                "CLAUDE_PROJECT_DIR"
            ]
        );
        assert!(env.passed.is_empty(), "nothing to say outside: {:?}", env.passed);
    }

    /// The OAuth scopes are "required when `CLAUDE_CODE_OAUTH_REFRESH_TOKEN` is set": beside it they're yours, even
    /// inside Claude Code; alone there, the host's.
    #[test]
    fn the_oauth_scopes_stay_with_their_refresh_token() {
        let with = dropped([
            "CLAUDE_CODE_CHILD_SESSION",
            "CLAUDE_CODE_OAUTH_SCOPES",
            "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
        ]);
        assert_eq!(with.removed, ["CLAUDE_CODE_CHILD_SESSION"]);
        let alone = dropped(["CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_OAUTH_SCOPES"]);
        assert_eq!(alone.removed, ["CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_OAUTH_SCOPES"]);
    }

    /// An IDE's terminal sets `CLAUDECODE` too ("IDE extensions also set this in their integrated terminals"), and
    /// a shell there is no Claude Code session's: tarae started from it isn't nested — your login and your tracing
    /// stay; `CLAUDECODE` itself still goes.
    #[test]
    fn an_ides_terminal_isnt_a_claude_code_session() {
        let ide = dropped(["CLAUDECODE", "CLAUDE_CODE_OAUTH_TOKEN", "TRACEPARENT"]);
        assert!(!ide.nested);
        assert_eq!(ide.removed, ["CLAUDECODE"], "only what Claude Code sets for its children");
    }

    /// Each of a parent session's own marks alone says tarae runs inside one — a stdio MCP server's process gets
    /// no `CLAUDE_CODE_CHILD_SESSION` but the session's id, an SDK host's claude its host's session id — and the
    /// host's login goes with it.
    #[test]
    fn each_mark_of_a_parent_session_alone_makes_it_nested() {
        for &mark in NESTED_BY {
            let env = dropped([mark, "CLAUDE_CODE_OAUTH_TOKEN"]);
            assert!(env.nested, "{mark}");
            assert!(env.removed.iter().any(|n| n == "CLAUDE_CODE_OAUTH_TOKEN"), "{mark}: {:?}", env.removed);
        }
    }

    /// The keys are dropped unless your config keeps them (a login by key alone, a gateway's token) — and inside a
    /// Claude Code session even then: there they're the host's. What's said names the setting where it helps.
    #[test]
    fn keys_go_to_claude_only_when_you_keep_them_outside_claude_code() {
        let keys = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL"];
        assert!(claude_env(keys, true).removed.is_empty(), "kept: a gateway's URL and token both go on");
        assert_eq!(dropped(keys).removed, ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]);
        let inside = claude_env(["CLAUDE_CODE_SESSION_ID", "ANTHROPIC_API_KEY"], true);
        assert_eq!(
            inside.removed,
            ["ANTHROPIC_API_KEY", "CLAUDE_CODE_SESSION_ID"],
            "the host's, kept or not"
        );
        assert!(dropped(["ANTHROPIC_API_KEY"]).said().unwrap().contains("llm.environment-key = \"keep\""));
        assert!(inside.said().unwrap().contains("the host's, whatever llm.environment-key says"));
    }

    /// `CLAUDE_CODE_OAUTH_TOKEN` is a login: outside Claude Code it's yours (`claude setup-token`) and Claude uses
    /// it; inside, it's the host session's (an SDK host passes the claude it starts its token), and Claude would
    /// answer on that host's login instead of yours.
    #[test]
    fn the_oauth_token_is_yours_outside_claude_code_and_the_hosts_inside() {
        assert!(dropped(["CLAUDE_CODE_OAUTH_TOKEN", "PATH"]).removed.is_empty(), "yours: passed");
        let inside = dropped(["CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_OAUTH_TOKEN"]);
        assert_eq!(inside.removed, ["CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_OAUTH_TOKEN"]);
    }

    /// What's left out is said by name — and, inside Claude Code, what's passed under a Claude name that no list
    /// holds (`CLAUDE_CODE_NEW_THING`), so it can be added. A key alone is said in words of its own; nothing left
    /// out says nothing.
    #[test]
    fn what_claude_doesnt_get_is_said_by_name() {
        let said = dropped([
            "CLAUDE_CODE_CHILD_SESSION",
            "CLAUDE_PID",
            "ANTHROPIC_API_KEY",
            "CLAUDE_CODE_NEW_THING",
        ])
        .said()
        .unwrap();
        assert!(said.starts_with("tarae runs inside Claude Code"), "{said}");
        assert!(said.contains(": CLAUDE_CODE_CHILD_SESSION, CLAUDE_PID."), "{said}");
        assert!(said.contains("ANTHROPIC_API_KEY isn't passed either"), "{said}");
        assert!(said.ends_with("Passed as they are: CLAUDE_CODE_NEW_THING."), "{said}");
        let key = dropped(["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "PATH"]).said().unwrap();
        assert!(
            key.starts_with("ANTHROPIC_API_KEY and ANTHROPIC_AUTH_TOKEN aren't passed to Claude:"),
            "{key}"
        );
        assert_eq!(dropped(["PATH", "CLAUDE_CODE_MAX_OUTPUT_TOKENS"]).said(), None);
    }

    /// tarae's environment as a test gives it: L0's names, an exported key and a setting of yours — each with a
    /// stand-in value (`zq7`: in nothing of tarae's own).
    pub(crate) fn nested_env() -> Env {
        let mut env: Env = L0.iter().map(|n| (n.to_string(), format!("{n}-zq7"))).collect();
        env.insert("ANTHROPIC_API_KEY".into(), "tarae-fake-key-zq7".into());
        env.insert("CLAUDE_CODE_MAX_OUTPUT_TOKENS".into(), "1234".into());
        env
    }

    /// A stand-in claude that writes its environment's names (no value) to `env.names` in the folder it's started
    /// in, then reads what it's sent.
    pub(crate) fn recording(cfg: &mut LlmConfig) {
        cfg.command = "sh".into();
        cfg.args = vec![
            "-c".into(),
            "awk 'BEGIN { for (k in ENVIRON) print k }' > env.tmp && mv env.tmp env.names; cat > /dev/null"
                .into(),
        ];
    }

    /// The names the stand-in wrote in `dir`, once it has (5 s at most), the file taken away for the next.
    fn names_in(ed: &mut Editor, dir: &Path) -> Vec<String> {
        let file = dir.join("env.names");
        let end = Instant::now() + crate::test_results::PATIENCE;
        while !file.exists() {
            assert!(Instant::now() < end, "the stand-in claude wrote no env.names");
            if let Some(ev) = ed.events.recv_timeout(Duration::from_millis(20)) {
                ed.handle_event(ev);
            }
        }
        let names = std::fs::read_to_string(&file).unwrap().lines().map(String::from).collect();
        std::fs::remove_file(&file).unwrap();
        names
    }

    /// What claude would inherit from the process itself is left out too, not only what a test gives: tarae's
    /// own checks run from inside a Claude Code session, its variables in the test binary's environment, and a
    /// stand-in claude got them all while every test passed. The variables are set only in a process of its own
    /// (`process_env_in_its_own_process`) — this one's is never changed.
    /// The same from a process outside any Claude Code session (its marks taken out of the child's environment,
    /// wherever the tests run) with `llm.environment-key = "keep"`: the stand-in's key goes on — `spawn` reads the
    /// setting — and `CLAUDECODE` (an IDE terminal's, say) still doesn't. A stand-in is the whole of what's
    /// judged (`ClaudeEnv::of`): the process's own key never reaches its claude.
    #[test]
    fn what_the_process_itself_holds_never_reaches_claude() {
        let run = |how: &str, env: &[(&str, &str)]| {
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .args(["--exact", "llm::tests::process_env_in_its_own_process", "--ignored", "--nocapture"])
                .env("TARAE_CLAUDE_ENV_CHILD", how)
                .envs(env.iter().copied());
            if how == "keep" {
                for &mark in NESTED_BY {
                    child.env_remove(mark);
                }
            }
            let out = child.output().unwrap();
            let said =
                String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
            assert!(out.status.success(), "{how}: {said}");
            assert!(said.contains("1 passed"), "{how}: the child ran it: {said}");
        };
        run("drop", &PROCESS_ENV);
        run("keep", &[("CLAUDECODE", "1"), ("ANTHROPIC_API_KEY", "tarae-fake-key-1")]);
    }

    /// A Claude Code session's variables, a key and a setting of yours — the process's own in the child.
    const PROCESS_ENV: [(&str, &str); 5] = [
        ("CLAUDECODE", "1"),
        ("CLAUDE_CODE_SESSION_ID", "x"),
        ("CLAUDE_CODE_HOST_SESSION_ID", "host-zq7"),
        ("ANTHROPIC_API_KEY", "tarae-fake-key-1"),
        ("CLAUDE_CODE_MAX_OUTPUT_TOKENS", "1234"),
    ];

    /// `what_the_process_itself_holds_never_reaches_claude`'s child: a stand-in claude started as every test
    /// starts one (`given` empty), from a process holding `PROCESS_ENV` (`drop`) — or, with the key kept, the
    /// stand-in holding one (`keep`).
    #[test]
    #[ignore]
    fn process_env_in_its_own_process() {
        let Some(how) = std::env::var_os("TARAE_CLAUDE_ENV_CHILD") else { return };
        let scratch = crate::scratch::Scratch::new("llm-process-env");
        let dir = scratch.to_path_buf();
        let mut cfg = LlmConfig { keep_env_key: how == "keep", ..LlmConfig::default() };
        recording(&mut cfg);
        let given = match how == "keep" {
            true => Env::from([("ANTHROPIC_API_KEY".into(), "tarae-fake-key-2".into())]),
            false => Env::new(),
        };
        let mut child = spawn(&cfg, &[], &dir, Some(&given), &Credential::Subscription).unwrap();
        drop(child.stdin.take());
        child.wait().unwrap();
        let names = std::fs::read_to_string(dir.join("env.names")).unwrap();
        let names: Vec<&str> = names.lines().collect();
        if how == "keep" {
            assert!(names.contains(&"ANTHROPIC_API_KEY"), "kept: {names:?}");
            assert!(!names.contains(&"CLAUDECODE"), "CLAUDECODE, the process's, reached claude");
        } else {
            for (gone, _) in &PROCESS_ENV[..4] {
                assert!(!names.contains(gone), "{gone}, the process's, reached claude");
            }
            assert!(names.contains(&"CLAUDE_CODE_MAX_OUTPUT_TOKENS"), "yours stays: {names:?}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// claude failing for want of a login, with a key tarae left out and no key card for it (here: answered `n`),
    /// is told why and what passes it — in the chat, after claude's own words — and nothing of the key's value. A
    /// key you keep, or another failure, adds nothing.
    #[test]
    fn a_login_failure_with_a_key_left_out_names_the_setting() {
        let scratch = crate::scratch::Scratch::new("llm-login");
        let root = scratch.to_path_buf();
        let mut ed = Editor::with_root(crate::config::Config::default(), root.clone());
        ed.claude_env = Some(Env::from([("ANTHROPIC_API_KEY".into(), "tarae-fake-key-zq7".into())]));
        ed.workspace.claude.env_key = Some(false);
        // (a stand-in claude with no login: each message answered as Claude Code does under `-p`)
        let failed = r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#;
        ed.config.llm.command = "sh".into();
        ed.config.llm.args = vec![
            "-c".into(),
            format!(
                "while read -r line; do case \"$line\" in *'\"type\":\"user\"'*) printf '%s\\n' '{failed}';; esac; done"
            ),
        ];
        crate::chat::open(&mut ed);
        ed.workspace.claude.chat.as_mut().unwrap().input = "hello".into();
        crate::chat::send(&mut ed);
        let hint = |ed: &Editor| {
            ed.workspace
                .claude
                .chat
                .as_ref()
                .unwrap()
                .msgs
                .iter()
                .find(|m| m.text.contains("no login"))
                .map(|m| m.text.clone())
        };
        let end = Instant::now() + crate::test_results::PATIENCE;
        while hint(&ed).is_none() {
            assert!(
                Instant::now() < end,
                "no hint: {:?}",
                ed.workspace.claude.chat.as_ref().unwrap().msgs.iter().map(|m| &m.text).collect::<Vec<_>>()
            );
            if let Some(ev) = ed.events.recv_timeout(Duration::from_millis(20)) {
                ed.handle_event(ev);
            }
        }
        let hint = hint(&ed).unwrap();
        assert!(
            hint.contains("ANTHROPIC_API_KEY") && hint.contains("llm.environment-key = \"keep\""),
            "{hint}"
        );
        assert!(!hint.contains("zq7"), "names only: {hint}");
        assert!(hint.ends_with("Or log in: run `claude` in a terminal, then /login."), "yours: {hint}");
        assert_eq!(login_hint(&mut ed, "Overloaded"), None, "not a login failure");
        ed.config.llm.keep_env_key = true;
        assert_eq!(
            login_hint(&mut ed, "Not logged in · Please run /login"),
            None,
            "the key went on: nothing to add"
        );
        drop(ed);
        std::fs::remove_dir_all(&root).ok();
    }

    /// The chat's claude and the one-turn claude of the ask and the commit draft (`start`) get tarae's environment
    /// less what `claude_env` says — the same names left out, the rest as it was (the reviewer's: its own test).
    /// The chat says so once since tarae started, by name: a new conversation's claude doesn't say it again.
    #[test]
    fn the_chats_and_the_asks_claude_leave_out_a_parent_sessions_variables() {
        let scratch = crate::scratch::Scratch::new("llm-claude-env");
        let root = scratch.to_path_buf();
        let mut ed = Editor::with_root(crate::config::Config::default(), root.clone());
        recording(&mut ed.config.llm);
        ed.claude_env = Some(nested_env());
        let log = crate::log::capture();
        let gone = ClaudeEnv::of(ed.claude_env.as_ref(), false).removed;
        let check = |names: Vec<String>, who: &str| {
            for name in &gone {
                assert!(!names.contains(name), "{who}: {name} reached claude");
            }
            for kept in ["CLAUDE_CODE_MAX_OUTPUT_TOKENS", "ANTHROPIC_BASE_URL", "MCP_CONNECTION_NONBLOCKING"]
            {
                assert!(names.iter().any(|n| n == kept), "{who}: {kept} left out — {names:?}");
            }
        };
        assert!(gone.iter().any(|n| n == "ANTHROPIC_API_KEY") && gone.iter().any(|n| n == "CLAUDECODE"));
        crate::chat::open(&mut ed);
        check(names_in(&mut ed, &root), "the chat");
        let notes = |ed: &Editor| -> Vec<String> {
            let c = ed.workspace.claude.chat.as_ref().unwrap();
            c.msgs
                .iter()
                .filter(|m| m.text.contains("CLAUDE_CODE_CHILD_SESSION"))
                .map(|m| m.text.clone())
                .collect()
        };
        assert_eq!(notes(&ed).len(), 1, "said in the chat");
        assert!(!notes(&ed)[0].contains("zq7"), "names only: {}", notes(&ed)[0]);
        let logged =
            log.lines().into_iter().filter(|l| l.starts_with("claude  info  tarae runs inside")).count();
        assert_eq!(logged, 1, "one log line");
        crate::chat::reset(&mut ed);
        names_in(&mut ed, &root);
        assert!(notes(&ed).is_empty(), "a new conversation: not again");
        let doc = ed.doc().id;
        draft(&mut ed, doc, "a commit message", "Draft it.", 0, 0).unwrap();
        check(names_in(&mut ed, &root), "a draft's (the ask's starts the same way, `start`)");
        drop(ed);
        std::fs::remove_dir_all(&root).ok();
    }

    /// A commit message's body wrapped at 72: the subject and short lines as they are, a long line between
    /// words, a list item's rest under its text; code, trailers and an over-long word whole.
    #[test]
    fn commit_messages_wrap_at_72() {
        let long = "word ".repeat(20).trim_end().to_string(); // 99 columns
        let msg = format!(
            "{long}\n\nshort line\n{long}\n- {long}\n    {long}\nCo-Authored-By: {long}\n{}",
            "x".repeat(80)
        );
        let out = wrap_message(&msg, 72);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], long, "the subject stays");
        assert_eq!(lines[2], "short line");
        let row = "word ".repeat(14).trim_end().to_string(); // 69 columns — one more word would be 74
        assert_eq!((lines[3], lines[4]), (row.as_str(), "word word word word word word"));
        assert_eq!(lines[5], format!("- {}", "word ".repeat(14).trim_end()), "a list item (71 columns)");
        assert!(lines[6].starts_with("  word"), "its rest under its text: {:?}", lines[6]);
        assert!(lines.contains(&format!("    {long}").as_str()), "code stays");
        assert!(lines.contains(&format!("Co-Authored-By: {long}").as_str()), "a trailer stays");
        assert_eq!(*lines.last().unwrap(), "x".repeat(80), "a word longer than the width stays whole");
        assert!(
            lines[2..]
                .iter()
                .all(|l| l.len() <= 72 || l.starts_with(' ') || !l.contains(' ') || l.contains(": "))
        );
    }

    #[test]
    fn stream_lines_and_preview() {
        let delta = serde_json::json!({"type": "stream_event", "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "ab"}}});
        assert!(matches!(parse_line(&delta), Some(StreamEv::Text(t)) if t == "ab"));
        let think =
            serde_json::json!({"type": "system", "subtype": "thinking_tokens", "estimated_tokens": 5});
        assert!(matches!(parse_line(&think), Some(StreamEv::Thinking)));
        let stopped =
            serde_json::json!({"type": "result", "subtype": "error_during_execution", "result": ""});
        assert!(matches!(parse_line(&stopped), Some(StreamEv::Done(Err(e))) if e == "stopped"));
        // Preview: the last lines being written
        assert_eq!(preview("\nx\ny\nz", 2), vec!["y", "z"]);
        assert_eq!(preview("plain\ntext", 5), vec!["plain", "text"]);
    }

    /// A draft's answer is the text it is — a code fence around it taken off.
    #[test]
    fn a_drafts_answer_loses_its_fence() {
        assert_eq!(answer_of("```rust\nlet x = 1;\n```"), "let x = 1;");
        assert_eq!(answer_of("\nfix: a thing\n"), "fix: a thing");
    }

    #[test]
    fn review_rows_expand_changes_in_place() {
        let text = Rope::from_str("a\nb\nc\n");
        let hunk = |decision| Hunk {
            from: 2,
            to: 4,
            old: "b\n".into(),
            new: "B\nB2\n".into(),
            decision,
            conflict: false,
        };
        let rev = |h| Review::new(0, 0, "", vec![h]);
        let rows = |h, text: &Rope| rev(h).rows(text).iter().map(shape).collect::<Vec<_>>();
        // (a line the change takes out or keeps is the document's own: its number)
        assert_eq!(
            rows(hunk(Decision::Undecided), &text),
            ["doc 1", "head 0", "- 2", "+ B (washed)", "+ B2 (washed)", "doc 3"]
        );
        // Accept = new text only, reject = original only
        assert_eq!(rows(hunk(Decision::Accept), &text), ["doc 1", "head 0", "+ B", "+ B2", "doc 3"]);
        assert_eq!(rows(hunk(Decision::Reject), &text), ["doc 1", "head 0", "  2", "doc 3"]);
        // Edited while the review waits: what it takes out comes from the text it was asked on
        let moved = Rope::from_str("a\nx\nc\n");
        assert_eq!(
            rows(hunk(Decision::Undecided), &moved),
            ["doc 1", "head 0", "- b (asked)", "+ B (washed)", "+ B2 (washed)", "doc 3"]
        );
    }

    /// A review row in words: `doc N` a line as is, `head K` change K's head, `- N`/`  N` a line it takes out or
    /// keeps, `+ text` one it proposes; `(asked)` from the text it was asked on.
    fn shape(r: &crate::codeview::Row) -> String {
        use crate::codeview::{Edit, Mark, OtherKind, Row};
        let sign = |m: Mark| match m {
            Mark::Added { .. } => "+",
            Mark::Taken => "-",
            Mark::Kept => " ",
        };
        match r {
            Row::Text { line, edit: None, .. } => format!("doc {}", line + 1),
            Row::Text { line, edit: Some(e), .. } => format!("{} {}", sign(e.mark), line + 1),
            Row::Header { hunk, .. } => format!("head {hunk}"),
            Row::Other {
                text, kind: OtherKind::Edit(Edit { mark: m @ Mark::Added { washed }, .. }), ..
            } => {
                format!("{} {text}{}", sign(*m), if *washed { " (washed)" } else { "" })
            }
            Row::Other { text, kind: OtherKind::Edit(e), .. } => format!("{} {text} (asked)", sign(e.mark)),
            Row::Other { text, .. } => format!("gone {text}"),
            Row::Doc { start, index, .. } => format!("folded {} {index}", start + 1),
        }
    }

    #[test]
    fn trailing_newline_follows_original() {
        assert_eq!(match_trailing_newline("a\n", "b".into()), "b\n");
        assert_eq!(match_trailing_newline("a", "b\n\n".into()), "b");
    }

    #[test]
    fn line_diff_marks_changes() {
        let d = line_diff("a\nb\nc", "a\nB\nc");
        assert_eq!(d, vec![(' ', "a"), ('-', "b"), ('+', "B"), (' ', "c")]);
    }
}
