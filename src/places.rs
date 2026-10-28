//! A file a text names, found one way (#196) — the chat's places (`path:line` in a message: underlined, pressed,
//! `C-]`), `gf` and a run's output (`task::Matcher`):
//!
//! - a name as written: absolute, or from the first of its folders it's a file in — the chat's the project's root then
//!   the workspace's folders, `gf`'s the document's folder then those, a run's its folder and the ones above;
//! - else a name given alone or with only its last folders (`TestDevice.java`, `device/TestDevice.java`), when it
//!   looks like a file: the project's and the folders' files (the file picker's list) that end with it by whole
//!   parts — one is it; several are a list to pick from (the chat's and `gf`'s picker of places; a run's output
//!   no place: a list row opens one file); none, no place.
//!
//! A name that comes to no file isn't drawn as a place (what's underlined opens), nor is it a stop of `C-]` or a place
//! follow goes to (`Known::link`, `follow::answer_place`: the chat's answers are the one truth). A stat and a listing
//! are disk waits: asked on a worker — the chat's after the frame that drew a name it hadn't (`after_frame`), kept
//! while the checkout's list of files can't have changed (`Workspace.listing`: a file made, removed or moved, a run
//! ended, its HEAD moved, a message sent — not a file's text written) and its folders stay.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::chat::ChatRow;
use crate::editor::Editor;

/// Files a name's last parts can stand for, listed at most (a picker's worth).
const SEVERAL_MAX: usize = 50;

/// What a name came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    File(PathBuf),
    /// Its last parts are these files' (by path, `SEVERAL_MAX` at most) — of how many there are.
    Several(Vec<PathBuf>, usize),
    Not,
}

/// The files a name's last parts are looked for in: the roots' (the project, then the folders), listed once and only
/// when asked — never for a name found as written.
pub struct Files {
    roots: Vec<PathBuf>,
    /// The roots' files by their file name, each name's in the listing's order: a name is compared only with the files
    /// its last part names (a run's output asks once a line — a scan of every file each line was a big project's
    /// slow output: #196's review).
    listed: Option<HashMap<OsString, Vec<PathBuf>>>,
    /// Files a name was compared with (tests: the work a run's lines cost, the same on any machine).
    #[cfg(test)]
    compared: usize,
}

impl Files {
    pub fn new(roots: Vec<PathBuf>) -> Files {
        Files {
            roots,
            listed: None,
            #[cfg(test)]
            compared: 0,
        }
    }

    /// Listed already as `paths` (tests: a stand-in for a big project's listing).
    #[cfg(test)]
    pub(crate) fn of(roots: Vec<PathBuf>, paths: Vec<PathBuf>) -> Files {
        Files { listed: Some(by_name(paths)), ..Files::new(roots) }
    }

    /// Files compared with a name so far (tests).
    #[cfg(test)]
    pub(crate) fn compared(&self) -> usize {
        self.compared
    }

    /// None to look in: a name is found as written or not at all.
    pub fn none() -> Files {
        Files::new(Vec::new())
    }

    /// The roots' files were listed (tests: never for a name found as written, nor with no roots to look in).
    #[cfg(test)]
    pub(crate) fn listed(&self) -> bool {
        self.listed.is_some()
    }

    /// The roots' files named `file`, in the listing's order.
    fn named(&mut self, file: &OsStr) -> &[PathBuf] {
        let roots = &self.roots;
        let listed = self.listed.get_or_insert_with(|| {
            by_name(
                roots.iter().flat_map(|r| crate::picker::list_files(r).into_iter().map(move |p| r.join(p))),
            )
        });
        listed.get(file).map_or(&[], Vec::as_slice)
    }
}

/// `paths` by their file name, each name's in their order.
fn by_name(paths: impl IntoIterator<Item = PathBuf>) -> HashMap<OsString, Vec<PathBuf>> {
    let mut by: HashMap<OsString, Vec<PathBuf>> = HashMap::new();
    for p in paths {
        if let Some(name) = p.file_name() {
            by.entry(name.to_os_string()).or_default().push(p);
        }
    }
    by
}

/// Whether `name` looks like a file — a `/` in it, or an extension with a letter (`a.rs`, not `v1.2`).
pub fn file_like(name: &str) -> bool {
    name.contains('/')
        || name
            .rsplit_once('.')
            .is_some_and(|(a, e)| !a.is_empty() && e.chars().any(|c| c.is_ascii_alphabetic()))
}

