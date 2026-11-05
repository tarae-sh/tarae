//! The lines between the frame's leaves (onetree S3, `.sb/onetree-plan.md` §4): one pass over what the frame laid out,
//! worked out once a frame (`term::frame_edges`) — each line, its cells, the leaf a drag of it sizes and the one it
//! heads (a press on it gives that one the keys). The mouse reads it (`Editor::border_at`); `Border::Divider` is one of
//! its lines, whichever two leaves it parts.
//!
//! A surface's own line is in its own cells — the chat's left gutter, the side panel's and the rail's right one, the
//! slot's top line, a docked group's edge toward the code — and sizes and heads that surface. Between two code panes
//! the line is the one before the other's (the cell between side by side, the lower one's top line over and under):
//! it sizes the pane before it (left, above) and heads the one after it.

use crate::pane::Side;
use crate::split::{Dir, PaneId, Rect};
use std::ops::Range;

/// A line: along `dir` (`Vertical` a column's `│`, `Horizontal` a row's `─`), at its column or row `at`, over the rows
/// or columns `span`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub dir: Dir,
    pub at: usize,
    pub span: Range<usize>,
    /// The leaf a drag of it sizes.
    pub sized: PaneId,
    /// The leaf it heads: a press on it gives it the keys.
    pub heads: PaneId,
}

impl Line {
    /// A surface's own line: it sizes and heads that surface.
    fn own(dir: Dir, at: usize, span: Range<usize>, id: PaneId) -> Line {
        Line { dir, at, span, sized: id, heads: id }
    }

    /// Whether cell (`x`, `y`) is on it.
    pub fn holds(&self, x: usize, y: usize) -> bool {
        match self.dir {
            Dir::Vertical => x == self.at && self.span.contains(&y),
            Dir::Horizontal => y == self.at && self.span.contains(&x),
        }
    }
}

/// What the pass reads of a frame: the leaves laid out with room, each surface's by its kind, the code panes in the
/// tree's order.
#[derive(Clone, Debug, Default)]
pub struct Leaves {
    /// The screen's width.
    pub width: usize,
    /// The code's first row (`Screen.text_top`) and one past the slot's last (`Screen::body_end`).
    pub text_top: usize,
    pub body_end: usize,
    pub chat: Option<(PaneId, Rect)>,
    /// The side panel, or the rail while a review is up — the left column as drawn.
    pub side: Option<(PaneId, Rect)>,
    pub rail: Option<(PaneId, Rect)>,
    /// The groups docked in sight: each one's host, its side and its rect, in the groups' order.
    pub docked: Vec<(PaneId, Side, Rect)>,
    /// The slot as laid (0 high when it's empty) and the column its line ends before (`Screen::slot_end`).
    pub slot: Option<(PaneId, Rect)>,
    pub slot_end: usize,
    pub code: Vec<(PaneId, Rect)>,
}

/// The frame's lines, in the order a cell finds them (`at`): the surfaces' own, then those between code panes.
#[derive(Clone, Debug, Default)]
pub struct Edges {
    pub lines: Vec<Line>,
}

impl Edges {
    /// Every line between two of `l`'s leaves.
    pub fn of(l: &Leaves) -> Edges {
        let mut lines = Vec::new();
        // (the chat's left line, where it doesn't fill the screen; the left column's right one — both the code's and the
        // slot's height)
        if let Some((id, c)) = l.chat.filter(|c| c.1.w < l.width) {
            lines.push(Line::own(Dir::Vertical, c.x, 0..l.body_end, id));
        }
        for (id, r) in [l.side, l.rail].into_iter().flatten() {
            if let Some(x) = (r.x + r.w).checked_sub(1) {
                lines.push(Line::own(Dir::Vertical, x, 0..l.body_end, id));
            }
        }
        // (a docked group's edge: its cell toward the code; over the code, the code's top line under its last row)
        for &(id, side, r) in &l.docked {
            lines.push(match side {
                Side::Right => Line::own(Dir::Vertical, r.x, r.y..r.y + r.h, id),
                Side::Left => Line::own(Dir::Vertical, (r.x + r.w).saturating_sub(1), r.y..r.y + r.h, id),
                Side::Top => Line::own(Dir::Horizontal, r.y + r.h, r.x..r.x + r.w, id),
            });
        }
        // (the slot's top line, under the code only — the chat's line its end)
        if let Some((id, s)) = l.slot.filter(|s| s.1.h > 0) {
            let end = l.slot_end.min(l.chat.map_or(l.width, |c| c.1.x));
            if s.x < end {
                lines.push(Line::own(Dir::Horizontal, s.y, s.x..end, id));
            }
        }
        // (between two code panes side by side: the cell after the left one's last, over the rows they share)
        for &(a, ra) in &l.code {
            for &(b, rb) in l.code.iter().filter(|c| c.1.x == ra.x + ra.w + 1) {
                let rows = ra.y.max(rb.y)..(ra.y + ra.h).min(rb.y + rb.h);
                if !rows.is_empty() {
                    lines.push(Line { dir: Dir::Vertical, at: ra.x + ra.w, span: rows, sized: a, heads: b });
                }
            }
        }
        // (over and under: the lower one's top line — its own first row — over the columns they share)
        for &(a, ra) in &l.code {
            for &(b, rb) in l.code.iter().filter(|c| c.1.y == ra.y + ra.h) {
                let cols = ra.x.max(rb.x)..(ra.x + ra.w).min(rb.x + rb.w);
                if crate::chrome::line_in_pane(rb, l.text_top) && !cols.is_empty() {
                    lines.push(Line { dir: Dir::Horizontal, at: rb.y, span: cols, sized: a, heads: b });
                }
            }
        }
        Edges { lines }
    }

