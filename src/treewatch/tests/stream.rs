//! By the real stream: FSEvents' service, reachable only outside a sandbox (tarae's denies it by design) — so these
//! skip themselves inside one, said as a skip (`sandbox::skipped_inside_for`), but the refusal's own test.

use super::*;
use std::process::Command;
use std::sync::mpsc::Receiver;

use crate::test_results::PATIENCE;

fn skipped(test: &str) -> bool {
    crate::sandbox::skipped_inside_for(
        &format!("treewatch::tests::stream::{test}"),
        "FSEvents' service is denied inside a sandbox",
    )
}

fn watching(roots: &[&Path], leave_out: Vec<PathBuf>) -> (TreeWatch, Receiver<Batch>) {
    let (tx, rx) = mpsc::channel();
    let w = TreeWatch::start(given(roots, leave_out), move |s: Said| drop(tx.send(s.batch)))
        .expect("the stream starts");
    (w, rx)
}

/// Batches until `done` holds of all heard so far, within the shared patience.
fn until(rx: &Receiver<Batch>, done: impl Fn(&[(PathBuf, Kind)]) -> bool) -> Vec<(PathBuf, Kind)> {
    let end = Instant::now() + PATIENCE;
    let mut heard = Vec::new();
    while !done(&heard) {
        match rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(b) => heard.extend(b),
            // (a long list as its count by kind: a loss of fifty in 5,000 is read by the numbers)
            Err(_) if heard.len() > 50 => {
                let n = |k: Kind| heard.iter().filter(|(_, kind)| *kind == k).count();
                let (c, d, ch) = (n(Kind::Created), n(Kind::Deleted), n(Kind::Changed));
                panic!("not heard within {PATIENCE:?}; heard {c} created, {d} deleted, {ch} changed")
            }
            Err(_) => panic!("not heard within {PATIENCE:?}; heard {heard:#?}"),
        }
    }
    heard
}

fn has(heard: &[(PathBuf, Kind)], path: &Path, kind: Kind) -> bool {
    heard.iter().any(|(p, k)| p == path && *k == kind)
}

/// The worker past its first walk: a marker file made and heard — what a test writes after it is said against a
/// full table (a file the walk met would be `Changed`, not `Created`). The marker itself may be either: the walk
/// can meet it.
fn armed(rx: &Receiver<Batch>, root: &Path) {
    let marker = root.join("armed.rs");
    write(&marker, "");
    until(rx, |h| h.iter().any(|(p, _)| *p == marker));
}

/// The words a watch said in their order, each with what it stood on (`Said::record`) — for a judge by path (#502).
type Words = Arc<std::sync::Mutex<Vec<(PathBuf, Kind, Option<Record>)>>>;

/// `watching`, its words also recorded — each in `Words` before its batch is sent.
fn watching_recorded(roots: &[&Path]) -> (TreeWatch, Receiver<Batch>, Words) {
    let (tx, rx) = mpsc::channel();
    let words = Words::default();
    let kept = Arc::clone(&words);
    let w = TreeWatch::start(given(roots, vec![]), move |s: Said| {
        let said = s.batch.iter().cloned().zip(s.record).map(|((p, k), r)| (p, k, r));
        kept.lock().unwrap().extend(said);
        drop(tx.send(s.batch))
    })
    .expect("the stream starts");
    (w, rx, words)
}

/// Whatever comes for a burst's length after the last thing awaited — what a stream gone wrong would still say. A net
/// for a late or doubled word, not a deadline: a word that lands past it, in what's awaited next, is judged the same
/// where a test judges by path (`five_thousand_…`, #502), whichever half it lands in.
fn late(rx: &Receiver<Batch>) -> Vec<(PathBuf, Kind)> {
    let end = Instant::now() + BURST_MAX + QUIET * 2;
    let mut heard = Vec::new();
    while let Ok(b) = rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
        heard.extend(b);
    }
    heard
}

