//! #609's tests: what the reader finds and the policy keeps, for real under each system's sandbox. Harmless: git's
//! files written by hand as plain text, plain-text markers (nothing executable, no hook, no git run), all in the
//! test's own scratch folder.

use super::*;
use crate::sandbox::{Env, Policy, command, git_found, policy_key, profile_path};

/// The test's scratch folder and its real path — not under a place every run writes (Linux's temp folder is `/tmp`),
/// so what's outside a project is outside what its run writes.
fn scratch(name: &str) -> (crate::scratch::Scratch, PathBuf) {
    #[cfg(target_os = "macos")]
    let s = crate::scratch::Scratch::new(name);
    #[cfg(not(target_os = "macos"))]
    let s = {
        let under = std::env::current_dir().unwrap().join("target");
        std::fs::create_dir_all(&under).unwrap();
        crate::scratch::Scratch::in_dir(&under, name)
    };
    let t = profile_path(&s);
    (s, t)
}

/// Whether the real sandbox can't start here (inside tarae's on macOS, Landlock short on Linux): said, skipped.
fn skipped(name: &str) -> bool {
    let test = format!("sandbox::gitfolders::tests::{name}");
    #[cfg(target_os = "macos")]
    return crate::sandbox::skipped_inside(&test);
    #[cfg(target_os = "linux")]
    return match crate::landlock::short_of(2) {
        Some(why) => {
            crate::test_results::said_skip(&format!("{test} — {why}"));
            true
        }
        None => false,
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = test;
        true
    }
}

/// Plain files at `at`, their folders made.
fn files(at: &Path, list: &[(&str, &str)]) {
    for (rel, text) in list {
        let f = at.join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }
}

/// A link at `at/rel` whose text is `to`.
fn link(at: &Path, rel: &str, to: &str) {
    let l = at.join(rel);
    std::fs::create_dir_all(l.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(to, l).unwrap();
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| format!("({e})"))
}

/// The policy for `env`'s project as a run's is made: its settings and what the reader finds (`git_found`, as
/// `env_with` does on the policy's worker).
fn policy_of(env: Env) -> Policy {
    let env = Env { git: git_found(&env), ..env };
    Policy::for_project(&env)
}

/// `lines` run under `env`'s policy, `vars` in their environment: each `t <what> '<command>'` answers `<what> ok`
/// where the command did, `<what> no` where it was refused.
fn run(env: Env, vars: &[(&str, &Path)], lines: &str) -> Vec<String> {
    let script = format!(
        "t() {{ if ( eval \"$2\" ) >/dev/null 2>&1; then echo \"$1 ok\"; else echo \"$1 no\"; fi; }}\n{lines}"
    );
    #[cfg(target_os = "macos")]
    let out = {
        // (with the walk before a run: the nested checkouts' git folders, #609 L2)
        let p = crate::sandbox::keyfolders::locked(policy_of(env));
        let mut c = command(Some(&p), "/bin/sh", &["-c".into(), script]);
        c.envs(vars.iter().copied()).stdin(std::process::Stdio::null()).output().unwrap().stdout
    };
    #[cfg(not(target_os = "macos"))]
    let out = {
        use crate::sandbox::{Here, RunDir, confined, run_env_of, spawn};
        let tmp = Some(profile_path(&std::env::temp_dir()));
        let run_env = run_env_of(&Here::now(), &crate::llm::LlmConfig::default());
        let env = Env { tmp, run_env, ..env };
        let p = confined(Policy { run: RunDir::new().ok(), ..policy_of(env) });
        let mut c = command(Some(&p), "sh", &["-c".into(), script]);
        c.envs(vars.iter().copied());
        let spawned = spawn(Some(&p), &c).unwrap();
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut spawned.stdout.unwrap(), &mut out).unwrap();
        crate::spawn::wait(spawned.pid).unwrap();
        out
    };
    String::from_utf8_lossy(&out).lines().map(str::to_string).collect()
}

