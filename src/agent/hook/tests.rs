//! The hook's tests: the rule on agy's real input shape, the script that asks tarae or refuses, a panic refused.

use serde_json::{Value, json};

use super::*;

/// A hook's input in agy's shape (the design session's probe, `hook.log`'s first line: its keys, the paths made
/// neutral), calling `name`.
fn agy_input(name: Value) -> Vec<u8> {
    json!({
        "artifactDirectoryPath": "/home/u/.gemini/antigravity-cli/brain/4d13",
        "conversationId": "4d13",
        "cwd": "/p/proj",
        "hookEventName": "PreToolUse",
        "lastUserInput": "Run this shell command and tell me its output: echo probe-ran-2",
        "modelName": "gemini-4-argon-medium",
        "stepIdx": 2,
        "toolCall": {"args": {"CommandLine": "echo probe-ran-2", "WaitMsBeforeAsync": 5000,
            "toolSummary": "Run echo command"}, "id": "call_6671", "name": name},
        "transcriptPath": "/home/u/.gemini/antigravity-cli/brain/4d13/.system_generated/logs/transcript_full.jsonl",
        "workspacePaths": ["/p/proj"],
    })
    .to_string()
    .into_bytes()
}

fn decided(input: &[u8]) -> (String, String) {
    let v = decide(input);
    (v["decision"].as_str().unwrap_or_default().into(), v["reason"].as_str().unwrap_or_default().into())
}

