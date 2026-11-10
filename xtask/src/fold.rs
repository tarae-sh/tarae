//! `cargo xtask fold PATH …` — what code mode (b) could save at most on live cells already run (main's
//! `.sb/decisions/chat.md` "Code mode (b)'s plan review"; the plan's §1 and §5 step 0): each saved stream-json file is
//! one cell, a folder its `.jsonl` files (a search helper's `.helper.jsonl` left out: its own claude, not the chat).
//! It reads those and nothing else — never `$state`, never Claude Code's own files.
//!
//! In a cell, each `assistant` message (its id: Claude Code splits one into an event a block) is one request, its
//! `usage` what it cost (output from the stream's `message_delta` when there is one: the assistant event's is the
//! count when it started). Its `tool_use` blocks are its calls; the `user` events' `tool_result` blocks the
//! results; a `user` event's own text the question (a stream claude was given on stdin carries none: then only
//! results count).
//!
//! A request **folds** into the program the previous one would have written when it follows a tool result, says
//! nothing (no text block: Claude had nothing to tell between the result and it), calls only the read-only
//! bindings (`BINDINGS`) and passes nothing it could only have chosen: every argument value is in an earlier
//! result of the cell or the question (`met`; the fixed choices, flags and knobs in `EXEMPT` aside).
//!
//! - **Bound A, round trips:** each folding request's own cost — input, cache write, cache read and output at its
//!   model's rates (`rates`). Folded, it isn't sent; but its output (the calls) would move into the program the
//!   request before writes, and that program's printed lines into the context — so it's an upper bound.
//! - **Bound B, context:** a result over `LARGE` tokens whose next request folds — its tokens that request's cache
//!   write (the step: the result and the call before it) — × the requests after that one × the cache-read rate: a
//!   program that kept none of it in the context. An upper bound too.
//! - **The self-check:** the requests' summed cost against the `result` event's `total_cost_usd`; more than 2 %
//!   apart is said on the cell's line (the rates, or a model the requests don't show — WebFetch's).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::Result;
use crate::turns::table;

/// The tools a program could call (the plan's §2: read-only, no card or review of their own).
const BINDINGS: [&str; 7] = ["read", "search", "symbols", "definition", "references", "diagnostics", "git"];

/// Fields a program fills without having read anything (the tools' schemas): a choice among a few (`search`'s
/// `output` lines · files · count, `case` smart · sensitive · insensitive, `type` one of rg's file types, `definition`'s
/// `kind` definition · implementation · type), a flag (`literal`, `multiline`, `tests`), or a knob saying how much
/// rather than what (`context`, `limit`, `offset`, `diagnostics`' `wait`). The rest — paths, patterns, symbols,
/// lines, globs, `exclude` — is data a chain step takes from a result. `git`'s `args` are apart (`met`): its
/// subcommand and its `-` options are choices, its revisions and paths data.
const EXEMPT: [(&str, &str); 14] = [
    ("search", "output"),
    ("search", "case"),
    ("search", "type"),
    ("search", "literal"),
    ("search", "multiline"),
    ("search", "tests"),
    ("search", "context"),
    ("search", "limit"),
    ("search", "offset"),
    ("references", "tests"),
    ("definition", "kind"),
    ("diagnostics", "wait"),
    ("git", "offset"),
    ("read", "offset"),
];

/// A result this many tokens or more is large (`turns::LARGE`'s 8k: code mode's gate counts the same).
const LARGE: u64 = 8_000;

/// The self-check's tolerance: the requests' sum against claude's total.
const CHECK: f64 = 0.02;

/// The price table tarae carries (`switchcost::Price`'s rows, the tools pack's `[[claude.price]]`).
const PRICE_TABLE: &str = include_str!("../../runtime/packs/tools/pack.toml");

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err("usage: cargo xtask fold PATH … (saved stream-json files, or folders of them)".into());
    }
    let mut groups = Vec::new();
    for a in args {
        let p = PathBuf::from(a);
        let mut cells = Vec::new();
        for f in streams_in(&p)? {
            let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            let stem =
                f.file_name().unwrap_or_default().to_string_lossy().trim_end_matches(".jsonl").to_string();
            cells.push((stem, text));
        }
        if cells.is_empty() {
            return Err(format!("no stream in {}", p.display()));
        }
        groups.push((a.clone(), cells));
    }
    print!("{}", report(&groups));
    Ok(())
}

