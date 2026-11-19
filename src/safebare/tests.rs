//! #610, landing 3: the offer to make git use a bare repository only when named — on a scratch global config only.

use std::path::{Path, PathBuf};

use super::*;
use crate::key::{Code, Key};

/// A scratch home — its `.gitconfig` the global git reads and writes — and the store beside it, in a test's own folder.
fn home(name: &str) -> (crate::scratch::Scratch, PathBuf) {
    let s = crate::scratch::Scratch::new(name);
    let dir = std::fs::canonicalize(s.path()).unwrap();
    (s, dir)
}

/// An editor reading and writing `dir`'s git config, its answers kept in `dir/state/safe-bare` — one start of tarae.
fn started(dir: &Path) -> Editor {
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = dir.to_path_buf();
    ed.workspace.viewer.keys_to_code();
    ed.safe_bare.home = Some(GitHome::Scratch(dir.to_path_buf()));
    ed.safe_bare.store = Some(dir.join("state/safe-bare"));
    ed
}

/// git's setting read as the git watch's first round reads it, landed.
fn read(ed: &mut Editor) {
    read_once(ed);
    crate::test_results::settle_until(ed, |ed| ed.safe_bare.setting != Setting::Unread);
}

/// A turn ends; `ran`: a command ran in it.
fn turn(ed: &mut Editor, ran: bool) {
    ed.workspace.claude.ran_in_turn = ran;
    turn_over(ed);
}

fn cards(ed: &Editor) -> usize {
    ed.offers.iter().filter(|o| o.kind() == "safe-bare").count()
}

/// The front card drawn, given the keys and read, then `c` on it — as a person's hand does.
fn press(ed: &mut Editor, c: char) {
    crate::term::render(ed, &mut Vec::new(), 140, 40).unwrap();
    let o = (ed.offers[0].id, ed.offers[0].rev);
    ed.offer_read_whole(o);
    ed.card_focus = Some(o);
    ed.handle_key(Key::plain(Code::Char(c)));
}

/// The screen as a frame `w`×`h` draws it.
fn screen(ed: &mut Editor, w: u16, h: u16) -> String {
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, w, h).unwrap();
    let mut vt = vt100::Parser::new(h, w, 0);
    vt.process(&bytes);
    vt.screen().contents()
}

fn said(ed: &Editor, text: &str) -> bool {
    ed.toasts.iter().any(|t| t.text == text)
}

const WRITTEN: &str = "[safe]\n\tbareRepository = explicit\n";

/// Only the last `system` or `global` value decides: tarae's own `-c` and a `GIT_CONFIG_COUNT` (both `command`) never
/// count, nor `local`; a git before 2.38 is never offered, the version it said kept. Breaks if a `command` line counts.
#[test]
fn only_system_and_global_decide() {
    let v = "git version 2.39.5 (Apple Git-154)\n";
    assert_eq!(judge(v, "global\texplicit\ncommand\texplicit\n"), Setting::Explicit("global".into()));
    assert_eq!(judge(v, "system\texplicit\n"), Setting::Explicit("system".into()));
    assert_eq!(judge(v, "command\texplicit\n"), Setting::Unset, "SAFE's -c, a GIT_CONFIG_COUNT");
    assert_eq!(judge(v, "global\tall\ncommand\texplicit\n"), Setting::Unset);
    assert_eq!(judge(v, "local\texplicit\n"), Setting::Unset, "git doesn't honour local");
    assert_eq!(judge(v, "system\texplicit\nglobal\tall\n"), Setting::Unset, "the last decides");
    assert_eq!(judge("git version 2.37.1\n", "global\texplicit\n"), Setting::Old("2.37.1".into()));
    assert!(matches!(judge("oops", ""), Setting::Failed(_)));
}

/// Exactly `explicit`, as git compares it: git 2.54 stops at `Explicit` ("fatal: bad config variable
/// 'safe.barerepository'"), so tarae doesn't take it for set, nor one with a space git kept from its quotes. Breaks if
/// the value is compared loosely.
#[test]
fn only_explicit_exactly_is_explicit() {
    let v = "git version 2.54.0 (Apple Git-157)\n";
    assert_eq!(judge(v, "global\tExplicit\n"), Setting::Unset);
    assert_eq!(judge(v, "global\t explicit\n"), Setting::Unset);
    assert_eq!(judge(v, "global\texplicit\n"), Setting::Explicit("global".into()));
}

