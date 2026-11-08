//! The lines between the frame's leaves (onetree S3, `.sb/onetree-plan.md` §4): one pass over what the frame laid out,
//! worked out once a frame (`term::frame_edges`) — each line, its cells, the leaf a drag of it sizes and the one it
//! heads (a press on it gives that one the keys). The mouse reads it (`Editor::border_at`); `Border::Divider` is one of
//! its lines, whichever two leaves it parts.
//!
//! A surface's own line is in its own cells — on each side of it that meets something (`sides`): the chat's left gutter,
//! the side panel's and the rail's right one, the slot's top line where they stand today, any side by place (onetree
//! S5a); a docked group's edge toward the code — and sizes and heads that surface. Between two code panes
//! the line is the one before the other's (the cell between side by side, the lower one's top line over and under):
//! it sizes the pane before it (left, above) and heads the one after it.
//!
//! The drawers read it too (S3b): a column panel's line and a docked group's edge draw their cells as the pass has them
//! — each cell's join where another line meets it (`Line::joins`), the leaf beside it that lights it with its own
//! (`Line::beside`) —, the code's dividers are its `dividers`, and a line along a row takes in the dividers that end
//! over it (`ups`).

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
    /// Where another line meets it: each cell along it (a row of a column's line) and the join drawn there — its other
    /// cells the plain line.
    pub joins: Vec<(usize, &'static str)>,
    /// What's beside a surface's own column line, each over its rows, the first that holds a row its own (`beside_at`):
    /// the leaf that lights the line with its own. Where none is, the code.
    pub beside: Vec<(Range<usize>, PaneId)>,
}

impl Line {
    /// A surface's own line: it sizes and heads that surface.
    fn own(dir: Dir, at: usize, span: Range<usize>, id: PaneId) -> Line {
        Line { dir, at, span, sized: id, heads: id, joins: Vec::new(), beside: Vec::new() }
    }

    /// Its glyph at cell `i` along it: the join there, else the plain line.
    pub fn glyph(&self, i: usize) -> &'static str {
        let plain = if self.dir == Dir::Vertical { "│" } else { "─" };
        self.joins.iter().find(|j| j.0 == i).map_or(plain, |j| j.1)
    }

    /// The leaf beside it at cell `i` along it — None: the code.
    pub fn beside_at(&self, i: usize) -> Option<PaneId> {
        self.beside.iter().find(|b| b.0.contains(&i)).map(|b| b.1)
    }

    /// Whether cell (`x`, `y`) is on it.
    pub fn holds(&self, x: usize, y: usize) -> bool {
        match self.dir {
            Dir::Vertical => x == self.at && self.span.contains(&y),
            Dir::Horizontal => y == self.at && self.span.contains(&x),
        }
    }
}

/// The side a surface keeps its own line on where today's layout has it (`sides`): the chat's left, the side panel's
/// and the rail's right, the slot's top. Its drawer's gutter: there, a line where that side meets something, else the
/// cell blank; a line on another side is a cell of its own (`Sides::cut`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Home {
    Left,
    Right,
    Top,
}

/// The sides of a surface that hold its own line (`sides`). Never its bottom: along a row the line is the lower one's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sides {
    pub left: bool,
    pub right: bool,
    pub top: bool,
}

impl Sides {
    /// Rect `r` of a surface at `home` less its lines on its other sides, each a cell of its own: what its drawer lays
    /// its text in (its home's line is its gutter's, inside).
    pub fn cut(self, home: Home, r: Rect) -> Rect {
        let left = usize::from(self.left && home != Home::Left);
        let right = usize::from(self.right && home != Home::Right);
        Rect { x: r.x + left, w: r.w.saturating_sub(left + right), ..r }
    }
}

