//! What's cut is whole where you look at it: a tool's row, the queued line, a run panel's command — cut to one row,
//! whole in the transcript, whole on screen while in hand (the view mode's cursor, the mouse, a panel's keys).

use super::*;
use crate::chat::{Chat, Look, Msg, Role};
use crate::chatdoc::Of;
use crate::config::Config;
use crate::event::Event;
use crate::key::{Code, Key};

const W: u16 = 140;
const H: u16 = 34;

/// A command far wider than the chat's row, its last word the one to look for.
const CMD: &str = "cargo fmt --all && git diff --stat | tail -3 && cargo clippy --all-targets -q 2>&1 | grep -E \
                   'warning|error' | head -20 && echo farewell";

fn run_row(cmd: &str) -> Msg {
    let look = Look { verb: "run", what: cmd.into(), pattern: true, range: String::new(), link: None };
    Msg::tool(look, "call-1")
}

/// The chat up and focused on a question, the run, an answer.
fn chat(msgs: Vec<Msg>) -> Editor {
    let mut ed = Editor::new(Config::default());
    ed.docs[0].text = ropey::Rope::from_str("fn main() {}\n");
    let mut c = Chat::new();
    c.msgs = msgs;
    ed.workspace.claude.chat = Some(c);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    frame(&mut ed);
    ed
}

fn convo() -> Vec<Msg> {
    vec![
        Msg::new(Role::User, "tidy it", ""),
        run_row(CMD),
        Msg::new(Role::Assistant, "Done, nothing left.", ""),
        Msg::new(Role::Note, "2.0s", ""),
    ]
}

fn frame(ed: &mut Editor) -> String {
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, W, H).unwrap();
    let mut p = vt100::Parser::new(H, W, 0);
    p.process(&bytes);
    p.screen().contents()
}

fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(key);
        frame(ed);
    }
}

/// The rows the last frame laid out for `of`.
fn rows_of(ed: &Editor, of: Of) -> usize {
    ed.screen.chat_laid.borrow().iter().find(|(o, _)| *o == of).map_or(0, |(_, r)| r.len())
}

fn transcript_text(ed: &Editor) -> String {
    crate::chatdoc::transcript(ed)
        .and_then(|t| t.pane.doc_in())
        .map(|d| d.text.to_string())
        .unwrap_or_default()
}

/// Where the cut row `cut` is on the last frame: its first row and column.
fn spot(ed: &Editor, cut: &Cut) -> (usize, usize) {
    let spots = ed.screen.cut_spots.borrow();
    let s = spots.iter().find(|s| s.3 == *cut).unwrap_or_else(|| panic!("{cut:?} recorded as cut"));
    (s.1, s.0)
}

/// The view mode's cursor onto a cut tool row draws it whole — as many rows as it takes, nothing above it moved;
/// off it, one row again. The transcript holds its words whole all along: `/` finds a word past the cut, `x` `y`
/// copies the command whole.
#[test]
fn a_cut_tool_row_is_whole_under_the_view_modes_cursor() {
    let mut ed = chat(convo());
    let screen = frame(&mut ed);
    assert!(!screen.contains("farewell"), "cut:\n{screen}");
    assert!(screen.lines().any(|l| l.contains("▸ run") && l.trim_end().ends_with('…')), "its `…`:\n{screen}");
    assert_eq!(rows_of(&ed, Of::Msg(1)), 1, "one row");
    assert!(transcript_text(&ed).contains(&format!("run    {CMD}\n")), "the transcript's line is whole");
    let (_, y) = spot(&ed, &Cut::Call("call-1".into()));
    let above = screen.lines().nth(y - 1).unwrap().to_string();
    feed(&mut ed, "<esc>/farewell<ret>");
    let screen = frame(&mut ed);
    assert!(screen.contains("farewell"), "whole in hand:\n{screen}");
    assert!(rows_of(&ed, Of::Msg(1)) > 2, "wrapped over its rows");
    assert_eq!(screen.lines().nth(y - 1).unwrap(), above, "the row over it stays");
    // (its rows after the first hang under the command, the knot on the first only)
    let row = |k: usize| screen.lines().nth(y + k).unwrap().to_string();
    let col = |l: &str| l.find("cargo").map(|b| l[..b].chars().count());
    let hang = col(&row(0)).unwrap();
    assert_eq!(row(1).chars().take(hang).filter(|c| !c.is_whitespace()).count(), 1, "only the gutter's line");
    assert!(row(1).chars().nth(hang).is_some_and(|c| !c.is_whitespace()), "under the command:\n{screen}");
    // (`x` `y` copies the line whole)
    feed(&mut ed, "xy");
    let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the copy's result");
    ed.handle_event(ev);
    assert!(crate::clipboard::written(&format!("run    {CMD}\n")), "the command whole");
    // (off it: one row again)
    feed(&mut ed, "<esc>gg");
    let screen = frame(&mut ed);
    assert_eq!(rows_of(&ed, Of::Msg(1)), 1, "folded:\n{screen}");
    assert!(!screen.contains("farewell"));
}

