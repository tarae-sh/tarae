//! `cargo xtask cut` — a take (`record`'s) made a video by the edit list beside it, a `cut.toml` (the full video's V1,
//! main's `.sb/full-video-plan.md` "How it's cut"): an hour and a half into about ten minutes, honest about time.
//!
//! - `cut FILE` — the take in FILE's folder cut by FILE, into `<folder>/<stem>/`: `cut.mp4` (1920×1080, its sound
//!   tarae's keys where the picture runs in real time and the beds the file names — `sound.rs`; `cut.wav` beside it), its
//!   sheet `sheet-NN.png` (every picture says its video time, its take time and its speed), YouTube's chapter list
//!   `chapters.txt` (video times) and what was made, `cut.txt`.
//! - `cut init TAKE [--out FILE]` — a first edit list for the take by rule, from its landmarks and its screens
//!   (`init.rs`): `TAKE/init.toml` unless FILE says.
//!
//! The file:
//!
//! ```toml
//! bg = "dark"                             # or "light" (dark if left out)
//! default = 8                             # what no span names: a speed, or "out"
//! mascot = false                          # no mascot at the keys in the band (`mascot.rs`; there if left out)
//!
//! [opening]                               # the video's first seconds (`Program`): the intro — the brush's swing
//!                                         # and `t a r a e`, `INTRO_MS` —, the card of what's built (`BUILT_MS`),
//!                                         # where it ends; then the take. Every word on the card is from here.
//! name = "halil"                          # the card's name, large; chapters.txt's 0:00, over the three parts
//! line = "A task manager for the terminal."    # what it is, to someone who has never heard of it
//! code = ["halil add \"pay rent friday 9am +home !high\"", "halil board"]   # up to two lines, drawn as code
//! at = "end - 6s"                         # where it ends: this much of the take at 1×, its keys heard
//! for = "4s"
//! zoom = "card:Run these checks"           # the camera there, on a target as a [[zoom]]'s: eased in from where
//!                                         # it ends' start, held to its end (a [[zoom]] is the take's alone)
//!
//! [[span]]                                # in the take's order, apart; the rest takes `default`
//! from = "turn 1 start - 4s"
//! to = "card 1 answered + 1s"
//! speed = 1                               # one of: speed = N (≥ 1) · fit = "6s" · out = true
//!
//! [[chapter]]                             # a card of `CHAPTER_MS` where it starts; chapters.txt
//! at = "start"
//! title = "An empty folder"
//! card = false                            # no card: its number and title stand in the band `CHAPTER_BAND_MS`
//!
//! [[caption]]                             # one line under the picture, ≤ `CAPTION_MAX` characters
//! at = "turn 1 start"
//! for = "3s"                              # of the video's time
//! text = "One ask: what the tool is."
//!
//! [[zoom]]                                # the camera on what must be read, eased in and back out — in the
//!                                         # take, never where it ends, whatever its time
//! at = "card 1 shown + 0.5s"
//! for = "2s"                              # held, of the video's time
//! on = "card:wants to run"                # a target as the Shorts' camera reads one (`camera::target`)
//!
//! [[music]]                               # a bed from a chapter on, until the next bed's (`sound.rs`)
//! chapter = 1                             # its chapter's number; chapter 1's from the video's first frame
//! file = "beds/one.mp3"                   # any file ffmpeg reads, from this file's folder (or a whole path)
//! loop = ["0:08.2", "1:52"]               # where it repeats, the track's own times marked by ear (else it
//!                                         # fades where it runs out and the next bed begins)
//! gain = 4                                # a made bed (the rain): dB on the file as made, its level its own —
//!                                         # not levelled, no swing; left out: levelled under the keys ([sound])
//!
//! [sound]                                 # the mix by distance (LU; each left out: its constant, `sound.rs`)
//! under = 0                               # the bed under the keys' own loudness where they may sound
//! over = 6                                # the bed where it leads, over that: the swing
//! ```
//!
//! A time in the take: `start`, `end`, a clock (`1:02:03.5`, `37:12`), a length from the start (`72.5s`), or a landmark
//! (`landmarks.jsonl`) — `turn N start|end`, `[KIND] card N shown|answered` (`run card 2 shown`: the second card of kind
//! `run`), `run N start|end`, `review N`, `front N` or `front NAME` (when that workspace came to the front), `bring N`
//! (a bring ended), `mark "WORDS"` (the driver's `record mark WORDS`, its words whole) or `mark N`, `chapter "WORDS"`
//! (the driver's `record chapter WORDS`) or `chapter N` —, `N` from 1 or `last`; any of them `+ 2s` or `- 500ms`. A landmark the take doesn't have, or a time past its end, is refused
//! with its line. A length: `3s`, `2.5s`, `500ms`, `1m 30s`.
//!
//! Every list is in the take's order and spans don't overlap: a file that reorders anything is refused with the line.
//! What the picture always says, in the band under tarae's cells (never over them): the take's real time (`0:37:12`)
//! at its left; on a sped span its factor (`▸▸ ×16`) at its right; where time is left out, the clock jumps and a mark
//! says how much (`+6 min`) for `JUMP_MS`; over where it ends, `where it ends`. The card of what's built has one small
//! line the cut makes, never the file (`told`): that it's a real session, how long it took and how long it's shown.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::Result;
use crate::camera;
use crate::scenes::docshots::Bg;
use crate::video::{self, FPS};

mod draw;
pub(crate) mod init;
mod mascot;
mod opening;
mod sound;
#[cfg(test)]
mod tests;

/// A chapter's card, crossing in and out.
pub(crate) const CHAPTER_MS: u64 = 2_000;
/// A chapter with no card: its number and title stand in the band this long (of the video), fading in and out over
/// `FADE_MS`, as the take goes on.
pub(crate) const CHAPTER_BAND_MS: u64 = 3_000;
/// A caption's longest (one line in the band, between the clock and the speed's mark).
pub(crate) const CAPTION_MAX: usize = 60;
/// The jump's mark stands this long (then fades), and comes in and goes out over `FADE_MS` — a caption's too.
pub(crate) const JUMP_MS: u64 = 1_000;
pub(crate) const FADE_MS: u64 = crate::captions::FADE_MS;
/// The intro: the brush's swing and `t a r a e` (`video::WideIntro`), the swing's frames then the name risen, held.
pub(crate) const INTRO_MS: u64 = 2_500;
/// The card of what's built.
pub(crate) const BUILT_MS: u64 = 3_500;
/// The card's words at their longest (characters): its name, its line, each line of code; and how many lines of code.
pub(crate) const NAME_MAX: usize = 24;
pub(crate) const LINE_MAX: usize = 60;
pub(crate) const CODE_MAX: usize = 56;
pub(crate) const CODE_LINES: usize = 2;

