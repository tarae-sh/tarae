//! Sessions: the participants — who acts. A session is one Claude at work in its workspace for the workspace's whole
//! life (`Workspace.claude`): its conversation and process, its notes and turns, the cards it asks, what it costs and
//! who pays, the grants answered for it, its runs' keeper (collab C0, `.sb/collab-plan.md` §1.1). You are a
//! session too, visiting workspaces — where you are in one is its `Viewer` (`workspace.rs`). The place, its
//! checkout and layout, is the workspace's.

use std::time::Instant;

use crate::event::{SessionId, WorkspaceId};

/// The participants' colours a theme has (`ui.participant.1` … `.6`), by `Session.colour`.
pub const COLOURS: usize = 6;
pub const PARTICIPANTS: [&str; COLOURS] = [
    "ui.participant.1",
    "ui.participant.2",
    "ui.participant.3",
    "ui.participant.4",
    "ui.participant.5",
    "ui.participant.6",
];

/// The id of the Claude born with workspace `id`: the same number, on the same machine.
pub fn born_with(id: WorkspaceId) -> SessionId {
    id.twin()
}

pub struct Session {
    pub id: SessionId,
    /// Its participant colour (`ui.participant.<colour + 1>`, collab C1): given as it's made — the lowest no other open
    /// session has — and kept for its life, the same on every surface it shows on.
    pub colour: u8,
    /// Chat pane on the right (if open).
    pub chat: Option<crate::chat::Chat>,
    /// Claude's notes pinned to code (`notes.rs`).
    pub notes: crate::notes::Notes,
    /// What Claude changed per chat turn — undone as one (`checkpoint.rs`).
    pub checkpoints: crate::checkpoint::Checkpoints,
    /// The in-buffer review (`llm.rs`) — `space i`'s, a chat edit's.
    pub review: Option<crate::llm::Review>,
    /// `space i`'s claude while it runs (`ask.rs`): one turn beside the chat, nothing of it the chat's.
    pub ask: Option<crate::ask::Ask>,
    /// A yes on the board's card (`board::Ask`), and the chat's conversation it was given in: another board in this
    /// session starts unasked while that conversation goes on. Not kept past a restart.
    pub board_yes: Option<Option<String>>,
    /// Its Claude ended a turn while its workspace was behind, and that hasn't been in front since: when, and how
    /// (stopped or failed: its words) — its tab says `done` (#86: not the `▲` of a card waiting), a toast says it
    /// once, and `C-q` goes there once nothing waits (the one that ended first, first).
    pub turn_end: Option<(Instant, Result<(), String>)>,
    /// Its Claude's tool calls waiting for the editor's answer (`tools::WATCHED`) — one silent past its time
    /// marks the session possibly stuck (`Doing::Stuck`).
    pub tool_waits: Vec<crate::workspace::ToolWait>,
    /// No sandbox here, or a partial one: what you said on its card for Claude's commands — asked at the
    /// session's first run, held until it ends, never kept past a restart (`tools::NoSandbox`, Q6).
    pub no_sandbox: Option<crate::tools::NoSandbox>,
    /// The key card's answer (`envkey.rs`): `true` — its chat's claude takes a key from tarae's environment
    /// until the session ends · `false` — not asked again. None: not asked. Never kept past a restart.
    pub env_key: Option<bool>,
    /// The run cards of its Claude's answered until you quit — each the last for its run in its folder
    /// (`sinceyes::Answered`): the same run asked again says `Same as card#41 — y at 18:02` and what changed
    /// since in what it runs (cardburden 3).
    pub answered: Vec<crate::sinceyes::Answered>,
    /// Its Claude's calls waiting on a card for the user's answer (`offer::Asking`) — each card must be up
    /// (in front) or parked (behind) until it's answered; one gone is told (`asking_sync`).
    pub asking: Vec<crate::offer::Asking>,
    /// What its Claude's turns cost and how warm its cache is (`usage.rs`).
    pub usage: crate::usage::Usage,
    /// Its conversation compacted before its cache expires while it's idle: the rule as read, the `/compact` sent
    /// (`idlecompact.rs`).
    pub idle: crate::idlecompact::Idle,
    /// Who pays for its Claude — the chat's, the ask's, the reviewer's unless `llm.reviewer-billing` says
    /// (`billing.rs`): `llm.billing` when it's made, kept with it (`worktree::Kept`), `:session-billing` switches it.
    pub billing: crate::billing::Billing,
    /// The billing its chat goes on once Claude's turn is over (the limit card's `y` mid-turn — `billing.rs`).
    pub billing_next: Option<crate::billing::Billing>,
    /// The plan's weekly window its limit card was up for (its reset, unix ms): not asked again for it.
    pub limit_asked: Option<u64>,
    /// Its Claude's conversation compacted since tarae started — how often, the last one, and whether one
    /// came behind and it hasn't been in front since (`compaction.rs`).
    pub compacted: crate::compaction::Compacted,
    /// The hosts its commands reach through tarae's proxy by the user's `s` (this session, until you quit), at
    /// their ports — what `:granted` lists and takes back; the proxy holds the answers itself (`hosts.rs`).
    pub hosts: Vec<(String, u16)>,
    /// The card's key each of `hosts` was given with (`hosts::keep_one`) — `:granted`'s stamp; none: none known.
    pub hosts_given: std::collections::BTreeMap<(String, u16), char>,
    /// The hosts whose pages Claude's `read` reaches unasked in it (`s` on a read's card, until you quit) — a read's
    /// answer opens no run (`web::tool`).
    pub web_hosts: Vec<(String, u16)>,
    /// Claude's reads of pages under way: what stops them (the turn's end, `web::tool::turn_over`).
    pub web_reads: Vec<crate::web::fetch::Stop>,
    /// The user's `y`s on read cards: the call each was for and the host its pages may reach — tarae's side, never
    /// in Claude's arguments; gone as that read lands or the turn ends.
    pub web_once: Vec<(crate::tools::Call, (String, u16))>,
    /// What its Claude may do past the defaults by the user's `s` on a card (this session — every grant but a
    /// host's, `hosts`), kept with its workspace in `$state/sessions` until it ends or `:sandbox-disallow` takes it
    /// back.
    pub allowed: crate::allow::Allowed,
    /// What its Claude may do, as last made (`Editor::caps_sync_of`) with the generation it was asked as — for
    /// what reads it on the main thread (the card line's shadow); None until the first lands. The gates make
    /// theirs on their workers, from what's answered at that moment.
    pub caps: Option<(u64, std::sync::Arc<crate::caps::Caps>)>,
    /// The last generation of its `caps` asked for: one landing older than what's there is dropped.
    pub caps_asked: u64,
    /// What its last sandboxed run's sandbox keeps apart and shares (9d) — its keeper's, or its Seatbelt policy's —
    /// as the run's policy has it (`Guard::guarantees`: made on the worker that made the policy, `linux::confine`),
    /// kept on the main loop once the run started (`tools::go_now` → `watch::sandbox`; one that didn't spawn
    /// leaves it as it was): the state reads it with no lock or probe. Shown only, never a run's input. None: no
    /// sandboxed run started yet — a keeper is born with the first.
    pub sandbox: Option<crate::sandbox::guarantees::Guarantees>,
    /// The session whose Claude started this one (`orchestra.rs`, `sessions_start`'s `y`) — its parent: only it may
    /// send this one messages, and a child's own chat lists no session tool (no children of children). Kept with
    /// its workspace as the parent's checkout (`worktree::Kept.parent`); one whose parent didn't come back is its own.
    pub parent: Option<crate::event::SessionId>,
    /// Its Claude's `sessions_start` under way — from the call to its answer (`orchestra::answered`, in
    /// `tools::answer_parts`): one at a time, so two calls side by side can't both pass `llm.children-max`.
    pub starting: Vec<crate::tools::Call>,
}

impl Session {
    /// A Claude not started yet: no chat, nothing answered for it, `llm.billing`'s default until its workspace says.
    pub fn new(id: SessionId) -> Session {
        Session {
            id,
            colour: 0,
            chat: None,
            notes: Default::default(),
            checkpoints: Default::default(),
            review: None,
            ask: None,
            board_yes: None,
            turn_end: None,
            tool_waits: Vec::new(),
            no_sandbox: None,
            env_key: None,
            answered: Vec::new(),
            asking: Vec::new(),
            usage: Default::default(),
            idle: Default::default(),
            billing: Default::default(),
            billing_next: None,
            limit_asked: None,
            compacted: Default::default(),
            hosts: Vec::new(),
            hosts_given: Default::default(),
            web_hosts: Vec::new(),
            web_reads: Vec::new(),
            web_once: Vec::new(),
            allowed: Default::default(),
            parent: None,
            starting: Vec::new(),
            caps: None,
            caps_asked: 0,
            sandbox: None,
        }
    }
}
