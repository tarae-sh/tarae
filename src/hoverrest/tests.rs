//! The pointer's rest on every code view (#481): one scene per place code is drawn (`codeview::CodeIn` — a scene for
//! each, by a `match` that won't compile without one), every cell of every row it drew walked by the pointer. A row of
//! the buffer's own text rests on the very word drawn under the pointer; any other row never rests; and `space k` in the
//! same view keeps the same rule, so the mouse and the keys can't drift apart.

use crate::codeview::{CodeIn, TextRow, View};
use crate::editor::Editor;
use crate::event::Event;
use crate::key::{Code, Key};
use crate::lsp_editor::Kind;
use crate::scratch::Scratch;

const W: u16 = 100;
const H: u16 = 30;

/// An editor showing a code view, and the words it draws: those only in the buffer's own text (each rests somewhere)
/// and those only on rows that aren't (drawn, never resting).
struct Scene {
    name: &'static str,
    ed: Editor,
    _dir: Scratch,
    rests: &'static [&'static str],
    silent: &'static [&'static str],
}

/// Every place code is drawn (a pane's id stands for any pane's).
const EVERY: [CodeIn; 5] =
    [CodeIn::Pane(0), CodeIn::EditReview, CodeIn::Preview, CodeIn::Output, CodeIn::Memo];

/// The scenes of the views drawn at `at` — a new place won't compile until it has its own.
fn scenes(at: CodeIn) -> Vec<Scene> {
    match at {
        CodeIn::Pane(_) => vec![plain(), code_review(), shown()],
        CodeIn::EditReview => vec![edit_review()],
        CodeIn::Preview => vec![preview()],
        CodeIn::Output => vec![output()],
        CodeIn::Memo => vec![memo()],
    }
}

/// Whether `a` and `b` are the same place, any pane standing for any other.
fn same(a: CodeIn, b: CodeIn) -> bool {
    std::mem::discriminant(&a) == std::mem::discriminant(&b)
}

#[test]
fn every_code_view_rests_on_its_text_and_only_there() {
    for at in EVERY {
        for mut s in scenes(at) {
            let name = s.name;
            let rows = frame(&mut s.ed);
            // (every view drawn is one of the places walked)
            for (v, _) in s.ed.screen.code_views.borrow().iter() {
                assert!(EVERY.iter().any(|&e| same(e, v.at)), "{name}: {:?} drawn, no scene", v.at);
            }
            let (view, texts) = drawn(&s.ed, at).unwrap_or_else(|| panic!("{name}: no {at:?} drawn"));
            let (rested, seen) = walk(&mut s.ed, &rows, &view, &texts, at, name);
            for w in s.rests {
                assert!(
                    rested.iter().any(|r| r == w),
                    "{name}: {w} never rested — {rested:?}\n{}",
                    rows.join("\n")
                );
            }
            for w in s.silent {
                assert!(
                    seen.iter().any(|r| r == w),
                    "{name}: {w} not drawn on a row not text\n{}",
                    rows.join("\n")
                );
                assert!(!rested.iter().any(|r| r == w), "{name}: {w} rested");
            }
            if let Some(w) = s.rests.first() {
                card(&mut s.ed, &rows, &view, w, name);
            }
            keys(&mut s, at, &view);
        }
    }
}

