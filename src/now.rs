//! What Claude is doing now, in words (the user, 2026-10-26 — motion's `.sb/now-plan.md`): the step its turn is at,
//! with that step's own time, its last finished sentence (its why) and the plan's step — the status line's one line,
//! right after the mode word, while the front session's turn runs (`term::draw_statusline`). Read from what's streamed
//! and nothing else; no token is spent on it.
//!
//! `step` is the one place a turn's doing is put in words: the line's verb (`running`, `asks to run`, `queued`) and
//! the top lines' word (`Step::word` — the chat's line, the follow card's head; `exploring` went with this). Its time
//! starts when the step does (`Since`, kept on the chat by `sync` after every event), never the turn's.

use std::time::{Duration, Instant};

use crate::chat::{Chat, Mark, Role};
use crate::editor::Editor;
use crate::workspace::{Doing, Workspace};

/// What stands before a step's words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    /// A stop's mark, motion's one vocabulary (`◦ • ● ▸`).
    Mark(Mark),
    /// Working, no call under way: the working sign's one cell, moving by the turn's pace (`shimmer.rs`,
    /// `term::working`).
    Working,
}

/// The step a turn is at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub sign: Sign,
    /// As the line says it: `running`, `asks to run`, `waits for your review of`, `queued`.
    pub verb: &'static str,
    /// What it acts on, whole — a command, a file and its lines, a pattern; empty: nothing.
    pub what: String,
    /// It waits on you, or may be stuck: its sign in warning.
    pub warn: bool,
    /// Its time when it isn't the step's own: a call possibly stuck counts from when it was made (`Doing::Stuck`).
    pub took: Option<Duration>,
    /// Which step it is: another starts its time again.
    pub key: String,
}

impl Step {
    fn new(sign: Sign, verb: &'static str, what: impl Into<String>, key: impl Into<String>) -> Step {
        Step { sign, verb, what: what.into(), warn: false, took: None, key: key.into() }
    }

    /// The word the top lines say for it (the chat's, the follow card's head) beside the turn's time: the verb's
    /// first word, what it waits on said there by its state (`▲ waits`).
    pub fn word(&self) -> &'static str {
        match self.verb {
            v if self.warn && v != "stuck?" => "waiting",
            "looking up" => "looking up",
            v => v.split(' ').next().unwrap_or(v),
        }
    }
}

