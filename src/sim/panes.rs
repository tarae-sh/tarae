//! The layout kept, simulated (panes P2b, plan §5.1): the window keys that change a checkout's panes — `C-w v`/`s`,
//! `q`, `o`, `H`/`L`, `t`, `w`, `>`, `=` — and another file opened in a pane, frames, the user taking their time;
//! each change written by a worker (`layout_keep`), the writes run in any order. After every step: the file holds a
//! layout the editor asked for, never one older than what it held before; once quiet, the newest.
//!
//! P3 step 6: the tests group docked and back (`C-w H`/`L`/`K`/`J` from its list) racing its run's end and a new run
//! — the place is part of the layout written, so the newest place wins the file. After every step a docked group's
//! leaf is in the tree exactly while its run is there, once; the slot's tabs never list a group docked in sight; the
//! keys are a docked group's (`Viewer.hand`) only while they're in it.
//!
//! P3 step 7: floated and back (`C-w e`), moved (`C-w m`, `l`, `j`), its run's `q` racing its end and a card coming
//! over it. After every step a floating group's own pane is out of the tree, once, while it floats; drawn, its frame is
//! on the screen whole; the slot's tabs never list it while it floats in sight; `C-w m` moves only a pane there.
//!
//! onetree S2c: sizes set (`C-w < > - +`) and a restart as the layout has it — the tree read back from its file with
//! the surfaces' shares as a file from before any size held them, the sizes from `sizes.json`'s text: the next frame
//! lays every pane in the same rect.
//!
//! Polish landing 1 and #299: Claude at work in follow mode the whole run — neither a card that came on its own nor the
//! follow card covers a floating pane's frame.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use serde_json::Value;

use super::{Choices, Sim};
use crate::editor::Editor;
use crate::event::Event;
use crate::key::{Code, Key};

const SEEDS: &[u64] = &[0, 1, 2];

const INPUTS: usize = 50;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn the_newest_layout_wins() {
    super::seeds("the_newest_layout_wins", SEEDS, run);
}

/// The layouts the editor asked to have written, by their number, and the newest the file was seen holding.
#[derive(Default)]
struct World {
    asked: RefCell<Vec<(u64, Value)>>,
    read: Cell<u64>,
    /// A restart that laid a pane elsewhere, said.
    moved: RefCell<Option<String>>,
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("the_newest_layout_wins", choices, LIMIT);
    let dir = crate::scratch::Scratch::new("sim-panes");
    let root = dir.to_path_buf();
    for f in ["a.txt", "b.txt"] {
        std::fs::write(root.join(f), format!("{f}\n")).unwrap();
    }
    let file = root.join("layouts.json");
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.clone();
    ed.layouts_path = Some(file.clone());
    ed.open(&root.join("a.txt")).unwrap();
    ed.workspace.test_run = Some(tests());
    // (the side panel open: `C-w < >` from the code size it — the chat is out of sight)
    ed.workspace.viewer.sidebar =
        Some(crate::sidebar::Sidebar::new(crate::sidebar::Mode::Files, root.clone()));
    // (Claude at work in follow mode, its chat out of sight: its card came on its own too — #299)
    let mut c = crate::chat::Chat::new();
    c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, "tidy the docs", ""));
    // (words enough for its full height, up to where the group floats)
    let words = "The docs say the same thing in three places; I keep the design page's words and point the others \
                 at it, then read each page again to be sure nothing it said is lost on the way.";
    for _ in 0..3 {
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Assistant, words, ""));
    }
    (c.hidden, c.follow.on, c.state) = (true, true, crate::chat::State::Writing);
    ed.workspace.claude.chat = Some(c);
    crate::term::render(&mut ed, &mut Vec::new(), 120, 40).unwrap();
    let world = World::default();
    let key = root.to_str().unwrap().to_string();
    let check = |ed: &Editor| held(ed, &world, &file, &key);
    sim.walk(&mut ed, INPUTS, |sim, ed| step(sim, ed, &world, &key), check, print);
    // Quiet: the newest asked for is what's there
    let newest = world.asked.borrow().last().map(|a| a.1.clone());
    let kept = crate::layouts::read(Some(&file)).get(&key).cloned();
    assert_eq!(kept, newest, "once quiet, the file holds the newest layout");
    // (…and its place for the tests group, the editor's now)
    if let Some(kept) = kept {
        let place = ed.workspace.group(crate::pane::GroupKind::Tests).map(|g| g.place.name());
        let tests =
            kept["groups"].as_array().and_then(|gs| gs.iter().find(|g| g["kind"] == "tests").cloned());
        assert_eq!(tests.map(|g| g["place"].clone()), place.map(Value::from), "the newest place wins");
    }
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// The layout the editor asked to have written last, if it's a new ask — after each event the scenario sends (one
/// step sends several) and after every step (a result landing is an event too).
fn record(ed: &Editor, world: &World, key: &str) {
    let last = world.asked.borrow().last().map_or(0, |a| a.0);
    if ed.layouts_seq != last
        && let Some(l) = ed.layouts.get(key)
    {
        world.asked.borrow_mut().push((ed.layouts_seq, l.clone()));
    }
}

