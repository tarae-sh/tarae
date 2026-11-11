//! The report's measure, by hand, on this Mac as it is (FSEvents: outside a sandbox):
//! `cargo test --bin tarae treewatch::tests::measure -- --ignored --exact --nocapture --test-threads=1`.

use super::*;
use std::process::Command;
use std::sync::mpsc::Receiver;

/// rust-analyzer's globs as `keep` would have them: `*.rs`, `Cargo.toml`, `Cargo.lock`, `rust-analyzer.toml`, `*.md`.
fn ra_keep() -> Keep {
    Arc::new(|p: &Path| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        name.ends_with(".rs")
            || name.ends_with(".md")
            || matches!(name, "Cargo.toml" | "Cargo.lock" | "rust-analyzer.toml")
    })
}

/// `PROC_PIDLISTTHREADS` (not in libc).
const PROC_PIDLISTTHREADS: i32 = 6;

/// This process's threads: each one's handle, name and CPU so far.
fn threads() -> Vec<(u64, String, Duration)> {
    let pid = std::process::id() as i32;
    let mut ids = vec![0u64; 1024];
    // SAFETY: fills at most the buffer's size in handles.
    let n = unsafe {
        libc::proc_pidinfo(pid, PROC_PIDLISTTHREADS, 0, ids.as_mut_ptr().cast(), (ids.len() * 8) as i32)
    };
    ids.truncate(n.max(0) as usize / 8);
    let size = std::mem::size_of::<libc::proc_threadinfo>() as i32;
    (ids.into_iter())
        .filter_map(|id| {
            // SAFETY: a plain value, filled by the call.
            let mut t: libc::proc_threadinfo = unsafe { std::mem::zeroed() };
            // SAFETY: fills only `t`, of the size given.
            let r =
                unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTHREADINFO, id, (&raw mut t).cast(), size) };
            let name: Vec<u8> = t.pth_name.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
            let cpu = Duration::from_nanos(t.pth_user_time + t.pth_system_time);
            (r == size).then(|| (id, String::from_utf8_lossy(&name).into_owned(), cpu))
        })
        .collect()
}

/// The threads in `now` that weren't in `before`, by name (`?` unnamed).
fn new_threads(before: &[(u64, String, Duration)], now: &[(u64, String, Duration)]) -> Vec<String> {
    let new = now.iter().filter(|t| !before.iter().any(|b| b.0 == t.0));
    new.map(|t| if t.1.is_empty() { "?".into() } else { t.1.clone() }).collect()
}

/// This process's descriptors, each with what it is (its path, or its type).
fn descriptors() -> Vec<(i32, String)> {
    let fds: Vec<i32> = fs::read_dir("/dev/fd")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect();
    (fds.into_iter())
        .filter_map(|fd| {
            // SAFETY: a plain value, filled by the call.
            let mut st: libc::stat = unsafe { std::mem::zeroed() };
            // SAFETY: fstat of a descriptor number (the folder's own, gone by now, fails and is skipped).
            if unsafe { libc::fstat(fd, &mut st) } != 0 {
                return None;
            }
            let mut path = [0u8; libc::PATH_MAX as usize];
            // SAFETY: F_GETPATH writes at most PATH_MAX bytes.
            let named = unsafe { libc::fcntl(fd, libc::F_GETPATH, path.as_mut_ptr()) } == 0;
            let what = match st.st_mode & libc::S_IFMT {
                _ if named => {
                    String::from_utf8_lossy(&path[..path.iter().position(|&b| b == 0).unwrap_or(0)])
                        .into_owned()
                }
                libc::S_IFIFO => "pipe".into(),
                libc::S_IFSOCK => "socket".into(),
                libc::S_IFCHR => "character device".into(),
                m => format!("mode {m:o}"),
            };
            Some((fd, what))
        })
        .collect()
}

fn new_descriptors(before: &[(i32, String)], now: &[(i32, String)]) -> Vec<String> {
    now.iter().filter(|d| !before.contains(d)).map(|(fd, what)| format!("{fd}: {what}")).collect()
}

fn cpu() -> Duration {
    // SAFETY: a plain value, filled by the call.
    let mut r: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: fills only `r`.
    assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut r) }, 0);
    let tv = |t: libc::timeval| Duration::new(t.tv_sec as u64, t.tv_usec as u32 * 1000);
    tv(r.ru_utime) + tv(r.ru_stime)
}

