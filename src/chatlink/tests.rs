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
            "hoverProvider": true, "definitionProvider": true, "referencesProvider": true,
            "workspaceSymbolProvider": true } } });
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

/// The chat's question by name asked of the server: its request's id (a key's — never an underline's).
fn asked_by_name(ed: &Editor) -> Option<(ClientId, u64)> {
    let key = named(ed).map(|q| q.0)?;
    ed.lsp.pending.iter().find_map(|(&(cid, id), p)| match p.kind {
        Kind::ChatNamed { n } if n == key => Some((cid, id)),
        _ => None,
    })
}

/// The underlines' questions asked of the server: each request's id and the name it asks.
fn underlines_asked(ed: &Editor) -> Vec<(ClientId, u64, String)> {
    let batch = &ed.workspace.claude.chat.as_ref().unwrap().links.batch;
    let mut out: Vec<(ClientId, u64, String)> = (ed.lsp.pending.iter())
        .filter_map(|(&(cid, id), p)| match p.kind {
            Kind::ChatNamed { n } => batch.iter().find(|a| a.n == n).map(|a| (cid, id, a.word.to_string())),
            _ => None,
        })
        .collect();
    crate::sorting::by_key(&mut out, |a| a.2.clone());
    out
}

/// A `workspace/symbol` answer: each symbol's name, line and character in `file`, and container.
fn symbols(file: &Path, found: &[(&str, u32, u32, &str)]) -> Value {
    let one = |&(name, line, ch, container): &(&str, u32, u32, &str)| {
        json!({ "name": name, "kind": 12, "containerName": container, "location": { "uri": crate::lsp::uri(file),
            "range": { "start": { "line": line, "character": ch }, "end": { "line": line, "character": ch } } } })
    };
    Value::Array(found.iter().map(one).collect())
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
    said(&mut ed, "gg/proposal<ret>gd", "not a name in the code");
    // (step 5: a name no cited place holds is asked by name — no server here, so the syntax tree's stand-in, said)
    const NONE: &str = "no language server that searches by name is running here";
    let later = |ed: &mut Editor, keys: &str, what: &str| {
        ed.toasts.clear();
        feed(ed, keys);
        until(ed, what, |ed| ed.toasts.iter().any(|t| t.text == what));
    };
    later(&mut ed, "gg/other<ret>gd", &format!("{NONE}, and the syntax tree finds no definition of `other`"));
    let stray = "`stray` isn't on the lines this paragraph cites (src/main.rs)";
    later(
        &mut ed,
        "gg/stray<ret>gd",
        &format!("{stray} — {NONE}, and the syntax tree finds no definition of `stray`"),
    );
    // (a name the tree finds defined: there)
    later(&mut ed, "gg/other\\(\\) \\{ helpe<ret>gd", &format!("found by the syntax tree ({NONE})"));
    let d = ed.doc();
    assert_eq!(d.selection().primary().cursor(&d.text), 3, "on `helper` where it's defined");
    assert!(ed.workspace.is_at(crate::workspace::At::Chat), "the keys still the chat's");
    // (the other keys need a server: said)
    said(&mut ed, " k", &format!("{NONE}, so nothing is known of `helper`"));
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
    later(
        &mut ed,
        "gd",
        &format!("these lines aren't in src/main.rs as quoted — found by the syntax tree ({NONE})"),
    );
    // (the tree reads the buffer as it is, never as saved: its definition under the line added)
    let d = ed.doc();
    assert_eq!(d.text.byte_to_line(d.selection().primary().cursor(&d.text)), 1, "found by name instead");
}

/// #389 step 5: an answer naming code no cited place holds — a qualified name in prose, a bare one, one in a block
/// that quotes nothing.
const NAMED: &str = "The sum is in `Total::helper`; `helper` is called from `main`.\n\n```rust\nfn other() { helper() }\n```\n";

