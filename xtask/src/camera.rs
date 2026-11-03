//! A Short's camera (`scenes --video --shorts`, `video.rs`): the scene's `@zoom:<target>` lines, each found on the
//! screen in force at its time (`target`) and eased to (`at`: `EASE_MS`, an ease-out — the motion rule's), held until
//! the next `@zoom` or beat (a beat with none of its own goes back to the whole screen). Where the camera is is a
//! region of the screen in cells; a frame draws the screen with glyphs at the largest whole size the region fits the
//! window at (`fit`) and crops it there (`crop`) — never past the picture's edge, the region always whole in it. A
//! target's region is shaped to the window as it's found (`framed`), the target's centre at the eye's height: the rows
//! or columns it adds put where they cut no card (a frame, or a borderless one by its ground — `cards`), else the cut
//! card taken in whole, else the window as the target has it — and on every frame a card the window cuts, one that
//! came while a zoom holds too, is left out on the window's ground (`cut`), a row of text the window's edge cuts drawn
//! cut there as a zoomed video crops (`Window`).
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
pub(crate) type Card = (usize, usize, usize, usize);

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

/// `first` (a target's region where the camera found it) as wide as `later` finds it on the same rows while it holds —
/// the columns of each later region whose rows meet `first`'s (a line being written reaching its end; the same lines
/// moved elsewhere by a scroll aren't it) —, its rows as they were. Inside the screen.
pub(crate) fn widened(first: Region, later: impl Iterator<Item = Region>, screen: (u16, u16)) -> Region {
    let (mut x0, mut x1) = (first.x, first.x + first.w);
    for r in later.filter(|r| r.y < first.y + first.h && first.y < r.y + r.h) {
        (x0, x1) = (x0.min(r.x), x1.max(r.x + r.w));
    }
    Region { x: x0, w: x1 - x0, ..first }.inside(screen)
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
/// - `code:A-B` the code's lines A to B by the gutter's numbers, as wide as the longest of them (a card over their rows
///   no part of them);
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
                let divs = dividers(cells);
                // (a line's text ends before any card drawn over its row: the follow card's words are no code)
                let held = cards(on);
                let lines: Vec<(usize, usize, usize)> = (0..rows)
                    .filter_map(|r| {
                        let (n, from, end) = numbered(&cells[r], &divs)?;
                        let carded =
                            |c: usize| held.iter().any(|k| k.0 <= r && r <= k.2 && k.1 <= c && c <= k.3);
                        let end = (from..end)
                            .rev()
                            .find(|&c| !blank(&cells[r][c]) && !carded(c))
                            .map_or(end, |c| c + 1);
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

/// The screen's pane dividers: each column where a line's `┬` (or `┼`) has a `│` under it — a divider starts at a
/// top line; an indent guide's `│` never does.
fn dividers(cells: &[Vec<String>]) -> Vec<usize> {
    let at = |r: usize, c: usize| cells.get(r).and_then(|row| row.get(c)).map_or("", String::as_str);
    let mut out: Vec<usize> = (0..cells.len())
        .flat_map(|r| (0..cells[r].len()).map(move |c| (r, c)))
        .filter(|&(r, c)| matches!(at(r, c), "┬" | "┼") && matches!(at(r + 1, c), "│" | "├" | "┤" | "┼"))
        .map(|(_, c)| c)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// A code row's gutter number, where it starts and where the row's text ends inside its pane: in the first pane of
/// the row (its stretches between the screen's `dividers`) whose first letter or digit starts a run of digits and a
/// blank — only signs, lines and blanks before it (a side panel's stretch starts with its names) —, the pane's end its
/// next divider.
fn numbered(row: &[String], dividers: &[usize]) -> Option<(u32, usize, usize)> {
    // (only where this row has the divider's line: a toast or a card drawn over it is one stretch)
    let lines: Vec<usize> = (dividers.iter().copied())
        .filter(|&d| row.get(d).is_some_and(|s| matches!(s.as_str(), "│" | "├" | "┤" | "┼" | "┬" | "┴")))
        .collect();
    let starts = std::iter::once(0).chain(lines.iter().map(|&c| c + 1));
    starts.into_iter().find_map(|from| {
        let pane = lines.iter().copied().find(|&c| c >= from).unwrap_or(row.len());
        let first = (from..pane).find(|&c| row[c].chars().next().is_some_and(|ch| ch.is_alphanumeric()))?;
        let digit = |c: usize| !row[c].is_empty() && row[c].chars().all(|ch| ch.is_ascii_digit());
        let end = (first..pane).find(|&c| !digit(c))?;
        // (a blank after the number, or a bar — the follow caret `▎` where the line's text starts)
        let bar = row[end].chars().next().is_some_and(|ch| ('\u{2500}'..='\u{259f}').contains(&ch));
        if end == first || !row[end].trim().is_empty() && !bar {
            return None;
        }
        let n: u32 = row[first..end].concat().parse().ok()?;
        // (the line's own text ends at a gap of three blanks: what stands past it — blame, a diagnostic, an answer's `¶`
        // — is drawn there, not written)
        // (from its first character past the indent and its guides)
        let start = (end..pane)
            .find(|&c| {
                row[c].chars().next().is_some_and(|ch| !ch.is_whitespace() && !matches!(ch, '│' | '▎'))
            })
            .unwrap_or(end);
        let pane = (start..pane)
            .find(|&c| (c..c + 3).all(|g| row.get(g).is_some_and(|s| s.trim().is_empty())) && c + 3 <= pane)
            .unwrap_or(pane);
        // (the code's scrollbar thumb in the screen's last column is no text of the line's)
        let pane = match pane == row.len() && row.last().is_some_and(|c| c == "▎") {
            true => pane - 1,
            false => pane,
        };
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

/// `target`'s region shaped to a window of `aspect` columns a row, the screen round it filling the window: the rows (or
/// columns) the shape adds go where no card is cut by the window's edge — of those placements the one that puts the
/// target's centre nearest `eye` down the window (a fraction of its height) and its middle across; where every
/// placement cuts one, that card is taken in with the target and shaped again while the window grows no more than
/// `TAKEN_IN_MAX` nor past `legible` columns (what it shows at a phone's legible glyph — a target wider than that
/// takes nothing in; at most four times); else the target's own shape placed so — the cards it cuts left out as each
/// frame is drawn (`cut`), the zoom kept. Inside the screen.
pub(crate) fn framed(screen: &Cells, target: Region, aspect: f32, eye: f32, legible: f32) -> Region {
    let rows = screen.text.len();
    let cols = screen.text.first().map_or(0, Vec::len);
    let size = (cols as u16, rows as u16);
    let cards = cards(screen);
    let shape = |r: Region| match r.w / r.h > aspect {
        true => (r.w.min(cols as f32), (r.w / aspect).min(rows as f32)),
        false => ((r.h * aspect).min(cols as f32), r.h.min(rows as f32)),
    };
    // (each whole-cell placement holding a stretch `from..from + len` in `extent`, the ones putting its centre nearest
    // `at` of the extent first)
    let starts = |from: f32, len: f32, extent: f32, n: usize, at: f32| -> Vec<f32> {
        let lo = (from + len - extent).max(0.0).ceil() as i64;
        let hi = from.min(n as f32 - extent).floor() as i64;
        let centre = from + len / 2.0 - extent * at;
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
        let (xs, ys) = (starts(want.x, want.w, w, cols, 0.5), starts(want.y, want.h, h, rows, eye));
        let mut cut = None;
        for &y in &ys {
            for &x in &xs {
                match cuts(x, y) {
                    None => return Region { x, y, w, h }.inside(size),
                    Some(c) => cut = cut.or(Some(c)),
                }
            }
        }
        let Some((r0, c0, r1, c1)) = cut else { break };
        let (x0, y0) = (want.x.min(c0 as f32), want.y.min(r0 as f32));
        let (x1, y1) = ((want.x + want.w).max(c1 as f32 + 1.0), (want.y + want.h).max(r1 as f32 + 1.0));
        let more = Region { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
        let own = shape(target).0;
        if shape(more).0 > (own * TAKEN_IN_MAX).min(legible.max(own)) {
            break;
        }
        want = more;
    }
    let (w, h) = shape(target);
    let x = starts(target.x, target.w, w, cols, 0.5).first().copied().unwrap_or(target.x);
    let y = starts(target.y, target.h, h, rows, eye).first().copied().unwrap_or(target.y);
    Region { x, y, w, h }.inside(size)
}

/// The cards a window of whole cells (columns `c0..c1`, rows `r0..r1`) cuts: each meeting it but not whole inside it,
/// but the one holding `target` (inside its air) — the card the target is is shown even cut: `framed` kept it whole where
/// it could; one that only meets it (an answer's card opened under the lines in sight) is left out. A Short's frame leaves them out on the window's ground, so nothing is ever cut at its edge — a card that
/// came while a zoom holds (a toast) too.
fn cut(screen: &Cells, (c0, r0, c1, r1): (usize, usize, usize, usize), target: Region) -> Vec<Card> {
    let (tx0, ty0) = (target.x + AIR, target.y + AIR);
    let (tx1, ty1) = (target.x + target.w - AIR, target.y + target.h - AIR);
    cards(screen)
        .into_iter()
        .filter(|&(cr0, cc0, cr1, cc1)| {
            let meets = cc0 < c1 && c0 <= cc1 && cr0 < r1 && r0 <= cr1;
            let whole = c0 <= cc0 && cc1 < c1 && r0 <= cr0 && cr1 < r1;
            let (x0, y0, x1, y1) = (cc0 as f32, cr0 as f32, cc1 as f32 + 1.0, cr1 as f32 + 1.0);
            let target_in = x0 <= tx0 && tx1 <= x1 && y0 <= ty0 && ty1 <= y1;
            meets && !whole && !target_in
        })
        .collect()
}

/// The window's left or right edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Edge {
    Left,
    Right,
}

/// What a Short's frame draws of the screen in a window of whole cells (columns `c0..c1`, rows `r0..r1`): every cell
/// but a card the window cuts (`left_out`, `cut`'s), laid with the window's ground; a row whose text runs past the
/// window's left or right edge drawn cut there as a zoomed video crops (`fade`: its last cells easing into their
/// ground). A row is drawn whole or not at all — what's drawn belongs to a row that's drawn: never its ground or its
/// blame without its code.
pub(crate) struct Window {
    pub(crate) left_out: Vec<Card>,
    pub(crate) fade: Vec<(usize, Edge)>,
}

impl Window {
    pub(crate) fn of(screen: &Cells, view: (usize, usize, usize, usize), target: Region) -> Window {
        let all = cards(screen);
        Window { left_out: cut(screen, view, target), fade: runs_past(screen, view, &all) }
    }

    /// Whether the frame draws the screen's cell at row `r`, column `c` (inside the window).
    pub(crate) fn drawn(&self, r: usize, c: usize) -> bool {
        !self.left_out.iter().any(|b| b.0 <= r && r <= b.2 && b.1 <= c && c <= b.3)
    }
}

/// The rows whose text the window's left or right edge cuts (row, edge): an inked cell on each side of it (a wide
/// character's second cell counts as inked), neither in a card — a card the window cuts is left out whole, and one
/// beside the window is no text of the row's. A line (`─`, `│`, `▎`…) runs on cut as it is.
fn runs_past(
    screen: &Cells,
    (c0, r0, c1, r1): (usize, usize, usize, usize),
    cards: &[Card],
) -> Vec<(usize, Edge)> {
    let mut out = Vec::new();
    for (r, row) in screen.text.iter().enumerate().take(r1).skip(r0) {
        let inked = |c: usize| match row.get(c).and_then(|s| s.chars().next()) {
            None => c > 0 && row.get(c - 1).and_then(|s| s.chars().next()).is_some_and(wide),
            Some(ch) => !ch.is_whitespace() && !('\u{2500}'..='\u{259f}').contains(&ch),
        };
        let carded = |c: usize| cards.iter().any(|b| b.0 <= r && r <= b.2 && b.1 <= c && c <= b.3);
        let across = |inside: usize, outside: usize| {
            inked(inside) && inked(outside) && !carded(inside) && !carded(outside)
        };
        if c0 > 0 && c0 < c1 && across(c0, c0 - 1) {
            out.push((r, Edge::Left));
        }
        if c1 < row.len() && c1 > c0 && across(c1 - 1, c1) {
            out.push((r, Edge::Right));
        }
    }
    out
}

/// A character drawn two cells wide (Hangul, CJK, full-width forms, emoji): the cell after it is empty.
fn wide(ch: char) -> bool {
    matches!(ch, '\u{1100}'..='\u{115f}' | '\u{2e80}'..='\u{a4cf}' | '\u{ac00}'..='\u{d7a3}' | '\u{f900}'..='\u{faff}'
        | '\u{fe30}'..='\u{fe4f}' | '\u{ff00}'..='\u{ff60}' | '\u{ffe0}'..='\u{ffe6}' | '\u{1f300}'..='\u{1faff}')
}

/// Each row's code: its text from the column the code starts in (past the gutter's numbers — the commonest such column
/// of the rows `numbered` reads, so a row an edit review proposes, which has no number, is read too) to its pane's end,
/// cells inside a card blank and the follow caret's `▎` left out; `None` for the top line, the status line, and every
/// row where no row has a number. What Claude's writing newly shows is read from it (`video.rs`).
pub(crate) fn code_rows(screen: &Cells) -> Vec<Option<String>> {
    let rows = screen.text.len();
    let divs = dividers(&screen.text);
    let mut starts: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for row in screen.text.iter().take(rows.saturating_sub(1)).skip(1) {
        let Some((_, first, end)) = numbered(row, &divs) else { continue };
        if let Some(c) = (first..end).find(|&c| !row[c].chars().all(|ch| ch.is_ascii_digit())) {
            *starts.entry(c).or_default() += 1;
        }
    }
    let Some(col) = starts.into_iter().max_by_key(|e| (e.1, e.0)).map(|e| e.0) else {
        return vec![None; rows];
    };
    let cards = cards(screen);
    (0..rows)
        .map(|r| {
            if r == 0 || r + 1 == rows {
                return None;
            }
            let row = &screen.text[r];
            let end = divs.iter().copied().find(|&c| c >= col).unwrap_or(row.len()).min(row.len());
            let carded = |c: usize| cards.iter().any(|b| b.0 <= r && r <= b.2 && b.1 <= c && c <= b.3);
            let text: String = (col..end)
                .map(|c| if carded(c) { " " } else { row[c].as_str() })
                .collect::<String>()
                .replace('▎', "");
            Some(text.trim_end().to_string())
        })
        .collect()
}

/// The cards on a screen (`video.rs` reads them too): every frame drawn with `╭╮╰╯`, and every borderless one — a
/// stretch of cells on a ground of its own (not the screen's commonest), as near a rectangle as a toast's or a peek's is, its first cell the kind's
/// `▎` as a toast's and a peek's are, narrower than three quarters of the screen (a cursor's line is a row across the
/// code), never the last row (the status line's) — and never the code's own rows (a wash on the lines Claude read or
/// changed: a gutter's number inside it), nor a row inside a frame (a list's selected row: the frame's own).
pub(crate) fn cards(screen: &Cells) -> Vec<Card> {
    let mut out = frames(&screen.text);
    let framed = out.clone();
    let inside = |b: Card| framed.iter().any(|f| f.0 <= b.0 && b.2 <= f.2 && f.1 <= b.1 && b.3 <= f.3);
    let divs = dividers(&screen.text);
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
            let marked = screen.text[b.0][b.1] == "▎";
            let code = (b.0..=b.2).any(|r| {
                numbered(&screen.text[r], &divs).is_some_and(|(_, at, _)| (b.1..=b.3).contains(&at))
            });
            if w >= 3 && w * 4 <= cols * 3 && n * 5 >= w * h * 4 && marked && !code && !inside(b) {
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
    /// nearest the eye's that cuts nothing; where every placement cuts a list close by, the list taken in whole; where
    /// taking it in would cost the zoom past `TAKEN_IN_MAX`, the window the target's own, the list it cuts left out
    /// (`cut`) — never the card the target is; and with room all round, the target's centre at the eye's height.
    #[test]
    fn a_framed_target_never_cuts_a_card() {
        let card: Card = (12, 20, 15, 39);
        let target = Region { x: 18.0, y: 11.0, w: 22.0, h: 6.0 };
        // (a window of 22 columns is 11 rows at 2 columns a row: rows 8–18 would cut the list's last row)
        let on = boxed(40, 20, &[(5, 10, 8, 30), card]);
        assert_eq!(framed(&on, target, 2.0, 0.5, 80.0), region(18.0, 9.0, 22.0, 11.0));
        // (every placement cuts a list just left of the card's columns: 2 more columns take it in)
        let on = boxed(40, 20, &[(7, 16, 10, 30), card]);
        assert_eq!(framed(&on, target, 2.0, 0.5, 80.0), region(16.0, 6.0, 24.0, 12.0));
        // (…unless those 2 columns are past what the window shows legibly: the target's own window)
        assert_eq!(framed(&on, target, 2.0, 0.5, 23.0), region(18.0, 8.0, 22.0, 11.0));
        // (…a list reaching far left: 30 columns, past 1.25 × 22 — the target's own window, the list it cuts left out
        // as the frame is drawn, the card itself never)
        let on = boxed(40, 20, &[(7, 10, 10, 30), card]);
        let window = framed(&on, target, 2.0, 0.5, 80.0);
        assert_eq!(window, region(18.0, 8.0, 22.0, 11.0));
        let cells = (18, 8, 40, 19);
        assert_eq!(cut(&on, cells, target), vec![(7, 10, 10, 30)]);
        // (nothing to cut: the target's centre at the eye's height — half way, then a fifth of the way down)
        let on = boxed(40, 30, &[card]);
        assert_eq!(framed(&on, target, 2.0, 0.5, 80.0), region(18.0, 8.0, 22.0, 11.0));
        // (a fifth of the way down is above where the target fits whole: as near as it can, its top the window's)
        assert_eq!(framed(&on, target, 2.0, 0.2, 80.0), region(18.0, 11.0, 22.0, 11.0));
        // (a target of one row and its air: its centre (row 12) 2.2 rows down a window of 11)
        let row = Region { h: 2.0, ..target };
        assert_eq!(framed(&on, row, 2.0, 0.2, 80.0), region(18.0, 10.0, 22.0, 11.0));
    }

    /// What a window cuts, on every frame: a card meeting it and not whole inside (one that came while a zoom holds,
    /// a toast at its edge); never a card whole inside it, one apart from it, nor the one the target is in.
    #[test]
    fn a_card_the_window_cuts_is_left_out() {
        let toast: Card = (2, 30, 4, 45);
        let list: Card = (10, 5, 14, 25);
        let far: Card = (20, 30, 24, 39);
        let on = boxed(50, 30, &[toast, list, far]);
        // (the window: columns 0..40, rows 0..18 — the toast past its right edge, the list inside, the far one apart)
        let target = region(4.0, 9.0, 23.0, 7.0);
        assert_eq!(cut(&on, (0, 0, 40, 18), target), vec![toast]);
        // (the target in the toast: shown, even cut)
        assert_eq!(cut(&on, (0, 0, 40, 18), region(31.0, 2.0, 10.0, 3.0)), Vec::<Card>::new());
        // (…a target the toast only meets, as an answer's card opened under the lines in sight: left out)
        assert_eq!(cut(&on, (0, 0, 40, 18), region(20.0, 1.0, 20.0, 3.0)), vec![toast]);
        // (the list cut by the window's foot)
        assert_eq!(cut(&on, (0, 0, 50, 12), region(30.0, 1.0, 17.0, 5.0)), vec![list]);
    }

    /// A target held is as wide as it grows on its rows (a line written past where it was found), its rows as they
    /// were; a region elsewhere (the lines scrolled away) changes nothing.
    #[test]
    fn a_held_target_is_as_wide_as_it_grows() {
        let first = region(2.0, 10.0, 20.0, 5.0);
        let grown = region(2.0, 11.0, 31.0, 5.0);
        let away = region(0.0, 30.0, 70.0, 5.0);
        assert_eq!(widened(first, [grown, away].into_iter(), SCREEN), region(2.0, 10.0, 31.0, 5.0));
        assert_eq!(widened(first, [away].into_iter(), SCREEN), first);
    }

    /// A borderless card is a toast's or a peek's: a ground of its own starting with the kind's `▎` — never a ground
    /// without it (a selection, a match), nor a wash on the code's own numbered rows, `▎` sign and all, nor a list's
    /// selected row inside its frame (V2's session list: left out alone where the window cut the list it's in).
    #[test]
    fn a_borderless_card_starts_with_its_mark() {
        let mut cells = grid(&[
            "▎ saved · a toast  ",
            "  a selection here ",
            "▎ 3 let x = 1;     ",
            "▎ 4▎let y = 2;     ",
            "                   ",
            "╭─ sessions ─╮",
            "│▎ main  2   │",
            "╰────────────╯",
            "",
        ]);
        cells.ground[0][0..12].fill(5);
        cells.ground[1][2..13].fill(6);
        cells.ground[2][0..14].fill(7);
        // (a line just written: the follow caret right after its number, its ink drying on a ground)
        cells.ground[3][0..14].fill(8);
        cells.ground[6][1..13].fill(9);
        assert_eq!(cards(&cells), vec![(5, 0, 7, 13), (0, 0, 0, 11)]);
    }

    /// A row whose text the window's edge cuts is drawn cut there, fading at that edge: text on both sides of it, either
    /// edge; a line running on, a blank on either side or a card there fades nothing. A wide character's second cell is
    /// inked.
    #[test]
    fn a_row_cut_at_the_edge_fades_there() {
        let mut cells = grid(&[
            "29 fn wind(m: &mut Map, t: Thread) {",
            "── demo › main.rs ─────────────────",
            "30     let k = t.name.clone();",
            "ab  cd           x  y",
            "abcdef                        ",
            "실실",
            "letters",
        ]);
        // (a wide character's second cell is empty, as vt100 leaves it)
        cells.text[5][..4].clone_from_slice(&["실", "", "실", ""].map(String::from));
        let card: Card = (4, 0, 4, 2);
        let fade = runs_past(&cells, (3, 0, 20, 7), &[card]);
        let at = |r: usize| fade.iter().filter(|f| f.0 == r).map(|f| f.1).collect::<Vec<_>>();
        assert_eq!(
            at(0),
            [Edge::Right],
            "`fn` starts at the left edge, a blank past it; `Map` crosses the right"
        );
        assert_eq!(at(1), [], "a line runs on cut as it is");
        assert_eq!(at(2), [Edge::Right], "the indent's blanks at the left edge: nothing there");
        assert_eq!(at(3), [], "a blank on the inner side of either edge");
        assert_eq!(at(4), [], "a card past the left edge: none of the row's text");
        assert_eq!(at(5), [Edge::Left], "Hangul: the edge between a wide character's two cells");
        assert_eq!(at(6), [Edge::Left], "`letters` crosses the left edge");
    }

    /// A row is drawn whole or not at all: V2's line under the cursor, its code cut at the window's left edge and
    /// blame at its end on the cursor line's ground, is every cell drawn (round 2 hid its code and left the ground and
    /// blame standing alone); a toast the right edge cuts is left out, the rest of its row drawn.
    #[test]
    fn a_row_is_drawn_whole_or_not_at_all() {
        let toast = format!("{:<40}▎ saved     ", "32 }");
        let mut cells = grid(&["31     m.insert(k, t); // main     not committed yet  ", &toast, "● normal"]);
        cells.ground[0][3..50].fill(9);
        cells.ground[1][40..52].fill(5);
        let view = (10, 0, 46, 2);
        let window = Window::of(&cells, view, region(10.0, 0.0, 20.0, 1.0));
        assert!((10..46).all(|c| window.drawn(0, c)), "the line under the cursor drawn whole");
        assert_eq!(window.left_out, vec![(1, 40, 1, 51)], "the toast past the right edge");
        assert!((10..40).all(|c| window.drawn(1, c)) && !(40..46).any(|c| window.drawn(1, c)));
        assert_eq!(window.fade, vec![(0, Edge::Left), (0, Edge::Right)]);
    }

    /// The code's rows as the writing is read from them: from the text's column past the numbers, a row with no number
    /// (an edit review's proposed one) read there too, a card's cells blank, the caret left out; the top and status
    /// lines none.
    #[test]
    fn the_codes_rows_are_read_past_the_numbers() {
        let cells = grid(&[
            "─ demo › main.rs ──",
            " 1 fn main() {",
            "│     let x = 1;▎",
            " 2 }      ╭─ a ─╮",
            "          │ b   │",
            "          ╰─────╯",
            "● normal",
        ]);
        let rows = code_rows(&cells);
        assert_eq!(rows[0], None);
        assert_eq!(rows[1].as_deref(), Some(" fn main() {"));
        assert_eq!(rows[2].as_deref(), Some("    let x = 1;"));
        assert_eq!(rows[3].as_deref(), Some(" }"));
        assert_eq!(rows[4].as_deref(), Some(""));
        assert_eq!(rows[6], None);
    }

    /// Each kind of target found on a screen: a card by its title or body (the smallest frame holding it), a toast by
    /// its ground, the code's lines by their numbers (as wide as the longest, not the side panel's rows), the status
    /// line, the strip, text no card holds by its row's stretch; and none for what isn't there.
    #[test]
    fn each_target_is_found_on_the_screen() {
        let mut cells = grid(&[
            "─ demo ┬ main.rs ──────────── main · fix ▲ ─",
            // (what's drawn past the line's end — blame, a `¶` — no part of it)
            "src/a  │ 1 fn main() {    demo, 1d ago",
            // (an indent guide's `│` down two rows of the code: no divider — no `┬` above it)
            "  b.rs │ 2 │   let x = 1;",
            "       │ 3 │",
            "       │ 4 ╭─ main asks ──────╮",
            "       │   │ main moved      │",
            "       │   ╰─ y move · n ─────╯",
            "▎yanked 2 lines · a toast",
            "● normal  main.rs 2:1",
            "       ╭─ command ──╮",
            "       │ :sh git    │",
            "● normal",
        ]);
        // (the toast's ground, its `▎`, its words and a cell of air: columns 0 to 25 of row 7)
        cells.ground[7][0..26].fill(5);
        // (the card: its frame from row 4 to 6, columns 11 to 30, a cell of air round it)
        let card = Some(region(10.0, 3.0, 22.0, 5.0));
        assert_eq!(target(&cells, "card:main moved"), card);
        assert_eq!(target(&cells, "card:main asks"), card);
        assert_eq!(target(&cells, "code:1-2"), Some(region(8.0, 0.0, 18.0, 4.0)));
        // (line 4's row under a card: the card's words are no part of the line)
        assert_eq!(target(&cells, "code:3-4"), Some(region(8.0, 2.0, 5.0, 4.0)));
        // (the code's scrollbar thumb in the screen's last column, line 1 running right up to it: no text of the line's)
        let kept = cells.text[1].clone();
        let last = kept.len() - 1;
        cells.text[1][22..last].fill("x".into());
        cells.text[1][last] = "▎".into();
        assert_eq!(target(&cells, "code:1-2"), Some(region(8.0, 0.0, (last - 9 + 2) as f32, 4.0)));
        cells.text[1] = kept;
        // (the last row: its air above it, none below the screen)
        assert_eq!(target(&cells, "status"), Some(region(0.0, 9.0, 10.0, 3.0)));
        assert_eq!(target(&cells, "strip"), Some(region(29.0, 0.0, 14.0, 3.0)));
        assert_eq!(target(&cells, "yanked"), Some(region(0.0, 6.0, 28.0, 3.0)));
        // (a stretch of ground with no `▎` is no card — a wash on the code's lines: the text's own stretch, not the
        // wash's columns 9–19)
        cells.ground[2][9..20].fill(6);
        assert_eq!(target(&cells, "let x"), Some(region(14.0, 1.0, 12.0, 3.0)));
        assert_eq!(target(&cells, "fn main"), Some(region(8.0, 0.0, 15.0, 3.0)));
        assert_eq!(target(&cells, "all"), Some(Region::all((cells.text[0].len() as u16, 12))));
        // (a prompt's card, its foot air on the status line)
        assert_eq!(target(&cells, "card:command"), Some(region(6.0, 8.0, 16.0, 4.0)));
        assert_eq!(target(&cells, "card:nowhere"), None);
        assert_eq!(target(&cells, "code:40-50"), None);
    }
}
