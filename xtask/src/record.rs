//! `cargo xtask record` — a live tarae recorded, and the wide video made from the recording (the full video's V0, main's
//! `.sb/full-video-plan.md`): where a scene's take drives a scripted tarae (`scenes --record`), this keeps a take of
//! whatever happens in a terminal — you at the keys, a key script, or both.
//!
//! - `record [--size 120x34] --out DIR [--keys FILE] [--state DIR] [--headless] -- COMMAND …` runs the command in a
//!   terminal of its own of exactly that size, shown in the terminal it's started from, and keeps a take in `DIR`. A
//!   pass-through: the command's bytes reach your terminal untouched, and what your terminal sends — your keys and its
//!   answers to tarae's questions (the background's colour, the keyboard protocol, the mouse) — reaches the command.
//!   The inner size stays fixed: started in a terminal smaller than it, it refuses and says the size it needs, and a
//!   resize later changes nothing inside. `--keys` plays a key script into it beside whatever you type (`keys.rs`);
//!   `--state` is the state home whose `tarae/ui/<pid>.sock` gives the landmarks (default `XDG_STATE_HOME`, else
//!   `~/.local/state`); `--headless` takes it with no terminal shown (nothing answers tarae's questions: its theme from
//!   `TARAE_BACKGROUND`).
//! - `record draw DIR [--bg dark|light] [--title T]` — the take replayed into its screens at their times and made the
//!   wide video, unedited (`video::make`): `DIR/take.mp4` and `DIR/sheet.png`.
//! - `record still DIR OUT.png [--at S]` — one screen of the take (at `S` seconds; by default its start, once it has
//!   stood still a second) as the wide video draws it, with its glyph size and how much of the room it fills.
//! - `record check DIR` — every place the take's screens show something of yours (`check.rs`).
//! - `record spike [OUT]` — V0's own take and measures (`spike.rs`).
//! - `record sizes [OUT]` — the take's busiest screen (code, a run's panel, the chat) at the sizes that fill the wide
//!   video at 17, 18 and 19 px glyphs (`spike::sizes`).
//!
//! A take, `DIR`:
//! - `take.json` — the size, the command, when it started (Unix ms), how long it ran (ms), its exit, its byte counts,
//!   the waits its key script missed.
//! - `screen.bin` — the terminal's output as the pty gave it, chunk by chunk: records of `[u64 µs since the start]
//!   [u8 whose][u32 length][bytes]`, little-endian (`put`, `read_log`), `whose` 0.
//! - `keys.bin` — the input, the same records, `whose` 1 the terminal (your keys and its answers) or 2 the key script.
//! - `events.jsonl` — what tarae's UI state socket said (`{"watch":[]}`), `{"ms":…,"event":{…}}` a line, and the
//!   part of a snapshot asked after some of them (`{"ms":…,"state":{front, workspaces, cards}}`).
//! - `landmarks.jsonl` — the cut's landmarks drawn from those (`landmarks.rs`), `{"ms":…,"what":…,…}` a line.
//!
//! The logs are written by a worker: the pass-through hands each chunk on and goes back to reading, so recording never
//! makes tarae's frames or your keys wait on a disk.

use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::os::fd::{AsFd, AsRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::scenes::docshots::Bg;
use crate::shot::{self, FRAME_END, Recorded};
use crate::video;
use crate::{Result, root};

mod check;
mod keys;
mod landmarks;
mod spike;
#[cfg(test)]
mod tests;

/// A take's size unless `--size` says (columns, rows).
pub(crate) const SIZE: (u16, u16) = (120, 34);

/// Whose a record is: the screen's, the terminal's input (your keys, its answers), the key script's.
pub(crate) const SCREEN: u8 = 0;
pub(crate) const TERMINAL: u8 = 1;
pub(crate) const SCRIPT: u8 = 2;

const USAGE: &str = "usage: cargo xtask record [--size 120x34] --out DIR [--keys FILE] [--state DIR] [--headless] -- \
                     COMMAND … | record draw DIR [--bg dark|light] [--title T] | record still DIR OUT.png [--at S] | \
                     record check DIR | record spike [OUT] | record sizes [OUT]";

pub fn run(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("check") => return check::run(&args[1..]),
        Some("draw") => return draw_cmd(&args[1..]),
        Some("still") => return still_cmd(&args[1..]),
        Some("spike") => return spike::run(&args[1..]),
        Some("sizes") => return spike::sizes(&args[1..]),
        _ => {}
    }
    let at = args.iter().position(|a| a == "--").ok_or(USAGE)?;
    let (opts, command) = (&args[..at], &args[at + 1..]);
    let program = command.first().ok_or(USAGE)?;
    let (mut size, mut out, mut keys, mut state, mut headless) = (SIZE, None, None, None, false);
    let mut it = opts.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{a} wants a value — {USAGE}"));
        match a.as_str() {
            "--size" => size = parse_size(value()?)?,
            "--out" => out = Some(PathBuf::from(value()?)),
            "--keys" => keys = Some(PathBuf::from(value()?)),
            "--state" => state = Some(PathBuf::from(value()?)),
            "--headless" => headless = true,
            other => return Err(format!("{other}: {USAGE}")),
        }
    }
    let out = out.ok_or_else(|| format!("--out DIR: where the take goes — {USAGE}"))?;
    let keys = match keys {
        Some(f) => keys::parse(&std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?)?,
        None => Vec::new(),
    };
    let outer = match headless {
        true => None,
        false => Some(Outer::stdio()?),
    };
    let ui = state.or_else(state_home);
    let mut cmd = Command::new(program);
    cmd.args(&command[1..]);
    let taken = record(Spec { size, out: out.clone(), outer, keys, quit_after_keys: false, ui }, cmd)?;
    eprintln!("{}", taken.said(&out));
    Ok(())
}