/// Claude's edit review: the card over any name of its context keeps clear of the change being decided — its head and
/// rows — whenever a side of the name has the room clear of them (#481: under the name, it hid what you answer).
#[test]
fn a_rest_card_in_the_edit_review_keeps_clear_of_the_change() {
    let mut text: String = (0..10).map(|i| format!("fn above_{i}() {{}}\n")).collect();
    text.push_str("fn taken_word() {}\n");
    text.extend((0..8).map(|i| format!("fn below_{i}() {{}}\n")));
    let (mut ed, _dir) = open("rest-clear", &[("a.rs", &text)]);
    let rope = ropey::Rope::from_str(&text);
    let (from, to) = (rope.line_to_byte(10), rope.line_to_byte(11));
    let hunk = crate::llm::Hunk {
        from,
        to,
        old: rope.byte_slice(from..to).to_string(),
        new: "fn added_word() {}\n".into(),
        decision: crate::llm::Decision::Undecided,
        conflict: false,
    };
    let mut rev = crate::llm::Review::new(1, ed.doc().id, "one", vec![hunk]);
    (rev.ready, rev.follow, rev.focused) = (true, false, true);
    ed.workspace.claude.review = Some(rev);
    let rows = frame(&mut ed);
    let (view, texts) = drawn(&ed, CodeIn::EditReview).expect("the edit review drawn");
    let (a, b) = ed.screen.review_change.get().expect("the change on screen");
    let (top, bottom) = (view.rect.y, view.rect.y + view.rect.h);
    let (mut walked, mut above_only) = (0, 0);
    for (k, t) in texts.iter().enumerate() {
        let y = top + k;
        let Some(at) = rows[y].find("above_").or_else(|| rows[y].find("below_")) else { continue };
        if t.is_none() || (a..=b).contains(&y) {
            continue;
        }
        let x = rows[y][..at].chars().count();
        ed.handle_event(Event::Hover(x as u16, y as u16));
        crate::hoverrest::rested(&mut ed);
        let n = crate::hoverrest::resting(&ed).unwrap_or_else(|| panic!("no rest at {x},{y}"));
        crate::hoverrest::shows(&mut ed, n, "the docs\n\nmore of them\n\nand more", None);
        let shown = frame(&mut ed);
        let (_, y0, _, h) = ed.screen.rest_card.get().unwrap_or_else(|| panic!("no card at {y}"));
        // (the room clear of the change on each side, as `term::docs_popup` measures it: a row of air below)
        let up = if b < y { y - (b + 1).max(top) } else { y - top };
        let down = if a > y { (a - y - 1).min(bottom - y - 2) } else { bottom.saturating_sub(y + 2) };
        if up >= h || down >= h {
            assert!(
                y0 + h <= a || y0 > b,
                "the card at {y0}+{h} over the change {a}..={b}, resting on row {y}\n{}",
                shown.join("\n")
            );
        }
        above_only += usize::from(up >= h && down < h);
        walked += 1;
        ed.hover_rest.rest = None;
    }
    assert_eq!(walked, 18, "every name of the context walked\n{}", rows.join("\n"));
    assert!(above_only > 0, "no name with room only above the change\n{}", rows.join("\n"));
}

/// The view drawn at `at` (any pane for a pane) and its rows, copied out of the frame's record.
fn drawn(ed: &Editor, at: CodeIn) -> Option<(View, Vec<TextRow>)> {
    let views = ed.screen.code_views.borrow();
    let (v, rows) = views.iter().filter(|(v, _)| same(v.at, at)).last()?;
    Some((v.clone(), rows.to_vec()))
}

/// Every cell of the view's rows under the pointer: on the buffer's own text a rest on the word drawn there (on a
/// letter of one), else none — a run's output never. The words rested on, and those drawn on rows that aren't text.
fn walk(
    ed: &mut Editor,
    rows: &[String],
    view: &View,
    texts: &[TextRow],
    at: CodeIn,
    name: &str,
) -> (Vec<String>, Vec<String>) {
    let (mut rested, mut seen) = (Vec::new(), Vec::new());
    let r = view.rect;
    for (k, text) in texts.iter().enumerate() {
        let y = r.y + k;
        let cells: Vec<char> = rows[y].chars().collect();
        for x in r.x + view.gutter..(r.x + r.w).min(cells.len()) {
            ed.handle_event(Event::Hover(x as u16, y as u16));
            let word = word_at(&cells, x);
            let rest = ed.hover_rest.rest.as_ref().map(|r| {
                let d = crate::hoverrest::doc_of(ed, &r.spot);
                (r.spot.pane.clone(), d.map(|d| d.text.byte_slice(r.spot.word.clone()).to_string()))
            });
            match (text, word, at) {
                (Some(_), Some(w), CodeIn::Pane(_) | CodeIn::EditReview | CodeIn::Preview) => {
                    let Some((pane, got)) = rest else { panic!("{name}: no rest on {w} at {x},{y}") };
                    assert!(matches!(pane, crate::hoverrest::In::Code(a) if same(a, at)), "{name}: {pane:?}");
                    assert_eq!(got.as_deref(), Some(w.as_str()), "{name}: the word drawn at {x},{y}");
                    rested.push(w);
                }
                (None, Some(w), _) => {
                    assert!(rest.is_none(), "{name}: {rest:?} rests on {w} at {x},{y}, a row not text");
                    seen.push(w);
                }
                _ => assert!(rest.is_none(), "{name}: {rest:?} rests at {x},{y} (`{}`)", cells[x]),
            }
        }
    }
    // (the pointer gone, nothing rests for what follows)
    ed.handle_event(Event::Hover(0, H - 1));
    ed.hover_rest.rest = None;
    (rested, seen)
}

/// The word whose letter is at `x` of a row as drawn, if one.
fn word_at(cells: &[char], x: usize) -> Option<String> {
    let is = |c: &char| crate::completion::is_word(*c);
    if !is(cells.get(x)?) {
        return None;
    }
    let from = (0..x).rev().take_while(|&i| is(&cells[i])).last().unwrap_or(x);
    let to = (x..cells.len()).take_while(|&i| is(&cells[i])).last().unwrap_or(x);
    Some(cells[from..=to].iter().collect())
}

