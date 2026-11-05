//! #239, #388: the places the chat's answers name are flags at their lines, each with the sentence that named it, cut
//! from the answer as written; kept for the conversation, newest first (focus A1b); judged by the turn (focus F0).

use std::path::{Path, PathBuf};

use super::*;
use crate::chat::{Chat, Msg, Role};

const W: u16 = 110;
const H: u16 = 30;

/// A project of the test's own with `files`, ten lines each, and a chat with no claude behind it — out of sight, as the
/// ask line leaves it once its message went: in sight, the column has the answer and no card opens by itself.
pub(crate) fn project(name: &str, files: &[&str]) -> (crate::scratch::Scratch, PathBuf, Editor) {
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
    let mut chat = Chat::new();
    chat.hidden = true;
    ed.workspace.claude.chat = Some(chat);
    (scratch, root, ed)
}

/// A turn that ends with `text` as its answer: its places waiting, a frame asks their names, their answers land.
pub(crate) fn answer(ed: &mut Editor, text: &str) {
    answers(ed, &[text], Vec::new());
}

/// `answer` for a turn that stopped at `stops` and answered in the messages `texts`.
fn answers(ed: &mut Editor, texts: &[&str], stops: Vec<crate::replay::Stop>) {
    answers_to(ed, "where?", texts, stops);
}

/// `answers` to the question `asked`.
pub(crate) fn answers_to(ed: &mut Editor, asked: &str, texts: &[&str], stops: Vec<crate::replay::Stop>) {
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
        crate::places::answered(ed) && ed.workspace.claude.flags.answer.is_none()
    });
}

/// The flags as (file, first line from 1, the sentence that named it).
fn chat_notes(ed: &Editor, root: &Path) -> Vec<(String, usize, String)> {
    let mut v: Vec<_> = (ed.workspace.claude.flags.list.iter())
        .map(|n| (crate::chat::shown(root, &n.path), n.lines.start + 1, n.words.clone()))
        .collect();
    v.sort();
    v
}

/// `chat_notes` of the newest answer alone (focus A1b: the ones before it are kept beside them).
fn newest_notes(ed: &Editor, root: &Path) -> Vec<(String, usize, String)> {
    let k = ed.workspace.claude.flags.answers;
    let mut v: Vec<_> = (ed.workspace.claude.flags.list.iter())
        .filter(|n| n.origin.n == k)
        .map(|n| (crate::chat::shown(root, &n.path), n.lines.start + 1, n.words.clone()))
        .collect();
    v.sort();
    v
}

