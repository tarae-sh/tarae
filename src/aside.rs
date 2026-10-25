//! Changes in no commit set aside while git moves a checkout — a move onto its base (`keepclose.rs`), the bring's
//! rebase (`rebase.rs`) — and put back after. Never git's `--autostash`: changes that don't go back it leaves in the
//! files as conflict markers, the checkout half moved, and its stash on top of `refs/stash` — every checkout's, so
//! whose entry is on top there is anyone's guess, and nothing here ever reads or pops it.
//!
//! - `take`: `git stash create` makes them a commit object on no ref — its tree the work tree's tracked files, its
//!   second parent the index, its first the commit they were made on (untracked and ignored files aren't in it and
//!   never touched) — kept at a ref of the checkout's own (`MOVE`, `REBASE`: `refs/worktree/` — no other checkout
//!   sees it, gc never takes it, a tarae gone midway leaves it there), made only where none is; then `reset --hard`,
//!   the tracked files as HEAD has them, for git to move.
//! - `back`: `stash apply --index` of that object — clean only if the work tree's changes and the index's both go;
//!   the ref dropped once they're back. One that doesn't go back still writes its markers: `put_back` or a reset
//!   follows.
//! - `put_back`: the checkout reset to the commit they were made on and applied there — clean by construction (made
//!   there): the checkout as it was, its index too.
//!
//! Whatever fails on the way, the changes stay at the ref and what's said names it (`kept_at`).

use std::path::Path;

/// Where a move onto its base keeps the checkout's changes while it runs.
pub const MOVE: &str = "refs/worktree/tarae/moving";
/// Where the bring's rebase keeps them, from its start to its end (a stop at a conflict, a restart between).
pub const REBASE: &str = "refs/worktree/tarae/rebasing";

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    crate::gitrepo::git_no_hooks(root, args)
}

fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// What's said when they stay set aside: where, the commit they were made on, and how to bring them back.
pub fn kept_at(root: &Path, at: &str, sha: &str) -> String {
    let on = git(root, &["rev-parse", "--short", &format!("{sha}^1")]).map(|s| s.trim().to_string());
    format!(
        "your changes in no commit are kept at {at} ({}, made on {}) — git stash apply --index {at} brings them \
         back, then git update-ref -d {at}",
        short(sha),
        on.as_deref().unwrap_or("its first parent"),
    )
}

fn log(what: &str) {
    crate::log::line("aside", "info", what);
}

/// The object kept at `at` there, if there is one.
pub fn kept(root: &Path, at: &str) -> Option<String> {
    let sha = git(root, &["rev-parse", "--quiet", "--verify", &format!("{at}^{{commit}}")]).ok()?;
    Some(sha.trim().to_string()).filter(|s| !s.is_empty())
}

/// The checkout's changes in no commit set aside at `at`, the tracked files then as HEAD has them: the object's id
/// — None when no tracked file differs from HEAD, nothing set aside. Refused while something is kept there already.
pub fn take(root: &Path, at: &str) -> Result<Option<String>, String> {
    if let Some(sha) = kept(root, at) {
        return Err(format!("changes set aside earlier are still there — {}", kept_at(root, at, &sha)));
    }
    let sha = git(root, &["stash", "create"])?.trim().to_string();
    if sha.is_empty() {
        return Ok(None);
    }
    // (`""`: only where no ref is — two takes never share one)
    git(root, &["update-ref", "-m", "tarae: set aside", at, &sha, ""])?;
    if let Err(e) = git(root, &["reset", "--quiet", "--hard"]) {
        return Err(format!("{e} — {}", kept_at(root, at, &sha)));
    }
    Ok(Some(sha))
}

/// The changes kept at `at` (`sha`) applied where the checkout is now, the ref dropped once they're back. An error:
/// they didn't go back cleanly (the files may hold markers now) or git failed — they're still at the ref.
pub fn back(root: &Path, at: &str, sha: &str) -> Result<(), String> {
    git(root, &["stash", "apply", "--index", "--quiet", sha])?;
    // (back: a ref left over would only refuse the next take, said)
    if let Err(e) = git(root, &["update-ref", "-d", at, sha]) {
        log(&format!("{at} not dropped after its changes went back: {e}"));
    }
    Ok(())
}

/// The checkout put back as it was when its changes were set aside (`sha`, at `at`): reset to the commit they were
/// made on — the branch with it — and the changes applied there.
pub fn put_back(root: &Path, at: &str, sha: &str) -> Result<(), String> {
    let said = |e: String| format!("{e} — {}", kept_at(root, at, sha));
    git(root, &["reset", "--quiet", "--hard", &format!("{sha}^1")]).map_err(said)?;
    back(root, at, sha).map_err(said)
}

/// The changes kept at `at` (`sha`) moved into git's stash as an entry of their own, `message` — never applied, never
/// popped — the ref dropped.
pub fn store(root: &Path, at: &str, sha: &str, message: &str) -> Result<(), String> {
    let said = |e: String| format!("{e} — {}", kept_at(root, at, sha));
    git(root, &["stash", "store", "--quiet", "-m", message, sha]).map_err(said)?;
    if let Err(e) = git(root, &["update-ref", "-d", at, sha]) {
        log(&format!("{at} not dropped after its changes were stashed: {e}"));
    }
    Ok(())
}
