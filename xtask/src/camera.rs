//! A Short's camera (`scenes --video --shorts`, `video.rs`): the scene's `@zoom:<target>` lines, each found on the
//! screen in force at its time (`target`) and eased to (`at`: `EASE_MS`, an ease-out — the motion rule's), held until
//! the next `@zoom` or beat (a beat with none of its own goes back to the whole screen). Where the camera is is a
//! region of the screen in cells; a frame draws the screen with glyphs at the largest whole size the region fits the
//! window at (`fit`) and crops it there (`crop`) — never past the picture's edge, the region always whole in it. A
//! target's region is shaped to the window as it's found (`framed`): the rows or columns it adds put where they cut no
//! card (a frame, or a borderless one by its ground — `cards`), else the cut card taken in whole.
//! Pure: the frames' drawing is `video.rs`'s.

/// A screen as the camera reads it: each cell's text and its ground (its background, as a number), row by row.
pub(crate) struct Cells {
    pub(crate) text: Vec<Vec<String>>,
    pub(crate) ground: Vec<Vec<u32>>,
}

impl Cells {
    pub(crate) fn of(screen: &vt100::Screen) -> Cells {
        let (rows, cols) = screen.size();
        let ground = |r, c| {
            screen.cell(r, c).map_or(0, |cell| match cell.bgcolor() {
                vt100::Color::Default => 0,
                vt100::Color::Idx(i) => 1 + u32::from(i),
                vt100::Color::Rgb(r, g, b) => 0x100_0000 | u32::from_be_bytes([0, r, g, b]),
            })
        };
        let text = (0..rows)
            .map(|r| {
                (0..cols)
                    .map(|c| screen.cell(r, c).map(|x| x.contents().to_string()).unwrap_or_default())
                    .collect()
            })
            .collect();
        Cells { text, ground: (0..rows).map(|r| (0..cols).map(|c| ground(r, c)).collect()).collect() }
    }
}

/// A card on the screen: its top row, left column, bottom row, right column (inclusive).
type Card = (usize, usize, usize, usize);

/// The camera's region: its left column, top row, width and height in cells (fractions while it moves).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Region {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

/// A move's length: the motion rule's ≤ 0.4 s.
pub(crate) const EASE_MS: u64 = 400;
/// The air kept round a target, in cells.
const AIR: f32 = 1.0;

impl Region {
    /// The whole screen of `cols`×`rows`.
    pub(crate) fn all((cols, rows): (u16, u16)) -> Region {
        Region { x: 0.0, y: 0.0, w: f32::from(cols), h: f32::from(rows) }
    }

    /// Inside the screen: no wider or taller than it, moved back in where it passes an edge.
    fn inside(self, (cols, rows): (u16, u16)) -> Region {
        let (cw, rh) = (f32::from(cols), f32::from(rows));
        let (w, h) = (self.w.clamp(1.0, cw), self.h.clamp(1.0, rh));
        Region { x: self.x.clamp(0.0, cw - w), y: self.y.clamp(0.0, rh - h), w, h }
    }

    fn toward(self, to: Region, t: f32) -> Region {
        let l = |a: f32, b: f32| a + (b - a) * t;
        Region { x: l(self.x, to.x), y: l(self.y, to.y), w: l(self.w, to.w), h: l(self.h, to.h) }
    }

    /// Cells `c0..c1`, rows `r0..r1` with `AIR` round them, inside the screen.
    fn of(c0: usize, r0: usize, c1: usize, r1: usize, screen: (u16, u16)) -> Region {
        let (x, y) = (c0 as f32 - AIR, r0 as f32 - AIR);
        let r = Region { x, y, w: (c1 - c0) as f32 + 2.0 * AIR, h: (r1 - r0) as f32 + 2.0 * AIR };
        r.inside(screen)
    }
}

