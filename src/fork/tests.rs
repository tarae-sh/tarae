//! Plan §4's tests (`.sb/session-fork-plan.md`): the tree as the source's, the conversation forked once and then its
//! own across `:restart`, the card holding through a turn, `n` making nothing, no answer carried, unsaved buffers saved
//! first — a stand-in claude only.

use std::path::{Path, PathBuf};

use super::*;
use crate::config::Config;

fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

fn has_branch(repo: &Path, b: &str) -> bool {
    let mut git = std::process::Command::new("git");
    git.arg("-C").arg(repo).args(["rev-parse", "--verify", "-q", &format!("refs/heads/{b}")]);
    git.output().unwrap().status.success()
}

/// A scratch folder of the test's own: a repository on `main` (`a.txt`, `del.txt`, `bin.dat`, `target/` ignored) and
/// a folder for sessions' checkouts beside it.
fn place(name: &str) -> (crate::scratch::Scratch, PathBuf, PathBuf) {
    let s = crate::scratch::Scratch::new(&format!("fork-{name}"));
    let dir = std::fs::canonicalize(&s).unwrap();
    let repo = dir.join("proj");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(repo.join("a.txt"), "one\n").unwrap();
    std::fs::write(repo.join("del.txt"), "going\n").unwrap();
    std::fs::write(repo.join("bin.dat"), [0u8, 1, 2, 255, 0, 7]).unwrap();
    std::fs::write(repo.join(".gitignore"), "target/\n").unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.email", "t@t"],
        &["config", "user.name", "t"],
        &["config", "commit.gpgsign", "false"],
        &["add", "-A"],
        &["commit", "-q", "-m", "base"],
    ] {
        git_in(&repo, args);
    }
    (s, repo, dir)
}

/// The stand-in claude: logs how each was started to `argv`, names its conversation `s-1` — `f-1` when it forks one or
/// resumes the fork's — and ends each turn at once, or once `go` is there when given.
fn claude(argv: &Path, go: Option<&Path>) -> String {
    let init = |id: &str| format!(r#"{{"type":"system","subtype":"init","session_id":"{id}"}}"#);
    let wait =
        go.map(|g| format!("while [ ! -f '{}' ]; do sleep 0.02; done; ", g.display())).unwrap_or_default();
    format!(
        "printf '%s\\n' \"$*\" >> '{argv}'; init='{s}'; case \"$*\" in *--fork-session*|*'--resume f-1'*) init='{f}';; esac; \
         while read -r line; do case \"$line\" in '{{\"message\"'*) printf '%s\\n' \"$init\"; {wait}\
         printf '%s\\n' '{{\"type\":\"result\",\"result\":\"done\"}}';; esac; done",
        argv = argv.display(),
        s = init("s-1"),
        f = init("f-1"),
    )
}

/// An editor in `repo` (on main), sessions' checkouts under `dir/wt`, its claude the stand-in.
fn editor(repo: &Path, dir: &Path, go: Option<&Path>) -> Editor {
    let mut ed = Editor::new(Config::default());
    (ed.workspace.root, ed.workspace.name) = (repo.to_path_buf(), "proj".into());
    ed.workspace.git_branch = Some("main".into());
    ed.config.workspace_worktrees = dir.join("wt").display().to_string();
    ed.config.llm.command = "sh".into();
    ed.config.llm.args = vec!["-c".into(), claude(&dir.join("argv.log"), go)];
    ed
}

fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key: crate::key::Key = if c == '<' {
            chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap()
        } else {
            crate::key::Key::plain(crate::key::Code::Char(c))
        };
        ed.handle_event(crate::event::Event::Key(key));
    }
}

fn said(ed: &Editor) -> String {
    ed.messages.iter().map(|m| m.2.as_str()).collect::<Vec<_>>().join(" | ")
}

#[track_caller]
fn settle_until(ed: &mut Editor, done: impl Fn(&Editor) -> bool) {
    crate::test_results::settle_saying(ed, done, said);
}

fn fork_card(ed: &Editor) -> Option<&Offer> {
    ed.offers.iter().find(|o| fork_of(o).is_some())
}

/// The fork card for the checkout tarae started in, up.
fn asked(ed: &mut Editor) {
    ask(ed, WorkspaceId::HOME, None);
    settle_until(ed, |ed| fork_card(ed).is_some() || said(ed).contains("fork:"));
    assert!(fork_card(ed).is_some(), "{}", said(ed));
}

