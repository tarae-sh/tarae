//! The `code` tool on the editor's side: a program's calls through the tools' own gates, together, given back in
//! the order handed out. On the stand-in engine (`codetool::tests`: a script of replies, `@` returning the last
//! answer given), so every machine runs them.

use std::path::PathBuf;

use super::*;
use crate::codetool::tests::{script, stand_in};
use crate::test_results::settle_until;

/// A project (`p`, with `a.txt`) and a place outside it (`outside/o.txt`) in the test's own folder, an editor whose
/// chat lists the tool and whose programs run on the stand-in. (The editor, the project's real path, outside's.)
fn editor(s: &crate::scratch::Scratch) -> (Editor, PathBuf, PathBuf) {
    let base = crate::paths::real(s).unwrap();
    let (dir, outside) = (base.join("p"), base.join("outside"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(dir.join("a.txt"), "alpha\n").unwrap();
    std::fs::write(outside.join("o.txt"), "outside marker\n").unwrap();
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = dir.clone();
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.claude.chat.as_mut().unwrap().code = true;
    ed.workspace.set_at(crate::workspace::Here::Chat);
    ENGINE.with(|e| *e.borrow_mut() = Some(stand_in()));
    BARE.with(|b| b.set(true));
    (ed, dir, outside)
}

fn generation(ed: &Editor) -> u64 {
    ed.workspace.claude.chat.as_ref().unwrap().generation()
}

/// A `pending` reply handing out `calls` (id, name, input).
fn pending(calls: &[(u32, &str, Value)]) -> String {
    let list: Vec<Value> = calls.iter().map(|(id, n, a)| json!({ "id": id, "name": n, "arg": a })).collect();
    format!("pending\n{}", json!({ "pending": list }))
}

/// A reply waiting on calls already out.
const WAITS: &str = "pending\n{\"pending\":[]}";

fn answered(id: u64) -> Option<Result<String, String>> {
    crate::tools::ANSWERED.with(|a| a.borrow().iter().find(|(k, _)| *k == json!(id)).map(|(_, r)| r.clone()))
}

/// The program `steps` (the stand-in's replies), as `code` call `n`.
fn start(ed: &mut Editor, n: u64, steps: &[&str]) {
    let call = Call::of(generation(ed), "", json!(n)).named("code");
    super::call(ed, call, &json!({ "src": script(steps) })).unwrap();
}

/// `start`, run to its answer.
fn run(ed: &mut Editor, n: u64, steps: &[&str]) -> Result<String, String> {
    start(ed, n, steps);
    settle_until(ed, |_| answered(n).is_some());
    answered(n).unwrap()
}

fn kept(id: u64, generation: u64) -> Kept {
    let output = [(false, "test a ... FAILED"), (true, "error: 1 failed")].into_iter().collect();
    let (ended, exit, took) = ("exit 101".into(), Some(101), Some(Duration::from_millis(1500)));
    Kept { id, generation, command: "cargo test".into(), ended, exit, took, output }
}

/// The design session's point 2 (2026-11-11): a batch's answers go back in the order the calls were handed out,
/// whichever came first — here `output`'s, given as it's asked, comes before the read's, which a worker brings, and
/// the program still gets the read's first: the last answer it's given (what `@` returns) is the later call's.
#[test]
fn a_batchs_answers_go_back_in_the_order_handed_out() {
    let s = crate::scratch::Scratch::new("codetool-order");
    let (mut ed, ..) = editor(&s);
    let g = generation(&ed);
    ed.workspace.claude.runs_kept.push_back(kept(7, g));
    let read = (0, "read", json!({ "files": ["a.txt"] }));
    let said = run(&mut ed, 1, &[&pending(&[read.clone(), (1, "output", json!("run#7"))]), WAITS, "@"]);
    let said = said.unwrap();
    assert!(said.starts_with("returned: {"), "the second call's answer went back last: {said}");
    assert!(said.contains(r#""lines":["test a ... FAILED","error: 1 failed"]"#), "{said}");
    assert!(said.contains(r#""exit":101"#) && said.contains(r#""ms":1500"#), "{said}");
    // the other way round: the read's answer is the last one given
    let said = run(
        &mut ed,
        2,
        &[&pending(&[(0, "output", json!("run#7")), (1, read.1, read.2.clone())]), WAITS, "@"],
    );
    let said = said.unwrap();
    assert!(said.starts_with("returned:\n== a.txt") && said.contains("1: alpha"), "{said}");
    // the call record names them as the program's, in the order handed out
    let steps = ed.workspace.claude.usage.tally.take_steps();
    let names: Vec<&str> = steps.concat();
    assert_eq!(names, ["code.read", "code.output", "code.output", "code.read"]);
    assert!(ed.workspace.claude.programs.is_empty(), "nothing left once they end");
}

/// Only the read-only tools are bound; what a program may not call, a page, an input that isn't one, a run not
/// kept or another conversation's: each a value it reads, `{ error }`, never the chat's error.
#[test]
fn what_a_program_may_not_call_is_an_error_it_reads() {
    let s = crate::scratch::Scratch::new("codetool-refused");
    let (mut ed, ..) = editor(&s);
    ed.workspace.claude.runs_kept.push_back(kept(8, generation(&ed) + 1));
    let cases: [(&str, Value, &str); 6] = [
        ("run", json!({ "command": "touch made" }), "no function run — a program has read, search,"),
        ("edit", json!({ "edits": [] }), "no function edit"),
        ("read", json!({ "files": ["https://example.com/"] }), "a program reads files, not pages"),
        ("read", json!("a.txt"), "read(input): its input is an object, as the read tool takes it"),
        (
            "output",
            json!("run#99"),
            "run#99 isn't kept: a program reads the whole output of this conversation's",
        ),
        ("output", json!("run#8"), "run#8 isn't kept"),
    ];
    for (n, (name, input, words)) in cases.into_iter().enumerate() {
        let said = run(&mut ed, 10 + n as u64, &[&pending(&[(0, name, input)]), "@"]).unwrap();
        let error = said.strip_prefix("returned: ").and_then(|j| serde_json::from_str::<Value>(j).ok());
        let error = error.as_ref().and_then(|v| v["error"].as_str()).unwrap_or_default().to_string();
        assert!(error.starts_with(words), "{name}: {said}");
    }
    assert!(!s.join("p/made").exists());
}

/// The threat model's line (the plan's §4): a program's read outside the project raises the same card a read of
/// Claude's does, its title saying it's the program asking; the user's `n` is the program's `{ error }`.
#[test]
fn a_programs_read_outside_asks_on_the_same_card_saying_so() {
    let s = crate::scratch::Scratch::new("codetool-card");
    let (mut ed, _, outside) = editor(&s);
    let o = outside.join("o.txt").display().to_string();
    start(&mut ed, 20, &[&pending(&[(0, "read", json!({ "files": [o] }))]), "@"]);
    let card = |ed: &Editor| {
        let read = ed.offers.iter().find(|o| matches!(o.what, crate::offer::What::Read { .. }));
        read.map(|o| o.title.clone())
    };
    settle_until(&mut ed, |ed| card(ed).is_some());
    assert_eq!(card(&ed).unwrap(), crate::reads::SCRIPT_TITLE);
    ed.offer_dismiss();
    settle_until(&mut ed, |_| answered(20).is_some());
    let said = answered(20).unwrap().unwrap();
    assert!(said.starts_with(r#"returned: {"error":"the user chose not to let you read"#), "{said}");
    assert!(!said.contains("outside marker"));
}

/// The turn's end stops a program: one waiting on a card ends at once, saying so (`Watch::stop`).
#[test]
fn the_turns_end_stops_a_program_waiting_on_its_card() {
    let s = crate::scratch::Scratch::new("codetool-turn");
    let (mut ed, _, outside) = editor(&s);
    let o = outside.join("o.txt").display().to_string();
    start(&mut ed, 30, &[&pending(&[(0, "read", json!({ "files": [o] }))]), "@"]);
    settle_until(&mut ed, |ed| ed.offers.iter().any(|o| matches!(o.what, crate::offer::What::Read { .. })));
    let watch = ed.workspace.claude.programs[0].watch.clone();
    crate::tools::turn_over(&mut ed);
    assert_eq!(answered(30), Some(Err("error: stopped: the turn ended".into())));
    assert_eq!(watch.stopped(), Some(super::super::Why::Host("the turn ended".into())));
    assert!(ed.workspace.claude.programs.is_empty());
    // its read's card answered later: nothing waits for it
    crate::reads::turn_over(&mut ed);
    settle_until(&mut ed, |ed| ed.offers.iter().all(|o| !matches!(o.what, crate::offer::What::Read { .. })));
}

/// A run's answer names its id only while the chat lists the tool (off: the answer to the byte), and a session keeps
/// its conversation's last `KEPT` runs.
#[test]
fn a_runs_id_is_named_and_its_output_kept_only_for_the_tool() {
    assert_eq!(named(true, 12, "exit 0 in 1.0s\n$ make\n".into()), "exit 0 in 1.0s · run#12\n$ make\n");
    assert_eq!(named(false, 12, "exit 0 in 1.0s\n$ make\n".into()), "exit 0 in 1.0s\n$ make\n");
    let mut q = VecDeque::new();
    keep_in(&mut q, kept(1, 5));
    for id in 2..12 {
        keep_in(&mut q, kept(id, 6));
    }
    let ids: Vec<u64> = q.iter().map(|k| k.id).collect();
    assert_eq!(ids, (4..12).collect::<Vec<_>>(), "the newest {KEPT}, another conversation's gone");
    let mut ed = Editor::new(crate::config::Config::default());
    keep(&mut ed, kept(1, 1));
    assert!(ed.workspace.claude.runs_kept.is_empty(), "no chat lists the tool: nothing kept");
}

/// On the real engine (where `cargo xtask quickjs` built it): the bindings as Claude reads them in the description —
/// `Promise.all` over a read and a kept run's output, a refused call as a value, a name of the program's own over a
/// binding's.
#[test]
fn the_real_engine_calls_the_bindings_as_described() {
    let Some(engine) = crate::codetool::tests::real() else { return };
    let s = crate::scratch::Scratch::new("codetool-real");
    let (mut ed, ..) = editor(&s);
    ENGINE.with(|e| *e.borrow_mut() = Some(engine));
    BARE.with(|b| b.set(false));
    let g = generation(&ed);
    ed.workspace.claude.runs_kept.push_back(kept(7, g));
    let src = "const [a, run] = await Promise.all([read({ files: ['a.txt'] }), output('run#7')]);\n\
               const no = await tool('run', { command: 'touch made' });\n\
               print(a.split('\\n')[1], run.exit, no.error.slice(0, 15));\n\
               return run.lines.filter((l) => l.includes('FAILED')).length;";
    let call = Call::of(g, "", json!(40)).named("code");
    super::call(&mut ed, call, &json!({ "src": src })).unwrap();
    settle_until(&mut ed, |_| answered(40).is_some());
    assert_eq!(answered(40).unwrap(), Ok("1: alpha 101 no function run\nreturned: 1".into()));
    // a program's own `read` is its own
    let src = "const read = (x) => x * 2; return read(21);";
    super::call(&mut ed, Call::of(g, "", json!(41)).named("code"), &json!({ "src": src })).unwrap();
    settle_until(&mut ed, |_| answered(41).is_some());
    assert_eq!(answered(41).unwrap(), Ok("returned: 42".into()));
    assert!(!s.join("p/made").exists());
}

/// The tool's whole schema, as `tools/list` carries it every request, stays under the plan's 900 tokens (about:
/// `usage::BYTES_PER_TOKEN`; piece d counts it with `chat_prompt_tokens`). Its declarations name every binding.
#[test]
fn the_description_stays_under_its_budget() {
    let listed = crate::tools::annotated(schema()).to_string();
    let about = listed.len() / crate::usage::BYTES_PER_TOKEN;
    assert!(about < 900, "about {about} tokens ({} bytes)", listed.len());
    for b in BINDINGS {
        assert!(DESCRIPTION.contains(&format!("declare function {b}(")), "{b} isn't declared");
    }
    let off = crate::tools::list_with(false, false, false).to_string();
    assert_eq!(off, crate::tools::list(false, false).to_string(), "off: the list to the byte");
    assert!(!off.contains("\"code\""));
    eprintln!("code tool schema: {} bytes, about {about} tokens", listed.len());
}

/// A batch's searches share the walkers one search has (`grep::shared`): taken while one walks, given back after.
#[test]
fn searches_together_share_the_walkers() {
    let free = crate::grep::shared(crate::grep::THREADS, crate::grep::shared_free);
    assert_eq!(free, 0, "a search on all of them leaves none");
    assert_eq!(crate::grep::shared(1, crate::grep::shared_free), crate::grep::THREADS - 1);
    assert_eq!(crate::grep::shared_free(), crate::grep::THREADS, "given back");
}
