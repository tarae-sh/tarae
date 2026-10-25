//! The notification card is news, never a pane with the keys (the user, 2026-10-25, replacing panes P3 step 7c's
//! focusable toasts: "it's a notification — shown, then gone"): it shows and fades and takes no key — no focus, not on
//! `C-w w`'s round, no keys of its own, no frame (a frame says "this has the keys"). The waiting cards' peek's look:
//! borderless on a slight ground, a message a row, its kind's `▎` (`term::draw_toasts`) — news top-right, asks
//! bottom-right, one family of one-row lines.
//!
//! - **A press on it opens `:messages`** (fading too), its home: a floating list pane, focused as your own key's pane,
//!   at the message pressed — where a message is read whole, scrolled and copied. With a prompt open the press is the
//!   prompt's, said.
//! - **`:messages` opened lets the toasts go** (user, 2026-10-23: the same words never twice), however it's opened;
//!   what's said while it's up is a row of it where the frame shows that row (`messages::rows_drawn`), else a toast
//!   as usual — an error never only on the status line — and a toast again once it closes.
//! - Its time: its reading time, an error staying until a key of yours goes by after `ERROR_MIN` — that key goes on to
//!   wherever it goes, never taken (`Editor::handle_key_now`) —, `Toast::waiting` under a picker, the narrow fold to the
//!   status line.

use crate::editor::Editor;

impl Editor {
    /// A press on the card's row `y`: `:messages` opened at the message on that row (the newest when it's on none).
    pub fn toast_press(&mut self, y: usize) {
        let rows = self.screen.toast_rows.borrow().clone();
        let under = rows.iter().rev().find(|r| r.0 <= y).or(rows.first()).map(|r| r.1);
        self.messages_open(under);
    }

    /// The card gone at once, fading or not (`:messages` opened: its words are there).
    pub fn toasts_put_away(&mut self) {
        if !self.toasts.is_empty() {
            self.unchanged = false;
        }
        self.toasts.clear();
        self.screen.toast_narrow.set(None);
    }
}

#[cfg(test)]
mod tests;
