//! Plugins: WebAssembly modules on tarae's own runtime (`tarae_wasm`, crates/tarae-wasm), speaking the API in
//! `plugin-sdk/wit/tarae.wit`. They live in `~/.config/tarae/plugins/*.wasm`.
//!
//! - `tarae_wasm::plugin`: `cabi` the canonical ABI's values in a plugin's memory, `abi` the API's types and
//!   loading and calling one plugin, `host` the plugin thread (judging permissions by `perm::invalid`, passed to
//!   it). `cli.rs`: `tarae plugin new|dev|test|vendor|list`. Here: the editor's side — commands and answers.
//! - **Nothing when unused**: the config watcher looks at the plugins folder (`host::Watch`), and the plugin
//!   thread starts the first time there's something in it.
//! - **Commands**: each plugin's commands become `:name args`, `":name"` in a keymap, and palette entries.
//!   A name tarae already uses, or another plugin took first, is skipped with a warning.
//! - **Answers**: actions apply in order. The edits among them — `replace-selections`, `edits`, `select` —
//!   are one transaction (one undo step) where the first of them is, on the buffer and selections as the
//!   plugin saw them: if the text changed meanwhile, or the user is typing in it, they're refused. The
//!   buffer in front stays in front.
//! - **Nothing a plugin says is trusted**: `run` takes only movement commands (so a plugin can't reach
//!   `:sh`, `:w` or anything else with effects outside the buffer), text is shown without control
//!   characters (no terminal escapes) and capped, and names are checked.
//! - **What a buffer gives a plugin** (#457): on its own (events, what it asked for coming back) only a buffer its
//!   grant covers (`plugin_covers`: its `read` patterns or `project`, approved; its own views) — any other is sent as
//!   nothing, and has no events but an empty `shown`. A command you run gets the buffer in front, and its answer's
//!   edits and moves apply there; never a file named like a key, nor a surface of tarae's own (`plugin_never`: the
//!   command is refused, said). It opens only a file its grant covers.

mod cli;
mod events;
pub(crate) mod examples;
mod install;
mod io;
pub(crate) mod keys;
pub(crate) mod overlay;
mod panel;
mod perm;
mod reference;
// (the plugin API and its thread are the runtime's crate's, crates/tarae-wasm — here, the editor's side)
use tarae_wasm::plugin::{abi, host};

use std::path::{Path, PathBuf};

pub use cli::cli;
pub use install::Install as PluginInstall;
#[cfg(test)]
pub(crate) use install::asks_title as install_asks_title;
#[cfg(test)]
pub(crate) use install::install_card;
#[cfg(test)]
pub(crate) use install::stand_in as install_stand_in;
pub(crate) use install::{accept_install, decline_install};
pub(crate) use install::{schema as tool_schema, tool_call};
/// (the simulation's builds: the demo asking for a set — `abi::demo_asking`)
#[cfg(test)]
pub(crate) fn demo_asking_for_sim(set: u8) -> Vec<u8> {
    abi::demo_asking(set)
}
#[cfg(test)]
pub use host::scan;
pub use host::{Found, Watch};
pub(crate) use perm::Change as PluginChange;

use abi::Action;
pub use abi::Event as PluginEvent;
pub use abi::HostEvent as PluginHostEvent;
pub use abi::Permission as PluginPermission;
pub use abi::Role as OverlayRole;
// (the simulation shows overlays as an answer would)
#[cfg(test)]
pub use abi::{Overlay as PluginOverlay, Span as OverlaySpan};
// (the sandbox's tests start a stand-in the way a plugin's spawn does)
pub use abi::Side as PluginSide;
pub use abi::Tone as PluginTone;
use host::{Call, Host, Input, Manifest, Note};
#[cfg(all(test, target_os = "linux"))]
pub(crate) use io::plugin_process;
pub use panel::{PanelState as PluginPanelState, key as plugin_panel_key, row_ids as plugin_panel_ids};
pub use perm::Ask as PluginAsk;
#[cfg(test)]
pub(crate) use perm::kept_files as approvals_kept_files;
pub use perm::{ApprovalsWatch, QUIT_WAIT};

/// Where the plugins' approvals are kept (none in tests) — what the config thread watches (#461).
pub fn approvals_store() -> Option<PathBuf> {
    perm::store_path()
}
pub use perm::Code as PluginCode;

use crate::document::DocId;
use crate::editor::Editor;
use crate::event::Event;
use crate::selection::{Range, Selection};
use crate::transaction::{Change, Transaction};

