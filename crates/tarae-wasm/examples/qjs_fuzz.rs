//! The code tool's fuzz hour (`.sb/codemode-s1-brief.md` §5): QuickJS-ng under the JIT against the interpreter,
//! on generated JavaScript. Each seed makes one program and its limits; the program runs on a fresh instance under
//! each, its calls answered alike, and everything is compared: every reply whole (the value, what it printed, the
//! ending's kind, the steps), every trap, and after every step the fuel left (after an out-of-fuel trap too: both
//! engines leave none), the linear memory's hash and length, and whether a grow was refused. A difference is a finding, kept with its
//! seed and program.
//!
//! ```text
//! cargo build --release -p tarae-wasm --example qjs_fuzz
//! qjs_fuzz <quickjs.wasm> --out DIR [--from SEED] [--secs N] [--threads N]   # the run
//! qjs_fuzz <quickjs.wasm> --replay SEED                                      # one case again, both traces
//! qjs_fuzz <quickjs.wasm> --selftest                                         # the comparison sees a difference
//! ```
#![allow(clippy::disallowed_methods, reason = "a dev harness, not in the release binary")]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tarae_wasm::{Instance, Interrupt, Limits, Module, Trap, Val};

/// The tool's memory cap (`codetool::limits`).
const MEMORY: usize = 64 << 20;
/// `codetool::FUEL_A_STEP`, and the tool's step limit (`codetool::LIMITS`).
const FUEL_A_STEP: u64 = 34_000_000_000;
const TOOL_STEPS: u32 = 180;
/// The fuel a case gets at most, for throughput (about 35 s of the interpreter): a case past it ends at the work
/// limit, compared like any other ending.
const FUEL_CAP: u64 = 30_000_000_000;
/// A run past this is stopped and its case not compared (counted as such).
const PATIENCE: Duration = Duration::from_secs(600);
/// Every this many seeds, the interpreter runs the case twice: a control on the harness's own determinism.
const CONTROL_EVERY: u64 = 50;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: qjs_fuzz <quickjs.wasm> …");
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let bytes = std::fs::read(path).expect("read the module");
    let module = Arc::new(Module::new(&bytes).expect("the runtime takes it"));
    assert!(module.compiled(), "the JIT declined the module: there is nothing to compare");
    let f = |n: &str| module.func(n).unwrap_or_else(|| panic!("no export {n}"));
    let fns =
        Fns { alloc: f("alloc"), start: f("start"), resume: f("resume"), init: module.func("_initialize") };
    let engine = Engine { module, fns };
    // compiled code nests on its thread's stack (`Limits::call_depth`)
    let big = |f: Box<dyn FnOnce() + Send>| {
        std::thread::Builder::new().stack_size(64 << 20).spawn(f).unwrap().join().unwrap()
    };
    if let Some(seed) = flag("--replay") {
        let seed = seed.parse().expect("a seed");
        big(Box::new(move || replay(&engine, seed)));
    } else if args.iter().any(|a| a == "--selftest") {
        big(Box::new(move || selftest(&engine)));
    } else {
        let from = flag("--from").map_or(0, |s| s.parse().expect("a seed"));
        let secs = flag("--secs").map_or(3600, |s| s.parse().expect("seconds"));
        let threads = flag("--threads").map_or(6, |s| s.parse().expect("a count"));
        let out = std::path::PathBuf::from(flag("--out").expect("--out DIR"));
        fuzz(Arc::new(engine), from, Duration::from_secs(secs), threads, &out);
    }
}

#[derive(Clone, Copy)]
struct Fns {
    alloc: u32,
    start: u32,
    resume: u32,
    init: Option<u32>,
}

struct Engine {
    module: Arc<Module>,
    fns: Fns,
}

// ---- one case

struct Case {
    seed: u64,
    src: String,
    steps: u32,
    fuel: u64,
    /// The host's flag set before this resume (1: the first) — a deterministic stop.
    interrupt_at: Option<u32>,
}

fn case(seed: u64) -> Case {
    let mut g = Gen::new(seed);
    let steps = g.r.weighted(&[(1u32, 15), (2, 15), (5, 30), (20, 25), (TOOL_STEPS, 15)]);
    let fuel = match g.r.pct(10) {
        true => g.r.range(1_000_000, 20_000_000_000),
        false => (2 * steps as u64 * FUEL_A_STEP).min(FUEL_CAP),
    };
    let interrupt_at = g.r.pct(3).then(|| 1 + g.r.below(3) as u32);
    let src = g.program();
    Case { seed, src, steps, fuel, interrupt_at }
}

/// What one run did: a line per reply, trap and step's state — the thing compared.
struct Run {
    trace: Vec<String>,
    ending: String,
    timed_out: bool,
}

