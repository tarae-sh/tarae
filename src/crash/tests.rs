use super::*;
use crate::chat::{Chat, Msg, Role};
use crate::config::Config;
use crate::event::{Event, Mouse, MouseKind};
use crate::key::{Code, Key};
use crate::workspace::Here;

const W: u16 = 140;
const H: u16 = 34;
/// The child's folder (its state and its root), from the parent's scratch folder.
const DIR_ENV: &str = "TARAE_TEST_CRASH_DIR";
/// The child's case (`a_panic_in_its_own_process`).
const CASE_ENV: &str = "TARAE_TEST_CRASH_CASE";
/// A token-like word (built in two parts: never a provider's whole format in the source).
const FAKE_TOKEN: &str = concat!("ghp", "_tarae0fake0token0abcd1234");
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
        report: None,
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
        Crashed {
            report: Some(PathBuf::from("/h/state/crashed/reports/20261112-041233-7.txt")),
            ..crashed(None)
        },
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
    // (its report last: what a narrow toast cuts first)
    let report = Some(PathBuf::from("/h/.local/state/tarae/crashed/reports/20261112-041233-7.txt"));
    assert_eq!(
        said_home(&Crashed { report, ..crashed(None) }, Some(Path::new("/h"))),
        "tarae crashed · src/cut.rs:151, build 32018b82 · report ~/.local/state/tarae/crashed/reports/20261112-041233-7.txt"
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
    let mut ed = chatting();
    let marker = file(&dir.join("state"), &ed.workspace.root);
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash::tests::crash_in_its_own_process", "--ignored", "--nocapture"])
        .env(DIR_ENV, dir.path())
        .output()
        .unwrap();
    let said_by = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && said_by.contains("1 passed"), "the child ran it: {said_by}");
    let text = std::fs::read_to_string(&marker).unwrap_or_default();
    assert!(!text.contains("secret") && !text.contains(SECRET), "no text in it: {text}");
    let c = take(&marker).expect("the marker");
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
    let Some(dir) = std::env::var_os(DIR_ENV).map(PathBuf::from) else { return };
    let mut ed = chatting();
    begin(Some(&dir.join("state")), ed.workspace.root.clone());
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

// ---- a panic that ends tarae (#541, #542): each in a child process of its own ----

/// The child's run of `case` (`a_panic_in_its_own_process`) in `dir` — it passed; its stderr.
fn child(case: &str, dir: &Path) -> String {
    let (status, stdout, stderr) = child_run(case, dir);
    let passed = status.is_some_and(|s| s.success()) && stdout.contains("1 passed");
    assert!(passed, "{case}: the child ran it ({status:?}):\n{stdout}\n{stderr}");
    stderr
}

/// The child's run of `case` in `dir`, waited for the shared patience at most (a child that hangs fails, its case named,
/// never the run): how it ended (None: killed past the patience), its stdout and stderr.
fn child_run(case: &str, dir: &Path) -> (Option<std::process::ExitStatus>, String, String) {
    use std::io::Read as _;
    use std::process::Stdio;
    let mut c = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash::tests::a_panic_in_its_own_process", "--ignored", "--nocapture"])
        .env(DIR_ENV, dir)
        .env(CASE_ENV, case)
        .env("TARAE_TEST_FAKE_SECRET", FAKE_TOKEN)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let read = |mut r: Box<dyn std::io::Read + Send>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = r.read_to_string(&mut s);
            s
        })
    };
    let (out, err) = (read(Box::new(c.stdout.take().unwrap())), read(Box::new(c.stderr.take().unwrap())));
    // (the shared patience: a child that hangs — a broken hook parking for good — fails here, never the run)
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    let status = loop {
        if let Some(s) = c.try_wait().unwrap() {
            break Some(s);
        }
        if std::time::Instant::now() >= end {
            let _ = c.kill();
            let _ = c.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let (stdout, stderr) = (out.join().unwrap(), err.join().unwrap());
    assert!(status.is_some(), "{case}: the child hung past the shared patience, killed:\n{stdout}\n{stderr}");
    (status, stdout, stderr)
}

/// The child's planned panic is over: a failed check after it panics as a test does, never parked as a second panic.
fn planned_done() {
    AS_ABORT.store(false, Ordering::SeqCst);
    ENDS_TARAE.store(false, Ordering::SeqCst);
}

/// The child's state folder and root in `dir`.
fn child_places(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join("state"), dir.join("root"))
}

