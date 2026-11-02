//! #239: what the chat's answers say of a place shows at it — the sentence at the line end, the paragraph on the card,
//! cut from the answer as written; kept for the conversation, newest first (focus A1b).

use std::path::{Path, PathBuf};

use super::Part;
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
    answers(ed, &[text], Vec::new());
}

/// `answer` for a turn that stopped at `stops` and answered in the messages `texts`.
fn answers(ed: &mut Editor, texts: &[&str], stops: Vec<crate::replay::Stop>) {
    answers_to(ed, "where?", texts, stops);
}

/// `answers` to the question `asked`.
fn answers_to(ed: &mut Editor, asked: &str, texts: &[&str], stops: Vec<crate::replay::Stop>) {
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::User, asked, ""));
    c.follow.new_turn();
    for s in stops {
        c.follow.replay.add(s);
    }
    // (`call`: a call of the turn's — what's said before it is the turn's narration)
    for t in texts {
        let role = if *t == "call" { Role::Tool } else { Role::Assistant };
        c.msgs.push(Msg::new(role, *t, ""));
    }
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

/// `chat_notes` of the newest answer alone (focus A1b: the ones before it are kept beside them).
fn newest_notes(ed: &Editor, root: &Path) -> Vec<(String, usize, String)> {
    let k = crate::notes::newest(&ed.workspace.claude.notes);
    let of_newest = |n: &crate::notes::Note| n.answer.as_ref().map(|o| o.n) == k;
    let mut v: Vec<_> = (ed.workspace.claude.notes.list.iter())
        .filter(|n| crate::notes::bare_chat(n) && of_newest(n))
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

/// Focus A1b: the conversation keeps its answers' places (`places_json`) and a continue brings them back from its rows
/// (`restore`) — the same notes, the same words; a new conversation's empty store leaves none.
#[test]
fn the_answers_come_back_with_their_conversation() {
    let (_s, root, mut ed) = project("chatnotes-kept", &["src/a.rs", "src/b.rs"]);
    answer(&mut ed, "The count is read in src/a.rs:3.\n\nIt never changes after.");
    answer(&mut ed, "The guard in src/a.rs:3 is enough; src/b.rs:2 checks it too.");
    let before = chat_notes(&ed, &root);
    assert_eq!(before.len(), 3, "both answers kept: {before:?}");
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let places = places_json(&ed.workspace.claude.notes, &c.msgs).expect("two answers placed");
    let rows = crate::chats::rows_of(&c.msgs);
    // (a new conversation: they go)
    crate::notes::forget_answers(&mut ed);
    assert!(chat_notes(&ed, &root).is_empty());
    // (continued: its rows back, then its places)
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs = crate::chats::msgs_of(&rows);
    c.msgs.push(Msg::new(Role::Note, "continued from 1m ago", ""));
    restore(&mut ed, Some(&places));
    assert_eq!(chat_notes(&ed, &root), before, "{places}");
    // (as kept: a continue has nothing to keep — else the next event saved it again, a conversation only looked at made
    // the newest)
    assert_eq!(ed.workspace.claude.notes.kept_bare, before.len());
}

/// The reviewer's (note 22): a continue through the store — the line texts of files that weren't open read there; a line
/// the file moved since found again by its text as its file opens; a place the answer's words no longer give, and an
/// answer whose question isn't where it was in the rows, left out and said once in `:messages`.
#[test]
fn a_continue_finds_moved_lines_and_says_what_it_lost() {
    let (s, root, mut ed) = project("chatnotes-continue", &["src/a.rs", "src/b.rs", "src/c.rs"]);
    answer(&mut ed, "The count is read in src/a.rs:3.\n\nIt never changes after.");
    answer(&mut ed, "The guard in src/b.rs:2 is enough.");
    answer(&mut ed, "Then src/b.rs:5.");
    answer(&mut ed, "It was src/c.rs:2.");
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let saved = crate::chats::Saved {
        id: "c1".into(),
        title: "where?".into(),
        updated: crate::chats::now(),
        rows: crate::chats::rows_of(&c.msgs),
        plan: None,
        usage: None,
        places: places_json(&ed.workspace.claude.notes, &c.msgs),
        seq: crate::chats::seq(),
    };
    let dir = s.join("chats");
    crate::chats::save(&dir, &root, saved);
    // (a file gone since it was kept)
    std::fs::remove_file(root.join("src/c.rs")).unwrap();
    let saved = crate::chats::load(&dir, &root).remove(0);
    // (no file was open: the store read each line's text)
    let answers = saved.places.as_ref().and_then(|p| p.as_array()).unwrap();
    let texts: Vec<&str> = (answers.iter().flat_map(|a| a["places"].as_array().unwrap()))
        .map(|p| p["text"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(texts, ["let line_3 = 3;", "let line_2 = 2;", "let line_5 = 5;", "let line_2 = 2;"]);
    // Meanwhile: two lines above src/a.rs:3; the second answer's words changed, the third's question
    let a = root.join("src/a.rs");
    let text = std::fs::read_to_string(&a).unwrap();
    std::fs::write(&a, format!("// one\n// two\n{text}")).unwrap();
    let mut msgs = crate::chats::msgs_of(&saved.rows);
    let k = msgs.iter().position(|m| m.text == "The guard in src/b.rs:2 is enough.").unwrap();
    msgs[k].text = "The guard is enough.".into();
    let q = msgs.iter().enumerate().filter(|(_, m)| m.role == Role::User).nth(2).unwrap().0;
    msgs[q].text = "where else?".into();
    crate::notes::forget_answers(&mut ed);
    ed.workspace.claude.chat.as_mut().unwrap().msgs = msgs;
    restore(&mut ed, saved.places.as_ref());
    let first = ("src/a.rs".to_string(), 3, "The count is read in src/a.rs:3.".to_string());
    assert_eq!(chat_notes(&ed, &root), [first]);
    let said: Vec<&str> =
        ed.messages.iter().map(|m| m.2.as_str()).filter(|m| m.contains("no longer found")).collect();
    assert_eq!(said, ["3 places of the conversation's answers no longer found"]);
    // (its file opened: the line found again by its text, two down — its flow with it)
    ed.open(&a).unwrap();
    crate::notes::attach(&mut ed);
    assert_eq!(chat_notes(&ed, &root)[0].1, 5);
    assert_eq!(flow_of(&ed, &a, 5).len(), 2, "{:?}", flow_of(&ed, &a, 5));
}

/// The reviewer's (note 21): a continue keeps where each answer's flow was — none for a turn that wrote at a note, and
/// none moved to another place once its own note went (a thread now): never worked out again from the words.
#[test]
fn a_continue_keeps_where_the_flow_was() {
    let (_s, root, mut ed) = project("chatnotes-flow-kept", &["src/a.rs", "src/b.rs"]);
    let (a, b) = (root.join("src/a.rs"), root.join("src/b.rs"));
    // (a turn that pinned a note: its place a note, no flow)
    let pin = serde_json::json!({"path": "src/b.rs", "line": 9, "text": "pinned"});
    crate::notes::tool_call(&mut ed, &pin).unwrap();
    answers(&mut ed, &["It is src/a.rs:4.\n\nThat is all."], Vec::new());
    assert!(flow_of(&ed, &a, 4).is_empty());
    // (a flow at src/a.rs:6, its note answered: a thread)
    answer(&mut ed, "The fix is in src/a.rs:6.\n\nThe guard in src/b.rs:2 stays.\n\nNothing else changes.");
    assert!(!flow_of(&ed, &a, 6).is_empty());
    let n = ed.workspace.claude.notes.list.iter_mut().find(|n| n.path == a && n.lines.start == 5).unwrap();
    n.thread.push(Entry { by: By::You, text: "why there?".into(), draft: false });
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let places = places_json(&ed.workspace.claude.notes, &c.msgs);
    restore(&mut ed, places.as_ref());
    assert!(flow_of(&ed, &a, 4).is_empty(), "the noted turn's: none");
    assert!(flow_of(&ed, &b, 2).is_empty(), "the thread's flow moved nowhere");
    let threads = ed.workspace.claude.notes.list.iter().filter(|n| !crate::notes::bare_chat(n)).count();
    assert_eq!(threads, 2, "the pinned note and the thread stay");
}

/// The reviewer's note 24: what the conversation keeps is what's there as you leave it, with no turn since — a note you
/// closed doesn't come back on a continue, and one answered with a reply comes back once, its thread, never a bare note
/// beside it. Kept as it changes: a plain quit keeps nothing.
#[test]
fn a_continue_brings_back_no_note_you_closed_nor_one_beside_its_thread() {
    let (s, root, mut ed) = project("chatnotes-closed", &["src/a.rs", "src/b.rs"]);
    let dir = s.join("chats");
    ed.chats_dir = Some(dir.clone());
    ed.workspace.claude.chat.as_mut().unwrap().conversation = Some("c1".into());
    // (claude a stand-in: what it's sent, never a process)
    let (tx, _rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    answer(&mut ed, "It is read in src/a.rs:3.");
    answer(&mut ed, "The guard in src/b.rs:2 checks it.");
    answer(&mut ed, "It is set in src/a.rs:7.");
    // Closed, and answered: no turn after either
    ed.open(&root.join("src/a.rs")).unwrap();
    feed(&mut ed, "3G");
    crate::typed::execute(&mut ed, "note-close").unwrap();
    let b = root.join("src/b.rs");
    let n = ed.workspace.claude.notes.list.iter_mut().find(|n| n.path == b).unwrap();
    n.thread.push(Entry { by: By::You, text: "why there?".into(), draft: false });
    // (the lines the store keeps: this exact list — a count would pass on an earlier save still landing)
    let kept = |dir: &Path, root: &Path| {
        let places = crate::chats::load(dir, root).into_iter().next().and_then(|c| c.places);
        let answers = places.as_ref().and_then(|p| p.as_array()).map_or(&[][..], |a| &a[..]);
        let lines = answers.iter().flat_map(|a| a["places"].as_array().unwrap());
        lines.map(|p| p["said"].as_u64().unwrap()).collect::<Vec<_>>()
    };
    crate::test_results::settle_until(&mut ed, |_| kept(&dir, &root) == [6]);
    // A new conversation, then this one continued
    crate::chat::reset_said(&mut ed, "a new conversation");
    let saved = crate::chats::load(&dir, &root).remove(0);
    crate::chat::continue_with(&mut ed, saved);
    assert_eq!(
        chat_notes(&ed, &root),
        [
            ("src/a.rs".to_string(), 7, "It is set in src/a.rs:7.".to_string()),
            ("src/b.rs".to_string(), 2, "The guard in src/b.rs:2 checks it.".to_string()),
        ]
    );
}

/// `:note-close` closes for good, so the note at the cursor wins (the user, 2026-10-30): never another line's because
/// its card is open; of a line's answers, the one its open card shows. With no note on the cursor's line, the open card's
/// in this file; one in another file isn't in sight, and is left. `q` and `esc` fold, close nothing.
#[test]
fn note_close_takes_the_note_at_the_cursor_then_the_open_cards() {
    let (_s, root, mut ed) = project("chatnotes-close", &["src/a.rs", "src/b.rs"]);
    let close = |ed: &mut Editor| crate::typed::execute(ed, "note-close");
    answer(&mut ed, "It is read in src/a.rs:3.");
    answer(&mut ed, "Then src/a.rs:5.");
    answer(&mut ed, "And src/a.rs:7.");
    answer(&mut ed, "Again src/a.rs:7.");
    ed.open(&root.join("src/a.rs")).unwrap();
    let lines = |ed: &Editor| chat_notes(ed, &root).iter().map(|n| n.1).collect::<Vec<_>>();
    let gist = |ed: &Editor, line: usize| {
        let n = chat_notes(ed, &root);
        n.iter().filter(|n| n.1 == line).map(|n| n.2.clone()).collect::<Vec<_>>()
    };
    // The turn's card open at line 7, the cursor on line 3: line 3's goes, the card stays
    feed(&mut ed, "3G");
    close(&mut ed).unwrap();
    assert_eq!(lines(&ed), [5, 7, 7]);
    assert!(ed.workspace.claude.notes.open.is_some(), "line 7's card still open");
    // No note on the cursor's line: the open card's — the answer it shows, of its line's two
    feed(&mut ed, "gg");
    close(&mut ed).unwrap();
    assert_eq!(gist(&ed, 7), ["And src/a.rs:7."], "the newest went: its card showed it");
    assert_eq!(lines(&ed), [5, 7]);
    // The cursor on a line of two answers, its card open: the card's, the newest, never the older one folded under it
    answer(&mut ed, "Also src/a.rs:5.");
    feed(&mut ed, "5G");
    close(&mut ed).unwrap();
    assert_eq!(gist(&ed, 5), ["Then src/a.rs:5."]);
    feed(&mut ed, "7G<ret>");
    close(&mut ed).unwrap();
    assert_eq!(lines(&ed), [5]);
    // `q` folds, closes nothing; then nothing at the cursor and no card: said
    feed(&mut ed, "5G<ret>q");
    assert_eq!(lines(&ed), [5]);
    feed(&mut ed, "gg");
    assert_eq!(close(&mut ed).unwrap_err(), "no note on this line");
    // A card open in another file isn't in sight: left
    answer(&mut ed, "Now src/b.rs:2.");
    assert_eq!(close(&mut ed).unwrap_err(), "no note on this line");
    assert_eq!(chat_notes(&ed, &root).len(), 2);
    // The reviewer's note 31: a card you opened yourself, its note closed, holds no later turn's card back
    feed(&mut ed, "5G<ret>");
    assert!(ed.workspace.claude.notes.open.is_some() && !ed.workspace.claude.notes.turned, "yours");
    close(&mut ed).unwrap();
    answer(&mut ed, "Lastly src/b.rs:9.");
    let open = ed.workspace.claude.notes.open;
    let n = ed.workspace.claude.notes.list.iter().find(|n| Some(n.id) == open);
    assert_eq!(n.map(|n| n.lines.start + 1), Some(9), "the turn's opened");
}

/// `:note-close` while follow holds the view (the user, 2026-10-30, CLAUDE.md's follow rule): a key of yours acts at your
/// cursor — the note on your cursor's line goes, never the one at Claude's place, follow pauses and the view comes back
/// to your cursor. With none on your line, the open card at Claude's place is refused too, and nothing moves; paused,
/// that card is simply the open one.
#[test]
fn note_close_acts_at_your_cursor_while_follow_holds_the_view() {
    let (_s, root, mut ed) = project("chatnotes-close-follow", &["src/a.rs"]);
    let long: String = (1..=60).map(|i| format!("let line_{i} = {i};\n")).collect();
    std::fs::write(root.join("src/c.rs"), long).unwrap();
    ed.open(&root.join("src/a.rs")).unwrap();
    ed.workspace.claude.chat.as_mut().unwrap().follow.on = true;
    screen(&mut ed);
    let close = |ed: &mut Editor| crate::typed::execute(ed, "note-close");
    let lines = |ed: &Editor| chat_notes(ed, &root).iter().map(|n| n.1).collect::<Vec<_>>();
    let paused = |ed: &Editor| ed.workspace.claude.chat.as_ref().unwrap().follow.paused;
    answer(&mut ed, "It starts in src/c.rs:1.");
    answer(&mut ed, "It is read in src/c.rs:40.");
    screen(&mut ed);
    assert_eq!(
        (crate::follow::view_line(&ed), ed.cursor_line()),
        (Some(39), 0),
        "Claude's place, your cursor apart"
    );
    assert_ne!(ed.doc().top, 0, "the screen is Claude's");
    close(&mut ed).unwrap();
    assert_eq!(lines(&ed), [40], "your line's, never Claude's");
    assert!(paused(&ed) && crate::follow::view_line(&ed).is_none(), "follow paused");
    assert!(code_rows(&mut ed).iter().any(|r| r.contains("let line_1 = 1;")), "the view back at your cursor");
    // Nothing on your line, the turn's card open at Claude's place: refused, follow leading on, the view where it was
    answer(&mut ed, "Now src/c.rs:50.");
    screen(&mut ed);
    let top = ed.doc().top;
    assert_eq!(close(&mut ed).unwrap_err(), "no note on this line");
    assert!(!paused(&ed) && crate::follow::view_line(&ed) == Some(49), "follow still leads");
    assert_eq!((ed.doc().top, lines(&ed)), (top, vec![40, 50]));
    // (paused it's no one's place: the open card's)
    ed.workspace.claude.chat.as_mut().unwrap().follow.paused = true;
    close(&mut ed).unwrap();
    assert_eq!(lines(&ed), [40]);
}

/// Follow leading at src/c.rs:40, the turn's card open there, your cursor on line 1 — an answer's note there when `yours`.
fn led_apart(name: &str, yours: bool) -> (crate::scratch::Scratch, PathBuf, Editor) {
    let (s, root, mut ed) = project(name, &["src/a.rs"]);
    let long: String = (1..=60).map(|i| format!("let line_{i} = {i};\n")).collect();
    std::fs::write(root.join("src/c.rs"), long).unwrap();
    ed.open(&root.join("src/a.rs")).unwrap();
    ed.workspace.claude.chat.as_mut().unwrap().follow.on = true;
    screen(&mut ed);
    if yours {
        answer(&mut ed, "It starts in src/c.rs:1.");
    }
    answer(&mut ed, "It is read in src/c.rs:40.");
    screen(&mut ed);
    let claudes = newest_of(&ed, |n| n.lines.start == 39).map(|n| n.id);
    assert_eq!(
        (crate::follow::view_line(&ed), ed.cursor_line(), ed.workspace.claude.notes.open),
        (Some(39), 0, claudes),
        "Claude's place and its card, your cursor apart"
    );
    (s, root, ed)
}

/// Follow paused and the view back at your cursor (line 1).
fn yours_again(ed: &mut Editor) -> bool {
    let paused = ed.workspace.claude.chat.as_ref().unwrap().follow.paused;
    paused
        && crate::follow::view_line(ed).is_none()
        && code_rows(ed).iter().any(|r| r.contains("let line_1 = 1;"))
}

/// #250: `space n` while follow holds the view replies to the note at your cursor, never the one at Claude's place; follow
/// pauses and the view comes back to your cursor. With none on your line it's a new note there, never a reply to the
/// turn's card at Claude's place — and that card says neither `space n` nor `enter`.
#[test]
fn space_n_acts_at_your_cursor_while_follow_holds_the_view() {
    use crate::editor::PromptKind;
    let kind = |ed: &Editor| ed.prompt.as_ref().map(|p| p.kind);
    let (_s, _root, mut ed) = led_apart("chatnotes-reply-follow", true);
    let yours = newest_of(&ed, |n| n.lines.start == 0).unwrap().id;
    feed(&mut ed, " n");
    assert!(matches!(kind(&ed), Some(PromptKind::Note(Some(i))) if i == yours), "{:?}", kind(&ed));
    assert!(yours_again(&mut ed), "follow paused, the view back at your cursor");
    let (_s, _root, mut ed) = led_apart("chatnotes-reply-follow-none", false);
    let rows = code_rows(&mut ed).join("\n");
    assert!(rows.contains("╭─ chat") && !rows.contains("space n") && !rows.contains("enter"), "{rows}");
    feed(&mut ed, " n");
    assert!(matches!(kind(&ed), Some(PromptKind::Note(None))), "{:?}", kind(&ed));
    assert!(yours_again(&mut ed), "follow paused, the view back at your cursor");
}

/// #250: `enter` while follow holds the view opens the card of the note at your cursor — never shuts the turn's card at
/// Claude's place; follow pauses and the view comes back to your cursor. With none on your line `enter` isn't the card's
/// at all; `q` and `esc` still close the card that's open — shutting what's in sight acts on no note.
#[test]
fn enter_acts_at_your_cursor_while_follow_holds_the_view() {
    use crate::key::{Code, Key};
    let (_s, _root, mut ed) = led_apart("chatnotes-enter-follow", true);
    let yours = newest_of(&ed, |n| n.lines.start == 0).unwrap().id;
    feed(&mut ed, "<ret>");
    assert_eq!(ed.workspace.claude.notes.open, Some(yours), "your note's card");
    assert!(yours_again(&mut ed), "follow paused, the view back at your cursor");
    let (_s, _root, ed) = led_apart("chatnotes-enter-follow-none", false);
    let keys = crate::notes::card_keys(&ed);
    assert!(!keys.contains(&Key::plain(Code::Enter)), "{keys:?}");
    assert!(keys.contains(&Key::plain(Code::Char('q'))) && keys.contains(&Key::plain(Code::Esc)), "{keys:?}");
}

/// #250: `]n` while follow holds the view walks on from the note at your cursor, never from the one at Claude's place.
#[test]
fn next_note_walks_on_from_your_cursor_while_follow_holds_the_view() {
    let (_s, _root, mut ed) = led_apart("chatnotes-step-follow", true);
    crate::notes::tool_call(&mut ed, &serde_json::json!({"path": "src/c.rs", "line": 20, "text": "pinned"}))
        .unwrap();
    ed.workspace.claude.notes.focus = None;
    assert_ne!(crate::follow::view_line(&ed).unwrap_or(0), 0, "follow holds the view apart");
    // (the walk: the newest answer's 40, then 1 and 20 by line)
    feed(&mut ed, "]n");
    assert_eq!(ed.cursor_line(), 19, "the next after yours");
    let paused = ed.workspace.claude.chat.as_ref().unwrap().follow.paused;
    assert!(paused && crate::follow::view_line(&ed).is_none(), "follow paused");
    let rows = code_rows(&mut ed).join("\n");
    assert!(rows.contains("let line_20 = 20;") && rows.contains("pinned"), "its card in sight: {rows}");
}

/// Reading step 2 over: a turn's card leaves your cursor where it was, so its keys are said only where they act on it.
/// With no note on your line `enter` is the card's, and `space n` a question of its own there (focus C1, the user on its
/// shots), not said; its words pressed are the card's wherever you are; on another note's line the keys are that note's and
/// the card doesn't say them, nor counts that line's notes as its own.
#[test]
fn the_open_cards_keys_act_on_it_or_it_doesnt_say_them() {
    let (_s, root, mut ed) = project("chatnotes-aimed", &["src/a.rs"]);
    let a = root.join("src/a.rs");
    answer(&mut ed, "First src/a.rs:2.");
    answer(&mut ed, "It is read in src/a.rs:7.");
    answer(&mut ed, "Again src/a.rs:7.");
    let card = newest_of(&ed, |n| n.path == a && n.lines.start == 6).unwrap().id;
    crate::notes::tool_call(&mut ed, &serde_json::json!({"path": "src/a.rs", "line": 2, "text": "pinned"}))
        .unwrap();
    let pinned = ed.workspace.claude.notes.list.iter().find(|n| n.answer.is_none()).unwrap().id;
    let aimed = |ed: &Editor| ed.prompt.as_ref().map(|p| p.kind);
    let is = |k: Option<crate::editor::PromptKind>, id: u64| matches!(k, Some(crate::editor::PromptKind::Note(Some(i))) if i == id);
    ed.open(&a).unwrap();
    // On another note's line: its keys, the card saying neither, nor that line's count
    feed(&mut ed, "2G");
    let rows = code_rows(&mut ed).join("\n");
    assert!(rows.contains("… 1 earlier") && rows.contains("╭─ chat"), "{rows}");
    assert!(!rows.contains("space n") && !rows.contains("enter"), "{rows}");
    assert!(!rows.contains(" of "), "its own line holds one: {rows}");
    feed(&mut ed, " n");
    assert!(is(aimed(&ed), pinned), "{:?}", aimed(&ed));
    feed(&mut ed, "<esc>");
    // (its words pressed: the card's)
    crate::notes::start_card(&mut ed);
    assert!(is(aimed(&ed), card), "{:?}", aimed(&ed));
    feed(&mut ed, "<esc>");
    // On a line with none: `enter` the card's, and it says so; `space n` your line's
    feed(&mut ed, "4G");
    let rows = code_rows(&mut ed).join("\n");
    assert!(!rows.contains("space n") && rows.contains("enter"), "{rows}");
    feed(&mut ed, " n");
    assert_eq!(aimed(&ed), Some(crate::editor::PromptKind::Note(None)));
    feed(&mut ed, "<esc><ret>");
    assert!(ed.workspace.claude.notes.earlier, "`enter` unfolded them, as the card said");
    let rows = code_rows(&mut ed).join("\n");
    assert!(rows.contains("It is read in src/a.rs:7."), "{rows}");
}

/// Focus C1: on a line's answers `space n`'s line stands at the card's foot, `… N earlier` above it, and answers the newest;
/// sent, the card shows that thread, your words on it — a press on a turn's card at Claude's place holds the view there,
/// follow paused, the card you pressed in sight with the line in it.
#[test]
fn a_reply_on_a_lines_answers_stands_at_the_cards_foot_and_stays_sent() {
    let (_s, root, mut ed) = project("chatnotes-reply-card", &["src/a.rs"]);
    ed.config.llm.command = "sh".into();
    let a = root.join("src/a.rs");
    answer(&mut ed, "It is read in src/a.rs:7.");
    answer(&mut ed, "Again src/a.rs:7.");
    let newest = newest_of(&ed, |n| n.path == a && n.lines.start == 6).unwrap().id;
    ed.open(&a).unwrap();
    feed(&mut ed, "7G nsure");
    let rows = code_rows(&mut ed);
    let at = |s: &str| rows.iter().position(|r| r.contains(s)).unwrap_or_else(|| panic!("{s}: {rows:#?}"));
    assert!(at("Again src/a.rs:7") < at("… 1 earlier") && at("… 1 earlier") < at("reply: sure"), "{rows:#?}");
    assert_eq!(at("enter send"), at("reply: sure") + 1, "its frame's foot under it: {rows:#?}");
    feed(&mut ed, "<ret>");
    assert_eq!(ed.workspace.claude.notes.open, Some(newest), "the thread's card");
    let rows = code_rows(&mut ed);
    assert!(rows.iter().any(|r| r.contains("sure")), "your words on it: {rows:#?}");
}

/// A press on the card follow holds in sight: the card stays, its line in it, follow paused — never the view gone to your
/// cursor, nor on with Claude under what you type; nor once the line ends, `esc` or sent.
#[test]
fn a_press_on_the_card_at_claudes_place_holds_it_in_sight() {
    let (_s, _root, mut ed) = led_apart("chatnotes-press-follow", false);
    ed.config.llm.command = "sh".into();
    assert!(crate::follow::view_line(&ed).is_some(), "follow holds the view");
    let top = ed.doc().top;
    crate::notes::start_card(&mut ed);
    let rows = code_rows(&mut ed).join("\n");
    assert!(rows.contains("reply:") && rows.contains("╭─ chat"), "{rows}");
    let f = &ed.workspace.claude.chat.as_ref().unwrap().follow;
    assert!(f.paused && crate::follow::view_line(&ed).is_none(), "follow paused");
    assert!(!ed.toasts.iter().any(|t| t.text.contains("you took over")), "no turn under way to take over");
    feed(&mut ed, "<esc>");
    let rows = code_rows(&mut ed).join("\n");
    assert_eq!(ed.doc().top, top, "`esc`: the view as it was");
    assert!(rows.contains("╭─ chat"), "the card in sight: {rows}");
    crate::notes::start_card(&mut ed);
    code_rows(&mut ed);
    feed(&mut ed, "sure<ret>");
    let rows = code_rows(&mut ed).join("\n");
    assert!(rows.contains("sure"), "sent, what you wrote in sight: {rows}");
}

/// That press with a turn under way takes it over: said, with the key that follows Claude again, as any key of yours that
/// pauses follow — no silent state change.
#[test]
fn a_press_on_the_card_at_claudes_place_mid_turn_says_you_took_over() {
    let (_s, _root, mut ed) = led_apart("chatnotes-press-said", false);
    ed.workspace.claude.chat.as_mut().unwrap().state = crate::chat::State::Thinking;
    assert!(crate::follow::view_line(&ed).is_some(), "follow holds the view");
    crate::notes::start_card(&mut ed);
    assert!(ed.workspace.claude.chat.as_ref().unwrap().follow.paused, "follow paused");
    assert!(ed.toasts.iter().any(|t| t.text.contains("you took over")), "said");
}

/// Reading step 2 over: a line inside an older answer's range acts on the note whose `¶` is on it — what the line end
/// says — never that older one because it began higher up.
#[test]
fn a_line_inside_an_older_answers_range_acts_on_its_own_note() {
    let (_s, root, mut ed) = project("chatnotes-range", &["src/a.rs"]);
    let a = root.join("src/a.rs");
    answer(&mut ed, "It spans src/a.rs:3-6.");
    answer(&mut ed, "Now src/a.rs:5.");
    ed.open(&a).unwrap();
    feed(&mut ed, "q5G n");
    let own = ed.workspace.claude.notes.list.iter().find(|n| n.lines.start == 4).unwrap().id;
    let kind = ed.prompt.as_ref().map(|p| p.kind);
    assert!(matches!(kind, Some(crate::editor::PromptKind::Note(Some(i))) if i == own), "{kind:?}");
    feed(&mut ed, "<esc><ret>");
    assert_eq!(ed.workspace.claude.notes.open, Some(own));
    crate::typed::execute(&mut ed, "note-close").unwrap();
    assert_eq!(chat_notes(&ed, &root).iter().map(|n| n.1).collect::<Vec<_>>(), [3]);
    // (a line inside a range two answers share, neither starting on it: the newest, what the card shows)
    answer(&mut ed, "Still src/a.rs:3-6.");
    feed(&mut ed, "q4G n");
    let newest = newest_of(&ed, |n| n.path == a && n.lines.start == 2).unwrap().id;
    let kind = ed.prompt.as_ref().map(|p| p.kind);
    assert!(matches!(kind, Some(crate::editor::PromptKind::Note(Some(i))) if i == newest), "{kind:?}");
}

/// `:note-close` on a line where another note has the focus: the open card's goes, the one in sight — not the note pinned
/// beside it.
#[test]
fn note_close_on_a_line_of_two_notes_takes_the_open_cards() {
    let (_s, root, mut ed) = project("chatnotes-card-first", &["src/a.rs"]);
    answer(&mut ed, "Here src/a.rs:9.");
    crate::notes::tool_call(&mut ed, &serde_json::json!({"path": "src/a.rs", "line": 9, "text": "pinned"}))
        .unwrap();
    ed.open(&root.join("src/a.rs")).unwrap();
    feed(&mut ed, "9G");
    crate::typed::execute(&mut ed, "note-close").unwrap();
    assert!(chat_notes(&ed, &root).is_empty(), "{:?}", chat_notes(&ed, &root));
    assert_eq!(ed.workspace.claude.notes.list.len(), 1, "the pinned one stays");
}

/// Reading step 2 over: a place whose path is no text — under a folder so named, its name found by its last parts — is
/// kept by no store; `json!` would panic, and a release build aborts on one.
#[test]
fn a_place_whose_path_is_no_text_is_left_out_of_the_store() {
    use std::os::unix::ffi::OsStrExt;
    let (_s, root, mut ed) = project("chatnotes-bytes", &["src/a.rs", "src/b.rs"]);
    answer(&mut ed, "It is read in src/a.rs:3 and src/b.rs:2.");
    let b = root.join("src/b.rs");
    let n = ed.workspace.claude.notes.list.iter_mut().find(|n| n.path == b).unwrap();
    n.path = PathBuf::from(std::ffi::OsStr::from_bytes(b"/x/\xff/b.rs"));
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let places = places_json(&ed.workspace.claude.notes, &c.msgs).unwrap();
    assert_eq!(places[0]["places"].as_array().unwrap().len(), 1, "{places}");
}

/// The reviewer's note 34: a line two answers name acts on the newest, what the card and the line end show — `space n`
/// replies to it and `:note-close` closes it, with its card opened by `enter` or shut — never the older one folded under it.
#[test]
fn a_line_of_two_answers_acts_on_the_one_in_sight() {
    let (_s, root, mut ed) = project("chatnotes-shown", &["src/a.rs"]);
    let a = root.join("src/a.rs");
    let close = |ed: &mut Editor| crate::typed::execute(ed, "note-close").unwrap();
    let gists = |ed: &Editor| chat_notes(ed, &root).into_iter().map(|n| n.2).collect::<Vec<_>>();
    answer(&mut ed, "It is read in src/a.rs:3.");
    answer(&mut ed, "Again src/a.rs:3.");
    ed.open(&a).unwrap();
    feed(&mut ed, "q3G n");
    let newest = newest_of(&ed, |n| n.path == a && n.lines.start == 2).unwrap().id;
    let kind = ed.prompt.as_ref().map(|p| p.kind);
    assert!(matches!(kind, Some(crate::editor::PromptKind::Note(Some(i))) if i == newest), "{kind:?}");
    feed(&mut ed, "<esc>");
    // (its card opened by `enter`: that answer's, and it's the one that goes)
    feed(&mut ed, "<ret>");
    assert_eq!(ed.workspace.claude.notes.open, Some(newest));
    close(&mut ed);
    assert_eq!(gists(&ed), ["It is read in src/a.rs:3."]);
    // (the card shut: the same)
    answer(&mut ed, "Once more src/a.rs:3.");
    feed(&mut ed, "q");
    close(&mut ed);
    assert_eq!(gists(&ed), ["It is read in src/a.rs:3."]);
    // (the older one's `¶` pressed: the card it opens still shows the newest on top, and that's what goes)
    answer(&mut ed, "Yet again src/a.rs:3.");
    feed(&mut ed, "q");
    let notes = ed.workspace.claude.notes.list.iter();
    let older = notes.into_iter().find(|n| n.gist().starts_with("It is read")).unwrap().id;
    crate::notes::pressed(&mut ed, older);
    assert_eq!(ed.workspace.claude.notes.open, Some(older));
    close(&mut ed);
    assert_eq!(gists(&ed), ["It is read in src/a.rs:3."]);
}

/// The reviewer's note 35: a chat closed mid-turn — your question in no store yet, a keep then counted from it — keeps its
/// answers' places: a note closed then, the others come back on a continue, none said lost.
#[test]
fn a_chat_closed_mid_turn_keeps_its_places_right() {
    let (s, root, mut ed) = project("chatnotes-mid-turn", &["src/a.rs"]);
    let dir = s.join("chats");
    ed.chats_dir = Some(dir.clone());
    ed.workspace.claude.chat.as_mut().unwrap().conversation = Some("c1".into());
    // (claude a stand-in: what it's sent, never a process)
    let (tx, _rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    answer(&mut ed, "It is read in src/a.rs:3.");
    answer(&mut ed, "It is set in src/a.rs:7.");
    let kept = |dir: &Path, root: &Path| {
        let places = crate::chats::load(dir, root).into_iter().find(|c| c.id == "c1").and_then(|c| c.places);
        let answers = places.as_ref().and_then(|p| p.as_array()).map_or(&[][..], |a| &a[..]);
        let lines = answers.iter().flat_map(|a| a["places"].as_array().unwrap());
        lines.map(|p| p["said"].as_u64().unwrap()).collect::<Vec<_>>()
    };
    crate::test_results::settle_until(&mut ed, |_| kept(&dir, &root) == [2, 6]);
    // The next question sent, its turn cut off by the close: kept nowhere
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::User, "and then?", ""));
    crate::chat::close(&mut ed);
    let earlier = crate::chat::left_kept(&ed).unwrap();
    ed.open(&root.join("src/a.rs")).unwrap();
    feed(&mut ed, "3G");
    crate::typed::execute(&mut ed, "note-close").unwrap();
    crate::test_results::settle_until(&mut ed, |_| kept(&dir, &root) == [6]);
    // (a keep taken before that one landing after it: refused, the closed note stays closed)
    crate::chats::save(&dir, &root, earlier);
    assert_eq!(kept(&dir, &root), [6]);
    crate::chat::open(&mut ed);
    let saved = crate::chats::load(&dir, &root).into_iter().find(|c| c.id == "c1").unwrap();
    crate::chat::continue_with(&mut ed, saved);
    let lost = ed.messages.iter().any(|m| m.2.contains("no longer found"));
    assert!(!lost, "{:?}", ed.messages.iter().map(|m| &m.2).collect::<Vec<_>>());
    assert_eq!(chat_notes(&ed, &root), [("src/a.rs".to_string(), 7, "It is set in src/a.rs:7.".to_string())]);
}

/// #252: a note closed with the chat open keeps the conversation's places, not its time: `updated` and its place in the
/// list stay what its last message made them, so the continue picker's order and age say the talk.
#[test]
fn a_note_closed_leaves_the_conversations_time_alone() {
    let (s, root, mut ed) = project("chatnotes-updated", &["src/a.rs"]);
    let dir = s.join("chats");
    answer(&mut ed, "It is read in src/a.rs:3.");
    answer(&mut ed, "It is set in src/a.rs:7.");
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let saved = |id: &str, updated: u64, places: Option<serde_json::Value>| crate::chats::Saved {
        id: id.into(),
        title: "where?".into(),
        updated,
        rows: crate::chats::rows_of(&c.msgs),
        plan: None,
        usage: None,
        places,
        seq: crate::chats::seq(),
    };
    let places = places_json(&ed.workspace.claude.notes, &c.msgs);
    let (older, newer) = (saved("older", 1000, places), saved("newer", 2000, None));
    crate::chats::save(&dir, &root, older);
    crate::chats::save(&dir, &root, newer);
    ed.chats_dir = Some(dir.clone());
    // (claude a stand-in: what it's sent, never a process)
    let (tx, _rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    let older = crate::chats::load(&dir, &root).into_iter().find(|c| c.id == "older").unwrap();
    crate::chat::continue_with(&mut ed, older);
    ed.open(&root.join("src/a.rs")).unwrap();
    feed(&mut ed, "3G");
    crate::typed::execute(&mut ed, "note-close").unwrap();
    let places = |c: &crate::chats::Saved| c.places.as_ref().map_or(0, |p| p.as_array().unwrap().len());
    crate::test_results::settle_until(&mut ed, |_| {
        crate::chats::load(&dir, &root).iter().any(|c| c.id == "older" && places(c) == 1)
    });
    let list: Vec<_> = crate::chats::load(&dir, &root).into_iter().map(|c| (c.id, c.updated)).collect();
    assert_eq!(list, [("newer".to_string(), 2000), ("older".to_string(), 1000)]);
}

/// The reviewer's note 36, through a real keep: an answer's place in an added folder has its line's text in the store,
/// what finds the line again on a continue.
#[test]
fn a_place_in_a_folder_keeps_its_line_text() {
    let (s, root, mut ed) = project("chatnotes-folder", &["src/a.rs"]);
    let outside = crate::scratch::Scratch::new("chatnotes-folder-infra");
    let infra = std::fs::canonicalize(&*outside).unwrap();
    std::fs::write(infra.join("deploy.rs"), "let one = 1;\nlet two = 2;\n").unwrap();
    let folder = crate::folders::Folder { at: infra, name: "infra".into(), instructions: Vec::new() };
    ed.workspace.folders.push(folder);
    let dir = s.join("chats");
    ed.chats_dir = Some(dir.clone());
    ed.workspace.claude.chat.as_mut().unwrap().conversation = Some("c1".into());
    answer(&mut ed, "It is set in deploy.rs:2.");
    let text = |dir: &Path, root: &Path| {
        let c = crate::chats::load(dir, root).into_iter().next();
        c.and_then(|c| c.places).map(|p| p[0]["places"][0]["text"].clone())
    };
    crate::test_results::settle_until(&mut ed, |_| text(&dir, &root).is_some_and(|t| !t.is_null()));
    assert_eq!(text(&dir, &root).unwrap(), "let two = 2;");
}

/// The reviewer's note 30: with the chat closed its answers are still at the code and still its conversation's — a note
/// closed then, and one answered (the reply opens a chat: a new conversation), are kept as such, so a continue brings
/// back neither the closed one nor a bare note beside the thread.
#[test]
fn what_you_do_with_the_chat_closed_stays_done_on_a_continue() {
    let (s, root, mut ed) = project("chatnotes-left", &["src/a.rs", "src/b.rs"]);
    let dir = s.join("chats");
    ed.chats_dir = Some(dir.clone());
    ed.workspace.claude.chat.as_mut().unwrap().conversation = Some("c1".into());
    // (claude a stand-in: what it's sent, never a process)
    let (tx, _rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    answer(&mut ed, "It is read in src/a.rs:3.");
    answer(&mut ed, "The guard in src/b.rs:2 checks it.");
    answer(&mut ed, "It is set in src/a.rs:7.");
    let kept = |dir: &Path, root: &Path| {
        let places = crate::chats::load(dir, root).into_iter().find(|c| c.id == "c1").and_then(|c| c.places);
        let answers = places.as_ref().and_then(|p| p.as_array()).map_or(&[][..], |a| &a[..]);
        let lines = answers.iter().flat_map(|a| a["places"].as_array().unwrap());
        lines.map(|p| p["said"].as_u64().unwrap()).collect::<Vec<_>>()
    };
    crate::test_results::settle_until(&mut ed, |_| kept(&dir, &root) == [2, 1, 6]);
    crate::chat::close(&mut ed);
    // Closed, the chat closed
    ed.open(&root.join("src/a.rs")).unwrap();
    feed(&mut ed, "3G");
    crate::typed::execute(&mut ed, "note-close").unwrap();
    crate::test_results::settle_until(&mut ed, |_| kept(&dir, &root) == [1, 6]);
    // Answered: the reply is in the thread as it opens the chat, which forgets the rest
    let b = root.join("src/b.rs");
    let n = ed.workspace.claude.notes.list.iter_mut().find(|n| n.path == b).unwrap();
    n.thread.push(Entry { by: By::You, text: "why there?".into(), draft: false });
    let id = n.id;
    let reply = crate::notes::Reply { id, text: "why there?".into(), fresh: false, why: None };
    crate::notes::send(&mut ed, vec![reply], None);
    assert!(ed.workspace.claude.chat.is_some(), "the reply opened it");
    crate::test_results::settle_until(&mut ed, |_| kept(&dir, &root) == [6]);
    // Continued, the reply's turn over (the stand-in never answers it): the line left, and the thread once
    ed.workspace.claude.chat.as_mut().unwrap().state = crate::chat::State::Idle;
    let saved = crate::chats::load(&dir, &root).into_iter().find(|c| c.id == "c1").unwrap();
    crate::chat::continue_with(&mut ed, saved);
    assert_eq!(ed.workspace.claude.chat.as_ref().unwrap().conversation.as_deref(), Some("c1"), "continued");
    assert_eq!(
        chat_notes(&ed, &root),
        [
            ("src/a.rs".to_string(), 7, "It is set in src/a.rs:7.".to_string()),
            ("src/b.rs".to_string(), 2, "The guard in src/b.rs:2 checks it.".to_string()),
        ]
    );
}

/// The reviewer's note 26: closing the chat leaves its answers at the code (the card reads best with the chat closed),
/// but the chat's next opening is a new conversation: they go then, `:chat-new` with it closed too — never kept with the
/// next one's rows — and a continue brings them back.
#[test]
fn a_new_conversation_after_the_chat_closed_takes_none_of_its_answers() {
    let (s, root, mut ed) = project("chatnotes-chat-closed", &["src/a.rs", "src/b.rs"]);
    let dir = s.join("chats");
    ed.chats_dir = Some(dir.clone());
    // (claude a stand-in: what it's sent, never a process)
    let (tx, _rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    let kept = |dir: &Path, root: &Path, id: &str| {
        let c = crate::chats::load(dir, root).into_iter().find(|c| c.id == id);
        let places = c.and_then(|c| c.places);
        let answers = places.as_ref().and_then(|p| p.as_array()).map_or(&[][..], |a| &a[..]);
        answers.iter().flat_map(|a| a["places"].as_array().unwrap()).count()
    };
    ed.workspace.claude.chat.as_mut().unwrap().conversation = Some("c1".into());
    answer(&mut ed, "It is read in src/a.rs:3.");
    crate::test_results::settle_until(&mut ed, |_| kept(&dir, &root, "c1") == 1);
    let first = [("src/a.rs".to_string(), 3, "It is read in src/a.rs:3.".to_string())];
    // Closed: still at the code. Opened again: a new conversation
    crate::chat::close(&mut ed);
    assert_eq!(chat_notes(&ed, &root), first);
    crate::chat::open(&mut ed);
    assert!(chat_notes(&ed, &root).is_empty());
    // (its keeps hold its own answers only)
    ed.workspace.claude.chat.as_mut().unwrap().conversation = Some("c2".into());
    answer(&mut ed, "Then src/b.rs:2.");
    crate::test_results::settle_until(&mut ed, |_| kept(&dir, &root, "c2") == 1);
    // `:chat-new` with the chat closed: the same
    crate::chat::close(&mut ed);
    crate::chat::reset_said(&mut ed, "a new conversation");
    assert!(chat_notes(&ed, &root).is_empty());
    // The first continued: back
    let saved = crate::chats::load(&dir, &root).into_iter().find(|c| c.id == "c1").unwrap();
    crate::chat::continue_with(&mut ed, saved);
    assert_eq!(chat_notes(&ed, &root), first);
}

/// Focus A1b: the turn's end opens the newest answer's first place's card (no key, your cursor where it was); `]n` walks
/// that answer's places in the order written, then the rest by file; a line two answers name is one card, the newest on
/// top, the earlier folded — `enter` unfolds it, `enter` again closes. A card you opened yourself keeps the turn's from
/// opening; while you type it waits, then opens.
#[test]
fn the_newest_answer_opens_its_card_and_walks_first() {
    let (_s, root, mut ed) = project("chatnotes-newest", &["src/a.rs", "src/b.rs"]);
    let open = |ed: &Editor| {
        let id = ed.workspace.claude.notes.open?;
        let n = ed.workspace.claude.notes.list.iter().find(|n| n.id == id)?;
        Some((crate::chat::shown(&root, &n.path), n.lines.start + 1))
    };
    let at = |p: &str, l: usize| Some((p.to_string(), l));
    answer(&mut ed, "It is read in src/a.rs:3.");
    assert_eq!(open(&ed), at("src/a.rs", 3));
    answer(&mut ed, "Now src/b.rs:7 reads it, src/a.rs:9 sets it and src/a.rs:3 again.");
    assert_eq!((open(&ed), ed.cursor_line()), (at("src/b.rs", 7), 0), "the newest's first place, no key");
    // `]n`: the newest's places as written (by file, `src/a.rs:3` would come first)
    ed.open(&root.join("src/b.rs")).unwrap();
    feed(&mut ed, "]n");
    assert_eq!(open(&ed), at("src/a.rs", 9));
    feed(&mut ed, "]n");
    assert_eq!((open(&ed), ed.cursor_line()), (at("src/a.rs", 3), 2));
    // (its line's two answers one card: the newest on top, the earlier folded; `enter` unfolds, `enter` closes)
    let card = code_rows(&mut ed).join("\n");
    assert!(card.contains("2 answers here") && card.contains("… 1 earlier"), "{card}");
    assert!(!card.contains("It is read in src/a.rs:3."), "the earlier folded: {card}");
    feed(&mut ed, "<ret>");
    let card = code_rows(&mut ed).join("\n");
    assert!(card.contains("It is read in src/a.rs:3.") && !card.contains("… 1 earlier"), "{card}");
    feed(&mut ed, "<ret>");
    assert_eq!(open(&ed), None, "closed");
    // A card you opened yourself: the next answer's doesn't take its place
    feed(&mut ed, "gg]n");
    let yours = open(&ed);
    answer(&mut ed, "Then src/b.rs:2.");
    assert_eq!(open(&ed), yours);
    // Typing: the turn's card waits, then opens
    feed(&mut ed, "q");
    ed.workspace.claude.chat.as_mut().unwrap().input = "and then?".into();
    answer(&mut ed, "Last src/b.rs:4.");
    assert_eq!(open(&ed), None, "not while you type");
    ed.workspace.claude.chat.as_mut().unwrap().input.clear();
    sync(&mut ed);
    assert_eq!(open(&ed), at("src/b.rs", 4));
    // A new conversation (`C-l`): they go, their card with them (claude a stand-in: what it's sent, never a process)
    let (tx, _rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    crate::chat::reset_said(&mut ed, "a new conversation");
    assert_eq!((chat_notes(&ed, &root), open(&ed)), (Vec::new(), None));
}

/// Focus A1b: where follow leads, the card the turn's end opens is in sight with its line — follow takes the view there
/// as to the answer's place, your cursor where it was; off (or paused), the view stays yours.
#[test]
fn the_turns_card_is_in_sight_where_follow_leads() {
    let (_s, root, mut ed) = project("chatnotes-sight", &["src/a.rs"]);
    // (lines that wrap in the code beside the chat)
    let tail = "// the strand kept in order, whatever the caller passes in, as the map holds it";
    let long: String = (1..=60).map(|i| format!("let line_{i} = {i}; {tail}\n")).collect();
    std::fs::write(root.join("src/c.rs"), long).unwrap();
    ed.open(&root.join("src/a.rs")).unwrap();
    ed.workspace.claude.chat.as_mut().unwrap().follow.on = true;
    screen(&mut ed);
    answer(
        &mut ed,
        "It is read in src/c.rs:40.\n\nIt never changes after that, whatever the caller passes in.",
    );
    let rows = code_rows(&mut ed);
    assert_eq!(ed.doc().path.as_deref(), Some(root.join("src/c.rs").as_path()), "follow took the view there");
    let line = rows.iter().position(|r| r.contains("let line_40 = 40;"));
    let foot = rows.iter().position(|r| r.trim_start().starts_with('╰'));
    assert!(line.is_some_and(|l| foot.is_some_and(|f| f > l)), "{}", rows.join("\n"));
    assert_eq!(ed.cursor_line(), 0, "your cursor where it was");
    // Its line in sight already at the pane's foot, wrapped beside the chat (the user's 80-column shot): the frame puts
    // the card's foot in sight too, the block measured at the pane's size as laid, never the last key's
    feed(&mut ed, "q");
    ed.doc_mut().top = 33;
    // (the keys in the chat, a question just sent)
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    code_rows(&mut ed);
    answer(&mut ed, "Still src/c.rs:45, and nothing else reads it after that, whatever the caller passes.");
    let rows = code_rows(&mut ed);
    let line = rows.iter().position(|r| r.contains("let line_45 = 45;"));
    let foot = rows.iter().position(|r| r.trim_start().starts_with('╰'));
    assert!(line.is_some_and(|l| foot.is_some_and(|f| f > l)), "{}", rows.join("\n"));
    // (off: the view yours — at your cursor, as the frame keeps it; the card opens under its line, out of sight)
    ed.workspace.claude.chat.as_mut().unwrap().follow.on = false;
    answer(&mut ed, "Then src/c.rs:55.");
    screen(&mut ed);
    let notes = &ed.workspace.claude.notes;
    let open = notes.open.and_then(|id| notes.list.iter().find(|n| n.id == id)).map(|n| n.lines.start);
    assert_eq!((open, ed.doc().top), (Some(54), 0));
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

/// The key-sequence walk: an answer's places are notes at their lines — the turn's end opens its place's card, `]n`
/// goes to them with the others, the line end its sentence, the card its paragraph named `chat`; a name that comes to
/// no file (or to several) is no place and fenced code none — its block is the answer's note, at the answer's first
/// place (focus A1); `space n` on one answers it, a thread then; the next answer's places join them, the line end the
/// newest's words only (focus A1b); an answer whose names come to nothing, its turn stopped nowhere, adds none. A
/// restart's notes keep only a thread (the answers' places come back with their conversation — `restore`).
#[test]
fn the_answers_places_are_notes_at_their_lines() {
    let (_s, root, mut ed) = project("chatnotes-walk", &["src/a.rs", "src/b.rs", "x/Same.rs", "y/Same.rs"]);
    // (the answer's time as the head says it, from the clock logic reads — either side of a minute's edge)
    let clock = || crate::log::clock(crate::clock::system());
    let before = clock();
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
    // The turn's end opened the answer's place's card, no key pressed (focus A1b): src/a.rs's line 3 — its paragraph
    // by the bar, the item whose names came to nothing after it (focus A1)
    ed.open(&root.join("src/a.rs")).unwrap();
    assert_eq!(ed.cursor_line(), 0, "your cursor where it was");
    let rows = screen(&mut ed);
    let line = rows.iter().find(|r| r.contains("let line_3 = 3;")).unwrap();
    // (its card open under it, named `chat`, headed by the question: the line end the `¶` alone — focus A1b)
    assert!(line.contains('¶') && !line.contains("¶ The count"), "{line}");
    let card = rows.join("\n");
    assert!(card.contains("╭─ chat") && !card.contains("¶ chat"), "{card}");
    let head = |t: &str| card.contains(&format!("› where? · {t}"));
    assert!(head(&before) || head(&clock()), "headed by the question and its time: {card}");
    assert!(card.contains("▎ The count is read twice"), "the paragraph on the card, marked: {card}");
    // (its list item's bullet as the chat draws it — focus A1c, #262)
    assert!(card.contains("The fix goes in") && card.contains("• Same.rs:1 is two files"), "{card}");
    // (closed: the line end its sentence)
    feed(&mut ed, "<esc>");
    let rows = screen(&mut ed);
    let line = rows.iter().find(|r| r.contains("let line_3 = 3;")).unwrap();
    assert!(line.contains("¶ not read this turn · The count is read t…"), "{line}");
    assert!(!rows.join("\n").contains("╭─ chat"), "closed");
    // (`]n` from the top: its line, the card again; on to src/b.rs: its range's first line)
    feed(&mut ed, "gg]n");
    assert_eq!(ed.cursor_line(), 2);
    assert!(screen(&mut ed).join("\n").contains("╭─ chat"), "open again");
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
    // A restart's notes keep the thread, never a bare one (those are the conversation's)
    let saved = crate::notes::to_json(&ed.workspace.claude.notes, &ed.docs);
    let words: Vec<(&str, &str)> =
        saved.iter().map(|n| (n["path"].as_str().unwrap(), n["thread"][0][0].as_str().unwrap())).collect();
    assert_eq!(words, [(root.join("src/b.rs").to_str().unwrap(), "chat")]);
    // An answer whose names come to nothing adds none
    answer(&mut ed, "Nothing in Gone.rs:1 nor Same.rs:2.");
    assert_eq!(chat_notes(&ed, &root), first);
    // The next one that names a place joins them (focus A1b); the thread stays
    answer(&mut ed, "Now src/a.rs:5 holds it.");
    assert_eq!(
        chat_notes(&ed, &root),
        [
            ("src/a.rs".to_string(), 3, "The count is read twice in src/a.rs:3.".to_string()),
            ("src/a.rs".to_string(), 5, "Now src/a.rs:5 holds it.".to_string()),
            ("src/b.rs".to_string(), 2, "The fix goes in src/b.rs:2-4, after the guard.".to_string()),
        ]
    );
    assert_eq!(ed.workspace.claude.notes.list.iter().filter(|n| crate::notes::bare_chat(n)).count(), 2);
    // (the line ends: the newest answer's words, an older answer's line a `¶` alone)
    ed.open(&root.join("src/a.rs")).unwrap();
    feed(&mut ed, "<esc>");
    let rows = code_rows(&mut ed);
    let [l3, l5] =
        ["let line_3 = 3;", "let line_5 = 5;"].map(|t| rows.iter().find(|r| r.contains(t)).unwrap());
    assert!(l3.contains('¶') && !l3.contains("The count"), "{l3}");
    assert!(l5.contains("¶ not read this turn · Now src/a.rs:5 hold…"), "{l5}");
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
    assert_eq!(sent, [("chat", false), ("chat", true), ("chat", false)]);
}

/// A stop of the turn's at `lines` of `path`: an edit's (its changed lines) or a read's.
fn stop(path: &Path, lines: Range<usize>, edit: bool) -> crate::replay::Stop {
    let hunk =
        crate::replay::Hunk { lines: lines.clone(), removed: vec!["a".into()], added: vec!["b".into()] };
    crate::replay::Stop {
        call: "c".into(),
        look: crate::chat::Look {
            verb: if edit { "edit" } else { "read" },
            what: String::new(),
            pattern: false,
            range: String::new(),
            link: None,
        },
        path: Some(path.to_path_buf()),
        lines: Some(lines),
        hunks: if edit { vec![hunk] } else { Vec::new() },
        why: String::new(),
        page: None,
        map: Default::default(),
    }
}

// ── What tarae saw of the places (focus F0) ──────────────────────────────────────────────────

/// A read's stop whose answer showed `lines` of `path`, Claude's claim on the file as it is.
fn read_of(ed: &mut Editor, path: &Path, lines: Range<usize>) -> crate::replay::Stop {
    let stamp = crate::disk::stamp(path).unwrap();
    ed.workspace.claude.chat.as_mut().unwrap().edits.seen.insert(path.to_path_buf(), stamp);
    let mut s = stop(path, lines.clone(), false);
    s.map.shown.push(crate::replay::Shown { path: path.to_path_buf(), lines, old: false });
    s
}

/// A `run_tests` of `path`, its report `verdict`.
fn tests_of(path: &Path, verdict: &str) -> crate::replay::Stop {
    let mut s = stop(path, 0..1, false);
    s.look.verb = "test";
    s.look.range = verdict.to_string();
    s.lines = None;
    s.map.holds = Some(crate::replay::Holds::File);
    s
}

/// What the newest note at `line` (from 1) of `path` says tarae saw.
fn saw(ed: &Editor, path: &Path, line: usize) -> Option<&'static str> {
    let n = newest_of(ed, |n| n.path == path && n.lines.start + 1 == line)?;
    crate::notes::check_word(n, crate::docpaths::find(ed, path)?)
}

/// `screen` at `w` cells wide.
fn screen_at(ed: &mut Editor, w: u16) -> Vec<String> {
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, w, H).unwrap();
    let mut vt = vt100::Parser::new(H, w, 0);
    vt.process(&bytes);
    vt.screen().rows(0, w).collect()
}

/// An answer's three verdicts: the line ends of its doubts say them before the words, faint; the place whose card the
/// turn's end opened says it on the card's head instead, its line end `¶` alone.
#[test]
fn each_place_says_what_tarae_saw_of_it() {
    let (_s, root, mut ed) = project("chatnotes-check", &["src/a.rs"]);
    let a = root.join("src/a.rs");
    ed.open(&a).unwrap();
    let read = read_of(&mut ed, &a, 0..4);
    let text = "The count starts at src/a.rs:2.\n\nThe guard moved to src/a.rs:7.\n\nNothing past src/a.rs:9 reads it.";
    answers(&mut ed, &[text], vec![read, stop(&a, 6..7, true)]);
    assert_eq!(saw(&ed, &a, 2), Some("read this turn"));
    assert_eq!(saw(&ed, &a, 7), Some("changed, no test seen"));
    assert_eq!(saw(&ed, &a, 9), Some("not read this turn"));
    let rows = screen(&mut ed);
    let row = |want: &str| rows.iter().any(|r| r.contains(want));
    assert!(row("¶ changed, no test seen · The guard moved"), "{rows:#?}");
    assert!(row("¶ not read this turn · Nothing past"), "{rows:#?}");
    let open = crate::notes::opened(&ed, ed.doc()).expect("the turn's end opened a card");
    let word = saw(&ed, &a, open.lines.start + 1).unwrap();
    assert!(rows.iter().any(|r| r.contains("where?") && r.contains(word)), "{rows:#?}");
    assert!(!row(&format!("¶ {word}")), "its card says it: {rows:#?}");
}

/// A line end says doubts only: what's as it should be — `read this turn`, `changed, tested` — keeps the line end it had
/// before there were verdicts, the card saying it. Doubted since, it says so there too.
#[test]
fn a_line_end_says_doubts_only() {
    let (_s, root, mut ed) = project("chatnotes-calm", &["src/a.rs"]);
    let a = root.join("src/a.rs");
    ed.open(&a).unwrap();
    let read = read_of(&mut ed, &a, 0..4);
    let stops = vec![read, stop(&a, 6..7, true), tests_of(&a, "12 passed")];
    let text = "The last is src/a.rs:9.\n\nIt starts at src/a.rs:2.\n\nThe guard moved to src/a.rs:7.";
    answers(&mut ed, &[text], stops);
    assert_eq!(saw(&ed, &a, 2), Some("read this turn"));
    assert_eq!(saw(&ed, &a, 7), Some("changed, tested"));
    let rows = code_rows(&mut ed);
    let end_of =
        |want: &str| rows.iter().find(|r| r.contains(want)).unwrap().split('¶').nth(1).unwrap().trim();
    assert_eq!(end_of("line_2 "), "It starts at src/a.rs:2.", "as before: {rows:#?}");
    assert_eq!(end_of("line_7 "), "The guard moved to src/a.rs:7.", "as before: {rows:#?}");
    assert_eq!(end_of("line_9 "), "", "a doubt with its card open: the card says it, `¶` alone: {rows:#?}");
    // Its card says it all the same, and the file moving under it makes it a doubt
    let at = |ed: &Editor, line: usize| newest_of(ed, |n| n.lines.start == line).unwrap().id;
    let id = at(&ed, 6);
    crate::notes::open(&mut ed, id);
    assert!(code_rows(&mut ed).iter().any(|r| r.contains("where?") && r.contains("changed, tested")));
    let id = at(&ed, 8);
    crate::notes::open(&mut ed, id);
    std::fs::write(&a, std::fs::read_to_string(&a).unwrap() + "more\n").unwrap();
    ed.doc_mut().disk = crate::disk::stamp(&a);
    let rows = code_rows(&mut ed);
    assert!(rows.iter().any(|r| r.contains("¶ tested, changed since · The guard")), "{rows:#?}");
}

/// What's cut at a line end is the note's words: its card starts where they do, after the check.
#[test]
fn the_words_cut_card_starts_after_the_check() {
    let (_s, root, mut ed) = project("chatnotes-cut", &["src/a.rs"]);
    let a = root.join("src/a.rs");
    ed.open(&a).unwrap();
    let read = read_of(&mut ed, &a, 0..4);
    let long = "The rest of it lives past src/a.rs:9 where nothing reads it and nothing writes it either, as far as it goes.";
    answers(&mut ed, &[&format!("It starts at src/a.rs:2.\n\n{long}")], vec![read]);
    let rows = screen(&mut ed);
    let y = rows.iter().position(|r| r.contains("line_9") && r.contains("¶ not read")).expect("its line");
    assert!(rows[y].contains('…'), "{}", rows[y]);
    let x = rows[y].chars().position(|c| c == '·').unwrap() + 2;
    let spots = ed.screen.cut_spots.borrow();
    assert!(spots.iter().any(|s| s.0 == y && s.1 == x), "at {y}:{x}: {}", rows[y]);
}

/// A chat note answered is a thread: its head still says what tarae saw of the place.
#[test]
fn a_note_you_answered_keeps_what_tarae_saw() {
    let (_s, root, mut ed) = project("chatnotes-answered", &["src/a.rs"]);
    let a = root.join("src/a.rs");
    ed.open(&a).unwrap();
    let read = read_of(&mut ed, &a, 0..4);
    answers(&mut ed, &["It starts at src/a.rs:2."], vec![read]);
    let id = newest_of(&ed, |n| n.path == a).unwrap().id;
    let entry = Entry { by: By::You, text: "why?".to_string(), draft: false };
    crate::notes::find_mut(&mut ed, id).unwrap().thread.push(entry);
    crate::notes::open(&mut ed, id);
    let rows = screen(&mut ed);
    assert!(rows.iter().any(|r| r.contains("¶ chat · read this turn")), "{rows:#?}");
}

/// Its stops are taken as the turn ends: a next turn begun before its places land changes nothing of what they say.
#[test]
fn the_places_are_judged_by_the_turn_that_named_them() {
    let (_s, root, mut ed) = project("chatnotes-next", &["src/a.rs"]);
    let a = root.join("src/a.rs");
    ed.open(&a).unwrap();
    let read = read_of(&mut ed, &a, 0..4);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::User, "where?", ""));
    c.follow.new_turn();
    c.follow.replay.add(read);
    c.msgs.push(Msg::new(Role::Assistant, "It starts at src/a.rs:2.", ""));
    turn_over(&mut ed, true);
    ed.workspace.claude.chat.as_mut().unwrap().follow.replay.begin();
    crate::term::render(&mut ed, &mut Vec::new(), W, H).unwrap();
    crate::test_results::settle_until(&mut ed, |ed| {
        crate::places::answered(ed) && ed.workspace.claude.notes.answer.is_none()
    });
    assert_eq!(saw(&ed, &a, 2), Some("read this turn"));
}

/// A test that passed says so; one that failed says that; a read and a run stay what they are only while the file is as
/// Claude last saw it — an edit of yours, unsaved or undone, a save, what another program wrote. What changed and wasn't
/// run, and what wasn't read, say the same whatever came after.
#[test]
fn a_file_changed_since_the_read_says_so() {
    let (_s, root, mut ed) = project("chatnotes-since", &["src/a.rs", "src/b.rs"]);
    let (a, b) = (root.join("src/a.rs"), root.join("src/b.rs"));
    ed.open(&b).unwrap();
    ed.open(&a).unwrap();
    ed.workspace.viewer.at = crate::workspace::At::Code;
    let read = read_of(&mut ed, &a, 0..4);
    ed.workspace.claude.chat.as_mut().unwrap().edits.seen.insert(b.clone(), crate::disk::stamp(&b).unwrap());
    let stops = vec![read, stop(&b, 2..3, true), tests_of(&b, "12 passed")];
    answers(
        &mut ed,
        &["It starts at src/a.rs:2.\n\nIt ends at src/b.rs:3.\n\nAnd src/a.rs:9 is left."],
        stops,
    );
    assert_eq!(saw(&ed, &a, 2), Some("read this turn"));
    assert_eq!(saw(&ed, &b, 3), Some("changed, tested"));
    // An edit of yours, unsaved: `read` reads the disk, so the buffer isn't what was read
    feed(&mut ed, "ix<esc>");
    assert!(ed.doc().is_modified());
    assert_eq!(saw(&ed, &a, 2), Some("read, changed since"));
    assert_eq!(saw(&ed, &a, 9), Some("not read this turn"));
    // Undone: as it was
    feed(&mut ed, "u");
    assert_eq!(saw(&ed, &a, 2), Some("read this turn"));
    // Written by something else, the watch's reload having taken it
    std::fs::write(&a, "changed by another program\n").unwrap();
    ed.doc_mut().disk = crate::disk::stamp(&a);
    assert_eq!(saw(&ed, &a, 2), Some("read, changed since"));
    let doc = ed.docs.iter_mut().find(|d| d.path.as_deref() == Some(b.as_path())).unwrap();
    std::fs::write(&b, "changed by another program\n").unwrap();
    doc.disk = crate::disk::stamp(&b);
    assert_eq!(saw(&ed, &b, 3), Some("tested, changed since"));
}

/// Changed during the turn, after the read: said as it ends. Two reads with a save between: the first's lines aren't
/// what's there, the second's are.
#[test]
fn a_file_changed_during_the_turn_says_so_at_its_end() {
    let (_s, root, mut ed) = project("chatnotes-during", &["src/a.rs"]);
    let a = root.join("src/a.rs");
    ed.open(&a).unwrap();
    let read = read_of(&mut ed, &a, 0..4);
    std::fs::write(&a, std::fs::read_to_string(&a).unwrap() + "more\n").unwrap();
    ed.doc_mut().disk = crate::disk::stamp(&a);
    answers(&mut ed, &["It starts at src/a.rs:2."], vec![read]);
    assert_eq!(saw(&ed, &a, 2), Some("read, changed since"));
    let was = read_of(&mut ed, &a, 0..4);
    let mut old = was.clone();
    old.map.shown[0].old = true;
    let again = read_of(&mut ed, &a, 6..8);
    answers(&mut ed, &["First src/a.rs:2 then src/a.rs:7."], vec![old, again]);
    assert_eq!(saw(&ed, &a, 2), Some("read, changed since"));
    assert_eq!(saw(&ed, &a, 7), Some("read this turn"));
}

/// An older answer keeps what its own turn saw, past the stops kept (`replay::TURNS`) — on its card's head, never its line
/// end (`¶` alone); a place with a newer answer on its line says the newer's; a continue has no stops: nothing said.
#[test]
fn an_older_answer_says_what_its_own_turn_saw() {
    let (_s, root, mut ed) = project("chatnotes-older", &["src/a.rs", "src/b.rs"]);
    let (a, b) = (root.join("src/a.rs"), root.join("src/b.rs"));
    ed.open(&a).unwrap();
    let read = read_of(&mut ed, &a, 0..4);
    answers(&mut ed, &["It starts at src/a.rs:2.\n\nThe rest is at src/a.rs:8."], vec![read]);
    for i in 0..crate::replay::TURNS + 1 {
        answer(&mut ed, &format!("Turn {i} is about src/b.rs:{}.", i % 5 + 1));
    }
    let older = newest_of(&ed, |n| n.path == a && n.lines.start == 1).unwrap();
    assert!(crate::notes::older(&ed.workspace.claude.notes, older));
    let id = older.id;
    assert_eq!(saw(&ed, &a, 2), Some("read this turn"), "its stops long gone");
    assert_eq!(saw(&ed, &a, 8), Some("not read this turn"));
    let rows = screen(&mut ed);
    assert!(rows.iter().all(|r| !r.contains("read this turn")), "never at an older line's end: {rows:#?}");
    // Its card: the word on its answer's head, under its question
    crate::notes::open(&mut ed, id);
    let rows = screen(&mut ed);
    assert!(rows.iter().any(|r| r.contains("where?") && r.contains("read this turn")), "{rows:#?}");
    // Continued: its words back, what tarae saw of them gone with the stops
    let places = {
        let c = ed.workspace.claude.chat.as_ref().unwrap();
        places_json(&ed.workspace.claude.notes, &c.msgs)
    };
    restore(&mut ed, places.as_ref());
    assert_eq!(saw(&ed, &a, 2), None);
    ed.open(&b).unwrap();
    assert_eq!(saw(&ed, &b, 1), None);
}

/// A flow at a stop names no place: it claims nothing of its line, so nothing's said of it.
#[test]
fn a_place_the_answer_didnt_name_says_nothing() {
    let (_s, root, mut ed) = project("chatnotes-unnamed", &["src/a.rs"]);
    let a = root.join("src/a.rs");
    ed.open(&a).unwrap();
    let read = read_of(&mut ed, &a, 0..4);
    answers(&mut ed, &["Nothing else reads it."], vec![read]);
    assert!(newest_of(&ed, |n| n.path == a).is_some());
    assert_eq!(saw(&ed, &a, 1), None);
}

/// A search's file, not open as the turn ended, has no stamp to hold a read against: nothing said, never a guess. Open,
/// the buffer's own; a file Claude read, its own stamp.
#[test]
fn a_hit_with_nothing_to_hold_it_against_says_nothing() {
    let (_s, root, mut ed) = project("chatnotes-hit", &["src/a.rs", "src/b.rs"]);
    let (a, b) = (root.join("src/a.rs"), root.join("src/b.rs"));
    ed.open(&a).unwrap();
    let hit = |path: &Path| {
        let mut s = stop(path, 2..3, false);
        s.look.verb = "search";
        s.map.shown.push(crate::replay::Shown { path: path.to_path_buf(), lines: 2..3, old: false });
        s
    };
    answers(&mut ed, &["Found at src/a.rs:3 and src/b.rs:3."], vec![hit(&a), hit(&b)]);
    assert_eq!(saw(&ed, &a, 3), Some("read this turn"));
    assert_eq!(newest_of(&ed, |n| n.path == b).and_then(|n| n.check), None);
    ed.open(&b).unwrap();
    assert_eq!(saw(&ed, &b, 3), None);
    // Claude's stamp of an earlier turn says nothing of what a hit showed in this one
    let seen = (std::time::SystemTime::UNIX_EPOCH, 1);
    ed.workspace.claude.chat.as_mut().unwrap().edits.seen.insert(a.clone(), seen);
    answers(&mut ed, &["Again at src/a.rs:3."], vec![hit(&a)]);
    assert_eq!(saw(&ed, &a, 3), Some("read this turn"));
    // A buffer with unsaved edits holds what no search read: nothing to hold a hit against
    ed.open(&a).unwrap();
    ed.workspace.viewer.at = crate::workspace::At::Code;
    feed(&mut ed, "ix<esc>");
    answers(&mut ed, &["Last at src/a.rs:3."], vec![hit(&a)]);
    assert_eq!(saw(&ed, &a, 3), None);
}

/// A narrow line end keeps its doubt and cuts the sentence in what's left; a few cells of it past the doubt are no
/// sentence, so the doubt stands alone. With no room for it whole the doubt is what's cut, never given up for the
/// sentence — whole under the mouse.
#[test]
fn the_doubt_stays_and_the_sentence_is_cut() {
    let (_s, root, mut ed) = project("chatnotes-narrow", &["src/a.rs"]);
    let a = root.join("src/a.rs");
    ed.open(&a).unwrap();
    let read = read_of(&mut ed, &a, 0..4);
    let long =
        "Nothing past src/a.rs:9 reads it, whatever the rest of the file says of it, as far as it goes.";
    answers(&mut ed, &[&format!("It starts at src/a.rs:2.\n\n{long}")], vec![read]);
    let word = "not read this turn";
    let (mut cut, mut alone, mut short) = (false, false, false);
    for w in 40..=W {
        let rows = screen_at(&mut ed, w);
        let Some(y) = rows.iter().position(|r| r.contains("line_9") && r.contains('¶')) else { continue };
        let row = &rows[y];
        let after = row.split('¶').nth(1).unwrap().split('│').next().unwrap().trim();
        match after.strip_prefix(word) {
            Some("") => alone = true,
            Some(rest) => {
                let words = rest.strip_prefix(" · ").unwrap_or_else(|| panic!("at {w}: {row}"));
                assert!(words.starts_with("Not"), "at {w}: {row}");
                cut |= words.ends_with('…');
            }
            None => {
                short = true;
                let kept = after.strip_suffix('…').unwrap_or_else(|| panic!("at {w}: {row}"));
                assert!(word.starts_with(kept), "at {w}: {row}");
                let x = row.chars().position(|c| c == '¶').unwrap() + 2;
                let spots = ed.screen.cut_spots.borrow();
                assert!(spots.iter().any(|s| s.0 == y && s.1 == x), "its card, at {w}: {row}");
            }
        }
    }
    assert!(cut && alone && short, "all seen: sentence cut {cut}, doubt alone {alone}, doubt cut {short}");
}

/// A place named through a link is the file it leads to: its note is on that file's buffer, its verdict what Claude's
/// answers showed of that file, kept by real path.
#[test]
fn a_place_named_through_a_link_is_its_file() {
    let (_s, root, mut ed) = project("chatnotes-link", &["src/a.rs"]);
    let (a, link) = (root.join("src/a.rs"), root.join("src/link.rs"));
    std::os::unix::fs::symlink(&a, &link).unwrap();
    ed.open(&a).unwrap();
    let read = read_by(&mut ed, &a, &link);
    answers(&mut ed, &["It starts at src/link.rs:2.\n\nAnd src/link.rs:9 is left."], vec![read]);
    assert_eq!(saw(&ed, &a, 2), Some("read this turn"));
    assert_eq!(saw(&ed, &a, 9), Some("not read this turn"));
    let rows = code_rows(&mut ed);
    assert!(rows.iter().any(|r| r.contains("line_9") && r.contains("¶ not read this turn")), "{rows:#?}");
}

/// A read through a link, as the tool lands it: its stop keeps the name asked, what it showed and Claude's stamp the file.
fn read_by(ed: &mut Editor, real: &Path, link: &Path) -> crate::replay::Stop {
    crate::replay::Stop { path: Some(link.to_path_buf()), ..read_of(ed, real, 0..4) }
}

/// A file only ever read through a link holds that read as any file does: not open, the verdict stands on Claude's stamp;
/// open, a change after the read is seen, not taken for what Claude read.
#[test]
fn a_file_read_through_a_link_holds_what_was_read() {
    let (_s, root, mut ed) = project("chatnotes-link-read", &["src/a.rs", "src/b.rs"]);
    let (a, b) = (root.join("src/a.rs"), root.join("src/b.rs"));
    let (la, lb) = (root.join("src/la.rs"), root.join("src/lb.rs"));
    std::os::unix::fs::symlink(&a, &la).unwrap();
    std::os::unix::fs::symlink(&b, &lb).unwrap();
    ed.open(&a).unwrap();
    let reads = vec![read_by(&mut ed, &a, &la), read_by(&mut ed, &b, &lb)];
    std::fs::write(&a, std::fs::read_to_string(&a).unwrap() + "more\n").unwrap();
    ed.doc_mut().disk = crate::disk::stamp(&a);
    answers(&mut ed, &["It starts at src/la.rs:2.\n\nThe other at src/lb.rs:2."], reads);
    assert_eq!(saw(&ed, &a, 2), Some("read, changed since"));
    let not_open = newest_of(&ed, |n| n.path == b).and_then(|n| n.check).map(|j| j.check);
    assert_eq!(not_open, Some(crate::turnmap::Check::Read));
}

/// The newest answer's of the notes `which` takes.
fn newest_of(ed: &Editor, which: impl Fn(&crate::notes::Note) -> bool) -> Option<&crate::notes::Note> {
    let notes = ed.workspace.claude.notes.list.iter().filter(|n| which(n));
    notes.max_by_key(|n| n.answer.as_ref().map_or(0, |o| o.n))
}

/// The chat note at `path`'s line `line` (from 1) whose line end begins `gist` — the newest answer's: its card's text.
fn card_of(ed: &Editor, path: &Path, line: usize, gist: &str) -> String {
    let n = newest_of(ed, |n| n.path == path && n.lines.start + 1 == line && n.gist().starts_with(gist));
    n.unwrap_or_else(|| panic!("no note at {}:{line}", path.display())).thread[0].text.clone()
}

/// The flow of the chat note at `path`'s line `line` (from 1) — the newest answer's: each part's kind, its place if
/// elsewhere, its text.
fn flow_of(ed: &Editor, path: &Path, line: usize) -> Vec<(&'static str, String, String)> {
    let n = newest_of(ed, |n| n.path == path && n.lines.start + 1 == line);
    let n = n.unwrap_or_else(|| panic!("no note at {}:{line}", path.display()));
    (n.flow.iter())
        .map(|p| match p {
            Part::Here(b) => ("here", String::new(), b.text.clone()),
            Part::Whole(b) => ("whole", String::new(), b.text.clone()),
            Part::Elsewhere(at, b) => ("elsewhere", at.clone(), b.text.clone()),
        })
        .collect()
}

/// The screen's rows left of the chat beside the code (its transcript says the answer whole).
fn code_rows(ed: &mut Editor) -> Vec<String> {
    let x = ed.screen.chat_x.unwrap_or(W as usize);
    screen(ed).iter().map(|r| r.chars().take(x).collect()).collect()
}

/// Focus A1, as the user had it after the shots (2026-10-30): the answer's place holds one card of its last words in
/// the order written — the place is the first named in its newest message that names one (an older message's passed
/// over), where follow takes the view; that line's own place note is it, one note, never `2 notes ·`: its line end that
/// line's sentence, its card that paragraph by the bar, each place-less one whole (a list's items line by line), one
/// placed elsewhere a faint line with its place (whole at its own line, whose note stays as it is); what was said before
/// the last call is narration, never in it. `]n` steps it as one note. An answer all of whose blocks are placed has no
/// flow, and replaces it.
#[test]
fn the_answers_flow_is_one_card_at_its_place() {
    let (_s, root, mut ed) = project("chatnotes-rest", &["src/a.rs", "src/b.rs"]);
    answers(
        &mut ed,
        &[
            "Let me look.\n\nFirst I read src/b.rs:4.",
            "call",
            "It cuts by the source text, never the rows. So a resize moves nothing.\n\n\
             The fix is in src/a.rs:6.\n\n\
             The guard in src/b.rs:2 stays as it is.\n\n\
             One caveat: a name two files end with is no place.\n\n\
             - it stays\n- in the chat\n\n```\nsrc/b.rs:9\n```",
        ],
        Vec::new(),
    );
    let (a, s) = (root.join("src/a.rs"), |s: &str| s.to_string());
    assert_eq!(
        chat_notes(&ed, &root),
        [
            (s("src/a.rs"), 6, s("The fix is in src/a.rs:6.")),
            (s("src/b.rs"), 2, s("The guard in src/b.rs:2 stays as it is.")),
            (s("src/b.rs"), 4, s("First I read src/b.rs:4.")),
        ]
    );
    assert_eq!(
        flow_of(&ed, &a, 6),
        [
            ("whole", s(""), s("It cuts by the source text, never the rows. So a resize moves nothing.")),
            ("here", s(""), s("The fix is in src/a.rs:6.")),
            ("elsewhere", s("src/b.rs:2"), s("The guard in src/b.rs:2 stays as it is.")),
            ("whole", s(""), s("One caveat: a name two files end with is no place.")),
            ("whole", s(""), s("it stays")),
            ("whole", s(""), s("in the chat")),
        ]
    );
    // (its first entry the flow as one text: what a reply carries)
    assert_eq!(
        card_of(&ed, &a, 6, "The fix"),
        "It cuts by the source text, never the rows. So a resize moves nothing.\n\nThe fix is in src/a.rs:6.\n\n\
         › src/b.rs:2  The guard in src/b.rs:2 stays as it is.\n\n\
         One caveat: a name two files end with is no place.\n\n- it stays\n- in the chat"
    );
    // On screen: the turn's end opened it (focus A1b) — one note on its line, the line end its sentence; the card the flow
    ed.open(&a).unwrap();
    let rows = code_rows(&mut ed);
    let card = rows.join("\n");
    assert!(!card.contains(" of 2 here"), "one note: {card}");
    for said in
        ["It cuts by the source text", "▎ The fix is in src/a.rs:6.", "› src/b.rs:2  The guard", "One caveat"]
    {
        assert!(card.contains(said), "{said:?} on the card: {card}");
    }
    assert!(!card.contains("Let me look") && !card.contains("First I read"), "the narration: {card}");
    feed(&mut ed, "q");
    let rows = code_rows(&mut ed);
    let line = rows.iter().find(|r| r.contains("let line_6 = 6;")).unwrap();
    assert!(line.contains("¶ not read this turn · The fix is in") && !line.contains("notes"), "{line}");
    // (`]n` to it, then on: the answer's next place as written — the line held one)
    feed(&mut ed, "gg]n");
    assert_eq!(ed.cursor_line(), 5);
    feed(&mut ed, "]n");
    assert_eq!((ed.doc().path.as_deref(), ed.cursor_line()), (Some(root.join("src/b.rs").as_path()), 1));
    // An answer all of whose blocks are placed: its notes alone, no flow (the ones before kept)
    answer(&mut ed, "Only src/a.rs:2 now.");
    let notes = chat_notes(&ed, &root);
    assert!(notes.len() == 4 && notes[0] == (s("src/a.rs"), 2, s("Only src/a.rs:2 now.")), "{notes:?}");
    assert!(flow_of(&ed, &a, 2).is_empty());
}

/// A flow's line for a paragraph placed elsewhere is one row however narrow the card: cut with `…`.
#[test]
fn a_paragraph_placed_elsewhere_is_one_row_on_a_narrow_card() {
    let (_s, root, mut ed) = project("chatnotes-narrow", &["src/a.rs", "src/b.rs"]);
    let long =
        "The guard in src/b.rs:2 stays as it is, since every caller checks the length before it gets there.";
    answer(&mut ed, &format!("The fix is in src/a.rs:6.\n\n{long}\n\nOne caveat: nothing else."));
    // (its card the turn's end opened)
    ed.open(&root.join("src/a.rs")).unwrap();
    let rows = code_rows(&mut ed);
    let stub: Vec<&String> = rows.iter().filter(|r| r.contains("› src/b.rs:2")).collect();
    assert!(stub.len() == 1 && stub[0].contains('…'), "{}", rows.join("\n"));
    assert!(!rows.iter().any(|r| r.contains("before it gets there")), "{}", rows.join("\n"));
}

/// The flow's lines as a card lays them: this line's paragraph keeps its bar on every row it wraps to (the chat's
/// renderer lays it, the bar before each row — focus A1c); one placed elsewhere is one row, cut with `…`, its whole
/// carried.
#[test]
fn the_flows_lines_wrap_with_their_bar_and_cut_to_one_row() {
    use crate::markdown::{Line, Span};
    let sp = |t: &str| Span { cut: None, hint: false, text: t.into(), style: Default::default() };
    let stub =
        Line::Cut(vec![sp("› "), sp("src/b.rs:2"), sp("  "), sp("The guard stays as it is, every caller")]);
    let rows = crate::markdown::wrap(&[stub], 16, Default::default());
    let text: Vec<String> = rows.iter().map(|r| r.iter().map(|s| s.text.as_str()).collect()).collect();
    assert_eq!(text, ["› src/b.rs:2  T…"]);
    assert!(rows[0].last().is_some_and(|s| s.cut.is_some()), "the stub's whole carried: {:?}", rows[0]);
    let (_s, root, mut ed) = project("chatnotes-bar", &["src/a.rs"]);
    let long = "each thread's name, how many knots it has, its length and the order it came in ".repeat(3);
    answer(&mut ed, &format!("The loop at src/a.rs:3 prints {long}.\n\nThat is all."));
    ed.open(&root.join("src/a.rs")).unwrap();
    let card = card_rows(&mut ed);
    let barred: Vec<&String> = card.iter().filter(|r| r.contains("▎ ")).collect();
    assert!(barred.len() >= 3, "wrapped: {card:#?}");
    let first = card.iter().position(|r| r.contains("▎ The loop")).expect("its first row");
    assert!(card[first..first + barred.len()].iter().all(|r| r.contains("│ ▎ ")), "a bar on each: {card:#?}");
}

/// A flow longer than its card has room for keeps its paragraph about this line in sight: what's before it folded into a
/// `…` row (whole on its card), what's past the room its last row's, saying how to read on in the chat (focus A1c) —
/// never the newest part a thread keeps.
#[test]
fn a_long_flow_keeps_its_lines_paragraph_in_sight() {
    let (_s, root, mut ed) = project("chatnotes-long", &["src/a.rs"]);
    let para = |k: usize| {
        format!("Paragraph {k} says a good deal about the strands and how they are kept in order.")
    };
    let before: Vec<String> = (1..=8).map(para).collect();
    let after: Vec<String> = (9..=20).map(para).collect();
    let text = format!("{}\n\nThe fix is in src/a.rs:6.\n\n{}", before.join("\n\n"), after.join("\n\n"));
    answer(&mut ed, &text);
    // (its card the turn's end opened)
    ed.open(&root.join("src/a.rs")).unwrap();
    let card = code_rows(&mut ed).join("\n");
    assert!(card.contains("▎ The fix is in src/a.rs:6."), "{card}");
    assert!(!card.contains("Paragraph 1 ") && !card.contains("Paragraph 20"), "{card}");
    assert!(card.lines().filter(|r| r.trim_matches(|c| c == '│' || c == ' ') == "…").count() == 1, "{card}");
    let more = |r: &&str| r.trim_matches(|c| c == '│' || c == ' ') == "… enter reads on in the chat";
    assert!(card.lines().filter(more).count() == 1, "{card}");
    // (the flow itself past `PARAGRAPH_MAX`: its last block cut with `…`, then the line that says where the rest is)
    let flow = flow_of(&ed, &root.join("src/a.rs"), 6);
    let n = flow.len();
    assert!(!flow.iter().any(|p| p.2.starts_with("Paragraph 20")), "{flow:?}");
    assert!(flow[n - 2].2.starts_with("Paragraph 19") && flow[n - 2].2.ends_with('…'), "{flow:?}");
    assert_eq!(flow[n - 1], ("whole", String::new(), "The rest is in the chat.".to_string()));
    // (what follows it short of the room: the window back over what's before it, no row left empty at its foot)
    let text = format!("{}\n\nThe fix is in src/a.rs:6.\n\n{}", before.join("\n\n"), para(9));
    answer(&mut ed, &text);
    // (the same line's card: it stays open, this answer on top — the first folded under it, `… 1 earlier`)
    let card = code_rows(&mut ed).join("\n");
    assert!(card.contains("▎ The fix is in src/a.rs:6.") && card.contains("Paragraph 9 "), "{card}");
    // (its foot the earlier answer's row, a blank row above it)
    let foot = card.lines().position(|r| r.contains("… 1 earlier")).unwrap();
    let last = card.lines().nth(foot - 2).unwrap();
    assert!(last.trim_matches(|c| c == '│' || c == ' ').ends_with("are kept in order."), "{card}");
}

/// The reviewer's (note 20): a place named again in a later paragraph — the same message or another — makes that
/// paragraph about it too: the answer's place's own `here`, another place's line elsewhere; its note is still the first
/// naming's. An answer all of whose paragraphs name places that came to files has no flow, however often it names them.
#[test]
fn a_place_named_again_is_that_paragraphs_place_too() {
    let (_s, root, mut ed) = project("chatnotes-again", &["src/a.rs", "src/b.rs"]);
    let (a, s) = (root.join("src/a.rs"), |s: &str| s.to_string());
    answers(
        &mut ed,
        &[
            "The fix is in src/a.rs:6.\n\nThe guard in src/b.rs:2 stays.\n\nNothing else changes.",
            "call",
            "Again src/a.rs:6 holds it.\n\nAnd src/b.rs:2 still guards it.\n\nThat is all.",
        ],
        Vec::new(),
    );
    assert_eq!(
        flow_of(&ed, &a, 6),
        [
            // (that line's paragraph wherever it stood — before the call too: the line end says its sentence)
            ("here", s(""), s("The fix is in src/a.rs:6.")),
            ("here", s(""), s("Again src/a.rs:6 holds it.")),
            ("elsewhere", s("src/b.rs:2"), s("And src/b.rs:2 still guards it.")),
            ("whole", s(""), s("That is all.")),
        ]
    );
    // (the notes the places' first namings: one each)
    assert_eq!(
        chat_notes(&ed, &root),
        [
            (s("src/a.rs"), 6, s("The fix is in src/a.rs:6.")),
            (s("src/b.rs"), 2, s("The guard in src/b.rs:2 stays."))
        ]
    );
    // (the review's note 22: a place named many times takes one of the cap's places — one named after it still placed)
    let again = "a.rs:6 ".repeat(170);
    answer(&mut ed, &format!("{again}\n\nThe guard in src/b.rs:2 stays.\n\nThat is all."));
    let kinds: Vec<&str> = flow_of(&ed, &a, 6).iter().map(|p| p.0).collect();
    assert_eq!(kinds, ["here", "elsewhere", "whole"]);
    assert!(
        chat_notes(&ed, &root).iter().any(|n| n.0 == "src/b.rs" && n.1 == 2),
        "{:?}",
        chat_notes(&ed, &root)
    );
    // Every paragraph naming a place, one named twice: no flow
    answer(
        &mut ed,
        "The fix is in src/a.rs:6.\n\nThe guard in src/b.rs:2 stays.\n\nSo src/a.rs:6 is the one place.",
    );
    assert!(flow_of(&ed, &a, 6).is_empty(), "{:?}", flow_of(&ed, &a, 6));
}

/// A turn that wrote at a note — pinned one or replied on one — gets no flow at its place: its answer is at the code
/// already (focus-line's "I answered at the note." at the last read was noise); its places still are notes.
#[test]
fn a_turn_that_wrote_at_a_note_gets_no_flow() {
    let (_s, root, mut ed) = project("chatnotes-noted", &["src/a.rs", "src/b.rs"]);
    let a = root.join("src/a.rs");
    let pin = serde_json::json!({"path": "src/b.rs", "line": 9, "text": "pinned"});
    crate::notes::tool_call(&mut ed, &pin).unwrap();
    answers(&mut ed, &["I answered at the note. That is all."], vec![stop(&a, 1..3, false)]);
    assert!(chat_notes(&ed, &root).is_empty(), "{:?}", chat_notes(&ed, &root));
    // A reply on it: the same; an answer's place still a note, without its flow
    let id = ed.workspace.claude.notes.list[0].id;
    crate::notes::tool_call(&mut ed, &serde_json::json!({"reply_to": id, "text": "there"})).unwrap();
    answers(&mut ed, &["It is src/a.rs:4.\n\nThat is all."], vec![stop(&a, 1..3, false)]);
    assert_eq!(chat_notes(&ed, &root), [("src/a.rs".to_string(), 4, "It is src/a.rs:4.".to_string())]);
    assert!(flow_of(&ed, &a, 4).is_empty());
    // (the next turn writes at none: its flow again)
    answers(&mut ed, &["Nothing else reads it."], vec![stop(&a, 1..3, false)]);
    assert_eq!(flow_of(&ed, &a, 2), [("whole", String::new(), "Nothing else reads it.".to_string())]);
}

/// Focus A1: an answer that names no place has its note at the turn's last edit, else its last read in the project —
/// kept beside the answers' before it (focus A1b); a turn that stopped nowhere in the project adds none.
#[test]
fn an_answer_with_no_place_is_at_the_turns_last_stop() {
    let (_s, root, mut ed) = project("chatnotes-stops", &["src/a.rs", "src/b.rs"]);
    let (a, b) = (root.join("src/a.rs"), root.join("src/b.rs"));
    answer(&mut ed, "It is read in src/a.rs:3.");
    let last = [("src/a.rs".to_string(), 3, "It is read in src/a.rs:3.".to_string())];
    assert_eq!(chat_notes(&ed, &root), last);
    // (stopped nowhere, or only outside the project: none added)
    answer(&mut ed, "Nothing else to add.");
    assert_eq!(chat_notes(&ed, &root), last);
    let outside = crate::scratch::Scratch::new("chatnotes-stops-outside");
    answers(&mut ed, &["Read it."], vec![stop(&outside.join("x.rs"), 0..2, false)]);
    assert_eq!(chat_notes(&ed, &root), last);
    // The last edit, an earlier one and reads after it passed over
    let stops = vec![stop(&b, 6..8, true), stop(&a, 4..5, true), stop(&b, 0..2, false)];
    answers(&mut ed, &["Done. Nothing else relied on it."], stops);
    assert_eq!(newest_notes(&ed, &root), [("src/a.rs".to_string(), 5, "Done.".to_string())]);
    assert_eq!(chat_notes(&ed, &root).len(), 2, "the first answer's kept");
    assert_eq!(card_of(&ed, &a, 5, "Done."), "Done. Nothing else relied on it.");
    // No edit: the last read in the project (one outside after it passed over)
    let stops = vec![stop(&a, 1..3, false), stop(&b, 7..9, false), stop(&outside.join("x.rs"), 0..2, false)];
    answers(&mut ed, &["Read both."], stops);
    assert_eq!(newest_notes(&ed, &root), [("src/b.rs".to_string(), 8, "Read both.".to_string())]);
    // A place the answer names wins over the stops: the flow on its note there
    answers(&mut ed, &["Done.\n\nThe guard in src/b.rs:2 stays."], vec![stop(&a, 4..5, true)]);
    let s = |s: &str| s.to_string();
    assert_eq!(newest_notes(&ed, &root), [(s("src/b.rs"), 2, s("The guard in src/b.rs:2 stays."))]);
    assert_eq!(
        flow_of(&ed, &b, 2),
        [("whole", s(""), s("Done.")), ("here", s(""), s("The guard in src/b.rs:2 stays."))]
    );
}

/// The stand-in claude: `/compact` compacts (no answer); a message asking to fail says half a sentence naming
/// `src/b.rs:2`, then the turn fails; any other answers naming `src/a.rs:3`.
const STAND_IN: &str = r#"while IFS= read -r line; do
  case "$line" in
    *'"content":"/compact"'*)
      printf '{"type":"result","subtype":"success","is_error":false,"result":""}\n';;
    *'what reads it'*)
      printf '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Let me read it."}}}\n'
      printf '{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"mcp__tarae__read","input":{"files":["src/b.rs:2-4"]}}]}}\n'
      printf '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"read"}]}]}}\n'
      printf '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Nothing else reads it."}}}\n'
      printf '{"type":"result","subtype":"success","is_error":false,"result":"done"}\n';;
    *'walk me'*)
      printf '{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"mcp__tarae__read","input":{"files":["src/s.rs"]}}]}}\n'
      printf '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"read"}]}]}}\n'
      printf '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"<show at=\\"src/s.rs:21-70\\">The loop winds every strand.</show> Then it sorts."}}}\n'
      printf '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"\\n\\nIt pops.\\n\\nIt ties.\\n\\nIt measures.\\n\\nIt keeps.\\n\\nIt sorts.\\n\\nIt ends.\\n\\nThat is all."}}}\n'
      printf '{"type":"result","subtype":"success","is_error":false,"result":"done"}\n';;
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
    // (focus A1: a whole answer naming no place, its turn's read in the project — its note there, the narration before
    // the read left out)
    ask(&mut ed, "what reads it?");
    assert_eq!(newest_notes(&ed, &root), [("src/b.rs".to_string(), 2, "Nothing else reads it.".to_string())]);
    assert_eq!(chat_notes(&ed, &root).len(), 2, "the whole answer's before it kept");
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
    assert_eq!(newest_notes(&ed, &root), [("src/b.rs".to_string(), 5, "Still src/b.rs:5.".to_string())]);
}