/// The tool's answers, by name: alike under both, so a difference in them is the engine's.
fn answer(name: &str, log: &str) -> (String, bool) {
    match name {
        "num" => ("42.5".into(), false),
        "str" => (r#""line one\nline two é""#.into(), false),
        "obj" => (r#"{"a":1,"b":[1,2.5,-0.0,1e300],"s":"x","n":null}"#.into(), false),
        "list" => (r#"[3,1,2,"x",{"k":true},[]]"#.into(), false),
        "log" => (log.into(), false),
        "fail" => (r#""refused: not here""#.into(), true),
        _ => ("null".into(), false),
    }
}

/// A test run's output, as a JSON string: mostly passes, some failures and errors.
fn test_log() -> String {
    let mut s = String::from("\"");
    for i in 0..300u32 {
        match i % 23 {
            0 => write!(s, "FAIL src/m{i}.rs:{} assertion `left == right` failed\\n", i * 7),
            5 => write!(s, "error[E0{i:03}]: mismatched types at lib.rs:{i}\\n"),
            _ => write!(s, "test m{i}::case_{} ... ok\\n", i * 31 % 97),
        }
        .unwrap();
    }
    s.push('"');
    s
}

fn hash(m: &[u8]) -> u64 {
    let mut h = 0x243f_6a88_85a3_08d3u64 ^ m.len() as u64;
    let (chunks, rest) = m.as_chunks::<8>();
    for &c in chunks {
        h = (h ^ u64::from_le_bytes(c)).wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(29);
    }
    for &b in rest {
        h = (h ^ b as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    h
}

/// Runs `c` on a fresh instance, answering every call, and says all it did.
fn run(e: &Engine, c: &Case, jit: bool, guard: &Guard, log: &str) -> Run {
    let timed_out = Arc::new(AtomicBool::new(false));
    let mut trace = Vec::new();
    let limits = Limits { memory_bytes: MEMORY, jit, ..Limits::default() };
    let out = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let mut inst = match Instance::new(e.module.clone(), limits, c.fuel) {
            Ok(i) => i,
            Err(t) => return format!("trap:{t:?}"),
        };
        let flag = Interrupt::new();
        inst.set_interrupt(flag.clone());
        let _watched = guard.watch(flag.clone(), timed_out.clone());
        let fns = e.fns;
        let call = |inst: &mut Instance, f: u32, args: &[Val]| -> Result<i32, Trap> {
            match inst.call(f, args)?[..] {
                [Val::I32(v)] => Ok(v),
                ref other => panic!("export gave {other:?}"),
            }
        };
        let put = |inst: &mut Instance, text: &[u8]| -> Result<i32, Trap> {
            let p = call(inst, fns.alloc, &[Val::I32(text.len() as i32)])?;
            inst.memory_mut()[p as u32 as usize..p as u32 as usize + text.len()].copy_from_slice(text);
            Ok(p)
        };
        let state = |inst: &Instance| {
            let m = inst.memory();
            let fuel = inst.fuel();
            format!("fuel {fuel} mem {:016x} len {} grow_refused {}", hash(m), m.len(), inst.grow_refused())
        };
        let mut reply = (|| {
            if let Some(init) = fns.init {
                inst.call(init, &[])?;
            }
            let p = put(&mut inst, c.src.as_bytes())?;
            call(&mut inst, fns.start, &[Val::I32(p), Val::I32(c.src.len() as i32), Val::I32(c.steps as i32)])
        })();
        let mut queue: Vec<(i32, String)> = Vec::new();
        let mut resumes = 0u32;
        loop {
            let p = match reply {
                Err(t) => {
                    trace.push(format!("trap {t:?}"));
                    trace.push(state(&inst));
                    return format!("trap:{t:?}");
                }
                Ok(p) => p,
            };
            let m = inst.memory();
            let at = p as u32 as usize;
            let Some(len) = m.get(at..at + 4) else { return "bad-reply".into() };
            let len = u32::from_le_bytes(len.try_into().unwrap()) as usize;
            let Some(text) = m.get(at + 4..at + 4 + len) else { return "bad-reply".into() };
            let text = String::from_utf8_lossy(text).into_owned();
            trace.push(text.clone());
            trace.push(state(&inst));
            let (head, body) = text.split_once('\n').unwrap_or((&text, ""));
            if !head.starts_with("pending") {
                let mut kind = match head {
                    "done" => "done".to_string(),
                    "error" if body.contains(r#""stopped":"steps""#) => "error:steps".into(),
                    "error" if body.contains(r#""syntax":true"#) => "error:syntax".into(),
                    "error" if body.contains(r#""error":"InternalError: out of memory"#) => {
                        "error:oom".into()
                    }
                    "error" => "error:js".into(),
                    other => format!("other:{other}"),
                };
                if inst.grow_refused() {
                    kind.push_str("+grow-refused");
                }
                return kind;
            }
            for c in head.split(' ').skip(1) {
                let (id, name) = c.split_once(':').unwrap_or((c, ""));
                queue.push((id.parse().unwrap_or(-1), name.to_string()));
            }
            if queue.is_empty() {
                return "stuck".into();
            }
            let (id, name) = queue.remove(0);
            let (a, err) = answer(&name, log);
            resumes += 1;
            if c.interrupt_at == Some(resumes) {
                flag.set();
            }
            reply = (|| {
                let p = put(&mut inst, a.as_bytes())?;
                call(
                    &mut inst,
                    fns.resume,
                    &[Val::I32(id), Val::I32(p), Val::I32(a.len() as i32), Val::I32(err as i32)],
                )
            })();
        }
    }));
    let ending = out.unwrap_or_else(|p| {
        let why =
            p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
        let why = format!("panic:{}", why.unwrap_or_default());
        trace.push(why.clone());
        why
    });
    let timed_out = timed_out.load(Ordering::SeqCst);
    Run { trace, ending: if timed_out { "timeout".into() } else { ending }, timed_out }
}

/// The first line two traces differ at (None: the same).
fn differ(a: &Run, b: &Run) -> Option<usize> {
    if a.trace == b.trace && a.ending == b.ending {
        return None;
    }
    Some(a.trace.iter().zip(&b.trace).position(|(x, y)| x != y).unwrap_or(a.trace.len().min(b.trace.len())))
}

fn cut(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => format!("{}… ({} bytes)", &s[..i], s.len()),
        None => s.to_string(),
    }
}

fn describe(c: &Case, runs: &[(&str, &Run)], at: Option<usize>) -> String {
    let mut s = format!(
        "seed {}\nsteps {} fuel {} interrupt_at {:?}\nfirst difference: {at:?}\n",
        c.seed, c.steps, c.fuel, c.interrupt_at
    );
    for (name, r) in runs {
        writeln!(s, "\n== {name}: {} ({} lines)", r.ending, r.trace.len()).unwrap();
        for (i, l) in r.trace.iter().enumerate() {
            let mark = if Some(i) == at { ">>" } else { "  " };
            writeln!(s, "{mark} {i}: {}", if Some(i) == at { l.clone() } else { cut(l, 600) }).unwrap();
        }
    }
    s
}

// ---- the run

/// A run watched: since when, its flag, and the mark that the guard stopped it.
type Slot = (Instant, Interrupt, Arc<AtomicBool>);

/// Stops a run past `PATIENCE` (its case then isn't compared).
#[derive(Default)]
struct Guard {
    slots: Mutex<BTreeMap<u64, Slot>>,
    next: AtomicU64,
}

struct Watched<'a>(&'a Guard, u64);

impl Drop for Watched<'_> {
    fn drop(&mut self) {
        self.0.slots.lock().unwrap().remove(&self.1);
    }
}

impl Guard {
    fn watch(&self, flag: Interrupt, timed_out: Arc<AtomicBool>) -> Watched<'_> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        self.slots.lock().unwrap().insert(id, (Instant::now(), flag, timed_out));
        Watched(self, id)
    }

    fn tick(&self) {
        for (since, flag, timed_out) in self.slots.lock().unwrap().values() {
            if since.elapsed() > PATIENCE && !flag.is_set() {
                timed_out.store(true, Ordering::SeqCst);
                flag.set();
            }
        }
    }
}

#[derive(Default)]
struct Tally {
    cases: u64,
    compared: u64,
    not_compared: u64,
    findings: Vec<u64>,
    controls: u64,
    control_failures: Vec<u64>,
    endings: BTreeMap<String, u64>,
    steps_limits: BTreeMap<u32, u64>,
    src_bytes: u64,
    trace_lines: u64,
    /// The load average's samples, one a minute.
    loads: Vec<f64>,
}

/// The machine's load average over the last minute (None where the sandbox hides it).
#[allow(unsafe_code)]
fn load() -> Option<f64> {
    let mut l = [0f64; 3];
    // SAFETY: getloadavg writes at most the count of doubles it's given.
    (unsafe { libc::getloadavg(l.as_mut_ptr(), 3) } >= 1).then_some(l[0])
}

#[allow(unsafe_code)]
fn cpu_seconds() -> f64 {
    // SAFETY: getrusage writes the struct it's given.
    unsafe {
        let mut u: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut u);
        let t = |v: libc::timeval| v.tv_sec as f64 + v.tv_usec as f64 / 1e6;
        t(u.ru_utime) + t(u.ru_stime)
    }
}

