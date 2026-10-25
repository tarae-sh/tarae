//! Per-pane follow (panes P3 step 8, plan §4.1 as CLAUDE.md has follow now): follow leads the code pane that had the
//! code's keys when it started; your keys in another pane never pause it — its view moves there, Claude's caret drawn
//! there, your selection and your view untouched.

use std::path::Path;

use serde_json::json;

use super::*;
use crate::config::Config;
use crate::key::{Code, Key};
use crate::llm::StreamEv;
use crate::split::PaneId;

const W: u16 = 160;
const H: u16 = 30;

struct Scene {
    ed: Editor,
    dir: crate::scratch::Scratch,
    g: u64,
    a: PaneId,
    b: PaneId,
}

/// `a.rs` (60 lines), `b.rs` and `c.rs` in a project, a chat at work, follow on in the left pane `a` (as a turn begun
/// there takes it); then `C-w v`: the keys in the right one `b` — on `b.rs` when `other`, else on the same file.
fn scene(name: &str, other: bool) -> Scene {
    let dir = crate::scratch::Scratch::new(name);
    let text: String = (1..=60).map(|i| format!("let x{i} = {i};\n")).collect();
    std::fs::write(dir.join("a.rs"), text).unwrap();
    std::fs::write(dir.join("b.rs"), "fn b() {}\n").unwrap();
    std::fs::write(dir.join("c.rs"), "fn c() {}\nfn d() {}\n").unwrap();
    let mut ed = Editor::new(Config::default());
    ed.session.root = dir.to_path_buf();
    ed.config.llm.command = "sh".into();
    ed.config.llm.args = vec!["-c".into(), "cat > /dev/null".into()];
    ed.open(&dir.join("a.rs")).unwrap();
    let mut c = crate::chat::Chat::new();
    let _ = crate::chat::stand_in(&mut c);
    c.state = crate::chat::State::Thinking;
    c.follow.on = true;
    let a = ed.session.focus;
    c.follow.pane = Some(a);
    let g = c.generation();
    ed.session.chat = Some(c);
    frame(&mut ed);
    press(&mut ed, "<C-w>v");
    let b = ed.session.focus;
    assert_ne!(a, b);
    if other {
        ed.open(&dir.join("b.rs")).unwrap();
    }
    frame(&mut ed);
    Scene { ed, dir, g, a, b }
}

fn frame(ed: &mut Editor) -> Vec<String> {
    let mut buf = Vec::new();
    crate::term::render(ed, &mut buf, W, H).unwrap();
    crate::screen::text(&buf, W, H).unwrap().lines().map(str::to_string).collect()
}

/// Keys as the person presses them: through the event loop, so follow sees each (`follow::watch`).
fn press(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let k: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_event(crate::event::Event::Key(k));
    }
}

/// Claude reads line `line` (1-based) of `file`.
fn read(ed: &mut Editor, g: u64, dir: &Path, file: &str, line: usize) {
    let input = json!({"file_path": dir.join(file), "offset": line, "limit": 1});
    let id = format!("r{file}{line}");
    crate::chat::stream_event(ed, g, StreamEv::Tool { id, name: "Read".into(), input });
}

fn follow(ed: &Editor) -> &crate::follow::Follow {
    &ed.session.chat.as_ref().unwrap().follow
}

/// The lines pane `id` drew in the last frame.
fn drawn(ed: &Editor, id: PaneId) -> Vec<usize> {
    let p = ed.screen.panes.iter().find(|p| p.0 == id).expect("drawn");
    p.3.iter().map(|o| o.0).collect()
}

/// The file pane `id` shows (the focused one's: the document in front — its view syncs at the event's end).
fn shows(ed: &Editor, id: PaneId) -> String {
    let v = ed.session.views.iter().find(|v| v.id == id).unwrap();
    let d = match id == ed.session.focus {
        true => ed.doc(),
        false => ed.docs.iter().find(|d| d.id == v.doc).unwrap(),
    };
    d.path.as_ref().unwrap().file_name().unwrap().to_string_lossy().into_owned()
}

