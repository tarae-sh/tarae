//! M0 of the mouse audit (`.sb/mouse-plan.md` §3): the walk. Every scene of the keys' routing test and the mouse's
//! own, one cell per distinct thing under the pointer on its frame, every act — held to `handle_mouse`'s routing
//! (`took_mouse` = `mouse_under`), the frame's coverage (every drawn cell is under something) and the table (`expect`:
//! what acts acts, what says says, what doesn't does neither).

use std::sync::OnceLock;

use crate::editor::{Drag, Editor};
use crate::event::{Event, Mouse, MouseKind as K};
use crate::mouse::{ACTS, Act, Expect, Part, SILENT_STATES, Surface, TOOK, Under, doc, expect, rows};
use crate::uistate::tests::{SCENES, Scene, chat, editor, frame};

const W: usize = 120;
const H: usize = 40;

fn feed(ed: &mut Editor, keys: &[&str]) {
    for k in keys {
        let key = match k.chars().count() {
            1 => crate::key::Key::plain(crate::key::Code::Char(k.chars().next().unwrap())),
            _ => k.trim_matches(['<', '>']).parse().unwrap(),
        };
        ed.handle_key(key);
    }
}

fn long_text(ed: &mut Editor) {
    ed.docs[0].text =
        ropey::Rope::from_str(&(0..60).map(|i| format!("let line_{i} = {i};\n")).collect::<String>());
}

/// The mouse's own scenes (§3), beside the routing test's: what only the mouse meets.
const MOUSE_SCENES: &[Scene] = &[
    ("a split side by side", |ed| {
        long_text(ed);
        feed(ed, &["<C-w>", "v"]);
    }),
    ("a split over and under", |ed| {
        long_text(ed);
        feed(ed, &["<C-w>", "s"]);
    }),
    ("the start screen", |ed| ed.docs[0].text = ropey::Rope::new()),
    ("a picker with a preview", |ed| {
        long_text(ed);
        let id = ed.doc().id;
        let item = |label: &str| crate::picker::Item {
            label: label.into(),
            action: crate::picker::Action::Buffer(id),
            hint: String::new(),
            glyph: None,
        };
        let p = crate::picker::Picker::of(
            "buffers",
            "buffers",
            vec![item("one"), item("two"), item("three")],
            false,
        );
        ed.open_picker(p, None);
    }),
    ("the command line, its completions", |ed| feed(ed, &[":", "w"])),
    ("the session list, floating", |ed| feed(ed, &[" ", "W"])),
    ("the chat beside a long file", |ed| {
        long_text(ed);
        chat(ed, "a draft");
        let c = ed.workspace.claude.chat.as_mut().unwrap();
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, "a question", ""));
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Assistant, "An answer, in a few words.", ""));
    }),
    // (what the wheel has room to scroll on each surface: `SCROLLS`)
    ("a long conversation", |ed| {
        chat(ed, "");
        let c = ed.workspace.claude.chat.as_mut().unwrap();
        for i in 0..30 {
            c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, format!("question {i}"), ""));
            c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Assistant, format!("answer {i}"), ""));
        }
    }),
    ("a picker of many rows, its preview long", |ed| {
        long_text(ed);
        let id = ed.doc().id;
        let items = (0..80)
            .map(|i| crate::picker::Item {
                label: format!("row {i}"),
                action: crate::picker::Action::Buffer(id),
                hint: String::new(),
                glyph: None,
            })
            .collect();
        ed.open_picker(crate::picker::Picker::of("buffers", "buffers", items, false), None);
    }),
    ("the messages list, long", |ed| {
        for i in 0..60 {
            ed.set_status(format!("message {i}"));
        }
        ed.messages_open(None);
    }),
    ("the side panel, many files", |ed| {
        let files = (0..80).map(|i| format!("f{i:02}.rs")).collect();
        ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::with_files(std::env::temp_dir(), files));
    }),
    ("a long test output", |ed| {
        let lines = (0..200).map(|i| format!("output line {i}")).collect();
        ed.workspace.test_run = Some(crate::testing::TestRun::finished("t", Vec::new(), lines));
    }),
    ("hover text, long", |ed| {
        let md: String = (0..80).map(|i| format!("line {i} of the docs\n\n")).collect();
        ed.popup = Some(crate::markdown::render(&md, &ed.theme, None));
    }),
];

