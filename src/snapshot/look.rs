//! #610: a folder a run makes that git takes for a repository (`gitrepo::git_would_take`) — git run from there or
//! below, the user's own, uses it instead of the project's repository and runs what its config and hooks name. Found
//! after the run with no walk and no git, from what the snapshot lists already (`Seen`): the folder of a new or
//! changed `HEAD` or `commondir` (any case: APFS finds `HEAD` as `head`), the folder above a new `objects/` or `refs/`,
//! the same in a new ignored folder's walk and below tools' output (`output_code`), the top of each ignored folder
//! (judged itself, against what it held before the run), and the remembered folders holding a `HEAD`
//! (`Workspace.heads`, `HEADS_MAX`: what each held at its last look kept beside it — 3 `lstat`s after a run, git's test
//! only where one changed). A noted one (`y` on its card: `Workspace.noted`) is named again when a run changes
//! what decides it — its `HEAD`, `commondir`, `config` or `hooks/` — by prefix over the same lists, no more looked at.
//! Named as a `Made` with its `Look` on the protected-made card (`tools::ask_made`).

use std::path::Path;

use super::Made;

/// Folders holding a `HEAD` remembered per workspace at most (`Workspace.heads`): past it the rest aren't looked at
/// after runs — counted (`Found.heads_past`), said to Claude.
pub const HEADS_MAX: usize = 256;

/// A `hooks/` folder's entries `look_hash` reads at most — past it, `big:` (never kept).
const HOOKS_MAX: usize = 64;

/// What a folder git takes for a repository is, beside its name (`Made.look`).
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub takes: crate::gitrepo::Takes,
    /// What a run changed in one noted before (`config`, `hooks/x` — from the folder); empty: this run made it one.
    pub changed: Vec<String>,
    /// What the run added that makes it one — or, in a noted one, a deciding file it added: what `r` removes. A file or
    /// a link as a `Made` (by its hash, as `remove_made` removes one), a folder (`/` at its end) only while empty
    /// (`rmdir`). Never the folder itself, never an edited file put back.
    pub added: Vec<Made>,
}

/// Which of what makes a folder one git takes it holds (an entry there, any kind) — 4 `lstat`s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Parts {
    head: bool,
    objects: bool,
    refs: bool,
    commondir: bool,
}

impl Parts {
    pub fn of(dir: &Path) -> Parts {
        Parts { head: at(dir, "HEAD"), ..Parts::store(dir, false) }
    }

    /// A remembered folder's look after a run: its `HEAD` as `head` says, the rest looked at — 3 `lstat`s.
    fn store(dir: &Path, head: bool) -> Parts {
        Parts { head, objects: at(dir, "objects"), refs: at(dir, "refs"), commondir: at(dir, "commondir") }
    }

    /// A folder holding a `HEAD` and nothing else known (`r` took what the run added): judged at the next run's look.
    pub fn head_only() -> Parts {
        Parts { head: true, ..Parts::default() }
    }
}

/// Whether `dir` holds an entry `name` (any kind, no link followed) — one `lstat`, counted in tests (`WORK`).
fn at(dir: &Path, name: &str) -> bool {
    #[cfg(test)]
    WORK.with(|w| w.set((w.get().0 + 1, w.get().1)));
    std::fs::symlink_metadata(dir.join(name)).is_ok()
}

