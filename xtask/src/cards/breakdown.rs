//! The breakdowns P6's next decisions hang on (`.sb/p6-step1-report.md` §5): every card by session, Docker's socket
//! cards by command, the bring card's `p`, the change sets.
//!
//! From v2 on the record keeps a Claude card's `conversation`, a failed preflight's `step` and first `test` on the
//! bring card back, a change set's `files` (`src/cards.rs`, P6 2b). A v1 entry has none of them: a socket card's
//! later chat is told apart there only by `same`/`changed`, and each section says how many entries name theirs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;

use super::{Opts, answers, claudes, median, s, secs, table, timed, why_of};

/// Since this commit (polish landing 4) only the bring card back after its preflight failed has `p` — the
/// preflight again; before, `p` was the plain card's "preflight, then bring". A tarae started before it and not
/// restarted still answered the old way: the line is as near as the record gets.
const P_AGAIN_SINCE: &str = "2026-11-04 00:03:16";
const P_AGAIN_COMMIT: &str = "574212553";

/// The socket table's rows at most — the rest summed in a line.
const COMMANDS_MAX: usize = 40;

/// A change set's files named on its line at most — the rest counted.
const SET_FILES_SHOWN: usize = 8;

/// Programs that end what they run after a time: `timeout 1800 cargo xtask …` is still that command
/// (`tools::pinned_head`'s limits — the tools pack's).
const LIMITS: &[&str] = &["timeout", "gtimeout"];

/// tarae's own: neither Claude's nor a refusal.
fn own(e: &Value) -> bool {
    !claudes(e) && s(e, "answer") != "refused"
}

/// Asked → answered, for a card answered by a key (a dropped one held nothing to the end).
fn up(e: &Value) -> Option<u64> {
    e["up_ms"].as_u64().filter(|_| s(e, "answer") != "dropped")
}

fn med(v: Vec<u64>) -> String {
    median(v).map_or_else(|| "—".into(), secs)
}

/// Its session as the tables say it: the home checkout marked, a child with its parent, none tarae's own.
fn who(e: &Value, o: &Opts) -> String {
    match s(e, "session") {
        "" => "(none)".into(),
        n if n == o.home => format!("{n} (home)"),
        n => match o.parents.get(n) {
            Some(p) => format!("{n} (child of {p})"),
            None => n.to_string(),
        },
    }
}

/// Home first, then by name.
fn who_order(e: &Value, o: &Opts) -> (bool, String) {
    (s(e, "session") != o.home, who(e, o))
}

/// Every card answered or dropped, by kind · `why` × session: how many, their answers, the medians drawn →
/// answered and asked → answered.
pub(super) fn by_session(all: &[Value], o: &Opts) -> String {
    let mut out = String::from(
        "\n## By session\n\nTimes are medians: drawn → answered, and asked → answered (`up_ms`: how long the card held \
         its session, in front or not); dropped cards are left out of both, never-drawn ones of the first.\n",
    );
    for (title, theirs) in [("Claude's cards", true), ("tarae's own", false)] {
        let mut rows: BTreeMap<(String, (bool, String)), Vec<&Value>> = BTreeMap::new();
        for e in all.iter().filter(|e| if theirs { claudes(e) } else { own(e) }) {
            let key = match theirs {
                true => format!("{} · `{}`", s(e, "kind"), why_of(e).0),
                false => s(e, "kind").to_string(),
            };
            rows.entry((key, who_order(e, o))).or_default().push(e);
        }
        out += &format!("\n### {title}\n\n");
        if rows.is_empty() {
            out += "None.\n";
            continue;
        }
        let t: Vec<Vec<String>> = rows
            .iter()
            .map(|((key, (_, who)), es)| {
                vec![
                    key.clone(),
                    who.clone(),
                    es.len().to_string(),
                    answers(es),
                    med(es.iter().filter_map(|e| timed(e)).collect()),
                    med(es.iter().filter_map(|e| up(e)).collect()),
                ]
            })
            .collect();
        let head = match theirs {
            true => "Kind · `why`",
            false => "Kind",
        };
        out += &table(&[head, "Session", "Cards", "Answers", "Drawn → answered", "Asked → answered"], &t);
    }
    out
}

