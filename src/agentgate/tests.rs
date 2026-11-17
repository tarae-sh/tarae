//! Guard 1's folder: its checks from `$state` down, its files written again when changed, and its own `config/`'s
//! servers; the watch while the agent runs, judging bytes.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde_json::json;

use super::*;
use crate::agent::STAND_IN;

fn chmod(p: &Path, mode: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn refused(state: &Path) -> String {
    prepare(&STAND_IN, Some(state), state.parent().unwrap()).err().unwrap_or_else(|| "started".into())
}

/// Condition 4: `$state`, `agent-homes/`, the row's folder, the project's in it (L-B6) and its `config/` are each this
/// user's, a real folder and nobody else's to write — one that isn't ends the start, said, and is left as it is (nothing repaired, nothing
/// made through a link). The chat says it where its agent would start, and nothing starts.
#[test]
fn a_folder_not_yours_alone_isnt_started_in() {
    let dir = crate::scratch::Scratch::new("agentgate-alone");
    let state = dir.join("state");
    let made = prepare(&STAND_IN, Some(&state), &dir).unwrap();
    let folder = made.prepared.folder().to_path_buf();
    assert_eq!(folder, state.join("agent-homes/stand-in").join(home_key(&dir)));
    let (homes, row) = (state.join("agent-homes"), state.join("agent-homes/stand-in"));
    let config = folder.join("config");
    for (dir, mode) in [(&state, 0o775), (&homes, 0o750), (&row, 0o755), (&folder, 0o705), (&config, 0o770)] {
        let was = std::fs::metadata(dir).unwrap().permissions().mode() & 0o7777;
        chmod(dir, mode);
        let said = format!("stand-in: {} isn't yours alone — not started", dir.display());
        assert_eq!(refused(&state), said, "{mode:o}");
        assert_eq!(std::fs::metadata(dir).unwrap().permissions().mode() & 0o7777, mode, "left as it was");
        chmod(dir, was);
    }
    // ($state only for others' writes: its read is its own business)
    chmod(&state, 0o755);
    assert!(prepare(&STAND_IN, Some(&state), &dir).is_ok());
    // (the project's folder a link, to a folder of this user's: refused, nothing made through it)
    let away = dir.join("away");
    std::fs::create_dir(&away).unwrap();
    chmod(&away, 0o700);
    let moved = dir.join("moved");
    std::fs::rename(&folder, &moved).unwrap();
    std::os::unix::fs::symlink(&away, &folder).unwrap();
    assert_eq!(refused(&state), format!("stand-in: {} isn't yours alone — not started", folder.display()));
    assert_eq!(std::fs::read_dir(&away).unwrap().count(), 0, "made through the link");
    std::fs::remove_file(&folder).unwrap();
    std::fs::rename(&moved, &folder).unwrap();
    // (a link where a folder should be, to a folder of this user's: refused, nothing made through it)
    let (other, there) = (dir.join("other"), dir.join("there"));
    std::fs::create_dir_all(other.join("tarae")).unwrap();
    std::fs::create_dir(&there).unwrap();
    chmod(&there, 0o700);
    std::os::unix::fs::symlink(&there, other.join("tarae/agent-homes")).unwrap();
    let state = other.join("tarae");
    assert_eq!(refused(&state), format!("stand-in: {} isn't yours alone — not started", homes_of(&state)));
    assert_eq!(std::fs::read_dir(&there).unwrap().count(), 0, "made through the link");
    // Through the chat: said where it would start, nothing started
    let state = dir.join("chat-state");
    std::fs::create_dir_all(state.join("tarae/agent-homes/stand-in")).unwrap();
    chmod(&state.join("tarae/agent-homes"), 0o700);
    chmod(&state.join("tarae/agent-homes/stand-in"), 0o755);
    let mut ed = Editor::with_root(crate::config::Config::default(), dir.path().to_path_buf());
    ed.claude_env = Some(crate::llm::Env::from([("XDG_STATE_HOME".into(), state.to_string_lossy().into())]));
    let mut c = crate::chat::Chat::new();
    (c.agent, c.agent_fixed) = (&STAND_IN, true);
    ed.workspace.claude.chat = Some(c);
    crate::chat::gate_opened(&mut ed);
    assert!(ed.workspace.claude.chat.as_ref().unwrap().starting(), "it waits for its folder");
    let said = format!(
        "stand-in: {} isn't yours alone — not started",
        state.join("tarae/agent-homes/stand-in").display()
    );
    let has = |ed: &Editor| ed.workspace.claude.chat.as_ref().unwrap().msgs.iter().any(|m| m.text == said);
    crate::test_results::settle_saying(&mut ed, has, |ed| {
        format!(
            "{:?}",
            ed.workspace.claude.chat.as_ref().unwrap().msgs.iter().map(|m| &m.text).collect::<Vec<_>>()
        )
    });
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert!(!c.running() && !c.starting(), "started, or still waiting");
}

/// The stand-in's script: its handshake — `session/new`, or `session/resume` where it continues the conversation —
/// then it reads until stdin closes.
fn script(dir: &crate::scratch::Scratch, open: &str) {
    let caps = json!({"sessionCapabilities": {"resume": {}}});
    let script = json!([
        {"expect": "initialize", "result": {"protocolVersion": 1, "agentCapabilities": caps}},
        {"expect": open, "result": {"sessionId": "s-1", "modes": {"currentModeId": "default"}}},
        {"eof": true},
    ]);
    std::fs::write(dir.join("script.json"), script.to_string()).unwrap();
}

/// An editor in `root` whose chat is the stand-in's (`script`, a new session), its state and store in `dir`.
fn running_editor(dir: &crate::scratch::Scratch, root: &Path) -> Editor {
    use crate::agent::acp::standin::{HEARD, SCRIPT};
    script(dir, "session/new");
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let mut ed = Editor::with_root(crate::config::Config::default(), root.to_path_buf());
    ed.claude_env = Some(crate::llm::Env::from([
        (SCRIPT.into(), path(&dir.join("script.json"))),
        (HEARD.into(), path(&dir.join("heard"))),
        ("XDG_STATE_HOME".into(), path(&dir.join("state"))),
    ]));
    ed.trust_path = Some(dir.join("trust.json"));
    let mut c = crate::chat::Chat::new();
    (c.agent, c.agent_fixed) = (&STAND_IN, true);
    ed.workspace.claude.chat = Some(c);
    ed
}

/// How many times the stand-in heard `method`.
fn heard_n(dir: &crate::scratch::Scratch, method: &str) -> usize {
    let text = std::fs::read_to_string(dir.join("heard")).unwrap_or_default();
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["method"] == method)
        .count()
}

