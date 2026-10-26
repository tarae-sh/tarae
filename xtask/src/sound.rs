//! `cargo xtask sound [--volume N] [OUT_DIR]` — a burst of real writing as tarae hears it, for tuning by ear:
//! tarae's own `writing::reveal_wake` streamed as claude streams an edit (its first pieces singly ~50 ms apart, then
//! ~190 characters every ~0.64 s — measured, `writing.rs`), shown at the stream's pace in 25 ms steps as the reveal
//! does once its pace has settled, each step's sound decided by tarae's own `typist.rs`, played by its own
//! `synth.rs` on its recorded strokes (`recorded.rs`), the edit landing unseen at the end — `writing.wav`, at the
//! default volume (`editor.sound-volume` 30) unless `--volume` says; each kind alone as a waveform, `keys.png`. No
//! device: files only. `make` and `install` turn a recording into tarae's strokes (`sound/samples.rs`), `compare`
//! plays the same writing by each.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::Result;

// (tarae's own, pure: what the WAVs hold is what tarae plays)
#[allow(dead_code, reason = "tarae's: not all of it is the WAVs'")]
#[path = "../../src/sound/recorded.rs"]
mod recorded;
#[path = "../../src/sound/said.rs"]
mod said;
#[path = "../../src/sound/sampled.rs"]
mod sampled;
#[allow(dead_code, reason = "tarae's: not all of it is the WAVs'")]
#[path = "../../src/sound/synth.rs"]
mod synth;
#[allow(dead_code, reason = "tarae's: not all of it is the WAVs'")]
#[path = "../../src/sound/typist.rs"]
mod typist;

use said::{Kind, Said};
use sampled::{Bank, Stroke};
use synth::Synth;

const RATE: u32 = 48_000;
const STEP: Duration = Duration::from_millis(25);

mod samples;

pub fn run(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("make") => return samples::make(&args[1..]),
        Some("install") => return install(&args[1..]),
        Some("compare") => return compare(&args[1..]),
        _ => {}
    }
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
    let samples = render(&burst, length + Duration::from_millis(600), volume, Some(&recorded::BANK));
    let path = out.join("writing.wav");
    wav(&path, &samples)?;
    let (peak, rms) = levels(&samples);
    // (each kind alone)
    let alone = |kind: Kind| {
        levels(&render(
            &[(Duration::ZERO, said(kind))],
            Duration::from_millis(500),
            volume,
            Some(&recorded::BANK),
        ))
        .0
    };
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
        .map(|&(kind, _)| {
            render(&[(Duration::ZERO, said(kind))], Duration::from_millis(300), volume, Some(&recorded::BANK))
        })
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

/// A paragraph of prose, typed by a person's rhythm (`typist::typing`) — the comparisons' second.
const PROSE: &str = "tarae is an editor you can watch at work. Claude reads, searches and writes in the open, and every \
                     change it makes waits for your eye.\nThe sound is there for the hours you sit beside it: a \
                     keyboard in the next room, typing what you are about to read.\n";

/// A bank `make` left, as tarae's own would be (leaked: the WAVs' run is short).
fn bank(m: &samples::Made) -> &'static Bank {
    let pcm: &'static [u8] = Vec::leak(m.pcm.iter().flat_map(|v| v.to_le_bytes()).collect());
    let strokes = |big: bool| -> &'static [Stroke] {
        Vec::leak(
            m.strokes
                .iter()
                .filter(|s| s.0 == big)
                .map(|s| Stroke { press: s.1, release: s.2, hold: s.3 })
                .collect(),
        )
    };
    Box::leak(Box::new(Bank {
        rate: m.rate as f32,
        pcm,
        letters: strokes(false),
        big: strokes(true),
        level: m.level,
    }))
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// The level that types `PROSE` as loud (RMS) as the synth does — so a comparison isn't won by loudness.
pub fn level_for(m: &samples::Made) -> Result<f32> {
    let typed = typist::typing(PROSE, 0x5eed);
    let length = typed.last().map_or(Duration::ZERO, |t| t.0) + Duration::from_millis(500);
    let synth = rms(&render(&typed, length, 30, None));
    let raw = samples::Made { level: 1.0, rate: m.rate, pcm: m.pcm.clone(), strokes: m.strokes.clone() };
    let at_one = rms(&render(&typed, length, 30, Some(bank(&raw))));
    Ok(synth / at_one.max(1e-9))
}

