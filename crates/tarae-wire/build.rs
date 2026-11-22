//! Stamps the build (`TARAE_BUILD`, read as `tarae_wire::BUILD`): the package's version, the commit its sources are
//! at and a `+` where they differ from it — the remote plan's S-C: a `Hello` and a `Welcome` tell it, a refusal and the
//! client's log say it, nothing checks it (the protocol's `V` is the rule). Outside a git checkout, in an unborn
//! repository, or with no git, the version alone.
//!
//! "Its sources" are what goes into the binary (`SOURCES`, from the workspace's root): a change elsewhere (docs, the
//! tests) is no `+`. It runs again as those change (relative paths: this checkout's) and as HEAD moves (its git
//! folder's existing files via `stamp::inspect`, absolute: another checkout building in the shared build-dir has its
//! own) — git's own reads only, no lock taken (`GIT_OPTIONAL_LOCKS=0`), ~30 ms.

#[path = "src/stamp.rs"]
mod stamp;

use std::path::PathBuf;
use std::process::Command;

/// What the binary is built from, below the workspace's root.
const SOURCES: &[&str] = &["src", "crates", "runtime", "build.rs", "Cargo.toml", "Cargo.lock"];

fn main() {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("../..");
    for s in SOURCES {
        println!("cargo::rerun-if-changed=../../{s}");
    }
    let found = stamp::inspect(&root, SOURCES, |args| {
        let out = Command::new("git")
            .args(["-c", "core.fsmonitor=", "--no-optional-locks"])
            .args(args)
            .current_dir(&root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    });
    let build = match found {
        Some(s) => {
            for p in &s.watches {
                println!("cargo::rerun-if-changed={}", p.display());
            }
            match s.suffix {
                Some(suffix) => format!("{version} {suffix}"),
                None => version,
            }
        }
        None => version,
    };
    println!("cargo::rustc-env=TARAE_BUILD={build}");
}
