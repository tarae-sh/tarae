//! The floating lists (panes P3 steps 7a and 7c): the session list (`sessionpane.rs`), `:messages`
//! (`messages.rs`) and `:folders` (`folders.rs`) — the viewer's, not a session's (no member of `Workspace.groups`), each in a named frame over the
//! code and the chat (`Frame::LIST`: 3/5 wide, as tall as its rows up to half the screen, its top at 1/4). One list's
//! state, keys, search and mouse for both, so the two never drift apart; each kind gives its rows' ids, what its
//! search matches, its `enter` and its own letters.
//!
//! - Its keys are a list's (`key::list_move`), `/`·`?` and `n`·`N` its own search (its matches lit — never
//!   type-to-narrow: its letters are keys), `esc code` gives the keys back with the list staying in sight, `q`
//!   closes it; a press on a row selects it, the same row again opens it as `enter`.
//! - **Done once used** (user, 2026-10-23): a floating list closes as what it was opened for is done — the session
//!   list's `enter` switching (zellij's session manager, tmux's choose-tree), the messages' `enter` opening one —
//!   floating panes sit in the middle of the screen, so a list that stayed would cover the code (`used`). A docked
//!   one stays: the session list docks (`C-w H/K/L`, `Sizes.sessions_dock` — P3 step 7d), `:messages` only floats.

use crate::editor::Editor;
use crate::key::{Code, Key, ListMove};
use crate::pane::GroupKind;
use crate::split::PaneId;
use crate::workspace::Here;

/// A list's view as last drawn (`hold`) — a floating list's, a group's list's, `:procs`', a plugin's panel's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    /// The selected row's id.
    selected: Option<u64>,
    /// The rows in sight.
    rows: usize,
    /// The first row in sight, and its id.
    top: usize,
    first: Option<u64>,
    /// The rows it had.
    n: usize,
    /// The wheel moved the view since a key last brought it to the selection (`wheel_to`, `unwheel`).
    wheeled: bool,
}

/// The wheel puts a list's view at `to`: it stays there (`hold`) until a key of the list's brings it back (`unwheel`).
pub fn wheel_to(top: &std::cell::Cell<usize>, shown: &std::cell::Cell<Option<Shown>>, to: usize) {
    top.set(to);
    shown.set(Some(Shown { wheeled: true, ..shown.get().unwrap_or_default() }));
}

/// Whether the wheel moved the list's view since a key last brought it to the selection.
pub fn wheeled(shown: &std::cell::Cell<Option<Shown>>) -> bool {
    shown.get().is_some_and(|w| w.wheeled)
}

/// A key of the list's: its view goes back to the selection at the next frame — whether the wheel had moved it.
pub fn unwheel(shown: &std::cell::Cell<Option<Shown>>) -> bool {
    let was = shown.get();
    shown.set(was.map(|w| Shown { wheeled: false, ..w }));
    was.is_some_and(|w| w.wheeled)
}

/// A floating list while it's up.
#[derive(Clone, Debug)]
pub struct FloatList {
    /// Its pane's id (`At::List`, the state's `pane#N`).
    pub list: PaneId,
    /// The row selected, by its id (a session's, a message's) — kept as rows come and go (one gone: the row there
    /// now).
    pub selected: u64,
    /// Its first row in sight, as last drawn.
    pub top: std::cell::Cell<usize>,
    /// The view as last drawn: while the selection and the rows in sight stay, the view stays where the wheel left it,
    /// on its rows as others come and go (`top_drawn`).
    pub shown: std::cell::Cell<Option<Shown>>,
    /// Its own search, as every pane's (never the code's): the last pattern, its matches lit while `hl`.
    pub search: Option<String>,
    pub hl: bool,
    /// A `/`·`?` it opened is its until it closes: the row selected as it opened (`esc` goes back to it).
    pub prompt: Option<u64>,
}

/// The kinds that are floating lists.
pub const KINDS: [GroupKind; 3] = [GroupKind::Sessions, GroupKind::Messages, GroupKind::Folders];

/// Kind `kind` is a floating list of the viewer's.
pub fn is_list(kind: GroupKind) -> bool {
    KINDS.contains(&kind)
}

/// What a list is called where a key it doesn't take is said.
pub(crate) fn what(kind: GroupKind) -> &'static str {
    match kind {
        GroupKind::Messages => "the messages",
        GroupKind::Folders => "the folders",
        _ => "the session list",
    }
}

