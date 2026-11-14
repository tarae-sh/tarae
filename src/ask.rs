//! `space i` (audit 2.B, B′ — decided on Q1's A/B, 2026-10-26: the chat's large context cost 6.4× its small one):
//! one turn of `claude -p` beside the chat, never queued behind its turn and never part of its conversation — on
//! the one-shot footprint (`agent::DEFAULT_ARGS`: no settings, no conversation kept), with tarae's `read` and `edit`
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

use crate::agent::{self, Agent, Event, Permission, Prompt};
use crate::document::DocId;
use crate::editor::Editor;
use crate::llm;
use crate::tools::Call;

/// What goes before the instruction, after the quotes (the wording Q1's A/B measured).
pub const ASKS: &str = "Make this change to the quoted code, in its file: ";

/// What the ask's claude is told beside Claude Code's own prompt.
const SYSTEM: &str = "You make one change in the user's editor: the change they ask for, to the code they quote. Each \
<quoted> block is a selection, with the file it's in and its lines. Make the change with the edit tool, in that file \
— in one call when you can; read with the read tool only what you need to get it right. A quote with a buffer \
attribute is an editor buffer, not a file: give that id as edit's file_path, and to read. Don't explain the change: \
the user reviews it in their editor.";

/// What a read past the project and its buffer is told: it asks no card (no one is in a conversation to answer it).
pub const READS_ONLY: &str = "space i reads only the project and the buffer it was asked in";

/// `space i`'s claude, while it runs (`Session.ask`).
pub struct Ask {
    /// Its tool calls' (`tools::Call`): one count with the chats'.
    pub generation: u64,
    agent: Agent,
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
    /// The input that asked (`Events::asked` as it started): its review is focused as it comes while that input is still
    /// the last one — a key typed since, it waits for `C-q` (#528, as `Offer.yours`).
    asked_by: Option<u64>,
}

/// Whether `space i`'s review of `generation` is yours as it comes: nothing typed since the `space i` that asked.
pub fn yours(ed: &Editor, generation: u64) -> bool {
    ed.workspace
        .claude
        .ask
        .as_ref()
        .filter(|a| a.generation == generation)
        .is_some_and(|a| a.asked_by.is_some() && a.asked_by == Some(ed.input_seq))
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

/// The ask's start on `cfg`: tarae's words, its tools tarae's two, no conversation, no settings, a request cap —
/// its wire's flags.
pub(crate) fn agent_start(cfg: &llm::LlmConfig) -> agent::Start<'_> {
    agent::Start { system: Some(SYSTEM), ..agent::Start::new(agent::Role::Ask, cfg) }
}

