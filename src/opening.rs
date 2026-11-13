//! Your opens read on a worker (#375, `.sb/openaside-plan.md` §3): a file you go to that isn't open yet is canonicalized,
//! stat'd, read, its grammar loaded and its tree parsed in one job (`read`) — nothing on screen changes until that lands
//! (#355's rule: the code in front stays, never a blank or uncoloured frame), and the document is made then, whole and
//! coloured, in front only if what was in front is still there (`Opening.from`, in the same pane and workspace) — else it
//! joins your buffers, said (`b.rs is ready — space b`). One open at a time: the next replaces the one under way, which
//! lands as nothing. Past `SAY_AFTER` it's said on the status line with its timer (`opening src/b.rs… 1.2s`); no key is
//! held meanwhile. One rule for every size: a big file no longer goes in front empty first.
//!
//! An act of yours that needs files' text, not their view (a project replace, a server's workspace edit, `space u`'s
//! files) reads every file it needs that isn't open in one job first (`read_then`) and acts as they land — never half
//! of it with the other half still being read.
//!
//! A file shown for Claude's turn or a review (`open_shown`: follow, the replay, the review rail, the git tab passing
//! over a file, `C-o` back to a preview) reads the same way in a slot of its own (`Editor.showing`), the next pass
//! taking its place, one read at a time (`show_next`) — a file passed over while one is read is never read — and lands in front only where it was asked: the same
//! pane still showing what it showed, and for follow no key of yours since; else it lands as nothing. It never joins
//! your buffers behind you, nor takes the place of an open of yours (Claude's waits for its next stop; a review's pass
//! is a key of yours, the newer act). Never said: the follow card or the list says what it shows.
//!
//! A few opens of yours that can't wait yet (restore, `:restart`'s, the command line's files, `nav::buffer_for`'s look)
//! and Claude's edit review in the buffer (`refactor::show`, as Claude's) still read on the loop (`Editor::open_as`) —
//! step 4 of the plan. In tests `Editor.read_inline` runs the job right there, as `offers = false` does.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::document::{DocId, Document};
use crate::editor::{Editor, Opener};

/// How long an open runs unsaid: most land well within it, and a step said for a frame would flash too (#355's).
pub const SAY_AFTER: Duration = Duration::from_millis(400);

/// What's done once the file is made (Ok: its document, and whether it's in front) or couldn't be opened (Err: why,
/// nothing made).
pub type Then = Box<dyn FnOnce(&mut Editor, anyhow::Result<(DocId, bool)>) + Send>;

/// A file being opened, not made yet — for you (`Editor.opening`) or shown for Claude or a review (`Editor.showing`):
/// its document to be, what to do once it's in front.
pub struct Opening {
    /// Its document to be — the open's own id: a landing for another is dropped.
    pub id: DocId,
    /// The path as asked (from the root).
    pub path: PathBuf,
    /// What was in front as it was asked: the workspace, the pane, its document.
    workspace: crate::event::WorkspaceId,
    pane: crate::split::PaneId,
    from: DocId,
    /// A jump left behind as it comes in front (`push_jump`).
    jump: bool,
    pub since: Instant,
    /// Past `SAY_AFTER`: said on the status line.
    pub said: bool,
    /// What its `then` is told: for yours, Ok's bool whether it's in front; for a shown one (only ever told in front),
    /// whether it was made now — not open before.
    then: Option<Then>,
    /// Who opens it: you, or the kind a shown one is made (`claudes_only`, `review_only`).
    by: Opener,
    /// A shown one follow asked: your input count then — a key of yours since and it lands as nothing.
    input: Option<u64>,
    /// A shown preview made a buffer of its own as it lands (`keep_showing`: `enter` on a row before it landed).
    keep: bool,
}

/// A file read for an open (`read`): its real path, its stamp taken before the read, its text, its language and the tree
/// parsed from that text (None: no language known; Err: its grammar didn't load — why).
pub struct Read {
    path: PathBuf,
    disk: Option<crate::disk::Stamp>,
    sure: Option<crate::disk::Stamp>,
    text: ropey::Rope,
    lang: Option<&'static crate::syntax::LangSpec>,
    syntax: Option<Result<crate::syntax::Syntax, String>>,
    /// No file there (a missing one reads as an empty text, created on save).
    missing: bool,
}

