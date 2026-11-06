//! WSL2's Windows homes (Q11 of the sandbox checkout's `.sb/offline-bridge-plan.md`): WSL mounts each Windows drive
//! (drvfs, `/mnt/c`), and a Windows user's profile there — `C:\Users\<name>` at `/mnt/c/Users/<name>` — holds the
//! keys a Linux home does (`.ssh`, `.aws`, `.kube`, `.azure`, `.config/gcloud`: the tools keep them in
//! `%USERPROFILE%`) and Windows' own (its credential stores, the browsers' profiles) — outside the Linux `$HOME`, so
//! no rule keyed by home held them. Each profile counts as a home: what holds keys there is kept from every run
//! (`sandbox::kept_in`) and never an answer kept (`allow::secret`, `allow::keepable`); to Claude's reads yours is
//! home and the others are others' homes (`cards::class`), never a volume's. Found from the mount table and the
//! drives' `Users` folders only — never by running a Windows program (`wslpath` goes through `/init`,
//! `cmd.exe /c echo %USERPROFILE%` is Windows'). Windows' names are matched in any case, as its drives open them.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// What holds keys in a Windows profile besides `allow::SECRETS` (which it holds as a Linux home does), as srt's
/// Windows profile names them (its README's "Exclusions"): the tools' Windows places, Windows' own stores, the
/// browsers', mail's and password managers' profiles, the registry's hive.
pub const WINDOWS: &[&str] = &[
    "_netrc",
    ".vault-token",
    ".terraform.d",
    ".config/github-copilot",
    "NTUSER.DAT",
    "AppData/Roaming/gcloud",
    "AppData/Roaming/GitHub CLI",
    "AppData/Roaming/gnupg",
    "AppData/Roaming/terraform.d",
    "AppData/Roaming/postgresql/pgpass.conf",
    "AppData/Roaming/Microsoft/Credentials",
    "AppData/Roaming/Microsoft/Protect",
    "AppData/Roaming/Microsoft/Vault",
    "AppData/Roaming/Microsoft/Crypto",
    "AppData/Roaming/Microsoft/SystemCertificates",
    "AppData/Local/Microsoft/Credentials",
    "AppData/Local/Microsoft/Protect",
    "AppData/Local/Microsoft/Vault",
    "AppData/Local/Microsoft/Crypto",
    "AppData/Local/Microsoft/IdentityCache",
    "AppData/Local/Microsoft/TokenBroker",
    "AppData/Local/Microsoft/OneAuth",
    "AppData/Local/Microsoft/Windows/INetCookies",
    "AppData/Local/Microsoft/Windows/UsrClass.dat",
    "AppData/Local/.IdentityService",
    "AppData/Local/MicrosoftCredentialProvider",
    "AppData/Local/github-copilot",
    "AppData/Local/Google/Chrome/User Data",
    "AppData/Local/Microsoft/Edge/User Data",
    "AppData/Local/BraveSoftware/Brave-Browser/User Data",
    "AppData/Local/Chromium/User Data",
    "AppData/Local/Vivaldi/User Data",
    "AppData/Roaming/Opera Software",
    "AppData/Roaming/Mozilla/Firefox/Profiles",
    "AppData/Roaming/Thunderbird/Profiles",
    "AppData/Local/1Password",
    "AppData/Roaming/Bitwarden",
    "AppData/Roaming/KeePass",
    "AppData/Roaming/KeePassXC",
];

/// The folders in a drive's `Users` that are no one's profile: everyone's (`Public`, as `/Users/Shared`) and the
/// template new users are made from; `All Users` and `Default User` are links besides.
const NOT_HOMES: &[&str] = &["Public", "Default", "Default User", "All Users"];

/// A Windows drive as WSL mounts it: its letter and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    pub letter: char,
    pub at: PathBuf,
}

/// The Windows homes here — none but on WSL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Homes {
    pub drives: Vec<Drive>,
    /// Your own profile: `USERPROFILE` (WSLENV passes it), else the one named as the Linux user, else the one
    /// profile whose `AppData` this user can list (Windows' rights keep the others' closed) — None: not told apart,
    /// every profile another's.
    pub yours: Option<PathBuf>,
    /// Every profile folder on the drives (`<drive>/Users/<name>`, but `NOT_HOMES` and links), yours among them —
    /// `here`'s only: `drives` reads no folder.
    pub found: Vec<PathBuf>,
    /// What's kept from every run (`sandbox::kept_in`): each found profile's places that hold keys, spelled as they
    /// are on the drive (a run's rules are by path, Windows' names in any case) — the `allow::SECRETS` rows there or
    /// not, Windows' own (`WINDOWS`) where they were as tarae started (each one a folder every run's rules list on
    /// its way: none for what isn't there). `here`'s only.
    pub kept: Vec<PathBuf>,
}