fn keys(sim: &Sim, ed: &mut Editor, world: &World, key: &str, keys: &str) {
    let mut chars = keys.chars();
    while let Some(ch) = chars.next() {
        let k: Key = match ch {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            ch => Key::plain(Code::Char(ch)),
        };
        sim.batch(ed, Event::Key(k));
        record(ed, world, key);
    }
}

/// A finished run of two tests, one failed: its group lays out a list.
fn tests() -> crate::testing::TestRun {
    use crate::test_results::{Case, Status};
    let case =
        |name: &str, status| Case { name: name.into(), status, secs: None, message: Vec::new(), at: None };
    let cases = vec![case("adds", Status::Passed), case("subtracts", Status::Failed)];
    crate::testing::TestRun::finished("math", cases, vec!["test result: FAILED".into()])
}

fn step(sim: &mut Sim, ed: &mut Editor, world: &World, key: &str) -> String {
    // (by arm: a window key 6, another file in the pane 2, a frame 2, time passing 1, a docking or floating key from the
    // tests' list 3, the run ending or a new one 2, a card coming 1, a restart 1)
    match sim.choices.weighted(&[6, 2, 2, 1, 3, 2, 1, 1]) {
        0 => {
            let k = *sim.choices.pick(&[
                "<C-w>v",
                "<C-w>s",
                "<C-w>q",
                "<C-w>o",
                "<C-w>H",
                "<C-w>L",
                "<C-w>t",
                "<C-w>w",
                "<C-w>>",
                "<C-w><lt>",
                "<C-w>+",
                "<C-w>-",
                "<C-w>=",
            ]);
            keys(sim, ed, world, key, &format!("<esc>{k}"));
            format!("keys {k}")
        }
        1 => {
            let f = *sim.choices.pick(&["a.txt", "b.txt"]);
            let path = ed.workspace.root.join(f);
            keys(sim, ed, world, key, &format!("<esc>:open {}<ret>", path.display()));
            format!("{f} opened in the pane")
        }
        2 => {
            let w = *sim.choices.pick(&[60, 100, 160]);
            crate::term::render(ed, &mut Vec::new(), w, 40).unwrap();
            format!("a frame, {w} columns")
        }
        4 => {
            let k = *sim.choices.pick(&[
                "<C-w>L",
                "<C-w>H",
                "<C-w>K",
                "<C-w>J",
                "j",
                "<tab>",
                "<C-w>e",
                "<C-w>e",
                "<C-w>ml<ret>",
                "<C-w>mj",
                "q",
            ]);
            ed.list_focus(crate::pane::GroupKind::Tests);
            keys(sim, ed, world, key, k);
            format!("the tests' list, {k}")
        }
        5 => match ed.workspace.test_run.take() {
            Some(_) => {
                sim.batch(ed, Event::Resize);
                record(ed, world, key);
                "the run closed".into()
            }
            None => {
                ed.workspace.test_run = Some(tests());
                sim.batch(ed, Event::Resize);
                record(ed, world, key);
                "a new run".into()
            }
        },
        // (a card over it — its `n` the next key half the time; tall half the time, reaching up to where the group floats)
        6 => {
            let tall = (0..12).map(|i| format!("src/f{i}.rs not viewed.")).collect::<Vec<_>>().join("\n");
            let o = crate::offer::Offer::new(
                "End the review?".into(),
                if sim.choices.weighted(&[1, 1]) == 0 { "2 files not viewed." } else { &tall },
                "",
                String::new(),
                crate::offer::What::ReviewEnd {
                    session: crate::event::WorkspaceId::HOME,
                    started: std::time::Instant::now(),
                },
            );
            ed.push_offer(o);
            sim.batch(ed, Event::Resize);
            if sim.choices.weighted(&[1, 1]) == 0 {
                keys(sim, ed, world, key, "n");
            }
            "a card".into()
        }
        // (a restart as the layout has it: the tree from its file, the surfaces' shares as before any size was set, the
        // sizes from `sizes.json`'s text — then a frame at the same width)
        7 => {
            use crate::pane::Shows;
            let w = *sim.choices.pick(&[60, 100, 160]);
            crate::term::render(ed, &mut Vec::new(), w, 40).unwrap();
            let before = ed.screen.leaves.clone();
            let file = ed.workspace.root.join("sizes.json");
            std::fs::write(&file, ed.sizes.to_json()).unwrap();
            let json = crate::restart::split_json(&ed.workspace.split);
            let mut tree = crate::restart::split_of(&json).expect("the tree read back");
            for s in [Shows::Side, Shows::Rail, Shows::Slot, Shows::Chat] {
                if let Some(id) = ed.workspace.leaf(s) {
                    tree.set_share(id, crate::split::Share::Auto);
                }
            }
            ed.workspace.split = tree;
            ed.sizes = crate::sizes::Sizes::read(Some(&file));
            crate::term::render(ed, &mut Vec::new(), w, 40).unwrap();
            if ed.screen.leaves != before {
                let said =
                    format!("a restart laid {:?} where {before:?} were ({:?})", ed.screen.leaves, ed.sizes);
                *world.moved.borrow_mut() = Some(said);
            }
            let s = ed.sizes;
            format!("a restart, {w} columns, side {:?} slot {:?}", s.side, s.bottom)
        }
        _ => {
            let ms = *sim.choices.pick(&[100, 3000]);
            sim.wait(Duration::from_millis(ms));
            "time passes".into()
        }
    }
}