/// A file written in place below a root (append: no new inode, no rename) reaches a batch, as `Changed`.
#[test]
fn a_file_written_in_place_below_a_root_reaches_a_batch() {
    if skipped("a_file_written_in_place_below_a_root_reaches_a_batch") {
        return;
    }
    let (_s, root) = scratch("stream-in-place");
    let a = root.join("src/a.rs");
    aged(&a, "fn a() {}");
    let (_w, rx) = watching(&[&root], vec![]);
    armed(&rx, &root);
    fs::OpenOptions::new().append(true).open(&a).unwrap().write_all(b"\n").unwrap();
    until(&rx, |h| has(h, &a, Kind::Changed));
}

/// A folder made with twenty files in one burst: all twenty said, each first as `Created`.
#[test]
fn a_folder_made_with_twenty_files_in_one_burst_says_all_twenty() {
    if skipped("a_folder_made_with_twenty_files_in_one_burst_says_all_twenty") {
        return;
    }
    let (_s, root) = scratch("stream-twenty");
    let (_w, rx) = watching(&[&root], vec![]);
    armed(&rx, &root);
    let files: Vec<PathBuf> = (0..20).map(|i| root.join(format!("pkg/m{i}.rs"))).collect();
    for f in &files {
        write(f, "pub fn f() {}");
    }
    let heard = until(&rx, |h| files.iter().all(|f| h.iter().any(|(p, _)| p == f)));
    for f in &files {
        let first = heard.iter().find(|(p, _)| p == f).map(|(_, k)| *k);
        assert_eq!(first, Some(Kind::Created), "{}", f.display());
    }
}

/// A folder removed (`rm -r`) or moved out of the tree (`mv`): each kept file it held is said deleted.
#[test]
fn a_folder_removed_or_moved_out_says_its_files_deleted() {
    if skipped("a_folder_removed_or_moved_out_says_its_files_deleted") {
        return;
    }
    let (_s, root) = scratch("stream-gone");
    let (_o, outside) = scratch("stream-gone-outside");
    let files = |dir: &str| [root.join(dir).join("a.rs"), root.join(dir).join("deep/b.rs")];
    for f in files("removed").iter().chain(&files("moved")) {
        aged(f, "text");
    }
    let (_w, rx) = watching(&[&root], vec![]);
    armed(&rx, &root);
    let run = |args: &[&str]| {
        assert!(Command::new(args[0]).args(&args[1..]).status().unwrap().success(), "{args:?}")
    };
    run(&["rm", "-r", root.join("removed").to_str().unwrap()]);
    run(&["mv", root.join("moved").to_str().unwrap(), outside.to_str().unwrap()]);
    let gone = [files("removed"), files("moved")].concat();
    until(&rx, |h| gone.iter().all(|f| has(h, f, Kind::Deleted)));
}

/// A root replaced by another: the new one's writes are said, and nothing more from the old one — its stream is
/// stopped, and what it said before is filtered by root.
#[test]
fn a_root_replaced_by_another_says_nothing_more_from_the_old() {
    if skipped("a_root_replaced_by_another_says_nothing_more_from_the_old") {
        return;
    }
    let (_a, a) = scratch("stream-root-a");
    let (_b, b) = scratch("stream-root-b");
    let (w, rx) = watching(&[&a], vec![]);
    armed(&rx, &a);
    w.set(vec![b.clone()], vec![]).expect("the new stream starts");
    armed(&rx, &b);
    write(&a.join("old.rs"), "old");
    let new = b.join("new.rs");
    write(&new, "new");
    let mut heard = until(&rx, |h| has(h, &new, Kind::Created));
    heard.extend(late(&rx));
    let old: Vec<_> = heard.iter().filter(|(p, _)| p.starts_with(&a)).collect();
    assert!(old.is_empty(), "the old root still said {old:?}");
}

