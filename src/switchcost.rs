//! What a switch of a conversation's billing, model or effort costs its prompt cache (apibill D6,
//! `.sb/apibill-design.md`): the next request writes the whole context again when the switch throws the
//! cache away — paid at the write price on your API key, counted toward the plan's limits on the subscription.
//!
//! - **Billing** always does: "Caches are isolated between organizations" (the prompt-caching docs; L3 measured
//!   it). **A model** starts cold. **Effort** as each model's row in the price table says (`Effort`).
//! - **Only a warm cache costs anything** (`usage::Usage::cache`): one gone cold is written again by staying too.
//! - **Dollars need prices**: the tools pack's `[[claude.price]]` (`sandboxpack`, built-in only), per model, the
//!   cache write per million tokens for the 5-minute and the hour-long cache. A model it doesn't list is said in
//!   tokens only; the subscription is always tokens. An API chat's cache lives an hour (`billing.rs`), so a switch
//!   onto the key is priced at the hour-long write.
//!
//! Said three ways: on the `$` card and the limit card (`said`), on the toast after a switch, and over the chat's
//! input until the next message goes (`line`: "next message rewrites ≈ $0.75 (model changed) · switching back
//! costs nothing" — nothing has gone out since, so going back to what it was costs nothing).

use crate::billing::Billing;
use crate::editor::Editor;
use crate::usage::{Cache, Usage};

/// What a change of effort does to a model's cache (`[[claude.price]]`'s `effort`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effort {
    /// Kept — Claude Code says so (Fable 5.1: "`/effort` … no longer invalidates the prompt cache", 2.1.260).
    Keeps,
    /// Written again.
    Rewrites,
    /// The API can keep it (per-message effort); whether Claude Code does isn't checked (L7) — "may rewrite".
    Unknown,
}

/// One row of the price table: a model, and what writing its cache costs per million tokens.
#[derive(Clone, Debug, PartialEq)]
pub struct Price {
    /// The model's id as claude names it, without a date or a `[1m]`-style suffix (`claude-opus-5-5`).
    pub model: String,
    /// The row holds for prompts up to this many tokens (Haiku 5.5's two tiers) — None: any.
    pub up_to: Option<u64>,
    pub write_5m: f64,
    pub write_1h: f64,
    /// A cache hit (and refresh).
    pub read: f64,
    pub effort: Effort,
}

/// `[[claude.price]]`'s rows, each checked: Err names the first one wrong.
pub fn read(v: &toml::Value) -> Result<Vec<Price>, String> {
    let rows = v.as_array().ok_or("`price` isn't a list of tables")?;
    rows.iter()
        .map(|r| {
            let t = r.as_table().ok_or("a `price` row isn't a table")?;
            let keys = ["model", "up-to", "write-5m", "write-1h", "read", "effort"];
            if let Some(k) = t.keys().find(|k| !keys.contains(&k.as_str())) {
                return Err(format!("a `price` row: no key `{k}` — {}", keys.join(", ")));
            }
            let model = t.get("model").and_then(toml::Value::as_str).ok_or("a `price` row has no `model`")?;
            let dollars = |k: &str| {
                let n = match t.get(k) {
                    Some(toml::Value::Float(f)) => *f,
                    Some(toml::Value::Integer(i)) => *i as f64,
                    _ => return Err(format!("`{model}`: no `{k}` (dollars per million tokens)")),
                };
                (n.is_finite() && n >= 0.0).then_some(n).ok_or(format!("`{model}`: `{k}` isn't a price"))
            };
            let up_to = match t.get("up-to") {
                None => None,
                Some(n) => Some(
                    n.as_integer()
                        .and_then(|n| u64::try_from(n).ok())
                        .ok_or(format!("`{model}`: `up-to` isn't a count of tokens"))?,
                ),
            };
            let effort = match t.get("effort").and_then(toml::Value::as_str) {
                Some("keeps") => Effort::Keeps,
                Some("rewrites") => Effort::Rewrites,
                Some("unknown") | None => Effort::Unknown,
                Some(e) => return Err(format!("`{model}`: effort `{e}` — keeps, rewrites or unknown")),
            };
            Ok(Price {
                model: model.to_string(),
                up_to,
                write_5m: dollars("write-5m")?,
                write_1h: dollars("write-1h")?,
                read: dollars("read")?,
                effort,
            })
        })
        .collect()
}

