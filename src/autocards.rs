//! Auto cards' shadow week (R1 of `.sb/card-reviewer-plan.md`, `llm.auto-cards = "shadow"`): each of Claude's cards
//! that `cardreview::AUTO` reviews is read by Haiku as it lands — one stage, the reasons prompt, the verdict on its
//! last line (R0's measure) — and the verdict is kept beside the user's answer in the card record, never acted on.
//!
//! - **Its input** (`prepare`): the card's structured fields (`Input::of`) and the user's own words from tarae's record
//!   of what it sent (`user_words`): the messages you typed (`Msg.typed`, marked where they're sent — a message
//!   tarae composes, another session's Claude's never), the steered words as `steer_take`'s note keeps them — the latest `USER_LATEST` and the first, each cut at
//!   `USER_CUT`; a parent's proposal by the parent's chat. Words not sent yet (a waiting steer, a queued message)
//!   stay out, said in the entry (`unsent`). Never a stream's `user` event, a tool's output or Claude's prose.
//! - **Its run** (`review`): on a worker through `llm::spawn`, in an empty folder of its own, `cardreview::args`'
//!   flags (no tools, no MCP, no settings, `POLICY`), billed to the card's session (`billing::Role::AutoCards`).
//!   `CALL_LIMIT` at most; an error, a timeout or no answer is `none`. A merge (`Offer.rev`) asks again, `REVS_MAX`
//!   revs at most a card.
//! - **Hidden until answered** (CR2): the verdict and its cost wait in `Editor.auto_cards`, in memory only, until the
//!   card is answered (or dropped) and the verdict is in — then one record entry a card, `reviewer` in it (record
//!   v3), its cost on the session's own usage line (`USAGE`). Never `tarae.log` (Claude's `read` may ask for it —
//!   `sandbox::is_taraes_log`), not even after; nothing on screen moves while one runs or waits (`ed.unchanged`).
//!   The record is `sandbox::taraes_own`: no run and no card reads it. A crash loses what's held (90 s at most).
//! - **The week's inputs** go beside the record, redacted (`INPUTS`), to be removed after the week's report.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::cardreview::{Input, Stage, Tokens, Verdict};
use crate::chat::{Chat, Role};
use crate::editor::Editor;
use crate::event::SessionId;
use crate::llm::{self, StreamEv};
use crate::offer::Offer;

/// The reviewer's model (R0's measure: Haiku with the reasons prompt).
pub const MODEL: &str = "haiku";
/// The policy it reads, by `cardreview::Policy::name`: `POLICY` as it is.
pub const POLICY: crate::cardreview::Policy = crate::cardreview::Policy::Base;
/// The most a review takes before it's `none` (`failed: "timeout"`) — R0's call limit.
pub const CALL_LIMIT: Duration = Duration::from_secs(90);
/// The most revs of one card reviewed: a card merged more is `none` (`failed: "merges"`).
pub const REVS_MAX: u32 = 3;
/// The user's latest messages given, and each one's most bytes (start and end kept, `quote::cut`); the conversation's
/// first goes too when it isn't among them.
pub const USER_LATEST: usize = 6;
pub const USER_CUT: usize = 1500;
/// The most of its reasons kept in the record (`said`), redacted.
pub const SAID_CUT: usize = 300;
/// claude's own cap on one review's spend (`--max-budget-usd`): a backstop, R0's ~$0.0005 a card far under it.
const MAX_USD: f64 = 0.05;
/// Its usage line in the card's session.
pub const USAGE: &str = "auto cards";
/// The week's inputs' folder in the record's (`$state/cards/inputs/`), removed after the week's report.
pub const INPUTS: &str = "inputs";

/// `llm.auto-cards`: off, or the shadow week's (R2 adds its values beside).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Off,
    Shadow,
}

impl Mode {
    pub const NAMES: &[&str] = &["off", "shadow"];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Shadow => "shadow",
        }
    }

    pub fn parse(s: &str) -> Mode {
        match s {
            "shadow" => Mode::Shadow,
            _ => Mode::Off,
        }
    }
}

