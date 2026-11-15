//! Guard 4 (agents beside Claude, step 3): what a project can make an agent load that runs as you — its row's `held`
//! paths (agy's `.agents/`: hooks, MCP servers, plugins and whatever else is there) — is held until you trust it on a
//! card. No flag keeps the agent from loading them, so held back is not started: the chat's start waits for the answer,
//! and what's sent meanwhile waits with it.
//!
//! - Read on guard 1's worker job (`agentgate::ready`): every file under the row's held paths in every folder from the
//!   root up to its git top (`chain`, step 4's L-B2: what agy loads), each by its bytes' SHA-256, the digest over (path
//!   from the top, that hash) in order — a yes is for each file's content, as Claude
//!   settings' is, so any change asks again.
//! - A link is never read through: its target, each hop, is resolved from the link's own folder without reading the
//!   disk (`within`). It's trusted only as a name for a file the listing holds — read and hashed with the rest — and
//!   keyed by that file's path from the root, so re-pointing it asks again and so does changing the file. A link out
//!   of the held paths, to nothing, to a folder, past `HOPS_MAX` hops, or with a `..` anywhere in its target (the OS
//!   takes a `..` after following the link before it, which a lexical resolution can't tell): never trusted, said.
//! - Bounds: `FILE_MAX` a file, `FILES_MAX` files, `DEPTH_MAX` folders deep. Past one, or a file that isn't plain or
//!   can't be read: never trusted — the agent isn't started there, said with the file's name and why (no card: its
//!   only answer would be `n`).
//! - One refusal no answer lifts: a held file naming an MCP server as tarae's (`agentgate::named_in`, the check the
//!   agent's own `config/` gets) — its tools would pass the hook as tarae's.
//! - The answer is kept in `trust.rs`'s store (`Kind::Agent`), at the first held path's place: a repository's
//!   checkouts share it, and a folder's is carried to the repository made there (#517, `trust::carried_rel`). A yes
//!   starts the agent once it's kept; `n` holds it in this project until you quit (never kept, as code trust's).
//! - Alone in step 3: not part of the repository's card, no `:` command (step 4, with the row's on-screen name).

use std::path::{Path, PathBuf};

use crate::agent::Row;
use crate::editor::Editor;
use crate::trust::{Digest, Place, State};

/// The most of one held file read: past it, never trusted.
pub const FILE_MAX: u64 = 1 << 20;
/// The most held files (links counted) read: past it, never trusted.
pub const FILES_MAX: usize = 256;
/// The most folders deep under a held path read: past it, never trusted.
pub const DEPTH_MAX: usize = 8;
/// The most links followed from a held link to the file it names (each hop resolved, never read through): past it,
/// never trusted.
pub const HOPS_MAX: usize = 4;
/// Files on the card at most; `v` shows every one.
pub const SHOWN_MAX: usize = 12;

/// Past `FILES_MAX` or `DEPTH_MAX`, as said.
const TOO_MANY: &str = "more files than tarae reads";

/// A row's held files as read, and the answer for them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    /// Each file from the git top (`Chain.top`), in order, and a link's file: the listed file it names, from the top.
    /// Empty: none there.
    pub files: Vec<(String, Option<String>)>,
    pub hash: Digest,
    pub state: State,
    /// Where the answer is kept: the first held path's.
    pub place: Place,
    /// What the answer is given for (`trust::Write::Answer.file`): the first held path.
    pub file: PathBuf,
}

/// The first held path of `row` under `root`: what its answer is kept for.
fn file_of(row: &Row, root: &Path) -> PathBuf {
    root.join(row.held.first().map_or("", |h| h.trim_end_matches('/')))
}

/// The folders a row's held paths are read in, and what they're keyed from (L-B2). agy loads `.agents/` in every
/// folder from its cwd up to the git top — the nearest holding `.git`, a folder or a worktree's file — and not above it
/// (measured, 1.5.9, #16: a worktree's file is its top); with no git top, the cwd's alone (measured). Found up the
/// root's real path, as agy's cwd is: stats only, no git.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chain {
    /// The git top's real path, or the root's with none: the keys are from it.
    pub top: PathBuf,
    /// Each folder from the top down to the root, from the top (the top itself empty).
    pub folders: Vec<PathBuf>,
    /// The top as the root was written (macOS's `/tmp` for `/private/tmp`), where that names the same folder: a link's
    /// absolute target is taken written either way.
    pub given: Option<PathBuf>,
}

