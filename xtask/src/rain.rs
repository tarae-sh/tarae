//! `cargo xtask rain OUT.wav --len 12m [--kind soft|leaves|heavy] [--seed N]` — the full video's bed: rain, made from
//! noise as the brush is (`.sb/full-video-rain-brief.md`; showcase's "The rain is made, not found", 2026-11-11).
//! Stereo, 48 kHz, 16 bits, as long as asked — no loop, nothing to seam —, written a block at a time, never held
//! whole; the same seed, the same bytes. Three layers, each made apart for the left and the right:
//!
//! - **The wash**, the far rain: pink noise between `Measure::wash_low` and 9 kHz, easing above 2.5 kHz, its level a
//!   slow random walk (`Walk`) the same on both sides — a decibel or two over ten to twenty-five seconds, no period.
//! - **The patter**, in two sizes of drop as rain has, each arriving at random (Poisson) times on each side and
//!   swelling with the wash (`.sb/full-video-rain-calls.md`: one size made ticks, crackle a few dB under a key):
//!   - **fine drops**, `Measure::fine_rate` a second (thousands), quiet and short (`FINE_ATTACK_S`, `FINE_DECAY_S`),
//!     each in one of `FINE_BANDS`: together the top's dense body, a hiss with grain in it;
//!   - **clear drops**, `Measure::clear_rate` a second, each a pat and not a tick: 5–15 ms of band-passed noise at
//!     1.2–4.5 kHz eased in over 0.6–1.2 ms, its level spread (most soft, a few clear) but never more than 6 dB over
//!     the median one, heard on its side and, fainter and a fraction of a millisecond later, on the other.
//! - **The near drops**, on leaves: `Measure::near_rate` a second, each a damped tone at 150–600 Hz (20–60 ms) eased
//!   in, with a soft top of noise under 2 kHz (~5 ms), placed wide — dull, never a key (`a_near_drop_isnt_a_key`).
//!
//! Nothing under 120 Hz (a high-pass on the whole: a phone doesn't play it); no thunder, wind or birds (each an event
//! a viewer reads as meaning something). Brought to about `LOUDNESS` by its own first `CALIBRATE_S` seconds, a soft
//! knee keeping it under `CEILING` (the cut sets the level).
//!
//! `cargo xtask rain samples [DIR]` (`target/rain/`): each kind's `SAMPLE_S` seconds as WAV and as a viewer gets it
//! (AAC 128 kb/s in an MP4 with a still naming it, the picture as long as the sound), their spectrograms, and the
//! figures of both side by side (`rain.txt`). `cargo xtask rain figures FILE.wav …`: a file's figures.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use crate::Result;
use crate::sound::{self, RATE};

const USAGE: &str = "usage: cargo xtask rain OUT.wav --len 12m [--kind soft|leaves|heavy] [--seed N] | rain samples \
                     [DIR] | rain figures FILE.wav …";

/// The file's own ceiling (−6 dBFS): 6 dB left for the cut, which sets the level.
const CEILING: f32 = 0.5;
/// Where the soft knee starts (−9 dBFS): under it every sample is as made.
const KNEE: f32 = 0.35;
/// Each kind's loudness before the cut levels it (LUFS, the two sides' power summed as BS.1770 does).
const LOUDNESS: f32 = -27.0;
/// The level is found on the file's own first seconds, as made.
const CALIBRATE_S: u32 = 8;
/// Made a block at a time, a second each.
const BLOCK: usize = RATE as usize;
/// Made and dropped first, so its filters and its walk have settled when the file starts.
const PREROLL: usize = RATE as usize / 2;
/// What a drop leaves past its block: room for the longest (a near drop's 60 ms with its far side's delay).
/// (A fine drop leaves nothing: its envelope runs in the sample loop.)
const RING: usize = BLOCK + RATE as usize / 4;
/// The walk steps every 10 ms, its gain eased between.
const CONTROL: u32 = RATE / 100;
/// The samples the user hears: each kind alone this long.
const SAMPLE_S: u32 = 25;
const R: f64 = RATE as f64;

pub fn run(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("samples") => {
            samples(&args.get(1).map_or_else(|| crate::root().join("target/rain"), PathBuf::from))
        }
        Some("figures") if args.len() > 1 => {
            for f in &args[1..] {
                let (l, r) = read_wav(Path::new(f))?;
                println!("{f}\n{}", table(&[("", &figures(&l, &r))]));
            }
            Ok(())
        }
        Some(out) if !out.starts_with('-') => make(out, &args[1..]),
        _ => Err(USAGE.into()),
    }
}

fn make(out: &str, args: &[String]) -> Result<()> {
    let (mut frames, mut kind, mut seed) = (None, Kind::Leaves, 1u64);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("rain: {a} needs a value\n{USAGE}"));
        match a.as_str() {
            "--len" => frames = Some(frames_of(value()?)?),
            "--kind" => kind = Kind::parse(value()?)?,
            "--seed" => seed = value()?.parse().map_err(|_| format!("rain: --seed is a number\n{USAGE}"))?,
            _ => return Err(format!("rain: {a}?\n{USAGE}")),
        }
    }
    let frames = frames.ok_or_else(|| format!("rain: --len says how long (12m, 25s, 1m30s)\n{USAGE}"))?;
    let started = Instant::now();
    let file = std::fs::File::create(out).map_err(|e| format!("{out}: {e}"))?;
    let mut w = std::io::BufWriter::with_capacity(1 << 20, file);
    let made = write(&mut w, kind, seed, frames)?;
    w.flush().map_err(|e| format!("{out}: {e}"))?;
    println!(
        "{out}: {}, seed {seed}, {:.1} s ({frames} frames, {:.1} MB) · gain {:+.1} dB · {} samples past the knee · \
         peak {:.1} dBFS · made in {:.1} s",
        kind.name(),
        frames as f64 / R,
        (44 + frames * 4) as f64 / 1e6,
        made.gain_db,
        made.kneed,
        20.0 * made.peak.log10(),
        started.elapsed().as_secs_f64(),
    );
    Ok(())
}

/// `12m`, `25s`, `1m30s`, `500ms`, `1h` → frames at `RATE`.
fn frames_of(s: &str) -> Result<u64> {
    let bad = || format!("rain: {s:?} isn't a length (12m, 25s, 1m30s, 500ms)");
    let (mut ms, mut rest) = (0u64, s);
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !c.is_ascii_digit()).ok_or_else(bad)?;
        let n: u64 = rest[..digits].parse().map_err(|_| bad())?;
        let after = &rest[digits..];
        let (unit, len) = match after.as_bytes()[0] {
            b'm' if after.starts_with("ms") => (1, 2),
            b'h' => (3_600_000, 1),
            b'm' => (60_000, 1),
            b's' => (1_000, 1),
            _ => return Err(bad()),
        };
        ms += n * unit;
        rest = &after[len..];
    }
    match ms {
        0 => Err(bad()),
        ms => Ok(ms * u64::from(RATE) / 1_000),
    }
}

// ── The kinds ────────────────────────────────────────────────────────────────────────────────────────────────

/// The same layers at different measures, for the user's ears.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// Wash-led, a fine steady rain.
    Soft,
    /// More patter, and the near drops: "비오는 자연의 소리".
    Leaves,
    /// Denser and fuller in the low-mids: a downpour.
    Heavy,
}

const KINDS: [Kind; 3] = [Kind::Soft, Kind::Leaves, Kind::Heavy];

