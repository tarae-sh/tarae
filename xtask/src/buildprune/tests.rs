use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tarae_testkit::scratch::Scratch;

use std::process::Command;

use super::*;

const DAY: Duration = Duration::from_secs(24 * 3600);

/// A cargo home of the test's own with `build/` in it — the place, by its real path.
struct Home {
    dir: Scratch,
    build: PathBuf,
}

fn home(name: &str) -> Home {
    let dir = Scratch::new(name);
    std::fs::create_dir_all(dir.join("cargo/build")).unwrap();
    let build = place(&dir.join("cargo/build"), Some(&dir.join("user"))).unwrap();
    Home { dir, build }
}

/// `path`'s modified time set `ago` back from now.
fn aged(path: &Path, ago: Duration) {
    let f = File::open(path).unwrap();
    f.set_modified(SystemTime::now() - ago).unwrap();
}

fn write(path: &Path, bytes: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![7u8; bytes]).unwrap();
}

/// A build folder `rel` under `h` with its lock and a `debug/` profile that built this repository's `tarae` — its
/// folder.
fn folder(h: &Home, rel: &str) -> PathBuf {
    let b = other(h, rel);
    std::fs::create_dir_all(b.join("debug/.fingerprint/tarae-1111111111111111")).unwrap();
    b
}

/// `folder` as another project's: none of this repository's packages built there.
fn other(h: &Home, rel: &str) -> PathBuf {
    let b = h.build.join(rel);
    std::fs::create_dir_all(b.join("debug/deps")).unwrap();
    std::fs::create_dir_all(b.join("debug/incremental")).unwrap();
    std::fs::create_dir_all(b.join("debug/.fingerprint/memchr-2222222222222222")).unwrap();
    File::create(b.join("debug/.cargo-build-lock")).unwrap();
    b
}

/// Every folder of `b` that the pass reads as touched, set `ago` back.
fn untouched(b: &Path, ago: Duration) {
    for d in ["debug/deps", "debug/incremental", "debug/.fingerprint", "debug", ""] {
        aged(&b.join(d), ago);
    }
}

/// A removing pass's options; `claimed` the checkouts' folders (this repository's package `tarae`).
fn opts(age: Option<Duration>, whole: bool, claimed: std::result::Result<Vec<PathBuf>, PathBuf>) -> Opts {
    let claimed = claimed.map(|dirs| Claims { dirs, own: vec!["tarae".into()] });
    Opts { age, whole, others: false, claimed, remove: true }
}

fn prune(h: &Home, o: &Opts) -> Pruned {
    pass(&h.build, o, SystemTime::now())
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> =
        std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
    out.sort();
    out
}

