//! The prompt's line (#153): Helix's prompt keys edit within it on every surface that has a prompt, its history, a
//! press and a drag in it, a long line kept around its cursor.

use super::*;
use crate::config::Config;
use crate::event::MouseKind as K;

/// An editor holding `text`, `keys` fed to it (as editor.rs `run`).
fn run(text: &str, keys: &str) -> Editor {
    let mut ed = Editor::new(Config::default());
    ed.docs[0].text = ropey::Rope::from_str(text);
    feed(&mut ed, keys);
    ed
}

fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(key);
    }
}

/// The prompt's line, a `|` where its cursor is.
fn line(ed: &Editor) -> String {
    let p = ed.prompt.as_ref().expect("a prompt up");
    let (before, after) = p.split();
    format!("{before}|{after}")
}

fn said(ed: &Editor) -> String {
    ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default()
}

/// A frame drawn, as the screen shows it.
fn frame(ed: &mut Editor) -> vt100::Screen {
    let (w, h) = (100, 30);
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, w, h).unwrap();
    let mut p = vt100::Parser::new(h, w, 0);
    p.process(&bytes);
    p.screen().clone()
}

/// Every key `[prompt]` and `[insert]` bind for the line, as Helix's prompt has it — each from the middle of
/// `abc def ghi` (the cursor after `def`), at `:` and at `/` alike.
#[test]
fn helix_prompt_keys_edit_within_the_line() {
    let mid = "<left><left><left><left>";
    let rows = [
        ("<left>X", "abc deX|f ghi"),
        ("<C-b>X", "abc deX|f ghi"),
        ("<right>X", "abc def X|ghi"),
        ("<C-f>X", "abc def X|ghi"),
        ("<C-a>X", "X|abc def ghi"),
        ("<home>X", "X|abc def ghi"),
        ("<C-e>X", "abc def ghiX|"),
        ("<end>X", "abc def ghiX|"),
        ("<A-b>X", "abc X|def ghi"),
        ("<C-left>X", "abc X|def ghi"),
        ("<A-f>X", "abc def X|ghi"),
        ("<C-right>X", "abc def X|ghi"),
        ("<C-a><A-f>X", "abc X|def ghi"),
        ("<A-f><A-f>X", "abc def ghiX|"),
        ("<A-b><A-b>X", "X|abc def ghi"),
        ("<C-w>", "abc | ghi"),
        ("<A-backspace>", "abc | ghi"),
        ("<A-d>", "abc def|"),
        ("<A-del>", "abc def|"),
        ("<C-u>", "| ghi"),
        ("<C-k>", "abc def|"),
        ("<backspace>", "abc de| ghi"),
        ("<C-h>", "abc de| ghi"),
        ("<del>", "abc def|ghi"),
        ("<C-d>", "abc def|ghi"),
        ("<C-r>a", "abc defREG| ghi"),
        ("7(", "abc def7(| ghi"),
    ];
    for open in [":", "/"] {
        for (keys, want) in rows {
            let mut ed = Editor::new(Config::default());
            ed.write_register('a', vec!["REG".into()]);
            feed(&mut ed, &format!("{open}abc def ghi{mid}{keys}"));
            assert_eq!(line(&ed), want, "{open} then {keys}");
            assert_eq!(ed.doc().id, ed.docs[0].id, "{open} {keys}: the code's document is current");
            assert!(ed.doc().text == "", "{open} {keys}: the code untouched");
        }
    }
    // A register's line break is a space: the line stays one line
    let mut ed = Editor::new(Config::default());
    ed.write_register('a', vec!["one\ntwo".into()]);
    feed(&mut ed, ":<C-r>a");
    assert_eq!(line(&ed), "one two|");
    // `C-r` waits for the register's name: said where the keys are, and in the state
    feed(&mut ed, "<C-r>");
    assert!(ed.keys_to().char_next, "the next key is a register's name");
    assert_eq!(ed.pending_display(), "C-r…");
    feed(&mut ed, "<esc>");
    assert!(ed.prompt.is_some(), "esc let the wait go, the prompt still up");
    assert!(!ed.keys_to().char_next);
}

