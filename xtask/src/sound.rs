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

pub(crate) const RATE: u32 = 48_000;
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

/// What a Short hears (`video.rs`), each at its ms: a key you typed (what it was: a return, the space bar, else a key),
/// a card's answer pressed as a stamp, an edit's landing settling the page, and a step of Claude's writing — what it
/// newly showed and how long until the next step — as tarae's reveal hears it (`typist::Typist::step`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Heard {
    Key(char),
    Stamp,
    Settle,
    Shown(String, Duration),
}

/// A Short's loudness: −20 LUFS integrated (BS.1770, `lufs`) at most — a keys-only track's (CLAUDE.md's Shorts bullet:
/// sparse short clicks reach −16 only by limiting every one flat); YouTube turns a loud track down, never a quiet one up.
const SHORT_LUFS: f32 = -20.0;
/// The most the limiter takes from the keys (dB): their gain no higher than leaves their loudest click this much over
/// the ceiling — the track quieter than `SHORT_LUFS` where that's what it costs, so a click keeps its own shape.
const LIMIT_MOST_DB: f32 = 2.0;
/// A Short's true peaks at most this (`limit`, dBTP) at first: room under −1 for AAC's overshoot — lowered where the
/// encoded track still passes it (`video::make`).
pub(crate) const SHORT_CEILING_DB: f32 = -2.0;

/// A Short's sound track as `track` made it: its samples, peak and RMS (dBFS, while sounding), true peak (dBTP), its
/// loudness (LUFS) and the gain it took (dB), the intro brush's loudness over the rest's (LU) before the gain and limiter
/// and after, and what the limiter took (`Limited`).
pub(crate) struct Track {
    pub(crate) samples: Vec<f32>,
    pub(crate) peak: f32,
    pub(crate) rms: f32,
    pub(crate) true_peak: f32,
    pub(crate) lufs: f32,
    pub(crate) gain: f32,
    pub(crate) brush: Option<(f32, f32)>,
    pub(crate) limited: Limited,
}

/// The limiter's largest gain reduction (dB, positive): on the keys and when (s into the track), and on the brush where
/// there is one — its own, then the two's together where they meet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Limited {
    pub(crate) keys: f32,
    pub(crate) keys_at: f32,
    pub(crate) brush: Option<(f32, f32)>,
}

/// A Short's sound track, `length` long: tarae's own strokes (`recorded::BANK`, the synth's settle) for each of
/// `heard`, as the audio thread plays them at the full volume. A key is the kind its character is (`Kind::Return`,
/// `Space`, else `Key`); a stamp is the big key with the settle under it, a firm press; a settle is tarae's,
/// `typist::GAP_MIN` after the writing's last key (`Typist::once`); Claude's writing is tarae's typist striking for each
/// step, as the reveal plays it. The intro's brush (`brush`) from `brush_at`, at its level against the keys. Then the
/// whole brought to `SHORT_LUFS` by one gain — or as near as `LIMIT_MOST_DB` of limiting lets it —, its true peaks held
/// under `ceiling_db` (dBTP; `master`) — the keys and the brush in balance (`master`).
pub(crate) fn track(
    heard: &[(Duration, Heard)],
    length: Duration,
    brush_at: Option<Duration>,
    ceiling_db: f32,
) -> Track {
    let mut seed: u32 = 0x5107_7a2a;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let start = Instant::now();
    let mut hand = typist::Typist::new(0x5eed);
    let mut said: Vec<(Duration, Said)> = Vec::new();
    for (t, h) in heard {
        let t = *t;
        let mut push = |kind: Kind, gain: f32| said.push((t, Said { kind, gain, seed: next(), delay: 0.0 }));
        match h {
            Heard::Key('\r' | '\n') => push(Kind::Return, 1.0),
            Heard::Key(' ') => push(Kind::Space, 1.0),
            Heard::Key(_) => push(Kind::Key, 0.9),
            Heard::Stamp => {
                push(Kind::Return, 1.0);
                push(Kind::Settle, 0.8);
            }
            Heard::Settle => said.push((t, hand.once(start + t, Kind::Settle))),
            Heard::Shown(text, step) => {
                if let Some(s) = hand.step(start + t, text, *step) {
                    said.push((t, s));
                }
            }
        }
    }
    said.sort_by_key(|s| s.0);
    let keys = render(&said, length, 100, Some(&recorded::BANK));
    let brush = brush_at.map(|at| {
        let from = ((at.as_secs_f64() * f64::from(RATE)) as usize).min(keys.len());
        let mut b = brush();
        b.truncate(keys.len() - from);
        (from, b)
    });
    // (the brush's loudness over the rest's)
    let over = |x: &[f32]| (brush.as_ref()).map(|(a, b)| lufs(&x[*a..a + b.len()]) - lufs(&x[a + b.len()..]));
    let mut mixed = keys.clone();
    if let Some((from, b)) = &brush {
        mixed[*from..from + b.len()].iter_mut().zip(b).for_each(|(x, b)| *x += b);
    }
    let before = over(&mixed);
    let laid = brush.as_ref().map(|(from, b)| (*from, b.as_slice()));
    let ceiling = 10f32.powf(ceiling_db / 20.0);
    let (samples, gain, limited) = master(&keys, laid, SHORT_LUFS, ceiling, LIMIT_MOST_DB);
    let (peak, rms) = levels(&samples);
    let brush = before.zip(over(&samples));
    let true_peak = 20.0 * true_peaks(&samples).into_iter().fold(0.0f32, f32::max).log10();
    Track { lufs: lufs(&samples), samples, peak, rms, true_peak, gain, brush, limited }
}

