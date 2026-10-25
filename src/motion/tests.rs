use super::*;
use crate::key::{Code, Key};

/// Keys as written (`<esc>`, `<ret>` named), each an event as the loop hands it in.
fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let k: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_event(crate::event::Event::Key(k));
    }
}

fn editor(text: &str) -> Editor {
    let mut ed = Editor::new(crate::config::Config::default());
    ed.docs[0].text = ropey::Rope::from_str(text);
    ed
}

/// The flash on the document as it is now: its act and the text of each range.
fn flashed(ed: &Editor) -> Option<(Act, Vec<String>)> {
    let What::Flash { ranges, act, doc, version } = &ed.motions.last()?.what else { return None };
    if (*doc, *version) != (ed.doc().id, ed.doc().version()) {
        return None;
    }
    let text = &ed.doc().text;
    Some((*act, ranges.iter().map(|r| text.byte_slice(r.clone()).to_string()).collect()))
}

/// `u` after an edit at two cursors two lines apart: what each took back flashes, not the line between them; `U` the
/// same.
#[test]
fn undo_flashes_what_it_took_back_at_every_cursor() {
    let mut ed = editor("ab\ncd\nab\n");
    feed(&mut ed, "%sab<ret>cX<esc>");
    assert_eq!(ed.doc().text.to_string(), "X\ncd\nX\n");
    assert_eq!(flashed(&ed), None, "typing flashes nothing: you saw it");
    feed(&mut ed, "u");
    assert_eq!(ed.doc().text.to_string(), "ab\ncd\nab\n");
    assert_eq!(flashed(&ed), Some((Act::Undo, vec!["ab".into(), "ab".into()])));
    feed(&mut ed, "U");
    assert_eq!(flashed(&ed), Some((Act::Redo, vec!["X".into(), "X".into()])));
    // (lines it put in or took out, not one for one: the lines whole)
    let mut ed = editor("a\nb\nc\n");
    feed(&mut ed, "xdjjxdu");
    assert_eq!(flashed(&ed), Some((Act::Undo, vec!["c\n".into()])));
    feed(&mut ed, "u");
    assert_eq!(flashed(&ed), Some((Act::Undo, vec!["a\n".into()])));
    assert_eq!(ed.motions.len(), 1, "one flash a document: the newer takes its place");
    // (within one line: the span itself; what it only took out, the character where it was)
    let mut ed = editor("one two\n");
    feed(&mut ed, "ecsix<esc>u");
    assert_eq!(flashed(&ed), Some((Act::Undo, vec!["one".into()])));
    feed(&mut ed, "U");
    assert_eq!(flashed(&ed), Some((Act::Redo, vec!["six".into()])));
    let mut ed = editor("one two\n");
    feed(&mut ed, "ea!<esc>u");
    assert_eq!(flashed(&ed), Some((Act::Undo, vec![" ".into()])), "the join");
    // (nothing to undo: nothing flashes)
    let mut ed = editor("one\n");
    feed(&mut ed, "u");
    assert_eq!(flashed(&ed), None);
}

/// `.` at three cursors: what it typed again flashes at each, nothing between.
#[test]
fn repeat_flashes_what_it_typed_at_every_cursor() {
    let mut ed = editor("a\nb\nc\nd\ne\nf\n");
    feed(&mut ed, "iX<esc>");
    assert_eq!(flashed(&ed), None);
    feed(&mut ed, "jjjCC.");
    assert_eq!(ed.doc().text.to_string(), "Xa\nb\nc\ndX\neX\nfX\n");
    assert_eq!(flashed(&ed), Some((Act::Repeat, vec!["X".into(); 3])));
}