/// What's typed goes in at the cursor and `enter` runs the whole line, on every surface with a prompt: the code's
/// `:`, `/` and `s`, a view mode's `/` (the chat's conversation — its line swapped in inside the conversation's own),
/// a floating list's `/` (`:messages`).
#[test]
fn every_surface_edits_its_line_at_the_cursor() {
    // The code's `/`: the line as edited is the search
    let ed = run("abXc YabXc\n", "/abc<left>X<C-a>Y<ret>");
    assert_eq!(ed.search.as_deref(), Some("YabXc"));
    // The code's `:` — a command run as edited (a number: that line)
    let ed = run(&"line\n".repeat(20), ":5<C-a>1<ret>");
    assert_eq!(ed.cursor_line(), 14, "`:15` ran");
    // The code's `s`: the regex as edited
    let mut ed = run("one two one\n", "%sne<C-a>o<ret>");
    assert_eq!(ed.doc().selection().ranges().len(), 2, "`one` selected twice");
    ed.prompt = None;
    // A view mode's `/`: the chat's conversation
    let mut ed = Editor::new(Config::default());
    let mut c = crate::chat::Chat::new();
    c.msgs = vec![
        crate::chat::Msg::new(crate::chat::Role::User, "where is the needle?", ""),
        crate::chat::Msg::new(crate::chat::Role::Assistant, "the needle is here", ""),
    ];
    ed.workspace.claude.chat = Some(c);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    frame(&mut ed);
    feed(&mut ed, "<esc>/nedle<C-a><C-f>e");
    assert_eq!(line(&ed), "ne|edle", "its line, edited at the cursor");
    let t = crate::chatdoc::transcript(&ed).unwrap();
    assert_eq!(t.pane.hits().map(|h| h.pattern.clone()), None, "not drawn yet");
    frame(&mut ed);
    let t = crate::chatdoc::transcript(&ed).unwrap();
    assert_eq!(t.pane.hits().map(|h| h.pattern.as_str()), Some("needle"), "its matches the line's");
    assert_eq!(ed.doc().id, ed.docs[0].id, "the code's document current again");
    feed(&mut ed, "<ret>");
    let t = crate::chatdoc::transcript(&ed).unwrap();
    assert_eq!(t.pane.keys.search.as_deref(), Some("needle"), "the conversation's search");
    assert_eq!(ed.search, None, "never the code's");
    // A floating list's `/`
    let mut ed = Editor::new(Config::default());
    ed.set_status("saved alpha.rs");
    ed.set_status("saved beta.rs");
    feed(&mut ed, ":messages<ret>");
    frame(&mut ed);
    feed(&mut ed, "/apha<C-a><C-f>l");
    assert_eq!(line(&ed), "al|pha");
    feed(&mut ed, "<ret>");
    let at = ed.float_list(crate::pane::GroupKind::Messages).map(|p| p.search.clone());
    assert_eq!(at, Some(Some("alpha".into())), "the list's search, as edited");
}

/// A sequence bound in insert mode (`[keys.insert] j = { k = … }`, common in Helix configs): its `j` waits as it does
/// in the code — `j` then `x` is text; `j` then `enter` runs the line with the `j` in it, `j` then `esc` closes: the
/// prompt's own keys never swallowed by a sequence under way. `j k` runs `normal_mode`, not for one line: said.
#[test]
fn an_insert_sequence_under_way_never_takes_the_prompts_own_keys() {
    let (config, w) = crate::config::parse("[keys.insert]\nj = { k = \"normal_mode\" }\n");
    assert!(w.is_empty(), "{w:?}");
    let mut ed = Editor::new(config.clone());
    ed.docs[0].text = ropey::Rope::from_str("aj bj\n");
    feed(&mut ed, "/aj");
    assert_eq!(line(&ed), "a|", "the `j` waits");
    assert_eq!(ed.pending_display(), "j");
    feed(&mut ed, "x");
    assert_eq!(line(&ed), "ajx|", "broken off: text");
    feed(&mut ed, "<backspace><ret>");
    assert!(ed.prompt.is_none(), "enter ran it");
    assert_eq!(ed.search.as_deref(), Some("aj"));
    // `enter` right after the `j`: the `j` goes in first, then the line runs
    feed(&mut ed, "/bj<ret>");
    assert!(ed.prompt.is_none(), "enter wasn't swallowed");
    assert_eq!(ed.search.as_deref(), Some("bj"));
    // `esc` right after it: closed
    feed(&mut ed, "/cj<esc>");
    assert!(ed.prompt.is_none(), "esc wasn't swallowed");
    // `C-a` after it: the `j` typed, then the move
    feed(&mut ed, ":wj<C-a>X");
    assert_eq!(line(&ed), "X|wj");
    // The sequence itself: what it runs isn't for the line — said, the line as it was
    feed(&mut ed, "<esc>:a<C-a>jk");
    assert_eq!(line(&ed), "|a");
    assert!(said(&ed).contains("does nothing in the command line"), "{}", said(&ed));
    // A floating list's `/` alike: `enter` after the `j` searches with it, `backspace` after a lone `j` takes the `j`
    // out — the line wasn't empty — and the list's search stays up
    let mut ed = Editor::new(config);
    ed.set_status("saved aj.rs");
    ed.set_status("saved b.rs");
    feed(&mut ed, "<esc>:messages<ret>");
    frame(&mut ed);
    let list = |ed: &Editor| ed.float_list(crate::pane::GroupKind::Messages).unwrap().search.clone();
    feed(&mut ed, "/aj<ret>");
    assert!(ed.prompt.is_none(), "enter ran it");
    assert_eq!(list(&ed).as_deref(), Some("aj"), "the `j` went in first");
    feed(&mut ed, "/j<backspace>");
    assert_eq!(line(&ed), "|", "the `j` typed, then taken out");
    feed(&mut ed, "<backspace>");
    assert!(ed.prompt.is_none(), "empty now: backspace closes it");
}

