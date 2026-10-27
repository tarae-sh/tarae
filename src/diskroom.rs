//! The disk's room (#168): what's free on the file systems tarae and its runs write to, looked at on a clock of
//! its own (`every`: 30 s, every 5 s once one is under `CLOSE`), how fast it drains — the last two looks' falls,
//! each over at least `SPAN_MIN`, since bursts are what fill it (a relink of tarae's tests writes 1.5 GB in 15 s, a
//! cold build 2.6 GB in its peak minute), and a burst that stopped stops counting two looks on — and the level
//! that says it, from a share of the disk:
//!
//! - **low**: under 5 % (50 GB at most) — the status line's mark and a warning.
//! - **reserve**: under 2 % (20 GB at most) — what writes in bulk waits for room: Claude's runs (`queued.rs`, in
//!   line, never refused) and overlap rounds (`overlap.rs`); the user's own runs go on.
//! - **critical**: under 1 % (10 GB at most) — an error that waits for a key; the reserve's hold.
//!
//! **A time left warns, never more** (the user's decision holds at the reserve, a share of the disk): under 2 h at
//! that pace (15 min the worse) on a disk with room is one check, `Disk filling fast`, a warning that holds
//! nothing; on a disk already past a space bar it's only the mark's time (`▲ disk 39.5 GB · ~28m 08s`) — one level
//! said once while it lasts, never again with each burst. The space levels are kept apart from the time ones
//! (`Vol.space`, `Vol.time`). A level ends only once free is a quarter past where it starts (and its time left as
//! far past), so it doesn't come and go. #168 came at 116 MB free: the old marks (10 GB, 2 GB) were less than a
//! minute of builds there.
//! tarae removes nothing to make room: what fills the disk is the user's tools' (cargo's build folders, #168 —
//! `cargo xtask build-prune` prunes tarae's own), and tarae only knows how much is free.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::editor::Editor;
use crate::procs::{Act, Finding, Volume, mem};

const GB: u64 = 1 << 30;
/// The disk's looks: this often…
pub const EVERY: Duration = Duration::from_secs(30);
/// …and this often once a file system has under `CLOSE` free.
pub const EVERY_CLOSE: Duration = Duration::from_secs(5);
const CLOSE: u64 = 50 * GB;
/// The drain rate is the fall to the last look over a span of at most this…
const SPAN: Duration = Duration::from_secs(60);
/// …and at least this (two looks a moment apart say little: a process look and a disk look side by side).
const SPAN_MIN: Duration = Duration::from_secs(10);
/// A time left past this isn't said.
const SAID_UNDER: Duration = Duration::from_secs(24 * 3600);

/// How close a file system is to full, worst last.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    #[default]
    Fine,
    Low,
    Reserve,
    Critical,
}

impl Level {
    /// Its bar: under this share of the disk (%), this much at most, or this time left.
    fn bar(self) -> Option<(u64, u64, Option<Duration>)> {
        match self {
            Level::Fine => None,
            Level::Low => Some((5, 50 * GB, Some(Duration::from_secs(2 * 3600)))),
            Level::Reserve => Some((2, 20 * GB, None)),
            Level::Critical => Some((1, 10 * GB, Some(Duration::from_secs(15 * 60)))),
        }
    }

    fn below(self) -> Level {
        match self {
            Level::Critical => Level::Reserve,
            Level::Reserve => Level::Low,
            Level::Low | Level::Fine => Level::Fine,
        }
    }

    /// Its word in a check's key (`disk-low:/`).
    fn word(self) -> &'static str {
        match self {
            Level::Fine => "fine",
            Level::Low => "low",
            Level::Reserve => "reserve",
            Level::Critical => "critical",
        }
    }

    /// Whether `free` of `total` with `left` at the drain rate is under its bar — or, `margin`, not yet a quarter
    /// past it.
    fn under(self, free: u64, total: u64, left: Option<Duration>, margin: bool) -> bool {
        let Some((share, most, time)) = self.bar() else { return false };
        let space = (total / 100 * share).min(most);
        let (space, time) = match margin {
            true => (space + space / 4, time.map(|t| t + t / 4)),
            false => (space, time),
        };
        free < space || time.zip(left).is_some_and(|(t, l)| l < t)
    }
}

