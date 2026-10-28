//! What the tests of tarae's crates share (a dev-dependency only): a test's own scratch folder (`scratch`), the
//! one patience every test waits with (`PATIENCE`) and the readers' seeded mutators (`mutate`).

pub mod mutate;
pub mod scratch;

/// How long a test waits for what it waits on before it calls it a hang. A guard, never the test's logic: it
/// only stops a test that would otherwise never end, so it's generous — a shared runner, or this Mac while several
/// sessions build at once, runs the suite 3–5× slower, and guards of a few seconds tripped there one a round
/// (CI's rounds 1–6). A passing test
/// never waits this long. One value for every test: the e2e harness's `TIMEOUT` and xtask's `PATIENCE` are the
/// same (tarae's `every_test_waits_with_one_patience`, which also keeps new deadlines off). What a test checks
/// against time — a timer under test, a program's own `read -t`, "viewed past 3 s" — is a condition or the
/// simulation's clock, never a deadline; a look that nothing happens meanwhile is `quiet_for`'s.
pub const PATIENCE: std::time::Duration = std::time::Duration::from_secs(120);