/// `id` without a `[1m]`-style suffix and a trailing date (`claude-haiku-4-5-20251001` → `claude-haiku-4-5`).
fn bare(id: &str) -> &str {
    let id = id.split('[').next().unwrap_or(id);
    match id.rsplit_once('-') {
        Some((head, date)) if date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => id,
    }
}

/// The row for model `id` with a context of `tokens` — the id whole, never a prefix (`claude-opus-5` isn't
/// `claude-opus-5-5`'s).
pub fn price_of<'a>(prices: &'a [Price], id: &str, tokens: u64) -> Option<&'a Price> {
    let id = bare(id);
    prices.iter().find(|p| p.model == id && p.up_to.is_none_or(|n| tokens <= n))
}

/// The project's instructions a fork writes again when the source's first request wasn't seen here (a conversation
/// continued): the source's own cold first write in the live check of 2026-11-08, this repository's.
pub const INSTRUCTIONS_MEASURED: u64 = 95_104;

/// What a fork's first request carries and costs (`fork.rs`), as a live fork measured it (2026-11-08): it reads the
/// source's whole context from the cache — the system prompt is the same prefix, the other checkout notwithstanding —
/// and then writes the project's instructions a second time (CLAUDE.md injected again for the fork's folder, about the
/// size of the source's own first write), so its context starts at about the two together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForkCost {
    /// The source's context, read from the cache.
    pub read: u64,
    /// The instructions written again — `seen`: the source's first request's cache write, else `INSTRUCTIONS_MEASURED`.
    pub write: u64,
    pub seen: bool,
    /// The read and the hour-long write (an API chat's cache) at the table's prices — None: a model it doesn't price.
    pub dollars: Option<f64>,
}

impl ForkCost {
    /// The fork's context as its first request leaves it.
    pub fn starts(&self) -> u64 {
        self.read + self.write
    }
}

/// What a fork of a source whose context is `context` costs — `first_write`: its first request's cache write, where
/// it was seen. None: no request reported yet, its size unknown.
pub fn fork_cost(
    context: Option<u64>,
    first_write: Option<u64>,
    model: &str,
    prices: &[Price],
) -> Option<ForkCost> {
    let read = context?;
    let (write, seen) = first_write.map_or((INSTRUCTIONS_MEASURED, false), |w| (w, true));
    let dollars = price_of(prices, model, read + write)
        .map(|p| (read as f64 * p.read + write as f64 * p.write_1h) / 1e6);
    Some(ForkCost { read, write, seen, dollars })
}

/// As the fork card says it after `Its conversation: main's, at turn 3 — `: `read again, plus the project's
/// instructions a second time (≈ 94k tokens): it starts at ≈ 200k, about twice main's. Its first message ≈ API
/// $0.77.` — money as the chat's row says it: `≈ $0.77` on the key, `≈ API $0.77` where the session isn't on it (what
/// the API would charge, not what you pay).
pub fn fork_said(c: &ForkCost, to: Billing, src: &str) -> String {
    let tokens = crate::usage::tokens;
    let instructions = match c.seen {
        true => format!("≈ {} tokens", tokens(c.write)),
        false => format!("not seen here: ≈ {} tokens, as once measured", tokens(c.write)),
    };
    let times = c.starts() as f64 / c.read.max(1) as f64;
    let times = match times {
        t if t >= 2.5 => format!(", about {t:.0}× {src}'s"),
        t if t >= 1.75 => format!(", about twice {src}'s"),
        t if c.read > 0 && t >= 1.05 => format!(", about {t:.1}× {src}'s"),
        _ => String::new(),
    };
    let first = match c.dollars {
        Some(d) if to == Billing::Api => format!("Its first message ≈ {}.", crate::usage::dollars(d)),
        Some(d) => format!("Its first message ≈ API {}.", crate::usage::dollars(d)),
        None if to == Billing::Api => "Its first message: no price for this model.".into(),
        None => "Its first message counts toward your plan's limits.".into(),
    };
    format!(
        "read again, plus the project's instructions a second time ({instructions}): it starts at ≈ {}{times}. {first}",
        tokens(c.starts())
    )
}

/// What the next request costs after a switch, against staying.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cost {
    /// Nothing cached yet: no conversation, or no request in it.
    Nothing,
    /// The cache went cold at `since_ms`: staying writes it again too.
    Cold { since_ms: u64 },
    /// The CLI doesn't say how warm it is.
    NotReported,
    /// Warm until `until_ms`: `tokens` written again — `dollars` on the key, at a price the table has.
    Warm { tokens: u64, dollars: Option<f64>, until_ms: u64 },
    /// It stays warm (an effort change on a model that keeps its cache).
    Kept,
}

