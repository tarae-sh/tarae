//! The chat's input as a buffer: insert mode's keys on its document, `[chat]` over them, the chat's own first.

use super::*;
use crate::config::Config;
use crate::editor::Mode;

/// An editor with the chat up and its input with the keys, the code holding `code`.
fn editor(code: &str) -> Editor {
    let mut ed = Editor::new(Config::default());
    ed.docs[0].text = ropey::Rope::from_str(code);
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.set_at(crate::workspace::Here::Chat);
    ed
}

/// Keys as `run` takes them (`<C-w>`, `<A-ret>`).
fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(key);
        assert_eq!(ed.doc().id, ed.docs[0].id, "after {key}: the code's document is current");
    }
}

/// What's typed, and the cursor.
fn input(ed: &Editor) -> (String, usize) {
    let i = &ed.workspace.claude.chat.as_ref().unwrap().input;
    (i.text(), i.cursor())
}

/// #413: the wheel over a draft taller than the input's rows scrolls them a row a notch — never the cursor, the selection
/// or the keys (in the code here); at an end nothing moves and nothing is drawn again; the cursor's row out of sight draws
/// no cursor; a key that moves the cursor or the text brings its row back.
#[test]
fn the_wheel_scrolls_a_draft_taller_than_its_rows() {
    use crate::event::{Mouse, MouseKind};
    let mut ed = editor("fn main() {}\n");
    let draft: String = (0..12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    ed.workspace.claude.chat.as_mut().unwrap().input = draft.as_str().into();
    // (the frame: the input as drawn, and whether the terminal's cursor shows)
    let frame = |ed: &mut Editor| {
        let mut out = Vec::new();
        crate::term::render(ed, &mut out, 120, 30).unwrap();
        let mut p = vt100::Parser::new(30, 120, 0);
        p.process(&out);
        (ed.screen.chat_input.get().unwrap(), !p.screen().hide_cursor())
    };
    let (at, cursor) = frame(&mut ed);
    assert_eq!((at.rows, at.first, cursor), (ROWS, 12 - ROWS, true), "its end in sight, the cursor there");
    let wheel = |ed: &mut Editor, kind| {
        ed.unchanged = false;
        ed.handle_mouse(Mouse { kind, x: at.x as u16, y: at.y as u16, alt: false, ctrl: false });
    };
    let before = input(&ed);
    wheel(&mut ed, MouseKind::ScrollDown);
    assert!(ed.unchanged, "at its end: nothing to scroll, nothing drawn");
    wheel(&mut ed, MouseKind::ScrollUp);
    let (now, cursor) = frame(&mut ed);
    assert_eq!(now.first, 12 - ROWS - 1, "a row up");
    assert!(!cursor, "the cursor's row out of sight: no cursor drawn");
    // (the keys in the code: the wheel over the input leaves them there)
    ed.workspace.viewer.keys_to_code();
    wheel(&mut ed, MouseKind::ScrollUp);
    assert_eq!(frame(&mut ed).0.first, 12 - ROWS - 2, "two rows up");
    assert_eq!(input(&ed), before, "the text and the cursor as they were");
    assert_eq!(ed.keys_to().region.name(), "code", "the keys where they were");
    // (as far as its first row, no further)
    for _ in 0..20 {
        wheel(&mut ed, MouseKind::ScrollUp);
    }
    assert_eq!(frame(&mut ed).0.first, 0);
    assert!(ed.unchanged, "at its top: the last notches moved nothing");
    wheel(&mut ed, MouseKind::ScrollDown);
    assert_eq!(frame(&mut ed).0.first, 1, "and back down a row");
    // A key typed: the cursor's row in sight again
    ed.workspace.set_at(crate::workspace::Here::Chat);
    feed(&mut ed, "!");
    let (now, cursor) = frame(&mut ed);
    assert_eq!((now.first, cursor), (12 - ROWS, true), "typing brings it back");
    assert!(input(&ed).0.ends_with("line 11!"));
}

/// Insert mode's keys edit it: digits are text (no counts), brackets and quotes as typed (no auto-pairs — it's
/// prose), `C-w` a word, `A-ret` a new line, `↑` a line up, `C-a`/`C-e` a line's ends, `C-k` to its end — the
/// code's document and mode untouched.
#[test]
fn the_input_takes_insert_modes_keys() {
    let mut ed = editor("fn main() {}\n");
    feed(&mut ed, "call f(\"x");
    assert_eq!(input(&ed).0, "call f(\"x", "nothing closed for you");
    feed(&mut ed, "\") 42");
    assert_eq!(input(&ed).0, "call f(\"x\") 42", "typed as it is");
    feed(&mut ed, "<C-w><backspace>");
    assert_eq!(input(&ed).0, "call f(\"x\")");
    feed(&mut ed, "<A-ret>second");
    assert_eq!(input(&ed), ("call f(\"x\")\nsecond".to_string(), 18));
    feed(&mut ed, "<up><C-a>!");
    assert_eq!(input(&ed).0, "!call f(\"x\")\nsecond");
    feed(&mut ed, "<C-e>;<down><C-a><C-k>");
    assert_eq!(input(&ed), ("!call f(\"x\");\n".to_string(), 14));
    assert_eq!(ed.doc().text.to_string(), "fn main() {}\n", "the code untouched");
    assert_eq!(ed.mode, Mode::Normal, "the code's mode untouched");
    assert_eq!(ed.keys_to().region.name(), "chat");
}

/// A sequence of `[insert]`'s under way (Helix's `j k` escape): its `j` waits, said on the status line; `enter`
/// sends the message with it, never leaves it for the next one; `j` then `k` runs what it's bound to.
#[test]
fn an_insert_sequence_under_way_goes_with_the_message() {
    let (config, w) = crate::config::parse("[keys.insert]\nj = { k = \"normal_mode\" }\n");
    assert!(w.is_empty(), "{w:?}");
    let mut ed = Editor::new(config);
    (ed.config.llm.command, ed.config.llm.args) = ("sh".into(), vec!["-c".into(), "cat > /dev/null".into()]);
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.set_at(crate::workspace::Here::Chat);
    feed(&mut ed, "hej");
    assert_eq!(input(&ed).0, "he", "the j waits for the next key");
    assert_eq!(ed.pending_display(), "j", "and the status line says it");
    feed(&mut ed, "<ret>");
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert!(
        c.msgs.iter().any(|m| m.text == "hej"),
        "sent with its j: {:?}",
        c.msgs.iter().map(|m| &m.text).collect::<Vec<_>>()
    );
    feed(&mut ed, "x");
    assert_eq!(input(&ed).0, "x", "no j left for the next message");
    assert_eq!(ed.pending_display(), "");
    // (a sequence broken by a key of the chat's own is typed too, before it: `j` then `C-a`)
    feed(&mut ed, "j<C-a>");
    assert_eq!(input(&ed), ("xj".to_string(), 0));
    // (and by a paste: the `j` goes in first, the pasted text after it — nothing left for the next key)
    feed(&mut ed, "<C-e>j");
    ed.handle_paste("X".into());
    assert_eq!(input(&ed).0, "xjjX");
    assert_eq!(ed.pending_display(), "");
    feed(&mut ed, "y");
    assert_eq!(input(&ed).0, "xjjXy");
    // (the input's text replaced — a message recalled, sent, cleared: the held `j` goes with it)
    feed(&mut ed, "j");
    ed.workspace.claude.chat.as_mut().unwrap().input.set("recalled");
    feed(&mut ed, "!");
    assert_eq!(input(&ed).0, "recalled!");
}

/// A paste as the terminal hands it in (`Event::Paste`) while an `[insert]` sequence waits: its `j` goes in first,
/// the pasted text after it (`hej` + `X` → `hejX`, never `heXj`), and the sequence is over — a `k` next is typed.
#[test]
fn a_paste_breaks_off_an_insert_sequence_under_way() {
    let (config, w) = crate::config::parse("[keys.insert]\nj = { k = \"normal_mode\" }\n");
    assert!(w.is_empty(), "{w:?}");
    let mut ed = Editor::new(config);
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.set_at(crate::workspace::Here::Chat);
    feed(&mut ed, "hej");
    assert_eq!(ed.pending_display(), "j");
    ed.handle_event(crate::event::Event::Paste("X".into()));
    assert_eq!(input(&ed), ("hejX".to_string(), 4));
    assert_eq!(ed.pending_display(), "", "nothing left waiting");
    feed(&mut ed, "k");
    assert_eq!(input(&ed).0, "hejXk", "a k after it is text");
    assert_eq!(ed.keys_to().region.name(), "chat", "still the input's keys");
}

/// A `C-`/`A-` key nothing binds is said with where `esc` goes from here: the code while there's nothing to
/// read, normal mode on the conversation once there is (as the input's hint says it).
#[test]
fn an_unused_key_says_where_esc_goes() {
    let mut ed = editor("fn main() {}\n");
    let said = |ed: &Editor| ed.toasts.last().map(|t| t.text.clone()).unwrap_or_default();
    feed(&mut ed, "<A-z>");
    assert!(said(&ed).ends_with("esc code"), "{}", said(&ed));
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, "a question", ""));
    feed(&mut ed, "<A-z>");
    assert!(said(&ed).ends_with("esc normal"), "{}", said(&ed));
}

