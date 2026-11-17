//! The code tool's S0 spike (`.sb/codemode-compute-plan.md` §5): QuickJS, built by `xtask/quickjs/build.sh`,
//! loaded and run on tarae-wasm through `start`/`resume`, with one stub binding (`tool(name, arg)`), measured
//! under the JIT and under the interpreter.
//!
//! ```text
//! cargo run --release -p tarae-wasm --example qjs_spike -- <quickjs.wasm>
//! ```
#![allow(clippy::disallowed_methods, reason = "a dev measure, not in the release binary")]

use std::time::{Duration, Instant};

use tarae_wasm::{Instance, Limits, Module, Trap, Val};

/// Enough for anything here but the endless loop, which gets the plugin host's budgets.
const FUEL: u64 = 1_000_000_000_000;
const JIT_FUEL: u64 = 2_000_000_000;
const INTERPRETER_FUEL: u64 = 500_000_000;
const MEMORY: usize = 64 << 20;
const LINES: usize = 20_000;

fn main() {
    let path = std::env::args().nth(1).expect("usage: qjs_spike <quickjs.wasm>");
    let bytes = std::fs::read(&path).expect("read the module");
    println!("module {path}: {} bytes", bytes.len());
    let imports = imports(&bytes);
    if !imports.is_empty() {
        println!("it imports {}:", imports.len());
        for i in &imports {
            println!("  {i}");
        }
        return;
    }
    println!("zero imports");
    // compiled code nests on this thread's stack (`Limits::call_depth`)
    let t = std::thread::Builder::new().stack_size(64 << 20);
    t.spawn(move || {
        for jit in [true, false] {
            measure(&bytes, jit);
        }
    })
    .unwrap()
    .join()
    .unwrap();
}

/// The import section's entries, `module.name`, read by hand (the runtime refuses the module with no list).
fn imports(b: &[u8]) -> Vec<String> {
    fn leb(b: &[u8], at: &mut usize) -> u32 {
        let (mut v, mut shift) = (0u32, 0);
        loop {
            let byte = b[*at];
            *at += 1;
            v |= ((byte & 0x7f) as u32) << shift;
            if byte & 0x80 == 0 {
                return v;
            }
            shift += 7;
        }
    }
    fn name(b: &[u8], at: &mut usize) -> String {
        let n = leb(b, at) as usize;
        let s = String::from_utf8_lossy(&b[*at..*at + n]).into_owned();
        *at += n;
        s
    }
    let mut at = 8;
    let mut out = Vec::new();
    while at < b.len() {
        let id = b[at];
        at += 1;
        let len = leb(b, &mut at) as usize;
        let end = at + len;
        if id == 2 {
            let mut p = at;
            for _ in 0..leb(b, &mut p) {
                let (m, n) = (name(b, &mut p), name(b, &mut p));
                let kind = b[p];
                p += 1;
                match kind {
                    0 => drop(leb(b, &mut p)),
                    1 => {
                        p += 1;
                        let flags = leb(b, &mut p);
                        leb(b, &mut p);
                        if flags & 1 == 1 {
                            leb(b, &mut p);
                        }
                    }
                    2 => {
                        let flags = leb(b, &mut p);
                        leb(b, &mut p);
                        if flags & 1 == 1 {
                            leb(b, &mut p);
                        }
                    }
                    _ => p += 2,
                }
                let k = ["func", "table", "memory", "global"].get(kind as usize).unwrap_or(&"?");
                out.push(format!("{m}.{n} ({k})"));
            }
        }
        at = end;
    }
    out
}

struct Engine {
    inst: Instance,
    alloc: u32,
    start: u32,
    resume: u32,
}

/// A reply: its header line (`pending 0:name 1:name`, `done`, `error`) and its JSON.
struct Reply {
    head: String,
    body: String,
}

impl Engine {
    fn new(bytes: &[u8], jit: bool) -> (Engine, Duration, Duration) {
        let t = Instant::now();
        let module = Module::new(bytes).expect("the runtime takes it");
        let compile = t.elapsed();
        let f = |n: &str| module.func(n).unwrap_or_else(|| panic!("no export {n}"));
        let (alloc, start, resume, init) = (f("alloc"), f("start"), f("resume"), f("_initialize"));
        let t = Instant::now();
        let limits = Limits { memory_bytes: MEMORY, jit, ..Limits::default() };
        let mut inst = Instance::new(module, limits, FUEL).expect("instantiated");
        inst.call(init, &[]).expect("_initialize");
        (Engine { inst, alloc, start, resume }, compile, t.elapsed())
    }

