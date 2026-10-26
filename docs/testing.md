# Tests

Run the test under your cursor and see failures on the line that failed — for Rust, Go, Python, and Java. Which
tools each language needs: [Language support](languages.md).

## Tests

`space x`, then:

| Key | Does |
|---|---|
| `x` | Run the test at the cursor |
| `f` | Run this file's tests |
| `l` | Run the last test again |
| `o` | The run's whole output in a buffer, at the failure you were looking at (search it, copy from it) |
| `c` | Close the panel (stops a run that's still going) |

The wheel over the panel scrolls what it shows beside the list: the failure's message from its top, or the
output, which keeps following its end until you scroll up (and again once you're back at the end).

| Language | Runs with |
|---|---|
| Rust | `cargo test -- module::path::name --exact` (integration tests with `--test`) |
| Go | `go test -v -run '^TestX$'` (benchmarks with `-bench`) |
| Python | `pytest file::Class::function`, with the project's `.venv` or `$VIRTUAL_ENV` Python |
| Java | Gradle `./gradlew :module:test --tests pkg.Class.method` (nested classes as `Outer$Inner`) or Maven `./mvnw -pl module -Dtest=Class#method test` — JUnit 4/5 and TestNG |

**Finding tests** works on the tree-sitter tree — `#[test]`-style attributes and `mod` paths in Rust, `TestXxx` in Go,
`class Test…`/`def test…` in Python, `@Test`-style annotations in Java. If the cursor isn't inside a test, the enclosing
`mod tests` or test class runs; failing that, the whole file. If the language's grammar isn't installed yet, tarae asks
once — one `y` downloads it and runs the test right away ([screenshot](screenshots/grammar-needed.png)).

Modified files are saved before a run. **Results** fill a panel below the code: a header like
`FAILED  math::*   ● 1  ▲ 1   26 ms`, each test on the left as `●` passed · `▲` failed · `◦` skipped (shapes, not just
colors), and the selected failure's message and location on the right. The failing line gets a red `▲ message` at its
end; `]x` `[x` jump between failures.

Results are read from libtest output, `go test -v`, `pytest -v --tb=short`, and for Java the Gradle/Maven JUnit XML
reports. While results can't be read yet (still building, a compile error), the full output shows instead.

[Rust](screenshots/test-results-rust.png) · [Go](screenshots/test-results-go.png) ·
[Python](screenshots/test-results-python.png) · [Java](screenshots/test-results-java.png) ·
[run output](screenshots/test-run.png) · [Java build output](screenshots/test-run-java.png)

### Language packs

Tests are found and run through **language packs**: data, not code. A pack is a `pack.toml` and a tree-sitter
query, built in from [`runtime/packs/`](../runtime/packs) — the same folder that says what the language is
([Adding a language](languages.md#adding-a-language)). One of your own in `~/.local/share/tarae/packs/<language>/`
adds a language tarae doesn't know, or changes one it does — no rebuild. Yours replaces the built-in pack a table at
a time: a `[test]` of your own keeps the built-in `[tasks]`, and the other way round ([what a pack of yours can
change](#what-a-pack-of-yours-can-change)). A query file of yours replaces the built-in one of the same name.

```toml
# ~/.local/share/tarae/packs/go/pack.toml — run from the module root, with the race detector
[test]
query = "tests.scm"                  # captures each test (@test), group (@group) and their names (@name)
files = ["*_test.go"]                # which files hold tests
cwd = { nearest = ["go.mod"] }       # or "file-dir"; `or = [...]` for a second choice
run = ["go", "test", "-race", "-v", "-run", "^{name}$", "./..."]        # the test at the cursor
run-file = ["go", "test", "-race", "-v", "-run", "^({names})$", "./..."] # the file's tests, joined with |
none-ran = [0]                       # how a run that matched no test ends

# How the output is read, a line at a time: the first rule that matches acts
[[test.line]]
match = '^\s*=== RUN\s+(?<name>\S+)'  # what follows is about this test
begin = true

[[test.line]]
match = '^\s*--- (?<status>PASS|FAIL|SKIP): (?<name>\S+)(?: \((?<secs>[\d.]+)s\))?'
status = { PASS = "passed", FAIL = "failed", SKIP = "skipped" }

[[test.line]]
match = '^    \s*(?<file>[^\s:]+\.go):(?<line>\d+): (?<message>.*)$'  # where it failed, and why
```

- **Three scopes.** The test at the cursor (`run`, `label`); the group around the cursor when it's in no test, like
  a `mod tests` or a `class Test…` (`run-group`, `label-group`); and the file (`run-file`, `label-file`).
- **Variables:**
  - `{name}` — the test
  - `{groups}` — the groups around it, joined with `::` (`group-sep` to change)
  - `{path}` — groups and name
  - `{groups/.}`, `{path/.}` — the same, joined with what follows the `/`
  - `{path|re}` — any of them escaped for a regular expression (`-t "^{path|re}$"`)
  - `{first-group}` — the file's first group (its class)
  - `{names}` — the file's tests
  - `{cwd}`, `{file}`, `{filename}`, `{rel}` (the file from `{cwd}`)
- **Helpers.** An argument that is just a list variable becomes its items, and one that ends up empty is left out.
  `helpers = ["cargo"]` adds `{cargo-target}` (`--lib`, `--bin x`, `--test x`) and `{module}` (the file's module
  path).
- **Variables of the pack's own** (`vars`, a list, in order — each may use those before it): how a build's layout
  is said. The [Java pack](../runtime/packs/java/pack.toml) works out `{java-root}` (the top of a multi-module
  build), the module, Gradle or Maven, the wrapper or the tool, the module's task and `{package}` with them:
  - `{ name = "root", topmost = ["pom.xml"] }` / `nearest = [...]` — the folder holding one, the topmost below the
    repository or the nearest up (`"roots"`: the language's `roots`); none: `else = "{…}"`, or the error `missing`
  - `{ name = "tool", first = ["{root}/gradlew", "gradle"] }` — the first that's a file, else a program name;
    `~/` and `${VAR}` are expanded, and one naming a variable that isn't set is passed over (Python's `{python}`:
    `["{cwd}/.venv/bin/python", "${VIRTUAL_ENV}/bin/python", "python3"]`)
  - `{ name = "module", parts = "{module-dir}", from = "{root}" }` — the folders between: `{module}` is `a/b`,
    `{module/:}` is `a:b`
  - `{ name = "x", if = "{module}", then = …, else = … }` (not empty), or `if-exists = "{root}/pom.xml"` — each side
    a template or a list (`then = ["-pl", "{module}"]`)
  - The test query's `@var.<name>` captures are variables too (Java's `@var.package-name`: the tree's `package`).
  `cwd` can be a template: `cwd = "{java-root}"`.
- **Variants.** `variant-by = "{java-build}"` picks `[test.variant.gradle]` or `[test.variant.maven]`, whose
  settings override the rest; a variant's own `vars` are worked out once it's picked.
- **Reading the output.** `[[test.line]]` rules read it a line at a time, as it comes; each line tries them in
  order and the first whose `match` matches acts, by its named groups — `name` (the test), `status`, `secs`,
  `file` and `line` (where it failed), `message` (a line of why):
  - `status = { ok = "passed", FAILED = "failed" }` by the `status` group's text (passed · failed · skipped), or
    `status = "skipped"` whatever the line
  - `begin = true`: the lines after are about the test this one names (libtest's `---- a::b stdout ----`); `end =
    true`: about none. A rule with no `name` group acts on that test. `known = true`: only a test already listed
    (by name, or as the end of one's `::` name) — a heading for no test is nobody's; `name-replace = { "." = "::"
    }` turns its name into the one the results use
  - `phase = "failures"`: the rule counts only in that part of the output (`""`: outside every one); `to = "…"`
    starts one (pytest's `FAILURES` block)
  - `place = "last"`: a later place replaces this one until the test has a message (a traceback's last frame);
    otherwise the first place that's a file stays
  - `none-ran = [0, 5]`: a run that listed no test and ended with one of these ran none — said as such, not passed

  The [Rust](../runtime/packs/rust/pack.toml) (libtest), Go and [Python](../runtime/packs/python/pack.toml)
  (pytest) packs read theirs this way. A test is failed from when it's first listed until a line says otherwise;
  one that said it skipped stays skipped when the runner calls it a pass.
- **JUnit XML.** With `parse = "junit"`, results come from the reports written to `reports` during the run, and
  failures point into `sources` (through a Java stack, or the first `path:line` in the failure).
- **Kinds.** A query pattern can give its tests a kind with `(#set! kind "bench")`. `[test.kind.bench]` then has
  settings of its own, and `file = false` keeps those tests out of a file's run. `@start` makes a test begin
  earlier, at its attributes.

The built-in [Go](../runtime/packs/go), [Rust](../runtime/packs/rust), [Python](../runtime/packs/python) and
[Java](../runtime/packs/java) packs show it all.

#### A pack of your own: vitest

TypeScript has no built-in tests. Two files add vitest (`it`/`test` in `describe`), with results read from its JUnit
report:

```toml
# ~/.local/share/tarae/packs/typescript/pack.toml — vitest
[test]
query = "tests.scm"
files = ["*.test.ts", "*.spec.ts"]
cwd = { nearest = ["package.json"] }
parse = "junit"
reports = ["{cwd}/node_modules/.cache/tarae"]
sources = ["{cwd}"]
group-sep = " > "                    # vitest 5's full name for a test: `math > edge > fails`
run = ["npx", "vitest", "run", "{rel}", "-t", "^{path|re}$", "--reporter=default", "--reporter=junit", "--outputFile.junit=node_modules/.cache/tarae/junit.xml"]
run-group = ["npx", "vitest", "run", "{rel}", "-t", "^{groups|re} > ", "--reporter=default", "--reporter=junit", "--outputFile.junit=node_modules/.cache/tarae/junit.xml"]
run-file = ["npx", "vitest", "run", "{rel}", "--reporter=default", "--reporter=junit", "--outputFile.junit=node_modules/.cache/tarae/junit.xml"]
label = "{path}"                     # as the report names it
```

```scheme
; ~/.local/share/tarae/packs/typescript/tests.scm
((call_expression
   function: [(identifier) @_f (member_expression object: (identifier) @_f)]
   arguments: (arguments . (string (string_fragment) @name))) @test
 (#any-of? @_f "it" "test"))

((call_expression
   function: [(identifier) @_f (member_expression object: (identifier) @_f)]
   arguments: (arguments . (string (string_fragment) @name))) @group
 (#eq? @_f "describe"))
```

`-t` takes a regular expression; `|re` escapes the name for one, so `adds (1+1)?` runs just that test. Failures
land on their line: vitest's report has it (`❯ src/math.test.ts:10:21`). For `.js` files, the same two files go in
`packs/javascript/`.

### Installing from packs

A pack also says how to get what its language needs, for tarae to [offer](languages.md#when-one-is-missing) when
it's missing:

```toml
# ~/.local/share/tarae/packs/go/pack.toml — gopls your way; other tools' built-in recipes stay
[install.gopls]                      # the program, as it's run
what = "Go's language server"
ways = [                             # the first whose program (or `needs`) is on PATH
    { run = ["go", "install", "golang.org/x/tools/gopls@latest"], bin = ["~/go/bin"] },
    { run = ["brew", "install", "gopls"] },
]
# hint = "…"                         # what to say when no way fits
```

`bin` is where it lands when that isn't on `PATH` (`~` and `${VAR}` work — a list like `GOPATH` gives its first);
tarae looks for the tool there too. A recipe of your own replaces the built-in one for that tool only. A way can
also be a release tarae downloads and checks itself — see [Downloads](languages.md#downloads).

### Tasks from packs

The build tasks tarae suggests (`space m p`) come from the packs too: `[tasks]` says which files make a project of
that kind, and what to run.

```toml
# ~/.local/share/tarae/packs/rust/pack.toml — replaces the built-in Rust tasks
[tasks]
when = ["Cargo.toml"]                # the project's root has one of these
rank = 10                            # where they go in the list, lower first (50 unless set)

[[tasks.task]]
name = "cargo check"
cmd = "cargo check --all-targets --message-format=short"   # run by sh -c

[[tasks.task]]
name = "nextest"
cmd = "cargo nextest run"
needs = "cargo-nextest"              # only when it's on PATH
```

`each = { json = "package.json", keys = "scripts" }` makes a task for each script, as `{key}` in `name` and `cmd`
(quoted for the shell there; the built-in [JavaScript pack](../runtime/packs/javascript) does). Tasks of your own in
the config come first ([Build tasks](configuration.md#build-tasks)).

`:preflight`'s default steps — for a project with none of its own ([Checks before a
push](configuration.md#checks-before-a-push-preflight)) — come from `[[preflight]]`, one entry per kind of project
(Java has two: Maven, Gradle):

```toml
# ~/.local/share/tarae/packs/go/pack.toml — replaces the built-in Go preflight defaults
[[preflight]]
when = ["go.mod"]
rank = 20                            # several kinds of project: their steps together, lower rank first
steps = [
  { name = "go vet", cmd = "go vet ./..." },
  { name = "go test", cmd = "go test ./..." },
]
```

`unless` holds an entry back when one of its files is there too, and `held` says why when nothing else runs. The
Java pack's Gradle entry has `unless = ["gradlew"]`: a project with its own wrapper lists it in its own
`[preflight]`.

What runs later outside Claude's sandbox — build scripts, manifests, wrappers — each pack lists in `protect`:
Claude's commands can't write these and its edits of them always ask. Every built-in pack's names apply in every
project, whatever its languages, and a pack of your own can't take one out (`llm.review-ask` adds yours):

```toml
# runtime/packs/rust/pack.toml
protect = ["build.rs", "Cargo.toml", ".cargo"]   # names, anywhere in the project, in any case;
                                                 # "gradle/wrapper": those names in a row
outputs = ["target"]                             # tools' output folders: the file walks skip them
first-made = ["target/", "Cargo.lock"]           # what a first build makes beside those names
```

On Linux without a mount namespace of the run's own (tiers host and offline; in tier own each protected name is
bound read-only over itself instead, and every folder takes new files) a folder holding a protected name takes no
new entry from Claude's commands (a folder there may only go, once emptied — `cargo clean`), so a fresh clone's first
build couldn't make `target/` beside `Cargo.toml`: before a command runs, tarae makes the pack's `first-made`,
empty — a folder (written with a `/`) in every folder holding one of its names, a file (written in place, as
`Cargo.lock` is) in the outermost — then takes away what's still empty when the command is over. A name that
is, or would hold, a protected one is never made. That's all `first-made` does: the walks go by `outputs` alone
(Java's `build/` is made first, but a repository whose `build/` holds sources is still searched).

A run can wait on something that isn't its own work — another cargo holding the build directory. `[[waits]]` names
the lines that say so: such a line is the run's state, not output (the task or test panel shows `waiting for cargo's
lock · 0:37` until the next line), and Claude's report tells the wait from the run (`Waited 16.0s for cargo's build
directory lock. Ran 11.0s.`). Every pack's apply to every run; a language that declares none has none.

```toml
# runtime/packs/rust/pack.toml
[[waits]]
line = '^\s*Blocking waiting for file lock on (?<what>.+)$'   # its named groups go into the texts as {what}
for = "cargo's lock"                  # the panel: waiting for …
report = "cargo's {what} lock"        # Claude's report (`for` unless set)
```

A task's places come the same way. Every tool's `path:line[:col]: [severity] message` is tarae's own; a form one
tool has beyond it is its pack's `[[places]]` — `match` with groups `path`, `line`, `col` (or `place`, a
`path:line:col` text), `severity`, `message`; `then` when the place comes on a later line; `message = "…"` for one
of its own. A rule whose place isn't a file lets the next try. `checks` names the commands whose places a run of
Claude's puts in your list (`]q`); `color-off` what turns a tool's colours off in the runs tarae reads.

```toml
# runtime/packs/rust/pack.toml
checks = ["cargo build", "cargo check", "cargo test", "cargo clippy", "cargo nextest", "cargo fmt", "cargo perf"]
color-off = { CARGO_TERM_COLOR = "never" }    # beside NO_COLOR=1, in every task and test run

# rustc's long form: the message, then where (` --> src/a.rs:3:5`) a few lines on
[[places]]
match = '^(?<severity>error|warning)(?:\[[^\]]*\])?: ?(?<message>.*)$'
then = '^--> (?<place>.+)$'
```

### What a pack of yours can change

Each top-level key of a `pack.toml` says how a pack of your own counts for it:

| Keys | A pack of yours |
|---|---|
| `[test]`, `[tasks]`, `[[preflight]]`, `[[waits]]`, `[[places]]` | replaces the built-in table |
| `[install.<tool>]`, `[grammar.<name>]` | replaces that entry; the others stay |
| `[language.<name>]`, `[server.<name>]` | replaces the keys it sets; the entry's other keys stay — in whichever folder the language is |
| `config-files`, `checks` | adds to the built-in list; nothing can be taken out |
| `outputs`, `first-made`, `protect`, `color-off`, `[builds]` | ignored, and said at the start: tarae reads them from its own packs only (they decide what Claude's runs may reach or make, or what's in their environment, and — `[builds]`: where a checkout builds — what a session's end may delete) |

A key no pack knows is a mistake in that pack, said with its name as one that doesn't parse is. Your
`[language]`, `[grammar]`, `[server]` and `checks` are read once, when tarae starts — after editing them,
`:restart`. `[test]`, `[tasks]`, `[[preflight]]`, `[[waits]]`, `[[places]]` and `[install]` are read when they're
used, so they count at once.
