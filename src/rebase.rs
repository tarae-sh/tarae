//! Rebasing a session onto its base (roadmap #3): `:workspace-bring` when the base moved since the session
//! left it — a card says how far (`What::Rebase`), and `y` rebases in the session's checkout once it's been
//! looked at (`rebase_check`: changes in no commit → `What::RebaseHeld`, `s` sets them aside for it · `g` the git
//! tab; a run
//! in the panel → it waits its turn). tarae runs git itself (Claude's sandbox can't write `.git`), in the task
//! panel. Clean: the bring card (`What::Rebased` —
//! bring · preflight, then bring · not now). A conflict: the rebase stops, and the session's own Claude is
//! asked in its chat to resolve the files — no git, no commit. When its turn ends with no markers left in
//! them, a review shows them against the commit being replayed and a card (`What::RebaseResolved`) offers
//! continue (staged, `git rebase --continue` — it may stop again: the same loop) or abort. Markers left: the
//! files are named. While it's under way the session's tab, chip and row say `rebasing n/m`, and
//! `:workspace-end` and `:restart` wait for it; `:workspace-bring` puts its card up again. `:rebase-continue` ·
//! `:rebase-abort` do the card's work by hand, in the session rebasing whichever is in front — and take up a
//! rebase tarae didn't start (one from before a restart).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::editor::Editor;
use crate::gitrepo::git;
use crate::offer::{Offer, What};

/// How often the step (n of m) is read while git runs.
const POLL: Duration = Duration::from_millis(150);
/// Most bytes of each side's diff Claude is sent (it reads the rest itself).
const SIDE_MAX: usize = 60_000;

/// git as tarae runs it for a rebase — outside the sandbox, so with no hooks: a `core.hooksPath` inside the
/// checkout (`.githooks`) is a folder the session's Claude can write, and pre-rebase, post-checkout,
/// post-rewrite and the commit hooks would run what it put there unsandboxed (review, 2026-10-04 — the same
/// flag as `gitrepo::NO_HOOKS`, here as a shell line for the task panel). And without
/// the conflict advice: its `hint:` lines tell you to `git add` and `--continue` yourself — tarae's step here —
/// and made the task panel tall enough to push the resolved card over the review (a shot). On `repo`, named
/// (`gitrepo::Repo::shell_line`: what's held, its values in the run's environment).
fn git_line(repo: &crate::gitrepo::Repo) -> crate::gitrepo::Line {
    let mut line = repo.shell_line();
    line.text = format!("{} {} -c advice.mergeConflict=false", line.text, crate::gitrepo::NO_HOOKS.join(" "));
    line
}

/// `the quick preflight` (the project has two sets) or `the preflight`.
fn preflight_said(quick: &Option<Vec<String>>) -> &'static str {
    match quick {
        Some(_) => "the quick preflight",
        None => "the preflight",
    }
}

/// A bring card's quick steps, a paragraph of its own it folds first (`Offer.folds`): what `:preflight-full` adds is
/// for before publishing.
fn steps_folded(card: &mut Offer, quick: Option<Vec<String>>) {
    let Some(steps) = quick else { return };
    let at = card.question.split('\n').count();
    card.question.push_str(&format!("\n\nQuick: {} — :preflight-full runs the rest.", steps.join(", ")));
    card.folds.push(at..at + 2);
}

/// What a rebase step that didn't start does: says why (and a start leaves no rebase behind).
type Unstarted = Box<dyn FnOnce(&mut Editor, String) + Send>;

/// A preflight a bring card's `p` ran that failed (`Workspace.bring_failed`): which (`title`), its failed step's
/// output, and the commit it tested (read as `p` started it — else, none read, the HEAD its card was graded at).
#[derive(Clone, Debug)]
pub struct Failed {
    pub title: String,
    pub how: Unpassed,
    pub output: Option<(String, Vec<String>)>,
    pub head: Option<String>,
}

/// How a preflight didn't pass, beside its output: whether a step failed, and the steps refused before they started,
/// each said whole (`preflight::Preflight::not_run`, #354 — not the change's failure, never a pass).
#[derive(Clone, Debug, Default)]
pub struct Unpassed {
    pub failed: bool,
    pub not_run: Vec<String>,
}

/// A session whose base moved since it left it — what the rebase card says (`y` rebases), and once it's
/// rebased, what the bring card says (`moved` 0 then).
#[derive(Clone, Debug)]
pub struct Moved {
    pub root: PathBuf,
    /// The session's name, its branch and its base.
    pub name: String,
    pub branch: String,
    pub base: String,
    /// The base's commits the branch hasn't · the branch's the base hasn't.
    pub moved: usize,
    pub ahead: usize,
}

impl Moved {
    /// The one bring card when its base moved (polish landing 4: rebase → preflight → bring on one `y`, where a
    /// rebase card, a rebased card and its `p` asked three times): what `y` does, decided, and one line of why; the
    /// quick set's steps (`quick`) a paragraph it folds first.
    pub fn card(&self, quick: Option<Vec<String>>) -> Offer {
        let (base, branch) = (&self.base, &self.branch);
        let q = format!(
            "Rebase {branch} onto {base}'s tip, run {}, then bring it in?\n\
             {base} moved {} since {branch} left it; {branch} has {}. A conflict or a failed preflight stops here.",
            preflight_said(&quick),
            crate::worktree::commits(self.moved),
            crate::worktree::commits(self.ahead),
        );
        let mut card = Offer::new(
            format!("Bring {branch} into {base}"),
            &q,
            "Rebasing…",
            format!("Couldn't rebase {branch}"),
            What::Rebase(Box::new(self.clone())),
        );
        card.decided = Some(0..1);
        steps_folded(&mut card, quick);
        card
    }

    /// The bring card of one already on its base's tip — asked (#155), or stopped after its rebase for what wants
    /// a person (`rebased`: high-risk changes not looked at, a message a resolution kept): `y` runs the quick
    /// preflight (`quick`: its steps, when the project has two sets) and brings once it passes — the record's 80 %
    /// `p`; a bring with no preflight is the failed card's `y bring anyway` only (4 % of answers were that `y`).
    pub fn bring_card(&self, quick: Option<Vec<String>>, rebased: bool) -> Offer {
        let (base, them) = (&self.base, if self.ahead == 1 { "it" } else { "them" });
        let q = format!(
            "Run {}, then bring {them} into {base}?\n{} on {base}'s tip. A failed preflight stops here.",
            preflight_said(&quick),
            crate::worktree::commits(self.ahead),
        );
        let title = match rebased {
            true => format!("Rebased {} onto {base}", self.branch),
            false => format!("Bring {} into {base}", self.branch),
        };
        let mut card = Offer::new(
            title,
            &q,
            "Bringing…",
            format!("Couldn't bring {}", self.branch),
            What::Rebased(Box::new(self.clone())),
        );
        card.decided = Some(0..1);
        steps_folded(&mut card, quick);
        card
    }

    /// The bring card again once its preflight passed but its HEAD moved since that preflight began (#373: a commit
    /// made in the git tab while it ran): nothing landed — what lands is the commit tested (`tested`), never another
    /// — and `y` runs the preflight again on what's there now (`head`), said in the warning colour.
    pub fn untested_card(&self, quick: Option<Vec<String>>, tested: &str, head: &str) -> Offer {
        let (base, branch, them) = (&self.base, &self.branch, if self.ahead == 1 { "it" } else { "them" });
        let short = |s: &str| s.get(..7).unwrap_or(s).to_string();
        let q = format!(
            "Run {} again, then bring {them} into {base}?\n\
             {branch} moved while its preflight ran — it tested {}, {branch} is at {} now. Nothing brought: {} \
             wasn't tested.\n{} on {base}'s tip. A failed preflight stops here.",
            preflight_said(&quick),
            short(tested),
            short(head),
            short(head),
            crate::worktree::commits(self.ahead),
        );
        let mut card = Offer::new(
            format!("Bring {branch} into {base}"),
            &q,
            "Bringing…",
            format!("Couldn't bring {branch}"),
            What::Rebased(Box::new(self.clone())),
        );
        card.decided = Some(0..1);
        // (`warning`, not `warnings`: those make it the failed card, its `y` a bring anyway with no preflight)
        card.warning = Some(1..2);
        steps_folded(&mut card, quick);
        card
    }

    /// The bring card once the preflight its `p` ran (`title`) failed: what's decided is a bring anyway (its
    /// first paragraph, `y bring anyway`), and the risk — the first test that failed and its panic line — in the
    /// warning colour, up front (frictions29 91: at the card's foot, under the passing card's words, a driver read
    /// it as the plain one and brought a failing commit); `failed` — the failed step's output (its buffer's
    /// title, its lines) — behind `v`. A step refused before it started (`how`, #354) is said in its own words, its
    /// reason and way out whole: up front when nothing failed, after the failure when something did.
    pub fn failed_card(&self, title: &str, how: &Unpassed, failed: Option<(String, Vec<String>)>) -> Offer {
        let (base, them) = (&self.base, if self.ahead == 1 { "it" } else { "them" });
        let first = failed.as_ref().and_then(|(_, lines)| crate::preflight::failed_test(lines));
        let mut not_run = how.not_run.iter();
        let lead = match first.is_none() && !how.failed {
            true => not_run.next(),
            false => None,
        };
        let mut risk = match (first, failed.is_some(), lead) {
            (Some((test, Some(why))), ..) => format!("The {title} failed: {test}\n    {why}"),
            (Some((test, None)), ..) => format!("The {title} failed: {test}"),
            (None, _, Some(said)) => format!("The {title} didn't pass: {said}"),
            (None, true, None) => format!("The {title} failed — v shows its output."),
            (None, false, None) => format!("The {title} failed — the task panel says why."),
        };
        for said in not_run {
            risk.push_str(&format!("\n    {said}"));
        }
        // (every other test that failed too, by name — frictions29 93: one known flake named alone read as the
        // only failure; `v` lists them)
        let also = failed.as_ref().and_then(|(_, lines)| crate::preflight::also_failed(lines));
        if let Some((n, others)) = also.filter(|(n, _)| *n > 1) {
            let others = crate::preflight::tests_named(&others, crate::preflight::NAMES_ROOM);
            risk.push_str(&format!("\n    {n} failed in all — also {others}"));
        }
        // (`p` runs it again: its answer says so)
        let q = format!(
            "{} on {base}'s tip now. Bring {them} into {base} anyway?\n\n{risk}",
            crate::worktree::commits(self.ahead),
        );
        let mut card = Offer::new(
            format!("Bring {} into {base}", self.branch),
            &q,
            "Bringing…",
            format!("Couldn't bring {}", self.branch),
            What::Rebased(Box::new(self.clone())),
        );
        // (in `warnings`, not `warning`: the held high-risk changes' lines take that — `push_bring_card`)
        card.warnings.push(2..2 + risk.lines().count());
        card.output = failed;
        card
    }

    /// The card for a session with nothing to bring, behind its base (`ahead` 0, #186): move it up to its
    /// base's tip — a fast-forward in its checkout.
    pub fn up_card(&self) -> Offer {
        let (base, branch) = (&self.base, &self.branch);
        let q = format!(
            "{branch} has no commits past {base} — nothing to bring. It's {} behind: move it up to {base}'s \
             tip?\n\nA fast-forward in its checkout: nothing of its own to replay, nothing lost.",
            crate::worktree::commits(self.moved),
        );
        Offer::new(
            format!("Move {} up to {base}", self.name),
            &q,
            "Moving up…",
            format!("Couldn't move {branch} up"),
            What::UpTo(Box::new(self.clone())),
        )
    }
}

/// What a rebase would be held back by in a session's checkout (#188: git refused it over changes in no
/// commit, and the card only offered `y` retry): files changed there, buffers unsaved there. Its card says
/// which, and offers what fits — `s` stashes the changes for the rebase (none unsaved), `g` shows them.
/// A bring's (`bring`, #371): changes in no commit on its base's tip, untracked too — its preflight would test them
/// and the bring land its commits without them: `g` only (set aside for a preflight, they'd vanish from under its
/// Claude for its whole run).
#[derive(Clone, Debug)]
pub struct Held {
    pub moved: Moved,
    /// Changed in no commit · unsaved in a buffer (relative to the checkout).
    pub changed: Vec<String>,
    pub unsaved: Vec<String>,
    pub bring: bool,
}

impl Held {
    /// Whether `s` answers it: a rebase's, nothing unsaved (a buffer's text isn't on disk for git to stash).
    pub fn stashes(&self) -> bool {
        !self.bring && self.unsaved.is_empty()
    }

    pub fn card(&self) -> Offer {
        let m = &self.moved;
        let it = |files: &[String]| files.len() == 1;
        let them = |files: &[String]| if it(files) { "it" } else { "them" };
        if self.bring {
            let c = &self.changed;
            let q = format!(
                "{} changed and {} in a commit — its preflight would test {}, and the bring lands {}'s commits \
                 without {}.\n\nCommit or stash {} (g shows {}), then :workspace-bring {} again.",
                named(c),
                if it(c) { "isn't" } else { "aren't" },
                them(c),
                m.branch,
                them(c),
                them(c),
                them(c),
                m.name,
            );
            let mut card = Offer::new(
                format!("Bring {}: changes in no commit", m.name),
                &q,
                "Showing them…",
                format!("Couldn't bring {}", m.branch),
                What::RebaseHeld(Box::new(self.clone())),
            );
            card.decided = Some(0..1);
            return card;
        }
        let q = match self.stashes() {
            true => {
                let c = &self.changed;
                format!(
                    "{} changed and {} in a commit — git won't rebase {} over {}.\n\ns stashes {} for the \
                     rebase and puts {} back after (kept in git's stash, named, if {} can't go back cleanly — \
                     never left half merged).",
                    named(c),
                    if it(c) { "isn't" } else { "aren't" },
                    m.branch,
                    them(c),
                    them(c),
                    them(c),
                    if it(c) { "it" } else { "they" },
                )
            }
            false => {
                let u = &self.unsaved;
                let mut q = format!(
                    "{} {} unsaved here — save {} (:w) first, then :workspace-bring {} again.",
                    named(u),
                    if it(u) { "is" } else { "are" },
                    them(u),
                    m.name,
                );
                let saved: Vec<String> = self.changed.iter().filter(|f| !u.contains(f)).cloned().collect();
                if !saved.is_empty() {
                    q.push_str(&format!(" {} changed too, in no commit.", named(&saved)));
                }
                q
            }
        };
        Offer::new(
            format!("Rebase {}: changes in no commit", m.name),
            &q,
            "Stashing and rebasing…",
            format!("Couldn't rebase {}", m.branch),
            What::RebaseHeld(Box::new(self.clone())),
        )
    }
}

/// The files git has changed in the checkout at `root`, relative — new ones untracked don't count: git
/// refuses only one in the way, and says so. None: git can't say.
pub(crate) fn changed(root: &Path) -> Option<Vec<String>> {
    let status = git(root, &["status", "--porcelain", "-z", "--untracked-files=no"], None).ok()?;
    let mut v = Vec::new();
    let mut entries = status.split('\0').filter(|e| !e.is_empty());
    while let Some(e) = entries.next() {
        // (a rename in the index: its old path comes next)
        if e.starts_with(['R', 'C']) {
            entries.next();
        }
        v.extend(e.get(3..).filter(|p| !p.is_empty()).map(str::to_string));
    }
    Some(v)
}