fn at(all: &[Duration], percent: usize) -> Duration {
    all[(all.len() * percent / 100).min(all.len() - 1)]
}

fn watch(roots: &[PathBuf], leave_out: Vec<PathBuf>, keep: Keep) -> (TreeWatch, Receiver<Batch>) {
    let (tx, rx) = mpsc::channel();
    let given = Given { roots: roots.to_vec(), leave_out, keep };
    (TreeWatch::start(given, move |s: Said| drop(tx.send(s.batch))).unwrap(), rx)
}

fn heard(rx: &Receiver<Batch>, path: &Path, within: Duration) {
    let end = Instant::now() + within;
    loop {
        let b = rx.recv_timeout(end.saturating_duration_since(Instant::now())).expect("a batch in time");
        if b.iter().any(|(p, _)| p == path) {
            return;
        }
    }
}

fn worker_cpu(threads: &[(u64, String, Duration)]) -> Duration {
    threads.iter().filter(|t| t.1 == "tarae-treewatch").map(|t| t.2).sum()
}

fn said_delays(what: &str, delays: &mut [Duration]) {
    delays.sort();
    let n = delays.len();
    eprintln!(
        "TREEWATCH delay, {what}, over {n} writes: p50 {:?} · p90 {:?} · min {:?} · max {:?}",
        at(delays, 50),
        at(delays, 90),
        delays[0],
        delays[n - 1]
    );
}

