//! `cargo xtask cards [--from DAY] [--to DAY] [--home NAME] [--sessions PATH] [--inert SPEC …] [PATH …]` — the
//! card record (`src/cards.rs`: `$state/cards/<yyyy-mm>.jsonl`, every file in a folder given, the state folder's
//! when none is) summed up as cardauto's §4.2 reads it:
//!
//! - **the P3 table** — Claude's cards by kind · `why` (its program after `:` folded into the row) × `shadow`:
//!   how many, their answers, how many stamped (answered within 1.5 s of being drawn), the median drawn →
//!   answered;
//! - **removed by P3** — Claude's cards with `shadow=none` of all of Claude's (`why` ≠ `own`, `spend`);
//! - **P3 wrong yes** — `shadow=none` ∧ `n`, each with its command, to be read by hand;
//! - **the stamp rate** by `why`; dropped and never-drawn cards are left out of stamps and times;
//! - **refused with no card**, each with its command and what Claude was told;
//! - **tarae's own cards**, by kind and answer;
//! - **auto cards' reviewer** (`reviewer`, R1's shadow week): its verdict beside your answer, by kind;
//!
//! and P6's breakdowns (`breakdown`): every card **by session** (the home checkout — `--home`, its branch, `main`
//! when not given — and each named one, a child marked by its kept record's parent: `--sessions`, the state
//! folder's `sessions/` when not given), Docker's **socket cards by command** (whether `a` covers it: the project's
//! `llm.docker-inert`, and `--inert` more) and by chat, the bring card's **`p`** and its failing step, the **change
//! sets** with their files (how many only in a folder the project's `[workspace] keep` names).
//!
//! `--from`/`--to` (`2026-10-31`, both ends in) keep only the entries of those days. A line that isn't an entry (a
//! write cut short) is counted and skipped.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::Result;

mod breakdown;
mod reviewer;

/// Answered within this long of being drawn: a stamp, not a read (cards.md §1.3).
const STAMP_MS: u64 = 1500;

/// What the report is asked for besides the record.
#[derive(Debug)]
struct Opts {
    /// Only the entries of these days (`2026-10-31`), both ends in.
    from: Option<String>,
    to: Option<String>,
    /// The home checkout's name in the record: its branch.
    home: String,
    /// A child session's name → its parent's, from the kept sessions.
    parents: BTreeMap<String, String>,
    /// `llm.docker-inert`'s entries, and the packs' entry runs (`cargo xtask`).
    inert: Vec<breakdown::Inert>,
    entries: Vec<Vec<String>>,
    /// The folders a workspace keeps, the project's `[workspace] keep` (a change set touching only them).
    keep: Vec<String>,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            from: None,
            to: None,
            home: "main".into(),
            parents: BTreeMap::new(),
            inert: Vec::new(),
            entries: Vec::new(),
            keep: Vec::new(),
        }
    }
}

/// `2026-10-31`, or why not.
fn day(flag: &str, v: Option<&String>) -> Result<String> {
    let v = v.ok_or(format!("{flag}: a day, 2026-10-31"))?;
    let ok = v.len() == 10
        && v.bytes().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == b'-' } else { c.is_ascii_digit() });
    ok.then(|| v.clone()).ok_or(format!("{flag} {v}: a day, 2026-10-31"))
}

