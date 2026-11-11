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
//!
//! Apart from folding, a line a folder: **the nav answers given while a server loaded** (the references-loading
//! brief, main's `.sb/decisions/chat.md`). Of `NAV`'s answers, those that are tarae's stand-in for a server that
//! will answer later (`while_loading`: text matches for `references`, the syntax tree's for `definition`, tarae's
//! outline for `symbols`, each saying the server is starting, still loading or didn't answer in time), by tool, of
//! how many; their characters; and what they cost to carry: each one's share (by characters) of the results its
//! next request wrote to the cache, at that request's write rates, and the same tokens read by each request after.
//!
//! `cargo xtask fold --record CALLS CELL …` — code mode (b) measured again on the call-order record (main's
//! `.sb/decisions/chat.md`, "Code mode (b), measured again"; the brief `.sb/codemode-b-remeasure-brief.md`). The
//! record (`CALLS`: a month's file or the folder; never found by itself) holds each request's tools, context and
//! output — no arguments, no results, no words — so the rule above can't be applied to it. A **loose** one can
//! (`loose`): a request folds when the one before it had calls, its own calls are all bindings, and its output is
//! at most the quiet threshold. Its cost is bound A, priced from its sizes alone (`sized_costs`); bound B is a
//! result of `LARGE` or more ahead of it (its context's growth less the output before: `result`). The cells given
//! calibrate it (`calibrate`), as each has both: the threshold that best tells a request that said nothing, how
//! often the loose rule is wrong each way, and the factor from the loose ceiling to the real bound, pooled, with
//! its spread over cells. The estimate is the record's loose ceiling times that factor; the report closes on the
//! rule set before the numbers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::Value;

use crate::Result;
use crate::turns::{CHAT, Turn, label, table};

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

/// The tools that answer with a stand-in while a server loads (`nav.rs`).
const NAV: [&str; 3] = ["references", "definition", "symbols"];

/// A result this many tokens or more is large (`turns::LARGE`'s 8k: code mode's gate counts the same).
const LARGE: u64 = 8_000;

/// The self-check's tolerance: the requests' sum against claude's total.
const CHECK: f64 = 0.02;

/// The price table tarae carries (`switchcost::Price`'s rows, the tools pack's `[[claude.price]]`).
const PRICE_TABLE: &str = include_str!("../../runtime/packs/tools/pack.toml");

const RECORD_USAGE: &str = "usage: cargo xtask fold --record CALLS CELL … (the call-order record's folder or file; saved stream-json cells, \
     or folders of them, to calibrate on)";

pub fn run(args: &[String]) -> Result<()> {
    if args.first().is_some_and(|a| a == "--record") {
        return record_run(&args[1..]);
    }
    if args.is_empty() {
        return Err("usage: cargo xtask fold PATH … (saved stream-json files, or folders of them)".into());
    }
    print!("{}", report(&groups_of(args)?));
    Ok(())
}

/// A path named, with its cells: each a file's stem and its stream.
type Group = (String, Vec<(String, String)>);

/// Each path named, with its cells.
fn groups_of(args: &[String]) -> Result<Vec<Group>> {
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
    Ok(groups)
}

fn record_run(args: &[String]) -> Result<()> {
    let [calls, cells @ ..] = args else { return Err(RECORD_USAGE.into()) };
    if cells.is_empty() {
        return Err(RECORD_USAGE.into());
    }
    let files = crate::turns::jsonl_in(Path::new(calls))?;
    if files.is_empty() {
        return Err(format!("no call-order record in {calls}"));
    }
    let mut text = String::new();
    for f in &files {
        text += &std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
    }
    print!("{}", record_report(calls, &text, &groups_of(cells)?));
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
    static ROWS: OnceLock<Vec<toml::Value>> = OnceLock::new();
    let rows = ROWS.get_or_init(|| {
        let table: Option<toml::Value> = toml::from_str(PRICE_TABLE).ok();
        table.and_then(|t| t.get("claude")?.get("price")?.as_array().cloned()).unwrap_or_default()
    });
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
    /// Its nav answers (`NAV`), in the order they came.
    nav: Vec<NavAnswer>,
}

/// A nav tool's answer in a cell.
struct NavAnswer {
    tool: String,
    /// tarae's stand-in for a server still loading (`while_loading`).
    loading: bool,
    chars: usize,
    /// The request that came after it (None: none did) — and the characters of every result in its step, this
    /// one's too: what that request wrote to the cache besides its own call.
    next: Option<usize>,
    step: usize,
}