/// Where `name` is: absolute, itself; else the first of `bases` it's a file in; else, looking like a file, the files of
/// `files` that end with it by whole parts.
pub fn find(name: &str, bases: &[PathBuf], files: &mut Files) -> Found {
    let name = name.trim().trim_start_matches("./");
    let p = Path::new(name);
    if name.is_empty() {
        return Found::Not;
    }
    if p.is_absolute() {
        return if p.is_file() { Found::File(p.to_path_buf()) } else { Found::Not };
    }
    if let Some(f) = bases.iter().map(|b| b.join(p)).find(|f| f.is_file()) {
        return Found::File(f);
    }
    in_files(name, files)
}

/// The files of `files` whose path ends with `name` by whole parts — a name with `..` or `.` in it is from a folder,
/// never a file's last parts.
pub fn in_files(name: &str, files: &mut Files) -> Found {
    let p = Path::new(name);
    let file = match p.file_name() {
        Some(f) if !files.roots.is_empty() && file_like(name) => f,
        _ => return Found::Not,
    };
    if p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Found::Not;
    }
    // (all of them counted — the picker's title says how many — the first `SEVERAL_MAX` kept)
    let (mut hits, mut n) = (Vec::new(), 0);
    let named = files.named(file);
    #[cfg(test)]
    let compared = named.len();
    for f in named.iter().filter(|f| f.ends_with(p)) {
        n += 1;
        if hits.len() < SEVERAL_MAX {
            hits.push(f.clone());
        }
    }
    #[cfg(test)]
    {
        files.compared += compared;
    }
    match n {
        0 => Found::Not,
        1 => Found::File(hits.remove(0)),
        _ => Found::Several(hits, n),
    }
}

/// `name:12` / `name:12:5` (1-based, as compilers print them): the name, and its line and column from 0.
pub fn line_split(s: &str) -> (&str, Option<(usize, usize)>) {
    let num = |s: &str| s.parse::<usize>().ok().filter(|&n| n > 0);
    let Some((head, last)) = s.rsplit_once(':').and_then(|(h, l)| Some((h, num(l)?))) else {
        return (s, None);
    };
    match head.rsplit_once(':').and_then(|(f, l)| Some((f, num(l)?))) {
        Some((file, line)) => (file, Some((line - 1, last - 1))),
        None => (head, Some((last - 1, 0))),
    }
}

/// `path:line` in `text`: (bytes of `path:line`, bytes of the path, the line from 0). The path must look like a file
/// (`file_like`) — URLs (`http://x.com:80`) and times (`12:30`) aren't.
pub fn names_in(text: &str) -> Vec<(Range<usize>, Range<usize>, usize)> {
    let is_path = |c: char| c.is_ascii_alphanumeric() || "_./-~+@".contains(c);
    let b = text.as_bytes();
    let mut out = Vec::new();
    for (i, _) in text.match_indices(':') {
        let digits = text[i + 1..].bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            continue;
        }
        let start = text[..i]
            .rfind(|c: char| !is_path(c))
            .map_or(0, |j| j + text[j..].chars().next().map_or(1, char::len_utf8));
        let path = text[start..i].trim_end_matches('.');
        if path.is_empty()
            || !file_like(path)
            || (start > 0 && b[start - 1] == b':')
            || !path.chars().any(|c| c.is_ascii_alphabetic())
        {
            continue;
        }
        let Ok(line) = text[i + 1..i + 1 + digits].parse::<usize>() else { continue };
        out.push((start..i + 1 + digits, start..start + path.len(), line.saturating_sub(1)));
    }
    out
}

/// A place a message's text names, its part on one row: the bytes of the row's spans it covers, the name as written
/// and its line (from 0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named {
    pub bytes: Range<usize>,
    pub name: Arc<str>,
    pub line: usize,
}

/// The places a message's rows name, each on every row it's drawn on (`ChatRow.places`): found in the message's lines
/// as the transcript has them (`chatdoc::lines_of` — a wrapped row joined to the one before), never row by row, so a
/// name the wrap broke is one place whatever the chat's width. Rows that are a link whole (a tool call's) take none.
pub fn mark_rows(rows: &mut [ChatRow]) {
    if !rows.iter().any(|r| r.link.is_none() && r.spans.iter().any(|s| s.text.contains(':'))) {
        return;
    }
    let (text, at) = crate::chatdoc::lines_of(rows);
    // (each line's places: the bytes of `path:line` on it, the name, the line)
    type Line = Vec<(Range<usize>, Arc<str>, usize)>;
    let named: Vec<Line> = text
        .split('\n')
        .map(|l| {
            names_in(l).into_iter().map(|(whole, path, line)| (whole, Arc::from(&l[path]), line)).collect()
        })
        .collect();
    for (r, a) in rows.iter_mut().zip(&at) {
        let Some(names) = named.get(a.line).filter(|n| !n.is_empty() && r.link.is_none()) else { continue };
        // (the row's text starts after its first `skip` characters — the drawer's — byte for byte the line's after)
        let lead: usize = r.spans.iter().flat_map(|s| s.text.chars()).take(r.skip).map(char::len_utf8).sum();
        for (whole, name, line) in names {
            let (s, e) = (whole.start.max(a.from), whole.end.min(a.from + a.len));
            if s < e {
                let bytes = lead + s - a.from..lead + e - a.from;
                r.places.push(Named { bytes, name: name.clone(), line: *line });
            }
        }
    }
}