/// `120x34` → (120, 34).
pub(crate) fn parse_size(s: &str) -> Result<(u16, u16)> {
    let bad = || format!("--size {s}: columns x rows, as 120x34");
    let (c, r) = s.split_once(['x', '×']).ok_or_else(bad)?;
    let (c, r): (u16, u16) = (c.parse().map_err(|_| bad())?, r.parse().map_err(|_| bad())?);
    if c < 20 || r < 5 {
        return Err(bad());
    }
    Ok((c, r))
}

/// The state home tarae binds its socket under, as it has it: `XDG_STATE_HOME`, else `~/.local/state`.
fn state_home() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
}

// ── The take ──────────────────────────────────────────────────────────────────────────────────────────────

/// The terminal a take is shown in: where its input comes from and its output goes (your terminal's, or a test's pty).
pub(crate) struct Outer {
    pub(crate) input: OwnedFd,
    pub(crate) output: OwnedFd,
}

impl Outer {
    /// This process's own terminal: stdin and stdout, each a copy of its descriptor.
    fn stdio() -> Result<Outer> {
        let dup =
            |fd: std::os::fd::BorrowedFd| fd.try_clone_to_owned().map_err(|e| format!("the terminal: {e}"));
        Ok(Outer { input: dup(io::stdin().as_fd())?, output: dup(io::stdout().as_fd())? })
    }
}

/// What a take is: its size (columns, rows), where it's kept, the terminal it's shown in (None: headless), the key
/// script played into it, whether tarae is quit when the script ends (`:q!`, as a shot quits it — else the take runs
/// until the command ends), and the state home whose socket gives the landmarks (None: none watched).
pub(crate) struct Spec {
    pub(crate) size: (u16, u16),
    pub(crate) out: PathBuf,
    pub(crate) outer: Option<Outer>,
    pub(crate) keys: Vec<keys::Step>,
    pub(crate) quit_after_keys: bool,
    pub(crate) ui: Option<PathBuf>,
}

/// A take kept: how long (ms), how it ended, its bytes (screen, keys), the socket's lines and the landmarks drawn from
/// them, the waits its key script missed.
#[derive(Debug)]
pub(crate) struct Taken {
    pub(crate) ms: u64,
    pub(crate) exit: Option<i32>,
    pub(crate) screen_bytes: u64,
    pub(crate) key_bytes: u64,
    pub(crate) events: u64,
    pub(crate) landmarks: u64,
    pub(crate) missed: Vec<String>,
}

impl Taken {
    fn said(&self, out: &Path) -> String {
        let mut s = format!(
            "record: {} kept — {}, {} bytes of screen, {} of keys, {} landmarks ({} events), exit {}",
            out.display(),
            run_secs(self.ms),
            self.screen_bytes,
            self.key_bytes,
            self.landmarks,
            self.events,
            self.exit.map_or("by a signal".into(), |c| c.to_string()),
        );
        for m in &self.missed {
            s += &format!("\nrecord: missed — {m}");
        }
        s
    }
}

fn run_secs(ms: u64) -> String {
    format!("{:.1} s", ms as f64 / 1000.0)
}

/// What the pass-through hands the worker: a chunk of the screen or of input with when it came, a line of the socket's
/// (an event, or the part of a snapshot asked after one), the screen's end.
enum Log {
    Screen(Instant, Vec<u8>),
    Key(Instant, u8, Vec<u8>),
    Told(Instant, Value),
    State(Instant, Value),
    End,
}

