//! `cargo xtask record rehearse --cwd FOLDER [--out DIR]` — take 4's rehearsal (its hands page, main's
//! `.sb/full-video-take4-hands.md` §4): the take's hands walked on a scratch copy of a finished project, with no Claude and
//! no spend, to prove before the take how its reaches land — the pickers' top rows, which way to a function lands on its
//! `fn` line, the selection after it, a chord's keys in one write.
//!
//! - **tarae only as a scene starts it**, never as `record take` does (that copies your config, whose claude is the real
//!   one): `scenes::live` with the stand-in `idle`, started by `shot::command` — refused unless its config names a stand-in
//!   (#247, `stand_in_only`), with a `PATH` whose `claude` refuses (`no_claude_path`). The take's surroundings
//!   (`take::surroundings`) write a copy of that config where tarae reads it (`XDG_CONFIG_HOME`): the copy is checked the
//!   same way before anything starts (`guard`), and its claude command said.
//! - **`--cwd`**: the scratch copy (`/Users/Shared/halil-rehearsal`), refused where it's the take's own folder
//!   (`THE_TAKES`) or under your home (`take::outside_home`: cargo would read your config). Nothing here makes, copies or
//!   removes it.
//! - **Rust**: the take's clone (`toolchain::TAKE_RUST`) on the `PATH` after the refusing `claude`'s folder, for
//!   rust-analyzer (`toolchain::env_for_a_rehearsal`: read, never marked).
//! - **The walk** (`steps`): each of §1's reaches by (c) `space f` + `/name`, (b) `space f` + `space s`, then (a) `space S`,
//!   each landing judged by the frame (the status line's `line:col` and the row of `fn name`), then `maf`, `space i`, the
//!   words typed with no return, `esc`. No `space l`, no ask sent. A chord in one write (`key --chord`), but the first use
//!   of `space f`, `space S`, `space i`, `space W` and `space v`, whose menu stands `MENU_STANDS`; the pauses between
//!   steps from one fixed table (`GAPS`), so a rehearsal and the take go alike. `state` read before every card's answer.
//! - **Its record**: the take in `DIR/take`, each step, every frame looked at and the checks in `DIR/rehearse.txt`.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::hands::{self, code};
use super::take::{self, TAKE_SIZE};
use super::{Spec, record, toolchain};
use crate::scenes::{self, docshots::Bg};
use crate::{Result, root, shot};

const USAGE: &str = "usage: cargo xtask record rehearse --cwd FOLDER [--out DIR]";

/// The take's own folder: never rehearsed in — a scratch copy of it is.
const THE_TAKES: &str = "/Users/Shared/halil";

/// The pauses between steps (ms), read in order and round again: one fixed table, so a rehearsal and the take type alike
/// (the hands page, §2b).
const GAPS: [u64; 12] = [640, 910, 480, 1260, 760, 1050, 560, 1690, 830, 980, 700, 1140];

/// The chords the video teaches: at their first use the menu stands `MENU_STANDS` before the second key; after that, and
/// every other chord, the keys go in one write.
const TAUGHT: [&str; 5] = ["space f", "space S", "space i", "space W", "space v"];
const MENU_STANDS: Duration = Duration::from_millis(900);

/// §1's reaches: take 3's time, the file's few letters, the function, a row's word that tells it from a namesake, the
/// words typed on its `space i` (never sent).
const REACHES: [(&str, &str, &str, Option<&str>, &str); 4] = [
    (
        "3022",
        "task",
        "open_tasks",
        None,
        "What's due should come first: late, then soonest due, then priority.",
    ),
    ("3175", "task", "open_tasks", None, "Add a test for this order with a fixed now."),
    ("3252", "main", "print_list", None, "Show each task's due date in words here."),
    ("3747", "board", "column", Some("Board"), "Order todo and doing by due here too, as the list does."),
];

