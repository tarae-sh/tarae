//! What a warning holds for, read from its data: a command's words (after `NAME=value` and a wrapper), the
//! scripts it runs, what's set around it, its options, the checked forms that excuse it, the core's facts.

use std::path::PathBuf;

use super::*;

/// The warnings a `[[claude.warn]]` text says (as one pack's, `tools pack`) — none of them wrong.
fn warns(text: &str) -> Vec<Warn> {
    let t: toml::Table = text.parse().unwrap();
    let mut problems = Vec::new();
    let w = read(&t["warn"], "tools pack", &mut problems);
    assert!(problems.is_empty(), "{problems:?}");
    w
}

/// What `cmd` (running `scripts`) is warned of, nothing set in its environment unless `env` says so.
#[derive(Default)]
struct Judge<'a> {
    env: &'a [&'a str],
    scripts: Vec<(String, String)>,
    reads: Vec<PathBuf>,
    outside: bool,
    script_changed: bool,
    kept: Vec<String>,
}

impl Judge<'_> {
    fn ids(&self, w: &[Warn], cmd: &str) -> Vec<String> {
        self.judged(w, cmd).into_iter().map(|w| w.id).collect()
    }

    fn judged(&self, w: &[Warn], cmd: &str) -> Vec<Warned> {
        let env = |n: &str| self.env.contains(&n);
        // (the packs' own — `timeout`, `env`, `nice` …)
        let wrappers = &crate::sandboxpack::get().wrappers;
        let in_outputs = |p: &str| p.starts_with("target/") && !p.contains("..");
        judge(
            w,
            &Facts {
                cmd,
                scripts: &self.scripts,
                env: &env,
                wrappers,
                reads: &self.reads,
                script_changed: self.script_changed,
                edit_protected: false,
                outside: self.outside,
                kept: &self.kept,
                in_outputs: &in_outputs,
            },
        )
    }
}

const DOCKER: &str = r#"
[[warn]]
id = "docker-own-config"
run = ["docker", "docker compose"]
missing = [
  { env = "DOCKER_CONFIG", say = "no DOCKER_CONFIG" },
  { env = "DOCKER_HOST", say = "no DOCKER_HOST" },
  { wrapper = ["timeout", "gtimeout"], say = "no timeout" },
]
say = "Docker: {missing}."
"#;

/// A command is its leading words — after `NAME=value` and a wrapper (`timeout -k 5 600 docker …`), never a word
/// another command says (`echo docker`); each of `missing` it lacks is named, and none lacking is no warning:
/// set in front of it, by an earlier `export`, around the script it's in, or in the run's environment.
#[test]
fn what_a_command_lacks_is_named() {
    let w = warns(DOCKER);
    let j = Judge::default();
    let said = j.judged(&w, "docker run --rm alpine");
    assert_eq!(said[0].said, "Docker: no DOCKER_CONFIG; no DOCKER_HOST; no timeout.");
    assert_eq!(said[0].line(), "Docker: no DOCKER_CONFIG; no DOCKER_HOST; no timeout. (tools pack)");
    assert!(j.ids(&w, "DOCKER_CONFIG=/x DOCKER_HOST=unix:///y timeout 600 docker run alpine").is_empty());
    assert!(j.ids(&w, "DOCKER_CONFIG=/x DOCKER_HOST=y timeout -k 5 600 docker compose up").is_empty());
    let said = j.judged(&w, "DOCKER_HOST=y timeout 600 docker run alpine");
    assert_eq!(said[0].said, "Docker: no DOCKER_CONFIG.");
    assert!(
        j.ids(&w, "export DOCKER_CONFIG=/x DOCKER_HOST=y; timeout 60 docker ps").is_empty(),
        "set before it"
    );
    let said = j.judged(
        &w,
        "DOCKER_CONFIG=/x DOCKER_HOST=y timeout 5 true; DOCKER_CONFIG=/x DOCKER_HOST=y docker ps",
    );
    assert_eq!(said[0].said, "Docker: no timeout.", "an earlier command's wrapper wraps nothing of this one");
    let set = Judge { env: &["DOCKER_CONFIG", "DOCKER_HOST"], ..Judge::default() };
    assert!(set.ids(&w, "timeout 60 docker ps").is_empty(), "in the run's environment");
    for not in ["echo docker run", "dockerd", "grep docker notes.txt", "cat docker-compose.yml"] {
        assert!(j.ids(&w, not).is_empty(), "{not}");
    }
    // (any command of the line — after a pipe, in an `sh -c`, after `&&`)
    for cmd in ["cd x && docker ps", "true | docker ps", "sh -c 'docker ps'", "if docker ps; then :; fi"] {
        assert_eq!(j.ids(&w, cmd), ["docker-own-config"], "{cmd}");
    }
}

/// What runs in a script runs too: a script's commands are judged, with what the line sets and wraps around
/// it (`DOCKER_HOST=… timeout 600 sh run.sh`) counted.
#[test]
fn a_scripts_commands_are_judged_with_the_line_around_them() {
    let w = warns(DOCKER);
    let j =
        Judge { scripts: vec![("run.sh".into(), "set -eu\ndocker build .\n".into())], ..Judge::default() };
    assert_eq!(j.ids(&w, "sh run.sh"), ["docker-own-config"]);
    assert!(j.ids(&w, "DOCKER_CONFIG=/x DOCKER_HOST=y timeout 600 sh run.sh").is_empty());
    let set_inside = Judge {
        scripts: vec![(
            "run.sh".into(),
            "export DOCKER_CONFIG=/x\nDOCKER_HOST=y timeout 60 docker build .\n".into(),
        )],
        ..Judge::default()
    };
    assert!(set_inside.ids(&w, "sh run.sh").is_empty(), "set in the script before it");
    // (a comment says nothing)
    let comment =
        Judge { scripts: vec![("c.sh".into(), "# docker build .\necho hi\n".into())], ..Judge::default() };
    assert!(comment.ids(&w, "sh c.sh").is_empty());
}

/// `options`, read as options are: a short one in a cluster (`-af`), a long one as itself or with `=`, BSD's
/// dashless form only as the first word (`ps e`, `ps auxe`) — never a longer option that starts the same
/// (`--fullname`), nor an option's value (`ps -o user`: `user` has an `e`, and it's `-o`'s); none of them: no
/// warning.
#[test]
fn an_option_counts_in_a_cluster() {
    let w = warns(
        r#"
[[warn]]
id = "process-details"
run = ["pgrep", "lsof"]
options = ["-f", "-a", "-p", "--full"]
say = "It reads other processes' details."
[[warn]]
id = "ps-env"
run = ["ps"]
options = ["-E", "e"]
option-values = ["-o", "-p", "-u"]
say = "It reads other processes' environments."
"#,
    );
    let j = Judge::default();
    for cmd in ["pgrep -f tarae", "pgrep -af x", "pgrep --full x", "lsof -p 12", "lsof -nP -p 12"] {
        assert_eq!(j.ids(&w, cmd), ["process-details"], "{cmd}");
    }
    for cmd in ["ps -E", "ps e", "ps auxe", "ps -xE", "ps -p 12 -E"] {
        assert_eq!(j.ids(&w, cmd), ["ps-env"], "{cmd}");
    }
    for cmd in [
        "pgrep tarae",
        "lsof -i :80",
        "ps aux",
        "ps -ef",
        "pgrep --fullname x",
        "ps -o user -p 12",
        "ps -p 12 -o pid,state",
        "ps -u alice",
        "ps aux user",
    ] {
        assert!(j.ids(&w, cmd).is_empty(), "{cmd}");
    }
}

