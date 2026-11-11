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

/// The take's state and data homes: made new in its home, 0700 and empty, and the two variables naming them — a data
/// home of yours would hold the grammars you downloaded, and the take shows the first one's card.
#[test]
fn the_take_has_a_state_and_a_data_home_of_its_own() {
    use std::os::unix::fs::MetadataExt;
    let dir = Scratch::new("xtask-take-homes");
    let env = super::super::take::homes_of_its_own(&dir).unwrap();
    let names: Vec<&str> = env.iter().map(|(k, _)| *k).collect();
    assert_eq!(names, ["XDG_STATE_HOME", "XDG_DATA_HOME"]);
    for ((_, v), name) in env.iter().zip(["state", "data"]) {
        assert_eq!(Path::new(v), dir.join(name));
        assert_eq!(std::fs::metadata(v).unwrap().mode() & 0o777, 0o700, "{name}");
        assert!(std::fs::read_dir(v).unwrap().next().is_none(), "{name} empty");
    }
    // (never one that's there: a home made new holds none)
    assert!(super::super::take::homes_of_its_own(&dir).is_err());
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
