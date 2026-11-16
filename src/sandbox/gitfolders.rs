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

use std::collections::VecDeque;
use std::ffi::OsString;
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

/// `text` (a `gitdir:` line, a `commondir`, a link's own text) resolved from the folder `base` (real) as the kernel
/// resolves it: component by component, each link met followed from its own text (at most `HOPS_MAX`), `..` from
/// where the walk is. With every path visited on the way, the target aside: each folder, each link met, a step not
/// there yet (and the steps after it, as written). None: a loop, a file on the way, a step that can't be looked at.
pub fn resolve(base: &Path, text: &Path) -> Option<(PathBuf, Vec<(PathBuf, Step)>)> {
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
        match next.symlink_metadata() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                gone = true;
                if !last {
                    way.push((next.clone(), Step::Missing));
                }
                cur = next;
            }
            Err(_) => return None,
            Ok(m) if m.file_type().is_symlink() => {
                hops += 1;
                if hops > HOPS_MAX {
                    return None;
                }
                way.push((next.clone(), Step::Link));
                let to = std::fs::read_link(&next).ok()?;
                if to.is_absolute() {
                    cur = PathBuf::from("/");
                }
                let mut more = parts(&to);
                more.extend(todo.drain(..));
                todo = more;
            }
            Ok(m) if m.is_dir() => {
                if !last {
                    way.push((next.clone(), Step::Folder));
                }
                cur = next;
            }
            Ok(_) if last => cur = next,
            Ok(_) => return None,
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
fn found_at(base: &Path, text: &Path, git: bool) -> Option<Found> {
    let (target, way) = resolve(base, text)?;
    let there = target.symlink_metadata().is_ok();
    Some(Found { target, way, there, git })
}

/// The git folders the checkout at `at` (real) leads to: where `.git` is a file, the folder it names (read through
/// `gitrepo::git_file`: never waited on) and that folder's common folder (`commondir`); where it's a link, the folder
/// it leads to; and for a `.git` folder (a link's too), where each link among its top-level entries leads — read in
/// name order, at most `ENTRIES_MAX` (past it none, said) and the first `LINKS_MAX` links followed. A plain `.git`
/// folder itself isn't among them: the profile keeps it by name. On a worker.
pub fn git_folders(at: &Path) -> Read {
    let mut out = Read::default();
    let dot = at.join(".git");
    let Ok(m) = dot.symlink_metadata() else { return out };
    if m.is_file() {
        let named = crate::gitrepo::git_file(&dot);
        let named = named.as_deref().and_then(|t| t.lines().next()?.strip_prefix("gitdir:").map(str::trim));
        let Some(f) = named.filter(|n| !n.is_empty()).and_then(|n| found_at(at, Path::new(n), true)) else {
            return out;
        };
        let common = f.there.then(|| crate::gitrepo::git_file(&f.target.join("commondir"))).flatten();
        let common = common.as_deref().map(str::trim).filter(|c| !c.is_empty());
        let common = common.and_then(|c| found_at(&f.target, Path::new(c), true));
        out.found.push(f);
        out.found.extend(common);
        return out;
    }
    let dir = match m.file_type().is_symlink() {
        true => match found_at(at, Path::new(".git"), true) {
            Some(f) if f.there => {
                let dir = f.target.clone();
                out.found.push(f);
                dir
            }
            _ => return out,
        },
        false => dot,
    };
    let Ok(entries) = std::fs::read_dir(&dir) else { return out };
    let Some(listed) = crate::sorting::listing(entries, ENTRIES_MAX) else {
        out.cut.push(dir);
        return out;
    };
    let links = listed.into_iter().filter(|(_, e)| e.file_type().is_ok_and(|t| t.is_symlink()));
    for (name, _) in links.take(LINKS_MAX) {
        out.found.extend(found_at(&dir, Path::new(&name), true));
    }
    out
}