/// For real, on both systems: a checkout whose `.git` is a file naming a git folder inside the project (`gitdir:
/// gitdata`, as `git init --separate-git-dir gitdata` leaves it) has that folder kept as `.git` is — its `hooks/`,
/// its `config`, a linked checkout's own `worktrees/<ws>/` — and neither the `.git` file nor a nested repository's
/// `.git/hooks`; a hard link to its `config` can't be made to write through (Seatbelt refuses a link whose source
/// isn't writable; Landlock's `REFER`). A linked checkout's run doesn't reach the common folder: it's outside its
/// project. Broken by a reader that finds nothing: the three `gitdata-*` lines and the link turn to `ok`.
#[test]
fn a_git_folder_a_git_file_names_in_the_project_is_kept_from_runs() {
    if skipped("a_git_folder_a_git_file_names_in_the_project_is_kept_from_runs") {
        return;
    }
    let (_s, t) = scratch("sb-gitdata");
    let (main, ws) = (t.join("main"), t.join("ws"));
    let linked = main.join("gitdata/worktrees/ws");
    let config = "[core]\n\trepositoryformatversion = 0\n";
    for (f, text) in [
        (main.join(".git"), "gitdir: gitdata\n".to_string()),
        (main.join("gitdata/HEAD"), "ref: refs/heads/main\n".into()),
        (main.join("gitdata/config"), config.into()),
        (main.join("gitdata/hooks/README.sample"), "kept\n".into()),
        (linked.join("HEAD"), "ref: refs/heads/ws\n".into()),
        (linked.join("commondir"), "../..\n".into()),
        (linked.join("gitdir"), format!("{}\n", ws.join(".git").display())),
        (main.join("nested/.git/hooks/README.sample"), "kept\n".into()),
        (main.join("src/main.rs"), "x".into()),
        (ws.join(".git"), format!("gitdir: {}\n", linked.display())),
        (ws.join("src/lib.rs"), "x".into()),
    ] {
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }
    for d in ["gitdata/objects", "gitdata/refs/heads"] {
        std::fs::create_dir_all(main.join(d)).unwrap();
    }
    let dot_git = |at: &Path| std::fs::read_to_string(at.join(".git")).unwrap();
    let (main_git, ws_git) = (dot_git(&main), dot_git(&ws));
    let vars = [("M", main.as_path()), ("W", ws.as_path())];
    // The main checkout's run: its `.git` file, a plain `.git` by name, and the folder the file names
    let got = run(
        Env { root: main.clone(), ..Env::default() },
        &vars,
        r##"
        t own-file 'printf x > "$M/src/x.rs"'
        t dot-git-file 'printf x >> "$M/.git"'
        t nested-git-hook 'printf x > "$M/nested/.git/hooks/x"'
        t gitdata-hook 'printf x > "$M/gitdata/hooks/x"'
        t gitdata-config 'printf "# x\n" >> "$M/gitdata/config"'
        t gitdata-linked 'printf x > "$M/gitdata/worktrees/ws/x"'
        t gitdata-hard-link 'ln "$M/gitdata/config" "$M/x" && printf y >> "$M/x"'
        "##,
    );
    let want = [
        "own-file ok",
        "dot-git-file no",
        "nested-git-hook no",
        "gitdata-hook no",
        "gitdata-config no",
        "gitdata-linked no",
        "gitdata-hard-link no",
    ];
    assert_eq!(got, want, "the main checkout's run");
    assert_eq!(dot_git(&main), main_git);
    assert!(!main.join("nested/.git/hooks/x").exists());
    assert!(!main.join("gitdata/hooks/x").exists() && !linked.join("x").exists());
    assert_eq!(read(&main.join("gitdata/config")), config, "as it was, by neither name");
    // The linked checkout's run: the common folder, and its own git folder in it, are outside its project
    let got = run(
        Env { root: ws.clone(), ..Env::default() },
        &vars,
        r#"
        t own-file 'printf x > "$W/src/x.rs"'
        t dot-git-file 'printf x >> "$W/.git"'
        t common-hook 'printf x > "$M/gitdata/hooks/y"'
        t linked-own 'printf x > "$M/gitdata/worktrees/ws/y"'
        "#,
    );
    let want = ["own-file ok", "dot-git-file no", "common-hook no", "linked-own no"];
    assert_eq!(got, want, "the linked checkout's run");
    assert_eq!(dot_git(&ws), ws_git);
    assert!(!main.join("gitdata/hooks/y").exists() && !linked.join("y").exists());
}

/// Android `repo`'s shape, by hand, for real on both systems: (a) links among a `.git` folder's entries into the
/// project (`hooks`, `config` → `tools/…`) — their targets refused, a file beside them in `tools` written; (b) the
/// project `top/proj` with `top/.repo` opened for writing (`llm.sandbox-write`), its `.git` entries linked into
/// `.repo/projects/proj.git` — those refused, a plain file in `.repo` written. (A folder holding the project is never
/// opened — `Policy::for_project` — so `.repo` is.) Broken by the links rule left out of the reader.
#[test]
fn a_git_folders_links_lead_to_what_is_kept_from_runs() {
    if skipped("a_git_folders_links_lead_to_what_is_kept_from_runs") {
        return;
    }
    let (_s, t) = scratch("sb-gitlinks");
    // (a)
    let r = t.join("r");
    let config = "[core]\n\tbare = false\n";
    files(
        &r,
        &[
            (".git/HEAD", "ref: refs/heads/main\n"),
            ("tools/hooks/pre-commit", "orig\n"),
            ("tools/config", config),
            ("tools/x", ""),
        ],
    );
    link(&r, ".git/hooks", "../tools/hooks");
    link(&r, ".git/config", "../tools/config");
    let got = run(
        Env { root: r.clone(), ..Env::default() },
        &[("R", &r)],
        r#"
        t hook 'printf x > "$R/tools/hooks/y"'
        t config 'printf x >> "$R/tools/config"'
        t through-git 'printf x > "$R/.git/hooks/y"'
        t beside 'printf x >> "$R/tools/x"'
        "#,
    );
    assert_eq!(got, ["hook no", "config no", "through-git no", "beside ok"], "links in the root");
    assert!(!r.join("tools/hooks/y").exists());
    assert_eq!(
        (read(&r.join("tools/config")), read(&r.join("tools/x"))),
        (config.to_string(), "x".to_string())
    );
    // (b)
    let top = t.join("top");
    let (proj, repo) = (top.join("proj"), top.join(".repo"));
    files(
        &top,
        &[
            (".repo/projects/proj.git/HEAD", "ref: refs/heads/main\n"),
            (".repo/projects/proj.git/hooks/pre-commit", "orig\n"),
            (".repo/projects/proj.git/config", config),
            ("proj/.git/HEAD", "ref: refs/heads/main\n"),
            ("proj/src/main.rs", "x"),
            (".repo/x", ""),
        ],
    );
    link(&proj, ".git/hooks", "../../.repo/projects/proj.git/hooks");
    link(&proj, ".git/config", "../../.repo/projects/proj.git/config");
    let env = Env { root: proj.clone(), write: vec![repo.display().to_string()], ..Env::default() };
    let got = run(
        env,
        &[("T", &top)],
        r#"
        t hook 'printf x > "$T/.repo/projects/proj.git/hooks/y"'
        t config 'printf x >> "$T/.repo/projects/proj.git/config"'
        t plain 'printf x >> "$T/.repo/x"'
        "#,
    );
    assert_eq!(got, ["hook no", "config no", "plain ok"], "links into a folder opened for writing");
    assert!(!repo.join("projects/proj.git/hooks/y").exists());
    assert_eq!(read(&repo.join("projects/proj.git/config")), config);
}

