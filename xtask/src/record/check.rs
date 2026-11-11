//! `cargo xtask record check DIR` — every place a take's screens show something of yours, before anything is cut from
//! it (the Shorts' rule, the full video's plan: "nothing of the user's on screen"). The screens are read as they were
//! drawn — their cells' text, row by row (`super::screens`), never the raw bytes, which a frame redraws in pieces — for:
//!
//! - `name` — your login name or your full name (the account's, and git's `user.name`), as a whole word — but a login
//!   shorter than `SHORT_NAME` letters (two letters match `:wq`'s alias in the `:` list): yours only where it can't be
//!   anything else, after `/Users/`, `/home/` or `~` or before `@`; as a bare word it's listed apart as weak, counted,
//!   never failing;
//! - `home` — your home's path;
//! - `mail` — any mail address (yours, git's `user.email`, said as yours);
//! - `money` — an amount: a currency sign before a number (`$` with cents or thousands, so a shell's `$1` isn't one),
//!   or a number before a currency's name or code.
//!
//! What's yours is read from this machine when it runs — never written into the repository or a test. Each place is
//! listed once for as long as it stands: its time, its row, what it found and the row's text; `DIR/check.txt` keeps
//! the list. It fails where a place isn't weak.

use std::path::Path;

use super::{Replay, Take};
use crate::Result;

/// What's yours, to look for: names (login, full), homes, mail addresses.
#[derive(Debug, Default, Clone)]
pub(crate) struct Yours {
    pub(crate) names: Vec<String>,
    pub(crate) homes: Vec<String>,
    pub(crate) mails: Vec<String>,
}

impl Yours {
    /// This machine's: the account's login and full name (`getpwuid`, `USER`), `HOME`, and git's `user.name` and
    /// `user.email` (the global settings' — a checkout's are the same person).
    pub(crate) fn here() -> Yours {
        let mut y = Yours::default();
        // SAFETY: getpwuid's record read at once, its strings copied before any other call could reuse it.
        unsafe {
            let pw = libc::getpwuid(libc::getuid());
            if !pw.is_null() {
                let s = |p: *const libc::c_char| {
                    (!p.is_null()).then(|| std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned())
                };
                y.names.extend(s((*pw).pw_name));
                // (the full name: GECOS's first field)
                y.names
                    .extend(s((*pw).pw_gecos).map(|g| g.split(',').next().unwrap_or_default().to_string()));
                y.homes.extend(s((*pw).pw_dir));
            }
        }
        y.names.extend(std::env::var("USER").ok());
        y.homes.extend(std::env::var("HOME").ok());
        let git = |key: &str| {
            let out =
                std::process::Command::new("git").args(["config", "--global", "--get", key]).output().ok()?;
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
        };
        y.names.extend(git("user.name"));
        y.mails.extend(git("user.email"));
        y.tidy();
        y
    }

    /// Each one once, the empty and the too short to tell (one letter, `/`) left out.
    pub(crate) fn tidy(&mut self) {
        for list in [&mut self.names, &mut self.homes, &mut self.mails] {
            list.retain(|s| s.trim().chars().count() > 1 && s.trim() != "/");
            for s in list.iter_mut() {
                *s = s.trim().trim_end_matches('/').to_string();
            }
            list.sort();
            list.dedup();
        }
    }
}

/// A login shorter than this is your name only where nothing else could be (`unmistakable`).
pub(crate) const SHORT_NAME: usize = 4;

/// A place a take shows something: from when to when it stood (ms), its row, its kind, what it found, the row's text,
/// and whether it's weak (a short login as a bare word: counted, not failing).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Hit {
    pub(crate) from: u64,
    pub(crate) to: u64,
    pub(crate) row: u16,
    pub(crate) kind: &'static str,
    pub(crate) found: String,
    pub(crate) text: String,
    pub(crate) weak: bool,
}