/// A yank flashes what went in; a paste in normal mode doesn't (what it put in is selected); one while inserting
/// does (its cursors end after it).
#[test]
fn a_yank_and_an_insert_paste_flash_a_normal_paste_does_not() {
    let mut ed = editor("one\ntwo\n");
    feed(&mut ed, "xy");
    assert_eq!(flashed(&ed), Some((Act::Yank, vec!["one\n".into()])));
    let mut ed = editor("one\ntwo\n");
    feed(&mut ed, "x\"_y");
    assert_eq!(flashed(&ed), None, "the black hole takes nothing in");
    feed(&mut ed, "xyjp");
    assert_eq!(flashed(&ed), None, "p selects what it put: nothing flashes");
    let mut ed = editor("ab\ncd\n");
    ed.registers.insert('"', vec!["ab\n".into()]);
    feed(&mut ed, "Ci");
    ed.handle_event(crate::event::Event::Paste("hey".into()));
    assert_eq!(ed.doc().text.to_string(), "heyab\nheycd\n");
    assert_eq!(flashed(&ed), Some((Act::Paste, vec!["hey".into(); 2])));
    feed(&mut ed, "<esc>i<C-r>\"");
    assert_eq!(flashed(&ed), Some((Act::Paste, vec!["ab\n".into(); 2])), "C-r too");
    feed(&mut ed, "z");
    assert_eq!(flashed(&ed).map(|f| f.0), None, "a key after it: the version moved, nothing drawn");
}

/// A yank's "yanked …" is a note only where a frame drew its flash: kept in `:messages` at once, no toast — to a
/// register and `space y` (once the clipboard took it) by the same rule; a frame that didn't draw it (not drawn: the
/// tests' default; the chat's transcript — chatdoc's view tests) toasts it after, once; one gone before any frame
/// drew it, as it goes; the black hole and `none` flash nothing — a toast at once.
#[test]
fn a_yank_is_a_note_where_its_flash_is_drawn_else_a_toast() {
    let toasts = |ed: &Editor| ed.toasts.iter().map(|t| t.text.clone()).collect::<Vec<_>>();
    let last = |ed: &Editor| ed.messages.last().map(|m| m.2.clone()).unwrap_or_default();
    let frame = |ed: &mut Editor| crate::term::render(ed, &mut Vec::new(), 80, 12).unwrap();
    let mut ed = editor("one\ntwo\n");
    ed.motion_drawn = true;
    feed(&mut ed, "xy");
    assert_eq!((last(&ed), toasts(&ed)), ("yanked 1 selection".into(), vec![]), "kept at once, no toast yet");
    frame(&mut ed);
    assert_eq!(toasts(&ed), Vec::<String>::new(), "drawn: a note");
    feed(&mut ed, "\"ay");
    frame(&mut ed);
    assert_eq!((last(&ed), toasts(&ed)), ("yanked 1 selection to register a".into(), vec![]));
    feed(&mut ed, " y");
    let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the copy's result");
    ed.handle_event(ev);
    frame(&mut ed);
    assert_eq!((last(&ed), toasts(&ed)), ("copied 1 selection".into(), vec![]), "space y: the same rule");
    feed(&mut ed, "\"_y");
    assert_eq!(toasts(&ed), ["yanked 1 selection to register _"], "nothing flashed: a toast at once");
    // (not drawn: the frame after it toasts it, the words kept once)
    let mut ed = editor("one\ntwo\n");
    feed(&mut ed, "xy");
    frame(&mut ed);
    assert_eq!(toasts(&ed), ["yanked 1 selection"], "not drawn: a toast");
    assert_eq!(ed.messages.iter().filter(|m| m.2 == "yanked 1 selection").count(), 1, "kept once");
    // (gone before a frame: another flash on it took its place)
    let mut ed = editor("one\ntwo\n");
    ed.motion_drawn = true;
    feed(&mut ed, "xyjxdu");
    assert_eq!(toasts(&ed), ["yanked 1 selection"], "replaced undrawn: a toast as it goes");
    // (`none`: no flash, a toast at once)
    let mut ed = editor("one\ntwo\n");
    crate::typed::execute(&mut ed, "set editor.motion none").unwrap();
    ed.toasts.clear();
    feed(&mut ed, "xy");
    assert_eq!((toasts(&ed), ed.motions.len()), (vec!["yanked 1 selection".to_string()], 0), "none: a toast");
}