/// #168's leak, laid out as rustc leaves it on macOS: in `deps/`, an incremental unit's objects — the current
/// ones linked from its session too (one also under an older name), orphans with no link elsewhere (one under two
/// names, its space counted once) — a dependency's objects built without incremental (no link elsewhere, its
/// library there) and a unit whose output is gone. The orphans go, and the gone unit's; the rest stays, a fresh
/// orphan too (a build may have only just let go of the lock).
#[test]
fn orphaned_objects_go_and_what_a_binary_names_stays() {
    let h = home("buildprune-orphans");
    let b = folder(&h, "ab/cdef0123");
    let (deps, session) = (b.join("debug/deps"), b.join("debug/incremental/tarae-3abc/s-1"));
    let unit = "tarae-1111111111111111";
    write(&deps.join(unit), 64);
    write(&session.join("cur.o"), 4096);
    for name in ["cur.aa", "cur.bb"] {
        std::fs::hard_link(session.join("cur.o"), deps.join(format!("{unit}.{name}.rcgu.o"))).unwrap();
    }
    write(&deps.join(format!("{unit}.old.cc.rcgu.o")), 8192);
    write(&deps.join(format!("{unit}.twice.dd.rcgu.o")), 8192);
    std::fs::hard_link(
        deps.join(format!("{unit}.twice.dd.rcgu.o")),
        deps.join(format!("{unit}.twice.ee.rcgu.o")),
    )
    .unwrap();
    write(&deps.join(format!("{unit}.fresh.ff.rcgu.o")), 8192);
    write(&deps.join("memchr-2222222222222222.memchr.a1-cgu.0.rcgu.o"), 4096);
    write(&deps.join("libmemchr-2222222222222222.rlib"), 64);
    write(&deps.join("gone-3333333333333333.x.y.rcgu.o"), 4096);
    for f in std::fs::read_dir(&deps).unwrap().flatten() {
        if !f.file_name().to_string_lossy().contains("fresh") {
            aged(&f.path(), Duration::from_secs(3600));
        }
    }
    let blocks =
        |n: &str| std::os::unix::fs::MetadataExt::blocks(&std::fs::metadata(deps.join(n)).unwrap()) * 512;
    let freed = blocks(&format!("{unit}.old.cc.rcgu.o"))
        + blocks(&format!("{unit}.twice.dd.rcgu.o"))
        + blocks("gone-3333333333333333.x.y.rcgu.o");
    let p = prune(&h, &opts(None, false, Ok(Vec::new())));
    assert_eq!(
        names(&deps),
        [
            "libmemchr-2222222222222222.rlib",
            "memchr-2222222222222222.memchr.a1-cgu.0.rcgu.o",
            "tarae-1111111111111111",
            "tarae-1111111111111111.cur.aa.rcgu.o",
            "tarae-1111111111111111.cur.bb.rcgu.o",
            "tarae-1111111111111111.fresh.ff.rcgu.o",
        ]
    );
    assert_eq!(p.objects, 4, "old.cc, twice.dd and .ee, gone's: {p:?}");
    assert_eq!(p.freed, freed, "each file once — twice.dd's second name not counted again: {p:?}");
    assert!(session.join("cur.o").exists(), "its session untouched");
    assert!(p.said().unwrap().contains("4 old objects"), "{:?}", p.said());
    assert_eq!(p.each.len(), 1, "its folder named: {p:?}");
    assert_eq!(p.each[0].0, b);
}

/// With an age: an incremental cache whose newest session is older goes (what of it has no link elsewhere
/// counted), a newer one stays; a test binary older than another of its name goes with its `.d`, one alone stays.
#[test]
fn stale_caches_and_old_binaries_go_with_an_age() {
    let h = home("buildprune-age");
    let b = folder(&h, "ab/cdef0123");
    let (deps, incr) = (b.join("debug/deps"), b.join("debug/incremental"));
    write(&incr.join("tarae-old/s-1/a.o"), 4096);
    write(&incr.join("tarae-new/s-2/a.o"), 4096);
    for p in ["tarae-old/s-1/a.o", "tarae-old/s-1", "tarae-old"] {
        aged(&incr.join(p), 3 * DAY);
    }
    write(&deps.join("tarae-4444444444444444"), 4096);
    write(&deps.join("tarae-4444444444444444.d"), 64);
    write(&deps.join("tarae-1111111111111111"), 4096);
    write(&deps.join("xtask-5555555555555555"), 4096);
    aged(&deps.join("tarae-4444444444444444"), 3 * DAY);
    aged(&deps.join("xtask-5555555555555555"), 3 * DAY);
    let p = prune(&h, &opts(Some(DAY), false, Ok(Vec::new())));
    assert!(!incr.join("tarae-old").exists() && incr.join("tarae-new").exists());
    assert!(!deps.join("tarae-4444444444444444").exists() && !deps.join("tarae-4444444444444444.d").exists());
    assert!(deps.join("tarae-1111111111111111").exists(), "the newest of its name");
    assert!(deps.join("xtask-5555555555555555").exists(), "alone of its name");
    assert_eq!((p.caches, p.binaries, p.folders), (1, 1, 0), "{p:?}");
    // Without an age: only orphaned objects
    write(&incr.join("tarae-other/s-3/a.o"), 4096);
    aged(&incr.join("tarae-other"), 3 * DAY);
    let p = prune(&h, &opts(None, false, Ok(Vec::new())));
    assert_eq!(p, Pruned::default());
    assert!(incr.join("tarae-other").exists());
}