/// The screen as the worker last drew it, for the key script's waits: its text, its frames so far, whether it ended.
#[derive(Default)]
pub(crate) struct Seen {
    now: Mutex<SeenNow>,
    changed: Condvar,
}

#[derive(Default)]
struct SeenNow {
    text: String,
    ended: bool,
}

impl Seen {
    /// Until `holds` holds of the screen's text, the screen ends, or `until` passes: whether it held.
    pub(crate) fn wait(&self, until: Instant, holds: impl Fn(&str) -> bool) -> bool {
        let mut now = self.now.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if holds(&now.text) {
                return true;
            }
            let Some(left) = until.checked_duration_since(Instant::now()).filter(|_| !now.ended) else {
                return false;
            };
            now = self.changed.wait_timeout(now, left).unwrap_or_else(|p| p.into_inner()).0;
        }
    }

    /// `d` passed, or the screen ended first: whether it's still there.
    pub(crate) fn pause(&self, d: Duration) -> bool {
        self.wait(Instant::now() + d, |_| false);
        !self.ended()
    }

    pub(crate) fn ended(&self) -> bool {
        self.now.lock().unwrap_or_else(|p| p.into_inner()).ended
    }

    fn set(&self, text: Option<String>, ended: bool) {
        let mut now = self.now.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(t) = text {
            now.text = t;
        }
        now.ended |= ended;
        self.changed.notify_all();
    }
}

