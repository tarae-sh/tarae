use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::*;
use crate::editor::ToastKind;

const MIN: Duration = Duration::from_secs(60);

/// Each bar on this Mac's disk (926 GB: 5 % is 46.3 GB, 2 % 18.5, 1 % 9.3) and on a small one (100 GB), from
/// space alone; then from the time left — 2 h, 15 min — on a disk with room.
#[test]
fn each_level_is_a_share_of_the_disk_or_a_time_left() {
    let mac = 926 * GB;
    let at = |free| level(free, mac, None, Level::Fine);
    assert_eq!(
        [at(60 * GB), at(45 * GB), at(20 * GB), at(18 * GB), at(10 * GB), at(9 * GB)],
        [Level::Fine, Level::Low, Level::Low, Level::Reserve, Level::Reserve, Level::Critical]
    );
    let small = |free| level(free, 100 * GB, None, Level::Fine);
    assert_eq!(
        [small(6 * GB), small(4 * GB), small(GB * 5 / 2), small(GB + GB / 2), small(GB / 2)],
        [Level::Fine, Level::Low, Level::Low, Level::Reserve, Level::Critical]
    );
    let big = 4096 * GB;
    assert_eq!(level(60 * GB, big, None, Level::Fine), Level::Fine, "50 GB at most, however big the disk");
    let left = |m: u32| level(500 * GB, mac, Some(MIN * m), Level::Fine);
    assert_eq!([left(180), left(90), left(10)], [Level::Fine, Level::Low, Level::Critical]);
}

/// A level holds until it's a quarter past its bar — then it steps down as far as it's clear, no further.
#[test]
fn a_level_ends_a_quarter_past_its_bar() {
    let mac = 926 * GB;
    assert_eq!(level(50 * GB, mac, None, Level::Low), Level::Low, "past 46.3, not past 57.9");
    assert_eq!(level(58 * GB, mac, None, Level::Low), Level::Fine);
    assert_eq!(level(15 * GB, mac, None, Level::Critical), Level::Reserve, "clear of 11.6, not of 23.1");
    assert_eq!(level(30 * GB, mac, None, Level::Critical), Level::Low);
    assert_eq!(
        level(500 * GB, mac, Some(MIN * 17), Level::Critical),
        Level::Critical,
        "15 min, not yet 18:45"
    );
    assert_eq!(level(500 * GB, mac, Some(MIN * 20), Level::Critical), Level::Low, "under 2 h still");
    assert_eq!(level(500 * GB, mac, Some(MIN * 160), Level::Low), Level::Fine);
}

/// The drain rate is the last two looks' falls, each over at least 10 s: a burst while it lasts, nothing two looks
/// after it stopped — two looks a moment apart say nothing, nor does room coming back.
#[test]
fn the_drain_rate_is_the_last_looks_fall() {
    let t0 = Instant::now();
    let looks = |l: &[(u64, u64)]| -> VecDeque<(Instant, u64)> {
        l.iter().map(|&(s, gb)| (t0 + Duration::from_secs(s), gb * GB)).collect()
    };
    let r = rate(&looks(&[(0, 100), (30, 99), (60, 90)])).unwrap();
    assert!((r - 9.0 * GB as f64 / 30.0).abs() < 1.0, "99 → 90 in 30 s: {r}");
    let r = rate(&looks(&[(0, 100), (30, 90), (60, 90)])).unwrap();
    assert!((r - 10.0 * GB as f64 / 30.0).abs() < 1.0, "one quiet look: the fall before it still counts");
    assert_eq!(rate(&looks(&[(0, 100), (30, 90), (60, 90), (90, 90)])), None, "two: the burst stopped");
    let r = rate(&looks(&[(0, 100), (15, 90), (20, 85)])).unwrap();
    assert!((r - 15.0 * GB as f64 / 20.0).abs() < 1.0, "from the newest look 10 s back: {r}");
    assert_eq!(rate(&looks(&[(0, 100), (5, 90)])), None, "5 s apart");
    assert_eq!(rate(&looks(&[(0, 90), (30, 100)])), None, "room came back");
    assert_eq!(rate(&looks(&[(0, 100), (120, 90)])), None, "two minutes apart: no span of a minute");
}

/// The disk's looks are 30 s apart, 5 s once a file system is under 50 GB.
#[test]
fn the_disk_is_looked_at_more_often_as_it_fills() {
    let v = |free| Volume { at: PathBuf::from("/"), free, total: 926 * GB };
    assert_eq!(every(&[v(60 * GB)]), EVERY);
    assert_eq!(every(&[v(60 * GB), v(49 * GB)]), EVERY_CLOSE);
}

