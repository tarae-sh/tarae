//! #389 step 4: a name in the chat answers the code's language keys at the spot in the file the answer cites or quotes —
//! the keys in sequence on the chat's view mode, a frame after each, a stand-in server (`cat`) answered by hand.

use super::*;
use crate::chat::{Chat, Msg};
use crate::key::{Code, Key};
use serde_json::json;

const W: u16 = 140;
const H: u16 = 34;

const SRC: &str =
    "fn helper() -> u32 {\n    1\n}\nfn main() {\n    let total = helper();\n    println!(\"{total}\");\n}\n";

/// The answer: a backticked name in a paragraph that cites a line, a block quoting a file after a paragraph citing
/// it, and a block quoting nothing.
const ANSWER: &str = "`main` at src/main.rs:4 keeps `total` and `stray`.\n\nThe call, src/main.rs:5:\n\n```rust\n    let total = helper();\n    // …\n}\n```\n\nA proposal:\n\n```rust\nfn other() { helper() }\n```\n";

/// A project of the test's own with `src/main.rs` (`SRC`), its server a stand-in when `server`, the chat up on
/// `ANSWER` with the keys, its places found and its blocks mapped.
fn setup(name: &str, server: bool, answer: &str) -> (crate::scratch::Scratch, Editor, PathBuf) {
    let scratch = crate::scratch::Scratch::new(name);
    let root = std::fs::canonicalize(&*scratch).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    let file = root.join("src/main.rs");
    std::fs::write(&file, SRC).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
    let cfg = match server {
        true => format!(
            "[lsp.fake]\ncommand = \"sh\"\nargs = [\"-c\", \"cat > '{}'\"]\n[lang.rust]\nlsp = [\"fake\"]\n",
            root.join("sent.log").display()
        ),
        false => "[lang.rust]\nlsp = []\n".to_string(),
    };
    let (config, w) = crate::config::parse(&cfg);
    assert!(w.is_empty(), "{w:?}");
    let mut ed = Editor::new(config);
    ed.workspace.root = root.clone();
    ed.open(&file).unwrap();
    if server {
        let cid = ed.doc().lsp.client.expect("attached");
        let init = json!({ "id": 0, "result": { "capabilities": { "positionEncoding": "utf-8",
            "hoverProvider": true, "definitionProvider": true, "referencesProvider": true } } });
        ed.on_lsp_message(cid, init);
    }
    let mut c = Chat::new();
    c.msgs = vec![Msg::new(Role::User, "q", ""), Msg::new(Role::Assistant, answer, "")];
    ed.workspace.claude.chat = Some(c);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    until(&mut ed, "its places found and its blocks mapped", |ed| {
        let c = ed.workspace.claude.chat.as_ref().unwrap();
        crate::places::answered(ed) && !c.links.asking && c.links.maps.contains_key(&1)
    });
    (scratch, ed, file)
}

/// A frame, as the screen shows it.
fn frame(ed: &mut Editor) -> String {
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, W, H).unwrap();
    let mut p = vt100::Parser::new(H, W, 0);
    p.process(&bytes);
    p.screen().contents()
}

/// Jobs handled, a frame after each, until `done`.
fn until(ed: &mut Editor, what: &str, done: impl Fn(&Editor) -> bool) {
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    frame(ed);
    while !done(ed) {
        assert!(std::time::Instant::now() < end, "{what}");
        if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
            ed.handle_event(ev);
            frame(ed);
        }
    }
}

/// Keys as `run` takes them (`<esc>`, `<ret>`), a frame after each.
fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(key);
        frame(ed);
    }
}

/// The chat's question asked of the server: its request's id and what it asks (a go-to, or the hover).
fn asked(ed: &Editor) -> Option<(ClientId, u64, Option<Goto>)> {
    ed.lsp.pending.iter().find_map(|(&(cid, id), p)| match p.kind {
        Kind::ChatAt { goto, .. } => Some((cid, id, goto)),
        _ => None,
    })
}

/// The column the chat's line starts at: where its divider meets the top line.
fn chat_left(shown: &str) -> usize {
    shown.lines().next().and_then(|l| l.chars().position(|c| c == '┬')).expect("a chat column")
}

/// A press at `name` on the chat's side of the row holding `row` (its last one there), ctrl held or not.
fn press(ed: &mut Editor, row: &str, name: &str, ctrl: bool) {
    use crate::event::{Mouse, MouseKind};
    let shown = frame(ed);
    let left = chat_left(&shown);
    let (y, line) = shown.lines().enumerate().filter(|(_, l)| l.contains(row)).last().expect(row);
    let chars: Vec<char> = line.chars().collect();
    let rest: String = chars[left..].iter().collect();
    let x = left + rest[..rest.rfind(name).expect(name)].chars().count() + 1;
    for kind in [MouseKind::Down, MouseKind::Up] {
        ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl });
    }
    frame(ed);
}

fn spot(ed: &Editor) -> Spot {
    match target(ed) {
        Ok(Target::At(s)) => s,
        t => panic!("a spot in the file: {t:?}"),
    }
}

