//! #609: the repository's real git folder, kept from runs as `.git` is. The sandbox keeps `.git` by name; the folder a
//! checkout's `.git` file names (`gitdir: gitdata`, `git init --separate-git-dir`), its common folder, and where the
//! links among a `.git` folder's entries lead (Android's `repo`: `.git/hooks -> ../.repo/projects/…/hooks`) have
//! other names — what runs later outside the sandbox (a hook, `core.fsmonitor` in a config) all the same. Read when
//! the policy is made (`found`, on its worker), kept by one rule (`git_kept`) for the policy and the hooks card alike.
//!
//! A kept path isn't a kept name: its parents can be swapped. So each target is resolved step by step from the text
//! that names it (`resolve`), never `canonicalize`, and every path the resolution visits where runs write is its
//! way: on macOS each is locked against unlink and create (`Policy::profile`'s `GD<n>`, up to `GIT_LOCKS_MAX`), on
//! Linux the keeper's grants leave a missing step, a link met and a folder off the target's own way unopened
//! (`GitKept.marks`). `llm.sandbox-protect`'s entries get the same ways.
//!
//! A checkout below the root with its own git folder (L2: `sub/.git` naming `sub/gitdata`, every checkout of a `repo`
//! top) is found before each run, by the walk each system makes then (macOS's `keyfolders::walk`, Linux's
//! `linux::there`), and kept by the same rule (`nested_kept`): one reading's paths are looked at once each (`Memo`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

/// The most of a `.git` folder's top-level entries read for its links: past it none is followed, and that's said
/// (`GitKept.cut`). A `.git` folder holds a dozen.
pub const ENTRIES_MAX: usize = 256;

/// The most of those links followed, the first by name: the same folder always gives the same list, so the policy's
/// key doesn't move between two reads.
pub const LINKS_MAX: usize = 64;

/// The most links followed resolving one path — the kernel's own (Linux's `MAXSYMLINKS`; macOS stops at 32): past it
/// git can't use the path either.
pub const HOPS_MAX: usize = 40;

/// macOS: the most paths on the ways to kept folders the profile locks — counted before the keys' lock in
/// `keyfolders::LOCKS_MAX` and `LOCKS_BYTES`, and capped on their own, so a key-heavy project can't push them out.
/// Past it a target is still kept (its writes refused), its way isn't locked, and the rules say so.
pub const GIT_LOCKS_MAX: usize = 32;

/// What a path on the way to a target was when it was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Folder,
    Link,
    /// Not there yet: a run could make it.
    Missing,
}

/// A folder to keep (`found`): where it is, resolved step by step from the text naming it, and every path the
/// resolution visited, the target aside.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub target: PathBuf,
    pub way: Vec<(PathBuf, Step)>,
    /// Whether the target was there when it was read.
    pub there: bool,
    /// Git's (a `.git` led to it), else an `llm.sandbox-protect` entry.
    pub git: bool,
}

/// What the reader found for the places (`found`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Read {
    pub found: Vec<Found>,
    /// `.git` folders past `ENTRIES_MAX`: no link in them followed.
    pub cut: Vec<PathBuf>,
}

/// `text`'s components as the resolver walks them: `..` kept, `.` and the root's own left out.
fn parts(text: &Path) -> VecDeque<OsString> {
    text.components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_os_string()),
            Component::ParentDir => Some("..".into()),
            _ => None,
        })
        .collect()
}

/// What the resolver saw at a path (`Memo::look`).
#[derive(Clone, Debug)]
enum Looked {
    Missing,
    /// A link, with its own text.
    Link(PathBuf),
    Folder,
    /// A file, or anything else that isn't a folder or a link.
    Other,
    /// It couldn't be looked at (nor a link's text read).
    Failed,
}

