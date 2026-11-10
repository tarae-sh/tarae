//! Auto cards' shadow week (R1): what the reviewer would have said beside what the user answered — the entries'
//! `reviewer` (record v3, `src/autocards.rs`). A yes is any key but `n` (`y`, `a`, `s`, `A`, a card's own letters):
//! the user let it go.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{listed, pct, s, secs, table};

fn verdict(e: &Value) -> &str {
    e["reviewer"]["verdict"].as_str().unwrap_or("none")
}

/// The user's answer as the reviewer's columns read it: `yes`, `n` or `dropped`.
fn answer(e: &Value) -> &'static str {
    match s(e, "answer") {
        "n" => "n",
        "dropped" => "dropped",
        _ => "yes",
    }
}

/// The `p`-th percentile of `v` (nearest rank), None when empty.
fn percentile(mut v: Vec<u64>, p: usize) -> Option<u64> {
    v.sort_unstable();
    let at = (v.len() * p).div_ceil(100).max(1);
    v.get(at - 1).copied()
}

/// The section for Claude's cards `claude`: nothing when none was reviewed.
pub(super) fn report(claude: &[&Value]) -> String {
    let reviewed: Vec<&Value> = claude.iter().copied().filter(|e| e["reviewer"].is_object()).collect();
    if reviewed.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "\n## Auto cards' reviewer (R1's shadow): {} of Claude's {} cards\n\nWhat it would have said beside your \
         answer — a yes is any key but `n`. Nothing acted on it.\n\n",
        reviewed.len(),
        claude.len()
    );
    let mut kinds: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for e in &reviewed {
        kinds.entry(s(e, "kind")).or_default().push(e);
    }
    let cells = |es: &[&Value]| {
        let n =
            |v: &str, a: &str| es.iter().filter(|e| verdict(e) == v && answer(e) == a).count().to_string();
        vec![
            es.len().to_string(),
            n("approve", "yes"),
            n("approve", "n"),
            n("approve", "dropped"),
            n("ask", "yes"),
            n("ask", "n"),
            n("ask", "dropped"),
            es.iter().filter(|e| verdict(e) == "none").count().to_string(),
        ]
    };
    let mut t: Vec<Vec<String>> =
        kinds.iter().map(|(k, es)| [vec![k.to_string()], cells(es)].concat()).collect();
    t.push([vec!["**Total**".into()], cells(&reviewed)].concat());
    let head =
        ["Kind", "Reviewed", "approve · yes", "approve · n", "approve · dropped", "ask · yes", "ask · n"];
    out += &table(&[&head[..], &["ask · dropped", "none"]].concat(), &t);

    let is = |v: &str, a: &str| -> Vec<&Value> {
        reviewed.iter().copied().filter(|e| verdict(e) == v && answer(e) == a).collect()
    };
    // (a verdict after your answer, or beside words it never read, couldn't have saved your key: said apart)
    let stale = |e: &&Value| ["late", "unsent", "words_since"].iter().any(|k| e["reviewer"][*k] == true);
    let (stale, saved): (Vec<&Value>, Vec<&Value>) = is("approve", "yes").into_iter().partition(stale);
    let wrong = is("approve", "n");
    let yes = reviewed.iter().filter(|e| answer(e) == "yes").count();
    out += &format!(
        "\n- **Saved** (approve ∧ your yes, in time and with every word you sent — a card it would have answered as \
         you did): {} of {} ({}) · **too late or stale** (approve ∧ your yes, but late, unsent words or words since): \
         {}.\n",
        saved.len(),
        reviewed.len(),
        pct(saved.len(), reviewed.len()),
        stale.len()
    );
    out += &format!("- **Wrong yes** (approve ∧ your `n`, late ones too): {}", wrong.len());
    out += if wrong.is_empty() { ".\n" } else { " — each with its reasons, to be read by hand:\n" };
    for e in &wrong {
        out += &format!("  {} — {}\n", listed(e), e["reviewer"]["said"].as_str().unwrap_or(""));
    }
    let asked = is("ask", "yes").len();
    out += &format!(
        "- **Safe, asked** (ask ∧ your yes, of your yeses): {asked} of {yes} ({}).\n",
        pct(asked, yes)
    );
    let mut failed: BTreeMap<&str, usize> = BTreeMap::new();
    for e in reviewed.iter().filter(|e| verdict(e) == "none") {
        *failed.entry(e["reviewer"]["failed"].as_str().unwrap_or("—")).or_default() += 1;
    }
    let failed: Vec<String> = failed.iter().map(|(f, n)| format!("{n} `{f}`")).collect();
    out += &format!(
        "- **`none`** by why: {}.\n",
        if failed.is_empty() { "—".into() } else { failed.join(" · ") }
    );
    let flag = |k: &str| reviewed.iter().filter(|e| e["reviewer"][k] == true).count();
    out += &format!(
        "- **Unsent words** waiting as it read or as it answered: {} · **words since** the card came, before your \
         answer: {} · **late** (the verdict after your answer): {}.\n",
        flag("unsent"),
        flag("words_since"),
        flag("late")
    );
    let costs: Vec<f64> = reviewed.iter().filter_map(|e| e["reviewer"]["cost"].as_f64()).collect();
    let total: f64 = costs.iter().sum();
    let each = if costs.is_empty() { 0.0 } else { total / costs.len() as f64 };
    let ms: Vec<u64> =
        reviewed.iter().filter_map(|e| e["reviewer"]["ms"].as_u64()).filter(|ms| *ms > 0).collect();
    let at = |p| percentile(ms.clone(), p).map_or_else(|| "—".into(), secs);
    out += &format!(
        "- **Cost**: ${total:.4} in all, ${each:.4} a card ({} with a cost) · **time** p50 {} · p90 {}.\n",
        costs.len(),
        at(50),
        at(90)
    );
    out += "- The week's inputs (`$state/cards/inputs/`) are removed once this report is written up: a replay of a \
            later wording is a spend, asked then.\n";
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(kind: &str, answer: &str, reviewer: Value) -> Value {
        json!({ "v": 3, "at": "2026-11-11T10:00:00", "session": "ci", "card": 1, "kind": kind, "why": "also",
            "answer": answer, "what": "cargo test", "reviewer": reviewer })
    }

    /// Every count the week's report reads: by kind, verdict × answer, saved, wrong yes with its reasons, safe asked,
    /// `none` by why, unsent · words since · late, cost and time.
    #[test]
    fn the_reviewers_week_reads_verdict_beside_answer() {
        let r = |v: &str, cost: f64, ms: u64| json!({ "verdict": v, "cost": cost, "ms": ms, "said": "the user named it" });
        let es = [
            entry("grant", "y", r("approve", 0.001, 1000)),
            entry("grant", "A", r("approve", 0.001, 2000)),
            entry("grant", "y", json!({ "verdict": "approve", "late": true })),
            entry("grant", "a", json!({ "verdict": "approve", "words_since": true })),
            entry("read", "y", json!({ "verdict": "approve", "unsent": true })),
            entry("grant", "n", r("approve", 0.001, 3000)),
            entry("run", "y", json!({ "verdict": "ask", "cost": 0.002, "ms": 4000, "unsent": true })),
            entry("run", "dropped", json!({ "verdict": "none", "failed": "timeout", "late": true })),
            entry("read", "n", json!({ "verdict": "ask", "words_since": true, "ms": 9000 })),
            json!({ "kind": "run", "answer": "y" }),
        ];
        let claude: Vec<&Value> = es.iter().collect();
        let out = report(&claude);
        for want in [
            "9 of Claude's 10 cards",
            "| grant     | 5        | 4             | 1           | 0                 | 0         | 0       | 0             | 0    |",
            "**Saved** (approve ∧ your yes, in time and with every word you sent — a card it would have answered as you \
             did): 2 of 9 (22 %) · **too late or stale** (approve ∧ your yes, but late, unsent words or words since): 3.",
            "**Wrong yes** (approve ∧ your `n`, late ones too): 1 — each",
            "card#1 · `cargo test` — the user named it",
            "**Safe, asked** (ask ∧ your yes, of your yeses): 1 of 6 (17 %)",
            "**`none`** by why: 1 `timeout`.",
            "**Unsent words** waiting as it read or as it answered: 2 · **words since** the card came, before your answer: \
             2 · **late** (the verdict after your answer): 2.",
            "$0.0050 in all, $0.0013 a card (4 with a cost) · **time** p50 3.0 s · p90 9.0 s.",
            "`$state/cards/inputs/`",
        ] {
            assert!(out.contains(want), "{want}\n{out}");
        }
        assert_eq!(report(&[&es[9]]), "", "nothing reviewed, no section");
    }
}
