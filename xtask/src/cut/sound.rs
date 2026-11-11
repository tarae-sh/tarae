//! The cut's sound (the full video's V2, `.sb/full-video-v2-brief.md`): tarae's own sounds where the picture runs in
//! real time, music where it doesn't, and the two never fighting.
//!
//! - **Keys**: the take's own input log (`keys.bin`), each key through the time map — heard at its video time where its
//!   stretch is shown at `KEYS_MAX_SPEED` or slower (and where it ends, in the opening), not in a faster one nor where
//!   it's left out.
//! - **The stamp** where a card is answered (its landmark; the key that answered it, `STAMP_KEY_MS` before, is the
//!   stamp's own), and **the brush** on the end card, as in the Shorts, and again with the intro's swing (the end card's
//!   as mastered, laid there too: the two bookends sound alike) — all mastered as a Short's keys are
//!   (`sound::track`: brought toward `sound::SHORT_LUFS`, the limiter taking at most `LIMIT_MOST_DB` from a click).
//! - **Beds** (`[[music]]`): each from its chapter on, its file's leading silence left out (`sound::sounds_from`), each
//!   at one loudness (`BED_REF_LUFS`) before the level below; crossed into the next over that chapter's card (or
//!   `CROSS_US` round a chapter with no card), equal power; repeating between its loop's two marks with a `SEAM_MS`
//!   seam; where it runs out with no loop, faded over `CROSS_US` to its end and the next begun under it; the first in
//!   under the intro's last `BED_UNDER_INTRO_US` (from chapter 1; else over `sound::BED_IN_S` at its chapter), leading
//!   through the card of what's built; the last out over the end card, under the brush.
//! - **Who leads** follows the picture's speed, the levels by distance (`Mix`, the edit list's `[sound]`, else
//!   `WIDE_UNDER_LU` and `LEAD_OVER_LU`): where the keys may sound (`realtime`), the bed `under` LU under the keys' own
//!   loudness (each alone); on a sped span and the cards it leads, `over` LU over that; eased `DUCK_US` — down before
//!   the stretch, up after it. The whole aims at no loudness: it's said (`cut.txt`), with its quietest and loudest
//!   three seconds.
//! - **The whole** limited under the ceiling (true peaks) that brings its AAC under `video::AAC_PEAK_DB`, as a Short's.

use std::path::Path;
use std::time::Duration;

use super::{Marks, Pace, Program, What, ms_of, video_time};
use crate::Result;
use crate::record::Rec;
use crate::sound::{self, Heard, RATE};
use crate::video;

/// The fastest span whose keys are heard (and the bed lies under them): an ask typed at 2× still sounds.
pub(crate) const KEYS_MAX_SPEED: f64 = 2.0;
/// A crossing between beds round a chapter with no card, and a bed's fade where it runs out.
pub(crate) const CROSS_US: u64 = 2_000_000;
/// A loop's seam: the loop's start eased in under what follows its end.
pub(crate) const SEAM_MS: u64 = 30;
/// The bed eased under the keys and back up, each way.
pub(crate) const DUCK_US: u64 = 300_000;
/// The wide video's bed where the keys may sound: this far under the keys' own loudness (LU, each alone) — apart from
/// the Shorts' `sound::BED_UNDER_LU`. At the keys' own: the music stays present while a card is read, a click's peak
/// still far over it (V2's calls, C — a guess from numbers until the user's ears pick).
pub(crate) const WIDE_UNDER_LU: f32 = 0.0;
/// The bed where it leads (the sped spans, the cards): this far over its level under the keys (LU) — the swing.
pub(crate) const LEAD_OVER_LU: f32 = 6.0;
/// Every bed brought to this loudness alone before its level: one track as loud as the next.
pub(crate) const BED_REF_LUFS: f32 = -23.0;
/// The quietest and loudest stretch `cut.txt` names: a window this long (BS.1770's short-term), stepped by `STEP_S`.
const WINDOW_S: f32 = 3.0;
const STEP_S: f32 = 1.0;
/// A key this soon before a card's answer is the answer: the stamp is its sound.
pub(crate) const STAMP_KEY_MS: u64 = 150;
/// The first bed comes in under the intro's last this much (µs), then leads through the card of what's built.
pub(crate) const BED_UNDER_INTRO_US: u64 = 500_000;