// ── Focus A1c: the card says this line, the chat the whole answer ──────────────────────────────────────

/// A project with `src/s.rs`: a doc comment of two paragraphs (lines 1, 3–4) over a function, then lines to the 80th
/// (`let line_6 = 6;` …).
fn spool(name: &str) -> (crate::scratch::Scratch, PathBuf, Editor) {
    let (s, root, ed) = project(name, &["src/a.rs", "src/b.rs"]);
    let mut text = String::from(
        "/// The spool keeps its order: a strand is wound in the order it came, so the wound list reads as the queue.\n\
         ///\n\
         /// Each strand is tied before it is measured, and one whose knot slips goes back to be tied again later.\n\
         /// Short.\nfn wind() {}\n",
    );
    text.extend((6..=80).map(|i| format!("let line_{i} = {i};\n")));
    std::fs::write(root.join("src/s.rs"), text).unwrap();
    (s, root, ed)
}

/// The open card's rows as the code shows them, its top line first: from the row naming it (`╭─ `) to its foot.
fn card_rows(ed: &mut Editor) -> Vec<String> {
    let rows = code_rows(ed);
    let top = rows.iter().position(|r| r.contains("╭─ ")).unwrap_or_else(|| panic!("no card: {rows:#?}"));
    let foot = top + rows[top..].iter().position(|r| r.contains("╰─")).expect("its foot");
    rows[top..=foot].to_vec()
}