/// Scenes made with more than a screen of a surface (by its name in `Surface`'s `Debug`): its wheel has room to scroll,
/// so there it must act — elsewhere a wheel that does nothing may be one with nothing to scroll. (The wheel's room isn't
/// set up for the rail, the terminal, a plugin's panel, a floating pane, a group's list or the edit review yet: S's.)
const SCROLLS: &[(&str, &str)] = &[
    ("a split side by side", "Code"),
    ("a split over and under", "Code"),
    ("the chat beside a long file", "Code"),
    ("a long conversation", "Transcript"),
    ("a picker of many rows, its preview long", "Picker"),
    ("a picker of many rows, its preview long", "PickerPreview"),
    ("the messages list, long", "Floatlist"),
    ("the side panel, many files", "Side"),
    ("a long test output", "Output"),
    ("hover text, long", "Popup"),
];

fn shown(ed: &mut Editor) -> String {
    frame(ed);
    crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), W as u16, H as u16).unwrap()
}

/// What an act can change: the frame as drawn (its styles too: a selection, a lit line), where the keys go, the code's
/// selection and the edit review's; and what was said.
#[derive(Clone)]
struct Snap {
    frame: Vec<u8>,
    /// The frame as text — what a failure names of what changed.
    text: String,
    keys: String,
    sel: String,
    said: u64,
}

impl PartialEq for Snap {
    fn eq(&self, o: &Snap) -> bool {
        (&self.frame, &self.keys, &self.sel, self.said) == (&o.frame, &o.keys, &o.sel, o.said)
    }
}

/// What changed between two snaps, for a failure: where the keys went, the first rows of text that differ.
fn changed(a: &Snap, b: &Snap) -> String {
    let mut out = Vec::new();
    if a.keys != b.keys {
        out.push(format!("keys {} → {}", a.keys, b.keys));
    }
    if a.sel != b.sel {
        out.push(format!("selection {} → {}", a.sel, b.sel));
    }
    let rows = a.text.lines().zip(b.text.lines()).enumerate().filter(|(_, (x, y))| x != y);
    for (i, (x, y)) in rows.take(2) {
        out.push(format!("row {i}: {:?} → {:?}", x.trim_end(), y.trim_end()));
    }
    if out.is_empty() && a.frame != b.frame {
        out.push("styles only".into());
    }
    out.join("; ")
}

fn snap(ed: &mut Editor) -> Snap {
    // (every mouse event drops the status line's old word first — not an act of the surface pressed; a second frame for
    // what settles once a frame drew it)
    ed.status = None;
    frame(ed);
    frame(ed);
    let keys = format!("{:?} {:?}", ed.keys_to().region, ed.mode);
    // (the edit review's too: it has no cursor, its selection a ground only — tests draw no colours)
    let review = ed.workspace.claude.review.as_ref().and_then(|r| r.marked);
    let mut sel = format!("{:?} {:?} {review:?}", ed.doc().id, ed.doc().selection());
    // (and every text surface's own document: a double there selects in it)
    use crate::sizes::Slot;
    for on in [
        Drag::Transcript,
        Drag::Input,
        Drag::Output(Slot::Test),
        Drag::Output(Slot::Task),
        Drag::Output(Slot::Procs),
        Drag::TerminalView,
    ] {
        if let Some(d) = crate::mouse::text_doc(ed, on) {
            sel += &format!(" {on:?} {:?}", d.selection());
        }
    }
    let frame = ed.last_frame.as_ref().unwrap().0.clone();
    let text = crate::screen::text(&frame, W as u16, H as u16).unwrap_or_default();
    Snap { frame, text, keys, sel, said: ed.said }
}

