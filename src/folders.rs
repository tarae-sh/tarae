//! Another folder in a session (piece F, frictions29 #186, `.sb/folders-plan.md`): a folder outside the project the
//! workspace works in too — Claude reads and edits there (reviewed) as in the project.
//!
//! - **The workspace's** (`Workspace.folders`), as its root is: a new workspace starts with none; kept with it across
//!   restarts (`worktree::Kept`) until removed; added by you only (`:folder-add`) — Claude asks in words.
//! - **File access, not configuration** (as Claude Code's added directories): a folder's `.tarae.toml` and
//!   `.claude/` are never read; its instruction files are named to Claude in the context, never loaded.
//! - **One lookup**, `Roots::of`: the project, else the folder holding a path — every "inside" check of Claude's
//!   tools that takes a folder goes through it, never a second `starts_with`. `search` and `replace` with no path
//!   walk the project only.
//! - Refused (`judge`), said: the project, in it or holding it; another folder, in one or holding one; home or above;
//!   what runs never read (`Caps.kept`); where a write would run something later (`Caps.later`); another checkout of
//!   the project's repository (that's a workspace).

use std::path::{Path, PathBuf};

use crate::caps::{Caps, Why};
use crate::editor::Editor;

/// The instruction files a folder's line names to Claude when it has them (never loaded).
const INSTRUCTIONS: [&str; 3] = ["CLAUDE.md", "CLAUDE.local.md", "AGENTS.md"];

/// A folder added to a workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// Where it is (a real path).
    pub at: PathBuf,
    /// What it's called (its last part — `-2`, `-3` after it where another folder of the workspace has it).
    pub name: String,
    /// Its instruction files (`INSTRUCTIONS`), as last looked (added, restored) — named to Claude.
    pub instructions: Vec<String>,
}

impl Folder {
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({ "at": self.at, "name": self.name })
    }

    /// One kept (`json`) — judged again, its instruction files looked for again, as it comes back (`folders_back`).
    pub fn from_json(v: &serde_json::Value) -> Option<Folder> {
        let at = PathBuf::from(v["at"].as_str()?);
        Some(Folder { name: v["name"].as_str()?.to_string(), at, instructions: Vec::new() })
    }
}

/// The instruction files in folder `at`.
fn instructions_in(at: &Path) -> Vec<String> {
    INSTRUCTIONS.iter().filter(|n| at.join(n).is_file()).map(|n| n.to_string()).collect()
}

/// The project and its workspace's folders (real paths) — what Claude's tools take as "inside", sent to a worker.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roots {
    project: PathBuf,
    folders: Vec<PathBuf>,
}

impl Roots {
    pub fn new(project: PathBuf, folders: Vec<PathBuf>) -> Roots {
        Roots { project, folders }
    }

    pub fn project(&self) -> &Path {
        &self.project
    }

    pub fn folders(&self) -> &[PathBuf] {
        &self.folders
    }

    /// Every root, the project first.
    pub fn all(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.project.as_path()).chain(self.folders.iter().map(PathBuf::as_path))
    }

    /// The root holding `real` (a real path): the project, else the folder — None outside them all.
    pub fn of(&self, real: &Path) -> Option<&Path> {
        self.all().find(|r| real.starts_with(r))
    }

    /// Where `path` really leads (`paths::inside`: its deepest existing folder resolved) when that's in one of them.
    /// A folder is the real path it was judged as: one that leads elsewhere now (swapped for a link) is no root —
    /// never its link's target taken for it. A worker's (it resolves links).
    pub fn inside(&self, path: &Path) -> Option<Inside> {
        let project = std::fs::canonicalize(&self.project).unwrap_or_else(|_| self.project.clone());
        if let Some(real) = crate::paths::inside(&project, path) {
            return Some(Inside { root: project, real, project: true });
        }
        let real = crate::paths::real(path)?;
        let f = self.folders.iter().find(|f| real.starts_with(f) && itself(f))?;
        Some(Inside { root: f.clone(), real, project: false })
    }

    /// What a refusal says Claude's tools change: the project, or the project and its folders.
    pub fn said(&self) -> &'static str {
        match self.folders.is_empty() {
            true => "inside it only",
            false => "inside it and its folders only",
        }
    }

    /// `said` for `path` (as given): and why, when it's in a folder that isn't the folder added any more. A worker's.
    pub fn said_for(&self, path: &Path) -> String {
        match self.folders.iter().find(|f| path.starts_with(f) && !itself(f)) {
            Some(f) => format!(
                "{} — folder {} isn't the folder added any more (it leads elsewhere now): not read or changed",
                self.said(),
                f.display()
            ),
            None => self.said().to_string(),
        }
    }
}

