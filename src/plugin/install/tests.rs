//! The install as the user's card, and a crate started (PD1's H2).

use super::*;
use crate::scratch::Scratch;
use serde_json::json;

#[track_caller]
fn settle(ed: &mut Editor, done: impl Fn(&Editor) -> bool) {
    crate::test_results::settle_saying(ed, done, |ed| format!("{:?}", said(ed)));
}

fn said(ed: &Editor) -> Vec<String> {
    ed.messages.iter().map(|m| m.2.clone()).collect()
}

/// Call `n`'s answer, once it came.
fn answered(n: u64) -> Option<Result<String, String>> {
    crate::tools::ANSWERED.with(|a| a.borrow().iter().find(|(id, _)| *id == json!(n)).map(|(_, r)| r.clone()))
}

/// The build the tests install: the demo asking to hear keys, and nothing else (`abi::demo_asking`).
fn build() -> Vec<u8> {
    super::super::abi::demo_asking(b'k')
}

/// A project holding the build at `plug/demo/target/demo.wasm`, in its crate `plug/demo` (package `demo`); the plugins
/// folder a scratch one beside it; the editor on the project.
fn project(name: &str) -> (Editor, Scratch) {
    let s = Scratch::new(name);
    let root = s.join("p");
    std::fs::create_dir_all(root.join("plug/demo/target")).unwrap();
    std::fs::write(root.join("plug/demo/Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    std::fs::write(root.join("plug/demo/target/demo.wasm"), build()).unwrap();
    stand_in(&s.join("plugins"));
    let ed = Editor::with_root(crate::config::Config::default(), root);
    (ed, s)
}

const WASM: &str = "plug/demo/target/demo.wasm";

/// Claude's ask through the tools server: Some — answered at once.
fn ask(ed: &mut Editor, n: u64, args: Value) -> Option<Value> {
    let msg = json!({ "jsonrpc": "2.0", "id": n, "method": "tools/call", "params": { "name": "plugin", "arguments": args } });
    crate::tools::mcp(ed, 0, &format!("r{n}"), &msg)
}

fn install_up(ed: &Editor) -> bool {
    ed.offers.first().is_some_and(|o| matches!(o.what, What::PluginInstall(_)))
}

/// Claude's `plugin` with `wasm`: one card, Claude's (its call waits, never yours), saying where the build was found,
/// what it registers and may do in the load card's words, and what the yes binds — no hash. Its `y` installs the bytes
/// the card holds, not a rebuild since, approves those bytes — never a dev approval —, and the load finds them approved:
/// no load card.
#[test]
fn claudes_install_is_one_card_and_its_yes_approves_those_bytes() {
    let (mut ed, s) = project("install-yes");
    ed.plugins_detached();
    assert!(ask(&mut ed, 1, json!({ "wasm": WASM })).is_none(), "it waits on its card");
    settle(&mut ed, install_up);
    let o = &ed.offers[0];
    assert_eq!((o.kind(), o.yours, o.claudes()), ("plugin-install", false, true));
    assert_eq!(o.title, "Claude asks to install plugin demo");
    let sha = crate::sha256::hex(&build());
    let q = &o.question;
    assert!(q.starts_with("From plug/demo/target/demo.wasm (crate plug/demo)\nIts commands: :"), "{q}");
    let words = super::super::perm::describe(&Permission::Keys);
    assert!(q.contains(&format!("\nIt asks to:\n· {words}\n")), "in the load card's words: {q}");
    assert!(q.ends_with("\nFor this build only — any change asks again."), "{q}");
    assert!(!q.contains(&sha[..12]) && !q.contains("Replaces"), "no hash, nothing replaced: {q}");
    // One install of a name at a time: asked again while it's up, refused, naming it
    let id = ed.offers[0].id;
    assert!(ask(&mut ed, 3, json!({ "wasm": WASM })).is_none());
    settle(&mut ed, |_| answered(3).is_some());
    assert_eq!(answered(3), Some(Err(format!("an install of plugin demo is under way — card#{id}"))));
    assert_eq!(ed.offers.len(), 1);
    // A rebuild since the card: not what lands
    std::fs::write(ed.workspace.root.join(WASM), super::super::abi::DEMO).unwrap();
    ed.offer_accept();
    settle(&mut ed, |_| answered(1).is_some());
    assert!(answered(1).unwrap().unwrap().starts_with("the user said yes"), "{:?}", answered(1));
    assert_eq!(std::fs::read(s.join("plugins/demo.wasm")).unwrap(), build(), "the card's bytes");
    assert!(!s.join("plugins/.demo.wasm.tmp").exists());
    let a = ed.plugins.as_ref().unwrap().approvals["demo"].clone();
    assert_eq!((a.granted, a.sha256, a.dev), (vec!["keys".to_string()], sha.clone(), None));
    // The load that follows: approved, no card
    let declared = vec![Permission::Keys];
    let p = ed.plugins.as_mut().unwrap();
    p.permissions.insert("demo".into(), declared.clone());
    p.code.insert("demo".into(), Code { sha256: sha, dev: None });
    ed.plugin_permissions_loaded("demo", &declared);
    assert!(
        ed.plugin_approved("demo") && ed.offers.is_empty(),
        "no load card: {:?}",
        ed.offers.first().map(|o| &o.title)
    );
    // Asked again over it: it says what it replaces
    assert!(ask(&mut ed, 2, json!({ "wasm": WASM })).is_none());
    settle(&mut ed, install_up);
    assert!(
        ed.offers[0].question.contains("\nReplaces the demo plugin installed now.\n"),
        "{}",
        ed.offers[0].question
    );
}

/// `n` writes nothing and approves nothing; Claude's call is told no.
#[test]
fn no_installs_nothing() {
    let (mut ed, s) = project("install-no");
    ed.plugins_detached();
    assert!(ask(&mut ed, 1, json!({ "wasm": WASM })).is_none());
    settle(&mut ed, install_up);
    ed.offer_dismiss();
    assert_eq!(answered(1), Some(Ok("the user said no — nothing was installed".into())));
    assert!(!s.join("plugins").exists(), "nothing written");
    assert!(ed.plugins.as_ref().unwrap().approvals.is_empty() && ed.plugin_early.is_empty());
}

/// `:plugin-install` raises the same card, yours: no call waits on it.
#[test]
fn the_typed_install_is_the_same_card_yours() {
    let (mut ed, _s) = project("install-typed");
    assert_eq!(ed.plugin_install_typed("").unwrap_err(), "which build? :plugin-install <path to the .wasm>");
    ed.plugin_install_typed(WASM).unwrap();
    settle(&mut ed, install_up);
    let o = &ed.offers[0];
    assert_eq!((o.title.as_str(), o.yours_kind(), o.claudes()), ("Install plugin demo", true, false));
    assert!(o.question.ends_with("\nFor this build only — any change asks again."));
}

/// Nothing installed yet — no plugin thread: the yes waits for it to start and is given before any plugin loads.
#[test]
fn before_the_plugins_start_the_yes_waits_for_them() {
    let (mut ed, s) = project("install-early");
    assert!(ed.plugins.is_none());
    assert!(ask(&mut ed, 1, json!({ "wasm": WASM })).is_none());
    settle(&mut ed, install_up);
    ed.offer_accept();
    settle(&mut ed, |_| answered(1).is_some());
    assert!(s.join("plugins/demo.wasm").is_file());
    assert_eq!(ed.plugin_early.len(), 1, "kept for the start");
    ed.plugins_detached();
    assert!(ed.plugin_early.is_empty());
    let a = &ed.plugins.as_ref().unwrap().approvals["demo"];
    assert_eq!((a.sha256.as_str(), a.dev.as_ref()), (crate::sha256::hex(&build()).as_str(), None));
}

/// The bytes didn't land: the approval is taken back — the one before is what holds —, said, and Claude told why.
#[test]
fn a_failed_write_takes_the_approval_back() {
    let (mut ed, s) = project("install-fails");
    ed.plugins_detached();
    let before = Code { sha256: "older".into(), dev: None };
    ed.approvals_change(Change::Approve {
        plugin: "demo".into(),
        granted: vec!["network".into()],
        code: before,
    });
    // (the plugins folder a file: nothing can be written there)
    std::fs::write(s.join("plugins"), "").unwrap();
    assert!(ask(&mut ed, 1, json!({ "wasm": WASM })).is_none());
    settle(&mut ed, install_up);
    ed.offer_accept();
    settle(&mut ed, |_| answered(1).is_some());
    let e = answered(1).unwrap().unwrap_err();
    assert!(e.starts_with("Plugin demo not installed — writing it in "), "{e}");
    assert!(said(&ed).contains(&e), "{:?}", said(&ed));
    let a = &ed.plugins.as_ref().unwrap().approvals["demo"];
    assert_eq!((a.sha256.as_str(), a.granted.clone()), ("older", vec!["network".to_string()]));
    assert!(ed.offers.is_empty());
}

/// What can't be installed is refused before any card, Claude told why: outside the project, not a `.wasm`, not a
/// plugin tarae loads — and a call asking both acts, or neither.
#[test]
fn what_cant_be_installed_has_no_card() {
    let (mut ed, s) = project("install-refused");
    std::fs::write(s.join("out.wasm"), build()).unwrap();
    std::fs::write(ed.workspace.root.join("plug/notes.txt"), "x").unwrap();
    std::fs::write(ed.workspace.root.join("plug/bad.wasm"), "not a module").unwrap();
    for (n, wasm, says) in [
        (1, "../out.wasm", "../out.wasm is outside the project"),
        (2, "plug/notes.txt", "plug/notes.txt isn't a .wasm"),
        (3, "plug/bad.wasm", "plug/bad.wasm can't be installed: "),
    ] {
        assert!(ask(&mut ed, n, json!({ "wasm": wasm })).is_none());
        settle(&mut ed, |_| answered(n).is_some());
        let e = answered(n).unwrap().unwrap_err();
        assert!(e.starts_with(says), "{wasm}: {e}");
    }
    // (a FIFO named like a build: refused, never waited on; one past the limit by its size, before a byte is read)
    let fifo = std::ffi::CString::new(ed.workspace.root.join("plug/fifo.wasm").to_str().unwrap()).unwrap();
    // SAFETY: a path in the test's scratch, a mode.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let big = std::fs::File::create(ed.workspace.root.join("plug/big.wasm")).unwrap();
    big.set_len(70 << 20).unwrap();
    for (n, wasm, says) in [
        (5, "plug/fifo.wasm", "isn't a .wasm"),
        (6, "plug/big.wasm", "plug/big.wasm is 70 MB — more than a plugin may be (64 MB)"),
    ] {
        assert!(ask(&mut ed, n, json!({ "wasm": wasm })).is_none());
        settle(&mut ed, |_| answered(n).is_some());
        let e = answered(n).unwrap().unwrap_err();
        assert!(e.contains(says), "{wasm}: {e}");
    }
    // (a build outside a crate's target/ — beside the project's own Cargo.toml: named by its file, no crate said)
    std::fs::write(ed.workspace.root.join("Cargo.toml"), "[package]\nname = \"project\"\n").unwrap();
    std::fs::write(ed.workspace.root.join("plug/echo.wasm"), build()).unwrap();
    assert!(ask(&mut ed, 4, json!({ "wasm": "plug/echo.wasm" })).is_none());
    settle(&mut ed, |ed| !ed.offers.is_empty());
    let o = ed.offers.remove(0);
    assert_eq!(o.title, "Claude asks to install plugin echo");
    assert!(o.question.starts_with("From plug/echo.wasm\n"), "{}", o.question);
    for args in [json!({ "wasm": WASM, "new": "x" }), json!({})] {
        let r = ask(&mut ed, 9, args).unwrap().to_string();
        assert!(r.contains("one of wasm") && r.contains("isError"), "{r}");
    }
    assert!(ed.offers.is_empty());
}

/// Claude's `new`: a card listing what tarae writes; its `y` writes the crate — `Cargo.toml` and `.cargo`, which a run
/// can't — from the example asked for. A folder there already, or outside the project, has no card.
#[test]
fn claudes_new_is_a_card_and_its_yes_writes_the_crate() {
    let (mut ed, _s) = project("install-new");
    let root = ed.workspace.root.clone();
    assert!(ask(&mut ed, 1, json!({ "new": "plugins/todo", "from": "stamps" })).is_none());
    settle(&mut ed, |ed| ed.offers.first().is_some_and(|o| o.kind() == "plugin-new"));
    let o = &ed.offers[0];
    assert_eq!((o.title.as_str(), o.claudes()), ("Claude asks to start plugin crate todo", true));
    assert!(
        o.question.starts_with("In plugins/todo, from the example stamps:\nCargo.toml · src/lib.rs"),
        "{}",
        o.question
    );
    assert!(o.question.contains(".cargo/config.toml") && o.question.contains("REFERENCE.md"));
    assert!(
        o.question.contains("tarae/ (this tarae's SDK)") && !o.question.contains("tarae/sdk"),
        "{}",
        o.question
    );
    ed.offer_accept();
    settle(&mut ed, |_| answered(1).is_some());
    assert!(answered(1).unwrap().unwrap().starts_with("the user said yes — plugins/todo is made"));
    let lib = std::fs::read_to_string(root.join("plugins/todo/src/lib.rs")).unwrap();
    assert_eq!(lib, super::super::examples::find("stamps").unwrap().lib);
    let config = std::fs::read_to_string(root.join("plugins/todo/.cargo/config.toml")).unwrap();
    assert!(config.contains("build-dir = \"target/build\""), "{config}");
    assert!(
        std::fs::read_to_string(root.join("plugins/todo/Cargo.toml")).unwrap().contains("name = \"todo\"")
    );
    for (n, args, says) in [
        (2, json!({ "new": "plugins/todo" }), "plugins/todo is there already"),
        (3, json!({ "new": "../away" }), "../away is outside the project"),
        (4, json!({ "new": "x", "from": "nope" }), "no example nope"),
    ] {
        assert!(ask(&mut ed, n, args).is_none());
        settle(&mut ed, |_| answered(n).is_some());
        let e = answered(n).unwrap().unwrap_err();
        assert!(e.starts_with(says), "{e}");
    }
    // (`n`: nothing made)
    assert!(ask(&mut ed, 5, json!({ "new": "plugins/other" })).is_none());
    settle(&mut ed, |ed| !ed.offers.is_empty());
    ed.offer_dismiss();
    assert_eq!(answered(5), Some(Ok("the user said no — nothing was made".into())));
    assert!(!root.join("plugins/other").exists());
}

/// Between Claude's `new` card and its `y`, a run (which writes the project) puts a link to a folder outside where the
/// look found nothing — the crate's parent: tarae's write refuses the link, nothing lands outside, and Claude is told.
#[test]
fn a_link_put_on_the_way_before_the_yes_is_never_gone_through() {
    let (mut ed, s) = project("install-new-link");
    let root = ed.workspace.root.clone();
    let away = s.join("away");
    std::fs::create_dir_all(&away).unwrap();
    assert!(ask(&mut ed, 1, json!({ "new": "plugins/foo" })).is_none());
    settle(&mut ed, |ed| ed.offers.first().is_some_and(|o| o.kind() == "plugin-new"));
    std::os::unix::fs::symlink(&away, root.join("plugins")).unwrap();
    ed.offer_accept();
    settle(&mut ed, |_| answered(1).is_some());
    let e = answered(1).unwrap().unwrap_err();
    assert!(e.ends_with("plugins is a link — not gone through"), "{e}");
    assert_eq!(std::fs::read_dir(&away).unwrap().count(), 0, "nothing written outside");
}

/// The look's read stops past the limit: a file that grows after its size was taken (here a reader longer than the
/// limit) is refused, never read whole.
#[test]
fn a_build_growing_past_the_limit_is_never_read_whole() {
    use std::io::Read;
    assert_eq!(at_most(std::io::repeat(7).take(65), 64).unwrap(), None);
    assert_eq!(at_most(&b"abc"[..], 3).unwrap().as_deref(), Some(&b"abc"[..]));
}

/// A crate `tarae plugin new` writes builds in a run sandboxed as Claude's runs are — the real sandbox, this machine's
/// toolchain and cargo's cache (offline: the template's crates cached already) — cargo's build files inside the crate
/// (the template's `.cargo/config.toml`: `build-dir`), where the run may write: a `build-dir` of the user's elsewhere
/// isn't granted to it. Ignored: `:preflight-full` runs it (`.tarae.toml`), never CI (no warm cache there). Broken once:
/// without the template's `build-dir` line it fails.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "a sandboxed build with cargo's cache: :preflight-full runs it"]
fn a_new_plugin_builds_in_a_sandboxed_run() {
    if crate::sandbox::skipped_inside("a_new_plugin_builds_in_a_sandboxed_run") {
        return;
    }
    let s = Scratch::new("plugin-sandboxed-build");
    let root = s.join("p");
    std::fs::create_dir_all(&root).unwrap();
    let dir = root.join("hello");
    let files = super::super::cli::crate_files("hello", None).unwrap();
    super::super::cli::write_crate(&root, Path::new("hello"), &files).unwrap();
    let here = crate::sandbox::Here::now();
    let policy = crate::sandbox::run_policy_in(&root, &crate::llm::LlmConfig::default(), &[], &here).unwrap();
    let args = ["build", "--release", "--offline", "--quiet"].map(String::from);
    let mut cmd = crate::sandbox::command(Some(&policy), "cargo", &args);
    cmd.current_dir(&dir).stdin(std::process::Stdio::null());
    let run = crate::sandbox::spawn(Some(&policy), &cmd).unwrap();
    let mut text = String::new();
    std::io::Read::read_to_string(&mut run.stdout.unwrap(), &mut text).unwrap();
    std::io::Read::read_to_string(&mut run.stderr.unwrap(), &mut text).unwrap();
    let status = crate::spawn::wait(run.pid).unwrap();
    assert!(status.success(), "the sandboxed build failed:\n{text}");
    assert!(dir.join("target/wasm32-unknown-unknown/release/hello.wasm").is_file(), "{text}");
}

/// `:plugin-new` writes the crate at once — your act — and opens its code.
#[test]
fn the_typed_new_writes_at_once() {
    let (mut ed, s) = project("install-new-typed");
    let root = ed.workspace.root.clone();
    for bad in ["", "a b", "a --to b"] {
        assert_eq!(ed.plugin_new_typed(bad).unwrap_err(), "usage: :plugin-new <dir> [--from <example>]");
    }
    assert!(ed.plugin_new_typed("tools/2fa").unwrap_err().contains("starts with a letter"));
    ed.plugin_new_typed("tools/hello --from sort-lines").unwrap();
    settle(&mut ed, |ed| said(ed).iter().any(|m| m.starts_with("Started plugin hello")));
    assert!(
        root.join("tools/hello/REFERENCE.md").is_file()
            && root.join("tools/hello/.cargo/config.toml").is_file()
    );
    ed.plugin_new_typed("tools/hello").unwrap();
    settle(&mut ed, |ed| said(ed).iter().any(|m| m.ends_with("tools/hello already exists")));
    // (the same writer as Claude's: a link on the way is never gone through, nor a `..` out)
    let away = s.join("away");
    std::fs::create_dir_all(&away).unwrap();
    std::os::unix::fs::symlink(&away, root.join("out")).unwrap();
    ed.plugin_new_typed("out/hello").unwrap();
    settle(&mut ed, |ed| said(ed).iter().any(|m| m.ends_with("out is a link — not gone through")));
    ed.plugin_new_typed("../hello").unwrap();
    settle(&mut ed, |ed| said(ed).iter().any(|m| m.contains("not a plain name in the project")));
    assert_eq!(std::fs::read_dir(&away).unwrap().count(), 0, "nothing written outside");
    assert!(!s.join("hello").exists());
}