/// A message in the session in front's chat, its turn over.
fn message(ed: &mut Editor, text: &str) {
    feed(ed, &format!("<space>l{text}<ret>"));
    settle_until(ed, |ed| {
        ed.workspace.claude.chat.as_ref().is_some_and(|c| c.conversation.is_some() && !c.busy())
    });
}

fn argv(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("argv.log")).unwrap_or_default()
}

/// Waits for a claude started so (`argv`'s last line holding `needle`).
#[track_caller]
fn started(ed: &mut Editor, dir: &Path, needle: &str) {
    settle_until(ed, |_| argv(dir).lines().last().is_some_and(|l| l.contains(needle)));
}

/// The fork's working tree is the source's — changed, new, deleted, a binary file, a staged one arriving unstaged, a
/// file under an ignored folder left out — at the source's HEAD, on a branch of its own; the source's files and index
/// as they were. Nothing is made before `y`.
#[test]
fn the_forks_tree_is_the_sources_and_the_source_is_untouched() {
    let (_s, repo, dir) = place("tree");
    std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(repo.join("new.txt"), "fresh\n").unwrap();
    std::fs::create_dir_all(repo.join("sub")).unwrap();
    std::fs::write(repo.join("sub/deep.txt"), "deep\n").unwrap();
    std::fs::remove_file(repo.join("del.txt")).unwrap();
    std::fs::write(repo.join("bin.dat"), [255u8, 0, 0, 9, 13, 10]).unwrap();
    std::fs::create_dir_all(repo.join("target")).unwrap();
    std::fs::write(repo.join("target/out.o"), "built").unwrap();
    std::fs::write(repo.join("staged.txt"), "staged\n").unwrap();
    git_in(&repo, &["add", "staged.txt"]);
    let (status, index) = (git_in(&repo, &["status", "--porcelain"]), git_in(&repo, &["ls-files", "-s"]));
    let head = git_in(&repo, &["rev-parse", "HEAD"]);
    let mut ed = editor(&repo, &dir, None);
    feed(&mut ed, ":session-fork<ret>");
    settle_until(&mut ed, |ed| fork_card(ed).is_some() || said(ed).contains("fork:"));
    let o = fork_card(&ed).unwrap_or_else(|| panic!("the card: {}", said(&ed)));
    assert_eq!(o.title, "Fork main as main-2");
    assert!(
        o.question
            .starts_with(&format!("Branch main-2 at {}, main's HEAD — it brings into main.", &head[..7]))
    );
    assert!(
        o.question.contains(
            "With it: 6 files not committed (3 new, 1 deleted) — 1 staged in main arrives unstaged."
        ),
        "{}",
        o.question
    );
    assert!(o.question.contains("Its conversation: none yet — a new one."), "{}", o.question);
    assert!(o.question.contains("None of main's answers come with it"), "{}", o.question);
    assert!(!has_branch(&repo, "main-2"), "nothing made before y");
    ed.offer_accept();
    settle_until(&mut ed, |ed| !ed.workspaces.is_empty() || said(ed).contains("Couldn't"));
    let fork = ed.workspace.root.clone();
    assert!(fork.starts_with(dir.join("wt")) && fork.ends_with("main-2"), "{}", fork.display());
    assert_eq!((ed.workspace.name.as_str(), ed.workspace.base.as_deref()), ("main-2", Some("main")));
    assert_eq!(git_in(&fork, &["symbolic-ref", "--short", "HEAD"]), "main-2");
    assert_eq!(git_in(&fork, &["rev-parse", "HEAD"]), head, "at the source's HEAD");
    let read = |root: &Path, rel: &str| std::fs::read(root.join(rel)).ok();
    for rel in ["a.txt", "new.txt", "sub/deep.txt", "bin.dat", "staged.txt", ".gitignore"] {
        assert_eq!(read(&fork, rel), read(&repo, rel), "{rel}");
    }
    assert!(!fork.join("del.txt").exists(), "its deletion carried");
    assert!(!fork.join("target").exists(), "an ignored folder isn't carried");
    // Nothing staged there: the changes in no commit, new files untracked
    assert_eq!(git_in(&fork, &["diff", "--cached", "--name-only"]), "");
    let there = git_in(&fork, &["status", "--porcelain"]);
    for row in [" M a.txt", " D del.txt", " M bin.dat", "?? new.txt", "?? staged.txt", "?? sub/"] {
        assert!(there.lines().any(|l| l == row), "{row:?} in {there:?}");
    }
    // The source as it was: its files, its index, its HEAD
    assert_eq!(git_in(&repo, &["status", "--porcelain"]), status);
    assert_eq!(git_in(&repo, &["ls-files", "-s"]), index);
    assert_eq!(git_in(&repo, &["rev-parse", "HEAD"]), head);
    assert!(said(&ed).contains("Session main-2 — forked from main at "), "{}", said(&ed));
    assert!(said(&ed).contains("1 staged in main arrives unstaged"), "{}", said(&ed));
    // (no conversation to fork: its claude starts fresh, its row says where it came from)
    started(&mut ed, &dir, "--strict-mcp-config");
    assert!(!argv(&dir).contains("--resume"), "{}", argv(&dir));
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert!(c.msgs.iter().any(|m| m.text == "forked from main — its files; no conversation yet"));
    assert!(
        ed.workspace
            .claude
            .checkpoints
            .tell
            .iter()
            .any(|t| t.starts_with("This session was forked from session main:"))
    );
}

