//! The warm start on a scratch repository: a stand-in for cargo (a script printing where a checkout builds and the
//! members it names), a cargo home of the test's own, a main checkout's `debug` made by hand — never the real cargo,
//! `~/.cargo` or a real build folder — and a stand-in clone (a copy that keeps times) wherever a copy is to land, so
//! the same runs on a file system that clones nothing.

use super::*;
use crate::scratch::Scratch;

/// Units by `<entry>` in their folder: (folder, entry). The hash is what ties them (`unit_of`).
pub(crate) const SERDE: &str = "aaaa1111aaaa1111";
const SERDE_BUILD: &str = "eeee5555eeee5555";
const MINE: &str = "bbbb2222bbbb2222";
const MINE_TEST: &str = "dddd4444dddd4444";
const MINE_BUILD: &str = "ffff6666ffff6666";
const PATHDEP: &str = "cccc3333cccc3333";

/// The hashes of the checkout's own packages' units (`mine`'s and the path dependency's): none may come over.
pub(crate) const OWN: [&str; 4] = [MINE, MINE_TEST, MINE_BUILD, PATHDEP];

/// The lock file: `mine` (the workspace's), `pathdep` (a path dependency outside it — no source either), `serde`.
const LOCK: &str = "version = 4\n\n[[package]]\nname = \"mine\"\nversion = \"0.1.0\"\ndependencies = [\"pathdep\", \"serde\"]\n\n\
                    [[package]]\nname = \"pathdep\"\nversion = \"0.1.0\"\n\n\
                    [[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";

/// The scratch repository (`sim/warm.rs` too).
pub(crate) struct Repo {
    pub(crate) dir: Scratch,
    pub(crate) home: PathBuf,
    pub(crate) builds: Builds,
}

impl Repo {
    /// A repository whose checkouts build inside themselves (`target/`, cargo's default) — or, `per_checkout`, in
    /// the cargo home's `build/<checkout's name>` with a home config saying it's one per checkout.
    pub(crate) fn new(name: &str, per_checkout: bool) -> Repo {
        let dir = Scratch::new(name);
        let (home, cargo) = (dir.join("home"), dir.join("cargo-home"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(cargo.join("build")).unwrap();
        let cargo = std::fs::canonicalize(&cargo).unwrap();
        let program = dir.join("fake-cargo");
        let build = match per_checkout {
            true => format!("{}/build/$(basename \"$top\")", cargo.display()),
            false => "$top/target".to_string(),
        };
        let script = format!(
            "top=\"$(pwd -P)\"\nprintf '{{\"target_directory\":\"%s/target\",\"build_directory\":\"%s\",\"packages\":[{{\"name\":\"mine\"}}]}}' \"$top\" \"{build}\"\n"
        );
        std::fs::write(&program, script).unwrap();
        // (the setting in the scratch's own `.cargo/config.toml`, above every checkout: the nearest wins over any
        // above the scratch — a home's own)
        let setting = match per_checkout {
            true => "[build]\nbuild-dir = \"{cargo-cache-home}/build/{workspace-path-hash}\"\n",
            false => "[build]\n",
        };
        std::fs::create_dir_all(dir.join(".cargo")).unwrap();
        std::fs::write(dir.join(".cargo/config.toml"), setting).unwrap();
        std::fs::write(home.join("Cargo.toml"), "[package]\nname = \"mine\"\n").unwrap();
        std::fs::write(home.join("Cargo.lock"), LOCK).unwrap();
        std::fs::write(home.join(".gitignore"), "target/\n").unwrap();
        let git = |args: &[&str]| crate::gitrepo::fixture_git(&home, args);
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "a"]);
        let home = std::fs::canonicalize(&home).unwrap();
        let builds = Builds::stand_in(&program, &cargo, &dir.join("user-home"));
        Repo { dir, home, builds }
    }

