//! What a switch costs — the table read as the tools pack says it, and the cost of a conversation's cache by its
//! state. No claude here: a `Usage` fed the request a stream would say.

use super::*;
use crate::usage::{Counts, Seen, Ttl};

/// A conversation whose last request carried `tokens` on `model`, its cache written `ago_ms` before `now`
/// (an hour-long write).
pub(crate) fn conversation(model: &str, tokens: u64, now: u64, ago_ms: u64) -> Usage {
    let mut u = Usage::default();
    u.resumed("conv-1");
    u.requesting(now - ago_ms);
    let counts = Counts { input: Some(10), write: Some(tokens - 10), read: Some(0), output: Some(5) };
    u.seen(Seen {
        id: Some("msg-1".into()),
        model: model.into(),
        counts,
        ttl: Some(Ttl::Hour1),
        at_ms: now - ago_ms,
    });
    u
}

const NOW: u64 = 1_800_000_000_000;

/// A warm cache switched onto the key costs its whole context at the hour-long write (an API chat's cache lives
/// an hour): 150k tokens of Opus 5.5 at $8/MTok is $1.20 — never the 5-minute write's $0.75. On the plan it's
/// tokens, toward its limits; a model the table lacks, tokens too. Cold, or nothing cached: no extra cost.
#[test]
fn a_warm_cache_onto_the_key_costs_its_context_at_the_hour_long_write() {
    let prices = &crate::sandboxpack::get().price;
    let warm = conversation("claude-opus-5-5", 150_000, NOW, 60_000);
    let until = NOW - 60_000 + Ttl::Hour1.ms();
    let c = cost(&warm, NOW, Billing::Api, "claude-opus-5-5", prices);
    assert_eq!(c, Cost::Warm { tokens: 150_000, dollars: Some(1.2), until_ms: until });
    let here = crate::log::Zone::Here;
    assert_eq!(c.said(Billing::Api, here), "rewrites ≈ $1.20 of cached context (150k tokens)");
    let plan = cost(&warm, NOW, Billing::Subscription, "claude-opus-5-5", prices);
    assert_eq!(
        plan,
        Cost::Warm { tokens: 150_000, dollars: None, until_ms: until },
        "the plan: never dollars"
    );
    assert_eq!(
        plan.said(Billing::Subscription, here),
        "rewrites ≈ 150k tokens of cached context, toward your plan's limits"
    );
    let unknown = cost(&warm, NOW, Billing::Api, "claude-someday-9", prices);
    assert!(
        unknown.said(Billing::Api, here).contains("150k tokens of cached context (no price for this model)")
    );
    // Cold since its TTL ran out: staying writes it again too
    let cold = conversation("claude-opus-5-5", 150_000, NOW, 2 * Ttl::Hour1.ms());
    let gone = NOW - 2 * Ttl::Hour1.ms() + Ttl::Hour1.ms();
    assert_eq!(cost(&cold, NOW, Billing::Api, "claude-opus-5-5", prices), Cost::Cold { since_ms: gone });
    assert!(
        Cost::Cold { since_ms: gone }
            .said(Billing::Api, here)
            .starts_with("no extra cost — the cache went cold at ")
    );
    assert_eq!(cost(&Usage::default(), NOW, Billing::Api, "claude-opus-5-5", prices), Cost::Nothing);
}

/// A model's row is found by its id whole — a date or a `[1m]` suffix aside — never by a prefix: `claude-opus-5`
/// isn't `claude-opus-5-5`'s row (that would price Opus 5.5 as Opus 5). Haiku 5.5's two tiers by the context.
#[test]
fn a_price_is_a_models_whole_id_never_a_prefix() {
    let prices = &crate::sandboxpack::get().price;
    let write = |id: &str, tokens| price_of(prices, id, tokens).map(|p| p.write_1h);
    assert_eq!(write("claude-opus-5-5", 1), Some(8.0));
    assert_eq!(write("claude-opus-5", 1), Some(10.0));
    assert_eq!(write("claude-opus-5-5[1m]", 1), Some(8.0), "the long-context suffix");
    assert_eq!(write("claude-haiku-4-5-20251001", 1), Some(2.0), "a dated id");
    assert_eq!(write("claude-opus-5-55", 1), None);
    assert_eq!(write("claude-haiku-5-5", 100_000), Some(0.2), "up to 100k: the lower tier");
    assert_eq!(write("claude-haiku-5-5", 100_001), Some(1.0), "over it: the higher");
    assert_eq!(write("opus", 1), None, "an alias isn't priced — tokens only");
}

