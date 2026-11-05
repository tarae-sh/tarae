use super::*;
use crate::config::Config;
use crate::event::Event;
use crate::key::Key;
use crate::test_results::settle_until;

fn key(k: &str) -> Key {
    k.parse().unwrap()
}

/// Keys as events (`<esc>`, `<ret>` by name) — each one's after-event work (`recovery_sync`) runs too.
fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let k = match c {
            '<' => key(&chars.by_ref().take_while(|&c| c != '>').collect::<String>()),
            c => key(&c.to_string()),
        };
        ed.handle_event(Event::Key(k));
    }
}

/// A test's own folder: `state` (the copies), `home` (where keys are), `project` (the files).
fn place(name: &str) -> PathBuf {
    let dir = crate::scratch::Scratch::new(&format!("recovery-{name}")).into_path();
    std::fs::create_dir_all(dir.join("project")).unwrap();
    std::fs::create_dir_all(dir.join("home")).unwrap();
    dir.canonicalize().unwrap()
}

/// An editor keeping its copies in `dir/state`, after a pause of 20 ms, with `dir/home` as home.
fn editor(dir: &Path) -> Editor {
    let mut ed = Editor::new(Config::default());
    let mut r = Recovery::new(dir.join("state"), ed.workspace.root.clone());
    r.quiet = Duration::from_millis(20);
    r.rules.home = Some(dir.join("home"));
    ed.recovery = Some(r);
    ed
}

fn recovery(ed: &mut Editor) -> &mut Recovery {
    ed.recovery.as_mut().unwrap()
}

