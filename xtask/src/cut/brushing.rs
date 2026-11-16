//! The brush on the tablet (the ztunnel video's edit, Z3, after §6.2's painting): one soft stroke a sweep of the
//! character's — `draw-1` to `-3`, across the tablet in about a second (`motions::Painted::sweeps`) —, made from noise
//! as the rain is (`rain::Rng`, `rain::Biquad`), never downloaded. It starts on the sweep's first frame and lasts as
//! long as the brush crosses, eased in and out — no attack: the rain's drops first read as key taps —; silent while the
//! brush is lifted and at rest. It follows the character's sweep, never the board's strokes: a dab or a short stroke
//! makes no sound of its own. Calm, not swishy, as the picture is.
//!
//! Which kind, the edit list's `[sound] brush` (none until the user's ears pick, as the rain's drops were picked):
//!
//! - **`dry`**, a dry brush on hanji: noise through a broad band at 1.6 kHz with a brighter one at 4 kHz that comes up
//!   as the brush presses mid-sweep, its level fluttering with the paper's grain (90 Hz) and soft catches on its fibres
//!   (about 28 a second, each a 4–9 ms swell — never a click): a papery hiss with grain in it, about half its energy
//!   over 3 kHz.
//! - **`damp`**, a damp brush: a round band at 750 Hz and a softer one at 1.6 kHz, little over 2.6 kHz (under a tenth of
//!   its energy over 3 kHz), a slow grain (30 Hz) and a few faint catches: softer and rounder, little hiss.
//! - **`ink`**, a brush loaded with ink that dries as it crosses: `damp` at its first frame turning into `dry` by its
//!   last.
//!
//! Not one loop: a sweep is 38 frames, and the same stroke every sweep would be a metronome over a long picture. Each
//! sweep's stroke is a little its own (`Vary`): its pitch (its bands 2–5 % up or down, one sweep up, the next down),
//! its grain's rate and depth, its loudness (within 1.5 dB) and its ease in and out — drawn from its picture, its step
//! and its place in the step, so a render is the same twice. Only so that no two sweeps in a row sound alike: the
//! stroke is still one a sweep, as long as the sweep, whatever the board draws under it.
//!
//! Laid in the centre with tarae's own sounds once they're mastered (`sound::make`): the beds lie under it as under the
//! keys, the whole's gain and limiter over it all; its level `UNDER_KEYS_LU` under the keys' own loudness, measured over
//! its sweeps alone.

use super::Program;
use super::mascot::motions::Sweep;
use crate::rain::{Biquad, Rng, Shape, high_pass};
use crate::sound::{self, RATE};

/// The brush's loudness over its sweeps, this far under the keys' own (LU, each alone, in the centre): under the taps,
/// a little over the rain at its gain 6 (take 6's: the keys −19.2 LUFS, the rain −34.0 as laid).
pub(crate) const UNDER_KEYS_LU: f32 = 12.0;

const R: f64 = RATE as f64;

/// A kind of brush (`[sound] brush`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Dry,
    Damp,
    Ink,
}

impl Kind {
    pub(crate) const ALL: [Kind; 3] = [Kind::Dry, Kind::Damp, Kind::Ink];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Kind::Dry => "dry",
            Kind::Damp => "damp",
            Kind::Ink => "ink",
        }
    }

    /// `[sound] brush`'s word: a kind, or `none` (Some(None)); else None.
    pub(crate) fn parse(word: &str) -> Option<Option<Kind>> {
        match word {
            "none" => Some(None),
            w => Kind::ALL.into_iter().find(|k| k.name() == w).map(Some),
        }
    }
}

/// A voice of the brush: noise high-passed at `low`, through `bands` (each a band-pass: its centre, Q, and its weight
/// with the brush light and pressed), low-passed at `high`; its level fluttering with the paper's grain (`grain`: the
/// flutter's rate — the low-pass on its noise, Hz — and depth) and its fibres' catches (`catches`: a second, height).
struct Recipe {
    low: f64,
    high: f64,
    bands: &'static [(f64, f64, f64, f64)],
    grain: (f64, f64),
    catches: (f64, f64),
}

const DRY: Recipe = Recipe {
    low: 500.0,
    high: 7_000.0,
    bands: &[(1_600.0, 0.6, 0.9, 0.9), (4_000.0, 0.9, 0.08, 0.3)],
    grain: (90.0, 0.5),
    catches: (28.0, 0.6),
};

const DAMP: Recipe = Recipe {
    low: 180.0,
    high: 2_600.0,
    bands: &[(750.0, 0.7, 0.9, 0.9), (1_600.0, 0.8, 0.25, 0.45), (3_400.0, 1.0, 0.02, 0.05)],
    grain: (30.0, 0.25),
    catches: (8.0, 0.25),
};

/// How a sweep's stroke is its own: its streams' seed, its bands' pitch (a factor), its grain's rate and depth
/// (factors), its loudness (dB) and its ease in and out (factors on their lengths).
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

/// A stroke's ease over its `len` s, at `t`: in over a quarter of it (0.22 s at most), out over the last 35 % (0.35 s at
/// most), each by `v`'s factor, raised cosines — 0 on its first sample and its last.
fn eased(t: f64, len: f64, v: &Vary) -> f64 {
    let (into, away) = ((len * 0.25).min(0.22) * v.into, (len * 0.35).min(0.35) * v.away);
    let rc = |x: f64| 0.5 - 0.5 * (std::f64::consts::PI * x.clamp(0.0, 1.0)).cos();
    rc(t / into) * rc((len - t) / away)
}

/// The brush's pressure across a sweep, 0 to 1: it presses into the middle and eases off.
fn pressure(t: f64, len: f64) -> f64 {
    (std::f64::consts::PI * t / len).sin()
}