    fn put(&mut self, text: &[u8]) -> Result<i32, Trap> {
        let p = self.call(self.alloc, &[Val::I32(text.len() as i32)])?;
        self.inst.memory_mut()[p as usize..p as usize + text.len()].copy_from_slice(text);
        Ok(p)
    }

    fn call(&mut self, f: u32, args: &[Val]) -> Result<i32, Trap> {
        match self.inst.call(f, args)?[..] {
            [Val::I32(v)] => Ok(v),
            ref other => panic!("unexpected results {other:?}"),
        }
    }

    fn reply(&self, p: i32) -> Reply {
        let m = self.inst.memory();
        let p = p as usize;
        let len = u32::from_le_bytes(m[p..p + 4].try_into().unwrap()) as usize;
        let text = String::from_utf8_lossy(&m[p + 4..p + 4 + len]).into_owned();
        let (head, body) = text.split_once('\n').unwrap_or((&text, ""));
        Reply { head: head.to_string(), body: body.to_string() }
    }

    fn start(&mut self, src: &str) -> Result<Reply, Trap> {
        let p = self.put(src.as_bytes())?;
        // (no step limit: -1 is u32::MAX steps — the fuel is what this measures)
        let r = self.call(self.start, &[Val::I32(p), Val::I32(src.len() as i32), Val::I32(-1)])?;
        Ok(self.reply(r))
    }

    fn resume(&mut self, id: i32, answer: &str, err: bool) -> Result<Reply, Trap> {
        let p = self.put(answer.as_bytes())?;
        let args = [Val::I32(id), Val::I32(p), Val::I32(answer.len() as i32), Val::I32(err as i32)];
        let r = self.call(self.resume, &args)?;
        Ok(self.reply(r))
    }

    fn fuel_used<T>(&mut self, budget: u64, f: impl FnOnce(&mut Engine) -> T) -> (T, u64, Duration) {
        self.inst.set_fuel(budget);
        let t = Instant::now();
        let out = f(self);
        (out, budget - self.inst.fuel(), t.elapsed())
    }
}

/// The stub binding: `output` gives the 20,000-line text; anything else, a small object.
fn answer(name: &str, text: &str) -> String {
    match name {
        "output" => json_string(text),
        _ => format!("{{\"name\":{},\"ok\":true}}", json_string(name)),
    }
}

fn json_string(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Runs a program to its end, answering every call: the last reply, the rounds, the time in `resume`.
fn drive(e: &mut Engine, src: &str, text: &str) -> Result<(Reply, usize, Duration), Trap> {
    let mut r = e.start(src)?;
    let (mut rounds, mut in_resume) = (0, Duration::ZERO);
    let mut out: Vec<(i32, String)> = Vec::new();
    while r.head.starts_with("pending") {
        rounds += 1;
        out.extend(r.head.split(' ').skip(1).map(|c| {
            let (id, name) = c.split_once(':').unwrap();
            (id.parse().unwrap(), name.to_string())
        }));
        let Some((id, name)) = (!out.is_empty()).then(|| out.remove(0)) else { break };
        let a = answer(&name, text);
        let t = Instant::now();
        r = e.resume(id, &a, false)?;
        in_resume += t.elapsed();
    }
    Ok((r, rounds, in_resume))
}

/// A test run's output: mostly passes, some failures, warnings and errors pointing at files.
fn test_log(lines: usize) -> String {
    let mut s: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let mut out = String::new();
    let mut n = 0;
    while n < lines {
        let r = next();
        let file =
            format!("src/{}/m{}.rs", ["chat", "editor", "sandbox", "plugin"][(r % 4) as usize], r % 37);
        match r % 100 {
            0..=79 => out.push_str(&format!("test {}::t{} ... ok\n", &file[4..file.len() - 3], r % 9973)),
            80..=84 => {
                out.push_str(&format!("test {}::t{} ... FAILED\n", &file[4..file.len() - 3], r % 9973))
            }
            85..=94 => {
                out.push_str(&format!(
                    "warning: unused variable: `v{}`\n   --> {file}:{}:9\n",
                    r % 50,
                    r % 900
                ));
                n += 1;
            }
            _ => {
                out.push_str(&format!(
                    "error[E0{}]: mismatched types\n   --> {file}:{}:5\n",
                    300 + r % 9,
                    r % 900
                ));
                n += 1;
            }
        }
        n += 1;
    }
    out
}

const FILTER: &str = r#"const text = await tool("output", "run#1");
const lines = text.split("\n");
const failed = lines.filter(l => l.endsWith(" ... FAILED")).map(l => l.slice(5, -10));
const byFile = new Map();
const kinds = {};
for (const l of lines) {
  const at = /^\s+--> ([^:]+):(\d+):\d+$/.exec(l);
  if (at) byFile.set(at[1], (byFile.get(at[1]) ?? 0) + 1);
  const k = /^(error|warning)(\[E\d+\])?:/.exec(l);
  if (k) kinds[k[1] + (k[2] ?? "")] = (kinds[k[1] + (k[2] ?? "")] ?? 0) + 1;
}
print(`${lines.length} lines, ${failed.length} failed`);
return { failed: failed.slice(0, 3), top: [...byFile].sort((a, b) => b[1] - a[1]).slice(0, 3), kinds };"#;

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn cut(s: &str, n: usize) -> String {
    let one: String = s.replace('\n', "⏎");
    match one.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", &one[..i]),
        None => one,
    }
}

