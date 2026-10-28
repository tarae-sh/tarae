//! M0 of the mouse audit (`.sb/mouse-plan.md` §3): the walk. Every scene of the keys' routing test and the mouse's
//! own, one cell per distinct thing under the pointer on its frame, every act — held to `handle_mouse`'s routing
//! (`took_mouse` = `mouse_under`), the frame's coverage (every drawn cell is under something) and the table (`expect`:
//! what acts acts, what says says, what doesn't does neither).

use std::sync::OnceLock;

use crate::editor::{Drag, Editor};
use crate::event::{Event, Mouse, MouseKind as K};
use crate::mouse::{ACTS, Act, Expect, Lit, Part, STATES, Surface, TOOK, Under, doc, expect, rows};
use crate::sizes::Border;
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
    // (its view mode's cursor on the last row: the wheel up takes the view past it — #181, nothing holds it to the
    // cursor)
    ("a long conversation, its cursor at the end", |ed| {
        chat(ed, "");
        let c = ed.workspace.claude.chat.as_mut().unwrap();
        for i in 0..30 {
            c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, format!("question {i}"), ""));
            c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Assistant, format!("answer {i}"), ""));
        }
        frame(ed);
        feed(ed, &["<esc>", "G"]);
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
    // (a cut in a floating pane, its card only where that pane is the topmost — U2's fourth review: one drawn outside
    // `whole::Within` was hidden to the walk too; its keys given back, so no row is in hand and the mouse's card is seen)
    ("the messages list, a message cut", |ed| {
        ed.set_status(format!("a message far wider than its list, {}and farewell", "on and on ".repeat(16)));
        ed.messages_open(None);
        frame(ed);
        feed(ed, &["<esc>"]);
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
    // (frictions #172–#174, #178, #179: what the mouse X piece added)
    ("the side panel, long names", |ed| {
        let root = ed.workspace.root.clone();
        let names = ["src/a_file_whose_name_runs_on_far_past_the_panel.rs", "src/main.rs", "Cargo.toml"];
        let files = names.map(String::from).to_vec();
        ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::with_files(root, files));
    }),
    ("two buffers, a nested path", |ed| {
        ed.docs[0].path = Some(ed.workspace.root.join("src/ui/card.rs"));
        let mut other = crate::document::Document::from_str(77, "other\n");
        other.path = Some(ed.workspace.root.join("other.rs"));
        ed.docs.push(other);
    }),
    ("a long diagnostic at a line's end", |ed| {
        long_text(ed);
        // (a file's: a server's diagnostics are never a scratch buffer's — `space d` lists a file's)
        ed.docs[0].path = Some(ed.workspace.root.join("src/main.rs"));
        let message = "mismatched types: expected `usize`, found `String` ".repeat(4);
        ed.docs[0].lsp.diagnostics = vec![crate::lsp::Diagnostic {
            from: 4,
            to: 8,
            severity: 1,
            message,
            raw: serde_json::json!({}),
            source: 0,
        }];
    }),
    // (a sequence under way in each place that keeps its own — `UNDER_WAY`)
    ("the terminal in view mode, a sequence under way", |ed| {
        crate::uistate::tests::terminal(ed);
        feed(ed, &["<C-\\>", "g"]);
    }),
    ("the task panel's output, a count under way", |ed| {
        crate::uistate::tests::task_panel(ed);
        ed.bottom_take_keys();
        feed(ed, &["3"]);
    }),
    ("the task panel's output, f waits for its character", |ed| {
        crate::uistate::tests::task_panel(ed);
        ed.bottom_take_keys();
        feed(ed, &["f"]);
    }),
    ("the chat's view mode, r waits for its character", |ed| {
        crate::uistate::tests::chat_view(ed);
        feed(ed, &["r"]);
    }),
    ("the completion menu, long", |ed| {
        long_text(ed);
        feed(ed, &["j", "A"]);
        let items: Vec<serde_json::Value> =
            (0..30).map(|i| serde_json::json!({"label": format!("line_{i}"), "detail": "i32"})).collect();
        let at = ed.doc().selection().primary().head;
        let mut c = crate::completion::Completion::new(0, ed.doc().id, at, serde_json::Value::from(items));
        c.filter("");
        c.selected = Some(0);
        ed.completion = Some(c);
    }),
    ("a note on the cursor's line", |ed| {
        long_text(ed);
        let path = std::env::temp_dir().join("tarae-mouse-walk-note.rs");
        ed.docs[0].path = Some(path.clone());
        crate::notes::add(ed, &path, 0..1, crate::notes::By::Author, "A word about this line.");
    }),
    ("a card cut to fit, long", |ed| {
        let body: Vec<String> = (0..80).map(|i| format!("line {i} of what it asks")).collect();
        let what = crate::offer::What::ReviewAtEnd;
        ed.push_offer(crate::offer::Offer::new(
            "A long card".into(),
            &body.join("\n"),
            "",
            String::new(),
            what,
        ));
    }),
    // (#189: floating frames without the keys — the follow card over the code, the lists given back their keys)
    ("the follow card over the code", |ed| {
        long_text(ed);
        chat(ed, "");
        let c = ed.workspace.claude.chat.as_mut().unwrap();
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, "a question", ""));
        (c.state, c.follow.on) = (crate::chat::State::Thinking, true);
        ed.workspace.viewer.at = crate::workspace::At::Code;
    }),
    // (#207: the review's rail with files, the code reading one — its rows and marks pressed from the code's keys)
    ("the review's rail, the code reading a file", super::turn_review),
    ("the messages list, its keys given back", |ed| {
        ed.set_status("one");
        ed.messages_open(None);
        frame(ed);
        feed(ed, &["<esc>"]);
    }),
    ("the folders list, its keys given back", |ed| {
        let at = ed.workspace.root.join("elsewhere");
        let folder = crate::folders::Folder { at, name: "elsewhere".into(), instructions: Vec::new() };
        ed.workspace.folders.push(folder);
        ed.folders_open();
        frame(ed);
        feed(ed, &["<esc>"]);
    }),
    // (#185: what goes with the message, over the chat's input — one chip, and three folded to `3 things`)
    ("the chat, a long quote over its input", |ed| {
        chat(ed, "");
        crate::quote::add(ed, vec![quote_of("src/a.rs L1–40", 40)]);
    }),
    // (#197b: Claude's thinking folded — to its first two rows once a stop came after it, `…` at their end; under way,
    // to its last eight, `… ` before them)
    ("the chat, a thought folded", |ed| {
        chat(ed, "");
        let c = ed.workspace.claude.chat.as_mut().unwrap();
        let thought =
            format!("**Reading the map**\n\nThe map is filled {}once.", "as each knot is met, ".repeat(8));
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, "a question", ""));
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Thought, thought, ""));
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Assistant, "An answer, in a few words.", ""));
    }),
    ("the chat, a thought under way", |ed| {
        chat(ed, "");
        let c = ed.workspace.claude.chat.as_mut().unwrap();
        let thought = "Each knot is counted as it is met, the strands laid out one by one. ".repeat(14);
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, "a question", ""));
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Thought, thought, ""));
        c.state = crate::chat::State::Thinking;
    }),
    ("the chat, three things over its input", |ed| {
        chat(ed, "");
        crate::quote::add(ed, vec![quote_of("src/a/rather/long/path/to/a/module.rs L1–3", 3)]);
        let c = ed.workspace.claude.chat.as_mut().unwrap();
        c.screen = Some(crate::chat::Shot { text: "the screen".into(), cols: 120, rows: 40 });
        c.with.push(crate::chat::With::Screen);
        c.pointed.push("chat".into());
        c.with.push(crate::chat::With::Pointed);
    }),
    // (a plugin's overlay in the asks' corner, through a host with no thread — a press is its plugin's, its answer after
    // the walk's look: `plugin::overlay::tests` hears it; a row cut to `CELLS_MAX`, whole under the mouse)
    ("a plugin's overlay", |ed| {
        use crate::plugin::{OverlayRole as Role, OverlaySpan as Span, PluginOverlay as Overlay};
        ed.plugins_detached();
        let span = |text: &str, role| Span { text: text.into(), role };
        let rows = vec![
            vec![span("C-w", Role::Key), span(" v ", Role::Text), span("space", Role::Key)],
            vec![span(&format!("a row past its cells, {}and farewell", "on and on ".repeat(8)), Role::Faint)],
        ];
        ed.plugin_overlay("demo", Some(Overlay { rows, hold: Some(600_000) }));
    }),
];

