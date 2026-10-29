//! The wheel audit (2026-10-28, by hand): on each surface the wheel down, up, sideways and past each end, and what moved
//! besides what's under the pointer — the keys, where they are, the focus, the code's cursor and selection, the state's
//! other fields — and whether a wheel with nothing left is silent. It prints its table; nothing is held to it yet.
//! `cargo test --bin tarae wheel_audit -- --ignored --nocapture`

use crate::editor::Editor;
use crate::event::{Mouse, MouseKind as K};
use crate::uistate::tests::{chat, editor, frame};
use crate::workspace::At;

const W: u16 = 120;
const H: u16 = 40;

fn shown(ed: &mut Editor) -> String {
    frame(ed);
    crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), W, H).unwrap()
}

fn keys(ed: &mut Editor, keys: &[&str]) {
    for k in keys {
        let key = match k.chars().count() {
            1 => crate::key::Key::plain(crate::key::Code::Char(k.chars().next().unwrap())),
            _ => k.trim_matches(['<', '>']).parse().unwrap(),
        };
        ed.handle_key(key);
        frame(ed);
    }
}

fn long_text(ed: &mut Editor) {
    let line =
        |i| format!("let line_{i} = {i}; // {}\n", "a long comment that runs past the pane ".repeat(4));
    ed.docs[0].text = ropey::Rope::from_str(&(0..200).map(line).collect::<String>());
}

/// What a wheel could change.
#[derive(Clone)]
struct Look {
    text: Vec<String>,
    keys: String,
    at: String,
    focus: String,
    doc: String,
    selection: String,
    views: String,
    state: serde_json::Value,
    said: usize,
}

fn look(ed: &mut Editor) -> Look {
    let text = shown(ed).lines().map(String::from).collect();
    let mut state = ed.ui_state().json();
    // (what moves on its own between two looks)
    for k in ["frame", "input", "pid", "now"] {
        state.as_object_mut().map(|o| o.remove(k));
    }
    Look {
        text,
        keys: format!("{:?}", ed.keys_to().region),
        at: format!("{:?}", ed.workspace.viewer.at),
        focus: format!("{:?}", ed.workspace.viewer.focus),
        doc: format!("{:?}", ed.doc().id),
        selection: format!("{:?}", ed.doc().selection()),
        views: format!(
            "{}:{} {:?}",
            ed.doc().top,
            ed.doc().left,
            ed.workspace.views.iter().map(|v| (v.top, v.left)).collect::<Vec<_>>()
        ),
        state,
        said: ed.messages.len(),
    }
}

/// The paths of the leaves that differ between two values (`panels.3.selected`), at most `max`.
fn paths(a: &serde_json::Value, b: &serde_json::Value, at: String, out: &mut Vec<String>) {
    use serde_json::Value as V;
    match (a, b) {
        (V::Object(x), V::Object(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                let (p, q) = (x.get(k).unwrap_or(&V::Null), y.get(k).unwrap_or(&V::Null));
                paths(p, q, if at.is_empty() { k.clone() } else { format!("{at}.{k}") }, out);
            }
        }
        (V::Array(x), V::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                paths(p, q, format!("{at}.{i}"), out);
            }
        }
        _ if a != b => out.push(at),
        _ => {}
    }
}