/// The fork's claude starts with `--resume <source's> --fork-session`, once; until claude names its own conversation
/// the fork is kept so, and a `:restart` forks it still; once named it resumes its own, never forking again. The source
/// keeps its own.
#[test]
fn the_conversation_is_forked_once_then_its_own_across_a_restart() {
    let (_s, repo, dir) = place("conv");
    let mut ed = editor(&repo, &dir, None);
    message(&mut ed, "hello");
    assert_eq!(ed.workspace.claude.chat.as_ref().unwrap().conversation.as_deref(), Some("s-1"));
    asked(&mut ed);
    let q = &fork_card(&ed).unwrap().question;
    assert!(
        q.contains("Its conversation: main's, at turn 1 — its first message writes it all again, "),
        "{q}"
    );
    ed.offer_accept();
    settle_until(&mut ed, |ed| !ed.workspaces.is_empty());
    started(&mut ed, &dir, "--resume s-1 --fork-session");
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert!(c.msgs.iter().any(|m| m.text == "hello"), "the source's rows");
    assert!(c.msgs.iter().any(|m| m.text == "forked from main at its turn 1 — the two go on apart"));
    // Kept as a fork not begun: a restart now forks it still
    let state = crate::worktree::snapshot(&ed);
    let kept = &state["sessions"][1]["fork"];
    assert_eq!(
        (kept["conversation"].as_str(), kept["name"].as_str()),
        (Some("s-1"), Some("main")),
        "{state}"
    );
    let mut back = editor(&repo, &dir, None);
    crate::worktree::restore(&mut back, &state);
    // (in front, as it was)
    let all = || std::iter::once(&back.workspace).chain(&back.workspaces);
    assert_eq!(all().filter(|l| l.claude.fork.is_some()).count(), 1, "the fork back, still one");
    let id = all().find(|l| l.claude.fork.is_some()).unwrap().id;
    back.workspace_switch(id);
    let lines = argv(&dir).lines().count();
    let one_more = |ed: &mut Editor| settle_until(ed, |_| argv(&dir).lines().count() > lines);
    crate::chat::restore(
        &mut back,
        crate::chat::Restored {
            conversation: None,
            input: String::new(),
            cursor: 0,
            focused: false,
            scroll: 0,
            follow: false,
            at_end: None,
            lazy: false,
        },
    );
    one_more(&mut back);
    started(&mut back, &dir, "--resume s-1 --fork-session");
    assert_eq!(argv(&dir).lines().count(), lines + 1, "one claude: {}", argv(&dir));
    assert!(
        back.workspace
            .claude
            .checkpoints
            .tell
            .iter()
            .any(|t| t.contains("forked from session main at its turn 1"))
    );
    drop(back);
    // Its first message: claude names the fork's own conversation — no fork kept any more; started again, it resumes that
    let told = ed.workspace.claude.checkpoints.tell.clone();
    assert!(
        told.iter().any(|t| t.contains("forked from session main at its turn 1: it's now main-2")),
        "{told:?}"
    );
    message(&mut ed, "again");
    assert_eq!(ed.workspace.claude.chat.as_ref().unwrap().conversation.as_deref(), Some("f-1"));
    assert!(ed.workspace.claude.fork.is_none() && ed.workspace.claude.checkpoints.tell.is_empty());
    assert!(crate::worktree::snapshot(&ed)["sessions"][1]["fork"].is_null());
    let lines = argv(&dir).lines().count();
    crate::chat::restart_process(&mut ed);
    settle_until(&mut ed, |_| argv(&dir).lines().count() > lines);
    started(&mut ed, &dir, "--resume f-1");
    let last = argv(&dir).lines().last().unwrap_or_default().to_string();
    assert!(!last.contains("--fork-session"), "{last}");
    assert_eq!(
        argv(&dir).matches("--fork-session").count(),
        2,
        "the fork's first and the restart's: {}",
        argv(&dir)
    );
    // The source keeps its own
    let home = ed.workspaces.iter().find(|l| l.id == WorkspaceId::HOME).unwrap();
    assert_eq!(home.claude.chat.as_ref().unwrap().conversation.as_deref(), Some("s-1"));
}