/// What one reading looked at, each path once: every `lstat` the resolver makes (and a link's `readlink`) goes
/// through it. A `repo` top's links share their ways' first steps — at 300 checkouts 21,430 steps over ~7,000 paths
/// (L2's measure) — so a run's reading costs one `lstat` a path, not one a step.
#[derive(Debug, Default)]
pub struct Memo {
    /// (by bytes: `Path`'s hash writes each component apart, several times the cost)
    looked: HashMap<OsString, Looked>,
    /// The `lstat`s made (the work budget's count).
    pub lstats: usize,
}

impl Memo {
    fn look(&mut self, p: &Path) -> Looked {
        if let Some(l) = self.looked.get(p.as_os_str()) {
            return l.clone();
        }
        self.lstats += 1;
        let l = match p.symlink_metadata() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Looked::Missing,
            Err(_) => Looked::Failed,
            Ok(m) if m.file_type().is_symlink() => std::fs::read_link(p).map_or(Looked::Failed, Looked::Link),
            Ok(m) if m.is_dir() => Looked::Folder,
            Ok(_) => Looked::Other,
        };
        self.looked.insert(p.as_os_str().to_os_string(), l.clone());
        l
    }
}

/// `text` (a `gitdir:` line, a `commondir`, a link's own text) resolved from the folder `base` (real) as the kernel
/// resolves it: component by component, each link met followed from its own text (at most `HOPS_MAX`), `..` from
/// where the walk is. With every path visited on the way, the target aside: each folder, each link met, a step not
/// there yet (and the steps after it, as written). None: a loop, a file on the way, a step that can't be looked at.
#[cfg(test)]
pub fn resolve(base: &Path, text: &Path) -> Option<(PathBuf, Vec<(PathBuf, Step)>)> {
    resolve_in(base, text, &mut Memo::default())
}

/// `resolve`, looking through `memo`.
fn resolve_in(base: &Path, text: &Path, memo: &mut Memo) -> Option<(PathBuf, Vec<(PathBuf, Step)>)> {
    let mut cur = if text.is_absolute() { PathBuf::from("/") } else { base.to_path_buf() };
    let mut todo = parts(text);
    let mut way = Vec::new();
    let (mut hops, mut gone) = (0, false);
    while let Some(c) = todo.pop_front() {
        if c == ".." {
            cur.pop();
            continue;
        }
        let next = cur.join(&c);
        let last = todo.is_empty();
        if gone {
            if !last {
                way.push((next.clone(), Step::Missing));
            }
            cur = next;
            continue;
        }
        match memo.look(&next) {
            Looked::Missing => {
                gone = true;
                if !last {
                    way.push((next.clone(), Step::Missing));
                }
                cur = next;
            }
            Looked::Failed => return None,
            Looked::Link(to) => {
                hops += 1;
                if hops > HOPS_MAX {
                    return None;
                }
                way.push((next.clone(), Step::Link));
                if to.is_absolute() {
                    cur = PathBuf::from("/");
                }
                let mut more = parts(&to);
                more.extend(todo.drain(..));
                todo = more;
            }
            Looked::Folder => {
                if !last {
                    way.push((next.clone(), Step::Folder));
                }
                cur = next;
            }
            Looked::Other if last => cur = next,
            Looked::Other => return None,
        }
    }
    // (a path visited twice — `l/../y` through `x/y` — once)
    let mut once: Vec<(PathBuf, Step)> = Vec::with_capacity(way.len());
    for s in way {
        if !once.iter().any(|(p, _)| *p == s.0) {
            once.push(s);
        }
    }
    Some((cur, once))
}

/// `resolve`'s answer as a `Found`.
fn found_at(base: &Path, text: &Path, git: bool, memo: &mut Memo) -> Option<Found> {
    let (target, way) = resolve_in(base, text, memo)?;
    let there = matches!(memo.look(&target), Looked::Link(_) | Looked::Folder | Looked::Other);
    Some(Found { target, way, there, git })
}

