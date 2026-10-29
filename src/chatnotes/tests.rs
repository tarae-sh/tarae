//! #239: what the chat's last answer says of a place shows at it — the sentence at the line end, the paragraph on the
//! card, cut from the answer as written; the next answer that names places replaces them.

use std::path::{Path, PathBuf};

use super::*;
use crate::chat::{Chat, Msg, Role};
use crate::notes::{By, Entry};

const W: u16 = 110;
const H: u16 = 30;

/// A project of the test's own with `files`, ten lines each, and a chat with no claude behind it.
fn project(name: &str, files: &[&str]) -> (crate::scratch::Scratch, PathBuf, Editor) {
    let scratch = crate::scratch::Scratch::new(name);
    let root = std::fs::canonicalize(&*scratch).unwrap();
    for f in files {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let text: String = (1..=10).map(|i| format!("let line_{i} = {i};\n")).collect();
        std::fs::write(&p, text).unwrap();
    }
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    ed.workspace.claude.chat = Some(Chat::new());
    (scratch, root, ed)
}

/// A turn that ends with `text` as its answer: its places waiting, a frame asks their names, their answers land.
fn answer(ed: &mut Editor, text: &str) {
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::User, "where?", ""));
    c.msgs.push(Msg::new(Role::Assistant, text, ""));
    turn_over(ed, true);
    crate::term::render(ed, &mut Vec::new(), W, H).unwrap();
    // (one frame asks the names; no frame after it: the answers that land are the last asked — a name put back
    // while its search was under way would never be answered, and this never settle)
    crate::test_results::settle_until(ed, |ed| {
        crate::places::answered(ed) && ed.workspace.claude.notes.answer.is_none()
    });
}

/// The chat notes as (file, first line from 1, the line end's words).
fn chat_notes(ed: &Editor, root: &Path) -> Vec<(String, usize, String)> {
    let mut v: Vec<_> = ed
        .workspace
        .claude
        .notes
        .list
        .iter()
        .filter(|n| n.thread[0].by == By::Chat)
        .map(|n| (crate::chat::shown(root, &n.path), n.lines.start + 1, n.gist().to_string()))
        .collect();
    v.sort();
    v
}

fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key: crate::key::Key = if c == '<' {
            chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap()
        } else {
            crate::key::Key::plain(crate::key::Code::Char(c))
        };
        ed.handle_key(key);
    }
}

fn screen(ed: &mut Editor) -> Vec<String> {
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, W, H).unwrap();
    let mut vt = vt100::Parser::new(H, W, 0);
    vt.process(&bytes);
    vt.screen().rows(0, W).collect()
}

/// The cut, by the answer's text as written: a paragraph naming two places gives each its own sentence and both the
/// paragraph (the sentence plain, the paragraph as written — the card draws its marks); a list item is that item; a range covers its lines; fenced code is no prose; a walk-through's stop is its
/// `why`, the block after it its paragraph; `e.g.` ends no sentence; a place named twice is said the first time.
#[test]
fn an_answer_is_cut_into_each_places_sentence_and_paragraph() {
    let text = "The count is read in `src/a.rs:3`, e.g. by the loop. The fix goes in src/b.rs:2-4,\n\
                after the guard.\n\n\
                Steps:\n\
                - `a.rs:7` keeps the old value\n  until the next frame.\n\
                - nothing here\n\n\
                ```\nsrc/a.rs:9 in a block\n```\n\n\
                <show at=\"src/c.rs:5-8\">where it starts</show>\n\
                It starts here and src/a.rs:3 again.";
    let said = cut(text);
    let got: Vec<(&str, Range<usize>, &str)> =
        said.iter().map(|s| (&*s.name, s.lines.clone(), s.sentence.as_str())).collect();
    assert_eq!(
        got,
        [
            ("src/a.rs", 2..3, "The count is read in src/a.rs:3, e.g. by the loop."),
            ("src/b.rs", 1..4, "The fix goes in src/b.rs:2-4, after the guard."),
            ("a.rs", 6..7, "a.rs:7 keeps the old value until the next frame."),
            ("src/c.rs", 4..8, "where it starts"),
        ]
    );
    let both =
        "The count is read in `src/a.rs:3`, e.g. by the loop. The fix goes in src/b.rs:2-4, after the guard.";
    assert_eq!(said[0].paragraph, both, "the paragraph by its source lines, joined");
    assert_eq!(said[1].paragraph, both);
    assert_eq!(
        said[2].paragraph, "`a.rs:7` keeps the old value until the next frame.",
        "a list item: that item"
    );
    assert_eq!(said[3].paragraph, "where it starts\n\nIt starts here and src/a.rs:3 again.");
    // (a heading, a quote and a table row are blocks of their own; a stop with no why says what follows it)
    let said = cut(
        "## In src/a.rs:1\n> quoted src/b.rs:2.\n| x | src/c.rs:3 |\n<show at=\"d.rs:1\"></show>\n\nIts body. More.",
    );
    let got: Vec<(&str, &str)> = said.iter().map(|s| (&*s.name, s.sentence.as_str())).collect();
    assert_eq!(
        got,
        [
            ("src/a.rs", "In src/a.rs:1"),
            ("src/b.rs", "quoted src/b.rs:2."),
            ("src/c.rs", "| x | src/c.rs:3 |"),
            ("d.rs", "Its body.")
        ]
    );
}

