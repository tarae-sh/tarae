//! A name in the chat asked where it stands (#389 steps 4 and 5, `chatlink.rs`), simulated: an answer citing a file's
//! line, quoting its lines in a block, naming one in prose on the cited lines and one no cited place holds; frames drawn,
//! its blocks mapped on a worker; the file's buffer edited (a line added or taken at its top — its lines move); the
//! transcript's cursor put on a name — the quoted one, the prose ones, the place, a word that isn't code — as a key
//! would; `gd` and `space k` there; time passing — a stand-in server (`Client::own`, its answers jobs among the steps)
//! answering each question with where it was asked, and a name by name with where the file had it at first (its item's
//! start, a line that may have moved since). After every step: a question asked only at its name in the file as the
//! buffer is then — never at a guess (a line moved since its map, or since the server's place, is said, not asked); a
//! question by name only about the name under the transcript's cursor; a hover card in the chat only for the newest
//! question, while the transcript's cursor is where it was asked, saying the place it was asked of.

use std::cell::RefCell;
use std::time::Duration;

use serde_json::json;

use super::{Choices, Sim};
use crate::chat::{Chat, Msg, Role};
use crate::editor::Editor;
use crate::key::{Code, Key};

const NAME: &str = "a_name_in_the_chat_is_asked_only_where_it_stands";

/// (16: a hover asked, the cursor put elsewhere, then its answer lands — never shown)
const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5, 16];

const INPUTS: usize = 60;

const LIMIT: Duration = crate::test_results::PATIENCE;

const TEXT: &str = "fn helper(a: u32) -> u32 {\n    a + 1\n}\n\nfn main() {\n    let total = helper(2);\n    println!(\"{total}\");\n}\n";

const ANSWER: &str = "It sums at src/main.rs:6, `total` kept:\n\n```rust\n    let total = helper(2);\n```\n\nNothing else but `helper` alone.";

/// Where the transcript's cursor is put: what it finds there (a code span drawn without its backticks).
const AT: &[&str] = &["helper(2)", "total", "main.rs:6", "Nothing", "helper alone"];

#[test]
fn a_name_in_the_chat_is_asked_only_where_it_stands() {
    super::seeds(NAME, SEEDS, run);
}

