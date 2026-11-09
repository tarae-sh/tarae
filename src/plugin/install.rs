//! PD1's H2: what Claude's `plugin` tool asks, and the typed commands beside it — one path each, two ways in.
//! - **A crate started** (`new`, or `:plugin-new <dir> [--from <example>]`): the template or an example, the WIT,
//!   `REFERENCE.md` generated from it, `Cargo.toml` and `.cargo` — written by tarae, never a run (those are names a
//!   run can't write in the project). Claude's raises a card (`plugin-new`) whose `y` writes it; yours writes it at once.
//! - **A build installed** (`wasm`, or `:plugin-install <wasm>`): one card, `plugin-install`, showing what it may do in
//!   the load card's words. Its `y` approves exactly the bytes the card holds — hash-bound, never a dev approval: a
//!   folder Claude edits never keeps a yes across rebuilds (#458) — before they're renamed into the plugins folder, so
//!   the load that follows finds them approved and no load card comes. A rebuild since the card isn't what lands.
//!
//! Claude never answers either card: no tool reaches `offer_accept_by` (`orchestra.rs`). The plugins folder is never a
//! run's to write (`allow::NEVER_WRITE`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use super::abi::{Permission, Plugin};
use super::perm::{Change, Code};
use crate::editor::Editor;
use crate::offer::{Offer, What};
use crate::tools::Call;

/// What a plugin's look may spend (`cli::list_in`'s measure).
const FUEL: u64 = 100_000_000;
/// The biggest module installed.
const MAX_BYTES: u64 = 64 << 20;

/// A build asked to be installed, as its card shows it — the bytes the card's `y` installs and approves.
#[derive(Clone, Debug)]
pub struct Install {
    /// Claude's call waiting on the card (None: you typed `:plugin-install`).
    pub call: Option<Call>,
    pub name: String,
    /// Where the `.wasm` was found, and the crate it's in (the nearest `Cargo.toml` above it), as shown: from the
    /// project's root — where it was found, never "built from": a run can write any bytes in the project.
    pub from: String,
    pub crate_dir: Option<String>,
    pub bytes: Arc<[u8]>,
    pub sha256: String,
    pub permissions: Vec<Permission>,
    pub commands: Vec<String>,
    /// Its subscriptions, as said: `saved`, `opened (yaml)`.
    pub events: Vec<String>,
    /// A plugin of that name is installed now.
    pub replaces: bool,
    /// The plugins folder it goes to.
    pub dir: PathBuf,
}

/// A crate Claude asked to start, as its card shows it.
#[derive(Clone, Debug)]
pub struct New {
    pub call: Call,
    /// The project's root, and where in it by real names (walked again through no link at `y`); as shown.
    pub root: PathBuf,
    pub rel: PathBuf,
    pub shown: String,
    pub from: Option<String>,
    pub files: Vec<(&'static str, String)>,
}

/// The plugins folder an install writes to — in tests, only a scratch one a test named (`stand_in`): never yours.
fn plugins_dir() -> Option<PathBuf> {
    #[cfg(test)]
    return STAND_IN.with(|s| s.borrow().clone());
    #[cfg(not(test))]
    super::dir()
}

#[cfg(test)]
thread_local! {
    static STAND_IN: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// (tests: installs go to `dir`, a scratch folder)
#[cfg(test)]
pub(crate) fn stand_in(dir: &Path) {
    STAND_IN.with(|s| *s.borrow_mut() = Some(dir.to_path_buf()));
}

/// The tool's schema: one tool, `wasm` its install, `new` (and `from`) its start of a crate.
pub(crate) fn schema() -> Value {
    let names: Vec<&str> = super::examples::EXAMPLES.iter().map(|e| e.name).collect();
    json!({
        "name": "plugin",
        "description": "Install a tarae plugin you built: the user's card shows what it may do, and their yes installs \
            and approves those bytes only (a run can't write the plugins folder). Its API: the crate's REFERENCE.md.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "wasm": { "type": "string", "description": "The built .wasm, in the project" },
                "new": { "type": "string", "description": "Instead: a folder to start a plugin crate in, on the user's \
                    yes (Cargo.toml, .cargo, REFERENCE.md)" },
                "from": { "type": "string", "description": format!("With new: an example — {}", names.join(" ")) }
            }
        }
    })
}

