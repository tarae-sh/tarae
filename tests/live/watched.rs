//! tarae as rust-analyzer's file watcher (W1b, `lspwatch.rs`), live: the real rust-analyzer told tarae watches, on
//! scratch crates and clones in folders of their own (`Scratch`, the live sandbox's), never this repository. No
//! claude: `llm.command` is `true` or `sh -c 'exit 0'`. Each test prints `W1B` lines for the report and keeps the
//! server's stderr (tarae.log's `rust-analyzer stderr` lines) in its output. `TARAE_W1B_CASES` picks some of a
//! test's cases (comma-separated).
//!
//! "As now" is the server watching by itself: a runtime folder (`TARAE_RUNTIME`) whose rust pack says
//! `watched-files = false` — one key of the built-in pack changed, as a pack of yours does.

use super::*;
use std::path::PathBuf;

use tarae_testkit::scratch::Scratch as Dir;

const CODE_CARD: &str = "Trust this repository's code";
const PROJECT_CARD: &str = "Trust this project";

fn cases() -> Option<String> {
    std::env::var("TARAE_W1B_CASES").ok().filter(|v| !v.is_empty())
}

fn picked(case: &str) -> bool {
    cases().is_none_or(|c| c.split(',').any(|n| n.trim() == case))
}

/// The code-trust card answered `y` and a project's settings card `n`, whichever comes, until the code is trusted
/// and no card is up.
fn trusted(t: &mut Term) {
    let end = Instant::now() + LIVE_TIMEOUT;
    let mut code = false;
    while Instant::now() < end {
        let s = t.screen();
        // (a card that came on its own takes keys once focused: its foot says `C-q answer` until then — one card is
        // drawn at a time, the next waiting behind, unfocused when it comes to the front)
        if (s.contains(CODE_CARD) || s.contains(PROJECT_CARD)) && s.contains("C-q answer") {
            t.send("\x11");
        } else if s.contains(CODE_CARD) {
            t.send("y");
            code = true;
        } else if s.contains(PROJECT_CARD) {
            t.send("n");
        } else if code {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    panic!("the trust cards never went\n{}", t.screen());
}

/// The lines of tarae.log a report keeps: the server's (its stderr too), the tree watch's and the watch's.
fn kept_log(sb: &Sandbox) -> Vec<String> {
    let text = std::fs::read_to_string(sb.state().join("tarae.log")).unwrap_or_default();
    (text.lines())
        .filter(|l| l.contains("rust-analyzer") || l.contains("tree-watch") || l.contains(" watch "))
        .map(str::to_string)
        .collect()
}

fn logged(sb: &Sandbox, what: &str) -> usize {
    std::fs::read_to_string(sb.state().join("tarae.log")).unwrap_or_default().matches(what).count()
}

/// The server's stderr as the log kept it, and whether it panicked.
fn said_stderr(sb: &Sandbox, name: &str) -> bool {
    let lines: Vec<String> =
        kept_log(sb).into_iter().filter(|l| l.contains("rust-analyzer") && l.contains("stderr")).collect();
    eprintln!("W1B {name}: rust-analyzer's stderr, {} lines:", lines.len());
    for l in &lines {
        eprintln!("    {l}");
    }
    lines.iter().any(|l| l.contains("panicked"))
}

fn said_watch(sb: &Sandbox, name: &str) {
    eprintln!("W1B {name}: tarae.log's watch lines:");
    for l in kept_log(sb).iter().filter(|l| l.contains("tree-watch") || l.contains(" watch ")) {
        eprintln!("    {l}");
    }
}

/// The server was told tarae watches: the tree watch started, and no line says it watches by itself.
fn declared(sb: &Sandbox) -> bool {
    logged(sb, "streams started in") > 0 && logged(sb, "watches its files itself") == 0
}

/// A runtime folder whose rust pack says what `pack` says, added to `sb`'s tarae (`TARAE_RUNTIME`).
fn with_pack(sb: Sandbox, pack: &str) -> Sandbox {
    let dir = sb.outside("runtime");
    std::fs::create_dir_all(dir.join("packs/rust")).unwrap();
    std::fs::write(dir.join("packs/rust/pack.toml"), pack).unwrap();
    sb.with_env("TARAE_RUNTIME", dir.to_str().unwrap())
}

const AS_NOW: &str = "[server.rust-analyzer]\nwatched-files = false\n";

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// `path`'s modified time set `secs` back.
fn aged(path: &Path, secs: u64) {
    let then = std::time::SystemTime::now() - Duration::from_secs(secs);
    std::fs::File::options().write(true).open(path).unwrap().set_modified(then).unwrap();
}

/// `gd` on `word` (found by `/`) until the screen's first row names `file`, within `within`: how long it took.
fn jumps(t: &mut Term, word: &str, file: &str, within: Duration) -> Option<Duration> {
    let t0 = Instant::now();
    loop {
        t.send(&format!("/{word}\r"));
        t.send("gd");
        if t.poll_for(Duration::from_secs(2), |s| s.rows[0].contains(file)).is_some() {
            return Some(t0.elapsed());
        }
        if t0.elapsed() > within {
            return None;
        }
        esc(t);
    }
}

fn git_in(sb: &Sandbox, dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", sb.git_config())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdout(std::process::Stdio::null())
        .status()
        .expect("git");
    assert!(st.success(), "git {args:?} in {}", dir.display());
}

fn cargo_build(dir: &Path, build_dir: &Path) -> Duration {
    let t0 = Instant::now();
    let st = Command::new("cargo")
        .args(["build", "-q"])
        .current_dir(dir)
        .env("CARGO_BUILD_BUILD_DIR", build_dir)
        .status()
        .expect("cargo");
    assert!(st.success(), "cargo build in {}", dir.display());
    t0.elapsed()
}

/// A crate of its own (`[workspace]`: no outer one).
fn a_crate(dir: &Path, name: &str, build: bool) {
    let script = if build { "build = \"build.rs\"\n" } else { "" };
    write(
        &dir.join("Cargo.toml"),
        &format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{script}\n[workspace]\n"
        ),
    );
}

/// The front workspace's root, as `tarae state` says it.
fn front_root(t: &Term) -> Option<PathBuf> {
    let v: serde_json::Value = serde_json::from_str(&t.state(&["--json"])).ok()?;
    let w = v["workspaces"].as_array()?.iter().find(|w| w["id"] == v["front"])?;
    w["root"].as_str().map(PathBuf::from)
}

fn quit(mut t: Term) {
    t.command("qa!");
    let _ = t.wait_exit();
}

// ---- what a process holds (macOS's proc_pidinfo) ----

/// `PROC_PIDLISTTHREADS` and `PROC_PIDLISTFDS` (not in libc).
const PROC_PIDLISTTHREADS: i32 = 6;
const PROC_PIDLISTFDS: i32 = 1;

/// A process's threads, descriptors, resident memory and CPU so far.
#[derive(Clone, Debug, Default)]
struct Held {
    threads: Vec<(String, Duration)>,
    fds: usize,
    rss: u64,
    cpu: Duration,
}

/// How many of the kernel's CPU units one nanosecond is: `proc_pidinfo`'s times on Apple silicon are in mach
/// ticks — calibrated on this process against `getrusage` (its own count in microseconds).
fn ns_per_unit() -> f64 {
    static SCALE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *SCALE.get_or_init(|| {
        let t0 = Instant::now();
        let mut x = 0u64;
        while t0.elapsed() < Duration::from_millis(300) {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
        }
        std::hint::black_box(x);
        // SAFETY: a plain value, filled by the call.
        let mut r: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: fills only `r`.
        unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut r) };
        let us = |t: libc::timeval| t.tv_sec as f64 * 1e6 + t.tv_usec as f64;
        let ns = (us(r.ru_utime) + us(r.ru_stime)) * 1000.0;
        let raw = task_info(std::process::id()).map_or(0, |t| t.pti_total_user + t.pti_total_system) as f64;
        let scale = if raw > 0.0 { ns / raw } else { 1.0 };
        // (1 on Intel; mach's timebase, 125/3, on Apple silicon)
        eprintln!("W1B cpu units: {scale:.2} ns a unit, calibrated on this process");
        scale
    })
}