/// The git folders the checkout at `at` (real) leads to: where `.git` is a file, the folder it names (read through
/// `gitrepo::git_file`: never waited on) and that folder's common folder (`commondir`); where it's a link, the folder
/// it leads to; and for a `.git` folder (a link's too), where each link among its top-level entries leads — read in
/// name order, at most `ENTRIES_MAX` (past it none, said) and the first `LINKS_MAX` links followed. A plain `.git`
/// folder itself isn't among them: the profile keeps it by name. On a worker.
pub fn git_folders(at: &Path) -> Read {
    git_folders_in(at, &mut Memo::default())
}

/// `git_folders`, looking through `memo` — one memo over a run's whole reading (L2).
pub fn git_folders_in(at: &Path, memo: &mut Memo) -> Read {
    let mut out = Read::default();
    let dot = at.join(".git");
    let dir = match memo.look(&dot) {
        Looked::Missing | Looked::Failed => return out,
        Looked::Other => {
            let named = crate::gitrepo::git_file(&dot);
            let named =
                named.as_deref().and_then(|t| t.lines().next()?.strip_prefix("gitdir:").map(str::trim));
            let Some(f) =
                named.filter(|n| !n.is_empty()).and_then(|n| found_at(at, Path::new(n), true, memo))
            else {
                return out;
            };
            let common = f.there.then(|| crate::gitrepo::git_file(&f.target.join("commondir"))).flatten();
            let common = common.as_deref().map(str::trim).filter(|c| !c.is_empty());
            let common = common.and_then(|c| found_at(&f.target, Path::new(c), true, memo));
            out.found.push(f);
            out.found.extend(common);
            return out;
        }
        Looked::Link(_) => match found_at(at, Path::new(".git"), true, memo) {
            Some(f) if f.there => {
                let dir = f.target.clone();
                out.found.push(f);
                dir
            }
            _ => return out,
        },
        Looked::Folder => dot,
    };
    // (a `.git` folder by name keeps each entry in it by the name: the entry, the link a target is named by, isn't on
    // its way — a `repo` top's 3,000 of them, each judged for nothing. Not a `.git` link's folder: it may be a place
    // itself, `.git -> .`, where the entry is a run's to swap)
    let by_name = dir == at.join(".git");
    let Ok(entries) = std::fs::read_dir(&dir) else { return out };
    let Some(listed) = crate::sorting::listing(entries, ENTRIES_MAX) else {
        out.cut.push(dir);
        return out;
    };
    let links = listed.into_iter().filter(|(_, e)| e.file_type().is_ok_and(|t| t.is_symlink()));
    for (name, _) in links.take(LINKS_MAX) {
        let Some(mut f) = found_at(&dir, Path::new(&name), true, memo) else { continue };
        if by_name && f.way.first().is_some_and(|(s, _)| s.parent() == Some(dir.as_path())) {
            f.way.remove(0);
        }
        out.found.push(f);
    }
    out
}

/// L2: `git_folders` of each nested checkout a run's walk found (`checkouts`, in its order), through one memo — a
/// `repo` top's checkouts share their ways' first steps. Each way starts with the checkout's own folders that hold
/// its target (`sub` for `sub/.git` naming `gitdata`): moved, the target moves with them — `mv sub subx`, a hook
/// written in `subx/gitdata`, moved back — so they're on its way (the place itself and above are left out by
/// `git_kept`'s rule).
pub fn nested_read(checkouts: &[PathBuf], memo: &mut Memo) -> Read {
    let mut out = Read::default();
    for c in checkouts {
        let r = git_folders_in(c, memo);
        for mut f in r.found {
            let mut up: Vec<(PathBuf, Step)> = c
                .ancestors()
                .filter(|a| f.target.starts_with(a) && !f.way.iter().any(|(s, _)| s == a))
                .map(|a| (a.to_path_buf(), Step::Folder))
                .collect();
            up.reverse();
            up.append(&mut f.way);
            f.way = up;
            out.found.push(f);
        }
        out.cut.extend(r.cut);
    }
    out
}

