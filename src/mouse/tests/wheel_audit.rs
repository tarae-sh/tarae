//! The wheel audit (2026-10-28): on each surface the wheel down, up, sideways and past each end, and what moved
//! besides what's under the pointer — the keys, where they are, the focus, the code's cursor and selection, a list's,
//! a card's place, the state's other fields — and whether a wheel with nothing left is silent. `wheel_audit` prints
//! its table (`cargo test --bin tarae wheel_audit -- --ignored --nocapture`); `the_wheel_only_scrolls` holds every
//! case to the mouse's rule (CLAUDE.md, the user 2026-10-28: the wheel only scrolls).

use crate::editor::Editor;
use crate::event::{Mouse, MouseKind as K};
use crate::pane::GroupKind;
use crate::uistate::tests::{chat, editor, frame};
use crate::workspace::Here;

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
    /// Each list's selection: the side panel's, the rail's file in hand, the floating lists', the picker's.
    lists: String,
    /// A card's place on the frame.
    card: String,
    /// What a group's list drives beside it: the tests' failure's scroll, the task's output (its cursor and first line
    /// in sight), `:procs`' detail.
    detail: String,
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
        at: format!("{:?}", ed.workspace.here()),
        focus: format!("{:?}", ed.workspace.viewer.focus),
        doc: format!("{:?}", ed.doc().id),
        selection: format!("{:?}", ed.doc().selection()),
        lists: format!(
            "{:?}",
            (
                ed.workspace.viewer.sidebar.as_ref().map(|s| s.selected),
                ed.workspace.code_review.as_ref().map(|r| r.selected),
                [GroupKind::Messages, GroupKind::Sessions].map(|k| ed.float_list(k).map(|p| p.selected)),
                ed.picker.as_ref().map(|p| p.selected),
                // (a group's lists, `:procs`', a plugin's panel's)
                ed.workspace.test_run.as_ref().map(|r| r.selected),
                ed.workspace.task_run.as_ref().map(|r| r.picked),
                ed.workspace.preflight.as_ref().map(|p| p.selected),
                ed.procs.panel.as_ref().map(|p| p.selected),
                ed.plugins.as_ref().and_then(|p| p.panel.as_ref()).map(|p| p.selected),
            )
        ),
        card: format!("{:?}", ed.screen.offer_card),
        detail: {
            let task = crate::sizes::Slot::Task;
            let rows = crate::rundoc::rows(ed, task);
            format!(
                "{:?}",
                (
                    ed.workspace.test_run.as_ref().map(|r| r.scroll),
                    crate::rundoc::cursor_line(ed, task),
                    ed.workspace.task_run.as_ref().map(|r| r.top(rows)),
                    (ed.workspace.outputs.doc(crate::sizes::Slot::Procs)).map(|d| d
                        .text
                        .lines()
                        .next()
                        .map(|l| l.to_string())),
                )
            )
        },
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
        ("LISTS", &a.lists, &b.lists),
        ("CARD", &a.card, &b.card),
        ("DETAIL", &a.detail, &b.detail),
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