/// The reports a child left in `dir`.
fn reports_in(dir: &Path) -> Vec<PathBuf> {
    let at = reports(&child_places(dir).0);
    let mut out: Vec<PathBuf> = std::fs::read_dir(&at)
        .map(|r| {
            r.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "txt")).collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn stand_in_restore() -> std::io::Result<()> {
    eprintln!("<restored>");
    Ok(())
}

/// A file of the child's root, typed into and told to its keeper (its pause a minute: nothing written by itself) —
/// `slow`, each copy's write that long.
fn typed_into(root: &Path, state: &Path, slow: Duration) -> Editor {
    let path = root.join("kept.txt");
    std::fs::write(&path, "one\n").unwrap();
    let mut ed = Editor::new(Config::default());
    let mut r = crate::recovery::Recovery::new(state.join("recovery"), root.to_path_buf());
    r.quiet = Duration::from_secs(60);
    r.slow = slow;
    ed.recovery = Some(r);
    crate::recovery::for_crashes();
    ed.open_now(&path).unwrap();
    for c in "ityped ".chars() {
        ed.handle_key(Key::plain(Code::Char(c)));
    }
    ed.handle_key(Key::plain(Code::Esc));
    ed.recovery_sync();
    ed
}

fn went() -> Went {
    WENT.lock().unwrap().clone().expect("the hook went its way")
}

/// The children of the tests below: the hook begun as the start begins it, the terminal a stand-in's (`<restored>`), the
/// release build's way (`AS_ABORT`), every panic ending tarae; then `CASE_ENV`'s panic.
#[test]
#[ignore = "a child: the tests below run it in a process of their own"]
fn a_panic_in_its_own_process() {
    let Some(dir) = std::env::var_os(DIR_ENV).map(PathBuf::from) else { return };
    let case = std::env::var(CASE_ENV).unwrap();
    let (state, root) = child_places(&dir);
    std::fs::create_dir_all(&root).unwrap();
    begin(Some(&state), root.clone());
    terminal(stand_in_restore);
    AS_ABORT.store(true, Ordering::SeqCst);
    ENDS_TARAE.store(true, Ordering::SeqCst);
    let caught = |f: &dyn Fn()| std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err();
    match case.as_str() {
        "says" => {
            let mut ed = Editor::new(Config::default());
            ed.docs[0].text = ropey::Rope::from_str(&format!("{SECRET} {FAKE_TOKEN}\n"));
            assert!(caught(&|| panic!("a stand-in panic")));
            planned_done();
            let w = went();
            assert!(w.written && w.symbolized, "{w:?}");
        }
        "token" => {
            assert!(caught(&|| panic!("the api answered 401 for {FAKE_TOKEN}")));
            planned_done();
        }
        "unsaved" => {
            let _ed = typed_into(&root, &state, Duration::ZERO);
            assert!(caught(&|| panic!("a stand-in panic")));
            planned_done();
            assert_eq!(went().flushed, crate::recovery::Flushed::Kept(1));
        }
        "keeper" => {
            let mut ed = typed_into(&root, &state, Duration::ZERO);
            ed.recovery.as_mut().unwrap().keeper_panics();
            planned_done();
            assert_eq!(went().flushed, crate::recovery::Flushed::KeeperItself, "asked nothing: {:?}", went());
        }
        "slow" => {
            let _ed = typed_into(&root, &state, Duration::from_secs(3));
            assert!(caught(&|| panic!("a stand-in panic")));
            planned_done();
            let w = went();
            assert_eq!((w.flushed, w.written), (crate::recovery::Flushed::Unanswered, true), "{w:?}");
        }
        "twice" => {
            // (the second panic comes while the first's hook runs, and parks; the first's hook ends the process where
            // release aborts — never back into libtest, whose end would wait on the hook lock the parked one holds.
            // What's checked is the parent's: this process's words, its report, its end)
            EXITS.store(true, Ordering::SeqCst);
            let _ = DURING.set(|| {
                std::thread::spawn(|| panic!("the second"));
                let end = std::time::Instant::now() + crate::test_results::PATIENCE;
                while WAITED.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(5));
                }
                if WAITED.load(Ordering::SeqCst) == 1 {
                    use std::io::Write as _;
                    println!("the second panic parked");
                    let _ = std::io::stdout().flush();
                }
            });
            let _ = std::thread::spawn(|| panic!("the first")).join();
            unreachable!("the first panic's hook ends the process");
        }
        "keys" => {
            let path = root.join("typed.txt");
            std::fs::write(&path, "one\n").unwrap();
            let mut ed = Editor::new(Config::default());
            ed.open_now(&path).unwrap();
            assert!(std::thread::spawn(|| panic!("a worker's")).join().is_err());
            planned_done();
            assert!(ending());
            let keys = "ixyz".chars().map(|c| Key::plain(Code::Char(c))).chain([Key::plain(Code::Esc)]);
            let save = ":w".chars().map(|c| Key::plain(Code::Char(c))).chain([Key::plain(Code::Enter)]);
            for k in keys.chain(save) {
                // (the batch's end, a frame's time away — never a wait)
                let until = std::time::Instant::now() + crate::test_results::PATIENCE;
                crate::term::handle_batch(&mut ed, Event::Key(k), until);
            }
            assert_eq!(ed.doc().text.to_string(), "one\n", "no key reached the buffer");
            assert_eq!(ed.mode, crate::editor::Mode::Normal);
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\n", "no file written");
        }
        other => panic!("no case {other}"),
    }
}