/// What an act did between two snaps. A hover acts only where it moves the keys or a selection —
/// what it shows (a cut row whole, a mark's words) is what it is, not an act; in a scene whose frame moves by itself
/// (a draft being written), neither is the frame.
fn did(a: &Snap, b: &Snap, act: Act) -> Did {
    let moved = a.keys != b.keys || a.sel != b.sel;
    let drawn = act != Act::Hover && !MOVING.get() && a.frame != b.frame;
    Did { acted: moved || drawn, said: a.said != b.said, selected: a.sel != b.sel, what: changed(a, b) }
}

/// What an act did (`did`): acted, said, changed a selection (the code's, the edit review's or a text surface's), and
/// what changed in words.
struct Did {
    acted: bool,
    said: bool,
    selected: bool,
    what: String,
}

thread_local!(static MOVING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });

/// Whether the keys are already where a press on `u` gives them — a press there with nothing else to do is no silence
/// (a press gives the keys to the surface it lands on: given, it's done).
fn holds(ed: &Editor, u: Under) -> bool {
    use crate::uistate::Region as R;
    let r = ed.keys_to().region;
    match u.surface {
        Surface::Code(p) | Surface::PaneTop(p) => {
            matches!(r, R::Code | R::Weave | R::Page) && ed.workspace.viewer.focus == p
        }
        Surface::Chat | Surface::Transcript | Surface::Input => matches!(r, R::Chat | R::View(_)),
        Surface::Side => matches!(r, R::Side(_)) || ed.workspace.is_at(crate::workspace::At::Side),
        Surface::Rail => r == R::Rail,
        Surface::Card => matches!(r, R::Card(_)),
        Surface::EditReview => r == R::EditReview,
        Surface::Picker | Surface::PickerPreview => r == R::Picker,
        Surface::Prompt => r == R::Prompt,
        Surface::Output(_) | Surface::List(_) | Surface::Plugin => {
            matches!(r, R::Output(_) | R::Slot(_) | R::List(_))
        }
        Surface::Floatlist(k) | Surface::Float(k) => {
            r == R::List(k) || matches!(r, R::Output(_) | R::Slot(_))
        }
        Surface::Terminal | Surface::TerminalView => matches!(r, R::Program | R::View(_)),
        Surface::SlotLine => {
            let tab = matches!(u.part, Part::Word(crate::mouse::Word::Tab(s)) if ed.slot() == Some(s));
            tab || matches!(r, R::Output(_) | R::Slot(_) | R::List(_))
        }
        _ => false,
    }
}

/// A cell to drag to from (`x`, `y`), on the same surface as `u` where one is near — a drag from a surface onto another
/// is the other's act, not this one's (what a press holds — a border, a frame — goes three on and one down).
fn drag_to(ed: &Editor, u: Under, x: usize, y: usize) -> (usize, usize) {
    let far = ((x + 3).min(W - 1), (y + 1).min(H - 1));
    // (a line held: toward the code — a panel's edge, the slot's top line up)
    if matches!(u.part, Part::Border(_) | Part::Frame(_)) {
        return (x.saturating_sub(3), y.saturating_sub(1));
    }
    let near = [(3, 0), (-3, 0), (3, 1), (0, 1), (0, -1), (1, 0), (-1, 0)];
    near.iter()
        .filter_map(|&(dx, dy)| Some((x.checked_add_signed(dx)?, y.checked_add_signed(dy)?)))
        .filter(|&(cx, cy)| cx < W && cy < H)
        .find(|&(cx, cy)| ed.mouse_at(cx, cy).surface == u.surface)
        .unwrap_or(far)
}

fn part_kind(p: Part) -> String {
    let head = |d: String| d.split(['(', ' ', '{']).next().unwrap_or_default().to_string();
    match p {
        Part::Word(w) => format!("Word({})", head(format!("{w:?}"))),
        Part::Frame(g) => format!("Frame({})", head(format!("{g:?}"))),
        _ => head(format!("{p:?}")),
    }
}