/// Without `--yes` nothing is removed: the listing says what a removing pass then does, folder by folder.
#[test]
fn the_listing_removes_nothing_and_says_what_yes_would() {
    let h = home("buildprune-list");
    let b = folder(&h, "ab/cdef0123");
    let (deps, incr) = (b.join("debug/deps"), b.join("debug/incremental"));
    write(&deps.join("gone-3333333333333333.x.y.rcgu.o"), 4096);
    aged(&deps.join("gone-3333333333333333.x.y.rcgu.o"), Duration::from_secs(3600));
    write(&incr.join("tarae-old/s-1/a.o"), 4096);
    for p in ["tarae-old/s-1/a.o", "tarae-old/s-1", "tarae-old"] {
        aged(&incr.join(p), 3 * DAY);
    }
    let gone = folder(&h, "cd/00000000000002");
    write(&gone.join("debug/deps/x-1111111111111111"), 4096);
    untouched(&gone, 3 * DAY);
    let before = (names(&deps), names(&incr), names(&h.build.join("cd")));
    let list = Opts { remove: false, ..opts(Some(DAY), true, Ok(vec![b.clone()])) };
    let listed = prune(&h, &list);
    assert_eq!((names(&deps), names(&incr), names(&h.build.join("cd"))), before, "nothing removed");
    let text = report(&listed, false, Some(h.dir.path()));
    assert!(
        text.contains("cdef0123: ") && text.contains("1 old object, 1 stale incremental cache"),
        "{text}"
    );
    assert!(
        text.contains("00000002: ")
            && text.contains(
                "no checkout of this repository builds here; this repository's (it built tarae), untouched 3d"
            ),
        "{text}"
    );
    assert!(text.contains("~/cargo/build/"), "the home as ~: {text}");
    assert!(text.ends_with("--yes removes it\n"), "{text}");
    let removed = prune(&h, &Opts { remove: true, ..list });
    assert_eq!(listed, removed, "the same pass, removing");
    assert!(!gone.exists() && !incr.join("tarae-old").exists());
    assert!(report(&removed, true, None).contains("Freed "));
}

/// A profile folder whose lock a build holds is left alone, said busy — the lock taken as cargo takes it
/// (`flock`, one per profile); one with no lock file isn't the layout it knows.
#[test]
fn a_folder_under_a_build_is_left_alone() {
    let h = home("buildprune-busy");
    let b = folder(&h, "ab/cdef0123");
    let orphan = b.join("debug/deps/gone-3333333333333333.x.y.rcgu.o");
    write(&orphan, 4096);
    aged(&orphan, Duration::from_secs(3600));
    let build = File::open(b.join("debug/.cargo-build-lock")).unwrap();
    build.lock().unwrap();
    let p = prune(&h, &opts(Some(DAY), false, Ok(Vec::new())));
    assert_eq!((p.busy, p.objects), (1, 0), "{p:?}");
    assert!(orphan.exists());
    assert!(report(&p, true, None).contains("1 profile left while a build holds its lock"));
    build.unlock().unwrap();
    let p = prune(&h, &opts(Some(DAY), false, Ok(Vec::new())));
    assert_eq!((p.busy, p.objects), (0, 1), "{p:?}");
    // No lock file: not the layout it knows — nothing
    let bare = h.build.join("cd/ef012345/debug/deps/gone-6666666666666666.x.y.rcgu.o");
    write(&bare, 4096);
    aged(&bare, Duration::from_secs(3600));
    let p = prune(&h, &opts(Some(DAY), true, Ok(Vec::new())));
    assert_eq!(p, Pruned::default());
    assert!(bare.exists());
}