fn chat(ed: &Editor) -> &crate::chat::Chat {
    ed.workspace.claude.chat.as_ref().unwrap()
}

fn said_n(ed: &Editor, words: &str) -> usize {
    chat(ed).msgs.iter().filter(|m| m.text == words).count()
}

#[track_caller]
fn settle_until(ed: &mut Editor, done: impl Fn(&Editor) -> bool) {
    crate::test_results::settle_saying(ed, done, |ed| {
        let msgs: Vec<_> =
            ed.workspace.claude.chat.iter().flat_map(|c| c.msgs.iter().map(|m| &m.text)).collect();
        let said: Vec<_> = ed.messages.iter().map(|m| &m.2).collect();
        format!("the chat: {msgs:?}; said: {said:?}")
    });
}

/// `bytes` put at `path` as a start of tarae's puts them: to a new name, renamed over it (the new name a folder up: in
/// a held folder, a file of its own is a change).
fn renamed_over(path: &Path, bytes: &[u8]) {
    let temp = path.parent().and_then(Path::parent).unwrap().join(".renamed");
    std::fs::write(&temp, bytes).unwrap();
    let mode = std::fs::metadata(path).unwrap().permissions().mode();
    chmod(&temp, mode & 0o7777);
    std::fs::rename(&temp, path).unwrap();
}