/// What the policy for the project at `root` (real) keeps of git's folders and `llm.sandbox-protect` (`protect`, as
/// set): `git_folders` of the root and of each other place runs write that a checkout may be (`places`: the
/// workspace's folders, `llm.sandbox-write`'s), then each protect entry inside the project resolved from the root.
/// On the policy's worker (`env_with`).
pub fn found(root: &Path, places: &[PathBuf], protect: &[String]) -> Read {
    let mut out = Read::default();
    let mut memo = Memo::default();
    let mut seen: Vec<&Path> = Vec::new();
    for at in std::iter::once(root).chain(places.iter().map(PathBuf::as_path)) {
        if seen.contains(&at) {
            continue;
        }
        seen.push(at);
        let r = git_folders_in(at, &mut memo);
        out.found.extend(r.found);
        out.cut.extend(r.cut);
    }
    for p in protect {
        let rel = Path::new(p);
        if rel.is_relative() && rel.components().all(|c| matches!(c, Component::Normal(_))) {
            out.found.extend(found_at(root, rel, false, &mut memo));
        }
    }
    out
}

/// What a policy keeps of what the reader found (`git_kept`) — the one rule the policy and the hooks card read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitKept {
    /// Git's folders kept, real paths: each inside the project or a folder opened for writing — and where a place is
    /// git's folder itself (`gitdir: .`), its `hooks` and `config` instead. What the report's words read.
    pub git: Vec<PathBuf>,
    /// Everything kept, git's and `llm.sandbox-protect`'s resolved: `Policy.protect` takes these.
    pub protect: Vec<PathBuf>,
    /// macOS: each kept target whose way is locked, with the paths on it the profile locks (none: nothing between
    /// it and its place to swap).
    pub ways: Vec<(PathBuf, Vec<PathBuf>)>,
    /// macOS: kept targets whose way is past `GIT_LOCKS_MAX` — still kept, their way not locked (said).
    pub unlocked: Vec<PathBuf>,
    /// The places that are git's folder themselves (`gitdir: .`): only their `hooks` and `config` kept.
    pub bare: Vec<PathBuf>,
    /// Linux: on the ways, what no grant may open — a step not there, a link met, a folder off its target's own way:
    /// each a hole, so the folder holding it takes no new entry and nothing there is swapped (a kept path not there
    /// is one too, found when the run starts: `linux::confine_in`).
    pub marks: Vec<PathBuf>,
    /// `.git` folders past `ENTRIES_MAX`: their links not followed (said).
    pub cut: Vec<PathBuf>,
    /// macOS, nested checkouts' (`nested_kept`): git's folders found past the shared cap — not kept (said, a count).
    pub dropped: Vec<PathBuf>,
}

/// The most git folders the rules name, and the refusal's words: past it, how many more.
pub const NAMED: usize = 3;

/// Whether `p` is one of `sorted` (real paths sorted by their bytes — `sorted`) or below one: each of its ancestors
/// looked up by binary search, O(depth · log n) where a scan of the list is O(n) a path — and a rule over every path
/// quadratic: L1's `git_kept` took 9.8 s at a `repo` top of 300 checkouts (`perf_a_repo_tops_git_folders` keeps it
/// fast). By bytes, a memcmp, not `Path`'s order, which parses components at every step: a real path's ancestors are
/// prefixes of its bytes. Not generic, so one copy serves every site.
#[inline(never)]
pub fn under_any(p: &Path, sorted: &[PathBuf]) -> bool {
    // (an ancestor sorting before the first is none of them, nor is any shorter one: a prefix sorts first)
    let Some(first) = sorted.first() else { return false };
    p.ancestors()
        .take_while(|a| a.as_os_str() >= first.as_os_str())
        .any(|a| sorted.binary_search_by(|s| s.as_os_str().cmp(a.as_os_str())).is_ok())
}