/// A `.git` pointed elsewhere is another policy: its key moves with the folder `.git` names, and comes back with it
/// (a keeper's tier-own seals are made from the new key's policy — D14). Broken by `git` and `protect` left out of
/// the key.
#[test]
fn a_git_folder_named_elsewhere_is_another_policy_key() {
    let (_s, t) = scratch("gf-key");
    files(
        &t,
        &[("a/HEAD", "ref: refs/heads/a\n"), ("b/HEAD", "ref: refs/heads/b\n"), (".git", "gitdir: a\n")],
    );
    let key = || policy_key(&policy_of(Env { root: t.clone(), ..Env::default() }));
    let k1 = key();
    std::fs::write(t.join(".git"), "gitdir: b\n").unwrap();
    let k2 = key();
    assert_ne!(k1, k2);
    std::fs::write(t.join(".git"), "gitdir: a\n").unwrap();
    assert_eq!(key(), k1);
}

/// The policy's worker fills what the reader finds (`env_with` → `git_found`). Broken by the call left out.
#[test]
fn env_with_reads_the_git_folders() {
    let (_s, t) = scratch("gf-env");
    files(&t, &[(".git", "gitdir: gitdata\n"), ("gitdata/HEAD", "ref: refs/heads/main\n")]);
    let llm = crate::llm::LlmConfig::default();
    let (env, _) = crate::sandbox::env_in(&t, &llm, &[], &crate::sandbox::Here::now());
    let found: Vec<&Path> = env.git.found.iter().map(|f| f.target.as_path()).collect();
    assert_eq!(found, [t.join("gitdata")], "git_folder found by the worker");
    assert_eq!(Policy::for_project(&env).git.git, [t.join("gitdata")]);
}

/// The ways to a kept git folder can't be swapped, for real on both systems: `.git/hooks -> ../tools/githooks`
/// (its way `tools`), `gitdir: sub/gitdata` (`sub`) and a dangling `.git/hooks` (`tools` not there): renamed away, a
/// link or a new folder in its place, removed, made where it's missing or renamed onto — every one refused, and git's
/// hooks read as they were (or nothing), never the plant in `evil/`; a file in the way's folders written as ever (where Linux's runs have no seals a folder there
/// takes no new entry, as any folder holding a protected name). On
/// macOS the other case too (APFS: `TOOLS` is `tools`). Broken by the lock's literals left out (macOS), the marks left
/// out (Linux, tier own: the dangling layout).
#[test]
fn a_git_folders_chain_cant_be_swapped_by_a_run() {
    // (each line on its own: what one that landed did is put back before the next — a `<way>x` moved home — so a
    // broken lock shows every line that lands, not only the first)
    const UNDO: &str =
        "u() { [ -e \"$R/$1x\" ] && { rm -rf \"$R/$1\"; mv \"$R/$1x\" \"$R/$1\"; }; } 2>/dev/null\n";
    if skipped("a_git_folders_chain_cant_be_swapped_by_a_run") {
        return;
    }
    let (_s, t) = scratch("sb-gitchain");
    // `.git/hooks -> ../tools/githooks`
    let a = t.join("a");
    files(
        &a,
        &[
            (".git/HEAD", "ref: refs/heads/main\n"),
            ("tools/githooks/pre-commit", "orig\n"),
            ("tools/x", ""),
            ("evil/githooks/pre-commit", "plant\n"),
        ],
    );
    link(&a, ".git/hooks", "../tools/githooks");
    let got = run(
        Env { root: a.clone(), ..Env::default() },
        &[("R", &a)],
        &format!(
            "{UNDO}{}",
            r#"
        t other-case 'mv "$R/TOOLS" "$R/toolsx"'
        u tools
        t renamed 'mv "$R/tools" "$R/toolsx"'
        u tools
        t linked 'mv "$R/tools" "$R/toolsx" && ln -s evil "$R/tools"'
        u tools
        t new-folder 'mv "$R/tools" "$R/toolsx" && mkdir -p "$R/tools/githooks" && printf x > "$R/tools/githooks/pre-commit"'
        u tools
        t removed 'rm -r "$R/tools"'
        t inside 'printf x >> "$R/tools/x"'
        "#
        ),
    );
    let want = ["other-case no", "renamed no", "linked no", "new-folder no", "removed no", "inside ok"];
    assert_eq!(got, want, "the way `tools`");
    assert_eq!(read(&a.join(".git/hooks/pre-commit")), "orig\n");
    assert!(a.join("tools").symlink_metadata().unwrap().is_dir() && !a.join("toolsx").exists());
    assert_eq!(read(&a.join("tools/x")), "x");
    // `gitdir: sub/gitdata`
    let b = t.join("b");
    files(
        &b,
        &[
            (".git", "gitdir: sub/gitdata\n"),
            ("sub/gitdata/HEAD", "orig\n"),
            ("sub/x", ""),
            ("evil/gitdata/HEAD", "plant\n"),
        ],
    );
    let got = run(
        Env { root: b.clone(), ..Env::default() },
        &[("R", &b)],
        &format!(
            "{UNDO}{}",
            r#"
        t other-case 'mv "$R/SUB" "$R/subx"'
        u sub
        t renamed 'mv "$R/sub" "$R/subx"'
        u sub
        t linked 'mv "$R/sub" "$R/subx" && ln -s evil "$R/sub"'
        u sub
        t new-folder 'mv "$R/sub" "$R/subx" && mkdir -p "$R/sub/gitdata" && printf x > "$R/sub/gitdata/HEAD"'
        u sub
        t removed 'rm -r "$R/sub"'
        t inside 'printf x >> "$R/sub/x"'
        "#
        ),
    );
    assert_eq!(got, want, "the way `sub`");
    assert_eq!(read(&b.join("sub/gitdata/HEAD")), "orig\n");
    assert!(b.join("sub").symlink_metadata().unwrap().is_dir() && !b.join("subx").exists());
    assert_eq!(read(&b.join("sub/x")), "x");
    // A dangling `.git/hooks`: its way not there yet — and a `.git/config` whose way's missing step is in a folder
    // holding nothing kept (`deep/b`, `deep` there: where Linux's grants would open `deep` whole but for the marks)
    let c = t.join("c");
    files(
        &c,
        &[(".git/HEAD", "ref: refs/heads/main\n"), ("evil/githooks/pre-commit", "plant\n"), ("deep/x", "")],
    );
    link(&c, ".git/hooks", "../tools/githooks");
    link(&c, ".git/config", "../deep/b/config");
    let got = run(
        Env { root: c.clone(), ..Env::default() },
        &[("R", &c)],
        r#"
        t linked 'ln -s evil "$R/tools"'
        t made 'mkdir "$R/tools"'
        t renamed-onto 'mkdir "$R/e" && mv "$R/e" "$R/tools"'
        t deep-made 'mkdir "$R/deep/b"'
        t deep-linked 'ln -s ../evil "$R/deep/b"'
        t deep-file 'printf x >> "$R/deep/x"'
        "#,
    );
    let want = ["linked no", "made no", "renamed-onto no", "deep-made no", "deep-linked no", "deep-file ok"];
    assert_eq!(got, want, "a way not there yet");
    assert!(c.join("tools").symlink_metadata().is_err() && !c.join(".git/hooks/pre-commit").exists());
    assert!(c.join("deep/b").symlink_metadata().is_err());
}

