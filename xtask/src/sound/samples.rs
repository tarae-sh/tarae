//! `cargo xtask sound make WAV NAME [--letters N]` — a recording of typing made into tarae's strokes: the WAV read (a
//! dev step: tarae itself never parses anyone's audio), its events found (onsets within `JOIN` ms one event — one press
//! or one release), its clean strokes taken by order (two events `HOLD` apart, nothing within `QUIET_DB` of them
//! around — a press and its release can't be told apart by their sound alone, measured 2026-10-26), the cleanest and
//! most distinct picked (a big key's apart where one rings as one), cut from the signal above 100 Hz, faded,
//! normalised, at `RATE` — `.sb/samples/banks/NAME.{pcm,txt,png}`. `install` makes one tarae's.

use std::f32::consts::{PI, TAU};
use std::path::Path;

use crate::Result;

/// A recording, mono.
pub struct Wav {
    pub rate: u32,
    pub x: Vec<f32>,
}

/// A RIFF WAVE of 16-bit PCM or 32-bit float, its channels averaged.
pub fn read(path: &Path) -> Result<Wav> {
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(format!("{}: not a RIFF WAVE", path.display()));
    }
    let u16le = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32le = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let (mut fmt, mut data) = (None, None);
    let mut i = 12;
    while i + 8 <= b.len() {
        let size = u32le(i + 4) as usize;
        let body = (i + 8)..(i + 8 + size).min(b.len());
        match &b[i..i + 4] {
            b"fmt " => fmt = Some((u16le(i + 8), u16le(i + 10) as usize, u32le(i + 12), u16le(i + 22))),
            b"data" => data = Some(body),
            _ => {}
        }
        i += 8 + size + size % 2;
    }
    let (Some((format, ch, rate, bits)), Some(data)) = (fmt, data) else {
        return Err(format!("{}: no fmt or data chunk", path.display()));
    };
    let d = &b[data];
    let samples: Vec<f32> = match (format, bits) {
        (3, 32) => d.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect(),
        (1, 16) => d.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c) as f32 / 32768.0).collect(),
        _ => return Err(format!("{}: format {format}, {bits} bits — float32 or PCM16 only", path.display())),
    };
    let ch = ch.max(1);
    let x = samples.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    Ok(Wav { rate, x })
}