/// #259: the card's head is its question in one row, cut with `…` before its time (kept), the whole question on the
/// cut's card; rendered as the chat renders it, its marks gone.
#[test]
fn a1c_the_question_is_one_row_whole_on_its_cut_card() {
    let (_s, root, mut ed) = project("a1c-question", &["src/a.rs"]);
    let asked = format!(
        "why is the count read twice {}and where does it go?",
        "when the strands are wound ".repeat(14)
    );
    answers_to(&mut ed, &asked, &["It is read in src/a.rs:3."], Vec::new());
    ed.open(&root.join("src/a.rs")).unwrap();
    let card = card_rows(&mut ed);
    let head = card.iter().position(|r| r.contains("› why is the count")).expect("its head");
    assert!(card[head].contains("… · "), "cut before its time: {card:#?}");
    assert!(card[head + 1].contains("It is read in src/a.rs:3."), "its paragraph next: {card:#?}");
    let cut = ed
        .screen
        .cut_spots
        .borrow()
        .iter()
        .any(|s| matches!(&s.3, crate::whole::Cut::Text(w) if w.text == asked));
    assert!(cut, "the whole question on its cut card");
    // (a short one with marks: as the chat has it)
    answers_to(&mut ed, "why is `count` **read** twice?", &["It is read in src/a.rs:5."], Vec::new());
    let card = card_rows(&mut ed).join("\n");
    assert!(card.contains("› why is count read twice? · ") && !card.contains('`'), "{card}");
}