// ── What's heard ─────────────────────────────────────────────────────────────────────────────────────────────

/// Where a moment of the take (ms) is heard: at these video times (µs: where it ends', the take part's), or nowhere —
/// its stretch sped past `KEYS_MAX_SPEED`, or left out.
#[derive(Debug, PartialEq)]
pub(crate) enum Where {
    At(Vec<u64>),
    Fast,
    Out,
}

pub(crate) fn heard_at(prog: &Program, t: u64) -> Where {
    let cut = prog.cut;
    let mut at = Vec::new();
    let o = &cut.opening;
    if o.ends(t)
        && let Some(p) = prog.parts.iter().find(|p| matches!(p.what, What::Ends { .. }))
    {
        at.push(p.at + (t - o.from) * 1000);
    }
    let map = &cut.map;
    let missed = match map.segs()[map.seg_of(t)].pace {
        Pace::Out => Where::Out,
        Pace::Speed(k) if k > KEYS_MAX_SPEED + 1e-6 => Where::Fast,
        Pace::Speed(_) => {
            at.extend(prog.video_of(map.video(t)));
            Where::Out
        }
    };
    match at.is_empty() {
        true => missed,
        false => Where::At(at),
    }
}

/// A key's stroke as `sound::track` hears it — a return, the space bar, else a key —; None for a key let go (the
/// keyboard protocol's release, `…:3u`).
pub(crate) fn stroke(b: &[u8]) -> Option<char> {
    if super::init::is_return(b) {
        return Some('\r');
    }
    if let Some(body) = b.strip_prefix(b"\x1b[").and_then(|r| r.strip_suffix(b"u")) {
        if body.ends_with(b":3") {
            return None;
        }
        let code: String = body.iter().take_while(|c| c.is_ascii_digit()).map(|&c| c as char).collect();
        return Some(if code == "32" { ' ' } else { 'k' });
    }
    Some(if b == b" " { ' ' } else { 'k' })
}

/// What the cut hears, at its video times, and what it doesn't: key strokes heard (one key heard twice where the
/// opening shows it too), keys not heard for their stretch's speed or because it's left out; stamps heard and not;
/// where the brush starts (µs) on the end card, and with the intro.
#[derive(Debug, Default)]
pub(crate) struct Sounds {
    pub(crate) list: Vec<(Duration, Heard)>,
    pub(crate) keys: usize,
    pub(crate) fast: usize,
    pub(crate) out: usize,
    pub(crate) stamps: usize,
    pub(crate) stamps_missed: usize,
    pub(crate) brush: Option<u64>,
    pub(crate) intro: Option<u64>,
}

/// The cut's sounds from the take's input log (`keys`) and its landmarks.
pub(crate) fn sounds(prog: &Program, keys: &[Rec], marks: &Marks) -> Sounds {
    let answered: Vec<u64> = marks.of("card-answered").map(ms_of).collect();
    let mut s = Sounds::default();
    let us = Duration::from_micros;
    for r in keys.iter().filter(|r| super::init::is_key(r)) {
        let t = r.us / 1000;
        if answered.iter().any(|&a| a >= t && a - t <= STAMP_KEY_MS) {
            continue;
        }
        let Some(c) = stroke(&r.bytes) else { continue };
        match heard_at(prog, t) {
            Where::At(vs) => {
                s.keys += vs.len();
                s.list.extend(vs.into_iter().map(|v| (us(v), Heard::Key(c))));
            }
            Where::Fast => s.fast += 1,
            Where::Out => s.out += 1,
        }
    }
    for &a in &answered {
        match heard_at(prog, a) {
            Where::At(vs) => {
                s.stamps += vs.len();
                s.list.extend(vs.into_iter().map(|v| (us(v), Heard::Stamp)));
            }
            _ => s.stamps_missed += 1,
        }
    }
    s.brush = prog.parts.iter().find(|p| p.what == What::End).map(|p| p.at);
    s.intro = prog.parts.iter().find(|p| p.what == What::Intro).map(|p| p.at);
    s.list.sort_by_key(|h| h.0);
    s
}