/// Whether `text`, tool `name`'s answer, is tarae's stand-in for a server that will answer later (`nav.rs`'s
/// `text_matches`, `tree_defs`, `outline`): its first line says what it is and why — starting, still loading, or no
/// answer in time. The words are copies of theirs (and of `lsp_editor.rs`'s `tool_server`, "is starting"), each of
/// those naming this function: a wording changed there is changed here, else fold counts 0.
fn while_loading(name: &str, text: &str) -> bool {
    let first = text.lines().next().unwrap_or_default();
    let later =
        ["is still loading", " is starting", "didn't answer within"].iter().any(|w| first.contains(w));
    let stand_in = match name {
        "references" => {
            first.contains(", so these are the whole-word text matches of `")
                || first.starts_with("no text matches of `")
        }
        "definition" => {
            first.contains(" by the syntax tree among its text matches — ")
                || first.contains(" found by the syntax tree either — ")
        }
        "symbols" => first.contains("(tarae's outline — "),
        _ => false,
    };
    stand_in && later
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
    let mut c = Cell { requests: Vec::new(), seen: String::new(), total: None, models: 0, nav: Vec::new() };
    let mut totals: BTreeMap<String, f64> = BTreeMap::new();
    // (the last thing a request could follow: a tool result, else a question)
    let mut after_result = false;
    let mut started: Option<usize> = None;
    // (each call's tool by its id; the characters of the results since the last request began)
    let mut called: BTreeMap<String, String> = BTreeMap::new();
    let mut step = 0;
    let at = |c: &mut Cell, id: &str, after_result: bool, step: &mut usize| -> usize {
        match c.requests.iter().position(|r| r.id == id) {
            Some(i) => i,
            None => {
                let seen = c.seen.len();
                c.requests.push(Request { id: id.to_string(), after_result, seen, ..Default::default() });
                let i = c.requests.len() - 1;
                // (the nav answers waiting for a request: this one carries them first)
                for a in c.nav.iter_mut().filter(|a| a.next.is_none()) {
                    (a.next, a.step) = (Some(i), *step);
                }
                *step = 0;
                i
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
                        let i = at(&mut c, id, after_result, &mut step);
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
                let i = at(&mut c, m["id"].as_str().unwrap_or_default(), after_result, &mut step);
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
                            let name = tool(b["name"].as_str().unwrap_or_default()).to_string();
                            called.insert(b["id"].as_str().unwrap_or_default().to_string(), name.clone());
                            r.calls.push((name, b["input"].clone()));
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
                            let text = text_of(&b["content"]);
                            step += text.chars().count();
                            let name = called.get(b["tool_use_id"].as_str().unwrap_or_default());
                            if let Some(name) = name.filter(|n| NAV.contains(&n.as_str())) {
                                c.nav.push(NavAnswer {
                                    tool: name.clone(),
                                    loading: while_loading(name, &text),
                                    chars: text.chars().count(),
                                    next: None,
                                    step: 0,
                                });
                            }
                            c.seen += &text;
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
    /// Nav answers by tool: given while a server loaded, of how many.
    nav: BTreeMap<String, (usize, usize)>,
    /// The loading ones' characters, and what carrying them cost: written to the cache, read after.
    loading_chars: usize,
    loading_cost: (f64, f64),
}

/// What carrying nav answer `a` cost in cell `c`: its share by characters of what its next request wrote to the
/// cache (that step's results), at that request's write rates; the same tokens read by each request after it.
fn carried(c: &Cell, a: &NavAnswer) -> (f64, f64) {
    let Some(j) = a.next else { return (0.0, 0.0) };
    let r = &c.requests[j];
    let Some(rates) = rates(&r.model, r.usage[0] + r.written() + r.usage[3]) else { return (0.0, 0.0) };
    if r.written() == 0 || a.step == 0 {
        return (0.0, 0.0);
    }
    let share = a.chars as f64 / a.step as f64;
    let written = r.usage[1] as f64 * rates.write_5m + r.usage[2] as f64 * rates.write_1h;
    let tokens = r.written() as f64 * share;
    let after = (c.requests.len() - j - 1) as f64;
    (written * share / 1e6, tokens * after * rates.read / 1e6)
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
    let mut nav: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let (mut loading_chars, mut loading_cost) = (0, (0.0, 0.0));
    for x in &c.nav {
        let e = nav.entry(x.tool.clone()).or_default();
        e.1 += 1;
        if x.loading {
            e.0 += 1;
            loading_chars += x.chars;
            let (w, r) = carried(c, x);
            loading_cost = (loading_cost.0 + w, loading_cost.1 + r);
        }
    }
    Measured { cost, a, b, requests: n, folding, check, nav, loading_chars, loading_cost }
}

/// The folder's line of nav answers given while a server loaded (the module's doc), over `cells`.
fn loading_line(cells: &[&Measured]) -> String {
    let mut by: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for m in cells {
        for (tool, (l, n)) in &m.nav {
            let e = by.entry(tool.as_str()).or_default();
            (e.0, e.1) = (e.0 + l, e.1 + n);
        }
    }
    if by.is_empty() {
        return "\nNav answers while a server loaded: none — no references, definition or symbols answer.\n"
            .into();
    }
    let tools: Vec<String> = by.iter().map(|(t, (l, n))| format!("{t} {l} of {n}")).collect();
    let chars: usize = cells.iter().map(|m| m.loading_chars).sum();
    let (w, r) = cells.iter().fold((0.0, 0.0), |s, m| (s.0 + m.loading_cost.0, s.1 + m.loading_cost.1));
    let cost: f64 = cells.iter().map(|m| m.cost).sum();
    let share =
        if cost > 0.0 { format!(", {:.0} % of the cells' $", (w + r) * 100.0 / cost) } else { String::new() };
    let cells_with = cells.iter().filter(|m| m.loading_chars > 0).count();
    format!(
        "\nNav answers while a server loaded: {} · in {cells_with} of {} cells · {} chars · {} to carry ({} written, \
         {} read after){share}.\n",
        tools.join(", "),
        cells.len(),
        thousands(chars),
        dollars(w + r),
        dollars(w),
        dollars(r),
    )
}

/// `n` with a comma every three digits.
fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
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
    // (+ 0.0: an empty sum is -0.0)
    format!("${:.3}", x + 0.0)
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
fn report(groups: &[Group]) -> String {
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
        out += &loading_line(&every);
        totals.push(row(vec![name.clone()], &every));
    }
    out += "\n## Totals\n\n";
    out += &table(&[&["named"][..], &head[..]].concat(), &totals);
    out
}

// ---- `--record`: the loose rule, its calibration on cells, the estimate on the record (the module's doc)

/// A model with no rates (a stand-in's, a name the table lacks) is priced as this one, said on the report.
const FALLBACK: &str = "claude-opus-5-5";
/// The rule set before the numbers (the brief's): (b) reopens at this share of chat cost over the record …
const REOPEN: f64 = 0.20;
/// … or when the turns whose own estimate is `TURN` or more hold `HELD` of all chat cost.
const TURN: f64 = 0.30;
const HELD: f64 = 0.25;
/// Fewer calibration cells with a loose ceiling above 0 than this carry no estimate.
const THIN: usize = 10;
/// The factor's spread: resamples of the cells (a fixed seed), their 10th and 90th percentiles.
const RESAMPLES: usize = 2000;
/// A folder's own factor counts toward the range with this many cells carrying a loose ceiling.
const FOLDER: usize = 5;

/// A call a program could make: the plan's bindings, and with `run` every run (an upper bound on a tool that runs
/// in the sandbox from a script: the record doesn't say which runs only read).
fn binds(tool: &str, run: bool) -> bool {
    BINDINGS.contains(&tool) || (run && tool == "run")
}

/// A turn as the call-order record holds it, or a cell read as if it did: names and sizes only.
struct Sized {
    model: String,
    steps: Vec<Vec<String>>,
    /// Each request's context and output.
    sizes: Vec<(Option<u64>, Option<u64>)>,
    /// What the turn wrote to the cache and sent uncached (its counts' write + input): how much of its first
    /// request's context was new.
    written: Option<u64>,
}

fn sized_turn(t: &Turn) -> Sized {
    Sized {
        model: t.model.clone(),
        steps: t.steps.clone(),
        sizes: t.sizes.clone(),
        written: Some(t.counts[0] + t.counts[1]),
    }
}

fn sized_cell(c: &Cell) -> Sized {
    Sized {
        model: c.requests.iter().map(|r| r.model.clone()).find(|m| !m.is_empty()).unwrap_or_default(),
        steps: c.requests.iter().map(|r| r.calls.iter().map(|(n, _)| n.clone()).collect()).collect(),
        sizes: (c.requests.iter())
            .map(|r| (Some(r.usage[0] + r.written() + r.usage[3]), Some(r.output.unwrap_or(r.usage[4]))))
            .collect(),
        written: Some(c.requests.iter().map(|r| r.usage[0] + r.written()).sum()),
    }
}

/// Each request's cost from its context and output alone, and its rates: it read the request before's context and
/// wrote the rest, for an hour (every saved cell's writes are 1-hour ones), its few uncached input tokens with it;
/// the turn's first request wrote what the turn wrote that the later ones don't account for. None: no context said.
fn sized_costs(t: &Sized) -> Vec<Option<(f64, Rates)>> {
    let n = t.sizes.len();
    let ctx = |i: usize| t.sizes.get(i).and_then(|s| s.0);
    let mut write: Vec<u64> = (0..n)
        .map(|i| match (i.checked_sub(1).and_then(ctx), ctx(i)) {
            (Some(before), Some(c)) => c.saturating_sub(before),
            _ => 0,
        })
        .collect();
    if let (Some(first), Some(w)) = (ctx(0), t.written) {
        write[0] = w.saturating_sub(write.iter().skip(1).sum()).min(first);
    }
    (0..n)
        .map(|i| {
            let c = ctx(i)?;
            let r = rates(&t.model, c).or_else(|| rates(FALLBACK, c))?;
            let out = t.sizes[i].1.unwrap_or(0) as f64;
            let cost = ((c - write[i]) as f64 * r.read + write[i] as f64 * r.write_1h + out * r.output) / 1e6;
            Some((cost, r))
        })
        .collect()
}

/// What request `i - 1`'s calls brought back: request `i`'s context over `i - 1`'s and its output (`turns`'
/// large results). None: a size not said, or a first request.
fn result(t: &Sized, i: usize) -> Option<u64> {
    let (before, out) = *t.sizes.get(i.checked_sub(1)?)?;
    Some(t.sizes.get(i)?.0?.saturating_sub(before? + out?))
}

/// What the loose rule finds in a sized turn.
#[derive(Default)]
struct Loose {
    /// Every request's cost from its sizes.
    cost: f64,
    a: f64,
    b: f64,
    folded: usize,
    requests: usize,
    /// Which requests fold.
    folds: Vec<bool>,
}

/// The loose rule on `t` (the module's doc): request `i` folds when `i - 1` had calls, `i`'s calls are all
/// bindings (`binds`, with `run` or not), and its output is `quiet` tokens or fewer. Bound A its sized cost; bound
/// B a result of `LARGE` or more ahead of it, carried by the requests after it at the cache-read rate.
fn loose(t: &Sized, quiet: u64, run: bool) -> Loose {
    let costs = sized_costs(t);
    let n = t.sizes.len().max(t.steps.len());
    let step = |i: usize| t.steps.get(i).map_or(&[][..], |s| &s[..]);
    let mut l = Loose { cost: costs.iter().flatten().map(|c| c.0).sum(), requests: n, ..Default::default() };
    for i in 0..n {
        let cost = costs.get(i).copied().flatten();
        let folds = i > 0
            && !step(i - 1).is_empty()
            && !step(i).is_empty()
            && step(i).iter().all(|s| binds(s, run))
            && t.sizes.get(i).and_then(|s| s.1).is_some_and(|o| o <= quiet)
            && cost.is_some();
        l.folds.push(folds);
        let Some((c, r)) = cost.filter(|_| folds) else { continue };
        l.folded += 1;
        l.a += c;
        if let Some(res) = result(t, i).filter(|x| *x >= LARGE) {
            l.b += res as f64 * (n - i - 1) as f64 * r.read / 1e6;
        }
    }
    l
}

/// The output threshold that best tells a request that said nothing from one that spoke, over `cand` (each
/// request's output, and whether it said nothing): the one with the fewest wrong either way, the lowest on a tie —
/// with how many spoke at or under it, and how many said nothing over it.
fn quiet_threshold(cand: &[(u64, bool)]) -> (u64, usize, usize) {
    let mut ts: Vec<u64> = cand.iter().map(|c| c.0).chain([0]).collect();
    ts.sort_unstable();
    ts.dedup();
    (ts.iter())
        .map(|&t| {
            let spoke = cand.iter().filter(|(o, quiet)| !quiet && *o <= t).count();
            let missed = cand.iter().filter(|(o, quiet)| *quiet && *o > t).count();
            (t, spoke, missed)
        })
        .min_by_key(|(t, s, m)| (s + m, *t))
        .unwrap_or((0, 0, 0))
}

/// The pooled ratio of `pairs` (real, loose) resampled over its items, a fixed seed: its 10th and 90th
/// percentiles (resamples whose loose sum is 0 left out).
fn spread(pairs: &[(f64, f64)]) -> (f64, f64) {
    let mut seed: u64 = 0x5eed;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    let mut fs = Vec::new();
    for _ in 0..RESAMPLES {
        let (mut real, mut lo) = (0.0, 0.0);
        for _ in 0..pairs.len() {
            let (r, l) = pairs[next() % pairs.len()];
            (real, lo) = (real + r, lo + l);
        }
        if lo > 0.0 {
            fs.push(real / lo);
        }
    }
    fs.sort_by(f64::total_cmp);
    match fs.len() {
        0 => (0.0, 0.0),
        n => (fs[n / 10], fs[(n * 9 / 10).min(n - 1)]),
    }
}

/// What the calibration cells say of the loose rule.
struct Calibration {
    /// A request's output at most this many tokens said nothing.
    quiet: u64,
    /// From the loose ceiling to `fold`'s real bound: pooled over the cells, and its spread (10th, 90th).
    factor: f64,
    spread: (f64, f64),
    /// The folders' own factors, lowest and highest (those with `FOLDER` cells or more carrying a ceiling): the
    /// kind of work moves it more than the sampling does.
    range: (f64, f64),
    /// Those folders by name, each with its factor.
    folders: Vec<(String, f64)>,
    /// Cells, and those whose loose ceiling is above 0.
    cells: usize,
    carrying: usize,
    /// Its section of the report.
    text: String,
}

impl Calibration {
    /// Every factor the calibration allows: the resampling's spread and the folders' own, together.
    fn bounds(&self) -> (f64, f64) {
        match self.folders.is_empty() {
            true => self.spread,
            false => (self.spread.0.min(self.range.0), self.spread.1.max(self.range.1)),
        }
    }
}

fn pct1(x: f64) -> String {
    // (+ 0.0: an empty sum is -0.0)
    format!("{:.1} %", x * 100.0 + 0.0)
}

/// A folder of cells by its checkout and name (`orchestra/codemode-ab` for `…/orchestra/target/live/codemode-ab`).
fn folder(path: &str) -> String {
    let parts: Vec<String> =
        Path::new(path).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let last = parts.last().cloned().unwrap_or_default();
    match parts.iter().rposition(|p| p == "target") {
        Some(i) if i > 0 => format!("{}/{last}", parts[i - 1]),
        _ => last,
    }
}

fn calibrate(groups: &[Group]) -> Calibration {
    let mut cells: Vec<(&str, String, Cell)> = Vec::new();
    for (name, cs) in groups {
        for (stem, text) in cs {
            cells.push((name.as_str(), task_arm(stem).0, cell(text)));
        }
    }
    // (the threshold: requests after one with calls, calling only bindings — did they say nothing?)
    let mut cand: Vec<(u64, bool)> = Vec::new();
    for (_, _, c) in &cells {
        for w in c.requests.windows(2) {
            let (before, r) = (&w[0], &w[1]);
            if !before.calls.is_empty() && !r.calls.is_empty() && r.calls.iter().all(|(n, _)| binds(n, false))
            {
                cand.push((r.output.unwrap_or(r.usage[4]), !r.text));
            }
        }
    }
    let (quiet, spoke, missed) = quiet_threshold(&cand);
    // (each cell: fold's real bound, the loose one, request by request where they differ)
    let (mut both, mut real_only, mut requests) = (0, 0, 0);
    let (mut spoke_l, mut after_q, mut chosen) = (0, 0, 0);
    let (mut real_cost, mut sized_cost) = (0.0, 0.0);
    let mut pairs: Vec<(f64, f64)> = Vec::new();
    // (a folder: its tasks, $, fold's bound, the loose ceiling, cells carrying one)
    let mut by: BTreeMap<&str, (Vec<String>, f64, f64, f64, usize)> = BTreeMap::new();
    for (group, task, c) in &cells {
        let m = measured(c);
        let l = loose(&sized_cell(c), quiet, false);
        for (r, lf) in c.requests.iter().zip(&l.folds) {
            requests += 1;
            match (folds(c, r), *lf) {
                (true, true) => both += 1,
                (true, false) => real_only += 1,
                (false, true) if r.text => spoke_l += 1,
                (false, true) if !r.after_result => after_q += 1,
                (false, true) => chosen += 1,
                (false, false) => {}
            }
        }
        real_cost += m.cost;
        sized_cost += l.cost;
        pairs.push((m.a + m.b, l.a + l.b));
        let e = by.entry(group).or_default();
        if !e.0.contains(task) {
            e.0.push(task.clone());
        }
        (e.1, e.2, e.3) = (e.1 + m.cost, e.2 + m.a + m.b, e.3 + l.a + l.b);
        e.4 += usize::from(l.a + l.b > 0.0);
    }
    let folders: Vec<(String, f64)> =
        by.iter().filter(|e| e.1.4 >= FOLDER && e.1.3 > 0.0).map(|(g, e)| (folder(g), e.2 / e.3)).collect();
    let range = folders.iter().fold((f64::INFINITY, 0.0f64), |r, x| (r.0.min(x.1), r.1.max(x.1)));
    let range = if folders.is_empty() { (0.0, 0.0) } else { range };
    let (real, lo) = pairs.iter().fold((0.0, 0.0), |s, p| (s.0 + p.0, s.1 + p.1));
    let factor = if lo > 0.0 { real / lo } else { 0.0 };
    let spread = spread(&pairs);
    let carrying = pairs.iter().filter(|p| p.1 > 0.0).count();
    let mut each: Vec<f64> = pairs.iter().filter(|p| p.1 > 0.0).map(|p| p.0 / p.1).collect();
    each.sort_by(f64::total_cmp);

    let mut t = format!(
        "\n## Calibration\n\n{} cells in {} folders (each named below). A request said nothing when its output is \
         **{quiet} tokens or fewer**: of {} requests after one with calls and calling only bindings, {spoke} spoke at \
         or under it, {missed} said nothing over it.\n\nThe loose rule against fold's, request by request ({requests} \
         requests): both fold {both}; loose only {} (it spoke {spoke_l}, after a question {after_q}, an argument it \
         chose {chosen}); fold's only {real_only}.\n\n",
        cells.len(),
        groups.len(),
        cand.len(),
        spoke_l + after_q + chosen,
    );
    let ratio = |r: f64, l: f64| if l > 0.0 { format!("{:.2}", r / l) } else { "—".into() };
    let rows: Vec<Vec<String>> = by
        .iter()
        .map(|(g, (tasks, cost, r, l, _))| {
            let n = cells.iter().filter(|c| c.0 == *g).count();
            vec![
                folder(g),
                n.to_string(),
                tasks.len().to_string(),
                dollars(*cost),
                format!("{} ({})", dollars(*r), if *cost > 0.0 { pct1(r / cost) } else { "—".into() }),
                format!("{} ({})", dollars(*l), if *cost > 0.0 { pct1(l / cost) } else { "—".into() }),
                ratio(*r, *l),
            ]
        })
        .collect();
    t += &table(&["folder", "cells", "tasks", "$", "fold's bound", "loose ceiling", "real / loose"], &rows);
    t += "\n";
    for (g, (tasks, ..)) in &by {
        let shown: Vec<&str> = tasks.iter().take(12).map(String::as_str).collect();
        let more = if tasks.len() > 12 { format!(" and {} more", tasks.len() - 12) } else { String::new() };
        t += &format!("- {g}: {}{more}\n", shown.join(", "));
    }
    t += &format!(
        "\n**Factor** from the loose ceiling to fold's bound: **{:.2}** pooled ({:.2}–{:.2}, the 10th to 90th \
         percentile of {RESAMPLES} resamples of the cells); {carrying} of {} cells have a loose ceiling above 0, their \
         own ratios {} median, {} – {}; the folders' own factors (those with {FOLDER} or more) {:.2}–{:.2}.\nSizes priced alone against the cells' own usage: {} against {} ({}).\n",
        factor,
        spread.0,
        spread.1,
        cells.len(),
        each.get(each.len() / 2).map_or("—".into(), |x| format!("{x:.2}")),
        each.first().map_or("—".into(), |x| format!("{x:.2}")),
        each.last().map_or("—".into(), |x| format!("{x:.2}")),
        range.0,
        range.1,
        dollars(sized_cost),
        dollars(real_cost),
        if real_cost > 0.0 {
            format!("{:+.1} %", (sized_cost / real_cost - 1.0) * 100.0)
        } else {
            "—".into()
        },
    );
    Calibration { quiet, factor, spread, range, folders, cells: cells.len(), carrying, text: t }
}

/// A chat turn of the record, measured both ways.
struct Rec<'a> {
    t: &'a Turn,
    /// The plan's bindings; every run folding too.
    ro: Loose,
    run: Loose,
    /// What claude reported, else its sized cost.
    cost: f64,
}

impl Rec<'_> {
    /// Its own estimated share, at factor `f`.
    fn share(&self, f: f64, run: bool) -> f64 {
        let l = if run { &self.run } else { &self.ro };
        if self.cost > 0.0 { (l.a + l.b) * f / self.cost } else { 0.0 }
    }
}