/// What changed from `a` to `b`, in words: the rows of the frame, then everything else that moved.
fn changed(a: &Look, b: &Look) -> String {
    let rows: Vec<usize> =
        (0..a.text.len().max(b.text.len())).filter(|&r| a.text.get(r) != b.text.get(r)).collect();
    let mut out = Vec::new();
    match (rows.first(), rows.last()) {
        (Some(f), Some(l)) => out.push(format!("rows {f}–{l} ({})", rows.len())),
        _ => out.push("frame same".into()),
    }
    for (what, x, y) in [
        ("KEYS", &a.keys, &b.keys),
        ("AT", &a.at, &b.at),
        ("FOCUS", &a.focus, &b.focus),
        ("DOC", &a.doc, &b.doc),
        ("SELECTION", &a.selection, &b.selection),
        ("view", &a.views, &b.views),
    ] {
        if x != y {
            out.push(format!("{what} {x} → {y}"));
        }
    }
    let mut p = Vec::new();
    paths(&a.state, &b.state, String::new(), &mut p);
    p.retain(|p| !p.starts_with("frame") && !p.contains("usage"));
    if !p.is_empty() {
        let n = p.len();
        p.truncate(6);
        out.push(format!(
            "state: {}{}",
            p.join(", "),
            if n > 6 { format!(" +{}", n - 6) } else { String::new() }
        ));
    }
    if b.said > a.said {
        out.push("SAID".into());
    }
    // (`TARAE_WHEEL_ROWS=1`: the first rows that differ, before and after)
    if std::env::var_os("TARAE_WHEEL_ROWS").is_some() {
        for &r in rows.iter().take(3) {
            let (x, y) =
                (a.text.get(r).map_or("", |s| s.trim_end()), b.text.get(r).map_or("", |s| s.trim_end()));
            out.push(format!("\n      {r:>2} - {x}\n      {r:>2} + {y}"));
        }
    }
    out.join(" · ")
}

fn wheel(ed: &mut Editor, kind: K, (x, y): (usize, usize), n: usize) {
    for _ in 0..n {
        ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
        frame(ed);
    }
}

/// `text`'s first cell on the frame, `dx` columns on.
fn find(ed: &mut Editor, text: &str, dx: usize) -> Option<(usize, usize)> {
    let shown = shown(ed);
    shown.lines().enumerate().find_map(|(y, row)| row.find(text).map(|i| (row[..i].chars().count() + dx, y)))
}

type Case = (&'static str, fn(&mut Editor), &'static str);

/// The surfaces: a name, its scene, the text the pointer rests on.
const CASES: &[Case] = &[
    ("code, keys in the code", |ed| long_text(ed), "line_5 "),
    (
        "code, no wrapping",
        |ed| {
            ed.config.soft_wrap = "never".into();
            long_text(ed);
        },
        "line_5 ",
    ),
    (
        "code, no wrapping, a split not focused",
        |ed| {
            ed.config.soft_wrap = "never".into();
            long_text(ed);
            keys(ed, &["<C-w>", "v"]);
        },
        "line_5 ",
    ),
    (
        "code, a split not focused",
        |ed| {
            long_text(ed);
            keys(ed, &["<C-w>", "v"]);
        },
        "line_5 ",
    ),
    (
        "code, keys in the side panel",
        |ed| {
            long_text(ed);
            let files = (0..80).map(|i| format!("f{i:02}.rs")).collect();
            ed.workspace.viewer.sidebar =
                Some(crate::sidebar::Sidebar::with_files(std::env::temp_dir(), files));
            ed.workspace.viewer.at = At::Side;
        },
        "line_5 ",
    ),
    ("chat transcript, keys in its input", |ed| conversation(ed), "answer 25"),
    (
        "chat transcript, keys in the code",
        |ed| {
            long_text(ed);
            conversation(ed);
            ed.workspace.viewer.at = At::Code;
        },
        "answer 25",
    ),
    (
        "chat transcript, its view mode",
        |ed| {
            conversation(ed);
            frame(ed);
            keys(ed, &["<esc>"]);
        },
        "answer 25",
    ),
    ("side panel, keys in the code", |ed| side(ed, At::Code), "f05.rs"),
    ("side panel, its keys", |ed| side(ed, At::Side), "f05.rs"),
    ("review rail, keys in the code", |ed| rail(ed, false), "f05.rs"),
    ("review rail, its keys", |ed| rail(ed, true), "f05.rs"),
    ("review rail, its keys, files on disk", |ed| rail_on_disk(ed), "f05.rs"),
    ("test output, keys in the code", |ed| output(ed, false), "output line 195"),
    ("test output, its keys", |ed| output(ed, true), "output line 195"),
    ("terminal, its program's keys", |ed| terminal(ed, None), "line 290"),
    ("terminal, its view mode", |ed| terminal(ed, Some(&["<C-\\>"])), "line 290"),
    ("terminal, keys in the code", |ed| terminal(ed, Some(&["<C-\\>", "<C-\\>"])), "line 290"),
    (":messages, its keys", |ed| messages(ed, false), "message 5 "),
    (":messages, keys given back", |ed| messages(ed, true), "message 5 "),
    ("session list, its keys", |ed| keys(ed, &[" ", "W"]), "│▎"),
    ("session list, keys given back", |ed| keys(ed, &[" ", "W", "<esc>"]), "│▎"),
    ("picker, its list", |ed| picker(ed), "row 3"),
    ("picker, its preview", |ed| picker(ed), "line_4 "),
    ("a card cut to fit", |ed| card(ed), "line 3 of what"),
    (
        "a card's v buffer",
        |ed| {
            card(ed);
            keys(ed, &["<C-q>", "v"]);
        },
        "line 5 of what",
    ),
    (
        "hover text",
        |ed| {
            long_text(ed);
            let md: String = (0..80).map(|i| format!("line {i} of the docs\n\n")).collect();
            ed.popup = Some(crate::markdown::render(&md, &ed.theme, None));
        },
        "line 3 of the docs",
    ),
];