/// Until `done` — looked at every few ms, 5 s at most (what a worker does on disk, seen from outside).
fn until(what: &str, done: impl Fn() -> bool) {
    let end = Instant::now() + crate::test_results::PATIENCE;
    while !done() {
        assert!(Instant::now() < end, "never: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Every copy in `dir/state`, any tarae's.
fn copies_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir.join("state")) else { return Vec::new() };
    rd.flatten().flat_map(|e| copies(&e.path())).collect()
}

/// The text of the one copy in `dir/state`, if there's one.
fn copy_text(dir: &Path) -> Option<String> {
    match copies_in(dir).as_slice() {
        [one] => read_copy(one).map(|c| c.text),
        _ => None,
    }
}

/// `project/a.txt` ("one"), edited in a tarae that then dies with the copy of its last edit on disk — the file.
/// The pause passes mid-edit first, as it does on a loaded machine between two keys: the copy written then
/// ("ne") is replaced by the last edit's — what's waited for is that one, not a copy.
fn left_behind(dir: &Path) -> PathBuf {
    let file = dir.join("project/a.txt");
    std::fs::write(&file, "one\n").unwrap();
    let mut ed = editor(dir);
    ed.open(&file).unwrap();
    feed(&mut ed, "c");
    until("the mid-edit copy is written", || copy_text(dir).as_deref() == Some("ne\n"));
    feed(&mut ed, "TWO<esc>");
    until("the last edit's copy is written", || copy_text(dir).as_deref() == Some("TWOne\n"));
    recovery(&mut ed).killed();
    file
}

/// A new tarae over `dir`'s state, its look at what gone ones left done: its recovery cards.
fn started(dir: &Path) -> Editor {
    let mut ed = editor(dir);
    ed.recovery_scan();
    settle_until(&mut ed, |ed| ed.recovery.as_ref().is_some_and(|r| r.scans > 0));
    ed
}

fn cards(ed: &Editor) -> Vec<&Offer> {
    ed.offers.iter().filter(|o| o.recovers()).collect()
}

/// A buffer edited: its copy is written after the pause — its whole text after a header naming the file
/// and its stamp. The tarae killed, the next start offers it on a card that says what changed against the
/// file and that the file is as it was.
#[test]
fn a_killed_taraes_unsaved_edits_are_offered_at_the_next_start() {
    let dir = place("offered");
    let file = left_behind(&dir);
    let c = read_copy(&copies_in(&dir)[0]).unwrap();
    assert_eq!((c.path.as_path(), c.text.as_str()), (file.as_path(), "TWOne\n"));
    assert!(c.disk.is_some(), "the file's stamp as the buffer knew it");
    let ed = started(&dir);
    let cards = cards(&ed);
    assert_eq!(cards.len(), 1);
    assert!(cards[0].title.ends_with("a.txt"), "{}", cards[0].title);
    assert!(cards[0].question.contains("+1 −1 lines"), "{}", cards[0].question);
    assert!(cards[0].question.contains("as it was then"), "{}", cards[0].question);
    assert_eq!(cards[0].answers(false).iter().map(|a| a.0).collect::<String>(), "ycDn");
    std::fs::remove_dir_all(dir).ok();
}

/// A tarae still running keeps its copies to itself — its lock is held. Once it's gone, they're offered.
#[test]
fn a_running_taraes_copies_are_not_offered() {
    let dir = place("running");
    let file = dir.join("project/a.txt");
    std::fs::write(&file, "one\n").unwrap();
    let mut running = editor(&dir);
    running.open(&file).unwrap();
    feed(&mut running, "cTWO<esc>");
    until("the copy is written", || copies_in(&dir).len() == 1);
    assert!(cards(&started(&dir)).is_empty(), "offered while its tarae runs");
    assert_eq!(copies_in(&dir).len(), 1, "and left alone");
    recovery(&mut running).killed();
    assert_eq!(cards(&started(&dir)).len(), 1);
    std::fs::remove_dir_all(dir).ok();
}

/// `y` puts the copy's text in the buffer — unsaved, `u` goes back to the file's — and the copy and its
/// folder go. The file isn't written.
#[test]
fn restore_puts_the_text_back() {
    let dir = place("restore");
    let file = left_behind(&dir);
    let copy = copies_in(&dir)[0].clone();
    let mut ed = started(&dir);
    ed.offer_accept();
    assert!(cards(&ed).is_empty());
    assert_eq!(ed.doc().path.as_deref(), Some(file.as_path()));
    assert_eq!(ed.doc().text.to_string(), "TWOne\n");
    assert!(ed.doc().is_modified());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\n", "nothing written");
    until("the copy and its folder go", || !copy.parent().unwrap().exists());
    feed(&mut ed, "u");
    assert_eq!(ed.doc().text.to_string(), "one\n");
    assert!(!ed.doc().is_modified());
    std::fs::remove_dir_all(dir).ok();
}

/// `y` while the file's buffer has edits of the user's own: refused on the card, which stays.
#[test]
fn restore_never_goes_over_edits_of_yours() {
    let dir = place("over");
    let file = left_behind(&dir);
    let mut ed = editor(&dir);
    ed.open(&file).unwrap();
    feed(&mut ed, "iMINE <esc>");
    ed.recovery_scan();
    settle_until(&mut ed, |ed| !cards(ed).is_empty());
    ed.offer_accept();
    assert_eq!(cards(&ed).len(), 1, "the card stays");
    assert_eq!(ed.doc().text.to_string(), "MINE one\n");
    assert!(ed.offer_said_now().is_some_and(|s| s.contains("unsaved edits of yours")));
    std::fs::remove_dir_all(dir).ok();
}

/// `D` removes the copy, opens nothing; `n` leaves it — and the next start asks again.
#[test]
fn discard_removes_the_copy_and_not_now_keeps_it() {
    let dir = place("discard");
    left_behind(&dir);
    let copy = copies_in(&dir)[0].clone();
    let mut ed = started(&dir);
    ed.offer_dismiss();
    assert!(cards(&ed).is_empty() && copy.exists(), "n keeps it");
    quit(ed, &dir, || {});
    let mut ed = started(&dir);
    assert_eq!(cards(&ed).len(), 1, "asked again");
    let docs = ed.docs.len();
    ed.offer_discard();
    assert!(cards(&ed).is_empty());
    assert_eq!(ed.docs.len(), docs, "nothing opened");
    until("the copy goes", || !copy.exists());
    assert!(cards(&started(&dir)).is_empty());
    std::fs::remove_dir_all(dir).ok();
}

/// `c` shows the copy against the file in a buffer of its own; the card stays, and goes with its answer.
#[test]
fn compare_shows_the_copy_against_the_file() {
    let dir = place("compare");
    left_behind(&dir);
    let mut ed = started(&dir);
    crate::recovery::compare(&mut ed);
    assert_eq!(cards(&ed).len(), 1, "the card stays");
    assert!(ed.doc().title.as_deref().is_some_and(|t| t.starts_with(COMPARE)));
    assert_eq!(ed.doc().text.to_string(), "TWOne\n");
    let id = ed.doc().id;
    settle_until(&mut ed, |ed| ed.docs.iter().any(|d| d.id == id && !d.git_hunks.is_empty()));
    ed.offer_dismiss();
    assert!(!ed.docs.iter().any(|d| d.id == id), "closed with the card");
    std::fs::remove_dir_all(dir).ok();
}

/// The file changed on disk after the copy was kept: the card says so, in the warning colour. A copy that's
/// what the file is now is no card — it goes.
#[test]
fn a_file_changed_on_disk_since_says_so() {
    let dir = place("changed");
    let file = left_behind(&dir);
    std::fs::write(&file, "three\n").unwrap();
    let ed = started(&dir);
    let card = cards(&ed)[0];
    assert!(card.question.contains("The file changed since"), "{}", card.question);
    assert!(card.warning.is_some());
    // (a child forked while this tarae holds the gone one's folder for its card — as another test's would — let
    // go only once this tarae has quit)
    let child = HeldBack::fork();
    quit(ed, &dir, move || drop(child));
    std::fs::write(&file, "TWOne\n").unwrap();
    let copy = copies_in(&dir)[0].clone();
    assert!(cards(&started(&dir)).is_empty(), "nothing to recover");
    assert!(!copy.exists());
    std::fs::remove_dir_all(dir).ok();
}

/// This tarae quits, and the next start finds the folders in `dir`'s state as a real next start would: once no
/// process has their locks open. A child forked meanwhile holds them too until its exec (another test's, here;
/// a fork copies every descriptor), so a lock it held for a card, its own keeper's, one let go at `n`, stay held
/// a moment past the quit — the next scan took the gone tarae for a running one and left its copy (a full run's
/// flake, `a_file_changed_on_disk_since_says_so`). `meanwhile` runs while it waits (a held-back child let go).
/// As `Recovery::killed` waits for a death.
fn quit(ed: Editor, dir: &Path, meanwhile: impl FnOnce() + Send + 'static) {
    drop(ed);
    let going = std::thread::spawn(meanwhile);
    let locks: Vec<PathBuf> = std::fs::read_dir(dir.join("state"))
        .map(|rd| rd.flatten().map(|e| e.path().join(LOCK)).filter(|l| l.exists()).collect())
        .unwrap_or_default();
    for lock in &locks {
        // (gone is let go: an answer's `Forget` removes its folder, as late as this)
        let free = || File::open(lock).map_or(true, |f| f.try_lock().is_ok());
        until(&format!("{} let go", lock.display()), free);
    }
    going.join().unwrap();
}

/// A child forked now — holding a copy of every descriptor open, a lock's too — that waits before its exec until
/// it's let go (`go`, or dropped: a failing test's too): another test's child not yet exec'd, made to wait as long
/// as the test needs. Harmless: `true`. Never longer than `HELD_AT_MOST`: it holds the test binary's output open
/// (the first version, without a bound, held a `cargo test … | grep` open for good once its test failed before
/// letting it go) — then it gives up before its exec, and `go` says so.
struct HeldBack {
    go: Option<std::io::PipeWriter>,
    child: Option<std::thread::JoinHandle<std::io::Result<std::process::ExitStatus>>>,
}

/// How long a held-back child waits to be let go, at most.
const HELD_AT_MOST: Duration = crate::test_results::PATIENCE;

impl HeldBack {
    fn fork() -> HeldBack {
        use std::io::Read as _;
        use std::os::fd::AsRawFd as _;
        use std::os::unix::process::CommandExt as _;
        let (mut forked, said) = std::io::pipe().unwrap();
        let (wait, go) = std::io::pipe().unwrap();
        let (said_fd, wait_fd) = (said.as_raw_fd(), wait.as_raw_fd());
        let at_most = HELD_AT_MOST.as_millis() as libc::c_int;
        let child = std::thread::spawn(move || {
            let mut cmd = std::process::Command::new("true");
            // SAFETY: only async-signal-safe calls (write, poll, read) between the fork and the exec; the error
            // made is an OS code's, no allocation
            unsafe {
                cmd.pre_exec(move || {
                    libc::write(said_fd, b"f".as_ptr().cast(), 1);
                    // (it holds the write end too: a byte lets it go, no end of file ever would)
                    let mut p = libc::pollfd { fd: wait_fd, events: libc::POLLIN, revents: 0 };
                    if libc::poll(&mut p, 1, at_most) == 0 {
                        return Err(std::io::Error::from_raw_os_error(libc::ETIMEDOUT));
                    }
                    let mut b = 0u8;
                    libc::read(wait_fd, (&raw mut b).cast(), 1);
                    Ok(())
                });
            }
            let status = cmd.status();
            drop((said, wait));
            status
        });
        forked.read_exact(&mut [0]).unwrap();
        HeldBack { go: Some(go), child: Some(child) }
    }

    /// Its exec goes ahead; it's waited for (within `HELD_AT_MOST` of its fork, by its own bound). Err: what
    /// went wrong, in words. None: let go already.
    fn go(&mut self) -> Option<Result<std::process::ExitStatus, String>> {
        let mut go = self.go.take()?;
        let child = self.child.take()?;
        let wrote = std::io::Write::write_all(&mut go, b"g").map_err(|e| format!("couldn't let it go: {e}"));
        let ended = match child.join() {
            Err(_) => Err("its thread panicked".to_string()),
            Ok(Err(e)) if e.raw_os_error() == Some(libc::ETIMEDOUT) => {
                Err(format!("never let go: it gave up after {HELD_AT_MOST:?} before its exec"))
            }
            Ok(Err(e)) => Err(format!("it didn't start: {e}")),
            Ok(Ok(s)) => Ok(s),
        };
        Some(wrote.and(ended))
    }
}

impl Drop for HeldBack {
    fn drop(&mut self) {
        let ended = self.go();
        if !std::thread::panicking()
            && let Some(ended) = ended
        {
            assert!(ended.as_ref().is_ok_and(|s| s.success()), "the held-back child: {ended:?}");
        }
    }
}

/// Saving removes the copy; so does undoing back to the file, and closing the buffer without saving.
#[test]
fn saving_or_dropping_the_edits_removes_the_copy() {
    let dir = place("saved");
    let file = dir.join("project/a.txt");
    std::fs::write(&file, "one\n").unwrap();
    let mut ed = editor(&dir);
    ed.open(&file).unwrap();
    let written = || copies_in(&dir).len() == 1;
    let gone = || copies_in(&dir).is_empty();
    for (edit, undo) in [("cTWO<esc>", ":w<ret>"), ("iX<esc>", "u"), ("iY<esc>", ":bc!<ret>")] {
        feed(&mut ed, edit);
        until(&format!("{edit}'s copy is written"), written);
        feed(&mut ed, undo);
        assert!(ed.flush_saves());
        // (the save's end is an event — it lands on the next one)
        feed(&mut ed, "<esc>");
        until(&format!("{undo} removes the copy"), gone);
    }
    std::fs::remove_dir_all(dir).ok();
}

/// A quit removes this tarae's folder; a signal's quit (the terminal went) keeps it, with what was typed
/// since the last copy.
#[test]
fn a_quit_removes_the_copies_and_a_signal_keeps_them() {
    let dir = place("quit");
    let file = dir.join("project/a.txt");
    std::fs::write(&file, "one\n").unwrap();
    let mut ed = editor(&dir);
    recovery(&mut ed).quiet = Duration::from_secs(60);
    ed.open(&file).unwrap();
    feed(&mut ed, "cTWO<esc>");
    recovery(&mut ed).keep = true;
    ed.recovery_end(false);
    let copies = copies_in(&dir);
    assert_eq!(copies.len(), 1, "written at the end, not after its pause");
    assert_eq!(read_copy(&copies[0]).unwrap().text, "TWOne\n");
    let folder = recovery(&mut ed).folder().to_path_buf();
    drop(ed);
    let mut ed = editor(&dir);
    ed.open(&file).unwrap();
    feed(&mut ed, "cTHREE<esc>");
    recovery(&mut ed).flush();
    assert_eq!(copies_in(&dir).len(), 2);
    ed.recovery_end(false);
    assert_eq!(copies_in(&dir).len(), 1, "the quit's own went");
    assert!(folder.exists(), "the gone tarae's stays");
    std::fs::remove_dir_all(dir).ok();
}

/// Nothing is written for a key's file: in a place that holds keys (by the real path — a link from the
/// project into `~/.ssh` too), named like one, in `editor.recovery-skip`; nor a file over the limit. The
/// plain file beside them is kept.
#[test]
fn nothing_is_written_for_a_keys_file() {
    let dir = place("keys");
    let (home, project) = (dir.join("home"), dir.join("project"));
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::create_dir_all(home.join("notes")).unwrap();
    let files = [
        home.join(".ssh/config"),
        project.join(".env"),
        project.join("id_ed25519"),
        project.join("server.PEM"),
        project.join("x.secret"),
        home.join("notes/n.txt"),
        project.join("big.txt"),
        project.join("plain.txt"),
    ];
    for f in &files {
        std::fs::write(f, "one\n").unwrap();
    }
    std::fs::write(home.join(".ssh/id_work"), "one\n").unwrap();
    std::os::unix::fs::symlink(home.join(".ssh/id_work"), project.join("work")).unwrap();
    let mut ed = editor(&dir);
    ed.config.recovery_skip = vec!["*.secret".into(), "~/notes/".into()];
    for f in files.iter().chain([&project.join("work")]) {
        ed.open(f).unwrap();
        if f.ends_with("big.txt") {
            ed.doc_mut().text = Rope::from("x".repeat(MAX_BYTES));
        }
        feed(&mut ed, "iX<esc>");
    }
    assert!(recovery(&mut ed).flush());
    let kept: Vec<PathBuf> = copies_in(&dir).iter().filter_map(|c| Some(read_copy(c)?.path)).collect();
    assert_eq!(kept, [project.join("plain.txt")]);
    std::fs::remove_dir_all(dir).ok();
}

/// The rules on their own: names in any case with one `*`, paths from a root, from `/` and from `~/`.
#[test]
fn skip_entries_match_names_and_paths() {
    let rules = Rules {
        home: Some(PathBuf::from("/h")),
        skip: vec!["*.Secret".into(), "private/".into(), "/abs/x".into(), "~/n/".into(), " ".into()],
        roots: vec![PathBuf::from("/p")],
    };
    let s = |p: &str| skipped(Path::new(p), &rules);
    assert!(s("/p/a/B.SECRET") && s("/q/x.secret"));
    assert!(s("/p/private/a.rs") && !s("/q/private/a.rs"), "a path is from a root");
    assert!(s("/abs/x/y") && s("/h/n/a") && !s("/h/m/a"));
    assert!(!s("/p/a.rs"), "a blank entry matches nothing");
    let wild = crate::secretnames::wild;
    assert!(wild(".env.*", ".env.local") && !wild(".env.*", ".envrc") && wild("*.pem", "a.pem"));
}

/// A slow disk (each copy takes a second) never holds the keys: typing goes on at full speed while a copy is
/// being written, and nothing the main loop does waits for it.
#[test]
fn the_main_thread_never_waits_on_a_slow_disk() {
    let dir = place("slow");
    let file = dir.join("project/a.txt");
    std::fs::write(&file, "one\n").unwrap();
    let mut ed = editor(&dir);
    let slow = Duration::from_secs(1);
    (recovery(&mut ed).slow, recovery(&mut ed).quiet) = (slow, Duration::ZERO);
    ed.open(&file).unwrap();
    let start = Instant::now();
    let mut worst = Duration::ZERO;
    for k in "iabcdefghijklmnopqrstuvwxyz".chars().map(String::from).chain(["esc".into()]) {
        let t = Instant::now();
        ed.handle_event(Event::Key(key(&k)));
        worst = worst.max(t.elapsed());
    }
    let typed = start.elapsed();
    eprintln!("27 keys while copies take {slow:?}: {typed:?}, worst key {worst:?}");
    assert!(typed < slow / 2, "the keys waited for the copy: {typed:?}");
    assert!(copies_in(&dir).is_empty(), "the first copy is still being written");
    until("the copy lands, later", || copies_in(&dir).len() == 1);
    std::fs::remove_dir_all(dir).ok();
}

/// A child forked while a tarae's lock was open holds it too until it execs (a fork copies every
/// descriptor; close-on-exec closes them only at the exec). The tarae killed meanwhile, its copies are offered
/// once that child lets go — `killed` waits for it, as a real death's next start would find it. (The flake
/// this pins: on a loaded Linux, another test's child not yet exec'd hid a gone tarae's folder from the scan
/// — no card, or the older copy left behind.) A harmless child: `true`, held back 300 ms before its exec.
#[test]
fn a_child_forked_while_the_lock_was_open_doesnt_hide_the_copies() {
    use std::io::Read as _;
    use std::os::fd::AsRawFd as _;
    use std::os::unix::process::CommandExt as _;
    let dir = place("forked");
    let file = dir.join("project/a.txt");
    std::fs::write(&file, "one\n").unwrap();
    let mut ed = editor(&dir);
    ed.open(&file).unwrap();
    feed(&mut ed, "cTWO<esc>");
    until("the copy is written", || copies_in(&dir).len() == 1);
    // The child says it's forked — holding every descriptor, the lock's too — then waits before its exec
    let (mut forked, said) = std::io::pipe().unwrap();
    let fd = said.as_raw_fd();
    let child = std::thread::spawn(move || {
        let mut cmd = std::process::Command::new("true");
        // SAFETY: only async-signal-safe calls (write, nanosleep) between the fork and the exec
        unsafe {
            cmd.pre_exec(move || {
                libc::write(fd, b"f".as_ptr().cast(), 1);
                let t = libc::timespec { tv_sec: 0, tv_nsec: 300_000_000 };
                libc::nanosleep(&t, std::ptr::null_mut());
                Ok(())
            });
        }
        let status = cmd.status();
        drop(said);
        status
    });
    forked.read_exact(&mut [0]).unwrap();
    recovery(&mut ed).killed();
    let ed = started(&dir);
    assert_eq!(cards(&ed).len(), 1, "the copy hidden by the child's hold on the lock");
    assert!(child.join().unwrap().unwrap().success());
    std::fs::remove_dir_all(dir).ok();
}

/// Two gone taraes' copies of one file: the newer is offered, the older goes.
#[test]
fn the_newest_copy_of_a_file_is_the_one_offered() {
    let dir = place("newest");
    let file = left_behind(&dir);
    let mut ed = editor(&dir);
    ed.open(&file).unwrap();
    feed(&mut ed, "cNEWER<esc>");
    until("the second copy is written", || copies_in(&dir).len() == 2);
    recovery(&mut ed).killed();
    let mut ed = started(&dir);
    assert_eq!(cards(&ed).len(), 1);
    assert_eq!(copies_in(&dir).len(), 1, "the older went");
    ed.offer_accept();
    assert_eq!(ed.doc().text.to_string(), "NEWERne\n");
    std::fs::remove_dir_all(dir).ok();
}

/// `editor`, its conversations kept in `dir/chats`, claude a stand-in: what it's sent, never a process.
fn chatting(dir: &Path) -> Editor {
    let mut ed = editor(dir);
    ed.chats_dir = Some(dir.join("chats"));
    let (tx, rx) = channel();
    std::mem::forget(rx);
    ed.claude_lines = Some(tx);
    ed
}

/// `chatting`, its look at what gone taraes left done.
fn chatting_started(dir: &Path) -> Editor {
    let mut ed = chatting(dir);
    ed.recovery_scan();
    settle_until(&mut ed, |ed| ed.recovery.as_ref().is_some_and(|r| r.scans > 0));
    ed
}

/// Conversation `id` of `root` in the store: 41k, its last request `ago` back on the hour's cache.
fn stored(dir: &Path, root: &Path, id: &str, ago: Duration) {
    let title = format!("about {id}");
    let saved = crate::chats::Saved {
        rows: crate::chats::rows_of(&[crate::chat::Msg::new(crate::chat::Role::User, &title, "")]),
        id: id.into(),
        title,
        updated: crate::chats::now(),
        plan: None,
        usage: Some(json!({
            "compacted_to": 41_000,
            "touched": crate::usage::now_ms() - ago.as_millis() as u64,
            "ttl": "1h",
        })),
        places: None,
        seq: crate::chats::seq(),
    };
    crate::chats::save(&dir.join("chats"), root, saved);
}

/// A gone tarae's folder `name`, keeping conversation `id` of `root` — the store holding it, an hour and more cold.
fn chat_gone(dir: &Path, name: &str, root: &Path, id: &str) -> PathBuf {
    stored(dir, root, id, Duration::from_secs(7_200));
    let folder = dir.join("state").join(name);
    chat_left(&folder, 1, root, root, id);
    folder
}

fn root_of(dir: &Path) -> PathBuf {
    chatting(dir).workspace.root.clone()
}

fn conversation(ed: &Editor) -> Option<&str> {
    ed.workspace.claude.chat.as_ref()?.conversation.as_deref()
}

/// The chat's conversation is `id` now, as the keeper is told.
fn in_conversation(ed: &mut Editor, id: Option<&str>) {
    ed.workspace.claude.chat.as_mut().unwrap().conversation = id.map(str::to_string);
    moved(&mut ed.recovery);
    ed.recovery_sync();
    recovery(ed).flush();
}

fn kept_ids(folder: &Path) -> Vec<String> {
    let mut ids: Vec<String> = chats_in(folder).iter().filter_map(|f| read_chat(f)).map(|c| c.id).collect();
    ids.sort();
    ids
}

/// What each session's chat is in stays beside the copies as it changes: a conversation, the next one, none (a new
/// conversation not begun); a session behind its own; gone with its session, and all of it with `editor.recovery` off.
#[test]
fn each_sessions_conversation_is_kept_as_it_changes() {
    let dir = place("kept-chat");
    let mut ed = chatting(&dir);
    crate::chat::open(&mut ed);
    in_conversation(&mut ed, Some("one"));
    let folder = recovery(&mut ed).folder().to_path_buf();
    assert_eq!(kept_ids(&folder), ["one"]);
    let c = read_chat(&chats_in(&folder)[0]).unwrap();
    assert_eq!((&c.root, &c.origin), (&ed.workspace.root, &ed.workspace.root));
    in_conversation(&mut ed, Some("two"));
    assert_eq!(kept_ids(&folder), ["two"]);
    in_conversation(&mut ed, None);
    assert!(kept_ids(&folder).is_empty(), "a new conversation not begun: nothing to continue");
    in_conversation(&mut ed, Some("three"));
    let kid = ed.workspace_add(dir.join("project"));
    ed.with_workspace(kid, |ed| {
        crate::chat::open(ed);
        ed.workspace.claude.chat.as_mut().unwrap().conversation = Some("kid's".into());
    });
    moved(&mut ed.recovery);
    ed.recovery_sync();
    recovery(&mut ed).flush();
    assert_eq!(kept_ids(&folder), ["kid's", "three"]);
    ed.workspaces.retain(|w| w.id != kid);
    moved(&mut ed.recovery);
    ed.recovery_sync();
    recovery(&mut ed).flush();
    assert_eq!(kept_ids(&folder), ["three"], "its session ended");
    ed.config.recovery = false;
    ed.recovery_sync();
    recovery(&mut ed).flush();
    assert!(kept_ids(&folder).is_empty(), "recovery off keeps nothing");
    std::fs::remove_dir_all(dir).ok();
}

/// A crash with nothing unsaved leaves a card of its own, saying the size and that it's read anew; no `c`. `y` continues
/// that conversation, its chat shown, the keys where they were — and the keeper's file and folder go.
#[test]
fn a_conversation_with_no_unsaved_edits_is_a_card_of_its_own() {
    let dir = place("chat-card");
    let folder = chat_gone(&dir, "gone", &root_of(&dir), "left");
    let mut ed = chatting_started(&dir);
    assert!(ed.workspace.claude.chat.is_none(), "nothing comes back by itself");
    let cards = cards(&ed);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].kind(), "recover-chats");
    assert_eq!(cards[0].title, "A conversation left open");
    let q = &cards[0].question;
    assert!(q.contains("41k · ") && q.contains(" · read anew · \"about left\""), "{q}");
    assert!(q.contains("Read anew: past its cache"), "{q}");
    assert_eq!(cards[0].answers(false).iter().map(|a| a.0).collect::<String>(), "yDn");
    let stands = crate::offer::Stands { drawn: true, whole: true };
    let compare = matches!(cards[0].act('c', stands), Some(crate::offer::Act::Compare));
    assert!(!compare, "no copy to compare");
    let at = ed.workspace.here();
    ed.offer_accept();
    assert!(self::cards(&ed).is_empty());
    settle_until(&mut ed, |ed| ed.workspace.claude.continues.is_none());
    assert_eq!(conversation(&ed), Some("left"));
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert_eq!(c.resume.as_deref(), Some("left"));
    assert!(c.msgs.iter().any(|m| m.text == "about left"), "its rows back");
    assert_eq!(ed.workspace.here(), at, "the keys stay");
    recovery(&mut ed).flush();
    assert!(!folder.exists(), "its file and folder go");
    assert!(cards_after(&dir).is_empty(), "asked once");
    std::fs::remove_dir_all(dir).ok();
}