#[derive(Default)]
struct World {
    /// A line added at the file's top.
    added: bool,
    /// The questions seen asked: their numbers.
    seen: Vec<u64>,
    /// The newest: whether it asks the hover, the place asked of (`line:character`), the transcript's cursor then.
    last: Option<(bool, String, usize)>,
    /// The questions by name seen asked: their numbers.
    named: Vec<u64>,
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new(NAME, choices, LIMIT);
    sim.frames = Some((120, 30));
    let dir = crate::scratch::Scratch::new("sim-chatlink");
    let root = std::fs::canonicalize(dir.as_ref()).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    let file = root.join("src/main.rs");
    std::fs::write(&file, TEXT).unwrap();
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    ed.open(&file).unwrap();
    // (a stand-in that answers each question with the place it was asked of)
    static TRAITS: std::sync::LazyLock<crate::lsp::ServerTraits> =
        std::sync::LazyLock::new(crate::lsp::ServerTraits::default);
    let file_uri = file.clone();
    let serve: crate::lsp::Serve = Box::new(move |method, params| match method {
        "initialize" => Some(json!({ "capabilities": { "positionEncoding": "utf-8", "hoverProvider": true,
            "definitionProvider": true, "workspaceSymbolProvider": true } })),
        // (where the file had it at first, its item's start — and a close name, never gone to)
        "workspace/symbol" => {
            let at = |name: &str, line: u32| {
                json!({ "name": name, "kind": 12, "location": { "uri": crate::lsp::uri(&file_uri),
                    "range": { "start": { "line": line, "character": 0 }, "end": { "line": line, "character": 0 } } } })
            };
            Some(json!([at("helper", 0), at("helpers", 4)]))
        }
        "textDocument/hover" => {
            let p = &params["position"];
            Some(
                json!({ "contents": { "kind": "markdown", "value": format!("asked at {}:{}", p["line"], p["character"]) } }),
            )
        }
        "textDocument/definition" => {
            let p = params["position"].clone();
            Some(json!([{ "uri": params["textDocument"]["uri"], "range": { "start": p, "end": p } }]))
        }
        _ => None,
    });
    let cid = ed.lsp_own("stand-in", &TRAITS, serve);
    let id = ed.doc().id;
    if let Some(d) = ed.docs.iter_mut().find(|d| d.id == id) {
        d.lsp.attach(cid, false, true);
    }
    let mut c = Chat::new();
    c.msgs = vec![Msg::new(Role::User, "where?", ""), Msg::new(Role::Assistant, ANSWER, "")];
    ed.workspace.claude.chat = Some(c);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    crate::term::render(&mut ed, &mut Vec::new(), 120, 30).unwrap();
    // (its view mode: `gd` and `space k` act on its cursor)
    ed.handle_key(Key::plain(Code::Esc));
    let world = RefCell::new(World::default());
    let check = |ed: &Editor| held(ed, id, &mut world.borrow_mut());
    sim.walk(&mut ed, INPUTS, |sim, ed| step(sim, ed, id, &mut world.borrow_mut()), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn keys(ed: &mut Editor, keys: &str) {
    for c in keys.chars() {
        ed.handle_key(Key::plain(Code::Char(c)));
    }
}

fn step(sim: &mut Sim, ed: &mut Editor, file: crate::document::DocId, w: &mut World) -> String {
    let c = &mut sim.choices;
    // (by arm: a frame 2, the file edited 2, the cursor put 3, `gd` 2, `space k` 2, time 2)
    match c.weighted(&[2, 2, 3, 2, 2, 2]) {
        0 => {
            crate::term::render(ed, &mut Vec::new(), 120, 30).unwrap();
            "a frame".into()
        }
        1 => {
            let Some(d) = ed.docs.iter_mut().find(|d| d.id == file) else { return "(no file)".into() };
            let tx = match w.added {
                false => crate::transaction::Transaction::new(vec![crate::transaction::Change::insert(
                    0, "// x\n",
                )]),
                true => crate::transaction::Transaction::new(vec![crate::transaction::Change::delete(0, 5)]),
            };
            d.apply(&tx);
            w.added = !w.added;
            format!("a line {} at the file's top", if w.added { "added" } else { "taken" })
        }
        2 => {
            let at = *c.pick(AT);
            // (as a key moves it: what floats closes)
            (ed.popup, ed.popup_chat) = (None, false);
            let Some(d) = crate::chatdoc::doc_mut(ed) else { return "(no transcript)".into() };
            let Some(k) = d.text.to_string().find(at) else { return format!("({at} not laid out yet)") };
            d.set_selection(crate::selection::Selection::point(k + 1));
            format!("the cursor on {at}")
        }
        3 => {
            keys(ed, "gd");
            "gd".into()
        }
        4 => {
            keys(ed, " k");
            "space k".into()
        }
        _ => {
            let ms = *c.pick(&[1, 30, 300]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    }
}

/// The rules, after every step.
fn held(ed: &Editor, file: crate::document::DocId, w: &mut World) -> Result<(), String> {
    // A question newly asked: at its name in the file as the buffer is now
    if let Some((n, doc, byte, word, head, goto)) = crate::chatlink::asked(ed)
        && !w.seen.contains(&n)
    {
        w.seen.push(n);
        let Some(d) = ed.docs.iter().find(|d| d.id == doc).filter(|_| doc == file) else {
            return Err(format!("question {n} asked of another document"));
        };
        let there = d.text.get_byte_slice(byte..byte + word.len()).map(|s| s.to_string());
        if there.as_deref() != Some(word.as_str()) {
            return Err(format!("question {n} about `{word}` asked at byte {byte}, where {there:?} is"));
        }
        let line = d.text.byte_to_line(byte);
        w.last =
            Some((goto.is_none(), format!("asked at {line}:{}", byte - d.text.line_to_byte(line)), head));
    }
    // A question by name newly asked: about the name under the transcript's cursor, no cited place holding it
    if let Some((n, word, _, _)) = crate::chatlink::named(ed)
        && !w.named.contains(&n)
    {
        w.named.push(n);
        // (the word at the cursor, read here from the transcript's text)
        let t = crate::chatdoc::transcript(ed).and_then(|t| t.pane.doc_in());
        let there = t.map(|d| {
            let (text, at) = (d.text.to_string(), d.selection().primary().cursor(&d.text));
            let w = |c: char| c.is_alphanumeric() || c == '_';
            let start = text[..at].rfind(|c| !w(c)).map_or(0, |i| i + 1);
            let end = text[at..].find(|c| !w(c)).map_or(text.len(), |i| at + i);
            text[start..end].to_string()
        });
        if there.as_deref() != Some(word.as_str()) {
            return Err(format!("question {n} by name about `{word}`, the cursor on {there:?}"));
        }
    }
    // A hover card in the chat: the newest question's, a hover, the cursor where it was asked, saying where that was
    if ed.popup_chat && ed.popup.is_some() {
        let head = ed
            .workspace
            .claude
            .chat
            .as_ref()
            .and_then(|c| c.transcript.as_ref())
            .and_then(|t| t.pane.doc_in());
        let head = head.map(|d| d.selection().primary().cursor(&d.text));
        let card = format!("{:?}", ed.popup);
        match &w.last {
            // (a cited place's peek: the file's own lines, no question)
            _ if !card.contains("asked at") => {}
            Some((true, said, at)) if card.contains(said.as_str()) && head == Some(*at) => {}
            last => {
                return Err(format!(
                    "a card in the chat ({card:.80}) for the newest question {last:?}, the cursor at {head:?}"
                ));
            }
        }
    }
    Ok(())
}

/// What a trace line ends with: the question in flight, the card in the chat.
fn print(ed: &Editor) -> String {
    let asked = crate::chatlink::asked(ed).map(|a| format!("asked {} `{}`@{}", a.0, a.3, a.2));
    let named = crate::chatlink::named(ed).map(|q| format!("by name {} `{}`", q.0, q.1));
    let card = (ed.popup_chat && ed.popup.is_some()).then(|| format!("{:?}", ed.popup).contains("asked at"));
    format!("{asked:?} · {named:?} · card {card:?}")
}
