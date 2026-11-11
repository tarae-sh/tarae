//! The code tool's engine host (`.sb/codemode-compute-plan.md`, S1's piece a): a program of Claude's run on the
//! engine — QuickJS-ng as a WebAssembly module with no imports, `xtask/quickjs/` — on tarae-wasm, by the engine's
//! two exports.
//!
//! - **The engine is data**: a module that answers `start(src, len, steps)` and `resume(id, answer, len, err)` with
//!   a reply (`xtask/quickjs/glue.c` says its form). Nothing here knows JavaScript but the type-annotation hint
//!   (`annotation`), which reads a program that didn't compile.
//! - **A fresh instance a program**: the engine's `Module` is compiled once and shared (`Arc`); each program gets
//!   its own memory, and is dropped whole when it ends or traps — nothing of one is left for the next.
//! - **Calls**: what the program awaits comes back together (`Next::Calls`, a `Promise.all`'s every call); the
//!   host answers them one by one (`Program::answer`), each answer a `resume`, which may hand out more.
//! - **Limits** (`LIMITS`): steps (work, counted by QuickJS's interrupt handler — the same on every machine and
//!   under the JIT or the interpreter), 64 calls, 64 MB of memory, fuel as the backstop for work inside the
//!   engine's own C that the step count doesn't see, and 20 KB of what Claude reads, the cut said (`Ended::answer`).
//!   Waiting for an answer costs nothing: no clock is read.
//! - **Never on the key or frame path**: loading and every `start` and `resume` run on a worker (`start_job`,
//!   `answer_job`); the `Program` travels between them and the main loop.

#![allow(dead_code, reason = "S1 a: the engine's host; the tool that calls it is piece b")]

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tarae_wasm::{Instance, Module, Trap, Val};

use crate::editor::Editor;
use crate::event::Jobs;

#[cfg(test)]
mod tests;

/// Where a dev build and the tests find the engine `cargo xtask quickjs` built. How it ships is S3's decision; till
/// then a release has none.
pub const ENGINE_DEV: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/quickjs/quickjs.wasm");

/// What one program may use.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Interrupt-handler calls, each 10,000 of a program's loop turns and branches (or a stretch of a regex's
    /// matching).
    pub steps: u32,
    /// The engine's linear memory (the engine's own heap limit, 48 MB, comes first: an `out of memory` the program
    /// can see).
    pub memory: usize,
    /// The runtime's fuel for the whole program, every `resume` included.
    pub fuel: u64,
    /// The bytes of the answer Claude reads.
    pub output: usize,
    /// Machine code where the runtime has it, else its interpreter.
    pub jit: bool,
}

/// Calls one program may make — the engine's own limit (`glue.c`'s `MAX_CALLS`), which refuses the 65th with a
/// `RangeError` the program sees; the host holds the engine to it too.
pub const CALLS: u32 = 64;

/// The bytes of what a program printed the engine keeps (`glue.c`'s `PRINTED_MAX`); past it, only counted.
const PRINTED_MAX: usize = 1 << 20;

/// The fuel a step of bytecode costs, for a loop of up to 20 statements a turn (`tests::measure`, QuickJS-ng
/// v0.18.0: 1.6×10⁸ an empty loop's, 2.5×10⁹ arithmetic's, 3.4×10¹⁰ twenty statements', 1.7×10¹¹ a hundred's —
/// it grows with the straight-line code between two jumps, which a program chooses).
pub const FUEL_A_STEP: u64 = 34_000_000_000;

/// Limits of `steps`, the fuel above them: twice what that many steps of bytecode cost (`FUEL_A_STEP`), so a
/// JavaScript loop meets the step limit first.
pub const fn limits(steps: u32) -> Limits {
    Limits {
        steps,
        memory: 64 << 20,
        fuel: 2 * steps as u64 * FUEL_A_STEP,
        output: 20 << 10,
        jit: cfg!(target_arch = "aarch64"),
    }
}

