//! A session forked (`.sb/session-fork-plan.md`, its four decisions as recommended): "try another way from here" — a
//! new session from another's present, its commit, its changes in no commit and its conversation so far; both go on
//! apart, `:weave` and the overlap news comparing them. `:session-fork [name]`, `O` on the session list's row (`o`'s
//! shifted twin, as Helix's `o`/`O`). Your act only: no session tool forks.
//!
//! - **The card** (`card`) says the branch and where its checkout goes, what comes with it (the changes counted, a file
//!   staged there arriving unstaged, new files past `snapshot`'s cap left out and named), the buffers unsaved there
//!   (D1: `y` saves them first), the conversation and what its first message costs (the source's context read from the
//!   cache, the project's instructions written a second time — measured live: `switchcost::fork_cost`), that none of the
//!   source's answers come
//!   (D4), and who pays (the create card's words and `$`, preset to the source's). One card a source: asked again, the
//!   one up comes to the front.
//! - **Between turns only** (D2): while the source works (`keepclose::busy_turn`) the card says it waits; a `y` then is
//!   kept (`Forking.go`) and the fork starts once nothing holds and every save landed (`sync`, each event, in memory).
//!   From the `y` until the tree is written the source's sends wait (`holds_sends`, as a base move holds them), then go
//!   in order (`release`): no turn of its begins in between.
//! - **The tree** (`make`, a worker, tarae's own git, no hooks): HEAD read before and after `snapshot::snap` of the
//!   source (moved: taken again once), `worktree add` at it, the tree laid over HEAD by `read-tree -m -u`, the index
//!   back to HEAD (`read-tree HEAD`, `update-index --refresh`: nothing staged, new files untracked), then the
//!   instruction files with the source's bases. The source's files and index are never written; a failure after the
//!   checkout was made removes it and its branch.
//! - **The conversation** (`chat::fork_open`): its claude starts with `--resume <source's> --fork-session`, its first
//!   message says where it was forked from (D3). Until claude names the fork's own conversation the fork is kept with
//!   the workspace (`Session.fork`, `worktree::Kept.fork`), so `:restart` forks it still.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::editor::Editor;
use crate::event::WorkspaceId;
use crate::offer::{Offer, OfferState, What};
use crate::worktree::Making;

/// Where a fork came from — kept with its session until its own conversation's id lands (`begun`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForkFrom {
    /// The source's checkout and name.
    pub root: PathBuf,
    pub name: String,
    /// The source's conversation as it was forked, and its turn — None: it had none yet (the files alone).
    pub conversation: Option<String>,
    pub turn: usize,
    /// New files of the source's that weren't carried (past `snapshot`'s cap).
    pub left_out: usize,
}

impl ForkFrom {
    pub fn json(&self) -> Value {
        json!({ "root": self.root.to_str(), "name": self.name, "conversation": self.conversation,
            "turn": self.turn, "left_out": self.left_out })
    }

    pub fn from_json(v: &Value) -> Option<ForkFrom> {
        Some(ForkFrom {
            root: PathBuf::from(v["root"].as_str()?),
            name: v["name"].as_str()?.to_string(),
            conversation: v["conversation"].as_str().map(str::to_string),
            turn: v["turn"].as_u64().unwrap_or(0) as usize,
            left_out: v["left_out"].as_u64().unwrap_or(0) as usize,
        })
    }
}

/// What the source's working tree carries against its HEAD, as taken.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Files {
    pub changed: usize,
    pub new: usize,
    pub deleted: usize,
    /// Staged in the source's index: they arrive unstaged.
    pub staged: usize,
    /// New files not carried (past `snapshot`'s cap), and their top folders with how many each, the most first.
    pub left_out: usize,
    pub left_top: Vec<(String, usize)>,
}

impl Files {
    fn total(&self) -> usize {
        self.changed + self.new + self.deleted
    }
}

/// Top folders named for the files left out, at most.
const LEFT_TOP: usize = 3;
/// Unsaved buffers named on the card, at most (the rest counted).
const UNSAVED_SHOWN: usize = 3;

