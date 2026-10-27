//! What's cut is whole where you look at it, one way: a tool's row, the queued line, a line's end, a list's row — cut
//! to one row, whole in the transcript, whole on a card over it while in hand (the mouse, the view mode's cursor, a
//! list's selected row) — nothing under it moves; a run panel's command on its own card over its line.

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

/// Every cut tarae draws is recorded (frictions #187, U2): in `term.rs` a text is cut by `whole::fit` or `fit_left` — a
/// `whole::Fit`, its whole carried to its print, which records it where it stands (`whole::at`, `cut_at`, a span's in
/// `chat_spans_at`; its text leaves only by `shown`: U2's review) —, `fit_part` (a part of a whole) or `fit_own` (its own
/// card or a word painted again: `OWN`, each named) — a bare `fit_ellipsis`, `fit_left`, `fit_middle`, `fit_keeping`,
/// `fit_place` or `fit_place_marked` only inside the fitting helpers (`HELPERS`), or handed to `whole::of` on its line or
/// the next (a `Fit` as `fit`'s).
/// What this can't see — a cut that reaches no print — the type holds, and where it lands the tests below
/// (`whole_fresh_and_cached`) and the walk (`every_cut_text_is_whole_under_the_mouse`).
#[test]
fn every_cut_tarae_draws_is_recorded() {
    let bare = bare_cuts(&crate::sim::lint::sources());
    assert!(bare.is_empty(), "cut, not recorded for the mouse — `whole::fit`:\n{}", bare.join("\n"));
}

/// Broken once: a bare cut in a drawer counts — a `crate::cut::` one too —; one in a helper, `whole::`'s and one handed
/// to `whole::of` don't.
#[test]
fn a_bare_cut_counts() {
    let src = "fn draw_x(out: &mut W) {\n    let a = fit_ellipsis(s, 4);\n    let b = crate::cut::fit_left(s, 4);\n    \
               let c = crate::whole::fit(s, 4);\n    let (d, m) = fit_middle(s, &[], 4);\n    \
               let d = crate::whole::of(d, s);\n}\nfn fit_keeping(s: &str) -> String {\n    fit_ellipsis(s, 2)\n}\n";
    let files = [("term.rs".to_string(), src.to_string())];
    let bare = bare_cuts(&files);
    assert_eq!(bare.len(), 2, "{bare:?}");
    assert!(bare[0].contains("draw_x") && bare[0].contains("fit_ellipsis(s, 4)"), "{bare:?}");
    assert!(bare[1].contains("cut::fit_left"), "{bare:?}");
    let elsewhere = [("chat.rs".to_string(), src.to_string())];
    assert!(bare_cuts(&elsewhere).is_empty(), "only what term.rs draws");
}

/// The fitting helpers: what they cut, their callers record.
const HELPERS: &[&str] = &["fit_middle", "fit_keeping", "fit_path_left", "fold_hints"];

/// What `term.rs` draws, a file at a time: its name, its lines, and the drawer each line is in.
fn drawers(files: &[(String, String)]) -> Vec<(&str, Vec<&str>, Vec<&str>)> {
    let top = regex::Regex::new(r"^(?:pub(?:\([\w:]+\))? )?fn (\w+)").unwrap();
    let drawn = files.iter().filter(|(f, _)| f == "term.rs" || f.starts_with("term/"));
    drawn
        .map(|(file, text)| {
            let lines: Vec<&str> = text.lines().collect();
            let mut within = "";
            let mut drawer = Vec::with_capacity(lines.len());
            for line in &lines {
                if let Some(c) = top.captures(line) {
                    within = c.get(1).map_or("", |m| m.as_str());
                } else if line.starts_with("impl") {
                    within = "";
                }
                drawer.push(within);
            }
            (file.as_str(), lines, drawer)
        })
        .collect()
}

/// Each bare cut in what `term.rs` draws: `file:line in fn: the line`.
fn bare_cuts(files: &[(String, String)]) -> Vec<String> {
    let call = regex::Regex::new(
        r"(^|[^\w:]|cut::)(fit_ellipsis|fit_left|fit_middle|fit_keeping|fit_place|fit_place_marked)\(",
    )
    .unwrap();
    let mut out = Vec::new();
    for (file, lines, drawer) in drawers(files) {
        let handed = |k: usize| lines.get(k).is_some_and(|l| l.contains("whole::of("));
        for (i, line) in lines.iter().enumerate() {
            if call.is_match(line) && !HELPERS.contains(&drawer[i]) && !handed(i) && !handed(i + 1) {
                out.push(format!("{file}:{} in {}: {}", i + 1, drawer[i], line.trim()));
            }
        }
    }
    out
}

/// The cuts `fit_own` makes — a plain string, nothing carried to its print, so what's wrapped in it passes the rule above
/// (U2's third review): each drawer that makes one, how many, and why its cut needs no record. The list only shrinks —
/// lower a count as you turn one into `whole::fit` (as the clock's and sleep's lints); a new one is a `whole::fit`.
const OWN: &[(&str, usize, &str)] = &[
    ("slot_header", 1, "a task's tab: its command whole on its own card (`Cut::Run`, `run_card`)"),
    ("member_line", 1, "a member's name lit again over the top line's print, which recorded its cut"),
    ("code_line", 1, "a buffer's name lit again over the top line's print, which recorded its cut"),
    ("now_segments", 2, "the now line's step and plan: its own card (`Cut::Now`, `now_card`)"),
    ("draw_statusline", 1, "the now line's sentence: its own card (`Cut::Now`, `now_card`)"),
    ("draw_welcome", 1, "the start screen's folder inside its foot, whose `whole::of` holds both whole"),
    ("draw_follow_card", 1, "a word of the card's top line lit again over its print, which recorded its cut"),
    ("draw_sidebar", 1, "a tab's name lit again over the top line's print, which recorded its cut"),
    ("rail_board", 1, "a name inside a row whose `fit_part` holds the row whole"),
    (
        "draw_chat",
        2,
        "a top line's detail lit again; the `with` chip: its own card (`Cut::With`, `with_card`)",
    ),
    ("run_card", 1, "a row of the run card, which is the command whole (`K` says all of it)"),
];

/// Each drawer's `fit_own` cuts and how many, in the order they're met.
fn own_cuts(files: &[(String, String)]) -> Vec<(String, usize)> {
    let call = regex::Regex::new(r"(^|[^\w])fit_own\(").unwrap();
    let mut out: Vec<(String, usize)> = Vec::new();
    for (_, lines, drawer) in drawers(files) {
        for (line, within) in lines.iter().zip(drawer) {
            let n = call.find_iter(line).count();
            match out.last_mut() {
                _ if n == 0 => {}
                Some(last) if last.0 == within => last.1 += n,
                _ => out.push((within.to_string(), n)),
            }
        }
    }
    out
}

/// Every `fit_own` is named (`OWN`), its count as many as there are: a new one fails (make it a `whole::fit`), one gone
/// fails until its count is lowered.
#[test]
fn every_own_cut_is_named_and_the_list_only_shrinks() {
    let counted = own_cuts(&crate::sim::lint::sources());
    let listed: Vec<(String, usize)> = OWN.iter().map(|o| (o.0.to_string(), o.1)).collect();
    assert_eq!(
        counted, listed,
        "`whole::fit_own` by drawer — a new one: `whole::fit`; one gone: lower `OWN`"
    );
}

/// Broken once: a `fit_own` counts in its drawer, `whole::fit_own` and a bare one alike; `fit_owner` doesn't.
#[test]
fn an_own_cut_counts_in_its_drawer() {
    let src = "fn draw_x() {\n    let a = crate::whole::fit_own(s, 4);\n    let b = fit_own(s, 2);\n    \
               let c = fit_owner(s);\n}\nfn draw_y() {\n    let d = fit_own(s, 1);\n}\n";
    let counted = own_cuts(&[("term.rs".to_string(), src.to_string())]);
    assert_eq!(counted, vec![("draw_x".to_string(), 2), ("draw_y".to_string(), 1)]);
}

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

/// The card as drawn on the last frame (x, y, width, height).
fn card(ed: &Editor) -> Option<(usize, usize, usize, usize)> {
    ed.screen.cut_card.borrow().as_ref().map(|c| c.rect)
}

/// Nothing moved: every row of `now` above `upto` the card doesn't cover is `before`'s, and on the rows it covers, the
/// cells left of it too.
fn nothing_moved(before: &str, now: &str, (x, y, _, h): (usize, usize, usize, usize), upto: usize) {
    // (the top line's turn timer aside)
    for (i, (was, is)) in before.lines().zip(now.lines()).enumerate().take(upto).skip(1) {
        match (y..y + h).contains(&i) {
            false => assert_eq!(was, is, "row {i} moved:\n{now}"),
            true => assert_eq!(
                was.chars().take(x).collect::<String>(),
                is.chars().take(x).collect::<String>(),
                "row {i} left of the card moved:\n{now}"
            ),
        }
    }
}

