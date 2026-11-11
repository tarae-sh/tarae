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
//! - **The clock** (`Watch`), the last net behind the steps and the fuel: a program's running time, summed over its
//!   `start` and every `resume` — never the waits between, on a card or an answer — is held to `Limits::clock`
//!   (10 s under the JIT, 60 s under the interpreter) by a watchdog that sets the instance's interrupt
//!   (`tarae_wasm::Interrupt`, checked wherever the runtime charges fuel). The host can set it at any moment too
//!   (`Watch::stop`: the user's `esc`, the turn's end). Steps, fuel and the clock each say what stopped the
//!   program, and the one way out (`REMEDY`).
//! - **Never on the key or frame path**: loading and every `start` and `resume` run on a worker (`start_job`,
//!   `answer_job`); the `Program` travels between them and the main loop.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use serde_json::Value;
use tarae_wasm::{Instance, Interrupt, Module, Trap, Val};

use crate::editor::Editor;
use crate::event::Jobs;

#[cfg(test)]
pub(crate) mod tests;
pub mod tool;

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

/// The running time one program may take under the JIT, and under the interpreter (the user, 2026-11-11): far past
/// what the step limit lets JavaScript run, so only the work the steps don't see (`JSON.parse`, a comparator's sort,
/// string building) meets it.
pub const CLOCK_JIT: Duration = Duration::from_secs(10);
pub const CLOCK_INTERPRETER: Duration = Duration::from_secs(60);

impl Limits {
    /// The running time this program may take (`CLOCK_JIT`, `CLOCK_INTERPRETER`).
    pub fn clock(&self) -> Duration {
        if self.jit { CLOCK_JIT } else { CLOCK_INTERPRETER }
    }
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
///   `indexOf` over megabytes) that is about 710 s at worst under the JIT and 3.8 h under the interpreter: over the
///   line, so the clock (`Limits::clock`, `Watch`) is the last net.
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

/// A program's clock and its stop. Its running time is summed over its `start` and every `resume` (`Program::step`
/// stamps each) — never the waits between. Its flag is the instance's interrupt: set by the step's watchdog once
/// the running time reaches the limit (`watchdog`), or by the host at any moment (`stop`).
pub struct Watch {
    flag: Interrupt,
    limit: Duration,
    ledger: Mutex<Ledger>,
    /// Rung as a step ends, the program ends or it's stopped: the step's watchdog ends with it, at once.
    wake: crate::event::Wake,
    /// Watchdogs alive.
    dogs: std::sync::atomic::AtomicU32,
    /// Tests: what a step's running is in a simulation (`src/sim/codeclock.rs`) — given the step's number, the
    /// watch, and the step's work to run. A simulation runs one thread at a time, so the engine's real work can't
    /// race its clock: the model says how the clock moves while a step runs.
    #[cfg(test)]
    model: Option<Model>,
}

/// A step's running in a simulation (`Watch::model`).
#[cfg(test)]
pub type Model = Arc<dyn Fn(u32, &Watch, &mut (dyn FnMut() + Send)) + Send + Sync>;

#[derive(Default)]
struct Ledger {
    /// The running time of the steps that ended.
    ran: Duration,
    /// When the step running began.
    since: Option<Instant>,
    /// Steps begun.
    steps: u32,
    /// Who set the flag, and the running time then.
    why: Option<Why>,
    at_stop: Option<Duration>,
    /// The program ended (or was dropped).
    ended: bool,
}

/// Who stopped a program through its flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// Its running time reached the limit.
    Clock,
    /// The host, saying why (the user's `esc`, the turn's end: piece c's words).
    Host(String),
}

impl Watch {
    /// A clock at `limits`' limit (`Limits::clock`), its flag unset.
    pub fn new(limits: &Limits) -> Arc<Watch> {
        Arc::new(Watch {
            flag: Interrupt::new(),
            limit: limits.clock(),
            ledger: Mutex::default(),
            wake: crate::event::Wake::default(),
            dogs: Default::default(),
            #[cfg(test)]
            model: None,
        })
    }

    /// `new`, its steps' running modelled in a simulation (`Watch::model`).
    #[cfg(test)]
    pub fn modelled(limits: &Limits, model: Model) -> Arc<Watch> {
        Arc::new(Watch { model: Some(model), ..Arc::into_inner(Watch::new(limits)).unwrap() })
    }

    fn ledger(&self) -> std::sync::MutexGuard<'_, Ledger> {
        self.ledger.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Stops the program, from any thread at any moment: a step running ends at the runtime's next check, a
    /// program waiting at its next step; its end says `stopped: {why}`.
    pub fn stop(&self, why: &str) {
        self.stop_by(&mut self.ledger(), Why::Host(why.into()));
    }

    /// The flag set, the first reason kept.
    fn stop_by(&self, l: &mut Ledger, why: Why) {
        if l.why.is_none() {
            l.at_stop = Some(l.ran + l.since.map_or(Duration::ZERO, crate::clock::since));
            l.why = Some(why);
        }
        self.flag.set();
        self.wake.ring();
    }