pub(super) fn run(args: &[String]) -> Result<()> {
    let [dir] = args else { return Err("usage: cargo xtask record check DIR".into()) };
    let dir = Path::new(dir);
    let hits = scan(dir, &Yours::here())?;
    let list = listed(&hits);
    let p = dir.join("check.txt");
    std::fs::write(&p, &list).map_err(|e| format!("{}: {e}", p.display()))?;
    print!("{list}");
    match hits.iter().filter(|h| !h.weak).count() {
        0 => Ok(()),
        n => Err(format!("{n} places show something of yours ({})", p.display())),
    }
}

/// The places listed, the weak ones apart after the rest, each kind counted.
pub(crate) fn listed(hits: &[Hit]) -> String {
    let line = |h: &Hit| {
        format!(
            "{:>8.2}s–{:<8} row {:>2}  {:<5}  {}  │ {}\n",
            h.from as f64 / 1000.0,
            format!("{:.2}s", h.to as f64 / 1000.0),
            h.row,
            h.kind,
            h.found,
            h.text.trim_end()
        )
    };
    let (weak, strong): (Vec<&Hit>, Vec<&Hit>) = hits.iter().partition(|h| h.weak);
    let mut list: String = strong.iter().map(|h| line(h)).collect();
    list += &format!("{} places\n", strong.len());
    if !weak.is_empty() {
        list += &format!("\nweak — a login under {SHORT_NAME} letters as a bare word, not failing:\n");
        list += &weak.iter().map(|h| line(h)).collect::<String>();
        list += &format!("{} weak\n", weak.len());
    }
    list
}

/// `listed`'s text read back: each place, the weak ones (listed after their heading) weak — its times to the 10 ms it
/// wrote, the row's text without its trailing blanks. A line that isn't a place's (a count, a heading) is passed over.
pub(crate) fn parse(text: &str) -> Result<Vec<Hit>> {
    let mut out = Vec::new();
    let mut weak = false;
    for (n, line) in text.lines().enumerate() {
        if line.starts_with("weak — ") {
            weak = true;
            continue;
        }
        let Some((head, row_text)) = line.split_once("  │ ") else { continue };
        let bad = || format!("check.txt:{}: not a place: {line}", n + 1);
        let (times, rest) = head.trim_start().split_once(' ').ok_or_else(bad)?;
        let (from, to) = times.split_once("s–").ok_or_else(bad)?;
        let ms = |s: &str| s.trim_end_matches('s').parse::<f64>().map(|x| (x * 1000.0).round() as u64);
        let (from, to) = (ms(from).map_err(|_| bad())?, ms(to).map_err(|_| bad())?);
        let rest = rest.trim_start().strip_prefix("row ").ok_or_else(bad)?.trim_start();
        let (row, rest) = rest.split_once(' ').ok_or_else(bad)?;
        let row: u16 = row.parse().map_err(|_| bad())?;
        let (kind, found) = rest.trim_start().split_once(' ').ok_or_else(bad)?;
        let kind = ["name", "home", "mail", "money"].into_iter().find(|k| *k == kind).ok_or_else(bad)?;
        out.push(Hit {
            from,
            to,
            row,
            kind,
            found: found.trim().to_string(),
            text: row_text.to_string(),
            weak,
        });
    }
    Ok(out)
}

/// The places the take in `dir` shows something of `yours` (`hits` over its screens, one held at a time: an hour and a
/// half's every screen at once would be gigabytes).
pub(crate) fn scan(dir: &Path, yours: &Yours) -> Result<Vec<Hit>> {
    let take = Take::read(dir)?;
    let mut replay = Replay::new(&take.screen, take.size);
    let (mut h, mut last) = (Hits::default(), 0);
    while let Some(t) = replay.next() {
        h.screen(t, replay.screen(), yours);
        last = t;
    }
    Ok(h.end(take.ms().max(last)))
}

/// Each place `frames` (each screen with its time, the take ending at `end`) shows something of `yours`, once for as
/// long as it stands on its row, in the order they came (the tests' made-up screens; a take's are `scan`'s).
#[cfg(test)]
pub(crate) fn hits(frames: &[(u64, vt100::Screen)], end: u64, yours: &Yours) -> Vec<Hit> {
    let mut h = Hits::default();
    for (t, screen) in frames {
        h.screen(*t, screen, yours);
    }
    h.end(end)
}