/// A folder on a way that holds only a mark (`deep`, for a `.git/config` leading through `deep/b`, not there yet) takes no
/// new file on Linux, as a folder holding a protected name doesn't — and the report gives that reason, from the run's
/// output and from Landlock's record of the folder alike, never a bare "writing deep". The folders are
/// `shut_by_marks`'s (every system); for real on Linux. Broken by the marks' folders left out of the words.
#[test]
fn a_folder_on_a_git_folders_way_says_why_it_takes_no_new_file() {
    let (_s, t) = scratch("sb-gitmarked");
    files(&t, &[(".git/HEAD", "ref: refs/heads/main\n"), ("deep/x", "")]);
    link(&t, ".git/config", "../deep/b/config");
    let env = || Env { root: t.clone(), ..Env::default() };
    let p = policy_of(env());
    assert_eq!(p.git.marks, [t.join("deep/b")]);
    assert_eq!(shut_by_marks(&p.git.marks, std::slice::from_ref(&t)), [t.clone(), t.join("deep")]);
    #[cfg(target_os = "linux")]
    {
        use crate::sandbox::{Denial, Here, RunDir, confined, run_env_of};
        if skipped("a_folder_on_a_git_folders_way_says_why_it_takes_no_new_file") {
            return;
        }
        let got = run(env(), &[("R", &t)], "t deep-new 'printf x > \"$R/deep/new\"'\n");
        assert_eq!(got, ["deep-new no"]);
        let tmp = Some(profile_path(&std::env::temp_dir()));
        let run_env = run_env_of(&Here::now(), &crate::llm::LlmConfig::default());
        let p = confined(Policy { run: RunDir::new().ok(), ..policy_of(Env { tmp, run_env, ..env() }) });
        let why = "a folder holding protected names takes no new files on Linux";
        // (from the output, as `sh` says it)
        let line = format!("sh: {}: Permission denied", t.join("deep/new").display());
        let said = p.said_in_output(&[(true, line)], &[]);
        assert!(said.iter().any(|d| d.plain().contains(why)), "{said:?}");
        // (from Landlock's record, which names only the folder)
        let deep = t.join("deep").display().to_string();
        let mut seen = [Denial { process: "sh".into(), op: "file-make-in".into(), target: deep }];
        p.merge_made(&mut seen, Vec::new());
        assert!(seen[0].plain().contains(why), "{}", seen[0].plain());
    }
}

/// Where the project is git's folder itself (`gitdir: .`), for real on both systems: its `hooks` and `config` are
/// kept (srt's set), the rest of it written (`objects/`: the threat model's "Not kept"), and the rules say so.
/// Broken by the two paths left out.
#[test]
fn a_git_folder_where_the_git_dir_is_the_project_keeps_its_hooks_and_config() {
    let (_s, t) = scratch("sb-gitbare");
    let config = "[core]\n\tbare = true\n";
    files(
        &t,
        &[
            (".git", "gitdir: .\n"),
            ("HEAD", "ref: refs/heads/main\n"),
            ("config", config),
            ("hooks/README.sample", "x"),
        ],
    );
    std::fs::create_dir_all(t.join("objects")).unwrap();
    let p = policy_of(Env { root: t.clone(), ..Env::default() });
    assert_eq!(p.git.git, [t.join("hooks"), t.join("config")]);
    assert_eq!(p.git.bare, std::slice::from_ref(&t));
    let rules = crate::sandbox::Guard::In(Box::new(p)).rules();
    assert!(rules.contains(", nor git's hooks and config (your edits change those)"), "{rules}");
    if skipped("a_git_folder_where_the_git_dir_is_the_project_keeps_its_hooks_and_config") {
        return;
    }
    let got = run(
        Env { root: t.clone(), ..Env::default() },
        &[("R", &t)],
        r#"
        t hook 'printf x > "$R/hooks/pre-commit"'
        t config 'printf x >> "$R/config"'
        t objects 'printf x > "$R/objects/x"'
        "#,
    );
    assert_eq!(got, ["hook no", "config no", "objects ok"]);
    assert!(!t.join("hooks/pre-commit").exists() && read(&t.join("config")) == config);
}