fn task_info(pid: u32) -> Option<libc::proc_taskinfo> {
    // SAFETY: a plain value, filled by the call.
    let mut t: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as i32;
    // SAFETY: fills only `t`, of the size given.
    let r = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDTASKINFO, 0, (&raw mut t).cast(), size) };
    (r == size).then_some(t)
}

fn held(pid: u32) -> Held {
    let scale = ns_per_unit();
    let units = |u: u64| Duration::from_nanos((u as f64 * scale) as u64);
    let task = task_info(pid);
    // (only the task's totals are in those units: `PROC_PIDTHREADINFO`'s thread times are nanoseconds already —
    // the first card's member case, its worker's 211 ms inside the process's 228 ms by `getrusage`)
    let mut ids = vec![0u64; 2048];
    // SAFETY: fills at most the buffer's size in handles.
    let n = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            PROC_PIDLISTTHREADS,
            0,
            ids.as_mut_ptr().cast(),
            (ids.len() * 8) as i32,
        )
    };
    ids.truncate(n.max(0) as usize / 8);
    let size = std::mem::size_of::<libc::proc_threadinfo>() as i32;
    let threads = (ids.into_iter())
        .filter_map(|id| {
            // SAFETY: a plain value, filled by the call.
            let mut t: libc::proc_threadinfo = unsafe { std::mem::zeroed() };
            // SAFETY: fills only `t`, of the size given.
            let r = unsafe {
                libc::proc_pidinfo(pid as i32, libc::PROC_PIDTHREADINFO, id, (&raw mut t).cast(), size)
            };
            let name: Vec<u8> = t.pth_name.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
            let name = if name.is_empty() { "?".into() } else { String::from_utf8_lossy(&name).into_owned() };
            (r == size).then(|| (name, Duration::from_nanos(t.pth_user_time + t.pth_system_time)))
        })
        .collect();
    let mut fds = vec![0u64; 8192];
    // SAFETY: fills at most the buffer's size (`proc_fdinfo`: 8 bytes each).
    let n = unsafe {
        libc::proc_pidinfo(pid as i32, PROC_PIDLISTFDS, 0, fds.as_mut_ptr().cast(), (fds.len() * 8) as i32)
    };
    Held {
        threads,
        fds: n.max(0) as usize / 8,
        rss: task.map_or(0, |t| t.pti_resident_size),
        cpu: task.map_or(Duration::ZERO, |t| units(t.pti_total_user + t.pti_total_system)),
    }
}

/// Threads by name, counted (`tarae-lsp-w ×2`).
fn names(h: &Held) -> std::collections::BTreeMap<String, usize> {
    let mut m = std::collections::BTreeMap::new();
    for (n, _) in &h.threads {
        *m.entry(n.clone()).or_insert(0) += 1;
    }
    m
}

/// What `now` holds beyond `before`: threads by name (more or fewer), descriptors, resident memory.
fn added(before: &Held, now: &Held) -> String {
    let (b, n) = (names(before), names(now));
    let mut threads = Vec::new();
    for (name, &c) in &n {
        let was = b.get(name).copied().unwrap_or(0);
        if c != was {
            threads.push(format!("{name} {:+}", c as i64 - was as i64));
        }
    }
    for (name, &was) in b.iter().filter(|(k, _)| !n.contains_key(*k)) {
        threads.push(format!("{name} -{was}"));
    }
    format!(
        "threads {:+} [{}] · descriptors {:+} · resident {:+.1} MB",
        now.threads.len() as i64 - before.threads.len() as i64,
        threads.join(", "),
        now.fds as i64 - before.fds as i64,
        (now.rss as f64 - before.rss as f64) / 1e6
    )
}

fn worker(h: &Held) -> Duration {
    h.threads.iter().filter(|t| t.0 == "tarae-treewatch").map(|t| t.1).sum()
}

/// The rust-analyzer below `tarae` (its pid).
fn server_of(tarae: u32) -> Option<u32> {
    tree(tarae).into_iter().find(|p| p.1 == "rust-analyzer").map(|p| p.0)
}