/// What a card's review is given, worked out as it lands (`prepare`).
pub struct Prepared {
    pub input: Input,
    /// The session whose chat's words it judges by (a proposal's: the parent's) — words sent since are counted there.
    chat: SessionId,
    /// Words of the user's waited unsent as it landed (a steer, a queued message).
    unsent: bool,
    /// The user's words sent so far in `chat` as it landed (`sent`): what the reviewer was given.
    sent: usize,
    /// The card's session: it pays.
    billed: SessionId,
    billing: crate::billing::Billing,
}

/// A review's verdict, as the record keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct Reviewed {
    /// None: no verdict (an error, a timeout, past `REVS_MAX`) — never an approve.
    pub verdict: Option<Verdict>,
    pub ms: u64,
    /// This run's own (its result's), None when it ended before one.
    pub tokens: Option<Tokens>,
    /// The card's whole cost, every rev's run ("$ a card").
    pub cost: Option<f64>,
    pub failed: Option<String>,
    /// Its reasons' end, redacted, `SAID_CUT` bytes at most.
    pub said: String,
}

impl Reviewed {
    fn failed(why: &str) -> Reviewed {
        Reviewed {
            verdict: None,
            ms: 0,
            tokens: None,
            cost: None,
            failed: Some(why.to_string()),
            said: String::new(),
        }
    }
}

/// A card's review while its verdict or its answer is still to come (`Editor.auto_cards`, by card id).
pub struct Pending {
    /// How many of the card's revs it has reviewed (`REVS_MAX`).
    revs: u32,
    /// Its run's own number: what an older rev's run still says is dropped.
    pub(crate) run: u64,
    input: Input,
    chat: SessionId,
    unsent: bool,
    billed: SessionId,
    billing: crate::billing::Billing,
    keep: bool,
    started: Instant,
    /// Its answer's text so far.
    text: String,
    /// What its runs cost, every rev's: held until the answer (a session's cost moving would say a review ended).
    spent: Vec<crate::usage::Spent>,
    /// Where this run's own start in `spent`: the older revs' before.
    spent_from: usize,
    pid: Option<u32>,
    folder: Option<Folder>,
    pub reviewed: Option<Reviewed>,
    /// The user's words sent so far in `chat` as the card landed: words sent since — while the reviewer read, or
    /// after its verdict — are ones it never saw (`words_since`).
    sent_at_landing: usize,
    /// The card's record entry, once answered — held until the verdict.
    pub entry: Option<Value>,
    /// The verdict came after the answer.
    late: bool,
}

/// The reviewer's folder: an empty one of tarae's own (0700, in the temp folder, made new), gone on a worker.
struct Folder(std::path::PathBuf);

impl Folder {
    fn made() -> Result<Folder, String> {
        use std::os::unix::fs::DirBuilderExt;
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("tarae-auto-cards-{}-{n}", std::process::id()));
        std::fs::DirBuilder::new().mode(0o700).create(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Folder(dir))
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The user's own words in chat `c`, as tarae sent them, oldest first: the messages you typed (`Msg.typed` — marked
/// where they're sent: the chat input, the ask line, their queue; no slash name, no `/compact`) and the steered words
/// as `steer_take`'s note keeps them. A row tarae composed — a rerun's report (a command's output), a wake, a rebase's
/// ask, a review's template, another session's Claude's message — is never typed; nor a tool's result, Claude's prose,
/// a stream's `user` event.
fn sent_words(c: &Chat) -> impl Iterator<Item = &str> {
    c.msgs.iter().filter(|m| m.typed).filter_map(|m| match m.role {
        Role::User
            if m.from.is_none()
                && crate::chat::slash_named(&m.text).is_none()
                && !crate::chat::is_compact(&m.text) =>
        {
            Some(m.text.as_str())
        }
        Role::Note => m.text.strip_prefix(crate::chat::STEERED),
        _ => None,
    })
}

/// How many of the user's words chat `c` has sent — what tells words sent between a card's landing and its answer.
pub fn sent(c: &Chat) -> usize {
    sent_words(c).count()
}

/// Whether words of the user's wait unsent in chat `c`: a steer, a queued message.
fn unsent(c: &Chat) -> bool {
    c.steer.is_some() || c.queued.is_some()
}

/// What the reviewer is given of chat `c`'s words, the latest first: the latest `USER_LATEST`, and the first when it
/// isn't among them (it usually names the task) — each cut at `USER_CUT`, start and end kept.
pub fn user_words(c: &Chat) -> Vec<String> {
    let all: Vec<&str> = sent_words(c).collect();
    let cut = |t: &str| crate::quote::cut(t, USER_CUT).0;
    let mut out: Vec<String> = all.iter().rev().take(USER_LATEST).map(|t| cut(t)).collect();
    if all.len() > USER_LATEST {
        out.push(cut(all[0]));
    }
    out
}

/// Whether a card landing now is reviewed: shadow on, and a record to keep its verdict in.
fn on(ed: &Editor) -> bool {
    ed.config.llm.auto_cards == Mode::Shadow && ed.config.llm.card_record && ed.cards_dir.is_some()
}

/// Card `o`'s review, worked out as it lands in `ed.workspace` (the card's — a session behind raises its cards with
/// its workspace in front): None while shadow is off, for a card `Input::of` doesn't review, and one with no action.
pub fn prepare(ed: &Editor, o: &Offer) -> Option<Prepared> {
    if !on(ed) {
        return None;
    }
    let whose = match &o.proposed {
        Some(p) => std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|w| w.claude.id == p.parent)?,
        None => &ed.workspace,
    };
    let c = whose.claude.chat.as_ref();
    let user = c.map(user_words).unwrap_or_default();
    let input = Input::of(o, &ed.workspace.root, &user)?;
    if input.action.is_empty() {
        return None;
    }
    Some(Prepared {
        input,
        chat: whose.claude.id,
        unsent: c.is_some_and(unsent),
        sent: c.map_or(0, sent),
        billed: ed.workspace.claude.id,
        billing: ed.workspace.claude.billing,
    })
}

