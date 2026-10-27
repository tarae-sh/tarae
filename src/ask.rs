//! `space i` (audit 2.B, B′ — decided on Q1's A/B, 2026-10-26: the chat's large context cost 6.4× its small one):
//! one turn of `claude -p` beside the chat, never queued behind its turn and never part of its conversation — on
//! the one-shot footprint (`llm::DEFAULT_ARGS`: no settings, no conversation kept), with tarae's `read` and `edit`
//! alone. Its message is the selections quoted as `space >` quotes them, whole, then the instruction; its change
//! comes through `edit` and is reviewed in the buffer, as every edit is — its own `space u` step, named by what you
//! asked. The chat is told nothing.
//!
//! - **Its tool server** (`control`): the chat's two schemas; a call is a `tools::Call` of this process's generation
//!   (one count with the chats', `tools::generation`), answered through `tools::answer` → `deliver` → its stdin
//!   (`respond`). What it has read and written is its own (`Ask.edits.seen`), its landings its own queue
//!   (`refactor::Whose::Ask`).
//! - **A buffer that isn't a file of the project** (no path, outside the project, a page, a woven view, a read-only
//!   document): its quote names it by an id of this ask's (`buffer-…`, fresh each time), and only this ask's `edit`
//!   and `read` take that id — worked out here against that buffer's text (`edittool::in_buffer`), reviewed in it and
//!   never saved (your `:w` saves it). The chat's `edit` knows no ids: there it's a path like any other. Any path
//!   this ask names goes to `edittool::call`, inside the project or refused.
//! - **Waiting**: the card of today's ask (`term::draw_ask_stream` over a review not ready) — the `new_string` as it
//!   streams; `esc` stops it (`llm::stop_waiting`, its pid in `Editor.llm_pid`), the review takes its place.

use std::path::Path;
use std::time::Instant;

use serde_json::{Value, json};

use crate::document::DocId;
use crate::editor::Editor;
use crate::llm::{self, StreamEv};
use crate::tools::Call;

/// What goes before the instruction, after the quotes (the wording Q1's A/B measured).
pub const ASKS: &str = "Make this change to the quoted code, in its file: ";

/// What the ask's claude is told beside Claude Code's own prompt.
const SYSTEM: &str = "You make one change in the user's editor: the change they ask for, to the code they quote. Each \
<quoted> block is a selection, with the file it's in and its lines. Make the change with the edit tool, in that file \
— in one call when you can; read with the read tool only what you need to get it right. A quote with a buffer \
attribute is an editor buffer, not a file: give that id as edit's file_path, and to read. Don't explain the change: \
the user reviews it in their editor.";

/// Its tools: tarae's two, none of Claude Code's.
const ALLOWED: &str = "mcp__tarae__read,mcp__tarae__edit";
const DISALLOWED: &str = "Read,Grep,Glob,Edit,Write,NotebookEdit,Bash,WebFetch,WebSearch,Task";

/// The most requests it makes: it has tools now, and a few reads and an edit's retry fit.
const MAX_TURNS: &str = "10";

/// What a read past the project and its buffer is told: it asks no card (no one is in a conversation to answer it).
pub const READS_ONLY: &str = "space i reads only the project and the buffer it was asked in";

/// `space i`'s claude, while it runs (`Session.ask`).
pub struct Ask {
    /// Its tool calls' (`tools::Call`): one count with the chats'.
    pub generation: u64,
    stdin: llm::Lines,
    /// Its process (None: a test's stand-in).
    pid: Option<u32>,
    /// What you asked — its review's title and its `space u` step's.
    pub instruction: String,
    /// The buffer it was asked in.
    doc: DocId,
    /// That buffer's id when it isn't a file of the project: `edit` and `read` take it (and only this ask's).
    buffer: Option<String>,
    /// Its waiting card's review (`llm_generation`), until a review takes its place.
    waiting: Option<u64>,
    started: Instant,
    /// Its landings and what it has seen (never the chat's).
    pub edits: crate::edits::Edits,
    /// A review of its change opened (said once, `claude answered in …`).
    reviewed: bool,
    /// The item of its `edit` being written (the card's preview takes a line between items).
    writing: Option<usize>,
    billing: crate::billing::Billing,
    keep: bool,
    /// Where its selections' lines start and end, in the text they were taken from (`taken`): its review cuts a
    /// change there (`cut`).
    bounds: Vec<usize>,
    taken: ropey::Rope,
}

