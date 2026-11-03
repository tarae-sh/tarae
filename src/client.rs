//! `tarae attach` (the remote plan's §8b, step 3): this terminal shows a host (`tarae --host`, `attach.rs`) and its
//! keys are the host's. It says what it is (`Hello`: its size, ground, undercurl, inside zellij), shows each frame as it
//! comes and acks it once it's written and flushed (presented: what the host's "only once drawn" rules count), sends
//! each input with the last frame it presented, and redraws the whole screen from the frames it holds once they rest
//! (`screen::Held` — the wire never carries that). The host going is said: `:detach` and a take-over restore the
//! terminal and say how to come back; a quit prints what it didn't write; a host that ended unsaid (a crash) leaves its
//! last frame up with a line of the client's own — `r` attaches again, `q` quits. `:restart` execs this client into
//! the host's new binary, the terminal left as it is.

use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use tarae_wire::{self as wire, Decoder, Gone, Hello, ToHost, ToInterface};

use crate::event::Event;

const USAGE: &str = "usage: tarae attach [--pid PID]   show a host (tarae --host) in this terminal";
/// The host's `Welcome` within this, or it's said it didn't answer.
const WELCOME_WITHIN: Duration = Duration::from_secs(10);
/// How long a client a restart exec'd waits for the new host to bind its socket.
const RESTART_WAIT: Duration = Duration::from_secs(60);

/// What the client's loop hears.
enum Heard {
    Host(ToInterface),
    /// The host's socket ended without a `Gone` (a crash: release aborts), and how.
    Ended(String),
    Input(Event),
    /// SIGTERM or SIGHUP: the client goes, its terminal restored (the host detaches).
    Signal,
    /// What the host asked came to (`ask`, on a thread of its own): its answer, and an OSC 52 for this terminal.
    Done {
        id: u32,
        answer: wire::Answer,
        osc: Vec<u8>,
    },
}

/// `tarae attach [--pid PID]`.
pub fn main(args: &[String]) -> anyhow::Result<()> {
    let (mut pid, mut restarted, mut ground) = (None, false, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--pid" => {
                pid = Some(
                    it.next()
                        .and_then(|p| p.parse::<u32>().ok())
                        .ok_or_else(|| anyhow::anyhow!("{USAGE}"))?,
                )
            }
            // (`:restart`'s exec: the terminal is as the last client left it, its ground known)
            "--restarted" => restarted = true,
            "--ground" => ground = it.next().and_then(|g| g.parse::<u8>().ok()),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => anyhow::bail!("unknown: {other}\n{USAGE}"),
        }
    }
    // (before any thread: SIGTERM and SIGHUP said to the loop — the client goes, its terminal restored)
    crate::term::catch_signals(false);
    let signals = crate::term::signal_reader();
    let ground = match ground {
        Some(1) => wire::Ground::Dark,
        Some(2) => wire::Ground::Light,
        Some(_) => wire::Ground::Unsaid,
        // (asked of the terminal before it's raw — as tarae asks it at its start)
        None => match crate::term::terminal(None).facts.light {
            None => wire::Ground::Unsaid,
            Some(false) => wire::Ground::Dark,
            Some(true) => wire::Ground::Light,
        },
    };
    let (pid, path) = crate::attach::find(pid)?;
    let wait = restarted.then_some(RESTART_WAIT);
    let (stream, dec) = attach_to(&path, ground, wait).map_err(|e| anyhow::anyhow!("{e}"))?;
    crate::term::terminal_begin()?;
    let (tx, rx) = mpsc::channel();
    {
        let tx = tx.clone();
        crate::event::thread("tarae-wire-keys", move || {
            crate::term::read_input(|ev| tx.send(Heard::Input(ev)).is_ok())
        });
    }
    if let Some(mut r) = signals {
        let tx = tx.clone();
        crate::event::thread("tarae-signals", move || {
            let mut b = [0u8; 1];
            loop {
                match io::Read::read(&mut r, &mut b) {
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Ok(1) => break,
                    _ => return,
                }
            }
            let _ = tx.send(Heard::Signal);
        });
    }
    let mut c = Client {
        pid,
        path,
        ground,
        stream,
        held: crate::screen::Held::new(),
        seq: 0,
        seen: 0,
        due: None,
        tx,
        speaker: crate::sound::Played::default(),
        offset: offset_now(),
        zone_at: crate::clock::now() + ZONE_EVERY,
    };
    c.listen(dec);
    let end = c.run(&rx);
    // (raw mode and the alternate screen survive an exec: the restarted client takes the terminal as it is)
    if !matches!(end, End::Exec(_)) {
        let _ = crate::term::terminal_end();
    }
    match end {
        End::Said(text) => {
            if !text.is_empty() {
                println!("{text}");
            }
            Ok(())
        }
        End::Failed(text) => {
            eprintln!("tarae attach: {text}");
            std::process::exit(1)
        }
        End::Exec(bin) => {
            use std::os::unix::process::CommandExt;
            let g = match c.ground {
                wire::Ground::Unsaid => "0",
                wire::Ground::Dark => "1",
                wire::Ground::Light => "2",
            };
            let err = std::process::Command::new(&bin)
                .args(["attach", "--pid", &c.pid.to_string(), "--restarted", "--ground", g])
                .exec();
            let _ = crate::term::terminal_end();
            eprintln!("tarae attach: the host restarted as {}, but it can't run: {err}", bin.display());
            std::process::exit(1)
        }
    }
}

