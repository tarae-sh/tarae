//! #196: a place in the chat's text opens what it looks like it opens — a name given alone found in the project (one
//! file: it; several: their picker; none: not drawn as a place), and a path the wrap breaks one place on every row it's
//! drawn on, whatever the chat's width.

use super::*;
use crate::chat::{Chat, Msg, Role};
use crate::config::Config;
use crate::event::{Mouse, MouseKind};

const H: u16 = 34;

/// A project of the test's own: its files, each a few lines.
fn project(name: &str, files: &[&str]) -> (crate::scratch::Scratch, PathBuf) {
    let scratch = crate::scratch::Scratch::new(name);
    let root = std::fs::canonicalize(&*scratch).unwrap();
    for f in files {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "one\ntwo\nthree\n").unwrap();
    }
    (scratch, root)
}

/// The rule itself: as written from the first base it's a file in; else the files that end with it by whole parts —
/// one it, several listed, none not; a name with `..`, one that doesn't look like a file and an absolute one not
/// there are never looked for.
#[test]
fn a_name_is_found_as_written_else_by_its_last_parts() {
    let (_s, root) =
        project("places-find", &["README.md", "src/main/dev/TestDevice.java", "a/Util.java", "b/Util.java"]);
    let bases = [root.clone()];
    let find = |name: &str| find(name, &bases, &mut Files::new(vec![root.clone()]));
    assert_eq!(find("README.md"), Found::File(root.join("README.md")), "as written");
    assert_eq!(find("./README.md"), Found::File(root.join("README.md")));
    let device = Found::File(root.join("src/main/dev/TestDevice.java"));
    assert_eq!(find("TestDevice.java"), device, "given alone: the one file");
    assert_eq!(find("dev/TestDevice.java"), device, "with its last folder");
    assert_eq!(find("evice.java"), Found::Not, "by whole parts, never a name's end");
    let mut both = match find("Util.java") {
        Found::Several(all, 2) => all,
        other => panic!("two files end with it: {other:?}"),
    };
    both.sort();
    assert_eq!(both, [root.join("a/Util.java"), root.join("b/Util.java")]);
    assert_eq!(find("a/Util.java"), Found::File(root.join("a/Util.java")));
    assert_eq!(find("Gone.java"), Found::Not);
    assert_eq!(find("../b/Util.java"), Found::Not, "from a folder: never a file's last parts");
    assert_eq!(find("/nowhere/TestDevice.java"), Found::Not);
    // (a name that doesn't look like a file lists nothing: a time, a word)
    let mut files = Files::new(vec![root.clone()]);
    assert_eq!(in_files("12", &mut files), Found::Not);
    assert!(files.listed.is_none(), "never listed for it");
}

/// More files end with a name than a picker lists: all of them counted, the first `SEVERAL_MAX` listed — the picker's
/// title says how many there are, and that it lists only the first (#196's review: it said "50 files" of 60).
#[test]
fn the_files_a_name_stands_for_are_counted_before_they_are_cut() {
    let many: Vec<String> = (0..60).map(|i| format!("m{i:02}/Same.java")).collect();
    let (_s, root) = project("places-many", &many.iter().map(String::as_str).collect::<Vec<_>>());
    match in_files("Same.java", &mut Files::new(vec![root.clone()])) {
        Found::Several(listed, n) => assert_eq!((listed.len(), n), (SEVERAL_MAX, 60)),
        other => panic!("60 files end with it: {other:?}"),
    }
    let mut ed = chat_on(&root, "Same.java:1");
    open_named(&mut ed, "Same.java".into(), 0);
    until(&mut ed, 120, "the picker", |ed| ed.picker.is_some());
    let p = ed.picker.as_ref().unwrap();
    assert!(p.title.contains("60 files end with it, the first 50 listed"), "{}", p.title);
}

#[test]
fn a_line_and_column_come_off_a_name() {
    assert_eq!(line_split("a.rs:12:5"), ("a.rs", Some((11, 4))));
    assert_eq!(line_split("a.rs:12"), ("a.rs", Some((11, 0))));
    assert_eq!(line_split("a.rs"), ("a.rs", None));
    assert_eq!(line_split("a.rs:x"), ("a.rs:x", None));
}

