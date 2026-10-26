//! tarae's plugin API on the runtime: `abi` (its types, and loading and calling one plugin, on the canonical ABI —
//! `cabi`) and `host` (the plugin thread the editor talks to by messages and callbacks).

pub mod abi;
mod cabi;
pub mod host;
