//! A plugin's overlay (the WIT's `overlay`, `.sb/overlays-plan.md` A1): a few rows floating over the screen, borderless —
//! news, never a pane with the keys (no frame, no focus, not on `C-w w`'s round), as a toast. One a plugin, replaced
//! by its next; it shows at once, stays its hold (else its text's reading time, `Toast::new`'s rule), fades and is
//! gone. A row that changed lands lit and dries (`term::flash_bg`, the flash's drying); while it's up it never gets
//! narrower (a row that slides is jitter). It floats where the asks stand, their guest; what it gives way to is
//! `term::draw_overlays`'s.

use std::time::{Duration, Instant};

use unicode_width::UnicodeWidthStr;

#[cfg(test)]
mod tests;

use super::abi;
use crate::editor::Editor;

/// The most rows an overlay has, and cells a row (the rest cut, `…`).
pub const ROWS_MAX: usize = 3;
pub const CELLS_MAX: usize = 80;
/// How long it fades once its hold is over (a toast's fade-out).
pub const FADE: Duration = Duration::from_millis(400);
/// How long a changed row dries (the flash's time).
pub const DRYING: Duration = crate::motion::FLASH;

/// A plugin's overlay as it's shown.
#[derive(Clone, Debug)]
pub struct Shown {
    /// Its rows, cleaned and cut to `CELLS_MAX` (what's drawn).
    pub rows: Vec<Vec<abi::Span>>,
    /// A cut row's whole text, by row (None: shown whole).
    pub whole: Vec<Option<String>>,
    pub hold: Duration,
    /// When it was last shown (its hold runs from there), and when each row last changed (it dries from there).
    pub at: Instant,
    pub wet: Vec<Instant>,
    /// The widest its rows have been while it's up, in cells.
    pub width: usize,
}

/// A span's width as drawn: a keycap has a cell of its ground each side.
pub fn span_width(s: &abi::Span) -> usize {
    s.text.width() + if s.role == abi::Role::Key { 2 } else { 0 }
}

fn row_width(row: &[abi::Span]) -> usize {
    row.iter().map(span_width).sum()
}

/// `row` cleaned (control and reordering characters replaced, a newline a space, empty spans dropped) and cut to
/// `CELLS_MAX` — with its whole text when it was cut.
fn cleaned(row: &[abi::Span]) -> (Vec<abi::Span>, Option<String>) {
    let row: Vec<abi::Span> = row
        .iter()
        .map(|s| abi::Span { text: super::shown(&s.text).replace('\n', " "), role: s.role })
        .filter(|s| !s.text.is_empty())
        .collect();
    match cut(&row, CELLS_MAX) {
        Some(out) => (out, Some(row.iter().map(|s| s.text.as_str()).collect())),
        None => (row, None),
    }
}

/// `row` cut to `cells`, a `…` at its end — None: it fits whole.
pub fn cut(row: &[abi::Span], cells: usize) -> Option<Vec<abi::Span>> {
    if row_width(row) <= cells {
        return None;
    }
    let (mut out, mut used) = (Vec::new(), 0);
    for s in row {
        // (a whole span only with a cell left for the `…`: there's more after it)
        if used + span_width(s) < cells {
            used += span_width(s);
            out.push(s.clone());
            continue;
        }
        // (the row is cut here — inside this span, or at its end where it fills what's left exactly: either way its
        // last cell is the `…`'s, `fit_ellipsis` as if it went on)
        let room = cells.saturating_sub(used + span_width(s) - s.text.width());
        if room > 0 {
            let text = format!("{}…", crate::cut::fit(&s.text, room - 1));
            out.push(abi::Span { text, role: s.role });
            return Some(out);
        }
        break;
    }
    out.push(abi::Span { text: "…".into(), role: abi::Role::Faint });
    Some(out)
}

impl Shown {
    /// `o` as it's shown at `now` — over `before`, the same plugin's still up: its rows unchanged stay dry, its width
    /// kept if it was wider.
    pub fn new(o: abi::Overlay, now: Instant, before: Option<&Shown>) -> Shown {
        let (rows, whole): (Vec<_>, Vec<_>) = o.rows.iter().take(ROWS_MAX).map(|r| cleaned(r)).unzip();
        let text: String = rows.iter().flatten().map(|s| s.text.as_str()).collect();
        let hold = match o.hold {
            Some(ms) => Duration::from_millis(ms),
            None => crate::editor::Toast::new(crate::editor::ToastKind::Info, text).life,
        };
        let wet = (rows.iter().enumerate())
            .map(|(i, r)| match before {
                Some(b) if b.rows.get(i) == Some(r) => b.wet[i],
                _ => now,
            })
            .collect();
        let width = rows.iter().map(|r| row_width(r)).max().unwrap_or(0);
        let width = before.map_or(width, |b| b.width.max(width));
        Shown { rows, whole, hold, at: now, wet, width }
    }