/// Writes below a folder left out say nothing, whether the stream leaves it out (the first eight) or the worker
/// drops it by prefix (the ninth on) — and the stream's own exclusion does hold: its raw events never name it.
#[test]
fn writes_below_left_out_folders_say_nothing() {
    if skipped("writes_below_left_out_folders_say_nothing") {
        return;
    }
    let (_s, root) = scratch("stream-left-out");
    let out: Vec<PathBuf> = (0..9).map(|i| root.join(format!("out{i}"))).collect();
    out.iter().for_each(|o| fs::create_dir(o).unwrap());
    let (excluded, dropped) = (&out[0], &out[8]);
    let (_w, rx) = watching(&[&root], out.clone());
    armed(&rx, &root);
    write(&excluded.join("deep/a.rs"), "a");
    write(&dropped.join("deep/b.rs"), "b");
    let marker = root.join("after.rs");
    write(&marker, "");
    let mut heard = until(&rx, |h| has(h, &marker, Kind::Created));
    heard.extend(late(&rx));
    let said: Vec<_> = heard.iter().filter(|(p, _)| out.iter().any(|o| p.starts_with(o))).collect();
    assert!(said.is_empty(), "said below a folder left out: {said:?}");

    // The stream alone, its raw events: the excluded folder's writes never reach tarae at all
    let (_stream, raw) =
        mac::stream_for_test(std::slice::from_ref(&root), std::slice::from_ref(excluded)).unwrap();
    write(&excluded.join("c.rs"), "c");
    let marker = root.join("raw.rs");
    write(&marker, "");
    let (end, mut names) = (Instant::now() + PATIENCE, Vec::new());
    while !names.contains(&marker) {
        match raw.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(Msg::Events(e)) => names.extend(e.into_iter().map(|(p, _)| p)),
            Ok(_) => {}
            Err(_) => panic!("the marker's raw event didn't come; heard {names:#?}"),
        }
    }
    assert!(
        !names.iter().any(|p| p.starts_with(excluded)),
        "the stream named the excluded folder: {names:#?}"
    );
}

/// What's kept and what's left out, replaced while it runs: a match that was there all along comes in unsaid (an
/// event for it is a change, never a creation), and a folder newly left out goes quiet — through a new stream,
/// since the stream's own exclusions changed.
#[test]
fn keep_and_left_out_replaced_while_it_runs() {
    if skipped("keep_and_left_out_replaced_while_it_runs") {
        return;
    }
    let (_s, root) = scratch("stream-replaced");
    let readme = root.join("README.md");
    aged(&readme, "read me");
    fs::create_dir(root.join("gen")).unwrap();
    let (w, rx) = watching(&[&root], vec![]);
    armed(&rx, &root);
    w.keep(Arc::new(|p: &Path| p.extension().is_some_and(|e| e == "rs" || e == "md"))).expect("kept");
    w.set(vec![root.clone()], vec![root.join("gen")]).expect("a new stream for the new exclusion");
    fs::OpenOptions::new().append(true).open(&readme).unwrap().write_all(b"!").unwrap();
    write(&root.join("gen/g.rs"), "left out");
    let marker = root.join("after.rs");
    write(&marker, "");
    let mut heard = until(&rx, |h| has(h, &readme, Kind::Changed) && has(h, &marker, Kind::Created));
    heard.extend(late(&rx));
    let below: Vec<_> = heard.iter().filter(|(p, _)| p.starts_with(root.join("gen"))).collect();
    assert!(below.is_empty(), "said below the folder newly left out: {below:?}");
    assert!(!has(&heard, &readme, Kind::Created), "the new match was held unsaid: {heard:?}");
}

