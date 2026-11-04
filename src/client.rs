//! `tarae attach` (the remote plan's §8b, step 3): this terminal shows a host (`tarae --host`, `attach.rs`) and its
//! keys are the host's. It says what it is (`Hello`: its size, ground, undercurl, inside zellij), shows each frame as it
//! comes and acks it once it's written and flushed (presented: what the host's "only once drawn" rules count), sends
//! each input with the last frame it presented, and redraws the whole screen from the frames it holds once they rest
//! (`screen::Held` — the wire never carries that). The host going is said: `:detach` and a take-over restore the
//! terminal and say how to come back; a quit prints what it didn't write; a host that ended unsaid (a crash) leaves its
//! last frame up with a line of the client's own — `r` attaches again, `q` quits. `:restart` execs this client into
//! the host's new binary, the terminal left as it is.
//!
//! `tarae attach ssh://…` (the remote plan's §8d, step 5): the host on another machine, through `ssh` to a relay there
//! (`ssh.rs`, `relay.rs`) — the same client over ssh's two pipes. This terminal stays cooked until the relay's `ok` and
//! the host's `Welcome` are in (ssh's prompts and warnings are on it as in any ssh); ssh's stderr is kept after that.
//! A host restarted there is attached again by ssh with the relay waiting for it (`--wait`); `r` runs ssh again with no
//! prompt (`BatchMode`: the terminal is raw).

use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use tarae_wire::{self as wire, Decoder, Gone, Hello, ToHost, ToInterface};

use crate::event::Event;

const USAGE: &str = "usage: tarae attach [--pid PID | ssh://[USER@]HOST[:PORT][/PATH][?pid=N] [--remote-tarae PATH]]   \
                     show a host (tarae --host) in this terminal";
/// The host's `Welcome` within this, or it's said it didn't answer.
const WELCOME_WITHIN: Duration = Duration::from_secs(10);
/// How long a client a restart exec'd waits for the new host to bind its socket.
const RESTART_WAIT: Duration = Duration::from_secs(60);

/// What the client's loop hears.
enum Heard {
    /// A message of the host's, on the link of that generation (`Client.link_n`: one before an `again` is passed over).
    Host(u32, ToInterface),
    /// The host's link ended without a `Gone` (a crash: release aborts; ssh's link cut), and how.
    Ended(u32, String),
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

/// `tarae attach [--pid PID | ssh://… [--remote-tarae PATH]]`; `--stdio …` the relay (`relay.rs`).
pub fn main(args: &[String]) -> anyhow::Result<()> {
    if args.iter().any(|a| a == "--stdio") {
        crate::relay::main(args);
    }
    let (mut pid, mut restarted, mut ground) = (None, false, None);
    let (mut url, mut tarae) = (None, None);
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
            "--remote-tarae" => tarae = Some(it.next().cloned().ok_or_else(|| anyhow::anyhow!("{USAGE}"))?),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            a if a.starts_with("ssh://") && url.is_none() => {
                url = Some(crate::ssh::parse(a).map_err(anyhow::Error::msg)?)
            }
            other => anyhow::bail!("unknown: {other}\n{USAGE}"),
        }
    }
    if url.is_some() && pid.is_some() {
        anyhow::bail!("over ssh the pid is the address's: ssh://HOST?pid=N");
    }
    if url.is_none() && tarae.is_some() {
        anyhow::bail!("--remote-tarae names the tarae an ssh:// address runs");
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
    let mut way = match url {
        Some(url) => Way::Ssh { url, tarae: tarae.unwrap_or_else(|| "tarae".into()), pid: None },
        None => {
            let (pid, path) = crate::attach::find(pid)?;
            Way::Here { pid, path }
        }
    };
    let wait = restarted.then_some(RESTART_WAIT);
    let (link, dec) = match attach_to(&mut way, ground, wait, false) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("tarae attach: {e}");
            std::process::exit(1)
        }
    };
    crate::term::terminal_begin()?;
    if let Some(ssh) = &link.ssh {
        ssh.quiet();
    }
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
    let Link { to, from, ssh, .. } = link;
    let mut c = Client {
        way,
        ground,
        to,
        ssh,
        link_n: 0,
        held: crate::screen::Held::new(),
        seq: 0,
        seen: 0,
        due: None,
        tx,
        speaker: crate::sound::Played::default(),
        offset: offset_now(),
        zone_at: crate::clock::now() + ZONE_EVERY,
    };
    c.listen(from, dec);
    let end = c.run(&rx);
    c.let_go();
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
                .args(["attach", "--pid", &c.way.pid().to_string(), "--restarted", "--ground", g])
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

/// Where the host is.
enum Way {
    /// Host `pid`'s socket on this machine.
    Here { pid: u32, path: PathBuf },
    /// Through ssh to `url`'s relay (`tarae`: the tarae it runs there); `pid` the host the relay said, once it has.
    Ssh { url: crate::ssh::Url, tarae: String, pid: Option<u32> },
}