/// #541: the terminal back first, then three lines of tarae's own — where and which build, (nothing unsaved: no second
/// line), the report — and no default message; the report has every field in order, its frames symbolized and none of
/// the environment or the buffer's text in it, nor in the marker.
#[test]
fn a_panic_says_where_and_keeps_a_report() {
    let dir = crate::scratch::Scratch::new("crash-says");
    let stderr = child("says", dir.path());
    let reports = reports_in(dir.path());
    assert_eq!(reports.len(), 1, "one report: {reports:?}\n{stderr}");
    let text = std::fs::read_to_string(&reports[0]).unwrap();
    let r = report::read(&text);
    let at = r.field("at");
    assert!(at.starts_with("src/crash/tests.rs:"), "{text}");
    let home = crate::allow::home();
    let lines = format!(
        "<restored>\ntarae crashed · {at} · build {}\nreport: {}\n",
        build_said(tarae_wire::BUILD),
        tilde(&reports[0], home.as_deref())
    );
    assert!(stderr.contains(&lines), "the terminal back, then its lines:\n{stderr}");
    assert!(
        !stderr.contains("panicked at") && !stderr.contains("RUST_BACKTRACE"),
        "no default message:\n{stderr}"
    );
    let keys: Vec<&str> = r.fields.iter().map(|f| f.0.as_str()).collect();
    let fixed = [
        "when", "build", "binary", "os", "pid", "thread", "at", "message", "root", "drawing", "unsaved",
        "symbols",
    ];
    assert_eq!(keys, fixed, "{text}");
    assert_eq!((r.field("message"), r.field("unsaved")), ("a stand-in panic", "none"));
    assert!(r.heading.starts_with("symbolized at the crash, in "), "{text}");
    assert!(text.contains("crash::tests::a_panic_in_its_own_process"), "its frames:\n{text}");
    // (the first frame kept is tarae's own: where the panic came, not std's machinery)
    assert!(
        r.frames.get(1).is_some_and(|l| l.contains("src/crash/tests.rs:")),
        "tarae's frame first:\n{text}"
    );
    let marker =
        std::fs::read_to_string(file(&child_places(dir.path()).0, &child_places(dir.path()).1)).unwrap();
    let path = std::env::var("PATH").unwrap_or_default();
    for (what, kept) in [("the report", &text), ("the marker", &marker), ("the lines", &stderr)] {
        for secret in [FAKE_TOKEN, SECRET] {
            assert!(!kept.contains(secret), "{what} holds {secret}:\n{kept}");
        }
        assert!(path.len() < 8 || !kept.contains(&path), "{what} holds the environment's PATH");
    }
    if let Some(h) = home.filter(|h| h.as_os_str().len() > 1) {
        assert!(!text.contains(&format!("{}/", h.display())), "home as ~:\n{text}");
    }
    assert!(marker.contains("\"report\""), "the marker names its report: {marker}");
}