/// What the fork card shows and its `y` makes.
#[derive(Clone, Debug)]
pub struct Forking {
    pub source: WorkspaceId,
    pub from: ForkFrom,
    /// Its branch and checkout: `base_sha` the source's HEAD (taken again as it forks), `base` the source's base — the
    /// two siblings bring into one place — and who pays, the create card's way.
    pub making: Making,
    pub files: Files,
    /// The source's context, its first request's cache write (the instructions, where seen) and model, for what the
    /// first message costs.
    pub tokens: Option<u64>,
    pub instructions: Option<u64>,
    pub model: String,
    /// The source's agent: whether its conversation comes along (`Caps::fork`), else a new one, said.
    pub agent: &'static crate::agent::Row,
    /// Kept current by `sync`: the source's unsaved buffers (relative), what its work under way waits for.
    pub unsaved: Vec<String>,
    pub holds: Option<&'static str>,
    /// `y` given while something held: it forks once nothing does.
    pub go: bool,
    /// Its `y` started saves still landing.
    pub saving: bool,
    /// Why the last `y` couldn't go on (a save that failed).
    pub failed: Option<String>,
}

impl Forking {
    /// What `sync` keeps current differs.
    fn moved(&self, o: &Forking) -> bool {
        (self.holds, &self.unsaved, self.go, self.saving, &self.failed)
            != (o.holds, &o.unsaved, o.go, o.saving, &o.failed)
    }
}

fn source_of(ed: &Editor, id: WorkspaceId) -> Option<&crate::workspace::Workspace> {
    std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.id == id)
}

/// The source a fork card is about.
fn fork_of(o: &Offer) -> Option<WorkspaceId> {
    match &o.what {
        What::SessionFork(f) => Some(f.source),
        _ => None,
    }
}

/// `:session-fork [name]`, `O` on the list's row: session `source` looked at on a worker, then the card — nothing made
/// before `y`. Its card up already: brought to the front.
pub fn ask(ed: &mut Editor, source: WorkspaceId, name: Option<&str>) {
    if let Some(i) = ed.offers.iter().position(|o| fork_of(o) == Some(source)) {
        match i {
            0 => ed.card_focus_front(),
            i => ed.offer_to_front(i),
        }
        return ed.set_status("its fork card is up — y forks, n not now");
    }
    let Some(l) = source_of(ed, source) else { return ed.set_error("fork: no such session") };
    let (root, label) = (l.root.clone(), l.label().to_string());
    // (the source's base: both bring into it — the checkout tarae started in, its own branch)
    let base = l.base.clone().or_else(|| l.git_branch.clone());
    let setting = ed.config.workspace_worktrees.clone();
    let name = name.map(str::to_string);
    let said = format!("Looking at what a fork of {label} would take…");
    let work = move || measure(&root, &label, name.as_deref(), base, &setting);
    ed.step(ed.events.shared_jobs(), "session", said, work, move |ed, found| match found {
        Ok((making, files)) => {
            // (who pays: as a new session's card asks it, as the limits stand when it comes — D2's, Q4)
            ed.api_check(ed.events.shared_jobs(), move |ed, _| asked(ed, source, making, files));
        }
        Err(e) => ed.set_error(format!("fork: {e}")),
    });
}

/// The fork card's conversation line where the source's agent can't fork one (`Caps::fork`).
pub fn cant_fork(row: &crate::agent::Row) -> String {
    format!("Its conversation: a new one — {} can't fork a conversation.", row.name)
}