/// `--whole`: a build folder no checkout claims and untouched for the age goes whole; one claimed, one touched
/// since, or any when a checkout didn't say where it builds, stays.
#[test]
fn a_folder_no_checkout_builds_in_goes_whole_only_when_every_checkout_said() {
    let h = home("buildprune-whole");
    let (gone, claimed, recent) =
        (folder(&h, "ab/00000000000001"), folder(&h, "ab/00000000000002"), folder(&h, "cd/00000000000003"));
    for b in [&gone, &claimed] {
        write(&b.join("debug/deps/x-1111111111111111"), 4096);
        untouched(b, 3 * DAY);
    }
    let unsure = prune(&h, &opts(Some(DAY), true, Err(PathBuf::from("/w/co"))));
    assert_eq!(unsure.folders, 0);
    assert!(unsure.unsure.as_deref().is_some_and(|u| u.contains("/w/co didn't say")), "{unsure:?}");
    assert!(gone.exists());
    let whole = || prune(&h, &opts(Some(DAY), true, Ok(vec![claimed.clone()])));
    // (a second profile under a build: the folder stays whole, its idle profile pruned as usual)
    std::fs::create_dir_all(gone.join("release/deps")).unwrap();
    let lock = gone.join("release/.cargo-build-lock");
    File::create(&lock).unwrap();
    aged(&gone.join("release/deps"), 3 * DAY);
    aged(&gone.join("release"), 3 * DAY);
    untouched(&gone, 3 * DAY);
    let build = File::open(&lock).unwrap();
    build.lock().unwrap();
    let p = whole();
    assert_eq!((p.folders, p.busy), (0, 1), "{p:?}");
    assert!(gone.exists());
    build.unlock().unwrap();
    let p = whole();
    assert_eq!(p.folders, 1, "{p:?}");
    assert!(!gone.exists() && claimed.exists() && recent.exists());
    assert!(h.build.join("ab").is_dir(), "only the build folder");
}

/// A marker folder in the test's own scratch (`keep.txt` in it, old, with an old orphan-looking object and an
/// old cache-looking folder) — what a planted link leads to; it must survive every pass.
fn marker(h: &Home, name: &str) -> PathBuf {
    let m = h.dir.join(name);
    write(&m.join("keep.txt"), 64);
    write(&m.join("gone-3333333333333333.x.y.rcgu.o"), 64);
    write(&m.join("tarae-old/s-1/a.o"), 64);
    write(&m.join("deps/gone-3333333333333333.x.y.rcgu.o"), 64);
    for p in
        ["keep.txt", "gone-3333333333333333.x.y.rcgu.o", "tarae-old/s-1/a.o", "tarae-old/s-1", "tarae-old"]
    {
        aged(&m.join(p), 3 * DAY);
    }
    std::fs::canonicalize(m).unwrap()
}

fn intact(m: &Path) {
    for p in [
        "keep.txt",
        "gone-3333333333333333.x.y.rcgu.o",
        "tarae-old/s-1/a.o",
        "deps/gone-3333333333333333.x.y.rcgu.o",
    ] {
        assert!(m.join(p).exists(), "{} survives", m.join(p).display());
    }
}