fn fuzz(e: Arc<Engine>, from: u64, wall: Duration, threads: usize, out: &std::path::Path) {
    std::fs::create_dir_all(out.join("findings")).unwrap();
    let log_path = out.join("log.txt");
    let logf = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(&log_path).unwrap());
    let say = |s: &str| {
        println!("{s}");
        let mut f = logf.lock().unwrap();
        writeln!(f, "{s}").unwrap();
    };
    let begun = Instant::now();
    let deadline = begun + wall;
    let next = AtomicU64::new(from);
    let tally = Mutex::new(Tally::default());
    let guard = Guard::default();
    let done = AtomicBool::new(false);
    let tool_log = test_log();
    say(&format!(
        "qjs_fuzz: from seed {from}, {} s of wall time, {threads} threads, pid {}",
        wall.as_secs(),
        std::process::id()
    ));
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for _ in 0..threads {
            let b = std::thread::Builder::new().stack_size(64 << 20);
            workers.push(
                b.spawn_scoped(scope, || {
                    while Instant::now() < deadline {
                        let seed = next.fetch_add(1, Ordering::SeqCst);
                        let c = case(seed);
                        let jit = run(&e, &c, true, &guard, &tool_log);
                        let int = run(&e, &c, false, &guard, &tool_log);
                        let control = (seed.is_multiple_of(CONTROL_EVERY) && !int.timed_out)
                            .then(|| run(&e, &c, false, &guard, &tool_log))
                            .filter(|again| !again.timed_out);
                        let mut t = tally.lock().unwrap();
                        t.cases += 1;
                        t.src_bytes += c.src.len() as u64;
                        t.trace_lines += int.trace.len() as u64;
                        *t.steps_limits.entry(c.steps).or_default() += 1;
                        if let Some(again) = &control {
                            t.controls += 1;
                            if let Some(at) = differ(&int, again) {
                                t.control_failures.push(seed);
                                let text = describe(
                                    &c,
                                    &[("interpreter", &int), ("interpreter again", again)],
                                    Some(at),
                                );
                                std::fs::write(out.join(format!("findings/control-{seed}.txt")), text)
                                    .unwrap();
                                std::fs::write(out.join(format!("findings/control-{seed}.js")), &c.src)
                                    .unwrap();
                            }
                        }
                        if jit.timed_out || int.timed_out {
                            t.not_compared += 1;
                            *t.endings.entry("timeout".into()).or_default() += 1;
                            continue;
                        }
                        t.compared += 1;
                        *t.endings.entry(int.ending.clone()).or_default() += 1;
                        if let Some(at) = differ(&jit, &int) {
                            t.findings.push(seed);
                            drop(t);
                            let text = describe(&c, &[("JIT", &jit), ("interpreter", &int)], Some(at));
                            std::fs::write(out.join(format!("findings/{seed}.txt")), text).unwrap();
                            std::fs::write(out.join(format!("findings/{seed}.js")), &c.src).unwrap();
                            say(&format!(
                                "FINDING seed {seed}: {} vs {} (line {at})",
                                jit.ending, int.ending
                            ));
                        }
                    }
                })
                .unwrap(),
            );
        }
        // the guard and the progress lines
        scope.spawn(|| {
            let mut last = Instant::now();
            while !done.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(500));
                guard.tick();
                if last.elapsed() >= Duration::from_secs(60) {
                    last = Instant::now();
                    let mut t = tally.lock().unwrap();
                    let el = begun.elapsed().as_secs_f64();
                    let l = load();
                    t.loads.extend(l);
                    say(&format!(
                        "{:>5.0} s: load {}, {} cases ({:.1}/s), {} compared, {} findings, {} controls ({} failed), cpu {:.0}% of {threads} threads, next seed {}; endings {:?}",
                        el,
                        l.map_or("n/a".into(), |l| format!("{l:.1}")),
                        t.cases,
                        t.cases as f64 / el,
                        t.compared,
                        t.findings.len(),
                        t.controls,
                        t.control_failures.len(),
                        100.0 * cpu_seconds() / (el * threads as f64),
                        next.load(Ordering::SeqCst),
                        t.endings
                    ));
                }
            }
        });
        for w in workers {
            w.join().unwrap();
        }
        done.store(true, Ordering::SeqCst);
    });
    let t = tally.into_inner().unwrap();
    let el = begun.elapsed().as_secs_f64();
    let end = next.load(Ordering::SeqCst);
    let summary = format!(
        "DONE: seeds {from}..{end} ({} seeds, every one ran), {:.0} s of wall time, cpu {:.0}% of {threads} threads\n\
         cases {} · compared {} · not compared (a run past {} s) {} · findings {} {:?}\n\
         controls (the interpreter twice) {} · failed {} {:?}\n\
         endings (compared) {:?}\n\
         step limits {:?}\n\
         program size: {} bytes on average · trace lines a case: {:.1} on average\n\
         load average (1 min, sampled each minute, on {} cores): {}",
        end - from,
        el,
        100.0 * cpu_seconds() / (el * threads as f64),
        t.cases,
        t.compared,
        PATIENCE.as_secs(),
        t.not_compared,
        t.findings.len(),
        t.findings,
        t.controls,
        t.control_failures.len(),
        t.control_failures,
        t.endings,
        t.steps_limits,
        t.src_bytes / t.cases.max(1),
        t.trace_lines as f64 / t.cases.max(1) as f64,
        std::thread::available_parallelism().map_or(0, |n| n.get()),
        match t.loads.is_empty() {
            true => "not readable here".to_string(),
            false => format!(
                "min {:.1}, mean {:.1}, max {:.1} ({} samples)",
                t.loads.iter().copied().fold(f64::MAX, f64::min),
                t.loads.iter().sum::<f64>() / t.loads.len() as f64,
                t.loads.iter().copied().fold(0.0, f64::max),
                t.loads.len()
            ),
        },
    );
    say(&summary);
    std::fs::write(out.join("summary.txt"), summary + "\n").unwrap();
}

fn replay(e: &Engine, seed: u64) {
    let c = case(seed);
    let (guard, log) = (Guard::default(), test_log());
    let jit = run(e, &c, true, &guard, &log);
    let int = run(e, &c, false, &guard, &log);
    let at = differ(&jit, &int);
    println!("---- program\n{}\n----", c.src);
    println!("{}", describe(&c, &[("JIT", &jit), ("interpreter", &int)], at));
    println!("{}", if at.is_some() { "DIFFER" } else { "SAME" });
}

/// The comparison sees a difference: seed s's JIT run against seed s+1's interpreter run differ, and a case
/// against itself doesn't. Then a few cases, each ending said.
fn selftest(e: &Engine) {
    let (guard, log) = (Guard::default(), test_log());
    for seed in 0..20 {
        let (c, d) = (case(seed), case(seed + 1));
        let (a, b) = (run(e, &c, true, &guard, &log), run(e, &d, false, &guard, &log));
        assert!(differ(&a, &b).is_some(), "seed {seed}: two programs compared the same");
        let i = run(e, &c, false, &guard, &log);
        println!(
            "seed {seed}: {} · {} bytes · {}",
            a.ending,
            c.src.len(),
            if differ(&a, &i).is_some() { "DIFFER" } else { "same" }
        );
    }
    println!("selftest: a planted difference is seen");
}

// ---- the corpus

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.below(hi - lo + 1)
    }
    fn pct(&mut self, p: u64) -> bool {
        self.below(100) < p
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize]
    }
    fn weighted<T: Copy>(&mut self, xs: &[(T, u64)]) -> T {
        let mut k = self.below(xs.iter().map(|x| x.1).sum());
        for &(x, w) in xs {
            if k < w {
                return x;
            }
            k -= w;
        }
        unreachable!()
    }
}