/// The card for a fork of `source` as measured.
fn asked(ed: &mut Editor, source: WorkspaceId, mut making: Making, files: Files) {
    let (asked, ask, note) = ed.new_session_ask();
    let Some(l) = source_of(ed, source) else { return ed.set_status("fork: that session ended meanwhile") };
    // (preset to the source's: the create card's words and `$`, one experience — Q4; its note only where it's about
    // what the fork runs on — your key until a reset is no line for a fork on the plan, nor the plan's 5-hour window
    // for one on the key)
    making.billing = l.claude.billing;
    // (who pays only where the source's row has tarae choose it, `Caps.billing`: agy signs in on its own)
    let bills = crate::llm::workspace_row(ed, l).caps.billing;
    making.billing_ask = ask.filter(|_| bills);
    making.billing_note = note.filter(|_| bills && asked == making.billing);
    let conversation = crate::chat::conversation_of(l);
    let usage = &l.claude.usage;
    let model = usage.conv.as_ref().and_then(|c| c.last.as_ref()).map(|r| r.model.clone());
    let model = model.or_else(|| l.claude.chat.as_ref().map(|c| c.model.clone())).unwrap_or_default();
    let (holds, unsaved, _, _) = state(ed, l);
    let f = Forking {
        source,
        from: ForkFrom {
            root: l.root.clone(),
            name: l.label().to_string(),
            turn: conversation.as_ref().map_or(0, |c| c.1),
            conversation: conversation.map(|c| c.0),
            left_out: files.left_out,
        },
        making,
        files,
        tokens: usage.context().map(|c| c.0),
        instructions: usage.conv.as_ref().and_then(|c| c.fixed.as_ref()).and_then(|r| r.counts.write),
        model,
        agent: crate::llm::workspace_row(ed, l),
        unsaved,
        holds,
        go: false,
        saving: false,
        failed: None,
    };
    let card = card(&f, ed.weakness_due(), ed.config.llm.cost);
    ed.push_offer(card);
}