/// A row that isn't cut is drawn as it is with the view mode's cursor on it — nothing to unfold, nothing shifts: a
/// short run's verdict and where it ran, a change's stamps, the spot a click on where it ran takes.
#[test]
fn a_row_that_isnt_cut_stays_as_it_is_under_the_cursor() {
    let mut run = Msg::tool(
        Look { verb: "run", what: "cargo test".into(), pattern: true, range: "exit 101".into(), link: None },
        "call-r",
    );
    run.ran = Some(crate::chat::Ran::of(&crate::sandbox::Guard::Outside, None, None, Vec::new()));
    let mut edit = Msg::tool(
        Look { verb: "edit", what: "src/a.rs".into(), pattern: false, range: String::new(), link: None },
        "call-e",
    );
    edit.stamps = vec![crate::stamp::Stamp('y')];
    let mut ed =
        chat(vec![Msg::new(Role::User, "fix it", ""), run, edit, Msg::new(Role::Assistant, "Done.", "")]);
    feed(&mut ed, "<esc>gg");
    let before = frame(&mut ed);
    let ran = ed.screen.chat_ran.borrow().clone();
    let row_of = |s: &str, word: &str| s.lines().find(|l| l.contains(word)).unwrap_or_default().to_string();
    for (word, at) in [("cargo test", "/cargo<ret>"), ("src/a.rs", "/a\\.rs<ret>")] {
        feed(&mut ed, at);
        let screen = frame(&mut ed);
        assert_eq!(row_of(&screen, word), row_of(&before, word), "{word}: drawn as it was");
        assert_eq!(*ed.screen.chat_ran.borrow(), ran, "where it ran: the same spot");
        assert!(ed.screen.cut_spots.borrow().is_empty(), "nothing cut");
    }
}

/// The mouse over a cut row draws it whole — the rows over it stay, those under it go down — a move that changes
/// nothing draws nothing, and off it, it folds.
#[test]
fn the_mouse_over_a_cut_row_draws_it_whole() {
    let mut ed = chat(convo());
    ed.workspace.viewer.at = crate::workspace::At::Code;
    let screen = frame(&mut ed);
    let cut = Cut::Call("call-1".into());
    let (x, y) = spot(&ed, &cut);
    let above = screen.lines().nth(y - 1).unwrap().to_string();
    let first = screen.lines().nth(y).unwrap().to_string();
    ed.handle_event(Event::Hover(x as u16 + 3, y as u16));
    assert!(!ed.unchanged, "taken in hand: a frame");
    let screen = frame(&mut ed);
    assert!(screen.contains("farewell"), "whole:\n{screen}");
    assert_eq!(screen.lines().nth(y - 1).unwrap(), above, "the row over it stays");
    let head = |l: &str| l.split("cargo").next().unwrap_or_default().to_string();
    assert_eq!(head(screen.lines().nth(y).unwrap()), head(&first), "its first row where it was");
    // (along its rows: still in hand, nothing to draw)
    ed.handle_event(Event::Hover(x as u16 + 3, y as u16 + 1));
    assert!(ed.unchanged, "a move that changes nothing draws nothing");
    ed.handle_event(Event::Hover(0, 0));
    assert!(!ed.unchanged);
    let screen = frame(&mut ed);
    assert!(!screen.contains("farewell"), "folded again:\n{screen}");
    assert_eq!(ed.cut_hover, None);
}