pub fn run(args: &[String]) -> Result<()> {
    let (inert, entries) = breakdown::inert_here(&crate::root());
    let keep = breakdown::keep_in(&crate::root().join(".tarae.toml"));
    let mut o = Opts { inert, entries, keep, ..Opts::default() };
    let (mut paths, mut sessions) = (Vec::new(), None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--from" => o.from = Some(day(a, it.next())?),
            "--to" => o.to = Some(day(a, it.next())?),
            "--home" => o.home = it.next().ok_or("--home: the home checkout's name in the record")?.clone(),
            "--sessions" => sessions = Some(PathBuf::from(it.next().ok_or("--sessions: a file or folder")?)),
            "--inert" => {
                let spec = it.next().ok_or("--inert: an llm.docker-inert entry")?;
                o.inert.push(
                    breakdown::Inert::parse(spec).ok_or(format!(
                        "--inert {spec:?}: as llm.docker-inert writes one, `run words: args`"
                    ))?,
                );
            }
            f if f.starts_with("--") => return Err(format!("{f}: not an option cards takes")),
            p => paths.push(PathBuf::from(p)),
        }
    }
    if paths.is_empty() {
        let state = state_dir().ok_or("neither XDG_STATE_HOME nor HOME is set: give the record's folder")?;
        paths.push(state.join("cards"));
        sessions = sessions.or_else(|| Some(state.join("sessions")).filter(|p| p.is_dir()));
    }
    if let Some(p) = &sessions {
        for f in jsonl_in(p)?.into_iter().chain(json_in(p)?) {
            let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            o.parents.extend(parents_of(&text, &o.home));
        }
    }
    let mut files = Vec::new();
    for p in &paths {
        files.extend(jsonl_in(p)?);
    }
    if files.is_empty() {
        return Err(format!("no card record in {}", paths[0].display()));
    }
    let mut text = String::new();
    for f in &files {
        text += &std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
    }
    let names: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
    print!("{}", report(&names.join(", "), &text, &o));
    Ok(())
}

/// `$XDG_STATE_HOME/tarae` (`~/.local/state/tarae`) — as `config::state_dir` finds it.
fn state_dir() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(state.join("tarae"))
}

/// The `.json` files in folder `p` (none when it's a file: `jsonl_in` took it).
fn json_in(p: &Path) -> Result<Vec<PathBuf>> {
    if !p.is_dir() {
        return Ok(Vec::new());
    }
    let rd = std::fs::read_dir(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut v: Vec<PathBuf> =
        rd.flatten().map(|e| e.path()).filter(|f| f.extension().is_some_and(|x| x == "json")).collect();
    v.sort();
    Ok(v)
}

/// A kept sessions file (`worktree::save_judged`: `repo`, `sessions` of `root`, `name`, `parent`) → each child's
/// name and its parent's — the home checkout (the repository's, beside its `.git`) by `home`, another by its name,
/// one not kept by its folder's.
fn parents_of(text: &str, home: &str) -> Vec<(String, String)> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    let kept = v["sessions"].as_array().cloned().unwrap_or_default();
    let home_root = Path::new(s(&v, "repo")).parent().map(Path::to_path_buf);
    let name_at = |root: &str| -> String {
        if home_root.as_deref() == Some(Path::new(root)) {
            return home.to_string();
        }
        match kept.iter().find(|k| s(k, "root") == root) {
            Some(k) => s(k, "name").to_string(),
            None => Path::new(root).file_name().map_or(root.into(), |n| n.to_string_lossy().into_owned()),
        }
    };
    kept.iter()
        .filter(|k| !s(k, "parent").is_empty())
        .map(|k| (s(k, "name").to_string(), name_at(s(k, "parent"))))
        .collect()
}

/// `p` itself, or the `.jsonl` files in it, oldest month first.
pub(crate) fn jsonl_in(p: &Path) -> Result<Vec<PathBuf>> {
    if !p.is_dir() {
        return Ok(vec![p.to_path_buf()]);
    }
    let rd = std::fs::read_dir(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut v: Vec<PathBuf> =
        rd.flatten().map(|e| e.path()).filter(|f| f.extension().is_some_and(|x| x == "jsonl")).collect();
    v.sort();
    Ok(v)
}

/// A field as text ("" when absent).
pub(crate) fn s<'a>(e: &'a Value, k: &str) -> &'a str {
    e[k].as_str().unwrap_or("")
}

