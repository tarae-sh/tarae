//! The sound, synthesized — no samples, no decoder: the Happy Hacking's Topre, built from how it sounds (modal
//! synthesis). Each contact of a stroke is a soft push (a raised cosine a millisecond or two wide — its width is its
//! softness, so nothing much rings above ~4 kHz) striking a few damped resonators: the dome folding, the slider on
//! the rubber (the body's low thump), the cap and housing's thock, and on the way up the spring driving the slider
//! into the top housing, louder than the way down (`TOPRE`). A key's place on the board (from its seed) moves its
//! modes and level a few per cent, the same place the same; the space bar and return are bigger keys, their wire
//! rattling (`bigger`); the desk and case answer (`Room`). The page settling is one slow, low push (`SETTLE`).
//! Pure — std only, no allocation once made (it runs in the audio callback): `cargo xtask sound` includes it.
//! macOS's for now (`sound.rs`: only its output plays it).

use std::f32::consts::TAU;

use super::said::{Kind, Said};

/// A mode: (Hz, decay ms, level).
pub type M = (f32, f32, f32);

const NO: M = (0.0, 0.0, 0.0);

/// One contact of a stroke: `at` ms into its half of the stroke, a raised-cosine push `width` ms wide at `level`
/// (its area), the modes it rings, and `air` — a breath of low-passed noise with it (the dome letting its air out,
/// the contact's grain).
#[derive(Clone, Copy, Debug)]
pub struct Contact {
    pub at: f32,
    pub width: f32,
    pub level: f32,
    pub air: f32,
    pub modes: [M; 4],
}

/// The desk and the case answering: a few early reflections (ms, level), low-passed at `tone` Hz.
#[derive(Clone, Copy, Debug)]
pub struct Room {
    pub taps: [(f32, f32); 5],
    pub tone: f32,
}

/// A Topre key, as its contacts. Modes below 400 Hz are the body's, the rest the cap's and housing's: a key's place
/// on the board moves each group by up to ±`board`/2, and its level as much.
#[derive(Clone, Copy, Debug)]
pub struct Topre {
    /// The way down: the dome folding, the slider on the rubber, the cap and housing.
    pub down: [Contact; 3],
    /// The way up, `hold` ms after: the slider driven into the top housing, and the case under it.
    pub up: [Contact; 2],
    /// How long a letter is held (ms, shortest and longest).
    pub hold: (f32, f32),
    pub board: f32,
    /// A big key's wire stabilizer: struck a few times as the key bottoms out and comes back.
    pub rattle: Contact,
    pub room: Room,
    pub level: f32,
}

const fn contact(at: f32, width: f32, level: f32, air: f32, modes: [M; 4]) -> Contact {
    Contact { at, width, level, air, modes }
}

/// The Happy Hacking's Topre, the Type-S's way: deeper and softer, a mat under it — the user's choice by ear among
/// three sets, 2026-10-26 (sound-plan "Topre v2"; the others, a plain HHKB on a desk and a brighter one, are kept in
/// its scratch comparison).
pub const TOPRE: Topre = Topre {
    down: [
        // the dome folding: a soft, low onset over a few ms
        contact(0.0, 3.5, 0.6, 0.3, [(420.0, 5.0, 1.0), (980.0, 3.0, 0.4), NO, NO]),
        // the slider on the rubber: the body's thump and the case's low ring
        contact(6.0, 2.0, 1.0, 0.25, [(215.0, 32.0, 1.15), (650.0, 9.0, 0.9), (120.0, 45.0, 0.12), NO]),
        // the cap and housing: the thock's upper modes, softened
        contact(6.3, 0.7, 1.0, 0.1, [(1350.0, 5.0, 0.55), (1600.0, 3.5, 0.25), (800.0, 7.0, 0.16), NO]),
    ],
    up: [
        // the spring driving the slider into the top housing: Topre's own, louder than the way down
        contact(0.0, 0.81, 1.5, 0.3, [(704.0, 9.0, 1.3), (1390.0, 5.0, 0.9), (2068.0, 2.5, 0.25), NO]),
        // the case under it
        contact(0.3, 1.44, 1.2, 0.0, [(225.0, 16.0, 0.6), (558.0, 7.0, 0.2), (117.0, 30.0, 0.06), NO]),
    ],
    hold: (75.0, 125.0),
    board: 0.05,
    rattle: contact(0.0, 0.25, 0.05, 0.02, [(2150.0, 6.0, 0.5), (3050.0, 4.0, 0.35), (1250.0, 5.0, 0.2), NO]),
    // (the case close, the desk farther behind a mat: a comb about flat over the body's 100–300 Hz, quieter and
    // darker than a bare desk's)
    room: Room { taps: [(0.7, 0.108), (1.6, 0.072), (4.3, 0.06), (7.1, 0.042), (11.3, 0.03)], tone: 1800.0 },
    level: 0.16,
};