/// A paste goes into the line at the cursor at once — one change, never a key at a time: a long line, quickly, and
/// a `jk` in it is text, never the sequence it would be typed; what a sequence under way had typed goes in first.
#[test]
fn a_paste_goes_in_at_once() {
    let (config, _) = crate::config::parse("[keys.insert]\nj = { k = \"normal_mode\" }\n");
    let mut ed = Editor::new(config);
    feed(&mut ed, ":ab<left>");
    let long = "word jk ".repeat(2500);
    let begun = std::time::Instant::now();
    ed.handle_paste(format!("{long}\nnext line"));
    let took = begun.elapsed();
    assert_eq!(line(&ed), format!("a{long} next line|b"), "at the cursor, one line, `jk` as text");
    assert!(took < std::time::Duration::from_secs(2), "{took:?} for 20 KB (key by key: minutes)");
    // A sequence's `j` under way: in first, the paste after it
    feed(&mut ed, "<esc>:j");
    ed.handle_paste("X".into());
    assert_eq!(line(&ed), "jX|");
    feed(&mut ed, "k");
    assert_eq!(line(&ed), "jXk|", "the sequence over: `k` typed");
    // A floating list's `/` follows a paste as it follows typing
    let mut ed = Editor::new(Config::default());
    ed.set_status("saved alpha.rs");
    ed.set_status("saved beta.rs");
    feed(&mut ed, ":messages<ret>");
    frame(&mut ed);
    let selected = |ed: &Editor| ed.float_list(crate::pane::GroupKind::Messages).unwrap().selected;
    let newest = selected(&ed);
    feed(&mut ed, "/");
    ed.handle_paste("alpha".into());
    assert_eq!(selected(&ed), newest - 1, "on `saved alpha.rs`, the row before the newest");
}

/// `:`'s completion reads what's before the cursor and keeps what's after; its dim rest (the ghost) and `→` taking
/// it only at the line's end — elsewhere `→` moves.
#[test]
fn completion_works_before_the_cursor() {
    let mut ed = run("", ":wri");
    assert!(ed.cmdline_ghost().is_some(), "at the end: a ghost");
    feed(&mut ed, "<left>");
    assert_eq!(ed.cmdline_ghost(), None, "mid-line: none");
    feed(&mut ed, "<right>");
    assert_eq!(line(&ed), "wri|", "→ moved, took nothing");
    feed(&mut ed, "<right>");
    assert!(
        line(&ed).starts_with("write") && line(&ed).ends_with('|'),
        "at the end → takes it: {}",
        line(&ed)
    );
    // Mid-line: the word before the cursor completed, the rest kept
    let mut ed = run("", ":wri x.rs<C-a><A-f><left>");
    assert_eq!(line(&ed), "wri| x.rs");
    let comp = ed.cmdline_completion().unwrap();
    let alone = run("", ":wri").cmdline_completion().unwrap();
    let inserts =
        |c: &crate::cmdline::Completion| c.cands.iter().map(|c| c.insert.clone()).collect::<Vec<_>>();
    assert_eq!(inserts(&comp), inserts(&alone), "for `wri`, not the whole line");
    assert!(comp.cands.len() > 1);
    feed(&mut ed, "<tab>");
    let (before, after) = ed.prompt.as_ref().unwrap().split();
    assert_eq!(before, comp.cands[0].insert, "the first candidate in place of `wri`");
    assert_eq!(after, " x.rs", "what's after the cursor kept");
    feed(&mut ed, "<tab>");
    let (before, after) = ed.prompt.as_ref().unwrap().split();
    assert_eq!((before.as_str(), after.as_str()), (comp.cands[1].insert.as_str(), " x.rs"), "cycling on");
}