fn said(ed: &crate::editor::Editor, kind: ToastKind, text: &str) -> usize {
    ed.messages.iter().filter(|m| m.1 == kind && m.2.contains(text)).count()
}

/// Through the editor: each level said once as it's reached (low and the reserve a warning, critical an error),
/// the status line's mark its words; the reserve holds what writes in bulk until it's a quarter past; room again
/// is said. Looks 16 min apart: no drain rate, space alone.
#[test]
fn each_level_is_said_once_and_the_reserve_holds_until_there_is_room() {
    crate::clock::hold();
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    let look = |ed: &mut crate::editor::Editor, gb: u64| {
        crate::clock::advance(16 * MIN);
        let free = gb * GB;
        ed.disk_looked(vec![Volume { at: PathBuf::from("/Volumes/x"), free, total: 926 * GB }], false);
    };
    let mark = |ed: &crate::editor::Editor| ed.procs.mark().map(|c| c.short.clone());
    look(&mut ed, 60);
    assert_eq!(mark(&ed), None);
    look(&mut ed, 45);
    look(&mut ed, 45);
    assert_eq!(mark(&ed).as_deref(), Some("disk 45.0 GB free"));
    // (the words whole: the free space and what tarae does — no command to free it, nothing removed)
    let whole = |ed: &crate::editor::Editor, text: &str| ed.messages.iter().filter(|m| m.2 == text).count();
    assert_eq!(whole(&ed, "Disk low: 45.0 GB free of 926.0 GB on /Volumes/x"), 1);
    assert_eq!(ed.disk.held(), None);
    look(&mut ed, 18);
    assert_eq!(ed.disk.held().as_deref(), Some("disk 18.0 GB free"));
    let reserve = "Disk at its reserve: 18.0 GB free of 926.0 GB on /Volumes/x. Claude's runs and overlap rounds \
                   wait for room";
    assert_eq!(whole(&ed, reserve), 1);
    assert_eq!(said(&ed, ToastKind::Warning, "Claude's runs and overlap rounds wait for room"), 1);
    look(&mut ed, 9);
    let full =
        "Disk almost full: 9.0 GB free of 926.0 GB on /Volumes/x. Claude's runs and overlap rounds wait";
    assert_eq!(whole(&ed, full), 1);
    assert_eq!(said(&ed, ToastKind::Error, full), 1);
    look(&mut ed, 15);
    assert_eq!(ed.disk.held().as_deref(), Some("disk 15.0 GB free"), "the reserve until 23.1 GB");
    look(&mut ed, 30);
    assert_eq!(ed.disk.held(), None);
    assert_eq!(said(&ed, ToastKind::Info, "room again"), 1);
    assert_eq!(mark(&ed).as_deref(), Some("disk 30.0 GB free"), "low still");
    look(&mut ed, 60);
    assert_eq!(mark(&ed), None);
    // A screenshot's tarae says nothing of the host's disk
    let mut shot = crate::editor::Editor::new(crate::config::Config::default());
    shot.disk_looked(vec![Volume { at: PathBuf::from("/"), free: GB, total: 926 * GB }], true);
    assert_eq!((shot.procs.mark().is_none(), shot.disk.held()), (true, None));
    crate::clock::release();
}

/// Draining fast is said by the time left at its rate, before space alone would: 180 GB free after 10 GB in the
/// last half minute is critical (~9 min), its mark the time — said as filling fast, a warning that holds nothing
/// (only the space reserve holds).
#[test]
fn a_burst_is_said_by_the_time_it_leaves_and_holds_nothing() {
    crate::clock::hold();
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    for gb in [200, 196, 190, 180] {
        ed.disk_looked(vec![Volume { at: PathBuf::from("/"), free: gb * GB, total: 926 * GB }], false);
        crate::clock::advance(Duration::from_secs(30));
    }
    assert_eq!(ed.disk.worst(), Level::Critical);
    let mark = ed.procs.mark().map(|c| c.short.clone()).unwrap_or_default();
    assert!(mark.starts_with("disk 180.0 GB · ~9m"), "{mark}");
    // (said once, as the first fall came — its getting worse is the mark's time, nothing new)
    assert_eq!(said(&ed, ToastKind::Warning, "Disk filling fast: 196.0 GB free of 926.0 GB on / — ~"), 1);
    assert_eq!(said(&ed, ToastKind::Warning, "Disk filling fast"), 1);
    assert!(!ed.messages.iter().any(|m| m.1 == ToastKind::Error), "never an error for a time left");
    assert_eq!(said(&ed, ToastKind::Warning, "wait"), 0, "nothing waits");
    assert_eq!(ed.disk.held(), None, "a time left holds nothing");
    // Space at the reserve as well: then it holds, said anew by its space level
    ed.disk_looked(vec![Volume { at: PathBuf::from("/"), free: 15 * GB, total: 926 * GB }], false);
    assert_eq!(ed.disk.held().as_deref(), Some("disk 15.0 GB free"));
    assert_eq!(said(&ed, ToastKind::Warning, "Disk at its reserve: 15.0 GB free"), 1);
    crate::clock::release();
}