/// The step session `l`'s turn is at — None: no turn under way.
pub fn step(ed: &Editor, l: &Workspace) -> Option<Step> {
    let c = l.claude.chat.as_ref().filter(|c| c.busy())?;
    let turn = turn_start(c);
    if crate::compaction::under_way(l) {
        return Some(Step::new(Sign::Working, "compacting", "", "compact"));
    }
    if c.settings.waiting() {
        return Some(Step::new(Sign::Working, "starting", "", format!("start{turn}")));
    }
    // (its newest call with no result yet, as its row says it)
    let live = c.msgs[turn..]
        .iter()
        .rev()
        .find(|m| m.role == Role::Tool && c.calls.iter().any(|k| k.id == m.call))
        .and_then(|m| m.look.as_ref().map(|look| (m.call.as_str(), look)));
    let place = |look: &crate::chat::Look| match look.range.is_empty() || look.verb == "run" {
        true => look.what.clone(),
        false => format!("{} {}", look.what, look.range),
    };
    match ed.doing(l) {
        Doing::Waiting => {
            let (sign, verb, what) = match live {
                Some((_, look)) => match look.verb {
                    "run" | "test" => (Sign::Mark(Mark::Run), "asks to run", look.what.clone()),
                    "edit" | "rename" | "replace" if l.claude.review.as_ref().is_some_and(|r| r.ready) => {
                        (Sign::Mark(Mark::Edit), "waits for your review of", look.what.clone())
                    }
                    "edit" | "rename" | "replace" => (Sign::Mark(Mark::Edit), "asks to change", place(look)),
                    "read" | "search" | "grep" | "glob" => {
                        (Sign::Mark(Mark::Read), "asks to read", place(look))
                    }
                    _ => (Sign::Mark(look.mark()), "waits for your answer", String::new()),
                },
                None if l.claude.review.as_ref().is_some_and(|r| r.ready) => {
                    (Sign::Mark(Mark::Edit), "waits for your review", String::new())
                }
                None => (Sign::Mark(Mark::Lookup), "waits for your answer", String::new()),
            };
            let call = live.map_or("", |(id, _)| id);
            let mut s = Step::new(sign, verb, what, format!("ask:{turn}:{call}"));
            s.warn = true;
            return Some(s);
        }
        Doing::Queued { pos } if live.is_some_and(|(_, look)| matches!(look.verb, "run" | "test")) => {
            let (call, look) = live?;
            let what = format!("{} · {}", crate::queued::ordinal(pos), look.what);
            return Some(Step::new(Sign::Mark(Mark::Run), "queued", what, format!("queued:{call}")));
        }
        Doing::Stuck { tool, secs } => {
            let mut s = Step::new(Sign::Mark(Mark::Read), "stuck?", tool, format!("stuck:{turn}:{tool}"));
            (s.warn, s.took) = (true, Some(Duration::from_secs(secs)));
            return Some(s);
        }
        _ => {}
    }
    if let Some((call, look)) = live {
        let (verb, what) = match look.verb {
            "run" => ("running", look.what.clone()),
            "test" => ("testing", look.what.clone()),
            "read" => ("reading", place(look)),
            "search" | "grep" | "glob" => ("searching", place(look)),
            "references" | "definition" | "symbols" => ("looking up", look.what.clone()),
            "diagnostics" => ("checking", look.what.clone()),
            "edit" => ("editing", place(look)),
            "rename" => ("renaming", look.what.clone()),
            "replace" => ("replacing", look.what.clone()),
            "note" => ("noting", look.what.clone()),
            "reply" => ("replying", look.what.clone()),
            "plan" => ("planning", look.what.clone()),
            "git" => ("reading git", look.what.clone()),
            "ui" => ("looking at", look.what.clone()),
            "start" => ("starting", look.what.clone()),
            "send" => ("messaging", look.what.clone()),
            _ => ("using", look.what.clone()),
        };
        return Some(Step::new(Sign::Mark(look.mark()), verb, what, format!("call:{call}")));
    }
    // (an edit's input streaming — its call not made yet: drawn in its buffer as it comes, `writing.rs`)
    if let Some(w) = c.writing.calls.iter().rev().find(|w| !w.done) {
        let file = w.items.last().map(|it| it.file.s.trim().to_string()).unwrap_or_default();
        return Some(Step::new(Sign::Mark(Mark::Edit), "writing", file, format!("edit:{}", w.id)));
    }
    let at = c.msgs.len();
    // (its words or its thinking's summary arriving, stood still, or working where nothing streams: the one cell, its
    // pace saying which)
    Some(match c.state == crate::chat::State::Writing {
        true => Step::new(Sign::Working, "writing", "", format!("write{at}")),
        false => Step::new(Sign::Working, "thinking", "", format!("think{at}")),
    })
}

/// A press on the status line's word of it: follow Claude's place — follow on again, or resumed from a pause (`C-f`'s
/// act from the chat, `space F`'s from the code); following already, the view back where Claude is.
pub fn press(ed: &mut Editor) {
    crate::follow::follow_session(ed, ed.workspace.id);
}

/// Where the turn under way starts in `c.msgs` — after the user's message that began it.
fn turn_start(c: &Chat) -> usize {
    c.msgs.iter().rposition(|m| m.role == Role::User).map_or(0, |i| i + 1)
}

/// The step a chat's turn is at, by its key, and since when (`Chat.now`).
#[derive(Clone, Debug, Default)]
pub struct Since {
    key: String,
    at: Option<Instant>,
    /// When, for whoever reads the state (`usage::now_ms`'s clock).
    pub at_ms: u64,
}

/// After every event: the front session's step, its time started again when it's another step.
pub fn sync(ed: &mut Editor) {
    let key = step(ed, &ed.workspace).map(|s| s.key);
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    match key {
        Some(k) if k != c.now.key || c.now.at.is_none() => {
            c.now = Since { key: k, at: Some(crate::clock::now()), at_ms: crate::usage::now_ms() }
        }
        Some(_) => {}
        None => c.now = Since::default(),
    }
}

/// How long session `l`'s step `s` has gone on.
pub fn took(l: &Workspace, s: &Step) -> Duration {
    if let Some(t) = s.took {
        return t;
    }
    let since = l.claude.chat.as_ref().filter(|c| c.now.key == s.key).and_then(|c| c.now.at);
    since.map_or(Duration::ZERO, crate::clock::since)
}

