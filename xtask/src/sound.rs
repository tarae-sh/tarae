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

/// A track `length` long in samples at `RATE`, whole ones (a last part of one left out): the length the keys are
/// rendered at, and every track mixed with them (the cut's `sound::track_len`).
pub(crate) fn samples_in(length: Duration) -> usize {
    (length.as_secs_f64() * RATE as f64) as usize
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
    let total = samples_in(length);
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
pub(crate) const SHORT_LUFS: f32 = -20.0;
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
    pub(crate) bed: Option<Bedded>,
}

/// A Short's music bed (CLAUDE.md's Shorts bullet: a music bed the user picks from YouTube Studio's Audio Library, about
/// 5 dB under the keys, faded in 0.5 s and out with the end card): the level the user chose on the hand mix of
/// 2026-11-05 — Short 1's keys −22.6 LUFS, the track at −7 dB −27.6, each measured alone (BS.1770, `lufs`) —, kept
/// as the one under the other, whatever the keys' loudness or the file's.
pub(crate) const BED_UNDER_LU: f32 = 5.0;
/// The bed eased in from the take's first frame this long, and out this long to the end card's last (linear, as the
/// hand mix's `afade`).
pub(crate) const BED_IN_S: f32 = 0.5;
pub(crate) const BED_OUT_S: f32 = 2.5;
/// A music file's sound starts where its 10 ms RMS first passes this (dBFS): its leading silence left out.
const BED_SOUNDS_DB: f32 = -50.0;
/// A sample the limiter turned down by more than this (dB) counts as touched.
pub(crate) const TOUCHED_DB: f32 = 0.5;

/// The bed as laid: where its sound starts in its file (s), its gain (dB, against its file), the keys' loudness and the
/// bed's alone (LUFS, before the limiter), and what the limiter took from the two together — its largest reduction
/// (dB) and when (s), and how long it took more than `TOUCHED_DB` (s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bedded {
    pub(crate) from: f32,
    pub(crate) gain: f32,
    pub(crate) keys: f32,
    pub(crate) bed: f32,
    pub(crate) limited: f32,
    pub(crate) limited_at: f32,
    pub(crate) touched: f32,
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
/// under `ceiling_db` (dBTP; `master`) — the keys and the brush in balance (`master`). A `music` file's samples (mono
/// at `RATE`) laid under it all as its bed (`bedded`).
pub(crate) fn track(
    heard: &[(Duration, Heard)],
    length: Duration,
    brush_at: Option<Duration>,
    ceiling_db: f32,
    music: Option<&[f32]>,
) -> Track {
    Keys::new(heard, length, brush_at).track(ceiling_db, music)
}

/// A track's keys and brush as rendered, before their level — what `track` makes whatever the ceiling — with their
/// true peaks and loudness (`Prepared`): made once, mastered at as many ceilings as asked.
pub(crate) struct Keys {
    keys: Vec<f32>,
    brush: Option<(usize, Vec<f32>)>,
    /// The brush's loudness over the rest's, before the gain and limiter.
    before: Option<f32>,
    tp: Vec<f32>,
    brush_tp: Option<Vec<f32>>,
    at: f32,
    brush_at: f32,
}

/// The keys' track at a ceiling, for the cut: its samples, loudness (LUFS), gain (dB) and the most the limiter took from
/// a click (dB).
pub(crate) struct Mastered {
    pub(crate) samples: Vec<f32>,
    pub(crate) lufs: f32,
    pub(crate) gain: f32,
    pub(crate) limited: f32,
}

/// The keys at the cap for the first ceiling tried, `SHORT_CEILING_DB` (`Keys::unit`): where the gain is capped — the
/// limiter would take more than `LIMIT_MOST_DB` to reach `SHORT_LUFS` — the track at another ceiling is this moved by
/// as many dB (the limiter's gains depend only on the ceiling over the keys' gain, which the cap holds), so only its
/// level moves. (The brush's own level is found to 0.05 LU, `reach`: at another ceiling its samples may differ by that.)
pub(crate) struct Unit {
    ceiling: f32,
    samples: Vec<f32>,
    lufs: f32,
    gain: f32,
    limited: f32,
}

impl Keys {
    pub(crate) fn new(heard: &[(Duration, Heard)], length: Duration, brush_at: Option<Duration>) -> Keys {
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
            let mut push =
                |kind: Kind, gain: f32| said.push((t, Said { kind, gain, seed: next(), delay: 0.0 }));
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
        let mut mixed = keys.clone();
        if let Some((from, b)) = &brush {
            mixed[*from..from + b.len()].iter_mut().zip(b).for_each(|(x, b)| *x += b);
        }
        let before = over(&brush, &mixed);
        drop(mixed);
        // (a gain's true peaks are the gain's times x's: found once)
        let (tp, brush_tp) = (true_peaks(&keys), brush.as_ref().map(|b| true_peaks(&b.1)));
        let (at, brush_at) = (lufs(&keys), brush.as_ref().map_or(f32::NEG_INFINITY, |b| lufs(&b.1)));
        Keys { keys, brush, before, tp, brush_tp, at, brush_at }
    }

