//! Documents by path (#214): `Editor.docs` searched for a file's buffer, for a stretch of code that only reads the
//! editor — a frame's drawing, the presence gathered at an event. Within `within` the documents can't change (it holds
//! the editor borrowed), so the first lookup lays them out by path's hash once and every other is a hash and one
//! compare away; outside one a lookup is the search it always was. The same document either way: the first with that
//! path, as `docs.iter().find` gives it.
//!
//! Why: a review's rail asks each file's buffer several times a frame and presence each place of each busy session's
//! turn every event — with many sessions open (17, ~300 buffers, a 49-file review) a search a time held the main loop
//! for seconds (#214's sample: most of it in `Path`'s component compares).

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::path::Path;

use crate::document::Document;
use crate::editor::Editor;

/// Documents by their path's hash: the first with it. Good while their paths are as they were laid out — a scope's
/// (`within`), or a pass that changes none (`notes::attach`).
pub struct Index {
    hash: RandomState,
    by: HashMap<u64, usize>,
}

impl Index {
    pub fn of(docs: &[Document]) -> Index {
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

    /// The first of `docs` (the ones laid out) whose path is `path` and that `also` takes.
    pub fn first(&self, docs: &[Document], path: &Path, also: impl Fn(&Document) -> bool) -> Option<usize> {
        worked(1);
        // (no document hashes so: none has that path — equal paths hash alike)
        let i = *self.by.get(&self.hash.hash_one(path))?;
        let fits = |d: &Document| same(d.path.as_deref(), path) && also(d);
        // (the first that hashes so is the first with that path — but where two paths' hashes meet, or `also` turns
        // it down: the rest searched)
        match docs.get(i).is_some_and(fits) {
            true => Some(i),
            false => docs.iter().skip(i + 1).position(fits).map(|k| i + 1 + k),
        }
    }
}

/// A stretch that reads one editor: which one (compared, never followed — another's lookups are searches) and its
/// documents by path — laid out at the first lookup.
struct Scope {
    of: *const Editor,
    by: Option<Index>,
}

thread_local! {
    static SCOPE: RefCell<Option<Scope>> = const { RefCell::new(None) };
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

/// `f` over `ed`, its lookups by path laid out once (`find`). A scope within another (of the same editor or not) is
/// the outer one's.
pub fn within<'a, R>(ed: &'a Editor, f: impl FnOnce(&'a Editor) -> R) -> R {
    struct Out(bool);
    impl Drop for Out {
        fn drop(&mut self) {
            if self.0 {
                SCOPE.with_borrow_mut(|s| *s = None);
            }
        }
    }
    let opened = SCOPE.with_borrow_mut(|s| {
        let open = s.is_none();
        if open {
            *s = Some(Scope { of: ed, by: None });
        }
        open
    });
    let _out = Out(opened);
    f(ed)
}

/// The first of `ed`'s documents whose path is `path`.
pub fn find<'a>(ed: &'a Editor, path: &Path) -> Option<&'a Document> {
    let laid = SCOPE.with_borrow_mut(|s| {
        let s = s.as_mut().filter(|s| std::ptr::eq(s.of, ed))?;
        Some(s.by.get_or_insert_with(|| Index::of(&ed.docs)).first(&ed.docs, path, |_| true))
    });
    match laid {
        Some(i) => i.map(|i| &ed.docs[i]),
        None => ed.docs.iter().find(|d| same(d.path.as_deref(), path)),
    }
}

#[cfg(test)]
mod tests;