/// Folder `at` (a real path as judged) is still itself: it resolves to that path, no link in its place. Reads the disk.
fn itself(at: &Path) -> bool {
    std::fs::canonicalize(at).is_ok_and(|r| r == at)
}

/// Where a path is, among the roots (`Roots::inside`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inside {
    /// The root holding it (real).
    pub root: PathBuf,
    /// Where it really leads.
    pub real: PathBuf,
    /// That root is the project (else one of its folders).
    pub project: bool,
}

impl Inside {
    /// `p` (a real path in the same root) as Claude's tools show it: from the project in it, whole in a folder.
    pub fn shown(&self, p: &Path) -> String {
        match self.project {
            true => crate::nav::rel(&self.root, p),
            false => p.display().to_string(),
        }
    }
}

/// `given` as `:folder-add` takes it: `~` your home, a relative path from the project.
fn given_path(project: &Path, given: &str, home: Option<&Path>) -> PathBuf {
    match (given.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        _ => project.join(given),
    }
}

/// Why folder `at` (real) can't be added beside `roots` — None: it can. `writes`: where sandboxed runs may write
/// (`Editor::workspaces_writes`; none when runs aren't sandboxed). A worker's (it reads the disk).
pub fn judge(at: &Path, roots: &Roots, caps: &Caps, writes: &[PathBuf]) -> Option<String> {
    let (project, holds, within) =
        (roots.project(), |p: &Path| p.starts_with(at), |p: &Path| at.starts_with(p));
    let name =
        |p: &Path| p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into());
    if !at.is_dir() {
        return Some("it isn't a folder".into());
    }
    if at == project {
        return Some("that's the project itself".into());
    }
    if within(project) {
        return Some("it's in the project — Claude works there already".into());
    }
    if holds(project) {
        return Some("it holds the project — add a folder beside it".into());
    }
    if roots.folders().iter().any(|f| f == at) {
        return Some("it's added already".into());
    }
    if let Some(f) = roots.folders().iter().find(|f| within(f)) {
        return Some(format!("it's in folder {}", name(f)));
    }
    if let Some(f) = roots.folders().iter().find(|f| holds(f)) {
        return Some(format!("it holds folder {}", name(f)));
    }
    if at.parent().is_none() || caps.home.as_deref().is_some_and(holds) {
        return Some("it's your home or holds it — everything would be Claude's".into());
    }
    if caps.kept.iter().any(|k| within(k) || holds(k)) {
        return Some(
            "it's in or holds what runs never read (keys, tokens, Claude's and tarae's state)".into(),
        );
    }
    if let Some(why) = caps.later.refuses(at) {
        return Some(why);
    }
    // (a run could put a link in its place — `/tmp/infra` → `~/.aws` — or in one of its folders: what it reads would
    // be read as the folder's)
    if let Some(w) = writes.iter().find(|w| within(w) && at != w.as_path()) {
        let w = crate::allow::tilde(w);
        return Some(format!("it's in {w}, where Claude's runs write — a run could put a link in its place"));
    }
    if let Some(w) = writes.iter().find(|w| holds(w) && at != w.as_path()) {
        let w = crate::allow::tilde(w);
        return Some(format!("it holds {w}, where Claude's runs write — a run could put a link there"));
    }
    if caps.defaults(Why::OtherCheckout).any(|c| within(c) || holds(c)) {
        return Some(
            "it's another checkout of this repository — that's a workspace (`:workspace-open`)".into(),
        );
    }
    None
}

