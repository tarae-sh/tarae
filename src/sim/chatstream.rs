//! The chat's transcript as a document, simulated (panes P2c step 2): an answer being written word by word — now and
//! then a paragraph's end, a long run that wraps, a question and a new answer, the turn's end — normal mode's keys on
//! the transcript and the input's around them (`esc` in and out, moves, `x`, `v`, a search, an edit's keys, `enter`'s
//! quote, `i`, `C-w` between the code and the chat), the wheel over its rows, frames at three sizes, time passing.
//! After every step: the document is what the last frame laid out, line for line — nothing a key typed or deleted
//! stays; never left swapped in; while its view has the keys its cursor is in its rows, and text arriving moves
//! nothing in sight (the rows above the line being written, the cursor, the top), said `new text after`; after a
//! frame, the rows drawn are the document's.
//!
//! Claude's writing lands in the code beside it unwatched (§2 — a line rewritten, lines added, a line only taken
//! out): review at the end's landing (`refactor::landed`), and a run of Claude's that rewrote the file (`run_wrote`)
//! with the disk watch's reload later, as an event (`handle_event` → `wet_sync`) — to the run's text or another, your
//! edit, a tick, a frame or a key coming between; ticks come as the main loop's. A landing the buffer took, and a
//! reload to the run's text, leave the lines it wrote wet; a buffer waiting is at the version it waits on (after every
//! step, as after every event); its wet lines are inside the text, on a motion of their own, drawn as wet as that
//! motion's drying at its age; the wake is on a motion's own step, never further than one away; a tick past its time
//! lets it go with its lines; with none left nothing is asked — the wake ends.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use super::{Choices, Sim};
use crate::chat::{Msg, Role, State};
use crate::chatdoc::{self, Of};
use crate::editor::Editor;
use crate::event::{Mouse, MouseKind};
use crate::key::{Code, Key};

const SEEDS: &[u64] = &[0, 1, 2];

const INPUTS: usize = 70;

const LIMIT: Duration = crate::test_results::PATIENCE;

const SIZES: &[(u16, u16)] = &[(120, 30), (160, 36), (100, 24)];

#[test]
fn the_chats_transcript_follows_an_answer_being_written() {
    super::seeds("the_chats_transcript_follows_an_answer_being_written", SEEDS, run);
}

/// What's in sight of the transcript while its view has the keys: its top row, its selection (what arrives at it goes
/// under and after it: on the line being written too, it keeps its bytes), and the text of the rows in sight above
/// the line an answer grows at.
type Sight = (usize, Vec<(Place, Place)>, Vec<String>);

/// A place in the transcript as text arriving leaves it: up to the end of the line an answer grows at, its byte — it
/// stays; past it (the queued line and its air, after the answer — or, the turn over, the question it became) it goes
/// down with what arrives, kept in sight (`held`'s cursor check).
#[derive(Debug, PartialEq, Eq)]
enum Place {
    At(usize),
    Past,
}

impl Place {
    fn same(&self, now: &Place) -> bool {
        match self {
            Place::At(_) => self == now,
            Place::Past => true,
        }
    }
}

/// The line an answer grows at: the last message's last — the queued line, and its air, come after it.
fn growing_line(ed: &Editor) -> usize {
    let laid = ed.screen.chat_laid.borrow();
    let upto = laid.iter().rposition(|(of, _)| matches!(of, Of::Msg(_))).map_or(laid.len(), |l| l + 1);
    let lines: usize = laid[..upto]
        .iter()
        .filter(|(of, _)| *of != Of::Own)
        .map(|(_, rows)| chatdoc::lines_of(rows).0.lines().count())
        .sum();
    lines.saturating_sub(1)
}

/// What the scenario keeps between steps: what was in sight before text arrived while the view had the keys, and
/// the frame's size (the last one drawn, and whether this step drew it).
#[derive(Default)]
struct World {
    before: RefCell<Option<Sight>>,
    size: Cell<(u16, u16)>,
    framed: Cell<bool>,
    /// A tick served that left a motion past its time standing, or a write that left no wet lines where it should
    /// have (the next check says so).
    stuck: RefCell<Option<String>>,
    /// What a run of Claude's left on disk, the watch's reload not come yet: the buffer's text then, the run's.
    disk: RefCell<Option<(ropey::Rope, ropey::Rope)>>,
    /// The last disk stamp given (each write's its own).
    stamps: Cell<u64>,
}

/// The code's file (never on disk: its writes and reloads are the scenario's).
const FILE: &str = "/nowhere/sim/notes.txt";