pub fn dir() -> Option<PathBuf> {
    Some(crate::config::config_path()?.parent()?.join("plugins"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginCommand {
    pub name: String,
    pub doc: String,
    pub plugin: String,
}

/// The most commands one plugin registers.
const MAX_COMMANDS: usize = 256;
#[cfg(test)]
use tarae_plugin_rules::MAX_TEXT;
/// Plugin text fit for the screen (control and reordering characters replaced, and capped) — the shared rules',
/// which the SDK's mock host says its words with too.
pub(crate) use tarae_plugin_rules::shown;

/// Why a plugin's command name can't be registered, if it can't.
fn bad_name(name: &str) -> Option<&'static str> {
    if name.is_empty()
        || name.chars().count() > 64
        || name.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        Some("isn't a command name")
    } else if name.parse::<usize>().is_ok() || crate::cmdline::find(name).is_some() {
        Some("is tarae's own")
    } else {
        None
    }
}

/// The longest status-line text from a plugin (in characters).
const MAX_STATUS: usize = 60;

/// The editor's plugin state: the thread, the commands the plugins registered, what they show in the
/// status line (per plugin and buffer), and the events they subscribed to.
pub struct Plugins {
    host: Host,
    pub commands: Vec<PluginCommand>,
    status: std::collections::BTreeMap<(String, DocId), String>,
    events: events::Events,
    /// What each plugin may do beyond the buffer (as it says), and the programs it runs, by (plugin, id).
    permissions: std::collections::HashMap<String, Vec<abi::Permission>>,
    /// Its `read` patterns with their folders resolved, on the plugin thread as it loaded (`perm::roots`, #457).
    roots: std::collections::HashMap<String, Vec<(String, Option<PathBuf>)>>,
    /// The code each load is (#458): its bytes' sha256 and, for a build its dev mark names, its folder — what an
    /// approval is held against.
    code: std::collections::HashMap<String, perm::Code>,
    procs: std::collections::HashMap<(String, u64), io::Proc>,
    /// Its connections (open, and being opened), and its timers — by (plugin, id), with their run's serial.
    socks: std::collections::HashMap<(String, u64), io::Sock>,
    pending: std::collections::HashMap<(String, u64), u64>,
    timers: std::collections::HashMap<(String, u64), (std::time::Instant, u64)>,
    clock: io::Clock,
    /// What's on its way to each plugin (for its current load).
    outbox: std::collections::HashMap<String, io::Outbox>,
    /// What you approved (kept), and the plugins asked about since tarae started.
    approvals: std::collections::BTreeMap<String, perm::Approval>,
    asked: std::collections::HashSet<String>,
    /// What each plugin asked for while its approval card was up: done once you approve, refused if you don't.
    held: std::collections::HashMap<String, Vec<Action>>,
    /// Where approvals are kept (#461: written behind memory, on workers) — the changes not written yet, in the order
    /// given (queued, or failed and to be tried again), the last given's sequence, the store's `served` of the newest
    /// read or write applied to memory, the failures in a row, and whether a retry's timer is set.
    perm_store: Option<PathBuf>,
    perm_unwritten: perm::Unwritten,
    perm_given: u64,
    perm_served: u64,
    perm_failed: usize,
    perm_retrying: bool,
    serial: u64,
    /// The panel a plugin shows below the code (the last one to show one), and the plugin whose answer to
    /// you (to a command, a key in its panel, a line typed) is being applied: its panel takes the keys, and
    /// it may ask for a line.
    pub panel: Option<panel::PanelState>,
    answering: Option<String>,
    /// Its lines being typed at and its picks (by token) — with the buffer your command ran on, if one asked for it in
    /// its answer (#457: an answer there is yours, once) —, and the buffers it shows (by its id).
    prompts: std::collections::HashMap<u64, (String, u64, Option<DocId>)>,
    views: std::collections::HashMap<(String, u64), DocId>,
    /// What plugins write beside buffers' lines, by (plugin, path): the version they're for, and the hints.
    hints: std::collections::HashMap<(String, String), (u64, Vec<abi::Hint>)>,
    /// The side panel's plugin tab (the last plugin to show one), and the status line's plugin text.
    pub side: Option<(String, abi::Side)>,
    pub badge: Option<(String, String, abi::Tone)>,
    /// Each plugin's overlay while it's up (`overlay.rs`), by plugin.
    pub overlays: std::collections::BTreeMap<String, overlay::Shown>,
    /// The tools plugins offered to install since tarae started (once each).
    pub(crate) offered: std::collections::HashSet<String>,
    /// Where each plugin's own folder is (`<this>/<plugin>`): the state folder's `plugins` (a test's scratch's).
    /// Approvals' `perm_store` stays apart: none in tests unless a test sets one.
    own_root: Option<PathBuf>,
    /// What each call from `plugin_flush` gave it of the buffer in front: its path, and whether its answer counts as
    /// yours (#457's tests).
    #[cfg(test)]
    flushed: Vec<(Option<String>, bool)>,
}

impl Plugins {
    /// Stops what a plugin started: its programs, connections and timers.
    fn stop_all(&mut self, plugin: &str) {
        self.procs.retain(|(p, _), _| p != plugin);
        self.socks.retain(|(p, _), _| p != plugin);
        self.pending.retain(|(p, _), _| p != plugin);
        let clock = &self.clock;
        self.timers.retain(|(p, _), at| {
            let keep = p != plugin;
            if !keep {
                clock.cancel(*at);
            }
            keep
        });
    }

    /// Forgets what a plugin registered (it's gone, or reloading).
    fn drop_plugin(&mut self, plugin: &str) {
        self.commands.retain(|c| c.plugin != plugin);
        self.status.retain(|(p, _), _| p != plugin);
        self.events.subs.retain(|(p, ..)| p != plugin);
        self.permissions.remove(plugin);
        self.roots.remove(plugin);
        self.code.remove(plugin);
        self.held.remove(plugin);
        self.outbox.remove(plugin);
        self.stop_all(plugin);
        if self.panel.as_ref().is_some_and(|s| s.plugin == plugin) {
            self.panel = None;
        }
        self.prompts.retain(|_, (p, ..)| p != plugin);
        self.views.retain(|(p, _), _| p != plugin);
        self.hints.retain(|(p, _), _| p != plugin);
        if self.side.as_ref().is_some_and(|(p, _)| p == plugin) {
            self.side = None;
        }
        if self.badge.as_ref().is_some_and(|(p, ..)| p == plugin) {
            self.badge = None;
        }
        self.overlays.remove(plugin);
    }
}

impl Editor {
    /// The plugins folder's contents changed (the config watcher noticed): load new and changed plugins, drop
    /// missing ones. The first time there's something, the plugin thread starts.
    /// `kept`: the approvals store as the watch last read it — the start's approvals, read on the config thread with
    /// the plugins first found (#461: no load reads the store; memory is set from it once, before any answer).
    pub fn plugins_sync(&mut self, found: Found, kept: Option<perm::Kept>) {
        self.plugins_sync_rooted(found, crate::config::state_dir(), kept);
    }

    /// `plugins_sync` with the plugins' state in a test's own scratch (`state`): their own folders
    /// (`state/plugins/<plugin>`). The host started any other way fails the test (`plugins_sync_rooted`).
    #[cfg(test)]
    pub(crate) fn plugins_sync_in(&mut self, state: &std::path::Path, found: Found) {
        self.plugins_sync_rooted(found, Some(state.to_path_buf()), None);
    }

    /// `plugins_sync`, the plugin thread started with `state` as the state folder its plugins keep their files in.
    fn plugins_sync_rooted(&mut self, found: Found, state: Option<PathBuf>, kept: Option<perm::Kept>) {
        if self.plugins.is_none() && !found.is_empty() {
            // A test's plugins write in its own scratch, never the real state folder (a `write-own` let past its
            // gate once reached for `~/.local/state/tarae/plugins/demo`): `plugins_sync_in`
            #[cfg(test)]
            assert!(
                state.as_deref().is_some_and(crate::scratch::holds),
                "the plugin host rooted at {state:?}, outside the test's scratch — start it with `plugins_sync_in`"
            );
        }
        let starts = self.plugins.is_none();
        if starts && (found.is_empty() || !self.plugins_start(state, kept.clone().unwrap_or_default())) {
            return;
        }
        if let Some(kept) = kept.filter(|_| !starts) {
            self.plugin_approvals_read(kept);
        }
        if let Some(p) = &self.plugins {
            p.host.sync(found);
        }
    }

    /// The plugin thread started with `state` as the state folder its plugins keep their files in (None: none — a
    /// test's stand-in that loads no plugin), nothing loaded yet, the approvals `kept` in memory. false if it couldn't
    /// start (said).
    fn plugins_start(&mut self, state: Option<PathBuf>, kept: perm::Kept) -> bool {
        let events = self.events.shared_sender();
        // (`tarae plugin dev`'s marks: `state/plugin-dev/<plugin>.json` — #458)
        let marks = state.as_ref().map(|s| s.join("plugin-dev"));
        let started = Host::start(
            move |note| {
                // (its `read` patterns resolved here, on the plugin thread: the gate never waits on the disk — #457;
                // and its dev mark read, so an approval is held against memory only — #458)
                let (roots, dev) = match &note {
                    Note::Loaded { plugin, manifest: Ok(m), code, .. } => (
                        perm::roots(&m.permissions),
                        marks.as_deref().and_then(|d| perm::dev_source(d, plugin, code)),
                    ),
                    _ => (Vec::new(), None),
                };
                let _ = events
                    .send(Event::Job(Box::new(move |ed: &mut Editor| ed.plugin_note(note, roots, dev))));
            },
            perm::invalid,
            crate::sha256::hex,
        );
        let Some(host) = started else {
            self.set_error("plugins: couldn't start their thread");
            return false;
        };
        self.plugins = Some(Plugins {
            host,
            commands: Vec::new(),
            status: Default::default(),
            events: Default::default(),
            permissions: Default::default(),
            roots: Default::default(),
            code: Default::default(),
            approvals: kept.all,
            asked: Default::default(),
            held: Default::default(),
            perm_store: perm::store_path(),
            perm_unwritten: Vec::new(),
            perm_given: 0,
            perm_served: kept.served,
            perm_failed: 0,
            perm_retrying: false,
            procs: Default::default(),
            socks: Default::default(),
            pending: Default::default(),
            timers: Default::default(),
            clock: Default::default(),
            outbox: Default::default(),
            serial: 0,
            panel: None,
            answering: None,
            prompts: Default::default(),
            views: Default::default(),
            hints: Default::default(),
            side: None,
            badge: None,
            overlays: Default::default(),
            offered: Default::default(),
            own_root: state.map(|s| s.join("plugins")),
            #[cfg(test)]
            flushed: Vec::new(),
        });
        self.plugins_given();
        true
    }

    /// As the plugins' side starts, before any plugin loads: `:restart`'s revokes not written before its exec given
    /// again, and an install's yes given before it started (the first plugin's, `install.rs`).
    fn plugins_given(&mut self) {
        for plugin in std::mem::take(&mut self.plugin_revokes_carried) {
            if self.plugins.as_ref().is_some_and(|p| p.approvals.contains_key(&plugin)) {
                self.approvals_change(perm::Change::Revoke { plugin });
            }
        }
        for c in std::mem::take(&mut self.plugin_early) {
            self.approvals_change(c);
        }
    }

    /// The plugins' side with no plugin thread (`Host::detached`): what a simulation drives by hand — an overlay shown
    /// through `plugin_overlay` as an answer would.
    #[cfg(test)]
    pub(crate) fn plugins_detached(&mut self) {
        self.plugins = Some(Plugins {
            host: Host::detached(),
            commands: Vec::new(),
            status: Default::default(),
            events: Default::default(),
            permissions: Default::default(),
            roots: Default::default(),
            code: Default::default(),
            approvals: Default::default(),
            asked: Default::default(),
            held: Default::default(),
            perm_store: None,
            perm_unwritten: Vec::new(),
            perm_given: 0,
            perm_served: 0,
            perm_failed: 0,
            perm_retrying: false,
            procs: Default::default(),
            socks: Default::default(),
            pending: Default::default(),
            timers: Default::default(),
            clock: Default::default(),
            outbox: Default::default(),
            serial: 0,
            panel: None,
            answering: None,
            prompts: Default::default(),
            views: Default::default(),
            hints: Default::default(),
            side: None,
            badge: None,
            overlays: Default::default(),
            offered: Default::default(),
            own_root: None,
            #[cfg(test)]
            flushed: Vec::new(),
        });
        self.plugins_given();
    }

    /// What the plugin thread says: a plugin loaded (its `read` patterns' `roots` resolved there, and the folder `dev`
    /// its dev mark names it a build of), failed or gone.
    fn plugin_note(&mut self, note: Note, roots: Vec<(String, Option<PathBuf>)>, dev: Option<PathBuf>) {
        let Some(p) = &mut self.plugins else { return };
        match note {
            Note::Loaded { plugin, manifest: Err(e), .. } => {
                p.drop_plugin(&plugin);
                self.plugin_cards_close(&plugin);
                self.plugin_commands_changed();
                self.set_error(format!("plugin {plugin}: {}", shown(&e)));
            }
            Note::Loaded {
                plugin,
                manifest: Ok(Manifest { commands, subscriptions, permissions }),
                reload,
                generation,
                code,
            } => {
                p.drop_plugin(&plugin);
                p.outbox.insert(plugin.clone(), io::Outbox::new(generation));
                p.asked.remove(&plugin);
                p.permissions.insert(plugin.clone(), permissions.clone());
                p.roots.insert(plugin.clone(), roots);
                p.code.insert(plugin.clone(), perm::Code { sha256: code, dev });
                events::subscribe(&mut p.events.subs, &plugin, subscriptions);
                crate::sorting::by(&mut p.events.subs, |a, b| a.0.cmp(&b.0));
                let mut taken: std::collections::HashSet<String> =
                    p.commands.iter().map(|c| c.name.clone()).collect();
                let mut skipped = Vec::new();
                let mut added = Vec::new();
                let too_many = commands.len().saturating_sub(MAX_COMMANDS);
                for c in commands.into_iter().take(MAX_COMMANDS) {
                    let why = bad_name(&c.name).or_else(|| taken.contains(&c.name).then_some("is taken"));
                    match why {
                        Some(why) => skipped.push(format!("“{}” {why}", shown(&c.name))),
                        None => {
                            added.push(format!(":{}", c.name));
                            taken.insert(c.name.clone());
                            p.commands.push(PluginCommand {
                                name: c.name,
                                doc: shown(&c.doc),
                                plugin: plugin.clone(),
                            });
                        }
                    }
                }
                if too_many > 0 {
                    skipped.push(format!("{too_many} commands past the first {MAX_COMMANDS}"));
                }
                self.plugin_commands_changed();
                // A card from before it loaded again asks about what it asked then: its asks go, its approval card
                // is rewritten for this code (or goes) — the card up is always about the code that's loaded (#458)
                self.plugin_asks_close(&plugin);
                self.plugin_card_renew(&plugin, false);
                self.plugin_permissions_loaded(&plugin, &permissions);
                if !skipped.is_empty() {
                    self.set_warning(format!("plugin {plugin}: skipped {}", skipped.join(", ")));
                }
                // Loading at start is routine; a reload is what `tarae plugin dev` waits to see
                if reload {
                    self.set_success(format!("Plugin {plugin} reloaded  {}", added.join(" ")));
                }
            }
            Note::Removed { plugin } => {
                p.drop_plugin(&plugin);
                self.plugin_cards_close(&plugin);
                self.plugin_commands_changed();
            }
        }
    }

    /// Keeps `:` completion in step with the registered commands.
    fn plugin_commands_changed(&mut self) {
        let cmds = self.plugins.as_ref().map(|p| p.commands.as_slice()).unwrap_or_default();
        self.cmd_lists.plugins =
            cmds.iter().map(|c| (c.name.clone(), format!("{}  · {}", c.doc, c.plugin))).collect();
    }

    pub fn plugin_command(&self, name: &str) -> Option<&PluginCommand> {
        self.plugins.as_ref()?.commands.iter().find(|c| c.name == name)
    }

    /// Runs a plugin command on the current buffer. Here only the rope is cloned (O(1)) and the selections
    /// copied — the plugin thread does the rest, and the answer comes back as an event.
    pub fn plugin_run(&mut self, name: &str, args: &str) -> Result<(), String> {
        let cmd = self.plugin_command(name).cloned().ok_or_else(|| format!("no such command: '{name}'"))?;
        if let Some(why) = self.plugin_never(&cmd.plugin, self.doc()) {
            return Err(format!(
                ":{name} isn't given {}: {why}",
                self.doc().display_name(&self.workspace.root)
            ));
        }
        let (input, seen) =
            self.plugin_input(&cmd.plugin, self.doc().id, args, true).expect("the current buffer is open");
        let events = self.events.shared_sender();
        let plugin = cmd.plugin.clone();
        let reply = Box::new(move |result: Result<Vec<Action>, String>| {
            let apply = move |ed: &mut Editor| match result {
                Ok(actions) => {
                    // A panel this command shows takes the keys
                    if let Some(p) = &mut ed.plugins {
                        p.answering = Some(plugin.clone());
                    }
                    ed.plugin_apply(&plugin, &seen, actions);
                    if let Some(p) = &mut ed.plugins {
                        p.answering = None;
                    }
                }
                Err(e) => ed.set_error(format!("{plugin}: {}", shown(&e))),
            };
            let _ = events.send(Event::Job(Box::new(apply)));
        });
        let p = self.plugins.as_ref().ok_or("plugins aren't running")?;
        p.host.call(&cmd.plugin, Call::Command(name.to_string()), input, reply)
    }

    /// What `plugin` is given about buffer `doc`, and what its answer applies to — `yours`: a command you ran on it
    /// (`plugin_run`, which refuses what's never given); else the buffer only if its grant covers it, an empty one
    /// otherwise (no path, name, text or selections). Only the rope is cloned (O(1)) and the selections copied: the
    /// plugin thread makes the strings.
    fn plugin_input(&self, plugin: &str, doc: DocId, args: &str, yours: bool) -> Option<(Input, Seen)> {
        let doc = self.docs.iter().find(|d| d.id == doc)?;
        let given =
            self.plugin_covers_doc(plugin, doc) || (yours && self.plugin_never(plugin, doc).is_none());
        if !given {
            let input = Input {
                args: args.to_string(),
                text: ropey::Rope::new(),
                ranges: Vec::new(),
                primary: 0,
                path: None,
                name: String::new(),
                language: None,
                version: 0,
                modified: false,
            };
            return Some((
                input,
                Seen { doc: doc.id, version: doc.version(), ranges: Vec::new(), yours: false },
            ));
        }
        let sel = doc.selection();
        let input = Input {
            args: args.to_string(),
            text: doc.text.clone(),
            ranges: sel.ranges().iter().map(|r| (r.anchor, r.head)).collect(),
            primary: sel.primary_index(),
            path: doc.path.as_ref().map(|p| p.display().to_string()),
            name: doc.display_name(&self.workspace.root),
            language: doc.lang_name().map(str::to_string),
            version: doc.version(),
            modified: doc.is_modified(),
        };
        Some((input, Seen { doc: doc.id, version: doc.version(), ranges: sel.ranges().to_vec(), yours }))
    }

    /// Whether a plugin's own grant covers buffer `doc` (#457's rule (a)): its file, or a view of the plugin's.
    pub(super) fn plugin_covers_doc(&self, plugin: &str, doc: &crate::document::Document) -> bool {
        match &*doc.path {
            Some(path) => self.plugin_covers(plugin, None, path),
            None => self
                .plugins
                .as_ref()
                .is_some_and(|p| p.views.iter().any(|((q, _), d)| q == plugin && *d == doc.id)),
        }
    }

    /// Why buffer `doc` is never given to a plugin, even when you run its command on it (#457): a file named like a
    /// key, or a surface of tarae's own — a buffer with no file that's neither yours (`Document::scratch`) nor the
    /// plugin's view: a run's output, a message, a quote whole, a web page.
    fn plugin_never(&self, plugin: &str, doc: &crate::document::Document) -> Option<&'static str> {
        match &*doc.path {
            Some(path) if crate::secretnames::named_secret(path) => Some("a file named like a key"),
            Some(_) => None,
            None if doc.scratch || self.plugin_covers_doc(plugin, doc) => None,
            None => Some("one of tarae's own views"),
        }
    }

    /// Opens for a plugin `asked` (as it wrote it, from the root if relative; `path:line`, from 1, where the whole
    /// isn't a file) — or at `line` (from 0: a side-tab row's place) —, then `then` once it's in front. Only a file
    /// its grant covers (#457): else your next key on a command of its would hand it the text. Refused and said at once
    /// when it reads no file at all, and for a buffer open at that path (its path made real as it opened); else which
    /// file, its real path and the grant are decided on the open's worker (no disk on the loop), and a refusal is the
    /// open's own failure, said the same.
    pub(crate) fn plugin_open(
        &mut self,
        plugin: &str,
        asked: &str,
        line: Option<usize>,
        then: impl FnOnce(&mut Editor) + Send + 'static,
    ) -> Result<(), String> {
        let refused = tarae_plugin_rules::may_not_open;
        let reach = self.plugin_grant(plugin).map_err(|_| refused(asked))?;
        let whole = self.path_of(Path::new(asked));
        if let Some(i) = self.docs.first(&whole, |_| true) {
            if !self.plugin_covers_doc(plugin, &self.docs[i]) {
                return Err(refused(asked));
            }
            match line {
                Some(line) => self.open_at_then(&whole, line, 0, then),
                None => self.open_file_then(&whole, false, move |ed, r| match r {
                    Ok(_) => then(ed),
                    Err(e) => ed.open_failed(e),
                }),
            }
            return Ok(());
        }
        // (`path:line`: the file before the colon, if the whole isn't one — the worker's to see)
        let split = asked
            .rsplit_once(':')
            .filter(|_| line.is_none())
            .and_then(|(file, l)| {
                Some((file.to_string(), self.path_of(Path::new(file)), l.parse::<usize>().ok()?))
            })
            .filter(|(.., l)| *l > 0);
        let (as_asked, who) = (asked.to_string(), plugin.to_string());
        let pick = move |whole: &Path| {
            let (written, file, line) = match split {
                Some((written, file, l)) if !perm::disk::exists(whole) => (written, file, Some(l - 1)),
                _ => (as_asked, whole.to_path_buf(), line),
            };
            match reach.covers(Some(&file), &perm::disk::real(&file)) {
                true => Ok((file, line)),
                false => Err(anyhow::anyhow!(refused(&written))),
            }
        };
        self.open_file_picked(&whole, pick, move |ed, r| match r {
            Ok(_) => then(ed),
            Err(e) => ed.set_error(format!("{who}: {e}")),
        });
        Ok(())
    }

    fn plugin_apply(&mut self, plugin: &str, seen: &Seen, actions: Vec<Action>) {
        let edits =
            |a: &Action| matches!(a, Action::ReplaceSelections(_) | Action::Edits(_) | Action::Select(_));
        let mut edited = false;
        // (what a plugin that hears keys heard stays in tarae: its overlay, its timers — `keys::refused`)
        let hears = self.plugin_hears_keys(plugin);
        for a in &actions {
            if let Some(what) = keys::refused(a).filter(|_| hears) {
                self.plugin_keys_refuse(plugin, a, what);
                continue;
            }
            if self.plugin_hold(plugin, a) {
                continue;
            }
            let r = match a {
                Action::Status(m) => {
                    self.set_status(shown(m));
                    Ok(())
                }
                Action::Success(m) => {
                    self.set_success(shown(m));
                    Ok(())
                }
                Action::Warning(m) => {
                    self.set_warning(shown(m));
                    Ok(())
                }
                Action::Error(m) => Err(shown(m)),
                Action::StatusBar(m) => {
                    if let Some(p) = &mut self.plugins {
                        let key = (plugin.to_string(), seen.doc);
                        let text: String = shown(m).replace('\n', " ").chars().take(MAX_STATUS).collect();
                        if text.trim().is_empty() {
                            p.status.remove(&key);
                        } else {
                            p.status.insert(key, text);
                        }
                    }
                    Ok(())
                }
                // Movement only: anything else could reach past the buffer (`:sh`, `:w`, …) — and only in a
                // buffer its grant covers, or the one your command ran on: where the cursor lands tells the text
                Action::Run(name) => match crate::commands::find(name) {
                    Some(c) if c.motion => {
                        let doc = self.doc();
                        if (seen.yours && doc.id == seen.doc) || self.plugin_covers_doc(plugin, doc) {
                            self.run_command_by_name(name);
                            Ok(())
                        } else {
                            Err("it moves only in a file it may read, or in answer to your command".into())
                        }
                    }
                    _ => Err(format!("plugins can run movement commands only, not “{}”", shown(name))),
                },
                Action::Open(path) if path.chars().any(char::is_control) => {
                    Err("a path with control characters".into())
                }
                // `path:line` (from 1): there, if `path` alone is the file
                Action::Open(path) => self.plugin_open(plugin, path, None, |_| {}),
                Action::Spawn(sp) => self.plugin_spawn(plugin, sp.clone()),
                Action::Stop(id) => {
                    self.plugin_stop(plugin, *id);
                    Ok(())
                }
                Action::ReadFile(id, path) => {
                    self.plugin_read(plugin, *id, path);
                    Ok(())
                }
                Action::ReadEnv(id, name) => {
                    let ev = match self.plugin_may(plugin, &abi::Permission::Env(name.clone())) {
                        Ok(()) => abi::HostEvent::Env(*id, std::env::var(name).ok()),
                        Err(why) => abi::HostEvent::Failed(*id, why),
                    };
                    self.plugin_events(plugin, vec![ev]);
                    Ok(())
                }
                Action::Connect(c) => {
                    match self.plugin_may_connect(plugin, c) {
                        Ok(()) => self.plugin_connect_now(plugin, c.clone()),
                        Err(None) => self.plugin_ask(plugin, perm::Ask::Connect(c.clone())),
                        Err(Some(why)) => self.plugin_events(plugin, vec![abi::HostEvent::Failed(c.id, why)]),
                    }
                    Ok(())
                }
                Action::Send(id, bytes) => {
                    self.plugin_send(plugin, *id, bytes.clone());
                    Ok(())
                }
                Action::Timer(id, ms) => {
                    self.plugin_timer(plugin, *id, *ms);
                    Ok(())
                }
                Action::Prompt(id, label) => {
                    self.plugin_prompt(plugin, *id, label, seen.yours.then_some(seen.doc));
                    Ok(())
                }
                Action::Show(view) => {
                    self.plugin_show(plugin, view.clone());
                    Ok(())
                }
                Action::Append(id, text) => self.plugin_append(plugin, *id, text),
                Action::Hints(h) => {
                    self.plugin_hints(plugin, h.clone());
                    Ok(())
                }
                Action::Side(s) => {
                    self.plugin_side(plugin, s.clone());
                    Ok(())
                }
                Action::Badge(b) => {
                    self.plugin_badge(plugin, b.clone());
                    Ok(())
                }
                Action::Find(id, pattern) => {
                    self.plugin_find(plugin, *id, pattern);
                    Ok(())
                }
                Action::WriteOwn(id, name, bytes) => {
                    self.plugin_write_own(plugin, *id, name, bytes);
                    Ok(())
                }
                Action::OfferInstall(id, tool) => {
                    self.plugin_offer_install(plugin, *id, tool);
                    Ok(())
                }
                Action::Pick(p) => {
                    self.plugin_pick(plugin, p.clone(), seen.yours.then_some(seen.doc));
                    Ok(())
                }
                Action::Overlay(o) => {
                    self.plugin_overlay(plugin, o.clone());
                    Ok(())
                }
                Action::Panel(panel) => {
                    self.plugin_panel(plugin, panel.clone());
                    Ok(())
                }
                // All of the answer's edits, where the first of them is
                _ if edited => Ok(()),
                _ => {
                    edited = true;
                    self.plugin_edit(plugin, seen, actions.iter().filter(|a| edits(a)))
                }
            };
            if let Err(e) = r {
                self.set_error(format!("{plugin}: {e}"));
            }
        }
    }

    /// One answer's edits as one undo step, if the buffer is as the plugin saw it — and its grant covers it, or the
    /// answer is to your command (#457: else a selection's place would tell the text). The buffer in front stays
    /// in front.
    fn plugin_edit<'a>(
        &mut self,
        plugin: &str,
        seen: &Seen,
        actions: impl Iterator<Item = &'a Action>,
    ) -> Result<(), String> {
        let i = self.docs.iter().position(|d| d.id == seen.doc).ok_or("the buffer was closed")?;
        if !seen.yours && !self.plugin_covers_doc(plugin, &self.docs[i]) {
            return Err(
                "it edits only a file it may read, or in answer to your command — its edits weren't applied"
                    .into(),
            );
        }
        if self.docs[i].version() != seen.version {
            return Err("the buffer changed while the plugin ran — its edits weren't applied".into());
        }
        if self.editing(seen.doc) || self.docs[i].loading {
            return Err("the buffer is being edited — the plugin's edits weren't applied".into());
        }
        let doc = &self.docs[i];
        let on_char = |text: &ropey::Rope, b: u64| {
            b <= text.len_bytes() as u64 && text.char_to_byte(text.byte_to_char(b as usize)) == b as usize
        };
        let mut changes = Vec::new();
        let mut select = None;
        for a in actions {
            match a {
                // The selections the plugin was given, whatever they are now
                Action::ReplaceSelections(texts) => {
                    if texts.len() != seen.ranges.len() {
                        return Err(format!("{} texts for {} selections", texts.len(), seen.ranges.len()));
                    }
                    changes.extend(seen.ranges.iter().zip(texts).map(|(r, t)| Change {
                        from: r.from(),
                        to: r.to(),
                        insert: t.clone(),
                    }));
                }
                Action::Edits(es) => {
                    for e in es {
                        if e.from > e.to || !on_char(&doc.text, e.from) || !on_char(&doc.text, e.to) {
                            return Err(format!(
                                "edit {}..{} isn't within the text, on character boundaries",
                                e.from, e.to
                            ));
                        }
                        changes.push(Change {
                            from: e.from as usize,
                            to: e.to as usize,
                            insert: e.insert.clone(),
                        });
                    }
                }
                Action::Select(ranges) if ranges.is_empty() => return Err("an empty selection list".into()),
                Action::Select(ranges) => select = Some(ranges.clone()),
                _ => {}
            }
        }
        crate::sorting::by_key(&mut changes, |c| (c.from, c.to));
        if changes.windows(2).any(|w| w[1].from < w[0].to) {
            return Err("its edits overlap".into());
        }
        let tx = Transaction::new(changes);
        // Selections are positions in the buffer after the edits: checked there before anything changes
        let selection = match select {
            None => None,
            Some(ranges) => {
                let mut after = doc.text.clone();
                tx.apply(&mut after, false);
                if let Some(&(a, h)) =
                    ranges.iter().find(|&&(a, h)| !on_char(&after, a) || !on_char(&after, h))
                {
                    return Err(format!("selection {a}..{h} isn't on character boundaries"));
                }
                Some(Selection::new(
                    ranges.iter().map(|&(a, h)| Range::new(a as usize, h as usize)).collect(),
                    0,
                ))
            }
        };
        let doc = &mut self.docs[i];
        let before = doc.snapshot();
        match selection {
            Some(sel) => doc.apply_with(&tx, sel),
            None => doc.apply(&tx),
        }
        doc.own.not_yours();
        doc.commit_undo(before);
        // A plugin's own edit isn't reported to `changed` subscribers (no plugin and edit calling each other)
        let (id, v) = (doc.id, doc.version());
        if let Some(p) = &mut self.plugins {
            p.events.by_plugin.insert(id, v);
        }
        Ok(())
    }
}

/// What a command's answer applies to: the buffer and selections as they were when it was asked — and whether you
/// asked (a command you ran: its answer edits and moves in that buffer whether the plugin's grant covers it or not).
#[derive(Clone)]
struct Seen {
    doc: DocId,
    version: u64,
    ranges: Vec<Range>,
    yours: bool,
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::abi::DEMO;
    use super::*;
    use crate::config::Config;