/// fseventsd's CPU so far and resident memory (KB), as `ps` says them — found by `pgrep -x` (`ps -axo comm=` gives
/// the full path, which the first card's search missed).
fn fseventsd() -> String {
    let pid = Command::new("pgrep").args(["-x", "fseventsd"]).output().unwrap();
    let pid = String::from_utf8_lossy(&pid.stdout).lines().next().unwrap_or_default().trim().to_string();
    if pid.is_empty() {
        return "fseventsd not found".into();
    }
    let out = Command::new("ps").args(["-o", "time=,rss=", "-p", &pid]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() { format!("fseventsd {pid}: ps said nothing") } else { text }
}

fn share(cpu: Duration, wall: Duration) -> f64 {
    cpu.as_secs_f64() / wall.as_secs_f64().max(1e-9) * 100.0
}

// ---- the load: each step the status line showed ----

/// rust-analyzer's step on the status line: its title, what it's at aside.
fn step_of(s: &Screen) -> Option<String> {
    let st = s.status();
    let i = st.find("rust-analyzer:")?;
    let title = st[i..].split(" · ").next().unwrap_or_default();
    Some(title.split("   ").next().unwrap_or_default().trim().to_string())
}

/// The load, from `from`: each step with when it was first and last seen (a poll every 50 ms), and when the
/// server was quiescent — or, `scan_only`, once `Roots Scanned` was seen and gone 2 s.
struct Load {
    steps: Vec<(String, Duration, Duration)>,
    quiescent: Option<Duration>,
    /// Why it stopped with no quiescence: the limit it waited (`within`), or none when `scan_only` ended it.
    limit: Option<Duration>,
}

impl Load {
    fn scanned(&self) -> String {
        let scan = self.steps.iter().filter(|s| s.0.contains("Roots Scanned"));
        let (first, last) = scan.fold((None, None), |(f, _), s| (f.or(Some(s.1)), Some(s.2)));
        match (first, last) {
            (Some(f), Some(l)) => {
                format!("Roots Scanned seen {:.2}s – {:.2}s", f.as_secs_f64(), l.as_secs_f64())
            }
            _ => "Roots Scanned never seen (shorter than a 50 ms poll)".into(),
        }
    }

    fn said(&self, name: &str) {
        eprintln!(
            "W1B {name}: {} · quiescent {}",
            self.scanned(),
            match (self.quiescent, self.limit) {
                (Some(q), _) => format!("at {:.1}s", q.as_secs_f64()),
                (None, Some(l)) => format!("NOT within the limit, {}s", l.as_secs()),
                (None, None) => "not waited for (a scan-only row: 2 s after Roots Scanned ended)".into(),
            }
        );
        for (s, f, l) in &self.steps {
            eprintln!("      {:>8.2}s – {:>8.2}s  {s}", f.as_secs_f64(), l.as_secs_f64());
        }
    }
}

fn load(t: &mut Term, sb: &Sandbox, from: Instant, within: Duration, scan_only: bool) -> Load {
    let log = sb.state().join("tarae.log");
    let quiescent = || {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let last = text.lines().rev().find(|l| l.contains(" rust-analyzer ") && l.contains(" quiescent "));
        last.is_some_and(|l| l.ends_with("nothing left to load"))
    };
    let mut out = Load { steps: Vec::new(), quiescent: None, limit: None };
    let (end, mut read) = (Instant::now() + within, Instant::now());
    let mut scan_gone: Option<Instant> = None;
    while Instant::now() < end {
        let now = from.elapsed();
        let step = step_of(&t.screen());
        if let Some(w) = &step {
            match out.steps.last_mut() {
                Some(l) if l.0 == *w => l.2 = now,
                _ => out.steps.push((w.clone(), now, now)),
            }
        }
        let scanning = step.as_deref().is_some_and(|s| s.contains("Roots Scanned"));
        if scanning {
            scan_gone = None;
        } else if scan_gone.is_none() && out.steps.iter().any(|s| s.0.contains("Roots Scanned")) {
            scan_gone = Some(Instant::now());
        }
        if scan_only && scan_gone.is_some_and(|g| g.elapsed() > Duration::from_secs(2)) {
            return out;
        }
        if read.elapsed() > Duration::from_millis(500) {
            read = Instant::now();
            if quiescent() {
                out.quiescent = Some(from.elapsed());
                return out;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    out.limit = Some(within);
    out
}

// ---- the tests ----

/// The generated-file case through tarae, for both places the build's outputs go: a build script writes
/// `$OUT_DIR/gen.rs` from `input.txt`, the crate `include!`s it and calls `target_fn()`. Loaded, `input.txt`
/// becomes `target_fn` and `cargo build` runs outside tarae, nothing saved: the server's `gd` must reach the
/// generated definition. Below `target` the out dir is a root of its own (in the shared stream); in a build folder
/// outside the project it's watched as below one of the project's output folders (`[builds]`). Cases
/// `target`, `outside`.
#[test]
#[ignore = "live: run with `cargo live` (rust-analyzer; outside the sandbox: FSEvents)"]
fn watched_files_a_generated_file_follows_the_build() {
    if !have("cargo") || !have("rust-analyzer") {
        return;
    }
    let mut stale = Vec::new();
    for case in ["target", "outside"].into_iter().filter(|c| picked(c)) {
        let sb = Sandbox::live(&format!("w1b-gen-{case}"));
        let build = if case == "target" { sb.file("target") } else { sb.outside("build-dir") };
        let sb = sb.with_env("CARGO_BUILD_BUILD_DIR", build.to_str().unwrap());
        sb.write_config(&format!("{LIVE_CONFIG}[llm]\ncommand = \"true\"\n"));
        a_crate(&sb.work(), "wgen", true);
        sb.write(
            "build.rs",
            "fn main() {\n    println!(\"cargo:rerun-if-changed=input.txt\");\n    \
             let name = std::fs::read_to_string(\"input.txt\").unwrap();\n    \
             let out = std::env::var(\"OUT_DIR\").unwrap();\n    \
             std::fs::write(format!(\"{out}/gen.rs\"), format!(\"pub fn {}() {{}}\\n\", name.trim())).unwrap();\n}\n",
        );
        write(
            &sb.file("src/lib.rs"),
            "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\n\npub fn call() {\n    target_fn();\n}\n",
        );
        sb.write("input.txt", "other_fn\n");
        let mut t = sb.spawn(&["src/lib.rs"]);
        trusted(&mut t);
        let from = Instant::now();
        let l = load(&mut t, &sb, from, LOADED_WITHIN, false);
        l.said(&format!("generated, {case}"));
        let before = jumps(&mut t, "target_fn", "gen.rs", Duration::from_secs(4));
        sb.write("input.txt", "target_fn\n");
        let built = cargo_build(&sb.work(), &build);
        let after = jumps(&mut t, "target_fn", "gen.rs", Duration::from_secs(60));
        eprintln!(
            "W1B generated, {case}: told tarae watches {} · before the build gd {} · cargo build {:.1}s · after it {}",
            declared(&sb),
            if before.is_some() { "ALREADY reached gen.rs" } else { "didn't reach gen.rs (right)" },
            built.as_secs_f64(),
            after.map_or("STALE for 60 s".into(), |d| format!("reached gen.rs in {:.1}s", d.as_secs_f64())),
        );
        said_watch(&sb, &format!("generated, {case}"));
        let panicked = said_stderr(&sb, &format!("generated, {case}"));
        if after.is_none() || before.is_some() || panicked || !declared(&sb) {
            stale.push(case);
        }
        quit(t);
    }
    assert!(stale.is_empty(), "the generated file didn't follow, or the case proved nothing, in {stale:?}");
}

/// Events while the server's first `Roots Scanned` runs don't break its load (#374's cousin, for watched files):
/// a crate with 20,000 files to scan, a file made or written every 20 ms from tarae's start to the load's end. Then
/// the plan's played change: `mod late;` unresolved, `src/late.rs` made by another process, nobody saves — `gd` on
/// `late::arrived` reaches it (tarae draws no rust-analyzer native diagnostic for the unresolved module here: the
/// first card's observable was wrong).
#[test]
#[ignore = "live: run with `cargo live` (rust-analyzer; outside the sandbox: FSEvents)"]
fn watched_files_an_event_during_the_first_scan() {
    if !have("cargo") || !have("rust-analyzer") {
        return;
    }
    let sb = Sandbox::live("w1b-scan");
    sb.write_config(&format!("{LIVE_CONFIG}[llm]\ncommand = \"true\"\n"));
    a_crate(&sb.work(), "wscan", false);
    write(&sb.file("src/lib.rs"), "mod late;\n\npub fn call() {\n    late::arrived();\n}\n");
    for i in 0..20_000 {
        write(&sb.file(&format!("big/d{}/f{i}.rs", i / 1000)), "pub fn f() {}\n");
    }
    let mut t = sb.spawn(&["src/lib.rs"]);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writes = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Instant>::new()));
    let writer = {
        let (stop, writes, src) = (stop.clone(), writes.clone(), sb.file("src"));
        std::thread::spawn(move || {
            let mut i = 0usize;
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                write(&src.join(format!("churn{}.rs", i % 50)), &format!("pub fn c{i}() {{}}\n"));
                writes.lock().unwrap().push(Instant::now());
                i += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
        })
    };
    trusted(&mut t);
    let from = Instant::now();
    let l = load(&mut t, &sb, from, LOADED_WITHIN, false);
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = writer.join();
    l.said("first scan");
    let w = writes.lock().unwrap().clone();
    let scan: Vec<&(String, Duration, Duration)> =
        l.steps.iter().filter(|s| s.0.contains("Roots Scanned")).collect();
    let during = match (scan.first(), scan.last()) {
        (Some(f), Some(l)) => w.iter().filter(|&&at| at >= from + f.1 && at <= from + l.2).count(),
        _ => 0,
    };
    let before = jumps(&mut t, "arrived", "late.rs", Duration::from_secs(5));
    esc(&mut t);
    write(&sb.file("src/late.rs"), "pub fn arrived() {}\n");
    let made = Instant::now();
    let after = jumps(&mut t, "arrived", "late.rs", Duration::from_secs(60)).map(|_| made.elapsed());
    eprintln!(
        "W1B first scan: told tarae watches {} · {} writes in all, {during} while Roots Scanned showed · loaded {} · before late.rs gd {} · after late.rs was made gd {}",
        declared(&sb),
        w.len(),
        l.quiescent.is_some(),
        if before.is_some() { "REACHED it (no proof)" } else { "doesn't reach it" },
        after.map_or("NEVER reached it within 60 s".into(), |d| format!(
            "reached it in {:.1}s",
            d.as_secs_f64()
        )),
    );
    said_watch(&sb, "first scan");
    let panicked = said_stderr(&sb, "first scan");
    quit(t);
    assert!(declared(&sb) && l.quiescent.is_some() && !panicked, "the load broke, or it wasn't told");
    assert!(before.is_none() && after.is_some(), "the played change didn't reach the server");
}

/// A run's end, a shared server with two checkouts, and a bring, on a scratch repository:
/// - **a run's write** (`:task`, a pack's task that runs `true`): `src/late.rs` written through a hard link from
///   outside the project, then the run — the server's `gd` reaches the new name. It claims nothing about the run's
///   look: FSEvents names a write made through a hard link by its in-tree path (the first card, 2026-11-11), so the
///   stream says it; the look's proof is the handed-in tests' (`lspwatch::tests`: each known moment asks for a look,
///   and `treewatch::tests`' look that says what no event named);
/// - **the shared server**: a session `s1` (a worktree; the server swapped to one for both): `src/extra.rs` made in
///   s1's checkout by another process — `gd` on `extra::brought` reaches it;
/// - **a bring**: s1's commit brought into main (`Cargo.lock` committed at the start: a bring refuses one changed
///   and not in a commit; the preflight one step that runs `true`: the packs' default would run `cargo test`) — once
///   it landed and main is in front (`tarae state`'s `front`), main's `gd` on `extra::brought` reaches it (git's
///   writes: the stream says them too, so this row shows the server hears a bring, not the look alone). Not landed,
///   or main never in front: failed before any `gd` (the second card's `gd` read s1's view).
///
/// What tarae holds with no server, one, and the shared one is printed beside. Cases `run`, `shared` (with the
/// bring).
#[test]
#[ignore = "live: run with `cargo live` (rust-analyzer, git; outside the sandbox: FSEvents)"]
fn watched_files_a_run_a_shared_server_and_a_bring() {
    if !have("cargo") || !have("rust-analyzer") {
        return;
    }
    let sb = Sandbox::live("w1b-share");
    let build = sb.outside("build");
    let sb = with_pack(
        sb,
        "[tasks]\nwhen = [\"Cargo.toml\"]\nrank = 10\n\n[[tasks.task]]\nname = \"look\"\ncmd = \"true\"\n",
    );
    let sb = sb.with_env("CARGO_BUILD_BUILD_DIR", build.to_str().unwrap());
    let wt = sb.outside("wt");
    sb.write_config(&format!(
        "{LIVE_CONFIG}auto-save = \"off\"\n[workspace]\nworktrees = \"{}\"\n[llm]\ncommand = \"sh\"\nargs = [\"-c\", \"exit 0\"]\n\
         [preflight]\nsteps = [{{ name = \"none\", cmd = \"true\" }}]\n",
        wt.display()
    ));
    let main = sb.work();
    a_crate(&main, "wshare", false);
    write(
        &main.join("src/lib.rs"),
        "mod extra;\nmod late;\n\npub fn call() {\n    late::new_name();\n    extra::brought();\n}\n",
    );
    write(&main.join("src/late.rs"), "pub fn old_name() {}\n");
    aged(&main.join("src/late.rs"), 600);
    write(&main.join(".gitignore"), "target\n");
    let st = Command::new("cargo").args(["generate-lockfile", "--offline"]).current_dir(&main).status();
    assert!(st.is_ok_and(|s| s.success()), "Cargo.lock made");
    sb.git(&["init", "-q", "-b", "main"]);
    sb.git(&["add", "-A"]);
    sb.git(&["commit", "-q", "-m", "start"]);
    let link = sb.outside("late-link.rs");
    std::fs::hard_link(main.join("src/late.rs"), &link).unwrap();

    let mut t = sb.spawn(&["src/lib.rs"]);
    let tarae = t.pid();
    std::thread::sleep(Duration::from_secs(2));
    let none = held(tarae);
    trusted(&mut t);
    let l = load(&mut t, &sb, Instant::now(), LOADED_WITHIN, false);
    l.said("share, main");
    let one = held(tarae);
    eprintln!("W1B held, one server (told tarae watches {}): {}", declared(&sb), added(&none, &one));
    let mut failed = Vec::new();

    // A run's write: through a hard link, then the run (no claim about its look: the stream names such a write)
    if picked("run") {
        let wrote = Instant::now();
        std::fs::write(&link, "pub fn new_name() {}\n").unwrap();
        t.command("task look");
        let after = jumps(&mut t, "new_name", "late.rs", Duration::from_secs(30));
        eprintln!(
            "W1B a run's write (through a hard link, then the run): gd {}",
            after.map_or("NEVER reached late.rs".into(), |_| format!(
                "reached late.rs {:.1}s after the write",
                wrote.elapsed().as_secs_f64()
            )),
        );
        if after.is_none() {
            failed.push("run");
            eprintln!("{}", t.screen());
        }
        t.command("o src/lib.rs");
    }

    if !picked("shared") {
        let panicked = said_stderr(&sb, "share");
        quit(t);
        assert!(failed.is_empty() && !panicked, "failed: {failed:?} (panicked {panicked})");
        return;
    }

    // The shared server: a second checkout (and the bring after it)
    let n = logged(&sb, "switched to the new");
    let made = new_session(&mut t, "s1");
    let end = Instant::now() + LOADED_WITHIN;
    while logged(&sb, "switched to the new") <= n && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(250));
    }
    let swapped = logged(&sb, "switched to the new") > n;
    let s1 = (std::fs::read_dir(&wt).into_iter().flatten().flatten())
        .flat_map(|e| [e.path(), e.path().join("s1")])
        .find(|p| p.join("Cargo.toml").is_file() && p.ends_with("s1"))
        .expect("s1's checkout below the worktrees folder");
    let shared = held(tarae);
    eprintln!(
        "W1B held, the shared server ({}, {:.1}s after s1 was made): {} · beside one server's: {}",
        if swapped { "swapped" } else { "NOT swapped within the limit" },
        made.elapsed().as_secs_f64(),
        added(&none, &shared),
        added(&one, &shared)
    );
    t.command(&format!("o {}", s1.join("src/lib.rs").display()));
    let before = jumps(&mut t, "brought", "extra.rs", Duration::from_secs(5));
    esc(&mut t);
    write(&s1.join("src/extra.rs"), "pub fn brought() {}\n");
    let made_at = Instant::now();
    let after = jumps(&mut t, "brought", "extra.rs", Duration::from_secs(60)).map(|_| made_at.elapsed());
    eprintln!(
        "W1B the shared server: before s1/src/extra.rs gd {} · after it was made gd {}",
        if before.is_some() { "REACHED it (no proof)" } else { "doesn't reach it" },
        after.map_or("NEVER reached it within 60 s".into(), |d| format!(
            "reached it in {:.1}s",
            d.as_secs_f64()
        ))
    );
    if before.is_some() || after.is_none() {
        failed.push("shared");
        eprintln!("{}", t.screen());
    }

    // A bring: s1's commit into main
    git_in(&sb, &s1, &["add", "-A"]);
    git_in(&sb, &s1, &["commit", "-q", "-m", "extra"]);
    std::thread::sleep(Duration::from_secs(2));
    let asked = Instant::now();
    t.command("workspace-bring");
    let card = "Bring s1 into main";
    if t.poll_for(Duration::from_secs(20), |s| s.contains(card)).is_some() {
        let end = Instant::now() + LIVE_TIMEOUT;
        while t.screen().contains(card) && Instant::now() < end {
            t.send("y");
            let _ = t.poll_for(Duration::from_millis(500), |s| !s.contains(card));
        }
    }
    // (its preflight one `true`: it lands in seconds — the shared patience, not the load's)
    let end = Instant::now() + LIVE_TIMEOUT;
    while !main.join("src/extra.rs").exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
    }
    let brought = Instant::now();
    let in_main = main.join("src/extra.rs").exists();
    // (main in front, by the state: a switch while the bring still runs doesn't take — asked again until it does)
    let main_real = main.canonicalize().unwrap();
    let end = Instant::now() + LIVE_TIMEOUT;
    let mut front = false;
    while in_main && !front && Instant::now() < end {
        t.command("workspace-switch 0");
        front = front_root(&t).is_some_and(|r| r.canonicalize().is_ok_and(|r| r == main_real));
        if !front {
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    let reached = match in_main && front {
        true => {
            t.command(&format!("o {}", main.join("src/lib.rs").display()));
            jumps(&mut t, "brought", "extra.rs", Duration::from_secs(60))
        }
        false => None,
    };
    eprintln!(
        "W1B a bring: brought {in_main} ({:.1}s after workspace-bring) · main in front {front} · main's gd on \
         extra::brought {}",
        brought.duration_since(asked).as_secs_f64(),
        match (in_main && front, reached) {
            (false, _) => "not read".to_string(),
            (true, None) => "NEVER reached extra.rs within 60 s".to_string(),
            (true, Some(_)) =>
                format!("reached extra.rs {:.1}s after the bring landed", brought.elapsed().as_secs_f64()),
        }
    );
    if !in_main || !front || reached.is_none() {
        failed.push("bring");
        eprintln!("{}", t.screen());
    }
    said_watch(&sb, "share");
    let panicked = said_stderr(&sb, "share");
    quit(t);
    assert!(failed.is_empty() && !panicked && declared(&sb), "failed: {failed:?} (panicked {panicked})");
}

/// The measure, a user-like start outside the sandbox, the build folder below each tree's `target`:
/// - Roots Scanned (and the load) for a clone of this repository and for the corpus (`target/treewatch-corpus`,
///   W1a's), as now and with W1b — with what tarae holds before the server and after its load;
/// - with W1b on the clone, loaded: tarae's CPU, the tree watch worker's, the server's and fseventsd's through one
///   full `cargo build` (the gate: 2 % of a core);
/// - the main loop while a 5,000-file change lands (a branch switch on a scratch repository and back, five times
///   each way, as now and with W1b): the round trip of `tarae state` every 20 ms beside its idle one, the longest
///   per arm beside the freeze watcher's threshold (`stall::SLOW`, 250 ms), and its lines.
///
/// Cases: `clone`, `corpus`, `build`, `switch`.
#[test]
#[ignore = "live: run with `cargo live` (rust-analyzer, cargo build of this repository; outside the sandbox: FSEvents)"]
fn watched_files_the_measure() {
    if !have("cargo") || !have("rust-analyzer") {
        return;
    }
    let dir = Dir::new("w1b-measure");
    let base = dir.path().canonicalize().unwrap();
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let clone = base.join("tarae");
    let mut trees: Vec<(&str, PathBuf)> = Vec::new();
    if picked("clone") || picked("build") {
        let st = Command::new("git")
            .args(["clone", "-q", "--depth", "1"])
            .arg(format!("file://{}", here.display()))
            .arg(&clone)
            .status()
            .unwrap();
        assert!(st.success(), "clone");
        trees.push(("clone", clone.clone()));
    }
    let corpus = here.join("target/treewatch-corpus");
    if picked("corpus") && corpus.is_dir() {
        let st = Command::new("cp").arg("-R").arg(&corpus).arg(base.join("corpus")).status().unwrap();
        assert!(st.success(), "copy of the corpus");
        trees.push(("corpus", base.join("corpus")));
    } else if picked("corpus") {
        eprintln!("W1B corpus: {} isn't there — not measured", corpus.display());
    }
    eprintln!("W1B fseventsd at the start: {}", fseventsd());
    for (name, tree) in &trees {
        for arm in ["as now", "W1b"] {
            let build = arm == "W1b" && *name == "clone" && picked("build");
            if !build && !picked(name) {
                continue;
            }
            let sb = Sandbox::live(&format!("w1b-{name}-{}", arm.replace(' ', "-")));
            let sb = sb.with_env("CARGO_BUILD_BUILD_DIR", tree.join("target").to_str().unwrap());
            let sb = if arm == "as now" { with_pack(sb, AS_NOW) } else { sb };
            sb.write_config(&format!("{LIVE_CONFIG}[llm]\ncommand = \"true\"\n"));
            let mut t = Term::spawn_in(&sb, tree, &["src/main.rs"], 160, 40);
            let tarae = t.pid();
            std::thread::sleep(Duration::from_secs(2));
            let before = held(tarae);
            trusted(&mut t);
            let from = Instant::now();
            let l = load(&mut t, &sb, from, LOADED_WITHIN, !build);
            let what = format!("{name}, {arm}");
            l.said(&what);
            let after = held(tarae);
            eprintln!("W1B held, {what}: told tarae watches {} · {}", declared(&sb), added(&before, &after));
            said_watch(&sb, &what);
            if build && l.quiescent.is_some() {
                let ra = server_of(tarae);
                let (h0, ra0, fs0) = (held(tarae), ra.map(held).unwrap_or_default(), fseventsd());
                let took = cargo_build(tree, &tree.join("target"));
                std::thread::sleep(Duration::from_secs(2));
                let (h1, ra1, fs1) = (held(tarae), ra.map(held).unwrap_or_default(), fseventsd());
                let wall = took + Duration::from_secs(2);
                let (cpu, work, server) = (
                    h1.cpu.saturating_sub(h0.cpu),
                    worker(&h1).saturating_sub(worker(&h0)),
                    ra1.cpu.saturating_sub(ra0.cpu),
                );
                eprintln!(
                    "W1B a full cargo build of the clone, {:.1}s: tarae {cpu:?} ({:.2} % of a core), the worker {work:?} ({:.2} %), rust-analyzer {server:?} ({:.2} %) · fseventsd (cpu time, rss KB) {fs0} → {fs1} · tarae's threads by CPU over the build: {}",
                    took.as_secs_f64(),
                    share(cpu, wall),
                    share(work, wall),
                    share(server, wall),
                    busiest(&h0, &h1),
                );
                let s = share(cpu, wall);
                eprintln!(
                    "GATE {} a full cargo build, tarae: {s:.2} % of a core",
                    if s < 2.0 { "ok  " } else { "FAIL" }
                );
                said_watch(&sb, &format!("{what}, after the build"));
            }
            said_stderr(&sb, &what);
            quit(t);
        }
    }
    if picked("switch") {
        for arm in ["as now", "W1b"] {
            switch_measure(&base, arm);
        }
    }
    if picked("counts") {
        switch_counts(&base);
    }
    // (by name only: `sample` stops tarae each millisecond)
    if cases().is_some() && picked("profile") {
        for arm in ["as now", "W1b"] {
            switch_profile(&base, arm);
        }
    }
    eprintln!("W1B fseventsd at the end: {}", fseventsd());
}

/// tarae's threads that spent the most CPU between `a` and `b`, by name.
fn busiest(a: &Held, b: &Held) -> String {
    let mut by: std::collections::BTreeMap<String, Duration> = std::collections::BTreeMap::new();
    for (n, c) in &b.threads {
        *by.entry(n.clone()).or_default() += *c;
    }
    for (n, c) in &a.threads {
        let e = by.entry(n.clone()).or_default();
        *e = e.saturating_sub(*c);
    }
    let mut v: Vec<(String, Duration)> = by.into_iter().filter(|(_, c)| !c.is_zero()).collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    v.iter().take(6).map(|(n, c)| format!("{n} {c:?}")).collect::<Vec<_>>().join(", ")
}

/// The freeze watcher's threshold (`stall::SLOW`): a stand of the main loop this long is logged.
const STALL_SLOW: Duration = Duration::from_millis(250);

/// A scratch repository whose branch `many` has 5,000 more `.rs` files — `many/d0/f0.rs` … `many/d49/f4999.rs`, 100
/// a folder, below the crate's root (the folder tarae and the server serve; `.gitignore` leaves out only `target`),
/// kept by rust-analyzer's `**/*.rs` —, tarae loaded on `main` (`arm`: "as now", the server watching by itself, or
/// "W1b") and quiet. `src/lib.rs`, the same on both branches, names one of them as a module (`gone`, by `#[path]`:
/// resolved only while it's there) and `src/kept.rs` (`kept`): `gd` on `gone::f` lands only on `many`, on
/// `kept::stay` on both. `name`: the sandbox's and the build folder's; `ra_log`: rust-analyzer's own log goes there,
/// each event it handles (`RA_LOG_FILE`, `RA_LOG=rust_analyzer::main_loop=debug`).
fn switch_setup(base: &Path, arm: &str, name: &str, ra_log: Option<&Path>) -> (Sandbox, Term) {
    let sb = Sandbox::live(&format!("w1b-{name}-{}", arm.replace(' ', "-")));
    let build = base.join(format!("{name}-build-{}", arm.replace(' ', "-")));
    let sb = sb.with_env("CARGO_BUILD_BUILD_DIR", build.to_str().unwrap());
    let sb = if arm == "as now" { with_pack(sb, AS_NOW) } else { sb };
    let sb = match ra_log {
        Some(f) => sb
            .with_env("RA_LOG_FILE", f.to_str().unwrap())
            .with_env("RA_LOG", "rust_analyzer::main_loop=debug"),
        None => sb,
    };
    sb.write_config(&format!("{LIVE_CONFIG}[llm]\ncommand = \"true\"\n"));
    let work = sb.work();
    a_crate(&work, "wswitch", false);
    write(
        &work.join("src/lib.rs"),
        "#[path = \"../many/d0/f0.rs\"]\npub mod gone;\npub mod kept;\n\npub fn call() {\n    gone::f();\n    kept::stay();\n}\n",
    );
    write(&work.join("src/kept.rs"), "pub fn stay() {}\n");
    write(&work.join(".gitignore"), "target\n");
    sb.git(&["init", "-q", "-b", "main"]);
    sb.git(&["add", "-A"]);
    sb.git(&["commit", "-q", "-m", "start"]);
    sb.git(&["checkout", "-q", "-b", "many"]);
    for i in 0..5_000 {
        write(&work.join(format!("many/d{}/f{i}.rs", i / 100)), "pub fn f() {}\n");
    }
    sb.git(&["add", "-A"]);
    sb.git(&["commit", "-q", "-m", "many"]);
    sb.git(&["checkout", "-q", "main"]);
    let mut t = sb.spawn(&["src/lib.rs"]);
    trusted(&mut t);
    let what = format!("{name}, {arm}");
    let l = load(&mut t, &sb, Instant::now(), LOADED_WITHIN, false);
    l.said(&what);
    eprintln!("W1B {what}: told tarae watches {}", declared(&sb));
    quiet(&mut t, 5);
    (sb, t)
}

/// `tarae state`'s round trips for `over`: each one's length and its start after `from`, sorted by length.
fn trips(t: &mut Term, from: Instant, over: Duration) -> Vec<(Duration, Duration)> {
    let (end, mut v) = (Instant::now() + over, Vec::new());
    while Instant::now() < end {
        let t0 = Instant::now();
        let _ = t.state(&[]);
        v.push((t0.elapsed(), t0 - from));
        std::thread::sleep(Duration::from_millis(20));
    }
    v.sort();
    v
}

/// The main loop while 5,000 files land: tarae on `main` (`switch_setup`), then `git checkout many` and back outside
/// it, five times each way: each switch's round trips, and the arm's longest beside the threshold.
fn switch_measure(base: &Path, arm: &str) {
    let (sb, mut t) = switch_setup(base, arm, "switch", None);
    let what = format!("switch, {arm}");
    let at = |v: &[(Duration, Duration)], p: usize| v[(v.len() * p / 100).min(v.len() - 1)].0;
    // (the round trip's own cost: tarae idle)
    let idle = trips(&mut t, Instant::now(), Duration::from_secs(5));
    let stalls0 = logged(&sb, "the main loop has stood");
    let mut longest: Option<(Duration, Duration, i32, &str)> = None;
    let (mut over50, mut over100, mut all) = (0, 0, 0);
    for round in 1..=5 {
        for to in ["many", "main"] {
            let stalls = logged(&sb, "the main loop has stood");
            let t0 = Instant::now();
            sb.git(&["checkout", "-q", to]);
            let git = t0.elapsed();
            let busy = trips(&mut t, t0, Duration::from_secs(10));
            let (max, when) = busy[busy.len() - 1];
            over50 += busy.iter().filter(|b| b.0 > Duration::from_millis(50)).count();
            over100 += busy.iter().filter(|b| b.0 > Duration::from_millis(100)).count();
            all += busy.len();
            if longest.is_none_or(|(m, ..)| max > m) {
                longest = Some((max, when, round, to));
            }
            eprintln!(
                "W1B {what}, round {round}, git checkout {to} ({:.0} ms): round trip over 10 s p50 {:?} · p99 {:?} · max {max:?} (begun {:.0} ms after the checkout began) ({} trips) · the freeze watcher's lines {}",
                git.as_secs_f64() * 1000.0,
                at(&busy, 50),
                at(&busy, 99),
                when.as_secs_f64() * 1000.0,
                busy.len(),
                logged(&sb, "the main loop has stood") - stalls,
            );
        }
    }
    let (max, when, round, to) = longest.expect("ten switches");
    eprintln!(
        "W1B {what}: the longest round trip over ten switches {max:?} (round {round}, to {to}, begun {:.0} ms after \
         the checkout) · the freeze watcher's threshold {STALL_SLOW:?}, its lines {} · trips over 50 ms {over50}, \
         over 100 ms {over100}, of {all} · idle p50 {:?} max {:?}",
        when.as_secs_f64() * 1000.0,
        logged(&sb, "the main loop has stood") - stalls0,
        at(&idle, 50),
        idle[idle.len() - 1].0,
    );
    said_watch(&sb, &what);
    said_stderr(&sb, &what);
    quit(t);
}

/// How long each sample of the switch back runs, from just before `git checkout main` (git's ~0.5 s, the long trips
/// by ~0.85 s).
const SAMPLE_SECS: u32 = 3;

/// A frame of the main thread's call tree with fewer samples than this isn't printed (the file has them all).
const SAMPLE_MIN: u64 = 5;

/// The main loop through the switch back to `main`, sampled: tarae on `main` (`switch_setup`), then three rounds of
/// `git checkout many` (unsampled, 10 s to settle) and `git checkout main` with `/usr/bin/sample` on the test's own
/// tarae (checked to be this test's child) for `SAMPLE_SECS`, begun once it has attached. Each sample's file goes
/// to `TARAE_W1B_SAMPLES` (else the scratch folder, gone after); the main thread's call tree is printed, its frames
/// with at least `SAMPLE_MIN` samples, and the switch's longest round trip beside it (sample stops the process each
/// millisecond: a profile's trips aren't a measure).
fn switch_profile(base: &Path, arm: &str) {
    let (sb, mut t) = switch_setup(base, arm, "profile", None);
    let what = format!("profile, {arm}");
    let pid = t.pid();
    let parent = Command::new("ps").args(["-o", "ppid=", "-p", &pid.to_string()]).output().unwrap();
    let parent = String::from_utf8_lossy(&parent.stdout).trim().to_string();
    assert_eq!(parent, std::process::id().to_string(), "pid {pid} isn't this test's own tarae: not sampled");
    let bin = std::env::var("TARAE_BIN").unwrap_or_else(|_| BIN.to_string());
    eprintln!("W1B {what}: sampling pid {pid}, this test's child, the build {bin}");
    let out = std::env::var_os("TARAE_W1B_SAMPLES").map(PathBuf::from).unwrap_or_else(|| base.to_path_buf());
    for round in 1..=3 {
        sb.git(&["checkout", "-q", "many"]);
        trips(&mut t, Instant::now(), Duration::from_secs(10));
        let file = out.join(format!("sample-{}-{round}.txt", arm.replace(' ', "-")));
        let mut sample = Command::new("/usr/bin/sample")
            .args([&pid.to_string(), &SAMPLE_SECS.to_string(), "1", "-mayDie", "-file"])
            .arg(&file)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        // (its first line, "Sampling process … for N seconds", once it has attached)
        let mut first = String::new();
        let mut lines = std::io::BufReader::new(sample.stdout.take().unwrap());
        std::io::BufRead::read_line(&mut lines, &mut first).unwrap();
        let t0 = Instant::now();
        sb.git(&["checkout", "-q", "main"]);
        let git = t0.elapsed();
        let busy = trips(&mut t, t0, Duration::from_secs(SAMPLE_SECS as u64 + 1));
        let st = sample.wait_with_output().unwrap();
        let (max, when) = busy[busy.len() - 1];
        eprintln!(
            "W1B {what}, round {round}, git checkout main ({:.0} ms): sampled {}s ({}), the longest trip {max:?} begun \
             {:.0} ms after the checkout began · {} · {}",
            git.as_secs_f64() * 1000.0,
            SAMPLE_SECS,
            first.trim(),
            when.as_secs_f64() * 1000.0,
            file.display(),
            if st.status.success() { "sample ok".to_string() } else { format!("sample {}", st.status) },
        );
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let tree = main_thread(&text);
        eprintln!("W1B {what}, round {round}: the main thread's call tree, frames of {SAMPLE_MIN}+ samples:");
        for l in tree.iter().filter(|l| samples_of(l) >= SAMPLE_MIN).take(400) {
            eprintln!("    {}", l.chars().take(240).collect::<String>());
        }
    }
    said_stderr(&sb, &what);
    quit(t);
}

/// The files the switch between `main` and `many` makes or removes (`switch_setup`).
const SWITCHED: usize = 5_000;

/// tarae.log's tree batch lines ("a batch said …", one every batch, changes only too), and the mark's ("the mark took
/// …": a make's late events, said nothing, their flags).
fn tree_batches(sb: &Sandbox) -> Vec<String> {
    let text = std::fs::read_to_string(sb.state().join("tarae.log")).unwrap_or_default();
    (text.lines()).filter(|l| l.contains("a batch said ") || l.contains(MARK)).map(str::to_string).collect()
}

const MARK: &str = "the mark took ";

/// How many events a mark line says the mark took (0 for a batch line).
fn mark_count(line: &str) -> usize {
    (line.split_once(MARK)).and_then(|(_, r)| r.split_whitespace().next()?.parse().ok()).unwrap_or(0)
}

/// A batch line's (created, deleted, changed).
fn batch_counts(line: &str) -> (usize, usize, usize) {
    let num = |word: &str| -> usize {
        let at = line.find(word).unwrap_or(0);
        line[..at].split_whitespace().last().and_then(|n| n.parse().ok()).unwrap_or(0)
    };
    (num(" created,"), num(" deleted,"), num(" changed,"))
}

/// Now as seconds into the UTC day.
fn utc_now() -> f64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    since.as_secs_f64().rem_euclid(86_400.0)
}