/// #259: a paragraph that ends with `:` keeps the fenced code that follows it — what it says of its line — drawn in its
/// colours on the card; code after any other paragraph is the chat's.
#[test]
fn a1c_a_paragraph_ending_with_a_colon_keeps_its_code() {
    assert_eq!(
        blocks("Do this:\n\n```rust\nlet x = 1;\n```\n\nNot this.\n\n```\nlet y = 2;\n```"),
        [
            Block { text: "Do this:".into(), item: false, code: Some("```rust\nlet x = 1;\n```".into()) },
            Block { text: "Not this.".into(), item: false, code: None },
        ]
    );
    let (_s, root, mut ed) = project("a1c-colon", &["src/a.rs"]);
    answer(
        &mut ed,
        "The fix at src/a.rs:3 is this:\n\n```rust\nlet fixed = 1;\n```\n\nThe guard at src/a.rs:5 stays.\n\n```rust\nlet other = 2;\n```",
    );
    let a = root.join("src/a.rs");
    assert!(
        card_of(&ed, &a, 3, "The fix").ends_with("this:\n\n```rust\nlet fixed = 1;\n```"),
        "{}",
        card_of(&ed, &a, 3, "The fix")
    );
    assert_eq!(card_of(&ed, &a, 5, "The guard"), "The guard at src/a.rs:5 stays.");
    ed.open(&a).unwrap();
    let card = card_rows(&mut ed).join("\n");
    assert!(card.contains("let fixed = 1;") && !card.contains("```") && !card.contains("other"), "{card}");
    // (an answer's flow: its paragraph about the line with its code, the place-less one whole)
    answer(&mut ed, "The order goes at src/a.rs:7, here:\n\n```rust\nlet order = 3;\n```\n\nThat is all.");
    let card = card_rows(&mut ed).join("\n");
    assert!(card.contains("▎ The order goes") && card.contains("let order = 3;"), "{card}");
    // (a walk-through's stop, the words after it ending `:`)
    answer(&mut ed, "<show at=\"src/a.rs:9\">The tail.</show> It ends here:\n\n```rust\nlet tail = 9;\n```");
    assert!(
        card_of(&ed, &a, 9, "The tail").ends_with("let tail = 9;\n```"),
        "{}",
        card_of(&ed, &a, 9, "The tail")
    );
}