/// A name in a block that quotes the file: `gd` asks the server at that very spot — its line and column in the file,
/// whatever the quote's indent — and its answer goes there; `space k` there shows the server's hover in the chat, by
/// the name, the keys still the chat's. A backticked name in a paragraph that cites a line: found on the lines
/// around it. A server busy loading answers empty: said, asked again as its work ends.
#[test]
fn a_name_the_answer_quotes_asks_its_server_at_that_spot() {
    let (_s, mut ed, file) = setup("chatlink-quoted", true, ANSWER);
    feed(&mut ed, "<esc>/helper<ret>");
    let s = spot(&ed);
    assert_eq!((s.line, s.col, s.word.as_str()), (4, 16, "helper"), "the quoted line's name, in the file");
    feed(&mut ed, "gd");
    let (cid, id, goto) = asked(&ed).expect("asked");
    assert_eq!(goto, Some(Goto::Definition));
    let a = ed.workspace.claude.chat.as_ref().unwrap().links.asked.clone().expect("kept");
    assert_eq!(ed.docs.iter().find(|d| d.id == a.doc).and_then(|d| d.path.clone()), Some(file.clone()));
    assert_eq!(a.byte, SRC.match_indices("helper").nth(1).unwrap().0, "at the name in the file");
    // (its answer: the definition, gone to — the chat keeps the keys, as a place pressed in it does)
    let to = json!([{ "uri": crate::lsp::uri(&file), "range": { "start": { "line": 0, "character": 3 },
        "end": { "line": 0, "character": 9 } } }]);
    ed.on_lsp_message(cid, json!({ "id": id, "result": to }));
    frame(&mut ed);
    let d = ed.doc();
    assert_eq!((d.path.as_deref(), d.selection().primary().cursor(&d.text)), (Some(file.as_path()), 3));
    assert!(ed.workspace.is_at(crate::workspace::At::Chat), "the keys stay in the chat");
    // `space k`: the hover, in the chat at the name
    feed(&mut ed, " k");
    let (cid, id, goto) = asked(&ed).expect("the hover asked");
    assert_eq!(goto, None);
    let hover =
        json!({ "contents": { "kind": "markdown", "value": "```rust\nfn helper() -> u32\n```\nhelps" } });
    ed.on_lsp_message(cid, json!({ "id": id, "result": hover }));
    let shown = frame(&mut ed);
    assert!(ed.popup_chat && shown.contains("╭─ helper"), "its card:\n{shown}");
    let card = ed.screen.popup.get().expect("drawn");
    assert!(card.0 > chat_left(&shown) && card.0 + card.2 <= W as usize, "in the chat's column: {card:?}");
    feed(&mut ed, "<esc>");
    assert!(ed.popup.is_none() && !ed.popup_chat, "esc closes it");
    // A backticked name in prose: on the lines its paragraph cites
    feed(&mut ed, "gg/total<ret>");
    let s = spot(&ed);
    assert_eq!((s.line, s.col), (4, 8), "`total` near src/main.rs:4");
    // The server busy, its answer empty: said, asked again as its work ends
    feed(&mut ed, "gd");
    let (cid, id, _) = asked(&ed).expect("asked");
    let progress = |kind: &str| json!({ "method": "$/progress", "params": { "token": "t", "value": { "kind": kind, "title": "Indexing" } } });
    ed.on_lsp_message(cid, progress("begin"));
    ed.on_lsp_message(cid, json!({ "id": id, "result": [] }));
    assert!(frame(&mut ed).contains("fake is loading — going there when ready"));
    assert!(asked(&ed).is_none());
    ed.on_lsp_message(cid, progress("end"));
    assert!(asked(&ed).is_some_and(|a| a.1 != id), "asked again");
}

/// A cited place itself: `gd` goes to its line, `space k` shows its lines in the chat (no server asked).
#[test]
fn a_cited_place_goes_there_and_shows_its_lines() {
    let (_s, mut ed, file) = setup("chatlink-place", false, ANSWER);
    feed(&mut ed, "<esc>gg/main.rs:5<ret>");
    assert!(matches!(target(&ed), Ok(Target::Place { .. })), "{:?}", target(&ed));
    feed(&mut ed, " k");
    let shown = frame(&mut ed);
    assert!(ed.popup_chat && shown.contains("╭─ src/main.rs:5"), "its peek:\n{shown}");
    assert!(shown.contains("let total = helper();"), "its lines:\n{shown}");
    feed(&mut ed, "<esc>gd");
    let d = ed.doc();
    assert_eq!(d.path.as_deref(), Some(file.as_path()));
    assert_eq!(d.text.byte_to_line(d.selection().primary().cursor(&d.text)), 4, "at its line");
}