/// The reviewer's flags: `cardreview::args`' (no tools, no MCP, no settings, one turn, `POLICY`), its stream read by
/// `llm::pump` — the model said by its config (`llm::command` adds it).
pub fn args() -> Vec<String> {
    let mut out = Vec::new();
    let mut it = crate::cardreview::args(MODEL, &POLICY.text(), Some(MAX_USD)).into_iter();
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

static GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Card `card` landed (inserted, parked, or merged into the one up, its `rev` bumped — `push_offer`): its review
/// starts, beside it — never delaying it, changing its rank or anything drawn. An older rev's run is dropped, its cost
/// kept.
pub fn review(ed: &mut Editor, card: u64, p: Option<Prepared>) {
    sweep(ed);
    let Some(p) = p else { return };
    let (revs, spent) = match ed.auto_cards.remove(&card) {
        Some(old) => {
            stop(old.pid);
            drop_folder(old.folder);
            (old.revs + 1, old.spent)
        }
        None => (1, Vec::new()),
    };
    let run = GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut pending = Pending {
        revs,
        run,
        input: p.input,
        chat: p.chat,
        unsent: p.unsent,
        billed: p.billed,
        billing: p.billing,
        keep: llm::keeps_key(ed),
        started: crate::clock::now(),
        text: String::new(),
        spent_from: spent.len(),
        spent,
        pid: None,
        folder: None,
        reviewed: None,
        sent_at_landing: p.sent,
        entry: None,
        late: false,
    };
    let cred = match (revs > REVS_MAX, llm::held(ed)) {
        (true, _) => Err("merges"),
        (_, Some(_)) => Err("config unread"),
        _ => ed.credential(p.billing, crate::billing::Role::AutoCards).map_err(|_| "billing"),
    };
    let cred = match cred {
        Ok(c) => c,
        Err(why) => {
            pending.reviewed = Some(Reviewed::failed(why));
            ed.auto_cards.insert(card, pending);
            return;
        }
    };
    let nonce = crate::cardreview::nonce(&pending.input, crate::cardreview::random_pick);
    let message = llm::user_message(&crate::cardreview::prompt(&pending.input, Stage::Two, &nonce));
    ed.auto_cards.insert(card, pending);
    match stand_in(ed) {
        Some(tx) => {
            let _ = tx.send(message);
        }
        None => {
            let cfg = llm::LlmConfig {
                args: args(),
                model: Some(MODEL.into()),
                effort: String::new(),
                keep_env_key: llm::keeps_key(ed),
                ..crate::models::config(ed)
            };
            let (env, tx) = (ed.claude_env.clone(), ed.events.sender());
            ed.events.jobs().spawn(move || {
                let started = (|| -> Result<(u32, Folder), String> {
                    let folder = Folder::made()?;
                    let mut child = llm::spawn(&cfg, &[], &folder.0, env.as_ref(), &cred)
                        .map_err(|e| e.said(&cfg.command, false))?;
                    let pid = child.id();
                    let sent = match child.stdin.take().map(llm::Lines::new) {
                        Some(stdin) => stdin.send(message).map_err(|e| e.to_string()),
                        None => Err("no stdin".into()),
                    };
                    if let Err(e) = sent {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(e);
                    }
                    llm::pump(child, true, tx, move |ev| {
                        Some(Box::new(move |ed: &mut Editor| on_event(ed, card, run, ev)))
                    });
                    Ok((pid, folder))
                })();
                move |ed: &mut Editor| began(ed, card, run, started)
            });
        }
    }
    ed.events.jobs().spawn(move || {
        crate::event::sleep(CALL_LIMIT);
        move |ed: &mut Editor| timed_out(ed, card, run)
    });
}

/// A test's stand-in reviewer (`Editor.auto_cards_told`): no process, the message it's told kept.
#[cfg(test)]
fn stand_in(ed: &Editor) -> Option<std::sync::mpsc::Sender<String>> {
    ed.auto_cards_told.clone()
}
#[cfg(not(test))]
fn stand_in(_: &Editor) -> Option<std::sync::mpsc::Sender<String>> {
    None
}

/// A review's run of card `card`, `run`, the one still wanted: its run.
fn running(ed: &mut Editor, card: u64, run: u64) -> Option<&mut Pending> {
    ed.auto_cards.get_mut(&card).filter(|p| p.run == run && p.reviewed.is_none())
}

/// Its claude started (or didn't — `none`, the reason kept for the record only).
fn began(ed: &mut Editor, card: u64, run: u64, r: Result<(u32, Folder), String>) {
    ed.unchanged = true;
    match r {
        Ok((pid, folder)) => match running(ed, card, run) {
            Some(p) => (p.pid, p.folder) = (Some(pid), Some(folder)),
            // (dropped, or answered by its limit, while it started)
            None => {
                stop(Some(pid));
                drop_folder(Some(folder));
            }
        },
        Err(e) => finish(ed, card, run, Err(e)),
    }
}

/// An event of card `card`'s review `run`. Nothing drawn, nothing logged: what it says waits for the answer.
pub(crate) fn on_event(ed: &mut Editor, card: u64, run: u64, ev: StreamEv) {
    ed.unchanged = true;
    let Some(p) = running(ed, card, run) else { return };
    match ev {
        StreamEv::Text(t) => p.text += &t,
        StreamEv::Spent(s) if !s.counts.any() && s.total_cost.is_none() => {}
        StreamEv::Spent(s) => p.spent.push(s),
        // (the credential it says it runs on — another than its billing's stops it, `billing.rs`; said in the record only)
        StreamEv::Conversation { key_source, .. } => {
            let role = crate::billing::Role::AutoCards;
            if crate::billing::checked(p.billing, p.keep, role, key_source.as_deref()).is_some() {
                stop(p.pid.take());
                finish(ed, card, run, Err("billing".into()));
            }
        }
        StreamEv::Done(Ok(text)) => {
            let text = if text.trim().is_empty() { std::mem::take(&mut p.text) } else { text };
            finish(ed, card, run, Ok(text));
        }
        StreamEv::Done(Err(e)) => finish(ed, card, run, Err(e)),
        StreamEv::Exit(e) => {
            finish(ed, card, run, Err(if e.is_empty() { "claude stopped".into() } else { e }))
        }
        _ => {}
    }
}

/// Its limit came: `none`, its claude stopped.
fn timed_out(ed: &mut Editor, card: u64, run: u64) {
    ed.unchanged = true;
    if let Some(p) = running(ed, card, run) {
        stop(p.pid.take());
        finish(ed, card, run, Err("timeout".into()));
    }
}

/// The end of its reasons — the verdict's line left out —, redacted, its last `SAID_CUT` bytes.
pub fn said(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let reasons = lines.split_last().map_or(&[][..], |(_, r)| r).join("\n");
    let r = crate::tokens::redacted(&reasons);
    let from = r.ceil_char_boundary(r.len().saturating_sub(SAID_CUT));
    r[from..].to_string()
}

/// Session `s`'s chat, wherever its workspace is.
fn chat_of(ed: &Editor, s: SessionId) -> Option<&Chat> {
    std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|w| w.claude.id == s)?.claude.chat.as_ref()
}