/// The cost of the next request of `u`'s conversation at `now_ms`, billed to `to` on model `model` (an id as
/// claude names it). On the key, the chat's hour-long write.
pub fn cost(u: &Usage, now_ms: u64, to: Billing, model: &str, prices: &[Price]) -> Cost {
    if u.conv.as_ref().is_none_or(|c| c.last.is_none()) {
        return Cost::Nothing;
    }
    match u.cache(now_ms) {
        Cache::NotReported => Cost::NotReported,
        Cache::Cold { since_ms } => Cost::Cold { since_ms },
        Cache::Warm { left_ms, .. } => {
            let tokens = u.context().map_or(0, |c| c.0);
            let dollars = match to {
                Billing::Api => price_of(prices, model, tokens).map(|p| tokens as f64 * p.write_1h / 1e6),
                Billing::Subscription => None,
            };
            Cost::Warm { tokens, dollars, until_ms: now_ms + left_ms }
        }
    }
}

impl Cost {
    /// As a card says it: `rewrites ≈ $1.20 of cached context (150k tokens)` · `rewrites ≈ 150k tokens of cached
    /// context, toward your plan's limits` · `no extra cost — the cache went cold at 14:32` …
    pub fn said(self, to: Billing, zone: crate::log::Zone) -> String {
        match self {
            Cost::Warm { tokens, dollars: Some(d), .. } => {
                format!(
                    "rewrites ≈ {} of cached context ({} tokens)",
                    crate::usage::dollars(d),
                    crate::usage::tokens(tokens)
                )
            }
            Cost::Warm { tokens, dollars: None, .. } => match to {
                Billing::Api => format!(
                    "rewrites ≈ {} tokens of cached context (no price for this model)",
                    crate::usage::tokens(tokens)
                ),
                Billing::Subscription => format!(
                    "rewrites ≈ {} tokens of cached context, toward your plan's limits",
                    crate::usage::tokens(tokens)
                ),
            },
            Cost::Cold { since_ms } => {
                format!("no extra cost — the cache went cold at {}", crate::usage::until(zone, since_ms))
            }
            Cost::Nothing => "no extra cost — nothing is cached yet".into(),
            Cost::NotReported => "the cache isn't reported — what it costs can't be said".into(),
            Cost::Kept => "keeps the cache".into(),
        }
    }

    /// As the line over the chat's input says it, short: `rewrites ≈ $1.20` · `rewrites ≈ 150k tokens` …
    fn short(self, zone: crate::log::Zone) -> String {
        match self {
            Cost::Warm { dollars: Some(d), .. } => format!("rewrites ≈ {}", crate::usage::dollars(d)),
            Cost::Warm { tokens, .. } => format!("rewrites ≈ {} tokens", crate::usage::tokens(tokens)),
            Cost::Cold { since_ms } => {
                format!("costs nothing extra — cold since {}", crate::usage::until(zone, since_ms))
            }
            Cost::Nothing => "costs nothing extra".into(),
            Cost::NotReported => "costs what can't be said — the cache isn't reported".into(),
            Cost::Kept => "keeps the cache".into(),
        }
    }
}

/// What a conversation runs on, as far as its cache goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    pub billing: Billing,
    /// The model chosen (`llm.model`'s value: `default`, `opus`, an id).
    pub model: String,
    /// `llm.effort` (empty: the model's own).
    pub effort: String,
}

impl Editor {
    /// What the session in front's conversation runs on now.
    pub fn setup(&self) -> Setup {
        Setup {
            billing: self.workspace.claude.billing,
            model: crate::models::of(self).model.unwrap_or_else(|| "default".into()),
            effort: crate::models::of(self).effort,
        }
    }

    /// The id of the model value `value` stands for, as claude named it (its `initialize` answer) — else, the
    /// value as it was, what answered last.
    pub(crate) fn model_id(&self, value: &str, before: &Setup) -> String {
        let usage = &self.workspace.claude.usage;
        let last = usage.conv.as_ref().and_then(|c| c.last.as_ref()).map(|r| r.model.clone());
        let chat = self.workspace.claude.chat.as_ref();
        let resolved = chat
            .and_then(|c| c.models.iter().find(|m| m.value == value))
            .map(|m| m.resolved.clone())
            .filter(|r| !r.is_empty());
        let answered = chat.map(|c| c.model.clone()).filter(|m| !m.is_empty());
        resolved
            .or_else(|| (value == before.model).then(|| last.or(answered)).flatten())
            .unwrap_or_else(|| value.to_string())
    }