/// A quote of `n` lines of code, its chip `label`.
fn quote_of(label: &str, n: usize) -> crate::quote::Taken {
    crate::quote::Taken {
        attrs: vec![("from", "code".into())],
        label: label.into(),
        text: (1..=n).map(|i| format!("let line_{i} = {i};")).collect::<Vec<_>>().join("\n"),
        redact: false,
        code: None,
    }
}

/// A `…` drawn that isn't a cut, by the words around it — matched at that `…`'s own cell, never anywhere on its row
/// (U2's fourth review: `r…` excused every `…` beside it, and a cut ending in a word starting with `r`): where (a
/// regex on `scene · surface`, `""` anywhere), the row before it (a regex ending at it) and after it (one starting
/// past it). Something under way (a page's read too), a character waited for on the status line (`f…`, `r…`), the
/// chat's placeholder, which-key's mark of a menu (`Tasks …`, the built-in groups' names).
const NOT_CUTS: &[(&str, &str, &str)] = &[
    ("", r"(^|\s)looking$", r"^(\s|$)"),
    ("", r"(^|\s)listing files$", r"^(\s|$)"),
    ("", r"\s{2}weaving$", r"^(\s|$)"),
    ("", r"\s{2}reading \S+$", r"^\s"),
    // (its keys padded to the position's width they stand in for: two blanks before at least)
    (r"waits for its character · Status$", r"\s{2}[fr]$", r"^(\s|$)"),
    ("", r"› Ask about \S+$", r"^(\s|$)"),
    (r" · WhichKey$", r"\s{2}(Git|Window|Tests|Tasks|Review|View|Goto|Match|Previous|Next) $", r"^(\s|$)"),
];

/// Whether the `…` at cell `x` of `row` (`cells`), drawn in `at` (`scene · surface`), isn't a cut (`NOT_CUTS`).
fn not_a_cut(at: &str, row: &[char], x: usize) -> bool {
    let (before, after): (String, String) = (row[..x].iter().collect(), row[x + 1..].iter().collect());
    let (before, after) = (before.replace('\0', ""), after.replace('\0', ""));
    let is = |re: &str, s: &str| regex::Regex::new(re).unwrap().is_match(s);
    NOT_CUTS.iter().any(|&(w, b, a)| is(w, at) && is(b, &before) && is(a, &after))
}

/// A frame's row as its cells (`screen::text` writes a wide character once): each cell's character, a wide one's second
/// cell `\0`, a mark that takes no cell left out.
fn cells(row: &str) -> Vec<char> {
    use unicode_width::UnicodeWidthChar;
    let mut out = Vec::new();
    for c in row.chars() {
        match c.width().unwrap_or(0) {
            0 if !out.is_empty() => {}
            2 => out.extend([c, '\0']),
            _ => out.push(c),
        }
    }
    out
}

/// Whether the frame shows a cut's own text at its cells (`[a, b)` of `row`): what's drawn there, each stretch between
/// its `…`s (its blanks as the frame has them, the ends' trimmed) a stretch of its whole, in order — and its `…` drawn
/// there (a cut's own drawing always has it: a label with none over its cells isn't it, whatever it says).
/// The walk's oracle of what's on top, from the frame alone (U2's fourth review: never `Whole::seen`, the predicate under
/// test — a cut tagged wrong was hidden to both): a cut under a floating pane, a card, a picker shows that one's cells,
/// not its own. Contiguous (U2's fifth: letters read in order with gaps let almost any short label over a long whole
/// pass).
fn shows_its_own(row: &[char], (a, b): (usize, usize), whole: &str) -> bool {
    let drawn: String =
        (row.get(a..b.min(row.len())).unwrap_or_default().iter()).filter(|c| **c != '\0').collect();
    let parts: Vec<&str> = drawn.split('…').map(str::trim).filter(|p| !p.is_empty()).collect();
    let mut rest = whole;
    drawn.contains('…')
        && !parts.is_empty()
        && parts.iter().all(|p| match rest.find(p) {
            Some(i) => {
                rest = &rest[i + p.len()..];
                true
            }
            None => false,
        })
}

/// The scenes whose keys leave a sequence under way where the keys are — a prefix, a count, an awaited character, the
/// window prefix — as their keys read (`SCENES`' and `MOUSE_SCENES`' last keys): a press there ends it (`STATES`). The
/// walk's oracle for that state, never the code's own `sequence_under_way` — which `the_scenes_under_way_are_listed`
/// holds to this list.
const UNDER_WAY: &[&str] = &[
    "a prefix under way",
    "the space menu",
    "a count",
    "f waits for its character",
    "a card focused, then C-w",
    "list:tests, the window prefix taken from it",
    "list:tests docked right, the window prefix taken from it",
    "C-w m moving the floating tests, the window prefix taken",
    "the terminal's program, the window prefix taken from it",
    "the task panel's output, space under way",
    "the task panel's output, ] under way",
    "the chat's view mode, a count under way",
    "the chat's view mode, a sequence under way",
    "the terminal in view mode, a sequence under way",
    "the task panel's output, a count under way",
    "the task panel's output, f waits for its character",
    "the chat's view mode, r waits for its character",
];

thread_local!(static SCENE_UNDER_WAY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });

/// Scenes made with more than a screen of a surface (by its name in `Surface`'s `Debug`): its wheel has room to scroll,
/// so there it must act — elsewhere a wheel that does nothing may be one with nothing to scroll. (The wheel's room isn't
/// set up for the rail, the terminal, a plugin's panel, a floating pane, a group's list or the edit review yet: S's.)
const SCROLLS: &[(&str, &str)] = &[
    ("a split side by side", "Code"),
    ("a split over and under", "Code"),
    ("the chat beside a long file", "Code"),
    ("a long conversation", "Transcript"),
    ("a long conversation, its cursor at the end", "Transcript"),
    ("a picker of many rows, its preview long", "Picker"),
    ("a picker of many rows, its preview long", "PickerPreview"),
    ("the messages list, long", "Floatlist"),
    ("the side panel, many files", "Side"),
    ("a long test output", "Output"),
    ("hover text, long", "Popup"),
    ("the completion menu, long", "Completion"),
    ("a card cut to fit, long", "Card"),
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
    /// The line lit as dragged (`Editor::border_lit`) — drawn in the accent, which tests draw no colours of.
    lit: String,
    said: u64,
    /// What the mouse over a word shows: the word lit, a presence tag, the now line's card, a strip mark's words.
    over: String,
    /// The word lit alone (`Editor.word_hover`) — what a card's answer shows under the mouse (#200; a peek's cut title
    /// whole on its card is drawn, not an act).
    word: String,
}

