use super::*;
use crate::chat::{Chat, Msg, Role};
use crate::config::Config;
use crate::event::{Event, Mouse, MouseKind};
use crate::key::{Code, Key};
use crate::workspace::Here;

const W: u16 = 140;
const H: u16 = 34;
/// The child's marker, from the parent's scratch folder.
const MARKER_ENV: &str = "TARAE_TEST_CRASH_MARKER";
/// The conversation's words: never in a marker, nor on a held chat's screen.
const SECRET: &str = "tarae-fake-transcript-words";

fn frame(ed: &mut Editor) -> String {
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, W, H).unwrap();
    let mut p = vt100::Parser::new(H, W, 0);
    p.process(&bytes);
    p.screen().contents()
}

/// A session's chat with a conversation, the keys in it.
fn chatting() -> Editor {
    let mut ed = Editor::new(Config::default());
    ed.docs[0].text = ropey::Rope::from_str("fn main() {}\n");
    let mut c = Chat::new();
    c.msgs = vec![Msg::new(Role::User, "tidy it", ""), Msg::new(Role::Assistant, SECRET, "")];
    ed.workspace.claude.chat = Some(c);
    ed.workspace.set_at(Here::Chat);
    ed
}

fn crashed(drawing: Option<Drawing>) -> Crashed {
    Crashed {
        at: "src/cut.rs:151".into(),
        message: "byte index 170 is not a char boundary".into(),
        thread: "main".into(),
        build: "0.1.0 32018b82".into(),
        drawing,
        held: false,
    }
}

/// A hold of `pane` in checkout `root`, by this build's crash or another's.
fn held(root: PathBuf, pane: Pane, same_build: bool) -> Held {
    let build = if same_build { tarae_wire::BUILD } else { "0.1.0 32018b82" };
    Held { root, session: "panes4".into(), pane, at: "src/cut.rs:151".into(), build: build.into() }
}

fn drawing(pane: Pane, again: bool) -> Drawing {
    Drawing { root: PathBuf::from("/w/panes4"), session: "panes4".into(), pane, again }
}

/// A slice's panic quotes what it sliced: kept up to it, one line, at most `MESSAGE` characters.
#[test]
fn a_message_is_kept_short_of_the_text_it_quotes() {
    // (core's three forms that quote what they sliced, up to Rust 1.9x's: the text never kept)
    let quoted = [
        ("byte index 99 is out of bounds of `tarae-fake — words`", "byte index 99 is out of bounds of"),
        (
            "byte index 13 is not a char boundary; it is inside '—' (bytes 11..14) of `tarae-fake — words`[...]",
            "byte index 13 is not a char boundary; it is inside '—' (bytes 11..14) of",
        ),
        (
            "begin <= end (12 <= 10) when slicing `tarae-fake —\nwords`",
            "begin <= end (12 <= 10) when slicing",
        ),
    ];
    for (m, _) in quoted {
        assert!(!cut(m).contains("tarae-fake"), "no text kept: {m}");
    }
    for (m, kept) in quoted {
        assert_eq!(cut(m), kept, "from {m}");
    }
    // (this toolchain's, as they panic: they quote nothing, kept whole)
    let text = "tarae-fake — words".to_string();
    let at = |n: usize| std::hint::black_box(n);
    for payload in [
        std::panic::catch_unwind(|| text[..at(99)].len()).unwrap_err(),
        std::panic::catch_unwind(|| text[..at(13)].len()).unwrap_err(),
        std::panic::catch_unwind(|| text[at(12)..at(10)].len()).unwrap_err(),
    ] {
        let m = payload.downcast_ref::<String>().expect("a formatted message");
        assert!(!cut(m).contains("tarae-fake"), "no text kept: {m}");
        assert!(m.contains('`') || cut(m) == *m, "kept whole when it quotes nothing: {m}");
    }
    assert_eq!(cut("one\ntwo"), "one");
    let long = "—".repeat(300);
    assert_eq!(cut(&long).chars().count(), MESSAGE + 1, "cut, an ellipsis after");
    assert!(cut(&long).ends_with('…'));
}

