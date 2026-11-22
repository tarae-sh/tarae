//! What a review run reads (bgreview, "tarae native" 2026-10-20): the snapshot it took (`snapshot::snap`'s tree)
//! as git holds it. Its members' tools are tarae's own `read` and `search` (`readtool::read_snapshot_seen`,
//! `nav::search_snapshot`) over this — each file's text from git's objects, never the checkout you go on
//! editing — so what a member reads is what its diff says, and every line it names is the snapshot's
//! (`notes::pin_finding` places it on the lines now). Nothing is written anywhere.
//!
//! - Listed once, on the worker that takes the snapshot (`ls-tree -r`, the sizes from `cat-file --batch-check`);
//!   texts read as a call asks for them, from one `cat-file --batch` a call, kept for the run's later calls up to
//!   `KEPT_MAX` bytes (the snapshot never changes: nothing kept goes stale).
//! - git runs with no hooks and never fetches a blob this clone doesn't have (`GIT_NO_LAZY_FETCH` — a partial
//!   clone's): such a file is left out, counted at the start and said when asked for.
//! - Left out, and said so when asked for: files named like keys (`sandbox::key_named_in`), links (a link's change
//!   is in the diff as its target text), submodules, files past `FILE_MAX`.
//! - A path is the checkout's (from its root, or absolute under it), resolved by its words alone — the snapshot has
//!   no links to follow; one outside the checkout is refused.
//! - Beside it, the files `review.rules` names (`Named`): the main checkout's, read once as the snapshot is taken
//!   and kept — each member reads only its own key's, by the path its prompt lists.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A file past this isn't read: a blob comes whole into memory (there's no file to stream or map) — and its change
/// is in the diff the reviewer is sent. What's kept for the run's other calls is `KEPT_MAX`'s to bound, not this:
/// this only keeps one blob's read in hand. Source grows past a round number unseen (term.rs went past the 1 MiB
/// this was), so it's well over this repository's biggest — term.rs, 1.28 MB on 2026-10-28 —, the chat's `read`
/// having no such wall for a text.
pub const FILE_MAX: u64 = 8 << 20;

/// The texts kept for a run's later calls at most (bytes): past it, the rest are read from git again each call.
/// (This repository's whole tree is ~35 MB: a run's searches of all of it read git once.)
const KEPT_MAX: usize = 64 << 20;

/// Why a file of the snapshot isn't read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Left {
    Key,
    Link,
    Submodule,
    /// Past `FILE_MAX` (its size).
    Big(u64),
    /// Not in this clone.
    Missing,
}

impl Left {
    /// Why, in words — after the file's path.
    pub fn said(self) -> String {
        match self {
            Left::Key => "named like a key — the reviewer doesn't read it".into(),
            Left::Link => "a link — not read (the diff has its target)".into(),
            Left::Submodule => "a submodule — not read".into(),
            Left::Big(n) => format!(
                "{} KB, past the {} MB a file the reviewer reads — its change is in the diff",
                n.div_ceil(1024),
                FILE_MAX >> 20
            ),
            Left::Missing => "not in this clone (never fetched) — not read".into(),
        }
    }
}

/// What a path names in the snapshot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Found {
    File,
    Folder,
    Left(Left),
    Nothing,
}

/// The snapshot a review run reads: its files (blob and size, by path from the root), those left out and why.
pub struct Snap {
    /// The checkout as the review names it, and its real path — a path under either is the checkout's.
    pub root: PathBuf,
    real: PathBuf,
    pub tree: String,
    files: BTreeMap<String, (String, u64)>,
    left: BTreeMap<String, Left>,
    kept: Mutex<Kept>,
    outlines: Mutex<HashMap<String, Arc<Vec<crate::nav::Sym>>>>,
    pub named: Named,
}

/// What `review.rules` names (your config's only, `trust::YOURS_ONLY`): files of the repository's main checkout a
/// review's members read beside the snapshot — each key an angle's name or `reviewer`, its paths from the main
/// checkout's root. Read once, as the snapshot is taken (`Snap::name`), the bytes kept: a run reads one version.
/// Through no link, never one named like a key, a plain file up to `FILE_MAX`; one not there is neither kept nor
/// said (one config serves every project).
#[derive(Default)]
pub struct Named {
    /// The main checkout (None: the repository has none — a bare one's — so nothing is read).
    pub main: Option<PathBuf>,
    /// The setting as it was when the snapshot was taken.
    pub keys: BTreeMap<String, Vec<String>>,
    /// Each path there: its bytes, or why it isn't read.
    files: BTreeMap<String, NamedRead>,
}

/// A `review.rules` file's bytes as kept, or why it isn't read.
pub type NamedRead = Result<Arc<[u8]>, String>;

#[derive(Default)]
struct Kept {
    bytes: usize,
    texts: HashMap<String, Arc<[u8]>>,
}

/// Its listing and kept texts are freed on a worker (`event::drop_later`) — a big tree's maps and up to `KEPT_MAX`
/// of texts, and the last hold may be the main loop's: a board card's `n` or a retake, a session's end, a card
/// dropped.
impl Drop for Snap {
    fn drop(&mut self) {
        let kept = std::mem::take(self.kept.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner));
        let outlines =
            std::mem::take(self.outlines.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner));
        crate::event::drop_later((
            std::mem::take(&mut self.files),
            std::mem::take(&mut self.left),
            kept,
            outlines,
            std::mem::take(&mut self.named.files),
        ));
    }
}

/// An instruction file there for the snapshot's reader (`Snap::rules`): its path, its lines, whether it's the
/// checkout's (git ignores it) rather than the snapshot's — and why it isn't read when it isn't (a link, past
/// `FILE_MAX`; its lines then 0). A `review.rules` file's (`Snap::named_rules`): `rel` its absolute path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub rel: String,
    pub lines: usize,
    pub from_checkout: bool,
    pub unread: Option<String>,
}

impl std::fmt::Debug for Snap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snap").field("root", &self.root).field("tree", &self.tree).finish_non_exhaustive()
    }
}

/// A text's lines, a last one with no line end counted.
fn lines_in(b: &[u8]) -> usize {
    b.iter().filter(|&&c| c == b'\n').count() + usize::from(!b.ends_with(b"\n"))
}

