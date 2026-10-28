//! The keys you press, as keycaps in the corner where tarae's asks stand — for a recording of tarae at work.
//!
//! Just the keys: one short row of the last few bindings, newest at the right, in the names tarae hands over (its
//! hints' names, whatever they are — none is spelled here); a binding's caps a cell apart, bindings three; a repeat
//! folds (`j ×4`); a count is its keys (`3` `j`). Insert mode is the key that entered it and the one that left it, a
//! faint `insert` after them while it lasts — tarae never hands over what's typed.
//!
//! It asks for `keys` alone, so it reads, connects to and runs nothing, and answers only with its overlay: tarae
//! refuses anything else a plugin that hears keys answers with. It's shown for the core's reading time and fades;
//! after a pause the row starts again. Hidden while an ask stands in the corner or which-key is up — tarae's doing.
//! `:keycast-off` stops it, `:keycast` starts it again; a press on it puts it away until the next key.
wit_bindgen::generate!({ path: "../wit", world: "plugin", additional_derives: [PartialEq] });

use std::cell::RefCell;
use tarae::plugin::types::{KeyEvent, Mode, Overlay, Role, Span};

/// The bindings in the row at most.
const BINDINGS: usize = 4;
/// The row's cells at most (tarae cuts at 80, and at its end — where the newest is): older bindings go whole first.
const CELLS: usize = 60;
/// A pause this long and the row starts again — under the shortest time tarae shows an overlay (3.5 s, a toast's
/// reading time), so keys that faded never come back beside new ones.
const QUIET_MS: u64 = 3000;
const _: () =
    assert!(QUIET_MS < 3500, "under a toast's shortest reading time, tarae's hold for a short overlay");

/// One binding pressed: its keys, how many times in a row, and whether it's still under way.
#[derive(Clone, Debug, PartialEq)]
struct Binding {
    keys: Vec<String>,
    times: u32,
    done: bool,
}

#[derive(Debug)]
struct State {
    on: bool,
    row: Vec<Binding>,
    /// The last key left the editor in insert mode.
    inserting: bool,
    /// When the last keys came (ms).
    last: Option<u64>,
}

impl Default for State {
    fn default() -> State {
        State { on: true, row: Vec::new(), inserting: false, last: None }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

impl State {
    /// Keys heard at `now`: the overlay they make.
    fn heard(&mut self, events: Vec<KeyEvent>, now: u64) -> Vec<Action> {
        if !self.on {
            return Vec::new();
        }
        if self.last.is_some_and(|t| now.saturating_sub(t) >= QUIET_MS) {
            (self.row, self.inserting) = (Vec::new(), false);
        }
        self.last = Some(now);
        for ev in events {
            self.key(ev);
        }
        vec![self.overlay()]
    }

    fn key(&mut self, ev: KeyEvent) {
        // (a sequence under way is replaced by what comes next: its next key, or its end)
        if self.row.last().is_some_and(|b| !b.done) {
            self.row.pop();
        }
        self.inserting = ev.mode == Mode::Insert;
        // (none named: insert mode left by a binding of letters — only the `insert` goes)
        if ev.keys.is_empty() {
            return;
        }
        match self.row.last_mut() {
            Some(b) if ev.done && b.keys == ev.keys => b.times += 1,
            _ => self.row.push(Binding { keys: ev.keys, times: 1, done: ev.done }),
        }
        let over = self.row.len().saturating_sub(BINDINGS);
        self.row.drain(..over);
    }

    fn overlay(&self) -> Action {
        let row = spans(&self.row, self.inserting);
        Action::Overlay((!row.is_empty()).then(|| Overlay { rows: vec![row], hold: None }))
    }
}

fn text(s: &str, role: Role) -> Span {
    Span { text: s.to_string(), role }
}

/// A span's cells as tarae draws it: a keycap has a cell of its ground each side. (Key names are a cell a character:
/// letters, `↑`, `×`.)
fn width(s: &Span) -> usize {
    s.text.chars().count() + if s.role == Role::Key { 2 } else { 0 }
}

fn binding(b: &Binding) -> Vec<Span> {
    let mut out = Vec::new();
    for (i, k) in b.keys.iter().enumerate() {
        if i > 0 {
            out.push(text(" ", Role::Text));
        }
        out.push(text(k, Role::Key));
    }
    if b.times > 1 {
        out.push(text(&format!(" ×{}", b.times), Role::Text));
    }
    out
}

/// The row: the newest bindings that fit in `CELLS`, older ones dropped whole; one alone too wide keeps its last keys
/// after a faint `…`.
fn spans(row: &[Binding], inserting: bool) -> Vec<Span> {
    let tail: Vec<Span> =
        if inserting { vec![text("   ", Role::Text), text("insert", Role::Faint)] } else { vec![] };
    let mut used: usize = tail.iter().map(width).sum();
    let mut kept: Vec<Vec<Span>> = Vec::new();
    for b in row.iter().rev() {
        let spans = binding(b);
        let gap = if kept.is_empty() { 0 } else { 3 };
        let w: usize = spans.iter().map(width).sum::<usize>() + gap;
        if used + w <= CELLS {
            used += w;
            kept.push(spans);
            continue;
        }
        if kept.is_empty() {
            kept.push(cut(b, CELLS.saturating_sub(used)));
        }
        break;
    }
    let mut out = Vec::new();
    for (i, spans) in kept.into_iter().rev().enumerate() {
        if i > 0 {
            out.push(text("   ", Role::Text));
        }
        out.extend(spans);
    }
    if !out.is_empty() {
        out.extend(tail);
    }
    out
}

/// `b` in `cells`: its first keys dropped for a faint `… ` until the rest fits (its last key always kept).
fn cut(b: &Binding, cells: usize) -> Vec<Span> {
    let mut keys = b.keys.clone();
    loop {
        let mut spans = vec![text("… ", Role::Faint)];
        spans.extend(binding(&Binding { keys: keys.clone(), ..b.clone() }));
        if keys.len() == 1 || spans.iter().map(width).sum::<usize>() <= cells {
            return spans;
        }
        keys.remove(0);
    }
}

struct Keycast;

impl Guest for Keycast {
    fn commands() -> Vec<Command> {
        vec![
            Command { name: "keycast".into(), doc: "Show the keys you press".into(), text: false },
            Command {
                name: "keycast-off".into(),
                doc: "Stop showing the keys you press".into(),
                text: false,
            },
        ]
    }

    fn subscriptions() -> Vec<Subscription> {
        Vec::new()
    }

    /// `keys` alone: with anything else tarae doesn't load a plugin that hears keys.
    fn permissions() -> Vec<Permission> {
        vec![Permission::Keys]
    }

    fn run(name: String, _cx: Context) -> Vec<Action> {
        STATE.with_borrow_mut(|s| {
            *s = State { on: name == "keycast", ..State::default() };
            if s.on { Vec::new() } else { vec![Action::Overlay(None)] }
        })
    }

    fn on(_event: Event, _cx: Context) -> Vec<Action> {
        Vec::new()
    }

    fn on_events(events: Vec<HostEvent>, now: Now) -> Vec<Action> {
        STATE.with_borrow_mut(|s| {
            let mut out = Vec::new();
            for e in events {
                match e {
                    HostEvent::Keys(keys) => out.extend(s.heard(keys, now.millis)),
                    // (a press on it: put away until the next key)
                    HostEvent::Pressed(_) if s.on => {
                        (s.row, s.inserting) = (Vec::new(), false);
                        out.push(Action::Overlay(None));
                    }
                    _ => {}
                }
            }
            out
        })
    }
}

export!(Keycast);

#[cfg(test)]
mod tests;