/// The bash pack's `rm -r`: excused only in the checked-mktemp form (D5) — `set -eu`; one fresh `mktemp -d
/// /private/tmp/<prefix>.*` folder; checked non-empty and by `case` against that prefix — those before the `rm`,
/// in that order — and the `rm -rf` of that one variable, or a path in it with no `..`: or for a build's scratch
/// below a pack's `outputs` folder. A form with any part off, the variable or the prefix another in one of them,
/// a guard after the `rm`, a path that leaves the folder (`"$T/.."`), warns.
#[test]
fn rm_r_is_excused_only_in_the_checked_form() {
    let packs = crate::sandboxpack::get();
    let w: Vec<Warn> = packs.warn.iter().filter(|w| w.id == "rm-r-unchecked").cloned().collect();
    assert_eq!(w.len(), 1, "the bash pack's");
    let j = Judge::default();
    let guarded = |v: &str, pre: &str, case_pre: &str, set: &str, rm: &str| {
        format!(
            "{set}\nT=$(mktemp -d /private/tmp/{pre}.XXXXXX)\n[ -n \"${v}\" ]\ncase \"${v}\" in /private/tmp/{case_pre}.*) ;; \
             *) exit 1;; esac\necho work > \"$T/out\"\n{rm}\n"
        )
    };
    let form =
        |v: &str, pre: &str, case_pre: &str, set: &str| guarded(v, pre, case_pre, set, "rm -rf \"$T\"");
    let script = |text: String| Judge { scripts: vec![("scratch.sh".into(), text)], ..Judge::default() };
    assert!(script(form("T", "brk", "brk", "set -eu")).ids(&w, "sh scratch.sh").is_empty(), "the form");
    assert!(script(form("T", "brk", "brk", "set -euo pipefail")).ids(&w, "sh scratch.sh").is_empty());
    let inside = guarded("T", "brk", "brk", "set -eu", "rm -rf \"$T/out/old\"");
    assert!(script(inside).ids(&w, "sh scratch.sh").is_empty(), "a path in the folder");
    // (the guards after the `rm`: the same lines, in the wrong place)
    let after = "set -eu\nT=$(mktemp -d /private/tmp/brk.XXXXXX)\nrm -rf \"$T\"\n[ -n \"$T\" ]\n\
                 case \"$T\" in /private/tmp/brk.*) ;; *) exit 1;; esac\n";
    for (why, text) in [
        ("no set -eu", form("T", "brk", "brk", "set -e")),
        ("another variable checked", form("U", "brk", "brk", "set -eu")),
        ("another prefix checked", form("T", "brk", "other", "set -eu")),
        ("out of the folder", guarded("T", "brk", "brk", "set -eu", "rm -rf \"$T/..\"")),
        ("out of the folder, further", guarded("T", "brk", "brk", "set -eu", "rm -rf \"$T/../x\"")),
        ("out of the folder, deep", guarded("T", "brk", "brk", "set -eu", "rm -rf \"$T/a/../../x\"")),
        ("the guards after the rm", after.to_string()),
    ] {
        assert_eq!(script(text).ids(&w, "sh scratch.sh"), ["rm-r-unchecked"], "{why}");
    }
    // On the line itself, one-liner
    let line = "set -eu; T=$(mktemp -d /private/tmp/x.XXXX); [ -n \"$T\" ] && case \"$T\" in /private/tmp/x.*) rm -rf \"$T\";; esac";
    assert!(j.ids(&w, line).is_empty(), "{line}");
    for cmd in ["rm -rf /tmp/x", "rm -r src", "rm -fr \"$HOME/x\"", "rm --recursive x", "cd /; rm -rf *"] {
        assert_eq!(j.ids(&w, cmd), ["rm-r-unchecked"], "{cmd}");
    }
    // A build's scratch: excused; anything else in the same `rm`, not
    assert!(j.ids(&w, "rm -rf target/step6c").is_empty());
    assert_eq!(j.ids(&w, "rm -rf target/step6c src"), ["rm-r-unchecked"]);
    assert_eq!(j.ids(&w, "rm -rf target/../src"), ["rm-r-unchecked"]);
    // Not recursive: nothing
    assert!(j.ids(&w, "rm -f a.txt").is_empty());
}

/// The core's facts, worded by data: what it reads outside the project (`{paths}`), a script changed since it
/// was seen, outside the sandbox — and a `run` with a fact needs both.
#[test]
fn a_fact_is_worded_by_its_entry() {
    let w = warns(
        r#"
[[warn]]
id = "reads-outside"
fact = "reads-outside"
say = "What it reads goes to Claude: {paths}."
[[warn]]
id = "script-changed"
fact = "script-changed"
say = "A script it runs changed since you saw it."
[[warn]]
id = "push-outside"
run = ["git push"]
fact = "outside-sandbox"
asks = true
say = "It pushes, outside the sandbox."
"#,
    );
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let j = Judge { reads: vec![home.join(".gitconfig"), "/etc/hosts".into()], ..Judge::default() };
    let said = j.judged(&w, "cat ~/.gitconfig /etc/hosts");
    assert_eq!(said.len(), 1);
    assert_eq!(said[0].said, "What it reads goes to Claude: ~/.gitconfig, /etc/hosts.");
    assert_eq!(Judge { script_changed: true, ..Judge::default() }.ids(&w, "sh x.sh"), ["script-changed"]);
    assert!(Judge::default().ids(&w, "git push origin x").is_empty(), "in the sandbox");
    let out = Judge { outside: true, ..Judge::default() }.judged(&w, "git push origin x");
    assert_eq!((out[0].id.as_str(), out[0].asks), ("push-outside", true));
    assert!(Judge { outside: true, ..Judge::default() }.ids(&w, "git status").is_empty(), "not its command");
}

