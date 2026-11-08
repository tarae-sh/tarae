//! Permissions: what a plugin may do beyond the buffer. It says what in `permissions()` — programs to run,
//! files to read (patterns), environment variables, the network — and nothing it didn't say is ever done.
//!
//! - **Approval**: a plugin that asks for any is held until you approve the whole list: a card shows it when
//!   the plugin loads (once until quit), and what you approved is kept (`plugin-permissions.json` in the
//!   state folder). A yes is for that list and that code (#458: the sha256 of the bytes the load read): new code,
//!   or a new list, is asked again — new code's yes forgets the hosts and files said yes to for the old. A build
//!   `tarae plugin dev` put in place (its dev mark, `dev_source`) keeps a yes given to a build of its folder. What
//!   it asks for while the card is up waits on the answer (`plugin_hold`): done on yes, refused on not now.
//! - **At the moment of use**: a connection to a host:port it hasn't used before is asked about, once; so is
//!   a file outside its read patterns (a kubeconfig `KUBECONFIG` points elsewhere) — when it asked for some
//!   reading at all. The answers are kept with the approval.
//! - **Paths** are checked as written (`..` taken out) and again once resolved, so neither `..` nor a
//!   symlink leads out of a pattern.
//! - `:plugin-permissions` lists each plugin's; `:plugin-revoke <plugin>` takes them back.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use super::abi::{Action, Connect, HostEvent, Permission};
use super::shown;
use crate::editor::Editor;
use crate::offer::{Offer, What};

/// The most requests held for one plugin while its approval card is up (more are refused at once).
const HELD_MAX: usize = 256;

/// What a `y` on an approval card for code no longer loaded says (#458).
const STALE: &str = "its code changed while this card was up — the card for the new code is up";

/// What a plugin asks to do right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Connect to a host:port it hasn't used before.
    Connect(Connect),
    /// Read a file outside its patterns: (request id, the path resolved).
    Read(u64, PathBuf),
}

impl Ask {
    /// The request waiting on the answer.
    fn id(&self) -> u64 {
        match self {
            Ask::Connect(c) => c.id,
            Ask::Read(id, _) => *id,
        }
    }

    /// What's asked — (a host?, the host:port or the file): another connection to that host, or that file
    /// read again, is the same question.
    fn subject(&self) -> (bool, String) {
        match self {
            Ask::Connect(c) => (true, target(c)),
            Ask::Read(_, path) => (false, path.display().to_string()),
        }
    }
}

/// The code a load is (#458): the sha256 of the bytes it read (`Note::Loaded`'s `code`), and the folder `tarae plugin
/// dev` built it in, when its dev mark names these bytes (`dev_source`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Code {
    pub(super) sha256: String,
    pub(super) dev: Option<PathBuf>,
}

/// One plugin's approvals, as kept.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Approval {
    /// The permissions approved, as `key` writes them.
    pub granted: Vec<String>,
    /// The code they're approved for (empty: none — an entry from before #458, asked again), and for a yes given
    /// to a dev build, its folder: that folder's later builds keep the yes. A dev rebuild leaves `sha256` as it was.
    pub sha256: String,
    pub dev: Option<PathBuf>,
    /// host:port it may connect to, and files outside its patterns it may read.
    pub hosts: Vec<String>,
    pub files: Vec<String>,
}

impl Approval {
    /// Whether it's for `code`: the bytes approved, or a build of the dev folder it was given to.
    pub(super) fn holds_for(&self, code: &Code) -> bool {
        (!self.sha256.is_empty() && self.sha256 == code.sha256)
            || (self.dev.is_some() && self.dev == code.dev)
    }
}

/// A permission as it's kept.
pub(super) fn key(p: &Permission) -> String {
    match p {
        Permission::Run(prog) => format!("run {prog}"),
        Permission::Read(pat) => format!("read {pat}"),
        Permission::Env(name) => format!("env {name}"),
        Permission::Network => "network".into(),
        Permission::Project => "project".into(),
        Permission::Keys => "keys".into(),
    }
}

/// A permission in words, for the card — what it amounts to, broad reading said so.
fn describe(p: &Permission) -> String {
    match p {
        Permission::Run(prog) => format!("run the program {} (with any arguments, as you)", shown(prog)),
        Permission::Read(pat) => {
            let fixed: String =
                pat.split('/').take_while(|c| !c.contains(['*', '?'])).collect::<Vec<_>>().join("/");
            let warn = match fixed.as_str() {
                "" => " ⚠ every file on this computer",
                "~" => " ⚠ every file in your home folder",
                _ => "",
            };
            format!("read files at {}{warn}", shown(pat))
        }
        Permission::Env(name) => format!("read the environment variable {}", shown(name)),
        Permission::Network => "connect over the network (each new host is asked)".into(),
        Permission::Project => {
            "read the files of the project you opened, and find them by name — not ones named like keys \
             (.env, *.pem …): those it asks for"
                .into()
        }
        Permission::Keys => {
            "hear the keys you press as commands, never a character typed — only while your own config's \
             plugins.keys is on, said on the status line while it hears; it shows them in its overlay only, \
             never edits, says, opens, keeps, reads, connects to or runs anything"
                .into()
        }
    }
}

/// Why what a plugin declares isn't what tarae takes, if it isn't: each permission as written (`invalid_one`), and
/// a plugin that hears keys asking for nothing else (overlays plan §7 1: a keylogger needs a way out, a keycast
/// none) — no `network`, no `run`, and no reads either: a path it picks is a lookup the filesystem makes, and on a
/// network mount even a missing file's lookup reaches a server. What needs no permission — an edit, a message, a
/// panel, its own file — is refused as it's answered (`keys::refused`), and its card says so (`describe`).
pub(super) fn invalid(declared: &[Permission]) -> Option<String> {
    if let Some(why) = declared.iter().find_map(invalid_one) {
        return Some(why);
    }
    let other = declared.iter().find(|p| **p != Permission::Keys);
    match (declared.contains(&Permission::Keys), other) {
        (true, Some(p)) => Some(format!(
            "it asks to hear keys and for “{}” — a plugin that hears keys asks for nothing else: it reads, \
             connects to and runs nothing (what it hears stays in tarae)",
            shown(&key(p))
        )),
        _ => None,
    }
}

