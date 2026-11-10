// tarae's functions, there in your program as globals: nothing to import.
// Paths are relative to the project's root; lines start at 1.
// A call that fails or is refused returns { error: "why" } instead of its result: check with `"error" in r`.

/** The file's lines start..end (end 0: to its end).
 * @returns {{ path: string, total: number, lines: { n: number, text: string }[] }} */
function read(path, start = 1, end = 0) {}

/** Lines matching the regex `pattern` (Rust's syntax; literal true: plain text) in `path`, a folder or a file
 * ("": the whole project). tests false leaves out lines in test code. `fn`: the function or type the line is in
 * ("" at the top level).
 * @returns {{ path: string, line: number, text: string, fn: string, test: boolean }[]} */
function search(pattern, path = "", literal = false, tests = true) {}

/** The file's outline — kind "fn", "struct", "enum", "trait", "impl", "mod", "const", "field"…; parent "" at the
 * top level.
 * @returns {{ name: string, kind: string, start: number, end: number, parent: string }[]} */
function symbols(path) {}

/** Where `symbol` is defined ("Editor::open": Editor's own first), as used in `path` at `line` when given.
 * @returns {{ path: string, line: number, text: string }[]} */
function definition(symbol, path = "", line = 0) {}

/** Where the symbol written in `path` (at `line`) is used, its declaration first.
 * @returns {{ path: string, line: number, text: string, fn: string, test: boolean }[]} */
function references(symbol, path, line = 0) {}

/** The language servers' errors and warnings (paths null: every file open) — severity "error", "warning",
 * "info", "hint".
 * @returns {{ path: string, line: number, severity: string, message: string }[]} */
function diagnostics(paths = null) {}

/** git's output for a command that only reads (log, show, diff, status, blame, ls-files, grep):
 * git(["log", "-3", "--name-only"]).
 * @returns {string} */
function git(args) {}

// console.log(...) is what comes back: only what the program prints.