/// A marker written reads back the same, and reading it takes it away.
#[test]
fn a_marker_reads_back_once() {
    let dir = crate::scratch::Scratch::new("crash-marker");
    let marker = file(dir.path(), Path::new("/w/main"));
    for c in [
        crashed(Some(drawing(Pane::Chat, false))),
        crashed(Some(drawing(Pane::Code(PathBuf::from("/w/panes4/src/a.rs")), true))),
        crashed(None),
        Crashed { held: true, ..crashed(Some(drawing(Pane::Chat, false))) },
    ] {
        write(&marker, &c).unwrap();
        assert_eq!(take(&marker), Some(c));
        assert_eq!(take(&marker), None, "gone once read");
    }
    assert_ne!(file(dir.path(), Path::new("/w/other")), marker, "each start root its own");
}

/// The start's words: the session and its pane, where, the build's commit; again when it was shown; no pane, where.
#[test]
fn a_crash_is_said_with_its_pane_where_and_build() {
    assert_eq!(
        said(&crashed(Some(drawing(Pane::Chat, false)))),
        "tarae crashed drawing panes4's chat — it opens held · src/cut.rs:151, build 32018b82"
    );
    assert_eq!(
        said(&crashed(Some(drawing(Pane::Code(PathBuf::from("/w/panes4/src/a.rs")), true)))),
        "tarae crashed again showing panes4's src/a.rs — it opens held · src/cut.rs:151, build 32018b82"
    );
    assert_eq!(said(&crashed(None)), "tarae crashed · src/cut.rs:151, build 32018b82");
    assert_eq!(
        said(&Crashed { held: true, ..crashed(Some(drawing(Pane::Chat, false))) }),
        "panes4's chat still held after a crash · src/cut.rs:151, build 32018b82"
    );
}

/// A hold not shown is carried over a quit or `:restart` (`keep`): the next start holds it again — the transcript not
/// laid out —, says so quietly, and works out the build again: another build's start drops the same-build row.
#[test]
fn a_hold_not_shown_is_carried_to_the_next_start() {
    let dir = crate::scratch::Scratch::new("crash-keep");
    let marker = dir.path().join("marker.json");
    for same_build in [true, false] {
        let mut ed = chatting();
        let mut c = crashed(Some(Drawing { root: ed.workspace.root.clone(), ..drawing(Pane::Chat, false) }));
        if same_build {
            c.build = tarae_wire::BUILD.to_string();
        }
        start(&mut ed, c);
        frame(&mut ed);
        // (the quit)
        keep_at(&ed, &marker);
        drop(ed);
        let c = take(&marker).expect("the hold kept");
        assert!(c.held && c.message.is_empty(), "a hold, no new crash: {c:?}");
        let mut ed = chatting();
        start(&mut ed, c);
        let screen = frame(&mut ed);
        assert!(screen.contains("panes4's chat still held after a crash"), "said:\n{screen}");
        assert!(!screen.contains("drawing panes4"), "no new crash said:\n{screen}");
        assert!(screen.contains(HELD) && !screen.contains(SECRET), "held again:\n{screen}");
        assert_eq!(screen.contains(SAME), same_build, "the build worked out again:\n{screen}");
        // (shown, then a quit: nothing kept)
        assert!(show(&mut ed));
        keep_at(&ed, &marker);
        assert_eq!(take(&marker), None, "shown, not kept");
    }
}

/// #470: a panic laying out a session's chat leaves a marker naming it (the hook, in a process of its own:
/// `crash_in_its_own_process`); the next start says it and holds that chat — its row, not the conversation — until
/// `enter` in its view mode; shown, a crash drawing it is said as again.
#[test]
fn a_crash_drawing_a_chat_holds_it_at_the_next_start() {
    let dir = crate::scratch::Scratch::new("crash-start");
    let marker = dir.path().join("marker.json");
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash::tests::crash_in_its_own_process", "--ignored", "--nocapture"])
        .env(MARKER_ENV, &marker)
        .output()
        .unwrap();
    let said_by = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && said_by.contains("1 passed"), "the child ran it: {said_by}");
    let text = std::fs::read_to_string(&marker).unwrap_or_default();
    assert!(!text.contains("secret") && !text.contains(SECRET), "no text in it: {text}");
    let c = take(&marker).expect("the marker");
    let mut ed = chatting();
    let d = c.drawing.clone().expect("the pane it was drawing");
    assert_eq!((&d.root, &d.pane, d.again), (&ed.workspace.root, &Pane::Chat, false));
    assert!(c.at.starts_with("src/crash.rs:"), "where: {}", c.at);
    assert_eq!(c.build, tarae_wire::BUILD);
    start(&mut ed, c);
    let screen = frame(&mut ed);
    assert!(screen.contains("tarae crashed drawing"), "said at once:\n{screen}");
    assert!(screen.contains(HELD) && !screen.contains(SECRET), "held:\n{screen}");
    assert!(screen.contains(SAME), "the same build said:\n{screen}");
    // (typing goes to the input; its view mode's `enter` shows it)
    ed.handle_key(Key::plain(Code::Esc));
    let screen = frame(&mut ed);
    assert!(
        screen.contains("enter show") && !screen.contains(SECRET),
        "its view mode says the key:\n{screen}"
    );
    ed.handle_key(Key::plain(Code::Enter));
    let screen = frame(&mut ed);
    assert!(screen.contains(SECRET) && !screen.contains(HELD), "shown:\n{screen}");
    let _d = super::drawing(&ed, Pane::Chat);
    let again = DRAWING.with(|d| d.borrow().as_ref().map(|d| d.again));
    assert_eq!(again, Some(true), "a crash now is said as again");
}