/// What the state says of session `l`'s turn (`turn.now`): its step in the line's words, since when, Claude's last
/// sentence — None: no turn under way.
pub fn state(ed: &Editor, l: &Workspace) -> Option<serde_json::Value> {
    let s = step(ed, l)?;
    let c = l.claude.chat.as_ref()?;
    let since = (c.now.key == s.key).then_some(c.now.at_ms);
    let said = said(c).map(|(s, _)| s);
    let text = format!("{} {}", s.verb, s.what).trim().to_string();
    Some(
        serde_json::json!({ "step": text, "waits": s.warn, "since_ms": since, "said": said, "plan": plan(c) }),
    )
}

/// The last sentence Claude finished this turn — its why: from its prose; until its prose's first sentence ends, its
/// thinking's summary's (`true`: a thought's, drawn in italic). It changes only as a sentence ends, never with each
/// token.
pub fn said(c: &Chat) -> Option<(String, bool)> {
    let turn = &c.msgs[turn_start(c)..];
    // (the newest message is still coming while its words or its thinking block are)
    let last = turn.len().checked_sub(1);
    let coming = |m: &crate::chat::Msg| match m.role {
        Role::Thought => c.pace.thinking,
        _ => c.state == crate::chat::State::Writing,
    };
    let whole = |i: usize| Some(i) != last || !coming(&turn[i]);
    let finished = |role: Role| {
        turn.iter()
            .enumerate()
            .rev()
            .filter(|(_, m)| m.role == role)
            .find_map(|(i, m)| last_sentence(&crate::chat::displayed(&m.text), whole(i)))
    };
    match finished(Role::Assistant) {
        Some(s) => Some((s, false)),
        None => finished(Role::Thought).map(|s| (s, true)),
    }
}

/// The plan's step in hand, `3/4 Run the unit tests` — None: no plan, or every step done.
pub fn plan(c: &Chat) -> Option<String> {
    let p = c.plan.as_ref()?;
    let i = p.current()?;
    Some(format!("{}/{} {}", i + 1, p.steps.len(), p.steps[i].text))
}

/// The last finished sentence of markdown `text` — `whole`: nothing more of it comes, so its last sentence is finished
/// too. A sentence ends at `.` `?` `!` `:` before a blank or a line's end, or with its paragraph — which a blank line, a
/// code fence (its block still streaming too) or the next block (a list item, a heading, a quote) ends, never a soft line
/// break; code blocks are no one's sentences; emphasis and code marks are taken off.
pub fn last_sentence(text: &str, whole: bool) -> Option<String> {
    // Paragraphs: their lines joined, code blocks left out (and ending the one before)
    let mut paragraphs: Vec<String> = Vec::new();
    let mut open = false;
    let mut code = false;
    // (the last paragraph ended by a blank line or a fence after it — the next block's start pushes one of its own)
    let mut ended = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            code = !code;
            (open, ended) = (false, true);
            continue;
        }
        if code || t.is_empty() {
            (open, ended) = (false, true);
            continue;
        }
        ended = false;
        let item = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("> "));
        let numbered =
            t.split_once(". ").filter(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        let heading = t.starts_with('#').then(|| t.trim_start_matches('#').trim());
        let starts = item.or(numbered.map(|(_, r)| r)).or(heading);
        let body = starts.unwrap_or(t);
        match open && starts.is_none() {
            true => {
                let p = paragraphs.last_mut()?;
                p.push(' ');
                p.push_str(body);
            }
            false => paragraphs.push(body.to_string()),
        }
        open = heading.is_none();
    }
    // (the last paragraph ends where nothing more comes, or a blank line or a fence came after it — a line break alone
    // only ends the line: a stop before it ends its sentence)
    let closed_last = whole || ended;
    let line_ended = text.ends_with('\n');
    let n = paragraphs.len();
    for (k, p) in paragraphs.iter().enumerate().rev() {
        let plain: String = p.chars().filter(|c| !matches!(c, '*' | '`')).collect();
        let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
        let closed = k + 1 < n || closed_last;
        let mut ends: Vec<usize> = plain
            .match_indices(['.', '?', '!', ':'])
            .map(|(i, m)| i + m.len())
            .filter(|&i| plain[i..].starts_with(' ') || i == plain.len() && line_ended)
            .collect();
        if closed && !plain.is_empty() && ends.last() != Some(&plain.len()) {
            ends.push(plain.len());
        }
        if let Some(&end) = ends.last() {
            let start = ends.len().checked_sub(2).map_or(0, |j| ends[j]);
            let s = plain[start..end].trim();
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
