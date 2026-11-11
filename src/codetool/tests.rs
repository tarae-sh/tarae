//! The host's own logic on a stand-in engine (every machine), then the real engine where `cargo xtask quickjs` built
//! it (else those skip, as a grammar's tests do).

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;

/// A stand-in engine: `start`'s source is a script of replies (separated by 0x1e), given one a step. Two replies
/// act: `!` spins until the fuel runs out, `@` ends with the last answer as the program's value.
const STAND_IN: &str = r#"(module
  (memory (export "memory") 16)
  (global $heap (mut i32) (i32.const 0x10000))
  (global $at (mut i32) (i32.const 0))
  (global $end (mut i32) (i32.const 0))
  (data (i32.const 0x100) "done\0a{\22done\22:")
  (func (export "_initialize"))
  (func (export "alloc") (param $n i32) (result i32) (local $p i32)
    (local.set $p (global.get $heap))
    (global.set $heap (i32.add (local.get $p) (i32.add (local.get $n) (i32.const 1))))
    (local.get $p))
  (func $next (result i32) (local $e i32) (local $n i32)
    (local.set $e (global.get $at))
    (block $b (loop $l
      (br_if $b (i32.ge_u (local.get $e) (global.get $end)))
      (br_if $b (i32.eq (i32.load8_u (local.get $e)) (i32.const 0x1e)))
      (local.set $e (i32.add (local.get $e) (i32.const 1)))
      (br $l)))
    (local.set $n (i32.sub (local.get $e) (global.get $at)))
    (i32.store (i32.const 0x80000) (local.get $n))
    (memory.copy (i32.const 0x80004) (global.get $at) (local.get $n))
    (global.set $at (i32.add (local.get $e) (i32.const 1)))
    (i32.const 0x80000))
  (func $reply (param $ans i32) (param $len i32) (result i32) (local $r i32)
    (local.set $r (call $next))
    (if (i32.and (i32.eq (i32.load (local.get $r)) (i32.const 1))
                 (i32.eq (i32.load8_u offset=4 (local.get $r)) (i32.const 0x21)))
      (then (loop $l (br $l))))
    (if (i32.and (i32.eq (i32.load (local.get $r)) (i32.const 1))
                 (i32.eq (i32.load8_u offset=4 (local.get $r)) (i32.const 0x40)))
      (then
        (memory.copy (i32.const 0x80004) (i32.const 0x100) (i32.const 13))
        (memory.copy (i32.const 0x80011) (local.get $ans) (local.get $len))
        (i32.store8 (i32.add (i32.const 0x80011) (local.get $len)) (i32.const 0x7d))
        (i32.store (i32.const 0x80000) (i32.add (local.get $len) (i32.const 14)))))
    (local.get $r))
  (func (export "start") (param $p i32) (param $n i32) (param $steps i32) (result i32)
    (global.set $at (local.get $p))
    (global.set $end (i32.add (local.get $p) (local.get $n)))
    (call $reply (i32.const 0) (i32.const 0)))
  (func (export "resume") (param $id i32) (param $p i32) (param $n i32) (param $err i32) (result i32)
    (call $reply (local.get $p) (local.get $n))))"#;

fn stand_in() -> Arc<Engine> {
    Arc::new(Engine::new(&wat::parse_str(STAND_IN).unwrap()).unwrap())
}

fn script(replies: &[&str]) -> String {
    replies.join("\x1e")
}

/// The stand-in's limits: little fuel, so `!` ends at once.
const SMALL: Limits = Limits { fuel: 10_000_000, ..LIMITS };

fn ended(n: Next) -> Ended {
    match n {
        Next::Ended(e) => e,
        Next::Calls(_, calls) => panic!("still waiting, on {calls:?}"),
    }
}

fn calls(n: Next) -> (Box<Program>, Vec<Call>) {
    match n {
        Next::Calls(p, c) => (p, c),
        Next::Ended(e) => panic!("ended: {e:?}"),
    }
}

fn error(n: Next) -> String {
    ended(n).result.expect_err("an error")
}

