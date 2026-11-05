//! A Short's camera (`scenes --video --shorts`, `video.rs`). **It flows, nothing pops** (CLAUDE.md's Shorts bullet):
//! the camera stands still on the whole at 1.0×, no drift, no push, and moves only for an emphasis — the one read a beat
//! may have (the scene's `@zoom:<target>`, `@zoom-deep:` where its text is too small otherwise; `READS_MAX` a Short at
//! most): eased in, held its look (`course`), eased back out. Where it aims (`aims`: the whole, a read's window while it
//! holds) is followed by a move of `DRIFT_MS` each way (`way`, `Path`: 쫀득 — a quick start that lands soft, 2 % past
//! going in), never a snap; exactly the whole everywhere else.
//! Where most of the screen changes at once (a view opening, a workspace switch) the two screens cross over `CROSS_MS`
//! (`crossfades`), never a cut. Where the camera is is a region of the screen in cells, fractions and all; the frame
//! draws it at any scale (`video.rs` resamples), so a push is continuous — never a whole glyph size's step. A read's
//! window is at a depth that reads (at least the columns a phone shows at its legible glyph — ~38 of 80 —, from the
//! target's pane's left: `pane_left`), a deep one's the target's own (`framed`). A news beat (`news:<text> over <target>`) frames what its
//! toast is about and lays the toast over the window's top (`callout`). Pure: the frames' drawing is `video.rs`'s.

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

/// An ease in and out, a half cosine: still at both ends, quickest in the middle.
pub(crate) fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    (1.0 - (std::f32::consts::PI * t).cos()) / 2.0
}

/// The camera's move into a read and back out to the whole: there this long after it starts, each way (CLAUDE.md's
/// Shorts bullet — ~1.2 s, 쫀득: a quick start that lands soft; never the motion rule's 0.4 s). How it goes is `way`.
pub(crate) const DRIFT_MS: u64 = 1_200;
/// The camera's path is worked out at this step, and read between its steps.
const STEP_MS: u64 = 10;
/// A read held once the camera is there, where its target stays: the one thing it's about, read — as at 1.5 s moves,
/// the look the user kept.
pub(crate) const READ_MS: u64 = 2_000;

/// Going back out to the whole, a critically damped spring's rate (1/s): its residue `(1 + ωt)e^(−ωt)` at 1 % at
/// `DRIFT_MS` (ωt = 6.638) — never past the whole, which is past the screen's edge.
const SPRING_W: f32 = 6.638 / (DRIFT_MS as f32 / 1000.0);
/// Going into a read, an underdamped spring's: how far past its mark it goes, once (its damping ζ from it:
/// `e^(−ζπ/√(1−ζ²))` = 2 %, ζ = 0.78), and its rate's (its envelope at 0.2 % at `DRIFT_MS`: ζωt = 6.683).
const OVERSHOOT: f32 = 0.02;
const OVER_ZETA_W_T: f32 = 6.683;

/// How much of the way a move of the camera has come `u` of `DRIFT_MS` on (CLAUDE.md's Shorts bullet: 쫀득 — a quick
/// start that lands soft, never a bounce that rocks; the user's (c) of three takes): `toward` a read past its mark by
/// `OVERSHOOT` once and back, else back out to the whole as the critically damped spring. 0 at its start, exactly 1
/// from its end on: a spring's own curve is scaled to end there exactly, its residue (< 1 %) taken up along the way.
pub(crate) fn way(u: f32, toward: bool) -> f32 {
    if u <= 0.0 {
        return 0.0;
    }
    if u >= 1.0 {
        return 1.0;
    }
    let secs = |u: f32| u * DRIFT_MS as f32 / 1000.0;
    let spring = |u: f32| {
        let wt = SPRING_W * secs(u);
        1.0 - (1.0 + wt) * (-wt).exp()
    };
    let under = |u: f32| {
        let ln = OVERSHOOT.ln();
        let zeta = -ln / (std::f32::consts::PI.powi(2) + ln * ln).sqrt();
        let (w, root) = (OVER_ZETA_W_T / zeta / secs(1.0), (1.0 - zeta * zeta).sqrt());
        let (t, wd) = (secs(u), w * root * secs(u));
        1.0 - (-zeta * w * t).exp() * (wd.cos() + zeta / root * wd.sin())
    };
    match toward {
        true => under(u) / under(1.0),
        false => spring(u) / spring(1.0),
    }
}

/// The move as numbers (`toward` a read or back): when it has come 90 % of the way (ms), and how far past its mark it
/// goes (a share of the way; 0: never).
pub(crate) fn way_numbers(toward: bool) -> (u64, f32) {
    let steps = DRIFT_MS as usize;
    let at = |ms: usize| way(ms as f32 / steps as f32, toward);
    let ninety = (0..=steps).find(|&ms| at(ms) >= 0.9).unwrap_or(steps) as u64;
    let past = (0..=steps).map(at).fold(1.0f32, f32::max) - 1.0;
    (ninety, past)
}
/// The least a read stands once the camera is there; under it, no read.
pub(crate) const READ_MIN_MS: u64 = 1_000;
/// Two screens cross where more than this share of the cells changed between them (a view opening, a workspace switch)
/// — over `CROSS_MS`, never a cut.
pub(crate) const CROSS: f32 = 0.5;
pub(crate) const CROSS_MS: u64 = 200;
/// The most reads a Short has (CLAUDE.md's Shorts bullet: a zoom is for emphasis only — the moments it's for, two or
/// three); one past them is said and its beat stays on the whole.
pub(crate) const READS_MAX: usize = 3;

/// A beat's one read as `course` lays it: when its beat came and the next one comes (none: the take's last), when its
/// target is first on the screen at or after its mark, when it leaves the screen before the next beat (none: it stays),
/// and its window.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Planned {
    pub(crate) beat: u64,
    pub(crate) next: Option<u64>,
    pub(crate) found: u64,
    pub(crate) gone: Option<u64>,
    pub(crate) window: Region,
}

/// A read laid on the take: when the camera starts toward its window (`into`, the drift beginning) and when it starts
/// away again (`out`) — there `DRIFT_MS` after `into` —, and how much longer its beat would have to be for the whole
/// read (0: it fits).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Read {
    pub(crate) into: u64,
    pub(crate) out: u64,
    pub(crate) short: u64,
}

impl Read {
    /// When the camera is there: the drift's end.
    pub(crate) fn there(&self) -> u64 {
        self.into + DRIFT_MS
    }
}