/// `v` sorted by bytes, for `under_any`.
pub fn sorted(v: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = v.into_iter().collect();
    crate::sorting::by(&mut v, |a, b| a.as_os_str().cmp(b.as_os_str()));
    v
}

/// Whether the real path `p` is `dir` or below it, by bytes (`Path::starts_with` parses both at every call).
fn holds(dir: &Path, p: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
    let (d, p) = (dir.as_os_str().as_bytes(), p.as_os_str().as_bytes());
    p.starts_with(d) && (p.len() == d.len() || p[d.len()] == b'/' || d.ends_with(b"/"))
}

/// What the run's rules say of git's folders kept by other names — the policy's and the nested checkouts' found
/// before the run (`kept`), after the protected names: `, nor git's folder gitdata` (`folders …`, at most `NAMED`,
/// then how many more); a place that is git's folder itself (`, nor git's hooks and config`); macOS's past the cap —
/// a way not locked, a nested checkout's folder not kept — one clause each, a count where there's more than one,
/// never a list; and the `.git` folders whose links weren't followed. Nothing where there's none of those.
pub fn said(kept: &[&GitKept], root: &Path, supervised: bool) -> String {
    let shown = |p: &PathBuf| crate::allow::from_project(root, p);
    let (mut folders, mut bare, mut unlocked, mut dropped, mut cut) =
        (Vec::new(), false, Vec::new(), 0, Vec::new());
    for k in kept {
        let in_bare = |g: &&PathBuf| k.bare.iter().any(|b| g.parent() == Some(b.as_path()));
        folders.extend(k.git.iter().filter(|g| !in_bare(g)));
        bare |= k.git.iter().any(|g| in_bare(&g));
        if !supervised {
            unlocked.extend(&k.unlocked);
            dropped += k.dropped.len();
        }
        cut.extend(&k.cut);
    }
    let mut s = match folders.as_slice() {
        [] => String::new(),
        [one] => format!(", nor git's folder {}", shown(one)),
        all => format!(", nor git's folders {}", named(all, shown)),
    };
    if bare {
        s.push_str(", nor git's hooks and config");
    }
    match unlocked.as_slice() {
        [] => {}
        [one] => s.push_str(&format!(" ({} not locked: past the cap)", shown(one))),
        all => s.push_str(&format!(" ({} git folders' ways aren't locked: past the cap)", all.len())),
    }
    match dropped {
        0 => {}
        1 => s.push_str(" (1 nested git folder isn't kept: past the cap)"),
        n => s.push_str(&format!(" ({n} nested git folders aren't kept: past the cap)")),
    }
    match cut.as_slice() {
        [] => {}
        [one] => s.push_str(&format!(
            " ({} holds more than {ENTRIES_MAX} entries: the links in it aren't followed)",
            shown(one)
        )),
        all => s.push_str(&format!(
            " ({} .git folders hold more than {ENTRIES_MAX} entries: the links in them aren't followed)",
            all.len()
        )),
    }
    s
}

/// `all` named as the words name git's folders: at most `NAMED`, then how many more (`a, b, c and 41 more`; two:
/// `a, b`).
pub fn named(all: &[&PathBuf], shown: impl Fn(&PathBuf) -> String) -> String {
    let first: Vec<String> = all.iter().take(NAMED).map(|p| shown(p)).collect();
    match all.len().checked_sub(NAMED) {
        Some(more) if more > 0 => format!("{} and {more} more", first.join(", ")),
        _ => first.join(", "),
    }
}

impl GitKept {
    /// Every path the profile locks, once each, sorted.
    pub fn locks(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.ways.iter().flat_map(|(_, w)| w.iter().cloned()).collect();
        crate::sorting::sort(&mut v);
        v.dedup();
        v
    }