/// `name`, or `name-2`, `-3`… — the first no folder of `folders` has.
fn unique_name(name: &str, folders: &[Folder]) -> String {
    let taken = |n: &str| folders.iter().any(|f| f.name == n);
    (1..).map(|i| if i == 1 { name.to_string() } else { format!("{name}-{i}") }).find(|n| !taken(n)).unwrap()
}

impl crate::workspace::Workspace {
    /// Its project and folders.
    pub fn roots(&self) -> Roots {
        Roots::new(self.root.clone(), self.folders.iter().map(|f| f.at.clone()).collect())
    }

    /// A folder of its — added, or kept and still being judged (`folders_judging`) — is `at`, holds it or is in it:
    /// the one check that a folder is in a workspace once, whichever way it came.
    pub fn has_folder(&self, at: &Path) -> bool {
        self.folders
            .iter()
            .chain(&self.folders_judging)
            .any(|f| at.starts_with(&f.at) || f.at.starts_with(at))
    }
}

/// Each of `folders` judged again (`judge`: `caps`, `writes`) beside `project` and the ones before it kept — its
/// real path still the one kept (a link now refused): those kept, their instruction files looked for again; those
/// dropped, each with its words. A worker's.
fn rejudge(
    folders: Vec<Folder>,
    project: &Path,
    caps: &Caps,
    writes: &[PathBuf],
) -> (Vec<Folder>, Vec<(PathBuf, String)>) {
    let (mut kept, mut dropped) = (Vec::new(), Vec::new());
    for mut f in folders {
        let roots = Roots::new(project.to_path_buf(), kept.iter().map(|f: &Folder| f.at.clone()).collect());
        let why = match std::fs::canonicalize(&f.at) {
            Err(e) => Some(e.to_string()),
            Ok(real) => judge(&real, &roots, caps, writes)
                .or_else(|| (real != f.at).then(|| format!("it leads elsewhere now ({})", real.display()))),
        };
        match why {
            Some(why) => {
                dropped.push((f.at.clone(), format!("{} ({}) — {why}", f.name, crate::allow::tilde(&f.at))))
            }
            None => {
                f.instructions = instructions_in(&f.at);
                kept.push(f);
            }
        }
    }
    (kept, dropped)
}

impl Editor {
    /// `:folder-add <path>`: judged on a worker (`judge`) against the front workspace, then that workspace's — in
    /// front or behind by then (`jobs()` swaps it in), never another's — or why not, said.
    pub fn folder_add(&mut self, arg: &str) -> Result<(), String> {
        let given = arg.trim().to_string();
        if given.is_empty() {
            return Err("folder-add needs a folder: `:folder-add ~/work/infra`".into());
        }
        let (basis, writes) = (self.caps_basis(), self.workspaces_writes());
        let roots = self.workspace.roots();
        #[cfg(test)]
        let home = self.caps_home.clone();
        #[cfg(not(test))]
        let home = crate::allow::home();
        let path = given_path(roots.project(), &given, home.as_deref());
        let id = self.workspace.id;
        self.events.jobs().spawn(move || {
            let r = std::fs::canonicalize(&path).map_err(|e| e.to_string()).and_then(|at| {
                match judge(&at, &roots, &basis.build(), &writes().unwrap_or_default()) {
                    Some(why) => Err(why),
                    None => Ok((instructions_in(&at), at)),
                }
            });
            move |ed: &mut Editor| {
                // (a workspace that ended meanwhile: its result lands in the one in front — never added there)
                if ed.workspace.id != id {
                    return ed
                        .set_status(format!("{given} isn't added — the workspace it was for has ended"));
                }
                match r {
                    Ok((instructions, at)) => ed.folder_added(at, instructions),
                    Err(why) => ed.set_error(format!("{given} isn't added — {why}")),
                }
            }
        });
        Ok(())
    }