/// The camera's reads: each beat's (none where it had no room for `READ_MIN_MS`, its target went too soon, or the Short
/// has had its `READS_MAX`).
pub(crate) struct Course {
    pub(crate) reads: Vec<Option<Read>>,
    /// How much longer each beat would have to be for its read, where the next beat cuts it short (cut, or none).
    pub(crate) short: Vec<(usize, u64)>,
    /// The reads whose target left the screen before one could stand (`READ_MIN_MS`).
    pub(crate) early: Vec<usize>,
    /// The reads past `READS_MAX`.
    pub(crate) over: Vec<usize>,
}

/// The camera's rule over a take's reads (each beat's one, in order): most beats have none — the camera stands on the
/// whole (`aims`). A read's drift begins as its target is found and arrives `DRIFT_MS` later; it holds there until
/// its target leaves the screen (an ask line sent, a view gone — past `READ_MS` too) or, where it stays, `READ_MS` —
/// never past the next beat (cut there, said with how much longer the beat would have to be: `short`). One that can't
/// stand `READ_MIN_MS` once there is none — `early` where its target went, `short` where the next beat came. Past
/// `READS_MAX` reads, the rest are none (`over`).
pub(crate) fn course(reads: &[Planned]) -> Course {
    let (mut laid, mut short, mut early, mut over) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (i, p) in reads.iter().enumerate() {
        if laid.iter().flatten().count() == READS_MAX {
            over.push(i);
            laid.push(None);
            continue;
        }
        let into = p.found;
        let (least, want) = (into + DRIFT_MS + READ_MIN_MS, p.gone.unwrap_or(into + DRIFT_MS + READ_MS));
        if p.gone.is_some_and(|g| g < least && p.next.is_none_or(|n| g <= n)) {
            early.push(i);
            laid.push(None);
            continue;
        }
        let out = want.min(p.next.unwrap_or(u64::MAX));
        let by = want - out;
        if by > 0 {
            short.push((i, by));
        }
        laid.push((out >= least).then_some(Read { into, out, short: by }));
    }
    Course { reads: laid, short, early, over }
}

impl Course {
    /// Each read's window from when to when the camera aims at it (`aims`' `reads`).
    pub(crate) fn windows(&self, reads: &[Planned]) -> Vec<(u64, u64, Region)> {
        (self.reads.iter().zip(reads)).filter_map(|(r, p)| r.map(|r| (r.into, r.out, p.window))).collect()
    }

    /// Where a punchline marked at `mark` opens: the first read still held then — once the camera is there, if that's
    /// later —, else the mark.
    pub(crate) fn punch(&self, mark: u64) -> u64 {
        (self.reads.iter().flatten()).find(|r| r.out > mark).map_or(mark, |r| mark.max(r.there()))
    }
}

/// How near a region brings the screen (1: the whole).
pub(crate) fn nearer(r: Region, screen: (u16, u16)) -> f32 {
    (f32::from(screen.0) / r.w).min(f32::from(screen.1) / r.h)
}

/// Where the camera aims over a take, from when, in order (`Path`'s): the whole (CLAUDE.md's Shorts bullet: the camera
/// stands still on the whole), but a read's window from when the camera starts toward it to when it starts away
/// (`reads`: from, to, window — in order, apart) — a Short's emphasis, the one place it moves.
pub(crate) fn aims(reads: &[(u64, u64, Region)], screen: (u16, u16)) -> Vec<(u64, Region)> {
    let all = Region::all(screen);
    let mut aimed = vec![(0, all)];
    for &(from, to, window) in reads {
        aimed.extend([(from, window.inside(screen)), (to, all)]);
    }
    aimed
}

/// The camera's path over a take: its `aims` (from when, where — in order; the whole before the first), each change of
/// aim a move from the step after it, its share of the way at each moment `way`'s — there exactly
/// `DRIFT_MS` on; moves that meet add up (a new aim mid-move joins it, no stop). The whole stays exactly the whole, and
/// every region is inside the screen. Worked out every `STEP_MS` to `until`, read between.
pub(crate) struct Path {
    at: Vec<Region>,
}

impl Path {
    pub(crate) fn of(aims: &[(u64, Region)], screen: (u16, u16), until: u64) -> Path {
        let all = Region::all(screen);
        let n = (until / STEP_MS + 2) as usize;
        let taps = (DRIFT_MS / STEP_MS) as usize;
        // (each change of aim: the first step it's in force at, how far it moves — by its offsets from the whole — and
        // whether toward a read)
        let (mut moves, mut was) = (Vec::new(), [0.0f32; 4]);
        for &(ms, r) in aims {
            let r = r.inside(screen);
            let now = [r.x - all.x, r.y - all.y, r.w - all.w, r.h - all.h];
            if now != was {
                let by = [now[0] - was[0], now[1] - was[1], now[2] - was[2], now[3] - was[3]];
                moves.push((ms.div_ceil(STEP_MS) as usize, by, now != [0.0; 4]));
            }
            was = now;
        }
        let at = (0..n)
            .map(|i| {
                // (a move starts from the step after its aim's, there exactly `DRIFT_MS` on)
                let mut d = [0.0f32; 4];
                for &(from, by, toward) in moves.iter().take_while(|m| m.0 < i) {
                    let way = way((i - from) as f32 / taps as f32, toward);
                    for (d, b) in d.iter_mut().zip(by) {
                        *d += way * b;
                    }
                }
                let r = Region { x: all.x + d[0], y: all.y + d[1], w: all.w + d[2], h: all.h + d[3] };
                if d.iter().all(|v| v.abs() < 1e-4) { all } else { r.inside(screen) }
            })
            .collect();
        Path { at }
    }

    /// Where the camera is `ms` into the take: between the two steps round it.
    pub(crate) fn at(&self, ms: u64) -> Region {
        let i = ((ms / STEP_MS) as usize).min(self.at.len() - 1);
        let (a, b) = (self.at[i], self.at[(i + 1).min(self.at.len() - 1)]);
        if a == b {
            return a;
        }
        let t = (ms % STEP_MS) as f32 / STEP_MS as f32;
        let l = |p: f32, q: f32| p + (q - p) * t;
        Region { x: l(a.x, b.x), y: l(a.y, b.y), w: l(a.w, b.w), h: l(a.h, b.h) }
    }
}

