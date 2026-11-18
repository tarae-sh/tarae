//! The test binary's limit on open descriptors, raised as it starts — as tarae's `main` raises its own
//! (`nofile::raise`) before anything runs.
//!
//! The editor a test drives is tarae's, which runs with the raised limit; the test binary has no `main` of ours,
//! so its tests ran with whatever the shell gave: macOS's 256 (launchd), 1024 on a stock Linux (systemd), CI's
//! container a million. The bin's tests hold a few hundred at once on a machine with many cores (443 at 14 threads
//! on the Mac, 2026-11-19) — more than macOS's stock limit — and what they held past their end (#643: each
//! sandbox test's keeper, ~7 descriptors and a process, until the binary ended) took a Raspberry Pi past 1024.
//! That one is ended with its test now (`keeper::used_by_this_test`) and checked; this takes the shell's limit
//! out of what a test depends on (the environment kind of nondeterminism, CLAUDE.md's rules).
//!
//! Run by the loader before `main` (`.init_array`, `__mod_init_func`) — before libtest starts a thread or a
//! test, so every test has it and no test changes the process (CLAUDE.md's rule). Only the soft limit, to
//! `nofile::target` as `raise` would: nothing remembered (`nofile::original` stays None, so `restore` and the
//! shim give nothing back — the tests of those raise in a process of their own), nothing pinned.

/// Set on a test binary's process: its limit stays as given — the scenes that start from a low limit to watch
/// `nofile::raise` set it (and `perf_keepers_memory_per_session`'s "not raised" run).
pub const NO_RAISE: &str = "TARAE_TEST_NO_RAISE";

#[used]
#[cfg_attr(any(target_os = "linux", target_os = "android"), unsafe(link_section = ".init_array"))]
#[cfg_attr(target_os = "macos", unsafe(link_section = "__DATA,__mod_init_func"))]
static RAISE: extern "C" fn() = raise;

/// What `RAISE` runs, before `main`: no thread, no lock, one call to set it.
extern "C" fn raise() {
    if std::env::var_os(NO_RAISE).is_some() {
        return;
    }
    // (the test binary run as a session's keeper, `keeper_argv`: a keeper never raises — tarae's `main` raises
    // only as the editor — so its runs get the limit tarae started with)
    #[cfg(target_os = "linux")]
    if std::fs::read("/proc/self/cmdline")
        .is_ok_and(|c| c.split(|b| *b == 0).any(|a| a == b"sandbox::keeper::tests::keeper_process"))
    {
        return;
    }
    let l = crate::nofile::limit();
    if let Some(to) = crate::nofile::target(l.rlim_cur, l.rlim_max, crate::nofile::cap()) {
        let new = libc::rlimit { rlim_cur: to, rlim_max: l.rlim_max };
        // SAFETY: a limit of ours, no higher than the hard one; one raw call, before any thread.
        unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &new) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENE: &str = "TARAE_TESTLIMIT_SCENE";

    /// The test binary started under a soft limit of 1024 — a stock Linux's, the Pi's of #643 — has it raised
    /// before any test runs, as tarae's `main` would: a test sees the hard limit (capped), the shell's limit gone
    /// from what it depends on. With `NO_RAISE` set it stays 1024. Each in a process of its own (the limit is the
    /// process's).
    #[test]
    fn the_test_binary_starts_with_its_limit_raised() {
        let l = crate::nofile::limit();
        let Some(want) = crate::nofile::target(1024, l.rlim_max, crate::nofile::cap()) else {
            crate::test_results::said_skip(&format!(
                "testlimit::tests::the_test_binary_starts_with_its_limit_raised — nothing above 1024 to raise to \
                 here (hard limit {})",
                l.rlim_max
            ));
            return;
        };
        let seen = |no_raise: bool| {
            use std::os::unix::process::CommandExt as _;
            let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
            cmd.args(["--exact", "testlimit::tests::limit_scene", "--ignored", "--nocapture"])
                .args(["--test-threads=1"])
                .env(SCENE, "1")
                .env_remove(NO_RAISE);
            if no_raise {
                cmd.env(NO_RAISE, "1");
            }
            // SAFETY: getrlimit/setrlimit only, between fork and exec.
            unsafe {
                cmd.pre_exec(|| {
                    let mut l = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
                    libc::getrlimit(libc::RLIMIT_NOFILE, &mut l);
                    l.rlim_cur = 1024;
                    libc::setrlimit(libc::RLIMIT_NOFILE, &l);
                    Ok(())
                });
            }
            let out = cmd.output().unwrap();
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            let said = text.lines().find_map(|l| l.split_once("scene: soft=").map(|(_, s)| s.to_string()));
            said.unwrap_or_else(|| {
                panic!("the scene said nothing:\n{text}\n{}", String::from_utf8_lossy(&out.stderr))
            })
        };
        assert_eq!(seen(false), want.to_string(), "raised before the tests, to the hard limit (capped)");
        assert_eq!(seen(true), "1024", "left as given with {NO_RAISE}");
    }

    #[test]
    #[ignore = "run by the_test_binary_starts_with_its_limit_raised, in a process of its own"]
    fn limit_scene() {
        if std::env::var_os(SCENE).is_none() {
            return;
        }
        println!("scene: soft={}", crate::nofile::limit().rlim_cur);
    }
}