    /// Git's folders whose way is kept from a swap here (`linux`: every one; macOS: those within the lock's cap) —
    /// what the hooks card leaves to the sandbox.
    pub fn held(&self, linux: bool) -> Vec<PathBuf> {
        let unlocked = sorted(self.unlocked.iter().cloned());
        self.git.iter().filter(|g| linux || unlocked.binary_search(g).is_err()).cloned().collect()
    }

    /// The kept target `path` is in or on the way to, and whether it's git's — None: neither. (A scan: once a
    /// refused path, never over every path.)
    pub fn holding(&self, path: &Path) -> Option<(&Path, bool)> {
        if let Some(g) = self.git.iter().find(|g| path.starts_with(g)) {
            return Some((g, true));
        }
        if let Some((t, _)) = self.ways.iter().find(|(_, w)| w.iter().any(|s| s == path)) {
            return Some((t, self.git.contains(t)));
        }
        self.protect.iter().find(|p| path.starts_with(p)).map(|p| (p.as_path(), false))
    }
}

/// Linux: the folders a run's grants leave without a rule of their own for `marks` (`GitKept.marks`, and kept paths not
/// there) — each from a mark's folder up to the outermost of `places` (the project, the places runs write) holding it,
/// that one too: they take no new entry, as a folder holding a protected name doesn't, and the report says so
/// (`linux::Confine::marked`). Sorted, once each. Words only: the grants are `grant`'s. (Linux's; tested everywhere)
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn shut_by_marks(marks: &[PathBuf], places: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for m in marks {
        let holding = places.iter().filter(|p| m.starts_with(p) && *p != m);
        let Some(top) = holding.min_by_key(|p| p.components().count()) else { continue };
        out.extend(m.ancestors().skip(1).take_while(|a| a.starts_with(top)).map(Path::to_path_buf));
    }
    crate::sorting::sort(&mut out);
    out.dedup();
    out
}

/// What a policy keeps of `read`: `kept_in` the places a target is kept in (the project, the folders opened for
/// writing), `writes` every place runs write (a way's path elsewhere can't be swapped by a run), `cap` the lock's
/// (macOS's `GIT_LOCKS_MAX`; tests lower it).
/// - A target in one of `kept_in` is kept, its way's paths where runs write locked (not those in a kept target or
///   under a protected name: the write rules hold them), whole ways in order while they fit `cap`.
/// - A target that is one of `kept_in` or holds one (`gitdir: .`) isn't kept whole — every write there would be
///   refused: its `hooks` and `config` are (srt's set), the rest of that folder written.
/// - One elsewhere isn't: no run writes there, nor one a protected name holds where it's kept: the name keeps it
///   (`.git`'s own `modules/`, where a submodule's `gitdir:` leads).
pub fn git_kept(read: &Read, kept_in: &[PathBuf], writes: &[PathBuf], cap: usize) -> GitKept {
    kept_by(read, kept_in, writes, &[], cap, false)
}

/// L2: what a run keeps of the nested checkouts' git folders found before it (`read`), by `git_kept`'s rule beside
/// the policy's own: what `held` keeps already (the policy's `protect`, sorted) isn't kept again. macOS (`cap` Some,
/// what's left of `GIT_LOCKS_MAX` after the policy's locks): the targets and the paths on their ways share it, the
/// targets first — one past it isn't kept (`dropped`), one whose way doesn't fit is kept, its way not locked
/// (`unlocked`). Linux (None): every one, each way's marks.
pub fn nested_kept(
    read: &Read,
    kept_in: &[PathBuf],
    writes: &[PathBuf],
    held: &[PathBuf],
    cap: Option<usize>,
) -> GitKept {
    match cap {
        Some(cap) => kept_by(read, kept_in, writes, held, cap, true),
        None => kept_by(read, kept_in, writes, held, usize::MAX, false),
    }
}