/// The share of the cells (0..=1) whose text or ground differ between two screens of one size.
pub(crate) fn changed(a: &Cells, b: &Cells) -> f32 {
    let cells = (a.text.iter().zip(&b.text)).map(|(x, y)| x.len().min(y.len())).sum::<usize>().max(1);
    let ground = (a.ground.iter().flatten().zip(b.ground.iter().flatten())).map(|(x, y)| x != y);
    let differ = (a.text.iter().flatten().zip(b.text.iter().flatten()))
        .zip(ground)
        .filter(|((x, y), g)| x != y || *g)
        .count();
    differ as f32 / cells as f32
}

/// The screens that come by a crossing (each one's index): more than `CROSS` of the cells changed from the one before
/// (`changed[i]`: between screen `i - 1` and `i`).
pub(crate) fn crossfades(changed: &[f32]) -> Vec<usize> {
    (changed.iter().enumerate()).skip(1).filter(|(_, c)| **c > CROSS).map(|(i, _)| i).collect()
}

/// What crosses `ms` into the take (the screens drawn at `times`, those that come by a crossing `fades`): the screen it
/// comes from — the one before the latest crossing begun within `CROSS_MS` — and how far the screen now is drawn over
/// it (eased in and out). None where nothing crosses.
pub(crate) fn crossing(times: &[u64], fades: &[usize], ms: u64) -> Option<(usize, f32)> {
    let &i = fades.iter().rev().find(|&&i| times[i] <= ms)?;
    (ms < times[i] + CROSS_MS).then(|| (i - 1, ease_in_out((ms - times[i]) as f32 / CROSS_MS as f32)))
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
/// - `rows:<a> … <b>` the rows from the one showing `<a>` through the one showing `<b>`, from where the first of their
///   texts starts to where the longest ends (each row's stretch, as a text's not in a card), air above and below
///   only: a card's lines that matter, where the card is wider than a read's legible columns — the card drawn cut
///   round them, its own pad their air across;
/// - `news:<text> over <target>` the target's, while the card holding `<text>` (a toast) is on the screen too: a news
///   beat, the code its toast names framed, the toast laid over the window's top (`news_framed`, `callout`).
///
/// Each with a cell of air round it (but `rows:` across). None where it isn't on that screen.
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
    let of = Region::of;
    let blank = |s: &str| s.trim().is_empty();
    if let Some((a, b)) = spec.strip_prefix("rows:").and_then(|s| s.split_once(" … ")) {
        let ((ra, a0, a1), (rb, b0, b1)) = (find(cells, a)?, find(cells, b)?);
        let ((r0, c0), (r1, c1)) = ((ra, stretch(&cells[ra], a0, a1)), (rb, stretch(&cells[rb], b0, b1)));
        // (their air across is the card's own pad: the window may cut the card round them, never their words)
        let r = (r0 <= r1).then(|| of(c0.0.min(c1.0), r0, c0.1.max(c1.1), r1 + 1, screen))?;
        return Some(Region { x: r.x + AIR, w: r.w - 2.0 * AIR, ..r });
    }
    match spec {
        "all" => Some(Region::all(screen)),
        "status" => {
            let r = rows.checked_sub(1)?;
            let (c0, c1) = text_span(&cells[r], 0, cols)?;
            Some(of(c0, r, c1, r + 1, screen))
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
            Some(of(start, 0, end, 1, screen))
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
                Some(of(c0, r0, c1, r1, screen))
            }
            None => {
                let text = spec.strip_prefix("card:").unwrap_or(spec);
                let (r, c0, c1) = find(cells, text)?;
                Some(match holding(on, (r, c0, c1)) {
                    Some((r0, c0, r1, c1)) => of(c0, r0, c1 + 1, r1 + 1, screen),
                    None => {
                        let (first, end) = stretch(&cells[r], c0, c1);
                        of(first, r, end, r + 1, screen)
                    }
                })
            }
        },
    }
}

/// The stretch of `row`'s text round its columns `c0..c1`: to two blanks, a line or the edge.
fn stretch(row: &[String], c0: usize, c1: usize) -> (usize, usize) {
    let blank = |c: usize| row[c].trim().is_empty();
    let stop = |c: usize| matches!(row[c].as_str(), "│" | "─");
    let gap = |c: usize| blank(c) && (c == 0 || blank(c - 1));
    let first = (0..c0).rev().find(|&c| stop(c) || gap(c)).map_or(0, |c| c + 1);
    let gap_after = |c: usize| blank(c) && (c + 1 >= row.len() || blank(c + 1));
    let end = (c1..row.len()).find(|&c| stop(c) || gap_after(c)).unwrap_or(row.len());
    text_span(row, first, end).unwrap_or((c0, c1))
}

/// A news target's toast text and the target it's about (`news:<text> over <target>`).
pub(crate) fn news(spec: &str) -> Option<(&str, &str)> {
    spec.strip_prefix("news:")?.split_once(" over ")
}

