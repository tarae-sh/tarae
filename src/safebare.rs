//! #610, landing 3: the offer to make the person's own git use a bare repository only when named — `git config --global
//! safe.bareRepository explicit`, the one thing that closes the time between a run making a folder git takes for a
//! repository and the person reading about it (a prompt's `git status` there runs what its config names first).
//!
//! - **git's setting** is read once since tarae started, on a worker, as the git watch's first round lands
//!   (`read_once`): `git version` (2.38 brought the setting; an older git is never offered) and `git config
//!   --show-scope --get-all safe.bareRepository` with tarae's own `-c` — the last value of the `system` and `global`
//!   lines only (`judge`): the `command` scope (tarae's `-c`, a `GIT_CONFIG_COUNT`) and `local` never count. It's read
//!   again at once after a write and by the command, so the look-alike cards drop their offer line (`cards_sync`).
//! - **The card** (S4, kind `safe-bare`) comes by itself at the end of the first turn in which a command ran through
//!   tarae's tools (`turn_over`) — wherever it ran — once since tarae started, and not when a look-alike card has
//!   carried the offer's line (`with_line`); never once the store holds an answer. `y` writes it through git itself
//!   (`write`), `n` never asks again, `esc` leaves it unanswered: the next start asks again.
//! - **The store**, `$state/safe-bare`: a line `yes <date>` or `no <date>`, appended (as `never-install`). Any line:
//!   the card never comes by itself; only `:git-safe-bare` asks.
//! - **Never the person's config in a test**: `GitHome::Yours` is no test build's; tests and the simulation give a
//!   scratch folder (`GitHome::Scratch`), and a test build's editor has none (nothing read, nothing offered).

use std::path::{Path, PathBuf};

use crate::editor::Editor;
use crate::gitrepo::GitHome;
use crate::offer::{Offer, What};

pub const TITLE: &str = "Let git use a bare repository only when named?";

/// What `y` runs, as the card shows it.
pub const COMMAND: &str = "git config --global safe.bareRepository explicit";

/// The first git with the setting (major, minor).
const SINCE: (u32, u32) = (2, 38);

/// The status line after `n` (S7).
pub const NEVER_SAID: &str = "Not asked again — :git-safe-bare asks";

/// What tarae read of the person's git.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Setting {
    /// Not read since tarae started.
    Unread,
    /// A bare repository only when named, set in `system` or `global` (the scope's word).
    Explicit(String),
    /// Not so in either: offered.
    Unset,
    /// A git before 2.38, its version as it said it: the setting means nothing to it — never offered.
    Old(String),
    /// git didn't say (its words): never offered.
    Failed(String),
}

/// The offer's state since tarae started.
pub struct SafeBare {
    /// Whose git config is read and written — None in a test build's editor (a test or the simulation gives a scratch one).
    pub home: Option<GitHome>,
    /// `$state/safe-bare` — None in tests unless given.
    pub store: Option<PathBuf>,
    pub setting: Setting,
    /// The store holds an answer: the card never comes by itself.
    pub answered: bool,
    /// The offer was in sight since tarae started — the card, or a look-alike card's line: the card doesn't come by
    /// itself again until the next start.
    pub offered: bool,
    /// A read under way (`read_once`).
    reading: bool,
}

impl SafeBare {
    pub fn new() -> SafeBare {
        #[cfg(not(test))]
        let (home, store) = (Some(GitHome::Yours), crate::config::state_dir().map(|d| d.join("safe-bare")));
        #[cfg(test)]
        let (home, store) = (None, None);
        SafeBare { home, store, setting: Setting::Unread, answered: false, offered: false, reading: false }
    }
}

/// `git version`'s version: `git version 2.39.5 (Apple Git-154)` → `2.39.5`, with its major and minor.
fn version_of(said: &str) -> Option<(String, (u32, u32))> {
    let v = said.split_whitespace().nth(2)?;
    let mut n = v.split('.').map(|p| p.parse::<u32>().ok());
    Some((v.to_string(), (n.next()??, n.next()??)))
}