/// A kind's measures. The levels are each layer's own, before the whole is brought to `LOUDNESS`.
struct Measure {
    wash: f64,
    /// Fine drops a second, on each side.
    fine_rate: f64,
    fine: f64,
    /// The wash's high-pass corner (Hz): lower is fuller.
    wash_low: f64,
    /// The low-mids round 400 Hz (dB).
    low_mids: f64,
    /// Above 2.5 kHz (dB): how far the wash eases.
    top: f64,
    /// Clear drops a second, on each side.
    clear_rate: f64,
    clear: f64,
    /// Near drops a second, both sides together.
    near_rate: f64,
    near: f64,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Soft => "soft",
            Kind::Leaves => "leaves",
            Kind::Heavy => "heavy",
        }
    }

    fn parse(s: &str) -> Result<Kind> {
        KINDS
            .into_iter()
            .find(|k| k.name() == s)
            .ok_or_else(|| format!("rain: no kind {s:?} (soft, leaves, heavy)"))
    }

    fn measure(self) -> Measure {
        match self {
            Kind::Soft => Measure {
                wash: 1.0,
                wash_low: 300.0,
                low_mids: 0.0,
                top: -5.0,
                fine_rate: 9_000.0,
                fine: 0.06,
                clear_rate: 160.0,
                clear: 0.54,
                near_rate: 0.0,
                near: 0.0,
            },
            Kind::Leaves => Measure {
                wash: 0.7,
                wash_low: 280.0,
                low_mids: 1.0,
                top: -5.0,
                fine_rate: 10_500.0,
                fine: 0.057,
                clear_rate: 260.0,
                clear: 0.58,
                near_rate: 2.5,
                near: 2.2,
            },
            Kind::Heavy => Measure {
                wash: 1.0,
                wash_low: 170.0,
                low_mids: 4.0,
                top: -3.0,
                fine_rate: 13_500.0,
                fine: 0.07,
                clear_rate: 420.0,
                clear: 0.83,
                near_rate: 3.0,
                near: 2.5,
            },
        }
    }
}

/// Which layers sound: all, but in the tests and the figures of each alone.
#[derive(Clone, Copy)]
struct Layers {
    wash: bool,
    fine: bool,
    clear: bool,
    near: bool,
}

const ALL: Layers = Layers { wash: true, fine: true, clear: true, near: true };
const NONE: Layers = Layers { wash: false, fine: false, clear: false, near: false };

// ── Its own small random source and filters ──────────────────────────────────────────────────────────────────

/// SplitMix64: one stream a use (`stream`), so a layer alone is the same as it is in the whole.
#[derive(Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64, stream: u64) -> Rng {
        Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03) ^ stream.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In [0, 1).
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.f()
    }

    /// Near-Gaussian, unit variance: four uniforms summed (a filter takes the rest of the way).
    fn gauss(&mut self) -> f64 {
        (self.f() + self.f() + self.f() + self.f() - 2.0) * 3f64.sqrt()
    }
}

#[derive(Clone, Copy)]
enum Shape {
    LowPass,
    HighPass,
    /// Its peak at 0 dB.
    BandPass,
    Peak,
    HighShelf,
}

/// A second-order section (the Audio EQ Cookbook's), transposed direct form II.
#[derive(Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    fn new(shape: Shape, f: f64, q: f64, db: f64) -> Biquad {
        let w = std::f64::consts::TAU * f / R;
        let (cw, alpha, a) = (w.cos(), w.sin() / (2.0 * q), 10f64.powf(db / 40.0));
        let [b0, b1, b2, a0, a1, a2] = match shape {
            Shape::LowPass => {
                [(1.0 - cw) / 2.0, 1.0 - cw, (1.0 - cw) / 2.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha]
            }
            Shape::HighPass => {
                [(1.0 + cw) / 2.0, -(1.0 + cw), (1.0 + cw) / 2.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha]
            }
            Shape::BandPass => [alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cw, 1.0 - alpha],
            Shape::Peak => {
                [1.0 + alpha * a, -2.0 * cw, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cw, 1.0 - alpha / a]
            }
            Shape::HighShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                [
                    a * ((a + 1.0) + (a - 1.0) * cw + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cw),
                    a * ((a + 1.0) + (a - 1.0) * cw - s),
                    (a + 1.0) - (a - 1.0) * cw + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cw),
                    (a + 1.0) - (a - 1.0) * cw - s,
                ]
            }
        };
        Biquad { b: [b0 / a0, b1 / a0, b2 / a0], a: [a1 / a0, a2 / a0], z: [0.0; 2] }
    }

    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// `x` through each section in turn.
fn through(sections: &mut [Biquad], mut x: f64) -> f64 {
    for b in sections {
        x = b.run(x);
    }
    x
}

/// A fourth-order Butterworth high-pass: two sections.
fn high_pass(f: f64) -> [Biquad; 2] {
    [Biquad::new(Shape::HighPass, f, 0.541, 0.0), Biquad::new(Shape::HighPass, f, 1.307, 0.0)]
}

// ── The layers ───────────────────────────────────────────────────────────────────────────────────────────────

/// The wash of one side: white noise made pink (Paul Kellet's refined filter), then shaped.
struct Wash {
    noise: Rng,
    pink: [f64; 7],
    shape: [Biquad; 5],
}

impl Wash {
    fn new(m: &Measure, noise: Rng) -> Wash {
        let [h0, h1] = high_pass(m.wash_low);
        let shape = [
            h0,
            h1,
            Biquad::new(Shape::Peak, 400.0, 0.7, m.low_mids),
            Biquad::new(Shape::HighShelf, 2_500.0, 0.6, m.top),
            Biquad::new(Shape::LowPass, 9_000.0, 0.707, 0.0),
        ];
        Wash { noise, pink: [0.0; 7], shape }
    }

    fn next(&mut self) -> f64 {
        let w = self.noise.gauss();
        let p = &mut self.pink;
        p[0] = 0.99886 * p[0] + w * 0.055_517_9;
        p[1] = 0.99332 * p[1] + w * 0.075_075_9;
        p[2] = 0.96900 * p[2] + w * 0.153_852;
        p[3] = 0.86650 * p[3] + w * 0.310_485_6;
        p[4] = 0.55000 * p[4] + w * 0.532_952_2;
        p[5] = -0.7616 * p[5] - w * 0.016_898;
        let x = (p[0] + p[1] + p[2] + p[3] + p[4] + p[5] + p[6] + w * 0.5362) * 0.2;
        p[6] = w * 0.115_926;
        through(&mut self.shape, x)
    }
}

/// The wash's swell: an Ornstein–Uhlenbeck walk in dB (no period, it only drifts back), smoothed, held within
/// ±`MOST` dB, stepped every `CONTROL` samples and eased between.
struct Walk {
    rng: Rng,
    x: f64,
    y: f64,
    from: f64,
    to: f64,
    step: u32,
}

impl Walk {
    /// Its spread (dB) and how long it takes to drift back (s).
    const SPREAD: f64 = 0.9;
    const BACK_S: f64 = 15.0;
    /// Its smoothing (s), and the most it swells or eases (dB).
    const SMOOTH_S: f64 = 2.0;
    const MOST: f64 = 1.5;

    fn new(mut rng: Rng) -> Walk {
        let x = Walk::SPREAD * rng.gauss();
        let g = Walk::gain_of(x);
        Walk { rng, x, y: x, from: g, to: g, step: 0 }
    }

    fn gain_of(y: f64) -> f64 {
        10f64.powf(Walk::MOST * (y / Walk::MOST).tanh() / 20.0)
    }

    fn gain(&mut self) -> f64 {
        if self.step == CONTROL {
            let dt = f64::from(CONTROL) / R;
            self.x += -self.x * dt / Walk::BACK_S
                + Walk::SPREAD * (2.0 * dt / Walk::BACK_S).sqrt() * self.rng.gauss();
            self.y += (self.x - self.y) * dt / Walk::SMOOTH_S;
            (self.from, self.to, self.step) = (self.to, Walk::gain_of(self.y), 0);
        }
        let g = self.from + (self.to - self.from) * f64::from(self.step) / f64::from(CONTROL);
        self.step += 1;
        g
    }
}

/// Arrivals at random: a Poisson process, `rate` a second, at sample times.
struct Arrivals {
    when: Rng,
    rate: f64,
    next: f64,
}