#[cfg(test)]
thread_local! {
    /// The work a run's look took on this thread (#610's budget, tests only): `lstat`s of what makes a folder one, and
    /// git's tests (`gitrepo::git_would_take`).
    pub(super) static WORK: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

/// A name that makes its folder one: `HEAD` or `commondir`, in any case.
fn deciding(name: &str) -> bool {
    name.eq_ignore_ascii_case("HEAD") || name.eq_ignore_ascii_case("commondir")
}

/// A name whose folder is above a repository's: `objects` or `refs`, in any case.
fn store(name: &str) -> bool {
    name.eq_ignore_ascii_case("objects") || name.eq_ignore_ascii_case("refs")
}

/// One folder to judge (`Seen`), from the root.
#[derive(Default)]
struct Cand {
    /// New `HEAD` and `commondir` names in it, from the root.
    made: Vec<String>,
    /// New folders in it named `objects` or `refs` (a walk's), from the root, `/` at the end.
    folders: Vec<String>,
    /// What it held before the run, where that's known (an ignored folder's top, a remembered one).
    then: Option<Parts>,
    /// A `HEAD` name the run made or changed there, or remembered: kept (`Workspace.heads`) while not whole yet.
    head: bool,
    remembered: bool,
    /// Named by the run's lists (not only judged against what it held): git's test whatever it holds.
    listed: bool,
    /// Noted: what decides it the run changed (from the folder), and of those the ones it made (from the root).
    changed: Vec<String>,
    changed_made: Vec<String>,
}

/// What a run's lists say of folders git may take now (`own`).
pub(super) struct Seen {
    /// By folder, in the order found — sorted to judge; found by `index` (a `HashMap<String, usize>` tarae has compiled
    /// anyway: a `BTreeMap<String, Cand>` of its own cost the release ~3 KB, a linear find is quadratic in a run that
    /// names many folders).
    cands: Vec<(String, Cand)>,
    index: std::collections::HashMap<String, usize>,
    /// The noted folders in this root, from it, sorted.
    noted: Vec<String>,
}

impl Seen {
    pub fn new(noted: Vec<String>) -> Seen {
        let mut noted = noted;
        crate::sorting::sort(&mut noted);
        Seen { cands: Vec::new(), index: Default::default(), noted }
    }

    /// Folder `folder`'s entry, made if it's new.
    fn entry(&mut self, folder: &str) -> &mut Cand {
        let i = match self.index.get(folder) {
            Some(&i) => i,
            None => {
                self.index.insert(folder.to_string(), self.cands.len());
                self.cands.push((folder.to_string(), Cand::default()));
                self.cands.len() - 1
            }
        };
        &mut self.cands[i].1
    }

    fn at(&mut self, folder: &str) -> &mut Cand {
        let c = self.entry(folder);
        c.listed = true;
        c
    }

    /// A path the run made (`new`) or changed, from the root (a file, a link).
    pub fn path(&mut self, rel: &str, new: bool) {
        let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
        let Some((last, above)) = parts.split_last() else { return };
        // (a noted one: what decides it, under it — its HEAD, commondir, config, hooks/; objects/ and refs/ run nothing)
        for i in (1..parts.len()).rev() {
            let folder = parts[..i].join("/");
            if self.noted.binary_search(&folder).is_err() {
                continue;
            }
            let rest = &parts[i..];
            let decides = match rest {
                [one] => deciding(one) || one.eq_ignore_ascii_case("config"),
                [hooks, _, ..] => hooks.eq_ignore_ascii_case("hooks"),
                _ => false,
            };
            if decides {
                let c = self.at(&folder);
                c.changed.push(rest.join("/"));
                if new {
                    c.changed_made.push(rel.to_string());
                }
            }
            break;
        }
        if deciding(last) && !above.is_empty() {
            let c = self.at(&above.join("/"));
            c.head |= last.eq_ignore_ascii_case("HEAD");
            if new {
                c.made.push(rel.to_string());
            }
        }
        // (the folder above a new path's objects/ or refs/)
        if new {
            for i in 1..above.len() {
                if store(above[i]) {
                    self.at(&above[..i].join("/"));
                }
            }
        }
    }

    /// A folder the run made, from the root (a walk's): one named `objects` or `refs` is the folder above's.
    pub fn folder(&mut self, rel: &str) {
        if let Some((above, name)) = rel.trim_end_matches('/').rsplit_once('/')
            && store(name)
        {
            self.at(above).folders.push(format!("{}/", rel.trim_end_matches('/')));
        }
    }

    /// A folder judged itself, with what it held before the run: the top of an ignored folder (`remembered` false),
    /// a remembered one holding a `HEAD`.
    pub fn then(&mut self, rel: &str, parts: Parts, remembered: bool) {
        let c = self.entry(rel.trim_end_matches('/'));
        c.then = Some(parts);
        c.remembered |= remembered;
        c.head |= remembered;
    }

    /// The folders git takes now that the run made one or changed, as `Made`s (each its `look_hash`), and those
    /// holding a `HEAD` git doesn't take yet (remembered first), with what each holds: to look at again after the next
    /// run. One the lists don't name that holds what it held: no git test (the work budget, `WORK`).
    pub fn judge(mut self, root: &Path) -> (Vec<Made>, Vec<(String, Parts)>) {
        let (mut made, mut heads) = (Vec::new(), Vec::new());
        crate::sorting::by(&mut self.cands, |a, b| a.0.cmp(&b.0));
        for (rel, c) in self.cands {
            // (the project's root: its own .git)
            if rel.is_empty() {
                continue;
            }
            let dir = root.join(&rel);
            if let Some(then) = c.then.filter(|_| !c.listed) {
                let now = if c.remembered { Parts::store(&dir, then.head) } else { Parts::of(&dir) };
                if now == then {
                    if c.remembered {
                        heads.push((false, rel, then));
                    }
                    continue;
                }
            }
            #[cfg(test)]
            WORK.with(|w| w.set((w.get().0, w.get().1 + 1)));
            let Some(takes) = crate::gitrepo::git_would_take(&dir) else {
                let now = Parts::of(&dir);
                if c.head && now.head {
                    heads.push((!c.remembered, rel, now));
                }
                continue;
            };
            let noted = self.noted.binary_search(&rel).is_ok();
            let (changed, added) = match noted {
                // (noted, nothing that decides it changed: not asked again)
                true if c.changed.is_empty() => continue,
                true => (c.changed, c.changed_made),
                false => {
                    let head =
                        |n: &&String| n.rsplit('/').next().is_some_and(|n| n.eq_ignore_ascii_case("HEAD"));
                    let added = match (c.made.iter().find(head), c.made.first(), c.then) {
                        (Some(h), _, _) => vec![h.clone()],
                        (None, Some(n), _) => vec![n.clone()],
                        (None, None, Some(then)) => {
                            let now = Parts::of(&dir);
                            // (what it was before the run: not this run's)
                            if now == then {
                                continue;
                            }
                            let new =
                                |had: bool, has: bool, n: &str| (has && !had).then(|| format!("{rel}/{n}"));
                            match (
                                new(then.head, now.head, "HEAD"),
                                new(then.commondir, now.commondir, "commondir"),
                            ) {
                                (Some(h), _) | (None, Some(h)) => vec![h],
                                _ => [
                                    new(then.objects, now.objects, "objects/"),
                                    new(then.refs, now.refs, "refs/"),
                                ]
                                .into_iter()
                                .flatten()
                                .collect(),
                            }
                        }
                        (None, None, None) => c.folders,
                    };
                    (Vec::new(), added)
                }
            };
            let added = added
                .into_iter()
                .map(|path| {
                    let hash =
                        if path.ends_with('/') { String::new() } else { super::made_hash(root, &path) };
                    Made { path, hash, look: None }
                })
                .collect();
            let hash = look_hash(root, &rel);
            made.push(Made { path: rel, hash, look: Some(Look { takes, changed, added }) });
        }
        crate::sorting::sort(&mut heads);
        (made, heads.into_iter().map(|(_, rel, parts)| (rel, parts)).collect())
    }
}

// ── At first trust (#610, landing 2): what a checkout already holds ─────────────────────────────────────────────────

/// The trust scan's line: every path named `HEAD` or `commondir`, in any case (APFS finds `HEAD` as `head`) — git's own
/// pathspec magic, after the line's options.
const SCAN_SPECS: [&str; 3] = ["--", ":(glob,icase)**/HEAD", ":(glob,icase)**/commondir"];

/// How long the line with the untracked half may take (`scan`): past it git is stopped and the cut said (S10) — ~14× the
/// 0.14 s measured at 200,000 files.
pub const SCAN_LIMIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Folders the trust scan judges at most (~100 ms at the measured rate): past it the rest aren't, the cut said.
pub const SCAN_MAX: usize = 4096;

/// How often the bounded line is looked at while it runs.
const SCAN_POLL: std::time::Duration = std::time::Duration::from_millis(20);

/// What `v` shows of a found folder's `HEAD` and `config`, at most (bytes, lines).
const PEEK_MAX: (u64, usize) = (512, 6);

/// Hook names `v` shows of a found folder at most — of the `HOOKS_MAX` + 1 entries of its `hooks/` read at most.
const HOOKS_SHOWN: usize = 12;

/// What git does with a folder it takes for a repository, as both cards say it — the protected-made card's (S2) and the
/// first-trust card's (S3): one sentence from one place, `many` for them.
pub fn git_uses(many: bool) -> String {
    let (it, its) = if many { ("them", "their") } else { ("it", "its") };
    format!(
        "git run from there or below — your terminal, a prompt's git status — uses {it} instead of this project's \
         repository and runs what {its} config and hooks name. tarae's own git doesn't."
    )
}

/// What `v` shows of one of a found folder's parts: its lines (a file's, or its hooks' names), and whether there was
/// more than those — said on the card as cut.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Peeked {
    pub lines: Vec<String>,
    pub cut: bool,
}

