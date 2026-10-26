//! The chat's input as a buffer: insert mode's keys on its document, `[chat]` over them, the chat's own first.

use super::*;
use crate::config::Config;

/// An editor with the chat up and its input with the keys, the code holding `code`.
fn editor(code: &str) -> Editor {
    let mut ed = Editor::new(Config::default());
    ed.docs[0].text = ropey::Rope::from_str(code);
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.viewer.at = crate::workspace::At::Chat;
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
    ed.workspace.viewer.at = crate::workspace::At::Chat;
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
    ed.workspace.viewer.at = crate::workspace::At::Chat;
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
    let chat_x = ed.screen.chat_x.unwrap();
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
    ed.workspace.viewer.at = crate::workspace::At::Code;
    feed(&mut ed, "ihey<esc>");
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    feed(&mut ed, "zzz");
    ed.workspace.viewer.at = crate::workspace::At::Code;
    feed(&mut ed, ".");
    let code = ed.doc().text.to_string();
    assert_eq!(code.matches("hey").count(), 2, "{code:?}");
    assert!(!code.contains('z'), "{code:?}");
    assert_eq!(input(&ed).0, "zzz");
}