/// `:ask <instruction>` (`space i`'s prompt): the selections quoted, its claude started with the message, the
/// waiting card up.
pub fn start(ed: &mut Editor, instruction: &str) -> Result<(), String> {
    let instruction = instruction.trim();
    if instruction.is_empty() {
        return Err("usage: :ask <instruction>".into());
    }
    if let Some(why) = llm::held(ed) {
        return Err(why);
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
    let cfg = crate::models::config(ed);
    let cred = ed.credential(ed.workspace.claude.billing, crate::billing::Role::Ask)?;
    let (billing, keep) = (cred.billing(), cfg.keep_env_key);
    // (tarae's MCP server first, then the one message — written off the main loop: a selection can be 200 KB, and a
    // claude still starting reads nothing yet)
    let first = agent::First { tools: true, prompt: Some(Prompt::Text(message)) };
    let generation = crate::tools::generation();
    let (agent, pid) = match stand_in(ed) {
        Some(agent) => {
            let sent = agent.initialize().and_then(|_| first.prompt.map_or(Ok(()), |p| agent.prompt(p)));
            sent.map_err(|e| format!("claude: {e}"))?;
            (agent, None)
        }
        None => {
            let at = agent::Place { dir: &ed.workspace.root, given: ed.claude_env.as_ref(), cred: &cred };
            let route = move |ev| -> Option<crate::event::Apply> {
                Some(Box::new(move |ed: &mut Editor| on_event(ed, generation, ev)))
            };
            let started = agent::start(&agent_start(&cfg), at, first, true, ed.events.sender(), route)
                .map_err(|e| e.said(&cfg.command, false))?;
            (started.agent, Some(started.pid))
        }
    };
    ed.llm_generation += 1;
    let waiting = ed.llm_generation;
    ed.workspace.claude.review = Some(llm::Review::new(waiting, doc_id, instruction, hunks));
    ed.llm_pid = pid;
    ed.workspace.claude.ask = Some(Ask {
        generation,
        agent,
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
        asked_by: ed.events.asked(),
    });
    Ok(())
}

/// Tests: the ask running — its generation, and what its `edit` names its buffer by (its id, else its path).
#[cfg(test)]
pub fn target(ed: &Editor) -> (u64, String) {
    let a = ed.workspace.claude.ask.as_ref().expect("an ask runs");
    let path = || ed.docs.iter().find(|d| d.id == a.doc).and_then(|d| d.path.clone()).unwrap_or_default();
    (a.generation, a.buffer.clone().unwrap_or_else(|| path().display().to_string()))
}

/// A test's stand-in claude (`Editor.ask_lines`): no process, its lines kept.
#[cfg(test)]
fn stand_in(ed: &Editor) -> Option<Agent> {
    ed.ask_lines.clone().map(Agent::stand_in)
}
#[cfg(not(test))]
fn stand_in(_: &Editor) -> Option<Agent> {
    None
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
pub(crate) fn on_event(ed: &mut Editor, generation: u64, ev: Event) {
    if of(ed, generation).is_none() {
        return;
    }
    match ev {
        Event::Thinking | Event::Thought(_) => {
            if let Some(r) = waiting(ed, generation) {
                r.thinking = true;
            }
        }
        Event::Text(_) => {
            if let Some(r) = waiting(ed, generation) {
                r.thinking = false;
            }
        }
        // (its `edit`'s new text as it's written: the card's preview, a line between items)
        Event::Writing(d) => {
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
        Event::Mcp { id, message } => {
            if let Some(response) = mcp(ed, generation, &id, &message) {
                answer_mcp(ed, &id, response);
            }
        }
        // (no tool of Claude Code's is its)
        Event::Permission { id, tool, .. } => {
            if let Some(a) = ed.workspace.claude.ask.as_ref() {
                a.agent.answer_permission(&id, Permission::Deny { message: refused(&tool) });
            }
        }
        // (what it cost is the session's: a turn of its own, beside the chat's conversation)
        Event::Spent(s) if !s.counts.any() && s.total_cost.is_none() => {}
        Event::Spent(s) => {
            ed.workspace.claude.usage.aside(s, "ask");
            ed.calls_record();
            crate::usage::view_sync(ed);
        }
        // (the account's limits, whichever claude says them)
        Event::Limits(r) => crate::limits::seen(ed, r),
        // (the credential it says it runs on — another than its billing's stops it: `billing.rs`)
        Event::Conversation { key_source, .. } => {
            let Some(a) = of(ed, generation) else { return };
            let role = crate::billing::Role::Ask;
            if let Some(said) = crate::billing::checked(a.billing, a.keep, role, key_source.as_deref()) {
                crate::log::line("claude", "warn", &said);
                stop(ed, generation, &said);
            }
        }
        Event::Done(r) => ended(ed, generation, r),
        Event::Exit(e) => ended(ed, generation, Err(if e.is_empty() { "claude stopped".into() } else { e })),
        Event::Requesting(_)
        | Event::Retry(_)
        | Event::Money(_)
        | Event::Usage(_)
        | Event::Answered(_)
        | Event::Ready { .. }
        | Event::Tool { .. }
        | Event::ThinkingBlock(_)
        | Event::ToolResult { .. }
        | Event::Searched(_)
        | Event::Compaction(_) => {}
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
        agent::kill(pid);
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

/// Its MCP server's answer (`response` a JSON-RPC message) to ask `id` — to its process only.
pub fn answer_mcp(ed: &mut Editor, id: &str, response: Value) {
    if let Some(a) = ed.workspace.claude.ask.as_ref() {
        a.agent.answer_mcp(id, response);
    }
}

/// A message to its MCP server (`read` and `edit`) → our answer (None: later — a read, an edit).
fn mcp(ed: &mut Editor, generation: u64, request_id: &str, msg: &Value) -> Option<Value> {
    let Some(id) = msg.get("id").cloned() else {
        return Some(json!({ "jsonrpc": "2.0", "result": {}, "id": 0 }));
    };
    let rev = match crate::mcp::rev(msg) {
        Ok(rev) => rev,
        Err(e) => return Some(json!({ "jsonrpc": "2.0", "id": id, "error": e })),
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
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": crate::mcp::finish(rev, method, r) }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
    })
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
    let mut left = crate::readtool::Left::answer();
    let mut out = format!("({id} is {name}, a buffer of the editor — no file)\n");
    for spec in specs {
        let it = crate::readtool::item(spec);
        let Some(ranges) = crate::readtool::ranges(&it.want, n, &id) else {
            out.push_str(&format!(
                "== {spec}: a buffer has no symbols to read by name — ask for line ranges\n"
            ));
            continue;
        };
        let whole = it.want != crate::readtool::Want::Whole;
        out.push_str(&crate::readtool::lines_of(ranges, n, &line, &id, whole, &mut left, &mut Vec::new()));
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
    if yours(ed, generation) {
        llm::review_focus(ed);
    }
    reviewing(ed);
    Ok(())
}