/// Its branch and checkout, and what comes with it (on a worker): `name`, else `<source>-2` (the next free number).
pub fn measure(
    root: &Path,
    label: &str,
    name: Option<&str>,
    base: Option<String>,
    setting: &str,
) -> Result<(Making, Files), String> {
    let top = crate::gitpanel::top_of(root)?;
    head_of(&top)?;
    let (branch, path) = match name {
        Some(n) => crate::worktree::named(&top, n, setting)?,
        None => (2..100)
            .find_map(|k| crate::worktree::named(&top, &format!("{label}-{k}"), setting).ok())
            .ok_or_else(|| format!("no free name {label}-N — :session-fork <name>"))?,
    };
    let (head, tree) = take(&top)?;
    let files = counts(&top, &head, &tree)?;
    let copies = crate::worktree::instructions(&top);
    let making = Making {
        top,
        branch,
        base: base.unwrap_or_else(|| head.clone()),
        base_sha: head,
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
    Ok((making, files))
}

fn head_of(top: &Path) -> Result<String, String> {
    crate::worktree::sha(top, "HEAD").map_err(|_| "it has no commit yet — nothing to fork from".to_string())
}

/// The source's working tree as a tree (`snapshot::snap`: its own index, never the real one) at the HEAD it sits on —
/// HEAD read before and after; moved meanwhile, taken again once. An ended workspace's save takes it the same way
/// (`endsave::keep`).
pub(crate) fn take(top: &Path) -> Result<(String, crate::snapshot::Tree), String> {
    for _ in 0..2 {
        let before = head_of(top)?;
        let tree = crate::snapshot::snap(top)?;
        if head_of(top)? == before {
            return Ok((before, tree));
        }
    }
    Err("its HEAD moved while its files were taken, twice — try again".into())
}

/// What tree `tree` carries against commit `head`: files changed, new and deleted — plumbing only.
pub(crate) fn changes(top: &Path, head: &str, tree: &str) -> Result<Files, String> {
    let diff = crate::gitrepo::git(
        top,
        &["diff-tree", "-r", "-z", "--no-renames", "--name-status", head, tree],
        None,
    )?;
    let mut f = Files::default();
    let mut parts = diff.split('\0');
    while let (Some(status), Some(_)) = (parts.next(), parts.next()) {
        match status {
            "A" => f.new += 1,
            "D" => f.deleted += 1,
            "" => break,
            _ => f.changed += 1,
        }
    }
    Ok(f)
}

/// What `tree` carries against `head`, and what the source's index has staged — plumbing only (nothing refreshed).
pub(crate) fn counts(top: &Path, head: &str, tree: &crate::snapshot::Tree) -> Result<Files, String> {
    let git = |args: &[&str]| crate::gitrepo::git(top, args, None);
    let mut f = Files { left_out: tree.left_out, ..changes(top, head, &tree.id)? };
    let staged = git(&["diff-index", "--cached", "--name-only", "-z", head])?;
    f.staged = staged.split('\0').filter(|p| !p.is_empty()).count();
    if f.left_out > 0 {
        // (the new files not in the tree, by their top folder)
        let untracked = git(&["ls-files", "-z", "--others", "--exclude-standard"])?;
        let inside = git(&["ls-tree", "-r", "-z", "--name-only", &tree.id])?;
        let inside: std::collections::HashSet<&str> = inside.split('\0').collect();
        let mut by: Vec<(String, usize)> = Vec::new();
        for p in untracked.split('\0').filter(|p| !p.is_empty() && !inside.contains(p)) {
            let top = match p.split_once('/') {
                Some((d, _)) => format!("{d}/"),
                None => p.to_string(),
            };
            match by.iter_mut().find(|(t, _)| *t == top) {
                Some(t) => t.1 += 1,
                None => by.push((top, 1)),
            }
        }
        crate::sorting::by_key(&mut by, |(t, n)| (std::cmp::Reverse(*n), t.clone()));
        by.truncate(LEFT_TOP);
        f.left_top = by;
    }
    Ok(f)
}

/// The new checkout `new` (a fresh `worktree add` at `head`) given `tree`'s files: laid over HEAD by git's two-way
/// merge (changed, new, deleted, modes and links as the tree has them), then the index back to HEAD with the files left
/// as they are — nothing staged, new files untracked. No hooks; on a worker.
pub(crate) fn write_tree(new: &Path, head: &str, tree: &str) -> Result<(), String> {
    let git = |args: &[&str]| crate::gitrepo::git_no_hooks(new, args);
    git(&["read-tree", "-m", "-u", head, tree])?;
    git(&["read-tree", head])?;
    // (the stat cache only: a file it can't refresh is still there, as written)
    let _ = git(&["update-index", "-q", "--refresh"]);
    Ok(())
}

/// What `make` made.
#[derive(Debug)]
pub(crate) struct Made {
    pub root: PathBuf,
    /// Instruction files that couldn't be copied, said.
    pub failed: Vec<String>,
    pub files: Files,
    /// The source's HEAD it was made at.
    pub head: String,
}

/// The fork's checkout (on a worker): the source's tree taken now, then laid as a checkout of its own at its HEAD
/// (`lay`).
pub(crate) fn make(m: &Making) -> Result<Made, String> {
    let (head, tree) = take(&m.top)?;
    let files = counts(&m.top, &head, &tree)?;
    let m = Making { base_sha: head.clone(), ..m.clone() };
    let (root, failed) = lay(&m, &tree.id, true)?;
    Ok(Made { root, failed, files, head })
}

/// The one way a checkout is made from a tree (on a worker — a fork's, an ended workspace's brought back,
/// `endsave::restore`): `m.branch` at `m.base_sha` in a checkout of its own — a new branch (`fresh`), else the one there
/// — `tree` written there (`write_tree`), the instruction files after it with `m.top`'s bases. Past the checkout's
/// making, a failure takes it away again, and a branch it made — nothing left half made. Its real path, and the
/// instruction files that couldn't be copied.
pub(crate) fn lay(m: &Making, tree: &str, fresh: bool) -> Result<(PathBuf, Vec<String>), String> {
    match fresh {
        true => crate::worktree::add(m)?,
        false => {
            crate::worktree::add_on(m)?;
            // (a branch there moved since the card: its tree laid on the new tip would stage the undoing of its commits)
            if crate::worktree::sha(&m.path, "HEAD").ok().as_deref() != Some(m.base_sha.as_str()) {
                undo(m, false);
                return Err("its branch moved since the card was shown — nothing made".into());
            }
        }
    }
    if let Err(e) = write_tree(&m.path, &m.base_sha, tree) {
        undo(m, fresh);
        let what = if fresh { "its checkout and branch were" } else { "its checkout was" };
        return Err(format!("its files couldn't be written ({e}) — {what} taken back"));
    }
    let failed = crate::worktree::copy_instructions(&m.top, &m.path, &m.copies);
    crate::instructions::keep_bases_from(&m.top, &m.path, &m.copies);
    let root = std::fs::canonicalize(&m.path).unwrap_or_else(|_| m.path.clone());
    Ok((root, failed))
}

/// A checkout `lay` began taken away, and its branch when it made it (`fresh`) — no hooks, the repository's git folder
/// named.
fn undo(m: &Making, fresh: bool) {
    let Ok(common) = crate::worktree::common_dir(&m.top) else { return };
    let repo = crate::gitrepo::Repo::git_folder(&common);
    if let Some(p) = m.path.to_str() {
        let _ = repo.run_no_hooks(&["worktree", "remove", "--force", p]);
    }
    if fresh {
        let _ = repo.run_no_hooks(&["branch", "-D", "--", &m.branch]);
    }
}

/// What holds a fork of `l` now (`keepclose::busy_turn`), its unsaved buffers (relative, and their ids) and its saves
/// landing. (Every buffer under `l`'s root: no lookup of `docpaths` answers a folder, so the one scan by path here.)
fn state(
    ed: &Editor,
    l: &crate::workspace::Workspace,
) -> (Option<&'static str>, Vec<String>, Vec<crate::document::DocId>, usize) {
    let holds = crate::keepclose::busy_turn(ed, l);
    let (mut unsaved, mut ids, mut saving) = (Vec::new(), Vec::new(), 0);
    for d in ed.docs.iter() {
        let Some(p) = d.path.as_ref().filter(|p| p.starts_with(&l.root)) else { continue };
        saving += d.saving as usize;
        if d.is_modified() {
            unsaved.push(p.strip_prefix(&l.root).unwrap_or(p).to_string_lossy().into_owned());
            ids.push(d.id);
        }
    }
    crate::sorting::sort(&mut unsaved);
    (holds, unsaved, ids, saving)
}

/// `a, b, c +2 more`.
fn listed(names: &[String]) -> String {
    let mut s = names.iter().take(UNSAVED_SHOWN).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > UNSAVED_SHOWN {
        s += &format!(" +{} more", names.len() - UNSAVED_SHOWN);
    }
    s
}

/// `1 file` · `7 files`.
fn files(n: usize) -> String {
    if n == 1 { "1 file".into() } else { format!("{n} files") }
}

/// What its work under way waits for, said of the source: `main's turn ends`, `you answer what it asks`.
fn holds_said(holds: &str, src: &str) -> String {
    holds.replacen("its ", &format!("{} ", crate::workspace::possessive(src)), 1)
}

/// `7 files not committed (2 new, 1 deleted)`.
fn with_said(f: &Files) -> String {
    let parts: Vec<String> = [(f.new, "new"), (f.deleted, "deleted")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, w)| format!("{n} {w}"))
        .collect();
    let parts = if parts.is_empty() { String::new() } else { format!(" ({})", parts.join(", ")) };
    format!("{} not committed{parts}", files(f.total()))
}