/// Where the keys may sound (video µs, apart, in order): where it ends, and each stretch of the take part shown at
/// `KEYS_MAX_SPEED` or slower — where the bed lies under the keys.
pub(crate) fn realtime(prog: &Program) -> Vec<(u64, u64)> {
    let map = &prog.cut.map;
    let mut out: Vec<(u64, u64)> = Vec::new();
    let mut push = |a: u64, b: u64| match out.last_mut() {
        _ if b <= a => {}
        Some(l) if l.1 >= a => l.1 = l.1.max(b),
        _ => out.push((a, b)),
    };
    for p in &prog.parts {
        match p.what {
            What::Ends { .. } => push(p.at, p.at + p.len),
            What::Take { from } => {
                for (i, s) in map.segs().iter().enumerate() {
                    if !matches!(s.pace, Pace::Speed(k) if k <= KEYS_MAX_SPEED + 1e-6) {
                        continue;
                    }
                    let (a, b) =
                        (map.seg_at(i).max(from), (map.seg_at(i) + map.seg_len(i)).min(from + p.len));
                    if a < b {
                        push(p.at + a - from, p.at + b - from);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

// ── The beds ────────────────────────────────────────────────────────────────────────────────────────────────────

/// A bed placed on the video: which (`cut.beds`' index), where it starts (µs) and from which sample of its file, its
/// way in and out (µs, from–to, equal power), each loop's seam (µs) and where it ran out (µs; no loop).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placed {
    pub(crate) bed: usize,
    pub(crate) start: u64,
    pub(crate) from: usize,
    pub(crate) fade_in: (u64, u64),
    pub(crate) fade_out: (u64, u64),
    pub(crate) seams: Vec<u64>,
    pub(crate) ran_out: Option<u64>,
}

fn samples(us: u64) -> usize {
    (us as f64 * f64::from(RATE) / 1e6).round() as usize
}

fn micros(n: usize) -> u64 {
    (n as f64 * 1e6 / f64::from(RATE)).round() as u64
}

/// The first bed's way in (µs, from–to): from chapter 1, under the intro's last `BED_UNDER_INTRO_US`; from a later
/// chapter, over `sound::BED_IN_S` from its crossing.
pub(crate) fn first_in(prog: &Program, chapter: usize) -> (u64, u64) {
    match (chapter, prog.parts.iter().find(|p| p.what == What::Intro)) {
        (0, Some(p)) => ((p.at + p.len).saturating_sub(BED_UNDER_INTRO_US), p.at + p.len),
        _ => {
            let a = if chapter == 0 { 0 } else { crossing(prog, chapter).0 };
            (a, a + (sound::BED_IN_S * 1e6) as u64)
        }
    }
}

/// Chapter `i`'s crossing (µs): its card, else `CROSS_US` round where it begins.
pub(crate) fn crossing(prog: &Program, i: usize) -> (u64, u64) {
    match prog.parts.iter().find(|p| p.what == What::Chapter(i)) {
        Some(p) => (p.at, p.at + p.len),
        None => {
            let c = prog.chapter_at(i);
            (c.saturating_sub(CROSS_US / 2), c + CROSS_US / 2)
        }
    }
}

/// The cut's beds laid out, each `tracks[j]` (its file's length in samples, where its sound starts): where each starts,
/// crosses, loops and runs out — and what a bed that runs out did, said.
pub(crate) fn layout(prog: &Program, tracks: &[(usize, usize)]) -> (Vec<Placed>, Vec<String>) {
    let beds = &prog.cut.beds;
    let end = (prog.parts.iter().find(|p| p.what == What::End))
        .map_or((prog.total, prog.total), |p| (p.at, p.at + p.len));
    let (mut out, mut said) = (Vec::new(), Vec::new());
    let mut early: Option<(u64, u64)> = None;
    for (j, bed) in beds.iter().enumerate() {
        let fade_in = match (early.take(), j) {
            (Some(w), _) => w,
            (None, 0) => first_in(prog, bed.chapter),
            (None, _) => crossing(prog, bed.chapter),
        };
        let start = fade_in.0;
        let fade_out = beds.get(j + 1).map_or(end, |n| crossing(prog, n.chapter));
        let (len, from) = tracks[j];
        let mut p = Placed { bed: j, start, from, fade_in, fade_out, seams: Vec::new(), ran_out: None };
        match bed.looped {
            Some((a, b)) => {
                let (a, b) = (samples(a * 1000), samples(b * 1000));
                let mut seam = start + micros(b.saturating_sub(from));
                while seam < fade_out.1 {
                    p.seams.push(seam);
                    seam += micros(b - a);
                }
            }
            None => {
                let runs = start + micros(len.saturating_sub(from));
                if runs < fade_out.1 {
                    let w = (runs.saturating_sub(CROSS_US).max(fade_in.1).min(runs), runs);
                    p.fade_out = w;
                    p.ran_out = Some(runs);
                    said.push(match beds.get(j + 1) {
                        Some(n) => {
                            early = Some(w);
                            format!(
                                "{}:{}: bed {} runs out at {} in the video, before its end at {} — it fades from {}, and \
                                 bed {} (line {}) begins under it",
                                prog.cut.file,
                                bed.line,
                                j + 1,
                                video_time(runs),
                                video_time(fade_out.1),
                                video_time(w.0),
                                j + 2,
                                n.line
                            )
                        }
                        None => format!(
                            "{}:{}: bed {} runs out at {} in the video, before the end card's end at {} — it fades from \
                             {}, and the rest is silent",
                            prog.cut.file,
                            bed.line,
                            j + 1,
                            video_time(runs),
                            video_time(fade_out.1),
                            video_time(w.0)
                        ),
                    });
                }
            }
        }
        out.push(p);
    }
    (out, said)
}

/// One bed onto the video's samples (`out`, at `gain`): its file's from `p.from` on, repeating between `looped`'s two
/// samples — at each seam what follows the loop's end eased out over `SEAM_MS` as its start comes in —, eased in and
/// out as `p` says (equal power: sine in, cosine out); nothing outside.
pub(crate) fn lay(p: &Placed, track: &[f32], looped: Option<(usize, usize)>, gain: f32, out: &mut [f32]) {
    let n_of = |us: u64| samples(us).min(out.len());
    let (s0, s1) = (n_of(p.start), n_of(p.fade_out.1));
    let (i0, i1) = (n_of(p.fade_in.0), n_of(p.fade_in.1));
    let (o0, o1) = (n_of(p.fade_out.0), n_of(p.fade_out.1));
    let seam = (SEAM_MS * u64::from(RATE) / 1000) as usize;
    let get = |i: usize| track.get(i).copied().unwrap_or(0.0);
    let quarter = std::f32::consts::FRAC_PI_2;
    for (n, y) in out.iter_mut().enumerate().take(s1).skip(s0) {
        let pos = p.from + (n - s0);
        let x = match looped {
            Some((a, b)) if pos >= b && b > a => {
                let d = (pos - b) % (b - a);
                match d < seam {
                    true => {
                        let w = d as f32 / seam as f32;
                        get(a + d) * w + get(b + d) * (1.0 - w)
                    }
                    false => get(a + d),
                }
            }
            _ => get(pos),
        };
        let into = match n < i1 {
            true => (quarter * (n.saturating_sub(i0)) as f32 / (i1 - i0).max(1) as f32).sin(),
            false => 1.0,
        };
        let away = match n >= o0 {
            true => (quarter * ((n - o0) as f32 / (o1 - o0).max(1) as f32).min(1.0)).cos(),
            false => 1.0,
        };
        *y += x * gain * into * away;
    }
}

/// The end card's brush as mastered (`len` samples from `end`) laid again from `intro`: the two bookends sound alike.
pub(crate) fn intro_brush(track: &mut [f32], end: usize, intro: usize, len: usize) {
    let len = len.min(track.len().saturating_sub(end)).min(track.len().saturating_sub(intro));
    if intro + len <= end {
        let (a, b) = track.split_at_mut(end);
        a[intro..intro + len].iter_mut().zip(&b[..len]).for_each(|(y, x)| *y += x);
    }
}

/// How far under the keys the bed lies at each of `len` samples (0 leading, 1 under): 1 over each of `realtime`'s
/// stretches, eased in over the `DUCK_US` before it and back out over the `DUCK_US` after — the most of any stretch's.
pub(crate) fn under(realtime: &[(u64, u64)], len: usize) -> Vec<f32> {
    let mut d = vec![0.0f32; len];
    let at = |us: u64| samples(us).min(len);
    let ramp = samples(DUCK_US).max(1) as f32;
    for &(a, b) in realtime {
        let (sa, sb) = (at(a), at(b));
        for (n, v) in d.iter_mut().enumerate().take(sa).skip(at(a.saturating_sub(DUCK_US))) {
            *v = v.max(1.0 - (sa - n) as f32 / ramp);
        }
        d[sa..sb].fill(1.0);
        for (n, v) in d.iter_mut().enumerate().take(at(b + DUCK_US)).skip(sb) {
            *v = v.max(1.0 - (n - sb) as f32 / ramp);
        }
    }
    d
}

/// The mix by distance (LU): the bed `under` the keys' own loudness where they may sound, and `over` that where it
/// leads. The edit list's `[sound]`, else `WIDE_UNDER_LU` and `LEAD_OVER_LU`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Mix {
    pub(crate) under: f32,
    pub(crate) over: f32,
}

impl Default for Mix {
    fn default() -> Mix {
        Mix { under: WIDE_UNDER_LU, over: LEAD_OVER_LU }
    }
}

/// The bed's gain where it leads (dB over its laid level, `BED_REF_LUFS`) and how much lower it lies under the keys
/// (dB), the keys at `keys` LUFS: `mixed`'s two numbers.
pub(crate) fn bed_gains(keys: f32, mix: Mix) -> (f32, f32) {
    (keys - mix.under + mix.over - BED_REF_LUFS, mix.over)
}

/// `keys` with `beds` under them: each bed sample at `lead_db` where the bed leads, `depth_db` lower where it lies
/// under the keys (`d` of the way between in dB).
pub(crate) fn mixed(keys: &[f32], beds: &[f32], d: &[f32], lead_db: f32, depth_db: f32) -> Vec<f32> {
    let k = std::f32::consts::LN_10 / 20.0;
    (keys.iter().zip(beds).zip(d)).map(|((x, b), d)| x + b * ((lead_db - d * depth_db) * k).exp()).collect()
}

// ── The track ─────────────────────────────────────────────────────────────────────────────────────────────────────

/// One bed's file read: its samples (mono at `RATE`), where its sound starts, its loop in samples.
struct Track {
    samples: Vec<f32>,
    from: usize,
    looped: Option<(usize, usize)>,
}

/// The levels a track came to: the keys alone (LUFS, their gain in dB, the most the limiter took from them), the bed
/// alone where it leads and where it lies under (LUFS), the whole (LUFS, dBTP) and what the limiter took from it (most,
/// when in s, how long more than `sound::TOUCHED_DB`).
struct Levels {
    keys: f32,
    keys_gain: f32,
    keys_limited: f32,
    lead: f32,
    under: f32,
    whole: f32,
    true_peak: f32,
    limited: (f32, f32, f32),
}

/// The cut's sound into `wav`: what's heard (`sounds`), the beds read with ffmpeg from `dir` (the edit list's folder),
/// laid, levelled and limited — the limiter's ceiling moved until the AAC comes out under `video::AAC_PEAK_DB`. What it
/// made, said (cut.txt's lines). The intro's swing starts `swing_skip` µs into its frames as made (the empty ones left
/// out): its brush the same, its head (a silence) cut by as much.
pub(crate) fn make(
    prog: &Program,
    dir: &Path,
    keys: &[Rec],
    marks: &Marks,
    (ffmpeg, swing_skip): (&Path, u64),
    wav: &Path,
) -> Result<Vec<String>> {
    let cut = prog.cut;
    let heard = sounds(prog, keys, marks);
    let length = Duration::from_micros(prog.total);
    let len = samples(prog.total);
    let mut said = Vec::new();

    // (the beds: read, laid out, each brought to one loudness and summed)
    let mut tracks = Vec::new();
    let end = prog.parts.iter().find(|p| p.what == What::End).map_or(prog.total, |p| p.at + p.len);
    for (j, bed) in cut.beds.iter().enumerate() {
        let path = match Path::new(&bed.file).is_absolute() {
            true => Path::new(&bed.file).to_path_buf(),
            false => dir.join(&bed.file),
        };
        if !path.is_file() {
            return Err(format!("{}:{}: music: {}: no such file", cut.file, bed.line, path.display()));
        }
        let start = if j == 0 { first_in(prog, bed.chapter).0 } else { crossing(prog, bed.chapter).0 };
        let upto = Duration::from_micros(end - start) + Duration::from_secs(10);
        let upto = bed.looped.map_or(upto, |(_, b)| upto.min(Duration::from_millis(b + 1_000)));
        let s = video::decoded(ffmpeg, &path, upto)
            .map_err(|e| format!("{}:{}: music: {e}", cut.file, bed.line))?;
        let from = sound::sounds_from(&s);
        let looped = bed.looped.map(|(a, b)| (samples(a * 1000), samples(b * 1000)));
        if let Some((_, b)) = looped {
            let seam = samples(SEAM_MS * 1000);
            if b + seam > s.len() {
                return Err(format!(
                    "{}:{}: music: its loop ends at {:.1} s, past the track's end ({:.1} s)",
                    cut.file,
                    bed.line,
                    b as f64 / f64::from(RATE),
                    s.len().saturating_sub(seam) as f64 / f64::from(RATE)
                ));
            }
            if b <= from {
                return Err(format!(
                    "{}:{}: music: its loop ends at {:.1} s, before its sound starts ({:.1} s)",
                    cut.file,
                    bed.line,
                    b as f64 / f64::from(RATE),
                    from as f64 / f64::from(RATE)
                ));
            }
        }
        tracks.push(Track { samples: s, from, looped });
    }
    let (placed, ran) = layout(prog, &tracks.iter().map(|t| (t.samples.len(), t.from)).collect::<Vec<_>>());
    let mut beds = vec![0.0f32; len];
    for (p, t) in placed.iter().zip(&tracks) {
        let mut one = vec![0.0f32; len];
        lay(p, &t.samples, t.looped, 1.0, &mut one);
        let (a, b) = (samples(p.start).min(len), samples(p.fade_out.1).min(len));
        let l = sound::lufs(&one[a..b]);
        let g = if l.is_finite() { 10f32.powf((BED_REF_LUFS - l) / 20.0) } else { 1.0 };
        beds.iter_mut().zip(&one).for_each(|(y, x)| *y += x * g);
    }
    drop(tracks);
    let real = realtime(prog);
    let share = real.iter().map(|(a, b)| b - a).sum::<u64>() as f64 / prog.total.max(1) as f64;
    let d = under(&real, len);

    // (the ceiling moved until the AAC's true peak is under its aim, as a Short's)
    let brush = heard.brush.map(Duration::from_micros);
    let brush_len = sound::brush().len();
    let made = |ceiling: f32| -> (Vec<f32>, Levels) {
        let mut keys = sound::track(&heard.list, length, brush, ceiling, None);
        if let (Some(b), Some(i)) = (heard.brush, heard.intro) {
            let skip = samples(swing_skip).min(brush_len);
            intro_brush(&mut keys.samples, samples(b) + skip, samples(i), brush_len - skip);
        }
        let loud = Some(keys.lufs).filter(|l| l.is_finite()).unwrap_or(sound::SHORT_LUFS);
        let (x, depth) = bed_gains(loud, cut.mix);
        let mut y = match cut.beds.is_empty() {
            true => keys.samples.clone(),
            false => mixed(&keys.samples, &beds, &d, x, depth),
        };
        let before = y.clone();
        let c = 10f32.powf(ceiling / 20.0);
        let (most, at) = sound::limit(&mut y, &sound::true_peaks(&before), c);
        let floor = 10f32.powf(-sound::TOUCHED_DB / 20.0);
        let touched =
            (before.iter().zip(&y)).filter(|(m, y)| m.abs() > 1e-6 && y.abs() < m.abs() * floor).count();
        // (the bed alone where it leads and where it lies under)
        let alone = |want: f32| -> f32 {
            let k = std::f32::consts::LN_10 / 20.0;
            let picked: Vec<f32> = (beds.iter().zip(&d))
                .filter(|(_, d)| (**d - want).abs() < 1e-6)
                .map(|(b, dd)| b * ((x - dd * depth) * k).exp())
                .collect();
            sound::lufs(&picked)
        };
        let levels = Levels {
            keys: keys.lufs,
            keys_gain: keys.gain,
            keys_limited: keys.limited.keys,
            lead: alone(0.0),
            under: alone(1.0),
            whole: sound::lufs(&y),
            true_peak: 20.0 * sound::true_peaks(&y).into_iter().fold(0.0f32, f32::max).log10(),
            limited: (most, at as f32 / RATE as f32, touched as f32 / RATE as f32),
        };
        (y, levels)
    };
    type Tried = (f32, std::result::Result<f32, String>);
    let (mut ceilings, mut tries): (Vec<Tried>, Vec<(Vec<f32>, Levels)>) = (Vec::new(), Vec::new());
    let mut ceiling = sound::SHORT_CEILING_DB;
    let measured = |c: &[Tried]| -> Vec<(f32, f32)> {
        c.iter().filter_map(|(c, p)| Some((*c, *p.as_ref().ok()?))).collect()
    };
    loop {
        let (y, levels) = made(ceiling);
        sound::wav(wav, &y)?;
        let peak = video::aac_peak(ffmpeg, wav);
        let failed = peak.is_err();
        ceilings.push((ceiling, peak));
        tries.push((y, levels));
        match video::next_ceiling(&measured(&ceilings)) {
            Some(next) if !failed && ceilings.len() < video::AAC_TRIES => ceiling = next,
            _ => break,
        }
    }
    let pick = video::best_ceiling(&measured(&ceilings))
        .and_then(|c| ceilings.iter().position(|t| t.0 == c))
        .unwrap_or(tries.len() - 1);
    if pick != tries.len() - 1 {
        sound::wav(wav, &tries[pick].0)?;
    }
    let (y, lv) = tries.swap_remove(pick);
    drop(tries);

    // (what it made, said)
    let s = |us: u64| video_time(us);
    said.push(format!(
        "sound: {} key strokes heard; {} keys not, their stretch faster than {}×, and {} left out; {} stamps heard{}; \
         the brush with the intro at {} and on the end card at {}",
        heard.keys,
        heard.fast,
        super::factor(KEYS_MAX_SPEED),
        heard.out,
        heard.stamps,
        match heard.stamps_missed {
            0 => String::new(),
            n => format!(", {n} not (a sped or left-out stretch)"),
        },
        heard.intro.map_or("—".into(), s),
        heard.brush.map_or("—".into(), s)
    ));
    let db = |x: f32| if x.is_finite() { format!("{x:.1}") } else { "—".into() };
    said.push(format!(
        "  the keys alone {} LUFS (gain {:+.1} dB, the limiter taking at most {:.1} dB from a click)",
        db(lv.keys),
        lv.keys_gain,
        lv.keys_limited
    ));
    if !cut.beds.is_empty() {
        said.push(format!(
            "  the bed alone: {} LUFS where it leads (sped spans, the cards), {} LUFS under the keys (spans at {}× or \
             slower, where it ends: {:.0} % of the video) — a swing of {} dB, eased {:.1} s each way; the mix {} LU under \
             the keys, {} over that{}",
            db(lv.lead),
            db(lv.under),
            super::factor(KEYS_MAX_SPEED),
            share * 100.0,
            db(lv.lead - lv.under),
            DUCK_US as f64 / 1e6,
            cut.mix.under,
            cut.mix.over,
            if cut.mix == Mix::default() { " (the constants)" } else { " (the list's [sound])" }
        ));
    }
    said.push(format!(
        "  the whole: {} LUFS integrated, {} dBTP (the WAV); the limiter took at most {:.1} dB (at {:.1} s), more than \
         {} dB for {:.2} s",
        db(lv.whole),
        db(lv.true_peak),
        lv.limited.0,
        lv.limited.1,
        sound::TOUCHED_DB,
        lv.limited.2
    ));
    // (the quietest and the loudest three seconds, by BS.1770's short-term loudness)
    let windows = sound::short_term(&y, WINDOW_S, STEP_S);
    let at = |i: usize| {
        let a = (i as f32 * STEP_S * 1e6) as u64;
        format!("{}–{}", s(a), s(a + (WINDOW_S * 1e6) as u64))
    };
    let by = |f: fn(f32, f32) -> bool| {
        (windows.iter().enumerate()).fold(None, |best: Option<(usize, f32)>, (i, &l)| match best {
            Some((_, b)) if !f(l, b) => best,
            _ => Some((i, l)),
        })
    };
    if let (Some(q), Some(l)) = (by(|a, b| a < b), by(|a, b| a > b)) {
        said.push(format!(
            "  the quietest {WINDOW_S} s {}: {} LUFS; the loudest {}: {} LUFS ({} LU between)",
            at(q.0),
            db(q.1),
            at(l.0),
            db(l.1),
            db(l.1 - q.1)
        ));
    }
    let tried: Vec<String> = (ceilings.iter())
        .map(|(c, p)| match p {
            Ok(p) => format!("{c:.2} → {p:.2}"),
            Err(e) => format!("{c:.2} → ({e})"),
        })
        .collect();
    said.push(format!("  the limiter's ceiling → the AAC's true peak (dBTP): {}", tried.join(", ")));
    // (each chapter's loudness: from its start — the first from 0:00 — to the next's)
    let starts: Vec<u64> =
        (0..cut.chapters.len()).map(|i| if i == 0 { 0 } else { prog.chapter_at(i) }).collect();
    for (i, c) in cut.chapters.iter().enumerate() {
        let (a, b) =
            (samples(starts[i]).min(len), samples(starts.get(i + 1).copied().unwrap_or(prog.total)).min(len));
        let tp = 20.0 * sound::true_peaks(&y[a..b]).into_iter().fold(0.0f32, f32::max).log10();
        said.push(format!(
            "  chapter {} “{}” {}–{}: {} LUFS, {} dBTP",
            i + 1,
            c.title,
            s(starts[i]),
            s(starts.get(i + 1).copied().unwrap_or(prog.total)),
            db(sound::lufs(&y[a..b])),
            db(tp)
        ));
    }
    for (p, bed) in placed.iter().zip(&cut.beds) {
        let name =
            Path::new(&bed.file).file_name().map_or(bed.file.clone(), |n| n.to_string_lossy().into_owned());
        let mut line = format!(
            "  bed {} “{name}” (line {}): in {}–{} (its sound from {:.2} s in the file), out {}–{}",
            p.bed + 1,
            bed.line,
            s(p.fade_in.0),
            s(p.fade_in.1),
            p.from as f64 / f64::from(RATE),
            s(p.fade_out.0),
            s(p.fade_out.1)
        );
        if let Some((a, b)) = bed.looped {
            let at: Vec<String> = p.seams.iter().map(|&v| s(v)).collect();
            line += &format!(
                "; loop {:.2} s → {:.2} s, its seams at {}",
                b as f64 / 1e3,
                a as f64 / 1e3,
                if at.is_empty() { "none (it never reaches its end)".into() } else { at.join(", ") }
            );
        }
        said.push(line);
    }
    said.extend(ran.into_iter().map(|r| format!("  {r}")));
    Ok(said)
}