/// The tools pack's table reads clean, every row with both writes, and what effort does to the cache as the
/// sources say: Fable 5.1 keeps it (Claude Code says so), Opus 5.5 unchecked (L7), Haiku 4.5 rewrites. A row
/// that's wrong is refused, saying which.
#[test]
fn the_price_table_reads_clean_and_a_wrong_row_is_refused() {
    let t = crate::sandboxpack::get();
    assert!(!t.problems.iter().any(|p| p.contains("price")), "{:?}", t.problems);
    let effort = |id: &str| price_of(&t.price, id, 1).map(|p| p.effort);
    assert_eq!(effort("claude-fable-5-1"), Some(Effort::Keeps));
    assert_eq!(effort("claude-opus-5-5"), Some(Effort::Unknown));
    assert_eq!(effort("claude-haiku-4-5"), Some(Effort::Rewrites));
    let rows = |s: &str| read(&s.parse::<toml::Table>().unwrap()["price"]);
    assert!(
        rows("[[price]]\nmodel = \"m\"\nwrite-5m = 1\nread = 1\n").unwrap_err().contains("no `write-1h`")
    );
    assert!(
        rows("[[price]]\nmodel = \"m\"\nwrite-5m = 1\nwrite-1h = 2\n").unwrap_err().contains("no `read`")
    );
    assert!(
        rows("[[price]]\nmodel = \"m\"\nwrite-5m = 1\nwrite-1h = -2\nread = 1\n")
            .unwrap_err()
            .contains("isn't a price")
    );
    assert!(
        rows("[[price]]\nmodel = \"m\"\nwrite-5m = 1\nwrite-1h = 2\nread = 1\ncolour = 1\n")
            .unwrap_err()
            .contains("no key `colour`")
    );
    assert!(
        rows("[[price]]\nmodel = \"m\"\nwrite-5m = 1\nwrite-1h = 2\nread = 1\neffort = \"maybe\"\n").is_err()
    );
    let row = &rows("[[price]]\nmodel = \"m\"\nwrite-5m = 1\nwrite-1h = 2.5\nread = 0.1\n").unwrap()[0];
    assert_eq!((row.write_1h, row.read), (2.5, 0.1));
    assert_eq!(price_of(&t.price, "claude-opus-5-5", 1).map(|p| p.read), Some(0.2), "0.05× its base input");
}

/// A fork's first request as the live check measured it (2026-11-08, Opus 5.5): it read the source's 106,087 tokens
/// from the cache and wrote 94,147 again — the project's instructions, about the source's own cold first write — so
/// its context starts at about 200k, twice the source's. At the table's prices that's $0.0212 read + $0.7532 written.
/// Not seen here (a continued conversation), the instructions are the measured 95k, said so.
#[test]
fn a_forks_first_request_reads_the_context_and_writes_the_instructions_again() {
    let prices = &crate::sandboxpack::get().price;
    let c = fork_cost(Some(106_087), Some(94_147), "claude-opus-5-5", prices).unwrap();
    assert_eq!((c.read, c.write, c.seen, c.starts()), (106_087, 94_147, true, 200_234));
    let d = c.dollars.unwrap();
    assert!((d - (106_087.0 * 0.2 + 94_147.0 * 8.0) / 1e6).abs() < 1e-9, "{d}");
    assert_eq!(
        fork_said(&c, Billing::Subscription, "main"),
        "read again, plus the project's instructions a second time (≈ 94k tokens): it starts at ≈ 200k, about \
         twice main's. Its first message ≈ API $0.77."
    );
    assert!(fork_said(&c, Billing::Api, "main").ends_with(" Its first message ≈ $0.77."));
    // Not seen here: the measured size, said so
    let guessed = fork_cost(Some(106_087), None, "claude-opus-5-5", prices).unwrap();
    assert_eq!((guessed.write, guessed.seen), (INSTRUCTIONS_MEASURED, false));
    assert_eq!(
        fork_said(&guessed, Billing::Subscription, "main"),
        "read again, plus the project's instructions a second time (not seen here: ≈ 95k tokens, as once \
         measured): it starts at ≈ 201k, about twice main's. Its first message ≈ API $0.78."
    );
    // A long conversation: the instructions a smaller share; a model the table doesn't price: no dollars
    let long = fork_cost(Some(300_000), Some(94_147), "opus", prices).unwrap();
    assert_eq!(long.dollars, None);
    assert_eq!(
        fork_said(&long, Billing::Subscription, "main"),
        "read again, plus the project's instructions a second time (≈ 94k tokens): it starts at ≈ 394k, about \
         1.3× main's. Its first message counts toward your plan's limits."
    );
    assert!(fork_said(&long, Billing::Api, "main").ends_with(" Its first message: no price for this model."));
    assert_eq!(fork_cost(None, Some(94_147), "claude-opus-5-5", prices), None, "no request reported yet");
}