fn name(u: Under) -> String {
    format!("{:?}/{}", u.surface, part_kind(u.part))
}

/// A cell and what a press there lands on.
type Target = ((usize, usize), Under);

/// One cell per distinct thing under a press on the frame — the middle one of its cells, of those drawn where any is
/// (a drag from text, not from the blank past it) — and the non-blank cells under nothing.
fn targets(ed: &Editor, text: &str) -> (Vec<Target>, Vec<(usize, usize)>) {
    let rows: Vec<Vec<char>> = text.lines().map(|l| l.chars().collect()).collect();
    // (each kind: its name, its cells, those of them drawn)
    let mut kinds: Vec<(String, Vec<Target>, Vec<Target>)> = Vec::new();
    let mut bare = Vec::new();
    for y in 0..H {
        for x in 0..W {
            let u = ed.mouse_at(x, y);
            let drawn = rows.get(y).and_then(|r| r.get(x)).is_some_and(|c| !c.is_whitespace());
            if u.surface == Surface::Nothing {
                if drawn {
                    bare.push((x, y));
                }
                continue;
            }
            let key = name(u);
            let k = match kinds.iter().position(|k| k.0 == key) {
                Some(i) => &mut kinds[i],
                None => {
                    kinds.push((key, Vec::new(), Vec::new()));
                    kinds.last_mut().unwrap()
                }
            };
            k.1.push(((x, y), u));
            if drawn {
                k.2.push(((x, y), u));
            }
        }
    }
    let pick = |(_, all, drawn): (String, Vec<_>, Vec<_>)| {
        let cells = if drawn.is_empty() { all } else { drawn };
        cells[cells.len() / 2]
    };
    (kinds.into_iter().map(pick).collect(), bare)
}

#[derive(Default)]
struct Walk {
    /// `scene · surface/part · act: …` where the table and the act disagree.
    off: Vec<String>,
    /// `scene · event at x,y: mouse_under says X, handle_mouse took Y`.
    routed: Vec<String>,
    /// `scene: N cells under nothing, first at x,y`.
    bare: Vec<String>,
    /// Every surface found, by its name.
    found: Vec<String>,
    /// `scene · surface` of `SCROLLS` whose wheel was judged with room.
    scrolled: Vec<String>,
    events: usize,
}

/// `kind` at (`x`, `y`), checked against `mouse_under`: the branch that took it is the one it named.
fn send(ed: &mut Editor, kind: K, x: usize, y: usize, scene: &str, walk: &mut Walk) {
    let said = ed.mouse_under(kind, x, y).surface;
    TOOK.set(None);
    ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
    walk.events += 1;
    let took = TOOK.get();
    if took != Some(said) {
        walk.routed.push(format!(
            "{scene} · {kind:?} at {x},{y}: mouse_under says {said:?}, handle_mouse took {took:?}"
        ));
    }
}

fn fresh(setup: fn(&mut Editor)) -> Editor {
    let mut ed = editor();
    setup(&mut ed);
    frame(&mut ed);
    ed
}

/// How the scene stood before a press: the keys already where the press gives them (`holds`), a focused card letting
/// go of them (`mouse::let_go`: the press says so and goes on).
#[derive(Clone, Copy, Default)]
struct Before {
    holds: bool,
    lets_go: bool,
    /// No text where it was pressed to drag over: the chat's input empty, its conversation with no message, the
    /// prompt's line empty, the terminal's view blank.
    empty: bool,
    /// More than a screen of the surface under the wheel (`SCROLLS`): it must scroll.
    room: bool,
}