impl Chain {
    /// `held` (a row's) in every folder of the chain, from the top, the top's first.
    pub fn held(&self, held: &[&str]) -> Vec<String> {
        let folders = self.folders.iter();
        let each = folders.flat_map(|f| held.iter().map(move |h| f.join(h.trim_end_matches('/'))));
        each.map(|p| p.display().to_string()).collect()
    }
}

/// `root`'s chain: from its git top down to it. Reads the disk (stats): a worker's.
pub fn chain(root: &Path) -> Chain {
    let real = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let top =
        (real.ancestors()).find(|d| std::fs::metadata(d.join(".git")).is_ok()).unwrap_or(&real).to_path_buf();
    let below = real.strip_prefix(&top).unwrap_or(Path::new(""));
    let mut folders = vec![PathBuf::new()];
    for c in below.components() {
        let next = folders[folders.len() - 1].join(c);
        folders.push(next);
    }
    let up = folders.len() - 1;
    let given = (root.ancestors().nth(up))
        .filter(|g| *g != top && std::fs::canonicalize(g).is_ok_and(|r| r == top))
        .map(Path::to_path_buf);
    Chain { top, folders, given }
}

/// One thing found under a held path: a file's bytes, or a link's target.
enum Found {
    File(String, Vec<u8>),
    Link(String, String),
}

impl Found {
    fn rel(&self) -> &str {
        match self {
            Found::File(r, _) | Found::Link(r, _) => r,
        }
    }
}

/// What's at `path` (`depth` folders under a held path) put in `found`, never followed through a link. Err: a path
/// from the root and why it can't be read.
fn gather(root: &Path, path: &Path, depth: usize, found: &mut Vec<Found>) -> Result<(), (String, String)> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let rel = || path.strip_prefix(root).unwrap_or(path).display().to_string();
    let Ok(meta) = std::fs::symlink_metadata(path) else { return Ok(()) };
    if meta.is_dir() {
        if depth >= DEPTH_MAX {
            return Err((rel(), TOO_MANY.into()));
        }
        let entries = std::fs::read_dir(path).map_err(|e| (rel(), e.to_string()))?;
        let paths = listed(entries.map(|e| e.map(|e| e.path()))).map_err(|e| (rel(), e.to_string()))?;
        return paths.iter().try_for_each(|p| gather(root, p, depth + 1, found));
    }
    if found.len() >= FILES_MAX {
        return Err((rel(), TOO_MANY.into()));
    }
    if meta.file_type().is_symlink() {
        let to = std::fs::read_link(path).map_err(|e| (rel(), e.to_string()))?;
        found.push(Found::Link(rel(), to.display().to_string()));
        return Ok(());
    }
    if !meta.is_file() {
        return Err((rel(), "not a plain file".into()));
    }
    let too_big = || (rel(), "too big to read".to_string());
    if meta.len() > FILE_MAX {
        return Err(too_big());
    }
    // (never through a link put there since, nor held by a FIFO)
    let mut bytes = Vec::new();
    let opened =
        std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path);
    match opened.and_then(|f| f.take(FILE_MAX + 1).read_to_end(&mut bytes)) {
        Err(e) => Err((rel(), e.to_string())),
        Ok(n) if n as u64 > FILE_MAX => Err(too_big()),
        Ok(_) => {
            found.push(Found::File(rel(), bytes));
            Ok(())
        }
    }
}

/// A folder's entries, sorted. Err: the first that couldn't be read — never dropped: a file left unlisted would go
/// unhashed while the agent may still load it (the design session's L4 note 6).
fn listed(entries: impl Iterator<Item = std::io::Result<PathBuf>>) -> std::io::Result<Vec<PathBuf>> {
    let mut paths = entries.collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    Ok(paths)
}

