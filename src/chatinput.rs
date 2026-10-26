//! The chat's input as a buffer (panes P2c, `.sb/panes-plan.md` §2.1e): a small document in insert mode, the
//! one place in the chat that takes typing. Its keys are the editor's insert mode's (`[insert]`, yours too) with the
//! chat's `[chat]` over them (`esc` → its view mode, `C-a`/`C-e` a line's ends, `A-ret` a new line), run with the
//! document swapped in (`docpane::swap`, `Which::Input`) — what isn't bound is typed, without auto-pairs (it's
//! prose). The chat's own keys come first (`chat::key`): `enter` sends, its chords, the page keys, `↑`/`↓` and `tab`
//! from an empty input, backspace taking a chip off — as they were before it was a buffer.

use crate::docpane::{After, Which};
use crate::editor::Editor;
use crate::key::{Code, Key};
use crate::keymap::{KeyTrie, Lookup, MappableCommand};
use crate::selection::{Range, Selection};

/// The chat's input is an `Input` (`input.rs`, the prompt's line one too).
pub use crate::input::Input;

/// The input's rows at most (more scroll), as the chat draws it.
pub const ROWS: usize = 5;

/// A press on the input's row `row` (`Screen::chat_input_row`) at column `col` of its text, `w` the width it was
/// wrapped to: the keys to it (the conversation's view mode left, as `i` leaves it), the cursor on the character
/// pressed, as a press in the code's insert mode puts it. Where it landed — a drag from there selects (`drag`).
pub fn click(ed: &mut Editor, row: usize, col: usize, w: usize) -> usize {
    crate::chatdoc::leave(ed);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return 0 };
    let pos = c.input.at(row, col, w);
    c.input.pane.doc_mut().set_selection(Selection::point(pos));
    pos
}

/// A drag from the press's character `anchor` (`click`'s) to row `row`, column `col` under the pointer: what's
/// between selected, both included, as a drag in the code selects — insert mode kept, as there. The keys gone from
/// it since: nothing.
pub fn drag(ed: &mut Editor, anchor: usize, row: usize, col: usize, w: usize) {
    if !ed.workspace.is_at(crate::workspace::At::Chat) || crate::chatdoc::keyed(ed) {
        return;
    }
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return };
    let pos = c.input.at(row, col, w);
    let d = c.input.pane.doc_mut();
    let anchor = crate::graphemes::snap(&d.text, anchor);
    d.set_selection(Selection::new(vec![Range::dragged(&d.text, anchor, pos)], 0));
}

/// A key for the input's document (`chat::key`, once the chat's own keys didn't take it): `[chat]` then
/// `[insert]`, run with it swapped in; what neither binds, typed — a `C-`/`A-` key nothing binds is said. Always
/// taken.
pub fn key(ed: &mut Editor, key: Key) -> bool {
    let cols = ed.screen.chat_view.get().map_or(60, |a| a.1).saturating_sub(2).max(1);
    crate::docpane::swap(ed, Which::Input, (ROWS, cols), |ed| typed(ed, key));
    let Some((a, deferred)) = ed.workspace.claude.chat.as_mut().and_then(|c| c.input.pane.take_after())
    else {
        return true;
    };
    if a == After::Editor {
        ed.run(&deferred, None);
    }
    true
}

/// A sequence of the input's under way (the `j` of `[keys.insert] j = { k = … }`): whether `key` goes on with it —
/// then it's the input's (`key`). Else what was typed of it is text, put in first, as a sequence broken off is in
/// the code: the chat's own key after it (`enter` sends) takes the input with it, never leaves it for the next.
pub fn under_way(ed: &mut Editor, key: Key) -> bool {
    let Some(pending) =
        ed.workspace.claude.chat.as_ref().map(|c| &c.input.pane.keys.pending).filter(|p| !p.is_empty())
    else {
        return false;
    };
    let mut keys = pending.clone();
    keys.push(key);
    if !matches!(lookup(ed, &keys), Lookup::NotFound) {
        return true;
    }
    let Some(c) = ed.workspace.claude.chat.as_mut() else { return false };
    // (what it typed so far goes in first: `insert_str` puts it before what it's given — here nothing)
    c.input.insert_str("");
    false
}

/// Where `esc` goes from the input, as its hint and its notices say it: normal mode on the conversation (`esc
/// normal`, its key as bound) — nothing to read yet, the code.
pub fn esc_said(ed: &Editor) -> (String, &'static str) {
    let to_view = ed.keymaps.chat.keys_for("chat_view").into_iter().next();
    match to_view {
        Some(k) if ed.workspace.claude.chat.as_ref().is_some_and(|c| !c.msgs.is_empty()) => (k, "normal"),
        // (`esc code` where the surface stays, `esc close` where it goes — the user, 2026-10-20)
        _ => ("esc".into(), "code"),
    }
}

/// `key` with the input swapped in.
fn typed(ed: &mut Editor, key: Key) {
    ed.pending.push(key);
    let cmds = match lookup(ed, &ed.pending) {
        Lookup::Pending => return,
        Lookup::Matched(cmds) => Some(cmds.to_vec()),
        Lookup::NotFound => None,
    };
    let keys = std::mem::take(&mut ed.pending);
    match cmds {
        // (the editor's own — `esc` to its view mode, the window table's, a `:` line — once it's put back)
        Some(cmds) if cmds.iter().all(|c| editors(ed, c)) => {
            if let Some(p) = ed.workspace.claude.chat.as_mut().map(|c| &mut c.input.pane) {
                p.deferred(cmds);
            }
            crate::docpane::ask(ed, After::Editor);
        }
        Some(cmds) => ed.run(&cmds, None),
        // (a sequence broken off — `j x` under `j k` — is text, as in the code; a last key that isn't a
        // character, said)
        None => {
            let chars: Vec<char> = keys.iter().filter_map(Key::plain_char).collect();
            if !chars.is_empty() {
                ed.with_group(|cx| chars.into_iter().for_each(|c| crate::commands::insert_char(cx, c)));
            }
            if let Some(&last) =
                keys.last().filter(|k| k.plain_char().is_none() && matches!(k.code, Code::Char(_)))
            {
                let (k, to) = esc_said(ed);
                ed.key_unused(last, "the chat", &format!("{k} {to}"));
            }
        }
    }
}

/// The keys under way in `[chat]`, else in `[insert]`.
fn lookup<'a>(ed: &'a Editor, keys: &[Key]) -> Lookup<'a> {
    for table in [&ed.keymaps.chat, &ed.keymaps.insert] {
        match table.search(keys) {
            Some(KeyTrie::Leaf(cmds)) => return Lookup::Matched(cmds),
            Some(KeyTrie::Node(_)) => return Lookup::Pending,
            None => {}
        }
    }
    Lookup::NotFound
}

/// A command a key of the input runs on the editor, the input put back first: its view mode, the window table's,
/// `view::EDITORS`, a `:` line bound to a key.
fn editors(ed: &Editor, c: &MappableCommand) -> bool {
    match c {
        MappableCommand::Static(s) => {
            s.name == "chat_view"
                || crate::view::EDITORS.contains(&s.name)
                || ed.keymaps.in_window_table(s.name)
        }
        MappableCommand::Typed(_) => true,
    }
}

#[cfg(test)]
mod tests;