fn judge(
    walk: &mut Walk,
    scene: &str,
    u: Under,
    act: Act,
    before: Before,
    Did { acted, said, selected, what }: Did,
) {
    let e = expect(u.surface, u.part, act);
    let quiet = !acted && !said;
    let pressed = matches!(act, Act::Press | Act::Double);
    // (a state the table can't hold, by surface: a press there silent until its landing — `SILENT_STATES`)
    let state = SILENT_STATES.iter().any(|s| scene.contains(s.0));
    let ok = state && pressed && quiet
        // (a scene whose frame moves by itself: an act that only draws isn't told from it)
        || MOVING.get() && quiet && e == Expect::Acts
        || match e {
        // (the wheel where there may be nothing to scroll: silent too — where there's room, it must act)
        // (a double on text: its word selected — drawn alone doesn't do, tests draw no colours)
        Expect::Acts if act == Act::Double && matches!(u.part, Part::Text { .. }) => selected || before.empty,
        Expect::Acts => {
            acted
                || (act == Act::Wheel && !before.room)
                || (pressed && quiet && before.holds)
                || (act == Act::Drag && quiet && before.empty)
        }
        Expect::Says(_) => said,
        // (a double is one press: past what the first did, the second changes nothing — it may say its word again)
        Expect::Again => !acted,
            Expect::Not(_) | Expect::Gap(..) => quiet || (pressed && before.lets_go && acted && said),
        };
    if !ok {
        let got = match (acted, said) {
            (true, true) => "acted and said",
            (true, false) => "acted",
            (false, true) => "said",
            (false, false) => "nothing",
        };
        walk.off.push(format!("{scene} · {} · {act:?}: expected {e:?}, got {got} ({what})", name(u)));
    }
}

fn before(ed: &Editor, u: Under) -> Before {
    let modal = matches!(u.surface, Surface::Card | Surface::Prompt) || u.part == Part::Outside;
    let chat = ed.workspace.claude.chat.as_ref();
    let empty = match u.surface {
        Surface::Input => chat.is_some_and(|c| c.input.is_empty()),
        Surface::Transcript => chat.is_some_and(|c| c.msgs.is_empty()),
        Surface::Prompt => ed.prompt.as_ref().is_some_and(|p| p.input.is_empty()),
        Surface::Picker if matches!(u.part, Part::Text { .. }) => {
            ed.picker.as_ref().is_some_and(|p| p.input.is_empty())
        }
        // (a program that printed nothing: its view's rows blank — a drag over them selects their line ends only)
        Surface::TerminalView => {
            crate::view::terminal_view_of(ed).is_some_and(|v| v.doc().text.chars().all(char::is_whitespace))
        }
        _ => false,
    };
    Before { holds: holds(ed, u), lets_go: ed.card_has_keys() && !modal, empty, room: false }
}

fn walk_scene(walk: &mut Walk, n: usize, (scene, setup): Scene) {
    // (one instant for the whole scene: a double is two presses at it, an age drawn never moves between two frames)
    crate::clock::hold();
    walk_held(walk, n, scene, setup);
    crate::clock::release();
}

/// One part of each surface (§3's cost: the whole walk took 38 s) — the `n`th scene its `n`th part, round, so every part
/// is met across the scenes.
fn sample(targets: Vec<Target>, n: usize) -> Vec<Target> {
    let mut surfaces: Vec<Surface> = Vec::new();
    for t in &targets {
        if !surfaces.contains(&t.1.surface) {
            surfaces.push(t.1.surface);
        }
    }
    let of = |s: Surface| targets.iter().filter(move |t| t.1.surface == s).copied().collect::<Vec<_>>();
    surfaces.into_iter().map(|s| of(s)[n % of(s).len()]).collect()
}