pub(crate) fn feed(ed: &mut Editor, keys: &str) {
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

pub(crate) fn screen(ed: &mut Editor) -> Vec<String> {
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
    let places = places_json(&ed.workspace.claude.flags, &c.msgs).expect("two answers placed");
    let rows = crate::chats::rows_of(&c.msgs);
    // (a new conversation: they go)
    crate::flags::forget(&mut ed);
    assert!(chat_notes(&ed, &root).is_empty());
    // (continued: its rows back, then its places)
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs = crate::chats::msgs_of(&rows);
    c.msgs.push(Msg::new(Role::Note, "continued from 1m ago", ""));
    restore(&mut ed, Some(&places));
    assert_eq!(chat_notes(&ed, &root), before, "{places}");
    // (as kept: a continue has nothing to keep — else the next event saved it again, a conversation only looked at made
    // the newest)
    assert_eq!(ed.workspace.claude.flags.kept, before.len());
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
        places: places_json(&ed.workspace.claude.flags, &c.msgs),
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
    crate::flags::forget(&mut ed);
    ed.workspace.claude.chat.as_mut().unwrap().msgs = msgs;
    restore(&mut ed, saved.places.as_ref());
    let first = ("src/a.rs".to_string(), 3, "The count is read in src/a.rs:3.".to_string());
    assert_eq!(chat_notes(&ed, &root), [first]);
    let said: Vec<&str> =
        ed.messages.iter().map(|m| m.2.as_str()).filter(|m| m.contains("no longer found")).collect();
    assert_eq!(said, ["3 places of the conversation's answers no longer found"]);
    // (its file opened: the line found again by its text, two down)
    ed.open(&a).unwrap();
    crate::flags::attach(&mut ed);
    assert_eq!(chat_notes(&ed, &root)[0].1, 5);
}

/// Reading step 2 over: a place whose path is no text — under a folder so named, its name found by its last parts — is
/// kept by no store; `json!` would panic, and a release build aborts on one.
#[test]
fn a_place_whose_path_is_no_text_is_left_out_of_the_store() {
    use std::os::unix::ffi::OsStrExt;
    let (_s, root, mut ed) = project("chatnotes-bytes", &["src/a.rs", "src/b.rs"]);
    answer(&mut ed, "It is read in src/a.rs:3 and src/b.rs:2.");
    let b = root.join("src/b.rs");
    let n = ed.workspace.claude.flags.list.iter_mut().find(|n| n.path == b).unwrap();
    n.path = PathBuf::from(std::ffi::OsStr::from_bytes(b"/x/\xff/b.rs"));
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let places = places_json(&ed.workspace.claude.flags, &c.msgs).unwrap();
    assert_eq!(places[0]["places"].as_array().unwrap().len(), 1, "{places}");
}
/// The reviewer's note 35: a chat closed mid-turn — your question in no store yet, a keep then counted from it — keeps its
/// answers' places: both come back on a continue, none said lost.
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
    assert!(crate::chat::left_kept(&ed).is_some());
    crate::test_results::settle_until(&mut ed, |_| kept(&dir, &root) == [2, 6]);
    crate::chat::open(&mut ed);
    let saved = crate::chats::load(&dir, &root).into_iter().find(|c| c.id == "c1").unwrap();
    crate::chat::continue_with(&mut ed, saved);
    let lost = ed.messages.iter().any(|m| m.2.contains("no longer found"));
    assert!(!lost, "{:?}", ed.messages.iter().map(|m| &m.2).collect::<Vec<_>>());
    assert_eq!(
        chat_notes(&ed, &root),
        [
            ("src/a.rs".to_string(), 3, "It is read in src/a.rs:3.".to_string()),
            ("src/a.rs".to_string(), 7, "It is set in src/a.rs:7.".to_string())
        ]
    );
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

/// The cut, by the answer's text as written: a paragraph naming two places gives each its own sentence (plain); a list
/// item is that item; a range covers its lines; fenced code is no prose; a walk-through's stop is its `why`; `e.g.` ends
/// no sentence; a place named twice is said the first time.
#[test]
fn an_answer_is_cut_into_each_places_sentence() {
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

/// The key-sequence walk (#388): an answer's places are flags at their lines — a `¶` alone at the line end, no card
/// opening at the turn's end, no words at the code; a name that comes to no file (or to several) is no place and fenced
/// code none; the next answer's places join them; an answer whose names come to nothing adds none. Flags are never the
/// restore file's (they come back with their conversation — `restore`) and never notes.
#[test]
fn the_answers_places_are_flags_at_their_lines() {
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
    // No card opened at the turn's end, no words at the code: the line end a `¶` alone (#388) — the answer the
    // column's, shown by itself (chatlink Q5)
    ed.open(&root.join("src/a.rs")).unwrap();
    assert_eq!((ed.cursor_line(), ed.workspace.claude.notes.open), (0, None), "your cursor where it was");
    assert!(crate::chat::column_in_sight(&ed), "the answer's words after its last call show the column");
    let rows = screen(&mut ed);
    let code = |r: &String| r.split('│').next().unwrap_or_default().trim_end().to_string();
    let line = rows.iter().map(code).find(|r| r.contains("let line_3 = 3;")).unwrap();
    assert!(line.ends_with("let line_3 = 3;   ¶"), "{line}");
    assert!(
        rows.iter().map(code).all(|r| !r.contains("The count is read")),
        "the answer nowhere at the code"
    );
    // Never notes, never in the restore file
    assert!(ed.workspace.claude.notes.list.is_empty());
    assert!(crate::notes::to_json(&ed.workspace.claude.notes, &ed.docs).is_empty());
    // An answer whose names come to nothing adds none
    answer(&mut ed, "Nothing in Gone.rs:1 nor Same.rs:2.");
    assert_eq!(chat_notes(&ed, &root), first);
    // The next one that names a place joins them (focus A1b)
    answer(&mut ed, "Now src/a.rs:5 holds it.");
    assert_eq!(
        chat_notes(&ed, &root),
        [
            ("src/a.rs".to_string(), 3, "The count is read twice in src/a.rs:3.".to_string()),
            ("src/a.rs".to_string(), 5, "Now src/a.rs:5 holds it.".to_string()),
            ("src/b.rs".to_string(), 2, "The fix goes in src/b.rs:2-4, after the guard.".to_string()),
        ]
    );
    // (the line ends: each its `¶`, an older answer's as the newest's)
    ed.open(&root.join("src/a.rs")).unwrap();
    let rows = screen(&mut ed);
    for t in ["let line_3 = 3;", "let line_5 = 5;"] {
        let row = rows.iter().map(code).find(|r| r.contains(t)).unwrap();
        assert!(row.ends_with(&format!("{t}   ¶")), "{row}");
    }
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

/// What the newest flag at `line` (from 1) of `path` says tarae saw.
fn saw(ed: &Editor, path: &Path, line: usize) -> Option<&'static str> {
    let n = newest_of(ed, |n| n.path == path && n.lines.start + 1 == line)?;
    Some(n.check?.word(crate::docpaths::find(ed, path)?))
}

/// An answer's three verdicts: the chat says the doubts after the rows that name their places, faint (chatlink Q3); the
/// code's line ends are `¶` alone; a flag's hover row says every verdict, a doubt or not.
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
    // (wide: the chat's measure holds a row's words and its doubt — one that doesn't fit is the hover's alone)
    let (w, h) = (200, H);
    let mut bytes = Vec::new();
    crate::term::render(&mut ed, &mut bytes, w, h).unwrap();
    let mut vt = vt100::Parser::new(h, w, 0);
    vt.process(&bytes);
    let rows: Vec<String> = vt.screen().rows(0, w).collect();
    let row = |want: &str| rows.iter().any(|r| r.contains(want));
    assert!(row("The guard moved to src/a.rs:7. · changed, no test seen"), "{rows:#?}");
    assert!(row("Nothing past src/a.rs:9 reads it. · not read this turn"), "{rows:#?}");
    assert!(!row("read this turn ·") && !row("src/a.rs:2. ·"), "a read place no doubt: {rows:#?}");
    for l in ["let line_2 = 2;", "let line_7 = 7;", "let line_9 = 9;"] {
        let r = rows.iter().find(|r| r.contains(l)).unwrap();
        assert!(r.split('│').next().unwrap().trim_end().ends_with(&format!("{l}   ¶")), "{r}");
    }
    let doc = crate::docpaths::find(&ed, &a);
    let hover = |line: usize| {
        let f = newest_of(&ed, |n| n.path == a && n.lines.start + 1 == line).unwrap();
        crate::flags::row_of(f, doc).0
    };
    assert_eq!(
        [hover(2), hover(7), hover(9)],
        [Some("read this turn"), Some("changed, no test seen"), Some("not read this turn")]
    );
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
        crate::places::answered(ed) && ed.workspace.claude.flags.answer.is_none()
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
    assert_eq!(saw(&ed, &a, 2), Some("read this turn"), "its stops long gone");
    assert_eq!(saw(&ed, &a, 8), Some("not read this turn"));
    let rows = screen(&mut ed);
    assert!(rows.iter().all(|r| !r.contains("read this turn")), "never at a line's end: {rows:#?}");
    // Continued: its words back, what tarae saw of them gone with the stops
    let places = {
        let c = ed.workspace.claude.chat.as_ref().unwrap();
        places_json(&ed.workspace.claude.flags, &c.msgs)
    };
    restore(&mut ed, places.as_ref());
    assert_eq!(saw(&ed, &a, 2), None);
    ed.open(&b).unwrap();
    assert_eq!(saw(&ed, &b, 1), None);
}

/// An answer that names no place claims nothing of a line: no flag — not at the turn's last stop either (#388: the words
/// are the chat's) — so nothing's said of one.
#[test]
fn a_place_the_answer_didnt_name_says_nothing() {
    let (_s, root, mut ed) = project("chatnotes-unnamed", &["src/a.rs"]);
    let a = root.join("src/a.rs");
    ed.open(&a).unwrap();
    let read = read_of(&mut ed, &a, 0..4);
    answers(&mut ed, &["Nothing else reads it."], vec![read]);
    assert!(ed.workspace.claude.flags.list.is_empty());
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

/// Whether the column is being taken to note `id`'s answer's paragraph (`notes::to_paragraph`), no card open — what
/// `enter` on an answer's line and a press on its `¶` do (the user, 2026-11-03).
pub(crate) fn reads(ed: &Editor, id: u64) -> bool {
    let Some(f) = crate::flags::find(ed, id) else { return false };
    let c = ed.workspace.claude.chat.as_ref();
    let at = c.and_then(|c| c.reading.as_ref()).map(|r| r.0.clone());
    c.is_some_and(|c| !c.hidden && !c.asking)
        && at == Some(f.reading())
        && ed.workspace.claude.notes.open.is_none()
}

/// The newest answer's of the flags `which` takes.
fn newest_of(ed: &Editor, which: impl Fn(&crate::flags::Flag) -> bool) -> Option<&crate::flags::Flag> {
    let flags = ed.workspace.claude.flags.list.iter().filter(|n| which(n));
    flags.max_by_key(|n| n.origin.n)
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
        if ended && crate::places::answered(ed) && ed.workspace.claude.flags.answer.is_none() {
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

/// The reviewer's two (#239): only a whole answer to a message of yours places flags. A bare turn — idle compaction's
/// `/compact`, no message of yours — places none: never the turn before's answer again. A turn that failed (or was
/// stopped) leaves the last whole answer's flags, never its own half sentence's.
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
    // A bare `/compact`: its end places nothing — the answer before not a second time
    let before = rows(&ed);
    assert!(crate::chat::submit_bare(&mut ed, "/compact", "compacting".into()), "sent bare");
    turn_ended(&mut ed, before);
    assert_eq!(chat_notes(&ed, &root), a(), "the turn before's answer placed again");
    assert_eq!(ed.workspace.claude.flags.answers, 1);
    // A new conversation; a whole answer places them; a failed turn's half sentence doesn't take their place
    crate::flags::forget(&mut ed);
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
    // (a whole answer naming no place, its turn's read in the project: no flag there — #388, its words the chat's)
    ask(&mut ed, "what reads it?");
    assert_eq!(chat_notes(&ed, &root), a(), "the whole answer's before it kept, none added");
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
    crate::test_results::settle_until(&mut ed, |ed| ed.workspace.claude.flags.answer.is_none());
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
    while ed.workspace.claude.flags.answer.is_some() {
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
