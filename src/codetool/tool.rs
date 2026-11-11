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
pub const DESCRIPTION: &str = "Run a JavaScript program in tarae to compute from text — count, group, filter, \
compare — instead of reading all of it into the conversation. Its functions are tarae's read-only tools: the same \
input as the tool of that name (the common fields below), the same gate and cards as your own calls, and the answer's \
text as you'd get it — or { error } when it's refused or fails. You get back what the program prints and returns, \
20 KB at most (the user sees it whole).

The program is the body of an async function: await at the top, return a JSON value. Plain JavaScript: no type \
annotations, no eval, no import. Nothing outside but these calls: no files, network, clock or environment. Calls \
awaited together (Promise.all) run side by side; 64 calls at most. A program that loops too long, builds huge strings \
or sorts too much stops at a limit and says so: do less in one program, or split it.

type Answer = string | { error: string };
declare function read(input: { files: string[] }): Promise<Answer>;
declare function search(input: { pattern: string; path?: string; glob?: string; literal?: boolean; \
output?: \"lines\" | \"files\" | \"count\"; limit?: number; offset?: number }): Promise<Answer>;
declare function symbols(input: { path: string; line?: number }): Promise<Answer>;
declare function definition(input: { symbol?: string; path?: string; line?: number; \
kind?: \"definition\" | \"implementation\" | \"type\" }): Promise<Answer>;
declare function references(input: { path: string; symbol: string; line?: number }): Promise<Answer>;
declare function diagnostics(input?: { paths?: string[] }): Promise<Answer>;
declare function git(input: { args: string[]; offset?: number }): Promise<Answer>;
/** A run's whole output as tarae kept it, by the id its answer names (\"run#12\") — this conversation's last 8 runs. */
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

/// A program of the chat's under way.
pub struct Running {
    /// Its number: its calls' (`tools::Inner.program`).
    pub key: u32,
    /// The chat's `code` call, answered as it ends.
    call: Option<Call>,
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
    ed.workspace.claude.programs.push(Running {
        key,
        call: Some(call),
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
        return finish(ed, r, Ended::failed(super::stopped_words(&why, &LIMITS)));
    }
    r.program = Some(program);
    r.order.extend(calls.iter().map(|c| c.id));
    r.calls.extend(calls.iter().cloned());
    // (a batch's searches share the walkers one search has: eight together walk on `THREADS` threads, not 32)
    let searches = calls.iter().filter(|c| c.name == "search").count().max(1);
    let share = (crate::grep::THREADS / searches).max(1) as u8;
    for c in calls {
        dispatch(ed, key, c, share);
    }
    pump(ed, key);
}

/// Call `c` of program `key` starts: the tool of its name, through its own gate, its answer to `answered`.
fn dispatch(ed: &mut Editor, key: u32, c: Asked, share: u8) {
    let Some(outer) = find(ed, key).and_then(|r| r.call.clone()) else { return };
    let name = recorded(&c.name);
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
    if r.order.contains(&id) {
        r.answers.insert(id, v);
    }
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
        finish(ed, r, Ended::failed(words));
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
