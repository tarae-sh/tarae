//! Picture cards (Z3's E3, `.sb/ztunnel-edit-plan-2.md` §3; the brief's §8): a drawing on the board — the frame above
//! the band, `BOARD` at 2160p, halved for 1080p —, the take stopped under it, drawn stroke by stroke in the order the
//! brush made it, the band under it with its line.
//!
//! - **A step is four files** beside each other: `NAME.png`, the whole picture so far; `NAME.ink.png`, its strokes and
//!   dabs on a clear ground (where it has strokes); `NAME.wash.png`, its washes and words (where it has fades); and
//!   `NAME.json`, what it adds, timed from its start (board px at 2160p, ms):
//!   `{"size":[3840,1860],"ms":2281,"strokes":[{"at":0,"ms":140,"r":24,"path":[[x,y],…]},…],"fades":[{"at":…,
//!   "ms":320,"rect":[x,y,w,h]},…]}`. The files are from the list's folder or a whole path, never in the repository.
//! - **Ink** comes in where the brush first comes within `r` of it (`reveal`: over every stroke, the earliest — a path
//!   under `DAB_PX` long is a dab, a disc growing over its `ms`), its leading end soft over `SOFT_MS`, laid `FRESH` of
//!   the way to the look's ink and dried over `DRY_MS` (`fresh = false`: as it is). **Wash**: a pixel's share is the
//!   strongest of the fades whose rectangle holds it. What no stroke reaches and no rectangle holds comes in at the
//!   step's `ms` over `FADE_MS` — what came at the end, counted. Settled, the board is `NAME.png` itself.
//! - **A frame's board** is a function of the step's clock and the files alone (`Held::board`): nothing carried from
//!   frame to frame, and no transcendental function (`smooth` is a polynomial, `sqrt` exact), so it is the same on Linux.
//! - **Checked** when the list is read (`Picture::parse`: what the list and the manifest say, each refused with the
//!   step's line and "step k") and before a frame is drawn (`check`: the files' sizes and corners, the layers making the
//!   step's picture over the one before — `MISS_MOST` px off by more than `MISS_BY` at the most —, and what came at the
//!   end — `END_MOST` of a layer's pixels above `FLOOR` at the most).
//! - **Held**: the step being drawn, and the next read on a worker while it draws (`Boards`) — never all of them.

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{CAPTION_MAX, Cut, Entry, FADE_MS, Marks, envelope, length, number, reading_ms};
use crate::Result;
use crate::camera::CROSS_MS;
use crate::scenes::docshots::Bg;
use crate::shot::Rgb;
use crate::video::{self, Scale};

/// The board at 2160p (px): the frame above the band. 1080p's is half.
pub(crate) const BOARD: (usize, usize) = (3840, 1860);
/// The leading end of a stroke comes up over this much of its clock (at 2600 px/s, the last ~130 px at 2160p).
pub(crate) const SOFT_MS: u64 = 50;
/// Fresh ink dries over this much of the video; a step stands at least this long after it's drawn.
pub(crate) const DRY_MS: u64 = 1_000;
/// How far fresh ink is laid toward the look's ink, easing to nothing as it dries.
pub(crate) const FRESH: f32 = 0.25;
/// The layers making the step's picture: a pixel more than this off on a channel is a miss…
pub(crate) const MISS_BY: u8 = 2;
/// …and this many misses at the most (0.1 % of the board).
pub(crate) const MISS_MOST: usize = BOARD.0 * BOARD.1 / 1000;
/// What came at the end counts only pixels whose alpha is above this (a stroke's blurred skirt past a tight `r`).
pub(crate) const FLOOR: u8 = 8;
/// What came at the end: this share of a layer's pixels above `FLOOR` at the most.
pub(crate) const END_MOST: f64 = 0.01;
/// A whole picture's corner: this far from the cut's ground on a channel at the most.
pub(crate) const CORNER_OFF: u8 = 3;
/// A step's `speed`: from, to.
pub(crate) const SPEEDS: (f64, f64) = (0.5, 3.0);
/// A stroke whose path is shorter than this (px at 2160p) is a dab.
pub(crate) const DAB_PX: f64 = 1.0;
/// No stroke reaches this pixel.
const NONE: u32 = u32::MAX;

const STEP_KEYS: [&str; 4] = ["file", "for", "speed", "text"];
const STEPS: &str = "steps = [{ file = \"pictures/1-room-1.png\", for = \"2s\" }, …]";

/// A stroke: from `at` (ms of its step), drawn over `ms`, showing the ink within `r` (px at 2160p) of its path.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Stroke {
    pub(crate) at: u64,
    pub(crate) ms: u64,
    pub(crate) r: f64,
    pub(crate) path: Vec<(f64, f64)>,
}

/// A fade: from `at` (ms of its step), over `ms`, bringing in the wash inside `rect` (x, y, w, h; px at 2160p).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Fade {
    pub(crate) at: u64,
    pub(crate) ms: u64,
    pub(crate) rect: [f64; 4],
}

/// A step: its whole picture (`file`, `NAME.png`), standing `hold` ms after it's drawn, its drawing `speed` times as
/// quick as the brush made it, its text (a row of lines, each a share of the step), the list's line; and its manifest —
/// its drawing's `ms`, strokes and fades.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Step {
    pub(crate) file: PathBuf,
    pub(crate) name: String,
    pub(crate) hold: u64,
    pub(crate) speed: f64,
    pub(crate) text: Vec<String>,
    pub(crate) line: usize,
    pub(crate) ms: u64,
    pub(crate) strokes: Vec<Stroke>,
    pub(crate) fades: Vec<Fade>,
}

