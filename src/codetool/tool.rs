//! The `code` tool (`.sb/codemode-compute-plan.md` §2, S1's piece b): a program of Claude's run on the engine
//! (`codetool.rs`), its calls tarae's read-only tools.
//!
//! - **Listed** only by a chat whose process started with `llm.code` on (`Chat.code`), after the rest: the list is
//!   the cache's prefix, the same to the byte for the process's life; off, today's list to the byte.
//! - **One gate**: each call a program makes is a `tools::Call` of the code call's own (`Call::inner`), through the
//!   tool's own function — `reads::Gate`, the read card (its title says it's Claude's program asking), the same
//!   refusals — and its answer comes back here (`tools::answer_parts` → `answered`), never to the chat. A refused or
//!   failed call is `{ error }`, a value the program reads. Only the read-only tools are bound (`BINDINGS`).
//! - **Calls handed out together run together** (the user, 2026-11-11): a `pending` list's calls all start at once,
//!   each tool doing its work as it does for Claude (its jobs, a server's requests sent together, a search's walkers
//!   — those shared by the batch, `grep::shared`); their answers go back **in the order handed out**, one `resume`
//!   each, whichever finished first — so the program's result, steps and fuel, and the call record, are the same on
//!   every run. A call waiting on a card holds back the answers after it, never the others' work.
//! - **`output("run#N")`**: a run's whole output as tarae kept it (`Kept`) — this conversation's last `KEPT` runs,
//!   until you quit; a run's answer names its id while the chat lists `code` (`named`).
//! - **The turn's end** stops every program (`turn_over`, `Watch::stop`): one running ends at the runtime's next
//!   check, one waiting ends at once.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use super::{Call as Asked, Ended, Engine, LIMITS, Next, Program, Watch};
use crate::editor::Editor;
use crate::tools::{Call, Inner, Part};

#[cfg(test)]
mod tests;

/// What a program may call, each as `tool(name, input)` (the bindings' prelude, `PRELUDE`): the read-only tools of
/// those names, and `output`, the tool's own.
pub const BINDINGS: &[&str] =
    &["read", "search", "symbols", "definition", "references", "diagnostics", "git", "output"];

/// A program's call as the call record and the counts name it (`features::CLAUDE_TOOLS`): `code.read`, … — the name
/// the program gave is never kept as it came (one no binding has: `code.other`).
pub fn recorded(name: &str) -> &'static str {
    const NAMES: [&str; 8] = [
        "code.read",
        "code.search",
        "code.symbols",
        "code.definition",
        "code.references",
        "code.diagnostics",
        "code.git",
        "code.output",
    ];
    BINDINGS.iter().position(|b| *b == name).map_or("code.other", |i| NAMES[i])
}

/// The bindings, before the program on its first line (`Program::start_in`): globals a program's own names may
/// shadow, each `tool(name, input)`.
pub const PRELUDE: &str = "Object.assign(globalThis, Object.fromEntries([\"read\", \"search\", \"symbols\", \
     \"definition\", \"references\", \"diagnostics\", \"git\", \"output\"].map((n) => [n, (i) => tool(n, i)])));";

/// What Claude reads of the tool, every request (the plan's §6: under 900 tokens).
pub const DESCRIPTION: &str = "Run JavaScript in tarae to compute from text (count, group, filter, compare) instead \
of reading it all into the conversation: worth it when the answer is much smaller than the text. Its functions are \
tarae's read-only tools: same input, limits, cards and answer text as your own calls ({ error } if refused or \
failed). You get what the program prints and returns, cut past 20 KB (the user sees it whole).

Await at the top level; return a JSON value. Plain JavaScript: no type annotations, eval \
or import. Nothing outside but these calls: no files, network, clock or environment. Each program starts fresh. \
Calls awaited together (Promise.all) run side by side. A program makes 64 calls at most. Too much work stops at a limit, saying so: do \
less, or split it.