/// `b - a`, two times of the UTC day, within half a day either way (a run across midnight).
fn day_diff(a: f64, b: f64) -> f64 {
    (b - a + 43_200.0).rem_euclid(86_400.0) - 43_200.0
}

/// A `didChangeWatchedFiles` rust-analyzer handled: when (seconds into the UTC day, from its log line's
/// `2026-11-11T10:49:12.874901+09:00`) and each entry's uri and type (1 created, 2 changed, 3 deleted).
struct Handled {
    at: f64,
    entries: Vec<(String, u32)>,
}

/// rust-analyzer's own log (`RA_LOG=rust_analyzer::main_loop=debug`): each `didChangeWatchedFiles` its main loop
/// took (`handle_event event=Notification`, the whole message on one line).
fn ra_handled(log: &Path) -> Vec<Handled> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let mut out = Vec::new();
    for l in text.lines() {
        if !l.contains("handle_event event=Notification")
            || !l.contains("\"workspace/didChangeWatchedFiles\"")
        {
            continue;
        }
        let Some(at) = l.split_whitespace().next().and_then(utc_of) else { continue };
        let list = l.split_once("Array [").map_or("", |(_, rest)| rest);
        let entries = (list.split("Object {").skip(1))
            .filter_map(|e| {
                let uri = e.split_once("String(\"")?.1.split_once("\")")?.0.to_string();
                let ty = e.split_once("Number(")?.1.split_once(')')?.0.parse().ok()?;
                Some((uri, ty))
            })
            .collect();
        out.push(Handled { at, entries });
    }
    out
}