/// §5: answering a two-change review's first change presses its head's stamp — lit on the key's frame, drying at its
/// steps (1 · .44 · .11), dry at its end; the head back in hand is lit, never pressed; the press wakes on its own clock
/// and ends; the second answer ends the review. At `calm` and `none` no press: dry at once.
#[test]
fn an_answered_change_presses_its_stamp_and_it_dries() {
    let seen = |ed: &mut Editor| {
        crate::term::MOTION_SEEN.with_borrow_mut(Vec::clear);
        crate::term::render(ed, &mut Vec::new(), 100, 20).unwrap();
        crate::term::MOTION_SEEN.with_borrow(|s| {
            s.iter().filter(|(k, ..)| *k == "press").map(|(_, at, g)| (*at, g.clone())).collect::<Vec<_>>()
        })
    };
    let review = |ed: &mut Editor| {
        let id = ed.doc().id;
        let changes = vec![(0, 3, "one".into(), "ONE".into()), (8, 13, "three".into(), "THREE".into())];
        crate::llm::review_with(ed, id, "upper", changes, None);
    };
    let mut ed = editor("one\ntwo\nthree\n");
    ed.motion_drawn = true;
    let head = Place::Head { doc: ed.doc().id, change: 0 };
    crate::clock::hold();
    review(&mut ed);
    feed(&mut ed, "y");
    let presses: Vec<What> = ed.motions.iter().map(|m| m.what.clone()).collect();
    assert_eq!(presses, [What::Press(head.clone())], "y: the first head pressed, the review on the second");
    assert_eq!(ed.pressed(&head), 1.0, "lit on the key's frame");
    let ms = |d: Option<Duration>| d.map(|d| d.as_millis());
    assert_eq!(ms(ed.motion_wake()), Some(102), "its first step a TICK on");
    let mut inks = Vec::new();
    for (at, k) in [(100, 44), (200, 11)] {
        ed.motion_at = Some(Duration::from_millis(at));
        let s = seen(&mut ed);
        assert!(
            matches!(&s[..], [(n, ink)] if *n == k && ink.starts_with("y Some(Rgb")),
            "at {at} ms: {s:?}"
        );
        inks.push(s[0].1.clone());
    }
    assert_ne!(inks[0], inks[1], "the ink dries, step by step");
    ed.motion_at = Some(PRESS);
    assert_eq!((seen(&mut ed), ed.pressed(&head)), (vec![], 0.0), "dry at its end");
    ed.motion_at = Some(Duration::from_millis(100));
    ed.workspace.claude.review.as_mut().unwrap().current = 0;
    assert_eq!(seen(&mut ed), [], "back in hand: lit, not pressed");
    ed.workspace.claude.review.as_mut().unwrap().current = 1;
    ed.motion_at = None;
    crate::clock::advance(PRESS);
    assert_eq!(ms(ed.motion_wake()), Some(0), "over: the frame that draws it dry");
    ed.schedule_tick();
    assert_eq!((ed.motions.len(), ed.motion_wake()), (0, None), "and nothing more");
    feed(&mut ed, "n");
    assert!(ed.workspace.claude.review.is_none(), "the second answer ends it");
    crate::clock::release();
    for level in ["calm", "none"] {
        let mut ed = editor("one\ntwo\nthree\n");
        crate::typed::execute(&mut ed, &format!("set editor.motion {level}")).unwrap();
        review(&mut ed);
        feed(&mut ed, "y");
        assert!(ed.motions.is_empty(), "{level}: no press");
    }
}

/// §5: a change's knot row stamped by your review's keys (`chat::stamp_call`) is pressed — no keys, none; a file
/// outside a turn review presses nothing (`codereview::press_turn_file`: only a turn's files are stamped).
#[test]
fn a_knot_stamped_by_your_keys_is_pressed() {
    let mut ed = editor("one\n");
    ed.motion_drawn = true;
    let mut c = crate::chat::Chat::new();
    let mut m = crate::chat::Msg::new(crate::chat::Role::Tool, "edit", "");
    m.call = "c1".into();
    c.msgs.push(m);
    ed.workspace.claude.chat = Some(c);
    crate::chat::stamp_call(&mut ed, "c1", &[]);
    assert!(ed.motions.is_empty(), "no keys: nothing pressed");
    crate::chat::stamp_call(&mut ed, "c1", &[crate::stamp::Stamp('y')]);
    assert_eq!(ed.pressed(&Place::Knot("c1".into())), 1.0, "pressed, lit on its frame");
    crate::codereview::press_turn_file(&mut ed, std::path::Path::new("/nowhere/a.rs"));
    assert_eq!(ed.motions.len(), 1, "no turn review: no rail stamp to press");
}

