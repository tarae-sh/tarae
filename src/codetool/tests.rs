//! The host's own logic on a stand-in engine (every machine), then the real engine where `cargo xtask quickjs` built
//! it (else those skip, as a grammar's tests do).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::*;

/// A stand-in engine: `start`'s source is a script of replies (separated by 0x1e), given one a step. Four replies
/// act: `!` spins until the fuel runs out, `@` ends with the last answer as the program's value, `+` grows its
/// memory from 1 MB to 64 MB and a page (no maximum of its own) and gives the next reply whatever the grow said,
/// `#` traps.
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
                 (i32.eq (i32.load8_u offset=4 (local.get $r)) (i32.const 0x2b)))
      (then
        (drop (memory.grow (i32.const 1009)))
        (local.set $r (call $next))))
    (if (i32.and (i32.eq (i32.load (local.get $r)) (i32.const 1))
                 (i32.eq (i32.load8_u offset=4 (local.get $r)) (i32.const 0x23)))
      (then unreachable))
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
    (call $reply (local.get $p) (local.get $n)))
  (func (export "oom") (result i32) (i32.const 0)))"#;

pub(crate) fn stand_in() -> Arc<Engine> {
    Arc::new(Engine::new(&wat::parse_str(STAND_IN).unwrap()).unwrap())
}