/// The setting from what git said: its version, then its `--show-scope --get-all` lines (`scope\tvalue`) — the last
/// `system` or `global` value decides; `command` and `local` lines never count. Exactly `explicit`, as git compares it:
/// git 2.54 stops at `Explicit` ("fatal: bad config variable 'safe.barerepository'") — not explicit, the offer's write
/// replaces it.
pub fn judge(version: &str, scoped: &str) -> Setting {
    let Some((v, at)) = version_of(version) else {
        return Setting::Failed(format!("git version said {:?}", version.trim()));
    };
    if at < SINCE {
        return Setting::Old(v);
    }
    let last = scoped
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .rfind(|(scope, _)| matches!(*scope, "system" | "global"));
    match last {
        Some((scope, "explicit")) => Setting::Explicit(scope.to_string()),
        _ => Setting::Unset,
    }
}

/// git's first line of words, else what failed.
fn words(err: &[u8]) -> String {
    let s = String::from_utf8_lossy(err);
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("git failed").to_string()
}

/// The setting as git says it now. Blocks: a worker's.
pub fn setting(home: &GitHome) -> Setting {
    let version = match crate::gitrepo::global(home, &["version"]) {
        Ok(r) if r.ok => String::from_utf8_lossy(&r.stdout).into_owned(),
        Ok(r) => return Setting::Failed(words(&r.stderr)),
        Err(e) => return Setting::Failed(e),
    };
    // (exit 1: no value in any scope — unset)
    match crate::gitrepo::global(home, &["config", "--show-scope", "--get-all", "safe.bareRepository"]) {
        Ok(r) => judge(&version, &String::from_utf8_lossy(&r.stdout)),
        Err(e) => Setting::Failed(e),
    }
}

/// Whether the store holds an answer (any line).
fn answered(store: Option<&Path>) -> bool {
    store
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|s| s.lines().any(|l| !l.trim().is_empty()))
}

/// An answer appended to the store: `yes <date>` or `no <date>` — appending, so two tareas answering both keep theirs.
fn keep(store: &Path, word: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = store.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let (y, m, d) = crate::log::day(crate::clock::system());
    let ends_open = std::fs::read(store).is_ok_and(|b| b.last().is_some_and(|&c| c != b'\n'));
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(store)?;
    writeln!(f, "{}{word} {y:04}-{m:02}-{d:02}", if ends_open { "\n" } else { "" })
}

/// `y`'s write, by git itself (its writer and lock; an `all` replaced in place), then where git says it went
/// (`--show-origin`), shown with home as `~`. Blocks: a worker's.
pub fn write(home: &GitHome) -> Result<String, String> {
    write_by(home, &crate::gitrepo::global)
}

/// How `write` runs git: `gitrepo::global`, or a test's.
type Git<'a> = &'a dyn Fn(&GitHome, &[&str]) -> Result<crate::gitrepo::Ran, String>;

/// `write`, each git run by `git` (a test's fails one). Once the write went through, where it went is said whatever the
/// second run gives — none: "your global git config", never "nothing written".
fn write_by(home: &GitHome, git: Git) -> Result<String, String> {
    let r = git(home, &["config", "--global", "safe.bareRepository", "explicit"])?;
    if !r.ok {
        return Err(words(&r.stderr));
    }
    let origin = git(home, &["config", "--global", "--show-origin", "--get", "safe.bareRepository"]);
    let out = origin.map(|r| String::from_utf8_lossy(&r.stdout).into_owned()).unwrap_or_default();
    let file = out.split('\t').next().and_then(|o| o.strip_prefix("file:")).map(PathBuf::from);
    let home_dir = match home {
        #[cfg(not(test))]
        GitHome::Yours => crate::allow::home(),
        #[cfg(test)]
        GitHome::Scratch(dir) => Some(dir.clone()),
    };
    Ok(match file {
        Some(f) => crate::crash::tilde(&f, home_dir.as_deref()),
        None => "your global git config".to_string(),
    })
}