/// A note's words are its row's suffix: in hand they wrap with the rest, every row within the chat's width, all of
/// them there.
#[test]
fn a_notes_words_wrap_whole_in_hand() {
    let gist = "the cut rows keep their whole words in the transcript and unfold where you look at them, wrapped \
                under their target, every row inside the chat, and then goodbye";
    let look =
        Look { verb: "note", what: "src/whole.rs:12".into(), pattern: false, range: gist.into(), link: None };
    let mut ed = chat(vec![Msg::new(Role::User, "note it", ""), Msg::tool(look, "call-n")]);
    let screen = frame(&mut ed);
    assert!(!screen.contains("goodbye"), "cut:\n{screen}");
    feed(&mut ed, "<esc>/goodbye<ret>");
    let screen = frame(&mut ed);
    assert!(screen.contains("goodbye"), "whole:\n{screen}");
    let w = ed.screen.chat_view.get().unwrap().1;
    let laid = ed.screen.chat_laid.borrow();
    let rows = &laid.iter().find(|(o, _)| *o == Of::Msg(1)).unwrap().1;
    assert!(rows.len() > 2, "wrapped");
    for r in rows.iter() {
        let used: usize =
            r.spans.iter().map(|s| unicode_width::UnicodeWidthStr::width(s.text.as_str())).sum();
        assert!(used <= w, "a row of {used} cells in {w}:\n{screen}");
    }
    let text = crate::chatdoc::lines_of(rows).0;
    assert!(text.contains(gist), "its words whole: {text:?}");
}

/// A note of several lines keeps them all: the transcript has every word (its lines one paragraph there), the folded
/// row cuts them, and in hand each line starts a row of its own.
#[test]
fn a_notes_lines_are_all_kept_and_each_starts_a_row_in_hand() {
    let gist = "the first line of the note\nand its second line, then goodbye";
    let look =
        Look { verb: "note", what: "src/whole.rs:12".into(), pattern: false, range: gist.into(), link: None };
    let mut ed = chat(vec![Msg::new(Role::User, "note it", ""), Msg::tool(look, "call-n")]);
    let screen = frame(&mut ed);
    assert!(!screen.contains("goodbye"), "cut:\n{screen}");
    assert!(transcript_text(&ed).contains("the first line of the note and its second line, then goodbye"));
    feed(&mut ed, "<esc>/goodbye<ret>");
    let screen = frame(&mut ed);
    assert!(
        screen.lines().any(|l| l
            .trim_start()
            .trim_start_matches('│')
            .trim_start()
            .starts_with("and its second")),
        "its second line starts a row:\n{screen}"
    );
    assert!(screen.contains("goodbye"), "whole:\n{screen}");
}