/// `cargo xtask sound install NAME`: `.sb/samples/banks/NAME` becomes tarae's — `src/sound/recorded.{pcm,rs}`.
fn install(args: &[String]) -> Result<()> {
    let [name] = args else { return Err("cargo xtask sound install NAME".into()) };
    let name = samples::bank_name(name)?;
    let m = samples::Made::load(&crate::root().join(".sb/samples/banks"), name)?;
    let src = crate::root().join("src/sound");
    let pcm: Vec<u8> = m.pcm.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(src.join("recorded.pcm"), &pcm).map_err(|e| e.to_string())?;
    let list = |big: bool| {
        m.strokes
            .iter()
            .filter(|s| s.0 == big)
            .map(|s| {
                format!(
                    "    Stroke {{ press: ({}, {}), release: ({}, {}), hold: {:.1} }}, // at {:.3} s\n",
                    s.1.0, s.1.1, s.2.0, s.2.1, s.3, s.4
                )
            })
            .collect::<String>()
    };
    let rs = format!(
        "//! The recorded strokes tarae plays (`sampled.rs`), made by `cargo xtask sound make` and `install {name}` —\n\
         //! never by hand. Cut from\n//! {source},\n\
         //! high-passed at 100 Hz, each faded and normalised: {n} strokes, {bytes} bytes.\n\n\
         use super::sampled::{{Bank, Stroke}};\n\n\
         pub static BANK: Bank = Bank {{\n    rate: {rate:.1},\n    pcm: include_bytes!(\"recorded.pcm\"),\n    letters: &LETTERS,\n    big: &BIG,\n    level: {level:.4},\n}};\n\n\
         const LETTERS: [Stroke; {nl}] = [\n{letters}];\n\n\
         const BIG: [Stroke; {nb}] = [\n{big}];\n",
        source = SOURCES.iter().find(|s| s.0 == name).map_or(name, |s| s.1),
        n = m.strokes.len(),
        bytes = pcm.len(),
        rate = m.rate as f32,
        level = m.level,
        nl = m.strokes.iter().filter(|s| !s.0).count(),
        letters = list(false),
        nb = m.strokes.iter().filter(|s| s.0).count(),
        big = list(true),
    );
    let path = src.join("recorded.rs");
    std::fs::write(&path, rs).map_err(|e| e.to_string())?;
    // (as `cargo fmt` leaves it, by the project's rustfmt.toml: its layout depends on the numbers)
    let fmt = std::process::Command::new("rustfmt").args(["--edition", "2024"]).arg(&path).status();
    if !fmt.as_ref().is_ok_and(|s| s.success()) {
        return Err(format!("rustfmt {}: {fmt:?}", path.display()));
    }
    println!(
        "src/sound/recorded.pcm: {} bytes ({:.1} KB) · src/sound/recorded.rs",
        pcm.len(),
        pcm.len() as f32 / 1024.0
    );
    Ok(())
}

/// Where a bank's recording came from (its licence and author), by the name `make` gave it.
const SOURCES: [(&str, &str); 1] = [(
    "trollarch2",
    "\"Keyboard Typing\" by trollarch2, https://freesound.org/people/trollarch2/sounds/331656/ (CC0)",
)];