fn conversation(ed: &mut Editor) {
    chat(ed, "");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    for i in 0..30 {
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, format!("question {i}"), ""));
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Assistant, format!("answer {i}"), ""));
    }
}

fn side(ed: &mut Editor, at: At) {
    long_text(ed);
    let files = (0..80).map(|i| format!("f{i:02}.rs")).collect();
    ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::with_files(std::env::temp_dir(), files));
    ed.workspace.viewer.at = at;
}

fn rail(ed: &mut Editor, focused: bool) {
    use crate::codereview::{Base, CodeReview, File};
    long_text(ed);
    let root = ed.workspace.root.clone();
    let file = |i: usize| File {
        rel: format!("src/f{i:02}.rs"),
        path: root.join(format!("src/f{i:02}.rs")),
        added: 1,
        removed: 0,
        new: false,
        gone: true,
        binary: false,
    };
    ed.workspace.code_review =
        Some(CodeReview::for_test(Base::Head, root.clone(), (0..80).map(file).collect()));
    frame(ed);
    if focused {
        crate::codereview::rail_focus(ed);
    }
}

thread_local! {
    /// A case's scratch folder, handed from its scene to the case (`wheel_audit` takes it after the scene and drops it
    /// as the case ends) — a scene is a plain `fn`.
    static KEPT: std::cell::RefCell<Option<crate::scratch::Scratch>> = const { std::cell::RefCell::new(None) };
}

/// The rail's 80 files on disk, in a scratch folder kept for the case (each shown in the preview slot as it's selected).
fn rail_on_disk(ed: &mut Editor) {
    use crate::codereview::{Base, CodeReview, File};
    let scratch = crate::scratch::Scratch::new("wheel-audit-rail");
    let root = scratch.path().to_path_buf();
    KEPT.set(Some(scratch));
    std::fs::create_dir_all(root.join("src")).unwrap();
    let file = |i: usize| {
        let rel = format!("src/f{i:02}.rs");
        std::fs::write(root.join(&rel), format!("fn f{i}() {{}}\n")).unwrap();
        File {
            rel: rel.clone(),
            path: root.join(rel),
            added: 1,
            removed: 0,
            new: false,
            gone: false,
            binary: false,
        }
    };
    let files = (0..80).map(file).collect();
    ed.workspace.root = root.clone();
    ed.docs[0].path.set(Some(root.join("notes.txt")));
    ed.workspace.code_review = Some(CodeReview::for_test(Base::Head, root, files));
    crate::codereview::show(ed, 0, false);
    frame(ed);
    crate::codereview::rail_focus(ed);
}

fn output(ed: &mut Editor, focused: bool) {
    long_text(ed);
    let lines = (0..200).map(|i| format!("output line {i}")).collect();
    ed.workspace.test_run = Some(crate::testing::TestRun::finished("t", Vec::new(), lines));
    frame(ed);
    if focused {
        ed.bottom_take_keys();
    }
}