/// `hits` as the screens come: those gone, and those still standing (each till the screen last seen).
#[derive(Default)]
struct Hits {
    done: Vec<Hit>,
    open: Vec<Hit>,
}

impl Hits {
    /// The screen at `t`: what stood before stood till now; what it shows that stood is longer, the rest begins.
    fn screen(&mut self, t: u64, screen: &vt100::Screen, yours: &Yours) {
        self.open.iter_mut().for_each(|o| o.to = t);
        let mut now = Vec::new();
        for (row, text) in rows(screen).into_iter().enumerate() {
            for (kind, found, weak) in found(&text, yours) {
                now.push(Hit { from: t, to: t, row: row as u16, kind, found, text: text.clone(), weak });
            }
        }
        let same =
            |a: &Hit, b: &Hit| a.row == b.row && a.kind == b.kind && a.found == b.found && a.weak == b.weak;
        // (one still standing: longer; one gone: done)
        let (stay, gone): (Vec<Hit>, Vec<Hit>) =
            self.open.drain(..).partition(|o| now.iter().any(|n| same(o, n)));
        self.done.extend(gone);
        for n in now {
            match stay.iter().position(|o| same(o, &n)) {
                Some(k) => self.open.push(Hit { from: stay[k].from, ..n }),
                None => self.open.push(n),
            }
        }
    }

    /// Every place, those still standing till the take's `end`, in the order they came.
    fn end(mut self, end: u64) -> Vec<Hit> {
        self.open.iter_mut().for_each(|o| o.to = end);
        self.done.extend(self.open);
        self.done.sort_by_key(|h| (h.from, h.row));
        self.done
    }
}

/// A screen's rows as text: each cell's, a wide character's second cell left out, an empty cell a space.
fn rows(screen: &vt100::Screen) -> Vec<String> {
    let (rows, cols) = screen.size();
    (0..rows)
        .map(|r| {
            (0..cols)
                .filter_map(|c| screen.cell(r, c))
                .filter(|c| !c.is_wide_continuation())
                .map(|c| if c.contents().is_empty() { " ".to_string() } else { c.contents().to_string() })
                .collect()
        })
        .collect()
}

/// What of `yours` one row's `text` shows: each kind, what it found, and whether it's weak (a login under
/// `SHORT_NAME` letters as a bare word).
pub(crate) fn found(text: &str, yours: &Yours) -> Vec<(&'static str, String, bool)> {
    let mut out = Vec::new();
    let lower = text.to_lowercase();
    for name in &yours.names {
        let n = name.to_lowercase();
        let bare = words(&lower).any(|w| w == n) || (name.contains(' ') && lower.contains(&n));
        let short = name.chars().count() < SHORT_NAME && !name.contains(' ');
        match (short, unmistakable(&lower, &n)) {
            (true, true) => out.push(("name", name.clone(), false)),
            (true, false) if bare => out.push(("name", name.clone(), true)),
            (false, _) if bare => out.push(("name", name.clone(), false)),
            _ => {}
        }
    }
    for home in &yours.homes {
        if text.contains(home.as_str()) {
            out.push(("home", home.clone(), false));
        }
    }
    for m in mails(text) {
        let yours = yours.mails.iter().any(|y| y.eq_ignore_ascii_case(&m));
        out.push(("mail", if yours { format!("{m} (yours)") } else { m }, false));
    }
    out.extend(money(text).into_iter().map(|m| ("money", m, false)));
    out
}

/// Whether `lower` shows the login `name` (lowercased) where it can only be a person's: a home folder's name (after
/// `/users/`, `/home/` or `~`) or a mail's user (before `@`), as the whole of it.
pub(crate) fn unmistakable(lower: &str, name: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
    let ends = |at: usize| !lower[at..].starts_with(word);
    let homed = ["/users/", "/home/", "~"].iter().any(|p| {
        let whole = format!("{p}{name}");
        lower.match_indices(&whole).any(|(i, _)| ends(i + whole.len()))
    });
    let mailed = (lower.match_indices(&format!("{name}@"))).any(|(i, _)| {
        !lower[..i].ends_with(word) && lower[i + name.len() + 1..].starts_with(char::is_alphanumeric)
    });
    homed || mailed
}