#[test]
fn calls_come_together_and_are_answered_one_by_one() {
    let e = stand_in();
    let src = script(&[
        r#"pending 0:read 1:search
{"pending":[{"id":0,"name":"read","arg":"a.rs"},{"id":1,"name":"search","arg":{"pattern":"x"}}]}"#,
        "pending\n{\"pending\":[]}",
        "@",
    ]);
    let (p, got) = calls(Program::start(&e, &src, SMALL));
    assert_eq!(
        got,
        [
            Call { id: 0, name: "read".into(), arg: json!("a.rs") },
            Call { id: 1, name: "search".into(), arg: json!({"pattern": "x"}) },
        ]
    );
    assert_eq!(p.waiting().collect::<Vec<_>>(), [0, 1]);
    let (p, more) = calls(p.answer(0, &json!("text")));
    assert!(more.is_empty(), "waits on the call still out");
    assert_eq!(p.waiting().collect::<Vec<_>>(), [1]);
    let end = ended(p.answer(1, &json!({"error": "refused at the card"})));
    assert_eq!(
        end.result,
        Ok(Some(json!({"error": "refused at the card"}))),
        "the answer reached the engine whole"
    );
}

#[test]
fn an_engine_that_breaks_the_protocol_ends_the_program_with_words() {
    let e = stand_in();
    let one = |reply: &str| error(Program::start(&e, reply, SMALL));
    let call = |id: u64| json!({"id": id, "name": "x"});
    let pending = |list: Vec<Value>| format!("pending\n{}", json!({ "pending": list }));
    assert_eq!(
        one(""),
        "the engine gave a reply `` that isn't JSON: EOF while parsing a value at line 1 column 0"
    );
    assert_eq!(one("hello\n{}"), "the engine gave a reply `hello` that isn't one");
    assert_eq!(
        one("done\nnot json"),
        "the engine gave a reply `done` that isn't JSON: expected ident at line 1 column 2"
    );
    assert_eq!(one("pending\n{}"), "the engine said pending with no list");
    assert_eq!(one(&pending(vec![])), "the engine waits with no call out");
    assert_eq!(one(&pending(vec![json!({"name": "x"})])), "the engine handed out a call with no id");
    assert_eq!(one(&pending(vec![call(64)])), "the engine handed out a call with no id");
    assert_eq!(one(&pending(vec![json!({"id": 0})])), "the engine handed out a call with no name");
    assert_eq!(one(&pending(vec![call(3), call(3)])), "the engine handed out call 3 again, or past 64 calls");
    // 64 calls in all, then one more in the next step: past the limit, whatever its id
    let all = pending((0..64).map(call).collect());
    let (p, got) = calls(Program::start(&e, &script(&[&all, &pending(vec![call(0)])]), SMALL));
    assert_eq!(got.len(), 64);
    assert_eq!(error(p.answer(0, &json!(1))), "the engine handed out call 0 again, or past 64 calls");
    // the host answering what isn't out is said, not run
    let (p, _) = calls(Program::start(&e, &pending(vec![call(5)]), SMALL));
    assert_eq!(error(p.answer(6, &json!(1))), "the host answered call 6, which isn't waiting");
}

#[test]
fn the_fuel_and_the_step_limit_end_a_program_with_their_words() {
    let e = stand_in();
    for jit in [true, false] {
        let fuel = error(Program::start(&e, "!", Limits { jit, ..SMALL }));
        assert!(fuel.starts_with("the program ran out of fuel, the limit of one program's work"), "{fuel}");
    }
    let (p, _) = calls(Program::start(
        &e,
        &script(&["pending\n{\"pending\":[{\"id\":0,\"name\":\"x\"}]}", "!"]),
        SMALL,
    ));
    assert!(
        error(p.answer(0, &json!(1))).contains("out of fuel"),
        "the fuel is the whole program's, resumes too"
    );
    let steps = r#"error
{"error":"InternalError: interrupted","stopped":"steps","steps":20001,"printed":"before\n","printed_bytes":7}"#;
    let end = ended(Program::start(&e, steps, SMALL));
    assert_eq!(
        end.result,
        Err("the program ran 20001 steps, the limit of one program (a step is 10,000 of its loop turns and branches)".into())
    );
    assert_eq!((end.printed.as_str(), end.steps), ("before\n", 20001));
}