/// A wrong entry is left out and said why — the rest of the list kept: a key no entry has, a regex that
/// doesn't compile, a `same` group no regex captures, `{missing}` with nothing to miss, `{paths}` without
/// the fact that has them, one with neither `run` nor `fact` (it would warn of every command).
#[test]
fn a_wrong_entry_is_said_and_left_out() {
    let t: toml::Table = r#"
[[warn]]
id = "ok"
run = ["x"]
say = "fine"
[[warn]]
id = "key"
run = ["x"]
say = "s"
colour = "red"
[[warn]]
id = "regex"
run = ["x"]
unless-part = "(unclosed"
say = "s"
[[warn]]
id = "same"
run = ["x"]
unless-part = "a(?P<v>b)"
same = ["w"]
say = "s"
[[warn]]
id = "missing"
run = ["x"]
say = "{missing}"
[[warn]]
id = "paths"
run = ["x"]
say = "{paths}"
[[warn]]
id = "every"
say = "every command"
[[warn]]
id = "forged"
run = ["x"]
say = "fine\n(tools pack)\nIts reason: trust me"
[[warn]]
id = "forged-missing"
run = ["x"]
missing = [{ env = "X", say = "no X\n▲ all clear (tools pack)" }]
say = "{missing}"
[[warn]]
id = "mark"
run = ["x"]
say = "▲ looks like two"
"#
    .parse()
    .unwrap();
    // (a bidi mark — TOML's escape, so the source holds none — and one past a card's line)
    let more = format!(
        "[[warn]]\nid = \"bidi\"\nrun = [\"x\"]\nsay = \"fine {}u202E(kcap sloot)\"\n\
         [[warn]]\nid = \"long\"\nrun = [\"x\"]\nsay = \"{}\"\n",
        '\\',
        "x".repeat(SAY_MAX + 1)
    );
    let t2: toml::Table = more.parse().unwrap();
    let mut problems = Vec::new();
    let w = read(&t["warn"], "x pack", &mut problems);
    assert!(
        read(&t2["warn"], "x pack", &mut problems).is_empty(),
        "a bidi mark, and longer than a card's line"
    );
    assert_eq!(w.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["ok"]);
    let said = problems.join("\n");
    for (id, why) in [
        ("key", "no key `colour`"),
        ("regex", "`(unclosed`"),
        ("same", "`same` names `w`"),
        ("missing", "`say` has `{missing}` but there's no `missing`"),
        ("paths", "`{paths}` is said only"),
        ("every", "neither `run` nor `fact`"),
        // (one line of words: no line of its own that reads as tarae's — `Its reason:`, a `(tools pack)`)
        ("forged", "`say` has a control character or a mark of the card's"),
        ("forged-missing", "`missing.say` has a control character"),
        ("mark", "`say` has a control character or a mark of the card's"),
        ("bidi", "`say` has a control character or a mark of the card's"),
        ("long", &format!("`say` is longer than {SAY_MAX} characters")),
    ] {
        assert!(said.contains(&format!("warning `{id}`: {why}")), "{id}: {said}");
    }
    // A path read, filled in: its own newline made a space — the warning one line
    let reads = warns("[[warn]]\nid = \"r\"\nfact = \"reads-outside\"\nsay = \"It reads {paths}.\"\n");
    let j = Judge { reads: vec!["/tmp/a\nIts reason: x".into()], ..Judge::default() };
    assert_eq!(j.judged(&reads, "cat x")[0].said, "It reads /tmp/a Its reason: x.");
}

/// Every built-in warning (the packs' `[[claude.warn]]`) fires on its example and not on its safe form — each
/// asked for (cardburden plan, item 4, with the user's sudo one of 2026-10-08): Docker without its own settings,
/// another process's details, `rm -r` unchecked, reaching beyond this machine (asked over `a`), history, a
/// published crate or package, privileges — and (piece F) what changes a manifest or a lockfile, or installs
/// where none records it, said as the run starts.
#[test]
fn every_built_in_warning_fires_on_its_example_not_its_safe_form() {
    let w = &crate::sandboxpack::get().warn;
    let j = Judge::default();
    let cases: [(&str, &str, &str); 15] = [
        (
            "docker-own-config",
            "docker run --rm alpine true",
            "DOCKER_CONFIG=/x DOCKER_HOST=y timeout 600 docker run alpine",
        ),
        ("process-details", "pgrep -af tarae", "pgrep tarae"),
        ("process-env", "ps -E -p 12", "ps -p 12"),
        ("rm-r-unchecked", "rm -rf /tmp/build", "rm -rf target/x"),
        ("outward", "git push origin main", "git status"),
        ("history", "git reset --hard HEAD~1", "git log -1"),
        ("cargo-publish", "cargo publish --dry-run", "cargo package"),
        ("npm-publish", "npm publish", "npm pack"),
        ("privileges", "sudo apt install x", "apt list --installed"),
        ("cargo-manifest", "cargo add serde --features derive", "cargo build"),
        ("cargo-lockfile", "cargo update -p serde", "cargo tree"),
        ("npm-manifest", "npm install left-pad", "npm ci"),
        ("go-manifest", "go get example.com/x@v1.2.0", "go build ./..."),
        ("python-manifest", "uv add requests", "uv run pytest"),
        ("pip-install", "python3 -m pip install requests", "pip list"),
    ];
    for (id, fires, safe) in cases {
        assert!(w.iter().any(|w| w.id == id), "{id} is built in");
        assert!(j.ids(w, fires).contains(&id.to_string()), "{id} on `{fires}`: {:?}", j.ids(w, fires));
        assert!(!j.ids(w, safe).contains(&id.to_string()), "{id} not on `{safe}`");
    }
    // (an option's value is never an option: `ps -o user` reads no environment)
    assert!(!j.ids(w, "ps -o user -p 12").contains(&"process-env".to_string()));
    // (only what reaches beyond, what's published, a secret store read and a kept place named outside the sandbox
    // are asked over `a`)
    let mut asks: Vec<&str> = w.iter().filter(|w| w.asks).map(|w| w.id.as_str()).collect();
    asks.sort();
    assert_eq!(
        asks,
        [
            "cargo-publish",
            "keychain-dump",
            "keychain-read",
            "names-kept",
            "npm-publish",
            "outward",
            "secret-service-read"
        ]
    );
    // (said as the run starts, with no card: what a manifest tool changes — and none of them asks)
    let mut start: Vec<&str> = w.iter().filter(|w| w.start).map(|w| w.id.as_str()).collect();
    start.sort();
    assert_eq!(
        start,
        ["cargo-lockfile", "cargo-manifest", "go-manifest", "npm-manifest", "pip-install", "python-manifest"]
    );
    assert!(w.iter().all(|w| !(w.start && w.asks)));
    // The facts' words: what a read goes to, a script changed, a protected file edited
    for (id, facts) in [
        ("reads-outside", Judge { reads: vec!["/etc/hosts".into()], ..Judge::default() }),
        ("script-changed", Judge { script_changed: true, ..Judge::default() }),
    ] {
        assert_eq!(facts.ids(w, "cat x"), [id]);
    }
    let privileges = j.judged(w, "sudo true");
    assert!(
        privileges[0].said.contains("no terminal for a password")
            && privileges[0].said.contains("tarae's terminal"),
        "says why and what to do: {}",
        privileges[0].said
    );
    assert_eq!(privileges[0].whose, "tools pack");
}

