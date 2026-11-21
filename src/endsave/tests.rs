use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use super::*;
use crate::config::Config;
use crate::event::WorkspaceId;
use crate::offer::What;

fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = crate::gitrepo::test_git(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repository on `main` (`a.txt`, `target/` ignored) in a folder of the test's own, sessions' checkouts beside it.
fn place(name: &str) -> (crate::scratch::Scratch, PathBuf, PathBuf) {
    let s = crate::scratch::Scratch::new(&format!("endsave-{name}"));
    let dir = s.to_path_buf();
    let repo = dir.join("proj");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(repo.join("a.txt"), "one\n").unwrap();
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

fn editor(repo: &Path, dir: &Path) -> Editor {
    let mut ed = Editor::new(Config::default());
    (ed.workspace.root, ed.workspace.name) = (repo.to_path_buf(), "proj".into());
    ed.workspace.git_branch = Some("main".into());
    ed.config.workspace_worktrees = dir.join("wt").display().to_string();
    ed.config.llm.command = "sh".into();
    ed.config.llm.args = vec!["-c".into(), "cat > /dev/null".into()];
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

fn says(ed: &Editor, needle: &str) -> bool {
    ed.messages.iter().any(|m| m.2.contains(needle))
}

#[track_caller]
fn settle_until(ed: &mut Editor, done: impl Fn(&Editor) -> bool) {
    crate::test_results::settle_saying(ed, done, said);
}

/// The card in front drawn and focused, as a person sees it before answering: its buttons.
fn card_in_sight(ed: &mut Editor) -> Vec<char> {
    let mut out = Vec::new();
    crate::term::render(ed, &mut out, 112, 30).unwrap();
    ed.card_focus_front();
    crate::term::render(ed, &mut out, 112, 30).unwrap();
    ed.screen.offer_buttons.iter().map(|b| b.3).collect()
}

/// `:session-new name`, its card answered `y` — the session in front, its chat's input without the keys.
fn new_session(ed: &mut Editor, name: &str) -> PathBuf {
    ed.workspace_switch(WorkspaceId::HOME);
    feed(ed, &format!(":session-new {name}<ret>"));
    settle_until(ed, |ed| !ed.offers.is_empty() || says(ed, "new session:"));
    let sessions = ed.workspaces.len();
    ed.offer_accept();
    settle_until(ed, |ed| ed.workspaces.len() > sessions);
    ed.workspace.leave(crate::workspace::Here::Chat);
    ed.workspace.root.clone()
}

/// The repository's saves' refs.
fn refs(repo: &Path) -> Vec<String> {
    let out = git_in(repo, &["for-each-ref", "--format=%(refname)", REFS]);
    out.lines().map(str::to_string).collect()
}

/// `git status --porcelain` rows, sorted.
fn status(dir: &Path) -> Vec<String> {
    // (untrimmed: a row's first column is a space where nothing is staged)
    let mut git = crate::gitrepo::test_git(dir);
    git.args(["status", "--porcelain", "-uall"]);
    let out = String::from_utf8(git.output().unwrap().stdout).unwrap();
    let mut rows: Vec<String> = out.lines().map(str::to_string).collect();
    rows.sort();
    rows
}

/// SM2 tests 1 and 5: a workspace with a change, a deletion, a new file, an ignored one and a commit only its branch has
/// ends by `D` (its card saying the way back) — saved, the success saying where; `:workspace-restore` lists it (name,
/// branch, counts, age), Enter asks its card, `y` brings it back: the same checkout path and branch at the old tip, the
/// same unstaged changes, the ignored file not there, the save gone.
#[test]
fn an_ended_workspace_comes_back_as_it_was() {
    let (_s, repo, dir) = place("back");
    let mut ed = editor(&repo, &dir);
    let wt = new_session(&mut ed, "fix");
    std::fs::write(wt.join("b.txt"), "bee\n").unwrap();
    std::fs::write(wt.join("c.txt"), "sea\n").unwrap();
    git_in(&wt, &["add", "-A"]);
    git_in(&wt, &["commit", "-qm", "on fix only"]);
    let tip = git_in(&wt, &["rev-parse", "HEAD"]);
    std::fs::write(wt.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::remove_file(wt.join("c.txt")).unwrap();
    std::fs::write(wt.join("notes.md"), "todo\n").unwrap();
    std::fs::create_dir_all(wt.join("target")).unwrap();
    std::fs::write(wt.join("target/out.bin"), "built\n").unwrap();
    let before = status(&wt);
    assert_eq!(before, [" D c.txt", " M a.txt", "?? notes.md"]);
    feed(&mut ed, ":workspace-end<ret>");
    settle_until(&mut ed, |ed| !ed.offers.is_empty() || says(ed, "end fix:"));
    let q = ed.offers[0].question.clone();
    assert!(q.contains("Removing takes them out of the checkout."), "{q}");
    assert!(q.contains("Removing both deletes fix too, and 1 commit on no other branch with it."), "{q}");
    assert!(
        q.ends_with("\n\nWhat goes is saved for 30 days — :workspace-restore fix brings it back."),
        "{q}"
    );
    assert_eq!(card_in_sight(&mut ed), ['c', 'R', 'D', 'n']);
    feed(&mut ed, "D");
    settle_until(&mut ed, |ed| says(ed, "Ended fix") || says(ed, "end fix:"));
    assert!(!wt.exists(), "{}", said(&ed));
    let saved = refs(&repo);
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert!(saved[0].starts_with("refs/tarae/ended/fix-"), "{saved:?}");
    assert!(says(&ed, &format!(" · saved at {}", saved[0])), "{}", said(&ed));
    assert_eq!(
        git_in(&repo, &["rev-parse", &format!("{}^", saved[0])]),
        tip,
        "the deleted branch's tip, kept"
    );
    assert!(git_in(&repo, &["branch", "--list", "fix"]).is_empty(), "its branch deleted");
    // The picker: name, branch, counts, its age
    feed(&mut ed, ":workspace-restore<ret>");
    settle_until(&mut ed, |ed| ed.picker.as_ref().is_some_and(|p| !p.loading));
    let p = ed.picker.as_ref().unwrap();
    assert_eq!(p.kind, "workspace-restore");
    let item = &p.items()[0];
    assert_eq!(item.label, "fix  3 files · 1 commit", "its branch is its name: not said");
    assert!(item.hint.ends_with(" ago") || item.hint == "just now", "{}", item.hint);
    feed(&mut ed, "<ret>");
    settle_until(&mut ed, |ed| !ed.offers.is_empty() || says(ed, "restore:"));
    let o = &ed.offers[0];
    assert_eq!(o.title, "Bring back fix?");
    for part in [
        ": 1 changed, 1 new, 1 deleted.\n",
        &format!("\nBranch fix at {} — 1 commit not in main.\n", &tip[..7])[..],
        "Its checkout, fix/, goes in\n",
        "Its conversation isn't brought back — its Claude starts fresh.",
    ] {
        assert!(o.question.contains(part), "{part:?} in {}", o.question);
    }
    assert_eq!(card_in_sight(&mut ed), ['y', 'n']);
    feed(&mut ed, "y");
    settle_until(&mut ed, |ed| says(ed, "brought back") || says(ed, "Couldn't bring back"));
    assert_eq!(ed.workspace.root, std::fs::canonicalize(&wt).unwrap(), "its session in front: {}", said(&ed));
    assert_eq!(git_in(&wt, &["symbolic-ref", "--short", "HEAD"]), "fix");
    assert_eq!(git_in(&wt, &["rev-parse", "HEAD"]), tip);
    assert_eq!(status(&wt), before, "the same changes, unstaged");
    assert_eq!(std::fs::read_to_string(wt.join("a.txt")).unwrap(), "one\ntwo\n");
    assert!(!wt.join("target").exists(), "ignored files aren't saved");
    assert!(refs(&repo).is_empty(), "its save gone once it's back");
    assert!(says(&ed, "Session fix — brought back as it was "), "{}", said(&ed));
    drop(ed);
    git_in(&repo, &["worktree", "remove", "--force", wt.to_str().unwrap()]);
}

/// SM2 test 2, and the branch reused (§6.6): a quiet end saves nothing; `y` on a card with commits only (its branch
/// stays) saves nothing either, its card without the line; `R` with a change saves, and its restore reuses the branch
/// left at that tip, the card saying so.
#[test]
fn only_an_end_where_something_goes_is_saved() {
    let (_s, repo, dir) = place("quiet");
    let mut ed = editor(&repo, &dir);
    new_session(&mut ed, "done");
    feed(&mut ed, ":workspace-end<ret>");
    settle_until(&mut ed, |ed| says(ed, "Ended done") || says(ed, "end done:"));
    assert!(refs(&repo).is_empty() && !says(&ed, "saved at"), "{}", said(&ed));
    let wt = new_session(&mut ed, "kept");
    git_in(&wt, &["commit", "-q", "--allow-empty", "-m", "on kept"]);
    feed(&mut ed, ":workspace-end<ret>");
    settle_until(&mut ed, |ed| !ed.offers.is_empty() || says(ed, "end kept:"));
    assert!(ed.offers[0].question.contains(":workspace-restore kept"), "D would save its commit");
    assert_eq!(card_in_sight(&mut ed), ['y', 'D', 'n']);
    feed(&mut ed, "y");
    settle_until(&mut ed, |ed| says(ed, "Ended kept") || says(ed, "end kept:"));
    assert!(refs(&repo).is_empty(), "its branch stays: nothing went");
    // R: the change saved, the branch left where it was — reused
    let wt = new_session(&mut ed, "wip");
    std::fs::write(wt.join("a.txt"), "changed\n").unwrap();
    let tip = git_in(&wt, &["rev-parse", "HEAD"]);
    feed(&mut ed, ":workspace-end<ret>");
    settle_until(&mut ed, |ed| !ed.offers.is_empty() || says(ed, "end wip:"));
    card_in_sight(&mut ed);
    feed(&mut ed, "R");
    settle_until(&mut ed, |ed| says(ed, "Ended wip") || says(ed, "end wip:"));
    assert_eq!(refs(&repo).len(), 1, "{}", said(&ed));
    feed(&mut ed, ":workspace-restore wip<ret>");
    settle_until(&mut ed, |ed| !ed.offers.is_empty() || says(ed, "restore:"));
    let q = &ed.offers[0].question;
    assert!(q.contains(&format!("Its branch wip, still at {}, in main.", &tip[..7])), "{q}");
    ed.offer_accept();
    settle_until(&mut ed, |ed| says(ed, "brought back") || says(ed, "Couldn't bring back"));
    assert_eq!(ed.workspace.root, std::fs::canonicalize(&wt).unwrap(), "{}", said(&ed));
    assert_eq!(status(&wt), [" M a.txt"]);
    drop(ed);
    git_in(&repo, &["worktree", "remove", "--force", wt.to_str().unwrap()]);
}

/// `e` for the checkout `wt` as the checks see it now.
fn ending_of(repo: &Path, wt: &Path, name: &str) -> Ending {
    crate::worktree::ending(name.into(), wt, Some("main".into()), repo, Vec::new()).unwrap()
}

/// A checkout `name` made by git beside the repository, on a branch of its name with a change in it.
fn checkout(repo: &Path, dir: &Path, name: &str) -> PathBuf {
    let wt = dir.join("wt").join(name);
    git_in(repo, &["worktree", "add", "-q", "-b", name, wt.to_str().unwrap(), "main"]);
    std::fs::write(wt.join("a.txt"), format!("{name}\n")).unwrap();
    std::fs::canonicalize(&wt).unwrap()
}

fn fails(_: &Path) -> Result<(String, crate::snapshot::Tree), String> {
    Err("a stand-in that fails".into())
}

/// SM2 test 3: a snapshot that fails removes nothing — the checkout and its branch stay, no ref, said why.
#[test]
fn a_save_that_fails_removes_nothing() {
    let (_s, repo, dir) = place("fails");
    let wt = checkout(&repo, &dir, "gone");
    let e = ending_of(&repo, &wt, "gone");
    let r = crate::worktree::remove(&e, true, None, fails, crate::clock::system());
    let Err(why) = r else { panic!("removed: a save that failed") };
    assert_eq!(why, "its files couldn't be saved (a stand-in that fails) — nothing removed");
    assert!(wt.is_dir() && !git_in(&repo, &["branch", "--list", "gone"]).is_empty());
    assert!(refs(&repo).is_empty());
    git_in(&repo, &["worktree", "remove", "--force", wt.to_str().unwrap()]);
}

/// A removal git was asked for that fails (a file made since the checks: git won't remove the checkout unforced) keeps
/// its save, said — a `--force` that stopped half way leaves a checkout that's no way back.
#[test]
fn a_removal_that_fails_once_git_ran_keeps_its_save() {
    let (_s, repo, dir) = place("unremoved");
    let wt = dir.join("wt/u");
    git_in(&repo, &["worktree", "add", "-q", "-b", "u", wt.to_str().unwrap(), "main"]);
    let wt = std::fs::canonicalize(&wt).unwrap();
    git_in(&wt, &["commit", "-q", "--allow-empty", "-m", "only on u"]);
    let e = ending_of(&repo, &wt, "u");
    assert!(e.goes(true) && e.changed.is_empty());
    std::fs::write(wt.join("since.txt"), "made since\n").unwrap();
    let r = crate::worktree::remove(&e, true, None, crate::fork::take, crate::clock::system());
    let Err(why) = r else { panic!("removed with a file made since") };
    let saved = refs(&repo);
    assert_eq!(saved.len(), 1, "its save kept: {why}");
    assert!(why.ends_with(&format!(" — saved at {} anyway", saved[0])), "{why}");
    assert!(wt.is_dir());
    git_in(&repo, &["worktree", "remove", "--force", wt.to_str().unwrap()]);
}

/// The card in front's failure, as `offer_finished` left it.
fn card_failed(ed: &Editor) -> Option<String> {
    ed.offers.iter().find_map(|o| match &o.state {
        crate::offer::OfferState::Failed(why) => Some(why.clone()),
        _ => None,
    })
}

/// `:workspace-end` of the session in front, `R` on its card (work in no commit): ended, saved.
fn end_by_r(ed: &mut Editor, name: &str) {
    feed(ed, ":workspace-end<ret>");
    settle_until(ed, |ed| !ed.offers.is_empty() || says(ed, &format!("end {name}:")));
    card_in_sight(ed);
    feed(ed, "R");
    settle_until(ed, |ed| says(ed, &format!("Ended {name}")) || says(ed, &format!("end {name}:")));
    assert!(says(ed, " · saved at "), "{}", said(ed));
}

/// A save whose base is gone by its restore comes back on the start checkout's branch — the card says so — and the
/// session brought back can end (its base there to compare with).
#[test]
fn a_restore_whose_base_is_gone_takes_the_start_branch_and_can_end() {
    let (_s, repo, dir) = place("based");
    git_in(&repo, &["branch", "feature"]);
    let mut ed = editor(&repo, &dir);
    let wt = new_session(&mut ed, "fix feature");
    assert_eq!(ed.workspace.base.as_deref(), Some("feature"));
    std::fs::write(wt.join("a.txt"), "changed\n").unwrap();
    end_by_r(&mut ed, "fix");
    git_in(&repo, &["branch", "-D", "feature"]);
    feed(&mut ed, ":workspace-restore fix<ret>");
    settle_until(&mut ed, |ed| !ed.offers.is_empty() || says(ed, "restore:"));
    let q = &ed.offers[0].question;
    assert!(q.contains(", in main.\nIts base feature is gone — main instead.\n"), "{q}");
    ed.offer_accept();
    settle_until(&mut ed, |ed| says(ed, "brought back") || card_failed(ed).is_some());
    assert_eq!(ed.workspace.root, std::fs::canonicalize(&wt).unwrap(), "{}", said(&ed));
    assert_eq!(ed.workspace.base.as_deref(), Some("main"));
    ed.workspace.leave(crate::workspace::Here::Chat);
    feed(&mut ed, ":workspace-end<ret>");
    let ends = |ed: &Editor| ed.offers.iter().any(|o| matches!(o.what, What::WorkspaceEnd(_)));
    settle_until(&mut ed, |ed| ends(ed) || says(ed, "end fix:"));
    assert!(ends(&ed), "its end asked: {}", said(&ed));
    drop(ed);
    git_in(&repo, &["worktree", "remove", "--force", wt.to_str().unwrap()]);
}

/// A reused branch that moved between the card and `y` makes nothing — its tree laid on the new tip would stage the
/// undoing of those commits: the checkout taken back, the branch where it moved to, the save still there.
#[test]
fn a_reused_branch_that_moved_since_the_card_makes_nothing() {
    let (_s, repo, dir) = place("moved");
    let mut ed = editor(&repo, &dir);
    let wt = new_session(&mut ed, "wip");
    std::fs::write(wt.join("a.txt"), "changed\n").unwrap();
    end_by_r(&mut ed, "wip");
    feed(&mut ed, ":workspace-restore wip<ret>");
    settle_until(&mut ed, |ed| !ed.offers.is_empty() || says(ed, "restore:"));
    assert!(ed.offers[0].question.contains("Its branch wip, still at "), "{}", ed.offers[0].question);
    let tree = git_in(&repo, &["rev-parse", "wip^{tree}"]);
    let moved = git_in(&repo, &["commit-tree", &tree, "-p", "wip", "-m", "moved meanwhile"]);
    git_in(&repo, &["update-ref", "refs/heads/wip", &moved]);
    ed.offer_accept();
    settle_until(&mut ed, |ed| says(ed, "brought back") || card_failed(ed).is_some());
    assert_eq!(card_failed(&ed).as_deref(), Some("its branch moved since the card was shown — nothing made"));
    assert!(!wt.exists(), "its checkout taken back");
    assert_eq!(git_in(&repo, &["rev-parse", "wip"]), moved);
    assert_eq!(refs(&repo).len(), 1, "its save still there");
}

/// SM2 test 4: a save goes only once it's both past the newest `count` and older than `days` — whichever keeps more —
/// by a stand-in clock (the saves' dates and `now`).
#[test]
fn pruning_keeps_the_newest_or_the_young_whichever_keeps_more() {
    let (_s, repo, dir) = place("prune");
    let wt = checkout(&repo, &dir, "w");
    let e = ending_of(&repo, &wt, "w");
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let ago = |d: u64| now - Duration::from_secs(d * 86_400);
    let leaves: Vec<String> = [40, 35, 31, 10, 1]
        .map(|d| keep(&e, false, ago(d), crate::fork::take).unwrap().unwrap().0.leaf().to_string())
        .into();
    let common = repo.join(".git");
    assert_eq!(list(&common).unwrap().len(), 5);
    // The newest 4 stay, old or not: only the 40 days' goes
    assert_eq!(prune(&common, 4, 30, now), [leaves[0].clone()]);
    // The newest 1 and the 30 days': the 31 and 35 days' go, the 10 days' stays
    assert_eq!(prune(&common, 1, 30, now), [leaves[2].clone(), leaves[1].clone()]);
    let left: Vec<String> = list(&common).unwrap().iter().map(|s| s.leaf().to_string()).collect();
    assert_eq!(left, [leaves[4].clone(), leaves[3].clone()]);
    assert_eq!(leaves[0], "w-20261206-080000");
    git_in(&repo, &["worktree", "remove", "--force", wt.to_str().unwrap()]);
}

/// SM2 test 7: a save runs no hook (a `reference-transaction` hook, which `update-ref` would fire) and signs nothing (a
/// stand-in `gpg.program` under `commit.gpgSign`) — each writing a marker in the test's own folder, the repository's
/// own commands trusted so that only the save's own guards hold them — and leaves the user's git as it was: the index,
/// HEAD, branches, stash, HEAD's reflog.
#[test]
fn a_save_runs_no_hook_and_leaves_the_users_git_as_it_was() {
    let (_s, repo, dir) = place("hooks");
    let wt = checkout(&repo, &dir, "h");
    std::fs::write(wt.join("new.txt"), "new\n").unwrap();
    git_in(&wt, &["add", "a.txt"]);
    let marker = dir.join("hook-ran");
    let script = format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display());
    std::fs::create_dir_all(repo.join(".git/hooks")).unwrap();
    for name in ["reference-transaction", "post-commit", "post-index-change"] {
        crate::scratch::write_script(&repo.join(".git/hooks").join(name), &script);
    }
    crate::scratch::write_script(&dir.join("gpg-stand-in"), &script);
    git_in(&repo, &["config", "gpg.program", dir.join("gpg-stand-in").to_str().unwrap()]);
    git_in(&repo, &["config", "commit.gpgSign", "true"]);
    for at in [&repo, &wt] {
        crate::gitrepo::forget(at);
        let r = crate::gitrepo::repo(at).unwrap();
        crate::gitrepo::answered(&r.config_file(), r.local.digest(), true);
        crate::gitrepo::forget(at);
        assert!(
            crate::gitrepo::repo(at).unwrap().held.is_none(),
            "its hooks and gpg trusted: only the save holds them"
        );
    }
    let index = git_in(&wt, &["rev-parse", "--git-path", "index"]);
    let index = wt.join(index);
    let look = || {
        (
            std::fs::read(&index).unwrap(),
            git_in(&wt, &["rev-parse", "HEAD"]),
            git_in(&repo, &["for-each-ref", "refs/heads", "refs/stash"]),
            git_in(&wt, &["reflog", "-n", "5"]),
        )
    };
    let before = look();
    let e = ending_of(&repo, &wt, "h");
    let (s, _) = keep(&e, false, crate::clock::system(), crate::fork::take).unwrap().expect("saved");
    assert!(!marker.exists(), "a hook ran");
    assert!(before == look(), "the user's git changed");
    assert_eq!(git_in(&repo, &["show", &format!("{}:new.txt", s.at)]), "new");
    assert_eq!(git_in(&repo, &["log", "-1", "--format=%an <%ae>", &s.at]), "tarae <tarae@localhost>");
    git_in(&repo, &["worktree", "remove", "--force", wt.to_str().unwrap()]);
}

/// SM2 test 6: the end card's way back is there exactly where something would go (`Ending::goes`, either answer) — its
/// changes, its branch's own commits (for `D`), a checkout's on no branch — and never on a quiet card, one whose
/// commits stay on its branch, or one that loses only ignored files.
#[test]
fn the_end_cards_way_back_is_where_something_would_go() {
    let base = Ending {
        root: "/nowhere/w".into(),
        common: "/nowhere/.git".into(),
        name: "w".into(),
        branch: Some("w".into()),
        tip: "0".repeat(40),
        base: "main".into(),
        changed: Vec::new(),
        ahead: Vec::new(),
        lost: Vec::new(),
        entries: Vec::new(),
        keep: Default::default(),
        brought: None,
        days: 30,
        left_out: Vec::new(),
    };
    let commit = vec![("abc1234".to_string(), "a commit".to_string())];
    let line = "What goes is saved for 30 days — :workspace-restore w brings it back.";
    let cases = [
        ("quiet", base.clone(), false),
        ("changed", Ending { changed: vec![" M a.txt".into()], ..base.clone() }, true),
        ("commits on its branch", Ending { ahead: commit.clone(), ..base.clone() }, false),
        (
            "commits only its branch has",
            Ending { ahead: commit.clone(), lost: commit.clone(), ..base.clone() },
            true,
        ),
        ("on no branch", Ending { branch: None, ahead: commit.clone(), ..base.clone() }, true),
        ("on no branch, nothing ahead", Ending { branch: None, ..base.clone() }, false),
    ];
    for (what, e, shown) in cases {
        assert_eq!(e.card().question.contains(line), shown, "{what}: {}", e.card().question);
    }
    // One day said so; past what a save takes at once, the card warns beside its capital letter
    let one = Ending { changed: vec!["?? n".into()], days: 1, ..base.clone() };
    assert!(one.card().question.contains("saved for 1 day —"), "{}", one.card().question);
    let many: Vec<String> = (0..=crate::snapshot::NEW_MAX).map(|i| format!("?? f{i}")).collect();
    let o = Ending { changed: many, ..base.clone() }.card();
    let lines: Vec<&str> = o.question.lines().collect();
    let at =
        lines.iter().position(|l| l.starts_with("Up to 5001 new files: a save takes 5000 at once")).unwrap();
    assert_eq!(lines[at - 1], "Removing takes them out of the checkout.");
    assert_eq!(o.warning, Some(at..at + 1), "only what may not be saved is warned");
    // In the warning color only where it's for good: a save keeps the changes, a deleted branch's commits, a checkout's
    // on no branch
    let saved = [
        Ending {
            changed: vec![" M a.txt".into()],
            ahead: commit.clone(),
            lost: commit.clone(),
            ..base.clone()
        },
        Ending { branch: None, ahead: commit.clone(), ..base.clone() },
    ];
    for e in saved {
        assert_eq!(e.card().warning, None, "{}", e.card().question);
    }
}

/// The picker's rows say a branch only where it isn't the workspace's name, `(no branch)` always — no column at all
/// while every branch is its name.
#[test]
fn the_picker_says_a_branch_only_where_it_isnt_the_name() {
    let s = |name: &str, branch: Option<&str>| Saved {
        at: format!("{REFS}{name}-x"),
        commit: String::new(),
        parent: String::new(),
        name: name.into(),
        branch: branch.map(str::to_string),
        base: "main".into(),
        when: UNIX_EPOCH,
        changed: 2,
        ahead: 1,
        lost: 0,
        left_out: 0,
    };
    let same = [s("fix", Some("fix")), s("docs-pass", Some("docs-pass"))];
    assert_eq!(labels(&same), ["fix        2 files · 1 commit", "docs-pass  2 files · 1 commit"]);
    let mixed = [s("fix", Some("fix")), s("spike", Some("spike/cache")), s("loose", None)];
    assert_eq!(
        labels(&mixed),
        [
            "fix                 2 files · 1 commit",
            "spike  spike/cache  2 files · 1 commit",
            "loose  (no branch)  2 files · 1 commit",
        ]
    );
}

/// Its ref's time part is UTC, and a name is made safe for a ref.
#[test]
fn a_saves_name_is_its_workspaces_and_its_utc_time() {
    assert_eq!(stamp(0), "19700101-000000");
    assert_eq!(stamp(1_800_000_000 + 3661), "20270115-090101");
    assert_eq!(ref_safe("feat/x y"), "feat-x-y");
    assert_eq!(ref_safe("..a..b.lock"), "a.b");
    assert_eq!(ref_safe("카페"), "--");
    assert_eq!(ref_safe(""), "workspace");
}