/// The files changed in no commit in the checkout at `root`, relative — new ones untracked too: a preflight sees
/// them, a bring doesn't take them (#371). None: git can't say.
pub(crate) fn unclean(root: &Path) -> Option<Vec<String>> {
    let status = git(root, &["status", "--porcelain", "-z", "--untracked-files=all"], None).ok()?;
    let mut v = Vec::new();
    let mut entries = status.split('\0').filter(|e| !e.is_empty());
    while let Some(e) = entries.next() {
        // (a rename in the index: its old path comes next)
        if e.starts_with(['R', 'C']) {
            entries.next();
        }
        v.extend(e.get(3..).filter(|p| !p.is_empty()).map(str::to_string));
    }
    Some(v)
}

/// Why a session's checkout can't be moved up to its base: changes in no commit — files git has changed
/// (`changed`), or buffers unsaved there (`unsaved`). None: nothing holds it (or git can't say — the move
/// itself will).
pub fn held_up(root: &Path, unsaved: &[PathBuf]) -> Option<String> {
    let mut files = changed(root)?;
    for p in unsaved.iter().filter_map(|p| p.strip_prefix(root).ok()) {
        let p = p.to_string_lossy().into_owned();
        if !files.contains(&p) {
            files.push(p);
        }
    }
    files.retain(|f| !f.is_empty());
    let some: Vec<&str> = files.iter().take(3).map(String::as_str).collect();
    let more = if files.len() > 3 { ", …" } else { "" };
    (!files.is_empty())
        .then(|| format!("it has changes in {}{more} — commit or stash them first", some.join(", ")))
}

/// Session `m`'s branch moved up to its base's tip — checked again first (nothing of its own since, no
/// changes): the tip it's at now, short (on a worker).
fn move_up(m: &Moved, unsaved: &[PathBuf]) -> Result<String, String> {
    let base_tip = crate::worktree::sha(&m.root, &format!("refs/heads/{}", m.base))?;
    let head = crate::worktree::sha(&m.root, "HEAD")?;
    if crate::worktree::count(&m.root, &format!("{base_tip}..{head}")) > 0 {
        return Err(format!("{} has commits of its own since — :workspace-bring again", m.branch));
    }
    if let Some(why) = held_up(&m.root, unsaved) {
        return Err(why);
    }
    crate::gitrepo::git_no_hooks(&m.root, &["merge", "--ff-only", "-q", &base_tip])?;
    Ok(base_tip.chars().take(7).collect())
}

/// Where a rebase stopped (read on a worker).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stop {
    /// Commit n of m.
    pub step: Option<(usize, usize)>,
    /// Its conflicted files, relative to the checkout.
    pub files: Vec<String>,
    /// The commit being replayed: (full sha, short, subject).
    pub commit: Option<(String, String, String)>,
    /// The branch being rebased · the commit it's replayed onto (git's `onto`, whole) — the base for one tarae
    /// didn't start, and what its end is judged against (#223: the base's branch may have moved since).
    pub head: Option<String>,
    pub onto: Option<String>,
    /// When git stopped there (its step file's time) — what waits since then, across a restart.
    pub since: Option<std::time::SystemTime>,
    /// The replayed commit's whole message (frictions29 #25: what a resolution may make wrong).
    pub message: Option<String>,
    /// What was untracked or ignored in the checkout as git stopped (`status`'s, a folder whole as `dir/`) —
    /// never the resolution's (#206: a build folder the series ignores only in a later commit). None: not read
    /// (a rebase tarae took up): no untracked file is counted as the resolution's then.
    pub before: Option<Vec<String>>,
}

impl Stop {
    /// `commit 2 of 5 (a1b2c3d Add login)`.
    fn at(&self) -> String {
        let commit = self.commit.as_ref().map(|(_, short, subject)| format!("{short} {subject}"));
        match (self.step, commit) {
            (Some((n, m)), Some(c)) => format!("commit {n} of {m} ({c})"),
            (Some((n, m)), None) => format!("commit {n} of {m}"),
            (None, Some(c)) => c,
            (None, None) => "a commit".into(),
        }
    }
}

/// A rebase under way in a session's checkout (`Workspace.rebase`).
pub struct Rebase {
    pub branch: String,
    pub base: String,
    pub stage: Stage,
    pub step: Option<(usize, usize)>,
    pub stop: Stop,
    /// Its Claude was asked to resolve this stop (the end of its turns is looked at) · the ask, waiting for
    /// the turn under way to end.
    pub asked: bool,
    waiting: Option<Ask>,
    /// What the resolution changed besides the conflicted files (#129): in its review and named on its card,
    /// never staged by tarae on `y` (#206) — a tracked one holds `y` until you stage or undo it, a made one is
    /// staged by the card's `a` only.
    pub also: Besides,
    /// The files whose changes in no commit were set aside for it (#188, `aside::REBASE`) — put back at its end,
    /// and said.
    pub stashed: Vec<String>,
    /// When its stop was resolved (`Stage::Resolved`) — from then it waits on you (`waits_on_you`).
    pub resolved_at: Option<std::time::Instant>,
    /// What becomes of the replayed commits' messages (frictions29 #25).
    pub messages: Messages,
    /// `rebase_recheck`'s reads: the last asked · the last landed — only the last asked lands (an older one may have
    /// read the checkout before a stage); the two apart, one is out.
    pub rechecks: (u64, u64),
    /// Its checkout's index watched while a tracked file holds a resolved stop's `y` (#245) — only then: gone once
    /// none does or the stage moves on (`rebase_index_watch`), its thread ending with it.
    index: Option<IndexWatch>,
}

/// A rebase's index watch (`watch_index`): its thread ends as this goes — told to stop, and woken.
struct IndexWatch {
    waker: crate::fswatch::Waker,
    /// Set as this goes; the thread holds the other `Arc` until it ends (a test's `Weak` sees it gone).
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for IndexWatch {
    fn drop(&mut self) {
        // (the flag too: a waker with no pipe wakes nothing, its thread ends at the next backstop)
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.waker.wake();
    }
}

/// Watches the index of the checkout at `root` (beside its HEAD, `git::find_head`) until `stop` (#245): a stage or
/// undo by anyone — the git tab, a terminal, another program — reads the resolved card's checkout again
/// (`Editor::rebase_index_moved`), as the git tab's own act does. Its first round reads too (a stage between the
/// turn's end and the watch's start); a wake with the index as it was (the backstop, a neighbour in its folder)
/// sends nothing — nothing reaches the loop while nobody touches the index, and it's gone before git does
/// (`rebase_run`).
fn watch_index(
    events: &crate::event::Queue,
    root: PathBuf,
    waker: crate::fswatch::Waker,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use std::os::unix::fs::MetadataExt;
    let tx = events.sender();
    crate::event::thread("tarae-rb-index", move || {
        let Some(index) = crate::git::find_head(&root).map(|h| h.with_file_name("index")) else { return };
        let stamp = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.modified().ok(), m.len(), m.ino()));
        let mut last = None;
        loop {
            let mut watcher = crate::fswatch::Watcher::with_waker(waker.clone()); // (armed before the stat)
            watcher.file(&index);
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let now = Some(stamp(&index));
            if now != last {
                last = now;
                let root = root.clone();
                let told = move |ed: &mut Editor| ed.rebase_index_moved(&root);
                if tx.send(crate::event::Event::Job(Box::new(told))).is_err() {
                    break;
                }
            }
            if watcher.wait() == crate::fswatch::Woke::Cancelled {
                break;
            }
        }
    });
}

/// The messages of the commits a rebase replays, where a resolution may have made them wrong (frictions29 #25:
/// a resolution flipped what a commit did, and its message went on saying the opposite).
#[derive(Clone, Debug, Default)]
pub struct Messages {
    /// The stop's message as Claude's answer says it should now read (a ```` ```commit-message ```` block) —
    /// it said the message no longer holds.
    pub proposed: Option<String>,
    /// The stop's message being written (`m` on the card): its buffer and file.
    pub editing: Option<(crate::document::DocId, PathBuf)>,
    /// Its file once saved: what the rebase goes on with (`rebase_continue`).
    pub written: Option<PathBuf>,
    /// The commits whose conflict was resolved and whose message went on as it was, by subject — the bring
    /// card names them.
    pub kept: Vec<String>,
}

/// The fenced block Claude's answer proposes the stop's message in.
const MESSAGE_FENCE: &str = "```commit-message";

