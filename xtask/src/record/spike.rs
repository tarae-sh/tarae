//! `cargo xtask record spike [OUT]` — V0's own take and its measures (the brief, main's `.sb/full-video-plan.md` V0),
//! into `OUT` (default `target/rec/record-spike`, made new). The real `claude` never starts: every tarae here is a
//! scene's — the demo project, a home and state of its own, the scenes' stand-in Claude (`scenes::live`, the take's
//! command refused unless its config names a stand-in, `shot::command`).
//!
//! 1. **The sizes**: tarae started at each of the brief's three sizes (`STILL_SIZES`), and at the sizes that fill the
//!    wide video's room at those rows (`filling`), the chat opened, each a take of its own (`start-<c>x<r>/`) and its
//!    screen as the wide video draws it (`start-<c>x<r>.png`).
//! 2. **The take**: about two minutes driven by `SPIKE_KEYS` (the stand-in's `nine` script: a run asked for outside the
//!    sandbox and answered `y` — the demo's own `cargo test` —, an answer with an edit reviewed, a second checkout
//!    opened as a workspace and brought) at `SIZE`, headless, its landmarks watched (`take/`); drawn as the wide video
//!    (`take/take.mp4`, `take/sheet.png`) and checked (`take/check.txt`).
//! 3. **The cost**: a key in a 200k-line file to its frame, as the perf harness measures it (`tests/perf.rs`'s
//!    `perf_e2e_key_in_a_big_file`: forty keys, the median), on tarae's own terminal and through the recorder with its
//!    landmarks watched, alternating, two rounds each; and its start to its first frame both ways (`bench.txt`).

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use super::{AfterKeys, Outer, Spec, Taken, keys, record};
use crate::scenes::{self, docshots::Bg};
use crate::shot::{self, FRAME_END, PAD};
use crate::{Result, root, video};

/// The brief's three sizes for the stills (columns, rows).
const STILL_SIZES: [(u16, u16); 3] = [(120, 34), (128, 36), (140, 40)];

/// A still's keys: the chat opened beside the code, then a moment for it to settle.
const STILL_KEYS: &[&str] = &["@1", " ", "l", "@2"];

/// The spike's take, on the stand-in's `nine` script (`scenes.rs`'s `claude`): an ask whose run goes outside the
/// sandbox (its card answered `y`: `cargo test --quiet` in the demo crate, its build its own folder), an ask whose
/// answer edits `src/main.rs` (the change answered `y` in its review), then a second checkout made and committed in
/// by `:sh`, opened as a workspace and brought into main.
const SPIKE_KEYS: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "@type:run the tests",
    "\r",
    "@20?Its reason",
    "@1.5",
    "\x11",
    "y",
    "@12",
    "@type:why does insert clone the name?",
    "\r",
    "@20?Change 1 of",
    "@1.5",
    "y",
    "@20?Nothing else",
    "@1.5",
    "\x1b",
    ":",
    "sh git worktree add -q ../fix && echo checkout made",
    "\r",
    "@20?checkout made",
    ":",
    "sh cd ../fix && echo 'Tied by fix.' >> README.md && git -c user.name=demo -c user.email=demo@example.com -c commit.gpgsign=false commit -qam 'fix: the readme' && echo fix committed",
    "\r",
    "@20?fix committed",
    ":",
    "workspace-open ../fix",
    "\r",
    "@3",
    ":",
    "workspace-bring",
    "\r",
    "@10?Bring fix",
    "@2",
    "y",
    "@30?Brought",
    "@3",
];

/// The bench's keys, as the perf harness's (`perf_e2e_key_in_a_big_file`), forty of them.
const BENCH_KEYS: [&str; 10] = ["j", "j", "w", "w", "e", "b", "k", "l", "h", "j"];
const BENCH_ROUNDS: usize = 2;
/// The longest a frame is waited for after a key (the harness's), and for the file's first line.
const FRAME_MAX: Duration = Duration::from_secs(2);
const OPEN_MAX: Duration = Duration::from_secs(60);

