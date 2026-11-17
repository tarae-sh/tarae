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