/// The read, on a worker: the path made real, the stamp before the bytes (a change during the read is caught by the next
/// watch round), the bytes (`read_file`: what isn't a regular file refused before it's opened), then its grammar loaded
/// and its first parse — so the first frame that shows it is coloured.
pub fn read(path: &Path) -> anyhow::Result<Read> {
    #[cfg(test)]
    tests::held(path);
    let path = std::fs::canonicalize(path).or_else(|_| std::path::absolute(path))?;
    let missing = std::fs::symlink_metadata(&path).is_err();
    let (disk, sure) = crate::disk::stamp_sure(&path);
    let text = crate::document::read_file(&path)?;
    let lang = crate::syntax::detect(&path);
    let syntax = lang.map(|spec| {
        crate::syntax::Loader::global().load(spec).map(|l| crate::syntax::Syntax::parsed(l, &text))
    });
    Ok(Read { path, disk, sure, text, lang, syntax, missing })
}

impl Editor {
    /// Opens `path` for you (a relative one from the root), nothing after; what couldn't open said (`open_failed`).
    pub fn open_file(&mut self, path: &Path) {
        self.open_file_then(path, false, |ed, r| {
            if let Err(e) = r {
                ed.open_failed(e)
            }
        })
    }

    /// Opens `path` with the cursor at (line, byte column), a jump left behind and the view centring it where it's out
    /// of sight (`jumped`); what couldn't open said.
    pub fn open_at(&mut self, path: &Path, line: usize, col: usize) {
        self.open_at_then(path, line, col, |_| {})
    }

    /// `open_at`, and `then` once the cursor is there.
    pub fn open_at_then(
        &mut self,
        path: &Path,
        line: usize,
        col: usize,
        then: impl FnOnce(&mut Editor) + Send + 'static,
    ) {
        self.open_file_then(path, true, move |ed, r| match r {
            Ok(_) => {
                crate::editor::place_cursor(ed.doc_mut(), line, col);
                ed.jumped();
                then(ed)
            }
            Err(e) => ed.open_failed(e),
        })
    }

    /// Opens `path` for you: a buffer open at that path already comes in front now; else it's read on a worker and comes
    /// in front as it lands — if what's in front now is still there then —, the open under way before it let go. `jump`:
    /// a jump left behind as it comes. `then` once it's in front (Ok), or with why it couldn't open (nothing made); never
    /// when it landed behind something you went to meanwhile, nor when another open took its place.
    pub fn open_file_then(
        &mut self,
        path: &Path,
        jump: bool,
        then: impl FnOnce(&mut Editor, anyhow::Result<DocId>) + Send + 'static,
    ) {
        self.open_file_then_behind(path, jump, move |ed, r| match r {
            Ok((id, true)) => then(ed, Ok(id)),
            Ok((_, false)) => {}
            Err(e) => then(ed, Err(e)),
        })
    }

    /// `open_file_then` whose `then` comes where it landed behind too (Ok, false: among your buffers, said) — for a
    /// buffer an act is set up in wherever it is (a commit's or a rebase's message: `:w` there goes on).
    pub fn open_file_then_behind(
        &mut self,
        path: &Path,
        jump: bool,
        then: impl FnOnce(&mut Editor, anyhow::Result<(DocId, bool)>) + Send + 'static,
    ) {
        let path = self.path_of(path);
        // (the one under way let go, a shown one too: this one is what you asked for now)
        self.opening = None;
        self.showing = None;
        // (a jump's origin goes with the buffer this open makes — one open already is never moved: #533)
        let reached = self.lsp.reach_next.take();
        if let Some(i) = self.docs.first(&path, |_| true) {
            if jump {
                self.push_jump();
            }
            self.current = i;
            self.workspace_keeps(self.docs[i].id);
            // (opened by you: a preview is yours from now on)
            self.docs[i].preview = false;
            let id = self.docs[i].id;
            return then(self, Ok((id, true)));
        }
        let id = self.alloc_id();
        self.lsp.reaching = reached.map(|r| (id, r));
        self.opening = Some(self.opening_of(id, path.clone(), jump, Box::new(then), Opener::You));
        if self.read_inline {
            let r = read(&path);
            return landed(self, id, r);
        }
        self.events.jobs().spawn(move || {
            let r = read(&path);
            move |ed: &mut Editor| landed(ed, id, r)
        });
        self.say_after(id);
    }

    /// Open `id` said on the status line past `SAY_AFTER`: a file that lands sooner shows nothing but itself.
    fn say_after(&mut self, id: DocId) {
        self.events.shared_jobs().spawn(move || {
            crate::event::sleep(SAY_AFTER);
            move |ed: &mut Editor| match ed.opening.as_mut().filter(|o| o.id == id) {
                Some(o) => o.said = true,
                None => ed.unchanged = true,
            }
        });
    }