/// The other way a secret comes: in the panic's own message — redacted in the report, the marker, and the next start's
/// log line.
#[test]
fn a_token_in_the_panics_message_is_redacted() {
    let dir = crate::scratch::Scratch::new("crash-token");
    let stderr = child("token", dir.path());
    let reports = reports_in(dir.path());
    let text = std::fs::read_to_string(&reports[0]).unwrap();
    assert!(text.contains("message   the api answered 401 for "), "its words kept:\n{text}");
    let (state, root) = child_places(dir.path());
    let marker = file(&state, &root);
    let kept = std::fs::read_to_string(&marker).unwrap();
    let log = crate::log::capture();
    let mut ed = Editor::new(Config::default());
    start(&mut ed, take(&marker).unwrap());
    let logged = log.lines().join("\n");
    assert!(logged.contains("the api answered 401 for"), "the start's log line: {logged}");
    for (what, s) in
        [("the report", &text), ("the marker", &kept), ("the log", &logged), ("the lines", &stderr)]
    {
        assert!(!s.contains(FAKE_TOKEN) && !s.contains("tarae0fake0token"), "{what} holds the token:\n{s}");
    }
}

/// #542: what the keeper hadn't written yet is written before tarae ends, and the second line says so and where.
#[test]
fn a_panic_writes_the_unsaved_edits_waiting() {
    let dir = crate::scratch::Scratch::new("crash-unsaved");
    let stderr = child("unsaved", dir.path());
    let (state, root) = child_places(dir.path());
    let copies: Vec<String> = std::fs::read_dir(state.join("recovery"))
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|run| std::fs::read_dir(run.path()).into_iter().flatten().flatten())
        .filter(|e| e.path().extension().is_some_and(|x| x == "txt"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
        .collect();
    assert!(copies.iter().any(|c| c.contains("typed one")), "the copy written: {copies:?}");
    let home = crate::allow::home();
    let line = format!(
        "unsaved changes in 1 file are kept — start tarae in {} and it offers them\n",
        tilde(&root, home.as_deref())
    );
    assert!(stderr.contains(&line), "its line:\n{stderr}");
    let text = std::fs::read_to_string(&reports_in(dir.path())[0]).unwrap();
    assert!(text.contains("unsaved   1 file kept\n"), "{text}");
}

/// The keeper is the thread that panicked: it's asked nothing (a record, not a timing — asked, it couldn't answer and
/// the wait would run out); a keeper too slow to answer within the wait: its copies as of their last pause, said so.
#[test]
fn a_panic_on_the_keepers_thread_waits_for_nothing() {
    let most = crate::recovery::MOST.as_secs();
    for case in ["keeper", "slow"] {
        let dir = crate::scratch::Scratch::new("crash-keeper");
        let stderr = child(case, dir.path());
        let line = format!("unsaved changes are kept as they were up to {most} s before — start tarae in ");
        assert!(stderr.contains(&line), "{case}:\n{stderr}");
    }
}

/// Two panics at once: the second comes while the first's hook runs and parks; the first's hook does the work and ends
/// the process (the abort's stand-in) with the second still parked — one report, one set of lines.
#[test]
fn two_panics_at_once_leave_one_report() {
    let dir = crate::scratch::Scratch::new("crash-twice");
    let (status, stdout, stderr) = child_run("twice", dir.path());
    assert!(
        status.is_some_and(|s| s.success()),
        "the first's hook ended it ({status:?}):\n{stdout}\n{stderr}"
    );
    assert!(stdout.contains("the second panic parked"), "the second came during the first's hook:\n{stdout}");
    assert_eq!(stderr.matches("tarae crashed · ").count(), 1, "{stderr}");
    assert!(!stderr.contains("the second"), "the second said nothing — parked till the end:\n{stderr}");
    assert_eq!(reports_in(dir.path()).len(), 1, "the first's report only");
}

/// A worker's panic gives the terminal back while the main loop lives on for the hook's wait: keys typed then change no
/// buffer and write no file.
#[test]
fn keys_after_a_panic_change_nothing() {
    let dir = crate::scratch::Scratch::new("crash-keys");
    child("keys", dir.path());
}

/// The three lines' words, each form.
#[test]
fn a_panics_lines_say_where_what_was_kept_and_the_report() {
    use crate::recovery::Flushed;
    let home = Some(Path::new("/h"));
    let root = Path::new("/h/Workspace/tarae");
    let report =
        Written::At(PathBuf::from("/h/.local/state/tarae/crashed/reports/20261112-041233-41213.txt"));
    let said = |f, r: &Written| lines("src/editdiff.rs:458", "0.1.0 87c40303", f, root, r, home);
    assert_eq!(
        said(Flushed::Kept(2), &report),
        [
            "tarae crashed · src/editdiff.rs:458 · build 87c40303",
            "unsaved changes in 2 files are kept — start tarae in ~/Workspace/tarae and it offers them",
            "report: ~/.local/state/tarae/crashed/reports/20261112-041233-41213.txt",
        ]
    );
    assert_eq!(
        said(Flushed::Kept(1), &report)[1],
        "unsaved changes in 1 file are kept — start tarae in ~/Workspace/tarae and it offers them"
    );
    for f in [Flushed::Unanswered, Flushed::KeeperItself] {
        assert_eq!(
            said(f, &report)[1],
            "unsaved changes are kept as they were up to 10 s before — start tarae in ~/Workspace/tarae and it offers them"
        );
    }
    assert_eq!(said(Flushed::Nothing, &report).len(), 2, "nothing kept: no second line");
    let third = |r: Written| said(Flushed::Nothing, &r)[1].clone();
    assert_eq!(
        third(Written::Failed("No space left on device (os error 28)".into())),
        "report not written: No space left on device (os error 28)"
    );
    assert_eq!(third(Written::Late), "report not written in 2 s");
    assert_eq!(third(Written::NoState), "no report (no state folder)");
}

/// A report as a crash in `root` at `when` would leave it, its unsaved field `unsaved`.
fn left(
    dir: &Path,
    root: &str,
    when: std::time::SystemTime,
    pid: u32,
    unsaved: crate::recovery::Flushed,
) -> PathBuf {
    let r = report::Report {
        crashed: crashed(None),
        when,
        pid,
        running: Some(crate::dsyms::Running { load: 0x1000, uuid: None }),
        symbols: None,
        root: PathBuf::from(root),
        unsaved,
        frames: report::Frames::Addresses(vec![0x1234, 0x5678]),
    };
    let path = report::write_new(dir, &r, Some(Path::new("/h"))).unwrap();
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
    path
}

/// The next start, in any folder: an unseen report said once — the crash, the unsaved files, where, the report last —
/// three said as three; the one this folder's marker named passed over (said with the marker).
#[test]
fn a_report_left_is_said_at_the_next_start_anywhere() {
    use crate::recovery::Flushed;
    let dir = crate::scratch::Scratch::new("crash-reports");
    let reports = dir.join("reports");
    let now = std::time::SystemTime::now();
    let hours = |h: u64| now - Duration::from_secs(h * 3600);
    let home = Some(Path::new("/h"));
    // (said, then marked seen — as the start's job does once its warning is up)
    let scan = |skip: Option<&Path>| {
        let s = report::scan(&reports, skip, now, home);
        report::mark_seen(&s.said);
        s.words
    };
    let a = left(&reports, "/h/Workspace/tarae", hours(2), 7, Flushed::Kept(2));
    // (a start that ended before its warning was up — a crash loop's: the next says it again)
    assert!(report::scan(&reports, None, now, home).words.is_some());
    let words = scan(None);
    let name = a.file_name().unwrap().to_string_lossy().to_string();
    assert_eq!(
        words.as_deref(),
        Some(&*format!(
            "tarae crashed 2h ago in ~/Workspace/tarae — 2 unsaved files wait there · src/cut.rs:151, build 32018b82 · report {}",
            a.display()
        )),
    );
    assert!(name.ends_with(".txt"));
    assert_eq!(scan(None), None, "said once");
    for (h, pid) in [(5, 8), (4, 9), (3, 10)] {
        left(&reports, "/h/Workspace/tarae", hours(h), pid, Flushed::Unanswered);
    }
    let words = scan(None).unwrap();
    assert!(
        words.starts_with(
            "tarae crashed 3 times, last 3h ago in ~/Workspace/tarae — unsaved changes wait there · "
        ),
        "{words}"
    );
    let marked = left(&reports, "/h/w", hours(1), 11, Flushed::Nothing);
    assert_eq!(scan(Some(&marked)), None, "the marker's: said with it");
    let unseen = marked.with_extension(report::UNSEEN_EXT);
    assert!(!unseen.exists(), "and seen");
}

/// The newest `KEEP` reports kept — never an unseen one pruned, however old.
#[test]
fn the_newest_reports_are_kept_and_unseen_ones_never_pruned() {
    let dir = crate::scratch::Scratch::new("crash-prune");
    let reports = dir.join("reports");
    std::fs::create_dir_all(&reports).unwrap();
    for i in 0..12 {
        std::fs::write(reports.join(format!("20261101-0000{i:02}-1.txt")), "r").unwrap();
    }
    std::fs::write(reports.join("20261101-000000-1.unseen"), "").unwrap();
    let gone = report::prune(&reports);
    let names: Vec<String> =
        gone.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
    assert_eq!(names, ["20261101-000001-1.txt"], "the oldest seen one, the unseen one kept");
    let left = std::fs::read_dir(&reports)
        .unwrap()
        .filter(|e| e.as_ref().unwrap().path().extension().unwrap() == "txt");
    assert_eq!(left.count(), report::KEEP + 1);
}

/// A report with addresses only is symbolized at the next start from the symbols it names (`atos` — macOS, skipped where
/// there's none): this test's own frames, this binary its symbols.
#[test]
fn a_reports_addresses_are_symbolized_at_the_next_start() {
    if !cfg!(target_os = "macos") || !Path::new("/usr/bin/atos").exists() {
        return;
    }
    let dir = crate::scratch::Scratch::new("crash-atos");
    let reports = dir.join("reports");
    let r = report::Report {
        crashed: crashed(None),
        when: std::time::SystemTime::now(),
        pid: 1,
        running: crate::dsyms::running(),
        symbols: Some(std::env::current_exe().unwrap()),
        root: PathBuf::from("/w"),
        unsaved: crate::recovery::Flushed::Nothing,
        frames: report::Frames::Addresses(report::addresses()),
    };
    let path = report::write_new(&reports, &r, None).unwrap();
    let _ = report::scan(&reports, None, std::time::SystemTime::now(), None);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("backtrace symbolized at the next start, from `symbols`"), "{text}");
    assert!(
        text.contains("a_reports_addresses_are_symbolized_at_the_next_start"),
        "this test's frame named:\n{text}"
    );
}

