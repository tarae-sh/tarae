//! #521: with `CARGO_HOME` moved, the rust pack reads cargo's home where it is — never `~/.cargo` whole. Its
//! token files stay closed, home's `.cargo/config.toml` is read (and told) only as an ancestor's config of a
//! project under home, rustup's proxies in `~/.cargo/bin` run through `PATH` as the policy's `Here` says it, and
//! a read refused at the default place says where the home is. A stand-in home of the test's own, fake tokens
//! (`concat!` in two parts); the live runs skip themselves where no sandbox starts (`live`).

use std::path::{Path, PathBuf};

use super::*;

/// A token-like line no provider's format matches.
const TOKEN_LINE: &str = concat!("token = \"tarae-fake-", "token-cargo-0521\"\n");

/// A stand-in home, cargo's and rustup's homes moved out of it, and a project under it or outside it.
struct Moved {
    _scratch: crate::scratch::Scratch,
    base: PathBuf,
    home: PathBuf,
    root: PathBuf,
    here: Here,
}

impl Moved {
    /// `under`: the project in home (`~/work/proj`), else beside it (`$BASE/elsewhere/proj`). `path`: `PATH` as
    /// the policy's `Here` says it, `{home}` and `{cargo}` (the toolchain's binaries, `$CARGO`'s folder) filled.
    fn new(name: &str, under: bool, path: &str) -> Moved {
        // (never in the shared temp folder — Linux's is `/tmp`: a run writes all of it)
        let tmp = std::env::temp_dir();
        let tmp = if tmp.starts_with(TMP) { PathBuf::from("/var/tmp") } else { tmp };
        let scratch = crate::scratch::Scratch::in_dir(&tmp, &format!("moved-{name}"));
        let base = std::fs::canonicalize(&*scratch).unwrap();
        let home = base.join("home");
        let cfg = format!("[registries.x]\nindex = \"https://example.invalid/index\"\n{TOKEN_LINE}");
        let files = [
            (".cargo/credentials.toml", format!("[registry]\n{TOKEN_LINE}")),
            (".cargo/config.toml", cfg),
            (".cargo/registry/src/index/x-1.0/lib.rs", "// x\n".into()),
            (".cargo/bin/cargo", "#!/bin/sh\necho stand-in cargo\n".into()),
        ];
        for (rel, text) in files {
            let p = home.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
        }
        let bin = home.join(".cargo/bin/cargo");
        std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        for d in ["cargo-moved/registry", "rustup-moved/toolchains", "tmp"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        let root = if under { home.join("work/proj") } else { base.join("elsewhere/proj") };
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        let cargo_dir = cargo().and_then(|c| c.parent().map(Path::to_path_buf)).unwrap_or_default();
        let path = path.replace("{home}", &home.display().to_string());
        let path = path.replace("{cargo}", &cargo_dir.display().to_string());
        let mut env: HashMap<String, std::ffi::OsString> = HashMap::new();
        env.insert("HOME".into(), home.clone().into());
        env.insert("CARGO_HOME".into(), base.join("cargo-moved").into());
        env.insert("RUSTUP_HOME".into(), base.join("rustup-moved").into());
        env.insert("PATH".into(), path.into());
        // (what this machine is, as a run's policy reads it; its variables and temp folder the stand-in's)
        let here = Here { env, tmp: Some(base.join("tmp")), ..Here::now() };
        // (no toolchain is asked: nothing found)
        toolchain_found_in(&root, ToolDirs::default(), &here);
        Moved { _scratch: scratch, base, home, root, here }
    }

    fn policy(&self) -> Policy {
        run_policy_in(&self.root, &crate::llm::LlmConfig::default(), &[], &self.here).expect("a run's policy")
    }

    /// What `script` prints run in `p` (from the project, `$H` the home, `PATH` the policy's).
    fn run(&self, p: &Policy, script: &str) -> String {
        let mut cmd = command(Some(p), "/bin/sh", &["-c".into(), script.into()]);
        let path = self.here.env.get("PATH").cloned().unwrap_or_default();
        cmd.current_dir(&self.root).env("H", &self.home).env("PATH", path);
        cmd.env("CARGO_HOME", self.base.join("cargo-moved"))
            .env("RUSTUP_HOME", self.base.join("rustup-moved"));
        // (Linux: the rules made, as a run's are — the policy alone has none)
        #[cfg(target_os = "linux")]
        let p = &confined(p.clone());
        #[cfg(target_os = "linux")]
        assert!(matches!(p.confine, Some(Ok(_))), "{:?}", p.confine);
        let spawned = spawn(Some(p), &cmd).unwrap();
        let mut out = String::new();
        std::io::Read::read_to_string(&mut spawned.stdout.unwrap(), &mut out).unwrap();
        crate::spawn::wait(spawned.pid).unwrap();
        out
    }
}

/// The cargo running these tests (`$CARGO`: the toolchain's own binary, no proxy).
fn cargo() -> Option<PathBuf> {
    std::env::var_os("CARGO").map(PathBuf::from).filter(|c| c.is_file())
}

/// Whether a live run starts here — else said, and the test's live part skipped: macOS inside a sandbox (Seatbelt
/// can't start), Linux without Landlock ABI 2.
fn live(test: &str) -> bool {
    let test = format!("sandbox::movedhome::{test}");
    #[cfg(target_os = "macos")]
    return !skipped_inside(&test);
    #[cfg(target_os = "linux")]
    match crate::landlock::short_of(2) {
        None => true,
        Some(why) => {
            crate::test_results::said_skip(&format!("{test} — {why}"));
            false
        }
    }
}

/// The read rule's line `r name path` prints: `name ok` or `name no`.
const READ: &str = r#"r() { if cat "$2" >/dev/null 2>&1; then echo "$1 ok"; else echo "$1 no"; fi; }"#;

/// T1: with `CARGO_HOME` moved, the default home's token file and its crates stay closed — cargo's home isn't
/// read whole there — while home's `.cargo/config.toml`, an ancestor's config of a project under home, is read
/// and told once.
#[test]
fn a_moved_cargo_homes_default_token_file_stays_closed() {
    let m = Moved::new("t1", true, "/usr/bin:/bin");
    let p = m.policy();
    let h = &m.home;
    assert!(!p.reads.allow.contains(&h.join(".cargo")), "{:?}", p.reads.allow);
    assert!(!p.reads.allow.contains(&h.join(".rustup")), "{:?}", p.reads.allow);
    for (rel, read) in [
        (".cargo/credentials.toml", false),
        (".cargo/credentials", false),
        (".cargo/registry/src/index/x-1.0/lib.rs", false),
        (".cargo/config.toml", true),
    ] {
        assert_eq!(p.reads_allow(&h.join(rel)), read, "{rel}");
    }
    let told: Vec<&PathBuf> = p.reads.copies.told.iter().map(|(f, _)| f).collect();
    assert_eq!(told, [&h.join(".cargo/config.toml")]);
    if !live("a_moved_cargo_homes_default_token_file_stays_closed") {
        return;
    }
    let out = m.run(
        &p,
        &format!(
            "{READ}\nr credentials \"$H/.cargo/credentials.toml\"; r registry \"$H/.cargo/registry/src/index/x-1.0/lib.rs\"\n\
             r config \"$H/.cargo/config.toml\""
        ),
    );
    assert_eq!(out.lines().collect::<Vec<_>>(), ["credentials no", "registry no", "config ok"]);
}

/// T2: rustup's proxies in `~/.cargo/bin`, on `PATH` as the policy's `Here` says it, run with cargo's home moved
/// — read and run, never written — and nothing else of that home opens with them.
#[test]
fn rustups_proxies_on_path_run_with_the_cargo_home_moved() {
    let m = Moved::new("t2", true, "{home}/.cargo/bin:/usr/bin:/bin");
    let p = m.policy();
    let (h, bin) = (&m.home, m.home.join(".cargo/bin"));
    assert!(p.reads_allow(&bin.join("cargo")));
    assert!(!p.writes_allow(&bin.join("cargo")) && !p.writes_allow(&bin.join("new")));
    assert!(!p.reads_allow(&h.join(".cargo/registry/src/index/x-1.0/lib.rs")));
    assert!(!p.reads_allow(&h.join(".cargo/credentials.toml")));
    if !live("rustups_proxies_on_path_run_with_the_cargo_home_moved") {
        return;
    }
    let out = m.run(
        &p,
        &format!(
            "{READ}\ncargo 2>/dev/null || echo cargo refused\n\
             r registry \"$H/.cargo/registry/src/index/x-1.0/lib.rs\"; r credentials \"$H/.cargo/credentials.toml\"\n\
             if ( : > \"$H/.cargo/bin/new\" ) 2>/dev/null; then echo write ok; else echo write no; fi"
        ),
    );
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        ["stand-in cargo", "registry no", "credentials no", "write no"]
    );
}