const USAGE: &str = "usage: cargo xtask cut FILE | cut init TAKE [--out FILE]";

pub fn run(args: &[String]) -> Result<()> {
    match args {
        [init, rest @ ..] if init == "init" => init::run(rest),
        [file] => draw::run(Path::new(file)),
        _ => Err(USAGE.into()),
    }
}

// ── Times ──────────────────────────────────────────────────────────────────────────────────────────────────────────────

/// A length (ms): `3s`, `2.5s`, `500ms`, `2m`, `1m 30s`, `1h 5m`.
pub(crate) fn length(s: &str) -> std::result::Result<u64, String> {
    let bad = || format!("\"{s}\": a length, as 3s, 2.5s, 500ms or 1m 30s");
    let mut rest = s.trim();
    if rest.is_empty() {
        return Err(bad());
    }
    let mut total = 0.0;
    while !rest.is_empty() {
        let n = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).ok_or_else(bad)?;
        let value: f64 = rest[..n].parse().map_err(|_| bad())?;
        rest = &rest[n..];
        let u = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
        total += value
            * match &rest[..u] {
                "ms" => 1.0,
                "s" => 1_000.0,
                "m" | "min" => 60_000.0,
                "h" => 3_600_000.0,
                _ => return Err(bad()),
            };
        rest = rest[u..].trim_start();
    }
    Ok(total.round() as u64)
}

/// A clock's time (ms): `1:02:03.5`, `37:12`.
fn clock_at(s: &str) -> std::result::Result<u64, String> {
    let bad = || format!("\"{s}\": a clock, as 37:12 or 1:02:03.5");
    let parts: Vec<&str> = s.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return Err(bad());
    }
    let secs: f64 = parts[parts.len() - 1].parse().map_err(|_| bad())?;
    let whole = |p: &str| p.parse::<u64>().map_err(|_| bad());
    let (h, m) = match parts.len() {
        3 => (whole(parts[0])?, whole(parts[1])?),
        _ => (0, whole(parts[0])?),
    };
    if !(0.0..60.0).contains(&secs)
        || (parts.len() == 3 && m >= 60)
        || parts[parts.len() - 1].starts_with('-')
    {
        return Err(bad());
    }
    Ok((h * 3_600_000 + m * 60_000) + (secs * 1000.0).round() as u64)
}

/// The take's time as the picture says it: `0:37:12`.
pub(crate) fn clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// The take's time to a tenth: `0:37:12.4`.
pub(crate) fn clock_tenths(ms: u64) -> String {
    format!("{}.{}", clock(ms), ms % 1000 / 100)
}

/// A video time as the sheet says it: `1:23.4`.
pub(crate) fn video_time(us: u64) -> String {
    let tenths = us / 100_000;
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
}

/// A video time as YouTube's chapter list says it: `1:23`, `1:02:03`.
fn chapter_time(us: u64) -> String {
    let s = us / 1_000_000;
    match s >= 3600 {
        true => format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60),
        false => format!("{}:{:02}", s / 60, s % 60),
    }
}

/// The jump's mark: how much of the take is left out there — `+40 s`, `+6 min`, `+1 h 12 min`.
pub(crate) fn jumped(ms: u64) -> String {
    if ms < 59_500 {
        return format!("+{} s", (ms as f64 / 1000.0).round().max(1.0));
    }
    let min = (ms as f64 / 60_000.0).round() as u64;
    match (min / 60, min % 60) {
        (0, m) => format!("+{m} min"),
        (h, 0) => format!("+{h} h"),
        (h, m) => format!("+{h} h {m} min"),
    }
}

/// How long something took, as tarae writes every time someone reads (`term.rs`'s `run_duration`, the same words):
/// `412ms`, `8.3s`, `7m 12s`, `1h 32m`, `2d 04h`.
pub(crate) fn run_duration(d: Duration) -> String {
    let s = d.as_secs_f32();
    let whole = d.as_secs().max(60);
    match s {
        _ if s < 0.9995 => format!("{:.0}ms", s * 1000.0),
        _ if s < 59.95 => format!("{s:.1}s"),
        _ if whole < 3600 => format!("{}m {:02}s", whole / 60, whole % 60),
        _ if whole < 86_400 => format!("{}h {:02}m", whole / 3600, whole / 60 % 60),
        _ => format!("{}d {:02}h", whole / 86_400, whole / 3600 % 24),
    }
}

/// The card of what's built's small line, made by the cut from the take's length (ms) and the video's (µs), so it can't
/// be wrong.
pub(crate) fn told(take_ms: u64, video_us: u64) -> String {
    format!(
        "A real session with Claude in tarae: {}, shown in {}",
        run_duration(Duration::from_millis(take_ms)),
        run_duration(Duration::from_micros(video_us))
    )
}

/// A sped span's mark: `▸▸ ×16`, `▸▸ ×2.5`; none at 1×.
pub(crate) fn sped(k: f64) -> Option<String> {
    (k > 1.0 + 1e-6).then(|| format!("▸▸ ×{}", factor(k)))
}

/// A speed's factor in words: whole from 10 up or where it is near whole, else to a tenth.
pub(crate) fn factor(k: f64) -> String {
    match k >= 10.0 || (k - k.round()).abs() < 0.05 {
        true => format!("{}", k.round()),
        false => format!("{k:.1}"),
    }
}

// ── Landmarks ───────────────────────────────────────────────────────────────────────────────────────────────────

/// A take's landmarks (`landmarks.jsonl`, in the order of their times) and its end: what a time in the file resolves by.
pub(crate) struct Marks {
    pub(crate) list: Vec<Value>,
    pub(crate) end: u64,
}

pub(crate) fn ms_of(v: &Value) -> u64 {
    v["ms"].as_u64().unwrap_or(0)
}

impl Marks {
    pub(crate) fn new(mut list: Vec<Value>, end: u64) -> Marks {
        list.sort_by_key(ms_of);
        Marks { list, end }
    }

