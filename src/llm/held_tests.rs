//! Friction #339: a config file that doesn't read holds every claude until it does (`llm::held_for`).

use std::sync::mpsc::{Receiver, channel};

use crate::config::{Config, parse};
use crate::editor::Editor;
use crate::key::{Code, Key};

/// A string never closed on its second line: the file doesn't read.
const BROKEN: &str = "[llm]\ncommand = \"sh\n";

fn reads() -> Config {
    parse("[llm]\ncommand = \"sh\"\n").0
}

/// `config`'s editor, its claude a stand-in: what it's sent, never a process.
fn editor(config: Config) -> (Editor, Receiver<String>) {
    let mut ed = Editor::new(config);
    let (tx, rx) = channel();
    ed.claude_lines = Some(tx);
    (ed, rx)
}

/// Keys as `run` takes them (`<space>`, `<ret>`).
fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(key);
    }
}

/// The screen as a frame `w`×`h` draws it.
fn screen(ed: &mut Editor, w: u16, h: u16) -> String {
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, w, h).unwrap();
    let mut vt = vt100::Parser::new(h, w, 0);
    vt.process(&bytes);
    vt.screen().contents()
}

fn last_error(ed: &Editor) -> String {
    let mut errors = ed.toasts.iter().filter(|t| t.kind == crate::editor::ToastKind::Error);
    errors.next_back().map(|t| t.text.clone()).unwrap_or_default()
}

/// Started on a config that doesn't read: the chat opened by `space L` starts no claude and says why in its first rows —
/// the file's own words with its line, `:config-reload` — a message sent waits; `space i`'s claude, a commit draft's and
/// `:claude-update` are refused with the same words; once it reads, the chat's claude starts and the message goes.
#[test]
fn no_claude_starts_until_the_config_reads() {
    let (config, warnings) = parse(BROKEN);
    assert_eq!(config.unread.len(), 1, "{warnings:?}");
    assert!(config.unread[0].starts_with("config.toml:2: "), "{:?}", config.unread);
    let (mut ed, rx) = editor(config);
    feed(&mut ed, "<space>L");
    let c = ed.workspace.claude.chat.as_ref().expect("the chat is open");
    assert!(!c.running() && c.held, "no claude started");
    let shown = screen(&mut ed, 120, 34);
    assert!(shown.contains("no claude until your config reads"), "{shown}");
    assert!(shown.contains("config.toml:2:"), "the file and its line: {shown}");
    assert!(shown.contains("fix it, then :config-reload"), "{shown}");
    feed(&mut ed, "hello<ret>");
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert_eq!(c.queued.as_deref(), Some("hello"), "it waits");
    assert_eq!(rx.try_iter().count(), 0, "nothing reached a claude");
    // Every other claude from the config holds alike, said as an error
    feed(&mut ed, "<esc><esc>:ask make it shorter<ret>");
    assert!(
        last_error(&ed).contains("no claude until your config reads — config.toml:2:"),
        "{}",
        last_error(&ed)
    );
    assert!(ed.workspace.claude.ask.is_none() && ed.workspace.claude.review.is_none(), "no ask started");
    ed.toasts.clear();
    feed(&mut ed, ":claude-update<ret>");
    assert!(last_error(&ed).contains("no claude until your config reads"), "{}", last_error(&ed));
    let doc = ed.doc().id;
    ed.toasts.clear();
    assert!(
        crate::llm::draft(&mut ed, doc, "draft", "a commit message", 0, 0)
            .is_err_and(|e| e.contains("no claude until"))
    );
    // It reads: the chat's claude starts, the message goes
    ed.reload_config(reads(), Vec::new());
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert!(c.running() && !c.held, "started once it reads");
    assert!(rx.try_iter().any(|l| l.contains("hello")), "the message went");
    assert!(!screen(&mut ed, 120, 34).contains("no claude until"), "nothing said once it reads");
}

/// Agents beside Claude, step 4, C.2: on a config whose agent can chat and nothing else (the stand-in's ACP row, as
/// agy's will be), `space i` and a commit draft are refused in their words — nothing started, the chat's own claude
/// told nothing — and the chat's start isn't held; the config unread is said first.
#[test]
fn space_i_and_a_draft_are_held_on_a_row_that_can_only_chat() {
    let mut config = reads();
    config.editor.llm.agent = &crate::agent::STAND_IN;
    let (mut ed, rx) = editor(config);
    feed(&mut ed, "<space>imake it shorter<ret>");
    assert_eq!(last_error(&ed), "not yet with stand-in — space i runs on Claude's sessions only");
    assert!(ed.workspace.claude.ask.is_none() && ed.workspace.claude.review.is_none(), "no ask started");
    let doc = ed.doc().id;
    let drafted = crate::llm::draft(&mut ed, doc, "draft", "a commit message", 0, 0);
    assert_eq!(drafted, Err("not yet with stand-in — a commit draft runs on Claude's sessions only".into()));
    assert_eq!(crate::llm::held_for(&ed, crate::agent::Role::Chat), None, "the chat's start isn't held");
    assert_eq!(rx.try_iter().count(), 0, "nothing reached a claude");
    // (a config that doesn't read is said before the row's hold)
    let (broken, warnings) = parse(BROKEN);
    ed.reload_config(broken, warnings);
    let held = crate::llm::held_for(&ed, crate::agent::Role::Ask).unwrap_or_default();
    assert!(held.starts_with("no claude until your config reads"), "{held}");
}