/// The card by the real path: the pointer resting on `word`, its answer stubbed, drawn in the frame — over the picker
/// for a preview's.
fn card(ed: &mut Editor, rows: &[String], view: &View, word: &str, name: &str) {
    let r = view.rect;
    let y = (r.y..r.y + r.h).find(|&y| rows[y].contains(word)).expect(word);
    let x = rows[y][..rows[y].find(word).unwrap()].chars().count() + 1;
    ed.handle_event(Event::Hover(x as u16, y as u16));
    crate::hoverrest::rested(ed);
    let n = crate::hoverrest::resting(ed).unwrap_or_else(|| panic!("{name}: no rest on {word}"));
    crate::hoverrest::shows(ed, n, &format!("the docs of {word}"), None);
    let shown = frame(ed);
    assert!(ed.screen.rest_card.get().is_some(), "{name}: no card\n{}", shown.join("\n"));
    let says = format!("the docs of {word}");
    assert!(shown.iter().any(|l| l.contains(&says)), "{name}: the card unseen\n{}", shown.join("\n"));
    ed.hover_rest.rest = None;
}

/// `space k` in the view: at the cursor where it's on the buffer's own text (a pane's, always — a cursor is a place in
/// the buffer, its line a row of text there), said where the view has no cursor of its own, the picker's typing in a
/// preview.
fn keys(s: &mut Scene, at: CodeIn, view: &View) {
    let (name, ed) = (s.name, &mut s.ed);
    serve(ed);
    match at {
        CodeIn::Pane(_) => {
            let d = ed.doc();
            let head = d.selection().primary().cursor(&d.text);
            let line = crate::movement::line_of(&d.text, head);
            let (_, texts) = drawn(ed, at).unwrap();
            assert!(
                texts.iter().flatten().any(|t| t.0 == line),
                "{name}: the cursor's line not a row of text"
            );
            feed(ed, " k");
            let asked = ed.lsp.pending.values().find(|p| matches!(p.kind, Kind::Hover));
            let p = asked.unwrap_or_else(|| panic!("{name}: space k asked nothing — {:?}", ed.status));
            assert_eq!(p.at(), (view.doc, head), "{name}: asked at the cursor");
        }
        CodeIn::EditReview | CodeIn::Output => {
            assert_ne!(ed.keys_to().region, crate::uistate::Region::Code, "{name}: its keys");
            feed(ed, " k");
            let asked = ed.lsp.pending.values().any(|p| matches!(p.kind, Kind::Hover));
            assert!(!asked, "{name}: space k asked at a cursor not in sight");
            assert!(ed.status.is_some(), "{name}: space k said nothing");
        }
        // (the memo's insert mode: typed, never asked)
        CodeIn::Memo => {
            assert_eq!(ed.keys_to().region, crate::uistate::Region::Memo, "{name}: the memo's keys");
            feed(ed, " k");
            let asked = ed.lsp.pending.values().any(|p| matches!(p.kind, Kind::Hover));
            assert!(!asked, "{name}: space k asked");
            let text = ed.workspace.memo.as_ref().map(crate::memo::Memo::text).unwrap_or_default();
            assert!(text.ends_with(" k"), "{name}: typed — {text:?}");
        }
        CodeIn::Preview => {
            assert_eq!(ed.keys_to().region, crate::uistate::Region::Picker, "{name}: the picker's keys");
            feed(ed, " k");
            let asked = ed.lsp.pending.values().any(|p| matches!(p.kind, Kind::Hover));
            assert!(!asked, "{name}: space k asked");
            assert!(ed.picker.as_ref().is_some_and(|p| p.query().ends_with(" k")), "{name}: typed");
        }
    }
}

/// A stand-in server with hover on every file shown, ready.
fn serve(ed: &mut Editor) {
    static TRAITS: std::sync::LazyLock<crate::lsp::ServerTraits> =
        std::sync::LazyLock::new(crate::lsp::ServerTraits::default);
    let serve: crate::lsp::Serve = Box::new(|method, _| match method {
        "initialize" => Some(
            serde_json::json!({ "capabilities": { "positionEncoding": "utf-8", "hoverProvider": true } }),
        ),
        _ => None,
    });
    let cid = ed.lsp_own("stand-in", &TRAITS, serve);
    for d in ed.docs.iter_mut().filter(|d| d.path.is_some()) {
        d.lsp.attach(cid, false, true);
    }
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    while !ed.client(cid).is_some_and(|c| c.ready) {
        let left = end.saturating_duration_since(std::time::Instant::now());
        let ev = ed.events.recv_timeout(left).expect("the stand-in ready");
        ed.handle_event(ev);
    }
}

fn frame(ed: &mut Editor) -> Vec<String> {
    let mut buf = Vec::new();
    crate::term::render(ed, &mut buf, W, H).unwrap();
    crate::screen::text(&buf, W, H).unwrap().lines().map(str::to_string).collect()
}