/// `a_crash_drawing_a_chat_holds_it_at_the_next_start`'s child: the hook begun, the chat's layout panics.
#[test]
#[ignore = "a child: a_crash_drawing_a_chat_holds_it_at_the_next_start runs it in a process of its own"]
fn crash_in_its_own_process() {
    let Some(marker) = std::env::var_os(MARKER_ENV) else { return };
    begin(PathBuf::from(marker));
    let mut ed = chatting();
    STAND_IN.set(true);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| frame(&mut ed)));
    STAND_IN.set(false);
    assert!(r.is_err(), "the stand-in panicked");
}

/// A code pane a crash held: its row in place of the file, its top line the path without the definition at the
/// cursor (that's its content) — the press on it shows it; `enter` likewise.
#[test]
fn a_code_pane_a_crash_held_shows_on_enter_or_a_press() {
    let dir = crate::scratch::Scratch::new("crash-code");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let path = root.join("held.rs");
    std::fs::write(&path, "fn tarae_fake_held_code() {}\n").unwrap();
    for press in [false, true] {
        let mut ed = Editor::new(Config::default());
        ed.workspace.root = root.clone();
        ed.open_now(&path).unwrap();
        // (the definition's name on the top line needs its syntax tree: Rust's grammar is in tests' build — its parse
        // waited for, as the snapshots wait)
        let parsed = |ed: &Editor| ed.doc().syntax.as_ref().is_some_and(|s| s.tree.is_some() && !s.in_flight);
        let end = std::time::Instant::now() + crate::test_results::PATIENCE;
        while !parsed(&ed) {
            let left = end.saturating_duration_since(std::time::Instant::now());
            let ev = ed.events.recv_timeout(left).expect("its syntax tree");
            ed.handle_event(ev);
        }
        ed.crash.held = Some(held(root.clone(), Pane::Code(path.clone()), false));
        let screen = frame(&mut ed);
        assert!(screen.contains(HELD) && !screen.contains(SAME), "held:\n{screen}");
        assert!(screen.contains("held.rs"), "its path on the top line:\n{screen}");
        assert!(!screen.contains("tarae_fake_held_code"), "none of it drawn, nor its name:\n{screen}");
        if press {
            let s = ed
                .screen
                .word_spots
                .borrow()
                .iter()
                .find(|s| s.press == crate::wordspot::Press::Show)
                .cloned();
            let s = s.expect("its row a spot");
            for kind in [MouseKind::Down, MouseKind::Up] {
                let m = Mouse { kind, x: (s.from + 2) as u16, y: s.y as u16, alt: false, ctrl: false };
                ed.handle_event(Event::Mouse(m));
            }
        } else {
            ed.handle_key(Key::plain(Code::Enter));
        }
        let screen = frame(&mut ed);
        assert!(screen.contains("tarae_fake_held_code") && !screen.contains(HELD), "shown:\n{screen}");
        assert!(screen.contains("ƒ tarae_fake_held_code"), "shown, its name on the top line:\n{screen}");
    }
}