/// The start symbolizes a waiting report before it keeps this build's dSYM and prunes the old ones (`start_work`): the
/// prune's stand-in, which takes the report's symbols away, finds the report symbolized already (macOS `atos`; skipped
/// where there's none). Its symbols: a clone of this test binary.
#[test]
fn a_waiting_report_is_symbolized_before_its_dsym_can_be_pruned() {
    if !cfg!(target_os = "macos") || !Path::new("/usr/bin/atos").exists() {
        return;
    }
    let dir = crate::scratch::Scratch::new("crash-order");
    let symbols = dir.join("tarae-symbols");
    // (a clone on APFS: `fs::copy` clones where it can)
    std::fs::copy(std::env::current_exe().unwrap(), &symbols).unwrap();
    let reports = dir.join("reports");
    let r = report::Report {
        crashed: crashed(None),
        when: std::time::SystemTime::now(),
        pid: 1,
        running: crate::dsyms::running(),
        symbols: Some(symbols.clone()),
        root: PathBuf::from("/w"),
        unsaved: crate::recovery::Flushed::Nothing,
        frames: report::Frames::Addresses(report::addresses()),
    };
    let path = report::write_new(&reports, &r, None).unwrap();
    let mut at_prune = String::new();
    report::start_work(Some(&reports), None, std::time::SystemTime::now(), None, || {
        at_prune = std::fs::read_to_string(&path).unwrap();
        // (the prune's stand-in: the symbols the report names are gone)
        std::fs::remove_file(&symbols).unwrap();
    });
    assert!(
        at_prune.contains("backtrace symbolized at the next start"),
        "symbolized before the prune:\n{at_prune}"
    );
}