/// In a full chat the row the mouse holds at its foot — the queued line is always last — goes up just enough to show
/// it whole, the mouse still on it (its rows went up under the mouse): it stays in hand.
#[test]
fn the_last_row_held_in_a_full_chat_shows_whole() {
    let msg = "Your walk-perf run sat 30 min with no output, stop it and look at what it waited on before you run it \
               again, then say goodbye";
    let long = (0..30).map(|i| format!("Paragraph {i} of the answer.")).collect::<Vec<_>>().join("\n\n");
    let mut ed = chat(vec![Msg::new(Role::User, "walk it", ""), Msg::new(Role::Assistant, long, "")]);
    ed.workspace.viewer.at = crate::workspace::At::Code;
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.state = crate::chat::State::Thinking;
    c.queue(msg.to_string(), false);
    let before = frame(&mut ed);
    let (x, y) = spot(&ed, &Cut::Queued);
    ed.handle_event(Event::Hover(x as u16, y as u16));
    let screen = frame(&mut ed);
    assert!(rows_of(&ed, Of::Queued) > 1, "unfolded");
    assert!(screen.contains("goodbye"), "its last row in sight:\n{screen}");
    assert_eq!(ed.cut_hover, Some(Cut::Queued), "the mouse still on it");
    // (the row under the mouse is one of its rows; the rows under it, the input's, where they were)
    assert!(ed.screen.cut_spots.borrow().iter().any(|s| s.0 == y && s.3 == Cut::Queued));
    for (i, (was, now)) in before.lines().zip(screen.lines()).enumerate().skip(y + 1) {
        assert_eq!(was, now, "row {i} under it moved:\n{screen}");
    }
    frame(&mut ed);
    assert_eq!(ed.cut_hover, Some(Cut::Queued), "and stays, frame after frame");
}

/// A row the mouse holds folds once it moves from under a mouse that stays put (what came above it pushed it up), and
/// when the terminal loses the focus.
#[test]
fn a_held_row_folds_when_it_leaves_a_still_mouse() {
    let msgs = || {
        let long = (0..30).map(|i| format!("Paragraph {i} of the answer.")).collect::<Vec<_>>().join("\n\n");
        vec![
            Msg::new(Role::Assistant, long, ""),
            Msg::new(Role::User, "tidy it", ""),
            run_row(CMD),
            Msg::new(Role::Assistant, "Done.", ""),
        ]
    };
    let mut ed = chat(msgs());
    ed.workspace.viewer.at = crate::workspace::At::Code;
    frame(&mut ed);
    let (x, y) = spot(&ed, &Cut::Call("call-1".into()));
    ed.handle_event(Event::Hover(x as u16 + 3, y as u16));
    frame(&mut ed);
    assert!(rows_of(&ed, Of::Msg(2)) > 1, "whole under the mouse");
    // (what comes pushes it up from under the mouse: folded)
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::User, "and more?", ""));
    c.msgs.push(Msg::new(Role::Assistant, "One.\n\nTwo.\n\nThree.\n\nFour.", ""));
    let screen = frame(&mut ed);
    assert_ne!(ed.cut_hover, Some(Cut::Call("call-1".into())));
    assert_eq!(rows_of(&ed, Of::Msg(2)), 1, "folded:\n{screen}");
    // (the focus gone, the mouse with it)
    let mut ed = chat(msgs());
    ed.workspace.viewer.at = crate::workspace::At::Code;
    frame(&mut ed);
    ed.handle_event(Event::Hover(x as u16 + 3, y as u16));
    assert!(ed.cut_hover.is_some());
    ed.handle_event(Event::Focus(false));
    assert_eq!(ed.cut_hover, None);
}

/// A press on the queued line, `thinking…` drawn before it: the frame's row is the transcript's own — its view mode,
/// the cursor in its words, and so in hand, whole.
#[test]
fn a_press_on_the_queued_line_lands_in_its_words() {
    let msg = "Your walk-perf run sat 30 min with no output, stop it and look at what it waited on before you run it \
               again, then say goodbye";
    let mut ed = chat(vec![Msg::new(Role::User, "walk it", "")]);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.state = crate::chat::State::Thinking;
    c.queue(msg.to_string(), false);
    frame(&mut ed);
    let (x, y) = spot(&ed, &Cut::Queued);
    let m = crate::event::Mouse {
        kind: crate::event::MouseKind::Down,
        x: (x + 10) as u16,
        y: y as u16,
        alt: false,
        ctrl: false,
    };
    ed.handle_mouse(m);
    let screen = frame(&mut ed);
    let t = crate::chatdoc::transcript(&ed).unwrap();
    let d = t.pane.doc_in().unwrap();
    let line = crate::movement::line_of(&d.text, d.selection().primary().cursor(&d.text));
    assert!(crate::chatdoc::has_keys(&ed), "its view mode");
    assert_eq!(d.text.line(line).to_string().trim_end(), msg, "the cursor in the queued words");
    assert!(rows_of(&ed, Of::Queued) > 1 && screen.contains("goodbye"), "whole:\n{screen}");
}

