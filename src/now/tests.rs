use serde_json::json;

use super::*;
use crate::chat::{Msg, Role};
use crate::llm::StreamEv;

/// An editor whose chat's turn is under way — a stand-in claude (no process) asked "keep the first strand", nothing of
/// its answer yet.
fn at_work() -> Editor {
    let mut ed = Editor::new(crate::config::Config::default());
    ed.docs[0].text = ropey::Rope::from_str("fn main() {}\n");
    let mut c = Chat::new();
    c.msgs = vec![Msg::new(Role::User, "earlier", "")];
    drop(crate::chat::stand_in(&mut c));
    ed.workspace.claude.chat = Some(c);
    assert!(crate::chat::submit(&mut ed, "keep the first strand", None, None), "asked");
    sync(&mut ed);
    ed
}

/// An event of the turn's stream, as the main loop applies it — `sync` after it, as after every event.
fn send(ed: &mut Editor, ev: StreamEv) {
    let g = ed.workspace.claude.chat.as_ref().map_or(0, Chat::generation);
    crate::chat::stream_event(ed, g, ev);
    sync(ed);
}

/// Claude calls tarae's `name` with `input` (its tool_use block), as call `id`.
fn call(ed: &mut Editor, id: &str, name: &str, input: serde_json::Value) {
    send(ed, StreamEv::Tool { id: id.into(), name: format!("mcp__tarae__{name}"), input });
}

/// The step as the line says it: its verb and what it acts on.
fn said_step(ed: &Editor) -> String {
    let s = step(ed, &ed.workspace).expect("a step");
    format!("{} {}", s.verb, s.what).trim().to_string()
}

/// A Claude card of the chat's up (its call waits on it).
fn card(ed: &mut Editor) {
    let what = crate::offer::What::Asks { call: crate::tools::Call::test(1) };
    ed.push_offer(crate::offer::Offer::new("Claude asks".into(), "?", "", String::new(), what));
    sync(ed);
}

/// The status line — the frame's last row — drawn `w` × `h`.
fn status(ed: &mut Editor, w: u16, h: u16) -> String {
    crate::term::render(ed, &mut Vec::new(), w, h).unwrap();
    let (frame, fw, fh) = ed.last_frame.as_ref().unwrap();
    let text = crate::screen::text(frame, *fw, *fh).unwrap_or_default();
    text.lines().last().unwrap_or_default().trim_end().to_string()
}

/// The frame's rows drawn `w` × `h`.
fn rows(ed: &mut Editor, w: u16, h: u16) -> Vec<String> {
    crate::term::render(ed, &mut Vec::new(), w, h).unwrap();
    let (frame, fw, fh) = ed.last_frame.as_ref().unwrap();
    crate::screen::text(frame, *fw, *fh).unwrap_or_default().lines().map(str::to_string).collect()
}