#[test]
fn a_syntax_error_with_type_annotations_says_plain_javascript() {
    let e = stand_in();
    let failed = "error\n{\"error\":\"SyntaxError: missing initializer for const variable\",\"syntax\":true}";
    // (the stand-in never reaches what's after its first reply: the program's text, for the hint to read)
    let src = format!("{failed}\x1e\nconst n: number = 1;");
    assert_eq!(
        error(Program::start(&e, &src, SMALL)),
        "SyntaxError: plain JavaScript: no type annotations (`: number`, `interface`, `as`) — line 3: const n: number = 1;"
    );
    // a runtime error isn't a syntax error, annotations or not; a syntax error without them keeps its words
    let runtime = format!("{}\x1e\nconst n: number = 1;", failed.replace(",\"syntax\":true", ""));
    assert_eq!(
        error(Program::start(&e, &runtime, SMALL)),
        "SyntaxError: missing initializer for const variable"
    );
    assert_eq!(
        error(Program::start(&e, failed, SMALL)),
        "SyntaxError: missing initializer for const variable"
    );
}

#[test]
fn annotations_are_told_from_javascript() {
    for ts in [
        "const n: number = 1;",
        "let xs: string[] = [];",
        "var m: Map<string, number> = new Map();",
        "function f(a: number) { return a; }",
        "const f = (line: string) => line.trim();",
        "function g(a, b: string) {}",
        "function h(a, b?: number) {}",
        "function k(): number { return 1; }",
        "const q = async (): Promise<void> => {};",
        "interface Hit { file: string }",
        "interface Hit<T> extends Base { file: T }",
        "type Row = { n: number };",
        "enum Kind { A, B }",
        "const t = (await tool('x')) as string;",
        "const c = [1, 2] as const;",
        "return x as Hit;",
    ] {
        assert!(annotation(ts).is_some(), "not seen: {ts}");
    }
    for js in [
        "const o = { n: 1, kind: Kind };",
        "const r = a ? b : c;",
        "const s = 'a: number' + \"b as T\";",
        "const t = `type X = ${1}`;",
        "// const n: number\nreturn 1;",
        "/* interface Hit { } */ return 1;",
        "outer: for (const x of xs) { break outer; }",
        "let type = 3; type = 4;",
        "f({ a: 1 }, { b: 'x' });",
        "switch (x) { case 1: return 2; default: return 3; }",
        "const { a: b, c: d } = o;",
        "const alias = 1; const asm = alias;",
    ] {
        assert_eq!(annotation(js), None, "seen in plain JavaScript: {js}");
    }
    assert_eq!(annotation("print(1);\n\nconst n: number = 1;"), Some((3, "const n: number = 1;")));
    assert_eq!(
        annotation("const s = 'it’s: number';\nconst n: number = 1;"),
        Some((2, "const n: number = 1;"))
    );
}

#[test]
fn what_claude_reads_is_cut_at_the_limit_and_says_so() {
    let end = |result, printed: &str| Ended {
        result,
        printed: printed.into(),
        printed_bytes: printed.len() as u64,
        steps: 1,
        fuel: 1,
    };
    assert_eq!(end(Ok(Some(json!(2))), "").text(), "returned: 2");
    assert_eq!(end(Ok(Some(json!("a\nb"))), "x\n").text(), "x\nreturned:\na\nb");
    assert_eq!(end(Ok(None), "").text(), "returned nothing, printed nothing");
    assert_eq!(end(Ok(None), "x").text(), "x\n");
    assert_eq!(end(Err("TypeError: no".into()), "before\n").text(), "before\nerror: TypeError: no");
    let mut over = end(Ok(None), "x\n");
    over.printed_bytes = 5 << 20;
    assert_eq!(over.text(), "x\n… print() kept its first 1024 KB, of 5242880 bytes printed\n");

    let line = "0123456789".repeat(9) + "\n"; // 91 bytes
    let long = end(Ok(Some(json!(7))), &line.repeat(300));
    let whole = long.text();
    let a = long.answer(LIMITS.output);
    assert_eq!((a.whole, a.shown), (whole.len(), 225 * 91), "cut at the last whole line");
    assert!(a.text.starts_with(&line.repeat(225)[..225 * 91 - 1]));
    assert!(
        a.text.ends_with(&format!(
            "\n… cut: the first 20475 of {} bytes — print less, or what you need from it (the user sees it whole)",
            whole.len()
        )),
        "{}",
        &a.text[a.text.len() - 200..]
    );
    let short = end(Ok(Some(json!(7))), &line);
    assert_eq!(
        short.answer(LIMITS.output),
        Answer { text: short.text(), shown: short.text().len(), whole: short.text().len() }
    );
    // no line's end near: cut on a character, never inside one
    let wide = end(Ok(None), &"ㄱ".repeat(10_000));
    let a = wide.answer(LIMITS.output);
    assert_eq!(a.shown, 20478, "{}", a.shown);
}

