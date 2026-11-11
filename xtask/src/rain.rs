//! `cargo xtask rain OUT.wav --len 12m [--kind forest|forest-sparse|forest-sparse-pat|forest-thick|soft|leaves|heavy]
//! [--seed N] [--drops DB]` — the full video's bed: rain, made from noise as the brush is (`.sb/full-video-rain-brief.md`;
//! showcase's "The rain is made, not found", and round 3's forest drizzle, `.sb/full-video-rain-round3.md`, 2026-11-11,
//! with round 4's fewer near drops and round 5's drop a bubble, `.sb/full-video-rain-round5.md`). Stereo, 48 kHz, 16
//! bits, as long as asked — no loop, nothing to seam —, written a block at a time, never held whole; the same seed,
//! the same bytes.
//!
//! **`forest`**, the default ("숲속 가랑비"), is a drizzle heard from inside a forest, in depth — each distance
//! quieter than the one before. Every drop is the bubble it traps (`bubble`, van den Doel's model): a sine at 3 / r
//! that rings a few milliseconds, dying as e^(−d·t) and sliding up as it dies, its amplitude as r^1.5 — a bigger
//! drop lower and louder —, eased in so nothing clicks. A tone and not a tap: round 4's dull bursts of noise were "a
//! keyboard".
//!
//! - **Near drops**, the ones you count: `Measure::pat_rate` a second (about 2.5; round 3's 7.5 came at a typist's
//!   pace and "barely sounded like rain"), at `LEAVES` places that stay where they are (`surfaces`), each with its
//!   own size of bubble (`NEAR_R`: 3.7 kHz down to 1.4 kHz); each strike's bubble a little apart (`STRIKE_R`), and a
//!   third ring again, a smaller bubble 10–40 ms later (`rings`, `drop_of`). Their loudest 5 ms about 19 dB over the
//!   hush at the median (`NEAR_DB`; round 4's 24).
//! - **Mid drops**: `Measure::mid_rate` a second (4.5), tiny bubbles (`MID_R`) each of its own size, `Measure::mid_db`
//!   under round 4's near pats (24–30 dB: about half sink into the hush, texture), less wide. Round 3's 19 a second,
//!   8–10 dB under, all stood out: with its near pats, 20 drops a second over the hush; round 4's `forest` about 5.5
//!   (`standing_out`).
//! - **Drips**: `Measure::drip_rate` a second (0.3–1), big bubbles (`DRIP_R`: 1 kHz down to 500 Hz), from `SPOTS`
//!   places to the sides; some heard twice, the drop and then its fall 60–120 ms later.
//! - **The far hush**: the wash and the fine drops below, darker, at `HUSH_LUFS` in the file (`hush_gain`), where
//!   round 3 had it — faint, not gone: single drops on silence sound like clicks in a void. The forest's gain is
//!   fixed, round 3's (`FOREST_GAIN_DB`): fewer drops over the same hush measure quieter, the cut sets the level.
//!
//! The drizzle thickens and thins in how many drops fall (`Density`): by up to about a third over twenty to forty
//! seconds. A drop is a tone (`a_drop_is_a_tone`: 10–90 % of its energy within 0.4 octaves), slides up
//! (`a_drop_slides_up`), doesn't click (`a_drop_doesnt_click`) and a place isn't a note (`a_place_isnt_a_note`).
//! **`forest-sparse`** has fewer near and mid drops still (`SPARSE` of them: 1.5 and 2.7 a second);
//! **`forest-sparse-pat`** is it with a faint pat under each near drop (round 4's burst, `PAT_UNDER_DB` under the
//! bubble: the drop landing on something); **`forest-thick`** falls half again as thick (`THICK`).
//!
//! `--drops DB` sets a forest kind's drops — the near ones with the pats under them, the mid ones and the drips,
//! together — against its far hush (the full video's V2b calls: a balance right alone is wrong under keys, where the
//! hush has to come up and the drops less). `0` is the kind as it is, to the byte
//! (`drops_at_0_leave_every_kinds_bytes_as_they_were`); the round-2 kinds have no hush and refuse it.
//!
//! The round-2 kinds (`soft`, `leaves`, `heavy`) are three layers, each made apart for the left and the right:
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
//! a viewer reads as meaning something). Brought to about `Kind::loudness` by its own first `CALIBRATE_S` seconds, a soft
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
use crate::sound::{self, RATE, stereo_lufs};

const USAGE: &str = "usage: cargo xtask rain OUT.wav --len 12m [--kind \
                     forest|forest-sparse|forest-sparse-pat|forest-thick|soft|leaves|heavy] [--seed N] [--drops DB] | rain \
                     samples [DIR] | rain figures FILE.wav …";

/// The file's own ceiling (−6 dBFS): 6 dB left for the cut, which sets the level.
const CEILING: f32 = 0.5;
/// Where the soft knee starts (−9 dBFS): under it every sample is as made.
const KNEE: f32 = 0.35;
/// Each kind's loudness before the cut levels it (LUFS, the two sides' power summed as BS.1770 does).
const LOUDNESS: f32 = -27.0;
/// The forest isn't brought to a loudness: its gain is round 3's (seed 1's, to −30 LUFS), its far hush where round 3
/// had it in the file (`HUSH_LUFS`, alone). Fewer drops over the same hush measure quieter; the cut sets the level.
const FOREST_GAIN_DB: f32 = -21.7;
const HUSH_LUFS: f32 = -44.0;
/// The level is found on the file's own first seconds, as made.
const CALIBRATE_S: u32 = 8;
/// Made a block at a time, a second each.
const BLOCK: usize = RATE as usize;
/// Made and dropped first, so its filters and its walk have settled when the file starts.
const PREROLL: usize = RATE as usize / 2;
/// What a drop leaves past its block: room for the longest (a drip's fall, up to 120 ms after its block's last drip,
/// and its bubble's 220 ms to −60 dB, a 6.9 mm one's, with its far side's delay).
/// (A fine drop leaves nothing: its envelope runs in the sample loop.)
const RING: usize = BLOCK + RATE as usize / 2;
/// The walk steps every 10 ms, its gain eased between.
const CONTROL: u32 = RATE / 100;
/// The samples the user hears: each kind alone this long.
const SAMPLE_S: u32 = 25;
/// Their seed: its density walk near the middle over their 25 s (×0.93–×1.06; seed 1's sat at ×0.80–×0.99).
const SEED: u64 = 38;
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
    let (mut frames, mut kind, mut seed, mut drops) = (None, Kind::Forest, 1u64, 0.0f64);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("rain: {a} needs a value\n{USAGE}"));
        match a.as_str() {
            "--len" => frames = Some(frames_of(value()?)?),
            "--kind" => kind = Kind::parse(value()?)?,
            "--seed" => seed = value()?.parse().map_err(|_| format!("rain: --seed is a number\n{USAGE}"))?,
            "--drops" => drops = drops_db(value()?)?,
            _ => return Err(format!("rain: {a}?\n{USAGE}")),
        }
    }
    let frames = frames.ok_or_else(|| format!("rain: --len says how long (12m, 25s, 1m30s)\n{USAGE}"))?;
    if drops != 0.0 && !kind.has_hush() {
        return Err(format!(
            "rain: --drops is a forest kind's (its drops against its far hush); {} has no hush",
            kind.name()
        ));
    }
    let started = Instant::now();
    let file = std::fs::File::create(out).map_err(|e| format!("{out}: {e}"))?;
    let mut w = std::io::BufWriter::with_capacity(1 << 20, file);
    let made = write(&mut w, kind, seed, frames, drops)?;
    w.flush().map_err(|e| format!("{out}: {e}"))?;
    println!(
        "{out}: {}, seed {seed}{}, {:.1} s ({frames} frames, {:.1} MB) · gain {:+.1} dB · {} samples past the knee · \
         peak {:.1} dBFS · made in {:.1} s",
        kind.name(),
        if drops == 0.0 { String::new() } else { format!(", the drops {drops:+.1} dB against the hush") },
        frames as f64 / R,
        (44 + frames * 4) as f64 / 1e6,
        made.gain_db,
        made.kneed,
        20.0 * made.peak.log10(),
        started.elapsed().as_secs_f64(),
    );
    Ok(())
}

/// The most `--drops` moves the drops either way (dB).
const DROPS_MOST_DB: f64 = 24.0;

/// `--drops`' dB: a number within `DROPS_MOST_DB` of 0.
fn drops_db(s: &str) -> Result<f64> {
    match s.parse::<f64>() {
        Ok(db) if db.abs() <= DROPS_MOST_DB => Ok(db),
        _ => Err(format!(
            "rain: --drops is dB on the drops against the hush, -{DROPS_MOST_DB} to {DROPS_MOST_DB}: {s:?}"
        )),
    }
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
    /// Round 3: a drizzle heard from inside a forest — "숲속 가랑비"; round 4's fewer near drops, round 5's bubbles.
    Forest,
    /// The forest with fewer near drops still (`SPARSE`): how often a near one falls is the ears' choice.
    Sparse,
    /// `forest-sparse` with a faint pat under each near drop (`PAT_UNDER_DB`): the drop landing on something.
    SparsePat,
    /// The forest, half again as thick (`THICK`): how much rain is the ears' choice.
    Thick,
    /// Wash-led, a fine steady rain.
    Soft,
    /// More patter, and the near drops: "비오는 자연의 소리".
    Leaves,
    /// Denser and fuller in the low-mids: a downpour.
    Heavy,
}

const KINDS: [Kind; 7] =
    [Kind::Forest, Kind::Sparse, Kind::SparsePat, Kind::Thick, Kind::Soft, Kind::Leaves, Kind::Heavy];

/// How much thicker `forest-thick` falls.
const THICK: f64 = 1.5;
/// `forest-sparse`'s near and mid drops against `forest`'s (the drips as they are).
const SPARSE: f64 = 0.6;