    /// A new checkout `name` (a session's), its files dated `old` — older than any unit (B3: a tool that keeps
    /// times), where cargo would trust a unit there by its time alone.
    pub(crate) fn checkout(&self, name: &str) -> PathBuf {
        let at = self.dir.join("wt").join(name);
        let at_s = at.to_str().unwrap();
        crate::gitrepo::fixture_git(&self.home, &["worktree", "add", "-q", "-b", name, at_s]);
        let old = std::time::UNIX_EPOCH + Duration::from_secs(946_684_800);
        for f in ["Cargo.toml", "Cargo.lock"] {
            std::fs::File::options().write(true).open(at.join(f)).unwrap().set_modified(old).unwrap();
        }
        std::fs::canonicalize(&at).unwrap()
    }

    /// Where checkout `top` builds, as the stand-in says.
    fn build_of(&self, top: &Path) -> PathBuf {
        let tool = self.builds.tool("rust");
        real_beyond(&self.builds.ask(tool, top).unwrap().1)
    }

    /// `top`'s `debug`, made by hand as a build of the home's would leave it: every unit with its fingerprint, its
    /// files' times now — newer than the new checkout's.
    pub(crate) fn built(&self, top: &Path) -> PathBuf {
        let debug = self.build_of(top).join("debug");
        let put = |rel: &str| {
            let p = debug.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, rel).unwrap();
        };
        put(".cargo-build-lock");
        for (pkg, h) in [
            ("serde", SERDE),
            ("serde", SERDE_BUILD),
            ("mine", MINE),
            ("mine", MINE_TEST),
            ("mine", MINE_BUILD),
        ] {
            put(&format!(".fingerprint/{pkg}-{h}/lib-{pkg}"));
        }
        // (a path dependency whose library has a name of its own: `[lib] name = "customlib"`)
        put(&format!(".fingerprint/pathdep-{PATHDEP}/lib-customlib"));
        for f in [
            format!("deps/libserde-{SERDE}.rlib"),
            format!("deps/libserde-{SERDE}.rmeta"),
            format!("deps/serde-{SERDE}.d"),
            format!("build/serde-{SERDE_BUILD}/out/x"),
            format!("deps/libmine-{MINE}.rlib"),
            format!("deps/mine-{MINE}.d"),
            format!("deps/mine-{MINE_TEST}"),
            format!("build/mine-{MINE_BUILD}/build-script-build"),
            format!("deps/libcustomlib-{PATHDEP}.rlib"),
            format!("deps/libcustomlib-{PATHDEP}.rmeta"),
        ] {
            put(&f);
        }
        std::fs::create_dir_all(debug.join("incremental/mine-1234")).unwrap();
        debug
    }
}

/// The stand-in clone: a copy, each file's time kept as `clonefile` keeps it.
fn copied(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type()?.is_dir() {
            copied(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
            let t = std::fs::metadata(&src)?.modified()?;
            std::fs::File::options().write(true).open(&dst)?.set_modified(t)?;
        }
    }
    Ok(())
}

/// A file system that clones nothing (Linux's ext4: `FICLONE` refused).
fn refused(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from_raw_os_error(libc::EOPNOTSUPP))
}

/// The entries in `debug`'s folders, sorted (`deps/libserde-….rlib`).
pub(crate) fn entries(debug: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for f in ["deps", "build", ".fingerprint", "incremental"] {
        for e in std::fs::read_dir(debug.join(f)).into_iter().flatten().flatten() {
            out.push(format!("{f}/{}", e.file_name().to_string_lossy()));
        }
    }
    crate::sorting::sort(&mut out);
    out
}

/// Nothing of the warm start's left beside the build folder: no staged copy.
fn no_staged(build: &Path) {
    let left = staged_in(build);
    assert!(left.is_empty(), "a staged copy left: {left:?}");
}

/// The staged copies in build folder `build`.
pub(crate) fn staged_in(build: &Path) -> Vec<String> {
    std::fs::read_dir(build)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(STAGED))
        .collect()
}