/// Whether a plugin that declared `declared` hears keys (`keys`) — and so writes nothing of its own.
pub(super) fn hears_keys(declared: &[Permission]) -> bool {
    declared.contains(&Permission::Keys)
}

/// Why a declared permission isn't one tarae takes, if it isn't: a program is a plain name, a variable a
/// plain name, a pattern absolute (or from `~/`) with no `.` or `..` in it — so what the card shows is what
/// it covers.
fn invalid_one(p: &Permission) -> Option<String> {
    let plain = |s: &str, extra: &str| {
        !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
    };
    let why = match p {
        Permission::Run(prog) if prog.len() > 64 || !plain(prog, "._+-") || prog.starts_with('.') => {
            "a program is a plain name (no path)"
        }
        Permission::Env(name)
            if name.len() > 128 || !plain(name, "_") || name.starts_with(|c: char| c.is_ascii_digit()) =>
        {
            "an environment variable's name"
        }
        Permission::Read(pat) if pat.len() > 200 => "a pattern up to 200 characters",
        Permission::Read(pat) if !(pat.starts_with('/') || pat.starts_with("~/")) => {
            "a pattern starts at / or ~/"
        }
        Permission::Read(pat) if pat.chars().any(|c| c.is_control() || super::bidi(c)) => {
            "no control characters"
        }
        Permission::Read(pat) if pat.split('/').skip(1).any(|c| matches!(c, "" | "." | "..")) => {
            "no `.`, `..` or empty parts in a pattern"
        }
        _ => return None,
    };
    Some(format!("its permission “{}” isn't valid: {why}", shown(&key(p))))
}

/// Where approvals are kept (none in tests, unless a test says).
pub(super) fn store_path() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::config::state_dir().map(|d| d.join("plugin-permissions.json"))
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect()
}

/// Every plugin's approvals, from `path`.
pub(super) fn load(path: Option<&Path>) -> BTreeMap<String, Approval> {
    let Some(text) = path.and_then(|p| std::fs::read_to_string(p).ok()) else { return BTreeMap::new() };
    let Ok(Value::Object(all)) = serde_json::from_str::<Value>(&text) else { return BTreeMap::new() };
    all.into_iter()
        .map(|(plugin, v)| {
            let a = Approval {
                granted: strs(&v["granted"]),
                sha256: v["sha256"].as_str().unwrap_or_default().to_string(),
                dev: v["dev"].as_str().map(PathBuf::from),
                hosts: strs(&v["hosts"]),
                files: strs(&v["files"]),
            };
            (plugin, a)
        })
        .collect()
}

pub(super) fn save(path: &Path, all: &BTreeMap<String, Approval>) -> std::io::Result<()> {
    // Inserted, not collected: a Map's `collect` sorts — a copy of the sort for each (~6 KB)
    let mut v = serde_json::Map::new();
    for (p, a) in all {
        let mut e = json!({ "granted": a.granted, "sha256": a.sha256, "hosts": a.hosts, "files": a.files });
        if let Some(dev) = &a.dev {
            e["dev"] = json!(dev.to_string_lossy());
        }
        v.insert(p.clone(), e);
    }
    write_whole(path, &Value::Object(v))
}

/// `v` written to `path` whole, then moved into place: never half a file (and a name of its own: two tareas at once).
fn write_whole(path: &Path, v: &Value) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let tmp = path.with_extension(format!("json.{}.{nanos}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_string_pretty(v).unwrap_or_default())?;
    std::fs::rename(tmp, path)
}

/// A dev mark's (`mark`): its build's sha256, the build's before it from the same folder, and the folder.
fn mark_read(path: &Path) -> Option<(String, Option<String>, PathBuf)> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let sha256 = v["sha256"].as_str().filter(|s| !s.is_empty())?.to_string();
    let previous = v["previous"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    Some((sha256, previous, PathBuf::from(v["source"].as_str()?)))
}

/// The dev mark (#458): `tarae plugin dev` writes `<marks>/<plugin>.json` before it renames a build into place — the
/// build's sha256 and the folder it was built in (`source`, real). It keeps the build's before it too, if it was the
/// same folder's: between this write and the rename, the plugins folder still holds that build, and a load of it
/// then (the watcher's, a start) is the same folder's dev build, not new code asking again.
pub(super) fn mark(marks: &Path, plugin: &str, sha256: &str, source: &Path) -> std::io::Result<()> {
    let path = marks.join(format!("{plugin}.json"));
    let previous = match mark_read(&path) {
        Some((same, previous, s)) if s == source && same == sha256 => previous,
        Some((before, _, s)) if s == source => Some(before),
        _ => None,
    };
    write_whole(&path, &json!({ "sha256": sha256, "previous": previous, "source": source.to_string_lossy() }))
}

/// The folder `tarae plugin dev` built the code `sha256` names in, if its mark (`mark`) names it — a `.wasm` put there
/// any other way (a copy, a download, a sync) isn't a build the mark names: no dev build, judged by its sha256. Reads
/// the disk: on the plugin thread, as it loads.
pub(super) fn dev_source(marks: &Path, plugin: &str, sha256: &str) -> Option<PathBuf> {
    let (last, previous, source) = mark_read(&marks.join(format!("{plugin}.json")))?;
    (!sha256.is_empty() && (last == sha256 || previous.as_deref() == Some(sha256))).then_some(source)
}

/// The declared list, as kept (sorted, so the order a plugin gives it in doesn't matter).
fn keys(declared: &[Permission]) -> Vec<String> {
    let mut k: Vec<String> = declared.iter().map(key).collect();
    crate::sorting::sort(&mut k);
    k.dedup();
    k
}

/// `~/x` → home/x; `..` and `.` taken out, lexically. None if it isn't absolute after that.
pub(super) fn expand(path: &str) -> Option<PathBuf> {
    let p = match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var_os("HOME")?).join(rest),
        None if path == "~" => PathBuf::from(std::env::var_os("HOME")?),
        None => PathBuf::from(path),
    };
    if !p.is_absolute() {
        return None;
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    Some(out)
}