/// #416: the input wraps at words as the transcript does — every row after a line's first starts on a word (none on a
/// blank: one cell further in than the rest), a blank may hang in a row's last cell, text leaves it free; a word longer
/// than the row still breaks; the rows hold the text whole, a line break their only gap.
#[test]
fn the_inputs_rows_break_at_words() {
    use unicode_width::UnicodeWidthStr as _;
    let text = "The wheel over the chat input scrolls its rows past them, out of sight.\n\
                a_word_far_longer_than_one_row_is_broken_where_it_must and then on\n\nend 中文 字";
    for w in [12, 22, 30, 41] {
        let room = w - 2;
        let laid = crate::input::Input::from(text).laid(w);
        let rows: Vec<(&str, usize)> = (0..laid.height()).map(|i| laid.row(i)).collect();
        // (the rows, line breaks between them where the text has one, are the text)
        let mut whole = String::new();
        for (i, (row, start)) in rows.iter().enumerate() {
            if i > 0 && text[..*start].ends_with('\n') {
                whole.push('\n');
            }
            whole.push_str(row);
        }
        assert_eq!(whole, text, "{w}: {rows:?}");
        for (i, (row, start)) in rows.iter().enumerate() {
            let wraps = rows.get(i + 1).is_some_and(|(_, next)| !text[..*next].ends_with('\n'));
            assert!(row.trim_end().width() < room, "{w}: {row:?} leaves its last cell to a blank");
            assert!(row.width() <= room, "{w}: {row:?} wider than {room}");
            let continued = *start > 0 && !text[..*start].ends_with('\n');
            assert!(!(continued && row.starts_with(' ')), "{w}: {row:?} starts on a blank");
            // (broken mid-word only where the word is longer than a row)
            if wraps && !row.ends_with(' ') {
                let next = rows[i + 1].0;
                let word = row.rsplit(' ').next().unwrap_or(row).to_string()
                    + next.split(' ').next().unwrap_or(next);
                assert!(word.width() >= room - 1, "{w}: {row:?} | {next:?} breaks a word that fits a row");
            }
        }
    }
    // (at 22: the rows as read)
    let laid = crate::input::Input::from("past them, out of sight now").laid(22);
    let rows: Vec<&str> = (0..laid.height()).map(|i| laid.row(i).0).collect();
    assert_eq!(rows, ["past them, out of ", "sight now"]);
    // (the cursor after the hanging blank: at that row's end, in the row)
    let mut i = crate::input::Input::from("past them, out of sight now");
    i.set_at("past them, out of sight now", "past them, out of ".len());
    assert_eq!(i.laid(22).cursor(), (0, 18));
}