/// Claude's call: a card comes after a worker's look (none: told why, no card). Ok: the call waits on its card.
pub(crate) fn tool_call(ed: &mut Editor, call: Call, args: &Value) -> Result<(), String> {
    let text = |k: &str| args[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let root = ed.workspace.root.clone();
    match (text("wasm"), text("new")) {
        (Some(wasm), None) => {
            let dir = plugins_dir().ok_or("no plugins folder here (set HOME or XDG_CONFIG_HOME)")?;
            ed.events.jobs().spawn(move || {
                let r = look(&root, &wasm, &dir);
                move |ed: &mut Editor| match r.and_then(|i| one_at_a_time(ed, i)) {
                    Ok(mut i) => {
                        i.call = Some(call);
                        let o = install_card(&i);
                        ed.push_offer(o);
                    }
                    Err(e) => crate::tools::answer(ed, call, Err(e)),
                }
            });
            Ok(())
        }
        (None, Some(dir)) => {
            let from = text("from");
            ed.events.jobs().spawn(move || {
                let r = new_look(&root, &dir, from.as_deref());
                move |ed: &mut Editor| match r {
                    Ok((rel, shown, files)) => {
                        let n = New { call, root, rel, shown, from, files };
                        let o = new_card(&n);
                        ed.push_offer(o);
                    }
                    Err(e) => crate::tools::answer(ed, call, Err(e)),
                }
            });
            Ok(())
        }
        _ => Err("one of wasm (a build to install) or new (a folder to start a crate in)".into()),
    }
}

/// One install of a name at a time, from its card to its bytes landing: two in flight could land out of order, the
/// folder's file not the one approved last.
fn one_at_a_time(ed: &Editor, i: Install) -> Result<Install, String> {
    let same = |o: &&Offer| matches!(&o.what, What::PluginInstall(x) if x.name == i.name);
    match ed.offers.iter().chain(&ed.workspace.parked).find(same) {
        Some(o) => Err(format!("an install of plugin {} is under way — card#{}", i.name, o.id)),
        None => Ok(i),
    }
}

/// `path` as shown: from the project's root when it's in it.
fn shown_in(root: &Path, path: &Path) -> String {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    path.strip_prefix(&root).unwrap_or(path).display().to_string()
}

/// The look at a build (on a worker): read once, hashed, loaded as tarae loads it — what it registers and asks for;
/// refused before any card when it isn't in the project, isn't a plugin tarae loads, or asks for what the load refuses.
fn look(root: &Path, wasm: &str, dir: &Path) -> Result<Install, String> {
    // Opened once, judged by what was opened (its descriptor's real path) and read from that descriptor — never the
    // path again: a swap since can't make "found at" wrong, and a file growing meanwhile stops at the limit
    let opened = crate::paths::open_read(&root.join(wasm)).map_err(|e| format!("{wasm}: {e}"))?;
    let top = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let real = opened.real;
    if !real.starts_with(&top) {
        return Err(format!("{wasm} is outside the project — a plugin is installed from the project"));
    }
    if real.extension().is_none_or(|e| e != "wasm") || !opened.meta.is_file() {
        return Err(format!("{wasm} isn't a .wasm"));
    }
    // (by its size first, before a byte is read; then the read itself stops at the limit)
    let len = opened.meta.len();
    if len > MAX_BYTES {
        return Err(format!("{wasm} is {} MB — more than a plugin may be (64 MB)", len.div_ceil(1 << 20)));
    }
    let read = at_most(&opened.file, MAX_BYTES).map_err(|e| format!("{wasm}: {e}"))?;
    let bytes: Arc<[u8]> = read.ok_or_else(|| format!("{wasm} grew past 64 MB while it was read"))?.into();
    // Its crate: the one whose `target/` cargo built it in, in the project — the plugin's name its package's, as
    // `tarae plugin dev` names it (a workspace's: none); a .wasm anywhere else is named by its file
    let found = (real.ancestors().skip(1).take_while(|a| a.starts_with(&top)))
        .find(|a| a.join("Cargo.toml").is_file() && real.starts_with(a.join("target")));
    let package = found
        .and_then(|c| std::fs::read_to_string(c.join("Cargo.toml")).ok())
        .and_then(|m| super::cli::package_name(&m));
    let stem = real.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
    let name = package.unwrap_or(stem);
    super::cli::name_ok(&name).map_err(|e| format!("{wasm}: {e}"))?;
    let loaded = Plugin::new(&bytes, tarae_wasm::Limits::default(), FUEL).and_then(|mut p| {
        let permissions = p.permissions(FUEL)?;
        if let Some(why) = super::perm::invalid(&permissions) {
            return Err(why);
        }
        Ok((permissions, p.commands(FUEL)?, p.subscriptions(FUEL)?))
    });
    let (permissions, commands, subscriptions) =
        loaded.map_err(|e| format!("{wasm} can't be installed: {}", super::shown(&e)))?;
    let events = subscriptions
        .iter()
        .map(|s| match s.languages.is_empty() {
            true => s.event.name().to_string(),
            false => format!("{} ({})", s.event.name(), super::shown(&s.languages.join(", "))),
        })
        .collect();
    Ok(Install {
        call: None,
        sha256: crate::sha256::hex(&bytes),
        from: shown_in(root, &real),
        crate_dir: found
            .map(|c| shown_in(root, c))
            .map(|c| if c.is_empty() { "the project".into() } else { c }),
        replaces: dir.join(format!("{name}.wasm")).exists(),
        name,
        bytes,
        permissions,
        commands: commands.iter().map(|c| super::shown(&c.name)).collect(),
        events,
        dir: dir.to_path_buf(),
    })
}

/// At most `max` bytes from `r` — None past that: a file growing since its size was taken stops here, never read whole.
fn at_most(r: impl std::io::Read, max: u64) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    r.take(max + 1).read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= max).then_some(bytes))
}

