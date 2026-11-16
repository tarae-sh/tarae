//! macOS: a folder holding a file named like a key can't be moved or removed by a run (G33). The key rule closes a key
//! by its name below a place, but not below tools' output, the run's own folder or an install — so a folder moved
//! there would take its key out of the rule. A walk before each run (`walk`, as Linux's of the key places, D-i)
//! finds the keys the rule closes below the places runs write (a protected name too: the lock is about reads), and
//! the profile denies `file-write-unlink` on each folder between one and its place's root, as literal parameters (`Policy::profile`, `KD<n>`) — srt's `generateMoveBlockingRules`, which
//! denies the same on a protected path's ancestors. Free: a folder made in this run, even one holding a new key
//! (the walk didn't see it — as Linux leaves a run's own new key until the next run), and a folder holding none.

use std::path::{Path, PathBuf};

use super::Policy;

/// The most entries the walk of the key places reads before a run — Linux's of those outside the project
/// (`linux::outside_keys`, D-i), macOS's of the places runs write (`walk`, G33): past it the run starts, the folders
/// not reached named in its report. Measured (`perf_a_walk_of_the_key_places`, Linux 7.0 on the GCP VM, warm, the
/// `perf` profile): 2.18 µs an entry — 20,011 of them 43.7 ms, about what a run in tier own takes to start (~39 ms);
/// a checkout of tarae (2,314 files, 250 folders) 4.8 ms, 1,000 files 2.8 ms, an empty `/tmp` 14 µs
/// (`.sb/sandbox-d2-walk.md`); macOS's, `perf_a_walk_for_the_key_folders`. Each folder sorted by name before it's
/// queued (ci5: a walk cut short finds the same on every disk) adds 0.04–0.07 µs an entry — +0.8 ms in folders of
/// 99, +1.5 ms of 999 or one of 20,000 (`sorting::tests::perf_a_listing_in_name_order`, this Mac): 2–3 % of the
/// walk. Every run walks afresh (user decision 2026-10-20, request 3's question 4): nothing kept from one run to the
/// next, so a key made anywhere below a place since is found.
pub const WALK_MAX: usize = 20_000;

/// The most folders the profile locks: the lock costs every command's start, and its literals' cost grows with the
/// square of their number. The cap is the largest count whose start adds at most ~20 ms at p90 (the design session,
/// 2026-11-11): measured by `.sb/key-folders-outside3.sh` (`the_lock_starts_by_its_count`, this Mac, 11 starts
/// each, none 13.4 ms at p90) — 50 real-length paths +1.8 ms, 100 +11.6 ms, 150 +26.6 ms, 200 +50.1 ms, 300
/// +118.7 ms, 500 +296 ms. Past it, or past `LOCKS_BYTES`, the keys left aren't locked (`walk`: whole keys, in
/// order), said once a conversation (`lock_said`) and named in the rules (`said`).
pub const LOCKS_MAX: usize = 100;

/// The most bytes of the locked folders' paths, whichever of it and `LOCKS_MAX` comes first: Seatbelt compiles every
/// literal of `file-write-unlink` into one data object of at most 65,535 bytes, however many rules hold them (2,000
/// real-length paths made 80,813 bytes in six rules as in one, and neither started: `.sb/key-folders-outside2.sh`),
/// and the longest paths broke its compiler (`INSTR_JUMP_NE_MAX_LENGTH`). About half of it, so the case with the
/// longest paths starts — 32 of 1,023 bytes, 32,736, started in the third card (+0.4 ms at p90,
/// `the_lock_starts_by_its_count`'s must-start row). A profile must never fail to start because of what a project
/// holds.
pub const LOCKS_BYTES: usize = 32 * 1024;

/// The op of a folder's clone refused (`Policy::profile`'s clone rule): Seatbelt checks `clonefile(2)` — and
/// `fclonefileat`, `clonefileat`, `copyfile`'s clone — on its source, by path (measured,
/// `.sb/key-folders-clone-probe.sh`).
pub const CLONE_OP: &str = "file-clone";

/// What the report says of the folders `cloned` (shown) a run couldn't clone: the place's rule — a folder is
/// cloned only where a run reads everything below it (tools' output in the project, its own folder, an install, a
/// cache it writes); anywhere else its copy could take out what the read rules keep (a key, home). No grant lifts
/// it (the rule is by place, not by what the folder holds); a copy does what's needed, each file read as itself.
pub fn cloned_said(cloned: &[String]) -> String {
    if cloned.is_empty() {
        return String::new();
    }
    const SAID: usize = 3;
    let mut named: Vec<String> = cloned.iter().take(SAID).map(|f| format!("{f}/")).collect();
    if cloned.len() > SAID {
        named.push(format!("{} more", cloned.len() - SAID));
    }
    format!(
        "{}: your commands can't clone a folder (clonefile, cp -c) but below tools' output in the project, their \
         own temp folder, an install or a cache they write — the copy would take out what they can't read. A file's \
         clone works; for a folder, copy it (cp -R, without -c): what they can't read stays where it is. Don't work \
         around it.\n",
        named.join(", ")
    )
}

/// The most entries the walk reads in the shared temp folders (`/private/tmp`, `$TMPDIR` and its siblings), after the
/// places and apart from their `WALK_MAX`: every program's scratch is there (this Mac's `/private/tmp`: 73k entries,
/// most of them Claude Code's scratchpads), so a run would wait ~250 ms for the whole cap. Walked level by level over
/// all of them at once, what's near their tops is always looked through; what's cut is the deep insides of other
/// programs' scratch trees. In entries, not time, so the same on any machine; measured
/// (`perf_a_walk_for_the_key_folders`): the largest of 2,000, 5,000 and 10,000 whose p90 here is under ~40 ms.
pub const TEMP_MAX: usize = 5_000;

/// The walk's caps (`walk`): entries in the places, entries in the shared temp folders, folders locked over both
/// and their paths' bytes.
#[derive(Clone, Copy, Debug)]
pub struct Caps {
    pub entries: usize,
    pub temp: usize,
    pub locks: usize,
    pub bytes: usize,
}

/// A run's (`locked`).
pub const CAPS: Caps = Caps { entries: WALK_MAX, temp: TEMP_MAX, locks: LOCKS_MAX, bytes: LOCKS_BYTES };