impl Way {
    fn pid(&self) -> u32 {
        match self {
            Way::Here { pid, .. } => *pid,
            Way::Ssh { pid, url, .. } => pid.or(url.pid).unwrap_or_default(),
        }
    }

    /// How to come back, said as the client goes.
    fn again_said(&self) -> String {
        match self {
            Way::Here { .. } => "tarae attach".into(),
            Way::Ssh { url, .. } => format!("tarae attach {}", url.written),
        }
    }
}

/// A way's two ends — the host's socket, or ssh's pipes — and ssh while it's the way.
struct Link {
    to: Box<dyn Write + Send>,
    from: Box<dyn Read + Send>,
    /// `from`'s descriptor (the `Welcome`'s wait).
    fd: RawFd,
    ssh: Option<crate::ssh::Ssh>,
}

impl Link {
    /// Let go before a `Welcome`: ssh, if it's the way, closed and reaped.
    fn end(self) {
        let Link { to, from, ssh, .. } = self;
        drop((to, from));
        if let Some(s) = ssh {
            s.end();
        }
    }
}

/// The way opened (`wait`: a socket that isn't there yet retried that long — a restarted host binding it; `batch`: ssh
/// asks nothing, the terminal being raw).
fn open_way(way: &mut Way, wait: Option<Duration>, batch: bool) -> Result<Link, String> {
    match way {
        Way::Here { path, .. } => {
            let stream = connect_here(path, wait)?;
            let to = stream.try_clone().map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(Link { fd: stream.as_raw_fd(), to: Box::new(to), from: Box::new(stream), ssh: None })
        }
        Way::Ssh { url, tarae, pid } => {
            let relay = crate::ssh::relay_args(url, *pid, wait);
            let o = crate::ssh::open(url, tarae, &relay, batch)?;
            *pid = Some(o.pid);
            crate::log::line(
                "attach",
                "info",
                &format!("over ssh to {}: host {} ({})", url.host, o.pid, o.project),
            );
            Ok(Link { fd: o.from.as_raw_fd(), to: Box::new(o.to), from: Box::new(o.from), ssh: Some(o.ssh) })
        }
    }
}

/// Host `path`'s socket (`wait`: retried that long while it isn't there yet).
fn connect_here(path: &Path, wait: Option<Duration>) -> Result<UnixStream, String> {
    let until = crate::clock::now() + wait.unwrap_or_default();
    loop {
        match crate::attach::connect(path) {
            Ok(s) => return Ok(s),
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
    }
}

/// The way opened, the host's `Hello` said and its `Welcome` in. What came after the `Welcome` stays in the decoder.
fn attach_to(
    way: &mut Way,
    ground: wire::Ground,
    wait: Option<Duration>,
    batch: bool,
) -> Result<(Link, Decoder<ToInterface>), String> {
    let mut link = open_way(way, wait, batch)?;
    match greet(&mut link, ground) {
        Ok(dec) => Ok((link, dec)),
        Err(e) => {
            let ssh_said = link.ssh.as_ref().and_then(|s| s.last_line());
            link.end();
            Err(match ssh_said {
                Some(l) => format!("{e} (ssh: {l})"),
                None => e,
            })
        }
    }
}

/// `Hello` said on `link`, the host's `Welcome` read within `WELCOME_WITHIN`.
fn greet(link: &mut Link, ground: wire::Ground) -> Result<Decoder<ToInterface>, String> {
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
    send(&mut link.to, &ToHost::Hello(hello)).map_err(|e| format!("saying hello: {e}"))?;
    let until = crate::clock::now() + WELCOME_WITHIN;
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
            Ok(None) => {
                if !readable_within(link.fd, until) {
                    return Err(format!("no Welcome within {} s", WELCOME_WITHIN.as_secs()));
                }
                match dec.read_from(&mut link.from) {
                    Ok(0) => return Err("the host's end closed before its Welcome".into()),
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(format!("reading the host's Welcome: {e}")),
                }
            }
        }
    }
    Ok(dec)
}

/// Whether `fd` has something to read (or its end) before `until`.
fn readable_within(fd: RawFd, until: std::time::Instant) -> bool {
    loop {
        let left = until.saturating_duration_since(crate::clock::now());
        let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: poll reads and writes only the one pollfd it's given.
        let n = unsafe { libc::poll(&mut p, 1, left.as_millis().min(i32::MAX as u128) as i32) };
        match n {
            1.. => return true,
            0 => return false,
            _ if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted => {}
            _ => return true,
        }
    }
}