/// A reload that doesn't read: said as an error toast with the hold, once; a claude already running goes on (its
/// restart refused, said), and one that reads says nothing of a hold.
#[test]
fn a_reload_that_doesnt_read_holds_what_would_start() {
    let (mut ed, rx) = editor(reads());
    crate::chat::open(&mut ed);
    assert!(ed.workspace.claude.chat.as_ref().unwrap().running());
    let (broken, warnings) = parse(BROKEN);
    ed.reload_config(broken, warnings);
    let said: Vec<_> =
        ed.toasts.iter().filter(|t| t.text.contains("config: no claude until it reads — ")).collect();
    assert_eq!(said.len(), 1, "one error toast: {:?}", ed.toasts.iter().map(|t| &t.text).collect::<Vec<_>>());
    assert!(said[0].text.contains("config.toml:2:") && said[0].text.contains(":config-reload"));
    assert!(ed.workspace.claude.chat.as_ref().unwrap().running(), "the running one goes on");
    crate::chat::restart_process(&mut ed);
    assert!(ed.workspace.claude.chat.as_ref().unwrap().running(), "not stopped for one that won't start");
    assert!(last_error(&ed).contains("no claude until"), "{}", last_error(&ed));
    assert!(crate::chat::submit(&mut ed, "hi", None, None), "a message to the running one goes");
    assert!(rx.try_iter().any(|l| l.contains("hi")));
    ed.toasts.clear();
    ed.reload_config(reads(), Vec::new());
    assert!(
        ed.toasts.iter().all(|t| !t.text.contains("no claude")),
        "a config that reads says nothing of it"
    );
}

/// Step 4's L2: agy's version, read at `initialize`, is said in the transcript once a version where it isn't the one
/// tarae's guards were measured on — never refused, the session going on; the measured one says nothing. (agy's row
/// with the chat's stand-in for its handle: no process.)
#[test]
fn an_agents_other_version_is_said_once_a_version() {
    let (mut ed, _rx) = editor(on(&crate::agent::AGY));
    crate::chat::open(&mut ed);
    let g = ed.workspace.claude.chat.as_ref().unwrap().generation();
    let says = |ed: &mut Editor, id: &str, v: &str| {
        let version = Some(v.to_string());
        let ev = crate::agent::Event::Conversation {
            id: id.into(),
            model: String::new(),
            version,
            key_source: None,
        };
        crate::chat::stream_event(ed, g, ev);
    };
    // (1.6.0 again after another: once a version, not once in a row)
    for (id, v) in [("s1", "1.5.9"), ("s1", "1.6.0"), ("s2", "1.7.0"), ("s3", "1.6.0")] {
        says(&mut ed, id, v);
    }
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let said: Vec<&str> =
        c.msgs.iter().filter(|m| m.text.contains("measured on")).map(|m| m.text.as_str()).collect();
    assert_eq!(
        said,
        [
            "agy 1.6.0: tarae's guards were measured on 1.5.9",
            "agy 1.7.0: tarae's guards were measured on 1.5.9"
        ]
    );
    assert!(c.running(), "never refused");
}

/// A config whose `llm.agent` is `row` (the stand-in's ACP row standing for agy, as no test starts agy).
fn on(row: &'static crate::agent::Row) -> Config {
    let mut config = reads();
    config.editor.llm.agent = row;
    config
}

/// The warnings said so far.
fn warnings(ed: &Editor) -> Vec<String> {
    let warned = ed.toasts.iter().filter(|t| t.kind == crate::editor::ToastKind::Warning);
    warned.map(|t| t.text.clone()).collect()
}

/// `space i` on the buffer's word: its starts counted by what reached its stand-in (`Editor.ask_lines`) — one a start.
fn space_i(ed: &mut Editor) -> (Result<(), String>, usize) {
    let (tx, rx) = channel();
    ed.ask_lines = Some(tx);
    let started = crate::ask::start(ed, "make it shorter");
    (started, rx.try_iter().filter(|l| l.contains("make it shorter")).count())
}

/// Every role but the chat's.
const HELPERS: [crate::agent::Role; 5] = [
    crate::agent::Role::Ask,
    crate::agent::Role::Draft,
    crate::agent::Role::Reviewer(crate::reviewer::Role::One),
    crate::agent::Role::WebSearch,
    crate::agent::Role::CardReview { max_usd: 0.05 },
];