/// A folder git takes for a repository the trust scan found: from the root, what makes it one, its `look_hash`, and its
/// `HEAD`, `config` (within `PEEK_MAX`) and hooks' names (`hooks`) as they read — a link as `→ its target` — what the
/// card's `v` shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Taken {
    pub rel: String,
    pub takes: crate::gitrepo::Takes,
    pub hash: String,
    pub head: Peeked,
    pub config: Peeked,
    pub hooks: Peeked,
}

impl Taken {
    /// Its look-hash covers all that decides it (none past what `look_hash` reads): a note of it can be kept.
    pub fn keepable(&self) -> bool {
        !self.hash.starts_with(super::BIG)
    }
}

/// What the trust scan found in a checkout (`scan`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scan {
    /// Folders git takes for a repository, sorted.
    pub found: Vec<Taken>,
    /// Folders holding a `HEAD` git doesn't take yet, whole, with what each holds — remembered (`Workspace.heads`).
    pub heads: Vec<(std::path::PathBuf, Parts)>,
    /// Every folder the line named (from the root, sorted, at most `SCAN_MAX`) — judged again with the cached half alone
    /// when the git tab's pull or switch reruns it, so the untracked ones found at first aren't lost.
    pub folders: Vec<String>,
    /// The untracked half took over `SCAN_LIMIT`: stopped, the cached half alone looked at.
    pub slow: bool,
    /// Folders past `SCAN_MAX`, not judged.
    pub past: usize,
}

