//! The mouse's rules every surface shares (`.sb/mouse-plan.md`; CLAUDE.md, "The mouse, one way on every surface"): a
//! press gives the keys to the surface it lands on. Before any surface takes a press, what holds the keys over them all
//! lets go: a prompt or a picker open closes as `esc` closes it (a press on it is its own), and a card focused gives
//! its keys back as `esc` does (it stays, waiting) — then the press acts where it landed.

use crate::editor::Editor;

fn inside((x0, y0, w, h): (usize, usize, usize, usize), x: usize, y: usize) -> bool {
    (x0..x0 + w).contains(&x) && (y0..y0 + h).contains(&y)
}

/// A press at (`x`, `y`), before any surface's: true when it's taken here — a prompt's own (a completion row picks that
/// completion, as `tab` cycling to it does), or the prompt or picker closed by a press outside it, nothing else pressed
/// (no click-through). A focused card pressed outside lets go of the keys and the press goes on (false) — said, unless
/// what's pressed says its own (`quiet_spot`).
pub fn press_first(ed: &mut Editor, x: usize, y: usize) -> bool {
    if ed.prompt.is_some() {
        match ed.screen.prompt_card.get() {
            Some(card) if inside(card, x, y) => {
                let row = ed.screen.prompt_rows.borrow().iter().find(|r| r.0 == y).map(|r| r.1);
                if let Some(i) = row {
                    ed.cmdline_pick(i);
                }
                // (its line: the cursor on the character pressed — a drag from there selects)
                if let Some(at) = crate::prompt::click(ed, x, y) {
                    ed.mouse_anchor = Some((crate::editor::Drag::Prompt, at));
                }
            }
            _ => ed.cancel_prompt(),
        }
        return true;
    }
    if ed.picker.is_some() {
        if ed.screen.picker_box.get().is_some_and(|b| inside(b, x, y)) {
            return false;
        }
        ed.picker_close();
        return true;
    }
    let on_card = ed.screen.offer_card.is_some_and(|c| inside(c, x, y))
        || ed.screen.offer_buttons.iter().any(|b| b.0 == y && (b.1..b.2).contains(&x));
    if ed.card_has_keys() && !on_card {
        ed.card_focus = None;
        if !quiet_spot(ed, x, y) {
            let key = ed.key_said("C-q");
            ed.set_status(format!("keys back — the card waits, {key} answers it"));
        }
    }
    false
}

/// What a press there does says it, the card's let-go besides it would be a second word: a toast (it opens `:messages`
/// at the message pressed — a message added before would be the one picked), a session's spot (the status line's
/// `▲1 waiting` goes to the card, the list, a session's name).
fn quiet_spot(ed: &Editor, x: usize, y: usize) -> bool {
    ed.screen.toast_card.get().is_some_and(|t| inside(t, x, y))
        || ed.screen.session_spots.borrow().iter().any(|s| s.0 == y && (s.1..s.2).contains(&x))
}

#[cfg(test)]
mod tests;