impl Arrivals {
    fn new(when: Rng, rate: f64) -> Arrivals {
        let mut a = Arrivals { when, rate, next: 0.0 };
        a.next = a.interval();
        a
    }

    fn interval(&mut self) -> f64 {
        match self.rate > 0.0 {
            true => -(1.0 - self.when.f()).ln() * R / self.rate,
            false => f64::INFINITY,
        }
    }

    /// The next arrival before `until` (a sample), taken.
    fn take(&mut self, until: u64) -> Option<u64> {
        if self.next >= until as f64 {
            return None;
        }
        let at = self.next.round() as u64;
        self.next += self.interval();
        Some(at)
    }
}

/// A sample at `at` in the ring of what drops leave.
fn ring_at(at: u64) -> usize {
    (at % RING as u64) as usize
}

/// The most a clear drop stands over the median one (a factor: 6 dB).
const CLEAR_MOST: f64 = 2.0;

/// One clear drop from `side` at `start`, into the ring: a pat, not a tick.
fn clear_drop(what: &mut Rng, ring: &mut [Vec<f64>; 2], side: usize, start: u64, level: f64) {
    let len = what.range(0.005, 0.015) * R;
    // (under the fine drops' top: a clear drop at 7 kHz stood 22 dB over the top's RMS — a tick)
    let f = 1_200.0 * (4_500.0f64 / 1_200.0).powf(what.f());
    let q = what.range(1.5, 3.0);
    // (most soft, a few clear — lognormal, a 4 dB spread — and none more than `CLEAR_MOST` over the median: an uncut
    // tail is crackle, the top carried by a few loud grains over almost nothing)
    let a = level * (0.6 * what.gauss()).exp().min(CLEAR_MOST);
    let far = what.range(0.0, 0.6);
    let late = (what.range(0.0, 0.0007) * R).round() as u64;
    let attack = what.range(0.0006, 0.0012) * R;
    // (e⁻³ at its end: its energy spread over the pat, not all in its first millisecond)
    let tau = len / 3.0;
    let mut bp = Biquad::new(Shape::BandPass, f, q, 0.0);
    // (its envelope's length, and 2 ms for the filter to ring out)
    for k in 0..(len + 0.002 * R) as u64 {
        let t = k as f64;
        let env = match t {
            t if t < attack => 0.5 - 0.5 * (std::f64::consts::PI * t / attack).cos(),
            t if t < len => (-(t - attack) / tau).exp(),
            _ => 0.0,
        };
        let n = if env > 0.0 { what.gauss() * env } else { 0.0 };
        let x = bp.run(n) * a;
        ring[side][ring_at(start + k)] += x;
        ring[1 - side][ring_at(start + k + late)] += x * far;
    }
}

/// The fine drops' bands (Hz): each drop in one, at random.
const FINE_BANDS: [f64; 4] = [2_500.0, 3_600.0, 5_000.0, 7_000.0];
/// A fine drop's envelope: a rise and a fall, each a time constant (s).
const FINE_ATTACK_S: f64 = 0.000_1;
const FINE_DECAY_S: f64 = 0.000_8;

/// The fine drops of one side: too many to make one by one, so each band keeps one envelope — a rise and a fall,
/// two one-pole states every drop in it adds to — over noise and through the band's filter. O(1) a sample at any
/// rate.
struct Fine {
    arrivals: Arrivals,
    /// Each drop's band and level.
    what: Rng,
    noise: Rng,
    /// Each band's fall and rise.
    env: [[f64; 2]; FINE_BANDS.len()],
    bp: [Biquad; FINE_BANDS.len()],
    /// The whole under 9 kHz, as the wash is.
    top: Biquad,
    /// Drops due (sample, band, level), from `next` on.
    due: Vec<(u64, usize, f64)>,
    next: usize,
    /// The fall's and the rise's factor a sample, and what brings the envelope's peak to 1.
    fall: f64,
    rise: f64,
    norm: f64,
}

impl Fine {
    fn new(arrivals: Arrivals, what: Rng, noise: Rng) -> Fine {
        let (td, ta) = (FINE_DECAY_S, FINE_ATTACK_S);
        let peak_t = (td / ta).ln() * ta * td / (td - ta);
        Fine {
            arrivals,
            what,
            noise,
            env: [[0.0; 2]; FINE_BANDS.len()],
            bp: FINE_BANDS.map(|f| Biquad::new(Shape::BandPass, f, 1.4, 0.0)),
            top: Biquad::new(Shape::LowPass, 9_000.0, 0.707, 0.0),
            due: Vec::new(),
            next: 0,
            fall: (-1.0 / (td * R)).exp(),
            rise: (-1.0 / (ta * R)).exp(),
            norm: 1.0 / ((-peak_t / td).exp() - (-peak_t / ta).exp()),
        }
    }

    /// The drops before `until`, made due.
    fn arrive(&mut self, until: u64) {
        self.due.drain(..self.next);
        self.next = 0;
        while let Some(at) = self.arrivals.take(until) {
            let band = ((self.what.f() * FINE_BANDS.len() as f64) as usize).min(FINE_BANDS.len() - 1);
            let a = (0.25 * self.what.gauss()).exp().min(CLEAR_MOST);
            self.due.push((at, band, a));
        }
    }

    /// The sample at `at` (the drops due by it added first).
    fn next(&mut self, at: u64) -> f64 {
        while let Some(&(_, band, a)) = self.due.get(self.next).filter(|d| d.0 <= at) {
            self.env[band][0] += a;
            self.env[band][1] += a;
            self.next += 1;
        }
        let mut x = 0.0;
        for (e, bp) in self.env.iter_mut().zip(&mut self.bp) {
            // (each band its own noise: one shared, their skirts added up in step and the peaks stood out)
            let n = (2.0 * self.noise.f() - 1.0) * 3f64.sqrt();
            e[0] *= self.fall;
            e[1] *= self.rise;
            x += bp.run(n * (e[0] - e[1]));
        }
        self.top.run(x * self.norm)
    }
}

/// One near drop at `start`, into the ring: a damped tone eased in, its pitch sagging a tenth as it dies, and a soft
/// top of noise under 2 kHz; heard on one side and, fainter and later, on the other.
fn near_drop(what: &mut Rng, ring: &mut [Vec<f64>; 2], start: u64, level: f64) {
    let side = usize::from(what.f() < 0.5);
    let f = 150.0 * 4f64.powf(what.f());
    let len = what.range(0.020, 0.060) * R;
    // (−40 dB at its end)
    let tau = len / 4.6;
    let attack = what.range(0.002, 0.004) * R;
    let a = level * what.range(0.4, 1.0);
    let far = what.range(0.15, 0.5);
    let late = (what.range(0.0002, 0.0007) * R).round() as u64;
    let (top_in, top_tau, top_len) = (0.001 * R, 0.0015 * R, 0.006 * R);
    let mut top = [Biquad::new(Shape::LowPass, 1_500.0, 0.707, 0.0); 2];
    let mut phase = 0.0f64;
    for k in 0..len as u64 {
        let t = k as f64;
        let ease = if t < attack { 0.5 - 0.5 * (std::f64::consts::PI * t / attack).cos() } else { 1.0 };
        phase += std::f64::consts::TAU * f * (1.0 - 0.1 * t / len) / R;
        let tone = phase.sin() * ease * (-t / tau).exp();
        let env = if t < top_len { (t / top_in).min(1.0) * (-t / top_tau).exp() } else { 0.0 };
        let n = if env > 0.0 { what.gauss() * env } else { 0.0 };
        let x = (tone + 1.5 * through(&mut top, n)) * a;
        ring[side][ring_at(start + k)] += x;
        ring[1 - side][ring_at(start + k + late)] += x * far;
    }
}

/// The rain as made, before its level: a block at a time.
struct Rain {
    m: Measure,
    on: Layers,
    /// The next sample (from the pre-roll's start).
    at: u64,
    wash: [Wash; 2],
    walk: Walk,
    fine: [Fine; 2],
    clear: [Arrivals; 2],
    clear_what: [Rng; 2],
    near: Arrivals,
    near_what: Rng,
    ring: [Vec<f64>; 2],
    floor: [[Biquad; 2]; 2],
}