    /// `dir`'s landmarks (none where it has no `landmarks.jsonl`), the take ending at `end`.
    pub(crate) fn read(dir: &Path, end: u64) -> Result<Marks> {
        let p = dir.join("landmarks.jsonl");
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("{}: {e}", p.display())),
        };
        let list = (text.lines().enumerate())
            .filter(|(_, l)| !l.trim().is_empty())
            .map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("{}:{}: {e}", p.display(), i + 1)))
            .collect::<Result<Vec<Value>>>()?;
        Ok(Marks::new(list, end))
    }

    pub(crate) fn of<'a>(&'a self, what: &'a str) -> impl Iterator<Item = &'a Value> + 'a {
        self.list.iter().filter(move |v| v["what"] == what)
    }

    /// A time in the take (ms) as the file says it (the module's doc): refused, in words, where the take has no such
    /// landmark or the time falls outside it.
    pub(crate) fn time(&self, said: &str) -> std::result::Result<u64, String> {
        let s = said.trim();
        // (an offset only past a mark's words: they may hold " - ")
        let after = s.rfind('"').map_or(0, |i| i + 1);
        let cut = [" + ", " - "]
            .iter()
            .filter_map(|p| s[after..].rfind(p).map(|i| (after + i, *p)))
            .max_by_key(|c| c.0);
        let (base, by) = match cut {
            Some((i, p)) => {
                let d = length(&s[i + 3..]).map_err(|e| format!("\"{said}\": {e}"))? as i64;
                (s[..i].trim(), if p == " + " { d } else { -d })
            }
            None => (s, 0),
        };
        let t = self.point(base).map_err(|e| format!("\"{said}\": {e}"))? as i64 + by;
        if t < 0 {
            return Err(format!("\"{said}\": before the take's start"));
        }
        if t as u64 > self.end {
            return Err(format!("\"{said}\": past the take's end, {}", clock(self.end)));
        }
        Ok(t as u64)
    }

    fn point(&self, base: &str) -> std::result::Result<u64, String> {
        for what in ["mark", "chapter"] {
            if let Some(w) = base.strip_prefix(what).and_then(|w| w.strip_prefix(' ')).map(str::trim)
                && let Some(w) = w.strip_prefix('"').and_then(|w| w.strip_suffix('"'))
            {
                let at: Vec<u64> = self.of(what).filter(|v| v["said"] == w).map(ms_of).collect();
                return match at[..] {
                    [ms] => Ok(ms),
                    [] => Err(format!("the take has no {what} “{w}”")),
                    _ => {
                        Err(format!("the take has {} {what}s “{w}” — one's words, or another time", at.len()))
                    }
                };
            }
        }
        let words: Vec<&str> = base.split_whitespace().collect();
        let plural = |n: usize, noun: &str| format!("{n} {noun}{}", if n == 1 { "" } else { "s" });
        let nth = |list: Vec<&'_ Value>, n: &str, noun: &str| -> std::result::Result<Value, String> {
            let k = match n {
                "last" => list.len(),
                n => n
                    .parse::<usize>()
                    .ok()
                    .filter(|&k| k >= 1)
                    .ok_or(format!("{n}: a count from 1, or last"))?,
            };
            list.get(k.wrapping_sub(1))
                .map(|v| (*v).clone())
                .ok_or(format!("the take has {}", plural(list.len(), noun)))
        };
        let same = |what: &str, v: &Value, key: &str| -> Option<u64> {
            self.of(what).find(|e| e[key] == v[key] && ms_of(e) >= ms_of(v)).map(ms_of)
        };
        match words.as_slice() {
            ["start"] => Ok(0),
            ["end"] => Ok(self.end),
            [one] if one.starts_with(|c: char| c.is_ascii_digit()) => match one.contains(':') {
                true => clock_at(one),
                false => length(one),
            },
            [_, ..] if base.starts_with(|c: char| c.is_ascii_digit()) => length(base),
            ["turn", n, edge @ ("start" | "end")] => {
                let v = nth(self.of("turn-started").collect(), n, "turn")?;
                match *edge {
                    "start" => Ok(ms_of(&v)),
                    _ => same("turn-ended", &v, "session").ok_or("that turn never ended in the take".into()),
                }
            }
            ["run", n, edge @ ("start" | "end")] => {
                let v = nth(self.of("run-started").collect(), n, "run")?;
                match *edge {
                    "start" => Ok(ms_of(&v)),
                    _ => same("run-ended", &v, "id").ok_or("that run never ended in the take".into()),
                }
            }
            [kind @ .., "card", n, edge @ ("shown" | "answered")] if kind.len() <= 1 => {
                let kind = kind.first().copied();
                let list = self.of("card-shown").filter(|v| kind.is_none_or(|k| v["kind"] == k)).collect();
                let v = nth(list, n, &kind.map_or("card".to_string(), |k| format!("{k} card")))?;
                match *edge {
                    "shown" => Ok(ms_of(&v)),
                    _ => same("card-answered", &v, "id").ok_or("that card was never answered in the take".into()),
                }
            }
            ["review", n] => Ok(ms_of(&nth(self.of("review-opened").collect(), n, "review")?)),
            ["bring", n] => Ok(ms_of(&nth(self.of("bring").collect(), n, "bring")?)),
            ["mark", n] => Ok(ms_of(&nth(self.of("mark").collect(), n, "mark")?)),
            ["chapter", n] => Ok(ms_of(&nth(self.of("chapter").collect(), n, "chapter")?)),
            ["front", n] if *n == "last" || n.parse::<usize>().is_ok() => {
                Ok(ms_of(&nth(self.of("front").collect(), n, "front")?))
            }
            ["front", name] => (self.of("front").find(|v| v["name"] == *name || v["to"] == *name))
                .map(ms_of)
                .ok_or(format!("no workspace {name} came to the front in the take")),
            _ => Err("not a time: start, end, a clock (37:12), a length (72.5s) or a landmark (turn 2 end, card 1 \
                      shown, run card 2 answered, run 1 start, review 1, front fix, bring 1, mark \"the board\", chapter 2), \
                      maybe + 2s"
                .into()),
        }
    }
}

// ── The time map ──────────────────────────────────────────────────────────────────────────────────────────────────────

/// How a stretch of the take is shown: at a speed (1 = as it was), or left out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Pace {
    Speed(f64),
    Out,
}

/// A stretch of the take, `from`..`to` ms, and its pace; the file's line that named it (None: the default's).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Seg {
    pub(crate) from: u64,
    pub(crate) to: u64,
    pub(crate) pace: Pace,
    pub(crate) line: Option<usize>,
}

/// The take's time against the video's: the stretches in order, end to end from 0 to the take's end, each one's start
/// in the video (µs, the take part's own clock — the cards between parts are `Program`'s).
#[derive(Debug)]
pub(crate) struct Map {
    segs: Vec<Seg>,
    at: Vec<u64>,
    len: u64,
}

impl Map {
    pub(crate) fn new(segs: Vec<Seg>) -> Map {
        let mut at = Vec::with_capacity(segs.len());
        let mut v = 0;
        for s in &segs {
            at.push(v);
            v += Self::into(s, s.to);
        }
        Map { segs, at, len: v }
    }

    /// The video's µs from `s`'s start to its take's `t` — one expression for a stretch's end and the next one's start,
    /// so they meet exactly.
    fn into(s: &Seg, t: u64) -> u64 {
        match s.pace {
            Pace::Out => 0,
            Pace::Speed(k) => ((t - s.from) as f64 * 1000.0 / k).round() as u64,
        }
    }