/// `git_kept` and `nested_kept`'s rule: `shared`, the targets count in `cap` first.
fn kept_by(
    read: &Read,
    kept_in: &[PathBuf],
    writes: &[PathBuf],
    held: &[PathBuf],
    cap: usize,
    shared: bool,
) -> GitKept {
    let mut out = GitKept::default();
    let (kept_in_sorted, writes_sorted) = (sorted(kept_in.iter().cloned()), sorted(writes.iter().cloned()));
    let place_of = |p: &Path| kept_in.iter().filter(|w| holds(w, p)).max_by_key(|w| w.as_os_str().len());
    // (a protected name holding it where it's kept: the write rules keep it whole)
    let named = |p: &Path| place_of(p).is_some_and(|w| super::protected_in(w, p));
    // (sets borrowing the reading's paths by their bytes: one hash a lookup — `Path`'s writes each component apart —
    // nothing copied; a `repo` top's 3,000)
    let (mut seen, mut dropped): (HashSet<&OsStr>, HashSet<&OsStr>) = (HashSet::new(), HashSet::new());
    let mut gits: HashSet<PathBuf> = HashSet::new();
    let mut kept: Vec<&Found> = Vec::new();
    for f in &read.found {
        let t = f.target.as_path();
        if f.git && kept_in.iter().any(|w| holds(t, w)) {
            if !out.bare.contains(&f.target) {
                out.bare.push(f.target.clone());
            }
            for n in ["hooks", "config"] {
                let p = t.join(n);
                if under_any(&p, &kept_in_sorted) && gits.insert(p.clone()) {
                    out.git.push(p.clone());
                    out.protect.push(p.clone());
                    out.ways.push((p, Vec::new()));
                }
            }
            continue;
        }
        let Some(w) = place_of(t) else { continue };
        if seen.contains(t.as_os_str()) || gits.contains(t) || super::protected_in(w, t) || under_any(t, held)
        {
            continue;
        }
        if shared && out.protect.len() >= cap {
            if dropped.insert(t.as_os_str()) {
                out.dropped.push(t.to_path_buf());
            }
            continue;
        }
        seen.insert(t.as_os_str());
        kept.push(f);
        out.protect.push(t.to_path_buf());
        if f.git {
            out.git.push(t.to_path_buf());
        }
    }
    // (shared: what's left of the cap after the targets)
    let cap = if shared { cap.saturating_sub(out.protect.len()) } else { cap };
    let protect = sorted(out.protect.iter().cloned());
    let (mut locked, mut marks): (HashSet<&OsStr>, HashSet<&OsStr>) = (HashSet::new(), HashSet::new());
    // (each path on a way judged once, by its bytes: whether a run can swap it — where it writes, not the place
    // itself, nor what the write rules already hold: a kept target, the policy's, a protected name)
    let mut judged: HashMap<&OsStr, bool> = HashMap::with_capacity(kept.len());
    // (the last way's steps and answers: a checkout's targets share their first steps — compared, not hashed)
    let mut last: Vec<(&OsStr, bool)> = Vec::new();
    // (Linux's reading, uncapped: its profile takes no ways — macOS's `GD` literals — only the marks)
    let locks = cap != usize::MAX;
    let mut full = false;
    for f in kept {
        let mut way: Vec<&(PathBuf, Step)> = Vec::new();
        for (i, s) in f.way.iter().enumerate() {
            let p = s.0.as_path();
            let o = p.as_os_str();
            let can = match last.get(i) {
                Some(&(l, can)) if l == o => can,
                _ => match judged.get(o) {
                    Some(&can) => can,
                    None => {
                        let place = writes.iter().any(|w| w.as_os_str() == o);
                        let held = under_any(p, &protect) || under_any(p, held) || named(p);
                        let can = !place && under_any(p, &writes_sorted) && !held;
                        judged.insert(o, can);
                        can
                    }
                },
            };
            last.truncate(i);
            last.push((o, can));
            if can {
                way.push(s);
            }
        }
        for (s, step) in &way {
            let off = !holds(s, &f.target);
            if (*step != Step::Folder || off) && marks.insert(s.as_os_str()) {
                out.marks.push(s.clone());
            }
        }
        if !locks {
            continue;
        }
        let new: Vec<&OsStr> =
            way.iter().map(|(s, _)| s.as_os_str()).filter(|s| !locked.contains(s)).collect();
        if !way.is_empty() && (full || locked.len() + new.len() > cap) {
            full = true;
            out.unlocked.push(f.target.clone());
            continue;
        }
        locked.extend(new);
        out.ways.push((f.target.clone(), way.iter().map(|(s, _)| s.clone()).collect()));
    }
    out.cut = read.cut.iter().filter(|c| under_any(c, &kept_in_sorted)).cloned().collect();
    out
}