    /// How strong it's drawn at `now`, 0..1: whole through its hold, then fading out over `FADE` — where nothing moves
    /// on a clock (`moving` false) whole until its hold is over, then gone.
    pub fn alpha(&self, now: Instant, moving: bool) -> f32 {
        let age = now.saturating_duration_since(self.at);
        match age.checked_sub(self.hold) {
            None => 1.0,
            Some(_) if !moving => 0.0,
            Some(past) => (1.0 - past.as_secs_f32() / FADE.as_secs_f32()).max(0.0),
        }
    }

    /// Whether it's gone at `now` (its hold and its fade over).
    pub fn gone(&self, now: Instant, moving: bool) -> bool {
        self.until(now, moving).is_zero()
    }

    /// How long until it's gone at `now`.
    pub fn until(&self, now: Instant, moving: bool) -> Duration {
        let age = now.saturating_duration_since(self.at);
        (self.hold + if moving { FADE } else { Duration::ZERO }).saturating_sub(age)
    }

    /// Its rows' whole text, a row a line (a cut row's whole, not what's drawn).
    pub fn text(&self) -> String {
        let row = |(r, whole): (&Vec<abi::Span>, &Option<String>)| {
            whole.clone().unwrap_or_else(|| r.iter().map(|s| s.text.as_str()).collect())
        };
        self.rows.iter().zip(&self.whole).map(row).collect::<Vec<_>>().join("\n")
    }

    /// How wet row `i` is at `now`, 0..1 (`motion::strength` over `DRYING`) — 0 where nothing moves on a clock.
    pub fn wetness(&self, i: usize, now: Instant, moving: bool) -> f32 {
        match (moving, self.wet.get(i)) {
            (true, Some(&at)) => crate::motion::strength(now.saturating_duration_since(at), DRYING),
            _ => 0.0,
        }
    }

    /// When it next needs a frame: a `TICK` while a row dries or it fades, else one as its fade starts (where nothing
    /// moves on a clock: one as it goes).
    fn wake(&self, now: Instant, moving: bool) -> Duration {
        let age = now.saturating_duration_since(self.at);
        let tick = crate::editor::TICK;
        if !moving {
            return self.hold.saturating_sub(age) + Duration::from_millis(2);
        }
        let drying = (0..self.rows.len()).any(|i| self.wetness(i, now, true) > 0.0);
        // (never past its going: a short hold ends inside a row's drying)
        let step = tick.min(self.until(now, true));
        match self.hold.checked_sub(age) {
            _ if drying => step,
            Some(left) if !left.is_zero() => left,
            _ => step,
        }
    }
}

impl Editor {
    /// A plugin's `overlay` action: shown (replacing its last), or none — gone at once.
    pub(crate) fn plugin_overlay(&mut self, plugin: &str, o: Option<abi::Overlay>) {
        let (now, moving) = (crate::clock::now(), self.moving());
        let Some(p) = &mut self.plugins else { return };
        let Some(o) = o else {
            p.overlays.remove(plugin);
            return;
        };
        let before = p.overlays.get(plugin).filter(|b| !b.gone(now, moving));
        let shown = Shown::new(o, now, before);
        p.overlays.insert(plugin.to_string(), shown);
    }

    /// A press on the overlay drawn at (`x`, `y`) — its plugin hears it (`pressed`, `row` its row).
    pub(crate) fn overlay_press(&mut self, x: usize, y: usize, row: usize) {
        let inside = |(ox, oy, ow, oh): (usize, usize, usize, usize)| {
            (ox..ox + ow).contains(&x) && (oy..oy + oh).contains(&y)
        };
        let plugin = self.screen.overlays.borrow().iter().rev().find(|o| inside(o.1)).map(|o| o.0.clone());
        if let Some(plugin) = plugin {
            self.plugin_events_from(&plugin, vec![abi::HostEvent::Pressed(row as u64)]);
        }
    }

    /// When the overlays next need a frame (`tick_wanted`) — None without one.
    pub(crate) fn overlays_wake(&self) -> Option<Duration> {
        let (now, moving) = (crate::clock::now(), self.moving());
        self.plugins.as_ref()?.overlays.values().map(|o| o.wake(now, moving)).min()
    }

    /// The overlays as the ui state lists them (`uistate::OverlayState`): by plugin, each with where the last frame drew it.
    pub(crate) fn ui_overlays(&self) -> Vec<crate::uistate::OverlayState> {
        let (now, moving) = (crate::clock::now(), self.moving());
        let Some(p) = &self.plugins else { return Vec::new() };
        let drawn = self.screen.overlays.borrow();
        (p.overlays.iter())
            .map(|(plugin, o)| crate::uistate::OverlayState {
                plugin: plugin.clone(),
                text: o.text(),
                rect: drawn.iter().find(|d| &d.0 == plugin).map(|d| d.1),
                until_ms: o.until(now, moving).as_millis() as u64,
            })
            .collect()
    }

    /// Lets go of the overlays whose time is over — true when one went (drawn: it's gone from the screen).
    pub(crate) fn overlays_prune(&mut self) -> bool {
        let (now, moving) = (crate::clock::now(), self.moving());
        let Some(p) = &mut self.plugins else { return false };
        let before = p.overlays.len();
        p.overlays.retain(|_, o| !o.gone(now, moving));
        p.overlays.len() < before
    }
}