/// The limits every program runs under, for the design session (`.sb/codemode-s1a-report.md` §2).
/// - `steps`: ten times the 20,000-line triage's 18 (QuickJS-ng v0.18.0) — 1.8 million loop turns or branches.
/// - `fuel`: above the steps (`limits`), 1.2×10¹³. Where the steps see nothing (`JSON.parse`, string building,
///   `indexOf` over megabytes) that is about 710 s at worst under the JIT and 3.8 h under the interpreter: **over the
///   line** (10 s, 60 s); the last net is the host-set flag (the user's yes, 2026-11-11), built after the re-pin.
/// - `jit`: machine code only on an arch whose JIT has been fuzzed on QuickJS against the interpreter (S1's call 5:
///   until then the interpreter — x86-64 today).
pub const LIMITS: Limits = limits(180);

/// The engine: its module compiled once, the exports the host calls.
pub struct Engine {
    module: Arc<Module>,
    fns: Fns,
    init: Option<u32>,
}

#[derive(Clone, Copy)]
struct Fns {
    alloc: u32,
    start: u32,
    resume: u32,
}

impl Engine {
    /// Decodes, validates and compiles `bytes` (~21 ms for QuickJS) — a worker's work.
    pub fn new(bytes: &[u8]) -> Result<Engine, String> {
        let module = Module::new(bytes).map_err(|e| format!("the engine doesn't load: {e}"))?;
        let f = |n: &str| module.func(n).ok_or_else(|| format!("the engine has no export `{n}`"));
        let fns = Fns { alloc: f("alloc")?, start: f("start")?, resume: f("resume")? };
        let init = module.func("_initialize");
        Ok(Engine { module: Arc::new(module), fns, init })
    }

    /// The engine at `path`, loaded once and kept while it loads; a worker's (it reads the disk and compiles).
    pub fn at(path: &Path) -> Result<Arc<Engine>, String> {
        static LOADED: Mutex<Option<Arc<Engine>>> = Mutex::new(None);
        let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = loaded.as_ref() {
            return Ok(e.clone());
        }
        let bytes = std::fs::read(path).map_err(|e| format!("no engine at {}: {e}", path.display()))?;
        let e = Arc::new(Engine::new(&bytes)?);
        *loaded = Some(e.clone());
        Ok(e)
    }

    /// This build's engine: a dev build's from `ENGINE_DEV`; a release has none yet (S3).
    pub fn here() -> Result<Arc<Engine>, String> {
        match cfg!(debug_assertions) {
            true => Engine::at(Path::new(ENGINE_DEV)),
            false => Err("this build has no code engine".into()),
        }
    }
}

/// A call the program waits on.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub id: u32,
    pub name: String,
    pub arg: Value,
}

/// How a program ended.
#[derive(Clone, Debug, PartialEq)]
pub struct Ended {
    /// What it returned (None: nothing), or why it ended without.
    pub result: Result<Option<Value>, String>,
    /// What it printed, its first `PRINTED_MAX` bytes.
    pub printed: String,
    /// All it asked to print.
    pub printed_bytes: u64,
    pub steps: u32,
    /// The runtime's fuel it burnt.
    pub fuel: u64,
}

/// What Claude reads of an ended program: `text`, `shown` bytes of the `whole`'s.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub text: String,
    pub shown: usize,
    pub whole: usize,
}

impl Ended {
    fn failed(why: impl Into<String>) -> Ended {
        Ended { result: Err(why.into()), printed: String::new(), printed_bytes: 0, steps: 0, fuel: 0 }
    }

    /// The whole of it, as the user sees it: what it printed, then what it returned or why it ended.
    pub fn text(&self) -> String {
        let mut out = self.printed.clone();
        if self.printed_bytes > self.printed.len() as u64 {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!(
                "… print() kept its first {} KB, of {} bytes printed\n",
                PRINTED_MAX >> 10,
                self.printed_bytes
            ));
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        match &self.result {
            Ok(Some(Value::String(s))) => out.push_str(&format!("returned:\n{s}")),
            Ok(Some(v)) => out.push_str(&format!("returned: {v}")),
            Ok(None) if out.is_empty() => out.push_str("returned nothing, printed nothing"),
            Ok(None) => {}
            Err(e) => out.push_str(&format!("error: {e}")),
        }
        out
    }

    /// `text` cut to `max` bytes at a line's end where one is near (else a character's), the cut said.
    pub fn answer(&self, max: usize) -> Answer {
        let whole = self.text();
        if whole.len() <= max {
            return Answer { shown: whole.len(), whole: whole.len(), text: whole };
        }
        let mut at = max;
        while !whole.is_char_boundary(at) {
            at -= 1;
        }
        if let Some(nl) = whole[..at].rfind('\n').filter(|&nl| nl >= max * 3 / 4) {
            at = nl + 1;
        }
        let text = format!(
            "{}\n… cut: the first {} of {} bytes — print less, or what you need from it (the user sees it whole)",
            whole[..at].trim_end_matches('\n'),
            at,
            whole.len()
        );
        Answer { text, shown: at, whole: whole.len() }
    }
}