/// A path broken by the wrap is one place, its part on each row; a row a link whole takes none.
#[test]
fn a_place_is_found_in_the_line_not_the_row() {
    use crate::markdown::Span;
    let span = |t: &str| Span { cut: None, hint: false, text: t.into(), style: Default::default() };
    let row = |t: &str, skip: usize, join: Option<bool>| ChatRow {
        spans: vec![span(t)],
        skip,
        join,
        ..ChatRow::default()
    };
    // (`▎ ` the drawer's; the second row goes on from the first with no space between — a word broken)
    let mut rows = vec![row("▎ see src/main/Te", 2, None), row("▎ stDevice.java:3 now", 2, Some(false))];
    mark_rows(&mut rows);
    let parts: Vec<(&str, &str, usize)> = rows
        .iter()
        .flat_map(|r| {
            let text = &r.spans[0].text;
            r.places.iter().map(move |p| (&text[p.bytes.clone()], &*p.name, p.line))
        })
        .collect();
    assert_eq!(
        parts,
        [("src/main/Te", "src/main/TestDevice.java", 2), ("stDevice.java:3", "src/main/TestDevice.java", 2)]
    );
    let mut linked = vec![ChatRow {
        link: Some(crate::chat::Link { path: "a.rs".into(), line: 0 }),
        ..row("a.rs:1", 0, None)
    }];
    mark_rows(&mut linked);
    assert!(linked[0].places.is_empty());
}

/// An editor on `root` with the chat up, `answer` its one answer.
fn chat_on(root: &Path, answer: &str) -> Editor {
    let mut ed = Editor::new(Config::default());
    ed.workspace.root = root.to_path_buf();
    ed.docs[0].text = ropey::Rope::from_str("fn main() {}\n");
    let mut c = Chat::new();
    c.msgs = vec![Msg::new(Role::User, "where?", ""), Msg::new(Role::Assistant, answer, "")];
    ed.workspace.claude.chat = Some(c);
    ed
}

/// A frame `w` wide: each row's characters, and whether each is underlined.
fn frame(ed: &mut Editor, w: u16) -> Vec<Vec<(char, bool)>> {
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, w, H).unwrap();
    let mut p = vt100::Parser::new(H, w, 0);
    p.process(&bytes);
    let s = p.screen();
    (0..H)
        .map(|y| {
            (0..w)
                .filter_map(|x| s.cell(y, x))
                .filter(|c| !c.is_wide_continuation())
                .map(|c| (c.contents().chars().next().unwrap_or(' '), c.underline()))
                .collect()
        })
        .collect()
}

/// Frames until the names drawn are answered (none asked, none being found).
fn settled(ed: &mut Editor, w: u16) -> Vec<Vec<(char, bool)>> {
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    loop {
        let f = frame(ed, w);
        let k = &ed.workspace.claude.chat.as_ref().unwrap().places;
        if !k.asking && k.wanted.borrow().is_empty() && !k.found.is_empty() {
            return f;
        }
        assert!(std::time::Instant::now() < end, "the names found");
        if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
            ed.handle_event(ev);
        }
    }
}

/// The underlined runs of a frame, in reading order (the text, the cells it's in — x, y) — `wrapped`: one that starts
/// its row joined to the run before when that one ended on the row above (a place the wrap carried on).
fn underlined(f: &[Vec<(char, bool)>], wrapped: bool) -> Vec<(String, Vec<(usize, usize)>)> {
    let mut out: Vec<(String, Vec<(usize, usize)>)> = Vec::new();
    let mut open = false;
    for (y, row) in f.iter().enumerate() {
        let first = row.iter().position(|c| c.1);
        for (x, &(ch, u)) in row.iter().enumerate() {
            let goes_on =
                wrapped && Some(x) == first && out.last().is_some_and(|r| r.1.last().unwrap().1 + 1 == y);
            match (u, open || goes_on) {
                (true, true) => {}
                (true, false) => out.push((String::new(), Vec::new())),
                (false, _) => {
                    open = false;
                    continue;
                }
            }
            open = true;
            let run = out.last_mut().unwrap();
            run.0.push(ch);
            run.1.push((x, y));
        }
        open = false;
    }
    out
}

