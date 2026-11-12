//! A crash's report (#541): one file a crash, `$XDG_STATE_HOME/tarae/crashed/reports/<UTC yyyymmdd-hhmmss>-<pid>.txt`
//! (0700 folder, 0600 file), and `<name>.unseen` beside it until a start has said it. Fixed fields, one a line — when,
//! build, the binary's load address and UUID, OS, pid, thread, where, the message's head (`crash::cut`, redacted),
//! where tarae started, the pane being drawn, what the keeper kept, where the build's symbols are kept — then the
//! frames: as return addresses first (`addresses`, written before anything reads a dSYM), replaced by the symbolized
//! ones when that ends within the hook's wait (`symbolize_now`), else by the next start (`atos` on the kept dSYM,
//! macOS). Never the environment, the arguments, a buffer's text: nothing but those fields is written, and the frames
//! are code addresses and the names and source lines they resolve to. Home is written as `~`.
//!
//! The next start, in any folder (`start_job`, a worker): each unseen report symbolized if it can be, said once — the
//! one the folder's marker names already said with it (`crash::said`) — and the newest `KEEP` kept, never an unseen one
//! pruned.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Crashed, build_said, tilde};
use crate::recovery::Flushed;

/// The reports' folder in `crashed/`.
pub const DIR: &str = "reports";
/// How many reports are kept — the newest.
pub const KEEP: usize = 10;
const UNSEEN: &str = "unseen";
const TXT: &str = "txt";
/// Frames captured at most.
const FRAMES_MAX: usize = 128;
/// The frames' heading in a report, and its line's start.
const BACKTRACE: &str = "backtrace ";

/// A report's frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frames {
    /// Return addresses, as captured.
    Addresses(Vec<usize>),
    /// Symbolized: `by` says when and how (`at the crash, in 180ms`), `text` the frames.
    Symbolized { by: String, text: String },
}

/// What a report says.
pub struct Report {
    pub crashed: Crashed,
    pub when: SystemTime,
    pub pid: u32,
    pub running: Option<crate::dsyms::Running>,
    pub symbols: Option<PathBuf>,
    pub root: PathBuf,
    pub unsaved: Flushed,
    pub frames: Frames,
}

/// The panicking thread's return addresses (`backtrace(3)`): no allocation, no symbols read.
pub fn addresses() -> Vec<usize> {
    let mut buf = [std::ptr::null_mut::<libc::c_void>(); FRAMES_MAX];
    // SAFETY: `buf` holds `FRAMES_MAX` pointers; `backtrace` writes at most that many and says how many it wrote.
    let n = unsafe { libc::backtrace(buf.as_mut_ptr(), FRAMES_MAX as libc::c_int) };
    buf[..(n.max(0) as usize).min(FRAMES_MAX)].iter().map(|p| *p as usize).collect()
}

/// `when` as a report's name says it, UTC: `20261112-041233`; and as its `when` field does, `2026-11-12 04:12:33 UTC`.
fn stamp(when: SystemTime) -> (String, String) {
    let s = when.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (y, mo, d) = crate::log::civil(s.div_euclid(86_400));
    let t = s.rem_euclid(86_400);
    let (h, mi, se) = (t / 3600, t % 3600 / 60, t % 60);
    (
        format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{se:02}"),
        format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{se:02} UTC"),
    )
}

/// The `unsaved` field's words for what the keeper did.
fn unsaved_said(f: Flushed) -> String {
    match f {
        Flushed::Nothing => "none".into(),
        Flushed::Kept(1) => "1 file kept".into(),
        Flushed::Kept(n) => format!("{n} files kept"),
        Flushed::Unanswered | Flushed::KeeperItself => {
            format!("kept as they were up to {} s before", crate::recovery::MOST.as_secs())
        }
    }
}