/// The look-alike cards' last line while git's setting isn't explicit (S2, S3): one folder or several.
pub fn line(many: bool) -> String {
    format!("Run :git-safe-bare to make git refuse {}.", if many { "them" } else { "it" })
}

/// `q`, a look-alike card's text, with the offer's line at its end while git's setting is known not to be explicit —
/// the offer in sight then: the card doesn't come by itself.
pub fn with_line(ed: &mut Editor, q: &mut String, many: bool) {
    if ed.safe_bare.setting == Setting::Unset {
        q.push_str("\n\n");
        q.push_str(&line(many));
        ed.safe_bare.offered = true;
    }
}

/// How many folders git takes for a repository a card names — 0: not a look-alike card.
fn looks_on(what: &What) -> usize {
    match what {
        What::LookAlike(a) => a.scan.found.len(),
        What::ProtectedMade { made, .. } => made.iter().filter(|m| m.look.is_some()).count(),
        _ => 0,
    }
}

/// Every look-alike card up or parked, its offer line as git's setting is now: dropped once explicit, added once
/// known not to be. Whether one changed.
fn cards_sync(ed: &mut Editor) -> bool {
    let want = ed.safe_bare.setting == Setting::Unset;
    let (mut changed, mut shown, focus) = (false, false, ed.card_focus);
    let mut refocus = None;
    let parked = ed.workspaces.iter_mut().flat_map(|w| w.parked.iter_mut());
    for o in ed.offers.iter_mut().chain(ed.workspace.parked.iter_mut()).chain(parked) {
        let n = looks_on(&o.what);
        if n == 0 || o.installing() {
            continue;
        }
        let tail = format!("\n\n{}", line(n > 1));
        match (o.question.ends_with(&tail), want) {
            (false, true) => o.question.push_str(&tail),
            (true, false) => o.question.truncate(o.question.len() - tail.len()),
            (has, _) => {
                shown |= has;
                continue;
            }
        }
        shown |= want;
        o.rev += 1;
        changed = true;
        if focus.is_some_and(|f| f.0 == o.id) {
            refocus = Some((o.id, o.rev));
        }
    }
    if refocus.is_some() {
        ed.card_focus = refocus;
    }
    ed.safe_bare.offered |= shown;
    changed
}

/// The card (S4) — `typed`: the command asked for it.
pub fn card(typed: bool) -> Offer {
    let q = format!(
        "git takes any folder holding HEAD, objects/ and refs/ for a repository and runs what its config names — one a \
         run or a cloned project made too.\n\n{COMMAND}\n\nThen a bare repository is used only when named (--git-dir \
         or GIT_DIR); your repositories, worktrees and submodules work as now.\n\nWrite it: into your global \
         git config. Never: not asked again — :git-safe-bare asks."
    );
    Offer::new(TITLE.into(), &q, "", TITLE.into(), What::SafeBare { typed }).deciding(0..3)
}

impl Editor {
    /// What a read said, applied: the setting, whether the store holds an answer, the look-alike cards' line.
    fn safe_bare_read(&mut self, setting: Setting, answered: bool) {
        self.safe_bare.reading = false;
        (self.safe_bare.setting, self.safe_bare.answered) = (setting, answered);
        if !cards_sync(self) {
            self.unchanged = true;
        }
    }
}

/// git's setting read on a worker, once since tarae started (the git watch's first round: `git::watch_round`).
pub fn read_once(ed: &mut Editor) {
    let s = &mut ed.safe_bare;
    if s.setting != Setting::Unread || s.reading {
        return;
    }
    let Some(home) = s.home.clone() else { return };
    s.reading = true;
    let store = s.store.clone();
    ed.events.jobs().spawn(move || {
        let (setting, answered) = (setting(&home), answered(store.as_deref()));
        move |ed: &mut Editor| ed.safe_bare_read(setting, answered)
    });
}

