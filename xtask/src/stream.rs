//! `cargo xtask stream [--model M] [--edit | --think] [prompt]` — how Claude's streamed answer arrives, measured:
//! one real answer (`claude -p` with the chat's stream-json flags, its summarized thinking and its model and effort
//! — `llm.model`/`llm.effort` from tarae's config, the user's then this project's; `--model` wins; it costs usage),
//! every stdout line timestamped as it's read, kept in `target/stream/last.tsv` (ms, line). `--edit` offers it the
//! Write tool and asks for a file, to measure an edit's input as it streams (`input_json_delta` — what
//! `writing.rs` draws); the write itself is refused (`-p` asks no one), nothing is written. `--think` asks a
//! question that takes thought, to measure the summarized thinking. `cargo xtask stream --file F` reads a kept one
//! again instead. The report names the model that answered (claude's `init` line).
//!
//! Says, for the text, thinking and input deltas: their sizes and the gaps between them (bursts = runs closer
//! than `BURST_GAP`), and what tarae's reader (`llm::pump`) makes of them — it batches deltas and sends them
//! `FLUSH` after its last send. Both rules are printed: the old one, where a delta that came sooner waited for the
//! next delta that didn't (or a tool call, the end) however long that was, and today's, where it goes when its
//! time is up — to tell bursts in claude's stream from ones tarae adds. Then what the main loop gets: the batches'
//! gaps, which `writing.rs`'s reveal paces itself by. Last, the stream as the turn's shimmer would see it: every
//! delta of any kind, its stalls (`STALL` and over) and what was going on across each, and the thinking's
//! textless signs (`thinking_tokens`, an empty thinking delta).

use std::io::{BufRead, BufReader, Write as _};
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::Result;

/// tarae's text batching (`llm::pump`).
const FLUSH: f64 = 25.0;
/// Deltas closer than this are one burst.
const BURST_GAP: f64 = 100.0;
/// A quiet this long, any kind of delta, is a stall to the shimmer (`shimmer::STILL_AFTER`).
const STALL: f64 = 1000.0;

const PROMPT: &str = "Explain in about 300 words, in plain prose with no lists, how a rope data structure \
                      keeps edits in the middle of a large text cheap.";

const EDIT_PROMPT: &str = "Write target/stream/probe.rs with the Write tool, in one call and no prose: about 80 \
                           lines of Rust, a small tokenizer for arithmetic expressions with a few unit tests.";

const THINK_PROMPT: &str = "In how many ways can a 3 by 8 rectangle be tiled with 1 by 2 dominoes? Work it out \
                            yourself, then answer with the number and two sentences on how you got it.";

pub fn run(args: &[String]) -> Result<()> {
    let mut args = args.to_vec();
    let mut flag = |name: &str| args.iter().position(|a| a == name).map(|k| args.remove(k)).is_some();
    let (edit, think) = (flag("--edit"), flag("--think"));
    let lines = match args.as_slice() {
        [flag, file] if flag == "--file" => read(file)?,
        [flag, model, rest @ ..] if flag == "--model" => {
            record(&rest.join(" "), Some(model.clone()), edit, think)?
        }
        _ => record(&args.join(" "), None, edit, think)?,
    };
    report(&lines);
    Ok(())
}

/// The chat's `llm.model` and `llm.effort` as tarae reads them: the user's config, then the project's
/// `.tarae.toml` and `.tarae.local.toml` over it ("" = the CLI's default).
fn chat_settings() -> (Option<String>, Option<String>) {
    let home = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")));
    let files = home
        .map(|h| h.join("tarae/config.toml"))
        .into_iter()
        .chain([crate::root().join(".tarae.toml"), crate::root().join(".tarae.local.toml")]);
    let (mut model, mut effort) = (None, None);
    for file in files {
        let Some(table) = std::fs::read_to_string(&file).ok().and_then(|t| t.parse::<toml::Table>().ok())
        else {
            continue;
        };
        let llm = table.get("llm");
        let get = |k: &str| llm.and_then(|l| l.get(k)).and_then(|v| v.as_str()).map(str::to_string);
        model = get("model").or(model);
        effort = get("effort").or(effort);
    }
    let set = |v: Option<String>| v.filter(|v| !v.is_empty());
    (set(model), set(effort))
}