/// L-B2's watch: every held folder up to the git top is armed — a file added in a parent's while the agent runs (a
/// `git pull` in the repository above the project's folder) stops it, said once, and the card asks again naming it
/// with the rest of the chain's.
#[test]
fn a_file_added_in_a_parents_held_folder_while_it_runs_stops_it() {
    let dir = crate::scratch::Scratch::new("agentgate-watch-parent");
    let repo = dir.join("repo");
    let root = repo.join("p");
    crate::agenttrust::repository_at(&repo);
    for folder in [&repo, &root] {
        std::fs::create_dir_all(folder.join(".stand-in")).unwrap();
        std::fs::write(folder.join(".stand-in/hooks.json"), "{}").unwrap();
    }
    let seen = crate::agenttrust::look(&STAND_IN, &root, None).unwrap();
    crate::trust::record(dir.join("trust.json"), crate::trust::Kind::Agent, seen.file, seen.hash, true)
        .unwrap();
    let mut ed = running_editor(&dir, &root);
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| chat(ed).running() && heard_n(&dir, "session/new") == 1);
    std::fs::write(repo.join(".stand-in/added.json"), "{\"pulled\": true}").unwrap();
    let card = |ed: &Editor| {
        let o = ed.offers.iter().find(|o| matches!(o.what, crate::offer::What::AgentFiles { .. }))?;
        Some(o.question.lines().skip(2).map(String::from).collect::<Vec<_>>())
    };
    settle_until(&mut ed, |ed| card(ed).is_some());
    let words = "stand-in's files in this project changed while it ran — stopped: it and everything it started ended. \
                 Their card asks again.";
    assert_eq!(said_n(&ed, words), 1, "said once");
    assert!(!chat(&ed).running(), "stopped");
    let lines = ["  .stand-in/added.json", "  .stand-in/hooks.json", "  p/.stand-in/hooks.json"];
    assert_eq!(card(&ed).unwrap(), lines);
    ed.workspace.claude.chat = None;
}

/// The design session's note 2 on L-B2: the top's `.git` is armed — a worktree's `.git` file written over in place
/// while the agent runs (no folder's list changes: nothing else armed sees it), so it's no repository's and the chain
/// is another, stops it, said once, and the card asks again for the chain as it is now: up to the repository above
/// (`up`). (A repository above, since the fix on L-B3: with none, a junk `.git` is still the top, and the chain the
/// same.)
#[test]
fn the_tops_git_written_over_while_it_runs_stops_it() {
    let dir = crate::scratch::Scratch::new("agentgate-watch-top");
    let up = dir.join("up");
    let wt = up.join("wt");
    let root = wt.join("p");
    crate::agenttrust::repository_at(&up);
    for folder in [&up, &wt, &root] {
        std::fs::create_dir_all(folder.join(".stand-in")).unwrap();
        std::fs::write(folder.join(".stand-in/hooks.json"), "{}").unwrap();
    }
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", dir.join("main/.git/worktrees/wt").display()))
        .unwrap();
    let seen = crate::agenttrust::look(&STAND_IN, &root, None).unwrap();
    crate::trust::record(dir.join("trust.json"), crate::trust::Kind::Agent, seen.file, seen.hash, true)
        .unwrap();
    let mut ed = running_editor(&dir, &root);
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| chat(ed).running() && heard_n(&dir, "session/new") == 1);
    std::fs::write(wt.join(".git"), "not a gitdir\n").unwrap();
    let card = |ed: &Editor| {
        let o = ed.offers.iter().find(|o| matches!(o.what, crate::offer::What::AgentFiles { .. }))?;
        Some(o.question.lines().skip(2).map(String::from).collect::<Vec<_>>())
    };
    settle_until(&mut ed, |ed| card(ed).is_some());
    let words = "stand-in's files in this project changed while it ran — stopped: it and everything it started ended. \
                 Their card asks again.";
    assert_eq!(said_n(&ed, words), 1, "said once");
    assert!(!chat(&ed).running(), "stopped");
    let lines = ["  .stand-in/hooks.json", "  wt/.stand-in/hooks.json", "  wt/p/.stand-in/hooks.json"];
    assert_eq!(card(&ed).unwrap(), lines);
    ed.workspace.claude.chat = None;
}