impl Rain {
    fn new(kind: Kind, seed: u64, on: Layers) -> Rain {
        let m = kind.measure();
        let s = |stream| Rng::new(seed, stream);
        let mut rain = Rain {
            wash: [Wash::new(&m, s(1)), Wash::new(&m, s(2))],
            walk: Walk::new(s(3)),
            fine: [
                Fine::new(Arrivals::new(s(10), m.fine_rate), s(11), s(12)),
                Fine::new(Arrivals::new(s(13), m.fine_rate), s(14), s(15)),
            ],
            clear: [Arrivals::new(s(4), m.clear_rate), Arrivals::new(s(5), m.clear_rate)],
            clear_what: [s(6), s(7)],
            near: Arrivals::new(s(8), m.near_rate),
            near_what: s(9),
            ring: [vec![0.0; RING], vec![0.0; RING]],
            floor: [high_pass(120.0), high_pass(120.0)],
            m,
            on,
            at: 0,
        };
        let mut skip = vec![0.0f32; PREROLL];
        rain.block(&mut skip.clone(), &mut skip);
        rain
    }

    /// The next `l.len()` samples of each side (at most `BLOCK`).
    fn block(&mut self, l: &mut [f32], r: &mut [f32]) {
        let until = self.at + l.len() as u64;
        // (the drops swell with the wash — rain swells as a whole — each at the walk's gain as its block starts)
        let swell = self.walk.to;
        if self.on.clear {
            for side in 0..2 {
                while let Some(at) = self.clear[side].take(until) {
                    clear_drop(&mut self.clear_what[side], &mut self.ring, side, at, self.m.clear * swell);
                }
            }
        }
        if self.on.fine {
            for fine in &mut self.fine {
                fine.arrive(until);
            }
        }
        if self.on.near {
            while let Some(at) = self.near.take(until) {
                near_drop(&mut self.near_what, &mut self.ring, at, self.m.near);
            }
        }
        for (i, (l, r)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            let at = ring_at(self.at + i as u64);
            let swell = self.walk.gain();
            let mut out = [0.0f32; 2];
            for (side, out) in out.iter_mut().enumerate() {
                let mut x = std::mem::take(&mut self.ring[side][at]);
                if self.on.wash {
                    x += self.wash[side].next() * swell * self.m.wash;
                }
                if self.on.fine {
                    x += self.fine[side].next(self.at + i as u64) * swell * self.m.fine;
                }
                *out = through(&mut self.floor[side], x) as f32;
            }
            (*l, *r) = (out[0], out[1]);
        }
        self.at = until;
    }
}

/// The soft knee: as made under `KNEE`, bent toward `CEILING` over it, never reaching it.
fn limit(x: f32) -> f32 {
    let a = x.abs();
    match a <= KNEE {
        true => x,
        false => (KNEE + (CEILING - KNEE) * ((a - KNEE) / (CEILING - KNEE)).tanh()).copysign(x),
    }
}

/// The two sides' loudness together (LUFS): each side's mean square, as `sound::lufs` gates it, summed.
fn stereo_lufs(l: &[f32], r: &[f32]) -> f32 {
    let power = |lufs: f32| 10f64.powf((f64::from(lufs) + 0.691) / 10.0);
    (-0.691 + 10.0 * (power(sound::lufs(l)) + power(sound::lufs(r))).log10()) as f32
}

/// What making a rain came to.
struct Made {
    gain_db: f32,
    /// Samples (both sides) the knee bent.
    kneed: u64,
    peak: f32,
}

/// The gain that brings the whole of `kind` to `LOUDNESS`, from its own first `CALIBRATE_S` seconds.
fn gain(kind: Kind, seed: u64) -> f32 {
    let n = (CALIBRATE_S * RATE) as usize;
    let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
    let mut rain = Rain::new(kind, seed, ALL);
    for (l, r) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
        rain.block(l, r);
    }
    10f32.powf((LOUDNESS - stereo_lufs(&l, &r)) / 20.0)
}

/// `frames` of `kind` (its `on` layers at the whole's level), a block at a time to `each`.
fn made(
    kind: Kind,
    seed: u64,
    frames: u64,
    on: Layers,
    mut each: impl FnMut(&[f32], &[f32]) -> Result<()>,
) -> Result<Made> {
    let gain = gain(kind, seed);
    let mut rain = Rain::new(kind, seed, on);
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let (mut left, mut kneed, mut peak) = (frames, 0u64, 0.0f32);
    while left > 0 {
        let n = left.min(BLOCK as u64) as usize;
        rain.block(&mut l[..n], &mut r[..n]);
        for x in l[..n].iter_mut().chain(r[..n].iter_mut()) {
            let y = *x * gain;
            kneed += u64::from(y.abs() > KNEE);
            *x = limit(y);
            peak = peak.max(x.abs());
        }
        each(&l[..n], &r[..n])?;
        left -= n as u64;
    }
    Ok(Made { gain_db: 20.0 * gain.log10(), kneed, peak })
}

/// `frames` of `kind` held whole (the samples, the figures, the tests).
fn render(kind: Kind, seed: u64, frames: u64, on: Layers) -> Result<(Vec<f32>, Vec<f32>, Made)> {
    let (mut ls, mut rs) = (Vec::with_capacity(frames as usize), Vec::with_capacity(frames as usize));
    let made = made(kind, seed, frames, on, |l, r| {
        ls.extend_from_slice(l);
        rs.extend_from_slice(r);
        Ok(())
    })?;
    Ok((ls, rs, made))
}

/// A 16-bit sample.
fn pcm(x: f32) -> [u8; 2] {
    ((x * 32_767.0).round() as i16).to_le_bytes()
}

/// The WAV (stereo, 16-bit, `RATE`) of `frames` of `kind`, written a block at a time.
fn write(w: &mut impl Write, kind: Kind, seed: u64, frames: u64) -> Result<Made> {
    w.write_all(&head(frames)?).map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity(BLOCK * 4);
    made(kind, seed, frames, ALL, |l, r| {
        bytes.clear();
        for (l, r) in l.iter().zip(r) {
            bytes.extend_from_slice(&pcm(*l));
            bytes.extend_from_slice(&pcm(*r));
        }
        w.write_all(&bytes).map_err(|e| e.to_string())
    })
}

/// A stereo 16-bit WAV's 44 bytes of head, for `frames`.
fn head(frames: u64) -> Result<Vec<u8>> {
    let data =
        u32::try_from(frames * 4).ok().filter(|d| *d <= u32::MAX - 36).ok_or("rain: too long for a WAV")?;
    let mut head = Vec::with_capacity(44);
    head.extend_from_slice(b"RIFF");
    head.extend_from_slice(&(36 + data).to_le_bytes());
    head.extend_from_slice(b"WAVEfmt ");
    head.extend_from_slice(&16u32.to_le_bytes());
    head.extend_from_slice(&1u16.to_le_bytes());
    head.extend_from_slice(&2u16.to_le_bytes());
    head.extend_from_slice(&RATE.to_le_bytes());
    head.extend_from_slice(&(RATE * 4).to_le_bytes());
    head.extend_from_slice(&4u16.to_le_bytes());
    head.extend_from_slice(&16u16.to_le_bytes());
    head.extend_from_slice(b"data");
    head.extend_from_slice(&data.to_le_bytes());
    Ok(head)
}