/// What comes after a `start` or an answer.
pub enum Next {
    /// The program waits: on these calls, new since the last step (none: on calls already handed out).
    Calls(Box<Program>, Vec<Call>),
    Ended(Ended),
}

/// A program waiting on its calls: its own instance of the engine.
pub struct Program {
    inst: Instance,
    fns: Fns,
    /// Calls handed out and not answered yet.
    out: BTreeSet<u32>,
    /// Calls asked in all.
    asked: u32,
    /// The fuel it started with.
    fuel: u64,
    /// Its step limit, for the words when it's met.
    steps: u32,
}

/// Why a step stopped short of a reply.
enum Stop {
    Trap(Trap),
    /// The engine broke its side of the protocol.
    Engine(String),
}

impl From<Trap> for Stop {
    fn from(t: Trap) -> Stop {
        Stop::Trap(t)
    }
}

impl Program {
    /// Runs `src` on a fresh instance of `engine` until it waits or ends — a worker's.
    pub fn start(engine: &Engine, src: &str, limits: Limits) -> Next {
        let wasm = tarae_wasm::Limits { memory_bytes: limits.memory, jit: limits.jit, ..Default::default() };
        let inst = match Instance::new(engine.module.clone(), wasm, limits.fuel) {
            Ok(i) => i,
            Err(t) => return Next::Ended(Ended::failed(stopped(Stop::Trap(t)))),
        };
        let mut p = Program {
            inst,
            fns: engine.fns,
            out: BTreeSet::new(),
            asked: 0,
            fuel: limits.fuel,
            steps: limits.steps,
        };
        let reply = (|| {
            if let Some(init) = engine.init {
                p.inst.call(init, &[])?;
            }
            let (at, len) = p.put(src.as_bytes())?;
            p.call(p.fns.start, &[at, len, Val::I32(limits.steps as i32)])
        })();
        p.next(reply, Some(src))
    }

    /// Answers call `id` with `value` (a refused or failed call is the binding's `{ error }`, a value too) and runs
    /// the program on — a worker's.
    pub fn answer(mut self, id: u32, value: &Value) -> Next {
        if !self.out.remove(&id) {
            return Next::Ended(Ended::failed(format!("the host answered call {id}, which isn't waiting")));
        }
        let text = value.to_string();
        let reply = (|| {
            let (at, len) = self.put(text.as_bytes())?;
            self.call(self.fns.resume, &[Val::I32(id as i32), at, len, Val::I32(0)])
        })();
        self.next(reply, None)
    }

