//! Guard 1's folder: its checks from `$state` down, its files written again when changed, and its own `config/`'s
//! servers.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde_json::json;

use super::*;
use crate::agent::STAND_IN;

fn chmod(p: &Path, mode: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn refused(state: &Path) -> String {
    prepare(&STAND_IN, Some(state)).err().unwrap_or_else(|| "started".into())
}

/// Condition 4: `$state`, `agent-homes/`, the row's folder and its `config/` are each this user's, a real folder and
/// nobody else's to write — one that isn't ends the start, said, and is left as it is (nothing repaired, nothing
/// made through a link). The chat says it where its agent would start, and nothing starts.
#[test]
fn a_folder_not_yours_alone_isnt_started_in() {
    let dir = crate::scratch::Scratch::new("agentgate-alone");
    let state = dir.join("state");
    let made = prepare(&STAND_IN, Some(&state)).unwrap();
    let folder = made.prepared.folder().to_path_buf();
    assert_eq!(folder, state.join("agent-homes/stand-in"));
    let homes = state.join("agent-homes");
    for (dir, mode) in [(&state, 0o775), (&homes, 0o750), (&folder, 0o755), (&folder.join("config"), 0o770)] {
        let was = std::fs::metadata(dir).unwrap().permissions().mode() & 0o7777;
        chmod(dir, mode);
        let said = format!("stand-in: {} isn't yours alone — not started", dir.display());
        assert_eq!(refused(&state), said, "{mode:o}");
        assert_eq!(std::fs::metadata(dir).unwrap().permissions().mode() & 0o7777, mode, "left as it was");
        chmod(dir, was);
    }
    // ($state only for others' writes: its read is its own business)
    chmod(&state, 0o755);
    assert!(prepare(&STAND_IN, Some(&state)).is_ok());
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
    c.agent = &STAND_IN;
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

fn homes_of(state: &Path) -> String {
    state.join(HOMES).display().to_string()
}

/// Condition 2: tarae's two files are the same bytes at every start, whichever tarae writes them (no path of its
/// own in them). One changed since — its bytes, its mode, a link put in its place — is written again and said, with
/// its size; a link is replaced, never written through.
#[test]
fn a_hook_file_changed_between_starts_is_written_again_and_said() {
    let dir = crate::scratch::Scratch::new("agentgate-changed");
    let state = dir.join("state");
    let first = prepare(&STAND_IN, Some(&state)).unwrap();
    let config = first.prepared.folder().join("config");
    let (hooks, script) = (config.join("hooks.json"), config.join(crate::agent::hook::SCRIPT_NAME));
    let bytes = |p: &Path| std::fs::read(p).unwrap();
    let (hooks_was, script_was) = (bytes(&hooks), bytes(&script));
    assert!(first.changed.is_empty(), "{:?}", first.changed);
    let exe = std::env::current_exe().unwrap();
    assert!(!String::from_utf8_lossy(&hooks_was).contains(exe.to_str().unwrap()), "a tarae's own path in it");
    let again = prepare(&STAND_IN, Some(&state)).unwrap();
    assert!(again.changed.is_empty() && bytes(&hooks) == hooks_was && bytes(&script) == script_was);
    // (its bytes)
    std::fs::write(&hooks, "{}").unwrap();
    let made = prepare(&STAND_IN, Some(&state)).unwrap();
    assert_eq!(made.changed, [("config/hooks.json".to_string(), 2)]);
    assert_eq!(bytes(&hooks), hooks_was);
    // (its mode)
    chmod(&hooks, 0o644);
    assert_eq!(prepare(&STAND_IN, Some(&state)).unwrap().changed.len(), 1);
    assert_eq!(std::fs::metadata(&hooks).unwrap().permissions().mode() & 0o7777, 0o600);
    // (a link in its place: replaced by tarae's file, what it pointed at untouched)
    let marker = dir.join("marker");
    std::fs::write(&marker, "untouched").unwrap();
    std::fs::remove_file(&script).unwrap();
    std::os::unix::fs::symlink(&marker, &script).unwrap();
    let made = prepare(&STAND_IN, Some(&state)).unwrap();
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
    let config = prepare(&STAND_IN, Some(&state)).unwrap().prepared.folder().join("config");
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
    assert!(prepare(&STAND_IN, Some(&state)).is_ok(), "a file that isn't JSON nor `.json`");
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
    let folder = prepare(&STAND_IN, Some(&state)).unwrap().prepared.folder().to_path_buf();
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