/// A picture: at the take's `at` (ms), its `[[picture]]`'s line, fresh ink or not, its steps.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Picture {
    pub(crate) at: u64,
    pub(crate) line: usize,
    pub(crate) fresh: bool,
    pub(crate) steps: Vec<Step>,
}

/// What the board shows in a frame: picture `n`'s step `k` (`n` None: the opening's picture, whole), the step's clock
/// (µs of its manifest; None once settled) and its text's line standing (which, how drawn of 255).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shows {
    pub(crate) n: Option<usize>,
    pub(crate) k: usize,
    pub(crate) t: Option<u64>,
    pub(crate) line: Option<(usize, u8)>,
}

/// `us` up to whole tenths of a second — whole frames (`HOLD_STEP`'s), so every frame after it moves by whole frames.
fn tenths(us: u64) -> u64 {
    us.div_ceil(100_000) * 100_000
}

impl Step {
    /// Its drawing in the video (µs, whole tenths).
    pub(crate) fn drawing(&self) -> u64 {
        tenths((self.ms as f64 * 1000.0 / self.speed).ceil() as u64)
    }

    /// Its stand after the drawing (µs, whole tenths).
    pub(crate) fn standing(&self) -> u64 {
        tenths(self.hold * 1000)
    }

    /// Its length in the video (µs): drawn, then standing.
    pub(crate) fn len(&self) -> u64 {
        self.drawing() + self.standing()
    }

    /// Its clock (µs of its manifest) `since` µs of video into it.
    pub(crate) fn clock(&self, since: u64) -> u64 {
        (since as f64 * self.speed).round() as u64
    }

    /// Where on its clock it's settled (µs): its `ms`, then fresh ink dried (`DRY_MS` of video) — without, what came at
    /// the end come in (`FADE_MS`).
    pub(crate) fn settles(&self, fresh: bool) -> u64 {
        let after = if fresh { DRY_MS } else { FADE_MS };
        self.ms * 1000 + (after as f64 * 1000.0 * self.speed).round() as u64
    }

    /// Half through its strokes on its clock (µs): between the first's start and the last's end — its fades' where it
    /// has none.
    pub(crate) fn halfway(&self) -> u64 {
        let spans: Vec<(u64, u64)> = match self.strokes.is_empty() {
            false => self.strokes.iter().map(|s| (s.at, s.at + s.ms)).collect(),
            true => self.fades.iter().map(|f| (f.at, f.at + f.ms)).collect(),
        };
        let a = spans.iter().map(|s| s.0).min().unwrap_or(0);
        let b = spans.iter().map(|s| s.1).max().unwrap_or(self.ms);
        (a + b) * 500
    }

    /// A layer's file beside its whole: `NAME.ink.png`, `NAME.wash.png`.
    pub(crate) fn layer(&self, what: &str) -> PathBuf {
        self.file.with_file_name(format!("{}.{what}.png", self.name))
    }
}

impl Picture {
    /// Its length in the video (µs): the crossing in, then each step drawn and standing.
    pub(crate) fn len(&self) -> u64 {
        CROSS_MS * 1000 + self.steps.iter().map(Step::len).sum::<u64>()
    }

    /// Where each step starts, µs after the picture's first frame: after the crossing in, back to back.
    pub(crate) fn starts(&self) -> Vec<u64> {
        (self.steps.iter())
            .scan(CROSS_MS * 1000, |at, s| {
                let now = *at;
                *at += s.len();
                Some(now)
            })
            .collect()
    }

    /// What the board shows `off` µs after the picture's first frame (`Shows` but `n`): through the crossing in, the
    /// first step at its start (the ground: nothing drawn yet).
    pub(crate) fn at(&self, off: u64) -> (usize, Option<u64>, Option<(usize, u8)>) {
        let starts = self.starts();
        let k = starts.partition_point(|&s| s <= off).saturating_sub(1);
        if off < starts[0] {
            return (0, Some(0), None);
        }
        let (s, since) = (&self.steps[k], off - starts[k]);
        let t = Some(s.clock(since)).filter(|&t| t < s.settles(self.fresh));
        // (a row's lines: each a share of the step, the last to its end)
        let line = match s.text.len() as u64 {
            0 => None,
            n => {
                let (len, each) = (s.len(), s.len() / n);
                let j = (since / each).min(n - 1);
                let end = if j + 1 == n { len } else { (j + 1) * each };
                Some((j as usize, envelope(since - j * each, end - j * each))).filter(|l| l.1 > 0)
            }
        };
        (k, t, line)
    }

    /// A `[[picture]]` (`e`), its files from `dir` or whole paths — refused, each with its line: what the table and its
    /// steps say, a step's file not there or in the repository, what its manifest says, a layer it needs not there.
    pub(super) fn parse(e: &Entry, marks: &Marks, dir: &Path) -> Result<Picture> {
        e.keys(&["at", "steps", "fresh"])?;
        let at = e.time("at", marks)?;
        let fresh = match e.t.get("fresh") {
            None => true,
            Some(toml::Value::Boolean(b)) => *b,
            Some(_) => {
                return Err(e.err("fresh", "picture: fresh = true or false (fresh ink drying, or as it is)"));
            }
        };
        let list = match e.t.get("steps") {
            None => return Err(e.src.at(e.line, format!("picture: no steps ({STEPS})"))),
            Some(toml::Value::Array(a)) if a.is_empty() => {
                return Err(e.err("steps", "picture: steps = [] — a step at least"));
            }
            Some(toml::Value::Array(a)) => a,
            Some(_) => return Err(e.err("steps", format!("picture: {STEPS}"))),
        };
        let steps = (list.iter().enumerate())
            .map(|(k, v)| Step::parse(e, k, v, fresh, dir))
            .collect::<Result<Vec<Step>>>()?;
        Ok(Picture { at, line: e.line, fresh, steps })
    }
}