type Answer = Promise<string | { error: string }>;
declare function read(input: { files: string[] }): Answer;
/** A count line, then — lines: each file's path, its hits under it as \"  12: text\" (\"  in fn f (10–40)\" before them); files: a path a line; count: \"path:3\" a line. */
declare function search(input: { pattern: string; path?: string; glob?: string; literal?: boolean; \
output?: \"lines\" | \"files\" | \"count\"; limit?: number; offset?: number }): Answer;
declare function symbols(input: { path: string; line?: number }): Answer;
declare function definition(input: { symbol?: string; path?: string; line?: number; \
kind?: \"definition\" | \"implementation\" | \"type\" }): Answer;
declare function references(input: { path: string; symbol: string; line?: number }): Answer;
declare function diagnostics(input?: { paths?: string[] }): Answer;
declare function git(input: { args: string[]; offset?: number }): Answer;
/** A run's whole output, by the id its answer names (\"run#12\"): this conversation's last 8 runs. */
declare function output(id: string): Promise<{ command: string; ended: string; exit: number | null; ms: number; \
lines: string[] } | { error: string }>;
declare function print(...values: unknown[]): void;";

/// The tool's schema, after the rest in `tools::list_with`.
pub fn schema() -> Value {
    json!({
        "name": "code",
        "description": DESCRIPTION,
        "inputSchema": {
            "type": "object",
            "properties": { "src": { "type": "string", "description": "The program: JavaScript, the body of an async function" } },
            "required": ["src"]
        }
    })
}

/// Whether the chat of `ed.workspace` lists `code`: as its process started (`Chat.code`).
pub fn lists(ed: &Editor) -> bool {
    ed.workspace.claude.chat.as_ref().is_some_and(|c| c.code)
}

/// A program that threw: its row says so first (`chat::Look::failed`, its mark `▲`): `failed · TypeError: …`.
pub const FAILED: &str = "failed · ";

/// A program a limit or the user's stop ended (`Ended.stopped`): its row says so first, the stop's kind after it —
/// `stopped · at its time limit (10.0s of running): …`, `stopped · the user stopped it (C-c) …` (`row_end`).
pub const STOPPED: &str = "stopped · ";

/// While a program runs, its last row says so — where its output comes once it ends (`term::program_rows`), in the
/// chat and on the follow card alike, with no key: how to stop it is the chat's foot's (`C-c stop program`, drawn only
/// while the chat has the keys — `chat::key`'s `C-c`: the program first, then the turn); the follow card's head has the
/// timer (the design session, 2026-11-12).
pub const RUNNING: &str = "running";

/// A program's row's end once it ended in an error (`why`; `stopped`: a limit or a stop, `Ended.stopped`): `stopped · `
/// and the stop's words after their own `stopped` (`at its time limit …`, `the user stopped it …`), or `failed · ` and
/// the error's first line.
pub fn row_end(why: &str, stopped: bool) -> String {
    let first = why.lines().next().unwrap_or_default();
    match stopped {
        true => {
            let rest =
                first.strip_prefix("stopped: ").or_else(|| first.strip_prefix("stopped ")).unwrap_or(first);
            format!("{STOPPED}{rest}")
        }
        false => format!("{FAILED}{first}"),
    }
}

/// What a program stopped by the user's `C-c` tells Claude (after `stopped: `).
pub const STOPPED_BY_YOU: &str = "the user stopped it (C-c) — go on without it, or ask them";

/// What the chat shows of a program under its row (`chat::Msg.program`): its calls as handed out, each a row with its
/// answer's verdict, and once it ends its whole output — what it printed and returned, never cut (Claude's answer is cut
/// past `LIMITS.output`; the row's card and the buffer it opens show all of it: the description's "the user sees it
/// whole").
#[derive(Clone, Debug, Default)]
pub struct Shown {
    pub calls: Vec<Row>,
    /// Its whole output, once it ended.
    pub output: Option<String>,
    /// A restored conversation's (`chats::msgs_of`): its calls and output weren't saved — its row's card says so
    /// (`NOT_KEPT`, `term::restored_card`), nothing under it. Saving them waits for S2.
    pub restored: bool,
}

/// What a restored conversation's program row says on its card (`Shown.restored`).
pub const NOT_KEPT: &str = "Its calls and output weren't kept with the conversation.";

/// One call of a program, as its row under the program's says it.
#[derive(Clone, Debug)]
pub struct Row {
    /// Its id in the program (`codetool::Call.id`).
    pub id: u32,
    pub look: crate::chat::Look,
    /// Refused or failed: its words are its range, its mark `▲`.
    pub failed: bool,
}

/// A program of the chat's under way.
pub struct Running {
    /// Its number: its calls' (`tools::Inner.program`).
    pub key: u32,
    /// The chat's `code` call, answered as it ends.
    call: Option<Call>,
    /// Its row in the chat (the `code` call's tool_use id), when one was found (`attach`).
    row: Option<String>,
    /// Its latest batch's number, its calls not judged yet (answered, or asking on a card) and the reads held for its
    /// card (`hold`); rung as the batch is judged or the program ends, so the bound's wait ends with it.
    batch: u32,
    unjudged: Vec<u32>,
    held: Vec<crate::reads::Pending>,
    judged: Arc<crate::event::Wake>,
    pub watch: Arc<Watch>,
    /// The program while it waits on its calls (None: a step runs).
    program: Option<Box<Program>>,
    /// Its calls not given back yet, in the order handed out — the order they're given back in.
    order: VecDeque<u32>,
    /// Answers come and not given back yet (one before them waits).
    answers: BTreeMap<u32, Value>,
    /// Every call it made, as handed out (piece c's rows).
    pub calls: Vec<Asked>,
}

impl Running {
    /// Its calls not answered yet, in the order handed out. (Tests till piece c's rows, as the two below.)
    #[cfg(test)]
    pub fn waiting(&self) -> impl Iterator<Item = u32> + '_ {
        self.order.iter().copied().filter(|id| !self.answers.contains_key(id))
    }

    /// Its reads held for its batch's card (`hold`), on no card yet; its batch's calls not judged yet.
    #[cfg(test)]
    pub fn holding(&self) -> (usize, usize) {
        (self.held.len(), self.unjudged.len())
    }

    /// Its calls answered and not given back yet (one before them waits).
    #[cfg(test)]
    pub fn held(&self) -> impl Iterator<Item = u32> + '_ {
        self.answers.keys().copied()
    }

    /// It waits on its calls, no step running.
    #[cfg(test)]
    pub fn between_steps(&self) -> bool {
        self.program.is_some()
    }

    /// (tests that draw: a program under way — no call, no engine, nothing it waits on)
    #[cfg(test)]
    pub fn drawn_only() -> Running {
        let watch = Watch::new(&LIMITS);
        Running {
            key: NEXT.fetch_add(1, Ordering::Relaxed),
            call: None,
            row: None,
            batch: 0,
            unjudged: Vec::new(),
            held: Vec::new(),
            judged: Arc::default(),
            watch,
            program: None,
            order: VecDeque::new(),
            answers: BTreeMap::new(),
            calls: Vec::new(),
        }
    }
}

/// The next program's number (unique since tarae started).
static NEXT: AtomicU32 = AtomicU32::new(1);

#[cfg(test)]
thread_local! {
    /// (tests: the engine a program started on this thread runs on — the stand-in, the real one)
    pub static ENGINE: std::cell::RefCell<Option<Arc<Engine>>> = const { std::cell::RefCell::new(None) };
    /// (tests: no prelude — the stand-in's source is its script of replies)
    pub static BARE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The bindings before the program (`PRELUDE`; tests on the stand-in: none).
fn prelude() -> &'static str {
    #[cfg(test)]
    if BARE.with(|b| b.get()) {
        return "";
    }
    PRELUDE
}

/// The engine, loaded on a worker (`Engine::here`).
fn load() -> impl FnOnce() -> Result<Arc<Engine>, String> + Send + 'static {
    #[cfg(test)]
    let given = ENGINE.with(|e| e.borrow().clone());
    #[cfg(not(test))]
    let given: Option<Arc<Engine>> = None;
    move || given.map_or_else(Engine::here, Ok)
}

/// The tool: the program started on a worker; answered as it ends.
pub fn call(ed: &mut Editor, call: Call, args: &Value) -> Result<(), String> {
    let src = (args["src"].as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or("src is required: the program, in JavaScript")?
        .to_string();
    let key = NEXT.fetch_add(1, Ordering::Relaxed);
    let watch = Watch::new(&LIMITS);
    let row = attach(ed, &src);
    ed.workspace.claude.programs.push(Running {
        key,
        call: Some(call),
        row,
        batch: 0,
        unjudged: Vec::new(),
        held: Vec::new(),
        judged: Arc::default(),
        watch: watch.clone(),
        program: None,
        order: VecDeque::new(),
        answers: BTreeMap::new(),
        calls: Vec::new(),
    });
    let jobs = ed.events.jobs();
    super::start_job(&jobs, load(), prelude(), src, LIMITS, watch, move |ed, next| stepped(ed, key, next));
    Ok(())
}

/// The chat row of the `code` call whose program is `src`: the newest waiting call of it with no program yet — its
/// program's rows start under it, its range saying how to stop it.
fn attach(ed: &mut Editor, src: &str) -> Option<String> {
    let c = ed.workspace.claude.chat.as_mut()?;
    let ids: Vec<&str> = (c.calls.iter())
        .filter(|k| k.name == "mcp__tarae__code" && k.input["src"].as_str() == Some(src))
        .map(|k| k.id.as_str())
        .collect();
    let m = c.msgs.iter_mut().rev().find(|m| m.program.is_none() && ids.contains(&m.call.as_str()))?;
    m.program = Some(Box::default());
    m.changed();
    Some(m.call.clone())
}

/// What program `key`'s row shows, changed by `f` (no row: nothing).
fn show(ed: &mut Editor, key: u32, f: impl FnOnce(&mut crate::chat::Look, &mut Shown)) {
    let Some(row) = find(ed, key).and_then(|r| r.row.clone()) else { return };
    show_at(ed, &row, f);
}

fn show_at(ed: &mut Editor, row: &str, f: impl FnOnce(&mut crate::chat::Look, &mut Shown)) {
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    let Some(m) = c.msgs.iter_mut().rev().find(|m| m.call == row) else { return };
    let (Some(look), Some(shown)) = (m.look.as_mut(), m.program.as_mut()) else { return };
    f(look, shown);
    m.changed();
}

/// A program's call as its row says it: the tool's own row (`chat::look`), `output`'s as a read of the run's output.
fn inner_look(root: &std::path::Path, name: &str, arg: &Value) -> crate::chat::Look {
    let look = |verb, what: String| crate::chat::Look {
        verb,
        what,
        pattern: false,
        range: String::new(),
        link: None,
    };
    match name {
        "output" => look("read", format!("{} output", arg.as_str().unwrap_or("a run's"))),
        _ if BINDINGS.contains(&name) => crate::chat::look(root, &format!("mcp__tarae__{name}"), arg),
        other => look("use", other.to_string()),
    }
}

/// A call's answer as its row ends: a lookup's verdict (`12 references in 3 files`), a run's output's lines — or, refused
/// or failed, its words.
fn verdict(v: &Value) -> (String, bool) {
    match v {
        Value::String(text) => (crate::nav::verdict(text).unwrap_or_default(), false),
        Value::Object(o) if o.contains_key("error") => {
            (o["error"].as_str().unwrap_or_default().to_string(), true)
        }
        Value::Object(o) => {
            let n = o.get("lines").and_then(Value::as_array).map_or(0, Vec::len);
            (crate::nav::count(n, "line"), false)
        }
        _ => (String::new(), false),
    }
}

fn find(ed: &mut Editor, key: u32) -> Option<&mut Running> {
    ed.workspace.claude.programs.iter_mut().find(|r| r.key == key)
}

/// A step landed: the end answered, or its new calls started together and what's come given back.
fn stepped(ed: &mut Editor, key: u32, next: Next) {
    let programs = &mut ed.workspace.claude.programs;
    // (gone: the turn ended and it was answered already)
    let Some(at) = programs.iter().position(|r| r.key == key) else { return };
    let (program, calls) = match next {
        Next::Ended(end) => {
            let r = programs.remove(at);
            return finish(ed, r, end);
        }
        Next::Calls(program, calls) => (program, calls),
    };
    let r = &mut programs[at];
    // (stopped as its step ended: its end is said now)
    if let Some(why) = r.watch.stopped() {
        let r = programs.remove(at);
        return finish(ed, r, Ended::stop(super::stopped_words(&why, &LIMITS)));
    }
    r.program = Some(program);
    r.order.extend(calls.iter().map(|c| c.id));
    r.calls.extend(calls.iter().cloned());
    // (its reads outside the project come on one card once the batch is judged, or at the bound — `hold`)
    r.batch += 1;
    r.unjudged = calls.iter().map(|c| c.id).collect();
    let (batch, judged) = (r.batch, r.judged.clone());
    let seen = judged.rings();
    // (a batch's searches share the walkers one search has: eight together walk on `THREADS` threads, not 32)
    let searches = calls.iter().filter(|c| c.name == "search").count().max(1);
    let share = (crate::grep::THREADS / searches).max(1) as u8;
    for c in calls {
        dispatch(ed, key, c, share);
    }
    if find(ed, key).is_some_and(|r| r.batch == batch && !r.unjudged.is_empty()) {
        ed.events.jobs().spawn(move || {
            judged.sleep(JUDGED_WITHIN, seen);
            move |ed: &mut Editor| judged_out(ed, key, batch)
        });
    }
    pump(ed, key);
}

/// How long a batch's reads outside the project wait for the rest of its verdicts before their card comes (the design
/// session's bound): the gate judges a batch in milliseconds; one judged later gets a card of its own.
pub const JUDGED_WITHIN: Duration = Duration::from_millis(300);

/// A read of program call `inner` that asks on a card (`reads::tool_ask`): held while its batch is judged, so the card
/// comes once, complete, and never grows after it's shown — a place joining a moment before `y` would be granted unread.
/// Given back (Some) to wait for a card of its own when it's judged after its batch's card came (or the bound), or when
/// its card warns (`Pending::warned`: never in a batch — judged now, its card its own); None: held, or its program is
/// gone (nothing waits for it).
pub fn hold(ed: &mut Editor, inner: Inner, p: crate::reads::Pending) -> Option<crate::reads::Pending> {
    let (key, id) = (inner.program, u32::from(inner.id));
    let r = find(ed, key)?;
    if !r.unjudged.contains(&id) {
        return Some(p);
    }
    if p.warned() {
        judged_one(ed, key, id);
        return Some(p);
    }
    r.held.push(p);
    judged_one(ed, key, id);
    None
}

/// Call `id` of program `key` judged: answered, or held for the card. The batch's last: its card.
fn judged_one(ed: &mut Editor, key: u32, id: u32) {
    let Some(r) = find(ed, key) else { return };
    r.unjudged.retain(|&u| u != id);
    if r.unjudged.is_empty() {
        release(ed, key);
    }
}

/// The bound came for batch `batch` of program `key`: its card comes with what's held; a call judged later asks alone.
fn judged_out(ed: &mut Editor, key: u32, batch: u32) {
    match find(ed, key).filter(|r| r.batch == batch && !r.unjudged.is_empty()) {
        Some(r) => {
            r.unjudged.clear();
            release(ed, key);
        }
        None => ed.unchanged = true,
    }
}

/// Program `key`'s held reads on their card: one alone, today's card; more, one card for them all (`Pending.more`).
fn release(ed: &mut Editor, key: u32) {
    let Some(r) = find(ed, key) else { return };
    r.judged.ring();
    // (in the order the program asked, whichever the gate judged first)
    crate::sorting::by_key(&mut r.held, |p| p.then.as_ref().and_then(|t| t.call.program()).map(|i| i.id));
    let mut held = std::mem::take(&mut r.held).into_iter();
    let Some(mut first) = held.next() else { return };
    first.more = held.collect();
    crate::reads::queue(ed, first);
}

/// Call `c` of program `key` starts: the tool of its name, through its own gate, its answer to `answered`.
fn dispatch(ed: &mut Editor, key: u32, c: Asked, share: u8) {
    let Some(outer) = find(ed, key).and_then(|r| r.call.clone()) else { return };
    let name = recorded(&c.name);
    let look = inner_look(&ed.workspace.root, &c.name, &c.arg);
    show(ed, key, |_, s| s.calls.push(Row { id: c.id, look, failed: false }));
    ed.workspace.claude.usage.tally.inner(name);
    ed.features.bump(crate::features::Area::Claude, name);
    // (ids are below `CALLS`, 64: the host holds the engine to it)
    let inner = outer.inner(key, c.id as u8, &c.name, share);
    let input = match &c.arg {
        Value::Null => json!({}),
        v => v.clone(),
    };
    let object = |ed: &mut Editor, f: fn(&mut Editor, Call, &Value) -> Result<(), String>| match input
        .is_object()
    {
        true => f(ed, inner.clone(), &input),
        false => Err(format!("{}(input): its input is an object, as the {} tool takes it", c.name, c.name)),
    };
    let asked = match c.name.as_str() {
        "output" => {
            let v = output(ed, outer.generation(), &c.arg);
            return given(ed, inner_of(&inner), v);
        }
        "read" if pages(&input) => Err("a program reads files, not pages: read a page yourself".to_string()),
        "read" => object(ed, crate::readtool::call),
        "search" => object(ed, crate::nav::search),
        "symbols" => object(ed, crate::nav::symbols),
        "definition" => object(ed, crate::nav::definition),
        "references" => object(ed, crate::nav::references),
        "diagnostics" => object(ed, crate::nav::diagnostics),
        "git" => object(ed, crate::gittool::call),
        other => Err(format!("no function {other} — a program has {}", BINDINGS.join(", "))),
    };
    if let Err(e) = asked {
        given(ed, inner_of(&inner), json!({ "error": e }));
    }
}

fn inner_of(call: &Call) -> Inner {
    call.program().expect("a program's call")
}

/// Whether a read asks for a page (`web::tool::is_url`): those go to the network on a card of their own.
fn pages(input: &Value) -> bool {
    let files = input["files"].as_array().into_iter().flatten().filter_map(Value::as_str);
    files.into_iter().any(crate::web::tool::is_url)
}

/// A program's call answered by its tool (`tools::answer_parts`): its text, or `{ error }`.
pub fn answered(ed: &mut Editor, inner: Inner, r: Result<Vec<Part>, String>) {
    let v = match r {
        Ok(parts) => {
            let text: Vec<String> = (parts.into_iter())
                .map(|p| match p {
                    Part::Text(t) => t,
                    Part::Image(i) => {
                        format!("[{} {}×{}: a picture, not text for a program]", i.mime, i.w, i.h)
                    }
                })
                .collect();
            Value::String(text.join("\n"))
        }
        Err(e) => json!({ "error": e }),
    };
    given(ed, inner, v);
}

/// Call `inner.id`'s answer kept, and what can go back given back.
fn given(ed: &mut Editor, inner: Inner, v: Value) {
    // (its program ended or was stopped: nothing waits for it)
    let (key, id) = (inner.program, u32::from(inner.id));
    let Some(r) = find(ed, key) else { return };
    if !r.order.contains(&id) {
        return;
    }
    let (range, failed) = verdict(&v);
    r.answers.insert(id, v);
    judged_one(ed, key, id);
    show(ed, key, |_, s| {
        if let Some(row) = s.calls.iter_mut().find(|row| row.id == id) {
            (row.look.range, row.failed) = (range, failed);
        }
    });
    pump(ed, key);
}

/// The next answer in the order handed out goes back, if it's come and no step runs: one `resume`.
fn pump(ed: &mut Editor, key: u32) {
    let Some(r) = find(ed, key) else { return };
    let Some(&id) = r.order.front().filter(|id| r.program.is_some() && r.answers.contains_key(id)) else {
        return;
    };
    r.order.pop_front();
    let value = r.answers.remove(&id).expect("its answer came");
    let program = r.program.take().expect("it waits");
    super::answer_job(&ed.events.jobs(), program, id, value, move |ed, next| stepped(ed, key, next));
}

/// The chat's call answered with how the program ended: what it printed and returned, cut at the tool's limit (an
/// error, if it ended in one).
fn finish(ed: &mut Editor, mut r: Running, end: Ended) {
    // (its row: how it ended, and its whole output — the user's, never cut)
    if let Some(row) = &r.row {
        let whole = end.shown();
        // (its row keeps the program's length; a stop or a failure says so first, `▲` its mark)
        let failed = end.result.as_ref().err().map(|e| row_end(e, end.stopped));
        show_at(ed, row, |look, s| {
            if let Some(f) = failed {
                look.range = f;
            }
            s.output = Some(whole);
        });
    }
    // (a card it asked on goes with it: nothing waits for its answer — nor the bound's wait)
    r.judged.ring();
    crate::reads::program_ended(ed, r.key);
    let Some(call) = r.call.take() else { return };
    let text = end.answer(LIMITS.output).text;
    let r = match end.result {
        Ok(_) => Ok(text),
        Err(_) => Err(text),
    };
    crate::tools::answer(ed, call, r);
}

/// The turn ended: every program stops (`Watch::stop`) — one running at its next check, one waiting now.
pub fn turn_over(ed: &mut Editor) {
    stop_all(ed, "the turn ended");
}

/// Every program of the session stops, saying `why`.
pub fn stop_all(ed: &mut Editor, why: &str) {
    let programs = &mut ed.workspace.claude.programs;
    programs.iter().for_each(|r| r.watch.stop(why));
    let (waiting, stepping): (Vec<Running>, Vec<Running>) =
        std::mem::take(programs).into_iter().partition(|r| r.program.is_some());
    *programs = stepping;
    for r in waiting {
        let words = super::stopped_words(&super::Why::Host(why.into()), &LIMITS);
        finish(ed, r, Ended::stop(words));
    }
}

// —— runs kept for `output` ——

/// The runs whose whole output a session keeps for its programs: its conversation's last ones.
pub const KEPT: usize = 8;

/// A run of Claude's, its output as tarae kept it (`task::Run.output`, `testing::TestRun.output` — the rope's nodes
/// shared while the run is still in its panel).
pub struct Kept {
    /// `run#<id>` (its trace's).
    pub id: u64,
    /// The chat process whose run it was.
    generation: u64,
    command: String,
    /// How it ended, in the run's answer's words (`exit 0`, `killed`, `stopped: …`), and its exit code.
    ended: String,
    exit: Option<i32>,
    took: Option<Duration>,
    output: crate::runout::Output,
}

impl Kept {
    /// Claude's `run` that ended.
    pub fn of_task(r: &crate::task::Run, generation: u64) -> Kept {
        use crate::task::State;
        let (ended, exit) = match &r.state {
            State::Passed => ("exit 0".to_string(), Some(0)),
            State::Failed(Some(c)) => (format!("exit {c}"), Some(*c)),
            State::Failed(None) => ("killed".to_string(), None),
            State::Stopped(why) => (format!("stopped: {why}"), None),
            State::NotRun(why) => (format!("not run: {why}"), None),
            State::Running => ("still running".to_string(), None),
        };
        let (id, command, took, output) = (r.trace.id(), r.task.cmd.clone(), r.took, r.output.clone());
        Kept { id, generation, command, ended, exit, took, output }
    }

    /// Claude's `run_tests` that ended.
    pub fn of_tests(r: &crate::testing::TestRun, generation: u64) -> Kept {
        use crate::testing::RunState;
        let (ended, exit) = match &r.state {
            RunState::Passed => ("passed".to_string(), Some(0)),
            RunState::Failed(Some(c)) => (format!("failed, exit {c}"), Some(*c)),
            RunState::Failed(None) => ("failed, killed".to_string(), None),
            RunState::NoTests => ("no tests ran".to_string(), Some(0)),
            RunState::Running => ("still running".to_string(), None),
        };
        let command = format!("tests: {}", r.target.label);
        Kept { id: r.trace.id(), generation, command, ended, exit, took: r.took, output: r.output.clone() }
    }
}

/// `k` kept, while the chat lists `code`: the last `KEPT` of its conversation (another conversation's go).
pub fn keep(ed: &mut Editor, k: Kept) {
    if !lists(ed) {
        return;
    }
    keep_in(&mut ed.workspace.claude.runs_kept, k);
}

/// `k` the newest of `kept`: another conversation's go, and past `KEPT` the oldest.
fn keep_in(kept: &mut VecDeque<Kept>, k: Kept) {
    kept.retain(|o| o.generation == k.generation && o.id != k.id);
    kept.push_back(k);
    while kept.len() > KEPT {
        kept.pop_front();
    }
}

/// A run's answer, its id named on its first line while the chat lists `code` (`listed`, `lists`) —
/// `output("run#12")` reads it whole; off, the answer to the byte.
pub fn named(listed: bool, id: u64, text: String) -> String {
    if !listed {
        return text;
    }
    match text.split_once('\n') {
        Some((first, rest)) => format!("{first} · run#{id}\n{rest}"),
        None => format!("{text} · run#{id}"),
    }
}

/// `output(id)`: the run's whole output as kept, or `{ error }` saying why not.
fn output(ed: &Editor, generation: u64, id: &Value) -> Value {
    let n = match id {
        Value::String(s) => s.trim().strip_prefix("run#").unwrap_or(s.trim()).parse::<u64>().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    };
    let Some(n) = n else {
        return json!({ "error": "output(id): a run's id as its answer names it, \"run#12\"" });
    };
    let kept = ed.workspace.claude.runs_kept.iter().find(|k| k.id == n && k.generation == generation);
    let Some(k) = kept else {
        return json!({ "error": format!(
            "run#{n} isn't kept: a program reads the whole output of this conversation's last {KEPT} runs, until \
             tarae quits — run it again"
        ) });
    };
    let lines: Vec<String> = k.output.iter().map(|(_, l)| l.into_owned()).collect();
    json!({
        "command": k.command,
        "ended": k.ended,
        "exit": k.exit,
        "ms": k.took.map_or(0, |t| t.as_millis() as u64),
        "lines": lines,
    })
}
