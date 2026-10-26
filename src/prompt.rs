//! The prompt's line (#153, `.sb/prompt-plan.md`): an `Input` (`input.rs`) — a small document in insert mode, as the
//! chat's input is — so its cursor goes anywhere in what's typed and Helix's prompt keys edit there. The prompt's own
//! keys come first (`Editor::handle_prompt_key`: `enter`, `esc`, `:`'s `tab` and its ghost); then `[prompt]` over
//! `[insert]` (`keymap.rs`, yours too), run with the line swapped in (`docpane::swap`, `Which::Prompt`) — only what
//! acts on one line (`LINE`); what neither binds is typed. Its history (`History`): what was run at `:`, and at
//! `/` `?`, kept since tarae started — never on disk (a `:sh` line can hold a secret).

use crate::docpane::Which;
use crate::editor::{Editor, Prompt, PromptKind};
use crate::key::{Code, Key};
use crate::keymap::{KeyTrie, Lookup, MappableCommand};
use crate::selection::Selection;
use crate::transaction::{Change, Transaction};
use std::collections::VecDeque;

/// Lines a history keeps, the oldest let go first.
pub const HISTORY_MAX: usize = 100;

/// The commands the line's keys run: its moves and edits — never a new line, a tab, a completion or anything that
/// acts on more than the line (`[insert]` binds those too; they're said instead).
const LINE: &[&str] = &[
    "input_word_left",
    "input_word_right",
    "move_char_left",
    "move_char_right",
    "move_next_word_start",
    "move_prev_word_start",
    "move_next_word_end",
    "goto_line_start",
    "goto_line_end_newline",
    "goto_line_end",
    "delete_char_backward",
    "delete_char_forward",
    "delete_word_backward",
    "delete_word_forward",
    "kill_to_line_start",
    "kill_to_line_end",
    "insert_register",
    "prompt_history_older",
    "prompt_history_newer",
];

/// Of `LINE`, the moves: a cursor moved, nothing selected (a word move selects the word in the code's insert mode).
const MOVES: &[&str] = &[
    "input_word_left",
    "input_word_right",
    "move_char_left",
    "move_char_right",
    "move_next_word_start",
    "move_prev_word_start",
    "move_next_word_end",
    "goto_line_start",
    "goto_line_end_newline",
    "goto_line_end",
];

/// Of `LINE`, those that take text out.
const DELETES: &[&str] = &[
    "delete_char_backward",
    "delete_char_forward",
    "delete_word_backward",
    "delete_word_forward",
    "kill_to_line_start",
    "kill_to_line_end",
];

/// What was run at a prompt, by kind: `:` one list, `/` and `?` another (the others keep none) — newest last, at
/// most `HISTORY_MAX` each, in memory only.
#[derive(Default, Debug)]
pub struct History {
    command: VecDeque<String>,
    search: VecDeque<String>,
}

impl History {
    /// `kind`'s list, if it keeps one.
    pub fn of(&self, kind: PromptKind) -> Option<&VecDeque<String>> {
        match kind {
            PromptKind::Command => Some(&self.command),
            PromptKind::Search { .. } => Some(&self.search),
            _ => None,
        }
    }

    /// `text` run at a `kind` prompt: the newest of its list — not an empty line, nor the newest again.
    pub fn remember(&mut self, kind: PromptKind, text: &str) {
        let list = match kind {
            PromptKind::Command => &mut self.command,
            PromptKind::Search { .. } => &mut self.search,
            _ => return,
        };
        if text.trim().is_empty() || list.back().is_some_and(|l| l == text) {
            return;
        }
        if list.len() == HISTORY_MAX {
            list.pop_front();
        }
        list.push_back(text.to_string());
    }
}

impl Prompt {
    /// A prompt of `kind` holding `text`, the cursor at its end.
    pub fn new(kind: PromptKind, text: &str) -> Prompt {
        let mut input = crate::input::Input::new(crate::document::PROMPT_INPUT);
        input.set(text);
        Prompt { kind, label: None, input, cycle: None, recalled: None, left: Default::default() }
    }

    /// What's typed.
    pub fn text(&self) -> String {
        self.input.text()
    }

    /// Where the cursor is (a byte of `text`).
    pub fn cursor(&self) -> usize {
        self.input.cursor()
    }

    /// The cursor after the last character.
    pub fn at_end(&self) -> bool {
        self.cursor() == self.input.pane.doc().text.len_bytes()
    }

    /// What's typed before the cursor and after it.
    pub fn split(&self) -> (String, String) {
        let (text, at) = (self.text(), self.cursor());
        (text[..at].to_string(), text[at..].to_string())
    }

    /// `text` in its place, the cursor at `cursor`.
    pub fn set_at(&mut self, text: &str, cursor: usize) {
        self.input.set_at(text, cursor);
    }