    /// The calls handed out and not answered yet.
    pub fn waiting(&self) -> impl Iterator<Item = u32> + '_ {
        self.out.iter().copied()
    }

    /// `text` into a buffer of the engine's: its address and length.
    fn put(&mut self, text: &[u8]) -> Result<(Val, Val), Stop> {
        let len = i32::try_from(text.len()).map_err(|_| Stop::Engine("text past 2 GB".into()))?;
        let at = self.call(self.fns.alloc, &[Val::I32(len)])?;
        let Val::I32(p) = at else { unreachable!("call gives an i32") };
        let dst = (p as u32 as usize)
            .checked_add(text.len())
            .and_then(|end| self.inst.memory_mut().get_mut(p as u32 as usize..end))
            .ok_or_else(|| Stop::Engine("alloc gave memory it hasn't got".into()))?;
        dst.copy_from_slice(text);
        Ok((at, Val::I32(len)))
    }

    /// Calls `f`, which gives one i32 (anything else is the engine's fault).
    fn call(&mut self, f: u32, args: &[Val]) -> Result<Val, Stop> {
        match self.inst.call(f, args)?[..] {
            [v @ Val::I32(_)] => Ok(v),
            _ => Err(Stop::Engine("an export gave something other than one i32".into())),
        }
    }

    /// The reply at `p`: its header line and its JSON.
    fn reply(&self, p: Val) -> Result<(String, Value), Stop> {
        let Val::I32(p) = p else { unreachable!("call gives an i32") };
        let m = self.inst.memory();
        let at = p as u32 as usize;
        let bad = || Stop::Engine("gave a reply outside its memory".into());
        let len = m.get(at..at.checked_add(4).ok_or_else(bad)?).ok_or_else(bad)?;
        let len = u32::from_le_bytes(len.try_into().unwrap()) as usize;
        let text = m.get(at + 4..(at + 4).checked_add(len).ok_or_else(bad)?).ok_or_else(bad)?;
        let text = String::from_utf8_lossy(text);
        let (head, body) = text.split_once('\n').unwrap_or((&text, ""));
        let body = serde_json::from_str(body)
            .map_err(|e| Stop::Engine(format!("gave a reply `{}` that isn't JSON: {e}", cut(head, 40))))?;
        Ok((head.to_string(), body))
    }

    /// What a step's reply says; `src` when it was the start's (a syntax error there gets the annotation hint).
    fn next(mut self, reply: Result<Val, Stop>, src: Option<&str>) -> Next {
        let read = reply.and_then(|p| self.reply(p));
        let fuel = self.fuel - self.inst.fuel();
        let (head, mut body) = match read {
            Ok(r) => r,
            Err(s) => return Next::Ended(Ended { fuel, ..Ended::failed(stopped(s)) }),
        };
        let word = head.split(' ').next().unwrap_or_default();
        match word {
            "pending" => match self.handed_out(&mut body) {
                Ok(calls) => Next::Calls(Box::new(self), calls),
                Err(why) => Next::Ended(Ended::failed(format!("the engine {why}"))),
            },
            "done" | "error" => {
                let steps = body["steps"].as_u64().unwrap_or(0) as u32;
                let result = match word {
                    // (no `done` key: it returned `undefined`, which JSON leaves out)
                    "done" => Ok(body.get_mut("done").map(Value::take)),
                    _ if body["stopped"] == "steps" => Err(step_limit(self.steps)),
                    _ => {
                        let e = body["error"].as_str().unwrap_or("an error it didn't say").to_string();
                        let hint = src.filter(|_| body["syntax"] == true).and_then(annotation);
                        Err(match hint {
                            Some((line, text)) => format!(
                                "SyntaxError: plain JavaScript: no type annotations (`: number`, `interface`, `as`) — line {line}: {}",
                                cut(text.trim(), 120)
                            ),
                            None => e,
                        })
                    }
                };
                Next::Ended(Ended {
                    result,
                    printed: body["printed"].as_str().unwrap_or_default().to_string(),
                    printed_bytes: body["printed_bytes"].as_u64().unwrap_or(0),
                    steps,
                    fuel,
                })
            }
            _ => Next::Ended(Ended::failed(format!(
                "the engine gave a reply `{}` that isn't one",
                cut(&head, 40)
            ))),
        }
    }

    /// The new calls of a `pending` reply, held to the protocol: each id new, below the limit.
    fn handed_out(&mut self, body: &mut Value) -> Result<Vec<Call>, String> {
        let Value::Array(list) = body["pending"].take() else {
            return Err("said pending with no list".into());
        };
        let mut calls = Vec::with_capacity(list.len());
        for mut c in list {
            let id =
                c["id"].as_u64().filter(|&id| id < CALLS as u64).ok_or("handed out a call with no id")?;
            let id = id as u32;
            let name = c["name"].as_str().ok_or("handed out a call with no name")?.to_string();
            self.asked += 1;
            if self.asked > CALLS || !self.out.insert(id) {
                return Err(format!("handed out call {id} again, or past {CALLS} calls"));
            }
            calls.push(Call { id, name, arg: c["arg"].take() });
        }
        if calls.is_empty() && self.out.is_empty() {
            return Err("waits with no call out".into());
        }
        Ok(calls)
    }
}

/// What every limit's words end with: the way out.
const REMEDY: &str = "do less in one program, or split it";

/// The words for a program the step limit ended: the limit itself, and the way out.
fn step_limit(limit: u32) -> String {
    format!("stopped at the limit of {limit} steps (a step is 10,000 loop turns and branches): {REMEDY}")
}