/// A kind's measures. The levels are each layer's own, before the whole is brought to `LOUDNESS`.
#[derive(Clone, Copy)]
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
    /// The forest's near drops a second, both sides together, as the density stands at 1.
    pat_rate: f64,
    /// Mid drops a second, and how far under round 4's near pats (dB, at random).
    mid_rate: f64,
    mid_db: [f64; 2],
    /// A pat under each near drop, this far under its bubble (dB), or none.
    under_db: Option<f64>,
    /// Drips a second (each heard twice, `DRIP_PAIR` of them).
    drip_rate: f64,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Forest => "forest",
            Kind::Sparse => "forest-sparse",
            Kind::SparsePat => "forest-sparse-pat",
            Kind::Thick => "forest-thick",
            Kind::Soft => "soft",
            Kind::Leaves => "leaves",
            Kind::Heavy => "heavy",
        }
    }

    fn parse(s: &str) -> Result<Kind> {
        KINDS.into_iter().find(|k| k.name() == s).ok_or_else(|| {
            format!("rain: no kind {s:?} (forest, forest-sparse, forest-sparse-pat, forest-thick, soft, leaves, heavy)")
        })
    }

    /// A forest kind: drops over a far hush (`hush_gain`), which `--drops` sets them against.
    fn has_hush(self) -> bool {
        self.measure().pat_rate > 0.0
    }

    /// Its file's loudness before the cut levels it (LUFS); none for the forest, whose gain is fixed
    /// (`FOREST_GAIN_DB`).
    fn loudness(self) -> Option<f32> {
        match self.has_hush() {
            true => None,
            false => Some(LOUDNESS),
        }
    }

    fn measure(self) -> Measure {
        // (the far hush's level is set by `hush_gain`; the round-2 kinds' near drops are the forest's drips)
        // (round 3's 7.5 near pats a second, a typist's pace under a video of typing: "so often it barely sounds
        // like rain")
        let forest = Measure {
            wash: 1.0,
            wash_low: 250.0,
            low_mids: 0.0,
            top: -8.0,
            fine_rate: 12_000.0,
            fine: 0.05,
            pat_rate: 2.5,
            // (at 15–25 a second and 8–10 dB under the near ones, every mid pat stood out: round 3's 20 drops a
            // second over the hush; 24–30 under, about half sink into it, texture)
            mid_rate: 4.5,
            mid_db: [24.0, 30.0],
            drip_rate: 0.5,
            ..NO_PATS
        };
        match self {
            Kind::Forest => forest,
            Kind::Sparse => {
                Measure { pat_rate: forest.pat_rate * SPARSE, mid_rate: forest.mid_rate * SPARSE, ..forest }
            }
            Kind::SparsePat => Measure { under_db: Some(PAT_UNDER_DB), ..Kind::Sparse.measure() },
            Kind::Thick => Measure {
                pat_rate: forest.pat_rate * THICK,
                mid_rate: forest.mid_rate * THICK,
                drip_rate: forest.drip_rate * THICK,
                ..forest
            },
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
                ..NO_PATS
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
                ..NO_PATS
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
                ..NO_PATS
            },
        }
    }
}

/// The round-2 kinds' rest: no pats.
const NO_PATS: Measure = Measure {
    wash: 0.0,
    fine_rate: 0.0,
    fine: 0.0,
    wash_low: 0.0,
    low_mids: 0.0,
    top: 0.0,
    clear_rate: 0.0,
    clear: 0.0,
    near_rate: 0.0,
    near: 0.0,
    pat_rate: 0.0,
    mid_rate: 0.0,
    mid_db: [0.0; 2],
    under_db: None,
    drip_rate: 0.0,
};

/// Which layers sound: all, but in the tests and the figures of each alone.
#[derive(Clone, Copy)]
struct Layers {
    wash: bool,
    fine: bool,
    clear: bool,
    near: bool,
    /// The forest's near drops (and the pats under them, where there are), and its mid ones.
    pats: bool,
    mids: bool,
    drips: bool,
}

const ALL: Layers =
    Layers { wash: true, fine: true, clear: true, near: true, pats: true, mids: true, drips: true };
const NONE: Layers =
    Layers { wash: false, fine: false, clear: false, near: false, pats: false, mids: false, drips: false };

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

// ── The forest's drops: bubbles ──────────────────────────────────────────────────────────────────────────────
//
// (round 4's drop, a short dull burst of noise, was "a keyboard": noise that starts and stops is a tap, whatever its
// colour. What the ear takes for a water drop is the bubble it traps — van den Doel's model of liquid sounds.)

/// The bubbles' radii (m), smallest to largest: a near drop's place's (3.7 kHz down to 1.4 kHz), a mid drop's, tiny
/// (5 to 3 kHz), and a drip's place's (1 kHz down to 500 Hz).
const NEAR_R: [f64; 2] = [0.0008, 0.0022];
const MID_R: [f64; 2] = [0.0006, 0.0010];
const DRIP_R: [f64; 2] = [0.003, 0.006];
/// Each strike's bubble against its place's (up to ±15 %): never twice one pitch — a fixed pitch is a xylophone.
const STRIKE_R: f64 = 0.15;
/// How far a bubble's pitch rises as it dies: f₀ · (1 + `BUBBLE_RISE` · d · t), about a tenth by a third of its
/// amplitude.
const BUBBLE_RISE: f64 = 0.1;
/// A bubble's onset from zero (s): nothing clicks — its first 0.3 ms under 2 % of its energy
/// (`a_drop_doesnt_click`), which the model's 0.3–0.5 ms kept for the larger bubbles alone…
const BUBBLE_ONSET_S: [f64; 2] = [0.0003, 0.0005];
/// …so at least this many of its own time constants (1/d) too: the smallest ease in as gently (at 0.4, a 0.6 mm
/// bubble's first 0.3 ms held 2.35 %; at 0.6, the most is 0.9 %).
const BUBBLE_ONSET_TAU: f64 = 0.6;
/// The bubble the near drops' level is read on: 1.5 mm, 2 kHz.
const BUBBLE_REF_R: f64 = 0.0015;
/// The share of near drops that ring again (a second, smaller bubble — the stutter much of what sounds wet), of those
/// the share that ring a third time, how long after the bubble before (s) and how much smaller (its radius ×).
const STUTTER: f64 = 1.0 / 3.0;
const STUTTER_THIRD: f64 = 1.0 / 3.0;
const STUTTER_S: [f64; 2] = [0.010, 0.040];
const STUTTER_R: [f64; 2] = [0.55, 0.8];
/// A near drop's loudest 5 ms on its louder side, a 1.5 mm bubble's, against round 4's near pats' (dB): a tone stands
/// out more than a noise of the same peak. (At −6 the median near drop stood 16 dB over the hush, the smaller ones
/// quieter by their size: −3 puts it at about 19, 5 dB under round 4's 24.)
const NEAR_DB: f64 = -3.0;
/// The pat under a near drop, in the sample that has one (`Kind::SparsePat`): round 4's burst, under the bubble (dB).
const PAT_UNDER_DB: f64 = 12.0;

/// A pat's leaf, from the smallest to the largest: its pitch (Hz) and its length (s). Each a little apart from its
/// size's (±10 %, ±15 %).
const PAT_PITCH: [f64; 2] = [3_000.0, 800.0];
const PAT_LEN_S: [f64; 2] = [0.008, 0.025];
/// A pat's onset (s), whatever its leaf: eased in, never instant.
const PAT_ATTACK_S: [f64; 2] = [0.0014, 0.0024];
/// What a drop's level is read by: its loudest 5 ms.
const PAT_WINDOW: usize = RATE as usize / 200;
/// The low-pass over a pat (Hz, fourth order): dull.
const NEAR_TOP: f64 = 2_500.0;
/// The places near drops fall at, and the places drips fall from: a place's fixed surfaces.
const LEAVES: usize = 16;
const SPOTS: usize = 4;
/// A near drop under its place's loudest (dB, at random): each strike a little apart.
const NEAR_SPREAD_DB: f64 = 3.0;
/// How wide the mid drops are placed against the near ones.
const MID_WIDTH: f64 = 0.6;
/// A drip against a near drop's loudest (dB, round 4's, so down with them by `NEAR_DB`); the share of drips whose
/// fall is heard after them, how long after (s) and how much under (dB).
/// (at −1–+2, at their length, they were nearly as loud as all the pats together)
const DRIP_DB: [f64; 2] = [-5.0, -2.0];
const DRIP_PAIR: f64 = 0.4;
const DRIP_FALL_S: [f64; 2] = [0.06, 0.12];
const DRIP_FALL_DB: [f64; 2] = [2.0, 5.0];
/// A near pat's leaf, from the smallest to the largest: each band-pass section's Q and its ring's share of the
/// energy — round 4's short dull burst with a hint of pitch, now only the faint pat under a drop (`PAT_UNDER_DB`).
const NEAR_Q: [f64; 2] = [0.45, 0.6];
const NEAR_TONE: [f64; 2] = [0.06, 0.1];

/// What a pat on a leaf sounds like (round 4's near pat; now the faint pat under a near drop, where there is one).
#[derive(Clone, Copy)]
struct Leaf {
    pitch: f64,
    len_s: f64,
    q: f64,
    /// The share of its energy its ring has.
    tone: f64,
}