/// `tab` with one candidate and text after the cursor: the name filled in, then one space before the rest — never
/// two — and the cursor after it, where the argument goes; a command that takes none gets no space.
#[test]
fn one_candidate_with_text_after_the_cursor() {
    // (a command taking an argument whose own name completes to it alone; one taking none, likewise)
    let alone = |takes: bool| {
        crate::cmdline::COMMANDS
            .iter()
            .filter(|c| (c.arg != crate::cmdline::Arg::None) == takes)
            .map(|c| c.names[0])
            .find(|n| {
                let short = &n[..n.len() - 1];
                let c = run("", &format!(":{short}")).cmdline_completion().unwrap();
                n.len() > 3
                    && n.chars().all(|c| c.is_ascii_lowercase())
                    && c.cands.len() == 1
                    && c.cands[0].insert == *n
            })
            .expect("such a command")
    };
    let (with, without) = (alone(true), alone(false));
    let short = &with[..with.len() - 1];
    // The rest right after the cursor: a space put between
    let ed = run("", &format!(":{short}x.rs<C-a>{}<tab>", "<right>".repeat(short.chars().count())));
    assert_eq!(line(&ed), format!("{with} |x.rs"));
    // A space there already: kept, the cursor past it
    let ed = run("", &format!(":{short} x.rs<C-a>{}<tab>", "<right>".repeat(short.chars().count())));
    assert_eq!(line(&ed), format!("{with} |x.rs"));
    // No argument: no space, the cursor after the name
    let short = &without[..without.len() - 1];
    let ed = run("", &format!(":{short} x<C-a>{}<tab>", "<right>".repeat(short.chars().count())));
    assert_eq!(line(&ed), format!("{without}| x"));
}

/// `↑`/`↓` (`C-p`/`C-n`) walk what was run at this kind of prompt — `:` its own, `/` and `?` theirs — back to what
/// was typed; an edit of yours ends the walk; past either end, said; a line that keeps none says so.
#[test]
fn the_history_walks_each_kinds_lines() {
    let mut ed = run("abc def\n", "/abc<ret>/def<ret>:1<ret>");
    feed(&mut ed, "/xy<up>");
    assert_eq!(line(&ed), "def|");
    feed(&mut ed, "<C-p>");
    assert_eq!(line(&ed), "abc|");
    feed(&mut ed, "<up>");
    assert_eq!(line(&ed), "abc|", "the oldest");
    assert!(said(&ed).starts_with("the oldest kept"), "{}", said(&ed));
    feed(&mut ed, "<down>");
    assert_eq!(line(&ed), "def|");
    feed(&mut ed, "<C-n>");
    assert_eq!(line(&ed), "xy|", "back to what was typed");
    feed(&mut ed, "<down>");
    assert!(said(&ed).starts_with("the newest"), "{}", said(&ed));
    // An edit ends the walk: `↓` from there is past the newest
    feed(&mut ed, "<up>z<down>");
    assert_eq!(line(&ed), "defz|");
    // `?` shares `/`'s; `:` its own
    feed(&mut ed, "<esc>?<up>");
    assert_eq!(line(&ed), "def|");
    feed(&mut ed, "<esc>:<up>");
    assert_eq!(line(&ed), "1|");
    feed(&mut ed, "<up>");
    assert_eq!(line(&ed), "1|", "one line run there");
    feed(&mut ed, "<esc>%s<up>");
    assert_eq!(line(&ed), "|");
    assert!(said(&ed).starts_with("this line keeps no history"), "{}", said(&ed));
}