/// `*` and `?` in one name.
fn name_matches(pat: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pat.chars().collect(), name.chars().collect());
    // (a star's last place, and where in the name it had got to)
    let (mut i, mut j, mut star) = (0, 0, None::<(usize, usize)>);
    while j < n.len() {
        if i < p.len() && (p[i] == '?' || p[i] == n[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some((i, j));
            i += 1;
        } else if let Some((si, sj)) = star {
            i = si + 1;
            j = sj + 1;
            star = Some((si, sj + 1));
        } else {
            return false;
        }
    }
    p[i..].iter().all(|&c| c == '*')
}

/// The disk the gate needs (#457): a pattern's folders resolved, a path made real, whether a file is there — only ever
/// off the main thread (the plugin thread at load, an open's worker). Through here so a test counts them by thread
/// (`the_gate_touches_no_disk_on_the_main_thread`).
pub(super) mod disk {
    use std::path::{Path, PathBuf};

    #[cfg(test)]
    thread_local! {
        /// The gate's disk touches on this thread.
        pub(in crate::plugin) static HERE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    /// …and on every thread.
    #[cfg(test)]
    pub(in crate::plugin) static ALL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn touched() {
        #[cfg(test)]
        {
            HERE.with(|n| n.set(n.get() + 1));
            ALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub(in crate::plugin) fn canonical(p: &Path) -> std::io::Result<PathBuf> {
        touched();
        std::fs::canonicalize(p)
    }

    /// `p` made real — as it is where nothing's there (a file to make).
    pub(in crate::plugin) fn real(p: &Path) -> PathBuf {
        canonical(p).unwrap_or_else(|_| p.to_path_buf())
    }

    pub(in crate::plugin) fn exists(p: &Path) -> bool {
        touched();
        p.exists()
    }
}

/// A pattern with the folders before its first wildcard resolved (macOS's /tmp is /private/tmp): what a
/// resolved path is matched against. Touches the disk: on the plugin thread, as it loads (`roots`).
fn resolved_pattern(pattern: &str) -> Option<PathBuf> {
    let pat = expand(pattern)?;
    let wild = |c: &Component| c.as_os_str().to_string_lossy().contains(['*', '?']);
    let fixed: PathBuf = pat.components().take_while(|c| !wild(c)).collect();
    let rest: PathBuf = pat.components().skip_while(|c| !wild(c)).collect();
    // (the longest part of it that exists)
    let mut base = fixed.clone();
    let mut tail = Vec::new();
    while !disk::exists(&base) {
        tail.push(base.file_name()?.to_owned());
        base.pop();
    }
    let mut out = disk::canonical(&base).ok()?;
    out.extend(tail.iter().rev());
    Some(out.join(rest))
}

/// Each `read` pattern a plugin declared, with its folders resolved (None: they don't resolve — it covers nothing
/// resolved). Made on the plugin thread as it loads and reloads (`plugins_start`'s note), so holding a buffer
/// against a pattern on every event waits on no disk; a pattern's folder swapped for a link later covers nothing
/// through it — the real path no longer matches —, which fails safe.
pub(super) fn roots(declared: &[Permission]) -> Vec<(String, Option<PathBuf>)> {
    declared
        .iter()
        .filter_map(|p| {
            if let Permission::Read(pat) = p { Some((pat.clone(), resolved_pattern(pat))) } else { None }
        })
        .collect()
}

/// Whether `path` is one `pattern` covers (`**`: any folders, none too).
pub(super) fn path_matches(pattern: &str, path: &Path) -> bool {
    expand(pattern).is_some_and(|pat| path_matches_in(&pat, path))
}

fn path_matches_in(pat: &Path, path: &Path) -> bool {
    let names =
        |p: &Path| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>();
    fn go(p: &[String], n: &[String]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some((first, rest)) if first == "**" => (0..=n.len()).any(|k| go(rest, &n[k..])),
            Some((first, rest)) => {
                n.split_first().is_some_and(|(name, more)| name_matches(first, name) && go(rest, more))
            }
        }
    }
    go(&names(pat), &names(path))
}

/// What a plugin's grant reaches (`plugin_grant`), taken from the editor to be held against a path with no disk
/// touched — on the loop, or on the worker an open of its goes to: its `read` patterns, as written and with their
/// folders resolved (`roots`), the project (a real path), the files allowed on its read card. A path's own real form
/// comes from where it was made real: a buffer's as it opened, an `open`'s on its worker.
#[derive(Clone)]
pub(super) struct Reach {
    patterns: Vec<(String, Option<PathBuf>)>,
    project: Option<PathBuf>,
    files: Vec<String>,
}

impl Reach {
    /// Whether it reaches `real` — and `lexical` as written, if there is one (a buffer's path was made real as it
    /// opened: `real` is all there is).
    pub(super) fn reaches(&self, lexical: Option<&Path>, real: &Path) -> bool {
        let covered = |at: &Path| self.patterns.iter().any(|(pat, _)| path_matches(pat, at));
        // (a pattern whose folders didn't resolve covers nothing resolved)
        let covered_real =
            |at: &Path| self.patterns.iter().any(|(_, r)| r.as_ref().is_some_and(|r| path_matches_in(r, at)));
        // The project's files, if it may read them — where they are, and where they lead; not ones named like
        // keys and tokens (`.env`, `*.pem` — `secretnames`): those are asked about like any file outside it
        let secret =
            lexical.is_some_and(crate::secretnames::named_secret) || crate::secretnames::named_secret(real);
        let in_project = !secret
            && self
                .project
                .as_ref()
                .is_some_and(|root| lexical.is_none_or(|l| l.starts_with(root)) && real.starts_with(root));
        let allowed_before = self.files.iter().any(|f| Path::new(f) == real);
        (lexical.is_none_or(covered) && covered_real(real)) || in_project || allowed_before
    }

    /// #457's rule (a): what it reaches, never a file named like a key, whatever its patterns say.
    ///
    /// The rule is `secretnames::named_secret` itself, on purpose stricter than the runs' `sandbox::key_named`: that
    /// one's exceptions (a toolchain's installs and packages, tools' output folders) are there so a run can work inside
    /// a package or a build's output; a plugin is handed a buffer, and needs none of that. Don't unify them.
    pub(super) fn covers(&self, lexical: Option<&Path>, real: &Path) -> bool {
        let secret = |p: &Path| crate::secretnames::named_secret(p);
        !lexical.is_some_and(secret) && !secret(real) && self.reaches(lexical, real)
    }
}

/// A connection's host:port, as approvals keep it (an IPv6 address in brackets).
pub(super) fn target(c: &Connect) -> String {
    if c.host.contains(':') { format!("[{}]:{}", c.host, c.port) } else { format!("{}:{}", c.host, c.port) }
}

/// Each plugin's approvals kept at `path`, as `:granted` lists them: (plugin, what it may do).
pub(crate) fn approved(path: &Path) -> Vec<(String, String)> {
    let n = |n: usize, one: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") };
    load(Some(path))
        .into_iter()
        .filter_map(|(plugin, a)| {
            let mut what = a.granted;
            if !a.hosts.is_empty() {
                what.push(format!("reach {}", n(a.hosts.len(), "host")));
            }
            if !a.files.is_empty() {
                what.push(format!("read {} more", n(a.files.len(), "file")));
            }
            let dev = if a.dev.is_some() { " · dev build" } else { "" };
            (!what.is_empty()).then(|| (plugin, format!("{}{dev}", what.join(", "))))
        })
        .collect()
}

impl Editor {
    /// Where the plugins' approvals are kept — None: no plugins, or none kept (tests).
    pub(crate) fn plugin_approvals_store(&self) -> Option<PathBuf> {
        self.plugins.as_ref()?.perm_store.clone()
    }

    /// Changes the approvals: on what's kept now (another tarae may have changed it), then kept.
    fn approvals_change(&mut self, f: impl FnOnce(&mut BTreeMap<String, Approval>)) -> std::io::Result<()> {
        let Some(p) = &mut self.plugins else { return Ok(()) };
        let Some(path) = p.perm_store.clone() else {
            f(&mut p.approvals);
            return Ok(());
        };
        let mut all = load(Some(&path));
        f(&mut all);
        let saved = save(&path, &all);
        p.perm_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        p.approvals = all;
        saved
    }

    /// The approvals as kept now — another tarae may have changed them (a revoke counts everywhere).
    fn approvals_fresh(&mut self) {
        let Some(p) = &mut self.plugins else { return };
        let Some(path) = &p.perm_store else { return };
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if mtime != p.perm_mtime {
            p.approvals = load(Some(path));
            p.perm_mtime = mtime;
        }
    }

    /// Its cards (approval, asks) closed: it was revoked or dropped — no `y` grants it later.
    pub(super) fn plugin_cards_close(&mut self, plugin: &str) {
        self.offers.retain(|o| !matches!(&o.what, What::PluginPermissions { plugin: q, .. } if q == plugin));
        self.plugin_asks_close(plugin);
    }

    /// Its asks closed and what waited on its approval refused — a reload's: what the code before asked isn't the
    /// new code's (its approval card is renewed for the new code, `plugin_card_renew`).
    pub(super) fn plugin_asks_close(&mut self, plugin: &str) {
        self.offers.retain(|o| !matches!(&o.what, What::PluginAsk { plugin: q, .. } if q == plugin));
        // (what waited on its approval card: refused — never left unanswered)
        let held = self.plugins.as_mut().and_then(|p| p.held.remove(plugin)).unwrap_or_default();
        let why = format!("its permissions wait for your approval (:plugin-approve {plugin})");
        let no: Vec<HostEvent> = held
            .iter()
            .filter_map(|a| match a {
                Action::Spawn(sp) => Some(sp.id),
                Action::ReadFile(id, _) | Action::ReadEnv(id, _) | Action::Find(id, _) => Some(*id),
                Action::Connect(c) => Some(c.id),
                _ => None,
            })
            .map(|id| HostEvent::Failed(id, why.clone()))
            .collect();
        if !no.is_empty() {
            self.plugin_events(plugin, no);
        }
    }

    /// `:plugin-approve <plugin>` — its approval card again.
    pub fn plugin_approve(&mut self, plugin: &str) -> Result<(), String> {
        let plugin = plugin.trim();
        let p = self.plugins.as_mut().ok_or("no plugins are loaded")?;
        let declared =
            p.permissions.get(plugin).cloned().ok_or_else(|| format!("no plugin {plugin} is loaded"))?;
        if declared.is_empty() {
            return Err(format!("{plugin} asks for nothing beyond the buffer"));
        }
        if let Some(why) = invalid(&declared) {
            return Err(format!("{plugin} can't be approved: {why}"));
        }
        p.asked.insert(plugin.to_string());
        self.plugin_permissions_card(plugin, declared);
        Ok(())
    }

    /// A plugin loaded, asking for `declared`: held until you approve them (a card asks — once until quit).
    pub(super) fn plugin_permissions_loaded(&mut self, plugin: &str, declared: &[Permission]) {
        if declared.is_empty() || self.plugin_approved(plugin) {
            return;
        }
        let Some(p) = &mut self.plugins else { return };
        if !p.asked.insert(plugin.to_string()) {
            return;
        }
        self.plugin_permissions_card(plugin, declared.to_vec());
    }

    /// The code a plugin's load is (#458) — none: not loaded.
    pub(super) fn plugin_code(&self, plugin: &str) -> Code {
        self.plugins.as_ref().and_then(|p| p.code.get(plugin)).cloned().unwrap_or_default()
    }

    fn plugin_permissions_card(&mut self, plugin: &str, declared: Vec<Permission>) {
        let o = self.plugin_permissions_offer(plugin, declared);
        self.push_offer(o);
    }

    /// Its approval card up, about code (or a list) no longer loaded — rewritten for what's loaded, `rev` bumped as
    /// any card that changes is (its focus gone with it: a key meant for what it said isn't an answer to what it says
    /// now; `keep_focus`: kept, the key that found it stale being the one that renews it), or closed when what's
    /// loaded needs none. True: rewritten, so the card up is about the code that's loaded (#458).
    pub(super) fn plugin_card_renew(&mut self, plugin: &str, keep_focus: bool) -> bool {
        let ours = |o: &Offer| {
            matches!(&o.what, What::PluginPermissions { plugin: q, .. } if q == plugin) && !o.installing()
        };
        let declared = self.plugins.as_ref().and_then(|p| p.permissions.get(plugin)).cloned();
        let Some(declared) = declared.filter(|d| !d.is_empty() && !self.plugin_approved(plugin)) else {
            self.offers.retain(|o| !ours(o));
            for l in std::iter::once(&mut self.workspace).chain(&mut self.workspaces) {
                l.parked.retain(|o| !ours(o));
            }
            return false;
        };
        let new = self.plugin_permissions_offer(plugin, declared);
        let mut renewed = None;
        let parked =
            std::iter::once(&mut self.workspace).chain(&mut self.workspaces).flat_map(|l| &mut l.parked);
        for o in self.offers.iter_mut().chain(parked).filter(|o| ours(o)) {
            (o.title, o.question, o.decided) = (new.title.clone(), new.question.clone(), new.decided.clone());
            (o.what, o.warnings) = (new.what.clone(), Vec::new());
            renewed = Some((o.id, o.rev));
            o.rev += 1;
        }
        let Some((id, rev)) = renewed else { return false };
        if keep_focus && self.card_focus == Some((id, rev)) {
            self.card_focus = Some((id, rev + 1));
        }
        // (asked: this card is the asking — no second one comes up for the load)
        if let Some(p) = &mut self.plugins {
            p.asked.insert(plugin.to_string());
        }
        true
    }

    /// A `y` on the approval card in front when it's about code (or a list) no longer loaded: it approves nothing,
    /// and that's said on the card, renewed for what's loaded — never a key that did nothing (#458).
    pub(crate) fn plugin_card_stale(&mut self) -> bool {
        let Some(What::PluginPermissions { plugin, permissions, code }) =
            self.offers.first().map(|o| &o.what)
        else {
            return false;
        };
        let now = self.plugins.as_ref().and_then(|p| p.permissions.get(plugin)).map(|d| keys(d));
        if now == Some(keys(permissions)) && self.plugin_code(plugin) == *code {
            return false;
        }
        let plugin = plugin.clone();
        let loaded = now.is_some();
        if self.plugin_card_renew(&plugin, true) {
            self.offer_refused(STALE);
        } else if loaded {
            self.set_status(format!(
                "Plugin {plugin}: its code changed while this card was up — the new code needs no yes"
            ));
        } else {
            self.set_status(format!(
                "Plugin {plugin}: its code changed while this card was up — it isn't loaded now"
            ));
        }
        true
    }

    /// Its approval card for what's loaded now — the code, the list and what changed since your yes.
    fn plugin_permissions_offer(&self, plugin: &str, declared: Vec<Permission>) -> Offer {
        let code = self.plugin_code(plugin);
        let before =
            self.plugins.as_ref().and_then(|p| p.approvals.get(plugin)).filter(|a| !a.granted.is_empty());
        let list_changed = before.is_some_and(|a| a.granted != keys(&declared));
        let code_changed = before.is_some_and(|a| !a.holds_for(&code));
        let head = match (before.is_some(), code_changed, list_changed) {
            (false, ..) => "It asks to:",
            (true, true, true) => "Its code changed since you approved it, and it now asks to:",
            (true, true, false) => "Its code changed since you approved it. It asks to:",
            (true, false, _) => "It now asks to:",
        };
        let mut lines: Vec<String> = declared.iter().map(|p| format!("· {}", describe(p))).collect();
        let n = lines.len();
        // (after the list: the rows `deciding` names stay the list's)
        if let Some(dev) = &code.dev {
            let at = shown(&crate::allow::tilde(dev));
            lines.push(format!("A dev build from {at} — its rebuilds there keep this answer."));
        }
        let dev = if code.dev.is_some() { " · dev build" } else { "" };
        Offer::new(
            format!("Plugin {plugin} asks for permissions{dev}"),
            &format!("{head}\n{}", lines.join("\n")),
            "…",
            format!("Plugin {plugin}"),
            What::PluginPermissions { plugin: plugin.to_string(), permissions: declared, code },
        )
        .deciding(1..1 + n)
    }

    /// Whether what a plugin asks for is what you approved, for this code (#458) — nothing to approve counts.
    pub(super) fn plugin_approved(&self, plugin: &str) -> bool {
        let Some(p) = &self.plugins else { return false };
        // (what the load refuses is never approved either — a kept approval, a card from before)
        if p.permissions.get(plugin).is_some_and(|d| invalid(d).is_some()) {
            return false;
        }
        let declared = p.permissions.get(plugin).map(|d| keys(d)).unwrap_or_default();
        let code = self.plugin_code(plugin);
        declared.is_empty()
            || p.approvals.get(plugin).is_some_and(|a| a.granted == declared && a.holds_for(&code))
    }

    /// The approval card's answer — for `declared` and the `code` the card was for.
    pub(crate) fn plugin_permissions_answer(
        &mut self,
        plugin: String,
        declared: Vec<Permission>,
        code: Code,
        allow: bool,
    ) {
        if let Some(why) = invalid(&declared).filter(|_| allow) {
            self.set_warning(format!("Plugin {plugin} can't be approved: {why}"));
            return self.plugin_held_go(&plugin);
        }
        if !allow {
            self.set_status(format!(
                "Plugin {plugin} does nothing beyond the buffer until you approve — :plugin-approve {plugin}"
            ));
            return self.plugin_held_go(&plugin);
        }
        // Still that plugin, still asking for that, still that code (a yes never approves bytes its card didn't show —
        // a card's own `y` finds that first, `plugin_card_stale`; this, for one given without it: said, and the card
        // for what's loaded up)
        let now = self.plugins.as_ref().and_then(|p| p.permissions.get(&plugin)).cloned();
        if now.as_ref().map(|d| keys(d)) != Some(keys(&declared)) || self.plugin_code(&plugin) != code {
            if let (Some(now), Some(p)) = (&now, &mut self.plugins) {
                p.asked.remove(&plugin);
                self.plugin_permissions_loaded(&plugin, now);
            }
            let up = self
                .offers
                .iter()
                .any(|o| matches!(&o.what, What::PluginPermissions { plugin: q, .. } if *q == plugin));
            let why =
                if up { STALE } else { "its code changed while its card was up — it approved nothing" };
            return self.set_status(format!("Plugin {plugin}: {why}"));
        }
        let granted = keys(&declared);
        let dev = code.dev.is_some();
        let r = self.approvals_change(|all| {
            let a = all.entry(plugin.clone()).or_default();
            // New code's yes: what was said yes to for the old code's use (hosts, files) isn't this code's
            if !a.holds_for(&code) {
                a.hosts.clear();
                a.files.clear();
            }
            (a.granted, a.sha256, a.dev) = (granted, code.sha256, code.dev);
        });
        match r {
            Ok(()) if dev => {
                self.set_success(format!("Plugin {plugin}: approved — a dev build, its rebuilds keep it"))
            }
            Ok(()) => self.set_success(format!("Plugin {plugin}: approved")),
            Err(e) => self.set_warning(format!("approved until you quit — couldn't keep it: {e}")),
        }
        self.plugin_held_go(&plugin);
    }

    /// Whether `a` waits for the plugin's approval card — it needs a permission, and the card asks for them now
    /// (the first time, it comes up, as when a request is refused). Held, it's done once you approve
    /// (`plugin_held_go`): the plugin goes on by itself, not stuck on a refusal from before.
    pub(super) fn plugin_hold(&mut self, plugin: &str, a: &Action) -> bool {
        let needs = matches!(
            a,
            Action::Spawn(_)
                | Action::ReadFile(..)
                | Action::ReadEnv(..)
                | Action::Connect(_)
                | Action::Find(..)
        );
        let Some(declared) = self.plugins.as_ref().and_then(|p| p.permissions.get(plugin)).cloned() else {
            return false;
        };
        if !needs || declared.is_empty() {
            return false;
        }
        self.approvals_fresh();
        if self.plugin_approved(plugin) {
            return false;
        }
        self.plugin_permissions_loaded(plugin, &declared);
        let card = self.offers.iter().chain(&self.workspace.parked).any(|o| {
            matches!(&o.what, What::PluginPermissions { plugin: q, .. } if q == plugin) && !o.installing()
        });
        let Some(p) = self.plugins.as_mut().filter(|_| card) else { return false };
        let held = p.held.entry(plugin.to_string()).or_default();
        if held.len() >= HELD_MAX {
            return false;
        }
        held.push(a.clone());
        true
    }

    /// The approval card answered: what waited on it, done now — or, not approved, refused as a request is
    /// then (its card gone, so none is held again).
    fn plugin_held_go(&mut self, plugin: &str) {
        let held = self.plugins.as_mut().and_then(|p| p.held.remove(plugin)).unwrap_or_default();
        if held.is_empty() {
            return;
        }
        let doc = self.doc();
        let seen = super::Seen { doc: doc.id, version: doc.version(), ranges: Vec::new(), yours: false };
        self.plugin_apply(plugin, &seen, held);
    }

    /// Whether a plugin may, right now, do what needs `need` (declared and approved). Err: why not.
    pub(super) fn plugin_may(&mut self, plugin: &str, need: &Permission) -> Result<(), String> {
        let Some(p) = &self.plugins else { return Err("plugins aren't running".into()) };
        let declared = p.permissions.get(plugin).cloned().unwrap_or_default();
        if !declared.contains(need) {
            return Err(format!("it didn't ask to {} (permissions())", describe(need)));
        }
        self.approvals_fresh();
        if !self.plugin_approved(plugin) {
            // (not asked again on its own — a plugin can't nag; `:plugin-approve` brings the card back)
            self.plugin_permissions_loaded(plugin, &declared);
            return Err(format!("its permissions wait for your approval (:plugin-approve {plugin})"));
        }
        Ok(())
    }

    /// Whether a plugin may read `path` (Ok: the path, resolved); `Err(None)`: ask you first.
    pub(super) fn plugin_may_read(&mut self, plugin: &str, path: &str) -> Result<PathBuf, Option<String>> {
        let Some(lexical) = expand(path) else {
            return Err(Some(format!("“{}”: an absolute path, or one from ~/", shown(path))));
        };
        // Resolved too (a symlink out of the pattern is out of it); a missing file is the reader's to say
        let real = std::fs::canonicalize(&lexical).unwrap_or(lexical.clone());
        match self.plugin_grant(plugin) {
            Ok(reach) if reach.reaches(Some(&lexical), &real) => Ok(real),
            Ok(_) => Err(None),
            Err(why) => Err(Some(why)),
        }
    }

    /// #457's rule (a): whether a plugin's own grant covers the file at `real` (resolved; `lexical`, as it was
    /// written) — `Reach::covers`. What a plugin opens, and the buffers it's told of on its own, are only these.
    pub(super) fn plugin_covers(&self, plugin: &str, lexical: Option<&Path>, real: &Path) -> bool {
        self.plugin_grant(plugin).is_ok_and(|reach| reach.covers(lexical, real))
    }

    /// What a plugin's grant reaches: its `read` patterns (their folders resolved as it loaded — `roots`) or
    /// `project`, approved, and the files allowed on its read card. Err: why it reads no file at all. Touches no disk.
    pub(super) fn plugin_grant(&self, plugin: &str) -> Result<Reach, String> {
        let Some(p) = &self.plugins else { return Err("plugins aren't running".into()) };
        let declared = p.permissions.get(plugin).map(Vec::as_slice).unwrap_or_default();
        let roots = p.roots.get(plugin).map(Vec::as_slice).unwrap_or_default();
        let patterns: Vec<(String, Option<PathBuf>)> = declared
            .iter()
            .filter_map(|d| if let Permission::Read(pat) = d { Some(pat) } else { None })
            .map(|pat| (pat.clone(), roots.iter().find(|(q, _)| q == pat).and_then(|(_, r)| r.clone())))
            .collect();
        let project = declared.contains(&Permission::Project).then(|| self.workspace.root.clone());
        if patterns.is_empty() && project.is_none() {
            return Err("it didn't ask to read files (permissions())".into());
        }
        if !self.plugin_approved(plugin) {
            return Err("its permissions wait for your approval".into());
        }
        let files = p.approvals.get(plugin).map(|a| a.files.clone()).unwrap_or_default();
        Ok(Reach { patterns, project, files })
    }

    /// Whether a plugin may connect to host:port; `Err(None)`: ask you first.
    pub(super) fn plugin_may_connect(&mut self, plugin: &str, c: &Connect) -> Result<(), Option<String>> {
        self.plugin_may(plugin, &Permission::Network).map_err(Some)?;
        let ok = |ch: char| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ':');
        if c.host.is_empty()
            || c.host.len() > 253
            || !c.host.chars().all(ok)
            || !(1..=65535).contains(&c.port)
        {
            return Err(Some(format!("“{}” port {} isn't a host to connect to", shown(&c.host), c.port)));
        }
        // (never reached whatever you'd answer: no card for it)
        if let Some(why) = super::io::never_reached(&c.host) {
            return Err(Some(why));
        }
        let target = target(c);
        let known = self
            .plugins
            .as_ref()
            .and_then(|p| p.approvals.get(plugin))
            .is_some_and(|a| a.hosts.contains(&target));
        if known { Ok(()) } else { Err(None) }
    }

    /// Asks you about one connection or file — one card a question: a request asking what a card already asks
    /// (another connection to that host) waits on that card's answer.
    pub(super) fn plugin_ask(&mut self, plugin: &str, ask: Ask) {
        let asked =
            self.offers.iter_mut().chain(self.workspace.parked.iter_mut()).find_map(|o| match &mut o.what {
                What::PluginAsk { plugin: p, asks }
                    if p == plugin && asks.first().map(Ask::subject) == Some(ask.subject()) =>
                {
                    Some(asks)
                }
                _ => None,
            });
        if let Some(asks) = asked {
            asks.push(ask);
            return;
        }
        let (title, question) = match &ask {
            Ask::Connect(c) => (
                format!("Plugin {plugin} wants to connect"),
                format!("to {}\nAllow it, from now on?", target(c)),
            ),
            Ask::Read(_, path) => (
                format!("Plugin {plugin} wants to read a file"),
                format!(
                    "{}\n(outside the files it asked for) Allow it, from now on?",
                    shown(&path.display().to_string())
                ),
            ),
        };
        // (what's decided: the host, the file — not the question after it)
        self.push_offer(
            Offer::new(
                title,
                &question,
                "…",
                format!("Plugin {plugin}"),
                What::PluginAsk { plugin: plugin.to_string(), asks: vec![ask] },
            )
            .deciding(0..1),
        );
    }

    /// Your answer about one connection or file: yes is kept, and the requests waiting on it go on; no fails
    /// them.
    pub(crate) fn plugin_ask_answer(&mut self, plugin: String, asks: Vec<Ask>, allow: bool) {
        // Still loaded and approved (a card left from before a revoke grants nothing)
        let loaded = self.plugins.as_ref().is_some_and(|p| p.permissions.contains_key(&plugin));
        self.approvals_fresh();
        if !loaded || !self.plugin_approved(&plugin) {
            return;
        }
        if !allow {
            let no = asks.iter().map(|a| HostEvent::Failed(a.id(), "you said no".into())).collect();
            return self.plugin_events(&plugin, no);
        }
        let Some((host, entry)) = asks.first().map(Ask::subject) else { return };
        let r = self.approvals_change(|all| {
            let a = all.entry(plugin.clone()).or_default();
            if host { a.hosts.push(entry) } else { a.files.push(entry) }
        });
        if let Err(e) = r {
            self.set_warning(format!("allowed until you quit — couldn't keep it: {e}"));
        }
        for ask in asks {
            match ask {
                Ask::Connect(c) => self.plugin_connect_now(&plugin, c),
                Ask::Read(id, path) => self.plugin_read_now(&plugin, id, path),
            }
        }
    }

    /// `:plugin-permissions` — every plugin's, in a buffer.
    pub fn plugin_permissions_list(&mut self) -> Result<(), String> {
        let p = self.plugins.as_ref().ok_or("no plugins are loaded")?;
        let mut out =
            String::from("# Plugin permissions\n\n`:plugin-revoke <plugin>` takes a plugin's back.\n");
        let mut names: Vec<&String> = p.permissions.keys().collect();
        crate::sorting::sort(&mut names);
        for name in names {
            let declared = &p.permissions[name];
            let a = p.approvals.get(name).cloned().unwrap_or_default();
            let code = p.code.get(name).cloned().unwrap_or_default();
            let state = if declared.is_empty() {
                "nothing beyond the buffer".to_string()
            } else if a.granted == keys(declared) && a.holds_for(&code) {
                match &a.dev {
                    Some(dev) => format!("approved · dev build from {}", shown(&crate::allow::tilde(dev))),
                    None => "approved".into(),
                }
            } else if !a.granted.is_empty() && !a.holds_for(&code) {
                "waiting for your approval — its code changed".into()
            } else {
                "waiting for your approval".into()
            };
            out.push_str(&format!("\n## {name} — {state}\n\n"));
            // (its sha256's first 12 hex: which build this is — `shasum -a 256` says the rest)
            if let Some(short) = code.sha256.get(..12) {
                out.push_str(&format!("code `{short}`\n\n"));
            }
            for d in declared {
                out.push_str(&format!("- {}\n", describe(d)));
            }
            for h in &a.hosts {
                out.push_str(&format!("- connected to {h}\n"));
            }
            for f in &a.files {
                out.push_str(&format!("- read {f}\n"));
            }
        }
        let id = self.open_virtual("plugin permissions", &out);
        if let Some(spec) = crate::syntax::spec("markdown") {
            self.load_language(id, spec);
        }
        Ok(())
    }

    /// `:plugin-revoke <plugin>` — its approvals, hosts and files forgotten; it asks again next time.
    pub fn plugin_revoke(&mut self, plugin: &str) -> Result<(), String> {
        let plugin = plugin.trim();
        self.approvals_fresh();
        let p = self.plugins.as_mut().ok_or("no plugins are loaded")?;
        if !p.approvals.contains_key(plugin) {
            return Err(format!("{plugin} has nothing approved"));
        }
        // What it runs stops now, and its cards go
        p.stop_all(plugin);
        self.plugin_cards_close(plugin);
        self.approvals_change(|all| {
            all.remove(plugin);
        })
        .map_err(|e| e.to_string())?;
        self.set_success(format!("Plugin {plugin}: permissions taken back"));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #458: an approval keeps the code it's for, and a dev yes its folder; an entry from before has none, and holds
    /// for no code.
    #[test]
    fn approvals_keep_their_code_and_one_without_holds_for_none() {
        let s = crate::scratch::Scratch::new("perm-code");
        let path = s.join("plugin-permissions.json");
        let dev = Approval {
            granted: vec!["network".into()],
            sha256: "ab".repeat(32),
            dev: Some("/src/a".into()),
            hosts: vec!["example.com:443".into()],
            files: Vec::new(),
        };
        let plain = Approval { sha256: "cd".repeat(32), dev: None, ..dev.clone() };
        let all = BTreeMap::from([("dev".to_string(), dev.clone()), ("plain".to_string(), plain.clone())]);
        save(&path, &all).unwrap();
        assert_eq!(load(Some(&path)), all);
        assert!(!disk_text(&path).contains("null"), "no dev, no key");
        let code = |sha: &str, dev: Option<&str>| Code { sha256: sha.into(), dev: dev.map(PathBuf::from) };
        assert!(
            plain.holds_for(&code(&"cd".repeat(32), None)) && !plain.holds_for(&code("ef", Some("/src/a")))
        );
        assert!(dev.holds_for(&code("ef", Some("/src/a"))) && !dev.holds_for(&code("ef", Some("/src/b"))));
        std::fs::write(&path, r#"{"old": {"granted": ["network"], "hosts": [], "files": []}}"#).unwrap();
        let old = &load(Some(&path))["old"];
        assert_eq!((old.sha256.as_str(), &old.dev), ("", &None));
        assert!(!old.holds_for(&Code::default()) && !old.holds_for(&code("ef", None)));
    }

    fn disk_text(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    /// The dev mark names its build and the one before it from the same folder (the rename window), never another
    /// folder's — and nothing it doesn't name.
    #[test]
    fn a_dev_mark_names_its_build_and_the_one_before_from_its_folder() {
        let s = crate::scratch::Scratch::new("perm-mark");
        let marks = s.join("plugin-dev");
        let (a, b) = (Path::new("/src/a"), Path::new("/src/b"));
        let at = |sha: &str| dev_source(&marks, "demo", sha);
        assert_eq!(at("h1"), None, "no mark");
        mark(&marks, "demo", "h1", a).unwrap();
        assert_eq!((at("h1"), at("h0"), at("")), (Some(a.into()), None, None));
        mark(&marks, "demo", "h2", a).unwrap();
        assert_eq!((at("h2"), at("h1")), (Some(a.into()), Some(a.into())));
        mark(&marks, "demo", "h2", a).unwrap();
        assert_eq!(at("h1"), Some(a.into()), "the same build again keeps the one before");
        mark(&marks, "demo", "h3", b).unwrap();
        assert_eq!((at("h3"), at("h2")), (Some(b.into()), None), "another folder's build before isn't b's");
        assert_eq!(dev_source(&marks, "other", "h3"), None);
    }

    #[test]
    fn patterns_cover_what_they_say() {
        let home = std::env::var("HOME").unwrap();
        let p = |s: &str| PathBuf::from(s.replace('~', &home));
        assert!(path_matches("~/.kube/**", &p("~/.kube/config")));
        assert!(path_matches("~/.kube/**", &p("~/.kube/cache/discovery/x.json")));
        assert!(!path_matches("~/.kube/**", &p("~/.ssh/id_rsa")));
        assert!(path_matches("~/.kube/*.yaml", &p("~/.kube/dev.yaml")));
        assert!(!path_matches("~/.kube/*.yaml", &p("~/.kube/a/dev.yaml")), "* stays in one folder");
        assert!(path_matches("/etc/k8s/admin.conf", Path::new("/etc/k8s/admin.conf")));
        assert!(name_matches("k?b*.conf", "kube-admin.conf") && !name_matches("*.conf", "x.yaml"));
        // `..` doesn't climb out
        assert_eq!(expand("~/.kube/../.ssh/id_rsa"), Some(p("~/.ssh/id_rsa")));
        assert!(!path_matches("~/.kube/**", &expand("~/.kube/../.ssh/id_rsa").unwrap()));
        assert_eq!(expand("relative/path"), None);
    }

    #[test]
    fn a_symlink_out_of_a_pattern_is_out_of_it() {
        let s = crate::scratch::Scratch::new("perm-symlink");
        let d = s.to_path_buf();
        std::fs::create_dir_all(d.join("allowed")).unwrap();
        std::fs::write(d.join("allowed/plain"), "x").unwrap();
        std::fs::write(d.join("secret"), "x").unwrap();
        let link = d.join("allowed/link");
        std::os::unix::fs::symlink(d.join("secret"), &link).unwrap();
        let pattern = format!("{}/allowed/**", d.display());
        assert!(path_matches(&pattern, &link), "as written, it's in");
        let resolved = roots(&[Permission::Read(pattern.clone())]).remove(0).1.unwrap();
        let real = std::fs::canonicalize(&link).unwrap();
        assert!(!path_matches_in(&resolved, &real), "resolved, it isn't");
        // …while a plain file in it stays in, though the temp folder itself is a symlink on macOS
        let plain = std::fs::canonicalize(d.join("allowed/plain")).unwrap();
        assert!(path_matches_in(&resolved, &plain));
    }
}