#[test]
fn a_program_runs_as_jobs_and_lands_on_the_main_loop() {
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    let main = std::thread::current().id();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let src = script(&["pending 0:x\n{\"pending\":[{\"id\":0,\"name\":\"x\"}]}", "@"]);
    let load = {
        let seen = seen.clone();
        move || {
            let on = if std::thread::current().id() == main { "main" } else { "worker" };
            seen.lock().unwrap().push(format!("loaded on a {on}"));
            Ok(stand_in())
        }
    };
    let s = seen.clone();
    start_job(&ed.events.jobs(), load, src, SMALL, move |ed, next| {
        assert_eq!(std::thread::current().id(), main);
        let (p, calls) = calls(next);
        s.lock().unwrap().push(format!("asked {}", calls[0].name));
        let s = s.clone();
        answer_job(&ed.events.jobs(), p, 0, json!([1, 2]), move |_, next| {
            s.lock().unwrap().push(format!("returned {}", ended(next).result.unwrap().unwrap()));
        });
    });
    assert!(seen.lock().unwrap().len() < 2, "nothing ran in the caller: {:?}", seen.lock().unwrap());
    crate::test_results::settle_until(&mut ed, |_| seen.lock().unwrap().len() == 3);
    assert_eq!(*seen.lock().unwrap(), ["loaded on a worker", "asked x", "returned [1,2]"]);
}

// —— the real engine ——

/// The engine `cargo xtask quickjs` built, if it's there.
fn real() -> Option<Arc<Engine>> {
    Path::new(ENGINE_DEV).exists().then(|| Engine::at(Path::new(ENGINE_DEV)).expect("the built engine loads"))
}

/// The tests' limits on the real engine: `LIMITS`, on machine code wherever there is some (the interpreter at
/// opt-level 0 is slow).
const REAL: Limits = Limits { jit: true, ..LIMITS };

/// Runs `src` to its end, answering every call (in the order handed out) with `answer`; the calls each step gave.
fn drive(
    e: &Engine,
    src: &str,
    limits: Limits,
    answer: impl Fn(&Call) -> Value,
) -> (Ended, Vec<Vec<String>>) {
    let mut steps = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    let mut next = Program::start(e, src, limits);
    loop {
        match next {
            Next::Ended(end) => return (end, steps),
            Next::Calls(p, calls) => {
                steps.push(calls.iter().map(|c| format!("{}:{}", c.id, c.name)).collect());
                queue.extend(calls);
                let c = queue.pop_front().expect("something to answer");
                next = p.answer(c.id, &answer(&c));
            }
        }
    }
}

fn ok(e: &Engine, src: &str) -> Ended {
    drive(e, src, REAL, |c| json!({ "name": c.name, "arg": c.arg })).0
}

