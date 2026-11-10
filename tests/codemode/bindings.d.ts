// tarae's functions, there in your program as globals: nothing to import.
// Paths are relative to the project's root; lines start at 1.
// A call that fails or is refused returns { error: "why" } instead of its result: check with `"error" in r`.

interface Failed { error: string }
interface Line { n: number; text: string }
interface File { path: string; total: number; lines: Line[] }
/** `fn`: the function or type the line is in ("" at the top level). */
interface Hit { path: string; line: number; text: string; fn: string; test: boolean }
/** kind "fn", "struct", "enum", "trait", "impl", "mod", "const", "field"…; parent "" at the top level. */
interface Symbol { name: string; kind: string; start: number; end: number; parent: string }
interface Place { path: string; line: number; text: string }
interface Use { path: string; line: number; text: string; fn: string; test: boolean }
interface Diagnostic { path: string; line: number; severity: "error" | "warning" | "info" | "hint"; message: string }

/** The file's lines start..end (end 0: to its end). */
declare function read(path: string, start?: number, end?: number): File | Failed;
/** Lines matching the regex `pattern` (Rust's syntax; literal true: plain text) in `path`, a folder or a file
 * ("": the whole project). tests false leaves out lines in test code. */
declare function search(pattern: string, path?: string, literal?: boolean, tests?: boolean): Hit[] | Failed;
/** The file's outline. */
declare function symbols(path: string): Symbol[] | Failed;
/** Where `symbol` is defined ("Editor::open": Editor's own first), as used in `path` at `line` when given. */
declare function definition(symbol: string, path?: string, line?: number): Place[] | Failed;
/** Where the symbol written in `path` (at `line`) is used, its declaration first. */
declare function references(symbol: string, path: string, line?: number): Use[] | Failed;
/** The language servers' errors and warnings (no paths: every file open). */
declare function diagnostics(paths?: string[]): Diagnostic[] | Failed;
/** git's output for a command that only reads (log, show, diff, status, blame, ls-files, grep):
 * git(["log", "-3", "--name-only"]). */
declare function git(args: string[]): string | Failed;

// console.log(...) is what comes back: only what the program prints.