/// #262: the card's words render as the chat's — `**bold**` and `code` without their marks, a list's items with bullets.
#[test]
fn a1c_the_cards_words_read_as_the_chats() {
    let (_s, root, mut ed) = project("a1c-markdown", &["src/a.rs"]);
    answer(
        &mut ed,
        "The count at src/a.rs:3 is **read twice**, by `tally`.\n\n- the loop\n- the guard\n\nThat is all.",
    );
    ed.open(&root.join("src/a.rs")).unwrap();
    let card = card_rows(&mut ed).join("\n");
    assert!(
        card.contains("is read twice, by tally.") && !card.contains("**") && !card.contains('`'),
        "{card}"
    );
    assert!(card.contains("• the loop") && card.contains("• the guard"), "{card}");
}

/// A paragraph long past the card's room: `n` sentences, at src/a.rs:3.
fn long_answer(n: usize) -> String {
    let words: Vec<String> = (1..=n).map(|i| format!("Sentence {i} says more of the strands.")).collect();
    format!("It is read in src/a.rs:3. {}", words.join(" "))
}

/// The chat's rows as drawn, right of the code.
fn chat_rows(ed: &mut Editor) -> String {
    let rows = screen(ed);
    let x = ed.screen.chat_x.expect("the chat drawn");
    rows.iter().map(|r| r.chars().skip(x).collect::<String>()).collect::<Vec<_>>().join("\n")
}