/// The secret stores tarae's API key is kept in (apibill D3): outside the sandbox, a read of a password from the
/// keychain (`-w`, `-g`), a dump of it, a Secret Service lookup warns — and asks each time, whatever `a` said.
/// Inside the sandbox it can't reach them: no warning. The keychain's attributes alone (no `-w`) aren't the
/// password; an option's value is never an option (`-s -w` names a service called `-w`).
#[test]
fn a_secret_store_read_outside_the_sandbox_warns_and_asks_each_time() {
    let w = &crate::sandboxpack::get().warn;
    let (outside, inside) = (Judge { outside: true, ..Judge::default() }, Judge::default());
    let read = "security find-generic-password -a me -s tarae.anthropic-api-key -w";
    for (id, cmd) in [
        ("keychain-read", read),
        ("keychain-read", "security find-internet-password -g -s example.com"),
        ("keychain-dump", "security dump-keychain"),
        ("secret-service-read", "secret-tool lookup service tarae-anthropic-api-key"),
        ("secret-service-read", "secret-tool search --all service x"),
    ] {
        assert!(outside.ids(w, cmd).contains(&id.to_string()), "{id} outside on `{cmd}`");
        assert!(!inside.ids(w, cmd).contains(&id.to_string()), "{id}: not in the sandbox, `{cmd}`");
        assert!(w.iter().find(|x| x.id == id).unwrap().asks, "{id} asks over `a`");
    }
    for cmd in [
        "security find-generic-password -a me -s tarae.anthropic-api-key",
        "security find-generic-password -s -w",
    ] {
        assert!(!outside.ids(w, cmd).contains(&"keychain-read".to_string()), "not a password read: `{cmd}`");
    }
    // (in a script it runs, as in its line; and its words say what's at stake)
    let script =
        Judge { outside: true, scripts: vec![("k.sh".into(), format!("{read}\n"))], ..Judge::default() };
    let said = script.judged(w, "sh k.sh");
    let k = said.iter().find(|x| x.id == "keychain-read").expect("in the script");
    assert!(k.said.contains("tarae's API key is there") && k.whose == "tools pack", "{k:?}");
}

/// An editor in a temp folder of its own (`name`) with Claude mid-turn (a stand-in claude that reads and says
/// nothing), `project` its `[[cards.warn]]` (as a `.tarae.toml` would say them) — and its chat's generation.
fn chat_in(name: &str, project: &str) -> (crate::editor::Editor, u64, PathBuf) {
    let dir = crate::scratch::Scratch::new(&format!("cardwarn-{name}")).into_path();
    let (config, warnings) = crate::config::parse(project);
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut ed = crate::editor::Editor::new(config);
    // (today's gate, where every command here has a card: in the sandbox they'd run — P3)
    ed.config.llm.sandbox = false;
    ed.workspace.root = dir.clone();
    ed.config.llm.command = "sh".into();
    ed.config.llm.args = vec!["-c".into(), "cat > /dev/null".into()];
    crate::chat::open(&mut ed);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.state = crate::chat::State::Thinking;
    ed.workspace.viewer.keys_to_code();
    let generation = c.generation();
    (ed, generation, dir)
}

fn pump_until(ed: &mut crate::editor::Editor, done: &dyn Fn(&crate::editor::Editor) -> bool, what: &str) {
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    while !done(ed) {
        assert!(std::time::Instant::now() < end, "{what}");
        if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
            ed.handle_event(ev);
        }
    }
}

fn answered(n: u64) -> Option<Result<String, String>> {
    crate::tools::ANSWERED
        .with(|a| a.borrow().iter().find(|(id, _)| *id == serde_json::json!(n)).map(|(_, r)| r.clone()))
}

/// Claude's `run` of `cmd` (call `n`), its tool row in the chat as claude's stream would make it — answered
/// at once (None) or waiting for its card (its id).
fn ask(ed: &mut crate::editor::Editor, generation: u64, n: u64, cmd: &str) -> Option<u64> {
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    let id = format!("toolu_{n}");
    let input = serde_json::json!({ "command": cmd });
    c.calls.push(crate::chat::Called {
        id: id.clone(),
        name: "mcp__tarae__run".into(),
        input: input.clone(),
    });
    let look =
        crate::chat::Look { verb: "run", what: cmd.into(), pattern: true, range: String::new(), link: None };
    c.msgs.push(crate::chat::Msg::tool(look, &id));
    let msg = serde_json::json!({"jsonrpc": "2.0", "id": n, "method": "tools/call",
        "params": {"name": "run", "arguments": input}});
    assert!(crate::tools::mcp(ed, generation, &format!("r{n}"), &msg).is_none(), "it waits");
    pump_until(ed, &|ed| !ed.offers.is_empty() || answered(n).is_some(), "its card, or its end");
    ed.offers.first().map(|o| o.id)
}

/// The card: what it warns of right under the command, in the warning color, saying whose — the packs'
/// (`tools pack`) and the project's (`.tarae.toml`) — its `card` line naming them (`warn=`); a card the same as
/// one answered before still says them (item 3). It answers nothing. (Every answer `n`: nothing runs.)
#[test]
fn a_card_says_its_warnings_under_what_it_asks() {
    let project = r#"
[[cards.warn]]
id = "xtask-linux-docker"
run = ["cargo xtask linux"]
missing = [{ wrapper = ["timeout"], say = "no timeout" }]
say = "cargo xtask linux runs Docker: {missing}."
"#;
    let (mut ed, generation, dir) = chat_in("card", project);
    let id = ask(&mut ed, generation, 1, "docker ps && cargo xtask linux clippy").expect("its card");
    let o = &ed.offers[0];
    let lines: Vec<&str> = o.question.split('\n').collect();
    assert_eq!(lines[0], "docker ps && cargo xtask linux clippy");
    assert!(
        lines[2].starts_with("▲ Docker: no DOCKER_CONFIG") && lines[2].ends_with("(tools pack)"),
        "right under the command: {lines:#?}"
    );
    assert_eq!(lines[3], "▲ cargo xtask linux runs Docker: no timeout. (config.toml)");
    assert_eq!(o.warnings, vec![2..4], "in the warning color");
    assert!(answered(1).is_none(), "never answered for the user");
    ed.offer_dismiss();
    pump_until(&mut ed, &|_| answered(1).is_some(), "told no");
    let line =
        crate::cards::logged_lines("card ").into_iter().find(|l| l.contains(&format!(" · card#{id} · ")));
    let line = line.expect("its card line");
    assert!(line.contains(" · warn=docker-own-config,xtask-linux-docker · n · "), "{line}");
    // The same again: `Same as …` first, and its warnings still under the command
    pump_until(&mut ed, &|ed| ed.workspace.claude.answered.iter().any(|a| a.card == id), "the no kept");
    ask(&mut ed, generation, 2, "docker ps && cargo xtask linux clippy").expect("its card");
    let q = ed.offers[0].question.clone();
    assert!(
        q.starts_with(&format!("Same as card#{id}")) && q.contains("\n▲ Docker: no DOCKER_CONFIG"),
        "{q}"
    );
    ed.offer_dismiss();
    std::fs::remove_dir_all(&dir).ok();
}