/// Read by real git on a scratch global: unset with tarae's `-c` on the read and a global that lacks it, and with a
/// `GIT_CONFIG_COUNT` saying `explicit` — offered; once the global says it, not.
#[test]
fn the_command_scope_never_counts_as_set() {
    let (_s, dir) = home("safebare-scope");
    std::fs::write(dir.join(".gitconfig"), "[user]\n\tname = t\n").unwrap();
    let gh = GitHome::Scratch(dir.clone());
    assert_eq!(setting(&gh), Setting::Unset, "SAFE's -c, a global without it");
    let (_s2, counted) = home("safebare-count");
    let env = [
        ("GIT_CONFIG_COUNT", "1"),
        ("GIT_CONFIG_KEY_0", "safe.bareRepository"),
        ("GIT_CONFIG_VALUE_0", "explicit"),
    ];
    crate::gitrepo::env_for(&counted, &env);
    assert_eq!(setting(&GitHome::Scratch(counted)), Setting::Unset, "GIT_CONFIG_COUNT");
    std::fs::write(dir.join(".gitconfig"), WRITTEN).unwrap();
    assert_eq!(setting(&gh), Setting::Explicit("global".into()));
}

/// The card comes at the end of the first turn a command ran in, once since tarae started; a turn with no run doesn't
/// count; not once the global says `explicit`. Breaks if a turn with no run counts or the card comes twice.
#[test]
fn offered_once_at_the_first_turn_a_command_ran_in() {
    let (_s, dir) = home("safebare-once");
    let mut ed = started(&dir);
    turn(&mut ed, true);
    assert_eq!(cards(&ed), 0, "the setting not read yet");
    read(&mut ed);
    turn(&mut ed, false);
    assert_eq!(cards(&ed), 0, "no run in the turn");
    turn(&mut ed, true);
    assert_eq!(cards(&ed), 1, "the first turn a command ran in");
    let o = ed.offers.iter().find(|o| o.kind() == "safe-bare").unwrap();
    assert_eq!(o.title, TITLE);
    assert!(o.question.contains(&format!("\n\n{COMMAND}\n\n")), "{}", o.question);
    assert!(!o.yours, "a turn's end brought it: it waits");
    // (its hints: waiting, its way in; focused, `esc back` — then `y Write it · n Never`, `never-install`'s word)
    let waiting = screen(&mut ed, 140, 40);
    assert!(waiting.contains("C-q answer · y Write it · n Never"), "{waiting}");
    ed.card_focus = Some((ed.offers[0].id, ed.offers[0].rev));
    let focused = screen(&mut ed, 140, 40);
    assert!(focused.contains("esc back · y Write it · n Never"), "{focused}");
    ed.offers.clear();
    turn(&mut ed, true);
    assert_eq!(cards(&ed), 0, "once since tarae started (esc: the next start)");
    // (the next start, unanswered: again)
    let mut ed = started(&dir);
    read(&mut ed);
    turn(&mut ed, true);
    assert_eq!(cards(&ed), 1, "unanswered: asked again at the next start");
    std::fs::write(dir.join(".gitconfig"), WRITTEN).unwrap();
    let mut ed = started(&dir);
    read(&mut ed);
    turn(&mut ed, true);
    assert_eq!(cards(&ed), 0, "the global says explicit");
}

/// `n`: `no <date>` kept, said, and never asked again at a later start; nothing written to git's config. Breaks if the
/// store isn't read.
#[test]
fn never_is_kept_and_not_asked_again() {
    let (_s, dir) = home("safebare-never");
    let mut ed = started(&dir);
    read(&mut ed);
    turn(&mut ed, true);
    press(&mut ed, 'n');
    assert_eq!(cards(&ed), 0);
    assert!(said(&ed, NEVER_SAID), "{:?}", ed.toasts);
    let store = dir.join("state/safe-bare");
    // (the write's end, a whole line: the file is there from its open on, before its words — #636)
    let whole = || std::fs::read_to_string(&store).ok().filter(|s| s.ends_with('\n'));
    crate::test_results::settle_saying(
        &mut ed,
        |_| whole().is_some(),
        |_| format!("the store holds {:?}", std::fs::read_to_string(&store)),
    );
    let kept = whole().unwrap();
    assert!(kept.starts_with("no 20") && kept.ends_with('\n') && kept.lines().count() == 1, "{kept:?}");
    assert!(!dir.join(".gitconfig").exists(), "nothing written to git's config");
    let mut ed = started(&dir);
    read(&mut ed);
    turn(&mut ed, true);
    assert_eq!(cards(&ed), 0, "never asked again");
}