/// A turn ended: a command ran in it through tarae's tools (`Session.ran_in_turn`) — the card, if git's setting is
/// known not to be explicit, the store holds no answer and the offer wasn't in sight since tarae started.
pub fn turn_over(ed: &mut Editor) {
    if !std::mem::take(&mut ed.workspace.claude.ran_in_turn) {
        return;
    }
    let s = &ed.safe_bare;
    if s.offered || s.answered || s.setting != Setting::Unset {
        return;
    }
    ed.safe_bare.offered = true;
    ed.push_offer(card(false));
}

/// `:git-safe-bare`: git's setting read again on a worker, then what it is said — explicit (S8), a git too old (S12) —
/// else the card, whatever the store holds.
pub fn command(ed: &mut Editor) {
    let Some(home) = ed.safe_bare.home.clone() else {
        return ed.set_status("git's global config isn't read here");
    };
    let store = ed.safe_bare.store.clone();
    ed.events.jobs().spawn(move || {
        let (setting, answered) = (setting(&home), answered(store.as_deref()));
        move |ed: &mut Editor| {
            ed.safe_bare_read(setting.clone(), answered);
            told(ed, setting);
        }
    });
}

/// What the command says of the setting it read: explicit (S8, its scope), a git too old (S12, the version read), git's
/// words — else the card.
fn told(ed: &mut Editor, setting: Setting) {
    ed.unchanged = false;
    match setting {
        Setting::Explicit(scope) => {
            ed.set_status(format!("Already set — safe.bareRepository = explicit, {scope}"))
        }
        Setting::Old(v) => {
            ed.set_warning(format!("git {v} ignores safe.bareRepository — 2.38 and later have it"))
        }
        Setting::Failed(e) => ed.set_error(format!("git's safe.bareRepository not read — {e}")),
        Setting::Unset | Setting::Unread => {
            ed.safe_bare.offered = true;
            ed.push_offer(card(true));
        }
    }
}

/// `y`: written by git on a worker — then `yes <date>` kept and the setting read again (the look-alike cards drop
/// their line); on failure git's words, and nothing kept.
pub(crate) fn answer(ed: &mut Editor, id: u64) {
    ed.offer_finished(id, Ok(()));
    let Some(home) = ed.safe_bare.home.clone() else { return };
    let store = ed.safe_bare.store.clone();
    ed.events.jobs().spawn(move || {
        let wrote = write(&home);
        let kept = match (&wrote, &store) {
            (Ok(_), Some(s)) => keep(s, "yes").err().map(|e| format!("{}: {e}", s.display())),
            _ => None,
        };
        let (setting, answered) = (setting(&home), answered(store.as_deref()));
        move |ed: &mut Editor| {
            ed.safe_bare_read(setting, answered);
            ed.unchanged = false;
            if let Some(e) = kept {
                crate::log::line("git", "warn", &format!("safe-bare's answer not kept: {e}"));
            }
            match wrote {
                Ok(file) => ed.set_success(format!("git: safe.bareRepository = explicit, in {file}")),
                Err(e) => ed.set_error(format!("git config --global failed — {e}; nothing written")),
            }
        }
    });
}

/// `n` (never): `no <date>` kept, not asked again — only the command asks.
pub(crate) fn never(ed: &mut Editor) {
    ed.safe_bare.answered = true;
    ed.set_status(NEVER_SAID);
    let Some(store) = ed.safe_bare.store.clone() else { return };
    ed.events.jobs().background(move || {
        if let Err(e) = keep(&store, "no") {
            crate::log::line(
                "git",
                "warn",
                &format!("{}: safe-bare's answer not kept: {e}", store.display()),
            );
        }
    });
}

#[cfg(test)]
mod tests;
