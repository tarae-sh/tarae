//! Where a project starts (`.sb/projects-plan.md`, user 2026-10-28): the nearest folder above the start holding
//! `.git` or a `.tarae.toml` — never home or above it; none found, the start folder itself. Helix's
//! `find_workspace` with `.tarae.toml` for its `.helix`: a worktree, a submodule or a checkout inside another is its
//! own project (its `.git` is nearest), and a `.tarae.toml` in a monorepo's subfolder keeps the project to it. Outside
//! git a `.tarae.toml` above the start makes that folder the project — it groups the folders under it (the user,
//! 2026-10-28: one in `~/Workspace/test` makes its three folders one project).
//!
//! **A mark counts only when it's yours** (tarae's reviewer, 2026-10-29): git's `safe.directory` owner rule — you
//! own the mark and the folder holding it — and nothing anyone can write (neither `o+w`), so no one else's
//! `/tmp/.git` or `/tmp/.tarae.toml` makes `/tmp` the project. A group's write is allowed, as git allows it: a
//! `git init --shared=group` repository is a project. One that fails is passed over and said once (`said`). So a
//! project file of yours narrows a checkout, or groups folders of yours; what it sets that runs or loosens a guard
//! waits for the trust card as ever (`trust.rs`).
//!
//! Every store keys by the project (`Workspace.root`): conversations, turns, restore, notes, layouts, grants. The
//! start folder stays where you are (`Workspace.start`): the paths you type are from it, the file picker looks there
//! first. Files only — no git runs here: `main` asks it once before the first frame.

use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

/// Who a mark must belong to: the process's user (`Me::now`) — a test passes another uid, the stand-in for a mark
/// someone else owns (never a real chown).
#[derive(Clone, Copy, Debug)]
pub struct Me {
    pub uid: u32,
}

impl Me {
    pub fn now() -> Me {
        Me { uid: unsafe { libc::getuid() } }
    }
}

/// The project found, and the marks passed over on the way — each with why (`said`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub root: PathBuf,
    pub skipped: Vec<(PathBuf, &'static str)>,
}

/// The project for the start folder `start` (a real path), with home `home` (a real path; None: no ceiling but `/`),
/// its marks `me`'s.
pub fn of_in(start: &Path, home: Option<&Path>, me: Me) -> Found {
    let mut skipped = Vec::new();
    let root = start
        .ancestors()
        // (home and what holds it are never a project's top: a dotfiles `~/.git` would take in every folder)
        .take_while(|a| home.is_none_or(|h| !h.starts_with(a)))
        .find(|a| match marks(a, me) {
            Mark::Yours => true,
            Mark::None => false,
            Mark::Not(at, why) => {
                skipped.push((at, why));
                false
            }
        })
        .unwrap_or(start)
        .to_path_buf();
    Found { root, skipped }
}

/// The project for `start`, with the process's home and user — links resolved in both.
pub fn of(start: &Path) -> Found {
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    of_in(&real(start), crate::allow::home().as_deref().map(real).as_deref(), Me::now())
}