/// The chat a restore brought back says itself where it's drawn; where this frame didn't draw it (too narrow a screen), a
/// toast says so, once.
#[test]
fn the_restored_chat_is_said_only_where_it_isnt_drawn() {
    let said = |t: &crate::editor::Toast| t.text.contains("Restored the chat");
    let dir = place("chat-sight");
    let root = root_of(&dir);
    chat_gone(&dir, "wide", &root, "left");
    let mut buf = Vec::new();
    let mut ed = chatting_started(&dir);
    ed.offer_accept();
    settle_until(&mut ed, |ed| ed.workspace.claude.continues.is_none());
    crate::term::render(&mut ed, &mut buf, 140, 40).unwrap();
    assert!(ed.screen.chat_x().is_some(), "drawn");
    assert!(!ed.toasts.iter().any(said), "nothing said: it's there");
    quit(ed, &dir, || {});
    chat_gone(&dir, "narrow", &root, "left");
    let mut ed = chatting_started(&dir);
    ed.offer_accept();
    crate::term::render(&mut ed, &mut buf, 64, 24).unwrap();
    assert!(!ed.toasts.iter().any(said), "not before its read has landed: it may not come back");
    settle_until(&mut ed, |ed| ed.workspace.claude.continues.is_none());
    buf.clear();
    crate::term::render(&mut ed, &mut buf, 64, 24).unwrap();
    assert!(ed.screen.chat_x().is_none(), "too narrow to draw it");
    assert_eq!(ed.toasts.iter().filter(|t| said(t)).count(), 1);
    buf.clear();
    ed.toasts.clear();
    crate::term::render(&mut ed, &mut buf, 64, 24).unwrap();
    assert!(!ed.toasts.iter().any(said), "once");
    std::fs::remove_dir_all(dir).ok();
}