/// `1 staged in main arrives unstaged` — None: none staged.
fn staged_said(f: &Files, src: &str) -> Option<String> {
    let verb = if f.staged == 1 { "arrives" } else { "arrive" };
    (f.staged > 0).then(|| format!("{} staged in {src} {verb} unstaged", f.staged))
}

/// `5,210 new files of main's aren't carried (past 5,000 at once): data/ 4,000, out/ 1,210` — None: none left out.
fn left_said(f: &Files, src: &str) -> Option<String> {
    if f.left_out == 0 {
        return None;
    }
    let tops: Vec<String> = f.left_top.iter().map(|(t, n)| format!("{t} {n}")).collect();
    let tops = if tops.is_empty() { String::new() } else { format!(": {}", tops.join(", ")) };
    Some(format!(
        "{} new {} of {} aren't carried (past {} at once){tops}.",
        f.left_out,
        if f.left_out == 1 { "file" } else { "files" },
        crate::workspace::possessive(src),
        crate::snapshot::NEW_MAX
    ))
}

/// The fork card for `f` — `weakness`: it says `billing::WEAKNESS` when it bills the key (not said yet).
/// `money`: its first message's price said (`llm.cost`).
pub fn card(f: &Forking, weakness: bool, money: bool) -> Offer {
    let (m, src) = (&f.making, f.from.name.as_str());
    let (folder, under) = match (m.path.file_name(), m.path.parent()) {
        (Some(n), Some(p)) => (n.to_string_lossy().into_owned(), crate::allow::tilde(p)),
        _ => (crate::allow::tilde(&m.path), String::new()),
    };
    let short = &m.base_sha[..m.base_sha.len().min(7)];
    let brings = match m.base == src {
        true => format!("it brings into {}", m.base),
        false => format!("it brings into {}, as {src}", m.base),
    };
    let mut q = format!(
        "Branch {} at {short}, {} HEAD — {brings}.\nIts checkout, {folder}/, goes in\n  {under}\n",
        m.branch,
        crate::workspace::possessive(src)
    );
    let mut warnings = Vec::new();
    let mut line = |q: &mut String, text: String, warn: bool| {
        if warn {
            let at = q.split('\n').count();
            warnings.push(at..at + 1);
        }
        *q += &format!("\n{text}");
    };
    let with = match f.files.total() {
        0 => format!(
            "With it: nothing uncommitted — {} files are its commit's.",
            crate::workspace::possessive(src)
        ),
        _ => match staged_said(&f.files, src) {
            Some(s) => format!("With it: {} — {s}.", with_said(&f.files)),
            None => format!("With it: {}.", with_said(&f.files)),
        },
    };
    line(&mut q, with, false);
    if let Some(left) = left_said(&f.files, src) {
        line(&mut q, left, true);
    }
    if !f.unsaved.is_empty() {
        line(&mut q, format!("Unsaved in {src}: {} — y saves them first.", listed(&f.unsaved)), false);
    }
    let conversation = match &f.from.conversation {
        None => "Its conversation: none yet — a new one.".to_string(),
        // (an agent that can't fork one: its fork starts a new one — `chat::ensure_proc` resumes none; agy 1.5.9's
        // `session/fork` answered -32601)
        Some(_) if !f.agent.caps.fork => cant_fork(f.agent),
        Some(_) => {
            let prices = &crate::sandboxpack::get().price;
            let cost = crate::switchcost::fork_cost(f.tokens, f.instructions, &f.model, prices);
            let cost = match cost {
                Some(c) => crate::switchcost::fork_said(&c, m.billing, src, money),
                None => {
                    "read again, plus the project's instructions a second time; its size isn't reported yet."
                        .into()
                }
            };
            format!(
                "Its conversation: {}, at turn {} — {cost}",
                crate::workspace::possessive(src),
                f.from.turn
            )
        }
    };
    line(&mut q, conversation, false);
    line(
        &mut q,
        format!(
            "None of {} answers come with it: its runs, reads and hosts ask again.",
            crate::workspace::possessive(src)
        ),
        false,
    );
    let mut warn = m.billing_said(&mut q, weakness);
    let state = match (&f.failed, f.go, f.holds) {
        (Some(e), ..) => Some((format!("{e} — y tries again."), true)),
        (None, true, _) if f.saving => {
            let n = f.unsaved.len();
            Some((format!("y given — saving {n} {}…", if n == 1 { "buffer" } else { "buffers" }), false))
        }
        (None, true, Some(h)) => Some((format!("y given — it forks once {}.", holds_said(h, src)), false)),
        (None, true, None) => Some(("y given — it forks now.".into(), false)),
        (None, false, Some(h)) => Some((format!("Not yet: y forks once {}.", holds_said(h, src)), false)),
        (None, false, None) => None,
    };
    if let Some((text, bad)) = state {
        q += "\n";
        let at = q.split('\n').count();
        q += &format!("\n{text}");
        if bad {
            warn = warn.or(Some(at..at + 1));
        }
    }
    let mut o = Offer::new(
        format!("Fork {src} as {}", m.branch),
        &q,
        "Forking…",
        "Couldn't fork".into(),
        What::SessionFork(Box::new(f.clone())),
    );
    o.warnings = warnings;
    // (where it would be cut: where its checkout goes first — the rest is what it carries and costs, so `y` answers at
    // once on a narrow screen)
    let cut = 1..3;
    let o = o.folding(vec![cut]);
    match warn {
        Some(lines) => o.warning(lines),
        None => o,
    }
}