/// A group's list's view as the last frame drew it, `:procs`' and a plugin's panel's too: (its first row in sight, the
/// rows in sight, its rows).
fn list_view(ed: &Editor) -> Option<(usize, usize, usize)> {
    if let Some((_, y1, first, scroll)) = ed.screen.plugin_panel.get() {
        let n = ed.plugins.as_ref().and_then(|p| p.panel.as_ref()).map_or(0, |p| p.panel.rows.len());
        return Some((scroll, y1 - first, n));
    }
    let lists = ed.screen.lists.borrow();
    let (kind, (_, rows, top)) = *lists.iter().find(|(k, _)| {
        matches!(k, GroupKind::Tests | GroupKind::Task | GroupKind::Preflight | GroupKind::Procs)
    })?;
    let w = &ed.workspace;
    let n = match kind {
        GroupKind::Tests => w.test_run.as_ref().map_or(0, |r| r.cases.len()),
        GroupKind::Task => w.task_run.as_ref().map_or(0, |r| r.place_rows().len()),
        GroupKind::Preflight => w.preflight.as_ref().map_or(0, |p| p.steps.len()),
        _ => ed.procs.rows.len(),
    };
    Some((top, rows, n))
}

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
            ed.workspace.set_at(Here::Side);
        },
        "line_5 ",
    ),
    ("chat transcript, keys in its input", |ed| conversation(ed), "answer 25"),
    (
        "chat transcript, keys in the code",
        |ed| {
            long_text(ed);
            conversation(ed);
            ed.workspace.viewer.keys_to_code();
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
    ("side panel, keys in the code", |ed| side(ed, Here::Code), "f05.rs"),
    ("side panel, its keys", |ed| side(ed, Here::Side), "f05.rs"),
    ("review rail, keys in the code", |ed| rail(ed, false), "f05.rs"),
    ("review rail, its keys", |ed| rail(ed, true), "f05.rs"),
    ("review rail, its keys, files on disk", |ed| rail_on_disk(ed), "f05.rs"),
    ("test output, keys in the code", |ed| output(ed, false), "output line 195"),
    ("test output, its keys", |ed| output(ed, true), "output line 195"),
    ("terminal, its program's keys", |ed| terminal(ed, None), "line 290"),
    ("terminal, its view mode", |ed| terminal(ed, Some(&["<C-\\>"])), "line 290"),
    ("terminal, keys in the code", |ed| terminal(ed, Some(&["<C-\\>", "<C-\\>"])), "line 290"),
    (":messages, its keys", |ed| messages(ed, false), "message 55"),
    (":messages, keys given back", |ed| messages(ed, true), "message 55"),
    ("session list, its keys", |ed| keys(ed, &[" ", "W"]), "│▎"),
    ("session list, keys given back", |ed| keys(ed, &[" ", "W", "<esc>"]), "│▎"),
    ("session list docked, an ask at its foot", |ed| sessions_docked_asked(ed), "ws03"),
    ("a group's list: the tests' results, its keys", |ed| results(ed, true, false), "case_05"),
    ("a group's list: the tests' results, keys in the code", |ed| results(ed, false, false), "case_05"),
    ("a group's list: the tests' results, running", |ed| results(ed, true, true), "case_75"),
    ("a group's list: the task's places, its keys", |ed| places(ed, true), "place m05"),
    ("a group's list: the task's places, keys in the code", |ed| places(ed, false), "place m05"),
    ("a group's list: a preflight's steps, its keys", |ed| steps(ed, true), "step 05"),
    ("a group's list: :procs, its keys", |ed| procs(ed, true), "job05"),
    ("a group's list: :procs, keys in the code", |ed| procs(ed, false), "job05"),
    ("a group's list: a plugin's panel, its keys", |ed| plugin(ed, true), "row 5"),
    ("a group's list: a plugin's panel, keys in the code", |ed| plugin(ed, false), "row 5"),
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

fn side(ed: &mut Editor, at: Here) {
    long_text(ed);
    let files = (0..80).map(|i| format!("f{i:02}.rs")).collect();
    ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::with_files(std::env::temp_dir(), files));
    ed.workspace.set_at(at);
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
            ed.workspace.set_at(Here::Program);
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

/// Fifty sessions in the session list docked right, the keys in the code, an ask standing over its foot — the list's
/// rows those above it.
fn sessions_docked_asked(ed: &mut Editor) {
    let scratch = crate::scratch::Scratch::new("wheel-audit-sessions");
    for i in 0..50 {
        let dir = scratch.path().join(format!("w{i:02}"));
        std::fs::create_dir_all(&dir).unwrap();
        let id = ed.workspace_add(dir);
        ed.workspace_by_id(id).unwrap().name = format!("ws{i:02}");
    }
    KEPT.set(Some(scratch));
    ed.sizes.sessions_dock = Some(crate::pane::Side::Right);
    ed.session_list();
    ed.float_list_leave(GroupKind::Sessions);
    let what = crate::offer::What::ReviewAtEnd;
    ed.push_offer(crate::offer::Offer::new("An ask".into(), "what it asks", "", String::new(), what));
    frame(ed);
    let (list, card) = (ed.screen.list(GroupKind::Sessions), ed.screen.offer_card);
    assert!(
        list.zip(card).is_some_and(|((y0, rows, _), c)| y0 + rows < c.1),
        "the ask over its foot, rows above it: {list:?} {card:?}"
    );
}

/// Eighty tests' results, the first one failed and selected (its failure the detail); `running`: still running, the
/// list at its newest rows.
fn results(ed: &mut Editor, focused: bool, running: bool) {
    use crate::test_results::{Case, Status};
    let case = |i: usize| Case {
        name: format!("t::case_{i:02}"),
        status: if i == 0 { Status::Failed } else { Status::Passed },
        secs: None,
        message: (0..40).map(|l| format!("failure line {l}")).collect(),
        at: None,
    };
    let mut run = crate::testing::TestRun::finished("t", (0..80).map(case).collect(), vec!["ok".into()]);
    if running {
        run.state = crate::testing::RunState::Running;
    }
    ed.workspace.test_run = Some(run);
    frame(ed);
    if focused {
        ed.list_focus(GroupKind::Tests);
    }
}

/// A task's sixty places beside its output, the first picked.
fn places(ed: &mut Editor, focused: bool) {
    let root = ed.workspace.root.clone();
    let t = crate::task::Task { name: "check".into(), cmd: "cargo check".into(), cwd: root.clone() };
    let mut r = crate::task::Run::for_test(t);
    r.state = crate::task::State::Passed;
    for i in 0..60 {
        (0..3).for_each(|l| r.push_line(false, format!("printed {i}.{l}"), false));
        let message = format!("place m{i:02}");
        let it = crate::quickfix::Item {
            path: root.join("a.rs"),
            line: i,
            col: 0,
            message,
            severity: 2,
            mark: None,
        };
        r.place(it, None);
    }
    ed.workspace.task_run = Some(r);
    frame(ed);
    if focused {
        ed.list_focus(GroupKind::Task);
    }
}

/// A preflight's sixty steps beside its output, the first selected.
fn steps(ed: &mut Editor, focused: bool) {
    let root = ed.workspace.root.clone();
    let t = crate::task::Task { name: "preflight".into(), cmd: String::new(), cwd: root };
    let mut r = crate::task::Run::for_test(t);
    r.state = crate::task::State::Passed;
    let step = |i: usize| crate::preflight::Step {
        name: format!("step {i:02}"),
        cmd: String::new(),
        note: None,
        from: String::new(),
        full: false,
    };
    let all: Vec<_> = (0..60).map(step).collect();
    let mut p =
        crate::preflight::Preflight::new(r.generation(), &all, crate::preflight::Set::Full, "k".into());
    p.steps.iter_mut().for_each(|s| s.state = crate::preflight::State::Passed);
    for s in &all {
        r.push_line(false, format!("── {} ──", s.name), true);
        (0..5).for_each(|l| r.push_line(false, format!("its line {l}"), false));
    }
    ed.workspace.preflight = Some(p);
    ed.workspace.task_run = Some(r);
    frame(ed);
    if focused {
        ed.list_focus(GroupKind::Preflight);
    }
}

/// `:procs` with sixty processes under tarae, the first of them selected (its detail beside — tarae's would list them
/// all).
fn procs(ed: &mut Editor, focused: bool) {
    use crate::procs::tests::{os, sample};
    let me = std::process::id();
    let mut all = vec![os(me, 1, "tarae")];
    // (names of their own: the pointer on the table's row, never the detail beside it)
    all.extend((0..60).map(|i| os(50_000 + i, me, &format!("job{i:02}"))));
    ed.procs_toggle();
    ed.procs_sampled(sample(all));
    ed.procs.panel.as_mut().unwrap().selected = 1;
    frame(ed);
    if !focused {
        ed.workspace.viewer.keys_to_code();
    }
}

/// A plugin's panel of eighty rows, the first selected.
fn plugin(ed: &mut Editor, focused: bool) {
    ed.plugin_panel_stand_in(80);
    frame(ed);
    if !focused {
        ed.workspace.viewer.keys_to_code();
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
            ed.workspace.set_at(if keys_in == "code" { Here::Code } else { Here::Chat });
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

/// The wheel only scrolls (CLAUDE.md's mouse bullet, the user 2026-10-28), on every surface of the audit's table: down,
/// up, past each end — the keys, where they are, the focus, the code's buffer, cursor and selection, every list's
/// selection and a card's place never move (no view mode by the wheel, no row dragged along, a card read to its end
/// keeps its width and its left edge); a notch past an end is silent and none piles up (the first one back moves
/// what moved); a run's output stops with its last line at the bottom. What it found broken once: each of the eight
/// fixes put back by hand, one at a time, failed it here (kinds C's commit says which assertion).
#[test]
fn the_wheel_only_scrolls() {
    let pty = !crate::sandbox::no_pty_here("mouse::tests::wheel_audit::the_wheel_only_scrolls");
    let mut off = Vec::new();
    for &(name, setup, at) in CASES {
        if name.starts_with("terminal") && !pty {
            continue;
        }
        crate::clock::hold();
        let _kept: Option<crate::scratch::Scratch>;
        let mut ed = editor();
        setup(&mut ed);
        _kept = KEPT.take();
        let cell = find(&mut ed, at, 2).unwrap_or_else(|| panic!("{name}: {at:?} not on the frame"));
        // (a list's case aims at the list, never the detail beside it)
        let under = ed.mouse_under(K::ScrollDown, cell.0, cell.1).surface;
        if name.starts_with("a group's list")
            && !matches!(under, crate::mouse::Surface::List(_) | crate::mouse::Surface::Plugin)
        {
            off.push(format!("{name}: the pointer at {cell:?} is over {under:?}, not the list"));
        }
        let mut fail =
            |what: &str, a: &Look, b: &Look| off.push(format!("{name}: {what} — {}", changed(a, b)));
        let mut looks = vec![look(&mut ed)];
        // (120 notches reach every case's end: its longest, the code's 200 lines wrapped, three rows a notch)
        for (kind, n) in [(K::ScrollDown, 3), (K::ScrollUp, 3), (K::ScrollDown, 120)] {
            wheel(&mut ed, kind, cell, n);
            looks.push(look(&mut ed));
        }
        // (at the end: one more notch, then one back)
        wheel(&mut ed, K::ScrollDown, cell, 1);
        let past_end = look(&mut ed);
        let end_view = list_view(&ed);
        let end_text = past_end.text.join("\n");
        wheel(&mut ed, K::ScrollUp, cell, 1);
        let back_from_end = look(&mut ed);
        wheel(&mut ed, K::ScrollUp, cell, 120);
        let top = look(&mut ed);
        let top_view = list_view(&ed);
        wheel(&mut ed, K::ScrollUp, cell, 1);
        let past_top = look(&mut ed);
        wheel(&mut ed, K::ScrollDown, cell, 1);
        let back_from_top = look(&mut ed);
        crate::clock::release();
        let first = looks[0].clone();
        // (it scrolls at all: down from where it starts, or back up — a chat and a terminal start at their bottom)
        let scrolls = looks[1].text != first.text || looks[2].text != looks[1].text;
        // (the session list's own cases have one session: nothing to scroll)
        if !scrolls && !name.starts_with("session list,") {
            fail("the wheel scrolled nothing", &first, &looks[2]);
        }
        looks.extend([
            past_end.clone(),
            back_from_end.clone(),
            top.clone(),
            past_top.clone(),
            back_from_top.clone(),
        ]);
        for l in &looks {
            let still = |a: &Look, b: &Look| {
                // (the detail a group's list drives: never moved by the wheel over the list)
                let detail = !name.starts_with("a group's list") || a.detail == b.detail;
                detail
                    && (&a.keys, &a.at, &a.focus, &a.doc, &a.selection, &a.lists, &a.card)
                        == (&b.keys, &b.at, &b.focus, &b.doc, &b.selection, &b.lists, &b.card)
            };
            if !still(&first, l) {
                fail("the keys, a cursor, a selection or a card moved", &first, l);
                break;
            }
        }
        let at_end = &looks[3];
        if past_end.text != at_end.text {
            fail("a notch past its end moved it", at_end, &past_end);
        }
        if past_top.text != top.text {
            fail("a notch past its top moved it", &top, &past_top);
        }
        if scrolls && back_from_end.text == past_end.text {
            fail(
                "notches piled up past its end: the first one back moved nothing",
                &past_end,
                &back_from_end,
            );
        }
        if scrolls && back_from_top.text == past_top.text {
            fail(
                "notches piled up past its top: the first one back moved nothing",
                &past_top,
                &back_from_top,
            );
        }
        if name.starts_with("test output")
            && !(end_text.contains("output line 195") && end_text.contains("output line 199"))
        {
            fail("its output went past its last line", &first, &past_end);
        }
        // (a list's view reaches its last row and its first, wherever its selection is)
        if name.starts_with("a group's list") {
            match (end_view, top_view) {
                (Some((t, rows, n)), Some((0, ..))) if t + rows >= n => {}
                views => fail(&format!("its view never reached both ends: {views:?}"), &first, &past_end),
            }
        }
    }
    assert!(off.is_empty(), "the wheel did more than scroll:\n{}", off.join("\n"));
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
