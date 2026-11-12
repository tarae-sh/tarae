//! An ended workspace saved, to be brought back (SM2, the design session's `.sb/workspace-end-snapshot-brief.md`): before
//! a checkout is removed with something that would go (`Ending::goes` — its changes, the commits only its deleted branch
//! has, a checkout's on no branch), its working tree is taken as the fork takes it (`fork::take`: tarae's own index,
//! untracked files in, ignored ones out, new ones past `snapshot::NEW_MAX` left out and said) and kept as one commit, the
//! tip its parent, on `refs/tarae/ended/<name>-<UTC time>` in the repository's git folder — it outlives the checkout. A
//! save that fails removes nothing. Never the user's own git state (index, HEAD, branches, stash, reflog): objects and
//! that ref only, no hooks, a fixed name with real dates, never signed.
//!
//! - **Brought back** (`:workspace-restore [name]`, its picker newest first): a checkout again at the saved parent — its
//!   branch reused when it's checked out nowhere and still there, else the next free name (`worktree::named_again`) —
//!   the saved tree laid over it unstaged by the fork's one way (`fork::lay`), then a session, its Claude fresh (the card
//!   says so); the ref goes once it's back.
//! - **Pruned** (`prune`, at start and after each end, on a worker): a save goes once it's past the newest
//!   `workspace.restore-count` and older than `workspace.restore-days` — whichever keeps more — said in the log; its
//!   objects stay until git's own gc.
//! - Nothing is told to Claude or a parent: the restore is the user's.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use unicode_width::UnicodeWidthStr as _;

use crate::editor::Editor;
use crate::fork::Files;
use crate::gitrepo::Repo;
use crate::offer::{Offer, What};
use crate::worktree::{Ending, Making};

/// Where the saves are, a ref each.
pub const REFS: &str = "refs/tarae/ended/";

/// How a checkout's files are taken: its HEAD and its tree (`fork::take`; a test's stand-in fails).
pub(crate) type Take = fn(&Path) -> Result<(String, crate::snapshot::Tree), String>;

/// A save, as its ref and commit say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    /// Its ref, whole · its commit · that commit's parent, the tip its checkout was on.
    pub at: String,
    pub commit: String,
    pub parent: String,
    /// The workspace, its branch (None: on no branch) and base, as it ended.
    pub name: String,
    pub branch: Option<String>,
    pub base: String,
    pub when: SystemTime,
    /// What its end card counted: rows changed, commits not in its base, commits on no other branch — and new files its
    /// tree left out.
    pub changed: usize,
    pub ahead: usize,
    pub lost: usize,
    pub left_out: usize,
}

impl Saved {
    /// Its ref's name past `REFS`: what `:workspace-restore` takes.
    pub fn leaf(&self) -> &str {
        self.at.strip_prefix(REFS).unwrap_or(&self.at)
    }
}

/// The trailers its commit's message carries, in order.
const TRAILERS: [&str; 7] = ["Workspace", "Branch", "Base", "Changed", "Ahead", "Lost", "Left-Out"];

/// `e`'s checkout saved before it's removed (`worktree::remove`, on its worker) — None: nothing would go
/// (`Ending::goes(both)`). Its files taken by `take`, the commit dated `now`. Err: nothing saved, nothing to remove.
pub(crate) fn keep(
    e: &Ending,
    both: bool,
    now: SystemTime,
    take: Take,
) -> Result<Option<(Saved, Files)>, String> {
    if !e.goes(both) {
        return Ok(None);
    }
    let said = |why: String| format!("its files couldn't be saved ({why}) — nothing removed");
    let (head, tree) = take(&e.root).map_err(said)?;
    // (what it left out, by top folder — only when it left some: two more of git's processes)
    let files = match tree.left_out {
        0 => Files::default(),
        n => crate::fork::counts(&e.root, &head, &tree).unwrap_or(Files { left_out: n, ..Files::default() }),
    };
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let lost = if both { e.lost.len() } else { 0 };
    let message = message(e, lost, files.left_out);
    let commit = commit_tree(&e.root, &tree.id, &head, &message, secs).map_err(said)?;
    let common = Repo::git_folder(&e.common);
    let leaf = format!("{}-{}", ref_safe(&e.name), stamp(secs));
    // (two ends of one name in a second: the next number)
    for k in 1..10 {
        let at = match k {
            1 => format!("{REFS}{leaf}"),
            k => format!("{REFS}{leaf}-{k}"),
        };
        let made =
            common.run_no_hooks(&["update-ref", "-m", "tarae: an ended workspace saved", &at, &commit, ""]);
        match made {
            Ok(_) => {
                let s = Saved {
                    at,
                    commit,
                    parent: head,
                    name: e.name.clone(),
                    branch: e.branch.clone(),
                    base: e.base.clone(),
                    when: UNIX_EPOCH + Duration::from_secs(secs),
                    changed: e.changed.len(),
                    ahead: e.ahead.len(),
                    lost,
                    left_out: files.left_out,
                };
                return Ok(Some((s, files)));
            }
            Err(_) if common.run(&["rev-parse", "--verify", "-q", &at], None).is_ok() => continue,
            Err(why) => return Err(said(why)),
        }
    }
    Err(said(format!("{REFS}{leaf} and nine after it are taken")))
}