/// Its `why` without the program after `:` (`not-a-check:awk` → `not-a-check`), and that program.
fn why_of(e: &Value) -> (&str, Option<&str>) {
    match s(e, "why").split_once(':') {
        Some((w, p)) => (w, Some(p)),
        None => (s(e, "why"), None),
    }
}

fn claudes(e: &Value) -> bool {
    !matches!(why_of(e).0, "own" | "spend") && s(e, "answer") != "refused"
}

/// Answered by a key, after being drawn: what stamps and times count.
fn timed(e: &Value) -> Option<u64> {
    e["drawn_ms"].as_u64().filter(|_| s(e, "answer") != "dropped")
}

fn stamped(e: &Value) -> bool {
    timed(e).is_some_and(|ms| ms <= STAMP_MS)
}

pub(crate) fn median(mut v: Vec<u64>) -> Option<u64> {
    v.sort_unstable();
    v.get(v.len() / 2).copied().filter(|_| !v.is_empty())
}

pub(crate) fn secs(ms: u64) -> String {
    format!("{:.1} s", ms as f64 / 1000.0)
}

pub(crate) fn pct(n: usize, of: usize) -> String {
    (n * 100 + of / 2).checked_div(of).map_or_else(|| "—".into(), |p| format!("{p} %"))
}

/// `24 y · 1 n` — how often each answer, most first.
fn answers(es: &[&Value]) -> String {
    let mut n: BTreeMap<&str, usize> = BTreeMap::new();
    for e in es {
        *n.entry(s(e, "answer")).or_default() += 1;
    }
    let mut v: Vec<(&str, usize)> = n.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v.iter().map(|(a, n)| format!("{n} {a}")).collect::<Vec<_>>().join(" · ")
}

/// A Markdown table, its columns padded to line up in a terminal too.
pub(crate) fn table(head: &[&str], rows: &[Vec<String>]) -> String {
    let w: Vec<usize> = (0..head.len())
        .map(|c| {
            rows.iter().map(|r| r[c].chars().count()).chain([head[c].chars().count()]).max().unwrap_or(0)
        })
        .collect();
    let line = |cells: Vec<String>| {
        let padded: Vec<String> = cells
            .iter()
            .enumerate()
            .map(|(c, x)| format!("{x}{}", " ".repeat(w[c] - x.chars().count())))
            .collect();
        format!("| {} |\n", padded.join(" | "))
    };
    let mut out = line(head.iter().map(|h| h.to_string()).collect());
    out += &line(w.iter().map(|w| "-".repeat(*w)).collect());
    for r in rows {
        out += &line(r.clone());
    }
    out
}

/// One entry, for a list read by hand: when, the session, the card and run, the command and where it ran.
fn listed(e: &Value) -> String {
    let card = e["card"].as_u64().map(|c| format!(" card#{c}")).unwrap_or_default();
    let run = e["run"].as_u64().map(|r| format!(" run#{r}")).unwrap_or_default();
    let cwd = match s(e, "cwd") {
        "" => String::new(),
        c => format!(" (in {c})"),
    };
    format!("- {} {}{card}{run} · `{}`{cwd}", s(e, "at"), s(e, "session"), s(e, "what"))
}

/// Whether entry `e`'s day is in `o`'s range.
fn in_range(e: &Value, o: &Opts) -> bool {
    let day = s(e, "at").get(..10).unwrap_or("");
    o.from.as_deref().is_none_or(|f| day >= f) && o.to.as_deref().is_none_or(|t| !day.is_empty() && day <= t)
}