    /// Its running time when it was stopped, if it was. (Tests till piece c's rows.)
    #[cfg(test)]
    pub fn stopped_at(&self) -> Option<Duration> {
        self.ledger().at_stop
    }

    /// Who stopped it, if anyone did.
    pub fn stopped(&self) -> Option<Why> {
        self.ledger().why.clone()
    }

    /// Its running time so far, the step running included. (Tests till piece c's rows.)
    #[cfg(test)]
    pub fn ran(&self) -> Duration {
        let l = self.ledger();
        l.ran + l.since.map_or(Duration::ZERO, crate::clock::since)
    }

    /// Steps begun.
    pub fn steps(&self) -> u32 {
        self.ledger().steps
    }

    /// A step begins (its number): with no running time left it's stopped before it runs.
    fn begin(&self) -> u32 {
        let mut l = self.ledger();
        l.steps += 1;
        l.since = Some(crate::clock::now());
        if l.ran >= self.limit {
            self.stop_by(&mut l, Why::Clock);
        }
        l.steps
    }

    /// The step running ends: its time added, its watchdog ended.
    fn end(&self) {
        let mut l = self.ledger();
        if let Some(t) = l.since.take() {
            l.ran += crate::clock::since(t);
        }
        drop(l);
        self.wake.ring();
    }

    fn close(&self) {
        self.ledger().ended = true;
        self.wake.ring();
    }

    /// Watchdogs alive, and of them those asleep that nothing has woken (tests: none outlives its step).
    #[cfg(test)]
    pub fn dogs(&self) -> (u32, usize) {
        (self.dogs.load(std::sync::atomic::Ordering::SeqCst), self.wake.asleep())
    }

    /// What step `k`'s watchdog does now: sleep this long (the running time left — it can't run out sooner), or
    /// nothing more (None): its step is over, the program ended or was stopped — or the time ran out, its step
    /// running, and the flag is set here, under the lock a step's end takes.
    fn tick(&self, k: u32) -> Option<Duration> {
        let mut l = self.ledger();
        let over = l.steps > k || (l.steps == k && l.since.is_none());
        if l.ended || l.why.is_some() || over {
            return None;
        }
        let running = l.since.filter(|_| l.steps == k);
        let left = self.limit.saturating_sub(l.ran + running.map_or(Duration::ZERO, crate::clock::since));
        if !left.is_zero() {
            return Some(left);
        }
        // (not begun yet: its `begin` finds nothing left)
        if running.is_some() {
            self.stop_by(&mut l, Why::Clock);
        }
        None
    }
}

/// Step `k`'s watchdog, the last net behind the steps and the fuel: a job that sleeps the running time left
/// (`event::Wake`: a simulation moves it), then sets the flag once it ran out with the step still running. Its
/// step's end rings it awake and it ends, at once — a program waiting on a card or an answer holds no sleeping
/// thread.
fn watchdog(watch: Weak<Watch>, k: u32) {
    let Some(w) = watch.upgrade() else { return };
    loop {
        // (the rings read before the look: a step that ends between the two ends the sleep at once)
        let seen = w.wake.rings();
        let Some(left) = w.tick(k) else { break };
        w.wake.sleep(left, seen);
    }
    w.dogs.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
}

/// A program waiting on its calls: its own instance of the engine.
pub struct Program {
    inst: Instance,
    watch: Arc<Watch>,
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

impl Drop for Program {
    fn drop(&mut self) {
        self.watch.close();
    }
}

impl Program {
    /// Runs `src` on a fresh instance of `engine` until it waits or ends — a worker's. Its clock runs (`Watch`), but
    /// with no watchdog nothing holds it to its limit: `start_job` gives it one. (Tests: the tool starts by `start_job`.)
    #[cfg(test)]
    pub fn start(engine: &Engine, src: &str, limits: Limits) -> Next {
        Program::start_with(engine, src, limits, Watch::new(&limits))
    }

    /// `start`, on `watch` — its flag the instance's interrupt.
    #[cfg(test)]
    pub fn start_with(engine: &Engine, src: &str, limits: Limits, watch: Arc<Watch>) -> Next {
        Program::start_in(engine, "", src, limits, watch)
    }

    /// `start_with`, `prelude` before `src` on its first line (no line break in it: an error's line stays the
    /// program's own) — the tool's bindings, say; the hint reads `src` alone.
    pub fn start_in(engine: &Engine, prelude: &str, src: &str, limits: Limits, watch: Arc<Watch>) -> Next {
        debug_assert!(!prelude.contains('\n'), "a prelude on the program's first line");
        let wasm = tarae_wasm::Limits { memory_bytes: limits.memory, jit: limits.jit, ..Default::default() };
        let mut inst = match Instance::new(engine.module.clone(), wasm, limits.fuel) {
            Ok(i) => i,
            Err(t) => {
                watch.close();
                return Next::Ended(Ended::failed(stopped(Stop::Trap(t), &watch)));
            }
        };
        inst.set_interrupt(watch.flag.clone());
        let mut p = Program {
            inst,
            watch,
            fns: engine.fns,
            out: BTreeSet::new(),
            asked: 0,
            fuel: limits.fuel,
            steps: limits.steps,
        };
        let whole = format!("{prelude}{src}");
        let (init, bytes, steps) = (engine.init, whole.as_bytes(), limits.steps as i32);
        let reply = p.step(move |p| {
            if let Some(init) = init {
                p.inst.call(init, &[])?;
            }
            let (at, len) = p.put(bytes)?;
            p.call(p.fns.start, &[at, len, Val::I32(steps)])
        });
        p.next(reply, Some(src))
    }