/// Review #168: a build folder is written by sandboxed runs, its pruning isn't sandboxed — a link planted at
/// `incremental`, at a profile, at `deps`, at an entry inside each, at the build folder itself or inside one
/// removed whole is never gone through: the marker it leads to survives every pass, the link inside a removal
/// goes as itself.
#[test]
fn planted_links_are_never_gone_through() {
    use std::os::unix::fs::symlink;
    let h = home("buildprune-links");
    let m = marker(&h, "marker");
    let pass_with = |whole: bool| prune(&h, &opts(Some(DAY), whole, Ok(Vec::new())));
    // `incremental` a link
    let a = folder(&h, "ab/00000000000001");
    std::fs::remove_dir(a.join("debug/incremental")).unwrap();
    symlink(&m, a.join("debug/incremental")).unwrap();
    // `deps` a link
    let b = folder(&h, "ab/00000000000002");
    std::fs::remove_dir(b.join("debug/deps")).unwrap();
    symlink(&m, b.join("debug/deps")).unwrap();
    // a profile a link (`release` → a folder with a `deps/` in it)
    let c = folder(&h, "ab/00000000000003");
    symlink(&m, c.join("release")).unwrap();
    // entries inside: a cache that's a link, an object that's a link, a link inside a stale cache
    let d = folder(&h, "ab/00000000000004");
    symlink(&m, d.join("debug/incremental/tarae-linked")).unwrap();
    symlink(
        m.join("gone-3333333333333333.x.y.rcgu.o"),
        d.join("debug/deps/gone-3333333333333333.a.b.rcgu.o"),
    )
    .unwrap();
    write(&d.join("debug/incremental/tarae-stale/s-1/a.o"), 64);
    symlink(&m, d.join("debug/incremental/tarae-stale/s-1/in")).unwrap();
    for p in [
        "debug/incremental/tarae-stale/s-1/a.o",
        "debug/incremental/tarae-stale/s-1",
        "debug/incremental/tarae-stale",
    ] {
        aged(&d.join(p), 3 * DAY);
    }
    // the build folder itself a link
    symlink(&m, h.build.join("ab/00000000000005")).unwrap();
    let p = pass_with(false);
    intact(&m);
    assert!(
        !d.join("debug/incremental/tarae-stale").exists(),
        "the stale cache went, its link as itself: {p:?}"
    );
    assert!(
        d.join("debug/deps/gone-3333333333333333.a.b.rcgu.o").symlink_metadata().is_ok(),
        "a link isn't a file"
    );
    assert!(std::fs::symlink_metadata(d.join("debug/incremental/tarae-linked")).is_ok());
    // A folder removed whole with a link inside it: the folder goes, the marker stays
    let e = folder(&h, "cd/00000000000006");
    symlink(&m, e.join("debug/deps/in")).unwrap();
    symlink(&m, e.join("debug/elsewhere")).unwrap();
    for f in [&a, &b, &c, &d, &e] {
        untouched(f, 3 * DAY);
    }
    let p = pass_with(true);
    intact(&m);
    assert!(!e.exists() && p.folders >= 1, "{p:?}");
    assert!(std::fs::symlink_metadata(h.build.join("ab/00000000000005")).is_ok(), "the link left as it is");
}

/// Review #168: a retake that fails loses the hold for good — a build that takes the lock between two removals
/// sees nothing removed after, in that step or the next (the cache stays), and the profile is counted busy.
#[test]
fn a_retake_that_fails_removes_nothing_more() {
    let h = home("buildprune-lost");
    let b = folder(&h, "ab/cdef0123");
    for i in 0..4 {
        let o = b.join(format!("debug/deps/gone-{i}333333333333333.x.y.rcgu.o"));
        write(&o, 64);
        aged(&o, Duration::from_secs(3600));
    }
    write(&b.join("debug/incremental/tarae-old/s-1/a.o"), 64);
    for p in [
        "debug/incremental/tarae-old/s-1/a.o",
        "debug/incremental/tarae-old/s-1",
        "debug/incremental/tarae-old",
    ] {
        aged(&b.join(p), 3 * DAY);
    }
    // (let go before every removal; the second time, a build takes the lock and keeps it)
    let build: std::rc::Rc<std::cell::RefCell<Option<File>>> = Default::default();
    let (taken, calls) = (build.clone(), std::rc::Rc::new(std::cell::Cell::new(0)));
    SLICE_FOR.set(Some(Duration::ZERO));
    BETWEEN.with(|f| {
        *f.borrow_mut() = Some(Box::new(move |lock: &Path| {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                let file = File::open(lock).unwrap();
                file.lock().unwrap();
                *taken.borrow_mut() = Some(file);
            }
        }))
    });
    let p = prune(&h, &opts(Some(DAY), false, Ok(Vec::new())));
    BETWEEN.with(|f| *f.borrow_mut() = None);
    SLICE_FOR.set(None);
    assert!(build.borrow().is_some(), "the build took it");
    assert_eq!((p.objects, p.caches, p.busy), (1, 0, 1), "one removed before the build took the lock: {p:?}");
    assert!(b.join("debug/incremental/tarae-old").exists());
}