/// How the client ends.
enum End {
    /// Its terminal restored, this said (may be empty).
    Said(String),
    Failed(String),
    /// Into the restarted host's binary.
    Exec(PathBuf),
}

/// The host's socket at `path`, its `Hello` said and its `Welcome` in (`wait`: retried that long while it isn't
/// there yet — a restarted host binding it). What came after the `Welcome` stays in the decoder.
fn attach_to(
    path: &Path,
    ground: wire::Ground,
    wait: Option<Duration>,
) -> Result<(UnixStream, Decoder<ToInterface>), String> {
    let until = crate::clock::now() + wait.unwrap_or_default();
    let stream = loop {
        match crate::attach::connect(path) {
            Ok(s) => break s,
            Err(e)
                if crate::clock::now() < until
                    && matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) =>
            {
                crate::event::sleep(Duration::from_millis(50));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(format!("no host there ({}) — start one with tarae --host", path.display()));
            }
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    };
    let (w, h) = crate::term::size().unwrap_or((80, 24));
    let hello = Hello {
        v: wire::V,
        build: env!("CARGO_PKG_VERSION").into(),
        w,
        h,
        ground,
        undercurl: crate::term::undercurl_here(),
        zellij: crate::zellij::inside(),
        // (this machine's clipboard, speaker and browser, the host's to ask: a clipboard with no program here is this
        // terminal's — a copy by OSC 52, a paste said)
        clipboard: wire::Clip::CopyAndPaste,
        // (only macOS plays tarae's sound yet)
        sound: cfg!(target_os = "macos"),
        open: true,
        zone: std::env::var("TZ").unwrap_or_default(),
        offset: offset_now(),
    };
    send(&stream, &ToHost::Hello(hello)).map_err(|e| format!("saying hello: {e}"))?;
    let _ = stream.set_read_timeout(Some(WELCOME_WITHIN));
    let mut dec = Decoder::<ToInterface>::new();
    loop {
        match dec.message() {
            Ok(Some(ToInterface::Welcome { .. })) => break,
            Ok(Some(ToInterface::Gone { why, .. })) => {
                return Err(match why {
                    Gone::Refused { said } => format!("refused — {said}"),
                    Gone::Protocol { said } => format!("the host didn't read it: {said}"),
                    other => format!("the host let go at once: {other:?}"),
                });
            }
            Ok(Some(other)) => return Err(format!("the host's first word wasn't Welcome: {other:?}")),
            Err(e) => return Err(format!("the host's bytes were wrong: {e}")),
            Ok(None) => match dec.read_from(&mut &stream) {
                Ok(0) => return Err("the host closed the socket before its Welcome".into()),
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    return Err(format!("no Welcome within {} s: {e}", WELCOME_WITHIN.as_secs()));
                }
            },
        }
    }
    let _ = stream.set_read_timeout(None);
    Ok((stream, dec))
}

fn send(mut s: &UnixStream, m: &ToHost) -> io::Result<()> {
    let mut out = Vec::new();
    wire::encode(m, &mut out).map_err(|e| io::Error::other(e.said))?;
    s.write_all(&out)
}

/// Seconds east of UTC now, as this machine's zone has it.
fn offset_now() -> i32 {
    // SAFETY: `time` and `localtime_r` only fill in the locals they're given.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() { 0 } else { tm.tm_gmtoff as i32 }
    }
}