/// `keys` with the brush laid over them (from its sample) brought to `target` LUFS, their peaks held under `ceiling`
/// (`limit`): the keys by one gain — never past where the limiter would take more than `most_db` from them; the track
/// quieter than `target` there —, the brush at the loudness over them it was made at (`brush`'s level against a key) — a
/// limiter takes more from a key's click than from the brush's swish, and more from the tap than from a click, so one
/// gain for both would move it —, the two limited again together. Each gain found again after the limiter took its
/// share (`reach`). The mix, the keys' gain in dB, and what the limiter took.
fn master(
    keys: &[f32],
    brush: Option<(usize, &[f32])>,
    target: f32,
    ceiling: f32,
    most_db: f32,
) -> (Vec<f32>, f32, Limited) {
    // (a gain's true peaks are the gain's times x's: found once)
    let scaled = |x: &[f32], peaks: &[f32], db: f32| {
        let g = 10f32.powf(db / 20.0);
        let mut y: Vec<f32> = x.iter().map(|v| v * g).collect();
        let took = limit(&mut y, peaks, ceiling / g);
        let l = lufs(&y);
        ((y, took), l)
    };
    let tp = (true_peaks(keys), brush.map(|b| true_peaks(b.1)));
    let (at, brush_at) = (lufs(keys), brush.map_or(f32::NEG_INFINITY, |b| lufs(b.1)));
    let made = |db: f32| {
        let ((mut y, took), laid) = scaled(keys, &tp.0, db);
        let mut limited = Limited { keys: took.0, keys_at: took.1 as f32 / RATE as f32, brush: None };
        if let (Some((from, b)), Some(peaks)) = (brush, &tp.1) {
            // (the brush its loudness over the keys, or the keys' gain where they're silent)
            let ((b, own), _) = match laid.is_finite() {
                true => reach(laid + brush_at - at, brush_at, |db| scaled(b, peaks, db)),
                false => scaled(b, peaks, db),
            };
            y[from..from + b.len()].iter_mut().zip(&b).for_each(|(y, b)| *y += b);
            // (the two together, where they meet: a tenth of a second round the brush)
            let pad = RATE as usize / 10;
            let both = &mut y[from.saturating_sub(pad)..(from + b.len() + pad).min(keys.len())];
            let peaks = true_peaks(both);
            let together = limit(both, &peaks, ceiling);
            limited.brush = Some((own.0, together.0));
        }
        let l = lufs(&y);
        ((y, limited), l)
    };
    // (the keys' loudest true peak: past `most_db` over the ceiling, the limiter takes more than `most_db`)
    let loudest = tp.0.iter().fold(0.0f32, |m, p| m.max(*p));
    let cap = 20.0 * (ceiling / loudest).log10() + most_db;
    let ((mut y, mut limited), mut db) = reach(target, made(0.0).1, made);
    if db > cap {
        ((y, limited), _) = made(cap);
        db = cap;
    }
    (y, db, limited)
}

