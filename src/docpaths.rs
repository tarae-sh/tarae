//! Documents by path (#214): `Editor.docs` searched for a file's buffer. The documents are a `Docs`, which lays
//! them out by path's hash (and by id) at the first lookup and keeps that until they change — one added, removed or
//! put in another's place (its own methods, a document dropped), or a path set (`DocPath::set`): every lookup then is
//! a hash and one compare away, from event to event and frame to frame. The same document a search gives: the first
//! with that path, as `docs.iter().find` gives it.
//!
//! Why: a review's rail asks each file's buffer several times a frame and presence each place of each busy session's
//! turn every event — with many sessions open (17, ~300 buffers, a 49-file review) a search a time held the main loop
//! for seconds (#214's sample: most of it in `Path`'s component compares); laid out anew each event and frame, the
//! buffers of every idle session were hashed again every time (#214's second piece).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};

use crate::document::{DocId, Document};
use crate::editor::Editor;

thread_local! {
    /// Bumped whenever a document's path may have changed on this thread — set, or its document dropped (one put in
    /// another's place drops that one): what `Docs` laid out is good while this stays. Per thread: the documents live
    /// on the main loop's (a test's own), and another test's documents don't move this one's.
    static MOVED: Cell<u64> = const { Cell::new(0) };
}

fn moved() {
    MOVED.with(|m| m.set(m.get().wrapping_add(1)));
}

/// A document's path (`Document.path`): read as the `Option<PathBuf>` it is; changed only through `set` (or `take`),
/// which tells `Docs` — a path changed in place would leave what it laid out finding the wrong buffer, or none.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct DocPath(Option<PathBuf>);

impl DocPath {
    pub fn new(path: Option<PathBuf>) -> DocPath {
        moved();
        DocPath(path)
    }

    pub fn set(&mut self, path: Option<PathBuf>) {
        moved();
        self.0 = path;
    }

    pub fn take(&mut self) -> Option<PathBuf> {
        moved();
        self.0.take()
    }

    pub fn replace(&mut self, path: Option<PathBuf>) -> Option<PathBuf> {
        moved();
        std::mem::replace(&mut self.0, path)
    }
}

impl std::ops::Deref for DocPath {
    type Target = Option<PathBuf>;
    fn deref(&self) -> &Option<PathBuf> {
        &self.0
    }
}

impl Drop for DocPath {
    fn drop(&mut self) {
        // (a pathless one too: dropped in another's place, the one there may have had a path)
        let _ = MOVED.try_with(|m| m.set(m.get().wrapping_add(1)));
    }
}

impl PartialEq<Option<PathBuf>> for DocPath {
    fn eq(&self, other: &Option<PathBuf>) -> bool {
        self.0 == *other
    }
}

/// The editor's documents (`Editor.docs`): a list read and changed in place as a slice, added to and taken from only
/// through its own methods — so what it laid out by path and id (`Laid`) is known good until one of them, or a path
/// set or a document dropped (`MOVED`).
#[derive(Default)]
pub struct Docs {
    list: Vec<Document>,
    laid: RefCell<Option<Laid>>,
}

/// The documents laid out: by path (`Index`) and by id, as of `MOVED`'s `at`.
struct Laid {
    at: u64,
    by_path: Index,
    by_id: HashMap<DocId, usize>,
}

impl std::ops::Deref for Docs {
    type Target = [Document];
    fn deref(&self) -> &[Document] {
        &self.list
    }
}

impl std::ops::DerefMut for Docs {
    fn deref_mut(&mut self) -> &mut [Document] {
        &mut self.list
    }
}

impl<'a> IntoIterator for &'a Docs {
    type Item = &'a Document;
    type IntoIter = std::slice::Iter<'a, Document>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.iter()
    }
}

impl<'a> IntoIterator for &'a mut Docs {
    type Item = &'a mut Document;
    type IntoIter = std::slice::IterMut<'a, Document>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.iter_mut()
    }
}

impl Docs {
    pub fn of(list: Vec<Document>) -> Docs {
        Docs { list, laid: RefCell::new(None) }
    }

    pub fn push(&mut self, d: Document) {
        moved();
        self.list.push(d);
    }

    pub fn insert(&mut self, at: usize, d: Document) {
        moved();
        self.list.insert(at, d);
    }

    pub fn remove(&mut self, at: usize) -> Document {
        moved();
        self.list.remove(at)
    }

    pub fn retain(&mut self, f: impl FnMut(&Document) -> bool) {
        moved();
        self.list.retain(f);
    }

    /// `f` over what's laid out — laid out anew first if anything moved since. `f` runs inside its borrow: never the
    /// caller's code, which may look a buffer up too (`first`'s `also`).
    fn laid<R>(&self, f: impl FnOnce(&Laid) -> R) -> R {
        let now = MOVED.with(Cell::get);
        let mut laid = self.laid.borrow_mut();
        if laid.as_ref().is_none_or(|l| l.at != now) {
            let mut by_id = HashMap::with_capacity(self.list.len());
            for (i, d) in self.list.iter().enumerate() {
                by_id.entry(d.id).or_insert(i);
            }
            *laid = Some(Laid { at: now, by_path: Index::of(&self.list), by_id });
        }
        f(laid.as_ref().expect("laid out"))
    }

