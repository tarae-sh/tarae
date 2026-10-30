//! #239: what the chat's last answer says of a place shows at it — the sentence at the line end, the paragraph on the
//! card, cut from the answer as written; the next answer that names places replaces them.

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
/// fenced code none — its block is the answer's note, at the answer's first place (focus A1); `space n` on one answers
/// it, a thread then, kept when the next answer's places replace the rest; an answer whose names come to nothing, its
/// turn stopped nowhere, leaves the last one's. None is kept across a restart but a thread.
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
    // `]n` from the top of src/a.rs: at its line, the sentence at the line end, the paragraph on its card — the
    // answer's place: its paragraph by the bar, the item whose names came to nothing after it (focus A1)
    ed.open(&root.join("src/a.rs")).unwrap();
    feed(&mut ed, "gg]n");
    assert_eq!(ed.cursor_line(), 2);
    let rows = screen(&mut ed);
    let line = rows.iter().find(|r| r.contains("let line_3 = 3;")).unwrap();
    assert!(line.contains("¶ The count is read twice in src/a.rs:3."), "{line}");
    let card = rows.join("\n");
    assert!(card.contains("¶ chat · the last answer"), "{card}");
    assert!(card.contains("▎ The count is read twice"), "the paragraph on the card, marked: {card}");
    assert!(card.contains("The fix goes in") && card.contains("- Same.rs:1 is two files"), "{card}");
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

/// The chat note at `path`'s line `line` (from 1) whose line end begins `gist`: its card's text.
fn card_of(ed: &Editor, path: &Path, line: usize, gist: &str) -> String {
    let n = ed
        .workspace
        .claude
        .notes
        .list
        .iter()
        .find(|n| n.path == path && n.lines.start + 1 == line && n.gist().starts_with(gist));
    n.unwrap_or_else(|| panic!("no note at {}:{line}", path.display())).thread[0].text.clone()
}

/// The flow of the chat note at `path`'s line `line` (from 1): each part's kind, its place if elsewhere, its text.
fn flow_of(ed: &Editor, path: &Path, line: usize) -> Vec<(&'static str, String, String)> {
    let n = ed.workspace.claude.notes.list.iter().find(|n| n.path == path && n.lines.start + 1 == line);
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
    // The keys: `]n` to it — one note on its line, the line end its sentence; the card the flow
    ed.open(&a).unwrap();
    feed(&mut ed, "gg]n");
    assert_eq!(ed.cursor_line(), 5);
    let rows = code_rows(&mut ed);
    let line = rows.iter().find(|r| r.contains("let line_6 = 6;")).unwrap();
    assert!(line.contains("¶ The fix is in src/a.rs:6.") && !line.contains("notes"), "{line}");
    let card = rows.join("\n");
    assert!(!card.contains(" of 2 here"), "one note: {card}");
    for said in
        ["It cuts by the source text", "▎ The fix is in src/a.rs:6.", "› src/b.rs:2  The guard", "One caveat"]
    {
        assert!(card.contains(said), "{said:?} on the card: {card}");
    }
    assert!(!card.contains("Let me look") && !card.contains("First I read"), "the narration: {card}");
    // (`]n` on: the next file's — the line held one)
    feed(&mut ed, "]n");
    assert_eq!((ed.doc().path.as_deref(), ed.cursor_line()), (Some(root.join("src/b.rs").as_path()), 1));
    // An answer all of whose blocks are placed: its notes alone, no flow
    answer(&mut ed, "Only src/a.rs:2 now.");
    assert_eq!(chat_notes(&ed, &root), [(s("src/a.rs"), 2, s("Only src/a.rs:2 now."))]);
    assert!(flow_of(&ed, &a, 2).is_empty());
}

/// A flow's line for a paragraph placed elsewhere is one row however narrow the card: cut with `…`.
#[test]
fn a_paragraph_placed_elsewhere_is_one_row_on_a_narrow_card() {
    let (_s, root, mut ed) = project("chatnotes-narrow", &["src/a.rs", "src/b.rs"]);
    let long =
        "The guard in src/b.rs:2 stays as it is, since every caller checks the length before it gets there.";
    answer(&mut ed, &format!("The fix is in src/a.rs:6.\n\n{long}\n\nOne caveat: nothing else."));
    ed.open(&root.join("src/a.rs")).unwrap();
    feed(&mut ed, "gg]n");
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
    ed.open(&root.join("src/a.rs")).unwrap();
    feed(&mut ed, "gg]n");
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
    feed(&mut ed, "gg]n");
    let card = code_rows(&mut ed).join("\n");
    assert!(card.contains("▎ The fix is in src/a.rs:6.") && card.contains("Paragraph 9 "), "{card}");
    let foot = card.lines().position(|r| r.starts_with('╰')).unwrap();
    let last = card.lines().nth(foot - 1).unwrap();
    assert!(last.trim_matches(|c| c == '│' || c == ' ') == "are kept in order.", "{card}");
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
/// and replaces the last answer's notes (#239's rule as A1 changed it); a turn that stopped nowhere in the project leaves
/// them.
#[test]
fn an_answer_with_no_place_is_at_the_turns_last_stop() {
    let (_s, root, mut ed) = project("chatnotes-stops", &["src/a.rs", "src/b.rs"]);
    let (a, b) = (root.join("src/a.rs"), root.join("src/b.rs"));
    answer(&mut ed, "It is read in src/a.rs:3.");
    let last = [("src/a.rs".to_string(), 3, "It is read in src/a.rs:3.".to_string())];
    assert_eq!(chat_notes(&ed, &root), last);
    // (stopped nowhere, or only outside the project: the last answer's stay)
    answer(&mut ed, "Nothing else to add.");
    assert_eq!(chat_notes(&ed, &root), last);
    let outside = crate::scratch::Scratch::new("chatnotes-stops-outside");
    answers(&mut ed, &["Read it."], vec![stop(&outside.join("x.rs"), 0..2, false)]);
    assert_eq!(chat_notes(&ed, &root), last);
    // The last edit, an earlier one and reads after it passed over
    let stops = vec![stop(&b, 6..8, true), stop(&a, 4..5, true), stop(&b, 0..2, false)];
    answers(&mut ed, &["Done. Nothing else relied on it."], stops);
    assert_eq!(chat_notes(&ed, &root), [("src/a.rs".to_string(), 5, "Done.".to_string())]);
    assert_eq!(card_of(&ed, &a, 5, "Done."), "Done. Nothing else relied on it.");
    // No edit: the last read in the project (one outside after it passed over)
    let stops = vec![stop(&a, 1..3, false), stop(&b, 7..9, false), stop(&outside.join("x.rs"), 0..2, false)];
    answers(&mut ed, &["Read both."], stops);
    assert_eq!(chat_notes(&ed, &root), [("src/b.rs".to_string(), 8, "Read both.".to_string())]);
    // A place the answer names wins over the stops: the flow on its note there
    answers(&mut ed, &["Done.\n\nThe guard in src/b.rs:2 stays."], vec![stop(&a, 4..5, true)]);
    let s = |s: &str| s.to_string();
    assert_eq!(chat_notes(&ed, &root), [(s("src/b.rs"), 2, s("The guard in src/b.rs:2 stays."))]);
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
    assert_eq!(chat_notes(&ed, &root), [("src/b.rs".to_string(), 2, "Nothing else reads it.".to_string())]);
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