    pub(crate) fn segs(&self) -> &[Seg] {
        &self.segs
    }

    /// Where stretch `i` starts in the video (µs).
    pub(crate) fn seg_at(&self, i: usize) -> u64 {
        self.at[i]
    }

    /// How long stretch `i` lasts in the video (µs).
    pub(crate) fn seg_len(&self, i: usize) -> u64 {
        self.at.get(i + 1).copied().unwrap_or(self.len) - self.at[i]
    }

    /// The take part's length in the video (µs).
    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    /// The stretch the take's `t` ms is in (its end: the last).
    pub(crate) fn seg_of(&self, t: u64) -> usize {
        self.segs.partition_point(|s| s.from <= t).saturating_sub(1)
    }

    /// The take's `t` ms in the video (µs): never backward as `t` goes on; a stretch left out all at one moment.
    pub(crate) fn video(&self, t: u64) -> u64 {
        let i = self.seg_of(t);
        self.at[i] + Self::into(&self.segs[i], t.min(self.segs[i].to))
    }

    /// The video's `v` µs in the take (ms) and the stretch shown there — a stretch left out shows nowhere: the moment it
    /// sits at shows what comes after it, and where nothing does (the take's tail left out) the video ends where it
    /// begins. Exact at each shown stretch's start; past the end, where the shown take ends.
    pub(crate) fn take(&self, v: u64) -> (u64, usize) {
        let i = self.at.partition_point(|&a| a <= v).saturating_sub(1);
        let s = &self.segs[i];
        let t = match s.pace {
            Pace::Out => s.from,
            Pace::Speed(k) => s.from + ((v - self.at[i]) as f64 * k / 1000.0).floor() as u64,
        };
        (t.min(s.to), i)
    }

    /// Whether the take's `t` falls inside a stretch left out (its edges are the shown stretches').
    pub(crate) fn left_out(&self, t: u64) -> bool {
        self.segs.iter().any(|s| s.pace == Pace::Out && s.from < t && t < s.to)
    }
}

// ── The file ────────────────────────────────────────────────────────────────────────────────────────────────────────────

/// The opening's words and where it ends: the card's name, line and lines of code; then this much (`len`) of the take
/// from `from` (ms), at 1×, the camera on `zoom`'s target there (its line) where it's given.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Opening {
    pub(crate) name: String,
    pub(crate) line: String,
    pub(crate) code: Vec<String>,
    pub(crate) from: u64,
    pub(crate) len: u64,
    pub(crate) zoom: Option<(String, usize)>,
}

impl Opening {
    /// Whether the take's `t` (ms) is shown where it ends.
    pub(crate) fn ends(&self, t: u64) -> bool {
        (self.from..self.from + self.len).contains(&t)
    }
}

/// A chapter, a caption, a zoom: where in the take (ms), and the file's line. A chapter shows a card, or (`card`
/// false) stands in the band as the take goes on.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Chapter {
    pub(crate) at: u64,
    pub(crate) title: String,
    pub(crate) line: usize,
    pub(crate) card: bool,
}

/// A caption: standing `len` ms of the video.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Caption {
    pub(crate) at: u64,
    pub(crate) len: u64,
    pub(crate) text: String,
    pub(crate) line: usize,
}

/// A zoom: held `len` ms of the video on `on`, moving in and out `camera::DRIFT_MS` each way — in the take part only
/// (where it ends has the opening's own, `Opening::zoom`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Zoom {
    pub(crate) at: u64,
    pub(crate) len: u64,
    pub(crate) on: String,
    pub(crate) line: usize,
}

/// A music bed: from chapter `chapter` (its index) on, its `file` as the edit list says it, repeating between `looped`'s
/// two times in the track (ms) where it's given; at `gain` (dB on the file as made) from its fade in to its fade out
/// where that's given — a made bed, its level its own —, else levelled; the file's line.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Bed {
    pub(crate) chapter: usize,
    pub(crate) file: String,
    pub(crate) looped: Option<(u64, u64)>,
    pub(crate) gain: Option<f32>,
    pub(crate) line: usize,
}

/// A made bed's gain, dB on its file as made: from this to `GAIN_MOST_DB`.
pub(crate) const GAIN_LEAST_DB: f64 = -40.0;
pub(crate) const GAIN_MOST_DB: f64 = 24.0;

/// A loop's shortest (ms): a seam every few seconds is no bed.
pub(crate) const LOOP_MIN: u64 = 2_000;

/// An edit list read and resolved against its take.
#[derive(Debug)]
pub(crate) struct Cut {
    pub(crate) file: String,
    pub(crate) bg: Bg,
    pub(crate) mascot: bool,
    pub(crate) map: Map,
    pub(crate) opening: Opening,
    pub(crate) chapters: Vec<Chapter>,
    pub(crate) captions: Vec<Caption>,
    pub(crate) zooms: Vec<Zoom>,
    pub(crate) beds: Vec<Bed>,
    pub(crate) mix: sound::Mix,
    pub(crate) end: u64,
}

/// The file's text by lines: where a table or a key is, for what's refused.
struct Src<'a> {
    lines: Vec<&'a str>,
    file: &'a str,
}

impl Src<'_> {
    /// The line (from 1) of the `k`th `[[name]]`, or of `[name]` (k None); 1 where it isn't written so.
    fn header(&self, name: &str, k: Option<usize>) -> usize {
        let want = match k {
            Some(_) => format!("[[{name}]]"),
            None => format!("[{name}]"),
        };
        (self.lines.iter().enumerate())
            .filter(|(_, l)| l.trim_start().starts_with(&want))
            .nth(k.unwrap_or(0))
            .map_or(1, |(i, _)| i + 1)
    }

    /// The line of `key = …` in the table whose header is on line `from` (0: the top, before any table); else `from`.
    fn key(&self, from: usize, key: &str) -> usize {
        for (i, l) in self.lines.iter().enumerate().skip(from) {
            let l = l.trim_start();
            if l.starts_with('[') {
                break;
            }
            if l.strip_prefix(key).is_some_and(|r| r.trim_start().starts_with('=')) {
                return i + 1;
            }
        }
        from.max(1)
    }

    fn at(&self, line: usize, said: impl std::fmt::Display) -> String {
        format!("{}:{line}: {said}", self.file)
    }
}

/// One table of the file with where it is.
struct Entry<'t, 's> {
    t: &'t toml::Table,
    line: usize,
    src: &'s Src<'s>,
    name: &'static str,
}

impl Entry<'_, '_> {
    fn keys(&self, allowed: &[&str]) -> Result<()> {
        match self.t.keys().find(|k| !allowed.contains(&k.as_str())) {
            Some(k) => Err(self.src.at(
                self.src.key(self.line, k),
                format!("{}: no key {k} (it takes {})", self.name, allowed.join(", ")),
            )),
            None => Ok(()),
        }
    }