/// A root's `target` not there at the start (a fresh clone: the first build makes it), given as left out: made and
/// filled, nothing below it is said. The design's question first, by the stream alone — does FSEvents leave out an
/// exclusion path made after the stream started? — printed (`TREEWATCH 1a`) and asserted (honored on macOS 27, the
/// second card), then the watch: its one stream runs on and nothing reaches the prefix drop.
#[test]
fn a_left_out_folder_made_after_the_start_says_nothing() {
    if skipped("a_left_out_folder_made_after_the_start_says_nothing") {
        return;
    }
    let (_s, root) = scratch("stream-made-later");
    let target = root.join("target");
    let (_stream, raw) =
        mac::stream_for_test(std::slice::from_ref(&root), std::slice::from_ref(&target)).unwrap();
    write(&target.join("debug/raw.rs"), "");
    let marker = root.join("raw-marker.rs");
    write(&marker, "");
    let (end, mut names) = (Instant::now() + PATIENCE, Vec::new());
    while !names.contains(&marker) {
        match raw.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(Msg::Events(e)) => names.extend(e.into_iter().map(|(p, _)| p)),
            Ok(_) => {}
            Err(_) => panic!("the marker's raw event didn't come; heard {names:#?}"),
        }
    }
    let honored = !names.iter().any(|p| p.starts_with(&target) && *p != target);
    eprintln!(
        "TREEWATCH 1a: an exclusion path made after the stream started is {}",
        ["not honored (what's below it is named)", "honored"][honored as usize]
    );
    assert!(
        honored,
        "FSEvents named what's below an exclusion path made after its stream started: on this macOS only the \
         worker's prefix drop leaves such a folder out (each event reaches the worker) until the next roots change \
         starts the stream with it there — what that costs is to measure and decide (the design session's call B, \
         2026-11-11, took the restart out). Named: {names:#?}"
    );
    drop(_stream);
    fs::remove_dir_all(&target).unwrap();

    let (w, rx) = watching(&[&root], vec![root.join(".git"), target.clone()]);
    armed(&rx, &root);
    let made: Vec<PathBuf> = (0..200).map(|i| target.join(format!("debug/f{i}.rs"))).collect();
    made.iter().for_each(|f| write(f, ""));
    let marker = root.join("after.rs");
    write(&marker, "");
    let mut heard = until(&rx, |h| has(h, &marker, Kind::Created));
    heard.extend(late(&rx));
    let below: Vec<_> = heard.iter().filter(|(p, _)| p.starts_with(&target)).collect();
    assert!(below.is_empty(), "said below the folder made later: {below:?}");
    let seen = w.seen();
    let numbers: Vec<u64> = seen.streams.iter().map(|(n, _)| *n).collect();
    eprintln!("TREEWATCH 1a: streams {numbers:?}, {} events dropped by prefix", seen.unwatched);
    assert_eq!(numbers, [1], "the one stream runs on");
    // (the folder itself may be named, and under load in more than one burst — each dropped by prefix and counted;
    // what's below it is what must never reach the worker)
    let dropped_below: Vec<_> =
        seen.unwatched_paths.iter().filter(|p| p.starts_with(&target) && **p != target).collect();
    assert!(dropped_below.is_empty(), "nothing below it reached the worker: {dropped_below:#?}");
}