/// Every way a conversation moves reaches the keeper by itself, nothing looking unless one did (`recovery_sync`'s own
/// check fails a site that forgets to say so): claude's id, a resume that found it gone, a new conversation, a chat
/// closed, a session added and closed.
#[test]
fn every_way_a_conversation_moves_reaches_the_keeper() {
    use crate::llm::StreamEv;
    let conversation_is = |id: &str| StreamEv::Conversation {
        id: id.into(),
        model: "m".into(),
        version: None,
        key_source: None,
    };
    let stream = |ed: &mut Editor, ev: StreamEv| {
        let generation = ed.workspace.claude.chat.as_ref().map_or(0, crate::chat::Chat::generation);
        crate::chat::stream_event(ed, generation, ev);
    };
    let dir = place("chat-moves");
    let mut ed = chatting(&dir);
    crate::chat::open(&mut ed);
    stream(&mut ed, conversation_is("one"));
    let folder = recovery(&mut ed).folder().to_path_buf();
    let told = |ed: &mut Editor| {
        ed.recovery_sync();
        recovery(ed).flush();
        kept_ids(&folder)
    };
    assert_eq!(told(&mut ed), ["one"], "claude gave it its id");
    ed.workspace.claude.chat.as_mut().unwrap().resume = Some("one".into());
    stream(&mut ed, StreamEv::Exit("No conversation found with session ID: one".into()));
    assert!(told(&mut ed).is_empty(), "gone from claude");
    stream(&mut ed, conversation_is("two"));
    assert_eq!(told(&mut ed), ["two"]);
    crate::chat::reset_said(&mut ed, "");
    assert!(told(&mut ed).is_empty(), "a new conversation asked for");
    stream(&mut ed, conversation_is("three"));
    assert_eq!(told(&mut ed), ["three"]);
    crate::chat::close(&mut ed);
    assert!(told(&mut ed).is_empty(), "the chat closed");
    let kid = dir.join("project");
    let id = ed.workspace_add(kid.clone());
    ed.with_workspace(id, |ed| {
        crate::chat::open(ed);
        stream(ed, conversation_is("four"));
    });
    assert_eq!(told(&mut ed), ["four"]);
    assert!(recovery(&mut ed).rules.roots.contains(&kid), "a session added: its root's rules too");
    ed.workspace_close(&kid);
    assert!(told(&mut ed).is_empty(), "its session ended");
    assert!(!recovery(&mut ed).rules.roots.contains(&kid));
    // (one with no chat: nothing of a chat's says it)
    ed.workspace_add(kid.clone());
    told(&mut ed);
    assert!(recovery(&mut ed).rules.roots.contains(&kid), "added, no chat");
    ed.workspace_close(&kid);
    told(&mut ed);
    assert!(!recovery(&mut ed).rules.roots.contains(&kid), "closed, no chat");
    std::fs::remove_dir_all(dir).ok();
}