/// Every miss is said where you look: a word that isn't code, a block that quotes nothing, a name not on the cited
/// lines, a file with no server, a line changed since the answer quoted it.
#[test]
fn the_chat_says_why_a_name_isnt_asked() {
    let (_s, mut ed, _file) = setup("chatlink-misses", false, ANSWER);
    let said = |ed: &mut Editor, keys: &str, what: &str| {
        feed(ed, keys);
        let toasts: Vec<&str> = ed.toasts.iter().map(|t| t.text.as_str()).collect();
        assert!(toasts.contains(&what), "{keys}: {what} — said: {toasts:?}");
        assert!(ed.screen.toast_card.get().is_some(), "{keys}: on screen");
        assert!(asked(ed).is_none(), "{keys}: nothing asked");
    };
    // (ctrl+press: `gd` at the name pressed — a plain press only puts the cursor there)
    press(&mut ed, "let total = helper();", "helper", false);
    assert!(ed.toasts.is_empty() && asked(&ed).is_none(), "a plain press asks nothing");
    assert_eq!(spot(&ed).word, "helper", "the cursor on it");
    press(&mut ed, "let total = helper();", "total", true);
    let toasts: Vec<&str> = ed.toasts.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(
        toasts,
        ["src/main.rs: there's no language server for rust files here"],
        "ctrl+press: gd there"
    );
    said(&mut ed, "gg/keeps<ret>gd", "not a name in the code");
    said(&mut ed, "gg/other<ret>gd", "this code block quotes no file the answer cites");
    said(&mut ed, "gg/stray<ret>gd", "`stray` isn't on the lines this paragraph cites (src/main.rs)");
    said(&mut ed, "gg/proposal<ret>gd", "not a name in the code");
    said(&mut ed, "gg/helper<ret> k", "src/main.rs: there's no language server for rust files here");
    // A line added above in its buffer, asked before a frame mapped it again: never asked at a guess
    let edit = |ed: &mut Editor, at: usize, text: &str| {
        let tx = crate::transaction::Transaction::new(vec![crate::transaction::Change::insert(at, text)]);
        ed.doc_mut().apply(&tx);
    };
    edit(&mut ed, 0, "// new\n");
    ed.toasts.clear();
    crate::chatlink::key(&mut ed, Some(Goto::Definition));
    let toasts: Vec<&str> = ed.toasts.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(toasts, ["src/main.rs changed since — `helper` isn't where the answer has it"]);
    // …mapped again after the frame: where it is now
    until(&mut ed, "mapped again", |ed| {
        ed.workspace.claude.chat.as_ref().unwrap().links.maps.contains_key(&1)
    });
    assert_eq!(spot(&ed).line, 5, "a line further down");
    // The quoted line gone from the file: said so
    let at = ed.doc().text.line_to_byte(5) + "    let total = ".len();
    edit(&mut ed, at, "x");
    until(&mut ed, "mapped again", |ed| {
        ed.workspace.claude.chat.as_ref().unwrap().links.maps.contains_key(&1)
    });
    said(&mut ed, "gd", "these lines aren't in src/main.rs as quoted");
}

/// Its parts: prose and fenced code in order, a list item a part of its own, an open fence code to its end.
#[test]
fn an_answer_reads_into_prose_and_code() {
    let p = parts("One\nline.\n- an item\n- another\n\n```rust\nlet a;\n\n```\n# Head\n~~~\nopen");
    assert_eq!(
        p,
        [
            Part::Prose("One line.".into()),
            Part::Prose("- an item".into()),
            Part::Prose("- another".into()),
            Part::Code { lang: "rust".into(), lines: vec!["let a;".into(), "".into()] },
            Part::Prose("# Head".into()),
            Part::Code { lang: String::new(), lines: vec!["open".into()] },
        ]
    );
    assert_eq!(cited("see a.rs:3-5 and b.rs:9"), [(Arc::from("a.rs"), 2, 4), (Arc::from("b.rs"), 8, 8)]);
}

/// A quoted block's lines in their file: by their text whatever the indent, nearest the cited line, an elision
/// skipping ahead, a line that drifted a little found, one not there none; a diff's removed lines none; with a window,
/// only near the line.
#[test]
fn a_quoted_block_finds_its_lines_in_the_file() {
    let file: Vec<String> =
        ["fn a() {", "    x();", "}", "", "fn b() {", "    x();", "    y();", "    z();", "    w();", "}"]
            .map(String::from)
            .to_vec();
    let code = |s: &str| s.lines().map(String::from).collect::<Vec<_>>();
    // (`x();` twice: the one nearest the cited line)
    assert_eq!(
        map_block(&file, &code("x();\n// …\nw();\n}"), 5, None, false),
        Some(vec![Some(5), None, Some(8), Some(9)])
    );
    assert_eq!(map_block(&file, &code("x();"), 0, None, false), Some(vec![Some(1)]));
    assert_eq!(
        map_block(&file, &code("fn b() {\n    z();\n  nope();"), 4, None, false),
        Some(vec![Some(4), Some(7), None])
    );
    assert_eq!(
        map_block(&file, &code(" fn b() {\n-    q();\n+    x();"), 4, None, true),
        Some(vec![Some(4), None, Some(5)])
    );
    assert_eq!(map_block(&file, &code("fn b() {"), 0, Some(2), false), None, "outside the window");
    assert_eq!(map_block(&file, &code("nothing"), 0, None, false), None);
}
