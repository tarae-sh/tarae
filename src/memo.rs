//! The memo (main's `.sb/memo-plan.md`, step (a); the user's go 2026-11-18, main's `.sb/decisions/ux.md` "The memo is
//! built in the core"): one small floating pane a workspace has, holding a document of yours. `space M` (`:memo`)
//! brings it up with the keys, insert mode at the end of its text; inside it's a document like any — tarae's own
//! editing, undo, `/` and `n`/`N` its own search, the mouse's selection — with `esc` normal mode there. Leaving it puts
//! it away: once the keys are elsewhere (`space M` again, `C-w w`, a press on the code) it isn't drawn — floating is
//! for the moment you use it.
//!
//! Its host is a pane of its own out of the tree (`Shows::Memo`, `Here::Memo`), named `GroupKind::Memo` in the float
//! code as the floating lists are (no member of `Workspace.groups`): `C-w m`, `C-w < > - +`, `C-w =` and the mouse on
//! its frame move and size it as every float, the frame kept with the sizes (`Sizes.memo`). First its measure wide — a line
//! of 40 characters whole, the cursor after it (`MEASURE`, `WIDTH`) — and as tall as its rows (three at least), up to
//! half the screen, then it scrolls; centred, its top at a quarter.
//!
//! Its document is a third shape of `docpane` (`Which::Memo`): its edits, its undo history and its mode stay, as the
//! code's do. Its keys run with it swapped in (`docpane::swap`) — `space`, `:` and `C-w` with nothing under way in
//! normal mode go on to the editor (`:w` there writes the code's file, as from a run's pane); in insert mode `C-w`
//! deletes a word while it holds text and goes on as the window prefix only when it's empty, as the chat's input does
//! — the way out of typing is `esc` first, or a press on the code.
//!
//! Its words are yours alone: kept in tarae's state folder (`$state/memos.json`, keyed by the checkout's real path),
//! written on a worker as you leave it, as auto-save saves (the terminal's focus lost, idle) and as tarae ends —
//! never in the project, never in any agent's context (only the screen you send shows it). It saves itself: `:w`
//! has nothing to do with it.

use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::docpane::{After, Pane, Which};
use crate::document::Document;
use crate::editor::{Editor, Mode};
use crate::key::{Code, Key};
use crate::keymap::{Lookup, MappableCommand};
use crate::pane::{Frame, GroupKind};
use crate::selection::Selection;
use crate::split::{PaneId, Rect};
use crate::workspace::Here;

/// Its text's measure: a line of this many characters stays whole, wrapped past it (the design session's word on the
/// plan's 5: a measure, not a pane's cells).
pub const MEASURE: usize = 40;

/// Its width before you size it: the measure, the cell after it the cursor takes at a full line's end, and its sides'
/// line and air each side — 45 (one camera zoom still holds it whole on a Short).
pub const WIDTH: usize = MEASURE + 1 + 2 * PAD;

/// Its text's rows at least, before it grows with its text.
pub const ROWS_MIN: usize = 3;

/// Its text's cells in from each side: the side's line and a blank.
pub const PAD: usize = 2;

/// The memo's document's id (`document`'s inputs' kind: no buffer's, swapped in for its keys one at a time).
pub const DOC: crate::document::DocId = crate::document::PICKER_INPUT - 1;

/// A workspace's memo, once it has been up (or kept from before).
pub struct Memo {
    /// Its document, its keys' state (its mode, its search), its search's matches.
    pub pane: Pane,
    /// Its pane's id (`Shows::Memo`, out of the tree; the state's `pane#N`).
    pub id: PaneId,
    /// Up: drawn, the keys its (put away once they go elsewhere — `Editor::memo_sync`).
    pub up: bool,
    /// Its first row in sight, as last drawn — kept so the cursor's row stays in sight (`sight`).
    pub scroll: std::cell::Cell<usize>,
}