/// Budget of work: an event touches what changed (#214). With twenty sessions, keys look at none of them; a conversation
/// that moved, each once; nothing after. The roots' compare too: only then.
#[test]
fn an_event_looks_at_the_sessions_only_when_one_moved() {
    const SESSIONS: usize = 20;
    let dir = place("chat-work");
    let mut ed = chatting(&dir);
    for _ in 0..SESSIONS {
        ed.workspace_add(dir.join("project"));
    }
    crate::chat::open(&mut ed);
    in_conversation(&mut ed, Some("one"));
    let looked = recovery(&mut ed).looks;
    feed(&mut ed, "abc");
    ed.handle_event(Event::Key(key("backspace")));
    assert_eq!(recovery(&mut ed).looks, looked, "keys move no session's conversation");
    in_conversation(&mut ed, Some("two"));
    assert_eq!(recovery(&mut ed).looks, looked + SESSIONS + 1, "each once");
    feed(&mut ed, "d");
    assert_eq!(recovery(&mut ed).looks, looked + SESSIONS + 1, "and no more");
    ed.config.recovery = false;
    feed(&mut ed, "e");
    let off = recovery(&mut ed).looks;
    assert_eq!(off, looked + 2 * (SESSIONS + 1), "the setting turned off: looked at once, its files going");
    feed(&mut ed, "f");
    assert_eq!(recovery(&mut ed).looks, off);
    std::fs::remove_dir_all(dir).ok();
}

/// The cards a next start raises.
fn cards_after(dir: &Path) -> Vec<String> {
    let ed = chatting_started(dir);
    self::cards(&ed).iter().map(|o| o.title.clone()).collect()
}

/// The first copy's card carries the conversation, its `y` bringing back both, the rest of the cards their files alone;
/// the folder goes with the last answer.
#[test]
fn a_copys_restore_brings_the_conversation_back_with_it() {
    let dir = place("chat-rides");
    let file = left_behind(&dir);
    let folder = copies_in(&dir)[0].parent().unwrap().to_path_buf();
    let root = root_of(&dir);
    stored(&dir, &root, "left", Duration::from_secs(60));
    chat_left(&folder, 1, &root, &root, "left");
    let other = dir.join("project/b.txt");
    let head = header(&other, &root, None, false, 4);
    std::fs::write(folder.join("8.txt"), format!("{head}\nnew\n")).unwrap();
    let mut ed = chatting_started(&dir);
    let cards = cards(&ed);
    assert_eq!(cards.len(), 2);
    let (first, second) = (&cards[0], &cards[1]);
    assert!(first.title.ends_with("a.txt"), "{}", first.title);
    assert_eq!(first.kind(), "recover");
    let q = &first.question;
    assert!(q.contains("Its conversation comes back with them") && q.contains("41k"), "{q}");
    assert!(q.contains("Discarded, the chat starts fresh too; C-p still lists it."), "{q}");
    assert!(!q.contains("read anew"), "warm: {q}");
    assert!(first.decided_lines().contains(&2), "what `y` decides: its row, at full strength");
    assert!(first.rows.iter().any(|(k, row)| q.split('\n').nth(*k) == Some(row)), "a row, cut at its end");
    assert!(!second.question.contains("conversation"), "{}", second.question);
    ed.offer_accept();
    assert_eq!(ed.doc().path.as_deref(), Some(file.as_path()));
    assert_eq!(ed.doc().text.to_string(), "TWOne\n");
    settle_until(&mut ed, |ed| ed.workspace.claude.continues.is_none());
    assert_eq!(conversation(&ed), Some("left"));
    assert_eq!(self::cards(&ed).len(), 1);
    assert!(folder.exists(), "a card still asks about it");
    ed.offer_discard();
    recovery(&mut ed).flush();
    assert!(!folder.exists(), "the last answer takes the folder");
    std::fs::remove_dir_all(dir).ok();
}

/// `n` leaves the conversation to the next start, nothing back; `D` starts fresh, and it's asked no more.
#[test]
fn discard_starts_fresh_and_not_now_asks_again() {
    let dir = place("chat-discard");
    let folder = chat_gone(&dir, "gone", &root_of(&dir), "left");
    let mut ed = chatting_started(&dir);
    ed.offer_dismiss();
    assert!(cards(&ed).is_empty() && ed.workspace.claude.chat.is_none());
    assert_eq!(kept_ids(&folder), ["left"]);
    quit(ed, &dir, || {});
    let mut ed = chatting_started(&dir);
    assert_eq!(cards(&ed).len(), 1, "asked again");
    ed.offer_discard();
    assert!(ed.workspace.claude.chat.is_none(), "nothing opened");
    assert!(ed.toasts.last().is_some_and(|t| t.text.contains("starts fresh")));
    recovery(&mut ed).flush();
    assert!(!folder.exists(), "its file and folder go");
    assert!(cards_after(&dir).is_empty());
    std::fs::remove_dir_all(dir).ok();
}

/// The same with a copy: `D` on its card takes the copy and starts fresh; `n` keeps both.
#[test]
fn a_copys_answers_take_its_conversation_along() {
    let dir = place("chat-both");
    left_behind(&dir);
    let copy = copies_in(&dir)[0].clone();
    let folder = copy.parent().unwrap().to_path_buf();
    let root = root_of(&dir);
    stored(&dir, &root, "left", Duration::from_secs(60));
    chat_left(&folder, 1, &root, &root, "left");
    let mut ed = chatting_started(&dir);
    ed.offer_dismiss();
    assert!(copy.exists() && kept_ids(&folder) == ["left"], "n keeps both");
    assert!(recovery(&mut ed).held.is_empty(), "its folder let go, nothing left to ask");
    quit(ed, &dir, || {});
    let mut ed = chatting_started(&dir);
    ed.offer_discard();
    assert!(ed.workspace.claude.chat.is_none());
    recovery(&mut ed).flush();
    assert!(!folder.exists(), "both and the folder go");
    std::fs::remove_dir_all(dir).ok();
}

/// The file changed since: the card's warning stays on that line, with a conversation's rows above it.
#[test]
fn the_warning_stays_on_its_line_beside_a_conversation() {
    let dir = place("chat-warn");
    let file = left_behind(&dir);
    std::fs::write(&file, "changed\n").unwrap();
    let folder = copies_in(&dir)[0].parent().unwrap().to_path_buf();
    let root = root_of(&dir);
    stored(&dir, &root, "left", Duration::from_secs(60));
    chat_left(&folder, 1, &root, &root, "left");
    let ed = chatting_started(&dir);
    let card = cards(&ed)[0];
    let warned = card.warning.clone().expect("it warns");
    let line = card.question.split('\n').nth(warned.start).unwrap();
    assert!(line.contains("they'd go over it"), "{line}");
    std::fs::remove_dir_all(dir).ok();
}

