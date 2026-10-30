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
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::User, "where?", ""));
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

/// Reading step 2 over: a turn's card leaves your cursor where it was, so its keys are said only where they act on it.
/// With no note on your line `space n` and `enter` are the card's, its words pressed are the card's wherever you are;
/// on another note's line the keys are that note's and the card doesn't say them, nor counts that line's notes as its own.
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
    // On a line with none: the card's, and it says so
    feed(&mut ed, "4G");
    let rows = code_rows(&mut ed).join("\n");
    assert!(rows.contains("space n") && rows.contains("enter"), "{rows}");
    feed(&mut ed, " n");
    assert!(is(aimed(&ed), card), "{:?}", aimed(&ed));
    feed(&mut ed, "<esc><ret>");
    assert!(ed.workspace.claude.notes.earlier, "`enter` unfolded them, as the card said");
    let rows = code_rows(&mut ed).join("\n");
    assert!(rows.contains("It is read in src/a.rs:7."), "{rows}");
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
    crate::notes::send(&mut ed, id, "why there?", false, None, None);
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
    assert!(card.contains("The fix goes in") && card.contains("- Same.rs:1 is two files"), "{card}");
    // (closed: the line end its sentence)
    feed(&mut ed, "<esc>");
    let rows = screen(&mut ed);
    let line = rows.iter().find(|r| r.contains("let line_3 = 3;")).unwrap();
    assert!(line.contains("¶ The count is read twice in src/a.rs:3."), "{line}");
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
    assert!(l5.contains("¶ Now src/a.rs:5 holds it."), "{l5}");
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
    }
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
    assert!(line.contains("¶ The fix is in src/a.rs:6.") && !line.contains("notes"), "{line}");
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

/// The flow's two lines as a card wraps them: this line's paragraph keeps its bar on every row it wraps to (its rows
/// after the first hang under it, what a copy drops); one placed elsewhere is one row, cut with `…`, its whole carried.
#[test]
fn the_flows_lines_wrap_with_their_bar_and_cut_to_one_row() {
    use crate::markdown::{Line, Span};
    let sp = |t: &str| Span { cut: None, hint: false, text: t.into(), style: Default::default() };
    let marked =
        Line::Marked { mark: vec![sp("▎ ")], spans: vec![sp("one two three four five six seven eight")] };
    let stub =
        Line::Cut(vec![sp("› "), sp("src/b.rs:2"), sp("  "), sp("The guard stays as it is, every caller")]);
    let (rows, drawn) =
        crate::markdown::wrap_drawn(&[marked, stub], 16, Default::default(), Default::default());
    let text: Vec<String> = rows.iter().map(|r| r.iter().map(|s| s.text.as_str()).collect()).collect();
    assert_eq!(text, ["▎ one two three", "▎ four five six", "▎ seven eight", "› src/b.rs:2  T…"]);
    assert_eq!(drawn[1].join.map(|j| j.drop), Some(2), "the bar isn't the text's");
    assert!(rows[3].last().is_some_and(|s| s.cut.is_some()), "the stub's whole carried: {:?}", rows[3]);
}

/// A flow longer than its card has room for keeps its paragraph about this line in sight: what's before it and past the
/// room folded into `…` rows (whole on their cards) — never the newest part a thread keeps.
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
    assert!(card.lines().filter(|r| r.trim_matches(|c| c == '│' || c == ' ') == "…").count() == 2, "{card}");
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
