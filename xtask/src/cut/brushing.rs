//! The brush on the tablet (the ztunnel video's edit, Z3, after §6.2's painting): one soft stroke a sweep of the
//! character's — across the tablet in about a second (`motions::Painted::sweeps`) —, made from noise as the rain is
//! (`rain::Rng`, `rain::Biquad`), never downloaded and kept in no file. It starts on the sweep's first frame and lasts
//! as long as the brush crosses; silent while the brush is lifted and at rest. It follows the character's sweep, never
//! the board's strokes: a dab or a short stroke makes no sound of its own. Wherever the character paints, it's heard;
//! nowhere else, and a cut with no sweep sounds as it did without it, to the bit.
//!
//! The sound (the user's pick of the second round's four, "rub로 가봐 일단은"): the paper rubbed, a low "shh" — noise
//! in 280 Hz–1.5 kHz through a band at 700 Hz and a softer one at 1.25 kHz, apart from the rain's highs — whose
//! texture is the paper's grain read at the brush's pace: its fibres laid along the stroke, a bump each (and finer ones
//! between), slow as it lands, fastest mid-sweep, slow as it lifts. No crackle, no flutter of its own. It follows the
//! brush across: its press (landing over its first 12 %, easing off by a third, lifted over its last 20 %) and its speed
//! (still at its ends, fastest mid-sweep): it eases in as the press lands, no swell under it, no low column of its own
//! as it lands, never a click.
//!
//! Not one loop: a sweep is 38 frames, and the same stroke every sweep would be a metronome over a long picture. Each
//! sweep's stroke is a little its own (`Vary`): its pitch (its band 2–5 % up or down, one sweep up, the next down),
//! its grain's pace and depth, its loudness (within 1.5 dB), its landing and its lift — drawn from its picture, its
//! step and its place in the step, so a render is the same twice. Only so that no two sweeps in a row sound alike: the
//! stroke is still one a sweep, as long as the sweep, whatever the board draws under it.
//!
//! Laid in the centre with tarae's own sounds once they're mastered (`sound::make`): the beds lie under it as under the
//! keys, the whole's gain and limiter over it all; its level `UNDER_KEYS_LU` under the keys' own loudness, measured
//! over its sweeps alone.

use std::f64::consts::PI;

use super::Program;
use super::mascot::motions::Sweep;
use crate::rain::{Biquad, Rng, Shape, high_pass};
use crate::sound::{self, RATE};

/// The brush's loudness over its sweeps, this far under the keys' own (LU, each alone, in the centre): under the taps,
/// a little over the rain at its gain 6 (take 6's: the keys −19.2 LUFS, the rain −34.0 as laid). As the user heard it.
pub(crate) const UNDER_KEYS_LU: f32 = 12.0;

const R: f64 = RATE as f64;

/// How a sweep's stroke is its own: its streams' seed, its band's pitch (a factor), its grain's pace and depth
/// (factors), its loudness (dB) and its landing and lift (factors on their lengths).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Vary {
    pub(crate) seed: u64,
    pub(crate) pitch: f64,
    pub(crate) grain: f64,
    pub(crate) depth: f64,
    pub(crate) level_db: f64,
    pub(crate) into: f64,
    pub(crate) away: f64,
}

impl Vary {
    /// Sweep `s`'s, drawn from its picture, step and place in the step: the same sweep the same, its pitch 2–5 % up
    /// on an even place and as much down on an odd one (two in a row never alike), the rest within narrow ranges.
    pub(crate) fn of(s: &Sweep) -> Vary {
        let seed = [s.picture, s.step, s.nth]
            .iter()
            .fold(0xb7a5_4000_u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100_0000_01b3));
        let mut r = Rng::new(seed, 4);
        let up = r.range(0.02, 0.05);
        Vary {
            seed,
            pitch: if s.nth.is_multiple_of(2) { 1.0 + up } else { 1.0 - up },
            grain: r.range(0.8, 1.25),
            depth: r.range(0.85, 1.15),
            level_db: r.range(-1.5, 0.0),
            into: r.range(0.9, 1.2),
            away: r.range(0.8, 1.2),
        }
    }
}

/// A stroke's place on the track (samples: its first, how many) and how it's its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Span {
    pub(crate) at: usize,
    pub(crate) n: usize,
    pub(crate) vary: Vary,
}

