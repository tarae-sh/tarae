use super::*;
use crate::trust::{State, Store};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

/// A checkout of the test's own where all three wait: its `.tarae.toml` sets what runs commands, its
/// `.claude/settings.json` a helper command, its git config `core.fsmonitor` (`fsmonitor` = what it says) — each
/// a marker in the test's folder, never run (held until trusted). The chat's claude is a stand-in that writes what
/// it's sent to `stdin` there. The editor up with the project's card; the rest not read yet.
/// The folder goes with the `Scratch`, after the editor (#441: an `into_path` here was left in the temp folder each
/// run).
fn checkout(name: &str, fsmonitor: Option<&str>) -> (crate::scratch::Scratch, Editor, PathBuf) {
    let s = crate::scratch::Scratch::new(&format!("repotrust-{name}"));
    let dir = s.to_path_buf();
    let root = dir.join("repo");
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(dir.join("user.gitconfig"), "").unwrap();
    crate::gitrepo::global_config_for(&dir, &dir.join("user.gitconfig"));
    std::fs::write(root.join(".tarae.toml"), crate::config::tests::RISKY_PROJECT).unwrap();
    std::fs::write(root.join(".claude/settings.json"), r#"{"apiKeyHelper": "printf marker-key"}"#).unwrap();
    git(&dir, &["init", "-q"]);
    let monitor = dir.join("fsmonitor").display().to_string();
    git(&dir, &["config", "core.fsmonitor", fsmonitor.unwrap_or(&monitor)]);
    let root = std::fs::canonicalize(&root).unwrap();
    let (mut c, _) = crate::config::load_from(None, Some(root.join(".tarae.toml")), &Store::default());
    // (the stand-in in the config either answer gives too)
    let script = "printf '%s\\n' \"$0\" \"$@\" > argv; exec cat >> stdin".to_string();
    let p = c.project.as_mut().unwrap();
    for llm in [&mut c.editor.llm, &mut p.with.0.llm, &mut p.without.0.llm] {
        llm.command = "sh".into();
        llm.args = vec!["-c".into(), script.clone()];
    }
    let mut ed = Editor::with_root(c, root.clone());
    ed.trust_path = Some(dir.join("trust.json"));
    ed.claude_settings_gate = true;
    (s, ed, root)
}

/// #441: these left their checkouts in the temp folder every run.
#[test]
fn the_checkout_tests_leave_no_folder_behind() {
    let left = crate::scratch::left_by(|| {
        no_holds_each_part_back();
        a_command_asks_its_part_alone();
    });
    assert!(left.is_empty(), "left in the temp folder: {left:?}");
}

/// Plain git in the test's repository (`dir`/repo), with its own user config.
fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir.join("repo"))
        .env("GIT_CONFIG_GLOBAL", dir.join("user.gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(ok.status.success(), "git {args:?}");
}

fn until(ed: &mut Editor, what: &str, done: impl Fn(&Editor) -> bool) {
    let end = Instant::now() + crate::test_results::PATIENCE;
    while !done(ed) {
        let cards: Vec<&str> = ed.offers.iter().map(|o| o.kind()).collect();
        assert!(Instant::now() < end, "timed out waiting for {what} — cards {cards:?}");
        if let Some(ev) = ed.events.recv_timeout(Duration::from_millis(20)) {
            ed.handle_event(ev);
        }
    }
}

fn kinds(ed: &Editor) -> Vec<&'static str> {
    ed.offers.iter().map(|o| o.kind()).collect()
}

fn parts(o: &Offer) -> Vec<&'static str> {
    match &o.what {
        What::RepoTrust { parts, .. } => parts.iter().map(|p| p.kind()).collect(),
        _ => Vec::new(),
    }
}

fn claude_card(ed: &Editor) -> bool {
    ed.offers.iter().any(|o| match &o.what {
        What::ClaudeSettings { .. } => true,
        What::RepoTrust { parts, .. } => parts.iter().any(|p| matches!(p.what, What::ClaudeSettings { .. })),
        _ => false,
    })
}

/// The chat opened, its settings read (their card joins), then the repository resolved as git's watch does (its
/// card joins): one card.
fn all_three(ed: &mut Editor, root: &Path) {
    crate::chat::open(ed);
    until(ed, "Claude's part", claude_card);
    let r = crate::gitrepo::repo(root).unwrap();
    assert!(ed.git_trust_check(&r), "git's part asked");
}

fn kept(ed: &Editor, file: &Path, hash: crate::trust::Digest) -> State {
    Store::read(ed.trust_path.as_deref()).state(file, hash)
}