/// Card `id` says `f` now — what it decides changed, so its `rev` moves (a `y` answers what was in sight).
fn replace(ed: &mut Editor, id: u64, f: Forking) {
    let new = card(&f, ed.weakness_due(), ed.config.llm.cost);
    let Some(o) = ed.offers.iter_mut().find(|o| o.id == id) else { return };
    (o.question, o.what, o.warning, o.warnings) = (new.question, new.what, new.warning, new.warnings);
    o.rev += 1;
}

/// Whether session `id`'s sends wait for a fork of it (D2, as a base move holds them — `Workspace.moving`): from its
/// `y`, kept or going, until the fork's tree is written — so nothing its Claude does after the `y` is in it, and a
/// turn's end lets no queued message in before the fork.
pub fn holds_sends(ed: &Editor, id: WorkspaceId) -> bool {
    ed.offers.iter().any(|o| match (&o.what, &o.state) {
        (What::SessionFork(f), OfferState::Asking) => f.source == id && f.go,
        (What::SessionFork(f), OfferState::Installing(_)) => f.source == id,
        _ => false,
    })
}

/// A fork of `source` made, failed or its `y` taken back: what its sends held goes, in order (`chat::send_queued`, as a
/// base move's landing lets them go).
pub(crate) fn release(ed: &mut Editor, source: WorkspaceId) {
    if source_of(ed, source).is_none() || holds_sends(ed, source) {
        return;
    }
    ed.with_workspace(source, crate::chat::send_queued);
}