/// An RBJ biquad.
#[derive(Clone, Copy)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl Biquad {
    fn make(high: bool, hz: f32, rate: f32) -> Biquad {
        let (s, c) = (TAU * hz / rate).sin_cos();
        let al = s / (2.0 * 0.707);
        let b = match high {
            true => [(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0],
            false => [(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0],
        };
        let a0 = 1.0 + al;
        Biquad { b: b.map(|v| v / a0), a: [-2.0 * c / a0, (1.0 - al) / a0], z: [0.0; 2] }
    }

    fn next(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

/// The envelope's frame (ms): each one's RMS of the signal above 80 Hz, in dB.
const FRAME: f32 = 1.0;

fn envelope(w: &Wav) -> Vec<f32> {
    let n = (w.rate as f32 * FRAME / 1000.0) as usize;
    let mut hp = Biquad::make(true, 80.0, w.rate as f32);
    let y: Vec<f32> = w.x.iter().map(|&v| hp.next(v)).collect();
    y.chunks(n).map(|c| db((c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())).collect()
}

/// Every onset (its sample): a frame (or the next) `rise` dB or more over the mean of the 10 ms before, the largest
/// rise within ±12 frames, above `floor` — to the sample, the first over a fifth of the next 6 frames' peak.
fn onsets(w: &Wav, env: &[f32], rise: f32, floor: f32) -> Vec<usize> {
    const SPREAD: usize = 12;
    let per = (w.rate as f32 * FRAME / 1000.0) as usize;
    let d: Vec<f32> = (0..env.len())
        .map(|i| match i < 13 || i + 1 >= env.len() {
            true => 0.0,
            false => (env[i].max(env[i + 1]) - env[i - 12..i - 2].iter().sum::<f32>() / 10.0).max(0.0),
        })
        .collect();
    let mut out = Vec::new();
    for i in SPREAD..d.len().saturating_sub(SPREAD) {
        if d[i] < rise || env[i].max(env[i + 1]) < floor {
            continue;
        }
        if d[i - SPREAD..=i + SPREAD].iter().enumerate().any(|(k, v)| *v > d[i] || (*v == d[i] && k < SPREAD))
        {
            continue;
        }
        let from = (i - 1) * per;
        let pk = w.x[from..(from + 6 * per).min(w.x.len())].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        out.push((from..from + 2 * per).find(|&s| w.x[s].abs() > pk * 0.2).unwrap_or(i * per));
    }
    out
}

/// An event: onsets within `JOIN` ms of its first (a press's dome and bottom, a release's rattle) — one press or one
/// release, measured over its first `LOOK` ms.
struct Event {
    at: usize,
    peak: f32,
    /// The share of its energy under 400 Hz.
    low: f32,
    centroid: f32,
    /// Ms to fall 20 dB under its peak (the next event cuts it).
    decay: f32,
}

const JOIN: f32 = 30.0;
const LOOK: f32 = 40.0;

fn events(w: &Wav, env: &[f32], onsets: &[usize]) -> Vec<Event> {
    let ms = |s: usize| s as f32 * 1000.0 / w.rate as f32;
    let mut starts = Vec::new();
    let mut k = 0;
    while k < onsets.len() {
        starts.push(onsets[k]);
        let first = onsets[k];
        while k < onsets.len() && ms(onsets[k] - first) < JOIN {
            k += 1;
        }
    }
    let per = (w.rate as f32 * FRAME / 1000.0) as usize;
    (0..starts.len())
        .map(|i| {
            let at = starts[i];
            let next = starts.get(i + 1).copied().unwrap_or(w.x.len());
            let end = next.min(at + (w.rate as f32 * LOOK / 1000.0) as usize).min(w.x.len());
            let (pi, pk) = w.x[at..end]
                .iter()
                .enumerate()
                .fold((0, 0.0f32), |m, (i, v)| if v.abs() > m.1 { (i, v.abs()) } else { m });
            let (low, centroid) = spectrum(&w.x, at, (w.rate as f32 * LOOK / 1000.0) as usize, next, w.rate);
            let f0 = (at + pi) / per;
            let fe = (next / per).min(env.len()).min(f0 + 300);
            let top = env[f0.min(env.len() - 1)];
            let decay = ((f0..fe).find(|&f| env[f] < top - 20.0).unwrap_or(fe) - f0) as f32 * FRAME;
            Event { at, peak: db(pk), low, centroid, decay }
        })
        .collect()
}

/// The share under 400 Hz and the centroid of `x[at..]`'s next `n` samples (cut at `stop`), half-Hann windowed.
fn spectrum(x: &[f32], at: usize, n: usize, stop: usize, rate: u32) -> (f32, f32) {
    let size = n.next_power_of_two();
    let len = n.min(stop.saturating_sub(at)).min(x.len() - at);
    let mut re = vec![0.0f32; size];
    let mut im = vec![0.0f32; size];
    for i in 0..len {
        re[i] = x[at + i] * (0.5 + 0.5 * (PI * i as f32 / len as f32).cos());
    }
    fft(&mut re, &mut im);
    let (mut low, mut total, mut moment) = (0.0f32, 0.0f32, 0.0f32);
    for k in 1..size / 2 {
        let hz = k as f32 * rate as f32 / size as f32;
        let p = re[k] * re[k] + im[k] * im[k];
        if hz < 400.0 {
            low += p;
        }
        total += p;
        moment += p * hz;
    }
    (low / total.max(1e-20), moment / total.max(1e-20))
}

/// In place, radix 2.
fn fft(re: &mut [f32], im: &mut [f32]) {
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
        let (ws, wc) = (-TAU / len as f32).sin_cos();
        for s in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (s + k, s + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                (re[b], im[b]) = (re[a] - tr, im[a] - ti);
                re[a] += tr;
                im[a] += ti;
                (cr, ci) = (cr * wc - ci * ws, cr * ws + ci * wc);
            }
        }
        len <<= 1;
    }
}

/// A stroke heard clean: two events `HOLD` ms apart, nothing within `QUIET_DB` of them from `CLEAR_BEFORE` ms
/// before the first to `CLEAR_AFTER` ms after the second.
const QUIET_DB: f32 = 12.0;
const CLEAR_BEFORE: f32 = 60.0;
const CLEAR_AFTER: f32 = 90.0;
const HOLD: (f32, f32) = (40.0, 170.0);

fn strokes(evs: &[Event], rate: u32) -> Vec<(usize, usize)> {
    let ms = |a: usize, b: usize| (evs[b].at as f32 - evs[a].at as f32) * 1000.0 / rate as f32;
    let mut out = Vec::new();
    for i in 0..evs.len().saturating_sub(1) {
        let j = i + 1;
        if !(HOLD.0..=HOLD.1).contains(&ms(i, j)) {
            continue;
        }
        let loud = evs[i].peak.max(evs[j].peak);
        let near = |k: usize| evs[k].peak > loud - QUIET_DB;
        let before = (0..i).rev().take_while(|&k| ms(k, i) < CLEAR_BEFORE).any(near);
        let after = (j + 1..evs.len()).take_while(|&k| ms(j, k) < CLEAR_AFTER).any(near);
        if !before && !after {
            out.push((i, j));
        }
    }
    out
}

/// A clean stroke's features, for picking distinct ones: press peak, release peak, press centroid (ln), release
/// centroid (ln), the press's low share, the press's decay, the hold.
fn stroke_features(rate: u32, evs: &[Event], (i, j): (usize, usize)) -> [f32; 7] {
    let (a, b) = (&evs[i], &evs[j]);
    [
        a.peak,
        b.peak,
        a.centroid.max(50.0).ln(),
        b.centroid.max(50.0).ln(),
        a.low,
        a.decay,
        (b.at - a.at) as f32 * 1000.0 / rate as f32,
    ]
}

/// The share of `x[at..at + n]`'s energy in `lo..hi` Hz (band-passed by two biquads).
fn band_share(w: &Wav, at: usize, n: usize, lo: f32, hi: f32) -> f32 {
    let r = w.rate as f32;
    let (mut h, mut l) = (Biquad::make(true, lo, r), Biquad::make(false, hi, r));
    let (mut e, mut all) = (0.0f32, 0.0f32);
    for &v in &w.x[at..(at + n).min(w.x.len())] {
        let y = l.next(h.next(v));
        e += y * y;
        all += v * v;
    }
    e / all.max(1e-20)
}

/// A row a window: `len` s from `pre` s before each start, each on its own peak (the shapes, not the levels), a tick
/// every 50 ms.
fn strips(path: &Path, w: &Wav, starts: &[usize], pre: f32, len: f32) -> Result<()> {
    const W: usize = 1400;
    const RH: usize = 70;
    let rows = starts.len().max(1);
    let mut px = vec![16u8; W * RH * rows * 3];
    let n = (len * w.rate as f32) as usize;
    let back = (pre * w.rate as f32) as usize;
    for (r, &s) in starts.iter().enumerate() {
        let a = s.saturating_sub(back);
        let peak = w.x[a..(s + n).min(w.x.len())].iter().fold(1e-9f32, |m, v| m.max(v.abs()));
        let per = (n / W).max(1);
        let mut put = |x: usize, y: usize, c: [u8; 3]| {
            let i = ((r * RH + y) * W + x) * 3;
            px[i..i + 3].copy_from_slice(&c);
        };
        for x in 0..W {
            let lo = a + x * per;
            let Some(chunk) = w.x.get(lo..(lo + per).min(w.x.len())) else { continue };
            let (mn, mx) = chunk.iter().fold((0.0f32, 0.0f32), |(m, k), v| (m.min(*v), k.max(*v)));
            let y = |v: f32| (RH as f32 / 2.0 - v / peak * (RH as f32 / 2.0 - 2.0)) as usize;
            for yy in y(mx)..=y(mn).min(RH - 1) {
                put(x, yy, [200, 196, 186]);
            }
            put(x, 0, [50, 50, 50]);
        }
        for t in (0..).map(|k| k as f32 * 0.05).take_while(|t| *t < len) {
            for yy in 0..4 {
                put((t / len * W as f32) as usize, yy, [255, 140, 60]);
            }
        }
    }
    crate::shot::save_png(path, W, RH * rows, &px)
}

/// A bank's name as `make` and `install` take it: it becomes a file's name in `.sb/samples/banks` and words in the
/// generated `recorded.rs` — letters, digits, `_` and `-` only, refused and said otherwise.
pub fn bank_name(name: &str) -> Result<&str> {
    match !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        true => Ok(name),
        false => Err(format!("bank name {name:?}: letters, digits, `_` and `-` only")),
    }
}

/// A stroke as cut: (big, press, release, hold ms, where in the recording s).
pub type Cut = (bool, (u32, u32), (u32, u32), f32, f32);

/// A bank as `make` leaves it: its PCM (16-bit LE mono) and its strokes, where each came from.
pub struct Made {
    pub rate: u32,
    pub pcm: Vec<i16>,
    pub strokes: Vec<Cut>,
    pub level: f32,
}

impl Made {
    pub fn txt(&self) -> String {
        let mut s = format!(
            "rate {}\nlevel {}\n# kind press_from press_len release_from release_len hold_ms at_s\n",
            self.rate, self.level
        );
        for (big, p, r, hold, at) in &self.strokes {
            let kind = if *big { "big" } else { "letter" };
            s.push_str(&format!("{kind} {} {} {} {} {hold:.1} {at:.3}\n", p.0, p.1, r.0, r.1));
        }
        s
    }

    pub fn load(dir: &Path, name: &str) -> Result<Made> {
        let txt = dir.join(format!("{name}.txt"));
        let t = std::fs::read_to_string(&txt).map_err(|e| format!("{}: {e}", txt.display()))?;
        let pcm = dir.join(format!("{name}.pcm"));
        let b = std::fs::read(&pcm).map_err(|e| format!("{}: {e}", pcm.display()))?;
        let mut m = Made {
            rate: 0,
            pcm: b.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect(),
            strokes: Vec::new(),
            level: 1.0,
        };
        for line in t.lines().filter(|l| !l.starts_with('#')) {
            let w: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| w.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
            match w.first() {
                Some(&"rate") => m.rate = num(1) as u32,
                Some(&"level") => m.level = num(1),
                Some(k) => m.strokes.push((
                    *k == "big",
                    (num(1) as u32, num(2) as u32),
                    (num(3) as u32, num(4) as u32),
                    num(5),
                    num(6),
                )),
                None => {}
            }
        }
        Ok(m)
    }
}

/// The signal above 100 Hz (24 dB an octave): a desk's or stand's low ring and a hand's thud out.
fn high_passed(w: &Wav) -> Vec<f32> {
    let r = w.rate as f32;
    let (mut a, mut b) = (Biquad::make(true, 100.0, r), Biquad::make(true, 100.0, r));
    w.x.iter().map(|&v| b.next(a.next(v))).collect()
}

/// Where an event truly strikes: its first sample at a quarter of its peak (within `JOIN` + 10 ms) — a faint blip
/// its onset was found on isn't the key.
fn strike(x: &[f32], at: usize, rate: u32) -> usize {
    let n = ((JOIN + 10.0) * rate as f32 / 1000.0) as usize;
    let s = &x[at..(at + n).min(x.len())];
    let pk = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    at + s.iter().position(|v| v.abs() >= pk * 0.25).unwrap_or(0)
}

/// The tail is cut where it has fallen this far under its peak: the room's noise is ~47 dB under the strokes.
const TAIL_DB: f32 = 36.0;

/// A slice of `x` from `from` to `to`: its tail cut where it has fallen `TAIL_DB` under its peak (+3 ms), faded in
/// over 1 ms and out over 8.
fn slice(x: &[f32], from: usize, to: usize, rate: u32) -> Vec<f32> {
    let ms = |m: f32| (m * rate as f32 / 1000.0) as usize;
    let mut s: Vec<f32> = x[from..to.min(x.len())].to_vec();
    let pk = s.iter().fold(1e-9f32, |m, v| m.max(v.abs()));
    let last = s.iter().rposition(|v| v.abs() > pk * 10f32.powf(-TAIL_DB / 20.0)).unwrap_or(s.len());
    s.truncate((last + ms(3.0)).min(s.len()));
    let (fin, fout) = (ms(1.0).min(s.len()), ms(8.0).min(s.len()));
    for (i, v) in s.iter_mut().take(fin).enumerate() {
        *v *= 0.5 - 0.5 * (PI * i as f32 / fin as f32).cos();
    }
    for (i, v) in s.iter_mut().rev().take(fout).enumerate() {
        *v *= 0.5 - 0.5 * (PI * i as f32 / fout as f32).cos();
    }
    s
}

/// The rate the strokes are kept at.
pub const RATE: u32 = 32_000;

/// `x` from `from` Hz to `to` (lower): a windowed sinc (Blackman, 32 taps a side) cut at 0.94 of the new half.
fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    use std::f64::consts::{PI, TAU};
    if from == to {
        return x.to_vec();
    }
    const TAPS: i64 = 32;
    let ratio = from as f64 / to as f64;
    let cut = 0.94 / ratio;
    let n = (x.len() as f64 / ratio).floor() as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 * ratio;
            let c = t.floor() as i64;
            let mut acc = 0.0f64;
            for k in c - TAPS..=c + TAPS {
                let Some(&v) = usize::try_from(k).ok().and_then(|k| x.get(k)) else { continue };
                let d = t - k as f64;
                let sinc = if d.abs() < 1e-9 { 1.0 } else { (PI * cut * d).sin() / (PI * cut * d) };
                let wpos = (d / (TAPS as f64 + 1.0) + 1.0) / 2.0;
                let win = match (0.0..=1.0).contains(&wpos) {
                    true => 0.42 - 0.5 * (TAU * wpos).cos() + 0.08 * (2.0 * TAU * wpos).cos(),
                    false => 0.0,
                };
                acc += v as f64 * cut * sinc * win;
            }
            acc as f32
        })
        .collect()
}