/// FSEvents' flags for a file's events, as `Tree::unwanted` reads them: a file made, written, renamed and removed is
/// named each time with `ItemIsFile` and nothing that says a folder or a link — and all four changes are seen.
/// Printed (`TREEWATCH flags`). Then the watch: such files no server keeps cost no stat (`Seen.unkept`).
#[test]
fn a_files_events_are_flagged_a_file() {
    if skipped("a_files_events_are_flagged_a_file") {
        return;
    }
    let (_s, root) = scratch("stream-flags");
    let (_stream, raw) = mac::stream_for_test(std::slice::from_ref(&root), &[]).unwrap();
    let (a, b) = (root.join("a.o"), root.join("b.o"));
    write(&a, "made");
    fs::OpenOptions::new().append(true).open(&a).unwrap().write_all(b" written").unwrap();
    fs::rename(&a, &b).unwrap();
    fs::remove_file(&b).unwrap();
    let marker = root.join("flags-marker.o");
    write(&marker, "");
    let (end, mut events) = (Instant::now() + PATIENCE, Vec::new());
    while !events.iter().any(|(p, _)| *p == marker) {
        match raw.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(Msg::Events(e)) => events.extend(e),
            Ok(_) => {}
            Err(_) => panic!("the marker's raw event didn't come; heard {events:#?}"),
        }
    }
    let of = |p: &Path| events.iter().filter(|(q, _)| q == p).map(|(_, f)| *f).collect::<Vec<u32>>();
    let (fa, fb) = (of(&a), of(&b));
    eprintln!("TREEWATCH flags: a.o {fa:#x?}, b.o {fb:#x?}");
    for f in fa.iter().chain(&fb) {
        assert!(
            f & flag::ITEM_IS_FILE != 0 && f & (flag::ITEM_IS_DIR | flag::ITEM_IS_SYMLINK | flag::SCAN) == 0,
            "a file's event not flagged a plain file: {f:#x} (a.o {fa:#x?}, b.o {fb:#x?})"
        );
    }
    let all = fa.iter().chain(&fb).fold(0, |x, f| x | f);
    // (made 0x100, removed 0x200, renamed 0x800, written 0x1000)
    for (bit, what) in [(0x100, "made"), (0x200, "removed"), (0x800, "renamed"), (0x1000, "written")] {
        assert!(all & bit != 0, "no event said {what}: a.o {fa:#x?}, b.o {fb:#x?}");
    }
    drop(_stream);

    let (w, rx) = watching(&[&root], vec![]);
    armed(&rx, &root);
    (0..50).for_each(|i| write(&root.join(format!("out/f{i}.o")), ""));
    let after = root.join("after.rs");
    write(&after, "");
    until(&rx, |h| has(h, &after, Kind::Created));
    let unkept = w.seen().unkept;
    eprintln!("TREEWATCH flags: the watch dropped {unkept} events with no stat for 50 files no server keeps");
    assert!(unkept >= 50, "each unkept file's event dropped with no stat ({unkept})");
}

/// A root removed and made again (a `cargo clean`, then a build): its file said deleted, then the new one created
/// — it stays a root.
#[test]
fn a_root_removed_and_made_again_is_heard_again() {
    if skipped("a_root_removed_and_made_again_is_heard_again") {
        return;
    }
    let (_s, base) = scratch("stream-root-again");
    let out = base.join("out");
    let gen_rs = out.join("gen.rs");
    write(&gen_rs, "gen");
    let (_w, rx) = watching(&[&out], vec![]);
    armed(&rx, &out);
    fs::remove_dir_all(&out).unwrap();
    until(&rx, |h| has(h, &gen_rs, Kind::Deleted));
    let again = out.join("again.rs");
    write(&again, "again");
    until(&rx, |h| has(h, &again, Kind::Created));
}

/// A root not there when the watch starts (another server's out dir before its build) is taken, and heard once
/// it's made.
#[test]
fn a_root_not_there_at_the_start_is_heard_once_made() {
    if skipped("a_root_not_there_at_the_start_is_heard_once_made") {
        return;
    }
    let (_a, a) = scratch("stream-absent-a");
    let (_b, b) = scratch("stream-absent-b");
    let later = b.join("later");
    let (_w, rx) = watching(&[&a, &later], vec![]);
    armed(&rx, &a);
    let made = later.join("made.rs");
    write(&made, "made");
    until(&rx, |h| has(h, &made, Kind::Created));
}

/// A root in a second stream — a build script's out dir below `target` — is heard, while the first stream leaves
/// `target` out: a flood of writes beside the out dir never reaches the worker (nothing dropped by prefix).
#[test]
fn a_root_in_a_second_stream_is_heard_below_a_folder_the_first_excludes() {
    if skipped("a_root_in_a_second_stream_is_heard_below_a_folder_the_first_excludes") {
        return;
    }
    let (_s, root) = scratch("stream-second");
    let (target, out) = (root.join("target"), root.join("target/debug/build/x-1/out"));
    fs::create_dir_all(&out).unwrap();
    fs::create_dir(root.join(".git")).unwrap();
    let (w, rx) = watching(&[&root, &out], vec![root.join(".git"), target.clone()]);
    armed(&rx, &root);
    armed(&rx, &out);
    for i in 0..100 {
        write(&target.join(format!("debug/flood/f{i}.rs")), "left out");
    }
    let (made, marker) = (out.join("gen.rs"), root.join("after.rs"));
    write(&made, "gen");
    write(&marker, "");
    let mut heard = until(&rx, |h| has(h, &made, Kind::Created) && has(h, &marker, Kind::Created));
    heard.extend(late(&rx));
    let seen = w.seen();
    assert_eq!(seen.unwatched, 0, "what's below target reached the worker; heard {heard:#?}");
    let streams: Vec<Group> = seen.streams.into_iter().map(|(_, g)| g).collect();
    let want = [
        Group { roots: vec![root.clone()], exclude: vec![root.join(".git"), target] },
        Group { roots: vec![out], exclude: vec![] },
    ];
    assert_eq!(streams, want);
}