/// `:restart`'s tarae brings its chats back itself and passes over what the one before kept of them. An exec that failed,
/// or an error before it, brought none back: the next start offers them. A signal keeps them too, a quit takes them.
#[test]
fn a_conversation_restart_brought_back_is_not_offered_and_a_failed_exec_keeps_it() {
    let dir = place("chat-ends");
    let root = root_of(&dir);
    stored(&dir, &root, "left", Duration::from_secs(60));
    let up = |dir: &Path| {
        let mut ed = chatting(dir);
        crate::chat::open(&mut ed);
        in_conversation(&mut ed, Some("left"));
        ed
    };
    // (`recovery_end(true)`: as `:restart` ends, its exec still to come)
    let mut ed = up(&dir);
    let folder = recovery(&mut ed).folder().to_path_buf();
    ed.recovery_end(true);
    assert_eq!(kept_ids(&folder), ["left"], "kept for the exec's failure");
    quit(ed, &dir, || {});
    let mut ed = chatting(&dir);
    let restored = crate::chat::Restored {
        conversation: Some("left".into()),
        input: String::new(),
        cursor: 0,
        focused: false,
        scroll: 0,
        follow: false,
        at_end: None,
        lazy: true,
    };
    crate::chat::restore(&mut ed, restored);
    ed.recovery_scan();
    settle_until(&mut ed, |ed| ed.recovery.as_ref().is_some_and(|r| r.scans > 0));
    assert!(cards(&ed).is_empty(), "back already");
    assert!(!folder.exists(), "its file and folder went");
    quit(ed, &dir, || {});
    let mut ed = up(&dir);
    let folder = recovery(&mut ed).folder().to_path_buf();
    ed.recovery_end(true);
    quit(ed, &dir, || {});
    assert_eq!(
        cards_after(&dir),
        ["A conversation left open"],
        "no new tarae took over: a failed exec's next start"
    );
    std::fs::remove_dir_all(&folder).ok();
    let mut ed = up(&dir);
    let folder = recovery(&mut ed).folder().to_path_buf();
    recovery(&mut ed).keep = true;
    ed.recovery_end(false);
    assert_eq!(kept_ids(&folder), ["left"], "the signal's");
    std::fs::remove_dir_all(&folder).ok();
    let mut ed = up(&dir);
    let folder = recovery(&mut ed).folder().to_path_buf();
    ed.recovery_end(false);
    assert!(!folder.exists(), "the quit's");
    std::fs::remove_dir_all(dir).ok();
}

/// `y` for a session that isn't here (not back yet, no longer listed): nothing done and nothing lost — said, its folder
/// let go, asked again at the next start.
#[test]
fn a_session_not_here_keeps_its_conversation_for_the_next_start() {
    let dir = place("chat-away");
    let root = root_of(&dir);
    let kid = dir.join("project");
    stored(&dir, &kid, "left", Duration::from_secs(60));
    let folder = dir.join("state/gone");
    chat_left(&folder, 1, &kid, &root, "left");
    let mut ed = chatting_started(&dir);
    assert_eq!(cards(&ed).len(), 1);
    ed.offer_accept();
    assert!(ed.toasts.last().is_some_and(|t| t.text.contains("isn't here")));
    assert!(ed.workspace.claude.chat.is_none());
    assert!(recovery(&mut ed).held.is_empty(), "its folder let go");
    quit(ed, &dir, || {});
    assert_eq!(kept_ids(&folder), ["left"]);
    assert_eq!(cards_after(&dir).len(), 1, "asked again");
    std::fs::remove_dir_all(dir).ok();
}

/// `y` once the store no longer holds it: said, the chat as it was, and no word of a restore. `y` on a chat already in
/// it: nothing to do, no refusal — it's back.
#[test]
fn a_conversation_gone_since_is_said_and_one_already_there_stays() {
    let dir = place("chat-since");
    let root = root_of(&dir);
    chat_gone(&dir, "gone", &root, "left");
    let mut ed = chatting_started(&dir);
    std::fs::remove_file(crate::config::state_file(&dir.join("chats"), &root)).unwrap();
    ed.offer_accept();
    settle_until(&mut ed, |ed| ed.workspace.claude.continues.is_none());
    assert_eq!(conversation(&ed), None);
    assert!(ed.toasts.iter().any(|t| t.text.contains("no longer kept")));
    assert!(!recovery(&mut ed).unseen, "nothing came back");
    quit(ed, &dir, || {});
    chat_gone(&dir, "again", &root, "left");
    let mut ed = chatting_started(&dir);
    crate::chat::open(&mut ed);
    in_conversation(&mut ed, Some("left"));
    let msg = crate::chat::Msg::new(crate::chat::Role::User, "said since", "");
    ed.workspace.claude.chat.as_mut().unwrap().msgs.push(msg);
    ed.offer_accept();
    settle_until(&mut ed, |ed| ed.workspace.claude.continues.is_none());
    assert_eq!(conversation(&ed), Some("left"));
    assert!(ed.toasts.iter().all(|t| !t.text.contains("of its own")), "it's that one");
    assert!(recovery(&mut ed).unseen, "it's where the restore asked");
    quit(ed, &dir, || {});
    // (the store has lost it, the chat hasn't: it's still there, not gone)
    chat_gone(&dir, "third", &root, "left");
    let mut ed = chatting_started(&dir);
    crate::chat::open(&mut ed);
    in_conversation(&mut ed, Some("left"));
    std::fs::remove_file(crate::config::state_file(&dir.join("chats"), &root)).unwrap();
    ed.offer_accept();
    settle_until(&mut ed, |ed| ed.workspace.claude.continues.is_none());
    assert_eq!(conversation(&ed), Some("left"));
    assert!(ed.toasts.iter().all(|t| !t.text.contains("no longer kept")), "it's in the chat");
    assert!(recovery(&mut ed).unseen);
    std::fs::remove_dir_all(dir).ok();
}