/// What the walk before a run found (`walk`): the keys whose folders the profile locks, those folders, and what it
/// didn't look through. Real paths. Empty on Linux (its mounts hold a hidden key's folder: busy) and where no key is
/// closed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyFolders {
    /// The files the key rule closes by name (`Policy::key_writes_closed`) below the places runs write — not one
    /// the user opened (`Grant::Key`), a protected name too — whose folders are locked, in the order found.
    pub keys: Vec<PathBuf>,
    /// The folders locked: each between one of `keys` (in a folder runs don't write, that folder's parent) and its
    /// place's root, that root too (a run could move the
    /// project's root into its own folder: the root's write rule is a `subpath`, which holds the root itself) — the
    /// outermost place's holding it, where one holds another (a grant in a shared temp folder); sorted, once each.
    pub folders: Vec<PathBuf>,
    /// Found, but their folders past `LOCKS_MAX` or `LOCKS_BYTES`: not locked.
    pub unlocked: Vec<PathBuf>,
    /// The folders the walk didn't reach in the places (past `WALK_MAX`), in the order it would have.
    pub unwalked: Vec<PathBuf>,
    /// The folders it didn't reach in the shared temp folders (past `TEMP_MAX`), in the order it would have: the
    /// deepest last. Said to Claude once a conversation (`temp_said`), not in each run's rules.
    pub temp_unwalked: Vec<PathBuf>,
    /// Another filesystem mounted below a place: not walked into.
    pub mounts: Vec<PathBuf>,
    /// The folders it couldn't read, and why — none for one gone, nor one the user may not open (another user's).
    pub failed: Vec<(PathBuf, String)>,
}

impl KeyFolders {
    /// The keys whose lock `folder` (real) is — none: not a folder the walk locked.
    pub fn holding(&self, folder: &Path) -> Vec<&PathBuf> {
        if self.folders.binary_search_by(|f| f.as_path().cmp(folder)).is_err() {
            return Vec::new();
        }
        self.keys.iter().filter(|k| k.starts_with(folder) && k.as_path() != folder).collect()
    }

    /// What the shared temp folders' cut says (`TEMP_MAX`) — the same sentence every run, so said to Claude once a
    /// conversation (`Guard::report_with`, `Told::temp_cut`), not in each run's rules: nothing where they were
    /// walked whole.
    pub fn temp_said(&self) -> String {
        if self.temp_unwalked.is_empty() {
            return String::new();
        }
        format!(
            "The shared temp folders are looked through before a command to their first {} entries, shallow first: \
             deeper there, a folder holding a file named like a key can be moved or removed by a command.",
            crate::features::thousands(TEMP_MAX as u64)
        )
    }

    /// What the lock's cap says (`LOCKS_MAX`, `LOCKS_BYTES`) where keys were past it — said to Claude once a
    /// conversation (`Guard::report_with`, `Told::lock_cut`), with how many keys this run; which they are, the rules
    /// name (`said`). Nothing where every key found was locked.
    pub fn lock_said(&self) -> String {
        if self.unlocked.is_empty() {
            return String::new();
        }
        let n = self.unlocked.len();
        format!(
            "Before a command the folders between each file named like a key and its place are locked, key by key, up \
             to {} folders and {} KB of their paths: past them — {n} {} here — a folder holding one can be moved or \
             removed by a command.",
            crate::features::thousands(LOCKS_MAX as u64),
            LOCKS_BYTES / 1024,
            if n == 1 { "key" } else { "keys" }
        )
    }

    /// What the run's rules say of what the walk didn't lock: the folders past its cap in the places, on another
    /// filesystem or unreadable, the keys past the lock's cap — a few each, then how many more. Nothing where all was
    /// walked and locked: what's locked is said where a move of one is refused (`refused_said`); the shared temp
    /// folders' cut is `temp_said`'s, the lock's cap `lock_said`'s.
    pub fn said(&self) -> String {
        const SAID: usize = 3;
        let names = |v: &[PathBuf], what: &str| {
            let mut named: Vec<String> =
                v.iter().take(SAID).map(|d| format!("{}/", crate::allow::tilde(d))).collect();
            if v.len() > SAID {
                named.push(format!("{} more {what}", v.len() - SAID));
            }
            named.join(", ")
        };
        let moved = "a folder there holding a file named like a key can be moved or removed by a command";
        let mut s = String::new();
        if !self.unwalked.is_empty() {
            s += &format!(
                " The places it writes hold more than the {} entries looked through before a command: {} weren't, \
                 and {moved}.",
                crate::features::thousands(WALK_MAX as u64),
                names(&self.unwalked, "folders")
            );
        }
        if !self.mounts.is_empty() {
            s += &format!(
                " Another filesystem mounted below the places it writes isn't looked through before a command: {} — \
                 {moved}.",
                names(&self.mounts, "")
            );
        }
        for (d, why) in self.failed.iter().take(SAID) {
            s += &format!(
                " {}/ wasn't looked through before a command ({why}): {moved}.",
                crate::allow::tilde(d)
            );
        }
        if self.failed.len() > SAID {
            s += &format!(" Nor were {} more folders.", self.failed.len() - SAID);
        }
        if !self.unlocked.is_empty() {
            let mut keys: Vec<String> =
                self.unlocked.iter().take(SAID).map(|k| crate::allow::tilde(k)).collect();
            if self.unlocked.len() > SAID {
                keys.push(format!("{} more", self.unlocked.len() - SAID));
            }
            s += &format!(" Past the lock's cap: {} — their folders aren't locked.", keys.join(", "));
        }
        s
    }
}

/// What the report says of the folders `refused` (real) a run couldn't move or remove because the walk locked them
/// (`KeyFolders::holding`): each — a few, then how many more — with the keys it holds, and the way: where it holds
/// one, the run's panel opens that file (the next run's walk leaves it out, and the folder is free); where it holds
/// more, opening one wouldn't free it — the user moves it, or the command runs outside. `shown` gives a path as the
/// report does.
pub fn refused_said(found: &KeyFolders, refused: &[PathBuf], shown: impl Fn(&Path) -> String) -> String {
    const SAID: usize = 3;
    let mut s = String::new();
    for f in refused.iter().take(SAID) {
        let keys = found.holding(f);
        let (holds, way) = match keys.as_slice() {
            [] => continue,
            [k] => (
                format!("a file named like a key ({})", shown(k)),
                "the run's panel (space m s) lets your commands use that file, which frees its folder"
                    .to_string(),
            ),
            more => {
                let mut named: Vec<String> = more.iter().take(SAID).map(|k| shown(k)).collect();
                if more.len() > SAID {
                    named.push(format!("{} more", more.len() - SAID));
                }
                (
                    format!("files named like keys ({})", named.join(", ")),
                    format!("they can move it themselves, or {}", super::OUTSIDE_WAY),
                )
            }
        };
        s += &format!(
            "{}/ holds {holds}: your commands can't move or remove it — moved, a key in it could be read where it \
             went. If a command needs to, tell the user: {way}. Don't work around it.\n",
            shown(f)
        );
    }
    if refused.len() > SAID {
        s += &format!("So do {} more folders, for the same reason.\n", refused.len() - SAID);
    }
    s
}

/// Whether a refusal of `op` on a folder may be the lock's: an unlink (a rename from it, `rmdir`) — or, rebuilt
/// from a run's output, its kernel report lost, any write.
pub fn lock_op(op: &str) -> bool {
    op == "file-write-unlink" || op == "file-write*"
}