/// The share of `recs`' cost in turns whose own estimate at `f` is `TURN` or more.
fn held(recs: &[&Rec], f: f64, run: bool) -> f64 {
    let all: f64 = recs.iter().map(|r| r.cost).sum();
    let held: f64 = recs.iter().filter(|r| r.share(f, run) >= TURN).map(|r| r.cost).sum();
    if all > 0.0 { held / all } else { 0.0 }
}

/// The factor the report carries: its middle and its range — the middle left out when the calibration can't carry
/// a verdict, both when it can't carry an estimate.
#[derive(Clone, Copy)]
struct Est {
    mid: Option<f64>,
    range: Option<(f64, f64)>,
}

impl Est {
    /// A share `at` a factor, as the report writes it: `6.1 % (5.5 %–7.0 %)`, `5.5 %–7.0 %`, or not carried.
    fn show(&self, at: impl Fn(f64) -> f64) -> String {
        match (self.mid, self.range) {
            (_, None) => "not carried".into(),
            (None, Some((lo, hi))) => format!("{}–{}", pct1(at(lo)), pct1(at(hi))),
            (Some(m), Some((lo, hi))) => format!("{} ({}–{})", pct1(at(m)), pct1(at(lo)), pct1(at(hi))),
        }
    }
}

/// The turn lengths the report splits by, and a turn of `n` requests' one.
const LENGTHS: [&str; 3] = ["under 10 requests", "10–30 requests", "over 30 requests"];