impl Leaf {
    /// A leaf of a size at random: a bigger one lower, longer and ringing more; broad and barely ringing (`NEAR_Q`,
    /// `NEAR_TONE`).
    fn near(what: &mut Rng) -> Leaf {
        let size = what.f();
        let along = |[a, b]: [f64; 2]| a + (b - a) * size;
        Leaf {
            pitch: (PAT_PITCH[0] * (PAT_PITCH[1] / PAT_PITCH[0]).powf(size) * what.range(0.9, 1.1))
                .clamp(PAT_PITCH[1], PAT_PITCH[0]),
            len_s: (along(PAT_LEN_S) * what.range(0.85, 1.15)).clamp(PAT_LEN_S[0], PAT_LEN_S[1]),
            q: along(NEAR_Q) * what.range(0.85, 1.15),
            tone: along(NEAR_TONE) * what.range(0.85, 1.15),
        }
    }
}

/// A place drops fall at: where (−1 left, 1 right), the size of the bubble a drop there traps (m, its radius) and the
/// leaf it lands on.
#[derive(Clone, Copy)]
struct Place {
    pan: f64,
    r: f64,
    leaf: Leaf,
}

/// The place's surfaces for `seed`: the `LEAVES` places the near drops fall at, spread across the stereo picture (one
/// to each of as many equal parts, somewhere in its middle), and the `SPOTS` places drips fall from, to the sides.
/// Each keeps its own size of bubble: the near ones one to each of as many equal parts of `NEAR_R` (by pitch), the
/// drips' anywhere in `DRIP_R`. (The places and leaves round 4's, the sizes from a stream of their own.)
fn surfaces(seed: u64) -> (Vec<Place>, Vec<Place>) {
    let mut what = Rng::new(seed, 18);
    let mut size = Rng::new(seed, 24);
    let spread = |what: &mut Rng, i: usize, n: usize, wide: f64| {
        -wide + 2.0 * wide * (i as f64 + what.range(0.2, 0.8)) / n as f64
    };
    let between = |[a, b]: [f64; 2], u: f64| a * (b / a).powf(u);
    let mut places: Vec<Place> = (0..LEAVES)
        .map(|i| {
            let pan = spread(&mut what, i, LEAVES, 0.95);
            let r = between(NEAR_R, (i as f64 + size.range(0.2, 0.8)) / LEAVES as f64);
            Place { pan, r, leaf: Leaf::near(&mut what) }
        })
        .collect();
    // (their order shuffled, so a leaf's size doesn't follow its place; then the bubbles', so neither does a drop's)
    for i in (1..places.len()).rev() {
        let j = (what.f() * (i + 1) as f64) as usize % (i + 1);
        let (a, b) = (places[i].pan, places[j].pan);
        (places[i].pan, places[j].pan) = (b, a);
    }
    for i in (1..places.len()).rev() {
        let j = (size.f() * (i + 1) as f64) as usize % (i + 1);
        let (a, b) = (places[i].r, places[j].r);
        (places[i].r, places[j].r) = (b, a);
    }
    // (out to the sides, one side then the other: in the middle, a drip's low pitch was nearly the same in both ears —
    // its layer's left and right correlated 0.69)
    let spots = (0..SPOTS)
        .map(|i| {
            let pan = if i % 2 == 0 { -1.0 } else { 1.0 } * what.range(0.5, 0.95);
            Place { pan, r: between(DRIP_R, size.f()), leaf: Leaf::near(&mut what) }
        })
        .collect();
    (places, spots)
}

/// A bubble of radius `r` (m): its pitch as it starts (Hz) and how fast it dies (e^(−d·t), d a second).
fn bubble_pitch(r: f64) -> f64 {
    3.0 / r
}

fn bubble_decay(r: f64) -> f64 {
    0.13 / r + 0.0072 * r.powf(-1.5)
}

/// One bubble of radius `r`, alone: a sine at `bubble_pitch` from zero over `onset` (s, raised-cosine), dying as
/// e^(−d·t) and rising in pitch as it dies (`BUBBLE_RISE`), till −60 dB; its amplitude (r / `BUBBLE_REF_R`)^1.5 — a
/// bigger drop lower and louder.
fn bubble(r: f64, onset: f64) -> Vec<f64> {
    let (f0, d) = (bubble_pitch(r), bubble_decay(r));
    let a = (r / BUBBLE_REF_R).powf(1.5);
    let onset = onset.max(BUBBLE_ONSET_TAU / d) * R;
    let len = (onset + 6.9 / d * R).ceil() as usize;
    let mut phase = 0.0f64;
    (0..len)
        .map(|k| {
            let (n, t) = (k as f64, k as f64 / R);
            let ease = if n < onset { 0.5 - 0.5 * (std::f64::consts::PI * n / onset).cos() } else { 1.0 };
            let x = a * ease * (-d * t).exp() * phase.sin();
            phase += std::f64::consts::TAU * f0 * (1.0 + BUBBLE_RISE * d * t) / R;
            x
        })
        .collect()
}

/// How many bubbles a near drop rings: once, or again (`STUTTER`), or a third time (`STUTTER_THIRD` of those).
fn rings(what: &mut Rng) -> usize {
    match what.f() {
        u if u < STUTTER * STUTTER_THIRD => 3,
        u if u < STUTTER => 2,
        _ => 1,
    }
}

/// One drop where bubbles of radius `r` form: its bubble a little apart from the place's (`STRIKE_R`), and `rings`
/// in all, each next one smaller — higher and, as its size has it, quieter — `STUTTER_S` after the one before.
fn drop_of(what: &mut Rng, r: f64, rings: usize) -> Vec<f64> {
    let mut r = r * what.range(1.0 - STRIKE_R, 1.0 + STRIKE_R);
    let (mut x, mut at) = (Vec::new(), 0usize);
    for i in 0..rings {
        if i > 0 {
            at += (what.range(STUTTER_S[0], STUTTER_S[1]) * R).round() as usize;
            r *= what.range(STUTTER_R[0], STUTTER_R[1]);
        }
        let b = bubble(r, what.range(BUBBLE_ONSET_S[0], BUBBLE_ONSET_S[1]));
        x.resize(x.len().max(at + b.len()), 0.0);
        for (k, v) in b.iter().enumerate() {
            x[at + k] += v;
        }
    }
    x
}

/// The gain that puts a near drop's loudest 5 ms, a `BUBBLE_REF_R` bubble's, at `NEAR_DB`: others louder or quieter
/// by their size.
fn near_gain() -> f64 {
    let mid = (BUBBLE_ONSET_S[0] + BUBBLE_ONSET_S[1]) / 2.0;
    10f64.powf(NEAR_DB / 20.0) / loudest(&bubble(BUBBLE_REF_R, mid), PAT_WINDOW)
}

/// A fourth-order Butterworth low-pass: two sections.
fn low_pass(f: f64) -> [Biquad; 2] {
    [Biquad::new(Shape::LowPass, f, 0.541, 0.0), Biquad::new(Shape::LowPass, f, 1.307, 0.0)]
}

/// One strike on `leaf` as it comes, alone and at no level yet: noise eased in over 1.4–2.4 ms and dying over the
/// leaf's length (e⁻² by its end, its last fifth faded out), rung through the leaf — two band-pass sections at its
/// pitch, a little resonance — and under its low-pass: dull. With it the leaf's own ring, a damped tone at its pitch
/// (`Leaf::tone` of the energy): noise alone peaked ~10 dB over its loudest 5 ms. Struck again, a leaf sounds like
/// itself: its pitch ±3 %, its length ±8 %.
fn pat_made(what: &mut Rng, leaf: &Leaf) -> Vec<f64> {
    let f = leaf.pitch * what.range(0.97, 1.03);
    let len = leaf.len_s * what.range(0.92, 1.08) * R;
    let (q, tone) = (leaf.q, leaf.tone);
    let attack = what.range(PAT_ATTACK_S[0], PAT_ATTACK_S[1]) * R;
    // (e⁻³ by its end, the shortest leaves held 90 % of their energy in 2.7 ms: a tick)
    let (tau, fade) = (len / 2.0, 0.8 * len);
    let mut dull = low_pass(NEAR_TOP);
    let [t0, t1] = low_pass(NEAR_TOP);
    let mut leaf = [Biquad::new(Shape::BandPass, f, q, 0.0), Biquad::new(Shape::BandPass, f, q, 0.0), t0, t1];
    let phase = what.range(0.0, std::f64::consts::TAU);
    let ease = |x: f64| 0.5 - 0.5 * (std::f64::consts::PI * x).cos();
    // (its envelope's length, and 5 ms for the leaf to ring out)
    let (mut noise, mut ring) = (Vec::new(), Vec::new());
    for k in 0..(len + 0.005 * R) as usize {
        let t = k as f64;
        let env = match t {
            t if t < attack => ease(t / attack),
            t if t < len => (-(t - attack) / tau).exp() * ease(((len - t) / (len - fade)).min(1.0)),
            _ => 0.0,
        };
        let n = if env > 0.0 { what.gauss() * env } else { 0.0 };
        noise.push(through(&mut leaf, n));
        ring.push(through(&mut dull, (phase + std::f64::consts::TAU * f * t / R).sin() * env));
    }
    let energy = |x: &[f64]| x.iter().map(|v| v * v).sum::<f64>().sqrt();
    let (a, b) = ((1.0 - tone).sqrt() / energy(&noise), tone.sqrt() / energy(&ring));
    noise.iter().zip(&ring).map(|(n, t)| n * a + t * b).collect()
}

/// The least time (ms) that may hold 90 % of a pat's energy, and the least onset (ms): over the tests' 3 and 1.
const PAT_SPAN_MS: f64 = 3.2;
const PAT_ONSET_MS: f64 = 1.1;

/// Whether a strike as it came is a pat: its energy not bunched into a tick, its onset not instant.
fn is_a_pat(x: &[f64]) -> bool {
    span90(x) >= PAT_SPAN_MS && onset(x) >= PAT_ONSET_MS
}