fn press(ed: &mut Editor, (x, y): (usize, usize), w: u16) {
    for kind in [MouseKind::Down, MouseKind::Up] {
        ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
    }
    frame(ed, w);
}

/// Jobs handled, a frame after each, until `done`.
fn until(ed: &mut Editor, w: u16, what: &str, done: impl Fn(&Editor) -> bool) {
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    while !done(ed) {
        assert!(std::time::Instant::now() < end, "{what}");
        if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
            ed.handle_event(ev);
            frame(ed, w);
        }
    }
}

/// The friction: `TestDevice.java:2` in an answer, the file deep in the project, was underlined and a press said
/// "isn't a file here". Now: a name one file ends with is underlined and opens it at its line; one two files end
/// with is underlined and opens their picker; one no file has isn't underlined, and a press there opens nothing.
#[test]
fn a_name_given_alone_opens_what_it_names_or_isnt_a_place() {
    let (_s, root) =
        project("places-alone", &["src/main/java/dev/TestDevice.java", "a/Util.java", "b/Util.java"]);
    let w = 140;
    let mut ed = chat_on(&root, "See TestDevice.java:2 and Util.java:3, not Gone.java:1.");
    let f = settled(&mut ed, w);
    let runs = underlined(&f, false);
    let texts: Vec<&str> = runs.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(texts, ["TestDevice.java:2", "Util.java:3"], "what's underlined opens — Gone.java isn't");
    // One file: opened at its line
    press(&mut ed, runs[0].1[3], w);
    assert_eq!(ed.doc().path.as_deref(), Some(root.join("src/main/java/dev/TestDevice.java").as_path()));
    let d = ed.doc();
    assert_eq!(d.text.byte_to_line(d.selection().primary().cursor(&d.text)), 1);
    // Two: their picker, each at the line
    press(&mut ed, runs[1].1[2], w);
    until(&mut ed, w, "the picker", |ed| ed.picker.is_some());
    let p = ed.picker.as_ref().unwrap();
    assert!(p.title.starts_with("Util.java:3") && p.title.contains("2 files"), "{}", p.title);
    let labels: Vec<&str> = p.items().iter().map(|i| i.label.as_str()).collect();
    assert!(labels.iter().any(|l| l.starts_with("a/Util.java:3")), "{labels:?}");
    assert!(labels.iter().any(|l| l.starts_with("b/Util.java:3")), "{labels:?}");
    ed.picker = None;
    // None: no place there — a press opens nothing and says nothing of a file
    let row = f.iter().position(|r| r.iter().map(|c| c.0).collect::<String>().contains("Gone.java")).unwrap();
    let text: String = f[row].iter().map(|c| c.0).collect();
    let x = text.chars().count() - text.split("Gone.java").nth(1).unwrap().chars().count() - 3;
    let before = ed.doc().id;
    press(&mut ed, (x, row), w);
    assert_eq!(ed.doc().id, before);
    assert!(ed.picker.is_none());
}