/// The view mode's cursor onto a cut tool row shows it whole on a card over it — its text where it started, its rows
/// after the first hanging under it; the row itself still one row, nothing else moved — and off it, the card goes. The
/// transcript holds its words whole all along: `/` finds a word past the cut, `x` `y` copies the command whole.
#[test]
fn a_cut_tool_row_is_whole_on_a_card_under_the_view_modes_cursor() {
    let mut ed = chat(convo());
    let screen = frame(&mut ed);
    assert!(!screen.contains("farewell"), "cut:\n{screen}");
    assert!(screen.lines().any(|l| l.contains("▸ run") && l.trim_end().ends_with('…')), "its `…`:\n{screen}");
    assert_eq!(rows_of(&ed, Of::Msg(1)), 1, "one row");
    assert!(transcript_text(&ed).contains(&format!("run    {CMD}\n")), "the transcript's line is whole");
    let (y, a, ..) = cut_text(&ed, "farewell").expect("recorded as cut");
    assert!(screen.lines().nth(y).unwrap().chars().skip(a).collect::<String>().starts_with("cargo fmt"));
    feed(&mut ed, "<esc>gg");
    let before = frame(&mut ed);
    feed(&mut ed, "/farewell<ret>");
    let screen = frame(&mut ed);
    assert!(screen.contains("farewell"), "whole in hand:\n{screen}");
    assert_eq!(rows_of(&ed, Of::Msg(1)), 1, "never unfolds");
    let rect = card(&ed).expect("its card");
    assert!(rect.3 > 1, "wrapped over rows of its own");
    let (y, a, ..) = cut_text(&ed, "farewell").unwrap();
    // (the hints under the input say the selection the search made)
    nothing_moved(&before, &screen, rect, H as usize - 4);
    // (its text where it started, its rows after the first hanging under it)
    let row = |k: usize| screen.lines().nth(y + k).unwrap().chars().skip(a).collect::<String>();
    assert!(row(0).starts_with("cargo fmt"), "where it started:\n{screen}");
    assert!(row(1).starts_with(|c: char| !c.is_whitespace()), "under the command:\n{screen}");
    // (`x` `y` copies the line whole)
    feed(&mut ed, "xy");
    let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the copy's result");
    ed.handle_event(ev);
    assert!(crate::clipboard::written(&format!("run    {CMD}\n")), "the command whole");
    // (off it: no card)
    feed(&mut ed, "<esc>gg");
    let screen = frame(&mut ed);
    assert!(!screen.contains("farewell"), "gone:\n{screen}");
    assert_eq!(card(&ed), None);
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

/// A cut `run` row that ran (`…  ok` at its end) in hand: its card covers the row to its end — nothing of the cut row
/// shows past it — and the view mode's cursor on it stays in sight, painted on the card.
#[test]
fn a_cut_rows_card_covers_it_to_its_end_and_keeps_the_cursor() {
    crossterm::style::force_color_output(true);
    // (its first row breaks short of the chat's edge: a long word after it goes to the next)
    let mut ran = run_row("cargo fmt --all && ./check_every_strand_tied_once.sh --all-of-them");
    ran.ran = Some(crate::chat::Ran::of(&crate::sandbox::Guard::Outside, None, None, Vec::new()));
    let mut ed = chat(vec![Msg::new(Role::User, "tidy it", ""), ran, Msg::new(Role::Assistant, "Done.", "")]);
    feed(&mut ed, "<esc>gg/cargo fmt<ret>");
    let mut bytes = Vec::new();
    crate::term::render(&mut ed, &mut bytes, W, H).unwrap();
    let mut vt = vt100::Parser::new(H, W, 0);
    vt.process(&bytes);
    let screen = vt.screen().contents();
    let (x, y, w, _) = card(&ed).expect("its card");
    let row: String = screen.lines().nth(y).unwrap().chars().skip(x + w).collect();
    assert!(row.trim().is_empty(), "the cut row shows past its card: {row:?}\n{screen}");
    // (the cursor on the card: its cell in the cursor's ground, not the card's)
    let &(cx, cy) = ed.screen.cursors.borrow().iter().find(|c| c.1 == y).expect("its cursor on the row");
    assert!((x..x + w).contains(&cx), "under the card");
    let bg = |x: usize| vt.screen().cell(cy as u16, x as u16).unwrap().bgcolor();
    assert_ne!(bg(cx), bg(x + w - 1), "the cursor painted on the card");
}

/// The mouse over a cut row shows it whole on its card — nothing else moves; the mouse gone onto the card keeps it (a
/// move that changes nothing draws nothing), and off both, the frame as it was.
#[test]
fn the_mouse_over_a_cut_row_shows_it_whole_on_a_card() {
    let mut ed = chat(convo());
    ed.workspace.viewer.at = crate::workspace::At::Code;
    let before = frame(&mut ed);
    let (y, x, ..) = cut_text(&ed, "farewell").expect("recorded as cut");
    ed.handle_event(Event::Hover(x as u16 + 3, y as u16));
    assert!(!ed.unchanged, "taken in hand: a frame");
    let screen = frame(&mut ed);
    assert!(screen.contains("farewell"), "whole:\n{screen}");
    assert_eq!(rows_of(&ed, Of::Msg(1)), 1, "never unfolds");
    nothing_moved(&before, &screen, card(&ed).expect("its card"), H as usize);
    // (onto its card's next row: still in hand, nothing to draw)
    ed.handle_event(Event::Hover(x as u16 + 3, y as u16 + 1));
    assert!(ed.unchanged, "a move that changes nothing draws nothing");
    assert!(ed.cut_hover.is_some(), "the card keeps it");
    ed.handle_event(Event::Hover(0, 0));
    assert!(!ed.unchanged);
    assert_eq!(frame(&mut ed), before, "gone as the mouse leaves");
    assert_eq!(ed.cut_hover, None);
}

/// A note's words are its row's suffix: on its card they wrap with the rest, the card inside the chat, all of them
/// there.
#[test]
fn a_notes_words_wrap_whole_on_its_card() {
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
    let (x, _, w, h) = card(&ed).expect("its card");
    let chat_w = ed.screen.chat_view.get().unwrap().1;
    assert!(h > 2, "wrapped");
    assert!(x + w <= W as usize && x + chat_w + 4 >= W as usize, "inside the chat: {x}+{w}:\n{screen}");
    let (_, _, _, whole) = cut_text(&ed, "goodbye").unwrap();
    assert!(whole.text.contains(gist), "its words whole: {:?}", whole.text);
}

/// A note of several lines keeps them all: the transcript has every word (its lines one paragraph there), the folded
/// row cuts them, and on its card each line starts a row of its own.
#[test]
fn a_notes_lines_are_all_kept_and_each_starts_a_row_on_its_card() {
    let gist = "the first line of the note\nand its second line, then goodbye";
    let look =
        Look { verb: "note", what: "src/whole.rs:12".into(), pattern: false, range: gist.into(), link: None };
    let mut ed = chat(vec![Msg::new(Role::User, "note it", ""), Msg::tool(look, "call-n")]);
    let screen = frame(&mut ed);
    assert!(!screen.contains("goodbye"), "cut:\n{screen}");
    assert!(transcript_text(&ed).contains("the first line of the note and its second line, then goodbye"));
    feed(&mut ed, "<esc>/goodbye<ret>");
    let screen = frame(&mut ed);
    let (x, ..) = card(&ed).expect("its card");
    assert!(
        screen.lines().any(|l| l
            .chars()
            .skip(x)
            .collect::<String>()
            .trim_start()
            .starts_with("and its second")),
        "its second line starts a row:\n{screen}"
    );
    assert!(screen.contains("goodbye"), "whole:\n{screen}");
}

/// In a full chat the row the mouse holds at its foot — the queued line is always last — shows whole on its card, the
/// chat's rows where they were; it stays in hand, frame after frame.
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
    let (y, x, ..) = cut_text(&ed, "goodbye").expect("recorded as cut");
    ed.handle_event(Event::Hover(x as u16, y as u16));
    let screen = frame(&mut ed);
    assert_eq!(rows_of(&ed, Of::Queued), 1, "never unfolds");
    assert!(screen.contains("goodbye"), "whole:\n{screen}");
    nothing_moved(&before, &screen, card(&ed).expect("its card"), H as usize);
    let held = ed.cut_hover.clone();
    assert!(matches!(&held, Some(Cut::Text(w)) if w.text.contains("goodbye")), "the mouse still on it");
    frame(&mut ed);
    assert_eq!(ed.cut_hover, held, "and stays, frame after frame");
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
    let (y, x, ..) = cut_text(&ed, "farewell").expect("recorded as cut");
    ed.handle_event(Event::Hover(x as u16 + 3, y as u16));
    let screen = frame(&mut ed);
    assert!(screen.contains("farewell"), "whole under the mouse:\n{screen}");
    // (what comes pushes it up from under the mouse: gone)
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(Msg::new(Role::User, "and more?", ""));
    c.msgs.push(Msg::new(Role::Assistant, "One.\n\nTwo.\n\nThree.\n\nFour.", ""));
    frame(&mut ed);
    let screen = frame(&mut ed);
    assert!(!matches!(&ed.cut_hover, Some(Cut::Text(w)) if w.text.contains("farewell")));
    assert!(!screen.contains("farewell"), "gone:\n{screen}");
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
/// the cursor in its words, and so in hand, whole on its card.
#[test]
fn a_press_on_the_queued_line_lands_in_its_words() {
    let msg = "Your walk-perf run sat 30 min with no output, stop it and look at what it waited on before you run it \
               again, then say goodbye";
    let mut ed = chat(vec![Msg::new(Role::User, "walk it", "")]);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.state = crate::chat::State::Thinking;
    c.queue(msg.to_string(), false);
    frame(&mut ed);
    let (y, x, ..) = cut_text(&ed, "goodbye").expect("recorded as cut");
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
    assert!(rows_of(&ed, Of::Queued) == 1 && screen.contains("goodbye"), "whole:\n{screen}");
    assert!(card(&ed).is_some(), "on its card");
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
    let (y, x, ..) = cut_text(&ed, "goodbye").expect("recorded as cut");
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

/// The cut text recorded on the last frame (`cut_at`) whose whole holds `word`: its row, its columns, its whole.
fn cut_text(ed: &Editor, word: &str) -> Option<(usize, usize, usize, Whole)> {
    let spots = ed.screen.cut_spots.borrow();
    spots.iter().find_map(|s| match &s.3 {
        Cut::Text(w) if w.text.contains(word) => Some((s.0, s.1, s.2, (**w).clone())),
        _ => None,
    })
}

/// The code with one line and a diagnostic on it saying `message`.
fn diagnosed(message: &str) -> Editor {
    let mut ed = Editor::new(Config::default());
    ed.docs[0].text = ropey::Rope::from_str("fn main() {}\n\nfn two() {}\n");
    ed.docs[0].lsp.diagnostics = vec![crate::lsp::Diagnostic {
        from: 3,
        to: 7,
        severity: 1,
        message: message.into(),
        raw: serde_json::json!({}),
        source: 0,
    }];
    frame(&mut ed);
    ed
}

/// frictions #178: a diagnostic cut to `…` at its line's end — the mouse over it shows it whole on a card over it, its
/// first row on the cut one and its text where it started (nothing else moves: the code's rows as they were), wrapped
/// in its pane's columns where they're too narrow; off it, the frame as it was. A whole one records nothing.
#[test]
fn the_mouse_over_a_cut_line_end_shows_it_whole_on_a_card() {
    let long = format!("{} and farewell", "mismatched types: expected `usize`, found `String` ".repeat(4));
    let mut ed = diagnosed(&long);
    let before = frame(&mut ed);
    assert!(!before.contains("farewell"), "cut:\n{before}");
    let (y, a, b, w) = cut_text(&ed, "farewell").expect("recorded as cut");
    assert_eq!(w.text, long);
    let row = before.lines().nth(y).unwrap();
    let start: String = row.chars().skip(a).take(12).collect();
    assert!(long.starts_with(&start), "its spot is where its text is drawn: {start:?} in {row:?}");
    assert!(row.chars().nth(b - 1) == Some('…'), "its spot ends at the `…`: {row:?}");
    ed.handle_event(Event::Hover((a + 3) as u16, y as u16));
    assert!(!ed.unchanged, "in hand: a frame");
    let screen = frame(&mut ed);
    assert!(screen.contains("farewell"), "whole:\n{screen}");
    let now = screen.lines().nth(y).unwrap();
    assert_eq!(
        now.chars().take(a).collect::<String>(),
        row.chars().take(a).collect::<String>(),
        "the code stays"
    );
    assert!(
        now.chars().skip(a).collect::<String>().starts_with(&start),
        "its text where it started:\n{screen}"
    );
    for (i, (was, is)) in before.lines().zip(screen.lines()).enumerate().take(y) {
        assert_eq!(was, is, "row {i} over it moved");
    }
    ed.handle_event(Event::Hover(0, 0));
    assert_eq!(ed.cut_hover, None);
    assert_eq!(frame(&mut ed), before, "gone as the mouse leaves");
    // (a short one isn't cut: nothing recorded, nothing to show)
    let mut ed = diagnosed("unused variable");
    frame(&mut ed);
    assert!(cut_text(&ed, "unused").is_none(), "whole already: no card");
}

/// frictions #179: a file's name cut in the side panel — whole on one row under the mouse, reaching right over the
/// code (a tooltip), its row's other cells and the panel's other rows as they were.
#[test]
fn the_mouse_over_a_cut_name_in_the_side_panel_shows_it_whole() {
    let mut ed = Editor::new(Config::default());
    let root = std::path::PathBuf::from("/r");
    let long = "a_very_long_file_name_farewell_the_panel_never_has_room_for_at_all.rs";
    let files = vec!["short.rs".to_string(), long.to_string()];
    ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::with_files(root, files));
    let before = frame(&mut ed);
    assert!(!before.contains("farewell"), "cut:\n{before}");
    let (y, a, _, w) = cut_text(&ed, "farewell").expect("recorded as cut");
    assert_eq!(w.text, long);
    assert!(cut_text(&ed, "short").is_none(), "a name that fits: nothing");
    ed.handle_event(Event::Hover((a + 2) as u16, y as u16));
    let screen = frame(&mut ed);
    let row = screen.lines().nth(y).unwrap();
    assert!(row.contains(long), "whole on its own row, over the code:\n{screen}");
    assert_eq!(row.find(long), before.lines().nth(y).unwrap().find("a_very"), "where it started");
    for (i, (was, is)) in before.lines().zip(screen.lines()).enumerate() {
        if i != y {
            assert_eq!(was, is, "row {i} moved");
        }
    }
    ed.handle_event(Event::Hover(0, 0));
    assert_eq!(frame(&mut ed), before, "gone as the mouse leaves");
}

/// A cut text under what floats over it isn't in hand: the mouse over a picker where a line end's `…` is under it
/// shows no card of it.
#[test]
fn a_cut_text_under_a_picker_is_not_in_hand() {
    let long = format!("{} farewell", "a long message about the code ".repeat(8));
    let mut ed = diagnosed(&long);
    let (y, a, ..) = cut_text(&ed, "farewell").expect("recorded as cut");
    let item = crate::picker::Item {
        label: "one".into(),
        action: crate::picker::Action::Message,
        hint: String::new(),
        glyph: None,
    };
    ed.open_picker(crate::picker::Picker::of("buffers", "things", vec![item], false), None);
    frame(&mut ed);
    let under = ed.mouse_under(crate::event::MouseKind::Down, a + 3, y).surface;
    assert!(
        matches!(under, crate::mouse::Surface::Picker | crate::mouse::Surface::PickerPreview),
        "{under:?}"
    );
    ed.handle_event(Event::Hover((a + 3) as u16, y as u16));
    assert_eq!(ed.cut_hover, None, "under the picker");
}

/// The side panel with two long names, the second selected.
fn side_panel(at: crate::workspace::At) -> Editor {
    let mut ed = Editor::new(Config::default());
    let files = vec![
        "a_first_long_file_name_that_says_hello_and_never_fits_in_the_panel.rs".to_string(),
        "a_very_long_file_name_farewell_the_panel_never_has_room_for_at_all.rs".to_string(),
    ];
    // (the code on the second: the row the panel marks without the keys too)
    ed.docs[0].path = Some(std::path::Path::new("/r").join(&files[1]));
    let mut sb = crate::sidebar::Sidebar::with_files(std::path::PathBuf::from("/r"), files);
    sb.selected = 1;
    ed.workspace.viewer.sidebar = Some(sb);
    ed.workspace.viewer.at = at;
    frame(&mut ed);
    ed
}

/// #187: a list's selected row is in hand while the list has the keys — its cut name whole on a card over it, on the
/// selected row's ground, nothing else moved; the list without the keys, none. The mouse over another cut row shows
/// that one instead — one card — and gone from it, the row in hand's again.
#[test]
fn a_lists_selected_row_is_whole_on_its_card_while_the_list_has_the_keys() {
    // (the panel's rows only: the path bar names the file whole)
    let (long, other) =
        ("farewell_the_panel_never_has_room_for_at_all.rs", "hello_and_never_fits_in_the_panel.rs");
    let row_of = |s: &str, y: usize| s.lines().nth(y).unwrap_or_default().to_string();
    let mut ed = side_panel(crate::workspace::At::Code);
    let before = frame(&mut ed);
    let (y, ..) = cut_text(&ed, "farewell").expect("marked, and cut");
    assert!(!row_of(&before, y).contains(long) && card(&ed).is_none(), "no keys, no row in hand:\n{before}");
    let mut ed = side_panel(crate::workspace::At::Side);
    let screen = frame(&mut ed);
    let (y, a, _, w) = cut_text(&ed, "farewell").expect("recorded as cut");
    assert!(row_of(&screen, y).contains(long), "whole:\n{screen}");
    assert_eq!(card(&ed).map(|c| (c.1, c.3)), Some((y, 1)), "one row, on its own");
    let row = screen.lines().nth(y).unwrap().chars().skip(a).collect::<String>();
    assert!(row.starts_with("a_very"), "where it started:\n{screen}");
    assert!(a > 0 && w.text.ends_with("at_all.rs"));
    assert_eq!(
        crate::whole::shown(&ed).map(|s| matches!(s.1, Some(Hand::Beside(_)))),
        Some(true),
        "the row in hand's (its ground the selected row's), beside its list"
    );
    // (the mouse over the other: that one, one card; gone from it, the row in hand's again)
    let (y1, a1, ..) = cut_text(&ed, "hello").expect("the other recorded as cut");
    ed.handle_event(Event::Hover((a1 + 2) as u16, y1 as u16));
    let screen = frame(&mut ed);
    assert!(row_of(&screen, y1).contains(other), "the mouse's:\n{screen}");
    assert!(!row_of(&screen, y).contains(long), "one card:\n{screen}");
    ed.handle_event(Event::Hover(0, H - 1));
    let screen = frame(&mut ed);
    assert!(row_of(&screen, y).contains(long), "back to the hand's:\n{screen}");
}

/// The side panel with the keys, `long` selected on its first row and two short names under it.
fn side_long(long: &str) -> Editor {
    let mut ed = Editor::new(Config::default());
    let files = vec![long.to_string(), "b_second.rs".to_string(), "c_third.rs".to_string()];
    let mut sb = crate::sidebar::Sidebar::with_files(std::path::PathBuf::from("/r"), files);
    sb.selected = 0;
    ed.workspace.viewer.sidebar = Some(sb);
    ed.workspace.viewer.at = crate::workspace::At::Side;
    frame(&mut ed);
    ed
}

/// #187, the user on U1's review: a row in hand's card never covers another row of its own list — beside a list with
/// the code free at its right, a name too long for its row runs on along it and wraps under it from the list's edge
/// rightwards only; the names under it stay in sight, and its rows read in order are the name whole.
#[test]
fn a_row_in_hand_too_long_for_its_row_wraps_beside_its_list_never_over_it() {
    let long = format!("a_{}farewell.rs", "long_segment_".repeat(24));
    let mut ed = side_long(&long);
    let screen = frame(&mut ed);
    let (y, a, ..) = cut_text(&ed, "farewell").expect("recorded as cut");
    let c = ed.screen.cut_card.borrow().clone().expect("the row in hand's card");
    assert!(c.rect.1 == y && c.rect.3 > 1, "on its row, wrapped: {c:?}\n{screen}");
    assert!(c.from > a, "its rows after the first start past the list: {c:?}\n{screen}");
    let lines: Vec<String> = screen.lines().map(str::to_string).collect();
    assert!(lines[y + 1].contains("b_second.rs"), "the next row in sight:\n{screen}");
    assert!(lines[y + 2].contains("c_third.rs"), "and the one after:\n{screen}");
    let read: String = (0..c.rect.3)
        .map(|k| {
            let from = if k == 0 { a } else { c.from + 1 };
            lines[y + k].chars().skip(from).collect::<String>().trim_end().to_string()
        })
        .collect();
    assert_eq!(read, long, "whole, in order:\n{screen}");
}

/// A picker as tall as the screen (one row free under its frame), `long` its first choice, selected.
fn picked(long: &str) -> Editor {
    let item = |label: &str| crate::picker::Item {
        label: label.into(),
        action: crate::picker::Action::Message,
        hint: String::new(),
        glyph: None,
    };
    let mut ed = Editor::new(Config::default());
    let items = vec![item(long), item("a short one"), item("another")];
    ed.open_picker(crate::picker::Picker::of("buffers", "things", items, false), None);
    frame(&mut ed);
    ed
}

/// #187, the user on U1's review: a framed list's row in hand — a picker's, a card's choices, `:granted` — is whole on a
/// card just outside its frame, over none of its rows nor the frame itself; its rows as they were. One row free there
/// and more to say: its start, then `↓ N more lines` on that row — a press there opens it whole.
#[test]
fn a_framed_lists_row_in_hand_is_whole_outside_its_frame() {
    let long = format!("choose {}farewell", "a long choice of words ".repeat(4));
    let mut ed = picked(&long);
    let screen = frame(&mut ed);
    let (y, ..) = cut_text(&ed, "farewell").expect("recorded as cut");
    let (fx, fy, _, fh) = ed.screen.picker_box.get().expect("its frame");
    let (x, top, w, h) = card(&ed).expect("the row in hand's card");
    assert!(
        top >= fy + fh || top + h <= fy,
        "outside its frame's rows: {:?} {:?}\n{screen}",
        (x, top, w, h),
        (fx, fy)
    );
    assert!(x + w <= W as usize, "on the ground there:\n{screen}");
    assert!(screen.contains("farewell"), "whole:\n{screen}");
    let lines: Vec<&str> = screen.lines().collect();
    assert!(lines[y].contains('…'), "its row as it was, cut:\n{screen}");
    assert!(lines[y + 1].contains("a short one"), "the next row in sight:\n{screen}");
    // (longer than its one row outside: never clipped unsaid)
    let longer = format!("choose {}farewell", "a long choice of words ".repeat(12));
    let mut ed = picked(&longer);
    let screen = frame(&mut ed);
    let c = ed.screen.cut_card.borrow().clone().expect("the row in hand's card");
    let row = screen.lines().nth(c.rect.1).unwrap_or_default().to_string();
    assert!(c.rect.3 == 1 && c.more == Some(c.rect.1), "one row, its more on it: {c:?}\n{screen}");
    assert!(
        row.contains("choose a long") && row.contains("more line"),
        "its start, how many more:\n{screen}"
    );
    super::pressed(&mut ed, c.rect.0 + 2, c.rect.1);
    assert!(ed.doc().read_only.is_some() && ed.doc().text == longer.as_str(), "whole, read-only");
}

/// A list whose rows say their state first (`:granted`'s), each `(name, state)`, the first selected.
fn granted_like(rows: &[(&str, &str)]) -> Editor {
    let items = (rows.iter())
        .map(|(label, hint)| crate::picker::Item {
            label: label.to_string(),
            action: crate::picker::Action::Message,
            hint: hint.to_string(),
            glyph: None,
        })
        .collect();
    let mut ed = Editor::new(Config::default());
    let mut p = crate::picker::Picker::of("buffers", "things", items, false).without_preview();
    (p.keys, p.hints_first) = (true, true);
    ed.open_picker(p, None);
    frame(&mut ed);
    ed
}

/// The card's rows as drawn (its first from its `x`, the rest from its `from`), trimmed.
fn card_rows(ed: &Editor, screen: &str) -> Option<Vec<String>> {
    let c = ed.screen.cut_card.borrow().clone()?;
    let (x, top, w, h) = c.rect;
    let rows = (screen.lines().skip(top).take(h).enumerate()).map(|(k, l)| {
        let from = if k == 0 { x } else { c.from };
        l.chars().skip(from).take(x + w - from).collect::<String>().trim().to_string()
    });
    Some(rows.collect())
}

/// #187, the user on U1's review round (2026-10-27): a framed list's row in hand takes the free ground's width under
/// its frame, not the frame's — `:granted`'s `this checkout` on one row with its name; and where it must still wrap,
/// the row's trailing state moves whole to the next row, never split.
#[test]
fn a_framed_lists_card_takes_the_grounds_width_and_never_splits_a_rows_state() {
    let state = "this checkout";
    let (_, _, fw, _) = granted_like(&[("a", state)]).screen.picker_box.get().expect("its frame");
    // (wider than the frame holds by a cell, as narrow as the ground under it is: its margins a cell or two)
    let name = format!("read /{}", "x".repeat(fw - 22));
    let mut ed = granted_like(&[(&name, state), ("read /tmp", "the temp folders")]);
    let screen = frame(&mut ed);
    let (fx, _, fw, _) = ed.screen.picker_box.get().expect("its frame");
    let (x, _, w, _) = card(&ed).expect("the row in hand's card");
    assert_eq!(card_rows(&ed, &screen), Some(vec![format!("{name}  {state}")]), "one row:\n{screen}");
    assert!(x < fx || x + w > fx + fw, "wider than its frame:\n{screen}");
    // (too long for the ground too: wrapped, its state whole on a row — after the name where it fits, else its own)
    // (its end swept across the first row's: the state where that row breaks; a name not cut has no card)
    let mut wrapped = 0;
    for i in 0..40 {
        let words = "a_segment ".repeat((fw - 30) / 10);
        let name = format!("read {} {}", "x".repeat(i), words.trim_end());
        let mut ed = granted_like(&[(&name, state)]);
        let screen = frame(&mut ed);
        let Some(rows) = card_rows(&ed, &screen) else { continue };
        wrapped += usize::from(rows.len() > 1);
        assert!(
            rows.iter().any(|r| r.ends_with(state)),
            "{state:?} whole on a row ({i}): {rows:?}\n{screen}"
        );
    }
    assert!(wrapped > 12, "wrapped where the ground doesn't hold it: {wrapped}");
}

/// #187, on U1's review: a docked surface's row in hand stands under what floats — a toast over the side panel's card
/// is read whole, and a press on it is the toast's, never the row's under the card.
#[test]
fn a_docked_row_in_hand_is_under_what_floats() {
    let long = format!("a_{}farewell.rs", "long_segment_".repeat(30));
    let mut ed = side_long(&long);
    ed.set_warning("a toast over the card");
    let screen = frame(&mut ed);
    let c = ed.screen.cut_card.borrow().clone().expect("the row in hand's card");
    let (ty, line) = screen.lines().enumerate().find(|l| l.1.contains("a toast over")).expect("read whole");
    let tx = line[..line.find("a toast").unwrap()].chars().count();
    let (cx, cy, cw, ch) = c.rect;
    assert!(
        (cy..cy + ch).contains(&ty) && (c.from..cx + cw).contains(&tx),
        "the toast where the card is: {c:?} at ({tx}, {ty})\n{screen}"
    );
    assert_eq!(super::pressed(&mut ed, tx, ty), Some((tx, ty)), "the toast's press");
}

/// #187, on U1's review: a press on a card is the press on the row it's about, whole — its drag and its release too:
/// pressed over the code's other rows and dragged there, the selection stays on the cut row.
#[test]
fn a_press_drag_and_release_on_a_card_are_the_rows() {
    use crate::event::{Mouse, MouseKind as K};
    let long: String = (1..=12).map(|i| format!("part {i} of a long message about the code ")).collect();
    let mut ed = diagnosed(&long);
    let (y, a, ..) = cut_text(&ed, "part 12").expect("recorded as cut");
    ed.handle_event(Event::Hover((a + 3) as u16, y as u16));
    frame(&mut ed);
    let (x, top, _, h) = card(&ed).expect("its card");
    assert!(top == y && h > 3, "down over the rows under it");
    let m = |kind, x: usize, y: usize| Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false };
    ed.handle_mouse(m(K::Down, x + 2, top + 2));
    ed.handle_mouse(m(K::Drag, x + 6, top + 2));
    ed.handle_mouse(m(K::Up, x + 6, top + 2));
    let d = ed.doc();
    let r = d.selection().primary();
    let line = |p: usize| d.text.byte_to_line(p);
    // (a block cursor's range ends after its character)
    assert_eq!((line(r.from()), line(r.to() - 1)), (0, 0), "on the cut row only: {r:?}");
    assert_eq!(ed.cut_press, None, "let go");
}

