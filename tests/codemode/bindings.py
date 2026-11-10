# tarae's functions, there in your program as built-ins: nothing to import.
# Paths are relative to the project's root; lines start at 1.
# A call that fails or is refused returns {"error": "why"} instead of its result: check with `"error" in r`.

def read(path: str, start: int = 1, end: int = 0) -> dict:
    """The file's lines start..end (end 0: to its end).
    {"path": str, "total": int, "lines": [{"n": int, "text": str}]}"""

def search(pattern: str, path: str = "", literal: bool = False, tests: bool = True) -> list:
    """Lines matching the regex `pattern` (Rust's syntax; literal=True: plain text) in `path`, a folder or a
    file ("": the whole project). tests=False leaves out lines in test code.
    [{"path": str, "line": int, "text": str, "fn": str, "test": bool}] — `fn`: the function or type the line
    is in ("" at the top level)."""

def symbols(path: str) -> list:
    """The file's outline: [{"name": str, "kind": str, "start": int, "end": int, "parent": str}] — kind
    "fn", "struct", "enum", "trait", "impl", "mod", "const", "field"…; parent "" at the top level."""

def definition(symbol: str, path: str = "", line: int = 0) -> list:
    """Where `symbol` is defined ("Editor::open": Editor's own first), as used in `path` at `line` when given:
    [{"path": str, "line": int, "text": str}]"""

def references(symbol: str, path: str, line: int = 0) -> list:
    """Where the symbol written in `path` (at `line`) is used, its declaration first:
    [{"path": str, "line": int, "text": str, "fn": str, "test": bool}]"""

def diagnostics(paths: list = None) -> list:
    """The language servers' errors and warnings (paths None: every file open):
    [{"path": str, "line": int, "severity": str, "message": str}] — severity "error", "warning", "info", "hint"."""

def git(args: list) -> str:
    """git's output for a command that only reads (log, show, diff, status, blame, ls-files, grep):
    git(["log", "-3", "--name-only"])."""

# print(...) is what comes back: only what the program prints.
