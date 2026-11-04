//! Friction #339: a config file that doesn't read holds every claude until it does (`llm::held`).

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