/// What a flash draws: at its peak every cell of its ranges, at `calm` the same, at `none` nothing; past its time
/// nothing; the accent over the ground, dried by the step.
#[test]
fn a_flash_is_drawn_on_its_cells_and_dries() {
    let cells = |ed: &mut Editor| {
        crate::term::MOTION_SEEN.with_borrow_mut(Vec::clear);
        crate::term::render(ed, &mut Vec::new(), 80, 12).unwrap();
        crate::term::MOTION_SEEN.with_borrow(|s| s.iter().map(|(_, _, g)| g.as_str()).collect::<String>())
    };
    let mut ed = editor("one two\n");
    feed(&mut ed, "ecsix<esc>u");
    assert_eq!(cells(&mut ed), "", "not drawn in tests unless turned on");
    ed.motion_drawn = true;
    assert_eq!(cells(&mut ed), "one");
    crate::typed::execute(&mut ed, "set editor.motion calm").unwrap();
    assert_eq!(cells(&mut ed), "one", "calm: whole");
    crate::typed::execute(&mut ed, "set editor.motion none").unwrap();
    assert_eq!(cells(&mut ed), "");
    crate::typed::execute(&mut ed, "set editor.motion full").unwrap();
    ed.motion_at = Some(Duration::from_millis(100));
    assert_eq!(cells(&mut ed), "one", "its step after the peak");
    ed.motion_at = Some(FLASH);
    assert_eq!(cells(&mut ed), "", "gone at its end");
    // (the drying: the accent's mix at the peak, near half of it a step on, the ground at the end)
    let m = &ed.motions[0];
    assert_eq!(strength(Duration::ZERO, m.life), 1.0);
    assert_eq!((strength(Duration::from_millis(150), m.life) * 100.0).round(), 44.0, "read at its step");
    assert_eq!(strength(FLASH, m.life), 0.0);
}

/// §2: the lines a write left — within one line that line, across lines each hunk's new lines whole (not the ones
/// between), what it only took out none.
#[test]
fn a_writes_wet_lines_are_the_lines_it_left() {
    let lines = |old: &str, new: &str| -> Vec<String> {
        let n = ropey::Rope::from_str(new);
        written(&ropey::Rope::from_str(old), &n).iter().map(|r| n.byte_slice(r.clone()).to_string()).collect()
    };
    assert_eq!(lines("a\nb\nc\n", "a\nB!\nc\n"), ["B!\n"]);
    assert_eq!(lines("a\nb\nc\nd\ne\n", "A\nb\nc\nd\nE\n"), ["A\n", "E\n"], "not the lines between");
    assert_eq!(lines("a\nb\nc\n", "a\nb\nx\ny\nc\n"), ["x\ny\n"]);
    assert_eq!(lines("a\nb", "a\nbc"), ["bc"], "the last line, no newline after it");
    assert!(lines("a\nb\nc\n", "a\nc\n").is_empty(), "a pure deletion: no ink left");
    assert!(lines("one two\n", "one\n").is_empty());
    assert!(lines("a\n", "a\n").is_empty());
}

/// The lines of the current document's wet record, as text.
fn wet(ed: &Editor) -> Vec<String> {
    let d = ed.doc();
    d.wet.iter().flatten().map(|r| d.text.byte_slice(r.clone()).to_string()).collect()
}