/// What `space i` makes of the current buffer: Ok(None) — a file of the project, `edit`'s by its path; Ok(Some) — a
/// scratch buffer or a file outside the project, taken by an id of its own (the user's decision, 2026-10-26); Err —
/// one tarae shows read-only (a page, the woven view, a run's output or the chat's transcript swapped in, any
/// read-only or virtual document): refused, said. An accepted change there would be put back the next event, and a
/// page's text never reaches Claude unframed.
fn taken_as(ed: &Editor) -> Result<Option<String>, String> {
    let d = ed.doc();
    let pane = crate::docpane::swapped(ed).is_some();
    if d.woven.is_some() || d.page.is_some() || d.read_only.is_some() || d.virtual_uri.is_some() || pane {
        let name = d.display_name(&ed.workspace.root);
        return Err(format!("space i changes what you can edit — {name} is read-only"));
    }
    let root = crate::nav::project_root(ed);
    let file = d.path.as_ref().is_some_and(|p| crate::paths::inside(&root, p).is_some());
    Ok((!file).then(|| format!("buffer-{}", &crate::quote::id_for(&d.text.to_string())[1..])))
}

/// `:ask <instruction>` (`space i`'s prompt): the selections quoted, its claude started with the message, the
/// waiting card up.
pub fn start(ed: &mut Editor, instruction: &str) -> Result<(), String> {
    let instruction = instruction.trim();
    if instruction.is_empty() {
        return Err("usage: :ask <instruction>".into());
    }
    if ed.workspace.claude.review.is_some() || ed.workspace.claude.ask.is_some() {
        return Err("an ask is already in progress (:ask-cancel)".into());
    }
    let doc = ed.doc();
    // (every range at least a character, as `space i` always took them)
    let ranges: Vec<(usize, usize)> = doc
        .selection()
        .ranges()
        .iter()
        .map(|r| {
            let r = r.min_width_1(&doc.text);
            (r.from(), r.to())
        })
        .collect();
    let hunks: Vec<llm::Hunk> = ranges
        .iter()
        .map(|&(from, to)| llm::Hunk {
            from,
            to,
            old: doc.text.byte_slice(from..to).to_string(),
            new: String::new(),
            decision: llm::Decision::Undecided,
            conflict: false,
        })
        .collect();
    // (each selection's lines, from the first's start to the line after its last — sorted as the selections are, which
    // never overlap; a bound is a cut only between selections: none where a neighbour's lines go on past it, as two
    // sharing a line)
    let text = &doc.text;
    let line_start = |l: usize| text.line_to_byte(l.min(text.len_lines()));
    let spans: Vec<(usize, usize)> = (ranges.iter())
        .map(|&(from, to)| {
            let last = text.byte_to_line(to.saturating_sub(1).max(from));
            (line_start(text.byte_to_line(from)), line_start(last + 1))
        })
        .collect();
    let mut bounds: Vec<usize> = (spans.iter().enumerate())
        .flat_map(|(i, &(start, end))| {
            let before = i.checked_sub(1).map(|i| spans[i].1);
            let after = spans.get(i + 1).map(|s| s.0);
            let start = before.is_none_or(|e| e <= start).then_some(start);
            let end = after.is_none_or(|s| end <= s).then_some(end);
            start.into_iter().chain(end)
        })
        .collect();
    crate::sorting::sort(&mut bounds);
    bounds.dedup();
    let taken = text.clone();
    let doc_id = doc.id;
    let buffer = taken_as(ed)?;
    let quotes = crate::quote::taken(ed, &ranges, buffer.as_deref())?;
    let blocks: Vec<String> = quotes.into_iter().map(|t| crate::quote::whole(t).block()).collect();
    let message = format!("{}\n\n{ASKS}{instruction}", blocks.join("\n"));
    let cfg = ed.config.llm.clone();
    let cred = ed.credential(ed.workspace.claude.billing, crate::billing::Role::Ask)?;
    let (billing, keep) = (cred.billing(), cfg.keep_env_key);
    let (stdin, child) = match stand_in(ed) {
        Some(lines) => (lines, None),
        None => {
            let cfg = llm::LlmConfig { args: args(&cfg.args), ..cfg };
            let extra = ["--append-system-prompt", SYSTEM, "--permission-prompt-tool", "stdio"];
            let mut child = llm::spawn(&cfg, &extra, &ed.workspace.root, ed.claude_env.as_ref(), &cred)
                .map_err(|e| e.said(&cfg.command, false))?;
            let Some(stdin) = child.stdin.take().map(llm::Lines::new) else {
                reap(child);
                return Err("claude: no stdin".into());
            };
            (stdin, Some(child))
        }
    };
    // (tarae's MCP server first, then the one message — written off the main loop: a selection can be 200 KB, and a
    // claude still starting reads nothing yet)
    let sent =
        stdin.send(llm::initialize_message(&["tarae"])).and_then(|_| stdin.send(llm::user_message(&message)));
    if let Err(e) = sent {
        if let Some(child) = child {
            reap(child);
        }
        return Err(format!("claude: {e}"));
    }
    ed.llm_generation += 1;
    let waiting = ed.llm_generation;
    ed.workspace.claude.review = Some(llm::Review::new(waiting, doc_id, instruction, hunks));
    let generation = crate::tools::generation();
    let pid = child.as_ref().map(std::process::Child::id);
    ed.llm_pid = pid;
    ed.workspace.claude.ask = Some(Ask {
        generation,
        stdin,
        pid,
        instruction: instruction.to_string(),
        doc: doc_id,
        buffer,
        waiting: Some(waiting),
        started: crate::clock::now(),
        edits: Default::default(),
        reviewed: false,
        writing: None,
        billing,
        keep,
        bounds,
        taken,
    });
    if let Some(child) = child {
        llm::pump(child, true, ed.events.sender(), move |ev| {
            Some(Box::new(move |ed: &mut Editor| on_event(ed, generation, ev)))
        });
    }
    Ok(())
}