impl Editor {
    /// Kind `kind`'s floating list, while it's up.
    pub fn float_list(&self, kind: GroupKind) -> Option<&FloatList> {
        match kind {
            GroupKind::Sessions => self.sessions_pane.as_ref(),
            GroupKind::Messages => self.messages_pane.as_ref(),
            GroupKind::Folders => self.folders_pane.as_ref(),
            _ => None,
        }
    }

    pub fn float_list_mut(&mut self, kind: GroupKind) -> Option<&mut FloatList> {
        self.float_list_place(kind)?.as_mut()
    }

    fn float_list_place(&mut self, kind: GroupKind) -> Option<&mut Option<FloatList>> {
        match kind {
            GroupKind::Sessions => Some(&mut self.sessions_pane),
            GroupKind::Messages => Some(&mut self.messages_pane),
            GroupKind::Folders => Some(&mut self.folders_pane),
            _ => None,
        }
    }

    /// Its rows' ids, in its order (the session list's: its workspaces' numbers — all this machine's).
    pub fn float_list_ids(&self, kind: GroupKind) -> Vec<u64> {
        match kind {
            GroupKind::Sessions => self.sessions_by_parent().iter().map(|l| l.id.n).collect(),
            GroupKind::Messages => self.message_ids(),
            GroupKind::Folders => self.folder_ids(),
            _ => Vec::new(),
        }
    }

    /// Kind `kind`'s list up with the keys — opened, or up already — `select` the row in hand when given (else, opened,
    /// `first`).
    pub(crate) fn float_list_open(&mut self, kind: GroupKind, first: u64, select: Option<u64>) {
        let next = self.next_pane;
        let Some(place) = self.float_list_place(kind) else { return };
        let made = place.is_none();
        let p = place.get_or_insert_with(|| FloatList {
            list: next,
            selected: first,
            top: Default::default(),
            shown: Default::default(),
            search: None,
            hl: false,
            prompt: None,
        });
        if let Some(id) = select {
            p.selected = id;
        }
        let list = p.list;
        if made {
            self.next_pane += 1;
        }
        self.list_takes_keys(list);
    }

    /// The keys to kind `kind`'s list, up (a press on it, `C-w w`).
    pub fn float_list_focus(&mut self, kind: GroupKind) {
        if let Some(list) = self.float_list(kind).map(|p| p.list) {
            self.list_takes_keys(list);
        }
    }

    /// A floating list takes the keys: only one floating pane has them — a card that had them lets go (it waits, `C-q`
    /// focuses it again), else `card_key`, first in `handle_key`, would still take every key while the list looks lit.
    fn list_takes_keys(&mut self, list: crate::split::PaneId) {
        self.card_focus = None;
        self.workspace.set_at(Here::List(list));
    }

    /// Kind `kind`'s list has the keys.
    pub fn float_list_keys(&self, kind: GroupKind) -> bool {
        self.float_list(kind).is_some_and(|p| self.workspace.docked() == Here::List(p.list))
    }

    /// The floating list with the keys, if one has them.
    pub fn float_list_in_hand(&self) -> Option<GroupKind> {
        KINDS.into_iter().find(|&k| self.float_list_keys(k))
    }

    /// The floating list whose pane is `id`.
    pub fn float_list_of(&self, id: PaneId) -> Option<GroupKind> {
        KINDS.into_iter().find(|&k| self.float_list(k).is_some_and(|p| p.list == id))
    }

    /// `q` on a list: it goes, the keys to the code.
    pub fn float_list_close(&mut self, kind: GroupKind) {
        if let Some(p) = self.float_list_place(kind).and_then(Option::take) {
            self.workspace.leave(Here::List(p.list));
        }
        self.float_moving = self.float_moving.filter(|&k| k != kind);
    }

    /// The list's keys go back to the code — it stays in sight.
    pub(crate) fn float_list_leave(&mut self, kind: GroupKind) {
        if let Some(p) = self.float_list(kind) {
            let list = p.list;
            self.workspace.leave(Here::List(list));
        }
    }