/// Answers are kept while the checkout's list of files can't have changed (#196's review: every write threw them away
/// — one name no file has listed the project again on each of Claude's writes): a file's text written asks nothing
/// again; a file made (`files_moved`, as a change set's create says) does, and the name it gives is underlined; an
/// answer the same as the one drawn draws no frame (`unchanged`).
#[test]
fn answers_stay_until_the_files_could_have_changed() {
    let (_s, root) = project("places-kept", &["src/TestDevice.java"]);
    let w = 140;
    let mut ed = chat_on(&root, "See TestDevice.java:2, not Gone.java:1.");
    settled(&mut ed, w);
    let k = |ed: &Editor| (ed.workspace.claude.chat.as_ref().unwrap().places.asking, answers(ed).len());
    assert_eq!(k(&ed), (false, 2));
    // A file's text written (an edit landed, a save): nothing asked again
    let device = root.join("src/TestDevice.java");
    std::fs::write(&device, "a\nb\n").unwrap();
    ed.wrote(&device);
    frame(&mut ed, w);
    assert_eq!(k(&ed), (false, 2), "kept across a write");
    // A file made: asked again — the name it gives a place now
    std::fs::write(root.join("src/Gone.java"), "x\n").unwrap();
    ed.files_moved(&root.join("src/Gone.java"));
    let texts: Vec<String> = underlined(&settled(&mut ed, w), false).into_iter().map(|r| r.0).collect();
    assert_eq!(texts, ["TestDevice.java:2", "Gone.java:1"]);
    // Asked again for nothing new (a run ended that made nothing): its answers land without a frame
    ed.workspace.listing += 1;
    frame(&mut ed, w);
    assert!(ed.workspace.claude.chat.as_ref().unwrap().places.asking, "asked again");
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    while ed.workspace.claude.chat.as_ref().unwrap().places.asking {
        assert!(std::time::Instant::now() < end, "its answers");
        if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
            ed.unchanged = false;
            ed.handle_event(ev);
        }
    }
    assert!(ed.unchanged, "the same answers: nothing to draw");
}

/// The texts underlined in a frame, in reading order.
fn texts(f: &[Vec<(char, bool)>]) -> Vec<String> {
    underlined(f, false).into_iter().map(|r| r.0).collect()
}

/// A name no file has, in the chat's answer: not a place. Then `trigger`, alone — what tells the chat its checkout's
/// files may have changed, as it does in use — and its file, made by the trigger or before it (`by_hand`: a file tarae
/// didn't see made — not a place by itself), is a place (#196's review: the tests called `files_moved` or bumped
/// `listing` themselves, so taking out any of the six lines that do it in use passed).
fn a_place_after(what: &str, by_hand: bool, trigger: impl FnOnce(&mut Editor, &Path)) {
    let (_s, root) = project(&format!("places-{what}"), &["src/TestDevice.java"]);
    let w = 140;
    let mut ed = chat_on(&root, "See TestDevice.java:2 and Gone.java:1.");
    assert_eq!(texts(&settled(&mut ed, w)), ["TestDevice.java:2"], "{what}: no file yet");
    if by_hand {
        std::fs::write(root.join("src/Gone.java"), "x\n").unwrap();
        assert_eq!(
            texts(&settled(&mut ed, w)),
            ["TestDevice.java:2"],
            "{what}: made unseen — not asked again"
        );
    }
    trigger(&mut ed, &root);
    assert!(root.join("src/Gone.java").is_file(), "{what}: the file made");
    assert_eq!(texts(&settled(&mut ed, w)), ["TestDevice.java:2", "Gone.java:1"], "{what}: a place after it");
}

/// Events handled (no frame) until `done` — a trigger's own end.
fn handled_until(ed: &mut Editor, what: &str, done: impl Fn(&Editor) -> bool) {
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    while !done(ed) {
        assert!(std::time::Instant::now() < end, "{what}");
        if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
            ed.handle_event(ev);
        }
    }
}

fn keys(ed: &mut Editor, keys: &str) {
    for k in keys.split(' ') {
        match k.strip_prefix('<').and_then(|k| k.strip_suffix('>')) {
            Some(k) => ed.handle_key(k.parse().unwrap()),
            None => k.chars().for_each(|c| ed.handle_key(crate::key::Key::plain(crate::key::Code::Char(c)))),
        }
    }
}

/// A save that made a file (`:w` of a new buffer — `saves_finish`).
#[test]
fn a_name_is_found_again_after_a_save_that_made_its_file() {
    a_place_after("saved", false, |ed, root| {
        ed.open(&root.join("src/Gone.java")).unwrap();
        keys(ed, "i x <esc> :w <ret>");
        handled_until(ed, "the save", |ed| ed.doc().saving == 0 && ed.doc().disk.is_some());
    });
}

