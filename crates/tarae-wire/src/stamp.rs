//! Build-stamp watch paths and git inspection (`build.rs`, #684 option D): std only, shared by `build.rs`
//! (via `#[path = "src/stamp.rs"]`) and the crate's tests so the rules are tested directly without running cargo.
//!
//! Cargo treats any missing `rerun-if-changed` path as dirty on every run, so every emitted path must exist on disk
//! while still catching any commit or checkout that moves `HEAD` (loose, packed, nested, unborn, detached, linked
//! worktree, or reftable).

use std::path::{Path, PathBuf};

/// What `inspect` found for a git checkout: existing paths for Cargo to watch, and the commit suffix (if any).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStamp {
    pub watches: Vec<PathBuf>,
    /// `None` in an unborn repository (0 commits); `Some("<sha>")` or `Some("<sha>+")` otherwise.
    pub suffix: Option<String>,
}

/// Existing git metadata paths to watch so Cargo reruns `build.rs` whenever `HEAD` moves, without watching any
/// missing file:
/// - **reftable**: `{dir}/reftable/tables.list` and `{common}/reftable/tables.list` (whichever exist, deduped).
/// - **`files` backend**: `{dir}/HEAD` if it exists, plus:
///   - symbolic `HEAD` with existing loose ref: that loose ref alone (loose shadows `packed-refs`, and packing
///     deletes it, triggering one rerun to switch to the packed branch);
///   - symbolic `HEAD` with missing loose ref (packed or unborn): `{common}/packed-refs` if it exists, plus the
///     deepest existing ancestor directory of the loose ref strictly below `common`;
///   - detached `HEAD`: `{dir}/HEAD` alone.
pub fn watch_paths(
    dir: &Path,
    common: &Path,
    symref: Option<&str>,
    exists: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let dir_tables = dir.join("reftable").join("tables.list");
    let common_tables = common.join("reftable").join("tables.list");
    let mut reftables = Vec::new();
    if exists(&dir_tables) {
        reftables.push(dir_tables.clone());
    }
    if common_tables != dir_tables && exists(&common_tables) {
        reftables.push(common_tables);
    }
    if !reftables.is_empty() {
        return reftables;
    }

    let mut out = Vec::new();
    let head = dir.join("HEAD");
    if exists(&head) {
        out.push(head);
    }

    if let Some(r) = symref.filter(|s| !s.is_empty()) {
        let loose = common.join(r);
        if exists(&loose) {
            out.push(loose);
        } else {
            let packed = common.join("packed-refs");
            if exists(&packed) {
                out.push(packed);
            }
            // Sibling-worktree transient: while this branch is packed or unborn, its loose ref does not exist yet,
            // so we watch its deepest existing ancestor directory below `common` (typically `{common}/refs/heads` or
            // `{common}/refs/heads/<prefix>`). A commit on a different branch in another worktree writes into that
            // shared directory and causes one extra rerun here. That is accepted: watching the ancestor is required
            // so a commit on *this* branch (creating the loose ref) does not freeze `TARAE_BUILD`, and the window is
            // transient — as soon as this branch has a loose ref, the watch narrows back to that single file.
            if let Some(ancestor) = loose
                .ancestors()
                .skip(1)
                .take_while(|a| *a != common && a.starts_with(common))
                .find(|a| exists(a))
            {
                out.push(ancestor.to_path_buf());
            }
        }
    }

    out
}

/// Inspects the git repository at `root` using `git`: returns `None` outside a git checkout (no watches, version
/// alone). Computes watches **before** `rev-parse --short=8 HEAD` so an unborn repository still watches for its first
/// commit while emitting `suffix: None` (version alone).
pub fn inspect(
    root: &Path,
    sources: &[&str],
    mut git: impl FnMut(&[&str]) -> Option<String>,
) -> Option<GitStamp> {
    let dir = git(&["rev-parse", "--absolute-git-dir"]).filter(|s| !s.is_empty())?;
    let common = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| dir.clone());
    let dir = root.join(dir);
    let common = root.join(common);
    let symref = git(&["symbolic-ref", "-q", "HEAD"]).filter(|s| !s.is_empty());
    let watches = watch_paths(&dir, &common, symref.as_deref(), |p| p.exists());

    let suffix = git(&["rev-parse", "--short=8", "HEAD"]).filter(|s| !s.is_empty()).map(|commit| {
        let mut status = vec!["status", "--porcelain", "--untracked-files=normal", "--"];
        status.extend_from_slice(sources);
        let changed = git(&status).is_none_or(|s| !s.is_empty());
        if changed { format!("{commit}+") } else { commit }
    });
    Some(GitStamp { watches, suffix })
}
