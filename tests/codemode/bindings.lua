-- tarae's functions, there in your program as globals: nothing to require.
-- Paths are relative to the project's root; lines start at 1.
-- A call that fails or is refused returns { error = "why" } instead of its result: check with `r.error`.

---@class Line { n: integer, text: string }
---@class File { path: string, total: integer, lines: Line[] }
---@class Hit { path: string, line: integer, text: string, fn: string, test: boolean } -- fn: the function or type the line is in ("" at the top level)
---@class Symbol { name: string, kind: string, start: integer, ["end"]: integer, parent: string } -- kind "fn", "struct", "enum", "trait", "impl", "mod", "const", "field"…; parent "" at the top level
---@class Place { path: string, line: integer, text: string }
---@class Use { path: string, line: integer, text: string, fn: string, test: boolean }
---@class Diagnostic { path: string, line: integer, severity: string, message: string } -- severity "error", "warning", "info", "hint"

--- The file's lines start..end (end 0: to its end).
---@return File
function read(path, start, end_) end

--- Lines matching the regex `pattern` (Rust's syntax; literal true: plain text) in `path`, a folder or a file
--- ("": the whole project). tests false leaves out lines in test code.
---@return Hit[]
function search(pattern, path, literal, tests) end

--- The file's outline.
---@return Symbol[]
function symbols(path) end

--- Where `symbol` is defined ("Editor::open": Editor's own first), as used in `path` at `line` when given.
---@return Place[]
function definition(symbol, path, line) end

--- Where the symbol written in `path` (at `line`) is used, its declaration first.
---@return Use[]
function references(symbol, path, line) end

--- The language servers' errors and warnings (paths nil: every file open).
---@return Diagnostic[]
function diagnostics(paths) end

--- git's output for a command that only reads (log, show, diff, status, blame, ls-files, grep):
--- git({ "log", "-3", "--name-only" }).
---@return string
function git(args) end

-- print(...) is what comes back: only what the program prints.