/// Review #168: a profile with no lock file isn't the layout it knows — a folder no checkout builds in keeps
/// it whole, and says why; never dropped in silence.
#[test]
fn a_profile_with_no_lock_file_keeps_its_folder_whole_and_says_why() {
    let h = home("buildprune-lockless");
    let b = folder(&h, "ab/00000000000001");
    std::fs::create_dir_all(b.join("shot/deps")).unwrap();
    aged(&b.join("shot/deps"), 3 * DAY);
    aged(&b.join("shot"), 3 * DAY);
    untouched(&b, 3 * DAY);
    let p = prune(&h, &opts(Some(DAY), true, Ok(Vec::new())));
    assert!(b.exists());
    assert_eq!(p.folders, 0);
    assert!(p.left.iter().any(|l| l.contains("shot has no lock file")), "{p:?}");
}

/// Review #168: a folder removed whole is renamed aside while every lock is held, the locks let go before it's
/// removed (a build starting there waits only for the rename) — nothing of it left; one an earlier pass left
/// aside is finished.
#[test]
fn a_folder_removed_whole_is_set_aside_and_its_locks_let_go_first() {
    let h = home("buildprune-aside");
    let b = folder(&h, "ab/00000000000001");
    write(&b.join("debug/deps/x-1111111111111111"), 4096);
    untouched(&b, 3 * DAY);
    let free: std::rc::Rc<std::cell::Cell<Option<bool>>> = Default::default();
    let seen = free.clone();
    ASIDE_THEN.with(|f| {
        *f.borrow_mut() = Some(Box::new(move |aside: &Path| {
            let lock = File::open(aside.join("debug/.cargo-build-lock")).unwrap();
            seen.set(Some(lock.try_lock().is_ok()));
        }))
    });
    let p = prune(&h, &opts(Some(DAY), true, Ok(Vec::new())));
    ASIDE_THEN.with(|f| *f.borrow_mut() = None);
    assert_eq!(free.get(), Some(true), "set aside, its locks already let go");
    assert_eq!(p.folders, 1, "{p:?}");
    assert!(names(&h.build.join("ab")).is_empty(), "nothing set aside left");
    // An earlier pass's, interrupted
    write(&h.build.join("ab/.xtask-pruning-1-0/debug/deps/x"), 4096);
    prune(&h, &opts(None, false, Ok(Vec::new())));
    assert!(!h.build.join("ab/.xtask-pruning-1-0").exists());
}

/// The place is a plain folder: not a link itself, not `/`, not the home folder nor above it.
#[test]
fn the_place_is_a_plain_folder_below_the_home() {
    let dir = Scratch::new("buildprune-place");
    let (user, elsewhere) = (dir.join("user"), dir.join("elsewhere"));
    std::fs::create_dir_all(user.join(".cargo/build")).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, dir.join("linked")).unwrap();
    let real = std::fs::canonicalize(user.join(".cargo/build")).unwrap();
    assert_eq!(place(&user.join(".cargo/build"), Some(&user)), Ok(real));
    assert!(place(&dir.join("linked"), Some(&user)).unwrap_err().contains("is a link"));
    assert!(place(&user, Some(&user)).unwrap_err().contains("holds the home folder"));
    assert!(place(dir.path(), Some(&user)).unwrap_err().contains("holds the home folder"), "above it");
    assert!(place(Path::new("/"), None).unwrap_err().contains("holds the home folder"));
    assert!(place(&dir.join("none"), None).is_err());
}