/// A take of `cmd` as `spec` says (`record`'s top). Refused, nothing started, where `spec.out` already holds a take or
/// the terminal it's shown in is smaller than its size.
pub(crate) fn record(spec: Spec, mut cmd: Command) -> Result<Taken> {
    let (cols, rows) = spec.size;
    let out = &spec.out;
    if out.join("take.json").exists() || out.join("screen.bin").exists() {
        return Err(format!("{} already holds a take — name another --out", out.display()));
    }
    if let Some(o) = &spec.outer {
        let there = winsize(o.output.as_raw_fd()).map_err(|e| {
            format!("the terminal to show the take in: {e} — --headless to take it with none")
        })?;
        fits(there, spec.size)?;
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let file = |name: &str| {
        let p = out.join(name);
        File::create(&p).map(BufWriter::new).map_err(|e| format!("{}: {e}", p.display()))
    };
    let files = Files {
        screen: file("screen.bin")?,
        keys: file("keys.bin")?,
        events: file("events.jsonl")?,
        landmarks: file("landmarks.jsonl")?,
    };
    // (your terminal raw while the take runs — every key and answer goes in as it is; put back when it ends)
    let _raw = match &spec.outer {
        Some(o) => Raw::on(&o.input).map_err(|e| format!("the terminal: {e}"))?,
        None => None,
    };
    let (master, slave) = crate::pty::open(rows, cols).map_err(|e| format!("pty: {e}"))?;
    let master = File::from(master);
    let stdio = || slave.try_clone().map(Stdio::from).map_err(|e| format!("pty: {e}"));
    cmd.stdin(stdio()?).stdout(stdio()?).stderr(stdio()?);
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            // (`setsid` + the pty as its controlling terminal: /dev/tty works, SIGWINCH would arrive)
            shot::cvt(libc::setsid())?;
            shot::cvt(libc::ioctl(0, libc::TIOCSCTTY as _, 0))?;
            Ok(())
        });
    }
    let started = Instant::now();
    let wall =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", cmd.get_program().to_string_lossy()))?;
    drop(slave);
    let pid = child.id();

    let (tx, rx) = mpsc::channel::<Log>();
    let seen = Arc::new(Seen::default());
    let worker = {
        let seen = seen.clone();
        let size = spec.size;
        std::thread::Builder::new()
            .name("record-log".into())
            .spawn(move || worker(rx, files, started, size, &seen))
            .map_err(|e| format!("the log's worker: {e}"))?
    };
    // (the screen: each chunk to your terminal at once, then handed to the worker)
    let screen = {
        let mut from = master.try_clone().map_err(|e| format!("pty: {e}"))?;
        let mut to = match &spec.outer {
            Some(o) => Some(File::from(o.output.try_clone().map_err(|e| format!("the terminal: {e}"))?)),
            None => None,
        };
        let tx = tx.clone();
        std::thread::Builder::new()
            .name("record-screen".into())
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match from.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let at = Instant::now();
                            // (your terminal gone: the take goes on, kept all the same)
                            if let Some(t) = &mut to
                                && t.write_all(&buf[..n]).is_err()
                            {
                                to = None;
                            }
                            let _ = tx.send(Log::Screen(at, buf[..n].to_vec()));
                        }
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        // (macOS: EIO once the last of the child's side closed)
                        Err(_) => break,
                    }
                }
                let _ = tx.send(Log::End);
            })
            .map_err(|e| format!("the screen's reader: {e}"))?
    };
    // (the take's end, for the threads that would wait on input that never comes: a pipe they poll beside it)
    let (stop_r, stop_w) = pipe().map_err(|e| format!("pipe: {e}"))?;
    let input = match &spec.outer {
        Some(o) => {
            let from = o.input.try_clone().map_err(|e| format!("the terminal: {e}"))?;
            let to = master.try_clone().map_err(|e| format!("pty: {e}"))?;
            let stop = stop_r.try_clone().map_err(|e| format!("pipe: {e}"))?;
            let tx = tx.clone();
            Some(
                std::thread::Builder::new()
                    .name("record-input".into())
                    .spawn(move || pass_input(from, to, &stop, &tx))
                    .map_err(|e| format!("the input's reader: {e}"))?,
            )
        }
        None => None,
    };
    let script = match spec.keys.is_empty() {
        true => None,
        false => {
            let to = master.try_clone().map_err(|e| format!("pty: {e}"))?;
            let (tx, seen, steps, quit) = (tx.clone(), seen.clone(), spec.keys, spec.quit_after_keys);
            Some(
                std::thread::Builder::new()
                    .name("record-keys".into())
                    .spawn(move || {
                        let mut send = |bytes: &[u8]| {
                            let mut to = &to;
                            let at = Instant::now();
                            let _ = to.write_all(bytes);
                            let _ = tx.send(Log::Key(at, SCRIPT, bytes.to_vec()));
                        };
                        let missed = keys::play(&steps, &mut send, &seen);
                        if quit {
                            keys::quit(&mut send, &seen, pid);
                        }
                        missed
                    })
                    .map_err(|e| format!("the key script: {e}"))?,
            )
        }
    };
    let stopped = Arc::new(AtomicBool::new(false));
    let watcher = match &spec.ui {
        Some(state) => {
            let sock = state.join("tarae/ui").join(format!("{pid}.sock"));
            let (tx, stopped) = (tx.clone(), stopped.clone());
            Some(
                std::thread::Builder::new()
                    .name("record-landmarks".into())
                    .spawn(move || landmarks::watch(&sock, &tx, &stopped))
                    .map_err(|e| format!("the landmarks' watcher: {e}"))?,
            )
        }
        None => None,
    };
    drop(tx);

    let status = child.wait().map_err(|e| format!("the command: {e}"))?;
    let _ = screen.join();
    stopped.store(true, Ordering::Relaxed);
    let _ = (&File::from(stop_w)).write_all(b"x");
    if let Some(t) = input {
        let _ = t.join();
    }
    let missed = script.map(|t| t.join().unwrap_or_default()).unwrap_or_default();
    if let Some(t) = watcher {
        let _ = t.join();
    }
    drop(master);
    let counts = worker.join().map_err(|_| "the log's worker panicked".to_string())??;
    let ms = started.elapsed().as_millis() as u64;
    let taken = Taken {
        ms,
        exit: status.code(),
        screen_bytes: counts.0,
        key_bytes: counts.1,
        events: counts.2,
        landmarks: counts.3,
        missed,
    };
    let command: Vec<String> = std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|a| a.to_string_lossy().into())
        .collect();
    let about = json!({
        "v": 1, "size": [cols, rows], "command": command, "started_unix_ms": wall as u64, "ms": ms,
        "exit": taken.exit, "shown": spec.outer.is_some(), "screen_bytes": taken.screen_bytes,
        "key_bytes": taken.key_bytes, "events": taken.events, "landmarks": taken.landmarks, "missed": taken.missed,
    });
    let p = out.join("take.json");
    std::fs::write(&p, format!("{about:#}\n")).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(taken)
}

/// A take's logs, open for the worker.
struct Files {
    screen: BufWriter<File>,
    keys: BufWriter<File>,
    events: BufWriter<File>,
    landmarks: BufWriter<File>,
}

