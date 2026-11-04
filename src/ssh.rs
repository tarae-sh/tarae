//! `tarae attach ssh://…` (the remote plan's §8d, step 5): the client's way to a host on another machine — `ssh` run in
//! this terminal's session (never setsid: it asks for a password or a host key on `/dev/tty`, the terminal cooked till
//! then), its remote command a login shell's `tarae attach --stdio` (`relay.rs`), which says a text preamble and then
//! carries protocol v0 unread. Here: the address read (`parse`), the remote line quoted so a login shell reads it as
//! written, sh's family and fish alike (`remote`), the preamble found past an rc file's noise (`preamble`), and ssh run,
//! its stderr shown until the client's raw mode and its last lines kept for what's said after (`open`).

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The relay's first line: what's before it is noise (an rc file's), what's after is the relay's.
pub const MAGIC: &[u8] = b"TARAE-STDIO 1\n";
/// Noise skipped looking for `MAGIC`, at most; past it what came is said.
pub const NOISE_MAX: usize = 64 << 10;
/// ssh's stderr lines kept for what's said once the terminal is raw.
const STDERR_KEPT: usize = 20;
/// The relay's `no` and its list, at most.
const NO_MAX: usize = 64 << 10;

/// `ssh://[user@]host[:port][/path][?pid=N]`, read.
#[derive(Clone, Debug, PartialEq)]
pub struct Url {
    pub user: Option<String>,
    /// As ssh takes it: `~/.ssh/config`'s aliases apply (an IPv6 address without its brackets).
    pub host: String,
    pub port: Option<u16>,
    /// The folder whose host is wanted, as the relay takes it: `~`, `~/x` (the remote home's), or `/x`. None: the
    /// relay picks the only host there.
    pub path: Option<String>,
    pub pid: Option<u32>,
    /// As it was written, for what's said (`tarae attach <it> comes back`).
    pub written: String,
}

/// `s` read as an `ssh://` address — Err says what in it isn't taken.
pub fn parse(s: &str) -> Result<Url, String> {
    let rest = s.strip_prefix("ssh://").ok_or_else(|| format!("{s}: not an ssh:// address"))?;
    if rest.contains('#') {
        return Err(format!("{s}: a # part isn't read"));
    }
    let (rest, query) = rest.split_once('?').map_or((rest, None), |(a, q)| (a, Some(q)));
    let (authority, path) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
    let (user, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };
    let user = match user {
        None => None,
        Some(u) if u.contains(':') => {
            return Err(format!("{s}: a password in the address isn't read — ssh asks for it"));
        }
        Some(u) => Some(
            name("user", &decode(u).map_err(|e| format!("{s}: {e}"))?).map_err(|e| format!("{s}: {e}"))?,
        ),
    };
    // (the port: None none written, Some its text — an empty one too, refused below)
    let (host, port) = if let Some(v6) = hostport.strip_prefix('[') {
        let (h, after) = v6.split_once(']').ok_or_else(|| format!("{s}: an unclosed [ in the host"))?;
        match after.strip_prefix(':') {
            Some(p) => (h.to_string(), Some(p)),
            None if after.is_empty() => (h.to_string(), None),
            None => return Err(format!("{s}: after the host's ] only :PORT")),
        }
    } else {
        match hostport.split_once(':') {
            Some((h, p)) => (decode(h).map_err(|e| format!("{s}: {e}"))?, Some(p)),
            None => (decode(hostport).map_err(|e| format!("{s}: {e}"))?, None),
        }
    };
    let host = name("host", &host).map_err(|e| format!("{s}: {e}"))?;
    let port = match port {
        None => None,
        Some(p) => match p.parse::<u16>() {
            Ok(n) if n > 0 && p.bytes().all(|b| b.is_ascii_digit()) => Some(n),
            _ => return Err(format!("{s}: the port {p:?} isn't a number from 1 to 65535")),
        },
    };
    let path = place(&decode(path).map_err(|e| format!("{s}: {e}"))?).map_err(|e| format!("{s}: {e}"))?;
    let mut pid = None;
    for pair in query.into_iter().flat_map(|q| q.split('&')).filter(|p| !p.is_empty()) {
        match pair.split_once('=') {
            Some(("pid", n)) if pid.is_none() => {
                // (written plainly: no sign, no leading zero)
                pid = Some(
                    n.parse::<u32>()
                        .ok()
                        .filter(|p| *p > 0 && p.to_string() == n)
                        .ok_or_else(|| format!("{s}: pid={n} isn't a process number"))?,
                )
            }
            Some(("pid", _)) => return Err(format!("{s}: pid given twice")),
            _ => return Err(format!("{s}: ?{pair} isn't read — only ?pid=N")),
        }
    }
    Ok(Url { user, host, port, path, pid, written: s.to_string() })
}

/// `%XX` decoded — UTF-8 after it, and no escape left half-written.
fn decode(s: &str) -> Result<String, String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok());
            let byte = hex
                .filter(|h| h.bytes().all(|c| c.is_ascii_hexdigit()))
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| format!("a % not followed by two hex digits at {:?}", &s[i..]))?;
            out.push(byte);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "what's %-escaped isn't UTF-8".to_string())
}