/// The queued line is the transcript's last, after the frame's own rows: its words whole there (what's queued is the
/// drawer's), cut on screen, whole under the mouse; the input's line is a line again.
#[test]
fn the_queued_line_is_whole_in_the_transcript_and_in_hand() {
    let msg = "Your walk-perf run (target/walk) sat 30 min with no output, stop it and look at what it waited on \
               before you run it again, then say goodbye";
    let mut ed = chat(vec![Msg::new(Role::User, "walk it", "")]);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.state = crate::chat::State::Thinking;
    c.queue(msg.to_string(), false);
    let screen = frame(&mut ed);
    let text = transcript_text(&ed);
    assert_eq!(text.lines().last(), Some(msg), "its words, whole, the transcript's last line");
    let rows: Vec<&str> = screen.lines().collect();
    let queued = rows.iter().position(|l| l.contains("queued Your walk-perf")).expect("its row");
    let thinking = rows.iter().position(|l| l.contains("thinking")).expect("thinking…");
    assert!(thinking < queued, "after the frame's own rows:\n{screen}");
    assert!(!screen.contains("goodbye"), "cut:\n{screen}");
    let line =
        rows.iter().rposition(|l| l.contains("────") && !l.contains("claude")).expect("the input's line");
    assert!(!rows[line].contains("queued"), "the input's line a line again:\n{screen}");
    let (x, y) = spot(&ed, &Cut::Queued);
    assert_eq!(y, queued);
    ed.handle_event(Event::Hover(x as u16, y as u16));
    let screen = frame(&mut ed);
    assert!(screen.contains("goodbye"), "whole in hand:\n{screen}");
    assert_eq!(transcript_text(&ed), text, "the same words, folded or not");
}

/// The run card stays inside the slot's columns: on a narrow, short screen, past its cap, its last row (`K`) fitted —
/// nothing printed past the edge wraps onto the line under it; a slot with no room for a card shows none.
#[test]
fn the_run_card_stays_inside_a_narrow_slot() {
    // (row by row: `contents()` joins a row that wrapped with the next — a row run past the edge would hide there)
    let render = |ed: &mut Editor, w: u16, h: u16| {
        let mut bytes = Vec::new();
        crate::term::render(ed, &mut bytes, w, h).unwrap();
        let mut p = vt100::Parser::new(h, w, 0);
        p.process(&bytes);
        p.screen().rows(0, w).collect::<Vec<String>>().join("\n")
    };
    let cmd =
        "CARGO_TARGET_DIR=target/whole cargo test -q --bin tarae chatdoc 2>&1 | grep -E 'result|FAILED' \
               && cargo clippy --all-targets -q && echo and then farewell"
            .repeat(3);
    let mut ed = Editor::new(Config::default());
    ed.docs[0].text = ropey::Rope::from_str(&"fn main() {}\n".repeat(30));
    let task = crate::task::Task { name: cmd.clone(), cmd, cwd: "/nowhere".into() };
    let mut r = crate::task::Run::for_test(task);
    r.state = crate::task::State::Passed;
    r.push_line(false, "test result: ok".into(), false);
    ed.workspace.task_run = Some(r);
    for (w, h) in [(30u16, 12u16), (12, 12)] {
        let before = render(&mut ed, w, h);
        let Some((x, y)) =
            ed.screen.cut_spots.borrow().iter().find(|s| s.3 == Cut::Run(Slot::Task)).map(|s| (s.1, s.0))
        else {
            assert_eq!(w, 12, "its label cut at {w}:\n{before}");
            continue;
        };
        ed.handle_event(Event::Hover(x as u16, y as u16));
        let screen = render(&mut ed, w, h);
        if w == 12 {
            assert_eq!(screen, before, "no room for a card: none");
        } else {
            assert!(screen.contains("$ CARGO") && screen.contains('K'), "its card, past its cap:\n{screen}");
            for (i, (was, now)) in before.lines().zip(screen.lines()).enumerate().skip(y) {
                assert_eq!(was, now, "row {i} under the card changed — a row ran past the edge:\n{screen}");
            }
        }
        ed.handle_event(Event::Hover(0, 0));
    }
}