/// The worker: every record written as it comes (flushed whenever nothing more is waiting), the screen drawn for the
/// key script's waits, the landmarks drawn from the socket's lines. Its counts: screen bytes, key bytes, socket lines,
/// landmarks.
fn worker(
    rx: mpsc::Receiver<Log>,
    mut f: Files,
    started: Instant,
    size: (u16, u16),
    seen: &Seen,
) -> Result<(u64, u64, u64, u64)> {
    let us = |at: Instant| at.saturating_duration_since(started).as_micros() as u64;
    let mut parser = vt100::Parser::new(size.1, size.0, 0);
    let mut whole = Whole::default();
    let mut marks = landmarks::Landmarks::default();
    let (mut screen, mut keys, mut events, mut landmarks) = (0u64, 0u64, 0u64, 0u64);
    let said = |e: io::Error| format!("the take's log: {e}");
    let mut next = rx.recv().ok();
    while let Some(log) = next {
        match log {
            Log::Screen(at, bytes) => {
                put(&mut f.screen, us(at), SCREEN, &bytes).map_err(said)?;
                screen += bytes.len() as u64;
                let ready = whole.take(&bytes);
                if !ready.is_empty() {
                    feed(&mut parser, &ready);
                    seen.set(Some(parser.screen().contents()), false);
                }
            }
            Log::Key(at, whose, bytes) => {
                put(&mut f.keys, us(at), whose, &bytes).map_err(said)?;
                keys += bytes.len() as u64;
            }
            Log::Told(at, ev) => {
                let ms = us(at) / 1000;
                writeln!(f.events, "{}", json!({ "ms": ms, "event": ev })).map_err(said)?;
                events += 1;
                for mark in marks.told(ms, &ev) {
                    writeln!(f.landmarks, "{}", landmarks::line(mark)).map_err(said)?;
                    landmarks += 1;
                }
            }
            Log::State(at, state) => {
                let ms = us(at) / 1000;
                writeln!(f.events, "{}", json!({ "ms": ms, "state": state })).map_err(said)?;
                for mark in marks.state(ms, &state) {
                    writeln!(f.landmarks, "{}", landmarks::line(mark)).map_err(said)?;
                    landmarks += 1;
                }
            }
            Log::End => {
                // (what was held for a frame's end that never came: drawn as it is)
                let rest = std::mem::take(&mut whole.held);
                feed(&mut parser, &rest);
                seen.set(Some(parser.screen().contents()), true);
            }
        }
        next = match rx.try_recv() {
            Ok(log) => Some(log),
            Err(mpsc::TryRecvError::Empty) => {
                for w in [&mut f.screen, &mut f.keys, &mut f.events, &mut f.landmarks] {
                    w.flush().map_err(said)?;
                }
                rx.recv().ok()
            }
            Err(mpsc::TryRecvError::Disconnected) => None,
        };
    }
    seen.set(None, true);
    for w in [&mut f.screen, &mut f.keys, &mut f.events, &mut f.landmarks] {
        w.flush().map_err(said)?;
    }
    Ok((screen, keys, events, landmarks))
}

/// Your terminal's input into the take's terminal, each chunk handed to the worker — until the take ends (`stop`
/// readable) or your terminal goes.
fn pass_input(from: OwnedFd, to: File, stop: &OwnedFd, tx: &mpsc::Sender<Log>) {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let mut fds = [
            libc::pollfd { fd: from.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: stop.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        ];
        // SAFETY: two pollfds of descriptors this thread holds.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if n < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        if fds[1].revents != 0 {
            return;
        }
        if fds[0].revents == 0 {
            continue;
        }
        // SAFETY: a read into this buffer, its length.
        let got = unsafe { libc::read(from.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if got <= 0 {
            if got < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        let bytes = &buf[..got as usize];
        let at = Instant::now();
        if (&to).write_all(bytes).is_err() {
            return;
        }
        let _ = tx.send(Log::Key(at, TERMINAL, bytes.to_vec()));
    }
}

/// A pipe, both ends close-on-exec (the command never holds them).
fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    use std::os::fd::FromRawFd;
    let mut fds = [0; 2];
    // SAFETY: two ints for pipe to fill.
    shot::cvt(unsafe { libc::pipe(fds.as_mut_ptr()) })?;
    // SAFETY: the two descriptors pipe just made, owned from here.
    let (r, w) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    for fd in [&r, &w] {
        // SAFETY: a descriptor held here.
        shot::cvt(unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) })?;
    }
    Ok((r, w))
}

/// The size of the terminal at `fd` (columns, rows); an error where it's none.
fn winsize(fd: RawFd) -> io::Result<(u16, u16)> {
    // SAFETY: a winsize for the ioctl to fill.
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    // SAFETY: TIOCGWINSZ into that winsize.
    shot::cvt(unsafe { libc::ioctl(fd, libc::TIOCGWINSZ as _, &mut ws) })?;
    Ok((ws.ws_col, ws.ws_row))
}