/// Real cargo's `metadata` in the project, in `p`: its status and what it said.
fn cargo_metadata(m: &Moved, p: &Policy) -> String {
    m.run(p, "cargo metadata --offline --no-deps --format-version 1 >/dev/null 2>\"$TMPDIR/err\" && echo cargo ok || { echo cargo failed; cat \"$TMPDIR/err\"; }")
}

/// T3: a project under home — real cargo, its home moved, reads home's `.cargo/config.toml` as an ancestor's
/// config (`GlobalContext::walk_tree`) and works: closed, it fails ("could not load Cargo configuration").
#[test]
fn cargo_reads_homes_config_as_an_ancestors_with_the_home_moved() {
    let test = "cargo_reads_homes_config_as_an_ancestors_with_the_home_moved";
    let m = Moved::new("t3", true, "{cargo}:/usr/bin:/bin");
    let p = m.policy();
    assert!(p.reads_allow(&m.home.join(".cargo/config.toml")));
    if cargo().is_none() {
        return crate::test_results::said_skip(&format!("sandbox::movedhome::{test} — no $CARGO"));
    }
    if !live(test) {
        return;
    }
    let out = cargo_metadata(&m, &p);
    assert!(out.starts_with("cargo ok"), "{out}");
}

/// T5: a project outside home (`/Users/Shared/…`, another disk) — cargo never reads home's `.cargo/config.toml`
/// there, so it stays closed and nothing is told, token and all; real cargo works without it.
#[test]
fn homes_cargo_config_stays_closed_for_a_project_outside_home() {
    let test = "homes_cargo_config_stays_closed_for_a_project_outside_home";
    let m = Moved::new("t5", false, "{cargo}:/usr/bin:/bin");
    let p = m.policy();
    assert!(!p.reads_allow(&m.home.join(".cargo/config.toml")));
    assert!(!p.reads_allow(&m.home.join(".cargo/config")));
    let told: Vec<&PathBuf> = p.reads.copies.told.iter().map(|(f, _)| f).collect();
    assert!(told.is_empty(), "{told:?}");
    if !live(test) {
        return;
    }
    let out = m.run(&p, &format!("{READ}\nr config \"$H/.cargo/config.toml\""));
    assert_eq!(out.trim_end(), "config no");
    if cargo().is_none() {
        return crate::test_results::said_skip(&format!("sandbox::movedhome::{test} — no $CARGO"));
    }
    let out = cargo_metadata(&m, &p);
    assert!(out.starts_with("cargo ok"), "{out}");
}