/// A history keeps `HISTORY_MAX` lines, the oldest let go; never an empty line, never the newest twice; and it's
/// memory only (a `:sh` line can hold a secret): the state drivers read holds the line typed, never the history.
/// (That a real tarae leaves none of it in `$state`, whatever it writes there: e2e's
/// `the_prompts_history_never_reaches_the_state_folder`.)
#[test]
fn the_history_keeps_a_hundred_lines_in_memory_only() {
    let mut h = History::default();
    for i in 0..HISTORY_MAX + 5 {
        h.remember(PromptKind::Command, &format!("line {i}"));
    }
    h.remember(PromptKind::Command, "line 104");
    h.remember(PromptKind::Command, "  ");
    h.remember(PromptKind::Rename, "no list");
    let list = h.of(PromptKind::Command).unwrap();
    assert_eq!(list.len(), HISTORY_MAX);
    assert_eq!((list[0].as_str(), list[HISTORY_MAX - 1].as_str()), ("line 5", "line 104"));
    assert!(h.of(PromptKind::Search { reverse: false }).unwrap().is_empty());
    assert!(h.of(PromptKind::Rename).is_none());
    let mut ed = run("", ":");
    ed.prompt_history.remember(PromptKind::Command, "sh export TOKEN=held-in-history");
    feed(&mut ed, "wq");
    let state = ed.ui_state().json().to_string();
    assert!(state.contains("\"wq\""), "what's typed: {state}");
    assert!(!state.contains("held-in-history"), "the state holds what's typed, not the history");
}

/// On an empty line `C-w` has nothing to delete — said, the prompt still up with the keys: never the window prefix
/// while a prompt is open (it's modal).
#[test]
fn an_empty_lines_c_w_says_nothing_to_delete() {
    let mut ed = run("", ":");
    feed(&mut ed, "<C-w>");
    assert_eq!(said(&ed), "nothing to delete — esc close");
    assert!(ed.prompt.is_some());
    assert!(ed.pending.is_empty(), "no window prefix under way");
    assert_eq!(ed.keys_to().region, crate::uistate::Region::Prompt);
    feed(&mut ed, "<C-u><A-d>");
    assert_eq!(said(&ed), "nothing to delete — esc close");
    // `backspace` on an empty line closes it, as before
    feed(&mut ed, "<backspace>");
    assert!(ed.prompt.is_none());
}

/// What `[insert]` binds that isn't for one line — a new line, a tab, a completion, an undo checkpoint, the page
/// keys — is said, the line as it was; an unbound `C-` key too.
#[test]
fn keys_not_for_one_line_are_said() {
    for key in ["<C-j>", "<C-x>", "<tab>", "<C-s>", "<pageup>", "<A-z>"] {
        let mut ed = run("", "/ab<left>");
        feed(&mut ed, key);
        assert_eq!(line(&ed), "a|b", "{key}");
        assert!(said(&ed).contains("does nothing in the command line"), "{key}: {}", said(&ed));
    }
}

/// A press on the line puts its cursor on the character pressed (on its `…`, the first in sight); a drag from there
/// selects to the character under the pointer, washed as the code's selection; past its end the line scrolls.
#[test]
fn a_press_and_a_drag_in_the_line() {
    // (colours drawn though tarae's test runner sets NO_COLOR)
    crossterm::style::force_color_output(true);
    let mut ed = run("", ":write a.rs b.rs");
    frame(&mut ed);
    let d = ed.screen.prompt_input.get().expect("its line drawn");
    let mouse = |ed: &mut Editor, kind, x: usize| {
        ed.handle_mouse(crate::event::Mouse { kind, x: x as u16, y: d.y as u16, alt: false, ctrl: false });
    };
    mouse(&mut ed, K::Down, d.x + 6);
    assert_eq!(line(&ed), "write |a.rs b.rs", "on the character pressed");
    assert!(ed.prompt.is_some() && ed.keys_to().region == crate::uistate::Region::Prompt);
    mouse(&mut ed, K::Drag, d.x + 9);
    mouse(&mut ed, K::Up, d.x + 9);
    let r = ed.prompt.as_ref().unwrap().input.pane.doc().selection().primary();
    assert_eq!((r.from(), r.to()), (6, 10), "`a.rs` selected, both ends included");
    let s = frame(&mut ed);
    let (plain, washed) =
        (s.cell(d.y as u16, d.x as u16).unwrap(), s.cell(d.y as u16, (d.x + 7) as u16).unwrap());
    assert_ne!(plain.bgcolor(), washed.bgcolor(), "the selection washed");
    // Typing goes on at the cursor
    feed(&mut ed, "<C-a>X");
    assert_eq!(line(&ed), "X|write a.rs b.rs");
    // A long line: its start cut, a press on the `…` the first character in sight
    let long = "x".repeat(200);
    let mut ed = run("", &format!(":{long}"));
    frame(&mut ed);
    let d = ed.screen.prompt_input.get().unwrap();
    assert!(d.first > 0, "the end in sight, the start cut");
    let mouse = |ed: &mut Editor, kind, x: usize| {
        ed.handle_mouse(crate::event::Mouse { kind, x: x as u16, y: d.y as u16, alt: false, ctrl: false });
    };
    mouse(&mut ed, K::Down, d.x - 1);
    assert_eq!(
        ed.prompt.as_ref().unwrap().cursor(),
        d.first - 1,
        "before the first in sight: it scrolls back"
    );
}