/// The checkout's HEAD moved (git's watch round — `git_reload_bases_in`): a checkout or reset brings files.
#[test]
fn a_name_is_found_again_after_head_moved() {
    a_place_after("head", true, |ed, root| crate::git::watch_round(ed, root, (None, None), true));
}

/// A task's run ended (`task_spawn`): it may have made files.
#[test]
fn a_name_is_found_again_after_a_task_ran() {
    a_place_after("task", false, |ed, root| {
        let cmd = "touch src/Gone.java".into();
        ed.task_start(crate::task::Task { name: "touch".into(), cmd, cwd: root.to_path_buf() });
        handled_until(ed, "the run's end", |ed| {
            ed.workspace.task_run.as_ref().is_some_and(|r| r.state != crate::task::State::Running)
        });
    });
}

/// A test run ended (`test_start_guarded`): it may have made files.
#[test]
fn a_name_is_found_again_after_tests_ran() {
    use crate::testing::{How, RunState, Target};
    a_place_after("tests", false, |ed, root| {
        let argv = ["sh", "-c", "touch src/Gone.java"].map(String::from).to_vec();
        let lines = crate::test_results::Rules::builtin("other");
        let how = How::Pack { lang: "other".into(), argv, lines, junit: None };
        ed.test_start(Target { label: "t".into(), cwd: root.to_path_buf(), how });
        handled_until(ed, "the run's end", |ed| {
            ed.workspace.test_run.as_ref().is_some_and(|r| r.state != RunState::Running)
        });
    });
}

/// A message sent (`submit_input`): what tarae didn't see — the terminal's, another program's — may have made files.
#[test]
fn a_name_is_found_again_after_a_message_is_sent() {
    a_place_after("sent", true, |ed, _| {
        ed.config.llm.command = "sh".into();
        ed.config.llm.args = vec!["-c".into(), "cat > /dev/null".into()];
        ed.config.llm.chat_settings = false;
        ed.workspace.claude.chat.as_mut().unwrap().input = "hello".into();
        crate::chat::send(ed);
        handled_until(ed, "the message sent", |ed| {
            ed.workspace.claude.chat.as_ref().is_some_and(|c| c.busy() && c.queued.is_none())
        });
    });
}

/// A change set of Claude's landed as in use (`refactor::land_told`, under review at the end — `refactor::landed`): a
/// create written unseen, a move or a delete accepted on its card (`accept_all`); `n` its call's id.
fn change_set_landed(ed: &mut Editor, root: &Path, n: u64, rel: &str, before: &str, op: crate::refactor::Op) {
    use crate::refactor::{FileChange, Op};
    let after = if matches!(op, Op::Delete) { "" } else { "x\n" };
    let f = FileChange {
        path: root.join(rel),
        rel: rel.into(),
        before: before.into(),
        after: after.into(),
        places: 1,
        op,
        protected: false,
    };
    ed.workspace.claude.chat.as_mut().unwrap().edits.at_end = Some(root.to_path_buf());
    let call = crate::tools::Call::test(n);
    crate::refactor::land_told(ed, call, "edit".into(), "change", vec![f], String::new(), String::new());
    let answered = move |_: &Editor| {
        crate::tools::ANSWERED.with(|a| a.borrow().iter().any(|(i, _)| *i == serde_json::json!(n)))
    };
    let card = |ed: &Editor| {
        ed.offers.iter().find_map(|o| match o.what {
            crate::offer::What::ChangeSet { whose, .. } => Some((o.id, whose)),
            _ => None,
        })
    };
    handled_until(ed, "its card or its answer", |ed| answered(ed) || card(ed).is_some());
    if let Some((id, whose)) = card(ed) {
        crate::refactor::accept_all(ed, id, whose);
    }
    handled_until(ed, "its answer", answered);
}

/// A change set that made a file (`refactor::landed`: an edit's create).
#[test]
fn a_name_is_found_again_after_a_change_set_made_it() {
    use crate::refactor::Op;
    a_place_after("set-create", false, |ed, root| {
        change_set_landed(ed, root, 1, "src/Gone.java", "", Op::Create)
    });
}