/// Asked while the source works (D2): the card says it waits; `y` then is kept, the card saying so, nothing made — and
/// it forks once the turn ends, once. `n` makes nothing.
#[test]
fn mid_turn_the_card_holds_then_forks_once_and_n_makes_nothing() {
    let (_s, repo, dir) = place("turn");
    let go = dir.join("go");
    let mut ed = editor(&repo, &dir, Some(&go));
    feed(&mut ed, "<space>lhello<ret>");
    settle_until(&mut ed, |ed| ed.workspace.claude.chat.as_ref().is_some_and(|c| c.conversation.is_some()));
    asked(&mut ed);
    let q = fork_card(&ed).unwrap().question.clone();
    assert!(q.ends_with("\n\nNot yet: y forks once main's turn ends."), "{q}");
    ed.offer_accept();
    let o = fork_card(&ed).expect("the card stays");
    assert!(o.question.ends_with("\n\ny given — it forks once main's turn ends."), "{}", o.question);
    assert!(matches!(o.state, OfferState::Asking));
    assert_eq!(o.answers(false).iter().map(|a| a.0).collect::<String>(), "n");
    for _ in 0..5 {
        ed.handle_event(crate::event::Event::Job(Box::new(|_| {})));
    }
    assert!(!has_branch(&repo, "main-2") && ed.workspaces.is_empty(), "held while the turn runs");
    std::fs::write(&go, "").unwrap();
    settle_until(&mut ed, |ed| !ed.workspaces.is_empty());
    assert!(fork_card(&ed).is_none());
    assert!(has_branch(&repo, "main-2") && !has_branch(&repo, "main-3"), "forked once");
    // `n`: nothing made
    asked(&mut ed);
    assert_eq!(fork_card(&ed).unwrap().title, "Fork main as main-3");
    ed.offer_dismiss();
    assert!(fork_card(&ed).is_none());
    for _ in 0..5 {
        ed.handle_event(crate::event::Event::Job(Box::new(|_| {})));
    }
    assert!(
        !has_branch(&repo, "main-3")
            && !dir.join("wt").read_dir().unwrap().flatten().any(|e| { e.path().join("main-3").exists() })
    );
    assert!(said(&ed).contains("No fork of main — nothing was made"), "{}", said(&ed));
    assert_eq!(ed.workspaces.len(), 1);
}

/// Sent to the source between its fork's `y` and its tree written (D2, nothing holding): the message waits as for a
/// base move, its turn starting only once the fork is made — and what that turn writes isn't in the fork's tree.
#[test]
fn a_message_sent_while_it_forks_waits_for_the_tree() {
    let (_s, repo, dir) = place("held");
    let mut ed = editor(&repo, &dir, None);
    // (the stand-in counts its messages; the one saying WRITE rewrites a.txt as its turn begins)
    let log = dir.join("msgs.log");
    ed.config.llm.args = vec![
        "-c".into(),
        format!(
            "while read -r line; do case \"$line\" in '{{\"message\"'*) printf 'm\\n' >> '{log}'; \
             case \"$line\" in *WRITE*) printf 'claude\\n' > '{a}';; esac; printf '%s\\n' '{init}'; \
             printf '%s\\n' '{{\"type\":\"result\",\"result\":\"done\"}}';; esac; done",
            log = log.display(),
            a = repo.join("a.txt").display(),
            init = r#"{"type":"system","subtype":"init","session_id":"s-1"}"#,
        ),
    ];
    let sent = || std::fs::read_to_string(&log).unwrap_or_default().lines().count();
    message(&mut ed, "hello");
    assert_eq!(sent(), 1);
    asked(&mut ed);
    ed.offer_accept();
    assert!(matches!(fork_card(&ed).map(|o| &o.state), Some(OfferState::Installing(_))), "being made");
    feed(&mut ed, "<space>lWRITE now<ret>");
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert_eq!(c.queued.as_deref(), Some("WRITE now"), "held while the fork is made");
    assert!(!c.busy() && sent() == 1, "no turn begun");
    settle_until(&mut ed, |ed| !ed.workspaces.is_empty() || said(ed).contains("Couldn't"));
    assert_eq!(
        std::fs::read_to_string(ed.workspace.root.join("a.txt")).unwrap(),
        "one\n",
        "the pre-send tree"
    );
    // (let go as the fork landed, in the same pass: sent now)
    let home = ed.workspaces.iter().find(|l| l.id == WorkspaceId::HOME).unwrap();
    assert!(home.claude.chat.as_ref().unwrap().queued.is_none(), "sent once the fork was made");
    settle_until(&mut ed, |_| sent() == 2);
    settle_until(&mut ed, |_| std::fs::read_to_string(repo.join("a.txt")).is_ok_and(|t| t == "claude\n"));
    assert_eq!(std::fs::read_to_string(ed.workspace.root.join("a.txt")).unwrap(), "one\n");
}