/// The watch (L5): the hook file changed while the agent runs stops it — its turn and group ended, said once in the
/// transcript and in a toast, its watch over. A message after it starts it again on the conversation, the file written
/// again, said.
#[test]
fn a_hook_file_changed_while_it_runs_stops_it() {
    let dir = crate::scratch::Scratch::new("agentgate-watch-hook");
    let root = dir.join("p");
    std::fs::create_dir_all(&root).unwrap();
    let mut ed = running_editor(&dir, &root);
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| chat(ed).running() && heard_n(&dir, "session/new") == 1);
    let state = crate::agent::acp::state(ed.claude_env.as_ref()).unwrap();
    let hooks = state.join(HOMES).join("stand-in").join(home_key(&root)).join("config/hooks.json");
    let was = std::fs::read(&hooks).unwrap();
    std::fs::write(&hooks, "{}").unwrap();
    settle_until(&mut ed, |ed| !chat(ed).running());
    let words = "stand-in's config/hooks.json changed while it ran — stopped: it and everything it started ended. \
                 tarae writes it again at its next start.";
    assert_eq!(said_n(&ed, words), 1, "said once");
    assert!(
        ed.messages.iter().any(|m| m.2 == "stand-in stopped — its config/hooks.json changed"),
        "the toast"
    );
    let c = chat(&ed);
    assert!(c.agent_gate.watch.is_none() && !c.agent_gate.stopping() && !c.starting(), "its watch over");
    script(&dir, "session/resume");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.input.set("again");
    crate::chat::send(&mut ed);
    settle_until(&mut ed, |_| heard_n(&dir, "session/prompt") == 1);
    assert_eq!(
        (heard_n(&dir, "session/new"), heard_n(&dir, "session/resume")),
        (1, 1),
        "started again on it"
    );
    assert_eq!(std::fs::read(&hooks).unwrap(), was, "written again");
    assert_eq!(said_n(&ed, "stand-in's config/hooks.json was changed outside tarae — written again"), 1);
    assert_eq!(said_n(&ed, words), 1, "no second stop");
    ed.workspace.claude.chat = None;
}

/// Bytes, not events (L2 note 2): another tarae's start renames the same bytes over tarae's two files, and a held file
/// is renamed over with its own bytes — nothing stops. A held file truly changed after them stops it, said as the held
/// files' change, never the hook's.
#[test]
fn the_same_bytes_renamed_over_its_files_stop_nothing() {
    let dir = crate::scratch::Scratch::new("agentgate-watch-same");
    let root = dir.join("p");
    let rules = root.join(".stand-in/rules.md");
    std::fs::create_dir_all(rules.parent().unwrap()).unwrap();
    std::fs::write(&rules, "be brief").unwrap();
    let seen = crate::agenttrust::look(&STAND_IN, &root, None).unwrap();
    let kind = crate::trust::Kind::Agent;
    crate::trust::record(dir.join("trust.json"), kind, seen.file.clone(), seen.hash, true).unwrap();
    let mut ed = running_editor(&dir, &root);
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| chat(ed).running() && heard_n(&dir, "session/new") == 1);
    // (another tarae's start: the same bytes renamed over both)
    let state = crate::agent::acp::state(ed.claude_env.as_ref()).unwrap();
    let config = state.join(HOMES).join("stand-in").join(home_key(&root)).join("config");
    let inode = |f: &str| std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(config.join(f)).unwrap());
    let was = (inode("hooks.json"), inode(crate::agent::hook::SCRIPT_NAME));
    let again = prepare(&STAND_IN, Some(&state), &root).unwrap();
    assert!(again.changed.is_empty(), "{:?}", again.changed);
    let now = (inode("hooks.json"), inode(crate::agent::hook::SCRIPT_NAME));
    assert!(was.0 != now.0 && was.1 != now.1, "both renamed over: {was:?} {now:?}");
    renamed_over(&rules, b"be brief");
    std::fs::write(&rules, "be long").unwrap();
    settle_until(&mut ed, |ed| !chat(ed).running());
    let held = "stand-in's files in this project changed while it ran — stopped: it and everything it started ended. \
                Their card asks again.";
    let msgs: Vec<_> = chat(&ed).msgs.iter().map(|m| m.text.clone()).collect();
    assert_eq!(said_n(&ed, held), 1, "{msgs:?}");
    assert!(!msgs.iter().any(|m| m.contains("hooks.json") || m.contains("tarae-hook.sh")), "{msgs:?}");
    ed.workspace.claude.chat = None;
}

fn homes_of(state: &Path) -> String {
    state.join(HOMES).display().to_string()
}