/// Tests: the ask running — its generation, and what its `edit` names its buffer by (its id, else its path).
#[cfg(test)]
pub fn target(ed: &Editor) -> (u64, String) {
    let a = ed.workspace.claude.ask.as_ref().expect("an ask runs");
    let path = || ed.docs.iter().find(|d| d.id == a.doc).and_then(|d| d.path.clone()).unwrap_or_default();
    (a.generation, a.buffer.clone().unwrap_or_else(|| path().display().to_string()))
}

/// A child that took no message ended and reaped, off the main thread.
fn reap(mut child: std::process::Child) {
    crate::event::thread("tarae-reap", move || {
        let _ = child.kill();
        let _ = child.wait();
    });
}

/// A test's stand-in claude (`Editor.ask_lines`): no process, its lines kept.
#[cfg(test)]
fn stand_in(ed: &Editor) -> Option<llm::Lines> {
    ed.ask_lines.clone().map(llm::Lines::to)
}
#[cfg(not(test))]
fn stand_in(_: &Editor) -> Option<llm::Lines> {
    None
}

/// Its claude's arguments from `base` (`llm.args`): never a conversation to continue, no settings, tarae's two tools
/// only, a request cap.
fn args(base: &[String]) -> Vec<String> {
    let mut args: Vec<String> = Vec::with_capacity(base.len() + 12);
    let mut it = base.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--resume" | "--session-id" => {
                it.next();
            }
            "--continue" | "--fork-session" => {}
            _ => args.push(a.clone()),
        }
    }
    let mut args = llm::with_tools(&args, "", ALLOWED);
    if !args.iter().any(|a| a == "--no-session-persistence") {
        args.push("--no-session-persistence".into());
    }
    for (flag, value) in [
        ("--setting-sources", ""),
        ("--disallowedTools", DISALLOWED),
        ("--permission-mode", "default"),
        ("--max-turns", MAX_TURNS),
    ] {
        args = llm::with_flag(&args, flag, value);
    }
    args
}