/// Every row of the plan's table (`.sb/now-plan.md` A.3), each from a stand-in turn as its stream brings it: the
/// verb, what it acts on, its sign; what waits on you in warning. Nothing of a turn: none.
#[test]
fn each_step_says_what_claude_is_doing() {
    let mut ed = Editor::new(crate::config::Config::default());
    assert_eq!(step(&ed, &ed.workspace), None, "no chat");
    ed = at_work();
    let s = step(&ed, &ed.workspace).unwrap();
    assert_eq!((s.verb, s.sign), ("thinking", Sign::Working), "asked, nothing back yet");
    // A call in flight, each kind
    let calls: &[(&str, serde_json::Value, &str, Mark)] = &[
        ("run", json!({"command": "cargo test -q"}), "running cargo test -q", Mark::Run),
        ("read", json!({"files": ["src/main.rs:120-180"]}), "reading src/main.rs:120-180", Mark::Read),
        ("search", json!({"pattern": "fn draw_chat"}), "searching fn draw_chat", Mark::Lookup),
        (
            "references",
            json!({"path": "src/main.rs", "symbol": "draw_chat"}),
            "looking up draw_chat",
            Mark::Lookup,
        ),
        ("definition", json!({"symbol": "Thread"}), "looking up Thread", Mark::Lookup),
        ("plan", json!({"title": "Keep it", "steps": []}), "planning Keep it", Mark::Lookup),
    ];
    for (name, input, says, mark) in calls {
        let mut ed = at_work();
        call(&mut ed, "t1", name, input.clone());
        let s = step(&ed, &ed.workspace).unwrap();
        assert_eq!(said_step(&ed), *says, "{name}: whole — what it acts on and where");
        assert_eq!((s.sign, s.warn), (Sign::Mark(*mark), false), "{name}");
        // (its result back: the call's step is over — Claude's next answer on its way, the wave)
        send(&mut ed, StreamEv::ToolResult { id: "t1".into(), text: "ok".into(), error: false });
        let s = step(&ed, &ed.workspace).unwrap();
        assert_eq!((s.verb, s.sign), ("thinking", Sign::Working), "{name} answered");
    }
    // Tests run
    let mut ed = at_work();
    call(&mut ed, "t1", "run_tests", json!({"path": "src/main.rs"}));
    assert!(said_step(&ed).starts_with("testing"), "{}", said_step(&ed));
    // An edit's input streaming, its call not made yet: the file it writes
    let mut ed = at_work();
    let piece =
        |field, add: &str, closed| crate::writing::Piece::Text { item: 0, field, add: add.into(), closed };
    let pieces = vec![
        piece(crate::writing::Field::File, "src/main.rs", true),
        piece(crate::writing::Field::Old, "fn", false),
    ];
    send(&mut ed, StreamEv::Writing(crate::writing::Delta { call: "t1".into(), pieces, done: false }));
    let s = step(&ed, &ed.workspace).unwrap();
    assert_eq!((said_step(&ed).as_str(), s.sign), ("writing src/main.rs", Sign::Mark(Mark::Edit)));
    // Thinking, prose arriving
    let mut ed = at_work();
    send(&mut ed, StreamEv::ThinkingBlock(true));
    assert_eq!(step(&ed, &ed.workspace).unwrap().sign, Sign::Working, "a think");
    send(&mut ed, StreamEv::ThinkingBlock(false));
    send(&mut ed, StreamEv::Text("Let me look".into()));
    let s = step(&ed, &ed.workspace).unwrap();
    assert_eq!((s.verb, s.sign), ("writing", Sign::Working), "words arriving");
    // Waiting on you: a card of its up, an edit in its review
    let waits: &[(&str, serde_json::Value, &str)] = &[
        (
            "run",
            json!({"command": "cargo test --quiet", "outside_sandbox": true}),
            "asks to run cargo test --quiet",
        ),
        ("read", json!({"files": ["/etc/hosts"]}), "asks to read /etc/hosts"),
        (
            "edit",
            json!({"edits": [{"file_path": "src/a.rs"}, {"file_path": "src/b.rs"}]}),
            "asks to change src/a.rs +1 more",
        ),
    ];
    for (name, input, says) in waits {
        let mut ed = at_work();
        call(&mut ed, "t1", name, input.clone());
        card(&mut ed);
        let s = step(&ed, &ed.workspace).unwrap();
        assert_eq!(said_step(&ed), *says, "{name}");
        assert!(s.warn && s.word() == "waiting", "{name}: in warning, `waiting` on the top lines");
    }
    let mut ed = at_work();
    call(&mut ed, "t1", "edit", json!({"edits": [{"file_path": "src/main.rs"}]}));
    let doc = ed.doc().id;
    crate::llm::review_with(
        &mut ed,
        doc,
        "keep it",
        vec![(0, 1, "fn main() {}".into(), "fn main() { }".into())],
        None,
    );
    sync(&mut ed);
    assert_eq!(said_step(&ed), "waits for your review of src/main.rs");
    // A call past its time: possibly stuck, its time the call's own
    let mut ed = at_work();
    call(&mut ed, "t1", "symbols", json!({"path": "src/main.rs"}));
    let g = ed.workspace.claude.chat.as_ref().unwrap().generation();
    ed.tool_waits(g, json!(7), "symbols", Duration::ZERO);
    std::thread::sleep(Duration::from_millis(5));
    sync(&mut ed);
    let s = step(&ed, &ed.workspace).unwrap();
    assert_eq!((said_step(&ed).as_str(), s.warn, s.took.is_some()), ("stuck? symbols", true, true));
    // Compacting; claude starting
    let mut ed = at_work();
    ed.workspace.claude.compacted.since = Some(crate::clock::now());
    assert_eq!(step(&ed, &ed.workspace).unwrap().verb, "compacting");
    let mut ed = at_work();
    ed.workspace.claude.chat.as_mut().unwrap().settings.reading_since(crate::clock::now());
    assert_eq!(step(&ed, &ed.workspace).unwrap().verb, "starting");
    // The turn over: none
    let mut ed = at_work();
    send(&mut ed, StreamEv::Done(Ok("done".into())));
    assert_eq!(step(&ed, &ed.workspace), None);
}