#[test]
#[ignore = "by hand: the tree watch's measure for its report (FSEvents: outside a sandbox)"]
fn the_reports_measure() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().unwrap();
    let trees: Vec<PathBuf> = std::env::var("TARAE_TREEWATCH_TREES")
        .map_or_else(|_| vec![here.clone()], |v| v.split(':').map(PathBuf::from).collect());

    // The start: the stream made and started, then the table's walk, three times each
    for root in &trees {
        let leave_out: Vec<PathBuf> = std::iter::once(".git".to_string())
            .chain(crate::sandbox::outputs().iter().cloned())
            .map(|o| root.join(o))
            .filter(|p| p.exists())
            .collect();
        for _ in 0..3 {
            let t0 = Instant::now();
            let (groups, _) = groups(std::slice::from_ref(root), &leave_out);
            let (stream, _rx) = mac::stream_for_test(std::slice::from_ref(root), &groups[0].exclude).unwrap();
            let streamed = t0.elapsed();
            let given = Given { roots: vec![root.clone()], leave_out: leave_out.clone(), keep: ra_keep() };
            let mut t = Tree::new(given).unwrap();
            let t1 = Instant::now();
            let links = t.fill(std::slice::from_ref(root));
            let walked = t1.elapsed();
            let bytes: usize = t.table.keys().map(|p| p.capacity()).sum::<usize>()
                + t.table.len() * (std::mem::size_of::<PathBuf>() + std::mem::size_of::<Held>());
            eprintln!(
                "TREEWATCH start {}: stream {streamed:?} · walk {walked:?} · {} entries · {bytes} bytes (keys and values) · {links} links to folders · left out {:?}",
                root.display(),
                t.table.len(),
                leave_out.iter().map(|l| l.strip_prefix(root).unwrap()).collect::<Vec<_>>(),
            );
            drop(stream);
        }
    }

    // Threads and descriptors a watch adds; the write-to-batch delay over 100 single writes (each its own file:
    // a late event can't stand for the next write's), against FSEvents' own delivery to a bare stream
    let (_s, root) = scratch("measure-delay");
    let mut raw_delays = Vec::new();
    {
        let (_stream, raw) = mac::stream_for_test(std::slice::from_ref(&root), &[]).unwrap();
        for i in 0..100 {
            let (f, t0) = (root.join(format!("raw{i}.rs")), Instant::now());
            fs::write(&f, "").unwrap();
            loop {
                match raw.recv_timeout(crate::test_results::PATIENCE).expect("a raw event in time") {
                    Msg::Events(e) if e.iter().any(|(p, _)| *p == f) => break,
                    _ => {}
                }
            }
            raw_delays.push(t0.elapsed());
        }
    }
    said_delays("FSEvents to a bare stream's callback", &mut raw_delays);
    let (threads0, fds0) = (threads(), descriptors());
    let (w, rx) = watch(std::slice::from_ref(&root), vec![], ra_keep());
    let marker = root.join("armed.rs");
    write(&marker, "");
    heard(&rx, &marker, crate::test_results::PATIENCE);
    let (threads1, fds1) = (threads(), descriptors());
    eprintln!(
        "TREEWATCH running: new threads {:?} · new descriptors {:?}",
        new_threads(&threads0, &threads1),
        new_descriptors(&fds0, &fds1)
    );
    let mut delays = Vec::new();
    for i in 0..100 {
        let (f, t0) = (root.join(format!("w{i}.rs")), Instant::now());
        fs::write(&f, "").unwrap();
        heard(&rx, &f, crate::test_results::PATIENCE);
        delays.push(t0.elapsed());
    }
    said_delays("write to batch", &mut delays);
    drop(w);
    drop(rx);
    crate::event::sleep(Duration::from_secs(1));
    let (threads2, fds2) = (threads(), descriptors());
    eprintln!(
        "TREEWATCH stopped: threads left of the watch's {:?} · descriptors left {:?}",
        new_threads(&threads0, &threads2),
        new_descriptors(&fds0, &fds2)
    );

    // Four streams — three roots (five folders left out each) and an out dir below the first one's `target` —: the
    // threads and descriptors they add, the total said beside the start log line's count
    let (_s, base) = scratch("measure-four");
    let roots: Vec<PathBuf> = (0..3).map(|i| base.join(format!("r{i}"))).collect();
    let names = [".git", "target", "l2", "l3", "l4"];
    let leave_out: Vec<PathBuf> = roots.iter().flat_map(|r| names.map(|n| r.join(n))).collect();
    leave_out.iter().for_each(|l| fs::create_dir_all(l).unwrap());
    let mut four = roots.clone();
    four.push(roots[0].join("target/debug/build/x-1/out"));
    fs::create_dir_all(&four[3]).unwrap();
    let (streams, _) = groups(&four, &leave_out);
    assert_eq!(streams.len(), 4, "four streams");
    let (threads0, fds0) = (threads(), descriptors());
    let (w, rx) = watch(&four, leave_out, ra_keep());
    for r in &four {
        let marker = r.join("armed.rs");
        write(&marker, "");
        heard(&rx, &marker, crate::test_results::PATIENCE);
    }
    assert_eq!(w.seen().streams.len(), 4);
    let (threads1, fds1) = (threads(), descriptors());
    let fds = new_descriptors(&fds0, &fds1);
    eprintln!(
        "TREEWATCH four streams running: new threads {:?} · {} new descriptors in all (the log's count {}) {fds:?}",
        new_threads(&threads0, &threads1),
        fds.len(),
        mac::descriptors(&streams),
    );
    drop(w);
    drop(rx);
    crate::event::sleep(Duration::from_secs(1));
    let (threads2, fds2) = (threads(), descriptors());
    eprintln!(
        "TREEWATCH four streams stopped: threads left of the watch's {:?} · descriptors left {:?}",
        new_threads(&threads0, &threads2),
        new_descriptors(&fds0, &fds2)
    );

    // CPU while 10,000 files are written below a folder left out, each case by exclusion paths: one root's
    // `target`; `target` holding a watched root (a build script's out dir, in the nested roots' stream); the last of
    // ten checkouts' `target`s — twenty folders left out, a stream a root. What reached the
    // prefix drop is said beside: nothing should
    for case in ["exclusion path", "the out dir, a stream apart", "twenty left-out folders over ten roots"] {
        let (_s, base) = scratch("measure-cpu");
        let (roots, target) = match case {
            "twenty left-out folders over ten roots" => {
                let roots: Vec<PathBuf> = (0..10).map(|i| base.join(format!("c{i}"))).collect();
                let target = roots[9].join("target");
                (roots, target)
            }
            "the out dir, a stream apart" => {
                let out = base.join("target/debug/build/x-1/out");
                fs::create_dir_all(&out).unwrap();
                (vec![base.clone(), out], base.join("target"))
            }
            _ => (vec![base.clone()], base.join("target")),
        };
        let leave_out: Vec<PathBuf> = roots.iter().flat_map(|r| [r.join(".git"), r.join("target")]).collect();
        leave_out.iter().for_each(|l| fs::create_dir_all(l).unwrap());
        let (streams, over) = groups(&roots, &leave_out);
        assert!(
            over.is_empty() && streams.iter().any(|g| g.exclude.contains(&target)),
            "{case}: {streams:#?}"
        );
        let (w, rx) = watch(&roots, leave_out, ra_keep());
        for r in &roots {
            let marker = r.join("armed.rs");
            write(&marker, "");
            heard(&rx, &marker, crate::test_results::PATIENCE);
        }
        let (cpu0, worker0, t0) = (cpu(), worker_cpu(&threads()), Instant::now());
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "treewatch::tests::measure::writes_for_the_measure",
                "--ignored",
                "--nocapture",
            ])
            .env("TARAE_TREEWATCH_WRITE", &target)
            .status()
            .unwrap();
        assert!(status.success());
        let wrote = t0.elapsed();
        // (in a root: with ten, `base` is only the folder above them)
        let marker = roots[0].join("after.rs");
        write(&marker, "");
        // (a slow fseventsd is behind by the flood: the marker's lag is part of the measure)
        heard(&rx, &marker, Duration::from_secs(900));
        let lag = t0.elapsed() - wrote;
        let (spent, worker, wall) = (cpu() - cpu0, worker_cpu(&threads()) - worker0, t0.elapsed());
        let seen = w.seen();
        eprintln!(
            "TREEWATCH cpu, {case}: process {spent:?}, the worker {worker:?}, over {wall:?} ({:.2} % / {:.2} % of a core) · the writes took {wrote:?}, the marker after them {lag:?} · the process {:.1} µs a file · {} streams · {} events dropped by prefix",
            spent.as_secs_f64() / wall.as_secs_f64() * 100.0,
            worker.as_secs_f64() / wall.as_secs_f64() * 100.0,
            spent.as_secs_f64() * 1e6 / 10_000.0,
            seen.streams.len(),
            seen.unwatched,
        );
        drop(w);
    }
}