    /// The first document whose path is `path` and that `also` takes: its index.
    pub fn first(&self, path: &Path, also: impl Fn(&Document) -> bool) -> Option<usize> {
        // (the index taken out of what's laid out before `also` runs: it may look a buffer up by path too)
        let i = self.laid(|l| l.by_path.at(path))?;
        first_from(&self.list, i, path, also)
    }

    /// The first document whose path is `path`.
    pub fn by_path(&self, path: &Path) -> Option<&Document> {
        self.first(path, |_| true).map(|i| &self.list[i])
    }

    /// Document `id`'s index (the first, were two to have it).
    pub fn index_of(&self, id: DocId) -> Option<usize> {
        let i = self.laid(|l| l.by_id.get(&id).copied())?;
        // (a document's id is a field: one changed in place is searched for)
        match self.list.get(i).is_some_and(|d| d.id == id) {
            true => Some(i),
            false => self.list.iter().position(|d| d.id == id),
        }
    }

    /// Tests: what's laid out, if still good, is what laying out anew gives — a path or a list changed without telling
    /// `MOVED` would leave a file's buffer unfound (`handle_event`'s check after every event).
    #[cfg(test)]
    #[cfg_attr(tarae_optimized, allow(dead_code))]
    pub fn check_laid(&self) {
        unworked(|| self.check_laid_in());
    }

    #[cfg(test)]
    #[cfg_attr(tarae_optimized, allow(dead_code))]
    fn check_laid_in(&self) {
        let now = MOVED.with(Cell::get);
        let laid = self.laid.borrow();
        let Some(l) = laid.as_ref().filter(|l| l.at == now) else { return };
        let mut first: HashMap<&Path, usize> = HashMap::new();
        for (i, d) in self.list.iter().enumerate() {
            if let Some(p) = d.path.as_deref() {
                first.entry(p).or_insert(i);
            }
        }
        for (p, i) in first {
            let kept = l.by_path.at(p).and_then(|k| first_from(&self.list, k, p, |_| true));
            assert_eq!(
                kept,
                Some(i),
                "{} laid out stale: a path or the list changed without `MOVED`",
                p.display()
            );
        }
        for (i, d) in self.list.iter().enumerate() {
            assert!(l.by_id.get(&d.id).is_some_and(|&k| k <= i), "document {:?} laid out stale", d.id);
        }
    }
}

/// Documents by their path's hash: the first with it. Good while their paths are as they were laid out (`Docs`).
struct Index {
    hash: RandomState,
    by: HashMap<u64, usize>,
}

impl Index {
    fn of(docs: &[Document]) -> Index {
        let hash = RandomState::new();
        let mut by = HashMap::with_capacity(docs.len());
        for (i, d) in docs.iter().enumerate() {
            if let Some(p) = d.path.as_deref() {
                worked(1);
                by.entry(hash.hash_one(p)).or_insert(i);
            }
        }
        Index { hash, by }
    }

    /// The first of the documents laid out whose path hashes as `path`'s: where `first_from` starts. None: no document
    /// has that path — equal paths hash alike.
    fn at(&self, path: &Path) -> Option<usize> {
        worked(1);
        self.by.get(&self.hash.hash_one(path)).copied()
    }
}

/// The first of `docs` (the ones laid out), from `i` (`Index::at`) on, whose path is `path` and that `also` takes.
fn first_from(docs: &[Document], i: usize, path: &Path, also: impl Fn(&Document) -> bool) -> Option<usize> {
    let fits = |d: &Document| same(d.path.as_deref(), path) && also(d);
    // (the first that hashes so is the first with that path — but where two paths' hashes meet, or `also` turns it
    // down: the rest searched)
    match docs.get(i).is_some_and(fits) {
        true => Some(i),
        false => docs.iter().skip(i + 1).position(fits).map(|k| i + 1 + k),
    }
}

#[cfg(test)]
thread_local! {
    /// Tests: the paths compared, hashed, joined or looked at on disk so far by the passes that count them
    /// (`worked`) — the work a budget holds (`docpaths::tests`).
    static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `n` paths compared, hashed, joined or looked at on disk (tests count them — `work`; nothing otherwise).
#[inline]
pub fn worked(n: usize) {
    #[cfg(test)]
    WORK.with(|w| w.set(w.get() + n));
    #[cfg(not(test))]
    let _ = n;
}

/// Tests: `f`, its paths and allocations left out of what's counted (`work`, `allocs::count`) — a test's own check
/// inside what a budget counts.
#[cfg(test)]
#[cfg_attr(tarae_optimized, allow(dead_code))]
pub fn unworked<R>(f: impl FnOnce() -> R) -> R {
    let w = work();
    let r = crate::allocs::uncounted(f);
    WORK.with(|c| c.set(w));
    r
}

/// Tests: the paths compared, hashed or joined so far on this thread.
#[cfg(test)]
pub fn work() -> usize {
    WORK.with(std::cell::Cell::get)
}

/// Whether a document's path is `path` — one compare, counted.
#[inline]
pub fn same(of: Option<&Path>, path: &Path) -> bool {
    worked(1);
    of == Some(path)
}

/// The first of `ed`'s documents whose path is `path`.
pub fn find<'a>(ed: &'a Editor, path: &Path) -> Option<&'a Document> {
    ed.docs.by_path(path)
}

#[cfg(test)]
mod tests;