/// The three found one after another, each on its own worker: one card for the repository, each part under its
/// heading with the lines its own card shows, saying faintly which came last; Claude's level says its title only.
/// The chat's claude started meanwhile, held back, and a message reaches it while the card waits.
#[test]
fn the_three_waiting_together_are_one_card_and_the_chat_doesnt_wait() {
    let (_s, mut ed, root) = checkout("one", None);
    assert_eq!(kinds(&ed), ["project-trust"], "tarae's part up at once");
    all_three(&mut ed, &root);
    assert_eq!(kinds(&ed), ["repo-trust"], "one card");
    let o = &ed.offers[0];
    assert_eq!(parts(o), ["project-trust", "claude-settings", "git-trust"]);
    assert_eq!((o.title.as_str(), o.rev), (TITLE, 2), "two parts joined it");
    let lines: Vec<&str> = o.question.split('\n').collect();
    assert_eq!(
        lines[0], "repo's settings run commands as you, even unasked.",
        "the settings' sentence alone"
    );
    assert!(lines.contains(&"tarae · .tarae.toml"), "{lines:?}");
    assert!(lines.contains(&"  editor.restart-build = \"touch marker\""), "{lines:?}");
    assert!(lines.contains(&"Claude · it started with these held back"), "{lines:?}");
    assert!(lines.contains(&"  .claude/settings.json: apiKeyHelper = printf marker-key"), "{lines:?}");
    assert!(lines.iter().any(|l| l.starts_with("git · .git/config — they run on every status")), "{lines:?}");
    assert!(lines.iter().any(|l| l.starts_with("  core.fsmonitor = ")), "{lines:?}");
    // (every part's lines decided, the first line and the end faint)
    let decided = o.decided_lines();
    let monitor = lines.iter().position(|l| l.starts_with("  core.fsmonitor = ")).unwrap();
    assert!(decided.start == 2 && decided.contains(&monitor) && lines[decided.end].is_empty(), "{lines:?}");
    assert_eq!(o.answers(false), [('y', "Trust"), ('v', "Details"), ('n', "Hold back")]);
    assert!(o.weighty() && o.goes_with_workspace(), "settled before y; with its session (Claude's part)");
    // Claude sees it by its title
    let s = ed.ui_state_claude();
    let card = s.cards.iter().find(|c| c.kind == "repo-trust").unwrap();
    assert_eq!(card.asks, TITLE);
    assert!(!s.text_claude(false).contains("marker"), "none of its lines");
    // The chat didn't wait: its claude runs held back, and gets what's typed
    until(&mut ed, "claude", |ed| ed.workspace.claude.chat.as_ref().is_some_and(|c| c.running()));
    ed.workspace.claude.chat.as_mut().unwrap().input = "hello while it waits".into();
    crate::chat::send(&mut ed);
    until(&mut ed, "the message", |_| {
        std::fs::read_to_string(root.join("stdin")).is_ok_and(|s| s.contains("hello while it waits"))
    });
    assert_eq!(kinds(&ed), ["repo-trust"], "still unanswered");
    // `v`: each part's own card whole, and what holding Claude's back does
    ed.offer_view();
    let all = ed.doc().text.to_string();
    for title in [crate::trust::TITLE, crate::claudetrust::TITLE, crate::gittrust::TITLE] {
        assert!(all.contains(&format!("── {title}")), "{all}");
    }
    assert!(all.contains("Held back, Claude runs with these emptied"), "{all}");
}