/// A stereo 16-bit WAV's two sides.
fn read_wav(path: &Path) -> Result<(Vec<f32>, Vec<f32>)> {
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bad = |why: &str| format!("{}: {why}", path.display());
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(bad("not a WAV"));
    }
    let (mut at, mut stereo) = (12usize, false);
    while at + 8 <= b.len() {
        let len = u32::from_le_bytes([b[at + 4], b[at + 5], b[at + 6], b[at + 7]]) as usize;
        let body = &b[at + 8..(at + 8 + len).min(b.len())];
        match &b[at..at + 4] {
            b"fmt " => stereo = body.len() >= 16 && body[2..4] == [2, 0] && body[14..16] == [16, 0],
            b"data" if stereo => return Ok(sides(body)),
            b"data" => return Err(bad("not stereo 16-bit")),
            _ => {}
        }
        at += 8 + len + len % 2;
    }
    Err(bad("no data"))
}

/// Interleaved 16-bit stereo's two sides.
fn sides(b: &[u8]) -> (Vec<f32>, Vec<f32>) {
    let s = |i: usize| f32::from(i16::from_le_bytes([b[i], b[i + 1]])) / 32_768.0;
    (0..b.len() / 4).map(|f| (s(f * 4), s(f * 4 + 2))).unzip()
}

// ── The figures: how it's judged before anyone hears it ──────────────────────────────────────────────────────

const CENTRES: [f64; 9] = [63.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0];

struct Band {
    /// Its energy against the whole's (dB): the spectrum's bins from `centre / √2` to `centre · √2`.
    share_db: f64,
    /// E[x⁴] / E[x²]² of the band (two band-pass sections, Q 1): plain noise 3, grains over it.
    kurtosis: f64,
    /// The mono sum's power against the two sides' (dB): 0 when they're apart, under 0 where they cancel.
    mono_db: f64,
}

struct Figures {
    lufs: f32,
    peak_db: f32,
    /// Peak over RMS (dB) over 6 kHz and in the 3 kHz octave: plain noise's is ~14 over 25 s, a crackle's far over.
    crest_top_db: f64,
    crest_3k_db: f64,
    bands: Vec<Band>,
    corr: f64,
    mono_db: f64,
    /// What lies under 120 Hz against the whole (dB).
    under_120_db: f64,
    /// The quietest, median and loudest 3 s (LUFS, a second apart).
    short: [f32; 3],
    /// The envelope's (5 ms) largest autocorrelation past 50 ms — its slow swell taken out — and its lag (ms).
    period: (f64, f64),
}

/// One octave band of `x` for its kurtosis: two band-pass sections at `centre` (their skirts read a neighbour's
/// energy ~10 dB down — the shares come from the spectrum).
fn octave(centre: f64) -> [Biquad; 2] {
    [Biquad::new(Shape::BandPass, centre, 1.0, 0.0); 2]
}

/// The two sides' peak over their RMS (dB), each through its own copy of `sections`.
fn crest_db(l: &[f32], r: &[f32], sections: [Biquad; 2]) -> f64 {
    let (mut peak, mut sum) = (0.0f64, 0.0f64);
    for side in [l, r] {
        let mut s = sections;
        for &x in side {
            let y = through(&mut s, f64::from(x));
            (peak, sum) = (peak.max(y.abs()), sum + y * y);
        }
    }
    20.0 * (peak / (sum / (l.len() + r.len()) as f64).sqrt()).log10()
}

/// The spectrum's length: 5.9 Hz a bin.
const FFT: usize = 8_192;

/// In place, radix 2.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let (wr, wi) =
            ((-std::f64::consts::TAU / len as f64).cos(), (-std::f64::consts::TAU / len as f64).sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                (re[b], im[b]) = (re[a] - tr, im[a] - ti);
                (re[a], im[a]) = (re[a] + tr, im[a] + ti);
                (cr, ci) = (cr * wr - ci * wi, cr * wi + ci * wr);
            }
        }
        len <<= 1;
    }
}

/// The power spectra (Welch: Hann windows, at most ~400 of them spread over the whole) of the left, the right and
/// their sum, `FFT / 2` bins each — the two sides in one transform, as the real and imaginary parts.
fn spectra(l: &[f32], r: &[f32]) -> [Vec<f64>; 3] {
    let mut out = [vec![0.0f64; FFT / 2], vec![0.0f64; FFT / 2], vec![0.0f64; FFT / 2]];
    if l.len() < FFT {
        return out;
    }
    let hop = ((l.len() - FFT) / 400).max(FFT / 2);
    let hann: Vec<f64> =
        (0..FFT).map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / FFT as f64).cos()).collect();
    let (mut re, mut im) = (vec![0.0f64; FFT], vec![0.0f64; FFT]);
    for start in (0..=l.len() - FFT).step_by(hop) {
        for i in 0..FFT {
            (re[i], im[i]) = (f64::from(l[start + i]) * hann[i], f64::from(r[start + i]) * hann[i]);
        }
        fft(&mut re, &mut im);
        for k in 1..FFT / 2 {
            let m = FFT - k;
            // (Z[k] = L[k] + iR[k]: L = (Z[k] + conj Z[N−k]) / 2, R = (Z[k] − conj Z[N−k]) / 2i)
            let (lr, li) = ((re[k] + re[m]) / 2.0, (im[k] - im[m]) / 2.0);
            let (rr, ri) = ((im[k] + im[m]) / 2.0, (re[m] - re[k]) / 2.0);
            out[0][k] += lr * lr + li * li;
            out[1][k] += rr * rr + ri * ri;
            out[2][k] += (lr + rr).powi(2) + (li + ri).powi(2);
        }
    }
    out
}

/// The two sides' kurtosis in the octave band at `centre`.
fn kurtosis_at(l: &[f32], r: &[f32], centre: f64) -> f64 {
    let (mut s2, mut s4) = (0.0f64, 0.0f64);
    for side in [l, r] {
        let mut f = octave(centre);
        for &x in side {
            let y = through(&mut f, f64::from(x));
            (s2, s4) = (s2 + y * y, s4 + y.powi(4));
        }
    }
    let n = (l.len() + r.len()) as f64;
    (s4 / n) / (s2 / n).powi(2)
}

fn figures(l: &[f32], r: &[f32]) -> Figures {
    let (mut ll, mut rr, mut lr) = (0.0f64, 0.0f64, 0.0f64);
    for (&a, &b) in l.iter().zip(r) {
        let (a, b) = (f64::from(a), f64::from(b));
        (ll, rr, lr) = (ll + a * a, rr + b * b, lr + a * b);
    }
    let total = ll + rr;
    let [sl, sr, ss] = spectra(l, r);
    let all: f64 = sl.iter().zip(&sr).map(|(a, b)| a + b).sum();
    let bands = CENTRES
        .iter()
        .map(|&c| {
            let bin = |f: f64| ((f * FFT as f64 / R).round() as usize).min(FFT / 2);
            let (from, to) = (bin(c / 2f64.sqrt()), bin(c * 2f64.sqrt()));
            let two: f64 = (from..to).map(|k| sl[k] + sr[k]).sum();
            let sum: f64 = ss[from..to].iter().sum();
            Band {
                share_db: 10.0 * (two / all).log10(),
                kurtosis: kurtosis_at(l, r, c),
                mono_db: 10.0 * (sum / two).log10(),
            }
        })
        .collect();
    let peak = l.iter().chain(r).fold(0.0f32, |p, x| p.max(x.abs()));
    let under_120_db =
        10.0 * ((1..(120.0 * FFT as f64 / R) as usize).map(|k| sl[k] + sr[k]).sum::<f64>() / all).log10();
    let (tl, tr) = (sound::short_term(l, 3.0, 1.0), sound::short_term(r, 3.0, 1.0));
    let power = |x: f32| 10f64.powf(f64::from(x) / 10.0);
    let mut short: Vec<f32> =
        tl.iter().zip(&tr).map(|(a, b)| (10.0 * (power(*a) + power(*b)).log10()) as f32).collect();
    short.sort_by(f32::total_cmp);
    let short = match short.len() {
        0 => [f32::NAN; 3],
        n => [short[0], short[n / 2], short[n - 1]],
    };
    Figures {
        lufs: stereo_lufs(l, r),
        peak_db: 20.0 * peak.log10(),
        crest_top_db: crest_db(l, r, high_pass(6_000.0)),
        crest_3k_db: crest_db(l, r, octave(3_000.0)),
        bands,
        corr: lr / (ll * rr).sqrt(),
        mono_db: 10.0 * ((ll + rr + 2.0 * lr) / total).log10(),
        under_120_db,
        short,
        period: period(l, r),
    }
}