/// The motion rule's ease-out: fast, then settling.
pub(crate) fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Where the camera is `ms` into the take, given its moves (each: when, where to — in order): the whole screen before
/// the first; each move eased over `EASE_MS` from wherever the camera was as it began (a move begun mid-way through
/// another starts from there, never a jump); every region inside the screen.
pub(crate) fn at(moves: &[(u64, Region)], screen: (u16, u16), ms: u64) -> Region {
    let all = Region::all(screen);
    let (mut from, mut to, mut since) = (all, all, 0u64);
    let eased = |from: Region, to: Region, since: u64, now: u64| {
        from.toward(to, ease_out(now.saturating_sub(since) as f32 / EASE_MS as f32))
    };
    for &(t, region) in moves.iter().take_while(|m| m.0 <= ms) {
        from = eased(from, to, since, t);
        (to, since) = (region.inside(screen), t);
    }
    eased(from, to, since, ms)
}

/// The largest whole glyph size (8 ..= `max`) whose cells, `cell(px)` each, show `region` whole in a `view` window.
pub(crate) fn fit(
    region: Region,
    view: (usize, usize),
    max: u32,
    cell: impl Fn(f32) -> (usize, usize),
) -> f32 {
    (8..=max)
        .rev()
        .map(|px| px as f32)
        .find(|&px| {
            let (cw, ch) = cell(px);
            region.w * cw as f32 <= view.0 as f32 && region.h * ch as f32 <= view.1 as f32
        })
        .unwrap_or(8.0)
}

/// The window's top-left in a picture of the screen (`pic` px, its cells `cell` px from a `pad` margin) that shows
/// `region`: centred on it, moved back inside the picture where it would pass an edge; where the picture is smaller
/// than the window, centred on the picture (negative: the window's ground round it).
pub(crate) fn crop(
    region: Region,
    cell: (usize, usize),
    pad: usize,
    pic: (usize, usize),
    view: (usize, usize),
) -> (i64, i64) {
    let axis = |start: f32, len: f32, cell: usize, pic: usize, view: usize| -> i64 {
        if pic < view {
            return (pic as i64 - view as i64) / 2;
        }
        let centre = pad as f32 + (start + len / 2.0) * cell as f32;
        ((centre - view as f32 / 2.0).round() as i64).clamp(0, (pic - view) as i64)
    };
    (axis(region.x, region.w, cell.0, pic.0, view.0), axis(region.y, region.h, cell.1, pic.1, view.1))
}