/// Each input a held pane meets, a frame after each: none panics, none shows it (`shown`: its content's words) — the
/// pointer's at (`x`, `y`), a blank row of it; then a press on its row shows it.
fn hammer(ed: &mut Editor, keys: &[&str], (x, y): (u16, u16), shown: &str) {
    let still = |ed: &mut Editor, what: &str| {
        let screen = frame(ed);
        assert!(ed.crash.held.is_some() && !screen.contains(shown), "{what} showed it:\n{screen}");
    };
    for k in keys {
        for k in k.split(' ') {
            let key = match k.chars().count() {
                1 => Key::plain(Code::Char(k.chars().next().unwrap())),
                _ => k.parse().unwrap(),
            };
            ed.handle_key(key);
        }
        still(ed, k);
    }
    let mouse = |kind, (x, y)| Event::Mouse(Mouse { kind, x, y, alt: false, ctrl: false });
    let inputs = [
        ("the wheel", vec![mouse(MouseKind::ScrollDown, (x, y)), mouse(MouseKind::ScrollUp, (x, y))]),
        ("a click on a blank row", vec![mouse(MouseKind::Down, (x, y)), mouse(MouseKind::Up, (x, y))]),
        ("a hover", vec![Event::Hover(x, y), Event::Hover(x + 1, y)]),
    ];
    for (what, events) in inputs {
        for e in events {
            ed.handle_event(e);
        }
        still(ed, what);
    }
    crate::term::render(ed, &mut Vec::new(), 80, 20).unwrap();
    ed.handle_event(Event::Resize);
    crate::term::render(ed, &mut Vec::new(), 50, 12).unwrap();
    ed.handle_event(Event::Resize);
    still(ed, "a resize");
    let spot =
        ed.screen.word_spots.borrow().iter().find(|s| s.press == crate::wordspot::Press::Show).cloned();
    let s = spot.expect("its row a spot");
    for kind in [MouseKind::Down, MouseKind::Up] {
        ed.handle_event(mouse(kind, ((s.from + 2) as u16, s.y as u16)));
    }
    let screen = frame(ed);
    assert!(screen.contains(shown) && !screen.contains(HELD), "a press on its row shows it:\n{screen}");
}

/// The hold never crashes itself: a held code pane, focused, and a held chat meet every move, page, search, the wheel,
/// a click, a hover and a resize, and stay held until a press on their row.
#[test]
fn a_held_pane_stays_held_whatever_comes() {
    let dir = crate::scratch::Scratch::new("crash-hold");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let path = root.join("held.rs");
    let code: String = (0..300).map(|i| format!("fn tarae_fake_held_{i}() {{}}\n")).collect();
    std::fs::write(&path, &code).unwrap();
    let moves = ["j", "k", "j j j", "G", "g e", "g g", "pagedown", "pageup", "C-f", "C-b", "C-d", "C-u"];

    let mut ed = Editor::new(Config::default());
    ed.workspace.root = root.clone();
    ed.docs[0].path.set(Some(path.clone()));
    ed.docs[0].text = ropey::Rope::from_str(&code);
    ed.crash.held = Some(held(root.clone(), Pane::Code(path), true));
    frame(&mut ed);
    let &(_, rect, gutter, _) = ed.screen.panes.first().expect("the pane");
    let text = crate::chrome::pane_text(rect, ed.screen.text_top);
    let blank = ((text.x + gutter + 2) as u16, (text.y + 4) as u16);
    let searches = ["/ f a k e enter", "n", "space / f a k e esc"];
    let keys: Vec<&str> = moves.iter().chain(&searches).copied().collect();
    hammer(&mut ed, &keys, blank, "tarae_fake_held");

    let mut ed = chatting();
    ed.workspace.root = root.clone();
    ed.crash.held = Some(held(root, Pane::Chat, true));
    frame(&mut ed);
    let (y, ..) = ed.screen.chat_view.get().expect("the transcript drawn");
    let x = ed.screen.chat_x().expect("the chat's column");
    let blank = ((x + 4) as u16, (y + 4) as u16);
    // (its view mode: `esc` from the input)
    let searches = ["/ f a k e enter", "n", "space / f a k e esc"];
    let keys: Vec<&str> = ["esc"].iter().chain(&moves).chain(&searches).copied().collect();
    hammer(&mut ed, &keys, blank, SECRET);
}

/// Another session's pane isn't held: the hold is the checkout's.
#[test]
fn the_hold_is_its_sessions() {
    let mut ed = chatting();
    ed.crash.held = Some(held(PathBuf::from("/w/elsewhere"), Pane::Chat, true));
    let screen = frame(&mut ed);
    assert!(screen.contains(SECRET) && !screen.contains(HELD), "drawn:\n{screen}");
    assert!(!show(&mut ed), "nothing of the front's to show");
}