    /// Opens for you a file `pick` names, on the worker before the read: given the path as asked (from the root), the
    /// file and a line to put the cursor on (from 0, a jump left behind) — or why it isn't opened (Err: nothing read,
    /// `then` told). A plugin's `open` (#457): which file `path:line` is and whether its grant covers it are the disk's
    /// to say, and so the worker's. Else as `open_file_then`, the path asked not open yet.
    pub(crate) fn open_file_picked(
        &mut self,
        path: &Path,
        pick: impl FnOnce(&Path) -> anyhow::Result<(PathBuf, Option<usize>)> + Send + 'static,
        then: impl FnOnce(&mut Editor, anyhow::Result<DocId>) + Send + 'static,
    ) {
        let path = self.path_of(path);
        self.opening = None;
        self.showing = None;
        let id = self.alloc_id();
        let then = move |ed: &mut Editor, r: anyhow::Result<(DocId, bool)>| match r {
            Ok((id, true)) => then(ed, Ok(id)),
            Ok((_, false)) => {}
            Err(e) => then(ed, Err(e)),
        };
        self.opening = Some(self.opening_of(id, path.clone(), false, Box::new(then), Opener::You));
        let work = move || pick(&path).and_then(|(file, line)| Ok((read(&file)?, file, line)));
        if self.read_inline {
            let r = work();
            return picked_landed(self, id, r);
        }
        self.events.jobs().spawn(move || {
            let r = work();
            move |ed: &mut Editor| picked_landed(ed, id, r)
        });
        self.say_after(id);
    }

    /// Opens `path` shown for Claude's turn or a review (`by`): a buffer open at that path already comes in front now;
    /// else it's read on a worker, the shown one under way before it let go, and comes in front as it lands, made
    /// `claudes_only` or `review_only` (no server offered until you take it up — #208, R7; a review's grammar as it
    /// comes in front, #534) — only where the
    /// pane it was asked in still shows what it showed then, and with `led` (follow's own move) no key of yours came
    /// since; else nothing. `then` once it's in front, with whether it was made now (Ok), or with why it couldn't open.
    /// Claude's asked while an open of yours is under way is nothing: yours is what you asked for.
    pub(crate) fn open_shown(
        &mut self,
        path: &Path,
        by: Opener,
        led: bool,
        then: impl FnOnce(&mut Editor, anyhow::Result<(DocId, bool)>) + Send + 'static,
    ) {
        let path = self.path_of(path);
        self.showing = None;
        if self.opening.as_ref().is_some_and(|o| o.workspace == self.workspace.id) {
            match by {
                Opener::Claude => return,
                _ => self.opening = None,
            }
        }
        if let Some(i) = self.docs.first(&path, |_| true) {
            self.current = i;
            self.workspace_keeps(self.docs[i].id);
            let id = self.docs[i].id;
            return then(self, Ok((id, false)));
        }
        let id = self.alloc_id();
        let mut o = self.opening_of(id, path.clone(), false, Box::new(then), by);
        o.input = led.then_some(self.input_seq);
        self.showing = Some(o);
        if self.read_inline {
            let r = read(&path);
            return shown_landed(self, id, r);
        }
        self.show_next();
    }

    /// The shown open asked last read on a worker — unless one is being read now: one read at a time, so on a slow disk
    /// the files passed over while it reads are never read, only the newest after it (a read under way can't be called
    /// back; its landing is dropped).
    fn show_next(&mut self) {
        if self.show_reading.is_some() {
            return;
        }
        let Some((id, path)) = self.showing.as_ref().map(|o| (o.id, o.path.clone())) else { return };
        self.show_reading = Some(id);
        self.events.jobs().spawn(move || {
            let r = read(&path);
            move |ed: &mut Editor| {
                ed.show_reading = None;
                shown_landed(ed, id, r);
                ed.show_next()
            }
        });
    }

    /// The shown open under way (a preview's) is made a buffer of its own as it lands — `enter` on its row before then.
    pub(crate) fn keep_showing(&mut self) {
        if let Some(o) = self.showing.as_mut() {
            o.keep = true;
        }
    }

    /// An open of `path` asked now, in the pane and workspace in front.
    fn opening_of(&self, id: DocId, path: PathBuf, jump: bool, then: Then, by: Opener) -> Opening {
        Opening {
            id,
            path,
            workspace: self.workspace.id,
            pane: self.workspace.viewer.focus,
            from: self.doc().id,
            jump,
            since: crate::clock::now(),
            said: false,
            then: Some(then),
            by,
            input: None,
            keep: false,
        }
    }