fn length(n: usize) -> usize {
    match n {
        n if n < 10 => 0,
        n if n <= 30 => 1,
        _ => 2,
    }
}

/// The least factor at which the rule reopens (b) — with the plan's bindings, or with every run folding: the
/// estimate over the record reaching `REOPEN`, or the turns whose own estimate reaches `TURN` holding `HELD` of the
/// chat's $. None: no factor does.
fn turning(recs: &[&Rec], run: bool) -> Option<f64> {
    let ab = |r: &Rec| if run { r.run.a + r.run.b } else { r.ro.a + r.ro.b };
    let cost: f64 = recs.iter().map(|r| r.cost).sum();
    let all: f64 = recs.iter().map(|r| ab(r)).sum();
    let whole = (all > 0.0 && cost > 0.0).then(|| REOPEN * cost / all);
    // (each turn enters the held ones at the factor its own estimate reaches `TURN`)
    let mut entry: Vec<(f64, f64)> = recs
        .iter()
        .filter(|r| ab(r) > 0.0 && r.cost > 0.0)
        .map(|r| (TURN * r.cost / ab(r), r.cost))
        .collect();
    entry.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut held = 0.0;
    let turns = (entry.iter())
        .find(|(_, c)| {
            held += c;
            held >= HELD * cost
        })
        .map(|e| e.0);
    [whole, turns].into_iter().flatten().reduce(f64::min)
}