    /// What the next message costs now that `before` became what's set (`setup`). None: nothing changed, or no
    /// conversation to cost anything.
    pub fn switch_cost(&self, before: &Setup) -> Option<Switch> {
        let now = self.setup();
        let changed: Vec<&'static str> = [
            (before.billing != now.billing, "billing"),
            (before.model != now.model, "model"),
            (before.effort != now.effort, "effort"),
        ]
        .into_iter()
        .filter_map(|(c, w)| c.then_some(w))
        .collect();
        if changed.is_empty() {
            return None;
        }
        let prices = &crate::sandboxpack::get().price;
        let (u, at) = (&self.workspace.claude.usage, crate::usage::now_ms());
        let model = self.model_id(&now.model, before);
        let mut s = Switch { cost: cost(u, at, now.billing, &model, prices), changed, maybe: false };
        if s.cost == Cost::Nothing {
            return None;
        }
        // (effort alone: as the model's row says — kept, written again, or not known which)
        if s.changed == ["effort"] && matches!(s.cost, Cost::Warm { .. }) {
            let tokens = u.context().map_or(0, |c| c.0);
            match price_of(prices, &model, tokens).map(|p| p.effort) {
                Some(Effort::Keeps) => s.cost = Cost::Kept,
                Some(Effort::Rewrites) => {}
                Some(Effort::Unknown) | None => s.maybe = true,
            }
        }
        Some(s)
    }

    /// A switch of billing, model or effort is about to change the session in front's conversation: what it ran
    /// on until now is kept for the line over the input — the first switch since the last message counts.
    pub fn switching(&mut self) {
        let now = self.setup();
        if let Some(c) = self.workspace.claude.chat.as_mut().filter(|c| c.conversation.is_some()) {
            c.switched.get_or_insert(now);
        }
    }

    /// What a toast after a switch ends with: ` — next message rewrites ≈ $1.20` — empty when there's nothing to
    /// say (no conversation, or back where it was).
    pub fn switched_said(&self) -> String {
        self.switch_now()
            .map(|s| format!(" — next message {}", s.what(self.interface.zone)))
            .unwrap_or_default()
    }

    /// The line over the chat's input after a switch, until the next message goes: `next message rewrites ≈
    /// $0.75 (model changed) · switching back costs nothing`. None: no switch since, or back where it was.
    #[cfg(test)]
    pub fn switch_line(&self) -> Option<String> {
        let (what, changed, back) = self.switch_parts()?;
        let back = if back.is_empty() { String::new() } else { format!(" · {back}") };
        Some(format!("{what} {changed}{back}"))
    }

    /// `switch_line` in its parts, for folding when narrow: what the next message costs, what changed
    /// (`(model changed)`), and `switching back costs nothing` (empty when nothing's warm to go back to).
    pub fn switch_parts(&self) -> Option<(String, String, &'static str)> {
        let s = self.switch_now()?;
        let back = if matches!(s.cost, Cost::Warm { .. }) { "switching back costs nothing" } else { "" };
        Some((
            format!("next message {}", s.what(self.interface.zone)),
            format!("({} changed)", s.changed.join(" and ")),
            back,
        ))
    }

    fn switch_now(&self) -> Option<Switch> {
        self.switch_cost(self.workspace.claude.chat.as_ref()?.switched.as_ref()?)
    }
}

/// A switch since the last message: what it costs, what changed (`billing`, `model`, `effort`), and whether the
/// cost is only maybe (an effort change on a model whose row says `unknown`).
#[derive(Clone, Debug, PartialEq)]
pub struct Switch {
    pub cost: Cost,
    pub changed: Vec<&'static str>,
    pub maybe: bool,
}

impl Switch {
    /// `rewrites ≈ $1.20` · `may rewrite ≈ 150k tokens` · `keeps the cache` …
    fn what(&self, zone: crate::log::Zone) -> String {
        match self.maybe {
            true => self.cost.short(zone).replacen("rewrites", "may rewrite", 1),
            false => self.cost.short(zone),
        }
    }
}

#[cfg(test)]
mod tests;