fn terminal(ed: &mut Editor, then: Option<&[&str]>) {
    long_text(ed);
    ed.terminal_run(
        vec!["/bin/sh".into(), "-c".into(), "seq -f 'line %g' 1 300; sleep 30".into()],
        Vec::new(),
    );
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    loop {
        if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
            ed.handle_event(ev);
        }
        if let Some(t) = ed.workspace.terminal.as_mut() {
            t.visible = true;
            ed.workspace.viewer.slot_front = Some(crate::sizes::Slot::Terminal);
            ed.workspace.viewer.at = At::Program;
            if shown(ed).contains("line 300") {
                break;
            }
        }
        assert!(std::time::Instant::now() < end, "the terminal's output came");
    }
    if let Some(k) = then {
        keys(ed, k);
    }
}

fn messages(ed: &mut Editor, back: bool) {
    for i in 0..60 {
        ed.set_status(format!("message {i}"));
    }
    ed.messages_open(None);
    frame(ed);
    if back {
        keys(ed, &["<esc>"]);
    }
}

fn picker(ed: &mut Editor) {
    long_text(ed);
    let id = ed.doc().id;
    let items = (0..80)
        .map(|i| crate::picker::Item {
            label: format!("row {i}"),
            action: crate::picker::Action::Buffer(id),
            hint: String::new(),
            glyph: None,
        })
        .collect();
    ed.open_picker(crate::picker::Picker::of("buffers", "buffers", items, false), None);
}

fn card(ed: &mut Editor) {
    long_text(ed);
    let body: Vec<String> = (0..80).map(|i| format!("line {i} of what it asks")).collect();
    let what = crate::offer::What::ReviewAtEnd;
    ed.push_offer(crate::offer::Offer::new("A long card".into(), &body.join("\n"), "", String::new(), what));
}

/// #215 by hand: four buffers, the chat open, the keys in the code (or the chat) — each buffer name at the code pane's top
/// line's right end: its spot, what `mouse_under` answers at its cells, and whether a press switches to it.
#[test]
#[ignore = "by hand: #215's buffer names (2026-10-28)"]
fn buffer_names_audit() {
    for (w, h) in [(140u16, 40u16), (100, 40)] {
        for (keys_in, selection, chat_open, sessions, following) in [
            ("code", false, true, 1, false),
            ("code", true, true, 1, false),
            ("chat", false, true, 1, false),
            ("code", false, false, 1, false),
            ("code", true, true, 17, false),
            ("chat", false, true, 17, false),
            ("code", true, true, 17, true),
            ("code", false, true, 1, true),
        ] {
            let mut ed = editor();
            let root = ed.workspace.root.clone();
            ed.docs[0].path.set(Some(root.join("src/scenes.rs")));
            long_text(&mut ed);
            for (n, name) in ["keymap.rs", "panel.rs", "configuration.md"].iter().enumerate() {
                let mut d = crate::document::Document::from_str(70 + n as u64, "one\ntwo\n");
                d.path.set(Some(root.join(name)));
                ed.workspace.docs.push(d.id);
                ed.docs.push(d);
            }
            ed.workspace.name = "panes4".into();
            // (each session's checkout a scratch folder of its own, kept for the case — never a guessable temp path)
            let homes: Vec<crate::scratch::Scratch> =
                (1..sessions).map(|i| crate::scratch::Scratch::new(&format!("wheel-audit-{i}"))).collect();
            for (i, home) in homes.iter().enumerate() {
                let id = ed.workspace_add(home.path().to_path_buf());
                ed.workspace_by_id(id).unwrap().name = format!("s{}", i + 1);
            }
            if chat_open {
                chat(&mut ed, "");
            }
            if following {
                let c = ed.workspace.claude.chat.as_mut().unwrap();
                c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, "a question", ""));
                (c.state, c.follow.on) = (crate::chat::State::Thinking, true);
            }
            if selection {
                keys(&mut ed, &["x", "x", "x"]);
            }
            ed.workspace.viewer.at = if keys_in == "code" { At::Code } else { At::Chat };
            let draw = |ed: &mut Editor| {
                ed.focus_sync();
                crate::term::render(ed, &mut Vec::new(), w, h).unwrap();
                crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), w, h).unwrap()
            };
            let text = draw(&mut ed);
            let top = text.lines().next().unwrap_or_default().to_string();
            println!(
                "\n## {w} wide · keys in the {keys_in} · selection {selection} · chat {chat_open} · {sessions} sessions · following {following}\n  {top}"
            );
            let spots: Vec<String> = ed
                .screen
                .word_spots
                .borrow()
                .iter()
                .filter(|s| s.y == 0)
                .map(|s| format!("{}..{} {:?} {:?}", s.from, s.to, s.press, s.on))
                .collect();
            println!("  spots on row 0: {spots:#?}");
            for name in ["scenes.rs", "keymap.rs", "panel.rs", "configuration.md"] {
                // (the last time it's on the row: the right end's, not the path's)
                let Some(i) = top.rfind(name) else {
                    println!("  {name}: not on the line");
                    continue;
                };
                let x = top[..i].chars().count() + 1;
                let under = ed.mouse_under(K::Down, x, 0);
                let before = ed.doc().id;
                ed.handle_mouse(Mouse { kind: K::Down, x: x as u16, y: 0, alt: false, ctrl: false });
                ed.handle_mouse(Mouse { kind: K::Up, x: x as u16, y: 0, alt: false, ctrl: false });
                let after = ed.doc().id;
                draw(&mut ed);
                println!("  {name} at {x}: {under:?} · pressed: {before:?} → {after:?}");
            }
            // (the editor and what watches the sessions' branches first, then their folders)
            drop(ed);
            drop(homes);
        }
    }
}

