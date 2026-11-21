use super::*;
use std::fs;
use std::io::Write as _;

use crate::scratch::Scratch;

// FSEvents' item flags as a stream gives them with a path — handed in for realism: the kind is never read from
// them, only from the stat and the table.
const CREATED: u32 = 0x100 | 0x10000;
const MODIFIED: u32 = 0x1000 | 0x10000;
const RENAMED: u32 = 0x800 | 0x10000;
const DIR_RENAMED: u32 = 0x800 | 0x20000;
const REMOVED: u32 = 0x200 | 0x10000;
const DIR: u32 = 0x20000;
const CREATED_DIR: u32 = 0x100 | 0x20000;
/// Every item flag FSEvents defines, `ItemCreated` (0x100) to `ItemCloned` (0x400000).
const ITEM_FLAGS: u32 = 0x007F_FF00;

/// A scratch folder, and its real path (FSEvents names `/private/var/…`).
fn scratch(name: &str) -> (Scratch, PathBuf) {
    let s = Scratch::new(&format!("treewatch-{name}"));
    let real = s.path().canonicalize().unwrap();
    (s, real)
}

fn rs() -> Keep {
    Arc::new(|p: &Path| p.extension().is_some_and(|e| e == "rs"))
}

fn given(roots: &[&Path], leave_out: Vec<PathBuf>) -> Given {
    Given { roots: roots.iter().map(|r| r.to_path_buf()).collect(), leave_out, keep: rs() }
}

/// A tree over `root` with its table filled, as the worker has it once the stream started.
fn tree(root: &Path, leave_out: &[&str]) -> Tree {
    let mut t = Tree::new(given(&[root], leave_out.iter().map(|l| root.join(l)).collect())).unwrap();
    t.fill(&[root.to_path_buf()]);
    t
}