/// `p` with the folders holding its keys found (`walk`) — macOS's, before each run (`sandbox::policy_from`). The
/// one place a supervised policy (Linux's: its mounts hold a hidden key's folder, busy) is left unwalked: nothing
/// read for a lock it has no use for (`a_supervised_run_reads_no_folder_for_locks`).
pub fn locked(mut p: Policy) -> Policy {
    if p.supervised {
        return p;
    }
    p.key_folders = walk(&p, CAPS);
    p
}

#[cfg(test)]
thread_local! {
    /// The folders and places the walk read on this thread (tests: a supervised run reads none).
    static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// One read of the disk by the walk, counted in tests.
fn read() {
    #[cfg(test)]
    READS.with(|r| r.set(r.get() + 1));
}

/// The files the key rule closes by name below the places runs write (`Policy::key_writes_closed`: not one the user
/// opened, nor in the run's own folder or an install; a protected name too, `.npmrc`, which runs don't write but a
/// move of its folder would take out all the same), and the folders between each and its place's root,
/// at most `caps.locks` folders locked. The places: the project, then the write grants the key rule covers — one
/// after another, each breadth-first, at most `caps.entries` over them all, so a full grant can't stop the walk
/// before the project's keys; then the shared temp folders, as on Linux after the rest, under a cap of their own
/// (`caps.temp`): one queue over them all, level by level, so the top of each is looked through and what's cut is
/// the deep insides of other programs' scratch trees. Each folder in name order, whole or not at all, so a walk cut
/// short finds the same on every disk. As Linux's walk (`linux::outside_keys`): files only, links not followed,
/// into the folders runs don't write too (a protected name, `.git`; a kept folder) — there the first key found is
/// enough, its chain from that folder's parent: the write rules keep anything in or at it from moving —, but nothing
/// below tools' output, `.git/objects` or `.git/lfs` (named by hash), the run's own folder nor an install; no other filesystem entered (named instead), nor a folder it couldn't read. Empty
/// where the profile closes no key (`keys` None; supervised, by the rule itself — `locked` doesn't call it then).
pub fn walk(p: &Policy, caps: Caps) -> KeyFolders {
    walk_in(p, caps, &super::shared_temp)
}

/// `walk`, `shared` telling the shared temp folders (tests: their own, on any system).
fn walk_in(p: &Policy, caps: Caps, shared: &dyn Fn(&Path) -> bool) -> KeyFolders {
    use std::collections::VecDeque;
    use std::os::unix::fs::MetadataExt as _;
    let mut found = KeyFolders::default();
    if p.keys.is_none() {
        return found;
    }
    let run = p.run.as_ref().map(|d| d.path());
    let skipped =
        |d: &Path| run.is_some_and(|r| d.starts_with(r)) || p.key_spared.iter().any(|s| d.starts_with(s));
    let covered = |w: &Path| p.key_places.iter().any(|k| w.starts_with(k));
    let grants = p.write.iter().filter(|w| !w.starts_with(&p.root) && covered(w));
    let (temp, grants): (Vec<&PathBuf>, Vec<&PathBuf>) = grants.partition(|w| shared(w));
    // (one inside a place before it is walked from there; one holding a place before it leaves that one to its own
    // walk: a grant above the project, `/tmp` above a grant in it)
    let fresh = |d: &Path, places: &[&Path]| !skipped(d) && !places.iter().any(|o| d.starts_with(o));
    let mut places: Vec<&Path> = Vec::new();
    for d in std::iter::once(&p.root).chain(grants) {
        if fresh(d, &places) {
            places.push(d);
        }
    }
    // (the places before it, the shared temp folders from `own` on)
    let own = places.len();
    for d in temp {
        if fresh(d, &places) {
            places.push(d);
        }
    }
    // (a place is a real path: a link there now is one swapped in since, not followed)
    let mut devs: Vec<Option<u64>> = Vec::new();
    for place in &places {
        read();
        devs.push(match std::fs::symlink_metadata(place) {
            Ok(m) if m.is_dir() => Some(m.dev()),
            Ok(_) => {
                found.failed.push((place.to_path_buf(), "a link now, not followed".into()));
                None
            }
            Err(e) => {
                found.failed.extend(not_read(place, &e));
                None
            }
        });
    }
    let another = |d: &Path| places.contains(&d);
    let outputs = super::outputs();
    // (each key, its place, and the first folder of its chain: its own, or above the folder runs don't write it's in)
    let mut keys: Vec<(usize, PathBuf, PathBuf)> = Vec::new();
    // (a folder runs don't write — a protected name, `.git`; a kept folder — holding the folder walked: the
    // outermost; and those where a key was found, whose walk ended there)
    let unwritten = |d: &Path| (!p.writes_folder(d)).then(|| d.to_path_buf());
    let mut ended: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    // (breadth-first from `queue`, `left` entries at most: the folders it didn't reach — None, it reached all)
    let mut bfs = |mut queue: VecDeque<(usize, PathBuf, Option<PathBuf>)>,
                   left: &mut usize,
                   found: &mut KeyFolders|
     -> Option<Vec<PathBuf>> {
        while let Some((i, dir, closed)) = queue.pop_front() {
            if closed.as_ref().is_some_and(|c| ended.contains(c)) {
                continue;
            }
            read();
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) => {
                    found.failed.extend(not_read(&dir, &e));
                    continue;
                }
            };
            let Some(listed) = crate::sorting::listing(entries, *left) else {
                queue.push_front((i, dir, closed));
                let left_out = |c: &Option<PathBuf>| c.as_ref().is_some_and(|c| ended.contains(c));
                return Some(queue.into_iter().filter(|(_, _, c)| !left_out(c)).map(|(_, d, _)| d).collect());
            };
            *left = left.saturating_sub(listed.len());
            for (name, e) in listed {
                if closed.as_ref().is_some_and(|c| ended.contains(c)) {
                    break;
                }
                let path = dir.join(&name);
                let Ok(t) = e.file_type() else { continue };
                if t.is_dir() {
                    // (named by hash, no key names: git's objects and LFS's)
                    let hashed =
                        (name == "objects" || name == "lfs") && dir.file_name().is_some_and(|d| d == ".git");
                    let output = outputs.iter().any(|o| name == o.as_str());
                    // (into a folder runs don't write too — a protected name, `.git`: the lock is about reads, and
                    // a key there, `.cargo/credentials`, leaves with a plain folder above it)
                    if hashed || output || skipped(&path) || another(&path) {
                        continue;
                    }
                    // (its own `lstat`, from its folder: another filesystem there is named, never entered)
                    match e.metadata() {
                        Ok(m) if Some(m.dev()) == devs[i] => {
                            let closed = closed.clone().or_else(|| unwritten(&path));
                            queue.push_back((i, path, closed));
                        }
                        Ok(_) => found.mounts.push(path),
                        Err(e) => found.failed.extend(not_read(&path, &e)),
                    }
                } else if t.is_file()
                    && crate::secretnames::is_secret(&name.to_string_lossy())
                    && p.key_writes_closed(&path)
                {
                    // (in a folder runs don't write, one key is enough: the write rules keep anything in or at it
                    // from moving, only the folders above it can — its chain starts at that folder's parent, and
                    // the walk there ends at this key)
                    let below = closed.as_deref().unwrap_or(&path);
                    let from = below.parent().unwrap_or(below).to_path_buf();
                    if let Some(c) = &closed {
                        ended.insert(c.clone());
                    }
                    keys.push((i, path, from));
                }
            }
        }
        None
    };
    let mut left = caps.entries;
    for i in (0..own).filter(|i| devs[*i].is_some()) {
        let start = (i, places[i].to_path_buf(), unwritten(places[i]));
        if let Some(cut) = bfs(VecDeque::from([start]), &mut left, &mut found) {
            let later = places[i + 1..own].iter().map(|d| d.to_path_buf());
            found.unwalked = cut.into_iter().chain(later).collect();
            break;
        }
    }
    let mut temp_left = caps.temp;
    let tops = (own..places.len())
        .filter(|i| devs[*i].is_some())
        .map(|i| (i, places[i].to_path_buf(), unwritten(places[i])));
    if let Some(cut) = bfs(tops.collect(), &mut temp_left, &mut found) {
        found.temp_unwalked = cut;
    }
    // (whole keys, in the order found — the project's nearest its root first, then the grants', then the temp
    // folders' —, each with every folder between it and its place, shared ones counted once: half a chain protects
    // nothing. The first key that doesn't fit and every one after it are left unlocked, so what's locked is the same
    // prefix of the keys on every disk)
    // (up to the outermost place holding the key's own: a grant inside a shared temp folder goes with a folder above
    // it there, which a run writes too)
    let top = |i: usize| -> &Path {
        let holding = places.iter().copied().filter(|o| places[i].starts_with(o));
        holding.min_by_key(|o| o.components().count()).unwrap_or(places[i])
    };
    let mut folders: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let (mut bytes, mut full) = (0, false);
    // (the ways to git's folders are locked first, in the same op's literals — #609, capped on their own at
    // `gitfolders::GIT_LOCKS_MAX` — so the keys get what's left of both caps)
    let gits = p.git.locks();
    let (git_count, git_bytes) = (gits.len(), gits.iter().map(|g| g.as_os_str().len()).sum::<usize>());
    for (i, k, from) in keys {
        if !full {
            let on_the_way = from.ancestors().take_while(|a| a.starts_with(top(i)));
            let new: Vec<&Path> = on_the_way.filter(|a| !folders.contains(*a)).collect();
            let more: usize = new.iter().map(|f| f.as_os_str().len()).sum();
            full =
                git_count + folders.len() + new.len() > caps.locks || git_bytes + bytes + more > caps.bytes;
            if !full {
                bytes += more;
                folders.extend(new.into_iter().map(Path::to_path_buf));
                found.keys.push(k);
                continue;
            }
        }
        found.unlocked.push(k);
    }
    found.folders = folders.into_iter().collect();
    crate::sorting::sort(&mut found.folders);
    found
}