/// The words for a step that trapped or broke the protocol: the instance is dropped with it.
fn stopped(s: Stop) -> String {
    match s {
        Stop::Trap(Trap::OutOfFuel) => format!(
            "stopped at the limit of one program's fuel (its work, the steps' and what they don't count: strings, \
             JSON, sorting): {REMEDY}"
        ),
        Stop::Trap(t) => format!("the engine stopped: {t}"),
        Stop::Engine(why) => format!("the engine {why}"),
    }
}

fn cut(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// A TypeScript construct in a program that didn't compile: its line (from 1) and that line's text. Strings,
/// template literals and comments are blanked first; what's matched isn't JavaScript anywhere it could stand
/// (a declaration's `: type`, a parameter's, a return type, `interface`, `enum`, `type X =`, `x as T`).
pub fn annotation(src: &str) -> Option<(usize, &str)> {
    use std::sync::LazyLock;
    static TS: LazyLock<regex::Regex> = LazyLock::new(|| {
        let id = r"[A-Za-z_$][\w$]*";
        let prim = "number|string|boolean|any|unknown|void|never|object|bigint|symbol";
        regex::Regex::new(
            &[
                format!(r"\b(?:let|const|var)\s+{id}\s*:\s*[A-Za-z_$\[{{(]"),
                format!(r"\b(?:interface|enum)\s+{id}\s*(?:<[^>]*>)?\s*(?:extends\b[^{{]*)?\{{"),
                format!(r"\btype\s+{id}\s*(?:<[^>]*>)?\s*="),
                format!(r"[\w$)\]]\s+as\s+(?:const\b|{id})"),
                format!(r"\)\s*:\s*{id}[\w$<>\[\]|., ]*\s*(?:=>|\{{)"),
                format!(r"\(\s*{id}\??\s*:\s*[A-Za-z_$\[{{]"),
                format!(r",\s*{id}\??\s*:\s*(?:{prim})\b\s*[,)=\[|]"),
            ]
            .join("|"),
        )
        .expect("the pattern is valid")
    });
    let code = blank(src);
    let at = TS.find(&code)?.start();
    let line = code[..at].matches('\n').count();
    Some((line + 1, src.lines().nth(line).unwrap_or_default()))
}

/// `src` with what's in strings, template literals and comments as spaces (newlines kept): the code alone, each
/// byte where it was.
fn blank(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let end = match c {
            b'\'' | b'"' | b'`' => {
                let mut j = i + 1;
                while j < b.len() && b[j] != c && !(c != b'`' && b[j] == b'\n') {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                (j + 1).min(b.len())
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                b[i..].iter().position(|&x| x == b'\n').map_or(b.len(), |n| i + n)
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                b[i + 2..].windows(2).position(|w| w == b"*/").map_or(b.len(), |n| i + 2 + n + 2)
            }
            _ => {
                out.push(c);
                i += 1;
                continue;
            }
        };
        let end = end.min(b.len());
        // (the quotes stay: `"x" as T` still reads as a value before `as`)
        out.extend(b[i..end].iter().enumerate().map(|(k, &x)| match x {
            b'\n' => b'\n',
            _ if (k == 0 || k == end - i - 1) && matches!(c, b'\'' | b'"' | b'`') => c,
            _ => b' ',
        }));
        i = end;
    }
    // (every byte that isn't ASCII went to a space: still UTF-8)
    String::from_utf8(out).unwrap_or_default()
}

/// Starts `src` on a worker, on the engine `load` gives there (`Engine::here`); `then` takes what comes on the main
/// loop.
pub fn start_job(
    jobs: &Jobs,
    load: impl FnOnce() -> Result<Arc<Engine>, String> + Send + 'static,
    src: String,
    limits: Limits,
    then: impl FnOnce(&mut Editor, Next) + Send + 'static,
) {
    jobs.spawn(move || {
        let next = match load() {
            Ok(e) => Program::start(&e, &src, limits),
            Err(why) => Next::Ended(Ended::failed(why)),
        };
        move |ed: &mut Editor| then(ed, next)
    });
}

/// Answers call `id` of `program` on a worker; `then` takes what comes on the main loop.
pub fn answer_job(
    jobs: &Jobs,
    program: Box<Program>,
    id: u32,
    value: Value,
    then: impl FnOnce(&mut Editor, Next) + Send + 'static,
) {
    jobs.spawn(move || {
        let next = program.answer(id, &value);
        move |ed: &mut Editor| then(ed, next)
    });
}