/// Which sides of the surface at `home` in `r` hold its own line, among `others` laid out with it (each a surface's
/// home, None for the code: its panes, a group docked in it) in a body `width` wide: a side that meets something,
/// never the screen's edge. Between a surface and the code the line is the surface's; between two surfaces side by
/// side the right one's where its home is its left (the chat's), else the left one's; along a row the lower one's.
pub fn sides(home: Home, r: Rect, others: &[(Option<Home>, Rect)], width: usize) -> Sides {
    let rows = |o: &Rect| o.y < r.y + r.h && r.y < o.y + o.h;
    let left = r.x > 0
        && (others.iter()).any(|(h, o)| o.x + o.w == r.x && rows(o) && (h.is_none() || home == Home::Left));
    let right = r.x + r.w < width
        && (others.iter()).any(|(h, o)| o.x == r.x + r.w && rows(o) && *h != Some(Home::Left));
    Sides { left, right, top: r.y > 0 }
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
    /// The column surfaces as drawn, wherever they are: the chat, the side panel or the rail while a review is up —
    /// each its leaf, its home and its rect.
    pub surfaces: Vec<(PaneId, Home, Rect)>,
    /// The groups docked in sight: each one's host, its side and its rect, in the groups' order.
    pub docked: Vec<(PaneId, Side, Rect)>,
    /// The slot as laid (0 high when it's empty) and the column its line ends before (`Screen::slot_end`).
    pub slot: Option<(PaneId, Rect)>,
    pub slot_end: usize,
    /// The slot's column beside the code with a code pane in it (`Workspace::slot_column`): its leaves — the line
    /// between a code pane in it and one outside sizes the column, as the slot's own line does.
    pub column: Vec<PaneId>,
    pub code: Vec<(PaneId, Rect)>,
    /// The code panes' top lines as drawn (`term::code_lines`), left to right: each pane, the row its line is on (over
    /// it at the code's first row, else its own first row) and its rect — none with no code drawn.
    pub tops: Vec<(PaneId, usize, Rect)>,
    /// A docked group's detail's top line, laid over and under: its host and the row.
    pub middles: Vec<(PaneId, usize)>,
    /// A picker over the editing area: a column panel's line meets nothing beside it, and turns down at the top.
    pub covered: bool,
}

impl Leaves {
    /// Which sides of leaf `id` (a column surface or the slot, at `home` in `r`) hold its own line (`sides`), among the
    /// rest laid out with room: the other surfaces and the slot by their homes, a docked group by its edge (docked right,
    /// its line on its left), the code panes as the code.
    pub fn sides_of(&self, id: PaneId, home: Home, r: Rect) -> Sides {
        let room = |r: &Rect| r.w > 0 && r.h > 0;
        let edge = |side: Side| match side {
            Side::Right => Home::Left,
            Side::Left => Home::Right,
            Side::Top => Home::Top,
        };
        let others: Vec<(Option<Home>, Rect)> = (self.surfaces.iter().map(|s| (s.0, Some(s.1), s.2)))
            .chain(self.slot.map(|s| (s.0, Some(Home::Top), s.1)))
            .chain(self.code.iter().map(|c| (c.0, None, c.1)))
            .chain(self.docked.iter().map(|d| (d.0, Some(edge(d.1)), d.2)))
            .filter(|o| o.0 != id && room(&o.2))
            .map(|o| (o.1, o.2))
            .collect();
        sides(home, r, &others, self.width)
    }
}

/// A divider as drawn between code panes side by side: the column left of pane `pane` with another left of it, from
/// that pane's top line down — one a pane, however many are beside it (the borders, `Edges::lines`, are one a pair:
/// what each shares).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Divider {
    pub at: usize,
    pub span: Range<usize>,
    pub pane: PaneId,
    /// Where a pane's top line meets it (`├ ┤ ┼`) or it starts at one (`┬ ┌ ┐`), each its row — else `│`.
    pub joins: Vec<(usize, &'static str)>,
}

impl Divider {
    /// Its glyph on row `y`.
    pub fn glyph(&self, y: usize) -> &'static str {
        self.joins.iter().find(|j| j.0 == y).map_or("│", |j| j.1)
    }
}

/// The frame's lines, in the order a cell finds them (`at`): the surfaces' own, then those between code panes.
#[derive(Clone, Debug, Default)]
pub struct Edges {
    pub lines: Vec<Line>,
    /// The code's dividers as drawn, in the panes' order.
    pub dividers: Vec<Divider>,
}