/// L-B6: one home a project, under its row's folder. A repository's is one for its checkouts — its main one, a linked
/// one (made by hand, git's links both ways) and a folder in either — and another repository's is its own; a folder in
/// no repository is keyed by its real path, so a link to it gets the same home. The name is 16 hex, no path in it, and
/// what the agent keeps in one home isn't in another's.
#[test]
fn each_repository_gets_its_own_home() {
    let dir = crate::scratch::Scratch::new("agentgate-homes");
    let state = dir.join("state");
    let row = state.join("agent-homes/stand-in");
    let home = |root: &Path| {
        let folder = prepare(&STAND_IN, Some(&state), root).unwrap().prepared.folder().to_path_buf();
        assert_eq!(folder.parent(), Some(row.as_path()), "under its row's folder");
        folder
    };
    let (one, two, wt, plain) = (dir.join("one"), dir.join("two"), dir.join("one-wt"), dir.join("plain"));
    for d in [one.join(".git"), two.join(".git"), one.join("sub"), plain.clone()] {
        std::fs::create_dir_all(d).unwrap();
    }
    let entry = one.join(".git/worktrees/wt");
    std::fs::create_dir_all(&entry).unwrap();
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", entry.display())).unwrap();
    std::fs::write(entry.join("gitdir"), format!("{}\n", wt.join(".git").display())).unwrap();
    std::fs::write(entry.join("commondir"), "../..\n").unwrap();
    std::os::unix::fs::symlink(&plain, dir.join("link")).unwrap();
    let first = home(&one);
    assert_eq!(home(&wt), first, "a repository's two checkouts: one home");
    assert_eq!(home(&one.join("sub")), first, "a folder in it: the repository's");
    let (second, own) = (home(&two), home(&plain));
    assert_ne!(first, second, "two repositories: two homes");
    assert!(own != first && own != second, "a folder in no repository: its own");
    assert_eq!(home(&dir.join("link")), own, "a link to it: the same home");
    let name = first.file_name().unwrap().to_str().unwrap();
    assert!(name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit()), "{name}");
    std::fs::write(first.join("conversation_summaries.db"), "one's").unwrap();
    assert!(
        !second.join("conversation_summaries.db").exists() && !own.join("conversation_summaries.db").exists()
    );
}

/// Condition 2: tarae's two files are the same bytes at every start, whichever tarae writes them (no path of its
/// own in them). One changed since — its bytes, its mode, a link put in its place — is written again and said, with
/// its size; a link is replaced, never written through.
#[test]
fn a_hook_file_changed_between_starts_is_written_again_and_said() {
    let dir = crate::scratch::Scratch::new("agentgate-changed");
    let state = dir.join("state");
    let first = prepare(&STAND_IN, Some(&state), &dir).unwrap();
    let config = first.prepared.folder().join("config");
    let (hooks, script) = (config.join("hooks.json"), config.join(crate::agent::hook::SCRIPT_NAME));
    let bytes = |p: &Path| std::fs::read(p).unwrap();
    let (hooks_was, script_was) = (bytes(&hooks), bytes(&script));
    assert!(first.changed.is_empty(), "{:?}", first.changed);
    let exe = std::env::current_exe().unwrap();
    assert!(!String::from_utf8_lossy(&hooks_was).contains(exe.to_str().unwrap()), "a tarae's own path in it");
    let again = prepare(&STAND_IN, Some(&state), &dir).unwrap();
    assert!(again.changed.is_empty() && bytes(&hooks) == hooks_was && bytes(&script) == script_was);
    // (its bytes)
    std::fs::write(&hooks, "{}").unwrap();
    let made = prepare(&STAND_IN, Some(&state), &dir).unwrap();
    assert_eq!(made.changed, [("config/hooks.json".to_string(), 2)]);
    assert_eq!(bytes(&hooks), hooks_was);
    // (its mode)
    chmod(&hooks, 0o644);
    assert_eq!(prepare(&STAND_IN, Some(&state), &dir).unwrap().changed.len(), 1);
    assert_eq!(std::fs::metadata(&hooks).unwrap().permissions().mode() & 0o7777, 0o600);
    // (a link in its place: replaced by tarae's file, what it pointed at untouched)
    let marker = dir.join("marker");
    std::fs::write(&marker, "untouched").unwrap();
    std::fs::remove_file(&script).unwrap();
    std::os::unix::fs::symlink(&marker, &script).unwrap();
    let made = prepare(&STAND_IN, Some(&state), &dir).unwrap();
    let rel = format!("config/{}", crate::agent::hook::SCRIPT_NAME);
    assert_eq!(made.changed.iter().map(|c| &c.0).collect::<Vec<_>>(), [&rel]);
    assert!(std::fs::symlink_metadata(&script).unwrap().is_file());
    assert_eq!((bytes(&script), std::fs::read_to_string(&marker).unwrap()), (script_was, "untouched".into()));
    let left: Vec<_> =
        std::fs::read_dir(&config).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
    assert_eq!(left.len(), 2, "nothing left of a write: {left:?}");
}