    fn line(&self, key: &str) -> usize {
        self.src.key(self.line, key)
    }

    fn err(&self, key: &str, said: impl std::fmt::Display) -> String {
        self.src.at(self.line(key), said)
    }

    fn text(&self, key: &str) -> Result<Option<&str>> {
        match self.t.get(key) {
            None => Ok(None),
            Some(toml::Value::String(s)) => Ok(Some(s)),
            Some(_) => Err(self.err(key, format!("{}.{key}: a string", self.name))),
        }
    }

    fn needed(&self, key: &str) -> Result<&str> {
        self.text(key)?.ok_or_else(|| self.src.at(self.line, format!("{}: no {key}", self.name)))
    }

    fn time(&self, key: &str, marks: &Marks) -> Result<u64> {
        marks.time(self.needed(key)?).map_err(|e| self.err(key, e))
    }

    fn length(&self, key: &str) -> Result<u64> {
        length(self.needed(key)?).map_err(|e| self.err(key, e))
    }
}

fn number(v: &toml::Value) -> Option<f64> {
    match v {
        toml::Value::Integer(i) => Some(*i as f64),
        toml::Value::Float(f) => Some(*f),
        _ => None,
    }
}

impl Cut {
    /// The edit list `text` (its name `file`, said in what's refused) against a take's landmarks and end (`marks`).
    pub(crate) fn parse(text: &str, file: &str, marks: &Marks) -> Result<Cut> {
        let src = Src { lines: text.lines().collect(), file };
        let top: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("{file}: {e}"))?;
        let root = Entry { t: &top, line: 0, src: &src, name: "the file" };
        root.keys(&[
            "bg", "default", "mascot", "opening", "span", "chapter", "caption", "zoom", "music", "sound",
        ])?;
        let mascot = match top.get("mascot") {
            None => true,
            Some(toml::Value::Boolean(b)) => *b,
            Some(_) => return Err(root.err("mascot", "mascot: true or false")),
        };
        let bg = match root.text("bg")? {
            None | Some("dark") => Bg::Dark,
            Some("light") => Bg::Light,
            Some(w) => return Err(root.err("bg", format!("bg = \"{w}\": dark or light"))),
        };
        let default = match top.get("default") {
            Some(toml::Value::String(s)) if s == "out" => Pace::Out,
            Some(v) if number(v).is_some_and(|k| k >= 1.0) => Pace::Speed(number(v).unwrap_or(1.0)),
            Some(_) => return Err(root.err("default", "default: a speed (1 or more) or \"out\"")),
            None => {
                return Err(
                    src.at(1, "no default: what no span names — a speed (default = 8) or default = \"out\"")
                );
            }
        };
        let list = |name: &'static str| -> Result<Vec<Entry>> {
            match top.get(name) {
                None => Ok(Vec::new()),
                Some(toml::Value::Array(a)) => (a.iter().enumerate())
                    .map(|(k, v)| match v.as_table() {
                        Some(t) => Ok(Entry { t, line: src.header(name, Some(k)), src: &src, name }),
                        None => Err(src.at(src.key(0, name), format!("{name}: tables, as [[{name}]]"))),
                    })
                    .collect(),
                Some(_) => Err(src.at(src.key(0, name), format!("{name}: tables, as [[{name}]]"))),
            }
        };

        // (the spans: in order, apart, each one pace; the gaps the default's)
        let mut named: Vec<Seg> = Vec::new();
        for e in list("span")? {
            e.keys(&["from", "to", "speed", "fit", "out"])?;
            let (from, to) = (e.time("from", marks)?, e.time("to", marks)?);
            if to <= from {
                return Err(e.err(
                    "to",
                    format!("span: it ends ({}) where or before it starts ({})", clock(to), clock(from)),
                ));
            }
            let paces = ["speed", "fit", "out"].iter().filter(|k| e.t.contains_key(**k)).count();
            if paces != 1 {
                return Err(src.at(e.line, "span: one of speed = N, fit = \"6s\" or out = true"));
            }
            let pace = if let Some(v) = e.t.get("speed") {
                match number(v) {
                    Some(k) if k >= 1.0 => Pace::Speed(k),
                    _ => return Err(e.err("speed", "span: speed is 1 or more (slow motion isn't a cut)")),
                }
            } else if e.t.contains_key("fit") {
                let fit = e.length("fit")?;
                if fit == 0 || fit > to - from {
                    return Err(e.err(
                        "fit",
                        format!(
                            "span: fit in {:.1} s what lasted {:.1} s — never slower than 1×",
                            fit as f64 / 1e3,
                            (to - from) as f64 / 1e3
                        ),
                    ));
                }
                Pace::Speed((to - from) as f64 / fit as f64)
            } else {
                match e.t.get("out") {
                    Some(toml::Value::Boolean(true)) => Pace::Out,
                    _ => return Err(e.err("out", "span: out = true")),
                }
            };
            if let Some(before) = named.last() {
                let was = before.line.unwrap_or(0);
                if from < before.from {
                    return Err(e.err("from", format!("span: out of the take's order — it starts ({}) before the span at line {was} ({})", clock(from), clock(before.from))));
                }
                if from < before.to {
                    return Err(e.err(
                        "from",
                        format!(
                            "span: it starts ({}) inside the span at line {was} (to {})",
                            clock(from),
                            clock(before.to)
                        ),
                    ));
                }
            }
            named.push(Seg { from, to, pace, line: Some(e.line) });
        }
        let mut segs = Vec::new();
        let mut at = 0;
        for s in named {
            if s.from > at {
                segs.push(Seg { from: at, to: s.from, pace: default, line: None });
            }
            at = s.to;
            segs.push(s);
        }
        if at < marks.end || segs.is_empty() {
            segs.push(Seg { from: at, to: marks.end.max(at + 1), pace: default, line: None });
        }
        let map = Map::new(segs);

        let opening = Self::opening(&top, &src, marks)?;

        let mut chapters: Vec<Chapter> = Vec::new();
        for e in list("chapter")? {
            e.keys(&["at", "title", "card"])?;
            let (at, title) = (e.time("at", marks)?, e.needed("title")?.trim().to_string());
            let card = match e.t.get("card") {
                None => true,
                Some(toml::Value::Boolean(b)) => *b,
                Some(_) => {
                    return Err(e.err("card", "chapter: card = false (no card: its title in the band)"));
                }
            };
            if title.is_empty() {
                return Err(e.err("title", "chapter: its title is empty"));
            }
            if let Some(b) = chapters.last()
                && at <= b.at
            {
                return Err(e.err(
                    "at",
                    format!(
                        "chapter: out of the take's order — at {}, not after the chapter at line {} ({})",
                        clock(at),
                        b.line,
                        clock(b.at)
                    ),
                ));
            }
            if map.left_out(at) {
                return Err(e.err("at", format!("chapter: at {}, inside a span left out", clock(at))));
            }
            chapters.push(Chapter { at, title, line: e.line, card });
        }
        // (where a chapter with no card stands in the band, of the map's µs: a caption waits for it)
        let banded: Vec<(u64, usize)> =
            (chapters.iter().filter(|c| !c.card)).map(|c| (map.video(c.at), c.line)).collect();

        let mut captions: Vec<Caption> = Vec::new();
        for e in list("caption")? {
            e.keys(&["at", "for", "text"])?;
            let (at, len, text) =
                (e.time("at", marks)?, e.length("for")?, e.needed("text")?.trim().to_string());
            let n = text.chars().count();
            if n == 0 || n > CAPTION_MAX || text.contains('\n') {
                return Err(
                    e.err("text", format!("caption: one line of 1 to {CAPTION_MAX} characters ({n} here)"))
                );
            }
            if map.left_out(at) {
                return Err(e.err("at", format!("caption: at {}, inside a span left out", clock(at))));
            }
            let (v0, v1) = (map.video(at), map.video(at) + len * 1000);
            if let Some((_, l)) = banded.iter().find(|(c, _)| v0 < c + CHAPTER_BAND_MS * 1000 && *c < v1) {
                return Err(e.err(
                    "at",
                    format!(
                        "caption: it shows while the chapter at line {l} stands in the band — one at a time"
                    ),
                ));
            }
            if let Some(b) = captions.last() {
                if at < b.at {
                    return Err(e.err(
                        "at",
                        format!(
                            "caption: out of the take's order — at {}, before the caption at line {} ({})",
                            clock(at),
                            b.line,
                            clock(b.at)
                        ),
                    ));
                }
                // (the 11-10 rule: a caption is gone before the next shows)
                if map.video(at) < map.video(b.at) + b.len * 1000 {
                    return Err(e.err(
                        "at",
                        format!(
                            "caption: it shows while the one at line {} still stands — one at a time",
                            b.line
                        ),
                    ));
                }
            }
            captions.push(Caption { at, len, text, line: e.line });
        }

        let mut zooms: Vec<Zoom> = Vec::new();
        for e in list("zoom")? {
            e.keys(&["at", "for", "on"])?;
            let (at, len, on) = (e.time("at", marks)?, e.length("for")?, e.needed("on")?.to_string());
            if map.left_out(at) {
                return Err(e.err("at", format!("zoom: at {}, inside a span left out", clock(at))));
            }
            if let Some(b) = zooms.last() {
                if at < b.at {
                    return Err(e.err(
                        "at",
                        format!(
                            "zoom: out of the take's order — at {}, before the zoom at line {} ({})",
                            clock(at),
                            b.line,
                            clock(b.at)
                        ),
                    ));
                }
                let back = map.video(b.at) + (2 * camera::DRIFT_MS + b.len) * 1000;
                if map.video(at) < back {
                    return Err(e.err(
                        "at",
                        format!("zoom: it starts before the one at line {} is back on the whole", b.line),
                    ));
                }
            }
            zooms.push(Zoom { at, len, on, line: e.line });
        }

        let mut beds: Vec<Bed> = Vec::new();
        for e in list("music")? {
            e.keys(&["chapter", "file", "loop", "gain"])?;
            let n = match e.t.get("chapter") {
                Some(toml::Value::Integer(n)) if *n >= 1 && (*n as usize) <= chapters.len() => *n as usize,
                Some(_) if chapters.is_empty() => {
                    return Err(e.err("chapter", "music: the file has no chapter for it to start at"));
                }
                Some(_) => {
                    return Err(
                        e.err("chapter", format!("music: chapter 1 to {} (its number)", chapters.len()))
                    );
                }
                None => {
                    return Err(src.at(e.line, "music: no chapter (the number of the chapter it starts at)"));
                }
            };
            if let Some(b) = beds.last()
                && n <= b.chapter + 1
            {
                return Err(e.err(
                    "chapter",
                    format!(
                        "music: chapter {n}, not after the bed at line {} (chapter {})",
                        b.line,
                        b.chapter + 1
                    ),
                ));
            }
            let file = e.needed("file")?.trim().to_string();
            if file.is_empty() {
                return Err(e.err("file", "music: its file is empty"));
            }
            let looped = match e.t.get("loop") {
                None => None,
                Some(toml::Value::Array(a)) if a.len() == 2 => {
                    let at = |v: &toml::Value| -> Result<u64> {
                        let s = v
                            .as_str()
                            .ok_or_else(|| e.err("loop", "music: loop = [\"0:08.2\", \"1:52\"]"))?;
                        match s.contains(':') {
                            true => clock_at(s.trim()),
                            false => length(s),
                        }
                        .map_err(|m| e.err("loop", format!("music: loop: {m}")))
                    };
                    let (a, b) = (at(&a[0])?, at(&a[1])?);
                    if b < a + LOOP_MIN {
                        return Err(e.err(
                            "loop",
                            format!(
                                "music: loop from {:.1} s to {:.1} s — its end {} s or more after its start",
                                a as f64 / 1e3,
                                b as f64 / 1e3,
                                LOOP_MIN / 1000
                            ),
                        ));
                    }
                    Some((a, b))
                }
                Some(_) => {
                    return Err(e.err("loop", "music: loop = [\"0:08.2\", \"1:52\"], its start and end"));
                }
            };
            let gain = match e.t.get("gain") {
                None => None,
                Some(v) => match number(v) {
                    Some(g) if (GAIN_LEAST_DB..=GAIN_MOST_DB).contains(&g) => Some(g as f32),
                    _ => {
                        return Err(e.err(
                            "gain",
                            format!(
                                "music: gain = dB on the file as made (0: as it is), {GAIN_LEAST_DB} to {GAIN_MOST_DB}"
                            ),
                        ));
                    }
                },
            };
            beds.push(Bed { chapter: n - 1, file, looped, gain, line: e.line });
        }
        let mut mix = sound::Mix::default();
        match top.get("sound") {
            None => {}
            Some(toml::Value::Table(t)) => {
                let e = Entry { t, line: src.header("sound", None), src: &src, name: "sound" };
                e.keys(&["under", "over"])?;
                let lu = |key: &str, least: f64| -> Result<Option<f32>> {
                    match t.get(key) {
                        None => Ok(None),
                        Some(v) => match number(v) {
                            Some(n) if (least..=30.0).contains(&n) => Ok(Some(n as f32)),
                            _ => Err(e.err(key, format!("sound: {key} = a number of LU, {least} to 30"))),
                        },
                    }
                };
                mix.under = lu("under", -10.0)?.unwrap_or(mix.under);
                mix.over = lu("over", 0.0)?.unwrap_or(mix.over);
            }
            Some(_) => return Err(src.at(src.key(0, "sound"), "sound: a table, as [sound]")),
        }
        Ok(Cut {
            mix,
            file: file.to_string(),
            bg,
            mascot,
            map,
            opening,
            chapters,
            captions,
            zooms,
            beds,
            end: marks.end,
        })
    }

    /// The file's `[opening]`: the card's words, each one line within its longest, and where it ends — refused, naming
    /// what's missing, where it isn't there.
    fn opening(top: &toml::Table, src: &Src, marks: &Marks) -> Result<Opening> {
        const WHAT: &str = "the card of what's built — name, line, code (up to two lines) — and where it ends, at and for (and zoom)";
        let t = match top.get("opening") {
            Some(toml::Value::Table(t)) => t,
            Some(_) => return Err(src.at(src.key(0, "opening"), "opening: a table, as [opening]")),
            None => return Err(src.at(1, format!("no [opening]: {WHAT}"))),
        };
        let e = Entry { t, line: src.header("opening", None), src, name: "opening" };
        e.keys(&["name", "line", "code", "at", "for", "zoom"])?;
        let missing: Vec<&str> =
            ["name", "line", "at", "for"].into_iter().filter(|k| !t.contains_key(*k)).collect();
        if !missing.is_empty() {
            return Err(src.at(e.line, format!("opening: no {} ({WHAT})", missing.join(", "))));
        }
        let one_line = |key: &str, s: &str, max: usize| -> Result<String> {
            let s = s.trim();
            let n = s.chars().count();
            match n == 0 || n > max || s.contains('\n') {
                true => Err(
                    e.err(key, format!("opening: its {key}, one line of 1 to {max} characters ({n} here)"))
                ),
                false => Ok(s.to_string()),
            }
        };
        let name = one_line("name", e.needed("name")?, NAME_MAX)?;
        let line = one_line("line", e.needed("line")?, LINE_MAX)?;
        let shape = || e.err("code", format!("opening: code = [\"halil board\"], up to {CODE_LINES} lines"));
        let code = match t.get("code") {
            None => Vec::new(),
            Some(toml::Value::Array(a)) if a.len() <= CODE_LINES => (a.iter())
                .map(|v| v.as_str().ok_or_else(shape).and_then(|s| one_line("code", s, CODE_MAX)))
                .collect::<Result<_>>()?,
            Some(_) => return Err(shape()),
        };
        let (from, len) = (e.time("at", marks)?, e.length("for")?);
        if len == 0 {
            return Err(e.err("for", "opening: for = \"4s\" or so, where it ends"));
        }
        if from + len > marks.end {
            return Err(e.err(
                "for",
                format!("opening: where it ends runs past the take's end, {}", clock(marks.end)),
            ));
        }
        let zoom = match e.text("zoom")?.map(str::trim) {
            None => None,
            Some("") => return Err(e.err("zoom", "opening: zoom = \"card:Run these checks\", a target")),
            Some(on) => Some((on.to_string(), e.line("zoom"))),
        };
        Ok(Opening { name, line, code, from, len, zoom })
    }
}