impl Memo {
    /// A memo holding `text`, its pane `id`.
    pub fn new(id: PaneId, text: &str) -> Memo {
        let mut doc = Document::from_str(DOC, text);
        doc.throwaway = true;
        // (prose: its lines wrap at its width, `j`/`k` by rows)
        doc.wrap = Some(true);
        Memo { pane: Pane::new(doc), id, up: false, scroll: Default::default() }
    }

    #[cfg(test)]
    pub fn text(&self) -> String {
        self.pane.doc_in().map(|d| d.text.to_string()).unwrap_or_default()
    }

    /// Its mode (insert while you type, normal after `esc`).
    pub fn mode(&self) -> Mode {
        self.pane.keys.mode
    }
}

/// The cells its text wraps at in a frame `w` wide: what's left of its sides and air — soft wrap keeps the last for the
/// cursor after a full line (`wrap::rows`), so `MEASURE` characters stand whole at `WIDTH`.
pub fn text_w(w: usize) -> usize {
    w.saturating_sub(2 * PAD).max(1)
}

/// Its rows at `width` cells: each line's wrapped rows, every line one at least.
pub fn rows(doc: &Document, width: usize, tab: usize) -> Vec<(usize, Vec<crate::wrap::Row>)> {
    let last = crate::movement::last_line(&doc.text);
    (0..=last).map(|line| (line, crate::wrap::rows(&doc.text, line, width, tab))).collect()
}

/// Its rows counted at `width` cells.
pub fn rows_n(doc: &Document, width: usize, tab: usize) -> usize {
    rows(doc, width, tab).iter().map(|(_, r)| r.len().max(1)).sum()
}

/// The row (counted from its first) the cursor is on, at `width` cells.
pub fn cursor_row(doc: &Document, width: usize, tab: usize, mode: Mode) -> usize {
    let text = &doc.text;
    let pos = crate::term::cursor_idx(mode, doc.selection().primary(), text);
    let line = crate::movement::line_of(text, pos);
    let before: usize = (0..line).map(|l| crate::wrap::rows(text, l, width, tab).len().max(1)).sum();
    before + crate::wrap::row_of(&crate::wrap::rows(text, line, width, tab), pos)
}

/// Its first row in sight in `inner` rows: where it was, moved just enough to hold the cursor's row (`row`), never past
/// its last row at the bottom.
pub fn sight(was: usize, row: usize, inner: usize, n: usize) -> usize {
    let inner = inner.max(1);
    let top = if row < was {
        row
    } else if row >= was + inner {
        row + 1 - inner
    } else {
        was
    };
    top.min(n.saturating_sub(inner))
}

/// Its frame before you move or size it: centred across, its top by the rule, `WIDTH` wide (`w` 0), as tall as its
/// rows (`h` 0).
pub const FRAME: Frame = Frame { x: Frame::PLACED, y: Frame::PLACED, w: 0, h: 0 };

/// Its cells on a screen `sw` × `sh` (the rows above the status line), its frame `f`, `n_at(w)` its rows at `w`
/// cells: as wide as `f` says (`WIDTH` unset), at least `WIDTH` where there's room; as tall as its rows (three at
/// least, up to half the screen) unless you set a height; centred across and its top at a quarter unless you moved it.
pub fn rect(f: Frame, sw: usize, sh: usize, n_at: impl Fn(usize) -> usize) -> Rect {
    let (sw, sh) = (sw.max(1), sh.max(1));
    let of = |v: u16, n: usize| (v as usize * n + 500) / 1000;
    let w = match f.w {
        0 => WIDTH,
        w => of(w, sw),
    }
    .clamp(WIDTH.min(sw), sw);
    let least = (ROWS_MIN + 2).min(sh);
    let h = match f.h {
        0 => (n_at(text_w(w)).max(ROWS_MIN) + 2).min((sh / 2).max(least)),
        h => of(h, sh).max(least),
    }
    .min(sh);
    let x = match f.x {
        Frame::PLACED => (sw - w) / 2,
        x => of(x, sw).min(sw - w),
    };
    let y = match f.y {
        Frame::PLACED => Frame::placed_y(h, sh),
        y => of(y, sh),
    }
    .min(sh - h);
    Rect { x, y, w, h }
}