/// §2, drawn: Claude's lines landed unwatched are on the change's green over their ground, drying (not drawn in tests
/// unless turned on); where the edit is shown (`Document::shown`) its own wash lands deeper instead, never a second;
/// dry at its end, let go with its lines by the tick, its wakes a `TICK` step of its own clock, then none; your edits
/// since move its lines along; at `calm` and `none` no wet ink, nor for a pure deletion.
#[test]
fn claudes_fresh_writing_dries() {
    let seen = |ed: &mut Editor| -> Vec<(usize, String)> {
        crate::term::MOTION_SEEN.with_borrow_mut(Vec::clear);
        crate::term::render(ed, &mut Vec::new(), 80, 12).unwrap();
        crate::term::MOTION_SEEN.with_borrow(|s| {
            s.iter().filter(|(k, ..)| *k == "wet").map(|(_, l, how)| (*l, how.clone())).collect()
        })
    };
    let how = |s: &[(usize, String)]| -> Vec<(usize, String)> {
        s.iter().map(|(l, h)| (*l, h.split(' ').next().unwrap_or_default().to_string())).collect()
    };
    let land = |ed: &mut Editor, after: &str| {
        let before = ed.doc().text.clone();
        crate::disk::reload_text(ed.doc_mut(), &ropey::Rope::from_str(after));
        let id = ed.doc().id;
        ed.wet(id, &before);
    };
    let mut ed = editor("a\nb\nc\nd\n");
    crate::clock::hold();
    land(&mut ed, "a\nB\nc\nD\n");
    assert_eq!(wet(&ed), ["B\n", "D\n"]);
    assert!(seen(&mut ed).is_empty(), "not drawn in tests unless turned on");
    ed.motion_drawn = true;
    let landed = seen(&mut ed);
    assert_eq!(how(&landed), [(1, "wash".into()), (3, "wash".into())], "the green over its lines alone");
    assert_eq!(ed.motion_wake().map(|d| d.as_millis()), Some(102), "a full TICK on");
    crate::clock::advance(Duration::from_millis(300));
    let later = seen(&mut ed);
    assert_eq!(how(&later), how(&landed));
    assert_ne!(later[0].1, landed[0].1, "drying");
    assert_eq!(ed.motion_wake().map(|d| d.as_millis()), Some(102), "on its own clock");
    // (shown: its wash deeper, not a second one; drying too)
    let hunk = |l: usize, was: &str, now: &str| crate::replay::Hunk {
        lines: l..l + 1,
        removed: vec![was.into()],
        added: vec![now.into()],
    };
    ed.doc_mut().shown = Some(vec![hunk(1, "b", "B"), hunk(3, "d", "D")]);
    let deeper = seen(&mut ed);
    assert_eq!(
        how(&deeper),
        [(1, "deeper".into()), (3, "deeper".into())],
        "the shown wash, deeper: one sign"
    );
    crate::clock::advance(Duration::from_millis(100));
    assert_ne!(seen(&mut ed)[0].1, deeper[0].1, "drying to its usual");
    ed.doc_mut().shown = None;
    // (dry at its end: nothing drawn, one wake to draw it gone, let go with its lines)
    crate::clock::advance(Duration::from_millis(400));
    assert!(seen(&mut ed).is_empty(), "dry at its end");
    assert_eq!(ed.motion_wake(), Some(Duration::ZERO));
    ed.schedule_tick();
    assert!(ed.motions.is_empty() && ed.doc().wet.is_none(), "let go with its lines");
    assert_eq!(ed.motion_wake(), None, "the wake ends");
    // (your edit since moves its lines along; a newer write takes its place; a pure deletion leaves none)
    land(&mut ed, "a\nB\nc\nD!\n");
    feed(&mut ed, "ggOnew<esc>");
    assert_eq!(wet(&ed), ["D!\n"]);
    land(&mut ed, "new\na\nB?\nc\nD!\n");
    assert_eq!((wet(&ed), ed.motions.len()), (vec!["B?\n".to_string()], 1), "the newer write in its place");
    ed.schedule_tick();
    crate::clock::advance(Duration::from_secs(1));
    ed.schedule_tick();
    land(&mut ed, "new\na\nc\nD!\n");
    assert!(ed.doc().wet.is_none() && ed.motions.is_empty(), "a pure deletion: no ink");
    land(&mut ed, "undo me\nnew\na\nc\nD!\n");
    feed(&mut ed, "u");
    assert!(ed.doc().wet.is_none(), "undone: its lines went (undo's flash says what it took back)");
    ed.schedule_tick();
    assert!(!ed.motions.iter().any(|m| matches!(m.what, What::Wet(_))), "its drying with them, at the tick");
    for m in ["calm", "none"] {
        crate::typed::execute(&mut ed, &format!("set editor.motion {m}")).unwrap();
        land(&mut ed, &format!("{m}\nnew\na\nc\nD!\n"));
        let drying = ed.motions.iter().any(|m| matches!(m.what, What::Wet(_)));
        assert!(ed.doc().wet.is_none() && !drying, "{m}: no wet ink");
    }
    crate::clock::release();
}