/// `to`, the target of a link in folder `from` (from the root), resolved without reading the disk: its path from the
/// root if that's under one of the `held` paths, else None. An absolute target is taken from any of `roots` (the root
/// as given, its real path: macOS's `/tmp` is `/private/tmp`). A `..` is None too: the caller refuses it first (the OS
/// pops what the component before it resolves to, maybe a link elsewhere — never the name before it).
fn within(roots: &[&Path], held: &[&str], from: &Path, to: &Path) -> Option<String> {
    use std::path::Component;
    let normal = |p: &Path| {
        let mut out = PathBuf::new();
        for c in p.components() {
            match c {
                Component::CurDir => {}
                Component::ParentDir => return None,
                c => out.push(c),
            }
        }
        Some(out)
    };
    let rel = match to.is_absolute() {
        true => {
            let to = normal(to)?;
            roots.iter().find_map(|r| to.strip_prefix(r).ok().map(Path::to_path_buf))?
        }
        false => normal(&from.join(to))?,
    };
    held.iter().any(|h| rel.starts_with(h)).then(|| rel.display().to_string())
}

/// The file each link of `found` (sorted, from `chain`'s top) names: its path from the top, each hop `within` `row`'s
/// held paths in every folder of the chain, the last a file `found` holds (None: not a link). Err: the first link that
/// names anything else, and why.
fn links_resolved(
    row: &Row,
    chain: &Chain,
    found: &[Found],
) -> Result<Vec<Option<String>>, (String, String)> {
    let shown = crate::trust::visible_line;
    let root = chain.top.as_path();
    let roots: Vec<&Path> = std::iter::once(root).chain(chain.given.as_deref()).collect();
    let held = chain.held(row.held);
    let held: Vec<&str> = held.iter().map(String::as_str).collect();
    let at = |rel: &str| found.binary_search_by(|f| f.rel().cmp(rel)).ok().map(|i| &found[i]);
    let resolved = |rel: &String, to: &String| {
        let (mut name, mut to) = (rel.clone(), to.clone());
        for _ in 0..HOPS_MAX {
            let from = Path::new(&name).parent().unwrap_or(Path::new(""));
            // (`sub/../x` is `x` only if `sub` is a folder; through a link it's elsewhere)
            if Path::new(&to).components().any(|c| c == std::path::Component::ParentDir) {
                return Err((name, format!("a link to {}, through ..", shown(&to))));
            }
            let Some(target) = within(&roots, &held, from, Path::new(&to)) else {
                return Err((name, format!("a link to {}, outside {}", shown(&to), row.held.join(", "))));
            };
            match at(&target) {
                Some(Found::File(..)) => return Ok(target),
                Some(Found::Link(_, next)) => (name, to) = (target, next.clone()),
                // (a stat, never a read: what's there decides only the words)
                None if std::fs::symlink_metadata(root.join(&target)).is_ok_and(|m| m.is_dir()) => {
                    return Err((name, format!("a link to {}, a folder", shown(&to))));
                }
                None => return Err((name, format!("a link to {}, which isn't there", shown(&to)))),
            }
        }
        Err((rel.clone(), format!("a chain of more than {HOPS_MAX} links")))
    };
    (found.iter())
        .map(|f| match f {
            Found::Link(rel, to) => resolved(rel, to).map(Some),
            Found::File(..) => Ok(None),
        })
        .collect()
}