pub(super) fn run(args: &[String]) -> Result<()> {
    let out = match args {
        [] => super::rec_dir("record-spike"),
        [o] => PathBuf::from(o),
        _ => return Err("usage: cargo xtask record spike [OUT]".into()),
    };
    if out.exists() {
        return Err(format!("{} is there already — name another, or move it away first", out.display()));
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    // (a recording's tarae: the grammars bundled, its homes its own — as `scenes --record`'s)
    let bin = shot::build(&root(), "shot-docs", &["bundled-grammars"])?;
    let me = std::env::current_exe().map_err(|e| format!("this xtask: {e}"))?;
    let mut said = Vec::new();

    // 1. the sizes
    let mut sizes = STILL_SIZES.to_vec();
    for (_, rows) in STILL_SIZES {
        let f = filling(rows)?;
        if !sizes.contains(&f) {
            sizes.push(f);
        }
    }
    for size in sizes {
        said.push(still_at(size, ("idle", STILL_KEYS, "start"), &bin, &me, &out)?);
    }

    // 2. the take
    let dir = out.join("take");
    let live = scenes::live("record-spike", "nine", Bg::Dark, &bin, &me, true)?;
    let cmd = shot::command(&live.setup, &live.state, &live.homes)?;
    let spec = Spec {
        size: super::SIZE,
        out: dir.clone(),
        outer: None,
        keys: keys::of_keys(SPIKE_KEYS)?,
        after_keys: AfterKeys::Quit,
        ui: Some(live.state.clone()),
        cwd: None,
        hands: None,
    };
    let taken = record(spec, cmd)?;
    said.push(taken.said(&dir));
    said.push(per_minute(&taken));
    drop(live);
    said.push(super::draw(&dir, Bg::Dark, "A live take, unedited")?);
    let hits = super::check::scan(&dir, &super::check::Yours::here())?;
    let weak = hits.iter().filter(|h| h.weak).count();
    said.push(format!("check: {} places, {weak} weak (take/check.txt)", hits.len() - weak));
    write(&dir.join("check.txt"), &super::check::listed(&hits))?;

    // 3. the cost
    said.push(bench(&bin, &me, &out)?);

    let all = said.join("\n") + "\n";
    write(&out.join("spike.txt"), &all)?;
    print!("{all}");
    Ok(())
}

fn write(p: &Path, text: &str) -> Result<()> {
    std::fs::write(p, text).map_err(|e| format!("{}: {e}", p.display()))
}

/// The size whose `rows` fill the wide video's room's height at the largest glyph that fits it, and as many columns as
/// its width then holds: the 16:9 frame's own size at those rows.
pub(crate) fn filling(rows: u16) -> Result<(u16, u16)> {
    let fonts = shot::Fonts::load(1)?;
    let (rw, rh) = video::WIDE_ROOM;
    let px = (8..=64)
        .rev()
        .map(|p| p as f32)
        .find(|&p| usize::from(rows) * shot::cell(&fonts, p).1 + 2 * PAD <= rh)
        .ok_or_else(|| format!("{rows} rows don't fit the room at any glyph"))?;
    let cols = (rw - 2 * PAD) / shot::cell(&fonts, px).0;
    Ok((u16::try_from(cols).unwrap_or(u16::MAX), rows))
}

/// The size whose columns fill the wide video's room's width at glyph `px`, and as many rows as its height then holds
/// (`video::fit` gives that glyph back for it).
pub(crate) fn filling_at(px: f32) -> Result<(u16, u16)> {
    let fonts = shot::Fonts::load(1)?;
    let (rw, rh) = video::WIDE_ROOM;
    let (cw, ch) = shot::cell(&fonts, px);
    let size = ((rw - 2 * PAD) / cw, (rh - 2 * PAD) / ch);
    Ok((u16::try_from(size.0).unwrap_or(u16::MAX), u16::try_from(size.1).unwrap_or(u16::MAX)))
}

/// The brief's glyphs for the busy screen (V1's "The size, on a busy frame": 17, 18, 19 px).
pub(crate) const BUSY_PX: [f32; 3] = [17.0, 18.0, 19.0];

/// The busy screen's keys: the spike's take up to its run's card answered `y` and the run's panel settled — the demo's
/// code, `cargo test --quiet`'s panel under it and the chat beside (V0's `look-40.png`).
const BUSY_KEYS: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "@type:run the tests",
    "\r",
    "@20?Its reason",
    "@1.5",
    "\x11",
    "y",
    "@20?test result",
    "@2",
];