/// A target's region on a screen (its cells' text, row by row — `recording::screen_cells`):
/// - `all` the whole screen;
/// - `status` the status line (the last row's text);
/// - `strip` the top row's last stretch of text (the sessions' strip on its right, while no chat column stands);
/// - `code:A-B` the code's lines A to B by the gutter's numbers, as wide as the longest of them;
/// - `card:<text>` (or any other text) the smallest card holding `<text>` (`cards`: a frame — a floating card, a list —,
///   or a toast's or a peek's ground), else its row's stretch of text (to two blanks or a line).
///
/// Each with a cell of air round it. None where it isn't on that screen.
pub(crate) fn target(on: &Cells, spec: &str) -> Option<Region> {
    let cells = &on.text;
    let rows = cells.len();
    let cols = cells.first().map_or(0, Vec::len);
    let screen = (cols as u16, rows as u16);
    let blank = |s: &str| s.trim().is_empty();
    match spec {
        "all" => Some(Region::all(screen)),
        "status" => {
            let r = rows.checked_sub(1)?;
            let (c0, c1) = text_span(&cells[r], 0, cols)?;
            Some(Region::of(c0, r, c1, r + 1, screen))
        }
        "strip" => {
            let row = cells.first()?;
            // (stretches of text apart from the line's `─` and blanks; the last one)
            let line = |s: &str| blank(s) || s == "─" || s == "┬" || s == "┴";
            let end = (0..cols).rev().find(|&c| !line(&row[c]))? + 1;
            let mut start = end - 1;
            while start > 0 && !(line(&row[start - 1]) && (start < 2 || line(&row[start - 2]))) {
                start -= 1;
            }
            Some(Region::of(start, 0, end, 1, screen))
        }
        _ => match spec.strip_prefix("code:") {
            Some(range) => {
                let (a, b) = range.split_once('-').unwrap_or((range, range));
                let (a, b): (u32, u32) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
                let lines: Vec<(usize, usize, usize)> = (0..rows)
                    .filter_map(|r| {
                        let (n, from, end) = numbered(&cells[r])?;
                        (a..=b).contains(&n).then_some((r, from, end))
                    })
                    .collect();
                let r0 = lines.iter().map(|l| l.0).min()?;
                let r1 = lines.iter().map(|l| l.0).max()? + 1;
                let c0 = lines.iter().map(|l| l.1).min()?;
                let c1 = lines.iter().map(|l| l.2).max()?;
                Some(Region::of(c0, r0, c1, r1, screen))
            }
            None => {
                let text = spec.strip_prefix("card:").unwrap_or(spec);
                let (r, c0, c1) = find(cells, text)?;
                let held = cards(on)
                    .into_iter()
                    .filter(|b| b.0 <= r && r <= b.2 && b.1 <= c0 && c1 <= b.3 + 1)
                    .min_by_key(|b| (b.2 - b.0 + 1) * (b.3 - b.1 + 1));
                Some(match held {
                    Some((r0, c0, r1, c1)) => Region::of(c0, r0, c1 + 1, r1 + 1, screen),
                    None => {
                        // (the row's stretch round it: to two blanks, a line or the edge)
                        let row = &cells[r];
                        let stop = |c: usize| matches!(row[c].as_str(), "│" | "─");
                        let gap = |c: usize| blank(&row[c]) && (c == 0 || blank(&row[c - 1]));
                        let first = (0..c0).rev().find(|&c| stop(c) || gap(c)).map_or(0, |c| c + 1);
                        let gap_after =
                            |c: usize| blank(&row[c]) && (c + 1 >= row.len() || blank(&row[c + 1]));
                        let end = (c1..row.len()).find(|&c| stop(c) || gap_after(c)).unwrap_or(row.len());
                        let (first, end) = text_span(row, first, end).unwrap_or((c0, c1));
                        Region::of(first, r, end, r + 1, screen)
                    }
                })
            }
        },
    }
}

/// A row's text between `from` and `to`: its first and past-its-last non-blank cell.
fn text_span(row: &[String], from: usize, to: usize) -> Option<(usize, usize)> {
    let first = (from..to).find(|&c| !row[c].trim().is_empty())?;
    let last = (from..to).rev().find(|&c| !row[c].trim().is_empty())?;
    Some((first, last + 1))
}

/// A code row's gutter number, where it starts and where the row's text ends inside its pane: in the first pane of
/// the row (its stretches between `│`s) whose first letter or digit starts a run of digits and a blank — only signs,
/// lines and blanks before it (a side panel's stretch starts with its names) —, the pane's end its next `│`.
fn numbered(row: &[String]) -> Option<(u32, usize, usize)> {
    let starts = std::iter::once(0).chain((0..row.len()).filter(|&c| row[c] == "│").map(|c| c + 1));
    starts.into_iter().find_map(|from| {
        let pane = (from..row.len()).find(|&c| row[c] == "│").unwrap_or(row.len());
        let first = (from..pane).find(|&c| row[c].chars().next().is_some_and(|ch| ch.is_alphanumeric()))?;
        let digit = |c: usize| !row[c].is_empty() && row[c].chars().all(|ch| ch.is_ascii_digit());
        let end = (first..pane).find(|&c| !digit(c))?;
        if end == first || !row[end].trim().is_empty() {
            return None;
        }
        let n: u32 = row[first..end].concat().parse().ok()?;
        let (_, last) = text_span(row, first, pane)?;
        Some((n, first, last))
    })
}