/// The message an answer proposes (`MESSAGE_FENCE`'s block, the last one) — None: it said the message holds.
pub fn proposed_message(answer: &str) -> Option<String> {
    let (_, after) = answer.rsplit_once(MESSAGE_FENCE)?;
    let body = after.strip_prefix('\n').unwrap_or(after);
    let (text, _) = body.split_once("```")?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The file the stop's message is written in (`m`), in the checkout's git folder: git's own editor hook takes
/// it from there as the rebase goes on (`rebase_continue`).
const MESSAGE_FILE: &str = "TARAE_REBASE_MSG";

/// The line in the message file after which what's said is left out — tarae's own cut, not git's comment lines:
/// a message's own `#` lines (a heading, `#123`) stay, and `core.commentChar` doesn't matter (the review of
/// frictions31: git's strip dropped them, or kept `#` help under another comment character).
const MESSAGE_CUT: &str = "──────── tarae: this line and what's below it are left out ────────";

/// The message as written in `text`: what comes before the last `MESSAGE_CUT` line, its trailing blank lines off.
pub fn message_written(text: &str) -> String {
    message_cut(text, MESSAGE_CUT)
}

/// The message as written in `text`: what comes before the last line that is `cut`, its trailing blank lines off —
/// every line of it, a `#` one too (the git tab's commit cuts at git's scissors line, frictions #146).
pub(crate) fn message_cut(text: &str, cut: &str) -> String {
    let kept = match text.rfind(&format!("\n{cut}")) {
        Some(i) => &text[..i],
        None if text.starts_with(cut) => "",
        None => text,
    };
    format!("{}\n", kept.trim_end())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// git runs in the task panel (run `generation`) — rebasing or continuing, or `aborting`.
    Running { generation: u64, aborting: bool },
    /// Stopped at a conflict.
    Stopped,
    /// Its files resolved: the review and the card are up.
    Resolved,
}

impl Rebase {
    /// `rebasing 2/5` — what the session's tab, chip and row say.
    pub fn said(&self) -> String {
        match self.step {
            Some((n, m)) => format!("rebasing {n}/{m}"),
            None => "rebasing".into(),
        }
    }

    /// Since when it waits on you — its card up or put away (`n`): the inbox lists it (`inbox.rs`). Resolved:
    /// since then. Stopped with nothing asked of its Claude (git stopped for another reason, or one tarae
    /// didn't start — after a restart, a quit): since git stopped, as git's files say — its real wait, whatever
    /// tarae was running then. None while it's Claude's (asked to resolve) or git's (running).
    pub fn waits_on_you(&self) -> Option<std::time::Instant> {
        let now = std::time::Instant::now();
        let since_git = || {
            let ago = std::time::SystemTime::now().duration_since(self.stop.since?).ok()?;
            Some(now.checked_sub(ago).unwrap_or(now))
        };
        match self.stage {
            Stage::Resolved => Some(self.resolved_at.or_else(since_git).unwrap_or(now)),
            Stage::Stopped if !self.asked && self.waiting.is_none() => Some(since_git().unwrap_or(now)),
            _ => None,
        }
    }

    /// Stopped for the user (said in the warning color), not git at work.
    pub fn stopped(&self) -> bool {
        !matches!(self.stage, Stage::Running { .. })
    }

    /// Why what would get in its way waits — `session` is its session's name.
    pub fn why(&self, session: &str) -> String {
        match self.stage {
            Stage::Running { .. } => format!("{session} is {} — let it finish", self.said()),
            _ => format!("{session} is {} — :rebase-continue or :rebase-abort first", self.said()),
        }
    }
}

/// What the session's Claude is asked: the message, its context (the files and both sides), the chip.
#[derive(Clone, Debug)]
pub struct Ask {
    pub question: String,
    about: String,
    chip: String,
}

// ── Git (workers) ────────────────────────────────────────────────────────────

fn git_dir(root: &Path) -> Option<PathBuf> {
    crate::gitrepo::repo(root).ok().map(|r| r.git_dir.clone())
}

/// The rebase's own folder in the git folder, while one is under way (`git am`'s isn't one).
fn state_dir(git_dir: &Path) -> Option<PathBuf> {
    let merge = git_dir.join("rebase-merge");
    let apply = git_dir.join("rebase-apply");
    if merge.is_dir() { Some(merge) } else { apply.join("rebasing").exists().then_some(apply) }
}

/// Commit n of m, as git counts in the rebase's folder.
fn step_of(state: &Path) -> Option<(usize, usize)> {
    let read = |f: &str| std::fs::read_to_string(state.join(f)).ok()?.trim().parse::<usize>().ok();
    match state.ends_with("rebase-merge") {
        true => Some((read("msgnum")?, read("end")?)),
        false => Some((read("next")?, read("last")?)),
    }
}

/// The rebase under way in the checkout at `root` — where it stopped — if there's one.
pub fn under_way(root: &Path) -> Option<Stop> {
    let state = state_dir(&git_dir(root)?)?;
    let read = |f: &str| std::fs::read_to_string(state.join(f)).ok().map(|s| s.trim().to_string());
    // (`ls-files -u`: a row per side, by path — a read that never takes the index lock)
    let mut files: Vec<String> = git(root, &["ls-files", "-u", "-z"], None)
        .unwrap_or_default()
        .split('\0')
        .filter_map(|e| e.split_once('\t').map(|(_, p)| p.to_string()))
        .collect();
    files.dedup();
    let commit =
        git(root, &["log", "-1", "--format=%H%x1f%h%x1f%s", "REBASE_HEAD"], None).ok().and_then(|l| {
            let mut p = l.trim().splitn(3, '\x1f').map(str::to_string);
            Some((p.next()?, p.next()?, p.next().unwrap_or_default()))
        });
    // (git writes the step file as it stops at each commit: its time is the stop's)
    let step_file = state.join(if state.ends_with("rebase-merge") { "msgnum" } else { "next" });
    let since =
        [step_file, state.clone()].iter().find_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    Some(Stop {
        step: step_of(&state),
        files,
        commit,
        head: read("head-name").map(|h| h.trim_start_matches("refs/heads/").to_string()),
        onto: read("onto"),
        since,
        message: git(root, &["log", "-1", "--format=%B", "REBASE_HEAD"], None)
            .ok()
            .map(|m| m.trim_end().to_string())
            .filter(|m| !m.is_empty()),
        before: None,
    })
}

/// How a run of git ended (read on a worker after it).
pub enum Ended {
    /// Stopped (at a conflict — Claude's ask made ready — or for a reason the panel says).
    Stopped(Box<Stop>, Option<Ask>),
    /// Done: the branch's commits on the commit it was replayed onto, at `tip` — and the base's commits past that
    /// one (`moved`: brought in by another session while a stop waited — #223), 0 when it's the base's tip.
    Done {
        ahead: usize,
        tip: String,
        moved: usize,
    },
    Aborted,
    /// Nothing under way and not done: git stopped before it began (the panel says why).
    Failed,
}

/// What became of the changes in no commit set aside for the rebase (`aside::REBASE`, `rebase_go`) at its end.
#[derive(Debug, PartialEq)]
pub enum Aside {
    /// None were.
    None,
    Back,
    /// They didn't go back cleanly (or couldn't be tried) — where they are, said.
    Kept(String),
}

/// `onto`: the commit a stop said it replays onto — done is judged against it, never the base's tip as it is now:
/// another session brought into the base while the stop was resolved (#223) and it's no ancestor of the branch.
fn ended(root: &Path, branch: &str, base: &str, onto: Option<&str>, aborting: bool) -> (Ended, Aside) {
    if let Some(mut stop) = under_way(root) {
        stop.before = Some(left_alone(root));
        let ask = (!stop.files.is_empty()).then(|| ask(root, branch, base, &stop));
        return (Ended::Stopped(Box::new(stop), ask), Aside::None);
    }
    let aside = changes_back(root, branch, base);
    if aborting {
        return (Ended::Aborted, aside);
    }
    let base_tip = crate::worktree::sha(root, &format!("{}^{{commit}}", onto.unwrap_or(base)));
    let end = match (base_tip, crate::worktree::sha(root, "HEAD")) {
        (Ok(b), Ok(t)) if crate::worktree::is_ancestor(root, &b, &t) => {
            let moved = match crate::worktree::sha(root, &format!("{base}^{{commit}}")) {
                Ok(now) if now != b => crate::worktree::count(root, &format!("{t}..{now}")),
                _ => 0,
            };
            Ended::Done {
                ahead: crate::worktree::count(root, &format!("{b}..{t}")),
                tip: t[..7].to_string(),
                moved,
            }
        }
        _ => Ended::Failed,
    };
    (end, aside)
}

/// What the rebase's end says of the changes set aside for it (`stashed`: their files, as looked at before).
fn aside_said(aside: &Aside, stashed: &[String]) -> String {
    let them = if stashed.is_empty() { "no commit".to_string() } else { named(stashed) };
    match aside {
        Aside::None => String::new(),
        Aside::Back => format!("; your changes in {them} are back"),
        Aside::Kept(why) => format!("; your changes in {them} didn't go back cleanly — {why}"),
    }
}

/// The changes set aside for the rebase put back where it ended (done, aborted, or never begun) — applied only onto
/// the tracked files as HEAD has them, so the reset after one that didn't go back takes nothing but its markers;
/// then, not back, kept in git's stash as an entry of their own, named (never applied by anyone's `pop`). A
/// conflict there leaves the rebase as it is: it may hold a resolution of Claude's.
fn changes_back(root: &Path, branch: &str, base: &str) -> Aside {
    const AT: &str = crate::aside::REBASE;
    let Some(sha) = crate::aside::kept(root, AT) else { return Aside::None };
    let clean = changed(root).is_some_and(|c| c.is_empty());
    if clean {
        if crate::aside::back(root, AT, &sha).is_ok() {
            return Aside::Back;
        }
        if let Err(e) = crate::gitrepo::git_no_hooks(root, &["reset", "--quiet", "--hard"]) {
            return Aside::Kept(format!("{e} — {}", crate::aside::kept_at(root, AT, &sha)));
        }
    }
    let message = format!("tarae: {branch}'s changes in no commit, set aside for its rebase onto {base}");
    match crate::aside::store(root, AT, &sha, &message) {
        Ok(()) => Aside::Kept(format!(
            "they're in git's stash as «{message}» — git stash apply {} brings them back",
            sha.get(..7).unwrap_or(&sha)
        )),
        Err(e) => Aside::Kept(e),
    }
}

/// `s` cut to `max` bytes (on a character's edge), saying so.
fn cut(mut s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut at = max;
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    let more = s.len() - at;
    s.truncate(at);
    s.push_str(&format!("\n… ({more} more bytes — read the files themselves)\n"));
    s
}

/// The session's Claude's ask for a stop with conflicts: the files and both sides — what the commit being
/// replayed changed in them, and what the base (and the commits replayed before it) changed since that
/// commit's parent.
fn ask(root: &Path, branch: &str, base: &str, stop: &Stop) -> Ask {
    let side = |to: &str| {
        let mut args = vec!["diff", "--no-color", "--no-ext-diff", "REBASE_HEAD^", to, "--"];
        args.extend(stop.files.iter().map(String::as_str));
        cut(git(root, &args, None).unwrap_or_default(), SIDE_MAX)
    };
    let n = stop.files.len();
    let (those, them) = if n == 1 { ("that file", "it's") } else { ("those files", "they're") };
    let question = format!(
        "Rebasing {branch} onto {base} stopped at {}: git couldn't merge {}. Resolve the conflicts in {those} \
         — keep what both sides meant, and leave no conflict markers. Don't run git and don't commit: tarae \
         stages them and goes on once I've looked. Tell me when {them} resolved.",
        stop.at(),
        stop.files.join(", "),
    );
    // (frictions29 #25: the commit keeps its message through the resolution unless someone asks whether it
    // still holds — a proposal lands in the card's `m`)
    let question = match &stop.message {
        Some(_) => format!(
            "{question} Then say whether your resolution changes what that commit's message (in <message>) \
             says. If it does, end your answer with the whole message as it should now read, in a \
             {MESSAGE_FENCE} block; if it still holds, say so and add no block."
        ),
        None => question,
    };
    let short = stop.commit.as_ref().map_or("", |c| c.1.as_str());
    let message = match &stop.message {
        Some(m) => format!("<message>\n{m}\n</message>\n"),
        None => String::new(),
    };
    let about = format!(
        "<rebase branch=\"{branch}\" onto=\"{base}\" commit=\"{short}\">\n{message}<conflicted>\n{}\n\
         </conflicted>\n<theirs note=\"what the commit being replayed changed in them\">\n{}</theirs>\n\
         <ours note=\"what {base}, and the commits replayed before this one, changed in them since that \
         commit's parent\">\n{}</ours>\n</rebase>",
        stop.files.join("\n"),
        side("REBASE_HEAD"),
        side("HEAD"),
    );
    let chip = format!("rebase · {n} conflicted file{}", if n == 1 { "" } else { "s" });
    Ask { question, about, chip }
}

/// Of `files` (relative, with an open buffer's text where there's one), those with conflict markers left.
fn marked(root: &Path, files: Vec<(String, Option<ropey::Rope>)>) -> Vec<String> {
    files
        .into_iter()
        .filter(|(rel, text)| {
            let text = match text {
                Some(t) => t.to_string(),
                None => std::fs::read_to_string(root.join(rel)).unwrap_or_default(),
            };
            crate::conflict::has_markers(&text)
        })
        .map(|(rel, _)| rel)
        .collect()
}

/// `status`'s entries in the checkout (relative): (its two letters, the path). With optional locks off, as
/// Claude's runs and the git tab read it — never a `git diff` over the working tree (it refreshes the index).
fn status(root: &Path, args: &[&str]) -> Vec<(String, String)> {
    let mut all = vec!["status", "--porcelain", "-z"];
    all.extend(args);
    let out = git(root, &all, None).unwrap_or_default();
    let mut v = Vec::new();
    let mut entries = out.split('\0').filter(|e| !e.is_empty());
    while let Some(e) = entries.next() {
        let (xy, path) = (e.get(..2).unwrap_or_default(), e.get(3..).unwrap_or_default());
        // (a rename or copy in the index: its old path comes next)
        if xy.starts_with(['R', 'C']) {
            entries.next();
        }
        if !path.is_empty() {
            v.push((xy.to_string(), path.to_string()));
        }
    }
    v
}

/// What's untracked or ignored in the checkout as git stops (`Stop.before`): a folder that's all untracked, or
/// ignored, whole (`dir/`) — never walked into, a build folder's thousands of files one row.
fn left_alone(root: &Path) -> Vec<String> {
    let v = status(root, &["--untracked-files=normal", "--ignored=matching"]);
    v.into_iter().filter(|(xy, _)| xy == "??" || xy == "!!").map(|(_, p)| p).collect()
}

/// What the checkout has changed besides the conflicted files (relative), as a resolution leaves it (#206).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Besides {
    /// Tracked files changed against the index — git won't go on while one is ("You must edit all merge
    /// conflicts…"), and tarae never stages one: the resolved card's `y` waits until you stage or undo them.
    pub tracked: Vec<String>,
    /// Untracked files that weren't there as git stopped (`before`; none counted when it wasn't read) — the
    /// resolution's own (code split into a new file): `y` leaves them untracked, said; the card's `a` stages
    /// those it named.
    pub made: Vec<String>,
}

impl Besides {
    fn all(&self) -> Vec<String> {
        self.tracked.iter().chain(&self.made).cloned().collect()
    }
}

/// What's changed in the checkout at `root` besides `conflicted`: tracked files the working tree changed against
/// the index (what git staged at the stop, the replayed commit's own, isn't), and untracked ones made since the
/// stop (`before`). Never what was untracked or ignored as git stopped, nor a file inside such a folder.
fn besides(root: &Path, conflicted: &[String], before: Option<&[String]>) -> Besides {
    let there = |p: &str| {
        before.is_none_or(|b| b.iter().any(|b| b == p || (b.ends_with('/') && p.starts_with(b.as_str()))))
    };
    let mut v = Besides::default();
    for (xy, path) in status(root, &["--untracked-files=all"]) {
        if conflicted.contains(&path) {
            continue;
        }
        match xy.as_str() {
            "??" if !there(&path) => v.made.push(path),
            "??" | "!!" => {}
            _ if !xy.ends_with(' ') => v.tracked.push(path),
            _ => {}
        }
    }
    v
}

/// `a.txt, b.rs` — the first few, the rest counted.
fn named(files: &[String]) -> String {
    let mut s = files.iter().take(4).cloned().collect::<Vec<_>>().join(", ");
    if files.len() > 4 {
        s.push_str(&format!(" and {} more", files.len() - 4));
    }
    s
}

/// A break a trap makes, to see its test or simulation fail (`sim/rebaserecheck.rs`, `worktree`'s tests; test
/// builds only — `broken` is false in others; one at a time, `Broken::set`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Broken {
    #[cfg(test)]
    No,
    /// A read again that finds no card up puts one up: a card answered comes back.
    BringsBack,
    /// A read again lands at whatever stop its rebase is resolved at by then — the stage alone looked at.
    AnyStop,
    /// A card said again keeps its `rev`: the key focused on what it said answers what it says now.
    SameRev,
    /// A tracked file found as `y` (or `m`'s `:w`) goes on with the card gone puts no card back: a warning only.
    GoneOnRefusal,
    /// The card put back isn't yours: it waits unfocused behind `C-q`, the refusal on it out of sight.
    NotYours,
    /// The conflicted paths staged as pathspecs: a `*` in a name a glob, matching its neighbours.
    GlobPathspecs,
    /// A conflicted path the resolution deleted staged with `git add`, as the others are (#325).
    DeletionsAdded,
    /// A conflicted path the resolution deleted left unstaged.
    DeletionsLeft,
    /// A HEAD move never looks at a stopped rebase: one ended outside tarae is kept, its card with it.
    HeadMoveKept,
    /// Going on never asks whether the rebase is still under way: `git rebase --continue` with none in progress.
    GoneGoesOn,
    /// A rebase ended outside tarae let go whatever its stage: one going on (git at work) too.
    LetGoAtWork,
}

#[cfg(test)]
thread_local! {
    static BROKEN: std::cell::Cell<Broken> = const { std::cell::Cell::new(Broken::No) };
}

/// Broken this way until it goes (dropped — a failed assertion too).
#[cfg(test)]
pub(crate) struct Back;

#[cfg(test)]
impl Drop for Back {
    fn drop(&mut self) {
        BROKEN.set(Broken::No);
    }
}

#[cfg(test)]
impl Broken {
    pub(crate) fn set(self) -> Back {
        BROKEN.set(self);
        Back
    }
}

#[cfg(test)]
fn broken(b: Broken) -> bool {
    BROKEN.get() == b
}

#[cfg(not(test))]
fn broken(_: Broken) -> bool {
    false
}

// ── The flow ─────────────────────────────────────────────────────────────────

/// The task panel's run is closed or replaced (`space m c`, another task) while it runs: if it's the
/// rebase's git, what git left is read as at its end — else the session would say `rebasing` for good.
pub fn run_dropped(ed: &mut Editor) {
    let Some(t) = ed.workspace.task_run.as_ref().filter(|t| t.state == crate::task::State::Running) else {
        return;
    };
    let generation = t.generation();
    if ran(ed, generation) {
        ed.set_warning("the rebase's git was stopped — it's read as git left it");
    }
}

/// A git run of the rebase's ended (`task.rs`) — true if it was one (what's next is decided on a worker).
pub fn ran(ed: &mut Editor, generation: u64) -> bool {
    let Some(r) = ed.workspace.rebase.as_ref() else { return false };
    let Stage::Running { generation: g, aborting } = r.stage else { return false };
    if g != generation {
        return false;
    }
    let (root, branch, base, onto) =
        (ed.workspace.root.clone(), r.branch.clone(), r.base.clone(), r.stop.onto.clone());
    // (git's exit says too little — a stop at conflicts exits 1 — so the run reads as under way until its end
    // is read, then as git left it: `rebase_ended`)
    let exit = match ed.workspace.task_run.as_mut().filter(|t| t.generation() == generation) {
        Some(t) => std::mem::replace(&mut t.state, crate::task::State::Running),
        None => crate::task::State::Failed(None),
    };
    ed.events.jobs().spawn(move || {
        let (end, aside) = ended(&root, &branch, &base, onto.as_deref(), aborting);
        move |ed: &mut Editor| ed.rebase_ended(generation, end, aside, exit)
    });
    true
}

/// While git runs: the step it's at, read every `POLL` (nothing once it's done).
fn poll(ed: &Editor, root: PathBuf, generation: u64, dir: Option<PathBuf>) {
    ed.events.jobs().spawn(move || {
        crate::event::pause(POLL);
        let dir = dir.or_else(|| git_dir(&root));
        let step = dir.as_deref().and_then(state_dir).as_deref().and_then(step_of);
        move |ed: &mut Editor| {
            let running = ed
                .workspace
                .task_run
                .as_ref()
                .is_some_and(|t| t.generation() == generation && t.state == crate::task::State::Running);
            let Some(r) = ed
                .workspace
                .rebase
                .as_mut()
                .filter(|r| matches!(r.stage, Stage::Running { generation: g, .. } if g == generation))
            else {
                return;
            };
            match step.filter(|s| r.step != Some(*s)) {
                Some(s) => r.step = Some(s),
                None => ed.unchanged = true,
            }
            if running {
                poll(ed, root, generation, dir);
            }
        }
    });
}

/// The session's chat has its claude now (`chat::gate_opened`): an ask that waited for it to start goes.
pub fn chat_ready(ed: &mut Editor) {
    if let Some(a) = ed.workspace.rebase.as_mut().and_then(|r| r.waiting.take()) {
        ed.rebase_ask(a);
    }
}

/// The session's Claude was asked, and its turn ended (`chat.rs`): an ask that waited for it goes now;
/// else, while the rebase is stopped at what it was asked about, its files are looked at for markers.
pub fn turn_over(ed: &mut Editor) {
    let Some(r) = ed.workspace.rebase.as_mut() else { return };
    if let Some(a) = r.waiting.take() {
        return ed.rebase_ask(a);
    }
    if r.stage != Stage::Stopped || !r.asked {
        return;
    }
    // (what its answer says of the commit's message: a version of it when it no longer holds)
    let answer = ed.workspace.claude.chat.as_ref().map(|c| c.last_answer()).unwrap_or_default();
    r.messages.proposed = proposed_message(&answer);
    let conflicted = r.stop.files.clone();
    let before = r.stop.before.clone();
    let files = ed.rebase_texts(&conflicted);
    let root = ed.workspace.root.clone();
    ed.events.jobs().spawn(move || {
        let left = marked(&root, files);
        // (what else the resolution changed — a snapshot blessed again: #129; never what was untracked or
        // ignored as git stopped: #206)
        let also = besides(&root, &conflicted, before.as_deref());
        move |ed: &mut Editor| ed.rebase_checked(left, also)
    });
}