#[test]
fn the_real_engine_runs_a_program_and_its_calls() {
    let Some(e) = real() else { return };
    let end = ok(&e, "return 1 + 1");
    assert_eq!((end.result, end.printed.as_str()), (Ok(Some(json!(2))), ""));
    assert!(end.steps < 5, "{}", end.steps);
    assert_eq!(ok(&e, "print('x'); return;").result, Ok(None));
    let (end, steps) = drive(
        &e,
        "const [a, b, c] = await Promise.all([tool('x', 1), tool('y', {n: 2}), tool('z')]);\n\
         print(a.arg, b.arg.n); const d = await tool('w', c.name); return d.arg;",
        REAL,
        |c| json!({ "name": c.name, "arg": c.arg }),
    );
    assert_eq!(steps, [vec!["0:x", "1:y", "2:z"], vec![], vec![], vec!["3:w"]], "together, then one by one");
    assert_eq!((end.result, end.printed.as_str()), (Ok(Some(json!("z"))), "1 2\n"));
    let sixty_five = ok(
        &e,
        "const p = []; for (let i = 0; i < 65; i++) p.push(tool('x', i)); return (await Promise.all(p)).length;",
    );
    assert!(sixty_five.result.unwrap_err().starts_with("RangeError: 64 calls is the limit of one program"));
    let evals = ok(
        &e,
        "const out = []; for (const f of [() => eval('1'), () => Function('return 1')(), \
         () => (async function () {}).constructor('return 1')()]) try { f() } catch (x) { out.push(String(x)) } \
         return out;",
    );
    let refused = [
        "ReferenceError: eval is not defined",
        "TypeError: eval is not supported",
        "TypeError: eval is not supported",
    ];
    assert_eq!(evals.result, Ok(Some(json!(refused))), "nothing a program makes becomes code");
}

#[test]
fn the_real_engine_stops_at_its_limits_and_the_next_program_starts_clean() {
    let Some(e) = real() else { return };
    let limit = |src: &str| {
        let end = ok(&e, src);
        (end.result.unwrap_err(), end.steps, end.printed)
    };
    let words = "the program ran 171 steps, the limit of one program (a step is 10,000 of its loop turns and branches)";
    assert_eq!(limit("print('before'); for (;;) {}"), (words.into(), 171, "before\n".into()));
    // the limit can't be caught, nor waited out
    assert_eq!(limit("for (;;) { try { for (;;) {} } catch (x) {} }").0, words);
    assert_eq!(limit("await tool('x'); for (;;) {}").0, words, "the steps are the whole program's");
    // a regex that backtracks forever is work too
    assert_eq!(limit("return /(a+)+$/.test('a'.repeat(40) + 'b')").0, words);
    // a result whose toJSON never ends: the limit, said as the limit
    assert_eq!(limit("return { toJSON() { for (;;) {} } }").0, words);
    let cycle = limit("const o = {}; o.o = o; return o;").0;
    assert_eq!(cycle, "the result can't be turned into JSON");
    // memory
    let big = limit("const s = 'x'.repeat(1 << 30); return s.length;").0;
    assert!(big.starts_with("RangeError: invalid string length"), "{big}");
    let grow = limit("const a = []; for (;;) a.push('x'.repeat(1 << 20) + a.length);").0;
    assert!(grow.starts_with("InternalError: out of memory"), "{grow}");
    assert_eq!(
        ok(&e, "return typeof a + typeof print").result,
        Ok(Some(json!("undefinedfunction"))),
        "nothing left"
    );
}

/// The limits are ordered: a JavaScript loop of up to 20 statements a turn meets the step limit, never the fuel;
/// and the task they're sized by, the 20,000-line triage, meets neither.
#[test]
fn a_loop_meets_the_step_limit_before_the_fuel_and_the_task_meets_neither() {
    let Some(e) = real() else { return };
    let twenty = format!("let s = 0; for (let i = 0; ; i++) {{ {} }}", "s = (s * 31 + i) | 0; ".repeat(20));
    for src in ["for (;;) {}", "let s = 0; for (let i = 0; ; i++) s = (s + i * 7) % 1000003;", &twenty] {
        let end = ok(&e, src);
        assert_eq!(end.steps, LIMITS.steps + 1, "{src}: {:?}", end.result);
        assert!(end.fuel < LIMITS.fuel, "{src}");
    }
    let log = test_log(20_000);
    let triage = drive(&e, MEASURED_TASK, REAL, |_| json!(log)).0;
    assert!(triage.result.is_ok(), "{:?}", triage.result);
    assert!(
        triage.steps * 10 <= LIMITS.steps && triage.fuel * 10 < LIMITS.fuel,
        "{} {}",
        triage.steps,
        triage.fuel
    );
}