/// A `set` that changes one root restarts one stream: three roots (five folders left out each), a stream each, the
/// second replaced — the first's and third's streams go on, their numbers kept and
/// still heard; the new root's is the one stream started.
#[test]
fn a_set_changing_one_root_restarts_one_stream() {
    if skipped("a_set_changing_one_root_restarts_one_stream") {
        return;
    }
    let (_s, base) = scratch("stream-one-set");
    let [a, b, c, d] = ["a", "b", "c", "d"].map(|n| base.join(n));
    let five = |r: &PathBuf| [".git", "target", "l2", "l3", "l4"].map(|n| r.join(n));
    [&a, &b, &c, &d].into_iter().flat_map(five).for_each(|l| fs::create_dir_all(l).unwrap());
    let leave = |roots: [&PathBuf; 3]| roots.into_iter().flat_map(five).collect::<Vec<_>>();
    let (w, rx) = watching(&[&a, &b, &c], leave([&a, &b, &c]));
    for r in [&a, &b, &c] {
        armed(&rx, r);
    }
    let before = w.seen().streams;
    assert_eq!(before.len(), 3, "{before:#?}");
    w.set(vec![a.clone(), d.clone(), c.clone()], leave([&a, &d, &c])).expect("the new stream starts");
    armed(&rx, &d);
    let after = w.seen().streams;
    let started: Vec<&(u64, Group)> =
        after.iter().filter(|(n, _)| !before.iter().any(|(m, _)| m == n)).collect();
    assert_eq!(started.len(), 1, "streams started by the set: {started:#?}");
    assert_eq!(started[0].1.roots, std::slice::from_ref(&d));
    assert_eq!(after.len(), 3, "{after:#?}");
    let still = [a.join("still.rs"), c.join("still.rs")];
    still.iter().for_each(|f| write(f, ""));
    until(&rx, |h| still.iter().all(|f| has(h, f, Kind::Created)));
}