/// B3's hazard: the new checkout's files are older than the home's units, so a unit of its own packages that came
/// over would be used unbuilt — another checkout's binary. None comes: not the library, not the test binary, not the
/// build script, not their fingerprints; the dependencies do, their times as built; no `incremental/`.
#[test]
fn a_checkouts_own_units_never_come_over_even_older_files() {
    let r = Repo::new("warm-own", false);
    let home_debug = r.built(&r.home);
    let new = r.checkout("s1");
    let said = copy_with(&r.builds, &new, &copied).expect("a cargo project").expect("warm");
    assert!(said.contains("starts warm") && said.contains("its own packages build fresh"), "{said}");
    let debug = new.join("target/debug");
    let got = entries(&debug);
    let own: Vec<&String> =
        got.iter().filter(|e| [MINE, MINE_TEST, MINE_BUILD].iter().any(|h| e.contains(h))).collect();
    assert!(own.is_empty(), "another checkout's units came over — cargo would use them unbuilt: {own:?}");
    let want = [
        format!(".fingerprint/serde-{SERDE}"),
        format!(".fingerprint/serde-{SERDE_BUILD}"),
        format!("build/serde-{SERDE_BUILD}"),
        format!("deps/libserde-{SERDE}.rlib"),
        format!("deps/libserde-{SERDE}.rmeta"),
        format!("deps/serde-{SERDE}.d"),
    ];
    assert_eq!(got, want, "the dependency's units, and nothing else (no incremental/)");
    let time = |d: &Path| {
        std::fs::metadata(d.join(format!("deps/libserde-{SERDE}.rlib"))).unwrap().modified().unwrap()
    };
    assert_eq!(time(&debug), time(&home_debug), "a unit's time as it was built");
    assert!(entries(&home_debug).iter().any(|e| e.contains(MINE)), "the home's own units stay in the home");
    // (`target/` made as cargo makes one: its cache directory tag)
    assert!(
        std::fs::read_to_string(new.join("target/CACHEDIR.TAG")).is_ok_and(|t| t.starts_with("Signature: "))
    );
    no_staged(&new.join("target"));
}

/// A path dependency outside the workspace (no `source` in Cargo.lock, not a member the ask names) goes too — its
/// library named `customlib`, not after its package: found by the hash of its fingerprint, never by a name.
#[test]
fn a_path_dependency_with_a_library_name_of_its_own_is_left_out() {
    let r = Repo::new("warm-pathdep", false);
    r.built(&r.home);
    let new = r.checkout("s1");
    copy_with(&r.builds, &new, &copied).unwrap().unwrap();
    let got = entries(&new.join("target/debug"));
    let theirs: Vec<&String> = got.iter().filter(|e| e.contains(PATHDEP)).collect();
    assert!(theirs.is_empty(), "the path dependency's units came over: {theirs:?}");
}

/// The home building (cargo's lock held by another): nothing is copied — no profile, no staged copy — and it starts
/// cold, said; once it's let go the next one copies.
#[test]
fn nothing_is_copied_while_the_home_holds_its_lock() {
    use std::os::fd::AsRawFd as _;
    let r = Repo::new("warm-lock", false);
    let home_debug = r.built(&r.home);
    let new = r.checkout("s1");
    let held = std::fs::File::open(home_debug.join(".cargo-build-lock")).unwrap();
    // SAFETY: flock on a descriptor of the test's own, let go as it closes.
    assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
    let why = copy_with(&r.builds, &new, &copied).unwrap().unwrap_err();
    assert!(why.contains("starts cold") && why.contains("was building"), "{why}");
    assert!(!new.join("target/debug").exists(), "nothing copied while it builds");
    no_staged(&new.join("target"));
    drop(held);
    copy_with(&r.builds, &new, &copied).unwrap().expect("warm once it's let go");
}