/// File `rel` of the main checkout `main`, for `review.rules` — None: nothing there. Its words for what isn't read
/// are its own: the snapshot's (`Left::said`) speak of the diff, which has nothing of these. Blocks (a worker's).
fn named_read(main: &Path, rel: &str) -> Option<NamedRead> {
    let path = Path::new(rel);
    if rel.is_empty() || !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return Some(Err("not a path inside the main checkout — not read".into()));
    }
    if crate::sandbox::key_named_in(main, &main.join(rel)) {
        return Some(Err(Left::Key.said()));
    }
    // (a link anywhere on the way said as one, never followed)
    let mut at = main.to_path_buf();
    let mut size = 0;
    for c in path.components() {
        at.push(c);
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_symlink() => {
                return Some(Err("a link — the reviewer follows none, not read".into()));
            }
            Ok(m) => size = m.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => return Some(Err(format!("not read: {e}"))),
        }
    }
    let big = |n: u64| {
        format!(
            "{} KB, past the {} MB a file the reviewer reads — not read",
            n.div_ceil(1024),
            FILE_MAX >> 20
        )
    };
    if size > FILE_MAX {
        return Some(Err(big(size)));
    }
    // (read through no link from the main checkout's root, as checked: one swapped in since is refused too)
    let dir = crate::beneath::Dir::root(main).and_then(|d| match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => d.folder(p, false),
        _ => Ok(d),
    });
    let name = path.file_name().unwrap_or_default();
    Some(match dir.and_then(|d| d.read_at_most(name, FILE_MAX as usize)) {
        Ok(Some(bytes)) => Ok(bytes.into()),
        Ok(None) => return None,
        Err(e) if e.kind() == std::io::ErrorKind::FileTooLarge => Err(big(FILE_MAX + 1)),
        Err(e) => Err(format!("not read: {e}")),
    })
}

/// git on the checkout at `root` for the snapshot: no hooks, and never a fetch for a blob this clone doesn't have.
fn git(root: &Path, args: &[&str]) -> Result<std::process::Command, String> {
    let all: Vec<&str> = crate::gitrepo::NO_HOOKS.iter().copied().chain(args.iter().copied()).collect();
    let mut cmd = crate::gitrepo::repo(root)?.command(&all);
    cmd.env("GIT_NO_LAZY_FETCH", "1");
    Ok(cmd)
}

impl Snap {
    /// Tree `tree` of the checkout at `root` listed (`ls-tree -r -z`, then the blobs' sizes from `cat-file
    /// --batch-check`). Blocks (a worker's).
    pub fn list(root: &Path, tree: &str) -> Result<Snap, String> {
        let out = crate::gitrepo::finish(&git(root, &["ls-tree", "-r", "-z", "--full-tree", tree])?, None)?;
        let out = String::from_utf8_lossy(&out);
        let mut left = BTreeMap::new();
        let mut wanted: Vec<(String, String)> = Vec::new();
        for rec in out.split('\0').filter(|r| !r.is_empty()) {
            let Some((meta, rel)) = rec.split_once('\t') else { continue };
            let mut f = meta.split_ascii_whitespace();
            let (Some(mode), Some(kind), Some(id)) = (f.next(), f.next(), f.next()) else { continue };
            // (a name git holds that isn't a plain path from the root is never one a tool's path could name)
            if !Path::new(rel).components().all(|c| matches!(c, Component::Normal(_))) {
                continue;
            }
            let why = match (mode, kind) {
                _ if crate::sandbox::key_named_in(root, &root.join(rel)) => Some(Left::Key),
                ("120000", _) => Some(Left::Link),
                (_, "commit") => Some(Left::Submodule),
                ("100644" | "100755", "blob") => None,
                _ => continue,
            };
            match why {
                Some(why) => drop(left.insert(rel.to_string(), why)),
                None => wanted.push((id.to_string(), rel.to_string())),
            }
        }
        let ids: String = wanted.iter().map(|(id, _)| format!("{id}\n")).collect();
        let sizes =
            crate::gitrepo::finish(&git(root, &["cat-file", "--batch-check"])?, Some(ids.as_bytes()))?;
        let sizes = String::from_utf8_lossy(&sizes);
        let mut files = BTreeMap::new();
        for ((id, rel), line) in wanted.into_iter().zip(sizes.lines()) {
            // `<id> blob <size>` — or `<id> missing`: not in this clone
            match line.split_ascii_whitespace().collect::<Vec<_>>()[..] {
                [_, "blob", size] => match size.parse::<u64>() {
                    Ok(size) if size <= FILE_MAX => drop(files.insert(rel, (id, size))),
                    Ok(size) => drop(left.insert(rel, Left::Big(size))),
                    Err(_) => drop(left.insert(rel, Left::Missing)),
                },
                _ => drop(left.insert(rel, Left::Missing)),
            }
        }
        Ok(Snap {
            root: root.to_path_buf(),
            real: crate::paths::real(root).unwrap_or_else(|| root.to_path_buf()),
            tree: tree.to_string(),
            files,
            left,
            kept: Mutex::default(),
            outlines: Mutex::default(),
            named: Named::default(),
        })
    }

    /// `review.rules`' files (`keys`: an angle's name or `reviewer` → paths from `main`'s root) read and kept
    /// (`Named`) — every key's: the angles that run are chosen after. Blocks (a worker's).
    pub fn name(&mut self, main: Option<PathBuf>, keys: BTreeMap<String, Vec<String>>) {
        let mut files = BTreeMap::new();
        if let Some(main) = &main {
            for rel in keys.values().flatten() {
                if !files.contains_key(rel)
                    && let Some(read) = named_read(main, rel)
                {
                    files.insert(rel.clone(), read);
                }
            }
        }
        self.named = Named { main, keys, files };
    }