/// A folder the walk couldn't read, and why — none for one gone since it was listed (nothing's there), nor one the
/// user may not open (another user's 0700 folder in `/tmp`): a run is the user with fewer rights, so it can't move
/// that either, and naming them would be every report's noise.
fn not_read(dir: &Path, e: &std::io::Error) -> Option<(PathBuf, String)> {
    match e.raw_os_error() {
        Some(libc::ENOENT | libc::EACCES | libc::EPERM) => None,
        Some(libc::ELOOP | libc::ENOTDIR) => Some((dir.to_path_buf(), "a link now, not followed".into())),
        _ => Some((dir.to_path_buf(), e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::{Denial, Env, Guard};

    /// A project and a folder opened for writing outside it, in the test's own folder (each file's text a fake
    /// token), and the policy of a run there — macOS's rule on any system, no temp folder walked.
    fn made(name: &str, files: &[&str]) -> (crate::scratch::Scratch, Policy) {
        let s = crate::scratch::Scratch::new(name);
        let t = std::fs::canonicalize(s.path()).unwrap();
        for f in files {
            let at = t.join(f);
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::fs::write(&at, "tarae-fake-token").unwrap();
        }
        let granted = t.join("granted");
        std::fs::create_dir_all(&granted).unwrap();
        let mut p = Policy::for_project(&Env { root: t.join("project"), ..Default::default() });
        (p.write, p.key_places, p.supervised) = (vec![granted.clone()], vec![granted], false);
        (s, p)
    }

    /// A key's chain goes up to the outermost place holding its own (G33's review): a write grant inside a shared
    /// temp folder, walked from the grant first, locks the folders between the grant and the temp folder too — a run
    /// writes there, so moving one would take the grant's key along.
    #[test]
    fn a_keys_chain_goes_up_to_the_outermost_place_holding_it() {
        let (s, mut p) = made("keyfolders-outer", &["tmp/a/granted/sub/id_rsa", "tmp/b/notes.txt"]);
        let t = p.root.parent().unwrap().to_path_buf();
        let (tmp, granted) = (t.join("tmp"), t.join("tmp/a/granted"));
        (p.write, p.key_places) = (vec![granted.clone(), tmp.clone()], vec![tmp.clone()]);
        p.keys = Some(Vec::new());
        let found = walk_in(&p, CAPS, &|d| d == tmp);
        assert_eq!(found.keys, [granted.join("sub/id_rsa")]);
        let want: Vec<PathBuf> =
            ["tmp", "tmp/a", "tmp/a/granted", "tmp/a/granted/sub"].map(|f| t.join(f)).into();
        assert_eq!(found.folders, want);
        drop(s);
    }

    /// `walk` with no shared temp folder among the places (the test's own folder may be in one): the same on any
    /// system.
    fn placed(p: &Policy, caps: Caps) -> KeyFolders {
        walk_in(p, caps, &|_| false)
    }

    /// In a folder runs don't write one key is enough, its lock above that folder (G33's review): a `.git` with two
    /// keys deep inside locks only the folders above `.git` — nothing in or at it moves, the write rules refuse it —
    /// and its walk ends at the first key; `.git/lfs` and `.git/objects` (named by hash) aren't entered; a key in a
    /// folder runs write beside it is found and locked as ever.
    #[test]
    fn in_a_folder_runs_dont_write_one_key_locks_the_folders_above_it() {
        let (s, mut p) = made(
            "keyfolders-unwritten",
            &[
                "project/app/.git/a/b/.env",
                "project/app/.git/a/b/server.pem",
                "project/app/.git/a/c/d/id_rsa",
                "project/app/.git/lfs/server.pem",
                // (a plain file: entered, `.git/objects` would be read before the key ends the walk — a read more)
                "project/app/.git/objects/o/x",
                "project/app/conf/.env",
            ],
        );
        let root = p.root.clone();
        p.keys = Some(Vec::new());
        assert!(!p.writes_folder(&root.join("app/.git")) && p.writes_folder(&root.join("app/conf")));
        let reads = || READS.with(|r| r.get());
        let before = reads();
        let found = placed(&p, CAPS);
        // (the two places, then `project`, `app`, `.git`, `conf`, `.git/a`, `.git/a/b` and `granted` — not
        // `.git/a/c`, queued before the key ended `.git`'s walk, nor `.git/lfs` or `.git/objects`)
        assert_eq!(reads() - before, 9, "{found:?}");
        // (breadth-first, by name: `conf`'s key before the deeper one in `.git`, and `.git`'s first ends its walk —
        // the rest of its folder too)
        assert_eq!(found.keys, [root.join("app/conf/.env"), root.join("app/.git/a/b/.env")]);
        assert!(found.unlocked.is_empty(), "{found:?}");
        let want: Vec<PathBuf> = [root.clone(), root.join("app"), root.join("app/conf")].into();
        assert_eq!(found.folders, want);
        drop(s);
    }

    /// The walk before a run finds the keys the rule closes below the places runs write — the project, a write grant;
    /// a protected name too (`.npmrc`), and inside a folder runs don't write (`.yarn/releases`, `.git`): the lock is
    /// about reads; there the folders above it — and locks each folder between one and its place's root, that root
    /// too, once each, in the
    /// profile's parameters and one deny of `file-write-unlink`; not a template, nor below tools' output,
    /// `.git/objects`, a link or a folder holding none; not the file the user opened (`Grant::Key`), whose folder is
    /// free — but a folder another key shares stays locked.
    #[test]
    fn the_walk_locks_each_folder_between_a_key_and_its_place() {
        let (s, mut p) = made(
            "keyfolders-walk",
            &[
                "project/config/.env",
                "project/a/b/c/server.pem",
                "project/plain/notes.txt",
                "project/.env.example",
                "project/target/t/.env",
                "project/.git/x/.env",
                "project/.git/objects/o/.env",
                "project/sub/.npmrc",
                "project/.yarn/releases/k.pem",
                "project/opened/.env",
                "project/twice/.env",
                "project/twice/id_rsa",
                "granted/sub/id_ed25519",
                "elsewhere/in/.env",
            ],
        );
        let (root, t) = (p.root.clone(), p.root.parent().unwrap().to_path_buf());
        std::os::unix::fs::symlink(t.join("elsewhere"), root.join("linked")).unwrap();
        p.keys = Some(vec![root.join("opened/.env")]);
        let found = placed(&p, CAPS);
        let mut keys = found.keys.clone();
        crate::sorting::sort(&mut keys);
        // (sorted: `granted` before `project`)
        let want = [
            "granted/sub/id_ed25519",
            "project/.git/x/.env",
            "project/.yarn/releases/k.pem",
            "project/a/b/c/server.pem",
            "project/config/.env",
            "project/sub/.npmrc",
            "project/twice/.env",
            "project/twice/id_rsa",
        ];
        assert_eq!(keys, want.iter().map(|k| t.join(k)).collect::<Vec<_>>());
        // (`.npmrc`: runs don't write it, yet a move of `sub` would take it out — the rule's own word for it)
        assert!(
            !p.key_refuses_change(&root.join("sub/.npmrc")) && p.key_writes_closed(&root.join("sub/.npmrc"))
        );
        let folders = [
            "granted",
            "granted/sub",
            "project",
            // (`.git` and `.yarn/releases` runs don't write: only the folders above them)
            "project/.yarn",
            "project/a",
            "project/a/b",
            "project/a/b/c",
            "project/config",
            "project/sub",
            "project/twice",
        ];
        let folders: Vec<PathBuf> = folders.iter().map(|f| t.join(f)).collect();
        assert_eq!(found.folders, folders);
        assert!(found.unwalked.is_empty() && found.mounts.is_empty() && found.failed.is_empty(), "{found:?}");
        assert_eq!(found.holding(&root.join("config")), [&root.join("config/.env")]);
        assert_eq!(found.holding(&root.join("twice")).len(), 2);
        assert!(
            found.holding(&root.join("plain")).is_empty() && found.holding(&root.join("opened")).is_empty()
        );
        assert_eq!(found.said(), "", "all walked and locked: nothing said");
        // The profile: each folder a literal parameter, in one deny
        p.key_folders = found;
        let params = p.params();
        let kd: Vec<&str> =
            params.iter().filter(|(k, _)| k.starts_with("KD")).map(|(_, v)| v.as_str()).collect();
        let shown: Vec<String> = folders.iter().map(|f| f.display().to_string()).collect();
        assert_eq!(kd, shown);
        let literals: String = (0..folders.len()).map(|i| format!(" (literal (param \"KD{i}\"))")).collect();
        let profile = p.profile();
        assert!(profile.contains(&format!("(deny file-write-unlink{literals}")), "{profile}");
        // The file the user opened: its folder free — one another key shares stays locked
        p.keys = Some(vec![root.join("config/.env"), root.join("twice/.env")]);
        let opened = placed(&p, CAPS);
        assert!(!opened.folders.contains(&root.join("config")), "{opened:?}");
        assert_eq!(opened.holding(&root.join("twice")), [&root.join("twice/id_rsa")]);
        // Linux's mounts hold a hidden key's folder: none locked, by the rule itself (`locked` doesn't walk then:
        // `a_supervised_run_reads_no_folder_for_locks`)
        p.supervised = true;
        assert_eq!(placed(&p, CAPS), KeyFolders::default());
        drop(s);
    }

    /// Past `max` entries the walk stops before the folder that doesn't fit whole, naming it and every one it
    /// didn't reach — the places after it too — and the report says so; past `locks` folders a key's aren't locked,
    /// said as well. The project's keys first: a full write grant after it can't stop the walk before them.
    #[test]
    fn the_walk_stops_at_its_caps_and_says_what_it_didnt_lock() {
        let files = ["project/a/.env", "project/b/1", "project/b/2", "project/b/3", "project/c/.env"];
        let (s, p) = made("keyfolders-caps", &files);
        let (root, granted) = (p.root.clone(), p.write[0].clone());
        std::fs::write(granted.join("id_rsa"), "tarae-fake-token").unwrap();
        // (the root's three, `a`'s one; `b`'s three don't fit in six)
        let found = placed(&p, Caps { entries: 6, ..CAPS });
        assert_eq!(found.keys, [root.join("a/.env")]);
        assert_eq!(found.folders, [root.clone(), root.join("a")]);
        assert_eq!(found.unwalked, [root.join("b"), root.join("c"), granted.clone()]);
        let said = found.said();
        assert!(said.contains("more than the 20,000 entries looked through before a command"), "{said}");
        assert!(said.contains(&format!("{}/, ", crate::allow::tilde(&root.join("b")))), "{said}");
        assert!(said.contains("can be moved or removed by a command"), "{said}");
        // (two folders: the root and `a`; `c` doesn't fit, nor the grant's two)
        let found = placed(&p, Caps { locks: 2, ..CAPS });
        assert_eq!(found.keys, [root.join("a/.env")]);
        assert_eq!(found.unlocked, [root.join("c/.env"), granted.join("id_rsa")]);
        let (said, cap) = (found.said(), found.lock_said());
        assert!(said.contains(" Past the lock's cap: "), "{said}");
        assert!(said.contains(&crate::allow::tilde(&root.join("c/.env"))), "{said}");
        let up_to = format!(
            "up to {LOCKS_MAX} folders and {} KB of their paths: past them — 2 keys here",
            LOCKS_BYTES / 1024
        );
        assert!(cap.contains(&up_to) && !said.contains("up to"), "{cap}");
        // (by bytes too, whichever comes first: the root's and `a`'s paths fit, `c`'s don't)
        let bytes = root.as_os_str().len() + root.join("a").as_os_str().len();
        let found = placed(&p, Caps { bytes, ..CAPS });
        assert_eq!(found.keys, [root.join("a/.env")]);
        assert_eq!(found.unlocked, [root.join("c/.env"), granted.join("id_rsa")]);
        drop(s);
    }

    /// The shared temp folders are walked after the places, level by level over all of them at once: under their cap
    /// the top of each is looked through — a key at the second's top, one a folder down in the first — and what's
    /// cut is the deep inside of a scratch tree, named.
    #[test]
    fn the_temp_folders_are_walked_shallow_first() {
        let deep = "tmpa/deep/d1/d2/d3/d4/d5/d6/d7/d8/d9/d10/d11/d12/.env";
        let mut files: Vec<String> =
            ["project/config/.env", "tmpa/x/service.pem", "tmpb/id_rsa", deep].map(String::from).to_vec();
        files.extend((0..10).flat_map(|y| (0..5).map(move |f| format!("tmpa/y{y}/f{f}.txt"))));
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        let (s, mut p) = made("keyfolders-shallow", &files);
        let t = p.root.parent().unwrap().to_path_buf();
        let (a, b) = (t.join("tmpa"), t.join("tmpb"));
        (p.write, p.key_places) = (vec![a.clone(), b.clone()], vec![a.clone(), b.clone()]);
        let shared = |d: &Path| d == a || d == b;
        // (the tops: tmpa's 12, tmpb's 1; then `deep`'s 1, `x`'s 1, `y0`'s 5 — 20; `y1` doesn't fit)
        let found = walk_in(&p, Caps { temp: 20, ..CAPS }, &shared);
        let mut keys = found.keys.clone();
        crate::sorting::sort(&mut keys);
        let want = ["project/config/.env", "tmpa/x/service.pem", "tmpb/id_rsa"];
        assert_eq!(keys, want.iter().map(|k| t.join(k)).collect::<Vec<_>>());
        let mut cut: Vec<PathBuf> = (1..10).map(|y| a.join(format!("y{y}"))).collect();
        cut.push(a.join("deep/d1"));
        assert_eq!(found.temp_unwalked, cut);
        assert!(found.unwalked.is_empty(), "{found:?}");
        for f in [&a, &a.join("x"), &b] {
            assert!(found.folders.contains(f), "{f:?}: {found:?}");
        }
        drop(s);
    }

    /// The places keep `WALK_MAX` and their order, the shared temp folders a cap of their own after them: a project
    /// cut short still has the temp folders walked, and their cut stays out of the places'. It's said apart
    /// (`temp_said`), never in the rules (`said`); a lock found there counts toward `LOCKS_MAX`.
    #[test]
    fn the_temp_folders_have_a_cap_of_their_own() {
        let files = [
            "project/a/.env",
            "project/b/1",
            "project/b/2",
            "project/b/3",
            "tmpa/k/.env",
            "tmpa/z/1",
            "tmpa/z/2",
            "tmpa/z/deeper/id_rsa",
        ];
        let (s, mut p) = made("keyfolders-tempcap", &files);
        let (root, granted) = (p.root.clone(), p.write[0].clone());
        let a = root.parent().unwrap().join("tmpa");
        (p.write, p.key_places) = (vec![granted.clone(), a.clone()], vec![granted.clone(), a.clone()]);
        let shared = |d: &Path| d == a;
        // (the project: its root's 2, `a`'s 1 — `b`'s 3 don't fit in 4; the temp folder: its top's 2, `k`'s 1,
        // `z`'s 3 — `deeper` doesn't fit in 6)
        let found = walk_in(&p, Caps { entries: 4, temp: 6, ..CAPS }, &shared);
        assert_eq!(found.keys, [root.join("a/.env"), a.join("k/.env")]);
        assert_eq!(found.unwalked, [root.join("b"), granted.clone()]);
        assert_eq!(found.temp_unwalked, [a.join("z/deeper")]);
        let (said, temp) = (found.said(), found.temp_said());
        assert!(said.contains(&format!("{}/", crate::allow::tilde(&root.join("b")))), "{said}");
        assert!(!said.contains("tmpa") && !said.contains("shallow"), "{said}");
        assert_eq!(
            temp,
            "The shared temp folders are looked through before a command to their first 5,000 entries, shallow \
             first: deeper there, a folder holding a file named like a key can be moved or removed by a command."
        );
        let whole = walk_in(&p, CAPS, &shared);
        assert!(whole.temp_unwalked.is_empty() && whole.temp_said().is_empty(), "{whole:?}");
        // (the project's root and `a` take the two locks: the temp folder's keys aren't locked)
        let found = walk_in(&p, Caps { locks: 2, ..CAPS }, &shared);
        assert_eq!(found.unlocked, [a.join("k/.env"), a.join("z/deeper/id_rsa")]);
        drop(s);
    }

    /// What finding the folders to lock costs each macOS run before it starts (`walk`, G33), p50 and p90 of 11: tarae's
    /// own checkout, as the project; a project of `WALK_MAX` entries — 200 folders of 99 files, a key in one; and the
    /// checkout with the shared temp folders as a run has them (`/private/tmp`, `$TMPDIR`: what this machine's hold,
    /// under `TEMP_MAX`); and the temp folders alone at 2,000, 5,000 and 10,000 entries — `TEMP_MAX`'s measure.
    #[test]
    fn perf_a_walk_for_the_key_folders() {
        if crate::test_results::unoptimized("sandbox::keyfolders::tests::perf_a_walk_for_the_key_folders") {
            return;
        }
        let (s, mut big) = made("keyfolders-perf", &[]);
        for d in 0..200 {
            let dir = big.root.join(format!("d{}/s{}", d / 20, d % 20));
            std::fs::create_dir_all(&dir).unwrap();
            for f in 0..99 {
                std::fs::write(dir.join(format!("f{f}.txt")), "").unwrap();
            }
        }
        std::fs::write(big.root.join("d3/s4/.env"), "tarae-fake-token").unwrap();
        big.write = Vec::new();
        let checkout = std::fs::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap();
        let mut own = Policy::for_project(&Env { root: checkout, ..Default::default() });
        own.supervised = false;
        let with_temp = own.clone();
        own.write = Vec::new();
        // (the temp folders alone: a project that isn't there, the checkout's shared temp folders its only grants)
        let (s2, mut temp) = made("keyfolders-perf-temp", &[]);
        temp.write = with_temp.write.iter().filter(|w| super::super::shared_temp(w)).cloned().collect();
        temp.key_places = with_temp.key_places.clone();
        let time = |p: &Policy, caps: Caps| {
            let mut ms: Vec<f64> = (0..11)
                .map(|_| {
                    let at = std::time::Instant::now();
                    std::hint::black_box(walk(p, caps));
                    at.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            ms.sort_by(f64::total_cmp);
            (ms[5], ms[9])
        };
        let mut out = format!("the shared temp folders: {:?}\n", temp.write);
        let at = |n: usize| Caps { temp: n, ..CAPS };
        for (what, p, caps) in [
            ("tarae's checkout".to_string(), &own, CAPS),
            ("20,011 entries".to_string(), &big, CAPS),
            ("the checkout and the temp folders".to_string(), &with_temp, CAPS),
            ("the temp folders alone, 2,000".to_string(), &temp, at(2_000)),
            ("the temp folders alone, 5,000".to_string(), &temp, at(5_000)),
            ("the temp folders alone, 10,000".to_string(), &temp, at(10_000)),
        ] {
            let found = walk(p, caps);
            let (p50, p90) = time(p, caps);
            out += &format!(
                "the walk for the key folders, {what}: p50 {p50:.2} ms, p90 {p90:.2} ms — {} keys, {} folders locked, \
                 {} folders not reached, {} in the temp folders\n",
                found.keys.len(),
                found.folders.len(),
                found.unwalked.len(),
                found.temp_unwalked.len()
            );
        }
        let _ = std::io::Write::write_all(&mut std::io::stderr(), out.as_bytes());
        drop((s, s2));
    }

    /// The one place a supervised policy is left unwalked is `locked`, the run's call: it reads no folder for a
    /// lock it has no use for (Linux's mounts hold a hidden key's folder) — counted in reads, not time. The same
    /// project unsupervised is read and its folders locked.
    #[test]
    fn a_supervised_run_reads_no_folder_for_locks() {
        let (s, mut p) = made("keyfolders-supervised", &["project/config/.env", "granted/sub/id_rsa"]);
        p.keys = Some(Vec::new());
        let reads = || READS.with(|r| r.get());
        p.supervised = true;
        let before = reads();
        let q = locked(p.clone());
        assert_eq!(
            (reads() - before, &q.key_folders),
            (0, &KeyFolders::default()),
            "supervised: nothing read"
        );
        p.supervised = false;
        let before = reads();
        let q = locked(p);
        assert!(reads() > before && q.key_folders.folders.contains(&q.root), "{:?}", q.key_folders);
        drop(s);
    }

    /// Under a cap the keys are taken whole and in order: the first whose folders don't fit, and every key after it,
    /// are left unlocked — even one that would fit (half a chain protects nothing; what's locked is the same prefix
    /// of the keys on every disk). Folders shared with a key already taken cost nothing.
    #[test]
    fn the_lock_takes_whole_keys_in_order() {
        let files = ["project/a/.env", "project/a/b/id_rsa", "project/c/d/.env"];
        let (s, p) = made("keyfolders-whole", &files);
        let (root, granted) = (p.root.clone(), p.write[0].clone());
        std::fs::write(granted.join("id_rsa"), "tarae-fake-token").unwrap();
        // (the root and `a` for a's key, `a/b` for the next — `a` shared; `c/d`'s key needs two more, past four; the
        // grant's one folder would fit, but it comes after)
        let found = placed(&p, Caps { locks: 4, ..CAPS });
        assert_eq!(found.keys, [root.join("a/.env"), root.join("a/b/id_rsa")]);
        assert_eq!(found.folders, [root.clone(), root.join("a"), root.join("a/b")]);
        assert_eq!(found.unlocked, [root.join("c/d/.env"), granted.join("id_rsa")]);
        // (all of them, under the run's caps)
        let all = placed(&p, CAPS);
        assert!(all.unlocked.is_empty() && all.keys.len() == 4 && all.folders.len() == 6, "{all:?}");
        drop(s);
    }

    /// The lock is one rule — Seatbelt makes one data object of the op's literals however many rules hold them —
    /// within both caps: `LOCKS_MAX` folders of a real path's length, and `LOCKS_BYTES` of the longest paths the file
    /// system takes (1,023 bytes). Each folder once, in order. Whether the profile starts in time is the outside
    /// card's (`.sb/key-folders-outside3.sh`, `the_lock_starts_by_its_count`).
    #[test]
    fn the_lock_is_one_rule_within_both_caps() {
        let mut p = Policy::for_project(&Env { root: "/private/tmp/p".into(), ..Default::default() });
        (p.keys, p.supervised) = (Some(Vec::new()), false);
        let real = |i: usize| {
            PathBuf::from(format!(
                "/Users/someone/Workspace/project/src/a/folder-number-{i:04}-with-a-real-name"
            ))
        };
        let longest = |i: usize| {
            let mut s = format!("/private/tmp/p/{i:04}");
            while s.len() < 1023 {
                s.push_str(&format!("/{}", "x".repeat(200.min(1022 - s.len()))));
            }
            PathBuf::from(s)
        };
        let at_count: Vec<PathBuf> = (0..LOCKS_MAX).map(real).collect();
        let at_bytes: Vec<PathBuf> = (0..LOCKS_BYTES / 1024).map(longest).collect();
        assert!(at_bytes.iter().all(|f| f.as_os_str().len() == 1023));
        for (what, folders) in [("LOCKS_MAX", at_count), ("LOCKS_BYTES", at_bytes)] {
            let total: usize = folders.iter().map(|f| f.as_os_str().len()).sum();
            assert!(folders.len() <= LOCKS_MAX && total <= LOCKS_BYTES, "{what}: within the caps");
            p.key_folders = KeyFolders { folders: folders.clone(), ..Default::default() };
            let params: std::collections::HashMap<String, String> = p.params().into_iter().collect();
            let profile = p.profile();
            let rules: Vec<Vec<usize>> = profile
                .lines()
                .filter(|l| l.starts_with("(deny file-write-unlink"))
                .map(|l| {
                    l.split("(param \"KD")
                        .skip(1)
                        .map(|s| s[..s.find('"').unwrap()].parse().unwrap())
                        .collect()
                })
                .collect();
            assert_eq!(rules.len(), 1, "{what}: one rule");
            let bytes: usize = rules[0].iter().map(|i| params[&format!("KD{i}")].len()).sum();
            assert!(bytes <= LOCKS_BYTES, "{what}: {bytes} bytes");
            assert_eq!(
                rules[0],
                (0..folders.len()).collect::<Vec<_>>(),
                "{what}: each folder once, in order"
            );
        }
    }

    /// A folder is cloned only where a run reads everything below it (G33's review, G35): the profile denies
    /// `file-clone` on every folder, then allows it below tools' output in the project, the run's own folder, an
    /// install and a write folder no place of the key rule holds or is held by (a cache) — not a granted folder
    /// where keys are closed, and in a cache not a folder holding a kept place, nor that place. A refused clone is said
    /// by its place, first (a folder named like a key or a protected one too), outside the project too, and the
    /// panel offers nothing: no grant lifts it.
    #[test]
    fn a_folders_clone_is_refused_by_place_and_said_so() {
        use crate::allow::Allowed;
        let (s, mut p) =
            made("keyfolders-clone", &["project/config/.env", "project/plain/x", "cache/a/kept/k"]);
        let cache = p.root.parent().unwrap().join("cache");
        p.write.push(cache.clone());
        p.reads.kept = vec![cache.join("a/kept")];
        p.keys = Some(Vec::new());
        p.key_folders = placed(&p, CAPS);
        let profile = p.profile();
        let at = crate::sandbox::below(&p.root);
        assert!(profile.contains("(deny file-clone (vnode-type DIRECTORY))"), "{profile}");
        // (the granted folder W0 is a place of the key rule: not free; the cache W1 is, but its kept place)
        let allow = format!(
            "(allow file-clone (require-all (vnode-type DIRECTORY) (require-any (require-all (subpath (param \
             \"ROOT\")) {}) (subpath (param \"W1\"))) (require-not (require-any (literal (param \"CA0\")) (literal \
             (param \"CA1\")) (subpath (param \"CH0\"))))))",
            crate::sandbox::key_free_folders(&at, false)
        );
        assert!(profile.contains(&allow), "{profile}");
        assert_eq!(p.clone_held(), (vec![cache.clone(), cache.join("a")], vec![cache.join("a/kept")]));
        // (after the deny, and nothing allows a folder's clone later: the later rule wins)
        let (deny_at, allow_at) =
            (profile.find("(deny file-clone").unwrap(), profile.find("(allow file-clone").unwrap());
        assert!(deny_at < allow_at && profile.matches("file-clone").count() == 2, "{profile}");
        // (tools' output: the folder itself and below it)
        let free = crate::sandbox::key_free_folders(&at, false);
        assert!(free.contains("target") && free.ends_with(")(/|$)\")"), "{free}");
        let none = Allowed::default();
        let caps = crate::caps::Caps::granted(&p.root, &none, &none, &none);
        let rules = p.key_rules();
        let cloned = |f: &str| Denial {
            process: "cp".into(),
            op: CLONE_OP.into(),
            target: if f.is_empty() {
                p.root.display().to_string()
            } else {
                p.root.join(f).display().to_string()
            },
        };
        let said = |d: Denial| Guard::In(Box::new(p.clone())).kept(&[d]).0;
        for f in ["plain", "config", ".env.d", ".git"] {
            let s = said(cloned(f));
            assert!(
                s.contains(&format!(
                    "{f}/: your commands can't clone a folder (clonefile, cp -c) but below tools' output in the \
                     project"
                )),
                "{f}: {s}"
            );
            assert!(!s.contains("move or remove") && !s.contains("named like a key, so"), "{f}: {s}");
            let panel = crate::blocked::choices_by(&[cloned(f)], &caps, Some(&rules));
            assert!(
                panel.is_empty(),
                "{f}: no grant lifts it: {:?}",
                panel.iter().map(|c| &c.grant).collect::<Vec<_>>()
            );
        }
        // (the root itself: by its path)
        let top = said(cloned(""));
        assert!(
            top.contains(&format!("{}/: your commands can't clone a folder", crate::allow::tilde(&p.root))),
            "{top}"
        );
        assert_eq!(cloned("plain").plain(), format!("cloning {}", cloned("plain").target_shown()));
        // (outside the project — the folder above home — the same words, by its path)
        let above = Denial { process: "cp".into(), op: CLONE_OP.into(), target: "/Users".into() };
        assert!(said(above).contains("/Users/: your commands can't clone a folder"));
        drop(s);
    }

    /// A folder the walk locked, its move or removal refused: the report says it holds a key and why it can't move,
    /// and the panel offers that key — where it's the only one, its opening frees the folder; holding more, none
    /// (one opened alone wouldn't), nor a folder (a grant of one doesn't lift the lock). A folder not locked, or
    /// another refusal of a locked one, is the place's as before.
    #[test]
    fn a_locked_folders_refusal_is_said_and_offers_its_one_key() {
        use crate::allow::{Allowed, Grant};
        let files = ["project/config/.env", "project/twice/.env", "project/twice/id_rsa", "project/plain/x"];
        let (s, mut p) = made("keyfolders-said", &files);
        let root = p.root.clone();
        p.key_folders = placed(&p, CAPS);
        let rules = p.key_rules();
        let none = Allowed::default();
        let caps = crate::caps::Caps::granted(&root, &none, &none, &none);
        let refused = |op: &str, f: &str| Denial {
            process: "mv".into(),
            op: op.into(),
            target: if f.is_empty() {
                root.display().to_string()
            } else {
                root.join(f).display().to_string()
            },
        };
        let said = |d: Denial| Guard::In(Box::new(p.clone())).kept(&[d]).0;
        let panel = |d: Denial| -> Vec<Grant> {
            crate::blocked::choices_by(&[d], &caps, Some(&rules)).into_iter().map(|c| c.grant).collect()
        };
        let one = said(refused("file-write-unlink", "config"));
        assert!(
            one.contains(
                "config/ holds a file named like a key (config/.env): your commands can't move or remove it — \
                 moved, a key in it could be read where it went. If a command needs to, tell the user: the run's \
                 panel (space m s) lets your commands use that file, which frees its folder."
            ),
            "{one}"
        );
        assert_eq!(panel(refused("file-write-unlink", "config")), [Grant::Key(root.join("config/.env"))]);
        // (rebuilt from the output, its kernel report lost: where runs write, only the lock refuses a folder's move)
        assert_eq!(panel(refused("file-write*", "config")), [Grant::Key(root.join("config/.env"))]);
        let two = said(refused("file-write-unlink", "twice"));
        assert!(
            two.contains(
                "twice/ holds files named like keys (twice/.env, twice/id_rsa): your commands can't move"
            ),
            "{two}"
        );
        assert!(two.contains("they can move it themselves, or ask to run it outside the sandbox"), "{two}");
        assert_eq!(panel(refused("file-write-unlink", "twice")), []);
        // (the project's root, locked too: said by its path)
        let top = said(refused("file-write-unlink", ""));
        assert!(
            top.contains(&format!("{}/ holds files named like keys", crate::allow::tilde(&root))),
            "{top}"
        );
        for d in [refused("file-write-unlink", "plain"), refused("file-write-data", "config")] {
            assert!(!said(d.clone()).contains("move or remove"), "{d:?}");
            assert!(!panel(d.clone()).iter().any(|g| matches!(g, Grant::Key(_))), "{d:?}");
        }
        drop(s);
    }
}