    /// Done once used (user, 2026-10-23): what a list was opened for is done — floating, it closes; docked it stays,
    /// its keys back to the code (the manager's overview kept in sight — P3 step 7d).
    pub(crate) fn float_list_used(&mut self, kind: GroupKind) {
        // (docked by its place — floating only while it has the keys where its column has no room: #414)
        match self.list_docked(kind) || self.docked_in_sight(kind) {
            true => self.float_list_leave(kind),
            false => self.float_list_close(kind),
        }
    }

    /// Kind `kind`'s list is docked (the session list, `Sizes.sessions_dock`) — up or not, its place; in sight there
    /// is `docked_in_sight` (else it floats for the frame — `floating_in_sight`).
    pub fn list_docked(&self, kind: GroupKind) -> bool {
        kind == GroupKind::Sessions && self.sizes.sessions_dock.is_some()
    }

    /// Kind `kind`'s list is up and docked where its column has no room, the keys elsewhere: it draws nothing — never
    /// over the code or the chat you read; the chat's top line names the sessions — and comes back as room does. Given
    /// the keys (`space W`, `C-w w`'s round, `cycle_view`) it floats for that time (P3 step 7d; #414, the user
    /// 2026-11-06).
    pub fn list_unshown(&self, kind: GroupKind) -> bool {
        self.float_list(kind).is_some()
            && self.list_docked(kind)
            && !self.docked_in_sight(kind)
            && !self.float_list_keys(kind)
    }

    /// The row the list's selection is on (by its id, else its first row).
    pub fn float_list_here(&self, kind: GroupKind) -> Option<u64> {
        let p = self.float_list(kind)?;
        let ids = self.float_list_ids(kind);
        ids.contains(&p.selected).then_some(p.selected).or_else(|| ids.first().copied())
    }

    /// The selection on row `i` (in the list's order), `top` its first row in sight.
    pub(crate) fn float_list_select(&mut self, kind: GroupKind, i: usize, top: usize) {
        let id = self.float_list_ids(kind).get(i).copied();
        if let (Some(p), Some(id)) = (self.float_list_mut(kind), id) {
            p.selected = id;
            p.top.set(top);
        }
    }

    /// A press on row `i` of the list: the keys its, that row selected — the same row again within `DOUBLE_CLICK`
    /// opens it, as `enter`.
    pub fn float_list_click(&mut self, kind: GroupKind, i: usize) {
        let Some(p) = self.float_list(kind) else { return };
        let top = p.top.get();
        self.float_list_focus(kind);
        self.float_list_select(kind, i, top);
        if self.clicked_again(kind.name(), i) {
            self.float_list_enter(kind);
        }
    }

    /// `enter` on the list: what its kind does with the row in hand.
    fn float_list_enter(&mut self, kind: GroupKind) {
        match kind {
            GroupKind::Messages => self.messages_enter(),
            GroupKind::Folders => self.folders_enter(),
            _ => self.sessions_enter(),
        }
    }

    /// The list's frame: the one you set (moved or sized, kept with the sizes), else a list's (`Frame::LIST`) — `h` 0
    /// while it's as tall as its rows.
    pub fn float_list_frame(&self, kind: GroupKind) -> Option<crate::pane::Frame> {
        self.float_list(kind)?;
        let kept = match kind {
            GroupKind::Messages => self.sizes.messages,
            GroupKind::Folders => self.sizes.folders,
            _ => self.sizes.sessions,
        };
        Some(kept.unwrap_or(crate::pane::Frame::LIST))
    }

    /// The list's cells on a screen `w` wide with `h` rows above the status line (`Frame::rect_rows`).
    pub fn float_list_rect(&self, kind: GroupKind, w: usize, h: usize) -> Option<crate::split::Rect> {
        // (docked in sight: a leaf of the tree, not a frame; docked with no room and no keys: nothing)
        if self.docked_in_sight(kind) || self.list_unshown(kind) {
            return None;
        }
        Some(self.float_list_frame(kind)?.rect_rows(w, h, self.float_list_ids(kind).len()))
    }

    /// The list's kept frame (`Sizes`): set, or back to a list's (None).
    pub(crate) fn float_list_frame_set(&mut self, kind: GroupKind, f: Option<crate::pane::Frame>) {
        match kind {
            GroupKind::Messages => self.sizes.messages = f,
            GroupKind::Folders => self.sizes.folders = f,
            _ => self.sizes.sessions = f,
        }
    }