/// The ask of `generation`, if it's still the session's.
fn of(ed: &mut Editor, generation: u64) -> Option<&mut Ask> {
    ed.workspace.claude.ask.as_mut().filter(|a| a.generation == generation)
}

/// Its change to `doc`, from `before`, as its review shows it: cut where its selections' lines meet, each its own
/// change as the selections were (#135) — only in the text they were taken from (one changed since, or another
/// file, is reviewed by line hunks alone).
pub fn cut(
    ed: &mut Editor,
    generation: u64,
    doc: DocId,
    before: &ropey::Rope,
    changes: Vec<(usize, usize, String, String)>,
) -> Vec<(usize, usize, String, String)> {
    match of(ed, generation) {
        Some(a) if a.doc == doc && a.taken == *before => crate::editdiff::cut_at(changes, &a.bounds),
        _ => changes,
    }
}

/// Its waiting card's review, while it waits.
fn waiting(ed: &mut Editor, generation: u64) -> Option<&mut llm::Review> {
    let w = of(ed, generation)?.waiting?;
    ed.workspace.claude.review.as_mut().filter(|r| r.generation == w && !r.ready)
}

/// An event of its stream.
pub(crate) fn on_event(ed: &mut Editor, generation: u64, ev: StreamEv) {
    if of(ed, generation).is_none() {
        return;
    }
    match ev {
        StreamEv::Thinking | StreamEv::Thought(_) => {
            if let Some(r) = waiting(ed, generation) {
                r.thinking = true;
            }
        }
        StreamEv::Text(_) => {
            if let Some(r) = waiting(ed, generation) {
                r.thinking = false;
            }
        }
        // (its `edit`'s new text as it's written: the card's preview, a line between items)
        StreamEv::Writing(d) => {
            for p in d.pieces {
                let crate::writing::Piece::Text { item, field: crate::writing::Field::New, add, .. } = p
                else {
                    continue;
                };
                let Some(a) = of(ed, generation) else { return };
                let next = a.writing.is_some_and(|i| i != item);
                a.writing = Some(item);
                if let Some(r) = waiting(ed, generation) {
                    r.thinking = false;
                    if next {
                        r.streamed.push('\n');
                    }
                    r.streamed.push_str(&add);
                }
            }
        }
        StreamEv::Control { request_id, request } => {
            if let Some(response) = control(ed, generation, &request_id, &request) {
                respond(ed, &request_id, response);
            }
        }
        // (what it cost is the session's: a turn of its own, beside the chat's conversation)
        StreamEv::Spent(s) if !s.counts.any() && s.total_cost.is_none() => {}
        StreamEv::Spent(s) => {
            ed.workspace.claude.usage.aside(s, "ask");
            ed.calls_record();
            crate::usage::view_sync(ed);
        }
        // (the account's limits, whichever claude says them)
        StreamEv::Limits(r) => crate::limits::seen(ed, r),
        // (the credential it says it runs on — another than its billing's stops it: `billing.rs`)
        StreamEv::Conversation { key_source, .. } => {
            let Some(a) = of(ed, generation) else { return };
            let role = crate::billing::Role::Ask;
            if let Some(said) = crate::billing::checked(a.billing, a.keep, role, key_source.as_deref()) {
                crate::log::line("claude", "warn", &said);
                stop(ed, generation, &said);
            }
        }
        StreamEv::Done(r) => ended(ed, generation, r),
        StreamEv::Exit(e) => {
            ended(ed, generation, Err(if e.is_empty() { "claude stopped".into() } else { e }))
        }
        StreamEv::Requesting(_)
        | StreamEv::Retry(_)
        | StreamEv::Money(_)
        | StreamEv::Usage(_)
        | StreamEv::Answered(_)
        | StreamEv::Initialized { .. }
        | StreamEv::Tool { .. }
        | StreamEv::ThinkingBlock(_)
        | StreamEv::ToolResult { .. }
        | StreamEv::Compaction(_) => {}
    }
}

