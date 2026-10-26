//! `cargo xtask sound [--volume N] [OUT_DIR]` — a burst of real writing as tarae hears it, for tuning by ear:
//! tarae's own `writing::reveal_wake` streamed as claude streams an edit (its first pieces singly ~50 ms apart, then
//! ~190 characters every ~0.64 s — measured, `writing.rs`), shown at the stream's pace in 25 ms steps as the reveal
//! does once its pace has settled, each step's sound decided by tarae's own `typist.rs`, played by its own
//! `synth.rs`, the edit landing unseen at the end — `topre.wav`, at the default volume (`editor.sound-volume` 30)
//! unless `--volume` says; each kind alone as a waveform, `keys.png`. No device: files only.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::Result;

// (tarae's own, pure: what the WAVs hold is what tarae plays)
#[path = "../../src/sound/said.rs"]
mod said;
#[allow(dead_code, reason = "tarae's: not all of it is the WAVs'")]
#[path = "../../src/sound/synth.rs"]
mod synth;
#[allow(dead_code, reason = "tarae's: not all of it is the WAVs'")]
#[path = "../../src/sound/typist.rs"]
mod typist;

use said::{Kind, Said};
use synth::Synth;

const RATE: u32 = 48_000;
const STEP: Duration = Duration::from_millis(25);

pub fn run(args: &[String]) -> Result<()> {
    let mut volume = 30u8;
    let mut out: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--volume" => {
                let v = it.next().ok_or("--volume N (0..=100)")?;
                volume = v.parse().ok().filter(|v| *v <= 100).ok_or(format!("--volume {v}: 0..=100"))?;
            }
            dir => out = Some(PathBuf::from(dir)),
        }
    }
    let out = out.unwrap_or_else(|| crate::root().join(".sb/sound"));
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let text = writing();
    let burst = burst(&text);
    let count = |k: Kind| burst.iter().filter(|(_, s)| s.kind == k).count();
    let length = burst.last().map_or(Duration::ZERO, |(t, _)| *t);
    println!(
        "{} characters of src/writing.rs's reveal_wake over {:.1} s: {} keys, {} space bars, {} returns, {} landing",
        text.chars().count(),
        length.as_secs_f32(),
        count(Kind::Key),
        count(Kind::Space),
        count(Kind::Return),
        count(Kind::Settle)
    );
    let samples = render(&burst, length + Duration::from_millis(600), volume);
    let path = out.join("topre.wav");
    wav(&path, &samples)?;
    let (peak, rms) = levels(&samples);
    // (each kind alone: a key at volume 30 is kept near −31.5 dBFS)
    let alone =
        |kind: Kind| levels(&render(&[(Duration::ZERO, said(kind))], Duration::from_millis(500), volume)).0;
    println!(
        "peak {peak:.1} dBFS · RMS {rms:.1} · alone: a key {:.1} · the space bar {:.1} · return {:.1} · \
         backspace {:.1} · the settle {:.1}  {}",
        alone(Kind::Key),
        alone(Kind::Space),
        alone(Kind::Return),
        alone(Kind::Erase),
        alone(Kind::Settle),
        path.display()
    );
    let path = out.join("keys.png");
    picture(&path, volume)?;
    println!("{} — {} alone as waveforms, 0.3 s each", path.display(), KINDS.map(|(_, n)| n).join(", "));
    Ok(())
}

const KINDS: [(Kind, &str); 5] = [
    (Kind::Key, "a key"),
    (Kind::Space, "the space bar"),
    (Kind::Return, "return"),
    (Kind::Erase, "backspace"),
    (Kind::Settle, "the settle"),
];

fn said(kind: Kind) -> Said {
    Said { kind, gain: 1.0, seed: 1, delay: 0.0 }
}

/// Each kind alone as a waveform (min and max a column), all on one scale, side by side.
fn picture(path: &Path, volume: u8) -> Result<()> {
    const W: usize = 300;
    const H: usize = 140;
    let waves: Vec<Vec<f32>> = KINDS
        .iter()
        .map(|&(kind, _)| render(&[(Duration::ZERO, said(kind))], Duration::from_millis(300), volume))
        .collect();
    let peak = waves.iter().flatten().fold(1e-9f32, |m, x| m.max(x.abs()));
    let (w, h) = (W * waves.len(), H);
    let mut px = vec![16u8; w * h * 3];
    for (c, wave) in waves.iter().enumerate() {
        let per = wave.len() / W;
        for x in 0..W {
            let chunk = &wave[x * per..(x + 1) * per];
            let (lo, hi) = chunk.iter().fold((0.0f32, 0.0f32), |(a, b), s| (a.min(*s), b.max(*s)));
            let y = |v: f32| (H as f32 / 2.0 - v / peak * (H as f32 / 2.0 - 4.0)) as usize;
            for yy in y(hi)..=y(lo).min(H - 1) {
                let i = (yy * w + c * W + x) * 3;
                px[i..i + 3].copy_from_slice(&[232, 227, 216]);
            }
        }
        // (a faint line between them)
        for yy in 0..H {
            let i = (yy * w + c * W) * 3;
            px[i..i + 3].copy_from_slice(&[60, 60, 60]);
        }
    }
    crate::shot::save_png(path, w, h, &px)
}