/// A change set that moved a file there (`refactor::landed`: an edit's move — the file it moves from isn't named).
#[test]
fn a_name_is_found_again_after_a_change_set_moved_it() {
    use crate::refactor::Op;
    a_place_after("set-move", false, |ed, root| {
        std::fs::write(root.join("src/Old.java"), "x\n").unwrap();
        let op = Op::Move { to: root.join("src/Gone.java"), to_rel: "src/Gone.java".into() };
        change_set_landed(ed, root, 2, "src/Old.java", "x\n", op);
        assert!(!root.join("src/Old.java").exists(), "moved");
    });
}

/// A change set that deleted a file (`refactor::landed`: an edit's delete) — another than the one made unseen.
#[test]
fn a_name_is_found_again_after_a_change_set_deleted_one() {
    use crate::refactor::Op;
    a_place_after("set-delete", true, |ed, root| {
        std::fs::write(root.join("src/Old.java"), "old\n").unwrap();
        change_set_landed(ed, root, 3, "src/Old.java", "old\n", Op::Delete);
        assert!(!root.join("src/Old.java").exists(), "deleted");
    });
}

/// `C-]`'s stops and where follow goes at the turn's end are the places the chat underlines (#196's review: they
/// went to the root joined to the name — a stop on `Gone.java:3`, which isn't one, and follow nowhere for a name
/// given alone): a name no file has is no stop, one given alone the file it came to, follow there once it's found.
#[test]
fn c_bracket_and_follow_go_where_the_underline_does() {
    let (_s, root) = project("places-hops", &["src/main/dev/TestDevice.java"]);
    let file = root.join("src/main/dev/TestDevice.java");
    let w = 140;
    let mut ed = chat_on(&root, "Not Gone.java:3 — see TestDevice.java:2.");
    ed.workspace.claude.chat.as_mut().unwrap().follow.on = true;
    // The turn ends before its names are found: follow waits for them
    crate::follow::on_done(&mut ed);
    assert_eq!(ed.doc().path, None, "nowhere yet");
    settled(&mut ed, w);
    until(&mut ed, w, "follow at the answer's file", |ed| ed.doc().path.as_deref() == Some(file.as_path()));
    assert_eq!(crate::follow::view_line(&ed), Some(1), "at its line");
    let hops = ed.workspace.claude.chat.as_ref().unwrap().hops(&root);
    let link = crate::chat::Link { path: file.clone(), line: 1 };
    assert_eq!(hops, [crate::chat::Hop::Place(link, None)], "the one place underlined");
}

/// A run's output names a file alone (javac's `Device.java:2: error: …`, printed from a module's folder): its place
/// is the one project file that ends with it — by the matcher a task's reader thread makes (`task_spawn`), in a real
/// run; one two files end with is no place (a list row opens one file); a matcher not given the roots never lists
/// them (#196's review: nothing pinned these).
#[test]
fn a_runs_name_given_alone_is_the_one_file_with_it() {
    let (_s, root) = project("places-run", &["src/main/java/dev/Device.java", "a/Util.java", "b/Util.java"]);
    let device = root.join("src/main/java/dev/Device.java");
    let mut m = crate::task::Matcher::builtin().with_files(vec![root.clone()]);
    let got = m.line(&root, "Device.java:2: error: cannot find symbol");
    assert_eq!(got.map(|i| (i.path, i.line)), Some((device.clone(), 1)));
    assert!(m.line(&root, "Util.java:3: error: x").is_none(), "two files: no one place");
    let mut bare = crate::task::Matcher::builtin();
    assert!(bare.line(&root, "Device.java:2: error: x").is_none());
    assert!(!bare.listed(), "never listed");
    // A task's run: its places
    let mut ed = chat_on(&root, "");
    let cmd = "printf 'Device.java:2: error: cannot find symbol\\nUtil.java:3: error: x\\n'";
    ed.task_start(crate::task::Task { name: "javac".into(), cmd: cmd.into(), cwd: root.clone() });
    until(&mut ed, 120, "the run's end", |ed| {
        ed.workspace.task_run.as_ref().is_some_and(|r| r.state != crate::task::State::Running)
    });
    let r = ed.workspace.task_run.as_ref().unwrap();
    let places: Vec<(&Path, usize)> = r.places.iter().map(|i| (i.path.as_path(), i.line)).collect();
    assert_eq!(places, [(device.as_path(), 1)], "{:?}", r.output.to_vec());
}