/// One answer, every line with the ms it was read at.
fn record(prompt: &str, model: Option<String>, edit: bool, think: bool) -> Result<Vec<(f64, String)>> {
    let prompt = match prompt.trim().is_empty() {
        true if edit => EDIT_PROMPT,
        true if think => THINK_PROMPT,
        true => PROMPT,
        false => prompt,
    };
    let (chat_model, effort) = chat_settings();
    let model = model.or(chat_model);
    eprintln!(
        "asking {} (effort {})",
        model.as_deref().unwrap_or("the CLI's default model"),
        effort.as_deref().unwrap_or("default")
    );
    let mut cmd = Command::new("claude");
    if let Some(m) = &model {
        cmd.args(["--model", m]);
    }
    if let Some(e) = &effort {
        cmd.args(["--effort", e]);
    }
    let mut child = cmd
        .args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--tools",
            if edit { "Write" } else { "" },
            "--strict-mcp-config",
            "--setting-sources",
            "",
            "--no-session-persistence",
            // (as the chat asks for its thinking — `chat::ensure_proc`)
            "--thinking-display",
            "summarized",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("claude: {e}"))?;
    let message = format!(
        "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"{}\"}}}}",
        prompt.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    writeln!(stdin, "{message}").map_err(|e| e.to_string())?;
    let t0 = Instant::now();
    let mut lines = Vec::new();
    for line in BufReader::new(child.stdout.take().ok_or("no stdout")?).lines().map_while(|l| l.ok()) {
        let at = t0.elapsed().as_secs_f64() * 1000.0;
        let done = line.contains("\"type\":\"result\"");
        lines.push((at, line));
        if done {
            break;
        }
    }
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let dir = crate::root().join("target/stream");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let kept: String = lines.iter().map(|(t, l)| format!("{t:.1}\t{l}\n")).collect();
    std::fs::write(dir.join("last.tsv"), kept).map_err(|e| e.to_string())?;
    eprintln!("kept in target/stream/last.tsv");
    Ok(lines)
}

fn read(file: &str) -> Result<Vec<(f64, String)>> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    Ok(text
        .lines()
        .filter_map(|l| {
            let (t, rest) = l.split_once('\t')?;
            Some((t.parse().ok()?, rest.to_string()))
        })
        .collect())
}

/// What a line is to tarae's reader.
#[derive(Clone, Copy, PartialEq)]
enum Seen {
    /// A text delta of so many chars.
    Text(usize),
    /// A summarized thinking delta of so many chars (batched as text is).
    Thought(usize),
    /// A sign of thinking with no text: an empty thinking delta or a `thinking_tokens` line.
    Tick,
    /// A thinking block began (true) or ended.
    Block(bool),
    /// A request went out: a tool's result back to Claude, or claude's `requesting`.
    Asked,
    /// A piece of a tool call's input (`input_json_delta`), so many chars of its JSON.
    Input(usize),
    /// What makes the reader send what it holds: a tool call's input ended, a whole tool call, a control request,
    /// the end.
    Flush,
    Other,
}

/// Every line, as the reader sees it.
fn seen(lines: &[(f64, String)]) -> Vec<(f64, Seen)> {
    // (the content blocks of the message under way that are tool calls: their end flushes · the thinking block open)
    let mut tools: Vec<u64> = Vec::new();
    let mut thinking: Option<u64> = None;
    lines
        .iter()
        .map(|(t, l)| {
            let Ok(ev) = serde_json::from_str::<serde_json::Value>(l) else { return (*t, Seen::Other) };
            let e = &ev["event"];
            let what = match (ev["type"].as_str(), e["type"].as_str()) {
                (Some("stream_event"), Some("message_start")) => {
                    tools.clear();
                    Seen::Other
                }
                (Some("stream_event"), Some("content_block_start")) => {
                    match e["content_block"]["type"].as_str() {
                        Some("tool_use") => {
                            tools.extend(e["index"].as_u64());
                            Seen::Other
                        }
                        Some("thinking" | "redacted_thinking") => {
                            thinking = e["index"].as_u64();
                            Seen::Block(true)
                        }
                        _ => Seen::Other,
                    }
                }
                (Some("stream_event"), Some("content_block_stop"))
                    if thinking.is_some() && e["index"].as_u64() == thinking =>
                {
                    thinking = None;
                    Seen::Block(false)
                }
                (Some("stream_event"), Some("content_block_delta")) => {
                    let d = &e["delta"];
                    let chars = |k: &str| d[k].as_str().map_or(0, |s| s.chars().count());
                    match d["type"].as_str() {
                        Some("text_delta") => Seen::Text(chars("text")),
                        Some("thinking_delta") => match chars("thinking") {
                            0 => Seen::Tick,
                            n => Seen::Thought(n),
                        },
                        Some("input_json_delta") => Seen::Input(chars("partial_json")),
                        _ => Seen::Other,
                    }
                }
                (Some("stream_event"), Some("content_block_stop"))
                    if e["index"].as_u64().is_some_and(|i| tools.contains(&i)) =>
                {
                    Seen::Flush
                }
                (Some("system"), _) if ev["subtype"] == "thinking_tokens" => Seen::Tick,
                (Some("system"), _) if ev["subtype"] == "status" && ev["status"] == "requesting" => {
                    Seen::Asked
                }
                (Some("user"), _) if l.contains("\"tool_result\"") => Seen::Asked,
                (Some("assistant"), _) if l.contains("\"tool_use\"") => Seen::Flush,
                (Some("control_request" | "result"), _) => Seen::Flush,
                _ => Seen::Other,
            };
            (*t, what)
        })
        .collect()
}