/// The line of a `[[picture]]`'s step `k`: its `k`th line opening a table (`{`) under `steps`, else `steps`' own.
fn step_line(e: &Entry, k: usize) -> usize {
    let from = e.line("steps");
    (e.src.lines.iter().enumerate().skip(from))
        .take_while(|(_, l)| !l.trim_start().starts_with('['))
        .filter(|(_, l)| l.trim_start().starts_with('{'))
        .nth(k)
        .map_or(from, |(i, _)| i + 1)
}

/// Whether `path` is inside this checkout but not under its `.sb/`: pictures are never in the repository.
fn in_repository(path: &Path) -> bool {
    let (Ok(p), Ok(root)) = (path.canonicalize(), crate::root().canonicalize()) else { return false };
    p.starts_with(&root) && !p.starts_with(root.join(".sb"))
}

/// A picture's file as the list names it, from `dir` or a whole path: there, a PNG, never in the repository.
pub(crate) fn file_of(name: &str, dir: &Path) -> std::result::Result<PathBuf, String> {
    let path = match Path::new(name).is_absolute() {
        true => PathBuf::from(name),
        false => dir.join(name),
    };
    if !path.is_file() {
        return Err(format!("{} not there", path.display()));
    }
    if in_repository(&path) {
        return Err(format!(
            "{} is in the repository — pictures are kept outside it (or under .sb/)",
            path.display()
        ));
    }
    Ok(path)
}

/// A manifest's whole number (ms, px).
fn whole(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f.round() as u64))
}

impl Step {
    /// Step `k` of the `[[picture]]` `e` (`v`), refused with its line and "step k".
    fn parse(e: &Entry, k: usize, v: &toml::Value, fresh: bool, dir: &Path) -> Result<Step> {
        let line = step_line(e, k);
        let at = |said: String| e.src.at(line, said);
        let who = format!("picture, step {}", k + 1);
        let Some(t) = v.as_table() else { return Err(at(format!("{who}: a table — {STEPS}"))) };
        if let Some(key) = t.keys().find(|x| !STEP_KEYS.contains(&x.as_str())) {
            return Err(at(format!("{who}: no key {key} (it takes {})", STEP_KEYS.join(", "))));
        }
        let file = match t.get("file") {
            None => return Err(at(format!("{who}: no file"))),
            Some(toml::Value::String(s)) => s.trim().to_string(),
            Some(_) => return Err(at(format!("{who}: file = \"pictures/1-room-1.png\", a string"))),
        };
        let name = (Path::new(&file).file_name().and_then(|n| n.to_str()))
            .and_then(|n| n.strip_suffix(".png"))
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                at(format!("{who}: file = \"{file}\" — the step's whole picture, NAME.png (its layers and manifest beside it)"))
            })?;
        let who = format!("{who} ({name})");
        let hold = match t.get("for") {
            None => return Err(at(format!("{who}: no for (how long it stands after it's drawn)"))),
            Some(toml::Value::String(s)) => length(s).map_err(|x| at(format!("{who}: for {x}")))?,
            Some(_) => return Err(at(format!("{who}: for = \"2s\", a length"))),
        };
        let speed = match t.get("speed") {
            None => 1.0,
            Some(v) => match number(v) {
                Some(s) if (SPEEDS.0..=SPEEDS.1).contains(&s) => s,
                _ => {
                    return Err(at(format!(
                        "{who}: speed = {} to {} — its drawing that many times as quick (1: as the brush made it)",
                        SPEEDS.0, SPEEDS.1
                    )));
                }
            },
        };
        let text: Vec<String> = match t.get("text") {
            None => Vec::new(),
            Some(toml::Value::String(s)) => vec![s.trim().to_string()],
            Some(toml::Value::Array(a)) if !a.is_empty() => (a.iter().enumerate())
                .map(|(j, v)| {
                    (v.as_str().map(|s| s.trim().to_string()))
                        .ok_or_else(|| at(format!("{who}: the text's line {}: a string", j + 1)))
                })
                .collect::<Result<_>>()?,
            Some(_) => return Err(at(format!("{who}: text = a line, or a row of lines"))),
        };
        let floor = if fresh { DRY_MS } else { FADE_MS };
        if hold < floor {
            let why = match fresh {
                true => format!("fresh ink dries over {:.1} s after it", DRY_MS as f64 / 1e3),
                false => format!("what came at the end comes in over {FADE_MS} ms after it"),
            };
            return Err(at(format!(
                "{who}: stands {:.2} s after it's drawn — {why}: for = \"{floor}ms\" at the least",
                hold as f64 / 1e3
            )));
        }
        let file = file_of(&file, dir).map_err(|x| at(format!("{who}: {x}")))?;
        let json = file.with_file_name(format!("{name}.json"));
        let (ms, strokes, fades) = manifest(&json, &|said| at(format!("{who}: {name}.json: {said}")))?;
        let step = Step { file, name, hold, speed, text, line, ms, strokes, fades };
        for (what, needed) in [("ink", !step.strokes.is_empty()), ("wash", !step.fades.is_empty())] {
            let p = step.layer(what);
            if needed && !p.is_file() {
                let has = if what == "ink" { "strokes" } else { "fades" };
                return Err(at(format!(
                    "{who}: it has {has} and no {what} layer — {} not there",
                    p.display()
                )));
            }
        }
        let n = step.text.len() as u64;
        for (j, l) in step.text.iter().enumerate() {
            let who = match n {
                1 => who.clone(),
                n => format!("{who}, line {} of {n}", j + 1),
            };
            let c = l.chars().count();
            if c == 0 || c > CAPTION_MAX || l.contains('\n') {
                return Err(at(format!("{who}: one line of 1 to {CAPTION_MAX} characters ({c} here)")));
            }
            let (share, need) = (step.len() / n / 1000, reading_ms(c));
            if share < need {
                return Err(at(format!(
                    "{who}: stands {:.2} s (its share of the step's {:.2} s, drawn and standing) — {c} characters need \
                     {:.2} s to be read",
                    share as f64 / 1e3,
                    step.len() as f64 / 1e6,
                    need as f64 / 1e3
                )));
            }
        }
        Ok(step)
    }
}