    /// One step of the program — its `start` or a `resume` — between its clock's stamps.
    fn step(&mut self, work: impl FnOnce(&mut Program) -> Result<Val, Stop> + Send) -> Result<Val, Stop> {
        let watch = self.watch.clone();
        let _n = watch.begin();
        #[cfg(test)]
        if let Some(model) = watch.model.clone() {
            let (mut work, mut out) = (Some(work), None);
            model(_n, &watch, &mut || out = work.take().map(|w| w(self)));
            watch.end();
            return out.expect("the model ran the step");
        }
        let out = work(self);
        watch.end();
        out
    }

    /// Answers call `id` with `value` (a refused or failed call is the binding's `{ error }`, a value too) and runs
    /// the program on — a worker's.
    pub fn answer(mut self, id: u32, value: &Value) -> Next {
        if !self.out.remove(&id) {
            return Next::Ended(Ended::failed(format!("the host answered call {id}, which isn't waiting")));
        }
        let text = value.to_string();
        let reply = self.step(|p| {
            let (at, len) = p.put(text.as_bytes())?;
            p.call(p.fns.resume, &[Val::I32(id as i32), at, len, Val::I32(0)])
        });
        self.next(reply, None)
    }

    /// The calls handed out and not answered yet.
    #[cfg(test)]
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
            Err(s) => return Next::Ended(Ended { fuel, ..Ended::failed(stopped(s, &self.watch)) }),
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

/// The words for a program the clock ended: the limit itself, and the way out.
pub(crate) fn time_limit(limit: Duration) -> String {
    format!(
        "stopped at the limit of one program's time ({} of running): {REMEDY}",
        crate::term::run_duration(limit)
    )
}

/// The words for a program stopped through its flag (`Watch`), at `limits`' clock.
pub(crate) fn stopped_words(why: &Why, limits: &Limits) -> String {
    match why {
        Why::Clock => time_limit(limits.clock()),
        Why::Host(why) => format!("stopped: {why}"),
    }
}

/// The words for a step that trapped or broke the protocol: the instance is dropped with it.
fn stopped(s: Stop, watch: &Watch) -> String {
    match s {
        Stop::Trap(Trap::OutOfFuel) => format!(
            "stopped at the limit of one program's work (what the steps don't count too: strings, JSON, sorting): \
             {REMEDY}"
        ),
        Stop::Trap(Trap::Interrupted) => match watch.stopped() {
            Some(Why::Clock) => time_limit(watch.limit),
            Some(why) => stopped_words(&why, &LIMITS),
            None => "the engine stopped: interrupted".into(),
        },
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

/// Starts `src` (`prelude` before it, `Program::start_in`) on a worker, on the engine `load` gives there
/// (`Engine::here`), held to `watch` (the host keeps it: `Watch::stop`); `then` takes what comes on the main loop.
#[allow(clippy::too_many_arguments, reason = "a program's start: each is its own")]
pub fn start_job(
    jobs: &Jobs,
    load: impl FnOnce() -> Result<Arc<Engine>, String> + Send + 'static,
    prelude: &'static str,
    src: String,
    limits: Limits,
    watch: Arc<Watch>,
    then: impl FnOnce(&mut Editor, Next) + Send + 'static,
) {
    watched(jobs, &watch);
    jobs.spawn(move || {
        let next = match load() {
            Ok(e) => Program::start_in(&e, prelude, &src, limits, watch),
            Err(why) => {
                watch.close();
                Next::Ended(Ended::failed(why))
            }
        };
        move |ed: &mut Editor| then(ed, next)
    });
}

/// The next step's watchdog, beside it (a timer: nothing lands).
fn watched(jobs: &Jobs, watch: &Arc<Watch>) {
    let (dog, k) = (Arc::downgrade(watch), watch.steps() + 1);
    watch.dogs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    jobs.detached(move || watchdog(dog, k));
}

/// Answers call `id` of `program` on a worker; `then` takes what comes on the main loop.
pub fn answer_job(
    jobs: &Jobs,
    program: Box<Program>,
    id: u32,
    value: Value,
    then: impl FnOnce(&mut Editor, Next) + Send + 'static,
) {
    watched(jobs, &program.watch);
    jobs.spawn(move || {
        let next = program.answer(id, &value);
        move |ed: &mut Editor| then(ed, next)
    });
}