/// A row's words: its runs of letters, digits, `_`, `-` and `.` (a login like `jo.k`), dots at their ends left off.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.')))
        .map(|w| w.trim_matches('.'))
        .filter(|w| !w.is_empty())
}

/// The mail addresses in `text`: a local part, `@`, and a domain with a dot and a name of two letters or more after it.
pub(crate) fn mails(text: &str) -> Vec<String> {
    let local = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-');
    let domain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-');
    let mut out = Vec::new();
    for (at, _) in text.match_indices('@') {
        let start = text[..at].rfind(|c: char| !local(c)).map_or(0, |i| i + 1);
        let end = text[at + 1..].find(|c: char| !domain(c)).map_or(text.len(), |i| at + 1 + i);
        let (user, host) = (&text[start..at], text[at + 1..end].trim_end_matches(['.', '-']));
        let tld = host.rsplit_once('.').map(|(_, t)| t).unwrap_or_default();
        if !user.trim_matches('.').is_empty()
            && tld.len() >= 2
            && tld.chars().all(|c| c.is_ascii_alphabetic())
        {
            out.push(format!("{user}@{host}"));
        }
    }
    out
}

/// The signs before an amount, and the names and codes after one.
const SIGNS: [char; 5] = ['$', '€', '£', '₩', '¥'];
const AFTER: [&str; 12] =
    ["usd", "krw", "eur", "gbp", "jpy", "won", "원", "dollars", "dollar", "euros", "cents", "달러"];

/// The amounts of money in `text`: a sign, maybe a space, a number (`$` only with cents or thousands — `$0.31`,
/// `$1,200` — so a shell's `$1` isn't one); or a number, maybe a space, a currency's name or code.
pub(crate) fn money(text: &str) -> Vec<String> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let number_at = |k: usize| -> usize {
        // (the end of a number from char `k`: digits, with `,` and `.` between digits)
        let mut j = k;
        while j < chars.len()
            && (chars[j].1.is_ascii_digit()
                || (matches!(chars[j].1, ',' | '.')
                    && j > k
                    && chars.get(j + 1).is_some_and(|c| c.1.is_ascii_digit())))
        {
            j += 1;
        }
        j
    };
    let byte = |k: usize| chars.get(k).map_or(text.len(), |c| c.0);
    let mut out = Vec::new();
    let mut k = 0;
    while k < chars.len() {
        let c = chars[k].1;
        if SIGNS.contains(&c) {
            let n0 = k + 1 + usize::from(chars.get(k + 1).is_some_and(|c| c.1 == ' '));
            let n1 = number_at(n0);
            let num = &text[byte(n0)..byte(n1)];
            let real = n1 > n0
                && (c != '$' || num.contains(',') || num.split_once('.').is_some_and(|(_, d)| d.len() == 2));
            if real {
                out.push(text[byte(k)..byte(n1)].to_string());
                k = n1;
                continue;
            }
        }
        // (a number that starts here, not inside a word)
        if c.is_ascii_digit() && (k == 0 || !chars[k - 1].1.is_alphanumeric()) {
            let n1 = number_at(k);
            let rest = text[byte(n1)..].trim_start_matches(' ').to_lowercase();
            let unit = AFTER.iter().find(|u| {
                rest.starts_with(*u) && !rest[u.len()..].chars().next().is_some_and(|c| c.is_alphanumeric())
            });
            if let Some(u) = unit {
                let gap = text[byte(n1)..].len() - text[byte(n1)..].trim_start_matches(' ').len();
                // (the unit's bytes as written: its lowercase is as long — ASCII, or Hangul with no case)
                out.push(text[byte(k)..byte(n1) + gap + u.len()].to_string());
                k = n1;
                continue;
            }
            k = n1.max(k + 1);
            continue;
        }
        k += 1;
    }
    out
}