/// `cargo xtask sound compare [DIR]`: the same writing by the synth's B and by each bank in `.sb/samples/banks` —
/// Claude's edit as tarae reveals it (`code-*.wav`, the settle at its end synthesized in all), a paragraph typed by a
/// person's rhythm (`prose-*.wav`) — and each bank's strokes alone, 0.6 s apart (`strokes-*.wav`); the edit by the
/// strokes tarae has installed, `code-final.wav`.
fn compare(args: &[String]) -> Result<()> {
    let out = args.first().map_or_else(|| crate::root().join(".sb/sound/compare"), PathBuf::from);
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let dir = crate::root().join(".sb/samples/banks");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".txt").map(String::from))
        .collect();
    names.sort();
    let code = burst(&writing());
    let prose = typist::typing(PROSE, 0x5eed);
    let end = |b: &[(Duration, Said)]| b.last().map_or(Duration::ZERO, |t| t.0) + Duration::from_millis(600);
    let x = render(&code, end(&code), 30, Some(&recorded::BANK));
    let path = out.join("code-final.wav");
    wav(&path, &x)?;
    let (peak, r) = levels(&x);
    println!("{} · peak {peak:.1} dBFS · RMS {r:.1} (the installed strokes)", path.display());
    let mut sets: Vec<(String, Option<&'static Bank>)> = vec![("synth-b".into(), None)];
    for n in &names {
        sets.push((n.clone(), Some(bank(&samples::Made::load(&dir, n)?))));
    }
    for (name, b) in &sets {
        for (what, burst) in [("code", &code), ("prose", &prose)] {
            let x = render(burst, end(burst), 30, *b);
            let path = out.join(format!("{what}-{name}.wav"));
            wav(&path, &x)?;
            let (peak, r) = levels(&x);
            println!(
                "{:<34} {:>5.1} s · peak {peak:>5.1} dBFS · RMS {r:>5.1}",
                path.display().to_string(),
                x.len() as f32 / RATE as f32
            );
        }
        if let Some(b) = b {
            // (each stroke as cut: its press, its release at its hold — no variation, at the bank's level)
            let all: Vec<&Stroke> = b.letters.iter().chain(b.big).collect();
            let gap = (0.6 * RATE as f32) as usize;
            let mut x = vec![0.0f32; gap * (all.len() + 1)];
            let master = said::master(30);
            for (k, s) in all.iter().enumerate() {
                let mut put = |(from, len): (u32, u32), at: usize| {
                    for i in 0..(len as f32 * RATE as f32 / b.rate) as usize {
                        let pos = i as f32 * b.rate / RATE as f32;
                        let j = from as usize + pos as usize;
                        let v = |j: usize| {
                            b.pcm
                                .get(j * 2..j * 2 + 2)
                                .map_or(0.0, |c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
                        };
                        if let Some(o) = x.get_mut(at + i) {
                            *o += v(j) * b.level * master;
                        }
                    }
                };
                let at = gap / 3 + k * gap;
                put(s.press, at);
                put(s.release, at + (s.hold / 1000.0 * RATE as f32) as usize);
            }
            let path = out.join(format!("strokes-{name}.wav"));
            wav(&path, &x)?;
            println!(
                "{:<34} {} strokes ({} letters, then {} big)",
                path.display().to_string(),
                all.len(),
                b.letters.len(),
                b.big.len()
            );
        }
    }
    Ok(())
}

/// The burst played: each sound handed to the synth at its step's sample, as the audio thread hands it — by `bank`'s
/// strokes, or synthesized all through.
fn render(burst: &[(Duration, Said)], length: Duration, volume: u8, bank: Option<&'static Bank>) -> Vec<f32> {
    render_with(burst, length, volume, bank, |s, out, master| s.render(out, master))
}

/// `render` with each stretch finished by `finish` (the synth, its stretch, the master level).
fn render_with(
    burst: &[(Duration, Said)],
    length: Duration,
    volume: u8,
    bank: Option<&'static Bank>,
    finish: impl Fn(&mut Synth, &mut [f32], f32),
) -> Vec<f32> {
    let mut synth = Synth::with(RATE as f32, bank);
    let total = (length.as_secs_f64() * RATE as f64) as usize;
    let mut out = vec![0.0f32; total];
    let mut at = 0usize;
    let master = said::master(volume);
    for (t, s) in burst {
        let i = ((t.as_secs_f64() * RATE as f64) as usize).min(total);
        finish(&mut synth, &mut out[at..i], master);
        synth.play(s);
        at = i;
    }
    finish(&mut synth, &mut out[at..], master);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The limiter changes nothing at the default volume: the code paragraph (`code-final.wav`'s — Claude's edit as
    /// tarae reveals it, the installed strokes) at volume 30 is the same samples as before it, when the volume was
    /// applied and clamped; and nothing in it comes near the knee.
    #[test]
    fn at_the_default_volume_the_limiter_leaves_every_sample_as_it_was() {
        let code = burst(&writing());
        let length = code.last().map_or(Duration::ZERO, |t| t.0) + Duration::from_millis(600);
        let bank = Some(&recorded::BANK);
        let now = render(&code, length, 30, bank);
        let before = render_with(&code, length, 30, bank, |s, out, master| {
            s.mix(out);
            for x in out {
                *x = (*x * master).clamp(-1.0, 1.0);
            }
        });
        assert!(now.iter().any(|x| *x != 0.0), "the paragraph is silent");
        assert_eq!(now, before, "the limiter changed a sample at volume 30");
        let peak = now.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak < synth::KNEE / 2.0, "the paragraph peaks at {peak}, within 6 dB of the knee");
    }
}