/// None of the source's answers come with it (D4): a read, a write grant and a host given there are asked again in the
/// fork — its session and conversation hold none.
#[test]
fn the_sources_answers_dont_come_with_the_fork() {
    let (_s, repo, dir) = place("grants");
    let mut ed = editor(&repo, &dir, None);
    message(&mut ed, "hello");
    let read = crate::allow::Grant::Read(dir.join("elsewhere"));
    ed.workspace.claude.allowed.add(&read);
    ed.workspace.claude.hosts.push(("example.test".into(), 443));
    ed.workspace.claude.chat.as_mut().unwrap().allowed.add(&crate::allow::Grant::Write(dir.join("out")));
    asked(&mut ed);
    ed.offer_accept();
    settle_until(&mut ed, |ed| !ed.workspaces.is_empty());
    let fork = &ed.workspace.claude;
    assert!(fork.allowed.is_empty() && fork.hosts.is_empty(), "{:?} {:?}", fork.allowed, fork.hosts);
    assert!(fork.chat.as_ref().unwrap().allowed.is_empty());
    assert!(!fork.allowed.covers(&read));
    assert!(crate::worktree::Kept::of(&ed.workspace).allowed.is_empty(), "nothing kept for it either");
    let home = ed.workspaces.iter().find(|l| l.id == WorkspaceId::HOME).unwrap();
    assert!(home.claude.allowed.covers(&read), "the source keeps its own");
}

/// The source's unsaved buffers (D1): named on the card; `y` saves them first, and the fork has what was in sight.
#[test]
fn unsaved_buffers_are_saved_first() {
    let (_s, repo, dir) = place("unsaved");
    let mut ed = editor(&repo, &dir, None);
    ed.open_now(&repo.join("a.txt")).unwrap();
    feed(&mut ed, "iEDIT <esc>");
    assert!(ed.doc().is_modified());
    asked(&mut ed);
    let q = fork_card(&ed).unwrap().question.clone();
    assert!(q.contains("\nUnsaved in main: a.txt — y saves them first."), "{q}");
    assert!(q.contains("With it: nothing uncommitted"), "what's on disk: {q}");
    ed.offer_accept();
    settle_until(&mut ed, |ed| !ed.workspaces.is_empty() || said(ed).contains("wasn't saved"));
    assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), "EDIT one\n", "saved in the source");
    assert_eq!(
        std::fs::read_to_string(ed.workspace.root.join("a.txt")).unwrap(),
        "EDIT one\n",
        "what was in sight"
    );
    assert!(said(&ed).contains("1 file not committed came with it"), "{}", said(&ed));
}

/// The card's words: new files past the cap named in warning (and told to the fork's Claude), the first message's cost
/// as the chat's row says money, a source with no conversation, a save that failed.
/// The making of `fix-2` from `fix`, on the plan, nothing asked.
fn making() -> Making {
    Making {
        top: PathBuf::from("/r/proj"),
        branch: "fix-2".into(),
        base: "main".into(),
        base_sha: "a1b2c3d4e5".into(),
        path: PathBuf::from("/w/fix-2"),
        files: (0, true),
        big: Vec::new(),
        builds: None,
        build_own: None,
        warm: None,
        copies: Vec::new(),
        others: Vec::new(),
        billing: crate::billing::Billing::Subscription,
        billing_ask: None,
        billing_note: None,
    }
}