/// `record rehearse …` (the module's doc).
pub(super) fn run(args: &[String]) -> Result<()> {
    let (mut cwd, mut out) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{a} wants a value — {USAGE}"));
        match a.as_str() {
            "--cwd" => cwd = Some(PathBuf::from(value()?)),
            "--out" => out = Some(PathBuf::from(value()?)),
            other => return Err(format!("{other}: {USAGE}")),
        }
    }
    let cwd = cwd.ok_or_else(|| format!("--cwd FOLDER: the scratch copy to rehearse in — {USAGE}"))?;
    let home =
        std::env::var_os("HOME").ok_or("no HOME: the rehearsal's folder must sit outside your home")?;
    let cwd = checked_cwd(&cwd, Path::new(&home))?;
    let out = out.unwrap_or_else(|| super::rec_dir("record-rehearse"));
    if out.exists() {
        return Err(format!("{} is there already — name another, or move it away first", out.display()));
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let bin = shot::build(&root(), "shot-docs", &["bundled-grammars"])?;
    let me = std::env::current_exe().map_err(|e| format!("this xtask: {e}"))?;
    let mut live = scenes::live("record-rehearse", "idle", Bg::Dark, &bin, &me, false)?;
    live.setup.cwd = cwd.clone();
    live.setup.files.clear();
    // (#247: refused here unless the scene's config names a stand-in; its PATH's claude refuses)
    let mut cmd = shot::command(&live.setup, &live.state, &live.homes)?;
    let scratch_home = hands::fresh_folder(Path::new("/tmp"), "tarae-rehearse")?;
    let file = live.setup.config.join("tarae/config.toml");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    // (a scene's config turns the language servers off; the rehearsal's rust-analyzer is the real one)
    let text = text.replacen("lsp = false\n", "", 1);
    let mut env = take::surroundings(&scratch_home, Some(&text), None, None)?;
    // (the copy tarae reads, by XDG_CONFIG_HOME: checked as shot::command checked the scene's)
    let claude = guard(&scratch_home.join("config/tarae/config.toml"))?;
    // (the take's Rust clone for rust-analyzer, after the refusing claude's folder on PATH)
    let rust = toolchain::env_for_a_rehearsal(&toolchain::Yours::here()?, Path::new(toolchain::TAKE_RUST))?;
    let no_claude = live.state.join("no-claude");
    for (k, v) in rust {
        match k {
            "PATH" => {
                let rest: Vec<PathBuf> = std::env::split_paths(&v).collect();
                let path = std::env::join_paths(std::iter::once(no_claude.clone()).chain(rest))
                    .map_err(|e| format!("the rehearsal's PATH: {e}"))?;
                env.push(("PATH", path));
            }
            _ => env.push((k, v)),
        }
    }
    let left = take::the_takes_env(&mut cmd, env);
    let take_dir = out.join("take");
    let mut h = Hands::new(&take_dir, &live.state);
    h.note(&format!("record rehearse: {} — in {}", bin.display(), cwd.display()));
    h.note(&format!("its claude: {claude} (the scenes' stand-in); PATH's claude: {}", no_claude.display()));
    h.note(&format!("record rehearse: {left}"));
    let spec = Spec {
        size: TAKE_SIZE,
        out: take_dir.clone(),
        outer: None,
        keys: Vec::new(),
        quit_after_keys: false,
        ui: Some(live.state.clone()),
        cwd: Some(cwd.clone()),
        hands: Some(take_dir.clone()),
    };
    let (done_tx, done) = mpsc::channel();
    let taking = std::thread::spawn(move || {
        let r = record(spec, cmd);
        let _ = done_tx.send(());
        r
    });
    let way = take_dir.join(hands::WAY_IN);
    while !way.exists() {
        if taking.is_finished() {
            return taking.join().map_err(|_| "the rehearsal panicked".to_string())?.map(|_| ());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    h.sock = Some(hands::way_in(Some(&take_dir))?);
    h.started = Instant::now();
    steps(&mut h);
    if done.recv_timeout(Duration::from_secs(60)).is_err() {
        h.note("tarae still running a minute after :q! — ended (SIGTERM)");
        if let Some(pid) = ui_sock(&live.state).and_then(|s| s.file_stem()?.to_str()?.parse::<i32>().ok()) {
            // SAFETY: a signal to the rehearsal's own tarae, found by its socket in the rehearsal's own state home.
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }
    let taken = taking.join().map_err(|_| "the rehearsal panicked".to_string())??;
    h.note(&taken.said(&take_dir));
    // (the checks after: the command the take ran is the scene's tarae)
    let p = take_dir.join("take.json");
    let ran: Value =
        std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let first = ran["command"][0].as_str().unwrap_or_default().to_string();
    h.note(&format!(
        "check: take.json's command {first:?} — {}",
        if Path::new(&first) == bin { "the scene's tarae" } else { "NOT the scene's tarae" }
    ));
    let p = out.join("rehearse.txt");
    std::fs::write(&p, h.said.join("\n") + "\n").map_err(|e| format!("{}: {e}", p.display()))?;
    let _ = std::fs::remove_dir_all(&scratch_home);
    drop(live);
    Ok(())
}

/// `cwd` made real, refused where it's the take's own folder or under `home` (cargo would read your config there).
fn checked_cwd(cwd: &Path, home: &Path) -> Result<PathBuf> {
    let said =
        |p: &Path| format!("{}: the take's own folder — rehearse in a scratch copy of it", p.display());
    if cwd == Path::new(THE_TAKES) {
        return Err(said(cwd));
    }
    let real = std::fs::canonicalize(cwd).map_err(|e| format!("{}: {e}", cwd.display()))?;
    if real == Path::new(THE_TAKES) || std::fs::canonicalize(THE_TAKES).is_ok_and(|t| t == real) {
        return Err(said(cwd));
    }
    if !real.is_dir() {
        return Err(format!("{}: not a folder", cwd.display()));
    }
    take::outside_home(&real, home)?;
    Ok(real)
}

/// The config copy tarae will read, refused unless its claude is a stand-in (`shot::stand_in_only`, #247); its claude
/// command and arguments, as tarae will start them.
fn guard(copy: &Path) -> Result<String> {
    let text = std::fs::read_to_string(copy).map_err(|e| format!("{}: {e}", copy.display()))?;
    shot::stand_in_only(&text, copy)?;
    let t: toml::Table = text.parse().map_err(|e| format!("{}: {e}", copy.display()))?;
    let llm = t.get("llm");
    let command = llm.and_then(|l| l.get("command")).and_then(toml::Value::as_str).unwrap_or_default();
    let args: Vec<&str> =
        (llm.and_then(|l| l.get("args")).and_then(toml::Value::as_array).into_iter().flatten())
            .filter_map(toml::Value::as_str)
            .collect();
    Ok(format!("{command} {}", args.join(" ")).trim_end().to_string())
}

/// The socket of the one tarae in `state`'s UI folder.
fn ui_sock(state: &Path) -> Option<PathBuf> {
    let dir = std::fs::read_dir(state.join("tarae/ui")).ok()?;
    dir.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "sock"))
}

/// The rehearsal's hands: each step a command line as the driver types it (`record …`), made a request as the client
/// makes it (`hands::request`), its answer kept with the clock; the frame read from tarae's UI socket.
struct Hands {
    sock: Option<PathBuf>,
    state: PathBuf,
    started: Instant,
    said: Vec<String>,
    gap: usize,
    taught: Vec<&'static str>,
}

impl Hands {
    fn new(_take: &Path, state: &Path) -> Hands {
        Hands {
            sock: None,
            state: state.to_path_buf(),
            started: Instant::now(),
            said: Vec::new(),
            gap: 0,
            taught: Vec::new(),
        }
    }

    fn run(&mut self, line: &str) -> (i32, String) {
        let words: Vec<String> = line.split(' ').map(String::from).collect();
        let at = self.started.elapsed().as_millis() as u64;
        let answer = match &self.sock {
            Some(sock) => hands::request(&words[0], &words[1..]).and_then(|(req, _)| hands::ask(sock, &req)),
            None => Err("no way in yet".into()),
        };
        let (c, said) = answer.unwrap_or_else(|e| (code::ERROR, e));
        let line = format!("{:>9}  record {line}\n           {c} · {said}", crate::cut::clock(at));
        eprintln!("{line}");
        self.said.push(line);
        (c, said)
    }

    fn note(&mut self, words: &str) {
        eprintln!("# {words}");
        self.said.push(format!("# {words}"));
    }

    /// The next pause of the fixed table.
    fn pause(&mut self) {
        std::thread::sleep(Duration::from_millis(GAPS[self.gap % GAPS.len()]));
        self.gap += 1;
    }

    /// A chord: in one write, but at a taught chord's first use, where its menu stands `MENU_STANDS` first.
    fn chord(&mut self, chord: &'static str) {
        if TAUGHT.contains(&chord) && !self.taught.contains(&chord) {
            self.taught.push(chord);
            let (first, rest) = chord.split_once(' ').unwrap_or((chord, ""));
            self.run(&format!("key {first}"));
            std::thread::sleep(MENU_STANDS);
            self.run(&format!("key {rest}"));
            return;
        }
        self.run(&format!("key --chord {chord}"));
    }

    /// The screen tarae drew last, a row a string (its UI socket's `frame`).
    fn frame(&mut self) -> Vec<String> {
        let got = (|| -> Result<Vec<String>> {
            let sock = ui_sock(&self.state).ok_or("no tarae UI socket in the rehearsal's state home")?;
            let s = shot::ui_connect(&sock).map_err(|e| format!("{}: {e}", sock.display()))?;
            let v = shot::ui_ask_on(s, &sock, &json!({ "get": "frame", "timeout_ms": 5000 }))?;
            let f = &v["frame"];
            let n = |k: &str| f[k].as_u64().and_then(|n| u16::try_from(n).ok()).unwrap_or(0);
            let (cols, rows) = (n("cols"), n("rows"));
            if cols == 0 || rows == 0 {
                return Err("no frame in its answer".into());
            }
            let mut p = vt100::Parser::new(rows, cols, 0);
            p.process(f["bytes"].as_str().unwrap_or_default().as_bytes());
            p.process(shot::FRAME_END);
            Ok(crate::recording::screen_cells(p.screen()).into_iter().map(|r| r.concat()).collect())
        })();
        got.unwrap_or_else(|e| {
            self.note(&format!("frame: {e}"));
            Vec::new()
        })
    }

    /// The frame kept in the record, its rows trimmed, under `what`.
    fn shown(&mut self, what: &str) -> Vec<String> {
        let rows = self.frame();
        self.note(&format!("frame — {what}"));
        for (r, row) in rows.iter().enumerate() {
            self.said.push(format!("  {r:2}|{}", row.trim_end()));
        }
        rows
    }

    /// The keys back in the code: `esc` until they are (three at most).
    fn back_to_code(&mut self) {
        for _ in 0..3 {
            if self.run("wait keys code --for 0").0 == code::HELD {
                return;
            }
            self.run("key esc");
        }
    }

    /// Each card in front answered `y` as it reads: its frame kept and `state` read first, `C-q` where it waits.
    fn cards(&mut self) {
        self.cards_within(6);
    }

    /// …the first waited for `patience` seconds.
    fn cards_within(&mut self, patience: u64) {
        for i in 0..5 {
            let (c, said) = self.run(&format!("wait card --for {}", if i == 0 { patience } else { 2 }));
            if c != code::HELD && c != code::CARD {
                return;
            }
            self.shown("a card up");
            self.run("state");
            if said.contains("waits") {
                self.run("key C-q");
            }
            self.pause();
            self.run("key --to card y");
        }
    }

    /// A picker's rows that hold `what`, its input row left out — the top one first.
    fn rows_with(rows: &[String], what: &str) -> Vec<String> {
        // (a picker's list is its frame's first column, up to the top border's ┬; its preview beside it)
        let cut = rows.iter().find(|r| r.contains('╭')).and_then(|r| r.chars().position(|c| c == '┬'));
        let list = |r: &String| match (cut, r.trim_start().starts_with('│')) {
            (Some(cut), true) => {
                let cell: String = r.chars().skip(1).take(cut.saturating_sub(1)).collect();
                cell.trim_matches(|c: char| c.is_whitespace() || c == '│' || c == '▎').trim().to_string()
            }
            (None, true) => r.split('│').nth(1).unwrap_or_default().trim().to_string(),
            _ => r.trim().to_string(),
        };
        rows.iter().map(list).filter(|r| r.contains(what) && !r.starts_with('›')).collect()
    }
}

/// Where the frame's cursor is against `name`'s `fn` line: (on it, said). The status line's `line:col`, the row of
/// `fn name` by its gutter's number, and the breadcrumb.
fn landed(rows: &[String], name: &str) -> (bool, String) {
    let at = rows.iter().rev().find_map(|r| {
        r.split_whitespace().rev().find_map(|w| {
            let (l, c) = w.split_once(':')?;
            (c.chars().all(|ch| ch.is_ascii_digit()) && !c.is_empty())
                .then(|| l.parse::<usize>().ok())
                .flatten()
        })
    });
    let want = format!("fn {name}");
    let fn_at = rows.iter().find_map(|r| {
        let part = r.split('│').find(|p| p.contains(&want))?;
        let digits: String = part
            .trim_start()
            .trim_start_matches(|c: char| !c.is_ascii_digit())
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse::<usize>().ok()
    });
    let crumb = rows.first().is_some_and(|r| r.contains(&format!("ƒ {name}")));
    let on = at.is_some() && at == fn_at;
    (on, format!("cursor line {at:?}, `{want}` on line {fn_at:?}, breadcrumb ƒ {name}: {crumb}"))
}

/// The rehearsal's walk (the module's doc).
fn steps(h: &mut Hands) {
    h.note("tarae up, the keys in the code");
    h.run("wait keys code --for 60");
    h.shown("the start");
    // (a Rust file first: rust-analyzer starts, and the code-trust card comes)
    h.chord("space f");
    h.run("wait keys picker --for 10");
    h.run("type --to picker main");
    h.pause();
    let rows = h.shown("space f, main");
    h.note(&format!("the picker's rows with main: {:?}", Hands::rows_with(&rows, "main")));
    h.run("key <ret>");
    h.cards();
    h.note("waiting for rust-analyzer: in state's procs, then under 5% cpu twice running (every 3 s, four minutes at most)");
    let (mut ready, mut quiet) = (false, 0);
    for _ in 0..80 {
        let (_, s) = h.run("state");
        let cpu = s.lines().find(|l| l.contains("rust-analyzer")).map(|l| {
            let after = l.split("cpu ").nth(1).unwrap_or_default();
            after.split('%').next().and_then(|n| n.trim().parse::<f64>().ok())
        });
        quiet = match cpu {
            Some(Some(c)) if c < 5.0 => quiet + 1,
            _ => 0,
        };
        if quiet == 2 {
            ready = true;
            break;
        }
        h.cards_within(0);
        std::thread::sleep(Duration::from_secs(3));
    }
    h.note(&format!("rust-analyzer ready: {ready}"));
    for (when, file, name, row, words) in REACHES {
        h.note(&format!("── the reach of take 3's {when} s: {name} in {file}"));
        // (c) space f + the file, then / and the name, n where a use comes first
        h.back_to_code();
        h.chord("space f");
        h.run("wait keys picker --for 10");
        h.run(&format!("type --to picker {file}"));
        h.pause();
        let rows = h.shown(&format!("(c) space f, {file}"));
        h.note(&format!("(c) the picker's rows with {file}: {:?}", Hands::rows_with(&rows, file)));
        h.run("key <ret>");
        h.run("wait keys code --for 5");
        h.chord("g g");
        h.run("key /");
        h.run("wait keys prompt --for 5");
        h.run(&format!("type --to prompt {name}"));
        h.run("key <ret>");
        h.pause();
        let mut tries = 0;
        loop {
            let rows = h.shown(&format!("(c) /{name}"));
            let (on, said) = landed(&rows, name);
            h.note(&format!(
                "(c) after {tries} n: {said} — {}",
                if on { "ON the fn line" } else { "not on it" }
            ));
            if on || tries == 3 {
                break;
            }
            tries += 1;
            h.run("key n");
            h.pause();
        }
        // (b) space f + the file (in front already), then space s and the name
        h.back_to_code();
        h.chord("g g");
        h.chord("space s");
        h.run("wait keys picker --for 10");
        h.run(&format!("type --to picker {name}"));
        h.pause();
        let rows = h.shown(&format!("(b) space s, {name}"));
        h.note(&format!("(b) the picker's rows with {name}: {:?}", Hands::rows_with(&rows, name)));
        h.run("key <ret>");
        h.run("wait keys code --for 5");
        h.pause();
        let rows = h.shown(&format!("(b) landed, {name}"));
        let (on, said) = landed(&rows, name);
        h.note(&format!("(b) {said} — {}", if on { "ON the fn line" } else { "not on it" }));
        // (a) space S, the name, the row that tells it from a namesake
        h.back_to_code();
        h.chord("g g");
        h.chord("space S");
        h.run("wait keys picker --for 10");
        h.run(&format!("type --to picker {name}"));
        // (the workspace's symbols come from rust-analyzer as the words are typed)
        std::thread::sleep(Duration::from_millis(2500));
        let rows = h.shown(&format!("(a) space S, {name}"));
        let found = Hands::rows_with(&rows, name);
        h.note(&format!("(a) the picker's rows with {name}: {found:?}"));
        if let Some(word) = row {
            match found.iter().position(|r| r.contains(word)) {
                Some(i) => {
                    h.note(&format!("(a) the {word} row is row {} of them: down {i}", i + 1));
                    for _ in 0..i {
                        h.run("key down");
                    }
                }
                None => h.note(&format!("(a) no row with {word}")),
            }
        }
        h.run("key <ret>");
        h.run("wait keys code --for 5");
        h.pause();
        let rows = h.shown(&format!("(a) landed, {name}"));
        let (on, said) = landed(&rows, name);
        h.note(&format!("(a) {said} — {}", if on { "ON the fn line" } else { "not on it" }));
        // then maf, and space i with the words, never sent
        h.chord("m a");
        h.run("key f");
        h.pause();
        h.shown(&format!("maf on {name}"));
        h.chord("space i");
        h.run("wait keys ask --for 10");
        h.run(&format!("type --to ask {words}"));
        h.pause();
        h.shown("space i, the words typed, not sent");
        h.run("key esc");
        h.pause();
        h.back_to_code();
        h.cards();
    }
    h.note("the end: :q!");
    h.back_to_code();
    h.run("key :");
    h.run("type --to command --send q!");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tarae_testkit::scratch::Scratch;

    /// The config copy a rehearsal's tarae reads is refused where it names the real claude, or none (tarae's default is
    /// the real one), and taken where it names a stand-in — its command said as tarae will start it.
    #[test]
    fn a_rehearsals_config_naming_the_real_claude_is_refused() {
        let dir = Scratch::new("xtask-rehearse-guard");
        let file = dir.path().join("config.toml");
        for (text, why) in [
            ("[llm]\ncommand = \"claude\"\n", "the real claude by name"),
            ("[llm]\ncommand = \"/opt/homebrew/bin/claude\"\n", "the real claude by its path"),
            ("[editor]\n", "no llm.command: tarae's default, the real claude"),
        ] {
            std::fs::write(&file, text).unwrap();
            assert!(guard(&file).is_err(), "{why}: refused");
        }
        std::fs::write(&file, "[llm]\ncommand = \"/x/xtask\"\nargs = [\"scenes\", \"--claude\", \"idle\"]\n")
            .unwrap();
        assert_eq!(guard(&file).unwrap(), "/x/xtask scenes --claude idle");
    }

    /// The take's own folder is never rehearsed in, and neither is a folder under the home.
    #[test]
    fn the_takes_own_folder_and_one_in_the_home_are_refused() {
        let dir = Scratch::new("xtask-rehearse-cwd");
        assert!(checked_cwd(Path::new(THE_TAKES), dir.path()).unwrap_err().contains("the take's own folder"));
        let under = dir.path().join("copy");
        std::fs::create_dir(&under).unwrap();
        assert!(checked_cwd(&under, dir.path()).is_err(), "under the home");
        let elsewhere = Scratch::new("xtask-rehearse-elsewhere");
        assert!(checked_cwd(elsewhere.path(), dir.path()).is_ok(), "outside it");
    }

    /// A landing judged by the frame: on the `fn` line where the status line's line is the gutter's number of the row
    /// with `fn name` — in a split's code pane too; not where the cursor is elsewhere.
    #[test]
    fn a_landing_is_the_status_lines_line_against_the_fn_rows_number() {
        let rows = |at: &str| {
            vec![
                "─ halil › src › task.rs › ƒ open_tasks ────".to_string(),
                " 168 pub fn open_tasks(tasks: &[Task]) -> Vec<&Task> {".to_string(),
                " 169     let mut open = tasks".to_string(),
                format!(" ● normal                                   rust   {at}"),
            ]
        };
        assert!(landed(&rows("168:1"), "open_tasks").0);
        assert!(!landed(&rows("179:1"), "open_tasks").0);
        let split = vec![
            "─ review ── last turn ──┬─ halil › src › board.rs › τ Board › ƒ column ─".to_string(),
            "  4 files  +811 −3  │●100     fn column(&self, c: usize) -> Vec<&Task> {".to_string(),
            " ● normal   rust   100:5".to_string(),
        ];
        assert!(landed(&split, "column").0);
    }
}