/// Stopped (`said` why): killed, nothing more of it applied.
fn stop(ed: &mut Editor, generation: u64, said: &str) {
    let w = of(ed, generation).and_then(|a| a.waiting);
    match w {
        Some(w) if waiting(ed, generation).is_some() => llm::stop_waiting(ed, Some(w), said),
        _ => {
            let what = gone(ed, generation).map(|a| a.instruction).unwrap_or_default();
            ed.set_warning(format!("Claude's {what}: {said}"));
        }
    }
}

/// The ask of `generation` gone from the session: its process killed (one still running), its cards dropped (no
/// one waits for their answers). A review of its change still up stays: your answer is yours.
fn gone(ed: &mut Editor, generation: u64) -> Option<Ask> {
    let a = ed.workspace.claude.ask.take_if(|a| a.generation == generation)?;
    if let Some(pid) = a.pid {
        llm::kill(pid);
        if ed.llm_pid == Some(pid) {
            ed.llm_pid = None;
        }
    }
    let mine = |w: &crate::offer::What| matches!(w, crate::offer::What::ChangeSet { whose: crate::refactor::Whose::Ask(g), .. } if *g == generation);
    ed.drop_cards(mine);
    Some(a)
}

/// Its turn over: its waiting card down; an error said; a turn that changed nothing, said so.
fn ended(ed: &mut Editor, generation: u64, r: Result<String, String>) {
    let still = waiting(ed, generation).is_some();
    let Some(a) = gone(ed, generation) else { return };
    if still {
        ed.workspace.claude.review = None;
    }
    match r {
        Err(e) => ed.set_error(format!("claude: {e}")),
        Ok(_) if !a.reviewed && a.edits.sets.is_empty() => ed.set_status("claude: no changes"),
        Ok(_) => {}
    }
}

/// A waiting review closed from outside (`llm::close_review`: `esc`, `:ask-cancel`, another review in its place):
/// the ask it was for is over.
pub fn closed(ed: &mut Editor, review: u64) {
    if let Some(g) =
        ed.workspace.claude.ask.as_ref().filter(|a| a.waiting == Some(review)).map(|a| a.generation)
    {
        gone(ed, g);
    }
}

/// `:ask-cancel`: whatever ask runs, stopped.
pub fn cancel(ed: &mut Editor) {
    if let Some(g) = ed.workspace.claude.ask.as_ref().map(|a| a.generation) {
        gone(ed, g);
    }
}

/// Its change came (`refactor::queue`, or its buffer's): its waiting card gives way to the review — the process
/// goes on, its call waits for your answer.
pub fn landing(ed: &mut Editor, generation: u64) {
    if waiting(ed, generation).is_some() {
        ed.workspace.claude.review = None;
    }
    if let Some(a) = of(ed, generation) {
        a.waiting = None;
    }
}

/// Its review opened: said as `space i`'s always was, once.
pub fn reviewing(ed: &mut Editor) {
    let Some(a) = ed.workspace.claude.ask.as_mut().filter(|a| !a.reviewed) else { return };
    a.reviewed = true;
    let took = crate::clock::since(a.started);
    ed.set_status(format!("claude answered in {}", crate::term::run_duration(took)));
}

/// Our answer to one of its control requests — to its process only.
pub fn respond(ed: &mut Editor, request_id: &str, response: Value) {
    if let Some(a) = ed.workspace.claude.ask.as_ref() {
        let _ = a.stdin.send(llm::control_response(request_id, response));
    }
}