/// The gain (dB) at which `made` comes to `target` LUFS, its loudness at 0 dB `at`: a limiter gives back less than each
/// dB put in, so the secant's, until within 0.05 LU. What `made` made at it, and the gain.
fn reach<T>(target: f32, at: f32, made: impl Fn(f32) -> (T, f32)) -> (T, f32) {
    if !at.is_finite() {
        return (made(0.0).0, 0.0);
    }
    let (mut was, mut db) = ((0.0, at), target - at);
    let (mut y, mut l) = made(db);
    for _ in 0..12 {
        if (target - l).abs() < 0.05 {
            break;
        }
        let slope = ((l - was.1) / (db - was.0)).clamp(0.05, 1.0);
        was = (db, l);
        db += (target - l) / slope;
        (y, l) = made(db);
    }
    (y, db)
}

/// A limiter on true peaks (`peaks`, `x`'s own — `true_peaks`): each sample's gain the least that keeps its peak under
/// `ceiling`, let back up over `RELEASE_S` after a peak and brought down over `ATTACK_S` before it (a backward pass: it
/// sees the peak coming) — never a peak past the ceiling, never a step in the gain. Its largest reduction (dB, positive)
/// and the sample it's at.
fn limit(x: &mut [f32], peaks: &[f32], ceiling: f32) -> (f32, usize) {
    const ATTACK_S: f32 = 0.002;
    const RELEASE_S: f32 = 0.060;
    let per = |s: f32| 1.0 - (-1.0 / (s * RATE as f32)).exp();
    let (attack, release) = (per(ATTACK_S), per(RELEASE_S));
    let mut gains: Vec<f32> = peaks.iter().map(|p| (ceiling / p).min(1.0)).collect();
    let mut g = 1.0f32;
    for n in &mut gains {
        g = n.min(g + (1.0 - g) * release);
        *n = g;
    }
    g = 1.0;
    for n in gains.iter_mut().rev() {
        g = n.min(g + (1.0 - g) * attack);
        *n = g;
    }
    let (mut least, mut at) = (1.0f32, 0);
    for (i, (v, g)) in x.iter_mut().zip(gains).enumerate() {
        *v *= g;
        if g < least {
            (least, at) = (g, i);
        }
    }
    (-20.0 * least.log10(), at)
}

/// The true peak at each sample of `x`: the largest of it, its neighbours and the three points a quarter apart on
/// either side, 4× oversampled by a Hann-windowed sinc over 16 samples (BS.1770's way to the peaks between samples —
/// what a player's converter, and AAC after it, reaches).
fn true_peaks(x: &[f32]) -> Vec<f32> {
    const TAPS: i32 = 8;
    let phase = |f: f32| -> Vec<f32> {
        (-TAPS + 1..=TAPS)
            .map(|k| {
                let t = k as f32 - f;
                let sinc = if t == 0.0 {
                    1.0
                } else {
                    (std::f32::consts::PI * t).sin() / (std::f32::consts::PI * t)
                };
                let hann = 0.5 + 0.5 * (std::f32::consts::PI * t / TAPS as f32).cos();
                sinc * hann
            })
            .collect()
    };
    let phases = [phase(0.25), phase(0.5), phase(0.75)];
    let at = |i: i64| x.get(usize::try_from(i).ok().unwrap_or(usize::MAX)).copied().unwrap_or(0.0);
    // (the peak between sample n and n + 1)
    let between: Vec<f32> = (0..x.len() as i64)
        .map(|n| {
            let mut p = x[n as usize].abs();
            for w in &phases {
                let v: f32 =
                    (w.iter().enumerate()).map(|(j, w)| w * at(n - i64::from(TAPS) + 1 + j as i64)).sum();
                p = p.max(v.abs());
            }
            p
        })
        .collect();
    (0..x.len()).map(|n| between[n].max(if n > 0 { between[n - 1] } else { 0.0 })).collect()
}