/// Each event, in memory only (`busy_turn`, `ed.docs`): each fork card up kept to what holds it, what's unsaved, its
/// saves; one whose source ended goes, said; one given `y` that nothing holds any more forks.
pub fn sync(ed: &mut Editor) {
    if !ed.offers.iter().any(|o| fork_of(o).is_some()) {
        return;
    }
    let (mut moved, mut gone, mut start) = (Vec::new(), Vec::new(), None);
    for o in ed.offers.iter().filter(|o| matches!(o.state, OfferState::Asking)) {
        let What::SessionFork(f) = &o.what else { continue };
        let Some(l) = source_of(ed, f.source) else {
            gone.push((o.id, f.from.name.clone()));
            continue;
        };
        let (holds, unsaved, _, saving) = state(ed, l);
        let mut g = Forking { holds, unsaved, ..(**f).clone() };
        // (its saves landed: one that failed leaves its buffer unsaved — the `y` waits for a new one, said)
        if g.saving && saving == 0 {
            g.saving = false;
            if g.go && !g.unsaved.is_empty() {
                g.go = false;
                g.failed = Some(format!("{} wasn't saved", listed(&g.unsaved)));
            }
        }
        if g.go && g.holds.is_none() && g.unsaved.is_empty() && saving == 0 && start.is_none() {
            start = Some(o.id);
        }
        if g.moved(f) {
            moved.push((o.id, f.go && !g.go, g));
        }
    }
    for (id, name) in gone {
        ed.offers.retain(|o| o.id != id);
        ed.card_how(id, format!("{name} ended — nothing forked"));
    }
    for (id, dropped, g) in moved {
        // (a save that failed took its `y` back: what that held goes)
        let source = g.source;
        replace(ed, id, g);
        if dropped {
            release(ed, source);
        }
    }
    if let Some(id) = start {
        ed.offer_accept_id(id);
    }
}

/// `y` on the front fork card while something holds it (D2) or buffers are unsaved (D1): kept as given, the saves
/// started, the card saying so — true. False: nothing holds, it forks now.
pub(crate) fn yes_waits(ed: &mut Editor) -> bool {
    let Some(o) = ed.offers.first() else { return false };
    let What::SessionFork(f) = &o.what else { return false };
    let Some(l) = source_of(ed, f.source) else { return false };
    let (holds, unsaved, docs, saving) = state(ed, l);
    if holds.is_none() && unsaved.is_empty() && saving == 0 {
        return false;
    }
    let (id, focused) = (o.id, ed.card_focus.is_some_and(|c| c.0 == o.id));
    if f.go {
        let why = match holds {
            Some(h) => format!("y given already — it forks once {}", holds_said(h, &f.from.name)),
            None => "y given already — it forks once its saves land".into(),
        };
        ed.offer_refused(why);
        return true;
    }
    let mut g = Forking { holds, unsaved, go: true, failed: None, ..(**f).clone() };
    // (D1: what you see is what it forks — each unsaved buffer there saved first)
    let failed: Vec<String> = (docs.into_iter())
        .filter_map(|d| ed.save_in_background(d, false, crate::saver::Then::Command).err())
        .collect();
    g.saving = !g.unsaved.is_empty() || saving > 0;
    if !failed.is_empty() {
        (g.go, g.failed) = (false, Some(failed.join("; ")));
    }
    replace(ed, id, g);
    // (your own `y`: the card keeps the keys through what it made it say)
    if focused && let Some(o) = ed.offers.iter().find(|o| o.id == id) {
        ed.card_focus = Some((id, o.rev));
    }
    true
}