/// An editor with a picker of `labels` up, `keys` fed to it (the picker's tests, #153's next step).
fn picker(config: Config, labels: &[&str], keys: &str) -> Editor {
    let mut ed = Editor::new(config);
    let item = |l: &&str| crate::picker::Item {
        label: l.to_string(),
        action: crate::picker::Action::Typed(l.to_string()),
        hint: String::new(),
        glyph: None,
    };
    ed.open_picker(crate::picker::Picker::new("things", labels.iter().map(item).collect(), false), None);
    feed(&mut ed, keys);
    ed
}

/// The picker's query, a `|` where its cursor is — and the list filtered by what it holds.
fn query(ed: &Editor) -> String {
    let p = ed.picker.as_ref().expect("a picker up");
    let (text, at) = (p.input.text(), p.input.cursor());
    assert_eq!(p.query(), text, "the list filtered by the line as it is");
    format!("{}|{}", &text[..at], &text[at..])
}

/// A picker's query edits as the prompt's line does: Helix's prompt keys where the picker has none of its own, each
/// from the middle of `abc def ghi`; the code's document stays current and untouched.
#[test]
fn a_pickers_query_edits_as_the_prompts_line() {
    let mid = "<left><left><left><left>";
    let rows = [
        ("<left>X", "abc deX|f ghi"),
        ("<right>X", "abc def X|ghi"),
        ("<C-a>X", "X|abc def ghi"),
        ("<C-e>X", "abc def ghiX|"),
        ("<A-b>X", "abc X|def ghi"),
        ("<C-left>X", "abc X|def ghi"),
        ("<A-f>X", "abc def X|ghi"),
        ("<C-right>X", "abc def X|ghi"),
        ("<C-w>", "abc | ghi"),
        ("<A-backspace>", "abc | ghi"),
        ("<A-d>", "abc def|"),
        ("<A-del>", "abc def|"),
        ("<C-k>", "abc def|"),
        ("<backspace>", "abc de| ghi"),
        ("<C-h>", "abc de| ghi"),
        ("<del>", "abc def|ghi"),
        ("<C-r>a", "abc defREG| ghi"),
        ("7(", "abc def7(| ghi"),
    ];
    for (keys, want) in rows {
        let mut ed = picker(Config::default(), &["one"], "");
        ed.write_register('a', vec!["REG".into()]);
        feed(&mut ed, &format!("abc def ghi{mid}{keys}"));
        assert_eq!(query(&ed), want, "{keys}");
        assert_eq!(ed.doc().id, ed.docs[0].id, "{keys}: the code's document is current");
        assert!(ed.doc().text == "", "{keys}: the code untouched");
    }
    // An edit mid-query filters the list by the whole line
    let mut ed = picker(Config::default(), &["src/term.rs", "src/editor.rs"], "trm<left><left>e");
    assert_eq!(query(&ed), "te|rm");
    assert_eq!(ed.picker.as_ref().unwrap().counts().0, 1, "only term.rs has `term`");
    // `C-r` waits for the register's name: in the state, and `esc` lets the wait go with the picker still up
    feed(&mut ed, "<C-r>");
    assert!(ed.keys_to().char_next, "the next key is a register's name");
    assert_eq!(ed.keys_to().region, crate::uistate::Region::Picker);
    feed(&mut ed, "<esc>");
    assert!(ed.picker.is_some() && !ed.keys_to().char_next, "esc let the wait go");
}