/// A report's text — its fields in their order, then its frames; home as `~` all through.
pub fn text(r: &Report, home: Option<&Path>) -> String {
    let c = &r.crashed;
    let binary = match &r.running {
        Some(b) => match &b.uuid {
            Some(id) => format!("loaded at {:#x}, UUID {id}", b.load),
            None => format!("loaded at {:#x}", b.load),
        },
        None => "unknown".into(),
    };
    let drawing = c.drawing.as_ref().map_or("none".into(), super::pane_said);
    let symbols = r.symbols.as_ref().map_or("none kept".into(), |p| tilde(p, home));
    let fields = [
        ("when", stamp(r.when).1),
        ("build", c.build.clone()),
        ("binary", binary),
        ("os", format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)),
        ("pid", r.pid.to_string()),
        ("thread", c.thread.clone()),
        ("at", c.at.clone()),
        ("message", c.message.clone()),
        ("root", tilde(&r.root, home)),
        ("drawing", drawing),
        ("unsaved", unsaved_said(r.unsaved)),
        ("symbols", symbols),
    ];
    let mut out = String::from("tarae crash report\n");
    for (k, v) in fields {
        out += &format!("{k:<10}{v}\n");
    }
    out += &frames_text(&r.frames, r.symbols.is_some());
    match home.map(|h| format!("{}/", h.display())) {
        Some(h) if h.len() > 1 => out.replace(&h, "~/"),
        _ => out,
    }
}

/// The frames' part: its heading line, then a line a frame — never an empty heading: no frames say so.
fn frames_text(f: &Frames, symbols: bool) -> String {
    match f {
        Frames::Addresses(a) if a.is_empty() => {
            format!("{BACKTRACE}none — this build's capture found no frames\n")
        }
        Frames::Addresses(a) => {
            let later = match symbols && cfg!(target_os = "macos") {
                true => "symbolized at the next start, from `symbols`",
                false => "no symbols kept for this build",
            };
            let mut out = format!("{BACKTRACE}addresses only — {later}\n");
            for (i, a) in a.iter().enumerate() {
                out += &format!("{i:>4}: {a:#018x}\n");
            }
            out
        }
        Frames::Symbolized { by, text } => format!("{BACKTRACE}symbolized {by}\n{}\n", text.trim_end()),
    }
}

/// Report `r` written new in `dir` (made 0700), its frames as they are, marked unseen — where.
pub fn write_new(dir: &Path, r: &Report, home: Option<&Path>) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt as _;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let name = format!("{}-{}", stamp(r.when).0, r.pid);
    let path = dir.join(format!("{name}.{TXT}"));
    let body = text(r, home);
    crate::disk::write_private(&path, |w| w.write_all(body.as_bytes()))?;
    crate::disk::write_private(&dir.join(format!("{name}.{UNSEEN}")), |_| Ok(()))?;
    Ok(path)
}

/// The frames symbolized now (std's backtrace, which reads the dSYM beside the binary when it's this build's), and the
/// report at `path` replaced at once — whether that was done: not when no frame resolved to a source line (a stripped
/// build with no symbols beside it), the addresses then left for the next start.
pub fn symbolize_now(
    path: &Path,
    r: &mut Report,
    backtrace: &std::backtrace::Backtrace,
    home: Option<&Path>,
) -> bool {
    let start = crate::clock::now();
    let text = format!("{backtrace:#}");
    if !resolved(&text) {
        return false;
    }
    let took = crate::clock::since(start);
    let text = trimmed(&text, env!("CARGO_MANIFEST_DIR"));
    r.frames =
        Frames::Symbolized { by: format!("at the crash, in {}", crate::term::run_duration(took)), text };
    let body = self::text(r, home);
    crate::disk::write_private(path, |w| w.write_all(body.as_bytes())).is_ok()
}