/// The reader's bound is the same on every disk: a `.git` folder of 70 links made in reverse name order keeps the
/// first 64 by name, the same on two reads, and so the same policy key; past 256 entries it follows none and says
/// so. Broken by `read_dir`'s own order.
#[test]
fn a_git_folders_links_are_followed_to_a_bound_in_name_order() {
    let (_s, t) = scratch("gf-bound");
    for i in (0..70).rev() {
        let n = format!("l{i:02}");
        std::fs::create_dir_all(t.join("t").join(&n)).unwrap();
        link(&t, &format!(".git/{n}"), &format!("../t/{n}"));
    }
    let env = || Env { root: t.clone(), ..Env::default() };
    let (one, two) = (git_found(&env()), git_found(&env()));
    let kept: Vec<PathBuf> = one.found.iter().map(|f| f.target.clone()).collect();
    let want: Vec<PathBuf> = (0..LINKS_MAX).map(|i| t.join(format!("t/l{i:02}"))).collect();
    assert_eq!(kept, want);
    assert_eq!(one, two);
    assert_eq!(policy_key(&policy_of(env())), policy_key(&policy_of(env())));
    // (the 64 share one way: `t`)
    assert_eq!(policy_of(env()).git.locks(), [t.join("t")]);
    // Past `ENTRIES_MAX`: none followed, said
    for i in 0..ENTRIES_MAX + 1 - 70 {
        files(&t, &[(&format!(".git/f{i:03}"), "x")]);
    }
    let cut = git_found(&env());
    assert!(cut.found.is_empty() && cut.cut == [t.join(".git")], "{cut:?}");
    let rules = crate::sandbox::Guard::In(Box::new(policy_of(env()))).rules();
    assert!(
        rules.contains(" (.git holds more than 256 entries: the links in it aren't followed)"),
        "{rules}"
    );
}

/// `llm.sandbox-protect`'s parents, for real on both systems: with `a/b` protected, `a` can't be renamed away nor
/// a link put in its place (macOS let both before #609: the profile kept `a/b` by its path alone). Broken by the
/// protect entries left out of the ways (macOS fails, Linux passes, as before).
#[test]
fn a_protected_paths_folders_are_kept_as_a_git_folders_way() {
    if skipped("a_protected_paths_folders_are_kept_as_a_git_folders_way") {
        return;
    }
    let (_s, t) = scratch("sb-protectway");
    files(&t, &[("a/b/f", "orig\n"), ("a/c", "")]);
    let env = Env { root: t.clone(), protect: vec!["a/b".into()], ..Env::default() };
    let got = run(
        env,
        &[("R", &t)],
        r#"
        t renamed 'mv "$R/a" "$R/a2"'
        t linked 'mv "$R/a" "$R/a2" && ln -s a2 "$R/a"'
        t inside 'printf x >> "$R/a/c"'
        "#,
    );
    assert_eq!(got, ["renamed no", "linked no", "inside ok"]);
    assert!(t.join("a").symlink_metadata().unwrap().is_dir() && read(&t.join("a/b/f")) == "orig\n");
}

/// What a refused write under git's folder by another name says, and the rules: the folder named, as the run's
/// panel and Claude read it.
#[test]
fn a_git_folder_kept_by_another_name_is_named_in_the_report() {
    use crate::sandbox::{Denial, Guard, Told, Watch};
    let (_s, t) = scratch("gf-words");
    files(
        &t,
        &[
            (".git", "gitdir: gitdata\n"),
            ("gitdata/HEAD", "ref: refs/heads/main\n"),
            ("gitdata/hooks/x.sample", "x"),
        ],
    );
    let p = policy_of(Env { root: t.clone(), ..Env::default() });
    assert_eq!(p.git.git, [t.join("gitdata")]);
    let w = Watch::quiet("gf-words.");
    let target = t.join("gitdata/hooks/x").display().to_string();
    w.record(Denial { process: "sh".into(), op: "file-write-create".into(), target });
    let g = Guard::In(Box::new(Policy { watch: Some(w), ..p }));
    let said = g.report_with(&[], Told::default(), false).text;
    let line = "Git's own folder, here gitdata, is kept from your commands: committing, staging, switching branches \
                and the like are the user's (their git tab) — ask them, or ask to run it outside the sandbox \
                (outside_sandbox, with your reason).\n";
    assert!(said.contains(line), "{said}");
    let rules = g.rules();
    assert!(
        rules.contains(", anywhere in it, nor git's folder gitdata (your edits change those)"),
        "{rules}"
    );
}

/// The resolver walks the text step by step, as the kernel does: each folder, link and missing step on the way
/// recorded, `..` from where the walk is, a link's own text followed; a loop gives up. And the kept rule: the way
/// where runs write is locked, a link and a missing step marked (Linux), past the cap kept but not locked.
#[test]
fn a_git_folders_way_is_every_step_its_text_takes() {
    let (_s, t) = scratch("gf-resolve");
    std::fs::create_dir_all(t.join("x/y")).unwrap();
    link(&t, "l", "x/y");
    let (to, way) = resolve(&t, Path::new("l/../y/z/w")).unwrap();
    assert_eq!(to, t.join("x/y/z/w"));
    let want = [
        (t.join("l"), Step::Link),
        (t.join("x"), Step::Folder),
        (t.join("x/y"), Step::Folder),
        (t.join("x/y/z"), Step::Missing),
    ];
    assert_eq!(way, want);
    link(&t, "loop", "loop");
    assert_eq!(resolve(&t, Path::new("loop/a")), None);
    let read = Read {
        found: vec![found_at(&t, Path::new("l/../y/z/w"), true, &mut Memo::default()).unwrap()],
        cut: Vec::new(),
    };
    let places = std::slice::from_ref(&t);
    let k = git_kept(&read, places, places, GIT_LOCKS_MAX);
    assert_eq!(k.ways, [(t.join("x/y/z/w"), want.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>())]);
    assert_eq!(k.marks, [t.join("l"), t.join("x/y/z")]);
    // Past the cap: kept, not locked
    let k = git_kept(&read, places, places, 3);
    assert_eq!((k.protect.len(), k.ways.len()), (1, 0));
    assert_eq!(k.unlocked, [t.join("x/y/z/w")]);
    assert!(k.held(false).is_empty() && k.held(true).len() == 1);
}