/// What tarae's reader makes of a stream (`reader`).
struct Read {
    /// How long each delta waited, and what it was.
    waits: Vec<(f64, Seen)>,
    /// When each batch went out, and its chars.
    batches: Vec<(f64, usize)>,
}

/// How long each delta waits in tarae's reader, and when each batch goes out — by the old rule (`timer` false: a
/// delta that came within `FLUSH` of the last send waits for the next delta that doesn't, or a flush) or today's
/// (`timer`: it goes when its `FLUSH` is up).
fn reader(seen: &[(f64, Seen)], timer: bool) -> Read {
    let mut last_send = f64::NEG_INFINITY;
    let mut waiting: Vec<(f64, Seen)> = Vec::new();
    let (mut waits, mut batches) = (Vec::new(), Vec::new());
    let mut send = |at: f64, waiting: &mut Vec<(f64, Seen)>, last_send: &mut f64| {
        if waiting.is_empty() {
            return;
        }
        let chars = waiting.iter().map(|(_, s)| delta(*s).unwrap_or(0)).sum();
        batches.push((at, chars));
        waits.extend(waiting.drain(..).map(|(t, s)| (at - t, s)));
        *last_send = at;
    };
    for &(t, s) in seen {
        if timer && !waiting.is_empty() && last_send + FLUSH <= t {
            send(last_send + FLUSH, &mut waiting, &mut last_send);
        }
        match s {
            Seen::Text(_) | Seen::Thought(_) | Seen::Input(_) => {
                waiting.push((t, s));
                if t - last_send >= FLUSH {
                    send(t, &mut waiting, &mut last_send);
                }
            }
            Seen::Flush => send(t, &mut waiting, &mut last_send),
            Seen::Tick | Seen::Block(_) | Seen::Asked | Seen::Other => {}
        }
    }
    if let Some(&(end, _)) = seen.last() {
        send(end, &mut waiting, &mut last_send);
    }
    Read { waits, batches }
}

/// A delta's chars, any kind.
fn delta(s: Seen) -> Option<usize> {
    match s {
        Seen::Text(n) | Seen::Thought(n) | Seen::Input(n) => Some(n),
        _ => None,
    }
}