impl Edges {
    /// Every line between two of `l`'s leaves.
    pub fn of(l: &Leaves) -> Edges {
        let mut lines = Vec::new();
        // (each column surface's lines on its sides that meet something, down its own rows: its home's first — the
        // chat's left, the side panel's and the rail's right —, then the other side's, each with the side the code is
        // toward for its joins. Stacked under something, its top line — its own header's row — is its line along a row:
        // a drag of it sizes its height, onetree S5b)
        let mut columns: Vec<(usize, Side)> = Vec::new();
        for &(id, home, r) in &l.surfaces {
            let s = l.sides_of(id, home, r);
            if s.top && r.w > 0 && r.h > 0 {
                lines.push(Line::own(Dir::Horizontal, r.y, r.x..r.x + r.w, id));
            }
            let rows = r.y..(r.y + r.h).min(l.body_end);
            let (left, right) =
                ((s.left, r.x, Side::Left), (s.right, r.x + r.w.saturating_sub(1), Side::Right));
            let order = if home == Home::Left { [left, right] } else { [right, left] };
            for (on, x, toward) in order {
                if on && r.w > 0 {
                    columns.push((lines.len(), toward));
                    lines.push(Line::own(Dir::Vertical, x, rows.clone(), id));
                }
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
        // (the slot's top line, under the code only — a column surface's line right of it its end; beside the code, its
        // line toward it)
        if let Some((id, s)) = l.slot.filter(|s| s.1.h > 0) {
            let rows = |r: &Rect| r.y <= s.y && s.y < r.y + r.h;
            let past = (l.surfaces.iter()).filter(|c| c.2.x > s.x && rows(&c.2)).map(|c| c.2.x).min();
            let end = l.slot_end.min(past.unwrap_or(l.width));
            let sides = l.sides_of(id, Home::Top, s);
            if s.x < end && sides.top {
                lines.push(Line::own(Dir::Horizontal, s.y, s.x..end, id));
            }
            for (on, x, toward) in
                [(sides.left, s.x, Side::Left), (sides.right, s.x + s.w.saturating_sub(1), Side::Right)]
            {
                if on && s.w > 0 {
                    columns.push((lines.len(), toward));
                    lines.push(Line::own(Dir::Vertical, x, s.y..s.y + s.h, id));
                }
            }
        }
        // (between two code panes side by side: the cell after the left one's last, over the rows they share — into the
        // slot's column or out of it, the column's edge: the slot sizes it)
        let column = |id: PaneId| l.column.contains(&id);
        for &(a, ra) in &l.code {
            for &(b, rb) in l.code.iter().filter(|c| c.1.x == ra.x + ra.w + 1) {
                let rows = ra.y.max(rb.y)..(ra.y + ra.h).min(rb.y + rb.h);
                let sized = match l.slot {
                    Some((slot, _)) if column(a) != column(b) => slot,
                    _ => a,
                };
                if !rows.is_empty() {
                    lines.push(Line {
                        dir: Dir::Vertical,
                        at: ra.x + ra.w,
                        span: rows,
                        sized,
                        heads: b,
                        joins: Vec::new(),
                        beside: Vec::new(),
                    });
                }
            }
        }
        // (over and under: the lower one's top line — its own first row — over the columns they share)
        for &(a, ra) in &l.code {
            for &(b, rb) in l.code.iter().filter(|c| c.1.y == ra.y + ra.h) {
                let cols = ra.x.max(rb.x)..(ra.x + ra.w).min(rb.x + rb.w);
                if crate::chrome::line_in_pane(rb, l.text_top) && !cols.is_empty() {
                    lines.push(Line {
                        dir: Dir::Horizontal,
                        at: rb.y,
                        span: cols,
                        sized: a,
                        heads: b,
                        joins: Vec::new(),
                        beside: Vec::new(),
                    });
                }
            }
        }
        // What the column panels' lines and the docked groups' edges meet, and the code's dividers as drawn
        let column = |lines: &[Line], id: PaneId| lines.iter().position(|ln| own_column(ln, id));
        for (i, toward) in columns {
            meet(l, &mut lines[i], toward);
        }
        for &(id, side, r) in &l.docked {
            if side != Side::Top
                && let Some(i) = column(&lines, id)
            {
                dock_meet(l, &mut lines[i], id, side, r);
            }
        }
        Edges { lines, dividers: dividers(&l.tops) }
    }

    /// Surface `id`'s own line down a column (the chat's, the side panel's, the rail's, a group's docked beside the
    /// code) — its home's first (`Leaves::sides_of`) — None where it has none.
    pub fn column(&self, id: PaneId) -> Option<&Line> {
        self.lines.iter().find(|l| own_column(l, id))
    }

    /// The columns of the code's dividers that end on the row over `y`: a line along row `y` takes them in (`┴`).
    pub fn ups(&self, y: usize) -> Vec<usize> {
        self.dividers.iter().filter(|d| y > 0 && d.span.end == y).map(|d| d.at).collect()
    }

    /// The columns of every line down a column that ends on the row over `y` — the code's dividers and the surfaces'
    /// own: a surface's top line along row `y` (the chat under the code, S5a) takes them in (`┴`).
    pub fn ends_over(&self, y: usize) -> Vec<usize> {
        let lines =
            (self.lines.iter()).filter(|l| l.dir == Dir::Vertical && y > 0 && l.span.end == y).map(|l| l.at);
        let mut out: Vec<usize> = lines.chain(self.ups(y)).collect();
        crate::sorting::by_key(&mut out, |&x| x);
        out.dedup();
        out
    }

    /// The first line on cell (`x`, `y`) that `passed` doesn't pass over (a word on it that isn't the line: a slot's
    /// tab, a name on a code pane's line).
    pub fn at(&self, x: usize, y: usize, passed: impl Fn(&Line) -> bool) -> Option<&Line> {
        self.lines.iter().find(|l| l.holds(x, y) && !passed(l))
    }
}

/// Whether `l` is surface `id`'s own line down a column.
fn own_column(l: &Line, id: PaneId) -> bool {
    l.heads == id && l.sized == id && l.dir == Dir::Vertical
}

/// A column panel's line (the side panel's or the rail's, the code `toward` its right; the chat's, the code toward its
/// left): what's beside it on each row and where its line meets this one. What it meets, in order: the groups docked
/// against it (on the code's side of it) or over the code — each over its rows, its top line a join —, the code panes'
/// top lines below the screen's first row that start or end at it, and only where none of those is, the slot (its top
/// line). The first that holds a row is that row's, its join only on its own line. On the first row both sides' top
/// lines meet it (`┬`); a picker over the editing area covers what's beside it: none, and the line turns down there.
fn meet(l: &Leaves, line: &mut Line, toward: Side) {
    let (join, corner, edge) = match toward {
        Side::Left => ("┤", "┌", line.at),
        _ => ("├", "┐", line.at + 1),
    };
    // (each part: its rows, the row of its own line, its leaf)
    let mut parts: Vec<(Range<usize>, usize, PaneId)> = Vec::new();
    if !l.covered {
        for &(id, side, r) in &l.docked {
            // (the groups docked toward this line are on the code's side of it: the chat's on the right, the side
            // panel's on the left)
            let against = match toward {
                Side::Left => side == Side::Right,
                _ => side == Side::Left,
            };
            if against || side == Side::Top {
                parts.push((r.y..r.y + r.h, r.y, id));
            }
        }
        for &(id, row, r) in l.tops.iter().filter(|t| t.1 > 0) {
            let at = match toward {
                Side::Left => r.x + r.w == edge,
                _ => r.x == edge,
            };
            if at {
                parts.push((row..row + 1, row, id));
            }
        }
        // (the slot beside this line, its side toward it there — by place, another column may stand between them)
        let beside = |s: &Rect| match toward {
            Side::Left => s.x + s.w == edge,
            _ => s.x == edge,
        };
        if parts.is_empty()
            && let Some((id, s)) = l.slot.filter(|s| s.1.h > 0 && s.1.w > 0 && beside(&s.1))
        {
            parts.push((s.y..s.y + s.h, s.y, id));
        }
    }
    line.joins.push((0, if l.covered { corner } else { "┬" }));
    for (i, p) in parts.iter().enumerate() {
        let first = parts.iter().position(|q| q.0.contains(&p.1)) == Some(i);
        if p.1 > 0 && first {
            line.joins.push((p.1, join));
        }
    }
    line.beside = parts.into_iter().map(|p| (p.0, p.2)).collect();
}

/// A docked group's edge (`host`'s, docked `side` in `r`), in its cell toward the code: it starts at its own top line
/// (`┌`/`┐`, never above it — `┬` where a code pane's top line meets it there too), its detail's top line laid over
/// and under meets it from its side (`├`/`┤`), the slot's top line or a code pane's from the code's (`┤`/`├`). Beside
/// it the slot over the slot's rows, else the code.
fn dock_meet(l: &Leaves, line: &mut Line, host: PaneId, side: Side, r: Rect) {
    let x = line.at;
    let (start, own, meet) = if side == Side::Right { ("┌", "├", "┤") } else { ("┐", "┤", "├") };
    let middle = l.middles.iter().find(|m| m.0 == host).map(|m| m.1);
    let code_at = |y: usize| l.tops.iter().any(|t| t.1 == y && (t.2.x + t.2.w == x || t.2.x == x + 1));
    // (the slot beside it: its line meets this one)
    let slot = l.slot.filter(|s| s.1.h > 0 && s.1.w > 0 && (s.1.x + s.1.w == x || x + 1 == s.1.x));
    for y in r.y..r.y + r.h {
        let glyph = match y {
            _ if y == r.y && code_at(y) => "┬",
            _ if y == r.y => start,
            _ if Some(y) == middle => own,
            _ if slot.is_some_and(|s| y == s.1.y) => meet,
            _ if code_at(y) => meet,
            _ => continue,
        };
        line.joins.push((y, glyph));
    }
    if let Some((id, s)) = slot {
        line.beside.push((s.y..s.y + s.h, id));
    }
}

/// The code's dividers as drawn (`tops`: the code panes' top lines, left to right): left of each pane with another
/// left of it over any of its rows, from its top line down. Each starts at a line (`┬`, `┌`/`┐` with a line on one side
/// only) and is joined where a pane's top line meets it from a side (`├` `┤`), from both (`┼`).
fn dividers(tops: &[(PaneId, usize, Rect)]) -> Vec<Divider> {
    let mut out: Vec<Divider> = Vec::new();
    for &(pane, top, r) in tops {
        let Some(x) = r.x.checked_sub(1) else { continue };
        let span = top..r.y + r.h;
        let beside = tops.iter().any(|q| {
            let q = q.2;
            q.x < r.x && q.x + q.w + 1 >= r.x && q.y < span.end && span.start < q.y + q.h
        });
        if beside {
            out.push(Divider { at: x, span, pane, joins: Vec::new() });
        }
    }
    let spans: Vec<(usize, Range<usize>)> = out.iter().map(|d| (d.at, d.span.clone())).collect();
    for d in &mut out {
        let x = d.at;
        for y in d.span.clone() {
            let up = y > 0 && spans.iter().any(|s| s.0 == x && s.1.contains(&(y - 1)));
            let left = tops.iter().any(|t| t.1 == y && t.2.x + t.2.w == x);
            let right = tops.iter().any(|t| t.1 == y && t.2.x == x + 1);
            let glyph = match (up, left, right) {
                (false, true, true) => "┬",
                (false, false, true) => "┌",
                (false, true, false) => "┐",
                (true, true, true) => "┼",
                (true, false, true) => "├",
                (true, true, false) => "┤",
                _ => continue,
            };
            d.joins.push((y, glyph));
        }
    }
    out
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
            surfaces: vec![(5, Home::Left, rect(84, 0, 36, 33)), (2, Home::Right, rect(0, 0, 32, 33))],
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
        let whole = Leaves { surfaces: vec![(5, Home::Left, rect(0, 0, 120, 33))], ..l };
        assert!(Edges::of(&whole).lines.iter().all(|l| l.heads != 5));
    }

    /// By place (onetree S5a): a surface's line is on each side that meets something — never the screen's edge —, one
    /// line between two surfaces (the chat's where its home faces the other, else the left one's), the slot's toward
    /// the code beside it; a line on a side other than its home's joins what's beside it there, the slot's only where
    /// the slot is beside it.
    #[test]
    fn each_surfaces_line_is_on_the_side_that_meets_something() {
        // (the chat on the left, the side panel right of it, the code over the slot)
        let l = Leaves {
            width: 120,
            text_top: 1,
            body_end: 33,
            surfaces: vec![(5, Home::Left, rect(0, 0, 36, 33)), (2, Home::Right, rect(36, 0, 32, 33))],
            slot: Some((4, rect(68, 30, 52, 3))),
            slot_end: 120,
            code: vec![(1, rect(68, 1, 52, 29))],
            ..Leaves::default()
        };
        assert_eq!(
            l.sides_of(5, Home::Left, rect(0, 0, 36, 33)),
            Sides { left: false, right: true, top: false }
        );
        assert_eq!(
            l.sides_of(2, Home::Right, rect(36, 0, 32, 33)),
            Sides { left: false, right: true, top: false }
        );
        let e = Edges::of(&l);
        let ats: Vec<_> =
            e.lines.iter().filter(|l| l.dir == Dir::Vertical).map(|l| (l.at, l.heads)).collect();
        assert_eq!(ats, vec![(35, 5), (67, 2)]);
        assert_eq!(e.column(5).unwrap().joins, vec![(0, "┬")], "the slot isn't beside the chat's line");
        assert_eq!(e.column(2).unwrap().joins, vec![(0, "┬"), (30, "├")]);
        // (the side panel left of the chat: the chat's home faces it — the chat's line, none of the side panel's)
        let nav = Leaves {
            surfaces: vec![(5, Home::Left, rect(52, 0, 36, 33)), (2, Home::Right, rect(88, 0, 32, 33))],
            slot: Some((4, rect(0, 30, 52, 3))),
            code: vec![(1, rect(0, 1, 52, 29))],
            ..l.clone()
        };
        assert_eq!(nav.sides_of(2, Home::Right, rect(88, 0, 32, 33)), Sides::default());
        let ats: Vec<_> =
            Edges::of(&nav).lines.iter().filter(|l| l.dir == Dir::Vertical).map(|l| l.at).collect();
        assert_eq!(ats, vec![52, 87]);
        // (the slot beside the code: its line toward it, cut from what it draws)
        let stack = Leaves {
            surfaces: vec![(5, Home::Left, rect(84, 0, 36, 33))],
            slot: Some((4, rect(49, 0, 35, 33))),
            code: vec![(1, rect(0, 1, 49, 32))],
            ..l
        };
        let s = stack.sides_of(4, Home::Top, rect(49, 0, 35, 33));
        assert_eq!(s, Sides { left: true, right: false, top: false });
        assert_eq!(s.cut(Home::Top, rect(49, 0, 35, 33)), rect(50, 0, 34, 33));
        let e = Edges::of(&stack);
        assert_eq!(e.at(49, 10, |_| false).map(|l| (l.heads, l.dir)), Some((4, Dir::Vertical)));
        assert_eq!(e.at(60, 0, |_| false), None, "at the screen's top: no top line of its own");
    }

    /// A column panel's line joins what meets it beside it: a code pane's top line below the first row (the side panel's
    /// `├`, the chat's `┤`), the slot's only where nothing else is; `┬` on the first row; a picker over the code leaves it
    /// its own top alone, turning down.
    #[test]
    fn a_column_panels_line_joins_what_meets_it() {
        let l = Leaves {
            width: 120,
            text_top: 1,
            body_end: 33,
            surfaces: vec![(5, Home::Left, rect(84, 0, 36, 33)), (2, Home::Right, rect(0, 0, 32, 33))],
            slot: Some((4, rect(32, 24, 52, 9))),
            slot_end: 84,
            code: vec![(1, rect(32, 1, 52, 15)), (3, rect(32, 16, 52, 8))],
            tops: vec![(1, 0, rect(32, 1, 52, 15)), (3, 16, rect(32, 16, 52, 8))],
            ..Leaves::default()
        };
        let e = Edges::of(&l);
        let (side, chat) = (e.column(2).unwrap(), e.column(5).unwrap());
        assert_eq!(side.joins, vec![(0, "┬"), (16, "├")]);
        assert_eq!(
            (side.glyph(16), side.glyph(20), side.beside_at(16), side.beside_at(24)),
            ("├", "│", Some(3), None)
        );
        assert_eq!(chat.joins, vec![(0, "┬"), (16, "┤")]);
        // (one pane: the slot beside it, its top line a join)
        let one = Leaves { tops: vec![(1, 0, rect(32, 1, 52, 23))], ..l.clone() };
        let e = Edges::of(&one);
        assert_eq!(e.column(2).unwrap().joins, vec![(0, "┬"), (24, "├")]);
        assert_eq!(e.column(2).unwrap().beside_at(30), Some(4));
        // (a group docked over the code holds its rows first: a pane's top line in them isn't a join)
        let over = Leaves { docked: vec![(6, Side::Top, rect(32, 10, 52, 10))], ..l.clone() };
        let e = Edges::of(&over);
        assert_eq!(e.column(2).unwrap().joins, vec![(0, "┬"), (10, "├")]);
        assert_eq!(e.column(2).unwrap().beside_at(16), Some(6));
        // (under a picker: only its own top line, turning down)
        let e = Edges::of(&Leaves { covered: true, ..l });
        assert_eq!(e.column(2).unwrap().joins, vec![(0, "┐")]);
        assert_eq!(e.column(5).unwrap().joins, vec![(0, "┌")]);
        assert!(e.column(2).unwrap().beside.is_empty());
    }

    /// A docked group's edge starts at its own top line (`┬` where a code pane's meets it there too), its detail laid
    /// under its list joins it from its side, the slot's and a code pane's top lines from the code's; the slot is beside
    /// it over the slot's rows. Docked over the code it has no column line.
    #[test]
    fn a_docked_groups_edge_joins_what_meets_it() {
        let l = Leaves {
            width: 120,
            text_top: 1,
            body_end: 33,
            docked: vec![(6, Side::Right, rect(60, 1, 24, 32))],
            middles: vec![(6, 10)],
            slot: Some((4, rect(32, 24, 28, 9))),
            slot_end: 60,
            tops: vec![(1, 0, rect(32, 1, 28, 11)), (3, 12, rect(32, 12, 28, 12))],
            ..Leaves::default()
        };
        let e = Edges::of(&l);
        let edge = e.column(6).unwrap();
        assert_eq!(edge.joins, vec![(1, "┌"), (10, "├"), (12, "┤"), (24, "┤")]);
        assert_eq!((edge.beside_at(24), edge.beside_at(23)), (Some(4), None));
        // (docked left: the other way round; a code pane's top line on its first row too: `┬`)
        let left = Leaves {
            docked: vec![(6, Side::Left, rect(32, 1, 24, 32))],
            slot: None,
            tops: vec![(1, 1, rect(56, 1, 28, 32))],
            ..l.clone()
        };
        let e = Edges::of(&left);
        assert_eq!(e.column(6).unwrap().at, 55);
        assert_eq!(e.column(6).unwrap().joins, vec![(1, "┬"), (10, "┤")]);
        let top = Leaves { docked: vec![(6, Side::Top, rect(32, 1, 52, 10))], ..l };
        assert!(Edges::of(&top).column(6).is_none());
    }

    /// The code's dividers are one a pane with another left of it, from its top line down: `┬` where it starts between
    /// two top lines, `┤` where the lower left pane's meets it, `┼` where both sides' do; a row's line under one takes it
    /// in (`ups`).
    #[test]
    fn the_codes_dividers_start_at_a_line_and_join_the_top_lines() {
        let (a, b) = ((1, 0, rect(0, 1, 40, 15)), (2, 16, rect(0, 16, 40, 14)));
        let l = Leaves {
            width: 81,
            text_top: 1,
            body_end: 30,
            tops: vec![a, b, (3, 0, rect(41, 1, 40, 29))],
            ..Leaves::default()
        };
        let e = Edges::of(&l);
        assert_eq!(e.dividers.len(), 1, "{:?}", e.dividers);
        let d = &e.dividers[0];
        assert_eq!((d.at, d.span.clone(), d.pane), (40, 0..30, 3));
        assert_eq!(d.joins, vec![(0, "┬"), (16, "┤")]);
        assert_eq!((e.ups(30), e.ups(29)), (vec![40], vec![]));
        // (four panes: the lower right one's divider starts where both lower top lines meet it, under the upper one's)
        let four =
            Leaves { tops: vec![a, b, (3, 0, rect(41, 1, 40, 15)), (4, 16, rect(41, 16, 40, 14))], ..l };
        let e = Edges::of(&four);
        assert_eq!(
            e.dividers.iter().map(|d| (d.span.clone(), d.glyph(16))).collect::<Vec<_>>(),
            vec![(0..16, "│"), (16..30, "┼")]
        );
        assert_eq!(e.ups(16), vec![40]);
    }
}