/// A control request from its claude → our answer (None: later — a read, an edit). No tool of Claude Code's is its;
/// `mcp_message` is its server: `read` and `edit`.
fn control(ed: &mut Editor, generation: u64, request_id: &str, req: &Value) -> Option<Value> {
    if req["subtype"] == "can_use_tool" {
        let tool = req["tool_name"].as_str().unwrap_or_default();
        return Some(json!({ "behavior": "deny", "message": refused(tool) }));
    }
    if req["subtype"] != "mcp_message" || req["server_name"] != "tarae" {
        return Some(json!({}));
    }
    let msg = &req["message"];
    let Some(id) = msg.get("id").cloned() else {
        return Some(json!({ "mcp_response": { "jsonrpc": "2.0", "result": {}, "id": 0 } }));
    };
    let rev = match crate::mcp::rev(msg) {
        Ok(rev) => rev,
        Err(e) => return Some(json!({ "mcp_response": { "jsonrpc": "2.0", "id": id, "error": e } })),
    };
    let method = msg["method"].as_str().unwrap_or_default();
    let result = match method {
        "tools/list" => Ok(json!({ "tools": [
            crate::tools::annotated(crate::readtool::schema()),
            crate::tools::annotated(crate::edittool::schema()),
        ] })),
        "tools/call" => {
            let (name, args) =
                (msg["params"]["name"].as_str().unwrap_or_default(), &msg["params"]["arguments"]);
            let call = Call::new(generation, request_id, id.clone(), rev, name);
            let r = match name {
                "read" => read(ed, generation, call, args),
                "edit" => edit(ed, generation, call, args),
                other => Err(refused(other)),
            };
            match r {
                Ok(()) => return None,
                Err(e) => Ok(crate::tools::content(Err(e))),
            }
        }
        m => crate::mcp::common(m, msg, rev, json!({ "tools": {} })).ok_or_else(|| crate::mcp::not_found(m)),
    };
    let response = match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": crate::mcp::finish(rev, method, r) }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
    };
    Some(json!({ "mcp_response": response }))
}

fn refused(tool: &str) -> String {
    format!("{tool} isn't space i's: it reads with read and makes the change with edit, nothing else")
}