/// One strike on `leaf` (`pat_made`), made again while it isn't a pat: noise at the leaf's pitch beats against its
/// ring, and about once in a hundred strikes the two add up in one short burst (2.6 ms, each alone 4.3 and 5.2) or
/// a noise peak comes early. (After 64 tries, the last — never seen.)
fn pat_alone(what: &mut Rng, leaf: &Leaf) -> Vec<f64> {
    let mut x = pat_made(what, leaf);
    for _ in 0..64 {
        if is_a_pat(&x) {
            break;
        }
        x = pat_made(what, leaf);
    }
    x
}

/// A drop's onset (ms): from where its envelope (0.5 ms RMS) first reaches a tenth of its peak to its peak.
fn onset(x: &[f64]) -> f64 {
    let w = RATE as usize / 2_000;
    let (mut sum, mut env) = (0.0f64, Vec::with_capacity(x.len()));
    for (i, v) in x.iter().enumerate() {
        sum += v * v;
        if i >= w {
            sum -= x[i - w] * x[i - w];
        }
        env.push(sum.max(0.0));
    }
    let (peak_at, peak) =
        env.iter().enumerate().fold((0, 0.0), |b, (i, e)| if *e > b.1 { (i, *e) } else { b });
    // (a tenth of its amplitude: a hundredth of its power)
    let from = env.iter().position(|e| *e >= 0.01 * peak).unwrap_or(0);
    (peak_at - from) as f64 * 1_000.0 / R
}

/// The RMS of `x`'s loudest `w` samples in a row.
fn loudest(x: &[f64], w: usize) -> f64 {
    let (mut sum, mut most) = (0.0f64, 0.0f64);
    for (i, v) in x.iter().enumerate() {
        sum += v * v;
        if i >= w {
            sum -= x[i - w] * x[i - w];
        }
        most = most.max(sum);
    }
    (most / w.min(x.len()).max(1) as f64).sqrt()
}

/// A pat's place, −1 (left) to 1 (right), from `u` in [0, 1): spread wide, more to the sides than the middle.
fn wide(u: f64) -> f64 {
    let p = 2.0 * u - 1.0;
    p.signum() * p.abs().powf(0.6)
}

/// One strike `x` into the ring at `start`, at `pan` (constant power), the quieter side up to 0.6 ms late, its
/// loudest 5 ms on its louder side at `level` — heard where it falls.
fn place(ring: &mut [Vec<f64>; 2], start: u64, x: &[f64], level: f64, pan: f64) {
    place_as(ring, start, x, level / loudest(x, PAT_WINDOW), pan);
}

/// As `place`, but `x` times `a` on its louder side, whatever its level.
fn place_as(ring: &mut [Vec<f64>; 2], start: u64, x: &[f64], a: f64, pan: f64) {
    let theta = (pan + 1.0) * std::f64::consts::FRAC_PI_4;
    let gain = [theta.cos(), theta.sin()];
    let near = usize::from(pan > 0.0);
    let far = gain[1 - near] / gain[near];
    let late = (pan.abs() * 0.0006 * R).round() as u64;
    for (k, v) in x.iter().enumerate() {
        let k = k as u64;
        ring[near][ring_at(start + k)] += v * a;
        ring[1 - near][ring_at(start + k + late)] += v * a * far;
    }
}

/// How thick the drizzle falls: a factor on the drops' rates, an Ornstein–Uhlenbeck walk stepped once a block (a
/// second), smoothed twice — thicker or thinner by up to about a third over twenty to forty seconds, no period.
struct Density {
    rng: Rng,
    x: f64,
    y: [f64; 2],
}

impl Density {
    /// How long it takes to drift back (s), and its smoothing (s).
    const BACK_S: f64 = 20.0;
    const SMOOTH_S: f64 = 6.0;
    /// The most it thickens (×1.3) or thins (÷1.3), as a logarithm.
    const MOST: f64 = 0.262;

    /// From the middle, so a file starts at the drizzle's own measure (from a draw, seed 1's sat at ×0.77 for the
    /// whole of its sample).
    fn new(rng: Rng) -> Density {
        Density { rng, x: 0.0, y: [0.0; 2] }
    }

    /// Its factor after one more second.
    fn step(&mut self) -> f64 {
        let dt = 1.0;
        self.x += -self.x * dt / Density::BACK_S + (2.0 * dt / Density::BACK_S).sqrt() * self.rng.gauss();
        // (smoothed once, a second's step was up to 0.1 of the rate)
        self.y[0] += (self.x - self.y[0]) * dt / Density::SMOOTH_S;
        self.y[1] += (self.y[0] - self.y[1]) * dt / Density::SMOOTH_S;
        (Density::MOST * (1.5 * self.y[1]).tanh()).exp()
    }
}

/// The far hush's gain: the wash and the fine drops, read on the same seed's first seconds as made, brought to
/// `HUSH_LUFS` in the file at the forest's gain — round 3's far "쏴", whatever drops fall over it.
fn hush_gain(kind: Kind, seed: u64) -> f64 {
    let secs = 4;
    let mut rain =
        Rain::with(kind.measure(), seed, Layers { wash: true, fine: true, ..NONE }, (1.0, 1.0), true);
    let (mut l, mut r) = (vec![0.0f32; secs * BLOCK], vec![0.0f32; secs * BLOCK]);
    for (l, r) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
        rain.block(l, r);
    }
    f64::from(10f32.powf((HUSH_LUFS - FOREST_GAIN_DB - stereo_lufs(&l, &r)) / 20.0))
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
    /// The places the near drops fall at, and the places drips fall from.
    places: Vec<Place>,
    spots: Vec<Place>,
    /// The pats under the near drops' own draws: the drops the same with them or without.
    under_what: Rng,
    /// `near_gain`, made once.
    near_gain: f64,
    near_pats: Arrivals,
    mid_pats: Arrivals,
    drips: Arrivals,
    pat_what: Rng,
    /// The near drops' own draws: the mid drops and the drips the same whatever their rate.
    leaf_what: Rng,
    density: Density,
    /// The density held at 1 (while `hush_gain` reads the pats).
    steady: bool,
    /// The far hush's gain on the wash and the fine drops (`hush_gain`); 1 without pats.
    hush: f64,
    /// The forest's drops — near (and the pats under them), mid and drips — against the hush (`--drops`, linear): 1, as
    /// the kind is.
    drops: f64,
    /// Made since the pre-roll: near drops, mid drops, drips (one heard twice, once).
    counts: [u64; 3],
    /// The density's least and most since the pre-roll.
    densities: [f64; 2],
    ring: [Vec<f64>; 2],
    floor: [[Biquad; 2]; 2],
}

impl Rain {
    /// `drops`: `--drops`' dB.
    fn new(kind: Kind, seed: u64, on: Layers, drops: f64) -> Rain {
        let hush = match kind.has_hush() {
            true => hush_gain(kind, seed),
            false => 1.0,
        };
        Rain::with(kind.measure(), seed, on, (hush, 10f64.powf(drops / 20.0)), false)
    }