/// A plain press at (`x`, `y`) and its release, through `handle_mouse`, a frame after each.
fn click(ed: &mut Editor, x: usize, y: usize) {
    use crate::event::{Mouse, MouseKind as K};
    let m = |kind| Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false };
    ed.handle_mouse(m(K::Down));
    frame(ed);
    ed.handle_mouse(m(K::Up));
    frame(ed);
}

/// The row in hand's card outside its list's frame, as drawn: a cell on its text, not its `↓ N more lines`.
fn outside_card(ed: &Editor, screen: &str) -> (usize, usize) {
    let c =
        ed.screen.cut_card.borrow().clone().unwrap_or_else(|| panic!("the row in hand's card:\n{screen}"));
    assert!(
        c.outside && c.more != Some(c.rect.1),
        "outside its frame, its first row its text: {c:?}\n{screen}"
    );
    (c.rect.0 + 2, c.rect.1)
}

/// #187, U1's review: a press on a framed list's row in hand's card — outside its frame — is a press outside, as on
/// the ground it covers: the sandbox's blocked list closes, nothing chosen and no grant's card. (Its `↓ N more lines`
/// still opens it whole — `a_framed_lists_row_in_hand_is_whole_outside_its_frame`.)
#[test]
fn a_press_on_the_blocked_lists_card_outside_its_frame_closes_it_and_grants_nothing() {
    // (a write refused deep in a long folder: `allow writing in …` its first row, cut)
    let scratch = crate::scratch::Scratch::new("whole-blocked");
    let deep = (0..3).fold(scratch.to_path_buf(), |p, _| p.join("a_rather_long_folder_name"));
    std::fs::create_dir_all(&deep).unwrap();
    let watch = crate::sandbox::Watch::quiet("t.");
    watch.record(crate::sandbox::Denial {
        process: "npm".into(),
        op: "file-write-create".into(),
        target: deep.join("index-v5").display().to_string(),
    });
    let policy = crate::sandbox::Policy {
        watch: Some(watch),
        ..crate::sandbox::Policy::for_project(&crate::sandbox::Env::default())
    };
    let task =
        crate::task::Task { name: "npm test".into(), cmd: "npm test".into(), cwd: scratch.to_path_buf() };
    let mut run = crate::task::Run::for_test(task);
    run.guard = Some(crate::sandbox::Guard::In(Box::new(policy)));
    run.by_claude = true;
    let mut ed = Editor::new(Config::default());
    ed.workspace.task_run = Some(run);
    ed.sandbox_blocked(crate::blocked::Which::Either);
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    while ed.picker.is_none() {
        let left = end.saturating_duration_since(std::time::Instant::now());
        let ev = ed.events.recv_timeout(left).expect("the blocked list before the timeout");
        ed.handle_event(ev);
    }
    let screen = frame(&mut ed);
    let first = ed.picker.as_ref().and_then(|p| p.current()).map(|i| i.label.clone()).unwrap_or_default();
    assert!(first.starts_with("allow writing in"), "a grant in hand: {first:?}\n{screen}");
    let (x, y) = outside_card(&ed, &screen);
    click(&mut ed, x, y);
    assert!(ed.picker.is_none(), "closed, as a press outside it");
    assert!(
        ed.offers.is_empty(),
        "nothing chosen, no grant's card: {:?}",
        ed.offers.first().map(|o| &o.title)
    );
}