impl PartialEq for Snap {
    fn eq(&self, o: &Snap) -> bool {
        (&self.frame, &self.keys, &self.sel, &self.lit, self.said)
            == (&o.frame, &o.keys, &o.sel, &o.lit, o.said)
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
    if a.lit != b.lit {
        out.push(format!("lit {} → {}", a.lit, b.lit));
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
    // (the stand-in for a state that changes by itself, `DRIFT`: landing after this snap's frames, as a thread's write
    // between two of them)
    if let n @ 1.. = DRIFT.get() {
        DRIFT.set(n - 1);
        if n == 1 {
            ed.docs[0].text = ropey::Rope::from_str("moved by itself\n");
        }
    }
    let frame = colourless(&frame);
    let strip = ed.workspace.claude.chat.as_ref().and_then(|c| c.follow.hover.clone());
    let over = format!("{:?} {:?} {:?} {strip:?}", ed.word_hover.get(), ed.presence.hover, ed.cut_hover);
    let word = format!("{:?}", ed.word_hover.get());
    Snap { frame, text, keys, sel, lit: format!("{:?}", ed.border_lit()), said: ed.said, over, word }
}

/// A snap where an act is judged, and whether the frame then holds still: a second snap with no event between. A frame
/// that changes between the two (a clock the walk doesn't hold, a thread's writes) is listed (`Walk.moved`, `when` it
/// was), and the acts on either side of it aren't judged (frictions #164). A diagnostic, not a guarantee: it catches what
/// lands between the two looks and only narrows the rest — a change that lands after the second look, during the next
/// act, is still that act's. Its job is to name the race the next time it lands where it's seen. A scene whose frame
/// moves by nature (`MOVING`) isn't looked at twice.
fn after(walk: &mut Walk, ed: &mut Editor, scene: &str, u: Under, when: &str) -> (Snap, bool) {
    let s = snap(ed);
    if MOVING.get() || UNGUARDED.get() {
        return (s, true);
    }
    let again = snap(ed);
    if again != s {
        walk.moved.push(format!("{scene} · {} · {when}: {}", name(u), changed(&s, &again)));
        return (again, false);
    }
    (s, true)
}

/// A frame's bytes without their colours (`38;2;r;g;b`, `48;2;…`, `58;2;…` in a style's sequence) — the walk judges a
/// frame as drawn with none (an act that only colours is no act here), whatever crossterm's switch says: it's the
/// process's, other tests turn it on as they run, and the walk was judged with or without colours by the order they ran
/// in (an edge pressed lights its line while held — in colour only).
fn colourless(frame: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(frame.len());
    let mut i = 0;
    while i < frame.len() {
        // (after a `;` within a sequence or its `[` first: the `[` kept)
        let rgb = [b"38;2;".as_slice(), b"48;2;", b"58;2;"].iter().any(|p| frame[i + 1..].starts_with(p));
        if rgb && matches!(frame[i], b';' | b'[') {
            if frame[i] == b'[' {
                out.push(b'[');
            }
            // (to the `;` before its three numbers, then each `;` and its digits)
            i += 5;
            for _ in 0..3 {
                i += 1 + frame[(i + 1).min(frame.len())..].iter().take_while(|b| b.is_ascii_digit()).count();
            }
            continue;
        }
        out.push(frame[i]);
        i += 1;
    }
    out
}

/// What an act did between two snaps. A hover acts where it moves the keys or a selection, or on a word, where it shows
/// what that word is (lit, its tag, its card, its words — `Snap.over`; tests draw no colours) — what else it shows is
/// drawn, not an act (a cut row whole; a line that drags, lit: `Did.drawn`); in a scene whose frame moves by itself (a
/// draft being written), neither is the frame.
fn did(a: &Snap, b: &Snap, act: Act, u: Under) -> Did {
    let word = act == Act::Hover
        && match u.part {
            Part::Word(_) => a.over != b.over,
            Part::Answer(_) => a.word != b.word,
            _ => false,
        };
    let moved = a.keys != b.keys || a.sel != b.sel || word;
    let drawn = !MOVING.get() && (a.frame != b.frame || a.lit != b.lit);
    Did {
        acted: moved || (drawn && act != Act::Hover),
        drawn,
        said: a.said != b.said,
        selected: a.sel != b.sel,
        what: changed(a, b),
    }
}

/// What an act did (`did`): acted, said, changed a selection (the code's, the edit review's or a text surface's), and
/// what changed in words.
struct Did {
    acted: bool,
    /// The frame changed (a hover's too: what it lights where the table says a hover acts — a line that drags).
    drawn: bool,
    said: bool,
    selected: bool,
    what: String,
}

thread_local!(static MOVING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });

thread_local! {
    /// The test of `after`'s: the scene's state changes by itself after this many more snaps (0: never).
    static DRIFT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The test of `after`'s, broken: no second look — every act judged, whatever moved.
    static UNGUARDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the keys are already where a press on `u` gives them — a press there with nothing else to do is no silence
/// (a press gives the keys to the surface it lands on: given, it's done).
fn holds(ed: &Editor, u: Under) -> bool {
    use crate::uistate::Region as R;
    let r = ed.keys_to().region;
    match u.surface {
        Surface::Code(p) | Surface::PaneTop(p) => {
            matches!(r, R::Code | R::Weave | R::Page) && ed.workspace.viewer.focus == p
        }
        // (the code's keys, where its `esc stop` is typed — a shortcut over them, `keys_chain`)
        Surface::Ask => matches!(r, R::Code | R::Weave | R::Page),
        // (news: a press leaves the keys where they are — what it does is its plugin's, heard after the walk's look)
        Surface::Overlay => true,
        // (a chip's press shows what it stands for — the keys given aren't all it does; `N things`' is)
        Surface::Chat if matches!(u.part, Part::Word(crate::mouse::Word::With(Some(_)))) => false,
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
        // (a line: what it heads — `Editor::border_heads`)
        Surface::Edge => match u.part {
            Part::Border(Border::Chat) => matches!(r, R::Chat | R::View(_)),
            Part::Border(Border::Side) => {
                matches!(r, R::Side(_)) || ed.workspace.is_at(crate::workspace::At::Side)
            }
            Part::Border(Border::Rail) => r == R::Rail,
            Part::Border(Border::Bottom) => ed.bottom_has_keys(),
            Part::Border(Border::Leaf(_)) => matches!(r, R::Output(_) | R::Slot(_) | R::List(_)),
            Part::Border(Border::Divider { heads, .. }) => {
                matches!(r, R::Code | R::Weave | R::Page) && ed.workspace.viewer.focus == heads
            }
            _ => false,
        },
        _ => false,
    }
}

/// A cell to drag to from (`x`, `y`), on the same surface as `u` where one is near — a drag from a surface onto another
/// is the other's act, not this one's (what a press holds — a border, a frame — goes three on and one down).
fn drag_to(ed: &Editor, u: Under, x: usize, y: usize) -> (usize, usize) {
    let far = ((x + 3).min(W - 1), (y + 1).min(H - 1));
    // (a line held: toward the code — a panel's edge, the slot's top line up — and a name on the slot's line, the line too)
    if matches!(u.part, Part::Border(_) | Part::Frame(_)) || u.surface == Surface::SlotLine {
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
    /// `scene · surface/part · when: …` where the frame changed between `after`'s two looks, with no event (frictions
    /// #164: an act judged against a frame that moves by itself is blamed for what it didn't do).
    moved: Vec<String>,
    /// `scene · surface` of `SCROLLS` whose wheel was judged with room.
    scrolled: Vec<String>,
    /// `scene · surface`, the row's cells and the `…`'s cell of a `…` drawn where no cut is recorded (`whole::cut_at`,
    /// the chat's rows).
    dots: Vec<(String, Vec<char>, usize)>,
    /// `scene · at x,y: …` where a cut text under the mouse wasn't taken in hand as its surface says, or drew nothing.
    cuts: Vec<String>,
    /// The scenes where a chip over the chat's input was met (`cuts_in`).
    chips: Vec<String>,
    /// `scene · surface/part at x,y: the keys …` where a press `gives_keys` left them elsewhere (#207).
    keys: Vec<String>,
    /// `surface/part` of every press `gives_keys` judged.
    given: Vec<String>,
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
    /// A key sequence under way (`STATES`): a press ends it.
    pending: bool,
    /// On the floating pane `C-w m` moves (`STATES`): a press says how it moves.
    moving_here: bool,
    /// On a card's answer that lights nothing under the mouse (#200): a peek's row, its "more lines" row (off its bottom
    /// line), an answer held until its text is read whole (`Editor::offer_press_held`).
    unlit: bool,
}

/// Whether the card's answer under `u` at row `y` lights nothing (`Before.unlit`).
fn unlit(ed: &Editor, u: Under, y: usize) -> bool {
    let Part::Answer(k) = u.part else { return false };
    let bottom = ed.screen.offer_card.is_some_and(|c| y + 1 == c.1 + c.3);
    k.is_ascii_digit() || !bottom || ed.offer_press_held(k)
}

fn judge(
    walk: &mut Walk,
    scene: &str,
    u: Under,
    act: Act,
    before: Before,
    Did { acted, drawn, said, selected, what }: Did,
) {
    let quiet = !acted && !said;
    let pressed = matches!(act, Act::Press | Act::Double);
    let e = state_expect(u, act, before).unwrap_or_else(|| expect(u.surface, u.part, act));
    // (a scene whose frame moves by itself: an act that only draws isn't told from it)
    let ok = MOVING.get() && quiet && e == Expect::Acts
        || match e {
            // (the wheel where there may be nothing to scroll: silent too — where there's room, it must act)
            // (a double on text: its word selected — drawn alone doesn't do, tests draw no colours)
            Expect::Acts if act == Act::Double && matches!(u.part, Part::Text { .. }) => {
                selected || before.empty
            }
            // (a hover acts by what it lights: a line that drags)
            Expect::Acts if act == Act::Hover => acted || drawn,
            // (a release that gives the keys: given already, it's done)
            Expect::Acts if act == Act::Release => acted || (quiet && before.holds),
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

/// A state that decides an act whatever it lands on (`STATES`): a key sequence under way — a press ends it, then acts
/// where it landed; `C-w m` moving a floating pane — a press on it (not its top line, which still drags it) says how it
/// moves (a double says it again, its drag is that press's), elsewhere it takes the keys and the move ends with them.
fn state_expect(u: Under, act: Act, before: Before) -> Option<Expect> {
    if before.pending && act == Act::Press {
        return Some(STATES[1].2);
    }
    if before.unlit && act == Act::Hover {
        return Some(Expect::Not(
            "a peek, its more lines' row or an answer held until it's read: nothing lit",
        ));
    }
    if !before.moving_here {
        return None;
    }
    // (its top line, pressed: held to drag it — the keys already the move's)
    if matches!(u.part, Part::Frame(_)) {
        return (act == Act::Press).then_some(Expect::Not("its top line still drags it"));
    }
    match act {
        Act::Press | Act::Double => Some(STATES[0].2),
        Act::Drag => Some(Expect::Not("the press said it")),
        _ => None,
    }
}

fn before(ed: &Editor, u: Under, (x, y): (usize, usize)) -> Before {
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
    Before {
        holds: holds(ed, u),
        lets_go: ed.card_has_keys() && !modal,
        empty,
        room: false,
        // (as the scene's keys left it: `UNDER_WAY` — before its first press, which ends it)
        pending: SCENE_UNDER_WAY.get(),
        moving_here: ed.float_moving.is_some() && ed.float_at(x, y).map(|f| f.0) == ed.float_moving,
        unlit: unlit(ed, u, y),
    }
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
    let mut out: Vec<Target> = surfaces.into_iter().map(|s| of(s)[n % of(s).len()]).collect();
    // (and a chip over the chat's input wherever one is drawn: only its own scenes have one — #185)
    let chip = |t: &&Target| matches!(t.1.part, Part::Word(crate::mouse::Word::With(_)));
    out.extend(targets.iter().filter(chip).filter(|t| !out.contains(t)).copied().collect::<Vec<_>>());
    out
}

fn walk_held(walk: &mut Walk, n: usize, scene: &str, setup: fn(&mut Editor)) {
    SCENE_UNDER_WAY.set(UNDER_WAY.contains(&scene));
    let mut ed = fresh(setup);
    // (a scene whose frame moves by itself: what arrives on the stand-in Claude's stream, a draft revealed as it comes)
    MOVING.set(scene.contains("being written") || snap(&mut ed) != snap(&mut ed));
    let text = shown(&mut ed);
    let (targets, bare) = targets(&ed, &text);
    if let Some(&(x, y)) = bare.first() {
        walk.bare.push(format!("{scene}: {} cells under nothing, first at {x},{y}", bare.len()));
    }
    cuts_in(walk, &ed, &text, scene, setup);
    keys_given(walk, scene, setup, &targets);
    for &((x, y), u) in &sample(targets, n) {
        let found = format!("{:?}", u.surface);
        let found = found.split('(').next().unwrap_or_default().to_string();
        if !walk.found.contains(&found) {
            walk.found.push(found);
        }
        // A press, its release, and a second press at the same instant (a double)
        let mut ed = fresh(setup);
        let (s0, still0) = after(walk, &mut ed, scene, u, "before the press");
        let b0 = before(&ed, u, (x, y));
        let front = |ed: &Editor| ed.offers.first().map(|o| o.id);
        let card = front(&ed);
        let word = word_at(&ed, u);
        send(&mut ed, K::Down, x, y, scene, walk);
        let (s1, still1) = after(walk, &mut ed, scene, u, "after the press");
        let b1 = Before { holds: holds(&ed, u), ..Before::default() };
        send(&mut ed, K::Up, x, y, scene, walk);
        let (s2, still2) = after(walk, &mut ed, scene, u, "after the release");
        // (the double is on what the press there is now — the first may have moved the screen, or answered a card and
        // brought the next one under the pointer: another card's press. Where the first press made what was under it go
        // — a start-screen row's picker, a completion accepted, which-key's next level — the second press lands on
        // another surface, and the table's row for that one has no double of this one to judge: its being one press,
        // nothing under it pressed, is `Editor.pressed_through`'s, tested by `overlays::a_double_click_where_…`)
        let again = ed.mouse_at(x, y) == u && front(&ed) == card && word_at(&ed, u) == word;
        let b2 = before(&ed, u, (x, y));
        send(&mut ed, K::Down, x, y, scene, walk);
        send(&mut ed, K::Up, x, y, scene, walk);
        let (s3, still3) = after(walk, &mut ed, scene, u, "after the double");
        if still0 && still1 {
            judge(walk, scene, u, Act::Press, b0, did(&s0, &s1, Act::Press, u));
        }
        if still1 && still2 {
            judge(walk, scene, u, Act::Release, b1, did(&s1, &s2, Act::Release, u));
        }
        if again && still2 && still3 {
            judge(walk, scene, u, Act::Double, b2, did(&s2, &s3, Act::Double, u));
        }
        // A drag from it — on, along its own surface
        let mut ed = fresh(setup);
        let (dx, dy) = drag_to(&ed, u, x, y);
        send(&mut ed, K::Down, x, y, scene, walk);
        let (s1, still1) = after(walk, &mut ed, scene, u, "after the drag's press");
        send(&mut ed, K::Drag, dx, dy, scene, walk);
        let (s2, still2) = after(walk, &mut ed, scene, u, "after the drag");
        send(&mut ed, K::Up, dx, dy, scene, walk);
        if still1 && still2 {
            judge(walk, scene, u, Act::Drag, before(&ed, u, (x, y)), did(&s1, &s2, Act::Drag, u));
        }
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

/// Where a press gives its surface the keys, whatever else it does (the mouse rule: a press gives the keys to the
/// surface it lands on) — the parts held to it after every press of every scene, not one sampled (#207: a press on the
/// review rail's file row showed the file and left the keys in the code).
fn gives_keys(u: Under) -> bool {
    match u.surface {
        Surface::Rail => matches!(u.part, Part::Row(_) | Part::Mark(_) | Part::Blank),
        Surface::Code(_) => matches!(u.part, Part::Text { .. }),
        _ => false,
    }
}

/// Each of the scene's targets where a press `gives_keys`: pressed and let go, the keys there after (`holds`).
fn keys_given(walk: &mut Walk, scene: &str, setup: fn(&mut Editor), targets: &[Target]) {
    for &((x, y), u) in targets.iter().filter(|t| gives_keys(t.1)) {
        let mut ed = fresh(setup);
        send(&mut ed, K::Down, x, y, scene, walk);
        send(&mut ed, K::Up, x, y, scene, walk);
        frame(&mut ed);
        if !walk.given.contains(&name(u)) {
            walk.given.push(name(u));
        }
        if !holds(&ed, u) {
            walk.keys.push(format!("{scene} · {} at {x},{y}: the keys {:?}", name(u), ed.keys_to().region));
        }
    }
}

/// Every `…` on the scene's frame under a cut recorded (else listed, `Walk.dots`), and every cut text there taken in hand
/// by the mouse over it — a card drawn — where what it's drawn on is what's under the pointer, and not where it isn't.
fn cuts_in(walk: &mut Walk, ed: &Editor, text: &str, scene: &str, setup: fn(&mut Editor)) {
    use crate::whole::Cut;
    let spots = ed.screen.cut_spots.borrow().clone();
    let rows: Vec<Vec<char>> = text.lines().map(cells).collect();
    for (y, row) in rows.iter().enumerate() {
        for (x, _) in row.iter().enumerate().filter(|c| *c.1 == '…') {
            if !spots.iter().any(|s| s.0 == y && (s.1..s.2).contains(&x)) {
                let surface = format!("{:?}", ed.mouse_at(x, y).surface);
                let surface = surface.split('(').next().unwrap_or_default().to_string();
                walk.dots.push((format!("{scene} · {surface}"), row.clone(), x));
            }
        }
    }
    for (y, a, b, cut) in spots {
        // (a chip over the chat's input: what goes, on its card — #185)
        if let Cut::With(which) = cut {
            let x = (a + b) / 2;
            let mut ed = fresh(setup);
            walk.chips.push(scene.to_string());
            let pressed = ed.mouse_at(x, y).part == Part::Word(crate::mouse::Word::With(which));
            let before = snap(&mut ed);
            ed.handle_event(Event::Hover(x as u16, y as u16));
            let after = snap(&mut ed);
            let what = match (pressed, ed.cut_hover == Some(Cut::With(which)), before.frame != after.frame) {
                (true, true, true) => continue,
                (false, ..) => "not a chip under the mouse",
                (_, false, _) => "not taken in hand",
                (_, true, false) => "in hand, nothing drawn",
            };
            walk.cuts.push(format!("{scene} · at {x},{y} (a chip, {which:?}): {what}"));
            continue;
        }
        let Cut::Text(w) = cut else { continue };
        let x = (a + b) / 2;
        let mut ed = fresh(setup);
        // (what's on top at its cells, as the frame shows them — never the code's own `Whole::seen`)
        let shows = rows.get(y).is_some_and(|row| shows_its_own(row, (a, b), &w.text));
        let before = snap(&mut ed);
        ed.handle_event(Event::Hover(x as u16, y as u16));
        let after = snap(&mut ed);
        let held = matches!(&ed.cut_hover, Some(Cut::Text(h)) if h.text == w.text);
        let what = match (shows, held, before.frame != after.frame) {
            (true, true, true) | (false, false, _) => continue,
            (true, false, _) => "not taken in hand",
            (true, true, false) => "in hand, nothing drawn",
            (false, true, _) => "in hand under what's over it",
        };
        walk.cuts.push(format!("{scene} · at {x},{y} ({:?}): {what} — {:?}", w.on, w.text));
    }
}

/// The word that does its key under `u`, as it says itself — a press may draw another in its place (`esc code` → `space
/// l focus`): a second press there is that one's, not a double.
fn word_at(ed: &Editor, u: Under) -> Option<(crate::wordspot::Press, Vec<String>)> {
    let Part::Word(crate::mouse::Word::Does(i)) = u.part else { return None };
    let spots = ed.screen.word_spots.borrow();
    spots.get(i).map(|s| (s.press.clone(), s.spans.iter().map(|s| s.0.clone()).collect()))
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
    let at = pressed.unwrap_or(wheel);
    let (s0, still0) = after(walk, &mut ed, scene, at, "before the hover");
    ed.handle_event(Event::Hover(x as u16, y as u16));
    let (s1, still1) = after(walk, &mut ed, scene, at, "after the hover");
    if let Some(u) = pressed.filter(|_| still0 && still1) {
        let with = Before { unlit: unlit(&ed, u, y), ..Before::default() };
        judge(walk, scene, u, Act::Hover, with, did(&s0, &s1, Act::Hover, u));
    }
    send(&mut ed, K::ScrollDown, x, y, scene, walk);
    let (s2, still2) = after(walk, &mut ed, scene, at, "after the wheel down");
    send(&mut ed, K::ScrollUp, x, y, scene, walk);
    let (s3, still3) = after(walk, &mut ed, scene, at, "after the wheel up");
    let (a, b) = (did(&s1, &s2, Act::Wheel, wheel), did(&s2, &s3, Act::Wheel, wheel));
    if pressed.is_none_or(|u| u.surface == wheel.surface) && still1 && still2 && still3 {
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
            drawn: a.drawn || b.drawn,
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

/// A press gives the keys to the surface it lands on (the mouse rule), wherever `gives_keys` says it does, in every scene
/// — #207: a press on the review rail's file row or mark, the code reading a file, left the keys in the code.
#[test]
fn a_press_gives_its_surface_the_keys() {
    if !pty("a_press_gives_its_surface_the_keys") {
        return;
    }
    let w = walk();
    // (each part met somewhere — a scene that lost it doesn't pass by not looking)
    for part in ["Rail/Row", "Rail/Mark", "Rail/Blank"] {
        assert!(w.given.iter().any(|g| g == part), "no press on {part} judged: {:?}", w.given);
    }
    assert!(w.keys.is_empty(), "{}:\n{}", w.keys.len(), w.keys.join("\n"));
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
    // (a frame that changed with no event between two looks: something the scene's state reads that isn't the walk's — a
    // clock it doesn't hold, a thread's writes — named first, where it was seen)
    assert!(w.moved.is_empty(), "the frame moved with no event:\n{}", w.moved.join("\n"));
    assert!(w.off.is_empty(), "{}:\n{}", w.off.len(), w.off.join("\n"));
    // (every surface set up with room had its wheel judged — a scene that lost it doesn't pass by not looking)
    let unmet: Vec<String> =
        SCROLLS.iter().map(|s| format!("{} · {}", s.0, s.1)).filter(|s| !w.scrolled.contains(s)).collect();
    assert!(unmet.is_empty(), "the wheel never judged where it has room: {unmet:?}");
}

/// Every `…` tarae draws shows its whole text under the mouse (frictions #178, #179): on every scene's frame, each `…` is
/// under a cut recorded where it's drawn, and the mouse over a cut text shows it whole (its card drawn) where what it's
/// drawn on is under the pointer — but a `…` that isn't a cut (`NOT_CUTS`). What no walk meets is held by
/// `whole::tests::every_cut_tarae_draws_is_recorded`.
#[test]
fn every_cut_text_is_whole_under_the_mouse() {
    if !pty("every_cut_text_is_whole_under_the_mouse") {
        return;
    }
    let w = walk();
    assert!(w.cuts.is_empty(), "{}", w.cuts.join("\n"));
    // (what goes with the message, over the chat's input: met in its scenes, one chip and three folded — #185)
    for scene in ["the chat, a long quote over its input", "the chat, three things over its input"] {
        assert!(w.chips.iter().any(|s| s == scene), "no chip met in {scene:?}");
    }
    let open: Vec<String> = (w.dots.iter())
        .filter(|(at, row, x)| !not_a_cut(at, row, *x))
        .map(|(at, row, x)| {
            format!("{at} at {x}: {}", row.iter().filter(|c| **c != '\0').collect::<String>().trim())
        })
        .collect();
    assert!(open.is_empty(), "a `…` not whole under the mouse:\n{}", open.join("\n"));
}

/// A `…` that isn't a cut is told at its own cell (U2's fourth review): a character waited for on the status line, a
/// menu's mark on which-key, a page being read — each excused there, and a cut beside it on the same row, or one whose
/// cut word starts with what an exemption's does, not.
#[test]
fn a_dot_that_isnt_a_cut_is_told_at_its_own_cell() {
    let at = |row: &str| {
        let row = cells(row);
        let x = row.iter().rposition(|c| *c == '…').unwrap();
        (row, x)
    };
    let waits = "f waits for its character · Status";
    for (scene, row, excused) in [
        (waits, "● normal  src/a.rs   f… ", true),
        (waits, "● normal  a message about the r…  f… ", true),
        ("the space menu · WhichKey", "│ m  Tasks …      │", true),
        ("a page · PaneTop", "─ web · docs.example.com  reading docs.example.com/guide… 3s ─", true),
        ("the chat · Input", "  › Ask about main.rs…", true),
        // (a cut on the same row as one excused: its own `…` judged, the last on the row)
        (waits, "● normal   f…  a message cut befo…", false),
        ("the space menu · WhichKey", "│ m  Tasks …  w  Window …  s  Search in the whole proj… │", false),
        ("a page · PaneTop", "─ web  reading docs.example.com/guide…  a title cut in the midd… ─", false),
        // (a cut word starting with an exemption's letter, where it isn't waited for)
        ("a long conversation · Transcript", "  ▸ cargo test -q -- the r…", false),
        (waits, "● normal  a message about the r…", false),
        ("the space menu · WhichKey", "│ s  Search in the whole …      │", false),
    ] {
        let (row, x) = at(row);
        assert_eq!(not_a_cut(scene, &row, x), excused, "{scene}: {:?}", row.iter().collect::<String>());
    }
}

/// The walk's oracle of a cut's being on top reads the frame (U2's fourth review): its own text at its cells, but its
/// `…` — a floating pane's, a card's, a blank ground's cells there are not it.
#[test]
fn a_cut_on_top_is_read_from_the_frame() {
    let whole = "a title far too long to fit and farewell";
    for (row, on_top) in [
        ("─ a title far too lo… ─", true),
        ("─ …too long to fit an… ─", true),
        ("─ │ messages ─────── ─", false),
        ("─                    ─", false),
        ("─ one  a message     ─", false),
    ] {
        assert_eq!(shows_its_own(&cells(row), (2, 21), whole), on_top, "{row:?}");
    }
    // (a long whole — the scene's cut message — and a short label over its cells: its letters are somewhere in it in
    // order, never a stretch of it; the cut's own stretches, a middle cut's two, are)
    let whole = format!("a message far wider than its list, {}and farewell", "on and on ".repeat(16));
    for (row, on_top) in [
        ("  one                 ", false),
        ("  sand                ", false),
        ("  a message far wid… ", true),
        ("  …on and farewell   ", true),
        ("  a message…farewell ", true),
        ("  farewell…a message ", false),
        // (a pane's label that is a stretch of it, with no `…` — U2's sixth review)
        ("  on and             ", false),
        ("  a                   ", false),
    ] {
        assert_eq!(shows_its_own(&cells(row), (2, 21), &whole), on_top, "{row:?}");
    }
}

/// Every floating frame lights half while the pointer is on it, as a docked surface's lines do (#189, the mouse's
/// bullet): in every scene each frame drawn without the keys is the wash with no pointer, half with the pointer on a cell
/// of it where the surface under the pointer is its (`lights_at`), the wash again with the pointer off it. Every frame's
/// corner drawn (`╭`) is one `Screen.frames` recorded — a frame drawn another way shows here — and one with no surface
/// for the pointer (`of` None) is up only while it has the keys.
#[test]
fn every_floating_frame_lights_half_under_the_pointer() {
    if !pty("every_floating_frame_lights_half_under_the_pointer") {
        return;
    }
    let inside = |(x, y, w, h): (usize, usize, usize, usize), cx: usize, cy: usize| {
        (x..x + w).contains(&cx) && (y..y + h).contains(&cy)
    };
    let (mut off, mut met) = (Vec::new(), Vec::new());
    for &(scene, setup) in SCENES.iter().chain(MOUSE_SCENES) {
        crate::clock::hold();
        let mut ed = fresh(setup);
        let text = shown(&mut ed);
        let frames = ed.screen.frames.borrow().clone();
        for (y, row) in text.lines().enumerate() {
            for (x, _) in row.chars().enumerate().filter(|c| c.1 == '╭') {
                if !frames.iter().any(|f| (f.rect.0, f.rect.1) == (x, y)) {
                    off.push(format!("{scene}: a frame at {x},{y} drawn but not recorded"));
                }
            }
        }
        for f in &frames {
            let (of, rect) = match (f.of, f.lit) {
                (_, Lit::Keys) => continue,
                (None, lit) => {
                    off.push(format!(
                        "{scene}: a frame at {:?} {lit:?}, nothing the pointer lights it for",
                        f.rect
                    ));
                    continue;
                }
                (Some(of), Lit::Half) => {
                    off.push(format!("{scene} · {of:?}: half lit with no pointer"));
                    continue;
                }
                (Some(of), Lit::Wash) => (of, f.rect),
            };
            met.push(format!("{scene} · {of:?}"));
            let (x, y, w, h) = rect;
            let cells = (y..y + h).flat_map(|cy| (x..x + w).map(move |cx| (cx, cy)));
            let mine: Vec<(usize, usize)> = cells
                .filter(|&(cx, cy)| ed.lights_at(ed.mouse_under(K::Down, cx, cy), cx, cy) == Some(of))
                .collect();
            // (under a prompt, a picker or pointing every press is theirs — outside, it closes them: nothing under them
            // lights, docked or floating)
            let modal = |cx, cy| {
                let s = ed.mouse_under(K::Down, cx, cy).surface;
                matches!(s, Surface::Prompt | Surface::Picker | Surface::Pointing)
            };
            let Some(&(cx, cy)) = mine.get(mine.len() / 2) else {
                if (y..y + h).all(|cy| (x..x + w).all(|cx| modal(cx, cy))) {
                    continue;
                }
                off.push(format!("{scene} · {of:?} at {rect:?}: no cell of it is its surface"));
                continue;
            };
            let lit =
                |ed: &Editor| ed.screen.frames.borrow().iter().find(|g| g.of == Some(of)).map(|g| g.lit);
            let mut over = fresh(setup);
            over.handle_event(Event::Hover(cx as u16, cy as u16));
            frame(&mut over);
            if lit(&over) != Some(Lit::Half) {
                off.push(format!("{scene} · {of:?} at {rect:?}: the pointer at {cx},{cy}, {:?}", lit(&over)));
            }
            let away = (0..H).flat_map(|ay| (0..W).map(move |ax| (ax, ay)));
            if let Some((ax, ay)) =
                away.clone().find(|&(ax, ay)| !frames.iter().any(|g| inside(g.rect, ax, ay)))
            {
                over.handle_event(Event::Hover(ax as u16, ay as u16));
                frame(&mut over);
                if lit(&over) != Some(Lit::Wash) {
                    off.push(format!(
                        "{scene} · {of:?}: the pointer gone off it to {ax},{ay}, {:?}",
                        lit(&over)
                    ));
                }
            }
        }
        crate::clock::release();
    }
    assert!(off.is_empty(), "{}", off.join("\n"));
    // (each kind met unlit: a waiting ask, the follow card, the floating lists)
    let kinds = [
        "a light card · Card",
        "the follow card over the code · Follow",
        "the code, the session list floating without the keys · Dock(Sessions)",
        "the messages list, its keys given back · Dock(Messages)",
        "the folders list, its keys given back · Dock(Folders)",
    ];
    let unmet: Vec<&&str> = kinds.iter().filter(|k| !met.iter().any(|m| m == *k)).collect();
    assert!(unmet.is_empty(), "never met unlit: {unmet:?}\nmet: {met:#?}");
}

/// A floating frame moved from under a still pointer goes back to its wash (`Ui::frame_lit`'s cell check): `:messages`
/// without the keys, the pointer on its left side — half — then the pane moved off that cell (as `C-w m` or a drag moves
/// it) with no hover after: the surface the last hover found is still its, the frame the wash.
#[test]
fn a_frame_moved_from_under_a_still_pointer_is_washed_again() {
    if !pty("a_frame_moved_from_under_a_still_pointer_is_washed_again") {
        return;
    }
    use crate::pane::GroupKind;
    let scene = MOUSE_SCENES.iter().find(|s| s.0 == "the messages list, its keys given back").unwrap();
    let mut ed = fresh(scene.1);
    let of = crate::mouse::Lights::Dock(GroupKind::Messages);
    let lit =
        |ed: &Editor| ed.screen.frames.borrow().iter().find(|g| g.of == Some(of)).map(|g| (g.lit, g.rect));
    let (_, (x, y, w, _)) = lit(&ed).expect("the messages list drawn");
    let (px, py) = (x, y + 1);
    assert_eq!(
        ed.lights_at(ed.mouse_under(K::Down, px, py), px, py),
        Some(of),
        "its left side is its surface"
    );
    ed.handle_event(Event::Hover(px as u16, py as u16));
    frame(&mut ed);
    assert_eq!(lit(&ed).map(|l| l.0), Some(Lit::Half), "the pointer on it");
    // (moved past the pointer's column: right of it where there's room, else left of it)
    let r = ed.float_rect(GroupKind::Messages).unwrap();
    let to = if px + 1 + w <= W { px + 1 } else { px.saturating_sub(w) };
    ed.float_set(GroupKind::Messages, crate::split::Rect { x: to, ..r });
    frame(&mut ed);
    assert_eq!(ed.pointer_over, Some(of), "no hover since: the surface found is still its");
    let (now, rect) = lit(&ed).unwrap();
    assert_ne!(rect.0, x, "it moved");
    assert_eq!(now, Lit::Wash, "moved from under the pointer at {px},{py}: {rect:?}");
}

/// What `after`'s second look does and doesn't do. The scene's state changed by itself (`DRIFT`, as a thread's write
/// lands) between the two looks after the first release: named where it moved (`Walk.moved`), not blamed on an act
/// (`Walk.off`) — and without the second look (`UNGUARDED`) the same drift is the release's failure, as #164's was. The
/// known limit: the same change landing just after the second look isn't seen by it, and the next act (the double) is
/// still blamed for it.
#[test]
fn a_frame_that_moves_between_two_looks_is_named_not_blamed_on_an_act() {
    let scene = SCENES[0];
    // (`snaps`: after how many it lands — the scene's own look for a moving frame takes 2, then each act's snap and,
    // guarded, its second look: 7 is after the release's snap, 8 after its second look; unguarded, 4 is after the
    // release's. The first target is the pane's top line, where a release does nothing (`Not("the press did it")`) nor a
    // double, so blamed they fail)
    let drift = |unguarded, snaps| {
        DRIFT.set(snaps);
        UNGUARDED.set(unguarded);
        let mut walk = Walk::default();
        walk_scene(&mut walk, 0, scene);
        DRIFT.set(0);
        UNGUARDED.set(false);
        walk
    };
    let at = format!("{} · ", scene.0);
    let named = |w: &Walk| w.moved.iter().any(|m| m.starts_with(&at) && m.contains("moved by itself"));
    let blamed =
        |w: &Walk, act: &str| w.off.iter().any(|m| m.starts_with(&at) && m.contains(&format!(" · {act}:")));
    let broken = drift(true, 4);
    assert!(blamed(&broken, "Release"), "unguarded, the release blamed: {:?}", broken.off);
    let w = drift(false, 7);
    assert!(named(&w), "between the looks: the drift named: {:?}", w.moved);
    assert!(w.off.is_empty(), "between the looks: no act blamed for it: {:?}", w.off);
    // (the limit, pinned: past the second look it isn't seen, and the next act takes the blame)
    let late = drift(false, 8);
    assert!(!named(&late), "after the second look: not seen by it: {:?}", late.moved);
    assert!(blamed(&late, "Double"), "after the second look: the double still blamed: {:?}", late.off);
}

/// A frame judged without its colours: two frames that differ only in them are the same, any other style or text
/// still tells them apart.
#[test]
fn the_walk_judges_frames_without_their_colours() {
    let a = b"\x1b[0;38;2;1;2;3;48;2;40;50;60;1mab\x1b[38;2;9;9;9mc";
    let b = b"\x1b[0;38;2;200;2;3;48;2;4;5;6;1mab\x1b[38;2;1;1;1mc";
    assert_eq!(colourless(a), colourless(b));
    assert_eq!(colourless(a), b"\x1b[0;1mab\x1b[mc".to_vec());
    let bold_off = b"\x1b[0;38;2;1;2;3;48;2;40;50;60mab\x1b[38;2;9;9;9mc";
    assert_ne!(colourless(a), colourless(bold_off), "bold still counts");
}

/// The walk's oracle for a sequence under way (`UNDER_WAY`, read from each scene's keys) and the code's own
/// (`Editor::sequence_under_way`, what `handle_mouse` ends on a press) agree on every scene — every name in the list a
/// scene's.
#[test]
fn the_scenes_under_way_are_listed() {
    if !pty("the_scenes_under_way_are_listed") {
        return;
    }
    let all: Vec<Scene> = SCENES.iter().chain(MOUSE_SCENES).copied().collect();
    let unknown: Vec<&&str> = UNDER_WAY.iter().filter(|n| !all.iter().any(|s| s.0 == **n)).collect();
    assert!(unknown.is_empty(), "listed, no such scene: {unknown:?}");
    let off: Vec<String> = all
        .iter()
        .filter_map(|&(scene, setup)| {
            let (listed, says) = (UNDER_WAY.contains(&scene), fresh(setup).sequence_under_way());
            (listed != says).then(|| format!("{scene}: listed {listed}, sequence_under_way {says}"))
        })
        .collect();
    assert!(off.is_empty(), "{}", off.join("\n"));
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

/// What still reacts against its kind (the five kinds — CLAUDE.md's mouse bullet, the user 2026-10-28, #215/#216), as
/// the kinds walk meets it: `act Surface/Part` (a hint drawn: `hint Surface/Part · key meaning`). Each is a gap a
/// landing fills (#197 a lit word's colour, B the rows' hover and the hints as buttons, C the wheel); a list that only
/// shrinks — one no longer met fails until it's taken off.
const KIND_GAPS: &[&str] = &[
    // B: a waiting card's peek (its digit: #200 lights the answers, not the peeks), the keycast's overlay (its text
    // changes under the pointer)
    "hover Card/Answer",
    "hover Overlay/Row",
    // B: a list's rows, a mark, a chip light under the pointer
    "hover Completion/Row",
    "hover EditReview/Row",
    "hover Floatlist/Row",
    "hover List/Row",
    "hover Picker/Row",
    "hover Prompt/Row",
    "hover Rail/Row",
    "hover Side/Row",
    "hover Toast/Row",
    "hover Welcome/Row",
    "hover WhichKey/Row",
    "hover Rail/Mark",
    "hover Status/Word(Now)",
    "hover Chat/Word(With)",
    // B: #216, the tab in front pressed gives the slot the keys
    "press SlotLine/Word(Tab)",
    // B: a key hint is a button, or plain faint words where its key costs
    "hint Chat/Blank · / search",
    "hint Chat/Blank · C-c clear",
    "hint Chat/Blank · C-s screen",
    "hint Chat/Blank · backspace take",
    "hint Chat/Blank · enter quote",
    "hint Chat/Blank · y copy",
    "hint Ask/Blank · esc stop",
    "hint Edge/Border · / search",
    "hint Edge/Border · enter quote",
    "hint Edge/Border · l log",
    "hint Edge/Border · o buffer",
    "hint Edge/Border · q hide",
    "hint Edge/Border · tab detail",
    "hint Edge/Border · y copy",
    "hint Float/Frame(Move) · / search",
    "hint Float/Frame(Move) · G latest",
    "hint Float/Frame(Move) · enter go",
    "hint Float/Frame(Move) · enter switch",
    "hint Float/Frame(Move) · esc code",
    "hint Float/Frame(Move) · o new",
    "hint Float/Frame(Move) · q close",
    "hint Float/Frame(Move) · tab output",
    "hint Float/Frame(Move) · y copy",
    "hint Float/Frame(Size) · enter done",
    "hint Float/Frame(Size) · h j k l move",
    "hint FollowCard/Blank · C-c clear",
    "hint NoteCard/Blank · :note-close",
    "hint NoteCard/Blank · ]n next",
    "hint NoteCard/Blank · space N all",
    "hint NoteCard/Blank · space n reply",
    "hint Output/Blank · y copy",
    "hint Picker/Outside · C-x close",
    "hint Picker/Outside · enter open",
    "hint Picker/Outside · esc close",
    "hint Picker/Outside · type narrow",
    "hint Prompt/Outside · enter run",
    "hint Prompt/Outside · enter search",
    "hint Prompt/Outside · esc close",
    "hint Prompt/Outside · tab complete",
    "hint Rail/Blank · ]F file",
    "hint Rail/Blank · ]g hunk",
    "hint Rail/Blank · ]u unviewed",
    "hint Rail/Blank · enter open",
    "hint Rail/Blank · esc code",
    "hint Rail/Blank · j k files",
    "hint Rail/Blank · m viewed,",
    "hint Rail/Blank · q end",
    "hint Rail/Blank · space n comment",
    "hint Rail/Blank · space u undo",
    "hint Rail/Blank · space v q keep",
    "hint Rail/Blank · space v u undo",
    "hint Rail/Blank · u next",
    "hint Side/Blank · enter open",
    "hint Side/Blank · esc code",
    "hint Side/Blank · q close",
    "hint Side/Blank · space e files",
    "hint Side/Blank · space g back",
    "hint Side/Blank · space o outline",
    "hint Side/Blank · tab git",
    "hint Status/Blank · C-d scroll",
    "hint Status/Blank · C-u scroll",
    "hint Status/Blank · C-x complete",
    "hint Status/Blank · enter accept",
    "hint Status/Blank · esc close",
    "hint Status/Blank · tab select",
    // C: a line half under the pointer while a window prefix is under way
    "hover Edge/Border",
    "hover Float/Frame(Move)",
];

/// The lines tarae draws surfaces and frames with.
const LINES: &str = "─│┬┴├┤┼╭╮╰╯";

/// A frame drawn in colour (as the terminal shows it): its rows' cells and each cell's (fg, bg).
fn painted(ed: &mut Editor) -> (Vec<Vec<char>>, Vec<(vt100::Color, vt100::Color)>) {
    crossterm::style::force_color_output(true);
    ed.focus_sync();
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, W as u16, H as u16).unwrap();
    let mut vt = vt100::Parser::new(H as u16, W as u16, 0);
    vt.process(&bytes);
    let text = crate::screen::text(&bytes, W as u16, H as u16).unwrap_or_default();
    let mut rows: Vec<Vec<char>> = text.lines().map(cells).collect();
    rows.resize(H, Vec::new());
    let s = vt.screen();
    let colours = (0..H * W)
        .map(|i| s.cell((i / W) as u16, (i % W) as u16).map_or_default(|c| (c.fgcolor(), c.bgcolor())))
        .collect();
    (rows, colours)
}

/// `u` as the gaps name it: its surface without its ids, its part's kind.
fn short(u: Under) -> String {
    let surface = format!("{:?}", u.surface);
    format!("{}/{}", surface.split('(').next().unwrap_or_default(), part_kind(u.part))
}

/// The kinds walk over one scene: each gap met — its name (`KIND_GAPS`' form) and where — and the kinds met. Every key
/// hint shown is a button; under the pointer a button's ground lightens (its text's colour stays: #197's, judged once it
/// lands), a reveal's card shows, text and none change nothing, a handle lights; a press on a button acts, on a reveal
/// doesn't, on text puts the cursor — the pointer's only where the frame holds still.
fn kinds_scene(scene: &str, setup: fn(&mut Editor), n: usize) -> (Vec<(String, String)>, Vec<&'static str>) {
    use crate::kinds::Kind;
    let mut ed = fresh(setup);
    let (rows, base) = painted(&mut ed);
    // (a frame that moves by itself — a draft written as it comes: its hints judged, not what the pointer changes)
    let still = !scene.contains("being written") && painted(&mut ed) == (rows.clone(), base.clone());
    let mut gaps: Vec<(String, String)> = Vec::new();
    let mut gap = |name: String, what: String| {
        if !gaps.iter().any(|g| g.0 == name) {
            gaps.push((name, format!("{scene} · {what}")));
        }
    };
    let mut met = Vec::new();
    // Every key hint shown is a button: its key's first cell is one a press acts on
    let hints = ed.screen.hints.borrow().clone();
    for h in hints {
        let row = &rows[h.y];
        let shown: String =
            row.get(h.from..h.to).unwrap_or_default().iter().filter(|c| **c != '\0').collect();
        if shown != h.key {
            continue;
        }
        let word = |c: &&char| **c != ' ' && !LINES.contains(**c);
        let meaning: String = row.get(h.to + 1..).unwrap_or_default().iter().take_while(word).collect();
        let u = ed.mouse_at(h.from, h.y);
        // (under a prompt, a picker or pointing a press anywhere else only closes it — but their own keys, on the status
        // line, are theirs)
        let modal = u.part == Part::Outside || u.surface == Surface::Pointing;
        if modal && h.y + 1 != H {
            continue;
        }
        if !matches!(ed.kind_at(h.from, h.y), Kind::Button(_)) {
            let at =
                format!("{} {meaning} at {},{} is {}", h.key, h.from, h.y, ed.kind_at(h.from, h.y).word());
            gap(format!("hint {} · {} {meaning}", short(u), h.key).trim_end().to_string(), at);
        }
    }
    if !still {
        return (gaps, met);
    }
    // Under the pointer, each thing as its kind
    let text = rows.iter().map(|r| r.iter().filter(|c| **c != '\0').collect::<String>()).collect::<Vec<_>>();
    let (targets, _) = targets(&ed, &text.join("\n"));
    for &((x, y), u) in &targets {
        let kind = ed.kind_at(x, y);
        if !met.contains(&kind.word()) {
            met.push(kind.word());
        }
        ed.handle_event(Event::Hover(x as u16, y as u16));
        let (_, over) = painted(&mut ed);
        let (was, now) = (base[y * W + x], over[y * W + x]);
        // (a surface's lines light half under the pointer whatever's on them — its own rule, not its elements')
        let line = rows[y].get(x).is_some_and(|c| LINES.contains(*c));
        // (a line lit full already by the keys of what it heads, a floating frame's by its own)
        let frames = ed.screen.frames.borrow().clone();
        let keyed = holds(&ed, u)
            || frames
                .iter()
                .rev()
                .find(|f| {
                    (f.rect.0..f.rect.0 + f.rect.2).contains(&x)
                        && (f.rect.1..f.rect.1 + f.rect.3).contains(&y)
                })
                .is_some_and(|f| f.lit == Lit::Keys);
        let off = match kind {
            Kind::Button(_) if was.1 == now.1 => Some("its ground stays under the pointer"),
            // (#197a: only its ground lightens)
            Kind::Button(_) if was.0 != now.0 => Some("its text changes colour under the pointer"),
            Kind::Reveal if ed.cut_hover.is_none() || over == base => Some("no card under the pointer"),
            Kind::Text | Kind::None if was != now && !line => Some("it changes under the pointer"),
            Kind::Handle if was == now && !keyed => Some("it doesn't light under the pointer"),
            _ => None,
        };
        if let Some(what) = off {
            gap(format!("hover {}", short(u)), format!("{} at {x},{y} ({}): {what}", short(u), kind.word()));
        }
    }
    // A press, on one of each surface's (the walk's sample)
    let mut walk = Walk::default();
    for ((x, y), u) in sample(targets, n) {
        let kind = ed.kind_at(x, y);
        let mut ed = fresh(setup);
        let s0 = snap(&mut ed);
        let b0 = before(&ed, u, (x, y));
        send(&mut ed, K::Down, x, y, scene, &mut walk);
        send(&mut ed, K::Up, x, y, scene, &mut walk);
        let d = did(&s0, &snap(&mut ed), Act::Press, u);
        // (a slot's tab in front: done only where the slot has the keys — #216, the walk's `holds` takes it as done)
        let done = match u.part {
            Part::Word(crate::mouse::Word::Tab(_)) => fresh(setup).bottom_has_keys(),
            _ => b0.holds,
        };
        let off = match kind {
            Kind::Button(_) if !(d.acted || d.said || done || b0.pending) => Some("a press does nothing"),
            // (its surface given the keys — a card focused, said on it — is the surface's rule, not an act of the reveal's;
            // a cut row that is text itself — blame, a folded thought — puts the cursor, as its text does)
            Kind::Reveal if d.selected && !matches!(u.part, Part::Text { .. }) => Some("a press acts"),
            Kind::Text if !(d.acted || b0.empty || b0.holds) => Some("a press puts no cursor"),
            _ => None,
        };
        if let Some(what) = off {
            gap(
                format!("press {}", short(u)),
                format!("{} at {x},{y} ({}): {what} — {}", short(u), kind.word(), d.what),
            );
        }
    }
    (gaps, met)
}

/// The kinds walk over every scene (the walk's, 120×40): what reacts against its kind, by name, and the kinds met.
fn kinds_walk() -> (Vec<(String, String)>, Vec<&'static str>) {
    let (mut gaps, mut met): (Vec<(String, String)>, Vec<&'static str>) = (Vec::new(), Vec::new());
    for (n, &(scene, setup)) in SCENES.iter().chain(MOUSE_SCENES).enumerate() {
        crate::clock::hold();
        let (g, m) = kinds_scene(scene, setup, n);
        crate::clock::release();
        gaps.extend(g.into_iter().filter(|g| !gaps.iter().any(|h| h.0 == g.0)).collect::<Vec<_>>());
        met.extend(m.into_iter().filter(|k| !met.contains(k)).collect::<Vec<_>>());
    }
    (gaps, met)
}

/// Every element reacts as its kind (the five kinds — CLAUDE.md, the mouse's bullet, the user 2026-10-28): in every
/// scene, every key hint shown is a button, and each thing under the pointer — hovered and pressed — acts as its kind
/// says (`kinds_scene`); what doesn't yet is `KIND_GAPS`, a list that only shrinks.
#[test]
fn every_element_reacts_as_its_kind() {
    if !pty("every_element_reacts_as_its_kind") {
        return;
    }
    let (gaps, met) = kinds_walk();
    let new: Vec<String> = (gaps.iter())
        .filter(|g| !KIND_GAPS.contains(&g.0.as_str()))
        .map(|g| format!("{:?} — {}", g.0, g.1))
        .collect();
    assert!(new.is_empty(), "{} not as their kind:\n{}", new.len(), new.join("\n"));
    let gone: Vec<&&str> = KIND_GAPS.iter().filter(|k| !gaps.iter().any(|g| g.0 == **k)).collect();
    assert!(gone.is_empty(), "met no more — take them off KIND_GAPS: {gone:?}");
    // (each kind met somewhere — a walk that lost one doesn't pass by not looking)
    for k in ["button", "reveal", "text", "handle", "none"] {
        assert!(met.contains(&k), "no {k} met: {met:?}");
    }
}

/// The kinds walk sees what it's for, broken once: every key hint drawn with no button (`wordspot::UNBUTTONED`) — the
/// chat's `esc code`, a slot's `C-w j scroll` — is a hint the walk names, none of them on `KIND_GAPS`.
#[test]
fn the_kinds_walk_sees_a_hint_drawn_with_no_button() {
    if !pty("the_kinds_walk_sees_a_hint_drawn_with_no_button") {
        return;
    }
    let scene = |name: &str| *SCENES.iter().chain(MOUSE_SCENES).find(|s| s.0 == name).expect("the scene");
    let (name, setup) = scene("the chat, empty");
    crate::clock::hold();
    let whole = kinds_scene(name, setup, 0).0;
    crate::wordspot::UNBUTTONED.set(true);
    let broken = kinds_scene(name, setup, 0);
    crate::wordspot::UNBUTTONED.set(false);
    crate::clock::release();
    let hint = |g: &(String, String)| g.0.starts_with("hint ") && !KIND_GAPS.contains(&g.0.as_str());
    assert!(!whole.iter().any(hint), "whole, no hint off its list: {whole:?}");
    let broken = broken.0;
    assert!(broken.iter().any(hint), "every hint unbuttoned, none named: {broken:?}");
}