const NUMS: &[&str] = &[
    "0",
    "(-0)",
    "1",
    "(-1)",
    "2",
    "3",
    "7",
    "10",
    "31",
    "32",
    "255",
    "256",
    "65535",
    "65536",
    "2147483647",
    "(-2147483648)",
    "4294967295",
    "4294967296",
    "(2**53)",
    "9007199254740993",
    "1e21",
    "1e-7",
    "0.1",
    "0.2",
    "0.5",
    "1.5",
    "(-1.5)",
    "2.5",
    "(-2.5)",
    "3.14159",
    "NaN",
    "Infinity",
    "(-Infinity)",
    "Number.MIN_VALUE",
    "Number.MAX_VALUE",
    "Number.EPSILON",
    "(1/3)",
    "123456789",
    "0x7f",
    "0o17",
    "0b1011",
    "1e308",
    "5e-324",
    "4.35",
    "0.000001",
    "1e-300",
    "(-1e-300)",
    "Number.MAX_SAFE_INTEGER",
    "(-0.5)",
    "0.49999999999999994",
    "2.225073858507201e-308",
    "(2**31)",
    "(2**32 + 0.5)",
    "(-(2**63))",
    "1.7976931348623157e308",
];
const STRS: &[&str] = &[
    "''",
    "'abc'",
    "'Hello, World'",
    "'  pad  '",
    "'ünïcödé'",
    "'😀x'",
    "'a,b,,c'",
    "'1e3'",
    "'0x1F'",
    "'-0'",
    "'  42  '",
    "'\\u0000z'",
    "'NaN'",
    "'true'",
    "'aaaaaaaaaaaaaaaaaaaa!'",
    "'The quick brown fox jumps over 12 lazy dogs'",
    "'x=1;y=22;z=333'",
    "'\\uD83D'",
    "'Ⅻ ß ﬁ İ ǅ'",
    "'1_000'",
    "'  -12.5e3xyz'",
    "'\\t\\n'",
    "'Infinity'",
    "'0b101'",
    "'.5'",
    "'2020-02-29T12:00:00Z'",
    "'Tue, 01 Mar 2022 10:00:00 GMT'",
    "'中文字符'",
    "'e\\u0301'",
];
const REGEXES: &[&str] = &[
    r"/a+/g",
    r"/(\w)(\d)?/g",
    r"/[aeiou]/gi",
    r"/^\s+|\s+$/g",
    r"/(?<w>\w+)\s(?<n>\d+)/",
    r"/\p{L}+/gu",
    r"/(a|b)*c/",
    r"/.*?,/g",
    r"/\bfox\b/i",
    r"/(?=a)a/g",
    r"/(.)\1/g",
    r"/[^\x00-\x7f]/gu",
    r"/(a+)+b/",
    r"/\d{2,}/g",
    r"/(?<!x)y/g",
    r"/[\u{1F600}-\u{1F64F}]/u",
    r"/(?:)/g",
    r"/[A-Z][a-z]*/g",
    r"/(\d+)\.(\d*)/",
];
const JSONS: &[&str] = &[
    r#"'{"a":[1,2.5e3,-0,"x\\u0041"],"b":{"c":null}}'"#,
    r#"'[1e400, -1e-400, 0.1, 123456789012345678901234567890]'"#,
    r#"'"\\ud83d\\ude00"'"#,
    r#"'{"__proto__": {"x": 1}, "y": [true, false, null]}'"#,
    r#"'[[[[[[1]]]]], {"": ""}]'"#,
    r#"'{"a": 1,}'"#,
];
const BIGS: &[&str] = &[
    "0n",
    "1n",
    "(-1n)",
    "(2n ** 64n)",
    "(-(2n ** 63n))",
    "123456789012345678901234567890n",
    "7n",
    "255n",
    "(2n ** 127n - 1n)",
];
const BINOPS: &[&str] = &["+", "-", "*", "/", "%", "**", "&", "|", "^", "<<", ">>", ">>>", "+", "-", "*"];
const CMPS: &[&str] = &["<", "<=", ">", ">=", "==", "===", "!=", "!=="];
const MATH1: &[&str] = &[
    "abs", "floor", "ceil", "round", "trunc", "sign", "sqrt", "cbrt", "exp", "expm1", "log", "log1p", "log2",
    "log10", "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh",
    "fround", "clz32",
];
const MATH2: &[&str] = &["atan2", "pow", "max", "min", "imul", "hypot"];
const TOOLS: &[&str] = &["num", "str", "obj", "list", "log", "fail", "none"];
const ERRORS: &[&str] = &["Error", "TypeError", "RangeError", "SyntaxError"];
const BOOLS: &[&str] = &["true", "false"];

/// Every program begins with these: the result's recorder (`P` folds each value into `H` and keeps the first 200),
/// and a few helpers the statements call.
const PRELUDE: &str = r#"const R = []; let H = 0;
const h = (v) => { try { switch (typeof v) { case 'number': return Object.is(v, -0) ? '-0' : String(v); case 'bigint': return v + 'n'; case 'string': return v.length > 160 ? JSON.stringify(v.slice(0, 80)) + '…' + v.length : JSON.stringify(v); case 'undefined': return 'undefined'; case 'function': return 'fn:' + v.length; case 'symbol': return v.toString(); case 'boolean': return String(v); } if (v === null) return 'null'; const j = JSON.stringify(v, (k, x) => typeof x === 'bigint' ? x + 'n' : typeof x === 'number' && (!isFinite(x) || Object.is(x, -0)) ? String(x) : x); return j === undefined ? 'u' : j.length > 160 ? j.slice(0, 160) + '…' + j.length : j; } catch (e) { return 'E:' + (e && e.name); } };
const P = (v) => { const x = h(v); for (let k = 0; k < x.length; k++) H = (Math.imul(H, 31) + x.charCodeAt(k)) | 0; if (R.length < 200) R.push(x); };
const sq = (x) => x * x;
function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }
function deep(n) { return n <= 0 ? 0 : 1 + deep(n - 1); }
function gcd(x, y) { x = Math.abs(x); y = Math.abs(y); let k = 0; while (y && k++ < 100) [x, y] = [y, x % y]; return x; }
function* gen(n) { for (let k = 0; k < n; k++) yield k * k % 7; }
class Pt { constructor(x, y) { this.x = x; this.y = y; } get len() { return Math.hypot(this.x, this.y); } add(o) { return new Pt(this.x + o.x, this.y - o.y); } toString() { return `(${this.x},${this.y})`; } }
class Q { #v = 0; static n = 0; inc(x) { this.#v += x; Q.n++; return this.#v; } }
async function twice(n) { const x = await tool('num', n); const y = await tool('obj', n); return [x, y && y.a, n]; }
const F = new Float64Array(16), I = new Int32Array(16), U8 = new Uint8Array(32), DV = new DataView(new ArrayBuffer(32));
let A = [1, 2, 3], O = { n: 1, k: 'v' }, M = new Map(), St = new Set(), B = 12345678901234567890n;
"#;

struct Gen {
    r: Rng,
    /// Numbers and strings in scope beyond the fixed ones (loop counters, parameters).
    nvars: Vec<String>,
    svars: Vec<String>,
    /// Only literals: the variables aren't declared yet.
    literal: bool,
    can_await: bool,
    depth: u32,
    /// Loops around the statement being made: no tool call in one (the 64-call limit would end most programs).
    loops: u32,
    id: u32,
}

impl Gen {
    fn new(seed: u64) -> Gen {
        let r = Rng(seed ^ 0x5eed_c0de_0000_0000);
        Gen { r, nvars: vec![], svars: vec![], literal: false, can_await: true, depth: 0, loops: 0, id: 0 }
    }

    fn fresh(&mut self) -> u32 {
        self.id += 1;
        self.id
    }

    fn program(&mut self) -> String {
        let mut s = String::from(PRELUDE);
        self.literal = true;
        let inits: Vec<String> = (0..4).map(|_| self.num(2)).collect();
        writeln!(s, "let a = {}, b = {}, c = {}, d = {};", inits[0], inits[1], inits[2], inits[3]).unwrap();
        let (x, y) = (self.st(2), self.st(2));
        writeln!(s, "let s = {x}, t = {y};").unwrap();
        self.literal = false;
        for _ in 0..self.r.range(4, 30) {
            s.push_str(&self.stmt());
        }
        s.push_str("return { H, R, v: [a, b, c, d, s, t].map(h), A: A.slice(0, 16).map(h), B: h(B) };");
        s
    }