fn rms_after(x: &[f32], at: usize, n: usize) -> f32 {
    let s = &x[at..(at + n).min(x.len())];
    (s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32).sqrt()
}

/// `cargo xtask sound make WAV NAME [--letters N]`: the recording's clean strokes, the best and most distinct picked
/// (a big key's apart when one rings as one), cut, normalised — `.sb/samples/banks/NAME.{pcm,txt}`, and a picture
/// of each picked (`NAME.png`).
pub fn make(args: &[String]) -> Result<()> {
    let [wav, name, rest @ ..] = args else {
        return Err("cargo xtask sound make WAV NAME [--letters N]".into());
    };
    let letters: usize = match rest {
        [flag, n] if flag == "--letters" => n.parse().map_err(|_| format!("--letters {n}"))?,
        _ => 10,
    };
    let name = bank_name(name)?;
    let w = read(Path::new(wav))?;
    let rate = w.rate;
    let ms = |m: f32| (m * rate as f32 / 1000.0) as usize;
    let env = envelope(&w);
    let mut sorted = env.clone();
    sorted.sort_by(f32::total_cmp);
    let noise = sorted[sorted.len() / 10];
    let evs = events(&w, &env, &onsets(&w, &env, 10.0, noise + 15.0));
    let ps = strokes(&evs, rate);
    let mut peaks: Vec<f32> = evs.iter().map(|e| e.peak).collect();
    peaks.sort_by(f32::total_cmp);
    let p40 = peaks[peaks.len() * 2 / 5];
    let thud = |e: &Event| band_share(&w, e.at, ms(40.0), 20.0, 80.0) > 0.25;
    let rings = |e: &Event| e.decay >= 20.0 && band_share(&w, e.at, ms(60.0), 150.0, 400.0) >= 0.05;
    // (each clean stroke: how clean — the loudest other event near it, dB under it — and whether it rings)
    let mut cands: Vec<((usize, usize), f32, bool)> = Vec::new();
    let (mut no_hold, mut quiet, mut lopsided, mut thuds) = (0, 0, 0, 0);
    for &(i, j) in &ps {
        let (a, b) = (&evs[i], &evs[j]);
        let hold = (b.at - a.at) as f32 * 1000.0 / rate as f32;
        let diff = b.peak - a.peak;
        if hold < 50.0 {
            no_hold += 1;
        } else if a.peak.min(b.peak) < p40 {
            quiet += 1;
        } else if !(-8.0..=12.0).contains(&diff) {
            lopsided += 1;
        } else if thud(a) || thud(b) {
            thuds += 1;
        } else {
            let loud = a.peak.max(b.peak);
            let near = evs
                .iter()
                .enumerate()
                .filter(|(k, e)| {
                    *k != i && *k != j && e.at + ms(CLEAR_BEFORE) >= a.at && e.at <= b.at + ms(CLEAR_AFTER)
                })
                .map(|(_, e)| e.peak)
                .fold(-120.0f32, f32::max);
            let margin = (loud - near).min(40.0);
            cands.push(((i, j), margin + 0.5 * a.peak.min(b.peak), rings(a)));
        }
    }
    println!(
        "{name}: {} clean strokes — {no_hold} held under 50 ms, {quiet} quiet, {lopsided} lopsided, {thuds} with a \
         thud left out: {} kept, {} ringing as a big key",
        ps.len(),
        cands.len(),
        cands.iter().filter(|c| c.2).count()
    );
    cands.sort_by(|a, b| b.1.total_cmp(&a.1));
    // (the most distinct among the cleanest: each next the farthest from those picked)
    let pick = |pool: Vec<&((usize, usize), f32, bool)>, n: usize| -> Vec<(usize, usize)> {
        let top: Vec<(usize, usize)> = pool.iter().take(4 * n).map(|c| c.0).collect();
        if top.is_empty() {
            return Vec::new();
        }
        let f: Vec<[f32; 7]> = top.iter().map(|p| stroke_features(rate, &evs, *p)).collect();
        let k = f.len() as f32;
        let mean: [f32; 7] = std::array::from_fn(|d| f.iter().map(|v| v[d]).sum::<f32>() / k);
        let sd: [f32; 7] = std::array::from_fn(|d| {
            (f.iter().map(|v| (v[d] - mean[d]).powi(2)).sum::<f32>() / k).sqrt().max(1e-6)
        });
        let z: Vec<[f32; 7]> = f.iter().map(|v| std::array::from_fn(|d| (v[d] - mean[d]) / sd[d])).collect();
        let dist = |a: &[f32; 7], b: &[f32; 7]| a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f32>();
        let mut chosen = vec![0usize];
        while chosen.len() < n.min(top.len()) {
            let next = (0..top.len())
                .filter(|i| !chosen.contains(i))
                .max_by(|&a, &b| {
                    let da = chosen.iter().map(|&c| dist(&z[a], &z[c])).fold(f32::MAX, f32::min);
                    let db = chosen.iter().map(|&c| dist(&z[b], &z[c])).fold(f32::MAX, f32::min);
                    da.total_cmp(&db)
                })
                .unwrap_or(0);
            chosen.push(next);
        }
        chosen.into_iter().map(|i| top[i]).collect()
    };
    let letter_pool: Vec<_> = cands.iter().filter(|c| !c.2).collect();
    let big_pool: Vec<_> = cands.iter().filter(|c| c.2).collect();
    let picked: Vec<(bool, (usize, usize))> = pick(letter_pool, letters)
        .into_iter()
        .map(|p| (false, p))
        .chain(pick(big_pool, 3).into_iter().map(|p| (true, p)))
        .collect();
    // (cut from the high-passed signal: the press up to its release, the release up to what comes next)
    let hp = high_passed(&w);
    let pre = ms(1.5);
    // (big, press, release, hold ms, gain, where in the recording s)
    type Sliced = (bool, Vec<f32>, Vec<f32>, f32, f32, f32);
    let mut cut: Vec<Sliced> = Vec::new();
    for &(big, (i, j)) in &picked {
        let (a, b) = (strike(&hp, evs[i].at, rate), strike(&hp, evs[j].at, rate));
        let press = slice(&hp, a - pre, (b - pre).min(a + ms(60.0)), rate);
        let next = evs.get(j + 1).map_or(hp.len(), |e| e.at - pre).max(b + ms(30.0));
        let release = slice(&hp, b - pre, next.min(b + ms(90.0)), rate);
        // (normalised by the louder side's first 20 ms)
        let loud = rms_after(&hp, a, ms(20.0)).max(rms_after(&hp, b, ms(20.0)));
        let hold = (b - a) as f32 * 1000.0 / rate as f32;
        cut.push((big, press, release, hold, 1.0 / loud.max(1e-6), a as f32 / rate as f32));
    }
    // (the energy above 16 kHz: what `RATE` leaves out)
    let mut all: Vec<f32> = cut.iter().flat_map(|c| c.1.iter().chain(&c.2).map(|v| v * c.4)).collect();
    all.resize(all.len().next_power_of_two(), 0.0);
    let mut im = vec![0.0f32; all.len()];
    fft(&mut all, &mut im);
    let (mut hi, mut tot) = (0.0f64, 0.0f64);
    for k in 1..all.len() / 2 {
        let p = (all[k] * all[k] + im[k] * im[k]) as f64;
        tot += p;
        if k as f32 * rate as f32 / all.len() as f32 > RATE as f32 / 2.0 {
            hi += p;
        }
    }
    let cut: Vec<_> = cut
        .into_iter()
        .map(|(big, p, r, hold, g, at)| {
            (big, resample(&p, rate, RATE), resample(&r, rate, RATE), hold, g, at)
        })
        .collect();
    let top =
        cut.iter().flat_map(|c| c.1.iter().chain(&c.2).map(move |v| (v * c.4).abs())).fold(1e-9f32, f32::max);
    let mut made = Made { rate: RATE, pcm: Vec::new(), strokes: Vec::new(), level: 1.0 };
    for (big, press, release, hold, g, at) in &cut {
        let mut put = |s: &[f32]| {
            let from = made.pcm.len() as u32;
            made.pcm.extend(s.iter().map(|v| (v * g / top * 0.95 * 32767.0).round() as i16));
            (from, s.len() as u32)
        };
        let p = put(press);
        let r = put(release);
        made.strokes.push((*big, p, r, *hold, *at));
    }
    let dir = crate::root().join(".sb/samples/banks");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    made.level = super::level_for(&made)?;
    let pcm: Vec<u8> = made.pcm.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(dir.join(format!("{name}.pcm")), &pcm).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{name}.txt")), made.txt()).map_err(|e| e.to_string())?;
    for (big, p, r, hold, at) in &made.strokes {
        println!(
            "  {:<6} at {at:>7.3} s · press {:>3.0} ms · release {:>3.0} ms · hold {hold:>3.0} ms",
            if *big { "big" } else { "letter" },
            p.1 as f32 * 1000.0 / RATE as f32,
            r.1 as f32 * 1000.0 / RATE as f32
        );
    }
    println!(
        "  {} strokes, {} bytes ({:.1} KB) at {RATE} Hz 16-bit — above {} kHz, left out: {:.2} % of the energy · \
         level {:.3} → {}",
        made.strokes.len(),
        pcm.len(),
        pcm.len() as f32 / 1024.0,
        RATE / 2000,
        hi / tot.max(1e-20) * 100.0,
        made.level,
        dir.join(format!("{name}.pcm")).display()
    );
    let rows: Vec<usize> = picked.iter().map(|(_, (i, _))| evs[*i].at).collect();
    strips(&dir.join(format!("{name}.png")), &w, &rows, 0.01, 0.25)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bank_name_is_letters_digits_underscore_and_dash_only() {
        for ok in ["trollarch2", "my_bank-2", "A"] {
            assert_eq!(bank_name(ok), Ok(ok));
        }
        for bad in ["", "../x", "a/b", "a b", "x\"); evil(", "a.b", "é", "a\nb"] {
            let said = bank_name(bad).unwrap_err();
            assert!(said.contains("letters, digits"), "{bad:?}: {said}");
        }
    }
}