fn pct(v: &[f64], p: f64) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    v.get(((v.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(0.0)
}

fn report(lines: &[(f64, String)]) {
    // (who answered: the `model` of claude's `system init` line)
    let model = lines.iter().find(|(_, l)| l.contains("\"subtype\":\"init\"")).and_then(|(_, l)| {
        let at = l.find("\"model\":\"")? + 9;
        Some(l[at..].split('"').next()?.to_string())
    });
    println!("model: {}", model.as_deref().unwrap_or("unknown (no init line)"));
    let seen = seen(lines);
    let old = reader(&seen, false);
    let new = reader(&seen, true);
    // (a delta's kind, by name)
    let name = |s: Seen| match s {
        Seen::Text(_) => "text",
        Seen::Thought(_) => "thinking",
        Seen::Input(_) => "input",
        Seen::Tick => "tick",
        Seen::Block(_) | Seen::Asked | Seen::Flush | Seen::Other => "",
    };
    for kind_name in ["text", "thinking", "input"] {
        let deltas: Vec<(f64, usize)> = seen
            .iter()
            .filter(|(_, s)| name(*s) == kind_name)
            .filter_map(|&(t, s)| Some((t, delta(s)?)))
            .collect();
        if deltas.len() < 2 {
            println!("{} {kind_name} deltas — nothing to measure", deltas.len());
            continue;
        }
        let of = |waits: &[(f64, Seen)]| -> Vec<f64> {
            waits.iter().filter(|(_, s)| name(*s) == kind_name).map(|w| w.0).collect()
        };
        kind(kind_name, &deltas, &of(&old.waits), &of(&new.waits));
    }
    shimmer(lines, &seen, name);
    let gaps: Vec<f64> = new.batches.windows(2).map(|w| w[1].0 - w[0].0).collect();
    let sizes: Vec<f64> = new.batches.iter().map(|b| b.1 as f64).collect();
    println!(
        "batches to the main loop (today's rule): {}, gap median {:.0} ms  p90 {:.0} ms; chars a batch median {:.0}  p90 \
         {:.0}",
        new.batches.len(),
        pct(&gaps, 0.5),
        pct(&gaps, 0.9),
        pct(&sizes, 0.5),
        pct(&sizes, 0.9)
    );
}

/// The stream as the turn's shimmer sees it, from the message sent (0 ms) to the end: every delta of any kind — the
/// gaps between them (the message and the end count as their ends), each stall of `STALL` or more with what's on its
/// two sides and how much of it a thinking block was open, the thinking's textless signs (which would be all that comes
/// during its quiet), and what the turn's word would say all along by tarae's rule (`shimmer::pace`; `-p` makes no
/// runs of tarae's).
fn shimmer(lines: &[(f64, String)], seen: &[(f64, Seen)], name: impl Fn(Seen) -> &'static str) {
    let end = seen.last().map_or(0.0, |s| s.0);
    let deltas: Vec<(f64, Seen)> = seen.iter().copied().filter(|(_, s)| delta(*s).is_some()).collect();
    let Some(&(first, _)) = deltas.first() else { return println!("shimmer: no deltas") };
    // (the thinking blocks open, start to end)
    let mut blocks: Vec<(f64, f64)> = Vec::new();
    for &(t, s) in seen {
        match s {
            Seen::Block(true) => blocks.push((t, end)),
            Seen::Block(false) => blocks.last_mut().into_iter().for_each(|b| b.1 = t),
            _ => {}
        }
    }
    let thinking =
        |a: f64, b: f64| -> f64 { blocks.iter().map(|k| (k.1.min(b) - k.0.max(a)).max(0.0)).sum() };
    // (the gaps from the message to the end: the message, each delta, the end)
    let points: Vec<(f64, &str)> = std::iter::once((0.0, "the message"))
        .chain(deltas.iter().map(|&(t, s)| (t, name(s))))
        .chain([(end, "the end")])
        .collect();
    let gaps: Vec<f64> = points.windows(2).map(|w| w[1].0 - w[0].0).collect();
    let quiet = gaps.iter().filter(|g| **g >= STALL).fold(0.0, |a, g| a + g);
    println!(
        "shimmer: {} deltas of any kind; the first {:.1} s after the message, the end {:.1} s after it",
        deltas.len(),
        first / 1000.0,
        end / 1000.0
    );
    println!(
        "  gap between (from the message to the end)  median {:.0} ms  p90 {:.0} ms  max {:.0} ms; quiet ≥ {STALL:.0} \
         ms: {:.1} s of the {:.1} s",
        pct(&gaps, 0.5),
        pct(&gaps, 0.9),
        pct(&gaps, 1.0),
        quiet / 1000.0,
        end / 1000.0
    );
    for w in points.windows(2).filter(|w| w[1].0 - w[0].0 >= STALL) {
        let ((a, from), (b, to)) = (w[0], w[1]);
        let ticks = seen.iter().filter(|(t, s)| *s == Seen::Tick && *t > a && *t < b).count();
        println!(
            "  stall at {:.1} s: {:.1} s, {from} → {to}; a thinking block open {:.1} s of it, {ticks} textless signs",
            a / 1000.0,
            (b - a) / 1000.0,
            thinking(a, b) / 1000.0
        );
    }
    // What the turn's word says, 10 ms a sample, the latest sign winning: the shimmer while the last of them is a
    // delta within `STILL_AFTER`, else the wave while a thinking block is open, before the turn's first event or while
    // a request is out with nothing of its answer yet, else still (a thinking block opening, a request out or a tool
    // call says the stream paused for work)
    let begun =
        seen.iter().find(|(_, s)| delta(*s).is_some() || matches!(s, Seen::Block(_))).map_or(end, |s| s.0);
    let sign = |s: Seen| delta(s).is_some() || matches!(s, Seen::Block(true) | Seen::Asked | Seen::Flush);
    let latest = |t: f64| seen.iter().rfind(|s| s.0 <= t && sign(s.1)).copied();
    let asked = |t: f64| latest(t).is_some_and(|s| s.1 == Seen::Asked);
    let (mut shim, mut wave, mut still) = (0.0, 0.0, 0.0);
    let mut t = 0.0;
    while t < end {
        let recent = latest(t).is_some_and(|s| delta(s.1).is_some() && t - s.0 < STALL);
        match () {
            _ if recent => shim += 10.0,
            _ if t < begun || thinking(t, t + 10.0) > 0.0 || asked(t) => wave += 10.0,
            _ => still += 10.0,
        }
        t += 10.0;
    }
    println!(
        "  the turn's word (shimmer::pace): shimmer {:.1} s · wave {:.1} s · still {:.1} s",
        shim / 1000.0,
        wave / 1000.0,
        still / 1000.0
    );
    let ticks: Vec<f64> = seen.iter().filter(|(_, s)| *s == Seen::Tick).map(|s| s.0).collect();
    let tick_gaps: Vec<f64> = ticks.windows(2).map(|w| w[1] - w[0]).collect();
    println!(
        "  textless thinking signs: {}, gap median {:.0} ms  max {:.0} ms",
        ticks.len(),
        pct(&tick_gaps, 0.5),
        pct(&tick_gaps, 1.0)
    );
    // (one of each, as claude sent it — what such a sign carries)
    // (the lines and what the reader saw of them are one for one)
    for sample in ["\"thinking_tokens\"", "\"content_block_delta\""] {
        let tick = lines.iter().zip(seen).find(|((_, l), s)| s.1 == Seen::Tick && l.contains(sample));
        if let Some(((t, l), _)) = tick {
            println!("  e.g. at {:.1} s: {}", t / 1000.0, l.chars().take(240).collect::<String>());
        }
    }
}

/// One kind of delta: claude's stream, then what each rule of the reader makes it wait.
fn kind(name: &str, deltas: &[(f64, usize)], old: &[f64], new: &[f64]) {
    let first = deltas[0].0;
    let span = deltas[deltas.len() - 1].0 - first;
    let gaps: Vec<f64> = deltas.windows(2).map(|w| w[1].0 - w[0].0).collect();
    let sizes: Vec<f64> = deltas.iter().map(|d| d.1 as f64).collect();
    let chars: usize = deltas.iter().map(|d| d.1).sum();
    let bursts = 1 + gaps.iter().filter(|g| **g > BURST_GAP).count();
    println!(
        "{name}: claude's stream: {} deltas, {chars} chars over {:.1} s ({:.0} chars/s)",
        deltas.len(),
        span / 1000.0,
        chars as f64 / (span / 1000.0)
    );
    println!(
        "  chars a delta  median {:.0}  p90 {:.0}  max {:.0}",
        pct(&sizes, 0.5),
        pct(&sizes, 0.9),
        pct(&sizes, 1.0)
    );
    println!(
        "  gap between    median {:.0} ms  p90 {:.0} ms  max {:.0} ms",
        pct(&gaps, 0.5),
        pct(&gaps, 0.9),
        pct(&gaps, 1.0)
    );
    println!(
        "  bursts (deltas < {BURST_GAP:.0} ms apart): {bursts}, gaps over 250 ms: {}",
        gaps.iter().filter(|g| **g > 250.0).count()
    );
    // tarae's reader, both rules
    for (rule, held) in [("old, held for the next line", old), ("today's, sent when its time is up", new)] {
        println!(
            "  tarae's reader ({rule}): a delta waits median {:.0} ms  p90 {:.0} ms  max {:.0} ms; over 100 ms: {} \
             of {}",
            pct(held, 0.5),
            pct(held, 0.9),
            pct(held, 1.0),
            held.iter().filter(|h| **h > 100.0).count(),
            held.len()
        );
    }
    // The gaps' timeline, a char a 50 ms (· = a delta came, space = quiet), the first 6 s
    let mut row = String::new();
    for k in 0..120 {
        let (a, b) = (first + k as f64 * 50.0, first + (k + 1) as f64 * 50.0);
        row.push(if deltas.iter().any(|d| d.0 >= a && d.0 < b) { '·' } else { ' ' });
    }
    println!("  first 6 s, 50 ms a cell: |{}|", row.trim_end());
}