/// Its commit: `tree` on `parent`, under tarae's name at `secs` (both dates), never signed — no `user.email` asked, no
/// gpg or ssh-keygen started (a pinentry could wait forever). No hooks; the repository's git.
fn commit_tree(root: &Path, tree: &str, parent: &str, message: &str, secs: u64) -> Result<String, String> {
    let date = std::ffi::OsString::from(format!("@{secs} +0000"));
    let env: Vec<(&str, std::ffi::OsString)> = vec![
        ("GIT_AUTHOR_NAME", "tarae".into()),
        ("GIT_AUTHOR_EMAIL", "tarae@localhost".into()),
        ("GIT_AUTHOR_DATE", date.clone()),
        ("GIT_COMMITTER_NAME", "tarae".into()),
        ("GIT_COMMITTER_EMAIL", "tarae@localhost".into()),
        ("GIT_COMMITTER_DATE", date),
    ];
    let args = ["commit-tree", "--no-gpg-sign", tree, "-p", parent, "-F", "-"];
    let cmd = crate::snapshot::command_env(root, &args, None, &env)?;
    let out = crate::gitrepo::finish(&cmd, Some(message.as_bytes()))?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

/// Its commit's message: a subject, then what `list` reads back as trailers.
fn message(e: &Ending, lost: usize, left_out: usize) -> String {
    let one = |s: &str| s.replace(['\n', '\r'], " ");
    let values = [
        one(&e.name),
        e.branch.as_deref().map(one).unwrap_or_default(),
        one(&e.base),
        e.changed.len().to_string(),
        e.ahead.len().to_string(),
        lost.to_string(),
        left_out.to_string(),
    ];
    let trailers: Vec<String> = TRAILERS.iter().zip(values).map(|(k, v)| format!("Tarae-{k}: {v}")).collect();
    format!("tarae: {} ended\n\n{}\n", one(&e.name), trailers.join("\n"))
}

/// A workspace's name as one part of a ref: letters, digits, `.`, `_` and `-` kept, anything else a `-`, no `.` first
/// nor `..` nor `.lock` at its end (git's rules).
fn ref_safe(name: &str) -> String {
    let mut s: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '-' }).collect();
    while s.contains("..") {
        s = s.replace("..", ".");
    }
    let s = s.trim_start_matches('.').trim_end_matches(".lock").trim_end_matches('.');
    if s.is_empty() { "workspace".into() } else { s.to_string() }
}