/// A step's manifest (`path`): its drawing's ms, strokes and fades — refused (`at`) where it can't be read, its size
/// isn't the board's, a stroke or fade runs past its ms or has none, a stroke's `r` isn't above 0 or its path is
/// empty, a point or rectangle is outside the board.
fn manifest(path: &Path, at: &dyn Fn(String) -> String) -> Result<(u64, Vec<Stroke>, Vec<Fade>)> {
    let text = std::fs::read_to_string(path).map_err(|x| at(x.to_string()))?;
    let v: Value = serde_json::from_str(&text).map_err(|x| at(x.to_string()))?;
    let (bw, bh) = (BOARD.0 as f64, BOARD.1 as f64);
    match v["size"].as_array().map(|a| a.iter().map(whole).collect::<Vec<_>>()).as_deref() {
        Some([Some(w), Some(h)]) if (*w as usize, *h as usize) == BOARD => {}
        Some([Some(w), Some(h)]) => {
            return Err(at(format!(
                "its size is {w}×{h} — the board is {}×{} (made at 2160p)",
                BOARD.0, BOARD.1
            )));
        }
        _ => return Err(at(format!("no size = [{}, {}]", BOARD.0, BOARD.1))),
    }
    let ms = whole(&v["ms"])
        .filter(|&m| m > 0)
        .ok_or_else(|| at("no ms above 0 (how long its drawing runs)".into()))?;
    let list = |key: &str| match &v[key] {
        Value::Null => Ok(Vec::new()),
        Value::Array(a) => Ok(a.clone()),
        _ => Err(at(format!("{key}: a list"))),
    };
    let timed = |what: &str, s: &Value| -> Result<(u64, u64)> {
        let (a, m) = (whole(&s["at"]), whole(&s["ms"]));
        match (a, m) {
            (Some(_), Some(0)) => Err(at(format!("{what}: ms = 0 — it's drawn over its ms (a dab's too)"))),
            (Some(a), Some(m)) if a + m > ms => {
                Err(at(format!("{what} runs to {} ms, past the step's ms ({ms})", a + m)))
            }
            (Some(a), Some(m)) => Ok((a, m)),
            _ => Err(at(format!("{what}: no at or ms"))),
        }
    };
    let mut strokes = Vec::new();
    for (i, s) in list("strokes")?.iter().enumerate() {
        let what = format!("stroke {}", i + 1);
        let (a, m) = timed(&what, s)?;
        let r = s["r"].as_f64().unwrap_or(0.0);
        if r <= 0.0 {
            return Err(at(format!("{what}: r = {r} — its half width and edge, above 0")));
        }
        let mut path = Vec::new();
        for p in s["path"].as_array().into_iter().flatten() {
            let (x, y) = (p[0].as_f64(), p[1].as_f64());
            match (x, y) {
                (Some(x), Some(y)) if (0.0..=bw).contains(&x) && (0.0..=bh).contains(&y) => path.push((x, y)),
                (Some(x), Some(y)) => {
                    return Err(at(format!("{what}: its point ({x}, {y}) is outside the board ({bw}×{bh})")));
                }
                _ => return Err(at(format!("{what}: its path's points are [x, y]"))),
            }
        }
        if path.is_empty() {
            return Err(at(format!("{what}: no path")));
        }
        strokes.push(Stroke { at: a, ms: m, r, path });
    }
    let mut fades = Vec::new();
    for (i, f) in list("fades")?.iter().enumerate() {
        let what = format!("fade {}", i + 1);
        let (a, m) = timed(&what, f)?;
        let r: Vec<f64> = f["rect"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
        let [x, y, w, h] = r[..] else { return Err(at(format!("{what}: rect = [x, y, w, h]"))) };
        if x < 0.0 || y < 0.0 || w <= 0.0 || h <= 0.0 || x + w > bw || y + h > bh {
            return Err(at(format!(
                "{what}: its rectangle [{x}, {y}, {w}, {h}] is outside the board ({bw}×{bh})"
            )));
        }
        fades.push(Fade { at: a, ms: m, rect: [x, y, w, h] });
    }
    Ok((ms, strokes, fades))
}

// ── The reveal ──────────────────────────────────────────────────────────────────────────────────────────────────────────

/// Each pixel's time on the step's clock (µs) where the brush first comes within `r` of it, over every stroke — on a
/// `dims` board at `s` of 2160p's (coordinates and `r` too), only where `ink` (RGBA) has ink; `NONE` where no stroke
/// reaches. A segment A→B at its share of the stroke's length: the smaller root `u` of |p − A − u(B − A)| = r (0 where
/// p is within r of A); a dab's disc grows from its point over its ms.
pub(crate) fn reveal(step: &Step, ink: &[u8], dims: (usize, usize), s: f64) -> Vec<u32> {
    let (w, h) = dims;
    let mut t0 = vec![NONE; w * h];
    // (the pixels of a box padded by `r` round a..b, inside the board)
    let span = |lo: f64, hi: f64, r: f64, n: usize| {
        let a = (lo - r).floor().max(0.0) as usize;
        let b = ((hi + r).ceil().max(0.0) as usize + 1).min(n);
        a..b
    };
    for st in &step.strokes {
        let r = st.r * s;
        let rr = r * r;
        let pts: Vec<(f64, f64)> = st.path.iter().map(|p| (p.0 * s, p.1 * s)).collect();
        let mut along = vec![0.0];
        for q in pts.windows(2) {
            let d = ((q[1].0 - q[0].0).powi(2) + (q[1].1 - q[0].1).powi(2)).sqrt();
            along.push(along.last().copied().unwrap_or(0.0) + d);
        }
        let total = along.last().copied().unwrap_or(0.0);
        let (at, ms) = (st.at as f64 * 1000.0, st.ms as f64 * 1000.0);
        let mut put = |x: usize, y: usize, t: f64| {
            let i = y * w + x;
            let t = t.round() as u32;
            if ink[i * 4 + 3] > 0 && t < t0[i] {
                t0[i] = t;
            }
        };
        if total < DAB_PX * s {
            let (cx, cy) = pts[0];
            for y in span(cy, cy, r, h) {
                for x in span(cx, cx, r, w) {
                    let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
                    if d <= r {
                        put(x, y, at + ms * d / r);
                    }
                }
            }
            continue;
        }
        for (i, q) in pts.windows(2).enumerate() {
            let ((ax, ay), (bx, by)) = (q[0], q[1]);
            let (dx, dy) = (bx - ax, by - ay);
            let (dd, seg) = (dx * dx + dy * dy, along[i + 1] - along[i]);
            for y in span(ay.min(by), ay.max(by), r, h) {
                for x in span(ax.min(bx), ax.max(bx), r, w) {
                    let (fx, fy) = (ax - (x as f64 + 0.5), ay - (y as f64 + 0.5));
                    let ff = fx * fx + fy * fy;
                    let u = if ff <= rr {
                        0.0
                    } else if dd == 0.0 {
                        continue;
                    } else {
                        let fd = fx * dx + fy * dy;
                        let disc = fd * fd - dd * (ff - rr);
                        if disc < 0.0 {
                            continue;
                        }
                        let u = (-fd - disc.sqrt()) / dd;
                        if !(0.0..=1.0).contains(&u) {
                            continue;
                        }
                        u
                    };
                    put(x, y, at + ms * (along[i] + u * seg) / total);
                }
            }
        }
    }
    t0
}

/// Which pixels of a `dims` board at `s` of 2160p's a fade's rectangle holds, and the rectangles there (x0, y0, x1, y1).
pub(crate) fn cover(step: &Step, dims: (usize, usize), s: f64) -> (Vec<bool>, Vec<[usize; 4]>) {
    let (w, h) = dims;
    let rects: Vec<[usize; 4]> = (step.fades.iter())
        .map(|f| {
            let [x, y, rw, rh] = f.rect;
            let at = |v: f64, n: usize| ((v * s).ceil() as usize).min(n);
            [(x * s).floor() as usize, (y * s).floor() as usize, at(x + rw, w), at(y + rh, h)]
        })
        .collect();
    let mut held = vec![false; w * h];
    for r in &rects {
        for y in r[1]..r[3] {
            held[y * w + r[0]..y * w + r[2]].fill(true);
        }
    }
    (held, rects)
}

/// `x / len` eased (3u² − 2u³), 0 before and 1 after: a polynomial, the same everywhere.
fn smooth(x: f64, len: f64) -> f32 {
    let u = (x / len).clamp(0.0, 1.0);
    (u * u * (3.0 - 2.0 * u)) as f32
}

/// `src` (straight alpha `a`) laid over `dst` at `share` of its alpha, its colour `lift` of the way to `full`.
fn lay(dst: &mut [u8], src: &[u8], share: f32, lift: f32, full: Rgb) {
    let k = f32::from(src[3]) / 255.0 * share;
    if k <= 0.0 {
        return;
    }
    for c in 0..3 {
        let s = f32::from(src[c]) + (f32::from(full[c]) - f32::from(src[c])) * lift;
        let d = f32::from(dst[c]);
        dst[c] = (d + (s - d) * k).round().clamp(0.0, 255.0) as u8;
    }
}

/// The board before a step with its wash, then its ink, laid whole: what its picture must be.
pub(crate) fn made(before: &[u8], wash: Option<&[u8]>, ink: Option<&[u8]>) -> Vec<u8> {
    let mut out = before.to_vec();
    for layer in [wash, ink].into_iter().flatten() {
        for (d, s) in out.as_chunks_mut::<3>().0.iter_mut().zip(layer.as_chunks::<4>().0) {
            lay(d, s, 1.0, 0.0, [0; 3]);
        }
    }
    out
}

/// What a step's files say against each other (`measure`): the pixels its layers miss its picture by (more than
/// `MISS_BY`) and the first (x, y); what came at the end of its ink and of its wash (pixels above `FLOOR` no stroke
/// reaches / no rectangle holds, of those above it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Found {
    pub(crate) missed: usize,
    pub(crate) first: Option<(usize, usize)>,
    pub(crate) ink: (usize, usize),
    pub(crate) wash: (usize, usize),
}