#[test]
fn the_real_engine_says_plain_javascript_for_type_annotations() {
    let Some(e) = real() else { return };
    let said = ok(&e, "const hits = 1;\nconst n: number = hits;\nreturn n;").result.unwrap_err();
    assert_eq!(
        said,
        "SyntaxError: plain JavaScript: no type annotations (`: number`, `interface`, `as`) — line 2: const n: number = hits;"
    );
    let plain = ok(&e, "return (1 +;").result.unwrap_err();
    assert!(plain.starts_with("SyntaxError: ") && !plain.contains("plain JavaScript"), "{plain}");
    assert!(plain.contains("program.js:1"), "the line is the program's own: {plain}");
}

#[test]
fn the_real_engine_counts_the_same_steps_under_the_jit_and_the_interpreter() {
    let Some(e) = real() else { return };
    let src = "let s = 0; for (let i = 0; i < 20000; i++) s += i % 7; return s;";
    let [jit, interp] = [true, false].map(|jit| ok_with(&e, src, Limits { jit, ..REAL }));
    assert_eq!(jit, interp, "result, steps and fuel alike");
    assert!(jit.steps > 0, "{jit:?}");
}

fn ok_with(e: &Engine, src: &str, limits: Limits) -> Ended {
    drive(e, src, limits, |_| json!(null)).0
}

#[test]
fn what_the_real_engine_printed_past_the_limit_is_cut_for_claude() {
    let Some(e) = real() else { return };
    let end = ok(&e, "for (let i = 0; i < 4000; i++) print('line ' + i); return 'end';");
    let a = end.answer(LIMITS.output);
    assert!(a.whole > LIMITS.output && a.shown <= LIMITS.output, "{} {}", a.whole, a.shown);
    assert!(a.text.contains("… cut: the first "), "{}", &a.text[a.text.len() - 120..]);
    assert!(end.text().ends_with("line 3999\nreturned:\nend"));
}