/// Whether a terminal `there` big (columns, rows) can show a take of `size`; refused saying the size it needs.
pub(crate) fn fits(there: (u16, u16), size: (u16, u16)) -> Result<()> {
    match there.0 >= size.0 && there.1 >= size.1 {
        true => Ok(()),
        false => Err(format!(
            "this terminal is {}×{}; the take needs {}×{} — make it at least that big, or take a smaller --size",
            there.0, there.1, size.0, size.1
        )),
    }
}

/// A terminal held raw while the take runs (every byte through as it is: no echo, no line editing, no signals from
/// keys — tarae's own), put back as it was on drop. None where the descriptor is no terminal.
struct Raw {
    fd: OwnedFd,
    was: libc::termios,
}

impl Raw {
    fn on(fd: &OwnedFd) -> io::Result<Option<Raw>> {
        // SAFETY: a descriptor held here.
        if unsafe { libc::isatty(fd.as_raw_fd()) } != 1 {
            return Ok(None);
        }
        let fd = fd.try_clone()?;
        // SAFETY: a termios for tcgetattr to fill.
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: the terminal's attributes into `t`.
        shot::cvt(unsafe { libc::tcgetattr(fd.as_raw_fd(), &mut t) })?;
        let was = t;
        // SAFETY: `t` made raw, then set.
        unsafe { libc::cfmakeraw(&mut t) };
        // SAFETY: as above.
        shot::cvt(unsafe { libc::tcsetattr(fd.as_raw_fd(), libc::TCSANOW, &t) })?;
        Ok(Some(Raw { fd, was }))
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        // SAFETY: the attributes it had, put back.
        unsafe { libc::tcsetattr(self.fd.as_raw_fd(), libc::TCSANOW, &self.was) };
    }
}

// ── The log ─────────────────────────────────────────────────────────────────────────────────────────────────────────

/// One record of a take's log: when (µs since the start), whose (`SCREEN`, `TERMINAL`, `SCRIPT`), its bytes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Rec {
    pub(crate) us: u64,
    pub(crate) whose: u8,
    pub(crate) bytes: Vec<u8>,
}

/// A record onto a log.
pub(crate) fn put(w: &mut impl Write, us: u64, whose: u8, bytes: &[u8]) -> io::Result<()> {
    let len = u32::try_from(bytes.len()).map_err(|_| io::Error::other("a chunk past 4 GiB"))?;
    w.write_all(&us.to_le_bytes())?;
    w.write_all(&[whose])?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(bytes)
}

/// A log's records, in order; a record cut short at its end (a take killed mid-write) left out.
pub(crate) fn read_log(path: &Path) -> Result<Vec<Rec>> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    let mut at = 0;
    while data.len() >= at + 13 {
        let us = u64::from_le_bytes(data[at..at + 8].try_into().expect("8 bytes"));
        let whose = data[at + 8];
        let len = u32::from_le_bytes(data[at + 9..at + 13].try_into().expect("4 bytes")) as usize;
        let Some(bytes) = data.get(at + 13..at + 13 + len) else { break };
        out.push(Rec { us, whose, bytes: bytes.to_vec() });
        at += 13 + len;
    }
    Ok(out)
}

/// A synchronized update's start (`shot::FRAME_END` its end).
const FRAME_START: &[u8] = b"\x1b[?2026h";

/// tarae's bytes cut at whole frames, as a shot feeds them (`shot::Vt::feed`): of what came so far, what's ready to be
/// drawn — everything but a synchronized update begun and not yet ended (or the start of its opening, cut between
/// chunks), held for what comes next. A command that draws no synchronized updates is drawn as it comes.
#[derive(Default)]
pub(crate) struct Whole {
    held: Vec<u8>,
}

impl Whole {
    pub(crate) fn take(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.held.extend_from_slice(chunk);
        let h = &self.held;
        let ended =
            h.windows(FRAME_END.len()).rposition(|w| w == FRAME_END).map_or(0, |i| i + FRAME_END.len());
        let open = h[ended..].windows(FRAME_START.len()).position(|w| w == FRAME_START).map(|i| ended + i);
        // (an opening cut at the chunk's end: `ESC [ ? 2 0` waiting for `2 6 h`)
        let cut =
            (1..FRAME_START.len()).rev().find(|&k| h.len() >= ended + k && h.ends_with(&FRAME_START[..k]));
        let at = open.or(cut.map(|k| h.len() - k)).unwrap_or(h.len());
        self.held.drain(..at).collect()
    }
}

