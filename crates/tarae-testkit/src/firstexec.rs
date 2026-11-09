//! A fresh binary's first exec, run once and alone before the tests start it (#459). macOS checks a freshly linked,
//! ad-hoc-signed binary on its first exec, and each build is a new code hash with a check of its own. Landing in a
//! burst of parallel execs under load, that check failed closed: the kernel refused a test's exec (namespace 9,
//! `OS_REASON_EXEC`, code 8: security policy). Run alone, before this suite's own burst of execs, it passes (other
//! checkouts' builds may still load the machine), and the execs after it find its verdict. Nothing is retried: a
//! refused first exec fails every test that waited on it, its reason said.
//!
//! A binary already running needs none: a test binary re-running itself (`current_exe()`) passed its check when it
//! was started.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Runs `exec` (the binary's first exec, waited for) once for `bin` in this process, before any caller goes on:
/// every caller for `bin` waits until it has run, and a failure fails each of them with its words. Callers for
/// other binaries don't wait on it. `bin` as written.
pub fn alone(bin: &Path, exec: impl FnOnce(&Path) -> Result<(), String>) {
    static FIRSTS: Firsts = Firsts::new();
    FIRSTS.alone(bin, exec);
}

/// One binary's first exec: its result once it has run, every caller waiting on it until then.
type First = Arc<OnceLock<Result<(), String>>>;

/// The first execs run so far, by binary (`alone`'s; a test's own).
struct Firsts(Mutex<Option<HashMap<PathBuf, First>>>);

impl Firsts {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }

    fn alone(&self, bin: &Path, exec: impl FnOnce(&Path) -> Result<(), String>) {
        // (the map held only to find `bin`'s: one binary's first exec doesn't hold up another's)
        let once = {
            let mut firsts = self.0.lock().unwrap_or_else(|e| e.into_inner());
            firsts.get_or_insert_default().entry(bin.to_path_buf()).or_default().clone()
        };
        // (a result kept, not a `Once`: one that panicked would tell the others only that it's poisoned)
        if let Err(e) = once.get_or_init(|| exec(bin)) {
            panic!("{e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A binary's first exec runs once, before any caller goes on (#459): callers at once on many threads, a
    /// stand-in exec counted (no real exec). Another binary's runs once too, its own; a refused one fails each caller
    /// with its reason, and runs once.
    #[test]
    fn a_binarys_first_exec_runs_once_before_any_caller_goes_on() {
        let firsts = Firsts::new();
        let log = Mutex::new(Vec::new());
        let at_once = std::sync::Barrier::new(8);
        std::thread::scope(|s| {
            for k in 0..8 {
                let (firsts, log, at_once) = (&firsts, &log, &at_once);
                s.spawn(move || {
                    let bin = if k % 2 == 0 { "/fresh/a" } else { "/fresh/b" };
                    at_once.wait();
                    firsts.alone(Path::new(bin), |b| {
                        log.lock().unwrap().push(format!("first exec {}", b.display()));
                        Ok(())
                    });
                    log.lock().unwrap().push(format!("started {bin}"));
                });
            }
        });
        let log = log.into_inner().unwrap();
        for bin in ["/fresh/a", "/fresh/b"] {
            let at = |e: String| log.iter().position(|l| *l == e);
            let first = at(format!("first exec {bin}")).unwrap_or_else(|| panic!("{bin}'s ran: {log:?}"));
            let runs = log.iter().filter(|l| **l == format!("first exec {bin}")).count();
            assert_eq!(runs, 1, "{bin}: {log:?}");
            let started = log.iter().enumerate().filter(|(_, l)| **l == format!("started {bin}"));
            assert!(started.clone().all(|(i, _)| i > first), "each after {bin}'s first exec: {log:?}");
            assert_eq!(started.count(), 4, "{log:?}");
        }
        let execs = AtomicUsize::new(0);
        for _ in 0..2 {
            let failed = std::panic::catch_unwind(|| {
                firsts.alone(Path::new("/fresh/refused"), |_| {
                    execs.fetch_add(1, Ordering::Relaxed);
                    Err("ended by the kernel: 9 (namespace 9), code 0x8".into())
                })
            });
            let said = failed.expect_err("a refused first exec fails");
            let said = said.downcast_ref::<String>().map(String::as_str);
            assert_eq!(said, Some("ended by the kernel: 9 (namespace 9), code 0x8"));
        }
        assert_eq!(execs.load(Ordering::Relaxed), 1);
    }
}
