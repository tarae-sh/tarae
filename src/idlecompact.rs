//! Idle compaction (audit 2.C, 2026-10-26): a big conversation left idle is compacted shortly before its prompt cache
//! expires, so the message you come back with writes a summary to the cache, not the whole context (a write costs
//! twice the input price, a read a tenth). Claude Code's own rule, read in 2.1.302's code: it arms a timer after each
//! request, on the 1 h TTL only, at 0.9 of the TTL from that request (54 min); fired, it compacts a context of at
//! least `max(100k, CLAUDE_CODE_IDLE_COMPACT_MIN_TOKENS)` (else 200k) unless `idleCompaction` or `autoCompactEnabled`
//! is `false` (or `DISABLE_COMPACT`, `DISABLE_AUTO_COMPACT`), the wake over 60 s late (a machine asleep), a newer
//! request, or the plan's limit near. Claude Code gates it by a server-side flag; tarae's is on by default (the user,
//! 2026-10-26). `claude -p` doesn't do it by itself, so tarae does, by the same rule and your same settings:
//!
//! - **When** is `decide`, a pure function of `Facts`. `usage::watch`'s one sleeping thread a session wakes at `due`.
//! - **Read where claude reads them** (`Places`): the settings files the chat's claude loads (its `--setting-sources`,
//!   lowest first, then its `--settings`, then the managed file), and the variables as it gets them (theirs `env`
//!   blocks over its environment). Read on a worker after a turn (for the row) and again when it fires.
//! - **What it sends** is `/compact` as a turn of its own (a bare message — a slash command is read only at a
//!   message's start); `compaction.rs` says what follows: `compacting…`, the chat's note (`idle`), the context figure.
//! - **Said**: the chat's last row `compacts 14:26` beside `warm until 14:32` while it would, the state's
//!   `usage.idle_compact`, a note in the chat as it's sent.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::editor::Editor;
use crate::usage::Ttl;
use crate::workspace::Workspace;

/// It fires at this share of the TTL after the last request (`fireAtFraction`'s default, 0.9).
pub const FRACTION: (u64, u64) = (9, 10);
/// A wake later than this past `due` lets it go (`lapsed`: the machine slept — the cache may be gone).
pub const LATE_MS: u64 = 60_000;
/// The context it compacts from, by default · at least, whatever the variable says.
pub const MIN_TOKENS: u64 = 200_000;
pub const FLOOR: u64 = 100_000;
/// The variable that moves `MIN_TOKENS`.
pub const MIN_VAR: &str = "CLAUDE_CODE_IDLE_COMPACT_MIN_TOKENS";
/// What turns it off, in the order claude asks: the variables that turn compaction off, then the two settings.
pub const OFF: [&str; 4] =
    ["DISABLE_COMPACT", "DISABLE_AUTO_COMPACT", "autoCompactEnabled", "idleCompaction"];
/// What's sent.
pub const COMMAND: &str = "/compact";
/// A settings file bigger than this isn't read (claudetrust's limit).
const READ_MAX: u64 = 1 << 20;

/// The rule as the chat's claude reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// What turned it off (one of `OFF`) — None: on.
    pub off: Option<&'static str>,
    pub min_tokens: u64,
}

impl Default for Rule {
    fn default() -> Rule {
        Rule { off: None, min_tokens: MIN_TOKENS }
    }
}

/// A session's: the rule as last read (a read under way), and the `/compact` tarae sent (the turn's start: its
/// compaction is `idle`).
#[derive(Default)]
pub struct Idle {
    pub rule: Rule,
    pub reading: bool,
    pub sent: Option<std::time::Instant>,
    /// Why it last didn't compact once woken — till the next turn.
    pub held: Option<Why>,
}

/// What of the session isn't idle — it waits on you, or works.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Busy {
    /// A turn under way (its tool calls waiting for you too).
    Turn,
    /// A message waits to go.
    Queued,
    /// Claude's edits wait for their review, or a review is open in the session.
    Review,
    /// A card of the session's waits for your answer.
    Card,
    /// A run of its Claude's is going, or waits its turn.
    Run,
    Rebase,
    /// Its checkout is being moved onto its base (`keepclose.rs`).
    Moving,
}