/// `p` itself, or the streams in it (`.jsonl`, not a helper's), by name.
fn streams_in(p: &Path) -> Result<Vec<PathBuf>> {
    if !p.is_dir() {
        return Ok(vec![p.to_path_buf()]);
    }
    let rd = std::fs::read_dir(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|f| f.extension().is_some_and(|x| x == "jsonl"))
        .filter(|f| !f.to_string_lossy().ends_with(".helper.jsonl"))
        .collect();
    v.sort();
    Ok(v)
}

/// $ per million tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rates {
    input: f64,
    write_5m: f64,
    write_1h: f64,
    read: f64,
    output: f64,
}

/// Model `id`'s rates for a prompt of `tokens`, from `PRICE_TABLE` as `switchcost::price_of` finds a row (the id
/// whole, its date and `[1m]` taken off). The table holds the cache's prices; the base input is the 5-minute write
/// over 1.25 and output five times the input — the pricing page's multipliers, every model alike.
fn rates(id: &str, tokens: u64) -> Option<Rates> {
    let id = id.split('[').next().unwrap_or(id);
    let id = match id.rsplit_once('-') {
        Some((head, date)) if date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => id,
    };
    let table: toml::Value = toml::from_str(PRICE_TABLE).ok()?;
    let rows = table.get("claude")?.get("price")?.as_array()?;
    let row = rows.iter().find(|r| {
        r.get("model").and_then(toml::Value::as_str) == Some(id)
            && r.get("up-to").and_then(toml::Value::as_integer).is_none_or(|n| tokens <= n as u64)
    })?;
    let n = |k: &str| match row.get(k)? {
        toml::Value::Float(f) => Some(*f),
        toml::Value::Integer(i) => Some(*i as f64),
        _ => None,
    };
    let input = n("write-5m")? / 1.25;
    Some(Rates {
        input,
        write_5m: n("write-5m")?,
        write_1h: n("write-1h")?,
        read: n("read")?,
        output: input * 5.0,
    })
}

/// One request of a cell.
#[derive(Default)]
struct Request {
    id: String,
    model: String,
    /// input · cache write 5 min · cache write 1 h · cache read · output
    usage: [u64; 5],
    /// Output from the stream's `message_delta` (the final count), over `usage`'s.
    output: Option<u64>,
    text: bool,
    calls: Vec<(String, Value)>,
    /// It came right after a tool result (not a question).
    after_result: bool,
    /// What it could have read: the cell's question and results before it, its length in `Cell::seen`.
    seen: usize,
}

impl Request {
    fn cost(&self) -> f64 {
        let r = rates(&self.model, self.usage[0] + self.usage[1] + self.usage[2] + self.usage[3]);
        let Some(r) = r else { return 0.0 };
        let u = self.usage.map(|n| n as f64);
        let out = self.output.unwrap_or(self.usage[4]) as f64;
        (u[0] * r.input + u[1] * r.write_5m + u[2] * r.write_1h + u[3] * r.read + out * r.output) / 1e6
    }

    fn written(&self) -> u64 {
        self.usage[1] + self.usage[2]
    }
}

/// One cell: its requests, and the question and results as one text, in the order they came.
struct Cell {
    requests: Vec<Request>,
    seen: String,
    /// claude's own total: per claude conversation its largest `total_cost_usd`, summed.
    total: Option<f64>,
    /// Models the result's `modelUsage` billed.
    models: usize,
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => {
            blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n")
        }
        _ => String::new(),
    }
}

