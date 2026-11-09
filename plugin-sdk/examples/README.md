# Plugin examples

Whole plugins, one idea each. `tarae plugin example <name>` prints one; `tarae plugin new <dir> --from <name>` (or
`:plugin-new`) starts a crate from it. The API: [docs/plugins-reference.md](../../docs/plugins-reference.md).

- [`sort-lines`](sort-lines/src/lib.rs) — a command over the selections
- [`yaml-tabs`](yaml-tabs/src/lib.rs) — an event with a language filter
- [`todo-hints`](todo-hints/src/lib.rs) — hints tied to a buffer version
- [`word-tally`](word-tally/src/lib.rs) — a panel with its own keys
- [`md-outline`](md-outline/src/lib.rs) — a side tab
- [`read-notes`](read-notes/src/lib.rs) — reading a file under a `read` pattern
- [`git-branch`](git-branch/src/lib.rs) — a program run with `run(prog)`
- [`http-status`](http-status/src/lib.rs) — an HTTP call through the SDK
- [`final-newline`](final-newline/src/lib.rs) — a before-save check
- [`stamps`](stamps/src/lib.rs) — state kept with `write-own`