/// The verdict by the rule set before the numbers, or why the calibration can't carry one and what would tighten
/// it — one paragraph — and the factor the report then carries.
fn judge(recs: &[&Rec], cal: &Calibration) -> (String, Est) {
    let (lo, hi) = cal.bounds();
    let (f_ro, f_run) = (turning(recs, false), turning(recs, true));
    let word = |f: f64| match (f_ro.is_some_and(|x| f >= x), f_run.is_some_and(|x| f >= x)) {
        (true, _) => "(b) is worth reopening",
        (false, true) => {
            "neither: only \"every run folds\" passes — a different tool, the user's own question"
        }
        (false, false) => "the stop stands",
    };
    let at = |x: Option<f64>| x.map_or("no factor".into(), |x| format!("a factor of {x:.2} or more"));
    let turns = format!("(b) reopens by the rule at {}, every run folding passes at {}", at(f_ro), at(f_run));
    if cal.carrying < THIN {
        let text = format!(
            "**No verdict: the calibration is too thin to carry an estimate.** {} of {} cells have a loose ceiling \
             above 0; it takes {THIN}. To tighten it: {} more cells with a request that folds loosely (after calls, \
             calling only bindings) — any live A/B kept under `target/live/`. The record's numbers below are the \
             loose ceiling only, an upper bound.",
            cal.carrying,
            cal.cells,
            THIN - cal.carrying
        );
        return (text, Est { mid: None, range: None });
    }
    let inside = [f_ro, f_run].into_iter().flatten().find(|x| lo < *x && *x <= hi);
    let Some(x) = inside else {
        let text = format!(
            "**Verdict, by the rule:** {} — at every factor the calibration allows ({lo:.2}–{hi:.2}; {turns}).",
            word(cal.factor)
        );
        return (text, Est { mid: Some(cal.factor), range: Some((lo, hi)) });
    };
    let side = |below: bool| {
        let v: Vec<String> = (cal.folders.iter())
            .filter(|f| (f.1 < x) == below)
            .map(|f| format!("{} {:.2}", f.0, f.1))
            .collect();
        if v.is_empty() { "none".into() } else { v.join(", ") }
    };
    let cost: f64 = recs.iter().map(|r| r.cost).sum();
    let mut by = [0.0; 3];
    for r in recs {
        by[length(r.ro.requests)] += r.cost;
    }
    let most = (0..3).max_by(|a, b| by[*a].total_cmp(&by[*b])).unwrap_or(0);
    let text = format!(
        "**No verdict: the factor spreads too wide to carry one.** {turns}; the calibration puts the factor anywhere \
         in {lo:.2}–{hi:.2} (resampling the cells {:.2}–{:.2}, the folders' own {:.2}–{:.2}), which holds {x:.2}: \
         \"{}\" at its low end, \"{}\" at its high end. To tighten it: a range wholly above or below {x:.2} — the \
         folders below it now: {}; above it: {}. Cells of the work the record holds most — its turns of {}, {} of \
         its chat $ — measured as the A/Bs' were, would put the factor on that work rather than on the A/Bs' tasks. \
         The estimates below are ranges, not a number to read.",
        cal.spread.0,
        cal.spread.1,
        cal.range.0,
        cal.range.1,
        word(lo),
        word(hi),
        side(true),
        side(false),
        LENGTHS[most],
        if cost > 0.0 { pct1(by[most] / cost) } else { "—".into() },
    );
    (text, Est { mid: None, range: Some((lo, hi)) })
}

/// A row of the record's tables: turns, chat $, then for the plan's bindings and with every run: requests folded
/// of all, the ceiling's A · B, the ceiling as a share, the estimate as `est` carries it.
fn record_row(head: String, recs: &[&Rec], est: Est) -> Vec<String> {
    let cost: f64 = recs.iter().map(|r| r.cost).sum();
    let share = |x: f64| if cost > 0.0 { pct1(x / cost) } else { "—".into() };
    let mut row = vec![head, recs.len().to_string(), dollars(cost)];
    for run in [false, true] {
        let pick = |r: &&Rec| {
            if run {
                (r.run.a, r.run.b, r.run.folded, r.run.requests)
            } else {
                (r.ro.a, r.ro.b, r.ro.folded, r.ro.requests)
            }
        };
        let (a, b, folded, requests) =
            recs.iter().map(pick).fold((0.0, 0.0, 0, 0), |s, x| (s.0 + x.0, s.1 + x.1, s.2 + x.2, s.3 + x.3));
        row.push(format!("{folded} / {requests}"));
        if !run {
            row.push(format!("{} · {}", dollars(a), dollars(b)));
        }
        row.push(share(a + b));
        row.push(est.show(|f| if cost > 0.0 { (a + b) * f / cost } else { 0.0 }));
    }
    row
}

const RECORD_HEAD: [&str; 10] = [
    "",
    "turns",
    "chat $",
    "folded",
    "ceiling A · B",
    "ceiling",
    "estimate (range)",
    "every run: folded",
    "ceiling",
    "estimate (range)",
];