/// The marks passed over, in a line for a warning — None: none.
pub fn said(skipped: &[(PathBuf, &'static str)]) -> Option<String> {
    let (at, why) = skipped.first()?;
    let more = match skipped.len() {
        1 => String::new(),
        n => format!(" (and {} more)", n - 1),
    };
    Some(format!("{} isn't taken as a project's mark — {why}{more}", crate::allow::tilde(at)))
}

enum Mark {
    /// No mark here.
    None,
    /// A mark, and yours.
    Yours,
    /// A mark that isn't yours: where, and why.
    Not(PathBuf, &'static str),
}

/// Whether folder `dir` is a project's top: a `.git` there (a folder, or a file naming a linked checkout — never a
/// link: `x/.git -> ../main/.git` would make x main's checkout, as `allow::common_dir` refuses it) or a `.tarae.toml`
/// — counted only when it and `dir` are `me`'s (`yours`).
fn marks(dir: &Path, me: Me) -> Mark {
    let git = dir.join(".git");
    let file = dir.join(".tarae.toml");
    let at = match std::fs::symlink_metadata(&git) {
        Ok(m) if m.is_dir() || m.is_file() => Some((git, m)),
        _ => std::fs::metadata(&file).ok().filter(std::fs::Metadata::is_file).map(|m| (file, m)),
    };
    let Some((at, m)) = at else { return Mark::None };
    match mine(&at, &m, me) {
        Ok(()) => Mark::Yours,
        Err(why) => Mark::Not(at, why),
    }
}

/// Whether the mark at `at` (its metadata `m`) and the folder holding it are `me`'s — owned by `me` (git's
/// `safe.directory` rule) and neither writable by anyone (`o+w`; a group's write allowed, as git's) — the one rule
/// for a project's mark and for the `.tarae.toml` read as its config (`config::project_found`). Err: why not.
pub fn mine(at: &Path, m: &std::fs::Metadata, me: Me) -> Result<(), &'static str> {
    yours(m, me)?;
    match at.parent().map(std::fs::metadata) {
        Some(Ok(d)) => {
            yours(&d, me).map_err(|why| if why == OTHER_OWNER { FOLDER_OTHER_OWNER } else { FOLDER_WRITABLE })
        }
        _ => Err(FOLDER_OTHER_OWNER),
    }
}

pub(crate) const OTHER_OWNER: &str = "someone else owns it";
pub(crate) const WRITABLE: &str = "anyone can write it";
const FOLDER_OTHER_OWNER: &str = "someone else owns its folder";
const FOLDER_WRITABLE: &str = "anyone can write its folder";

/// Whether what `m` says is `me`'s: owned by `me`, and not writable by anyone (`o+w`) — a group's write is allowed.
fn yours(m: &std::fs::Metadata, me: Me) -> Result<(), &'static str> {
    if m.uid() != me.uid {
        return Err(OTHER_OWNER);
    }
    if m.mode() & 0o002 != 0 { Err(WRITABLE) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(root: &Path, dirs: &[&str], files: &[&str]) {
        for d in dirs {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in files {
            std::fs::write(root.join(f), "").unwrap();
        }
    }

    #[test]
    fn the_nearest_git_or_project_file_above_the_start_is_the_project() {
        let s = crate::scratch::Scratch::new("project-of");
        let home = std::fs::canonicalize(&s).unwrap();
        let at = |p: &str| home.join(p);
        mk(
            &home,
            &[
                "repo/.git",
                "repo/src/deep",
                "repo/services/api/src",
                "repo/vendor/sub/lib",
                "loose/a/b",
                "wt/x",
            ],
            &["repo/services/api/.tarae.toml", "repo/vendor/sub/.git", "wt/.git"],
        );
        let of = |p: &str| of_in(&at(p), Some(&home), Me::now()).root;
        assert_eq!(of("repo"), at("repo"), "the top itself");
        assert_eq!(of("repo/src/deep"), at("repo"), "a subfolder: its checkout's top");
        assert_eq!(of("repo/services/api/src"), at("repo/services/api"), "a project file nearer narrows it");
        assert_eq!(
            of("repo/vendor/sub/lib"),
            at("repo/vendor/sub"),
            "a checkout inside another (`.git` a file)"
        );
        assert_eq!(of("wt/x"), at("wt"), "a linked checkout's `.git` file");
        assert_eq!(of("loose/a/b"), at("loose/a/b"), "none found: the start folder");
    }

    #[test]
    fn home_and_above_are_never_a_project_nor_a_link_named_git() {
        let s = crate::scratch::Scratch::new("project-home");
        let top = std::fs::canonicalize(&s).unwrap();
        let home = top.join("home");
        // (a dotfiles repository at home, a project file above it)
        mk(&top, &["home/.git", "home/work/a", "home/other/.git", "home/linked/x"], &[".tarae.toml"]);
        std::os::unix::fs::symlink(home.join("other/.git"), home.join("linked/.git")).unwrap();
        let of = |p: &Path| of_in(p, Some(&home), Me::now()).root;
        assert_eq!(
            of(&home.join("work/a")),
            home.join("work/a"),
            "not home's `.git`, not the file above home"
        );
        assert_eq!(of(&home), home, "started at home: home itself");
        assert_eq!(of(&home.join("linked/x")), home.join("linked/x"), "a `.git` that's a link marks nothing");
        assert_eq!(of_in(&home.join("work/a"), None, Me::now()).root, home, "no home known: no ceiling");
    }

    /// Outside git a project file of yours above the start groups the folders under it (the user's call); a mark that
    /// isn't yours — someone else owns it or its folder (a stand-in: another uid as `me`), or anyone can write it or
    /// its folder — is passed over, the climb going on, and said.
    #[test]
    fn a_mark_counts_only_when_its_yours() {
        use std::os::unix::fs::PermissionsExt as _;
        let s = crate::scratch::Scratch::new("project-owner");
        let home = std::fs::canonicalize(&s).unwrap();
        let at = |p: &str| home.join(p);
        mk(&home, &["test/wow", "test/palindrome", "shared/x", "open/.git", "open/y"], &["test/.tarae.toml"]);
        mk(&home, &[], &["shared/.tarae.toml"]);
        let me = Me::now();
        let of = |p: &str, me| of_in(&at(p), Some(&home), me);
        // (yours: grouped)
        assert_eq!(of("test/wow", me).root, at("test"));
        assert_eq!(of("test/palindrome", me), Found { root: at("test"), skipped: Vec::new() });
        // (another's: passed over, said)
        let other = Me { uid: me.uid.wrapping_add(1) };
        let f = of("test/wow", other);
        assert_eq!(f.root, at("test/wow"), "not grouped by another's file");
        assert_eq!(f.skipped, [(at("test/.tarae.toml"), OTHER_OWNER)]);
        let file = crate::allow::tilde(&at("test/.tarae.toml"));
        assert_eq!(
            said(&f.skipped).unwrap(),
            format!("{file} isn't taken as a project's mark — someone else owns it")
        );
        // (anyone can write the file, or the folder holding a `.git`)
        std::fs::set_permissions(at("shared/.tarae.toml"), std::fs::Permissions::from_mode(0o666)).unwrap();
        assert_eq!(of("shared/x", me).skipped, [(at("shared/.tarae.toml"), WRITABLE)]);
        assert_eq!(of("shared/x", me).root, at("shared/x"));
        std::fs::set_permissions(at("open"), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(
            of("open/y", me),
            Found { root: at("open/y"), skipped: vec![(at("open/.git"), FOLDER_WRITABLE)] }
        );
        std::fs::set_permissions(at("open"), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(of("open/y", me).root, at("open"), "put right: yours again");
        assert_eq!(said(&[]), None);
    }

    /// git's `safe.directory` looks only at the owner: a repository made with `git init --shared=group` (its `.git`
    /// g+w, whatever its group) is a project, started in its `src/` too; one anyone can write (`o+w`) isn't, and says
    /// so. (Modes only — never a chown nor a chgrp: the group is your own, git's rule ignores it either way.)
    #[test]
    fn a_group_may_write_a_mark_and_its_folder_but_not_everyone() {
        use std::os::unix::fs::PermissionsExt as _;
        let s = crate::scratch::Scratch::new("project-shared");
        let home = std::fs::canonicalize(&s).unwrap();
        let at = |p: &str| home.join(p);
        mk(&home, &["repo/.git", "repo/src"], &[]);
        let mode = |p: &str, m| std::fs::set_permissions(at(p), std::fs::Permissions::from_mode(m)).unwrap();
        let of = |p: &str| of_in(&at(p), Some(&home), Me::now());
        // (shared with a group: `--shared=group` makes `.git` 2775, its folder as the umask leaves it)
        mode("repo/.git", 0o2775);
        mode("repo", 0o775);
        assert_eq!(
            of("repo/src"),
            Found { root: at("repo"), skipped: Vec::new() },
            "group-writable: a project"
        );
        // (anyone can write the `.git`, or its folder)
        mode("repo/.git", 0o777);
        let f = of("repo/src");
        assert_eq!(f, Found { root: at("repo/src"), skipped: vec![(at("repo/.git"), WRITABLE)] });
        let git = crate::allow::tilde(&at("repo/.git"));
        assert_eq!(
            said(&f.skipped).unwrap(),
            format!("{git} isn't taken as a project's mark — anyone can write it")
        );
        mode("repo/.git", 0o775);
        mode("repo", 0o777);
        assert_eq!(of("repo/src").skipped, [(at("repo/.git"), FOLDER_WRITABLE)]);
        mode("repo", 0o700);
    }
}