/// `2026-11-11T10:49:12.874901+09:00` as seconds into the UTC day.
fn utc_of(stamp: &str) -> Option<f64> {
    let time = stamp.split_once('T')?.1;
    let at = time.rfind(['+', '-'])?;
    let (clock, offset) = (&time[..at], &time[at..]);
    let mut hms = clock.split(':');
    let (h, m, s): (f64, f64, f64) =
        (hms.next()?.parse().ok()?, hms.next()?.parse().ok()?, hms.next()?.parse().ok()?);
    let sign = if offset.starts_with('-') { -1.0 } else { 1.0 };
    let (oh, om) = offset[1..].split_once(':')?;
    let off: f64 = oh.parse::<f64>().ok()? * 3600.0 + om.parse::<f64>().ok()? * 60.0;
    Some((h * 3600.0 + m * 60.0 + s - sign * off).rem_euclid(86_400.0))
}

/// `gd` as `jumps`, then back to `src/lib.rs` alone — the file it reached closed, so the next one's first row
/// can't name it.
fn jumps_back(t: &mut Term, word: &str, file: &str, within: Duration) -> Option<Duration> {
    let r = jumps(t, word, file, within);
    esc(t);
    t.command("bc!");
    t.command("o src/lib.rs");
    r
}

/// One switch of `switch_counts`: where to, when it began and when git ended (seconds into the UTC day), and the
/// tree's (created, deleted, changed) over its batches.
struct Switched {
    name: String,
    to: &'static str,
    began: f64,
    ended: f64,
    tree: (usize, usize, usize),
}