/// Keys as typed: plain characters, `<name>` for the rest (`<C-w>`), a frame after each.
fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_event(Event::Key(key));
        frame(ed);
    }
}

/// An editor on `files` (name, text) in a folder of its own, the first in front.
fn open(name: &'static str, files: &[(&str, &str)]) -> (Editor, Scratch) {
    let dir = Scratch::new(name);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = std::fs::canonicalize(dir.as_ref()).unwrap();
    for (file, text) in files.iter().rev() {
        let path = dir.join(file);
        std::fs::write(&path, text).unwrap();
        ed.open(&path).unwrap();
    }
    (ed, dir)
}

fn plain() -> Scene {
    let (ed, _dir) = open("rest-plain", &[("a.txt", "fn plain_word() {}\nlet other_word = 1;\n")]);
    Scene { name: "a pane", ed, _dir, rests: &["plain_word", "other_word"], silent: &[] }
}

/// `space v`'s and `:diff`'s view: the base's removed line a row of its own above the line it went before.
fn code_review() -> Scene {
    let (base, now) = (
        "fn kept_word() {}\nfn removed_word() {}\nfn below_word() {}\n",
        "fn kept_word() {}\nfn below_word() {}\n",
    );
    let (mut ed, _dir) = open("rest-review", &[("a.txt", now)]);
    let d = ed.doc_mut();
    d.review_base = Some(ropey::Rope::from_str(base));
    d.git_base = Some(std::sync::Arc::new(base.to_string()));
    d.git_hunks = crate::git::hunks(base, now);
    d.git_version = d.version();
    Scene {
        name: "the code review",
        ed,
        _dir,
        rests: &["kept_word", "below_word"],
        silent: &["removed_word"],
    }
}

/// An edit follow shows: the line it replaced a row above its own.
fn shown() -> Scene {
    let (mut ed, _dir) = open("rest-shown", &[("a.txt", "fn first_word() {}\nfn shown_new() {}\n")]);
    let edit = crate::replay::Hunk {
        lines: 1..2,
        removed: vec!["fn shown_gone() {}".into()],
        added: vec!["fn shown_new() {}".into()],
    };
    ed.doc_mut().shown = Some(vec![edit]);
    Scene { name: "an edit shown", ed, _dir, rests: &["first_word", "shown_new"], silent: &["shown_gone"] }
}

/// Claude's edit review: the buffer holds the old text until you accept — its context, kept and taken-out lines are the
/// buffer's, the lines it proposes aren't.
fn edit_review() -> Scene {
    let text = "fn context_word() {}\nfn taken_word() {}\nfn kept_word() {}\nfn after_word() {}\n";
    let (mut ed, _dir) = open("rest-edit", &[("a.rs", text)]);
    let rope = ropey::Rope::from_str(text);
    let (from, to) = (rope.line_to_byte(1), rope.line_to_byte(3));
    let hunk = crate::llm::Hunk {
        from,
        to,
        old: rope.byte_slice(from..to).to_string(),
        new: "fn added_word() {}\nfn kept_word() {}\n".into(),
        decision: crate::llm::Decision::Undecided,
        conflict: false,
    };
    let mut rev = crate::llm::Review::new(1, ed.doc().id, "two", vec![hunk]);
    (rev.ready, rev.follow, rev.focused) = (true, false, true);
    ed.workspace.claude.review = Some(rev);
    let rests = &["context_word", "taken_word", "kept_word", "after_word"];
    Scene { name: "the edit review", ed, _dir, rests, silent: &["added_word"] }
}

/// A picker's preview of a buffer (both are this text: whichever it shows).
fn preview() -> Scene {
    let text = "fn preview_word() {}\n";
    let (mut ed, _dir) = open("rest-preview", &[("a.txt", text), ("b.txt", text)]);
    feed(&mut ed, " b");
    Scene { name: "a picker's preview", ed, _dir, rests: &["preview_word"], silent: &[] }
}

/// A run panel's output, its keys: a run's words, never a file's code.
fn output() -> Scene {
    let (mut ed, _dir) = open("rest-output", &[("a.txt", "fn code_word() {}\n")]);
    let lines = (0..3).map(|i| format!("output_word {i}")).collect();
    ed.workspace.test_run = Some(crate::testing::TestRun::finished("t", Vec::new(), lines));
    ed.workspace.viewer.slot_front = Some(crate::sizes::Slot::Test);
    frame(&mut ed);
    feed(&mut ed, "<C-w>j");
    Scene { name: "a run's output", ed, _dir, rests: &[], silent: &[] }
}

/// The memo, its keys: your words, never a file's code.
fn memo() -> Scene {
    let (mut ed, _dir) = open("rest-memo", &[("a.txt", "fn code_word() {}\n")]);
    feed(&mut ed, " Mmemo_word here");
    Scene { name: "the memo", ed, _dir, rests: &[], silent: &[] }
}