/// Card `card`'s review `run` ended with `r` (its answer's text, or why not): the verdict held — written now if the
/// card was answered already. Words of the user's waiting unsent now are `unsent` too: at landing or at the verdict.
pub fn finish(ed: &mut Editor, card: u64, run: u64, r: Result<String, String>) {
    let Some(chat) =
        ed.auto_cards.get(&card).filter(|p| p.run == run && p.reviewed.is_none()).map(|p| p.chat)
    else {
        return;
    };
    let unsent_now = chat_of(ed, chat).is_some_and(unsent);
    let Some(p) = ed.auto_cards.get_mut(&card) else { return };
    let ms = crate::clock::since(p.started).as_millis() as u64;
    let tokens = p.spent[p.spent_from..].last().map(|s| Tokens {
        input: s.counts.input.unwrap_or(0),
        output: s.counts.output.unwrap_or(0),
        cache_read: s.counts.read.unwrap_or(0),
        cache_write: s.counts.write.unwrap_or(0),
    });
    let costs: Vec<f64> = p.spent.iter().filter_map(|s| s.total_cost).collect();
    let cost = (!costs.is_empty()).then(|| costs.iter().sum());
    p.reviewed = Some(match r {
        Ok(text) => Reviewed {
            verdict: Some(crate::cardreview::verdict(Stage::Two, &text)),
            ms,
            tokens,
            cost,
            failed: None,
            said: said(&text),
        },
        Err(e) => Reviewed { ms, tokens, cost, ..Reviewed::failed(&crate::tokens::redacted(&e)) },
    });
    p.unsent |= unsent_now;
    p.late = p.entry.is_some();
    drop_folder(p.folder.take());
    if p.entry.is_some() {
        write(ed, card);
    }
}