/// A relink's burst — 1.5 GB in 15 s — then nothing: the warning it brought (under 2 h at that rate) ends within
/// two looks, the rate gone with the drain; nothing held meanwhile.
#[test]
fn a_burst_that_stopped_ends_its_warning_within_two_looks() {
    crate::clock::hold();
    let mut ed = crate::editor::Editor::new(crate::config::Config::default());
    let look = |ed: &mut crate::editor::Editor, mb: u64| {
        let free = mb << 20;
        ed.disk_looked(vec![Volume { at: PathBuf::from("/"), free, total: 926 * GB }], false);
        crate::clock::advance(Duration::from_secs(15));
    };
    look(&mut ed, 300 * 1024);
    look(&mut ed, 300 * 1024 - 1536);
    assert_eq!(ed.disk.worst(), Level::Low, "~50 min at 1.5 GB in 15 s");
    assert_eq!(said(&ed, ToastKind::Warning, "Disk filling fast: 298.5 GB free"), 1);
    assert_eq!(ed.disk.held(), None);
    look(&mut ed, 300 * 1024 - 1536);
    assert_eq!(ed.disk.worst(), Level::Low, "one quiet look: held a look longer, not said again");
    look(&mut ed, 300 * 1024 - 1536);
    assert_eq!(ed.disk.worst(), Level::Fine, "two: no fall, no rate");
    assert_eq!(said(&ed, ToastKind::Warning, "Disk filling fast"), 1, "said once");
    assert!(ed.procs.mark().is_none());
    crate::clock::release();
}

/// Review #168: two relinks in a row, each 1.5 GB in 15 s with a quiet look between. On a disk low by space
/// (40 GB): `Disk low` once, the bursts only the mark's time — never `filling fast`, never an error. On a disk with
/// room: `Disk filling fast` once, a warning, for both. A flip between the two is nothing new.
#[test]
fn two_bursts_in_a_row_say_each_thing_once() {
    crate::clock::hold();
    let run = |start_mb: u64| {
        let mut ed = crate::editor::Editor::new(crate::config::Config::default());
        let mut free = start_mb;
        let mut marks = Vec::new();
        for fall in [0, 1536, 0, 1536, 0, 0, 0] {
            free -= fall;
            let vol = Volume { at: PathBuf::from("/"), free: free << 20, total: 926 * GB };
            ed.disk_looked(vec![vol], false);
            marks.push(ed.procs.mark().map(|c| c.short.clone()));
            crate::clock::advance(Duration::from_secs(15));
        }
        (ed, marks)
    };
    let (low, marks) = run(40 * 1024);
    assert_eq!(said(&low, ToastKind::Warning, "Disk low: 40.0 GB free"), 1);
    assert_eq!(
        said(&low, ToastKind::Warning, "filling fast"),
        0,
        "a burst on a disk low by space: nothing new"
    );
    assert!(!low.messages.iter().any(|m| m.1 == ToastKind::Error), "never an error for a time left");
    assert!(marks[1].as_deref().is_some_and(|m| m.contains(" · ~")), "the burst: the mark's time {marks:?}");
    assert_eq!(marks.last().unwrap().as_deref(), Some("disk 37.0 GB free"), "low still, by space");
    let (room, _) = run(300 * 1024);
    assert_eq!(said(&room, ToastKind::Warning, "Disk filling fast: 298.5 GB free"), 1, "once for both");
    assert_eq!(said(&room, ToastKind::Warning, "Disk filling fast"), 1);
    assert!(!room.messages.iter().any(|m| m.1 == ToastKind::Error));
    assert!(room.procs.mark().is_none(), "ended two looks after the second");
    crate::clock::release();
}