pub(crate) fn script(replies: &[&str]) -> String {
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

/// tarae's own memory cap, alone (the real engine's 100 MB case can't tell it from the module's maximum and QuickJS's
/// heap limit): the stand-in has neither, so only `Limits.memory` refuses its grow past 64 MB — and the program ends
/// at its memory limit, said with its kind first and the way out, whether the engine then traps (an allocator's
/// abort) or says an error of its own.
#[test]
fn a_program_past_the_memory_cap_ends_at_its_memory_limit() {
    let e = stand_in();
    let limits = Limits { fuel: 1 << 30, ..LIMITS };
    let words = "stopped at its memory limit (64 MB): do less in one program, or split it";
    for then in ["#", "error\n{\"error\":\"its own words\"}"] {
        let end = ended(Program::start(&e, &script(&["+", then]), limits));
        assert_eq!(end.result, Err(words.into()), "after {then:?}");
        assert!(end.stopped, "a limit's ending");
    }
    // the engine's own out of memory with no grow refused: its heap limit was the bound met, and the words say it
    let heap = "error\n{\"error\":\"InternalError: out of memory\"}";
    let end = ended(Program::start(&e, heap, limits));
    let said = "stopped at its memory limit (48 MB): do less in one program, or split it";
    assert_eq!((end.result, end.stopped), (Err(said.into()), true));
    // after a refused grow the cap is the bound met, whatever the engine then says
    let end = ended(Program::start(&e, &script(&["+", heap]), limits));
    assert_eq!(end.result, Err(words.into()));
    // a program that copes with the refusal ends as it says
    let end = ended(Program::start(&e, &script(&["+", "done\n{\"done\":1}"]), limits));
    assert_eq!(end.result, Ok(Some(json!(1))));
    // with room for it, the grow is no ending: the trap after it is the engine's own
    let roomy = Limits { memory: 128 << 20, ..limits };
    let end = ended(Program::start(&e, &script(&["+", "#"]), roomy));
    assert_eq!(end.result, Err("the engine stopped: unreachable".into()));
}

/// `ENGINE_HEAP` is the figure the pinned engine was built with: a re-pin that moves `glue.c`'s heap limit moves the
/// words' figure with it, or this fails.
#[test]
fn the_engines_heap_limit_is_glues() {
    let glue = include_str!("../../xtask/quickjs/glue.c");
    let line = glue.lines().find(|l| l.starts_with("#define MEMORY_LIMIT ")).expect("glue.c's MEMORY_LIMIT");
    assert_eq!(line, format!("#define MEMORY_LIMIT ({} << 20)", ENGINE_HEAP >> 20));
    // (whole MB, as the words say it; inside the cap: the bound a program meets first)
    const { assert!(ENGINE_HEAP.is_multiple_of(1 << 20) && ENGINE_HEAP < LIMITS.memory) };
    // and the words the guard says its ending in are the ones the host reads as it
    let words =
        glue.lines().find(|l| l.starts_with("#define OUT_OF_MEMORY ")).expect("glue.c's OUT_OF_MEMORY");
    assert_eq!(words, format!("#define OUT_OF_MEMORY {ENGINE_OUT_OF_MEMORY:?}"));
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
        let fuel = ended(Program::start(&e, "!", Limits { jit, ..SMALL }));
        assert_eq!(
            fuel.result.unwrap_err(),
            "stopped at its work limit (what the steps don't count too: strings, JSON, sorting): do less in one \
             program, or split it"
        );
        assert!(fuel.stopped, "a limit is a stop, not a failure");
    }
    let (p, _) = calls(Program::start(
        &e,
        &script(&["pending\n{\"pending\":[{\"id\":0,\"name\":\"x\"}]}", "!"]),
        SMALL,
    ));
    assert!(
        error(p.answer(0, &json!(1))).starts_with("stopped at its work limit"),
        "the fuel is the whole program's, resumes too"
    );
    let steps = r#"error
{"error":"InternalError: interrupted","stopped":"steps","steps":20001,"printed":"before\n","printed_bytes":7}"#;
    let end = ended(Program::start(&e, steps, Limits { steps: 20_000, ..SMALL }));
    // the words say the limit (the program's own count is one past it), and the way out
    assert_eq!(
        end.result,
        Err(
            "stopped at its step limit (20000 steps; a step is 10,000 loop turns and branches): do less in one \
             program, or split it"
                .into()
        )
    );
    assert_eq!((end.printed.as_str(), end.steps, end.stopped), ("before\n", 20001, true));
    // (an error the program threw is no stop: its row says `failed`)
    let threw = r#"error
{"error":"TypeError: no","steps":3,"printed":"","printed_bytes":0}"#;
    let end = ended(Program::start(&e, threw, SMALL));
    assert_eq!((end.result, end.stopped), (Err("TypeError: no".into()), false));
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
        stopped: false,
    };
    assert_eq!(end(Ok(Some(json!(2))), "").text(), "returned: 2");
    assert_eq!(end(Ok(Some(json!("a\nb"))), "x\n").text(), "x\nreturned:\na\nb");
    assert_eq!(end(Ok(None), "").text(), "returned nothing, printed nothing");
    assert_eq!(end(Ok(None), "x").text(), "x\n");
    assert_eq!(end(Err("TypeError: no".into()), "before\n").text(), "before\nerror: TypeError: no");
    // (the user's copy under the row: no `error: ` — the row's `▲` says it)
    assert_eq!(end(Err("TypeError: no".into()), "before\n").shown(), "before\nTypeError: no");
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
    start_job(&ed.events.jobs(), load, "", src, SMALL, Watch::new(&SMALL), move |ed, next| {
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

#[test]
fn the_clocks_words_say_its_limit_and_the_way_out() {
    assert_eq!(
        time_limit(CLOCK_JIT),
        "stopped at its time limit (10.0s of running): do less in one program, or split it"
    );
    assert_eq!(
        time_limit(CLOCK_INTERPRETER),
        "stopped at its time limit (1m 00s of running): do less in one program, or split it"
    );
    assert_eq!(Limits { jit: true, ..LIMITS }.clock(), CLOCK_JIT);
    assert_eq!(Limits { jit: false, ..LIMITS }.clock(), CLOCK_INTERPRETER);
}

/// The host's stop (the user's `C-c`, the turn's end) ends a program running — at the runtime's next check, from
/// another thread — or waiting, at its next step; its words say who, and the first reason is kept.
#[test]
fn the_hosts_stop_ends_a_program_running_or_waiting() {
    let e = stand_in();
    for jit in [true, false] {
        // (a stop that didn't land ends at this fuel, in seconds)
        let limits = Limits { jit, fuel: 1 << 36, ..SMALL };
        let watch = Watch::new(&limits);
        let run = std::thread::spawn({
            let (e, watch) = (e.clone(), watch.clone());
            move || Program::start_with(&e, "!", limits, watch)
        });
        crate::event::sleep(Duration::from_millis(20));
        watch.stop("the turn ended");
        watch.stop("C-c");
        assert_eq!(error(run.join().unwrap()), "stopped: the turn ended", "running, jit {jit}");
        assert_eq!(watch.stopped(), Some(Why::Host("the turn ended".into())));

        let watch = Watch::new(&limits);
        let pending = "pending\n{\"pending\":[{\"id\":0,\"name\":\"x\"}]}";
        let (p, _) = calls(Program::start_with(&e, &script(&[pending, "@"]), limits, watch.clone()));
        watch.stop("C-c");
        assert_eq!(error(p.answer(0, &json!(1))), "stopped: C-c", "waiting, jit {jit}");
        assert_eq!(watch.steps(), 2);
    }
}

/// A program's running time is its steps' alone: the time a step runs counts, the wait for an answer doesn't.
#[test]
fn the_clock_counts_the_steps_not_the_waits() {
    let e = stand_in();
    let watch = Watch::new(&SMALL);
    let pending = "pending\n{\"pending\":[{\"id\":0,\"name\":\"x\"}]}";
    let (p, _) = calls(Program::start_with(&e, &script(&[pending, "@"]), SMALL, watch.clone()));
    let first = watch.ran();
    crate::event::sleep(Duration::from_millis(50));
    assert_eq!(watch.ran(), first, "waiting adds nothing");
    assert!(ended(p.answer(0, &json!(1))).result.is_ok());
    assert!(watch.ran() >= first && watch.ran() < first + Duration::from_millis(50), "{:?}", watch.ran());
    assert_eq!(watch.steps(), 2);
}

// —— the real engine ——

/// The engine `cargo xtask quickjs` built, if it's there.
pub(crate) fn real() -> Option<Arc<Engine>> {
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
        "ReferenceError: 'eval' is not defined",
        "TypeError: eval is not supported",
        "TypeError: eval is not supported",
    ];
    assert_eq!(evals.result, Ok(Some(json!(refused))), "nothing a program makes becomes code");
    // `Atomics` works on the program's own typed arrays alone: no other thread, and a wait throws
    let wait = ok(
        &e,
        "try { Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0); return 'waited' } \
         catch (x) { return String(x) }",
    );
    assert_eq!(wait.result, Ok(Some(json!("TypeError: cannot block in this thread"))));
    // nor is anything loaded: no module loader is set, so a dynamic import (async since v0.18.0) rejects
    let import = ok(&e, "try { await import('x'); return 'loaded' } catch (x) { return String(x) }");
    let said = import
        .result
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    assert!(said.contains("Error") && said != "loaded", "{:?}", import.result);
}

#[test]
fn the_real_engine_stops_at_its_limits_and_the_next_program_starts_clean() {
    let Some(e) = real() else { return };
    let limit = |src: &str| {
        let end = ok(&e, src);
        (end.result.unwrap_err(), end.steps, end.printed)
    };
    let words = format!(
        "stopped at its step limit ({} steps; a step is 10,000 loop turns and branches): do less in one program, \
         or split it",
        LIMITS.steps
    );
    let words = words.as_str();
    assert_eq!(limit("print('before'); for (;;) {}"), (words.into(), LIMITS.steps + 1, "before\n".into()));
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
    // (the engine's heap limit is met first here, inside the cap: the words say its figure, never the cap's)
    let memory = "stopped at its memory limit (48 MB): do less in one program, or split it";
    let grow = ok(&e, "const a = []; for (;;) a.push('x'.repeat(1 << 20) + a.length);");
    assert_eq!((grow.result, grow.stopped), (Err(memory.into()), true));
    // 100 MB kept ends it, at the heap limit's words. This proves one of the three bounds holds, not which — the module's own maximum
    // (`inside.sh`'s `--max-memory`, 64 MB), QuickJS's heap limit (`glue.c`, 48 MB, met first) and `Limits.memory`
    // each hold alone; tarae's own, the last, is `a_program_past_the_memory_cap_ends_at_its_memory_limit`'s
    let kept = ok(
        &e,
        "const a = []; for (let i = 0; i < 100; i++) a.push('x'.repeat(1 << 20) + i); return a.length;",
    );
    assert_eq!(kept.result, Err(memory.into()));
    // (an `out of memory` caught: the guard's own tests, below)
    assert_eq!(
        ok(&e, "return typeof a + typeof print").result,
        Ok(Some(json!("undefinedfunction"))),
        "nothing left"
    );
}

/// Recursion past the stack glue.c sets (`STACK_LIMIT`, inside the module's own) is the engine's error, which the
/// program can catch: never a trap.
#[test]
fn deep_recursion_is_an_error_the_program_sees() {
    let Some(e) = real() else { return };
    let caught = ok(&e, "function f(n) { return f(n + 1) + 1 } try { f(0) } catch (x) { return String(x) }");
    assert_eq!(caught.result, Ok(Some(json!("InternalError: stack overflow"))));
    let left = ok(&e, "function f(n) { return f(n + 1) + 1 } return f(0);").result.unwrap_err();
    assert!(left.starts_with("InternalError: stack overflow"), "{}", cut(&left, 200));
}

// —— the out-of-memory guard (glue.c's `oom`) ——
//
// The cap is the engine's allocator refusing past `ENGINE_HEAP`, guard or no guard. The original throws a catchable
// `out of memory`; the guard ends the program at the next poll after the first refusal, and every reply from then is
// that ending, in words the glue makes without the engine's heap.

const HEAP_WORDS: &str = "stopped at its memory limit (48 MB): do less in one program, or split it";

/// The heap filled in a `try` until the engine refuses, the refusal caught.
const FILL: &str =
    "let a = []; try { for (;;) a.push('x'.repeat(1 << 20) + a.length); } catch (x) { a = null; ";

#[test]
fn out_of_memory_caught_and_carried_on_still_ends_it() {
    let Some(e) = real() else { return };
    let end = ok(&e, &format!("{FILL} for (;;) {{}} }}"));
    assert_eq!(
        (end.result, end.stopped),
        (Err(HEAP_WORDS.into()), true),
        "the memory's words, not the steps'"
    );
}

#[test]
fn out_of_memory_caught_again_and_again_ends_it() {
    let Some(e) = real() else { return };
    let end = ok(&e, "let a = []; for (;;) try { a.push('x'.repeat(1 << 20)) } catch { a = [] }");
    assert_eq!(end.result, Err(HEAP_WORDS.into()));
    assert!(end.steps <= 1, "ended at the first refusal: {} steps", end.steps);
}

/// A refusal inside a builtin (which throws at once) ends the program at its next poll, not 10,000 branches later:
/// the fuel it spends after the refusal is at most two `indexOf`s over 8 MB, each a reference run's. The fuel past
/// that ends a broken guard's run (else 10,000 of them).
#[test]
fn a_refusal_in_a_builtin_ends_at_the_next_poll() {
    let Some(e) = real() else { return };
    let s = "const s = 'ab'.repeat(1 << 22); let a = [];";
    let one = ok(&e, &format!("{s} return s.indexOf('c')")).fuel - ok(&e, &format!("{s} return -1")).fuel;
    assert!(one > 1_000_000, "an indexOf over 8 MB is work: {one}");
    let after = "let n = 0; for (;;) n += s.indexOf('c');";
    let cases = [
        ("repeat", format!("{s} try {{ for (;;) a.push('ab'.repeat(1 << 19)) }} catch (x) {{}}")),
        (
            "join",
            format!(
                "{s} const p = Array.from({{length: 1 << 10}}, (_, i) => 'x'.repeat(511) + i); \
                 try {{ for (;;) a.push(p.join('')) }} catch (x) {{}}"
            ),
        ),
        (
            "JSON.stringify",
            format!(
                "{s} const o = Array.from({{length: 1 << 12}}, (_, i) => 'y'.repeat(120) + i); \
                 try {{ for (;;) a.push(JSON.stringify(o)) }} catch (x) {{}}"
            ),
        ),
        (
            "a sort's comparator",
            format!(
                "{s} [5, 3, 8, 1, 9, 2, 7].sort((x, y) => {{ try {{ for (;;) a.push('z'.repeat(1 << 20) + a.length) }} \
                 catch (e) {{}} return 0 }});"
            ),
        ),
    ];
    for (what, before) in cases {
        let alone = ok(&e, &format!("{before} return 'carried on'"));
        assert_eq!(alone.result, Err(HEAP_WORDS.into()), "{what}");
        let limits = Limits { fuel: alone.fuel + 100 * one, ..REAL };
        let on = drive(&e, &format!("{before} {after}"), limits, |_| json!(null)).0;
        assert_eq!(on.result, Err(HEAP_WORDS.into()), "{what}");
        let spent = on.fuel.saturating_sub(alone.fuel);
        assert!(spent <= 2 * one, "{what}: {spent} fuel after the refusal, one indexOf {one}");
    }
}

/// Past a refusal, whatever the program goes on to reply — a value, a wait, its own error — is the memory's
/// ending: with the heap let go, with it still full (the words need none of it), and with the refusal coming while a
/// call's answer is read on `resume`.
#[test]
fn every_reply_after_a_refusal_is_out_of_memory() {
    let Some(e) = real() else { return };
    let kept = "let a = []; try { for (;;) a.push('x'.repeat(1 << 20) + a.length); } catch (x) { ";
    for tail in ["return 'ok' }", "await tool('x'); return 'ok' }", "throw new Error('mine') }"] {
        for (heap, before) in [("let go", FILL), ("still full", kept)] {
            let (end, steps) = drive(&e, &format!("{before}{tail}"), REAL, |_| json!(null));
            assert_eq!(end.result, Err(HEAP_WORDS.into()), "{tail}, the heap {heap}");
            assert!(steps.is_empty(), "{tail}, the heap {heap}: no call handed out, {steps:?}");
        }
    }
    // 40 MB kept, then an answer of 12 MB: the refusal is the answer's, read on `resume`
    let (end, steps) = drive(
        &e,
        "const a = []; for (let i = 0; i < 40; i++) a.push('x'.repeat(1 << 20) + i); \
         try { const v = await tool('x'); return v.length } catch (x) { return 'caught' }",
        REAL,
        |_| json!("y".repeat(12 << 20)),
    );
    assert_eq!(steps, [vec!["0:x".to_string()]], "the program got as far as its call");
    assert_eq!(end.result, Err(HEAP_WORDS.into()), "refused on resume");
    // and refused after the last poll, where only the reply can say it: inside `tool()` itself (a long name is a
    // block of its own), which leaves the call it records broken (the reply's JSON then fails, `answer` says the
    // refusal) — every refusal of the program walked, on the probe
    let Some(probe) = probe() else { return };
    let src = "await tool('n'.repeat(1000)); return 'ok'";
    let mut p = by_hand(&probe, REAL);
    assert_eq!(p.start_by_hand(src).0, "pending 0:nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn");
    let grown = p.probe_call("grows", &[], &probe);
    for n in 1..=grown {
        let mut p = by_hand(&probe, REAL);
        p.probe_call("refuse_at", &[Val::I32(n as i32), Val::I32(1)], &probe);
        let (head, body) = p.start_by_hand(src);
        assert_eq!(
            (head.as_str(), &body["error"]),
            ("error", &json!(ENGINE_OUT_OF_MEMORY)),
            "refusal {n} of {grown}"
        );
    }
}

/// The glue's own clearing: a second program on the same instance (which the host never does — each program has a
/// fresh one) doesn't inherit the first one's refusal.
#[test]
fn a_refusal_doesnt_end_the_next_program() {
    let Some(e) = real() else { return };
    let mut p = by_hand(&e, REAL);
    let (head, body) = p.start_by_hand(&format!("{FILL} return 'ok' }}"));
    assert_eq!((head.as_str(), &body["error"]), ("error", &json!(ENGINE_OUT_OF_MEMORY)), "{body}");
    let (head, body) = p.start_by_hand("return 1 + 1");
    assert_eq!((head.as_str(), &body["done"]), ("done", &json!(2)), "{body}");
}

/// The probe module (`inside.sh`, glue.c's GLUE_PROBE): the tests' alone, never shipped.
const PROBE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/quickjs/out/quickjs-probe.wasm");
const PROBE_EXPORTS: [&str; 3] = ["refuse_at", "grows", "compile_grows"];

fn probe() -> Option<Engine> {
    let bytes = std::fs::read(PROBE).ok()?;
    Some(Engine::new(&bytes).expect("the probe loads"))
}

/// An instance of `e` driven by hand: `start` twice on one instance, or a probe's export called before it — what the
/// host never does.
fn by_hand(e: &Engine, limits: Limits) -> Program {
    let wasm = tarae_wasm::Limits { memory_bytes: limits.memory, jit: limits.jit, ..Default::default() };
    let mut inst = Instance::new(e.module.clone(), wasm, limits.fuel).expect("an instance");
    if let Some(init) = e.init {
        inst.call(init, &[]).expect("_initialize");
    }
    Program {
        inst,
        watch: Watch::new(&limits),
        fns: e.fns,
        out: BTreeSet::new(),
        asked: 0,
        fuel: limits.fuel,
        steps: limits.steps,
        memory: limits.memory,
    }
}

impl Program {
    /// `start`'s reply, its header and its JSON (a trap or a broken reply: `trap` and what it was).
    fn start_by_hand(&mut self, src: &str) -> (String, Value) {
        let reply = self.put(src.as_bytes()).and_then(|(at, len)| {
            self.call(self.fns.start, &[at, len, Val::I32(self.steps as i32)]).and_then(|r| self.reply(r))
        });
        reply.unwrap_or_else(|s| {
            let why = match s {
                Stop::Trap(t) => t.to_string(),
                Stop::Engine(e) => e,
            };
            ("trap".into(), json!(why))
        })
    }

    /// One of the probe's exports, called.
    fn probe_call(&mut self, name: &str, args: &[Val], e: &Engine) -> u32 {
        let f = e.module.func(name).expect("the probe's export");
        match self.inst.call(f, args).ok().as_deref() {
            Some([Val::I32(n)]) => *n as u32,
            _ => 0,
        }
    }
}

/// The shipped module holds nothing of the probe, though both come from one glue.c — and holds the glue's flag, read
/// after a trap (`Program::oom_noted`).
#[test]
fn the_shipped_engine_holds_nothing_of_the_probe() {
    let Ok(shipped) = std::fs::read(ENGINE_DEV) else { return };
    let e = Engine::new(&shipped).expect("the engine loads");
    assert!(e.module.func("oom").is_some(), "quickjs.wasm exports the glue's flag");
    for name in PROBE_EXPORTS {
        assert!(e.module.func(name).is_none(), "quickjs.wasm exports `{name}`");
        assert!(!shipped.windows(name.len()).any(|w| w == name.as_bytes()), "quickjs.wasm names `{name}`");
    }
    // (the names looked for are the probe's: a rename there can't leave this test looking at nothing)
    if let Some(p) = probe() {
        assert!(PROBE_EXPORTS.iter().all(|n| p.module.func(n).is_some()), "the probe's exports are these");
    }
}

/// The bodies the property walks: every growing allocation of each refused in turn.
const BODIES: [(&str, &str); 13] = [
    ("array growth", "const a = []; for (let i = 0; i < 3000; i++) a.push(i);"),
    ("object growth", "const o = {}; for (let i = 0; i < 300; i++) o['k' + i] = i;"),
    ("ropes, flattened", "let s = ''; for (let i = 0; i < 300; i++) s += 'ab' + i; s.indexOf('z');"),
    ("repeat", "'abc'.repeat(5000).length;"),
    ("join", "Array.from({length: 300}, (_, i) => 'x' + i).join(',');"),
    ("replaceAll", "'a-b-c-'.repeat(300).replaceAll('-', '+');"),
    (
        "JSON both ways",
        "JSON.parse(JSON.stringify({a: [1, 2, {b: 'c'.repeat(600)}], n: Array.from({length: 200}, (_, i) => i)}));",
    ),
    ("a regex with captures", "[...'ab-12 cd-34 '.repeat(60).matchAll(/(\\w+)-(\\d+)/g)].map(m => m[2]);"),
    (
        "Map and Set",
        "const m = new Map(), t = new Set(); for (let i = 0; i < 300; i++) { m.set('k' + i, i); t.add(i) }",
    ),
    (
        "closures",
        "const fs = []; for (let i = 0; i < 300; i++) fs.push(() => i); fs.reduce((a, f) => a + f(), 0);",
    ),
    ("BigInt", "let b = 1n; for (let i = 0; i < 100; i++) b *= 12345678901234567890n; b.toString().length;"),
    (
        "typed arrays",
        "const t = new Float64Array(2048); t.fill(1.5); new Uint8Array(t.buffer).slice(0, 4000).length;",
    ),
    (
        "promise jobs",
        "await Promise.all(Array.from({length: 60}, (_, i) => Promise.resolve(i).then(x => [x, String(x)])));",
    ),
];

/// The one body whose refusals the engine carries on past, the guard off: a Map's hash table it leaves ungrown.
const GIVES_UP: &str = "Map and Set";

/// What the guard stands on, walked on the probe: every growing allocation of each body refused in turn, the body
/// inside `try { BODY; return 'absorbed' } catch (e) { return String(e) }`.
/// - **The guard off**, the engine's own answer: it never carries on past a refusal (`absorbed`) — no retry, so
///   ending at the first refusal is never early — but at a Map's hash table, which it leaves ungrown
///   (`map_hash_resize`: "no reporting of memory allocation failure"): a growth given up, not retried. (A job it
///   couldn't queue is swallowed too: the program then waits on a promise nothing settles, an error ending.)
/// - **The guard on**, every refusal ends the program at the memory limit: none is swallowed.
/// - A refusal in the compile ends it as well, at the memory limit or in a trap (the compiler reads on past a
///   bytecode buffer it couldn't grow), never with a value.
#[test]
fn the_engine_never_retries_a_refused_allocation() {
    let Some(probe) = probe() else { return };
    let t0 = std::time::Instant::now();
    let (mut runs, mut gave_up, mut compile_traps) = (0, 0, 0);
    for (what, body) in BODIES {
        let src = format!("try {{ {body} return 'absorbed' }} catch (e) {{ return String(e) }}");
        let mut p = by_hand(&probe, REAL);
        let whole = p.start_by_hand(&src);
        assert_eq!((whole.0.as_str(), &whole.1["done"]), ("done", &json!("absorbed")), "{what}: {}", whole.1);
        let grown = p.probe_call("grows", &[], &probe);
        let compiled = p.probe_call("compile_grows", &[], &probe);
        assert!(grown > compiled, "{what}: no growing allocation past the compile");
        for n in 1..=grown {
            for guard in [false, true] {
                let mut p = by_hand(&probe, REAL);
                p.probe_call("refuse_at", &[Val::I32(n as i32), Val::I32(guard as i32)], &probe);
                let (head, body) = p.start_by_hand(&src);
                runs += 1;
                let at = format!(
                    "{what}: allocation {n} of {grown} refused ({} the compile), the guard {}",
                    if n <= compiled { "in" } else { "past" },
                    if guard { "on" } else { "off" }
                );
                match (n <= compiled, guard, head.as_str()) {
                    (true, _, "trap") => compile_traps += 1,
                    (_, true, _) => assert_eq!(
                        (head.as_str(), &body["error"]),
                        ("error", &json!(ENGINE_OUT_OF_MEMORY)),
                        "{at}: {body}"
                    ),
                    (_, false, "error") => {}
                    (_, false, "done") if body["done"] == "absorbed" && what == GIVES_UP => gave_up += 1,
                    (_, false, "done") => {
                        assert_ne!(body["done"], json!("absorbed"), "{at}: the program went on")
                    }
                    _ => panic!("{at}: {head} {body}"),
                }
            }
        }
    }
    assert!(
        gave_up > 0,
        "a Map's ungrown hash table is no longer the refusal the engine carries on past: re-read it"
    );
    let line = format!(
        "{runs} refusals walked ({gave_up} given up at a Map's table, the guard off; {compile_traps} trapped in the \
         compile), in {:.2?}\n",
        t0.elapsed()
    );
    let _ = std::io::Write::write_all(&mut std::io::stderr(), line.as_bytes());
}

/// Every refusal in the compile of each of `BODIES` that traps inside the engine, the guard on or off, run through
/// the host: where, the trap, and how the host ended it.
fn compile_traps(probe: &Engine, guard: bool) -> Vec<(String, String, Ended)> {
    let mut traps = Vec::new();
    for (what, body) in BODIES {
        let src = format!("try {{ {body} return 'absorbed' }} catch (e) {{ return String(e) }}");
        let mut p = by_hand(probe, REAL);
        p.start_by_hand(&src);
        let compiled = p.probe_call("compile_grows", &[], probe);
        for n in 1..=compiled {
            let mut p = by_hand(probe, REAL);
            p.probe_call("refuse_at", &[Val::I32(n as i32), Val::I32(guard as i32)], probe);
            let steps = Val::I32(p.steps as i32);
            let reply = p.put(src.as_bytes()).and_then(|(at, len)| p.call(p.fns.start, &[at, len, steps]));
            let trap = match &reply {
                Err(Stop::Trap(t)) => t.to_string(),
                _ => continue,
            };
            let at = format!("{what}: refusal {n} of {compiled}");
            traps.push((at, trap, ended(p.next(reply, Some(&src)))));
        }
    }
    traps
}

/// A refusal while compiling can trap inside the engine (the compiler reads on past a buffer it couldn't grow): the
/// glue's flag noted, the host says the memory limit's words, a limit's ending — one sentence for one cause, never
/// "try again" — and the trap's own name goes to the log.
#[test]
fn a_trap_after_a_refusal_in_the_compile_ends_at_the_memory_limit() {
    let Some(probe) = probe() else { return };
    let log = crate::log::capture();
    let traps = compile_traps(&probe, true);
    assert!(!traps.is_empty(), "no refusal in the compile traps now: re-read the property walk's count");
    for (at, trap, end) in &traps {
        assert_eq!((&end.result, end.stopped), (&Err(HEAP_WORDS.into()), true), "{at}: trapped {trap}");
    }
    let lines = log.lines();
    for (at, trap, _) in &traps {
        let line = format!("the engine trapped after a refusal: {trap}");
        assert!(lines.iter().any(|l| l.contains(&line)), "{at}: `{line}` not in the log {lines:?}");
    }
}

/// Beside it, a trap with no refusal noted is the engine's own fault, whatever came before: the compile's refusals
/// with the guard off (the glue notes none) still say "the engine stopped: …" — the flag can't make every fault a
/// limit.
#[test]
fn a_trap_with_no_refusal_noted_says_the_engine_stopped() {
    let Some(probe) = probe() else { return };
    let traps = compile_traps(&probe, false);
    assert!(!traps.is_empty(), "no refusal in the compile traps now: re-read the property walk's count");
    for (at, trap, end) in &traps {
        let said = format!("the engine stopped: {trap}");
        assert_eq!((&end.result, end.stopped), (&Err(said), false), "{at}");
    }
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

/// The work the steps don't see (the re-pin's §4): each runs for ever, and only the fuel — or the flag — ends it.
pub const HEAVY: [(&str, &str); 3] = [
    ("JSON.parse", "const s = JSON.stringify(await tool('o')); for (;;) JSON.parse(s);"),
    (
        "a comparator's sort",
        "await tool('o'); const a = Array.from({length: 200000}, (_, i) => (i * 7919) % 200003); \
         for (;;) a.slice().sort((x, y) => x - y);",
    ),
    ("string building", "await tool('o'); for (;;) 'ab'.repeat(1 << 22).replaceAll('a', 'c').length;"),
];

/// The host's stop lands inside the work the steps don't see, on both engines: each shape, its second step running
/// for ever, is stopped from another thread once that step has begun. (In real time: the clock's own test is the
/// simulation's, `src/sim/codeclock.rs`.) And a stopped program only ever says its ending: nothing in the host
/// gives an instance a new flag but its start — a QuickJS stopped inside a builtin is torn.
#[test]
fn the_hosts_stop_ends_the_work_the_steps_dont_see() {
    let host = include_str!("../codetool.rs");
    assert_eq!(host.matches(".set_interrupt(").count(), 1, "only `start_with` gives an instance its flag");
    let Some(e) = real() else { return };
    let log = test_log(2_000);
    for jit in [true, false] {
        for (what, src) in HEAVY {
            let limits = Limits { jit, ..LIMITS };
            let watch = Watch::new(&limits);
            let (p, _) = calls(Program::start_with(&e, src, limits, watch.clone()));
            let run = std::thread::spawn({
                let log = log.clone();
                move || p.answer(0, &json!(log))
            });
            // (waits on the second step's stamp, never a time: wherever the stop lands from then on — before the
            // engine's first instruction or deep in its builtin — the next check ends the step, and the first step's
            // fuel is already spent)
            let since = std::time::Instant::now();
            while watch.steps() < 2 {
                assert!(
                    since.elapsed() < crate::test_results::PATIENCE,
                    "{what}: its second step never began"
                );
                std::thread::yield_now();
            }
            watch.stop("the turn ended");
            let end = ended(run.join().unwrap());
            assert_eq!(end.result, Err("stopped: the turn ended".into()), "{what}, jit {jit}");
            assert!(end.fuel > 0 && end.fuel < limits.fuel, "{what}: {}", end.fuel);
        }
    }
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

/// QuickJS's dispatch on the runtime under both engines (what the host-set interrupt flag costs is read here,
/// `.sb/codemode-s1-flag-report.md`): a loop of bytecode alone, and the 20,000-line triage (bytecode, regex,
/// strings). Median of 5; reported only. Skipped where `cargo xtask quickjs` hasn't built the engine.
#[test]
fn perf_code_engine_dispatch() {
    if crate::test_results::unoptimized("codetool::tests::perf_code_engine_dispatch") {
        return;
    }
    let Some(e) = real() else { return };
    let log = test_log(20_000);
    let unbounded = |jit| Limits { jit, steps: u32::MAX, ..LIMITS };
    let looped = "let s = 0; for (let i = 0; i < 2000000; i++) s = (s * 31 + i) | 0; return s;";
    for (jit, engine) in [(true, "jit"), (false, "interp")] {
        for (what, src) in [("loop", looped), ("triage", MEASURED_TASK)] {
            let mut all: Vec<std::time::Duration> = (0..5)
                .map(|_| {
                    let t = std::time::Instant::now();
                    let end = drive(&e, src, unbounded(jit), |_| json!(log)).0;
                    let t = t.elapsed();
                    assert!(end.result.is_ok(), "{what}: {:?}", end.result);
                    t
                })
                .collect();
            all.sort_unstable();
            let ms = all[all.len() / 2].as_secs_f64() * 1e3;
            let line = format!("PERF code_engine_{what}_{engine}_ms {ms:.2} noise=1\n");
            let _ = std::io::Write::write_all(&mut std::io::stderr(), line.as_bytes());
        }
    }
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
        // (a case the steps see — an engine that polls inside a builtin's loop; none of these on the original
        // QuickJS, 2026-06-04 — is said, and left out of the slowest rate: its steps bound it)
        println!(
            " fuel a second in C-heavy work (on 1e10 fuel; the slowest taken where the steps see nothing):"
        );
        for (what, src) in heavy {
            let (end, rate, _) = time(what, src, on(10_000_000_000));
            match end.steps {
                0 => slowest[k] = slowest[k].min(rate),
                n => println!("    (the steps see it: {n} steps on 1e10 fuel)"),
            }
        }
    }
    println!(
        "== for a step limit S: fuel F = limits(S) = 2 × S × FUEL_A_STEP ({FUEL_A_STEP:.2e}; the costliest step \
         above {a_step:.2e}); the longest a program runs on F where the steps see nothing (the slowest rate above)"
    );
    for (what, s) in [
        ("3× the task", 3 * task),
        ("10× the task", 10 * task),
        ("100×", 100 * task),
        ("1,000×", 1000 * task),
    ] {
        let f = limits(s).fuel as f64;
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
pub(crate) fn test_log(lines: usize) -> String {
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