/// A paste goes into the picker's query at its cursor, one line, and the list is filtered by the whole of it.
#[test]
fn a_paste_goes_into_a_pickers_query_at_its_cursor() {
    let mut ed = picker(Config::default(), &["src/term.rs", "src/editor.rs", "docs/term.md"], "src.rs");
    assert_eq!(ed.picker.as_ref().unwrap().counts().0, 2);
    feed(&mut ed, "<left><left><left>");
    ed.handle_event(crate::event::Event::Paste("te\nrm".into()));
    assert_eq!(query(&ed), "srcte rm|.rs", "at the cursor, its line break a space");
    assert_eq!(ed.picker.as_ref().unwrap().counts().0, 1, "only src/term.rs");
}

/// The picker's own keys come first (Helix: prompt keys work in pickers except where they conflict): `C-d` `C-u`
/// half a page, `C-f` `C-b` the preview, `home` `end` the rows, `C-n` `C-p` `tab` a row — the query as it was.
#[test]
fn a_pickers_own_keys_come_before_the_querys() {
    let labels = ["a1", "a2", "a3", "a4"];
    for key in ["<C-d>", "<C-u>", "<C-f>", "<C-b>", "<home>", "<end>", "<C-n>", "<C-p>", "<tab>", "<S-tab>"] {
        let ed = picker(Config::default(), &labels, &format!("a<left>{key}"));
        assert_eq!(query(&ed), "|a", "{key}: the query as it was");
    }
    let ed = picker(Config::default(), &labels, "a<end>");
    assert_eq!(ed.picker.as_ref().unwrap().selected, 3, "end: the last row");
    let ed = picker(Config::default(), &labels, "a<C-n><C-n>");
    assert_eq!(ed.picker.as_ref().unwrap().selected, 2, "C-n: a row down");
}

/// What's typed narrows the list at once: a sequence bound in insert mode (`j k`) never holds a character back in
/// a picker — `j` filters as it's typed, `jk` is text.
#[test]
fn a_pickers_query_narrows_as_each_character_is_typed() {
    let (config, w) = crate::config::parse("[keys.insert]\nj = { k = \"normal_mode\" }\n");
    assert!(w.is_empty(), "{w:?}");
    let mut ed = picker(config, &["jk", "jx", "ab"], "j");
    assert_eq!(query(&ed), "j|", "the `j` typed at once");
    assert_eq!(ed.picker.as_ref().unwrap().counts().0, 2);
    assert!(ed.pending_display().is_empty(), "nothing under way");
    feed(&mut ed, "k");
    assert_eq!(query(&ed), "jk|");
    assert_eq!(ed.picker.as_ref().unwrap().counts().0, 1);
}

/// What `[insert]` binds that isn't for one line, and an unbound `A-` key, are said in the picker's words; an empty
/// query's deletes say there's nothing to delete. The query as it was.
#[test]
fn keys_not_for_a_pickers_query_are_said() {
    for key in ["<C-j>", "<C-x>", "<A-z>"] {
        let ed = picker(Config::default(), &["ab"], &format!("ab<left>{key}"));
        assert_eq!(query(&ed), "a|b", "{key}");
        assert!(said(&ed).contains("does nothing in the list's query"), "{key}: {}", said(&ed));
    }
    let ed = picker(Config::default(), &["ab"], "<C-w>");
    assert_eq!(said(&ed), "nothing to delete — esc close");
    assert!(ed.picker.is_some());
}