    /// The first line on cell (`x`, `y`) that `passed` doesn't pass over (a word on it that isn't the line: a slot's
    /// tab, a name on a code pane's line).
    pub fn at(&self, x: usize, y: usize, passed: impl Fn(&Line) -> bool) -> Option<&Line> {
        self.lines.iter().find(|l| l.holds(x, y) && !passed(l))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: usize, y: usize, w: usize, h: usize) -> Rect {
        Rect { x, y, w, h }
    }

    /// Two code panes side by side meet only over the rows both have: a pane beside the lower of two stacked ones
    /// shares its line with that one alone, never the upper's.
    #[test]
    fn a_line_between_two_code_panes_spans_only_what_they_share() {
        let l = Leaves {
            width: 81,
            text_top: 1,
            body_end: 30,
            code: vec![(1, rect(0, 1, 40, 15)), (2, rect(0, 16, 40, 14)), (3, rect(41, 16, 40, 14))],
            ..Leaves::default()
        };
        let e = Edges::of(&l);
        let v: Vec<_> = e.lines.iter().filter(|l| l.dir == Dir::Vertical).collect();
        assert_eq!(v.len(), 1, "{:?}", e.lines);
        assert_eq!((v[0].at, v[0].span.clone(), v[0].sized, v[0].heads), (40, 16..30, 2, 3));
        // (the stacked pair's line: the lower one's first row, sizing the upper one)
        let h: Vec<_> = e.lines.iter().filter(|l| l.dir == Dir::Horizontal).collect();
        assert_eq!(h.len(), 1, "{:?}", e.lines);
        assert_eq!((h[0].at, h[0].span.clone(), h[0].sized, h[0].heads), (16, 0..40, 1, 2));
        assert_eq!(e.at(40, 10, |_| false), None, "beside the upper one: no line");
        assert_eq!(e.at(40, 20, |_| false).map(|l| l.heads), Some(3));
    }

    /// A surface's own line sizes and heads it; the slot's ends where the chat's begins; one cell found by two lines is
    /// the first's unless it passes over it.
    #[test]
    fn a_surfaces_own_line_sizes_and_heads_it() {
        let l = Leaves {
            width: 120,
            text_top: 1,
            body_end: 33,
            chat: Some((5, rect(84, 0, 36, 33))),
            side: Some((2, rect(0, 0, 32, 33))),
            slot: Some((4, rect(32, 24, 52, 9))),
            slot_end: 84,
            code: vec![(1, rect(32, 1, 52, 23))],
            ..Leaves::default()
        };
        let e = Edges::of(&l);
        let own = |x, y| e.at(x, y, |_| false).map(|l| (l.sized, l.heads, l.dir));
        assert_eq!(own(84, 0), Some((5, 5, Dir::Vertical)), "the chat's");
        assert_eq!(own(31, 32), Some((2, 2, Dir::Vertical)), "the side panel's, down to the slot's last row");
        assert_eq!(own(83, 24), Some((4, 4, Dir::Horizontal)), "the slot's, to the chat's line");
        assert_eq!(own(84, 24), Some((5, 5, Dir::Vertical)), "the chat's line where they meet");
        assert_eq!(own(31, 33), None, "the status line: none");
        // (a word on the slot's line passed over: nothing else there)
        assert_eq!(e.at(40, 24, |l| l.heads == 4), None);
        // (the slot's line ends at the chat's however far the slot is said to run)
        let past = Edges::of(&Leaves { slot_end: 120, ..l.clone() });
        assert_eq!(past.at(90, 24, |_| false), None, "under the chat: no line");
        // (the chat filling the screen has no line)
        let whole = Leaves { chat: Some((5, rect(0, 0, 120, 33))), ..l };
        assert!(Edges::of(&whole).lines.iter().all(|l| l.heads != 5));
    }
}