/// `20261108-143205`: `secs` as UTC.
fn stamp(secs: u64) -> String {
    let secs = secs as i64;
    let (days, s) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, mo, d) = crate::log::civil(days);
    format!("{y:04}{mo:02}{d:02}-{:02}{:02}{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// Save `s` taken back (its removal failed, its checkout still there) — if still at its commit; logged.
pub(crate) fn forget(common: &Path, s: &Saved) {
    if let Err(why) = Repo::git_folder(common).run_no_hooks(&["update-ref", "-d", &s.at, &s.commit]) {
        crate::log::line("session", "warning", &format!("{} couldn't be taken back: {why}", s.at));
    }
}

/// The repository's saves (its git folder `common`), newest first. On a worker.
pub(crate) fn list(common: &Path) -> Result<Vec<Saved>, String> {
    let format =
        "--format=%(refname)%00%(objectname)%00%(parent)%00%(committerdate:unix)%00%(contents:body)%1e";
    let out = Repo::git_folder(common).run(&["for-each-ref", "--sort=-committerdate", format, REFS], None)?;
    Ok(out.split('\x1e').filter_map(|r| saved_of(r.trim_start_matches('\n'))).collect())
}

/// One of `list`'s records — None: not one of tarae's (no workspace trailer, not one parent).
fn saved_of(record: &str) -> Option<Saved> {
    let mut f = record.split('\0');
    let (at, commit, parent, when, body) = (f.next()?, f.next()?, f.next()?, f.next()?, f.next()?);
    if parent.is_empty() || parent.contains(' ') {
        return None;
    }
    let trailer = |k: &str| {
        let key = format!("Tarae-{k}: ");
        body.lines().find_map(|l| l.strip_prefix(key.as_str())).map(str::trim)
    };
    let count = |k: &str| trailer(k).and_then(|v| v.parse().ok()).unwrap_or(0);
    Some(Saved {
        at: at.to_string(),
        commit: commit.to_string(),
        parent: parent.to_string(),
        name: trailer("Workspace")?.to_string(),
        branch: trailer("Branch").filter(|b| !b.is_empty()).map(str::to_string),
        base: trailer("Base").unwrap_or_default().to_string(),
        when: UNIX_EPOCH + Duration::from_secs(when.trim().parse().ok()?),
        changed: count("Changed"),
        ahead: count("Ahead"),
        lost: count("Lost"),
        left_out: count("Left-Out"),
    })
}

/// The saves past what's kept go: past the newest `count` and older than `days` by `now` — each if still at its
/// commit, said in the log. Their names. On a worker.
pub(crate) fn prune(common: &Path, count: usize, days: u64, now: SystemTime) -> Vec<String> {
    let Ok(all) = list(common) else { return Vec::new() };
    let young =
        |s: &Saved| now.duration_since(s.when).map_or(true, |age| age < Duration::from_secs(days * 86_400));
    let repo = Repo::git_folder(common);
    let gone: Vec<String> = (all.iter().enumerate())
        .filter(|(i, s)| *i >= count && !young(s))
        .filter(|(_, s)| repo.run_no_hooks(&["update-ref", "-d", &s.at, &s.commit]).is_ok())
        .map(|(_, s)| s.leaf().to_string())
        .collect();
    if !gone.is_empty() {
        let said =
            format!("ended workspaces' saves pruned (past {count} and {days} days): {}", gone.join(", "));
        crate::log::line("session", "info", &said);
    }
    gone
}

/// `4,210 new files weren't saved (past 5,000 at once): data/ 4,000, out/ 210.` — None: none left out.
pub(crate) fn left_said(f: &Files) -> Option<String> {
    if f.left_out == 0 {
        return None;
    }
    let tops: Vec<String> = f.left_top.iter().map(|(t, n)| format!("{t} {n}")).collect();
    let tops = if tops.is_empty() { String::new() } else { format!(": {}", tops.join(", ")) };
    Some(format!(
        "{} new {} saved (past {} at once){tops}.",
        f.left_out,
        if f.left_out == 1 { "file wasn't" } else { "files weren't" },
        crate::snapshot::NEW_MAX
    ))
}

// ── Bringing one back ───────────────────────────────────────────────────────────────────

/// What the restore card shows and its `y` makes.
#[derive(Clone, Debug)]
pub struct Restoring {
    pub saved: Saved,
    /// Its branch and checkout: `base_sha` the saved parent, `base` the saved base; who pays, the create card's way.
    pub making: Making,
    /// Its branch made (true), or the one there reused (`worktree::named_again`).
    pub fresh: bool,
    /// What the saved tree carries against its parent · commits of its parent not in `making.base`.
    pub files: Files,
    pub ahead: usize,
    /// Its saved base, gone from the repository since: `making.base` is the start checkout's branch instead, as
    /// `:session-new`'s would be (an end of it later compares with that).
    pub gone: Option<String>,
}

/// `:workspace-restore` — the picker of the repository's saves, newest first: name, branch, counts, age.
pub fn picker(ed: &mut Editor) {
    use crate::picker::{Action, FillError, Item};
    let root = ed.workspace.root.clone();
    let fill: crate::picker::Fill = Box::new(move || {
        let all = list(&crate::worktree::common_dir(&root)?)?;
        if all.is_empty() {
            return Err(FillError::Nothing(
                "nothing saved — a workspace ended with work in it is saved as it ends".into(),
            ));
        }
        let now = crate::clock::system();
        Ok((all.iter().zip(labels(&all)))
            .map(|(s, label)| Item {
                label,
                action: Action::Typed(format!("workspace-restore {}", s.leaf())),
                hint: crate::term::ago(now.duration_since(s.when).unwrap_or_default(), true),
                glyph: None,
            })
            .collect())
    });
    // (their ages a column that keeps its width: a long row is cut, never its time)
    let mut p = crate::picker::Picker::of("workspace-restore", "saved workspaces", Vec::new(), false)
        .without_preview();
    p.times = true;
    ed.open_picker(p, Some(fill));
}

/// The picker's rows: each name, its branch only where it isn't the name (`(no branch)` always) — a column only while
/// a row has one — and what its end counted; the columns by display width.
fn labels(all: &[Saved]) -> Vec<String> {
    let branch = |s: &Saved| match &s.branch {
        None => "(no branch)".to_string(),
        Some(b) if *b == s.name => String::new(),
        Some(b) => b.clone(),
    };
    let w = all.iter().map(|s| s.name.width()).max().unwrap_or(0);
    let bw = all.iter().map(|s| branch(s).width()).max().unwrap_or(0);
    let pad = |s: &str, w: usize| format!("{s}{}", " ".repeat(w.saturating_sub(s.width())));
    (all.iter())
        .map(|s| match bw {
            0 => format!("{}  {}", pad(&s.name, w), counts_said(s)),
            _ => format!("{}  {}  {}", pad(&s.name, w), pad(&branch(s), bw), counts_said(s)),
        })
        .collect()
}

/// `7 files · 2 commits` — what its end card counted.
fn counts_said(s: &Saved) -> String {
    let files = match s.changed {
        0 => None,
        1 => Some("1 file".to_string()),
        n => Some(format!("{n} files")),
    };
    let commits = (s.ahead.max(s.lost) > 0).then(|| crate::worktree::commits(s.ahead.max(s.lost)));
    files.into_iter().chain(commits).collect::<Vec<_>>().join(" · ")
}

/// `:workspace-restore <name>` — the save looked at on a worker (by its ref's name, else the newest of that workspace),
/// then the card: nothing made before `y`.
pub fn ask(ed: &mut Editor, which: &str) {
    let which = which.trim().to_string();
    let (root, start) = (ed.workspace.root.clone(), ed.start_root());
    let setting = ed.config.workspace_worktrees.clone();
    let said = format!("Looking at {} save…", crate::workspace::possessive(&which));
    let work = move || restoring(&root, &which, &setting, &start);
    ed.step(ed.events.shared_jobs(), "session", said, work, |ed, found| match found {
        Ok(mut r) => ed.api_check(ed.events.shared_jobs(), move |ed, _| {
            (r.making.billing, r.making.billing_ask, r.making.billing_note) = ed.new_session_ask();
            let card = card(&r, ed.weakness_due());
            ed.push_offer(card);
        }),
        Err(e) => ed.set_error(format!("restore: {e}")),
    });
}

/// The save `which` of the repository at `root`, and the checkout it would come back as — its base the saved one, else
/// (gone) the branch of the checkout tarae started in, `start`. On a worker.
fn restoring(root: &Path, which: &str, setting: &str, start: &Path) -> Result<Restoring, String> {
    let top = crate::gitpanel::top_of(root)?;
    let all = list(&crate::worktree::common_dir(&top)?)?;
    let saved = (all.iter().find(|s| s.leaf() == which))
        .or_else(|| all.iter().find(|s| s.name == which))
        .cloned()
        .ok_or_else(|| format!("nothing saved as {which} — :workspace-restore lists what is"))?;
    let (branch, path, reused) = crate::worktree::named_again(&top, &saved.name, &saved.parent, setting)?;
    let tree = crate::worktree::sha(&top, &format!("{}^{{tree}}", saved.commit))?;
    let files = crate::fork::changes(&top, &saved.parent, &tree)?;
    let there = |b: &str| !b.is_empty() && crate::worktree::sha(&top, &format!("{b}^{{commit}}")).is_ok();
    let (base, gone) = match there(&saved.base) {
        true => (saved.base.clone(), None),
        false => (crate::worktree::base_or_start(None, start)?, Some(saved.base.clone())),
    };
    let ahead = crate::worktree::count(&top, &format!("{base}..{}", saved.parent));
    let copies = crate::worktree::instructions(&top);
    let making = Making {
        top,
        branch,
        base,
        base_sha: saved.parent.clone(),
        path,
        files: (0, true),
        big: Vec::new(),
        builds: None,
        build_own: None,
        warm: None,
        copies,
        others: Vec::new(),
        billing: Default::default(),
        billing_ask: None,
        billing_note: None,
    };
    Ok(Restoring { saved, making, fresh: !reused, files, ahead, gone })
}

/// `4 changed, 2 new, 1 deleted`.
fn files_said(f: &Files) -> String {
    let parts: Vec<String> = [(f.changed, "changed"), (f.new, "new"), (f.deleted, "deleted")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, w)| format!("{n} {w}"))
        .collect();
    parts.join(", ")
}