    /// `/`·`?` on the list: its own search's prompt, from the row in hand.
    fn float_list_search_open(&mut self, kind: GroupKind, reverse: bool) {
        let here = self.float_list_here(kind);
        if let Some(p) = self.float_list_mut(kind) {
            p.prompt = here;
        }
        self.open_prompt(crate::editor::PromptKind::Search { reverse }, "");
    }

    /// The row `pat` matches next after `from`'s (before it, `reverse`), round the list's end (said). Typing: only
    /// the selection moves, an unfinished pattern leaves it; entered: kept as the list's search, lit, and a pattern
    /// that isn't one or matches nothing said.
    fn float_list_search(&mut self, kind: GroupKind, pat: &str, reverse: bool, from: u64, typing: bool) {
        let re = match crate::search::compile(pat) {
            Ok(re) => re,
            Err(e) if !typing => return self.set_error(e),
            Err(_) => return,
        };
        if !typing && let Some(p) = self.float_list_mut(kind) {
            (p.search, p.hl) = (Some(pat.to_string()), true);
        }
        let ids = self.float_list_ids(kind);
        let hits = matched(self, kind, &re);
        let at = ids.iter().position(|&id| id == from).unwrap_or(0);
        let n = ids.len();
        let order: Vec<usize> = match reverse {
            false => (1..=n).map(|k| (at + k) % n).collect(),
            true => (1..=n).map(|k| (at + n - k) % n).collect(),
        };
        let Some(&i) = order.iter().find(|&&i| hits.get(i).copied().unwrap_or(false)) else {
            if !typing {
                let none = match kind {
                    GroupKind::Messages => "no message",
                    GroupKind::Folders => "no folder",
                    _ => "no session",
                };
                self.set_error(format!("{none} matches /{pat}/"));
            }
            return;
        };
        if let Some(p) = self.float_list_mut(kind) {
            p.selected = ids[i];
        }
        if !typing && if reverse { i >= at } else { i <= at } {
            self.set_status("search wrapped around");
        }
    }

    /// `n`·`N` on the list: its search's next row (the other way).
    fn float_list_search_next(&mut self, kind: GroupKind, reverse: bool) {
        let (Some(pat), Some(here)) =
            (self.float_list(kind).and_then(|p| p.search.clone()), self.float_list_here(kind))
        else {
            return self.set_status("no search in the list yet — / searches it");
        };
        self.float_list_search(kind, &pat, reverse, here, false);
    }
}

/// Which of the list's rows `re` matches, in its order.
fn matched(ed: &Editor, kind: GroupKind, re: &regex::Regex) -> Vec<bool> {
    match kind {
        GroupKind::Messages => crate::messages::rows(ed).iter().map(|r| re.is_match(&r.text)).collect(),
        GroupKind::Folders => crate::folders::rows(ed).iter().map(|r| re.is_match(&r.text)).collect(),
        _ => ed.sessions_rows().iter().map(|r| crate::sessionpane::matches(r, re)).collect(),
    }
}

/// The pattern kind `kind`'s list lights: the one its `/` is typing, else its last search while lit.
pub fn lit(ed: &Editor, kind: GroupKind) -> Option<regex::Regex> {
    let p = ed.float_list(kind)?;
    let typed = ed.prompt.as_ref().filter(|_| p.prompt.is_some()).map(|t| t.text());
    let pat = match typed {
        Some(t) => Some(t).filter(|t| !t.is_empty()),
        None => p.search.clone().filter(|_| p.hl),
    }?;
    crate::search::compile(&pat).ok()
}

/// A list's own `/`·`?` is up (the code's search prompt isn't the code's).
pub fn searching(ed: &Editor) -> bool {
    KINDS.into_iter().any(|k| ed.float_list(k).is_some_and(|p| p.prompt.is_some()))
}