/// #187, U1's review: the prompt's candidate in hand (`:theme`'s, cycled to) — a press on its card outside the
/// prompt's frame closes the prompt, as a press outside it, never the candidate picked.
#[test]
fn a_press_on_the_prompts_card_outside_its_frame_closes_it() {
    let long = format!("a_theme_{}farewell", "with_a_long_name_".repeat(10));
    let mut ed = Editor::new(Config::default());
    ed.cmd_lists.themes = Some(vec![long.clone(), "another".into()]);
    feed(&mut ed, ":theme <tab>");
    let screen = frame(&mut ed);
    assert_eq!(ed.prompt.as_ref().map(|p| p.text()), Some(format!("theme {long}")), "cycled to it");
    let (x, y) = outside_card(&ed, &screen);
    click(&mut ed, x, y);
    assert!(ed.prompt.is_none(), "closed, as a press outside it — not its candidate picked:\n{screen}");
}

/// #187, U1's review: completion's item in hand — a press on its card outside the menu's frame never accepts it.
#[test]
fn a_press_on_completions_card_outside_its_frame_accepts_nothing() {
    let long = format!("a_word_{}farewell", "with_a_long_name_".repeat(10));
    let mut ed = Editor::new(Config::default());
    ed.docs[0].text = ropey::Rope::from_str("fn main() {}\n");
    feed(&mut ed, "A");
    let items = serde_json::json!([{"label": long, "detail": "i32"}, {"label": "short", "detail": "i32"}]);
    let at = ed.doc().selection().primary().head;
    let mut c = crate::completion::Completion::new(0, ed.doc().id, at, items);
    c.filter("");
    c.selected = Some(0);
    ed.completion = Some(c);
    let screen = frame(&mut ed);
    let (x, y) = outside_card(&ed, &screen);
    click(&mut ed, x, y);
    let text = ed.doc().text.to_string();
    assert!(!text.contains("farewell"), "not accepted: {text:?}\n{screen}");
}

