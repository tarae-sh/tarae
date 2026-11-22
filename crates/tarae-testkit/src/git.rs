//! Tests only: a `git` command isolated from the user's global/system config and from any surrounding
//! repository's environment (`TEST_GIT_REMOVES`), shared across workspace crates.

use std::path::Path;
use std::process::Command;

/// Tests: what git takes from the environment as a repository's own — another repository (`GIT_DIR`,
/// `GIT_WORK_TREE`: tarae's own, under `git rebase -x 'cargo test'` or a hook) or config above every file
/// (`GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_COUNT`: `git -c …`). `git rev-parse --local-env-vars`'s list (git 2.54),
/// which git itself clears entering a submodule — and the environment's template folder.
pub const TEST_GIT_REMOVES: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
    // (not a repository's: `init.templateDir`'s stand-in, a user's hooks copied into a fixture)
    "GIT_TEMPLATE_DIR",
];

/// Tests only: git a test starts itself, in `dir`, reading no global config at all — `/dev/null`, no system config,
/// none of the machine's ignore and attributes, and none from the environment: no `git -c`'s, no other repository
/// (`TEST_GIT_REMOVES`) — so the user's `safe.bareRepository`, hooks or fsmonitor reach no test (#678), and a test
/// run from inside git (a hook, `rebase -x`) works in `dir`, not git's repository. Not the test's own either: a
/// global a test registers (`global_config_for`) stands in for a hostile user's, which only tarae's git under test
/// (`base`) may meet — a set-up or a query running its filters would change what the test measures. A test that means
/// its own git to see one names the file on that one command (`.env("GIT_CONFIG_GLOBAL", file)`). Every git a test
/// starts goes through here (`every_git_a_test_starts_goes_through_test_git`).
pub fn test_git(dir: impl AsRef<Path>) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir.as_ref());
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null");
    cmd.env("GIT_CONFIG_NOSYSTEM", "1").env("XDG_CONFIG_HOME", "/dev/null");
    for name in TEST_GIT_REMOVES {
        cmd.env_remove(name);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::test_git;

    /// `test_git` takes nothing of git's from the test's environment: each of a repository's own names removed (a
    /// test run under `git -c …`, a hook or `rebase -x` would hand it config above every file, or tarae's own
    /// repository), and the three that shut out the machine's config set.
    #[test]
    fn test_git_takes_no_repository_or_config_from_the_environment() {
        let git = test_git("/");
        let envs: std::collections::HashMap<_, _> = git.get_envs().collect();
        // (`git rev-parse --local-env-vars`, git 2.54, and the template folder `fixture_git` kept out)
        let removed = "GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_CONFIG GIT_CONFIG_PARAMETERS GIT_CONFIG_COUNT \
                       GIT_OBJECT_DIRECTORY GIT_DIR GIT_WORK_TREE GIT_IMPLICIT_WORK_TREE GIT_GRAFT_FILE GIT_INDEX_FILE \
                       GIT_NO_REPLACE_OBJECTS GIT_REPLACE_REF_BASE GIT_PREFIX GIT_SHALLOW_FILE GIT_COMMON_DIR \
                       GIT_TEMPLATE_DIR";
        for name in removed.split_whitespace() {
            assert_eq!(envs.get(std::ffi::OsStr::new(name)), Some(&None), "{name}: not removed");
        }
        for (name, value) in [
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("XDG_CONFIG_HOME", "/dev/null"),
        ] {
            assert_eq!(
                envs.get(std::ffi::OsStr::new(name)),
                Some(&Some(std::ffi::OsStr::new(value))),
                "{name}"
            );
        }
    }
}