/// The integrated loudness of `x` (mono at `RATE`), LUFS, as ITU-R BS.1770-4 measures it: K-weighted, in 400 ms blocks
/// a 100 ms step apart, gated at −70 LUFS and then 10 LU under the blocks kept. −∞ where no block is kept.
pub(crate) fn lufs(x: &[f32]) -> f32 {
    // (the K-weighting's two stages at 48 kHz: the head's shelf, then the high-pass)
    let biquad = |x: &[f64], b: [f64; 3], a: [f64; 2]| -> Vec<f64> {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        x.iter()
            .map(|&v| {
                let y = b[0] * v + b[1] * x1 + b[2] * x2 - a[0] * y1 - a[1] * y2;
                (x2, x1, y2, y1) = (x1, v, y1, y);
                y
            })
            .collect()
    };
    let x: Vec<f64> = x.iter().map(|&v| f64::from(v)).collect();
    let shelf = biquad(
        &x,
        [1.535_124_859_586_97, -2.691_696_189_406_38, 1.198_392_810_852_85],
        [-1.690_659_293_182_41, 0.732_480_774_215_85],
    );
    let k = biquad(&shelf, [1.0, -2.0, 1.0], [-1.990_047_454_833_98, 0.990_072_250_366_21]);
    let mut sums = vec![0.0f64; k.len() + 1];
    for (i, v) in k.iter().enumerate() {
        sums[i + 1] = sums[i] + v * v;
    }
    let (block, step) = (RATE as usize * 4 / 10, RATE as usize / 10);
    let blocks: Vec<f64> = (0..)
        .map(|i| i * step)
        .take_while(|s| s + block <= k.len())
        .map(|s| (sums[s + block] - sums[s]) / block as f64)
        .collect();
    let loud = |ms: f64| -0.691 + 10.0 * ms.log10();
    let mean = |b: &[f64]| b.iter().sum::<f64>() / b.len() as f64;
    let kept: Vec<f64> = blocks.into_iter().filter(|&ms| loud(ms) > -70.0).collect();
    if kept.is_empty() {
        return f32::NEG_INFINITY;
    }
    let gate = loud(mean(&kept)) - 10.0;
    let kept: Vec<f64> = kept.into_iter().filter(|&ms| loud(ms) > gate).collect();
    loud(mean(&kept)) as f32
}

/// The intro's brush, as 10-27's `mix.pl` made it (main's `.sb/youtube/2026-10-27-first-clip/`), at `RATE` from the
/// intro's start: a swish as it swings down (band-passed noise rising 500 → 2,200 Hz, 0.30–0.80 s), its point's tap on
/// the cursor at 0.80 s (trollarch2's third letter, pitched down, its release after) and a dry drag as it draws the
/// cursor (to 1.16 s). Made at mix.pl's 32 kHz (the bank's rate) and its level against a key — its keys at 0.55 of a
/// stroke (0.925 on average), `track`'s at 0.9 —, in `track`'s units before its scale.
pub(crate) fn brush() -> Vec<f32> {
    const R: f32 = 32_000.0;
    let bank = &recorded::BANK;
    // (mix.pl's units — a 16-bit sample — to `track`'s, by a key's gain in each)
    let units = 0.9 / (0.55 * 0.925) / 32_768.0;
    let sample = |i: u32| {
        let at = i as usize * 2;
        bank.pcm.get(at..at + 2).map_or(0.0, |b| f32::from(i16::from_le_bytes([b[0], b[1]])))
    };
    let mut mix = vec![0.0f32; (1.3 * R) as usize];
    // (mix.pl's `put`: a slice of the bank from `at` s, read every `pitch`, at `gain` and the bank's level)
    let put = |mix: &mut Vec<f32>, at: f32, (from, len): (u32, u32), gain: f32, pitch: f32| {
        let o = (at * R) as usize;
        for i in 0.. {
            let s = i as f32 * pitch;
            if s >= len as f32 - 1.0 || o + i >= mix.len() {
                break;
            }
            let (j, f) = (s as u32, s.fract());
            let v = sample(from + j) * (1.0 - f) + sample(from + j + 1) * f;
            mix[o + i] += v * gain * bank.level;
        }
    };
    let mut seed: u64 = 7;
    let mut rnd = || {
        seed = (seed * 1_103_515_245 + 12_345) % 2_147_483_648;
        seed as f32 / 2_147_483_648.0
    };
    let (mut lp, mut bp, mut hp, mut before) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for (i, m) in mix.iter_mut().enumerate().take((1.16 * R) as usize).skip((0.30 * R) as usize) {
        let t = i as f32 / R;
        let n = rnd() * 2.0 - 1.0;
        if t < 0.80 {
            // (a state-variable band-pass, its centre rising with the swing)
            let e = ((t - 0.30) / 0.50).powf(2.2);
            let w = 2.0 * (std::f32::consts::PI * (500.0 + 1_700.0 * e) / R).sin();
            lp += w * bp;
            bp += w * (n - lp - 0.35 * bp);
            *m += bp * 7_650.0 * e;
        } else {
            let e = (-(t - 0.80) / 0.10).exp() * (0.6 + 0.4 * rnd());
            hp = 0.7 * (hp + n - before);
            before = n;
            *m += hp * 1_800.0 * e;
        }
    }
    if let Some(touch) = bank.letters.get(2) {
        put(&mut mix, 0.80, touch.press, 1.6, 0.62);
        put(&mut mix, 0.89, touch.release, 0.7, 0.55);
    }
    // (to `RATE`, linear between)
    let n = (mix.len() as f32 * RATE as f32 / R) as usize;
    (0..n)
        .map(|k| {
            let s = k as f32 * R / RATE as f32;
            let (j, f) = (s as usize, s.fract());
            let (a, b) = (mix[j], mix.get(j + 1).copied().unwrap_or(0.0));
            (a + (b - a) * f) * units
        })
        .collect()
}