/// Card `card` was answered (or dropped) and `entry` is its record entry: back to be written now when no review is
/// held for it — else held until its verdict (written then, `reviewer` in it); with shadow turned off since, written
/// at once as `none` (`failed: "off"`).
pub fn answered(ed: &mut Editor, card: u64, entry: Value) -> Option<Value> {
    let Some(p) = ed.auto_cards.get_mut(&card) else { return Some(entry) };
    let words_since =
        |ed: &Editor, p: &Pending| chat_of(ed, p.chat).is_some_and(|c| sent(c) != p.sent_at_landing);
    p.entry = Some(entry);
    if ed.config.llm.auto_cards != Mode::Shadow && p.reviewed.is_none() {
        stop(p.pid.take());
        p.reviewed = Some(Reviewed::failed("off"));
    }
    p.reviewed.as_ref()?;
    let since = words_since(ed, &ed.auto_cards[&card]);
    let p = ed.auto_cards.remove(&card)?;
    Some(written(ed, card, p, Some(since)))
}

/// Card `card`'s held entry and verdict, both in: written.
fn write(ed: &mut Editor, card: u64) {
    let Some(p) = ed.auto_cards.remove(&card) else { return };
    let entry = written(ed, card, p, None);
    ed.cards_record(vec![entry]);
}

/// `p`'s entry with its `reviewer` (record v3), its input kept beside (`INPUTS`), its cost on its session's usage
/// line — `since`: words were sent between the card's landing and its answer (None: the verdict came after it).
fn written(ed: &mut Editor, card: u64, p: Pending, since: Option<bool>) -> Value {
    let r = p.reviewed.unwrap_or_else(|| Reviewed::failed("quit"));
    let mut entry = p.entry.unwrap_or(Value::Null);
    entry["reviewer"] = reviewer(&r, p.unsent, p.late, since.filter(|_| !p.late), p.revs);
    let input = json!({
        "at": entry["at"],
        "session": entry["session"],
        "card": card,
        "input": input_json(&p.input),
    });
    ed.cards_record_in(INPUTS, vec![input]);
    aside(ed, p.billed, p.spent);
    entry
}