/// §2: a run of Claude's that rewrote an open file — its buffer wet as it takes the run's text (the disk watch's
/// reload, a version later), now where it has already; one with your unsaved edits, or that took another text, none.
#[test]
fn a_run_that_rewrote_an_open_file_dries_as_it_reloads() {
    let (was, now) = ("fn a() {}\nfn b() {}\n", "fn a() {}\nfn b() { 1 }\n");
    let path = std::path::PathBuf::from("/nowhere/run/a.rs");
    let blob = |t: &str| Some(crate::snapshot::Blob { id: String::new(), text: Some(t.into()) });
    let change = crate::snapshot::Change { path: path.clone(), before: blob(was), after: blob(now) };
    let at = |text: &str| {
        let mut ed = editor(text);
        ed.doc_mut().path = Some(path.clone());
        ed
    };
    let mut ed = at(was);
    ed.run_wrote(std::slice::from_ref(&change));
    crate::motion::wet_sync(&mut ed);
    assert!(ed.doc().wet.is_none() && ed.wet_waiting.len() == 1, "waiting for the buffer to take it");
    crate::disk::reload_text(ed.doc_mut(), &ropey::Rope::from_str(now));
    crate::motion::wet_sync(&mut ed);
    assert_eq!((wet(&ed), ed.wet_waiting.len()), (vec!["fn b() { 1 }\n".to_string()], 0));
    let mut ed = at(now);
    ed.run_wrote(std::slice::from_ref(&change));
    assert_eq!(wet(&ed), ["fn b() { 1 }\n"], "taken already: wet now");
    let mut ed = at(was);
    feed(&mut ed, "ix<esc>");
    ed.run_wrote(std::slice::from_ref(&change));
    assert!(ed.wet_waiting.is_empty(), "your unsaved edits: the buffer takes nothing");
    let mut ed = at(was);
    ed.run_wrote(std::slice::from_ref(&change));
    crate::disk::reload_text(ed.doc_mut(), &ropey::Rope::from_str("other\n"));
    crate::motion::wet_sync(&mut ed);
    assert!(ed.doc().wet.is_none() && ed.wet_waiting.is_empty(), "another text: let go");
}

/// §2, wired: a run of Claude's that rewrote an open file (as `cargo fmt` does), through `tools::run_changes` — its
/// result first, the buffer still as it was: waiting, nothing wet; then the disk watch's round (focus back) reloads
/// it, and the lines the run left are wet. (The repository and its file are the test's own.)
#[test]
fn a_runs_rewrite_of_an_open_file_dries_as_the_watch_reloads_it() {
    let s = crate::scratch::Scratch::new("motion-run-wet");
    let dir = std::fs::canonicalize(s.path()).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"])
            .args(["-c", "user.email=t@example.invalid", "-c", "user.name=t"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    let file = dir.join("a.rs");
    std::fs::write(&file, "fn a() {}\nfn b()  {  }\nfn c() {}\n").unwrap();
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "base"]);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = dir.clone();
    ed.open(&file).unwrap();
    let pump = |ed: &mut Editor, what: &str, done: &dyn Fn(&Editor) -> bool| {
        let end = std::time::Instant::now() + crate::test_results::PATIENCE;
        while !done(ed) {
            assert!(std::time::Instant::now() < end, "{what}");
            if let Some(ev) = ed.events.recv_timeout(Duration::from_millis(20)) {
                ed.handle_event(ev);
            }
        }
    };
    // (the run: its snapshot before, the file rewritten as a formatter would)
    let snap = crate::snapshot::Before::take(&dir);
    std::fs::write(&file, "fn a() {}\nfn b() {}\nfn c() {}\n").unwrap();
    let told = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let t = told.clone();
    crate::tools::run_changes(&mut ed, snap, 0, "cargo fmt".into(), move |_, _| {
        t.store(true, std::sync::atomic::Ordering::Relaxed)
    });
    pump(&mut ed, "the run's result", &|_| told.load(std::sync::atomic::Ordering::Relaxed));
    assert_eq!(ed.doc().text.to_string(), "fn a() {}\nfn b()  {  }\nfn c() {}\n", "not reloaded yet");
    assert!(ed.doc().wet.is_none() && ed.wet_waiting.len() == 1, "waiting for the buffer to take it");
    // (the watch's round: focus back)
    ed.handle_event(crate::event::Event::Focus(true));
    pump(&mut ed, "the reload", &|ed| ed.doc().text == "fn a() {}\nfn b() {}\nfn c() {}\n");
    assert_eq!((wet(&ed), ed.wet_waiting.len()), (vec!["fn b() {}\n".to_string()], 0), "wet as it reloaded");
}