impl Busy {
    pub fn name(self) -> &'static str {
        match self {
            Busy::Turn => "a turn",
            Busy::Queued => "a message queued",
            Busy::Review => "a review",
            Busy::Card => "a card",
            Busy::Run => "a run",
            Busy::Rebase => "a rebase",
            Busy::Moving => "a move onto its base",
        }
    }
}

/// Why it doesn't compact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// No chat whose claude runs, or no request reported.
    NoCache,
    /// The 5-minute TTL: claude's rule is the hour's only.
    NotOneHour,
    Off(&'static str),
    Small,
    /// Woken past `due` + `LATE_MS`.
    Late,
    Busy(Busy),
    /// That plan window is near its limit (its label).
    Pressing(String),
}

impl Why {
    /// The state's word.
    pub fn name(&self) -> &'static str {
        match self {
            Why::NoCache => "no cache",
            Why::NotOneHour => "5m TTL",
            Why::Off(_) => "off",
            Why::Small => "small",
            Why::Late => "late",
            Why::Busy(_) => "busy",
            Why::Pressing(_) => "near a limit",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Compact,
    /// Not yet: wake then.
    WakeAt(u64),
    LetGo(Why),
}

/// What `decide` goes by.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub now_ms: u64,
    /// A chat whose claude runs, to send it to.
    pub chat: bool,
    /// The last request that touched the cache (unix ms) · the TTL of its write (None: not reported).
    pub touched_ms: Option<u64>,
    pub ttl: Option<Ttl>,
    /// The context's tokens (None: not reported).
    pub tokens: Option<u64>,
    pub rule: Rule,
    pub busy: Option<Busy>,
    /// A plan window near its limit (its label) — None on API billing, which no plan window limits.
    pub pressing: Option<String>,
}

/// When it fires after a request at `touched_ms` — None but on the 1 h TTL.
pub fn due(touched_ms: u64, ttl: Ttl) -> Option<u64> {
    (ttl == Ttl::Hour1).then(|| touched_ms + ttl.ms() * FRACTION.0 / FRACTION.1)
}

/// Whether to compact now. Each rule lets it go, or says when to look again; all of them pass → `Compact`.
pub fn decide(f: &Facts) -> Decision {
    use Decision::{Compact, LetGo, WakeAt};
    let (true, Some(touched), Some(ttl)) = (f.chat, f.touched_ms, f.ttl) else { return LetGo(Why::NoCache) };
    let Some(due) = due(touched, ttl) else { return LetGo(Why::NotOneHour) };
    if let Some(off) = f.rule.off {
        return LetGo(Why::Off(off));
    }
    if f.tokens.is_none_or(|n| n < f.rule.min_tokens) {
        return LetGo(Why::Small);
    }
    if f.now_ms < due {
        return WakeAt(due);
    }
    if f.now_ms > due + LATE_MS {
        return LetGo(Why::Late);
    }
    if let Some(b) = f.busy {
        return LetGo(Why::Busy(b));
    }
    if let Some(w) = &f.pressing {
        return LetGo(Why::Pressing(w.clone()));
    }
    Compact
}

/// The facts of session `s` at `now_ms`, `cards`: whether a card of its waits (`cards_waiting`).
pub fn facts(s: &Workspace, cards: bool, limits: &crate::limits::Limits, now_ms: u64) -> Facts {
    let u = &s.claude.usage;
    let conv = u.conv.as_ref();
    let on_plan = s.claude.billing == crate::billing::Billing::Subscription;
    let pressing = limits.known(now_ms).into_iter().find(|r| crate::limits::pressing(r));
    Facts {
        now_ms,
        chat: s.claude.chat.as_ref().is_some_and(|c| c.running()),
        touched_ms: conv.and_then(|c| c.touched_ms),
        ttl: conv.and_then(|c| c.ttl),
        tokens: u.context().map(|c| c.0),
        rule: s.claude.idle.rule.clone(),
        busy: busy(s, cards),
        pressing: pressing.filter(|_| on_plan).map(|r| crate::limits::label(&r.window)),
    }
}

/// Whether a card of session `s`'s waits for your answer: the front's are up (`Editor.offers`), one behind keeps
/// its own (`parked`) — swapped in by `with_workspace` too (`Editor.behind`: `offers` are still the front's then);
/// its Claude's calls asking (`asking`) either way.
pub fn cards_waiting(ed: &Editor, s: &Workspace) -> bool {
    let front = s.id == ed.workspace.id && !ed.background();
    let up = if front { !ed.offers.is_empty() } else { !s.parked.is_empty() };
    up || !s.claude.asking.is_empty()
}