/// What a card's reviews cost, on session `billed`'s own usage line (`USAGE`).
fn aside(ed: &mut Editor, billed: SessionId, spent: Vec<crate::usage::Spent>) {
    if spent.is_empty() {
        return;
    }
    let Some(w) =
        std::iter::once(&mut ed.workspace).chain(&mut ed.workspaces).find(|w| w.claude.id == billed)
    else {
        return;
    };
    for s in spent {
        w.claude.usage.aside(s, USAGE);
    }
    ed.calls_record();
    crate::usage::view_sync(ed);
}

/// Reviews whose card left with no answer — replaced by a newer one (one bring or end card a checkout), withdrawn by
/// what it asked about (a bring queue's, a rebase's, a worktree's or a fork's own end) — that no answer path saw
/// (`answered`): its run stopped, its spend put on its session's line; no entry, as a card left unanswered keeps none.
/// At each `review` and at `quit`.
fn sweep(ed: &mut Editor) {
    let up: std::collections::HashSet<u64> = std::iter::once(&ed.workspace)
        .chain(&ed.workspaces)
        .flat_map(|w| &w.parked)
        .chain(&ed.offers)
        .map(|o| o.id)
        .collect();
    let gone: Vec<u64> = ed
        .auto_cards
        .iter()
        .filter(|(id, p)| p.entry.is_none() && !up.contains(id))
        .map(|(id, _)| *id)
        .collect();
    for card in gone {
        let Some(mut p) = ed.auto_cards.remove(&card) else { continue };
        stop(p.pid.take());
        drop_folder(p.folder.take());
        aside(ed, p.billed, p.spent);
    }
}

/// The record's `reviewer`: `{ verdict: approve|ask|none, model, policy, ms, tokens?, cost?, said, failed?, unsent?,
/// late?, words_since?, revs? }` — `tokens` the last run's own, `cost` every rev's.
pub fn reviewer(r: &Reviewed, unsent: bool, late: bool, since: Option<bool>, revs: u32) -> Value {
    let mut v = json!({
        "verdict": r.verdict.map_or("none", Verdict::word),
        "model": MODEL,
        "policy": POLICY.name(),
        "ms": r.ms,
        "tokens": r.tokens.as_ref().map(|t| json!({ "prompt": t.prompt(), "output": t.output })),
        "cost": r.cost,
        "said": r.said,
        "failed": r.failed,
        "unsent": unsent.then_some(true),
        "late": late.then_some(true),
        "words_since": since,
        "revs": (revs > 1).then_some(revs.min(REVS_MAX + 1)),
    });
    if let Some(o) = v.as_object_mut() {
        o.retain(|_, x| !x.is_null() && x.as_str() != Some(""));
    }
    v
}

/// An `Input` as the week's inputs keep it (every string redacted already, `Input::of`).
pub fn input_json(i: &Input) -> Value {
    let action: Vec<Value> = i.action.iter().map(|(f, v)| json!([f.tag(), v])).collect();
    json!({ "kind": i.kind, "why": i.why, "root": i.root, "action": action, "refused": i.refused, "user": i.user })
}

/// When tarae ends: the answered cards' entries still waiting for a verdict written (`none`, `failed: "quit"`), the
/// rest's reviews stopped, their spend on their sessions' lines — a card left unanswered keeps no entry, as before.
pub fn quit(ed: &mut Editor) {
    let cards: Vec<u64> = ed.auto_cards.keys().copied().collect();
    let mut entries = Vec::new();
    for card in cards {
        let Some(mut p) = ed.auto_cards.remove(&card) else { continue };
        stop(p.pid.take());
        match p.entry.is_some() {
            true => {
                p.reviewed.get_or_insert_with(|| Reviewed::failed("quit"));
                entries.push(written(ed, card, p, None));
            }
            false => {
                drop_folder(p.folder.take());
                aside(ed, p.billed, p.spent);
            }
        }
    }
    ed.cards_record(entries);
}

/// A review's claude stopped (`llm::pump` reaps it).
fn stop(pid: Option<u32>) {
    if let Some(pid) = pid {
        llm::kill(pid);
    }
}

fn drop_folder(f: Option<Folder>) {
    if let Some(f) = f {
        crate::event::drop_later(f);
    }
}

#[cfg(test)]
mod tests;