/// `row`'s held files in every folder from `root` up to its git top (`chain`), as one listing keyed from the top, and
/// the answer kept for them in `store` (`trust.rs`'s; None: none kept) — at the root's place, as before. Err: why it
/// isn't started there, whatever is answered. Reads the disk: a worker's.
pub fn look(row: &Row, root: &Path, store: Option<&Path>) -> Result<Seen, String> {
    let name = row.name;
    let shown = crate::trust::visible_line;
    let refused = |(rel, why): (String, String)| {
        format!("{}: {why} — never trusted; {name} isn't started here", shown(&rel))
    };
    let chain = chain(root);
    let mut found = Vec::new();
    for h in chain.held(row.held) {
        gather(&chain.top, &chain.top.join(h), 0, &mut found).map_err(refused)?;
    }
    found.sort_by(|a, b| a.rel().cmp(b.rel()));
    let links = links_resolved(row, &chain, &found).map_err(refused)?;
    for f in &found {
        let Found::File(rel, bytes) = f else { continue };
        let json = Path::new(rel).extension().is_some_and(|e| e.eq_ignore_ascii_case("json"));
        if let Some(server) = crate::agentgate::named_in(json, bytes) {
            return Err(format!(
                "{} names an MCP server \"{}\" — its tools would pass for tarae's; {name} isn't started here",
                shown(rel),
                shown(&server)
            ));
        }
    }
    // (a link by the file it names, which is digested by its bytes with the rest)
    let digested: Vec<(String, String)> = (found.iter().zip(&links))
        .map(|(f, file)| match (f, file) {
            (Found::File(rel, bytes), _) => (rel.clone(), crate::sha256::hex(bytes)),
            (Found::Link(rel, _), file) => (rel.clone(), format!("link\0{}", file.as_deref().unwrap_or(""))),
        })
        .collect();
    let hash = crate::trust::hash(&digested);
    let file = file_of(row, root);
    let place = crate::trust::place_of(&file);
    let state = match found.is_empty() {
        true => State::Trusted,
        false => crate::trust::Store::read(store).state_at(&place, hash),
    };
    let files = (found.into_iter().zip(links)).map(|(f, file)| (f.rel().to_string(), file)).collect();
    Ok(Seen { files, hash, state, place, file })
}

/// `look`, a folder's answer carried to its repository first (#517), with its place's key and `storewrite::last()`
/// before the read — what a take-back since is told by.
pub fn look_since(
    row: &Row,
    root: &Path,
    store: Option<&Path>,
) -> (Result<Seen, String>, (String, u64), Vec<crate::trust::Carried>) {
    let place = crate::trust::place_of(&file_of(row, root));
    let carried = crate::trust::carry(store, std::slice::from_ref(&place));
    let at = crate::storewrite::last();
    (look(row, root, store), (crate::trust::key(&place), at), carried)
}

// ── The card ────────────────────────────────────────────────────────────────────────

/// A file's line, as the card and `v` say it.
fn line((rel, to): &(String, Option<String>)) -> String {
    let shown = crate::trust::visible_line;
    match to {
        Some(file) => format!("  {} → {} (a link)", shown(rel), shown(file)),
        None => format!("  {}", shown(rel)),
    }
}

/// The card's text and the lines being decided: that the agent waits for it and why, then each file (`SHOWN_MAX`, the
/// rest counted — `v` shows them).
pub fn question(name: &str, seen: &Seen) -> (String, std::ops::Range<usize>) {
    let mut lines = vec![
        format!(
            "{name} isn't started until you answer — these run with your permissions, outside tarae's sandbox:"
        ),
        String::new(),
    ];
    lines.extend(seen.files.iter().take(SHOWN_MAX).map(line));
    if seen.files.len() > SHOWN_MAX {
        lines.push(format!("  … and {} more — v shows every one", seen.files.len() - SHOWN_MAX));
    }
    let decided = 2..lines.len();
    (lines.join("\n"), decided)
}

/// What `v` adds: every file, and why it asks.
pub fn detail(name: &str, seen: &Seen) -> String {
    let mut lines: Vec<String> = seen.files.iter().map(line).collect();
    lines.push(String::new());
    lines.push(format!(
        "{name} loads these from the project by itself, with no question: a hook runs a command as you on its first \
         tool call, an MCP server or a plugin runs as you while it works. tarae can't hold them back one by one, so \
         {name} waits for your answer. After a yes they run outside tarae's sandbox, which no command of Claude's ever \
         does."
    ));
    lines.join("\n")
}

