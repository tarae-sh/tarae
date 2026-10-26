//! The sound, recorded: a keyboard's real strokes, each its press and its release as they were struck, played in the
//! audio callback (`synth.rs` hands it every sound but the page settling, which stays synthesized). The strokes are
//! raw 16-bit PCM embedded whole (`recorded.rs`, made by `cargo xtask sound make` — no decoder or parser here: the
//! bytes are tarae's own, cut once as a dev step). Varied as the synth varies: which stroke (a key's place on the
//! board, from its seed: the same place the same stroke, never one twice in a row), its pitch ±2 %, its level ±10 %,
//! the release a little early or late. Pure — std only, no allocation once made: `cargo xtask sound` includes it.

use super::said::{Kind, Said};
use super::synth::centred;

/// One stroke: its press and its release, each (first sample, samples) in the bank's PCM.
#[derive(Clone, Copy, Debug)]
pub struct Stroke {
    pub press: (u32, u32),
    pub release: (u32, u32),
    /// Ms from the press's start to the release's, as it was struck.
    pub hold: f32,
}

/// A keyboard's strokes.
#[derive(Debug)]
pub struct Bank {
    pub rate: f32,
    /// 16-bit little-endian, mono.
    pub pcm: &'static [u8],
    pub letters: &'static [Stroke],
    /// The space bar's (a big key's) — none where the recording had none told apart: a letter, lower, stands in.
    pub big: &'static [Stroke],
    /// What puts a stroke at the synth's loudness.
    pub level: f32,
}

impl Bank {
    fn at(&self, i: u32) -> f32 {
        let i = i as usize * 2;
        match self.pcm.get(i..i + 2) {
            Some(b) => i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0,
            None => 0.0,
        }
    }
}

/// A slice as it plays: `wait` samples, then `len` of the bank's from `from`, read every `step` (linear between).
#[derive(Clone, Copy, Default)]
struct Slice {
    from: u32,
    len: u32,
    pos: f32,
    step: f32,
    gain: f32,
    wait: u32,
}

impl Slice {
    fn done(&self) -> bool {
        self.len == 0 || self.pos + 1.0 >= self.len as f32
    }
}

/// How a kind changes a stroke: (its pool is the big keys', pitch ×, level ×).
fn how(kind: Kind) -> (bool, f32, f32) {
    match kind {
        Kind::Key | Kind::Settle => (false, 1.0, 1.0),
        Kind::Space => (true, 1.0, 1.0),
        Kind::Return => (true, 0.92, 1.12),
        Kind::Erase => (false, 0.93, 1.0),
    }
}

/// A big key with none recorded: a letter this much lower.
const BIG_STANDS_IN: f32 = 0.8;

/// At most this many slices at once (a stroke is two) — a new one takes the oldest's place.
pub const SLICES: usize = 32;

pub struct Sampler {
    rate: f32,
    bank: &'static Bank,
    slices: [Slice; SLICES],
    next: usize,
    /// The stroke each pool (the letters, the big keys) played last.
    last: [usize; 2],
}

impl Sampler {
    pub fn new(bank: &'static Bank, rate: f32) -> Sampler {
        Sampler { rate, bank, slices: [Slice::default(); SLICES], next: 0, last: [usize::MAX; 2] }
    }

    /// A stroke for `s` (the settle isn't one: nothing).
    pub fn play(&mut self, s: &Said) {
        let b = self.bank;
        if s.kind == Kind::Settle || b.letters.is_empty() {
            return;
        }
        let (big, mut pitch, size) = how(s.kind);
        // (the pool it plays from: a big key with none recorded takes a letter's stroke, lower)
        let from_big = big && !b.big.is_empty();
        if big && !from_big {
            pitch *= BIG_STANDS_IN;
        }
        let pool = if from_big { b.big } else { b.letters };
        let mut rng = s.seed | 1;
        let r: [f32; 4] = std::array::from_fn(|_| centred(&mut rng));
        // (its place on the board, from its seed, as the synth's: the same place the same stroke — never the stroke
        // its pool played last, whichever kind played it)
        let which = from_big as usize;
        let mut i = ((s.seed >> 3) % 47) as usize % pool.len();
        if i == self.last[which] && pool.len() > 1 {
            i = (i + 1) % pool.len();
        }
        self.last[which] = i;
        let st = pool[i];
        let step = b.rate / self.rate * pitch * (1.0 + 0.04 * r[0]);
        let gain = b.level * s.gain * size * (1.0 + 0.2 * r[1]);
        let at = s.delay.max(0.0) * self.rate;
        // (the release a little early or late; slowed with the pitch, as a bigger key is)
        let hold = st.hold * (1.0 + 0.2 * r[2]) / pitch / 1000.0 * self.rate;
        self.add(Slice { from: st.press.0, len: st.press.1, pos: 0.0, step, gain, wait: at as u32 });
        let up = gain * (1.0 + 0.15 * r[3]);
        self.add(Slice {
            from: st.release.0,
            len: st.release.1,
            pos: 0.0,
            step,
            gain: up,
            wait: (at + hold) as u32,
        });
    }

    fn add(&mut self, s: Slice) {
        let at = self.slices.iter().position(Slice::done).unwrap_or(self.next);
        self.next = (at + 1) % SLICES;
        self.slices[at] = s;
    }