/// The report of `--record`: the calibration on `groups`' cells, then the record's `text` (its files named `from`).
fn record_report(from: &str, text: &str, groups: &[Group]) -> String {
    let cal = calibrate(groups);
    let (all, broken) = crate::turns::entries(text);
    let turns: Vec<Turn> = (all.iter())
        .filter(|e| e["what"] != "board-whole")
        .map(Turn::of)
        .filter(|t| CHAT.contains(&t.kind))
        .collect();
    let recs: Vec<Rec> = turns
        .iter()
        .map(|t| {
            let s = sized_turn(t);
            let ro = loose(&s, cal.quiet, false);
            let run = loose(&s, cal.quiet, true);
            let cost = t.cost.filter(|c| *c > 0.0).unwrap_or(ro.cost);
            Rec { t, ro, run, cost }
        })
        .collect();
    let every: Vec<&Rec> = recs.iter().collect();
    let (verdict, est) = judge(&every, &cal);

    let mut out = format!(
        "# Code mode (b), measured again on the record\n\n{verdict}\n\nFrom {from}: {} entries, {} chat turns",
        all.len(),
        recs.len()
    );
    if let (Some(first), Some(last)) = (turns.first(), turns.last()) {
        out += &format!(", {} – {}", first.at, last.at);
    }
    if broken > 0 {
        out += &format!(" ({broken} line{} not an entry, skipped)", if broken == 1 { "" } else { "s" });
    }
    let no_size: usize = turns.iter().map(|t| t.sizes.iter().filter(|s| s.0.is_none()).count()).sum();
    let requests: usize = turns.iter().map(|t| t.sizes.len()).sum();
    let reported: Vec<&Rec> = recs.iter().filter(|r| r.t.cost.is_some_and(|c| c > 0.0)).collect();
    let (said, sized) = reported.iter().fold((0.0, 0.0), |s, r| (s.0 + r.cost, s.1 + r.ro.cost));
    let unpriced = turns.iter().filter(|t| rates(&t.model, 0).is_none()).count();
    out += &format!(
        ".\n\nChat $ is what claude reported ({} of {} turns; the rest priced from their sizes). Sized against \
         reported: {} against {} ({}). Requests with no size said: {no_size} of {requests}{}.\n",
        reported.len(),
        recs.len(),
        dollars(sized),
        dollars(said),
        if said > 0.0 { format!("{:+.1} %", (sized / said - 1.0) * 100.0) } else { "—".into() },
        if unpriced > 0 {
            format!("; {unpriced} turns' model has no rates, priced as {FALLBACK}")
        } else {
            String::new()
        },
    );
    out += &cal.text;

    // The ceiling and the estimate
    out += &format!(
        "\n## The ceiling and the estimate\n\nA request folds loosely after one with calls, calling only the plan's \
         bindings (`{}`), its output {} tokens or fewer; \"every run\" lets `run` in too. Ceiling: bound A + bound B \
         as a share of chat $; estimate: the ceiling × the factor, {}. An estimate, said as one: the factor is \
         calibrated with the bindings and carried to runs as it is.\n\n",
        BINDINGS.join("`, `"),
        cal.quiet,
        match (est.mid, est.range) {
            (_, None) => "not carried (too few cells)".to_string(),
            (None, Some((lo, hi))) => format!("anywhere in {lo:.2}–{hi:.2} (no middle carried)"),
            (Some(m), Some((lo, hi))) => format!("{m:.2} ({lo:.2}–{hi:.2})"),
        },
    );
    let mut rows = vec![record_row("the record".into(), &every, est)];
    let mut months: BTreeMap<&str, Vec<&Rec>> = BTreeMap::new();
    let mut kinds: BTreeMap<&str, Vec<&Rec>> = BTreeMap::new();
    let mut lengths: BTreeMap<u8, Vec<&Rec>> = BTreeMap::new();
    for r in &recs {
        months.entry(r.t.at.get(..7).unwrap_or("?")).or_default().push(r);
        kinds.entry(r.t.kind).or_default().push(r);
        lengths.entry(length(r.ro.requests) as u8).or_default().push(r);
    }
    rows.extend(months.iter().map(|(m, rs)| record_row(m.to_string(), rs, est)));
    rows.extend(kinds.iter().map(|(k, rs)| record_row(k.to_string(), rs, est)));
    rows.extend(lengths.iter().map(|(l, rs)| record_row(LENGTHS[*l as usize].into(), rs, est)));
    out += &table(&RECORD_HEAD, &rows);

    // Where it concentrates: by each turn's own estimate when one is carried, else by its ceiling
    let (by_f, by) = match est.mid {
        Some(m) => (m, "own estimate (the plan's bindings, the factor's middle)"),
        None => (1.0, "own loose ceiling (the plan's bindings; no estimate carried)"),
    };
    let mut sorted: Vec<&Rec> = recs.iter().collect();
    sorted.sort_by(|a, b| b.share(by_f, false).total_cmp(&a.share(by_f, false)));
    let cost: f64 = recs.iter().map(|r| r.cost).sum();
    // (a turn's band by its own estimate: none, then under each of these, then `TURN` or more)
    let band = |s: f64| match s {
        s if s <= 0.0 => 0,
        s if s < 0.1 => 1,
        s if s < 0.2 => 2,
        s if s < TURN => 3,
        _ => 4,
    };
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (i, name) in ["0", "under 10 %", "10–20 %", "20–30 %", "30 % or more"].iter().enumerate() {
        let rs: Vec<&&Rec> = sorted.iter().filter(|r| band(r.share(by_f, false)) == i).collect();
        let c: f64 = rs.iter().map(|r| r.cost).sum();
        rows.push(vec![
            name.to_string(),
            rs.len().to_string(),
            dollars(c),
            if cost > 0.0 { pct1(c / cost) } else { "—".into() },
        ]);
    }
    out += &format!("\n## Where it concentrates\n\nTurns by their {by}:\n\n");
    out += &table(&["band", "turns", "chat $", "of all chat $"], &rows);
    out += "\nThe 15 largest:\n\n";
    let rows: Vec<Vec<String>> = (sorted.iter().take(15))
        .map(|r| {
            vec![
                r.t.at.clone(),
                r.t.kind.into(),
                format!("{} / {}", r.ro.folded, r.ro.requests),
                dollars(r.cost),
                pct1(r.share(1.0, false)),
                est.show(|f| r.share(f, false)),
            ]
        })
        .collect();
    out += &table(&["at", "kind", "folded", "$", "ceiling", "estimate"], &rows);
    let mut pairs: BTreeMap<(String, String), usize> = BTreeMap::new();
    for r in sorted.iter().filter(|r| r.share(by_f, false) >= TURN) {
        for (i, f) in r.ro.folds.iter().enumerate().skip(1) {
            if *f {
                let step = |i: usize| r.t.steps.get(i).map_or(String::new(), |s| label(s));
                *pairs.entry((step(i - 1), step(i))).or_default() += 1;
            }
        }
    }
    let mut pairs: Vec<_> = pairs.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out += &format!(
        "\nIn the turns at {:.0} % or more by that measure, the step pairs whose second request folds (10 at most):\n\n",
        TURN * 100.0
    );
    let rows: Vec<Vec<String>> =
        pairs.iter().take(10).map(|((a, b), n)| vec![format!("{a} → {b}"), n.to_string()]).collect();
    out += &table(&["pair", "times"], &rows);

    out += "\nThe loading fallback can't be seen in the record (it holds no results); references-loading's fix \
            changes how large some results are, so bound B on turns before it may be larger than after.\n";

    // The rule set before
    let share = |run: bool, f: f64| {
        let ab: f64 = every.iter().map(|r| if run { r.run.a + r.run.b } else { r.ro.a + r.ro.b }).sum();
        if cost > 0.0 { ab * f / cost } else { 0.0 }
    };
    out += &format!(
        "\n## The rule set before the numbers\n\n(b) reopens if the estimate with the plan's bindings is {:.0} % or \
         more of chat $ over the record, or if turns holding {:.0} % or more of chat $ have an estimate of {:.0} % or \
         more; else the stop stands; if only \"every run folds\" passes, neither; a calibration that can't carry it \
         gives no verdict.\n\n- The estimate with the plan's bindings: {}.\n- Chat $ in turns whose own estimate is \
         {:.0} % or more: {}.\n- Every run folding: the estimate {}; held by turns at {:.0} % or more {}.\n\n{verdict}\n",
        REOPEN * 100.0,
        HELD * 100.0,
        TURN * 100.0,
        est.show(|f| share(false, f)),
        TURN * 100.0,
        est.show(|f| held(&every, f, false)),
        est.show(|f| share(true, f)),
        TURN * 100.0,
        est.show(|f| held(&every, f, true)),
    );
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

    /// The nav answers given while a server loaded, from a hand-made cell: `references` twice (text matches while
    /// rust-analyzer loads — counted; text matches because it found none, nothing later — not), `definition` once
    /// (the server's: not), `symbols` twice (tarae's outline while it loads — counted; the server's — not). The
    /// first is carried by the second request (its share of that step's 4,000 tokens written, read once after), the
    /// outline by the third (written, nothing after).
    #[test]
    fn the_nav_answers_given_while_a_server_loaded_are_counted_and_priced() {
        let text = include_str!("../fixtures/fold-loading.jsonl");
        let r = report(&[("kept".into(), vec![("callers-off-1-7".into(), text.into())])]);
        let has = |s: &str| assert!(r.contains(s), "{s:?} not in:\n{r}");
        has(
            "Nav answers while a server loaded: definition 0 of 1, references 1 of 2, symbols 1 of 2 · in 1 of 1 cells",
        );
        let c = cell(text);
        let len = |a: &NavAnswer| a.chars as f64;
        let [t1, t2, t5, t3, t4] = &c.nav[..] else { panic!("5 nav answers: {}", c.nav.len()) };
        assert_eq!(c.nav.iter().map(|a| a.loading).collect::<Vec<_>>(), [true, false, false, true, false]);
        assert_eq!((t1.next, t3.next), (Some(1), Some(2)));
        let m = measured(&c);
        assert_eq!(m.loading_chars, t1.chars + t3.chars);
        has(&format!(" · {} chars · ", thousands(m.loading_chars)));
        // (opus 5.5: an hour's write $8, a read $0.20, a million each)
        let one = len(t1) / (len(t1) + len(t2) + len(t5));
        let three = len(t3) / (len(t3) + len(t4));
        let written = (4000.0 * one + 1000.0 * three) * 8.0 / 1e6;
        let read = 4000.0 * one * 0.20 / 1e6;
        assert!((m.loading_cost.0 - written).abs() < 1e-12, "written {} ≠ {written}", m.loading_cost.0);
        assert!((m.loading_cost.1 - read).abs() < 1e-12, "read {} ≠ {read}", m.loading_cost.1);
        assert_eq!(thousands(1234567), "1,234,567");
    }

    /// A made-up record's turn (opus 5.5: an hour's write $8, a read $0.20, output $20, a million each): `steps`
    /// each its tools, context and output.
    fn turn(steps: &[(&[&str], u64, u64)], write: u64) -> Turn {
        let steps: Vec<Value> = (steps.iter())
            .map(|(tools, context, output)| serde_json::json!({"tools": tools, "context": context, "output": output}))
            .collect();
        Turn::of(&serde_json::json!({
            "v": 1, "at": "2026-10-20 10:00:00", "what": "chat", "model": "claude-opus-5-5",
            "requests": steps.len(), "counts": {"input": 0, "write": write, "read": 0, "output": 0}, "cost": 0.5,
            "calls": {"calls": 5, "read_only_runs": 0}, "steps": steps,
        }))
    }

    /// The loose rule on a made-up turn: a quiet `read` after a `search` folds (its sized cost bound A; the 9,000
    /// tokens its search brought back, carried by the 4 requests after it, bound B); one that speaks (300 tokens out)
    /// doesn't, nor a quiet `run` (not a binding — it does once every run folds), nor a quiet `read` after a request
    /// with no calls, nor the first request.
    #[test]
    fn the_loose_rule_folds_a_quiet_binding_request_after_calls_and_nothing_else() {
        let t = turn(
            &[
                (&["search"], 20_000, 50),
                (&["read"], 29_050, 40),
                (&["read"], 30_000, 300),
                (&["run"], 31_000, 30),
                (&[], 32_000, 200),
                (&["read"], 33_000, 20),
            ],
            18_000,
        );
        let l = loose(&sized_turn(&t), 60, false);
        assert_eq!(l.folds, [false, true, false, false, false, false]);
        // (A: 20,000 read × $0.20 + 9,050 written × $8 + 40 out × $20; B: 9,000 × 4 × $0.20)
        assert!((l.a - 0.0772).abs() < 1e-12, "bound A {}", l.a);
        assert!((l.b - 0.0072).abs() < 1e-12, "bound B {}", l.b);
        assert_eq!((l.folded, l.requests), (1, 6));
        let run = loose(&sized_turn(&t), 60, true);
        assert_eq!(run.folds, [false, true, false, true, false, false]);
        // (the run: 30,000 read, 1,000 written, 30 out)
        assert!((run.a - 0.0772 - 0.0146).abs() < 1e-12, "with runs {}", run.a);
        // (the first request wrote what the turn wrote less the later ones' growth: 5,000 of its 20,000)
        let first = sized_costs(&sized_turn(&t))[0].unwrap().0;
        assert!(
            (first - (15_000.0 * 0.2 + 5_000.0 * 8.0 + 50.0 * 20.0) / 1e6).abs() < 1e-12,
            "first {first}"
        );
    }

    /// The verdict waits on a calibration that can carry it. One made-up turn of $0.50 whose loose ceiling is
    /// $0.0808 (A $0.0772, B 9,000 × 2 × $0.20 a million; $0.0948 with its run): by the rule (b) reopens at a factor
    /// of 0.2 × 0.5 / 0.0808 = 1.24 (the turn's own 30 % comes later, at 1.86), every run passes at 1.05. Too few
    /// cells: no verdict, no estimate. A range holding 1.24: no verdict, the estimate a range, the folders named on
    /// each side. Ranges wholly below or above: the stop stands, or (b) reopens.
    #[test]
    fn the_verdict_waits_on_a_calibration_that_can_carry_it() {
        let t = turn(
            &[(&["search"], 20_000, 50), (&["read"], 29_050, 40), (&["run"], 30_000, 30), (&[], 31_000, 300)],
            18_000,
        );
        let s = sized_turn(&t);
        let (ro, run) = (loose(&s, 60, false), loose(&s, 60, true));
        let rec = Rec { t: &t, ro, run, cost: 0.5 };
        let recs = [&rec];
        let f_ro = turning(&recs, false).unwrap();
        assert!((f_ro - 0.2 * 0.5 / (0.0772 + 0.0036)).abs() < 1e-9, "turns at {f_ro}");
        let cal = |carrying: usize, factor: f64, spread: (f64, f64), range: (f64, f64)| Calibration {
            quiet: 60,
            factor,
            spread,
            range,
            folders: vec![("a/low".into(), range.0), ("b/high".into(), range.1)],
            cells: 40,
            carrying,
            text: String::new(),
        };
        let (text, est) = judge(&recs, &cal(3, 1.0, (0.9, 1.1), (0.8, 1.3)));
        assert!(
            text.starts_with("**No verdict: the calibration is too thin to carry an estimate.** 3 of 40"),
            "{text}"
        );
        assert_eq!(est.show(|f| f), "not carried");
        let (text, est) = judge(&recs, &cal(20, 1.0, (0.9, 1.1), (0.8, 1.3)));
        assert!(text.starts_with("**No verdict: the factor spreads too wide to carry one.**"), "{text}");
        assert!(
            text.contains("which holds 1.24")
                && text.contains("below it now: a/low 0.80; above it: b/high 1.30"),
            "{text}"
        );
        assert_eq!(est.show(|f| f), "80.0 %–130.0 %");
        let (text, est) = judge(&recs, &cal(20, 0.33, (0.30, 0.36), (0.2, 0.5)));
        assert!(text.starts_with("**Verdict, by the rule:** the stop stands — at every factor"), "{text}");
        assert_eq!(est.show(|f| f), "33.0 % (20.0 %–50.0 %)");
        let (text, _) = judge(&recs, &cal(20, 1.5, (1.4, 1.6), (1.3, 1.7)));
        assert!(text.starts_with("**Verdict, by the rule:** (b) is worth reopening"), "{text}");
    }

    /// A result's size is the next context over this one and its output; none with a size missing or for a first
    /// request.
    #[test]
    fn a_results_size_is_the_contexts_growth_less_the_output_before() {
        let t = Sized {
            model: "claude-opus-5-5".into(),
            steps: Vec::new(),
            sizes: vec![(Some(100), Some(10)), (Some(9_110), Some(5)), (None, None), (Some(500), Some(1))],
            written: None,
        };
        assert_eq!([0, 1, 2, 3].map(|i| result(&t, i)), [None, Some(9_000), None, None]);
    }

    /// A request of a made-up stream: its text, calls (tarae's tools), cache write (an hour's), read and output,
    /// and the text of each call's result.
    struct Req<'a> {
        text: &'a str,
        calls: Vec<(&'a str, Value)>,
        write: u64,
        read: u64,
        out: u64,
        result: &'a str,
    }

    fn stream(question: &str, reqs: &[Req]) -> String {
        use serde_json::json;
        let mut lines =
            vec![json!({"type": "user", "message": {"content": [{"type": "text", "text": question}]}})];
        for (i, r) in reqs.iter().enumerate() {
            let mut content = Vec::new();
            if !r.text.is_empty() {
                content.push(json!({"type": "text", "text": r.text}));
            }
            for (k, (name, input)) in r.calls.iter().enumerate() {
                content.push(json!({"type": "tool_use", "id": format!("t{i}-{k}"),
                                    "name": format!("mcp__tarae__{name}"), "input": input}));
            }
            let usage = json!({"input_tokens": 0, "cache_creation_input_tokens": r.write,
                               "cache_creation": {"ephemeral_1h_input_tokens": r.write},
                               "cache_read_input_tokens": r.read, "output_tokens": r.out});
            lines.push(
                json!({"type": "assistant", "message": {"id": format!("m{i}"), "model": "claude-opus-5-5",
                                                              "content": content, "usage": usage}}),
            );
            if !r.calls.is_empty() {
                let results: Vec<Value> = (0..r.calls.len())
                    .map(|k| json!({"type": "tool_result", "tool_use_id": format!("t{i}-{k}"), "content": r.result}))
                    .collect();
                lines.push(json!({"type": "user", "message": {"content": results}}));
            }
        }
        lines.iter().map(|l| format!("{l}\n")).collect()
    }

    /// The calibration on two made-up cells whose bounds are known by construction. In the first, a quiet read of
    /// a window round the hit its search printed folds both ways: fold's A and the loose A are the same request's
    /// cost ($0.0772), its B 9,050 written (fold's) or 9,000 brought back (the loose one), read once after. In the
    /// second a quiet read of a path no result holds folds only loosely ($0.0132), and a read that spoke (120 out)
    /// neither — which sets the threshold at the quiet ones' 40.
    #[test]
    fn the_calibration_finds_the_threshold_and_the_factor_of_cells_known_by_construction() {
        use serde_json::json;
        let found = stream(
            "where is foo used",
            &[
                Req {
                    text: "",
                    calls: vec![("search", json!({"pattern": "foo"}))],
                    write: 20_000,
                    read: 0,
                    out: 50,
                    result: "src/a.rs\n  12: foo()",
                },
                Req {
                    text: "",
                    calls: vec![("read", json!({"files": ["src/a.rs:10-14"]}))],
                    write: 9_050,
                    read: 20_000,
                    out: 40,
                    result: "10: fn f() {",
                },
                Req { text: "In f.", calls: vec![], write: 50, read: 29_050, out: 300, result: "" },
            ],
        );
        let chose = stream(
            "where is bar used",
            &[
                Req {
                    text: "",
                    calls: vec![("search", json!({"pattern": "bar"}))],
                    write: 20_000,
                    read: 0,
                    out: 50,
                    result: "src/b.rs:3: bar",
                },
                Req {
                    text: "",
                    calls: vec![("read", json!({"files": ["src/zzz.rs"]}))],
                    write: 1_050,
                    read: 20_000,
                    out: 40,
                    result: "1: mod b;",
                },
                Req {
                    text: "Now b.",
                    calls: vec![("read", json!({"files": ["src/b.rs"]}))],
                    write: 100,
                    read: 21_050,
                    out: 120,
                    result: "3: bar",
                },
                Req { text: "In b.", calls: vec![], write: 50, read: 21_270, out: 300, result: "" },
            ],
        );
        let cal = calibrate(&[(
            "x/target/live/made".into(),
            vec![("found-off-1-7".into(), found), ("chose-off-1-7".into(), chose)],
        )]);
        assert_eq!(cal.quiet, 40);
        let real = 0.0772 + 9_050.0 * 0.2 / 1e6;
        let lo = 0.0772 + 9_000.0 * 0.2 / 1e6 + 0.0132;
        assert!((cal.factor - real / lo).abs() < 1e-9, "factor {} ≠ {}", cal.factor, real / lo);
        assert_eq!((cal.cells, cal.carrying), (2, 2));
        assert!(cal.text.contains("both fold 1; loose only 1 (it spoke 0, after a question 0, an argument it chose 1); fold's only 0."), "{}", cal.text);
        assert!(cal.text.contains("| x/made "), "{}", cal.text);
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