/// Several: the card counts them and names each row by its session, the start's first; `n` lets go of both folders and
/// `y` takes both.
#[test]
fn several_conversations_are_one_card_named_the_starts_first() {
    let dir = place("chat-several");
    let (a, z) = (dir.join("project/a"), dir.join("project/z"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&z).unwrap();
    let store = dir.join("chats");
    stored(&dir, &a, "of a", Duration::from_secs(60));
    stored(&dir, &z, "of z", Duration::from_secs(60));
    let state = dir.join("state");
    chat_left(&state.join("one"), 1, &a, &z, "of a");
    chat_left(&state.join("two"), 2, &z, &z, "of z");
    // (z is where this one started: first, though it sorts last)
    let left = scan(&state, Path::new("own"), &z, &[], Some(&store), &[]).left;
    assert_eq!(left.iter().map(|l| l.root.clone()).collect::<Vec<_>>(), [z.clone(), a.clone()]);
    let card = chats_card(left, "C-p");
    assert_eq!(card.title, "2 conversations left open");
    let lines: Vec<&str> = card.question.split('\n').collect();
    assert_eq!(lines[0], "The chats were in them when tarae ended:");
    assert!(lines[1].starts_with("z · 41k") && lines[2].starts_with("a · 41k"), "{lines:?}");
    assert_eq!(card.rows.iter().map(|r| r.0).collect::<Vec<_>>(), [1, 2]);
    let up = |dir: &Path| {
        let mut ed = chatting(dir);
        ed.workspace_add(a.clone());
        ed.workspace_add(z.clone());
        ed.recovery = Some(Recovery::new(state.clone(), z.clone()));
        ed.recovery_scan();
        settle_until(&mut ed, |ed| !cards(ed).is_empty());
        ed
    };
    let mut ed = up(&dir);
    assert_eq!(cards(&ed).len(), 1);
    ed.offer_dismiss();
    assert!(recovery(&mut ed).held.is_empty(), "both folders let go");
    quit(ed, &dir, || {});
    let mut ed = up(&dir);
    ed.offer_accept();
    until("both folders go", || !state.join("one").exists() && !state.join("two").exists());
    drop(ed);
    std::fs::remove_dir_all(dir).ok();
}

/// Only what can be continued is offered: a conversation the store no longer holds goes (its folder too, with nothing
/// else in it); of two a session left, the newest; another project's stays for a tarae started there.
#[test]
fn what_can_be_continued_is_offered_and_another_projects_is_left() {
    let dir = place("chat-which");
    let root = root_of(&dir);
    let unknown = dir.join("state/unknown");
    chat_left(&unknown, 1, &root, &root, "no longer kept");
    let older = chat_gone(&dir, "older", &root, "older");
    let newer = chat_gone(&dir, "newer", &root, "newer");
    let elsewhere = dir.join("project");
    let theirs = dir.join("state/theirs");
    chat_left(&theirs, 1, &elsewhere, &elsewhere, "theirs");
    let ed = chatting_started(&dir);
    let cards = cards(&ed);
    assert_eq!(cards.len(), 1);
    assert!(cards[0].question.contains("about newer"), "{}", cards[0].question);
    assert!(!unknown.exists(), "nothing to continue");
    assert!(kept_ids(&older).is_empty(), "the older went");
    assert_eq!(kept_ids(&newer), ["newer"]);
    assert_eq!(kept_ids(&theirs), ["theirs"], "another project's, its folder kept");
    std::fs::remove_dir_all(dir).ok();
}

/// A row says what's known: a name only among several, a size as reported, `read anew` only past the TTL — never a guess.
#[test]
fn a_conversations_row_says_what_is_known() {
    let row = |context, cold, named| {
        let past = crate::chats::Past { id: "c".into(), title: "why?\nand more".into(), updated: 0, context };
        let name = "main".to_string();
        let (file, dir) = (PathBuf::new(), PathBuf::new());
        let left = Left { root: PathBuf::new(), name, past, cold, at: UNIX_EPOCH, file, dir };
        chat_row(&left, named, 7_200)
    };
    assert_eq!(row(Some(41_000), false, false), "41k · 2h ago · \"why?\"");
    assert_eq!(row(Some(41_000), true, false), "41k · 2h ago · read anew · \"why?\"");
    assert_eq!(row(None, false, true), "main · 2h ago · \"why?\"");
    use crate::usage::Usage;
    let kept = |touched: u64, ttl: &str| json!({ "touched": touched, "ttl": ttl });
    assert!(Usage::kept_cold(&kept(0, "1h"), 3_600_000));
    assert!(!Usage::kept_cold(&kept(0, "1h"), 3_599_999));
    assert!(Usage::kept_cold(&kept(0, "5m"), 300_000));
    assert!(!Usage::kept_cold(&json!({ "touched": 0 }), u64::MAX), "its TTL not reported");
}

/// A row begins with its session's name only among several (`Offer.named` says so where `chat_row` does).
#[test]
fn the_named_rows_are_those_with_a_name() {
    let (a, b) = (Path::new("/a"), Path::new("/b"));
    assert!(chats_card_of(&[("one", a)]).named.is_empty());
    let card = chats_card_of(&[("one", a), ("three", b)]);
    let want = vec![(1, 3, a.to_path_buf()), (2, 5, b.to_path_buf())];
    assert_eq!(card.named, want);
    let lines: Vec<&str> = card.question.split('\n').collect();
    assert!(lines[1].starts_with("one ·") && lines[2].starts_with("three ·"), "{lines:?}");
    // (riding on a copy's card: its rows below its first line and the lead)
    let copy = Found {
        path: PathBuf::from("/a/f"),
        file: PathBuf::new(),
        dir: PathBuf::new(),
        text: Rope::new(),
        at: UNIX_EPOCH,
        disk: Disk::Same,
        base: Rope::new(),
        added: 1,
        removed: 0,
        chats: lefts_of(&[("one", a), ("three", b)]),
    };
    let card = super::card(copy, a, "C-p", crate::log::Zone::Here);
    assert_eq!(card.named, vec![(2, 3, a.to_path_buf()), (3, 5, b.to_path_buf())]);
    let lines: Vec<&str> = card.question.split('\n').collect();
    assert!(lines[2].starts_with("one ·") && lines[3].starts_with("three ·"), "{lines:?}");
}

/// `y` after you began a conversation of your own: yours stays, said, the one it left a key away.
#[test]
fn a_restore_answered_late_stays_off_what_you_began() {
    let dir = place("chat-late");
    chat_gone(&dir, "gone", &root_of(&dir), "left");
    let mut ed = chatting_started(&dir);
    crate::chat::open(&mut ed);
    in_conversation(&mut ed, Some("mine"));
    let msg = crate::chat::Msg::new(crate::chat::Role::User, "mine", "");
    ed.workspace.claude.chat.as_mut().unwrap().msgs.push(msg);
    ed.offer_accept();
    settle_until(&mut ed, |ed| ed.workspace.claude.continues.is_none());
    assert_eq!(conversation(&ed), Some("mine"));
    assert!(ed.toasts.last().is_some_and(|t| t.text.contains("conversation of its own")));
    assert!(!recovery(&mut ed).unseen, "nothing came back");
    std::fs::remove_dir_all(dir).ok();
}

/// A session's name where a row's cut falls: in its colour while it's there whole, plain once the cut reaches it — each
/// byte of the `…` met, wide characters too. (A `.chat` stays until it's answered: a row that can't be drawn is every
/// start's at that width.)
#[test]
fn a_name_at_a_rows_cut_is_in_its_colour_whole_or_not_at_all() {
    let dir = place("chat-cut");
    let mut ed = chatting(&dir);
    let (root, other) = (ed.workspace.root.clone(), dir.join("other"));
    let mut buf = Vec::new();
    for letter in ["n", "한"] {
        let (mut whole, mut reached) = (0, 0);
        for n in 1..70 {
            let name = letter.repeat(n);
            ed.push_offer(chats_card_of(&[(&name, &root), ("other", &other)]));
            buf.clear();
            crate::term::render(&mut ed, &mut buf, 160, 30).unwrap();
            let row = format!("{name} · ");
            let spots = ed.screen.cut_spots.borrow();
            let spot = spots.iter().find_map(|s| match &s.3 {
                crate::whole::Cut::Text(w) if w.text.starts_with(&row) => Some((s.2 - s.1, w)),
                _ => None,
            });
            let (cells, cut) = spot.unwrap_or_else(|| panic!("{name}'s row, cut"));
            // (the row's cells less its `…`)
            let there = unicode_width::UnicodeWidthStr::width(name.as_str()) < cells;
            let coloured = cut.pieces.first().map(|p| p.0.clone());
            assert_eq!(coloured, there.then_some(0..name.len()), "{name} in {cells} cells");
            *(if there { &mut whole } else { &mut reached }) += 1;
            drop(spots);
            ed.offers.clear();
        }
        assert!(whole > 0 && reached > 0, "{letter}: {whole} whole, {reached} reached");
    }
    std::fs::remove_dir_all(dir).ok();
}

/// A conversation is kept under what the chrome calls its session, and that's what the next start's card says: the home
/// session its branch — its folder's name only until the branch is known, told again once it is and as it moves, the
/// conversation the same —, a session behind its name.
#[test]
fn a_conversation_is_kept_under_the_name_the_chrome_calls_its_session() {
    let dir = place("chat-name");
    let root = root_of(&dir);
    let kid = dir.join("project");
    let ago = Duration::from_secs(60);
    stored(&dir, &root, "home's", ago);
    stored(&dir, &kid, "kid's", ago);
    let mut ed = chatting(&dir);
    crate::chat::open(&mut ed);
    in_conversation(&mut ed, Some("home's"));
    let folder = recovery(&mut ed).folder().to_path_buf();
    let kept = || -> Vec<(String, String)> {
        let mut all: Vec<_> = chats_in(&folder).iter().filter_map(|f| read_chat(f)).collect();
        all.sort_by_key(|c| c.id.clone());
        all.into_iter().map(|c| (c.id, c.name)).collect()
    };
    let told = |id: &str, name: &str| (id.to_string(), name.to_string());
    let own = ed.workspace.name.clone();
    assert_eq!(kept(), [told("home's", &own)], "the branch not known yet");
    let branch = |ed: &mut Editor, name: &str| {
        let root = ed.workspace.root.clone();
        crate::git::watch_round(ed, &root, (Some(name.into()), None), false);
        ed.recovery_sync();
        recovery(ed).flush();
    };
    branch(&mut ed, "trunk");
    assert_eq!(kept(), [told("home's", "trunk")]);
    branch(&mut ed, "topic");
    assert_eq!(kept(), [told("home's", "topic")], "it moved");
    let id = ed.workspace_add(kid.clone());
    ed.with_workspace(id, |ed| {
        crate::chat::open(ed);
        ed.workspace.claude.chat.as_mut().unwrap().conversation = Some("kid's".into());
    });
    moved(&mut ed.recovery);
    ed.recovery_sync();
    recovery(&mut ed).flush();
    assert_eq!(kept(), [told("home's", "topic"), told("kid's", "project")]);
    recovery(&mut ed).killed();
    drop(ed);
    let mut ed = chatting(&dir);
    ed.workspace_add(kid);
    ed.recovery_scan();
    settle_until(&mut ed, |ed| ed.recovery.as_ref().is_some_and(|r| r.scans > 0));
    let card = cards(&ed)[0];
    let lines: Vec<&str> = card.question.split('\n').collect();
    assert!(lines[1].starts_with("topic · ") && lines[2].starts_with("project · "), "{lines:?}");
    assert_eq!(card.named[0], (1, "topic".len(), root), "its colour's bytes, its branch's");
    std::fs::remove_dir_all(dir).ok();
}

/// The cards a next start raises with a session of checkout `kid` behind.
fn cards_after_with_kid(dir: &Path, kid: &Path) -> usize {
    let mut ed = chatting(dir);
    ed.workspace_add(kid.to_path_buf());
    ed.recovery_scan();
    settle_until(&mut ed, |ed| ed.recovery.as_ref().is_some_and(|r| r.scans > 0));
    let n = cards(&ed).len();
    ed.offer_dismiss();
    quit(ed, dir, || {});
    n
}

fn session(ed: &Editor, id: WorkspaceId) -> &crate::workspace::Workspace {
    std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|w| w.id == id).unwrap()
}

