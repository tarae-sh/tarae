//! The take's toolchain clone on a made-up home of the test's own (`Scratch::new`): stand-in files, never your rustup
//! or cargo, and a stand-in `rustup` that's never run.

use std::os::unix::fs::{PermissionsExt, symlink};

use tarae_testkit::scratch::Scratch;

use super::*;

fn put(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn exe(p: &Path) {
    put(p, "#!/bin/sh\nexit 1\n");
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A home with a rustup (a stable default, another toolchain, its downloads) and a cargo (its registry, its git, its
/// build folder, a token file, a config, rustup's proxies — links and a same file — and a program of its own).
fn a_home(dir: &Path) -> Yours {
    let home = dir.join("home");
    let (rustup, cargo) = (home.join(".rustup"), home.join(".cargo"));
    put(&rustup.join("settings.toml"), "default_toolchain = \"stable-test-host\"\nversion = \"12\"\n");
    put(
        &rustup.join("toolchains/stable-test-host/lib/rustlib/src/rust/library/core/src/lib.rs"),
        "//! core\n",
    );
    for tool in ["rustc", "cargo", "rustdoc", "rustfmt"] {
        exe(&rustup.join("toolchains/stable-test-host/bin").join(tool));
    }
    put(&rustup.join("toolchains/1.0.0-test-host/bin/rustc"), "");
    put(&rustup.join("downloads/part"), "");
    put(&rustup.join("update-hashes/stable-test-host"), "");
    put(&cargo.join("registry/src/index.crates.io-0/itoa-1.0.0/src/lib.rs"), "//! itoa\n");
    put(&cargo.join("git/db/x/HEAD"), "");
    put(&cargo.join("build/ab/cdef/x"), "");
    put(&cargo.join("credentials.toml"), "[registry]\ntoken = \"tarae-fake-token\"\n");
    put(&cargo.join("config.toml"), "[build]\nbuild-dir = \"/Users/u/.cargo/build\"\n");
    let bin = cargo.join("bin");
    exe(&bin.join("rustup"));
    symlink("rustup", bin.join("cargo")).unwrap();
    symlink(bin.join("rustup"), bin.join("rustc")).unwrap();
    std::fs::hard_link(bin.join("rustup"), bin.join("rustdoc")).unwrap();
    // (a proxy for a tool the toolchain doesn't hold; programs of yours, an old tarae among them)
    symlink("rustup", bin.join("rls")).unwrap();
    exe(&bin.join("just"));
    exe(&bin.join("tarae"));
    // (one of yours named as a tool the toolchain holds: a rustfmt installed by hand, not rustup)
    exe(&bin.join("rustfmt"));
    let path = std::env::join_paths([
        bin,
        rustup.join("toolchains/stable-test-host/bin"),
        PathBuf::from("/usr/bin"),
    ])
    .unwrap();
    Yours { rustup, cargo, home, path }
}

/// What's cloned: rustup's settings and the stable toolchain, cargo's registry and rustup's proxies for the tools the
/// toolchain holds — and what's not: another toolchain, rustup's downloads, cargo's token file, its config, git, build,
/// a proxy for a tool it lacks, and your programs (an old tarae). The variables: the two homes, rustup never
/// installing, and the proxies first on a `PATH` without rustup's folders, your cargo's `bin` last (there, rustup's
/// `~/.cargo/env` leaves the order alone). Yours untouched.
#[test]
fn the_takes_toolchain_is_your_stable_and_registry_cloned_and_nothing_else() {
    let dir = Scratch::new("xtask-take-rust");
    let yours = a_home(&dir);
    let to = dir.join("shared/halil-rust");
    let c = clone(&yours, &to).unwrap();

    assert_eq!(c.toolchain, "stable-test-host");
    for there in [
        "rustup/settings.toml",
        "rustup/toolchains/stable-test-host/lib/rustlib/src/rust/library/core/src/lib.rs",
        "rustup/toolchains/stable-test-host/bin/rustc",
        "cargo/registry/src/index.crates.io-0/itoa-1.0.0/src/lib.rs",
        "cargo/bin/rustup",
    ] {
        assert!(to.join(there).is_file(), "{there} cloned");
    }
    for not in [
        "rustup/toolchains/1.0.0-test-host",
        "rustup/downloads",
        "rustup/update-hashes",
        "cargo/credentials.toml",
        "cargo/config.toml",
        "cargo/git",
        "cargo/build",
        "cargo/bin/just",
        "cargo/bin/tarae",
        "cargo/bin/rls",
        "cargo/bin/rustfmt",
    ] {
        assert!(!to.join(not).exists(), "{not} left");
    }
    // (a link to rustup made again beside the clone, never back to yours; the same file cloned)
    for link in ["cargo", "rustc"] {
        assert_eq!(
            std::fs::read_link(to.join("cargo/bin").join(link)).unwrap(),
            Path::new("rustup"),
            "{link}"
        );
    }
    assert!(std::fs::symlink_metadata(to.join("cargo/bin/rustdoc")).unwrap().is_file());
    assert_eq!(c.proxies, ["rustup", "cargo", "rustc", "rustdoc"]);
    let names: Vec<&str> = c.env.iter().map(|(k, _)| *k).collect();
    assert_eq!(names, ["RUSTUP_HOME", "CARGO_HOME", "RUSTUP_AUTO_INSTALL", "PATH"]);
    assert_eq!(c.env[0].1, to.join("rustup").into_os_string());
    assert_eq!(c.env[1].1, to.join("cargo").into_os_string());
    assert_eq!(c.env[2].1, "0");
    let path: Vec<PathBuf> = std::env::split_paths(&c.env[3].1).collect();
    assert_eq!(path, [to.join("cargo/bin"), PathBuf::from("/usr/bin"), yours.cargo.join("bin")]);
    for tool in ["cargo", "rustc", "rustdoc"] {
        assert_eq!(which(tool, &c.env[3].1), Some(to.join("cargo/bin").join(tool)), "{tool}");
    }
    assert!(
        yours.cargo.join("bin/cargo").exists() && yours.rustup.join("toolchains/1.0.0-test-host").exists()
    );
}

/// #525: the clone `record rust-clone` made before the take is taken as it is — what it made, and the variables a clone
/// at the take's start would give — once: a take marks it taken. Refused where no rust-clone made it, or where it
/// holds anything else (Finder's `.DS_Store` aside).
#[test]
fn a_clone_made_before_the_take_is_taken_as_it_is_and_only_once() {
    let dir = Scratch::new("xtask-take-rust-again");
    let yours = a_home(&dir);
    let to = dir.join("shared/halil-rust");
    let made = clone(&yours, &to).unwrap();
    put(&to.join(".DS_Store"), "");
    let again = reuse(&yours, &to).unwrap();
    assert_eq!((&again.toolchain, &again.proxies, &again.env), (&made.toolchain, &made.proxies, &made.env));
    assert_eq!(again.took, Duration::ZERO);
    mark_taken(&to).unwrap();
    let e = reuse(&yours, &to).err().unwrap();
    assert!(e.contains("a take took it already"), "{e}");

    let bare = dir.join("shared/bare");
    std::fs::create_dir_all(bare.join("rustup")).unwrap();
    std::fs::create_dir_all(bare.join("cargo")).unwrap();
    let e = reuse(&yours, &bare).err().unwrap();
    assert!(e.contains("not a clone record rust-clone made"), "{e}");
    let used = dir.join("shared/used");
    clone(&yours, &used).unwrap();
    put(&used.join("halil/Cargo.toml"), "");
    let e = reuse(&yours, &used).err().unwrap();
    assert!(e.contains("holds .record-rust-clone cargo halil rustup"), "{e}");
}

/// Cargo found outside your home: no proxies, `PATH` as yours with your cargo's `bin` last.
#[test]
fn no_proxies_where_a_run_finds_cargo_outside_your_home() {
    let dir = Scratch::new("xtask-take-rust-path");
    let mut yours = a_home(&dir);
    let elsewhere = dir.join("opt/bin");
    exe(&elsewhere.join("cargo"));
    exe(&elsewhere.join("rustc"));
    let usr = PathBuf::from("/usr/bin");
    yours.path = std::env::join_paths([elsewhere.clone(), yours.cargo.join("bin"), usr.clone()]).unwrap();
    let to = dir.join("halil-rust");
    let c = clone(&yours, &to).unwrap();
    assert!(c.proxies.is_empty() && !to.join("cargo/bin").exists());
    let path = &c.env.iter().find(|(k, _)| *k == "PATH").unwrap().1;
    assert_eq!(std::env::split_paths(path).collect::<Vec<_>>(), [elsewhere, usr, yours.cargo.join("bin")]);
}

/// Refused with words, nothing made: a target off your homes' volume (`/dev` is a file system of its own on macOS and
/// Linux), and one that holds anything (nothing cloned into it).
#[test]
fn a_clone_is_refused_off_your_volume_and_into_a_folder_that_holds_anything() {
    let dir = Scratch::new("xtask-take-rust-refused");
    let yours = a_home(&dir);
    let off = Path::new("/dev/tarae-xtask-take-rust");
    let e = clone(&yours, off).err().expect("refused");
    assert!(e.contains("isn't on the volume of") && e.contains("cp -c"), "{e}");
    assert!(!off.exists());
    let full = dir.join("full");
    put(&full.join("last-take"), "");
    let e = clone(&yours, &full).err().expect("refused");
    assert!(e.contains("not empty"), "{e}");
    assert!(!full.join("rustup").exists());
}

/// The take's tarae is named by its path: none given, a bare name (`PATH` would answer with an old install) and a
/// folder refused; a program's path taken, made absolute.
#[test]
fn the_takes_tarae_is_named_by_its_path_never_found_on_path() {
    let dir = Scratch::new("xtask-take-tarae");
    let bin = dir.join("target/release/tarae");
    exe(&bin);
    let take = |p: Option<&str>| super::super::take::tarae_bin(p.map(Path::new));
    let e = take(None).unwrap_err();
    assert!(e.contains("never one found on PATH"), "{e}");
    let e = take(Some("tarae")).unwrap_err();
    assert!(e.starts_with("tarae: a name, not a path"), "{e}");
    assert!(take(Some(dir.join("target").to_str().unwrap())).unwrap_err().contains("not a program"));
    assert_eq!(take(Some(bin.to_str().unwrap())).unwrap(), std::fs::canonicalize(&bin).unwrap());
}

/// The take's state, data and cache homes: the first two made new in its home, the cache where it's given (#519: the
/// preflight's base copy is there, and fmt prints its path), each 0700 and empty, and the variables naming them — a
/// data home of yours would hold the grammars you downloaded, and the take shows the first one's card.
#[test]
fn the_take_has_a_state_a_data_and_a_cache_home_of_its_own() {
    use std::os::unix::fs::MetadataExt;
    let dir = Scratch::new("xtask-take-homes");
    let (home, cache) = (dir.join("home"), dir.join("shared/halil-cache"));
    std::fs::create_dir(&home).unwrap();
    let env = super::super::take::homes_of_its_own(&home, &cache).unwrap();
    let names: Vec<&str> = env.iter().map(|(k, _)| *k).collect();
    assert_eq!(names, ["XDG_STATE_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"]);
    for ((_, v), at) in env.iter().zip([home.join("state"), home.join("data"), cache.clone()]) {
        assert_eq!(Path::new(v), at);
        assert_eq!(std::fs::metadata(v).unwrap().mode() & 0o777, 0o700, "{}", at.display());
        // (#610: the state holds one thing — the offer about git's bare repositories answered `no`, never on camera)
        let held: Vec<_> = std::fs::read_dir(v).unwrap().map(|e| e.unwrap().file_name()).collect();
        match at == home.join("state") {
            true => assert_eq!(held, ["tarae"], "{} holds the answer only", at.display()),
            false => assert!(held.is_empty(), "{} empty", at.display()),
        }
    }
    let seed = std::fs::read_to_string(home.join("state/tarae/safe-bare")).unwrap();
    assert!(seed.starts_with("no ") && seed.lines().count() == 1, "{seed:?}");
    // (never one that's there: a home made new holds none)
    assert!(super::super::take::homes_of_its_own(&home, &cache).is_err());
    // (a cache that holds anything — the last take's — refused, an empty one taken)
    let (again, used) = (dir.join("again"), dir.join("used"));
    std::fs::create_dir(&again).unwrap();
    std::fs::create_dir_all(used.join("tarae")).unwrap();
    let e = super::super::take::homes_of_its_own(&again, &used).unwrap_err();
    assert!(e.contains("not empty"), "{e}");
    // (the take's own: outside your home and the temp folders, where runs write)
    let take = Path::new(super::super::take::TAKE_CACHE);
    assert!(take.starts_with("/Users/Shared") && !take.starts_with(std::env::temp_dir()));
}

/// A take's folder in `/Users/Shared` (everyone's) — here a stand-in in the test's own: one absent made new, 0700; an
/// empty real folder of yours taken (made 0700); a link to an empty folder refused, the folder it points at left as it
/// was (`read_dir` would follow it); one that holds anything refused. Another user's can't be made without another
/// user, so the check is asked on a real folder's metadata with another uid as `me`.
#[test]
fn a_takes_folder_is_made_new_or_an_empty_real_folder_of_yours() {
    use super::super::take::{folder_for_a_take, homes_of_its_own, my_uid, real_and_yours};
    use std::os::unix::fs::MetadataExt;
    let dir = Scratch::new("xtask-take-folders");
    let shared = dir.join("shared");
    let mode = |p: &Path| std::fs::symlink_metadata(p).unwrap().mode() & 0o777;
    // (absent: made new, 0700)
    let new = shared.join("halil");
    folder_for_a_take(&new).unwrap();
    assert!(new.is_dir() && !new.is_symlink() && mode(&new) == 0o700);
    // (an empty folder you made, open to others: taken, and made 0700)
    let empty = shared.join("halil-checkouts");
    std::fs::create_dir(&empty).unwrap();
    std::fs::set_permissions(&empty, std::fs::Permissions::from_mode(0o777)).unwrap();
    folder_for_a_take(&empty).unwrap();
    assert_eq!(mode(&empty), 0o700);
    // (a link to an empty folder: refused, by the take's folders and its cache home alike; what it points at untouched)
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    std::fs::set_permissions(&elsewhere, std::fs::Permissions::from_mode(0o755)).unwrap();
    let link = shared.join("halil-cache");
    symlink(&elsewhere, &link).unwrap();
    let e = folder_for_a_take(&link).unwrap_err();
    assert!(e.contains("a link to") && e.contains("not a folder"), "{e}");
    let home = dir.join("home");
    std::fs::create_dir(&home).unwrap();
    let e = homes_of_its_own(&home, &link).unwrap_err();
    assert!(e.contains("a link to"), "{e}");
    assert_eq!(mode(&elsewhere), 0o755, "never made 0700 through the link");
    assert!(link.is_symlink());
    // (one that holds anything: refused)
    let full = shared.join("halil-rust");
    put(&full.join("last-take"), "");
    let e = folder_for_a_take(&full).unwrap_err();
    assert!(e.contains("not empty"), "{e}");
    // (a file: refused)
    let file = shared.join("a-file");
    put(&file, "");
    assert!(folder_for_a_take(&file).unwrap_err().contains("not a folder"));
    // (another user's: the check on a real folder's metadata, `me` another uid)
    let m = std::fs::symlink_metadata(&empty).unwrap();
    assert_eq!(m.uid(), my_uid());
    real_and_yours(&empty, &m, m.uid()).unwrap();
    let e = real_and_yours(&empty, &m, m.uid() + 1).unwrap_err();
    assert!(e.contains(&format!("owned by uid {}, not you", m.uid())), "{e}");
}

/// The clone `reuse` takes is a real folder of yours, others not writing in it: a link to one refused (the clone
/// behind it untouched), and one others may write in.
#[test]
fn a_clone_behind_a_link_or_open_to_others_is_never_taken() {
    let dir = Scratch::new("xtask-take-rust-link");
    let yours = a_home(&dir);
    let real = dir.join("elsewhere/halil-rust");
    clone(&yours, &real).unwrap();
    reuse(&yours, &real).unwrap();
    let link = dir.join("shared/halil-rust");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    symlink(&real, &link).unwrap();
    let e = reuse(&yours, &link).err().unwrap();
    assert!(e.contains("a link to") && e.contains("make it again"), "{e}");
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o777)).unwrap();
    let e = reuse(&yours, &real).err().unwrap();
    assert!(e.contains("mode 777, others may write in it"), "{e}");
}

/// The take's folder outside your real home (#521's follow-up: cargo reads `~/.cargo/config.toml` from the project's
/// folder up): one under it refused, also through a link into it and before it's there; the home itself too; one
/// beside it taken.
#[test]
fn the_takes_folder_is_refused_under_your_real_home() {
    use super::super::take::outside_home;
    let dir = Scratch::new("xtask-take-cwd-home");
    let home = dir.join("Users/u");
    std::fs::create_dir_all(home.join("Workspace")).unwrap();
    let e = outside_home(&home.join("Workspace/halil"), &home).unwrap_err();
    assert!(e.contains("under your home") && e.contains("cargo reads"), "{e}");
    assert!(outside_home(&home, &home).is_err());
    let link = dir.join("Users/Shared-link");
    symlink(home.join("Workspace"), &link).unwrap();
    let e = outside_home(&link.join("halil/deep"), &home).unwrap_err();
    assert!(e.contains("under your home"), "{e}");
    // (a home given through a link, the folder by its real path)
    let home_link = dir.join("home-link");
    symlink(&home, &home_link).unwrap();
    assert!(outside_home(&home.join("halil"), &home_link).is_err());
    // (beside it: taken — a name that starts as the home's does too)
    std::fs::create_dir_all(dir.join("Users/Shared")).unwrap();
    outside_home(&dir.join("Users/Shared/halil"), &home).unwrap();
    outside_home(&dir.join("Users/u-shared/halil"), &home).unwrap();
}

/// rustup's proxy's own variables and `cargo run`'s, set for `cargo xtask`, never reach the take's tarae: its
/// `RUSTUP_TOOLCHAIN` is tarae's pinned one, which the clone doesn't hold. The clone's own homes and `PATH` do, with
/// the clone's values over yours.
#[test]
fn rustups_toolchain_pin_is_left_out_of_the_take() {
    let os = |s: &str| std::ffi::OsString::from(s);
    let theirs = [
        "RUSTUP_TOOLCHAIN",
        "RUSTUP_TOOLCHAIN_SOURCE",
        "CARGO",
        "CARGO_PKG_NAME",
        "CARGO_MANIFEST_DIR",
        "CARGO_CRATE_NAME",
        "CARGO_BIN_NAME",
        "CARGO_PRIMARY_PACKAGE",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "CLAUDECODE",
    ];
    let caller = theirs
        .iter()
        .chain(&["RUSTUP_HOME", "CARGO_HOME", "PATH"])
        .map(|n| (os(n), os("yours")))
        .collect::<Vec<_>>();
    let set = ["RUSTUP_HOME", "CARGO_HOME", "PATH"].map(|n| (os(n), os("the clone's")));
    let (env, left) = super::super::take::takes_env(caller, &set);
    let mut theirs = theirs.map(String::from).to_vec();
    theirs.sort();
    assert_eq!(left, theirs);
    assert_eq!(env, set.to_vec());
}