// ── Docker's socket cards ─────────────────────────────────────────────────────────────────────

/// One `llm.docker-inert` entry (`tools::Inert`, the same reading): after `run`'s words, the acts, switches,
/// options with one plain value, and test names where `test_names`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Inert {
    run: Vec<String>,
    words: Vec<String>,
    flags: Vec<String>,
    values: Vec<String>,
    test_names: bool,
}

impl Inert {
    /// `"cargo xtask linux: clippy test --userns --bin= --"` — None for one not so written (tarae frees nothing
    /// by it either).
    pub(crate) fn parse(text: &str) -> Option<Inert> {
        let plain = |w: &str| {
            !w.is_empty()
                && !w.starts_with(['-', '.'])
                && w.chars().all(|c| c.is_ascii_alphanumeric() || "._-+".contains(c))
        };
        let option = |w: &str| w.strip_prefix("--").or_else(|| w.strip_prefix('-')).is_some_and(plain);
        let (run, args) = text.split_once(':')?;
        let mut i = Inert { run: run.split_whitespace().map(String::from).collect(), ..Inert::default() };
        if i.run.is_empty() || !i.run.iter().all(|w| plain(w)) {
            return None;
        }
        for w in args.split_whitespace() {
            match w {
                "--" => i.test_names = true,
                w if w.strip_suffix('=').is_some_and(option) => i.values.push(w.trim_end_matches('=').into()),
                w if option(w) => i.flags.push(w.into()),
                w if plain(w) => i.words.push(w.into()),
                _ => return None,
            }
        }
        Some(i)
    }

    /// Whether every one of `args` is what it lets vary (`tools::inert_args`).
    fn covers(&self, args: &[&str]) -> bool {
        let value = |w: &str| {
            !w.is_empty()
                && !w.starts_with('-')
                && w.chars().all(|c| c.is_ascii_alphanumeric() || "_-.:".contains(c))
        };
        let test_name =
            |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':');
        let has = |list: &[String], w: &str| list.iter().any(|x| x == w);
        let (mut i, mut testing) = (0, false);
        while let Some(&w) = args.get(i) {
            if w == "--" {
                return self.test_names && args[i + 1..].iter().all(|w| test_name(w));
            }
            if has(&self.words, w) || has(&self.flags, w) {
                testing |= w == "test";
                i += 1;
            } else if has(&self.values, w) && args.get(i + 1).is_some_and(|v| value(v)) {
                i += 2;
            } else if self.test_names && testing && test_name(w) {
                i += 1;
            } else {
                return false;
            }
        }
        true
    }
}

/// The project's `llm.docker-inert` (its `.tarae.toml`) and the packs' `entry` runs (`runtime/packs/*/pack.toml`'s
/// `[claude] entry` — `cargo xtask`): what `a` on Docker's card lets vary, read where tarae reads them.
pub(crate) fn inert_here(root: &Path) -> (Vec<Inert>, Vec<Vec<String>>) {
    let read = |p: &Path| std::fs::read_to_string(p).ok().and_then(|t| t.parse::<toml::Table>().ok());
    let strs = |v: Option<&toml::Value>| -> Vec<String> {
        v.and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect()
    };
    let project = read(&root.join(".tarae.toml"));
    let inert = strs(project.as_ref().and_then(|t| t.get("llm")).and_then(|l| l.get("docker-inert")));
    let mut entries = Vec::new();
    let mut packs: Vec<_> =
        std::fs::read_dir(root.join("runtime/packs")).into_iter().flatten().flatten().collect();
    packs.sort_by_key(|d| d.file_name());
    for d in packs {
        let Some(pack) = read(&d.path().join("pack.toml")) else { continue };
        let es = pack.get("claude").and_then(|c| c.get("entry")).and_then(toml::Value::as_array);
        for e in es.into_iter().flatten() {
            if let Some(run) = e.get("run").and_then(toml::Value::as_str) {
                entries.push(run.split_whitespace().map(String::from).collect());
            }
        }
    }
    (inert.iter().filter_map(|s| Inert::parse(s)).collect(), entries)
}