    fn n(&mut self) -> String {
        let d = self.r.range(0, 3) as u32;
        self.num(d)
    }

    fn s(&mut self) -> String {
        let d = self.r.range(0, 3) as u32;
        self.st(d)
    }

    fn num_leaf(&mut self) -> String {
        match self.r.below(10) {
            0..=3 if !self.literal => match !self.nvars.is_empty() && self.r.pct(50) {
                true => {
                    let i = self.r.below(self.nvars.len() as u64) as usize;
                    self.nvars[i].clone()
                }
                false => self.r.pick(&["a", "b", "c", "d"]).into(),
            },
            0..=6 => self.r.pick(NUMS).into(),
            7 | 8 => match self.r.range(0, 2000) as i64 - 1000 {
                v if v < 0 => format!("({v})"),
                v => v.to_string(),
            },
            _ => {
                let f = match self.r.pct(50) {
                    true => f64::from_bits(self.r.next()),
                    false => (self.r.range(0, 200_000) as f64 - 100_000.0) / 1000.0,
                };
                match f {
                    f if !f.is_finite() => "0.5".into(),
                    f if f.is_sign_negative() => format!("({f:e})"),
                    f => format!("{f:e}"),
                }
            }
        }
    }

    fn num(&mut self, d: u32) -> String {
        if d == 0 || self.r.pct(30) {
            return self.num_leaf();
        }
        let d = d - 1;
        match self.r.below(24) {
            0..=5 => {
                let (x, op, y) = (self.num(d), self.r.pick(BINOPS), self.num(d));
                format!("({x} {op} {y})")
            }
            6 => {
                let op = self.r.pick(&["-", "~", "+", "!"]);
                format!("({op}{})", self.num(d))
            }
            7 | 8 => format!("Math.{}({})", self.r.pick(MATH1), self.num(d)),
            9 => format!("Math.{}({}, {})", self.r.pick(MATH2), self.num(d), self.num(d)),
            10 | 11 => {
                let (x, c, y, p, q) = (self.num(d), self.r.pick(CMPS), self.num(d), self.num(d), self.num(d));
                format!("(({x}) {c} ({y}) ? {p} : {q})")
            }
            12 | 13 => {
                let s = self.st(d);
                match self.r.below(9) {
                    0 => format!("({s}).length"),
                    1 => format!("({s}).charCodeAt(({}) | 0)", self.num(d)),
                    2 => format!("({s}).indexOf({})", self.st(d)),
                    3 => {
                        format!("parseInt({s}, {})", self.r.pick(&["10", "16", "2", "36", "0", "undefined"]))
                    }
                    4 => format!("parseFloat({s})"),
                    5 => format!("Number({s})"),
                    6 => format!("(({s}).codePointAt(({}) & 7) ?? -1)", self.num(d)),
                    7 => format!("({s}).search({})", self.r.pick(REGEXES)),
                    _ => format!("({s}).localeCompare({})", self.st(d)),
                }
            }
            14 | 15 if !self.literal => {
                let (i, b) = (self.num(d), self.r.pick(BOOLS));
                match self.r.below(10) {
                    0 => format!("I[({i}) & 15]"),
                    1 => format!("F[({i}) & 15]"),
                    2 => format!("U8[({i}) & 31]"),
                    3 => format!("DV.getInt16(({i}) & 30, {b})"),
                    4 => format!("DV.getUint32(({i}) & 28, {b})"),
                    5 => format!("DV.getFloat64(({i}) & 24, {b})"),
                    6 => format!("DV.getFloat32(({i}) & 28, {b})"),
                    7 => format!("(A[({i}) & 7] ?? 0)"),
                    8 => "A.length".into(),
                    _ => format!("(O.n ?? {i})"),
                }
            }
            16 => format!("(({}) {} 0)", self.num(d), self.r.pick(&["|", ">>>", ">>", "^"])),
            17 => match self.r.below(4) {
                0 => format!("sq({})", self.num(d)),
                1 => format!("fib(({}) & 15)", self.num(d)),
                2 => format!("gcd(({}) | 0, ({}) | 0)", self.num(d), self.num(d)),
                _ => format!("new Pt({}, {}).len", self.num(d), self.num(d)),
            },
            18 if !self.literal => self
                .r
                .pick(&["Number(B % 1000003n)", "Number(BigInt.asIntN(32, B))", "Number(B & 255n)"])
                .into(),
            19 => {
                let x = self.num(d);
                match self.r.below(3) {
                    0 => format!("({x}).toString({}).length", self.r.range(2, 36)),
                    1 => format!("Number(({x}).toFixed({}))", self.r.below(21)),
                    _ => format!("Number(({x}).toPrecision({}))", self.r.range(1, 21)),
                }
            }
            20 => {
                let op = self.r.pick(BINOPS);
                format!("[{}, {}, {}].reduce((x, y) => x {op} y)", self.num(d), self.num(d), self.num(d))
            }
            21 if !self.literal => format!("Math.max(...A.slice(0, 8), {})", self.num(d)),
            22 => format!(
                "Date.UTC(2000 + (({}) & 63), ({}) & 11, (({}) & 27) + 1)",
                self.num(d),
                self.num(d),
                self.num(d)
            ),
            23 => format!("new Date(({}) * 86400000).getUTCDay()", self.num(d)),
            _ => format!("(({}) % ({}) || {})", self.num(d), self.num(d), self.num(d)),
        }
    }

    fn st_leaf(&mut self) -> String {
        if !self.literal {
            if !self.svars.is_empty() && self.r.pct(30) {
                let i = self.r.below(self.svars.len() as u64) as usize;
                return self.svars[i].clone();
            }
            if self.r.pct(40) {
                return self.r.pick(&["s", "t"]).into();
            }
        }
        self.r.pick(STRS).into()
    }