/// T4: a read refused at a moved home's default place says where the home is — in a run's report, on the read
/// card, in the refusal and in the answer — the home as `~` where it's under home; a write there, or the same
/// read with the home where it was, says nothing of it.
#[test]
fn a_read_at_a_moved_homes_default_says_where_the_home_is() {
    let m = Moved::new("t4", true, "/usr/bin:/bin");
    let h = &m.home;
    let crate_file = h.join(".cargo/registry/src/index/x-1.0/lib.rs");
    let shown = |p: &Path| p.display().to_string();
    let report = |p: &Policy, ops: &[(&str, PathBuf)]| {
        let watch = Watch::quiet("moved.");
        for (op, path) in ops {
            watch.record(Denial { process: "x".into(), op: op.to_string(), target: shown(path) });
        }
        Guard::In(Box::new(Policy { watch: Some(watch), ..p.clone() })).report(&[])
    };
    let at = crate::allow::tilde(&m.base.join("cargo-moved"));
    let said = |paths: &str| format!("{paths}: not cargo's home — CARGO_HOME puts it at {at}; read there.");
    let p = m.policy();
    let reads = [("file-read-data", crate_file.clone()), ("file-read-data", h.join(".cargo/git/db/y"))];
    let r = report(&p, &reads);
    let both =
        format!("{}, {}", crate::allow::tilde(&h.join(".cargo/git/db/y")), crate::allow::tilde(&crate_file));
    assert!(r.contains(&said(&both)), "{r}");
    // (more than a few: counted)
    let many: Vec<(&str, PathBuf)> = (0..5)
        .map(|i| ("file-read-data", h.join(format!(".cargo/registry/src/index/x-1.0/{i}.rs"))))
        .collect();
    assert!(report(&p, &many).contains(" and 2 more: not cargo's home"), "{}", report(&p, &many));
    // (a write, or a token file — kept, said as kept)
    let w = report(
        &p,
        &[("file-write-create", crate_file.clone()), ("file-read-data", h.join(".cargo/credentials.toml"))],
    );
    assert!(!w.contains("not cargo's home"), "{w}");
    // Unmoved: nothing of it
    let mut unmoved = m.here.clone();
    unmoved.env.remove("CARGO_HOME");
    unmoved.env.remove("RUSTUP_HOME");
    let q = run_policy_in(&m.root, &crate::llm::LlmConfig::default(), &[], &unmoved).unwrap();
    assert!(q.moved_homes.is_empty(), "{:?}", q.moved_homes);
    assert!(!report(&q, &reads).contains("not cargo's home"));
    // Claude's read: its card, a refusal and the answer after a `y` carry the same line
    let caps =
        crate::caps::Caps::of_defaults(crate::allow::defaults_in(&m.root, false, Some(h), &m.here), Some(h));
    let line = said(&crate::allow::tilde(&crate_file));
    assert_eq!(caps.moved_said(&crate_file).as_deref(), Some(line.as_str()));
    assert_eq!(caps.moved_said(&m.base.join("cargo-moved/registry/x")), None);
    let unmoved_caps =
        crate::caps::Caps::of_defaults(crate::allow::defaults_in(&m.root, false, Some(h), &unmoved), Some(h));
    assert_eq!(unmoved_caps.moved_said(&crate_file), None);
    let input = serde_json::json!({ "file_path": crate_file });
    let p = crate::reads::Pending {
        request_id: String::new(),
        generation: 0,
        input: input.clone(),
        look: crate::reads::look("Read", &input, &m.root),
        class: crate::cards::Class::Home,
        since: std::time::Instant::now(),
        found: Default::default(),
        repository: None,
        not_whole: None,
        own: None,
        then: None,
        log: false,
        moved: Some(line.clone()),
    };
    let (q, _) = crate::reads::card_text(&p, false, None, None);
    assert!(q.ends_with(&format!("\n\n{line}")), "{q}");
    assert_eq!(crate::reads::refusal(&p, "the user said no"), format!("the user said no\n{line}"));
}