/// Only tarae's own tools pass, by the name agy gives them (`mcp_tarae_<tool>`); the agent's own (`run_command`,
/// `read_file`, `write_file`), another server's, `mcp_tarae_` alone and another case are refused with `DENY`; no
/// name, a name that isn't text, junk and an input past `INPUT_MAX` with `UNREAD`.
#[test]
fn the_hook_allows_only_taraes_tools() {
    let allow = ("allow".to_string(), String::new());
    assert_eq!(decided(&agy_input(json!("mcp_tarae_search"))), allow);
    assert_eq!(decided(&agy_input(json!("mcp_tarae_read"))), allow);
    let refused = ("deny".to_string(), DENY.to_string());
    for name in
        ["run_command", "read_file", "write_file", "mcp_other_x", "mcp_tarae_", "MCP_TARAE_search", ""]
    {
        assert_eq!(decided(&agy_input(json!(name))), refused, "{name}");
    }
    let unread = ("deny".to_string(), UNREAD.to_string());
    assert_eq!(decided(&agy_input(Value::Null)), unread, "no name");
    assert_eq!(decided(&agy_input(json!(7))), unread, "a name that isn't text");
    assert_eq!(decided(b"{\"toolCall\": "), unread, "junk");
    assert_eq!(decided(b""), unread, "nothing");
    let mut long = agy_input(json!("mcp_tarae_search"));
    long.splice(1..1, format!("\"pad\": \"{}\", ", "x".repeat(INPUT_MAX)).into_bytes());
    assert!(serde_json::from_slice::<Value>(&long).is_ok());
    assert_eq!(decided(&long), unread, "past INPUT_MAX, an allowed name in it");
    // (read to the bound and a byte, never past)
    assert_eq!(answer(&long[..]), decide(&long).to_string());
    assert_eq!(answer(&agy_input(json!("mcp_tarae_search"))[..]), r#"{"decision":"allow"}"#);
    // (the panic's line is `UNREAD`'s)
    assert_eq!(PANIC_LINE, format!("{}\n", deny(UNREAD)));
}

/// The script runs `sh` on `SCRIPT` with `hook` as `ENV` (None: not set), `input` on stdin: its stdout.
fn script(dir: &std::path::Path, hook: Option<&std::path::Path>, input: &str) -> String {
    use std::io::Write as _;
    let path = dir.join(SCRIPT_NAME);
    std::fs::write(&path, SCRIPT).unwrap();
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.arg(&path).env_remove(ENV);
    if let Some(h) = hook {
        cmd.env(ENV, h);
    }
    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    String::from_utf8(out.stdout).unwrap()
}

/// The hook file's program asks the tarae in `ENV` — run with `agent-hook`, the input its, its answer passed on — and
/// refuses the call itself, its input read, where none is there (not set, a folder, a file that isn't executable) or
/// none answers (it fails, says nothing, or says something and fails): the hook always answers.
#[test]
fn the_script_asks_tarae_or_refuses() {
    use std::os::unix::fs::PermissionsExt;
    let dir = crate::scratch::Scratch::new("hook-script");
    let refused = |out: &str| {
        let v: Value = serde_json::from_str(out.trim()).unwrap_or_else(|_| panic!("one JSON line: {out:?}"));
        v["decision"] == "deny" && v["reason"].as_str().is_some_and(|r| r.contains("no answer from tarae"))
    };
    let input = String::from_utf8(agy_input(json!("mcp_tarae_search"))).unwrap();
    assert!(refused(&script(dir.path(), None, &input)), "not set");
    assert!(refused(&script(dir.path(), Some(dir.path()), &input)), "a folder");
    let tarae = dir.join("tarae");
    // (a stand-in tarae: says its arguments and what it read)
    std::fs::write(&tarae, "#!/bin/sh\nprintf '%s|' \"$@\"; cat\n").unwrap();
    std::fs::set_permissions(&tarae, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(refused(&script(dir.path(), Some(&tarae), &input)), "not executable");
    std::fs::set_permissions(&tarae, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(script(dir.path(), Some(&tarae), &input), format!("agent-hook|{input}\n"));
    // (a tarae that doesn't answer: refused all the same, with an answer)
    for (body, why) in [
        ("exit 1", "it fails"),
        ("cat >/dev/null; exit 0", "it says nothing"),
        ("printf '{\"decision\":\"allow\"}'; exit 1", "it says something and fails"),
        ("head -c 1 >/dev/null; kill -9 $$", "it dies before it answers"),
    ] {
        std::fs::write(&tarae, format!("#!/bin/sh\n{body}\n")).unwrap();
        assert!(refused(&script(dir.path(), Some(&tarae), &input)), "{why}");
    }
}

/// L-B3 (#6: agy 1.5.9 runs a hook's `command` through a shell): the hook file names its script shell-quoted, so a
/// state folder whose path holds a space, a quote, `$` or a backtick still runs it — the stand-in, which runs a hook
/// as agy does (`sh -c <command>`), reads each file `file` wrote and gets that script's answer — and a `$(touch …)` in
/// the path runs nothing: no marker (in this test's own folder). Each folder and the marker are under the scratch
/// folder.
#[test]
fn the_hook_files_command_is_the_scripts_path_shell_quoted() {
    use std::os::unix::fs::PermissionsExt;
    let dir = crate::scratch::Scratch::new("hook-quoted");
    let marker = dir.join("marker");
    let plain = dir.join("plain");
    // (`$(touch <marker>)`: the marker's absolute path makes folders under the scratch one, the `/`s in it)
    let names =
        ["plain", "sp ace", "it's", "do$HOME llar", "back`tick", "x'$(touch {m})'y", "z$(touch {m})z"];
    for (i, name) in names.iter().enumerate() {
        let folder = dir.join(name.replace("{m}", &marker.display().to_string()));
        let config = folder.join("config");
        std::fs::create_dir_all(&config).unwrap();
        let script = config.join(SCRIPT_NAME);
        let said = json!({"decision": "allow", "said": i});
        std::fs::write(&script, format!("#!/bin/sh\ncat >/dev/null\necho '{said}'\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let text = file(crate::agent::AGY_TOOLS_OFF.hook, script.to_str().unwrap());
        std::fs::write(config.join("hooks.json"), &text).unwrap();
        let answer = crate::agent::acp::standin::hook(folder.to_str().unwrap(), "", &json!({}));
        assert_eq!(answer["said"], i, "{name}: {answer}");
    }
    assert!(!marker.exists(), "a `$(…)` in the path ran");
    // (the usual folder's: the path in single quotes, nothing else)
    let text =
        file(crate::agent::AGY_TOOLS_OFF.hook, plain.join("config").join(SCRIPT_NAME).to_str().unwrap());
    let v: Value = serde_json::from_str(&text).unwrap();
    let want = format!("'{}/config/{SCRIPT_NAME}'", plain.display());
    assert_eq!(v[GROUP]["PreToolUse"][0]["hooks"][0]["command"], want.as_str());
    assert_eq!(quoted("a'b"), r"'a'\''b'");
}

/// A panic in the hook is a refusal on stdout, exit 0 (`on_panic`, in a process of its own: the test binary re-run on
/// `panicking`).
#[test]
fn a_panic_in_the_hook_is_a_refusal() {
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe)
        .args(["--exact", "agent::hook::panicking", "--ignored", "--nocapture", ARG])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert_eq!(stdout.lines().last(), Some(PANIC_LINE.trim_end()), "{stdout}");
}