/// The top lines say the step's word — the chat's and the follow card's head — never `exploring`: a run reads
/// `running` there as on the status line.
#[test]
fn the_top_lines_say_the_steps_word() {
    let mut ed = at_work();
    call(&mut ed, "t1", "run", json!({"command": "cargo test -q"}));
    ed.workspace.claude.chat.as_mut().unwrap().follow.on = false;
    let rows = rows(&mut ed, 160, 40);
    assert!(rows[0].contains("running") && !rows.iter().any(|r| r.contains("exploring")), "{rows:#?}");
    assert!(rows[39].contains("▸ running cargo test -q"), "{}", rows[39]);
}

/// The last finished sentence: a sentence ends at its stop before a blank, or with its paragraph; code blocks aren't
/// sentences; emphasis is taken off; the one being written isn't one yet.
#[test]
fn the_last_sentence_is_the_last_finished_one() {
    let text = "A repeated name now keeps the first strand. I will run the **unit tests** to check `it`.";
    assert_eq!(last_sentence(text, true).as_deref(), Some("I will run the unit tests to check it."));
    assert_eq!(last_sentence(text, false).as_deref(), Some("A repeated name now keeps the first strand."));
    assert_eq!(last_sentence("Let me look", false), None, "none finished yet");
    assert_eq!(last_sentence("Let me look", true).as_deref(), Some("Let me look"), "whole: it ended");
    let code = "Keyed by name:\n\n```rust\nthreads.insert(a, b);\n```\n\n";
    assert_eq!(
        last_sentence(code, false).as_deref(),
        Some("Keyed by name:"),
        "a code block is no one's sentence"
    );
    let list = "Two ways:\n- `insert` replaces the value\n- `entry` keeps";
    assert_eq!(
        last_sentence(list, false).as_deref(),
        Some("insert replaces the value"),
        "an item ends with its line"
    );
    let wrapped = "I will run the unit tests\nto check it. Then";
    assert_eq!(last_sentence(wrapped, false).as_deref(), Some("I will run the unit tests to check it."));
}

/// A soft line break ends no sentence: one wrapped over lines doesn't show its first half as finished — a paragraph
/// ends at a blank line, a code fence or a new block.
#[test]
fn a_soft_line_break_ends_no_sentence() {
    let half = last_sentence("Done here. I will run the unit tests\n", false);
    assert_eq!(half.as_deref(), Some("Done here."), "half a sentence over a line break isn't one");
    assert_eq!(last_sentence("I will run the unit tests\n", false), None, "none finished yet");
    let stop = last_sentence("I will run the unit tests.\n", false);
    assert_eq!(
        stop.as_deref(),
        Some("I will run the unit tests."),
        "its stop ends it, a line break after it"
    );
}

/// A code fence opening ends the paragraph before it, while the code block still streams — as a blank line and the
/// next list item do.
#[test]
fn a_fence_ends_the_paragraph_before_it() {
    let fence = last_sentence("Keyed by name\n```rust\nlet x = ", false);
    assert_eq!(fence.as_deref(), Some("Keyed by name"), "closed by the fence while the block streams");
    let blank = last_sentence("Keyed by name\n\nThen", false);
    assert_eq!(blank.as_deref(), Some("Keyed by name"), "closed by a blank line");
    let item = last_sentence("- insert replaces it\n- entry keeps", false);
    assert_eq!(item.as_deref(), Some("insert replaces it"), "closed by the next item");
}