fn walk_held(walk: &mut Walk, n: usize, scene: &str, setup: fn(&mut Editor)) {
    let mut ed = fresh(setup);
    // (a scene whose frame moves by itself: what arrives on the stand-in Claude's stream, a draft revealed as it comes)
    MOVING.set(scene.contains("being written") || snap(&mut ed) != snap(&mut ed));
    let text = shown(&mut ed);
    let (targets, bare) = targets(&ed, &text);
    if let Some(&(x, y)) = bare.first() {
        walk.bare.push(format!("{scene}: {} cells under nothing, first at {x},{y}", bare.len()));
    }
    for &((x, y), u) in &sample(targets, n) {
        let found = format!("{:?}", u.surface);
        let found = found.split('(').next().unwrap_or_default().to_string();
        if !walk.found.contains(&found) {
            walk.found.push(found);
        }
        // A press, its release, and a second press at the same instant (a double)
        let mut ed = fresh(setup);
        let s0 = snap(&mut ed);
        let b0 = before(&ed, u);
        let front = |ed: &Editor| ed.offers.first().map(|o| o.id);
        let card = front(&ed);
        send(&mut ed, K::Down, x, y, scene, walk);
        let s1 = snap(&mut ed);
        send(&mut ed, K::Up, x, y, scene, walk);
        let s2 = snap(&mut ed);
        // (the double is on what the press there is now — the first may have moved the screen, or answered a card and
        // brought the next one under the pointer: another card's press)
        let again = ed.mouse_at(x, y) == u && front(&ed) == card;
        let b2 = before(&ed, u);
        send(&mut ed, K::Down, x, y, scene, walk);
        send(&mut ed, K::Up, x, y, scene, walk);
        let s3 = snap(&mut ed);
        judge(walk, scene, u, Act::Press, b0, did(&s0, &s1, Act::Press));
        judge(walk, scene, u, Act::Release, Before::default(), did(&s1, &s2, Act::Release));
        if again {
            judge(walk, scene, u, Act::Double, b2, did(&s2, &s3, Act::Double));
        }
        // A drag from it — on, along its own surface
        let mut ed = fresh(setup);
        let (dx, dy) = drag_to(&ed, u, x, y);
        send(&mut ed, K::Down, x, y, scene, walk);
        let s1 = snap(&mut ed);
        send(&mut ed, K::Drag, dx, dy, scene, walk);
        let s2 = snap(&mut ed);
        send(&mut ed, K::Up, dx, dy, scene, walk);
        judge(walk, scene, u, Act::Drag, before(&ed, u), did(&s1, &s2, Act::Drag));
        wheel_at(walk, scene, setup, (x, y), Some(u));
    }
    // (what only a wheel lands on — hover text: the press goes under it)
    let mut only: Vec<(Surface, (usize, usize))> = Vec::new();
    for y in 0..H {
        for x in 0..W {
            let s = ed.mouse_under(K::ScrollDown, x, y).surface;
            if !only.iter().any(|o| o.0 == s) && ed.mouse_at(x, y).surface != s {
                only.push((s, (x, y)));
            }
        }
    }
    for (_, cell) in only {
        wheel_at(walk, scene, setup, cell, None);
    }
}

/// A hover at `cell` (on `pressed`, what a press there lands on), then the wheel down and up — judged by what the wheel
/// there goes to, where that's the pressed surface or one only a wheel lands on.
fn wheel_at(
    walk: &mut Walk,
    scene: &str,
    setup: fn(&mut Editor),
    (x, y): (usize, usize),
    pressed: Option<Under>,
) {
    let mut ed = fresh(setup);
    let wheel = ed.mouse_under(K::ScrollDown, x, y);
    let s0 = snap(&mut ed);
    ed.handle_event(Event::Hover(x as u16, y as u16));
    let s1 = snap(&mut ed);
    if let Some(u) = pressed {
        judge(walk, scene, u, Act::Hover, Before::default(), did(&s0, &s1, Act::Hover));
    }
    send(&mut ed, K::ScrollDown, x, y, scene, walk);
    let s2 = snap(&mut ed);
    send(&mut ed, K::ScrollUp, x, y, scene, walk);
    let s3 = snap(&mut ed);
    let (a, b) = (did(&s1, &s2, Act::Wheel), did(&s2, &s3, Act::Wheel));
    if pressed.is_none_or(|u| u.surface == wheel.surface) {
        let what = [a.what, b.what].join("; ");
        let surface = format!("{:?}", wheel.surface);
        let surface = surface.split('(').next().unwrap_or_default();
        let room = SCROLLS.iter().any(|s| s.0 == scene && s.1 == surface);
        if room {
            walk.scrolled.push(format!("{scene} · {surface}"));
        }
        let with = Before { room, ..Before::default() };
        let did = Did {
            acted: a.acted || b.acted,
            said: a.said || b.said,
            selected: a.selected || b.selected,
            what,
        };
        judge(walk, scene, wheel, Act::Wheel, with, did);
    }
}