fn say(what: &str, r: &Result<(Reply, usize, Duration), Trap>) {
    match r {
        Ok((r, rounds, _)) => println!("  {what}: {} (rounds {rounds}) {}", r.head, cut(&r.body, 300)),
        Err(t) => println!("  {what}: trap: {t}"),
    }
}

fn measure(bytes: &[u8], jit: bool) {
    println!("\n== {}", if jit { "JIT" } else { "interpreter" });
    let (mut e, compile, init) = Engine::new(bytes, jit);
    println!(
        "  load: Module::new {compile:.1?} (compiled to machine code: {}), instance + _initialize {init:.1?}",
        e.inst.module().compiled()
    );
    if jit && !e.inst.module().compiled() {
        println!("  (the JIT declined the module: everything below under JIT runs on the interpreter)");
    }
    let log = test_log(LINES);
    println!("  text: {} lines, {} bytes", log.lines().count(), log.len());

    // a program from nothing: a fresh runtime and context, compile, run
    let trivial: Vec<_> = (0..7)
        .map(|_| {
            let (r, fuel, t) = e.fuel_used(FUEL, |e| drive(e, "return 1 + 1", ""));
            assert!(r.as_ref().is_ok_and(|r| r.0.body.starts_with(r#"{"done":2,"printed":"","#)));
            (t, fuel)
        })
        .collect();
    println!(
        "  start `return 1 + 1` (a fresh runtime in the same instance): median {:.2?}, fuel {}",
        median(trivial.iter().map(|t| t.0).collect()),
        trivial[0].1
    );
    let cold: Vec<_> = (0..3)
        .map(|_| {
            let t = Instant::now();
            let (mut e, _, _) = Engine::new(bytes, jit);
            drive(&mut e, "return 1 + 1", "").unwrap();
            t.elapsed()
        })
        .collect();
    println!("  start cold (Module::new + instance + _initialize + program): median {:.1?}", median(cold));

    // parse and filter the 20,000-line text, handed in as the stub binding's answer
    let mut times = Vec::new();
    let mut resumes = Vec::new();
    let mut last = None;
    for _ in 0..5 {
        let (r, fuel, t) = e.fuel_used(FUEL, |e| drive(e, FILTER, &log));
        let (reply, _, in_resume) = r.as_ref().expect("no trap");
        times.push(t);
        resumes.push(*in_resume);
        last = Some((reply.head.clone(), reply.body.clone(), fuel));
    }
    let (head, body, fuel) = last.unwrap();
    println!(
        "  20,000 lines parsed and filtered: median {:.1?} (of which the answer's JSON.parse + the filter, in resume: {:.1?}), fuel {fuel}",
        median(times),
        median(resumes)
    );
    println!("    {head} {}", cut(&body, 400));
    println!("  memory after: {} KB of the {} MB cap", e.inst.memory().len() >> 10, MEMORY >> 20);

    // several calls at once
    let all = r#"const [a, b, c] = await Promise.all([tool("x", 1), tool("y", 2), tool("z", 3)]);
        return [a.name, b.name, c.name];"#;
    let r = e.start(all).unwrap();
    println!("  Promise.all of three: first reply `{}`", r.head);
    say("  answered", &drive(&mut e, all, ""));

    let gone = r#"const out = [];
        for (const f of [() => eval("1"), () => Function("return 1")(), () => (function () {}).constructor("return 1")(),
                         () => (async function () {}).constructor("return 1")(), () => (function* () {}).constructor("yield 1")().next()])
          try { out.push(String(f())); } catch (e) { out.push(e.name + ": " + e.message); }
        return out;"#;
    say("eval and Function", &drive(&mut e, gone, ""));
    say(
        "65 calls",
        &drive(
            &mut e,
            "const p = []; for (let i = 0; i < 65; i++) p.push(tool('x', i)); return (await Promise.all(p)).length;",
            "",
        ),
    );
    say("a type annotation", &drive(&mut e, "const n: number = 1; return n;", ""));
    say("a thrown error", &drive(&mut e, "print('before'); null.x;", ""));
    say("waits on nothing", &drive(&mut e, "await new Promise(() => {});", ""));
    say("deep recursion", &drive(&mut e, "function f(n) { return f(n + 1) + 1; } return f(0);", ""));
    say("a 1 GB string", &drive(&mut e, "const s = 'x'.repeat(1 << 30); return s.length;", ""));
    say("a 512 MB string", &drive(&mut e, "const s = 'x'.repeat(1 << 29); return s.length;", ""));
    let grow = "const a = []; for (;;) a.push('x'.repeat(1 << 20) + a.length);";
    say("growing past memory", &drive(&mut e, grow, ""));
    println!("  memory after: {} KB", e.inst.memory().len() >> 10);
    say("the same instance after", &drive(&mut e, "return 'still here'", ""));

    // an endless loop on the plugin host's budget
    let budget = if jit { JIT_FUEL } else { INTERPRETER_FUEL };
    let (r, fuel, t) = e.fuel_used(budget, |e| drive(e, "for (;;) {}", ""));
    say(&format!("`for (;;) {{}}` on {budget} fuel: after {t:.2?} ({fuel} burnt)"), &r);
    println!(
        "    → {:.2e} fuel a second; 5 s of CPU ≈ {:.1e} fuel",
        fuel as f64 / t.as_secs_f64(),
        5.0 * fuel as f64 / t.as_secs_f64()
    );
    e.inst.set_fuel(FUEL);
    say("the same instance after the trap (fuel given again)", &drive(&mut e, "return 'still here'", ""));

    // fuel against time for real JS loops, and where the filter's time goes (a fresh instance: the trap left
    // QuickJS halfway through)
    let (mut e, _, _) = Engine::new(bytes, jit);
    for (what, src) in [
        ("1e6 empty turns", "for (let i = 0; i < 1e6; i++) {} return 1;"),
        (
            "1e6 turns of arithmetic",
            "let s = 0; for (let i = 0; i < 1e6; i++) s = (s + i * 7) % 1000003; return s;",
        ),
        (
            "the answer only (JSON.parse of 600 KB)",
            "const t = await tool('output', 'run#1'); return t.length;",
        ),
        ("+ split", "const t = await tool('output', 'run#1'); return t.split('\\n').length;"),
        (
            "+ endsWith filter",
            "const t = await tool('output', 'run#1'); return t.split('\\n').filter(l => l.endsWith(' ... FAILED')).length;",
        ),
        (
            "+ one regex a line",
            "const t = await tool('output', 'run#1'); return t.split('\\n').filter(l => /^\\s+--> ([^:]+):(\\d+):\\d+$/.test(l)).length;",
        ),
    ] {
        let mut runs: Vec<(Duration, u64)> = (0..3)
            .map(|_| {
                let (r, fuel, t) = e.fuel_used(FUEL, |e| drive(e, src, &log));
                assert!(
                    r.as_ref().is_ok_and(|r| r.0.head == "done"),
                    "{what}: {}",
                    r.map(|r| r.0.body).unwrap_or_default()
                );
                (t, fuel)
            })
            .collect();
        runs.sort();
        let (t, fuel) = runs[1];
        println!("  {what}: {t:.1?}, fuel {fuel} ({:.2e} a second)", fuel as f64 / t.as_secs_f64());
    }
}