/// Its card — `weakness`: it says `billing::WEAKNESS` when it bills the key (not said yet).
pub fn card(r: &Restoring, weakness: bool) -> Offer {
    let (s, m) = (&r.saved, &r.making);
    let (folder, under) = match (m.path.file_name(), m.path.parent()) {
        (Some(f), Some(p)) => (f.to_string_lossy().into_owned(), crate::allow::tilde(p)),
        _ => (crate::allow::tilde(&m.path), String::new()),
    };
    let when = crate::term::ago(crate::clock::system().duration_since(s.when).unwrap_or_default(), true);
    let files = match files_said(&r.files) {
        f if f.is_empty() => format!("Its files as they were {when}: its commit's, nothing uncommitted."),
        f => format!("Its files as they were {when}: {f}."),
    };
    let short = &m.base_sha[..m.base_sha.len().min(7)];
    let on = match r.fresh {
        true => format!("Branch {} at {short}", m.branch),
        false => format!("Its branch {}, still at {short}", m.branch),
    };
    let ahead = match r.ahead {
        0 => format!(", in {}", m.base),
        n => format!(" — {} not in {}", crate::worktree::commits(n), m.base),
    };
    let gone = match &r.gone {
        Some(b) if b.is_empty() => format!("\nIts base isn't known — {} instead.", m.base),
        Some(b) => format!("\nIts base {b} is gone — {} instead.", m.base),
        None => String::new(),
    };
    let mut q = format!(
        "{files}\n{on}{ahead}.{gone}\nIts checkout, {folder}/, goes in\n  {under}\n\nIts conversation isn't brought back — \
         its Claude starts fresh."
    );
    let warn = m.billing_said(&mut q, weakness);
    let o = Offer::new(
        format!("Bring back {}?", s.name),
        &q,
        "Bringing it back…",
        format!("Couldn't bring back {}", s.name),
        What::WorkspaceRestore(Box::new(r.clone())),
    );
    match warn {
        Some(lines) => o.warning(lines),
        None => o,
    }
}