/// `bytes` into `parser` — vt100 can panic on odd input: the screen so far kept rather than the take lost.
pub(crate) fn feed(parser: &mut vt100::Parser, bytes: &[u8]) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parser.process(bytes)));
}

/// A take's screens at their times (ms since its start): its screen log replayed at its `size`, a screen after each
/// chunk that drew a whole frame — one that looks like the one before it (a resting redraw) left out.
pub(crate) fn screens(recs: &[Rec], size: (u16, u16)) -> Vec<(u64, vt100::Screen)> {
    let mut replay = Replay::new(recs, size);
    let mut out = Vec::new();
    while let Some(ms) = replay.next() {
        out.push((ms, replay.screen().clone()));
    }
    out
}

/// A take's screens one at a time, the log replayed as it's read — `screens`' screens, never more than one held: what
/// an hour and a half's take is cut by (`cut.rs`), where every screen kept at once would be gigabytes.
pub(crate) struct Replay<'a> {
    recs: &'a [Rec],
    at: usize,
    parser: vt100::Parser,
    whole: Whole,
    last: Option<(Vec<u8>, (u16, u16), bool)>,
    /// How many screens it has given (each looking unlike the one before).
    pub(crate) shown: u64,
}

impl<'a> Replay<'a> {
    pub(crate) fn new(recs: &'a [Rec], size: (u16, u16)) -> Replay<'a> {
        Replay {
            recs,
            at: 0,
            parser: vt100::Parser::new(size.1, size.0, 0),
            whole: Whole::default(),
            last: None,
            shown: 0,
        }
    }

    /// The next screen's time (ms), the screen then `screen()`; None past the log's end.
    #[allow(clippy::should_implement_trait)]
    pub(crate) fn next(&mut self) -> Option<u64> {
        while let Some(r) = self.recs.get(self.at) {
            self.at += 1;
            if self.fed(r) {
                return Some(r.us / 1000);
            }
        }
        None
    }

    /// The screen as it stands `ms` into the take: every record by then fed. Whether a new screen came on the way.
    pub(crate) fn to(&mut self, ms: u64) -> bool {
        let mut came = false;
        while let Some(r) = self.recs.get(self.at).filter(|r| r.us / 1000 <= ms) {
            self.at += 1;
            came |= self.fed(r);
        }
        came
    }

    pub(crate) fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// `r` fed where it's the screen's and ends a whole frame: whether the screen now looks unlike the one before.
    fn fed(&mut self, r: &Rec) -> bool {
        if r.whose != SCREEN {
            return false;
        }
        let ready = self.whole.take(&r.bytes);
        if ready.is_empty() {
            return false;
        }
        feed(&mut self.parser, &ready);
        let s = self.parser.screen();
        let looks = (s.contents_formatted(), s.cursor_position(), s.hide_cursor());
        if self.last.as_ref() == Some(&looks) {
            return false;
        }
        self.last = Some(looks);
        self.shown += 1;
        true
    }
}

/// A take read back: its `take.json`, size and screen log.
pub(crate) struct Take {
    pub(crate) about: Value,
    pub(crate) size: (u16, u16),
    pub(crate) screen: Vec<Rec>,
}

impl Take {
    pub(crate) fn read(dir: &Path) -> Result<Take> {
        let p = dir.join("take.json");
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e} — not a take", p.display()))?;
        let about: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
        let n = |i: usize| about["size"][i].as_u64().and_then(|v| u16::try_from(v).ok());
        let size = n(0).zip(n(1)).ok_or_else(|| format!("{}: no size", p.display()))?;
        Ok(Take { size, screen: read_log(&dir.join("screen.bin"))?, about })
    }

    /// How long the take ran (ms): its `take.json`'s, else its last chunk's time.
    pub(crate) fn ms(&self) -> u64 {
        self.about["ms"].as_u64().unwrap_or_else(|| self.screen.last().map_or(0, |r| r.us / 1000))
    }
}

// ── The take drawn ────────────────────────────────────────────────────────────────────────────────────────────────────

fn bg_of(word: &str) -> Result<Bg> {
    match word {
        "dark" => Ok(Bg::Dark),
        "light" => Ok(Bg::Light),
        w => Err(format!("--bg {w}: dark or light")),
    }
}

fn draw_cmd(args: &[String]) -> Result<()> {
    let dir = PathBuf::from(args.first().ok_or(USAGE)?);
    let (mut bg, mut title) = (Bg::Dark, "A live take".to_string());
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--bg", Some(v)) => bg = bg_of(v)?,
            ("--title", Some(v)) => title = v.clone(),
            _ => return Err(format!("{a}: {USAGE}")),
        }
    }
    let made = draw(&dir, bg, &title)?;
    println!("{}", made);
    Ok(())
}