/// A name no cited place holds is asked of the workspace (`workspace/symbol`): the exact name's one definition gone to —
/// a qualified name's container picks it among several, a close name never counts —, several a list of places;
/// `space k` asks the hover at the one found, on the name in its line; a busy server's empty answer said and asked
/// again as its work ends; the cursor moved before the answer: dropped.
#[test]
fn a_name_no_cited_place_holds_is_asked_by_name() {
    let (s, mut ed, file) = setup("chatlink-named", true, NAMED);
    let sent = || std::fs::read_to_string(s.join("sent.log")).unwrap_or_default();
    // A qualified name: its container's of the two
    feed(&mut ed, "<esc>/Total::helpe<ret>");
    assert!(
        matches!(target(&ed), Ok(Target::Named { ref word, container: Some(ref c), why: None }) if word == "helper" && c == "Total"),
        "{:?}",
        target(&ed)
    );
    feed(&mut ed, "gd");
    let (cid, id) = asked_by_name(&ed).expect("asked by name");
    until(&mut ed, "sent", |_| sent().contains(r#""query":"helper""#));
    let found = [("helper", 4, 16, "main"), ("helper", 0, 3, "Total"), ("helpers", 1, 0, "Total")];
    ed.on_lsp_message(cid, json!({ "id": id, "result": symbols(&file, &found) }));
    frame(&mut ed);
    let d = ed.doc();
    assert_eq!((d.path.as_deref(), d.selection().primary().cursor(&d.text)), (Some(file.as_path()), 3));
    assert!(ed.workspace.is_at(crate::workspace::At::Chat), "the keys stay in the chat");
    assert!(ed.picker.is_none(), "one: no list");
    // A bare name with two definitions: a list of places, grouped by file
    // (the transcript draws a code span without its backticks)
    feed(&mut ed, "gg/; helpe<ret>gd");
    let (cid, id) = asked_by_name(&ed).expect("asked by name");
    ed.on_lsp_message(cid, json!({ "id": id, "result": symbols(&file, &found) }));
    until(&mut ed, "its list", |ed| ed.picker.is_some());
    let p = ed.picker.as_ref().unwrap();
    assert_eq!((p.kind, p.title.as_str()), ("places", "definitions"));
    let rows: Vec<&str> =
        p.items().iter().filter(|i| i.label.contains("src/main.rs:")).map(|i| &*i.label).collect();
    assert_eq!(rows.len(), 2, "the two exact ones, never `helpers`: {rows:?}");
    feed(&mut ed, "<esc>");
    assert!(ed.picker.is_none());
    // `space k` on a name in a block that quotes nothing: the hover asked at the one found, on its name
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    feed(&mut ed, "<esc>gg/\\{ helpe<ret> k");
    let (cid, id) = asked_by_name(&ed).expect("asked by name");
    ed.on_lsp_message(cid, json!({ "id": id, "result": symbols(&file, &[("helper", 0, 0, "")]) }));
    let (_, _, goto) = asked(&ed).expect("the hover asked");
    assert_eq!(goto, None);
    let a = ed.workspace.claude.chat.as_ref().unwrap().links.asked.clone().expect("kept");
    assert_eq!(a.byte, 3, "on the name in its line, not where its item starts");
    // The server busy: said, asked again as its work ends
    feed(&mut ed, "<esc>gd");
    let (cid, id) = asked_by_name(&ed).expect("asked by name");
    let progress = |kind: &str| json!({ "method": "$/progress", "params": { "token": "t", "value": { "kind": kind, "title": "Indexing" } } });
    ed.on_lsp_message(cid, progress("begin"));
    ed.on_lsp_message(cid, json!({ "id": id, "result": [] }));
    assert!(frame(&mut ed).contains("fake is loading — going there when ready"));
    assert!(asked_by_name(&ed).is_none());
    ed.on_lsp_message(cid, progress("end"));
    let (_, again) = asked_by_name(&ed).expect("asked again");
    assert_ne!(again, id);
    // Its answer after the transcript's cursor moved: dropped
    feed(&mut ed, "h");
    ed.toasts.clear();
    let before = ed.doc().selection().primary().cursor(&ed.doc().text);
    ed.on_lsp_message(cid, json!({ "id": again, "result": symbols(&file, &[("helper", 4, 16, "")]) }));
    frame(&mut ed);
    assert_eq!(ed.doc().selection().primary().cursor(&ed.doc().text), before, "not gone to");
    assert!(named(&ed).is_none() && ed.toasts.is_empty(), "dropped, unsaid: {:?}", ed.toasts);
    // Nothing of that name: said
    feed(&mut ed, "gd");
    let (cid, id) = asked_by_name(&ed).expect("asked by name");
    ed.on_lsp_message(cid, json!({ "id": id, "result": symbols(&file, &[("helpers", 1, 0, "")]) }));
    let toasts: Vec<&str> = ed.toasts.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(toasts, ["fake found nothing named `helper`"]);
}

/// #389 step 5b: names written as code — two that come to a definition, one that doesn't, a keyword, words, a file — and
/// a block with a keyword and a name in it.
const UNDERLINED: &str = "Both `helper` and `Total::helper` are found; `nothing_here` isn't.\n\nSkip `let`, `cargo test` and `main.rs`.\n\n```rust\nlet x = helper();\n```\n";

/// The words underlined in the chat's column, each run of underlined cells one, top to bottom.
fn underlined(ed: &mut Editor) -> Vec<String> {
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, W, H).unwrap();
    let mut p = vt100::Parser::new(H, W, 0);
    p.process(&bytes);
    let s = p.screen();
    let left = chat_left(&s.contents()) as u16;
    let mut out = Vec::new();
    for y in 0..H {
        let mut run = String::new();
        for x in left..W {
            match s.cell(y, x).filter(|c| c.underline()) {
                Some(c) => run.push_str(c.contents()),
                None if !run.is_empty() => out.push(std::mem::take(&mut run)),
                None => {}
            }
        }
        out.extend((!run.is_empty()).then_some(run));
    }
    out
}

/// A name written as code is underlined once it comes to a definition by name, asked after the frame that drew it — once,
/// in a batch, never a keyword, words or a file's name; one that comes to none stays plain; a loading server's empty
/// answer asked again as its work ends; a press on an underlined name is `gd` there. A keyword's `gd` is said at once,
/// nothing asked — in prose and in a block alike.
#[test]
fn a_name_written_as_code_is_underlined_once_it_comes_to_a_definition() {
    // (its keywords read from the grammar's highlights query)
    crate::syntax::Loader::global().load(crate::syntax::spec("rust").unwrap()).unwrap();
    let (_s, mut ed, file) = setup("chatlink-underlined", true, UNDERLINED);
    // Asked after the frame: the names only, once each
    let batch = underlines_asked(&ed);
    let words: Vec<&str> = batch.iter().map(|a| &*a.2).collect();
    assert_eq!(words, ["Total", "helper", "nothing_here"], "never a keyword, words or a file");
    assert!(underlined(&mut ed).is_empty(), "nothing underlined before it's known");
    let cid = batch[0].0;
    let progress = |kind: &str| json!({ "method": "$/progress", "params": { "token": "t", "value": { "kind": kind, "title": "Indexing" } } });
    ed.on_lsp_message(cid, progress("begin"));
    for (_, id, word) in &batch {
        let found = match word.as_str() {
            "helper" => symbols(&file, &[("helper", 0, 3, "Total")]),
            "Total" => symbols(&file, &[("Total", 0, 0, "")]),
            _ => symbols(&file, &[("nothing_here_too", 1, 0, "")]),
        };
        ed.on_lsp_message(cid, json!({ "id": id, "result": found }));
    }
    // (in prose only: the block's `helper` answers the keys unmarked)
    assert_eq!(underlined(&mut ed), ["helper", "Total", "helper"]);
    assert!(
        underlines_asked(&ed).is_empty(),
        "each asked once; the one the loading server didn't find waits"
    );
    ed.on_lsp_message(cid, progress("end"));
    let again = underlines_asked(&ed);
    assert_eq!(
        again.iter().map(|a| &*a.2).collect::<Vec<_>>(),
        ["nothing_here"],
        "asked again as its work ends"
    );
    ed.on_lsp_message(cid, json!({ "id": again[0].1, "result": [] }));
    frame(&mut ed);
    assert!(underlines_asked(&ed).is_empty(), "never asked again");
    assert_eq!(underlined(&mut ed), ["helper", "Total", "helper"], "`nothing_here` plain");
    // A press on an underlined name: `gd` there, asked by name
    press(&mut ed, "are found", "helper", false);
    assert_eq!(
        named(&ed).map(|q| (q.1, q.2, q.3)),
        Some(("helper".to_string(), Some("Total".to_string()), Some(Goto::Definition)))
    );
    // A keyword: said at once, nothing asked
    assert!(ed.workspace.is_at(crate::workspace::At::Chat), "the keys stay in the chat");
    // (in prose, then in the block: the cursor ends on the match's last character)
    for keys in ["gg/le<ret>gd", "gg/le<ret>ngd"] {
        ed.toasts.clear();
        feed(&mut ed, keys);
        let toasts: Vec<&str> = ed.toasts.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(toasts, ["`let` is a rust keyword — nothing to look up"], "{keys}");
        assert!(named(&ed).is_none() && asked(&ed).is_none(), "{keys}: nothing asked");
    }
}

/// A grammar's keywords are what its highlights query captures as one: a string's own capture, a group's, a predicate's
/// on a keyword capture — never punctuation, a macro's name or a name the query captures otherwise.
#[test]
fn a_grammars_keywords_are_its_highlights_querys() {
    let rust = crate::syntax::Loader::global().load(crate::syntax::spec("rust").unwrap()).unwrap();
    for kw in ["let", "fn", "for", "match", "struct", "use", "in", "while"] {
        assert!(rust.keywords.contains(kw), "{kw}");
    }
    for not in ["&", "macro_rules", "macro_rules!", "self", "helper", "Some"] {
        assert!(!rust.keywords.contains(not), "{not}");
    }
    assert_eq!(crate::syntax::keyword_in(["rust"], "fn").as_deref(), Some("rust"));
    assert_eq!(crate::syntax::keyword_in(["rust"], "helper"), None);
}

/// A qualified name's container: the name right before its `::` or `.`.
#[test]
fn a_qualified_name_gives_its_container() {
    assert_eq!(qualifier("`Thread::length`", 9).as_deref(), Some("Thread"));
    assert_eq!(qualifier("ui.accent", 3).as_deref(), Some("ui"));
    assert_eq!(qualifier("`length`", 1), None);
    assert_eq!(qualifier("a :: b", 5), None);
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