/// A key while a list's own `/`·`?` is up — false: the prompt isn't a list's. `esc` goes back to the row it opened on,
/// `enter` searches (empty: its last pattern again), typing moves the selection to the next match as it goes.
pub fn prompt_key(ed: &mut Editor, key: Key) -> bool {
    use crate::editor::PromptKind;
    let Some((kind, from)) =
        KINDS.into_iter().find_map(|k| ed.float_list(k).and_then(|p| p.prompt).map(|from| (k, from)))
    else {
        return false;
    };
    let Some(PromptKind::Search { reverse }) = ed.prompt.as_ref().map(|p| p.kind) else {
        // (another prompt now: the list's went)
        if let Some(p) = ed.float_list_mut(kind) {
            p.prompt = None;
        }
        return false;
    };
    // (a sequence of the line's under way — the `j` of `[keys.insert] j = { k = … }`, `C-r` waiting: a key that goes
    // on with it is the line's; else what it typed goes in first, so `enter` searches with it and `backspace` sees
    // the line as it is)
    if crate::prompt::under_way(ed, crate::docpane::Which::Prompt, key) {
        ed.handle_prompt_key(key);
        prompt_typed(ed);
        return true;
    }
    let empty = ed.prompt.as_ref().is_some_and(|p| p.input.is_empty());
    let cancel = key.code == Code::Esc
        || (key.ctrl && key.code == Code::Char('c'))
        || (key.code == Code::Backspace && empty);
    if cancel {
        ed.prompt = None;
        if let Some(p) = ed.float_list_mut(kind) {
            (p.prompt, p.selected) = (None, from);
        }
        return true;
    }
    if crate::editor::prompt_submits(key) {
        let text = ed.prompt.take().map(|p| p.text()).unwrap_or_default();
        ed.prompt_history.remember(PromptKind::Search { reverse }, &text);
        let last = ed.float_list_mut(kind).and_then(|p| {
            p.prompt = None;
            p.search.clone()
        });
        let pat = if text.is_empty() { last.unwrap_or_default() } else { text };
        if pat.is_empty() {
            return true;
        }
        ed.float_list_search(kind, &pat, reverse, from, false);
        return true;
    }
    ed.handle_prompt_key(key);
    prompt_typed(ed);
    true
}

/// A list's own `/`·`?` after its line changed (a key, a paste): the selection on the next match as it goes, back on
/// the row it opened on while the line is empty. Nothing when the prompt isn't a list's.
pub fn prompt_typed(ed: &mut Editor) {
    let Some((kind, from)) =
        KINDS.into_iter().find_map(|k| ed.float_list(k).and_then(|p| p.prompt).map(|from| (k, from)))
    else {
        return;
    };
    let Some((crate::editor::PromptKind::Search { reverse }, typed)) =
        ed.prompt.as_ref().map(|p| (p.kind, p.text()))
    else {
        return;
    };
    match typed.is_empty() {
        true => {
            if let Some(p) = ed.float_list_mut(kind) {
                p.selected = from;
            }
        }
        false => ed.float_list_search(kind, &typed, reverse, from, true),
    }
}

/// The wheel over a list, its keys or not: its view three rows and nothing else — the selection stays, in sight or not
/// (the user, 2026-10-28: the wheel never moves a list's selection); the next key that moves it brings the view back
/// to it (`top_drawn`).
pub fn wheel(ed: &mut Editor, kind: GroupKind, up: bool) {
    let n = ed.float_list_ids(kind).len();
    let rows = rows_in_sight(ed, kind).max(1);
    if let Some(p) = ed.float_list(kind) {
        let top = p.top.get().saturating_add_signed(if up { -3 } else { 3 });
        wheel_to(&p.top, &p.shown, top.min(n.saturating_sub(rows)));
    }
}

/// The first row in sight this frame of a list whose rows are `ids`, `rows` of them in sight, its selection on row
/// `at`: on the same first row as rows come and go (a message said, one dropped from the front), and where the wheel
/// left it while the selection (by its id) and the rows in sight stay; else — no wheel since a key, the selection
/// moved, an ask come over the list's foot — the selection kept in sight from there.
pub fn top_drawn(p: Option<&FloatList>, ids: &[u64], at: usize, rows: usize) -> usize {
    match p {
        Some(p) => hold(&p.top, &p.shown, ids, at, rows, false),
        None => hold(&Default::default(), &Default::default(), ids, at, rows, false),
    }
}