/// A command's first part's words — before a pipe (what follows one only filters what's shown), `&&`, `||` or `;`,
/// outside quotes, a `2>&1` left out — and whether it's the only part but for a pipe's filters.
fn first_part(cmd: &str) -> (Vec<&str>, bool) {
    let (mut quote, mut end, mut alone) = (None, None, true);
    let b = cmd.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, b'\'' | b'"') => quote = Some(c),
            (None, b'|')
                if b.get(i + 1) != Some(&b'|') && i.checked_sub(1).and_then(|p| b.get(p)) != Some(&b'|') =>
            {
                end.get_or_insert(i);
            }
            (None, b'|' | b';') => {
                end.get_or_insert(i);
                alone = false;
            }
            (None, b'&') if b.get(i + 1) == Some(&b'&') => {
                end.get_or_insert(i);
                alone = false;
            }
            _ => {}
        }
    }
    let head = &cmd[..end.unwrap_or(cmd.len())];
    let redirect =
        |w: &&str| matches!(w.as_bytes(), [d, b'>', b'&', e] if d.is_ascii_digit() && e.is_ascii_digit());
    (head.split_whitespace().filter(|w| !redirect(w)).collect(), alone)
}

/// Whether `NAME=value`, a name set in front of a command.
fn env_set(w: &str) -> bool {
    w.split_once('=').is_some_and(|(n, _)| {
        n.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The pinned command `cmd` is, when it's one whose code a pack's entry pins and whose own arguments are all
/// inert (`tools::pinned_head`: what `a` on its card covers, the user's decisions 2026-10-27) — its words, as the
/// table names it (`cargo xtask linux …`). One part only: a line of several isn't read here.
pub(crate) fn pinned(cmd: &str, o: &Opts) -> Option<String> {
    let (words, alone) = first_part(cmd);
    if !alone {
        return None;
    }
    let lead = words.iter().take_while(|w| env_set(w)).count();
    let rest = &words[lead..];
    let after = match rest {
        [t, d, ..] if LIMITS.contains(t) && !d.starts_with('-') => 2,
        _ => 0,
    };
    let ws = &rest[after..];
    let starts =
        |ws: &[&str], run: &[String]| run.len() <= ws.len() && run.iter().zip(ws).all(|(a, b)| a == b);
    let spec = o.inert.iter().find(|i| !i.run.is_empty() && starts(ws, &i.run))?;
    let spec_words: Vec<&str> = spec.run.iter().map(String::as_str).collect();
    o.entries.iter().find(|e| !e.is_empty() && starts(&spec_words, e))?;
    let kept = lead + after + spec.run.len();
    spec.covers(&words[kept..]).then(|| format!("{} …", spec.run.join(" ")))
}

/// The folders a workspace keeps — `[workspace] keep` in the project's config, `toml` (its `.tarae.toml`): its plans
/// and drafts, git-ignored. Read where tarae reads it, never a name assumed (decision 17).
pub(crate) fn keep_in(toml: &Path) -> Vec<String> {
    let t = std::fs::read_to_string(toml).ok().and_then(|t| t.parse::<toml::Table>().ok());
    let keep = t.as_ref().and_then(|t| t.get("workspace")).and_then(|w| w.get("keep"));
    keep.and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .map(|k| k.trim_start_matches("./").trim_end_matches('/').to_string())
        .filter(|k| !k.is_empty())
        .collect()
}

/// Claude's cards asking for a socket (`shadow=socket`) by command: whether `a` covers it (pinned, inert
/// arguments), and how often a pinned one was asked again in its session after an `a` on it.
pub(super) fn sockets(all: &[Value], o: &Opts) -> String {
    let socket: Vec<&Value> = all.iter().filter(|e| claudes(e) && s(e, "shadow") == "socket").collect();
    let mut out = format!("\n## Socket cards by command: {}\n", socket.len());
    if socket.is_empty() {
        return out;
    }
    let named = |e: &Value| match pinned(s(e, "what"), o) {
        Some(p) => (p, true),
        None => (first_part(s(e, "what")).0.join(" "), false),
    };
    let mut rows: BTreeMap<(String, bool), Vec<&Value>> = BTreeMap::new();
    for e in &socket {
        rows.entry(named(e)).or_default().push(e);
    }
    let mut rows: Vec<_> = rows.into_iter().collect();
    rows.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    let t: Vec<Vec<String>> = rows
        .iter()
        .take(COMMANDS_MAX)
        .map(|((cmd, pin), es)| {
            let sessions: BTreeSet<&str> = es.iter().map(|e| s(e, "session")).collect();
            vec![
                format!("`{}`", if cmd.is_empty() { "—" } else { cmd }),
                if *pin { "yes" } else { "no" }.into(),
                es.len().to_string(),
                answers(es),
                sessions.len().to_string(),
                med(es.iter().filter_map(|e| timed(e)).collect()),
            ]
        })
        .collect();
    out += &format!(
        "\nPinned · inert: a pack's entry pins its code and its own arguments are all `llm.docker-inert`'s ({} \
         entr{} read) — what the card's `a` covers. A pipe's filters and a `2>&1` aside; a line of several commands \
         is never one.\n\n",
        o.inert.len(),
        if o.inert.len() == 1 { "y" } else { "ies" }
    );
    out += &table(&["Command", "Pinned · inert", "Cards", "Answers", "Sessions", "Median time"], &t);
    if rows.len() > COMMANDS_MAX {
        let left = &rows[COMMANDS_MAX..];
        let cards: usize = left.iter().map(|r| r.1.len()).sum();
        out += &format!("\n{} more commands, {cards} cards.\n", left.len());
    }
    // Asked again after an `a`: `a` is the chat's; `same` is the session's since tarae started (`sinceyes`) —
    // the same words with nothing changed since can only be a later chat
    let (mut again, mut unchanged, mut changed, mut other) = (0, 0, 0, 0);
    let mut allowed: BTreeSet<(&str, String)> = BTreeSet::new();
    for e in &socket {
        let (cmd, true) = named(e) else { continue };
        let key = (s(e, "session"), cmd);
        if allowed.contains(&key) {
            again += 1;
            match (e["changed"].as_bool() == Some(true), e.get("same").is_some()) {
                (true, _) => changed += 1,
                (false, true) => unchanged += 1,
                (false, false) => other += 1,
            }
        }
        if s(e, "answer") == "a" {
            allowed.insert(key);
        }
    }
    // By chat (v2's `conversation`): asked again after an `a` on it in the same chat — what `a` covers, unless what
    // it runs changed — or in a later chat of its session
    let (mut named_n, mut same_chat, mut same_changed, mut later) = (0, 0, 0, 0);
    let mut chats: BTreeMap<(&str, String), BTreeSet<&str>> = BTreeMap::new();
    for e in socket.iter().filter(|e| e.get("conversation").is_some()) {
        named_n += 1;
        let (cmd, true) = named(e) else { continue };
        let (key, chat) = ((s(e, "session"), cmd), s(e, "conversation"));
        match chats.get(&key).map(|c| c.contains(chat)) {
            Some(true) => {
                same_chat += 1;
                same_changed += usize::from(e["changed"].as_bool() == Some(true));
            }
            Some(false) => later += 1,
            None => {}
        }
        if s(e, "answer") == "a" {
            chats.entry(key).or_default().insert(chat);
        }
    }
    let by_chat = match named_n {
        0 => "the record keeps a Claude card's `conversation` from v2 on, and no card here names its chat — \
              only the session, and a run's id counted across tarae."
            .to_string(),
        n => format!(
            "{n} of {} cards name their chat (`conversation`, kept from v2 on). Asked again after an `a` on the same \
             pinned command: {same_chat} in the same chat ({same_changed} with what it runs changed since: the card \
             was due; the rest `a` should have covered), {later} in a later chat of its session.",
            socket.len()
        ),
    };
    let pinned_n: usize = rows.iter().filter(|r| r.0.1).map(|r| r.1.len()).sum();
    out += &format!(
        "\n- **By chat:** {by_chat}\n\
         - **Asked again after an `a`** on the same pinned command in its session: {again} of {pinned_n} pinned \
         cards.\n  - {unchanged} the same words with nothing changed since (`same`, no `changed`): `a` is the chat's, so a \
         later chat.\n  - {changed} with what it runs changed since (`changed`): the card was due.\n  - {other} in other \
         words (another test, another option): a later chat, a changed pin or a tarae started again — the record \
         can't tell.\n"
    );
    out
}

// ── tarae's own: the bring card's `p`, the change sets ────────────────────────────────────────

/// The bring card's (`rebased`) `p` by session: before `P_AGAIN_SINCE` the plain card's "preflight, then bring",
/// since then the failed card's "preflight again", and the most of those in a row (each the card back failed).
pub(super) fn preflight_again(all: &[Value], o: &Opts) -> String {
    let rebased: Vec<&Value> = all.iter().filter(|e| own(e) && s(e, "kind") == "rebased").collect();
    let p = |e: &&&Value| s(e, "answer") == "p";
    let mut out = format!(
        "\n## The bring card's `p`: {}\n\nBefore {P_AGAIN_COMMIT} ({}) `p` was the plain card's \"preflight, then \
         bring\"; since, only the card back after a failed preflight has it: \"preflight again\". In a row: `p` on \
         each `rebased` card of the session in turn, the card back failed each time. The card back names the step it \
         failed at and the first test that failed from v2 on: by failing step below.\n",
        rebased.iter().filter(p).count(),
        &P_AGAIN_SINCE[..16],
    );
    if rebased.is_empty() {
        return out;
    }
    // (session → cards, `p` before, `p` since, in a row now, most in a row)
    let mut rows: BTreeMap<(bool, String), [usize; 5]> = BTreeMap::new();
    for e in &rebased {
        let r = rows.entry(who_order(e, o)).or_default();
        let since = s(e, "at") >= P_AGAIN_SINCE;
        r[0] += 1;
        match (s(e, "answer") == "p", since) {
            (true, false) => r[1] += 1,
            (true, true) => {
                r[2] += 1;
                r[3] += 1;
                r[4] = r[4].max(r[3]);
            }
            (false, _) => r[3] = 0,
        }
    }
    let mut total = [0; 5];
    let mut t: Vec<Vec<String>> = Vec::new();
    for ((_, who), r) in &rows {
        for i in [0, 1, 2] {
            total[i] += r[i];
        }
        total[4] = total[4].max(r[4]);
        t.push(vec![who.clone(), r[0].to_string(), r[1].to_string(), r[2].to_string(), r[4].to_string()]);
    }
    t.push(vec![
        "**Total**".into(),
        total[0].to_string(),
        total[1].to_string(),
        total[2].to_string(),
        total[4].to_string(),
    ]);
    out += "\n";
    out += &table(&["Session", "`rebased` cards", "`p` before", "`p` since", "Most `p` in a row since"], &t);
    out + &by_step(&rebased, o)
}

/// The bring cards back after a failed preflight (v2's `step`) by the step they failed at — one refused before it
/// started apart —, their answers, how many first failed at a generated reference out of date, the first tests that
/// failed; then a test first to fail in more than one session.
fn by_step(rebased: &[&Value], o: &Opts) -> String {
    let failed: Vec<&Value> = rebased.iter().copied().filter(|e| e.get("step").is_some()).collect();
    let mut out = format!("\n### By failing step: {} of {} cards name theirs\n", failed.len(), rebased.len());
    if failed.is_empty() {
        return out + "\nNone here: the record keeps a failed preflight's step from v2 on.\n";
    }
    let mut steps: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for e in &failed {
        let step = match e["not_run"].as_bool() == Some(true) {
            true => format!("{} (not run)", s(e, "step")),
            false => s(e, "step").to_string(),
        };
        steps.entry(step).or_default().push(e);
    }
    let t: Vec<Vec<String>> = steps
        .iter()
        .map(|(step, es)| {
            let mut tests: BTreeMap<&str, usize> = BTreeMap::new();
            for e in es.iter().filter(|e| !s(e, "test").is_empty()) {
                *tests.entry(s(e, "test")).or_default() += 1;
            }
            let mut tests: Vec<_> = tests.into_iter().collect();
            tests.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            let tests: Vec<String> = tests.iter().map(|(t, n)| format!("`{t}` ×{n}")).collect();
            let stale = es.iter().filter(|e| e["generated"].as_bool() == Some(true)).count();
            vec![
                format!("`{step}`"),
                es.len().to_string(),
                answers(es),
                stale.to_string(),
                if tests.is_empty() { "—".into() } else { tests.join(" · ") },
            ]
        })
        .collect();
    out += "\n";
    out += &table(&["Failing step", "Cards", "Answers", "Stale references", "First tests"], &t);
    // (a test first to fail in several sessions: a flake, or the base's own failure — not one session's change)
    let mut where_: BTreeMap<&str, BTreeSet<(bool, String)>> = BTreeMap::new();
    for e in failed.iter().filter(|e| !s(e, "test").is_empty()) {
        where_.entry(s(e, "test")).or_default().insert(who_order(e, o));
    }
    let across: Vec<String> = where_
        .iter()
        .filter(|(_, ws)| ws.len() > 1)
        .map(|(t, ws)| format!("`{t}` — {}", ws.iter().map(|w| w.1.as_str()).collect::<Vec<_>>().join(", ")))
        .collect();
    out += &match across.is_empty() {
        true => "\n- No test was the first to fail in more than one session.\n".to_string(),
        false => format!(
            "\n- **First to fail in more than one session** (a flake, or the base's own): {}.\n",
            across.join("; ")
        ),
    };
    out
}

/// A change set's files, as the record keeps them (v2's `files`).
fn files_of(e: &Value) -> Vec<&str> {
    e["files"].as_array().into_iter().flatten().filter_map(Value::as_str).collect()
}

/// Every change set's card, listed by its time, session and chat (to find in that conversation) with its files
/// from v2 on; then how many touched only a folder the workspace keeps (`Opts::keep`, the project's own).
pub(super) fn change_sets(all: &[Value], o: &Opts) -> String {
    let sets: Vec<&Value> = all.iter().filter(|e| own(e) && s(e, "kind") == "change-set").collect();
    let mut out = format!(
        "\n## Change sets: {}\n\nEach by its time, session and chat, to find in that conversation; its files from v2 \
         on.\n",
        sets.len()
    );
    if !sets.is_empty() {
        out += "\n";
    }
    for e in &sets {
        let card = e["card"].as_u64().map(|c| format!(" card#{c}")).unwrap_or_default();
        let took = timed(e).map(|ms| format!(" · {}", secs(ms))).unwrap_or_default();
        let chat = s(e, "conversation");
        let chat =
            if chat.is_empty() { String::new() } else { format!(" · chat {}", &chat[..chat.len().min(8)]) };
        let files = files_of(e);
        let more =
            files.len().saturating_sub(SET_FILES_SHOWN) + e["files_more"].as_u64().unwrap_or(0) as usize;
        let files = match files.is_empty() {
            true => String::new(),
            false => {
                let shown = files[..files.len().min(SET_FILES_SHOWN)].join(", ");
                if more > 0 { format!(" · {shown} … {more} more") } else { format!(" · {shown}") }
            }
        };
        out += &format!("- {} {}{card} · {}{took}{chat}{files}\n", s(e, "at"), who(e, o), s(e, "answer"));
    }
    let named: Vec<&&Value> = sets.iter().filter(|e| !files_of(e).is_empty()).collect();
    if named.is_empty() {
        return out + "\n- No change set here names its files: the record keeps them from v2 on.\n";
    }
    if o.keep.is_empty() {
        return out + "\n- **Only a kept folder:** the project keeps none (`[workspace] keep`).\n";
    }
    let kept = |f: &str| {
        o.keep.iter().any(|k| f == k || f.strip_prefix(k.as_str()).is_some_and(|r| r.starts_with('/')))
    };
    // (one with files past what's kept isn't known to be only there)
    let only =
        named.iter().filter(|e| e.get("files_more").is_none() && files_of(e).into_iter().all(kept)).count();
    let keep: Vec<String> = o.keep.iter().map(|k| format!("`{k}`")).collect();
    out + &format!(
        "\n- **Only a kept folder** ({}, the project's `[workspace] keep`): {only} of {} change sets naming their \
         files.\n",
        keep.join(", "),
        named.len()
    )
}