/// What the chat's names came to (`Chat.places`): answers for the checkout's files as they were when asked (`key`: its
/// `listing`, its roots), the ones from before its files last could have changed drawn until they're asked again; and
/// the names drawn without an answer (`wanted`), asked after the frame.
#[derive(Debug, Default)]
pub struct Known {
    found: HashMap<Arc<str>, Found>,
    was: HashMap<Arc<str>, Found>,
    key: Option<(u64, Vec<PathBuf>)>,
    generation: u64,
    asking: bool,
    wanted: RefCell<Vec<Arc<str>>>,
}

impl Known {
    /// Names drawn are still being found (asked after the last frame, or being found): what's a link isn't known yet.
    pub fn waiting(&self) -> bool {
        self.asking || !self.wanted.borrow().is_empty()
    }

    /// What `name` came to as far as known — and, not known for the checkout as it is, asked after this frame.
    pub fn of(&self, name: &str) -> Option<&Found> {
        if !self.found.contains_key(name) {
            let mut wanted = self.wanted.borrow_mut();
            if !wanted.iter().any(|w| &**w == name) {
                wanted.push(Arc::from(name));
            }
        }
        self.found.get(name).or_else(|| self.was.get(name))
    }

    /// What the chat takes `name` for — the file it came to, or, a name several files end with, the name as written
    /// from `root` (pressed, found again: `chat::jump`); None where it came to no file, or isn't known yet (asked after
    /// this frame). The one answer for what's underlined (`term::underline_refs`) and `C-]`'s stops (`Chat::hops`).
    pub fn link(&self, root: &Path, name: &str) -> Option<PathBuf> {
        match self.of(name)? {
            Found::File(p) => Some(p.clone()),
            Found::Several(..) => Some(crate::chat::resolve(root, name)),
            Found::Not => None,
        }
    }
}

/// The front chat's names drawn are all answered: none asked after the last frame, none being found (tests: a
/// place is a link once its name came to a file).
#[cfg(test)]
pub fn answered(ed: &Editor) -> bool {
    ed.workspace.claude.chat.as_ref().is_none_or(|c| !c.places.waiting())
}

/// The front chat's answers, kept for the checkout's files as they are, are what each name comes to on disk now (the
/// simulation's check: one found before the files changed never lands as theirs). Answers from before the last change
/// aren't judged until the frame after it asks them again.
#[cfg(test)]
pub(crate) fn check(ed: &Editor) -> Result<(), String> {
    let Some(k) = ed.workspace.claude.chat.as_ref().map(|c| &c.places) else { return Ok(()) };
    let roots: Vec<PathBuf> = ed.workspace.roots().all().map(Path::to_path_buf).collect();
    if k.key.as_ref() != Some(&(ed.workspace.listing, roots.clone())) {
        return Ok(());
    }
    let mut files = Files::new(roots.clone());
    let mut names: Vec<&Arc<str>> = k.found.keys().collect();
    crate::sorting::by_key(&mut names, |n| (*n).clone());
    for name in names {
        let now = find(name, &roots, &mut files);
        if k.found[name] != now {
            return Err(format!("{name} kept as {:?}, the disk says {now:?}", k.found[name]));
        }
    }
    Ok(())
}

/// What the front chat's names came to, as kept for the checkout as it is (the simulation's trace and end).
#[cfg(test)]
pub(crate) fn answers(ed: &Editor) -> Vec<(String, Found)> {
    let Some(k) = ed.workspace.claude.chat.as_ref().map(|c| &c.places) else { return Vec::new() };
    let mut out: Vec<(String, Found)> = k.found.iter().map(|(n, f)| (n.to_string(), f.clone())).collect();
    crate::sorting::by_key(&mut out, |a| a.0.clone());
    out
}