/// The paper under the brush, `n` samples: 1 and a flutter about it (a low-passed noise, `grain`), with a soft swell
/// at each fibre the hairs catch on (`catches`, Poisson: each a raised cosine 4–9 ms wide) — never under 0.
fn paper(n: usize, (rate, depth): (f64, f64), (often, height): (f64, f64), rng: &mut Rng) -> Vec<f64> {
    let a = (-std::f64::consts::TAU * rate / R).exp();
    // (a one-pole low-pass's spread for unit noise in)
    let spread = ((1.0 - a) / (1.0 + a)).sqrt();
    let mut y = 0.0;
    let mut m: Vec<f64> = (0..n)
        .map(|_| {
            y = (1.0 - a) * rng.gauss() + a * y;
            1.0 + depth * y / spread
        })
        .collect();
    if often > 0.0 {
        let mut t = 0.0;
        loop {
            t += -(1.0 - rng.f()).ln() / often;
            let at = (t * R) as usize;
            if at >= n {
                break;
            }
            let w = (rng.range(0.004, 0.009) * R) as usize;
            let h = height * rng.range(0.4, 1.0);
            for (i, v) in m.iter_mut().skip(at).take(w).enumerate() {
                *v += h * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / w as f64).cos());
            }
        }
    }
    m.iter_mut().for_each(|v| *v = v.max(0.0));
    m
}

/// One voice (`r`), `n` samples, its streams from `seed`, its pitch and grain as `v` says: brought to an RMS of 1, not
/// eased.
fn voice(r: &Recipe, n: usize, seed: u64, v: &Vary) -> Vec<f64> {
    let mut noise = Rng::new(seed, 1);
    let grain = (r.grain.0 * v.grain, r.grain.1 * v.depth);
    let m = paper(n, grain, r.catches, &mut Rng::new(seed, 2));
    let mut low = high_pass(r.low * v.pitch);
    let mut high = Biquad::new(Shape::LowPass, r.high * v.pitch, 0.707, 0.0);
    let mut bands: Vec<Biquad> =
        r.bands.iter().map(|b| Biquad::new(Shape::BandPass, b.0 * v.pitch, b.1, 0.0)).collect();
    let len = n as f64 / R;
    let y: Vec<f64> = (0..n)
        .map(|i| {
            let p = pressure(i as f64 / R, len);
            let x = low.iter_mut().fold(noise.gauss(), |x, b| b.run(x));
            let banded: f64 =
                (bands.iter_mut().zip(r.bands)).map(|(b, w)| b.run(x) * (w.2 + (w.3 - w.2) * p)).sum();
            high.run(banded) * m[i] * (0.7 + 0.3 * p)
        })
        .collect();
    let rms = (y.iter().map(|v| v * v).sum::<f64>() / n.max(1) as f64).sqrt();
    match rms > 0.0 {
        true => y.iter().map(|v| v / rms).collect(),
        false => y,
    }
}

/// One stroke of `kind`, `n` samples long, its own as `v` says: its voice eased in and out, at a level of its own (up
/// to 1.5 dB under the others'), before the track's.
pub(crate) fn stroke(kind: Kind, n: usize, v: &Vary) -> Vec<f32> {
    let seed = v.seed;
    let len = n as f64 / R;
    let y = match kind {
        Kind::Dry => voice(&DRY, n, seed, v),
        Kind::Damp => voice(&DAMP, n, seed, v),
        Kind::Ink => {
            let (wet, dry) = (voice(&DAMP, n, seed, v), voice(&DRY, n, seed ^ (1 << 32), v));
            (wet.iter().zip(&dry).enumerate())
                .map(|(i, (w, d))| {
                    let k = (i as f64 / n.max(1) as f64).powf(1.3);
                    w * (1.0 - k) + d * k
                })
                .collect()
        }
    };
    let level = 0.1 * 10f64.powf(v.level_db / 20.0);
    (y.iter().enumerate()).map(|(i, x)| (x * level * eased(i as f64 / R, len, v)) as f32).collect()
}

/// The brush's track, `len` samples: a stroke of `kind` on each span (`spans`), nothing between, before its level.
pub(crate) fn track(kind: Kind, spans: &[Span], len: usize) -> Vec<f32> {
    let mut y = vec![0.0f32; len];
    for s in spans {
        let x = stroke(kind, s.n, &s.vary);
        y.iter_mut().skip(s.at).zip(x).for_each(|(o, v)| *o += v);
    }
    y
}

/// The brush as laid: its kind, how many sweeps and how long they last in all (s), its loudness over them (LUFS in the
/// centre) and its true peak (dBTP), both before the whole's gain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Brushed {
    pub(crate) kind: Kind,
    pub(crate) sweeps: usize,
    pub(crate) secs: f64,
    pub(crate) lufs: f32,
    pub(crate) peak: f32,
}

/// The brush of `kind` onto `keys` (tarae's own sounds as mastered, the centre's): a stroke on each sweep (its first
/// frame, how many: `Painted::sweeps`), all of them by one gain, their loudness over the sweeps `UNDER_KEYS_LU` under
/// the keys' own (`keys_lufs`, in the centre). None with no kind or no sweep — `keys` as they were, to the bit.
pub(crate) fn lay(keys: &mut [f32], kind: Option<Kind>, sweeps: &[Sweep], keys_lufs: f32) -> Option<Brushed> {
    let kind = kind?;
    let spans = spans(sweeps, keys.len());
    if spans.is_empty() {
        return None;
    }
    let y = track(kind, &spans, keys.len());
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
    Some(Brushed { kind, sweeps: spans.len(), secs: n as f64 / R, lufs, peak: 20.0 * peak.log10() })
}
