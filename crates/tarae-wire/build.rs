//! Stamps the build (`TARAE_BUILD`, read as `tarae_wire::BUILD`): the package's version, the commit its sources are
//! at and a `+` where they differ from it — the remote plan's S-C: a `Hello` and a `Welcome` tell it, a refusal and the
//! client's log say it, nothing checks it (the protocol's `V` is the rule). Outside a git checkout, or with no git, the
//! version alone.
//!
//! "Its sources" are what goes into the binary (`SOURCES`, from the workspace's root): a change elsewhere (docs, the
//! tests) is no `+`. It runs again as those change (relative paths: this checkout's) and as HEAD moves (its git
//! folder's files, absolute: another checkout building in the shared build-dir has its own) — git's own reads only,
//! no lock taken (`GIT_OPTIONAL_LOCKS=0`), ~30 ms.

use std::path::{Path, PathBuf};
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
    let stamp = match stamp(&root) {
        Some(s) => format!("{version} {s}"),
        None => version,
    };
    println!("cargo::rustc-env=TARAE_BUILD={stamp}");
}

/// `<commit>` or `<commit>+`, HEAD's files watched — None: no git, or not a checkout.
fn stamp(root: &Path) -> Option<String> {
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(["-c", "core.fsmonitor=", "--no-optional-locks"])
            .args(args)
            .current_dir(root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let dir = git(&["rev-parse", "--absolute-git-dir"])?;
    let common = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]).unwrap_or(dir.clone());
    println!("cargo::rerun-if-changed={dir}/HEAD");
    println!("cargo::rerun-if-changed={common}/packed-refs");
    if let Some(r) = git(&["symbolic-ref", "-q", "HEAD"]) {
        println!("cargo::rerun-if-changed={common}/{r}");
    }
    let commit = git(&["rev-parse", "--short=8", "HEAD"])?;
    let mut status = vec!["status", "--porcelain", "--untracked-files=normal", "--"];
    status.extend(SOURCES);
    let changed = git(&status).is_none_or(|s| !s.is_empty());
    Some(if changed { format!("{commit}+") } else { commit })
}