/// std's frames without its own around the panic: those up to its short backtrace's end marker (the capture, this
/// hook) and from its begin marker on (the runtime's start) — as std's short form cuts them — and, before the first of
/// tarae's own (its source under `own`, tarae's tree), the panic's machinery and any dependency's: the first frame kept
/// is tarae's (where a dependency panicked, the report's `at` names it). No marker, no frame of tarae's: those cuts
/// aren't made.
fn trimmed(text: &str, own: &str) -> String {
    let mut frames: Vec<String> = Vec::new();
    for l in text.lines() {
        let head = l.trim_start().split_once(':').is_some_and(|(n, _)| n.parse::<usize>().is_ok());
        match frames.last_mut() {
            Some(f) if !head => {
                f.push('\n');
                f.push_str(l);
            }
            _ => frames.push(l.to_string()),
        }
    }
    let first = |f: &String| f.lines().next().unwrap_or_default().to_string();
    let from =
        frames.iter().position(|f| first(f).contains("__rust_end_short_backtrace")).map_or(0, |i| i + 1);
    let to = (frames.iter().skip(from))
        .position(|f| first(f).contains("__rust_begin_short_backtrace"))
        .map_or(frames.len(), |i| from + i);
    let tarae = |f: &String| {
        f.lines().skip(1).any(|l| l.trim_start().strip_prefix("at ").is_some_and(|p| p.starts_with(own)))
    };
    let from = frames[from..to].iter().position(tarae).map_or(from, |i| from + i);
    frames[from..to].join("\n")
}

#[cfg(test)]
pub(super) fn trimmed_for(text: &str, own: &str) -> String {
    trimmed(text, own)
}

/// std's frames name a source line somewhere (`at <file>:<line>`).
fn resolved(frames: &str) -> bool {
    frames.lines().any(|l| l.trim_start().starts_with("at "))
}

/// A report read back: its fields, and its frames' heading and lines.
#[derive(Debug, Default)]
pub struct Read {
    pub fields: Vec<(String, String)>,
    pub heading: String,
    pub frames: Vec<String>,
}

impl Read {
    pub fn field(&self, k: &str) -> &str {
        self.fields.iter().find(|f| f.0 == k).map_or("", |f| f.1.as_str())
    }

    /// Its frames' addresses, when that's all it has.
    fn addresses(&self) -> Option<Vec<usize>> {
        if !self.heading.starts_with("addresses only") {
            return None;
        }
        let hex = |l: &str| l.split("0x").nth(1).and_then(|h| usize::from_str_radix(h.trim(), 16).ok());
        Some(self.frames.iter().filter_map(|l| hex(l)).collect())
    }
}

pub fn read(text: &str) -> Read {
    let mut r = Read::default();
    let mut lines = text.lines().skip(1);
    for l in lines.by_ref() {
        if let Some(h) = l.strip_prefix(BACKTRACE) {
            r.heading = h.to_string();
            break;
        }
        let (k, v) = l.split_at(l.len().min(10));
        r.fields.push((k.trim().to_string(), v.to_string()));
    }
    r.frames = lines.map(str::to_string).collect();
    r
}

/// The report at `path`, its frames addresses only, symbolized from the symbols it names (`atos`, macOS) and replaced —
/// whether it was.
fn symbolize_later(path: &Path, text: &str, home: Option<&Path>) -> bool {
    let r = read(text);
    let Some(addrs) = r.addresses().filter(|a| !a.is_empty()) else { return false };
    let symbols = match r.field("symbols") {
        "none kept" | "" => return false,
        s => match (s.strip_prefix("~/"), home) {
            (Some(rest), Some(h)) => h.join(rest),
            _ => PathBuf::from(s),
        },
    };
    let load = r.field("binary").strip_prefix("loaded at 0x").and_then(|l| l.split(',').next());
    let Some(load) = load.filter(|_| symbols.exists()) else { return false };
    let Some(lines) = atos(&symbols, load, &addrs) else { return false };
    let mut frames = String::new();
    for (i, (a, l)) in addrs.iter().zip(&lines).enumerate() {
        frames += &format!("{i:>4}: {a:#018x} - {l}\n");
    }
    let head = text.find(&format!("\n{BACKTRACE}")).map_or(text, |at| &text[..at + 1]);
    let body = format!("{head}{BACKTRACE}symbolized at the next start, from `symbols`\n{frames}");
    crate::disk::write_private(path, |w| w.write_all(body.as_bytes())).is_ok()
}