/// The flags: lists unless `--yes`; `--whole` only with an age named, `--others` only with `--whole`.
#[test]
fn the_flags_read_and_whole_needs_an_age() {
    let read = |a: &[&str]| Args::read(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(read(&[]), Ok(Args::default()));
    assert_eq!(
        read(&["--age", "2d", "--whole", "--others", "--yes", "--in", "/b"]),
        Ok(Args { age: Some(2 * DAY), whole: true, others: true, yes: true, at: Some(PathBuf::from("/b")) })
    );
    assert!(read(&["--others"]).unwrap_err().contains("--others goes with --whole"));
    assert!(read(&["--age", "2d", "--others"]).unwrap_err().contains("--others goes with --whole"));
    assert_eq!(read(&["--age", "30m"]).unwrap().age, Some(Duration::from_secs(1800)));
    assert_eq!(read(&["--age", "6h"]).unwrap().age, Some(Duration::from_secs(6 * 3600)));
    assert!(read(&["--whole"]).unwrap_err().contains("--whole needs --age"));
    for bad in [&["--age"][..], &["--age", "2w"], &["--age", "d"], &["--all"]] {
        assert!(read(bad).is_err(), "{bad:?}");
    }
}

/// Review #168 (1): a project's root under `--in` — one named as cargo names a build folder but holding a `.git`,
/// and one named as a project is (`org/proj`, whose `target/debug` looks like a triple's profile) — is never
/// removed whole, each said; a real build folder beside them goes.
#[test]
fn a_projects_root_under_the_place_is_never_removed_whole() {
    let h = home("buildprune-project");
    let hexed = folder(&h, "ab/00000000000001");
    write(&hexed.join(".git/HEAD"), 64);
    let proj = h.build.join("org/proj");
    std::fs::create_dir_all(proj.join("target/debug/deps")).unwrap();
    std::fs::create_dir_all(proj.join("target/debug/.fingerprint/tarae-1111111111111111")).unwrap();
    File::create(proj.join("target/debug/.cargo-build-lock")).unwrap();
    write(&proj.join(".git/HEAD"), 64);
    let real = folder(&h, "cd/00000000000002");
    // (and one whose project file is a folder down: `debug/Cargo.toml`)
    let lower = folder(&h, "ef/00000000000003");
    write(&lower.join("debug/Cargo.toml"), 64);
    for d in [".git", ""] {
        aged(&hexed.join(d), 3 * DAY);
    }
    untouched(&hexed, 3 * DAY);
    aged(&lower.join("debug/Cargo.toml"), 3 * DAY);
    untouched(&lower, 3 * DAY);
    for d in ["target/debug/deps", "target/debug/.fingerprint", "target/debug", "target", ".git", ""] {
        aged(&proj.join(d), 3 * DAY);
    }
    untouched(&real, 3 * DAY);
    let p = prune(&h, &opts(Some(DAY), true, Ok(Vec::new())));
    assert!(hexed.join(".git/HEAD").exists() && proj.join(".git/HEAD").exists(), "{p:?}");
    assert!(!real.exists(), "the build folder went: {p:?}");
    assert_eq!(p.folders, 1, "{p:?}");
    assert!(
        p.left.iter().any(|l| l.contains("00000000000001: holds .git, which cargo never makes")),
        "{p:?}"
    );
    assert!(p.left.iter().any(|l| l.contains("org/proj: not named as cargo names a build folder")), "{p:?}");
    assert!(lower.join("debug/Cargo.toml").exists(), "{p:?}");
    assert!(p.left.iter().any(|l| l.contains("00000000000003: holds debug/Cargo.toml")), "{p:?}");
}

/// Review #168 (2): the global build-dir holds every project's folders, none of them a checkout here — one that
/// built none of this repository's packages is kept whole and said so, unless `--others`.
#[test]
fn another_projects_folder_goes_whole_only_with_others() {
    let h = home("buildprune-others");
    let theirs = other(&h, "ab/00000000000001");
    untouched(&theirs, 3 * DAY);
    let p = prune(&h, &opts(Some(DAY), true, Ok(Vec::new())));
    assert!(theirs.exists());
    assert_eq!(p.folders, 0, "{p:?}");
    let text = report(&p, true, None);
    assert!(
        text.contains("kept whole") && text.contains("another project's, as far as this can tell"),
        "{text}"
    );
    assert!(text.contains("(--others removes it)"), "{text}");
    let p = prune(&h, &Opts { others: true, ..opts(Some(DAY), true, Ok(Vec::new())) });
    assert!(!theirs.exists());
    assert_eq!(p.folders, 1, "{p:?}");
    assert!(report(&p, true, None).contains("no checkout of this repository builds here; another project's"));
}

/// Review #168 (3): what won't go — a folder an earlier pass set aside, a stale cache — is said with its error,
/// never dropped: here each deeper than a removal goes (as root too, unlike a mode).
#[test]
fn what_wont_go_is_said() {
    let h = home("buildprune-wont");
    let b = folder(&h, "ab/cdef0123");
    let deep = |at: PathBuf| {
        let leaf = (0..TREE_DEPTH + 1).fold(at, |p, _| p.join("d"));
        write(&leaf.join("f"), 64);
    };
    deep(h.build.join("ab/.xtask-pruning-1-0"));
    deep(b.join("debug/incremental/tarae-old"));
    aged(&b.join("debug/incremental/tarae-old/d"), 3 * DAY);
    aged(&b.join("debug/incremental/tarae-old"), 3 * DAY);
    let p = prune(&h, &opts(Some(DAY), false, Ok(Vec::new())));
    assert!(
        p.left.iter().any(|l| l.contains(".xtask-pruning-1-0: set aside by an earlier pass, won't go")
            && l.contains("deeper than")),
        "{p:?}"
    );
    assert!(p.left.iter().any(|l| l.contains("tarae-old: won't go") && l.contains("deeper than")), "{p:?}");
    assert!(report(&p, true, None).contains("left: "), "in the report");
}

/// git in a stand-in repository of the test's own: its own name, no signing, no hooks, none of the machine's config.
fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap();
    assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
}