/// `text` as Claude's write leaves it, `how`.
fn rewritten(text: &ropey::Rope, how: &str) -> ropey::Rope {
    let mut lines: Vec<String> = text.lines().filter(|l| l.len_bytes() > 0).map(|l| l.to_string()).collect();
    match how {
        "a line rewritten" if lines.is_empty() => lines.push("fn main() {}\n".into()),
        "a line rewritten" => lines[0] = "fn main() { run(); }\n".into(),
        "lines added" => lines.extend(["let x = 1;\n".into(), "let y = x;\n".into()]),
        _ => {
            lines.pop();
        }
    }
    ropey::Rope::from_str(&lines.concat())
}

/// A write landed in `ed`'s code over `before` (its text now `after`, if it took it): its lines wet — else said.
fn wet_as_written(ed: &Editor, before: &ropey::Rope, after: &ropey::Rope) -> Option<String> {
    let d = &ed.docs[0];
    let lines = crate::motion::written(before, after);
    if d.text != *after || lines.is_empty() {
        return None;
    }
    (d.wet.as_ref() != Some(&lines)).then(|| format!("a write landed, its lines {lines:?}, wet {:?}", d.wet))
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("the_chats_transcript_follows_an_answer_being_written", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.docs[0].text = ropey::Rope::from_str("fn main() {}\n");
    ed.docs[0].path = Some(FILE.into());
    let mut c = crate::chat::Chat::new();
    c.msgs = vec![
        Msg::new(Role::User, "why does insert clone the name?", ""),
        Msg::new(Role::Assistant, "Because the map keeps its own.\n\nThe needle is the key's owner.", ""),
        Msg::new(Role::Note, "3.1s", ""),
        Msg::new(Role::User, "and the second?", ""),
    ];
    let claude = crate::chat::stand_in(&mut c);
    ed.session.chat = Some(c);
    ed.motion_drawn = true;
    ed.session.at = crate::session::At::Chat;
    let world = World::default();
    world.size.set(SIZES[0]);
    frame(&mut ed, &world);
    let check = |ed: &Editor| {
        while claude.try_recv().is_ok() {}
        held(ed, &world)
    };
    sim.walk(&mut ed, INPUTS, |sim, ed| input(sim, ed, &world), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn frame(ed: &mut Editor, world: &World) {
    let (w, h) = world.size.get();
    crate::term::render(ed, &mut Vec::new(), w, h).unwrap();
    world.framed.set(true);
}

/// The text of the transcript's row `i` as drawn.
fn row_text(ed: &Editor, i: usize) -> Option<String> {
    let t = chatdoc::transcript(ed)?;
    let s = t.row(i)?;
    Some(t.pane.doc_in()?.text.byte_slice(s.start..s.start + s.len).to_string())
}

/// What's in sight while the view has the keys (`Sight`).
fn in_sight(ed: &Editor) -> Option<Sight> {
    if !chatdoc::has_keys(ed) {
        return None;
    }
    let t = chatdoc::transcript(ed)?;
    let d = t.pane.doc_in()?;
    let (_, _, area, _) = ed.screen.chat_view.get()?;
    let growing = growing_line(ed).min(crate::movement::last_line(&d.text));
    let top = t.top_row();
    let rows = (top..(top + area).min(t.rows()))
        .filter(|&i| t.row(i).is_some_and(|s| s.line < growing))
        .filter_map(|i| row_text(ed, i))
        .collect();
    let end = crate::movement::line_end(&d.text, growing);
    let place = |p: usize| if p <= end { Place::At(p) } else { Place::Past };
    let ranges = d.selection().ranges().iter().map(|r| (place(r.anchor), place(r.head))).collect();
    Some((top, ranges, rows))
}

fn keys(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(ch) = chars.next() {
        let k: Key = match ch {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            ch => Key::plain(Code::Char(ch)),
        };
        ed.handle_key(k);
    }
}

fn input(sim: &mut Sim, ed: &mut Editor, world: &World) -> String {
    world.framed.set(false);
    let c = &mut sim.choices;
    // (by arm: text arrives 6, a key 6, a question 1, the turn's end 1, the wheel 2, a frame 2, time passing 1, a write
    // landing in the code 2, a run rewriting it 1, the watch's reload 2, your edit in the code 1, a tick 3, a short
    // while 2)
    let said = match c.weighted(&[6, 6, 1, 1, 2, 2, 1, 2, 1, 2, 1, 3, 2]) {
        0 => {
            let words = *c.pick(&[
                "word ",
                "another needle ",
                "\n\n",
                "a much longer run of words that wraps across the chat's rows wherever it is narrow enough ",
                "한글 넓은 글자 ",
            ]);
            *world.before.borrow_mut() = in_sight(ed);
            let Some(c) = ed.session.chat.as_mut() else { return "(no chat)".into() };
            if c.msgs.last().is_none_or(|m| m.role != Role::Assistant) || c.state == State::Idle {
                c.msgs.push(Msg::new(Role::Assistant, "", ""));
            }
            c.state = State::Writing;
            let m = c.msgs.last_mut().expect("an answer, above");
            m.text.push_str(words);
            m.changed();
            frame(ed, world);
            format!("{words:?} arrives")
        }
        1 => {
            let k = *c.pick(&[
                "<esc>",
                "<esc>",
                "i",
                "j",
                "k",
                "gg",
                "G",
                "x",
                "w",
                "vjj",
                "/needle<ret>",
                "n",
                "<C-d>",
                "<C-u>",
                "d",
                "u",
                "<ret>",
                "<C-w>h",
                "<C-w>l",
            ]);
            keys(ed, k);
            ed.focus_sync();
            format!("keys {k}")
        }
        2 => {
            let Some(c) = ed.session.chat.as_mut() else { return "(no chat)".into() };
            c.msgs.push(Msg::new(Role::User, "and then?", ""));
            c.msgs.push(Msg::new(Role::Assistant, "", ""));
            c.state = State::Thinking;
            "a question, its answer begun".into()
        }
        3 => {
            crate::chat::stand_in_done(ed);
            "the turn ends".into()
        }
        4 => {
            let up = c.below(2) == 0;
            let (Some(x), Some((y, _, area, _))) = (ed.screen.chat_x, ed.screen.chat_view.get()) else {
                return "(no chat drawn: no wheel over it)".into();
            };
            let row = c.below(area.max(1));
            let kind = if up { MouseKind::ScrollUp } else { MouseKind::ScrollDown };
            ed.handle_mouse(Mouse { kind, x: (x + 4) as u16, y: (y + row) as u16, alt: false, ctrl: false });
            ed.focus_sync();
            format!("the wheel {} over row {row}", if up { "up" } else { "down" })
        }
        5 => {
            let size = *c.pick(SIZES);
            world.size.set(size);
            frame(ed, world);
            format!("a frame, {}×{}", size.0, size.1)
        }
        6 => {
            let ms = *c.pick(&[100, 3000]);
            sim.wait(Duration::from_millis(ms));
            "time passes".into()
        }
        7 => {
            // (§2: Claude's write lands in the code unwatched — review at the end's: the buffer takes it unless
            // you've edited it, its lines wet)
            let how = *c.pick(&["a line rewritten", "lines added", "a line taken out"]);
            let before = ed.docs[0].text.clone();
            let after = rewritten(&before, how);
            let f = crate::refactor::FileChange {
                path: FILE.into(),
                rel: "notes.txt".into(),
                before: before.clone(),
                after: after.clone(),
                places: 1,
                op: crate::refactor::Op::Text,
                protected: false,
            };
            let generation = ed.session.chat.as_ref().map_or(0, |c| c.generation());
            let whose = crate::refactor::Whose::Claude(generation);
            crate::refactor::landed(ed, &f, Some(stamp(world)), whose, None);
            *world.stuck.borrow_mut() = wet_as_written(ed, &before, &after);
            let left = if ed.docs[0].text == after { "" } else { " (not taken: your edits)" };
            format!("Claude's write lands: {how}{left}")
        }
        8 => {
            // (§2: a run of Claude's rewrote the file — its result now, the watch's reload later)
            let how = *c.pick(&["a line rewritten", "lines added", "a line taken out"]);
            let before = ed.docs[0].text.clone();
            let after = rewritten(&before, how);
            let blob = |t: &ropey::Rope| {
                Some(crate::snapshot::Blob { id: String::new(), text: Some(t.to_string()) })
            };
            let change =
                crate::snapshot::Change { path: FILE.into(), before: blob(&before), after: blob(&after) };
            ed.run_wrote(&[change]);
            *world.disk.borrow_mut() = Some((before, after));
            format!("a run of Claude's rewrote it: {how}")
        }
        9 => {
            // (the disk watch's reload, an event as the loop takes it: to the run's text, or another written since)
            let other = c.below(4) == 0;
            let Some((before, run)) = world.disk.borrow_mut().take() else {
                return "(nothing on disk to reload)".into();
            };
            let to = if other { rewritten(&run, "lines added") } else { run.clone() };
            // (still waiting: nothing of yours or another write came between)
            let waiting = ed.wet_waiting.iter().any(|w| w.doc == ed.docs[0].id);
            let (took, at) = (to.clone(), stamp(world));
            let reload = move |ed: &mut Editor| ed.disk_take(std::path::Path::new(FILE), &took, Some(at));
            ed.handle_event(crate::event::Event::Job(Box::new(reload)));
            if waiting && !other {
                *world.stuck.borrow_mut() = wet_as_written(ed, &before, &run);
            }
            let left = if ed.docs[0].text == to { "" } else { " (not taken: your edits)" };
            let waited = if waiting { " (waited on)" } else { "" };
            format!("the watch reloads it{}{waited}{left}", if other { ", another text" } else { "" })
        }
        10 => {
            // (your edit in the code, as a key there makes it — or, edited, your save: its text the file's again)
            if ed.docs[0].is_modified() && c.below(2) == 0 {
                ed.docs[0].mark_saved();
                return "you save the code".into();
            }
            let typed = crate::transaction::Change { from: 0, to: 0, insert: "// ".into() };
            ed.docs[0].apply(&crate::transaction::Transaction::new(vec![typed]));
            "you type in the code".into()
        }
        11 => {
            // (as the main loop's wake: its tick, then a frame — one past a motion's time lets it go)
            let over = |ed: &Editor| {
                ed.motions
                    .iter()
                    .find(|m| crate::clock::since(m.born) >= crate::motion::shown_for(m.life))
                    .cloned()
            };
            ed.schedule_tick();
            frame(ed, world);
            let bare = ed.motions.iter().any(|m| {
                matches!(m.what, crate::motion::What::Wet(id) if ed.docs.iter().any(|d| d.id == id && d.wet.is_none()))
            });
            if let Some(m) = over(ed) {
                *world.stuck.borrow_mut() =
                    Some(format!("a tick left a motion {:?} old", crate::clock::since(m.born)));
            } else if bare {
                *world.stuck.borrow_mut() = Some("a tick left a drying whose lines went".into());
            }
            "a tick".into()
        }
        _ => {
            let ms = *c.pick(&[30, 100, 102, 250, 1000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
    };
    // (as the main loop after every event — the keys here are handed to `handle_key` itself)
    crate::motion::wet_sync(ed);
    said
}

/// A disk stamp of its own (each write's, so the buffer takes it — `disk_take`).
fn stamp(world: &World) -> crate::disk::Stamp {
    let n = world.stamps.get() + 1;
    world.stamps.set(n);
    (std::time::UNIX_EPOCH + Duration::from_secs(n), n)
}

/// §2's drying: wet lines inside the text, each document's on a motion of its own and drawn as wet as its drying at
/// its age; the wake on a motion's own step, at most one away (now for one over); none with none left.
fn wet_held(ed: &Editor, world: &World) -> Result<(), String> {
    use crate::motion::{What, shown_for, strength};
    if let Some(s) = world.stuck.borrow_mut().take() {
        return Err(s);
    }
    // (a buffer waiting on a run's text is at the version it waits on: one moved was looked at)
    for w in &ed.wet_waiting {
        if !ed.docs.iter().any(|d| d.id == w.doc && d.version() == w.version) {
            return Err(format!("a buffer waiting on a run's text at version {}, moved on", w.version));
        }
    }
    for d in &ed.docs {
        let m = ed.motions.iter().find(|m| m.what == What::Wet(d.id));
        // (one whose lines went — an undo — draws nothing, and the next tick lets it go)
        let Some(lines) = &d.wet else { continue };
        let Some(m) = m else { return Err(format!("wet lines {lines:?} with no motion")) };
        let n = d.text.len_bytes();
        if lines.iter().any(|r| r.end > n || r.is_empty()) || lines.windows(2).any(|w| w[0].end > w[1].start)
        {
            return Err(format!("wet lines {lines:?} outside a text of {n} bytes, or out of order"));
        }
        let k = strength(crate::clock::since(m.born), m.life);
        let drawn = ed.wet_on(d).map(|(_, k)| k);
        let want = (k > 0.0).then_some(k);
        if drawn != want {
            return Err(format!("wet lines drawn {drawn:?}, their drying at its age {want:?}"));
        }
    }
    let wake = ed.motion_wake();
    let Some(w) = wake else {
        return match ed.motions.is_empty() {
            true => Ok(()),
            false => Err("a motion under way, and no wake asked".into()),
        };
    };
    if ed.motions.is_empty() {
        return Err(format!("no motion left, a wake asked in {w:?}"));
    }
    let slack = Duration::from_millis(2);
    let on_a_step = ed.motions.iter().any(|m| {
        let age = crate::clock::since(m.born);
        let over = age >= shown_for(m.life);
        (over && w.is_zero())
            || (!over
                && (age + w).saturating_sub(slack).as_nanos().is_multiple_of(crate::editor::TICK.as_nanos()))
    });
    if !on_a_step || w > crate::editor::TICK + slack {
        return Err(format!("a wake in {w:?}, on no motion's own step"));
    }
    Ok(())
}

fn held(ed: &Editor, world: &World) -> Result<(), String> {
    wet_held(ed, world)?;
    if crate::docpane::swapped(ed).is_some() {
        return Err("a document left swapped in".into());
    }
    let Some(t) = chatdoc::transcript(ed) else { return Ok(()) };
    let d = t.pane.doc_in().ok_or("the transcript's document is out")?;
    // What the last frame laid out, line for line — nothing a key typed or deleted stays (read-only)
    let laid = ed.screen.chat_laid.borrow();
    if !laid.is_empty() {
        let want: String =
            laid.iter().filter(|(of, _)| *of != Of::Own).map(|(_, rows)| chatdoc::lines_of(rows).0).collect();
        if d.text != want.as_str() {
            return Err(format!(
                "the transcript's {} lines aren't the {} laid out",
                d.text.len_lines(),
                want.lines().count()
            ));
        }
    }
    if let Some(before) = world.before.borrow_mut().take()
        && let Some(now) = in_sight(ed)
    {
        // (a cursor past the line an answer grows at — on the queued line — goes down with it, kept in sight)
        let below = matches!(before.1.first(), Some((_, Place::Past)));
        let same = before.1.len() == now.1.len()
            && before.1.iter().zip(&now.1).all(|(a, b)| a.0.same(&b.0) && a.1.same(&b.1));
        if (!below && before.0 != now.0) || !same {
            return Err(format!(
                "text arriving moved the view: top {} → {}, {:?} → {:?}",
                before.0, now.0, before.1, now.1
            ));
        }
        if let Some(i) =
            (0..before.2.len().min(now.2.len())).filter(|_| !below).find(|&i| before.2[i] != now.2[i])
        {
            return Err(format!("text arriving changed a row in sight: {:?} → {:?}", before.2[i], now.2[i]));
        }
        if !chatdoc::came(ed) {
            return Err("text arrived and the view doesn't say so".into());
        }
    }
    if !chatdoc::has_keys(ed) {
        return Ok(());
    }
    let Some((y, _, area, from)) = ed.screen.chat_view.get() else { return Ok(()) };
    let g = d.rows.as_deref().ok_or("a transcript with no rows")?;
    let cursor = d.selection().primary().cursor(&d.text);
    let line = crate::movement::line_of(&d.text, cursor);
    let at = g.index(line, crate::wrap::row_of(g.of(line), cursor));
    let top = t.top_row();
    if at < top || at >= top + area {
        return Err(format!("the cursor's row {at} out of the rows from {top} ({area})"));
    }
    // (after a frame: the rows drawn are the document's, from its top)
    let (Some((frame, w, h)), Some(x)) = (ed.last_frame.as_ref(), ed.screen.chat_x) else { return Ok(()) };
    if !world.framed.get() || (*w, *h) != world.size.get() {
        return Ok(());
    }
    // (the frame's rows: the document's, the frame's own among them — `thinking…` before the queued line — skipped)
    let first = chatdoc::laid_row(&laid, top);
    if from != first {
        return Err(format!("drawn from row {from}, the view's top is {top} (row {first} of the frame's)"));
    }
    let screen = crate::screen::text(frame, *w, *h).unwrap_or_default();
    for (i, shown) in screen.lines().skip(y).take(area).enumerate() {
        let Some(row) = chatdoc::doc_row(&laid, first + i) else { continue };
        let Some(want) = row_text(ed, row) else { break };
        let shown: String = shown.chars().skip(x).collect();
        let want: String = want.trim().chars().take(16).collect();
        if !shown.contains(&want) {
            return Err(format!("row {i}: drawn {shown:?}, the document's row {row} {want:?}"));
        }
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let keys = ed.keys_to().region.name();
    let doc = chatdoc::transcript(ed).and_then(|t| {
        let d = t.pane.doc_in()?;
        let line = crate::movement::line_of(&d.text, d.selection().primary().cursor(&d.text));
        Some(format!(
            "transcript at {}/{} from row {}",
            line + 1,
            crate::movement::last_line(&d.text) + 1,
            t.top_row()
        ))
    });
    let c = ed.session.chat.as_ref();
    format!(
        "keys {keys} · {} · {} messages, {:?}",
        doc.unwrap_or_else(|| "no transcript".into()),
        c.map_or(0, |c| c.msgs.len()),
        c.map(|c| c.state)
    )
}