/// The file holds a layout asked for (each write a job of a later step than its ask), never older than one it held
/// before.
fn held(ed: &Editor, world: &World, file: &std::path::Path, key: &str) -> Result<(), String> {
    record(ed, world, key);
    if let Some(said) = world.moved.borrow_mut().take() {
        return Err(said);
    }
    docked_held(ed)?;
    fold_held(ed)?;
    follow_held(ed)?;
    let Some(kept) = crate::layouts::read(Some(file)).get(key).cloned() else { return Ok(()) };
    let asked = world.asked.borrow();
    let seqs: Vec<u64> = asked.iter().filter(|a| a.1 == kept).map(|a| a.0).collect();
    let Some(&newest) = seqs.last() else {
        return Err(format!("the file holds a layout never asked for: {kept}"));
    };
    // (the same layout asked again later — `C-w v` then `C-w q` — is several numbers: the file is at the earliest of
    // them no older than what it held before; only when all are older did it go back)
    let read = world.read.get();
    match seqs.iter().find(|&&s| s >= read) {
        Some(&seq) => world.read.set(seq),
        None => return Err(format!("the file went back to layout {newest} after {read}")),
    }
    Ok(())
}

/// A card that came on its own never covers a floating pane you opened (polish landing 1): drawn whole by the last frame,
/// unfocused and not yours, it meets no floating frame that frame drew — folded to its peeks there instead.
fn fold_held(ed: &Editor) -> Result<(), String> {
    let whole = (ed.screen.offer_buttons.iter()).any(|b| !b.3.is_ascii_digit() && b.3 != crate::mouse::WAY);
    // (waiting as that frame drew it — its way in on its bottom line: the keys may have left it since, a key with no
    // frame after it)
    let waiting = ed.screen.offer_buttons.iter().any(|b| b.3 == crate::mouse::WAY);
    let came = ed.offers.first().is_some_and(|o| !o.yours) && waiting;
    let Some((x, y, w, h)) = ed.screen.offer_card.filter(|_| whole && came) else { return Ok(()) };
    for (k, r) in ed.screen.floating.borrow().iter() {
        if x < r.x + r.w && r.x < x + w && y < r.y + r.h && r.y < y + h {
            return Err(format!(
                "a card that came on its own at {:?} covers {k:?}'s frame {r:?}",
                (x, y, w, h)
            ));
        }
    }
    Ok(())
}