impl Scan {
    /// What it didn't look through, its reasons alone, as S10 says them — None: all of it. Each says its own lead at
    /// its call: the card (`lookalike::Ask::card`) a short one under its title, the toast (`look_alikes_cut`) the whole.
    pub fn cut_why(&self) -> Option<String> {
        let mut why = Vec::new();
        if self.slow {
            why.push(format!("the untracked files took over {}", crate::term::run_duration(SCAN_LIMIT)));
        }
        if self.past > 0 {
            let s = if self.past == 1 { "" } else { "s" };
            why.push(format!("{} more folder{s} holding a HEAD", self.past));
        }
        (!why.is_empty()).then(|| why.join(" and "))
    }
}

/// The trust scan (#610, landing 2): the folders in the checkout at `root` git takes for a repository, found by one
/// `git ls-files` through the snapshot's builder — its repository's held settings until trust is answered
/// (`Repo::command`: a `core.fsmonitor` doesn't run), no hooks — then git's own test on each folder holding a `HEAD` or
/// `commondir` (`gitrepo::git_would_take`). `again`: None at the first look — the cached and the untracked halves in
/// one line, stopped past `SCAN_LIMIT` (then the cached half alone, unbounded: the index bounds it); the folders of the
/// look before when the git tab's pull or switch reruns it — the cached half alone, those judged with it. A folder
/// below `.git` or `.repo` (git's and `repo`'s own stores, refused to runs by name — #609) isn't judged; nor is the
/// root's own. On a worker.
pub fn scan(root: &Path, again: Option<&[String]>) -> Result<Scan, String> {
    use std::os::unix::ffi::OsStrExt as _;
    fn line(rest: &[&'static str]) -> Vec<&'static str> {
        ["ls-files", "-z"].iter().chain(rest).chain(&SCAN_SPECS).copied().collect()
    }
    let cached = || super::git_bytes(root, &line(&["--cached"]), None);
    let mut scan = Scan::default();
    let bytes = match again {
        Some(_) => cached()?,
        None => match within(root, &line(&["--cached", "--others", "--exclude-standard"]))? {
            Some(b) => b,
            None => {
                scan.slow = true;
                cached()?
            }
        },
    };
    let mut folders: Vec<Vec<u8>> = bytes
        .split(|&b| b == 0)
        .filter_map(|p| Some(p[..p.iter().rposition(|&b| b == b'/')?].to_vec()))
        .filter(|f| {
            !f.split(|&b| b == b'/')
                .any(|c| c.eq_ignore_ascii_case(b".git") || c.eq_ignore_ascii_case(b".repo"))
        })
        .collect();
    folders.extend(again.unwrap_or_default().iter().map(|f| f.as_bytes().to_vec()));
    crate::sorting::sort(&mut folders);
    folders.dedup();
    scan.past = folders.len().saturating_sub(SCAN_MAX);
    folders.truncate(SCAN_MAX);
    for f in folders {
        let dir = root.join(std::ffi::OsStr::from_bytes(&f));
        let rel = String::from_utf8_lossy(&f).into_owned();
        match crate::gitrepo::git_would_take(&dir) {
            Some(takes) => scan.found.push(Taken {
                hash: look_hash(root, &rel),
                head: peek(&dir.join("HEAD")),
                config: peek(&dir.join("config")),
                hooks: hooks(&dir.join("hooks")),
                rel: rel.clone(),
                takes,
            }),
            None => {
                let parts = Parts::of(&dir);
                if parts.head {
                    scan.heads.push((dir, parts));
                }
            }
        }
        scan.folders.push(rel);
    }
    Ok(scan)
}

/// The line `args` on `root`'s repository through the snapshot's builder, given `SCAN_LIMIT`: its stdout — None, stopped
/// past it. Its pipes read as they fill, without waiting (a full one can't hold git up), between waits of `SCAN_POLL` by
/// `event::sleep` on `clock`'s time (a simulation moves it) — no thread of its own.
fn within(root: &Path, args: &[&str]) -> Result<Option<Vec<u8>>, String> {
    let said = |e: std::io::Error| format!("git: {e}");
    let cmd = super::command_env(root, args, None, &[])?;
    #[cfg(test)]
    let cmd = stand_in(root).unwrap_or(cmd);
    let s = crate::spawn::setsid_git(&cmd).map_err(said)?;
    let (mut out, mut err) = (Drain::of(s.stdout), Drain::of(s.stderr));
    let start = crate::clock::now();
    let mut stopped = false;
    loop {
        err.take();
        if out.take() {
            break;
        }
        if crate::clock::since(start) >= SCAN_LIMIT {
            crate::spawn::term_group(s.pid);
            stopped = true;
            break;
        }
        crate::event::sleep(SCAN_POLL);
    }
    let status = crate::spawn::wait(s.pid).map_err(said)?;
    err.take();
    let (stdout, stderr) = (out.got, err.got);
    match () {
        _ if stopped => Ok(None),
        _ if status.success() => Ok(Some(stdout)),
        _ => {
            let e = String::from_utf8_lossy(&stderr);
            Err(e.lines().find(|l| !l.trim().is_empty()).unwrap_or("git failed").to_string())
        }
    }
}

/// A child's pipe read without waiting (`within`): what came so far, and whether it ended.
struct Drain {
    pipe: Option<std::io::PipeReader>,
    got: Vec<u8>,
}

impl Drain {
    fn of(pipe: Option<std::io::PipeReader>) -> Drain {
        use std::os::fd::AsRawFd as _;
        if let Some(p) = &pipe {
            // SAFETY: fcntl on a descriptor this pipe owns, live for the calls.
            unsafe {
                let flags = libc::fcntl(p.as_raw_fd(), libc::F_GETFL);
                libc::fcntl(p.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }
        Drain { pipe, got: Vec::new() }
    }

    /// What's there read — whether the pipe has ended (or there's none).
    fn take(&mut self) -> bool {
        use std::io::Read as _;
        let Some(p) = self.pipe.as_mut() else { return true };
        let mut buf = [0u8; 16 * 1024];
        loop {
            match p.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => self.got.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return false,
                Err(_) => break,
            }
        }
        self.pipe = None;
        true
    }
}

/// Tests: checkouts whose untracked half is a stand-in git that never ends (`sleep`, harmless) — the cut's.
#[cfg(test)]
pub(crate) static SLOW: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
fn stand_in(root: &Path) -> Option<std::process::Command> {
    let slow = SLOW.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|s| root.starts_with(s));
    slow.then(|| {
        let mut c = std::process::Command::new("sleep");
        c.arg("120");
        c
    })
}

/// A file's first lines as `v` shows them (`PEEK_MAX`), blank ones left out — a link's target as `→ …`, read through no
/// link and with no wait (a FIFO); none: not there.
fn peek(path: &Path) -> Peeked {
    use std::io::Read as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    if let Ok(t) = std::fs::read_link(path) {
        return Peeked { lines: vec![format!("→ {}", t.display())], cut: false };
    }
    let Ok(f) =
        std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW).open(path)
    else {
        return Peeked::default();
    };
    let mut b = Vec::new();
    let _ = f.take(PEEK_MAX.0).read_to_end(&mut b);
    let text = String::from_utf8_lossy(&b);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let cut = lines.len() > PEEK_MAX.1 || b.len() as u64 == PEEK_MAX.0;
    Peeked { lines: lines.iter().take(PEEK_MAX.1).map(|l| l.to_string()).collect(), cut }
}

/// A found folder's hooks as `v` shows them: the names in `hooks/` that aren't git's own `*.sample`, sorted, at most
/// `HOOKS_SHOWN` — of at most `HOOKS_MAX` + 1 entries read (`look_hash`'s bound); past either, cut. Only names: no hook
/// is opened, a folder that's a link is named by its target as `→ …` and not read, nor is anything that isn't one.
fn hooks(path: &Path) -> Peeked {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_symlink() => {
            let to = std::fs::read_link(path).map(|t| t.display().to_string()).unwrap_or_default();
            Peeked { lines: vec![format!("→ {to}")], cut: false }
        }
        Ok(m) if m.is_dir() => {
            let read: Vec<String> = std::fs::read_dir(path)
                .map(|r| {
                    r.flatten()
                        .take(HOOKS_MAX + 1)
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            let mut names: Vec<String> = read.iter().filter(|n| !n.ends_with(".sample")).cloned().collect();
            crate::sorting::sort(&mut names);
            let cut = read.len() > HOOKS_MAX || names.len() > HOOKS_SHOWN;
            names.truncate(HOOKS_SHOWN);
            Peeked { lines: names, cut }
        }
        _ => Peeked::default(),
    }
}

/// What decides folder `rel` (from `root`) and what it runs, as a digest (`Made.hash` of a look-alike): its `HEAD`,
/// `commondir` and `config` (each within `HASH_MAX`, read through no link) and `hooks/`'s names and contents (within
/// `HOOKS_MAX` entries) — past either, `big:`, never kept (`Made::keepable`).
pub(super) fn look_hash(root: &Path, rel: &str) -> String {
    let mut parts: Vec<String> = ["HEAD", "commondir", "config"]
        .iter()
        .map(|n| super::made_hash(root, &format!("{rel}/{n}")))
        .collect();
    let hooks = format!("{rel}/hooks");
    match std::fs::symlink_metadata(root.join(&hooks)) {
        Ok(m) if m.is_dir() => {
            let names: Vec<String> = std::fs::read_dir(root.join(&hooks))
                .map(|r| {
                    r.flatten()
                        .take(HOOKS_MAX + 1)
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            if names.len() > HOOKS_MAX {
                parts.push(super::BIG.to_string());
            }
            let mut names = names;
            crate::sorting::sort(&mut names);
            for n in names.iter().take(HOOKS_MAX) {
                parts.push(format!("{n}\0{}", super::made_hash(root, &format!("{hooks}/{n}"))));
            }
        }
        _ => parts.push(super::made_hash(root, &hooks)),
    }
    let joined = parts.join("\0");
    match parts.iter().any(|p| p.starts_with(super::BIG)) {
        true => format!("{}{}", super::BIG, crate::sha256::hex(joined.as_bytes())),
        false => crate::sha256::hex(joined.as_bytes()),
    }
}