#[test]
fn the_card_says_what_isnt_carried_and_what_the_first_message_costs() {
    let making = making();
    let mut f = Forking {
        source: WorkspaceId::HOME,
        from: ForkFrom {
            root: "/r/fix".into(),
            name: "fix".into(),
            conversation: Some("s-1".into()),
            turn: 14,
            left_out: 5210,
        },
        making,
        files: Files {
            changed: 2,
            new: 1,
            deleted: 0,
            staged: 2,
            left_out: 5210,
            left_top: vec![("data/".into(), 4000), ("out/".into(), 1210)],
        },
        tokens: None,
        model: "m".into(),
        unsaved: Vec::new(),
        holds: None,
        go: false,
        saving: false,
        failed: None,
    };
    let o = card(&f, false);
    let lines: Vec<&str> = o.question.split('\n').collect();
    assert_eq!(lines[0], "Branch fix-2 at a1b2c3d, fix's HEAD — it brings into main, as fix.");
    let left = lines.iter().position(|l| l.starts_with("5210 new files of fix's aren't carried")).unwrap();
    assert_eq!(
        lines[left],
        "5210 new files of fix's aren't carried (past 5000 at once): data/ 4000, out/ 1210."
    );
    assert_eq!(o.warnings, vec![left..left + 1], "in warning");
    assert!(o.question.contains("With it: 3 files not committed (1 new) — 2 staged in fix arrive unstaged."));
    assert!(
        o.question
            .contains("at turn 14 — its first message writes it all again, its size isn't reported yet.")
    );
    assert!(told(&f.from, "fix-2").ends_with(" 5210 new files of fix's weren't carried."));
    // What the write costs: on the key in dollars, else what the API would charge, said so; tokens where no price
    let prices = [crate::switchcost::Price {
        model: "m".into(),
        up_to: None,
        write_5m: 6.25,
        write_1h: 7.5,
        effort: crate::switchcost::Effort::Unknown,
    }];
    use crate::billing::Billing::{Api, Subscription};
    let said = |b, model| crate::switchcost::fork_said(Some(112_000), b, model, &prices);
    assert_eq!(said(Api, "m").as_deref(), Some("≈ $0.84 (112k tokens)"));
    assert_eq!(said(Subscription, "m").as_deref(), Some("≈ API $0.84 (112k tokens)"));
    assert_eq!(said(Subscription, "other").as_deref(), Some("≈ 112k tokens, toward your plan's limits"));
    assert_eq!(crate::switchcost::fork_said(None, Api, "m", &prices), None);
    // No conversation yet: the files alone
    f.from.conversation = None;
    assert!(card(&f, false).question.contains("\nIts conversation: none yet — a new one.\n"));
    assert!(told(&f.from, "fix-2").starts_with("This session was forked from session fix: it's now fix-2"));
    // A save that failed: said in warning, its `y` back
    (f.go, f.failed) = (false, Some("a.txt wasn't saved".into()));
    let o = card(&f, false);
    let last = o.question.split('\n').count() - 1;
    assert!(o.question.ends_with("\n\na.txt wasn't saved — y tries again."), "{}", o.question);
    assert_eq!(o.warning, Some(last..last + 1));
    assert_eq!(o.answers(false).iter().map(|a| a.0).collect::<String>(), "yn");
}

/// Who pays is the source's (Q4), and the card says only what the fork runs on: with `a` on the limit card keeping new
/// sessions on your key, a fork of a session on the plan says nothing of the key; one of a session on the key says
/// until when.
#[test]
fn the_cards_billing_words_are_what_the_fork_runs_on() {
    use crate::billing::Billing::{Api, Subscription};
    for source in [Subscription, Api] {
        let mut ed = Editor::new(Config::default());
        ed.api_ready = Some(Ok(()));
        ed.billing_kept.api_until_ms = Some(crate::usage::now_ms() + 3 * 86_400_000);
        assert_eq!(ed.new_session_ask().0, Api, "a new session would run on the key");
        ed.workspace.claude.billing = source;
        super::asked(&mut ed, WorkspaceId::HOME, making(), Files::default());
        let o = fork_card(&ed).expect("the card");
        let m = o.making().unwrap();
        assert_eq!(m.billing, source, "preset to the source's");
        let on_key = o.question.contains("Its Claude runs on your API key until ");
        assert_eq!(on_key, source == Api, "{source:?}: {}", o.question);
        assert!(!o.question.contains("on your plan") && !o.bills(), "nothing asked: {}", o.question);
    }
}