    /// Folder `at` judged: the workspace in front's now (the one it was judged for), kept, said.
    fn folder_added(&mut self, at: PathBuf, instructions: Vec<String>) {
        let ws = &mut self.workspace;
        // (another added it meanwhile, the same twice, or kept and being judged again as the workspace came back)
        if ws.has_folder(&at) {
            return self.set_status(format!("{} is added already", at.display()));
        }
        let last = at.file_name().map_or_else(|| at.display().to_string(), |n| n.to_string_lossy().into());
        let name = unique_name(&last, &ws.folders);
        ws.folders.push(Folder { at: at.clone(), name: name.clone(), instructions });
        self.folders_changed();
        self.set_success(format!(
            "folder {name} added ({}) — Claude reads and edits there as in the project",
            crate::allow::tilde(&at)
        ));
    }

    /// `:folder-remove [name]`: the folder named (by its name or path) — with none named, the only one.
    pub fn folder_remove(&mut self, arg: &str) -> Result<(), String> {
        let given = arg.trim();
        let folders = &self.workspace.folders;
        let i = match given {
            "" if folders.len() == 1 => 0,
            "" if folders.is_empty() => return Err("no folder is added here".into()),
            "" => return Err(format!("which folder? {}", names(folders))),
            g => folders
                .iter()
                .position(|f| f.name == g || f.at == Path::new(g) || crate::allow::tilde(&f.at) == g)
                .ok_or_else(|| format!("no folder {g} here — {}", names(folders)))?,
        };
        self.folder_remove_at(i);
        Ok(())
    }

    /// The front workspace's `i`th folder removed, said.
    pub(crate) fn folder_remove_at(&mut self, i: usize) {
        if i >= self.workspace.folders.len() {
            return;
        }
        let f = self.workspace.folders.remove(i);
        self.folders_changed();
        self.set_success(format!("folder {} removed — outside the project again for Claude", f.name));
    }

    /// The front workspace's folders changed: kept, and what its Claude may do made again.
    fn folders_changed(&mut self) {
        self.workspaces_keep();
        self.caps_sync();
    }

    /// Workspace `id`'s kept folders as it came back (`folders_judging`), judged again on a worker as `:folder-add`
    /// judges (`judge`, its own `Caps`) — one gone, a link now, or that holds or sits in what it mustn't is dropped and
    /// said; the rest are its folders, their instruction files looked for again.
    pub(crate) fn folders_back(&mut self, id: crate::event::WorkspaceId) {
        let Some(ws) = std::iter::once(&self.workspace).chain(&self.workspaces).find(|l| l.id == id) else {
            return;
        };
        let (basis, project, kept) = (self.caps_basis_in(ws), ws.root.clone(), ws.folders_judging.clone());
        let (front, writes) = (self.workspace.id, self.workspaces_writes());
        self.events_for(id);
        self.events.jobs().spawn(move || {
            let (back, dropped) = rejudge(kept, &project, &basis.build(), &writes().unwrap_or_default());
            move |ed: &mut Editor| {
                if ed.workspace.id != id {
                    return;
                }
                ed.workspace.folders_judging.clear();
                // (one added meanwhile is there once: `has_folder`, as `:folder-add`'s landing asks)
                for f in back {
                    if !ed.workspace.has_folder(&f.at) {
                        ed.workspace.folders.push(f);
                    }
                }
                ed.folders_changed();
                if !dropped.is_empty() {
                    let said: Vec<String> = dropped.into_iter().map(|(_, s)| s).collect();
                    ed.set_warning(format!("folder dropped as it came back: {}", said.join(" · ")));
                }
            }
        });
        self.events_for(front);
    }

    /// The front workspace's folders looked at again as a turn starts (`chat::submit_as`): each judged as
    /// `folders_back` does — one that's a link now, gone, or in or holding what it mustn't is dropped and said (the
    /// tools refuse it meanwhile: `Roots::inside`). Nothing when there are none.
    pub(crate) fn folders_look(&mut self) {
        if self.workspace.folders.is_empty() {
            return;
        }
        let (basis, writes) = (self.caps_basis(), self.workspaces_writes());
        let (project, now) = (self.workspace.root.clone(), self.workspace.folders.clone());
        self.events.jobs().spawn(move || {
            let (kept, dropped) = rejudge(now, &project, &basis.build(), &writes().unwrap_or_default());
            move |ed: &mut Editor| {
                // (their instruction files as they are now)
                for f in ed.workspace.folders.iter_mut() {
                    if let Some(k) = kept.iter().find(|k| k.at == f.at) {
                        f.instructions = k.instructions.clone();
                    }
                }
                if dropped.is_empty() {
                    return;
                }
                ed.workspace.folders.retain(|f| !dropped.iter().any(|(at, _)| *at == f.at));
                ed.folders_changed();
                let said: Vec<String> = dropped.into_iter().map(|(_, s)| s).collect();
                ed.set_warning(format!("folder dropped: {}", said.join(" · ")));
            }
        });
    }
}