/// Claude's sentence on the line changes only as one ends, never with each token — and its thinking's says the why
/// until its prose's first sentence ends (italic on the line).
#[test]
fn the_sentence_changes_only_when_one_ends() {
    let mut ed = at_work();
    let said_now = |ed: &Editor| said(ed.workspace.claude.chat.as_ref().unwrap());
    send(&mut ed, StreamEv::ThinkingBlock(true));
    send(&mut ed, StreamEv::Thought("Checking the map's keys. Then the".into()));
    assert_eq!(
        said_now(&ed),
        Some(("Checking the map's keys.".into(), true)),
        "its thinking's, before any prose"
    );
    send(&mut ed, StreamEv::Thought(" clone.".into()));
    send(&mut ed, StreamEv::ThinkingBlock(false));
    assert_eq!(said_now(&ed), Some(("Then the clone.".into(), true)), "the block closed: its last one ended");
    send(&mut ed, StreamEv::Text("A repeated name now keeps".into()));
    assert_eq!(said_now(&ed), Some(("Then the clone.".into(), true)), "prose under way: still the thought's");
    let mut seen = Vec::new();
    for t in [" the first", " strand.", " I will", " run the", " unit tests.", "\n"] {
        send(&mut ed, StreamEv::Text(t.into()));
        let s = said_now(&ed).map(|s| s.0).unwrap_or_default();
        if seen.last() != Some(&s) {
            seen.push(s);
        }
    }
    assert_eq!(
        seen,
        ["Then the clone.", "A repeated name now keeps the first strand.", "I will run the unit tests."],
        "one change a sentence"
    );
    // On the line: the thought's in italic
    let mut ed = at_work();
    send(&mut ed, StreamEv::Thought("Checking the map's keys. ".into()));
    let line = status(&mut ed, 160, 40);
    let at = line.find("Checking").expect("the sentence on the line");
    let mut vt = vt100::Parser::new(40, 160, 0);
    vt.process(&ed.last_frame.as_ref().unwrap().0);
    let cell =
        vt.screen().cell(39, line[..at].width() as u16).map(|c| (c.contents().to_string(), c.italic()));
    assert_eq!(cell, Some(("C".to_string(), true)), "a thought's sentence in italic");
}

/// A turn's line — the long sentence, a plan — at 160, 100 and 80 columns: the plan's step folds first, then the
/// sentence is cut, then gone; the step and its time stay; the mode word is first.
#[test]
fn the_line_folds_the_plan_then_the_sentence() {
    let mut ed = at_work();
    let steps = json!([{"text": "Read how the map is filled", "status": "done"},
        {"text": "Run the unit tests", "status": "doing"}]);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.plan = Some(crate::plan::Plan::from_json(&json!({"title": "Keep", "steps": steps})).unwrap());
    c.follow.on = false;
    send(&mut ed, StreamEv::Text("A repeated name now keeps the first strand. I will run the unit tests to check that nothing else relied on the old replace.\n".into()));
    call(&mut ed, "t1", "run", json!({"command": "cargo test -q"}));
    let whole = "I will run the unit tests to check that nothing else relied on the old replace.";
    let wide = status(&mut ed, 160, 40);
    assert!(wide.contains("▸ running cargo test -q") && wide.contains("› 2/2 Run the unit tests"), "{wide}");
    assert!(wide.contains(&whole[..60]), "{wide}");
    let mid = status(&mut ed, 100, 30);
    assert!(mid.contains("▸ running cargo test -q") && !mid.contains("› 2/2"), "the plan folds first: {mid}");
    assert!(
        mid.contains("— I will run") && mid.contains('…') && !mid.contains(whole),
        "the sentence cut: {mid}"
    );
    let narrow = status(&mut ed, 80, 24);
    assert!(narrow.contains("▸ running cargo test -q") && !narrow.contains("› 2/2"), "{narrow}");
    assert!(narrow.trim_start().starts_with("●"), "the mode word first: {narrow}");
    for line in [&wide, &mid, &narrow] {
        assert!(line.width() <= 160 && !line.contains("—  "), "{line}");
    }
}

