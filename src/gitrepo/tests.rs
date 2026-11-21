use super::*;

/// git in `dir` as a test sets up its repository (no launcher: what's set up is what the launcher is asked about).
fn git_in(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(["-c", "protocol.file.allow=always", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Every git a test starts itself goes through `test_git` — no global config at all, never the user's (#678: their
/// `safe.bareRepository=explicit` failed two tests). The list holds what's left, per file, and only shrinks
/// (the message prints the table as it is now).
#[test]
fn every_git_a_test_starts_goes_through_test_git() {
    let now = crate::sim::lint::test_counts(&["new(\"git\")"], &[]);
    crate::sim::lint::only_shrinks("git started directly in tests", &now, GIT_STARTED_IN_TESTS);
}

/// `test_git` takes nothing of git's from the test's environment: each of a repository's own names removed (a
/// test run under `git -c …`, a hook or `rebase -x` would hand it config above every file, or tarae's own
/// repository), and the three that shut out the machine's config set.
#[test]
fn test_git_takes_no_repository_or_config_from_the_environment() {
    let git = test_git("/");
    let envs: std::collections::HashMap<_, _> = git.get_envs().collect();
    // (`git rev-parse --local-env-vars`, git 2.54, and the template folder `fixture_git` kept out)
    let removed = "GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_CONFIG GIT_CONFIG_PARAMETERS GIT_CONFIG_COUNT \
                   GIT_OBJECT_DIRECTORY GIT_DIR GIT_WORK_TREE GIT_IMPLICIT_WORK_TREE GIT_GRAFT_FILE GIT_INDEX_FILE \
                   GIT_NO_REPLACE_OBJECTS GIT_REPLACE_REF_BASE GIT_PREFIX GIT_SHALLOW_FILE GIT_COMMON_DIR \
                   GIT_TEMPLATE_DIR";
    for name in removed.split_whitespace() {
        assert_eq!(envs.get(std::ffi::OsStr::new(name)), Some(&None), "{name}: not removed");
    }
    for (name, value) in
        [("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_CONFIG_NOSYSTEM", "1"), ("XDG_CONFIG_HOME", "/dev/null")]
    {
        assert_eq!(envs.get(std::ffi::OsStr::new(name)), Some(&Some(std::ffi::OsStr::new(value))), "{name}");
    }
}

/// git a test starts itself, outside `test_git`, per file (from the repository's root). `tests/` and xtask are crates
/// of their own: `test_git` is tarae's test build's, so theirs wait for a helper they can reach.
const GIT_STARTED_IN_TESTS: &[(&str, usize)] = &[
    ("src/allow.rs", 2),
    ("src/apply/tests.rs", 1),
    ("src/basetree/tests.rs", 1),
    ("src/billing/tests.rs", 1),
    ("src/chat.rs", 1),
    ("src/codereview.rs", 26),
    ("src/codetrust/tests.rs", 3),
    ("src/compat.rs", 1),
    ("src/config.rs", 7),
    ("src/endsave/tests.rs", 2),
    ("src/folders/tests.rs", 1),
    ("src/fork/tests.rs", 2),
    ("src/git.rs", 2),
    ("src/gitmenu.rs", 2),
    ("src/gitpanel.rs", 3),
    // (`test_git` itself)
    ("src/gitrepo.rs", 1),
    ("src/gitrepo/held/tests.rs", 10),
    ("src/gitrepo/tests.rs", 3),
    ("src/gittool/tests.rs", 2),
    ("src/gittrust/tests.rs", 4),
    ("src/hooks.rs", 1),
    ("src/keepclose/tests.rs", 1),
    ("src/lookalike/tests.rs", 1),
    ("src/lsp_editor.rs", 1),
    ("src/motion/tests.rs", 1),
    ("src/orchestra/tests.rs", 1),
    ("src/overlap/tests.rs", 3),
    ("src/picker.rs", 1),
    ("src/plugin/mod.rs", 2),
    ("src/reads.rs", 2),
    ("src/repotrust/tests.rs", 1),
    ("src/restart.rs", 1),
    ("src/reviewer.rs", 1),
    ("src/reviews/tests.rs", 1),
    ("src/safebare/tests.rs", 1),
    ("src/sandbox.rs", 4),
    ("src/sandbox/linux.rs", 1),
    ("src/sandbox/proxy/macos_tests.rs", 1),
    ("src/sandbox/refused.rs", 1),
    ("src/sim/applies.rs", 1),
    ("src/sim/brings.rs", 1),
    ("src/sim/codetrust.rs", 1),
    ("src/sim/commit.rs", 1),
    ("src/sim/folders.rs", 1),
    ("src/sim/keepclose.rs", 2),
    ("src/sim/lookalike.rs", 1),
    ("src/sim/rebaseexit.rs", 1),
    ("src/sim/sessions.rs", 1),
    ("src/sim/trustscan.rs", 1),
    ("src/sinceyes/tests.rs", 1),
    ("src/snapread.rs", 2),
    ("src/snapshot/look.rs", 1),
    ("src/snapshot/tests.rs", 2),
    ("src/terminal.rs", 1),
    ("src/weave/tests.rs", 1),
    ("src/workspacekeep.rs", 1),
    ("src/worktree.rs", 8),
    ("src/writing/tests.rs", 1),
    ("tests/e2e.rs", 4),
    ("tests/harness/mod.rs", 2),
    ("tests/live.rs", 6),
    ("tests/live/watched.rs", 2),
    ("xtask/src/buildprune/tests.rs", 1),
    ("xtask/src/linux.rs", 3),
    ("xtask/src/record/hands/tests.rs", 1),
    ("xtask/src/scenes.rs", 1),
];

/// A commit is said by its first 7 characters, a shorter one whole — never cut inside a character (#479).
#[test]
fn a_short_sha_is_said_whole() {
    assert_eq!(short_sha("0123456789abcdef0123456789abcdef01234567"), "0123456");
    assert_eq!((short_sha("abc"), short_sha(""), short_sha("0123456")), ("abc", "", "0123456"));
    assert_eq!(short_sha("실타래"), "실타래", "7 bytes end inside 래: whole");
    assert_eq!(short_sha("ab👍🏽cdef"), "ab👍🏽cdef", "inside the emoji: whole");
    assert_eq!(short_sha("실abcd타래"), "실abcd", "on a boundary: its first 7 bytes");
}

/// A repository's folders as given, nothing of its own found (no git run).
fn plain(git_dir: PathBuf, work_tree: Option<PathBuf>, dir: PathBuf) -> Repo {
    Repo {
        common_dir: git_dir.clone(),
        git_dir,
        work_tree,
        dir,
        local: Arc::default(),
        held: None,
        asks: false,
        stamps: Vec::new(),
        racy: false,
        subs: Vec::new(),
        dormant: Vec::new(),
    }
}

/// Past `RACY` since the test's last change: a resolve now is kept.
fn settled() {
    std::thread::sleep(RACY + std::time::Duration::from_millis(100));
}

/// A fresh folder of this test's own in the temp folder, real path.
fn temp(name: &str) -> PathBuf {
    crate::scratch::Scratch::new(&format!("gitrepo-{name}")).into_path()
}

/// A repository with one commit of `files` (path, text) in `dir`.
fn repository(dir: &Path, files: &[(&str, &str)]) {
    for (rel, text) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    git_in(dir, &["init", "-q"]);
    git_in(dir, &["add", "-A"]);
    git_in(dir, &["commit", "-qm", "base"]);
}

/// The tripwire's question: this crate's folders (a subfolder too, and one that isn't there yet) are
/// this repository; a repository of a test's own, even one made under `target/`, is not.
#[test]
fn tests_know_this_repository() {
    let own = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(in_this_repository(own));
    assert!(in_this_repository(&own.join("src")));
    assert!(in_this_repository(&own.join("target/not-there-yet")));
    let dir = own.join(format!("target/tarae-own-repo-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let ok = Command::new("git").args(["init", "-q"]).current_dir(&dir).output().unwrap();
    assert!(ok.status.success());
    assert!(!in_this_repository(&dir), "a repository of its own");
    assert!(!in_this_repository(&std::env::temp_dir()));
    std::fs::remove_dir_all(&dir).ok();
}

/// From a root in a subfolder: the repository's folders, the top as the work tree, git run in the root (its
/// `ls-files` of that folder) — and a file's git from the top, its path from there.
#[test]
fn a_root_resolves_once_and_runs_in_itself() {
    let dir = temp("root");
    repository(&dir, &[("pkg/a.txt", "a\n"), ("b.txt", "b\n")]);
    settled();
    let root = dir.join("pkg");
    let r = repo(&root).unwrap();
    assert_eq!(
        (&r.git_dir, &r.common_dir, &r.work_tree, &r.dir),
        (&dir.join(".git"), &dir.join(".git"), &Some(dir.clone()), &root)
    );
    assert!(r.held.is_none() && !r.asks, "nothing of its own runs commands");
    assert!(Arc::ptr_eq(&r, &repo(&root).unwrap()), "kept");
    assert_eq!(git(&root, &["ls-files"], None).unwrap(), "a.txt\n");
    let (at, rel) = for_file(&root, &dir.join("b.txt")).unwrap();
    assert_eq!(
        (at.dir.as_path(), rel.as_str()),
        (dir.as_path(), "b.txt"),
        "a file outside the root, in the tree"
    );
    assert_eq!(at.run(&["show", &format!(":{rel}")], None).unwrap(), "b\n");
    assert!(for_file(&root, &std::env::temp_dir().join("elsewhere.txt")).is_none());
    std::fs::remove_dir_all(&dir).ok();
}

/// Kept by the root's real path: two spellings of one folder (a link to it) are one entry, one resolving.
#[test]
fn a_root_is_kept_by_its_real_path() {
    let dir = temp("spelling");
    let real = dir.join("r");
    std::fs::create_dir_all(&real).unwrap();
    repository(&real, &[("a.txt", "a\n")]);
    std::os::unix::fs::symlink(&real, dir.join("link")).unwrap();
    settled();
    let a = repo(&real).unwrap();
    assert!(Arc::ptr_eq(&a, &repo(&dir.join("link")).unwrap()), "one entry for both spellings");
    assert!(cached(&real).is_some_and(|c| Arc::ptr_eq(&c, &a)));
    std::fs::remove_dir_all(&dir).ok();
}

/// A folder first resolved inside an outer repository, given a repository of its own since: resolved again
/// (its own `.git` isn't the one kept).
#[test]
fn a_root_with_a_git_of_its_own_since_is_resolved_again() {
    let dir = temp("inner");
    repository(&dir, &[("inner/a.txt", "a\n")]);
    let inner = dir.join("inner");
    assert_eq!(repo(&inner).unwrap().work_tree.as_deref(), Some(dir.as_path()));
    git_in(&inner, &["init", "-q"]);
    settled();
    assert_eq!(repo(&inner).unwrap().work_tree.as_deref(), Some(inner.as_path()), "its own now");
    assert_eq!(cached(&inner).map(|r| r.git_dir.clone()), Some(inner.join(".git")));
    std::fs::remove_dir_all(&dir).ok();
}

/// At most `cap` roots kept; the least lately used goes first — a root asked again stays.
#[test]
fn the_kept_roots_are_capped_least_lately_used_first() {
    let dir = temp("cap");
    let root = |i: usize| dir.join(format!("d{i}"));
    let mut kept = Kept { cap: 3, entries: Vec::new(), epoch_now: || 0 };
    for i in 0..5 {
        std::fs::create_dir_all(root(i).join(".git")).unwrap();
        let r = plain(root(i).join(".git"), Some(root(i)), root(i));
        kept.put(root(i), Arc::new(r), 0);
        if i == 2 {
            assert!(kept.get(&root(0)).is_some(), "0 used again: the newest");
        }
    }
    let left: Vec<PathBuf> = kept.entries.iter().map(|(k, ..)| k.clone()).collect();
    assert_eq!(left, [root(0), root(3), root(4)]);
    assert_eq!(KEPT_MAX, 64);
    std::fs::remove_dir_all(&dir).ok();
}

/// Not a repository: an error, and not kept — a `git init` there later is seen.
#[test]
fn no_repository_is_said_and_not_kept() {
    let dir = temp("none");
    let e = repo(&dir).unwrap_err();
    assert!(e.contains("not a git repository"), "{e}");
    git_in(&dir, &["init", "-q"]);
    assert_eq!(repo(&dir).unwrap().work_tree.as_deref(), Some(dir.as_path()));
    std::fs::remove_dir_all(&dir).ok();
}

/// A root that is a bare-looking folder (`HEAD`, `objects/`, `refs/` — what files in a project can be) is
/// never taken for a repository: `safe.bareRepository=explicit`.
#[test]
fn a_bare_looking_folder_is_no_repository() {
    let dir = temp("bare");
    std::fs::write(dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::create_dir_all(dir.join("objects")).unwrap();
    std::fs::create_dir_all(dir.join("refs")).unwrap();
    std::fs::write(dir.join("config"), "[core]\n\tbare = true\n").unwrap();
    let e = repo(&dir).unwrap_err();
    assert!(e.contains("bare repository"), "{e}");
    std::fs::remove_dir_all(&dir).ok();
}

/// #610: `git_would_take` is git's own test — each of the plan's thirteen layouts judged by real git as well
/// (`rev-parse --absolute-git-dir` from the folder: a scratch `HOME` and global config, no system one, the climb
/// stopped above the layouts), and named exactly where git takes the folder itself. Each entry: `name/` a folder,
/// `name->to` a link, `name=text` a file.
#[test]
fn git_would_take_is_gits_own_test() {
    let s = crate::scratch::Scratch::new("git-would-take");
    let (home, at) = (s.join("home"), s.join("layouts"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("gitconfig"), "").unwrap();
    let git = |dir: &Path, args: &[&str]| {
        let mut c = Command::new("git");
        c.args(["-c", "safe.bareRepository=all", "-c", "init.defaultBranch=main"])
            .args(args)
            .current_dir(dir);
        c.env("HOME", &home).env("GIT_CONFIG_GLOBAL", home.join("gitconfig")).env("GIT_CONFIG_NOSYSTEM", "1");
        c.env("GIT_CEILING_DIRECTORIES", &at);
        for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "GIT_OBJECT_DIRECTORY", "GIT_INDEX_FILE"] {
            c.env_remove(v);
        }
        c.output().unwrap()
    };
    let head = "HEAD=ref: refs/heads/main\n";
    let whole = [head, "objects/", "refs/"];
    let with = |more: &[&'static str]| whole.iter().copied().chain(more.iter().copied()).collect::<Vec<_>>();
    let layouts: Vec<(&str, Vec<&str>, bool)> = vec![
        ("1-ref", with(&[]), true),
        ("5-id", vec!["HEAD=0123456789abcdef0123456789abcdef01234567\n", "objects/", "refs/"], true),
        ("6-link-head", vec!["HEAD->refs/heads/main", "objects/", "refs/"], true),
        ("9-linked-folders", vec![head, "o/", "r/", "objects->o", "refs->r"], true),
        ("7-commondir", vec![head, "commondir=common\n", "common/", "common/objects/", "common/refs/"], true),
        ("2-head-alone", vec![head], false),
        ("3-one-folder", vec![head, "objects/"], false),
        ("4-junk-head", vec!["HEAD=not a head\n", "objects/", "refs/"], false),
        ("8-files", vec![head, "objects=x", "refs=x"], false),
        (
            "18-a-git-too",
            with(&[".git/", ".git/HEAD=ref: refs/heads/main\n", ".git/objects/", ".git/refs/"]),
            false,
        ),
        ("19-junk-git-file", with(&[".git=junk\n"]), false),
        ("20-empty-git-folder", with(&[".git/"]), true),
    ];
    for (name, entries, _) in &layouts {
        let dir = at.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        for e in entries {
            if let Some(d) = e.strip_suffix('/') {
                std::fs::create_dir_all(dir.join(d)).unwrap();
            } else if let Some((n, to)) = e.split_once("->") {
                std::os::unix::fs::symlink(to, dir.join(n)).unwrap();
            } else if let Some((n, text)) = e.split_once('=') {
                std::fs::write(dir.join(n), text).unwrap();
            }
        }
    }
    // (21: a checkout's own top, its HEAD, objects/ and refs/ beside its `.git`)
    let root = at.join("21-root");
    std::fs::create_dir_all(&root).unwrap();
    assert!(git(&root, &["init", "-q"]).status.success());
    for e in whole {
        match e.split_once('=') {
            Some((n, text)) => std::fs::write(root.join(n), text).unwrap(),
            None => std::fs::create_dir_all(root.join(e)).unwrap(),
        }
    }
    let all = layouts.iter().map(|(n, _, named)| (*n, *named)).chain([("21-root", false)]);
    let mut wrong = Vec::new();
    for (name, named) in all {
        let dir = at.join(name);
        let out = git(&dir, &["rev-parse", "--absolute-git-dir"]);
        let said = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let gits = out.status.success()
            && std::fs::canonicalize(&said).ok() == Some(std::fs::canonicalize(&dir).unwrap());
        let ours = git_would_take(&dir).is_some();
        if ours != gits || ours != named {
            wrong.push(format!("{name}: ours {ours}, git's {gits} ({said:?}), the plan's {named}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
    assert_eq!(git_would_take(&at.join("7-commondir")), Some(Takes::Common));
    assert_eq!(git_would_take(&at.join("1-ref")), Some(Takes::Folders));
}

/// A linked checkout: its own git folder, the common one, and its own top; the common one named alone runs
/// what outlives the checkout (`worktree list`).
#[test]
fn a_linked_checkout_has_its_own_git_folder() {
    let dir = temp("linked");
    let main = dir.join("main");
    std::fs::create_dir_all(&main).unwrap();
    repository(&main, &[("a.txt", "a\n")]);
    git_in(&main, &["worktree", "add", "-q", "-b", "side", "../side"]);
    let side = dir.join("side");
    let r = repo(&side).unwrap();
    assert_eq!(r.git_dir, main.join(".git/worktrees/side"));
    assert_eq!(r.common_dir, main.join(".git"));
    assert_eq!(r.work_tree.as_deref(), Some(side.as_path()));
    assert_eq!(git(&side, &["symbolic-ref", "--short", "HEAD"], None).unwrap(), "side\n");
    let list = Repo::git_folder(&r.common_dir).run(&["worktree", "list", "--porcelain"], None).unwrap();
    assert_eq!(list.lines().filter(|l| l.starts_with("worktree ")).count(), 2, "{list}");
    std::fs::remove_dir_all(&dir).ok();
}

/// A file in no session's repository (`for_file_in`): its own repository, found from its folder, when no run
/// writes there — but not one whose git folder is where runs write (a linked checkout outside of a repository
/// inside), nor one inside a write place (told: something there passes for a repository).
#[test]
fn a_file_outside_the_sessions_is_read_only_away_from_writes() {
    let dir = temp("outside");
    let (w, plain, linked) = (dir.join("w"), dir.join("plain"), dir.join("linked"));
    std::fs::create_dir_all(w.join("main")).unwrap();
    std::fs::create_dir_all(&plain).unwrap();
    repository(&plain, &[("a.txt", "plain\n")]);
    repository(&w.join("main"), &[("a.txt", "main\n")]);
    git_in(&w.join("main"), &["worktree", "add", "-q", "-b", "side", linked.to_str().unwrap()]);
    let writes = vec![w.clone()];
    let places = Places { roots: Vec::new(), writes: Arc::new(move || Some(writes.clone())) };
    let (r, rel) = for_file_in(&places, &plain.join("a.txt")).unwrap();
    assert_eq!((r.git_dir.clone(), rel.as_str()), (plain.join(".git"), "a.txt"));
    assert_eq!(for_file_in(&places, &linked.join("a.txt")).err(), Some(NoGit::Writable { looks: true }));
    assert_eq!(for_file_in(&places, &w.join("main/a.txt")).err(), Some(NoGit::Writable { looks: true }));
    std::fs::write(w.join("loose.txt"), "x\n").unwrap();
    assert_eq!(for_file_in(&places, &w.join("loose.txt")).err(), Some(NoGit::Writable { looks: false }));
    // Runs not sandboxed: nothing held back — found from the file's folder, wherever (a bare look-alike still not)
    let anywhere = Places { roots: Vec::new(), writes: Arc::new(|| None) };
    let (r, _) = for_file_in(&anywhere, &w.join("main/a.txt")).unwrap();
    assert_eq!(r.git_dir, w.join("main/.git"));
    let bare = w.join("bare");
    for d in ["objects", "refs"] {
        std::fs::create_dir_all(bare.join(d)).unwrap();
    }
    std::fs::write(bare.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(bare.join("x.txt"), "x\n").unwrap();
    assert_eq!(for_file_in(&anywhere, &bare.join("x.txt")).err(), Some(NoGit::None));
    std::fs::remove_dir_all(&dir).ok();
}

/// The shell line the rebase runs: both folders given, quoted, the guard on, no fetch, git's words in English
/// (`ENGLISH`: `rebase_ended` reads its `error:` lines).
#[test]
fn the_shell_line_names_the_repository() {
    let mut r = plain("/r/it's/.git".into(), Some("/r/it's".into()), "/r/it's".into());
    let start = r"git --git-dir '/r/it'\''s/.git' --work-tree '/r/it'\''s' -c safe.bareRepository=explicit";
    let no_fetch = ("GIT_NO_LAZY_FETCH".to_string(), "1".to_string());
    let english = ("LANGUAGE".to_string(), "en".to_string());
    assert_eq!(r.shell_line(), Line { text: start.into(), env: vec![no_fetch.clone(), english.clone()] });
    // (what's held goes in as on every run: each value in a variable of its own, named by `--config-env` — a
    // key that holds a `=` whole — the values only in the environment, the network's held too; no flags)
    let config = vec![
        ("merge.m=x.driver".to_string(), "git merge-file %A".to_string()),
        ("credential.helper".to_string(), "store --file /x/tarae-fake-token".to_string()),
        ("filter.f.clean".to_string(), "it's".to_string()),
    ];
    let env = vec![("GIT_PROXY_COMMAND", "false")];
    let flags = vec![("--no-textconv", &["blame"][..])];
    r.held = Some(Arc::new(Hold { config, env, flags, ..Hold::default() }));
    let line = r.shell_line();
    assert_eq!(
        line.text,
        format!(
            "{start} '--config-env=merge.m=x.driver=TARAE_GIT_HELD_0' '--config-env=credential.helper=TARAE_GIT_HELD_1' '--config-env=filter.f.clean=TARAE_GIT_HELD_2'"
        )
    );
    let env = [
        ("GIT_NO_LAZY_FETCH", "1"),
        ("LANGUAGE", "en"),
        ("TARAE_GIT_HELD_0", "git merge-file %A"),
        ("TARAE_GIT_HELD_1", "store --file /x/tarae-fake-token"),
        ("TARAE_GIT_HELD_2", "it's"),
        ("GIT_PROXY_COMMAND", "false"),
    ];
    assert_eq!(line.env, env.map(|(k, v)| (k.to_string(), v.to_string())));
}

/// A file in a submodule: its own repository, from the git folder the session's records (`modules/…`), its
/// path from the submodule's top — the gutter's base its index's. A folder with a `.git` that the index has
/// no gitlink for, or a gitlink whose `.git` names a folder outside `modules/`, is the session's repository's
/// (nothing taken from it).
#[test]
fn a_submodule_file_takes_the_submodules_recorded_repository() {
    let dir = temp("sub");
    let (lib, top) = (dir.join("lib"), dir.join("top"));
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::create_dir_all(&top).unwrap();
    repository(&lib, &[("src/x.txt", "lib's\n")]);
    repository(&top, &[("a.txt", "top's\n")]);
    git_in(&top, &["submodule", "add", "-q", lib.to_str().unwrap(), "vendor/lib"]);
    git_in(&top, &["commit", "-qm", "lib"]);
    let file = top.join("vendor/lib/src/x.txt");
    std::fs::write(&file, "lib's\nchanged\n").unwrap();
    let (r, rel) = for_file(&top, &file).unwrap();
    assert_eq!(r.git_dir, top.join(".git/modules/vendor/lib"));
    assert_eq!(r.work_tree.as_deref(), Some(top.join("vendor/lib").as_path()));
    assert_eq!(rel, "src/x.txt");
    assert_eq!(crate::git::index_blob(&Places::only(&top), &file).unwrap().as_deref(), Some("lib's\n"));

    // Not a gitlink: a folder of the session's repository with a `.git` naming the module's folder
    std::fs::create_dir_all(top.join("other")).unwrap();
    std::fs::write(top.join("other/y.txt"), "y\n").unwrap();
    git_in(&top, &["add", "other/y.txt"]);
    std::fs::write(top.join("other/.git"), "gitdir: ../.git/modules/vendor/lib\n").unwrap();
    let (r, rel) = for_file(&top, &top.join("other/y.txt")).unwrap();
    assert_eq!((r.git_dir.clone(), rel.as_str()), (top.join(".git"), "other/y.txt"));
    assert_eq!(
        crate::git::index_blob(&Places::only(&top), &top.join("other/y.txt")).unwrap().as_deref(),
        Some("y\n")
    );

    // A gitlink whose `.git` names a git folder outside `modules/`: not followed
    std::fs::write(top.join("vendor/lib/.git"), format!("gitdir: {}\n", lib.join(".git").display())).unwrap();
    let (r, rel) = for_file(&top, &file).unwrap();
    assert_eq!((r.git_dir.clone(), rel.as_str()), (top.join(".git"), "vendor/lib/src/x.txt"));
    assert_eq!(
        crate::git::index_blob(&Places::only(&top), &file).unwrap(),
        None,
        "the session's index has only the gitlink"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A test's git reads no global ignore, not even from its default place, which git reads with no config at all
/// (`~/.config/git/ignore`): with a stand-in home whose ignore lists `.env`, `.env` isn't ignored.
#[test]
fn a_tests_git_reads_no_global_ignore_from_its_default_place() {
    let dir = temp("xdg");
    git_in(&dir, &["init", "-q"]);
    let home = dir.join("home");
    std::fs::create_dir_all(home.join(".config/git")).unwrap();
    std::fs::write(home.join(".config/git/ignore"), ".env\n").unwrap();
    let out = base(&dir).env("HOME", &home).args(["check-ignore", "-q", ".env"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "not ignored: {}", String::from_utf8_lossy(&out.stderr));
    std::fs::remove_dir_all(&dir).ok();
}

/// tarae's own git speaks English (`ENGLISH`: tarae reads its words); the hooked form leaves the user's language as
/// it is — the user's hooks get git's environment, and a translated tool in them stays translated.
#[test]
fn taraes_own_git_is_in_english_and_the_hooked_form_in_the_users_language() {
    let dir = temp("english");
    repository(&dir, &[("a.txt", "a\n")]);
    let r = repo(&dir).unwrap();
    let language = |cmd: &Command| {
        cmd.get_envs().find(|(k, _)| *k == "LANGUAGE").map(|(_, v)| v.map(std::ffi::OsStr::to_os_string))
    };
    assert_eq!(language(&r.command(&["status"])), Some(Some("en".into())), "tarae's own");
    assert_eq!(language(&r.hooked_command(&["status"]).unwrap()), None, "the user's, inherited");
    std::fs::remove_dir_all(&dir).ok();
}

/// A fixture's git (`fixture_git`) takes nothing of the machine's: a user config naming a hooks folder, an fsmonitor
/// and a template folder, and the environment's template folder — none runs (markers in the test's own scratch) nor
/// is copied into the repository it makes. In a process of its own (the variables are the process's): this binary on
/// `fixture_git_in_its_own_process`.
#[test]
fn a_fixtures_git_takes_none_of_the_machines_config() {
    use std::os::unix::fs::PermissionsExt as _;
    let s = crate::scratch::Scratch::new("fixture-git");
    let (ran, repo) = (s.join("ran"), s.join("repo"));
    let script = |at: PathBuf, name: &str| {
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(&at, format!("#!/bin/sh\necho {name} >> '{}'\n", ran.display())).unwrap();
        std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    for hook in ["pre-commit", "post-commit"] {
        script(s.join("hooks").join(hook), hook);
    }
    script(s.join("fsmonitor"), "fsmonitor");
    for (t, file) in [("template", "from-config"), ("env-template", "from-env")] {
        std::fs::create_dir_all(s.join(t).join("info")).unwrap();
        std::fs::write(s.join(t).join("info").join(file), "").unwrap();
    }
    let config = s.join("user.gitconfig");
    let (hooks, fsmonitor, template) = (s.join("hooks"), s.join("fsmonitor"), s.join("template"));
    std::fs::write(
        &config,
        format!(
            "[core]\n\thooksPath = {}\n\tfsmonitor = {}\n[init]\n\ttemplateDir = {}\n",
            hooks.display(),
            fsmonitor.display(),
            template.display()
        ),
    )
    .unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "gitrepo::tests::fixture_git_in_its_own_process", "--ignored", "--nocapture"])
        .env("GIT_CONFIG_GLOBAL", &config)
        .env("GIT_TEMPLATE_DIR", s.join("env-template"))
        .env("TARAE_FIXTURE_AT", &repo)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && String::from_utf8_lossy(&out.stdout).contains("1 passed"), "{err}");
    assert!(!ran.exists(), "ran: {}", std::fs::read_to_string(&ran).unwrap_or_default());
    for file in ["from-config", "from-env"] {
        assert!(!repo.join(".git/info").join(file).exists(), "{file}: a template copied");
    }
    assert!(repo.join(".git/HEAD").exists(), "its repository made");
}

#[test]
#[ignore = "a_fixtures_git_takes_none_of_the_machines_config's own process"]
fn fixture_git_in_its_own_process() {
    let at = PathBuf::from(std::env::var_os("TARAE_FIXTURE_AT").unwrap());
    fixture_git(&at, &["init", "-q"]);
    std::fs::write(at.join("a.txt"), "a\n").unwrap();
    fixture_git(&at, &["add", "-A"]);
    fixture_git(&at, &["status", "--porcelain"]);
    fixture_git(&at, &["commit", "-qm", "a"]);
}

/// The reads that wait on a FIFO: a blocking open, then a read.
const BLOCKING_READS: &[&str] = &["read_to_string(", "fs::read(", "File::open("];

/// The words that say a path is one of git's files a project holds or names.
const GIT_NAMES: &[&str] = &[".git\"", "commondir", "gitdir", "packed-refs", "HEAD", ".gitmodules"];

/// Blocking reads of git's files left, per file (`no_read_of_gits_files_waits`) — none since #607's sweep (the
/// design session's search found none, 2026-11-16). The list only shrinks: a read added fails.
const BLOCKING_GIT_READS: &[(&str, usize)] = &[];

/// Each file's blocking reads of git's files: one of `BLOCKING_READS` whose own line or call (its arguments, to its
/// closing parenthesis — a call rustfmt breaks over lines too) holds one of `GIT_NAMES`. A path built on a line
/// before isn't seen: the words are what this can go by.
fn blocking_git_reads(files: Vec<(String, String)>) -> std::collections::BTreeMap<String, usize> {
    let mut out = std::collections::BTreeMap::new();
    for (file, code) in files {
        let mut n = 0;
        for read in BLOCKING_READS {
            for (at, _) in code.match_indices(read) {
                let start = code[..at].rfind('\n').map_or(0, |i| i + 1);
                let end = code[at..].find('\n').map_or(code.len(), |i| at + i);
                let args = call_args(&code[at + read.len()..]);
                n += usize::from(GIT_NAMES.iter().any(|w| code[start..end].contains(w) || args.contains(w)));
            }
        }
        if n > 0 {
            out.insert(file, n);
        }
    }
    out
}

/// A call's arguments: `rest`, just past its `(`, to the `)` that closes it.
fn call_args(rest: &str) -> &str {
    let mut depth = 1;
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 1 => return &rest[..i],
            ')' => depth -= 1,
            _ => {}
        }
    }
    rest
}

/// #607's sweep held: no read of one of git's files in what a non-test build compiles (`sim::lint::sources`: every
/// code root, comments and test code out) opens it waiting — each goes through `git_file` (`paths::plain_file_within`
/// under it). One put back fails here, naming its file.
#[test]
fn no_read_of_gits_files_waits() {
    let now = blocking_git_reads(crate::sim::lint::sources());
    crate::sim::lint::only_shrinks("blocking reads of git's files", &now, BLOCKING_GIT_READS);
}

/// What `no_read_of_gits_files_waits` counts: a blocking read naming one of git's files on its line or in its call,
/// broken over lines too — not a read through `git_file`, nor a blocking read of another file.
#[test]
fn a_blocking_read_of_a_git_file_counts() {
    let code = "let a = std::fs::read_to_string(dir.join(\"commondir\")).ok();\n\
                let b = crate::gitrepo::git_file(&dir.join(\"commondir\"));\n\
                let c = fs::read(\n    p.join(\"HEAD\"),\n);\n\
                let d = File::open(&top.join(\".gitmodules\"));\n\
                let e = std::fs::read_to_string(&notes);\n\
                let f = File::open(p.join(\".github\"));\n";
    let n = blocking_git_reads(vec![("a.rs".into(), code.into())]);
    assert_eq!(n.get("a.rs"), Some(&3), "{n:?}");
}

/// #607: `git_file` reads a plain file and refuses at once — unopened — a FIFO, a link to one and a folder; past its
/// bound it reads nothing. Each look bounded by the shared patience; files written by hand, no git run.
#[test]
fn a_fifo_in_place_of_a_git_file_is_refused_unopened() {
    let dir = crate::scratch::Scratch::new("gitrepo-fifo");
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let (plain, fifo, link) = (base.join("plain"), base.join("fifo"), base.join("link"));
    std::fs::write(&plain, "gitdir: x\n").unwrap();
    fifo_at(&fifo);
    std::os::unix::fs::symlink(&fifo, &link).unwrap();
    let read = bounded(move || {
        let within = |n| git_file_within(&plain, n);
        (git_file(&plain), within(10), within(9), git_file(&fifo), git_file(&link), git_file(&base))
    });
    let text = Some("gitdir: x\n".to_string());
    assert_eq!(read, (text.clone(), text, None, None, None, None));
}

/// #607: the reads `submodules` and `submodule` make of a submodule's checkout — its `.git` file (a run can make a
/// FIFO by that name in a project's folder) and the `commondir` of the git folder it names — never wait: a FIFO
/// `.git` names no git folder, a FIFO `commondir` names no common one (the git folder is its own). Each look bounded
/// by the shared patience; git's files written by hand, no git run.
#[test]
fn a_fifo_in_place_of_a_submodules_git_files_is_never_waited_on() {
    let dir = crate::scratch::Scratch::new("gitrepo-sub-fifo");
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let (sub, git_dir) = (base.join("sub"), base.join("modules/sub"));
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::create_dir_all(&git_dir).unwrap();
    std::fs::write(sub.join(".git"), "gitdir: ../modules/sub\n").unwrap();
    std::fs::write(git_dir.join("commondir"), "../..\n").unwrap();
    let look = |sub: &Path| {
        let sub = sub.to_path_buf();
        bounded(move || {
            let named = named_git_dir(&sub);
            let common = named.as_deref().and_then(|n| std::fs::canonicalize(n).ok()).map(|g| common_of(&g));
            (named, common)
        })
    };
    let named = Some(sub.join("../modules/sub"));
    assert_eq!(look(&sub), (named.clone(), Some(base.clone())), "as written");
    fifo_at(&git_dir.join("commondir"));
    assert_eq!(look(&sub), (named, Some(git_dir.clone())), "no commondir read: its own");
    fifo_at(&sub.join(".git"));
    assert_eq!(look(&sub), (None, None), "no git folder named");
}