/// `record sizes [OUT]` (default `target/rec/record-sizes`, made new): the busy screen (`BUSY_KEYS`, the stand-in's
/// `nine` script) at the size that fills the wide video at each of `BUSY_PX` (`filling_at`), each a take of its own
/// (`busy-<c>x<r>/`) and its picture as the wide video draws it (`busy-<c>x<r>.png`); `sizes.txt` says each.
pub(super) fn sizes(args: &[String]) -> Result<()> {
    let out = match args {
        [] => super::rec_dir("record-sizes"),
        [o] => PathBuf::from(o),
        _ => return Err("usage: cargo xtask record sizes [OUT]".into()),
    };
    if out.exists() {
        return Err(format!("{} is there already — name another, or move it away first", out.display()));
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let bin = shot::build(&root(), "shot-docs", &["bundled-grammars"])?;
    let me = std::env::current_exe().map_err(|e| format!("this xtask: {e}"))?;
    let mut said = Vec::new();
    for px in BUSY_PX {
        let size = filling_at(px)?;
        said.push(format!("{px} px: {}×{}", size.0, size.1));
        said.push(still_at(size, ("nine", BUSY_KEYS, "busy"), &bin, &me, &out)?);
    }
    let all = said.join("\n") + "\n";
    write(&out.join("sizes.txt"), &all)?;
    print!("{all}");
    Ok(())
}

/// tarae at `size` on the stand-in's `script`, driven by `keys` (the spike's start: the chat opened, `STILL_KEYS`), a
/// take of its own in `out/<name>-<c>x<r>/`, and its screen just before the quit drawn as the wide video draws it
/// (`super::still`) at `out/<name>-<c>x<r>.png`. Said: `still`'s line.
fn still_at(
    size: (u16, u16),
    (script, keys, name): (&'static str, &[&str], &str),
    bin: &Path,
    me: &Path,
    out: &Path,
) -> Result<String> {
    let name = format!("{name}-{}x{}", size.0, size.1);
    let dir = out.join(&name);
    let live = scenes::live("record-spike", script, Bg::Dark, bin, me, false)?;
    let cmd = shot::command(&live.setup, &live.state, &live.homes)?;
    let spec = Spec {
        size,
        out: dir.clone(),
        outer: None,
        keys: keys::of_keys(keys)?,
        after_keys: AfterKeys::Quit,
        ui: None,
        cwd: None,
        hands: None,
    };
    record(spec, cmd)?;
    // (the quit's esc: the script's last record but its `:q!`)
    let keys = super::read_log(&dir.join("keys.bin"))?;
    let esc = keys.iter().rev().nth(1).map(|r| r.us / 1000).ok_or("the still's take sent no quit")?;
    super::still(&dir, &out.join(format!("{name}.png")), Some(esc.saturating_sub(1)), Bg::Dark)
}

/// A take's bytes and landmarks a minute.
fn per_minute(t: &Taken) -> String {
    let min = t.ms as f64 / 60_000.0;
    format!(
        "a minute: {:.0} KB of screen, {:.0} bytes of keys, {:.1} landmarks, {:.1} socket lines",
        t.screen_bytes as f64 / min / 1024.0,
        t.key_bytes as f64 / min,
        t.landmarks as f64 / min,
        t.events as f64 / min
    )
}

// ── The cost ──────────────────────────────────────────────────────────────────────────────────────────────────────────

/// A terminal a driver reads and types into, as the perf harness's: what came, with when, and the screen drawn from it.
struct Driver {
    master: File,
    rx: Receiver<(Instant, Vec<u8>)>,
    parser: vt100::Parser,
    whole: super::Whole,
}

impl Driver {
    fn on(master: File, size: (u16, u16)) -> Result<Driver> {
        let mut from = master.try_clone().map_err(|e| format!("pty: {e}"))?;
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("record-bench-reader".into())
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                while let Ok(n) = from.read(&mut buf) {
                    if n == 0 || tx.send((Instant::now(), buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("the bench's reader: {e}"))?;
        Ok(Driver {
            master,
            rx,
            parser: vt100::Parser::new(size.1, size.0, 0),
            whole: super::Whole::default(),
        })
    }

    fn took(&mut self, bytes: &[u8]) {
        let ready = self.whole.take(bytes);
        super::feed(&mut self.parser, &ready);
    }

    /// The first frame's end after `since`, by `max`: how long after.
    fn frame_end(&mut self, since: Instant, max: Duration) -> Option<Duration> {
        let mut tail: Vec<u8> = Vec::new();
        loop {
            let left = (since + max).checked_duration_since(Instant::now())?;
            let (at, chunk) = self.rx.recv_timeout(left).ok()?;
            self.took(&chunk);
            tail.extend_from_slice(&chunk);
            if tail.windows(FRAME_END.len()).any(|w| w == FRAME_END) {
                return Some(at.saturating_duration_since(since));
            }
            let keep = tail.len().saturating_sub(FRAME_END.len());
            tail.drain(..keep);
        }
    }

    /// Until `text` is on the screen, by `max`.
    fn wait_text(&mut self, text: &str, max: Duration) -> bool {
        let end = Instant::now() + max;
        while !self.parser.screen().contents().contains(text) {
            let Some(left) = end.checked_duration_since(Instant::now()) else { return false };
            match self.rx.recv_timeout(left) {
                Ok((_, chunk)) => self.took(&chunk),
                Err(_) => return false,
            }
        }
        true
    }

    /// What came in `d`, taken (the whole-screen redraw after the first frames, as the harness lets it pass).
    fn settle(&mut self, d: Duration) {
        let end = Instant::now() + d;
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            match self.rx.recv_timeout(left) {
                Ok((_, chunk)) => self.took(&chunk),
                Err(_) => break,
            }
        }
    }

    /// A key to its frame, as the harness's `key_to_frame`: what's waiting taken first, the key sent, the time to the
    /// next frame's end.
    fn key_to_frame(&mut self, key: &[u8]) -> Option<Duration> {
        while let Ok((_, chunk)) = self.rx.try_recv() {
            self.took(&chunk);
        }
        let t0 = Instant::now();
        self.master.write_all(key).ok()?;
        self.frame_end(t0, FRAME_MAX)
    }

    /// Forty keys (`BENCH_KEYS`), each to its frame (ms).
    fn keys(&mut self) -> Vec<f64> {
        let keys = BENCH_KEYS.iter().cycle().take(40);
        keys.filter_map(|k| self.key_to_frame(k.as_bytes())).map(|d| d.as_secs_f64() * 1000.0).collect()
    }

    fn quit(&mut self) {
        let _ = self.master.write_all(b"\x1b");
        self.settle(Duration::from_millis(60));
        let _ = self.master.write_all(b":q!\r");
    }
}

/// One round's measure: the start to its first frame (ms) and each key's to its frame.
type Round = (Option<f64>, Vec<f64>);

/// The bench (this file's top, 3.): tarae on a 200k-line file, on its own terminal and through the recorder, in turns.
fn bench(bin: &Path, me: &Path, out: &Path) -> Result<String> {
    let size = super::SIZE;
    let (mut direct, mut through) = (Vec::new(), Vec::new());
    for round in 0..BENCH_ROUNDS {
        direct.push(bench_direct(bin, me, size)?);
        through.push(bench_recorded(bin, me, size, &out.join(format!("bench-{round}")))?);
    }
    let med = |v: Vec<f64>| -> Option<f64> {
        let mut v = v;
        v.sort_by(f64::total_cmp);
        v.get(v.len() / 2).copied()
    };
    let side = |rounds: &[Round]| {
        let keys: Vec<f64> = rounds.iter().flat_map(|r| r.1.clone()).collect();
        let n = keys.len();
        let starts: Vec<f64> = rounds.iter().filter_map(|r| r.0).collect();
        (med(keys), n, med(starts))
    };
    let (d, t) = (side(&direct), side(&through));
    let f = |v: Option<f64>| v.map_or("—".into(), |v| format!("{v:.2} ms"));
    let text = format!(
        "the cost (a key in a 200k-line file to its frame, the median of {} keys; the start to the first frame, the \
         median of {BENCH_ROUNDS}):\n  tarae's own terminal: key {}, start {}\n  through the recorder, landmarks \
         watched: key {} ({} keys), start {}\n",
        d.1,
        f(d.0),
        f(d.2),
        f(t.0),
        t.1,
        f(t.2)
    );
    write(&out.join("bench.txt"), &text)?;
    Ok(text)
}

/// The bench's tarae: a scene's, on `big.rs` (200k lines, as the harness's `big_file`).
fn bench_live(bin: &Path, me: &Path) -> Result<(scenes::Live, Command)> {
    let mut live = scenes::live("record-spike", "idle", Bg::Dark, bin, me, false)?;
    let text: String = (0..200_000)
        .map(|i| format!("fn f{i}(x: u32) -> u32 {{ let y = x + {i}; y * 2 }} // 타래\n"))
        .collect();
    write(&live.setup.cwd.join("big.rs"), &text)?;
    live.setup.files = vec!["big.rs".into()];
    let cmd = shot::command(&live.setup, &live.state, &live.homes)?;
    Ok((live, cmd))
}

fn bench_direct(bin: &Path, me: &Path, size: (u16, u16)) -> Result<Round> {
    let (_live, mut cmd) = bench_live(bin, me)?;
    let (master, slave) = crate::pty::open(size.1, size.0).map_err(|e| format!("pty: {e}"))?;
    let stdio = || slave.try_clone().map(Stdio::from).map_err(|e| format!("pty: {e}"));
    cmd.stdin(stdio()?).stdout(stdio()?).stderr(stdio()?);
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            shot::cvt(libc::setsid())?;
            shot::cvt(libc::ioctl(0, libc::TIOCSCTTY as _, 0))?;
            Ok(())
        });
    }
    let mut d = Driver::on(File::from(master), size)?;
    let t0 = Instant::now();
    let mut child = cmd.spawn().map_err(|e| format!("tarae: {e}"))?;
    // Drop `cmd`'s copies of the slave too: on Linux the pty's end comes only once no one holds the slave.
    drop(cmd);
    drop(slave);
    let round = drive(&mut d, t0);
    d.quit();
    if !d.wait_exit(&mut child) {
        let _ = child.kill();
        let _ = child.wait();
    }
    round
}