/// W1b step 3, round 5: does a big burst of kept files arrive whole? tarae on `main` (`switch_setup`, rust-analyzer's
/// log kept), then `git checkout many` and back outside it, three times each way. Each switch: every tree batch as
/// tarae.log says it, when the test saw it after git ended (polled each 20 ms, until the switch's whole count, then 3
/// s more for a late or doubled one; 30 s at most) — and, read from rust-analyzer's log once tarae has quit, each
/// `didChangeWatchedFiles` it took from the switch's start until the next one's, when after git ended, its entries
/// below `many/` by type and how many distinct. In round 1 `gd`: `kept::stay` lands both ways, `gone::f` on `many`
/// and nowhere back on `main`. Fails, once all is printed, unless every count is exact: 5,000 created on the way
/// there, 5,000 deleted on the way back, nothing else of `many/`, none twice. rust-analyzer's log is kept in
/// `TARAE_W1B_KEEP` (else the scratch folder, gone after).
fn switch_counts(base: &Path) {
    let keep = std::env::var_os("TARAE_W1B_KEEP").map(PathBuf::from).unwrap_or_else(|| base.to_path_buf());
    let ra_log = keep.join("counts-ra.log");
    let (sb, mut t) = switch_setup(base, "W1b", "counts", Some(&ra_log));
    let what = "counts, W1b";
    let mut failed: Vec<String> = Vec::new();
    let mut switches: Vec<Switched> = Vec::new();
    for round in 1..=3 {
        for to in ["many", "main"] {
            let name = format!("round {round}, git checkout {to}");
            let before = tree_batches(&sb).len();
            let (want_made, want_gone) = if to == "many" { (SWITCHED, 0) } else { (0, SWITCHED) };
            let began = utc_now();
            let t0 = Instant::now();
            // (polled from git's start, beside it: a batch that lands while git still runs is timed too — `seen`'s
            // times from `t0`)
            let (git, seen) = std::thread::scope(|s| {
                let g = s.spawn(|| {
                    sb.git(&["checkout", "-q", to]);
                    t0.elapsed()
                });
                let (mut seen, mut whole): (Vec<(Duration, String)>, Option<Instant>) = (Vec::new(), None);
                loop {
                    let lines = tree_batches(&sb);
                    for l in lines.iter().skip(before + seen.len()) {
                        seen.push((t0.elapsed(), l.clone()));
                    }
                    let (m, g2) = (seen.iter().map(|(_, l)| batch_counts(l)))
                        .fold((0, 0), |(m, g), c| (m + c.0, g + c.1));
                    if whole.is_none() && g.is_finished() && m >= want_made && g2 >= want_gone {
                        whole = Some(Instant::now());
                    }
                    let done = whole.is_some_and(|w| w.elapsed() > Duration::from_secs(3));
                    if done || t0.elapsed() > Duration::from_secs(30) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                (g.join().expect("git"), seen)
            });
            let gone_at = began + git.as_secs_f64();
            let after_git = |at: Duration| (at.as_secs_f64() - git.as_secs_f64()) * 1000.0;
            for (i, (at, l)) in seen.iter().enumerate() {
                let tail = (l.split_once("a batch said ").map(|(_, r)| r))
                    .or_else(|| l.find(MARK).map(|at| &l[at..]))
                    .unwrap_or(l.as_str());
                eprintln!(
                    "W1B {what}, {name}: tree batch {} seen {:.0} ms after git ended (below 0: while git ran): {tail}",
                    i + 1,
                    after_git(*at)
                );
            }
            let tree = (seen.iter().map(|(_, l)| batch_counts(l)))
                .fold((0, 0, 0), |(m, g, c), b| (m + b.0, g + b.1, c + b.2));
            let marked: usize = seen.iter().map(|(_, l)| mark_count(l)).sum();
            eprintln!(
                "W1B {what}, {name} ({:.0} ms): the tree said {} created, {} deleted, {} changed in {} batches, the \
                 mark took {marked} late events, the last line seen {} after git ended (want {want_made} created, \
                 {want_gone} deleted)",
                git.as_secs_f64() * 1000.0,
                tree.0,
                tree.1,
                tree.2,
                seen.iter().filter(|(_, l)| mark_count(l) == 0).count(),
                seen.last().map_or("—".to_string(), |(at, _)| format!("{:.0} ms", after_git(*at))),
            );
            if (tree.0, tree.1) != (want_made, want_gone) {
                failed.push(format!("{name}: the tree said {} created, {} deleted", tree.0, tree.1));
            }
            if round == 1 {
                let kept = jumps_back(&mut t, "kept::stay", "kept.rs", Duration::from_secs(60));
                let within = if to == "many" { 60 } else { 10 };
                let gone = jumps_back(&mut t, "gone::f", "f0.rs", Duration::from_secs(within));
                eprintln!(
                    "W1B {what}, {name}: gd kept::stay → kept.rs {kept:?} · gd gone::f → f0.rs {gone:?} (within {within} s; \
                     want {})",
                    if to == "many" { "both land" } else { "kept lands, gone nowhere" }
                );
                if kept.is_none() {
                    failed.push(format!("{name}: gd on a kept file's symbol didn't land"));
                }
                if gone.is_some() != (to == "many") {
                    failed.push(format!("{name}: gd on gone::f {gone:?}"));
                }
            }
            switches.push(Switched { name, to, began, ended: gone_at, tree });
        }
    }
    said_watch(&sb, what);
    said_stderr(&sb, what);
    quit(t);
    // (rust-analyzer has exited with tarae: its log is whole)
    let handled = ra_handled(&ra_log);
    eprintln!(
        "W1B {what}: rust-analyzer's log {} ({} bytes): {} didChangeWatchedFiles in all",
        ra_log.display(),
        std::fs::metadata(&ra_log).map_or(0, |m| m.len()),
        handled.len()
    );
    for (i, s) in switches.iter().enumerate() {
        let next = switches.get(i + 1).map(|n| n.began);
        let mine: Vec<&Handled> = (handled.iter())
            .filter(|h| day_diff(s.began, h.at) >= 0.0 && next.is_none_or(|n| day_diff(n, h.at) < 0.0))
            .collect();
        let (mut by_type, mut other, mut uris) = ([0usize; 4], 0, std::collections::BTreeSet::new());
        for h in &mine {
            eprintln!(
                "W1B {what}, {}: rust-analyzer took a didChangeWatchedFiles of {} entries {:.0} ms after git ended",
                s.name,
                h.entries.len(),
                day_diff(s.ended, h.at) * 1000.0
            );
            for (uri, ty) in &h.entries {
                if uri.contains("/many/") {
                    by_type[(*ty as usize).min(3)] += 1;
                    uris.insert((uri.clone(), *ty));
                } else {
                    other += 1;
                }
            }
        }
        let many = by_type.iter().sum::<usize>();
        eprintln!(
            "W1B {what}, {} (tree: {} created, {} deleted, {} changed): rust-analyzer took {} entries below many/ in {} \
             notifications — {} created, {} changed, {} deleted, {} distinct — and {other} others",
            s.name,
            s.tree.0,
            s.tree.1,
            s.tree.2,
            many,
            mine.len(),
            by_type[1],
            by_type[2],
            by_type[3],
            uris.len(),
        );
        let want = if s.to == "many" { [0, SWITCHED, 0, 0] } else { [0, 0, 0, SWITCHED] };
        if by_type != want || uris.len() != many {
            failed.push(format!(
                "{}: rust-analyzer took {} created, {} changed, {} deleted below many/ ({} distinct of {many})",
                s.name,
                by_type[1],
                by_type[2],
                by_type[3],
                uris.len()
            ));
        }
    }
    assert!(failed.is_empty(), "W1B {what}: not whole — {failed:#?}");
}

/// A sample file's main thread: its header and call tree ("Call graph:" lists each thread, the main one first and
/// named `com.apple.main-thread`).
fn main_thread(text: &str) -> Vec<String> {
    let graph = text.lines().skip_while(|l| !l.starts_with("Call graph:")).skip(1);
    let header = |l: &str| {
        l.starts_with("    ") && l.split_whitespace().nth(1).is_some_and(|w| w.starts_with("Thread_"))
    };
    let mut out = Vec::new();
    for l in graph {
        if l.trim().is_empty() || (header(l) && !out.is_empty()) {
            break;
        }
        out.push(l.to_string());
    }
    out
}

/// A call tree line's sample count: its first number past the tree's marks (`+ ! : |` and spaces).
fn samples_of(line: &str) -> u64 {
    let rest = line.trim_start_matches([' ', '+', '!', ':', '|']);
    rest.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0)
}