/// #187: a text past `CUT_CARD_MAX` rows — its card's last row says how many more; a press there opens it whole in a
/// read-only buffer (pressed again, the same one); a press elsewhere on the card is the press on the row it's about.
#[test]
fn a_long_cut_says_how_many_more_lines_and_a_press_opens_it_whole() {
    // (one line, wrapped to far more rows than a card takes)
    let long: String = (1..=40).map(|i| format!("part {i} of a long message about the code ")).collect();
    let long = format!("{long}farewell");
    let mut ed = diagnosed(&long);
    let (y, a, b, _) = cut_text(&ed, "farewell").expect("recorded as cut");
    ed.handle_event(Event::Hover((a + 3) as u16, y as u16));
    let screen = frame(&mut ed);
    let (x, top, w, h) = card(&ed).expect("its card");
    assert_eq!(h, crate::term::CUT_CARD_MAX, "as tall as a card goes:\n{screen}");
    let more = ed.screen.cut_card.borrow().as_ref().and_then(|c| c.more).expect("its more row");
    assert_eq!(more, top + h - 1);
    assert!(screen.lines().nth(more).unwrap().contains("more lines"), "says so:\n{screen}");
    // (a press elsewhere on it: the press on its row, inside its cut text; off it, where it was)
    assert_eq!(super::pressed(&mut ed, x + 2, top + 2), Some(((x + 2).clamp(a, b - 1), y)));
    assert_eq!(super::pressed(&mut ed, x + w + 1, top + 2), Some((x + w + 1, top + 2)));
    let press = |ed: &mut Editor, x: usize, y: usize| {
        let kind = crate::event::MouseKind::Down;
        let m = crate::event::Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false };
        ed.handle_mouse(m);
        ed.handle_mouse(crate::event::Mouse { kind: crate::event::MouseKind::Up, ..m });
    };
    // (on its more row, the mouse gone onto it: the whole, read-only)
    ed.handle_event(Event::Hover((x + 2) as u16, more as u16));
    assert!(ed.cut_hover.is_some(), "the card keeps it");
    frame(&mut ed);
    press(&mut ed, x + 2, more);
    let d = ed.doc();
    assert_eq!(d.title.as_deref(), Some("whole · code"));
    assert!(d.read_only.is_some() && d.text == long.as_str(), "whole, read-only");
    assert_eq!(ed.cut_hover, None, "the card gone with its act");
    let n = ed.docs.len();
    ed.current = 0;
    frame(&mut ed);
    ed.handle_event(Event::Hover((a + 3) as u16, y as u16));
    frame(&mut ed);
    press(&mut ed, x + 2, more);
    assert_eq!(ed.docs.len(), n, "the same buffer");
}