    /// Each of `paths` opened in turn (`:open a b`, `gf` over several), each at its (line, column) where it has one: the
    /// next asked once the one before is in front, so the last ends in front and each passes through it as before; what
    /// couldn't open said, the rest still opened.
    pub fn open_each(&mut self, mut paths: std::collections::VecDeque<(PathBuf, Option<(usize, usize)>)>) {
        let Some((path, at)) = paths.pop_front() else { return };
        self.open_file_then(&path, at.is_some(), move |ed, r| {
            match (r, at) {
                (Ok(_), Some((line, col))) => {
                    crate::editor::place_cursor(ed.doc_mut(), line, col);
                    ed.jumped();
                }
                (Ok(_), None) => {}
                (Err(e), _) => ed.open_failed(e),
            }
            ed.open_each(paths)
        });
    }

    /// Files an act of yours needs the text of (1.B: a replace's, a server's edit's, `space u`'s): each not open yet read
    /// on one worker and made one of your buffers behind what's in front, then `then` with the ones that couldn't be —
    /// missing (`NotFound`) or refused, why. All of them first: the act never runs on half of them.
    pub fn read_then(
        &mut self,
        paths: Vec<PathBuf>,
        then: impl FnOnce(&mut Editor, Vec<(PathBuf, anyhow::Error)>) + Send + 'static,
    ) {
        let paths: Vec<PathBuf> = paths
            .into_iter()
            .map(|p| self.path_of(&p))
            .filter(|p| self.docs.first(p, |_| true).is_none())
            .collect();
        if paths.is_empty() {
            return then(self, Vec::new());
        }
        let work = move || paths.into_iter().map(|p| (read(&p), p)).collect::<Vec<_>>();
        if self.read_inline {
            let r = work();
            return read_landed(self, r, then);
        }
        self.events.jobs().spawn(move || {
            let r = work();
            move |ed: &mut Editor| read_landed(ed, r, then)
        });
    }

    /// The open under way, past `SAY_AFTER`: its path as shown, and since when (the status line's `opening …`).
    pub fn opening_said(&self) -> Option<(String, Instant)> {
        let o = self.opening.as_ref().filter(|o| o.said && o.workspace == self.workspace.id)?;
        Some((self.shown(&o.path).display().to_string(), o.since))
    }

    /// Document `doc` made one of the buffers from a read, opened `by`: in front (`front`, taking the start's blank
    /// scratch's place) or behind what's there, its tree the read's, then all a file opened has (`opened_setup`).
    fn make_read(&mut self, id: DocId, r: Read, front: bool, by: Opener) {
        let mut doc = Document::placeholder(id, &r.path);
        (doc.disk, doc.disk_sure) = (r.disk, r.sure);
        doc.text = r.text;
        doc.loading = false;
        doc.claudes_only = by == Opener::Claude;
        doc.review_only = by == Opener::Review;
        doc.lsp.reached = self.lsp.reaching.take_if(|(at, _)| *at == id).map(|(_, r)| r);
        let unparsed = match r.syntax {
            Some(Ok(syn)) => {
                doc.syntax = Some(syn);
                None
            }
            Some(Err(e)) => r.lang.map(|spec| (spec, e)),
            None => None,
        };
        match front {
            true => self.add_doc(doc),
            false => {
                self.docs.push(doc);
                self.workspace_keeps(id);
            }
        }
        // (its grammar missing: offered as any file asks it — its card and all; held where Claude or a review opened it)
        if let Some((spec, e)) = unparsed {
            self.language_loaded(id, spec, Err(e));
        }
        self.opened_setup(id, &r.path, false);
    }
}

/// The read of a picked open (`open_file_picked`) landed: the open is of the file picked, at its line — then as any.
fn picked_landed(ed: &mut Editor, id: DocId, r: anyhow::Result<(Read, PathBuf, Option<usize>)>) {
    let r = r.map(|(r, file, line)| {
        if let Some(o) = ed.opening.as_mut().filter(|o| o.id == id) {
            o.path = file;
            if let Some(line) = line {
                o.jump = true;
                let then = o.then.take();
                o.then = Some(Box::new(move |ed: &mut Editor, r: anyhow::Result<(DocId, bool)>| {
                    if let Ok((_, true)) = r {
                        crate::editor::place_cursor(ed.doc_mut(), line, 0);
                        ed.jumped();
                    }
                    if let Some(then) = then {
                        then(ed, r)
                    }
                }));
            }
        }
        r
    });
    landed(ed, id, r)
}