/// What the policy for the project at `root` (real) keeps of git's folders and `llm.sandbox-protect` (`protect`, as
/// set): `git_folders` of the root and of each other place runs write that a checkout may be (`places`: the
/// workspace's folders, `llm.sandbox-write`'s), then each protect entry inside the project resolved from the root.
/// On the policy's worker (`env_with`).
pub fn found(root: &Path, places: &[PathBuf], protect: &[String]) -> Read {
    let mut out = Read::default();
    let mut seen: Vec<&Path> = Vec::new();
    for at in std::iter::once(root).chain(places.iter().map(PathBuf::as_path)) {
        if seen.contains(&at) {
            continue;
        }
        seen.push(at);
        let r = git_folders(at);
        out.found.extend(r.found);
        out.cut.extend(r.cut);
    }
    for p in protect {
        let rel = Path::new(p);
        if rel.is_relative() && rel.components().all(|c| matches!(c, Component::Normal(_))) {
            out.found.extend(found_at(root, rel, false));
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
        self.git.iter().filter(|g| linux || !self.unlocked.contains(g)).cloned().collect()
    }

    /// What the run's rules say of it, after the protected names (`, nor git's folder gitdata`): git's folders kept by
    /// another name, a place that is git's folder itself (`, nor git's hooks and config`), macOS's past the lock's
    /// cap, and the `.git` folders whose links weren't followed. Nothing where there's none of those.
    pub fn said(&self, root: &Path, supervised: bool) -> String {
        let shown = |p: &PathBuf| crate::allow::from_project(root, p);
        let bare = |g: &PathBuf| self.bare.iter().any(|b| g.parent() == Some(b.as_path()));
        let folders: Vec<String> = self.git.iter().filter(|g| !bare(g)).map(shown).collect();
        let mut s = match folders.as_slice() {
            [] => String::new(),
            [one] => format!(", nor git's folder {one}"),
            all => format!(", nor git's folders {}", all.join(", ")),
        };
        if self.git.iter().any(bare) {
            s.push_str(", nor git's hooks and config");
        }
        let unlocked: Vec<String> = self.unlocked.iter().filter(|_| !supervised).map(shown).collect();
        if !unlocked.is_empty() {
            s.push_str(&format!(" ({} not locked: past the cap)", unlocked.join(", ")));
        }
        for c in &self.cut {
            s.push_str(&format!(
                " ({} holds more than {ENTRIES_MAX} entries: the links in it aren't followed)",
                shown(c)
            ));
        }
        s
    }

    /// The kept target `path` is in or on the way to, and whether it's git's — None: neither.
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
/// - One elsewhere isn't: no run writes there.
pub fn git_kept(read: &Read, kept_in: &[PathBuf], writes: &[PathBuf], cap: usize) -> GitKept {
    let mut out = GitKept::default();
    let inside = |p: &Path, places: &[PathBuf]| places.iter().any(|w| p.starts_with(w));
    let place_of =
        |p: &Path| kept_in.iter().filter(|w| p.starts_with(w)).max_by_key(|w| w.components().count());
    let mut kept: Vec<&Found> = Vec::new();
    for f in &read.found {
        if f.git && kept_in.iter().any(|w| w.starts_with(&f.target)) {
            if !out.bare.contains(&f.target) {
                out.bare.push(f.target.clone());
            }
            for n in ["hooks", "config"] {
                let p = f.target.join(n);
                if inside(&p, kept_in) && !out.git.contains(&p) {
                    out.git.push(p.clone());
                    out.protect.push(p.clone());
                    out.ways.push((p, Vec::new()));
                }
            }
            continue;
        }
        if place_of(&f.target).is_none() || out.protect.contains(&f.target) {
            continue;
        }
        kept.push(f);
        out.protect.push(f.target.clone());
        if f.git {
            out.git.push(f.target.clone());
        }
    }
    let mut locked: Vec<PathBuf> = Vec::new();
    let mut full = false;
    for f in kept {
        // (what a run can swap: where it writes, not the place itself, nor what the write rules already hold — a
        // kept target, a protected name)
        let held = |s: &Path| {
            out.protect.iter().any(|p| s.starts_with(p))
                || place_of(s).is_some_and(|w| super::protected_in(w, s))
                || writes.iter().any(|w| w == s)
        };
        let way: Vec<&(PathBuf, Step)> =
            f.way.iter().filter(|(s, _)| inside(s, writes) && !held(s)).collect();
        for (s, step) in &way {
            let off = !f.target.starts_with(s);
            if (*step != Step::Folder || off) && !out.marks.contains(s) {
                out.marks.push(s.clone());
            }
        }
        let paths: Vec<PathBuf> = way.iter().map(|(s, _)| s.clone()).collect();
        let new: Vec<&PathBuf> = paths.iter().filter(|s| !locked.contains(s)).collect();
        if !paths.is_empty() && (full || locked.len() + new.len() > cap) {
            full = true;
            out.unlocked.push(f.target.clone());
            continue;
        }
        locked.extend(new.into_iter().cloned());
        out.ways.push((f.target.clone(), paths));
    }
    out.cut = read.cut.iter().filter(|c| inside(c, kept_in)).cloned().collect();
    out
}

#[cfg(test)]
mod tests;
