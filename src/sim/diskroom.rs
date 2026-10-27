//! The disk's looks, simulated (#168): free space at random — a burst, a slow fall, room coming back — looked at
//! 5 s to 2 min apart on the simulated clock, an overlap round asked between. After every step: the hold is the
//! reserve's (held exactly while a file system's space is at it or worse — never by a time left alone), no round
//! starts while it holds, the status line's mark is the worst level's, the check follows the space level whenever
//! there is one (a time left alone is `filling fast`, and only on a disk with room), and each check (low, reserve,
//! critical, filling fast) was said once each time its level was reached — a time left coming and going under a
//! space level is nothing new, and the only errors are critical's by space. At the end, room:
//! nothing held, nothing marked, a round armed again.

use std::path::PathBuf;
use std::time::Duration;

use super::{Choices, Sim};
use crate::diskroom::Level;
use crate::editor::{Editor, ToastKind};
use crate::procs::Volume;

const SEEDS: &[u64] = &[0, 1, 2, 3, 4, 5];
const INPUTS: usize = 24;
const LIMIT: Duration = crate::test_results::PATIENCE;
const GB: u64 = 1 << 30;

#[test]
fn the_reserve_holds_exactly_while_the_disk_is_at_it() {
    super::seeds("the_reserve_holds_exactly_while_the_disk_is_at_it", SEEDS, run);
}

/// How many times each check was reached (low, reserve, critical, filling fast) — and said.
#[derive(Default)]
struct Count {
    reached: [usize; 4],
}

/// What the one file system's check says now (`disk-low`, … `disk-fast`), as an index into `Count.reached`.
fn word(ed: &Editor) -> Option<usize> {
    let key = ed.disk.findings().first().map(|f| f.key.clone())?;
    ["disk-low:", "disk-reserve:", "disk-critical:", "disk-fast:"].iter().position(|w| key.starts_with(w))
}

/// What it should say, from the levels alone: the space level's whenever there is one, else `fast` for a time
/// left alone — never a flip with each burst.
fn expected(ed: &Editor) -> Option<usize> {
    match (ed.disk.worst_space(), ed.disk.worst_time()) {
        (Level::Fine, Level::Fine) => None,
        (Level::Fine, _) => Some(3),
        (space, _) => Some(space as usize - 1),
    }
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("the_reserve_holds_exactly_while_the_disk_is_at_it", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    let mut free = 300 * GB;
    let count = std::cell::RefCell::new(Count::default());
    // (the rounds started as the last step left it, and whether the disk held then)
    let last = std::cell::Cell::new((0, false));
    let check = |ed: &Editor| -> Result<(), String> {
        // (only space holds: a level by time left alone warns)
        let held = ed.disk.held().is_some();
        if held != (ed.disk.worst_space() >= Level::Reserve) {
            return Err(format!("held {held} at {:?} by space", ed.disk.worst_space()));
        }
        let (rounds, was) = last.replace((crate::overlap::rounds(ed), held));
        if held && was && crate::overlap::rounds(ed) > rounds {
            return Err("an overlap round started while the disk held".into());
        }
        if (ed.disk.worst() > Level::Fine) != ed.procs.mark().is_some() {
            return Err(format!("mark {:?} at {:?}", ed.procs.mark().map(|c| &c.short), ed.disk.worst()));
        }
        if word(ed) != expected(ed) {
            return Err(format!("says {:?}, its levels {:?}", word(ed), expected(ed)));
        }
        let said = |kind: ToastKind, text: &str| {
            ed.messages.iter().filter(|m| m.1 == kind && m.2.contains(text)).count()
        };
        let c = count.borrow();
        let got = [
            said(ToastKind::Warning, "Disk low:"),
            said(ToastKind::Warning, "wait for room"),
            said(ToastKind::Error, "Disk almost full:"),
            said(ToastKind::Warning, "Disk filling fast:"),
        ];
        if said(ToastKind::Error, "") != c.reached[2] {
            return Err(format!(
                "{} errors, critical by space reached {}",
                said(ToastKind::Error, ""),
                c.reached[2]
            ));
        }
        match got == c.reached {
            true => Ok(()),
            false => Err(format!("said {got:?}, reached {:?}", c.reached)),
        }
    };
    let mut input = |sim: &mut Sim, ed: &mut Editor| -> String {
        let gap = *sim.choices.pick(&[5, 30, 60, 120]);
        sim.wait(Duration::from_secs(gap));
        // (by arm: a burst 1, a slow fall 2, room coming back 1, an overlap round asked 1)
        let what = match sim.choices.weighted(&[1, 2, 1, 1]) {
            0 => {
                free = free.saturating_sub(*sim.choices.pick(&[20, 60, 120]) * GB);
                "burst"
            }
            1 => {
                free = free.saturating_sub(*sim.choices.pick(&[1, 5]) * GB);
                "fall"
            }
            2 => {
                free = (free + *sim.choices.pick(&[10, 100]) * GB).min(900 * GB);
                "room"
            }
            _ => {
                crate::overlap::ask(ed);
                return "round asked".into();
            }
        };
        let before = expected(ed);
        ed.disk_looked(vec![Volume { at: PathBuf::from("/Volumes/x"), free, total: 926 * GB }], false);
        let after = expected(ed);
        // (each said as its level is reached — from below it, or from a worse one stepping down; a time left coming
        // and going under a space level is nothing new)
        if let Some(w) = after.filter(|_| after != before) {
            count.borrow_mut().reached[w] += 1;
        }
        format!("{what} → {} GB free", free / GB)
    };
    sim.walk(&mut ed, INPUTS, &mut input, check, print);
    sim.wait(Duration::from_secs(16 * 60));
    ed.disk_looked(vec![Volume { at: PathBuf::from("/Volumes/x"), free: 500 * GB, total: 926 * GB }], false);
    let armed = crate::overlap::waiting(&ed) || {
        crate::overlap::ask(&mut ed);
        crate::overlap::waiting(&ed)
    };
    sim.at_end(|| {
        if ed.disk.held().is_some() || ed.procs.mark().is_some() {
            return Err(format!("room, yet {:?} / {:?}", ed.disk.held(), ed.procs.mark().map(|c| &c.short)));
        }
        match armed {
            true => Ok(()),
            false => Err("with room again a round isn't armed".into()),
        }
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn print(ed: &Editor) -> String {
    format!(
        "{:?} · held {:?} · round waiting {}",
        ed.disk.worst(),
        ed.disk.held(),
        crate::overlap::waiting(ed)
    )
}