/// Review #168 (2): `claimed` on a stand-in repository (never this one) — each checkout's folders as cargo says
/// them, the repository's own package; a checkout that doesn't say is the answer.
#[test]
fn claimed_says_where_each_checkout_builds_and_its_own_package() {
    let dir = Scratch::new("buildprune-claimed");
    let repo = dir.join("repo");
    let (t, b) = (dir.join("t"), dir.join("b"));
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::create_dir_all(repo.join(".cargo")).unwrap();
    std::fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"standin-pkg\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(repo.join("src/lib.rs"), "").unwrap();
    let config = format!("[build]\ntarget-dir = {:?}\nbuild-dir = {:?}\n", t.display(), b.display());
    std::fs::write(repo.join(".cargo/config.toml"), config).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "stand-in"]);
    git(&repo, &["worktree", "add", "-q", "-b", "second", "../second"]);
    let c = claimed(&repo).expect("both said");
    assert_eq!(c.own, ["standin-pkg"]);
    assert_eq!(c.dirs, [b.clone(), t.clone()], "both checkouts build there");
    // A checkout that doesn't say: none can be told unclaimed
    std::fs::write(dir.join("second/Cargo.toml"), "not a manifest [").unwrap();
    assert_eq!(claimed(&repo), Err(dir.join("second")));
}

#[test]
fn what_a_pass_did_reads_in_a_few_words() {
    let p = Pruned { freed: 9 << 30, objects: 7334, binaries: 1, busy: 2, ..Pruned::default() };
    assert_eq!(p.said().as_deref(), Some("9.0 GB — 7,334 old objects, 1 old test binary"));
    assert_eq!(Pruned { busy: 2, ..Pruned::default() }.said(), None);
    assert_eq!(report(&Pruned::default(), false, None), "Nothing old to prune\n");
}