/// The envelope's largest autocorrelation from 50 ms to 5 s, and its lag (ms): the RMS of each 5 ms of the two sides
/// together, less the mean of the second round it (the swell is slow by design; a rhythm isn't).
fn period(l: &[f32], r: &[f32]) -> (f64, f64) {
    let frame = RATE as usize / 200;
    let env: Vec<f64> = (0..l.len() / frame)
        .map(|k| {
            let s: f64 =
                (k * frame..(k + 1) * frame).map(|i| f64::from(l[i]).powi(2) + f64::from(r[i]).powi(2)).sum();
            (s / frame as f64).sqrt()
        })
        .collect();
    let mut sums = vec![0.0f64; env.len() + 1];
    for (i, e) in env.iter().enumerate() {
        sums[i + 1] = sums[i] + e;
    }
    let w = 100;
    let d: Vec<f64> = (0..env.len())
        .map(|k| {
            let (a, b) = (k.saturating_sub(w), (k + w + 1).min(env.len()));
            env[k] - (sums[b] - sums[a]) / (b - a) as f64
        })
        .collect();
    let var: f64 = d.iter().map(|x| x * x).sum();
    let mut best = (f64::NEG_INFINITY, 0.0);
    for lag in 10..=1_000.min(d.len() / 2) {
        let c: f64 = d.iter().zip(&d[lag..]).map(|(a, b)| a * b).sum::<f64>() / var;
        if c > best.0 {
            best = (c, lag as f64 * 5.0);
        }
    }
    best
}

/// The figures, side by side (each named: the WAV, the AAC).
fn table(cols: &[(&str, &Figures)]) -> String {
    let mut s = String::new();
    let row = |s: &mut String, name: &str, cells: Vec<String>| {
        s.push_str(&format!("  {name:<44}"));
        for c in cells {
            s.push_str(&format!("{c:>22}"));
        }
        s.push('\n');
    };
    row(&mut s, "", cols.iter().map(|c| c.0.to_string()).collect());
    let each = |f: &dyn Fn(&Figures) -> String| cols.iter().map(|c| f(c.1)).collect::<Vec<_>>();
    row(&mut s, "loudness (LUFS)", each(&|f| format!("{:.1}", f.lufs)));
    row(&mut s, "sample peak (dBFS)", each(&|f| format!("{:.1}", f.peak_db)));
    row(&mut s, "sample peak over the loudness (dB)", each(&|f| format!("{:.1}", f.peak_db - f.lufs)));
    row(
        &mut s,
        "crest over 6 kHz / in the 3 kHz octave (dB)",
        each(&|f| format!("{:.1} / {:.1}", f.crest_top_db, f.crest_3k_db)),
    );
    row(&mut s, "L/R correlation", each(&|f| format!("{:.3}", f.corr)));
    row(&mut s, "mono sum against the two sides (dB)", each(&|f| format!("{:+.2}", f.mono_db)));
    row(&mut s, "under 120 Hz (dB)", each(&|f| format!("{:.1}", f.under_120_db)));
    row(
        &mut s,
        "3 s loudness: quietest / median / loudest",
        each(&|f| format!("{:.1} / {:.1} / {:.1}", f.short[0], f.short[1], f.short[2])),
    );
    row(
        &mut s,
        "envelope: most autocorrelated past 50 ms",
        each(&|f| format!("{:.3} at {:.0} ms", f.period.0, f.period.1)),
    );
    row(&mut s, "octave band: share dB · kurtosis · mono dB", vec![]);
    for (i, c) in CENTRES.iter().enumerate() {
        let name = match *c >= 1_000.0 {
            true => format!("  {} kHz", c / 1_000.0),
            false => format!("  {c} Hz"),
        };
        row(
            &mut s,
            &name,
            each(&|f| {
                let b = &f.bands[i];
                format!("{:6.1} · {:5.2} · {:+5.2}", b.share_db, b.kurtosis, b.mono_db)
            }),
        );
    }
    s
}

/// The share of `x`'s energy over `f` (a fourth-order high-pass).
fn share_over(x: &[f32], f: f64) -> f64 {
    let mut hp = high_pass(f);
    let (mut over, mut all) = (0.0f64, 0.0f64);
    for &v in x {
        let v = f64::from(v);
        let h = through(&mut hp, v);
        (over, all) = (over + h * h, all + v * v);
    }
    over / all
}

// ── The samples, as a viewer gets them ───────────────────────────────────────────────────────────────────────

fn samples(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let ffmpeg = crate::video::ffmpeg().ok_or("rain samples: no ffmpeg")?;
    let frames = u64::from(SAMPLE_S * RATE);
    let seed = 1;
    let mut report = format!(
        "The full video's rain — each kind's {SAMPLE_S} s, seed {seed}: the WAV as made and its AAC (128 kb/s, \
         decoded back) side by side.\n\n"
    );
    for kind in KINDS {
        let name = kind.name();
        let wav = dir.join(format!("rain-{name}.wav"));
        let mp4 = dir.join(format!("rain-{name}.mp4"));
        let started = Instant::now();
        let file = std::fs::File::create(&wav).map_err(|e| format!("{}: {e}", wav.display()))?;
        let mut w = std::io::BufWriter::new(file);
        let made = write(&mut w, kind, seed, frames)?;
        w.flush().map_err(|e| e.to_string())?;
        let took = started.elapsed().as_secs_f64();
        encode(&ffmpeg, &format!("rain · {name}"), &wav, SAMPLE_S, &mp4)?;
        for (from, png, filter) in [
            (&wav, format!("rain-{name}.png"), "[0:a]showspectrumpic=s=1600x600:legend=1:scale=log"),
            (&mp4, format!("rain-{name}-aac.png"), "[0:a]showspectrumpic=s=1600x600:legend=1:scale=log"),
            (
                &wav,
                format!("rain-{name}-2s.png"),
                "[0:a]atrim=0:2,showspectrumpic=s=1600x600:legend=1:scale=log",
            ),
        ] {
            ff(&ffmpeg, |c| {
                c.arg("-i").arg(from).args(["-filter_complex", filter]).arg(dir.join(&png));
            })?;
        }
        let (l, r) = read_wav(&wav)?;
        let (al, ar) = decoded(&ffmpeg, &mp4)?;
        let alone = |on: Layers| render(kind, seed, frames, on).map(|(l, r, _)| (l, r));
        let (wl, wr) = alone(Layers { wash: true, ..NONE })?;
        let (fl, fr) = alone(Layers { fine: true, ..NONE })?;
        let (cl, cr) = alone(Layers { clear: true, ..NONE })?;
        let drops = |l: &[f32], r: &[f32]| {
            format!(
                "{:.1} (kurtosis {:.1} at 4 kHz, {:.1} at 8 kHz; crest {:.1} dB over 6 kHz, {:.1} in the 3 kHz \
                 octave)",
                stereo_lufs(l, r),
                kurtosis_at(l, r, 4_000.0),
                kurtosis_at(l, r, 8_000.0),
                crest_db(l, r, high_pass(6_000.0)),
                crest_db(l, r, octave(3_000.0)),
            )
        };
        let near = match kind.measure().near_rate > 0.0 {
            true => {
                let (nl, nr) = alone(Layers { near: true, ..NONE })?;
                let tp =
                    sound::true_peaks(&nl).into_iter().chain(sound::true_peaks(&nr)).fold(0.0f32, f32::max);
                format!(
                    "near drops {:.1} (true peak {:.1} dBTP, {:.2} % of its energy over 3 kHz)",
                    stereo_lufs(&nl, &nr),
                    20.0 * tp.log10(),
                    100.0 * share_over(&[nl, nr].concat(), 3_000.0),
                )
            }
            false => "no near drops".into(),
        };
        report.push_str(&format!(
            "{name}: gain {:+.1} dB to about {LOUDNESS} LUFS · {} samples past the knee · peak {:.1} dBFS · made in \
             {took:.1} s\n  each layer alone (LUFS): wash {:.1} · fine drops {} · clear drops {}\n  {near}\n",
            made.gain_db,
            made.kneed,
            20.0 * made.peak.log10(),
            stereo_lufs(&wl, &wr),
            drops(&fl, &fr),
            drops(&cl, &cr),
        ));
        report.push_str(&table(&[("WAV", &figures(&l, &r)), ("AAC 128k", &figures(&al, &ar))]));
        report.push('\n');
    }
    let txt = dir.join("rain.txt");
    std::fs::write(&txt, &report).map_err(|e| format!("{}: {e}", txt.display()))?;
    print!("{report}");
    Ok(())
}