    /// The runtime's speed on a plugin under both engines (what the host-set interrupt flag costs is read here,
    /// `.sb/codemode-s1-flag-report.md`): the demo plugin's `trim-trailing` over 2 MB — its own Rust compiled to
    /// WebAssembly, the call's JSON both ways. Median of 5; reported only.
    #[test]
    fn perf_plugin_trim_2mb() {
        if crate::test_results::unoptimized("plugin::mod::tests::perf_plugin_trim_2mb") {
            return;
        }
        let text: String =
            (0..60_000).map(|i| format!("line {i} 타래 fn main() {{ let x = {i}; }}   \n")).collect();
        assert!(text.len() > 2 << 20);
        let cx = abi::Context {
            buffer: abi::Buffer { name: "a.rs".into(), text: Some(text), ..Default::default() },
            ..Default::default()
        };
        for (jit, engine) in [(true, "jit"), (false, "interp")] {
            let limits = tarae_wasm::Limits { jit, ..Default::default() };
            let mut p = abi::Plugin::new(DEMO, limits, 1 << 40).unwrap();
            let mut all: Vec<Duration> = (0..5)
                .map(|_| {
                    let t = Instant::now();
                    let done = p.run("trim-trailing", &cx, 1 << 40).unwrap();
                    let t = t.elapsed();
                    assert!(
                        matches!(&done[0], abi::Action::Success(s) if s == "60000 lines trimmed"),
                        "{done:?}"
                    );
                    t
                })
                .collect();
            all.sort_unstable();
            let ms = all[all.len() / 2].as_secs_f64() * 1e3;
            let line = format!("PERF plugin_trim_2mb_{engine}_ms {ms:.2} noise=1\n");
            let _ = std::io::Write::write_all(&mut std::io::stderr(), line.as_bytes());
        }
    }

    #[track_caller]
    fn settle_until(ed: &mut Editor, done: impl Fn(&Editor) -> bool) {
        crate::test_results::settle_saying(ed, done, |ed| {
            let texts: Vec<String> = ed.docs.iter().map(|d| d.text.to_string()).collect();
            format!("{texts:?}: {}", toasts(ed))
        });
    }

    fn toasts(ed: &Editor) -> String {
        ed.toasts.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join(" | ")
    }

    /// An editor with the demo plugin loaded from its own folder, on `text` — its state (`state/`) in that folder.
    fn setup(name: &str, text: &str) -> (crate::scratch::Scratch, Editor, PathBuf) {
        setup_with(name, text, DEMO)
    }

    /// `setup` with the demo's build as `wasm` (patched: `abi::demo_reading`). The folder goes with the `Scratch`,
    /// after the editor (#441: an `into_path` the caller was to remove was left in the temp folder each run where it
    /// didn't).
    fn setup_with(name: &str, text: &str, wasm: &[u8]) -> (crate::scratch::Scratch, Editor, PathBuf) {
        let s = crate::scratch::Scratch::new(&format!("plugins-{name}"));
        let dir = s.to_path_buf();
        std::fs::write(dir.join("demo.wasm"), wasm).unwrap();
        let mut ed = Editor::new(Config::default());
        ed.docs[0].text = ropey::Rope::from_str(text);
        ed.plugins_sync_in(&dir.join("state"), host::scan(&dir));
        settle_until(&mut ed, |ed| ed.plugin_command("shout").is_some());
        (s, ed, dir)
    }

    /// #441: it left its folder in the temp folder every run.
    #[test]
    fn the_setup_tests_leave_no_folder_behind() {
        let left = crate::scratch::left_by(a_plugins_tab_opened_mid_listing_drops_a_reveal);
        assert!(left.is_empty(), "left in the temp folder: {left:?}");
    }

    fn key(ed: &mut Editor, k: &str) {
        ed.handle_key(k.parse().unwrap());
    }

    /// The plugin thread judges permissions by the editor's own rule (`perm::invalid`, passed through
    /// `plugins_sync`): the demo loads under it, and the demo asking for an environment variable by a name that
    /// isn't one (`TARAE-DEMO`, its bytes changed in place) doesn't — said, with no command of it registered.
    #[test]
    fn the_plugin_thread_judges_permissions_by_the_editors_rule() {
        let (_s, ed, dir) = setup("rule", "x\n");
        let declared = &ed.plugins.as_ref().unwrap().permissions["demo"];
        assert!(declared.contains(&abi::Permission::Env("TARAE_DEMO".into())), "{declared:?}");
        std::fs::remove_dir_all(dir).ok();

        let at = DEMO.windows(10).position(|w| w == b"TARAE_DEMO").expect("the demo's permission");
        assert_eq!(DEMO.windows(10).filter(|w| w == b"TARAE_DEMO").count(), 1, "only the permission");
        let mut bytes = DEMO.to_vec();
        bytes[at + 5] = b'-';
        let dir = crate::scratch::Scratch::new("plugins-rule-refused");
        std::fs::write(dir.join("demo.wasm"), bytes).unwrap();
        let mut ed = Editor::new(Config::default());
        ed.plugins_sync_in(&dir.join("state"), host::scan(&dir));
        // (refused or loaded, whichever comes: a rule not passed fails here at once, not at the patience's end)
        settle_until(&mut ed, |ed| {
            toasts(ed).contains("plugin demo:") || ed.plugin_command("shout").is_some()
        });
        assert!(ed.plugin_command("shout").is_none(), "loaded: {}", toasts(&ed));
        let said = toasts(&ed);
        assert!(said.contains("plugin demo: its permission"), "{said}");
        assert!(said.contains("isn't valid: an environment variable's name"), "{said}");
    }