/// `atos` on `symbols` (a dSYM, or a binary with its debug map) for `addrs` of an image loaded at `load` (hex): a line
/// an address — None where there's no `atos` (not macOS, no developer tools) or it didn't say one a line.
fn atos(symbols: &Path, load: &str, addrs: &[usize]) -> Option<Vec<String>> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let out = std::process::Command::new("/usr/bin/atos")
        .arg("-o")
        .arg(symbols)
        .args(["-l", &format!("0x{load}")])
        // (a return address is past its call: one byte back is the call's line, as std's symbolizer looks — the
        // first frame's is where it is)
        .args(
            addrs
                .iter()
                .enumerate()
                .map(|(i, a)| format!("{:#x}", if i == 0 { *a } else { a.saturating_sub(1) })),
        )
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let lines: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect();
    (out.status.success() && lines.len() == addrs.len()).then_some(lines)
}

/// A report a start says: where it was left, when, where and which build, what the keeper kept, its path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub path: PathBuf,
    pub when: SystemTime,
    pub root: String,
    pub at: String,
    pub build: String,
    pub unsaved: String,
    pub message: String,
}

/// The start's words for the unseen reports `seen` (the newest last), by `now`: the crash, the unsaved files, where and
/// which build, the report's path — in that order, what a narrow toast cuts from the end.
pub fn words(seen: &[Seen], now: SystemTime, home: Option<&Path>) -> Option<String> {
    let last = seen.last()?;
    let ago = crate::term::ago(now.duration_since(last.when).unwrap_or_default(), true);
    let unsaved = match last.unsaved.as_str() {
        "none" | "" => String::new(),
        "1 file kept" => " — 1 unsaved file waits there".into(),
        u => match u.strip_suffix(" files kept") {
            Some(n) => format!(" — {n} unsaved files wait there"),
            None => " — unsaved changes wait there".into(),
        },
    };
    let on_purpose = last.message == super::ON_PURPOSE;
    let what = match seen.len() {
        1 => format!(
            "{}{} {ago} in {}",
            super::crashed_said(&last.message),
            if on_purpose { "," } else { "" },
            last.root
        ),
        n if on_purpose => format!("tarae crashed {n} times, the last a crash-test {ago} in {}", last.root),
        n => format!("tarae crashed {n} times, last {ago} in {}", last.root),
    };
    let place = format!("{}, build {}", last.at, build_said(&last.build));
    Some(format!("{what}{unsaved} · {place} · report {}", tilde(&last.path, home)))
}

/// What a start's look at the reports found: its words, and the unseen marks to take once they're said (`mark_seen`) —
/// a start that ends before its warning is drawn (a crash loop's) leaves them to the next.
#[derive(Debug, Default)]
pub struct Scanned {
    pub words: Option<String>,
    pub said: Vec<PathBuf>,
}

/// The next start's look at `dir` (a worker): each unseen report symbolized if it can be — `skip`, the one this folder's
/// marker named, said with it already, marked seen now — then the newest `KEEP` kept. What to say, if anything.
pub fn scan(dir: &Path, skip: Option<&Path>, now: SystemTime, home: Option<&Path>) -> Scanned {
    let Ok(entries) = std::fs::read_dir(dir) else { return Scanned::default() };
    let mut unseen: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(&format!(".{UNSEEN}"))?.to_string().into())
        .collect();
    crate::sorting::sort(&mut unseen);
    let (mut seen, mut said) = (Vec::new(), Vec::new());
    for name in unseen {
        let mark = dir.join(format!("{name}.{UNSEEN}"));
        let path = dir.join(format!("{name}.{TXT}"));
        let when = std::fs::metadata(&path).and_then(|m| m.modified()).unwrap_or(now);
        if let Ok(text) = std::fs::read_to_string(&path) {
            symbolize_later(&path, &text, home);
            let r = read(&text);
            // (by name: one folder holds them all, and a path can be said two ways — `/tmp`, `/private/tmp`)
            if skip.and_then(Path::file_name) != path.file_name() {
                let f = |k: &str| r.field(k).to_string();
                let (root, at, build, unsaved) = (f("root"), f("at"), f("build"), f("unsaved"));
                seen.push(Seen { path, when, root, at, build, unsaved, message: f("message") });
                said.push(mark);
                continue;
            }
        }
        let _ = std::fs::remove_file(mark);
    }
    prune(dir);
    Scanned { words: words(&seen, now, home), said }
}