/// #259: an answer's card never grows — past its room its last row says how to read on; `enter` on its line (and a press
/// on that row) takes the chat to its paragraph, the keys where they were, the card open; an older answer's stub there
/// unfolded for it.
#[test]
fn a1c_a_card_past_its_room_reads_on_in_the_chat() {
    let (_s, root, mut ed) = project("a1c-more", &["src/a.rs", "src/b.rs"]);
    // (short of `PARAGRAPH_MAX`: a paragraph alone, no flow — the closed chat's test has one)
    answer(&mut ed, &long_answer(30));
    assert!(newest_of(&ed, |n| n.lines.start == 2).is_some_and(|n| n.flow.is_empty()));
    answer(&mut ed, "Then src/b.rs:2 checks it.");
    ed.open(&root.join("src/a.rs")).unwrap();
    let first = newest_of(&ed, |n| n.lines.start == 2).map(|n| n.id).unwrap();
    crate::notes::open(&mut ed, first);
    assert!(chat_rows(&mut ed).contains("◂ a.rs:3"), "the older answer a stub in the chat");
    let card = card_rows(&mut ed);
    assert!(card.len() <= 18 + 2, "never grown: {card:#?}");
    assert!(card[card.len() - 2].contains("… enter reads on in the chat"), "{card:#?}");
    let (at, open) = (ed.workspace.viewer.at, ed.workspace.claude.notes.open);
    feed(&mut ed, "3G<ret>");
    let chat = chat_rows(&mut ed);
    let chat = if chat.contains("Sentence 1 ") { chat } else { chat_rows(&mut ed) };
    assert!(
        chat.contains("It is read in") && chat.contains("Sentence 1 "),
        "the chat at its paragraph: {chat}"
    );
    assert!(!chat.contains("◂ a.rs:3"), "its stub unfolded: {chat}");
    assert_eq!(
        (ed.workspace.viewer.at, ed.workspace.claude.notes.open),
        (at, open),
        "the keys, the card as they were"
    );
    // (a press on its row does the same: from the bottom again)
    ed.workspace.claude.chat.as_mut().unwrap().scroll = 0;
    ed.workspace.claude.chat.as_mut().unwrap().reading = None;
    assert!(!chat_rows(&mut ed).contains("Sentence 1 "), "at the bottom");
    let (y, x) = {
        let rows = ed.screen.note_text.borrow();
        let r = rows.iter().find(|r| r.2.iter().any(|s| s.text.contains("reads on"))).expect("its row");
        (r.0, r.1 + 4)
    };
    for kind in [crate::event::MouseKind::Down, crate::event::MouseKind::Up] {
        ed.handle_mouse(crate::event::Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
    }
    assert!(ed.prompt.is_none(), "never a reply");
    chat_rows(&mut ed);
    assert!(chat_rows(&mut ed).contains("Sentence 1 "), "pressed: the chat at its paragraph");
    // (a card that fits says no such row, and `enter` closes it)
    feed(&mut ed, "<esc>");
    ed.open(&root.join("src/b.rs")).unwrap();
    feed(&mut ed, "2G<ret>");
    assert!(!card_rows(&mut ed).join("\n").contains("reads on"));
    feed(&mut ed, "<ret>");
    assert_eq!(ed.workspace.claude.notes.open, None);
}

/// #259, the review's: reading on from the chat's view mode moves the view mode's own rows (not the chat's scroll), from
/// the minimized chat shows it first, and with no conversation kept says the card is all there is.
#[test]
fn a1c_reading_on_from_the_view_mode_the_minimized_chat_and_none_kept() {
    let at_paragraph = |ed: &mut Editor| {
        chat_rows(ed);
        chat_rows(ed).contains("Sentence 1 ")
    };
    let setup = |name: &str| {
        let (s, root, mut ed) = project(name, &["src/a.rs", "src/b.rs"]);
        answer(&mut ed, &long_answer(30));
        answer(&mut ed, "Then src/b.rs:2 checks it.");
        ed.open(&root.join("src/a.rs")).unwrap();
        let first = newest_of(&ed, |n| n.lines.start == 2).map(|n| n.id).unwrap();
        crate::notes::open(&mut ed, first);
        (s, ed)
    };
    // The view mode: its document's rows taken there, the chat's scroll left as it was
    let (_s, mut ed) = setup("a1c-read-view");
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    feed(&mut ed, "<esc>");
    chat_rows(&mut ed);
    assert!(
        ed.workspace.claude.chat.as_ref().unwrap().transcript.as_ref().is_some_and(|t| t.keyed),
        "view mode"
    );
    assert!(!at_paragraph(&mut ed), "at the bottom");
    let scroll = ed.workspace.claude.chat.as_ref().unwrap().scroll;
    crate::notes::read_on(&mut ed);
    assert!(at_paragraph(&mut ed), "the view mode at its paragraph:\n{}", chat_rows(&mut ed));
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert_eq!((c.scroll, c.reading.as_ref().map(|r| r.1)), (scroll, Some(true)), "done, on its own rows");
    assert_eq!(ed.workspace.viewer.at, crate::workspace::At::Chat, "the keys where they were");
    // The chat minimized while Claude works: shown, then at its paragraph
    let (_s, mut ed) = setup("a1c-read-min");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    (c.state, c.follow.on, c.unfolded) = (crate::chat::State::Thinking, true, false);
    code_rows(&mut ed);
    assert!(ed.screen.chat_view.get().is_none(), "minimized");
    crate::notes::read_on(&mut ed);
    assert!(at_paragraph(&mut ed), "shown, at its paragraph:\n{}", chat_rows(&mut ed));
    // No conversation kept (the chat closed, nothing saved in tests): said, nothing opened
    let (_s, mut ed) = setup("a1c-read-none");
    crate::chat::close(&mut ed);
    crate::notes::read_on(&mut ed);
    assert!(ed.workspace.claude.chat.is_none(), "nothing opened");
    assert!(
        ed.toasts.iter().any(|t| t.text.contains("conversation isn't kept")),
        "said: {:?}",
        ed.toasts.iter().map(|t| &t.text).collect::<Vec<_>>()
    );
}

/// #262, the review's: a note of yours reads as the chat draws your message — line by line, a `#` or a `|` as typed — and a
/// Claude's (the reviewer's here) as the chat renders a Claude's: markdown, its single line breaks joined.
#[test]
fn a1c_your_note_keeps_its_lines_a_claudes_is_markdown() {
    let (_s, root, mut ed) = project("a1c-yours", &["src/a.rs"]);
    let a = root.join("src/a.rs");
    ed.open(&a).unwrap();
    let id = crate::notes::add(&mut ed, &a, 2..3, By::You, "first line\n# not a heading\n| a | b |");
    crate::notes::open(&mut ed, id);
    let card = card_rows(&mut ed).join("\n");
    for l in ["first line", "# not a heading", "| a | b |"] {
        assert!(card.lines().any(|r| r.contains(l)), "{l:?} a row as typed:\n{card}");
    }
    let id = crate::notes::add(&mut ed, &a, 5..6, By::Reviewer, "This value\nis never checked.");
    crate::notes::open(&mut ed, id);
    let card = card_rows(&mut ed).join("\n");
    assert!(card.contains("This value is never checked."), "joined, as the chat renders it:\n{card}");
}

/// #259: the chat closed, reading on opens it again on the conversation the answer is of — its answers' notes with it,
/// the card open as it was.
#[test]
fn a1c_reading_on_opens_the_closed_chat_on_its_conversation() {
    let (s, root, mut ed) = project("a1c-closed", &["src/a.rs"]);
    let dir = s.join("chats");
    ed.chats_dir = Some(dir.clone());
    ed.workspace.claude.chat.as_mut().unwrap().conversation = Some("c1".into());
    // (claude a stand-in: what it's sent, never a process)
    let (tx, _rx) = std::sync::mpsc::channel();
    ed.claude_lines = Some(tx);
    answer(&mut ed, &long_answer(40));
    let kept = |dir: &Path, root: &Path| {
        crate::chats::load(dir, root).into_iter().any(|c| c.id == "c1" && c.places.is_some())
    };
    crate::test_results::settle_until(&mut ed, |_| kept(&dir, &root));
    crate::chat::close(&mut ed);
    ed.open(&root.join("src/a.rs")).unwrap();
    // (the turn's card still open, the chat closed: its answer's notes stay)
    feed(&mut ed, "3G");
    assert!(ed.workspace.claude.notes.open.is_some());
    assert!(card_rows(&mut ed).join("\n").contains("reads on"), "its card, cut");
    feed(&mut ed, "<ret>");
    let c = ed.workspace.claude.chat.as_ref().expect("the chat opened again");
    assert_eq!(c.conversation.as_deref(), Some("c1"), "on its conversation");
    assert!(ed.workspace.claude.notes.open.is_some(), "its card open again");
    chat_rows(&mut ed);
    assert!(chat_rows(&mut ed).contains("It is read in"), "the chat at its paragraph");
    assert_eq!(ed.workspace.viewer.at, crate::workspace::At::Code, "the keys where they were");
}

/// #260: which of its answer's places a card is, on its top line (`chat · 2/3`), as `]n` walks them; an answer of one
/// place says none, and keeps its line's count beside another kind of note there.
#[test]
fn a1c_a_card_says_which_of_its_answers_places_it_is() {
    let (_s, root, mut ed) = project("a1c-position", &["src/a.rs", "src/b.rs"]);
    answer(
        &mut ed,
        "It is read in src/a.rs:3.\n\nThe guard at src/b.rs:2 checks it.\n\nAnd src/a.rs:5 sets it.",
    );
    ed.open(&root.join("src/a.rs")).unwrap();
    let top = |ed: &mut Editor| card_rows(ed)[0].clone();
    assert!(top(&mut ed).contains("╭─ chat · 1/3 ─"), "{}", top(&mut ed));
    feed(&mut ed, "]n");
    assert!(top(&mut ed).contains("chat · 2/3"), "{}", top(&mut ed));
    feed(&mut ed, "]n");
    assert!(top(&mut ed).contains("chat · 3/3"), "{}", top(&mut ed));
    // (one place, a thread of yours on its line: none, the line's count kept)
    answer(&mut ed, "Only src/a.rs:7.");
    let a = root.join("src/a.rs");
    crate::notes::add(&mut ed, &a, 6..7, By::You, "mine too");
    ed.open(&a).unwrap();
    let id = newest_of(&ed, |n| n.lines.start == 6 && crate::notes::bare_chat(n)).unwrap().id;
    crate::notes::open(&mut ed, id);
    let t = top(&mut ed);
    assert!(t.contains("╭─ chat ─") && !t.contains('/') && t.contains(" of 2 here"), "{t}");
}

/// #263: a range past a screen (a walk-through's stop over most of a file) opens its card under its first line, `]n` with
/// it in sight; one that fits under its last.
#[test]
fn a1c_a_range_past_a_screen_opens_under_its_first_line() {
    let (_s, root, mut ed) = spool("a1c-range");
    answer(
        &mut ed,
        "<show at=\"src/s.rs:6-70\">The loop winds every strand.</show> It sorts them at its end.",
    );
    let s = root.join("src/s.rs");
    ed.open(&s).unwrap();
    let above = |ed: &mut Editor| {
        let rows = code_rows(ed);
        let top = rows.iter().position(|r| r.contains("╭─ ")).expect("its card in sight");
        rows[top - 1].clone()
    };
    feed(&mut ed, "gg]n");
    assert!(above(&mut ed).contains("let line_6 = 6;"), "under its first line");
    feed(&mut ed, "<ret>G");
    assert_eq!(ed.workspace.claude.notes.open, None, "closed");
    feed(&mut ed, "]n");
    assert!(above(&mut ed).contains("let line_6 = 6;"), "`]n` from the foot: in sight");
    answer(&mut ed, "<show at=\"src/s.rs:20-23\">These four tie it.</show> Then it goes.");
    ed.open(&s).unwrap();
    feed(&mut ed, "gg]n");
    assert!(above(&mut ed).contains("let line_23 = 23;"), "one that fits: under its last line");
}

/// `answer`, every frame at `w`×`h` — follow paused during the turn when `paused`.
fn answer_at(ed: &mut Editor, text: &str, (w, h): (u16, u16), paused: bool) {
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::User, "where?", ""));
    c.follow.new_turn();
    c.follow.paused = paused;
    c.msgs.push(Msg::new(Role::Assistant, text, ""));
    turn_over(ed, true);
    crate::term::render(ed, &mut Vec::new(), w, h).unwrap();
    crate::test_results::settle_until(ed, |ed| {
        crate::places::answered(ed) && ed.workspace.claude.notes.answer.is_none()
    });
}