struct Client {
    pid: u32,
    path: PathBuf,
    ground: wire::Ground,
    stream: UnixStream,
    /// What the terminal was sent of the host's frames (the resting redraw's).
    held: crate::screen::Held,
    /// Inputs sent.
    seq: u64,
    /// The last frame presented: written and flushed.
    seen: u64,
    /// When the whole screen goes again (`RESTING_REDRAW` after the last frame).
    due: Option<std::time::Instant>,
    tx: mpsc::Sender<Heard>,
    /// The host's sound, played here.
    speaker: crate::sound::Played,
    /// The offset from UTC the host was last told, and when it's looked at again (`ZONE_EVERY`; focus back too).
    offset: i32,
    zone_at: std::time::Instant,
}

/// How often the client looks whether its zone's offset moved (summer time, the laptop moved) — and as focus comes
/// back.
const ZONE_EVERY: Duration = Duration::from_secs(3600);

impl Client {
    /// The host's messages read on a thread of their own into the loop.
    fn listen(&self, mut dec: Decoder<ToInterface>) {
        let Ok(mut s) = self.stream.try_clone() else { return };
        let tx = self.tx.clone();
        crate::event::thread("tarae-wire-host", move || {
            let why = loop {
                match dec.message() {
                    Ok(Some(m)) => {
                        if tx.send(Heard::Host(m)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => match dec.read_from(&mut s) {
                        Ok(0) => break "its socket closed".to_string(),
                        Ok(_) => {}
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        Err(e) => break e.to_string(),
                    },
                    Err(e) => break format!("its bytes were wrong: {e}"),
                }
            };
            let _ = tx.send(Heard::Ended(why));
        });
    }

    fn run(&mut self, rx: &mpsc::Receiver<Heard>) -> End {
        let mut ended = false;
        loop {
            // (the resting redraw, else the zone's hourly look — idle, the next wake is an hour off)
            let at = self.due.map_or(self.zone_at, |d| d.min(self.zone_at));
            let heard = match rx.recv_timeout(at.saturating_duration_since(crate::clock::now())) {
                Ok(h) => h,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let now = crate::clock::now();
                    if self.due.is_some_and(|d| d <= now) {
                        self.due = None;
                        let mut out = Vec::new();
                        self.held.whole(&mut out);
                        let _ = write_out(&out);
                    }
                    if self.zone_at <= now {
                        self.zone_again(ended);
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return End::Said(String::new()),
            };
            match heard {
                Heard::Host(m) if !ended => {
                    if let Some(end) = self.heard(m) {
                        return end;
                    }
                }
                Heard::Host(_) => {}
                Heard::Ended(why) if !ended => {
                    ended = true;
                    self.due = None;
                    let _ =
                        crate::term::last_line(&format!(" the host ended ({why}) — r attach again · q quit"));
                }
                Heard::Ended(_) => {}
                Heard::Signal => {
                    let _ = send(&self.stream, &ToHost::Bye);
                    return End::Said(String::new());
                }
                Heard::Done { id, answer, osc } => {
                    // (its OSC 52 after the frame on screen, as tarae's own goes after its frame)
                    if !osc.is_empty() {
                        let _ = write_out(&osc);
                    }
                    if !ended {
                        let _ = send(&self.stream, &ToHost::Answer { id, answer });
                    }
                }
                Heard::Input(ev) if ended => match ev {
                    Event::Key(k) if k.code == crate::key::Code::Char('r') && !k.ctrl && !k.alt => {
                        match self.again() {
                            Ok(()) => ended = false,
                            Err(e) => {
                                let _ = crate::term::last_line(&format!(" no host: {e} — r again · q quit"));
                            }
                        }
                    }
                    Event::Key(k)
                        if matches!(k.code, crate::key::Code::Char('q') | crate::key::Code::Esc)
                            || (k.ctrl && k.code == crate::key::Code::Char('c')) =>
                    {
                        return End::Said(String::new());
                    }
                    _ => {}
                },
                Heard::Input(Event::Resize) => {
                    if let Ok((w, h)) = crate::term::size() {
                        let _ = send(&self.stream, &ToHost::Resize { w, h });
                    }
                }
                Heard::Input(ev) => {
                    if let Event::Focus(true) = ev {
                        self.zone_again(ended);
                    }
                    if let Some(event) = crate::attach::input_of(ev) {
                        self.seq += 1;
                        let _ = send(&self.stream, &ToHost::Input { seq: self.seq, seen: self.seen, event });
                    }
                }
            }
        }
    }

    /// One message of the host's — Some: the client's end.
    fn heard(&mut self, m: ToInterface) -> Option<End> {
        match m {
            ToInterface::Frame { n, w, h, bytes, .. } => {
                if !bytes.is_empty() && write_out(&bytes).is_err() {
                    let _ = send(&self.stream, &ToHost::Bye);
                    return Some(End::Failed("this terminal can't be written to".into()));
                }
                self.held.apply(&bytes, w, h);
                self.seen = n;
                let _ = send(&self.stream, &ToHost::Ack { frame: n });
                if !bytes.is_empty() {
                    self.due = Some(crate::clock::now() + crate::interface::RESTING_REDRAW);
                }
                None
            }
            ToInterface::Gone { why, said } => Some(match why {
                Gone::Detached => End::Said("detached — tarae attach comes back".into()),
                Gone::TakenOver => End::Said("taken over by another tarae attach".into()),
                Gone::Quit => End::Said(said),
                Gone::Restarting { bin } => End::Exec(PathBuf::from(bin)),
                Gone::Refused { said } => End::Failed(format!("refused — {said}")),
                Gone::Protocol { said } => {
                    End::Failed(format!("the host didn't read what this sent: {said}"))
                }
            }),
            ToInterface::Ask { id, ask } => {
                let tx = self.tx.clone();
                crate::event::thread("tarae-wire-ask", move || {
                    let (answer, osc) = asked(ask);
                    let _ = tx.send(Heard::Done { id, answer, osc });
                });
                None
            }
            ToInterface::Sound(s) => {
                self.speaker.heard(s);
                None
            }
            ToInterface::Welcome { .. } => None,
        }
    }

    /// The zone's offset looked at again (hourly, focus back): the host told where it moved.
    fn zone_again(&mut self, ended: bool) {
        self.zone_at = crate::clock::now() + ZONE_EVERY;
        let offset = offset_now();
        if offset == self.offset || ended {
            return;
        }
        self.offset = offset;
        let zone = std::env::var("TZ").unwrap_or_default();
        let _ = send(&self.stream, &ToHost::Zone { zone, offset });
    }

    /// `r` on the host-ended line: the host looked for again (the one asked for, else the one `tarae state` picks).
    fn again(&mut self) -> Result<(), String> {
        let (pid, path) = match crate::attach::find(None) {
            Ok(found) => found,
            Err(_) => (self.pid, self.path.clone()),
        };
        let (stream, dec) = attach_to(&path, self.ground, None)?;
        (self.pid, self.path, self.stream) = (pid, path, stream);
        self.held = crate::screen::Held::new();
        self.listen(dec);
        Ok(())
    }
}

/// What the host asked of this machine done here, on a thread of its own (a clipboard program, a browser waited for):
/// its answer, and an OSC 52 for this terminal where the copy goes through it.
fn asked(ask: wire::Ask) -> (wire::Answer, Vec<u8>) {
    use crate::clipboard::{Read, Why, Wrote};
    let why = |w| match w {
        Why::Ssh => wire::Why::Ssh,
        Why::NoProgram => wire::Why::NoProgram,
    };
    match ask {
        wire::Ask::Copy(text) => match crate::clipboard::write_here(&text) {
            Ok(Wrote::Program) => (wire::Answer::Copied(None), Vec::new()),
            Ok(Wrote::Terminal(osc, w)) => (wire::Answer::Copied(Some(why(w))), osc),
            Err(e) => (wire::Answer::Failed(e), Vec::new()),
        },
        wire::Ask::Paste => match crate::clipboard::read_here() {
            Ok(Read::Text(t)) => (wire::Answer::Pasted(t), Vec::new()),
            Ok(Read::Terminal(w)) => (wire::Answer::PasteUnreadable(why(w)), Vec::new()),
            Err(e) => (wire::Answer::Failed(e), Vec::new()),
        },
        // (an address only, http(s) only, as `:web-open` hands one — checked here again: this machine runs the opener)
        wire::Ask::Open(url) if !crate::web::facts::followed(&url) => {
            (wire::Answer::Failed("only http(s) addresses are opened".into()), Vec::new())
        }
        wire::Ask::Open(url) => {
            let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
            match open(program, &url) {
                Ok(()) => (wire::Answer::Opened, Vec::new()),
                Err(e) => (wire::Answer::Failed(format!("{program}: {e}")), Vec::new()),
            }
        }
    }
}

#[cfg(not(test))]
fn open(program: &str, url: &str) -> Result<(), String> {
    crate::web::page::opened_by(program, url)
}

#[cfg(test)]
fn open(_: &str, _: &str) -> Result<(), String> {
    Err("not in tests".into())
}

/// A frame's bytes on the terminal, flushed: presented.
fn write_out(bytes: &[u8]) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(bytes)?;
    out.flush()
}