/// The walk, once for the tests that read it.
fn walk() -> &'static Walk {
    static WALK: OnceLock<Walk> = OnceLock::new();
    WALK.get_or_init(|| {
        let mut walk = Walk::default();
        for (n, &scene) in SCENES.iter().chain(MOUSE_SCENES).enumerate() {
            walk_scene(&mut walk, n, scene);
        }
        walk
    })
}

fn pty(test: &str) -> bool {
    !crate::sandbox::no_pty_here(&format!("mouse::tests::walk::{test}"))
}

/// The routing test: in every scene, at every distinct thing under the pointer, for a press, its release, a double,
/// a drag, the wheel — the branch of `handle_mouse` that took it (`took_mouse`) is the one `mouse_under` named. A
/// branch added without `mouse_under` knowing shows here.
#[test]
fn mouse_at_is_where_handle_mouse_sends_every_press() {
    if !pty("mouse_at_is_where_handle_mouse_sends_every_press") {
        return;
    }
    let w = walk();
    assert!(w.events > 1000, "the walk sent {} events", w.events);
    assert!(w.routed.is_empty(), "{} of {}:\n{}", w.routed.len(), w.events, w.routed.join("\n"));
}

/// The frame's coverage: every non-blank cell of every scene's frame is under some surface — a new surface that draws
/// without recording where shows here (the mouse's twin of `every_row_the_state_lists_is_on_the_frame`).
#[test]
fn every_drawn_cell_is_under_something() {
    if !pty("every_drawn_cell_is_under_something") {
        return;
    }
    let w = walk();
    assert!(w.bare.is_empty(), "{}", w.bare.join("\n"));
}

/// The walk: every act on everything under the pointer does what the table says — a press, a drag or a wheel that
/// does nothing says why where the user looks; nothing at all only for a hover, a release, a wheel with nothing to
/// scroll, a double whose press did it all, and the gaps a landing fills (`Expect::Gap`, silent until it does).
#[test]
fn no_press_is_lost_in_silence() {
    if !pty("no_press_is_lost_in_silence") {
        return;
    }
    let w = walk();
    assert!(w.off.is_empty(), "{}:\n{}", w.off.len(), w.off.join("\n"));
    // (every surface set up with room had its wheel judged — a scene that lost it doesn't pass by not looking)
    let unmet: Vec<String> =
        SCROLLS.iter().map(|s| format!("{} · {}", s.0, s.1)).filter(|s| !w.scrolled.contains(s)).collect();
    assert!(unmet.is_empty(), "the wheel never judged where it has room: {unmet:?}");
}

/// `docs/mouse.md` is the table's (`TARAE_BLESS=1` writes it again), and every surface the walk found has a row there.
#[test]
fn the_docs_mouse_table_is_the_codes() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/mouse.md");
    let now = doc();
    if std::env::var_os("TARAE_BLESS").is_some_and(|v| v == "1") {
        std::fs::write(&path, &now).unwrap();
    }
    let pinned = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        pinned == now,
        "docs/mouse.md isn't the table's — TARAE_BLESS=1 cargo test mouse::tests::walk\n\n{now}"
    );
    assert!(ACTS.len() == 6);
    if !pty("the_docs_mouse_table_is_the_codes") {
        return;
    }
    let listed: Vec<String> = rows()
        .iter()
        .map(|r| format!("{:?}", r.2).split('(').next().unwrap_or_default().to_string())
        .collect();
    let missing: Vec<&String> = walk().found.iter().filter(|f| !listed.contains(f)).collect();
    assert!(missing.is_empty(), "surfaces with no row in docs/mouse.md: {missing:?}");
}