/// Condition 3: a server of the agent's own `config/` named as tarae's (`tarae`, any case, alone or before a mark)
/// ends the start, said with its file — read by key where it reads as JSON, by the word where a `.json` file
/// doesn't; one that can't be read ends it too. Other names, other keys and other files don't.
#[test]
fn a_server_named_as_taraes_in_its_config_isnt_started_with() {
    let dir = crate::scratch::Scratch::new("agentgate-servers");
    let state = dir.join("state");
    let config = prepare(&STAND_IN, Some(&state), &dir).unwrap().prepared.folder().join("config");
    let file = config.join("mcp_config.json");
    let refused_by = |text: &str| {
        std::fs::write(&file, text).unwrap();
        let said = refused(&state);
        std::fs::remove_file(&file).unwrap();
        said
    };
    let named = |server: &str| {
        format!(
            "stand-in: config/mcp_config.json names an MCP server \"{server}\" — its tools would pass for tarae's; not started"
        )
    };
    for server in ["tarae", "Tarae", "tarae_x", "TARAE-x"] {
        let text = json!({"mcpServers": {"other": {}, server: {"command": "x"}}}).to_string();
        assert_eq!(refused_by(&text), named(server), "{server}");
    }
    let listed = json!({"mcp": {"servers": [{"name": "other"}, {"name": "tarae"}]}}).to_string();
    assert_eq!(refused_by(&listed), named("tarae"));
    assert_eq!(refused_by("{ // tarae\n }"), named("tarae"), "not JSON: by the word");
    for text in [
        json!({"mcpServers": {"taraexyz": {}, "other": {"env": {"TARAE_X": "1"}, "args": ["tarae"]}}}),
        json!({"projects": {"/p/tarae": {"name": "tarae"}}}),
    ] {
        assert_eq!(refused_by(&text.to_string()), "started", "{text}");
    }
    std::fs::write(config.join(".migrated"), "tarae").unwrap();
    assert!(prepare(&STAND_IN, Some(&state), &dir).is_ok(), "a file that isn't JSON nor `.json`");
    std::fs::write(config.join("projects"), json!({"mcpServers": {"tarae": {}}}).to_string()).unwrap();
    assert_eq!(
        refused(&state),
        named("tarae").replace("mcp_config.json", "projects"),
        "JSON by another name"
    );
    std::fs::remove_file(config.join("projects")).unwrap();
    let unread = |why: &str| {
        format!("stand-in: config/mcp_config.json can't be read for its MCP servers — {why}; not started")
    };
    std::fs::write(dir.join("far.json"), "{}").unwrap();
    std::os::unix::fs::symlink(dir.join("far.json"), &file).unwrap();
    assert_eq!(refused(&state), unread("not a plain file (a link is never read through)"));
    std::fs::remove_file(&file).unwrap();
    let big = format!("{{\"pad\": \"{}\"}}", "x".repeat(READ_MAX as usize));
    assert_eq!(refused_by(&big), unread("too big to read"));
}

/// What the real agy leaves in its `config/` (the design session's probe, its L2 note 3) passes condition 3's check:
/// an empty `mcp_config.json` (0 bytes — not JSON, and no word in it), an empty `.migrated`, `config.json` with its
/// plugins, a `projects/` folder.
#[test]
fn what_agy_leaves_in_its_config_passes() {
    let dir = crate::scratch::Scratch::new("agentgate-agy-left");
    let state = dir.join("state");
    let folder = prepare(&STAND_IN, Some(&state), &dir).unwrap().prepared.folder().to_path_buf();
    let config = folder.join("config");
    std::fs::write(config.join("mcp_config.json"), "").unwrap();
    std::fs::write(config.join(".migrated"), "").unwrap();
    std::fs::write(config.join("config.json"), json!({"plugins": {}}).to_string()).unwrap();
    std::fs::create_dir(config.join("projects")).unwrap();
    assert_eq!(
        servers_named_as_taraes("stand-in", &folder, &config, &[]),
        Ok(()),
        "a 0-byte mcp_config.json, as agy leaves it"
    );
    assert_eq!(refused(&state), "started", "and the start");
}