/// Each sweep as samples of a track `len` long: from where its first frame is shown, as long as its frames last.
pub(crate) fn spans(sweeps: &[Sweep], len: usize) -> Vec<Span> {
    let at = |k: u64| super::sound::samples(Program::time_of(k)).min(len);
    (sweeps.iter())
        .map(|s| Span { at: at(s.at), n: at(s.at + s.frames) - at(s.at), vary: Vary::of(s) })
        .filter(|s| s.n > 0)
        .collect()
}

/// A raised cosine from 0 (x ≤ 0) to 1 (x ≥ 1).
fn rc(x: f64) -> f64 {
    0.5 - 0.5 * (PI * x.clamp(0.0, 1.0)).cos()
}

/// How fast the brush crosses at `t` of its `len` s, 0 to 1: still as it lands and as it lifts, fastest mid-sweep.
fn speed(t: f64, len: f64) -> f64 {
    (PI * (t / len).clamp(0.0, 1.0)).sin().powf(0.6)
}

/// How hard it presses at `t`, 0 to 1: landing over its first 12 % (0.14 s at most), easing off by a third as it
/// crosses, lifted over its last 20 % (0.24 s at most) — the landing and the lift each by `v`'s factor. 0 on its first
/// sample and its last.
fn press(t: f64, len: f64, v: &Vary) -> f64 {
    let lift = (0.2 * len).min(0.24) * v.away;
    rc(t / land(len, v)) * rc((len - t) / lift) * (1.0 - 0.35 * t / len)
}

/// How long the press takes to land on a stroke `len` s long (s): its first 12 %, 0.14 s at most, `v.into` times.
fn land(len: f64, v: &Vary) -> f64 {
    (0.12 * len).min(0.14) * v.into
}

/// A fourth-order Butterworth low-pass: two sections.
fn low_pass(f: f64) -> [Biquad; 2] {
    [Biquad::new(Shape::LowPass, f, 0.541, 0.0), Biquad::new(Shape::LowPass, f, 1.307, 0.0)]
}

/// `x` through each section in turn.
fn through(sections: &mut [Biquad], x: f64) -> f64 {
    sections.iter_mut().fold(x, |x, b| b.run(x))
}

fn rms(y: &[f64]) -> f64 {
    (y.iter().map(|v| v * v).sum::<f64>() / y.len().max(1) as f64).sqrt()
}

/// `y` brought to an RMS of 1 (silence as it is).
fn normal(mut y: Vec<f64>) -> Vec<f64> {
    let r = rms(&y);
    if r > 0.0 {
        y.iter_mut().for_each(|v| *v /= r);
    }
    y
}

/// The paper's fibres a second the brush crosses at full speed (× `v.grain`): the rub's grain, at the brush's pace.
const FIBRES: f64 = 160.0;

/// Where the brush is along the paper at each of `n` samples, in fibres: `FIBRES` a second (× `v.grain`) at full speed,
/// at the brush's pace — the rub reads its grain there.
pub(crate) fn along(n: usize, v: &Vary) -> Vec<f64> {
    let (len, mut x) = (n as f64 / R, 0.0);
    (0..n)
        .map(|i| {
            x += speed(i as f64 / R, len) * FIBRES * v.grain / R;
            x
        })
        .collect()
}

/// The rub's band: high-passed, low-passed (Hz) — a low "shh", not a hiss.
const RUB_BAND: (f64, f64) = (280.0, 1_500.0);