/// The take in `dir` as the wide video, unedited: its screens at their times (`screens`), between the title card and
/// the end card, silent — `dir/take.mp4` and its sheet. What it made, said.
pub(crate) fn draw(dir: &Path, bg: Bg, title: &str) -> Result<String> {
    let take = Take::read(dir)?;
    let frames = screens(&take.screen, take.size);
    if frames.is_empty() {
        return Err(format!("{}: the take drew no screen", dir.display()));
    }
    let end = take.ms().max(frames.last().map_or(0, |f| f.0));
    let rec = Recorded {
        pointers: vec![None; frames.len()],
        frames,
        beats: Vec::new(),
        waits: Vec::new(),
        zooms: Vec::new(),
        notes: Vec::new(),
        keys: Vec::new(),
        sounds: Vec::new(),
        hook: None,
        missed: Vec::new(),
        wire: Vec::new(),
        end,
        joins: Vec::new(),
    };
    let mp4 = dir.join("take.mp4");
    let yours = std::env::var_os("HOME");
    let made = video::make(&rec, bg, title, &mp4, yours.as_deref(), video::Form::Wide, None)?;
    let mut said = format!(
        "{}: {:.1} s, {} screens of the take, glyphs {} px, {} frames ({} differ), the sheet {} pictures",
        mp4.display(),
        made.seconds,
        rec.frames.len(),
        made.px,
        made.frames,
        made.differ,
        made.sheet
    );
    for m in made.missed {
        said += &format!("\n  {m}");
    }
    Ok(said)
}

fn still_cmd(args: &[String]) -> Result<()> {
    let (dir, out) = match args {
        [d, o, ..] => (PathBuf::from(d), PathBuf::from(o)),
        _ => return Err(USAGE.into()),
    };
    let at = match &args[2..] {
        [] => None,
        [flag, s] if flag == "--at" => {
            Some((s.parse::<f64>().map_err(|_| format!("--at {s}: seconds"))? * 1000.0) as u64)
        }
        _ => return Err(USAGE.into()),
    };
    println!("{}", still(&dir, &out, at, Bg::Dark)?);
    Ok(())
}

/// The screen of the take in `dir` at `at` ms (None: its start once it stood still a second — `settled`) as the
/// wide video draws it, a PNG at `out`. Said: the size, the glyph's px, the screen's picture and how much of the room
/// (`video::WIDE_ROOM`) it fills, and the cells that room holds at that glyph.
pub(crate) fn still(dir: &Path, out: &Path, at: Option<u64>, bg: Bg) -> Result<String> {
    let take = Take::read(dir)?;
    let frames = screens(&take.screen, take.size);
    let i = match at {
        Some(ms) => frames.partition_point(|f| f.0 <= ms).saturating_sub(1),
        None => settled(&frames, 1000),
    };
    let (ms, screen) = frames.get(i).ok_or_else(|| format!("{}: the take drew no screen", dir.display()))?;
    let (px, pic, rgb) = video::wide_still(screen, bg)?;
    shot::save_png(out, video::W, video::H, &rgb)?;
    let fonts = shot::Fonts::load(1)?;
    let (cw, ch) = shot::cell(&fonts, px);
    let (rw, rh) = video::WIDE_ROOM;
    let holds = ((rw - 2 * shot::PAD) / cw, (rh - 2 * shot::PAD) / ch);
    Ok(format!(
        "{}: {}×{} at {:.1} s — glyphs {px} px (cells {cw}×{ch}), the screen {}×{} px: {:.0} % of the room's width, \
         {:.0} % of its height; at {px} px the room holds {}×{}",
        out.display(),
        take.size.0,
        take.size.1,
        *ms as f64 / 1000.0,
        pic.0,
        pic.1,
        100.0 * pic.0 as f64 / rw as f64,
        100.0 * pic.1 as f64 / rh as f64,
        holds.0,
        holds.1
    ))
}

/// The first of `frames` the next one comes `still_ms` or more after (the screen stood still that long): a start's
/// whole screen, past its first half-drawn frames. The last where none does.
pub(crate) fn settled(frames: &[(u64, vt100::Screen)], still_ms: u64) -> usize {
    (0..frames.len()).find(|&i| frames.get(i + 1).is_none_or(|n| n.0 >= frames[i].0 + still_ms)).unwrap_or(0)
}

/// The repository's place for a take nobody named: `target/rec/<name>`.
pub(crate) fn rec_dir(name: &str) -> PathBuf {
    root().join("target/rec").join(name)
}