/// The look at a crate to start (on a worker): its folder in the project and not there yet — from the root, by real
/// names — as shown, its files. (Judged again as it's written, `cli::write_crate`: a link put on the way since is
/// refused.)
type Starting = (PathBuf, String, Vec<(&'static str, String)>);

fn new_look(root: &Path, dir: &str, from: Option<&str>) -> Result<Starting, String> {
    let path = root.join(dir);
    let outside = || format!("{dir} is outside the project");
    let real = crate::paths::inside(root, &path).ok_or_else(outside)?;
    if real.exists() {
        return Err(format!("{dir} is there already"));
    }
    let top = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let rel = real.strip_prefix(&top).map_err(|_| outside())?.to_path_buf();
    let name = real.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let files = super::cli::crate_files(name, from)?;
    Ok((rel, shown_in(root, &real), files))
}

/// The install's card: where it was found, what it registers, what it may do in the load card's words, what it
/// replaces, and what the yes binds — never its hash (#458: nobody compares hex).
fn install_card(i: &Install) -> Offer {
    let title = match i.call {
        Some(_) => format!("Claude asks to install plugin {}", i.name),
        None => format!("Install plugin {}", i.name),
    };
    let mut lines = vec![match &i.crate_dir {
        Some(c) => format!("From {} (crate {c})", i.from),
        None => format!("From {}", i.from),
    }];
    lines.push(match i.commands.is_empty() {
        true => "No commands".into(),
        false => format!(
            "Its commands: {}",
            i.commands.iter().map(|c| format!(":{c}")).collect::<Vec<_>>().join(" · ")
        ),
    });
    if !i.events.is_empty() {
        lines.push(format!("On {}", i.events.join(" · ")));
    }
    let first = lines.len();
    match i.permissions.is_empty() {
        true => lines.push("Nothing beyond the buffer.".into()),
        false => {
            lines.push("It asks to:".into());
            lines.extend(i.permissions.iter().map(|p| format!("· {}", super::perm::describe(p))));
        }
    }
    let decided = first..lines.len();
    if i.replaces {
        lines.push(format!("Replaces the {} plugin installed now.", i.name));
    }
    lines.push("For this build only — any change asks again.".into());
    let mut o = Offer::new(
        title,
        &lines.join("\n"),
        "Installing…",
        format!("Couldn't install plugin {}", i.name),
        What::PluginInstall(Box::new(i.clone())),
    )
    .deciding(decided);
    // (its first line keeps its end: the file's name)
    o.ends = vec![0];
    o
}

fn new_card(n: &New) -> Offer {
    // (the vendored SDK one entry: its files are this tarae's, the same in every crate)
    let mut paths: Vec<&str> = n.files.iter().map(|(p, _)| *p).filter(|p| !p.starts_with("tarae/")).collect();
    if n.files.iter().any(|(p, _)| p.starts_with("tarae/")) {
        paths.push("tarae/ (this tarae's SDK)");
    }
    let from = n.from.as_ref().map(|e| format!(", from the example {e}")).unwrap_or_default();
    let q = format!(
        "In {}{from}:\n{}\nWritten by tarae — Cargo.toml and .cargo are names a run can't write.",
        n.shown,
        paths.join(" · ")
    );
    // (the title the act and the crate's name, the body where: the folder said once)
    let name = n.rel.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    Offer::new(
        format!("Claude asks to start plugin crate {name}"),
        &q,
        "Writing…",
        format!("Couldn't start {}", n.shown),
        What::PluginNew(Box::new(n.clone())),
    )
    .deciding(0..2)
}

/// How an install's approval is taken back when its bytes didn't land: the name, the bytes it approved and what was
/// approved before for that name, or none; or the early change, if the plugin thread hasn't started yet.
enum TakeBack {
    Store(String, String, Option<super::perm::Approval>),
    Early(Change),
}

impl Editor {
    /// `:plugin-install <wasm>` — the install's card, yours (focused at once): the same look and card as Claude's.
    pub fn plugin_install_typed(&mut self, wasm: &str) -> Result<(), String> {
        let wasm = wasm.trim().to_string();
        if wasm.is_empty() {
            return Err("which build? :plugin-install <path to the .wasm>".into());
        }
        let dir = plugins_dir().ok_or("no plugins folder here (set HOME or XDG_CONFIG_HOME)")?;
        let root = self.workspace.root.clone();
        self.events.jobs().spawn(move || {
            let r = look(&root, &wasm, &dir);
            move |ed: &mut Editor| match r.and_then(|i| one_at_a_time(ed, i)) {
                Ok(i) => ed.push_offer(install_card(&i)),
                Err(e) => ed.set_error(e),
            }
        });
        Ok(())
    }

    /// `:plugin-new <dir> [--from <example>]` — the crate written at once (your act: `Cargo.toml` and `.cargo` are yours
    /// to write), in the project, through no link as Claude's is; its `src/lib.rs` opened.
    pub fn plugin_new_typed(&mut self, rest: &str) -> Result<(), String> {
        let usage = "usage: :plugin-new <dir> [--from <example>]";
        let words: Vec<&str> = rest.split_whitespace().collect();
        let (dir, from) = match words[..] {
            [dir] => (dir.to_string(), None),
            [dir, "--from", e] => (dir.to_string(), Some(e.to_string())),
            _ => return Err(usage.into()),
        };
        let root = self.workspace.root.clone();
        let path = root.join(&dir);
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        let files = super::cli::crate_files(&name, from.as_deref())?;
        self.events.jobs().spawn(move || {
            let r = super::cli::write_crate(&root, Path::new(&dir), &files);
            move |ed: &mut Editor| match r {
                Ok(()) => {
                    ed.set_success(format!("Started plugin {name} in {dir} — the API is its REFERENCE.md"));
                    ed.open_file(&path.join("src/lib.rs"));
                }
                Err(e) => ed.set_error(e),
            }
        });
        Ok(())
    }

    /// The install's `y` approves its bytes: in memory now (the store behind, `approvals_change`), or — the plugin
    /// thread not started yet — as it starts, before any plugin loads. Nothing to approve for a plugin that asks for
    /// nothing beyond the buffer.
    fn install_approve(&mut self, i: &Install) -> Option<TakeBack> {
        if i.permissions.is_empty() {
            return None;
        }
        let granted = super::perm::keys(&i.permissions);
        let code = Code { sha256: i.sha256.clone(), dev: None };
        let c = Change::Approve { plugin: i.name.clone(), granted, code };
        match &self.plugins {
            Some(p) => {
                let before = p.approvals.get(&i.name).cloned();
                self.approvals_change(c);
                Some(TakeBack::Store(i.name.clone(), i.sha256.clone(), before))
            }
            None => {
                self.plugin_early.push(c.clone());
                Some(TakeBack::Early(c))
            }
        }
    }

    /// The approval taken back: its bytes didn't land — only while it's still the one the install gave (a revoke, or
    /// another tarae's answer since, stands).
    fn install_take_back(&mut self, t: TakeBack) {
        let ours = |ed: &Editor, plugin: &str, sha: &str| {
            let p = ed.plugins.as_ref();
            p.and_then(|p| p.approvals.get(plugin)).is_some_and(|a| a.sha256 == sha && a.dev.is_none())
        };
        match t {
            TakeBack::Early(c) if self.plugin_early.contains(&c) => self.plugin_early.retain(|x| *x != c),
            TakeBack::Early(Change::Approve { plugin, code, .. }) if ours(self, &plugin, &code.sha256) => {
                self.approvals_change(Change::Revoke { plugin })
            }
            TakeBack::Early(_) => {}
            TakeBack::Store(plugin, sha, _) if !ours(self, &plugin, &sha) => {}
            TakeBack::Store(plugin, _, Some(a)) => {
                let code = Code { sha256: a.sha256, dev: a.dev };
                self.approvals_change(Change::Approve { plugin, granted: a.granted, code });
            }
            TakeBack::Store(plugin, _, None) => self.approvals_change(Change::Revoke { plugin }),
        }
    }
}

/// What `sim/plugininstall.rs` reads: the sha256 of the bytes an install card shows; a plugin's approval in memory
/// (its sha256, and whether it's a dev one).
#[cfg(test)]
impl Editor {
    pub(crate) fn plugin_install_shown(&self, o: &Offer) -> Option<String> {
        match &o.what {
            What::PluginInstall(i) => Some(i.sha256.clone()),
            _ => None,
        }
    }

    pub(crate) fn plugin_install_approval(&self, plugin: &str) -> Option<(String, bool)> {
        let a = self.plugins.as_ref()?.approvals.get(plugin)?;
        Some((a.sha256.clone(), a.dev.is_some()))
    }
}

/// The install card's `y`: those bytes approved, then written beside the plugins folder's file and renamed into place on
/// a worker — the load the rename starts finds them approved.
pub(crate) fn accept_install(ed: &mut Editor, id: u64, i: Install) {
    let take_back = ed.install_approve(&i);
    let (dir, name, bytes) = (i.dir.clone(), i.name.clone(), i.bytes.clone());
    ed.events.jobs().spawn(move || {
        let r = place(&dir, &name, &bytes);
        move |ed: &mut Editor| installed(ed, id, i, take_back, r)
    });
}

/// `bytes` as `<name>.wasm` in `dir`, through a temporary name: tarae never reads a half-written plugin.
fn place(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let dest = dir.join(format!("{name}.wasm"));
    let tmp = dir.join(format!(".{name}.wasm.tmp"));
    let r = std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&tmp, bytes))
        .and_then(|()| std::fs::rename(&tmp, &dest));
    r.map(|()| dest).map_err(|e| {
        std::fs::remove_file(&tmp).ok();
        format!("writing it in {}: {e}", crate::allow::tilde(dir))
    })
}

