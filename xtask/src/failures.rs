//! `--keep-going`'s count (`cargo xtask linux test --repeat N --keep-going`, `cargo xtask kernels --keep-going`):
//! every run of N goes on past a failing one, and at the end each test that failed is said once, with the runs it
//! failed in — a whole run finds every race it meets, not the first one's alone.
//!
//! A run's failing tests are libtest's own list: the `failures:` block that names them, four spaces in, one a line,
//! before `test result: FAILED` (the other `failures:` block, of `---- name stdout ----` sections, names none). A run
//! that failed with no test named — a test binary that crashed, or the build — is said by its number.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    /// The runs begun, the one under way last.
    runs: u32,
    /// Each test that failed, with the runs (1-based) it failed in.
    tests: BTreeMap<String, BTreeSet<u32>>,
    /// The runs that failed.
    failed: BTreeSet<u32>,
    /// Inside a `failures:` block of the run under way.
    listing: bool,
}

impl Tally {
    /// A run begins: the lines from now on are its.
    pub(crate) fn run_starts(&mut self) {
        self.runs += 1;
        self.listing = false;
    }

    /// A line of the run under way's output (its end trimmed or not).
    pub(crate) fn line(&mut self, line: &str) {
        let line = line.trim_end();
        if line == "failures:" {
            self.listing = true;
            return;
        }
        if !self.listing {
            return;
        }
        match line.strip_prefix("    ").filter(|n| !n.is_empty() && !n.contains(char::is_whitespace)) {
            Some(name) if self.runs > 0 => {
                self.tests.entry(name.to_string()).or_default().insert(self.runs);
            }
            _ => self.listing = false,
        }
    }

    /// The run under way failed.
    pub(crate) fn run_failed(&mut self) {
        self.failed.insert(self.runs);
        self.listing = false;
    }

    pub(crate) fn any_failed(&self) -> bool {
        !self.failed.is_empty()
    }

    /// A line of a stream of runs (`xtask linux`'s loop): its marks begin a run (`run 2 of 5`) and fail it
    /// (`failed on run 2 of 5`), the rest are the run's.
    pub(crate) fn streamed(&mut self, line: &str) {
        let line = line.trim_end();
        let of = |rest: &str| {
            rest.split_once(" of ").is_some_and(|(a, b)| a.parse::<u32>().is_ok() && b.parse::<u32>().is_ok())
        };
        match line {
            l if l.strip_prefix("failed on run ").is_some_and(of) => self.run_failed(),
            l if l.strip_prefix("run ").is_some_and(of) => self.run_starts(),
            l => self.line(l),
        }
    }

    /// What's said at the end, of `of` runs: a head line, then each test that failed once, the most runs first —
    /// `  2 of 5  a::b  (runs 2, 4)` —, then the failed runs that named no test. Nothing when none failed.
    pub(crate) fn said(&self, of: u32) -> Vec<String> {
        if self.failed.is_empty() {
            return Vec::new();
        }
        let runs = |r: &BTreeSet<u32>| {
            let n: Vec<String> = r.iter().map(u32::to_string).collect();
            format!("{} {}", if n.len() == 1 { "run" } else { "runs" }, n.join(", "))
        };
        let mut tests: Vec<(&String, &BTreeSet<u32>)> = self.tests.iter().collect();
        tests.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
        let mut out = vec![format!(
            "failed in {} of {of} runs — {} {} failed, each once with the runs it failed in:",
            self.failed.len(),
            tests.len(),
            if tests.len() == 1 { "test" } else { "tests" }
        )];
        let w = tests.iter().map(|(n, _)| n.chars().count()).max().unwrap_or(0);
        for (name, r) in &tests {
            out.push(format!("  {} of {of}  {name:<w$}  ({})", r.len(), runs(r)));
        }
        let named: BTreeSet<u32> = self.tests.values().flatten().copied().collect();
        let blind: BTreeSet<u32> = self.failed.difference(&named).copied().collect();
        if !blind.is_empty() {
            out.push(format!(
                "  {} failed with no test named (a test binary that crashed, or the build — its output)",
                runs(&blind)
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// libtest's output, as `cargo test --no-fail-fast` prints a failing binary's: the stdout sections' block
    /// names nothing, the list after it does.
    fn failing(names: &[&str]) -> String {
        let sections: String =
            names.iter().map(|n| format!("---- {n} stdout ----\npanicked at x\n\n")).collect();
        let list: String = names.iter().map(|n| format!("    {n}\n")).collect();
        format!(
            "running 3 tests\ntest a::ok ... ok\n{}\nfailures:\n\n{sections}\nfailures:\n{list}\ntest result: FAILED. 1 \
             passed; {} failed\n\nerror: test failed, to rerun pass `--bin tarae`\n",
            names.iter().map(|n| format!("test {n} ... FAILED\n")).collect::<String>(),
            names.len()
        )
    }

    fn fed(runs: &[(bool, String)]) -> Tally {
        let mut t = Tally::default();
        for (ok, out) in runs {
            t.run_starts();
            out.lines().for_each(|l| t.line(l));
            if !ok {
                t.run_failed();
            }
        }
        t
    }

    /// Five runs, three failing: each test once, the most runs first, with its runs; a run that failed naming none
    /// said by its number; the `stdout` sections' names never counted.
    #[test]
    fn every_failing_test_is_said_once_with_the_runs_it_failed_in() {
        let pass = (true, "running 3 tests\ntest result: ok. 3 passed\n".to_string());
        let t = fed(&[
            pass.clone(),
            (false, failing(&["repotrust::tests::granted", "terminal::tests::a_question"])),
            pass.clone(),
            (false, failing(&["repotrust::tests::granted"])),
            (false, "error: test failed, to rerun pass `--bin tarae`\n(signal: 11, SIGSEGV)\n".into()),
        ]);
        assert!(t.any_failed());
        assert_eq!(
            t.said(5),
            [
                "failed in 3 of 5 runs — 2 tests failed, each once with the runs it failed in:",
                "  2 of 5  repotrust::tests::granted    (runs 2, 4)",
                "  1 of 5  terminal::tests::a_question  (run 2)",
                "  run 5 failed with no test named (a test binary that crashed, or the build — its output)",
            ]
        );
        assert!(fed(&[pass.clone(), pass]).said(2).is_empty(), "none failed: nothing said");
    }

    /// A list's end is its blank line: a later line four spaces in (a test's own output) isn't a name.
    #[test]
    fn a_list_ends_at_its_blank_line() {
        let out =
            format!("{}\n    not a test name, a test's own words\n    later::line\n", failing(&["a::b"]));
        let t = fed(&[(false, out)]);
        assert_eq!(t.said(1)[1], "  1 of 1  a::b  (run 1)");
        assert_eq!(t.said(1).len(), 2, "{:?}", t.said(1));
    }
}
