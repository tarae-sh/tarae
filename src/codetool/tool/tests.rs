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
    assert_eq!(card(&ed).unwrap(), crate::reads::script_title("Claude"));
    ed.offer_dismiss();
    settle_until(&mut ed, |_| answered(20).is_some());
    let said = answered(20).unwrap().unwrap();
    assert!(said.starts_with(r#"returned: {"error":"the user chose not to let you read"#), "{said}");
    assert!(!said.contains("outside marker"));
}

/// The threat model's line (the plan's §4): a kept output and a tool's answer are data — they reach the program, and
/// Claude only as what the program prints or returns. A marker in a kept run's output: a program that counts its lines
/// answers without it; one that returns the output has it. On the real engine too, where it's built: `print` is the
/// way out.
#[test]
fn a_kept_outputs_words_reach_claude_only_as_the_program_prints_them() {
    const MARKER: &str = "tarae-marker-7f3a";
    let s = crate::scratch::Scratch::new("codetool-marker");
    let (mut ed, ..) = editor(&s);
    let g = generation(&ed);
    let mut k = kept(7, g);
    k.output =
        [(false, format!("test a ... {MARKER}").as_str()), (true, "error: 1 failed")].into_iter().collect();
    ed.workspace.claude.runs_kept.push_back(k);
    let asks = pending(&[(0, "output", json!("run#7"))]);
    let counted = run(&mut ed, 80, &[&asks, "done\n{\"done\":2}"]).unwrap();
    assert_eq!(counted, "returned: 2");
    assert!(!counted.contains(MARKER));
    let returned = run(&mut ed, 81, &[&asks, "@"]).unwrap();
    assert!(returned.contains(MARKER), "returned, it's Claude's: {returned}");

    let Some(engine) = crate::codetool::tests::real() else { return };
    ENGINE.with(|e| *e.borrow_mut() = Some(engine));
    BARE.with(|b| b.set(false));
    let quiet = "const r = await output('run#7'); return r.lines.length;";
    let printed = "const r = await output('run#7'); print(r.lines[0]); return r.lines.length;";
    for (n, src, shown) in [(82, quiet, false), (83, printed, true)] {
        super::call(&mut ed, Call::of(g, "", json!(n)).named("code"), &json!({ "src": src })).unwrap();
        settle_until(&mut ed, |_| answered(n).is_some());
        let said = answered(n).unwrap().unwrap();
        assert_eq!((said.contains(MARKER), said.ends_with("returned: 2")), (shown, true), "{src}: {said}");
    }
}

/// The threat model's line (the plan's §4): `output(id)` gives only what this session's own conversation kept. A run
/// another session's Claude made is kept in that session (its `runs_kept`); asked from this one it's refused, even when
/// the two chats' generations are the same (here both a fresh chat's) — the session's own list is what decides.
#[test]
fn another_sessions_run_is_refused() {
    let s = crate::scratch::Scratch::new("codetool-other");
    let (mut ed, dir, _) = editor(&s);
    let g = generation(&ed);
    let other = ed.workspace_add(dir);
    ed.with_workspace(other, |ed| {
        let mut c = crate::chat::Chat::new();
        c.code = true;
        ed.workspace.claude.chat = Some(c);
        keep(ed, kept(5, generation(ed)));
    });
    let theirs = ed.workspaces.iter().find(|w| w.id == other).unwrap();
    assert_eq!((theirs.claude.runs_kept.len(), theirs.claude.chat.as_ref().unwrap().generation()), (1, g));
    assert!(ed.workspace.claude.runs_kept.is_empty(), "kept in its own session");
    let said = run(&mut ed, 90, &[&pending(&[(0, "output", json!("run#5"))]), "@"]).unwrap();
    assert!(said.starts_with(r#"returned: {"error":"run#5 isn't kept"#), "{said}");
    assert!(!said.contains("FAILED"));
}

/// The design session's call 3 (2026-11-12): a batch's reads outside the project come on one card, once its calls are
/// all judged — complete, never grown after it's shown; it lists each place with its class and offers only `y` (each
/// once) and `n`. A read inside the project in the same batch goes as ever, no card.
#[test]
fn a_batchs_reads_outside_come_on_one_card_and_y_lets_each_through_once() {
    let s = crate::scratch::Scratch::new("codetool-batch");
    let (mut ed, _, outside) = editor(&s);
    let other = outside.parent().unwrap().join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("p.txt"), "other marker\n").unwrap();
    let (o, p) = (outside.join("o.txt").display().to_string(), other.join("p.txt").display().to_string());
    let calls = [
        (0, "read", json!({ "files": [o] })),
        (1, "read", json!({ "files": ["a.txt"] })),
        (2, "read", json!({ "files": [p] })),
    ];
    start(&mut ed, 60, &[&pending(&calls), WAITS, WAITS, "@"]);
    let reads =
        |ed: &Editor| ed.offers.iter().filter(|o| matches!(o.what, crate::offer::What::Read { .. })).count();
    settle_until(&mut ed, |ed| reads(ed) > 0);
    assert_eq!(reads(&ed), 1, "one card for the batch");
    let card = read_card(&ed).unwrap();
    assert_eq!(card.title, "Claude's program wants to read — 2 places outside the project");
    // (each place a line with its class, in the order asked; no warning line on a batch card)
    let lines: Vec<&str> = card.question.lines().take(3).collect();
    assert!(lines[0].starts_with(&format!("{o}  · ")), "{}", card.question);
    assert!(lines[1].starts_with(&format!("{p}  · ")), "{}", card.question);
    assert_eq!(lines[2], "", "{}", card.question);
    assert!(card.warnings.is_empty() && card.warning.is_none(), "{}", card.question);
    assert!(!card.question.contains("a.txt"), "a read in the project asks nothing: {}", card.question);
    assert_eq!(card.answers(false).iter().map(|a| a.0).collect::<String>(), "yn", "each once, or none");
    ed.offer_accept_by('y');
    settle_until(&mut ed, |_| answered(60).is_some());
    // (the last answer given is the third call's: the place the card let through, read)
    let said = answered(60).unwrap().unwrap();
    assert!(said.contains("other marker"), "{said}");
    // let through once: a read of it again asks again
    assert!(!ed.workspace.claude.chat.as_ref().unwrap().allowed.reads(&other), "never kept");
}

fn read_card(ed: &Editor) -> Option<&crate::offer::Offer> {
    ed.offers.iter().find(|o| matches!(o.what, crate::offer::What::Read { .. }))
}

/// The read cards of program `n`, answered as they come — `key` gives each its answer, `y` or `n` — until it ends: each
/// as it came.
fn answer_cards(
    ed: &mut Editor,
    n: u64,
    key: impl Fn(&crate::offer::Offer) -> char,
) -> Vec<crate::offer::Offer> {
    let mut seen = Vec::new();
    loop {
        settle_until(ed, |ed| answered(n).is_some() || read_card(ed).is_some());
        let Some(card) = read_card(ed).cloned() else { return seen };
        match key(&card) {
            'y' => ed.offer_accept_by('y'),
            _ => ed.offer_dismiss(),
        }
        settle_until(ed, |ed| read_card(ed).is_none_or(|o| o.id != card.id));
        seen.push(card);
    }
}

/// The design session's call on the batch card (2026-11-12): a place with a warning never rides in a batch. Of three
/// places outside, the one named like a key asks on a card of its own — today's single card, its title saying so — and
/// the other two come on a batch card of two, which says nothing of keys. Each card's answer is its own: `y` on the
/// batch doesn't let the key's file through, nor does `n` refuse it — whichever card comes first.
#[test]
fn a_place_named_like_a_key_asks_alone_and_the_batchs_answer_isnt_its() {
    let s = crate::scratch::Scratch::new("codetool-batch-keys");
    let (mut ed, _, outside) = editor(&s);
    let base = outside.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(base.join("other")).unwrap();
    std::fs::write(base.join("other/q.txt"), "other marker\n").unwrap();
    let file = |p: PathBuf| p.display().to_string();
    let (o, q) = (file(outside.join("o.txt")), file(base.join("other/q.txt")));
    let batch = |c: &crate::offer::Offer| c.title.contains("places outside the project");
    for (n, on_batch, on_key) in [(61, 'y', 'n'), (62, 'n', 'y')] {
        // (a key of its own each run: an `n` holds for its place until the turn ends)
        let keyed = base.join(format!("keyed{n}"));
        std::fs::create_dir_all(&keyed).unwrap();
        std::fs::write(keyed.join(".env"), "keyed marker\n").unwrap();
        // (the key's read last: what the program returns is its answer)
        let calls = [
            (0, "read", json!({ "files": [o] })),
            (1, "read", json!({ "files": [q] })),
            (2, "read", json!({ "files": [file(keyed.join(".env"))] })),
        ];
        start(&mut ed, n, &[&pending(&calls), WAITS, WAITS, "@"]);
        let cards = answer_cards(&mut ed, n, |c| if batch(c) { on_batch } else { on_key });
        let titles: Vec<&String> = cards.iter().map(|c| &c.title).collect();
        assert_eq!(cards.len(), 2, "a batch card and the key's own: {titles:?}, {:?}", answered(n));
        let (b, single): (Vec<_>, Vec<_>) = cards.iter().partition(|c| batch(c));
        let (b, single) = (b[0], single[0]);
        assert_eq!(b.title, "Claude's program wants to read — 2 places outside the project");
        assert!(b.question.contains(&o) && b.question.contains(&q), "{}", b.question);
        assert!(!b.question.contains(".env") && !b.question.contains("like a key"), "{}", b.question);
        assert_eq!(single.title, crate::reads::script_keys_title("Claude"), "today's single card, by itself");
        assert!(single.question.contains(".env") && !single.question.contains(&o), "{}", single.question);
        // (the program's value is the key's read: what its own card said, not the batch's)
        let said = answered(n).unwrap().unwrap();
        match on_key {
            'y' => assert!(said.contains("keyed marker"), "its own y read it: {said}"),
            _ => assert!(
                !said.contains("keyed marker") && said.contains("the user chose not to let"),
                "the batch's y didn't let it through: {said}"
            ),
        }
    }
}

/// …and a place with a token-like line asks alone too, its warning line on its own card in the warning colour (names,
/// never values); the batch card of the rest has none.
#[test]
fn a_place_with_a_token_asks_alone_with_its_warning() {
    let s = crate::scratch::Scratch::new("codetool-batch-token");
    let (mut ed, _, outside) = editor(&s);
    let (other, third) = (outside.parent().unwrap().join("other"), outside.parent().unwrap().join("third"));
    std::fs::create_dir_all(&other).unwrap();
    std::fs::create_dir_all(&third).unwrap();
    // (the fake, never a real one)
    std::fs::write(other.join("p.txt"), "other marker\npassword = tarae-fake-token\n").unwrap();
    std::fs::write(third.join("r.txt"), "third marker\n").unwrap();
    let file = |p: PathBuf| p.display().to_string();
    let (o, p, r) = (file(outside.join("o.txt")), file(other.join("p.txt")), file(third.join("r.txt")));
    let calls = [
        (0, "read", json!({ "files": [o] })),
        (1, "read", json!({ "files": [p] })),
        (2, "read", json!({ "files": [r] })),
    ];
    start(&mut ed, 63, &[&pending(&calls), WAITS, WAITS, "@"]);
    let cards = answer_cards(&mut ed, 63, |_| 'y');
    assert_eq!(cards.len(), 2, "{cards:#?}");
    let (b, single): (Vec<_>, Vec<_>) =
        cards.iter().partition(|c| c.title.contains("places outside the project"));
    let (b, single) = (b[0], single[0]);
    assert!(b.question.contains(&o) && b.question.contains(&r) && !b.question.contains(&p), "{}", b.question);
    assert!(b.warnings.is_empty() && b.warning.is_none(), "{}", b.question);
    assert_eq!(single.title, crate::reads::script_title("Claude"));
    let said = format!("{} holds a token-like line.", crate::allow::tilde(&other.join("p.txt")));
    let warned: Vec<&str> = single
        .question
        .lines()
        .enumerate()
        .filter(|(i, _)| single.warning.as_ref().is_some_and(|w| w.contains(i)))
        .map(|(_, l)| l)
        .collect();
    assert_eq!(warned, [said.as_str()], "{}", single.question);
    assert!(!single.question.contains("tarae-fake-token"), "names, never values");
}

/// The naming landing's rule 1 on the program card (read-program, which no shot reaches: the shot's build has no code
/// engine — `Engine::here`): on the stand-in's row, each card a program's reads raise — a single read outside, one named
/// like a key, a batch of two — is `reads::next`'s, its title and words (`question`, `batch_text`) saying `stand-in`
/// and no Claude.
#[test]
fn the_program_card_names_its_rows_agent() {
    let s = crate::scratch::Scratch::new("codetool-card-row");
    let (mut ed, _, outside) = editor(&s);
    let a = crate::agent::STAND_IN.shown;
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    (c.agent, c.agent_fixed) = (&crate::agent::STAND_IN, true);
    let base = outside.parent().unwrap().to_path_buf();
    for d in ["other", "single", "keyed"] {
        std::fs::create_dir_all(base.join(d)).unwrap();
    }
    std::fs::write(base.join("other/q.txt"), "other marker\n").unwrap();
    std::fs::write(base.join("single/s.txt"), "single marker\n").unwrap();
    std::fs::write(base.join("keyed/.env"), "keyed marker\n").unwrap();
    let file = |p: PathBuf| p.display().to_string();
    // (a single read outside: its card alone)
    start(
        &mut ed,
        66,
        &[&pending(&[(0, "read", json!({ "files": [file(base.join("single/s.txt"))] }))]), "@"],
    );
    let mut cards = answer_cards(&mut ed, 66, |_| 'n');
    // (two places and a key's file together: the batch card of two, and the key's own)
    let calls = [
        (0, "read", json!({ "files": [file(outside.join("o.txt"))] })),
        (1, "read", json!({ "files": [file(base.join("other/q.txt"))] })),
        (2, "read", json!({ "files": [file(base.join("keyed/.env"))] })),
    ];
    start(&mut ed, 67, &[&pending(&calls), WAITS, WAITS, "@"]);
    cards.extend(answer_cards(&mut ed, 67, |_| 'n'));
    let titles: Vec<&str> = cards.iter().map(|c| c.title.as_str()).collect();
    let batch = format!("{a}'s program wants to read — 2 places outside the project");
    let want = [crate::reads::script_title(a), batch, crate::reads::script_keys_title(a)];
    let mut sorted = titles.clone();
    sorted.sort_by_key(|t| want.iter().position(|w| w == t));
    assert_eq!(sorted, want, "each card once, by its builder: {titles:?}");
    for c in &cards {
        let said = format!("{}\n{}", c.title, c.question);
        assert!(said.contains(a) && !said.contains("Claude"), "not the stand-in's words:\n{said}");
    }
    // (each body is its builder's: `question` for a single read, `batch_text` for the batch)
    for c in &cards {
        let crate::offer::What::Read { pending, keep, .. } = &c.what else { panic!("a read card") };
        let built = match pending.more.is_empty() {
            true => crate::reads::question(a, &pending.look, *keep, None),
            false => crate::reads::batch_text(a, pending),
        };
        assert!(c.question.starts_with(&built), "{}:\n{}\n— built —\n{built}", c.title, c.question);
    }
}

/// …and a place where keys are (`Class::Secret`), though nothing in it is named like a key: a scratch home's
/// `.ssh/config` asks alone, its class on its own card; the batch card of the rest says nothing of it.
#[test]
fn a_place_where_keys_are_asks_alone_though_not_named_like_one() {
    let s = crate::scratch::Scratch::new("codetool-batch-secret");
    let (mut ed, _, outside) = editor(&s);
    let base = outside.parent().unwrap().to_path_buf();
    let (third, home) = (base.join("third"), base.join("home"));
    std::fs::create_dir_all(&third).unwrap();
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::write(third.join("r.txt"), "third marker\n").unwrap();
    std::fs::write(home.join(".ssh/config"), "ssh marker\n").unwrap();
    ed.caps_home = Some(home.clone());
    let config = home.join(".ssh/config");
    assert!(!crate::secretnames::named_secret(&config), "not named like a key: only its place is Secret");
    let file = |p: &std::path::Path| p.display().to_string();
    let (o, r, c) = (file(&outside.join("o.txt")), file(&third.join("r.txt")), file(&config));
    let calls = [
        (0, "read", json!({ "files": [o] })),
        (1, "read", json!({ "files": [r] })),
        (2, "read", json!({ "files": [c] })),
    ];
    start(&mut ed, 64, &[&pending(&calls), WAITS, WAITS, "@"]);
    let cards = answer_cards(&mut ed, 64, |_| 'y');
    assert_eq!(cards.len(), 2, "{cards:#?}");
    let (b, single): (Vec<_>, Vec<_>) =
        cards.iter().partition(|c| c.title.contains("places outside the project"));
    let (b, single) = (b[0], single[0]);
    assert!(
        b.question.contains(&o) && b.question.contains(&r) && !b.question.contains(".ssh"),
        "{}",
        b.question
    );
    assert!(!b.question.contains("where keys are"), "{}", b.question);
    assert_eq!(single.title, crate::reads::script_title("Claude"), "today's single card, by itself");
    assert!(single.question.contains(".ssh") && !single.question.contains(&o), "{}", single.question);
    // (its class on the card: what it asks, for its `card` line — today's single card's text names no class)
    let asked = single.asked.as_ref().map(|a| &a.request);
    assert!(matches!(asked, Some(crate::cards::Request::Read(crate::cards::Class::Secret))), "{asked:?}");
    let crate::offer::What::Read { pending, .. } = &single.what else { panic!("a read card") };
    assert!(
        pending.class == crate::cards::Class::Secret && pending.look.keys.is_empty(),
        "{:?}",
        pending.class
    );
    let said = answered(64).unwrap().unwrap();
    assert!(said.contains("ssh marker"), "its own y read it: {said}");
}

/// A batch card cuts a search's long pattern as a single card does, and its `v` shows it whole after its place — any
/// place's, not only the first (the design session's long-place scene, 2026-11-12).
#[test]
fn a_batch_cards_cut_pattern_is_whole_on_its_v_whichever_place_it_is() {
    let s = crate::scratch::Scratch::new("codetool-batch-pattern");
    let (mut ed, _, outside) = editor(&s);
    let other = outside.parent().unwrap().join("other");
    std::fs::create_dir_all(&other).unwrap();
    let o = outside.join("o.txt").display().to_string();
    let long = "(allowWrite|denyWrite|allowRead|denyRead)\\s*[:=]\\s*\\[.*(home|tmp|cache|ssh|keychain)";
    let calls = [
        (0, "read", json!({ "files": [o] })),
        (1, "search", json!({ "pattern": long, "path": other.display().to_string() })),
    ];
    start(&mut ed, 61, &[&pending(&calls), WAITS, "@"]);
    let card =
        |ed: &Editor| ed.offers.iter().find(|o| matches!(o.what, crate::offer::What::Read { .. })).cloned();
    settle_until(&mut ed, |ed| card(ed).is_some());
    let card = card(&ed).unwrap();
    assert!(card.question.contains("…"), "cut on the card: {}", card.question);
    assert_eq!(card.answers(false).iter().map(|a| a.0).collect::<String>(), "yvn");
    let details = card.read_details().expect("its `v`");
    let said = format!("\nThe whole pattern searched for in {}:\n\n{long}\n", other.display());
    assert_eq!(details, said);
}

/// A card a program asked on goes with it: stopped by the user's `C-c` (`STOPPED_BY_YOU`), the card is dropped, the
/// program answers saying who stopped it, and its row says how it ended, its calls under it.
#[test]
fn the_users_stop_ends_a_program_its_card_and_its_row_says_so() {
    let s = crate::scratch::Scratch::new("codetool-stop");
    let (mut ed, _, outside) = editor(&s);
    let o = outside.join("o.txt").display().to_string();
    let src = script(&[&pending(&[(0, "read", json!({ "files": [o] }))]), "@"]);
    let input = json!({ "src": src });
    {
        let c = ed.workspace.claude.chat.as_mut().unwrap();
        c.calls.push(crate::chat::Called {
            id: "t1".into(),
            name: "mcp__tarae__code".into(),
            input: input.clone(),
        });
        let look = crate::chat::look(&ed.workspace.root, "mcp__tarae__code", &input);
        c.msgs.push(crate::chat::Msg::tool(look, "t1"));
    }
    let call = Call::of(generation(&ed), "", json!(70)).named("code");
    super::call(&mut ed, call, &input).unwrap();
    let row = |ed: &Editor| {
        let m = ed.workspace.claude.chat.as_ref().unwrap().msgs.iter().find(|m| m.call == "t1").unwrap();
        (m.look.clone().unwrap(), m.program.clone().unwrap())
    };
    assert!(row(&ed).1.output.is_none(), "running: its last row says how to stop it (`term::program_rows`)");
    settle_until(&mut ed, |ed| ed.offers.iter().any(|o| matches!(o.what, crate::offer::What::Read { .. })));
    assert_eq!(row(&ed).1.calls.len(), 1, "its call's row is under it");
    stop_all(&mut ed, STOPPED_BY_YOU);
    assert_eq!(answered(70), Some(Err(format!("error: stopped: {STOPPED_BY_YOU}"))));
    assert!(
        ed.offers.iter().all(|o| !matches!(o.what, crate::offer::What::Read { .. })),
        "its card went with it"
    );
    let (look, shown) = row(&ed);
    assert!(look.failed(), "{}", look.range);
    // (a stop, not a failure: `stopped`, the stop's words after it; under it the output without `error: ` — the `▲`
    // says it)
    assert_eq!(look.range, format!("{STOPPED}{STOPPED_BY_YOU}"));
    assert_eq!(shown.output.as_deref(), Some(format!("stopped: {STOPPED_BY_YOU}").as_str()));
}

/// Keys, as the user presses them: a program running mid-turn, waiting on its card (which came on its own: the chat keeps
/// the keys). Its row says `→ running` with no key; the chat's foot `C-c stop program`. The first `C-c` stops the
/// program — Claude told, the turn going on, no interrupt sent, its row `stopped · …`, the foot back to `C-c stop`; the
/// second stops the turn.
#[test]
fn c_c_in_the_chat_stops_the_program_then_the_turn_and_the_rows_say_so() {
    let s = crate::scratch::Scratch::new("codetool-keys");
    let (mut ed, _, outside) = editor(&s);
    let o = outside.join("o.txt").display().to_string();
    let input = json!({ "src": script(&[&pending(&[(0, "read", json!({ "files": [o] }))]), "@"]) });
    let look = crate::chat::look(&ed.workspace.root, "mcp__tarae__code", &input);
    let claude = {
        let c = ed.workspace.claude.chat.as_mut().unwrap();
        let claude = crate::chat::stand_in(c);
        (c.hidden, c.state) = (false, crate::chat::State::Writing);
        c.calls.push(crate::chat::Called {
            id: "t1".into(),
            name: "mcp__tarae__code".into(),
            input: input.clone(),
        });
        c.msgs.push(crate::chat::Msg::tool(look, "t1"));
        claude
    };
    let call = Call::of(generation(&ed), "", json!(95)).named("code");
    super::call(&mut ed, call, &input).unwrap();
    settle_until(&mut ed, |ed| ed.offers.iter().any(|o| matches!(o.what, crate::offer::What::Read { .. })));
    let screen = |ed: &mut Editor| {
        let mut bytes = Vec::new();
        crate::term::render(ed, &mut bytes, 140, 30).unwrap();
        let mut p = vt100::Parser::new(30, 140, 0);
        p.process(&bytes);
        p.screen().contents()
    };
    let c_c = |ed: &mut Editor| ed.handle_key("C-c".parse::<crate::key::Key>().unwrap());
    let s = screen(&mut ed);
    assert!(s.contains(&format!("→ {RUNNING}")) && !s.contains("C-c stops"), "its row, no key: {s}");
    assert!(s.contains("C-c stop program"), "the foot: {s}");

    c_c(&mut ed);
    assert_eq!(answered(95), Some(Err(format!("error: stopped: {STOPPED_BY_YOU}"))));
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert_eq!(
        (ed.workspace.claude.programs.len(), c.busy(), c.stopping),
        (0, true, false),
        "the turn goes on"
    );
    assert_eq!(claude.try_iter().filter(|l| l.contains("interrupt")).count(), 0);
    let s = screen(&mut ed);
    assert!(s.contains(STOPPED) && !s.contains(&format!("→ {RUNNING}")), "its row: {s}");
    assert!(s.contains("C-c stop") && !s.contains("stop program"), "the foot: {s}");

    c_c(&mut ed);
    assert!(ed.workspace.claude.chat.as_ref().unwrap().stopping, "the second stops the turn");
    assert_eq!(claude.try_iter().filter(|l| l.contains("interrupt")).count(), 1);
}

/// A program's row says `stopped` for a limit or the host's stop, the stop's kind right after it, and `failed` only when
/// the program threw (the design session, 2026-11-12: the limit's row hid the limit).
#[test]
fn a_programs_row_says_stopped_for_a_limit_and_failed_for_a_throw() {
    let time = crate::codetool::time_limit(std::time::Duration::from_secs(10));
    assert_eq!(
        row_end(&time, true),
        "stopped · at its time limit (10.0s of running): do less in one program, or split it"
    );
    assert_eq!(row_end(&format!("stopped: {STOPPED_BY_YOU}"), true), format!("stopped · {STOPPED_BY_YOU}"));
    assert_eq!(
        row_end("TypeError: x is undefined\n  at line 2", false),
        "failed · TypeError: x is undefined"
    );
    // (a thrown string that reads like a stop is still a failure: what decides is how it ended, not its words)
    assert_eq!(row_end("stopped at its time limit", false), "failed · stopped at its time limit");
    for range in [row_end(&time, true), row_end("Error: no", false)] {
        let look = crate::chat::Look { verb: "code", what: "x".into(), pattern: true, range, link: None };
        assert!(look.failed(), "▲ either way: {}", look.range);
    }
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

/// The tool's whole schema, as `tools/list` carries it every request, stays under the plan's 900 tokens as billed.
/// `usage::BYTES_PER_TOKEN`'s 4 undercounts this text — TypeScript declarations tokenize densely — so bytes are
/// turned into tokens at the ratio the last live pair measured: 2026-11-12, on this text (2,279 bytes),
/// `chat_prompt_tokens` 11,459 → `chat_prompt_tokens_with_code` 12,285, 826 tokens billed (2.76 bytes a token). The
/// run before it, the same day, on the text before the cut (2,611 bytes): 11,459 → 12,386, 927 tokens, where 4 said
/// 652. A change to the description runs the live pair again and puts its figures here. Its declarations name every
/// binding.
#[test]
fn the_description_stays_under_its_budget() {
    const LIVE_BYTES: usize = 2279;
    const LIVE_TOKENS: usize = 826;
    let listed = crate::tools::annotated(schema()).to_string();
    let billed = (listed.len() * LIVE_TOKENS).div_ceil(LIVE_BYTES);
    assert!(billed < 900, "{billed} tokens as billed ({} bytes, 2026-11-12's ratio)", listed.len());
    for b in BINDINGS {
        assert!(DESCRIPTION.contains(&format!("declare function {b}(")), "{b} isn't declared");
    }
    let off = crate::tools::list_with(false, false, false).to_string();
    assert_eq!(off, crate::tools::list(false, false).to_string(), "off: the list to the byte");
    assert!(!off.contains("\"code\""));
    eprintln!("code tool schema: {} bytes, {billed} tokens as billed", listed.len());
}

/// The design session's point 5 (2026-11-11): eight reads and four searches handed out together, against the same twelve
/// one by one (a call a step), in wall time on this checkout's own `src/` (read only; no card: all inside). Median of 5
/// each, alternated; reported only, as `perf_code_engine_dispatch` is. On the stand-in engine: the time is the tools'
/// and the host's, not QuickJS's.
#[test]
fn perf_code_batch_together_against_one_by_one() {
    if crate::test_results::unoptimized("codetool::tool::tests::perf_code_batch_together_against_one_by_one")
    {
        return;
    }
    let s = crate::scratch::Scratch::new("codetool-batch-perf");
    let (mut ed, ..) = editor(&s);
    ed.workspace.root = crate::paths::real(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let files =
        ["term.rs", "chat.rs", "editor.rs", "tools.rs", "sandbox.rs", "llm.rs", "reads.rs", "offer.rs"];
    let mut calls: Vec<(u32, &str, Value)> = (files.iter().enumerate())
        .map(|(i, f)| (i as u32, "read", json!({ "files": [format!("src/{f}")] })))
        .collect();
    for (i, p) in ["fn ", "Editor", "unwrap\\(", "clock::now"].into_iter().enumerate() {
        calls.push((8 + i as u32, "search", json!({ "pattern": p, "path": "src", "output": "count" })));
    }
    let together: Vec<String> =
        std::iter::once(pending(&calls)).chain((1..calls.len()).map(|_| WAITS.to_string())).collect();
    let apart: Vec<String> = calls.iter().map(|c| pending(std::slice::from_ref(c))).collect();
    let mut n = 200;
    let mut time = |steps: &[String]| {
        let mut steps: Vec<&str> = steps.iter().map(String::as_str).collect();
        steps.push("@");
        n += 1;
        let t = std::time::Instant::now();
        let said = run(&mut ed, n, &steps);
        let t = t.elapsed();
        assert!(said.as_ref().is_ok_and(|s| s.starts_with("returned:")), "{said:?}");
        t
    };
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for _ in 0..5 {
        a.push(time(&together));
        b.push(time(&apart));
    }
    let median = |mut v: Vec<std::time::Duration>| {
        v.sort_unstable();
        v[v.len() / 2].as_secs_f64() * 1e3
    };
    let (a, b) = (median(a), median(b));
    let line = format!(
        "PERF code_batch_together_ms {a:.1} noise=1\nPERF code_batch_one_by_one_ms {b:.1} noise=1\n\
         code batch: together {a:.1} ms, one by one {b:.1} ms ({:.2}×)\n",
        b / a
    );
    let _ = std::io::Write::write_all(&mut std::io::stderr(), line.as_bytes());
}

/// A batch's searches share the walkers one search has (`grep::shared`): taken while one walks, given back after.
#[test]
fn searches_together_share_the_walkers() {
    let free = crate::grep::shared(crate::grep::THREADS, crate::grep::shared_free);
    assert_eq!(free, 0, "a search on all of them leaves none");
    assert_eq!(crate::grep::shared(1, crate::grep::shared_free), crate::grep::THREADS - 1);
    assert_eq!(crate::grep::shared_free(), crate::grep::THREADS, "given back");
}