/// Its frame once moved or sized to `r` from `was` (drawn at `was_r`): what didn't change stays by the rule — a width
/// unset, a height as its rows', a left centred, a top at a quarter.
pub fn frame_of(was: Frame, was_r: Rect, r: Rect, sw: usize, sh: usize) -> Frame {
    let f = Frame::of_rect(r, sw, sh);
    let same_w = r.w == was_r.w;
    Frame {
        x: if was.x == Frame::PLACED && r.x == was_r.x && same_w { Frame::PLACED } else { f.x },
        y: if was.y == Frame::PLACED && r.y == was_r.y { Frame::PLACED } else { f.y },
        w: if was.w == 0 && same_w { 0 } else { f.w.max(1) },
        h: if was.h == 0 && r.h == was_r.h { 0 } else { f.h.max(1) },
    }
}

/// The file its words are kept in.
pub fn path() -> Option<PathBuf> {
    crate::config::state_dir().map(|d| d.join("memos.json"))
}

/// The memos kept (a small file, read once before the first frame, as `layouts.json`).
pub fn read(path: Option<&std::path::Path>) -> Map<String, Value> {
    crate::layouts::read(path)
}

/// The writes' numbers, newer than every one before (`layouts::save` keeps the newest asked for).
static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The commands a key in the memo runs on the editor, its document put back first: the window table's, the palette,
/// the inbox, a `:` line bound to a key — they act on the session, never on its words.
const EDITORS: &[&str] = &["command_palette", "inbox", "memo"];

fn editors(ed: &Editor, c: &MappableCommand) -> bool {
    match c {
        MappableCommand::Static(s) => EDITORS.contains(&s.name) || ed.keymaps.in_window_table(s.name),
        MappableCommand::Typed(_) => true,
    }
}

/// The keys the memo with nothing under way lets go on down the chain (`keys_to`'s layer): in normal mode `C-w`,
/// `space` and `:`; in insert mode `C-w` only while it's `empty` — with text it deletes a word, as in the chat's input
/// (`chat::key`) and the code's insert mode, so hands that know it there never put the memo away mid-sentence.
pub fn passed(mode: Mode, empty: bool) -> Vec<Key> {
    let ctrl = Key { code: Code::Char('w'), ctrl: true, alt: false, shift: false };
    let window = vec![ctrl, Key { alt: true, ..ctrl }];
    match mode {
        Mode::Insert if empty => window,
        Mode::Insert => Vec::new(),
        _ => [window, vec![Key::plain(Code::Char(' ')), Key::plain(Code::Char(':'))]].concat(),
    }
}

impl Memo {
    /// Nothing typed in it.
    pub fn is_empty(&self) -> bool {
        self.pane.doc_in().is_none_or(|d| d.text.len_bytes() == 0)
    }
}