/// The step's own time on the line, not the turn's: a call's starts as it's made, again at the next step.
#[test]
fn the_steps_time_is_its_own() {
    let mut ed = at_work();
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.started -= Duration::from_secs(300);
    call(&mut ed, "t1", "run", json!({"command": "cargo test -q"}));
    let s = step(&ed, &ed.workspace).unwrap();
    assert!(took(&ed.workspace, &s) < Duration::from_secs(5), "the run's, not the turn's 5 minutes");
    let line = status(&mut ed, 160, 40);
    let time = line.split("cargo test -q ").nth(1).and_then(|r| r.split_whitespace().next());
    assert!(time.is_some_and(|t| t.ends_with("ms")), "under a second: {line}");
}

use unicode_width::UnicodeWidthStr;

/// The state says it for drivers — the front session's `turn.now`: the step in the line's words, since when, the
/// sentence, the plan — and Claude's level never (it's Claude's own doing).
#[test]
fn the_state_says_it_and_claudes_level_never() {
    let mut ed = at_work();
    send(&mut ed, StreamEv::Text("I will run the tests.\n".into()));
    call(&mut ed, "t1", "run", json!({"command": "cargo test -q"}));
    let v = ed.ui_state().json();
    let now = &v["workspaces"][0]["sessions"][0]["turn"]["now"];
    assert_eq!(now["step"], "running cargo test -q", "{v:#}");
    assert_eq!((now["said"].as_str(), now["waits"].as_bool()), (Some("I will run the tests."), Some(false)));
    assert!(now["since_ms"].as_u64().is_some_and(|ms| ms > 0), "{now}");
    let claudes = ed.ui_state_claude().json().to_string();
    assert!(!claudes.contains("\"turn\"") && !claudes.contains("running cargo test"), "{claudes}");
    send(&mut ed, StreamEv::Done(Ok("done".into())));
    assert!(ed.ui_state().json()["workspaces"][0]["sessions"][0]["turn"].is_null(), "between turns: none");
}

/// The cells the line takes on the status row, from where it recorded them (`Screen.cut_spots`).
fn line_at(ed: &Editor) -> (u16, u16) {
    let spots = ed.screen.cut_spots.borrow();
    let s = spots.iter().find(|s| s.3 == crate::whole::Cut::Now).expect("the line's spot");
    (s.1 as u16, s.0 as u16)
}

/// A press on the line follows Claude's place: paused, following again; off, on — as `C-f` does in the chat and
/// `space F` in the code. A press elsewhere on the status line doesn't.
#[test]
fn a_press_on_the_line_follows_claude() {
    let press = |ed: &mut Editor, (x, y): (u16, u16)| {
        let m = crate::event::Mouse { kind: crate::event::MouseKind::Down, x, y, alt: false, ctrl: false };
        ed.handle_event(crate::event::Event::Mouse(m));
    };
    for (on, paused) in [(true, true), (false, false)] {
        let mut ed = at_work();
        call(&mut ed, "t1", "run", json!({"command": "cargo test -q"}));
        let f = &mut ed.workspace.claude.chat.as_mut().unwrap().follow;
        (f.on, f.paused) = (on, paused);
        ed.workspace.viewer.at = crate::workspace::At::Code;
        status(&mut ed, 160, 40);
        let (x, y) = line_at(&ed);
        press(&mut ed, (2, y));
        assert!(!ed.workspace.claude.chat.as_ref().unwrap().follow.active(), "the mode word isn't it");
        status(&mut ed, 160, 40);
        press(&mut ed, (x + 4, y));
        let f = &ed.workspace.claude.chat.as_ref().unwrap().follow;
        assert!(f.active(), "following again from on={on} paused={paused}");
    }
}