/// `y` on the restore card: its checkout made and the saved tree laid on a worker, then its session in front, its chat
/// open — the save gone once it's back.
pub(crate) fn start(ed: &mut Editor, id: u64, r: Restoring) {
    ed.events.shared_jobs().spawn(move || {
        let made = restore(&r);
        move |ed: &mut Editor| landed(ed, id, r, made)
    });
}

/// The checkout again (on a worker): the saved tree laid at its parent (`fork::lay`), then its ref deleted — if still
/// at its commit; one that couldn't be is logged (pruned later).
fn restore(r: &Restoring) -> Result<(PathBuf, Vec<String>), String> {
    let (s, m) = (&r.saved, &r.making);
    let tree = crate::worktree::sha(&m.top, &format!("{}^{{tree}}", s.commit))?;
    let made = crate::fork::lay(m, &tree, r.fresh)?;
    if let Ok(common) = crate::worktree::common_dir(&m.top) {
        forget(&common, s);
    }
    Ok(made)
}

fn landed(ed: &mut Editor, id: u64, r: Restoring, made: Result<(PathBuf, Vec<String>), String>) {
    let (root, failed) = match made {
        Ok(made) => made,
        Err(e) => return ed.offer_finished(id, Err(e)),
    };
    ed.offer_finished(id, Ok(()));
    crate::warm::begin(ed, &root);
    let m = &r.making;
    let session = ed.workspace_add(root);
    if let Some(l) = ed.workspace_by_id(session) {
        (l.name, l.branch, l.base) = (m.branch.clone(), Some(m.branch.clone()), Some(m.base.clone()));
        l.claude.billing = m.billing;
    }
    if m.billing == crate::billing::Billing::Api {
        ed.weakness_shown();
    }
    ed.workspace_switch(session);
    crate::chat::open(ed);
    let when =
        crate::term::ago(crate::clock::system().duration_since(r.saved.when).unwrap_or_default(), true);
    ed.set_success(format!("Session {} — brought back as it was {when}", m.branch));
    if !failed.is_empty() {
        ed.set_warning(format!("not copied into it: {}", failed.join(", ")));
    }
}

#[cfg(test)]
mod tests;