/// What of session `s` isn't idle, the first in this order (`cards`: `cards_waiting`).
pub fn busy(s: &Workspace, cards: bool) -> Option<Busy> {
    let chat = s.claude.chat.as_ref();
    let review = chat.is_some_and(|c| !c.edits.sets.is_empty())
        || s.claude.review.is_some()
        || s.code_review.is_some();
    let claudes_test =
        s.test_run.as_ref().is_some_and(|r| r.by_claude && r.state == crate::testing::RunState::Running);
    let claudes_task =
        s.task_run.as_ref().is_some_and(|r| r.by_claude && r.state == crate::task::State::Running);
    let run = claudes_test
        || claudes_task
        || !s.runs_next.is_empty()
        || !s.claude.tool_waits.is_empty()
        || chat.is_some_and(|c| !c.reruns.is_empty());
    [
        (chat.is_some_and(|c| c.busy()), Busy::Turn),
        (chat.is_some_and(|c| c.queued.is_some() || !c.sends.is_empty()), Busy::Queued),
        (review, Busy::Review),
        (cards, Busy::Card),
        (run, Busy::Run),
        (s.rebase.is_some(), Busy::Rebase),
        (s.moving.is_some(), Busy::Moving),
    ]
    .into_iter()
    .find_map(|(holds, b)| holds.then_some(b))
}

/// When session `s` compacts if nothing happens till then, as things stand now — the row's `compacts 14:26`, the
/// state's.
pub fn next_at(ed: &Editor, s: &Workspace, now_ms: u64) -> Option<u64> {
    let f = facts(s, cards_waiting(ed, s), &ed.limits, now_ms);
    let at = due(f.touched_ms?, f.ttl?)?;
    (decide(&Facts { now_ms: at.max(now_ms), ..f }) == Decision::Compact).then_some(at)
}

/// The state's `usage.idle_compact`: when, what turned it off, from what size, why it last didn't.
pub fn json(ed: &Editor, s: &Workspace, now_ms: u64) -> Value {
    let (r, held) = (&s.claude.idle.rule, s.claude.idle.held.as_ref().map(Why::name));
    json!({ "at_ms": next_at(ed, s, now_ms), "off": r.off, "min_tokens": r.min_tokens, "held": held })
}

// ── The rule, read as claude reads it ────────────────────────────────────────────────

/// `min_tokens` from the variable's value: a number, at least `FLOOR` — else `MIN_TOKENS` ("" too: a held-back
/// variable reads as unset, `claudetrust.rs`).
pub fn min_tokens(v: Option<&str>) -> u64 {
    match v.map(str::trim).and_then(|v| v.parse::<f64>().ok()).filter(|n| n.is_finite()) {
        Some(n) => (n.ceil().max(0.0) as u64).max(FLOOR),
        None => MIN_TOKENS,
    }
}