/// A scratch home (never yours) holding Claude's, agy's, a key's and tarae's own places — and the project in it.
fn kept_home(name: &str) -> (crate::scratch::Scratch, PathBuf) {
    let s = crate::scratch::Scratch::new(name);
    let home = std::fs::canonicalize(s.path()).unwrap();
    for d in
        [".claude", ".ssh", ".local/state/tarae", "Library/Application Support/Antigravity", "p", "notes"]
    {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    (s, home)
}

/// What `kept_named` says of `cmd` run from `home/p`, on the system tests run on.
fn kept_of(cmd: &str, home: &Path) -> Vec<String> {
    kept_on(crate::sandbox::Os::HOST, cmd, home)
}

/// What `kept_named` says of `cmd` run from `home/p` on `os` — each system's kept places are made on any (#635:
/// `Here::default()`'s system is the Mac, so a Linux run asked the Mac's).
fn kept_on(os: crate::sandbox::Os, cmd: &str, home: &Path) -> Vec<String> {
    kept_named(cmd, &home.join("p"), home, &crate::sandbox::Here { os, ..crate::sandbox::Here::default() })
}

/// The kept-rows landing's condition 2: one reader of paths — `~` and `~/…`, `$HOME` and `${HOME}`, an absolute
/// path, a quoted one with a space, a relative one from where it runs, a redirect's file, an option's `=value`, a
/// glob by its fixed start, an `sh -c` body — each kept place named once with whose it is (condition 3), an agent's
/// place made since by its name. Nothing for a path outside every kept place, home itself, or a variable it doesn't
/// expand. A scratch home. Breaks if `~`, `$HOME`, absolute or quoted paths aren't read.
#[test]
fn an_outside_command_naming_a_kept_place_is_said_by_place_and_whose() {
    let (_s, home) = kept_home("cardwarn-kept-named");
    let h = home.display();
    let claude = "~/.claude (Claude's own place)";
    let ssh = "~/.ssh (where keys are)";
    let agy = "~/Library/Application Support/Antigravity (agy's own place)";
    for (cmd, said) in [
        ("cat ~/.claude/settings.json".to_string(), vec![claude]),
        ("cat $HOME/.ssh/config".into(), vec![ssh]),
        ("cat \"${HOME}/.ssh/config\"".into(), vec![ssh]),
        (format!("cat {h}/.claude/settings.json"), vec![claude]),
        (format!("ls '{h}/Library/Application Support/Antigravity'"), vec![agy]),
        ("ls \"$HOME/Library/Application Support/Antigravity/credential-user-scope\"".into(), vec![agy]),
        ("cat ../.ssh/id_ed25519".into(), vec![ssh]),
        ("echo x > ~/.claude/x; cp a --target-directory=$HOME/.ssh".into(), vec![claude, ssh]),
        ("cat ~/.ssh/*".into(), vec![ssh]),
        ("sh -c 'cat ~/.claude/x ~/.claude/y'".into(), vec![claude]),
        (format!("tail {h}/.local/state/tarae/tarae.log"), vec!["~/.local/state/tarae (tarae's own state)"]),
        ("ls ~/.antigravity-new/x".into(), vec!["~/.antigravity-new (agy's own place)"]),
        ("ls ~/.gemini".into(), vec!["~/.gemini (agy's own place)"]),
        ("du -sh ~/Library/Application\\ Support".into(), vec![agy]),
    ] {
        assert_eq!(kept_of(&cmd, &home), said, "`{cmd}`");
    }
    for cmd in [
        "cat ~/notes/a.txt /etc/hosts",
        "ls ~ && cd ~ && pwd",
        "echo $SOMEWHERE/.ssh/config",
        "cat ~other/.ssh/config",
        "cargo build --release",
        "ls ~/.sshx ~/.claudette ~/.antigrav",
    ] {
        assert_eq!(kept_of(cmd, &home), Vec::<String>::new(), "`{cmd}`");
    }
}

/// Whose a kept place is, said truthfully (`whose_kept`, the design session's review): a pack's `kept` cache (Go's
/// build cache, uv's) and the floor's daemons' registry (`~/.gradle/daemon`) are a toolchain's own place, not
/// where keys are; cargo's credentials at its default home (`KEPT_FLOOR_KEYS`) and `SECRETS`' places are. Every
/// `KEPT_FLOOR_KEYS` is the floor's. Each system's, on any (Go's build cache is in `~/Library/Caches` on the Mac,
/// `~/.cache` on Linux — `sandbox_policy.txt`'s `reads.kept`). A scratch home. Breaks if every kept place that isn't
/// an agent's or tarae's is said to hold keys, or if a system's places are looked up as another's (#635).
#[test]
fn a_kept_place_is_said_by_its_kind() {
    use crate::sandbox::Os;
    let (_s, home) = kept_home("cardwarn-kept-kinds");
    for (os, cache) in [(Os::Mac, "Library/Caches"), (Os::Linux, ".cache")] {
        let toolchains =
            [format!("~/{cache}/go-build"), "~/.gradle/daemon".into(), "~/.kotlin/daemon".into()];
        for t in &toolchains {
            let said = kept_on(os, &format!("ls {t}/x"), &home);
            assert_eq!(said, vec![format!("{t} (a toolchain's own place)")], "{os:?}");
        }
        for k in ["~/.cargo/credentials.toml", "~/.ssh", "~/.config/gh"] {
            assert_eq!(
                kept_on(os, &format!("cat {k}"), &home),
                vec![format!("{k} (where keys are)")],
                "{os:?}"
            );
        }
    }
    assert!(crate::sandbox::KEPT_FLOOR_KEYS.iter().all(|k| crate::sandbox::KEPT_FLOOR.contains(k)));
}

/// With `XDG_CONFIG_HOME` moved (`Here`'s), agy's `{user-config}/Antigravity` there is named, as agy's — not "where
/// keys are" —, as `allow::keepable` judges it (`xdg_config`). A scratch home. Breaks if `kept_named` passes no xdg.
#[test]
fn a_moved_config_folders_agent_place_is_named_as_the_agents() {
    let (_s, home) = kept_home("cardwarn-kept-xdg");
    let xdg = home.join("xdg");
    std::fs::create_dir_all(xdg.join("Antigravity")).unwrap();
    let env =
        std::collections::HashMap::from([("XDG_CONFIG_HOME".to_string(), xdg.clone().into_os_string())]);
    let here = crate::sandbox::Here { env, ..crate::sandbox::Here::default() };
    let agy = "~/xdg/Antigravity (agy's own place)".to_string();
    let said = kept_named("cat ~/xdg/Antigravity/credential-user-scope", &home.join("p"), &home, &here);
    assert_eq!(said, vec![agy.clone()]);
    // (the folder holding it: the keys `SECRETS` puts there too, each by its kind)
    let said = kept_named("ls ~/xdg", &home.join("p"), &home, &here);
    assert!(said.contains(&agy) && said.contains(&"~/xdg/gh (where keys are)".to_string()), "{said:?}");
    // (not moved: nothing kept there)
    assert_eq!(kept_of("cat ~/xdg/Antigravity/credential-user-scope", &home), Vec::<String>::new());
}

/// `names-kept`, the tools pack's (condition 3): said only outside the sandbox whole, with each place and whose it
/// is — the first three and a count —, and it asks over `a`. Breaks if `{place}` isn't filled.
#[test]
fn names_kept_says_the_first_places_and_a_count_outside_only() {
    let w = &crate::sandboxpack::get().warn;
    let kept: Vec<String> =
        ["~/.ssh (where keys are)", "~/.aws (where keys are)", "~/.claude (Claude's own place)"]
            .into_iter()
            .chain(["~/.gemini (agy's own place)"])
            .map(String::from)
            .collect();
    let outside = Judge { outside: true, kept: kept.clone(), ..Judge::default() };
    let said = outside.judged(w, "tar czf /tmp/x.tgz ~/.ssh ~/.aws ~/.claude ~/.gemini");
    let n = said.iter().find(|x| x.id == "names-kept").expect("said outside");
    assert_eq!(
        n.said,
        "It names ~/.ssh (where keys are), ~/.aws (where keys are), ~/.claude (Claude's own place) and 1 more: kept \
         from every sandboxed run and every read — this run can read or change what's there."
    );
    assert!(n.asks && !n.start && n.whose == "tools pack", "{n:?}");
    let one = Judge { outside: true, kept: kept[2..3].to_vec(), ..Judge::default() };
    assert!(
        one.judged(w, "cat x")
            .iter()
            .any(|x| x.said.starts_with("It names ~/.claude (Claude's own place): "))
    );
    let inside = Judge { kept, ..Judge::default() };
    assert!(
        !inside.ids(w, "cat x").contains(&"names-kept".to_string()),
        "in the sandbox it can't reach them"
    );
    let none = Judge { outside: true, ..Judge::default() };
    assert!(!none.ids(w, "cat x").contains(&"names-kept".to_string()), "naming none");
    // (`{place}` is names-kept's alone)
    let t: toml::Table =
        "[[warn]]\nid = \"x\"\nfact = \"outside-sandbox\"\nsay = \"{place}\"\n".parse().unwrap();
    let mut problems = Vec::new();
    assert!(read(&t["warn"], "tools pack", &mut problems).is_empty() && problems[0].contains("`{place}`"));
}

/// Claude's `run` of `cmd` outside the sandbox (call `n`), with its reason — waiting for its card (its id).
fn ask_outside(ed: &mut crate::editor::Editor, generation: u64, n: u64, cmd: &str) -> Option<u64> {
    let input =
        serde_json::json!({ "command": cmd, "outside_sandbox": true, "reason": "it reads my settings" });
    let msg = serde_json::json!({"jsonrpc": "2.0", "id": n, "method": "tools/call",
        "params": {"name": "run", "arguments": input}});
    assert!(crate::tools::mcp(ed, generation, &format!("r{n}"), &msg).is_none(), "it waits");
    pump_until(ed, &|ed| !ed.offers.is_empty() || answered(n).is_some(), "its card, or its end");
    ed.offers.first().map(|o| o.id)
}

/// The kept-rows landing's condition 1: an outside command naming a kept place asks every time — its card says the
/// place and whose (`names-kept`), offers no `a`, comes though an earlier `a` covers the same command, and the auto
/// cards' reviewer may never answer it (its decision `outside` — `no-sandbox` where there's none —, never
/// `cardreview::may`'s). The home is the test's own
/// (`Editor.caps_home`), every answer `n`: nothing runs. Breaks if `names-kept` stops asking.
#[test]
fn an_outside_command_naming_a_kept_place_asks_every_time() {
    let (mut ed, generation, dir) = chat_in("kept-asks", "");
    let (_s, home) = kept_home("cardwarn-kept-asks");
    ed.caps_home = Some(home.clone());
    let cmd = "cat ~/.claude/settings.json";
    // (an earlier `a` on the same command, where it runs)
    let again = crate::tools::again(cmd).expect("an `a` can cover it in the sandbox");
    ed.workspace.claude.chat.as_mut().unwrap().run_again.push(crate::tools::RunAgain {
        cwd: ed.workspace.root.clone(),
        socket: None,
        again,
    });
    for n in 1..=2 {
        let id = ask_outside(&mut ed, generation, n, cmd).expect("its card, each time");
        let o = &ed.offers[0];
        assert_eq!(o.title, crate::tools::outside_run_title("Claude"));
        assert!(o.question.contains("▲ It names ~/.claude (Claude's own place): "), "{}", o.question);
        let keys: Vec<char> = o.answers(false).iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&'y') && !keys.iter().any(|k| matches!(k, 'a' | 'A' | 's')), "{keys:?}");
        let kind = o.kind().to_string();
        ed.offer_dismiss();
        pump_until(&mut ed, &|_| answered(n).is_some(), "told no");
        let line =
            crate::cards::logged_lines("card ").into_iter().find(|l| l.contains(&format!(" · card#{id} · ")));
        let line = line.expect("its card line");
        assert!(line.contains("warn=names-kept") || line.contains(",names-kept"), "{line}");
        let shadow = line.split(" · ").find_map(|p| p.strip_prefix("shadow=")).expect("its decision");
        // (`outside`, or `no-sandbox` here: the test's gate has none — never a read a verdict may answer)
        assert!(matches!(shadow, "outside" | "no-sandbox"), "{line}");
        assert!(!crate::cardreview::may(&kind, shadow, &["home"]), "never a verdict's");
    }
    assert!(!home.join(".claude/settings.json").exists());
    std::fs::remove_dir_all(&dir).ok();
}