/// The keys at its bottom line, as its mode has them (a key that does nothing there is never shown): typing, `esc`
/// and the way to the code; in normal mode, `space M` puts it away.
pub fn hints(ed: &Editor) -> Vec<(String, &'static str)> {
    let Some(m) = ed.workspace.memo.as_ref() else { return Vec::new() };
    match m.mode() {
        // (typing: `esc` first is the way out — `C-w` deletes a word here)
        Mode::Insert => vec![("esc".into(), "normal")],
        _ => {
            let key = ed.keymaps.bindings().get("memo").cloned();
            let mut v: Vec<(String, &'static str)> = key.map(|k| (k, "put away")).into_iter().collect();
            // (`code`: the keys to the code, as `esc code` says it everywhere — `to the code` folded at 45 wide, under the
            // run of line every frame keeps before its keys)
            v.push(("C-w w".into(), "code"));
            v
        }
    }
}

/// The memo's keys' state while it has the keys — its sequence under way, as which-key and the status line show it.
pub fn keyed(ed: &Editor) -> Option<&crate::docpane::Keys> {
    ed.workspace.memo.as_ref().filter(|_| ed.memo_has_keys()).map(|m| &m.pane.keys)
}

/// A sequence of the memo's own under way (`g`, `m`, `z` in its normal mode), the editor's none.
pub fn under_way(ed: &Editor) -> Option<&crate::docpane::Keys> {
    keyed(ed).filter(|k| ed.pending.is_empty() && !k.pending.is_empty())
}

impl Editor {
    /// The memo is up with the keys.
    pub fn memo_has_keys(&self) -> bool {
        self.workspace.memo.as_ref().is_some_and(|m| m.up) && self.workspace.is_at(Here::Memo)
    }

    /// The memo is drawn: up (the keys its, or a picker or a prompt it opened over it).
    pub fn memo_up(&self) -> bool {
        self.workspace.memo.as_ref().is_some_and(|m| m.up)
    }

    /// `space M`, `:memo`: the memo up with the keys, insert mode at the end of its text — made the first time, with
    /// what was kept for this checkout; up already, put away.
    pub fn memo_toggle(&mut self) {
        if self.memo_up() && self.workspace.is_at(Here::Memo) {
            return self.memo_away();
        }
        self.memo_bring();
    }

    /// The memo up with the keys, insert mode at the end of its text.
    pub fn memo_bring(&mut self) {
        if self.workspace.memo.is_none() {
            let id = self.next_pane;
            self.next_pane += 1;
            let kept = self.memos.get(&self.memo_key()).and_then(|v| v["text"].as_str()).unwrap_or_default();
            self.workspace.memo = Some(Memo::new(id, kept));
        }
        let Some(m) = self.workspace.memo.as_mut() else { return };
        let id = m.id;
        if !m.up {
            m.up = true;
            m.pane.dropped();
            m.pane.keys.mode = Mode::Insert;
            let d = m.pane.doc_mut();
            let end = d.text.len_bytes();
            d.set_selection(Selection::point(end));
        }
        if !self.workspace.panes.iter().any(|p| p.id == id) {
            self.workspace.panes.push(crate::pane::Pane { id, shows: crate::pane::Shows::Memo });
        }
        self.card_focus = None;
        self.plugin_panel_unfocus();
        self.procs_unfocus();
        self.workspace.set_at(Here::Memo);
    }

    /// The memo put away: not drawn, the keys to the code — its words kept.
    pub fn memo_away(&mut self) {
        self.workspace.leave(Here::Memo);
        self.memo_sync();
    }

    /// Every key and event (`focus_sync`): once the keys are elsewhere, the memo is put away — not drawn, a prompt or
    /// search it opened closed, its words kept.
    pub fn memo_sync(&mut self) {
        if !self.memo_up() || self.workspace.is_at(Here::Memo) {
            return;
        }
        let Some(m) = self.workspace.memo.as_mut() else { return };
        m.up = false;
        if self.float_moving == Some(GroupKind::Memo) {
            self.float_moving = None;
        }
        if let Some(m) = self.workspace.memo.as_mut()
            && m.pane.dropped()
        {
            self.prompt = None;
        }
        self.memo_keep();
    }

    /// The memo's words changed since they were last kept (auto-save's idle wait starts for them too).
    pub fn memo_unkept(&self) -> bool {
        let Some(d) = self.workspace.memo.as_ref().and_then(|m| m.pane.doc_in()) else { return false };
        let kept = self.memos.get(&self.memo_key()).and_then(|v| v["text"].as_str()).unwrap_or_default();
        d.text != kept
    }

    /// Its key in `memos.json`: the checkout's real path (`Workspace.root`, links resolved as it was read), as
    /// `layouts.json`'s.
    fn memo_key(&self) -> String {
        self.workspace.root.to_string_lossy().into_owned()
    }

    /// The memo's words kept once they changed — a worker's write (`layouts::save`: the newest asked for wins, under
    /// a lock beside the file); nothing back to the loop.
    pub fn memo_keep(&mut self) {
        let Some(text) =
            self.workspace.memo.as_ref().and_then(|m| m.pane.doc_in()).map(|d| d.text.to_string())
        else {
            return;
        };
        let key = self.memo_key();
        if self.memos.get(&key).and_then(|v| v["text"].as_str()) == Some(text.as_str()) {
            return;
        }
        // (nothing typed in one never kept: nothing to write)
        if text.is_empty() && !self.memos.contains_key(&key) {
            return;
        }
        let v = json!({ "text": text });
        self.memos.insert(key.clone(), v.clone());
        let Some(path) = self.memos_path.clone() else { return };
        let seq = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        self.events.jobs().background(move || crate::layouts::save(&path, &key, v, seq));
    }

    /// The memo's rows and columns now (its frame on this screen), as its keys move by them.
    fn memo_size(&self) -> (usize, usize) {
        let r = self.float_rect(GroupKind::Memo).unwrap_or(Rect { x: 0, y: 0, w: WIDTH, h: ROWS_MIN + 2 });
        (r.h.saturating_sub(2).max(1), text_w(r.w))
    }

    /// The memo's cells on this screen while it's up (`float_rect`).
    pub fn memo_rect(&self) -> Option<Rect> {
        let m = self.workspace.memo.as_ref().filter(|m| m.up)?;
        let doc = m.pane.doc_in()?;
        let (sw, sh) = self.float_screen();
        let tab = self.config.tab_width;
        Some(rect(self.sizes.memo.unwrap_or(FRAME), sw, sh, |w| rows_n(doc, w, tab)))
    }

    /// Its frame set to the cells `r` (a move or a size, by keys or the mouse).
    pub(crate) fn memo_set(&mut self, r: Rect) {
        let Some(was_r) = self.memo_rect() else { return };
        let (sw, sh) = self.float_screen();
        self.sizes.memo = Some(frame_of(self.sizes.memo.unwrap_or(FRAME), was_r, r, sw, sh));
    }

    /// Its frame as it first stood (`C-w =`).
    pub(crate) fn memo_auto(&mut self) {
        self.sizes.memo = None;
    }
}

/// Before drawing: the matches its search shows — the pattern its `/` prompt is typing, else its last search while
/// shown (`n`/`N` are its own, as in every pane).
pub fn refresh_search(ed: &mut Editor) {
    let typed = ed.prompt.as_ref().filter(|p| matches!(p.kind, crate::editor::PromptKind::Search { .. }));
    let typed = typed.map(|p| p.text());
    if !ed.memo_up() {
        return;
    }
    if let Some(m) = ed.workspace.memo.as_mut() {
        m.pane.refresh_search(typed);
    }
}

/// `esc` in normal mode with its keys: its matches no longer shown (the code's stay as they are).
pub fn search_unshown(ed: &mut Editor) {
    if let Some(m) = ed.workspace.memo.as_mut().filter(|m| m.mode() != Mode::Insert) {
        m.pane.keys.search_hl = false;
    }
}

/// Runs `f` with the memo's document swapped in (`docpane::swap`): its edits, history and mode kept.
fn swap<R>(ed: &mut Editor, f: impl FnOnce(&mut Editor) -> R) -> Option<R> {
    let size = ed.memo_size();
    crate::docpane::swap(ed, Which::Memo, size, f).map(|r| r.0)
}

/// A key while the memo has them: its prompt's (`/`), else its mode's on its document — what `passed` names goes on
/// with nothing under way, as a sequence of the editor's under way does. Returns the handler
/// that took it.
pub fn key(ed: &mut Editor, key: Key) -> Option<crate::uistate::Handler> {
    use crate::uistate::Handler;
    if !ed.memo_has_keys() || !ed.pending.is_empty() {
        return None;
    }
    let m = ed.workspace.memo.as_ref()?;
    if ed.prompt.is_some() {
        if !m.pane.prompt {
            return None;
        }
        swap(ed, |ed| ed.handle_prompt_key(key));
        return Some(Handler::Prompt);
    }
    if m.pane.keys.idle() && passed(m.mode(), m.is_empty()).contains(&key) {
        return None;
    }
    swap(ed, |ed| match ed.mode {
        Mode::Insert if ed.on_next_char.is_none() => typed(ed, key),
        _ => crate::docpane::key_in(ed, key, |ed, mode, keys| ed.keymaps.lookup(mode, keys), editors),
    });
    after(ed);
    Some(Handler::Memo)
}

/// `key` in insert mode with the memo swapped in: `[insert]`'s command, else typed — a sequence broken off (`j x`
/// under `j k`) is text, as in the code; a last key that isn't a character, said.
fn typed(ed: &mut Editor, key: Key) {
    ed.pending.push(key);
    let cmds = match ed.keymaps.lookup(Mode::Insert, &ed.pending) {
        Lookup::Pending => return,
        Lookup::Matched(cmds) => Some(cmds.to_vec()),
        Lookup::NotFound => None,
    };
    let keys = std::mem::take(&mut ed.pending);
    match cmds {
        Some(cmds) if cmds.iter().all(|c| editors(ed, c)) => {
            if let Some(p) = ed.workspace.memo.as_mut().map(|m| &mut m.pane) {
                p.deferred(cmds);
            }
            crate::docpane::ask(ed, After::Editor);
        }
        Some(cmds) => ed.run(&cmds, None),
        None => {
            let chars: Vec<char> = keys.iter().filter_map(Key::plain_char).collect();
            if !chars.is_empty() {
                ed.with_group(|cx| chars.into_iter().for_each(|c| crate::commands::insert_char(cx, c)));
            }
            if let Some(&last) = keys.last().filter(|k| k.plain_char().is_none()) {
                ed.key_unused(last, "the memo", "esc normal");
            }
        }
    }
}

/// What a command asked for, once its key is done.
fn after(ed: &mut Editor) {
    let Some((a, deferred)) = ed.workspace.memo.as_mut().and_then(|m| m.pane.take_after()) else { return };
    match a {
        // (`esc` in normal mode with nothing selected: it stays, normal mode — `space M` puts it away)
        After::Leave => {}
        After::Editor => ed.run(&deferred, None),
        _ => {}
    }
}

/// A press on the memo's text at its row `row` (from its first in sight) and cell `x` of its text: the keys its, the
/// cursor on that character — where it landed (a drag from there selects).
pub fn click(ed: &mut Editor, row: usize, x: usize) -> Option<usize> {
    if !ed.memo_has_keys() {
        ed.memo_bring();
    }
    let pos = pos_at(ed, row, x)?;
    let d = ed.workspace.memo.as_mut()?.pane.doc_in_mut()?;
    d.set_selection(Selection::point(pos));
    Some(pos)
}

/// The character the memo draws on its row `row` (from its first in sight) at cell `x` of its text.
pub fn pos_at(ed: &Editor, row: usize, x: usize) -> Option<usize> {
    let (_, cols) = ed.memo_size();
    let tab = ed.config.tab_width;
    let m = ed.workspace.memo.as_ref()?;
    let d = m.pane.doc_in()?;
    let mut k = m.scroll.get() + row;
    let all = rows(d, cols, tab);
    for (line, rs) in &all {
        if k < rs.len() {
            return Some(crate::wrap::pos_in_row(&d.text, *line, rs, k, x, tab));
        }
        k -= rs.len();
    }
    Some(d.text.len_bytes())
}

#[cfg(test)]
mod tests;