fn write(p: &Path, text: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

/// Written a minute ago: its stamp held sure (`disk::sure`), so only a real change moves it.
fn aged(p: &Path, text: &str) {
    write(p, text);
    let then = SystemTime::now() - Duration::from_secs(60);
    fs::File::options().write(true).open(p).unwrap().set_modified(then).unwrap();
}

/// A batch with its paths below `root`, to compare.
fn said(batch: Batch, root: &Path) -> Vec<(String, Kind)> {
    let below = |p: PathBuf| p.strip_prefix(root).unwrap().display().to_string();
    batch.into_iter().map(|(p, k)| (below(p), k)).collect()
}

fn kinds(list: &[(&str, Kind)]) -> Vec<(String, Kind)> {
    list.iter().map(|(p, k)| (p.to_string(), *k)).collect()
}

/// The reason the last step's record proves for what it said of `p` (#502).
fn reason(t: &Tree, p: &Path) -> Option<Reason> {
    t.record.get(p).expect("said, so recorded").reason()
}

/// Made, written in place and removed: said once each with its kind — decided by one stat and the table, so a
/// server hears `Created` for a file it never had and `Deleted` for one it had.
#[test]
fn made_written_in_place_and_removed_each_said_once_with_its_kind() {
    let (_s, root) = scratch("kinds");
    let mut t = tree(&root, &[]);
    let a = root.join("src/a.rs");
    write(&a, "fn a() {}");
    assert_eq!(said(t.step(&[(a.clone(), CREATED)]), &root), kinds(&[("src/a.rs", Kind::Created)]));
    fs::OpenOptions::new().append(true).open(&a).unwrap().write_all(b"\n").unwrap();
    assert_eq!(said(t.step(&[(a.clone(), MODIFIED)]), &root), kinds(&[("src/a.rs", Kind::Changed)]));
    fs::remove_file(&a).unwrap();
    assert_eq!(said(t.step(&[(a.clone(), MODIFIED)]), &root), kinds(&[("src/a.rs", Kind::Deleted)]));
    assert_eq!(t.step(&[(a.clone(), MODIFIED)]), [], "gone and not held: nothing");
}

/// A rename is two names: the old one gone (held: deleted), the new one there (not held: created).
#[test]
fn a_rename_says_the_old_name_deleted_and_the_new_created() {
    let (_s, root) = scratch("rename");
    aged(&root.join("a.rs"), "a");
    let mut t = tree(&root, &[]);
    fs::rename(root.join("a.rs"), root.join("b.rs")).unwrap();
    let events = [(root.join("a.rs"), RENAMED), (root.join("b.rs"), RENAMED)];
    assert_eq!(said(t.step(&events), &root), kinds(&[("a.rs", Kind::Deleted), ("b.rs", Kind::Created)]));
}

/// An event for a held file is a change even when its stamp didn't move: a write of the same size within the
/// clock's grain keeps the stamp (a reproducible build's rewrite keeps the very mtime and size), and a server told
/// nothing would keep the old text. The fill's entry, sure (a minute old) or not (written as the walk ran), isn't
/// marked: the fill told the server nothing, so its first event says it changed — the fresh one still owed its
/// once-care until then (#286).
#[test]
fn an_event_naming_a_fills_entry_says_changed_sure_or_not() {
    for sure in [true, false] {
        let (_s, root) = scratch("fill-direct");
        let a = root.join("a.rs");
        if sure {
            aged(&a, "a");
        } else {
            write(&a, "fn a() {}");
        }
        // (the clock held: the fresh stamp stays too fresh to judge whatever the machine's pace)
        crate::clock::hold();
        let mut t = tree(&root, &[]);
        let held = t.table[&a];
        assert_eq!((held.sure, held.owed), (sure, !sure), "the fill's entry, sure {sure}: owed when fresh");
        let stamp = held.stamp;
        assert_eq!(
            said(t.step(&[(a.clone(), MODIFIED)]), &root),
            kinds(&[("a.rs", Kind::Changed)]),
            "sure {sure}"
        );
        assert!(t.table[&a].stamp == stamp, "the stamp the same");
        assert!(t.marked.is_empty(), "no mark taken, sure {sure}");
        assert_eq!(said(t.step(&[(a, MODIFIED)]), &root), kinds(&[("a.rs", Kind::Changed)]), "and the next");
        crate::clock::release();
    }
}

/// A folder moved in is one event, for the folder: each kept file in it, however deep, is created. Moved away,
/// also one event: each file it held is deleted — known only from the table.
#[test]
fn a_folder_moved_in_says_each_kept_file_created_and_moved_away_each_deleted() {
    let (_s, root) = scratch("moved");
    let (_o, outside) = scratch("moved-outside");
    aged(&outside.join("pkg/a.rs"), "a");
    aged(&outside.join("pkg/deep/b.rs"), "b");
    aged(&outside.join("pkg/notes.txt"), "not kept");
    let mut t = tree(&root, &[]);
    fs::rename(outside.join("pkg"), root.join("pkg")).unwrap();
    let moved = [(root.join("pkg"), DIR_RENAMED)];
    assert_eq!(
        said(t.step(&moved), &root),
        kinds(&[("pkg/a.rs", Kind::Created), ("pkg/deep/b.rs", Kind::Created)])
    );
    fs::rename(root.join("pkg"), outside.join("pkg")).unwrap();
    assert_eq!(
        said(t.step(&moved), &root),
        kinds(&[("pkg/a.rs", Kind::Deleted), ("pkg/deep/b.rs", Kind::Deleted)])
    );
}

/// "Must scan below" (FSEvents coalesced what happened there): the folder is compared with the table by stamp —
/// the file changed, the one made and the one removed are said, and none of the files that stayed as they were.
#[test]
fn must_scan_below_says_the_three_changes_and_nothing_else() {
    let (_s, root) = scratch("scan");
    for f in ["src/same.rs", "src/changed.rs", "src/gone.rs", "src/deep/same.rs", "other.rs"] {
        aged(&root.join(f), "text");
    }
    let mut t = tree(&root, &[]);
    write(&root.join("src/changed.rs"), "a longer text");
    write(&root.join("src/deep/new.rs"), "new");
    fs::remove_file(root.join("src/gone.rs")).unwrap();
    assert_eq!(
        said(t.step(&[(root.join("src"), flag::MUST_SCAN_SUB_DIRS)]), &root),
        kinds(&[
            ("src/changed.rs", Kind::Changed),
            ("src/deep/new.rs", Kind::Created),
            ("src/gone.rs", Kind::Deleted)
        ])
    );
}

/// A stamp the walk took in the instant of a write (`disk::sure` says it can't tell a later write of the same size
/// apart) is no proof of "unchanged" once events were lost over it: a compare after "must scan below" says such a
/// file changed even when its stamp didn't move.
#[test]
fn a_compare_after_lost_events_says_changed_where_the_held_stamp_couldnt_tell() {
    let (_s, root) = scratch("racy");
    let a = root.join("a.rs");
    write(&a, "aaaa");
    let mut t = tree(&root, &[]);
    let mtime = fs::metadata(&a).unwrap().modified().unwrap();
    // the same size, its mtime put back: the very stamp the walk took
    write(&a, "bbbb");
    fs::File::options().write(true).open(&a).unwrap().set_modified(mtime).unwrap();
    assert_eq!(
        said(t.step(&[(root.clone(), flag::MUST_SCAN_SUB_DIRS)]), &root),
        kinds(&[("a.rs", Kind::Changed)])
    );
}

/// A file an event made, compared again within `disk::RACY` (its stamp not yet `sure`) — by its folder's event, by a
/// look at its folder, by a look at the root — is said nothing: an event stood for it and its stamp didn't move (W1b
/// step 3, round 5: a checkout's 5,000 files were said changed again by the next folder's compare).
#[test]
fn an_event_made_file_compared_within_two_seconds_says_nothing() {
    let (_s, root) = scratch("fresh-event");
    let mut t = tree(&root, &[]);
    let (src, a) = (root.join("src"), root.join("src/a.rs"));
    write(&a, "fn a() {}");
    // (the clock held: the stamp stays too fresh to judge whatever the machine's pace)
    crate::clock::hold();
    assert_eq!(said(t.step(&[(a.clone(), CREATED)]), &root), kinds(&[("src/a.rs", Kind::Created)]));
    assert!(!t.table[&a].sure, "its stamp too fresh to judge");
    assert_eq!(t.step(&[(src.clone(), DIR)]), [], "its folder's event");
    assert_eq!(t.step(&[(src.clone(), flag::LOOKED)]), [], "a look at its folder");
    crate::clock::advance(Duration::from_secs(1));
    assert_eq!(t.step(&[(root.clone(), flag::LOOKED)]), [], "a look at the root, a second on");
    crate::clock::advance(Duration::from_millis(1500));
    assert_eq!(t.step(&[(src, DIR)]), [], "its stamp judged at last: nothing owed");
    crate::clock::release();
}

/// The same file, compared after FSEvents said events were lost over it ("must scan below"; the dropped flags,
/// every root; "must scan below" for a folder inside one whose own event comes in the same burst — walked once, by
/// the outer, as carefully): its held stamp can't tell a write of the same size apart, so it's said changed.
#[test]
fn an_event_made_file_compared_after_lost_events_is_said_changed() {
    for case in ["must scan", "dropped", "inside a folder's event"] {
        let (_s, root) = scratch("fresh-lost");
        let mut t = tree(&root, &[]);
        let (src, a) = (root.join("src"), root.join("src/a.rs"));
        write(&a, "fn a() {}");
        crate::clock::hold();
        assert_eq!(said(t.step(&[(a.clone(), CREATED)]), &root), kinds(&[("src/a.rs", Kind::Created)]));
        let events = match case {
            "must scan" => vec![(src, flag::MUST_SCAN_SUB_DIRS)],
            "dropped" => vec![(src, flag::KERNEL_DROPPED | flag::MUST_SCAN_SUB_DIRS)],
            _ => vec![(root.clone(), DIR), (src, flag::MUST_SCAN_SUB_DIRS)],
        };
        assert_eq!(said(t.step(&events), &root), kinds(&[("src/a.rs", Kind::Changed)]), "{case}");
        assert_eq!(reason(&t, &a), Some(Reason::Lost), "{case}");
        crate::clock::release();
    }
}

/// A make split across two bursts (W1b step 3, round 6's 430: a checkout cut at `BURST_MAX`): the folder's event in
/// the first, its compare saying the file created; another compare before its own event, the stamp the same; then
/// its own event in the next, its stamp the same — the mark taken, nothing said, its flags kept for the log.
#[test]
fn a_make_split_across_two_bursts_is_said_created_once() {
    let (_s, root) = scratch("split");
    let mut t = tree(&root, &[]);
    let a = root.join("pkg/a.rs");
    write(&a, "fn a() {}");
    crate::clock::hold();
    assert_eq!(
        said(t.step(&[(root.join("pkg"), CREATED_DIR)]), &root),
        kinds(&[("pkg/a.rs", Kind::Created)])
    );
    assert_eq!(t.step(&[(root.clone(), flag::LOOKED)]), [], "a compare before its own event");
    assert_eq!(t.step(&[(a.clone(), CREATED | MODIFIED)]), [], "its own event, late");
    assert_eq!(t.marked, [CREATED | MODIFIED]);
    crate::clock::release();
}

/// The mark is taken once: a second event for the file, its stamp still the same, says it changed — a write of the
/// same size within the clock's grain.
#[test]
fn a_second_event_after_the_mark_is_taken_says_changed() {
    let (_s, root) = scratch("split-twice");
    let mut t = tree(&root, &[]);
    let a = root.join("pkg/a.rs");
    write(&a, "fn a() {}");
    crate::clock::hold();
    assert_eq!(t.step(&[(root.join("pkg"), CREATED_DIR)]).len(), 1);
    assert_eq!(t.step(&[(a.clone(), CREATED)]), [], "the mark taken");
    assert_eq!(said(t.step(&[(a, MODIFIED)]), &root), kinds(&[("pkg/a.rs", Kind::Changed)]));
    assert!(t.marked.is_empty());
    crate::clock::release();
}

/// Friction #502's diagnosis: a make by `fs::write` is two operations on its name (the open that creates it, the write
/// that fills it), so a burst that ends between them (`BURST_MAX`'s cut, or a writer stalled past `QUIET` — a loaded
/// machine) splits it. The event said it created; what comes for it after says changed: its write's own event, the
/// stamp the same ("written, then named again") or moved ("stat between open and write"), or a look's compare where
/// the stamp moved ("the look after"). (Lost events over it, the third source: `an_event_made_file_compared_after_…`.)
#[test]
fn a_make_split_by_a_bursts_end_says_changed_after_created() {
    for case in ["written, then named again", "stat between open and write", "the look after"] {
        let (_s, root) = scratch("split-make");
        let mut t = tree(&root, &[]);
        let a = root.join("pkg/a.rs");
        write(&a, if case == "written, then named again" { "pub fn f() {}\n" } else { "" });
        crate::clock::hold();
        let first = [(root.join("pkg"), CREATED_DIR), (a.clone(), CREATED)];
        assert_eq!(said(t.step(&first), &root), kinds(&[("pkg/a.rs", Kind::Created)]), "{case}");
        if case != "written, then named again" {
            fs::write(&a, "pub fn f() {}\n").unwrap();
        }
        let then = match case {
            "the look after" => (root.clone(), flag::LOOKED),
            _ => (a.clone(), CREATED | MODIFIED),
        };
        assert_eq!(said(t.step(&[then]), &root), kinds(&[("pkg/a.rs", Kind::Changed)]), "{case}");
        // (the record proves it: an event on an entry an event put in an earlier step, or the stamp moved)
        let (want, unmoved) = match case {
            "written, then named again" => (Reason::Named, [0, 1]),
            _ => (Reason::Moved, [0, 0]),
        };
        assert_eq!((reason(&t, &a), t.unmoved), (Some(want), unmoved), "{case}");
        crate::clock::release();
    }
}

/// A first event whose stamp moved since the compare is a write after the make: changed, mark or not.
#[test]
fn a_first_event_with_a_moved_stamp_says_changed() {
    let (_s, root) = scratch("split-moved");
    let mut t = tree(&root, &[]);
    let a = root.join("pkg/a.rs");
    write(&a, "fn a() {}");
    assert_eq!(t.step(&[(root.join("pkg"), CREATED_DIR)]).len(), 1);
    write(&a, "fn a() { /* longer */ }");
    assert_eq!(said(t.step(&[(a, MODIFIED)]), &root), kinds(&[("pkg/a.rs", Kind::Changed)]));
    assert!(t.marked.is_empty());
}

/// The mark ends (calls 7): the split make's late event past `MARKED_FOR` after the compare that set it is a rewrite
/// that kept the very mtime and size (a reproducible build's output), said changed.
#[test]
fn a_late_event_after_the_marks_window_says_changed() {
    let (_s, root) = scratch("split-window");
    let mut t = tree(&root, &[]);
    let a = root.join("pkg/a.rs");
    write(&a, "fn a() {}");
    crate::clock::hold();
    assert_eq!(
        said(t.step(&[(root.join("pkg"), CREATED_DIR)]), &root),
        kinds(&[("pkg/a.rs", Kind::Created)])
    );
    // (rewritten, the same size, its mtime put back: the very stamp the compare took)
    let mtime = fs::metadata(&a).unwrap().modified().unwrap();
    write(&a, "fn b() {}");
    fs::File::options().write(true).open(&a).unwrap().set_modified(mtime).unwrap();
    crate::clock::advance(MARKED_FOR);
    assert_eq!(said(t.step(&[(a.clone(), MODIFIED)]), &root), kinds(&[("pkg/a.rs", Kind::Changed)]));
    assert!(t.marked.is_empty());
    assert_eq!((reason(&t, &a), t.unmoved), (Some(Reason::PastMark), [1, 0]), "counted past the window");
    crate::clock::release();
}

/// A root come back with files in it (a folder moved into place: one event, the root's) marks what its compare
/// found, and no event for those files is coming: the first one past the window says changed.
#[test]
fn a_returned_roots_file_says_changed_at_its_first_event_after_the_window() {
    let (_s, base) = scratch("root-back-window");
    let root = base.join("p");
    aged(&root.join("a.rs"), "a");
    let mut t = tree(&root, &[]);
    fs::rename(&root, base.join("q")).unwrap();
    assert_eq!(said(t.step(&[(root.clone(), flag::ROOT_CHANGED)]), &root), kinds(&[("a.rs", Kind::Deleted)]));
    aged(&base.join("r/b.rs"), "b");
    crate::clock::hold();
    fs::rename(base.join("r"), &root).unwrap();
    assert_eq!(said(t.step(&[(root.clone(), flag::ROOT_CHANGED)]), &root), kinds(&[("b.rs", Kind::Created)]));
    crate::clock::advance(MARKED_FOR);
    assert_eq!(said(t.step(&[(root.join("b.rs"), MODIFIED)]), &root), kinds(&[("b.rs", Kind::Changed)]));
    assert!(t.marked.is_empty());
    crate::clock::release();
}

/// A look naming a file is a compare of it (calls 7): a save's look after its event, in a later step, the stamp the
/// same, says nothing.
#[test]
fn an_event_then_a_look_of_the_same_file_says_nothing_more() {
    let (_s, root) = scratch("save-look");
    let a = root.join("src/a.rs");
    aged(&a, "fn a() {}");
    let mut t = tree(&root, &[]);
    crate::clock::hold();
    write(&a, "fn a() { 1 }");
    assert_eq!(said(t.step(&[(a.clone(), MODIFIED)]), &root), kinds(&[("src/a.rs", Kind::Changed)]));
    assert_eq!(t.step(&[(a, flag::LOOKED)]), [], "the save's look, after its event");
    crate::clock::release();
}

/// A look of a file whose stamp moved says it changed.
#[test]
fn a_look_of_a_file_whose_stamp_moved_says_changed() {
    let (_s, root) = scratch("look-moved");
    let a = root.join("src/a.rs");
    aged(&a, "fn a() {}");
    let mut t = tree(&root, &[]);
    write(&a, "fn a() { 1 }");
    assert_eq!(said(t.step(&[(a, flag::LOOKED)]), &root), kinds(&[("src/a.rs", Kind::Changed)]));
}

/// A look first and the file's own event after, the stamp the same: changed once, by the look — whose find marks it
/// as a compare's does —, then nothing: the event, within the window, takes the mark.
#[test]
fn a_look_first_and_its_event_after_say_changed_once() {
    let (_s, root) = scratch("look-first");
    let a = root.join("src/a.rs");
    aged(&a, "fn a() {}");
    let mut t = tree(&root, &[]);
    crate::clock::hold();
    write(&a, "fn a() { 1 }");
    assert_eq!(said(t.step(&[(a.clone(), flag::LOOKED)]), &root), kinds(&[("src/a.rs", Kind::Changed)]));
    assert_eq!(t.step(&[(a, MODIFIED)]), [], "its event, after the look");
    assert_eq!(t.marked, [MODIFIED]);
    crate::clock::release();
}

/// The log's line for the events the mark took: how many, their flags by count, the most first.
#[test]
fn the_marks_line_says_its_events_flags_most_first() {
    let line = marked_line(&[0x11100, 0x1100, 0x11100]);
    assert!(line.starts_with("the mark took 3 events — "), "{line}");
    assert!(line.ends_with("their flags 0x11100 ×2, 0x1100 ×1"), "{line}");
}

/// A file no event stood for and too fresh to judge — the start's fill found it, or a compare did (a folder moved
/// in) — is said changed once: at the first compare after its stamp can be judged (`disk::RACY` on), not at one
/// before, never again (#286's care, once).
#[test]
fn a_file_the_fill_found_fresh_is_said_changed_once_when_it_can_be_judged() {
    let (_s, root) = scratch("fresh-fill");
    write(&root.join("src/a.rs"), "fn a() {}");
    crate::clock::hold();
    let mut t = tree(&root, &[]);
    write(&root.join("pkg/b.rs"), "fn b() {}");
    assert_eq!(
        said(t.step(&[(root.join("pkg"), CREATED_DIR)]), &root),
        kinds(&[("pkg/b.rs", Kind::Created)]),
        "a compare's own find"
    );
    crate::clock::advance(Duration::from_secs(1));
    assert_eq!(t.step(&[(root.clone(), flag::LOOKED)]), [], "before its stamp can be judged");
    crate::clock::advance(Duration::from_millis(1500));
    assert_eq!(
        said(t.step(&[(root.join("src"), DIR), (root.join("pkg"), DIR)]), &root),
        kinds(&[("pkg/b.rs", Kind::Changed), ("src/a.rs", Kind::Changed)]),
        "the first compare that can judge it"
    );
    crate::clock::advance(Duration::from_secs(1));
    assert_eq!(t.step(&[(root.clone(), flag::LOOKED)]), [], "never again");
    crate::clock::release();
}

/// Friction #502's path 3: a file a compare found (its folder's event; its own not come yet), too fresh to judge, is
/// owed; a look's compare past `disk::RACY` with no event for it before says it changed — the debt judged, once.
/// Its own event first, the look says nothing: an event stood for it, nothing is owed.
#[test]
fn a_compares_find_judged_at_a_look_says_changed_once_unless_its_event_came_first() {
    for event_first in [false, true] {
        let (_s, root) = scratch("debt-look");
        let mut t = tree(&root, &[]);
        let b = root.join("pkg/b.rs");
        write(&b, "fn b() {}");
        crate::clock::hold();
        let made = said(t.step(&[(root.join("pkg"), CREATED_DIR)]), &root);
        assert_eq!(made, kinds(&[("pkg/b.rs", Kind::Created)]), "a compare's own find");
        if event_first {
            assert_eq!(t.step(&[(b.clone(), CREATED | MODIFIED)]), [], "its own event takes the mark");
        }
        crate::clock::advance(crate::disk::RACY);
        let looked = said(t.step(&[(root.clone(), flag::LOOKED)]), &root);
        if event_first {
            assert_eq!(looked, [], "an event stood for it: nothing owed");
        } else {
            assert_eq!(looked, kinds(&[("pkg/b.rs", Kind::Changed)]), "the debt judged at the look");
            assert_eq!(reason(&t, &b), Some(Reason::Debt));
            assert_eq!(t.step(&[(root.clone(), flag::LOOKED)]), [], "once");
        }
        crate::clock::release();
    }
}

/// The record proves a reason or none (#502's calls): a changed with the stamp the same by a folder's compare, a look
/// with no debt or one still too fresh, an event inside its mark's window, on the fill's entry or on one an event put
/// in the same step, lost events over a sure stamp — round 5's bug and its kin — has none; each reason's own has it.
#[test]
fn a_changed_word_has_a_reason_only_where_the_record_proves_one() {
    let now = Instant::now();
    let stamp: Stamp = (SystemTime::UNIX_EPOCH + Duration::from_secs(100), 10);
    let s = Duration::from_secs;
    let r = |why, by, (sure, owed), mark: Option<Duration>, age: Duration| Record {
        why,
        step: 2,
        held: Some(Held {
            stamp,
            sure,
            owed,
            unnamed: mark.map(|_| now),
            put: Some(Put { by, at: now, step: 1 }),
        }),
        stamp: Some(stamp),
        age: Some(age),
        mark,
    };
    let cases = [
        ("a folder's compare", r(Why::Folder, By::Compare, (false, false), Some(s(1)), s(3)), None),
        ("a look, nothing owed", r(Why::Look, By::Compare, (true, false), Some(s(1)), s(3)), None),
        ("a look's debt too fresh", r(Why::Look, By::Compare, (false, true), Some(s(1)), s(1)), None),
        ("a debt an event put", r(Why::Look, By::Event, (false, true), None, s(3)), None),
        ("an event inside the window", r(Why::Event, By::Compare, (false, false), Some(s(1)), s(1)), None),
        ("an event on the fill's entry", r(Why::Event, By::Fill, (true, false), None, s(60)), None),
        ("lost over a sure stamp", r(Why::Lost, By::Event, (true, false), None, s(60)), None),
        (
            "an event in its put's step",
            Record { step: 1, ..r(Why::Event, By::Event, (false, false), None, s(0)) },
            None,
        ),
        ("lost", r(Why::Lost, By::Event, (false, false), None, s(1)), Some(Reason::Lost)),
        (
            "past the mark",
            r(Why::Event, By::Compare, (false, false), Some(MARKED_FOR), s(5)),
            Some(Reason::PastMark),
        ),
        ("a compare's debt", r(Why::Look, By::Compare, (false, true), Some(s(2)), s(2)), Some(Reason::Debt)),
        ("the fill's debt", r(Why::Look, By::Fill, (false, true), None, s(2)), Some(Reason::Debt)),
        ("named again", r(Why::Event, By::Event, (false, false), None, s(0)), Some(Reason::Named)),
        (
            "moved",
            Record { stamp: Some((stamp.0, 11)), ..r(Why::Folder, By::Compare, (true, false), None, s(9)) },
            Some(Reason::Moved),
        ),
    ];
    for (case, record, want) in cases {
        assert_eq!(record.reason(), want, "{case}: {}", record.line());
    }
    let none = r(Why::Folder, By::Compare, (false, false), None, s(3)).line();
    assert!(
        none.starts_with("no reason: by Folder in step 2; put by Some(Compare) in step Some(1)"),
        "{none}"
    );
}

/// Events dropped (by FSEvents' client side or the kernel): whatever path comes with the flag, every root is
/// compared whole — what was dropped could be anywhere.
#[test]
fn dropped_events_compare_every_root_whole() {
    for dropped in [flag::USER_DROPPED, flag::KERNEL_DROPPED] {
        let (_s, root) = scratch("dropped");
        aged(&root.join("src/a.rs"), "a");
        aged(&root.join("tests/b.rs"), "b");
        let mut t = tree(&root, &[]);
        write(&root.join("tests/b.rs"), "changed");
        write(&root.join("tests/c.rs"), "made");
        let event = [(root.join("src"), dropped | flag::MUST_SCAN_SUB_DIRS)];
        assert_eq!(
            said(t.step(&event), &root),
            kinds(&[("tests/b.rs", Kind::Changed), ("tests/c.rs", Kind::Created)]),
            "flag {dropped:#x}"
        );
    }
}

/// A file `keep` refuses, or one below a folder left out, is never said — by its own event, a folder's, or the
/// whole root compared.
#[test]
fn a_file_keep_refuses_or_below_a_left_out_folder_says_nothing() {
    let (_s, root) = scratch("refused");
    let mut t = tree(&root, &["target"]);
    write(&root.join("notes.txt"), "not kept");
    write(&root.join("target/debug/build/out.rs"), "left out");
    let events = [
        (root.join("notes.txt"), CREATED),
        (root.join("target/debug/build/out.rs"), CREATED),
        (root.join("target/debug"), flag::MUST_SCAN_SUB_DIRS),
    ];
    assert_eq!(t.step(&events), []);
    assert_eq!(
        t.step(&[(root.clone(), flag::MUST_SCAN_SUB_DIRS)]),
        [],
        "the root compared leaves them out too"
    );
}

/// `keep` replaced: its new matches come into the table with nothing said (they were there all along) — so the
/// root compared finds nothing new, and an event for one is a change, not a creation.
#[test]
fn keep_replaced_fills_the_table_with_the_new_matches_unsaid() {
    let (_s, root) = scratch("keep");
    aged(&root.join("a.rs"), "a");
    aged(&root.join("README.md"), "read me");
    let mut t = tree(&root, &[]);
    let readme = root.join("README.md");
    assert!(!t.table.contains_key(&readme));
    t.set_keep(Arc::new(|p: &Path| p.extension().is_some_and(|e| e == "rs" || e == "md")));
    assert!(t.table.contains_key(&readme), "the new match is held");
    assert_eq!(t.step(&[(root.clone(), flag::MUST_SCAN_SUB_DIRS)]), [], "nothing said for it");
    write(&readme, "read me, written since");
    assert_eq!(said(t.step(&[(readme, MODIFIED)]), &root), kinds(&[("README.md", Kind::Changed)]));
}

/// Two events for one path in a burst — and a folder's compare that finds it too — say it once.
#[test]
fn two_events_for_one_path_in_a_burst_are_said_once() {
    let (_s, root) = scratch("once");
    aged(&root.join("a.rs"), "a");
    let mut t = tree(&root, &[]);
    write(&root.join("a.rs"), "written twice");
    let a = root.join("a.rs");
    let events = [(a.clone(), MODIFIED), (a, MODIFIED | 0x400), (root.clone(), flag::MUST_SCAN_SUB_DIRS)];
    assert_eq!(said(t.step(&events), &root), kinds(&[("a.rs", Kind::Changed)]));
}

/// A burst settles after `QUIET` with no event, or at `BURST_MAX` from its first however many keep coming —
/// `fswatch`'s rule, so a build's stream of writes is still said twice a second.
#[test]
fn a_burst_settles_after_its_quiet_or_at_its_longest() {
    let t0 = Instant::now();
    let mut b = Burst::default();
    assert_eq!(b.left(t0), None, "none under way");
    b.add(t0);
    assert_eq!(b.left(t0), Some(QUIET));
    assert_eq!(b.left(t0 + QUIET), Some(Duration::ZERO), "quiet: settled");
    let mut at = t0;
    while at < t0 + BURST_MAX {
        b.add(at);
        at += QUIET / 2;
    }
    assert_eq!(b.left(t0 + BURST_MAX), Some(Duration::ZERO), "never quiet: settled at its longest");
}

/// A root itself renamed away or removed: each file it held is deleted, and it stays a root — when it's there
/// again (FSEvents' root-changed word, an event below it, or a look of it) it's compared, its files created: a
/// `cargo clean` and the next build, for a build script's out dir.
#[test]
fn a_root_removed_says_its_files_deleted_and_made_again_its_files_created() {
    let (_s, base) = scratch("root-gone");
    let root = base.join("p");
    aged(&root.join("a.rs"), "a");
    aged(&root.join("src/b.rs"), "b");
    let mut t = tree(&root, &[]);
    fs::rename(&root, base.join("q")).unwrap();
    assert_eq!(
        said(t.step(&[(root.clone(), flag::ROOT_CHANGED)]), &root),
        kinds(&[("a.rs", Kind::Deleted), ("src/b.rs", Kind::Deleted)])
    );
    assert_eq!(t.root_paths(), std::slice::from_ref(&root), "still a root");
    // back, by the root-changed word
    aged(&root.join("c.rs"), "c");
    assert_eq!(said(t.step(&[(root.clone(), flag::ROOT_CHANGED)]), &root), kinds(&[("c.rs", Kind::Created)]));
    // gone and back, by an event below it: the root compared, not only the file named
    fs::remove_dir_all(&root).unwrap();
    assert_eq!(said(t.step(&[(root.clone(), flag::ROOT_CHANGED)]), &root), kinds(&[("c.rs", Kind::Deleted)]));
    aged(&root.join("d.rs"), "d");
    aged(&root.join("gen/e.rs"), "e");
    assert_eq!(
        said(t.step(&[(root.join("d.rs"), CREATED)]), &root),
        kinds(&[("d.rs", Kind::Created), ("gen/e.rs", Kind::Created)])
    );
    // gone (no word of it came) and back, by a look of the root
    fs::remove_dir_all(&root).unwrap();
    aged(&root.join("f.rs"), "f");
    let got = said(t.step(&[(root.clone(), flag::LOOKED)]), &root);
    assert_eq!(got, kinds(&[("d.rs", Kind::Deleted), ("f.rs", Kind::Created), ("gen/e.rs", Kind::Deleted)]));
}

/// A root not there yet is taken (a request naming another server's out dir before its build isn't refused) and
/// watched from when it's made; one that's there and isn't a folder is refused.
#[test]
fn a_root_not_there_yet_is_taken_and_watched_once_made() {
    let (_s, base) = scratch("root-absent");
    let out = base.join("target/debug/build/x-1/out");
    let mut t = Tree::new(given(&[&base, &out], vec![base.join("target")])).unwrap();
    t.fill(&t.root_paths());
    assert!(t.roots.iter().any(|r| r.path == out && r.id.is_none()));
    aged(&out.join("gen.rs"), "gen");
    assert_eq!(said(t.step(&[(out.clone(), flag::ROOT_CHANGED)]), &out), kinds(&[("gen.rs", Kind::Created)]));
    write(&base.join("a.rs"), "a");
    let why = roots_of(&[base.clone(), base.join("a.rs")]).err().expect("a file isn't a root");
    assert!(why.ends_with("a.rs isn't a folder"), "{why}");
}

/// Roots replaced: the old root's files leave the table unsaid (not deleted, only no longer watched), and an
/// event from it — one the old stream still said — is nothing.
#[test]
fn roots_replaced_say_nothing_more_of_the_old_one() {
    let (_a, a) = scratch("roots-a");
    let (_b, b) = scratch("roots-b");
    aged(&a.join("a.rs"), "a");
    aged(&b.join("b.rs"), "b");
    let mut t = tree(&a, &[]);
    t.set(roots_of(std::slice::from_ref(&b)).unwrap(), vec![]);
    assert_eq!(t.table.keys().collect::<Vec<_>>(), [&b.join("b.rs")]);
    write(&a.join("a.rs"), "changed");
    assert_eq!(t.step(&[(a.join("a.rs"), MODIFIED), (a.clone(), flag::MUST_SCAN_SUB_DIRS)]), []);
}

/// The folders left out replaced: what's below a new one leaves the table, one no longer left out is walked —
/// both unsaid; and with a new root in the same call, it's walked too.
#[test]
fn left_out_replaced_drops_and_fills_the_table_unsaid() {
    let (_s, root) = scratch("left-out");
    let (_o, other) = scratch("left-out-other");
    aged(&root.join("src/a.rs"), "a");
    aged(&root.join("gen/b.rs"), "b");
    aged(&other.join("o.rs"), "o");
    let mut t = tree(&root, &["gen"]);
    assert_eq!(t.table.keys().collect::<Vec<_>>(), [&root.join("src/a.rs")]);
    t.set(roots_of(&t.root_paths()).unwrap(), vec![root.join("src")]);
    assert_eq!(t.table.keys().collect::<Vec<_>>(), [&root.join("gen/b.rs")]);
    assert_eq!(t.step(&[(root.clone(), flag::MUST_SCAN_SUB_DIRS)]), []);
    t.set(roots_of(&[root.clone(), other.clone()]).unwrap(), vec![root.join("gen")]);
    let mut want = vec![root.join("src/a.rs"), other.join("o.rs")];
    want.sort();
    assert_eq!(t.table.keys().cloned().collect::<Vec<_>>(), want, "roots and left out in one call");
}

/// Up to `EXCLUDED_MAX` folders left out go to a root's stream, in the order given (the largest first), and only
/// those below it: one outside (or the root itself) would leave out nothing of the stream's. Past them, the root is
/// named to be said once (the rest dropped by prefix).
#[test]
fn the_stream_leaves_out_the_first_eight_below_a_root() {
    let root = PathBuf::from("/r");
    let mut out: Vec<PathBuf> = vec!["/elsewhere/target".into(), root.clone()];
    out.extend((0..10).map(|i| root.join(format!("o{i}"))));
    let (groups, over) = groups(std::slice::from_ref(&root), &out);
    let exclude = (0..8).map(|i| root.join(format!("o{i}"))).collect();
    assert_eq!(groups, [Group { roots: vec![root.clone()], exclude }]);
    assert_eq!(over, [root]);
}

/// Each root's `.git` and `target`, in its order.
fn git_and_target(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots.iter().flat_map(|r| [r.join(".git"), r.join("target")]).collect()
}

/// Roots below a folder left out with none of their own — build scripts' out dirs below `target` — share one
/// stream with no exclusions, apart from the root that folder is below, whose stream leaves it out (but not a
/// folder below another it leaves out).
#[test]
fn roots_below_a_left_out_folder_share_one_stream() {
    let root = PathBuf::from("/r");
    let (out, out2) = (root.join("target/debug/build/x-1/out"), root.join("target/debug/build/y-2/out"));
    let roots = vec![root.clone(), out.clone(), out2.clone()];
    let leave_out = vec![root.join("target"), root.join(".git"), root.join("target/debug")];
    let (groups, over) = groups(&roots, &leave_out);
    assert!(over.is_empty());
    let want = [
        Group { roots: vec![root.clone()], exclude: leave_out[..2].to_vec() },
        Group { roots: vec![out, out2], exclude: vec![] },
    ];
    assert_eq!(groups, want);
}

/// A nested root with folders left out of its own — a checkout below `worktrees`, with its `target` — gets its own
/// stream, its `target` excluded there, never a flood dropped by prefix; an out dir beside it, with none, goes in
/// the shared stream; the outer root's stream leaves out `worktrees` and not the checkout's folders below it.
#[test]
fn a_nested_root_with_its_own_left_out_folders_gets_its_own_stream() {
    let root = PathBuf::from("/r");
    let (co, out) = (root.join("worktrees/co"), root.join("target/debug/build/x-1/out"));
    let roots = vec![root.clone(), co.clone(), out.clone()];
    let leave_out = vec![
        root.join("target"),
        root.join("worktrees"),
        root.join(".git"),
        co.join("target"),
        co.join(".git"),
    ];
    let (groups, over) = groups(&roots, &leave_out);
    assert!(over.is_empty());
    let want = [
        Group { roots: vec![root.clone()], exclude: leave_out[..3].to_vec() },
        Group { roots: vec![co], exclude: leave_out[3..].to_vec() },
        Group { roots: vec![out], exclude: vec![] },
    ];
    assert_eq!(groups, want);
}

/// Ten checkouts with a `.git` and a `target` each — twenty folders left out — and one root with ten: a stream a
/// root, in the order given, each leaving out its own folders, and only the big root meets the cap — its first
/// `EXCLUDED_MAX`, the root named to be said once.
#[test]
fn each_root_has_a_stream_with_its_own_exclusions() {
    let mut roots: Vec<PathBuf> = (0..10).map(|i| PathBuf::from(format!("/w/c{i}"))).collect();
    let mut leave_out = git_and_target(&roots);
    let big = PathBuf::from("/w/big");
    leave_out.extend((0..10).map(|i| big.join(format!("o{i}"))));
    roots.push(big.clone());
    let (groups, over) = groups(&roots, &leave_out);
    let mut want: Vec<Group> = (roots[..10].iter())
        .map(|r| Group { roots: vec![r.clone()], exclude: git_and_target(std::slice::from_ref(r)) })
        .collect();
    want.push(Group { roots: vec![big.clone()], exclude: leave_out[20..28].to_vec() });
    assert_eq!(groups, want);
    assert_eq!(over, [big]);
}

/// A root's stream depends on that root alone: a root that gains a folder left out (`target` after its first
/// build), one that comes first in the sorted list, one that goes — every other root's stream stays as it was, the
/// nested roots' too, so `set` leaves them running; a nested root that comes changes only the nested stream.
#[test]
fn one_roots_change_leaves_the_other_streams_as_they_were() {
    let mut roots: Vec<PathBuf> = (1..6).map(|i| PathBuf::from(format!("/w/c{i}"))).collect();
    roots.push(roots[0].join("target/debug/build/x-1/out"));
    let leave_out = |roots: &[PathBuf]| git_and_target(roots);
    let (before, _) = groups(&roots, &leave_out(&roots));
    assert_eq!(before.len(), 6, "{before:#?}");
    let stay = |after: (Vec<Group>, Vec<PathBuf>), but: &[&PathBuf]| {
        let ours = |g: &&Group| !g.roots.iter().any(|r| but.contains(&r));
        let gone: Vec<&Group> = before.iter().filter(ours).filter(|g| !after.0.contains(g)).collect();
        assert!(gone.is_empty(), "restarted: {gone:#?}\nbefore {before:#?}\nafter {:#?}", after.0);
    };
    let mut more = leave_out(&roots);
    more.push(roots[2].join("node_modules"));
    stay(groups(&roots, &more), &[&roots[2]]);
    let mut added = roots.clone();
    added.insert(0, PathBuf::from("/w/a"));
    stay(groups(&added, &leave_out(&added)), &[]);
    let mut removed = roots.clone();
    removed.remove(3);
    stay(groups(&removed, &leave_out(&removed)), &[&roots[3]]);
    let mut nested = roots.clone();
    nested.push(roots[1].join("target/out"));
    stay(groups(&nested, &leave_out(&nested)), &[&roots[5]]);
}

/// The prefix drop, the last line: what reaches the worker below a folder no stream leaves out — a root's ninth,
/// here `target`, holding a root — is dropped and counted, while the nested root's own file is said.
#[test]
fn the_prefix_drop_drops_and_counts_what_reaches_it() {
    let (_s, root) = scratch("prefix-drop");
    let out = root.join("target/debug/build/x-1/out");
    let mut leave_out: Vec<PathBuf> = (0..8).map(|i| root.join(format!("o{i}"))).collect();
    leave_out.push(root.join("target"));
    let roots = [root.clone(), out.clone()];
    let mut t = Tree::new(Given { roots: roots.to_vec(), leave_out, keep: rs() }).unwrap();
    t.fill(&roots);
    write(&root.join("target/debug/x.rs"), "left out");
    write(&out.join("gen.rs"), "gen");
    let events = [(root.join("target/debug/x.rs"), CREATED), (out.join("gen.rs"), CREATED)];
    assert_eq!(said(t.step(&events), &root), kinds(&[("target/debug/build/x-1/out/gen.rs", Kind::Created)]));
    assert_eq!(t.unwatched, 1);
    assert_eq!(t.unwatched_paths, [root.join("target/debug/x.rs")]);
}

/// The start says no, in words, for a root that's there and isn't a folder — before anything runs.
#[test]
fn the_start_refuses_a_root_that_isnt_a_folder() {
    let (_s, root) = scratch("no-root");
    write(&root.join("a.rs"), "a");
    let why = TreeWatch::start(given(&[&root.join("a.rs")], vec![]), |_| {}).err().expect("refused");
    #[cfg(target_os = "macos")]
    assert!(why.ends_with("a.rs isn't a folder"), "{why}");
    #[cfg(not(target_os = "macos"))]
    assert!(why.starts_with("no tree watch on"), "{why}");
}

/// A look (an event marked `flag::LOOKED`): a file is compared as a folder's file is — nothing when its stamp held
/// (calls 7) —, a folder is compared. What a look's compare finds that no event named, made or its stamp moved and that stamp old, is
/// counted missed; the same found by the stream's own compare, or with a fresh stamp, or named by an event, isn't.
#[test]
fn a_look_decides_a_file_by_the_table_compares_a_folder_and_counts_what_the_stream_missed() {
    let (_s, root) = scratch("look");
    aged(&root.join("same.rs"), "same");
    aged(&root.join("a.rs"), "a");
    aged(&root.join("named.rs"), "named");
    let mut t = tree(&root, &[]);
    let same = [(root.join("same.rs"), flag::LOOKED)];
    assert_eq!(t.step(&same), [], "its stamp held: nothing");
    let thirty_ago = SystemTime::now() - Duration::from_secs(30);
    changes_in(&root, thirty_ago);
    let looked = [(root.join("named.rs"), MODIFIED), (root.clone(), flag::LOOKED)];
    let s = t.settle(&looked).expect("said");
    assert_eq!(
        said(s.batch, &root),
        kinds(&[
            ("a.rs", Kind::Changed),
            ("copied.rs", Kind::Created),
            ("fresh.rs", Kind::Created),
            ("named.rs", Kind::Changed)
        ])
    );
    assert_eq!(t.missed, 2, "a.rs and copied.rs: old stamps, no event");
    // (fresh.rs's stamp, held in the instant of its write, may say it changed again: never missed)
    t.settle(&[(root.clone(), flag::LOOKED)]);
    assert_eq!(t.missed, 2, "nothing moved: nothing missed, the running total as it was");

    // the stream's own compare finds the same and counts nothing
    let (_s, root) = scratch("look-stream");
    aged(&root.join("a.rs"), "a");
    aged(&root.join("named.rs"), "named");
    let mut t = tree(&root, &[]);
    changes_in(&root, thirty_ago);
    assert_eq!(t.step(&[(root.clone(), flag::MUST_SCAN_SUB_DIRS)]).len(), 4);
    assert_eq!(t.missed, 0, "the stream asked for that compare");
    // and a folder's own event carrying every item flag FSEvents defines: the stream's, never a look's. (`fresh.rs`
    // written again as it was a moment ago keeps its stamp wherever the filesystem's clock didn't tick between the two
    // writes — a jiffy on Linux before multigrain stamps: 5 runs in 10 on colima's 6.8 — and a folder's compare goes by
    // the stamp alone (round 5): so it's written longer, its size the stamp's move whatever the clock.)
    changes_in(&root, thirty_ago - Duration::from_secs(10));
    write(&root.join("fresh.rs"), "just made, and again");
    assert_eq!(t.step(&[(root.clone(), ITEM_FLAGS)]).len(), 4);
    assert_eq!(t.missed, 0, "the stream named that folder");
}

/// The missed total is logged the first time and each time it doubles: 1, 2, 4, 8, … — a step past one, once.
#[test]
fn the_missed_total_is_logged_each_time_it_doubles() {
    let logged: Vec<(usize, usize)> =
        [(0, 0), (0, 1), (1, 1), (1, 2), (2, 3), (3, 4), (4, 7), (7, 8), (2, 9), (9, 15)]
            .into_iter()
            .filter(|&(before, after)| doubled(before, after))
            .collect();
    assert_eq!(logged, [(0, 1), (1, 2), (3, 4), (7, 8), (2, 9)]);
}

/// `a.rs` and `named.rs` written, `copied.rs` made — each its mtime put at `at`, as `cp -p` leaves it — and
/// `fresh.rs` made now.
fn changes_in(root: &Path, at: SystemTime) {
    for (f, text) in
        [("a.rs", "a, written since"), ("named.rs", "named, written since"), ("copied.rs", "copy")]
    {
        write(&root.join(f), text);
        fs::File::options().write(true).open(root.join(f)).unwrap().set_modified(at).unwrap();
    }
    write(&root.join("fresh.rs"), "just made");
}

/// `look`, `keep` and `set` go to the worker — a look as events marked `flag::LOOKED`; a look and
/// `keep` sent, with no reply waited for (the main loop calls them) — and a gone worker is said by each, never taken
/// for a quiet tree.
#[test]
fn the_watch_hands_a_look_to_the_worker_and_says_when_its_gone() {
    let (tx, rx) = mpsc::channel();
    let w = TreeWatch { tx };
    let p = PathBuf::from("/r/a.rs");
    w.look(vec![p.clone()]).unwrap();
    assert!(matches!(rx.recv().unwrap(), Msg::Events(e) if e == [(p.clone(), flag::LOOKED)]));
    w.keep(rs()).expect("sent, no reply waited for");
    assert!(matches!(rx.recv().unwrap(), Msg::Keep(_)));
    drop(rx);
    assert_eq!(w.look(vec![p]), Err(GONE.to_string()));
    assert_eq!(w.keep(rs()), Err(GONE.to_string()));
    assert_eq!(w.set(vec![], vec![]), Err(GONE.to_string()));
}

/// The nearest folder wins: a build script's output folder below `target`, given as a root of its own, is watched —
/// its files kept and said, by their events, by the root's compare (no deletion: the walk reaches it past
/// `target`) and by a compare of `target` itself (its roots compared) — while the rest of `target` says nothing.
/// And `target` isn't given to the stream: it would leave out the root below it.
#[test]
fn a_root_below_a_left_out_folder_is_watched() {
    let (_s, root) = scratch("nested-out");
    let out = root.join("target/debug/build/out");
    aged(&root.join("src/a.rs"), "a");
    aged(&out.join("gen.rs"), "gen");
    aged(&root.join("target/debug/x.rs"), "left out");
    let roots = [root.clone(), out.clone()];
    let leave_out = vec![root.join("target"), root.join(".git")];
    let mut t = Tree::new(Given { roots: roots.to_vec(), leave_out: leave_out.clone(), keep: rs() }).unwrap();
    t.fill(&roots);
    assert_eq!(t.table.keys().collect::<Vec<_>>(), [&root.join("src/a.rs"), &out.join("gen.rs")]);
    let streams = [
        Group { roots: vec![root.clone()], exclude: leave_out.clone() },
        Group { roots: vec![out.clone()], exclude: vec![] },
    ];
    assert_eq!(
        groups(&roots, &leave_out).0,
        streams,
        "target left out by the root's stream; the out dir apart"
    );
    // (aged: a stamp taken in the instant of its write would make the compare below say it changed, `disk::sure`)
    aged(&out.join("new.rs"), "new");
    write(&root.join("target/debug/y.rs"), "left out");
    let events = [(out.join("new.rs"), CREATED), (root.join("target/debug/y.rs"), CREATED)];
    assert_eq!(said(t.step(&events), &root), kinds(&[("target/debug/build/out/new.rs", Kind::Created)]));
    assert_eq!(t.step(&[(root.clone(), flag::MUST_SCAN_SUB_DIRS)]), [], "the root's compare reaches it");
    write(&out.join("more.rs"), "more");
    assert_eq!(
        said(t.step(&[(root.join("target"), flag::MUST_SCAN_SUB_DIRS)]), &root),
        kinds(&[("target/debug/build/out/more.rs", Kind::Created)])
    );
}

/// A checkout nested below a folder that becomes left out stays watched as its own root — its files kept, its own
/// folders left out still out — while the rest of that folder leaves the table.
#[test]
fn a_checkout_nested_below_a_left_out_folder_is_watched() {
    let (_s, root) = scratch("nested-checkout");
    let co = root.join("worktrees/co");
    aged(&co.join("src/c.rs"), "c");
    aged(&root.join("worktrees/other.rs"), "other");
    let roots = [root.clone(), co.clone()];
    let mut t = Tree::new(Given { roots: roots.to_vec(), leave_out: vec![], keep: rs() }).unwrap();
    t.fill(&roots);
    t.set(roots_of(&roots).unwrap(), vec![root.join("worktrees"), co.join("target")]);
    assert_eq!(t.table.keys().collect::<Vec<_>>(), [&co.join("src/c.rs")]);
    write(&co.join("src/c.rs"), "c, written");
    write(&root.join("worktrees/other.rs"), "other, written");
    write(&co.join("target/t.rs"), "left out");
    let events = [
        (co.join("src/c.rs"), MODIFIED),
        (root.join("worktrees/other.rs"), MODIFIED),
        (co.join("target/t.rs"), CREATED),
    ];
    assert_eq!(said(t.step(&events), &root), kinds(&[("worktrees/co/src/c.rs", Kind::Changed)]));
}

/// A rename that only changes case, on a case-insensitive volume: one stat finds the file under both names, so
/// their folder is compared — the old name deleted, the new created; a folder's too.
#[test]
fn a_rename_that_only_changes_case_says_the_old_deleted_and_the_new_created() {
    let (_s, root) = scratch("case");
    write(&root.join("probe"), "");
    if !root.join("PROBE").exists() {
        return crate::test_results::said_skip(
            "treewatch::tests::a_rename_that_only_changes_case_says_the_old_deleted_and_the_new_created — \
             a case-sensitive volume",
        );
    }
    aged(&root.join("a.rs"), "a");
    aged(&root.join("src/b.rs"), "b");
    let mut t = tree(&root, &[]);
    fs::rename(root.join("a.rs"), root.join("A.rs")).unwrap();
    let events = [(root.join("a.rs"), RENAMED), (root.join("A.rs"), RENAMED)];
    assert_eq!(said(t.step(&events), &root), kinds(&[("A.rs", Kind::Created), ("a.rs", Kind::Deleted)]));
    fs::rename(root.join("src"), root.join("Src")).unwrap();
    let events = [(root.join("src"), DIR_RENAMED), (root.join("Src"), DIR_RENAMED)];
    assert_eq!(
        said(t.step(&events), &root),
        kinds(&[("Src/b.rs", Kind::Created), ("src/b.rs", Kind::Deleted)])
    );
}

/// `watchable`: a root on its own — a local folder, or why not — so one bad root is left out, the others kept.
#[test]
fn watchable_says_why_a_root_isnt() {
    let (_s, root) = scratch("watchable");
    write(&root.join("a.rs"), "a");
    assert_eq!(watchable(&root), Ok(()));
    let why = watchable(&root.join("missing")).unwrap_err();
    assert!(why.ends_with("missing isn't a folder"), "{why}");
    let why = watchable(&root.join("a.rs")).unwrap_err();
    assert!(why.ends_with("a.rs isn't a folder"), "{why}");
}

/// An event FSEvents says is of a plain file, its path no server keeps and the table holding nothing at or below it
/// (a build's `.o`s): dropped with no stat, counted once for its path however many events named it.
#[test]
fn a_file_event_no_server_wants_costs_no_stat() {
    let (_s, root) = scratch("unwanted");
    let mut t = tree(&root, &[]);
    write(&root.join("out/a.o"), "");
    let events = [(root.join("out/a.o"), CREATED), (root.join("out/a.o"), MODIFIED)];
    assert_eq!(t.step(&events), []);
    assert_eq!((t.stats, t.unkept), (0, 1), "no stat, one drop for the path");
}

/// `unwanted` comes before `watched`: a plain file no server keeps below a folder left out counts as unwanted, not
/// as dropped by prefix, and is never checked against the roots and what's left out; a kept file below it still is.
#[test]
fn an_unwanted_file_is_dropped_before_the_roots_are_asked() {
    let (_s, root) = scratch("unwanted-first");
    let mut t = tree(&root, &["target"]);
    write(&root.join("target/a.o"), "");
    write(&root.join("target/b.rs"), "");
    let checked = t.checked.get();
    assert_eq!(t.step(&[(root.join("target/a.o"), CREATED)]), []);
    assert_eq!((t.unkept, t.unwatched, t.stats), (1, 0, 0), "unwanted, not by prefix, no stat");
    assert_eq!(t.checked.get() - checked, 0, "never checked against the roots and what's left out");
    assert_eq!(t.step(&[(root.join("target/b.rs"), CREATED)]), []);
    assert_eq!((t.unkept, t.unwatched, t.stats), (1, 1, 0), "a kept file below it: by prefix");
    assert!(t.checked.get() - checked >= 1);
}

/// A path the table holds, or holds something below, is looked at whatever its flags say, and said deleted when
/// it's gone: a folder replaced by a file of its name (the file's event) says what the folder held deleted; a held
/// file no server keeps now (put so by hand: `keep` replaced with the table left as it was), removed, is deleted.
#[test]
fn a_path_the_table_holds_is_still_looked_at_and_said_deleted_when_gone() {
    let (_s, root) = scratch("unwanted-held");
    write(&root.join("gen/a.rs"), "");
    write(&root.join("notes.txt"), "");
    let mut t =
        Tree::new(Given { roots: vec![root.clone()], leave_out: vec![], keep: Arc::new(|_: &Path| true) })
            .unwrap();
    t.fill(std::slice::from_ref(&root));
    t.keep = rs();
    fs::remove_dir_all(root.join("gen")).unwrap();
    write(&root.join("gen"), "a file now");
    assert_eq!(said(t.step(&[(root.join("gen"), CREATED)]), &root), kinds(&[("gen/a.rs", Kind::Deleted)]));
    fs::remove_file(root.join("notes.txt")).unwrap();
    assert_eq!(
        said(t.step(&[(root.join("notes.txt"), REMOVED)]), &root),
        kinds(&[("notes.txt", Kind::Deleted)])
    );
    assert_eq!((t.stats, t.unkept), (2, 0), "both looked at");
}

/// An event flagged both a file and a folder is looked at — one event's flags, or two events' for one path in a
/// burst (a file replaced by a folder): the folder compared, its kept file created.
#[test]
fn an_event_flagged_both_file_and_folder_is_looked_at() {
    let (_s, root) = scratch("unwanted-both");
    let mut t = tree(&root, &[]);
    write(&root.join("out.d/a.rs"), "");
    let both = [(root.join("out.d"), CREATED | DIR)];
    assert_eq!(said(t.step(&both), &root), kinds(&[("out.d/a.rs", Kind::Created)]));
    write(&root.join("gen.o/b.rs"), "");
    let two = [(root.join("gen.o"), CREATED), (root.join("gen.o"), CREATED_DIR)];
    assert_eq!(said(t.step(&two), &root), kinds(&[("gen.o/b.rs", Kind::Created)]));
    assert_eq!(t.unkept, 0);
}

/// A kept file's event is looked at: one stat, said created.
#[test]
fn a_kept_files_event_is_looked_at() {
    let (_s, root) = scratch("unwanted-kept");
    let mut t = tree(&root, &[]);
    write(&root.join("out/b.rs"), "");
    assert_eq!(
        said(t.step(&[(root.join("out/b.rs"), CREATED)]), &root),
        kinds(&[("out/b.rs", Kind::Created)])
    );
    assert_eq!((t.stats, t.unkept), (1, 0));
}

/// Every batch is said with its counts, what said them and its events' flags that asked for more than a look (the
/// live switch counts read these lines) — one of changes only too.
#[test]
fn a_batch_is_said_with_its_counts_and_where_it_came_from() {
    let p = PathBuf::from;
    let batch = [
        (p("/r/a.rs"), Kind::Created),
        (p("/r/b.rs"), Kind::Deleted),
        (p("/r/c.rs"), Kind::Deleted),
        (p("/r/d.rs"), Kind::Changed),
    ];
    let events = [
        (p("/r/a.rs"), CREATED),
        (p("/r"), DIR | flag::MUST_SCAN_SUB_DIRS),
        (p("/r/b.rs"), REMOVED | flag::KERNEL_DROPPED),
    ];
    let line = said_files(&batch, ([1, 2, 1, 0], [0, 0]), &events, None);
    let want = "a batch said 1 created, 2 deleted, 1 changed, said by 1 an event (changed with the stamp the same: 0 \
                past a mark's window, 0 with no mark), 2 a folder's compare, 0 a look, 1 lost events, from 3 events \
                (1 must scan below, 1 dropped, 0 root changed)";
    assert!(line.starts_with(want), "{line}");
    let line = said_files(&batch[3..], ([0, 0, 0, 1], [0, 0]), &events[..1], None);
    let want = "a batch said 0 created, 0 deleted, 1 changed, said by 0 an event (changed with the stamp the same: 0 \
                past a mark's window, 0 with no mark), 0 a folder's compare, 1 a look, 0 lost events, from 1 events";
    assert!(line.starts_with(want), "changes only: {line}");
    let line = said_files(&batch[3..], ([0, 0, 1, 0], [2, 3]), &events[..1], None);
    let want = "a batch said 0 created, 0 deleted, 1 changed, said by 1 an event (changed with the stamp the same: 2 \
                past a mark's window, 3 with no mark), 0 a folder's compare";
    assert!(line.starts_with(want), "changes only: {line}");
}

/// What a step said is told apart by why: a file's event, a folder's compare, a look, lost events (`Tree.by`).
#[test]
fn a_step_counts_what_it_said_by_why() {
    let (_s, root) = scratch("by-why");
    aged(&root.join("src/kept.rs"), "kept");
    let mut t = tree(&root, &[]);
    write(&root.join("a.rs"), "a");
    write(&root.join("pkg/b.rs"), "b");
    write(&root.join("looked.rs"), "l");
    write(&root.join("src/new.rs"), "n");
    let events = [
        (root.join("a.rs"), CREATED),
        (root.join("pkg"), CREATED_DIR),
        (root.join("looked.rs"), flag::LOOKED),
        (root.join("src"), flag::MUST_SCAN_SUB_DIRS),
    ];
    assert_eq!(t.step(&events).len(), 4);
    let by = |why: Why| t.by[why as usize];
    assert_eq!((by(Why::Event), by(Why::Folder), by(Why::Look), by(Why::Lost)), (1, 1, 1, 1));
}

#[cfg(target_os = "macos")]
mod stream;

#[cfg(target_os = "macos")]
mod measure;