    fn prepared(&self) -> Prepared<'_> {
        Prepared {
            keys: &self.keys,
            brush: self.brush.as_ref().map(|(from, b)| (*from, b.as_slice())),
            tp: (&self.tp, self.brush_tp.as_deref()),
            at: self.at,
            brush_at: self.brush_at,
        }
    }

    /// `track`'s: mastered at `ceiling_db`, `music` under it, its figures.
    fn track(&self, ceiling_db: f32, music: Option<&[f32]>) -> Track {
        let ceiling = 10f32.powf(ceiling_db / 20.0);
        let (samples, gain, limited) = self.prepared().master(SHORT_LUFS, ceiling, LIMIT_MOST_DB);
        let brush = self.before.zip(over(&self.brush, &samples));
        let (samples, bed) = match music {
            Some(m) => {
                let (y, b) = bedded(samples, m, ceiling);
                (y, Some(b))
            }
            None => (samples, None),
        };
        let (peak, rms) = levels(&samples);
        let true_peak = 20.0 * true_peaks(&samples).into_iter().fold(0.0f32, f32::max).log10();
        Track { lufs: lufs(&samples), samples, peak, rms, true_peak, gain, brush, limited, bed }
    }

    /// The keys at the cap for the first ceiling tried (`Unit`): made once, whatever the ceilings tried.
    pub(crate) fn unit(&self) -> Unit {
        let p = self.prepared();
        let ceiling = SHORT_CEILING_DB;
        let c = 10f32.powf(ceiling / 20.0);
        let cap = p.cap(c, LIMIT_MOST_DB);
        let ((samples, limited), lufs) = p.made(cap, c);
        Unit { ceiling, samples, lufs, gain: cap, limited: limited.keys }
    }

    /// The keys mastered at `ceiling_db` as `track` masters them, for the cut (no bed, none of a Short's figures): where
    /// the gain is capped there, `unit` times the ceiling — no pass over the track —, else mastered again.
    pub(crate) fn mastered(&self, unit: &Unit, ceiling_db: f32) -> Mastered {
        let moved = ceiling_db - unit.ceiling;
        if unit.lufs.is_finite() && unit.lufs + moved < SHORT_LUFS {
            let samples = match moved {
                0.0 => unit.samples.clone(),
                _ => {
                    let k = 10f32.powf(moved / 20.0);
                    unit.samples.iter().map(|x| x * k).collect()
                }
            };
            return Mastered {
                samples,
                lufs: unit.lufs + moved,
                gain: unit.gain + moved,
                limited: unit.limited,
            };
        }
        let ceiling = 10f32.powf(ceiling_db / 20.0);
        let (samples, gain, limited) = self.prepared().master(SHORT_LUFS, ceiling, LIMIT_MOST_DB);
        Mastered { lufs: lufs(&samples), samples, gain, limited: limited.keys }
    }
}

/// The brush's loudness over the rest's in `x`: what comes before it and after — the end card's brush has little after it.
fn over(brush: &Option<(usize, Vec<f32>)>, x: &[f32]) -> Option<f32> {
    (brush.as_ref()).map(|(a, b)| {
        let rest: Vec<f32> = x[..*a].iter().chain(&x[a + b.len()..]).copied().collect();
        lufs(&x[*a..a + b.len()]) - lufs(&rest)
    })
}

/// `keys` (the track as mastered) with `music` laid under them as their bed: from where its sound starts
/// (`sounds_from`), eased in over `BED_IN_S` from the first sample and out over the last `BED_OUT_S`, `BED_UNDER_LU`
/// under the keys (each alone), the two limited together under `ceiling` (`limit`, true peaks) — one gain for the bed,
/// none for the keys, so a click keeps its shape but where the two meet over the ceiling, said (`Bedded`).
fn bedded(keys: Vec<f32>, music: &[f32], ceiling: f32) -> (Vec<f32>, Bedded) {
    let from = sounds_from(music);
    let laid = bed_laid(&music[from..], keys.len());
    let (loud, under) = (lufs(&keys), lufs(&laid));
    // (silent keys or a silent file: the bed as it is)
    let gain = Some(loud - BED_UNDER_LU - under).filter(|g| g.is_finite()).unwrap_or(0.0);
    let g = 10f32.powf(gain / 20.0);
    let mixed: Vec<f32> = keys.iter().zip(&laid).map(|(k, b)| k + b * g).collect();
    let mut y = mixed.clone();
    let (most, at) = limit(&mut y, &true_peaks(&mixed), ceiling);
    let floor = 10f32.powf(-TOUCHED_DB / 20.0);
    let touched = (mixed.iter().zip(&y)).filter(|(m, y)| m.abs() > 1e-6 && y.abs() < m.abs() * floor).count();
    let s = |n: usize| n as f32 / RATE as f32;
    let bed = Bedded {
        from: s(from),
        gain,
        keys: loud,
        bed: under + gain,
        limited: most,
        limited_at: s(at),
        touched: s(touched),
    };
    (y, bed)
}

/// Where `music`'s sound starts (a sample): the first 10 ms whose RMS passes `BED_SOUNDS_DB` — its leading silence left
/// out, never its own swell; 0 where none does.
pub(crate) fn sounds_from(music: &[f32]) -> usize {
    let w = RATE as usize / 100;
    let floor = 10f32.powf(BED_SOUNDS_DB / 20.0);
    music.chunks(w).position(|c| rms(c) > floor).map_or(0, |k| k * w)
}

/// `music` laid for a track `len` samples long: eased in over `BED_IN_S` from its first sample, out over the last
/// `BED_OUT_S` to its end (linear), silent past the music's end.
fn bed_laid(music: &[f32], len: usize) -> Vec<f32> {
    let ramp = |s: f32| (s * RATE as f32).max(1.0);
    let (into, out) = (ramp(BED_IN_S), ramp(BED_OUT_S));
    (0..len)
        .map(|n| {
            let x = music.get(n).copied().unwrap_or(0.0);
            x * (n as f32 / into).min(1.0) * ((len - n) as f32 / out).min(1.0)
        })
        .collect()
}