/// The read of open `id` landed: dropped where another open took its place; failed, why — nothing made; the file open
/// already by another way since (a preview, Claude's turn) comes in front as that buffer; in another workspace now, not
/// made there, said; else made — in front where what was in front still is (a jump left first, then `then`), else among
/// your buffers, said.
fn landed(ed: &mut Editor, id: DocId, r: anyhow::Result<Read>) {
    let Some(mut o) = ed.opening.take_if(|o| o.id == id) else {
        crate::event::drop_later(r);
        ed.unchanged = true;
        return;
    };
    let then = o.then.take();
    let say = |ed: &mut Editor, r| {
        if let Some(then) = then {
            then(ed, r)
        }
    };
    let r = match r {
        Ok(r) => r,
        Err(e) => return say(ed, Err(e)),
    };
    let name = ed.shown(&o.path).display().to_string();
    if ed.workspace.id != o.workspace {
        crate::event::drop_later(r);
        return ed.set_status(format!("{name} not opened — you're in another workspace now"));
    }
    let here = ed.doc().id == o.from && ed.workspace.viewer.focus == o.pane;
    // (open already by another way since: that buffer, the read let go)
    if let Some(i) = ed.docs.first(&r.path, |_| true) {
        crate::event::drop_later(r);
        let id = ed.docs[i].id;
        ed.workspace_keeps(id);
        ed.docs[i].preview = false;
        if !here {
            ed.set_status(format!("{name} is ready — space b"));
            return say(ed, Ok((id, false)));
        }
        if o.jump {
            ed.push_jump();
        }
        ed.current = i;
        return say(ed, Ok((id, true)));
    }
    if here && o.jump {
        ed.push_jump();
    }
    ed.make_read(id, r, here, Opener::You);
    if !here {
        ed.set_status(format!("{name} is ready — space b"));
    }
    say(ed, Ok((id, here)));
}

/// The read of shown open `id` landed: dropped where another took its place, or where it no longer belongs in front —
/// another workspace, its pane showing something else, a key of yours since follow asked it (nothing said: the next
/// stop or pass shows its own); else in front in its pane — the buffer open by another way since, or made `by` it —,
/// then `then`; why it couldn't open told to `then`.
fn shown_landed(ed: &mut Editor, id: DocId, r: anyhow::Result<Read>) {
    let Some(mut o) = ed.showing.take_if(|o| o.id == id) else {
        crate::event::drop_later(r);
        ed.unchanged = true;
        return;
    };
    let focus = ed.workspace.viewer.focus;
    let shows = match o.pane == focus {
        true => Some(ed.doc().id),
        false => ed.workspace.views.iter().find(|v| v.id == o.pane).map(|v| v.doc),
    };
    let moved = o.input.is_some_and(|n| n != ed.input_seq);
    if ed.workspace.id != o.workspace || shows != Some(o.from) || moved {
        crate::event::drop_later(r);
        ed.unchanged = true;
        return;
    }
    let Some(then) = o.then.take() else { return };
    let r = match r {
        Ok(r) => r,
        Err(e) => return then(ed, Err(e)),
    };
    let (by, keep) = (o.by, o.keep);
    ed.in_pane(o.pane, move |ed| {
        let made = match ed.docs.first(&r.path, |_| true) {
            // (open already by another way since: that buffer, the read let go)
            Some(i) => {
                crate::event::drop_later(r);
                ed.current = i;
                ed.workspace_keeps(ed.docs[i].id);
                false
            }
            None => {
                ed.make_read(id, r, true, by);
                true
            }
        };
        let id = ed.doc().id;
        then(ed, Ok((id, made)));
        if keep {
            ed.preview_keep(id);
        }
    })
}

/// `read_then`'s reads landed: each made a buffer behind what's in front (one open by another way since left as it is),
/// a missing file none; then `then` with the ones that couldn't be.
fn read_landed(
    ed: &mut Editor,
    reads: Vec<(anyhow::Result<Read>, PathBuf)>,
    then: impl FnOnce(&mut Editor, Vec<(PathBuf, anyhow::Error)>),
) {
    let mut failed = Vec::new();
    for (r, asked) in reads {
        match r {
            Ok(r) if r.missing => {
                let e = std::io::Error::from(std::io::ErrorKind::NotFound);
                failed.push((asked, anyhow::Error::from(e)));
            }
            Ok(r) if ed.docs.first(&r.path, |_| true).is_some() => crate::event::drop_later(r),
            Ok(r) => {
                let id = ed.alloc_id();
                ed.make_read(id, r, false, Opener::You);
            }
            Err(e) => failed.push((asked, e)),
        }
    }
    then(ed, failed)
}

#[cfg(test)]
pub(crate) mod tests;