/// FSEvents' own delivery on this Mac, over minutes: a bare stream (no worker), a file written every 20 s for
/// `TARAE_TREEWATCH_PROBE_S` seconds (300), each one's arrival said — late, or never.
#[test]
#[ignore = "by hand: FSEvents' delivery to a bare stream, over minutes (outside a sandbox)"]
fn fsevents_delivery_over_minutes() {
    let (_s, root) = scratch("probe");
    let secs: u64 = std::env::var("TARAE_TREEWATCH_PROBE_S").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    let t0 = Instant::now();
    let (_stream, raw) = mac::stream_for_test(std::slice::from_ref(&root), &[]).unwrap();
    eprintln!("TREEWATCH probe: stream started in {:?}", t0.elapsed());
    let mut written: Vec<(PathBuf, Instant)> = Vec::new();
    let mut next = Instant::now();
    while t0.elapsed() < Duration::from_secs(secs) {
        if Instant::now() >= next {
            let f = root.join(format!("p{}.rs", written.len()));
            fs::write(&f, "").unwrap();
            written.push((f, Instant::now()));
            next += Duration::from_secs(20);
        }
        if let Ok(Msg::Events(e)) = raw.recv_timeout(next.saturating_duration_since(Instant::now())) {
            for (p, flags) in e {
                match written.iter().find(|w| w.0 == p) {
                    Some((_, at)) => eprintln!(
                        "TREEWATCH probe: {} after {:?} (flags {flags:#x})",
                        p.display(),
                        at.elapsed()
                    ),
                    None => eprintln!(
                        "TREEWATCH probe: {} (not written by the probe, flags {flags:#x})",
                        p.display()
                    ),
                }
            }
        }
    }
    eprintln!("TREEWATCH probe: {} written over {:?}", written.len(), t0.elapsed());
}

/// What the flood's writer writes: kept-looking `.rs` files, or what a build writes (`.o`, `.d`, `.rmeta` in turn:
/// not kept, so an event and a stat each).
#[derive(Clone, Copy)]
enum Files {
    Rs,
    Build,
}

/// 10,000 files written below `target` by the writer in its own process (`files`), with the watch over `roots`
/// armed: the process's and the worker's CPU, the marker's lag after the flood, the paths said meanwhile and what
/// reached the prefix drop — one `TREEWATCH cpu` line and a `GATE` line at `gate` % of a core.
fn flood(case: &str, roots: &[PathBuf], leave_out: Vec<PathBuf>, target: &Path) {
    flood_of(case, roots, leave_out, target, Files::Rs, 2.0);
}