/// The policy of a project at `t` as macOS's walk before a run reads it (any system: the rule's computation), with no
/// other write place.
fn walked_policy(t: &Path) -> Policy {
    let mut p = policy_of(Env { root: t.to_path_buf(), ..Env::default() });
    (p.keys, p.supervised, p.write) = (Some(Vec::new()), false, Vec::new());
    p
}

/// #609 L2, for real on both systems: a checkout below the root with its own git folder — `sub/.git` naming
/// `gitdata` (its way `sub`: moved, the folder moves with it) and `vendor/x/.git/hooks -> ../../../tools/h` (its way
/// `tools`) — is kept from runs, found before the run (macOS's walk, Linux's `there`), its way locked: writes to
/// either refused, `sub` and `tools` not renamed nor a link put in their place, git's hooks read as they were; a file
/// in `sub` or `tools` written as ever. Broken by the nested checkouts not read (macOS's walk, Linux's `there`), and
/// on macOS by the checkout's own folders left off the way (`sub-renamed`, `sub-moved-back` land).
#[test]
fn a_nested_checkouts_git_folder_is_kept_from_runs_and_its_way_locked() {
    const UNDO: &str =
        "u() { [ -e \"$R/$1x\" ] && { rm -rf \"$R/$1\"; mv \"$R/$1x\" \"$R/$1\"; }; } 2>/dev/null\n";
    if skipped("a_nested_checkouts_git_folder_is_kept_from_runs_and_its_way_locked") {
        return;
    }
    let (_s, t) = scratch("sb-gitnested");
    files(
        &t,
        &[
            (".git/HEAD", "ref: refs/heads/main\n"),
            ("sub/.git", "gitdir: gitdata\n"),
            ("sub/gitdata/HEAD", "ref: refs/heads/main\n"),
            ("sub/gitdata/hooks/pre-commit", "orig\n"),
            ("sub/x", ""),
            ("vendor/x/.git/HEAD", "ref: refs/heads/main\n"),
            ("tools/h/pre-commit", "orig\n"),
            ("tools/y", ""),
            ("evil/pre-commit", "plant\n"),
        ],
    );
    link(&t, "vendor/x/.git/hooks", "../../../tools/h");
    let got = run(
        Env { root: t.clone(), ..Env::default() },
        &[("R", &t)],
        &format!(
            "{UNDO}{}",
            r#"
        t sub-hook 'printf x > "$R/sub/gitdata/hooks/pre-commit"'
        t sub-renamed 'mv "$R/sub" "$R/subx"'
        t sub-moved-back 'printf x > "$R/subx/gitdata/hooks/pre-commit"'
        u sub
        t tools-hook 'printf x > "$R/tools/h/pre-commit"'
        t tools-renamed 'mv "$R/tools" "$R/toolsx"'
        u tools
        t tools-linked 'mv "$R/tools" "$R/toolsx" && ln -s evil "$R/tools"'
        u tools
        t sub-inside 'printf x >> "$R/sub/x"'
        t tools-inside 'printf x >> "$R/tools/y"'
        "#
        ),
    );
    let want = [
        "sub-hook no",
        "sub-renamed no",
        "sub-moved-back no",
        "tools-hook no",
        "tools-renamed no",
        "tools-linked no",
        "sub-inside ok",
        "tools-inside ok",
    ];
    assert_eq!(got, want);
    assert_eq!(read(&t.join("sub/gitdata/hooks/pre-commit")), "orig\n");
    assert_eq!(read(&t.join("tools/h/pre-commit")), "orig\n");
}

/// #609 L2: a refused write under a nested checkout's git folder kept before the run says so as `.git`'s does, naming
/// it, and the rules name it (macOS's walk; any system). Broken by the refusal or the rules reading the policy's alone.
#[test]
fn a_nested_git_folder_refused_is_named_in_the_report() {
    use crate::sandbox::{Denial, Guard, Told, Watch};
    let (_s, t) = scratch("gf-nested-words");
    files(
        &t,
        &[("sub/.git", "gitdir: gitdata\n"), ("sub/gitdata/HEAD", ""), ("sub/gitdata/hooks/x.sample", "x")],
    );
    let mut p = walked_policy(&t);
    p.key_folders = crate::sandbox::keyfolders::walk(&p, crate::sandbox::keyfolders::CAPS);
    let w = Watch::quiet("gf-nested-words.");
    let target = t.join("sub/gitdata/hooks/x").display().to_string();
    w.record(Denial { process: "sh".into(), op: "file-write-create".into(), target });
    let g = Guard::In(Box::new(Policy { watch: Some(w), ..p }));
    let said = g.report_with(&[], Told::default(), false).text;
    assert!(said.contains("Git's own folder, here sub/gitdata, is kept from your commands"), "{said}");
    let rules = g.rules();
    assert!(rules.contains(", nor git's folder sub/gitdata (your edits change those)"), "{rules}");
}