/// A build that started first keeps its folder: nothing of the copy lands in it, nothing staged is left.
#[test]
fn a_profile_already_there_is_left_alone() {
    let r = Repo::new("warm-there", false);
    r.built(&r.home);
    let new = r.checkout("s1");
    std::fs::create_dir_all(new.join("target/debug/deps")).unwrap();
    std::fs::write(new.join("target/debug/deps/its-own"), "x").unwrap();
    let why = copy_with(&r.builds, &new, &copied).unwrap().unwrap_err();
    assert!(why.contains("started first"), "{why}");
    assert_eq!(entries(&new.join("target/debug")), ["deps/its-own"]);
    no_staged(&new.join("target"));
    // (one that lands between the look and the rename: the exclusive rename refuses it)
    let (a, b) = (r.dir.join("a"), r.dir.join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    assert_eq!(rename_new(&a, &b).unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
    assert!(a.exists(), "refused: nothing moved");
}

/// Never into the home (it has no home to start from but itself), and never from a session's checkout into another:
/// a session's units, where its runs write, are never the source.
#[test]
fn never_into_the_home_nor_from_a_session() {
    let r = Repo::new("warm-direction", false);
    r.built(&r.home);
    let why = copy_with(&r.builds, &r.home, &copied).unwrap().unwrap_err();
    assert!(why.contains("no main checkout"), "{why}");
    let s1 = r.checkout("s1");
    let s1_debug = r.built(&s1);
    std::fs::write(s1_debug.join("deps/libplanted-0123456701234567.rlib"), "a session's").unwrap();
    let s2 = r.checkout("s2");
    copy_with(&r.builds, &s2, &copied).unwrap().unwrap();
    let got = entries(&s2.join("target/debug"));
    assert!(!got.iter().any(|e| e.contains("planted")), "a session's unit came over: {got:?}");
    assert!(got.iter().any(|e| e.contains(SERDE)), "the home's: {got:?}");
}

/// A file system that clones nothing (Linux without `FICLONE`): nothing is copied — no profile, no staged copy — it
/// starts cold, said; and the create card says cold before it.
#[test]
fn a_file_system_that_clones_nothing_gets_nothing() {
    let r = Repo::new("warm-noclone", false);
    r.built(&r.home);
    let tool = r.builds.tool("rust");
    let path = r.dir.join("wt/s1");
    let until = crate::clock::now() + Duration::from_secs(5);
    assert_eq!(planned_with(&r.builds, tool, &r.home, &path, until, &|_| false), None, "the card: cold");
    let warm = planned_with(&r.builds, tool, &r.home, &path, until, &|_| true);
    assert!(
        matches!(warm, Some(Some(n)) if n > 0),
        "where it clones, the card says warm and what's shared: {warm:?}"
    );
    let new = r.checkout("s1");
    let why = copy_with(&r.builds, &new, &refused).unwrap().unwrap_err();
    assert!(why.contains("starts cold") && why.contains("clones nothing"), "{why}");
    assert!(!new.join("target/debug").exists(), "nothing copied");
    no_staged(&new.join("target"));
}

/// The card's shared size: the home's units but its own packages' — what a clone holds.
#[test]
fn the_card_measures_what_is_shared_without_the_checkouts_own() {
    let r = Repo::new("warm-size", false);
    let debug = r.built(&r.home);
    let tool = r.builds.tool("rust");
    let until = crate::clock::now() + Duration::from_secs(5);
    let path = r.dir.join("wt/s1");
    let Some(Some(shared)) = planned_with(&r.builds, tool, &r.home, &path, until, &|_| true) else {
        panic!()
    };
    let whole = crate::worktree::disk_size(&debug, until).0;
    let names = own_names(tool.warm().unwrap(), &[&r.home], &[]);
    assert!(names.contains("mine") && names.contains("pathdep") && !names.contains("serde"), "{names:?}");
    assert!(shared > 0 && shared < whole, "{shared} of {whole}");
    // (no home build: nothing to start from — cold)
    std::fs::remove_file(debug.join(".cargo-build-lock")).unwrap();
    assert_eq!(planned_with(&r.builds, tool, &r.home, &path, until, &|_| true), None);
}

/// A build folder one per checkout in the cargo home (`{workspace-path-hash}`): made as cargo marks a new one, the
/// dependencies cloned into it, the checkout's own left out. A shared one (no per-checkout setting) gets nothing.
#[test]
fn a_per_checkout_build_folder_in_the_cargo_home_starts_warm() {
    let r = Repo::new("warm-percheckout", true);
    r.built(&r.home);
    let new = r.checkout("s1");
    let build = r.build_of(&new);
    assert!(!build.starts_with(&new) && !build.exists(), "{}", build.display());
    copy_with(&r.builds, &new, &copied).unwrap().unwrap();
    assert!(std::fs::read_to_string(build.join("CACHEDIR.TAG")).is_ok_and(|t| t.starts_with("Signature: ")));
    let got = entries(&build.join("debug"));
    assert!(got.iter().any(|e| e.contains(SERDE)) && !got.iter().any(|e| e.contains(MINE)), "{got:?}");
    // (a fixed setting: the folder isn't its own — shared, one build at a time)
    std::fs::write(r.dir.join(".cargo/config.toml"), "[build]\nbuild-dir = \"/b/shared\"\n").unwrap();
    let s2 = r.checkout("s2");
    let why = copy_with(&r.builds, &s2, &copied).unwrap().unwrap_err();
    assert!(why.contains("shared"), "{why}");
    assert!(!r.build_of(&s2).exists());
}

/// A staged copy a gone tarae left goes with the next copy; one of a live process's, one not shaped as ours, and a
/// link shaped as ours stay.
#[test]
fn a_gone_taraes_staged_copy_goes_and_nothing_else() {
    let dir = Scratch::new("warm-staged");
    let build = dir.join("b");
    // (a pid no process has: past the system's limit)
    let gone = build.join("debug.tarae-warm-2147483646-3");
    let live = build.join(format!("debug.tarae-warm-{}-0", std::process::id()));
    let other = build.join("debug.tarae-warm-x-1");
    let parent = build.join(format!("debug.tarae-warm-{}-7", std::os::unix::process::parent_id()));
    for d in [&gone, &live, &other, &parent] {
        std::fs::create_dir_all(d.join("deps")).unwrap();
    }
    std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
    let link = build.join("debug.tarae-warm-2147483645-1");
    std::os::unix::fs::symlink(dir.join("elsewhere"), &link).unwrap();
    clear_staged(&build, "debug");
    assert!(!gone.exists(), "a gone tarae's");
    assert!(live.exists() && other.exists() && parent.exists(), "a live one's, one not ours");
    assert!(dir.join("elsewhere").exists(), "never through a link");
}

/// The rust pack says it all as data: the profile, the lock, the folders, how a package's units are found.
#[test]
fn the_rust_pack_says_how_a_warm_start_goes() {
    let b = Builds::stand_in(Path::new("/nonexistent"), Path::new("/nonexistent"), Path::new("/nonexistent"));
    let w = b.tool("rust").warm().expect("[builds.warm]");
    assert_eq!(w.profile, Path::new("debug"));
    assert_eq!(w.lock, ".cargo-build-lock");
    assert_eq!(w.folders, ["deps", "build", ".fingerprint"]);
    assert_eq!(w.units, ".fingerprint");
    assert_eq!(w.lock_file.0, "Cargo.lock");
    assert_eq!(unit_of("libtarae_wire-1780f9d1ab12cd34.rmeta"), Some(("libtarae_wire", "1780f9d1ab12cd34")));
    assert_eq!(unit_of("tarae-wire-0531ba2f00000000"), Some(("tarae-wire", "0531ba2f00000000")));
    assert_eq!(unit_of(".cargo-build-lock"), None);
}

/// The variable naming a folder on a file system that clones (Linux: btrfs, or XFS made with `-m reflink=1`) for
/// `the_real_clone_on_a_file_system_that_clones`.
const REFLINK_DIR: &str = "TARAE_TEST_REFLINK_DIR";

/// The real clone, rename and probe on a file system that clones, in the folder `TARAE_TEST_REFLINK_DIR` names —
/// skipped and said without it: CI's Docker is overlay or ext4, where only the refused path runs (on a VM with a
/// loop-mounted reflink XFS or btrfs, by hand). The probe says yes; a profile's tree is cloned, each file's bytes and
/// time as they were (`futimens`: one cloned now would read to cargo as rebuilt, and every unit after it); the
/// exclusive rename moves it where nothing is, and refuses where something is.
#[test]
fn the_real_clone_on_a_file_system_that_clones() {
    let test = "warm::tests::the_real_clone_on_a_file_system_that_clones";
    let Some(at) = std::env::var_os(REFLINK_DIR).filter(|v| !v.is_empty()) else {
        let why = format!("{REFLINK_DIR} names no folder on a file system that clones");
        return crate::test_results::said_skip(&format!("{test} — {why}"));
    };
    let dir = Scratch::in_dir(Path::new(&at), "warm-reflink");
    assert!(clones_in(&dir), "the probe: {} clones nothing", dir.display());
    let from = dir.join("from");
    let old = std::time::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    let files = [format!("deps/libserde-{SERDE}.rlib"), format!("build/serde-{SERDE_BUILD}/out/x")];
    for (n, f) in files.iter().enumerate() {
        let p = from.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, format!("unit {n}").repeat(4096)).unwrap();
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(old).unwrap();
    }
    let staged = dir.join("debug.staged");
    clone_tree(&from, &staged).expect("cloned");
    for (n, f) in files.iter().enumerate() {
        let p = staged.join(f);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), format!("unit {n}").repeat(4096), "{f}");
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), old, "{f}: its time as it was built");
    }
    let debug = dir.join("debug");
    rename_new(&staged, &debug).expect("where nothing is");
    assert!(!staged.exists() && debug.join(&files[0]).is_file());
    let again = dir.join("again");
    std::fs::create_dir(&again).unwrap();
    assert_eq!(rename_new(&again, &debug).unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
    assert!(again.exists() && debug.join(&files[0]).is_file(), "refused: nothing moved");
}