fn flood_of(case: &str, roots: &[PathBuf], leave_out: Vec<PathBuf>, target: &Path, files: Files, gate: f64) {
    let (w, rx) = watch(roots, leave_out, ra_keep());
    for r in roots {
        let marker = r.join("armed.rs");
        write(&marker, "");
        heard(&rx, &marker, crate::test_results::PATIENCE);
    }
    let (cpu0, worker0, t0) = (cpu(), worker_cpu(&threads()), Instant::now());
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "treewatch::tests::measure::writes_for_the_measure", "--ignored", "--nocapture"])
        .env("TARAE_TREEWATCH_WRITE", target)
        .env("TARAE_TREEWATCH_FILES", if matches!(files, Files::Build) { "build" } else { "rs" })
        .status()
        .unwrap();
    assert!(status.success());
    let wrote = t0.elapsed();
    let marker = roots[0].join("after.rs");
    write(&marker, "");
    let (mut said, end) = (0usize, Instant::now() + crate::test_results::PATIENCE);
    loop {
        let b = rx.recv_timeout(end.saturating_duration_since(Instant::now())).expect("the marker in time");
        said += b.len();
        if b.iter().any(|(p, _)| *p == marker) {
            break;
        }
    }
    let lag = t0.elapsed() - wrote;
    let (spent, worker, wall) = (cpu() - cpu0, worker_cpu(&threads()) - worker0, t0.elapsed());
    let seen = w.seen();
    let share = spent.as_secs_f64() / wall.as_secs_f64() * 100.0;
    eprintln!(
        "TREEWATCH cpu, {case}: process {spent:?}, the worker {worker:?}, over {wall:?} ({share:.2} % / {:.2} % of a core) · the writes took {wrote:?}, the marker after them {lag:?} · {} paths said meanwhile · {} streams (numbers {:?}) · {} events dropped by prefix · {} with no stat (no server keeps them)",
        worker.as_secs_f64() / wall.as_secs_f64() * 100.0,
        said.saturating_sub(1),
        seen.streams.len(),
        seen.streams.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        seen.unwatched,
        seen.unkept,
    );
    eprintln!(
        "GATE {} {case}: {share:.2} % of a core (gate {gate} %)",
        if share < gate { "ok  " } else { "FAIL" }
    );
}

/// The tree's left-out folders below `roots` as the LSP side gives them: each root's whole list
/// (`lspwatch::walk_left_out`), there or not.
fn left_out_as_given(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots.iter().flat_map(|r| crate::lspwatch::walk_left_out(r)).collect()
}

/// A scratch git repository with `.gitignore` `target` and `folders` made in it, committed (nothing to commit but
/// the ignore file).
fn repository(name: &str, folders: &[&str]) -> (Scratch, PathBuf) {
    let (s, root) = scratch(name);
    fs::write(root.join(".gitignore"), "target\n").unwrap();
    for f in folders {
        fs::create_dir_all(root.join(f)).unwrap();
    }
    crate::gitrepo::fixture_git(&root, &["init", "-q"]);
    crate::gitrepo::fixture_git(&root, &["add", "-A"]);
    crate::gitrepo::fixture_git(&root, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "t"]);
    (s, root)
}