/// The first place `text` is on the screen: its row, first cell and past its last.
fn find(cells: &[Vec<String>], text: &str) -> Option<(usize, usize, usize)> {
    cells.iter().enumerate().find_map(|(r, row)| {
        // (the row as a string, each character's cell beside it — a wide one's second cell is empty)
        let (mut s, mut at) = (String::new(), Vec::new());
        for (c, cell) in row.iter().enumerate() {
            let cell = if cell.is_empty() { "" } else { cell.as_str() };
            for _ in cell.chars() {
                at.push(c);
            }
            s.push_str(cell);
        }
        let byte = s.find(text)?;
        let first = s[..byte].chars().count();
        let last = first + text.chars().count() - 1;
        Some((r, *at.get(first)?, *at.get(last)? + 1))
    })
}

/// A cut card is taken in with the target only while that costs the zoom at most this much (the window this many
/// times as wide); past it the camera draws the target alone (`framed`'s clip).
const TAKEN_IN_MAX: f32 = 1.25;

/// `target`'s region shaped to a window of `aspect` columns a row, and what of it is drawn (the clip: outside it the
/// window's ground). The rows (or columns) the shape adds go where no card is cut by the window's edge — the placement
/// nearest the target's centre, all of it drawn; where every placement cuts one, that card is taken in with the target
/// and shaped again while the window grows no more than `TAKEN_IN_MAX` (at most four times); else the target's own
/// shape nearest its centre, only the target drawn — nothing cut, the zoom kept. Inside the screen.
pub(crate) fn framed(screen: &Cells, target: Region, aspect: f32) -> (Region, Region) {
    let rows = screen.text.len();
    let cols = screen.text.first().map_or(0, Vec::len);
    let size = (cols as u16, rows as u16);
    let cards = cards(screen);
    let shape = |r: Region| match r.w / r.h > aspect {
        true => (r.w.min(cols as f32), (r.w / aspect).min(rows as f32)),
        false => ((r.h * aspect).min(cols as f32), r.h.min(rows as f32)),
    };
    // (each whole-cell placement holding a stretch `from..from + len` in `extent`, nearest its centre first)
    let starts = |from: f32, len: f32, extent: f32, n: usize| -> Vec<f32> {
        let lo = (from + len - extent).max(0.0).ceil() as i64;
        let hi = from.min(n as f32 - extent).floor() as i64;
        let centre = from + len / 2.0 - extent / 2.0;
        let mut all: Vec<f32> = (lo..=hi.max(lo)).map(|s| s as f32).collect();
        all.sort_by(|a, b| (a - centre).abs().total_cmp(&(b - centre).abs()));
        all
    };
    let mut want = target;
    for _ in 0..4 {
        let (w, h) = shape(want);
        let cuts = |x: f32, y: f32| -> Option<Card> {
            let (x1, y1) = (x + w, y + h);
            cards.iter().copied().find(|&(r0, c0, r1, c1)| {
                let (cr0, cc0, cr1, cc1) = (r0 as f32, c0 as f32, r1 as f32 + 1.0, c1 as f32 + 1.0);
                let meets = cc0 < x1 && x < cc1 && cr0 < y1 && y < cr1;
                let whole = x <= cc0 && cc1 <= x1 && y <= cr0 && cr1 <= y1;
                meets && !whole
            })
        };
        let (xs, ys) = (starts(want.x, want.w, w, cols), starts(want.y, want.h, h, rows));
        let mut cut = None;
        for &y in &ys {
            for &x in &xs {
                match cuts(x, y) {
                    None => {
                        let window = Region { x, y, w, h }.inside(size);
                        return (window, window);
                    }
                    Some(c) => cut = cut.or(Some(c)),
                }
            }
        }
        let Some((r0, c0, r1, c1)) = cut else { break };
        let (x0, y0) = (want.x.min(c0 as f32), want.y.min(r0 as f32));
        let (x1, y1) = ((want.x + want.w).max(c1 as f32 + 1.0), (want.y + want.h).max(r1 as f32 + 1.0));
        let more = Region { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
        if shape(more).0 > shape(target).0 * TAKEN_IN_MAX {
            break;
        }
        want = more;
    }
    let (w, h) = shape(target);
    let x = starts(target.x, target.w, w, cols).first().copied().unwrap_or(target.x);
    let y = starts(target.y, target.h, h, rows).first().copied().unwrap_or(target.y);
    (Region { x, y, w, h }.inside(size), target)
}

/// The cards on a screen: every frame drawn with `╭╮╰╯`, and every borderless one — a stretch of cells on a ground
/// of its own (not the screen's commonest), as near a rectangle as a toast's or a peek's is, narrower than three
/// quarters of the screen (a cursor's line is a row across the code), never the last row (the status line's).
fn cards(screen: &Cells) -> Vec<Card> {
    let mut out = frames(&screen.text);
    let g = &screen.ground;
    let rows = g.len();
    let cols = g.first().map_or(0, Vec::len);
    let mut count: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    g.iter().flatten().for_each(|v| *count.entry(*v).or_default() += 1);
    let Some(common) = count.into_iter().max_by_key(|e| (e.1, e.0)).map(|e| e.0) else { return out };
    let mut seen = vec![vec![false; cols]; rows.saturating_sub(1)];
    for r in 0..rows.saturating_sub(1) {
        for c in 0..cols {
            if seen[r][c] || g[r][c] == common {
                continue;
            }
            // (its cells of that ground, joined side by side and one above another)
            let (mut stack, mut n, mut b) = (vec![(r, c)], 0usize, (r, c, r, c));
            seen[r][c] = true;
            while let Some((y, x)) = stack.pop() {
                n += 1;
                b = (b.0.min(y), b.1.min(x), b.2.max(y), b.3.max(x));
                let near = [(y.wrapping_sub(1), x), (y + 1, x), (y, x.wrapping_sub(1)), (y, x + 1)];
                for (ny, nx) in near {
                    if ny < rows - 1 && nx < cols && !seen[ny][nx] && g[ny][nx] == g[r][c] {
                        seen[ny][nx] = true;
                        stack.push((ny, nx));
                    }
                }
            }
            let (w, h) = (b.3 - b.1 + 1, b.2 - b.0 + 1);
            if w >= 3 && w * 4 <= cols * 3 && n * 5 >= w * h * 4 {
                out.push(b);
            }
        }
    }
    out
}

/// Every frame drawn with `╭╮╰╯` (or `╭╮` and two sides ending together, a foot of air): its corners' rows and
/// columns (top, left, bottom, right — inclusive).
fn frames(cells: &[Vec<String>]) -> Vec<Card> {
    let at = |r: usize, c: usize| cells.get(r).and_then(|row| row.get(c)).map_or("", String::as_str);
    let mut out = Vec::new();
    for (r0, row) in cells.iter().enumerate() {
        for c0 in (0..row.len()).filter(|&c| row[c] == "╭") {
            let Some(c1) = (c0 + 1..row.len()).find(|&c| row[c] == "╮") else { continue };
            let side =
                |c: usize| (r0 + 1..cells.len()).take_while(|&r| matches!(at(r, c), "│" | "├" | "┤")).last();
            let below = side(c0).map_or(r0 + 1, |r| r + 1);
            if at(below, c0) == "╰" && at(below, c1) == "╯" {
                out.push((r0, c0, below, c1));
            } else if let Some(last) = side(c0).filter(|&r| side(c1) == Some(r)) {
                // (a card standing on the status line, its foot air — a prompt's: its two sides ending together)
                out.push((r0, c0, last, c1));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (u16, u16) = (80, 43);

    fn region(x: f32, y: f32, w: f32, h: f32) -> Region {
        Region { x, y, w, h }
    }

    /// The camera's path, a frame at a time: the whole screen before any move; a move eased out over `EASE_MS` (more
    /// than half the way in its first quarter, there at its end, never past it); held; a move begun mid-way through
    /// another starting from where the camera was — no frame jumps more than a move's first step; a target past the
    /// screen's edge moved back inside; and each frame's crop of the picture inside it.
    #[test]
    fn the_camera_eases_to_each_target_and_never_passes_the_frame() {
        let card = region(40.0, 30.0, 40.0, 10.0);
        let list = region(10.0, 8.0, 50.0, 12.0);
        // (a target past the right and bottom edges: moved back inside)
        let over = region(70.0, 40.0, 30.0, 8.0);
        let moves = [(1_000, card), (3_000, list), (3_200, over)];
        let frames: Vec<Region> = (0..5_000u64).step_by(33).map(|ms| at(&moves, SCREEN, ms)).collect();
        assert_eq!(at(&moves, SCREEN, 0), Region::all(SCREEN));
        assert_eq!(at(&moves, SCREEN, 999), Region::all(SCREEN));
        assert_eq!(at(&moves, SCREEN, 1_000 + EASE_MS), card);
        assert_eq!(at(&moves, SCREEN, 2_999), card, "held until the next move");
        let quarter = at(&moves, SCREEN, 1_000 + EASE_MS / 4);
        assert!((80.0 - quarter.w) / (80.0 - card.w) > 0.5, "not an ease-out: {quarter:?}");
        // (interrupted at 3.2 s: the third move starts where the second had got to)
        let mid = at(&moves, SCREEN, 3_200);
        assert!(mid != card && mid != list, "{mid:?}");
        let last = at(&moves, SCREEN, 3_200 + EASE_MS);
        assert_eq!(last, region(50.0, 35.0, 30.0, 8.0), "the target moved back inside the screen");
        for pair in frames.windows(2) {
            let step = (pair[1].x - pair[0].x).abs().max((pair[1].w - pair[0].w).abs());
            assert!(step < 12.0, "a jump: {pair:?}");
        }
        let cell = |px: f32| ((px * 0.6) as usize, (px * 1.45) as usize);
        let view = (882, 1186);
        for r in &frames {
            assert!(
                r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= 80.0 && r.y + r.h <= 43.0,
                "past the screen: {r:?}"
            );
            let px = fit(*r, view, 64, cell);
            let (cw, ch) = cell(px);
            let pic = (80 * cw + 28, 43 * ch + 28);
            let (x, y) = crop(*r, (cw, ch), 14, pic, view);
            for (o, pic, view) in [(x, pic.0, view.0), (y, pic.1, view.1)] {
                match pic >= view {
                    true => assert!(o >= 0 && o as usize + view <= pic, "the crop passes the picture: {r:?}"),
                    false => assert_eq!(o, (pic as i64 - view as i64) / 2),
                }
            }
            // (the region whole in the window)
            let shown = |start: f32, len: f32, cell: usize, o: i64, view: usize| {
                let (a, b) = (14.0 + start * cell as f32, 14.0 + (start + len) * cell as f32);
                a >= o as f32 - 0.5 && b <= (o + view as i64) as f32 + 0.5
            };
            assert!(shown(r.x, r.w, cw, x, view.0) && shown(r.y, r.h, ch, y, view.1), "cut: {r:?} at {px}");
        }
        // (whole screen at 80×43: 19 px glyphs fit 882 px across)
        assert_eq!(fit(Region::all(SCREEN), view, 64, cell), 19.0);
    }

    /// A screen of `rows`, every cell on one ground.
    fn grid(rows: &[&str]) -> Cells {
        let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        let cells = |r: &&str| {
            let mut v: Vec<String> = r.chars().map(String::from).collect();
            v.resize(w, " ".into());
            v
        };
        Cells { text: rows.iter().map(cells).collect(), ground: vec![vec![0; w]; rows.len()] }
    }

    /// A blank `cols`×`rows` screen with each of `cards` drawn as a frame.
    fn boxed(cols: usize, rows: usize, cards: &[Card]) -> Cells {
        let mut text = vec![vec![" ".to_string(); cols]; rows];
        for &(r0, c0, r1, c1) in cards {
            for r in [r0, r1] {
                text[r][c0..=c1].fill("─".into());
            }
            for row in &mut text[r0..=r1] {
                (row[c0], row[c1]) = ("│".into(), "│".into());
            }
            (text[r0][c0], text[r0][c1], text[r1][c0], text[r1][c1]) =
                ("╭".into(), "╮".into(), "╰".into(), "╯".into());
        }
        Cells { text, ground: vec![vec![0; cols]; rows] }
    }

    /// A target shaped to the window: the rows it adds slid off a list above it, so the list isn't cut — the placement
    /// nearest its centre that cuts nothing, all of it drawn; where every placement cuts a list close by, the list taken
    /// in whole; where taking it in would cost the zoom past `TAKEN_IN_MAX`, the target drawn alone.
    #[test]
    fn a_framed_target_never_cuts_a_card() {
        let card: Card = (12, 20, 15, 39);
        let target = Region { x: 18.0, y: 11.0, w: 22.0, h: 6.0 };
        let whole = |r: Region| (r, r);
        // (a window of 22 columns is 11 rows at 2 columns a row: rows 8–18 would cut the list's last row)
        let on = boxed(40, 20, &[(5, 10, 8, 30), card]);
        assert_eq!(framed(&on, target, 2.0), whole(region(18.0, 9.0, 22.0, 11.0)));
        // (every placement cuts a list just left of the card's columns: 2 more columns take it in)
        let on = boxed(40, 20, &[(7, 16, 10, 30), card]);
        assert_eq!(framed(&on, target, 2.0), whole(region(16.0, 6.0, 24.0, 12.0)));
        // (…a list reaching far left: 30 columns, past 1.25 × 22 — the card alone, drawn in its own rows)
        let on = boxed(40, 20, &[(7, 10, 10, 30), card]);
        assert_eq!(framed(&on, target, 2.0), (region(18.0, 8.0, 22.0, 11.0), target));
        // (nothing to cut: centred on the target)
        let on = boxed(40, 20, &[card]);
        assert_eq!(framed(&on, target, 2.0), whole(region(18.0, 8.0, 22.0, 11.0)));
    }

    /// Each kind of target found on a screen: a card by its title or body (the smallest frame holding it), a toast by
    /// its ground, the code's lines by their numbers (as wide as the longest, not the side panel's rows), the status
    /// line, the strip, text no card holds by its row's stretch; and none for what isn't there.
    #[test]
    fn each_target_is_found_on_the_screen() {
        let mut cells = grid(&[
            "─ demo › main.rs ──────────── main · fix ▲ ─",
            "src/a  │ 1 fn main() {",
            "  b.rs │ 2     let x = 1;",
            "       │ 3 }",
            "       │   ╭─ main asks ──────╮",
            "       │   │ main moved      │",
            "       │   ╰─ y move · n ─────╯",
            " yanked 2 lines · a toast",
            "● normal  main.rs 2:1",
            "       ╭─ command ──╮",
            "       │ :sh git    │",
            "● normal",
        ]);
        // (the toast's ground, from column 1 to 23 of row 7)
        cells.ground[7][1..24].fill(5);
        // (the card: its frame from row 4 to 6, columns 11 to 30, a cell of air round it)
        let card = Some(region(10.0, 3.0, 22.0, 5.0));
        assert_eq!(target(&cells, "card:main moved"), card);
        assert_eq!(target(&cells, "card:main asks"), card);
        assert_eq!(target(&cells, "code:1-2"), Some(region(8.0, 0.0, 18.0, 4.0)));
        // (the last row: its air above it, none below the screen)
        assert_eq!(target(&cells, "status"), Some(region(0.0, 9.0, 10.0, 3.0)));
        assert_eq!(target(&cells, "strip"), Some(region(29.0, 0.0, 14.0, 3.0)));
        assert_eq!(target(&cells, "yanked"), Some(region(0.0, 6.0, 25.0, 3.0)));
        assert_eq!(target(&cells, "fn main"), Some(region(8.0, 0.0, 15.0, 3.0)));
        assert_eq!(target(&cells, "all"), Some(Region::all((cells.text[0].len() as u16, 12))));
        // (a prompt's card, its foot air on the status line)
        assert_eq!(target(&cells, "card:command"), Some(region(6.0, 8.0, 16.0, 4.0)));
        assert_eq!(target(&cells, "card:nowhere"), None);
        assert_eq!(target(&cells, "code:40-50"), None);
    }
}