// ── `:folders` — a floating list (`floatlist.rs`) ──────────────────────────────────────────────────────

/// A folder's row in `:folders`.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Its id: its place in the list, from 1.
    pub id: u64,
    pub name: String,
    /// Where it is, `~` for your home.
    pub at: String,
    /// Its instruction files, said after it (`has CLAUDE.md`).
    pub has: String,
    /// What its search matches: name, place and instruction files.
    pub text: String,
}

/// The front workspace's folders' rows, in the order they were added.
pub fn rows(ed: &Editor) -> Vec<Row> {
    (ed.workspace.folders.iter().enumerate())
        .map(|(i, f)| {
            let at = crate::allow::tilde(&f.at);
            let has = match f.instructions.is_empty() {
                true => String::new(),
                false => format!("has {}", f.instructions.join(", ")),
            };
            let text = format!("{} {at} {has}", f.name);
            Row { id: i as u64 + 1, name: f.name.clone(), at, has, text }
        })
        .collect()
}

/// The list's keys for its top line.
pub fn hints() -> Vec<(&'static str, &'static str)> {
    vec![("d", "remove"), ("/", "search"), ("esc", "code"), ("q", "close")]
}

impl Editor {
    pub fn folder_ids(&self) -> Vec<u64> {
        (1..=self.workspace.folders.len() as u64).collect()
    }

    /// `:folders`: the list up with the keys — none added, said instead.
    pub fn folders_open(&mut self) {
        if self.workspace.folders.is_empty() {
            return self.set_status("no folder is added here — :folder-add <path> adds one");
        }
        self.float_list_open(crate::pane::GroupKind::Folders, 1, None);
    }

    /// `enter` on the list: nothing to open — said, with what its keys do.
    pub(crate) fn folders_enter(&mut self) {
        let key = crate::key::Key::plain(crate::key::Code::Enter);
        self.key_unused(key, "the folders", "d removes, q closes");
    }

    /// A letter on the list: `d` removes the row's folder (the last gone, the list goes too); the rest are said.
    pub(crate) fn folders_key(&mut self, c: char) {
        use crate::pane::GroupKind;
        match c {
            'd' => {
                let Some(id) = self.float_list_here(GroupKind::Folders) else { return };
                self.folder_remove_at(id as usize - 1);
                if self.workspace.folders.is_empty() {
                    self.float_list_close(GroupKind::Folders);
                }
            }
            _ => self.key_unused(
                crate::key::Key::plain(crate::key::Code::Char(c)),
                "the folders",
                "d removes, / searches, esc code",
            ),
        }
    }
}

/// `a (~/x), b (~/y)` — the folders as a refusal lists them.
fn names(folders: &[Folder]) -> String {
    match folders.is_empty() {
        true => "none is added".into(),
        false => {
            let each: Vec<String> =
                folders.iter().map(|f| format!("{} ({})", f.name, crate::allow::tilde(&f.at))).collect();
            format!("added: {}", each.join(", "))
        }
    }
}

/// The context's line naming the workspace's folders to Claude (none: no line) — its instruction files named,
/// never loaded.
pub fn context_line(folders: &[Folder]) -> Option<String> {
    if folders.is_empty() {
        return None;
    }
    let each: Vec<String> = folders
        .iter()
        .map(|f| match f.instructions.is_empty() {
            true => f.at.display().to_string(),
            false => format!("{} (has {})", f.at.display(), f.instructions.join(", ")),
        })
        .collect();
    Some(format!(
        "folders (read and edit there as in the project; paths absolute; search them by path): {}",
        each.join(", ")
    ))
}

#[cfg(test)]
mod tests;