/// The paper rubbed: noise in `RUB_BAND` through a band at 700 Hz and a softer one at 1.25 kHz, its level the paper's
/// grain under the brush — the fibres laid along its path, a bump each, most low and a few high, and finer ones between
/// (3.3 a fibre), read at the brush's pace: slow as it lands, fastest mid-sweep, slow as it lifts. A rough texture that
/// follows the hand, not a flutter of its own. Nothing swells under it as it lands: it eases in with the press alone
/// (the design session's brief on the user's "네 제안대로 가자", 2026-11-17 — the old swell, 45–200 Hz up over 25 ms,
/// stood a low column of its own in each sweep's first 0.15 s). An RMS of 1.
fn rubbed(n: usize, v: &Vary) -> Vec<f64> {
    let len = n as f64 / R;
    let (mut noise, mut tex) = (Rng::new(v.seed, 11), Rng::new(v.seed, 15));
    let x = along(n, v);
    let fibres = (FIBRES * v.grain * len) as usize + 3;
    let mut bumps = |k: usize| -> Vec<f64> { (0..k).map(|_| tex.gauss().max(0.0).powf(1.5)).collect() };
    let (coarse, fine) = (bumps(fibres), bumps((fibres as f64 * 3.3) as usize + 3));
    let mean = coarse.iter().chain(&fine).sum::<f64>() / (coarse.len() + fine.len()) as f64;
    let at = |t: &[f64], x: f64| {
        let k = (x.floor() as usize).min(t.len() - 2);
        let f = rc(x - k as f64);
        t[k] * (1.0 - f) + t[k + 1] * f
    };
    let (mut hp, mut lp) = (high_pass(RUB_BAND.0 * v.pitch), low_pass(RUB_BAND.1 * v.pitch));
    let mut low = Biquad::new(Shape::BandPass, 700.0 * v.pitch, 0.7, 0.0);
    let mut high = Biquad::new(Shape::BandPass, 1_250.0 * v.pitch, 1.1, 0.0);
    let y: Vec<f64> = (0..n)
        .map(|i| {
            let t = i as f64 / R;
            let (s, x) = (speed(t, len), x[i]);
            let grain = (0.2 + v.depth * (0.55 * at(&coarse, x) + 0.25 * at(&fine, 3.3 * x)) / mean).max(0.0);
            let y = through(&mut lp, through(&mut hp, noise.gauss()));
            (low.run(y) + 0.5 * high.run(y)) * grain * press(t, len, v) * (0.35 + 0.65 * s)
        })
        .collect();
    normal(y)
}

/// One stroke, `n` samples long, its own as `v` says, at a level of its own (up to 1.5 dB under the others'), before
/// the track's: already landing and lifting with the press, only its first and last 3 ms tapered (0 on its first sample
/// and its last).
pub(crate) fn stroke(n: usize, v: &Vary) -> Vec<f32> {
    let y = rubbed(n, v);
    let level = 0.1 * 10f64.powf(v.level_db / 20.0);
    let last = n.saturating_sub(1) as f64 / R;
    let taper = |t: f64| rc(t / 0.003) * rc((last - t) / 0.003);
    (y.iter().enumerate()).map(|(i, x)| (x * level * taper(i as f64 / R)) as f32).collect()
}

/// The brush's track, `len` samples: a stroke on each span (`spans`), nothing between, before its level.
pub(crate) fn track(spans: &[Span], len: usize) -> Vec<f32> {
    let mut y = vec![0.0f32; len];
    for s in spans {
        let x = stroke(s.n, &s.vary);
        y.iter_mut().skip(s.at).zip(x).for_each(|(o, v)| *o += v);
    }
    y
}

/// The brush as laid: how many sweeps and how long they last in all (s), its loudness over them (LUFS in the centre)
/// and its true peak (dBTP), both before the whole's gain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Brushed {
    pub(crate) sweeps: usize,
    pub(crate) secs: f64,
    pub(crate) lufs: f32,
    pub(crate) peak: f32,
}

/// The brush onto `keys` (tarae's own sounds as mastered, the centre's): a stroke on each sweep (its first frame, how
/// many: `Painted::sweeps`), all of them by one gain, their loudness over the sweeps `UNDER_KEYS_LU` under the keys' own
/// (`keys_lufs`, in the centre). None with no sweep — `keys` as they were, to the bit.
pub(crate) fn lay(keys: &mut [f32], sweeps: &[Sweep], keys_lufs: f32) -> Option<Brushed> {
    let spans = spans(sweeps, keys.len());
    if spans.is_empty() {
        return None;
    }
    let y = track(&spans, keys.len());
    let heard: Vec<f32> = spans.iter().flat_map(|s| y[s.at..s.at + s.n].iter().copied()).collect();
    let own = super::sound::centred(sound::lufs(&heard));
    if !own.is_finite() {
        return None;
    }
    let lufs = keys_lufs - UNDER_KEYS_LU;
    let g = 10f32.powf((lufs - own) / 20.0);
    keys.iter_mut().zip(&y).for_each(|(k, b)| *k += b * g);
    let peak = sound::true_peaks(&heard).into_iter().fold(0.0f32, f32::max) * g;
    let n: usize = spans.iter().map(|s| s.n).sum();
    Some(Brushed { sweeps: spans.len(), secs: n as f64 / R, lufs, peak: 20.0 * peak.log10() })
}