/// U2's sweep (#187): a cut on a surface no walk meets — its text recorded where it's printed (`whole::at`), whole on a
/// card under the mouse, gone as it leaves: `on` the surface it's recorded on, `word` the end only the whole has.
fn whole_under_the_mouse(ed: &mut Editor, on: On, word: &str) -> String {
    let before = frame(ed);
    assert!(!before.contains(word), "cut:\n{before}");
    let (y, a, b, w) = cut_text(ed, word).unwrap_or_else(|| panic!("recorded as cut:\n{before}"));
    assert_eq!(w.on, on, "on what it's drawn on");
    let row = before.lines().nth(y).unwrap();
    assert_eq!(row.chars().nth(b - 1), Some('…'), "its spot ends at the `…`: {row:?}");
    assert!(a < b);
    ed.handle_event(Event::Hover(((a + b) / 2) as u16, y as u16));
    let screen = frame(ed);
    assert!(screen.contains(word), "whole:\n{screen}");
    ed.handle_event(Event::Hover(0, H - 1));
    assert_eq!(frame(ed), before, "gone as the mouse leaves");
    w.text
}

#[test]
fn a_cut_toasts_message_is_whole_under_the_mouse() {
    let mut ed = Editor::new(Config::default());
    let long = format!("the build stopped: {} and farewell", "a reason that runs on ".repeat(16));
    ed.set_error(format!("{long}\nsecond line\nthird line\nfourth line"));
    // (shown, at full strength)
    let fresh = std::time::Instant::now().checked_sub(std::time::Duration::from_millis(500)).unwrap();
    for t in &mut ed.toasts {
        (t.at, t.shown) = (fresh, true);
    }
    let whole = whole_under_the_mouse(&mut ed, On::Toast, "farewell");
    assert!(whole.contains("fourth line"), "the message whole, every line: {whole:?}");
}

/// The user on U2's shots (2026-10-28): a cut toast's card is the toast unfolded where it stands — over it whole, its bar
/// too, the kind's `▎` beside the card's first row, its words where they were — whichever way it grows: down from its
/// top row where the toast is high, up over it from the status line where it's low.
#[test]
fn a_cut_toasts_card_is_the_toast_unfolded_where_it_stands() {
    for low in [true, false] {
        let mut ed = Editor::new(Config::default());
        ed.docs[0].text = ropey::Rope::from_str(&"fn main() {}\n".repeat(40));
        // (the cursor off the top's rows: the toast there)
        if !low {
            feed(&mut ed, "20G");
        }
        let long = format!("the build stopped: {} and farewell", "a reason that runs on ".repeat(16));
        ed.set_error(format!("{long}\nsecond line\nthird line"));
        let fresh = std::time::Instant::now().checked_sub(std::time::Duration::from_millis(500)).unwrap();
        for t in &mut ed.toasts {
            (t.at, t.shown) = (fresh, true);
        }
        let before = frame(&mut ed);
        let (tx, ty, tw, th) = ed.screen.toast_card.get().expect("its toast");
        assert_eq!(ty > H as usize / 2, low, "the toast where it's meant to be:\n{before}");
        let (y, a, b, _) = cut_text(&ed, "farewell").expect("recorded as cut");
        ed.handle_event(Event::Hover(((a + b) / 2) as u16, y as u16));
        let screen = frame(&mut ed);
        let (x, top, w, h) = card(&ed).expect("its card");
        assert!(
            x <= tx && x + w >= tx + tw && top <= ty && top + h >= ty + th,
            "over the toast whole: {:?} over {:?}\n{screen}",
            (x, top, w, h),
            (tx, ty, tw, th)
        );
        match low {
            true => assert!(top < ty, "up over it:\n{screen}"),
            false => assert_eq!(top, ty, "from its top row:\n{screen}"),
        }
        let lines: Vec<String> = screen.lines().map(str::to_string).collect();
        let first: String = lines[top].chars().skip(tx).collect();
        assert!(
            first.starts_with(" ▎ the build stopped"),
            "its bar beside its first row, its words where they were:\n{screen}"
        );
        let bars = (top..top + h).filter(|&r| lines[r].chars().skip(x).take(w).any(|c| c == '▎')).count();
        assert_eq!(bars, 1, "one bar:\n{screen}");
        assert!(screen.contains("farewell"), "whole:\n{screen}");
    }
}

/// The user on U2's shots (2026-10-28): a card that ends inside a word of what's under it reaches on to that word's end,
/// its ground over the rest — no letter of the word after the card: the side panel's row in hand over the code, its end
/// swept across the code's words.
#[test]
fn a_card_never_ends_inside_a_word_under_it() {
    let code =
        "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau\n";
    let letter = |s: &str, x: usize| s.chars().nth(x).is_some_and(char::is_alphanumeric);
    let mut reached = 0;
    for i in 0..12 {
        let long = format!("a_{}_{}.rs", "x".repeat(i), "name_".repeat(8));
        let side = |at| {
            let mut ed = Editor::new(Config::default());
            ed.docs[0].text = ropey::Rope::from_str(&code.repeat(20));
            let files = vec![long.clone(), "b_second.rs".to_string()];
            let mut sb = crate::sidebar::Sidebar::with_files(std::path::PathBuf::from("/r"), files);
            sb.selected = 0;
            ed.workspace.viewer.sidebar = Some(sb);
            ed.workspace.viewer.at = at;
            ed
        };
        let before = frame(&mut side(crate::workspace::At::Code));
        let mut ed = side(crate::workspace::At::Side);
        let screen = frame(&mut ed);
        let c = ed.screen.cut_card.borrow().clone().expect("the row in hand's card");
        for (k, &end) in c.ends.iter().enumerate() {
            let (under, now) =
                (before.lines().nth(c.rect.1 + k).unwrap(), screen.lines().nth(c.rect.1 + k).unwrap());
            assert!(
                !(letter(under, end - 1) && letter(under, end)),
                "ends inside a word ({i}, its row {k} at {end}):\n{under}\n{now}"
            );
            assert_eq!(now.chars().nth(end - 1), Some(' '), "its ground to there:\n{now}");
            reached += usize::from(letter(under, end - 1));
        }
    }
    assert!(reached > 0, "the sweep ended inside a word somewhere");
}

/// The user, 2026-10-28 (U2's fifth review): a word is Helix's `w`'s — over a path, a URL, a hash under the side panel's
/// row in hand the card stops at the word's punctuation, never on to the screen's edge, and a word running on more than
/// `WORD_REACH` past it (the hash) leaves the card at its own end, the rest showing. What it holds (`Card::holds`) is what
/// it draws: its last cell its, the next the code's as before.
#[test]
fn a_card_reaches_a_words_end_never_past_its_punctuation_nor_twelve_cells() {
    use crate::movement::{CharClass, char_class};
    let code = "src/ui/card_view.rs https://x.io/a 0123456789abcdef0123456789abcdef01234567\n";
    let word = |s: &str, x: usize| s.chars().nth(x).is_some_and(|c| char_class(c) == CharClass::Word);
    // (the word's cells from `x` on)
    let run = |s: &str, x: usize| s.chars().skip(x).take_while(|&c| char_class(c) == CharClass::Word).count();
    let (mut at_punct, mut capped) = (0, 0);
    for i in 0..60 {
        let long = format!("a_{}_{}.rs", "x".repeat(i), "name_".repeat(8));
        let side = |at| {
            let mut ed = Editor::new(Config::default());
            ed.docs[0].text = ropey::Rope::from_str(&code.repeat(20));
            let files = vec![long.clone(), "b_second.rs".to_string()];
            let mut sb = crate::sidebar::Sidebar::with_files(std::path::PathBuf::from("/r"), files);
            sb.selected = 0;
            ed.workspace.viewer.sidebar = Some(sb);
            ed.workspace.viewer.at = at;
            ed
        };
        let before = frame(&mut side(crate::workspace::At::Code));
        let mut ed = side(crate::workspace::At::Side);
        let screen = frame(&mut ed);
        let Some(c) = ed.screen.cut_card.borrow().clone() else { continue };
        for (k, &end) in c.ends.iter().enumerate() {
            let y = c.rect.1 + k;
            let (under, now) = (before.lines().nth(y).unwrap(), screen.lines().nth(y).unwrap());
            let on = run(under, end);
            assert!(
                !(word(under, end - 1) && on > 0) || on > crate::screen::WORD_REACH,
                "ends inside a word it could reach ({i}, its row {k} at {end}):\n{under}\n{now}"
            );
            assert!(end < W as usize, "never on to the screen's edge ({i}, row {k}):\n{now}");
            assert_eq!(now.chars().nth(end - 1), Some(' '), "its ground to there:\n{now}");
            let rest = |s: &str| s.chars().skip(end).take(6).collect::<String>();
            assert_eq!(rest(now), rest(under), "the code after it as it was ({i}, row {k}):\n{now}");
            assert!(c.holds(&ed, end - 1, y) && !c.holds(&ed, end, y), "holds what it draws ({i}, row {k})");
            at_punct +=
                usize::from(word(under, end - 1) && !word(under, end) && under.chars().nth(end) != Some(' '));
            capped += usize::from(word(under, end - 1) && on > crate::screen::WORD_REACH);
        }
    }
    assert!(at_punct > 0 && capped > 0, "the sweep met punctuation ({at_punct}) and the hash ({capped})");
}