/// Real writing: tarae's own `writing::reveal_wake`, as written.
fn writing() -> String {
    const SRC: &str = include_str!("../../src/writing.rs");
    let from = SRC.find("pub fn reveal_wake(").unwrap_or(0);
    let to = SRC[from..].find("\n}\n").map_or(SRC.len().min(from + 1200), |e| from + e + 3);
    SRC[from..to].to_string()
}

/// The pieces as claude streams an edit's text, revealed in 25 ms steps, each step's sound: (when it's heard,
/// what). The edit lands 0.4 s after its last character shows.
fn burst(text: &str) -> Vec<(Duration, Said)> {
    let chars: Vec<char> = text.chars().collect();
    // (came: when each piece arrives, and how many characters it brings)
    let mut came: Vec<(Duration, usize)> = (0..4).map(|i| (Duration::from_millis(50 * i), 8)).collect();
    let mut at = Duration::from_millis(200);
    let mut sent: usize = came.iter().map(|c| c.1).sum();
    while sent < chars.len() {
        at += Duration::from_millis(640);
        let n = 190.min(chars.len() - sent);
        came.push((at, n));
        sent += n;
    }
    let start = Instant::now();
    let mut t = typist::Typist::new(0x5eed);
    let (mut shown, mut out) = (0usize, Vec::new());
    let mut now = Duration::ZERO;
    let mut arrived = 0usize;
    loop {
        // (what came by now; shown evenly toward when the next piece is due — the reveal's settled pace)
        let mut due = now;
        let mut total = 0;
        for (i, (t, n)) in came.iter().enumerate() {
            if *t <= now {
                total += n;
                due = came.get(i + 1).map_or(*t + Duration::from_millis(150), |next| next.0);
            }
        }
        arrived = arrived.max(total.min(chars.len()));
        let left = arrived - shown;
        let steps = (due.saturating_sub(now).as_secs_f32() / STEP.as_secs_f32()).ceil().max(1.0) as usize;
        let to = shown + left.div_ceil(steps);
        let step: String = chars[shown..to].iter().collect();
        shown = to;
        if let Some(s) = t.step(start + now, &step, STEP) {
            out.push((now, s));
        }
        if shown >= chars.len() {
            break;
        }
        now += STEP;
    }
    let s = t.once(start + now + Duration::from_millis(400), Kind::Settle);
    out.push((now + Duration::from_millis(400), s));
    out
}

/// The burst played: each sound handed to the synth at its step's sample, as the audio thread hands it.
fn render(burst: &[(Duration, Said)], length: Duration, volume: u8) -> Vec<f32> {
    let mut synth = Synth::new(RATE as f32);
    let total = (length.as_secs_f64() * RATE as f64) as usize;
    let mut out = vec![0.0f32; total];
    let mut at = 0usize;
    let master = said::master(volume);
    for (t, s) in burst {
        let i = ((t.as_secs_f64() * RATE as f64) as usize).min(total);
        synth.render(&mut out[at..i], master);
        synth.play(s);
        at = i;
    }
    synth.render(&mut out[at..], master);
    out
}

fn levels(samples: &[f32]) -> (f32, f32) {
    let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let sounding: Vec<f32> = samples.iter().copied().filter(|x| x.abs() > 1e-4).collect();
    let rms = (sounding.iter().map(|x| x * x).sum::<f32>() / sounding.len().max(1) as f32).sqrt();
    let db = |x: f32| 20.0 * x.max(1e-9).log10();
    (db(peak), db(rms))
}

/// 16-bit PCM, mono.
fn wav(path: &Path, samples: &[f32]) -> Result<()> {
    let data = samples.len() as u32 * 2;
    let mut b: Vec<u8> = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    std::fs::write(path, b).map_err(|e| format!("{}: {e}", path.display()))
}