impl Editor {
    /// A session by its checkout.
    pub(crate) fn session_at(&self, root: &Path) -> Option<crate::event::WorkspaceId> {
        std::iter::once(&self.workspace).chain(&self.workspaces).find(|l| l.root == root).map(|l| l.id)
    }

    /// What keeps git from running in the session in front now: its files unsaved, a task in the panel.
    fn rebase_free(&self) -> Result<(), String> {
        let root = &self.workspace.root;
        let unsaved: Vec<String> = self
            .docs
            .iter()
            .filter(|d| d.is_modified() && d.path.as_ref().is_some_and(|p| p.starts_with(root)))
            .map(|d| d.display_name(root))
            .collect();
        if !unsaved.is_empty() {
            return Err(format!("unsaved changes in {} — save them first", unsaved.join(", ")));
        }
        match self.workspace.task_run.as_ref().filter(|r| r.state == crate::task::State::Running) {
            Some(r) => Err(format!("{} is running — let it finish, or stop it (space m c)", r.task.name)),
            None => Ok(()),
        }
    }

    /// `files` (relative) with an unsaved buffer's text — what the user typed is what's checked. A saved one's
    /// is the disk's (read on the worker): its reload can lag the write — Claude's resolution was on disk while
    /// the buffer still showed the markers (a shot, 2026-10-04).
    fn rebase_texts(&self, files: &[String]) -> Vec<(String, Option<ropey::Rope>)> {
        let root = &self.workspace.root;
        files
            .iter()
            .map(|rel| {
                let p = root.join(rel);
                let unsaved =
                    self.docs.iter().find(|d| d.path.as_deref() == Some(p.as_path()) && d.is_modified());
                (rel.clone(), unsaved.map(|d| d.text.clone()))
            })
            .collect()
    }

    /// git in the task panel for the rebase in front — its step read while it runs. `cmd` makes the line from
    /// its start (`git_line`): the repository as kept, else read on a worker first — `gitrepo`'s list is capped,
    /// so a start, `--continue` or `--abort` never fails on it. One that can't start: `unstarted` says why.
    fn rebase_git(
        &mut self,
        name: &str,
        cmd: impl FnOnce(&str) -> String + Send + 'static,
        aborting: bool,
        unstarted: Unstarted,
    ) {
        let root = self.workspace.root.clone();
        if let Some(repo) = crate::gitrepo::cached(&root) {
            let line = git_line(&repo);
            if let Err(e) = self.rebase_run(name, cmd(&line.text), line.env, aborting) {
                unstarted(self, e);
            }
            return;
        }
        let (name, asked) = (name.to_string(), self.workspace.id);
        // (`jobs()`: applied in the session that asked, swapped in if it's behind by then — `with_workspace`; one
        // gone meanwhile would have it applied in front instead: dropped)
        self.events.jobs().spawn(move || {
            let line = crate::gitrepo::repo(&root).map(|r| git_line(&r));
            move |ed: &mut Editor| {
                ed.rebase_lines_read += 1;
                if ed.workspace.id != asked {
                    return;
                }
                if let Err(e) = line.and_then(|g| ed.rebase_run(&name, cmd(&g.text), g.env, aborting)) {
                    unstarted(ed, e);
                }
            }
        });
    }

    /// `rebase_git`'s line started, with its environment.
    fn rebase_run(
        &mut self,
        name: &str,
        cmd: String,
        env: Vec<(String, String)>,
        aborting: bool,
    ) -> Result<(), String> {
        let root = self.workspace.root.clone();
        let task = crate::task::Task { name: name.into(), cmd, cwd: root.clone() };
        let generation = self.task_start_git(task, &env).ok_or("git didn't start")?;
        if let Some(r) = self.workspace.rebase.as_mut() {
            r.stage = Stage::Running { generation, aborting };
        }
        self.rebase_index_watch();
        // (its reloads said as one line once it's over — frictions29 #68; this run's, by its generation)
        self.disk_batch_start("rebase", generation);
        poll(self, root, generation, None);
        Ok(())
    }

    /// `y` on the base-moved card: its session to the front, and its rebase looked at, then started.
    pub fn rebase_start(&mut self, card: u64, m: Moved) {
        // (a pane's key has its document lent: its session to the front, and the rest, once it's back)
        if crate::docpane::lent(self) {
            crate::docpane::later(self, move |ed| ed.rebase_start(card, m));
            return;
        }
        let Some(id) = self.session_at(&m.root) else {
            return self.offer_finished(card, Err(format!("{} isn't open any more", m.name)));
        };
        // (another bring under way: in line behind it, rebased in its turn onto the tip as it is then — #371)
        if self.bring_waits(id) {
            self.offer_finished(card, Ok(()));
            return self.bring_join(id);
        }
        self.workspace_switch(id);
        self.offer_finished(card, Ok(()));
        // (the one bring card's `y`: on to the preflight and the bring once it's rebased — `bring_go_on`)
        (self.workspace.bring_go, self.workspace.bring_rebased) = (true, false);
        self.rebase_check(m, false);
    }

    /// What would hold the rebase back in the front session's checkout, looked at before git runs (#188: it
    /// failed in the task panel, the card offering only a retry): a rebase already there — its card, to go
    /// on or abort; a run in the task panel — the rebase waits its turn behind it (`queued.rs`); changes in
    /// no commit, or unsaved buffers there — named on a card (`Held`) offering what fits. `stash`: its changes
    /// are set aside for it (the card's `s`, `rebase_go`) — only unsaved buffers hold it then.
    pub(crate) fn rebase_check(&mut self, m: Moved, stash: bool) {
        let id = self.workspace.id;
        if self.workspace.rebase.is_some() {
            if let Err(e) = self.rebase_offer(id) {
                self.set_status(e);
            }
            return;
        }
        if self.rebase_queued(&m, stash) {
            return;
        }
        let root = self.workspace.root.clone();
        let unsaved: Vec<String> = self
            .docs
            .iter()
            .filter(|d| d.is_modified() && d.path.as_ref().is_some_and(|p| p.starts_with(&root)))
            .map(|d| d.display_name(&root))
            .collect();
        self.events.jobs().spawn(move || {
            let found = under_way(&root).ok_or_else(|| changed(&root).unwrap_or_default());
            // (changes an earlier rebase set aside, still there — tarae gone midway, a stash entry that failed: this
            // one's end would take them for its own; under way, they're its own)
            let at = crate::aside::REBASE;
            let leftover = match found {
                Ok(_) => None,
                Err(_) => crate::aside::kept(&root, at).map(|sha| crate::aside::kept_at(&root, at, &sha)),
            };
            move |ed: &mut Editor| match (found, leftover) {
                // (one tarae didn't start, or from before a restart: taken up, its card up)
                (Ok(stop), _) => {
                    ed.rebase_adopt(stop);
                    if let Err(e) = ed.rebase_offer(id) {
                        ed.set_error(format!("rebase {}: {e}", m.branch));
                    }
                    ed.bring_queue_rebased(Err("a rebase was under way there — its card".into()));
                }
                (Err(_), Some(kept)) => {
                    ed.set_error(format!(
                        "rebase {}: not started — an earlier rebase's changes in no commit are still set aside \
                         there: {kept}",
                        m.branch
                    ));
                    ed.bring_queue_rebased(Err("changes an earlier rebase set aside are still there".into()));
                }
                (Err(changed), None) if !unsaved.is_empty() || (!stash && !changed.is_empty()) => {
                    ed.push_offer(Held { moved: m, changed, unsaved, bring: false }.card());
                    // (the queue never stashes for you: its card asks, a single bring after)
                    ed.bring_queue_rebased(Err(
                        "changes not in a commit in its checkout — its card asks".into()
                    ));
                }
                (Err(changed), None) => ed.rebase_go(m, if stash { changed } else { Vec::new() }),
            }
        });
    }

    /// A run in the front session's task panel (or its checkout being moved onto its base — `keepclose.rs`): the
    /// rebase waits its turn behind it — true if it does.
    fn rebase_queued(&mut self, m: &Moved, stash: bool) -> bool {
        let busy = self.workspace.task_run.as_ref().is_some_and(|r| r.state == crate::task::State::Running)
            || self.workspace.moving.is_some();
        if busy {
            crate::queued::push(self, crate::queued::What::Rebase { moved: Box::new(m.clone()), stash });
        }
        busy
    }

    /// The rebase started in the front session's task panel — its changes `stashed` for it first when there are
    /// some: set aside on a commit object of their own at `aside::REBASE` as git starts (the same moment
    /// `--autostash` takes them, never its stash), put back at its end (`changes_back`).
    fn rebase_go(&mut self, m: Moved, stashed: Vec<String>) {
        // (a run took the panel while the checkout was looked at: behind it)
        if self.workspace.rebase.is_some() || self.rebase_queued(&m, !stashed.is_empty()) {
            return;
        }
        let aside = !stashed.is_empty();
        self.workspace.rebase = Some(Rebase {
            branch: m.branch.clone(),
            base: m.base.clone(),
            stage: Stage::Stopped,
            step: None,
            stop: Stop::default(),
            asked: false,
            waiting: None,
            also: Besides::default(),
            stashed,
            resolved_at: None,
            messages: Default::default(),
            rechecks: (0, 0),
            index: None,
        });
        let base = crate::packs::sh_quote(&m.base);
        let branch = m.branch.clone();
        self.rebase_git(
            &format!("rebase onto {}", m.base),
            move |git| match aside {
                false => format!("{git} rebase {base}"),
                // (none to take, nothing made; a ref already there refuses it, git's words in the panel)
                true => format!(
                    "c=$({git} stash create) && {{ [ -z \"$c\" ] || {{ {git} update-ref -m 'tarae: set aside' {at} \
                     \"$c\" '' && {git} reset --quiet --hard; }}; }} && {git} rebase {base}",
                    at = crate::aside::REBASE,
                ),
            },
            false,
            Box::new(move |ed, e| {
                (ed.workspace.rebase, ed.workspace.bring_go) = (None, false);
                if !ed.bring_queue_rebased(Err(format!("the rebase didn't start: {e}"))) {
                    ed.set_error(format!("rebase {branch}: {e}"));
                }
            }),
        );
    }

    /// `s` on the held card: the rebase, its changes stashed for it and put back after.
    pub fn rebase_held_stash(&mut self, h: Held) {
        if crate::docpane::lent(self) {
            crate::docpane::later(self, move |ed| ed.rebase_held_stash(h));
            return;
        }
        let Some(id) = self.session_at(&h.moved.root) else { return };
        self.workspace_switch(id);
        self.rebase_check(h.moved, true);
    }

    /// `g` on the held card: its session's git tab, with the keys — the changes to commit, stash or drop.
    pub fn rebase_held_show(&mut self, h: Held) {
        if crate::docpane::lent(self) {
            crate::docpane::later(self, move |ed| ed.rebase_held_show(h));
            return;
        }
        let Some(id) = self.session_at(&h.moved.root) else { return };
        self.workspace_switch(id);
        let shown =
            self.workspace.viewer.sidebar.as_ref().is_some_and(|s| s.mode == crate::sidebar::Mode::Git)
                && self.workspace.is_at(crate::workspace::Here::Side);
        if !shown {
            self.sidebar_show(crate::sidebar::Mode::Git);
        }
    }

    /// A git run of the rebase ended — `end` as git left it, `exit` as its process did.
    fn rebase_ended(&mut self, generation: u64, end: Ended, aside: Aside, exit: crate::task::State) {
        use crate::task::State;
        // (this run's reloads, whatever became of the rebase meanwhile: never left holding its session's toasts)
        self.disk_batch_end(generation);
        let running = |r: &Rebase| matches!(r.stage, Stage::Running { generation: g, .. } if g == generation);
        let Some((branch, base)) =
            self.workspace.rebase.as_ref().filter(|r| running(r)).map(|r| (r.branch.clone(), r.base.clone()))
        else {
            return;
        };
        // The panel says it as it is: stopped at its conflicts (not failed), done, aborted — or git's exit
        let state = match &end {
            Ended::Stopped(stop, _) if !stop.files.is_empty() => {
                let n = stop.files.len();
                State::Stopped(format!("{n} conflict{}", if n == 1 { "" } else { "s" }))
            }
            Ended::Done { .. } | Ended::Aborted => State::Passed,
            _ => exit,
        };
        if let Some(t) = self.workspace.task_run.as_mut().filter(|t| t.generation() == generation) {
            t.state = state;
        }
        let name = self.workspace.label().to_string();
        self.git_refresh();
        // (HEAD moved: its buffers' git bases and the blame, now — not when the watch next looks)
        let root = self.workspace.root.clone();
        self.git_reload_bases_in(&root);
        match end {
            Ended::Stopped(stop, ask) => {
                let at = stop.at();
                if let Some(r) = self.workspace.rebase.as_mut() {
                    (r.stage, r.step, r.asked) = (Stage::Stopped, stop.step, false);
                    r.stop = *stop;
                    r.also = Besides::default();
                    (r.messages.proposed, r.messages.editing, r.messages.written) = (None, None, None);
                }
                match ask {
                    Some(a) => self.rebase_ask(a),
                    None => self.set_warning(format!(
                        "the rebase stopped at {at} — the task panel says why; :rebase-continue or :rebase-abort"
                    )),
                }
                // (a conflict stops only this one: its Claude resolves it as for a single bring)
                self.bring_queue_rebased(Err(format!(
                    "its rebase stopped at {at} — bring it once it's resolved"
                )));
            }
            Ended::Done { ahead, tip, moved } => {
                let (stashed, kept) =
                    self.workspace.rebase.take().map(|r| (r.stashed, r.messages.kept)).unwrap_or_default();
                self.rebase_cards_gone();
                // (the one bring card's `y` goes on by itself — but past a message a resolution kept, which a
                // person reads first: its card then)
                // (kept while it goes — its words say `bringing · rebased` until the preflight takes over)
                let go = self.workspace.bring_go
                    && kept.is_empty()
                    && self.workspace.id != crate::event::WorkspaceId::HOME;
                (self.workspace.bring_go, self.workspace.bring_rebased) = (go, go);
                // (#223: the base moved on while a stop waited — rebased onto where it was, not its tip)
                let since = match (moved, go) {
                    (0, true) => " — the preflight next, then the bring".to_string(),
                    (0, false) => String::new(),
                    (n, true) => {
                        format!("; {base} moved {} since — rebasing again", crate::worktree::commits(n))
                    }
                    (n, false) => format!(
                        "; {base} moved {} since — its card rebases again",
                        crate::worktree::commits(n)
                    ),
                };
                let said = format!(
                    "Rebased {branch} onto {base} — {} on top, now at {tip}{since}{}",
                    crate::worktree::commits(ahead),
                    aside_said(&aside, &stashed)
                );
                // (a bring under way says where it is in its session's words, and one toast at its end — the user,
                // 2026-11-03: the rebase's own is recorded only)
                match aside {
                    Aside::Kept(_) => self.set_warning(said),
                    _ if go => self.recorded(crate::editor::ToastKind::Success, said),
                    _ => self.set_success(said),
                }
                // (its instruction files: the main checkout's changes since its copy — #27)
                crate::instructions::follow(self);
                // (a session's: bring it now? The checkout tarae started in has no base to go into — the bring
                // queue's one under way goes on to its preflight instead)
                // (the queue looks again, and rebases again if the base moved — `bring_queue_rebased`)
                if self.workspace.id != crate::event::WorkspaceId::HOME && !self.bring_queue_rebased(Ok(())) {
                    let (root, quick, sid) =
                        (self.workspace.root.clone(), self.preflight_quick(), self.workspace.id);
                    let m = Moved { root: root.clone(), name, branch, base: base.clone(), moved, ahead };
                    if go {
                        return self.bring_go_on(m, quick);
                    }
                    // (not on the base's tip: the bring's fast-forward can't land — the bring card again, as
                    // `:workspace-bring` gives)
                    let mut card = match moved {
                        0 => m.bring_card(quick, true),
                        _ => m.card(quick),
                    };
                    // (frictions29 #25: a resolution may have made a message wrong — named before it's brought)
                    if !kept.is_empty() {
                        let quoted: Vec<String> = kept.iter().map(|s| format!("«{s}»")).collect();
                        card.question.push_str(&format!(
                            "\n\nKept {} through a resolved conflict: {} — read {} before bringing (the git \
                             tab's log).",
                            if kept.len() == 1 { "its message" } else { "their messages" },
                            quoted.join(", "),
                            if kept.len() == 1 { "it" } else { "them" },
                        ));
                    }
                    match moved {
                        0 => crate::codereview::push_bring_card(self, sid, card, root, base),
                        _ => self.push_offer(card),
                    }
                }
            }
            Ended::Aborted => {
                self.workspace.bring_go = false;
                let stashed = self.workspace.rebase.take().map(|r| r.stashed).unwrap_or_default();
                self.rebase_cards_gone();
                let said =
                    format!("Rebase aborted — {branch} is back where it was{}", aside_said(&aside, &stashed));
                match aside {
                    Aside::Kept(_) => self.set_warning(said),
                    _ => self.set_status(said),
                }
                self.bring_queue_rebased(Err("its rebase was aborted".into()));
            }
            // (git's own words, its `error:`/`fatal:` lines — #188: "the task panel says why" sent you looking)
            Ended::Failed => {
                self.workspace.bring_go = false;
                let stashed = self.workspace.rebase.take().map(|r| r.stashed).unwrap_or_default();
                let why: Vec<String> = self
                    .workspace
                    .task_run
                    .as_ref()
                    .filter(|t| t.generation() == generation)
                    .map(|t| t.output.iter().map(|(_, l)| l.trim().to_string()).collect())
                    .unwrap_or_default();
                let why: Vec<String> = why
                    .into_iter()
                    .filter(|l| l.starts_with("error:") || l.starts_with("fatal:"))
                    .take(2)
                    .collect();
                let why = match why.is_empty() {
                    true => "the task panel says why".to_string(),
                    false => format!("git said: {}", why.join(" ")),
                };
                self.set_error(format!(
                    "rebase {branch}: git stopped before it began — {why}{}",
                    aside_said(&aside, &stashed)
                ));
                self.bring_queue_rebased(Err("its rebase failed before it began".into()));
            }
        }
    }