/// A step's files measured on a `dims` board at `s` of 2160p's: its layers over the board `before` against its
/// `whole` (RGB), and what came at the end.
pub(crate) fn measure(
    step: &Step,
    (before, whole): (&[u8], &[u8]),
    (ink, wash): (Option<&[u8]>, Option<&[u8]>),
    dims: (usize, usize),
    s: f64,
) -> Found {
    let got = made(before, wash, ink);
    let off = |(a, b): (&[u8; 3], &[u8; 3])| a.iter().zip(b).any(|(x, y)| x.abs_diff(*y) > MISS_BY);
    let pairs = got.as_chunks::<3>().0.iter().zip(whole.as_chunks::<3>().0);
    let mut misses = pairs.enumerate().filter(|(_, p)| off(*p));
    let first = misses.next().map(|(i, _)| (i % dims.0, i / dims.0));
    let missed = first.map_or(0, |_| 1 + misses.count());
    let late = |layer: Option<&[u8]>, reached: &dyn Fn(usize) -> bool| {
        let px = layer.map_or(&[][..], |l| l);
        let over: Vec<usize> = (0..px.len() / 4).filter(|&i| px[i * 4 + 3] > FLOOR).collect();
        (over.iter().filter(|&&i| !reached(i)).count(), over.len())
    };
    let t0 = ink.map_or(Vec::new(), |i| reveal(step, i, dims, s));
    let (held, _) = cover(step, dims, s);
    Found { missed, first, ink: late(ink, &|i| t0[i] != NONE), wash: late(wash, &|i| held[i]) }
}