#[test]
#[ignore = "by hand: the wheel audit's table (2026-10-28)"]
fn wheel_audit() {
    let pty = !crate::sandbox::no_pty_here("mouse::tests::wheel_audit::wheel_audit");
    for &(name, setup, at) in CASES {
        if name.starts_with("terminal") && !pty {
            println!("\n## {name}: no pty here");
            continue;
        }
        crate::clock::hold();
        // (its folder, if the scene made one: removed as the case ends, after the editor rooted in it — declared first)
        let _kept: Option<crate::scratch::Scratch>;
        let mut ed = editor();
        setup(&mut ed);
        _kept = KEPT.take();
        let Some(cell) = find(&mut ed, at, 2) else {
            println!("\n## {name}: {at:?} not on the frame\n{}", shown(&mut ed));
            crate::clock::release();
            continue;
        };
        let l0 = look(&mut ed);
        println!("\n## {name} — the pointer at {cell:?}, keys {} ({})", l0.keys, l0.at);
        wheel(&mut ed, K::ScrollDown, cell, 3);
        let l1 = look(&mut ed);
        println!("  down ×3:   {}", changed(&l0, &l1));
        wheel(&mut ed, K::ScrollUp, cell, 3);
        let l2 = look(&mut ed);
        println!("  up ×3:     {}", changed(&l1, &l2));
        wheel(&mut ed, K::ScrollRight, cell, 3);
        let l3 = look(&mut ed);
        println!("  right ×3:  {}", changed(&l2, &l3));
        wheel(&mut ed, K::ScrollLeft, cell, 3);
        let l4 = look(&mut ed);
        println!("  left ×3:   {}", changed(&l3, &l4));
        wheel(&mut ed, K::ScrollDown, cell, 300);
        let l5 = look(&mut ed);
        wheel(&mut ed, K::ScrollDown, cell, 1);
        let l6 = look(&mut ed);
        println!("  past the end:  {}", changed(&l5, &l6));
        println!("  keys at the end: {} ({}) — from the start: {}", l6.keys, l6.at, changed(&l0, &l6));
        wheel(&mut ed, K::ScrollUp, cell, 300);
        let l7 = look(&mut ed);
        wheel(&mut ed, K::ScrollUp, cell, 1);
        let l8 = look(&mut ed);
        println!("  past the top:  {}", changed(&l7, &l8));
        println!("  overall:   {}", changed(&l0, &l8));
        wheel(&mut ed, K::ScrollLeft, cell, 1);
        let l9 = look(&mut ed);
        println!("  left at the edge: {}", changed(&l8, &l9));
        crate::clock::release();
    }
}
