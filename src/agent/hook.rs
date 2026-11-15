//! `tarae agent-hook`: guard 1's hook (agents beside Claude, step 3). An agent whose own tools tarae turns off (its
//! row's `Recipe`) runs it before each tool call, from the hook file tarae writes in its folder (`agentgate.rs`):
//! the call on stdin, the answer one JSON line on stdout. It allows only tarae's own tools (`mcp_tarae_<tool>`, as
//! agy names them) and refuses every other — the agent's own, another MCP server's, input it can't read, a panic.
//!
//! Two files, the same bytes from every tarae and every start (the design session's condition 2): the hook file
//! names `SCRIPT` beside it by its absolute path, and `SCRIPT` asks the tarae that started the agent — its own path
//! in the agent's environment as `ENV` — or, with none there, refuses the call itself. Two taraes at different paths
//! write the same files, and a hook with no tarae behind it refuses (tarae's tools too: broken in plain sight, never
//! open).
//!
//! It reads no config, touches no state and opens no editor: `main` looks at it first. It logs nothing of its input,
//! which holds the user's last message, the conversation's id and paths.

use std::io::{Read, Write};

use serde_json::{Value, json};

/// What tarae's own path is put in the agent's environment as (`acp::command`).
pub const ENV: &str = "TARAE_AGENT_HOOK";

/// Its word on tarae's command line: `tarae agent-hook`.
pub const ARG: &str = "agent-hook";

/// The most of the input it reads; past it, the call is refused.
pub const INPUT_MAX: usize = 1 << 20;

/// The hook file's program, beside it.
pub const SCRIPT_NAME: &str = "tarae-hook.sh";

/// The hook file's program: the tarae in `ENV` asked, else the call refused here. No JSON is read in `sh`; its
/// input is read to its end before the refusal, as tarae's own hook does.
pub const SCRIPT: &str = r#"#!/bin/sh
# tarae's hook for an agent it starts, written by tarae: the same at every start, from every tarae.
# The tarae that started the agent answers (its path in TARAE_AGENT_HOOK); with none there, every call is refused.
h=$TARAE_AGENT_HOOK
if [ -n "$h" ] && [ -f "$h" ] && [ -x "$h" ]; then
  exec "$h" agent-hook
fi
cat >/dev/null
printf '%s\n' "{\"decision\":\"deny\",\"reason\":\"tarae's hook has no tarae to ask — refused\"}"
"#;

/// The hook group's name in the hook file.
pub const GROUP: &str = "tarae";

/// Why another tool is refused — the agent reads it, and may say it.
pub const DENY: &str = "tarae runs every tool here — use tarae's own (mcp_tarae_…)";

/// Why a call it can't read is refused.
pub const UNREAD: &str = "tarae's hook couldn't read this call — refused";

/// The answer to a panic, written as it is (`on_panic`): `deny(UNREAD)`'s line.
const PANIC_LINE: &str =
    "{\"decision\":\"deny\",\"reason\":\"tarae's hook couldn't read this call — refused\"}\n";

/// The hook file for `hook`, naming `script` (an absolute path).
pub fn file(hook: super::Hook, script: &str) -> String {
    match hook {
        super::Hook::Agy { timeout } => {
            let one = json!({"type": "command", "command": script, "timeout": timeout});
            let file = json!({GROUP: {"PreToolUse": [{"matcher": "*", "hooks": [one]}]}});
            serde_json::to_string_pretty(&file).unwrap_or_default() + "\n"
        }
    }
}

fn deny(reason: &str) -> Value {
    json!({"decision": "deny", "reason": reason})
}

/// The whole rule: a call named `mcp_tarae_<tool>` is allowed; any other name, no name, or input that isn't a call
/// it can read (past `INPUT_MAX`, not JSON) is refused.
pub fn decide(input: &[u8]) -> Value {
    if input.len() > INPUT_MAX {
        return deny(UNREAD);
    }
    let Ok(call) = serde_json::from_slice::<Value>(input) else { return deny(UNREAD) };
    match call["toolCall"]["name"].as_str() {
        Some(name) if name.strip_prefix(super::acp::OWN).is_some_and(|tool| !tool.is_empty()) => {
            json!({"decision": "allow"})
        }
        Some(_) => deny(DENY),
        None => deny(UNREAD),
    }
}

/// `decide` on what `input` gives, read to `INPUT_MAX` and a byte: the answer's line, without its newline.
pub fn answer(input: impl Read) -> String {
    let mut buf = Vec::new();
    let answer = match input.take(INPUT_MAX as u64 + 1).read_to_end(&mut buf) {
        Ok(_) => decide(&buf),
        Err(_) => deny(UNREAD),
    };
    answer.to_string()
}

/// A panic from here on is a refusal: its line written straight to stdout and the process ended 0, before any
/// unwinding — so the same where a panic aborts (a release build).
pub fn on_panic() {
    std::panic::set_hook(Box::new(|_| {
        // SAFETY: a write of a constant's bytes to descriptor 1, then the process's end; nothing else is touched.
        unsafe {
            libc::write(1, PANIC_LINE.as_ptr().cast(), PANIC_LINE.len());
            libc::_exit(0)
        }
    }));
}

/// `tarae agent-hook` (`main.rs`, its first look): the call on stdin answered on stdout, exit 0 whatever it was.
pub fn cli() -> ! {
    on_panic();
    let line = answer(std::io::stdin().lock());
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}").and_then(|_| out.flush());
    std::process::exit(0)
}

/// The hook as a test build's program runs it (`agentgate::program`: the test binary re-run on this one test, `ARG`
/// among its arguments): `answer` on stdin, written to descriptor 3, the agent's pipe (libtest's own lines go to
/// /dev/null).
#[test]
#[ignore = "the agent's hook in a test build: a stand-in agent's child, never run alone"]
fn child() {
    use std::os::fd::FromRawFd;
    if !std::env::args().any(|a| a == ARG) {
        return;
    }
    // SAFETY: descriptor 3 is the agent's pipe, as the program's `3>&1` gave it; nothing else here owns it.
    let mut out = unsafe { std::fs::File::from_raw_fd(3) };
    let line = answer(std::io::stdin().lock());
    let _ = writeln!(out, "{line}");
}

/// The test of `on_panic` in a process of its own (it sets the process's panic hook): a panic after it, `ARG` among
/// its arguments.
#[test]
#[ignore = "the hook's panic, in a process of its own: a test's child, never run alone"]
fn panicking() {
    if !std::env::args().any(|a| a == ARG) {
        return;
    }
    on_panic();
    panic!("a panic in the hook");
}

#[cfg(test)]
mod tests;