impl Homes {
    /// The profile folder `p` is in, or is: `<drive>/Users/<name>` by its path alone (any name but `NOT_HOMES`'),
    /// found or not.
    pub fn profile_of(&self, p: &Path) -> Option<PathBuf> {
        self.drives.iter().find_map(|d| {
            let mut c = p.strip_prefix(&d.at).ok()?.components();
            let (users, name) = (c.next()?.as_os_str(), c.next()?.as_os_str());
            let home = users.eq_ignore_ascii_case("Users")
                && !NOT_HOMES.iter().any(|n| name.eq_ignore_ascii_case(n));
            home.then(|| d.at.join(users).join(name))
        })
    }

    /// Whether `p` is in a place of a Windows profile that holds keys (`allow::SECRETS`, `WINDOWS`), in any case.
    pub fn secret(&self, p: &Path) -> bool {
        let Some(home) = self.profile_of(p) else { return false };
        let rest: Vec<&OsStr> = p.strip_prefix(&home).map_or_else(|_| Vec::new(), |r| names(r));
        rows().any(|row| {
            let row: Vec<&OsStr> = names(Path::new(row));
            rest.len() >= row.len() && rest.iter().zip(&row).all(|(a, b)| a.eq_ignore_ascii_case(b))
        })
    }

    /// Whether `p` is a Windows profile or holds one (`/`, `/mnt`, `/mnt/c`, `/mnt/c/Users`) — by the drives alone.
    pub fn holds_a_home(&self, p: &Path) -> bool {
        let holds = self.drives.iter().any(|d| {
            d.at.starts_with(p)
                || p.strip_prefix(&d.at).is_ok_and(|r| {
                    let r = names(r);
                    r.len() == 1 && r[0].eq_ignore_ascii_case("Users")
                })
        });
        holds || self.profile_of(p).as_deref() == Some(p)
    }

    /// Whether `p` is your profile, in it, or holds it (`/mnt/c`).
    pub fn yours_holds_or_in(&self, p: &Path) -> bool {
        self.yours.as_deref().is_some_and(|y| p.starts_with(y) || y.starts_with(p))
    }

    /// `profile` (`USERPROFILE`: a Windows path, `C:\Users\x`, or one WSLENV turned into a Linux one) as a profile
    /// folder here — the found one in its case — if it is one.
    fn profile_at(&self, profile: &str) -> Option<PathBuf> {
        let p = match profile.as_bytes() {
            [b'/', ..] => PathBuf::from(profile),
            [l, b':', ..] => {
                let d = self.drives.iter().find(|d| d.letter.eq_ignore_ascii_case(&char::from(*l)))?;
                d.at.join(profile[2..].trim_start_matches('\\').replace('\\', "/"))
            }
            _ => return None,
        };
        let p = PathBuf::from(p.to_string_lossy().trim_end_matches('/'));
        (self.profile_of(&p).as_deref() == Some(&p)).then(|| {
            let same = |f: &&PathBuf| f.as_os_str().eq_ignore_ascii_case(p.as_os_str());
            self.found.iter().find(same).cloned().unwrap_or(p)
        })
    }
}

/// A relative path's names (`.` and the like left out).
fn names(p: &Path) -> Vec<&OsStr> {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n),
            _ => None,
        })
        .collect()
}

/// The places that hold keys in a profile: a Linux home's (`allow::SECRETS`) and Windows' own.
fn rows() -> impl Iterator<Item = &'static str> {
    crate::allow::SECRETS.iter().chain(WINDOWS).copied()
}

/// Profile `home`'s places that hold keys as `Homes.kept` has them: each row found on the drive in any case and
/// spelled as it is there, else an `allow::SECRETS` row as written. Reads the folders on the way, each once.
fn kept_of(home: &Path) -> Vec<PathBuf> {
    let mut listed: std::collections::HashMap<PathBuf, Vec<std::ffi::OsString>> = Default::default();
    let mut kept = Vec::new();
    for row in rows() {
        let mut at = Some(home.to_path_buf());
        for name in names(Path::new(row)) {
            let Some(dir) = at.take() else { break };
            let there = listed.entry(dir.clone()).or_insert_with(|| {
                let list = std::fs::read_dir(&dir).into_iter().flatten().flatten();
                list.map(|e| e.file_name()).collect()
            });
            at = there.iter().find(|n| n.eq_ignore_ascii_case(name)).map(|n| dir.join(n));
        }
        match at {
            Some(p) => kept.push(p),
            None if crate::allow::SECRETS.contains(&row) => kept.push(home.join(row)),
            None => {}
        }
    }
    kept
}

