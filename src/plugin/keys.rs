//! The keys you press, to the plugins that hear them (`.sb/overlays-plan.md` A2) — **commands only**: a key is heard
//! as the layer of `keys_to()`'s chain that takes it treats it (`Takes`), never as text.
//!
//! - **Gated three ways**: your own config's `plugins.keys` (off by default, `trust::YOURS_ONLY`), the plugin's `keys`
//!   permission approved on its card, and the status line naming who hears while one does (`term.rs`, `keys → …`).
//!   A plugin that asks for `keys` asks for nothing else — no `network`, `run`, `read`, `env` or `project`
//!   (`perm::invalid`: it doesn't load, nor is it approved).
//! - **What it heard stays in tarae** (`refused`): it answers with its overlay and its timers — never an edit, a
//!   message, a panel, a buffer, a read, a file of its own or anything else a plugin, the disk or Claude could read
//!   again; refused at apply (`plugin_apply`), whatever called it, said once a load.
//! - **What's heard** (`heard`): a command layer's keys, the sequence so far in the names tarae shows (`Key`'s
//!   `Display`, #202: `enter`, `-`, `↑` — never the keymap's notation a panel's `key` event uses) — a count's digits,
//!   `space f`, `C-w h` — and whether it's done; a card's or the edit review's answers (a letter only if it's one of
//!   them); in a line being typed (a prompt, a picker, the chat's input) only `enter` and `esc`; in insert
//!   mode only the key that left it (none named when a binding of characters did); never the terminal's program's keys
//!   (only tarae's own `[terminal]` rows), pointing's labels, the minimized chat's first key, nor the character a
//!   command waits for (`f x`, `r x`, `"a` — `f`, `r`, `"` alone).
//! - **Only your keys**: the terminal's, where they come in (`handle_event`) — never one tarae feeds itself (`.`'s
//!   replay, a macro, a press that types a `:` line).
//! - **Nothing waits on it**: built only while one hears, queued to its plugin's outbox — what comes while a call is in
//!   flight joins one `keys` event (`plugin_queue`), the newest `QUEUED_MAX` kept.

use super::abi::{self, Action, HostEvent, KeyEvent};
use crate::editor::{Editor, Mode};
use crate::key::{Code, Key};
use crate::uistate::Takes;

/// The key events one call to a plugin carries at most: a burst while it's busy keeps the newest (a keycast shows the
/// last few).
pub(super) const QUEUED_MAX: usize = 64;

/// What a key's hearing needs from before it's handled.
pub(crate) struct Before {
    takes: Takes,
    /// The key goes to the code in insert mode (typing).
    typing: bool,
    /// The sequence under way, as the hints name it (a count's digits first).
    pending: Vec<String>,
    /// A command waits for a character (`f`, `r`, `"`, a `gw` label): this key is it.
    char_next: bool,
    /// A line is being typed: a prompt or a picker is open.
    modal: bool,
    /// What the card or the edit review that has the keys answers to.
    answers: Vec<char>,
    /// The terminal's program has the keys, and this is one of tarae's own `[terminal]` rows (`C-\`).
    terminal_row: bool,
}

fn names(pending: &str) -> Vec<String> {
    pending.split(' ').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

fn mode_of(m: Mode) -> abi::Mode {
    match m {
        Mode::Normal => abi::Mode::Normal,
        Mode::Insert => abi::Mode::Insert,
        Mode::Select => abi::Mode::Select,
    }
}

/// A line's own acts: `enter` and `esc`, nothing held with them.
fn act(key: Key) -> bool {
    matches!(key.code, Code::Enter | Code::Esc) && !key.ctrl && !key.alt
}

/// What a plugin that hears keys may not answer with — the kind, in words; None: it may (§7 1, (c)). Only what shows
/// on your screen and nowhere else is let through: its overlay (Claude's level never holds one), its timers and a
/// stop (of nothing: it runs no program). The rest would put what it heard where it's read again: your text (saved,
/// sent to other plugins, read by Claude), the cursor (Claude's context, the next plugin's input), a message (the
/// log, Claude's `ui`), status text, a panel, a side tab, a badge, hints, a buffer, a pick, a prompt, a path opened
/// (recent files, Claude's open files), a read (the path it picks is the filesystem's lookup, a server's on a network
/// mount — the load refuses its patterns too: `perm::invalid`), a program, a connection, a file of its own, an
/// install. A new action is decided here (the match is whole).
pub(super) fn refused(a: &Action) -> Option<&'static str> {
    Some(match a {
        Action::Overlay(_) | Action::Timer(..) | Action::Stop(_) => return None,
        Action::ReadFile(..) | Action::ReadEnv(..) | Action::Find(..) => "a read",
        Action::ReplaceSelections(_) | Action::Edits(_) => "an edit",
        Action::Select(_) | Action::Run(_) => "a move",
        Action::Status(_) | Action::Success(_) | Action::Warning(_) | Action::Error(_) => "a message",
        Action::StatusBar(_) => "status text",
        Action::Open(_) => "a file opened",
        Action::Spawn(_) => "a program",
        Action::Connect(_) | Action::Send(..) => "a connection",
        Action::Panel(_) => "a panel",
        Action::Prompt(..) => "a prompt",
        Action::Show(_) | Action::Append(..) => "a buffer",
        Action::Hints(_) => "hints",
        Action::Side(_) => "a side tab",
        Action::Pick(_) => "a pick",
        Action::Badge(_) => "a badge",
        Action::WriteOwn(..) => "a file of its own",
        Action::OfferInstall(..) => "an install",
    })
}