    fn st(&mut self, d: u32) -> String {
        if d == 0 || self.r.pct(30) {
            return self.st_leaf();
        }
        let d = d - 1;
        match self.r.below(18) {
            0 => format!("String({})", self.num(d)),
            1 => format!("({}).toFixed({})", self.num(d), self.r.below(21)),
            2 => format!("({}).toPrecision({})", self.num(d), self.r.range(1, 21)),
            3 => format!("({}).toExponential({})", self.num(d), self.r.below(21)),
            4 => format!("({}).toString({})", self.num(d), self.r.range(2, 36)),
            5 | 6 => format!("({} + {})", self.st(d), self.st(d)),
            7 => format!("`<${{{}}}|${{{}}}>`", self.num(d), self.st(d)),
            8 => {
                let (x, i, j) = (self.st(d), self.num(d), self.num(d));
                match self.r.below(4) {
                    0 => format!("({x}).slice(({i}) | 0, ({j}) | 0)"),
                    1 => format!("({x}).substring(({i}) | 0, ({j}) | 0)"),
                    2 => format!("(({x}).at(({i}) | 0) ?? '')"),
                    _ => format!("({x}).slice(({i}) | 0)"),
                }
            }
            9 => {
                let x = self.st(d);
                let m = self.r.pick(&[
                    ".toUpperCase()",
                    ".toLowerCase()",
                    ".trim()",
                    ".trimStart()",
                    ".normalize('NFD')",
                    ".normalize('NFKC')",
                    "",
                ]);
                match m {
                    "" => format!("[...({x})].reverse().join('')"),
                    m => format!("({x}){m}"),
                }
            }
            10 => {
                let pad = self.r.pick(&["padStart", "padEnd"]);
                format!("({}).{pad}(({}) & 63, {})", self.st(d), self.num(d), self.st(d))
            }
            11 => format!("({}).repeat(({}) & 7)", self.st(d), self.num(d)),
            12 => format!("({}).split({}).join({})", self.st(d), self.st(d), self.st(d)),
            13 => {
                let (x, re) = (self.st(d), self.r.pick(REGEXES));
                match self.r.below(3) {
                    0 => format!("({x}).replace({re}, {})", self.st(d)),
                    1 => format!("({x}).replace({re}, (m, ...g) => m.length + '/' + g.length)"),
                    _ => format!("({x}).replace({re}, '[$&|$1]')"),
                }
            }
            14 if !self.literal => format!("JSON.stringify({})", self.obj(d)),
            15 => match self.r.pct(50) {
                true => format!("String.fromCharCode(({}) & 65535, ({}) & 65535)", self.num(d), self.num(d)),
                false => format!("String.fromCodePoint(({}) & 1114111)", self.num(d)),
            },
            16 if !self.literal => match self.r.below(6) {
                0 => format!("A.join({})", self.st(d)),
                1 => "Object.keys(O).join()".into(),
                2 => format!("typeof ({})", self.num(d)),
                3 => format!("(({}).match({}) ?? []).join('|')", self.st(d), self.r.pick(REGEXES)),
                4 => format!("({}).replaceAll({}, {})", self.st(d), self.st(d), self.st(d)),
                _ => format!("[{}, {}, {}].sort().join('|')", self.st(d), self.st(d), self.st(d)),
            },
            _ => format!("h({})", self.num(d)),
        }
    }

    fn obj(&mut self, d: u32) -> String {
        match self.r.below(6) {
            0 => format!(
                "{{ a: {}, s: {}, arr: A.slice(0, 4), n: null, o: {{ x: {} }} }}",
                self.num(d),
                self.st(d),
                self.num(d)
            ),
            1 => "O".into(),
            2 => format!("[{}, {}, true, null]", self.num(d), self.st(d)),
            3 => format!("new Pt({}, {})", self.num(d), self.num(d)),
            4 => format!("{{ ...O, z: {} }}", self.num(d)),
            _ => "Object.fromEntries(M)".into(),
        }
    }

    fn cond(&mut self) -> String {
        let (x, c, y) = (self.n(), self.r.pick(CMPS), self.n());
        format!("({x}) {c} ({y})")
    }

    fn block(&mut self) -> String {
        self.depth += 1;
        let mut s = String::new();
        for _ in 0..self.r.range(1, 4) {
            s.push_str(&self.stmt());
        }
        self.depth -= 1;
        s
    }

    fn loop_body(&mut self) -> String {
        self.loops += 1;
        let b = self.block();
        self.loops -= 1;
        b
    }

    fn trips(&mut self) -> u64 {
        self.r.weighted(&[
            (2, 15),
            (5, 20),
            (10, 20),
            (50, 15),
            (300, 12),
            (3000, 10),
            (30000, 6),
            (300000, 2),
        ])
    }

    fn stmt(&mut self) -> String {
        const KINDS: &[(u8, u64)] = &[
            (0, 14),
            (1, 8),
            (2, 8),
            (3, 4),
            (4, 2),
            (5, 8),
            (6, 4),
            (7, 6),
            (8, 5),
            (9, 3),
            (10, 8),
            (11, 14),
            (12, 1),
            (13, 1),
            (20, 6),
            (21, 6),
            (22, 2),
            (23, 2),
            (24, 4),
            (25, 1),
            (26, 1),
            (27, 1),
        ];
        let kind = loop {
            let k = self.r.weighted(KINDS);
            if (k < 20 || self.depth < 3) && (k != 11 || (self.can_await && self.loops == 0)) {
                break k;
            }
        };
        let k = self.fresh();
        match kind {
            0 => format!("P({});\n", self.n()),
            1 => format!("P({});\n", self.s()),
            2 => {
                let v = self.r.pick(&["a", "b", "c", "d"]);
                match self.r.below(3) {
                    0 => format!("{v} = {};\n", self.n()),
                    1 => format!("{v} {}= {};\n", self.r.pick(BINOPS), self.n()),
                    _ => format!("{v}++;\n"),
                }
            }
            3 => {
                let v = self.r.pick(&["s", "t"]);
                let op = self.r.pick(&["=", "+="]);
                format!("{v} {op} {}; if ({v}.length > 4096) {v} = {v}.slice(-64);\n", self.s())
            }
            4 => format!("print({}, {}, {});\n", self.n(), self.s(), self.obj(1)),
            5 => self.arrays(),
            6 => self.maps(),
            7 => self.typed(),
            8 => self.bigint(),
            9 => {
                let x = self.n();
                match self.r.below(4) {
                    0 => {
                        format!("P(Array.from(new Uint8Array(new Float64Array([{x}]).buffer)).join('.'));\n")
                    }
                    1 => format!(
                        "P(Array.from(new Uint32Array(new Float32Array([{x}, {}]).buffer)).join('.'));\n",
                        self.n()
                    ),
                    2 => format!("P(new BigInt64Array(new Float64Array([{x}]).buffer)[0]);\n"),
                    _ => format!(
                        "{{ const v{k} = {x}; P(Number(String(v{k})) === v{k} || v{k} !== v{k}); P([v{k}.toFixed(2), v{k}.toExponential(3), v{k}.toPrecision(7), (v{k} | 0).toString(2), Math.round(v{k} * 100) / 100]); }}\n"
                    ),
                }
            }
            10 => self.misc(k),
            11 => self.tool(k),
            12 => self.hog(k),
            13 => format!("if ({}) throw new {}({});\n", self.cond(), self.r.pick(ERRORS), self.s()),
            20 => {
                let c = self.cond();
                let (x, y) = (self.block(), self.block());
                format!("if ({c}) {{\n{x}}} else {{\n{y}}}\n")
            }
            21 => {
                let n = self.trips();
                match self.r.below(4) {
                    0 | 1 => {
                        let head = match self.r.pct(70) {
                            true => format!("for (let i{k} = 0; i{k} < {n}; i{k}++)"),
                            false => format!("for (let i{k} = {n}; i{k}-- > 0;)"),
                        };
                        self.nvars.push(format!("i{k}"));
                        let b = self.loop_body();
                        self.nvars.pop();
                        format!("{head} {{\n{b}}}\n")
                    }
                    2 => {
                        self.nvars.push(format!("i{k}"));
                        let b = self.loop_body();
                        self.nvars.pop();
                        format!("for (const i{k} of A.slice(0, 8)) {{\n{b}}}\n")
                    }
                    _ => {
                        let s = self.s();
                        self.svars.push(format!("ch{k}"));
                        let b = self.loop_body();
                        self.svars.pop();
                        format!("for (const ch{k} of ({s}).slice(0, 64)) {{\n{b}}}\n")
                    }
                }
            }
            22 => {
                let n = self.trips();
                self.nvars.push(format!("w{k}"));
                let b = self.loop_body();
                self.nvars.pop();
                format!("{{ let w{k} = 0; while (true) {{\n{b}if (++w{k} >= {n}) break; }} }}\n")
            }
            23 => {
                let x = self.n();
                let (p, q, r, s) = (self.block(), self.block(), self.block(), self.block());
                format!(
                    "switch (({x}) & 3) {{\ncase 0: {{\n{p}}} break;\ncase 1: {{\n{q}}}\ncase 2: {{\n{r}}} break;\ndefault: {{\n{s}}}\n}}\n"
                )
            }
            24 => {
                let throw = match self.r.below(5) {
                    0 => format!("if ({}) throw new {}({});\n", self.cond(), self.r.pick(ERRORS), self.s()),
                    1 => "P(O.missing.deep);\n".into(),
                    2 => format!("P(undeclared{k});\n"),
                    3 => format!("if ({}) null.f();\n", self.cond()),
                    _ => String::new(),
                };
                let (x, y) = (self.block(), self.block());
                let fin = match self.r.pct(50) {
                    true => format!(" finally {{\n{}}}", self.block()),
                    false => String::new(),
                };
                format!(
                    "try {{\n{x}{throw}}} catch (e) {{\nP('E:' + h(e && e.name) + ':' + h(e && e.message));\n{y}}}{fin}\n"
                )
            }
            25 => {
                let (n, m) = (self.trips().min(3000), self.trips().min(300));
                self.nvars.push(format!("i{k}"));
                self.nvars.push(format!("j{k}"));
                let (c1, c2) = (self.cond(), self.cond());
                let b = self.loop_body();
                self.nvars.truncate(self.nvars.len() - 2);
                format!(
                    "L{k}: for (let i{k} = 0; i{k} < {n}; i{k}++) {{ for (let j{k} = 0; j{k} < {m}; j{k}++) {{\nif ({c1}) continue L{k};\nif ({c2}) break L{k};\n{b}}} }}\n"
                )
            }
            26 => {
                let was = std::mem::replace(&mut self.can_await, false);
                self.nvars.push("x".into());
                self.svars.push("y".into());
                let b = self.block();
                let ret = self.n();
                self.nvars.pop();
                self.svars.pop();
                self.can_await = was;
                let (p, q, r) = (self.n(), self.s(), self.n());
                format!(
                    "{{ const f{k} = (x, y) => {{\n{b}return {ret};\n}}; P(f{k}({p}, {q})); P(f{k}({r}, 'z')); }}\n"
                )
            }
            _ => {
                let n = self.trips().min(3000);
                self.nvars.push(format!("w{k}"));
                let b = self.loop_body();
                self.nvars.pop();
                format!("{{ let w{k} = 0; do {{\n{b}}} while (++w{k} < {n}); }}\n")
            }
        }
    }

