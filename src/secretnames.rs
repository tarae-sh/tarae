//! File names that look like keys and tokens — the shared rules' list (`tarae_plugin_rules::secretnames`, MIT OR
//! Apache-2.0 like the SDK, so a plugin's test holds a file to the same list): never copied for recovery
//! (`recovery.rs`), never read or found through a plugin's `project` permission (`plugin/perm.rs`, `plugin/io.rs`),
//! closed to Claude's commands inside the project (`sandbox::key_regexes` on macOS, `sandbox::linux::there` on
//! Linux).

pub use tarae_plugin_rules::secretnames::*;