/// What a plugin hears of `key`, from how it was taken (`b`) and where things are after it (`pending`, `mode`) —
/// None: nothing.
fn heard(key: Key, b: &Before, pending: &[String], mode: Mode) -> Option<KeyEvent> {
    let one = |keys: Vec<String>| Some(KeyEvent { keys, done: true, mode: mode_of(mode) });
    // (a line being typed: only its acts — whatever layer took the key)
    if b.modal && !act(key) {
        return None;
    }
    // (tarae's own `[terminal]` rows are commands — `C-\\`, the window prefix taken from the program)
    let takes = match b.takes {
        Takes::Program if b.terminal_row => Takes::Commands,
        t => t,
    };
    match takes {
        Takes::Label | Takes::Unfold | Takes::Program => None,
        Takes::Answers => match key.plain_char() {
            Some(c) if !b.answers.contains(&c) => None,
            _ => one(vec![key.to_string()]),
        },
        // (insert mode: only the key that left it — unnamed when it's a character, a binding of letters' last)
        Takes::Text if b.typing => match mode {
            Mode::Insert => None,
            _ if key.plain_char().is_some() => one(Vec::new()),
            _ => one(vec![key.to_string()]),
        },
        Takes::Text => act(key).then(|| one(vec![key.to_string()]))?,
        // (the character a command waited for: never — the sequence before it, done)
        Takes::Commands if b.char_next => (!b.pending.is_empty()).then(|| one(b.pending.clone()))?,
        Takes::Commands => {
            let mut keys = b.pending.clone();
            keys.push(key.to_string());
            // (under way: this key began or carried on the sequence — a sticky one, `Z`, stays as it was: done)
            if !pending.is_empty() && pending == keys.as_slice() {
                return Some(KeyEvent { keys, done: false, mode: mode_of(mode) });
            }
            one(keys)
        }
    }
}

impl Editor {
    /// The plugins that hear the keys you press now: your `plugins.keys` on, each one's `keys` approved — from memory
    /// only (the key path reads no file).
    pub(crate) fn keys_heard_by(&self) -> Vec<&str> {
        let Some(p) = self.plugins.as_ref().filter(|_| self.config.plugins_keys) else { return Vec::new() };
        let mut v: Vec<&str> = (p.permissions.iter())
            .filter(|(name, d)| super::perm::hears_keys(d) && self.plugin_approved(name))
            .map(|(name, _)| name.as_str())
            .collect();
        crate::sorting::sort(&mut v);
        v
    }

    /// Before your key is handled: what its hearing needs — None while no plugin hears keys (nothing built).
    pub(crate) fn keys_before(&self, key: Key) -> Option<Before> {
        if self.keys_heard_by().is_empty() {
            return None;
        }
        let k = self.keys_to();
        let to = k.route(key);
        Some(Before {
            takes: self.takes_of(to),
            typing: to == crate::uistate::Region::Code && self.mode == Mode::Insert,
            pending: names(&k.pending),
            char_next: k.char_next,
            modal: self.picker.is_some() || self.prompt.is_some(),
            answers: k.answers,
            terminal_row: self.workspace.program_has_keys()
                && self.pending.is_empty()
                && self.keymaps.terminal_takes(key).is_some(),
        })
    }

    /// Whether `plugin` declared `keys` — held to `refused` whether it hears now or not: what it heard before a revoke,
    /// or while your setting was on, is still its.
    pub(super) fn plugin_hears_keys(&self, plugin: &str) -> bool {
        (self.plugins.as_ref())
            .and_then(|p| p.permissions.get(plugin))
            .is_some_and(|d| super::perm::hears_keys(d))
    }

    /// `a`, from a plugin that hears keys, refused (`refused`: `what`): a request it waits on fails, and you're told
    /// once a load — in tarae's words, nothing of what it answered.
    pub(super) fn plugin_keys_refuse(&mut self, plugin: &str, a: &Action, what: &str) {
        let why = format!("{what}: a plugin that hears keys shows what it hears in its overlay only");
        let id = match a {
            Action::Spawn(s) => Some(s.id),
            Action::Connect(c) => Some(c.id),
            Action::Pick(p) => Some(p.id),
            Action::Prompt(id, _)
            | Action::WriteOwn(id, ..)
            | Action::OfferInstall(id, _)
            | Action::ReadFile(id, _)
            | Action::ReadEnv(id, _)
            | Action::Find(id, _) => Some(*id),
            _ => None,
        };
        if let Some(id) = id {
            self.plugin_events(plugin, vec![HostEvent::Failed(id, why)]);
        }
        let Some(o) = self.plugins.as_mut().and_then(|p| p.outbox.get_mut(plugin)) else { return };
        if !std::mem::replace(&mut o.keys_refused, true) {
            self.set_warning(format!(
                "plugin {plugin} hears keys: {what} refused — it shows what it hears in its overlay only"
            ));
        }
    }

    /// What a plugin hears of your key, now that it's handled (`b`: from before it).
    fn key_heard(&self, key: Key, b: &Before) -> Option<KeyEvent> {
        heard(key, b, &names(&self.keys_to().pending), self.mode)
    }

    /// Your key handled: what it was as a command, to every plugin that hears keys.
    pub(crate) fn keys_after(&mut self, key: Key, b: Before) {
        let Some(ev) = self.key_heard(key, &b) else { return };
        let to: Vec<String> = self.keys_heard_by().into_iter().map(str::to_string).collect();
        for plugin in to {
            self.plugin_queue(&plugin, vec![HostEvent::Keys(vec![ev.clone()])], 0);
        }
    }
}

#[cfg(test)]
mod tests;