// ── The files ───────────────────────────────────────────────────────────────────────────────────────────────────────────

/// A whole picture: 8-bit RGB, or RGBA laid on `ground` — its size first (`size`, where it's said).
fn whole_png(path: &Path, ground: Rgb, size: Option<(usize, usize)>) -> std::result::Result<Vec<u8>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().map_err(|e| e.to_string())?;
    let (w, h) = (reader.info().width as usize, reader.info().height as usize);
    if let Some((sw, sh)) = size.filter(|s| *s != (w, h)) {
        return Err(format!("it is {w}×{h}, not {sw}×{sh}"));
    }
    let mut px = vec![0; reader.output_buffer_size().ok_or("too big")?];
    let info = reader.next_frame(&mut px).map_err(|e| e.to_string())?;
    px.truncate(info.buffer_size());
    match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgb, png::BitDepth::Eight) => Ok(px),
        (png::ColorType::Rgba, png::BitDepth::Eight) => Ok(made(&ground.repeat(w * h), None, Some(&px))),
        _ => Err("not 8-bit RGB or RGBA".into()),
    }
}

/// A layer: 8-bit RGBA of the board's size, its corner clear.
fn layer_png(path: &Path) -> std::result::Result<Vec<u8>, String> {
    let (w, h, px) = video::rgba(path).map_err(|e| e.rsplit(": ").next().unwrap_or(&e).to_string())?;
    if (w, h) != BOARD {
        return Err(format!("it is {w}×{h}, not {}×{}", BOARD.0, BOARD.1));
    }
    if px[3] != 0 {
        return Err(format!(
            "its corner isn't clear (alpha {}) — a layer is its strokes on a clear ground",
            px[3]
        ));
    }
    Ok(px)
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// A whole picture checked: the board's size, its corner the cut's ground (`bg`'s).
fn checked_whole(path: &Path, bg: Bg, ground: Rgb) -> std::result::Result<Vec<u8>, String> {
    let name = path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let px = whole_png(path, ground, Some(BOARD)).map_err(|e| format!("{name}: {e}"))?;
    let corner = [px[0], px[1], px[2]];
    if corner.iter().zip(ground).any(|(a, b)| a.abs_diff(b) > CORNER_OFF) {
        let look = if bg == Bg::Dark { "meok" } else { "hanji" };
        return Err(format!(
            "{name}: its corner is {} — not this cut's ground, {look}'s {} (made for one ground)",
            hex(corner),
            hex(ground)
        ));
    }
    Ok(px)
}

/// Every picture's files checked at 2160p before a frame is drawn — refused, each with its step's line and "step k":
/// a whole not the board's size or its corner not the cut's ground; a layer not 8-bit RGBA, not the board's size or its
/// corner not clear; the layers not making the step's picture over the one before; what came at the end. The
/// opening's picture: its size and corner. What each step's files said (`Found`).
pub(crate) fn check(cut: &Cut, ground: Rgb) -> Result<Vec<Vec<Found>>> {
    let mut all = Vec::new();
    for p in &cut.pictures {
        let mut before = ground.repeat(BOARD.0 * BOARD.1);
        let mut found = Vec::new();
        for (k, s) in p.steps.iter().enumerate() {
            let refuse = |said: String| {
                format!("{}:{}: picture, step {} ({}): {said}", cut.file, s.line, k + 1, s.name)
            };
            let whole = checked_whole(&s.file, cut.bg, ground).map_err(refuse)?;
            let layer = |what: &str, has: bool| -> Result<Option<Vec<u8>>> {
                let p = s.layer(what);
                match has {
                    false => Ok(None),
                    true => {
                        layer_png(&p).map(Some).map_err(|e| refuse(format!("{}.{what}.png: {e}", s.name)))
                    }
                }
            };
            let (ink, wash) = (layer("ink", !s.strokes.is_empty())?, layer("wash", !s.fades.is_empty())?);
            let f = measure(s, (&before, &whole), (ink.as_deref(), wash.as_deref()), BOARD, 1.0);
            if f.missed > MISS_MOST {
                let (x, y) = f.first.unwrap_or_default();
                return Err(refuse(format!(
                    "its layers miss {}.png on {} px (more than {MISS_BY}/255 off), the first at ({x}, {y}) — {MISS_MOST} \
                     at the most: wash then ink over the step before must make it",
                    s.name, f.missed
                )));
            }
            for (what, (late, of), why) in
                [("ink", f.ink, "no stroke within its r"), ("wash", f.wash, "in no fade's rectangle")]
            {
                if late as f64 > of as f64 * END_MOST {
                    return Err(refuse(format!(
                        "what came at the end: {late} of its {of} {what} px above alpha {FLOOR} ({:.1} %) — {why}; {:.0} % \
                         at the most",
                        100.0 * late as f64 / of as f64,
                        100.0 * END_MOST
                    )));
                }
            }
            found.push(f);
            before = whole;
        }
        all.push(found);
    }
    if let Some((path, line)) = &cut.opening.picture {
        checked_whole(path, cut.bg, ground)
            .map_err(|e| format!("{}:{line}: opening: picture: {e}", cut.file))?;
    }
    Ok(all)
}

// ── The boards a cut's frames draw ──────────────────────────────────────────────────────────────────────────────────────

/// A step's pictures at the video's size (`dims`, `s` of 2160p's), held while it's drawn: the board before it and its
/// whole (RGB), its layers (RGBA, None where it has none), each ink pixel's time (`reveal`), the pixels and
/// rectangles of its fades (`cover`).
pub(crate) struct Held {
    key: (usize, usize),
    w: usize,
    before: Vec<u8>,
    whole: Vec<u8>,
    ink: Option<Vec<u8>>,
    wash: Option<Vec<u8>>,
    t0: Vec<u32>,
    held: Vec<bool>,
    rects: Vec<[usize; 4]>,
}

/// A board of 2160p's (RGB, RGBA: `n` bytes a pixel) at `s` of its size.
fn sized(px: Vec<u8>, n: usize, s: f64) -> Vec<u8> {
    match (s < 1.0, n) {
        (false, _) => px,
        (true, 3) => video::half(&px, BOARD).2,
        (true, _) => video::shrunk(&(BOARD.0, BOARD.1, px), 0.5).2,
    }
}

impl Held {
    /// Step `key` (`step`) read at `s` of 2160p's over `before` (at that size).
    pub(crate) fn load(
        step: &Step,
        key: (usize, usize),
        before: Vec<u8>,
        (s, ground): (f64, Rgb),
    ) -> std::result::Result<Held, String> {
        let dims = ((BOARD.0 as f64 * s) as usize, (BOARD.1 as f64 * s) as usize);
        let said = |e: String| format!("{}: {e}", step.file.display());
        let whole = sized(whole_png(&step.file, ground, Some(BOARD)).map_err(said)?, 3, s);
        let layer = |what: &str, has: bool| -> std::result::Result<Option<Vec<u8>>, String> {
            match has {
                false => Ok(None),
                true => layer_png(&step.layer(what)).map(|p| Some(sized(p, 4, s))).map_err(said),
            }
        };
        let (ink, wash) = (layer("ink", !step.strokes.is_empty())?, layer("wash", !step.fades.is_empty())?);
        let t0 = ink.as_deref().map_or(Vec::new(), |i| reveal(step, i, dims, s));
        let (held, rects) = cover(step, dims, s);
        Ok(Held { key, w: dims.0, before, whole, ink, wash, t0, held, rects })
    }

    /// The board at the step's clock `t` (µs; None: settled, its whole picture): the board before it, its wash at each
    /// pixel's share (the strongest running fade whose rectangle holds it), its ink at each pixel's (`smooth` from its
    /// time over `SOFT_MS`, `fresh`: lifted toward `full`, drying over `DRY_MS`); what no stroke reaches and no
    /// rectangle holds from the step's ms over `FADE_MS`.
    pub(crate) fn board(&self, step: &Step, t: Option<u64>, fresh: bool, full: Rgb) -> Vec<u8> {
        let Some(t) = t else { return self.whole.clone() };
        let t = t as f64;
        let n = self.before.len() / 3;
        let mut out = self.before.clone();
        let late = smooth(t - step.ms as f64 * 1000.0, FADE_MS as f64 * 1000.0 * step.speed);
        if let Some(wash) = &self.wash {
            let width = self.w;
            let mut share = vec![0f32; n];
            for (f, r) in step.fades.iter().zip(&self.rects) {
                let a = smooth(t - f.at as f64 * 1000.0, f.ms as f64 * 1000.0);
                if a > 0.0 {
                    for y in r[1]..r[3] {
                        for v in &mut share[y * width + r[0]..y * width + r[2]] {
                            *v = v.max(a);
                        }
                    }
                }
            }
            for (i, (d, s)) in out.as_chunks_mut::<3>().0.iter_mut().zip(wash.as_chunks::<4>().0).enumerate()
            {
                if s[3] > 0 {
                    lay(d, s, if self.held[i] { share[i] } else { late }, 0.0, full);
                }
            }
        }
        if let Some(ink) = &self.ink {
            let (soft, dry) = (SOFT_MS as f64 * 1000.0, DRY_MS as f64 * 1000.0 * step.speed);
            for (i, (d, s)) in out.as_chunks_mut::<3>().0.iter_mut().zip(ink.as_chunks::<4>().0).enumerate() {
                if s[3] == 0 {
                    continue;
                }
                let (share, lift) = match self.t0[i] {
                    NONE => (late, 0.0),
                    t0 => {
                        let since = t - f64::from(t0);
                        let lift =
                            if fresh && since >= 0.0 { FRESH * (1.0 - smooth(since, dry)) } else { 0.0 };
                        (smooth(since, soft), lift)
                    }
                };
                lay(d, s, share, lift, full);
            }
        }
        out
    }
}

/// A step being read on a worker: which, and the reading.
type Reading = ((usize, usize), JoinHandle<std::result::Result<Held, String>>);

/// The boards a cut's frames draw, at the video's size: the step being drawn (`Held`), the next read on a worker while
/// it draws, the opening's picture once; how many steps were read and how long a frame waited for one; what went
/// wrong reading one, said after the frames.
pub(crate) struct Boards<'c> {
    cut: &'c Cut,
    s: f64,
    ground: Rgb,
    full: Rgb,
    held: Option<Held>,
    next: Option<Reading>,
    opening: Option<Vec<u8>>,
    pub(crate) failed: Option<String>,
    pub(crate) read: usize,
    pub(crate) waited: Duration,
}