    /// The session's Claude asked to resolve the conflicts (its chat opened if it wasn't) — or, while it's
    /// answering something else, once that turn ends.
    fn rebase_ask(&mut self, a: Ask) {
        if self.workspace.claude.chat.is_none() {
            // (the keys where they were — out of sight)
            let at = self.workspace.here();
            crate::chat::open(self);
            self.workspace.set_at(at);
            if let Some(c) = &mut self.workspace.claude.chat {
                c.hidden = true;
            }
        }
        let Some(r) = self.workspace.rebase.as_mut() else { return };
        let files = named(&r.stop.files);
        if self.workspace.claude.chat.as_ref().is_some_and(crate::chat::Chat::busy) {
            r.waiting = Some(a);
            return self.set_status(format!("conflicts in {files} — Claude is asked once its turn ends"));
        }
        let sent = crate::chat::submit(self, &a.question, Some(a.about.clone()), Some(a.chip.clone()));
        // (frictions34 41: a chat whose claude isn't running — none started since tarae did — starts it once the
        // project's Claude settings are read, which the send began — only that first read sets `waiting`, and its
        // end opens the gate: the ask goes then, `chat_ready`)
        let starting = !sent && self.workspace.claude.chat.as_ref().is_some_and(|c| c.starts_later());
        let Some(r) = self.workspace.rebase.as_mut() else { return };
        match (sent, starting) {
            (true, _) => {
                r.asked = true;
                self.set_status(format!(
                    "conflicts in {files} — Claude is resolving them; you review them before the rebase goes on"
                ))
            }
            (false, true) => {
                r.waiting = Some(a);
                self.set_status(format!("conflicts in {files} — Claude is asked once it starts"))
            }
            // (not asked: it waits on you — the inbox lists it, its card goes on or aborts)
            (false, false) => self.set_warning(format!(
                "conflicts in {files} — couldn't reach Claude (the chat says why); resolve them, then \
                 :rebase-continue"
            )),
        }
    }

    /// The markers looked for after Claude's turn: some left — named; none — the review of the resolved
    /// files, and of what else it changed (`also`, named on the card, never staged), against the commit being
    /// replayed, and the card.
    fn rebase_checked(&mut self, left: Vec<String>, also: Besides) {
        let Some(r) = self.workspace.rebase.as_mut().filter(|r| r.stage == Stage::Stopped) else { return };
        if !left.is_empty() {
            return self.set_warning(format!(
                "conflict markers still in {} — ask Claude again, or fix them and :rebase-continue",
                named(&left)
            ));
        }
        r.stage = Stage::Resolved;
        r.resolved_at = Some(std::time::Instant::now());
        r.also = also;
        let mut stop = r.stop.clone();
        stop.files.extend(r.also.all());
        self.rebase_index_watch();
        match stop.commit {
            // (the card once the review is open — it's about what the review shows)
            Some((sha, short, subject)) => {
                self.workspace.leave(crate::workspace::Here::Chat);
                let root = self.workspace.root.clone();
                let label = format!("{short} {subject}");
                crate::codereview::start_replay(
                    self,
                    root,
                    sha,
                    label,
                    stop.files,
                    Editor::rebase_resolved_card,
                );
            }
            None => self.rebase_resolved_card(),
        }
    }

    /// The resolved card: continue · abort · not now — while the rebase is still resolved and waiting.
    fn rebase_resolved_card(&mut self) {
        if self.workspace.rebase.as_ref().is_some_and(|r| r.stage == Stage::Resolved) {
            let o = self.rebase_card();
            self.push_offer(o);
        }
    }

    /// The card of the front session's rebase, stopped (not git at work): `y` stages its files and goes on ·
    /// `X` aborts · `n` not now. Resolved, it's about the review up; stopped short of that, about where it
    /// stopped. Either way it says the files aren't staged yet — `y` stages them (#175: the card offered to
    /// continue while the git tab showed them unmerged, and nothing said `y` would stage them).
    fn rebase_card(&self) -> Offer {
        let r = self.workspace.rebase.as_ref().expect("a rebase");
        let (stop, branch, name) = (&r.stop, &r.branch, self.workspace.label());
        let n = stop.files.len() + r.also.tracked.len() + r.also.made.len();
        let shown = if n == 1 { "it" } else { "them" };
        let them = if stop.files.len() == 1 { "it" } else { "them" };
        let resolved = r.stage == Stage::Resolved;
        // (short: the card shares the code's room with the review it's about and the task panel — what `y`
        // does, its button says)
        // (#206: what else changed is never staged on `y` — a tracked file holds it, git wouldn't go on with one
        // unstaged; a made one is left untracked, or staged by `a`)
        let (tracked, made) = (&r.also.tracked, &r.also.made);
        let it = |v: &[String]| if v.len() == 1 { "it" } else { "them" };
        let is = |v: &[String]| if v.len() == 1 { "is" } else { "are" };
        // (`a` once nothing holds `y` — it stages what `y` does, and these)
        let stages_made = resolved && tracked.is_empty() && !made.is_empty();
        let mut also = String::new();
        if !tracked.is_empty() {
            also.push_str(&format!(" It also changed {} — not staged by tarae.", named(tracked)));
        }
        if !made.is_empty() {
            also.push_str(&format!(
                " It made {}, untracked: {}y goes on without {}.",
                named(made),
                if stages_made { format!("a stages {} too, ", it(made)) } else { String::new() },
                it(made),
            ));
        }
        // (what holds `y` — its answer gone from the card, the key refused on it: `Offer.held`)
        let held = (resolved && !tracked.is_empty()).then(|| {
            format!(
                "y waits until {} {} staged or undone — the git tab (space g)",
                named(tracked),
                is(tracked)
            )
        });
        let (title, first) = match (r.stage, stop.files.is_empty()) {
            (Stage::Resolved, _) => (
                format!("{name}: conflicts resolved"),
                format!(
                    "{} resolved for {}.{also} The review shows {shown} against that commit — not staged yet: \
                     y stages {them} and goes on{}.",
                    named(&stop.files),
                    stop.at(),
                    match tracked.is_empty() {
                        true => String::new(),
                        false => format!(
                            " once {} {} staged or undone in the git tab (space g)",
                            it(tracked),
                            is(tracked)
                        ),
                    },
                ),
            ),
            (_, false) => (
                format!("{name}: {}", r.said()),
                format!(
                    "Rebasing {branch} onto {} stopped at {}. {} {} unmerged: y stages {them} once no conflict \
                     markers are left, and goes on.",
                    r.base,
                    stop.at(),
                    named(&stop.files),
                    if stop.files.len() == 1 { "is" } else { "are" },
                ),
            ),
            (_, true) => (
                format!("{name}: {}", r.said()),
                format!(
                    "Rebasing {branch} onto {} stopped at {} — the task panel says why. y goes on.",
                    r.base,
                    stop.at(),
                ),
            ),
        };
        // (frictions29 #25: resolved, the commit's message may no longer hold — `m` writes it first)
        let message = resolved && stop.message.is_some();
        let said = match (message, r.messages.proposed.is_some()) {
            (false, _) => String::new(),
            (true, true) => {
                "Claude says the commit's message no longer holds — m shows its version; :w there \
                             goes on with it.\n\n"
                    .to_string()
            }
            (true, false) => {
                "m writes the commit's message first — :w there goes on with it.\n\n".to_string()
            }
        };
        let q = format!("{first}\n\n{said}X aborts the rebase: {branch} goes back as it was.");
        let x = q.lines().count() - 1;
        let root = self.workspace.root.clone();
        let mut o = Offer::new(
            title,
            &q,
            "Going on with the rebase…",
            "Couldn't go on with the rebase".into(),
            What::RebaseResolved { root, message, made: stages_made },
        )
        .warning(x..x + 1);
        o.held = held;
        o
    }

    /// What else the resolution of the rebase at `root` changed, read again (#206): `y` on its card while a
    /// tracked file held it, or the git tab's status read (a stage or undo there) — changed since the card,
    /// the card says it as it now stands (the same card, its `rev` up: a key meant for what it said isn't an
    /// answer to what it says now).
    pub(crate) fn rebase_recheck(&mut self, root: &Path) {
        let Some(id) = self.session_at(root) else { return };
        self.with_workspace(id, |ed| {
            let Some(r) = ed.workspace.rebase.as_mut().filter(|r| r.stage == Stage::Resolved) else { return };
            if r.also.tracked.is_empty() {
                return;
            }
            r.rechecks.0 += 1;
            let (n, stop) = (r.rechecks.0, r.stop.clone());
            let root = ed.workspace.root.clone();
            ed.events.jobs().spawn(move || {
                let now = besides(&root, &stop.files, stop.before.as_deref());
                move |ed: &mut Editor| {
                    let Some(r) = ed.workspace.rebase.as_mut().filter(|r| r.rechecks.0 == n) else { return };
                    r.rechecks.1 = n;
                    // (still resolved at the stop it read for: gone on since — its card answered — or resolved
                    // again at the next stop, what it read isn't that one's)
                    let there = r.stage == Stage::Resolved && (r.stop == stop || broken(Broken::AnyStop));
                    if !there || r.also == now {
                        // (nothing on screen changed: an index watch's read of a stage elsewhere draws nothing)
                        return ed.unchanged = true;
                    }
                    r.also = now;
                    ed.rebase_card_again();
                    ed.rebase_index_watch();
                }
            });
        });
    }

    /// The front session's checkout's index watched (`watch_index`, #245) exactly while its rebase is resolved and a
    /// tracked file holds `y` (`Rebase.index`): started once then, gone once that no longer holds — freed (a read
    /// again lands with none, which reads again only while one holds), or the rebase going on or aborted
    /// (`rebase_run`: git's own index writes never reach the loop). Not in tests (`Editor.index_watch`).
    fn rebase_index_watch(&mut self) {
        let (root, on) = (self.workspace.root.clone(), self.index_watch);
        let Some(r) = self.workspace.rebase.as_mut() else { return };
        if !on || r.stage != Stage::Resolved || r.also.tracked.is_empty() {
            r.index = None;
            return;
        }
        if r.index.is_some() {
            return;
        }
        let (waker, stop) = (crate::fswatch::Waker::new(), std::sync::Arc::default());
        r.index = Some(IndexWatch { waker: waker.clone(), stop: std::sync::Arc::clone(&stop) });
        watch_index(&self.events, root, waker, stop);
    }

    /// The index of the checkout at `root` changed — whoever changed it (#245): its resolved rebase's checkout read
    /// again (`rebase_recheck`), its card said as it now stands once that lands. Nothing on screen yet.
    pub(crate) fn rebase_index_moved(&mut self, root: &Path) {
        self.drawn_for("a rebase's index watch");
        self.unchanged = true;
        self.rebase_recheck(root);
    }

    /// The card of the rebase in front said again as it now stands (`rebase_card`), wherever it waits — the same
    /// card, its `rev` up: a key meant for what it said isn't an answer to what it says now (the focus lets go).
    /// None up — answered, or gone with the stop — none is put up (one answered `n` waits parked, said again there).
    fn rebase_card_again(&mut self) {
        let new = self.rebase_card();
        let root = self.workspace.root.clone();
        let mine = |o: &Offer| matches!(&o.what, What::RebaseResolved { root: r, .. } if *r == root);
        let parked =
            std::iter::once(&mut self.workspace).chain(&mut self.workspaces).flat_map(|l| &mut l.parked);
        match self.offers.iter_mut().chain(parked).find(|o| mine(o)) {
            Some(o) => {
                let rev = o.rev + u64::from(!broken(Broken::SameRev));
                (o.question, o.warning, o.held) = (new.question, new.warning, new.held);
                (o.what, o.rev) = (new.what, rev);
            }
            None if broken(Broken::BringsBack) => self.push_offer(new),
            None => {}
        }
    }

    /// The resolved card of the rebase in front put back where none waits (`rebase_offer`), what holds it refused
    /// on it (`why` where nothing does) — #206: `y`, `a` and `m`'s `:w` go on with the card gone, and a tracked
    /// file changed since its last read left only a warning then. One answered `n` waits parked: left there.
    /// Your input brought it back, so it's yours (`Offer.yours`) while that input is still the last one (#205):
    /// handed the keys as an answer hands them on (`card_next`) — focused once a frame has drawn it, a key typed
    /// on before then refused on it, never an answer.
    fn rebase_card_back(&mut self, why: &str) {
        let root = self.workspace.root.clone();
        let mine = |o: &Offer| matches!(&o.what, What::RebaseResolved { root: r, .. } if *r == root);
        let waiting = |ed: &Editor| {
            let parked = std::iter::once(&ed.workspace).chain(&ed.workspaces).flat_map(|l| &l.parked);
            ed.offers.iter().chain(parked).find(|o| mine(o)).map(|o| (o.id, o.rev, o.held.clone()))
        };
        if waiting(self).is_some() || broken(Broken::GoneOnRefusal) {
            return;
        }
        let yours = self.events.asked() == Some(self.input_seq) && !broken(Broken::NotYours);
        if self.rebase_offer(self.workspace.id).is_err() {
            return;
        }
        let Some((id, rev, held)) = waiting(self) else { return };
        self.offer_said = Some((id, rev, held.unwrap_or_else(|| why.to_string())));
        // (in front here, not parked in a session behind: the keys handed on to it)
        if let Some(o) = self.offers.iter_mut().find(|o| o.id == id) {
            o.yours = yours;
            if yours && self.offers.first().is_some_and(|f| f.id == id) {
                self.card_next = Some((id, rev));
            }
        }
    }