/// The key-sequence walk: an answer's places are notes at their lines — `]n` goes to them with the others, the line
/// end its sentence, the card its paragraph named `chat`; a name that comes to no file (or to several) is no place and
/// fenced code none; `space n` on one answers it, a thread then, kept when the next answer's places replace the
/// rest; an answer whose names come to nothing leaves the last one's. None is kept across a restart but a thread.
#[test]
fn the_last_answers_places_are_notes_at_their_lines() {
    let (_s, root, mut ed) = project("chatnotes-walk", &["src/a.rs", "src/b.rs", "x/Same.rs", "y/Same.rs"]);
    answer(
        &mut ed,
        "The count is read twice in `src/a.rs:3`. The fix goes in src/b.rs:2-4, after the guard.\n\n\
         - Same.rs:1 is two files, Gone.rs:2 none.\n\n```\nsrc/a.rs:9\n```",
    );
    let first = [
        ("src/a.rs".to_string(), 3, "The count is read twice in src/a.rs:3.".to_string()),
        ("src/b.rs".to_string(), 2, "The fix goes in src/b.rs:2-4, after the guard.".to_string()),
    ];
    assert_eq!(chat_notes(&ed, &root), first);
    // `]n` from the top of src/a.rs: at its line, the sentence at the line end, the paragraph on its card
    ed.open(&root.join("src/a.rs")).unwrap();
    feed(&mut ed, "gg]n");
    assert_eq!(ed.cursor_line(), 2);
    let rows = screen(&mut ed);
    let line = rows.iter().find(|r| r.contains("let line_3 = 3;")).unwrap();
    assert!(line.contains("¶ The count is read twice in src/a.rs:3."), "{line}");
    let card = rows.join("\n");
    assert!(card.contains("¶ chat · the last answer"), "{card}");
    assert!(card.contains("The fix goes in"), "the paragraph on the card: {card}");
    // (`]n` on to src/b.rs: its range's first line)
    feed(&mut ed, "]n");
    assert_eq!(ed.doc().path.as_deref(), Some(root.join("src/b.rs").as_path()));
    assert_eq!((ed.cursor_line(), ed.workspace.claude.notes.list[1].lines.clone()), (1, 1..4));
    // `space n` on it answers that note — your words in its thread make it one (sent: a stand-in, no claude here)
    feed(&mut ed, " n");
    let id = ed.workspace.claude.notes.list[1].id;
    let kind = ed.prompt.as_ref().map(|p| p.kind);
    assert!(matches!(kind, Some(crate::editor::PromptKind::Note(Some(i))) if i == id), "{kind:?}");
    feed(&mut ed, "<esc>");
    let n = ed.workspace.claude.notes.list.iter_mut().find(|n| n.id == id).unwrap();
    n.thread.push(Entry { by: By::You, text: "why there?".into(), draft: false });
    // A restart keeps the thread, never the bare one (the last answer's)
    let saved = crate::notes::to_json(&ed.workspace.claude.notes, &ed.docs);
    let words: Vec<(&str, &str)> =
        saved.iter().map(|n| (n["path"].as_str().unwrap(), n["thread"][0][0].as_str().unwrap())).collect();
    assert_eq!(words, [(root.join("src/b.rs").to_str().unwrap(), "chat")]);
    // An answer whose names come to nothing leaves the last one's
    answer(&mut ed, "Nothing in Gone.rs:1 nor Same.rs:2.");
    assert_eq!(chat_notes(&ed, &root), first);
    // The next one that names a place replaces the bare ones; the thread stays
    answer(&mut ed, "Now src/a.rs:5 holds it.");
    assert_eq!(
        chat_notes(&ed, &root),
        [
            ("src/a.rs".to_string(), 5, "Now src/a.rs:5 holds it.".to_string()),
            ("src/b.rs".to_string(), 2, "The fix goes in src/b.rs:2-4, after the guard.".to_string()),
        ]
    );
    assert_eq!(ed.workspace.claude.notes.list.iter().filter(|n| crate::notes::bare_chat(n)).count(), 1);
    // (never told to Claude while bare: the state's notes sent are the thread only)
    let sent: Vec<(&str, bool)> = ed
        .workspace
        .claude
        .notes
        .list
        .iter()
        .map(|n| crate::uistate::NoteState::of(&root, n))
        .map(|s| (s.kind, s.sent))
        .collect();
    assert_eq!(sent, [("chat", true), ("chat", false)]);
}