#[test]
fn a_pages_title_cut_is_whole_under_the_mouse() {
    let mut ed = Editor::new(Config::default());
    let id = crate::web::page::stand_in(&mut ed, "https://docs.example.com/guide", "Its text.\n");
    let title = format!("web · docs.example.com › {} and farewell", "A Guide to Everything ".repeat(6));
    let d = ed.docs.iter_mut().find(|d| d.id == id).unwrap();
    d.title = Some(title);
    d.page.as_mut().unwrap().fetching = None;
    let whole = whole_under_the_mouse(&mut ed, On::Top, "farewell");
    assert!(whole.starts_with("A Guide"), "the title whole: {whole:?}");
}

/// A cut on the frame that laid it out and on the next, which draws what the first laid out (U2's review, 2026-10-28:
/// the chat's rows were laid before the frame began, and cached after): recorded on `on`, its whole holding `word`, whole
/// on its card under the mouse — `anew` makes its rows be laid out again.
fn whole_fresh_and_cached(ed: &mut Editor, on: On, word: &str, anew: fn(&mut Editor)) {
    for when in ["laid out anew", "cached"] {
        if when == "laid out anew" {
            anew(ed);
        }
        let before = frame(ed);
        let (y, a, b, w) = cut_text(ed, word).unwrap_or_else(|| panic!("recorded as cut, {when}:\n{before}"));
        assert_eq!(w.on, on, "on what it's drawn on, {when}");
        assert!(!before.contains(word) && a < b, "cut, {when}:\n{before}");
        ed.handle_event(Event::Hover(((a + b) / 2) as u16, y as u16));
        let screen = frame(ed);
        assert!(screen.contains(word), "whole under the mouse, {when}:\n{screen}");
        ed.handle_event(Event::Hover(1, 1));
        frame(ed);
    }
}

fn chat_of(ed: &mut Editor) -> &mut Chat {
    ed.workspace.claude.chat.as_mut().unwrap()
}

/// The chat's chip on a sent message: its rows cached with the message.
#[test]
fn a_cut_chip_is_whole_on_a_fresh_frame_and_a_cached_one() {
    let chip = format!("a screen went with it — {} and farewell", "the title of a long screen ".repeat(4));
    let mut ed = chat(vec![Msg::new(Role::User, "look", chip)]);
    whole_fresh_and_cached(&mut ed, On::Chat, "farewell", |ed| chat_of(ed).msgs[0].changed());
}

/// A run's wait under its row in the chat: the frame's own row, laid out before the frame begins.
#[test]
fn a_cut_wait_is_whole_on_a_fresh_frame_and_a_cached_one() {
    let mut ed = chat(vec![Msg::new(Role::User, "build it", ""), run_row("cargo build")]);
    let t = crate::task::Task { name: "build".into(), cmd: "cargo build".into(), cwd: "/".into() };
    let mut r = crate::task::Run::for_test(t);
    r.state = crate::task::State::Running;
    let rev = crate::mcp::Rev::default();
    r.chat = Some(crate::tools::Call::new(1, "r1", serde_json::json!(1), rev, "run"));
    let said = format!("{} and farewell", "the lock another cargo holds on its build folder ".repeat(2));
    r.wait.now = Some((crate::waits::Hit { said, report: String::new() }, crate::clock::now()));
    ed.workspace.task_run = Some(r);
    chat_of(&mut ed).state = crate::chat::State::Thinking;
    // (one instant: its time is in its words — a second gone by between two frames is another text, its card a frame
    // later, `whole::drawn`)
    crate::clock::hold();
    whole_fresh_and_cached(&mut ed, On::Chat, "farewell", |_| {});
    crate::clock::release();
}

/// The queued line: its rows cached while nothing changed (`Chat.queued_laid`).
#[test]
fn a_cut_queued_line_is_whole_on_a_fresh_frame_and_a_cached_one() {
    let mut ed = chat(vec![Msg::new(Role::User, "walk it", "")]);
    let c = chat_of(&mut ed);
    c.state = crate::chat::State::Thinking;
    c.queue(
        format!("{} and farewell", "look at what it waited on before you run it again ".repeat(3)),
        false,
    );
    whole_fresh_and_cached(&mut ed, On::Transcript, "farewell", |ed| {
        *chat_of(ed).queued_laid.borrow_mut() = None
    });
}

/// The status line's chip naming the session in front (no chat up, two workspaces): a piece cut before its column is
/// known, carried to its print.
#[test]
fn a_cut_status_piece_is_whole_on_a_fresh_frame_and_a_cached_one() {
    let dir = crate::scratch::Scratch::new("whole-status");
    let mut ed = Editor::new(Config::default());
    ed.workspace_add(dir.to_path_buf());
    ed.workspace.git_branch = Some(format!("{}and-farewell", "a-branch-named-at-length-".repeat(2)));
    whole_fresh_and_cached(&mut ed, On::Status, "farewell", |_| {});
}

/// A floating group's row: the tests' note over its output, the group floated (`C-w e`) — on the floating list's
/// surface, not the slot's (`slot_on`), so its card is drawn over what floats.
#[test]
fn a_floating_groups_cut_row_is_whole_on_a_fresh_frame_and_a_cached_one() {
    let mut ed = Editor::new(Config::default());
    let case = |name: String| crate::test_results::Case {
        name,
        status: crate::test_results::Status::Failed,
        secs: None,
        message: vec!["assertion failed".into()],
        at: None,
    };
    let long = format!("tests::case_{}_and_farewell", "with_a_name_that_runs_on_".repeat(4));
    let run = crate::testing::TestRun::finished("t", vec![case(long)], vec!["ok".into()]);
    ed.workspace.test_run = Some(run);
    frame(&mut ed);
    ed.list_focus(crate::pane::GroupKind::Tests);
    feed(&mut ed, "<C-w>e");
    let g = ed.workspace.group(crate::pane::GroupKind::Tests).unwrap();
    assert!(matches!(g.place, crate::pane::Place::Floating(_)), "floated");
    // (the keys back in the code: no row in hand, the mouse's alone)
    ed.workspace.viewer.at = crate::workspace::At::Code;
    whole_fresh_and_cached(&mut ed, On::List, "farewell", |_| {});
    // (no test ran: its note over the output, in the same frame)
    let r = ed.workspace.test_run.as_mut().unwrap();
    (r.state, r.cases) = (crate::testing::RunState::NoTests, Vec::new());
    // (narrow: the floating frame too narrow for it)
    let narrow = |ed: &mut Editor| {
        let mut bytes = Vec::new();
        crate::term::render(ed, &mut bytes, 70, H).unwrap();
        let mut p = vt100::Parser::new(H, 70, 0);
        p.process(&bytes);
        p.screen().contents()
    };
    let screen = narrow(&mut ed);
    let (y, a, b, w) = cut_text(&ed, "runner did").unwrap_or_else(|| panic!("the note cut:\n{screen}"));
    assert_eq!(w.on, On::List, "the note on what floats:\n{screen}");
    ed.handle_event(Event::Hover(((a + b) / 2) as u16, y as u16));
    let screen = narrow(&mut ed);
    assert!(card(&ed).is_some() && screen.contains("did:"), "whole over what floats:\n{screen}");
}