impl<'c> Boards<'c> {
    /// The boards of `cut` at `scale` on `look` (its ground, its ink for fresh ink) — refused where the board isn't
    /// 2160p's or half of it.
    pub(crate) fn new(cut: &'c Cut, scale: Scale, look: &video::Look) -> Result<Boards<'c>> {
        let s = match scale.k() {
            1 => 0.5,
            2 => 1.0,
            k => return Err(format!("pictures are drawn at 1080p or 2160p, not at {k}× 1080p's")),
        };
        Ok(Boards {
            cut,
            s,
            ground: look.ground,
            full: look.ink,
            held: None,
            next: None,
            opening: None,
            failed: None,
            read: 0,
            waited: Duration::ZERO,
        })
    }

    fn empty(&self) -> Vec<u8> {
        self.ground.repeat((BOARD.0 as f64 * self.s * (BOARD.1 as f64 * self.s)) as usize)
    }

    /// The board a frame shows (`Shows`), at the video's size.
    pub(crate) fn board(&mut self, shows: &Shows) -> Vec<u8> {
        let Some(n) = shows.n else {
            if self.opening.is_none() {
                let got = (self.cut.opening.picture.as_ref())
                    .ok_or_else(|| "the opening has no picture".to_string())
                    .and_then(|(p, _)| {
                        whole_png(p, self.ground, Some(BOARD)).map_err(|e| format!("{}: {e}", p.display()))
                    });
                match got {
                    Ok(px) => self.opening = Some(sized(px, 3, self.s)),
                    Err(e) => _ = self.failed.get_or_insert(e),
                }
            }
            return self.opening.clone().unwrap_or_else(|| self.empty());
        };
        let key = (n, shows.k);
        self.hold(key);
        let p = &self.cut.pictures[n];
        match &self.held {
            Some(h) if h.key == key => h.board(&p.steps[shows.k], shows.t, p.fresh, self.full),
            _ => self.empty(),
        }
    }

    /// Step `key` held: the worker's where it read it, else read now; then the next one's reading begun.
    fn hold(&mut self, key: (usize, usize)) {
        if self.held.as_ref().is_some_and(|h| h.key == key) {
            return;
        }
        let waited = Instant::now();
        let got = match self.next.take() {
            Some((k, job)) if k == key => {
                job.join().unwrap_or_else(|_| Err("a picture's reading failed".into()))
            }
            _ => {
                let before = self.before(key);
                Held::load(&self.cut.pictures[key.0].steps[key.1], key, before, (self.s, self.ground))
            }
        };
        self.waited += waited.elapsed();
        self.read += 1;
        let held = match got {
            Ok(h) => h,
            Err(e) => {
                self.failed.get_or_insert(e);
                return;
            }
        };
        let (n, k) = key;
        let next = match k + 1 < self.cut.pictures[n].steps.len() {
            true => Some(((n, k + 1), held.whole.clone())),
            false => (n + 1 < self.cut.pictures.len()).then(|| ((n + 1, 0), self.empty())),
        };
        if let Some((nk, before)) = next {
            let step = self.cut.pictures[nk.0].steps[nk.1].clone();
            let at = (self.s, self.ground);
            self.next = Some((nk, std::thread::spawn(move || Held::load(&step, nk, before, at))));
        }
        self.held = Some(held);
    }

    /// The board before step `key`: the ground for a picture's first, else the step before's whole picture.
    fn before(&self, (n, k): (usize, usize)) -> Vec<u8> {
        if k == 0 {
            return self.empty();
        }
        match &self.held {
            Some(h) if h.key == (n, k - 1) => h.whole.clone(),
            _ => {
                let p = &self.cut.pictures[n].steps[k - 1].file;
                whole_png(p, self.ground, Some(BOARD))
                    .map_or_else(|_| self.empty(), |px| sized(px, 3, self.s))
            }
        }
    }
}