    /// The paths `review.rules` gives `keys`' members, in the setting's order, each once.
    fn named_rels(&self, keys: &[String]) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for rel in keys.iter().filter_map(|k| self.named.keys.get(k)).flatten() {
            if !out.contains(&rel.as_str()) {
                out.push(rel);
            }
        }
        out
    }

    /// `review.rules`' files for the members of `keys`, by absolute path — each with its lines, or why it isn't
    /// read; one not there left out (as a missing instruction file is).
    pub fn named_rules(&self, keys: &[String]) -> Vec<Rule> {
        let Some(main) = &self.named.main else { return Vec::new() };
        (self.named_rels(keys).into_iter())
            .filter_map(|rel| {
                let path = main.join(rel).display().to_string();
                Some(match self.named.files.get(rel)? {
                    Ok(b) => Rule { rel: path, lines: lines_in(b), from_checkout: false, unread: None },
                    Err(why) => Rule { rel: path, lines: 0, from_checkout: false, unread: Some(why.clone()) },
                })
            })
            .collect()
    }

    /// `given` (a `read` item's path) as one of `review.rules`' files for `keys`' members: its absolute path in
    /// the main checkout — or its path from there, but only where the snapshot has nothing (a file of the snapshot
    /// is the snapshot's) — the path to show and its bytes as kept, or why it isn't read. None: not one of them.
    pub fn named_text(&self, given: &str, keys: &[String]) -> Option<(String, NamedRead)> {
        let main = self.named.main.as_ref()?;
        let path = Path::new(given.trim());
        let rel = self.named_rels(keys).into_iter().find(|rel| match path.is_absolute() {
            true => path == main.join(rel),
            false => self.rel_of(given).as_deref() == Some(*rel) && self.found(rel) == Found::Nothing,
        })?;
        Some((main.join(rel).display().to_string(), self.named.files.get(rel)?.clone()))
    }

    /// How many files it lists that this clone doesn't have (said at the start).
    pub fn missing(&self) -> usize {
        self.left.values().filter(|l| **l == Left::Missing).count()
    }

    /// `given` (from the root, or absolute) as a path of the snapshot (`/`-separated, "" the root) — None: outside
    /// the checkout. By its words: `..` steps back a name, never past the root.
    pub fn rel_of(&self, given: &str) -> Option<String> {
        let given = Path::new(given.trim());
        let rel = match given.is_absolute() {
            true => given.strip_prefix(&self.root).or_else(|_| given.strip_prefix(&self.real)).ok()?,
            false => given,
        };
        let mut parts: Vec<&str> = Vec::new();
        for c in rel.components() {
            match c {
                Component::Normal(s) => parts.push(s.to_str()?),
                Component::CurDir => {}
                Component::ParentDir => {
                    parts.pop()?;
                }
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        Some(parts.join("/"))
    }

    /// An instruction file (`worktree::INSTRUCTIONS`, at the checkout's top) the snapshot hasn't — git ignores it,
    /// and the project's rules would be out of the review's sight: read from the checkout as it is now, through no
    /// link, a plain file up to `FILE_MAX`, never one named like a key. None: not one of those names, or the
    /// snapshot has it (then it's the snapshot's, as any file); Some(Err): why it isn't read. Blocks (a worker's).
    pub fn instruction(&self, rel: &str) -> Option<Result<Vec<u8>, String>> {
        if !crate::worktree::INSTRUCTIONS.contains(&rel) || self.found(rel) != Found::Nothing {
            return None;
        }
        if crate::sandbox::key_named_in(&self.root, &self.root.join(rel)) {
            return Some(Err(Left::Key.said()));
        }
        let size = std::fs::symlink_metadata(self.root.join(rel)).map(|m| m.len()).unwrap_or(0);
        if size > FILE_MAX {
            return Some(Err(Left::Big(size).said()));
        }
        let read = crate::beneath::Dir::root(&self.root).and_then(|d| d.read(rel.as_ref()));
        Some(match read {
            Ok(Some(bytes)) if bytes.len() as u64 <= FILE_MAX => Ok(bytes),
            Ok(Some(bytes)) => Err(Left::Big(bytes.len() as u64).said()),
            Ok(None) => Err("no such file in the snapshot or the checkout".into()),
            Err(e) if e.raw_os_error() == Some(libc::ELOOP) => Err(Left::Link.said()),
            Err(e) => Err(format!("not read from the checkout: {e}")),
        })
    }

    /// The checkout's instruction files (`worktree::INSTRUCTIONS`, at its top) `read` gives — the snapshot's, else
    /// one git ignores from the checkout (`instruction`) — with their lines: named in the reviewer's prompt, whose
    /// listing and search never show an ignored one (frictions 55: it concluded there was none). One there that
    /// isn't read is listed with why — never taken for none. Blocks (a worker's).
    pub fn rules(&self) -> Vec<Rule> {
        let rule = |rel: &str, read: Result<Vec<u8>, String>, from_checkout| match read {
            Ok(b) => Rule { rel: rel.to_string(), lines: lines_in(&b), from_checkout, unread: None },
            Err(why) => Rule { rel: rel.to_string(), lines: 0, from_checkout, unread: Some(why) },
        };
        // (the checkout's, git ignoring it: there when its name is, a link too)
        let there = |rel: &str| std::fs::symlink_metadata(self.root.join(rel)).is_ok();
        (crate::worktree::INSTRUCTIONS.iter())
            .filter_map(|&rel| match (self.instruction(rel), self.found(rel)) {
                (Some(Err(_)), _) if !there(rel) => None,
                (Some(got), _) => Some(rule(rel, got, true)),
                (None, Found::File) => {
                    let text =
                        self.text(rel).map(|t| t.to_vec()).ok_or_else(|| "not read from git".to_string());
                    Some(rule(rel, text, false))
                }
                (None, Found::Left(why)) => Some(rule(rel, Err(why.said()), false)),
                (None, _) => None,
            })
            .collect()
    }

    /// What `rel` (a path of the snapshot) names.
    pub fn found(&self, rel: &str) -> Found {
        if self.files.contains_key(rel) {
            return Found::File;
        }
        if let Some(l) = self.left.get(rel) {
            return Found::Left(*l);
        }
        let prefix = format!("{rel}/");
        let under = |k: Option<&String>| k.is_some_and(|k| k.starts_with(&prefix));
        match rel.is_empty()
            || under(self.files.range(prefix.clone()..).next().map(|e| e.0))
            || under(self.left.range(prefix.clone()..).next().map(|e| e.0))
        {
            true => Found::Folder,
            false => Found::Nothing,
        }
    }

    /// The files it reads in `scope` (a path of the snapshot: a file, a folder, "" all), by path.
    pub fn files_in<'a>(&'a self, scope: &'a str) -> Box<dyn Iterator<Item = &'a str> + 'a> {
        if scope.is_empty() {
            return Box::new(self.files.keys().map(String::as_str));
        }
        if self.files.contains_key(scope) {
            return Box::new(std::iter::once(scope));
        }
        let prefix = format!("{scope}/");
        Box::new(
            self.files
                .range(prefix.clone()..)
                .map(|e| e.0.as_str())
                .take_while(move |k| k.starts_with(&prefix)),
        )
    }

    /// The text of file `rel` as the snapshot has it (None: not one it reads). Blocks (a worker's).
    pub fn text(&self, rel: &str) -> Option<Arc<[u8]>> {
        let mut out = None;
        self.each_text(&[rel], &mut |_, t| {
            out = Some(t);
            false
        })
        .ok()?;
        out
    }

    /// The texts of `rels` (those it reads — others skipped) to `each` with their paths: those kept first, the
    /// rest from one `cat-file --batch` in their order, kept while there's room (false from `each`: enough —
    /// the rest unread). Blocks (a worker's).
    pub fn each_text(
        &self,
        rels: &[&str],
        each: &mut dyn FnMut(&str, Arc<[u8]>) -> bool,
    ) -> Result<(), String> {
        let mut todo: Vec<(&str, &str)> = Vec::new();
        for &rel in rels {
            let Some((id, _)) = self.files.get(rel) else { continue };
            let kept = self.kept.lock().unwrap_or_else(|p| p.into_inner()).texts.get(rel).cloned();
            match kept {
                Some(t) => {
                    if !each(rel, t) {
                        return Ok(());
                    }
                }
                None => todo.push((rel, id)),
            }
        }
        if todo.is_empty() {
            return Ok(());
        }
        let cmd = git(&self.root, &["cat-file", "--batch"])?;
        let said = |e: std::io::Error| format!("git cat-file: {e}");
        let (s, mut w) = crate::spawn::setsid_fed(&cmd).map_err(said)?;
        let ids: String = todo.iter().map(|(_, id)| format!("{id}\n")).collect();
        // (fed from a thread of its own, its errors read on another: neither pipe can hold the other up)
        let feed = crate::event::thread("tarae-snap-feed", move || {
            let _ = w.write_all(ids.as_bytes());
        });
        let errs = s.stderr.map(|mut e| {
            crate::event::thread("tarae-snap-err", move || {
                let mut v = Vec::new();
                let _ = e.read_to_end(&mut v);
                v
            })
        });
        // (its first failure kept, the loop left — never a return before the wait below: git reaped whatever came)
        let read = |out: std::io::PipeReader| -> Result<(), String> {
            let mut out = std::io::BufReader::new(out);
            for (rel, _) in &todo {
                let mut head = String::new();
                if out.read_line(&mut head).map_err(said)? == 0 {
                    return Err(format!("git cat-file ended before {rel}"));
                }
                // `<id> blob <size>` — or `<id> missing` (gone since it was listed: skipped)
                let size = match head.split_ascii_whitespace().collect::<Vec<_>>()[..] {
                    [_, "blob", size] => {
                        size.parse::<usize>().map_err(|_| format!("git cat-file: {head}"))?
                    }
                    _ => continue,
                };
                let mut bytes = vec![0; size + 1];
                out.read_exact(&mut bytes).map_err(said)?;
                bytes.truncate(size);
                let text: Arc<[u8]> = bytes.into();
                let mut kept = self.kept.lock().unwrap_or_else(|p| p.into_inner());
                if kept.bytes + size <= KEPT_MAX {
                    kept.bytes += size;
                    kept.texts.insert(rel.to_string(), text.clone());
                }
                drop(kept);
                if !each(rel, text) {
                    break;
                }
            }
            Ok(())
        };
        // (its output closed early, git's next write fails, it exits, the wait returns)
        let done = s.stdout.map_or(Ok(()), read);
        let _ = feed.join();
        let err = errs.and_then(|t| t.join().ok()).unwrap_or_default();
        let status = crate::spawn::wait(s.pid).map_err(said)?;
        if !status.success() && done.is_ok() && !err.is_empty() {
            let e = String::from_utf8_lossy(&err);
            return Err(format!("git cat-file: {}", e.lines().next().unwrap_or("failed")));
        }
        done
    }

    /// File `rel`'s outline from its text (`outlines::of_text`), kept for the run.
    pub fn outline(&self, rel: &str, text: &str) -> Result<Arc<Vec<crate::nav::Sym>>, String> {
        if let Some(s) = self.outlines.lock().unwrap_or_else(|p| p.into_inner()).get(rel) {
            return Ok(s.clone());
        }
        let syms = crate::outlines::of_text(&self.root.join(rel), None, text)?;
        self.outlines.lock().unwrap_or_else(|p| p.into_inner()).insert(rel.to_string(), syms.clone());
        Ok(syms)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// git in the test's own repository, none of the user's config read (no global or system file — no hooks of
    /// theirs, no signing), `input` on its stdin. Its output.
    pub(crate) fn git_in(dir: &Path, args: &[&str], input: &[u8]) -> String {
        let mut child = crate::gitrepo::test_git(dir)
            .args(["-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"])
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn git(dir: &Path, args: &[&str]) {
        git_in(dir, args, b"");
    }

    /// A checkout with a file, a key, a link and a file past the cap — its tree as `snapshot::snap` takes it.
    fn checkout(dir: &Path) -> String {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(dir.join("id_rsa"), "tarae-fake-token\n").unwrap();
        std::os::unix::fs::symlink("src/a.rs", dir.join("link")).unwrap();
        // (sparse: its size without writing it — git still reads it whole as it adds it)
        std::fs::File::create(dir.join("big.txt")).unwrap().set_len(FILE_MAX + 1).unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.email", "t@t"],
            &["config", "user.name", "t"],
            &["add", "-A"],
            &["commit", "-q", "-m", "base"],
        ] {
            git(dir, args);
        }
        std::fs::write(dir.join("new.txt"), "untracked\n").unwrap();
        crate::snapshot::snap(dir).unwrap().id
    }

    fn text(s: &Snap, rel: &str) -> Option<String> {
        s.text(rel).map(|t| String::from_utf8_lossy(&t).into_owned())
    }

    /// What it reads is the snapshot's text, whatever the file holds now; a key, a link and a file past the cap
    /// are listed as left out, each with its reason; folders are told from files.
    #[test]
    fn the_snapshot_holds_its_texts_and_leaves_out_keys_links_and_big_files() {
        let s = crate::scratch::Scratch::new("snapread");
        let root = s.join("proj");
        let tree = checkout(&root);
        std::fs::write(root.join("src/a.rs"), "fn changed() {}\n").unwrap();
        let snap = Snap::list(&root, &tree).unwrap();
        assert_eq!(text(&snap, "src/a.rs").as_deref(), Some("fn a() {}\n"), "as taken");
        assert_eq!(text(&snap, "src/a.rs").as_deref(), Some("fn a() {}\n"), "kept: the same again");
        assert_eq!(text(&snap, "new.txt").as_deref(), Some("untracked\n"));
        assert_eq!(snap.found("id_rsa"), Found::Left(Left::Key));
        assert_eq!(snap.found("link"), Found::Left(Left::Link));
        assert_eq!(snap.found("big.txt"), Found::Left(Left::Big(FILE_MAX + 1)));
        assert_eq!(
            (snap.found("src"), snap.found(""), snap.found("sr")),
            (Found::Folder, Found::Folder, Found::Nothing)
        );
        for gone in ["id_rsa", "link", "big.txt", "src"] {
            assert_eq!(text(&snap, gone), None, "{gone}");
        }
        assert_eq!(snap.files_in("").collect::<Vec<_>>(), ["new.txt", "src/a.rs"]);
        assert_eq!(snap.files_in("src").collect::<Vec<_>>(), ["src/a.rs"]);
    }

    /// A file past what this read once (1 MiB — term.rs and editor.rs both went past it) is read as the chat reads
    /// one: by range, by name and in a search's hits; one past `FILE_MAX` is still left out, said in its place.
    #[test]
    fn a_big_source_is_read_by_range_name_and_search_and_a_huge_one_is_said() {
        let s = crate::scratch::Scratch::new("snapread-big");
        let root = s.join("proj");
        std::fs::create_dir_all(root.join("src")).unwrap();
        let filler = format!("// {}\n", "filler ".repeat(13));
        let lines = 15_000;
        let mut big = filler.repeat(lines);
        big.push_str("fn needle_here() {\n    let found = 1;\n}\n");
        assert!(big.len() as u64 > 1 << 20 && (big.len() as u64) < FILE_MAX, "{} bytes", big.len());
        std::fs::write(root.join("src/big.rs"), &big).unwrap();
        let tree = checkout(&root);
        let snap = Snap::list(&root, &tree).unwrap();
        assert_eq!(snap.found("src/big.rs"), Found::File);
        let read = |f: &[&str]| crate::readtool::read_snapshot(&snap, &serde_json::json!({ "files": f }));
        let at = lines + 2;
        let got = read(&[&format!("src/big.rs:{at}-{at}"), "big.txt"]).unwrap();
        assert!(got.contains(&format!("{at}:     let found = 1;")), "by range: {got}");
        assert!(got.contains("== big.txt: ") && got.contains("past the 8 MB"), "the huge one said: {got}");
        let got = read(&["src/big.rs:needle_here"]).unwrap();
        assert!(got.contains("fn needle_here() {") && got.contains("let found = 1;"), "by name: {got}");
        let search = |args| crate::nav::search_snapshot(&snap, &args);
        let got = search(serde_json::json!({ "pattern": "let found" })).unwrap();
        assert!(got.contains("src/big.rs") && got.contains(&format!("{at}:")), "searched: {got}");
        let got = search(serde_json::json!({ "pattern": "x", "path": "big.txt" })).unwrap_err();
        assert!(got.contains("past the 8 MB"), "the huge one said: {got}");
    }

    /// `review.rules` over a main checkout `main/` (its `.sb/` ignored, as git ignores it in every checkout of it:
    /// `.sb/dev/design.md` of 2 lines, `secret.md` beside it, a key-named file, a link, a link on the way, one
    /// past `FILE_MAX`) and a session's checkout `wt/` of it with its own `.sb/dev/design.md`, `outside.txt`
    /// beside both: `wt`'s snapshot with `keys`' files read. The scratch, the main checkout (real path), the
    /// session's, the snapshot.
    pub(crate) fn rules_fixture(
        name: &str,
        keys: &[(&str, &[&str])],
    ) -> (crate::scratch::Scratch, PathBuf, PathBuf, Snap) {
        let s = crate::scratch::Scratch::new(name);
        let dir = crate::paths::real(&s).unwrap();
        let (main, wt) = (dir.join("main"), dir.join("wt"));
        std::fs::create_dir_all(&main).unwrap();
        std::fs::write(main.join("a.txt"), "one\n").unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.email", "t@t"],
            &["config", "user.name", "t"],
            &["add", "-A"],
            &["commit", "-q", "-m", "base"],
            &["worktree", "add", "-q", "../wt", "-b", "side"],
        ] {
            git(&main, args);
        }
        std::fs::write(main.join(".git/info/exclude"), ".sb/\n").unwrap();
        let sb = main.join(".sb/dev");
        std::fs::create_dir_all(&sb).unwrap();
        std::fs::write(sb.join("design.md"), "## Design (CLAUDE.md)\n- rule one\n").unwrap();
        std::fs::write(sb.join("secret.md"), "the main checkout's other file\n").unwrap();
        std::fs::write(sb.join("id_rsa"), "tarae-fake-token\n").unwrap();
        std::os::unix::fs::symlink("design.md", sb.join("link.md")).unwrap();
        std::os::unix::fs::symlink("dev", main.join(".sb/linked")).unwrap();
        std::fs::File::create(sb.join("big.md")).unwrap().set_len(FILE_MAX + 1).unwrap();
        std::fs::write(dir.join("outside.txt"), "marker\n").unwrap();
        std::fs::write(wt.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::create_dir_all(wt.join(".sb/dev")).unwrap();
        std::fs::write(wt.join(".sb/dev/design.md"), "the session's own copy\n").unwrap();
        let tree = crate::snapshot::snap(&wt).unwrap().id;
        let mut snap = Snap::list(&wt, &tree).unwrap();
        let keys = keys.iter().map(|(k, ps)| (k.to_string(), ps.iter().map(|p| p.to_string()).collect()));
        snap.name(crate::instructions::main_or_self(&wt), keys.collect());
        (s, main, wt, snap)
    }

    /// A `read` item's answer from `snap` for a member of `keys`.
    fn read_as(snap: &Snap, f: &str, keys: &[&str]) -> Result<String, String> {
        let keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        crate::readtool::read_snapshot_seen(snap, &serde_json::json!({ "files": [f] }), &keys).0
    }

    /// A design-angle member's `read` gives the file `review.rules` names for design, by its absolute path or
    /// its path from the main checkout (the snapshot has nothing there) — and nothing beside it: a file in the
    /// same folder, a `../` out of the main checkout, and the file asked by another angle's member are refused
    /// with the snapshot's words.
    #[test]
    fn a_rules_file_is_read_by_its_angle_and_nothing_beside_it() {
        let (_s, main, _wt, snap) = rules_fixture("snapread-rules", &[("design", &[".sb/dev/design.md"])]);
        let abs = main.join(".sb/dev/design.md").display().to_string();
        let got = read_as(&snap, &format!("{abs}:1-2"), &["design"]).unwrap();
        assert!(
            got.contains(&format!("== {abs}:1-2 (of 2 lines)\n1: ## Design (CLAUDE.md)\n2: - rule one")),
            "{got}"
        );
        let got = read_as(&snap, ".sb/dev/design.md", &["design"]).unwrap();
        assert!(got.contains(&format!("== {abs} (of 2 lines)")) && got.contains("2: - rule one"), "{got}");
        let outside = "outside the checkout — the reviewer reads only its snapshot";
        for f in [
            main.join(".sb/dev/secret.md"),
            main.join(".sb/dev/../../../outside.txt"),
            main.join("../outside.txt"),
        ] {
            let got = read_as(&snap, &f.display().to_string(), &["design"]).unwrap_err();
            assert!(got.contains(outside), "{}: {got}", f.display());
        }
        let got = read_as(&snap, ".sb/dev/secret.md", &["design"]).unwrap_err();
        assert!(got.contains("no such file in the snapshot"), "{got}");
        assert!(read_as(&snap, "../outside.txt", &["design"]).unwrap_err().contains(outside));
        let got = read_as(&snap, &abs, &["tests"]).unwrap_err();
        assert!(got.contains(outside), "not the tests angle's: {got}");
    }

    /// #450: the reviewer's `read` takes the chat's rule on long lines — a snapshot's file and a `review.rules`
    /// file read whole cut a line past 500 characters, said with the way to it, and `path:N` gives it whole.
    #[test]
    fn the_reviewers_read_cuts_a_long_line_says_it_and_gives_it_whole_when_asked() {
        let (_s, main, wt, _) = rules_fixture("snapread-long", &[]);
        let long = format!("{}LINE_END", "x".repeat(6857));
        std::fs::write(main.join(".sb/dev/long.md"), format!("# Rules\n{long}\n")).unwrap();
        std::fs::write(wt.join("long.txt"), format!("one\n{long}\n")).unwrap();
        let tree = crate::snapshot::snap(&wt).unwrap().id;
        let mut snap = Snap::list(&wt, &tree).unwrap();
        let keys = [("design".to_string(), vec![".sb/dev/long.md".to_string()])];
        snap.name(crate::instructions::main_or_self(&wt), keys.into_iter().collect());
        let rules = main.join(".sb/dev/long.md").display().to_string();
        for shown in ["long.txt", rules.as_str()] {
            let got = read_as(&snap, shown, &["design"]).unwrap();
            let cut = format!("… line 2 cut at 500 of 6,865 characters — `{shown}:2` gives it whole");
            assert!(got.ends_with(&cut) && !got.contains("LINE_END"), "{shown} whole: {got}");
            let got = read_as(&snap, &format!("{shown}:2"), &["design"]).unwrap();
            assert!(got.contains(&format!("2: {long}")) && !got.contains("cut at"), "{shown}:2: {got}");
        }
    }

    /// A rules file named like a key, a link (or one on the way) and one past `FILE_MAX` are listed as not read,
    /// each with why, and said so when asked for; one not there isn't listed at all.
    #[test]
    fn a_key_named_linked_or_huge_rules_file_is_said_and_a_missing_one_isnt() {
        let paths: &[&str] = &[
            ".sb/dev/design.md",
            ".sb/dev/id_rsa",
            ".sb/dev/link.md",
            ".sb/linked/design.md",
            ".sb/dev/big.md",
            ".sb/dev/gone.md",
        ];
        let (_s, main, _wt, snap) = rules_fixture("snapread-rules-left", &[("design", paths)]);
        let rules = snap.named_rules(&["design".into()]);
        let at = |rel: &str| main.join(rel).display().to_string();
        let got: Vec<(String, usize, Option<String>)> =
            rules.iter().map(|r| (r.rel.clone(), r.lines, r.unread.clone())).collect();
        let link = Some("a link — the reviewer follows none, not read".to_string());
        assert_eq!(
            got,
            [
                (at(".sb/dev/design.md"), 2, None),
                (at(".sb/dev/id_rsa"), 0, Some(Left::Key.said())),
                (at(".sb/dev/link.md"), 0, link.clone()),
                (at(".sb/linked/design.md"), 0, link),
                (
                    at(".sb/dev/big.md"),
                    0,
                    Some("8193 KB, past the 8 MB a file the reviewer reads — not read".into())
                ),
            ]
        );
        let got = read_as(&snap, &at(".sb/dev/id_rsa"), &["design"]).unwrap_err();
        assert_eq!(got, format!("== {}: {}", at(".sb/dev/id_rsa"), Left::Key.said()));
        let said = crate::reviewer::named_said(Some(&snap), &["design".into()], false);
        assert!(said.contains(&format!("{} (2 lines)", at(".sb/dev/design.md"))), "{said}");
        assert!(said.contains("Named in review.rules but not read — say so in your verdict: "), "{said}");
        assert!(!said.contains("gone.md"), "a missing one isn't listed: {said}");
    }

    /// A rules file edited — or removed — after the review began reads as it was when it began.
    #[test]
    fn a_rules_file_edited_after_the_review_began_reads_as_it_was() {
        let (_s, main, _wt, snap) =
            rules_fixture("snapread-rules-edit", &[("design", &[".sb/dev/design.md"])]);
        let file = main.join(".sb/dev/design.md");
        std::fs::write(&file, "## Design (CLAUDE.md)\n- rule two, since\n- and a third\n").unwrap();
        let got = read_as(&snap, &file.display().to_string(), &["design"]).unwrap();
        assert!(got.contains("2: - rule one") && !got.contains("since"), "as it began: {got}");
        assert_eq!(snap.named_rules(&["design".into()])[0].lines, 2);
        std::fs::remove_file(&file).unwrap();
        assert!(read_as(&snap, &file.display().to_string(), &["design"]).unwrap().contains("2: - rule one"));
    }

    /// Run from a session's checkout, the files are the main checkout's — never the session's own `.sb/` copy;
    /// from the main checkout, its own; a bare repository's checkout has no main one: nothing read, said once.
    #[test]
    fn from_a_sessions_checkout_the_rules_files_are_the_main_checkouts() {
        let (s, main, wt, snap) = rules_fixture("snapread-rules-main", &[("design", &[".sb/dev/design.md"])]);
        assert_eq!(crate::instructions::main_or_self(&wt), Some(main.clone()));
        assert_eq!(crate::instructions::main_or_self(&main), Some(main.clone()), "the main checkout: itself");
        assert_eq!(
            snap.named_rules(&["design".into()])[0].rel,
            main.join(".sb/dev/design.md").display().to_string()
        );
        for f in [".sb/dev/design.md".to_string(), wt.join(".sb/dev/design.md").display().to_string()] {
            let got = read_as(&snap, &f, &["design"]);
            assert!(!format!("{got:?}").contains("the session's own copy"), "{f}: {got:?}");
        }
        // (a bare repository's worktree)
        let dir = crate::paths::real(&s).unwrap();
        git(&dir, &["clone", "-q", "--bare", "main", "b.git"]);
        git(&dir.join("b.git"), &["worktree", "add", "-q", "../bw", "main"]);
        let bw = dir.join("bw");
        assert_eq!(crate::instructions::main_or_self(&bw), None);
        let tree = crate::snapshot::snap(&bw).unwrap().id;
        let mut bare = Snap::list(&bw, &tree).unwrap();
        bare.name(None, [("design".to_string(), vec![".sb/dev/design.md".to_string()])].into());
        assert!(bare.named_rules(&["design".into()]).is_empty(), "the prompts quiet");
        assert!(crate::reviewer::named_said(Some(&bare), &["design".into()], false).is_empty());
        assert_eq!(
            crate::reviewer::named_aside(Some(&bare), None),
            ["review.rules: this repository has no main checkout, so its files aren't read"]
        );
    }

    /// A snapshot let go here (the main loop, a card's `n`) frees its listing and texts on a worker.
    #[test]
    fn a_snapshot_is_freed_on_a_worker() {
        let s = crate::scratch::Scratch::new("snapread-drop");
        let root = s.join("proj");
        let tree = checkout(&root);
        let snap = Snap::list(&root, &tree).unwrap();
        assert!(snap.text("src/a.rs").is_some(), "a text kept");
        let before = crate::event::DROPPED_LATER.with(|n| n.get());
        drop(snap);
        assert_eq!(crate::event::DROPPED_LATER.with(|n| n.get()), before + 1);
    }

    /// A git-ignored CLAUDE.md is read from the checkout as it is now, said so in its header; a tracked one is the
    /// snapshot's; another ignored file, a deeper CLAUDE.md and a linked AGENTS.md aren't read.
    #[test]
    fn an_ignored_instruction_file_is_read_from_the_checkout_and_said_so() {
        let s = crate::scratch::Scratch::new("snapread-instructions");
        let root = s.join("proj");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join(".gitignore"), "CLAUDE.md\nAGENTS.md\nnotes.txt\nsrc/CLAUDE.md\n").unwrap();
        std::fs::write(root.join("CLAUDE.local.md"), "tracked rules\n").unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.email", "t@t"],
            &["config", "user.name", "t"],
            &["add", "-A"],
            &["commit", "-q", "-m", "base"],
        ] {
            git(&root, args);
        }
        std::fs::write(root.join("CLAUDE.md"), "the project's rules\n").unwrap();
        std::fs::write(root.join("notes.txt"), "ignored\n").unwrap();
        std::fs::write(root.join("src/CLAUDE.md"), "deeper\n").unwrap();
        std::os::unix::fs::symlink("notes.txt", root.join("AGENTS.md")).unwrap();
        let snap = Snap::list(&root, &crate::snapshot::snap(&root).unwrap().id).unwrap();
        std::fs::write(root.join("CLAUDE.local.md"), "changed since\n").unwrap();
        let read = |f: &str| match crate::readtool::read_snapshot(&snap, &serde_json::json!({ "files": [f] }))
        {
            Ok(s) | Err(s) => s,
        };
        let claude = read("CLAUDE.md");
        assert!(claude.contains("1: the project's rules"), "{claude}");
        assert!(
            claude.starts_with("== CLAUDE.md (of 1 line) — from the checkout: not in the snapshot"),
            "{claude}"
        );
        let tracked = read("CLAUDE.local.md");
        assert!(tracked.contains("1: tracked rules") && !tracked.contains("from the checkout"), "{tracked}");
        for refused in ["notes.txt", "src/CLAUDE.md", "AGENTS.md"] {
            let said = read(refused);
            assert!(!said.contains("1: "), "{refused}: {said}");
        }
        assert!(read("AGENTS.md").contains("a link"), "{}", read("AGENTS.md"));
        // (named for the reviewer's prompt: what read gives, the ignored one marked the checkout's)
        let rule = |rel: &str, from_checkout| Rule { rel: rel.into(), lines: 1, from_checkout, unread: None };
        let rules = snap.rules();
        assert_eq!(rules[..2], [rule("CLAUDE.md", true), rule("CLAUDE.local.md", false)]);
        // (one there that isn't read — the linked AGENTS.md — said with why, never taken for none: the review's
        // finding)
        assert_eq!((rules.len(), rules[2].rel.as_str(), rules[2].lines), (3, "AGENTS.md", 0), "{rules:?}");
        assert!(rules[2].unread.as_deref().is_some_and(|w| w.contains("a link")), "{rules:?}");
    }

    /// frictions #188: a long git-ignored CLAUDE.md asked whole comes as its headings with their lines (a
    /// snapshot's file's outline — never its first lines alone, nor the whole text past `WHOLE_MAX`), nested, a
    /// heading in a code block not one; a section read by those lines.
    #[test]
    fn a_long_instruction_file_asked_whole_answers_its_headings_and_a_range_reads_a_section() {
        let s = crate::scratch::Scratch::new("snapread-headings");
        let root = s.join("proj");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".gitignore"), "CLAUDE.md\n").unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.email", "t@t"],
            &["config", "user.name", "t"],
            &["add", "-A"],
            &["commit", "-q", "-m", "base"],
        ] {
            git(&root, args);
        }
        let mut lines: Vec<String> = vec!["# Project".into(), String::new()];
        lines.extend((0..400).map(|i| format!("- the project's direction, its point {i} of four hundred")));
        let direction = lines.len() + 1;
        lines.push("## Direction".into());
        lines.extend(["- one way each", "```sh", "## not a heading", "```"].map(String::from));
        let rules = lines.len() + 1;
        lines.extend(["## Rules", "- the first rule", "- the second rule"].map(String::from));
        let sub = lines.len() + 1;
        lines.extend(["### Sub", "- a rule under it"].map(String::from));
        let n = lines.len();
        let text = lines.join("\n") + "\n";
        assert!(text.len() > 16 * 1024, "past read's whole: {} bytes", text.len());
        std::fs::write(root.join("CLAUDE.md"), &text).unwrap();
        let snap = Snap::list(&root, &crate::snapshot::snap(&root).unwrap().id).unwrap();
        let read =
            |f: &str| crate::readtool::read_snapshot(&snap, &serde_json::json!({ "files": [f] })).unwrap();
        let whole = read("CLAUDE.md");
        assert!(whole.starts_with("== CLAUDE.md — ") && whole.contains("its outline"), "{whole}");
        assert!(whole.contains("from the checkout"), "said the checkout's: {whole}");
        for row in [
            format!("# Project  1–{n}"),
            format!("  ## Direction  {direction}–{}", rules - 1),
            format!("  ## Rules  {rules}–{n}"),
            format!("    ### Sub  {sub}–{n}"),
        ] {
            assert!(whole.lines().any(|l| l == row), "{row:?} in {whole}");
        }
        assert!(!whole.contains("not a heading") && !whole.contains("point 0"), "headings only: {whole}");
        let section = read(&format!("CLAUDE.md:{rules}-{}", sub - 1));
        assert!(section.contains(&format!("{rules}: ## Rules\n")), "{section}");
        assert!(section.contains("- the second rule") && !section.contains("### Sub"), "{section}");
    }

    /// A path from the root or absolute under it (as given or as its real path) is the snapshot's; `..` past the
    /// root, or anywhere else, is outside.
    #[test]
    fn a_path_outside_the_checkout_is_none() {
        let s = crate::scratch::Scratch::new("snapread-paths");
        let root = s.join("proj");
        let tree = checkout(&root);
        let snap = Snap::list(&root, &tree).unwrap();
        let real = std::fs::canonicalize(&root).unwrap();
        assert_eq!(snap.rel_of("src/a.rs").as_deref(), Some("src/a.rs"));
        assert_eq!(snap.rel_of("./src/../new.txt").as_deref(), Some("new.txt"));
        assert_eq!(snap.rel_of(&root.join("src").display().to_string()).as_deref(), Some("src"));
        assert_eq!(snap.rel_of(&real.join("src/a.rs").display().to_string()).as_deref(), Some("src/a.rs"));
        for out in ["../x.txt", "src/../../x", "/etc/hosts"] {
            assert_eq!(snap.rel_of(out), None, "{out}");
        }
        assert_eq!(snap.rel_of(&s.join("x").display().to_string()), None);
    }

    /// A blob this clone doesn't have (as a partial clone's): never fetched, left out and counted.
    #[test]
    fn a_blob_this_clone_lacks_is_left_out_and_counted() {
        let s = crate::scratch::Scratch::new("snapread-missing");
        let root = s.join("proj");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        let here = git_in(&root, &["hash-object", "-w", "--stdin"], b"here\n");
        let gone = "0123456789abcdef0123456789abcdef01234567";
        let entries = format!("100644 blob {here}\there.txt\n100644 blob {gone}\tgone.txt\n");
        let tree = git_in(&root, &["mktree", "--missing"], entries.as_bytes());
        let snap = Snap::list(&root, &tree).unwrap();
        assert_eq!((snap.missing(), snap.found("gone.txt")), (1, Found::Left(Left::Missing)));
        assert_eq!(text(&snap, "here.txt").as_deref(), Some("here\n"));
    }

    /// A measurement, not a check (`cargo perf`): this repository's tree as of `tests/perf.rs`'s `CORPUS` (its
    /// paths) listed — what a review run waits for before its first member starts — and every text read once, as a
    /// first search of the whole tree does (later ones read what's kept).
    #[test]
    fn perf_snapshot_reads() {
        if crate::test_results::unoptimized("snapread::tests::perf_snapshot_reads") {
            return;
        }
        let corpus = crate::test_results::corpus();
        const PATHS: &[&str] =
            &["src", "runtime", "docs", "tests", "xtask/src", "Cargo.toml", "README.md", "CONTRIBUTING.md"];
        let s = crate::scratch::Scratch::new("snapread-perf");
        let root = s.join("proj");
        std::fs::create_dir_all(&root).unwrap();
        let here = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let archive = crate::gitrepo::test_git(&here)
            .args(["archive", "--format=tar", corpus, "--"])
            .args(PATHS)
            .output()
            .unwrap();
        if !archive.status.success() {
            let why = format!("snapread::tests::perf_snapshot_reads — no commit {corpus} here");
            return crate::test_results::said_skip(&why);
        }
        let mut tar = std::process::Command::new("tar")
            .args(["-xf", "-"])
            .current_dir(&root)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        tar.stdin.take().unwrap().write_all(&archive.stdout).unwrap();
        assert!(tar.wait().unwrap().success());
        for args in [&["init", "-q", "-b", "main"][..], &["add", "-A"]] {
            git(&root, args);
        }
        let tree = crate::snapshot::snap(&root).unwrap().id;
        let mut runs = Vec::new();
        for _ in 0..5 {
            let t = std::time::Instant::now();
            let snap = Snap::list(&root, &tree).unwrap();
            let listed = t.elapsed();
            let all: Vec<&str> = snap.files_in("").collect();
            let (mut n, mut bytes) = (0, 0);
            snap.each_text(&all, &mut |_, t| {
                (n, bytes) = (n + 1, bytes + t.len());
                true
            })
            .unwrap();
            runs.push((listed, t.elapsed() - listed, n, bytes));
        }
        runs.sort();
        let (listed, read, n, bytes) = runs[runs.len() / 2];
        let line = format!(
            "PERF snapshot_list_ms {:.1} noise=30\nPERF snapshot_read_all_ms {:.1} noise=30\n\
             (snapshot: {n} files, {} MB)\n",
            listed.as_secs_f64() * 1e3,
            read.as_secs_f64() * 1e3,
            bytes >> 20,
        );
        let _ = std::io::stderr().write_all(line.as_bytes());
    }
}