    fn arrays(&mut self) -> String {
        match self.r.below(12) {
            0 | 1 => format!("A.push({}); if (A.length > 64) A.length = 8;\n", self.n()),
            2 => "A.sort((x, y) => (x > y) - (x < y));\n".into(),
            3 => self.r.pick(&["A.sort();\n", "A.reverse();\n", "A = A.filter((x) => x === x);\n"]).into(),
            4 => {
                self.nvars.push("x".into());
                self.nvars.push("k".into());
                let e = self.n();
                self.nvars.truncate(self.nvars.len() - 2);
                format!("A = A.map((x, k) => {e});\n")
            }
            5 => format!("P(A.slice(({}) & 7).join());\n", self.n()),
            6 => format!("P([A.indexOf({}), A.includes(NaN), A.lastIndexOf(-0)]);\n", self.n()),
            7 => "P(A.reduce((p, x) => p + x, 0));\n".into(),
            8 => format!(
                "A.splice(({}) & 7, 1, {}, {}); if (A.length > 64) A.length = 8;\n",
                self.n(),
                self.n(),
                self.n()
            ),
            9 => format!(
                "{{ const Z = Array.from({{ length: ({}) & 1023 }}, (_, k) => (k * {}) % 1009); Z.sort((x, y) => x - y); P(Z[({}) & 7]); P(Z.length); }}\n",
                self.n(),
                self.r.range(1, 100000),
                self.n()
            ),
            10 => format!("P([[1, [2, [3, [4]]]], A.slice(0, 2), {}].flat(2));\n", self.n()),
            _ => "P([...new Set(A)].length); P(Object.entries({ ...A.slice(0, 3) }));\n".into(),
        }
    }

    fn maps(&mut self) -> String {
        match self.r.below(8) {
            0 => format!("O[{}] = {}; if (Object.keys(O).length > 32) O = {{ n: 1 }};\n", self.s(), self.n()),
            1 => format!("delete O[{}]; P(Object.keys(O).length);\n", self.s()),
            2 => format!("P(O[{}]); P(({}) in O);\n", self.s(), self.s()),
            3 => format!(
                "M.set({}, {}); P(M.get({})); P(M.size); if (M.size > 64) M.clear();\n",
                self.n(),
                self.s(),
                self.n()
            ),
            4 => format!(
                "St.add({}); St.add({}); P(St.has({})); P(St.size); if (St.size > 64) St.clear();\n",
                self.n(),
                self.s(),
                self.n()
            ),
            5 => "P(Object.fromEntries(M)); P([...St].slice(0, 8));\n".into(),
            6 => format!("P(JSON.parse(JSON.stringify({})));\n", self.obj(2)),
            _ => {
                let j = self.r.pick(JSONS);
                format!("try {{ P(JSON.parse({j})); }} catch (e) {{ P(e.name); }}\n")
            }
        }
    }

    fn typed(&mut self) -> String {
        let b = self.r.pick(BOOLS);
        match self.r.below(14) {
            0 => format!("F[({}) & 15] = {};\n", self.n(), self.n()),
            1 => format!("I[({}) & 15] = {};\n", self.n(), self.n()),
            2 => format!("U8[({}) & 31] = {};\n", self.n(), self.n()),
            3 => format!("new Uint8ClampedArray(U8.buffer)[({}) & 31] = {};\n", self.n(), self.n()),
            4 => format!("DV.setFloat64(({}) & 24, {}, {b});\n", self.n(), self.n()),
            5 => format!("DV.setInt16(({}) & 30, {});\n", self.n(), self.n()),
            6 => format!("DV.setFloat32(({}) & 28, {}, {b});\n", self.n(), self.n()),
            7 => format!(
                "DV.setBigInt64(({}) & 24, B, {b}); P(DV.getBigUint64(({}) & 24, {b}));\n",
                self.n(),
                self.n()
            ),
            8 => self
                .r
                .pick(&["P(Array.from(I).join());\n", "P(F.join());\n", "P(Array.from(U8).join());\n"])
                .into(),
            9 => self
                .r
                .pick(&["I.sort(); P(I.join());\n", "F.sort(); P(F.join());\n", "F.reverse();\n"])
                .into(),
            10 => format!(
                "P(F.indexOf({})); P(I.subarray(({}) & 7, 12).reduce((x, y) => x + y, 0));\n",
                self.n(),
                self.n()
            ),
            11 => format!(
                "U8.set(I.subarray(0, 8), ({}) & 15); F.fill({}, ({}) & 15);\n",
                self.n(),
                self.n(),
                self.n()
            ),
            12 => format!("I.copyWithin(({}) & 15, ({}) & 15); P(I.join());\n", self.n(), self.n()),
            _ => format!(
                "try {{ P(Atomics.add(I, ({}) & 15, ({}) | 0)); P(Atomics.load(I, 0)); }} catch (e) {{ P(e.name); }}\n",
                self.n(),
                self.n()
            ),
        }
    }