// ── The video's parts ─────────────────────────────────────────────────────────────────────────────────────────────────

/// What a part of the video is: the intro, the card of what's built, where it ends (the take from its ms), a stretch of
/// the take part (from the map's µs), a chapter's card, the take's last screen held, the end card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum What {
    Intro,
    Built,
    Ends { from: u64 },
    Take { from: u64 },
    Chapter(usize),
    Last,
    End,
}

/// A part: what, from where in the video (µs), how long (µs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Part {
    pub(crate) what: What,
    pub(crate) at: u64,
    pub(crate) len: u64,
}

/// A card in place of the screen: the intro (its ms in), the card of what's built, a chapter's, the end card.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Card {
    Intro(u64),
    Built,
    Chapter(usize),
    End,
}

/// What one frame shows: the take's screen at its ms, or a card; and what the band says — the take's time, a sped
/// span's factor, the jump's mark (how much was left out, and how drawn, of 255), whether it's where it ends, a caption
/// or a chapter with no card (which, how drawn) —; and where in the map it is (the camera's clock), the take part's
/// frames only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Frame {
    pub(crate) screen: Option<u64>,
    pub(crate) card: Option<Card>,
    pub(crate) clock: Option<u64>,
    pub(crate) speed: Option<f64>,
    pub(crate) jump: Option<(u64, u8)>,
    pub(crate) ends: bool,
    pub(crate) caption: Option<(usize, u8)>,
    pub(crate) chapter: Option<(usize, u8)>,
    pub(crate) map: Option<u64>,
}