/// The reports `said` (`Scanned.said`) marked seen — once their words are up.
pub fn mark_seen(said: &[PathBuf]) {
    for mark in said {
        let _ = std::fs::remove_file(mark);
    }
}

/// The reports in `dir` past the newest `KEEP` (by name: their time) removed — never one still unseen. What went.
pub fn prune(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(&format!(".{TXT}")).map(str::to_string))
        .collect();
    crate::sorting::sort(&mut names);
    let over = names.len().saturating_sub(KEEP);
    let mut gone = Vec::new();
    for name in &names[..over] {
        if dir.join(format!("{name}.{UNSEEN}")).exists() {
            continue;
        }
        let path = dir.join(format!("{name}.{TXT}"));
        if std::fs::remove_file(&path).is_ok() {
            gone.push(path);
        }
    }
    gone
}

/// The start's work on a crash's reports and symbols, in its order: the waiting reports looked at and symbolized first
/// (`scan`, in `dir` — none without a state folder), then this build's dSYM kept and the old ones pruned (`keep`) — so a
/// report never loses the dSYM it needs to a prune beside it.
pub fn start_work(
    dir: Option<&Path>,
    skip: Option<&Path>,
    now: SystemTime,
    home: Option<&Path>,
    keep: impl FnOnce(),
) -> Scanned {
    let found = dir.map(|d| scan(d, skip, now, home)).unwrap_or_default();
    keep();
    found
}

/// The start's look at the reports, any folder's, and this build's symbols kept (a worker, `start_work`): what it found
/// said as a warning and in the log, once. `skip`: the one this folder's marker named (`crash::said` says it).
pub fn start_job(ed: &mut crate::editor::Editor, dir: Option<PathBuf>, skip: Option<PathBuf>) {
    let home = crate::allow::home();
    ed.events.jobs().spawn(move || {
        let now = crate::clock::system();
        let found = start_work(dir.as_deref(), skip.as_deref(), now, home.as_deref(), keep_symbols);
        move |ed: &mut crate::editor::Editor| match found.words {
            Some(w) => {
                let logged = w.clone();
                super::said_at_start(ed, w, &logged, false);
                // (said: marked seen — a worker's, as it's the disk)
                let said = found.said;
                ed.events.jobs().background(move || mark_seen(&said));
            }
            None => ed.unchanged = true,
        }
    });
}

/// The start's (a worker, `detached`): the running build's dSYM kept by its UUID (`dsyms::keep`), so a crash's frames can
/// be symbolized after the next build replaces the one beside the binary — told to the hook only when it's this image's.
pub fn keep_symbols() {
    // (tests keep nothing of the test binary's: `start_work`'s order is tested with a stand-in)
    if cfg!(test) {
        return;
    }
    let Ok(bin) = std::env::current_exe() else { return };
    let running = crate::dsyms::running().and_then(|r| r.uuid);
    match crate::dsyms::keep(&bin) {
        Ok(Some(k)) => {
            if let Err(e) = &k.pruned {
                crate::log::line("tarae", "warn", &format!("older symbols not all pruned: {e}"));
            }
            let ours = k.at.file_stem().and_then(|s| s.to_str()).zip(running.as_deref());
            if ours.is_some_and(|(kept, id)| kept == id) {
                super::symbols_kept(k.at);
            }
        }
        Ok(None) => {}
        Err(e) => crate::log::line("tarae", "info", &format!("this build's symbols not kept: {e}")),
    }
}

#[cfg(test)]
pub(super) const UNSEEN_EXT: &str = UNSEEN;