    /// Session `id`'s rebase met where it's in the way (`:workspace-bring`, #175): its card up in front — `y`
    /// goes on, `X` aborts — instead of a line saying what to type. Git at work there: Err says so.
    pub(crate) fn rebase_offer(&mut self, id: crate::event::WorkspaceId) -> Result<(), String> {
        let mut card = Err(String::new());
        self.with_workspace(id, |ed| {
            let name = ed.workspace.label().to_string();
            card = match ed.workspace.rebase.as_ref() {
                Some(r) if !r.stopped() => Err(r.why(&name)),
                Some(_) => Ok((ed.workspace.root.clone(), ed.rebase_card())),
                None => Err(format!("no rebase under way in {name}")),
            };
        });
        let (root, o) = card?;
        // (one card for it: one left from before goes)
        let mine = |o: &Offer| matches!(&o.what, What::RebaseResolved { root: r, .. } if *r == root);
        self.offers.retain(|o| !mine(o));
        for l in std::iter::once(&mut self.workspace).chain(&mut self.workspaces) {
            l.parked.retain(|o| !mine(o));
        }
        self.push_offer(o);
        Ok(())
    }

    /// `:rebase-continue [<session>]` · `:rebase-abort [<session>]`: the rebase of the session named — else
    /// of the session rebasing: the one in front if it is, the only one if one is, a pick if several (#175:
    /// after the inbox brought another session to the front, it went to that one — "no rebase under way").
    /// None known: each session's checkout is looked at (one from before a restart, or by hand).
    pub fn rebase_by_hand(&mut self, which: &str, go_on: bool) -> Result<(), String> {
        if !which.trim().is_empty() {
            let (id, _) = self.session_named(which)?;
            return self.rebase_in(id, go_on);
        }
        let rebasing: Vec<_> = std::iter::once(&self.workspace)
            .chain(&self.workspaces)
            .filter(|l| l.rebase.is_some())
            .map(|l| l.id)
            .collect();
        if !rebasing.is_empty() {
            return self.rebase_choose(&rebasing, go_on);
        }
        let roots: Vec<_> = std::iter::once(&self.workspace)
            .chain(&self.workspaces)
            .map(|l| (l.id, l.root.clone()))
            .collect();
        self.events.shared_jobs().spawn(move || {
            let found: Vec<_> =
                roots.into_iter().filter_map(|(id, r)| under_way(&r).map(|s| (id, s))).collect();
            move |ed: &mut Editor| {
                let ids: Vec<_> = found.iter().map(|f| f.0).collect();
                for (id, stop) in found {
                    ed.with_workspace(id, |ed| {
                        if ed.workspace.rebase.is_none() {
                            ed.rebase_adopt(stop);
                        }
                    });
                }
                if let Err(e) = ed.rebase_choose(&ids, go_on) {
                    ed.set_error(e);
                }
            }
        });
        Ok(())
    }

    /// Of the sessions rebasing (`ids`), the one to go on with or abort: the front one, the only one, or
    /// the user's pick.
    fn rebase_choose(&mut self, ids: &[crate::event::WorkspaceId], go_on: bool) -> Result<(), String> {
        let front = self.workspace.id;
        match ids {
            [] if self.workspaces.is_empty() => {
                Err(format!("no rebase under way in {}", self.workspace.label()))
            }
            [] => Err("no rebase under way in any session".into()),
            _ if ids.contains(&front) => self.rebase_in(front, go_on),
            [id] => self.rebase_in(*id, go_on),
            _ => {
                let cmd = if go_on { "rebase-continue" } else { "rebase-abort" };
                let items = self
                    .sessions_in_order()
                    .into_iter()
                    .filter(|l| ids.contains(&l.id))
                    .map(|l| crate::picker::Item {
                        label: l.label().to_string(),
                        hint: l
                            .rebase
                            .as_ref()
                            .map(|r| format!("{} onto {}", r.said(), r.base))
                            .unwrap_or_default(),
                        action: crate::picker::Action::Typed(format!("{cmd} {}", l.id)),
                        glyph: None,
                    })
                    .collect();
                let mut p = crate::picker::Picker::of(
                    "rebase-session",
                    format!("{cmd}: which session?"),
                    items,
                    false,
                )
                .without_preview();
                p.compact = true;
                self.open_picker(p, None);
                Ok(())
            }
        }
    }

    /// Session `id`'s rebase gone on with (`go_on`) or aborted — in its own task panel; from behind, said.
    fn rebase_in(&mut self, id: crate::event::WorkspaceId, go_on: bool) -> Result<(), String> {
        let mut done = Ok(());
        let mut name = String::new();
        self.with_workspace(id, |ed| {
            name = ed.workspace.label().to_string();
            done = if go_on { ed.rebase_continue() } else { ed.rebase_abort() };
        });
        if done.is_ok() && id != self.workspace.id {
            let what = if go_on { "goes on" } else { "is aborted" };
            self.set_status(format!("{name}'s rebase {what} — its task panel shows it (space W goes there)"));
        }
        done
    }

    /// `m` on the resolved card, a session's rebase (wherever it is): the replayed commit's message — as
    /// Claude proposes it, else as it is — written to a file in its git folder (a worker) and opened to write.
    /// `:w` goes on with it (`rebase_saved`), `:q` puts the card back (`rebase_message_left`).
    pub fn rebase_message(&mut self, root: &Path) {
        let Some(id) = self.session_at(root) else { return };
        self.with_workspace(id, |ed| {
            let Some(r) = ed.workspace.rebase.as_ref().filter(|r| r.stage == Stage::Resolved) else { return };
            let (proposed, own) = (r.messages.proposed.clone(), r.stop.message.clone().unwrap_or_default());
            let at = r.stop.at();
            let root = ed.workspace.root.clone();
            let said = "writing the commit's message…";
            let work = move || -> Result<PathBuf, String> {
                let dir = git_dir(&root).ok_or("no git folder")?;
                let file = dir.join(MESSAGE_FILE);
                let whose = match proposed.is_some() {
                    true => "Claude's version of the message",
                    false => "the message as it is",
                };
                let text = format!(
                    "{}\n\n{MESSAGE_CUT}\n{whose} — for {at}, after its conflicts were resolved.\n:w stages the files \
                     and goes on with this message · :q leaves it as it was\n",
                    proposed.unwrap_or(own)
                );
                std::fs::write(&file, text).map_err(|e| e.to_string())?;
                Ok(file)
            };
            ed.step(ed.events.jobs(), "rebase-message", said, work, move |ed, file| {
                let file = match file {
                    Ok(f) => f,
                    Err(e) => {
                        ed.set_error(format!("the commit's message: {e}"));
                        return ed.rebase_resolved_card();
                    }
                };
                if ed.workspace.rebase.as_ref().is_none_or(|r| r.stage != Stage::Resolved) {
                    return;
                }
                if let Err(e) = ed.open(&file) {
                    ed.set_error(format!("{e:#}"));
                    return ed.rebase_resolved_card();
                }
                let doc = ed.doc_mut();
                doc.throwaway = true;
                doc.set_selection(crate::selection::Selection::point(0));
                let doc = doc.id;
                if let Some(r) = ed.workspace.rebase.as_mut() {
                    r.messages.editing = Some((doc, file));
                }
                ed.set_status(":w goes on with this message · :q leaves it as it was");
            });
        });
    }

    /// Whether the buffer in front is a rebase's commit message being written (`m`).
    pub fn in_rebase_message(&self) -> bool {
        let id = self.doc().id;
        self.workspace.rebase.as_ref().is_some_and(|r| r.messages.editing.as_ref().is_some_and(|e| e.0 == id))
    }

    /// `:q` in the message being written: it closes, the message stays as it was, and the card is back.
    pub fn rebase_message_left(&mut self) -> Result<(), String> {
        if let Some(r) = self.workspace.rebase.as_mut() {
            r.messages.editing = None;
        }
        self.close_current(true).map_err(|e| format!("{e:#}"))?;
        self.set_status("the commit's message stays as it was — the rebase's card is back");
        // (one card for it: put up again meanwhile by a bring, it isn't doubled)
        let id = self.workspace.id;
        self.rebase_offer(id)
    }

    /// Document `gone` closed (`close_doc`): a rebase's message being written in it, in any session, is left as it
    /// was — said, and the resolved card back (the review of frictions31: closed by `:bd` or a session's end, the
    /// card `m` took away never came back).
    pub(crate) fn rebase_message_closed(&mut self, gone: crate::document::DocId) {
        let writing = |l: &crate::workspace::Workspace| {
            l.rebase.as_ref().and_then(|r| r.messages.editing.as_ref()).is_some_and(|e| e.0 == gone)
        };
        let ids: Vec<_> = std::iter::once(&self.workspace)
            .chain(&self.workspaces)
            .filter(|l| writing(l))
            .map(|l| l.id)
            .collect();
        for id in ids {
            self.with_workspace(id, |ed| {
                if let Some(r) = ed.workspace.rebase.as_mut() {
                    r.messages.editing = None;
                }
                ed.set_status(
                    "the commit's message was closed — it stays as it was; the rebase's card is back",
                );
                let id = ed.workspace.id;
                if let Err(e) = ed.rebase_offer(id) {
                    ed.set_warning(e);
                }
            });
        }
    }

    /// A buffer was saved: the rebase's message being written goes on with it — its buffer closes, the files
    /// are staged and the rebase continues with that message.
    pub fn rebase_saved(&mut self, id: crate::document::DocId) {
        let Some(r) = self.workspace.rebase.as_mut() else { return };
        let Some((_, file)) = r.messages.editing.take_if(|e| e.0 == id) else { return };
        r.messages.written = Some(file);
        self.close_doc(id);
        if let Err(e) = self.rebase_continue() {
            self.set_error(e);
        }
    }

    /// The resolved card's, a session's rebase (wherever it is): `y` continue · `a` continue, the files the
    /// resolution made that the card named staged too · `X` abort.
    pub fn rebase_answer(&mut self, root: &Path, key: char) {
        let Some(id) = self.session_at(root) else { return };
        self.with_workspace(id, |ed| {
            let done = match key {
                'X' => ed.rebase_abort(),
                _ => ed.rebase_go_on(key == 'a'),
            };
            if let Err(e) = done {
                ed.set_error(e);
            }
        });
    }

    /// `:rebase-continue` — the stopped rebase's conflicted files, with no markers left, staged (those git
    /// listed — one the resolution deleted as its deletion, #325 —, nothing else — #206: a whole tree staged took a build folder into a commit) and `git rebase
    /// --continue` (it may stop again) — never with a buffer of the checkout unsaved (`rebase_free`), so what's
    /// staged is the text the markers were looked for in. What else the resolution changed is never staged: a
    /// tracked file changed besides them stops it, named (git wouldn't go on — #129), yours to stage or undo;
    /// an untracked one it made stays untracked, said. A rebase tarae didn't start is taken up first.
    /// The replayed commit goes on with the message written with `m` (`Messages.written`), else as it is — a
    /// resolved stop's then named on the bring card (`Messages.kept`).
    pub fn rebase_continue(&mut self) -> Result<(), String> {
        self.rebase_go_on(false)
    }

    /// `rebase_continue`, and with `made` (the resolved card's `a`) the files the resolution made that its card
    /// named staged with the conflicted ones — still untracked and still not there as git stopped; one made
    /// since the card isn't staged unseen.
    fn rebase_go_on(&mut self, made: bool) -> Result<(), String> {
        let name = self.workspace.label().to_string();
        let Some(r) = self.workspace.rebase.as_ref() else {
            return self.rebase_find(true);
        };
        if let Stage::Running { .. } = r.stage {
            return Err(r.why(&name));
        }
        self.rebase_free()?;
        let mut files = r.stop.files.clone();
        let before = r.stop.before.clone();
        let named_made = if made { r.also.made.clone() } else { Vec::new() };
        let texts = self.rebase_texts(&files);
        let root = self.workspace.root.clone();
        let written = r.messages.written.clone();
        self.events.jobs().spawn(move || {
            let left = marked(&root, texts);
            // (the message written with `m`, tarae's lines cut off, beside it — what git's editor hook copies in)
            let message: Option<Result<PathBuf, String>> = written.map(|f| {
                let text = std::fs::read_to_string(&f).map_err(|e| e.to_string())?;
                let out = f.with_extension("out");
                std::fs::write(&out, message_written(&text)).map_err(|e| e.to_string())?;
                Ok(out)
            });
            // (finished or aborted by hand meanwhile: nothing to go on with — #325's `y` ran `git rebase --continue`
            // with no rebase in progress)
            let ended = under_way(&root).is_none();
            let besides = besides(&root, &files, before.as_deref());
            // (a conflicted path the resolution deleted — a modify/delete resolved as a delete: #325)
            let gone: Vec<String> = (files.iter())
                .filter(|f| {
                    let there = std::fs::symlink_metadata(root.join(f));
                    there.is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                })
                .cloned()
                .collect();
            move |ed: &mut Editor| {
                if ended && !broken(Broken::GoneGoesOn) {
                    return ed.rebase_let_go(" — nothing to go on with");
                }
                let Some(r) = ed.workspace.rebase.as_mut().filter(|r| r.stopped()) else { return };
                if !left.is_empty() {
                    return ed
                        .set_warning(format!("conflict markers still in {} — not going on", named(&left)));
                }
                // (a tracked file changed besides the conflict: git won't go on while it's unstaged, and tarae
                // stages only what the conflict named — #206)
                let (tracked, made) = (&besides.tracked, &besides.made);
                if !tracked.is_empty() {
                    let one = tracked.len() == 1;
                    let msg = format!(
                        "not going on — {} changed besides the conflict and {} unstaged: tarae stages only the \
                         conflicted files — stage or undo {} in the git tab (space g), then :rebase-continue",
                        named(tracked),
                        if one { "is" } else { "are" },
                        if one { "it" } else { "them" },
                    );
                    // (its card says what this found — put back where none waits: `y` and `a` took it away as
                    // they were pressed, and `m`'s `:w` comes here with it gone)
                    if r.also != besides {
                        r.also = besides;
                        ed.rebase_card_again();
                    }
                    ed.rebase_index_watch();
                    ed.rebase_card_back(&msg);
                    return ed.set_warning(msg);
                }
                // (what the resolution made: `a` stages what its card named, `y` goes on without it — said)
                let staged: Vec<String> = made.iter().filter(|f| named_made.contains(f)).cloned().collect();
                let untracked: Vec<String> = made.iter().filter(|f| !staged.contains(f)).cloned().collect();
                files.extend(staged);
                // (a resolved stop's commit going on with its message as it was: named on the bring card)
                r.messages.written = None;
                let message = match message {
                    Some(Err(e)) => return ed.set_error(format!("the commit's message: {e} — not going on")),
                    Some(Ok(f)) => Some(f),
                    None => None,
                };
                if message.is_none()
                    && r.stage == Stage::Resolved
                    && let Some((_, _, subject)) = &r.stop.commit
                {
                    r.messages.kept.push(subject.clone());
                }
                r.messages.editing = None;
                if !untracked.is_empty() {
                    ed.set_status(format!(
                        "going on without {}: {} untracked, not in the commit",
                        named(&untracked),
                        if untracked.len() == 1 { "it stays" } else { "they stay" }
                    ));
                }
                ed.rebase_review_gone();
                ed.rebase_cards_gone();
                let gone = if broken(Broken::DeletionsAdded) { Vec::new() } else { gone };
                let quote = |f: &String| crate::packs::sh_quote(f);
                let quoted: Vec<String> = files.iter().filter(|f| !gone.contains(f)).map(quote).collect();
                let deleted: Vec<String> = match broken(Broken::DeletionsLeft) {
                    true => Vec::new(),
                    false => gone.iter().map(quote).collect(),
                };
                // (the replayed commit's message: written with `m`, put in place by git's own editor hook — so the
                // commit keeps its author and date — else kept as it was, no editor)
                let editor = message.map(|f| {
                    crate::packs::sh_quote(&format!("cp {}", crate::packs::sh_quote(&f.to_string_lossy())))
                });
                // (the conflicted paths as git named them, literally — a `*` in a name is no glob — and nothing
                // else: #206. One the resolution deleted is staged as git's own hint at a modify/delete stop says,
                // `git rm`: `--cached` touches no file, `--ignore-unmatch` goes on where it's gone from the index
                // already — staged before, the git tab's `s` — where `add` fails on a pathspec matching nothing:
                // #325)
                let literal = if broken(Broken::GlobPathspecs) { "" } else { " --literal-pathspecs" };
                let cmd = move |git: &str| {
                    let mut add = String::new();
                    if !quoted.is_empty() {
                        add.push_str(&format!("{git}{literal} add -- {} && ", quoted.join(" ")));
                    }
                    if !deleted.is_empty() {
                        let rm = "rm -q --cached --ignore-unmatch";
                        add.push_str(&format!("{git}{literal} {rm} -- {} && ", deleted.join(" ")));
                    }
                    match &editor {
                        // (`whitespace`: git keeps the message's own `#` lines — tarae cut its own already)
                        Some(e) => {
                            format!(
                                "{add}GIT_EDITOR={e} {git} -c commit.cleanup=whitespace rebase --continue"
                            )
                        }
                        None => format!("{add}GIT_EDITOR=true {git} rebase --continue"),
                    }
                };
                let said: Unstarted = Box::new(|ed, e| ed.set_error(format!("rebase: {e}")));
                ed.rebase_git("rebase --continue", cmd, false, said);
            }
        });
        Ok(())
    }