/// `keys` with the brush laid over them (from its sample) brought to `target` LUFS, their peaks held under `ceiling`
/// (`limit`): the keys by one gain — never past where the limiter would take more than `most_db` from them; the track
/// quieter than `target` there —, the brush at the loudness over them it was made at (`brush`'s level against a key) — a
/// limiter takes more from a key's click than from the brush's swish, and more from the tap than from a click, so one
/// gain for both would move it —, the two limited again together. Each gain found again after the limiter took its
/// share (`reach`). The mix, the keys' gain in dB, and what the limiter took. (`keys`, the brush from its sample, their
/// true peaks and loudness — `Keys`'s, found once.)
#[cfg(test)]
fn master(
    keys: &[f32],
    brush: Option<(usize, &[f32])>,
    target: f32,
    ceiling: f32,
    most_db: f32,
) -> (Vec<f32>, f32, Limited) {
    let tp = (true_peaks(keys), brush.map(|b| true_peaks(b.1)));
    let (at, brush_at) = (lufs(keys), brush.map_or(f32::NEG_INFINITY, |b| lufs(b.1)));
    Prepared { keys, brush, tp: (&tp.0, tp.1.as_deref()), at, brush_at }.master(target, ceiling, most_db)
}

struct Prepared<'a> {
    keys: &'a [f32],
    brush: Option<(usize, &'a [f32])>,
    tp: (&'a [f32], Option<&'a [f32]>),
    at: f32,
    brush_at: f32,
}