/// #416: a draft taller than the input's rows has a thumb in the cell at their right — the docked chat's outer cell, as
/// the transcript's above it, the ask line's inside its frame's right line —, sized by the share in sight and where the
/// wheel left them; none while the draft fits.
#[test]
fn a_draft_taller_than_its_rows_has_a_thumb() {
    use crate::event::MouseKind;
    for asking in [false, true] {
        let mut ed = editor("fn main() {}\n");
        let thumbs = |ed: &mut Editor| {
            let s = frame(ed);
            let at = ed.screen.chat_input.get().unwrap();
            let (_, w) = s.size();
            let rows: Vec<usize> = (0..at.rows)
                .filter(|&r| {
                    (at.x as u16..w)
                        .any(|x| s.cell((at.y + r) as u16, x).is_some_and(|c| c.contents() == "▎"))
                })
                .collect();
            (at, rows)
        };
        let set = |ed: &mut Editor, text: &str| {
            let c = ed.workspace.claude.chat.as_mut().unwrap();
            c.input = text.into();
            c.asking = asking;
        };
        set(&mut ed, "a short draft");
        assert_eq!(thumbs(&mut ed).1, Vec::<usize>::new(), "asking {asking}: it fits, no thumb");
        let draft: String = (0..40).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        set(&mut ed, &draft);
        let (at, rows) = thumbs(&mut ed);
        assert_eq!(rows, [at.rows - 1], "asking {asking}: its end in sight, the thumb at the foot");
        for _ in 0..40 {
            mouse(&mut ed, MouseKind::ScrollUp, (at.x, at.y));
        }
        assert_eq!(thumbs(&mut ed).1, [0], "asking {asking}: wheeled to the top, the thumb there");
    }
}

/// A frame, as the screen shows it.
fn frame(ed: &mut Editor) -> vt100::Screen {
    let (w, h) = (140, 34);
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, w, h).unwrap();
    let mut p = vt100::Parser::new(h, w, 0);
    p.process(&bytes);
    p.screen().clone()
}