/// A generation no other `Known` has had (a job's answer lands only on the one that asked, as it was).
fn generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// After each frame: the names it drew without an answer, found on a worker — one search at a time; what's drawn
/// meanwhile waits for the next, asked once its answer lands. The checkout's files changed since (another `listing`,
/// other roots): every name answered is asked again, its old answer drawn meanwhile.
pub fn after_frame(ed: &mut Editor) {
    let roots: Vec<PathBuf> = ed.workspace.roots().all().map(Path::to_path_buf).collect();
    let key = (ed.workspace.listing, roots.clone());
    let Some(k) = ed.workspace.claude.chat.as_mut().map(|c| &mut c.places) else { return };
    if k.key.as_ref() != Some(&key) {
        let found = std::mem::take(&mut k.found);
        k.wanted.get_mut().extend(found.keys().cloned());
        k.was.extend(found);
        (k.key, k.generation) = (Some(key), generation());
    }
    if k.asking || k.wanted.get_mut().is_empty() {
        return;
    }
    let mut wanted = std::mem::take(k.wanted.get_mut());
    crate::sorting::by_key(&mut wanted, |n| n.clone());
    wanted.dedup();
    k.asking = true;
    let generation = k.generation;
    ed.events.jobs().spawn(move || {
        let mut files = Files::new(roots.clone());
        let found: Vec<(Arc<str>, Found)> =
            wanted.into_iter().map(|n| (n.clone(), find(&n, &roots, &mut files))).collect();
        move |ed: &mut Editor| landed(ed, generation, found)
    });
}

/// A search's answers: kept when they're for the checkout's files as they are (an answer from before they changed is
/// dropped — asked again); follow's wait on them over (`follow::answer_place`). A frame only where something on screen
/// changes: an answer other than the one drawn, names that wait to be asked, follow moved or held on them.
fn landed(ed: &mut Editor, generation: u64, found: Vec<(Arc<str>, Found)>) {
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    let k = &mut c.places;
    k.asking = false;
    let mut same = true;
    if k.generation == generation {
        same = found.iter().all(|(n, f)| k.found.get(n).or_else(|| k.was.get(n)) == Some(f));
        k.found.extend(found);
    }
    let pending = !k.wanted.get_mut().is_empty();
    // (the chat scrolled while they were found: `follow::reading` acts after the next frame)
    let reading = c.follow.read_at.is_some_and(|r| r != c.scroll);
    let went = crate::follow::answer_place(ed);
    ed.unchanged = same && !pending && !reading && !went;
}

impl Editor {
    /// The list of files in `path`'s checkout may have changed — a file made, removed or moved, a run ended, its HEAD
    /// moved: the chat's names are found again (`places.rs`). Not for a file's text written: a name's answer stays.
    pub(crate) fn files_moved(&mut self, path: &Path) {
        let Some(id) = self.workspace_of(path) else { return };
        for w in std::iter::once(&mut self.workspace).chain(&mut self.workspaces).filter(|w| w.id == id) {
            w.listing += 1;
        }
    }
}

/// A place pressed whose name isn't a file as written (`chat::jump`): found the one way — one file opened at its line,
/// several a picker of them at it, none said.
pub fn open_named(ed: &mut Editor, name: String, line: usize) {
    let roots: Vec<PathBuf> = ed.workspace.roots().all().map(Path::to_path_buf).collect();
    let open: HashMap<PathBuf, ropey::Rope> =
        ed.docs.iter().filter_map(|d| Some((d.path.clone()?, d.text.clone()))).collect();
    ed.events.jobs().spawn(move || {
        let found = find(&name, &roots, &mut Files::new(roots.clone()));
        let lines = match &found {
            Found::Several(all, _) => {
                let at = all.iter().map(|p| (p.clone(), line, 0)).collect();
                crate::lsp_editor::read_places(at, &open, None)
            }
            _ => Vec::new(),
        };
        move |ed: &mut Editor| match found {
            Found::File(p) => crate::chat::open_place(ed, crate::chat::Link { path: p, line }),
            Found::Several(_, n) => several(ed, &format!("{name}:{}", line + 1), n, lines),
            Found::Not => ed.set_status(format!("no file {name} in the project or its folders")),
        }
    });
}

/// A name `n` files end with: the picker of places (the first `SEVERAL_MAX` of them), each at the line — `enter` opens
/// the one meant.
pub fn several(ed: &mut Editor, said: &str, n: usize, lines: Vec<crate::lsp_editor::Place>) {
    let root = ed.workspace.root.clone();
    let listed = lines.len();
    let items = crate::lsp_editor::place_items(&root, lines, &[]);
    let title = match listed < n {
        true => format!("{said} — {n} files end with it, the first {listed} listed"),
        false => format!("{said} — {n} files end with it"),
    };
    let picker = crate::picker::Picker::of("places", title, items, true)
        .grouped(root)
        .in_folders(ed.workspace.roots().folders());
    ed.open_picker(picker, None);
}

#[cfg(test)]
mod tests;