/// `y`, nothing holding: the source's conversation as of now with it, the checkout made on a worker, then its session
/// in front, its chat forking that conversation.
pub(crate) fn start(ed: &mut Editor, id: u64, mut f: Forking) {
    let Some(l) = source_of(ed, f.source) else {
        return ed.offer_finished(id, Err(format!("{} ended — nothing forked", f.from.name)));
    };
    let conversation = crate::chat::conversation_of(l);
    f.from.turn = conversation.as_ref().map_or(0, |c| c.1);
    f.from.conversation = conversation.map(|c| c.0);
    let saved = crate::chat::saved_of(l);
    let m = f.making.clone();
    ed.events.shared_jobs().spawn(move || {
        let made = make(&m);
        move |ed: &mut Editor| landed(ed, id, f, saved, made)
    });
}

fn landed(
    ed: &mut Editor,
    id: u64,
    f: Forking,
    saved: Option<crate::chats::Saved>,
    made: Result<Made, String>,
) {
    let made = match made {
        Ok(made) => made,
        Err(e) => {
            ed.offer_finished(id, Err(e));
            return release(ed, f.source);
        }
    };
    ed.offer_finished(id, Ok(()));
    crate::warm::begin(ed, &made.root);
    let session = ed.workspace_add(made.root.clone());
    let (m, from) = (&f.making, ForkFrom { left_out: made.files.left_out, ..f.from.clone() });
    if let Some(l) = ed.workspace_by_id(session) {
        (l.name, l.branch, l.base) = (m.branch.clone(), Some(m.branch.clone()), Some(m.base.clone()));
        l.claude.billing = m.billing;
        l.claude.fork = Some(from.clone());
    }
    if m.billing == crate::billing::Billing::Api {
        ed.weakness_shown();
    }
    ed.workspace_switch(session);
    crate::chat::fork_open(ed, saved);
    ed.workspaces_keep();
    let src = &from.name;
    let with = match made.files.total() {
        0 => String::new(),
        _ => match staged_said(&made.files, src) {
            Some(s) => format!(" · {} came with it; {s}", with_said(&made.files)),
            None => format!(" · {} came with it", with_said(&made.files)),
        },
    };
    ed.set_success(format!(
        "Session {} — forked from {src} at {}{with}",
        m.branch,
        crate::gitrepo::short_sha(&made.head)
    ));
    if let Some(left) = left_said(&made.files, src) {
        ed.set_warning(left);
    }
    if !made.failed.is_empty() {
        ed.set_warning(format!("not copied into it: {}", made.failed.join(", ")));
    }
    // (its tree written and the fork's claude started on the conversation as it was: the source's sends go)
    release(ed, f.source);
}

/// The fork's own conversation began (claude named it), or a new one of yours took its place: it's no fork waiting any
/// more — kept so.
pub fn begun(ed: &mut Editor) {
    if ed.workspace.claude.fork.take().is_some() {
        ed.workspaces_keep();
    }
}

/// The fork's row in its chat, under the source's rows.
pub fn noted(from: &ForkFrom) -> String {
    match &from.conversation {
        Some(_) => format!("forked from {} at its turn {} — the two go on apart", from.name, from.turn),
        None => format!("forked from {} — its files; no conversation yet", from.name),
    }
}

/// What its first message tells Claude (D3), with `checkpoints.tell`: where it was forked from, that the other goes on
/// apart — and new files that weren't carried.
pub fn told(from: &ForkFrom, me: &str) -> String {
    let at = match &from.conversation {
        Some(_) => format!(" at its turn {}", from.turn),
        None => String::new(),
    };
    let left = match from.left_out {
        0 => String::new(),
        n => format!(" {n} new files of {} weren't carried.", crate::workspace::possessive(&from.name)),
    };
    format!(
        "This session was forked from session {}{at}: it's now {me}, in a checkout and branch of its own with {} \
         files as they were. {} goes on apart — its work from here isn't yours.{left}",
        from.name,
        crate::workspace::possessive(&from.name),
        from.name
    )
}

#[cfg(test)]
mod tests;