/// `y`: git writes exactly `[safe] bareRepository = explicit`, replacing an `all` in place; S6 names the file
/// `--show-origin` gave (home as `~`); `yes <date>` kept; not asked again. Breaks if the write isn't git's own.
#[test]
fn yes_writes_exactly_and_names_the_file() {
    let (_s, dir) = home("safebare-yes");
    let mut ed = started(&dir);
    read(&mut ed);
    turn(&mut ed, true);
    press(&mut ed, 'y');
    let done = "git: safe.bareRepository = explicit, in ~/.gitconfig";
    crate::test_results::settle_until(&mut ed, |ed| said(ed, done));
    assert_eq!(std::fs::read_to_string(dir.join(".gitconfig")).unwrap(), WRITTEN);
    assert_eq!(ed.safe_bare.setting, Setting::Explicit("global".into()), "read again at once");
    let kept = std::fs::read_to_string(dir.join("state/safe-bare")).unwrap();
    assert!(kept.starts_with("yes 20"), "{kept:?}");
    let mut ed = started(&dir);
    read(&mut ed);
    turn(&mut ed, true);
    assert_eq!(cards(&ed), 0);
    // (an `all` replaced in place)
    let (_s2, dir) = home("safebare-all");
    std::fs::write(dir.join(".gitconfig"), "[safe]\n\tbareRepository = all\n").unwrap();
    assert_eq!(write(&GitHome::Scratch(dir.clone())).as_deref(), Ok("~/.gitconfig"));
    assert_eq!(std::fs::read_to_string(dir.join(".gitconfig")).unwrap(), WRITTEN);
}

/// A write git refuses (its config locked) says git's words and keeps nothing — no answer in the store. Breaks if the
/// answer is kept before the write lands.
#[test]
fn a_failing_write_keeps_nothing() {
    let (_s, dir) = home("safebare-fail");
    std::fs::write(dir.join(".gitconfig.lock"), "").unwrap();
    let mut ed = started(&dir);
    read(&mut ed);
    turn(&mut ed, true);
    press(&mut ed, 'y');
    fn failed(ed: &Editor) -> Option<String> {
        ed.toasts.iter().find(|t| t.text.starts_with("git config --global failed — ")).map(|t| t.text.clone())
    }
    crate::test_results::settle_until(&mut ed, |ed| failed(ed).is_some());
    let t = failed(&ed).unwrap();
    assert!(t.ends_with("; nothing written") && t.contains("lock"), "{t}");
    assert!(!dir.join("state/safe-bare").exists(), "nothing kept");
    assert!(!dir.join(".gitconfig").exists());
    assert_eq!(ed.safe_bare.setting, Setting::Unset);
}

/// The write went through but where it went couldn't be asked (`--show-origin`'s git didn't start): said as "your global
/// git config", never as a failure — `answer` would say "nothing written" and keep no `yes`. Only a seam fails the second
/// run alone: both go to the same git on the same home, and nothing runs between them. Breaks if the second run's
/// failure ends the write.
#[test]
fn where_it_went_unasked_is_still_written() {
    let (_s, dir) = home("safebare-origin");
    let runs = std::cell::Cell::new(0);
    let git = |h: &GitHome, args: &[&str]| {
        runs.set(runs.get() + 1);
        if runs.get() == 1 { crate::gitrepo::global(h, args) } else { Err("git couldn't start".into()) }
    };
    assert_eq!(write_by(&GitHome::Scratch(dir.clone()), &git).as_deref(), Ok("your global git config"));
    assert_eq!(runs.get(), 2);
    assert_eq!(std::fs::read_to_string(dir.join(".gitconfig")).unwrap(), WRITTEN);
}

/// The command: the setting already explicit (S8, with its scope), a git too old (S12, the version read) — else the
/// card, after `n` too.
#[test]
fn the_command_says_why_it_doesnt_ask() {
    let (_s, dir) = home("safebare-command");
    let mut ed = started(&dir);
    told(&mut ed, Setting::Old("2.37.1".into()));
    assert!(said(&ed, "git 2.37.1 ignores safe.bareRepository — 2.38 and later have it"), "{:?}", ed.toasts);
    ed.safe_bare.answered = true;
    command(&mut ed);
    crate::test_results::settle_until(&mut ed, |ed| cards(ed) == 1);
    std::fs::write(dir.join(".gitconfig"), WRITTEN).unwrap();
    ed.offers.clear();
    command(&mut ed);
    let s8 = "Already set — safe.bareRepository = explicit, global";
    crate::test_results::settle_until(&mut ed, |ed| said(ed, s8));
    assert_eq!(cards(&ed), 0);
}