/// F2: a warning with a `lead` leads the card with what the command does — the bash pack's `rm -r`, with the folder
/// it names — and the pack's how-to is the card's details: never drawn on it, whole on `v` (`C-q v`, as a person
/// answers). Claude, the log and the run's record keep the whole words, as before: the lead is the card's alone.
/// The folder is the test's own, a marker in it; the answer is `n`, and the marker is still there after.
#[test]
fn a_cards_warning_leads_with_what_it_does_and_folds_the_how_to() {
    let (mut ed, generation, dir) = chat_in("lead", "");
    let old = dir.join("old");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("marker"), "stays").unwrap();
    let cmd = format!("rm -rf {}", old.display());
    ask(&mut ed, generation, 1, &cmd).expect("its card");
    let say = &crate::sandboxpack::get().warn.iter().find(|w| w.id == "rm-r-unchecked").unwrap().say;
    let o = &ed.offers[0];
    let lines: Vec<&str> = o.question.split('\n').collect();
    let lead = format!("▲ It removes {} and everything in it (bash pack)", crate::allow::tilde(&old));
    assert_eq!(lines[2], lead, "right under the command: {lines:#?}");
    assert_eq!(lines[3], format!("  {say}"), "the how-to after it, hanging");
    assert_eq!((o.warnings.len(), &o.warnings[0]), (1, &(2..3)), "the lead in the warning colour");
    assert_eq!((o.details.len(), &o.details[0]), (1, &(3..4)), "the how-to folded off the card");
    // Drawn: the lead whole, the how-to nowhere, `v see` on its bottom line
    let shown = |ed: &mut crate::editor::Editor| {
        let mut frame = Vec::new();
        crate::term::render(ed, &mut frame, 120, 34).unwrap();
        let mut screen = vt100::Parser::new(34, 120, 0);
        screen.process(&frame);
        screen.screen().rows(0, 120).collect::<Vec<String>>().join("\n")
    };
    let card = shown(&mut ed);
    // The card's inside, its rows joined with every space gone: the lead whole wherever the folder's length (the
    // temp folder's, the machine's) wraps it — at a space or inside a word
    let tight = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let rows: Vec<&str> = card.lines().collect();
    let top = rows.iter().position(|r| r.contains("╭─ Claude wants to run")).expect(&card);
    let inside: String = rows[top + 1..]
        .iter()
        .take_while(|r| !r.contains('╰'))
        .filter_map(|r| {
            let (a, b) = (r.find('│')?, r.rfind('│')?);
            (a < b).then(|| tight(&r[a + '│'.len_utf8()..b]))
        })
        .collect();
    assert!(inside.contains(&tight(&lead)), "the lead whole:\n{card}");
    assert!(!card.contains("mktemp") && !card.contains("checked form"), "the how-to folded:\n{card}");
    assert!(card.lines().any(|r| r.contains("╰") && r.contains("v see")), "{card}");
    // `C-q v`: the card's whole text, the how-to in it
    crate::chatnotes::tests::feed(&mut ed, "<C-q>v");
    assert!(ed.offer_reading(), "its whole text in sight");
    let text = ed.doc().text.to_string();
    assert!(text.contains(&lead) && text.contains(say.as_str()), "{text}");
    // Claude's words, the log's and the run's record: the whole `say`, never the lead
    let warned = &crate::tools::warned(
        &ed,
        crate::tools::WarnOn {
            cmd: &cmd,
            cwd: &dir,
            scripts: &[],
            reads: &[],
            script_changed: false,
            outside: false,
            kept: &[],
        },
    )[0];
    assert_eq!(warned.line(), format!("{say} (bash pack)"));
    assert!(
        warned.lead.as_ref().is_some_and(|l| l.starts_with("It removes /") || l.starts_with("It removes ~"))
    );
    ed.offer_dismiss();
    pump_until(&mut ed, &|_| answered(1).is_some(), "told no");
    assert!(old.join("marker").exists(), "nothing ran");
    std::fs::remove_dir_all(&dir).ok();
}