fn mouse(ed: &mut Editor, kind: crate::event::MouseKind, (x, y): (usize, usize)) {
    ed.handle_mouse(crate::event::Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
}

/// A press in the input puts its cursor on the character pressed and a drag from there selects to the one under the
/// pointer, both included, as in the code's insert mode — forward and back, on a wrapped row too; insert mode kept,
/// the keys the input's; the selection washed as the code's is.
#[test]
fn a_drag_in_the_input_selects_as_in_the_codes_insert_mode() {
    use crate::event::MouseKind as K;
    let mut ed = editor("fn main() {}\n");
    // (colours drawn though tarae's test runner sets NO_COLOR; meok's wash)
    crossterm::style::force_color_output(true);
    ed.theme = crate::theme::load("meok").unwrap();
    let long = (0..14).map(|i| format!("word{i:02}")).collect::<Vec<_>>().join(" ");
    feed(&mut ed, &format!("one two three {long}"));
    let screen = frame(&mut ed).contents();
    let chat_x = ed.screen.chat_x().unwrap();
    let cell = |word: &str| {
        screen.lines().enumerate().find_map(|(y, l)| {
            let l: String = l.chars().skip(chat_x).collect();
            let b = l.find(word)?;
            Some((chat_x + l[..b].chars().count(), y))
        })
    };
    let selected = |ed: &Editor| {
        let d = ed.workspace.claude.chat.as_ref().unwrap().input.pane.doc();
        let r = d.selection().primary();
        (d.text.byte_slice(r.from()..r.to()).to_string(), r.head)
    };
    let (x, y) = cell("two").unwrap();
    mouse(&mut ed, K::Down, (x + 1, y));
    assert_eq!(selected(&ed), (String::new(), 5), "a press: the cursor on the `w` pressed");
    mouse(&mut ed, K::Drag, (x + 2, y));
    mouse(&mut ed, K::Drag, (x + 6, y));
    assert_eq!(selected(&ed), ("wo thr".to_string(), 11), "to the `r` under the pointer, both in");
    mouse(&mut ed, K::Up, (x + 6, y));
    assert_eq!(selected(&ed).0, "wo thr", "the release keeps it");
    let painted = frame(&mut ed);
    let bg = |dx: usize| painted.cell(y as u16, (x + dx) as u16).unwrap().bgcolor();
    assert_eq!(painted.contents(), screen, "the text drawn as it was");
    assert_ne!(bg(1), bg(0), "what's selected washed");
    assert_eq!(bg(6), bg(1), "to its last character");
    assert_eq!(bg(7), bg(0), "and no further");
    let mode = ed.workspace.claude.chat.as_ref().unwrap().input.pane.keys.mode;
    assert_eq!(
        (ed.keys_to().region.name(), mode),
        ("chat".to_string(), Mode::Insert),
        "its keys, insert mode"
    );
    // (back: from the press's character to the one under the pointer, the press's still in)
    let (x, y) = cell("three").unwrap();
    mouse(&mut ed, K::Down, (x + 2, y));
    mouse(&mut ed, K::Drag, (x - 4, y));
    assert_eq!(selected(&ed), ("two thr".to_string(), 4));
    // (a wrapped row: its cells are its own characters)
    let first = cell("one").unwrap().1;
    let (word, (x, y)) = (0..14)
        .rev()
        .map(|i| format!("word{i:02}"))
        .find_map(|w| cell(&w).filter(|c| c.1 > first).map(|c| (w, c)))
        .expect("the input wraps");
    mouse(&mut ed, K::Down, (x, y));
    mouse(&mut ed, K::Drag, (x + 5, y));
    assert_eq!(selected(&ed).0, word);
    // (past a wrapped row's end: its last character, never the next row's first — as in the code)
    let w = ed.screen.chat_input.get().unwrap().w;
    let first_row = ed.workspace.claude.chat.as_ref().unwrap().input.laid(w).row(0).0.to_string();
    let (x, y) = cell("one").unwrap();
    mouse(&mut ed, K::Down, (x, y));
    mouse(&mut ed, K::Drag, (139, y));
    assert_eq!(selected(&ed).0, first_row, "the first row whole, nothing of the second");
    assert_eq!(ed.doc().text.to_string(), "fn main() {}\n", "the code untouched");
}

/// What the code's `.` repeats stays the code's last insert — never what was typed in the chat.
#[test]
fn the_codes_repeat_stays_the_codes() {
    let mut ed = editor("abc\n");
    ed.workspace.viewer.keys_to_code();
    feed(&mut ed, "ihey<esc>");
    ed.workspace.set_at(crate::workspace::Here::Chat);
    feed(&mut ed, "zzz");
    ed.workspace.viewer.keys_to_code();
    feed(&mut ed, ".");
    let code = ed.doc().text.to_string();
    assert_eq!(code.matches("hey").count(), 2, "{code:?}");
    assert!(!code.contains('z'), "{code:?}");
    assert_eq!(input(&ed).0, "zzz");
}