/// #609 L2: a nested checkout the walk before a run doesn't reach (past its cap, lowered to 8 entries here) isn't
/// found, and the rules name the folder not looked through and say a checkout's git folder there isn't kept; one it
/// reaches is (`a/gitdata`). macOS's walk (its rule computed on any system). Broken by the cut's words left as they
/// were.
#[test]
fn a_nested_checkout_past_the_walks_cap_is_named() {
    use crate::sandbox::keyfolders::{CAPS, Caps, walk};
    let (_s, t) = scratch("gf-nested-cut");
    files(
        &t,
        &[
            ("a/.git", "gitdir: gitdata\n"),
            ("a/gitdata/HEAD", ""),
            ("a/gitdata/hooks/x", ""),
            ("z/deep/deeper/c/.git", "gitdir: gitdata\n"),
            ("z/deep/deeper/c/gitdata/HEAD", ""),
        ],
    );
    let p = walked_policy(&t);
    let found = walk(&p, Caps { entries: 8, ..CAPS });
    assert_eq!(found.git.git, [t.join("a/gitdata")]);
    let c = t.join("z/deep/deeper/c");
    let u =
        found.unwalked.iter().find(|u| c.starts_with(u)).unwrap_or_else(|| panic!("{:?}", found.unwalked));
    let said = found.said();
    assert!(said.contains(&format!("{}/", crate::allow::tilde(u))), "{said}");
    assert!(said.contains("nor is a checkout's git folder found there kept"), "{said}");
}

/// #609 L2: a git folder a protected name holds where it's kept is the name's — a submodule's `gitdir:` into the
/// root's `.git/modules/m` — so neither kept again nor locked (no `G`, no `GD`: nothing of the cap spent); a nested
/// checkout's own folder by another name is. Broken by the rule's "held by a name" left out.
#[test]
fn a_submodules_git_folder_is_held_by_its_name() {
    let (_s, t) = scratch("gf-submodule");
    files(
        &t,
        &[
            (".git/HEAD", ""),
            (".git/modules/m/HEAD", ""),
            ("m/.git", "gitdir: ../.git/modules/m\n"),
            ("n/.git", "gitdir: gitdata\n"),
            ("n/gitdata/HEAD", ""),
        ],
    );
    let found = crate::sandbox::keyfolders::walk(&walked_policy(&t), crate::sandbox::keyfolders::CAPS);
    assert_eq!(found.git.git, [t.join("n/gitdata")]);
    assert_eq!(found.git.locks(), [t.join("n")]);
}

/// #609 L2's work budget, the same on any machine (CI's gate): a `repo` top of 300 checkouts read before a run looks
/// at each path once — one `lstat` a path (the memo), 7,527 of them here, where a step at a time made 21,430 (L2's
/// measure). Broken by the memo bypassed.
#[test]
fn a_repo_tops_git_folders_are_read_once_a_path() {
    let (_s, t) = scratch("gf-repo-reads");
    let checkouts = repo_top(&t, 300);
    let mut memo = Memo::default();
    let read = nested_read(&checkouts, &mut memo);
    assert_eq!(read.found.len(), 3_000, "10 a checkout");
    assert_eq!(memo.lstats, memo.looked.len(), "one lstat a path");
    assert!(memo.lstats <= 8_000, "{} lstats", memo.lstats);
}

/// #609 L2 at a top of 300 checkouts whose git folders are all in one store, by macOS's walk (its rule computed on
/// any system):
/// - `.repo`, a protected name (`REPO_NAMED`, the user's word): every checkout's git folders are held by the name,
///   nothing kept by the walk (no `G`, no `GD`: none of the cap spent), nothing said;
/// - a store no name holds (`.store`): the walk keeps what its cap holds — 32 git folders, three checkouts whole,
///   their ways past it — and the rules say the rest in counts: at most three folders named, then how many more, one
///   clause each past the cap, never a list; `lock_said` gives the count.
///
/// Broken by `G` past the cap, the count clauses as a list, `lock_said` without git.
#[test]
fn a_repo_tops_git_folders_are_kept_or_said() {
    let walked = |store: &str| {
        let (s, t) = scratch("gf-repo-kept");
        let checkouts = repo_top_in(&t, 300, store);
        let p = walked_policy(&t);
        let found = crate::sandbox::keyfolders::walk(&p, crate::sandbox::keyfolders::CAPS);
        let words = said(&[&p.git, &found.git], &t, false);
        (s, t, checkouts, found, words)
    };
    let (_s, t, _, found, words) = walked(".repo");
    assert_eq!(REPO_NAMED, crate::sandbox::protected_in(&t, &t.join(".repo")), "REPO_NAMED is the name's");
    if REPO_NAMED {
        assert_eq!(found.git, GitKept::default());
        assert_eq!(words, "");
    }
    let (_s, t, checkouts, found, words) = walked(".store");
    let whole = checkouts
        .iter()
        .filter(|c| {
            let rel = c.strip_prefix(&t).unwrap().display().to_string();
            found.git.git.contains(&t.join(format!(".store/projects/{rel}.git/svn")))
        })
        .count();
    assert_eq!((found.git.git.len(), whole), (GIT_LOCKS_MAX, 3), "{:?}", found.git.git);
    assert_eq!((found.git.dropped.len(), found.git.unlocked.len()), (3_000 - GIT_LOCKS_MAX, GIT_LOCKS_MAX));
    assert!(found.git.locks().is_empty());
    assert!(
        words.starts_with(", nor git's folders .store/projects/") && words.contains(" and 29 more"),
        "{words}"
    );
    assert!(words.ends_with(
        " (32 git folders' ways aren't locked: past the cap) (2968 nested git folders aren't kept: past the cap)"
    ));
    assert!(words.len() <= 260, "{} characters: {words}", words.len());
    let lock = found.lock_said();
    assert!(lock.contains("3000 git folders here"), "{lock}");
}