/// The level of `free` of `total` with `left` at the drain rate, having been `was`: the worst bar it's under, but
/// one it was at holds until it's a quarter past it.
pub fn level(free: u64, total: u64, left: Option<Duration>, was: Level) -> Level {
    let now = [Level::Critical, Level::Reserve, Level::Low]
        .into_iter()
        .find(|l| l.under(free, total, left, false))
        .unwrap_or(Level::Fine);
    let mut l = was.max(now);
    while l > now && !l.under(free, total, left, true) {
        l = l.below();
    }
    l
}

/// The drain rate (bytes a second): the last two looks' falls, the steeper — each from the newest look at least
/// `SPAN_MIN` before it, that one within `SPAN` — so a burst counts while it lasts and stops counting two looks
/// after it stops, while one quiet look between a build's writes doesn't end and say again what it brought. None
/// while there's no fall (`looks` oldest first).
pub fn rate(looks: &VecDeque<(Instant, u64)>) -> Option<f64> {
    let fall = |n: usize| -> Option<f64> {
        let &(tl, fl) = looks.iter().rev().nth(n)?;
        let &(ti, fi) =
            looks.iter().rev().skip(n + 1).find(|(t, _)| tl.saturating_duration_since(*t) >= SPAN_MIN)?;
        let dt = tl.saturating_duration_since(ti);
        (dt <= SPAN && fi > fl).then(|| (fi - fl) as f64 / dt.as_secs_f64())
    };
    match (fall(0), fall(1)) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// How often the disk is looked at, as it is now.
pub fn every(vols: &[Volume]) -> Duration {
    match vols.iter().any(|v| v.total > 0 && v.free < CLOSE) {
        true => EVERY_CLOSE,
        false => EVERY,
    }
}

/// One file system as the disk's looks know it.
#[derive(Clone, Debug, Default)]
struct Vol {
    looks: VecDeque<(Instant, u64)>,
    free: u64,
    total: u64,
    /// The level said: the worse of `space` and `time`.
    level: Level,
    /// The level by its share of the disk alone — what holds (`Room::held`).
    space: Level,
    /// The level by its time left alone (low, critical) — warns, holds nothing.
    time: Level,
}

impl Vol {
    fn left(&self) -> Option<Duration> {
        rate(&self.looks).map(|r| Duration::from_secs_f64((self.free as f64 / r).min(1e9)))
    }

    /// What holds bulk writes: the space reserve or worse.
    fn holds(&self) -> bool {
        self.space >= Level::Reserve
    }
}

/// The disk's looks and levels, per file system (by where it was looked at from).
#[derive(Debug, Default)]
pub struct Room {
    vols: HashMap<PathBuf, Vol>,
}

impl Room {
    /// A look: each file system's free space kept, its level anew.
    pub fn looked(&mut self, vols: &[Volume], now: Instant) {
        self.vols.retain(|at, _| vols.iter().any(|v| v.at == *at));
        for v in vols.iter().filter(|v| v.total > 0) {
            let k = self.vols.entry(v.at.clone()).or_default();
            k.looks.push_back((now, v.free));
            // (two spans' worth: `rate` reads the fall to the look before the last too)
            while k.looks.front().is_some_and(|(t, _)| now.saturating_duration_since(*t) > 2 * SPAN) {
                k.looks.pop_front();
            }
            (k.free, k.total) = (v.free, v.total);
            k.space = level(k.free, k.total, None, k.space);
            // (no space bar can be under `u64::MAX` free: the time bars alone)
            k.time = level(u64::MAX, k.total, k.left(), k.time);
            k.level = k.space.max(k.time);
        }
    }

    /// The worst level now.
    #[cfg(test)]
    pub fn worst(&self) -> Level {
        self.vols.values().map(|v| v.level).max().unwrap_or_default()
    }

    /// The worst level by space alone — what holds.
    #[cfg(test)]
    pub fn worst_space(&self) -> Level {
        self.vols.values().map(|v| v.space).max().unwrap_or_default()
    }

    /// The worst level by time left alone.
    #[cfg(test)]
    pub fn worst_time(&self) -> Level {
        self.vols.values().map(|v| v.time).max().unwrap_or_default()
    }

    /// What writes in bulk waits for, while a file system is at the space reserve or worse: `disk 18.0 GB free` —
    /// never a time left alone.
    pub fn held(&self) -> Option<String> {
        let v = self.vols.values().filter(|v| v.holds()).min_by_key(|v| v.free)?;
        Some(format!("disk {} free", mem(v.free)))
    }

    /// Each file system past a bar, as a check (`procs::Procs::record`): its key names the space level once there
    /// is one, so a new one is a new check, said once — a burst on a disk already low by space changes only the
    /// mark's time; a time left alone, on a disk with room, is one check whatever its level, `fast`, a warning.
    pub fn findings(&self) -> Vec<Finding> {
        let mut vols: Vec<(&PathBuf, &Vol)> =
            self.vols.iter().filter(|(_, v)| v.level > Level::Fine).collect();
        crate::sorting::by(&mut vols, |a, b| a.0.cmp(b.0));
        vols.into_iter()
            .map(|(at, v)| {
                let shown = crate::sandbox::home_shown(&at.display().to_string());
                let left = v.left().filter(|l| *l < SAID_UNDER);
                let pace = left.map(|l| format!(" — ~{} at this rate", crate::term::run_duration(l)));
                let (word, did) = match v.space {
                    Level::Fine => ("fast", "nothing — said"),
                    Level::Low => (v.space.word(), "nothing — said"),
                    s => (s.word(), "Claude's runs and overlap rounds wait for room"),
                };
                Finding {
                    key: format!("disk-{word}:{}", at.display()),
                    pid: 0,
                    what: format!(
                        "{} free of {} on {shown}{}",
                        mem(v.free),
                        mem(v.total),
                        pace.unwrap_or_default()
                    ),
                    short: match left {
                        Some(l) => format!("disk {} · ~{}", mem(v.free), crate::term::run_duration(l)),
                        None => format!("disk {} free", mem(v.free)),
                    },
                    did: did.into(),
                    act: Act::Nothing,
                }
            })
            .collect()
    }
}

/// A check's key is the disk's.
pub fn is_disk(key: &str) -> bool {
    key.starts_with("disk-")
}

impl Editor {
    /// The disk looked at (`procs::start`'s thread, on its own clock or with a look at the processes): its levels
    /// anew, a new one said once, what waited let go as the reserve ends. A screenshot's tarae (`shot`) says
    /// nothing of the host's disk.
    pub fn disk_looked(&mut self, vols: Vec<Volume>, shot: bool) {
        let held = self.disk.held().is_some();
        if !shot {
            self.disk.looked(&vols, crate::clock::now());
        }
        let found = self.disk.findings();
        for f in self.procs.record_where(&found, crate::clock::system(), is_disk) {
            match f.key.split(':').next() {
                Some("disk-low") => self.set_warning(format!("Disk low: {}", f.what)),
                // (a time left alone: a warning, holding nothing)
                Some("disk-fast") => self.set_warning(format!("Disk filling fast: {}", f.what)),
                Some("disk-reserve") => self.set_warning(format!(
                    "Disk at its reserve: {}. Claude's runs and overlap rounds wait for room",
                    f.what
                )),
                _ => self.set_error(format!(
                    "Disk almost full: {}. Claude's runs and overlap rounds wait",
                    f.what
                )),
            }
        }
        self.procs.free = vols;
        if held && self.disk.held().is_none() {
            self.disk_released();
        }
    }

    /// Room again: Claude's runs waiting in every workspace's line go on, and the overlap rounds.
    fn disk_released(&mut self) {
        let ids: Vec<_> =
            std::iter::once(self.workspace.id).chain(self.workspaces.iter().map(|w| w.id)).collect();
        for id in ids {
            self.with_workspace(id, crate::queued::run_next);
        }
        crate::overlap::resume(self);
        self.set_status("Disk: room again — Claude's runs and overlap rounds go on");
    }
}

#[cfg(test)]
mod tests;