fn installed(ed: &mut Editor, id: u64, i: Install, take_back: Option<TakeBack>, r: Result<PathBuf, String>) {
    ed.offer_finished(id, Ok(()));
    let answer = match r {
        Ok(_) => {
            ed.set_success(format!(
                "Plugin {} installed — for this build only, any change asks again",
                i.name
            ));
            Ok("the user said yes — installed; it loads now. A rebuild is installed by asking again."
                .to_string())
        }
        Err(e) => {
            if let Some(t) = take_back {
                ed.install_take_back(t);
            }
            let why = format!("Plugin {} not installed — {e}", i.name);
            ed.set_error(why.clone());
            Err(why)
        }
    };
    if let Some(call) = i.call {
        crate::tools::answer(ed, call, answer);
    }
}

/// The install card's `n`: nothing written, nothing approved.
pub(crate) fn decline_install(ed: &mut Editor, i: Install) {
    ed.set_status(format!("Plugin {} not installed", i.name));
    if let Some(call) = i.call {
        crate::tools::answer(ed, call, Ok("the user said no — nothing was installed".into()));
    }
}

/// The crate card's `y`: its files written on a worker, through no link.
pub(crate) fn accept_new(ed: &mut Editor, id: u64, n: New) {
    let (root, rel, files) = (n.root.clone(), n.rel.clone(), n.files.clone());
    ed.events.jobs().spawn(move || {
        let r = super::cli::write_crate(&root, &rel, &files);
        move |ed: &mut Editor| {
            ed.offer_finished(id, Ok(()));
            let answer = match r {
                Ok(()) => {
                    ed.set_success(format!("Started plugin crate {} — the API is its REFERENCE.md", n.shown));
                    Ok(format!(
                        "the user said yes — {} is made; build it with `cargo build --release` there",
                        n.shown
                    ))
                }
                Err(e) => {
                    ed.set_error(format!("Couldn't start {}: {e}", n.shown));
                    Err(e)
                }
            };
            crate::tools::answer(ed, n.call, answer);
        }
    });
}

pub(crate) fn decline_new(ed: &mut Editor, n: New) {
    ed.set_status(format!("No crate made in {}", n.shown));
    crate::tools::answer(ed, n.call, Ok("the user said no — nothing was made".into()));
}

#[cfg(test)]
mod tests;