/// A repository with a folder git takes for a repository committed (`a/`) — the first-trust card's (S3).
fn checkout(dir: &Path) -> PathBuf {
    let root = dir.join("repo");
    let f = root.join("a");
    std::fs::create_dir_all(f.join("objects/info")).unwrap();
    std::fs::create_dir_all(f.join("refs/heads")).unwrap();
    std::fs::write(f.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(f.join("objects/info/keep"), "").unwrap();
    std::fs::write(f.join("refs/heads/keep"), "").unwrap();
    for args in [&["init", "-q"][..], &["add", "-A"], &["commit", "-q", "-m", "base"]] {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"])
            .args(["-c", "user.email=t@example.invalid", "-c", "user.name=t"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    root
}

/// The look-alike card (S3) carries the offer's line while git's setting isn't explicit, and S4 doesn't also come by
/// itself; once `:git-safe-bare`'s `y` writes it, the line drops from the card in sight, no restart. Breaks if the
/// setting isn't read again after the write.
#[test]
fn the_offer_rides_on_a_look_alike_card_and_drops_once_written() {
    let (_s, dir) = home("safebare-rides");
    let root = checkout(&dir);
    let mut ed = started(&dir);
    ed.workspace.root = root.clone();
    ed.trust_path = Some(dir.join("trust.json"));
    read(&mut ed);
    crate::clock::hold();
    let scan = crate::snapshot::scan(&root, None).unwrap();
    crate::clock::release();
    ed.look_alikes_found(&root, scan);
    let look = |ed: &Editor| ed.offers.iter().find(|o| o.kind() == "look-alike").map(|o| o.question.clone());
    crate::test_results::settle_until(&mut ed, |ed| look(ed).is_some());
    let tail = format!("\n\n{}", line(false));
    assert!(look(&ed).unwrap().ends_with(&tail), "{:?}", look(&ed));
    assert_eq!(tail, "\n\nRun :git-safe-bare to make git refuse it.");
    turn(&mut ed, true);
    assert_eq!(cards(&ed), 0, "the look-alike card carried it");
    // (typed: the card the command asked for is the person's own — in front)
    for c in ":git-safe-bare".chars() {
        ed.handle_key(Key::plain(Code::Char(c)));
    }
    ed.handle_key(Key::plain(Code::Enter));
    crate::test_results::settle_until(&mut ed, |ed| cards(ed) == 1);
    let kinds: Vec<_> = ed.offers.iter().map(|o| (o.kind(), o.yours)).collect();
    assert_eq!(ed.offers[0].kind(), "safe-bare", "{kinds:?}");
    press(&mut ed, 'y');
    crate::test_results::settle_until(&mut ed, |ed| ed.safe_bare.setting != Setting::Unset);
    assert_eq!(std::fs::read_to_string(dir.join(".gitconfig")).unwrap(), WRITTEN);
    let q = look(&ed).unwrap();
    assert!(!q.contains(":git-safe-bare"), "{q}");
}

/// A command of Claude's run outside the sandbox makes its turn count, as one inside does. Breaks if only sandboxed runs
/// count.
#[test]
fn a_turn_with_a_command_outside_the_sandbox_counts() {
    let (_s, dir) = home("safebare-outside");
    let mut ed = started(&dir);
    read(&mut ed);
    let task = crate::task::Task { name: "true".into(), cmd: "true".into(), cwd: dir.clone() };
    let who = crate::tools::For::Claude(crate::tools::Call::test(1));
    let guard = Some(crate::sandbox::Guard::Outside);
    assert!(ed.task_start_for(task, who, guard, Default::default()).is_ok());
    assert!(ed.workspace.claude.ran_in_turn, "the run marks its turn");
    turn_over(&mut ed);
    assert_eq!(cards(&ed), 1);
    crate::test_results::settle_until(&mut ed, |ed| {
        ed.workspace.task_run.as_ref().is_some_and(|r| r.state != crate::task::State::Running)
    });
}