/// The page settling: one slow push into low modes, under a key's level.
pub const SETTLE: Contact =
    contact(0.0, 5.0, 1.9, 0.04, [(110.0, 55.0, 0.9), (185.0, 35.0, 0.45), (370.0, 14.0, 0.12), NO]);

/// How a kind changes a letter: (pitch ×, decays and widths ×, held longer by ms, level ×, rattle).
fn bigger(kind: Kind) -> (f32, f32, f32, f32, f32) {
    match kind {
        Kind::Key | Kind::Settle => (1.0, 1.0, 0.0, 1.0, 0.0),
        Kind::Space => (0.74, 1.6, 30.0, 1.1, 1.0),
        Kind::Return => (0.84, 1.3, 25.0, 1.2, 0.6),
        Kind::Erase => (0.93, 1.1, 8.0, 1.0, 0.0),
    }
}

/// A two-pole resonator with a zero at DC (a band-pass — a body radiates nothing still): struck by a unit impulse
/// it rings at its level, decaying.
#[derive(Clone, Copy, Default)]
struct Res {
    b: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Res {
    fn new((hz, decay, amp): M, rate: f32) -> Res {
        if amp == 0.0 || hz <= 0.0 {
            return Res::default();
        }
        let w = TAU * hz.min(rate * 0.45) / rate;
        let r = (-1.0 / (decay * rate / 1000.0).max(1.0)).exp();
        // (an impulse through 1 − z⁻² rings at about twice the all-pole's level)
        Res { b: amp / 2.0, a1: 2.0 * r * w.cos(), a2: r * r, ..Res::default() }
    }

    fn next(&mut self, x: f32) -> f32 {
        let y = self.b * (x - self.x2) + self.a1 * self.y1 - self.a2 * self.y2;
        (self.x2, self.x1, self.y2, self.y1) = (self.x1, x, self.y1, y);
        y
    }
}

/// How fast a contact's air dies (ms) and where it's cut (Hz).
const AIR: (f32, f32) = (1.2, 3200.0);

/// A contact as it plays.
#[derive(Clone, Copy, Default)]
struct Hit {
    wait: u32,
    width: u32,
    age: u32,
    left: u32,
    /// The push's level a sample (its area is the contact's level).
    push: f32,
    air: f32,
    air_now: f32,
    air_k: f32,
    lp_k: f32,
    lp: [f32; 2],
    res: [Res; 4],
}

impl Hit {
    /// `c` at `at` ms, its modes' Hz by `tune`, decays and width × `slow`.
    fn new(c: &Contact, at: f32, level: f32, slow: f32, tune: impl Fn(f32) -> f32, rate: f32) -> Hit {
        let ms = |m: f32| (m * rate / 1000.0).max(0.0);
        let width = ms(c.width * slow).max(1.0) as u32;
        let longest = c.modes.iter().filter(|m| m.2 > 0.0).fold(AIR.0, |l, m| l.max(m.1));
        Hit {
            wait: ms(at) as u32,
            width,
            age: 0,
            // (7 decays: -60 dB)
            left: width + ms(7.0 * longest * slow) as u32,
            push: c.level * level / width as f32,
            air: c.air * level,
            air_now: 0.0,
            air_k: (-1.0 / ms(AIR.0).max(1.0)).exp(),
            lp_k: (-TAU * AIR.1 / rate).exp(),
            lp: [0.0; 2],
            res: c.modes.map(|(hz, decay, amp)| Res::new((tune(hz), decay * slow, amp), rate)),
        }
    }

    fn next(&mut self, noise: f32) -> f32 {
        if self.wait > 0 {
            self.wait -= 1;
            return 0.0;
        }
        if self.left == 0 {
            return 0.0;
        }
        self.left -= 1;
        let x = match self.age < self.width {
            true => self.push * (1.0 - (TAU * (self.age as f32 + 0.5) / self.width as f32).cos()),
            false => 0.0,
        };
        // (the air rises over the push's first half, then dies)
        match self.age * 2 < self.width {
            true => self.air_now = self.air * (self.age * 2) as f32 / self.width as f32,
            false => self.air_now *= self.air_k,
        }
        self.age += 1;
        let mut y: f32 = self.res.iter_mut().map(|r| r.next(x)).sum();
        let mut a = noise * self.air_now;
        for lp in &mut self.lp {
            *lp = (1.0 - self.lp_k) * a + self.lp_k * *lp;
            a = *lp;
        }
        y += a;
        y
    }