/// 16-bit PCM, mono.
pub(crate) fn wav(path: &Path, samples: &[f32]) -> Result<()> {
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

    /// The intro's brush is 10-27's: silent until the swing, the swish rising to the tap at 0.80 s — its loudest —,
    /// the tap at mix.pl's level against a key (1.6 of a stroke to a key's 0.55 × 0.925, pitched down), then gone.
    #[test]
    fn the_brush_swishes_then_taps_at_its_level_against_a_key() {
        let b = brush();
        let at = |s: f32| (s * RATE as f32) as usize;
        let peak = |r: std::ops::Range<usize>| b[r].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert_eq!(peak(0..at(0.30)), 0.0, "silent before the swing");
        assert!(peak(at(0.40)..at(0.50)) < peak(at(0.65)..at(0.80)), "the swish rises");
        let loudest = b.iter().enumerate().max_by(|x, y| x.1.abs().total_cmp(&y.1.abs())).map(|x| x.0);
        assert!(
            loudest.is_some_and(|i| (at(0.80)..at(0.86)).contains(&i)),
            "the tap the loudest: {loudest:?}"
        );
        // (a key as `track` plays it: the same stroke's press, at a key's 0.9 and the bank's level)
        let st = recorded::BANK.letters[2].press;
        let pcm = recorded::BANK.pcm;
        let key = (st.0..st.0 + st.1)
            .map(|i| {
                f32::from(i16::from_le_bytes([pcm[i as usize * 2], pcm[i as usize * 2 + 1]])).abs() / 32_768.0
            })
            .fold(0.0f32, f32::max)
            * recorded::BANK.level
            * 0.9;
        let ratio = peak(at(0.80)..at(0.86)) / key;
        let mix_pl = 1.6 / (0.55 * 0.925);
        assert!(
            (0.85 * mix_pl..1.1 * mix_pl).contains(&ratio),
            "the tap {ratio:.2}× a key, mix.pl's {mix_pl:.2}×"
        );
        assert_eq!(peak(at(1.25)..b.len()), 0.0, "gone after");
    }

    /// `lufs` and `true_peaks` are BS.1770's: a 997 Hz sine at full scale reads −3.01 LUFS, 20 dB down −23.01, silence
    /// nothing; a peak between samples is found.
    #[test]
    fn the_loudness_is_bs1770s() {
        let sine = |a: f32| -> Vec<f32> {
            (0..RATE * 3)
                .map(|n| a * (2.0 * std::f32::consts::PI * 997.0 * n as f32 / RATE as f32).sin())
                .collect()
        };
        assert!((lufs(&sine(1.0)) + 3.01).abs() < 0.05, "{}", lufs(&sine(1.0)));
        assert!((lufs(&sine(0.1)) + 23.01).abs() < 0.05, "{}", lufs(&sine(0.1)));
        assert_eq!(lufs(&vec![0.0; RATE as usize]), f32::NEG_INFINITY);
        // (a quarter of the rate, 45° off: every sample at 0.707, its true peak 1)
        let quarter: Vec<f32> = (0..4_800)
            .map(|n| (std::f32::consts::FRAC_PI_2 * n as f32 + std::f32::consts::FRAC_PI_4).sin())
            .collect();
        let tp = true_peaks(&quarter)[100..4_700].iter().fold(0.0f32, |m, p| m.max(*p));
        assert!((tp - 1.0).abs() < 0.02, "true peak {tp}");
    }

    /// A Short's track stands at `SHORT_LUFS` or as near as `LIMIT_MOST_DB` of limiting lets it, with every sample under
    /// `SHORT_CEILING_DB`, and the intro's brush keeps its balance against the keys: it follows their loudness, the
    /// limiter moving it under 0.5 LU.
    #[test]
    fn a_shorts_track_is_at_its_loudness_under_its_ceiling_the_brush_in_balance() {
        let keys: Vec<(Duration, Heard)> = (0..60)
            .map(|i| (Duration::from_millis(3_000 + i * 130), Heard::Key(if i % 6 == 5 { ' ' } else { 'a' })))
            .collect();
        assert!((SHORT_LUFS + 20.0).abs() < 0.5 && LIMIT_MOST_DB <= 2.0, "near −20 LUFS, barely limited");
        let t = track(&keys, Duration::from_secs(12), Some(Duration::ZERO), SHORT_CEILING_DB);
        assert!(t.lufs < SHORT_LUFS + 0.1, "{:.2} LUFS", t.lufs);
        assert!(t.limited.keys <= LIMIT_MOST_DB + 0.01, "the limiter took {:.2} dB", t.limited.keys);
        assert!(
            (t.lufs - SHORT_LUFS).abs() < 0.1 || (t.limited.keys - LIMIT_MOST_DB).abs() < 0.01,
            "as loud as either lets: {:.2} LUFS, {:.2} dB taken",
            t.lufs,
            t.limited.keys
        );
        let tp = true_peaks(&t.samples).into_iter().fold(0.0f32, f32::max);
        assert!(20.0 * tp.log10() <= SHORT_CEILING_DB + 0.1, "true peak {:.2} dBTP", 20.0 * tp.log10());
        assert!((t.true_peak - 20.0 * tp.log10()).abs() < 1e-3, "its true peak said");
        let (before, after) = t.brush.expect("the brush");
        assert!(
            (after - before).abs() < 0.5,
            "the brush {before:+.1} LU over the keys before, {after:+.1} after"
        );
    }

    /// The keys come to their loudness only as far as the limiter barely touches them: a steady tone (its peaks 3 dB
    /// over its loudness) reaches the target untouched; sparse clicks (their peaks far over it) stop where the limiter
    /// would take more than its most, the track quieter than the target, that most taken at a click.
    #[test]
    fn the_keys_are_as_loud_as_the_limiter_barely_touching_them_lets() {
        let ceiling = 10f32.powf(-2.0 / 20.0);
        let tone: Vec<f32> = (0..RATE * 4)
            .map(|n| 0.01 * (std::f32::consts::TAU * 997.0 * n as f32 / RATE as f32).sin())
            .collect();
        let (y, db, limited) = master(&tone, None, -20.0, ceiling, 2.0);
        assert!((lufs(&y) + 20.0).abs() < 0.05, "{:.2} LUFS", lufs(&y));
        assert!((db - 23.0).abs() < 0.1 && limited.keys == 0.0, "{db:+.2} dB, {:.2} dB taken", limited.keys);
        // (a click every half second: a few samples of a decaying tone, each louder than the last)
        let mut clicks = vec![0.0f32; RATE as usize * 4];
        for k in 0..8 {
            for i in 0..240 {
                let t = i as f32 / RATE as f32;
                clicks[k * RATE as usize / 2 + i] = 0.05
                    * (1.0 + k as f32 / 8.0)
                    * (-t * 400.0).exp()
                    * (std::f32::consts::TAU * 3_000.0 * t).sin();
            }
        }
        let (y, _, limited) = master(&clicks, None, -20.0, ceiling, 2.0);
        assert!(lufs(&y) < -21.0, "quieter than the target: {:.2} LUFS", lufs(&y));
        assert!((limited.keys - 2.0).abs() < 0.01, "{:.2} dB taken", limited.keys);
        assert!((3.5..3.505).contains(&limited.keys_at), "at the loudest click: {:.3} s", limited.keys_at);
        let tp = true_peaks(&y).into_iter().fold(0.0f32, f32::max);
        assert!(tp <= ceiling * 1.001, "under the ceiling: {tp}");
    }

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