    /// `text` in its place, the cursor at its end.
    pub fn set(&mut self, text: &str) {
        self.input.set(text);
    }
}

/// A key for the line, once the prompt's own keys didn't take it (`handle_prompt_key`): `[prompt]` then `[insert]`,
/// run with it swapped in; what neither binds, typed. What it changed ends a `tab` cycle and a walk in the history.
pub fn key(ed: &mut Editor, key: Key) {
    let Some(p) = ed.prompt.as_ref() else { return };
    let (before, recalled) = (p.text(), p.recalled.clone());
    let cols = ed.screen.prompt_input.get().map_or(60, |i| i.w).max(1);
    crate::docpane::swap(ed, Which::Prompt, (1, cols), |ed| typed(ed, key));
    let Some(p) = ed.prompt.as_mut() else { return };
    let text = p.text();
    // (one line: a register's line breaks are spaces, as a paste's are)
    if text.contains(['\n', '\r']) {
        let at = p.cursor();
        p.set_at(&text.replace(['\n', '\r'], " "), at);
    }
    if p.text() != before {
        p.cycle = None;
        // (a line recalled stays where the walk is — only an edit of yours ends it)
        if p.recalled == recalled {
            p.recalled = None;
        }
    }
}

/// A sequence of the line's under way (the `j` of `[keys.insert] j = { k = … }`) or `C-r` waiting for a register's
/// name: whether `key` is the line's — the wait's whatever it is, the sequence's when it goes on with it. A sequence
/// broken off is text, put in first, and the key is the prompt's own as ever (`enter` runs the whole line, `esc`
/// closes) — as a sequence broken off is in the code.
pub fn under_way(ed: &mut Editor, key: Key) -> bool {
    let Some(keys) = ed.prompt.as_ref().map(|p| &p.input.pane.keys) else { return false };
    if keys.char_next() {
        return true;
    }
    if keys.pending.is_empty() {
        return false;
    }
    let mut seq = keys.pending.clone();
    seq.push(key);
    if !matches!(lookup(ed, &seq), Lookup::NotFound) {
        return true;
    }
    if let Some(p) = ed.prompt.as_mut() {
        p.input.insert_str("");
        (p.cycle, p.recalled) = (None, None);
    }
    false
}

/// A paste into the line: at the cursor, at once — one change, never key by key (no sequence of `[insert]`'s fires
/// in it; what one under way had typed goes in first). `text` is one line already (`handle_paste`).
pub fn paste(ed: &mut Editor, text: &str) {
    let Some(p) = ed.prompt.as_mut() else { return };
    p.input.insert_str(text);
    (p.cycle, p.recalled) = (None, None);
}

/// `key` with the line swapped in.
fn typed(ed: &mut Editor, key: Key) {
    // `C-r`'s register: a character names it; any other key lets the wait go
    if let Some((f, count)) = ed.on_next_char.take() {
        if let Some(ch) = key.plain_char() {
            ed.with_group_count(count, |cx| f(cx, ch));
        }
        return;
    }
    ed.pending.push(key);
    let cmds = match lookup(ed, &ed.pending) {
        Lookup::Pending => return,
        Lookup::Matched(cmds) => Some(cmds.to_vec()),
        Lookup::NotFound => None,
    };
    let keys = std::mem::take(&mut ed.pending);
    let last = keys.last().copied().unwrap_or(key);
    match cmds {
        Some(cmds) if cmds.iter().all(|c| named(c, LINE)) => {
            // The line empty, nothing to delete — said. (The chat's empty input gives `C-w` to the window prefix
            // instead: the chat is a pane among the others. A prompt is modal — it holds every key until `enter` or
            // `esc` — so a window move from under it would leave it standing over another pane.)
            if ed.doc().text.len_bytes() == 0 && cmds.iter().any(|c| named(c, DELETES)) {
                return ed.set_status("nothing to delete — esc close");
            }
            ed.run(&cmds, None);
            if ed.on_next_char.is_some() {
                ed.char_for = keys.iter().map(Key::to_string).collect();
            }
            if cmds.iter().all(|c| named(c, MOVES)) {
                let d = ed.doc_mut();
                let head = d.selection().primary().head;
                d.set_selection(Selection::point(head));
            }
        }
        // (bound in `[insert]`, but not for one line: a new line, a tab, a completion, the editor's)
        Some(_) => said_unused(ed, last),
        // (a sequence broken off is text, as in the code; a last key that isn't a character, said)
        None => {
            let chars: Vec<char> = keys.iter().filter_map(Key::plain_char).collect();
            if !chars.is_empty() {
                ed.with_group(|cx| chars.into_iter().for_each(|c| crate::commands::insert_char(cx, c)));
            }
            if last.plain_char().is_none() && matches!(last.code, Code::Char(_)) {
                said_unused(ed, last);
            }
        }
    }
}