fn bench_recorded(bin: &Path, me: &Path, size: (u16, u16), dir: &Path) -> Result<Round> {
    let (live, cmd) = bench_live(bin, me)?;
    let (master, slave) = crate::pty::open(size.1, size.0).map_err(|e| format!("pty: {e}"))?;
    let outer = Outer { input: slave.try_clone().map_err(|e| format!("pty: {e}"))?, output: slave };
    let spec = Spec {
        size,
        out: dir.to_path_buf(),
        outer: Some(outer),
        keys: Vec::new(),
        after_keys: AfterKeys::Stay,
        ui: Some(live.state.clone()),
        cwd: None,
        hands: None,
    };
    let mut d = Driver::on(File::from(master), size)?;
    let t0 = Instant::now();
    let taking = std::thread::Builder::new()
        .name("record-bench-take".into())
        .spawn(move || record(spec, cmd))
        .map_err(|e| format!("the bench's take: {e}"))?;
    let round = drive(&mut d, t0);
    d.quit();
    let taken = taking.join().map_err(|_| "the bench's take panicked".to_string())??;
    drop(live);
    let (r0, keys) = round?;
    eprintln!("bench: through the recorder — {} landmarks, {} socket lines", taken.landmarks, taken.events);
    Ok((r0, keys))
}

/// A round on a tarae started at `t0`: its first frame, the file's first line, the redraws let pass, forty keys.
fn drive(d: &mut Driver, t0: Instant) -> Result<Round> {
    let start = d.frame_end(t0, OPEN_MAX).map(|t| t.as_secs_f64() * 1000.0);
    if !d.wait_text("fn f1(", OPEN_MAX) {
        return Err(format!("the bench's file never showed:\n{}", d.parser.screen().contents()));
    }
    d.settle(Duration::from_millis(600));
    let keys = d.keys();
    if keys.len() < 30 {
        return Err(format!("frames came for {} of 40 keys", keys.len()));
    }
    Ok((start, keys))
}

impl Driver {
    /// Whether `child` ended within two seconds, its output taken meanwhile (an exiting process whose terminal nobody
    /// reads hangs on macOS).
    fn wait_exit(&mut self, child: &mut std::process::Child) -> bool {
        let end = Instant::now() + Duration::from_secs(2);
        while Instant::now() < end {
            if let Ok(Some(_)) = child.try_wait() {
                return true;
            }
            self.settle(Duration::from_millis(20));
        }
        false
    }
}