/// A user or host as ssh is given it: something, never read as an option, no blank or control character.
fn name(what: &str, s: &str) -> Result<String, String> {
    if s.is_empty() {
        return Err(format!("no {what}"));
    }
    if s.starts_with('-') {
        return Err(format!("the {what} {s:?} starts with - (ssh would read it as an option)"));
    }
    if s.chars().any(|c| c.is_whitespace() || c.is_control() || c == '/') {
        return Err(format!("the {what} {s:?} holds a blank, a / or a control character"));
    }
    Ok(s.to_string())
}

/// The address's path as the relay takes it: `/~` and `/~/x` the remote home's (Zed's way), `/x` as written; none
/// (or `/`) for the relay's pick.
fn place(path: &str) -> Result<Option<String>, String> {
    if path.chars().any(|c| c.is_control()) {
        return Err("the path holds a control character".into());
    }
    Ok(match path {
        "" | "/" => None,
        "/~" | "/~/" => Some("~".into()),
        p => Some(p.strip_prefix("/~/").map_or_else(|| p.to_string(), |rel| format!("~/{rel}"))),
    })
}

/// `s` as one word for a login shell — read the same by sh, bash, zsh and fish: single quotes, with each `'` and `\`
/// outside them, escaped (inside fish's single quotes `\\` and `\'` are escapes, inside sh's nothing is; outside both
/// read `\'` and `\\` as the character).
pub fn quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        match c {
            '\'' | '\\' => {
                out.push('\'');
                out.push('\\');
                out.push(c);
                out.push('\'');
            }
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// The remote command: the login shell's PATH (`~/.zprofile`, `~/.profile`: Homebrew's, cargo's — what `ssh host cmd`'s
/// shell lacks), then `tarae attach --stdio args…` (`tarae`: `--remote-tarae`'s, else the PATH's). Two shells read it
/// — the login shell this line, `$SHELL -lc` its argument — so each word is quoted twice.
pub fn remote(tarae: &str, args: &[String]) -> String {
    let mut inner = format!("{} attach --stdio", quote(tarae));
    for a in args {
        inner.push(' ');
        inner.push_str(&quote(a));
    }
    format!("exec \"$SHELL\" -lc {}", quote(&inner))
}

/// The relay's arguments for `url` (`wait`: `--wait S`, a restarted host's; `pid`: the host it had).
pub fn relay_args(url: &Url, pid: Option<u32>, wait: Option<Duration>) -> Vec<String> {
    let mut a = Vec::new();
    if let Some(w) = wait {
        a.extend(["--wait".into(), w.as_secs().to_string()]);
    }
    match pid.or(url.pid) {
        Some(p) => a.extend(["--pid".into(), p.to_string()]),
        None => a.extend(url.path.clone()),
    }
    a
}

/// ssh's arguments (S-A): no pty (`-T`: piece 0 measured a pty's keys 10–16 ms late, `ObscureKeystrokeTiming`), no
/// escape character, a dead link noticed in 45 s; `batch`: no prompt (the terminal is raw); no flag of the user's — that's
/// `~/.ssh/config`'s.
pub fn args(url: &Url, remote: &str, batch: bool) -> Vec<String> {
    let mut a: Vec<String> =
        ["-T", "-e", "none", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3"]
            .map(String::from)
            .into();
    if batch {
        a.extend(["-o".into(), "BatchMode=yes".into()]);
    }
    if let Some(p) = url.port {
        a.extend(["-p".into(), p.to_string()]);
    }
    if let Some(u) = &url.user {
        a.extend(["-l".into(), u.clone()]);
    }
    a.extend([url.host.clone(), "--".into(), remote.into()]);
    a
}

/// What the relay said after `MAGIC`.
#[derive(Debug, PartialEq)]
pub enum Said {
    /// Connected to host `pid`, its project.
    Ok { pid: u32, project: String },
    /// No host: why, and the hosts there (a line each).
    No { why: String, hosts: Vec<String> },
}

/// Why no preamble came.
#[derive(Debug)]
pub enum Before {
    /// The stream ended before `MAGIC`: what came (noise, or a relay that isn't one).
    Ended(Vec<u8>),
    /// `NOISE_MAX` bytes and no `MAGIC`: their start.
    TooMuch(Vec<u8>),
    /// `MAGIC` came, then something that isn't the relay's word.
    Garbled(String),
    Io(io::Error),
}

/// The preamble read from `r`: `MAGIC` past up to `NOISE_MAX` of noise, then `ok PID PROJECT`, or `no WHY` and the
/// hosts to the stream's end. Read a byte at a time: nothing after the `ok` line is taken (the protocol's).
pub fn preamble(r: &mut impl Read) -> Result<Said, Before> {
    let mut seen = Vec::new();
    loop {
        if seen.ends_with(MAGIC) {
            break;
        }
        if seen.len() >= NOISE_MAX + MAGIC.len() {
            seen.truncate(NOISE_MAX);
            return Err(Before::TooMuch(seen));
        }
        match byte(r).map_err(Before::Io)? {
            Some(b) => seen.push(b),
            None => return Err(Before::Ended(seen)),
        }
    }
    let mut line = Vec::new();
    while let Some(b) = byte(r).map_err(Before::Io)? {
        if b == b'\n' {
            break;
        }
        line.push(b);
        if line.len() > NO_MAX {
            break;
        }
    }
    let line = String::from_utf8_lossy(&line).into_owned();
    if let Some(ok) = line.strip_prefix("ok ") {
        let (pid, project) = ok.split_once(' ').unwrap_or((ok, ""));
        let pid = pid.parse::<u32>().map_err(|_| Before::Garbled(line.clone()))?;
        return Ok(Said::Ok { pid, project: project.to_string() });
    }
    let Some(why) = line.strip_prefix("no ") else { return Err(Before::Garbled(line)) };
    let mut rest = Vec::new();
    let _ = r.take(NO_MAX as u64).read_to_end(&mut rest);
    let hosts =
        String::from_utf8_lossy(&rest).lines().filter(|l| !l.trim().is_empty()).map(String::from).collect();
    Ok(Said::No { why: why.to_string(), hosts })
}

fn byte(r: &mut impl Read) -> io::Result<Option<u8>> {
    let mut b = [0u8; 1];
    loop {
        match r.read(&mut b) {
            Ok(0) => return Ok(None),
            Ok(_) => return Ok(Some(b[0])),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

/// ssh's stderr: shown on this terminal while it's cooked (its warnings, a host key's), its last lines kept.
#[derive(Default)]
struct Stderr {
    shown: bool,
    last: VecDeque<String>,
    part: Vec<u8>,
}

impl Stderr {
    fn heard(&mut self, bytes: &[u8]) {
        if self.shown {
            let mut e = io::stderr().lock();
            let _ = e.write_all(bytes);
            let _ = e.flush();
        }
        for &b in bytes {
            if b == b'\n' {
                self.line();
            } else if self.part.len() < 4096 {
                self.part.push(b);
            }
        }
    }

    fn line(&mut self) {
        let l = String::from_utf8_lossy(&self.part).trim_end_matches('\r').to_string();
        self.part.clear();
        if l.trim().is_empty() {
            return;
        }
        if self.last.len() == STDERR_KEPT {
            self.last.pop_front();
        }
        self.last.push_back(l);
    }
}

/// ssh running, to a relay that said `ok`.
pub struct Ssh {
    child: Child,
    stderr: Arc<Mutex<Stderr>>,
}

/// ssh's two pipes and the relay's `ok`.
pub struct Opened {
    pub ssh: Ssh,
    pub to: ChildStdin,
    pub from: ChildStdout,
    pub pid: u32,
    pub project: String,
}

/// ssh run to `url`'s relay (`relay`: its arguments; `tarae`: the remote one; `batch`: no prompt — the terminal is
/// raw), its preamble read. Err: why, in words — ssh's own last line where it failed, a missing or older tarae there,
/// the relay's no and its list.
pub fn open(url: &Url, tarae: &str, relay: &[String], batch: bool) -> Result<Opened, String> {
    let line = remote(tarae, relay);
    let mut child = Command::new("ssh")
        .args(args(url, &line, batch))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ssh can't run: {e}"))?;
    let stderr = Arc::new(Mutex::new(Stderr { shown: !batch, ..Default::default() }));
    let reader = child.stderr.take().map(|mut err| {
        let kept = Arc::clone(&stderr);
        crate::event::thread("tarae-ssh-err", move || {
            let mut buf = [0u8; 4096];
            loop {
                match err.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => lock(&kept).heard(&buf[..n]),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            lock(&kept).line();
        })
    });
    let (Some(to), Some(mut from)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("ssh's pipes weren't made".into());
    };
    let said = preamble(&mut from);
    let ended = |mut child: Child| {
        // (its stdout over: ssh ends, its stderr read to the end — the last line is its reason)
        let status = child.wait().ok();
        if let Some(r) = reader {
            let _ = r.join();
        }
        (status.and_then(|s| s.code()), lock(&stderr).last.iter().cloned().collect::<Vec<_>>())
    };
    let host = &url.host;
    match said {
        Ok(Said::Ok { pid, project }) => {
            Ok(Opened { ssh: Ssh { child, stderr: Arc::clone(&stderr) }, to, from, pid, project })
        }
        Ok(Said::No { why, hosts }) => {
            drop((to, from));
            let _ = ended(child);
            let mut said = why;
            for h in hosts {
                said += &format!("\n  {h}");
            }
            Err(said)
        }
        Err(Before::TooMuch(noise)) => {
            let _ = child.kill();
            drop((to, from));
            let _ = ended(child);
            Err(format!(
                "{host} said no {} line in its first {} KB — what came:\n{}",
                MAGIC_WORD,
                NOISE_MAX >> 10,
                shown(&noise)
            ))
        }
        Err(Before::Garbled(line)) => {
            let _ = child.kill();
            drop((to, from));
            let _ = ended(child);
            Err(format!("the relay on {host} said what this tarae can't read: {line:?}"))
        }
        Err(Before::Ended(noise)) => {
            drop((to, from));
            let (code, last) = ended(child);
            Err(before_said(url, tarae, code, &last, &noise))
        }
        Err(Before::Io(e)) => {
            let _ = child.kill();
            drop((to, from));
            let _ = ended(child);
            Err(format!("reading ssh {host}: {e}"))
        }
    }
}

/// `MAGIC` as said.
const MAGIC_WORD: &str = "TARAE-STDIO";

/// What came before a stream ended, shown: its start, lossy.
fn shown(noise: &[u8]) -> String {
    let s = String::from_utf8_lossy(&noise[..noise.len().min(400)]).into_owned();
    if noise.len() > 400 { format!("{s}…") } else { s }
}

/// Why ssh ended before the relay spoke, in words: by its exit code (255 ssh's own, 127 no such program) and its last
/// stderr lines (an older tarae's `unknown: --stdio`).
pub fn before_said(url: &Url, tarae: &str, code: Option<i32>, last: &[String], noise: &[u8]) -> String {
    let host = &url.host;
    let line = last.last().map(String::as_str).unwrap_or("no word on its stderr");
    if last.iter().any(|l| l.contains("unknown: --stdio")) {
        return format!(
            "the tarae on {host} is older than this one (it has no attach --stdio) — update it there"
        );
    }
    match code {
        Some(255) => format!("ssh {host}: {line}"),
        Some(127) => {
            if tarae == "tarae" {
                format!(
                    "no tarae on {host} — its login shell's PATH has none ({line}); --remote-tarae PATH names one"
                )
            } else {
                format!("no tarae at {tarae} on {host} ({line})")
            }
        }
        code => {
            let code = code.map_or("a signal".into(), |c| format!("exit {c}"));
            let noise =
                if noise.is_empty() { String::new() } else { format!("; it printed:\n{}", shown(noise)) };
            format!("ssh {host} ended ({code}) before the relay spoke: {line}{noise}")
        }
    }
}

impl Ssh {
    /// From here on the terminal is the client's: ssh's stderr kept, never shown (it would tear the frame).
    pub fn quiet(&self) {
        lock(&self.stderr).shown = false;
    }

    /// ssh's last stderr line, if it said any.
    pub fn last_line(&self) -> Option<String> {
        lock(&self.stderr).last.back().cloned()
    }

    /// ssh let go: given a moment to end once its stdin closed (the caller dropped it), then SIGTERM, reaped.
    pub fn end(mut self) {
        let until = crate::clock::now() + Duration::from_millis(500);
        while crate::clock::now() < until {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            crate::event::pause(Duration::from_millis(10));
        }
        self.cut();
    }

    /// ssh cut at once — a link that answers nothing (no `Pong`): SIGTERM, reaped.
    pub fn cut(mut self) {
        // SAFETY: kill only sends a signal to ssh, our child, not yet reaped.
        unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
        let _ = self.child.wait();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests;