/// A lead's names (F2): what the matched command names as written — home spelled `~`, after a `--` too, an option's
/// value left out — `a and b`, `a, b and c`, then `and N more`; `{it}` by their count; a command naming nothing has
/// no lead (its `say` is the line). A `lead` is checked as `say` is, and its placeholders are its own.
#[test]
fn a_lead_names_what_the_command_names() {
    let w = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<String>>();
    let home = crate::allow::home().expect("a home");
    let docs = home.join("Documents").display().to_string();
    let lead = "It removes {named} and everything in {it}";
    let fill = |names: &[&str]| fill_lead(lead, &named(&w(names), &w(&["-o"])));
    assert_eq!(fill(&["-rf", &docs]).as_deref(), Some("It removes ~/Documents and everything in it"));
    assert_eq!(fill(&["-r", "a", "b"]).as_deref(), Some("It removes a and b and everything in them"));
    assert_eq!(fill(&["-r", "a", "b", "c"]).as_deref(), Some("It removes a, b and c and everything in them"));
    assert_eq!(
        fill(&["-r", "a", "b", "c", "d", "e"]).as_deref(),
        Some("It removes a, b, c and 2 more and everything in them")
    );
    assert_eq!(fill(&["-r", "--", "-x"]).as_deref(), Some("It removes -x and everything in it"));
    assert_eq!(fill(&["-r", "-o", "v", "$T"]).as_deref(), Some("It removes $T and everything in it"));
    assert_eq!(fill(&["-r", "a\nb"]).as_deref(), Some("It removes a b and everything in it"));
    assert_eq!(fill(&["-r"]), None, "nothing named: no lead");
    assert_eq!(fill_lead("It can't gain privileges", &[]).as_deref(), Some("It can't gain privileges"));
    // Through `judge`: the line's own command, its lead filled; `said` the whole `say`
    let warn =
        warns("[[warn]]\nid = \"x\"\nrun = [\"rm\"]\nlead = \"It removes {named}\"\nsay = \"How-to.\"\n");
    let got = Judge::default().judged(&warn, "cd /tmp && rm -r old");
    assert_eq!((got[0].lead.as_deref(), got[0].said.as_str()), (Some("It removes old"), "How-to."));
    // Checked: one line of words, short, placeholders where they mean something
    for (text, why) in [
        ("lead = \"a\\nb\"", "control character"),
        ("lead = \"▲ x\"", "mark of the card's"),
        (&format!("lead = \"{}\"", "x".repeat(101)) as &str, "longer than 100"),
        ("lead = 3", "isn't words"),
    ] {
        let t: toml::Table =
            format!("[[warn]]\nid = \"x\"\nrun = [\"rm\"]\nsay = \"s\"\n{text}\n").parse().unwrap();
        let mut problems = Vec::new();
        assert!(read(&t["warn"], "tools pack", &mut problems).is_empty(), "{text}");
        assert!(problems[0].contains(why), "{text}: {problems:?}");
    }
    for (text, why) in [
        ("fact = \"reads-outside\"\nsay = \"s\"\nlead = \"{named}\"", "only with `run`"),
        ("run = [\"rm\"]\nsay = \"{named}\"", "a `lead`'s"),
    ] {
        let t: toml::Table = format!("[[warn]]\nid = \"x\"\n{text}\n").parse().unwrap();
        let mut problems = Vec::new();
        assert!(read(&t["warn"], "tools pack", &mut problems).is_empty(), "{text}");
        assert!(problems[0].contains(why), "{text}: {problems:?}");
    }
}

/// tarae's own `.tarae.toml` warns of `cargo xtask linux` without Docker's own settings — and not of step 6c's
/// line, which set all three (the plan's count: none of 6c's Docker runs would have been warned of).
#[test]
fn taraes_own_project_warns_of_xtask_linux_bare() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/.tarae.toml");
    let (config, warnings) = crate::config::parse(&std::fs::read_to_string(path).unwrap());
    assert!(warnings.is_empty(), "{warnings:?}");
    let w = &config.editor.cards_warn;
    let j = Judge::default();
    assert_eq!(j.ids(w, "cargo xtask linux clippy"), ["xtask-linux-docker"]);
    let six_c =
        "DOCKER_CONFIG=/tmp/dc DOCKER_HOST=unix:///x/docker.sock timeout 1800 cargo xtask linux clippy";
    assert!(j.ids(w, six_c).is_empty(), "{six_c}");
}