/// A variable set the way claude reads a switch.
fn truthy(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

/// The rule from settings `layers`, lowest first (a later one's key wins), and `env`, the environment claude got —
/// each layer's `env` block laid over it, as claude applies them.
pub fn rule(layers: &[Map<String, Value>], env: impl Fn(&str) -> Option<String>) -> Rule {
    let setting = |key: &str| layers.iter().rev().find_map(|l| l.get(key)?.as_bool());
    let var = |name: &str| {
        let set = layers.iter().rev().find_map(|l| l.get("env")?.get(name)?.as_str().map(String::from));
        set.or_else(|| env(name))
    };
    let off = OFF.into_iter().find(|&k| match k.starts_with("DISABLE_") {
        true => var(k).is_some_and(|v| truthy(&v)),
        false => setting(k) == Some(false),
    });
    Rule { off, min_tokens: min_tokens(var(MIN_VAR).as_deref()) }
}

/// A settings file as an object — None: not there, not a regular file, too big, not a JSON object.
fn read_file(path: &Path) -> Option<Map<String, Value>> {
    std::fs::metadata(path).ok().filter(|m| m.is_file() && m.len() <= READ_MAX)?;
    match serde_json::from_slice(&std::fs::read(path).ok()?).ok()? {
        Value::Object(o) => Some(o),
        _ => None,
    }
}

/// `flag`'s value in `args`.
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// What a read needs, taken to a worker: what the chat's claude was started with and where its settings are.
#[derive(Clone, Debug)]
pub struct Places {
    /// Its `--setting-sources` (None: not given — claude loads all three).
    pub sources: Option<String>,
    /// Its `--settings`: tarae's JSON, or a file.
    pub settings: Option<String>,
    pub root: PathBuf,
    /// Your Claude settings file (`claudetrust::user_settings`; None in tests).
    pub user: Option<PathBuf>,
    /// The managed settings file and its folder's files (none in tests).
    pub managed: Vec<PathBuf>,
    /// The variables the rule reads, as claude got them.
    pub env: Vec<(&'static str, String)>,
}

impl Places {
    /// For session `ed.workspace`'s chat — None: it has none.
    pub fn of(ed: &Editor) -> Option<Places> {
        // (a simulation's stand-in was started with nothing: claude's own default, every scope)
        let c = ed.workspace.claude.chat.as_ref()?;
        let args = c.launched.as_ref().map_or(&[][..], |l| &l.0.args[..]);
        // (a test's stand-in is the whole of the environment, as `llm::spawn` lays it — else tarae's own, never in
        // a test build)
        let names = std::iter::once(MIN_VAR).chain(OFF.into_iter().filter(|k| k.starts_with("DISABLE_")));
        let env =
            names.filter_map(|n| crate::llm::env_var(ed.claude_env.as_ref(), n).map(|v| (n, v))).collect();
        Some(Places {
            sources: flag(args, "--setting-sources").map(String::from),
            settings: flag(args, "--settings").map(String::from),
            root: ed.workspace.root.clone(),
            user: ed.claude_user_settings.clone(),
            managed: if cfg!(test) { Vec::new() } else { managed() },
            env,
        })
    }

    /// The rule, read now — on a worker.
    pub fn rule(&self) -> Rule {
        let sources = self.sources.as_deref().unwrap_or(crate::claudetrust::SOURCES);
        let loads = |s: &str| sources.split(',').any(|x| x.trim() == s);
        let mut layers = Vec::new();
        if loads("user") {
            layers.extend(self.user.as_deref().and_then(read_file));
        }
        for (name, source) in crate::claudetrust::FILES {
            if loads(source) {
                layers.extend(read_file(&self.root.join(name)));
            }
        }
        // (tarae's `--settings` is JSON; a path, as claude takes one too, from the folder it runs in)
        if let Some(s) = &self.settings {
            match serde_json::from_str(s) {
                Ok(Value::Object(o)) => layers.push(o),
                _ => layers.extend(read_file(&self.root.join(s))),
            }
        }
        layers.extend(self.managed.iter().filter_map(|p| read_file(p)));
        rule(&layers, |n| self.env.iter().find(|(k, _)| *k == n).map(|(_, v)| v.clone()))
    }
}

/// The managed settings: the file, then its folder's `.json` files by name (Claude Code's settings docs).
fn managed() -> Vec<PathBuf> {
    let dir = match std::env::consts::OS {
        "macos" => "/Library/Application Support/ClaudeCode",
        _ => "/etc/claude-code",
    };
    let dir = Path::new(dir);
    let mut drop_ins: Vec<PathBuf> = std::fs::read_dir(dir.join("managed-settings.d"))
        .map(|rd| {
            rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect()
        })
        .unwrap_or_default();
    crate::sorting::sort(&mut drop_ins);
    std::iter::once(dir.join("managed-settings.json")).chain(drop_ins).collect()
}

// ── On the loop ────────────────────────────────────────────────────────────────────

/// The facts of the session in front, now.
fn now(ed: &Editor) -> Facts {
    facts(&ed.workspace, cards_waiting(ed, &ed.workspace), &ed.limits, crate::usage::now_ms())
}

/// After a turn (`usage::watch`): the rule read again for the row, where it may come to compacting.
pub fn arm(ed: &mut Editor) {
    ed.workspace.claude.idle.held = None;
    let f = now(ed);
    let hour = f.ttl == Some(Ttl::Hour1);
    if f.chat && hour && f.tokens.is_some_and(|n| n >= FLOOR) {
        read(ed, false);
    }
}

/// The usage watch woke inside the cache's life (`usage::woke`): compact (the rule read again first), wake later,
/// or let it go. True: the read is under way — it arms the watch again once it's decided (`go`).
pub fn woke(ed: &mut Editor) -> bool {
    match decide(&now(ed)) {
        Decision::Compact => return read(ed, true),
        Decision::WakeAt(at) => crate::usage::sleep_until(ed, at),
        Decision::LetGo(why) => let_go(ed, &why),
    }
    false
}

/// The rule read on a worker for the chat of now; `compact`: then, still due, `/compact` (`go`). False: no chat.
fn read(ed: &mut Editor, compact: bool) -> bool {
    let Some(places) = Places::of(ed) else { return false };
    let Some(generation) = ed.workspace.claude.chat.as_ref().map(|c| c.generation()) else { return false };
    ed.workspace.claude.idle.reading = true;
    ed.events.jobs().spawn(move || {
        let rule = places.rule();
        move |ed: &mut Editor| {
            ed.workspace.claude.idle.reading = false;
            // (another claude since — a restart, the chat closed: its own turn arms it again)
            if ed.workspace.claude.chat.as_ref().is_none_or(|c| c.generation() != generation) {
                ed.unchanged = true;
                if compact {
                    crate::usage::rearm(ed);
                }
                return;
            }
            let same = ed.workspace.claude.idle.rule == rule;
            ed.workspace.claude.idle.rule = rule;
            match compact {
                true => {
                    go(ed);
                    crate::usage::rearm(ed);
                }
                false if same => ed.unchanged = true,
                false => {}
            }
        }
    });
    true
}

/// Fired, the rule read again: `/compact`, if it still holds.
fn go(ed: &mut Editor) {
    match decide(&now(ed)) {
        Decision::Compact => {}
        Decision::WakeAt(_) => return,
        Decision::LetGo(why) => return let_go(ed, &why),
    }
    let touched = ed.workspace.claude.usage.conv.as_ref().and_then(|c| c.touched_ms);
    let idle = touched.map_or(0, |t| crate::usage::now_ms().saturating_sub(t));
    let idle = crate::term::run_duration(std::time::Duration::from_millis(idle));
    let cold = ed.workspace.claude.usage.cold_at().map_or_else(String::new, crate::usage::until);
    let note = format!(
        "Idle {idle} — compacting before Claude's cache expires at {cold}, so the next message doesn't write it \
         all again (\"idleCompaction\": false in your Claude settings turns it off)"
    );
    if !crate::chat::submit_bare(ed, COMMAND, note) {
        crate::log::line("claude", "info", "idle-compact: not sent — the chat couldn't take it");
        return;
    }
    ed.workspace.claude.idle.sent = ed.workspace.claude.chat.as_ref().map(|c| c.started);
    crate::log::line("claude", "info", &format!("idle-compact: /compact sent, idle {idle}"));
}

/// Not compacted once woken — in the log. The row said `compacts 14:26` up to now where the rule was read off just
/// now or the wake came late: a frame takes it off. What the row never said (too small, a card, near a limit —
/// `next_at` read the same) draws nothing.
fn let_go(ed: &mut Editor, why: &Why) {
    ed.unchanged = !matches!(why, Why::Off(_) | Why::Late);
    ed.workspace.claude.idle.held = Some(why.clone());
    let said = match why {
        Why::NoCache | Why::NotOneHour | Why::Small => return,
        Why::Off(k) => format!("{k} turns it off"),
        Why::Late => "woken too late (the machine slept)".to_string(),
        Why::Busy(b) => format!("{} waits", b.name()),
        Why::Pressing(w) => format!("the {w} window is near its limit"),
    };
    crate::log::line("claude", "info", &format!("idle-compact: not compacted — {said}"));
}

/// A compaction came (`compaction::on_compacted`): whether in the turn tarae's `/compact` started — then it's said
/// `idle`.
pub fn of_turn(ed: &mut Editor) -> bool {
    let started = ed.workspace.claude.chat.as_ref().map(|c| c.started);
    let idle = started.is_some() && ed.workspace.claude.idle.sent == started;
    if idle {
        ed.workspace.claude.idle.sent = None;
    }
    idle
}

#[cfg(test)]
mod tests;