    fn bigint(&mut self) -> String {
        let bl = self.r.pick(BIGS);
        match self.r.below(10) {
            0 | 1 => format!(
                "B = BigInt.asIntN(64 + (({}) & 63), B * {bl} + BigInt(({}) | 0));\n",
                self.n(),
                self.n()
            ),
            2 => format!("P(B / ({bl} || 1n)); P(B % ({bl} || 7n));\n"),
            3 => format!("P(B ** BigInt(({}) & 15));\n", self.n()),
            4 => format!("P(B << BigInt(({}) & 63)); P(B >> 3n);\n", self.n()),
            5 => format!("P(BigInt.asUintN(64, B)); P(B.toString((({}) & 31) + 2));\n", self.n()),
            6 => format!("P(B & {bl}); P(B ^ {bl}); P(-B);\n"),
            7 => format!("P(BigInt(({}) | 0) * B);\n", self.n()),
            8 => format!("P(BigInt(String(({}) | 0)) === BigInt(({}) | 0));\n", self.n(), self.n()),
            _ => format!("P([B > {}, B == {}, typeof (B * {bl})]);\n", self.n(), self.n()),
        }
    }

    fn misc(&mut self, k: u32) -> String {
        match self.r.below(22) {
            0 => format!(
                "{{ const {{ x, y = {}, ...rest }} = {{ x: {}, z: {}, w: [1, 2] }}; P(x); P(y); P(rest); }}\n",
                self.n(),
                self.n(),
                self.s()
            ),
            1 => format!("{{ const [p, , q = {}, ...r] = A; P(p); P(q); P(r.length); }}\n", self.n()),
            2 => format!(
                "P(String(new Pt({}, {}).add(new Pt({}, {}))));\n",
                self.n(),
                self.n(),
                self.n(),
                self.n()
            ),
            3 => format!(
                "{{ const px = new Proxy(O, {{ get: (o, key) => (key in o ? o[key] : String(key).length), has: () => true }}); P(px[{}]); P('zz' in px); }}\n",
                self.s()
            ),
            4 => "Object.defineProperty(O, 'g', { get() { return a * 2; }, configurable: true }); P(O.g);\n"
                .into(),
            5 => format!("try {{ P(eval({})); }} catch (e) {{ P(e.name); }}\n", quote(&self.n())),
            6 => format!("P(fib(({}) & 15));\n", self.n()),
            7 => {
                let n = self.r.pick(&["100", "3000", "100000", "2000000"]);
                format!("try {{ P(deep({n})); }} catch (e) {{ P(e.name); }}\n")
            }
            8 => format!(
                "{{ let z = 0; for (const v of gen(({}) & 255)) z += v; P(z); P([...gen(({}) & 31)].join('')); }}\n",
                self.n(),
                self.n()
            ),
            9 => format!(
                "P(new Date(Date.UTC(2000 + (({}) & 63), ({}) & 11, (({}) & 27) + 1, ({}) & 23, ({}) & 59)).toISOString());\n",
                self.n(),
                self.n(),
                self.n(),
                self.n(),
                self.n()
            ),
            10 => format!("P(Date.parse({}));\n", self.s()),
            11 => format!(
                "try {{ P(new Date({}).toISOString()); }} catch (e) {{ P(e.name); }} P(new Date({}).toUTCString());\n",
                self.n(),
                self.n()
            ),
            12 => format!(
                "P(typeof ({}) + typeof {} + (A instanceof Array) + Array.isArray(O));\n",
                self.n(),
                self.s()
            ),
            13 => {
                let (p, q, r) = (self.n(), self.n(), self.s());
                format!(
                    "{{ const sy = Symbol({r}); const o = {{ [sy]: {p}, [Symbol.toPrimitive]: (hint) => hint === 'number' ? {q} : {r} }}; P(+o); P(`${{o}}`); P(o[sy]); P(sy.description); }}\n"
                )
            }
            14 => {
                format!("{{ const q = new Q(); P(q.inc({})); P(q.inc({})); P(Q.n); }}\n", self.n(), self.n())
            }
            15 => format!(
                "{{ const wm = new WeakMap(); const key = {{}}; wm.set(key, {}); P(wm.get(key)); }}\n",
                self.n()
            ),
            16 => format!("P(((...xs) => xs.length + xs[0])({}, {}, {}));\n", self.n(), self.n(), self.n()),
            17 => format!("P(O?.missing?.deep ?? {});\n", self.n()),
            18 => format!(
                "P([{}, {}, {}].sort((x, y) => x.localeCompare(y)).join('|'));\n",
                self.s(),
                self.s(),
                self.s()
            ),
            19 => {
                let n = self.trips().min(3000);
                format!(
                    "{{ const mk = (n) => {{ let z = n; return () => (z = (z * 31 + 7) % 1000003); }}; const g{k} = mk(({}) | 0); for (let i = 0; i < {n}; i++) g{k}(); P(g{k}()); }}\n",
                    self.n()
                )
            }
            20 => {
                let re = self.r.pick(REGEXES);
                format!(
                    "{{ const re = new RegExp({re}.source, {re}.flags.includes('g') ? {re}.flags : {re}.flags + 'g'); let m, n = 0; while ((m = re.exec({})) && n++ < 50) P(m.index + ':' + m[0]); }}\n",
                    self.s()
                )
            }
            _ => format!(
                "try {{ P(new Function('x', {})(1)); }} catch (e) {{ P(e.name); }}\n",
                quote(&format!("return {}", self.n()))
            ),
        }
    }

    fn tool(&mut self, k: u32) -> String {
        let tn = self.r.pick(TOOLS);
        match self.r.below(9) {
            0 | 1 => format!("P(await tool('{tn}', {}));\n", self.n()),
            2 => format!(
                "{{ const [x{k}, y{k}] = await Promise.all([tool('{tn}', {}), tool('{}', {})]); P(x{k}); P(y{k}); }}\n",
                self.n(),
                self.r.pick(TOOLS),
                self.s()
            ),
            3 => format!(
                "try {{ P(await tool('fail', {})); }} catch (e) {{ P('caught:' + h(e)); }}\n",
                self.n()
            ),
            4 => format!(
                "P((await Promise.allSettled([tool('fail', 1), tool('{tn}', 2)])).map((r) => r.status).join());\n"
            ),
            5 => format!("P(await twice({}));\n", self.n()),
            6 => format!(
                "{{ const L{k} = await tool('log', 0); P(typeof L{k} === 'string' ? L{k}.split('\\n').filter((l) => /FAIL|error/.test(l)).length : -1); }}\n"
            ),
            7 => format!("P(await (({}) > 0 ? tool('{tn}', 1) : Promise.resolve(7)));\n", self.n()),
            _ => format!(
                "{{ const q{k} = []; Promise.resolve().then(() => q{k}.push('a')).then(() => q{k}.push('b')); await null; q{k}.push('c'); await null; await null; P(q{k}.join()); P(await tool('{tn}', {})); }}\n",
                self.obj(1)
            ),
        }
    }

    fn hog(&mut self, k: u32) -> String {
        match self.r.below(3) {
            0 => {
                let n = self.r.pick(&["1000", "50000", "500000"]);
                format!(
                    "{{ const big{k} = []; for (let i = 0; i < {n}; i++) big{k}.push({{ i, s: 'x'.repeat(i & 63) }}); P(big{k}.length); }}\n"
                )
            }
            1 => format!(
                "try {{ P('ab'.repeat(1 << (({}) & 31)).length); }} catch (e) {{ P(e.name); }}\n",
                self.n()
            ),
            _ => format!(
                "try {{ const ab = new ArrayBuffer(({}) & 0x3ffffff); P(ab.byteLength); }} catch (e) {{ P(e.name); }}\n",
                self.n()
            ),
        }
    }
}

/// `s` as a JavaScript string literal.
fn quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}