/// The Windows drives in mount table `mounts` (`/proc/self/mounts`' text): drvfs — by its type, its 9p `aname`, or
/// a source that's a drive (`C:\`, as a virtiofs drive shows it).
pub fn drives_in(mounts: &str) -> Vec<Drive> {
    let mut v: Vec<Drive> = mounts
        .lines()
        .filter_map(|l| {
            let mut f = l.split(' ');
            let (source, at, kind, options) =
                (unescape(f.next()?), unescape(f.next()?), f.next()?, f.next()?);
            let path = options.split([';', ',']).find_map(|o| o.strip_prefix("path=")).map(unescape);
            let drive = |s: &str| match s.as_bytes() {
                [l, b':', ..] if l.is_ascii_alphabetic() => Some(char::from(*l).to_ascii_lowercase()),
                _ => None,
            };
            let drvfs = kind == "drvfs" || options.split([';', ',']).any(|o| o == "aname=drvfs");
            let letter = match drvfs {
                true => path.as_deref().and_then(drive).or_else(|| drive(&source)),
                false => matches!(kind, "9p" | "virtiofs").then(|| drive(&source)).flatten(),
            };
            Some(Drive { letter: letter?, at: PathBuf::from(at) })
        })
        .collect();
    v.dedup();
    v
}

/// A mount table's field as written: `\NNN` its octal escapes (a space `\040`, a backslash `\134`).
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let octal =
            b.get(i + 1..i + 4).filter(|o| b[i] == b'\\' && o.iter().all(|c| (b'0'..=b'7').contains(c)));
        match octal {
            Some(o) => {
                out.push(o.iter().fold(0u8, |n, c| n.wrapping_mul(8).wrapping_add(c - b'0')));
                i += 4;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The profiles on `drives`, and which is yours (`Homes.yours`'s rule: `profile`, `USERPROFILE`; `user`, the Linux
/// user's name). Reads the drives' `Users` folders: a worker's.
pub fn find(drives: Vec<Drive>, profile: Option<&str>, user: Option<&str>) -> Homes {
    let mut found: Vec<PathBuf> = Vec::new();
    for d in &drives {
        let Ok(list) = std::fs::read_dir(d.at.join("Users")) else { continue };
        let mut here: Vec<PathBuf> =
            list.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect();
        crate::sorting::sort(&mut here);
        found.extend(here);
    }
    let mut h = Homes { drives, ..Homes::default() };
    found.retain(|f| h.profile_of(f).as_deref() == Some(f.as_path()));
    h.found = found;
    h.kept = h.found.iter().flat_map(|f| kept_of(f)).collect();
    let named = |f: &&PathBuf| user.is_some_and(|u| f.file_name().is_some_and(|n| n.eq_ignore_ascii_case(u)));
    let readable: Vec<&PathBuf> =
        h.found.iter().filter(|f| std::fs::read_dir(f.join("AppData")).is_ok()).collect();
    h.yours = profile
        .and_then(|p| h.profile_at(p))
        .or_else(|| h.found.iter().find(named).cloned())
        .or_else(|| (readable.len() == 1).then(|| readable[0].clone()));
    h
}

/// The Windows drives here, from the mount table (`/proc`'s, no folder read: any thread's) — `Homes.drives` alone.
#[cfg(not(test))]
pub fn drives() -> Homes {
    static DRIVES: std::sync::OnceLock<Vec<Drive>> = std::sync::OnceLock::new();
    let d =
        DRIVES.get_or_init(|| drives_in(&std::fs::read_to_string("/proc/self/mounts").unwrap_or_default()));
    Homes { drives: d.clone(), ..Homes::default() }
}

/// The Windows homes here (`find`, once): what a policy and a session's `Caps` are made with (`sandbox::Here`). It
/// reads the drives' `Users` folders the first time: a worker's — `caps::Basis::build` asks it first, at a
/// session's start.
#[cfg(not(test))]
pub fn here() -> Homes {
    static HOMES: std::sync::OnceLock<Homes> = std::sync::OnceLock::new();
    let found = HOMES.get_or_init(|| {
        let var = |n: &str| std::env::var(n).ok().filter(|v| !v.is_empty());
        let user = var("USER").or_else(|| var("LOGNAME"));
        find(drives().drives, var("USERPROFILE").as_deref(), user.as_deref())
    });
    found.clone()
}

/// Tests: the stand-in's drives (`stand_in`), none unless a test gives some.
#[cfg(test)]
pub fn drives() -> Homes {
    Homes { drives: here().drives, ..Homes::default() }
}

/// Tests: the stand-in (`stand_in`), none unless a test gives one.
#[cfg(test)]
pub fn here() -> Homes {
    STAND_IN.with(|s| s.borrow().clone()).unwrap_or_default()
}

#[cfg(test)]
thread_local! {
    /// Tests: the Windows homes this thread's code sees (`stand_in`) — none unless a test gives some.
    static STAND_IN: std::cell::RefCell<Option<Homes>> = const { std::cell::RefCell::new(None) };
}

/// Tests: `homes` as this thread's Windows homes until the guard goes.
#[cfg(test)]
pub fn stand_in(homes: Homes) -> impl Drop {
    struct Back;
    impl Drop for Back {
        fn drop(&mut self) {
            STAND_IN.with(|s| s.borrow_mut().take());
        }
    }
    STAND_IN.with(|s| *s.borrow_mut() = Some(homes));
    Back
}

#[cfg(test)]
mod tests;