/// #609 L2, for real on both systems: at a `repo` top of 300 checkouts every checkout's hooks are refused to a run —
/// by the name `.repo` (`REPO_NAMED`; Linux's keeper keeps them all without it too) — and so is `.repo/repo/`, `repo`'s
/// own Python, which the user runs outside the sandbox: a new file there and its `main.py` changed. The refusal says
/// why. Broken by the name left out of the tools pack (macOS: every line lands).
#[test]
fn a_repo_tops_hooks_are_refused_to_runs() {
    if skipped("a_repo_tops_hooks_are_refused_to_runs") {
        return;
    }
    let (_s, t) = scratch("sb-repo-top");
    let checkouts = repo_top(&t, 300);
    let rels: Vec<String> =
        checkouts.iter().map(|c| c.strip_prefix(&t).unwrap().display().to_string()).collect();
    let mut lines: String =
        rels.iter().map(|r| format!("t {r} 'printf x > \"$R/.repo/projects/{r}.git/hooks/x\"'\n")).collect();
    lines += "t repo-tool-new 'printf x > \"$R/.repo/repo/x\"'\n";
    lines += "t repo-tool 'printf x >> \"$R/.repo/repo/main.py\"'\n";
    let got = run(Env { root: t.clone(), ..Env::default() }, &[("R", &t)], &lines);
    let mut want: Vec<String> = rels.iter().map(|r| format!("{r} no")).collect();
    // (without the name: on Linux without seals a folder holding a protected name — `.repo/repo/.git` — takes no new
    // file, but its files are written)
    let new_kept = REPO_NAMED || cfg!(target_os = "linux");
    want.push(format!("repo-tool-new {}", if new_kept { "no" } else { "ok" }));
    want.push(format!("repo-tool {}", if REPO_NAMED { "no" } else { "ok" }));
    assert_eq!(got, want);
    assert!(!t.join(".repo/repo/x").exists() && read(&t.join(".repo/repo/main.py")).is_empty());
}

/// #609 L2: a write refused under `.repo` says why — `repo`'s folder, kept as git's own is, one that isn't `repo`'s
/// too — as one under `.git` does, never a bare protected name; and the rules list the name. Broken by the words'
/// arm left out.
#[test]
fn a_write_refused_under_repo_says_why() {
    use crate::sandbox::{Denial, Guard, Told, Watch};
    let (_s, t) = scratch("gf-repo-words");
    files(&t, &[(".repo/repo/main.py", "")]);
    let w = Watch::quiet("gf-repo-words.");
    let target = t.join(".repo/projects/p.git/hooks/x").display().to_string();
    w.record(Denial { process: "sh".into(), op: "file-write-create".into(), target });
    let g = Guard::In(Box::new(Policy {
        watch: Some(w),
        ..policy_of(Env { root: t.clone(), ..Env::default() })
    }));
    let said = g.report_with(&[], Told::default(), false).text;
    assert!(
        said.contains(
            ".repo is kept from your commands as git's own folder is: it's where Android's repo keeps its \
             checkouts' git folders and its own code"
        ) && said.contains("A folder named .repo that isn't repo's is kept all the same."),
        "{said}"
    );
    assert!(g.rules().contains(".repo"), "{}", g.rules());
}

/// L2's measure, `cargo perf`: a top of 300 checkouts whose git folders are in a store no name holds (`.store`, the
/// worst case) — read before a run (`nested_read`, one memo) and kept (`nested_kept`: macOS's cap, and Linux's every
/// one) — and the rule at a `repo` top, where `.repo` holds them all; median and p90 of 11, warm. What it keeps fast:
/// L1's rule scanned lists, 9.8 s at 300 (`under_any`'s sorted lookups make it milliseconds). Budgets: the reading
/// 10 ms on Linux (macOS reports it), the rule 5 ms on both.
#[test]
fn perf_a_repo_tops_git_folders() {
    if crate::test_results::unoptimized("sandbox::gitfolders::tests::perf_a_repo_tops_git_folders") {
        return;
    }
    let (_s, t) = scratch("gf-repo-perf");
    let checkouts = repo_top_in(&t, 300, ".store");
    let p = walked_policy(&t);
    let (kept_in, writes) = p.git_places();
    let read = nested_read(&checkouts, &mut Memo::default());
    let time = |f: &dyn Fn()| {
        f();
        let mut ms: Vec<f64> = (0..11)
            .map(|_| {
                let at = std::time::Instant::now();
                f();
                at.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        ms.sort_by(f64::total_cmp);
        (ms[5], ms[9])
    };
    let reads = time(&|| drop(std::hint::black_box(nested_read(&checkouts, &mut Memo::default()))));
    let kept = time(&|| {
        drop(std::hint::black_box(nested_kept(&read, &kept_in, &writes, &[], Some(GIT_LOCKS_MAX))));
    });
    // (Linux's: all 3,000 kept, every way's path looked up among them — where a scan was quadratic)
    let all = time(&|| drop(std::hint::black_box(nested_kept(&read, &kept_in, &writes, &[], None))));
    // (a `repo` top: the name holds every target)
    let (_r, r) = scratch("gf-repo-perf-named");
    let named_read = nested_read(&repo_top(&r, 300), &mut Memo::default());
    let (r_in, r_writes) = walked_policy(&r).git_places();
    let named = time(&|| {
        drop(std::hint::black_box(nested_kept(&named_read, &r_in, &r_writes, &[], Some(GIT_LOCKS_MAX))));
    });
    let reads_budget = if cfg!(target_os = "linux") { 10.0 } else { 1000.0 };
    let mut out = String::new();
    for (name, (p50, p90), budget) in [
        ("repo_top_300_git_reads_ms", reads, reads_budget),
        ("repo_top_300_git_kept_ms", kept, 5.0),
        ("repo_top_300_git_kept_all_ms", all, 5.0),
        ("repo_top_300_git_kept_named_ms", named, 5.0),
    ] {
        out += &format!("PERF {name} {p50:.2} budget={budget} noise=30\n({name}: p90 {p90:.2} ms)\n");
        assert!(p50 <= budget, "{name}: {p50:.2} ms at the median, over {budget} ms");
    }
    let _ = std::io::Write::write_all(&mut std::io::stderr(), out.as_bytes());
}