/// A sample as a viewer gets it: `words` on a still, `wav` as AAC at 128 kb/s, the picture cut to the sound's `secs`
/// (`-shortest` alone left a looped still running on: 94–96 s of picture for 25 s of sound).
fn encode(ffmpeg: &Path, words: &str, wav: &Path, secs: u32, mp4: &Path) -> Result<()> {
    let still = mp4.with_extension("ppm");
    std::fs::write(&still, picture(words)).map_err(|e| format!("{}: {e}", still.display()))?;
    let made = ff(ffmpeg, |c| {
        c.args(["-loop", "1", "-framerate", "1", "-i"]).arg(&still).arg("-i").arg(wav);
        c.args([
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-c:v",
            "libx264",
            "-tune",
            "stillimage",
            "-pix_fmt",
            "yuv420p",
        ]);
        c.args([
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-shortest",
            "-t",
            &secs.to_string(),
            "-movflags",
            "+faststart",
        ]);
        c.arg(mp4);
    });
    let _ = std::fs::remove_file(&still);
    made
}

/// ffmpeg, quiet, overwriting, with what `args` adds.
fn ff(ffmpeg: &Path, args: impl FnOnce(&mut Command)) -> Result<()> {
    let mut c = Command::new(ffmpeg);
    c.args(["-hide_banner", "-loglevel", "error", "-y"]);
    args(&mut c);
    let out = c.output().map_err(|e| format!("{}: {e}", ffmpeg.display()))?;
    match out.status.success() {
        true => Ok(()),
        false => Err(format!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim())),
    }
}

/// An MP4's sound, decoded back to 48 kHz stereo.
fn decoded(ffmpeg: &Path, mp4: &Path) -> Result<(Vec<f32>, Vec<f32>)> {
    let out = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(mp4)
        .args(["-map", "0:a", "-f", "s16le", "-ac", "2", "-ar", "48000", "-"])
        .output()
        .map_err(|e| format!("{}: {e}", ffmpeg.display()))?;
    match out.status.success() {
        true => Ok(sides(&out.stdout)),
        false => Err(format!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim())),
    }
}

/// A plain still that names the kind: its words in a small pixel face, ink on meok's ground (a PPM, 1280×720).
fn picture(words: &str) -> Vec<u8> {
    let (w, h, scale) = (1280usize, 720usize, 9usize);
    let (ground, ink) = ([15u8, 20, 25], [214u8, 208, 194]);
    let mut px: Vec<[u8; 3]> = vec![ground; w * h];
    let n = words.chars().count();
    let (x0, y0) = ((w - (n * 6 - 1) * scale) / 2, (h - 8 * scale) / 2);
    for (ci, c) in words.chars().enumerate() {
        for (row, bits) in glyph(c).iter().enumerate() {
            for col in 0..5 {
                if bits >> (4 - col) & 1 == 1 {
                    for dy in 0..scale {
                        let (x, y) = (x0 + (ci * 6 + col) * scale, y0 + row * scale + dy);
                        px[y * w + x..y * w + x + scale].fill(ink);
                    }
                }
            }
        }
    }
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    out.extend(px.into_iter().flatten());
    out
}