impl Frame {
    fn card(c: Card) -> Frame {
        Frame {
            screen: None,
            card: Some(c),
            clock: None,
            speed: None,
            jump: None,
            ends: false,
            caption: None,
            chapter: None,
            map: None,
        }
    }
}

/// How far a thing that stands `len` µs is drawn `since` µs into it (of 255): coming in and going out over `FADE_MS`.
pub(crate) fn envelope(since: u64, len: u64) -> u8 {
    if since >= len {
        return 0;
    }
    let fade = (FADE_MS * 1000) as f32;
    let a = (since as f32 / fade).min((len - since) as f32 / fade).clamp(0.0, 1.0);
    (a * 255.0).round() as u8
}

/// The video, part by part: the opening — the intro, the card of what's built, where it ends —, the take part with each
/// chapter's card where it starts, its last screen held, the end card; where time is left out (the map's µs, how much of the take);
/// each caption's stretch on the map, and each chapter's with no card (from, to, which).
pub(crate) struct Program<'c> {
    pub(crate) cut: &'c Cut,
    pub(crate) parts: Vec<Part>,
    pub(crate) total: u64,
    pub(crate) jumps: Vec<(u64, u64)>,
    captions: Vec<(u64, u64)>,
    banded: Vec<(u64, u64, usize)>,
}