    /// The next `out.len()` samples added into `out`.
    pub fn add_to(&mut self, out: &mut [f32]) {
        let b = self.bank;
        for s in self.slices.iter_mut().filter(|s| !s.done()) {
            for o in out.iter_mut() {
                if s.wait > 0 {
                    s.wait -= 1;
                    continue;
                }
                if s.done() {
                    break;
                }
                let i = s.pos as u32;
                let f = s.pos - i as f32;
                let (x0, x1) = (b.at(s.from + i), b.at(s.from + i + 1));
                *o += (x0 + (x1 - x0) * f) * s.gain;
                s.pos += s.step;
            }
        }
    }

    /// Whether a stroke is still sounding or waiting.
    pub fn busy(&self) -> bool {
        self.slices.iter().any(|s| !s.done())
    }
}

#[cfg(test)]
mod tests {
    use super::super::recorded::BANK;
    use super::*;

    fn said(kind: Kind, seed: u32) -> Said {
        Said { kind, gain: 1.0, seed, delay: 0.01 }
    }

    fn played(bank: &'static Bank, s: Said) -> Vec<f32> {
        let mut sm = Sampler::new(bank, 48_000.0);
        sm.play(&s);
        let mut all = Vec::new();
        while sm.busy() {
            let mut buf = [0.0f32; 512];
            sm.add_to(&mut buf);
            all.extend_from_slice(&buf);
            assert!(all.len() < 48_000, "{:?} still sounding after a second", s.kind);
        }
        all
    }

    /// How loud a stroke may be at the default volume (`editor.sound-volume` 30), dBFS: under the ceiling, over the
    /// floor — fixed numbers, not the code's factors (trollarch2's bank, measured over these seeds 2026-10-27: −17.9
    /// dBFS at most, −26.2 at least; the synth's keys were −31.4).
    const CEILING_DB: f32 = -12.0;
    const FLOOR_DB: f32 = -45.0;

    #[test]
    fn every_stroke_sounds_within_bounds_and_ends() {
        assert!(!BANK.letters.is_empty(), "no strokes recorded");
        let master = super::super::said::master(30);
        for kind in [Kind::Key, Kind::Space, Kind::Return, Kind::Erase] {
            for seed in 0..64 {
                let x = played(&BANK, said(kind, seed * 8 + 1));
                let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs())) * master;
                let db = 20.0 * peak.max(1e-9).log10();
                assert!(x.iter().all(|v| v.is_finite()), "{kind:?} {seed}: not finite");
                assert!(db <= CEILING_DB, "{kind:?} {seed}: {db:.1} dBFS at volume 30, past {CEILING_DB}");
                assert!(db >= FLOOR_DB, "{kind:?} {seed}: {db:.1} dBFS at volume 30, under {FLOOR_DB}");
                // (its delay is silence)
                assert!(x[..400].iter().all(|v| *v == 0.0), "{kind:?} sounded before its delay");
            }
        }
    }

    #[test]
    fn every_stroke_in_the_bank_is_inside_its_pcm() {
        let samples = BANK.pcm.len() as u32 / 2;
        for s in BANK.letters.iter().chain(BANK.big) {
            for (from, len) in [s.press, s.release] {
                assert!(len > 0 && from + len <= samples, "{s:?} past the PCM's {samples} samples");
            }
            assert!(s.hold > 0.0);
        }
    }

    #[test]
    fn the_same_seed_the_same_and_never_one_stroke_twice_in_a_row() {
        assert_eq!(played(&BANK, said(Kind::Key, 9)), played(&BANK, said(Kind::Key, 9)));
        assert_ne!(played(&BANK, said(Kind::Key, 9)), played(&BANK, said(Kind::Key, 10)));
        // (two seeds of one place on the board: the second takes the next stroke)
        let mut sm = Sampler::new(&BANK, 48_000.0);
        sm.play(&said(Kind::Key, 8));
        let first = sm.last[0];
        sm.play(&said(Kind::Key, 8 + 47 * 8));
        assert_ne!(sm.last[0], first, "the same stroke twice in a row");
    }

    static NO_BIG: Bank =
        Bank { rate: BANK.rate, pcm: BANK.pcm, letters: BANK.letters, big: &[], level: BANK.level };

    /// A bank with no big key recorded: the space bar is a letter, lower; the settle is never a stroke.
    #[test]
    fn a_letter_stands_in_for_a_big_key_and_the_settle_is_the_synths() {
        assert!(played(&NO_BIG, said(Kind::Space, 1)).iter().any(|v| *v != 0.0));
        assert!(played(&BANK, said(Kind::Settle, 1)).is_empty());
    }

    /// A big key standing in on a letter's stroke is that pool's next stroke too: a key, then the space bar or return
    /// of the same place on the board, never the same stroke twice in a row (they were tracked apart).
    #[test]
    fn a_stand_in_never_repeats_the_letter_just_played() {
        assert!(NO_BIG.letters.len() > 1);
        for big in [Kind::Space, Kind::Return] {
            for seed in (0..47).map(|p| p * 8) {
                let mut sm = Sampler::new(&NO_BIG, 48_000.0);
                sm.play(&said(Kind::Key, seed));
                // (one place on the board: `(seed >> 3) % 47` the same)
                sm.play(&said(big, seed + 47 * 8));
                // (each stroke two slices in order: the key's press first, the big key's third)
                let (key, then) = (sm.slices[0].from, sm.slices[2].from);
                assert_ne!(key, then, "{big:?} after a key at place {}: one stroke twice", seed >> 3);
            }
        }
    }
}