/// Following and leading already, the view held somewhere else than where Claude is now (its last place): a press on
/// the line brings the view to Claude's place — its read's lines — your cursor where it was.
#[test]
fn a_press_while_following_brings_the_view_to_claudes_place() {
    let scratch = crate::scratch::Scratch::new("now-press");
    let root = std::fs::canonicalize(&scratch).unwrap();
    let path = root.join("a.rs");
    std::fs::write(&path, (0..120).map(|i| format!("let line_{i} = {i};\n")).collect::<String>()).unwrap();
    let mut ed = at_work();
    ed.workspace.root = root.clone();
    ed.open(&path).unwrap();
    call(&mut ed, "t1", "read", json!({"files": ["a.rs:80-90"]}));
    let doc = ed.doc().id;
    let look = ed.workspace.claude.chat.as_ref().unwrap().msgs.last().unwrap().look.clone().unwrap();
    let gaze = |ed: &mut Editor, lines| {
        let f = &mut ed.workspace.claude.chat.as_mut().unwrap().follow;
        f.gaze = Some(crate::follow::Gaze::new(look.clone(), Some(path.clone()), Some(lines)));
    };
    // (following, the view where Claude was — then Claude somewhere else, the view not there yet)
    ed.workspace.viewer.at = crate::workspace::At::Code;
    gaze(&mut ed, 0..5);
    let id = ed.workspace.id;
    crate::follow::follow_session(&mut ed, id);
    assert_eq!(
        (ed.doc().id, crate::follow::view_line(&ed)),
        (doc, Some(0)),
        "following, the view at its last place"
    );
    gaze(&mut ed, 79..90);
    status(&mut ed, 160, 40);
    let (x, y) = line_at(&ed);
    let m = crate::event::Mouse { kind: crate::event::MouseKind::Down, x: x + 4, y, alt: false, ctrl: false };
    ed.handle_event(crate::event::Event::Mouse(m));
    let f = &ed.workspace.claude.chat.as_ref().unwrap().follow;
    assert!(f.active(), "still following");
    assert_eq!(crate::follow::view_line(&ed), Some(79), "the view where Claude is now");
    assert_eq!(ed.cursor_line(), 0, "your cursor where it was");
}

/// The mouse over the line: its words whole on a card right above it — the sentence the line cut, the plan's step,
/// the key that follows Claude; the mouse leaving lets it go. Nothing else on the frame moves.
#[test]
fn the_mouse_over_the_line_shows_it_whole() {
    let mut ed = at_work();
    ed.workspace.claude.chat.as_mut().unwrap().follow.on = false;
    let long = "I will run the unit tests to check that nothing else relied on the old replace, then say what changed.";
    send(&mut ed, StreamEv::Text(format!("{long}\n")));
    call(&mut ed, "t1", "run", json!({"command": "cargo test -q"}));
    ed.workspace.viewer.at = crate::workspace::At::Code;
    let before = rows(&mut ed, 100, 30);
    assert!(!before[29].contains(long), "cut on the line: {}", before[29]);
    let (x, y) = line_at(&ed);
    ed.handle_event(crate::event::Event::Hover(x + 2, y));
    assert_eq!(ed.cut_hover, Some(crate::whole::Cut::Now));
    let over = rows(&mut ed, 100, 30);
    let card: String = over[..29].iter().rev().take(6).map(|r| r.trim()).collect::<Vec<_>>().join(" ");
    assert!(card.contains("replace, then say") && card.contains("space F follow Claude"), "{over:#?}");
    // (its time goes on between frames: the rows compared without it)
    let untimed = |rows: &[String]| {
        let words = |r: &String| {
            r.replace(|c: char| c.is_ascii_digit(), "").split_whitespace().collect::<Vec<_>>().join(" ")
        };
        rows.iter().map(words).collect::<Vec<_>>()
    };
    assert_eq!(untimed(&over[29..]), untimed(&before[29..]), "the line itself unmoved");
    ed.handle_event(crate::event::Event::Hover(x, 2));
    assert_eq!(ed.cut_hover, None);
    assert_eq!(untimed(&rows(&mut ed, 100, 30)), untimed(&before), "gone as the mouse leaves");
}