/// Step 4's L2, case one: a session begun on Claude stays Claude's under a config that names another agent since —
/// its row fixed at its first start, the change said where you look, and every role's start on Claude: `space i`
/// starts once, nothing held.
#[test]
fn a_claude_session_runs_on_claude_under_another_agents_config() {
    use crate::agent::{CLAUDE, STAND_IN};
    let (mut ed, rx) = editor(on(&CLAUDE));
    feed(&mut ed, "ihello<esc>");
    crate::chat::open(&mut ed);
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert!(c.running() && c.row().is_some_and(|r| std::ptr::eq(r, &CLAUDE)), "fixed at its first start");
    ed.toasts.clear();
    ed.reload_config(on(&STAND_IN), Vec::new());
    assert_eq!(
        warnings(&ed),
        ["this session stays on claude — llm.agent's stand-in starts with a new conversation"]
    );
    assert!(std::ptr::eq(crate::llm::session_row(&ed), &CLAUDE), "the session's, not the config's");
    for role in HELPERS {
        assert_eq!(crate::llm::held_for(&ed, role), None, "{role:?}");
    }
    let (started, starts) = space_i(&mut ed);
    assert_eq!((started, starts), (Ok(()), 1), "space i started once, on Claude");
    assert!(ed.workspace.claude.ask.is_some());
    assert!(rx.try_iter().all(|l| !l.contains("make it shorter")), "the chat's own claude told nothing");
    // (the same config read again says nothing more: nothing moved)
    ed.toasts.clear();
    ed.reload_config(on(&STAND_IN), Vec::new());
    assert!(warnings(&ed).is_empty(), "{:?}", warnings(&ed));
}

/// Step 4's L2: a config's agent changed shows on every chat with no row yet, behind too (`chat::agent_moved`) — which
/// its first start takes — and says nothing where the session in front has none; a session behind that has its row keeps
/// it.
#[test]
fn a_chat_behind_with_no_row_yet_shows_the_new_agent() {
    use crate::agent::{CLAUDE, STAND_IN};
    let s = crate::scratch::Scratch::new("held-behind-row");
    let (mut ed, _rx) = editor(on(&CLAUDE));
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    let ids: Vec<_> = ["a", "b"]
        .map(|n| {
            let root = std::fs::canonicalize(s.path()).unwrap().join(n);
            std::fs::create_dir(&root).unwrap();
            ed.workspace_add(root)
        })
        .into();
    for (&id, fixed) in ids.iter().zip([false, true]) {
        ed.with_workspace(id, |ed| {
            let mut c = crate::chat::Chat::new();
            c.agent_fixed = fixed;
            ed.workspace.claude.chat = Some(c);
        });
    }
    ed.toasts.clear();
    ed.reload_config(on(&STAND_IN), Vec::new());
    let agent_of = |ed: &Editor, id| {
        let w = ed.workspaces.iter().find(|w| w.id == id).unwrap();
        w.claude.chat.as_ref().unwrap().agent.name
    };
    assert_eq!(ed.workspace.claude.chat.as_ref().unwrap().agent.name, "stand-in", "in front, no row yet");
    assert_eq!(
        (agent_of(&ed, ids[0]), agent_of(&ed, ids[1])),
        ("stand-in", "claude"),
        "behind: none yet, and its own"
    );
    assert!(warnings(&ed).is_empty(), "nothing to say in front: {:?}", warnings(&ed));
}

/// Step 4's L2, case two: a session begun on another agent stays its own under a Claude config since — the change
/// said, its other roles held in L1's words, never run on Claude behind its back: `space i` starts nothing, nor does a
/// draft. A new conversation takes the config's again.
#[test]
fn another_agents_session_holds_its_helpers_under_a_claude_config() {
    use crate::agent::{CLAUDE, STAND_IN};
    let (mut ed, rx) = editor(on(&STAND_IN));
    feed(&mut ed, "ihello<esc>");
    crate::chat::open(&mut ed);
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert!(c.running() && c.row().is_some_and(|r| std::ptr::eq(r, &STAND_IN)), "fixed at its first start");
    ed.toasts.clear();
    ed.reload_config(on(&CLAUDE), Vec::new());
    assert_eq!(
        warnings(&ed),
        ["this session stays on stand-in — llm.agent's claude starts with a new conversation"]
    );
    assert!(std::ptr::eq(crate::llm::session_row(&ed), &STAND_IN), "the session's, not the config's");
    let (started, starts) = space_i(&mut ed);
    assert_eq!(started, Err("not yet with stand-in — space i runs on Claude's sessions only".into()));
    assert_eq!(starts, 0, "space i started nothing");
    assert!(ed.workspace.claude.ask.is_none() && ed.workspace.claude.review.is_none());
    let doc = ed.doc().id;
    let drafted = crate::llm::draft(&mut ed, doc, "draft", "a commit message", 0, 0);
    assert_eq!(drafted, Err("not yet with stand-in — a commit draft runs on Claude's sessions only".into()));
    for role in HELPERS {
        let held = crate::llm::held_for(&ed, role).unwrap_or_default();
        assert!(held.starts_with("not yet with stand-in — "), "{role:?}: {held}");
    }
    assert_eq!(crate::llm::held_for(&ed, crate::agent::Role::Chat), None, "its own chat goes on");
    assert!(rx.try_iter().all(|l| !l.contains("make it shorter")), "nothing reached a claude");
    // (a new conversation has no row: the config's is picked again)
    crate::chat::reset(&mut ed);
    assert!(std::ptr::eq(crate::llm::session_row(&ed), &CLAUDE), "a new conversation's is the config's");
}