/// `tarae --crash-test`'s crash is said as what it is, never as a real one: at the next start in its folder (the marker)
/// and in another (its report), alone or the last of several.
#[test]
fn a_crash_test_is_said_as_one() {
    let c = Crashed { message: ON_PURPOSE.into(), at: "src/crash.rs:360".into(), ..crashed(None) };
    assert_eq!(
        said_home(&c, None),
        "tarae crash-test, a panic on purpose · src/crash.rs:360, build 32018b82"
    );
    let now = std::time::SystemTime::now();
    let seen = report::Seen {
        path: PathBuf::from("/r/20261113-010000-7.txt"),
        when: now - Duration::from_secs(2 * 3600),
        root: "~/w".into(),
        at: "src/crash.rs:360".into(),
        build: "0.1.0 32018b82".into(),
        unsaved: "none".into(),
        message: ON_PURPOSE.into(),
    };
    assert_eq!(
        report::words(std::slice::from_ref(&seen), now, None).as_deref(),
        Some(
            "tarae crash-test, a panic on purpose, 2h ago in ~/w · src/crash.rs:360, build 32018b82 · report \
             /r/20261113-010000-7.txt"
        )
    );
    let three = report::words(&[seen.clone(), seen.clone(), seen], now, None).unwrap();
    assert!(three.starts_with("tarae crashed 3 times, the last a crash-test 2h ago in ~/w · "), "{three}");
}