/// The screen's rows at `w`×`h`, left of the chat.
fn code_rows_at(ed: &mut Editor, (w, h): (u16, u16)) -> Vec<String> {
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, w, h).unwrap();
    let mut vt = vt100::Parser::new(h, w, 0);
    vt.process(&bytes);
    let x = ed.screen.chat_x.unwrap_or(w as usize);
    vt.screen().rows(0, w).map(|r| r.chars().take(x).collect()).collect()
}

/// The user's 80-column shot (`a1c-end`): where follow leads, the card the turn's end opens is in sight with its line and
/// stays there as the keys come back to the code through the chat's view mode — whose scroll once took the view to the
/// first file the chat showed (the turn's read, line 1), though the chat showed the card's place too (`follow::reading`).
/// Never while you type, nor where follow is paused at your cursor (the view yours; off too —
/// `the_turns_card_is_in_sight_where_follow_leads`).
#[test]
fn a1c_the_turns_card_is_in_sight_at_80_columns() {
    const S: (u16, u16) = (80, 24);
    let in_sight = |rows: &[String]| {
        let top = rows.iter().position(|r| r.contains("╭─ chat"));
        top.is_some_and(|t| t > 0 && rows[t - 1].contains("let line_21 = 21;"))
    };
    {
        // (the shot's turn: the file read whole, then a walk-through's stop over most of it — streamed, so it plays)
        let (_s, root, mut ed) = spool("a1c-sight80");
        ed.workspace.claude.chat = None;
        ed.config.llm.command = "sh".into();
        ed.config.llm.args = vec!["-c".into(), STAND_IN.into(), "stand-in".into()];
        ed.config.llm.chat_settings = false;
        ed.open(&root.join("src/s.rs")).unwrap();
        crate::chat::open(&mut ed);
        ed.workspace.claude.chat.as_mut().unwrap().follow.on = true;
        code_rows_at(&mut ed, S);
        let before = rows(&ed);
        // (a question taller than the chat, as the shot's: its view mode moves the transcript)
        let asked =
            format!("walk me through it: {}", "where does the order of the strands go wrong? ".repeat(12));
        ed.workspace.claude.chat.as_mut().unwrap().input = asked.as_str().into();
        crate::chat::send(&mut ed);
        let end = std::time::Instant::now() + crate::test_results::PATIENCE;
        loop {
            code_rows_at(&mut ed, S);
            let c = ed.workspace.claude.chat.as_ref().unwrap();
            let ended = rows(&ed) > before && !c.busy() && !c.follow.touring();
            if ended && crate::places::answered(&ed) && ed.workspace.claude.notes.answer.is_none() {
                break;
            }
            assert!(std::time::Instant::now() < end, "the turn didn't end within the patience");
            if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
                ed.handle_event(ev);
            }
        }
        assert_eq!(ed.workspace.viewer.at, crate::workspace::At::Chat);
        // (the keys given to the code after — `esc esc`, as the shot has it: the chat's view mode, then the code)
        let rows = code_rows_at(&mut ed, S);
        assert!(in_sight(&rows), "at its end:\n{}", rows.join("\n"));
        let scroll = |ed: &Editor| ed.workspace.claude.chat.as_ref().unwrap().scroll;
        let was = scroll(&ed);
        feed(&mut ed, "<esc>");
        code_rows_at(&mut ed, S);
        feed(&mut ed, "<esc>");
        assert_ne!(scroll(&ed), was, "the view mode left the chat scrolled: what the chat shows moved");
        assert_eq!(ed.workspace.viewer.at, crate::workspace::At::Code);
        let rows = code_rows_at(&mut ed, S);
        assert!(in_sight(&rows), "the code given the keys: its line and card in sight:\n{}", rows.join("\n"));
        assert!(ed.workspace.claude.notes.open.is_some());
    }
    // (paused at your cursor: the view yours, the card under its line out of sight)
    let (_s, root, mut ed) = spool("a1c-sight80-paused");
    ed.open(&root.join("src/s.rs")).unwrap();
    ed.workspace.claude.chat.as_mut().unwrap().follow.on = true;
    code_rows_at(&mut ed, S);
    answer_at(
        &mut ed,
        "<show at=\"src/s.rs:21-70\">The loop winds every strand.</show> Then it sorts.",
        S,
        true,
    );
    let rows = code_rows_at(&mut ed, S);
    assert!(!in_sight(&rows) && ed.doc().top == 0, "paused: the view yours:\n{}", rows.join("\n"));
    // (typing: it waits, out of sight, until you stop)
    let (_s, root, mut ed) = spool("a1c-sight80-typing");
    ed.open(&root.join("src/s.rs")).unwrap();
    ed.workspace.claude.chat.as_mut().unwrap().input = "and then?".into();
    code_rows_at(&mut ed, S);
    answer_at(
        &mut ed,
        "<show at=\"src/s.rs:21-70\">The loop winds every strand.</show> Then it sorts.",
        S,
        false,
    );
    assert_eq!((ed.workspace.claude.notes.open, ed.doc().top), (None, 0), "not while you type");
}