    fn done(&self) -> bool {
        self.wait == 0 && self.left == 0
    }
}

fn xorshift(r: &mut u32) -> u32 {
    *r ^= *r << 13;
    *r ^= *r >> 17;
    *r ^= *r << 5;
    *r
}

/// A number in -0.5 ‥ 0.5.
fn centred(r: &mut u32) -> f32 {
    (xorshift(r) >> 8) as f32 / (1 << 24) as f32 - 0.5
}

/// A sound as it plays: a stroke's way down, way up and a big key's rattles — or the page settling.
#[derive(Clone, Copy, Default)]
struct Voice {
    hits: [Hit; 10],
    n: usize,
    rng: u32,
}

impl Voice {
    fn new(t: &Topre, s: &Said, rate: f32) -> Voice {
        let mut rng = s.seed | 1;
        let r: [f32; 4] = std::array::from_fn(|_| centred(&mut rng));
        let mut v = Voice { rng: rng | 1, ..Voice::default() };
        let delay = s.delay.max(0.0) * 1000.0;
        if s.kind == Kind::Settle {
            let level = t.level * s.gain * (1.0 + 0.1 * r[1]);
            v.add(Hit::new(&SETTLE, delay, level, 1.0, |hz| hz * (1.0 + 0.04 * r[0]), rate));
            return v;
        }
        // (its place on the board, from its seed: the same place moves its modes and level the same way)
        let mut place = ((s.seed >> 3) % 47 + 1).wrapping_mul(0x9e37_79b9) | 1;
        let p: [f32; 3] = std::array::from_fn(|_| centred(&mut place));
        let (pitch, slow, longer, size, rattle) = bigger(s.kind);
        let body = pitch * (1.0 + t.board * p[0]) * (1.0 + 0.02 * r[0]);
        let thock = pitch * (1.0 + t.board * p[1]) * (1.0 + 0.02 * r[0]);
        let tune = move |hz: f32| hz * if hz < 400.0 { body } else { thock };
        let level = t.level * size * s.gain * (1.0 + t.board * p[2]) * (1.0 + 0.1 * r[1]);
        let hold = t.hold.0 + (t.hold.1 - t.hold.0) * (r[2] + 0.5) + longer;
        // (its bottom a little early or late)
        let late = 1.2 * r[3];
        let mut add =
            |c: &Contact, at: f32, level: f32| v.add(Hit::new(c, delay + at, level, slow, tune, rate));
        for (i, c) in t.down.iter().enumerate() {
            add(c, c.at * slow + if i > 0 { late } else { 0.0 }, level);
        }
        for c in &t.up {
            add(c, hold + c.at, level);
        }
        if rattle > 0.0 {
            let bottom = t.down[1].at * slow + late;
            for (at, l) in [(0.0, 1.0), (2.7, 0.55), (6.1, 0.3)] {
                add(&t.rattle, bottom + at, level * rattle * l);
            }
            for (at, l) in [(0.4, 0.8), (3.6, 0.4)] {
                add(&t.rattle, hold + at, level * rattle * l);
            }
        }
        v
    }

    fn add(&mut self, h: Hit) {
        if self.n < self.hits.len() {
            self.hits[self.n] = h;
            self.n += 1;
        }
    }

    fn next(&mut self) -> f32 {
        let noise = xorshift(&mut self.rng) as f32 / u32::MAX as f32 * 2.0 - 1.0;
        self.hits[..self.n].iter_mut().map(|h| h.next(noise)).sum()
    }

    fn done(&self) -> bool {
        self.hits[..self.n].iter().all(Hit::done)
    }
}

/// The room's memory, in samples: its farthest reflection (≤ ~20 ms at 96 kHz).
const ECHO: usize = 2048;

/// The room as it plays: what was heard, and the reflections read back from it.
struct Echo {
    buf: [f32; ECHO],
    i: usize,
    taps: [(usize, f32); 5],
    lp_k: f32,
    lp: f32,
    /// Samples until what's in it has died away.
    left: usize,
}

impl Echo {
    fn new(room: &Room, rate: f32) -> Echo {
        let taps = room.taps.map(|(ms, g)| (((ms * rate / 1000.0) as usize).clamp(1, ECHO - 1), g));
        Echo { buf: [0.0; ECHO], i: 0, taps, lp_k: (-TAU * room.tone / rate).exp(), lp: 0.0, left: 0 }
    }