fn send(s: &mut (impl Write + ?Sized), m: &ToHost) -> io::Result<()> {
    let mut out = Vec::new();
    wire::encode(m, &mut out).map_err(|e| io::Error::other(e.said))?;
    s.write_all(&out)?;
    s.flush()
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
    way: Way,
    ground: wire::Ground,
    /// The link's end toward the host (its other end is `listen`'s thread's).
    to: Box<dyn Write + Send>,
    /// ssh while it's the way.
    ssh: Option<crate::ssh::Ssh>,
    /// The link's generation: what an older one's thread still says is passed over.
    link_n: u32,
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
    fn listen(&self, mut from: Box<dyn Read + Send>, mut dec: Decoder<ToInterface>) {
        let tx = self.tx.clone();
        let n = self.link_n;
        let closed = if self.ssh.is_some() { "the link closed" } else { "its socket closed" };
        crate::event::thread("tarae-wire-host", move || {
            let why = loop {
                match dec.message() {
                    Ok(Some(m)) => {
                        if tx.send(Heard::Host(n, m)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => match dec.read_from(&mut from) {
                        Ok(0) => break closed.to_string(),
                        Ok(_) => {}
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        Err(e) => break e.to_string(),
                    },
                    Err(e) => break format!("its bytes were wrong: {e}"),
                }
            };
            let _ = tx.send(Heard::Ended(n, why));
        });
    }

    /// The client goes: ssh, if it's the way, let go (its stdin closed first: the relay's EOF, the host's detach).
    fn let_go(&mut self) {
        self.to = Box::new(io::sink());
        if let Some(s) = self.ssh.take() {
            s.end();
        }
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
                Heard::Host(n, _) | Heard::Ended(n, _) if n != self.link_n => {}
                Heard::Host(_, m) if !ended => match self.heard(m) {
                    // (over ssh the host's new binary is the other machine's: ssh again, the relay waiting for it)
                    Some(End::Exec(_)) if self.ssh.is_some() => {
                        if let Err(e) = self.again(Some(RESTART_WAIT)) {
                            ended = true;
                            self.due = None;
                            let _ = crate::term::last_line(&format!(
                                " the host restarted, but: {e} — r attach again · q quit"
                            ));
                        }
                    }
                    Some(end) => return end,
                    None => {}
                },
                Heard::Host(..) => {}
                Heard::Ended(_, why) if !ended => {
                    ended = true;
                    self.due = None;
                    let why = match self.ssh.as_ref().and_then(|s| s.last_line()) {
                        Some(l) => format!("{why}; ssh: {l}"),
                        None => why,
                    };
                    let _ =
                        crate::term::last_line(&format!(" the host ended ({why}) — r attach again · q quit"));
                }
                Heard::Ended(..) => {}
                Heard::Signal => {
                    let _ = send(&mut self.to, &ToHost::Bye);
                    return End::Said(String::new());
                }
                Heard::Done { id, answer, osc } => {
                    // (its OSC 52 after the frame on screen, as tarae's own goes after its frame)
                    if !osc.is_empty() {
                        let _ = write_out(&osc);
                    }
                    if !ended {
                        let _ = send(&mut self.to, &ToHost::Answer { id, answer });
                    }
                }
                Heard::Input(ev) if ended => match ev {
                    Event::Key(k) if k.code == crate::key::Code::Char('r') && !k.ctrl && !k.alt => {
                        match self.again(None) {
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
                        let _ = send(&mut self.to, &ToHost::Resize { w, h });
                    }
                }
                Heard::Input(ev) => {
                    if let Event::Focus(true) = ev {
                        self.zone_again(ended);
                    }
                    if let Some(event) = crate::attach::input_of(ev) {
                        self.seq += 1;
                        let _ = send(&mut self.to, &ToHost::Input { seq: self.seq, seen: self.seen, event });
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
                    let _ = send(&mut self.to, &ToHost::Bye);
                    return Some(End::Failed("this terminal can't be written to".into()));
                }
                self.held.apply(&bytes, w, h);
                self.seen = n;
                let _ = send(&mut self.to, &ToHost::Ack { frame: n });
                if !bytes.is_empty() {
                    self.due = Some(crate::clock::now() + crate::interface::RESTING_REDRAW);
                }
                None
            }
            ToInterface::Gone { why, said } => Some(match why {
                Gone::Detached => End::Said(format!("detached — {} comes back", self.way.again_said())),
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
        let _ = send(&mut self.to, &ToHost::Zone { zone, offset });
    }

    /// Attached again — `r` on the host-ended line, a restart over ssh (`wait`: the relay waits for its socket). Here:
    /// the host looked for again (the one asked for, else the one `tarae state` picks); over ssh: ssh again, asking
    /// nothing (the terminal is raw), to the host it had.
    fn again(&mut self, wait: Option<Duration>) -> Result<(), String> {
        if let Way::Here { pid, path } = &mut self.way
            && let Ok((p, at)) = crate::attach::find(None)
        {
            (*pid, *path) = (p, at);
        }
        let (link, dec) = attach_to(&mut self.way, self.ground, wait, true)?;
        self.let_go();
        let Link { to, from, ssh, .. } = link;
        if let Some(s) = &ssh {
            s.quiet();
        }
        (self.to, self.ssh) = (to, ssh);
        self.link_n += 1;
        self.held = crate::screen::Held::new();
        self.listen(from, dec);
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
            // (inside a multiplexer the program and this terminal: the host told the program's, as protocol v0 can say)
            Ok(Wrote::Both(_, osc)) => (wire::Answer::Copied(None), osc),
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