/// The follow card came on its own too (#299): drawn by the last frame, it meets no floating frame that frame drew.
fn follow_held(ed: &Editor) -> Result<(), String> {
    let Some((x, y, w, h)) = ed.screen.thought_card.get() else { return Ok(()) };
    for (k, r) in ed.screen.floating.borrow().iter() {
        if x < r.x + r.w && r.x < x + w && y < r.y + r.h && r.y < y + h {
            return Err(format!("the follow card at {:?} covers {k:?}'s frame {r:?}", (x, y, w, h)));
        }
    }
    Ok(())
}

/// A docked group's leaf is in the tree exactly while its run is there, once, its pane with it; the slot's tabs never
/// list it while it's docked in sight; `Viewer.hand` names it only while the keys are in it.
fn docked_held(ed: &Editor) -> Result<(), String> {
    use crate::pane::{GroupKind, Place, Shows};
    let g = ed.workspace.group(GroupKind::Tests).ok_or("no tests group")?;
    let leaves = ed.workspace.split.leaves();
    let hosts: Vec<_> =
        ed.workspace.panes.iter().filter(|p| p.shows == Shows::Group(GroupKind::Tests)).collect();
    let in_tree = hosts.iter().filter(|p| leaves.contains(&p.id)).count();
    let docked = matches!(g.place, Place::Docked(_)) && ed.workspace.test_run.is_some();
    let floating = matches!(g.place, Place::Floating(_));
    // (floating: its own pane, out of the tree — kept while it floats, its run there or not)
    let want = (usize::from(docked), usize::from(docked || floating));
    if (in_tree, hosts.len()) != want {
        return Err(format!(
            "{in_tree} leaves and {} panes of the group, want {want:?} ({:?})",
            hosts.len(),
            g.place
        ));
    }
    if ed.out_of_slot(GroupKind::Tests) && ed.slot_tabs().iter().any(|t| t.slot == crate::sizes::Slot::Test) {
        return Err("the slot's tabs list the group out of the slot".into());
    }
    // (drawn floating: whole on the screen the last frame had)
    if let (Some(w), Some(h)) = (ed.screen.width, ed.screen.height) {
        for (k, r) in ed.screen.floating.borrow().iter() {
            if r.x + r.w > w || r.y + r.h > h {
                return Err(format!("{k:?}'s frame {r:?} past the screen {w}×{h}"));
            }
        }
    }
    if ed.float_moving.is_some_and(|k| !ed.floating_in_sight(k)) {
        return Err("C-w m moving a pane not floating in sight".into());
    }
    let keys = ed.workspace.is_at(crate::workspace::At::Slot)
        || ed.workspace.is_at(crate::workspace::At::List(g.list));
    // (a card over it: the hand stays under it)
    if ed.workspace.viewer.hand.is_some()
        && !keys
        && !matches!(ed.workspace.viewer.at, crate::workspace::At::Float(_))
    {
        return Err(format!(
            "the hand {:?} with the keys at {:?}",
            ed.workspace.viewer.hand, ed.workspace.viewer.at
        ));
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let place = ed.workspace.group(crate::pane::GroupKind::Tests).map(|g| g.place.name()).unwrap_or("-");
    format!("{} code panes · tests {place} · layout {}", ed.workspace.views.len(), ed.layouts_seq)
}