/// `top_drawn` for any list whose view is `top` and was last drawn as `shown` — kept in them for the next frame. With
/// `tail` (a run's results as they come) it follows the newest rows while its view is at its end and the selection
/// stays (the wheel back at the end: followed again).
pub fn hold(
    top: &std::cell::Cell<usize>,
    shown: &std::cell::Cell<Option<Shown>>,
    ids: &[u64],
    at: usize,
    rows: usize,
    tail: bool,
) -> usize {
    let (n, end) = (ids.len(), ids.len().saturating_sub(rows.max(1)));
    let (cell, was) = (top, shown.get());
    let mut top = cell.get();
    let selected = ids.get(at).copied();
    let same = was.filter(|w| (w.selected, w.rows) == (selected, rows));
    let at_end = was.is_none_or(|w| w.selected == selected && top + w.rows >= w.n);
    // (the wheel's view stands until a key, the selection or the rows in sight change — a tail's back at its end
    // follows again)
    let wheeled = same.is_some_and(|w| w.wheeled) && !(tail && at_end);
    // (on the row that was first as rows come and go, unless the wheel moved it since)
    if let Some(w) = same
        && top == w.top
        && let Some(f) = w.first.and_then(|f| ids.iter().position(|&i| i == f))
    {
        top = f;
    }
    let top = match () {
        _ if tail && at_end => end,
        _ if wheeled => top.min(end),
        _ => top.clamp((at + 1).saturating_sub(rows.max(1)), at).min(end),
    };
    cell.set(top);
    shown.set(Some(Shown { selected, rows, top, first: ids.get(top).copied(), n, wheeled }));
    top
}

/// The list's first row in sight, as last drawn.
fn top_was(ed: &Editor, kind: GroupKind) -> usize {
    ed.float_list(kind).map_or(0, |p| p.top.get())
}

/// The list's rows in sight, as last drawn (none before).
fn rows_in_sight(ed: &Editor, kind: GroupKind) -> usize {
    ed.screen.list(kind).map_or(0, |l| l.1)
}

/// A key while a floating list has the keys — false when none has them.
pub fn key(ed: &mut Editor, key: Key) -> bool {
    if ed.mode != crate::editor::Mode::Normal || !ed.pending.is_empty() || ed.prompt.is_some() {
        return false;
    }
    let Some(kind) = ed.float_list_in_hand() else { return false };
    let ids = ed.float_list_ids(kind);
    let here = ed.float_list_here(kind).and_then(|id| ids.iter().position(|&x| x == id)).unwrap_or(0);
    let top = top_was(ed, kind);
    let rows = rows_in_sight(ed, kind);
    // (a key of the list's — not the editor's, nor `esc` and `q` — brings the view back to the selection, wherever the
    // wheel left it: one that moves it, one at its end, one that acts on it)
    let back = |ed: &Editor| ed.float_list(kind).map(|p| unwheel(&p.shown));
    if let Some(m) = crate::key::list_move(key, rows) {
        back(ed);
        let (selected, top) = match m {
            ListMove::Scroll(by) => {
                let (top, at) = crate::key::in_sight(by, top, here, ids.len(), rows, 0);
                (at, top)
            }
            m => (m.from(here, ids.len()), top),
        };
        // (at its end, or one row: said — no silent key)
        if (selected, top) == (here, top_was(ed, kind)) {
            ed.key_unused(key, what(kind), "it's at its end");
        }
        ed.float_list_select(kind, selected, top);
        return true;
    }
    match (key.code, key.ctrl, key.alt) {
        // (the editor's: the space menu, the command line, the windows — `S-tab` the next pane)
        (Code::Char(' ' | ':') | Code::BackTab, false, false) | (Code::Char('w'), true, false) => {
            return false;
        }
        // (`esc` as the code's: its search's matches unlit too)
        (Code::Esc, ..) => {
            if let Some(p) = ed.float_list_mut(kind) {
                p.hl = false;
            }
            ed.float_list_leave(kind);
        }
        (Code::Char('q'), false, false) => ed.float_list_close(kind),
        (Code::Enter, false, false) => {
            back(ed);
            ed.float_list_enter(kind)
        }
        // Its own search, as every pane's (user, 2026-10-23: never type-to-narrow — its letters are keys)
        (Code::Char(c @ ('/' | '?')), false, false) => ed.float_list_search_open(kind, c == '?'),
        (Code::Char(c @ ('n' | 'N')), false, false) => {
            back(ed);
            ed.float_list_search_next(kind, c == 'N')
        }
        (Code::Char(c), false, false) => {
            back(ed);
            match kind {
                GroupKind::Messages => ed.messages_key(c),
                GroupKind::Folders => ed.folders_key(c),
                _ => ed.session_list_key(c),
            }
        }
        _ => ed.key_unused(key, what(kind), "esc code"),
    }
    true
}

#[cfg(test)]
mod tests;