impl Prepared<'_> {
    fn master(&self, target: f32, ceiling: f32, most_db: f32) -> (Vec<f32>, f32, Limited) {
        let made = |db: f32| self.made(db, ceiling);
        let cap = self.cap(ceiling, most_db);
        let ((mut y, mut limited), mut db) = reach(target, made(0.0).1, made);
        if db > cap {
            ((y, limited), _) = made(cap);
            db = cap;
        }
        (y, db, limited)
    }

    /// The keys' gain past which the limiter takes more than `most_db` from their loudest true peak under `ceiling`.
    fn cap(&self, ceiling: f32, most_db: f32) -> f32 {
        let loudest = self.tp.0.iter().fold(0.0f32, |m, p| m.max(*p));
        20.0 * (ceiling / loudest).log10() + most_db
    }

    /// `x` at `db`, limited under `ceiling` (its true peaks `peaks`, the gain's times x's); its loudness.
    fn scaled(x: &[f32], peaks: &[f32], db: f32, ceiling: f32) -> ((Vec<f32>, (f32, usize)), f32) {
        let g = 10f32.powf(db / 20.0);
        let mut y: Vec<f32> = x.iter().map(|v| v * g).collect();
        let took = limit(&mut y, peaks, ceiling / g);
        let l = lufs(&y);
        ((y, took), l)
    }

    /// The keys at `db` and the brush at its loudness over them, limited under `ceiling`; the mix's loudness.
    fn made(&self, db: f32, ceiling: f32) -> ((Vec<f32>, Limited), f32) {
        let (keys, (at, brush_at)) = (self.keys, (self.at, self.brush_at));
        let ((mut y, took), laid) = Self::scaled(keys, self.tp.0, db, ceiling);
        let mut limited = Limited { keys: took.0, keys_at: took.1 as f32 / RATE as f32, brush: None };
        if let (Some((from, b)), Some(peaks)) = (self.brush, self.tp.1) {
            // (the brush its loudness over the keys, or the keys' gain where they're silent)
            let ((b, own), _) = match laid.is_finite() {
                true => reach(laid + brush_at - at, brush_at, |db| Self::scaled(b, peaks, db, ceiling)),
                false => Self::scaled(b, peaks, db, ceiling),
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
    }
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
pub(crate) fn limit(x: &mut [f32], peaks: &[f32], ceiling: f32) -> (f32, usize) {
    most(&limit_sides(&mut [x], peaks, ceiling))
}

/// The largest reduction of a limiter's `gains` (dB, positive) and the sample it's at.
pub(crate) fn most(gains: &[f32]) -> (f32, usize) {
    let (mut least, mut at) = (1.0f32, 0);
    for (i, &g) in gains.iter().enumerate() {
        if g < least {
            (least, at) = (g, i);
        }
    }
    (-20.0 * least.log10(), at)
}

/// `limit` on a track's sides at once (the cut's left and right): one gain at each sample for every side, from
/// `peaks` — the larger side's true peak there (`true_peaks_of`) —, so the image never leans. Its gains, one a
/// sample. One side: `limit`.
pub(crate) fn limit_sides(sides: &mut [&mut [f32]], peaks: &[f32], ceiling: f32) -> Vec<f32> {
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
    for x in sides.iter_mut() {
        x.iter_mut().zip(&gains).for_each(|(v, g)| *v *= g);
    }
    gains
}

/// The true peak at each sample of a track's sides: the larger side's (`true_peaks` of each).
pub(crate) fn true_peaks_of(sides: &[&[f32]]) -> Vec<f32> {
    let mut each = sides.iter().map(|x| true_peaks(x));
    let first = each.next().unwrap_or_default();
    each.fold(first, |mut m, p| {
        m.iter_mut().zip(p).for_each(|(m, p)| *m = m.max(p));
        m
    })
}

/// A track's true peak (dBTP): its larger side's (the cut reads it bounded, `true_peak_after`).
#[cfg(test)]
pub(crate) fn true_peak_db(sides: &[&[f32]]) -> f32 {
    20.0 * true_peaks_of(sides).into_iter().fold(0.0f32, f32::max).log10()
}

/// The two sides' loudness together (LUFS): each side's mean square, as `lufs` gates it, summed (the rain's
/// figures and the cut's, one way for both).
pub(crate) fn stereo_lufs(l: &[f32], r: &[f32]) -> f32 {
    let power = |lufs: f32| 10f64.powf((f64::from(lufs) + 0.691) / 10.0);
    (-0.691 + 10.0 * (power(lufs(l)) + power(lufs(r))).log10()) as f32
}

/// `short_term` over both sides: each window's two mean squares summed, as BS.1770 sums channels.
pub(crate) fn stereo_short_term(l: &[f32], r: &[f32], window_s: f32, step_s: f32) -> Vec<f32> {
    let power = |lufs: f32| 10f64.powf((f64::from(lufs) + 0.691) / 10.0);
    (short_term(l, window_s, step_s).into_iter().zip(short_term(r, window_s, step_s)))
        .map(|(a, b)| (-0.691 + 10.0 * (power(a) + power(b)).log10()) as f32)
        .collect()
}

/// The left and right's correlation over the whole (1: the same, as mono; 0: unrelated; −1: opposed); 1 where one
/// side is silent throughout and the other too.
pub(crate) fn correlation(l: &[f32], r: &[f32]) -> f32 {
    let (mut lr, mut ll, mut rr) = (0.0f64, 0.0f64, 0.0f64);
    for (a, b) in l.iter().zip(r) {
        let (a, b) = (f64::from(*a), f64::from(*b));
        (lr, ll, rr) = (lr + a * b, ll + a * a, rr + b * b);
    }
    match ll * rr {
        0.0 if ll == rr => 1.0,
        0.0 => 0.0,
        p => (lr / p.sqrt()) as f32,
    }
}

/// The true peak at each sample of `x`: the largest of it, its neighbours and the three points a quarter apart on
/// either side, 4× oversampled by a Hann-windowed sinc over 16 samples (BS.1770's way to the peaks between samples —
/// what a player's converter, and AAC after it, reaches).
pub(crate) fn true_peaks(x: &[f32]) -> Vec<f32> {
    let phases = phases();
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

/// `true_peaks`' half width: a sample's true peak reads its neighbours this far off on either side.
const TAPS: i32 = 8;

/// `true_peaks`' three interpolators, a quarter, a half and three quarters past a sample.
fn phases() -> [Vec<f32>; 3] {
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
    [phase(0.25), phase(0.5), phase(0.75)]
}

/// The blocks `true_peak_after` bounds (samples).
const BOUND_BLOCK: usize = 512;

/// The true peak (linear) of each of `ranges` of `after` — a track's sides once a limiter took `gains` from them — as
/// `true_peak_db` would find it over that range alone (`after[a..b]`), without reading all of it: `peaks` are the sides'
/// true peaks before (`true_peaks_of`). Over a block, the limited signal's interpolated value at a sample is at most
/// the block's largest gain times its peak before, plus the gains' spread times the interpolators' summed size times
/// the largest sample there (|Σ w·g·x| ≤ gₘₐₓ·|Σ w·x| + Σ |w|·(gₘₐₓ − g)·|x|): blocks are read exactly, the
/// highest bound first, until none left could pass the peak found. A range's first and last block are always read
/// (outside a range is silence to it, which no peak before bounds). The same figure as `true_peak_db`'s, bit for bit.
pub(crate) fn true_peak_after(
    after: &[&[f32]],
    peaks: &[f32],
    gains: &[f32],
    ranges: &[(usize, usize)],
) -> Vec<f32> {
    let w = phases().iter().map(|p| p.iter().map(|w| w.abs()).sum::<f32>()).fold(0.0f32, f32::max);
    let pad = TAPS as usize;
    let n = peaks.len();
    // (each block's bound, from the whole: its gains' range and largest sample padded by `TAPS` either side)
    let blocks = n.div_ceil(BOUND_BLOCK);
    let bounds: Vec<f32> = (0..blocks)
        .map(|k| {
            let (s, e) = (k * BOUND_BLOCK, ((k + 1) * BOUND_BLOCK).min(n));
            let (ps, pe) = (s.saturating_sub(pad), (e + pad).min(n));
            let (mut lo, mut hi, mut x) = (f32::INFINITY, 0.0f32, 0.0f32);
            for i in ps..pe {
                (lo, hi) = (lo.min(gains[i]), hi.max(gains[i]));
                // (the sample before the limiter: after over its gain, or the peak before where the gain is 0)
                for side in after {
                    x = x.max(if gains[i] > 0.0 { side[i].abs() / gains[i] } else { peaks[i] });
                }
            }
            let p = peaks[s..e].iter().fold(0.0f32, |m, p| m.max(*p));
            // (a margin for the bound's own rounding)
            (hi * p + (hi - lo) * w * x) * 1.001 + 1e-9
        })
        .collect();
    let exact = |a: usize, b: usize, s: usize, e: usize| -> f32 {
        // (read over the range as `true_peak_db` would, padded so the block's own samples see what they'd see there)
        let (ps, pe) = (s.saturating_sub(pad).max(a), (e + pad).min(b));
        let mut m = 0.0f32;
        for side in after {
            let tp = true_peaks(&side[ps..pe]);
            m = tp[s - ps..e - ps].iter().fold(m, |m, p| m.max(*p));
        }
        m
    };
    (ranges.iter())
        .map(|&(a, b)| {
            let (a, b) = (a.min(n), b.min(n));
            if a >= b {
                return 0.0;
            }
            let (first, last) = (a / BOUND_BLOCK, (b - 1) / BOUND_BLOCK);
            let span = |k: usize| ((k * BOUND_BLOCK).max(a), ((k + 1) * BOUND_BLOCK).min(b));
            // (the edges: within `TAPS` of the range's ends, read)
            let mut found = 0.0f32;
            let mut rest = Vec::new();
            for k in first..=last {
                let (s, e) = span(k);
                match s < a + pad || e + pad > b {
                    true => found = found.max(exact(a, b, s, e)),
                    false => rest.push(k),
                }
            }
            rest.sort_by(|i, j| bounds[*j].total_cmp(&bounds[*i]));
            for k in rest {
                if bounds[k] <= found {
                    break;
                }
                let (s, e) = span(k);
                found = found.max(exact(a, b, s, e));
            }
            found
        })
        .collect()
}

/// The integrated loudness of `x` (mono at `RATE`), LUFS, as ITU-R BS.1770-4 measures it: K-weighted, in 400 ms blocks
/// a 100 ms step apart, gated at −70 LUFS and then 10 LU under the blocks kept. −∞ where no block is kept.
pub(crate) fn lufs(x: &[f32]) -> f32 {
    let k = k_weighted(x);
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

/// The loudness of each `window_s` of `x` stepped by `step_s` (LUFS, K-weighted, ungated — BS.1770's short-term at
/// 3 s): the first from 0, the last ending at or before `x`'s end. −∞ for a silent one.
pub(crate) fn short_term(x: &[f32], window_s: f32, step_s: f32) -> Vec<f32> {
    let k = k_weighted(x);
    let mut sums = vec![0.0f64; k.len() + 1];
    for (i, v) in k.iter().enumerate() {
        sums[i + 1] = sums[i] + v * v;
    }
    let (w, step) = ((window_s * RATE as f32) as usize, ((step_s * RATE as f32) as usize).max(1));
    (0..)
        .map(|i| i * step)
        .take_while(|s| s + w <= k.len())
        .map(|s| (-0.691 + 10.0 * ((sums[s + w] - sums[s]) / w as f64).log10()) as f32)
        .collect()
}

/// `x` K-weighted as BS.1770 hears it: the head's shelf, then the high-pass (their coefficients at 48 kHz).
fn k_weighted(x: &[f32]) -> Vec<f64> {
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
    biquad(&shelf, [1.0, -2.0, 1.0], [-1.990_047_454_833_98, 0.990_072_250_366_21])
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
    wav_sides(path, &[samples])
}

/// 16-bit PCM, a channel a side (each as long as the first), interleaved: one side, `wav`.
pub(crate) fn wav_sides(path: &Path, sides: &[&[f32]]) -> Result<()> {
    let (n, ch) = (sides.first().map_or(0, |s| s.len()), sides.len() as u16);
    let data = n as u32 * 2 * u32::from(ch);
    let mut b: Vec<u8> = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&ch.to_le_bytes());
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 2 * u32::from(ch)).to_le_bytes());
    b.extend_from_slice(&(2 * ch).to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for i in 0..n {
        for s in sides {
            let s = s.get(i).copied().unwrap_or(0.0);
            b.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
        }
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

    /// `short_term`: each 3 s window stepped by a second — silence then a sine 20 dB down reads nothing, then a third
    /// of the sine (4.77 LU under it), then the sine's own −23.01.
    #[test]
    fn the_short_term_loudness_is_each_windows_own() {
        let n = RATE as usize;
        let x: Vec<f32> = (0..6 * n)
            .map(|i| match i < 3 * n {
                true => 0.0,
                false => 0.1 * (2.0 * std::f32::consts::PI * 997.0 * i as f32 / RATE as f32).sin(),
            })
            .collect();
        let w = short_term(&x, 3.0, 1.0);
        assert_eq!(w.len(), 4, "{w:?}");
        assert_eq!(w[0], f32::NEG_INFINITY);
        assert!((w[1] + 23.01 + 4.77).abs() < 0.1, "{w:?}");
        assert!((w[3] + 23.01).abs() < 0.05, "{w:?}");
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
        let t = track(&keys, Duration::from_secs(12), Some(Duration::ZERO), SHORT_CEILING_DB, None);
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
        // (the end card's brush: the keys all before it, the track ending ~0.3 s after it — over the rest still said, in
        // balance)
        let end = track(
            &keys,
            Duration::from_millis(13_000),
            Some(Duration::from_millis(11_000)),
            SHORT_CEILING_DB,
            None,
        );
        let (before, after) = end.brush.expect("the end card's brush");
        assert!(
            before.is_finite() && (after - before).abs() < 0.5,
            "the end card's brush {before:+.1} LU over the keys before, {after:+.1} after"
        );
    }

    /// A Short's music bed (CLAUDE.md's Shorts bullet, the user's hand mix of 2026-11-05): from where its sound starts,
    /// its leading silence left out; `BED_UNDER_LU` under the keys, each alone, the keys as mastered without it; eased in
    /// over `BED_IN_S` from the first sample and out over the last `BED_OUT_S`; the two under the ceiling, what the
    /// limiter took from them said and small.
    #[test]
    fn a_shorts_bed_lies_its_level_under_the_keys_from_its_sound_faded_in_and_out() {
        let keys: Vec<(Duration, Heard)> =
            (0..60).map(|i| (Duration::from_millis(3_000 + i * 130), Heard::Key('a'))).collect();
        // (a quarter second of silence, then a tone)
        let quiet = RATE as usize / 4;
        let music: Vec<f32> = (0..RATE as usize * 20)
            .map(|n| match n < quiet {
                true => 0.0,
                false => 0.2 * (std::f32::consts::TAU * 220.0 * n as f32 / RATE as f32).sin(),
            })
            .collect();
        assert_eq!(sounds_from(&music), quiet, "its silence left out");
        let length = Duration::from_secs(14);
        let t = track(&keys, length, None, SHORT_CEILING_DB, Some(&music));
        let alone = track(&keys, length, None, SHORT_CEILING_DB, None);
        let b = t.bed.expect("the bed");
        assert!(alone.bed.is_none());
        assert_eq!(b.from, 0.25);
        assert!((b.keys - alone.lufs).abs() < 1e-3, "the keys as mastered alone: {:.2}", b.keys);
        assert!((b.keys - b.bed - BED_UNDER_LU).abs() < 0.05, "{:.2} LU under", b.keys - b.bed);
        // (the bed alone where no key sounds: the first 3 s, the last 2.5)
        let bed: Vec<f32> = t.samples.iter().zip(&alone.samples).map(|(m, k)| m - k).collect();
        let at = |s: f32| rms(&bed[(s * RATE as f32) as usize..][..RATE as usize / 50]);
        let full = at(2.0);
        assert!(at(0.0) < full * 0.03, "eased in from nothing");
        assert!((at(0.24) / full - 0.5).abs() < 0.05, "half way in at 0.25 s: {:.2}", at(0.24) / full);
        assert!((at(14.0 - 1.26) / full - 0.5).abs() < 0.05, "half way out 1.25 s before the end");
        assert!(bed.last().unwrap().abs() < full * 0.01, "out by the end");
        assert!(t.true_peak <= SHORT_CEILING_DB + 0.01, "true peak {:.2} dBTP", t.true_peak);
        assert!(
            b.limited < 1.0 && b.touched < 1.0,
            "the limiter took {:.2} dB, {:.2} s",
            b.limited,
            b.touched
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

    /// The keys made once (`Keys`) and mastered at each ceiling are what `track` makes there: where the gain is capped
    /// (sparse keys), the unit moved by the ceiling — at the first ceiling the same samples; at another the keys within a
    /// millionth of the ceiling, the brush's stretch within 1 % (its level found to 0.05 LU), the same gain and loudness
    /// within 0.01 —; where it isn't (keys dense enough to reach `SHORT_LUFS`), mastered again, the same samples.
    #[test]
    fn the_keys_made_once_are_the_track_at_every_ceiling() {
        let ms = Duration::from_millis;
        let brush_at = Some(ms(3_000));
        for (every, capped) in [(400, true), (25, false)] {
            let heard: Vec<(Duration, Heard)> = (0..(3_000 / every))
                .map(|i| (ms(200 + i * every), Heard::Key(['a', ' ', '\r'][(i % 3) as usize])))
                .collect();
            let length = ms(4_500);
            let keys = Keys::new(&heard, length, brush_at);
            let unit = keys.unit();
            for ceiling in [SHORT_CEILING_DB, -2.6, -3.4] {
                assert_eq!(unit.lufs + ceiling < SHORT_LUFS, capped, "every {every} ms at {ceiling}");
                let (got, want) =
                    (keys.mastered(&unit, ceiling), track(&heard, length, brush_at, ceiling, None));
                let c = 10f32.powf(ceiling / 20.0);
                assert_eq!(got.samples.len(), want.samples.len());
                if ceiling == SHORT_CEILING_DB || !capped {
                    assert!(
                        got.samples == want.samples,
                        "every {every} ms at {ceiling}: not the same samples"
                    );
                }
                // (the brush from 3 s, with the tenth of a second round it the two are limited in)
                let brush = RATE as usize * 29 / 10;
                let off = |r: std::ops::Range<usize>| {
                    (got.samples[r.clone()].iter().zip(&want.samples[r]))
                        .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()))
                };
                let (keys_off, brush_off) = (off(0..brush), off(brush..want.samples.len()));
                assert!(keys_off <= c * 1e-6, "every {every} ms at {ceiling}: a key's sample {keys_off} off");
                assert!(brush_off <= c * 0.01, "every {every} ms at {ceiling}: the brush's {brush_off} off");
                assert!((got.gain - want.gain).abs() < 0.01, "gain {} against {}", got.gain, want.gain);
                assert!((got.lufs - want.lufs).abs() < 0.01, "{} LUFS against {}", got.lufs, want.lufs);
                assert!((got.limited - want.limited.keys).abs() < 0.01, "{} dB taken", got.limited);
            }
        }
    }

    /// The true peak after a limiter, bounded from the peaks before it (`true_peak_after`), is `true_peak_db`'s over
    /// each range, bit for bit: a track limited as the cut limits it, and gains at random (the bound holds for any), over
    /// the whole, ranges starting and ending inside blocks, one shorter than the interpolators and an empty one.
    #[test]
    fn the_true_peak_after_the_limiter_bounded_is_the_true_peak() {
        let mut seed = 7u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let n = RATE as usize * 3;
        // (quiet noise with loud bursts here and there, two sides apart)
        let mut side = || -> Vec<f32> {
            (0..n)
                .map(|i| {
                    let burst = (i / 4_000) % 5 == 2;
                    (rnd() - 0.5) * if burst { 2.4 } else { 0.2 }
                })
                .collect()
        };
        let (l0, r0) = (side(), side());
        let ranges = [(0, n), (1_000, 77_777), (40_100, 40_110), (90_000, 90_000), (100_003, n)];
        let check = |gains: &[f32], what: &str| {
            let (l, r): (Vec<f32>, Vec<f32>) = (
                l0.iter().zip(gains).map(|(x, g)| x * g).collect(),
                r0.iter().zip(gains).map(|(x, g)| x * g).collect(),
            );
            let peaks = true_peaks_of(&[&l0, &r0]);
            let got = true_peak_after(&[&l, &r], &peaks, gains, &ranges);
            for (&(a, b), got) in ranges.iter().zip(got) {
                let want = match a < b {
                    true => true_peaks_of(&[&l[a..b], &r[a..b]]).into_iter().fold(0.0f32, f32::max),
                    false => 0.0,
                };
                assert_eq!(got.to_bits(), want.to_bits(), "{what}, {a}..{b}: {got} against {want}");
            }
        };
        let (mut l, mut r) = (l0.clone(), r0.clone());
        let peaks = true_peaks_of(&[&l0, &r0]);
        let gains = limit_sides(&mut [&mut l, &mut r], &peaks, 10f32.powf(-2.0 / 20.0));
        assert!(most(&gains).0 > 6.0, "the limiter at work: {:.1} dB", most(&gains).0);
        check(&gains, "limited");
        let wild: Vec<f32> = (0..n).map(|_| 0.05 + 0.95 * rnd()).collect();
        check(&wild, "gains at random");

        // (designed: the half-way interpolator's taps around `at`, each sample `sign` of its weight's sign)
        let b = BOUND_BLOCK;
        let half = &phases()[1];
        let pattern = |x: &mut [f32], at: usize, sign: &dyn Fn(i64) -> f32| {
            for (j, w) in half.iter().enumerate() {
                let k = j as i64 - i64::from(TAPS) + 1;
                x[(at as i64 + k) as usize] = w.signum() * sign(k);
            }
        };
        let peak =
            |x: &[f32], r: std::ops::Range<usize>| true_peaks(x)[r].iter().fold(0.0f32, |m, p| m.max(*p));
        let n = b * 8;
        // (a cluster whose taps cancel before the limiter, some then taken down: the peak after is over the block's
        // largest gain times its peaks before — the gains' spread is the bound's; found among small random ones)
        let c = b * 5 / 2;
        let (mut x, mut g, mut after) = (vec![0.0f32; n], vec![1.0f32; n], Vec::new());
        let (mut before, mut over) = (0.0, 0.0);
        for _ in 0..20_000 {
            for i in c - 12..c + 12 {
                (x[i], g[i]) = (rnd() * 2.0 - 1.0, if rnd() < 0.5 { 1.0 } else { 0.01 });
            }
            after = x.iter().zip(&g).map(|(x, g)| x * g).collect();
            (before, over) = (peak(&x, 2 * b..3 * b), peak(&after, 2 * b..3 * b));
            if over > before * 1.05 {
                break;
            }
        }
        assert!(over > before * 1.05, "the design: {over} after against {before} before");
        // (a louder peak elsewhere than the cluster's before, quieter than its after: read first)
        let mut x2 = after.clone();
        x2[6 * b + 100] = (before + over) / 2.0;
        let p = true_peaks(&{
            let mut x = x.clone();
            x[6 * b + 100] = (before + over) / 2.0;
            x
        });
        let got = true_peak_after(&[&x2], &p, &g, &[(0, n)])[0];
        assert_eq!(got.to_bits(), peak(&x2, 0..n).to_bits(), "the gains' spread: {got}");
        // (a range starting inside a pattern whose left half cancels its right: cut there, its peak is over what the
        // whole's peaks say — a range's edges are read)
        let mut x = vec![0.0f32; n];
        let a = 3 * b + 100;
        pattern(&mut x, a, &|k| if k >= 0 { 1.0 } else { -1.0 });
        let (whole, cut) = (peak(&x, a..4 * b), true_peaks(&x[a..]).into_iter().fold(0.0f32, f32::max));
        assert!(cut > whole * 1.05, "the design: {cut} cut against {whole} whole");
        x[6 * b + 100] = (whole + cut) / 2.0;
        let p = true_peaks(&x);
        let got = true_peak_after(&[&x], &p, &vec![1.0; n], &[(a, n)])[0];
        let want = true_peaks(&x[a..]).into_iter().fold(0.0f32, f32::max);
        assert_eq!(got.to_bits(), want.to_bits(), "a range's edge: {got} against {want}");
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

    /// The cut's limiter (V2b): one gain at every sample for both sides, from the larger side's true peak — a loud
    /// left turns the quiet right down with it, by as much —; the track's true peak is its larger side's.
    #[test]
    fn the_limiter_takes_the_same_gain_from_both_sides_and_the_true_peak_is_the_larger_sides() {
        let n = RATE as usize;
        let sine =
            |hz: f32, a: f32, i: usize| a * (std::f32::consts::TAU * hz * i as f32 / RATE as f32).sin();
        let l0: Vec<f32> =
            (0..n).map(|i| sine(1_000.0, if (n / 3..n / 2).contains(&i) { 0.95 } else { 0.1 }, i)).collect();
        let r0: Vec<f32> = (0..n).map(|i| sine(300.0, 0.2, i)).collect();
        let peaks = true_peaks_of(&[&l0, &r0]);
        let (pl, pr) = (true_peaks(&l0), true_peaks(&r0));
        assert!((0..n).all(|i| peaks[i] == pl[i].max(pr[i])), "the larger side's at each sample");
        let before = true_peak_db(&[&l0, &r0]);
        assert_eq!(before, true_peak_db(&[&l0]).max(true_peak_db(&[&r0])));
        assert!(true_peak_db(&[&l0]) > true_peak_db(&[&r0]) + 6.0, "the left the larger");
        let (mut l, mut r) = (l0.clone(), r0.clone());
        let ceiling = 10f32.powf(-6.0 / 20.0);
        let gains = limit_sides(&mut [&mut l, &mut r], &peaks, ceiling);
        for i in 0..n {
            for (x, y) in [(l0[i], l[i]), (r0[i], r[i])] {
                assert!((y - x * gains[i]).abs() < 1e-7, "at {i}: {y} from {x}, the gain {}", gains[i]);
            }
        }
        let (most, at) = most(&gains);
        assert!(most > 3.0 && (n / 3..n / 2).contains(&at), "{most:.2} dB at {at}");
        assert!(
            r[n * 5 / 12].abs() <= r0[n * 5 / 12].abs() && gains[n * 5 / 12] < 0.7,
            "the right turned down too"
        );
        assert!(true_peak_db(&[&l, &r]) <= -6.0 + 0.05, "{:.2} dBTP", true_peak_db(&[&l, &r]));
    }

    /// The cut's figures over both sides (V2b): the correlation is 1 for the same on both, −1 for opposed, near 0 for
    /// unrelated; a window's loudness over both is its two powers summed — the same on both 3 dB over one, a silent
    /// side adding nothing.
    #[test]
    fn the_sides_correlation_and_loudness_are_both_sides() {
        let n = RATE as usize * 4;
        let x: Vec<f32> =
            (0..n).map(|i| 0.3 * (std::f32::consts::TAU * 440.0 * i as f32 / RATE as f32).sin()).collect();
        let y: Vec<f32> =
            (0..n).map(|i| 0.3 * (std::f32::consts::TAU * 1_013.0 * i as f32 / RATE as f32).sin()).collect();
        let minus: Vec<f32> = x.iter().map(|v| -v).collect();
        let silent = vec![0.0f32; n];
        assert!((correlation(&x, &x) - 1.0).abs() < 1e-6 && (correlation(&x, &minus) + 1.0).abs() < 1e-6);
        assert!(correlation(&x, &y).abs() < 0.01, "{}", correlation(&x, &y));
        assert_eq!((correlation(&x, &silent), correlation(&silent, &silent)), (0.0, 1.0));
        let (one, both, half) = (
            short_term(&x, 3.0, 1.0),
            stereo_short_term(&x, &x, 3.0, 1.0),
            stereo_short_term(&x, &silent, 3.0, 1.0),
        );
        for i in 0..one.len() {
            assert!(
                (both[i] - one[i] - 10.0 * 2f32.log10()).abs() < 1e-3 && (half[i] - one[i]).abs() < 1e-3,
                "{i}"
            );
        }
        assert!((stereo_lufs(&x, &x) - lufs(&x) - 10.0 * 2f32.log10()).abs() < 1e-3);
    }

    /// A Short's sound is the bytes it was before the cut's sound went stereo (V2b): keys of each kind, a stamp, the
    /// brush and a bed (a made tone), mastered, limited and written as its WAV — FNV-1a over the file, taken from the
    /// code as it was. On macOS only: the floats' `sin`, `exp` and `powf` are the platform's libm, and its figure is this
    /// Mac's (`:preflight` runs it at every landing).
    #[test]
    #[cfg(target_os = "macos")]
    fn a_shorts_sound_is_byte_for_byte_what_it_was() {
        let ms = Duration::from_millis;
        let mut heard: Vec<(Duration, Heard)> =
            (0..40).map(|i| (ms(500 + i * 110), Heard::Key(['a', ' ', '\r'][(i % 3) as usize]))).collect();
        heard.push((ms(6_000), Heard::Stamp));
        let music: Vec<f32> = (0..RATE as usize * 12)
            .map(|n| {
                let t = n as f32 / RATE as f32;
                0.3 * (std::f32::consts::TAU * 196.0 * t).sin() * (0.6 + 0.4 * (t * 1.7).sin())
            })
            .collect();
        let t = track(&heard, Duration::from_secs(10), Some(ms(8_000)), SHORT_CEILING_DB, Some(&music));
        let dir = tarae_testkit::scratch::Scratch::new("xtask-short-bytes");
        let at = dir.join("short.wav");
        wav(&at, &t.samples).unwrap();
        let bytes = std::fs::read(&at).unwrap();
        let fnv = bytes
            .iter()
            .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3));
        assert_eq!((bytes.len(), fnv), (960_044, 2_814_364_272_007_694_777), "a Short's WAV changed");
    }
}