/// The stand-in claude: `/compact` compacts (no answer); a message asking to fail says half a sentence naming
/// `src/b.rs:2`, then the turn fails; any other answers naming `src/a.rs:3`.
const STAND_IN: &str = r#"while IFS= read -r line; do
  case "$line" in
    *'"content":"/compact"'*)
      printf '{"type":"result","subtype":"success","is_error":false,"result":""}\n';;
    *'please fail'*)
      printf '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Now src/b.rs:2 is where the"}}}\n'
      printf '{"type":"result","subtype":"error_during_execution","is_error":true,"result":"boom"}\n';;
    *'"type":"user"'*)
      printf '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"It is read in src/a.rs:3."}}}\n'
      printf '{"type":"result","subtype":"success","is_error":false,"result":"done"}\n';;
  esac
done"#;

/// Rows in the chat (a message sent is one).
fn rows(ed: &Editor) -> usize {
    ed.workspace.claude.chat.as_ref().map_or(0, |c| c.msgs.len())
}

/// The stand-in's turn — sent past `before` rows — ended and its places (if any) placed: events handled, a frame
/// after each as the main loop draws (its names asked after it).
fn turn_ended(ed: &mut Editor, before: usize) {
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    loop {
        crate::term::render(ed, &mut Vec::new(), W, H).unwrap();
        let ended = rows(ed) > before && ed.workspace.claude.chat.as_ref().is_some_and(|c| !c.busy());
        if ended && crate::places::answered(ed) && ed.workspace.claude.notes.answer.is_none() {
            return;
        }
        assert!(std::time::Instant::now() < end, "the turn didn't end within the patience");
        if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
            ed.handle_event(ev);
        }
    }
}

fn ask(ed: &mut Editor, text: &str) {
    let before = rows(ed);
    ed.workspace.claude.chat.as_mut().unwrap().input = text.into();
    crate::chat::send(ed);
    turn_ended(ed, before);
}

/// The reviewer's two (#239): only a whole answer to a message of yours places notes. A bare turn — idle compaction's
/// `/compact`, no message of yours — places none: never the turn before's answer again (one you closed would come
/// back). A turn that failed (or was stopped) leaves the last whole answer's notes, never its own half sentence.
#[test]
fn only_a_whole_answer_to_your_message_places_notes() {
    let (_s, root, mut ed) = project("chatnotes-turns", &["src/a.rs", "src/b.rs"]);
    ed.workspace.claude.chat = None;
    ed.config.llm.command = "sh".into();
    ed.config.llm.args = vec!["-c".into(), STAND_IN.into(), "stand-in".into()];
    ed.config.llm.chat_settings = false;
    crate::chat::open(&mut ed);
    let a = || vec![("src/a.rs".to_string(), 3, "It is read in src/a.rs:3.".to_string())];
    ask(&mut ed, "where is it read?");
    assert_eq!(chat_notes(&ed, &root), a());
    // Closed; then a bare `/compact`: its end brings none back
    crate::typed::execute(&mut ed, "notes-clear chat").unwrap();
    assert!(chat_notes(&ed, &root).is_empty());
    let before = rows(&ed);
    assert!(crate::chat::submit_bare(&mut ed, "/compact", "compacting".into()), "sent bare");
    turn_ended(&mut ed, before);
    assert!(chat_notes(&ed, &root).is_empty(), "the turn before's answer placed again");
    // A whole answer places them; a failed turn's half sentence doesn't take their place
    ask(&mut ed, "where is it read?");
    assert_eq!(chat_notes(&ed, &root), a());
    ask(&mut ed, "please fail");
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let said: Vec<(crate::chat::Role, &str)> = c.msgs.iter().map(|m| (m.role, m.text.as_str())).collect();
    assert!(
        said.iter().any(|m| m.1.contains("src/b.rs:2 is where the")),
        "its partial answer is in the chat: {said:?}"
    );
    assert_eq!(chat_notes(&ed, &root), a());
}