/// A checkout's burst of kept files arrives whole (the design session, 2026-11-11: "a big burst of kept files must
/// arrive whole before the commit"): 5,000 `.rs` in 50 folders made, then the 50 folders removed in one `rm -r`, as
/// a checkout takes a branch's files and their folders. Exactly 5,000 said created and then 5,000 said deleted,
/// each path once each way, judged by path across both halves: created, then deleted, nothing missing or doubled,
/// nothing else of theirs (a burst's length after the last awaited, for one late or doubled). A look at the root
/// right after each half (as the LSP side's looks come at a run's end, git's moment). A changed is allowed only where
/// its record proves a reason (#502's calls, `Record::reason`): lost events, an event past its mark, a look's debt
/// past `disk::RACY`, an event on an entry an event put in an earlier burst (a make split by a burst's end — at load
/// the make runs for seconds, cut every `BURST_MAX`), the stamp moved. One with none is round 5's bug (a compare
/// saying changed for no reason: its extra 5,000): printed as "no reason", failed. On a quiet machine: 0 changed.
#[test]
fn five_thousand_kept_files_made_and_removed_with_their_folders_arrive_whole() {
    if skipped("five_thousand_kept_files_made_and_removed_with_their_folders_arrive_whole") {
        return;
    }
    const FILES: usize = 5_000;
    let (_s, root) = scratch("stream-whole");
    let many = root.join("many");
    let (w, rx, words) = watching_recorded(&[&root]);
    armed(&rx, &root);
    let dirs: Vec<PathBuf> = (0..50).map(|d| many.join(format!("d{d}"))).collect();
    let files: Vec<PathBuf> = (0..FILES).map(|i| dirs[i / 100].join(format!("f{i}.rs"))).collect();
    let of = |h: &[(PathBuf, Kind)], k: Kind| {
        h.iter().filter(|(p, kind)| *kind == k && p.starts_with(&many)).count()
    };
    let t0 = Instant::now();
    files.iter().for_each(|f| write(f, "pub fn f() {}\n"));
    let mut made = until(&rx, |h| of(h, Kind::Created) >= FILES);
    let made_in = t0.elapsed();
    w.look(vec![root.clone()]).expect("a look");
    made.extend(late(&rx));
    let t1 = Instant::now();
    let st = Command::new("rm").arg("-r").args(&dirs).status().unwrap();
    assert!(st.success(), "rm -r of the 50 folders");
    let mut gone = until(&rx, |h| of(h, Kind::Deleted) >= FILES);
    let gone_in = t1.elapsed();
    w.look(vec![root.clone()]).expect("a look");
    gone.extend(late(&rx));
    let distinct = |h: &[(PathBuf, Kind)], k: Kind| {
        let set: std::collections::BTreeSet<&PathBuf> =
            h.iter().filter(|(p, kind)| *kind == k && p.starts_with(&many)).map(|(p, _)| p).collect();
        set.len()
    };
    let line = |h: &[(PathBuf, Kind)]| {
        format!(
            "{} created ({} distinct), {} deleted ({} distinct), {} changed",
            of(h, Kind::Created),
            distinct(h, Kind::Created),
            of(h, Kind::Deleted),
            distinct(h, Kind::Deleted),
            of(h, Kind::Changed)
        )
    };
    // (by path, across both halves: each word of a file below `many`, in order, with its record)
    let mut by_path: BTreeMap<PathBuf, Vec<(Kind, Option<Record>)>> = BTreeMap::new();
    for (p, k, r) in words.lock().unwrap().iter().filter(|(p, _, _)| p.starts_with(&many)) {
        by_path.entry(p.clone()).or_default().push((*k, *r));
    }
    let (mut wrong, mut changed, mut reasons) = (Vec::new(), Vec::new(), BTreeMap::<String, usize>::new());
    for f in &files {
        let said = by_path.remove(f).unwrap_or_default();
        let shape = said.len() >= 2
            && said[0].0 == Kind::Created
            && said[said.len() - 1].0 == Kind::Deleted
            && said[1..said.len() - 1].iter().all(|(k, _)| *k == Kind::Changed);
        if !shape {
            wrong.push(format!("{}: {:?}", f.display(), said.iter().map(|(k, _)| *k).collect::<Vec<_>>()));
        }
        for (_, r) in said.iter().filter(|(k, _)| *k == Kind::Changed) {
            let reason = r.and_then(|r| r.reason());
            *reasons.entry(reason.map_or("no reason".into(), |r| format!("{r:?}"))).or_default() += 1;
            let line = r.map_or("no reason: no record".into(), |r| r.line());
            changed.push((reason.is_none(), format!("{}: {line}", f.display())));
        }
    }
    wrong.extend(by_path.keys().map(|p| format!("{}: not a file made", p.display())));
    eprintln!(
        "TREEWATCH whole: made, all heard in {made_in:?}: {} · removed, all heard in {gone_in:?}: {} · changed by \
         reason {reasons:?}",
        line(&made),
        line(&gone)
    );
    let first = |list: &[String]| list.iter().take(50).cloned().collect::<Vec<_>>().join("\n  ");
    assert!(
        wrong.is_empty(),
        "{} files not created then deleted once each:\n  {}",
        wrong.len(),
        first(&wrong)
    );
    // (a changed with no reason the record proves: each changed printed, those first)
    crate::sorting::by_key(&mut changed, |(none, _)| !*none);
    let lines: Vec<String> = changed.iter().map(|(_, l)| l.clone()).collect();
    let none = changed.iter().filter(|(none, _)| *none).count();
    assert_eq!(none, 0, "{none} changed with no reason, of {}:\n  {}", changed.len(), first(&lines));
}