/// By hand, for measuring a warm start (the timing and the path check, `.sb/warm-measure/`): the real `copy` — the real
/// packs, cargo and cargo home, the repository's main checkout — into the checkout `TARAE_WARM_INTO` names, as a new
/// session's worker runs it; what it says printed. Never run with the suite: it writes a real build folder.
#[test]
#[ignore = "by hand: clones the main checkout's builds into the checkout TARAE_WARM_INTO names"]
fn copy_into_a_checkout_by_hand() {
    let into = std::env::var_os("TARAE_WARM_INTO").expect("TARAE_WARM_INTO names a new checkout");
    let t = Instant::now();
    let said = copy(&Builds::real(), Path::new(&into));
    println!("warm: {said:?} in {:.2} s", t.elapsed().as_secs_f64());
}

/// The real clone where it is one (APFS): a folder's files cloned, their times kept.
#[cfg(target_os = "macos")]
#[test]
fn clonefile_keeps_a_units_time() {
    let dir = Scratch::new("warm-clonefile");
    let from = dir.join("from");
    std::fs::create_dir_all(from.join("a")).unwrap();
    std::fs::write(from.join("a/f"), "x").unwrap();
    let old = std::time::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    std::fs::File::options().write(true).open(from.join("a/f")).unwrap().set_modified(old).unwrap();
    if !clones_in(&dir) {
        return;
    }
    clone_tree(&from, &dir.join("to")).unwrap();
    assert_eq!(std::fs::metadata(dir.join("to/a/f")).unwrap().modified().unwrap(), old);
}