/// The whole report for the record's `text` (its files named `from`).
fn report(from: &str, text: &str, o: &Opts) -> String {
    let mut broken = 0;
    let read: Vec<Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let v = serde_json::from_str::<Value>(l).ok().filter(Value::is_object);
            broken += usize::from(v.is_none());
            v
        })
        .collect();
    let all: Vec<Value> = read.iter().filter(|e| in_range(e, o)).cloned().collect();
    let mut out = format!("# Cards\n\nFrom {from}: {} entries", all.len());
    if o.from.is_some() || o.to.is_some() {
        let (f, t) = (o.from.as_deref().unwrap_or("the first"), o.to.as_deref().unwrap_or("the last"));
        out += &format!(" from {f} to {t} (of {})", read.len());
    }
    if let (Some(first), Some(last)) = (all.first(), all.last()) {
        out += &format!(", {} – {}", s(first, "at"), s(last, "at"));
    }
    if broken > 0 {
        out += &format!(" ({broken} line{} not an entry, skipped)", if broken == 1 { "" } else { "s" });
    }
    out += ".\n";
    let claude: Vec<&Value> = all.iter().filter(|e| claudes(e)).collect();

    // The P3 table
    out += &format!(
        "\n## The P3 table (`why` × `shadow` × answer × stamp)\n\nClaude's cards. Stamped = answered within {} of \
         being drawn; times are drawn → answered (dropped and never-drawn cards left out of both).\n\n",
        secs(STAMP_MS)
    );
    let mut rows: BTreeMap<(String, String), Vec<&Value>> = BTreeMap::new();
    for e in &claude {
        let class = e["class"].as_array().map(|c| {
            let c: Vec<&str> = c.iter().filter_map(Value::as_str).collect();
            format!(" (`class={}`)", c.join(","))
        });
        let key = format!("{} · `{}`{}", s(e, "kind"), why_of(e).0, class.unwrap_or_default());
        rows.entry((key, s(e, "shadow").to_string())).or_default().push(e);
    }
    let mut t: Vec<Vec<String>> = rows
        .iter()
        .map(|((key, shadow), es)| {
            let mut programs: Vec<&str> = es.iter().filter_map(|e| why_of(e).1).collect();
            programs.sort_unstable();
            programs.dedup();
            let key = match programs.is_empty() {
                true => key.clone(),
                false => format!(
                    "{key} ({})",
                    programs.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", ")
                ),
            };
            let times: Vec<u64> = es.iter().filter_map(|e| timed(e)).collect();
            vec![
                key,
                shadow.clone(),
                es.len().to_string(),
                answers(es),
                es.iter().filter(|e| stamped(e)).count().to_string(),
                median(times).map_or_else(|| "—".into(), secs),
            ]
        })
        .collect();
    t.push(vec![
        "**Total**".into(),
        String::new(),
        format!("**{}**", claude.len()),
        answers(&claude),
        format!("**{}**", claude.iter().filter(|e| stamped(e)).count()),
        String::new(),
    ]);
    out += &table(&["Kind · `why`", "`shadow`", "Cards", "Answers", "Stamped", "Median time"], &t);

    // The rates
    let none: Vec<&Value> = claude.iter().copied().filter(|e| s(e, "shadow") == "none").collect();
    out += &format!(
        "\n- **Removed by P3** (`shadow=none`): {} of {} ({}).\n",
        none.len(),
        claude.len(),
        pct(none.len(), claude.len())
    );
    let wrong: Vec<&Value> = none.iter().copied().filter(|e| s(e, "answer") == "n").collect();
    out += &format!("- **P3 wrong yes** (`shadow=none` ∧ `n`): {}", wrong.len());
    out += match wrong.is_empty() {
        true => ".\n",
        false => {
            " — read each by hand: did the person only not want it now, or could it hurt inside the \
                  capabilities?\n"
        }
    };
    for e in &wrong {
        out += &format!("  {}\n", listed(e));
    }
    let mut by_why: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for e in claude.iter().filter(|e| timed(e).is_some()) {
        let n = by_why.entry(why_of(e).0).or_default();
        n.0 += usize::from(stamped(e));
        n.1 += 1;
    }
    let rates: Vec<String> =
        by_why.iter().map(|(w, (st, of))| format!("`{w}` {st} of {of} ({})", pct(*st, *of))).collect();
    out += &format!(
        "- **Stamp rate** by `why`: {}.\n",
        if rates.is_empty() { "—".into() } else { rates.join(" · ") }
    );

    // Refused with no card
    let refused: Vec<&Value> = all.iter().filter(|e| s(e, "answer") == "refused").collect();
    out += &format!("\n## Refused with no card: {}\n", refused.len());
    if !refused.is_empty() {
        out += "\n";
    }
    for e in &refused {
        out += &format!("{} — {}\n", listed(e), s(e, "said"));
    }

    // tarae's own
    let own: Vec<&Value> = all.iter().filter(|e| !claudes(e) && s(e, "answer") != "refused").collect();
    out += &format!("\n## tarae's own cards: {}\n", own.len());
    let mut kinds: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for e in &own {
        kinds.entry(s(e, "kind")).or_default().push(e);
    }
    if !kinds.is_empty() {
        out += "\n";
        let t: Vec<Vec<String>> = kinds
            .iter()
            .map(|(k, es)| {
                let times: Vec<u64> = es.iter().filter_map(|e| timed(e)).collect();
                vec![
                    k.to_string(),
                    es.len().to_string(),
                    answers(es),
                    median(times).map_or_else(|| "—".into(), secs),
                ]
            })
            .collect();
        out += &table(&["Kind", "Cards", "Answers", "Median time"], &t);
    }

    out += &reviewer::report(&claude);
    out += &breakdown::by_session(&all, o);
    out += &breakdown::sockets(&all, o);
    out += &breakdown::preflight_again(&all, o);
    out += &breakdown::change_sets(&all, o);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture's record, summed up: the P3 table's rows by kind · why × shadow (programs folded in, a
    /// dropped card counted but neither stamped nor timed), the removed share, the one wrong yes with its
    /// command, the stamp rate, the refusal with what Claude was told, tarae's own apart — and the broken
    /// line skipped, said.
    #[test]
    fn the_record_reads_as_the_p3_table() {
        let r = report("cards.jsonl", include_str!("../fixtures/cards.jsonl"), &Opts::default());
        let has = |s: &str| assert!(r.contains(s), "{s:?} not in:\n{r}");
        has(
            "From cards.jsonl: 9 entries, 2026-10-15 09:01:02 – 2026-10-15 10:10:00 (1 line not an entry, skipped).",
        );
        has(
            "| read · `read` (`class=home`)               | grant:read | 1     | 1 n                   | 0       | 394.3 s     |",
        );
        has(
            "| run · `not-a-check` (`awk`, `mkdir`, `rm`) | none       | 3     | 1 dropped · 1 n · 1 y | 1       | 6.0 s       |",
        );
        has(
            "| run · `reads-outside` (`class=home`)       | grant:read | 1     | 1 y                   | 0       | 1.7 s       |",
        );
        has(
            "| run · `unreadable` (`$`)                   | none       | 1     | 1 y                   | 0       | 11.7 s      |",
        );
        has(
            "| **Total**                                  |            | **6** | 3 y · 2 n · 1 dropped | **1**   |             |",
        );
        has("- **Removed by P3** (`shadow=none`): 4 of 6 (67 %).");
        has("- **P3 wrong yes** (`shadow=none` ∧ `n`): 1 — read each by hand");
        has("  - 2026-10-15 09:07:00 cardauto card#5 run#13 · `rm -rf .sb`\n");
        has(
            "- **Stamp rate** by `why`: `not-a-check` 1 of 2 (50 %) · `read` 0 of 1 (0 %) · `reads-outside` 0 of 1 (0 %) · `unreadable` 0 of 1 (0 %).",
        );
        has(
            "## Refused with no card: 1\n\n- 2026-10-15 10:10:00 cardauto run#31 · `sh -n x.sh` — x.sh was written",
        );
        has("## tarae's own cards: 2");
        has("| rebased     | 1     | 1 p     | 1.2 s       |");
    }

    /// The options as `run` reads them for the breakdown fixture: its days, the kept sessions' parents, the
    /// repository's own `llm.docker-inert` and packs' entries.
    fn breakdown_opts() -> Opts {
        let (inert, entries) = breakdown::inert_here(&crate::root());
        let parents = parents_of(include_str!("../fixtures/sessions.json"), "main").into_iter().collect();
        Opts {
            from: Some("2026-10-31".into()),
            to: Some("2026-11-05".into()),
            parents,
            inert,
            entries,
            ..Opts::default()
        }
    }

    /// A table row of `r` with these cells, whatever their padding: its line's index.
    fn has_row(r: &str, cells: &[&str]) -> usize {
        let want = format!("| {} |", cells.join(" | "));
        let squeezed = |l: &str| l.split_whitespace().collect::<Vec<_>>().join(" ");
        r.lines().position(|l| squeezed(l) == want).unwrap_or_else(|| panic!("{want:?} not in:\n{r}"))
    }

    /// A kept child's parent is named: the home checkout (beside the repository's `.git`) by its name in the
    /// record, another kept session by its name, one not kept by its folder's; a session with none isn't listed.
    #[test]
    fn the_kept_sessions_name_each_childs_parent() {
        let mut p = parents_of(include_str!("../fixtures/sessions.json"), "main");
        p.sort();
        let p: Vec<(&str, &str)> = p.iter().map(|(c, p)| (c.as_str(), p.as_str())).collect();
        assert_eq!(p, [("kid", "fix"), ("lost", "gone"), ("solo", "main")]);
        assert!(parents_of("not json", "main").is_empty());
    }

    /// `--from`/`--to`: only those days' entries, said with how many were read; then every card by session — the home
    /// checkout marked, a child with its parent, the refusal in neither table, a dropped card in no median.
    #[test]
    fn a_range_of_days_reads_by_session() {
        let r = report("cards.jsonl", include_str!("../fixtures/cards-breakdown.jsonl"), &breakdown_opts());
        let has = |s: &str| assert!(r.contains(s), "{s:?} not in:\n{r}");
        has(
            "From cards.jsonl: 16 entries from 2026-10-31 to 2026-11-05 (of 18), 2026-10-31 09:00:00 – 2026-11-05 \
             10:00:00.",
        );
        has("### Claude's cards");
        has_row(&r, &["run · `outside`", "kid (child of fix)", "1", "1 y", "15.0 s", "15.5 s"]);
        let home = has_row(&r, &["run · `socket`", "main (home)", "3", "2 y · 1 a", "2.0 s", "6.0 s"]);
        let fix = has_row(&r, &["run · `socket`", "fix", "3", "1 a · 1 n · 1 y", "4.0 s", "5.0 s"]);
        assert!(home < fix, "the home checkout first:\n{r}");
        has_row(&r, &["run · `socket`", "kid (child of fix)", "1", "1 y", "1.2 s", "1.3 s"]);
        has("### tarae's own");
        has_row(&r, &["change-set", "main (home)", "1", "1 y", "19.0 s", "20.0 s"]);
        has_row(&r, &["change-set", "kid (child of fix)", "1", "1 dropped", "—", "—"]);
        has_row(&r, &["rebased", "fix", "4", "3 p · 1 y", "8.0 s", "9.0 s"]);
        assert!(!r.contains("`script`"), "a refusal is in neither table:\n{r}");
    }

    /// Socket cards by command: the pinned ones with inert arguments as one row (a time limit, a pipe's filter and a
    /// `2>&1` aside), one with an argument `a` doesn't cover on its own; asked again after an `a` in its session, split
    /// by `same`/`changed`; the card out of range left out.
    #[test]
    fn socket_cards_read_by_command_and_asked_again() {
        let r = report("cards.jsonl", include_str!("../fixtures/cards-breakdown.jsonl"), &breakdown_opts());
        let has = |s: &str| assert!(r.contains(s), "{s:?} not in:\n{r}");
        has("## Socket cards by command: 7");
        has_row(&r, &["`cargo xtask linux …`", "yes", "5", "3 y · 2 a", "2", "3.0 s"]);
        has_row(
            &r,
            &[
                "`cargo xtask linux test --bin tarae other_test -- --nocapture`",
                "no",
                "1",
                "1 n",
                "1",
                "6.0 s",
            ],
        );
        has_row(&r, &["`docker ps`", "no", "1", "1 y", "1", "1.2 s"]);
        has("**Asked again after an `a`** on the same pinned command in its session: 3 of 5 pinned cards.");
        has("  - 1 the same words with nothing changed since");
        has("  - 1 with what it runs changed since");
        has("  - 1 in other words");
    }

    /// What `a` covers, as tarae reads it: a pack-pinned command's own arguments all inert — names set in front and
    /// a time limit kept apart; a `--bin=x`, a path, a second command or an option after `--` isn't.
    #[test]
    fn pinned_reads_as_docker_inert_does() {
        let o = Opts {
            inert: vec![
                breakdown::Inert::parse("cargo xtask linux: clippy test --userns --bin= --").unwrap(),
            ],
            entries: vec![vec!["cargo".into(), "xtask".into()]],
            ..Opts::default()
        };
        let pinned = |c: &str| breakdown::pinned(c, &o);
        for yes in [
            "cargo xtask linux clippy",
            "RUST_LOG=x cargo xtask linux test --bin tarae -- a::b c_d",
            "timeout 600 cargo xtask linux test --userns a_test 2>&1 | grep -E 'x|y'",
        ] {
            assert_eq!(pinned(yes).as_deref(), Some("cargo xtask linux …"), "{yes}");
        }
        for no in [
            "cargo xtask linux test --bin=tarae",
            "cargo xtask linux test --bin ../x",
            "cargo xtask linux test -- --nocapture",
            "cargo fmt && cargo xtask linux clippy",
            "cargo xtask kernels",
            "docker ps",
        ] {
            assert_eq!(pinned(no), None, "{no}");
        }
        let unpinned = Opts { entries: Vec::new(), ..o };
        assert_eq!(breakdown::pinned("cargo xtask linux clippy", &unpinned), None, "no entry pins its code");
        assert!(breakdown::Inert::parse("no colon here").is_none());
    }

    /// The bring card's `p` by session, before and since the commit that made it "preflight again", with the most in
    /// a row; the change sets listed by time and session.
    #[test]
    fn the_bring_cards_p_and_the_change_sets_are_broken_down() {
        let r = report("cards.jsonl", include_str!("../fixtures/cards-breakdown.jsonl"), &breakdown_opts());
        let has = |s: &str| assert!(r.contains(s), "{s:?} not in:\n{r}");
        has("## The bring card's `p`: 4");
        has_row(&r, &["main (home)", "1", "0", "1", "1"]);
        has_row(&r, &["fix", "4", "1", "2", "2"]);
        has_row(&r, &["**Total**", "5", "1", "3", "2"]);
        has("## Change sets: 2");
        has("- 2026-11-05 09:00:00 main (home) card#15 · y · 19.0 s\n");
        has("- 2026-11-05 10:00:00 kid (child of fix) card#16 · dropped\n");
        assert!(!r.contains("card#17"), "past --to:\n{r}");
        // (v1: nothing names a chat, a step or a file — said so)
        has("and no card here names its chat");
        has("### By failing step: 0 of 5 cards name theirs");
        has("- No change set here names its files: the record keeps them from v2 on.");
    }

    /// The v2 fixture's options: the kept sessions' parents, the repository's own `llm.docker-inert` and packs'
    /// entries, and the folders kept as `toml`'s `[workspace] keep` says.
    fn v2_opts(toml: &Path) -> Opts {
        let (inert, entries) = breakdown::inert_here(&crate::root());
        let parents = parents_of(include_str!("../fixtures/sessions.json"), "main").into_iter().collect();
        Opts { parents, inert, entries, keep: breakdown::keep_in(toml), ..Opts::default() }
    }

    fn v2(toml: &Path) -> String {
        report("cards.jsonl", include_str!("../fixtures/cards-v2.jsonl"), &v2_opts(toml))
    }

    /// P6 2b, by chat: a pinned socket card asked again after an `a` on it in the same chat (what `a` covers, but
    /// for one whose command changed) told from one in a later chat — a v1 entry names none, and isn't counted.
    #[test]
    fn by_chat_tells_the_same_chat_from_a_later_one() {
        let r = v2(&crate::root().join(".tarae.toml"));
        let has = |s: &str| assert!(r.contains(s), "{s:?} not in:\n{r}");
        has(
            "- **By chat:** 4 of 5 cards name their chat (`conversation`, kept from v2 on). Asked again after an `a` on \
             the same pinned command: 2 in the same chat (1 with what it runs changed since: the card was due; the \
             rest `a` should have covered), 1 in a later chat of its session.",
        );
        has("on the same pinned command in its session: 3 of 5 pinned cards.");
    }

    /// P6 2b, the `p`'s failing step: the cards back after a failed preflight by step — one refused before it started
    /// apart — with their answers, the stale references among them and the first tests; a test first to fail in two
    /// sessions named; the plain card (no step) not counted.
    #[test]
    fn the_ps_failing_step_and_first_tests() {
        let r = v2(&crate::root().join(".tarae.toml"));
        let has = |s: &str| assert!(r.contains(s), "{s:?} not in:\n{r}");
        has("### By failing step: 5 of 6 cards name theirs");
        has_row(
            &r,
            &[
                "`test`",
                "4",
                "3 p · 1 y",
                "1",
                "`a::b::flaky` ×3 · `keymap::tests::keymap_reference_is_up_to_date` ×1",
            ],
        );
        has_row(&r, &["`clippy linux (not run)`", "1", "1 n", "0", "—"]);
        has(
            "- **First to fail in more than one session** (a flake, or the base's own): `a::b::flaky` — fix, kid (child of fix).",
        );
    }

    /// P6 2b, a change set's files on its line (8 at most, the rest counted — what the record left out too) with its
    /// chat; how many touched only a folder the workspace keeps, as the project's `[workspace] keep` names it — not a
    /// literal `.sb` (decision 17): a config keeping `.notes` counts those, this repository's `.sb` its own.
    #[test]
    fn the_change_sets_files_are_listed() {
        let r = v2(&crate::root().join("xtask/fixtures/cards-keep.toml"));
        let has = |s: &str| assert!(r.contains(s), "{s:?} not in:\n{r}");
        has("- 2026-11-09 14:00:00 fix card#12 · y · 9.0 s · chat c1-01234 · .notes/plan.md\n");
        has("- 2026-11-09 14:10:00 fix card#13 · y · 9.0 s · chat c1-01234 · .notes/a.md, src/a.rs\n");
        has("src/f6.rs, src/f7.rs … 5 more\n");
        has("- 2026-11-09 15:00:00 main (home) card#17 · y · 9.0 s\n");
        has(
            "- **Only a kept folder** (`.notes`, the project's `[workspace] keep`): 2 of 5 change sets naming their files.",
        );
        let r = v2(&crate::root().join(".tarae.toml"));
        assert!(
            r.contains(
                "- **Only a kept folder** (`.sb`, the project's `[workspace] keep`): 1 of 5 change sets"
            ),
            "{r}"
        );
        let r = v2(&crate::root().join("xtask/fixtures/no-such.toml"));
        assert!(r.contains("- **Only a kept folder:** the project keeps none (`[workspace] keep`)."), "{r}");
    }
}