/// A press on the picker's query puts its cursor on the character pressed, the terminal's cursor drawn there; a drag
/// selects, washed as the prompt's line is; typing goes on at the cursor. A long query keeps its cursor in sight.
#[test]
fn a_press_and_a_drag_in_a_pickers_query() {
    crossterm::style::force_color_output(true);
    let mut ed = picker(Config::default(), &["write a.rs"], "write a.rs b.rs");
    frame(&mut ed);
    let d = ed.screen.picker_input.get().expect("its query drawn");
    let mouse = |ed: &mut Editor, kind, x: usize| {
        ed.handle_mouse(crate::event::Mouse { kind, x: x as u16, y: d.y as u16, alt: false, ctrl: false });
    };
    mouse(&mut ed, K::Down, d.x + 6);
    assert_eq!(query(&ed), "write |a.rs b.rs", "on the character pressed");
    let s = frame(&mut ed);
    assert_eq!(s.cursor_position(), (d.y as u16, (d.x + 6) as u16), "the terminal's cursor there");
    mouse(&mut ed, K::Drag, d.x + 9);
    mouse(&mut ed, K::Up, d.x + 9);
    assert!(ed.picker.is_some(), "the drag kept it up");
    let r = ed.picker.as_ref().unwrap().input.pane.doc().selection().primary();
    assert_eq!((r.from(), r.to()), (6, 10), "`a.rs` selected, both ends included");
    let s = frame(&mut ed);
    let (plain, washed) =
        (s.cell(d.y as u16, d.x as u16).unwrap(), s.cell(d.y as u16, (d.x + 7) as u16).unwrap());
    assert_ne!(plain.bgcolor(), washed.bgcolor(), "the selection washed");
    feed(&mut ed, "<C-a>X");
    assert_eq!(query(&ed), "X|write a.rs b.rs");
    // A long query: its end in sight, then its start once the cursor goes there
    let mut ed = picker(Config::default(), &["x"], &"x".repeat(200));
    frame(&mut ed);
    assert!(ed.screen.picker_input.get().unwrap().first > 0, "the end in sight, the start cut");
    feed(&mut ed, "<C-a>");
    frame(&mut ed);
    assert_eq!(ed.screen.picker_input.get().unwrap().first, 0, "the start in sight with the cursor");
}

/// The line's first byte drawn is the last frame's: an edit before the next frame (a paste, a key in the same batch)
/// can leave it inside a character — a press then lands on a character boundary, never panics.
#[test]
fn a_press_after_an_edit_lands_on_a_character() {
    let long = "가나다라".repeat(40);
    let mut ed = run("", &format!(":{long}"));
    frame(&mut ed);
    let d = ed.screen.prompt_input.get().unwrap();
    assert!(d.first > 0 && d.first.is_multiple_of(3), "cut at a syllable: {d:?}");
    // One byte more before it, no frame: the last frame's first byte is inside a syllable now
    ed.prompt.as_mut().unwrap().set(&format!("x{long}"));
    assert!(!ed.prompt.as_ref().unwrap().text().is_char_boundary(d.first));
    // (the `…` before it, its first cells, its last — a press past the card would close it)
    for x in [d.x - 1, d.x, d.x + 3, d.x + d.w - 2] {
        ed.handle_mouse(crate::event::Mouse {
            kind: K::Down,
            x: x as u16,
            y: d.y as u16,
            alt: false,
            ctrl: false,
        });
        let p = ed.prompt.as_ref().unwrap();
        assert!(p.text().is_char_boundary(p.cursor()), "x {x}: {}", p.cursor());
        ed.handle_mouse(crate::event::Mouse {
            kind: K::Drag,
            x: x as u16 + 1,
            y: d.y as u16,
            alt: false,
            ctrl: false,
        });
        ed.handle_mouse(crate::event::Mouse {
            kind: K::Up,
            x: x as u16 + 1,
            y: d.y as u16,
            alt: false,
            ctrl: false,
        });
    }
}

/// A long line is drawn around its cursor: the end in sight with the cursor there, the start with it at 0, a
/// character at a time as it moves past an edge (the rest holds still), and back to its start once it all fits.
#[test]
fn a_long_line_keeps_its_cursor_in_sight() {
    use crate::term::prompt_window as window;
    let text = "abcdefghijklmnopqrstuvwxyz";
    let w = window(text, 26, 0, 10);
    assert_eq!((w.first, w.end, w.lead, w.tail), (18, 26, true, false), "`…` + 8 + the cursor's cell");
    let w = window(text, 0, 18, 10);
    assert_eq!((w.first, w.end, w.lead, w.tail), (0, 9, false, true), "9 + `…`");
    // Moving within what's shown holds still; past its first, a character back
    assert_eq!(window(text, 12, 10, 10).first, 10);
    assert_eq!(window(text, 9, 10, 10).first, 9);
    // Right past the end: on as far as the cursor needs (the `…` coming takes a cell too)
    let w = window(text, 9, 0, 10);
    assert_eq!((w.first, w.lead, w.end), (2, true, 10), "{w:?}");
    // Shorter now, all of it fits: from its start
    assert_eq!(
        window("abc", 3, 18, 10),
        crate::term::PromptLine { first: 0, end: 3, lead: false, tail: false }
    );
    // Wide characters keep their cells whole
    let w = window("가나다라마바", 18, 0, 7);
    assert!(w.lead && "가나다라마바"[w.first..w.end].chars().count() <= 3, "{w:?}");
}