/// A run panel's list: the tests' selected case, its name cut, whole on its card while the list has the keys — beside
/// the list, never over its other rows; another under the mouse.
#[test]
fn a_run_panels_selected_row_is_whole_in_hand_and_under_the_mouse() {
    let mut ed = Editor::new(Config::default());
    let case = |name: String| crate::test_results::Case {
        name,
        status: crate::test_results::Status::Failed,
        secs: None,
        message: vec!["assertion failed".into()],
        at: None,
    };
    let long =
        |k: usize| format!("tests::case_{k}_{}_and_farewell_{k}", "with_a_name_that_runs_on_".repeat(4));
    let run = crate::testing::TestRun::finished("t", vec![case(long(1)), case(long(2))], vec!["ok".into()]);
    ed.workspace.test_run = Some(run);
    frame(&mut ed);
    let before = frame(&mut ed);
    assert!(card(&ed).is_none(), "no keys, no row in hand:\n{before}");
    ed.list_focus(crate::pane::GroupKind::Tests);
    let screen = frame(&mut ed);
    let (y, ..) = cut_text(&ed, "farewell_1").expect("recorded as cut");
    assert_eq!(crate::whole::shown(&ed).map(|s| matches!(s.1, Some(Hand::Beside(_)))), Some(true));
    assert_eq!(card(&ed).map(|c| c.1), Some(y), "along its own row");
    assert!(screen.contains("farewell_1"), "whole:\n{screen}");
    let (y2, a2, ..) = cut_text(&ed, "farewell_2").expect("the other recorded");
    let row2: String = screen.lines().nth(y2).unwrap().chars().skip(a2).take(13).collect();
    assert_eq!(row2, "case_2_with_a", "never over its other row:\n{screen}");
}

/// A cut cut again narrower than what leads it (U2's third review): `lead("abc ")` refit to `abc…`, the old cut's start
/// inside the new `…` — placed at this cut's `…`, its whole what those cells stand for (the lead's cut part, the old
/// whole); cut again in its tail, from where it was cut, the tail taken in.
#[test]
fn a_cut_cut_again_into_its_lead_is_placed_from_its_new_cut() {
    let placed = |cut: &Fit| {
        begin();
        at(On::Status, cut, (10, 0), Style::default(), (0, 80));
        PLACED.take().into_iter().map(|(spot, w, _)| (spot, w.text)).collect::<Vec<_>>()
    };
    let led = super::fit("a whole name", 6).lead("abc ");
    assert_eq!(led.shown(), "abc a who…");
    let again = led.fit(4);
    assert_eq!(again.shown(), "abc…");
    assert_eq!(
        placed(&again),
        vec![((0, 13, 14), " a whole name".to_string())],
        "at its `…`, all it stands for"
    );
    let tailed = super::fit("a whole name", 6).lead("abc ").then(" tail");
    let again = tailed.fit(12);
    assert_eq!(again.shown(), "abc a who… …");
    assert_eq!(
        placed(&again),
        vec![((0, 14, 22), "a whole name tail".to_string())],
        "from its cut, the tail in"
    );
}

/// A cut on a floating frame's line is seen only where that frame is the topmost drawn (U2's fourth review: `On::Frame`
/// matched any floating surface): a card's title cut on its frame (its first row's cut carried there — it was dropped),
/// the prompt's frame drawn over it: no card; another card's frame over it (the same surface — which frame is topmost
/// tells them apart) and the note card over it (no frame — the surface does), each laid in the frame's record, as
/// no layout today puts them there: no card; with nothing over it, its card. (A picker and which-key hide the cards.)
#[test]
fn a_cut_on_a_frame_under_another_frame_is_not_in_hand() {
    let mut ed = Editor::new(Config::default());
    let title = format!("Run {}and farewell", "a rather long command ".repeat(8));
    let body: Vec<String> = (0..2).map(|i| format!("line {i} of what it asks")).collect();
    let what = crate::offer::What::ReviewAtEnd;
    ed.push_offer(crate::offer::Offer::new(title, &body.join("\n"), "", String::new(), what));
    let alone = frame(&mut ed);
    let (y, a, b, w) =
        cut_text(&ed, "farewell").unwrap_or_else(|| panic!("the title cut on its frame:\n{alone}"));
    let card = match w.on {
        On::Frame(crate::mouse::Surface::Card, rect) => rect,
        on => panic!("on the card's frame: {on:?}"),
    };
    // (the prompt, its completions over the card's title)
    feed(&mut ed, ":s");
    let screen = frame(&mut ed);
    // (a cell of the cut under the prompt's frame, drawn after the card's)
    let frames = ed.screen.frames.borrow().clone();
    let over = |x: usize| {
        let inside =
            |r: (usize, usize, usize, usize)| (r.0..r.0 + r.2).contains(&x) && (r.1..r.1 + r.3).contains(&y);
        frames.iter().rev().find(|f| inside(f.rect)).is_some_and(|f| f.rect != card)
    };
    let x = (a..b).find(|&x| over(x)).unwrap_or_else(|| panic!("the prompt over the cut:\n{screen}"));
    assert!(cut_text(&ed, "farewell").is_some(), "recorded, drawn first:\n{screen}");
    ed.handle_event(Event::Hover(x as u16, y as u16));
    let hovered = frame(&mut ed);
    assert!(
        ed.cut_hover.is_none() && !hovered.contains("farewell"),
        "the prompt over it, no card:\n{hovered}"
    );
    ed.handle_event(Event::Hover(0, H - 1));
    feed(&mut ed, "<esc>");
    frame(&mut ed);
    // (another card's frame over it — the same surface, `mouse_under` saying `Card` there: only which frame is topmost
    // tells them apart. Two cards never touch in a layout today, so it's laid in the frame's record, drawn after it)
    let x = (a + b) / 2;
    let other = crate::mouse::Framed {
        rect: (x.saturating_sub(4), y.saturating_sub(1), 12, 3),
        of: Some(crate::mouse::Lights::Card),
        lit: crate::mouse::Lit::Wash,
    };
    ed.screen.frames.borrow_mut().push(other);
    assert_eq!(ed.mouse_at(x, y).surface, crate::mouse::Surface::Card, "a card's surface there");
    ed.handle_event(Event::Hover(x as u16, y as u16));
    assert!(ed.cut_hover.is_none(), "another card over it, no card");
    ed.handle_event(Event::Hover(0, H - 1));
    frame(&mut ed);
    // (what has no frame over it — the note card, `mouse_under`'s before the card: only the surface tells them apart)
    ed.screen.note_card.set(Some((x.saturating_sub(4), y.saturating_sub(1), 12, 3)));
    assert_eq!(ed.mouse_at(x, y).surface, crate::mouse::Surface::NoteCard, "the note card's surface there");
    ed.handle_event(Event::Hover(x as u16, y as u16));
    assert!(ed.cut_hover.is_none(), "the note card over it, no card");
    ed.screen.note_card.set(None);
    ed.handle_event(Event::Hover(0, H - 1));
    frame(&mut ed);
    // (nothing over it: its card)
    ed.handle_event(Event::Hover(((a + b) / 2) as u16, y as u16));
    let hovered = frame(&mut ed);
    assert!(hovered.contains("farewell"), "in sight, its card:\n{hovered}");
}

/// A cut hidden under a floating pane isn't in hand (U2's third review): a page's title cut on its pane's top line, the
/// floating `:messages` laid over it — the mouse there is over the list, whose surface a docked line's could be: no
/// card; the list moved off it, its card.
#[test]
fn a_cut_hidden_under_a_floating_pane_is_not_in_hand() {
    use crate::pane::GroupKind;
    use crate::split::Rect;
    let mut ed = Editor::new(Config::default());
    let id = crate::web::page::stand_in(&mut ed, "https://docs.example.com/guide", "Its text.\n");
    let title = format!("web · docs.example.com › {} and farewell", "A Guide to Everything ".repeat(6));
    let d = ed.docs.iter_mut().find(|d| d.id == id).unwrap();
    d.title = Some(title);
    d.page.as_mut().unwrap().fetching = None;
    frame(&mut ed);
    let (y, a, b, w) = cut_text(&ed, "farewell").expect("the title cut");
    assert_eq!((w.on, w.float), (On::Top, None), "docked");
    let x = (a + b) / 2;
    ed.set_status("a message to list");
    ed.messages_open(None);
    frame(&mut ed);
    ed.workspace.viewer.at = crate::workspace::At::Code;
    for (over, at) in [(true, y), (false, y + 12)] {
        ed.float_set(GroupKind::Messages, Rect { x: x.saturating_sub(10), y: at, w: 40, h: 6 });
        let before = frame(&mut ed);
        let floats = ed.float_at(x, y).map(|f| f.0);
        assert_eq!(floats, over.then_some(GroupKind::Messages), "the list over it: {over}\n{before}");
        assert!(cut_text(&ed, "farewell").is_some(), "recorded, drawn first");
        ed.handle_event(Event::Hover(x as u16, y as u16));
        let screen = frame(&mut ed);
        match over {
            true => {
                assert!(ed.cut_hover.is_none() && !screen.contains("farewell"), "hidden, no card:\n{screen}")
            }
            false => assert!(screen.contains("farewell"), "in sight, its card:\n{screen}"),
        }
        ed.handle_event(Event::Hover(0, H - 1));
        frame(&mut ed);
    }
}