/// A run panel's command cut to its tab's label: with the keys in the panel nothing pops up, and `K` says it whole;
/// the mouse over the label shows it whole on a card right over the line — nothing else on the frame moves — a move
/// along the label draws nothing, and the mouse leaving takes the card away. (A run that ended: its time stands.)
#[test]
fn a_run_panels_command_is_whole_on_a_card_over_its_line() {
    let scratch = crate::scratch::Scratch::new("whole-run");
    let dir = std::fs::canonicalize(scratch.path()).unwrap();
    let mut ed = Editor::new(Config::default());
    ed.workspace.root = dir.clone();
    ed.docs[0].text = ropey::Rope::from_str(&"fn main() {}\n".repeat(30));
    let cmd = "CARGO_TARGET_DIR=target/whole cargo test -q --bin tarae chatdoc 2>&1 | grep -E 'result|FAILED' \
               && echo and then farewell";
    let task = crate::task::Task { name: cmd.into(), cmd: cmd.into(), cwd: dir.clone() };
    let mut r = crate::task::Run::for_test(task);
    r.state = crate::task::State::Passed;
    r.by_claude = true;
    r.ran = Some(crate::chat::Ran::of(&crate::sandbox::Guard::Outside, Some(7), None, Vec::new()));
    r.push_line(false, "test result: ok".into(), false);
    ed.workspace.task_run = Some(r);
    let before = frame(&mut ed);
    assert!(!before.contains("farewell"), "its label cut:\n{before}");
    let (x, y) = spot(&ed, &Cut::Run(Slot::Task));
    // (the keys in the panel: nothing pops up — `K` says it whole)
    ed.bottom_take_keys();
    let screen = frame(&mut ed);
    assert!(!screen.contains("$ CARGO"), "no card with the keys:\n{screen}");
    ed.handle_key("K".parse().unwrap());
    let said = ed.messages.last().map(|m| m.2.clone()).unwrap_or_default();
    assert!(said.starts_with(&format!("{cmd} — ran outside")), "{said}");
    ed.workspace.viewer.at = crate::workspace::At::Code;
    let before = frame(&mut ed);
    // (the mouse over the label: the card, right over the line, its words whole)
    ed.handle_event(Event::Hover(x as u16, y as u16));
    assert!(!ed.unchanged, "a frame");
    let screen = frame(&mut ed);
    let rows: Vec<&str> = screen.lines().collect();
    let top =
        rows.iter().position(|l| l.contains("$ CARGO_TARGET_DIR")).unwrap_or_else(|| panic!("{screen}"));
    assert!(top < y, "over the line:\n{screen}");
    assert!(rows[top..y].iter().any(|l| l.contains("farewell")), "whole, right over the line:\n{screen}");
    assert!(!rows[y - 1].trim().is_empty(), "touching the line");
    // (nothing else on the frame moved: the rows over the card, the line and the panel under it)
    for (i, (was, now)) in before.lines().zip(screen.lines()).enumerate() {
        if i < top || i >= y {
            assert_eq!(was, now, "row {i} moved:\n{screen}");
        }
    }
    // (along the label: nothing to draw; away: gone, the frame as it was)
    ed.handle_event(Event::Hover(x as u16 + 1, y as u16));
    assert!(ed.unchanged, "a move that changes nothing draws nothing");
    ed.handle_event(Event::Hover(0, 0));
    assert!(!ed.unchanged);
    assert_eq!(frame(&mut ed), before, "gone as the mouse leaves");
}