/// The files' card: alone, never part of the repository's in step 3.
pub fn card(name: &str, seen: &Seen) -> crate::offer::Offer {
    let title = match seen.place {
        Place::Repo { .. } => format!("Trust this project's {name} files?"),
        Place::File(_) => format!("Trust this folder's {name} files?"),
    };
    let (q, decided) = question(name, seen);
    let asked = crate::cards::Asked::new(crate::cards::Why::Own, crate::cards::Request::Own);
    let what = crate::offer::What::AgentFiles { hash: seen.hash };
    let o = crate::offer::Offer::new(title, &q, "", format!("{name} files"), what).deciding(decided);
    o.asked(asked)
}

impl Editor {
    /// The answer on the files' card (`hash` = what it showed). `y`: kept, and the agent starts once it is; `n`: not
    /// started in this project until you quit, never kept.
    pub(crate) fn agent_files_answer(&mut self, id: u64, hash: Digest, trusted: bool) {
        self.offer_finished(id, Ok(()));
        let Some(c) = self.workspace.claude.chat.as_mut() else { return };
        let (name, gate) = (c.agent.name, &mut c.agent_gate);
        let asked = gate.asked.is_some();
        let Some(seen) = gate.files.as_mut().filter(|s| s.hash == hash && asked) else { return };
        if !trusted {
            seen.state = State::Ignored;
            let place = seen.place.clone();
            gate.asked = None;
            if !self.agent_files_held.contains(&place) {
                self.agent_files_held.push(place);
            }
            return self
                .set_status(format!("Held back — {name} isn't started in this project until you quit"));
        }
        seen.state = State::Trusted;
        let (n, place, file, gid) = (seen.files.len(), seen.place.clone(), seen.file.clone(), gate.id);
        self.set_success(format!(
            "Trusted — {name} starts with this project's {}",
            crate::nav::count(n, "file")
        ));
        let Some(path) = self.trust_path.clone() else { return agent_files_kept(self, gid) };
        let (given, seen) = (crate::trust::given(), crate::storewrite::seen_now(&path));
        let kind = crate::trust::Kind::Agent;
        let w = crate::trust::Write::Answer { kind, place: Some(place), file, hash, trusted, given, seen };
        crate::trust::queue(self, path, vec![w], move |ed, r| match r {
            Ok(w) if w.contains(&crate::trust::Wrote::Skipped) => ed.agent_files_skipped(gid, hash),
            Ok(_) => agent_files_kept(ed, gid),
            // (not kept: the yes in hand holds until you quit)
            Err(e) => {
                ed.set_error(format!("couldn't keep the answer: {e}"));
                agent_files_kept(ed, gid)
            }
        });
    }

    /// A yes on the files not written: changed in another tarae before it, unseen — asked again, nothing started.
    fn agent_files_skipped(&mut self, gid: u64, hash: Digest) {
        let Some(c) = self.workspace.claude.chat.as_mut().filter(|c| c.agent_gate.id == gid) else { return };
        let name = c.agent.name;
        let Some(seen) = c.agent_gate.files.as_mut().filter(|s| s.hash == hash) else { return };
        seen.state = State::Ask;
        let card = card(name, seen);
        self.set_warning("Changed in another tarae before your yes was kept — asked again");
        self.trust_offer(card);
    }

    /// `v` on the files' card: its text and `detail` — for what the card shows (`hash`), else its text alone.
    pub(crate) fn agent_files_view(&mut self, hash: Digest) {
        let c = self.workspace.claude.chat.as_ref();
        let seen = c.and_then(|c| Some((c.agent.name, c.agent_gate.files.as_ref()?)));
        let more = seen.filter(|(_, s)| s.hash == hash).map(|(n, s)| detail(n, s)).unwrap_or_default();
        self.offer_view_with(more);
    }
}

/// A yes kept (gate `gid`'s): the agent starts — its files read again, the yes in hand for them.
fn agent_files_kept(ed: &mut Editor, gid: u64) {
    let Some(c) = ed.workspace.claude.chat.as_mut().filter(|c| c.agent_gate.id == gid) else { return };
    c.agent_gate.asked = None;
    crate::chat::gate_opened(ed);
}

#[cfg(test)]
mod tests;