/// W1b step 3's rows, by hand (FSEvents: outside a sandbox), each root's list as the LSP side gives it
/// (`lspwatch::walk_left_out`: there or not, the checkouts inside, the deep output folders git ignores):
/// - ten checkouts with a `.git` and a `target` each, and an out dir below the first one's `target` (`groups`: eleven
///   streams): each stream's start in a row, as `Streams::set` starts them; threads and descriptors with all eleven;
///   the CPU gate with 10,000 files below the tenth `target`;
/// - 1a: a root with no `target` at the start, the watch started, then `target` made by the flood;
/// - a workspace member's own `target`, ignored and there at the start;
/// - a checkout inside a checkout (`.claude/worktrees/x`): the outer root alone, then with the inner as a root of its
///   own (another server serving it);
/// - recorded, not gated: a member's own `target` made after the start (its first own build), with what a build
///   writes (`Files::Build`), against 10 %.
#[test]
#[ignore = "by hand: W1b step 3's measure (FSEvents: outside a sandbox)"]
fn the_step3_measure() {
    let (_s, base) = scratch("step3");
    let roots: Vec<PathBuf> = (0..10).map(|i| base.join(format!("c{i}"))).collect();
    for r in &roots {
        fs::create_dir_all(r.join(".git")).unwrap();
        fs::create_dir_all(r.join("target")).unwrap();
    }
    let out = roots[0].join("target/debug/build/x-1/out");
    fs::create_dir_all(&out).unwrap();
    let mut all = roots.clone();
    all.push(out);
    let leave_out = left_out_as_given(&roots);
    eprintln!("TREEWATCH ten roots: {} folders left out, {} of them there", leave_out.len(), {
        leave_out.iter().filter(|l| l.exists()).count()
    });
    let (streams, over) = groups(&all, &leave_out);
    assert!(over.is_empty() && streams.len() == 11, "{streams:#?}");
    let mut started = Vec::new();
    let mut times = Vec::new();
    for g in &streams {
        let t0 = Instant::now();
        started.push(mac::stream_for_test(&g.roots, &g.exclude).unwrap());
        times.push(t0.elapsed());
    }
    let sum: Duration = times.iter().sum();
    eprintln!("TREEWATCH ten roots: eleven streams started in a row in {sum:?}: {times:?}");
    drop(started);
    let (threads0, fds0) = (threads(), descriptors());
    let (w, rx) = watch(&all, leave_out.clone(), ra_keep());
    for r in &all {
        let marker = r.join("armed.rs");
        write(&marker, "");
        heard(&rx, &marker, crate::test_results::PATIENCE);
    }
    let (threads1, fds1) = (threads(), descriptors());
    eprintln!(
        "TREEWATCH ten roots running: {} streams · new threads {:?} · {} new descriptors (the log's count {})",
        w.seen().streams.len(),
        new_threads(&threads0, &threads1),
        new_descriptors(&fds0, &fds1).len(),
        mac::descriptors(&streams),
    );
    drop(w);
    drop(rx);
    crate::event::sleep(Duration::from_secs(1));
    flood("ten roots and an out dir, eleven streams", &all, leave_out, &roots[9].join("target"));

    // 1a: a fresh clone — no `target` as the watch starts; the flood's writer makes it
    let (_s, fresh) = repository("step3-fresh", &["src"]);
    let roots = vec![fresh.clone()];
    flood(
        "1a: the root's target made after the start",
        &roots,
        left_out_as_given(&roots),
        &fresh.join("target"),
    );

    // A workspace member's own `target`, below the root but not directly: ignored, there at the start
    let (_s, root) = repository("step3-member", &["target", "crates/member/target"]);
    let roots = vec![root.clone()];
    let leave_out = left_out_as_given(&roots);
    assert!(leave_out.contains(&root.join("crates/member/target")), "{leave_out:#?}");
    flood("a member's own target, ignored", &roots, leave_out, &root.join("crates/member/target"));

    // A checkout inside a checkout: the outer root alone, then the inner one a root of its own too
    let (_s, outer) = repository("step3-outer", &["target"]);
    let inner = outer.join(".claude/worktrees/x");
    fs::create_dir_all(inner.join("target")).unwrap();
    fs::write(inner.join(".git"), "gitdir: ../../../.git/worktrees/x\n").unwrap();
    let roots = vec![outer.clone()];
    flood(
        "a checkout inside a checkout, its target",
        &roots,
        left_out_as_given(&roots),
        &inner.join("target"),
    );
    let roots = vec![outer.clone(), inner.clone()];
    let both = left_out_as_given(&roots);
    flood(
        "a checkout inside a checkout, both roots, the inner's target",
        &roots,
        both,
        &inner.join("target"),
    );

    // Recorded, not gated: a member's own `target` made after the walk (its first own build), what a build writes
    let (_s, root) = repository("step3-member-later", &["target", "crates/member/src"]);
    let roots = vec![root.clone()];
    let leave_out = left_out_as_given(&roots);
    let later = root.join("crates/member/target");
    assert!(!leave_out.contains(&later), "{leave_out:#?}");
    flood_of(
        "recorded: a member's target made later, a build's files",
        &roots,
        leave_out,
        &later,
        Files::Build,
        10.0,
    );
}