    /// `:rebase-abort` — `git rebase --abort`: the branch back where it was before the rebase.
    pub fn rebase_abort(&mut self) -> Result<(), String> {
        let name = self.workspace.label().to_string();
        let Some(r) = self.workspace.rebase.as_ref() else {
            return self.rebase_find(false);
        };
        if let Stage::Running { .. } = r.stage {
            return Err(r.why(&name));
        }
        self.rebase_free()?;
        self.rebase_review_gone();
        self.rebase_cards_gone();
        let said: Unstarted = Box::new(|ed, e| ed.set_error(format!("rebase: {e}")));
        self.rebase_git("rebase --abort", |git| format!("{git} rebase --abort"), true, said);
        Ok(())
    }

    /// `:rebase-continue`/`:rebase-abort` with no rebase of tarae's here: one under way in the checkout
    /// (from before a restart, or started by hand) is taken up, then gone on with (`go_on`) or aborted.
    fn rebase_find(&mut self, go_on: bool) -> Result<(), String> {
        let root = self.workspace.root.clone();
        self.events.jobs().spawn(move || {
            let stop = under_way(&root);
            move |ed: &mut Editor| {
                let Some(stop) = stop else {
                    let name = ed.workspace.label().to_string();
                    return ed.set_error(format!("no rebase under way in {name}"));
                };
                if ed.workspace.rebase.is_none() {
                    ed.rebase_adopt(stop);
                }
                let done = if go_on { ed.rebase_continue() } else { ed.rebase_abort() };
                if let Err(e) = done {
                    ed.set_error(e);
                }
            }
        });
        Ok(())
    }

    /// A rebase found under way in the front session's checkout, taken up as stopped there.
    pub(crate) fn rebase_adopt(&mut self, stop: Stop) {
        let s = &self.workspace;
        let branch = stop.head.clone().or_else(|| s.branch.clone()).unwrap_or_else(|| "HEAD".into());
        let base = s
            .base
            .clone()
            .or_else(|| stop.onto.as_ref().map(|o| o.chars().take(7).collect()))
            .unwrap_or_default();
        self.workspace.rebase = Some(Rebase {
            branch,
            base,
            stage: Stage::Stopped,
            step: stop.step,
            stop,
            asked: false,
            waiting: None,
            also: Besides::default(),
            // (one tarae didn't start, or started before a restart: changes set aside for it are found at its end
            // by their ref — `changes_back`)
            stashed: Vec::new(),
            resolved_at: None,
            messages: Default::default(),
            rechecks: (0, 0),
            index: None,
        });
    }

    /// The review of a stop's resolution ends (the rebase goes on, or is aborted).
    fn rebase_review_gone(&mut self) {
        let replay = self
            .workspace
            .code_review
            .as_ref()
            .is_some_and(|r| matches!(r.base, crate::codereview::Base::Replay { .. }));
        if replay {
            crate::codereview::close(self);
        }
    }

    /// The front session's checkout's HEAD moved (`git::watch_round`) while its rebase waits stopped or resolved:
    /// git asked whether it's still under way — one ended outside tarae (continued or aborted by hand, #325) is let
    /// go (`rebase_let_go`). A rebase of tarae's running moves HEAD itself: never looked at then.
    pub(crate) fn rebase_head_moved(&mut self) {
        if broken(Broken::HeadMoveKept) || !self.workspace.rebase.as_ref().is_some_and(Rebase::stopped) {
            return;
        }
        let root = self.workspace.root.clone();
        self.events.jobs().spawn(move || {
            let gone = under_way(&root).is_none();
            move |ed: &mut Editor| match gone {
                true => ed.rebase_let_go(""),
                // (nothing on screen changes: a HEAD move within the stop — a `git commit` there)
                false => ed.unchanged = true,
            }
        });
    }

    /// The front session's rebase, which git no longer has under way, let go — ended outside tarae: its cards,
    /// review and inbox row gone, a bring waiting on it told, and the changes set aside for it put back as any end
    /// puts them (`changes_back`), said with `more`. Only while it's stopped or resolved: one of tarae's running
    /// ends by its own run.
    fn rebase_let_go(&mut self, more: &str) {
        let stopped = |r: &mut Rebase| r.stopped() || broken(Broken::LetGoAtWork);
        let Some(Rebase { branch, base, stashed, .. }) = self.workspace.rebase.take_if(stopped) else {
            return;
        };
        self.rebase_review_gone();
        self.rebase_cards_gone();
        self.workspace.bring_go = false;
        self.bring_queue_rebased(Err("its rebase ended outside tarae".into()));
        let (root, name, more) =
            (self.workspace.root.clone(), self.workspace.label().to_string(), more.to_string());
        self.events.jobs().spawn(move || {
            let aside = changes_back(&root, &branch, &base);
            move |ed: &mut Editor| {
                let said =
                    format!("{name}: the rebase ended outside tarae{more}{}", aside_said(&aside, &stashed));
                match aside {
                    Aside::Kept(_) => ed.set_warning(said),
                    _ => ed.set_status(said),
                }
            }
        });
    }

    /// The resolved card of the front session's rebase goes (answered by hand).
    fn rebase_cards_gone(&mut self) {
        let root = self.workspace.root.clone();
        let mine = |o: &Offer| matches!(&o.what, What::RebaseResolved { root: r, .. } if *r == root);
        self.offers.retain(|o| !mine(o));
        self.workspace.parked.retain(|o| !mine(o));
    }

    /// `y` (or `p`) on the plain bring card: the preflight, then the bring — in line behind another bring under way
    /// (#371).
    pub fn bring_asked(&mut self, m: Moved) {
        match self.session_at(&m.root) {
            Some(id) if self.bring_waits(id) => self.bring_join(id),
            _ => self.rebase_preflight_then_bring(m, false),
        }
    }

    /// `y` on the bring card.
    pub fn rebase_bring(&mut self, m: Moved) {
        let Some(id) = self.session_at(&m.root) else { return };
        if let Err(e) = self.workspace_bring_now(&id.to_string()) {
            self.set_error(e);
        }
    }

    /// What session `l`'s state words say of a bring or rebase under way there, where that session is named (the
    /// strip, the list, the chip): `rebasing 2/5` — and a bring its card's `y` asked for (polish landing 4, the user
    /// 2026-11-03: its progress said there, never on the card nor a toast a step) `bringing · rebasing 2/5`, then
    /// `bringing · rebased · preflight 2/4`, or `… · waits on trust` while its trust card holds its steps back (#323).
    /// The bring queue's says its own (`bring_said`).
    pub(crate) fn bring_progress(&self, l: &crate::workspace::Workspace) -> Option<String> {
        let queued = self.bring_queue_at(l.id).is_some();
        if let Some(r) = &l.rebase {
            return Some(match l.bring_go && !queued {
                true => format!("bringing · {}", r.said()),
                false => r.said(),
            });
        }
        // (its preflight passed, the bring itself landing)
        if l.bring_landing && !queued {
            return Some(
                if l.bring_rebased { "bringing · rebased · landing" } else { "bringing · landing" }.into(),
            );
        }
        // (its checkout looked at before the rebase starts; graded after it, before its preflight)
        if l.bring_go && !queued {
            return Some(if l.bring_rebased { "bringing · rebased" } else { "bringing" }.into());
        }
        if !l.bring_after_preflight || queued {
            return None;
        }
        let rebased = if l.bring_rebased { "rebased · " } else { "" };
        if l.bring_trust {
            return Some(format!("bringing · {rebased}waits on trust"));
        }
        let at = l.preflight.as_ref().filter(|p| p.took.is_none()).map(|p| {
            let done = p.steps.iter().filter(|s| s.state != crate::preflight::State::Waiting).count();
            format!(" {}/{}", done.max(1), p.steps.len())
        });
        Some(format!("bringing · {rebased}preflight{}", at.unwrap_or_default()))
    }

    /// Whether session `l`'s bring waits on its trust card (#323) — on you, its words in the warning colour.
    pub(crate) fn bring_waits_trust(&self, l: &crate::workspace::Workspace) -> bool {
        l.bring_trust && l.bring_after_preflight && self.bring_queue_at(l.id).is_none()
    }

    /// Whether session `l`'s bring goes on by itself now — its card's `y` (rebase, preflight, bring) or a preflight
    /// a bring waits for; the bring queue's is the queue's.
    pub(crate) fn bring_going(&self, l: &crate::workspace::Workspace) -> bool {
        (l.bring_go || l.bring_after_preflight || l.bring_landing) && self.bring_queue_at(l.id).is_none()
    }

    /// `fix is being brought — bringing · rebased · preflight 2/4`: what a bring asked while one goes there says.
    pub(crate) fn being_brought(&self, l: &crate::workspace::Workspace) -> String {
        let at = self.bring_progress(l).unwrap_or_else(|| "bringing".into());
        format!("{} is being brought — {at}", l.label())
    }

    /// The one bring card's `y` after its rebase landed (`m`, on its base's tip unless the base moved on meanwhile):
    /// rebased again, or graded — the held card up while high-risk changes wait for a look, else the quick preflight
    /// (`quick`: its steps) and the bring, a failure its card back (`rebase_bring_failed`).
    pub(crate) fn bring_go_on(&mut self, m: Moved, quick: Option<Vec<String>>) {
        // (another bring went on meanwhile — it stopped at a conflict, the line didn't wait: first in line, looked at
        // again in its turn — #371)
        if self.bring_goes_first_in_line(self.workspace.id) {
            return;
        }
        if m.moved > 0 {
            self.workspace.bring_go = true;
            return self.rebase_check(m, false);
        }
        let (sid, root, base) = (self.workspace.id, m.root.clone(), m.base.clone());
        crate::codereview::bring_go_graded(self, sid, m.bring_card(quick, true), root, base);
    }

    /// `y` on the move-up card: its branch fast-forwarded to its base's tip, in its checkout (a worker).
    pub fn rebase_move_up(&mut self, id: u64, m: Moved) {
        self.offer_finished(id, Ok(()));
        let unsaved: Vec<PathBuf> =
            self.docs.iter().filter(|d| d.is_modified()).filter_map(|d| d.path.clone()).collect();
        let said = format!("Moving {} up to {}…", m.name, m.base);
        let work = move || {
            let r = move_up(&m, &unsaved);
            (m, r)
        };
        self.step(self.events.shared_jobs(), "session", said, work, |ed, (m, r)| {
            match r {
                Ok(tip) => ed.set_success(format!("Moved {} up to {} — now at {tip}", m.name, m.base)),
                Err(e) => ed.set_error(format!("move {} up: {e}", m.name)),
            }
            ed.git_refresh();
            ed.git_reload_bases();
        });
    }

    /// The bring card `m` again after its `p`'s preflight (`title`) failed (frictions34 42: a toast said it, and
    /// the output went with the task panel's next run): the first test that failed and why, in the warning
    /// colour, and `v` for the failed step's output (`Offer.output` — kept on the card, whatever runs next).
    fn rebase_bring_failed(&mut self, m: Moved, title: &str) {
        let p = self.workspace.preflight.as_ref();
        let failed = p.and_then(|p| p.step_text("", None, &p.kept).ok());
        let how = p.map(|p| Unpassed { failed: p.failed(), not_run: p.not_run() }).unwrap_or_default();
        let output = failed.clone();
        let head = self.workspace.bring_tested.take();
        let card = m.failed_card(title, &how, failed);
        self.workspace.bring_failed = Some(Failed { title: title.into(), how, output, head });
        let (sid, root, base) = (self.workspace.id, m.root.clone(), m.base.clone());
        crate::codereview::push_bring_card(self, sid, card, root, base);
    }

    /// `p` on the bring card: the quick preflight in its session, and the bring once it passes (the full one
    /// is for before publishing). The commit it tests is read first, on a worker, and the preflight starts once
    /// it's in: should it fail, the failure is that commit's (`Workspace.bring_tested` → `Failed.head`) — not one
    /// Claude made while it ran, which the review of frictions29 91 found recorded as the failing one, the
    /// commit that failed getting the plain card again.
    /// `rebased`: the one bring card's `y` rebased it first (its state words say so).
    pub fn rebase_preflight_then_bring(&mut self, m: Moved, rebased: bool) {
        let Some(id) = self.session_at(&m.root) else { return };
        // (going from here: no card asked meanwhile stands beside it — its state words say `bringing`)
        self.with_workspace(id, |ed| ed.workspace.bring_go = true);
        let stands_in = self.bring_queue_stands_in();
        let root = m.root.clone();
        self.events.shared_jobs().spawn(move || {
            let head = git(&root, &["rev-parse", "HEAD"], None).ok().map(|h| h.trim().to_string());
            let unclean = unclean(&root).unwrap_or_default();
            move |ed: &mut Editor| {
                // (changes in no commit: the preflight would test them, the bring land its commits without them —
                // stopped as a rebase is, its held card up, #371)
                if !unclean.is_empty() {
                    ed.with_workspace(id, |ed| {
                        (ed.workspace.bring_go, ed.workspace.bring_rebased) = (false, false)
                    });
                    return ed.push_offer(
                        Held { moved: m, changed: unclean, unsaved: Vec::new(), bring: true }.card(),
                    );
                }
                ed.with_workspace(id, |ed| {
                    (ed.workspace.bring_after_preflight, ed.workspace.bring_rebased) = (true, rebased);
                    // (the bring its card's `y` asked for goes on as the preflight's)
                    (ed.workspace.bring_go, ed.workspace.bring_unchecked) = (false, false);
                    ed.workspace.bring_card_after = Some(Box::new(m));
                    ed.workspace.bring_tested = head;
                    ed.bring_preflight_start(stands_in);
                });
            }
        });
    }