/// std's frames trimmed: its own around the panic, and the panic's machinery and a dependency's frames before tarae's —
/// the first frame kept is tarae's own (its source in tarae's tree); with none of tarae's, only std's markers cut.
#[test]
fn the_first_frame_kept_is_taraes_own() {
    let own = "/w/tarae";
    let frame = |n: usize, name: &str, at: &str| {
        format!("  {n:>2}:        0x{n:x} - {name}\n                at {at}:{n}:1")
    };
    let text = |tarae: bool| {
        let (a, b) = match tarae {
            true => ("/w/tarae/src/editdiff.rs", "/w/tarae/src/main.rs"),
            false => ("/x/a.rs", "/x/b.rs"),
        };
        [
            frame(0, "std::backtrace::Backtrace::create", "/rustc/h/library/std/src/backtrace.rs"),
            frame(1, "tarae::crash::hook", "/w/tarae/src/crash.rs"),
            frame(
                2,
                "std::sys::backtrace::__rust_end_short_backtrace",
                "/rustc/h/library/std/src/sys/backtrace.rs",
            ),
            frame(3, "rust_begin_unwind", "/rustc/h/library/std/src/panicking.rs"),
            frame(4, "core::panicking::panic_fmt", "/rustc/h/library/core/src/panicking.rs"),
            frame(5, "ropey::slice::RopeSlice::char", "/h/.cargo/registry/src/ropey-1.6.1/src/slice.rs"),
            frame(6, "tarae::editdiff::cut_at", a),
            frame(7, "tarae::main", b),
            frame(
                8,
                "std::sys::backtrace::__rust_begin_short_backtrace",
                "/rustc/h/library/std/src/sys/backtrace.rs",
            ),
            frame(9, "std::rt::lang_start", "/rustc/h/library/std/src/rt.rs"),
        ]
        .join("\n")
    };
    let kept = report::trimmed_for(&text(true), own);
    let names: Vec<&str> = kept.lines().filter_map(|l| l.split(" - ").nth(1)).collect();
    assert_eq!(names, ["tarae::editdiff::cut_at", "tarae::main"], "{kept}");
    let kept = report::trimmed_for(&text(false), own);
    let names: Vec<&str> = kept.lines().filter_map(|l| l.split(" - ").nth(1)).collect();
    assert_eq!(
        names.first(),
        Some(&"rust_begin_unwind"),
        "no frame of tarae's: only std's markers cut — {kept}"
    );
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
