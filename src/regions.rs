//! Where each thing to read was drawn on the last frame, by kind (focus D1, `.sb/focus-plan.md` §1): the code panes,
//! the chat's top line, transcript and input, the follow card, the asks, the toasts, the status line's now and its rest,
//! a note's card, the hovers, the side panel, the slot, what floats, a prompt — each `(kind, rect)` recorded where it's
//! drawn (`term::record_regions`), bottom first, so a cell is the last region holding it. What `cargo xtask focus`
//! reads a turn's reading path from: the words new to read in each region, frame to frame. Chrome — the drivers',
//! never Claude's level.

use serde_json::{Value, json};

/// A region's kind — one fixed list (`name`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A code pane — the edit review and wet ink count as the pane they're in.
    Code,
    ChatTop,
    ChatTranscript,
    /// The chat's input, its rule and its cost row: what you type, not what's said to you.
    ChatInput,
    Follow,
    /// A card that asks (the front one and its peeks, `space i`'s waiting card).
    Ask,
    Toast,
    /// What Claude is doing now, on the status line (`now_segments`).
    StatusNow,
    StatusRest,
    Note,
    /// `K`'s docs, the pointer's rest card, a cut text's card, completion.
    Hover,
    Side,
    /// The bottom slot and what's docked as it is (run output, lists).
    Slot,
    /// Pickers, which-key, the floating lists, the slash list, the plugins' overlays.
    Float,
    /// A prompt (`:`, `/`, `space n`'s `ask claude:`) — what you type.
    Prompt,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Code => "code",
            Kind::ChatTop => "chat.top",
            Kind::ChatTranscript => "chat.transcript",
            Kind::ChatInput => "chat.input",
            Kind::Follow => "follow",
            Kind::Ask => "ask",
            Kind::Toast => "toast",
            Kind::StatusNow => "status.now",
            Kind::StatusRest => "status.rest",
            Kind::Note => "note",
            Kind::Hover => "hover",
            Kind::Side => "side",
            Kind::Slot => "slot",
            Kind::Float => "float",
            Kind::Prompt => "prompt",
        }
    }
}

/// (x, y, width, height), in cells.
pub type Rect = (usize, usize, usize, usize);

/// One region as drawn: its kind, its cells, and for a code pane which it is and whether follow leads it (else the
/// focused one) — `lead`; `must`: what's in it waits for your answer (an ask, the edit review).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub kind: Kind,
    pub rect: Rect,
    pub pane: Option<crate::split::PaneId>,
    pub lead: bool,
    pub must: bool,
}

impl Region {
    pub fn json(&self) -> Value {
        let (x, y, w, h) = self.rect;
        let mut v = json!({ "kind": self.kind.name(), "rect": [x, y, w, h] });
        if let Some(p) = self.pane {
            v["pane"] = json!(format!("pane#{p}"));
        }
        if self.lead {
            v["lead"] = json!(true);
        }
        if self.must {
            v["must"] = json!(true);
        }
        v
    }
}

/// The frame's regions, bottom first (`Screen.regions`).
#[derive(Clone, Debug, Default)]
pub struct Regions(std::cell::RefCell<Vec<Region>>);

impl Regions {
    /// Kind `kind` drawn at `rect` — an empty one isn't listed.
    pub fn add(&self, kind: Kind, rect: Rect) {
        let must = kind == Kind::Ask;
        self.push(Region { kind, rect, pane: None, lead: false, must });
    }

    /// A code pane's (`pane`: None for the edit review or the start screen in the code's place).
    pub fn code(&self, rect: Rect, pane: Option<crate::split::PaneId>, lead: bool, must: bool) {
        self.push(Region { kind: Kind::Code, rect, pane, lead, must });
    }

    fn push(&self, r: Region) {
        if r.rect.2 > 0 && r.rect.3 > 0 {
            self.0.borrow_mut().push(r);
        }
    }

    /// Covered whole: what's under it isn't seen.
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    pub fn list(&self) -> Vec<Region> {
        self.0.borrow().clone()
    }
}

/// The region holding cell (x, y): the last listed (they're bottom first) — None: `other` (as `cargo xtask focus`
/// reads them).
#[cfg(test)]
pub fn at(regions: &[Region], x: usize, y: usize) -> Option<&Region> {
    regions.iter().rev().find(|r| {
        let (rx, ry, w, h) = r.rect;
        (rx..rx + w).contains(&x) && (ry..ry + h).contains(&y)
    })
}