/// The reviewer on the follow-up: another name waiting — one `follow::answer_place` asks as it looks for the answer's
/// place, here a place in the answer's fenced code, which its notes never take — holds none of the answer's notes:
/// they land with their own answers, no frame between.
#[test]
fn another_name_waiting_holds_none_of_the_answers_notes() {
    let (_s, root, mut ed) = project("chatnotes-others", &["src/a.rs", "src/b.rs"]);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::User, "where?", ""));
    c.msgs.push(Msg::new(Role::Assistant, "It is read in src/a.rs:3.\n\n```\nsrc/b.rs:2\n```", ""));
    turn_over(&mut ed, true);
    // (the frame's ask, the chat not drawn: the answer's own names only — a drawn chat would ask `src/b.rs` with them)
    crate::places::after_frame(&mut ed);
    assert!(!ed.workspace.claude.chat.as_ref().unwrap().places.pending("src/b.rs"));
    // (follow waits for the answer's place: as the names land it looks, asking `src/b.rs`)
    ed.workspace.claude.chat.as_mut().unwrap().follow.answer_wait = true;
    crate::test_results::settle_until(&mut ed, |ed| ed.workspace.claude.notes.answer.is_none());
    assert_eq!(
        chat_notes(&ed, &root),
        [("src/a.rs".to_string(), 3, "It is read in src/a.rs:3.".to_string())]
    );
    let places = &ed.workspace.claude.chat.as_ref().unwrap().places;
    assert!(places.pending("src/b.rs"), "follow's own name still waits for a frame");
}

/// The reviewer's round 4: the checkout's files change while the answer's names are being found — the frame after
/// asks again only the names answered (`found`), the search under way lands for a generation passed and is dropped,
/// and its names are on no list: the notes ask them again and say a frame is due, so the main loop (a batch, a frame
/// only when one is due) asks them and the notes still appear.
#[test]
fn the_files_changing_mid_search_still_places_the_notes() {
    let (_s, root, mut ed) = project("chatnotes-moved", &["src/a.rs"]);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::User, "where?", ""));
    c.msgs.push(Msg::new(Role::Assistant, "It is read in src/a.rs:3.", ""));
    turn_over(&mut ed, true);
    // (the frame's ask, the chat not drawn: `src/a.rs` in flight)
    crate::places::after_frame(&mut ed);
    assert!(ed.workspace.claude.chat.as_ref().unwrap().places.pending("src/a.rs"));
    // A file made meanwhile; the frame after it: the files changed — the search under way is for the ones before
    ed.files_moved(&root.join("src/a.rs"));
    crate::places::after_frame(&mut ed);
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    while ed.workspace.claude.notes.answer.is_some() {
        assert!(std::time::Instant::now() < end, "the answer's notes held: no frame asked its names again");
        let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) else { continue };
        // (a batch of this one event — its end now — and a frame only where `batch_done` says one is due)
        let (unchanged, _) = crate::term::handle_batch(&mut ed, ev, std::time::Instant::now());
        if crate::term::batch_done(&mut ed, unchanged) {
            crate::term::render(&mut ed, &mut Vec::new(), W, H).unwrap();
        }
    }
    assert_eq!(
        chat_notes(&ed, &root),
        [("src/a.rs".to_string(), 3, "It is read in src/a.rs:3.".to_string())]
    );
}

/// A line where the answer's own `note` sits takes no chat note — Claude said it there already; the answer's other
/// places still do, and a note pinned in an earlier turn doesn't hold one back.
#[test]
fn the_answers_own_note_keeps_its_line() {
    let (_s, root, mut ed) = project("chatnotes-own", &["src/a.rs", "src/b.rs"]);
    let pin = |ed: &mut Editor, line: u64| {
        let args =
            serde_json::json!({"path": "src/b.rs", "line": line, "end_line": line + 1, "text": "pinned"});
        crate::notes::tool_call(ed, &args).unwrap();
    };
    pin(&mut ed, 5);
    answer(&mut ed, "It's src/b.rs:6 that reads it; src/a.rs:1 calls it.");
    assert_eq!(
        chat_notes(&ed, &root),
        [("src/a.rs".to_string(), 1, "It's src/b.rs:6 that reads it; src/a.rs:1 calls it.".to_string())]
    );
    // (the next turn pins nothing: the earlier note doesn't keep its line)
    answer(&mut ed, "Still src/b.rs:5.");
    assert_eq!(chat_notes(&ed, &root), [("src/b.rs".to_string(), 5, "Still src/b.rs:5.".to_string())]);
}