/// A session behind: `y` continues that conversation in its chat, whichever the store has first, with no claude until its
/// first message — a chat as any, so its conversation is kept by this tarae at once: a second crash before you go there
/// loses nothing. A new conversation asked for meanwhile drops it. `D` leaves it to start fresh, never going on by itself.
#[test]
fn a_session_behind_continues_by_that_id_or_starts_fresh() {
    let dir = place("chat-behind");
    let root = root_of(&dir);
    let kid = dir.join("project");
    stored(&dir, &kid, "left", Duration::from_secs(7_200));
    stored(&dir, &kid, "later", Duration::from_secs(60));
    let back = |ed: &mut Editor| {
        let id = ed.workspace_add(dir.join("project"));
        ed.workspace_by_id(id).unwrap().claude.brought_back = true;
        id
    };
    let gone = |name: &str| chat_left(&dir.join("state").join(name), 1, &kid, &root, "left");
    let restored = |t: &crate::editor::Toast| t.text.contains("Restored");
    gone("first");
    let mut ed = chatting(&dir);
    let id = back(&mut ed);
    ed.recovery_scan();
    settle_until(&mut ed, |ed| !cards(ed).is_empty());
    ed.offer_accept();
    assert!(!ed.toasts.iter().any(restored), "not before its read has landed");
    let chat = session(&ed, id).claude.chat.as_ref().expect("its chat, at once");
    assert!(!crate::chat::started(chat), "before its conversation's read lands too");
    settle_until(&mut ed, |ed| session(ed, id).claude.continues.is_none());
    let chat = session(&ed, id).claude.chat.as_ref().expect("its chat, whatever comes before you go there");
    assert_eq!(chat.conversation.as_deref(), Some("left"));
    assert!(ed.toasts.last().is_some_and(|t| t.text == "project · Restored the chat"));
    assert!(!crate::chat::started(chat), "no claude until its first message");
    ed.recovery_sync();
    recovery(&mut ed).flush();
    let own = recovery(&mut ed).folder().to_path_buf();
    assert_eq!(kept_ids(&own), ["left"], "kept here now");
    recovery(&mut ed).killed();
    drop(ed);
    assert_eq!(cards_after_with_kid(&dir, &kid), 1, "a second crash: offered again");
    gone("second");
    let mut ed = chatting(&dir);
    let id = back(&mut ed);
    ed.recovery_scan();
    settle_until(&mut ed, |ed| !cards(ed).is_empty());
    ed.offer_accept();
    ed.with_workspace(id, |ed| crate::chat::reset_said(ed, ""));
    assert_eq!(session(&ed, id).claude.continues, None, "a new conversation asked for");
    settle_until(&mut ed, |ed| session(ed, id).claude.chat.as_ref().is_some_and(|c| !c.past.is_empty()));
    let conv = session(&ed, id).claude.chat.as_ref().and_then(|c| c.conversation.as_deref());
    assert_eq!(conv, None, "the read landed after: nothing continued");
    assert!(!ed.toasts.iter().any(restored), "nor said");
    quit(ed, &dir, || {});
    gone("third");
    let mut ed = chatting(&dir);
    let id = back(&mut ed);
    ed.recovery_scan();
    settle_until(&mut ed, |ed| !cards(ed).is_empty());
    ed.offer_discard();
    assert!(!session(&ed, id).claude.brought_back, "discard starts fresh");
    std::fs::remove_dir_all(dir).ok();
}

/// A session the list brought back goes on with no word of a restore: no crash, nothing restored.
#[test]
fn a_session_brought_back_says_nothing_of_a_restore() {
    let dir = place("chat-listed");
    let kid = dir.join("project");
    stored(&dir, &kid, "its own", Duration::from_secs(60));
    let mut ed = chatting(&dir);
    let id = ed.workspace_add(kid);
    ed.workspace_by_id(id).unwrap().claude.brought_back = true;
    ed.with_workspace(id, crate::chat::open);
    settle_until(&mut ed, |ed| !session(ed, id).claude.brought_back);
    let chat = session(&ed, id).claude.chat.as_ref().unwrap();
    assert_eq!(chat.conversation.as_deref(), Some("its own"));
    assert!(ed.toasts.iter().all(|t| !t.text.contains("Restored")));
    std::fs::remove_dir_all(dir).ok();
}

/// A session behind whose conversation can't come back: what became of it is said once it's known, by that session's
/// name, and never that it was restored — the store has lost it, or its chat has one of its own.
#[test]
fn a_restore_behind_that_cant_come_back_says_so_by_its_name() {
    let dir = place("chat-behind-not");
    let root = root_of(&dir);
    let kid = dir.join("project");
    let up = |name: &str| {
        stored(&dir, &kid, "left", Duration::from_secs(60));
        chat_left(&dir.join("state").join(name), 1, &kid, &root, "left");
        let mut ed = chatting(&dir);
        let id = ed.workspace_add(kid.clone());
        ed.recovery_scan();
        settle_until(&mut ed, |ed| !cards(ed).is_empty());
        (ed, id)
    };
    let (mut ed, id) = up("gone");
    std::fs::remove_file(crate::config::state_file(&dir.join("chats"), &kid)).unwrap();
    ed.offer_accept();
    settle_until(&mut ed, |ed| session(ed, id).claude.continues.is_none());
    assert!(ed.toasts.iter().any(|t| t.text == "project · that conversation is no longer kept"));
    assert!(ed.toasts.iter().all(|t| !t.text.contains("Restored")));
    quit(ed, &dir, || {});
    let (mut ed, id) = up("own");
    ed.with_workspace(id, |ed| {
        crate::chat::open(ed);
        let msg = crate::chat::Msg::new(crate::chat::Role::User, "mine", "");
        ed.workspace.claude.chat.as_mut().unwrap().msgs.push(msg);
    });
    ed.offer_accept();
    settle_until(&mut ed, |ed| session(ed, id).claude.continues.is_none());
    let said =
        |t: &crate::editor::Toast| t.text.starts_with("project · the chat has a conversation of its own");
    assert!(ed.toasts.iter().any(said), "{:?}", ed.toasts.last().map(|t| &t.text));
    assert!(ed.toasts.iter().all(|t| !t.text.contains("Restored")));
    std::fs::remove_dir_all(dir).ok();
}