impl<'c> Program<'c> {
    pub(crate) fn new(cut: &'c Cut) -> Program<'c> {
        let map = &cut.map;
        let mut parts = Vec::new();
        let mut at = 0;
        let mut push = |what: What, len: u64| {
            parts.push(Part { what, at, len });
            at += len;
        };
        push(What::Intro, INTRO_MS * 1000);
        push(What::Built, BUILT_MS * 1000);
        push(What::Ends { from: cut.opening.from }, cut.opening.len * 1000);
        let mut from = 0;
        for (i, c) in cut.chapters.iter().enumerate().filter(|(_, c)| c.card) {
            let m = map.video(c.at);
            if m > from {
                push(What::Take { from }, m - from);
                from = m;
            }
            push(What::Chapter(i), CHAPTER_MS * 1000);
        }
        if map.len() > from {
            push(What::Take { from }, map.len() - from);
        }
        push(What::Last, video::LAST_MS * 1000);
        push(What::End, video::END_MS * 1000);
        let mut jumps: Vec<(u64, u64)> = Vec::new();
        for (i, s) in map.segs().iter().enumerate().filter(|(_, s)| s.pace == Pace::Out) {
            match jumps.last_mut() {
                Some(j) if j.0 == map.seg_at(i) => j.1 += s.to - s.from,
                _ => jumps.push((map.seg_at(i), s.to - s.from)),
            }
        }
        let captions =
            cut.captions.iter().map(|c| (map.video(c.at), map.video(c.at) + c.len * 1000)).collect();
        let banded = (cut.chapters.iter().enumerate().filter(|(_, c)| !c.card))
            .map(|(i, c)| (map.video(c.at), map.video(c.at) + CHAPTER_BAND_MS * 1000, i))
            .collect();
        Program { cut, parts, total: at, jumps, captions, banded }
    }

    /// Where chapter `i` starts in the video (µs): its card's start, or (no card) the take's frame where it begins.
    pub(crate) fn chapter_at(&self, i: usize) -> u64 {
        let card = self.parts.iter().find(|p| p.what == What::Chapter(i)).map(|p| p.at);
        card.or_else(|| self.video_of(self.cut.map.video(self.cut.chapters[i].at))).unwrap_or(0)
    }

    /// The video's frames at `FPS`.
    pub(crate) fn frames(&self) -> u64 {
        (self.total * FPS).div_ceil(1_000_000)
    }

    /// Frame `k`'s time (µs).
    pub(crate) fn time_of(k: u64) -> u64 {
        k * 1_000_000 / FPS
    }

    /// The first frame at or after `v` µs.
    pub(crate) fn frame_at(v: u64) -> u64 {
        (v * FPS).div_ceil(1_000_000)
    }

    /// The part the video's `v` µs is in (the last past the end).
    pub(crate) fn part(&self, v: u64) -> &Part {
        &self.parts[self.parts.partition_point(|p| p.at <= v).saturating_sub(1)]
    }

    /// Where the map's `m` µs is in the video (the take part's frame that shows it; the last screen held for its end).
    pub(crate) fn video_of(&self, m: u64) -> Option<u64> {
        (self.parts.iter())
            .find_map(|p| match p.what {
                What::Take { from } if from <= m && m < from + p.len => Some(p.at + m - from),
                _ => None,
            })
            .or_else(|| {
                (m >= self.cut.map.len())
                    .then(|| self.parts.iter().find(|p| p.what == What::Last).map(|p| p.at))?
            })
    }

    /// What the frame at the video's `v` µs shows.
    pub(crate) fn frame(&self, v: u64) -> Frame {
        let p = self.part(v);
        let off = v.saturating_sub(p.at);
        match p.what {
            What::Intro => Frame::card(Card::Intro(off / 1000)),
            What::Built => Frame::card(Card::Built),
            What::Ends { from } => {
                let t = (from + off / 1000).min(self.cut.end);
                Frame { screen: Some(t), card: None, clock: Some(t), ends: true, ..Frame::card(Card::Built) }
            }
            What::Chapter(i) => Frame::card(Card::Chapter(i)),
            What::End => Frame::card(Card::End),
            What::Take { from } => self.take_frame(from + off, true),
            What::Last => self.take_frame(self.cut.map.len(), false),
        }
    }

    fn take_frame(&self, m: u64, moving: bool) -> Frame {
        let map = &self.cut.map;
        let (t, i) = map.take(m);
        let speed = match map.segs()[i].pace {
            Pace::Speed(k) if moving && k > 1.0 + 1e-6 => Some(k),
            _ => None,
        };
        let jump = (self.jumps.iter().rev().find(|j| j.0 <= m))
            .map(|j| (j.1, envelope(m - j.0, (JUMP_MS + 2 * FADE_MS) * 1000)))
            .filter(|j| j.1 > 0);
        let caption = (self.captions.iter().enumerate())
            .find(|(_, c)| c.0 <= m && m < c.1)
            .map(|(k, c)| (k, envelope(m - c.0, c.1 - c.0)))
            .filter(|c| c.1 > 0);
        let chapter = (self.banded.iter().find(|c| c.0 <= m && m < c.1))
            .map(|c| (c.2, envelope(m - c.0, c.1 - c.0)))
            .filter(|c| c.1 > 0 && moving);
        Frame {
            screen: Some(t),
            card: None,
            clock: Some(t),
            speed,
            jump,
            ends: false,
            caption,
            chapter,
            map: Some(m),
        }
    }

    /// Where the video crosses from one picture to the next (µs), and whether through the ground (`through`): each
    /// part's start past the first, through it — never one part's words over the next's —; each jump, the take's two
    /// screens over each other (where a part starts too: through the ground).
    pub(crate) fn crossings(&self) -> Vec<(u64, bool)> {
        let mut out: Vec<(u64, bool)> = self.parts.iter().skip(1).map(|p| (p.at, true)).collect();
        out.extend(self.jumps.iter().filter_map(|j| self.video_of(j.0)).map(|v| (v, false)));
        out.sort_unstable_by_key(|c| (c.0, !c.1));
        out.dedup_by_key(|c| c.0);
        out
    }

    /// YouTube's chapter list: 0:00 the opening, by the name of what's built (the intro, the card, where it ends), then
    /// each chapter's card's time in the video and its title; and what keeps YouTube from taking it (fewer than three,
    /// one under ten seconds), said.
    pub(crate) fn chapters(&self) -> (String, Vec<String>) {
        let list: Vec<(u64, String)> = [(0, self.cut.opening.name.clone())]
            .into_iter()
            .chain((self.cut.chapters.iter().enumerate()).map(|(i, c)| (self.chapter_at(i), c.title.clone())))
            .collect();
        let text: String = list.iter().map(|(at, t)| format!("{} {t}\n", chapter_time(*at))).collect();
        let mut said = Vec::new();
        if list.len() < 3 {
            said.push(format!(
                "chapters.txt: {} chapters — YouTube takes a list of three or more",
                list.len()
            ));
        }
        let ends: Vec<u64> = list.iter().skip(1).map(|c| c.0).chain([self.total]).collect();
        for ((at, t), end) in list.iter().zip(ends) {
            if end - at < 10_000_000 {
                said.push(format!(
                    "chapters.txt: “{t}” runs {:.1} s — YouTube wants ten or more",
                    (end - at) as f64 / 1e6
                ));
            }
        }
        (text, said)
    }
}