    fn with(m: Measure, seed: u64, on: Layers, (hush, drops): (f64, f64), steady: bool) -> Rain {
        let s = |stream| Rng::new(seed, stream);
        let (places, spots) = surfaces(seed);
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
            places,
            spots,
            under_what: s(23),
            near_gain: near_gain(),
            near_pats: Arrivals::new(s(16), m.pat_rate),
            mid_pats: Arrivals::new(s(19), m.mid_rate),
            drips: Arrivals::new(s(20), m.drip_rate),
            pat_what: s(17),
            leaf_what: s(22),
            density: Density::new(s(21)),
            steady,
            hush,
            drops,
            counts: [0; 3],
            densities: [f64::INFINITY, 0.0],
            ring: [vec![0.0; RING], vec![0.0; RING]],
            floor: [high_pass(120.0), high_pass(120.0)],
            m,
            on,
            at: 0,
        };
        let mut skip = vec![0.0f32; PREROLL];
        rain.block(&mut skip.clone(), &mut skip);
        (rain.counts, rain.densities) = ([0; 3], [f64::INFINITY, 0.0]);
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
        // (the drizzle thickens and thins in how many drops fall, a step a block)
        let d = if self.steady { 1.0 } else { self.density.step() };
        self.densities = [self.densities[0].min(d), self.densities[1].max(d)];
        self.near_pats.rate = self.m.pat_rate * d;
        self.mid_pats.rate = self.m.mid_rate * d;
        self.drips.rate = self.m.drip_rate * d;
        let db = |db: f64| 10f64.powf(db / 20.0);
        // (each kind of drop drawn whether it sounds or not, so the others are the same with it or without)
        // (near: at the place's own size of bubble, each strike a little apart)
        while let Some(at) = self.near_pats.take(until) {
            let w = &mut self.leaf_what;
            let at_place = self.places[(w.f() * LEAVES as f64) as usize % LEAVES];
            let level = db(-w.range(0.0, NEAR_SPREAD_DB));
            let n = rings(w);
            let x = drop_of(w, at_place.r, n);
            let a = level * swell * self.near_gain * self.drops;
            if self.on.pats {
                place_as(&mut self.ring, at, &x, a, at_place.pan);
            }
            if let Some(under) = self.m.under_db {
                let pat = pat_alone(&mut self.under_what, &at_place.leaf);
                if self.on.pats {
                    let level = loudest(&x, PAT_WINDOW) * a * db(-under);
                    place(&mut self.ring, at, &pat, level, at_place.pan);
                }
            }
            self.counts[0] += 1;
        }
        // (mid: further, so quieter and less wide, tiny bubbles, each of its own size)
        while let Some(at) = self.mid_pats.take(until) {
            let w = &mut self.pat_what;
            let pan = MID_WIDTH * wide(w.f());
            let r = MID_R[0] * (MID_R[1] / MID_R[0]).powf(w.f());
            let level = db(-w.range(self.m.mid_db[0], self.m.mid_db[1]));
            let x = bubble(r, w.range(BUBBLE_ONSET_S[0], BUBBLE_ONSET_S[1]));
            if self.on.mids {
                place(&mut self.ring, at, &x, level * swell * self.drops, pan);
            }
            self.counts[1] += 1;
        }
        while let Some(at) = self.drips.take(until) {
            let w = &mut self.pat_what;
            let spot = self.spots[(w.f() * SPOTS as f64) as usize % SPOTS];
            let level = db(NEAR_DB + w.range(DRIP_DB[0], DRIP_DB[1]));
            let x = drop_of(w, spot.r, 1);
            if self.on.drips {
                place(&mut self.ring, at, &x, level * swell * self.drops, spot.pan);
            }
            self.counts[2] += 1;
            // (some heard twice: the drop, then its fall — one drip)
            if w.f() < DRIP_PAIR {
                let after = (w.range(DRIP_FALL_S[0], DRIP_FALL_S[1]) * R).round() as u64;
                let level = level * db(-w.range(DRIP_FALL_DB[0], DRIP_FALL_DB[1]));
                let x = drop_of(w, spot.r, 1);
                if self.on.drips {
                    place(&mut self.ring, at + after, &x, level * swell * self.drops, spot.pan);
                }
            }
        }
        for (i, (l, r)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            let at = ring_at(self.at + i as u64);
            let swell = self.walk.gain();
            let mut out = [0.0f32; 2];
            for (side, out) in out.iter_mut().enumerate() {
                let mut x = std::mem::take(&mut self.ring[side][at]);
                if self.on.wash {
                    x += self.wash[side].next() * swell * self.m.wash * self.hush;
                }
                if self.on.fine {
                    x += self.fine[side].next(self.at + i as u64) * swell * self.m.fine * self.hush;
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

/// What making a rain came to.
struct Made {
    gain_db: f32,
    /// Samples (both sides) the knee bent.
    kneed: u64,
    peak: f32,
    /// Near drops, mid drops and drips made; the density's least and most.
    counts: [u64; 3],
    densities: [f64; 2],
}

/// The gain that brings the whole of `kind` to its loudness (`Kind::loudness`), from its own first `CALIBRATE_S`
/// seconds; the forest's, fixed.
fn gain(kind: Kind, seed: u64) -> f32 {
    let Some(loudness) = kind.loudness() else { return 10f32.powf(FOREST_GAIN_DB / 20.0) };
    let n = (CALIBRATE_S * RATE) as usize;
    let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
    let mut rain = Rain::new(kind, seed, ALL, 0.0);
    for (l, r) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
        rain.block(l, r);
    }
    10f32.powf((loudness - stereo_lufs(&l, &r)) / 20.0)
}

/// `frames` of `kind` (its `on` layers at the whole's level, its drops `drops` dB against the hush), a block at a time
/// to `each`.
fn made(
    kind: Kind,
    seed: u64,
    frames: u64,
    (on, drops): (Layers, f64),
    mut each: impl FnMut(&[f32], &[f32]) -> Result<()>,
) -> Result<Made> {
    let gain = gain(kind, seed);
    let mut rain = Rain::new(kind, seed, on, drops);
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
    Ok(Made { gain_db: 20.0 * gain.log10(), kneed, peak, counts: rain.counts, densities: rain.densities })
}

/// `frames` of `kind` held whole (the samples, the figures, the tests).
fn render(kind: Kind, seed: u64, frames: u64, on: Layers) -> Result<(Vec<f32>, Vec<f32>, Made)> {
    render_drops(kind, seed, frames, (on, 0.0))
}

/// `render` with its drops `drops` dB against the hush.
fn render_drops(kind: Kind, seed: u64, frames: u64, on: (Layers, f64)) -> Result<(Vec<f32>, Vec<f32>, Made)> {
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

/// The WAV (stereo, 16-bit, `RATE`) of `frames` of `kind`, its drops `drops` dB against the hush, written a block at a
/// time.
fn write(w: &mut impl Write, kind: Kind, seed: u64, frames: u64, drops: f64) -> Result<Made> {
    w.write_all(&head(frames)?).map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity(BLOCK * 4);
    made(kind, seed, frames, (ALL, drops), |l, r| {
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

/// The shortest time (ms) that holds 90 % of a drop's energy: under ~3 ms it's a tick, over it a pat.
fn span90(x: &[f64]) -> f64 {
    let mut cum = vec![0.0f64; x.len() + 1];
    for (i, v) in x.iter().enumerate() {
        cum[i + 1] = cum[i] + v * v;
    }
    let need = 0.9 * cum[x.len()];
    let (mut j, mut best) = (0, usize::MAX);
    for i in 0..x.len() {
        while j < x.len() && cum[j] - cum[i] < need {
            j += 1;
        }
        if cum[j] - cum[i] < need {
            break;
        }
        best = best.min(j - i);
    }
    best as f64 * 1_000.0 / R
}

/// The events in `l` and `r` over a bed of `bed` (its mean power on a side): each a 5 ms window (a millisecond apart)
/// on its louder side, the loudest within 10 ms either side and at least `least` dB over the bed — its dB over it. A
/// drop heard as one, where it falls.
fn events(l: &[f32], r: &[f32], bed: f64, least: f64) -> Vec<f64> {
    let (hop, w) = (RATE as usize / 1_000, PAT_WINDOW);
    let cum = |x: &[f32]| {
        let mut c = vec![0.0f64; x.len() + 1];
        for (i, v) in x.iter().enumerate() {
            c[i + 1] = c[i] + f64::from(*v).powi(2);
        }
        c
    };
    let (cl, cr) = (cum(l), cum(r));
    let p: Vec<f64> = (0..l.len().saturating_sub(w) / hop)
        .map(|k| (cl[k * hop + w] - cl[k * hop]).max(cr[k * hop + w] - cr[k * hop]) / w as f64)
        .collect();
    let floor = bed * 10f64.powf(least / 10.0);
    (0..p.len())
        .filter(|&k| {
            p[k] >= floor
                && p[k.saturating_sub(10)..k].iter().all(|q| *q < p[k])
                && p[k + 1..(k + 11).min(p.len())].iter().all(|q| *q <= p[k])
        })
        .map(|k| 10.0 * (p[k] / bed).log10())
        .collect()
}

/// The mean power of a side (the two sides').
fn power(l: &[f32], r: &[f32]) -> f64 {
    l.iter().chain(r).map(|x| f64::from(*x).powi(2)).sum::<f64>() / (l.len() + r.len()) as f64
}

/// The `q`th quantile of `xs` (sorted here).
fn quantile(xs: &mut [f64], q: f64) -> f64 {
    xs.sort_by(f64::total_cmp);
    match xs.len() {
        0 => f64::NAN,
        n => xs[((n - 1) as f64 * q).round() as usize],
    }
}

/// How wide a drop's spectrum is: the octaves between where 10 % of its energy lies under and where 90 % does. A
/// resonance, a "tok", is narrow (under an octave); a dull burst is wide.
fn octaves80(x: &[f64]) -> f64 {
    let (mut re, mut im) = (vec![0.0f64; FFT], vec![0.0f64; FFT]);
    let n = x.len().min(FFT);
    re[..n].copy_from_slice(&x[..n]);
    fft(&mut re, &mut im);
    let p: Vec<f64> = (0..FFT / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect();
    let all: f64 = p.iter().sum();
    let (mut cum, mut lo, mut hi) = (0.0, 0, 0);
    for (k, v) in p.iter().enumerate() {
        cum += v;
        if cum < 0.1 * all {
            lo = k + 1;
        }
        if cum < 0.9 * all {
            hi = k + 1;
        }
    }
    (hi.max(1) as f64 / lo.max(1) as f64).log2()
}

/// The share of `x`'s energy over `f` (a fourth-order high-pass).
#[cfg(test)]
fn share_over<T: Copy + Into<f64>>(x: &[T], f: f64) -> f64 {
    let mut hp = high_pass(f);
    let (mut over, mut all) = (0.0f64, 0.0f64);
    for &v in x {
        let v: f64 = v.into();
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
    let seed = SEED;
    let mut report = format!(
        "The full video's rain — each kind's {SAMPLE_S} s, seed {seed}: the WAV as made and its AAC (128 kb/s, \
         decoded back) side by side.\n\n"
    );
    // (round 5: the sparse drizzle's drop a bubble, alone and with a faint pat under each near one — how much of the
    // drop is water the ears' choice; round 4's kept beside them, `rain-forest-sparse-r4.mp4`)
    for kind in [Kind::Sparse, Kind::SparsePat] {
        let name = kind.name();
        let wav = dir.join(format!("rain-{name}.wav"));
        let mp4 = dir.join(format!("rain-{name}.mp4"));
        let started = Instant::now();
        let file = std::fs::File::create(&wav).map_err(|e| format!("{}: {e}", wav.display()))?;
        let mut w = std::io::BufWriter::new(file);
        let made = write(&mut w, kind, seed, frames, 0.0)?;
        w.flush().map_err(|e| e.to_string())?;
        let took = started.elapsed().as_secs_f64();
        encode(&ffmpeg, &format!("rain · {}", name.replace('-', " ")), &wav, SAMPLE_S, &mp4)?;
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
        let tp = sound::true_peaks(&l).into_iter().chain(sound::true_peaks(&r)).fold(0.0f32, f32::max);
        report.push_str(&format!(
            "{name}: gain {:+.1} dB {} · {} samples past the knee · peak {:.1} dBFS, true peak {:.1} dBTP · made \
             in {took:.1} s\n",
            made.gain_db,
            match kind.loudness() {
                Some(l) => format!("to about {l} LUFS"),
                None => format!("(round 3's, the far hush at {HUSH_LUFS} LUFS alone)"),
            },
            made.kneed,
            20.0 * made.peak.log10(),
            20.0 * tp.log10(),
        ));
        report.push_str(&heard_one_by_one(kind, seed, frames, &l, &r)?);
        report.push_str(&table(&[("WAV", &figures(&l, &r)), ("AAC 128k", &figures(&al, &ar))]));
        report.push('\n');
    }
    let txt = dir.join("rain.txt");
    std::fs::write(&txt, &report).map_err(|e| format!("{}: {e}", txt.display()))?;
    print!("{report}");
    Ok(())
}

/// The drops that stand out in `kind`'s `frames`, over its far hush `hl`, `hr`: in the drops alone (the pats and the
/// drips), each 5 ms whose power reaches the hush's — the whole there 3 dB or more over the hush. (Counted in the
/// whole, the hush's own swells count too: seed 38's makes 3.8 a second.)
fn standing_out(kind: Kind, seed: u64, frames: u64, hl: &[f32], hr: &[f32]) -> Result<usize> {
    let (l, r, _) = render(kind, seed, frames, Layers { pats: true, mids: true, drips: true, ..NONE })?;
    Ok(events(&l, &r, power(hl, hr), 0.0).len())
}

/// The forest's depth, for the report: how many of each distance fell, how loud each is and how far under the far
/// hush lies, the drops heard over it, the hush itself, and each kind of drop alone against the no-key rules.
fn heard_one_by_one(kind: Kind, seed: u64, frames: u64, l: &[f32], r: &[f32]) -> Result<String> {
    let secs = frames as f64 / R;
    let (hl, hr, _) = render(kind, seed, frames, Layers { wash: true, fine: true, ..NONE })?;
    let (pl, pr, made) = render(kind, seed, frames, Layers { pats: true, ..NONE })?;
    let (ml, mr, _) = render(kind, seed, frames, Layers { mids: true, ..NONE })?;
    let (dl, dr, _) = render(kind, seed, frames, Layers { drips: true, ..NONE })?;
    let [n, m, d] = made.counts;
    let (lp, lh) = (stereo_lufs(&pl, &pr), stereo_lufs(&hl, &hr));
    let hush = power(&hl, &hr);
    let mut alone = events(&pl, &pr, hush, 0.0);
    let whole = events(l, r, hush, 3.0).len();
    let standing = standing_out(kind, seed, frames, &hl, &hr)?;
    let swells = events(&hl, &hr, hush, 3.0).len();
    let short: Vec<f32> = {
        let (a, b) = (sound::short_term(&hl, 3.0, 1.0), sound::short_term(&hr, 3.0, 1.0));
        let p = |x: f32| 10f64.powf(f64::from(x) / 10.0);
        let mut s: Vec<f32> =
            a.iter().zip(&b).map(|(a, b)| (10.0 * (p(*a) + p(*b)).log10()) as f32).collect();
        s.sort_by(f32::total_cmp);
        s
    };
    let mut s = format!(
        "  near drops {n} ({:.1} a second, at {LEAVES} places) · mid drops {m} ({:.1} a second) · drips {d} ({:.2} \
         a second, from {SPOTS} places) · the density from ×{:.2} to ×{:.2}\n  loudness alone (LUFS): the near \
         drops{} {lp:.1} · the mid drops {:.1} · the drips {:.1} · the far hush {lh:.1}, {:.1} dB under the near \
         drops\n  {} events in the near drops alone ({:.1} a second), their loudest 5 ms over the hush: 10 % {:.1} · \
         median {:.1} · 90 % {:.1} · most {:.1} dB\n  drops that stand out — the drops alone at the hush's power or \
         more, each lifting the whole 3 dB over it: {standing} ({:.1} a second) · in the whole, {whole} events 3 dB \
         or more over the hush ({:.1} a second), the hush alone {swells} of its own swells ({:.1} a second)\n  the \
         hush alone: kurtosis {:.2} at 1 kHz, {:.2} at 4 kHz, {:.2} at 8 kHz · 3 s loudness {:.1} / {:.1} / {:.1}\n",
        n as f64 / secs,
        m as f64 / secs,
        d as f64 / secs,
        made.densities[0],
        made.densities[1],
        if kind.measure().under_db.is_some() { " and their pats" } else { "" },
        stereo_lufs(&ml, &mr),
        stereo_lufs(&dl, &dr),
        lp - lh,
        alone.len(),
        alone.len() as f64 / secs,
        quantile(&mut alone, 0.1),
        quantile(&mut alone, 0.5),
        quantile(&mut alone, 0.9),
        quantile(&mut alone, 1.0),
        standing as f64 / secs,
        whole as f64 / secs,
        swells as f64 / secs,
        kurtosis_at(&hl, &hr, 1_000.0),
        kurtosis_at(&hl, &hr, 4_000.0),
        kurtosis_at(&hl, &hr, 8_000.0),
        short.first().copied().unwrap_or(f32::NAN),
        short.get(short.len() / 2).copied().unwrap_or(f32::NAN),
        short.last().copied().unwrap_or(f32::NAN),
    );
    s.push_str(
        "  one alone, 1,000 of each, 10 % / median / 90 %: its spectrum's width (octaves holding 10–90 % of its \
         energy) · how far its second half is centred over its first · its first 0.3 ms's share of its energy (and \
         the most) · two strikes of one place apart in pitch · its pitch\n",
    );
    for (name, kind) in [("near drops", Depth::Near), ("mid drops", Depth::Mid), ("drips", Depth::Drip)] {
        let all = one_by_one(kind, seed, 1_000);
        let three = |f: fn(&Alone) -> f64, scale: f64| {
            let mut v: Vec<f64> = all.iter().map(|a| f(a) * scale).collect();
            let q = |v: &mut [f64], at| quantile(v, at);
            (q(&mut v, 0.1), q(&mut v, 0.5), q(&mut v, 0.9), q(&mut v, 1.0))
        };
        let (w, sl, cl, ap, hz) = (
            three(|a| a.width, 1.0),
            three(|a| a.slide, 100.0),
            three(|a| a.click, 100.0),
            three(|a| a.apart, 100.0),
            three(|a| a.pitch, 1.0),
        );
        s.push_str(&format!(
            "    {name}: {:.2} / {:.2} / {:.2} oct · +{:.1} / +{:.1} / +{:.1} % · {:.2} / {:.2} / {:.2} % (most \
             {:.2}) · {:.1} / {:.1} / {:.1} % · {:.0} / {:.0} / {:.0} Hz\n",
            w.0, w.1, w.2, sl.0, sl.1, sl.2, cl.0, cl.1, cl.2, cl.3, ap.0, ap.1, ap.2, hz.0, hz.1, hz.2,
        ));
    }
    Ok(s)
}

/// The forest's kinds of drop, by how far they fall, read one at a time.
#[derive(Clone, Copy, PartialEq)]
enum Depth {
    Near,
    Mid,
    Drip,
}

/// One drop alone, as the tests and the report read it.
struct Alone {
    /// Octaves holding 10–90 % of its energy.
    width: f64,
    /// How far its second half (by energy) is centred over its first, a share.
    slide: f64,
    /// Its first 0.3 ms's share of its energy.
    click: f64,
    /// It and another strike of its place apart in pitch, a share (each one bubble: the place's, not its stutter);
    /// a mid drop, which has no place, and the next.
    apart: f64,
    /// Its pitch (Hz).
    pitch: f64,
}

/// `n` drops of `kind` alone, as they're made for `seed`: the near ones at each place in turn, ringing as they come,
/// the mid ones each its own, the drips from each spot in turn.
fn one_by_one(kind: Depth, seed: u64, n: usize) -> Vec<Alone> {
    let (places, spots) = surfaces(seed);
    let mut what = Rng::new(seed, 99);
    let onset = |what: &mut Rng| what.range(BUBBLE_ONSET_S[0], BUBBLE_ONSET_S[1]);
    (0..n)
        .map(|i| {
            let (x, a, b) = match kind {
                Depth::Near => {
                    let r = places[i % LEAVES].r;
                    let rings = rings(&mut what);
                    (drop_of(&mut what, r, rings), drop_of(&mut what, r, 1), drop_of(&mut what, r, 1))
                }
                Depth::Mid => {
                    // (each of its own size: two mid drops apart)
                    let mut mid = || {
                        let r = MID_R[0] * (MID_R[1] / MID_R[0]).powf(what.f());
                        bubble(r, onset(&mut what))
                    };
                    let (x, y) = (mid(), mid());
                    (x.clone(), x, y)
                }
                Depth::Drip => {
                    let r = spots[i % SPOTS].r;
                    (drop_of(&mut what, r, 1), drop_of(&mut what, r, 1), drop_of(&mut what, r, 1))
                }
            };
            let (first, second) = halves(&x);
            Alone {
                width: octaves80(&x),
                slide: second / first - 1.0,
                click: click(&x),
                apart: (pitch(&a) / pitch(&b) - 1.0).abs(),
                pitch: pitch(&x),
            }
        })
        .collect()
}

/// `x`'s periods between its upward zero crossings (each to a fraction of a sample): where each falls, its pitch
/// (Hz) and its energy.
fn periods(x: &[f64]) -> Vec<(f64, f64, f64)> {
    let ups: Vec<f64> = (1..x.len())
        .filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0)
        .map(|i| i as f64 - 1.0 + x[i - 1] / (x[i - 1] - x[i]))
        .collect();
    ups.windows(2)
        .map(|w| {
            let e: f64 = x[w[0].ceil() as usize..w[1].ceil() as usize].iter().map(|v| v * v).sum();
            ((w[0] + w[1]) / 2.0, R / (w[1] - w[0]), e)
        })
        .collect()
}

/// The pitch of a tone, its periods weighted by their energy (Hz).
fn pitch(x: &[f64]) -> f64 {
    let p = periods(x);
    p.iter().map(|q| q.1 * q.2).sum::<f64>() / p.iter().map(|q| q.2).sum::<f64>()
}

/// The pitch of a tone's first half of its energy and of its second (Hz).
fn halves(x: &[f64]) -> (f64, f64) {
    let all: f64 = x.iter().map(|v| v * v).sum();
    let (mut cum, mut mid) = (0.0, x.len());
    for (i, v) in x.iter().enumerate() {
        cum += v * v;
        if cum >= all / 2.0 {
            mid = i;
            break;
        }
    }
    let (mut a, mut b) = ([0.0f64; 2], [0.0f64; 2]);
    for (at, f, e) in periods(x) {
        let h = if at < mid as f64 { &mut a } else { &mut b };
        (h[0], h[1]) = (h[0] + f * e, h[1] + e);
    }
    (a[0] / a[1], b[0] / b[1])
}

/// The share of a drop's energy in its first 0.3 ms, from its first sample: a click is energy at once.
fn click(x: &[f64]) -> f64 {
    let all: f64 = x.iter().map(|v| v * v).sum();
    let n = (0.0003 * R).round() as usize;
    x[..n.min(x.len())].iter().map(|v| v * v).sum::<f64>() / all
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
    let (w, h) = (1280usize, 720usize);
    let (ground, ink) = ([15u8, 20, 25], [214u8, 208, 194]);
    let mut px: Vec<[u8; 3]> = vec![ground; w * h];
    let n = words.chars().count();
    // (9 pixels a dot, smaller for words that wouldn't fit with a margin)
    let scale = (w * 9 / 10 / (n * 6).max(1)).clamp(1, 9);
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
        'c' => [0, 0, 0b01110, 0b10000, 0b10000, 0b10001, 0b01110, 0],
        'k' => [0b10000, 0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0],
        'p' => [0, 0, 0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111, 0],
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
        write(&mut b, kind, seed, frames, 0.0).unwrap();
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
        assert!(wav(Kind::Forest, 1, frames) == wav(Kind::Forest, 1, frames), "the forest, the same seed");
        assert_eq!(frames_of("12m").unwrap(), 12 * 60 * u64::from(RATE));
        assert_eq!(frames_of("1m30s").unwrap(), 90 * u64::from(RATE));
        assert_eq!(frames_of("500ms").unwrap(), u64::from(RATE) / 2);
        assert!(frames_of("12").is_err() && frames_of("").is_err() && frames_of("2x").is_err());
    }

    /// `--drops 0` is every kind as it was before `--drops` came: FNV-1a over each kind's WAV (seed 38, across a
    /// block's end), taken from the code as it was. On macOS only: the floats' `sin`, `exp` and `powf` are the
    /// platform's libm, and its figures are this Mac's.
    #[test]
    #[cfg(target_os = "macos")]
    fn drops_at_0_leave_every_kinds_bytes_as_they_were() {
        let frames = u64::from(RATE) * 2 + 4_801;
        let got: Vec<(&str, u64)> = (KINDS.iter())
            .map(|&k| {
                let b = wav(k, SEED, frames);
                (
                    k.name(),
                    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
                    }),
                )
            })
            .collect();
        let was = [
            ("forest", 5_297_035_654_491_544_816),
            ("forest-sparse", 10_244_817_708_142_689_839),
            ("forest-sparse-pat", 6_477_354_380_291_187_604),
            ("forest-thick", 4_318_022_275_886_606_862),
            ("soft", 5_876_012_249_163_684_313),
            ("leaves", 13_838_684_041_028_107_154),
            ("heavy", 15_351_788_193_998_514_218),
        ];
        assert_eq!(got, was, "a kind's bytes at --drops 0 changed");
    }

    /// `--drops` moves the drops and nothing else: the far hush alone the same bytes at −8 and at 0; the drops alone at
    /// −8 dB (and +4) the drops at 0 times that, sample for sample (under the knee); the whole between. A number out of
    /// range or not one is refused, and so is a kind with no hush.
    #[test]
    fn drops_move_the_drops_against_the_hush_and_nothing_else() {
        let frames = u64::from(RATE) * 2;
        let at = |on: Layers, db: f64| render_drops(Kind::SparsePat, SEED, frames, (on, db)).unwrap();
        let hush = Layers { wash: true, fine: true, ..NONE };
        let (h0, h8) = (at(hush, 0.0), at(hush, -8.0));
        assert!(h0.0 == h8.0 && h0.1 == h8.1, "the hush moved");
        let drops = Layers { pats: true, mids: true, drips: true, ..NONE };
        let d0 = at(drops, 0.0);
        let loudest = d0.0.iter().chain(&d0.1).fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(loudest > 0.01 && loudest < KNEE / 2.0, "the drops alone, under the knee at +4: {loudest}");
        for db in [-8.0, 4.0] {
            let g = 10f32.powf(db as f32 / 20.0);
            let d = at(drops, db);
            let off = (d.0.iter().zip(&d0.0).chain(d.1.iter().zip(&d0.1)))
                .fold(0.0f32, |m, (a, b)| m.max((a - b * g).abs()));
            assert!(off < 1e-6, "{db:+} dB: a sample {off} off the drops at 0 times {g}");
        }
        let lufs = |x: &(Vec<f32>, Vec<f32>, Made)| stereo_lufs(&x.0, &x.1);
        let (all0, all8) = (at(ALL, 0.0), at(ALL, -8.0));
        assert!(lufs(&h0) < lufs(&all8) && lufs(&all8) < lufs(&all0) - 1.0, "the whole between");
        for bad in ["25", "-24.5", "loud", ""] {
            assert!(drops_db(bad).is_err(), "{bad:?}");
        }
        assert_eq!(drops_db("-8").unwrap(), -8.0);
        let dir = tarae_testkit::scratch::Scratch::new("xtask-rain-drops");
        let out = dir.join("soft.wav");
        let args: Vec<String> = ["--len", "1s", "--kind", "soft", "--drops", "-8"].map(String::from).into();
        let e = make(out.to_str().unwrap(), &args).unwrap_err();
        assert!(e.contains("soft has no hush"), "{e}");
        assert!(!out.exists(), "nothing written");
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

    /// Wide: the two sides made apart, their correlation low (the forest's in its own test).
    #[test]
    fn left_and_right_are_apart() {
        for kind in round_2() {
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
                // (the forest is dull by design — its pats under 2.5 kHz, its "쏴" small —: its 4 and 8 kHz far under
                // the pats' body, but still there)
                let least = match (kind.measure().pat_rate > 0.0, i) {
                    (true, 6) => -22.0,
                    (true, 7) => -30.0,
                    _ => -14.0,
                };
                assert!(b[i] > least, "its body at {c} Hz — {say}");
            }
            let top = b[6].max(b[7]);
            assert!(top - b[4] < 6.0, "not hiss: the top no more than 6 dB over 1 kHz — {say}");
            low_mids.push((kind, b[2]));
        }
        let at = |k| low_mids.iter().find(|m| m.0 == k).unwrap().1;
        assert!(
            at(Kind::Heavy) > at(Kind::Soft) + 3.0,
            "the downpour fuller at 250 Hz than the soft: {low_mids:.1?}"
        );
    }

    /// The round-2 kinds (the forest's are its own tests: a sparse sound has a high crest and kurtosis by nature).
    fn round_2() -> impl Iterator<Item = Kind> {
        KINDS.into_iter().filter(|k| k.measure().pat_rate == 0.0)
    }

    /// It isn't hiss and it isn't crackle: the wash alone grainless (kurtosis near plain noise's 3), the whole
    /// grainier at 4 kHz where the clear drops are, and no band carried by a few loud grains (the first patter's 28–54
    /// at 8 and 16 kHz).
    #[test]
    fn it_isnt_hiss_nor_crackle() {
        let (l, r) = secs(Kind::Leaves, Layers { wash: true, ..NONE }, 4);
        let wash = kurtosis_at(&l, &r, 4_000.0);
        assert!((2.7..3.3).contains(&wash), "the wash alone at 4 kHz: {wash:.2}");
        for kind in round_2() {
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
            if kind.measure().clear_rate == 0.0 {
                continue;
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
        for kind in round_2() {
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

    /// The drops at each distance, 400 alone as they're made (the near ones at each place in turn, ringing once,
    /// twice or three times as they come).
    fn drops() -> [(Depth, Vec<Alone>); 3] {
        [Depth::Near, Depth::Mid, Depth::Drip].map(|d| (d, one_by_one(d, 1, 400)))
    }

    fn name(d: Depth) -> &'static str {
        match d {
            Depth::Near => "near drop",
            Depth::Mid => "mid drop",
            Depth::Drip => "drip",
        }
    }

    /// A drop is a tone, not a tap: one alone holds 10–90 % of its energy within 0.4 octaves at the median (round
    /// 3's ringing pats 0.48, round 4's bursts 1.35 — "a keyboard").
    #[test]
    fn a_drop_is_a_tone() {
        for (d, all) in drops() {
            let mut w: Vec<f64> = all.iter().map(|a| a.width).collect();
            let median = quantile(&mut w, 0.5);
            assert!(median < 0.4, "a {} {median:.2} octaves wide at the median", name(d));
        }
    }

    /// It slides up as it dies: every one's second half of its energy centred at least 2 % above its first.
    #[test]
    fn a_drop_slides_up() {
        for (d, all) in drops() {
            for (i, a) in all.iter().enumerate() {
                assert!(
                    a.slide >= 0.02,
                    "{} {i}: its second half {:+.1} % on its first",
                    name(d),
                    a.slide * 100.0
                );
            }
        }
    }

    /// It doesn't click: every one's first 0.3 ms holds under 2 % of its energy.
    #[test]
    fn a_drop_doesnt_click() {
        for (d, all) in drops() {
            for (i, a) in all.iter().enumerate() {
                assert!(a.click < 0.02, "{} {i}: {:.2} % in its first 0.3 ms", name(d), a.click * 100.0);
            }
        }
    }

    /// A place isn't a note: two strikes of one apart in pitch by more than 2 % at the median — a fixed pitch is a
    /// xylophone. And the pitches the model gives (3 / r, risen 5 % by its energy's middle): near drops 1.2–4.7 kHz,
    /// mid ones 3–5.5, drips 430–1,300 Hz (each place's ±15 %).
    #[test]
    fn a_place_isnt_a_note() {
        for (d, all) in drops() {
            let mut apart: Vec<f64> = all.iter().map(|a| a.apart).collect();
            let median = quantile(&mut apart, 0.5);
            assert!(median > 0.02, "two strikes of a {}'s place {:.1} % apart", name(d), median * 100.0);
            let range = match d {
                Depth::Near => 1_200.0..=4_700.0,
                Depth::Mid => 3_000.0..=5_500.0,
                Depth::Drip => 430.0..=1_300.0,
            };
            if d != Depth::Near {
                // (a near drop's stutter rings higher, so its pitch with it is checked on the place's alone below)
                for a in &all {
                    assert!(range.contains(&a.pitch), "a {} at {:.0} Hz", name(d), a.pitch);
                }
            }
        }
        let (places, _) = surfaces(1);
        let mut what = Rng::new(1, 5);
        for p in &places {
            let f = pitch(&drop_of(&mut what, p.r, 1));
            assert!((1_200.0..=4_700.0).contains(&f), "a near drop at {f:.0} Hz");
        }
    }

    /// The places stay where they are: sixteen, each its own place across the picture and its own size of bubble,
    /// across `NEAR_R`; their leaves round 4's; four places drips fall from, to the sides, their bubbles in `DRIP_R`;
    /// the same for a seed.
    #[test]
    fn the_near_drops_fall_at_places_that_stay() {
        let (places, spots) = surfaces(1);
        assert_eq!((places.len(), spots.len()), (LEAVES, SPOTS));
        let mut pans: Vec<f64> = places.iter().map(|l| l.pan).collect();
        pans.sort_by(f64::total_cmp);
        assert!(pans.windows(2).all(|p| p[1] - p[0] > 0.02), "each its own place: {pans:.2?}");
        assert!(pans[0] < -0.8 && pans[LEAVES - 1] > 0.8, "across the picture: {pans:.2?}");
        let mut rs: Vec<f64> = places.iter().map(|p| p.r).collect();
        rs.sort_by(f64::total_cmp);
        assert!(rs.windows(2).all(|r| r[1] / r[0] > 1.02), "each its own size: {rs:.5?}");
        assert!(rs[0] < 0.00085 && rs[LEAVES - 1] > 0.0021, "across NEAR_R: {rs:.5?}");
        for p in &places {
            assert!((800.0..=3_000.0).contains(&p.leaf.pitch) && (0.008..=0.025).contains(&p.leaf.len_s));
        }
        for s in &spots {
            assert!(s.pan.abs() >= 0.5 && (DRIP_R[0]..=DRIP_R[1]).contains(&s.r));
        }
        let again = surfaces(1).0;
        assert!(again.iter().zip(&places).all(|(a, b)| a.r == b.r && a.pan == b.pan));
    }

    /// Depth, over the samples' 25 s: few drops stand out — lifting the whole 3 dB or more over the far hush, from
    /// whichever layer (`standing_out`): 4–6.5 a second in `forest`, 2.5–3.5 in `forest-sparse` (round 3's 20) —, the
    /// near drops 2–3 a second (1–2), drips 0.3–1; the far hush alone where round 3 had it in the file, `HUSH_LUFS`
    /// (±1) — faint, not gone. The pat under each near drop changes neither the drops nor how many stand out.
    #[test]
    fn the_forest_has_depth() {
        let frames = u64::from(SAMPLE_S * RATE);
        let a_second = |n: usize| n as f64 / f64::from(SAMPLE_S);
        for (kind, near, out) in [
            (Kind::Forest, 2.0..=3.0, 4.0..=6.5),
            (Kind::Sparse, 1.0..=2.0, 2.5..=3.5),
            (Kind::SparsePat, 1.0..=2.0, 2.5..=3.5),
        ] {
            let made = render(kind, SEED, frames, Layers { pats: true, drips: true, ..NONE }).unwrap().2;
            let (hl, hr, _) = render(kind, SEED, frames, Layers { wash: true, fine: true, ..NONE }).unwrap();
            let [n, _, d] = made.counts.map(|c| a_second(c as usize));
            assert!(near.contains(&n), "{kind:?}: {n:.1} near drops a second");
            assert!((0.3..=1.0).contains(&d), "{kind:?}: {d:.2} drips a second");
            let standing = a_second(standing_out(kind, SEED, frames, &hl, &hr).unwrap());
            assert!(out.contains(&standing), "{kind:?}: {standing:.1} drops a second stand out");
            let hush = stereo_lufs(&hl, &hr);
            assert!((hush - HUSH_LUFS).abs() <= 1.0, "{kind:?}: the far hush at {hush:.1} LUFS");
        }
    }

    /// The bubble alone and with a pat under it are the same drops: with the pats left out, the same bytes; the pat
    /// `PAT_UNDER_DB` under its bubble, so its own loudness well under the drops'.
    #[test]
    fn the_pat_under_a_drop_is_faint_and_changes_nothing_else() {
        let frames = u64::from(6 * RATE);
        let rest = Layers { pats: false, ..ALL };
        let (a, b) = (
            render(Kind::Sparse, 1, frames, rest).unwrap(),
            render(Kind::SparsePat, 1, frames, rest).unwrap(),
        );
        assert!(a.0 == b.0 && a.1 == b.1, "the rest the same with the pats or without");
        let near = Layers { pats: true, ..NONE };
        let (bl, br, _) = render(Kind::Sparse, 1, frames, near).unwrap();
        let (pl, pr, _) = render(Kind::SparsePat, 1, frames, near).unwrap();
        let pat: (Vec<f32>, Vec<f32>) = (
            pl.iter().zip(&bl).map(|(p, b)| p - b).collect(),
            pr.iter().zip(&br).map(|(p, b)| p - b).collect(),
        );
        let under = stereo_lufs(&bl, &br) - stereo_lufs(&pat.0, &pat.1);
        assert!((8.0..=16.0).contains(&under), "the pats {under:.1} LU under the bubbles");
    }

    /// The drizzle thickens and thins in how many drops fall: over ten minutes by up to about a third either way and
    /// never more, slowly, and back and forth — a swing every twenty to forty seconds or so.
    #[test]
    fn the_drizzle_thickens_and_thins() {
        let mut d = Density::new(Rng::new(1, 21));
        let f: Vec<f64> = (0..600).map(|_| d.step()).collect();
        let (least, most) =
            (f.iter().copied().fold(f64::INFINITY, f64::min), f.iter().copied().fold(0.0, f64::max));
        assert!(least <= 0.85 && most >= 1.18, "from ×{least:.2} to ×{most:.2}");
        assert!(least >= 1.0 / 1.31 && most <= 1.31, "from ×{least:.2} to ×{most:.2}");
        // (slow: still much the same ten seconds on, a minute on not)
        let lagged = |lag: usize| {
            let g: Vec<f64> = f.iter().map(|x| x.ln()).collect();
            let m = g.iter().sum::<f64>() / g.len() as f64;
            let var: f64 = g.iter().map(|x| (x - m).powi(2)).sum();
            g.iter().zip(&g[lag..]).map(|(a, b)| (a - m) * (b - m)).sum::<f64>() / var
        };
        let (ten, minute) = (lagged(10), lagged(60));
        assert!(ten > 0.6 && minute < 0.4, "its autocorrelation {ten:.2} at 10 s, {minute:.2} at 60 s");
        let swings = f.windows(2).filter(|w| (w[0] - 1.0) * (w[1] - 1.0) < 0.0).count();
        assert!((8..=60).contains(&swings), "{swings} crossings in ten minutes");
    }

    /// The far hush alone is steady and near plain noise: kurtosis near 3, its 3 s loudness within about ±1.5 LU.
    #[test]
    fn the_far_hush_is_steady_and_plain() {
        let (l, r) = secs(Kind::Forest, Layers { wash: true, fine: true, ..NONE }, 12);
        for c in [1_000.0, 4_000.0, 8_000.0] {
            let k = kurtosis_at(&l, &r, c);
            assert!((2.7..3.6).contains(&k), "the hush at {c} Hz: {k:.2}");
        }
        let short = sound::short_term(&l, 3.0, 1.0);
        let (lo, hi) = (
            short.iter().copied().fold(f32::INFINITY, f32::min),
            short.iter().copied().fold(f32::MIN, f32::max),
        );
        assert!(hi - lo <= 3.0, "its 3 s loudness from {lo:.1} to {hi:.1}");
    }

    /// The forest is wide, whole in mono and has no period: left and right mostly apart (a drop at one place reaches
    /// both ears, so not as apart as noise made for each side: under 0.3), the mono sum losing little, the envelope's
    /// autocorrelation past 50 ms low.
    #[test]
    fn the_forest_is_wide_whole_and_without_a_period() {
        for kind in [Kind::Forest, Kind::Sparse, Kind::Thick] {
            let (l, r) = secs(kind, ALL, 12);
            let f = figures(&l, &r);
            assert!(f.corr.abs() < 0.3, "{kind:?}: correlation {:.3}", f.corr);
            assert!(f.mono_db > -0.5, "{kind:?}: the mono sum {:+.2} dB", f.mono_db);
            assert!(f.period.0 < 0.2, "{kind:?}: {:.3} at {:.0} ms", f.period.0, f.period.1);
        }
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