/// Tests' switch for the name `.repo` (#609 L2, Q1 — the user's "제안대로 해줘", 2026-11-16): whether it's a protected
/// name (the tools pack's `protect`), so a `repo` top's git folders are held by the name (true), or kept by the walk
/// within its cap and the rest said (false). The tests that depend on it assert it agrees with `protected_in`.
#[cfg(test)]
pub const REPO_NAMED: bool = true;

/// Tests': a `repo` top of `n` checkouts at `t` (real), L2's measured layout — under 12 top folders, two or three
/// deep, each `<c>/.git/` a folder of `HEAD`, `index` and 10 links into `.repo/projects/<c>.git/`, whose `objects`
/// links on into `.repo/project-objects/<c>.git/objects`; and `.repo/repo/` with a plain `.git` of its own (`repo`'s
/// Python: `main.py`). Links, empty files and folders only. The checkouts, in the order made.
#[cfg(test)]
pub fn repo_top(t: &Path, n: usize) -> Vec<PathBuf> {
    repo_top_in(t, n, ".repo")
}

/// `repo_top` with its store named `store` — one no name holds (`.store`) for the walk's cap and its words.
#[cfg(test)]
pub fn repo_top_in(t: &Path, n: usize, store: &str) -> Vec<PathBuf> {
    use std::os::unix::fs::symlink;
    const ENTRIES: [&str; 10] = [
        "config",
        "description",
        "hooks",
        "info",
        "logs",
        "objects",
        "packed-refs",
        "refs",
        "rr-cache",
        "svn",
    ];
    let mut checkouts = Vec::new();
    for i in 0..n {
        let rel = match i % 2 {
            0 => format!("d{}/p{i}", i % 12),
            _ => format!("d{}/g{i}/p{i}", i % 12),
        };
        let up = "../".repeat(Path::new(&rel).components().count() + 1);
        let c = t.join(&rel);
        let dot = c.join(".git");
        std::fs::create_dir_all(&dot).unwrap();
        std::fs::write(dot.join("HEAD"), "").unwrap();
        std::fs::write(dot.join("index"), "").unwrap();
        let project = t.join(format!("{store}/projects/{rel}.git"));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(t.join(format!("{store}/project-objects/{rel}.git/objects"))).unwrap();
        for e in ENTRIES {
            symlink(format!("{up}{store}/projects/{rel}.git/{e}"), dot.join(e)).unwrap();
            match e {
                "objects" => {
                    symlink(format!("{up}project-objects/{rel}.git/objects"), project.join(e)).unwrap()
                }
                "config" | "description" | "packed-refs" => std::fs::write(project.join(e), "").unwrap(),
                _ => std::fs::create_dir(project.join(e)).unwrap(),
            }
        }
        checkouts.push(c);
    }
    std::fs::create_dir_all(t.join(store).join("repo/.git")).unwrap();
    std::fs::write(t.join(store).join("repo/.git/HEAD"), "").unwrap();
    std::fs::write(t.join(store).join("repo/main.py"), "").unwrap();
    checkouts
}

#[cfg(test)]
mod tests;