/// What `spec`'s card says on its first row inside its frame — an input's line: the ask line's question as it's typed —,
/// trimmed (a caption's `{}`, `captions::timed`). None where no framed card holds the target's text on this screen, or
/// that row is blank.
pub(crate) fn said_in(on: &Cells, spec: &str) -> Option<String> {
    let bare = spec.rsplit_once('|').filter(|(_, n)| n.parse::<u16>().is_ok()).map_or(spec, |(s, _)| s);
    let (r0, c0, r1, c1) = holding(on, find(&on.text, bare.strip_prefix("card:").unwrap_or(bare))?)?;
    let row = on.text.get(r0 + 1).filter(|_| r1 > r0 + 1 && on.text[r0][c0] == "╭")?;
    let said = row[c0 + 1..c1].concat();
    // (past the input's prompt mark)
    let said = said.trim().trim_start_matches('›').trim();
    (!said.is_empty()).then(|| said.to_string())
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
    (aspect, eye, legible, deep): (f32, f32, f32, bool),
) -> Region {
    let toast = find(&screen.text, text).and_then(|at| holding(screen, at));
    let r = shaped(screen, target, (aspect, eye, legible, deep), toast, (&[], aspect));
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
/// frame is drawn (`cut`), the zoom kept. A `deep` read's window is so; a shallow one's (every other read's) is at
/// least `legible` columns wide — the emphasis at a depth that reads (CLAUDE.md's Shorts bullet), wider only for a
/// target wider than that —, taking in what it cuts while that costs it no more than `TAKEN_IN_MAX`, else narrower —
/// deeper, never narrower than its target — until it cuts none (a card wholly in or wholly out), its left edge at
/// the target's pane's left (`pane_left`: its gutter kept) wherever the target fits that way, centred only where it
/// doesn't. Inside the screen.
#[cfg(test)]
pub(crate) fn framed(
    screen: &Cells,
    target: Region,
    aspect: f32,
    eye: f32,
    legible: f32,
    deep: bool,
) -> Region {
    shaped(screen, target, (aspect, eye, legible, deep), None, (&[], aspect))
}

/// `framed`, its window clear of `later` too — the cards the screens after this one draw while the read stands (a card
/// growing beside the target as the camera holds: V4's follow card, a column wider by the hold), each wholly in or out
/// as this screen's are; one over the target's own cells isn't one to clear. `drawn` is the shape (columns a row) the
/// frame shows a window at (`video::view`: the whole's cells), where a window narrows to clear a card.
pub(crate) fn framed_among(
    screen: &Cells,
    target: Region,
    (aspect, eye, legible, deep): (f32, f32, f32, bool),
    (later, drawn): (&[Card], f32),
) -> Region {
    shaped(screen, target, (aspect, eye, legible, deep), None, (later, drawn))
}

/// `framed`, `skip` (a news beat's toast) no card of the screen's, `later` cards to clear as its own and `drawn` the
/// frame's shape (`framed_among`).
fn shaped(
    screen: &Cells,
    target: Region,
    (aspect, eye, legible, deep): (f32, f32, f32, bool),
    skip: Option<Card>,
    (later, drawn): (&[Card], f32),
) -> Region {
    let rows = screen.text.len();
    let cols = screen.text.first().map_or(0, Vec::len);
    let size = (cols as u16, rows as u16);
    let (least, legible) = match deep {
        true => (0.0, legible),
        false => (legible.min(cols as f32), cols as f32),
    };
    // (a shallow window from its pane's left first, where that holds the target)
    let left = (!deep).then(|| pane_left(&screen.text, target) as f32);
    // (the card the target is in is drawn even cut — a target narrower than its card, `|N`: no placement avoids it)
    let mut cards: Vec<Card> =
        cards(screen).into_iter().filter(|&b| !holds(b, target) && Some(b) != skip).collect();
    let over = |(r0, c0, r1, c1): Card| {
        (c0 as f32) < target.x + target.w
            && target.x < c1 as f32 + 1.0
            && (r0 as f32) < target.y + target.h
            && target.y < r1 as f32 + 1.0
    };
    for &b in later.iter().filter(|&&b| !over(b)) {
        if !cards.contains(&b) {
            cards.push(b);
        }
    }
    let shape = |r: Region| match r.w.max(least) / r.h > aspect {
        true => (r.w.max(least).min(cols as f32), (r.w.max(least) / aspect).min(rows as f32)),
        false => ((r.h * aspect).min(cols as f32), r.h.min(rows as f32)),
    };
    // (each whole-cell placement holding a stretch `from..from + len` in `extent`, the ones putting its centre nearest
    // `at` of the extent first)
    let starts = |from: f32, len: f32, extent: f32, n: usize, at: f32, first: Option<f32>| -> Vec<f32> {
        let lo = (from + len - extent).max(0.0).ceil() as i64;
        let hi = from.min(n as f32 - extent).floor() as i64;
        let centre = from + len / 2.0 - extent * at;
        let mut all: Vec<f32> = (lo..=hi.max(lo)).map(|s| s as f32).collect();
        all.sort_by(|a, b| (a - centre).abs().total_cmp(&(b - centre).abs()));
        if let Some(i) = first.and_then(|f| all.iter().position(|&s| s == f)) {
            let f = all.remove(i);
            all.insert(0, f);
        }
        all
    };
    let cuts = |x: f32, y: f32, (w, h): (f32, f32)| -> Option<Card> {
        let (x1, y1) = (x + w, y + h);
        cards.iter().copied().find(|&(r0, c0, r1, c1)| {
            let (cr0, cc0, cr1, cc1) = (r0 as f32, c0 as f32, r1 as f32 + 1.0, c1 as f32 + 1.0);
            let meets = cc0 < x1 && x < cc1 && cr0 < y1 && y < cr1;
            let whole = x <= cc0 && cc1 <= x1 && y <= cr0 && cr1 <= y1;
            meets && !whole
        })
    };
    let mut want = target;
    for _ in 0..4 {
        let (w, h) = shape(want);
        let (xs, ys) =
            (starts(want.x, want.w, w, cols, 0.5, left), starts(want.y, want.h, h, rows, eye, None));
        let mut cut = None;
        for &y in &ys {
            for &x in &xs {
                match cuts(x, y, (w, h)) {
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
    // (a shallow window that cuts a card wherever it stands, and can't take it in: narrower a column at a time — a
    // deeper read, never narrower than its target — until what the frame shows of it leaves every card wholly out; V4's
    // review beside the follow card. The frame shows a window as `drawn`'s shape about its middle — `k` times its
    // width where its height binds —, so the columns shown are what's placed, whole cells, and the window centred in them)
    let k = (drawn / aspect).max(1.0);
    let own = target.w.min(cols as f32);
    let mut sw = ((least * k).floor() - 1.0).min(cols as f32);
    while sw >= own {
        let (sh, w) = ((sw / drawn).min(rows as f32), sw / k);
        let h = w / aspect;
        if sh >= target.h.min(rows as f32) {
            let (xs, ys) = (
                starts(target.x, target.w, sw, cols, 0.5, left),
                starts(target.y, target.h, sh, rows, eye, None),
            );
            for &y in &ys {
                if let Some(&x) = xs.iter().find(|&&x| cuts(x, y, (sw, sh)).is_none()) {
                    return Region { x: x + (sw - w) / 2.0, y: y + (sh - h) / 2.0, w, h }.inside(size);
                }
            }
        }
        sw -= 1.0;
    }
    let (w, h) = shape(target);
    let x = starts(target.x, target.w, w, cols, 0.5, left).first().copied().unwrap_or(target.x);
    let y = starts(target.y, target.h, h, rows, eye, None).first().copied().unwrap_or(target.y);
    Region { x, y, w, h }.inside(size)
}

/// The left column of the pane `target` starts in: past the nearest of the screen's `dividers` at or left of it (its
/// air may stand on the divider), else the screen's edge.
fn pane_left(cells: &[Vec<String>], target: Region) -> usize {
    let at = target.x.max(0.0) as usize;
    dividers(cells).into_iter().filter(|&d| d <= at).max().map_or(0, |d| (d + 1).min(at))
}

/// What a frame shows of `window` where it draws a window at `drawn`'s shape (columns a row; `video::view`): the window
/// widened, or made taller, about its middle to that shape.
pub(crate) fn shown(window: Region, drawn: f32) -> Region {
    let (w, h) = (window.w.max(window.h * drawn), window.h.max(window.w / drawn));
    Region { x: window.x + (window.w - w) / 2.0, y: window.y + (window.h - h) / 2.0, w, h }
}

/// Whether a shallow read's `window` keeps its target's pane's left (its gutter's numbers): not where the target can't
/// fit a window from there (a card at the screen's edge) — the window then holds the target whole (`framed`), and the
/// read says so.
pub(crate) fn keeps_panes_left(screen: &Cells, target: Region, window: Region) -> bool {
    window.x <= pane_left(&screen.text, target) as f32
}

/// Whether `card` holds `target` (inside its air).
fn holds((cr0, cc0, cr1, cc1): Card, target: Region) -> bool {
    let (tx0, ty0) = (target.x + AIR, target.y + AIR);
    let (tx1, ty1) = (target.x + target.w - AIR, target.y + target.h - AIR);
    let (x0, y0, x1, y1) = (cc0 as f32, cr0 as f32, cc1 as f32 + 1.0, cr1 as f32 + 1.0);
    x0 <= tx0 && tx1 <= x1 && y0 <= ty0 && ty1 <= y1
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

    fn close(a: Region, b: Region) -> bool {
        [(a.x, b.x), (a.y, b.y), (a.w, b.w), (a.h, b.h)].iter().all(|(p, q)| (p - q).abs() < 1e-3)
    }

    /// How far `r` has come from `from` toward `to` (0..=1), by its width.
    fn along(from: Region, to: Region, r: Region) -> f32 {
        (r.w - from.w) / (to.w - from.w)
    }

    /// The move (CLAUDE.md's Shorts bullet: 쫀득 — a quick start that lands soft, never a bounce that rocks; the user's
    /// (c)): from 0 to exactly 1 over `DRIFT_MS`, from rest (its first hundredth under 1 % of the way), 90 % of the way
    /// in 30–60 % of its time. Into a read past its mark by `OVERSHOOT` once, then back to it with no second swing, 90 %
    /// within 0.4 s; back out to the whole never back nor past it (past the whole is past the screen's edge), over a
    /// third of the way in its first fifth and within 1 % of it its last tenth.
    #[test]
    fn the_move_starts_quick_and_lands_soft() {
        for toward in [true, false] {
            assert_eq!((way(0.0, toward), way(1.0, toward)), (0.0, 1.0));
            let (ninety, _) = way_numbers(toward);
            assert!((DRIFT_MS * 3 / 10..=DRIFT_MS * 3 / 5).contains(&ninety), "90 % at {ninety} ms");
            assert!(way(0.01, toward) < 0.01, "from rest");
        }
        let curve = |toward| -> Vec<f32> { (0..=1000).map(|k| way(k as f32 / 1000.0, toward)).collect() };
        let over = curve(true);
        let peak = (0..over.len()).max_by(|&a, &b| over[a].total_cmp(&over[b])).unwrap();
        assert!((over[peak] - 1.0 - OVERSHOOT).abs() < 0.002, "past by {}", over[peak] - 1.0);
        assert!(over[..=peak].windows(2).all(|w| w[0] <= w[1]), "one way up");
        assert!(over[peak..].windows(2).all(|w| w[1] <= w[0] && w[1] >= 1.0 - 1e-4), "settled once");
        assert!(way_numbers(true).0 <= 410, "in: 90 % at {} ms", way_numbers(true).0);
        let back = curve(false);
        assert!(back.windows(2).all(|w| w[0] <= w[1] && w[1] <= 1.0), "never back nor past");
        assert!(way(0.2, false) > 0.33 && way(0.9, false) > 0.99);
        assert_eq!(way_numbers(false).1, 0.0);
    }

    /// A change of aim is a camera's move of `DRIFT_MS` (CLAUDE.md's Shorts bullet: ~1.2 s each way): none before it,
    /// its way in time `way`'s, there exactly `DRIFT_MS` on and held, back on the whole exactly after it; no frame's step
    /// past four times the mean speed (its quickest ~3.7, going in); a new aim mid-move joins it and gets there; every
    /// region inside the screen.
    #[test]
    fn the_camera_moves_in_and_out_never_a_snap() {
        let all = Region::all(SCREEN);
        let card = region(4.5, 4.5, 75.5, 38.5);
        assert!((1_100..=1_300).contains(&DRIFT_MS), "about 1.2 s each way");
        let path = Path::of(&[(1_000, card), (8_000, all)], SCREEN, 12_000);
        assert_eq!((path.at(0), path.at(1_000)), (all, all));
        for k in 1..10 {
            let (u, ms) = (k as f32 / 10.0, 1_000 + DRIFT_MS * k / 10);
            let came = along(all, card, path.at(ms));
            assert!((came - way(u, true)).abs() < 0.005, "at {u}: {came}");
        }
        assert!(close(path.at(1_000 + DRIFT_MS), card), "there: {:?}", path.at(1_000 + DRIFT_MS));
        assert!(close(path.at(7_999), card), "held");
        let back = along(card, all, path.at(8_000 + DRIFT_MS / 2));
        assert!((back - way(0.5, false)).abs() < 0.005, "out: {back}");
        assert_eq!(path.at(8_000 + DRIFT_MS), all, "back on the whole exactly");
        let frames: Vec<Region> = (0..12_000u64).step_by(33).map(|ms| path.at(ms)).collect();
        let most = (all.w - card.w) * 4.0 * 33.0 / DRIFT_MS as f32;
        for p in frames.windows(2) {
            assert!((p[1].w - p[0].w).abs() <= most, "a jump: {p:?}");
        }
        // (a new aim mid-move)
        let list = region(10.0, 4.0, 60.0, 30.0);
        let path = Path::of(&[(1_000, card), (1_600, list)], SCREEN, 8_000);
        for ms in (0..8_000u64).step_by(10) {
            let r = path.at(ms);
            assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= 80.001 && r.y + r.h <= 43.001, "{r:?}");
        }
        assert!(close(path.at(1_600 + DRIFT_MS), list));
    }

    /// The camera stands still on the whole at 1.0× (CLAUDE.md's Shorts bullet) — exactly the whole at every moment but
    /// a read's: from when it starts toward the read's window to `DRIFT_MS` after it starts away; there it eases in, holds
    /// the window, and eases back out. Two reads, the whole between them.
    #[test]
    fn the_camera_stands_on_the_whole_but_for_a_read() {
        let all = Region::all(SCREEN);
        let (window, other) = (region(0.0, 10.0, 56.0, 30.0), region(20.0, 2.0, 40.0, 21.5));
        let reads = [(5_000, 9_500, window), (20_000, 24_000, other)];
        let aimed = aims(&reads, SCREEN);
        assert_eq!(aimed, [(0, all), (5_000, window), (9_500, all), (20_000, other), (24_000, all)]);
        let path = Path::of(&aimed, SCREEN, 30_000);
        for ms in (0..30_000u64).step_by(5) {
            // (the step before a move's first is read toward it)
            if !reads.iter().any(|r| r.0 < ms + STEP_MS && ms < r.1 + DRIFT_MS) {
                assert_eq!(path.at(ms), all, "on the whole at {ms} ms");
            } else if reads.iter().any(|r| r.0 < ms && ms + STEP_MS < r.1 + DRIFT_MS) {
                assert_ne!(path.at(ms), all, "moving at {ms} ms");
            }
        }
        assert!(close(path.at(5_000 + DRIFT_MS), window) && close(path.at(9_500), window), "held");
        let back = along(window, all, path.at(9_500 + DRIFT_MS / 2));
        assert!((back - way(0.5, false)).abs() < 0.001, "eased out");
        assert!(close(path.at(20_000 + DRIFT_MS), other));
    }

    /// A read: the drift begins as its target is found, the camera there `DRIFT_MS` on; it holds `READ_MS` there, or
    /// until its target goes (past `READ_MS` too); the next beat cuts it short, said by how much the beat is short —
    /// none where it can't stand `READ_MIN_MS` once there; one whose target goes before is none, `early`.
    #[test]
    fn a_read_is_reached_by_the_drift_and_holds_its_look() {
        let w = region(0.0, 0.0, 56.0, 30.0);
        let plan = |found: u64, next: Option<u64>, gone: Option<u64>| Planned {
            beat: found,
            next,
            found,
            gone,
            window: w,
        };
        // (the two that stand nowhere first: they don't count against `READS_MAX`)
        let reads = [
            plan(1_000, Some(1_000 + DRIFT_MS + 500), None),
            plan(5_000, None, Some(5_000 + DRIFT_MS + 500)),
            plan(10_000, None, None),
            plan(20_000, None, Some(20_000 + DRIFT_MS + READ_MS + 1_500)),
            plan(30_000, Some(30_000 + DRIFT_MS + 1_200), None),
        ];
        let course = course(&reads);
        assert_eq!(course.reads[0], None, "no room for its least");
        assert_eq!((course.reads[1], course.early.as_slice()), (None, &[1][..]), "gone too soon");
        assert_eq!(course.reads[2], Some(Read { into: 10_000, out: 10_000 + DRIFT_MS + READ_MS, short: 0 }));
        assert_eq!(course.reads[2].map(|r| r.there()), Some(10_000 + DRIFT_MS));
        assert_eq!(
            course.reads[3].map(|r| r.out),
            Some(20_000 + DRIFT_MS + READ_MS + 1_500),
            "until it goes"
        );
        assert_eq!(
            course.reads[4],
            Some(Read { into: 30_000, out: 30_000 + DRIFT_MS + 1_200, short: READ_MS - 1_200 }),
            "cut by the next beat"
        );
        assert_eq!(course.short, [(0, READ_MS - 500), (4, READ_MS - 1_200)]);
        assert_eq!(course.windows(&reads).first(), Some(&(10_000, 10_000 + DRIFT_MS + READ_MS, w)));
        // (a punchline marked in a read opens there; before the camera is there, as it arrives; after, its mark)
        assert_eq!(course.punch(10_000 + DRIFT_MS + 100), 10_000 + DRIFT_MS + 100);
        assert_eq!(course.punch(9_500), 10_000 + DRIFT_MS);
        assert_eq!(course.punch(60_000), 60_000);
    }

    /// A zoom is for emphasis only: a Short reads `READS_MAX` times at most — the rest stay with the captions, said; a
    /// read that stood nowhere (no room) isn't counted.
    #[test]
    fn a_short_reads_three_times_at_most() {
        let w = region(0.0, 0.0, 56.0, 30.0);
        let beat = |i: u64, next: u64| Planned {
            beat: i * 10_000,
            next: Some(i * 10_000 + next),
            found: i * 10_000,
            gone: None,
            window: w,
        };
        let reads = [
            beat(0, 10_000),
            beat(1, 1_000),
            beat(2, 10_000),
            beat(3, 10_000),
            beat(4, 10_000),
            beat(5, 10_000),
        ];
        let course = course(&reads);
        let stood: Vec<usize> =
            (course.reads.iter().enumerate()).filter(|r| r.1.is_some()).map(|r| r.0).collect();
        assert_eq!(stood, [0, 2, 3]);
        assert_eq!(course.over, [4, 5]);
    }

    /// Two screens cross where more than `CROSS` of their cells changed (text or ground): from the one before, the new
    /// one eased in and out over `CROSS_MS`; a smaller change (a key typed, a line written) cuts as it is.
    #[test]
    fn a_screen_mostly_changed_crosses_the_one_before() {
        let a = grid(&["aaaa", "aaaa"]);
        let mut b = grid(&["bbbb", "bbba"]);
        assert_eq!(changed(&a, &a), 0.0);
        assert_eq!(changed(&a, &b), 7.0 / 8.0);
        b.ground[1][3] = 4;
        assert_eq!(changed(&a, &b), 1.0, "a ground too");
        let typed = grid(&["aaab", "aaaa"]);
        assert_eq!(changed(&a, &typed), 1.0 / 8.0);
        let fades = crossfades(&[0.0, 0.1, 0.9, 0.5, 0.6]);
        assert_eq!(fades, [2, 4], "over half, not half");
        let times = [0, 1_000, 2_000, 2_050, 5_000];
        assert_eq!(crossing(&times, &fades, 1_999), None);
        assert_eq!(crossing(&times, &fades, 2_000), Some((1, 0.0)), "from the one before");
        let mid = crossing(&times, &fades, 2_000 + CROSS_MS / 2).unwrap();
        assert!(mid.0 == 1 && (mid.1 - 0.5).abs() < 0.01, "{mid:?}");
        assert!(crossing(&times, &fades, 2_000 + CROSS_MS / 10).unwrap().1 < 0.05, "eased in");
        assert_eq!(crossing(&times, &fades, 2_000 + CROSS_MS), None, "crossed");
        assert_eq!(crossing(&times, &fades, 5_100).map(|c| c.0), Some(3));
    }

    /// A shallow read (every one but a deep one) stands at a depth that reads (CLAUDE.md's Shorts bullet): `legible`
    /// columns wide — never the screen's half —, from the pane's left where the target fits so (here the screen's),
    /// else as near its middle as holds it whole, its middle row where the deep one's is; wider only for a wider target.
    /// A deep one the target's own shape.
    #[test]
    fn a_read_stands_at_a_depth_that_reads_and_a_deep_one_its_target() {
        let on = boxed(80, 43, &[]);
        let target = region(30.0, 20.0, 20.0, 4.0);
        let deep = framed(&on, target, 2.0, 0.5, 40.0, true);
        assert_eq!(deep, region(30.0, 17.0, 20.0, 10.0));
        let read = framed(&on, target, 2.0, 0.5, 40.0, false);
        assert_eq!(read, region(20.0, 12.0, 40.0, 20.0));
        let near = region(2.0, 20.0, 30.0, 4.0);
        assert_eq!(
            framed(&on, near, 2.0, 0.5, 40.0, false),
            region(0.0, 12.0, 40.0, 20.0),
            "from the pane's left"
        );
        // (a target wider than what reads: its own width)
        let wide = region(0.0, 20.0, 70.0, 4.0);
        assert_eq!(framed(&on, wide, 2.0, 0.5, 40.0, false).w, 70.0);
    }

    /// A shallow window keeps its pane's gutter: its left edge at the target's pane's left (past a side panel's divider)
    /// where the target fits that way, centred where it doesn't; a deep one centred on its target as ever.
    #[test]
    fn a_shallow_window_starts_at_its_panes_left() {
        let mut on = boxed(80, 43, &[]);
        on.text[0].fill("─".into());
        on.text[0][7] = "┬".into();
        for row in &mut on.text[1..] {
            row[7] = "│".into();
        }
        // (the code's lines at the pane's left: from the pane's left, not centred at 0; their air on the divider, from it)
        let near = region(8.0, 20.0, 31.0, 4.0);
        assert_eq!(pane_left(&on.text, near), 8);
        assert_eq!(framed(&on, near, 2.0, 0.5, 40.0, false).x, 8.0);
        assert!(keeps_panes_left(&on, near, framed(&on, near, 2.0, 0.5, 40.0, false)));
        assert_eq!(pane_left(&on.text, region(7.0, 20.0, 31.0, 4.0)), 7);
        let deep = framed(&on, near, 2.0, 0.5, 40.0, true);
        assert_eq!(deep.x, near.x, "a deep read its own");
        // (a target that wouldn't fit a window from the pane's left: centred, as near it as the screen lets)
        let far = region(50.0, 20.0, 28.0, 4.0);
        let window = framed(&on, far, 2.0, 0.5, 40.0, false);
        assert_eq!(window.x, 40.0);
        // (…the target whole, the pane's left not kept: said)
        assert!(window.x <= far.x && far.x + far.w <= window.x + window.w, "{window:?}");
        assert!(!keeps_panes_left(&on, far, window));
        // (no divider: the screen's edge)
        assert_eq!(pane_left(&boxed(80, 43, &[]).text, far), 0);
    }

    /// A card is wholly in a window or wholly out of it: V4's review (its lines to column 37) beside the follow card (its
    /// frame from column 40, rows 23–31, 38 wide) — a shallow window of 46 columns cuts the card wherever it stands and
    /// can't take it in, so it narrows a column at a time until the card is out (40, from the pane's left), its rows
    /// the window's shape; never narrower than its target — one that can't clear the card that way is cut as before.
    #[test]
    fn a_shallow_window_narrows_till_a_card_beside_it_is_wholly_out() {
        let follow: Card = (23, 40, 31, 77);
        let on = boxed(80, 34, &[follow]);
        let review = region(0.0, 11.0, 37.0, 13.0);
        let aspect = 46.0 / 20.5;
        let window = framed(&on, review, aspect, 0.5, 46.0, false);
        assert_eq!((window.x, window.w), (0.0, 40.0), "{window:?}");
        assert!(window.x + window.w <= follow.1 as f32, "the card wholly out");
        assert!(
            window.y <= review.y && review.y + review.h <= window.y + window.h,
            "the target whole: {window:?}"
        );
        assert!((window.w / window.h - aspect).abs() < 0.01, "the window's shape");
        // (a target reaching past the card's left: no narrower window clears it — 46 wide, the card cut as before)
        let wide = region(0.0, 11.0, 42.0, 13.0);
        assert_eq!(framed(&on, wide, aspect, 0.5, 46.0, false).w, 46.0);
        // (the card a column narrower on this screen, wider by the hold: clear of it as the hold draws it; a later card
        // over the target's own cells — a review's line lit — none to clear)
        let first = boxed(80, 34, &[(23, 41, 31, 77)]);
        let params = (aspect, 0.5, 46.0, false);
        assert_eq!(framed_among(&first, review, params, (&[], aspect)).w, 41.0, "this screen's alone");
        assert_eq!(framed_among(&first, review, params, (&[follow, (12, 0, 12, 44)], aspect)).w, 40.0);
        // (the frame drawing it 1.076× as wide as its shape — the whole's cells, its height binding: the columns it
        // shows are what clear the card, 0–41 exactly, the window centred in them)
        let drawn = aspect * 1.076;
        let w = framed_among(&first, review, params, (&[], drawn));
        let shown = (w.x + w.w / 2.0 - w.h * drawn / 2.0, w.x + w.w / 2.0 + w.h * drawn / 2.0);
        assert!(shown.0.abs() < 1e-3 && (shown.1 - 41.0).abs() < 1e-3, "{shown:?} {w:?}");
        let seen = super::shown(w, drawn);
        assert!(seen.x.abs() < 1e-3 && (seen.w - 41.0).abs() < 1e-3, "{seen:?}");
        assert!(keeps_panes_left(&first, review, seen), "the gutter shown from column 0");
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

    /// At the screen's top row the camera's region keeps its air by moving in.
    #[test]
    fn a_target_at_the_top_keeps_its_air_by_moving_in() {
        let on = grid(&["─ fix › src › main.rs ──", "", "  mod thread;"]);
        let framed = target(&on, "fix › src").expect("on screen");
        assert_eq!((framed.y, framed.h), (0.0, 3.0), "the camera's: its air moved in from the edge");
    }

    /// A callout's `{}` is what a framed card says on its first row inside its frame — the ask line's question as it's
    /// typed —; none where that row is blank or no card holds the target.
    #[test]
    fn a_cards_first_row_inside_its_frame_is_what_it_says() {
        let mut on = boxed(30, 6, &[(1, 2, 4, 25)]);
        let put = |on: &mut Cells, r: usize, c: usize, s: &str| {
            s.chars().enumerate().for_each(|(k, ch)| on.text[r][c + k] = ch.to_string())
        };
        put(&mut on, 1, 4, " ask claude ");
        put(&mut on, 2, 4, "› keep the first");
        assert_eq!(said_in(&on, "card:ask claude|44").as_deref(), Some("keep the first"));
        assert_eq!(said_in(&on, "card:nowhere"), None);
        put(&mut on, 2, 4, "›               ");
        assert_eq!(said_in(&on, "card:ask claude"), None, "nothing typed yet");
    }

    /// A card's lines that matter (`rows:<a> … <b>`, V3's card wider than a read): from the first of their texts to
    /// the longest's end, the rows between taken in, air above and below only — the card's own pad their air across, so
    /// a window as wide as the rows reads them whole; none where `<b>`'s row is above `<a>`'s or either isn't there.
    #[test]
    fn a_cards_lines_that_matter_are_read_without_its_frame() {
        let mut on = boxed(60, 13, &[(1, 4, 8, 57)]);
        let put = |on: &mut Cells, r: usize, c: usize, s: &str| {
            s.chars().enumerate().for_each(|(k, ch)| on.text[r][c + k] = ch.to_string())
        };
        put(&mut on, 1, 6, " main moved ");
        put(&mut on, 2, 6, "1 new commit — 2 workspaces behind it:");
        put(&mut on, 4, 8, "fix — 1 behind, would conflict in src/main.rs");
        put(&mut on, 5, 12, "L31");
        put(&mut on, 6, 8, "docs — 1 behind, its changed file goes with it");
        put(&mut on, 7, 6, "C-q answer · y move");
        let docs = "docs — 1 behind, its changed file goes with it".chars().count() as f32;
        assert_eq!(target(&on, "rows:fix — … docs —"), Some(region(8.0, 3.0, docs, 5.0)));
        assert_eq!(
            target(&on, "card:main moved"),
            Some(region(3.0, 0.0, 56.0, 10.0)),
            "the card whole, with air"
        );
        assert_eq!(target(&on, "rows:docs — … fix —"), None, "upward");
        assert_eq!(target(&on, "rows:fix — … nowhere"), None);
        // (rows in no card: each from past two blanks)
        put(&mut on, 10, 0, "} let k;  a b");
        put(&mut on, 11, 0, "y  c d e");
        assert_eq!(target(&on, "rows:a b … c d"), Some(region(3.0, 9.0, 10.0, 4.0)));
    }

    /// A target shaped to the window: the rows it adds slid off a list above it, so the list isn't cut — the placement
    /// nearest the eye's that cuts nothing; where every placement cuts a list close by, the list taken in whole; where
    /// taking it in would cost the zoom past `TAKEN_IN_MAX`, the window the target's own; and with room all round, the
    /// target's centre at the eye's height.
    #[test]
    fn a_framed_target_never_cuts_a_card() {
        let card: Card = (12, 20, 15, 39);
        let target = Region { x: 18.0, y: 11.0, w: 22.0, h: 6.0 };
        // (a window of 22 columns is 11 rows at 2 columns a row: rows 8–18 would cut the list's last row)
        let on = boxed(40, 20, &[(5, 10, 8, 30), card]);
        assert_eq!(framed(&on, target, 2.0, 0.5, 80.0, true), region(18.0, 9.0, 22.0, 11.0));
        // (every placement cuts a list just left of the card's columns: 2 more columns take it in)
        let on = boxed(40, 20, &[(7, 16, 10, 30), card]);
        assert_eq!(framed(&on, target, 2.0, 0.5, 80.0, true), region(16.0, 6.0, 24.0, 12.0));
        // (…unless those 2 columns are past what the window shows legibly: the target's own window)
        assert_eq!(framed(&on, target, 2.0, 0.5, 23.0, true), region(18.0, 8.0, 22.0, 11.0));
        // (…a list reaching far left: 30 columns, past 1.25 × 22 — the target's own window)
        let on = boxed(40, 20, &[(7, 10, 10, 30), card]);
        let window = framed(&on, target, 2.0, 0.5, 80.0, true);
        assert_eq!(window, region(18.0, 8.0, 22.0, 11.0));
        // (nothing to cut: the target's centre at the eye's height — half way, then a fifth of the way down)
        let on = boxed(40, 30, &[card]);
        assert_eq!(framed(&on, target, 2.0, 0.5, 80.0, true), region(18.0, 8.0, 22.0, 11.0));
        // (a fifth of the way down is above where the target fits whole: as near as it can, its top the window's)
        assert_eq!(framed(&on, target, 2.0, 0.2, 80.0, true), region(18.0, 11.0, 22.0, 11.0));
        // (a target of one row and its air: its centre (row 12) 2.2 rows down a window of 11)
        let row = Region { h: 2.0, ..target };
        assert_eq!(framed(&on, row, 2.0, 0.2, 80.0, true), region(18.0, 10.0, 22.0, 11.0));
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

    /// A target cut to `|N` columns of the card it's in (the ask line across the screen, its words at its left): no
    /// placement avoids that card, so the window still slides past another card it would cut (a list under it).
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
        let window = framed(&on, t, 2.0, 0.5, 80.0, true);
        assert_eq!(window, region(0.0, 3.0, 16.0, 8.0), "rows 2–9 would cut the list: slid to take it whole");
    }

    /// A news beat (`news:<text> over <target>`): its target the code's while the toast is up (none without it); the
    /// toast as it stands where the window has room for it, else laid again between its phrases (`src/main.rs L30 —`
    /// moves down whole, never `L30 —` left from its file); the window moved up so the callout and a row of air stand
    /// above the code.
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
        let r = news_framed(&on, "would conflict", code, (1.8, 0.3, 40.0, true));
        let n = callout(&on, "would conflict", r.w as usize - 2).expect("the toast").lines.len() as f32;
        assert!(r.y + 1.0 + n <= code.y, "the callout above the code: {r:?}, {n} lines, {code:?}");
        assert!(code.y + code.h <= r.y + r.h, "the code in the window: {r:?}");
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