/// `key` does nothing on the line: said where it would act.
fn said_unused(ed: &mut Editor, key: Key) {
    let out = match ed.offers.is_empty() {
        true => "enter runs the line, esc close".to_string(),
        // (the way back is this line's own `esc`: what reaches the card is said from the code)
        false => format!("esc close — then {} reaches the card", ed.c_q_from_code()),
    };
    ed.key_unused(key, "the command line", &out);
}

/// The keys under way in `[prompt]`, else in `[insert]`.
fn lookup<'a>(ed: &'a Editor, keys: &[Key]) -> Lookup<'a> {
    for table in [&ed.keymaps.prompt, &ed.keymaps.insert] {
        match table.search(keys) {
            Some(KeyTrie::Leaf(cmds)) => return Lookup::Matched(cmds),
            Some(KeyTrie::Node(_)) => return Lookup::Pending,
            None => {}
        }
    }
    Lookup::NotFound
}

fn named(c: &MappableCommand, names: &[&str]) -> bool {
    matches!(c, MappableCommand::Static(s) if names.contains(&s.name))
}

/// `prompt_history_older` / `_newer`: the line run before the one in sight at this kind of prompt, or after it (past
/// the newest, what you'd typed) — the cursor at its end. Runs with the line swapped in.
pub fn walk(ed: &mut Editor, older: bool) {
    if crate::docpane::swapped(ed) != Some(Which::Prompt) {
        return ed.set_status("the history walks a prompt's line — : or / first");
    }
    let Some(p) = ed.prompt.as_ref() else { return };
    let kind = p.kind;
    let Some(list) = ed.prompt_history.of(kind) else {
        return ed.set_status("this line keeps no history — : and / do");
    };
    let n = list.len();
    let typed = ed.doc().text.to_string();
    let (line, recalled) = match (p.recalled.clone(), older) {
        (None, true) if n > 0 => (list[n - 1].clone(), Some((0, typed))),
        (Some((i, draft)), true) if i + 1 < n => (list[n - 2 - i].clone(), Some((i + 1, draft))),
        (Some((0, draft)), false) => (draft, None),
        (Some((i, draft)), false) => (list[n - i].clone(), Some((i - 1, draft))),
        (_, true) => {
            let what = if n == 0 { "nothing run here yet" } else { "the oldest kept" };
            return ed.set_status(format!("{what} — {HISTORY_MAX} lines are kept since tarae started"));
        }
        (None, false) => return ed.set_status("the newest — ↑ goes back"),
    };
    let d = ed.doc_mut();
    let tx = Transaction::new(vec![Change { from: 0, to: d.text.len_bytes(), insert: line.clone() }]);
    d.apply(&tx);
    d.set_selection(Selection::point(line.len()));
    if let Some(p) = ed.prompt.as_mut() {
        p.recalled = recalled;
    }
}

/// The line's character drawn at screen column `x` (`Screen.prompt_input`) — past either end of what's in sight, the
/// next one out (a drag there scrolls the line a character at a time, its cursor kept in sight).
fn at_x(ed: &Editor, x: usize) -> Option<usize> {
    let drawn = ed.screen.prompt_input.get()?;
    let p = ed.prompt.as_ref()?;
    let (text, rope) = (p.text(), &p.input.pane.doc().text);
    // (a byte of the last frame's line: an edit since — a paste, a key in the same batch — can leave it inside a
    // character now)
    let first = crate::graphemes::snap(rope, drawn.first);
    if x < drawn.x && first > 0 {
        return Some(crate::graphemes::prev_boundary(rope, first));
    }
    Some(p.input.at_col(&text[first..], first, x.saturating_sub(drawn.x).min(drawn.w)))
}

/// A press on the screen at (`x`, `y`): on the prompt's line, its cursor on the character pressed, as a press in the
/// code's insert mode puts it — where it landed (a drag from there selects, `drag`); elsewhere None.
pub fn click(ed: &mut Editor, x: usize, y: usize) -> Option<usize> {
    ed.screen.prompt_input.get().filter(|d| d.y == y)?;
    let at = at_x(ed, x)?;
    let p = ed.prompt.as_mut()?;
    p.input.pane.doc_mut().set_selection(Selection::point(at));
    p.cycle = None;
    Some(at)
}

/// A drag from the press's character `anchor` (`click`'s) to screen column `x`: what's between selected, both
/// included, as a drag in the code selects — insert mode kept, as there.
pub fn drag(ed: &mut Editor, anchor: usize, x: usize) {
    let Some(at) = at_x(ed, x) else { return };
    let Some(p) = ed.prompt.as_mut() else { return };
    let d = p.input.pane.doc_mut();
    let anchor = crate::graphemes::snap(&d.text, anchor);
    d.set_selection(Selection::new(vec![crate::selection::Range::dragged(&d.text, anchor, at)], 0));
}

#[cfg(test)]
mod tests;
