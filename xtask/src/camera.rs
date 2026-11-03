//! A Short's camera (`scenes --video --shorts`, `video.rs`): the scene's `@zoom:<target>` lines, each found on the
//! screen in force at its time (`target`) and eased to (`at`: `EASE_MS`, an ease-out — the motion rule's), held until
//! the next `@zoom` or beat (a beat with none of its own goes back to the whole screen). Where the camera is is a
//! region of the screen in cells; a frame draws the screen with glyphs at the largest whole size the region fits the
//! window at (`fit`) and crops it there (`crop`) — never past the picture's edge, the region always whole in it. A
//! target's region is shaped to the window as it's found (`framed`), the target's centre at the eye's height: the rows
//! or columns it adds put where they cut no card (a frame, or a borderless one by its ground — `cards`), else the cut
//! card taken in whole, else the window as the target has it — and on every frame a card the window cuts, one that
//! came while a zoom holds too, is left out on the window's ground (`cut`), a row of text the window's edge cuts drawn
//! cut there as a zoomed video crops (`Window`), a sliver of one the left edge leaves erased. A news beat
//! (`news:<text> over <target>`) frames what its toast is about and lays the toast over the window's top (`callout`).
//! A target that floats over the code (a list, a card, a prompt — `spotlit`) is a spotlight's: what's round it dimmed
//! toward the ground, eased in and out with the camera (`lit`); the code's targets and a news beat's toast stay bright.
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
///   or a toast's or a peek's ground), else its row's stretch of text (to two blanks or a line);
/// - any of them `|N`: at most `N` columns of it, air included, from its left — where tarae draws what the caption is
///   about across the screen's width (the ask line, a doc comment, an answer's card), the part it's about at a glyph a
///   phone reads; the rest cut at the window's edge, fading (`Window`), its card drawn cut;
/// - `news:<text> over <target>` the target's, while the card holding `<text>` (a toast) is on the screen too: a news
///   beat, the code its toast names framed, the toast laid over the window's top (`news_framed`, `callout`).
///
/// Each with a cell of air round it. None where it isn't on that screen.
pub(crate) fn target(on: &Cells, spec: &str) -> Option<Region> {
    if let Some((text, inner)) = news(spec) {
        holding(on, find(&on.text, text)?)?;
        return target(on, inner);
    }
    if let Some((spec, n)) = spec.rsplit_once('|').and_then(|(s, n)| Some((s, n.parse::<u16>().ok()?))) {
        let r = target(on, spec)?;
        return Some(Region { w: r.w.min(f32::from(n)), ..r });
    }
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
                Some(match holding(on, (r, c0, c1)) {
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

/// A news target's toast text and the target it's about (`news:<text> over <target>`).
pub(crate) fn news(spec: &str) -> Option<(&str, &str)> {
    spec.strip_prefix("news:")?.split_once(" over ")
}

/// What the spotlight lights on `on` for `spec`'s target: the card it floats in (a list, a card, a prompt, a toast or a
/// peek — `cards`) — a prompt's with the status line under it, where its keys are said (a card standing on it at the
/// screen's left, a cell in: its foot the row above or the line itself; the asks stand at the right). Nothing for the code's targets (`all`, `status`, `strip`, `code:`, a row's stretch of
/// text), a card standing between the code's lines (an answer's or a note's: rows of the document — `between`), a news
/// beat (its toast is laid over the window, bright, and what it's about is code), nor a card this screen doesn't draw.
pub(crate) fn spotlit(on: &Cells, spec: &str) -> Vec<Card> {
    let card = || {
        if news(spec).is_some() {
            return None;
        }
        let spec = spec.rsplit_once('|').filter(|(_, n)| n.parse::<u16>().is_ok()).map_or(spec, |(s, _)| s);
        if matches!(spec, "all" | "status" | "strip") || spec.starts_with("code:") {
            return None;
        }
        let text = spec.strip_prefix("card:").unwrap_or(spec);
        holding(on, find(&on.text, text)?).filter(|&b| !between(on, b))
    };
    let Some(b) = card() else { return Vec::new() };
    let rows = on.text.len();
    let standing = b.2 + 2 == rows && b.1 <= 1;
    match standing {
        true => vec![b, (rows - 1, 0, rows - 1, on.text[rows - 1].len().saturating_sub(1))],
        false => vec![b],
    }
}

/// How lit the spotlight is `ms` into the take (0 none, 1 full), given when it turns on and off (each: when, whether its
/// target is `spotlit` — a camera move, or its card gone from the screen while the camera holds there; in order): eased
/// as the camera is (`EASE_MS`, `ease_out`), from wherever it was as a change began — in with a move to a floating
/// target, out with the next move to the code or as its card goes.
pub(crate) fn lit(spots: impl IntoIterator<Item = (u64, bool)>, ms: u64) -> f32 {
    let (mut from, mut to, mut since) = (0.0f32, 0.0f32, 0u64);
    let eased = |from: f32, to: f32, since: u64, now: u64| {
        from + (to - from) * ease_out(now.saturating_sub(since) as f32 / EASE_MS as f32)
    };
    for (t, on) in spots.into_iter().take_while(|m| m.0 <= ms) {
        from = eased(from, to, since, t);
        (to, since) = (if on { 1.0 } else { 0.0 }, t);
    }
    eased(from, to, since, ms)
}

/// The smallest card holding the text found at (row, first cell, past its last).
fn holding(on: &Cells, (r, c0, c1): (usize, usize, usize)) -> Option<Card> {
    (cards(on).into_iter())
        .filter(|b| b.0 <= r && r <= b.2 && b.1 <= c0 && c1 <= b.3 + 1)
        .min_by_key(|b| (b.2 - b.0 + 1) * (b.3 - b.1 + 1))
}

/// A news beat's toast as it's laid over a window's top: the card holding its text, line by line each cell the
/// callout draws — a cell of the screen copied there (row, column), `None` the card's own ground (`ground`, one of its
/// blank cells). The card as it stands on the screen where it fits `cols`; else its words laid again between phrases
/// as tarae's toasts break (`toast_wrap`: a phrase ends after `—`, `·` or a word ending in `,` `;` `:`) — a phrase
/// that doesn't fit a line's end starts the next where it fits one whole, a longer one breaks between its words —, its
/// mark (and the air before it) on the first line, its text as far in as the card has it and its right margin kept.
/// None where no card holds `text`.
pub(crate) struct Callout {
    pub(crate) card: Card,
    pub(crate) ground: (usize, usize),
    pub(crate) lines: Vec<Vec<Option<(usize, usize)>>>,
}

pub(crate) fn callout(on: &Cells, text: &str, cols: usize) -> Option<Callout> {
    let card @ (r0, c0, r1, c1) = holding(on, find(&on.text, text)?)?;
    let t = &on.text;
    // (a wide character's second cell is filled: it's part of its word)
    let filled = |r: usize, c: usize| {
        let s = t[r][c].as_str();
        !s.trim().is_empty() || s.is_empty() && c > 0 && t[r][c - 1].chars().next().is_some_and(wide)
    };
    let ground = (r0..=r1)
        .flat_map(|r| (c0 + 1..=c1).rev().map(move |c| (r, c)))
        .find(|&(r, c)| !filled(r, c))
        .unwrap_or((r0, c1));
    if c1 - c0 < cols {
        let lines = (r0..=r1).map(|r| (c0..=c1).map(|c| Some((r, c))).collect()).collect();
        return Some(Callout { card, ground, lines });
    }
    // (its mark: the first cell, or the one after a cell of air)
    let mark = if t[r0].get(c0 + 1).is_some_and(|c| c == "▎") && t[r0][c0] != "▎" { c0 + 1 } else { c0 };
    let (mut words, mut first, mut last) = (Vec::new(), c1, mark);
    for r in r0..=r1 {
        let mut word: Vec<(usize, usize)> = Vec::new();
        for c in mark + 1..=c1 {
            if filled(r, c) {
                (first, last) = (first.min(c), last.max(c));
                word.push((r, c));
            } else if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        }
        if !word.is_empty() {
            words.push(word);
        }
    }
    if words.is_empty() {
        return None;
    }
    let (lead, pad) = (first - c0, c1 - last);
    let room = cols.saturating_sub(lead + pad).max(1);
    let ends = |w: &[(usize, usize)]| {
        let said: String = w.iter().map(|&(r, c)| t[r][c].as_str()).collect();
        said == "—" || said == "·" || said.ends_with([',', ';', ':'])
    };
    let mut phrases: Vec<Vec<Vec<(usize, usize)>>> = vec![Vec::new()];
    for w in words {
        let end = ends(&w);
        if let Some(p) = phrases.last_mut() {
            p.push(w);
        }
        if end {
            phrases.push(Vec::new());
        }
    }
    let mut rows: Vec<Vec<Option<(usize, usize)>>> = vec![Vec::new()];
    for p in phrases.into_iter().filter(|p| !p.is_empty()) {
        let width = p.iter().map(Vec::len).sum::<usize>() + p.len() - 1;
        let fits = |line: &[Option<(usize, usize)>], w: usize| line.is_empty() || line.len() + 1 + w <= room;
        if rows.last().is_some_and(|l| !fits(l, width)) && width <= room {
            rows.push(Vec::new());
        }
        for w in p {
            if rows.last().is_some_and(|l| !fits(l, w.len())) {
                rows.push(Vec::new());
            }
            let Some(line) = rows.last_mut() else { unreachable!("never empty") };
            if !line.is_empty() {
                line.push(None);
            }
            line.extend(w.into_iter().map(Some));
        }
    }
    let width = lead + rows.iter().map(Vec::len).max().unwrap_or(0) + pad;
    let lines = (rows.into_iter().enumerate())
        .map(|(k, row)| {
            let mut line = vec![None; lead];
            if k == 0 {
                (c0..=mark).for_each(|c| line[c - c0] = Some((r0, c)));
            }
            line.extend(row);
            line.resize(width, None);
            line
        })
        .collect();
    Some(Callout { card, ground, lines })
}

/// Where a callout `w` cells wide stands in a window of whole cells (columns `c0..c1`, rows `r0..r1`): its top-left cell
/// (row, column), a row and a cell of air in from the window's top-right corner — as a toast stands on the screen.
pub(crate) fn callout_at((c0, r0, c1, _): (usize, usize, usize, usize), w: usize) -> (usize, usize) {
    (r0 + 1, c1.saturating_sub(w + 1).max(c0))
}

/// A news target's region shaped to the window (`framed`, its toast never a card the window avoids or takes in — it's
/// laid over the window, not seen where it stands), moved up where the window's top hasn't room above the target for
/// the callout (`callout` at the window's width, less its air) and a row of air under it. Inside the screen.
pub(crate) fn news_framed(
    screen: &Cells,
    text: &str,
    target: Region,
    aspect: f32,
    eye: f32,
    legible: f32,
) -> Region {
    let toast = find(&screen.text, text).and_then(|at| holding(screen, at));
    let r = shaped(screen, target, aspect, eye, legible, toast);
    let Some(c) = callout(screen, text, (r.w as usize).saturating_sub(2)) else { return r };
    let size = (screen.text.first().map_or(0, Vec::len) as u16, screen.text.len() as u16);
    let need = 1.0 + c.lines.len() as f32;
    Region { y: r.y.min((target.y - need).floor()).max(0.0), ..r }.inside(size)
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
    shaped(screen, target, aspect, eye, legible, None)
}

/// `framed`, `skip` (a news beat's toast) no card of the screen's.
fn shaped(screen: &Cells, target: Region, aspect: f32, eye: f32, legible: f32, skip: Option<Card>) -> Region {
    let rows = screen.text.len();
    let cols = screen.text.first().map_or(0, Vec::len);
    let size = (cols as u16, rows as u16);
    // (the card the target is in is drawn even cut — a target narrower than its card, `|N`: no placement avoids it)
    let cards: Vec<Card> =
        cards(screen).into_iter().filter(|&b| !holds(b, target) && Some(b) != skip).collect();
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
/// it could — and one standing between the code's lines (`between`), drawn cut as they are. A Short's frame leaves them out on the window's ground, so nothing is ever cut at its edge — a card that
/// came while a zoom holds (a toast) too.
fn cut(screen: &Cells, view: (usize, usize, usize, usize), target: Region) -> Vec<Card> {
    cards(screen).into_iter().filter(|&b| cuts(view, b) && !holds(b, target) && !between(screen, b)).collect()
}

/// Whether `card` stands between the code's lines (an answer's or a note's card, the code under it moved down): no row
/// of it numbered, the rows just above and below it numbered left of where it starts (a floating card over the gutter
/// covers the numbers instead). A row of the document as its lines are: cut at the window's edge it's drawn cut as they
/// are — left out, it would leave a hole in the code.
fn between(screen: &Cells, (r0, c0, r1, _): Card) -> bool {
    let divs = dividers(&screen.text);
    let number = |r: usize| screen.text.get(r).and_then(|row| numbered(row, &divs)).map(|n| n.1);
    let left_of = |r: usize| number(r).is_some_and(|c| c < c0);
    r0 > 0 && left_of(r0 - 1) && left_of(r1 + 1) && !(r0..=r1).any(|r| number(r).is_some())
}

/// Whether a window of whole cells (columns `c0..c1`, rows `r0..r1`) cuts `card`: meets it, not whole inside it.
fn cuts((c0, r0, c1, r1): (usize, usize, usize, usize), (cr0, cc0, cr1, cc1): Card) -> bool {
    let meets = cc0 < c1 && c0 <= cc1 && cr0 < r1 && r0 <= cr1;
    let whole = c0 <= cc0 && cc1 < c1 && r0 <= cr0 && cr1 < r1;
    meets && !whole
}

/// Whether `card` holds `target` (inside its air).
fn holds((cr0, cc0, cr1, cc1): Card, target: Region) -> bool {
    let (tx0, ty0) = (target.x + AIR, target.y + AIR);
    let (tx1, ty1) = (target.x + target.w - AIR, target.y + target.h - AIR);
    let (x0, y0, x1, y1) = (cc0 as f32, cr0 as f32, cc1 as f32 + 1.0, cr1 as f32 + 1.0);
    x0 <= tx0 && tx1 <= x1 && y0 <= ty0 && ty1 <= y1
}

/// The window's left or right edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Edge {
    Left,
    Right,
}

/// What a Short's frame draws of the screen in a window of whole cells (columns `c0..c1`, rows `r0..r1`): every cell
/// but a card the window cuts (`left_out`, `cut`'s) and a news beat's toast, laid over the window instead (`news`,
/// `callout`), laid with the window's ground; a row whose text runs past the window's left or right edge drawn cut
/// there as a zoomed video crops (`fade`: its last cells easing into their ground); a sliver of one the left edge
/// leaves erased to its ground (`slivers`). A row is drawn whole or not at all — what's drawn belongs to a row that's
/// drawn: never its ground or its blame without its code.
pub(crate) struct Window {
    pub(crate) left_out: Vec<Card>,
    pub(crate) fade: Vec<(usize, Edge)>,
    pub(crate) slivers: Vec<(usize, usize, usize)>,
}

impl Window {
    pub(crate) fn of(
        screen: &Cells,
        view: (usize, usize, usize, usize),
        target: Region,
        news: Option<Card>,
    ) -> Window {
        // (the card the target is in, where the window cuts it — a target narrower than its card, `|N` —, and a card
        // between the code's lines are drawn cut as a code row is: their rows fading at the edge)
        let others: Vec<Card> = (cards(screen).into_iter())
            .filter(|&b| !(cuts(view, b) && (holds(b, target) || between(screen, b))))
            .collect();
        let mut left_out = cut(screen, view, target);
        left_out.extend(news.filter(|n| !left_out.contains(n)));
        let slivers = slivers(screen, view, &others, target);
        Window { left_out, fade: runs_past(screen, view, &others), slivers }
    }

    /// Whether the frame draws the screen's cell at row `r`, column `c` (inside the window).
    pub(crate) fn drawn(&self, r: usize, c: usize) -> bool {
        !self.left_out.iter().any(|b| b.0 <= r && r <= b.2 && b.1 <= c && c <= b.3)
    }
}

/// The rows whose text the window's left or right edge cuts (row, edge): an inked cell on each side of it (a wide
/// character's second cell counts as inked), neither in one of `cards` — a card the window cuts is left out whole, and
/// one beside the window is no text of the row's (the target's own card, cut, isn't among them). A line (`─`, `│`,
/// `▎`…) runs on cut as it is.
fn runs_past(
    screen: &Cells,
    (c0, r0, c1, r1): (usize, usize, usize, usize),
    cards: &[Card],
) -> Vec<(usize, Edge)> {
    let mut out = Vec::new();
    for (r, row) in screen.text.iter().enumerate().take(r1).skip(r0) {
        let inked = |c: usize| inked(row, c);
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

/// Whether a row's cell `c` is inked: text, not a blank nor a line (`─`, `│`, `▎`…) — a wide character's second cell
/// counts.
fn inked(row: &[String], c: usize) -> bool {
    match row.get(c).and_then(|s| s.chars().next()) {
        None => c > 0 && row.get(c - 1).and_then(|s| s.chars().next()).is_some_and(wide),
        Some(ch) => !ch.is_whitespace() && !('\u{2500}'..='\u{259f}').contains(&ch),
    }
}

/// A sliver: what the window's left edge leaves of a row's text, at most this many cells — a `{`, a `;`, a word's last
/// letters say nothing.
const SLIVER: usize = 3;

/// The slivers a window of whole cells (columns `c0..c1`, rows `r0..r1`) leaves at its left edge (row, first cell,
/// past its last): a row's text running in from past the edge (text just outside it, or one blank between) that ends
/// within `SLIVER` cells — at two blanks or a line —, none of it in one of `cards` nor the target's own cells. Drawn,
/// it's noise: the frame erases it to its ground (left out, never faded — a faded `{` is still a `{`).
fn slivers(
    screen: &Cells,
    (c0, r0, c1, r1): (usize, usize, usize, usize),
    cards: &[Card],
    target: Region,
) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    if c0 == 0 {
        return out;
    }
    let own = |r: usize, c: usize| {
        let (r, c) = (r as f32, c as f32);
        target.y + AIR <= r
            && r < target.y + target.h - AIR
            && target.x + AIR <= c
            && c < target.x + target.w - AIR
    };
    for (r, row) in screen.text.iter().enumerate().take(r1).skip(r0) {
        let ink = |c: usize| inked(row, c);
        let space = |c: usize| row.get(c).is_some_and(|s| s.chars().all(char::is_whitespace));
        let from = match ink(c0) {
            true => c0,
            false if c0 + 1 < c1 && space(c0) && ink(c0 + 1) => c0 + 1,
            false => continue,
        };
        // (text just outside: the cell before `from`, or the one before a blank)
        let mut before = (from - 1..from).chain(from.checked_sub(2).filter(|_| space(from - 1)));
        if !before.any(|c| c < c0 && ink(c)) {
            continue;
        }
        let mut to = from;
        while to < c1 && (ink(to) || space(to) && to + 1 < c1 && ink(to + 1)) {
            to += 1;
        }
        let carded = |c: usize| cards.iter().any(|b| b.0 <= r && r <= b.2 && b.1 <= c && c <= b.3);
        if to < c1 && to - from <= SLIVER && !(from..to).any(|c| carded(c) || own(r, c)) {
            out.push((r, from, to));
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
/// `▎` as a toast's and a peek's are (or a blank, the `▎` after it: a toast's cell of air), narrower than three quarters of the screen (a cursor's line is a row across the
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
            let first = &screen.text[b.0];
            let marked = first[b.1] == "▎"
                || first[b.1].trim().is_empty() && first.get(b.1 + 1).is_some_and(|c| c == "▎");
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

    /// A borderless card is a toast's or a peek's: a ground of its own starting with the kind's `▎` (or a cell of air
    /// and the `▎`, as V1's `would conflict` toast stands: #336 framed only its row's words) — never a ground
    /// without it (a selection, a match), nor a wash on the code's own numbered rows, `▎` sign and all, nor a list's
    /// selected row inside its frame (V2's session list: left out alone where the window cut the list it's in).
    #[test]
    fn a_borderless_card_starts_with_its_mark() {
        let mut cells = grid(&[
            "▎ saved · a toast  ",
            "  a selection here ",
            "▎ 3 let x = 1;     ",
            "▎ 4▎let y = 2;     ",
            " ▎ would conflict  ",
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
        // (a toast's cell of air before its mark)
        cells.ground[4][0..12].fill(10);
        assert_eq!(cards(&cells), vec![(5, 0, 7, 13), (0, 0, 0, 11), (4, 0, 4, 11)]);
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

    /// A target cut to `|N` columns of the card it's in (the ask line across the screen, its words at its left): the
    /// card drawn cut, its row fading where the edge cuts its words; no placement avoids it, so the window still slides
    /// past another card it would cut (a list under it), never leaving that one out where it needn't.
    #[test]
    fn a_target_cut_short_shows_its_card_cut() {
        let list: Card = (9, 0, 10, 10);
        let mut on = boxed(40, 12, &[(5, 0, 7, 39), list]);
        for (i, ch) in "ask".chars().enumerate() {
            on.text[5][2 + i] = ch.to_string();
        }
        for (i, ch) in "› why does insert clone".chars().enumerate() {
            on.text[6][2 + i] = ch.to_string();
        }
        let t = target(&on, "card:ask|16").unwrap();
        assert_eq!(t, region(0.0, 4.0, 16.0, 5.0), "the card's first 16 columns, air included");
        assert_eq!(target(&on, "card:ask"), Some(region(0.0, 4.0, 40.0, 5.0)));
        let window = framed(&on, t, 2.0, 0.5, 80.0);
        assert_eq!(window, region(0.0, 3.0, 16.0, 8.0), "rows 2–9 would cut the list: slid to take it whole");
        let shown = Window::of(&on, (0, 3, 16, 11), t, None);
        assert_eq!(shown.left_out, vec![], "the card the target is in, drawn cut");
        assert_eq!(shown.fade, vec![(6, Edge::Right)], "its words fade where the edge cuts them");
    }

    /// A card between the code's lines (an answer's, the code under it moved down: no number on its rows) the window cuts
    /// is drawn cut as the lines are, its words fading at the edge — left out, it was a hole in the code (V4's replay). The same card floating over the code (numbers beside it) is left out.
    #[test]
    fn a_card_between_the_lines_is_drawn_cut() {
        let rows = |beside: &str| {
            grid(&[
                "44 let a = 0;",
                "45 let b = 1;",
                &format!("{beside:<3}╭──────────────────────────────╮"),
                &format!("{beside:<3}│ Done — winds from the front  │"),
                &format!("{beside:<3}╰──────────────────────────────╯"),
                "46 let c = 2;",
                "47 let d = 3;",
            ])
        };
        let target = region(0.0, 0.0, 14.0, 3.0);
        let on = rows("");
        assert_eq!(cards(&on), vec![(2, 3, 4, 34)]);
        let shown = Window::of(&on, (0, 0, 20, 7), target, None);
        assert_eq!(shown.left_out, vec![], "between the lines: drawn");
        assert_eq!(shown.fade, vec![(3, Edge::Right)], "its words cut at the edge, fading");
        // (floating over the code: a number beside each of its rows)
        let over = |n: usize| format!("{n} ");
        let mut on = rows("");
        for (r, n) in [(2, 50), (3, 51), (4, 52)] {
            on.text[r][..3].clone_from_slice(&over(n).chars().map(String::from).collect::<Vec<_>>());
        }
        let shown = Window::of(&on, (0, 0, 20, 7), target, None);
        assert_eq!(shown.left_out, vec![(2, 3, 4, 34)], "floating: left out");
        assert_eq!(shown.fade, vec![]);
        // (floating over the gutter, the numbers under it: left out)
        let mut on = rows("");
        for r in 2..=4 {
            let row: String = on.text[r].concat();
            on.text[r] = row
                .replacen("   ", "", 1)
                .chars()
                .map(String::from)
                .chain([" ", " ", " "].map(String::from))
                .collect();
        }
        assert_eq!(cards(&on), vec![(2, 0, 4, 31)]);
        assert_eq!(
            Window::of(&on, (0, 0, 20, 7), target, None).left_out,
            vec![(2, 0, 4, 31)],
            "over the gutter: left out"
        );
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
        let window = Window::of(&cells, view, region(10.0, 0.0, 20.0, 1.0), None);
        assert!((10..46).all(|c| window.drawn(0, c)), "the line under the cursor drawn whole");
        assert_eq!(window.left_out, vec![(1, 40, 1, 51)], "the toast past the right edge");
        assert!((10..40).all(|c| window.drawn(1, c)) && !(40..46).any(|c| window.drawn(1, c)));
        assert_eq!(window.fade, vec![(0, Edge::Left), (0, Edge::Right)]);
    }

    /// A news beat (`news:<text> over <target>`): its target the code's while the toast is up (none without it); the
    /// toast as it stands where the window has room for it, else laid again between its phrases (`src/main.rs L30 —`
    /// moves down whole, never `L30 —` left from its file); the window moved up so the callout and a row of air stand
    /// above the code; and the toast never drawn where it stands on the screen — whole in the window too.
    #[test]
    fn a_news_beat_frames_its_code_and_lays_its_toast_over_the_top() {
        // (as tarae draws it: a cell of air before its mark)
        let toast = [" ▎ main ≈ fix · src/main.rs L30 — ", "   would conflict"];
        let rows: Vec<String> = (0..24)
            .map(|r| match r {
                0 => "─".repeat(60),
                1 | 2 => format!("{:<26}{:<34} ", "", toast[r - 1]),
                r => format!("{n}     let x{n} = {n};", n = r + 24),
            })
            .collect();
        let mut on = grid(&rows.iter().map(String::as_str).collect::<Vec<_>>());
        on.ground[1][26..60].fill(5);
        on.ground[2][26..60].fill(5);
        let spec = "news:would conflict over code:30-31";
        let code = target(&on, "code:30-31").expect("the code");
        assert_eq!(target(&on, spec), Some(code));
        assert_eq!(target(&on, "news:saved over code:30-31"), None, "no toast up, no news");
        let said = |c: &Callout| -> Vec<String> {
            let cell =
                |x: &Option<(usize, usize)>| x.map_or(" ", |(r, c)| on.text[r][c].as_str()).to_string();
            c.lines.iter().map(|l| l.iter().map(cell).collect::<String>().trim_end().to_string()).collect()
        };
        let whole = callout(&on, "would conflict", 40).expect("the toast");
        assert_eq!(whole.card, (1, 26, 2, 59));
        assert_eq!(said(&whole), toast.map(|t| t.trim_end().to_string()), "as it stands, with room");
        let laid = callout(&on, "would conflict", 29).expect("the toast");
        assert_eq!(said(&laid), [" ▎ main ≈ fix ·", "   src/main.rs L30 —", "   would conflict"]);
        assert!(laid.lines.iter().all(|l| l.len() == 21), "each line the callout's width");
        assert_eq!(on.text[laid.ground.0][laid.ground.1], " ", "its ground a blank of its own");
        let r = news_framed(&on, "would conflict", code, 1.8, 0.3, 40.0);
        let n = callout(&on, "would conflict", r.w as usize - 2).expect("the toast").lines.len() as f32;
        assert!(r.y + 1.0 + n <= code.y, "the callout above the code: {r:?}, {n} lines, {code:?}");
        assert!(code.y + code.h <= r.y + r.h, "the code in the window: {r:?}");
        let view = (0, 0, 60, 24);
        assert!(Window::of(&on, view, code, Some(whole.card)).left_out.contains(&whole.card));
        assert!(Window::of(&on, view, code, None).left_out.is_empty(), "a toast whole in a window is drawn");
    }

    /// A sliver — what the left edge leaves of a row's text, a `{`, a `;`, a word's last letters — is erased: text
    /// just past the edge (or one blank between), ending within `SLIVER` cells; a row's own text starting at the edge,
    /// a longer run, one in a card or the target's own cells is drawn.
    #[test]
    fn a_sliver_at_the_left_edge_is_left_out() {
        let on = grid(&[
            "abcdefgh) {",
            "abcdefghij;",
            "abcdefghwind ────",
            "abcdefghijklmnopqrstuvwxyz",
            "          let",
            "ab        {",
            "abcdefghij {",
            "abcdefghij;",
            "abcdefghij;",
        ]);
        let own = region(9.0, 7.0, 4.0, 3.0);
        let found = slivers(&on, (10, 0, 40, 9), &[(7, 0, 7, 12)], own);
        assert_eq!(found, [(0, 10, 11), (1, 10, 11), (2, 10, 12), (6, 11, 12)]);
        assert_eq!(slivers(&on, (0, 0, 40, 9), &[], own), [], "no edge past the screen's");
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

    /// The spotlight lights a target floating over the code — a card, a prompt, a toast, cut to `|N` or not — by its
    /// whole card; never the code's targets, a row's text, a news beat, nor a card standing between the code's lines.
    #[test]
    fn a_floating_target_is_lit_and_the_codes_are_not() {
        let mut cells = grid(&[
            "─ demo ┬ main.rs ────────────────────────────────────",
            "src/a  │ 1 fn main() {",
            "  b.rs │ 2     let x = 1;",
            "       │ 3 ╭─ sessions ───────╮",
            "       │   │ main working     │",
            "       │   ╰─ enter switch ───╯",
            "       │ 4 }",
            "       │     ╭─ note ─────╮",
            "       │     │ a thread   │",
            "       │     ╰────────────╯",
            "       │ 5 fn more() {}",
            "▎yanked 2 lines                         ╭─ main asks ─╮",
            " ╭─ command ──╮                         │ y run       │",
            " │ :sh git    │                         ╰─ y · n ─────╯",
            "● normal  enter run  esc close",
        ]);
        cells.ground[11][0..16].fill(5);
        let list = vec![(3, 11, 5, 30)];
        assert_eq!(spotlit(&cells, "card:sessions"), list);
        assert_eq!(spotlit(&cells, "card:sessions|20"), list, "cut to its left: lit whole");
        assert_eq!(spotlit(&cells, "main working"), list);
        // (a prompt standing on the status line: the line its keys are said on lit with it; an ask beside it, its keys
        // on its own foot, alone)
        let cols = cells.text[0].len();
        assert_eq!(spotlit(&cells, "card:command"), vec![(12, 1, 13, 14), (14, 0, 14, cols - 1)]);
        assert_eq!(spotlit(&cells, "card:y run"), vec![(11, 40, 13, 54)]);
        assert_eq!(spotlit(&cells, "card:yanked"), vec![(11, 0, 11, 15)]);
        // (the code's targets, a row's text and a news beat stay bright)
        for spec in
            ["all", "status", "strip", "code:1-2", "code:1-2|20", "let x", "news:yanked over code:1-2"]
        {
            assert_eq!(spotlit(&cells, spec), [], "{spec}");
        }
        // (a note's card between lines 4 and 5: rows of the document, no spotlight)
        assert_eq!(spotlit(&cells, "card:a thread"), []);
        assert_eq!(spotlit(&cells, "card:nowhere"), []);
    }

    /// The spotlight eases in with a move to a floating target and out with the next to the code, as the camera does
    /// (`EASE_MS`, an ease-out), a move begun mid-way starting from where it had got to; dark before any.
    #[test]
    fn the_spotlight_eases_in_and_out_with_the_camera() {
        let moves = [(1_000, true), (3_000, false), (3_100, true), (6_000, false)];
        assert_eq!(lit(moves, 0), 0.0);
        assert_eq!(lit(moves, 999), 0.0);
        assert!(lit(moves, 1_000 + EASE_MS / 4) > 0.5, "not an ease-out");
        assert_eq!(lit(moves, 1_000 + EASE_MS), 1.0);
        assert_eq!(lit(moves, 2_999), 1.0, "held until the next move");
        // (out from 3 s, back in at 3.1 s from where it had got to: never a jump)
        let (out, back) = (lit(moves, 3_100), lit(moves, 3_133));
        assert!(0.0 < out && out < 1.0 && back > out, "{out} {back}");
        assert!((lit(moves, 3_100 + EASE_MS) - 1.0).abs() < 1e-6);
        assert_eq!(lit(moves, 6_000 + EASE_MS), 0.0);
        let frames: Vec<f32> = (0..7_000u64).step_by(33).map(|ms| lit(moves, ms)).collect();
        assert!(frames.windows(2).all(|p| (p[1] - p[0]).abs() < 0.3), "a jump");
    }
}