/// By hand, the runtime optimized (the rest needn't be):
/// `cargo test --config 'profile.test.package.tarae-wasm.opt-level=3' --bin tarae codetool::tests::measure --
/// --ignored --nocapture`. Under a minute, every case bounded by a small fuel (never by `LIMITS`), or ending by
/// itself:
/// - the 20,000-line triage: its steps and fuel (what `LIMITS.steps` is ten times of);
/// - the fuel a step costs, by the shape of a loop (`FUEL_A_STEP` is the largest of these);
/// - fuel a second where the steps see nothing (C-heavy work, allocating or not), under the JIT and the
///   interpreter;
/// - and from them, for a step limit: the fuel it needs (`fuel_for`), and the longest a program runs on it.
#[test]
#[ignore = "a measure, by hand"]
fn measure() {
    let e = real().expect("cargo xtask quickjs first");
    let log = test_log(20_000);
    const FILTER: &str = MEASURED_TASK;
    // (the steps never end a case here: a small fuel does, or the program itself)
    let on = |fuel: u64| Limits { steps: u32::MAX, fuel, ..LIMITS };
    let straight =
        |n| format!("let s = 0; for (let i = 0; ; i++) {{ {} }}", "s = (s * 31 + i) | 0; ".repeat(n));
    let (twenty, hundred) = (straight(20), straight(100));
    let shapes: [(&str, &str); 8] = [
        ("an empty loop", "for (;;) {}"),
        ("arithmetic", "let s = 0; for (let i = 0; ; i++) s = (s + i * 7) % 1000003;"),
        ("a call a turn", "function f(x) { return x + 1 } let s = 0; for (;;) s = f(s);"),
        ("20 statements a turn", &twenty),
        ("100 statements a turn", &hundred),
        (
            "an object and strings",
            "const o = {}; for (let i = 0; ; i++) { const k = 'k' + (i % 100); o[k] = (o[k] ?? 0) + 1; }",
        ),
        (
            "the triage's loop, for ever",
            "const ls = (await tool('o')).split('\\n'); for (;;) for (const l of ls) { const m = /^test (\\S+) \\.\\.\\. (\\w+)$/.exec(l); if (m && m[2] === 'FAILED') l.length; }",
        ),
        ("a regex's backtracking", "for (;;) /(a+)+$/.test('a'.repeat(40) + 'b');"),
    ];
    let heavy: [(&str, &str); 6] = [
        ("JSON.parse of 600 KB", "const s = JSON.stringify(await tool('o')); for (;;) JSON.parse(s);"),
        (
            "string building (8 MB repeat + replaceAll)",
            "for (;;) 'ab'.repeat(1 << 22).replaceAll('a', 'c').length;",
        ),
        (
            "indexOf over 8 MB (allocates nothing)",
            "const s = 'ab'.repeat(1 << 22); let n = 0; for (;;) n += s.indexOf('c');",
        ),
        (
            "a typed array's fill, 40 MB (allocates nothing)",
            "const a = new Uint8Array(40 << 20); for (let i = 0; ; i++) a.fill(i & 255);",
        ),
        (
            "sorting 200,000 with a comparator",
            "const a = Array.from({length: 200000}, (_, i) => (i * 7919) % 200003); for (;;) a.slice().sort((x, y) => x - y);",
        ),
        (
            "a regex over 20,000 lines",
            "const ls = (await tool('o')).split('\\n'); const re = /^\\s+--> ([^:]+):(\\d+):\\d+$/; for (;;) ls.filter(l => re.test(l)).length;",
        ),
    ];
    let mut task = 0u32;
    let mut a_step = 0f64;
    let mut slowest = [f64::MAX; 2];
    for (k, jit) in [true, false].into_iter().enumerate() {
        println!("== {}", if jit { "JIT" } else { "interpreter" });
        let time = |what: &str, src: &str, limits: Limits| {
            let t = std::time::Instant::now();
            let end = drive(&e, src, Limits { jit, ..limits }, |_| json!(log)).0;
            let t = t.elapsed();
            let (fuel, steps) = (end.fuel as f64, end.steps);
            println!(
                "  {what}: {t:.2?}, {steps} steps, fuel {fuel:.2e} — {:.2e} a second, {:.2e} a step — {}",
                fuel / t.as_secs_f64(),
                fuel / steps.max(1) as f64,
                cut(&end.text().replace('\n', " ⏎ "), 60)
            );
            (end, fuel / t.as_secs_f64(), fuel / steps.max(1) as f64)
        };
        let (end, _, _) = time("the 20,000-line triage", FILTER, on(100_000_000_000));
        assert!(end.result.is_ok(), "{:?}", end.result);
        task = end.steps;
        // (to 5 steps: a fuel trap leaves the count inside the engine; 1e12 fuel bounds a shape that'd cost more)
        println!(" the fuel a step costs, by the loop's shape (to 5 steps, on 1e12 fuel):");
        for (what, src) in shapes {
            let (end, _, per_step) = time(what, src, Limits { steps: 5, ..on(1_000_000_000_000) });
            assert_eq!(end.steps, 6, "{what}: the step limit ended it");
            a_step = a_step.max(per_step);
        }
        println!(" fuel a second where the steps see nothing (on 1e10 fuel):");
        for (what, src) in heavy {
            let (end, rate, _) = time(what, src, on(10_000_000_000));
            assert_eq!(end.steps, 0, "{what}: the steps saw it");
            slowest[k] = slowest[k].min(rate);
        }
    }
    println!(
        "== for a step limit S: fuel F = 2 × S × {a_step:.2e} (the costliest step above); the longest a program"
    );
    println!("   runs on F where the steps see nothing (the slowest rate above)");
    for (what, s) in [
        ("3× the task", 3 * task),
        ("10× the task", 10 * task),
        ("100×", 100 * task),
        ("1,000×", 1000 * task),
    ] {
        let f = 2.0 * s as f64 * a_step;
        println!(
            "  {what}: S = {s}, F = {f:.2e}: {:.1} s under the JIT, {:.1} s under the interpreter",
            f / slowest[0],
            f / slowest[1]
        );
    }
}

/// The task the limits are sized by (S0's): a 20,000-line test log triaged — failures, `-->` by file, error codes.
const MEASURED_TASK: &str = r#"const text = await tool("output", "run#1");
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

/// A test run's output: mostly passes, some failures, warnings and errors pointing at files (S0's generator).
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
        let name = &file[4..file.len() - 3];
        match r % 100 {
            0..=79 => out.push_str(&format!("test {name}::t{} ... ok\n", r % 9973)),
            80..=84 => out.push_str(&format!("test {name}::t{} ... FAILED\n", r % 9973)),
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