    /// A plugin's tab opened while a name pressed in a path waits for its listing (mouse X): the side panel leaves the
    /// files tab as every tab change does (`Sidebar::switch_to`) — the reveal goes with it, never taken on a later
    /// `space e` (the reviewer's, round 3).
    #[test]
    fn a_plugins_tab_opened_mid_listing_drops_a_reveal() {
        let (_s, mut ed, dir) = setup("side-reveal", "x\n");
        let root = std::fs::canonicalize(&dir).unwrap();
        ed.workspace.root = root.clone();
        let files = vec!["src/ui/card.rs".to_string()];
        ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::with_files(root.clone(), files));
        ed.sidebar_reveal(&root.join("src/ui"));
        let reveal = |ed: &Editor| ed.workspace.viewer.sidebar.as_ref().and_then(|s| s.reveal.clone());
        assert_eq!(reveal(&ed).as_deref(), Some("src/ui"), "kept for the listing it asked");
        let side = abi::Side {
            title: "demo".into(),
            note: String::new(),
            rows: Vec::new(),
            keys: Vec::new(),
            open: true,
            focus: false,
        };
        ed.plugin_side("demo", Some(side));
        let mode = ed.workspace.viewer.sidebar.as_ref().map(|s| s.mode);
        assert_eq!(mode, Some(crate::sidebar::Mode::Plugin));
        assert_eq!(reveal(&ed), None, "the files tab left: gone");
    }

    /// A plugin's command typed on the command line is counted by the name it registered — never its words
    /// (`features.rs`).
    #[test]
    fn a_plugin_command_counts_by_its_name() {
        let (_s, mut ed, dir) = setup("count", "x\n");
        ed.config.features_count = true;
        for c in ":shout zqxmarker".chars() {
            ed.handle_key(crate::key::Key::plain(crate::key::Code::Char(c)));
        }
        key(&mut ed, "ret");
        let pending = ed.features.pending();
        assert!(pending.iter().any(|(k, c)| k == "plugin:shout" && c.n == 1), "{pending:?}");
        assert!(pending.iter().all(|(k, _)| !k.contains("zqxmarker")), "{pending:?}");
        std::fs::remove_dir_all(dir).ok();
    }

    /// A message starting `what` about a file named `name`.
    fn said(ed: &Editor, what: &str, name: &str) -> bool {
        ed.toasts.iter().any(|t| t.text.starts_with(what) && t.text.contains(name))
    }

    /// A key the way the terminal sends it (through the event loop, which is what `changed` watches).
    fn press(ed: &mut Editor, k: &str) {
        ed.handle_event(crate::event::Event::Key(k.parse().unwrap()));
    }

    /// Runs the event loop for a while.
    fn settle_for(ed: &mut Editor, d: Duration) {
        let end = Instant::now() + d;
        while let Some(ev) = ed.events.recv_timeout(end.saturating_duration_since(Instant::now())) {
            ed.handle_event(ev);
        }
    }

    fn disk(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn until_said(ed: &mut Editor, what: &str) {
        settle_until(ed, |ed| toasts(ed).contains(what));
    }

    /// `y` on the front card, if it's what `is` says.
    fn answer(ed: &mut Editor, is: impl Fn(&crate::offer::What) -> bool, yes: bool) {
        let front = ed.offers.first().map(|o| &o.what);
        assert!(
            front.is_some_and(&is),
            "the card: {:?}",
            ed.offers.iter().map(|o| &o.title).collect::<Vec<_>>()
        );
        if yes { ed.offer_accept() } else { ed.offer_dismiss() }
    }

    /// The demo's permissions approved, and `dir` the project: its files are what the demo's `project` covers —
    /// what it's told of on its own and may open (#457).
    fn granted(ed: &mut Editor, dir: &Path) {
        answer(ed, approval, true);
        ed.workspace.root = std::fs::canonicalize(dir).unwrap();
    }

    fn approval(w: &crate::offer::What) -> bool {
        matches!(w, crate::offer::What::PluginPermissions { plugin, .. } if plugin == "demo")
    }

    fn ask(w: &crate::offer::What) -> bool {
        matches!(w, crate::offer::What::PluginAsk { .. })
    }

    #[test]
    fn permissions_wait_for_approval_and_hold_to_what_was_asked() {
        let (_s, mut ed, dir) = setup("perm", "x\n");
        // Held: the card asks as it loads, and what needs a permission waits for it — done once approved, the
        // plugin going on by itself (#167: it stayed refused until asked again)
        ed.plugin_run("exec", "").unwrap();
        settle_until(&mut ed, |ed| {
            ed.plugins.as_ref().is_some_and(|p| p.held.get("demo").is_some_and(|h| h.len() == 1))
        });
        assert!(!toasts(&ed).contains("failed 1"), "{}", toasts(&ed));
        answer(&mut ed, approval, true);
        // (its output and its end go to the plugin undrawn; its answers, which say them, drawn)
        let flags = quiet_until(&mut ed, "exit 1 Some(0) []");
        assert!(flags.contains(&true) && flags.last() == Some(&false), "{flags:?}");
        // A variable set for it: only one it may read
        ed.toasts.clear();
        ed.plugin_run("exec-with", "TARAE_DEMO=1").unwrap();
        until_said(&mut ed, "exit 1 Some(0) []");
        ed.plugin_run("exec-with", "DYLD_INSERT_LIBRARIES=/tmp/x.dylib").unwrap();
        until_said(&mut ed, "failed 1 it may set only variables it may read, not DYLD_INSERT_LIBRARIES");
        // Only what it asked for
        ed.plugin_run("getenv", "PATH").unwrap();
        until_said(&mut ed, "failed 11 it didn't ask to read the environment variable PATH");
        ed.plugin_run("getenv", "TARAE_DEMO").unwrap();
        until_said(&mut ed, "env 11 None");
        // Taken back: held again, and asked again
        ed.plugin_revoke("demo").unwrap();
        ed.toasts.clear();
        ed.plugin_run("exec", "").unwrap();
        until_said(&mut ed, "failed 1 its permissions wait for your approval (:plugin-approve demo)");
        // … but the card isn't pushed at you again by the plugin; you bring it back
        assert!(!ed.offers.iter().any(|o| approval(&o.what)));
        ed.plugin_approve("demo").unwrap();
        assert!(ed.offers.first().is_some_and(|o| approval(&o.what)));
        std::fs::remove_dir_all(dir).ok();
    }

    /// What waited on the approval card is refused when you say not now — said, never left unanswered; with no
    /// card up a request is refused at once; `:plugin-approve`'s card holds again, and its yes goes on.
    #[test]
    fn not_approving_refuses_what_waited_on_the_card() {
        let (_s, mut ed, dir) = setup("not-now", "x\n");
        let held = |ed: &Editor| ed.plugins.as_ref().map_or(0, |p| p.held.get("demo").map_or(0, Vec::len));
        ed.plugin_run("exec", "").unwrap();
        ed.plugin_run("getenv", "TARAE_DEMO").unwrap();
        settle_until(&mut ed, |ed| held(ed) == 2);
        answer(&mut ed, approval, false);
        let why = "its permissions wait for your approval (:plugin-approve demo)";
        settle_until(&mut ed, |ed| {
            toasts(ed).contains(&format!("failed 1 {why}"))
                && toasts(ed).contains(&format!("failed 11 {why}"))
        });
        assert_eq!(held(&ed), 0);
        // No card up: refused at once, not held
        ed.toasts.clear();
        ed.plugin_run("exec", "").unwrap();
        until_said(&mut ed, &format!("failed 1 {why}"));
        assert_eq!(held(&ed), 0);
        // Its card brought back: held again, and done on yes
        ed.plugin_approve("demo").unwrap();
        ed.plugin_run("exec", "").unwrap();
        settle_until(&mut ed, |ed| held(ed) == 1);
        answer(&mut ed, approval, true);
        until_said(&mut ed, "exit 1 Some(0) []");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn files_in_its_patterns_are_read_others_are_asked_about() {
        // The demo reads by a pattern of the test's own scratch folder, patched into its build — never a folder
        // whose name anyone can guess (and so make first), nor a fixed one another run of the tests shares
        let reads = crate::scratch::Scratch::new("plugins-files-reads");
        let pattern = format!("{}/**", reads.display());
        let (_s, mut ed, dir) = setup_with("files", "x\n", &abi::demo_reading(&pattern));
        let declared = &ed.plugins.as_ref().unwrap().permissions["demo"];
        assert!(declared.contains(&abi::Permission::Read(pattern.clone())), "{declared:?}");
        let base = reads.to_path_buf();
        std::fs::write(base.join("a.txt"), "hello").unwrap();
        let outside = dir.join("outside.txt");
        std::fs::write(&outside, "secret").unwrap();
        let link = base.join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        answer(&mut ed, approval, true);
        ed.plugin_run("fetch", &base.join("a.txt").display().to_string()).unwrap();
        until_said(&mut ed, "file 10 5");
        // Out by `..`, or by a symlink: asked about, not read
        for sneaky in [format!("{}/../../../etc/hosts", base.display()), link.display().to_string()] {
            ed.toasts.clear();
            ed.plugin_run("fetch", &sneaky).unwrap();
            settle_until(&mut ed, |ed| ed.offers.first().is_some_and(|o| ask(&o.what)));
            answer(&mut ed, ask, false);
            until_said(&mut ed, "failed 10 you said no");
        }
        // Allowed once: kept
        ed.plugin_run("fetch", &outside.display().to_string()).unwrap();
        settle_until(&mut ed, |ed| ed.offers.first().is_some_and(|o| ask(&o.what)));
        answer(&mut ed, ask, true);
        until_said(&mut ed, "file 10 6");
        ed.toasts.clear();
        ed.plugin_run("fetch", &outside.display().to_string()).unwrap();
        until_said(&mut ed, "file 10 6");
        assert!(ed.offers.is_empty(), "not asked twice");
        // A relative path isn't one
        ed.plugin_run("fetch", "a.txt").unwrap();
        until_said(&mut ed, "an absolute path, or one from ~/");
        std::fs::remove_dir_all(dir).ok();
    }

    /// #457 (it was the confirming test, check 2): a plugin granted no read can't open a file outside its patterns and
    /// the project — either form of `open`, said — and that file opened by you is told to it as nothing: no `opened`,
    /// a `shown` with no name or text (the demo says `shown with text: …` for a `.text.` name).
    #[test]
    fn a_plugin_with_no_read_opens_no_file_and_hears_nothing_of_one() {
        let (_s, mut ed, dir) = setup("no-read", "x\n");
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        ed.workspace.root = std::fs::canonicalize(&project).unwrap();
        let elsewhere = crate::scratch::Scratch::new("plugins-no-read-elsewhere");
        let marker = elsewhere.join("notes.text.md");
        std::fs::write(&marker, "key = tarae-fake-token-457\n").unwrap();
        let marker = marker.display().to_string();
        // Nothing granted: its read is refused
        answer(&mut ed, approval, false);
        ed.plugin_run("fetch", &marker).unwrap();
        until_said(&mut ed, "failed 10 its permissions wait for your approval");
        // …and so is `open`, said — the file and the file at a line
        for at in [marker.clone(), format!("{marker}:1")] {
            ed.toasts.clear();
            ed.plugin_run("visit", &at).unwrap();
            until_said(&mut ed, "demo: it may not open “");
            assert!(ed.doc().path.is_none(), "{at}: opened");
        }
        // You open it: nothing of it reaches the plugin on its own
        ed.toasts.clear();
        ed.open(Path::new(&marker)).unwrap();
        let doc = ed.doc().id;
        for e in [abi::Event::Opened, abi::Event::Changed, abi::Event::Saved, abi::Event::BeforeSave] {
            assert!(ed.subscribers(e, doc).is_empty(), "{e:?}");
        }
        ed.plugin_run("echo", "").unwrap();
        until_said(&mut ed, "args=");
        assert!(
            !toasts(&ed).contains("opened ") && !toasts(&ed).contains("shown with text"),
            "{}",
            toasts(&ed)
        );
    }

    /// The demo approved, the project `dir/project`, and in front `name` in a scratch of its own: a file no grant of
    /// the demo's covers (#457). Its path as the buffer has it.
    fn uncovered(ed: &mut Editor, dir: &Path, name: &str) -> (crate::scratch::Scratch, String) {
        answer(ed, approval, true);
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        ed.workspace.root = std::fs::canonicalize(&project).unwrap();
        let elsewhere = crate::scratch::Scratch::new("plugins-uncovered");
        std::fs::write(elsewhere.join(name), "key = tarae-fake-token-457\n").unwrap();
        ed.open(&elsewhere.join(name)).unwrap();
        let path = ed.doc().path.as_ref().unwrap().display().to_string();
        (elsewhere, path)
    }

    /// What `plugin_flush`'s calls gave the demo since the last look: (path, its answer yours) each.
    fn flushed(ed: &mut Editor) -> Vec<(Option<String>, bool)> {
        std::mem::take(&mut ed.plugins.as_mut().unwrap().flushed)
    }

    /// #457, the carrier step 1 missed: what comes back to a plugin on its own — a timer's tick, a program's output
    /// line and its end, a key in its panel — with a file it can't read in front carries an empty buffer, and an
    /// answer that isn't yours; with one it can, the buffer.
    #[test]
    fn what_comes_back_on_its_own_carries_no_buffer_it_cant_read() {
        let (_s, mut ed, dir) = setup("flush-carriers", "");
        let (_elsewhere, _) = uncovered(&mut ed, &dir, "notes.md");
        ed.plugin_run("table", "").unwrap();
        settle_until(&mut ed, |ed| panel(ed).is_some());
        flushed(&mut ed);
        let each = |ed: &mut Editor, what: &str, act: &dyn Fn(&mut Editor), said: &str| {
            ed.toasts.clear();
            act(ed);
            until_said(ed, said);
            let given = flushed(ed);
            assert!(!given.is_empty() && given.iter().all(|g| *g == (None, false)), "{what}: {given:?}");
        };
        each(&mut ed, "a key in its panel", &|ed| key(ed, "ret"), "key ret");
        key(&mut ed, "esc");
        each(&mut ed, "a tick", &|ed| ed.plugin_run("later", "1").unwrap(), "tick 30");
        each(&mut ed, "a program's end", &|ed| ed.plugin_run("exec", "").unwrap(), "exit 1");
        let line =
            |ed: &mut Editor| ed.plugin_queue("demo", vec![abi::HostEvent::Lines(1, vec!["out".into()])], 0);
        each(&mut ed, "a program's output line", &line, "lines 1");
        // A file it may read (the project's): given
        let mine = ed.workspace.root.join("a.rs");
        std::fs::write(&mine, "fn a() {}\n").unwrap();
        ed.open(&mine).unwrap();
        ed.toasts.clear();
        ed.plugin_run("later", "1").unwrap();
        until_said(&mut ed, "tick 30");
        let given = flushed(&mut ed);
        assert!(given.iter().any(|(p, _)| p.as_deref().is_some_and(|p| p.ends_with("a.rs"))), "{given:?}");
    }

    /// #457: a line or a pick your command asked for, answered with the buffer it ran on in front, is an answer to that
    /// command there — that buffer given, once; asked for by a key in its panel, or answered with another buffer in
    /// front, never.
    #[test]
    fn a_line_or_pick_your_command_asked_for_is_yours_on_its_buffer_once() {
        let (_s, mut ed, dir) = setup("chained", "");
        let (_elsewhere, path) = uncovered(&mut ed, &dir, "notes.md");
        let answered = |ed: &mut Editor, keys: &[&str]| {
            ed.toasts.clear();
            flushed(ed);
            for k in keys {
                key(ed, k);
            }
            until_said(ed, "answer ");
            flushed(ed)
        };
        let yours = vec![(Some(path.clone()), true)];
        // Your command's line, and its pick
        ed.plugin_run("question", "").unwrap();
        settle_until(&mut ed, |ed| ed.prompt.is_some());
        assert_eq!(answered(&mut ed, &["a", "ret"]), yours, "a line");
        ed.plugin_run("choose", "").unwrap();
        settle_until(&mut ed, |ed| ed.picker.is_some());
        assert_eq!(answered(&mut ed, &["ret"]), yours, "a pick");
        // …once: what comes after on its own isn't
        ed.plugin_run("later", "1").unwrap();
        until_said(&mut ed, "tick 30");
        assert_eq!(flushed(&mut ed), [(None, false)], "after");
        // Another buffer in front as it's answered (one it may read: the project's)
        ed.plugin_run("question", "").unwrap();
        settle_until(&mut ed, |ed| ed.prompt.is_some());
        let other = ed.workspace.root.join("other.txt");
        std::fs::write(&other, "x\n").unwrap();
        ed.open(&other).unwrap();
        assert!(answered(&mut ed, &["a", "ret"]).iter().all(|(_, yours)| !yours), "another buffer");
        // A key in its panel asked for it: its own surface, never yours
        ed.open(Path::new(&path)).unwrap();
        ed.plugin_run("table", "").unwrap();
        settle_until(&mut ed, |ed| panel(ed).is_some());
        key(&mut ed, "/");
        settle_until(&mut ed, |ed| ed.prompt.is_some());
        assert_eq!(answered(&mut ed, &["a", "ret"]), [(None, false)], "a key in its panel");
    }

    /// #457: holding a buffer against a plugin's grant — every event's, every call's — and a plugin's `open` touch no
    /// disk on the main thread (`perm::disk` counts by thread): the patterns resolved on the plugin thread as it
    /// loaded, an open's file made real and judged on the open's worker.
    #[test]
    fn the_gate_touches_no_disk_on_the_main_thread() {
        let reads = crate::scratch::Scratch::new("plugins-disk-reads");
        let (_s, mut ed, dir) =
            setup_with("disk", "", &abi::demo_reading(&format!("{}/**", reads.display())));
        answer(&mut ed, approval, true);
        std::fs::create_dir_all(dir.join("project")).unwrap();
        ed.workspace.root = std::fs::canonicalize(dir.join("project")).unwrap();
        for f in ["a.txt", "b.txt"] {
            std::fs::write(reads.join(f), "x\n").unwrap();
        }
        std::fs::write(dir.join("out.txt"), "x\n").unwrap();
        // (out.txt opened by you: its `visit` later is judged by the buffer's path)
        ed.open(&dir.join("out.txt")).unwrap();
        let here = || perm::disk::HERE.with(|n| n.get());
        let before = (here(), perm::disk::ALL.load(std::sync::atomic::Ordering::Relaxed));
        // A buffer its pattern covers: told of it on every event, given it on a call
        ed.open(&reads.join("a.txt")).unwrap();
        assert!(ed.plugin_covers_doc("demo", ed.doc()), "covered: the check ran");
        for k in ["i", "y", "esc"] {
            press(&mut ed, k);
        }
        ed.plugin_run("echo", "").unwrap();
        until_said(&mut ed, "args=");
        ed.plugin_run("later", "1").unwrap();
        until_said(&mut ed, "tick 30");
        assert_eq!(here(), before.0, "the events' and calls' gate touched the disk on the main thread");
        // Its `open`: on the worker, for a file it may read and one it may not
        ed.read_inline = false;
        ed.plugin_run("visit", &reads.join("b.txt").display().to_string()).unwrap();
        settle_until(&mut ed, |ed| ed.doc().path.as_ref().is_some_and(|p| p.ends_with("b.txt")));
        ed.toasts.clear();
        // (as the buffer has it: the plain one found open)
        let out = std::fs::canonicalize(dir.join("out.txt")).unwrap().display().to_string();
        for at in [format!("{out}:1"), out] {
            ed.toasts.clear();
            ed.plugin_run("visit", &at).unwrap();
            until_said(&mut ed, "demo: it may not open “");
            assert!(ed.doc().path.as_ref().is_some_and(|p| p.ends_with("b.txt")), "{at}: opened");
        }
        assert_eq!(here(), before.0, "the open's gate touched the disk on the main thread");
        let all = perm::disk::ALL.load(std::sync::atomic::Ordering::Relaxed);
        assert!(all >= before.1 + 2, "the worker's touches counted ({} → {all})", before.1);
    }

    /// #457, check 1: what a plugin's own grant covers — its `read` patterns and the project, once approved — never
    /// a file named like a key, not even one its pattern names (a read of one is asked about on a card instead).
    #[test]
    fn a_plugins_grant_covers_its_patterns_and_the_project_never_a_key() {
        let read = crate::scratch::Scratch::new("plugins-covers-read");
        let (_s, mut ed, dir) =
            setup_with("covers", "", &abi::demo_reading(&format!("{}/*", read.display())));
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        ed.workspace.root = std::fs::canonicalize(&project).unwrap();
        // (no file made: a key's name is enough, and nothing reads one)
        let covers = |ed: &Editor, p: &Path| {
            let real = std::fs::canonicalize(p.parent().unwrap()).unwrap().join(p.file_name().unwrap());
            ed.plugin_covers("demo", Some(p), &real)
        };
        assert!(!covers(&ed, &read.join("notes.txt")), "not approved yet");
        answer(&mut ed, approval, true);
        assert!(covers(&ed, &read.join("notes.txt")), "its pattern");
        assert!(covers(&ed, &project.join("a.rs")), "the project");
        assert!(!covers(&ed, &dir.join("notes.txt")), "neither");
        assert!(!covers(&ed, &read.join("id_ed25519")), "a key its pattern names");
        assert!(!covers(&ed, &project.join(".env")), "a key in the project");
    }

    /// #457, check 3: a row of its side tab that places a file it may not read opens nothing — said — though the row
    /// came from a command you ran on that file.
    #[test]
    fn a_side_rows_place_opens_only_a_file_it_may_read() {
        let (_s, mut ed, dir) = setup("side-place", "");
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        granted(&mut ed, &project);
        let outside = dir.join("web.txt");
        std::fs::write(&outside, "kind: Deployment\n").unwrap();
        ed.open(&outside).unwrap();
        ed.plugin_run("side", "").unwrap();
        settle_until(&mut ed, |ed| ed.plugins.as_ref().is_some_and(|p| p.side.is_some()));
        crate::typed::execute(&mut ed, "new").unwrap();
        ed.workspace.set_at(crate::workspace::Here::Side);
        ed.workspace.viewer.sidebar.as_mut().unwrap().selected = 0;
        key(&mut ed, "ret");
        assert!(toasts(&ed).contains("demo: it may not open “"), "{}", toasts(&ed));
        assert!(ed.doc().path.is_none(), "not brought back to the front");
    }

    /// #457, check 4: on its own (an event, what it asked for coming back) a plugin is given a buffer its grant covers
    /// and nothing of one it doesn't — no path, name, text or selections; a command you run gets the one in front.
    #[test]
    fn an_uncovered_buffer_is_given_only_to_your_command() {
        let (_s, mut ed, dir) = setup("given", "");
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        granted(&mut ed, &project);
        let (inside, outside) = (project.join("a.txt"), dir.join("b.txt"));
        for f in [&inside, &outside] {
            std::fs::write(f, "key = tarae-fake-token-457\n").unwrap();
        }
        ed.open(&inside).unwrap();
        let a = ed.doc().id;
        ed.open(&outside).unwrap();
        let b = ed.doc().id;
        let given = |ed: &Editor, doc, yours| {
            let (i, seen) = ed.plugin_input("demo", doc, "", yours).unwrap();
            (i.path.is_some(), i.name.is_empty(), i.text.len_bytes(), i.ranges.len(), seen.yours)
        };
        assert_eq!(given(&ed, a, false), (true, false, 27, 1, false), "covered");
        assert_eq!(given(&ed, b, false), (false, true, 0, 0, false), "not covered: nothing");
        assert_eq!(given(&ed, b, true), (true, false, 27, 1, true), "your command's");
    }

    /// #457, check 5: a buffer its grant doesn't cover has no `opened`, `changed`, `saved`, `before-save` or `closed`
    /// — the demo trims a `.trim` file before it's saved, and says it opened and closed one — only an empty `shown`.
    #[test]
    fn an_uncovered_buffer_has_no_events_but_an_empty_shown() {
        let (_s, mut ed, dir) = setup("no-events", "");
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        granted(&mut ed, &project);
        let path = dir.join("a.trim");
        std::fs::write(&path, "x  \n").unwrap();
        ed.open(&path).unwrap();
        let doc = ed.doc().id;
        assert_eq!(ed.subscribers(abi::Event::Shown, doc), ["demo"], "shown, as nothing");
        for k in ["i", "y", "esc"] {
            press(&mut ed, k);
        }
        crate::typed::execute(&mut ed, "w").unwrap();
        settle_until(&mut ed, |ed| said(ed, "Saved ", "a.trim"));
        crate::typed::execute(&mut ed, "bc").unwrap();
        settle_for(&mut ed, Duration::from_millis(500));
        assert_eq!(disk(&path), "yx  \n", "no before-save");
        let told = toasts(&ed);
        assert!(!told.contains("opened ") && !told.contains("closed "), "{told}");
        assert!(ed.plugin_status().is_empty());
    }

    /// #457, check 6: an answer's edits and moves land in a buffer its grant doesn't cover only when it answers your
    /// command — else where a selection or the cursor lands would tell it the text.
    #[test]
    fn edits_and_moves_in_an_uncovered_buffer_only_answer_you() {
        let (_s, mut ed, dir) = setup("edit-gate", "");
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        granted(&mut ed, &project);
        let path = dir.join("b.txt");
        std::fs::write(&path, "hello world\n").unwrap();
        ed.open(&path).unwrap();
        let (doc, version) = (ed.doc().id, ed.doc().version());
        let head = |ed: &Editor| ed.doc().selection().primary().head;
        let its_own = Seen { doc, version, ranges: Vec::new(), yours: false };
        ed.plugin_apply("demo", &its_own, vec![Action::Select(vec![(6, 11)])]);
        assert!(toasts(&ed).contains("it edits only a file it may read"), "{}", toasts(&ed));
        ed.plugin_apply("demo", &its_own, vec![Action::Run("move_next_word_start".into())]);
        assert!(toasts(&ed).contains("it moves only in a file it may read"), "{}", toasts(&ed));
        assert_eq!(head(&ed), 0, "neither applied");
        let yours = Seen { yours: true, ..its_own };
        ed.plugin_apply("demo", &yours, vec![Action::Select(vec![(0, 1)])]);
        assert_eq!(head(&ed), 1, "selected");
        ed.plugin_apply("demo", &yours, vec![Action::Run("move_next_word_start".into())]);
        assert_ne!(head(&ed), 1, "moved");
    }

    /// #457, Q2: a buffer with no file — its own view is the plugin's, on its own too; your scratch is given to a
    /// command you run on it; a surface of tarae's own never is, the command refused and said. The chat's transcript
    /// is a pane of its own, never the buffer a command runs on: with the chat's keys it runs on the code behind.
    #[test]
    fn buffers_with_no_file_its_view_your_scratch_never_taraes_own() {
        let (_s, mut ed, _dir) = setup("no-file", "scratch words\n");
        // Your scratch, on your command
        ed.plugin_run("echo", "").unwrap();
        until_said(&mut ed, r#"name=[scratch]"#);
        // Its own view: covered, on its own as well
        ed.plugin_run("view", "").unwrap();
        settle_until(&mut ed, |ed| ed.doc().title.as_deref() == Some("web-1 · demo"));
        let (input, _) = ed.plugin_input("demo", ed.doc().id, "", false).unwrap();
        assert_eq!(input.text.to_string(), "kind: Pod\n");
        // tarae's own: a message shown whole
        let message = ed.open_virtual("message #3", "tarae-fake-token-457\n");
        let refused = ed.plugin_run("echo", "").unwrap_err();
        assert_eq!(refused, ":echo isn't given message #3: one of tarae's own views");
        let (input, _) = ed.plugin_input("demo", message, "", false).unwrap();
        assert_eq!(input.text.len_bytes(), 0, "nor on its own");
        ed.close_doc(message);
        // The chat's transcript: never in front for a command
        let mut c = crate::chat::Chat::new();
        c.msgs = vec![crate::chat::Msg::new(crate::chat::Role::Assistant, "tarae-fake-token-chat", "")];
        ed.workspace.claude.chat = Some(c);
        ed.workspace.set_at(crate::workspace::Here::Chat);
        crate::term::render(&mut ed, &mut Vec::new(), 120, 30).unwrap();
        ed.toasts.clear();
        ed.plugin_run("echo", "").unwrap();
        until_said(&mut ed, "args=");
        assert!(!toasts(&ed).contains("tarae-fake-token-chat"), "{}", toasts(&ed));
    }

    /// #457, Q3: a command you run with a file named like a key in front is refused, saying why — never run on an
    /// empty buffer (a change you'd not see); and nothing of it reaches the plugin on its own, the project's though it is.
    #[test]
    fn a_command_on_a_file_named_like_a_key_is_refused_saying_why() {
        let (_s, mut ed, dir) = setup("key-front", "API_TOKEN=tarae-fake-token-457\n");
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        granted(&mut ed, &project);
        // (named, never written or read: a run can't read a key's file)
        ed.docs[0].path.set(Some(ed.workspace.root.join(".env")));
        let refused = ed.plugin_run("shout", "").unwrap_err();
        assert_eq!(refused, ":shout isn't given .env: a file named like a key");
        assert_eq!(ed.doc().text.to_string(), "API_TOKEN=tarae-fake-token-457\n");
        let (input, _) = ed.plugin_input("demo", ed.doc().id, "", false).unwrap();
        assert!(input.path.is_none() && input.text.len_bytes() == 0);
    }

    #[test]
    fn connections_are_asked_about_then_carry_bytes_both_ways() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut got = [0u8; 4];
            s.read_exact(&mut got).unwrap();
            s.write_all(b"pong").unwrap();
            // (the listener kept for the second dial: a port let go can be another test's before it's bound again)
            (got, listener)
        });
        let (_s, mut ed, dir) = setup("socket", "x\n");
        answer(&mut ed, approval, true);
        ed.plugin_run("dial", &port.to_string()).unwrap();
        // A new host:port: asked about
        settle_until(&mut ed, |ed| ed.offers.first().is_some_and(|o| ask(&o.what)));
        // Its panel shown meanwhile doesn't take the keys: they're the card's
        ed.plugin_run("table", "").unwrap();
        settle_until(&mut ed, |ed| panel(ed).is_some());
        assert!(panel(&ed).is_some() && !ed.workspace.is_at(crate::workspace::Here::Plugin));
        answer(&mut ed, ask, true);
        until_said(&mut ed, "connected 20");
        // (the bytes and the close go to the plugin undrawn; its answers, which say them, drawn)
        let flags = quiet_until(&mut ed, r#"closed 20 ""#);
        assert!(toasts(&ed).contains("data 20 [112, 111, 110, 103]"), "{}", toasts(&ed));
        assert!(flags.contains(&true) && flags.last() == Some(&false), "{flags:?}");
        let (got, listener) = server.join().unwrap();
        assert_eq!(&got, b"ping", "what the plugin sent arrived");
        assert!(ed.plugins.as_ref().unwrap().socks.is_empty(), "closed: gone");
        // The same host:port again: not asked
        let hold = std::thread::spawn(move || listener.accept().map(|(s, _)| s));
        ed.toasts.clear();
        ed.plugin_run("dial", &port.to_string()).unwrap();
        until_said(&mut ed, "connected 20");
        assert!(ed.offers.is_empty());
        // Hung up: closed, and nothing more comes from it
        ed.plugin_run("hang-up", "").unwrap();
        settle_until(&mut ed, |ed| ed.plugins.as_ref().unwrap().socks.is_empty());
        let _ = hold.join();
        // Nobody there: failed
        ed.plugin_run("dial", "1").unwrap();
        settle_until(&mut ed, |ed| ed.offers.first().is_some_and(|o| ask(&o.what)));
        answer(&mut ed, ask, true);
        until_said(&mut ed, "failed 20 127.0.0.1:1");
        std::fs::remove_dir_all(dir).ok();
    }

    /// Connections to a new host in one answer (the Kubernetes plugin's first: two API groups at once) are one
    /// card, and `y` lets every one of them go — none is left waiting forever. Another host is its own card.
    #[test]
    fn connections_waiting_on_one_host_are_one_card_and_all_go() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port() as u64;
        let server =
            std::thread::spawn(move || (0..2).map(|_| listener.accept().unwrap().0).collect::<Vec<_>>());
        let (_s, mut ed, dir) = setup("one-card", "x\n");
        answer(&mut ed, approval, true);
        let seen =
            Seen { doc: ed.docs[0].id, version: ed.docs[0].version(), ranges: Vec::new(), yours: false };
        let to = |id, port| Action::Connect(abi::Connect { id, host: "127.0.0.1".into(), port });
        ed.plugin_apply("demo", &seen, vec![to(20, port), to(21, port), to(22, 1)]);
        let asking: Vec<String> =
            ed.offers.iter().filter(|o| ask(&o.what)).map(|o| o.question.clone()).collect();
        assert_eq!(asking.len(), 2, "one card a host: {asking:?}");
        answer(&mut ed, ask, true);
        settle_until(&mut ed, |ed| {
            toasts(ed).contains("connected 20") && toasts(ed).contains("connected 21")
        });
        // (the other host's card, asking still)
        answer(&mut ed, ask, false);
        until_said(&mut ed, "failed 22 you said no");
        assert!(ed.offers.is_empty());
        drop(server.join());
        std::fs::remove_dir_all(dir).ok();
    }

    /// A host never reached — an address `route::never` refuses, a metadata server's own name, an address in a form
    /// other than a.b.c.d — fails at once, before any card: no answer of yours would let it go.
    #[test]
    fn a_host_never_reached_fails_with_no_card() {
        let (_s, mut ed, dir) = setup("never", "x\n");
        answer(&mut ed, approval, true);
        let seen =
            Seen { doc: ed.docs[0].id, version: ed.docs[0].version(), ranges: Vec::new(), yours: false };
        let to = |id, host: &str| Action::Connect(abi::Connect { id, host: host.into(), port: 80 });
        let hosts = vec![to(23, "169.254.169.254"), to(24, "metadata.google.internal"), to(25, "2852039166")];
        ed.plugin_apply("demo", &seen, hosts);
        assert!(ed.offers.is_empty(), "no card");
        until_said(&mut ed, "failed 23 169.254.169.254 is never reached");
        until_said(&mut ed, "failed 24 “metadata.google.internal” is a cloud metadata server's own name");
        until_said(&mut ed, "failed 25 “2852039166” is an address in a form other than a.b.c.d");
        std::fs::remove_dir_all(dir).ok();
    }

    /// `shown` goes out when the buffer in front changes — switching buffers, opening one — and not again
    /// while it stays.
    #[test]
    fn the_buffer_in_front_is_shown() {
        let (_s, mut ed, dir) = setup("shown", "x\n");
        granted(&mut ed, &dir);
        let (a, b) = (dir.join("a.front"), dir.join("b.front"));
        std::fs::write(&a, "a\n").unwrap();
        std::fs::write(&b, "b\n").unwrap();
        // (each says the file and when — each its own message: the file names, in order)
        let shown = |ed: &Editor| {
            let named = |t: &String| {
                t.rsplit(' ').nth(1).and_then(|p| p.rsplit('/').next()).unwrap_or("").to_string()
            };
            ed.toasts
                .iter()
                .map(|t| &t.text)
                .filter(|t| t.starts_with("shown "))
                .map(named)
                .collect::<Vec<_>>()
        };
        ed.open(&a).unwrap();
        settle_until(&mut ed, |ed| shown(ed) == ["a.front"]);
        ed.open(&b).unwrap();
        settle_until(&mut ed, |ed| shown(ed) == ["a.front", "b.front"]);
        key(&mut ed, "j");
        settle_for(&mut ed, Duration::from_millis(200));
        assert_eq!(shown(&ed), ["a.front", "b.front"], "the same one in front: not again");
        // (back to one already open)
        ed.cycle_buffer(-1);
        ed.handle_event(crate::event::Event::Resize);
        settle_until(&mut ed, |ed| shown(ed) == ["a.front", "b.front", "a.front"]);
        std::fs::remove_dir_all(dir).ok();
    }

    /// An event subscribed to twice — every buffer, and Markdown with its text — gives the text only for
    /// Markdown.
    #[test]
    fn an_event_s_text_is_for_the_languages_that_asked() {
        let (_s, mut ed, dir) = setup("shown-text", "x\n");
        granted(&mut ed, &dir);
        let (md, txt) = (dir.join("a.text.md"), dir.join("b.text.log"));
        std::fs::write(&md, "# a\n").unwrap();
        std::fs::write(&txt, "b\n").unwrap();
        ed.open(&md).unwrap();
        until_said(&mut ed, "shown with text: true");
        ed.open(&txt).unwrap();
        until_said(&mut ed, "shown with text: false");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn timers_tick_unless_stopped() {
        let (_s, mut ed, dir) = setup("timer", "x\n");
        ed.plugin_run("later", "10").unwrap();
        until_said(&mut ed, "tick 30");
        ed.toasts.clear();
        ed.plugin_run("later", "300").unwrap();
        ed.plugin_run("never", "").unwrap();
        settle_for(&mut ed, Duration::from_millis(600));
        assert!(!toasts(&ed).contains("tick"), "{}", toasts(&ed));
        std::fs::remove_dir_all(dir).ok();
    }

    /// Each event handled as the main loop does: whether it said it changed nothing (then it isn't drawn).
    fn quiet_flags(ed: &mut Editor, d: Duration) -> Vec<bool> {
        let end = Instant::now() + d;
        let mut out = Vec::new();
        while let Some(ev) = ed.events.recv_timeout(end.saturating_duration_since(Instant::now())) {
            ed.unchanged = false;
            ed.handle_event(ev);
            out.push(std::mem::take(&mut ed.unchanged));
        }
        out
    }

    /// `quiet_flags` until a message says `what`.
    fn quiet_until(ed: &mut Editor, what: &str) -> Vec<bool> {
        let end = Instant::now() + crate::test_results::PATIENCE;
        let mut out = Vec::new();
        while !toasts(ed).contains(what) {
            let ev = ed.events.recv_timeout(end.saturating_duration_since(Instant::now()));
            let ev = ev.unwrap_or_else(|| panic!("timed out: {}", toasts(ed)));
            ed.unchanged = false;
            ed.handle_event(ev);
            out.push(std::mem::take(&mut ed.unchanged));
        }
        out
    }

    /// A watch's every change comes and goes without a frame: what's only for the plugin (a tick, a
    /// connection's bytes) and an answer of only I/O aren't drawn — an answer that says something is.
    #[test]
    fn what_changes_nothing_on_screen_isnt_drawn() {
        let (_s, mut ed, dir) = setup("quiet", "x\n");
        // (what loading sent it answered first — its buffer shown — and quietly)
        assert!(quiet_flags(&mut ed, Duration::from_millis(200)).iter().all(|q| *q));
        ed.plugin_run("quiet", "").unwrap();
        let flags = quiet_flags(&mut ed, Duration::from_millis(400));
        // (the command's answer; then the tick, and the plugin's answer to it: `stop` only)
        assert_eq!(flags.len(), 3, "{flags:?}");
        assert_eq!(&flags[1..], [true, true]);
        ed.plugin_run("later", "10").unwrap();
        let flags = quiet_flags(&mut ed, Duration::from_millis(400));
        assert!(toasts(&ed).contains("tick 30"), "{}", toasts(&ed));
        assert_eq!(flags.last(), Some(&false), "an answer that says something is drawn: {flags:?}");
        std::fs::remove_dir_all(dir).ok();
    }

    fn panel(ed: &Editor) -> Option<&panel::PanelState> {
        ed.plugins.as_ref()?.panel.as_ref()
    }

    #[test]
    fn a_panel_takes_the_keys_until_esc() {
        let (_s, mut ed, dir) = setup("panel", "x\n");
        answer(&mut ed, approval, true);
        ed.plugin_run("table", "").unwrap();
        settle_until(&mut ed, |ed| panel(ed).is_some());
        assert!(
            panel(&ed).is_some() && ed.workspace.is_at(crate::workspace::Here::Plugin),
            "shown by a command: it has the keys"
        );
        // tarae moves through the rows
        key(&mut ed, "j");
        key(&mut ed, "j");
        assert_eq!(panel(&ed).unwrap().selected, 2);
        key(&mut ed, "G");
        assert_eq!(panel(&ed).unwrap().selected, 29);
        key(&mut ed, "k");
        // …the plugin gets the rest, with the row
        key(&mut ed, "d");
        until_said(&mut ed, r#"key d Some(28) Some("default/web-28")"#);
        key(&mut ed, "C-k");
        until_said(&mut ed, "key C-k Some(28)");
        // The editor's still: space, `:`
        key(&mut ed, ":");
        assert!(ed.prompt.is_some(), "the command line opened");
        key(&mut ed, "esc");
        // …and `C-w`, the windows' (uxaudit D8: it went to the plugin)
        key(&mut ed, "C-w");
        assert_eq!(ed.pending.len(), 1, "C-w waits for its window key");
        // …but `C-w q` doesn't close it, as its own `q` doesn't (the plugin's): said, the keys kept
        key(&mut ed, "q");
        assert!(
            panel(&ed).is_some() && ed.workspace.is_at(crate::workspace::Here::Plugin),
            "the panel stays, with the keys"
        );
        let said = ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
        assert!(said.contains("closes by its own keys"), "{said}");
        // esc: the keys go back to the code, the panel stays
        key(&mut ed, "esc");
        assert!(panel(&ed).is_some() && !ed.workspace.is_at(crate::workspace::Here::Plugin));
        ed.toasts.clear();
        key(&mut ed, "d");
        settle_for(&mut ed, Duration::from_millis(100));
        assert!(!toasts(&ed).contains("key d"), "{}", toasts(&ed));
        // A click on a row: the keys, and that row
        ed.plugin_panel_click(Some(5));
        assert_eq!(
            (ed.workspace.is_at(crate::workspace::Here::Plugin), panel(&ed).unwrap().selected),
            (true, 5)
        );
        // Rows that moved: the selection stays on its row (by id)
        let mut moved = panel(&ed).unwrap().panel.clone();
        moved.rows.rotate_left(3);
        moved.ids.rotate_left(3);
        ed.plugin_panel("demo", Some(moved));
        let st = panel(&ed).unwrap();
        assert_eq!((st.selected, st.panel.ids[st.selected].as_str()), (2, "default/web-5"));
        std::fs::remove_dir_all(dir).ok();
    }

    /// What the demo's panel hears for `enter` and `-`, each as it says it (`key ret Some(0) …`).
    fn panel_heard() -> Vec<String> {
        let (_s, mut ed, dir) = setup("panel-keys", "x\n");
        answer(&mut ed, approval, true);
        ed.plugin_run("table", "").unwrap();
        settle_until(&mut ed, |ed| panel(ed).is_some());
        let mut heard = Vec::new();
        for k in ["ret", "minus"] {
            ed.toasts.clear();
            key(&mut ed, k);
            settle_until(&mut ed, |ed| toasts(ed).contains("key "));
            let said = toasts(&ed);
            let at = said.find("key ").unwrap();
            heard.push(said[at..].split(" · ").next().unwrap().to_string());
        }
        std::fs::remove_dir_all(dir).ok();
        heard
    }

    /// #202's review: a panel's plugin hears a key in the keymap's notation, the plugin API's word — `ret`, `minus`,
    /// never the shown `enter`, `-` (the k8s plugin matches `"ret"`).
    #[test]
    fn a_plugin_hears_keys_in_the_keymaps_notation() {
        assert_eq!(
            panel_heard(),
            [r#"key ret Some(0) Some("default/web-0")"#, r#"key minus Some(0) Some("default/web-0")"#]
        );
    }

    /// …and it would see a key sent by its shown name.
    #[test]
    fn the_plugin_key_test_sees_a_key_sent_by_its_shown_name() {
        let _back = crate::key::Broken::NotationAsShown.set();
        let heard = panel_heard();
        assert!(heard[0].starts_with("key enter ") && heard[1].starts_with("key - "), "{heard:?}");
    }

    /// A plugin's panel moves by tarae's moving keys — they never come to the plugin (docs/plugins.md, the WIT's
    /// `panel`; user, 2026-10-21) — by the rows it drew: `C-d` half of them, `C-f` all, `C-e`/`C-y` the view a row
    /// with the selection kept in sight; a letter still comes.
    #[test]
    fn a_panels_moving_keys_are_taraes_never_the_plugins() {
        let (_s, mut ed, dir) = setup("panel-moves", "x\n");
        answer(&mut ed, approval, true);
        ed.plugin_run("table", "").unwrap();
        settle_until(&mut ed, |ed| panel(ed).is_some());
        let draw = |ed: &mut Editor| crate::term::render(ed, &mut Vec::new(), 100, 40).unwrap();
        draw(&mut ed);
        let (_, y1, first, _) = ed.screen.plugin_panel.get().expect("the panel drawn");
        let rows = y1 - first;
        assert!(rows > 2 && rows < 15, "{rows} rows in sight of its 30");
        let at = |ed: &Editor| panel(ed).map(|p| (p.selected, p.scroll.get())).unwrap();
        ed.toasts.clear();
        for (k, selected) in [
            ("C-f", rows),
            ("C-b", 0),
            ("pagedown", rows),
            ("C-u", rows - rows / 2),
            ("C-d", rows - rows / 2 + rows / 2),
            ("home", 0),
            ("end", 29),
            ("g", 0),
            ("C-n", 1),
            ("C-p", 0),
        ] {
            key(&mut ed, k);
            draw(&mut ed);
            assert_eq!(at(&ed).0, selected, "{k} ({rows} rows)");
        }
        // `C-e`: the view a row, the selection on the nearest row in sight; `C-y` back, the selection staying
        key(&mut ed, "C-e");
        draw(&mut ed);
        assert_eq!(at(&ed), (1, 1), "C-e");
        key(&mut ed, "C-y");
        draw(&mut ed);
        assert_eq!(at(&ed), (1, 0), "C-y");
        // The wheel: its view three rows and nothing else — the selection stays, in sight or not (the user, 2026-10-28)
        let wheel = |ed: &mut Editor| {
            let m = crate::event::Mouse {
                kind: crate::event::MouseKind::ScrollDown,
                x: 10,
                y: first as u16,
                alt: false,
                ctrl: false,
            };
            ed.handle_mouse(m);
            draw(ed);
        };
        wheel(&mut ed);
        assert_eq!(at(&ed), (1, 3), "the wheel past the selection: it stays, out of sight");
        // (a key: the view back to the selection, from the row it moved to)
        for _ in 0..4 {
            key(&mut ed, "j");
        }
        draw(&mut ed);
        let (sel, top) = at(&ed);
        assert!(sel == 5 && (top..top + rows).contains(&5), "j: in sight again: {:?}", at(&ed));
        for _ in 0..3 {
            wheel(&mut ed);
        }
        assert_eq!(at(&ed), (5, (top + 9).min(30 - rows)), "the wheel: the view, the selection where it was");
        // (the editor's keys over it — the space menu opened and closed — leave the wheel's view)
        let wheeled = at(&ed);
        key(&mut ed, "space");
        draw(&mut ed);
        key(&mut ed, "esc");
        draw(&mut ed);
        assert_eq!(at(&ed), wheeled, "space and esc: the view where the wheel left it");
        // A plugin's own key on a row out of sight: the row back in sight first, said — the plugin hears nothing yet
        key(&mut ed, "d");
        let said = ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
        assert_eq!(said, "row 6 is selected, back in sight — d again for it");
        draw(&mut ed);
        let (_, top) = at(&ed);
        assert!((top..top + rows).contains(&5), "brought back: {:?}", at(&ed));
        settle_for(&mut ed, Duration::from_millis(100));
        assert!(!toasts(&ed).contains("key d"), "the plugin heard d: {}", toasts(&ed));
        key(&mut ed, "g");
        key(&mut ed, "j");
        draw(&mut ed);
        settle_for(&mut ed, Duration::from_millis(100));
        assert!(!toasts(&ed).contains("key "), "a moving key reached the plugin: {}", toasts(&ed));
        key(&mut ed, "d");
        until_said(&mut ed, r#"key d Some(1)"#);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_plugins_line_answers_what_was_typed() {
        let (_s, mut ed, dir) = setup("prompt", "x\n");
        ed.plugin_run("question", "").unwrap();
        settle_until(&mut ed, |ed| ed.prompt.is_some());
        assert_eq!(ed.prompt.as_ref().unwrap().label.as_deref(), Some("demo · filter:"), "it says whose");
        // Not over a line being typed
        ed.plugin_run("question", "").unwrap();
        until_said(&mut ed, "failed 6 you're typing something else");
        for k in ["w", "e", "b", "ret"] {
            key(&mut ed, k);
        }
        until_said(&mut ed, r#"answer 6 Some("web")"#);
        ed.plugin_run("question", "").unwrap();
        settle_until(&mut ed, |ed| ed.prompt.is_some());
        key(&mut ed, "esc");
        until_said(&mut ed, "answer 6 None");
        // Replaced by another line: it's told nothing was typed
        ed.plugin_run("question", "").unwrap();
        settle_until(&mut ed, |ed| ed.prompt.is_some());
        ed.toasts.clear();
        ed.open_prompt(crate::editor::PromptKind::Command, "");
        until_said(&mut ed, "answer 6 None");
        ed.prompt = None;
        // Only in answer to you: not on its own a moment later, but after a key in its panel, yes
        ed.plugin_run("nag", "").unwrap();
        until_said(&mut ed, "failed 6 a line is asked for only in answer to you");
        assert!(ed.prompt.is_none());
        ed.plugin_run("table", "").unwrap();
        settle_until(&mut ed, |ed| panel(ed).is_some());
        key(&mut ed, "/");
        settle_until(&mut ed, |ed| ed.prompt.is_some());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_plugins_buffer_is_shown_replaced_and_grown() {
        let (_s, mut ed, dir) = setup("view", "x\n");
        let title = "web-1 · demo";
        let text = |ed: &Editor| {
            ed.docs
                .iter()
                .filter(|d| d.title.as_deref() == Some(title))
                .map(|d| d.text.to_string())
                .collect::<Vec<_>>()
        };
        ed.plugin_run("view", "").unwrap();
        settle_until(&mut ed, |ed| !text(ed).is_empty());
        // (a panel it shows along with it doesn't take the keys back from the buffer)
        ed.plugin_run("view-and-panel", "").unwrap();
        settle_until(&mut ed, |ed| panel(ed).is_some());
        assert!(panel(&ed).is_some() && !ed.workspace.is_at(crate::workspace::Here::Plugin));
        assert_eq!(text(&ed), ["kind: Pod\n"]);
        assert_eq!(ed.doc().title.as_deref(), Some(title), "in front");
        ed.plugin_run("more", "").unwrap();
        settle_until(&mut ed, |ed| text(ed) == ["kind: Pod\nstatus: Running\n"]);
        ed.plugin_run("view", "").unwrap();
        settle_until(&mut ed, |ed| text(ed) == ["kind: Pod\n"]);
        assert_eq!(text(&ed).len(), 1, "one buffer per id");
        // Closed by you: appending fails, and it stays closed
        let id = ed.doc().id;
        ed.close_doc(id);
        until_said(&mut ed, "failed 7 its buffer is closed");
        ed.toasts.clear();
        ed.plugin_run("more", "").unwrap();
        until_said(&mut ed, "failed 7 its buffer is closed");
        assert!(text(&ed).is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    /// The approvals kept in `dir`'s state folder from now on (none in tests otherwise).
    fn stored(ed: &mut Editor, dir: &Path) -> PathBuf {
        let store = dir.join("state/plugin-permissions.json");
        ed.plugins.as_mut().unwrap().perm_store = Some(store.clone());
        store
    }

    fn kept_has(store: &Path, plugin: &str) -> bool {
        perm::load(Some(store)).contains_key(plugin)
    }

    /// The demo reading by a pattern of `reads` (its `a.txt` there), approved, and that yes in its store.
    fn approved_kept(name: &str, reads: &Path) -> (crate::scratch::Scratch, Editor, PathBuf, PathBuf) {
        let (s, mut ed, dir) =
            setup_with(name, "x\n", &abi::demo_reading(&format!("{}/**", reads.display())));
        std::fs::write(reads.join("a.txt"), "hello").unwrap();
        let store = stored(&mut ed, &dir);
        answer(&mut ed, approval, true);
        settle_until(&mut ed, |_| kept_has(&store, "demo"));
        (s, ed, dir, store)
    }

    /// What needs its permissions is refused at once — a read and a program.
    fn refused_now(ed: &mut Editor, reads: &Path) {
        ed.toasts.clear();
        ed.plugin_run("fetch", &reads.join("a.txt").display().to_string()).unwrap();
        until_said(ed, "failed 10 its permissions wait for your approval");
        ed.plugin_run("exec", "").unwrap();
        until_said(ed, "failed 1 its permissions wait for your approval");
        assert!(!toasts(ed).contains("file 10") && !toasts(ed).contains("exit 1"), "{}", toasts(ed));
    }

    /// #461: a revoke holds for the plugin's very next action, while its write is still held — memory is the truth,
    /// the store is written behind it; let go, the store agrees. Broken once by `plugin_may` reading the store.
    #[test]
    fn a_revoke_holds_for_the_next_action_before_its_write_lands() {
        let reads = crate::scratch::Scratch::new("plugins-revoke-held-reads");
        let (_s, mut ed, _dir, store) = approved_kept("revoke-held", &reads);
        crate::storewrite::hold(&store, Some(0));
        ed.plugin_revoke("demo").unwrap();
        refused_now(&mut ed, &reads);
        assert!(kept_has(&store, "demo"), "the write held: the store still says approved");
        crate::storewrite::release(&store, 1);
        settle_until(&mut ed, |_| !kept_has(&store, "demo"));
        crate::storewrite::hold(&store, None);
    }

    /// …and while its write fails: refused all the same, said, tried again until it lands.
    #[test]
    fn a_revoke_holds_while_its_failed_write_is_tried_again() {
        let reads = crate::scratch::Scratch::new("plugins-revoke-failed-reads");
        let (_s, mut ed, _dir, store) = approved_kept("revoke-failed", &reads);
        crate::storewrite::fail_next(&store);
        ed.plugin_revoke("demo").unwrap();
        until_said(
            &mut ed,
            "plugin demo: permissions taken back — not kept yet: a test's failed write, trying again",
        );
        refused_now(&mut ed, &reads);
        assert!(kept_has(&store, "demo"), "failed: the store still says approved");
        // (#604: on what it asserts — the worker's write lands in the store an event before the main loop applies it
        // and memory lets the change go; landed in memory, it's in the store)
        settle_until(&mut ed, |ed| ed.plugins.as_ref().unwrap().perm_unwritten.is_empty());
        assert!(!kept_has(&store, "demo"), "tried again: the store lets it go");
    }

    /// #461, at quit: a revoke whose write was held and then failed (its retry not come yet) is written by the quit's
    /// own try — never approved again at the next start —, and nothing is said.
    #[test]
    fn a_revoke_not_written_at_quit_is_written_by_the_quit() {
        let reads = crate::scratch::Scratch::new("plugins-quit-reads");
        let (_s, mut ed, _dir, store) = approved_kept("quit", &reads);
        crate::storewrite::hold(&store, Some(0));
        ed.plugin_revoke("demo").unwrap();
        crate::storewrite::fail_next(&store);
        crate::storewrite::release(&store, 1);
        until_said(&mut ed, "not kept yet");
        crate::storewrite::hold(&store, None);
        assert!(kept_has(&store, "demo"), "its write failed");
        assert_eq!(ed.plugin_approvals_quit(perm::QUIT_WAIT), Vec::<String>::new());
        assert!(!kept_has(&store, "demo"), "the quit's write");
    }

    /// …and a write held for good is named within the quit's bound, the store as it was.
    #[test]
    fn a_write_held_at_quit_is_named_within_the_bound() {
        let reads = crate::scratch::Scratch::new("plugins-quit-held-reads");
        let (_s, mut ed, _dir, store) = approved_kept("quit-held", &reads);
        crate::storewrite::hold(&store, Some(0));
        ed.plugin_revoke("demo").unwrap();
        let t = Instant::now();
        let said = ed.plugin_approvals_quit(perm::QUIT_WAIT);
        let took = t.elapsed();
        assert_eq!(
            said,
            ["tarae: not written before quitting (1.0s waited): plugin demo: permissions taken back"]
        );
        // (the bound, and room for a loaded runner to wake — never the test's patience)
        assert!(took < perm::QUIT_WAIT * 3, "took {took:?}");
        assert!(kept_has(&store, "demo"));
        crate::storewrite::hold(&store, None);
    }

    /// #461: writes land in the order given whichever worker runs first, each on the store as it is at its turn —
    /// memory right after each, before any lands.
    #[test]
    fn approval_writes_land_in_the_order_given() {
        let reads = crate::scratch::Scratch::new("plugins-order-reads");
        let (_s, mut ed, _dir, store) = approved_kept("order", &reads);
        for revoke_last in [false, true] {
            crate::storewrite::hold(&store, Some(0));
            ed.plugin_revoke("demo").unwrap();
            assert!(!ed.plugin_approved("demo"));
            ed.plugin_approve("demo").unwrap();
            answer(&mut ed, approval, true);
            assert!(ed.plugin_approved("demo"));
            if revoke_last {
                ed.plugin_revoke("demo").unwrap();
                assert!(!ed.plugin_approved("demo"));
            }
            assert!(kept_has(&store, "demo"), "held: as before");
            crate::storewrite::hold(&store, None);
            settle_until(&mut ed, |ed| ed.plugins.as_ref().unwrap().perm_unwritten.is_empty());
            assert_eq!(kept_has(&store, "demo"), !revoke_last, "revoke last: {revoke_last}");
            assert_eq!(ed.plugin_approved("demo"), !revoke_last);
        }
    }

    /// #461: the store read while a write of ours waits (another tarae's change, our own old state) never undoes it —
    /// the read, with what isn't written yet on top.
    #[test]
    fn a_read_of_the_store_while_a_write_waits_doesnt_undo_it() {
        let reads = crate::scratch::Scratch::new("plugins-read-waits-reads");
        let (_s, mut ed, _dir, store) = approved_kept("read-waits", &reads);
        crate::storewrite::hold(&store, Some(0));
        ed.plugin_revoke("demo").unwrap();
        let mut watch = perm::ApprovalsWatch::new(Some(store.clone()));
        let read = watch.changed().expect("the first read");
        assert!(read.all.contains_key("demo"), "the store still says approved");
        ed.plugin_approvals_read(read);
        assert!(!ed.plugin_approved("demo"), "undone by a read");
        crate::storewrite::hold(&store, None);
        settle_until(&mut ed, |_| !kept_has(&store, "demo"));
    }

    /// #461: a read on its way when its permissions are taken back gives no bytes.
    #[test]
    fn a_read_whose_permission_was_taken_back_on_its_way_fails() {
        let reads = crate::scratch::Scratch::new("plugins-read-revoked-reads");
        let (_s, mut ed, _dir, _store) = approved_kept("read-revoked", &reads);
        ed.toasts.clear();
        // (straight to its worker, then the revoke before the loop applies what it read)
        ed.plugin_read("demo", 10, &reads.join("a.txt").display().to_string());
        ed.plugin_revoke("demo").unwrap();
        until_said(&mut ed, "failed 10 its permissions were taken back");
        assert!(!toasts(&ed).contains("file 10"), "{}", toasts(&ed));
    }

    /// #461: `:restart` carries a revoke not written, and the next tarae gives it again as its plugins start — its
    /// store still saying approved.
    #[test]
    fn a_revoke_not_written_is_given_again_after_restart() {
        let reads = crate::scratch::Scratch::new("plugins-restart-reads");
        let (_s, mut ed, dir, store) = approved_kept("restart", &reads);
        crate::storewrite::hold(&store, Some(0));
        ed.plugin_revoke("demo").unwrap();
        let carried = crate::restart::snapshot(&mut ed)["store_writes"]["plugins"].clone();
        assert_eq!(carried, serde_json::json!(["demo"]));
        drop(ed);
        crate::storewrite::cut(&store);
        assert!(kept_has(&store, "demo"));
        let mut ed = Editor::new(Config::default());
        ed.plugin_revokes_carried = vec!["demo".into()];
        let kept = perm::ApprovalsWatch::new(Some(store.clone())).changed();
        ed.plugins_sync_rooted(host::scan(&dir), Some(dir.join("state")), kept);
        settle_until(&mut ed, |ed| ed.plugin_command("shout").is_some());
        assert!(!ed.plugin_approved("demo"), "approved again after the restart");
    }

    /// #461, beside #457's: the approvals' disk and a `read-file`'s stay off the main thread (`perm::disk` counts by
    /// thread) — a read inside its pattern, one out through a symlink (its card shows the real path), the card's `y`,
    /// the approval's `y`, a revoke, and a read of the store the watch made, applied.
    #[test]
    fn plugin_disk_stays_off_the_main_thread() {
        let reads = crate::scratch::Scratch::new("plugins-disk-461-reads");
        let (_s, mut ed, dir) =
            setup_with("disk-461", "x\n", &abi::demo_reading(&format!("{}/**", reads.display())));
        std::fs::write(reads.join("a.txt"), "hello").unwrap();
        let outside = dir.join("outside.txt");
        std::fs::write(&outside, "secret").unwrap();
        std::os::unix::fs::symlink(&outside, reads.join("link")).unwrap();
        let store = stored(&mut ed, &dir);
        let here = || perm::disk::HERE.with(|n| n.get());
        let all = || perm::disk::ALL.load(std::sync::atomic::Ordering::Relaxed);
        let before = (here(), all());
        answer(&mut ed, approval, true);
        settle_until(&mut ed, |_| kept_has(&store, "demo"));
        ed.plugin_run("fetch", &reads.join("a.txt").display().to_string()).unwrap();
        until_said(&mut ed, "file 10 5");
        ed.plugin_run("fetch", &reads.join("link").display().to_string()).unwrap();
        settle_until(&mut ed, |ed| ed.offers.first().is_some_and(|o| ask(&o.what)));
        let real = std::fs::canonicalize(&outside).unwrap().display().to_string();
        assert!(ed.offers[0].question.contains(&real), "{}", ed.offers[0].question);
        answer(&mut ed, ask, true);
        until_said(&mut ed, "file 10 6");
        settle_until(&mut ed, |_| perm::load(Some(&store))["demo"].files == [real.clone()]);
        ed.plugin_revoke("demo").unwrap();
        settle_until(&mut ed, |_| !kept_has(&store, "demo"));
        let at = store.clone();
        let read = std::thread::spawn(move || perm::ApprovalsWatch::new(Some(at)).changed()).join().unwrap();
        ed.plugin_approvals_read(read.expect("the watch's first read"));
        assert!(!ed.plugin_approved("demo"));
        assert_eq!(here(), before.0, "the approvals' or a read's disk on the main thread");
        assert!(all() >= before.1 + 6, "the workers' touches counted ({} → {})", before.1, all());
    }

    #[test]
    fn approvals_are_kept() {
        let (_s, mut ed, dir) = setup("kept", "x\n");
        let store = stored(&mut ed, &dir);
        answer(&mut ed, approval, true);
        settle_until(&mut ed, |_| kept_has(&store, "demo"));
        let kept = perm::load(Some(&store));
        assert_eq!(
            kept["demo"].granted,
            ["env TARAE_DEMO", "network", "project", "read /tarae-demo/**", "run true"]
        );
        assert_eq!(kept["demo"].sha256, crate::sha256::hex(DEMO), "for this code");
        assert_eq!(kept["demo"].dev, None);
        std::fs::remove_dir_all(dir).ok();
    }

    /// The demo with a custom section `tag` more: the same permissions, other code (#458).
    fn build(tag: &str) -> Vec<u8> {
        let n = u8::try_from(tag.len()).unwrap();
        [DEMO, &[0, n + 1, n], tag.as_bytes()].concat()
    }

    /// `bytes` as the demo's new build — a new stamp even for the same bytes —, reloaded.
    fn rebuild(ed: &mut Editor, dir: &Path, bytes: &[u8]) {
        let path = dir.join("demo.wasm");
        let before = std::fs::metadata(&path).and_then(|m| m.modified()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        let f = std::fs::File::options().write(true).open(&path).unwrap();
        f.set_modified(before + Duration::from_secs(1)).unwrap();
        ed.toasts.clear();
        ed.plugins_sync_in(&dir.join("state"), host::scan(dir));
        until_said(ed, "Plugin demo reloaded");
    }

    /// `tarae plugin dev`'s mark (`perm::mark`, as `cli::install` writes it): `bytes` a build of `source`.
    fn marked(dir: &Path, bytes: &[u8], source: &str) {
        let marks = dir.join("state/plugin-dev");
        perm::mark(&marks, "demo", &crate::sha256::hex(bytes), Path::new(source)).unwrap();
    }

    fn card(ed: &Editor) -> Option<(String, String)> {
        ed.offers.first().filter(|o| approval(&o.what)).map(|o| (o.title.clone(), o.question.clone()))
    }

    fn held(ed: &Editor) -> bool {
        ed.plugins.as_ref().is_some_and(|p| p.held.get("demo").is_some_and(|h| h.len() == 1))
    }

    /// `exec` runs at once — approved, nothing asked.
    fn runs(ed: &mut Editor) {
        ed.toasts.clear();
        ed.plugin_run("exec", "").unwrap();
        until_said(ed, "exit 1 Some(0) []");
        assert_eq!(card(ed), None);
    }

    /// #458: an approval is for the code it was given to — a new build asking for the same list asks again, saying its
    /// code changed; what needs a permission waits on that card; the yes keeps the new code's hash and forgets the
    /// hosts and files said yes to for the old.
    #[test]
    fn a_card_up_as_new_code_loads_is_rewritten_for_it() {
        let (_s, mut ed, dir) = setup("card-renewed", "x\n");
        let (id, rev) = (ed.offers[0].id, ed.offers[0].rev);
        let v2 = build("v2");
        rebuild(&mut ed, &dir, &v2);
        // The same card, changed (`rev`), about the code loaded — one, not a second beside it
        let cards: Vec<_> = ed.offers.iter().filter(|o| approval(&o.what)).map(|o| (o.id, o.rev)).collect();
        assert_eq!(cards, [(id, rev + 1)]);
        answer(&mut ed, approval, true);
        assert!(ed.plugin_approved("demo"), "{}", toasts(&ed));
        assert_eq!(ed.plugins.as_ref().unwrap().approvals["demo"].sha256, crate::sha256::hex(&v2));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn new_code_with_the_same_list_asks_again() {
        let (_s, mut ed, dir) = setup("new-code", "x\n");
        answer(&mut ed, approval, true);
        let a = ed.plugins.as_mut().unwrap().approvals.get_mut("demo").unwrap();
        assert_eq!(a.sha256, crate::sha256::hex(DEMO));
        a.hosts.push("example.com:443".into());
        a.files.push("/etc/hosts".into());
        let v2 = build("v2");
        rebuild(&mut ed, &dir, &v2);
        let (title, question) = card(&ed).expect("asked again");
        assert_eq!(title, "Plugin demo asks for permissions");
        assert!(
            question.starts_with("Its code changed since you approved it. It asks to:\n· "),
            "{question}"
        );
        // A yes on a card still about the code before (one the load missed) approves nothing — never bytes its card
        // didn't show — and says so on the card, now the new code's
        let before = perm::Code { sha256: crate::sha256::hex(DEMO), dev: None };
        let what = &mut ed.offers[0].what;
        let crate::offer::What::PluginPermissions { code, .. } = what else { unreachable!() };
        *code = before;
        let (id, rev) = (ed.offers[0].id, ed.offers[0].rev);
        answer(&mut ed, approval, true);
        assert!(!ed.plugin_approved("demo") && !toasts(&ed).contains("approved"), "{}", toasts(&ed));
        assert_eq!(
            ed.offer_said_now(),
            Some("its code changed while this card was up — the card for the new code is up")
        );
        assert_eq!((ed.offers[0].id, ed.offers[0].rev), (id, rev + 1));
        assert!(
            matches!(&ed.offers[0].what, crate::offer::What::PluginPermissions { code, .. }
                if code.sha256 == crate::sha256::hex(&v2)),
            "the card for the new code"
        );
        ed.plugin_run("exec", "").unwrap();
        settle_until(&mut ed, held);
        answer(&mut ed, approval, true);
        until_said(&mut ed, "exit 1 Some(0) []");
        let a = &ed.plugins.as_ref().unwrap().approvals["demo"];
        assert_eq!(a.sha256, crate::sha256::hex(&v2));
        assert!(a.hosts.is_empty() && a.files.is_empty(), "{a:?}");
        std::fs::remove_dir_all(dir).ok();
    }

    /// The same bytes again (a new stamp: a `touch`, a copy of the same build) reload with their yes.
    #[test]
    fn the_same_code_reloaded_keeps_its_yes() {
        let (_s, mut ed, dir) = setup("same-code", "x\n");
        answer(&mut ed, approval, true);
        rebuild(&mut ed, &dir, DEMO);
        assert_eq!(card(&ed), None);
        runs(&mut ed);
        std::fs::remove_dir_all(dir).ok();
    }

    /// The demo approved as a dev build of `/src/a` (v1, its mark naming it).
    fn dev_approved(name: &str) -> (crate::scratch::Scratch, Editor, PathBuf) {
        let (s, mut ed, dir) = setup(name, "x\n");
        let v1 = build("v1");
        marked(&dir, &v1, "/src/a");
        rebuild(&mut ed, &dir, &v1);
        let (title, question) = card(&ed).expect("asked");
        assert_eq!(title, "Plugin demo asks for permissions · dev build");
        assert!(question.starts_with("It asks to:\n"), "{question}");
        assert!(
            question.ends_with(
                "\nA dev build from /src/a, for a folder you build yourself — its rebuilds there keep this answer."
            ),
            "{question}"
        );
        answer(&mut ed, approval, true);
        assert!(
            toasts(&ed).contains("Plugin demo: approved — a dev build, its rebuilds keep it"),
            "{}",
            toasts(&ed)
        );
        (s, ed, dir)
    }

    /// A dev build's yes holds for its folder's next builds, each named by the mark `tarae plugin dev` wrote before it
    /// put the build in place — and for the build before, loaded between the mark's write and the rename (the
    /// design session's rename window: the mark keeps the last two).
    #[test]
    fn a_dev_build_keeps_its_yes_across_rebuilds() {
        let (_s, mut ed, dir) = dev_approved("dev");
        let (v2, v3) = (build("v2"), build("v3"));
        marked(&dir, &v2, "/src/a");
        rebuild(&mut ed, &dir, &v2);
        assert_eq!(card(&ed), None);
        runs(&mut ed);
        // v3's mark written, v2 still in place and loaded again: the window
        marked(&dir, &v3, "/src/a");
        rebuild(&mut ed, &dir, &v2);
        assert_eq!(card(&ed), None, "the build before the mark's");
        rebuild(&mut ed, &dir, &v3);
        assert_eq!(card(&ed), None);
        runs(&mut ed);
        // A dev rebuild writes nothing: the approval stays at the build you said yes to
        let a = &ed.plugins.as_ref().unwrap().approvals["demo"];
        assert_eq!(
            (a.sha256.as_str(), a.dev.as_deref()),
            (crate::sha256::hex(&build("v1")).as_str(), Some(Path::new("/src/a")))
        );
        ed.plugin_permissions_list().unwrap();
        let listed = ed.doc().text.to_string();
        assert!(listed.contains("## demo — approved · dev build from /src/a"), "{listed}");
        assert!(listed.contains(&format!("code `{}`", &crate::sha256::hex(&v3)[..12])), "{listed}");
        std::fs::remove_dir_all(dir).ok();
    }

    /// A `.wasm` put in place by anything but `tarae plugin dev` — a copy, a download — isn't a build the mark names:
    /// no dev build, its code changed, asked again.
    #[test]
    fn a_build_not_from_plugin_dev_doesnt_claim_the_mark() {
        let (_s, mut ed, dir) = dev_approved("not-dev");
        let v2 = build("v2");
        marked(&dir, &v2, "/src/a");
        rebuild(&mut ed, &dir, &v2);
        assert_eq!(card(&ed), None);
        rebuild(&mut ed, &dir, &build("v3"));
        let (title, question) = card(&ed).expect("asked again");
        assert_eq!(title, "Plugin demo asks for permissions");
        assert!(question.starts_with("Its code changed since you approved it. It asks to:"), "{question}");
        assert!(!question.contains("dev build"), "{question}");
        ed.plugin_permissions_list().unwrap();
        let listed = ed.doc().text.to_string();
        assert!(listed.contains("## demo — waiting for your approval — its code changed"), "{listed}");
        std::fs::remove_dir_all(dir).ok();
    }

    /// A dev build of another folder (a crate of the same name elsewhere) doesn't take the yes given to `/src/a`'s.
    #[test]
    fn a_dev_build_from_another_folder_asks_again() {
        let (_s, mut ed, dir) = dev_approved("dev-elsewhere");
        let v2 = build("v2");
        marked(&dir, &v2, "/src/b");
        rebuild(&mut ed, &dir, &v2);
        let (title, question) = card(&ed).expect("asked again");
        assert_eq!(title, "Plugin demo asks for permissions · dev build");
        assert!(question.starts_with("Its code changed since you approved it. It asks to:"), "{question}");
        assert!(question.contains("A dev build from /src/b"), "{question}");
        std::fs::remove_dir_all(dir).ok();
    }

    /// Piece E: `:granted` lists what a plugin was approved, for every project, and `d` takes it back as
    /// `:plugin-revoke` does — the next load asks again.
    #[test]
    fn granted_lists_a_plugins_approvals_and_d_revokes_them() {
        let (_s, mut ed, dir) = setup("granted", "x\n");
        let store = stored(&mut ed, &dir);
        answer(&mut ed, approval, true);
        settle_until(&mut ed, |_| kept_has(&store, "demo"));
        ed.granted();
        settle_until(&mut ed, |ed| ed.picker.as_ref().is_some_and(|p| p.kind == crate::granted::KIND));
        let p = ed.picker.as_mut().unwrap();
        let row = p.items().iter().find(|i| i.label.starts_with("demo: ")).map(|i| i.label.clone());
        assert_eq!(
            row.as_deref(),
            Some("demo: env TARAE_DEMO, network, project, read /tarae-demo/**, run true")
        );
        let heading = p.items().iter().any(|i| i.label == "plugins · everywhere");
        assert!(heading);
        let taken =
            crate::picker::Action::Granted(Some((crate::granted::Taken::Plugin("demo".into()), Some('y'))));
        p.select_where(|a| *a == taken);
        key(&mut ed, "d");
        assert!(!ed.plugin_approved("demo"), "revoked");
        settle_until(&mut ed, |_| !kept_has(&store, "demo"));
        settle_until(&mut ed, |ed| {
            ed.picker.as_ref().is_some_and(|p| !p.items().iter().any(|i| i.label.starts_with("demo: ")))
        });
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn commands_run_and_their_edits_are_one_undo_step() {
        let (_s, mut ed, dir) = setup("run", "hello world\n");
        assert_eq!(ed.plugin_command("shout").map(|c| c.plugin.as_str()), Some("demo"));
        assert!(ed.cmd_lists.plugins.iter().any(|(n, _)| n == "shout"), "in `:` completion");
        key(&mut ed, "e"); // select "hello"
        crate::typed::execute(&mut ed, "shout").unwrap();
        settle_until(&mut ed, |ed| ed.doc().text != "hello world\n");
        assert_eq!(ed.doc().text.to_string(), "HELLO world\n");
        key(&mut ed, "u");
        assert_eq!(ed.doc().text.to_string(), "hello world\n", "one undo step");
        // Only commands registered with `text: true` see the buffer
        crate::typed::execute(&mut ed, "echo x").unwrap();
        settle_until(&mut ed, |ed| toasts(ed).contains("args="));
        assert!(toasts(&ed).contains(r#"args="x""#) && toasts(&ed).contains("text=false"), "{}", toasts(&ed));
        crate::typed::execute(&mut ed, "grab o").unwrap();
        settle_until(&mut ed, |ed| ed.doc().selection().ranges().len() == 2);
        assert_eq!(ed.doc().selection().ranges(), [Range::new(4, 5), Range::new(7, 8)]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn edits_are_refused_when_the_buffer_changed_meanwhile() {
        let (_s, mut ed, dir) = setup("stale", "a  \nb \n");
        crate::typed::execute(&mut ed, "trim-trailing").unwrap();
        // Typing before the answer arrives
        key(&mut ed, "i");
        key(&mut ed, "x");
        key(&mut ed, "esc");
        settle_until(&mut ed, |ed| toasts(ed).contains("changed while"));
        assert_eq!(ed.doc().text.to_string(), "xa  \nb \n");
        // Unchanged: applied
        crate::typed::execute(&mut ed, "trim-trailing").unwrap();
        settle_until(&mut ed, |ed| ed.doc().text == "xa\nb\n");
        assert!(toasts(&ed).contains("2 lines trimmed"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn failures_and_bad_answers_are_reported() {
        let (_s, mut ed, dir) = setup("fail", "text\n");
        for (cmd, says) in [
            ("boom", "demo: crashed (a panic?)"),
            ("stray", "demo: edit 1099511627776..1099511627776 isn't within the text"),
            ("send nonsense", "demo: plugins can run movement commands only, not “nonsense”"),
            ("notify hi", "demo: error hi"),
            ("spin", "demo: ran too long"),
        ] {
            crate::typed::execute(&mut ed, cmd).unwrap();
            settle_until(&mut ed, |ed| toasts(ed).contains(says));
        }
        assert!(toasts(&ed).contains("warning hi"));
        assert_eq!(ed.doc().text.to_string(), "text\n");
        // After all that, it still works
        key(&mut ed, "e");
        crate::typed::execute(&mut ed, "shout").unwrap();
        settle_until(&mut ed, |ed| ed.doc().text == "TEXT\n");
        std::fs::remove_dir_all(dir).ok();
    }

    /// An answer lands on the buffer and selections the plugin saw, leaves the buffer in front in front,
    /// and never lands in the middle of typing.
    #[test]
    fn answers_land_where_they_were_asked() {
        let (_s, mut ed, dir) = setup("where", "hello world\n");
        // The selection moves on before the answer: the plugin's selections are the ones replaced
        key(&mut ed, "e");
        crate::typed::execute(&mut ed, "shout").unwrap();
        key(&mut ed, "w");
        settle_until(&mut ed, |ed| ed.doc().text != "hello world\n");
        assert_eq!(ed.doc().text.to_string(), "HELLO world\n");
        // Another buffer comes to the front: it stays there, and typing goes on in it
        key(&mut ed, "u");
        ed.doc_mut().set_selection(Selection::new(vec![Range::new(0, 5)], 0));
        let first = ed.doc().id;
        crate::typed::execute(&mut ed, "shout").unwrap();
        crate::typed::execute(&mut ed, "new").unwrap();
        key(&mut ed, "i");
        key(&mut ed, "z");
        settle_until(&mut ed, |ed| ed.docs.iter().any(|d| d.id == first && d.text == "HELLO world\n"));
        key(&mut ed, "q");
        assert_eq!(ed.doc().text.to_string(), "zq");
        key(&mut ed, "esc");
        // Typing in the buffer itself (insert mode, nothing typed yet): refused, not mixed into the typing
        crate::typed::execute(&mut ed, "bc!").unwrap();
        assert_eq!(ed.doc().id, first);
        crate::typed::execute(&mut ed, "shout").unwrap();
        key(&mut ed, "i");
        settle_until(&mut ed, |ed| toasts(ed).contains("being edited"));
        assert_eq!(ed.doc().text.to_string(), "HELLO world\n");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn opening_a_file_at_a_line() {
        let (_s, mut ed, dir) = setup("goto", "");
        granted(&mut ed, &dir);
        let path = dir.join("lines.txt");
        std::fs::write(&path, "a\nb\nc\n").unwrap();
        ed.plugin_run("visit", &format!("{}:3", path.display())).unwrap();
        settle_until(&mut ed, |ed| ed.doc().path.as_deref().is_some_and(|p| p.ends_with("lines.txt")));
        let d = ed.doc();
        assert_eq!(d.text.char_to_line(d.selection().primary().cursor(&d.text)), 2);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn hints_beside_lines_until_edited_a_card_and_a_pick() {
        let (_s, mut ed, dir) = setup("beside", "");
        answer(&mut ed, approval, true);
        // (not .yaml: no language server started by a test)
        let path = dir.join("web.txt");
        std::fs::write(&path, "kind: Deployment\nreplicas: 3\n").unwrap();
        ed.open(&path).unwrap();
        let hints = |ed: &Editor| {
            ed.plugin_hints_for(ed.doc()).map(|(l, t, _)| (l, t.to_string())).collect::<Vec<_>>()
        };
        ed.plugin_run("hint", "").unwrap();
        settle_until(&mut ed, |ed| !hints(ed).is_empty());
        assert_eq!(hints(&ed), [(0, "all good".to_string()), (1, "2/3 ready".to_string())]);
        let mut frame = Vec::new();
        crate::term::render(&mut ed, &mut frame, 120, 20).unwrap();
        assert!(String::from_utf8_lossy(&frame).contains("2/3 ready"), "beside the line");
        // Edited: they wait for the plugin's next ones (the lines may have moved)
        for k in ["i", "x", "esc"] {
            key(&mut ed, k);
        }
        assert!(hints(&ed).is_empty());
        // A pick: enter chooses, esc chooses nothing
        ed.plugin_run("choose", "").unwrap();
        settle_until(&mut ed, |ed| ed.picker.is_some());
        assert_eq!(ed.picker.as_ref().unwrap().title, "demo · choose");
        key(&mut ed, "ret");
        until_said(&mut ed, r#"answer 41 Some("x")"#);
        ed.plugin_run("choose", "").unwrap();
        settle_until(&mut ed, |ed| ed.picker.is_some());
        key(&mut ed, "esc");
        until_said(&mut ed, "answer 41 None");
        // …and a press outside it, as `esc` (the mouse's rule): told nothing was chosen
        ed.plugin_run("choose", "").unwrap();
        settle_until(&mut ed, |ed| ed.picker.is_some());
        crate::term::render(&mut ed, &mut Vec::new(), 120, 20).unwrap();
        ed.toasts.clear();
        let at =
            crate::event::Mouse { kind: crate::event::MouseKind::Down, x: 2, y: 19, alt: false, ctrl: false };
        ed.handle_mouse(at);
        assert!(ed.picker.is_none(), "closed by the press");
        until_said(&mut ed, "answer 41 None");
        ed.reopen_last_picker();
        assert!(ed.picker.is_none(), "not kept for space '");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_tab_in_the_side_panel_and_a_badge() {
        use crate::sidebar::Mode as SMode;
        let (_s, mut ed, dir) = setup("side", "");
        granted(&mut ed, &dir);
        let path = dir.join("web.txt");
        std::fs::write(&path, "kind: Deployment\nreplicas: 3\n").unwrap();
        ed.open(&path).unwrap();
        ed.plugin_run("side", "").unwrap();
        settle_until(&mut ed, |ed| {
            ed.workspace.viewer.sidebar.as_ref().is_some_and(|s| s.mode == SMode::Plugin)
        });
        let s = ed.workspace.viewer.sidebar.as_ref().unwrap();
        assert!(!ed.workspace.is_at(crate::workspace::Here::Side), "shown, without the keys");

        let labels = |ed: &Editor| {
            ed.workspace
                .viewer
                .sidebar
                .as_ref()
                .unwrap()
                .lines()
                .iter()
                .map(|l| l.label.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(&ed),
            ["web.txt", "payment-gateway-reconciler-canary", "web-1", "CrashLoopBackOff"]
        );
        assert_eq!(s.here(&ed), Some(1), "the row for the code under the cursor: the deepest");
        let mut frame = Vec::new();
        crate::term::render(&mut ed, &mut frame, 140, 20).unwrap();
        let frame = String::from_utf8_lossy(&frame).into_owned();
        assert!(frame.contains("demo") && frame.contains("⎈ test"), "its tab, and the badge");
        assert!(frame.contains("deploy detail"), "a long name is cut, its state isn't");
        // Asked to: with the keys, on the row for the code under the cursor — its name cut short, whole on its card
        // (and its state after it) over the code
        ed.plugin_run("side", "focus").unwrap();
        settle_until(&mut ed, |ed| ed.workspace.is_at(crate::workspace::Here::Side));
        assert_eq!(ed.workspace.viewer.sidebar.as_ref().unwrap().selected, 1);
        let mut frame = Vec::new();
        crate::term::render(&mut ed, &mut frame, 140, 20).unwrap();
        let frame = String::from_utf8_lossy(&frame).into_owned();
        let whole = frame.find("payment-gateway-reconciler-canary").expect("its whole name");
        assert!(frame[whole..].contains("deploy detail"), "and its state");
        // h folds, l unfolds, enter on a row with no place and other keys go to the plugin
        ed.workspace.viewer.sidebar.as_mut().unwrap().selected = 0;
        key(&mut ed, "h");
        assert_eq!(labels(&ed), ["web.txt …"], "folded, and saying so");
        key(&mut ed, "l");
        assert_eq!(labels(&ed).len(), 4);
        // (a note isn't a row to land on)
        ed.workspace.viewer.sidebar.as_mut().unwrap().selected = 2;
        key(&mut ed, "j");
        assert_eq!(ed.workspace.viewer.sidebar.as_ref().unwrap().selected, 2);
        ed.workspace.viewer.sidebar.as_mut().unwrap().selected = 2;
        key(&mut ed, "ret");
        until_said(&mut ed, r#"key ret Some(2) Some("pod")"#);
        key(&mut ed, "e");
        until_said(&mut ed, r#"key e Some(2) Some("pod")"#);
        // (in the keymap's notation, the plugin API's word: never the shown `-` — #202's review)
        key(&mut ed, "minus");
        until_said(&mut ed, r#"key minus Some(2) Some("pod")"#);
        // A buffer it shows then takes the keys from its tab
        ed.plugin_run("view", "").unwrap();
        settle_until(&mut ed, |ed| ed.doc().title.as_deref() == Some("web-1 · demo"));
        assert!(!ed.workspace.is_at(crate::workspace::Here::Side));
        ed.workspace.set_at(crate::workspace::Here::Side);
        // Enter on a row with a place: there, in the code
        ed.workspace.viewer.sidebar.as_mut().unwrap().selected = 1;
        key(&mut ed, "ret");
        assert!(!ed.workspace.is_at(crate::workspace::Here::Side), "the code has the keys");
        ed.plugin_run("unside", "").unwrap();
        settle_until(&mut ed, |ed| {
            ed.plugins.as_ref().is_some_and(|p| p.side.is_none() && p.badge.is_none())
        });
        std::fs::remove_dir_all(dir).ok();
    }

    /// `project` reads the project's files — not ones named like keys and tokens (`.env`, `*.pem`, `id_rsa` —
    /// `secretnames`, data): `find` leaves them out and a read of one is asked about, as a file outside it is.
    /// It read them unasked (the DB plugin's `.env` beside its queries).
    #[test]
    fn the_project_permission_leaves_out_files_named_like_keys() {
        let (_s, mut ed, dir) = setup("project-secrets", "");
        answer(&mut ed, approval, true);
        let project = dir.join("project");
        std::fs::create_dir_all(project.join("deploy")).unwrap();
        for f in ["Cargo.toml", ".env", "deploy/server.pem", "deploy/id_ed25519"] {
            std::fs::write(project.join(f), "x = 1\n").unwrap();
        }
        let project = std::fs::canonicalize(&project).unwrap();
        for args in [&["init", "-q"][..], &["add", "-A"]] {
            let ok = std::process::Command::new("git").args(args).current_dir(&project).output().unwrap();
            assert!(ok.status.success(), "git {args:?}");
        }
        ed.workspace.root = project.clone();
        ed.plugin_run("find", "**").unwrap();
        until_said(&mut ed, r#"found 50 ["#);
        let said = toasts(&ed);
        assert!(said.contains("Cargo.toml"), "{said}");
        for secret in [".env", "server.pem", "id_ed25519"] {
            assert!(!said.contains(secret), "{secret} found: {said}");
        }
        // Read: asked about (a card), not read
        ed.plugin_run("fetch", &project.join(".env").display().to_string()).unwrap();
        settle_until(&mut ed, |ed| {
            ed.offers
                .iter()
                .any(|o| matches!(&o.what, crate::offer::What::PluginAsk { plugin, .. } if plugin == "demo"))
        });
        assert!(!toasts(&ed).contains("file 10 "), "not read: {}", toasts(&ed));
        // …a file of the project's, read unasked as before
        ed.offers.clear();
        ed.plugin_run("fetch", &project.join("Cargo.toml").display().to_string()).unwrap();
        until_said(&mut ed, "file 10 ");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_project_s_files_found_and_read_its_own_files_written_tools_offered() {
        let (_s, mut ed, dir) = setup("project", "");
        answer(&mut ed, approval, true);
        // The project is the folder tarae started in — here a repository of the test's own (git lists its
        // files: never in tarae's own repository): its files found by pattern (what git ignores, not)
        let project = dir.join("project");
        std::fs::create_dir_all(project.join("sub")).unwrap();
        std::fs::create_dir_all(project.join("ignored")).unwrap();
        for f in ["Cargo.toml", "sub/Cargo.toml", "ignored/Cargo.toml"] {
            std::fs::write(project.join(f), "[package]\nname = \"p\"\n").unwrap();
        }
        std::fs::write(project.join(".gitignore"), "ignored/\n").unwrap();
        let project = std::fs::canonicalize(&project).unwrap();
        for args in [&["init", "-q"][..], &["add", "-A"]] {
            let ok = std::process::Command::new("git").args(args).current_dir(&project).output().unwrap();
            assert!(ok.status.success(), "git {args:?}");
        }
        ed.workspace.root = project.clone();
        ed.plugin_run("find", "**/Cargo.toml").unwrap();
        until_said(&mut ed, r#"found 50 ["Cargo.toml""#);
        assert!(!toasts(&ed).contains("ignored/"), "{}", toasts(&ed));
        ed.plugin_run("find", "../**").unwrap();
        until_said(&mut ed, "failed 50 “../**”: a pattern from the project's folder");
        // …and read without being asked; a file outside it is still asked about
        let manifest = project.join("Cargo.toml");
        ed.plugin_run("fetch", &manifest.display().to_string()).unwrap();
        until_said(&mut ed, "file 10 ");
        // Its own folder (the state folder's `plugins/demo`, the test's): written, nowhere else
        ed.plugin_run("keep", "").unwrap();
        until_said(&mut ed, "written 51 ");
        let own = dir.join("state/plugins/demo/notes/a.txt");
        assert_eq!(std::fs::read_to_string(&own).unwrap(), "hello");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&own).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "the plugin's alone: {mode:o}");
        // A tool there already: installed at once; one no pack knows: said so
        ed.plugin_run("install", "sh").unwrap();
        until_said(&mut ed, "installed 52");
        ed.plugin_run("install", "kustomize-not-a-tool").unwrap();
        until_said(&mut ed, "failed 52 no language pack says how to install kustomize-not-a-tool");
        // One a pack knows (and not here): a card, the plugin's — once until quit; with no way this machine can
        // take (its pack says `brew install`, and there's no brew — Linux), the pack's hint instead
        let here = crate::packs::install("kustomize").is_some_and(|f| f.run.is_some());
        let missing =
            !crate::packs::on_path("kustomize") && crate::packs::installed_at("kustomize").is_none();
        if missing && !here {
            ed.offer_popups = true;
            ed.plugin_run("install", "kustomize").unwrap();
            until_said(
                &mut ed,
                "failed 52 kustomize: or a release from github.com/kubernetes-sigs/kustomize",
            );
            assert!(ed.offers.is_empty(), "no card");
        }
        if missing && here {
            ed.offer_popups = true;
            ed.plugin_run("install", "kustomize").unwrap();
            settle_until(&mut ed, |ed| !ed.offers.is_empty());
            let is_ours = |w: &crate::offer::What| matches!(w, crate::offer::What::Install { tool, then: crate::install::Then::Plugin { id: 52, .. }, .. } if tool == "kustomize");
            assert!(ed.offers.iter().any(|o| is_ours(&o.what)));
            ed.plugin_run("install", "kustomize").unwrap();
            until_said(&mut ed, "failed 52 kustomize was offered already");
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn subscriptions_for_some_languages_hear_only_of_those() {
        let (_s, mut ed, dir) = setup("langs", "");
        granted(&mut ed, &dir);
        for s in &mut ed.plugins.as_mut().unwrap().events.subs {
            s.2 = vec!["rust".into()];
        }
        let (txt, rs) = (dir.join("notes.txt"), dir.join("main.rs"));
        std::fs::write(&txt, "x\n").unwrap();
        std::fs::write(&rs, "fn main() {}\n").unwrap();
        ed.open(&txt).unwrap();
        ed.open(&rs).unwrap();
        settle_until(&mut ed, |ed| said(ed, "opened ", "main.rs"));
        settle_for(&mut ed, Duration::from_millis(100));
        assert!(!said(&ed, "opened ", "notes.txt"), "not a Rust buffer");
        std::fs::remove_dir_all(dir).ok();
    }

    /// Opened, changed (once input stops), saved and closed reach the demo, which answers with messages and
    /// status-line text — shown for its own buffer only.
    #[test]
    fn events_reach_their_subscribers() {
        let (_s, mut ed, dir) = setup("events", "");
        granted(&mut ed, &dir);
        let path = dir.join("notes.txt");
        std::fs::write(&path, "one two\n").unwrap();
        ed.open(&path).unwrap();
        settle_until(&mut ed, |ed| said(ed, "opened ", "notes.txt"));
        assert!(ed.plugin_status().is_empty(), "nothing changed yet");
        for k in ["i", "x", "space", "esc"] {
            press(&mut ed, k);
        }
        settle_until(&mut ed, |ed| ed.plugin_status() == ["3 words"]);
        let mut frame = Vec::new();
        crate::term::render(&mut ed, &mut frame, 120, 20).unwrap();
        assert!(String::from_utf8_lossy(&frame).contains("3 words"), "in the status line");
        crate::typed::execute(&mut ed, "w").unwrap();
        settle_until(&mut ed, |ed| ed.plugin_status().first().is_some_and(|s| s.starts_with("saved v")));
        assert_eq!(disk(&path), "x one two\n");
        crate::typed::execute(&mut ed, "new").unwrap();
        assert!(ed.plugin_status().is_empty(), "another buffer: its own status");
        crate::typed::execute(&mut ed, "bp").unwrap();
        crate::typed::execute(&mut ed, "bc").unwrap();
        settle_until(&mut ed, |ed| said(ed, "closed ", "notes.txt"));
        std::fs::remove_dir_all(dir).ok();
    }

    /// The demo adds a `!` to `.loop` files on every change: its own edit isn't reported back to it, so there's
    /// one `!`, not one after another for ever.
    #[test]
    fn a_plugins_own_edit_is_not_reported_back() {
        let (_s, mut ed, dir) = setup("loop", "");
        granted(&mut ed, &dir);
        let path = dir.join("a.loop");
        std::fs::write(&path, "ab").unwrap();
        ed.open(&path).unwrap();
        for k in ["i", "x", "esc"] {
            press(&mut ed, k);
        }
        settle_until(&mut ed, |ed| ed.doc().text.to_string().ends_with('!'));
        settle_for(&mut ed, Duration::from_millis(1000));
        assert_eq!(ed.doc().text.to_string(), "xab!");
        // A person's next edit is reported again
        for k in ["i", "y", "esc"] {
            press(&mut ed, k);
        }
        settle_until(&mut ed, |ed| ed.doc().text.to_string().matches('!').count() == 2);
        std::fs::remove_dir_all(dir).ok();
    }

    /// `before-save` edits (the demo trims `.trim` files) are in the file written, and the buffer is saved.
    #[test]
    fn before_save_edits_go_into_the_file() {
        let (_s, mut ed, dir) = setup("presave", "");
        granted(&mut ed, &dir);
        let path = dir.join("a.trim");
        std::fs::write(&path, "x  \ny \n").unwrap();
        ed.open(&path).unwrap();
        crate::typed::execute(&mut ed, "w").unwrap();
        settle_until(&mut ed, |ed| said(ed, "Saved ", "a.trim"));
        assert_eq!(disk(&path), "x\ny\n");
        assert!(!ed.doc().is_modified());
        key(&mut ed, "u");
        assert_eq!(ed.doc().text.to_string(), "x  \ny \n", "the plugin's edit is one undo step");
        std::fs::remove_dir_all(dir).ok();
    }

    /// A plugin too slow before saving: the file is saved as it is when the wait runs out, and its late answer
    /// is dropped.
    #[test]
    fn a_slow_before_save_doesnt_hold_saving() {
        let (_s, mut ed, dir) = setup("slowsave", "");
        granted(&mut ed, &dir);
        ed.plugins.as_mut().unwrap().events.wait = Duration::from_millis(50);
        let path = dir.join("a.slow");
        std::fs::write(&path, "hi\n").unwrap();
        ed.open(&path).unwrap();
        key(&mut ed, "i");
        key(&mut ed, "x");
        key(&mut ed, "esc");
        crate::typed::execute(&mut ed, "w").unwrap();
        settle_until(&mut ed, |ed| toasts(ed).contains("took too long before saving"));
        ed.flush_saves();
        assert_eq!(disk(&path), "xhi\n");
        settle_for(&mut ed, Duration::from_millis(800));
        assert!(!toasts(&ed).contains("ran too long"), "the late answer is dropped: {}", toasts(&ed));
        std::fs::remove_dir_all(dir).ok();
    }

    /// A plugin's `run` reaches movement commands, and nothing else: no `:sh`, no `:w`.
    #[test]
    fn plugins_can_only_move() {
        let (_s, mut ed, dir) = setup("only-move", "hello world\n");
        let marker = dir.join("pwned");
        for line in [format!(":sh touch {}", marker.display()), format!(":w {}", marker.display())] {
            crate::typed::execute(&mut ed, &format!("send {line}")).unwrap();
            settle_until(&mut ed, |ed| toasts(ed).contains("movement commands only"));
            ed.toasts.clear();
        }
        std::thread::sleep(Duration::from_millis(200));
        assert!(!marker.exists(), "the plugin ran a command it mustn't");
        crate::typed::execute(&mut ed, "send move_next_word_start").unwrap();
        settle_until(&mut ed, |ed| ed.doc().selection().primary().head != 0);
        std::fs::remove_dir_all(dir).ok();
    }

    /// Plugin text reaches the screen without terminal escapes, and capped.
    #[test]
    fn plugin_text_is_made_safe_to_show() {
        assert_eq!(shown("a\x1b]52;c;evil\x07b\nc"), "a\u{FFFD}]52;c;evil\u{FFFD}b\nc");
        let long = "x".repeat(MAX_TEXT + 10);
        assert_eq!(shown(&long).chars().count(), MAX_TEXT + 1);
        assert_eq!(bad_name("42"), Some("is tarae's own"));
        assert_eq!(bad_name("a\x1bb"), Some("isn't a command name"));
        assert_eq!(bad_name("shout"), None);
    }

    #[test]
    fn reloads_skips_taken_names_and_unloads() {
        let (_s, mut ed, dir) = setup("reload", "x\n");
        // A second copy registers the same names: all skipped, the first keeps them
        std::fs::write(dir.join("copy.wasm"), DEMO).unwrap();
        ed.plugins_sync(host::scan(&dir), None);
        settle_until(&mut ed, |ed| toasts(ed).contains("plugin copy: skipped"));
        assert!(toasts(&ed).contains("“shout” is taken"));
        assert_eq!(ed.plugin_command("shout").unwrap().plugin, "demo");
        // A new build of demo: reloaded
        std::fs::write(dir.join("demo.wasm"), [DEMO, &[0, 1, 0]].concat()).unwrap();
        ed.plugins_sync(host::scan(&dir), None);
        settle_until(&mut ed, |ed| toasts(ed).contains("Plugin demo reloaded"));
        std::fs::remove_file(dir.join("demo.wasm")).unwrap();
        ed.plugins_sync(host::scan(&dir), None);
        settle_until(&mut ed, |ed| ed.plugin_command("shout").is_none());
        assert_eq!(crate::typed::execute(&mut ed, "shout"), Err("no such command: 'shout'".into()));
        std::fs::remove_dir_all(dir).ok();
    }

    /// A test's plugin host keeps its plugins' files in the test's own scratch: started with the real state folder
    /// (`plugins_sync`'s), a temp path of no test's, or one climbing out of the scratch, it fails the test before its
    /// thread starts — nothing loaded, nothing written.
    #[test]
    fn a_tests_plugin_host_is_rooted_in_its_scratch() {
        let dir = crate::scratch::Scratch::new("plugins-rooted");
        std::fs::write(dir.join("demo.wasm"), DEMO).unwrap();
        let started = |state: Option<PathBuf>| {
            let mut ed = Editor::new(Config::default());
            let start = std::panic::AssertUnwindSafe(|| match &state {
                Some(s) => ed.plugins_sync_in(s, host::scan(&dir)),
                None => ed.plugins_sync(host::scan(&dir), None),
            });
            std::panic::catch_unwind(start).map_err(|e| {
                let said = e.downcast_ref::<String>().cloned().unwrap_or_default();
                assert!(said.contains("outside the test's scratch"), "{said}");
            })
        };
        for outside in [None, Some(std::env::temp_dir().join("state")), Some(dir.join("../state"))] {
            assert!(started(outside.clone()).is_err(), "{outside:?} let start");
        }
        assert!(started(Some(dir.join("state"))).is_ok());
    }

    #[test]
    fn no_plugins_no_thread() {
        let mut ed = Editor::new(Config::default());
        ed.plugins_sync(Found::new(), None);
        assert!(ed.plugins.is_none());
    }

    /// Asking a plugin costs the editor the same for a line and for 8 MB: the rope is shared, not copied.
    /// The best of a few runs each — a stall only ever adds time (a loaded runner's worst of five took 2.5
    /// ms) — and the 8 MB one against the one-line one, not an absolute: a copy costs several ms more.
    #[test]
    fn running_a_command_costs_the_same_for_any_size() {
        let best = |text: &str, name: &str| {
            let (_s, mut ed, dir) = setup(name, text);
            let best = (0..5)
                .map(|_| {
                    let t = Instant::now();
                    ed.plugin_run("trim-trailing", "").unwrap();
                    t.elapsed()
                })
                .min()
                .unwrap();
            settle_until(&mut ed, |ed| toasts(ed).contains("trimmed"));
            std::fs::remove_dir_all(dir).ok();
            best
        };
        let big: String = (0..240_000).map(|i| format!("line {i} with some text on it   \n")).collect();
        assert!(big.len() > 8 << 20);
        let (large, line) = (best(&big, "cost-large"), best("one line   \n", "cost-line"));
        // (measured: about 1 µs each; the 8 MB copied, 3 ms — 1 ms is room for noise, not for a copy)
        assert!(
            large < line + Duration::from_millis(1),
            "8 MB took {large:?} against {line:?} for a line — the text copied?"
        );
    }
}