/// The curve: an ease-out at whole steps, over under `GONE`.
#[test]
fn the_drying_eases_out_and_ends() {
    let s = Duration::from_secs(1);
    let steps: Vec<f32> = (0..10).map(|k| (strength(TICK * k, s) * 100.0).round() / 100.0).collect();
    assert_eq!(steps, [1.0, 0.81, 0.64, 0.49, 0.36, 0.25, 0.16, 0.09, 0.0, 0.0]);
    assert_eq!(shown_for(s), Duration::from_millis(800));
    assert_eq!(shown_for(FLASH), FLASH);
    assert_eq!(shown_for(Duration::from_millis(300)), Duration::from_millis(300));
}

/// The wakes: a frame each step on the flash's own clock, one as it goes, then none — at `calm` one, at `none`
/// and pinned (`TARAE_MOTION_AT`) none.
#[test]
fn a_flash_wakes_on_its_steps_then_never() {
    let mut ed = editor("one two\n");
    crate::clock::hold();
    feed(&mut ed, "ecsix<esc>u");
    let ms = |d: Option<Duration>| d.map(|d| d.as_millis());
    assert_eq!(ms(ed.motion_wake()), Some(102), "a full TICK after the key");
    crate::clock::advance(Duration::from_millis(40));
    assert_eq!(ms(ed.motion_wake()), Some(62), "on its own clock, not the wave's");
    crate::clock::advance(Duration::from_millis(62));
    assert_eq!(ms(ed.motion_wake()), Some(100));
    crate::clock::advance(Duration::from_millis(100));
    assert_eq!(ms(ed.motion_wake()), Some(100));
    crate::clock::advance(Duration::from_millis(100));
    assert_eq!(ms(ed.motion_wake()), Some(0), "over: the frame that draws it gone");
    ed.schedule_tick();
    assert!(ed.motions.is_empty(), "let go");
    assert!(!ed.unchanged, "its going is drawn");
    assert_eq!(ed.motion_wake(), None, "and nothing more");
    feed(&mut ed, "U");
    crate::typed::execute(&mut ed, "set editor.motion calm").unwrap();
    assert_eq!(ms(ed.motion_wake()), Some(302), "calm: once, as it goes");
    crate::typed::execute(&mut ed, "set editor.motion none").unwrap();
    assert_eq!(ms(ed.motion_wake()), Some(302), "none: no steps, one wake as it goes");
    crate::typed::execute(&mut ed, "set editor.motion full").unwrap();
    ed.motion_at = Some(Duration::ZERO);
    assert_eq!(ed.motion_wake(), None, "pinned: no frames");
    crate::clock::release();
}

/// Nothing stays listed past its time: at `none` no flash is made (the state's `motions` empty, no wake); one made
/// before a switch to `none`, or while unfocused, asks one wake at its end and is let go by it — listed nowhere when
/// focus comes back.
#[test]
fn a_flash_never_outlives_its_time_in_the_state() {
    let listed = |ed: &Editor| ed.ui_state().motions.len();
    let mut ed = editor("one two\n");
    crate::clock::hold();
    crate::typed::execute(&mut ed, "set editor.motion none").unwrap();
    feed(&mut ed, "ecsix<esc>u");
    assert_eq!((listed(&ed), ed.motion_wake()), (0, None), "none: nothing made");
    crate::typed::execute(&mut ed, "set editor.motion full").unwrap();
    feed(&mut ed, "U");
    assert_eq!(listed(&ed), 1);
    ed.unfocused = true;
    let ms = |d: Option<Duration>| d.map(|d| d.as_millis());
    assert_eq!(ms(ed.motion_wake()), Some(302), "unfocused: one wake, at its end");
    crate::clock::advance(Duration::from_millis(302));
    ed.schedule_tick();
    ed.unfocused = false;
    assert_eq!((listed(&ed), ed.motion_wake()), (0, None), "let go by its time, none when focus is back");
    feed(&mut ed, "u");
    crate::typed::execute(&mut ed, "set editor.motion none").unwrap();
    crate::clock::advance(Duration::from_millis(100));
    assert_eq!(ms(ed.motion_wake()), Some(202), "made before `none`: one wake at its end");
    crate::clock::advance(Duration::from_millis(202));
    ed.schedule_tick();
    assert_eq!(listed(&ed), 0);
    crate::clock::release();
}