fn said(ed: &Editor, words: &str) -> bool {
    ed.toasts.iter().any(|t| t.text.contains(words))
}

/// Claude's edit of a.rs replacing `old` with `new`, streamed but its last `held` characters — still being written — and
/// the events until its rows are drawn (its place is looked for on a worker).
fn writing_into(ed: &mut Editor, g: u64, old: &str, new: &str, held: usize) {
    let s = |t: &str| serde_json::to_string(t).unwrap();
    let input =
        format!(r#"{{"edits":[{{"file_path":"a.rs","old_string":{},"new_string":{}}}]}}"#, s(old), s(new));
    let mut scan = Scan::new(Tool::Edit);
    scan.feed(&input[..input.len() - held]);
    let d = Delta { call: "t9".into(), tool: Tool::Edit, pieces: scan.take(), done: false };
    crate::chat::stream_event(ed, g, StreamEv::Writing(d));
    settle(ed, |ed| {
        ed.docs.iter().any(|d| d.path.as_ref().is_some_and(|p| p.ends_with("a.rs")) && shown(d).is_some())
    });
}

/// Events until `done` holds (a place is looked for on a worker).
fn settle(ed: &mut Editor, done: impl Fn(&Editor) -> bool) {
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    while !done(ed) {
        let left = end.saturating_duration_since(std::time::Instant::now());
        let ev = ed.events.recv_timeout(left).expect("an event before the time's up");
        ed.handle_event(ev);
    }
}

/// Your keys in the other pane on the same file — moves, a selection, typing — never pause follow; Claude's read moves
/// the pane follow leads, its place marked there and its path bar saying `● follow`, while your pane, your selection
/// and the keys stay where you left them.
#[test]
fn your_keys_in_another_pane_leave_follow_its_own() {
    let Scene { mut ed, dir, g, a, b } = scene("pane-follow-same", false);
    press(&mut ed, "jjjwix<esc>");
    let mine = ed.doc().selection().clone();
    assert!(!follow(&ed).paused, "your keys in another pane never pause it");
    read(&mut ed, g, &dir, "a.rs", 50);
    let rows = frame(&mut ed);
    assert!(!follow(&ed).paused && follow(&ed).pane == Some(a));
    assert_eq!(ed.session.focus, b, "the keys stay in your pane");
    assert_eq!(ed.doc().selection(), &mine, "your selection where you left it");
    assert!(drawn(&ed, a).contains(&49), "the followed pane on the line read: {:?}", drawn(&ed, a));
    assert_eq!((drawn(&ed, b)[0], ed.cursor_line()), (0, 3), "your pane's view yours");
    assert_eq!(
        crate::follow::place(&ed, a, ed.doc()),
        Some(49),
        "Claude's place marked in the followed pane"
    );
    assert_eq!(crate::follow::place(&ed, b, ed.doc()), None, "never in yours");
    let doc = ed.doc();
    assert!(
        crate::follow::caret_in(&ed, a, doc) && !crate::follow::caret_in(&ed, b, doc),
        "Claude's caret, as it writes, in the followed pane only"
    );
    let bx = ed.screen.panes.iter().find(|p| p.0 == b).unwrap().1.x;
    let (left, right): (String, String) =
        (rows[0].chars().take(bx).collect(), rows[0].chars().skip(bx).collect());
    assert!(left.contains("● follow") && !right.contains("● follow"), "its path bar says it: {}", rows[0]);
    // A half page of yours: your view, follow's untouched
    press(&mut ed, "<C-d>");
    assert!(!follow(&ed).paused, "a view key in your pane isn't taking over the other");
}

/// Two files: what Claude reads opens in the pane follow leads, never in yours; one it opened there goes when it moves
/// on. Claude's edit written in it while you type in yours: that pane's view on the writing, yours on your file.
#[test]
fn follow_opens_what_claude_reads_in_its_own_pane() {
    let Scene { mut ed, dir, g, a, b } = scene("pane-follow-files", true);
    read(&mut ed, g, &dir, "c.rs", 2);
    frame(&mut ed);
    assert_eq!((shows(&ed, a), shows(&ed, b)), ("c.rs".into(), "b.rs".into()));
    assert!(ed.doc().path.as_ref().unwrap().ends_with("b.rs"), "the code with the keys still yours");
    read(&mut ed, g, &dir, "a.rs", 40);
    frame(&mut ed);
    assert_eq!(shows(&ed, a), "a.rs");
    assert!(
        !ed.docs.iter().any(|d| d.path.as_ref().is_some_and(|p| p.ends_with("c.rs"))),
        "its preview gone"
    );
    assert!(drawn(&ed, a).contains(&39), "{:?}", drawn(&ed, a));
    // Claude writes over line 45 of a.rs while you type in b.rs
    press(&mut ed, "ifoo<esc>");
    let s = |t: &str| serde_json::to_string(t).unwrap();
    let input = format!(
        r#"{{"edits":[{{"file_path":"a.rs","old_string":{},"new_string":{}}}]}}"#,
        s("let x45 = 45;\n"),
        s("let x45 = 45;\nlet y = 0;\nlet z = 0;\n")
    );
    let mut scan = Scan::new(Tool::Edit);
    scan.feed(&input[..input.len() - 12]);
    let d = Delta { call: "t1".into(), tool: Tool::Edit, pieces: scan.take(), done: false };
    crate::chat::stream_event(&mut ed, g, StreamEv::Writing(d));
    let written = |ed: &Editor| {
        ed.docs.iter().any(|d| d.path.as_ref().is_some_and(|p| p.ends_with("a.rs")) && shown(d).is_some())
    };
    settle(&mut ed, written);
    frame(&mut ed);
    let led = crate::follow::led_in(&ed, a).expect("follow leads its pane");
    assert!((44..=47).contains(&led), "on the writing: {led}");
    assert!(drawn(&ed, a).contains(&led), "in sight there: {:?}", drawn(&ed, a));
    assert!(!follow(&ed).paused && ed.doc().path.as_ref().unwrap().ends_with("b.rs"));
    assert_eq!((ed.cursor_line(), drawn(&ed, b)[0]), (0, 0), "yours where you left it");
}

/// `C-f` with follow leading another pane: it moves to the code pane you were in, said; again, off. A turn begun from
/// there takes that pane too.
#[test]
fn c_f_moves_follow_to_the_pane_you_are_in() {
    let Scene { mut ed, dir, g, a, b } = scene("pane-follow-cf", true);
    crate::follow::toggle(&mut ed);
    assert_eq!((follow(&ed).pane, follow(&ed).on, follow(&ed).paused), (Some(b), true, false));
    assert!(said(&ed, "following in this pane"), "said");
    read(&mut ed, g, &dir, "a.rs", 30);
    frame(&mut ed);
    assert_eq!((shows(&ed, a), shows(&ed, b)), ("a.rs".into(), "a.rs".into()), "the read in this pane now");
    assert_eq!(crate::follow::view_line(&ed), Some(29));
    assert_eq!(drawn(&ed, a)[0], 0, "the other pane left where it was");
    crate::follow::toggle(&mut ed);
    assert!(!follow(&ed).on && said(&ed, "follow off"));
    // A turn begun with the keys in the other pane takes that one
    press(&mut ed, "<C-w>h");
    ed.session.chat.as_mut().unwrap().state = crate::chat::State::Idle;
    assert!(crate::chat::submit(&mut ed, "next", None, None), "sent");
    assert_eq!(follow(&ed).pane, Some(a), "a turn takes the pane with the code's keys");
}

/// `C-f` in the other pane while Claude writes in the first: this pane goes to the writing at once, its view on it.
#[test]
fn c_f_takes_this_pane_to_the_writing() {
    let Scene { mut ed, dir, g, a, b } = scene("pane-follow-cf-writing", true);
    // (Claude reads the file's top first: line 45 out of sight there)
    read(&mut ed, g, &dir, "a.rs", 1);
    frame(&mut ed);
    let s = |t: &str| serde_json::to_string(t).unwrap();
    let input = format!(
        r#"{{"edits":[{{"file_path":"a.rs","old_string":{},"new_string":{}}}]}}"#,
        s("let x45 = 45;\n"),
        s("let x45 = 45;\nlet y = 0;\nlet z = 0;\n")
    );
    let mut scan = Scan::new(Tool::Edit);
    scan.feed(&input[..input.len() - 12]);
    let d = Delta { call: "t1".into(), tool: Tool::Edit, pieces: scan.take(), done: false };
    crate::chat::stream_event(&mut ed, g, StreamEv::Writing(d));
    let written = |ed: &Editor| {
        ed.docs.iter().any(|d| d.path.as_ref().is_some_and(|p| p.ends_with("a.rs")) && shown(d).is_some())
    };
    settle(&mut ed, written);
    frame(&mut ed);
    let there = crate::follow::led_in(&ed, a).expect("follow leads its pane to the writing");
    assert!(drawn(&ed, a).contains(&there), "{there}: {:?}", drawn(&ed, a));
    assert_eq!(shows(&ed, b), "b.rs");
    crate::follow::toggle(&mut ed);
    frame(&mut ed);
    assert_eq!((follow(&ed).pane, shows(&ed, b)), (Some(b), "a.rs".into()), "this pane, at the writing");
    let open = |f: &str| ed.docs.iter().any(|d| d.path.as_ref().is_some_and(|p| p.ends_with(f)));
    assert!(open("b.rs"), "the file you opened in it stays open");
    let led = crate::follow::view_line(&ed).expect("led here");
    assert!((44..=47).contains(&led) && drawn(&ed, b).contains(&led), "{led}: {:?}", drawn(&ed, b));
}

/// Closing the pane follow leads ends it, said — `C-w o` from yours, `C-w q` in it (hopping into it isn't taking over).
#[test]
fn closing_the_followed_pane_ends_follow() {
    let Scene { mut ed, .. } = scene("pane-follow-only", false);
    press(&mut ed, "<C-w>o");
    assert!(!follow(&ed).on, "follow off");
    assert!(said(&ed, "the pane follow led closed"), "said");
    let Scene { mut ed, .. } = scene("pane-follow-close", true);
    press(&mut ed, "<C-w>h");
    assert!(follow(&ed).on && !follow(&ed).paused, "a hop into it isn't taking over");
    press(&mut ed, "<C-w>q");
    assert!(!follow(&ed).on && said(&ed, "the pane follow led closed"));
}

/// The keys in the followed pane: today's follow — the view led, a move of yours there pauses it, said.
#[test]
fn in_the_followed_pane_a_move_of_yours_takes_over() {
    let Scene { mut ed, dir, g, a, .. } = scene("pane-follow-in", true);
    read(&mut ed, g, &dir, "a.rs", 40);
    press(&mut ed, "<C-w>h");
    assert_eq!(ed.session.focus, a);
    assert!(!follow(&ed).paused, "the hop isn't a move");
    assert_eq!(crate::follow::view_line(&ed), Some(39), "led here as ever");
    press(&mut ed, "j");
    assert!(follow(&ed).paused && said(&ed, "you took over"));
}

/// Another pane zoomed on the same file while Claude writes out of the followed pane's sight: the placing its writing
/// asks for is that pane's — never taken by yours, your view where it was (your cursor near the writing: a placing
/// taken by mistake would keep it in sight, and stay).
#[test]
fn zoomed_on_the_same_file_your_view_stays() {
    let Scene { mut ed, dir, g, b, .. } = scene("pane-follow-zoom-same", false);
    read(&mut ed, g, &dir, "a.rs", 1);
    press(&mut ed, "<C-w>z:41<ret>");
    frame(&mut ed);
    let top = ed.doc().top;
    writing_into(&mut ed, g, "let x45 = 45;\n", "let x45 = 45;\nlet y = 0;\nlet z = 0;\n", 12);
    frame(&mut ed);
    assert_eq!(
        (ed.session.focus, ed.doc().top, ed.cursor_line()),
        (b, top, 40),
        "your zoomed view untouched"
    );
    assert!(!follow(&ed).paused);
}

/// A replay stop taken in your pane (`C-]` on a tool row, `]r`) while follow leads the other: Claude moving on there
/// leaves it showing in yours; follow never paused.
#[test]
fn a_stop_you_took_stays_while_follow_goes_on_elsewhere() {
    let Scene { mut ed, dir, g, a, b } = scene("pane-follow-replay", true);
    read(&mut ed, g, &dir, "a.rs", 40);
    read(&mut ed, g, &dir, "c.rs", 1);
    assert!(crate::follow::replay_call(&mut ed, "ra.rs40"), "a stop with a place");
    assert_eq!(shows(&ed, b), "a.rs", "shown where the key was pressed");
    read(&mut ed, g, &dir, "c.rs", 2);
    frame(&mut ed);
    let f = follow(&ed);
    assert!(f.replaying.is_some() && f.replay.at.is_some(), "the stop still showing in yours");
    assert!(!f.paused && shows(&ed, a) == "c.rs" && shows(&ed, b) == "a.rs");
    // `C-f` takes follow into your pane: live there, the stop's showing over
    crate::follow::toggle(&mut ed);
    let f = follow(&ed);
    assert_eq!((f.pane, f.replaying.is_some(), f.replay.at), (Some(b), false, None), "live in this pane");
    assert_eq!(shows(&ed, b), "c.rs", "where Claude is");
}

/// Claude's edit in the followed pane running past its bottom, its block begun on screen, your cursor on the block's
/// line in the other pane (one file, one selection): that cursor is yours there, never this pane's — the pane is taken
/// onto the block (`past_sight`).
#[test]
fn your_cursor_elsewhere_doesnt_hold_the_writing_back() {
    let Scene { mut ed, dir, g, a, .. } = scene("pane-follow-past", false);
    read(&mut ed, g, &dir, "a.rs", 1);
    press(&mut ed, ":21<ret>");
    assert_eq!(ed.cursor_line(), 20);
    // (follow off while it's written: nothing moves the pane before the look)
    ed.session.chat.as_mut().unwrap().follow.on = false;
    frame(&mut ed);
    let new: String = std::iter::once("let x21 = 21;\n".to_string())
        .chain((0..20).map(|i| format!("let w{i} = {i};\n")))
        .collect();
    writing_into(&mut ed, g, "let x21 = 21;\n", &new, 12);
    frame(&mut ed);
    ed.session.chat.as_mut().unwrap().follow.on = true;
    let past = ed.in_pane(a, |ed| super::past_sight(ed));
    assert_eq!(past.map(|l| l.start), Some(20), "its head below the followed pane's bottom");
}

/// Another pane zoomed (`C-w z`): follow goes on unseen, its card saying where; unzoomed, its pane is on Claude's line.
#[test]
fn zoomed_on_another_pane_follow_goes_on_unseen() {
    let Scene { mut ed, dir, g, a, .. } = scene("pane-follow-zoom", true);
    press(&mut ed, "<C-w>z");
    read(&mut ed, g, &dir, "a.rs", 50);
    let rows = frame(&mut ed);
    assert!(!follow(&ed).paused);
    assert!(
        rows.iter().any(|r| r.contains("follow") && r.contains("in a.rs")),
        "the card says where: {rows:#?}"
    );
    press(&mut ed, "<C-w>z");
    frame(&mut ed);
    assert!(drawn(&ed, a).contains(&49), "{:?}", drawn(&ed, a));
}