/// `y` trusts each part, each kept in its own store's entry; a later change to one asks about that part alone, on
/// its own card as ever.
#[test]
fn yes_trusts_each_part_in_its_own_store_and_a_change_asks_its_part_alone() {
    let (_s, mut ed, root) = checkout("yes", None);
    all_three(&mut ed, &root);
    let What::RepoTrust { parts, .. } = ed.offers[0].what.clone() else { panic!("{:?}", kinds(&ed)) };
    ed.offer_accept();
    assert!(ed.offers.is_empty());
    let said = ed.toasts.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join(" | ");
    assert!(said.contains("Trusted — this repository's settings apply: tarae's "), "{said}");
    assert!(said.contains("Claude's 1, git's 1"), "{said}");
    assert_eq!(ed.config.restart_build, "touch marker", "tarae's part applies");
    let seen = ed.workspace.claude.chat.as_ref().and_then(|c| c.settings.seen.clone()).unwrap();
    assert_eq!(seen.state, State::Trusted);
    for p in &parts {
        let (file, hash) = match &p.what {
            What::ProjectTrust { hash } => (root.join(".tarae.toml"), *hash),
            What::ClaudeSettings { hash } => (seen.file.clone(), *hash),
            What::GitTrust(a) => {
                assert!(crate::gitrepo::answered_since(&a.config, a.digest));
                (a.config.clone(), a.digest)
            }
            _ => unreachable!(),
        };
        until(&mut ed, "each answer kept", |ed| kept(ed, &file, hash) == State::Trusted);
    }
    // Claude's settings changed: its own card alone, word for word as ever
    std::fs::write(root.join(".claude/settings.json"), r#"{"apiKeyHelper": "printf other-marker"}"#).unwrap();
    until(&mut ed, "Claude's card again", claude_card);
    assert_eq!(kinds(&ed), ["claude-settings"], "that part alone");
    let seen = ed.workspace.claude.chat.as_ref().and_then(|c| c.settings.seen.clone()).unwrap();
    assert_eq!(ed.offers[0].question, crate::claudetrust::question(&seen).0);
    assert_eq!(ed.offers[0].title, crate::claudetrust::TITLE);
    // tarae's changed (Claude's answered): its own card alone too
    ed.offer_dismiss();
    let more = crate::config::tests::RISKY_PROJECT.replace("touch marker", "touch marker2");
    std::fs::write(root.join(".tarae.toml"), more).unwrap();
    let (c, _) = crate::config::load_from(None, Some(root.join(".tarae.toml")), &Store::default());
    ed.project_trust_set(c.project);
    assert_eq!(kinds(&ed), ["project-trust"]);
    assert_eq!(ed.offers[0].title, crate::trust::TITLE);
}

/// Piece E: `:granted` lists each part trusted — for every checkout of the repository — and `d` takes each back:
/// what it let run follows at once (tarae's settings held back, the chat's claude started again with Claude's held
/// back, tarae's git holding the repository's commands), and the next look asks (the store has no answer).
#[test]
fn granted_takes_each_part_back_and_what_it_let_run_follows() {
    use crate::granted::Taken;
    use crate::trust::Kind;
    let (_s, mut ed, root) = checkout("granted", None);
    all_three(&mut ed, &root);
    let What::RepoTrust { parts, .. } = ed.offers[0].what.clone() else { panic!("{:?}", kinds(&ed)) };
    ed.offer_accept();
    let seen = ed.workspace.claude.chat.as_ref().and_then(|c| c.settings.seen.clone()).unwrap();
    let kept_at: Vec<(PathBuf, crate::trust::Digest)> = parts
        .iter()
        .map(|p| match &p.what {
            What::ProjectTrust { hash } => (root.join(".tarae.toml"), *hash),
            What::ClaudeSettings { hash } => (seen.file.clone(), *hash),
            What::GitTrust(a) => (a.config.clone(), a.digest),
            _ => unreachable!(),
        })
        .collect();
    for (file, hash) in &kept_at {
        until(&mut ed, "each answer kept", |ed| kept(ed, file, *hash) == State::Trusted);
    }
    let held = "\"apiKeyHelper\":\"\"";
    let argv = || std::fs::read_to_string(root.join("argv")).unwrap_or_default();
    until(&mut ed, "claude with them", |ed| {
        ed.workspace.claude.chat.as_ref().is_some_and(|c| c.running()) && !argv().contains(held)
    });
    assert_eq!(ed.config.restart_build, "touch marker");
    ed.granted();
    until(&mut ed, "the list", |ed| ed.picker.as_ref().is_some_and(|p| p.kind == crate::granted::KIND));
    let labels: Vec<String> = ed.picker.as_ref().unwrap().items().iter().map(|i| i.label.clone()).collect();
    for l in [
        "tarae's project settings · every checkout",
        ".tarae.toml",
        "Claude's settings · every checkout",
        ".claude/settings.json",
        "git's own commands · every checkout",
        ".git/config",
    ] {
        assert!(labels.iter().any(|x| x == l), "{l} in {labels:?}");
    }
    for kind in [Kind::Tarae, Kind::Claude, Kind::Git] {
        let p = ed.picker.as_mut().unwrap();
        p.select_where(
            |a| matches!(a, crate::picker::Action::Granted(Some((Taken::Trust(k, _), Some('y')))) if *k == kind),
        );
        ed.handle_event(crate::event::Event::Key(crate::key::Key::plain(crate::key::Code::Char('d'))));
        let (file, hash) = &kept_at[kind as usize];
        // (the store written, not only the mark `Store::state` reads meanwhile)
        let place = crate::trust::place_of(file);
        until(&mut ed, "taken back", |ed| {
            !crate::trust::is_taking(&place) && kept(ed, file, *hash) == State::Ask
        });
    }
    until(&mut ed, "the list again", |ed| {
        ed.picker.as_ref().is_some_and(|p| !p.items().iter().any(|i| i.label == ".git/config"))
    });
    // What each part let run follows on its own worker's result (`trust_take`'s job → `trust_taken`), never the
    // store's write waited on above nor the list's — K2 read Claude's part between the two.
    let seen = |ed: &Editor| {
        ed.workspace.claude.chat.as_ref().and_then(|c| c.settings.seen.as_ref()).map(|s| s.state)
    };
    until(&mut ed, "tarae's and Claude's parts held back", |ed| {
        ed.config.restart_build.is_empty() && seen(ed) == Some(State::Ask)
    });
    until(&mut ed, "claude started again held back", |ed| {
        ed.workspace.claude.chat.as_ref().is_some_and(|c| c.running()) && argv().contains(held)
    });
    let r = crate::gitrepo::repo(&root).unwrap();
    assert!(r.asks, "git's commands wait for the card again");
    assert!(!root.parent().unwrap().join("fsmonitor").exists(), "never run");
}

/// Pieces E and J: what another checkout of the repository keeps as its own (a place in it, a key file) isn't used
/// here, and the list counts it on its last row, faint; what it kept that names no checkout is every checkout's,
/// listed here.
#[test]
fn answers_kept_for_another_checkout_are_counted_not_used() {
    let (_s, mut ed, root) = checkout("elsewhere", None);
    let dir = root.parent().unwrap().to_path_buf();
    std::fs::write(dir.join("user.gitconfig"), "[user]\nname = t\nemail = t@t\n").unwrap();
    git(&dir, &["commit", "-q", "--allow-empty", "-m", "x"]);
    git(&dir, &["worktree", "add", "-q", "../wt"]);
    let wt = std::fs::canonicalize(dir.join("wt")).unwrap();
    let store = dir.join("sandbox.json");
    ed.allow_path = Some(store.clone());
    let other = crate::sandbox::profile_path(&wt);
    crate::allow::keep(&store, &other, |a| {
        a.add_read("/opt/tarae-stand-in".into());
        a.add_write(other.join("out"));
        a.add(&crate::allow::Grant::Key(other.join(".env")));
    })
    .unwrap();
    ed.granted();
    until(&mut ed, "the list", |ed| ed.picker.as_ref().is_some_and(|p| p.kind == crate::granted::KIND));
    let items = ed.picker.as_ref().unwrap().items();
    let last = items.last().unwrap();
    assert_eq!(last.label, "2 more kept for other checkouts — not used here");
    assert_eq!(last.action, crate::picker::Action::Heading, "faint, never the row in hand");
    assert!(items.iter().any(|i| i.label == "read /opt/tarae-stand-in"), "every checkout's");
    assert!(
        !items.iter().any(|i| i.label.contains(".env") || i.label.contains("wt/out")),
        "not this checkout's"
    );
}

/// `n` holds each part back, each kept so; the toast names the commands that ask again.
#[test]
fn no_holds_each_part_back() {
    let (_s, mut ed, root) = checkout("no", None);
    all_three(&mut ed, &root);
    let What::RepoTrust { parts, .. } = ed.offers[0].what.clone() else { panic!("{:?}", kinds(&ed)) };
    ed.offer_dismiss();
    assert!(ed.offers.is_empty());
    let said = ed.toasts.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join(" | ");
    assert!(
        said.contains("Held back — :project-trust, :claude-settings, :git-trust to look again"),
        "{said}"
    );
    assert_eq!(ed.config.restart_build, "");
    let What::GitTrust(a) = &parts[2].what else { panic!() };
    until(&mut ed, "git's kept", |ed| kept(ed, &a.config, a.digest) == State::Ignored);
    let What::ProjectTrust { hash } = parts[0].what else { panic!() };
    until(&mut ed, "tarae's kept", |ed| kept(ed, &root.join(".tarae.toml"), hash) == State::Ignored);
}

/// A part joining changes the card: focused, it lets go of the keys (`card_focus` is by `rev` — a `y` meant for what
/// it said isn't an answer to what it says now), and what it said about a key pressed before isn't carried over.
#[test]
fn a_part_joining_lets_go_of_the_keys() {
    let (_s, mut ed, root) = checkout("settle", None);
    crate::chat::open(&mut ed);
    until(&mut ed, "Claude's part", claude_card);
    let o = &ed.offers[0];
    assert_eq!(parts(o), ["project-trust", "claude-settings"]);
    assert!(o.question.split('\n').next().unwrap().ends_with("even unasked."), "{}", o.question);
    ed.card_focus = Some((o.id, o.rev));
    assert!(ed.card_has_keys());
    ed.offer_refused("x isn't an answer here");
    assert!(ed.offer_said_now().is_some());
    let r = crate::gitrepo::repo(&root).unwrap();
    ed.git_trust_check(&r);
    assert_eq!(parts(&ed.offers[0]), ["project-trust", "claude-settings", "git-trust"]);
    assert!(!ed.card_has_keys(), "changed: unfocused");
    assert_eq!(ed.offer_said_now(), None, "nothing carried over");
}

/// A git value too long to show is never trusted: the card says so in the warning colour, and `y` trusts the rest
/// and holds git's back.
#[test]
fn a_git_value_too_long_to_show_is_held_back_on_yes() {
    let long = format!("/bin/{}", "x".repeat(crate::trust::SHOWN_MAX));
    let (_s, mut ed, root) = checkout("long", Some(&long));
    all_three(&mut ed, &root);
    let o = &ed.offers[0];
    let lines: Vec<&str> = o.question.split('\n').collect();
    let warned: Vec<&str> = o.warnings.iter().flat_map(|r| r.clone()).map(|i| lines[i]).collect();
    assert_eq!(warned, ["  a value is too long to show — y holds git's part back"]);
    let What::RepoTrust { parts, .. } = o.what.clone() else { panic!() };
    ed.offer_accept();
    let said = ed.toasts.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join(" | ");
    assert!(
        said.contains("git's held back: a value too long to show") && !said.contains("git's 1"),
        "{said}"
    );
    assert_eq!(ed.config.restart_build, "touch marker", "the rest trusted");
    let What::GitTrust(a) = &parts[2].what else { panic!() };
    until(&mut ed, "git's kept held back", |ed| kept(ed, &a.config, a.digest) == State::Ignored);
}

/// A part asked alone by its command leaves the repository's card; the one part left is its own card again.
#[test]
fn a_command_asks_its_part_alone() {
    let (_s, mut ed, root) = checkout("alone", None);
    all_three(&mut ed, &root);
    crate::typed::execute(&mut ed, "project-trust").unwrap();
    assert_eq!(kinds(&ed), ["repo-trust", "project-trust"]);
    assert_eq!(parts(&ed.offers[0]), ["claude-settings", "git-trust"]);
    assert_eq!(ed.offers[1].title, crate::trust::TITLE);
    crate::typed::execute(&mut ed, "claude-settings").unwrap();
    assert_eq!(kinds(&ed), ["git-trust", "project-trust", "claude-settings"], "git's own card again");
    let r = crate::gitrepo::repo(&root).unwrap();
    let own = crate::gittrust::Ask::of(&r).card(&ed.workspace.root);
    assert_eq!((ed.offers[0].title.as_str(), &ed.offers[0].question), (own.title.as_str(), &own.question));
}

/// `:git-trust` typed: the checkout's repository's card, alone — also after an answer, and out of the
/// repository's card while it's there.
#[test]
fn git_trust_typed_asks_about_the_repository_again() {
    let (_s, mut ed, root) = checkout("typed", None);
    all_three(&mut ed, &root);
    ed.offer_dismiss();
    assert!(ed.offers.is_empty());
    feed(&mut ed, ":git-trust<ret>");
    until(&mut ed, "git's card", |ed| !ed.offers.is_empty());
    assert_eq!(kinds(&ed), ["git-trust"]);
    assert_eq!(ed.offers[0].title, crate::gittrust::TITLE);
    // Asked while the repository's card is up: it leaves it
    let (_s, mut ed, root) = checkout("typed-up", None);
    all_three(&mut ed, &root);
    feed(&mut ed, ":git-trust<ret>");
    until(&mut ed, "git's card", |ed| kinds(ed).contains(&"git-trust"));
    assert_eq!(kinds(&ed), ["repo-trust", "git-trust"]);
    assert_eq!(parts(&ed.offers[0]), ["project-trust", "claude-settings"]);
}

/// A part of another checkout (a submodule's git, say) isn't gathered: its own card beside it.
#[test]
fn another_checkouts_part_stays_its_own_card() {
    let (_s, mut ed, root) = checkout("other", None);
    let r = crate::gitrepo::repo(&root).unwrap();
    let mut ask = crate::gittrust::Ask::of(&r);
    ask.place = root.join("sub");
    ask.config = root.join(".git/modules/sub/config");
    ed.trust_offer(ask.card(&root));
    assert_eq!(kinds(&ed), ["project-trust", "git-trust"]);
}

// ── The agent's held files (guard 4) as a part: agy's, played by the stand-in's row (`.stand-in/` held) ────────────

/// The stand-in's held files in the checkouts below.
const AGENT_FILES: [&str; 2] = ["  .stand-in/hooks.json", "  .stand-in/mcp.json"];

/// `checkout`'s, with `files` rules files more than `AGENT_FILES` (`.stand-in/rules/<n>.md`) and the chat's agent the
/// stand-in's row: it answers its handshake and a turn, and writes what it heard to `heard` in the test's folder. Its
/// held files, git's commands and tarae's settings each wait; nothing is read yet but tarae's (its card up).
fn agent_checkout(name: &str, files: usize) -> (crate::scratch::Scratch, Editor, PathBuf) {
    use crate::agent::acp::standin::{HEARD, SCRIPT};
    let (s, mut ed, root) = checkout(name, None);
    let held = root.join(".stand-in");
    std::fs::create_dir_all(held.join("rules")).unwrap();
    std::fs::write(held.join("hooks.json"), r#"{"hooks": {}}"#).unwrap();
    std::fs::write(held.join("mcp.json"), r#"{"mcpServers": {}}"#).unwrap();
    for n in 0..files {
        std::fs::write(held.join(format!("rules/{n:02}.md")), "be brief").unwrap();
    }
    let script = serde_json::json!([
        {"expect": "initialize", "result": {"protocolVersion": 1, "agentCapabilities": {}}},
        {"expect": "session/new", "result": {"sessionId": "s-1", "modes": {"currentModeId": "default"}}},
        {"expect": "session/prompt"},
        {"end_turn": "end_turn"},
    ]);
    let dir = s.to_path_buf();
    std::fs::write(dir.join("script.json"), script.to_string()).unwrap();
    let path = |p: PathBuf| p.to_string_lossy().into_owned();
    ed.claude_env = Some(crate::llm::Env::from([
        (SCRIPT.into(), path(dir.join("script.json"))),
        (HEARD.into(), path(dir.join("heard"))),
        ("XDG_STATE_HOME".into(), path(dir.join("state"))),
    ]));
    ed.workspace.claude.chat = Some(stand_in_chat());
    (s, ed, root)
}

fn stand_in_chat() -> crate::chat::Chat {
    let mut c = crate::chat::Chat::new();
    (c.agent, c.agent_fixed) = (&crate::agent::STAND_IN, true);
    c
}

/// The methods the stand-in heard — none: it never started.
fn heard(s: &crate::scratch::Scratch) -> Vec<String> {
    let text = std::fs::read_to_string(s.join("heard")).unwrap_or_default();
    let lines = text.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok());
    lines.filter_map(|v| v["method"].as_str().map(String::from)).collect()
}

fn agent_card(ed: &Editor) -> bool {
    ed.offers.iter().any(|o| match &o.what {
        What::AgentFiles { .. } => true,
        What::RepoTrust { parts, .. } => parts.iter().any(|p| matches!(p.what, What::AgentFiles { .. })),
        _ => false,
    })
}

/// The agent's files read as its chat would start it (their part joins tarae's card), then git's part as git's watch
/// finds it.
fn agent_and_the_rest(ed: &mut Editor, root: &Path) {
    crate::chat::gate_opened(ed);
    until(ed, "the agent's part", agent_card);
    let r = crate::gitrepo::repo(root).unwrap();
    assert!(ed.git_trust_check(&r), "git's part asked");
}

fn toasts(ed: &Editor) -> String {
    ed.toasts.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join(" | ")
}

fn agent_kept(ed: &Editor, root: &Path) -> State {
    crate::agenttrust::look(&crate::agent::STAND_IN, root, ed.trust_path.as_deref()).unwrap().state
}

/// The agent's held files join tarae's and git's parts as its own card would come: one card, the agent's part in
/// Claude's place — with its session, it waits for the answer, nothing started.
#[test]
fn the_agents_files_join_tarae_and_git_in_order() {
    let (s, mut ed, root) = agent_checkout("agent-joins", 0);
    crate::chat::gate_opened(&mut ed);
    until(&mut ed, "the agent's part", agent_card);
    assert_eq!(parts(&ed.offers[0]), ["project-trust", "agent-files"]);
    let r = crate::gitrepo::repo(&root).unwrap();
    assert!(ed.git_trust_check(&r), "git's part asked");
    assert_eq!(kinds(&ed), ["repo-trust"], "one card");
    let o = &ed.offers[0];
    assert_eq!(parts(o), ["project-trust", "agent-files", "git-trust"]);
    let lines: Vec<&str> = o.question.split('\n').collect();
    assert_eq!(
        lines[0], "repo's settings run commands as you, even unasked.",
        "the settings' sentence alone"
    );
    let at =
        lines.iter().position(|l| *l == "stand-in · it isn't started until you answer").expect("its heading");
    assert!(lines[..at].contains(&"tarae · .tarae.toml") && lines[at + 4].starts_with("git · "), "{lines:?}");
    assert!(o.goes_with_workspace() && o.weighty());
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert!(c.starting() && !c.running(), "it waits for the answer");
    assert!(heard(&s).is_empty(), "the agent started: {:?}", heard(&s));
}

/// The agent's part on the joined card and its own card alone say the same: its subtitle's words, each file by name,
/// and under them the one `▲` line — that after a yes they run with your permissions, outside tarae's sandbox — in the
/// warning colour on both.
#[test]
fn the_joined_card_names_every_file_and_warns_what_a_yes_runs() {
    let (_s, mut ed, root) = agent_checkout("agent-names", 0);
    agent_and_the_rest(&mut ed, &root);
    let o = &ed.offers[0];
    let What::RepoTrust { parts, .. } = &o.what else { panic!("{:?}", kinds(&ed)) };
    let own = &parts[1];
    let q: Vec<&str> = own.question.split('\n').collect();
    assert_eq!(q[0], "stand-in isn't started until you answer.");
    let own_lines: Vec<&str> = own.decided_lines().map(|i| q[i]).collect();
    assert_eq!(
        own_lines,
        [AGENT_FILES[0], AGENT_FILES[1], crate::agenttrust::AFTER_YES],
        "each file, then the ▲ line"
    );
    assert_eq!(
        crate::agenttrust::AFTER_YES,
        "▲ after a yes these run with your permissions, outside tarae's sandbox: hooks, MCP servers, plugins"
    );
    let warned = |o: &Offer| -> Vec<usize> { o.warnings.iter().flat_map(|r| r.clone()).collect() };
    assert_eq!(warned(own), [q.len() - 1], "alone: in the warning colour");
    // (joined: its own card's lines under its heading, the ▲ line still the warning colour's)
    let lines: Vec<&str> = o.question.split('\n').collect();
    let at = lines.iter().position(|l| l.starts_with("stand-in · ")).unwrap();
    assert_eq!(lines[at + 1..at + 1 + own_lines.len()], own_lines[..], "{lines:?}");
    assert_eq!(warned(o), [at + own_lines.len()], "joined: in the warning colour");
}

/// Guard 4's fold (sandbox.md, "what runs is never folded"): a repository can't choose what the card shows by sorting
/// twelve harmless files first. What runs comes first and is never folded — a hook named last, a workflow (running until
/// measured), a plugin as its folder with its file count — and only rules files fold, the fold saying their kind and
/// count. On both cards.
#[test]
fn what_runs_is_never_folded_behind_files_read() {
    let (_s, mut ed, root) = agent_checkout("agent-fold", 12);
    let held = root.join(".stand-in");
    std::fs::write(held.join("zz-hook.json"), r#"{"hooks": {}}"#).unwrap();
    std::fs::create_dir_all(held.join("plugins/lint")).unwrap();
    for f in ["a.js", "b.js"] {
        std::fs::write(held.join("plugins/lint").join(f), "run()").unwrap();
    }
    // (a `.json` among the rules: no rules file — it runs, its own row)
    std::fs::write(held.join("rules/zz.json"), "{}").unwrap();
    std::fs::create_dir_all(held.join("workflows")).unwrap();
    std::fs::write(held.join("workflows/release.md"), "1. Tag it.").unwrap();
    agent_and_the_rest(&mut ed, &root);
    let What::RepoTrust { parts, .. } = &ed.offers[0].what else { panic!("{:?}", kinds(&ed)) };
    let own = &parts[1];
    let q: Vec<&str> = own.question.split('\n').collect();
    let own_lines: Vec<&str> = own.decided_lines().map(|i| q[i]).collect();
    let rules: Vec<String> = (0..6).map(|n| format!("  .stand-in/rules/{n:02}.md")).collect();
    let mut want = vec![
        "  .stand-in/hooks.json",
        "  .stand-in/mcp.json",
        "  .stand-in/plugins/lint/ (2 files)",
        "  .stand-in/rules/zz.json",
        "  .stand-in/workflows/release.md",
        "  .stand-in/zz-hook.json",
    ];
    want.extend(rules.iter().map(String::as_str));
    want.extend(["  … and 6 more rules files — v shows every one", crate::agenttrust::AFTER_YES]);
    assert_eq!(own_lines, want, "alone");
    let joined = ed.offers[0].question.clone();
    assert!(joined.contains("\n  .stand-in/zz-hook.json\n") && joined.contains(want[12]), "joined: {joined}");
}

/// A fold hides two or more: with one rules file past the room, it's shown and nothing is folded.
#[test]
fn one_file_left_over_is_shown_not_folded() {
    let (_s, mut ed, _) = agent_checkout("agent-one-over", 11);
    crate::chat::gate_opened(&mut ed);
    until(&mut ed, "the agent's part", agent_card);
    let own = ed.offers.iter().find_map(|o| match &o.what {
        What::RepoTrust { parts, .. } => parts.iter().find(|p| matches!(p.what, What::AgentFiles { .. })),
        _ => None,
    });
    let own = own.expect("the agent's part");
    let q: Vec<&str> = own.question.split('\n').collect();
    let own_lines: Vec<&str> = own.decided_lines().map(|i| q[i]).collect();
    // (two that run, eleven rules files: one past the twelve rows, shown)
    assert_eq!(own_lines.len(), 2 + 11 + 1, "{own_lines:?}");
    assert!(own_lines.contains(&"  .stand-in/rules/10.md"), "{own_lines:?}");
    assert!(!own_lines.iter().any(|l| l.contains("… and")), "{own_lines:?}");
}

/// `y` trusts every part, each kept in its own store's entry, and starts the agent: the message that waited goes.
#[test]
fn yes_trusts_every_part_and_starts_the_agent() {
    let (s, mut ed, root) = agent_checkout("agent-yes", 0);
    agent_and_the_rest(&mut ed, &root);
    let What::RepoTrust { parts, .. } = ed.offers[0].what.clone() else { panic!("{:?}", kinds(&ed)) };
    ed.workspace.claude.chat.as_mut().unwrap().input.set("hello");
    crate::chat::send(&mut ed);
    assert!(heard(&s).is_empty(), "started before the answer");
    ed.offer_accept();
    assert!(ed.offers.is_empty());
    let said = toasts(&ed);
    assert!(said.contains("Trusted — this repository's settings apply: tarae's "), "{said}");
    assert!(said.contains(", stand-in's 2, git's 1"), "{said}");
    assert_eq!(ed.config.restart_build, "touch marker");
    until(&mut ed, "the agent's turn", |_| heard(&s).contains(&"session/prompt".to_string()));
    let text = std::fs::read_to_string(s.join("heard")).unwrap();
    assert!(text.contains("hello"), "the message that waited: {text}");
    until(&mut ed, "the agent's kept", |ed| agent_kept(ed, &root) == State::Trusted);
    let What::GitTrust(a) = &parts[2].what else { panic!() };
    until(&mut ed, "git's kept", |ed| kept(ed, &a.config, a.digest) == State::Trusted);
}

/// `n` holds every part back; the agent isn't started in this project until you quit — a chat opened again is held
/// in those words, no card — and its no is never kept.
#[test]
fn no_holds_every_part_and_the_agent_until_you_quit() {
    let (s, mut ed, root) = agent_checkout("agent-no", 0);
    agent_and_the_rest(&mut ed, &root);
    ed.offer_dismiss();
    assert!(ed.offers.is_empty());
    let said = toasts(&ed);
    let words = "Held back — :project-trust, :agent-files, :git-trust to look again · stand-in isn't started in this \
                 project until you quit";
    assert!(said.contains(words), "{said}");
    assert_eq!(ed.config.restart_build, "");
    ed.workspace.claude.chat = Some(stand_in_chat());
    crate::chat::gate_opened(&mut ed);
    let held = crate::agenttrust::held_back("stand-in");
    until(&mut ed, "held", |ed| {
        ed.workspace.claude.chat.as_ref().unwrap().msgs.iter().any(|m| m.text == held)
    });
    assert!(!agent_card(&ed) && heard(&s).is_empty(), "asked again, or started");
    assert_eq!(agent_kept(&ed, &root), State::Ask, "never kept");
}

/// `v`: the agent's own card whole and its detail — every file, why it asks.
#[test]
fn v_shows_the_agents_part_whole() {
    let (_s, mut ed, root) = agent_checkout("agent-view", crate::agenttrust::SHOWN_MAX);
    agent_and_the_rest(&mut ed, &root);
    ed.offer_view();
    let all = ed.doc().text.to_string();
    assert!(all.contains("── Trust this project's stand-in files?"), "{all}");
    assert!(all.contains("stand-in loads these from the project by itself"), "{all}");
    let last = format!("  .stand-in/rules/{:02}.md", crate::agenttrust::SHOWN_MAX - 1);
    assert!(all.contains(&last) && all.contains(AGENT_FILES[0]), "every file: {all}");
}

/// `:agent-files` takes the agent's part off the card and asks about it alone, its own card word for word; after an
/// `n` (the hold until you quit) it asks again, and its `y` starts the agent.
#[test]
fn agent_files_asks_its_part_alone_and_again_after_no() {
    let (s, mut ed, root) = agent_checkout("agent-typed", 0);
    agent_and_the_rest(&mut ed, &root);
    let What::RepoTrust { parts: was, .. } = ed.offers[0].what.clone() else { panic!("{:?}", kinds(&ed)) };
    crate::typed::execute(&mut ed, "agent-files").unwrap();
    assert_eq!(kinds(&ed), ["repo-trust", "agent-files"]);
    assert_eq!(parts(&ed.offers[0]), ["project-trust", "git-trust"]);
    assert_eq!((&ed.offers[1].title, &ed.offers[1].question), (&was[1].title, &was[1].question));
    // Held back, all of it: asked again by the command, alone
    ed.offer_dismiss();
    ed.offer_dismiss();
    assert!(ed.offers.is_empty() && toasts(&ed).contains(&crate::agenttrust::held_back("stand-in")));
    crate::typed::execute(&mut ed, "agent-files").unwrap();
    assert_eq!(kinds(&ed), ["agent-files"]);
    assert_eq!(ed.offers[0].question, was[1].question);
    ed.offer_accept();
    until(&mut ed, "the agent's start", |_| heard(&s).contains(&"session/new".to_string()));
    until(&mut ed, "the agent's kept", |ed| agent_kept(ed, &root) == State::Trusted);
    assert!(ed.agent_files_held.is_empty(), "the hold let go");
    // Asked once trusted: not again here — what takes the yes back, said
    let e = crate::typed::execute(&mut ed, "agent-files").unwrap_err();
    assert!(e.starts_with("trusted already — :granted takes the yes back"), "{e}");
}

/// On Claude's row `:agent-files` is `:claude-settings`: what its project files run as you are its settings — the
/// same card, alone, out of the repository's; with no chat, the same words.
#[test]
fn agent_files_on_claudes_row_asks_about_claudes_settings() {
    let (_s, mut ed, root) = checkout("claude-typed", None);
    let none = crate::typed::execute(&mut ed, "agent-files").unwrap_err();
    assert_eq!(Err(none), ed.claude_settings_ask());
    all_three(&mut ed, &root);
    crate::typed::execute(&mut ed, "agent-files").unwrap();
    assert_eq!(kinds(&ed), ["repo-trust", "claude-settings"]);
    assert_eq!(parts(&ed.offers[0]), ["project-trust", "git-trust"]);
    assert_eq!(ed.offers[1].title, crate::claudetrust::TITLE);
}

/// Keys to the code (the chat opened with them).
fn feed(ed: &mut Editor, keys: &str) {
    ed.workspace.leave(crate::workspace::Here::Chat);
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key: crate::key::Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => crate::key::Key::plain(crate::key::Code::Char(c)),
        };
        ed.handle_key(key);
    }
}