/// 5×8 (the last row under the line), the letters the stills need.
fn glyph(c: char) -> [u8; 8] {
    match c {
        'a' => [0, 0, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111, 0],
        'e' => [0, 0, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110, 0],
        'f' => [0b00110, 0b01000, 0b11100, 0b01000, 0b01000, 0b01000, 0b01000, 0],
        'h' => [0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0],
        'i' => [0b00100, 0, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110, 0],
        'l' => [0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110, 0],
        'n' => [0, 0, 0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0],
        'o' => [0, 0, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110, 0],
        'r' => [0, 0, 0b10110, 0b11001, 0b10000, 0b10000, 0b10000, 0],
        's' => [0, 0, 0b01111, 0b10000, 0b01110, 0b00001, 0b11110, 0],
        't' => [0b01000, 0b01000, 0b11100, 0b01000, 0b01000, 0b01001, 0b00110, 0],
        'v' => [0, 0, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100, 0],
        'y' => [0, 0, 0b10001, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110],
        '·' => [0, 0, 0, 0, 0b00100, 0, 0, 0],
        _ => [0; 8],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(kind: Kind, on: Layers, s: u32) -> (Vec<f32>, Vec<f32>) {
        let (l, r, _) = render(kind, 1, u64::from(s * RATE), on).unwrap();
        (l, r)
    }

    fn wav(kind: Kind, seed: u64, frames: u64) -> Vec<u8> {
        let mut b = Vec::new();
        write(&mut b, kind, seed, frames).unwrap();
        b
    }

    /// The same seed, the same bytes; another seed, others; the length asked, to the sample (across a block's end).
    #[test]
    fn the_same_seed_gives_the_same_bytes_at_the_length_asked() {
        let frames = u64::from(RATE) * 2 + 4_801;
        let (a, b, c) =
            (wav(Kind::Leaves, 1, frames), wav(Kind::Leaves, 1, frames), wav(Kind::Leaves, 2, frames));
        assert!(a == b, "the same seed");
        assert!(a[44..] != c[44..], "another seed");
        assert_eq!(a.len() as u64, 44 + frames * 4);
        assert_eq!(u32::from_le_bytes([a[40], a[41], a[42], a[43]]) as u64, frames * 4);
        assert_eq!(frames_of("12m").unwrap(), 12 * 60 * u64::from(RATE));
        assert_eq!(frames_of("1m30s").unwrap(), 90 * u64::from(RATE));
        assert_eq!(frames_of("500ms").unwrap(), u64::from(RATE) / 2);
        assert!(frames_of("12").is_err() && frames_of("").is_err() && frames_of("2x").is_err());
    }

    /// No sample over the file's own ceiling (the knee bends whatever comes near it, as made under it); no DC.
    #[test]
    fn no_sample_over_its_ceiling_and_no_dc() {
        for x in (0..2_000).map(|i| i as f32 / 100.0) {
            assert!(limit(x) <= CEILING && limit(-x) >= -CEILING, "{x} → {}", limit(x));
            assert!(x > KNEE || limit(x) == x, "{x} as made under the knee");
        }
        for kind in KINDS {
            let b = wav(kind, 1, u64::from(RATE) * 4);
            let s: Vec<i16> = b[44..].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
            let most = s.iter().map(|v| v.unsigned_abs()).max().unwrap();
            assert!(f32::from(most) / 32_768.0 <= CEILING, "{kind:?}: {most}");
            for side in 0..2 {
                let mean =
                    s.iter().skip(side).step_by(2).map(|v| f64::from(*v)).sum::<f64>() / (s.len() / 2) as f64;
                assert!(mean.abs() / 32_768.0 < 1e-4, "{kind:?}'s DC on side {side}: {mean}");
            }
        }
    }

    /// Wide: the two sides made apart, their correlation low.
    #[test]
    fn left_and_right_are_apart() {
        for kind in KINDS {
            let (l, r) = secs(kind, ALL, 4);
            let f = figures(&l, &r);
            assert!(f.corr.abs() < 0.1, "{kind:?}: {:.3}", f.corr);
        }
    }

    /// Each kind's spectrum stays rain: nothing under 120 Hz, little at the very top, the body between — so a later
    /// change can't turn it into hiss or rumble without this saying so. The downpour fuller in the low-mids.
    #[test]
    fn each_kinds_bands_stay_rain() {
        let mut low_mids = Vec::new();
        for kind in KINDS {
            let (l, r) = secs(kind, ALL, 4);
            let f = figures(&l, &r);
            let b: Vec<f64> = f.bands.iter().map(|b| b.share_db).collect();
            let say = format!("{kind:?}: {b:.1?}, {:.1} dB under 120 Hz", f.under_120_db);
            assert!(f.under_120_db < -30.0, "nothing under 120 Hz — {say}");
            assert!(b[0] < -45.0, "no rumble at 63 Hz — {say}");
            assert!(b[1] < -15.0, "little at 125 Hz — {say}");
            assert!(b[8] < -15.0, "little at 16 kHz — {say}");
            for (i, c) in CENTRES.iter().enumerate().take(8).skip(3) {
                assert!(b[i] > -14.0, "its body at {c} Hz — {say}");
            }
            let top = b[6].max(b[7]);
            assert!(top - b[4] < 6.0, "not hiss: the top no more than 6 dB over 1 kHz — {say}");
            low_mids.push(b[2]);
        }
        assert!(
            low_mids[2] > low_mids[0] + 3.0,
            "the downpour fuller at 250 Hz than the soft: {low_mids:.1?}"
        );
    }

    /// It isn't hiss and it isn't crackle: the wash alone grainless (kurtosis near plain noise's 3), the whole
    /// grainier at 4 kHz where the clear drops are, and no band carried by a few loud grains (the first patter's 28–54
    /// at 8 and 16 kHz).
    #[test]
    fn it_isnt_hiss_nor_crackle() {
        let (l, r) = secs(Kind::Leaves, Layers { wash: true, ..NONE }, 4);
        let wash = kurtosis_at(&l, &r, 4_000.0);
        assert!((2.7..3.3).contains(&wash), "the wash alone at 4 kHz: {wash:.2}");
        for kind in KINDS {
            let (l, r) = secs(kind, ALL, 4);
            let at = |c| kurtosis_at(&l, &r, c);
            assert!(at(4_000.0) > 3.4, "{kind:?}'s whole at 4 kHz: {:.2}", at(4_000.0));
            for c in [2_000.0, 4_000.0, 8_000.0, 16_000.0] {
                assert!(at(c) < 12.0, "{kind:?} at {c} Hz: {:.2}", at(c));
            }
        }
    }

    /// The patter's two sizes: the fine drops alone near plain noise (their many overlapping), the clear drops alone
    /// well over it (each heard).
    #[test]
    fn the_fine_drops_are_near_noise_the_clear_ones_grainy() {
        for kind in KINDS {
            let (l, r) = secs(kind, Layers { fine: true, ..NONE }, 4);
            for c in [4_000.0, 8_000.0] {
                let k = kurtosis_at(&l, &r, c);
                assert!(k < 4.5, "{kind:?}'s fine drops at {c} Hz: {k:.2}");
            }
            let (l, r) = secs(kind, Layers { clear: true, ..NONE }, 4);
            let k = kurtosis_at(&l, &r, 4_000.0);
            assert!(k > 6.0, "{kind:?}'s clear drops at 4 kHz: {k:.2}");
        }
    }

    /// No drop is a key: the top's loudest grain not far over its body (crest over 6 kHz and in the 3 kHz octave),
    /// the file's peak not far over its loudness — the first patter's stood 27–31 dB, 22–25 dB and 17–20 dB, a bright
    /// tick about 5 dB under a key's click with the bed at the keys' level.
    #[test]
    fn no_drop_is_a_key() {
        for kind in KINDS {
            let (l, r) = secs(kind, ALL, 8);
            let (top, mid) = (crest_db(&l, &r, high_pass(6_000.0)), crest_db(&l, &r, octave(3_000.0)));
            let peak = 20.0 * l.iter().chain(&r).fold(0.0f32, |p, x| p.max(x.abs())).log10();
            let over = peak - stereo_lufs(&l, &r);
            assert!(top <= 20.0, "{kind:?}: crest over 6 kHz {top:.1} dB");
            assert!(mid <= 18.5, "{kind:?}: crest in the 3 kHz octave {mid:.1} dB");
            assert!(over <= 14.0, "{kind:?}: peak {over:.1} dB over its loudness");
        }
    }

    /// A sample's picture ends with its sound (the first samples' looped still ran 94–96 s past 25 s of sound; 10 s
    /// came out right, so it's the samples' own length, of silence). Without ffmpeg and ffprobe it says nothing.
    #[test]
    fn a_samples_picture_ends_with_its_sound() {
        let Some(ffmpeg) = crate::video::ffmpeg() else { return };
        let ffprobe = ffmpeg.with_file_name("ffprobe");
        if !ffprobe.is_file() {
            return;
        }
        let dir = tarae_testkit::scratch::Scratch::new("xtask-rain-mp4");
        let (wav_at, mp4) = (dir.join("rain.wav"), dir.join("rain.mp4"));
        let frames = u64::from(SAMPLE_S * RATE);
        let mut silence = head(frames).unwrap();
        silence.resize(44 + frames as usize * 4, 0);
        std::fs::write(&wav_at, silence).unwrap();
        encode(&ffmpeg, "rain", &wav_at, SAMPLE_S, &mp4).unwrap();
        let out = Command::new(&ffprobe)
            .args(["-v", "error", "-show_entries", "stream=codec_type,duration", "-of", "csv=p=0"])
            .arg(&mp4)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        let of = |kind: &str| {
            said.lines()
                .find_map(|l| l.strip_prefix(kind)?.strip_prefix(',')?.trim().parse::<f64>().ok())
                .unwrap_or_else(|| panic!("no {kind} stream's length in {said:?}"))
        };
        let (picture, sound) = (of("video"), of("audio"));
        assert!((picture - sound).abs() < 0.1, "the picture {picture} s, the sound {sound} s");
        assert!(!mp4.with_extension("ppm").exists(), "the still left behind");
    }

    /// A near drop isn't a key: dull, nearly all its energy under 3 kHz.
    #[test]
    fn a_near_drop_isnt_a_key() {
        let (l, r) = secs(Kind::Leaves, Layers { near: true, ..NONE }, 8);
        let over = share_over(&[l, r].concat(), 3_000.0);
        assert!(over < 0.01, "{:.2} % of its energy over 3 kHz", over * 100.0);
    }

    /// The clear drops' arrivals in a made-up minute: as many as chance allows, never on a grid (their gaps as uneven
    /// as a Poisson process's — a grid's are all one, its coefficient of variation 0). The fine drops' are the same
    /// `Arrivals`.
    #[test]
    fn the_patter_arrives_by_chance_never_on_a_grid() {
        let rate = Kind::Leaves.measure().clear_rate;
        let mut a = Arrivals::new(Rng::new(1, 4), rate);
        let mut at = Vec::new();
        while let Some(t) = a.take(60 * u64::from(RATE)) {
            at.push(t as f64);
        }
        let (mean, n) = (rate * 60.0, at.len() as f64);
        assert!((n - mean).abs() < 5.0 * mean.sqrt(), "{n} drops in a minute, {mean} by chance");
        let gaps: Vec<f64> = at.windows(2).map(|w| w[1] - w[0]).collect();
        let m = gaps.iter().sum::<f64>() / gaps.len() as f64;
        let sd = (gaps.iter().map(|g| (g - m).powi(2)).sum::<f64>() / gaps.len() as f64).sqrt();
        assert!((0.9..1.1).contains(&(sd / m)), "their gaps' variation: {:.3}", sd / m);
    }
}
