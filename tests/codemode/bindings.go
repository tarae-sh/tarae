// tarae's functions, in your program's own package (main): call them as they are, declare none of them again.
// Paths are relative to the project's root; lines start at 1. A call that fails or is refused returns its error.
package main

type Line struct {
	N    int
	Text string
}

type File struct {
	Path  string
	Total int
	Lines []Line
}

// Hit: Fn is the function or type the line is in ("" at the top level).
type Hit struct {
	Path string
	Line int
	Text string
	Fn   string
	Test bool
}

// Symbol: Kind "fn", "struct", "enum", "trait", "impl", "mod", "const", "field"…; Parent "" at the top level.
type Symbol struct {
	Name, Kind string
	Start, End int
	Parent     string
}

type Place struct {
	Path string
	Line int
	Text string
}

type Use struct {
	Path string
	Line int
	Text string
	Fn   string
	Test bool
}

// Diagnostic: Severity "error", "warning", "info", "hint".
type Diagnostic struct {
	Path     string
	Line     int
	Severity string
	Message  string
}

// Read returns the file's lines start..end (end 0: to its end).
func Read(path string, start, end int) (File, error) { return File{}, nil }

// Search returns the lines matching the regex pattern (Rust's syntax; literal: plain text) in path, a folder or
// a file ("": the whole project); tests false leaves out lines in test code.
func Search(pattern, path string, literal, tests bool) ([]Hit, error) { return nil, nil }

// Symbols returns the file's outline.
func Symbols(path string) ([]Symbol, error) { return nil, nil }

// Definition returns where symbol is defined ("Editor::open": Editor's own first), as used in path at line when
// given ("" and 0: not).
func Definition(symbol, path string, line int) ([]Place, error) { return nil, nil }

// References returns where the symbol written in path (at line; 0: its first) is used, its declaration first.
func References(symbol, path string, line int) ([]Use, error) { return nil, nil }

// Diagnostics returns the language servers' errors and warnings (no paths: every file open).
func Diagnostics(paths ...string) ([]Diagnostic, error) { return nil, nil }

// Git returns git's output for a command that only reads (log, show, diff, status, blame, ls-files, grep):
// Git("log", "-3", "--name-only").
func Git(args ...string) (string, error) { return "", nil }

// fmt.Println(...) is what comes back: only what the program prints.