fn cell(stream: &str) -> Cell {
    let mut c = Cell { requests: Vec::new(), seen: String::new(), total: None, models: 0 };
    let mut totals: BTreeMap<String, f64> = BTreeMap::new();
    // (the last thing a request could follow: a tool result, else a question)
    let mut after_result = false;
    let mut started: Option<usize> = None;
    let at = |c: &mut Cell, id: &str, after_result: bool| -> usize {
        match c.requests.iter().position(|r| r.id == id) {
            Some(i) => i,
            None => {
                let seen = c.seen.len();
                c.requests.push(Request { id: id.to_string(), after_result, seen, ..Default::default() });
                c.requests.len() - 1
            }
        }
    };
    let usage = |u: &Value| {
        let n = |v: &Value| v.as_u64().unwrap_or(0);
        let written = n(&u["cache_creation_input_tokens"]);
        let hour = n(&u["cache_creation"]["ephemeral_1h_input_tokens"]);
        // (no split said: every write at the 5-minute price)
        let five = written.saturating_sub(hour);
        [n(&u["input_tokens"]), five, hour, n(&u["cache_read_input_tokens"]), n(&u["output_tokens"])]
    };
    for v in stream.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
        match v["type"].as_str() {
            Some("stream_event") => {
                let e = &v["event"];
                match e["type"].as_str() {
                    Some("message_start") => {
                        let id = e["message"]["id"].as_str().unwrap_or_default();
                        let i = at(&mut c, id, after_result);
                        started = Some(i);
                    }
                    Some("message_delta") => {
                        if let (Some(i), Some(o)) = (started, e["usage"]["output_tokens"].as_u64()) {
                            c.requests[i].output = Some(o);
                        }
                    }
                    _ => {}
                }
            }
            Some("assistant") => {
                let m = &v["message"];
                let i = at(&mut c, m["id"].as_str().unwrap_or_default(), after_result);
                let r = &mut c.requests[i];
                r.model = m["model"].as_str().unwrap_or_default().to_string();
                if m["usage"].is_object() {
                    r.usage = usage(&m["usage"]);
                }
                for b in m["content"].as_array().into_iter().flatten() {
                    match b["type"].as_str() {
                        Some("text") if !b["text"].as_str().unwrap_or_default().trim().is_empty() => {
                            r.text = true
                        }
                        Some("tool_use") => {
                            let name = b["name"].as_str().unwrap_or_default();
                            r.calls.push((tool(name).to_string(), b["input"].clone()));
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                let content = &v["message"]["content"];
                let results: Vec<&Value> = (content.as_array().into_iter().flatten())
                    .filter(|b| b["type"] == "tool_result")
                    .collect();
                match results.is_empty() {
                    // (a question: what tarae sent, when the stream carries it)
                    true => {
                        c.seen += &text_of(content);
                        after_result = false;
                    }
                    false => {
                        for b in results {
                            c.seen += &text_of(&b["content"]);
                            c.seen.push('\n');
                        }
                        after_result = true;
                    }
                }
                c.seen.push('\n');
            }
            Some("result") => {
                let id = v["session_id"].as_str().unwrap_or_default().to_string();
                let t = v["total_cost_usd"].as_f64().unwrap_or(0.0);
                let e = totals.entry(id).or_insert(0.0);
                *e = e.max(t);
                c.models = c.models.max(v["modelUsage"].as_object().map_or(0, |m| m.len()));
                after_result = false;
            }
            _ => {}
        }
    }
    c.total = (!totals.is_empty()).then(|| totals.values().sum());
    c
}

/// A tool's name as the bindings say it: tarae's MCP prefix off (`mcp__tarae__read` → `read`).
fn tool(name: &str) -> &str {
    match name.strip_prefix("mcp__").and_then(|r| r.split_once("__")) {
        Some((_, t)) => t,
        None => name,
    }
}

/// Whether `n` is written in `seen` as a number of its own (no digit beside it: 21 isn't in 2141).
fn has_number(seen: &str, n: i64) -> bool {
    let d = n.to_string();
    seen.match_indices(&d).any(|(i, _)| {
        let before = seen[..i].chars().next_back().is_some_and(|c| c.is_ascii_digit());
        let after = seen[i + d.len()..].chars().next().is_some_and(|c| c.is_ascii_digit());
        !before && !after
    })
}

/// The lines `seen` prints as hits: a number then `:` at a line's start (`  2141: pub fn count`).
fn hit_lines(seen: &str) -> Vec<u64> {
    seen.lines()
        .filter_map(|l| {
            let (n, _) = l.trim_start().split_once(':')?;
            n.parse().ok()
        })
        .collect()
}

/// A value a program would have had: a string of 3 characters or more found as written (a shorter one — `.`,
/// `src` is three — names nothing to check), an integer written as a number, a flag always.
fn value_met(v: &Value, seen: &str) -> bool {
    match v {
        Value::String(s) if s.chars().count() < 3 => true,
        Value::String(s) => seen.contains(s.as_str()),
        Value::Number(n) => n.as_i64().is_none_or(|n| has_number(seen, n)),
        Value::Array(a) => a.iter().all(|x| value_met(x, seen)),
        Value::Object(o) => o.values().all(|x| value_met(x, seen)),
        Value::Bool(_) | Value::Null => true,
    }
}

/// A `read` spec's parts met: its path as written; after a `:`, a name as written, or each range — met when either
/// end is a number in `seen` or it holds a line `seen` printed as a hit (a program reads a window round a hit it
/// got: `hit_lines`); `-N` (the last N lines) a knob. A URL whole.
fn spec_met(spec: &str, seen: &str) -> bool {
    let hits = || hit_lines(seen);
    let Some((path, rest)) = spec.split_once(':').filter(|_| !spec.contains("://")) else {
        return value_met(&Value::String(spec.to_string()), seen);
    };
    if !value_met(&Value::String(path.to_string()), seen) {
        return false;
    }
    let ranges = rest.bytes().all(|b| b.is_ascii_digit() || b == b'-' || b == b',' || b == b' ');
    if !ranges {
        return value_met(&Value::String(rest.to_string()), seen);
    }
    rest.split(',').map(str::trim).filter(|r| !r.is_empty()).all(|r| {
        let (a, b) = r.split_once('-').unwrap_or((r, r));
        let (Ok(a), Ok(b)) = (a.parse::<u64>(), b.parse::<u64>()) else {
            // (`-N`: how many from the end)
            return a.is_empty();
        };
        has_number(seen, a as i64) || has_number(seen, b as i64) || hits().iter().any(|h| (a..=b).contains(h))
    })
}

/// Every argument of call `name(input)` a program would have had (`value_met`), `EXEMPT`'s fields aside.
fn met(name: &str, input: &Value, seen: &str) -> bool {
    let Some(fields) = input.as_object() else { return true };
    fields.iter().all(|(field, v)| match (name, field.as_str()) {
        _ if EXEMPT.contains(&(name, field.as_str())) => true,
        ("read", "files") => {
            v.as_array().into_iter().flatten().all(|s| spec_met(s.as_str().unwrap_or(""), seen))
        }
        // (its subcommand and options chosen; the rest — revisions, paths — data)
        ("git", "args") => (v.as_array().into_iter().flatten().skip(1))
            .filter(|a| !a.as_str().is_some_and(|a| a.starts_with('-')))
            .all(|a| value_met(a, seen)),
        _ => value_met(v, seen),
    })
}

/// Whether request `r` of cell `c` folds into the program before it (the module's doc).
fn folds(c: &Cell, r: &Request) -> bool {
    let seen = &c.seen[..r.seen];
    r.after_result
        && !r.text
        && !r.calls.is_empty()
        && r.calls.iter().all(|(name, input)| BINDINGS.contains(&name.as_str()) && met(name, input, seen))
}

/// A cell measured.
struct Measured {
    cost: f64,
    a: f64,
    b: f64,
    requests: usize,
    folding: usize,
    /// The self-check, when it failed: said on the cell's line.
    check: Option<String>,
}

fn measured(c: &Cell) -> Measured {
    let n = c.requests.len();
    let (mut a, mut b, mut folding) = (0.0, 0.0, 0);
    for (i, r) in c.requests.iter().enumerate() {
        if !folds(c, r) {
            continue;
        }
        folding += 1;
        a += r.cost();
        if r.written() >= LARGE {
            let read = rates(&r.model, r.usage[0] + r.written() + r.usage[3]).map_or(0.0, |x| x.read);
            b += r.written() as f64 * (n - i - 1) as f64 * read / 1e6;
        }
    }
    let cost: f64 = c.requests.iter().map(Request::cost).sum();
    let unpriced: Vec<&str> =
        (c.requests.iter()).filter(|r| rates(&r.model, 0).is_none()).map(|r| r.model.as_str()).collect();
    let check = match c.total {
        _ if !unpriced.is_empty() => Some(format!("no rates for {}", unpriced[0])),
        None => Some("no result: claude's total not said".into()),
        Some(t) if (cost - t).abs() > CHECK * t => Some(format!(
            "rates off: ${cost:.4} summed, ${t:.4} claude's{}",
            if c.models > 1 { format!(" ({} models billed)", c.models) } else { String::new() }
        )),
        Some(_) => None,
    };
    Measured { cost, a, b, requests: n, folding, check }
}

/// A cell's task and arm from its file's name, `<task>-<arm>-<run>-<pid>` as the live A/Bs keep them; else the
/// whole name, arm `—`.
fn task_arm(stem: &str) -> (String, String) {
    let parts: Vec<&str> = stem.rsplitn(4, '-').collect();
    match parts.as_slice() {
        [pid, run, arm, task] if pid.parse::<u64>().is_ok() && run.parse::<u64>().is_ok() => {
            (task.to_string(), arm.to_string())
        }
        _ => (stem.to_string(), "—".into()),
    }
}

fn dollars(x: f64) -> String {
    format!("${x:.3}")
}

/// A row of means over `cells`: cells, $ a cell, bound A, bound B (each a cell), (A + B) as a share of the cells' $,
/// folding requests of all.
fn row(head: Vec<String>, cells: &[&Measured]) -> Vec<String> {
    let n = cells.len().max(1) as f64;
    let sum = |f: &dyn Fn(&Measured) -> f64| cells.iter().map(|m| f(m)).sum::<f64>();
    let (cost, a, b) = (sum(&|m| m.cost), sum(&|m| m.a), sum(&|m| m.b));
    let share = if cost > 0.0 { format!("{:.0} %", (a + b) * 100.0 / cost) } else { "—".into() };
    let folding: usize = cells.iter().map(|m| m.folding).sum();
    let requests: usize = cells.iter().map(|m| m.requests).sum();
    let mut r = head;
    r.extend([
        cells.len().to_string(),
        dollars(cost / n),
        dollars(a / n),
        dollars(b / n),
        share,
        format!("{folding} / {requests}"),
    ]);
    r
}

/// The report for `groups`: each a name (the folder or file named) and its cells (a file's stem, its stream).
fn report(groups: &[(String, Vec<(String, String)>)]) -> String {
    let mut out = String::from(
        "# What folding could save at most\n\nBound A: the folding requests' own cost; bound B: a result over 8k \
         tokens before a folding request, carried by the requests after it, at the cache-read rate. Both upper \
         bounds, each a cell's mean.\n",
    );
    let head = ["cells", "$ a cell", "bound A", "bound B", "(A + B) / $", "folding"];
    let mut totals = Vec::new();
    for (name, cells) in groups {
        let all: Vec<(String, String, String, Measured)> = cells
            .iter()
            .map(|(stem, text)| {
                let (task, arm) = task_arm(stem);
                (stem.clone(), task, arm, measured(&cell(text)))
            })
            .collect();
        out += &format!("\n## {name}\n\n");
        for (stem, _, _, m) in &all {
            out += &format!(
                "- {stem}: {} · {} of {} requests fold · A {} · B {}{}\n",
                dollars(m.cost),
                m.folding,
                m.requests,
                dollars(m.a),
                dollars(m.b),
                m.check.as_ref().map(|c| format!(" — {c}")).unwrap_or_default()
            );
        }
        let mut by: BTreeMap<(&str, &str), Vec<&Measured>> = BTreeMap::new();
        let mut tasks: BTreeMap<&str, Vec<&Measured>> = BTreeMap::new();
        for (_, task, arm, m) in &all {
            by.entry((task, arm)).or_default().push(m);
            tasks.entry(task).or_default().push(m);
        }
        let rows: Vec<Vec<String>> =
            by.iter().map(|((t, a), ms)| row(vec![t.to_string(), a.to_string()], ms)).collect();
        out += "\nBy task and arm, the mean of its runs:\n\n";
        out += &table(&[&["task", "arm"][..], &head[..]].concat(), &rows);
        let rows: Vec<Vec<String>> = tasks.iter().map(|(t, ms)| row(vec![t.to_string()], ms)).collect();
        out += "\nBy task, the mean of its runs in every arm:\n\n";
        out += &table(&[&["task"][..], &head[..]].concat(), &rows);
        let every: Vec<&Measured> = all.iter().map(|x| &x.3).collect();
        let off = every.iter().filter(|m| m.check.is_some()).count();
        out += &format!(
            "\nSelf-check: {off} of {} cells more than {:.0} % from claude's total.\n",
            every.len(),
            CHECK * 100.0
        );
        totals.push(row(vec![name.clone()], &every));
    }
    out += "\n## Totals\n\n";
    out += &table(&[&["named"][..], &head[..]].concat(), &totals);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hand-made cell: a search (the turn's first request — nothing to fold into), a read of a path the search
    /// printed (folds), text then a read of a path a result holds (Claude said something: no), a read of a path no
    /// result holds (Claude chose it: no). The second request, split in two events as Claude Code writes them, is one
    /// request; the fourth's output is the stream's `message_delta`'s, so the sum meets claude's total.
    #[test]
    fn a_step_folds_only_after_a_result_saying_nothing_with_every_argument_read_before() {
        let r = report(&[(
            "kept".into(),
            vec![("find-off-1-7".into(), include_str!("../fixtures/fold.jsonl").into())],
        )]);
        let has = |s: &str| assert!(r.contains(s), "{s:?} not in:\n{r}");
        has("- find-off-1-7: $0.153 · 1 of 4 requests fold · A $0.075 · B $0.004\n");
        let c = cell(include_str!("../fixtures/fold.jsonl"));
        let m = measured(&c);
        let folding: Vec<bool> = c.requests.iter().map(|r| folds(&c, r)).collect();
        assert_eq!(folding, [false, true, false, false]);
        // (A: 2 input × $4 + 9,000 written for an hour × $8 + 5,000 read × $0.20 + 100 out × $20, a million each;
        // B: those 9,000 carried by the 2 requests after it at $0.20)
        assert!((m.a - 0.075008).abs() < 1e-9, "bound A {}", m.a);
        assert!((m.b - 0.0036).abs() < 1e-9, "bound B {}", m.b);
        assert!(m.check.is_none(), "{:?}", m.check);
        has("| find | off | 1     | $0.153   | $0.075  | $0.004  | 51 %        | 1 / 4   |");
    }

    /// The self-check says a cell whose requests don't sum to claude's total, on its line.
    #[test]
    fn a_cell_whose_requests_miss_claudes_total_says_so() {
        let off = include_str!("../fixtures/fold.jsonl").replace("0.152832", "0.2");
        let r = report(&[("kept".into(), vec![("find-off-1-7".into(), off)])]);
        assert!(r.contains("— rates off: $0.1528 summed, $0.2000 claude's"), "{r}");
        assert!(r.contains("Self-check: 1 of 1 cells"), "{r}");
    }

    /// A read's range is met by a hit it holds or an end a result wrote; a name or path as written; a number only
    /// whole (21 isn't in 2141).
    #[test]
    fn a_read_spec_is_met_by_what_results_printed() {
        let seen = "src/a.rs\n  in fn f (10–30)\n  2141: let x = 1;\n";
        assert!(spec_met("src/a.rs", seen));
        assert!(spec_met("src/a.rs:2120-2160", seen), "holds the hit 2141");
        assert!(spec_met("src/a.rs:10-30", seen), "the span's ends");
        assert!(!spec_met("src/a.rs:500-600", seen));
        assert!(!spec_met("src/b.rs", seen));
        assert!(!spec_met("src/a.rs:Editor::open", seen), "a name no result wrote");
        assert!(!has_number(seen, 21));
        assert!(met("git", &serde_json::json!({"args": ["log", "--oneline", "-5", "--", "src/a.rs"]}), seen));
        assert!(!met("git", &serde_json::json!({"args": ["show", "HEAD~3:src/b.rs"]}), seen));
    }
}