/// The code's scrollbar beside an open card (the 80-column shot): the card's rows aren't the file's, so the thumb skips
/// them — its column blank beside the card, the bar laid on the code's other rows, never down the card's edge.
#[test]
fn a1c_the_scrollbar_skips_the_cards_rows() {
    const S: (u16, u16) = (80, 24);
    let (_s, root, mut ed) = spool("a1c-thumb");
    ed.open(&root.join("src/s.rs")).unwrap();
    ed.workspace.claude.chat.as_mut().unwrap().follow.on = true;
    code_rows_at(&mut ed, S);
    answer_at(
        &mut ed,
        "<show at=\"src/s.rs:21-70\">The loop winds every strand.</show> Then it sorts.",
        S,
        false,
    );
    let rows = code_rows_at(&mut ed, S);
    let top = rows.iter().position(|r| r.contains("╭─ chat")).expect("its card in sight");
    let foot = top + rows[top..].iter().position(|r| r.contains("╰─")).expect("its foot");
    // (a row's last cell: the frame's edge on the card's rows, the thumb's on the code's)
    let thumb = |r: &String| r.trim_end().ends_with('▎');
    let crossed: Vec<&String> = rows[top..=foot].iter().filter(|r| thumb(r)).collect();
    assert!(crossed.is_empty(), "the thumb down the card's edge: {crossed:#?}\n{}", rows.join("\n"));
    // (the code's rows keep it: the view in the file's first third, the thumb drawn on its lines there)
    let code = |i: usize| (i < top || i > foot) && rows[i].contains("let line_");
    assert!(
        (0..rows.len()).any(|i| code(i) && thumb(&rows[i])),
        "the thumb on the code's rows:\n{}",
        rows.join("\n")
    );
}

/// `space L` with the turn's card in sight where follow held it (the 80-column shot): follow's hold goes with the chat,
/// and the view stays with the card and its line — it went back to your cursor on line 1, the card's line at the foot and
/// the card below it. Your next move brings the view back to your cursor, as the wheel's hold does.
#[test]
fn a1c_the_card_stays_with_its_line_as_the_chat_goes() {
    const S: (u16, u16) = (80, 24);
    let (_s, root, mut ed) = spool("a1c-wide");
    ed.open(&root.join("src/s.rs")).unwrap();
    ed.workspace.claude.chat.as_mut().unwrap().follow.on = true;
    code_rows_at(&mut ed, S);
    answer_at(
        &mut ed,
        "<show at=\"src/s.rs:21-70\">The loop winds every strand.</show> Then it sorts.",
        S,
        false,
    );
    ed.workspace.viewer.at = crate::workspace::At::Code;
    let block = |rows: &[String]| {
        let top = rows.iter().position(|r| r.contains("╭─ chat"))?;
        let foot = top + rows[top..].iter().position(|r| r.contains("╰─"))?;
        rows[top - 1].contains("let line_21 = 21;").then_some(foot)
    };
    assert!(block(&code_rows_at(&mut ed, S)).is_some(), "in sight before");
    feed(&mut ed, " L");
    assert!(ed.workspace.claude.chat.is_none(), "the chat gone");
    let rows = code_rows_at(&mut ed, S);
    assert!(block(&rows).is_some(), "the card whole, under its line:\n{}", rows.join("\n"));
    assert_eq!(ed.cursor_line(), 0, "your cursor where it was");
    feed(&mut ed, "j");
    let rows = code_rows_at(&mut ed, S);
    assert!(
        rows.iter().any(|r| r.contains("let line_6 = 6;")),
        "a move: the view at your cursor:\n{}",
        rows.join("\n")
    );
}

/// #261, the review's: two notes on one folded doc comment's two paragraphs each show on their own paragraph's row, and
/// still with the block's first rows above the screen.
#[test]
fn a1c_two_notes_on_one_doc_comment_each_show() {
    let (_s, root, mut ed) = spool("a1c-doc-two");
    answer(&mut ed, "The doc at src/s.rs:1 says the order.\n\nAnd src/s.rs:3 says how a strand is tied.");
    let s = root.join("src/s.rs");
    ed.open(&s).unwrap();
    feed(&mut ed, "Gq");
    assert_eq!(ed.workspace.claude.notes.open, None);
    feed(&mut ed, "gg");
    let marks = |ed: &mut Editor| {
        let rows = code_rows(ed);
        let at: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].contains('¶')).collect();
        (rows, at)
    };
    let (rows, at) = marks(&mut ed);
    let first = rows.iter().position(|r| r.contains("▎ The spool")).expect("its first paragraph");
    let second = rows.iter().position(|r| r.contains("▎ Each strand")).expect("its second");
    assert!(
        at.len() == 2 && (first..second).contains(&at[0]) && at[1] >= second,
        "one on each paragraph: {at:?}\n{}",
        rows.join("\n")
    );
    let spots = |ed: &Editor| {
        ed.screen
            .word_spots
            .borrow()
            .iter()
            .filter(|w| matches!(w.press, crate::wordspot::Press::Note(_)))
            .count()
    };
    assert_eq!(spots(&ed), 2, "both buttons");
    // (its first row above the screen — the view a row down: the second still shows)
    feed(&mut ed, "<C-e>");
    let (rows, at) = marks(&mut ed);
    assert!(!at.is_empty() && rows[at[at.len() - 1]].contains('▎'), "cut at the top:\n{}", rows.join("\n"));
}

/// #261, the review's second: two notes on one paragraph of a folded comment share its row — one mark, their count
/// together (`2 notes`), as two on one line; never one dropped. And at every width there's a mark: a paragraph whose
/// rows are full (wrapped short of the code's width by less than a `¶` takes) had none — it goes to the block's
/// nearest row with room.
#[test]
fn a1c_two_notes_on_one_paragraph_count_together() {
    let (_s, root, mut ed) = spool("a1c-doc-para");
    answer(&mut ed, "The doc at src/s.rs:3 says the tie.\n\nAnd src/s.rs:4 is short.");
    let s = root.join("src/s.rs");
    ed.open(&s).unwrap();
    feed(&mut ed, "Gq");
    feed(&mut ed, "gg");
    // (the narrow code's row has room for its `¶` alone, both behind it; wider, the mark says how many — at 200 the
    // paragraph is one full row, so the mark is on the blank row above it, where there's room for the count too)
    let rows = code_rows(&mut ed);
    assert_eq!(rows.iter().filter(|r| r.contains('¶')).count(), 1, "one mark:\n{}", rows.join("\n"));
    for w in [140u16, 160, 200] {
        code_rows_at(&mut ed, (w, 30));
        let rows = code_rows_at(&mut ed, (w, 30));
        let at: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].contains('¶')).collect();
        assert!(at.len() == 1, "{w}: one mark:\n{}", rows.join("\n"));
        assert!(rows[at[0]].contains("2 notes"), "{w}: both counted: {}", rows[at[0]]);
        let full = !rows[at[0]].contains("Short.");
        assert_eq!(full, w == 200, "{w}: on its paragraph's row unless that's full:\n{}", rows.join("\n"));
        if full {
            assert!(
                rows[at[0] + 1].contains("▎ Each strand"),
                "{w}: right above its paragraph:\n{}",
                rows.join("\n")
            );
        }
    }
}

/// #261: a note on a folded doc comment's line shows `¶` and its words on the block's rows of that line's paragraph —
/// the first of them with room: a button there, `]n` and `enter` on it as on any line.
#[test]
fn a1c_a_note_on_a_doc_comment_shows_on_its_row() {
    let (_s, root, mut ed) = spool("a1c-doc");
    answer(&mut ed, "The doc at src/s.rs:3 says how a strand is tied.");
    let s = root.join("src/s.rs");
    ed.open(&s).unwrap();
    feed(&mut ed, "Gq");
    assert_eq!(ed.workspace.claude.notes.open, None);
    feed(&mut ed, "gg");
    let rows = code_rows(&mut ed);
    // (its paragraph's rows full: its `¶` alone, the card says the rest)
    let at = rows.iter().position(|r| r.contains('¶')).unwrap_or_else(|| panic!("its line end: {rows:#?}"));
    assert!(
        rows[at].contains("▎ ") && !rows.iter().any(|r| r.contains("///")),
        "on the folded block: {rows:#?}"
    );
    let second = rows.iter().position(|r| r.contains("▎ Each strand")).expect("its paragraph");
    assert!(at >= second, "in its line's paragraph, never the one before: {rows:#?}");
    assert!(
        rows[second..=at].iter().all(|r| r.split('▎').nth(1).is_some_and(|t| !t.trim().is_empty())),
        "no blank row between: {rows:#?}"
    );
    let id = ed.workspace.claude.notes.list[0].id;
    let y = (ed.screen.word_spots.borrow().iter())
        .find(|w| w.press == crate::wordspot::Press::Note(id))
        .map(|w| w.y);
    assert_eq!(y, Some(at), "its `¶` a button there");
    feed(&mut ed, "]n");
    assert_eq!(
        (ed.cursor_line(), ed.workspace.claude.notes.open),
        (2, Some(id)),
        "`]n` there, its card open"
    );
    feed(&mut ed, "<ret>");
    assert_eq!(ed.workspace.claude.notes.open, None, "`enter` on it: closed");
    feed(&mut ed, "<ret>");
    assert_eq!(ed.workspace.claude.notes.open, Some(id), "and open again");
}