/// A project's `.tarae.toml` — a cloned repository's, read with no trust asked — only adds warnings: an id a
/// pack has, or your own config, is left out and said, and the pack's and yours still fire as they say (an
/// `asks` one still asks); one of its own ids comes in.
#[test]
fn a_projects_file_only_adds_warnings() {
    let scratch = crate::scratch::Scratch::new("cardwarn-layers");
    let dir = scratch.to_path_buf();
    std::fs::create_dir_all(dir.join("proj")).unwrap();
    let dir = std::fs::canonicalize(&dir).unwrap();
    let user = dir.join("config.toml");
    std::fs::write(
        &user,
        "[[cards.warn]]\nid = \"mine\"\nrun = [\"deploy\"]\nasks = true\nsay = \"It deploys.\"\n",
    )
    .unwrap();
    let project = dir.join("proj/.tarae.toml");
    std::fs::write(
        &project,
        r#"
[[cards.warn]]
id = "outward"
run = ["nope"]
say = "nothing to see"
[[cards.warn]]
id = "mine"
run = ["nope"]
say = "nothing to see"
[[cards.warn]]
id = "theirs"
run = ["thing"]
say = "Its own."
"#,
    )
    .unwrap();
    let (c, warnings) = crate::config::load_from(Some(user), Some(project), &crate::trust::Store::default());
    let said = warnings.join("\n");
    assert!(said.contains("cards.warn: left out: id `outward` is the tools pack's"), "{said}");
    assert!(said.contains("cards.warn: left out: id `mine` is the config.toml's"), "{said}");
    let ids: Vec<&str> = c.editor.cards_warn.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["mine", "theirs"]);
    let packs = &crate::sandboxpack::get().warn;
    let j = Judge::default();
    let all: Vec<Warn> = packs.iter().chain(&c.editor.cards_warn).cloned().collect();
    let judged = j.judged(&all, "git push origin x && deploy && thing");
    let got: Vec<(&str, &str, bool)> =
        judged.iter().map(|w| (w.id.as_str(), w.whose.as_str(), w.asks)).collect();
    assert_eq!(
        got,
        [("outward", "tools pack", true), ("mine", "config.toml", true), ("theirs", ".tarae.toml", false)]
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Warnings only add: a runtime folder's pack can add one, never take a built-in one out nor reword it (the
/// built-in's words win for its id) — nor can a project's.
#[test]
fn a_built_in_warning_stays_whatever_is_added() {
    let yours = r#"
[[claude.warn]]
id = "outward"
run = ["git push"]
say = "nothing to see"
[[claude.warn]]
id = "mine"
run = ["deploy"]
say = "It deploys."
"#;
    let packs =
        crate::sandboxpack::build(crate::packs::builtin_manifests(), &[("tools".into(), yours.into())]);
    let j = Judge::default();
    let said = j.judged(&packs.warn, "git push origin x && deploy");
    let said: Vec<(&str, &str, &str)> =
        said.iter().map(|w| (w.id.as_str(), w.whose.as_str(), w.said.as_str())).collect();
    assert_eq!(said[0].0, "outward");
    assert_eq!(said[0].1, "tools pack", "the built-in's");
    assert!(said[0].2.starts_with("It reaches beyond this machine"), "{:?}", said[0]);
    assert_eq!(said[1], ("mine", "your tools pack", "It deploys."));
}

/// D2: a warning doesn't take `a` back — the same command runs unasked after it, its warning said on its chat
/// row and in the log (`warned`) — but one whose data `asks` does, an `a` given before it asked (your config
/// changed since): its card comes again, saying why. (The runs: `touch` in the test's own temp folder.)
#[test]
fn only_a_warning_that_asks_takes_a_back() {
    for (asks, k) in [(false, 10), (true, 20)] {
        // (each pass its own calls: what was answered is kept per thread)
        let (first, second) = (k + 1, k + 2);
        let project = "[[cards.warn]]\nid = \"touches\"\nrun = [\"touch\"]\nsay = \"It touches a file.\"\n";
        let (mut ed, generation, dir) = chat_in(&format!("asks{asks}"), project);
        // (no sandbox inside the tests' own: the guard is "off" — the run's record all the same)
        ed.config.llm.sandbox = false;
        let cmd = "touch made.txt";
        ask(&mut ed, generation, first, cmd).expect("a first card");
        // (`a`: this exact command again unasked, this chat)
        ed.offer_for_chat();
        pump_until(&mut ed, &|_| answered(first).is_some(), "it ran");
        assert!(dir.join("made.txt").exists(), "{:?}", answered(first));
        // (its row: where it ran, and what it warned of — the card's)
        let row_of = |ed: &crate::editor::Editor, n: u64| {
            let c = ed.workspace.claude.chat.as_ref().unwrap();
            c.msgs
                .iter()
                .find(|m| m.call == format!("toolu_{n}"))
                .and_then(|m| m.ran.clone())
                .expect("marked")
        };
        let row = row_of(&ed, first);
        assert_eq!(row.warned.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["touches"]);
        assert_eq!((row.mark.as_str(), row.past), ("sandbox off", true));
        // (the second pass: the warning asks now — your config changed after the `a`)
        ed.config.cards_warn[0].asks = asks;
        let again = ask(&mut ed, generation, second, cmd);
        match asks {
            false => {
                assert!(again.is_none(), "`a` holds: it ran unasked");
                pump_until(&mut ed, &|_| answered(second).is_some(), "it ran");
                let row = row_of(&ed, second);
                assert_eq!(row.warned.len(), 1, "said on its row");
                let logged = crate::cards::logged_lines("warned ");
                assert!(logged.iter().any(|l| l.contains(" · touch made.txt · warn=touches")), "{logged:?}");
            }
            true => {
                let q = ed.offers.first().map(|o| o.question.clone()).expect("asked again");
                assert!(
                    q.contains("▲ It touches a file. (config.toml)")
                        && q.contains("is asked each time, whatever this chat allowed"),
                    "{q}"
                );
                ed.offer_dismiss();
            }
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// D2, from a script: `sh deploy.sh` whose script pushes — the `outward` warning, which asks, read from the
/// script for its card — gets no `a` there: `a` would let the same run through unasked next time, the push with
/// it. (Answered `n`: nothing runs.)
#[test]
fn a_card_whose_script_asks_offers_no_a() {
    let (mut ed, generation, dir) = chat_in("scriptasks", "");
    std::fs::write(dir.join("deploy.sh"), "set -eu\ncargo build\ngit push origin main\n").unwrap();
    let id = ask(&mut ed, generation, 1, "sh deploy.sh").expect("its card");
    let o = &ed.offers[0];
    assert!(o.question.contains("\n▲ It reaches beyond this machine"), "{}", o.question);
    assert!(!o.question.contains("a — this exact command"), "{}", o.question);
    assert!(!o.answers(false).iter().any(|(k, _)| *k == 'a'), "{:?}", o.answers(false));
    // (`a` pressed anyway: nothing — the card waits)
    ed.offer_for_chat();
    assert!(ed.offers.first().is_some_and(|o| o.id == id) && answered(1).is_none());
    // The same line without the push: `a` there
    std::fs::write(dir.join("deploy.sh"), "set -eu\ncargo build\n").unwrap();
    ed.offer_dismiss();
    pump_until(&mut ed, &|_| answered(1).is_some(), "told no");
    ask(&mut ed, generation, 2, "sh deploy.sh").expect("its card");
    assert!(ed.offers[0].answers(false).iter().any(|(k, _)| *k == 'a'), "{:?}", ed.offers[0].answers(false));
    ed.offer_dismiss();
    std::fs::remove_dir_all(&dir).ok();
}

/// A command split as a shell splits it — quotes kept together, `#` a comment, a `$(…)` its own command — and
/// a path that's a build's scratch only below the project's `outputs` folder.
#[test]
fn a_line_is_split_as_a_shell_splits_it() {
    let words = |t: &str| commands(t).into_iter().map(|c| c.words.join(" ")).collect::<Vec<_>>();
    assert_eq!(words("a 'b c' \"d;e\"; f | g && h"), ["a b c d;e", "f", "g", "h"]);
    assert_eq!(words("T=$(mktemp -d x) # gone\nrm -rf \"$T\""), ["T=", "mktemp -d x", "rm -rf $T"]);
    assert_eq!(commands("rm -rf \"$T\"")[0].raw, "rm -rf \"$T\"", "as written, for `unless-part`");
    let root = std::path::Path::new("/p");
    let out = ["target".to_string()];
    assert!(in_outputs(root, root, "target/tmp/x", &out));
    assert!(in_outputs(root, &root.join("target"), "tmp/x", &out), "from where it runs");
    for not in ["src", "../target/x", "/p/target/x", "target/*", "$T", "~/target"] {
        assert!(!in_outputs(root, root, not, &out), "{not}");
    }
}