    /// The quick preflight a bring in front waits for (`bring_after_preflight`), started — or, its project's steps
    /// held back by a trust card still asking (#323), waiting on that card (`bring_trust`, the card up in the stack);
    /// held back by a no, the bring stopped and said. `stands_in`: a simulation stands in for the preflight's task.
    fn bring_preflight_start(&mut self, stands_in: bool) {
        if self.config_project_held("preflight.") {
            let asking = self.project_trust.as_ref().is_some_and(|p| p.state == crate::trust::State::Ask);
            if asking {
                self.workspace.bring_trust = true;
                if !self.project_trust_up() {
                    let _ = self.project_trust_ask();
                }
                return;
            }
            (self.workspace.bring_after_preflight, self.workspace.bring_trust) = (false, false);
            self.workspace.bring_card_after = None;
            let name = self.workspace.label();
            return self.set_error(format!(
                "{name} not brought — its project's preflight steps are held back (:project-trust to look again)"
            ));
        }
        self.workspace.bring_trust = false;
        if stands_in {
            return;
        }
        if let Err(e) = self.preflight_start(crate::preflight::Set::Quick) {
            self.workspace.bring_after_preflight = false;
            self.set_error(e);
        }
    }

    /// The project's trust answered or read again: every bring waiting on it (#323) goes on to its preflight, waits
    /// still, or stops, said — as `bring_preflight_start` decides now.
    pub(crate) fn bring_trust_sync(&mut self) {
        let waiting: Vec<crate::event::WorkspaceId> = std::iter::once(&self.workspace)
            .chain(&self.workspaces)
            .filter(|l| l.bring_trust && l.bring_after_preflight)
            .map(|l| l.id)
            .collect();
        let stands_in = self.bring_queue_stands_in();
        for id in waiting {
            self.with_workspace(id, |ed| ed.bring_preflight_start(stands_in));
        }
    }

    /// The preflight (`title`: which set) ended, `passed` or not: a bring waiting for it goes on, or doesn't —
    /// one its card's `p` asked for gets the card back, saying what failed.
    pub(crate) fn rebase_preflight_done(&mut self, passed: bool, title: &str) {
        // (taken whatever happens: one left from a preflight that didn't start isn't the next one's)
        let card = self.workspace.bring_card_after.take();
        // (what it tested is what lands, never HEAD as the bring runs — #373)
        let mut tested = None;
        if passed {
            self.workspace.bring_failed = None;
            tested = self.workspace.bring_tested.take();
        }
        if !std::mem::take(&mut self.workspace.bring_after_preflight) {
            return;
        }
        let (id, name) = (self.workspace.id, self.workspace.label().to_string());
        // (the bring queue's one under way: said there, the next going on if it failed)
        if self.bring_queue_at(id) == Some(crate::bringqueue::Stage::Preflight) {
            // (its card back saying what failed, as a single bring's — then out, the next going on)
            if let Some(m) = card.filter(|_| !passed) {
                self.rebase_bring_failed(*m, title);
            }
            self.bring_queue_preflight(passed, title);
            if !passed {
                return;
            }
        } else if !passed {
            if let Some(m) = card {
                return self.rebase_bring_failed(*m, title);
            }
            // (the preflight's own toast said it, as the bring's end)
            let said = format!("{name} not brought — the {title} didn't pass");
            return self.recorded(crate::editor::ToastKind::Warning, said);
        }
        // (said as landing until it has: the preflight's end and the bring's start one instant — #321)
        // (a rebase there gets its card instead, nothing landing)
        self.workspace.bring_landing = self.bring_queue_at(id).is_none() && self.workspace.rebase.is_none();
        if let Err(e) = self.workspace_bring_tested(&id.to_string(), tested) {
            self.workspace.bring_landing = false;
            // (the queue's one under way: out, the next going on — else it waited at its bring for good)
            match self.bring_queue_at(id) {
                Some(_) => self.bring_queue_out(id, e),
                None => self.set_error(e),
            }
        }
    }
}

/// A rebase stopped on a conflict — its Claude asked to resolve it (`asked`: Claude's to do, not yours) or nothing
/// asked yet (it waits on you) — for a session's line in tests (`term::tests`).
#[cfg(test)]
pub(crate) fn stopped_for_test(step: (usize, usize), asked: bool) -> Rebase {
    Rebase {
        branch: "dev".into(),
        base: "main".into(),
        stage: Stage::Stopped,
        step: Some(step),
        stop: Stop::default(),
        asked,
        waiting: None,
        also: Besides::default(),
        stashed: Vec::new(),
        resolved_at: None,
        messages: Default::default(),
        rechecks: (0, 0),
        index: None,
    }
}

/// A rebase stopped at conflicts, its Claude asked `question` as `rebase_ask` asks — for tests elsewhere of
/// what reaches Claude with it (`chat::tests`).
#[cfg(test)]
pub(crate) fn ask_for_test(ed: &mut Editor, question: &str) {
    ed.workspace.rebase = Some(Rebase {
        branch: "dev".into(),
        base: "main".into(),
        stage: Stage::Stopped,
        step: None,
        stop: Stop::default(),
        asked: false,
        waiting: None,
        also: Besides::default(),
        stashed: Vec::new(),
        resolved_at: None,
        messages: Default::default(),
        rechecks: (0, 0),
        index: None,
    });
    let a = Ask { question: question.into(), about: "<conflicts/>".into(), chip: "rebase".into() };
    ed.rebase_ask(a);
}

/// The front session's rebase stopped at a conflict its Claude resolved, the replayed commit's message
/// `message` (and Claude's version of it, `proposed`) — for the simulation of what `m` does (`sim/rebasemsg.rs`).
#[cfg(test)]
pub(crate) fn resolved_for_test(ed: &mut Editor, message: &str, proposed: Option<&str>) {
    let stop = Stop {
        step: Some((1, 2)),
        files: vec!["a.txt".into()],
        commit: Some(("0".repeat(40), "0000000".into(), message.lines().next().unwrap_or_default().into())),
        message: Some(message.into()),
        ..Stop::default()
    };
    ed.workspace.rebase = Some(Rebase {
        branch: "dev".into(),
        base: "main".into(),
        stage: Stage::Resolved,
        step: stop.step,
        stop,
        asked: true,
        waiting: None,
        also: Besides::default(),
        stashed: Vec::new(),
        resolved_at: Some(std::time::Instant::now()),
        messages: Messages { proposed: proposed.map(String::from), ..Messages::default() },
        rechecks: (0, 0),
        index: None,
    });
}

/// The front session's rebase resolved at its stop `n`, git having named `files` conflicted there — what else the
/// checkout changed read as the end of Claude's turn reads it (`besides`), the cards of the stop before gone and no
/// card put up (`rebase_offer` puts it) — for the simulation of the resolved card read again
/// (`sim/rebaserecheck.rs`).
#[cfg(test)]
pub(crate) fn resolved_at_for_test(ed: &mut Editor, n: usize, files: &[&str]) {
    let stop = Stop {
        step: Some((n, n + 1)),
        files: files.iter().map(|f| f.to_string()).collect(),
        commit: Some((format!("{n:040}"), format!("{n:07}"), format!("stop {n}"))),
        ..Stop::default()
    };
    ed.rebase_cards_gone();
    let also = besides(&ed.workspace.root, &stop.files, None);
    if ed.workspace.rebase.is_none() {
        resolved_for_test(ed, "stop", None);
    }
    let r = ed.workspace.rebase.as_mut().expect("a rebase");
    (r.stage, r.step, r.stop, r.also) = (Stage::Resolved, stop.step, stop, also);
    r.messages = Messages::default();
}

/// The front session's rebase's index watch, seen from outside: gone once its thread ended (#245's test).
#[cfg(test)]
pub(crate) fn index_watch_for_test(ed: &Editor) -> Option<std::sync::Weak<std::sync::atomic::AtomicBool>> {
    ed.workspace.rebase.as_ref()?.index.as_ref().map(|w| std::sync::Arc::downgrade(&w.stop))
}

/// The front session's rebase going on — git at work, as `y` leaves it once its line starts (its cards gone) — for
/// `sim/rebaserecheck.rs`, which runs no process.
#[cfg(test)]
pub(crate) fn going_on_for_test(ed: &mut Editor) {
    ed.rebase_cards_gone();
    if let Some(r) = ed.workspace.rebase.as_mut() {
        r.stage = Stage::Running { generation: 0, aborting: false };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review of frictions31: the message written with `m` loses only tarae's own lines — a `#` heading or `#123`
    /// of the message's own stays, whatever git's comment character (`commit.cleanup=whitespace` on the run).
    #[test]
    fn the_message_written_keeps_its_own_hash_lines() {
        let text = format!(
            "Fix the gate\n\n# Why\n#123 asked for it\n; kept too\n\n{MESSAGE_CUT}\nhelp\n:w goes on\n"
        );
        assert_eq!(message_written(&text), "Fix the gate\n\n# Why\n#123 asked for it\n; kept too\n");
        assert_eq!(message_written("No cut line\n\n"), "No cut line\n");
    }

    /// frictions29 93: a preflight that failed more than one test names them all on the bring card, as many as
    /// fit, the rest `+N more` — not the first alone, which read as the only failure.
    #[test]
    fn the_bring_card_after_a_failed_preflight_names_every_failing_test() {
        let m = Moved {
            root: PathBuf::from("/stand-in/feat"),
            name: "feat".into(),
            branch: "feat".into(),
            base: "main".into(),
            moved: 0,
            ahead: 1,
        };
        let mut out = vec!["running 40 tests".to_string()];
        out.extend((0..30).map(|i| format!("test tests::flaky_{i:02} ... FAILED")));
        out.extend(["failures:".into(), "---- tests::flaky_00 stdout ----".into()]);
        out.extend(["thread 'tests::flaky_00' panicked at src/a.rs:1:1:".into(), "boom".into()]);
        let o = m.failed_card(
            "quick preflight",
            &Default::default(),
            Some(("quick preflight · test".into(), out)),
        );
        let last = o.question.lines().last().unwrap();
        assert!(
            o.question.contains(
                "The quick preflight failed: tests::flaky_00\n    panicked at src/a.rs:1:1: boom\n"
            )
        );
        assert!(last.starts_with("    30 failed in all — also tests::flaky_01, tests::flaky_02, "), "{last}");
        assert!(last.ends_with(" more") && !last.contains("flaky_29"), "{last}");
        assert_eq!(o.warnings, vec![2..5], "the whole risk in the warning colour");
        // One failure: as before
        let one = vec!["test a ... FAILED".to_string()];
        let o = m.failed_card("quick preflight", &Default::default(), Some(("t".into(), one)));
        assert!(o.question.ends_with("The quick preflight failed: a"), "{}", o.question);
    }

    #[test]
    fn a_stop_says_where_it_is() {
        let mut s = Stop { step: Some((2, 5)), ..Default::default() };
        assert_eq!(s.at(), "commit 2 of 5");
        s.commit = Some(("f".repeat(40), "fffffff".into(), "Add login".into()));
        assert_eq!(s.at(), "commit 2 of 5 (fffffff Add login)");
        s.step = None;
        assert_eq!(s.at(), "fffffff Add login");
        let files: Vec<String> = (1..=6).map(|i| format!("f{i}")).collect();
        assert_eq!(named(&files[..2]), "f1, f2");
        assert_eq!(named(&files), "f1, f2, f3, f4 and 2 more");
        let r = Rebase {
            branch: "b".into(),
            base: "main".into(),
            stage: Stage::Stopped,
            step: Some((2, 5)),
            stop: Stop::default(),
            asked: false,
            waiting: None,
            also: Besides::default(),
            stashed: Vec::new(),
            resolved_at: None,
            messages: Default::default(),
            rechecks: (0, 0),
            index: None,
        };
        assert_eq!(r.said(), "rebasing 2/5");
        assert_eq!(r.why("feat"), "feat is rebasing 2/5 — :rebase-continue or :rebase-abort first");
        assert!(cut("é".repeat(10), 5).starts_with("éé\n… (16 more bytes"));
    }

    /// #175: `:rebase-continue` and `:rebase-abort` go to the session rebasing, not the one in front — the
    /// front one when it is, the only one, or the user's pick of several; a session named goes first.
    #[test]
    fn by_hand_it_goes_to_the_session_rebasing() {
        let scratch = crate::scratch::Scratch::new("rebase-which");
        let dir = scratch.to_path_buf();
        for p in ["front", "a", "b"] {
            std::fs::create_dir_all(dir.join(p)).unwrap();
        }
        let dir = std::fs::canonicalize(dir).unwrap();
        let mut ed = Editor::new(crate::config::Config::default());
        (ed.workspace.root, ed.workspace.name) = (dir.join("front"), "front".into());
        let a = ed.workspace_open(&dir.join("a")).unwrap();
        let b = ed.workspace_open(&dir.join("b")).unwrap();
        assert!(ed.workspace_switch(crate::event::WorkspaceId::HOME).is_in());
        // (git at work in each: the answer says so — nothing runs)
        let running = |base: &str| Rebase {
            branch: "x".into(),
            base: base.into(),
            stage: Stage::Running { generation: 0, aborting: false },
            step: Some((1, 3)),
            stop: Stop::default(),
            asked: false,
            waiting: None,
            also: Besides::default(),
            stashed: Vec::new(),
            resolved_at: None,
            messages: Default::default(),
            rechecks: (0, 0),
            index: None,
        };
        ed.with_workspace(a, |ed| ed.workspace.rebase = Some(running("main")));
        let label = |ed: &mut Editor, id| {
            let mut n = String::new();
            ed.with_workspace(id, |ed| n = ed.workspace.label().to_string());
            n
        };
        let name_a = label(&mut ed, a);
        // One, behind: that one
        assert_eq!(ed.rebase_by_hand("", true), Err(format!("{name_a} is rebasing 1/3 — let it finish")));
        // Two, behind: asked which
        ed.with_workspace(b, |ed| ed.workspace.rebase = Some(running("dev")));
        assert_eq!(ed.rebase_by_hand("", false), Ok(()));
        let p = ed.picker.take().expect("a pick");
        assert_eq!(p.title, "rebase-abort: which session?");
        let rows: Vec<_> = p.items().iter().map(|i| (i.hint.as_str(), i.action.clone())).collect();
        assert_eq!(
            rows,
            [
                ("rebasing 1/3 onto main", crate::picker::Action::Typed(format!("rebase-abort {a}"))),
                ("rebasing 1/3 onto dev", crate::picker::Action::Typed(format!("rebase-abort {b}"))),
            ]
        );
        // Named: that one
        let name_b = label(&mut ed, b);
        assert_eq!(
            ed.rebase_by_hand(&b.to_string(), true),
            Err(format!("{name_b} is rebasing 1/3 — let it finish"))
        );
        // The front one rebasing too: the front's
        ed.workspace.rebase = Some(running("main"));
        assert_eq!(ed.rebase_by_hand("", true), Err("front is rebasing 1/3 — let it finish".to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }
}