/// W1b step 3's third card, by hand (FSEvents: outside a sandbox) — run once as built and once with `Tree::unwanted`
/// off (the card's "before"):
/// - the recorded row again, the same mix: a member's own `target` made after the walk, 10,000 `.o`/`.d`/`.rmeta`
///   (call A: at or under 6 % is the known rest);
/// - 10,000 of the same in a plain folder below a root (`out/`: nothing leaves it out, no server keeps them).
#[test]
#[ignore = "by hand: W1b step 3's third measure (FSEvents: outside a sandbox)"]
fn the_round3_measure() {
    let (_s, root) = repository("r3-member-later", &["target", "crates/member/src"]);
    let roots = vec![root.clone()];
    let leave_out = left_out_as_given(&roots);
    let later = root.join("crates/member/target");
    assert!(!leave_out.contains(&later), "{leave_out:#?}");
    flood_of(
        "recorded: a member's target made later, a build's files",
        &roots,
        leave_out,
        &later,
        Files::Build,
        6.0,
    );

    let (_s, root) = repository("r3-out", &["target", "src"]);
    let roots = vec![root.clone()];
    let leave_out = left_out_as_given(&roots);
    let out = root.join("out");
    assert!(!leave_out.contains(&out), "{leave_out:#?}");
    flood_of(
        "out/: a plain folder below the root, files no server keeps",
        &roots,
        leave_out,
        &out,
        Files::Build,
        6.0,
    );
}

/// The walk's time (`lspwatch::walk_left_out`) and its git's share (the walk alone, `lsp::nested_walk`, timed apart),
/// on each folder of `TARAE_WALK_ROOTS` (`:`-separated), and on a scratch repository past `DEEP_MAX` output-named
/// folders — one `TREEWATCH walk` line each.
#[test]
#[ignore = "by hand: the walk's time for W1b step 3's report"]
fn the_walks_time() {
    let (_s, synthetic) = scratch("walk-time");
    fs::write(synthetic.join(".gitignore"), "target\nnode_modules\n").unwrap();
    for i in 0..4000 {
        fs::create_dir_all(synthetic.join(format!("a{}/b{}/c{i}", i / 400, i / 40))).unwrap();
    }
    for i in 0..80 {
        fs::create_dir_all(synthetic.join(format!("a{}/m{i}/target", i % 10))).unwrap();
    }
    crate::gitrepo::fixture_git(&synthetic, &["init", "-q"]);
    let given = std::env::var("TARAE_WALK_ROOTS").unwrap_or_default();
    let mut roots: Vec<PathBuf> = given.split(':').filter(|s| !s.is_empty()).map(PathBuf::from).collect();
    roots.push(synthetic);
    for r in roots {
        let r = r.canonicalize().unwrap_or(r);
        let t0 = Instant::now();
        let nested = crate::lsp::nested_walk(&r);
        let walk = t0.elapsed();
        let t1 = Instant::now();
        let list = crate::lspwatch::walk_left_out(&r);
        let whole = t1.elapsed();
        let fixed = crate::lspwatch::tree_left_out(std::slice::from_ref(&r)).len();
        eprintln!(
            "TREEWATCH walk {}: the walk {walk:?} ({} checkouts, {} output-named below), with git {whole:?} → {} left \
             out ({} deep)",
            r.display(),
            nested.checkouts.len(),
            nested.outputs.len(),
            list.len(),
            list.len().saturating_sub(fixed + nested.checkouts.len()),
        );
    }
}

/// The CPU measure's writer, in a process of its own: 10,000 kept-looking files below `TARAE_TREEWATCH_WRITE`.
#[test]
#[ignore = "the_reports_measure's writer, in its own process"]
fn writes_for_the_measure() {
    let Some(dir) = std::env::var_os("TARAE_TREEWATCH_WRITE").map(PathBuf::from) else { return };
    let build = std::env::var("TARAE_TREEWATCH_FILES").is_ok_and(|f| f == "build");
    for i in 0..10_000 {
        if i % 100 == 0 {
            fs::create_dir_all(dir.join(format!("d{}", i / 100))).unwrap();
        }
        let ext = if build { ["o", "d", "rmeta"][i % 3] } else { "rs" };
        fs::write(dir.join(format!("d{}/f{i}.{ext}", i / 100)), "x").unwrap();
    }
}