/// By hand: a root below a folder left out. The watch says a write there (that folder holds a root, so it isn't
/// given to the stream); then the design's question — does a stream given that folder as an exclusion path still
/// name what's below the root inside it? Printed, either way.
#[test]
#[ignore = "by hand: the real stream, its answer printed"]
fn a_root_below_a_left_out_folder_by_the_real_stream() {
    if skipped("a_root_below_a_left_out_folder_by_the_real_stream") {
        return;
    }
    let (_s, root) = scratch("stream-nested");
    let (target, out) = (root.join("target"), root.join("target/out"));
    fs::create_dir_all(&out).unwrap();
    let (_w, rx) = watching(&[&root, &out], vec![target.clone()]);
    armed(&rx, &root);
    let made = out.join("gen.rs");
    write(&made, "");
    until(&rx, |h| has(h, &made, Kind::Created));

    // The stream alone, given the folder above the root as an exclusion path
    let (_stream, raw) = mac::stream_for_test(&[root.clone(), out.clone()], &[target]).unwrap();
    write(&out.join("raw.rs"), "");
    let marker = root.join("raw-marker.rs");
    write(&marker, "");
    let (end, mut events) = (Instant::now() + PATIENCE, Vec::new());
    while !events.iter().any(|(p, _)| *p == marker) {
        match raw.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(Msg::Events(e)) => events.extend(e),
            Ok(_) => {}
            Err(_) => panic!("the marker's raw event didn't come; heard {events:#?}"),
        }
    }
    let below = events.iter().any(|(p, _)| p.starts_with(&out));
    eprintln!(
        "an exclusion path above a root: events below the root {}",
        ["don't come", "come"][below as usize]
    );
}

/// Seatbelt's rules with only FSEvents' service denied.
const NO_FSEVENTS: &str = "(version 1)(allow default)(deny mach-lookup (global-name \"com.apple.FSEvents\"))";

/// Where FSEvents' service is denied the start says no, in words, and nothing on stderr (libFSEvents may print
/// there, over a full-screen tarae) — this binary again in a process of its own: inside tarae's sandbox (where a
/// tarae running there is), which it inherits, or under `sandbox-exec` with only that service denied.
#[test]
fn the_start_says_no_where_fsevents_is_denied() {
    let (_s, root) = scratch("stream-denied");
    let exe = std::env::current_exe().unwrap();
    let mut child = if std::env::var_os(crate::sandbox::MARK).is_some() {
        Command::new(exe)
    } else {
        let mut c = Command::new(crate::sandbox::SANDBOX_EXEC);
        c.args(["-p", NO_FSEVENTS]).arg(exe);
        c
    };
    let out = child
        .args(["--exact", "treewatch::tests::stream::refused_in_its_own_process", "--ignored", "--quiet"])
        .env("TARAE_TREEWATCH_DENIED", &root)
        .output()
        .unwrap();
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && stdout.contains("1 passed"), "{stdout}{stderr}");
    assert!(stderr.is_empty(), "the refused start wrote to stderr: {stderr}");
}

#[test]
#[ignore = "the_start_says_no_where_fsevents_is_denied's own process, under sandbox-exec"]
fn refused_in_its_own_process() {
    let Some(root) = std::env::var_os("TARAE_TREEWATCH_DENIED") else { return };
    refused(Path::new(&root));
}

fn refused(root: &Path) {
    let why =
        TreeWatch::start(given(&[root], vec![]), |_| {}).err().expect("refused where FSEvents is denied");
    assert!(why.contains("FSEvents"), "{why}");
}