/// Its `read`: of its buffer by the id (answered here), else of the project's files (`readtool`) — never a page.
fn read(ed: &mut Editor, generation: u64, call: Call, args: &Value) -> Result<(), String> {
    let files: Vec<&str> = args["files"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if files.iter().any(|f| crate::web::tool::is_url(f)) {
        return Err(format!("{READS_ONLY}, not pages"));
    }
    let buffer = of(ed, generation).and_then(|a| a.buffer.clone());
    let ours = |f: &str| buffer.as_deref().is_some_and(|b| crate::readtool::item(f).path == b);
    match (files.iter().filter(|f| ours(f)).count(), files.len()) {
        (0, _) => crate::readtool::call(ed, call, args),
        (n, all) if n < all => Err("read the buffer and files in calls of their own".into()),
        _ => {
            let text = buffer_lines(ed, generation, &files)?;
            crate::tools::answer(ed, call, Ok(text));
            Ok(())
        }
    }
}

/// Its buffer's lines as `read` reads a file's specs (`id`, `id:a-b,c-d`, `id:-30`; an end past the text clamped —
/// `readtool::ranges` and `lines_of`, the path's own code), only those asked taken from the rope.
fn buffer_lines(ed: &mut Editor, generation: u64, specs: &[&str]) -> Result<String, String> {
    let a = of(ed, generation).ok_or("space i is over")?;
    let (doc, id) = (a.doc, a.buffer.clone().unwrap_or_default());
    let d = ed.docs.iter().find(|d| d.id == doc).ok_or("the buffer was closed")?;
    let text = &d.text;
    // (its lines as `str::lines` counts a file's: none after a last line break)
    let ends = text.len_bytes() > 0 && text.byte(text.len_bytes() - 1) == b'\n';
    let n = if text.len_bytes() == 0 { 0 } else { text.len_lines() - usize::from(ends) };
    let line = |l: usize| text.line(l - 1).to_string().trim_end_matches(['\n', '\r']).to_string();
    let name = d.display_name(Path::new("/"));
    let mut left = crate::readtool::LINES_MAX;
    let mut out = format!("({id} is {name}, a buffer of the editor — no file)\n");
    for spec in specs {
        let it = crate::readtool::item(spec);
        let Some(ranges) = crate::readtool::ranges(&it.want, n, &id) else {
            out.push_str(&format!(
                "== {spec}: a buffer has no symbols to read by name — ask for line ranges\n"
            ));
            continue;
        };
        out.push_str(&crate::readtool::lines_of(ranges, n, &line, &id, &mut left, &mut Vec::new()));
    }
    Ok(out)
}

/// Its `edit`: of its buffer by the id — worked out here, reviewed in it, never saved — else the edit tool's, on the
/// project's files (`edittool::call`: inside the project or refused).
fn edit(ed: &mut Editor, generation: u64, call: Call, args: &Value) -> Result<(), String> {
    let a = of(ed, generation).ok_or("space i is over")?;
    let (doc, instruction) = (a.doc, a.instruction.clone());
    let Some(id) = a.buffer.clone() else { return crate::edittool::call(ed, call, args) };
    let named = args["edits"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|e| e["file_path"].as_str().map(str::trim) == Some(id.as_str()));
    if !named {
        return crate::edittool::call(ed, call, args);
    }
    let text = ed.docs.iter().find(|d| d.id == doc).ok_or("the buffer was closed")?.text.clone();
    let before = text.to_string();
    let after = crate::edittool::in_buffer(&before, args, &id)?;
    if after == before {
        crate::tools::answer(ed, call, Ok(format!("nothing to change — {id} is as it was")));
        return Ok(());
    }
    // (the review slot is another's — a chat edit's, once this ask's own review was answered)
    if ed.workspace.claude.review.as_ref().is_some_and(|r| r.ready) {
        return Err("the user is answering another change — call again in a moment".into());
    }
    landing(ed, generation);
    let changes = crate::editdiff::line_changes(&before, &after);
    let changes = cut(ed, generation, doc, &text, changes);
    let done = move |ed: &mut Editor, applied: bool| {
        let now = ed.docs.iter().find(|d| d.id == doc).map(|d| d.text.to_string());
        let said = match now {
            _ if !applied => format!("declined by the user — {id} left as it was"),
            Some(now) if now == after => format!("accepted by the user — in {id}, unsaved (it isn't a file)"),
            Some(_) => format!("accepted in part by the user — read {id} again before changing it"),
            None => format!("not changed: {id} was closed"),
        };
        crate::tools::answer(ed, call, Ok(said));
    };
    if let Some(i) = ed.docs.iter().position(|d| d.id == doc) {
        ed.current = i;
    }
    llm::review_with(ed, doc, &instruction, changes, Some(Box::new(done)));
    reviewing(ed);
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Its arguments from `llm.args` with a conversation's flags in them: none of those survives, and it has no
    /// settings, Claude Code's tools disallowed, tarae's two allowed, its request cap.
    #[test]
    fn its_args_carry_no_conversation_and_its_own_limits() {
        let mut base = crate::llm::LlmConfig::default().args;
        base.extend(["--resume", "x", "--continue", "--session-id", "y", "--fork-session"].map(String::from));
        let args = super::args(&base);
        for gone in ["--resume", "x", "--continue", "--session-id", "y", "--fork-session"] {
            assert!(!args.iter().any(|a| a == gone), "{gone} in {args:?}");
        }
        let value = |flag: &str| {
            let at: Vec<usize> =
                args.iter().enumerate().filter(|(_, a)| *a == flag).map(|(i, _)| i).collect();
            assert_eq!(at.len(), 1, "{flag} once: {args:?}");
            args[at[0] + 1].as_str()
        };
        assert_eq!(value("--setting-sources"), "");
        assert_eq!(value("--disallowedTools"), super::DISALLOWED);
        assert_eq!(value("--max-turns"), "10");
        assert_eq!(value("--tools"), "");
        assert_eq!(value("--allowedTools"), "mcp__tarae__read,mcp__tarae__edit");
        assert_eq!(value("--permission-mode"), "default");
        assert!(args.iter().any(|a| a == "--no-session-persistence") && args.iter().any(|a| a == "-p"));
    }
}