/// A run printing a name alone on each of its lines, in a big project: each line compares the name with the files of
/// its file name only, never the whole listing (#196's review: every line not found as written scanned every file, on
/// the reader thread — a Gradle run's hundreds of `Foo.java:N` lines over 100,000 files slowed its output). The work
/// is counted, the same on any machine: 100,000 files listed, 400 lines — each line's names one file or three.
#[test]
fn a_runs_lines_compare_a_name_with_the_files_of_its_name_only() {
    let s = crate::scratch::Scratch::new("places-big");
    let root = std::fs::canonicalize(&*s).unwrap();
    let mut paths: Vec<PathBuf> =
        (0..100_000).map(|i| root.join(format!("m{}/src/p{}/F{i}.java", i / 1000, i % 100))).collect();
    paths.extend(["a", "b", "c"].map(|d| root.join(d).join("Util.java")));
    let files = Files::of(vec![root.clone()], paths);
    let mut m = crate::task::Matcher::builtin().with_listed(files);
    let (mut placed, mut expected) = (0, 0);
    for k in 0..400 {
        let (line, alone) = match k % 4 {
            0 => ("Util.java:3: error: x".to_string(), false),
            _ => (format!("F{}.java:{}: error: cannot find symbol", k * 241 % 100_000, k + 1), true),
        };
        placed += usize::from(m.line(&root, &line).is_some());
        expected += if alone { 1 } else { 3 };
        assert!(
            m.compared() <= expected,
            "line {k}: compared {}, the files of its names {expected}",
            m.compared()
        );
    }
    assert_eq!(placed, 300, "every name one file has a place; `Util.java`, three, none");
}

/// A path longer than the chat's rows, at several widths: underlined whole across every row the wrap put it on —
/// the underlined parts, read in order, the place exactly — and a press on any of its rows opens it at its line.
#[test]
fn a_path_the_wrap_breaks_is_one_place_on_every_row() {
    let path = "src/main/java/com/example/devices/integration/fixtures/TestDeviceFixtureRegistry.java";
    let (_s, root) = project("places-wrap", &[path]);
    let answer = format!("The registry is at {path}:3 — read it first.");
    for w in [100u16, 140, 200] {
        let mut ed = chat_on(&root, &answer);
        let f = settled(&mut ed, w);
        let runs = underlined(&f, true);
        assert_eq!(runs.len(), 1, "width {w}: one place: {runs:?}");
        assert_eq!(runs[0].0, format!("{path}:3"), "width {w}: underlined whole");
        let rows: Vec<usize> = runs[0].1.iter().map(|c| c.1).collect();
        let (first, last) = (rows[0], *rows.last().unwrap());
        assert!(last > first, "width {w}: the wrap broke it");
        for k in 0..=last - first {
            // (the cursor back at the top: each press seen opening it at its line)
            if ed.doc().path.is_some() {
                ed.doc_mut().set_selection(crate::selection::Selection::point(0));
            }
            let before = ed.doc().id;
            // (where its rows are now: the chat's rows as this frame has them)
            let runs = underlined(&frame(&mut ed, w), true);
            let ys: Vec<usize> = runs[0].1.iter().map(|c| c.1).collect();
            let y = ys[0] + k;
            let cell = *runs[0].1.iter().find(|c| c.1 == y).unwrap();
            press(&mut ed, cell, w);
            assert_eq!(
                ed.doc().path.as_deref(),
                Some(root.join(path).as_path()),
                "width {w}, row {y}: opened"
            );
            let d = ed.doc();
            let line = d.text.byte_to_line(d.selection().primary().cursor(&d.text));
            assert_eq!(line, 2, "width {w}, row {y}: at its line (from {before:?})");
        }
    }
}