    fn next(&mut self, x: f32) -> f32 {
        if x != 0.0 {
            self.left = ECHO + 512;
        } else if self.left == 0 {
            return 0.0;
        } else {
            self.left -= 1;
        }
        self.buf[self.i] = x;
        let e: f32 = self.taps.iter().map(|&(d, g)| g * self.buf[(self.i + ECHO - d) % ECHO]).sum();
        self.i = (self.i + 1) % ECHO;
        self.lp = (1.0 - self.lp_k) * e + self.lp_k * self.lp;
        x + self.lp
    }
}

/// At most this many sounds at once — a new one takes the oldest's place.
pub const VOICES: usize = 16;

pub struct Synth {
    rate: f32,
    topre: Topre,
    voices: [Option<Voice>; VOICES],
    next: usize,
    echo: Echo,
}

impl Synth {
    pub fn new(rate: f32) -> Synth {
        Synth { rate, topre: TOPRE, voices: [None; VOICES], next: 0, echo: Echo::new(&TOPRE.room, rate) }
    }

    /// Played by other numbers (the tests', and `cargo xtask sound`'s comparisons).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn tune(&mut self, t: Topre) {
        self.echo = Echo::new(&t.room, self.rate);
        self.topre = t;
    }

    pub fn play(&mut self, s: &Said) {
        let free = self.voices.iter().position(Option::is_none);
        let at = free.unwrap_or(self.next);
        self.next = (at + 1) % VOICES;
        self.voices[at] = Some(Voice::new(&self.topre, s, self.rate));
    }

    /// The next `out.len()` samples (mono), at `master` (0 ‥ 1), within ±1.
    pub fn render(&mut self, out: &mut [f32], master: f32) {
        out.fill(0.0);
        for slot in &mut self.voices {
            let Some(v) = slot else { continue };
            for s in out.iter_mut() {
                *s += v.next();
            }
            if v.done() {
                *slot = None;
            }
        }
        for s in out {
            *s = (self.echo.next(*s) * master).clamp(-1.0, 1.0);
        }
    }

    /// Whether a sound is still ringing.
    #[cfg_attr(not(test), allow(dead_code))] // (the tests' and `cargo xtask sound`'s)
    pub fn busy(&self) -> bool {
        self.voices.iter().any(Option::is_some) || self.echo.left > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [Kind; 5] = [Kind::Key, Kind::Space, Kind::Return, Kind::Erase, Kind::Settle];

    fn rendered(t: Topre, kind: Kind, seed: u32) -> Vec<f32> {
        let mut s = Synth::new(48_000.0);
        s.tune(t);
        s.play(&Said { kind, gain: 1.0, seed, delay: 0.01 });
        let mut all = Vec::new();
        let mut buf = [0.0f32; 512];
        while s.busy() {
            s.render(&mut buf, 1.0);
            all.extend_from_slice(&buf);
            assert!(all.len() < 48_000, "{kind:?} still ringing after a second");
        }
        all
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    #[test]
    fn every_sound_rings_within_bounds_and_ends() {
        for kind in KINDS {
            let all = rendered(TOPRE, kind, 42);
            assert!(all.iter().all(|x| x.is_finite() && x.abs() <= 1.0), "{kind:?} out of bounds");
            assert!(peak(&all) > 0.01, "{kind:?} silent: {}", peak(&all));
            // (its delay is silence)
            assert!(all[..400].iter().all(|x| *x == 0.0), "{kind:?} sounded before its delay");
        }
    }

    #[test]
    fn the_same_seed_sounds_the_same_and_another_differs() {
        assert_eq!(rendered(TOPRE, Kind::Key, 9), rendered(TOPRE, Kind::Key, 9));
        assert_ne!(rendered(TOPRE, Kind::Key, 9), rendered(TOPRE, Kind::Key, 10));
    }

    /// Topre's own sign: the spring driving the slider into the top housing is louder than the bottom-out.
    #[test]
    fn the_way_up_is_louder_than_the_way_down() {
        let quiet = |c: Contact| Contact { level: 0.0, air: 0.0, ..c };
        for kind in [Kind::Key, Kind::Space, Kind::Return] {
            let down = Topre { up: TOPRE.up.map(quiet), rattle: quiet(TOPRE.rattle), ..TOPRE };
            let up = Topre { down: TOPRE.down.map(quiet), rattle: quiet(TOPRE.rattle), ..TOPRE };
            let (d, u) = (peak(&rendered(down, kind, 5)), peak(&rendered(up, kind, 5)));
            assert!(u > d * 1.2, "{kind:?}: the way up {u} ≤ the way down {d} × 1.2");
        }
    }

    #[test]
    fn silence_once_every_voice_is_done() {
        let mut s = Synth::new(48_000.0);
        let mut buf = [1.0f32; 64];
        s.render(&mut buf, 1.0);
        assert!(!s.busy() && buf.iter().all(|x| *x == 0.0));
    }
}
