use super::*;
use crate::event::{Event, Mouse, MouseKind};
use crate::uistate::tests::{chat, editor, frame};
use crate::workspace::At;

/// The spot of the first word that does `press`, on the frame last drawn.
fn spot(ed: &Editor, press: &Press) -> Spot {
    let spots = ed.screen.word_spots.borrow();
    let found = spots.iter().find(|s| s.press == *press).cloned();
    found.unwrap_or_else(|| {
        panic!("no {press:?} among {:?}", spots.iter().map(|s| &s.press).collect::<Vec<_>>())
    })
}

fn mouse(ed: &mut Editor, kind: MouseKind, (x, y): (usize, usize)) {
    ed.handle_event(Event::Mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false }));
}

/// A press and its release on the middle of `press`'s word, then a frame.
fn press_word(ed: &mut Editor, press: &Press) {
    let s = spot(ed, press);
    let at = ((s.from + s.to) / 2, s.y);
    mouse(ed, MouseKind::Down, at);
    mouse(ed, MouseKind::Up, at);
    frame(ed);
}

/// Claude at work in the chat (the panel open), follow on.
fn working(ed: &mut Editor) {
    chat(ed, "");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.state = crate::chat::State::Thinking;
    c.follow.on = true;
    c.unfolded = true;
    frame(ed);
}

/// frictions #147: after follow paused, `paused` on the chat's top line pressed follows again — and `follow` pressed
/// stops it, as `C-f` in the chat does.
#[test]
fn the_chats_follow_word_is_c_f() {
    let mut ed = editor();
    working(&mut ed);
    ed.workspace.claude.chat.as_mut().unwrap().follow.paused = true;
    frame(&mut ed);
    assert_eq!(spot(&ed, &Press::Follow).on, Surface::Chat);
    crate::clock::hold();
    press_word(&mut ed, &Press::Follow);
    let f = &ed.workspace.claude.chat.as_ref().unwrap().follow;
    assert!(f.on && !f.paused, "paused → following again");
    // (past a double's while: a press of its own)
    crate::clock::advance(crate::editor::DOUBLE_CLICK);
    press_word(&mut ed, &Press::Follow);
    crate::clock::release();
    assert!(!ed.workspace.claude.chat.as_ref().unwrap().follow.on, "following → off");
    assert!(ed.screen.word_spots.borrow().iter().all(|s| s.press != Press::Follow), "off: no word for it");
}

/// The turn's word at the chat's top line's right end follows Claude's place, as the status line's does; idle, the
/// model's name is `C-t`'s.
#[test]
fn the_turns_word_follows_claudes_place() {
    let mut ed = editor();
    working(&mut ed);
    let f = &mut ed.workspace.claude.chat.as_mut().unwrap().follow;
    (f.on, f.paused) = (false, false);
    frame(&mut ed);
    let now = ed
        .screen
        .word_spots
        .borrow()
        .iter()
        .find(|s| s.press == Press::Now && s.on == Surface::Chat)
        .cloned();
    let now = now.expect("the turn's word on the chat's top line");
    assert!(now.spans.iter().any(|s| s.0.contains("thinking")), "{:?}", now.spans);
    mouse(&mut ed, MouseKind::Down, (now.from, now.y));
    assert!(ed.workspace.claude.chat.as_ref().unwrap().follow.active(), "following Claude's place");
}

/// `▲ waits` on the follow card is `C-q`'s: the card it waits on gets the keys.
#[test]
fn the_follow_cards_waits_is_c_q() {
    let mut ed = editor();
    working(&mut ed);
    // (minimized: the follow card stands for the chat)
    ed.workspace.claude.chat.as_mut().unwrap().unfolded = false;
    ed.workspace.viewer.at = At::Code;
    let what = crate::offer::What::Asks { call: crate::tools::Call::test(1) };
    ed.push_offer(crate::offer::Offer::new("Claude asks".into(), "?", "", String::new(), what));
    frame(&mut ed);
    let waits = Press::Card;
    let s = spot(&ed, &waits);
    assert_eq!((s.on, s.spans[0].0.as_str()), (Surface::FollowCard, "▲ waits"));
    assert!(!ed.card_has_keys());
    press_word(&mut ed, &waits);
    assert!(ed.card_has_keys(), "the card has the keys");
}

/// A key hint's pair at the chat's foot does its key: `esc code` gives the code the keys, and from the code
/// `space l focus` gives them back.
#[test]
fn a_hint_pair_does_its_key() {
    let mut ed = editor();
    chat(&mut ed, "");
    frame(&mut ed);
    let esc = Press::Keys(crate::chatinput::esc_said(&ed).0);
    assert_eq!(spot(&ed, &esc).spans[0].0, "esc");
    press_word(&mut ed, &esc);
    assert!(ed.workspace.is_at(At::Code), "esc: the code's");
    press_word(&mut ed, &Press::Keys("space l".into()));
    assert!(ed.workspace.is_at(At::Chat), "space l: the chat's");
}

/// A double on a word is one press: a toggle isn't undone by a habit's second click.
#[test]
fn a_double_on_a_word_is_one_press() {
    let mut ed = editor();
    working(&mut ed);
    let s = spot(&ed, &Press::Follow);
    crate::clock::hold();
    for _ in 0..2 {
        mouse(&mut ed, MouseKind::Down, (s.from, s.y));
        mouse(&mut ed, MouseKind::Up, (s.from, s.y));
    }
    crate::clock::release();
    assert!(!ed.workspace.claude.chat.as_ref().unwrap().follow.on, "pressed once: off");
}

/// The status line's words do their commands wherever the keys are — from the chat's input `space g` would be typed:
/// `full screen C-w z back` brings the panes back.
#[test]
fn the_status_lines_words_do_their_commands() {
    let mut ed = editor();
    chat(&mut ed, "");
    ed.handle_key("C-w".parse().unwrap());
    ed.handle_key("z".parse().unwrap());
    frame(&mut ed);
    assert!(ed.zoomed().is_some());
    let zoom = Press::Command("zoom_toggle");
    let s = spot(&ed, &zoom);
    assert_eq!((s.on, s.y), (Surface::Status, 39));
    press_word(&mut ed, &zoom);
    assert!(ed.zoomed().is_none(), "the panes back");
    assert_eq!(ed.workspace.claude.chat.as_ref().unwrap().input.text(), "", "nothing typed");
}

/// The mouse over a word lights it — drawn again (a frame) and gone as the mouse leaves; under a card, nothing is lit
/// nor pressed.
#[test]
fn the_mouse_over_a_word_lights_it() {
    let mut ed = editor();
    working(&mut ed);
    let s = spot(&ed, &Press::Follow);
    ed.handle_event(Event::Hover(s.from as u16, s.y as u16));
    assert!(!ed.unchanged, "drawn: it lights");
    frame(&mut ed);
    assert_eq!(ed.word_hover.get(), Some((s.y, s.from, s.to)));
    ed.handle_event(Event::Hover(s.from as u16, s.y as u16 + 5));
    assert!(!ed.unchanged, "drawn: it goes");
    frame(&mut ed);
    assert_eq!(ed.word_hover.get(), None);
    // (where it is, again: nothing new under it — the surface's own half light came with the move before)
    ed.handle_event(Event::Hover(s.from as u16, s.y as u16 + 5));
    assert!(ed.unchanged, "a move over nothing draws nothing");
}

/// frictions #197a: a word under the mouse keeps its text's colour, only its ground lightens — every kind of word, drawn
/// faded (a buffer's name on the code's line, the chat having the keys) or at full strength (the chat's own), meok and
/// hanji: each cell's text colour as it was, its ground another. One already on the selected row's ground (#197's
/// review: a name flashing, brought into sight from a link — the pane's buffer tab, the slot's tab) on another still.
#[test]
fn a_lit_word_keeps_its_texts_colour() {
    // (colours drawn though tarae's test runner sets NO_COLOR)
    crossterm::style::force_color_output(true);
    for theme in ["meok", "hanji"] {
        let mut ed = editor();
        ed.theme = crate::theme::load(theme).unwrap();
        let mut other = crate::document::Document::from_str(77, "other\n");
        other.path.set(Some(ed.workspace.root.join("other.rs")));
        ed.docs.push(other);
        // (a task's run in the slot: its tab on the slot's line)
        let t = crate::task::Task {
            name: "check".into(),
            cmd: "cargo check".into(),
            cwd: ed.workspace.root.clone(),
        };
        ed.workspace.task_run = Some(crate::task::Run::for_test(t));
        working(&mut ed);
        let keys = |s: &Spot| matches!(&s.press, Press::Keys(_)) && s.on == Surface::Chat;
        let words: Vec<Spot> = {
            let spots = ed.screen.word_spots.borrow();
            let of = |p: Press| spots.iter().find(|s| s.press == p).cloned();
            [of(Press::Buffer(77)), of(Press::Follow), spots.iter().find(|s| keys(s)).cloned()]
                .into_iter()
                .map(|s| s.expect("a buffer's name, `follow`, a hint at the chat's foot"))
                .collect()
        };
        // (the slot's tab: a word with a record of its own)
        let tab = |e: &&crate::kinds::Element| {
            matches!(e.kind, crate::kinds::Kind::Button(crate::mouse::Part::Word(crate::mouse::Word::Tab(_))))
        };
        let own = ed.screen.elements.borrow().iter().find(tab).map(|e| (e.y, e.from, e.to));
        let own = own.unwrap_or_else(|| panic!("{theme}: a word with a record of its own"));
        // (flashing: the buffer's pane, the slot's run)
        let pane = match words[0].on {
            Surface::PaneTop(id) => format!("pane#{id}"),
            on => panic!("{theme}: the buffer's name on {on:?}"),
        };
        let run = format!("run#{}", ed.workspace.task_run.as_ref().unwrap().trace.id());
        let flashing = [(Some(pane), (words[0].y, words[0].from, words[0].to)), (Some(run), own)];
        // (the clock held: a flash lasts the whole test, whatever the machine's pace)
        let held = crate::clock::hold();
        let cells = |ed: &mut Editor, flash: &Option<String>, (y, a, b): (usize, usize, usize)| {
            ed.ui_flash = flash.clone().map(|id| (id, held));
            ed.focus_sync();
            let mut bytes = Vec::new();
            crate::term::render(ed, &mut bytes, 120, 40).unwrap();
            let mut p = vt100::Parser::new(40, 120, 0);
            p.process(&bytes);
            let cell =
                |x: usize| p.screen().cell(y as u16, x as u16).map(|c| (c.fgcolor(), c.bgcolor())).unwrap();
            (a..b).map(cell).collect::<Vec<_>>()
        };
        let at = words.iter().map(|s| (None, (s.y, s.from, s.to))).chain([(None, own)]).chain(flashing);
        for (flash, (y, a, b)) in at {
            let said = format!(
                "{theme}: row {y} {a}..{b}{}",
                flash.as_ref().map_or(String::new(), |f| format!(", {f} flashing"))
            );
            let before = cells(&mut ed, &flash, (y, a, b));
            ed.handle_event(Event::Hover(a as u16, y as u16));
            let lit = cells(&mut ed, &flash, (y, a, b));
            assert_eq!(ed.word_hover.get().map(|w| w.0), Some(y), "{said}: lit");
            let fg = |c: &[(vt100::Color, vt100::Color)]| c.iter().map(|c| c.0).collect::<Vec<_>>();
            assert_eq!(fg(&lit), fg(&before), "{said}: keeps its text's colour");
            assert!(lit.iter().zip(&before).all(|(l, b)| l.1 != b.1), "{said}: its ground lit");
            // (the mouse off it, onto the code's text)
            ed.handle_event(Event::Hover(5, 3));
            assert_eq!(cells(&mut ed, &flash, (y, a, b)), before, "{said}: let go");
        }
        crate::clock::release();
    }
}

fn until(ed: &mut Editor, what: &str, done: impl Fn(&Editor) -> bool) {
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    while !done(ed) {
        assert!(std::time::Instant::now() < end, "timed out waiting for {what}");
        if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
            ed.handle_event(ev);
        }
    }
}

/// What was said last.
fn status(ed: &Editor) -> String {
    ed.messages.last().map(|m| m.2.clone()).unwrap_or_default()
}

/// `C-c stop` at the chat's foot, pressed while a turn runs: the turn is stopping — the interrupt reaches its claude (a
/// stand-in writing what it's sent to a file) — and it's said at once, the turn ending as claude answers. Minimized, with
/// no claude behind the turn, it says there's nothing to stop.
#[test]
fn stop_pressed_stops_the_turn() {
    let dir = crate::scratch::Scratch::new("wordspot-stop");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut ed = editor();
    ed.workspace.root = root.clone();
    let sent = root.join("sent");
    ed.config.llm.args = vec!["-c".into(), format!("cat > '{}'", sent.display())];
    chat(&mut ed, "hello");
    ed.workspace.claude.chat.as_mut().unwrap().unfolded = true;
    crate::chat::send(&mut ed);
    until(&mut ed, "the turn under way, its claude running", |ed| {
        ed.workspace.claude.chat.as_ref().is_some_and(|c| c.busy() && c.running())
    });
    frame(&mut ed);
    let stop = Press::Keys("C-c".into());
    assert_eq!(spot(&ed, &stop).on, Surface::Chat);
    assert!(!ed.workspace.claude.chat.as_ref().unwrap().stopping);
    press_word(&mut ed, &stop);
    assert!(ed.workspace.claude.chat.as_ref().unwrap().stopping, "the turn stopping");
    assert_eq!(status(&ed), "stopping the turn");
    let read = || std::fs::read_to_string(&sent).unwrap_or_default();
    until(&mut ed, "the interrupt at its claude", |_| read().contains("\"tarae-stop\""));
    assert!(read().contains("\"interrupt\""), "{}", read());
    // (no claude behind a turn: nothing stops, and that's said)
    let mut ed = editor();
    working(&mut ed);
    ed.workspace.claude.chat.as_mut().unwrap().unfolded = false;
    frame(&mut ed);
    assert_eq!(spot(&ed, &stop).on, Surface::FollowCard);
    press_word(&mut ed, &stop);
    assert!(!ed.workspace.claude.chat.as_ref().unwrap().stopping);
    assert_eq!(status(&ed), "nothing to stop: its claude isn't running");
}

/// A key hint's keys go only in normal mode: in INSERT `space l` would be typed into the file — refused, said, nothing
/// typed. A sequence under way (a count and `g`) ends as any press ends it (the mouse's one rule, landing O), then the
/// hint's keys go — never joined to it: nothing moved by a `2g…`.
#[test]
fn a_hint_pressed_mid_insert_is_refused_and_mid_sequence_ends_it_then_goes() {
    let mut ed = editor();
    chat(&mut ed, "");
    ed.workspace.viewer.at = At::Code;
    let text = ed.doc().text.to_string();
    let focus = Press::Keys("space l".into());
    crate::clock::hold();
    ed.handle_key("i".parse().unwrap());
    frame(&mut ed);
    press_word(&mut ed, &focus);
    assert!(status(&ed).starts_with("insert mode: esc first"), "{}", status(&ed));
    assert_eq!(
        (ed.doc().text.to_string(), ed.mode),
        (text.clone(), crate::editor::Mode::Insert),
        "nothing typed"
    );
    assert!(ed.workspace.is_at(At::Code));
    ed.handle_key("esc".parse().unwrap());
    ed.handle_key("2".parse().unwrap());
    ed.handle_key("g".parse().unwrap());
    frame(&mut ed);
    let at = ed.doc().selection().clone();
    // (past a double's while: a press of its own)
    crate::clock::advance(crate::editor::DOUBLE_CLICK);
    press_word(&mut ed, &focus);
    crate::clock::release();
    assert!(ed.pending_display().is_empty(), "the sequence ended: {:?}", ed.pending_display());
    assert_eq!((ed.doc().text.to_string(), ed.doc().selection()), (text, &at), "nothing changed or moved");
    assert!(ed.workspace.is_at(At::Chat), "then space l's: the chat has the keys");
}

/// What the last frame shows at `s`'s columns of its row.
fn drawn(ed: &Editor, s: &Spot) -> String {
    let text = crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), 120, 40).unwrap();
    let row: Vec<char> = text.lines().nth(s.y).unwrap_or_default().chars().collect();
    row.get(s.from..s.to).map(|c| c.iter().collect()).unwrap_or_default()
}

/// The spot of the first word that does `press` on `on`, on the frame last drawn.
fn spot_on(ed: &Editor, on: Surface, press: &Press) -> Spot {
    let spots = ed.screen.word_spots.borrow();
    let found = spots.iter().find(|s| s.on == on && s.press == *press).cloned();
    found.unwrap_or_else(|| panic!("no {press:?} on {on:?} among {spots:?}"))
}

/// A page being read, the keys in the chat (a draft in it): its line's `C-c stop`, paired as drawn — the key and its
/// words, at their columns — pressed, stops the page's read in that pane; the chat's turn and its input untouched.
#[test]
fn a_panes_hint_as_drawn_goes_to_that_pane() {
    let mut ed = editor();
    crate::web::page::stand_in(&mut ed, "https://example.com/a", "a page\n");
    chat(&mut ed, "hello");
    frame(&mut ed);
    assert!(ed.workspace.is_at(At::Chat));
    let stop = Press::Keys("C-c".into());
    let s = spot_on(&ed, Surface::PaneTop(ed.workspace.viewer.focus), &stop);
    let words: Vec<&str> = s.spans.iter().map(|s| s.0.as_str()).collect();
    assert_eq!((words, drawn(&ed, &s).as_str(), s.home), (vec!["C-c", " stop"], "C-c stop", Some(At::Code)));
    press_word(&mut ed, &stop);
    assert!(ed.doc().page.as_ref().is_some_and(|p| p.fetching.is_none()), "the page's read stopped");
    assert!(ed.workspace.is_at(At::Code), "a pane's line: the keys stay in the pane");
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert_eq!((c.input.text().to_string(), c.msgs.len()), ("hello".to_string(), 0), "the chat untouched");
}

/// A hint goes to the surface it was drawn for, wherever the keys went since: the minimized chat's `C-c stop` with the
/// keys in the code stops the turn (here: says there's nothing to stop) — never the code's `C-c`, its comment; the chat's
/// foot `enter send` with the keys in the code sends the chat's draft, never the code's `enter`.
#[test]
fn a_hint_goes_to_the_surface_it_was_drawn_for() {
    let mut ed = editor();
    ed.docs[0].text = ropey::Rope::from_str("fn a() {}\n");
    working(&mut ed);
    ed.workspace.claude.chat.as_mut().unwrap().unfolded = false;
    frame(&mut ed);
    let stop = Press::Keys("C-c".into());
    let s = spot_on(&ed, Surface::FollowCard, &stop);
    assert_eq!((drawn(&ed, &s).as_str(), s.home), ("C-c stop", Some(At::Chat)));
    ed.workspace.viewer.at = At::Code;
    press_word(&mut ed, &stop);
    assert_eq!(ed.doc().text.to_string(), "fn a() {}\n", "no comment toggled");
    assert_eq!(status(&ed), "nothing to stop: its claude isn't running");
    assert!(ed.workspace.is_at(At::Code), "it acted in the chat: the keys back in the code");
    // The chat's foot
    let mut ed = editor();
    chat(&mut ed, "hello");
    ed.workspace.claude.chat.as_mut().unwrap().unfolded = true;
    frame(&mut ed);
    let send = Press::Keys("enter".into());
    let s = spot_on(&ed, Surface::Chat, &send);
    assert_eq!((drawn(&ed, &s).as_str(), s.home), ("enter send", Some(At::Chat)));
    ed.workspace.viewer.at = At::Code;
    let (text, at) = (ed.doc().text.to_string(), ed.doc().selection().clone());
    press_word(&mut ed, &send);
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert!(c.msgs.iter().any(|m| m.text == "hello"), "the chat's draft sent");
    assert_eq!((ed.doc().text.to_string(), ed.doc().selection()), (text, &at), "the code untouched");
    assert!(ed.workspace.is_at(At::Code), "it acted in the chat: the keys back in the code");
}

/// Two panes on a page being read: the other pane's `C-c stop` pressed focuses that pane, stops the read there, and the
/// focus stays on it — a press gives the keys to what it lands on (F); never given back to the pane focused before.
#[test]
fn a_panes_hint_focuses_that_pane_and_it_keeps_the_focus() {
    let mut ed = editor();
    crate::web::page::stand_in(&mut ed, "https://example.com/a", "a page\n");
    // (two panes side by side on the page, each its line's `C-c stop`; the keys in the new one)
    ed.handle_key("C-w".parse().unwrap());
    ed.handle_key("v".parse().unwrap());
    frame(&mut ed);
    let focused = ed.workspace.viewer.focus;
    let stop = Press::Keys("C-c".into());
    let other = ed.screen.word_spots.borrow().iter().find_map(|s| match s.on {
        Surface::PaneTop(id) if id != focused && s.press == stop => Some(id),
        _ => None,
    });
    let other = other.expect("the other pane's `C-c stop`");
    let s = spot_on(&ed, Surface::PaneTop(other), &stop);
    assert_eq!(drawn(&ed, &s), "C-c stop");
    press_word(&mut ed, &stop);
    assert!(ed.doc().page.as_ref().is_some_and(|p| p.fetching.is_none()), "the page's read stopped");
    assert_eq!(
        ed.workspace.viewer.focus, other,
        "the pane pressed keeps the focus — not given back to {focused}"
    );
    assert!(ed.workspace.is_at(At::Code), "the keys in it");
}

/// A slot's line drawn while the chat had the keys, a draft in it: its run's `space x c close` is the slot's, by what it
/// is — never typed into the chat's draft; pressed, the panel closes and the keys come back to the chat.
#[test]
fn a_slot_lines_hint_drawn_with_the_keys_elsewhere_is_the_slots() {
    let mut ed = editor();
    let lines = (0..20).map(|i| format!("output line {i}")).collect();
    ed.workspace.test_run = Some(crate::testing::TestRun::finished("t", Vec::new(), lines));
    chat(&mut ed, "hello");
    ed.workspace.claude.chat.as_mut().unwrap().unfolded = true;
    frame(&mut ed);
    assert!(ed.workspace.is_at(At::Chat));
    let close = Press::Keys("space x c".into());
    let s = spot_on(&ed, Surface::SlotLine, &close);
    assert_eq!((drawn(&ed, &s).as_str(), s.home), ("space x c close", Some(At::Slot)));
    press_word(&mut ed, &close);
    assert_eq!(
        ed.workspace.claude.chat.as_ref().unwrap().input.text(),
        "hello",
        "nothing typed in the draft"
    );
    assert!(ed.slot().is_none(), "the test panel closed");
    assert!(ed.workspace.is_at(At::Chat), "the keys back in the chat");
}

/// The follow card's foot keys take their home from what each key is, never from where the keys are: drawn with the
/// keys in the side panel, `space l open the chat` is the code's way in — pressed, it opens the chat from the code;
/// drawn with the keys in the minimized chat, `C-c stop` and `C-f unfollow` are the chat's.
#[test]
fn the_follow_cards_keys_are_homed_by_what_they_are() {
    let mut ed = editor();
    working(&mut ed);
    ed.workspace.claude.chat.as_mut().unwrap().unfolded = false;
    ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::with_files(std::env::temp_dir(), Vec::new()));
    ed.workspace.viewer.at = At::Side;
    frame(&mut ed);
    let open = Press::Keys("space l".into());
    let s = spot_on(&ed, Surface::FollowCard, &open);
    assert_eq!((drawn(&ed, &s).as_str(), s.home), ("space l open the chat", Some(At::Code)));
    press_word(&mut ed, &open);
    assert!(ed.workspace.is_at(At::Chat), "the chat opened, its keys — {:?}", ed.keys_to().region);
    // (the keys in the minimized chat: its turn's own keys)
    let mut ed = editor();
    working(&mut ed);
    ed.workspace.claude.chat.as_mut().unwrap().unfolded = false;
    frame(&mut ed);
    for k in ["C-c", "C-f"] {
        assert_eq!(spot_on(&ed, Surface::FollowCard, &Press::Keys(k.into())).home, Some(At::Chat), "{k}");
    }
}

/// The pressable hints are named, not every pair whose key parses: the idle chat's foot has `enter send` as a word and
/// never `C-l new` (a new conversation by a stray click) nor `C-z undo turn`, `C-s screen`, `C-o point`.
#[test]
fn only_named_hints_are_words() {
    let mut ed = editor();
    chat(&mut ed, "");
    ed.workspace.claude.chat.as_mut().unwrap().unfolded = true;
    // (wide enough for the foot's every pair)
    ed.sizes.chat = Some(96);
    frame(&mut ed);
    let on_chat = |ed: &Editor, k: &str| {
        ed.screen
            .word_spots
            .borrow()
            .iter()
            .any(|s| s.on == Surface::Chat && s.press == Press::Keys(k.into()))
    };
    assert!(on_chat(&ed, "enter"), "enter send");
    let text = crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), 120, 40).unwrap();
    assert!(text.contains("C-l new") && text.contains("C-o point"), "the foot shows them: {text}");
    for k in ["C-l", "C-z", "C-s", "C-o"] {
        assert!(!on_chat(&ed, k), "{k} is no word");
    }
    assert!(pressable("enter", "send") && pressable("C-c", " stop") && !pressable("C-l", "new"));
}

/// To the chat's input a word sends a chord, `enter` or `esc` only — letters would be typed into the draft: refused and
/// said, the draft as it was.
#[test]
fn a_word_never_types_letters_into_the_chats_draft() {
    let mut ed = editor();
    chat(&mut ed, "hello");
    frame(&mut ed);
    let pane = Surface::Chat;
    let spot =
        Spot::new(pane, Press::Keys("G".into()), (0, 0), vec![("G".into(), Style::default())]).home(At::Chat);
    ed.screen.word_spots.borrow_mut().push(spot);
    let i = ed.screen.word_spots.borrow().len() - 1;
    press(&mut ed, i);
    assert_eq!(ed.workspace.claude.chat.as_ref().unwrap().input.text(), "hello", "nothing typed");
    assert!(status(&ed).starts_with("G would be typed into the chat's draft"), "{}", status(&ed));
}

/// A hint that means "go there" moves the keys and they stay: the chat's `esc code` leaves them in the code; from the
/// code, the chat's `space l focus` leaves them in the chat — neither given back.
#[test]
fn a_hint_that_goes_there_leaves_the_keys_there() {
    let mut ed = editor();
    chat(&mut ed, "");
    ed.workspace.claude.chat.as_mut().unwrap().unfolded = true;
    frame(&mut ed);
    let esc = Press::Keys("esc".into());
    let s = spot_on(&ed, Surface::Chat, &esc);
    assert_eq!((drawn(&ed, &s).as_str(), s.home), ("esc code", Some(At::Chat)));
    crate::clock::hold();
    press_word(&mut ed, &esc);
    assert!(ed.workspace.is_at(At::Code), "esc code: the code's");
    let focus = Press::Keys("space l".into());
    assert_eq!(spot_on(&ed, Surface::Chat, &focus).home, Some(At::Code));
    // (the keys gone to the side panel since that frame: lent to the code, `space l` takes them on to the chat)
    ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::with_files(std::env::temp_dir(), Vec::new()));
    ed.workspace.viewer.at = At::Side;
    crate::clock::advance(crate::editor::DOUBLE_CLICK);
    press_word(&mut ed, &focus);
    crate::clock::release();
    assert!(ed.workspace.is_at(At::Chat), "space l focus: the chat's, not back to the side panel");
}

/// The minimized chat's card over a word: the press and the light are the card's — a word it covers isn't pressed or
/// lit through it; the card's own words are.
#[test]
fn a_word_under_the_follow_card_is_neither_pressed_nor_lit() {
    let mut ed = editor();
    working(&mut ed);
    ed.workspace.claude.chat.as_mut().unwrap().unfolded = false;
    frame(&mut ed);
    let (x0, y0, w, h) = ed.screen.chat_mini.get().expect("the minimized chat's card");
    // (a word of the code's line drawn under the card's middle row — a pane's, as if its line ran there)
    let (x, y) = (x0 + w / 2, y0 + h / 2);
    let pane = Surface::PaneTop(ed.workspace.viewer.focus);
    let under =
        Spot::new(pane, Press::Keys("enter".into()), (x - 1, y), vec![("ent".into(), Style::default())]);
    ed.screen.word_spots.borrow_mut().push(under.home(At::Code));
    let i = ed.screen.word_spots.borrow().len() - 1;
    let u = ed.mouse_at(x, y);
    assert!(
        u.surface == Surface::FollowCard && u.part != crate::mouse::Part::Word(crate::mouse::Word::Does(i)),
        "{u:?}"
    );
    ed.presence.hover_at = Some((x, y));
    assert_eq!(lit(&ed, x, y), None, "nothing lit through the card");
    // (its own: the turn's word on its top line)
    let own = spot_on(&ed, Surface::FollowCard, &Press::Now);
    assert!(matches!(
        ed.mouse_at(own.from, own.y).part,
        crate::mouse::Part::Word(crate::mouse::Word::Does(_))
    ));
    assert!(lit(&ed, own.from, own.y).is_some(), "its own lit");
}

/// The slot's line as drawn: the way in from the code (`C-w j scroll`) at its columns, typed in the code — from the chat
/// too — gives the slot the keys; then its own keys (`esc code`) are the slot's.
#[test]
fn the_slot_lines_hints_as_drawn() {
    let mut ed = editor();
    let lines = (0..20).map(|i| format!("output line {i}")).collect();
    ed.workspace.test_run = Some(crate::testing::TestRun::finished("t", Vec::new(), lines));
    frame(&mut ed);
    let way_in = Press::Keys("C-w j".into());
    let s = spot_on(&ed, Surface::SlotLine, &way_in);
    assert_eq!((drawn(&ed, &s).as_str(), s.home), ("C-w j scroll", Some(At::Code)));
    // (the keys gone to the side panel since that frame)
    ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::with_files(std::env::temp_dir(), Vec::new()));
    ed.workspace.viewer.at = At::Side;
    press_word(&mut ed, &way_in);
    assert!(ed.workspace.is_at(At::Slot), "the slot's keys — {:?}", ed.keys_to().region);
    let esc = Press::Keys("esc".into());
    let s = spot_on(&ed, Surface::SlotLine, &esc);
    assert_eq!((drawn(&ed, &s).as_str(), s.home), ("esc code", Some(At::Slot)));
    crate::clock::hold();
    crate::clock::advance(crate::editor::DOUBLE_CLICK);
    press_word(&mut ed, &esc);
    crate::clock::release();
    assert!(ed.workspace.is_at(At::Code), "esc: the code's");
}

/// The chat idle: its top line's right end, the model's name as drawn, pressed, is `C-t`'s — the model picker.
#[test]
fn the_models_name_as_drawn_is_c_t() {
    let mut ed = editor();
    chat(&mut ed, "");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.unfolded = true;
    c.model = "claude-stand-in".into();
    frame(&mut ed);
    let s = spot_on(&ed, Surface::Chat, &Press::Model);
    assert_eq!(drawn(&ed, &s), "claude-stand-in");
    press_word(&mut ed, &Press::Model);
    assert!(ed.picker.as_ref().is_some_and(|p| p.kind == "model"), "the model picker");
}

/// The status line's spot of `press`.
fn status_word(ed: &mut Editor, press: &Press) -> Spot {
    frame(ed);
    let spots = ed.screen.word_spots.borrow();
    let found = spots.iter().find(|s| s.on == Surface::Status && s.press == *press).cloned();
    found.unwrap_or_else(|| panic!("no {press:?} on the status line among {spots:?}"))
}

/// The status line's words, each its own: the branch opens the git tab, a process mark `:procs`, the file's warnings its
/// diagnostics, Claude's change waiting the review's keys (`C-q`), the inbox's `· C-q` the card's.
#[test]
fn each_status_line_word_does_its_own() {
    // The branch: the git tab
    let mut ed = editor();
    ed.workspace.git_branch = Some("topic".into());
    let s = status_word(&mut ed, &Press::Command("git_panel"));
    assert_eq!(s.spans[0].0, "topic");
    press_word(&mut ed, &Press::Command("git_panel"));
    assert!(ed.workspace.viewer.sidebar.is_some(), "the git tab");
    // A process mark: `:procs`
    let mut ed = editor();
    let check = crate::procs::Finding {
        key: "test".into(),
        pid: 0,
        what: "a stand-in check".into(),
        short: "a check".into(),
        did: String::new(),
        act: crate::procs::Act::Nothing,
    };
    ed.procs.record(&[check], std::time::SystemTime::now());
    let s = status_word(&mut ed, &Press::Command(":procs"));
    assert!(s.spans.iter().any(|s| s.0.contains("a check")), "{:?}", s.spans);
    press_word(&mut ed, &Press::Command(":procs"));
    assert!(ed.procs.panel.is_some(), ":procs");
    // The file's warnings: its diagnostics
    let mut ed = editor();
    // (a file's: the list is of a file's diagnostics)
    let path = ed.workspace.root.join("a.rs");
    ed.doc_mut().path.set(Some(path));
    ed.doc_mut().lsp.diagnostics = vec![crate::lsp::Diagnostic {
        from: 0,
        to: 3,
        severity: 2,
        message: "a warning".into(),
        raw: serde_json::json!({}),
        source: 0,
    }];
    let s = status_word(&mut ed, &Press::Command("diagnostics_picker"));
    assert_eq!(s.spans[0].0, "▲ 1");
    press_word(&mut ed, &Press::Command("diagnostics_picker"));
    assert!(ed.picker.is_some(), "the diagnostics");
    // Claude's change waiting, the keys in the chat: the review gets them
    let mut ed = editor();
    crate::uistate::tests::edit_review(&mut ed);
    chat(&mut ed, "");
    let s = status_word(&mut ed, &Press::Card);
    assert_eq!(s.spans[0].0, "claude ● change");
    press_word(&mut ed, &Press::Card);
    assert_eq!(ed.keys_to().region, crate::uistate::Region::EditReview, "the review's keys");
    // The inbox's `· C-q`: the card
    let mut ed = editor();
    let what = crate::offer::What::Asks { call: crate::tools::Call::test(1) };
    ed.push_offer(crate::offer::Offer::new("Claude asks".into(), "?", "", String::new(), what));
    let s = status_word(&mut ed, &Press::Card);
    assert!(s.spans.iter().any(|s| s.0.contains("C-q")), "{:?}", s.spans);
    press_word(&mut ed, &Press::Card);
    assert!(ed.card_has_keys(), "the card's keys");
}

/// A file in the project, open, and the side panel's files listed (none of it on disk: the tree is given).
fn in_project(ed: &mut Editor) -> std::path::PathBuf {
    let root = ed.workspace.root.clone();
    let rels = ["Cargo.toml", "src/main.rs", "src/ui/card.rs", "src/ui/list.rs"].map(String::from).to_vec();
    ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::with_files(root.clone(), rels));
    let path = root.join("src/ui/card.rs");
    ed.docs[0].path.set(Some(path.clone()));
    path
}

/// frictions #172: a tab's name on the side panel's top line shows that tab, the keys there — and the tab already in
/// sight pressed keeps it open (a press never closes what it names, where `space o` on it would).
#[test]
fn a_tabs_name_on_the_side_panels_line_shows_that_tab() {
    let mut ed = editor();
    in_project(&mut ed);
    frame(&mut ed);
    let names: Vec<String> = (ed.screen.word_spots.borrow().iter())
        .filter(|s| matches!(s.press, Press::Side(_)))
        .map(|s| s.spans[0].0.clone())
        .collect();
    assert_eq!(names, ["files", "outline", "git"], "each name its word");
    press_word(&mut ed, &Press::Side(crate::sidebar::Mode::Outline));
    let mode = |ed: &Editor| ed.workspace.viewer.sidebar.as_ref().map(|s| s.mode);
    assert_eq!(mode(&ed), Some(crate::sidebar::Mode::Outline));
    assert!(ed.workspace.is_at(At::Side), "the keys there");
    press_word(&mut ed, &Press::Side(crate::sidebar::Mode::Outline));
    assert_eq!(mode(&ed), Some(crate::sidebar::Mode::Outline), "pressed again: still there, never closed");
    assert!(ed.workspace.is_at(At::Side));
    press_word(&mut ed, &Press::Side(crate::sidebar::Mode::Git));
    assert_eq!(mode(&ed), Some(crate::sidebar::Mode::Git));
}

/// frictions #173: a buffer's name at the code's top line's right end switches to it, in that pane, the keys there.
#[test]
fn a_buffers_name_on_the_codes_line_switches_to_it() {
    let mut ed = editor();
    let mut other = crate::document::Document::from_str(77, "other\n");
    other.path.set(Some(ed.workspace.root.join("other.rs")));
    ed.docs.push(other);
    frame(&mut ed);
    let s = spot(&ed, &Press::Buffer(77));
    assert!(s.spans[0].0.contains("other.rs"), "{:?}", s.spans);
    assert!(matches!(s.on, crate::mouse::Surface::PaneTop(_)));
    ed.workspace.viewer.at = At::Chat;
    press_word(&mut ed, &Press::Buffer(77));
    assert_eq!(ed.doc().id, 77, "switched");
    assert!(ed.workspace.is_at(At::Code), "the keys to the code");
    // (the one in sight pressed: stays)
    let first = ed.docs[0].id;
    press_word(&mut ed, &Press::Buffer(77));
    assert_eq!(ed.doc().id, 77);
    press_word(&mut ed, &Press::Buffer(first));
    assert_eq!(ed.doc().id, first, "and back");
}

/// frictions #174: a name in the code's path shows its row in the side panel's files — a folder selected and unfolded,
/// the file its own row, the project its top — the keys there, as `space e`.
#[test]
fn a_name_in_the_codes_path_shows_its_row_in_the_files() {
    use crate::sidebar::Target;
    let mut ed = editor();
    let path = in_project(&mut ed);
    let root = ed.workspace.root.clone();
    ed.workspace.viewer.sidebar = None;
    frame(&mut ed);
    let words: Vec<String> = (ed.screen.word_spots.borrow().iter())
        .filter(|s| matches!(s.press, Press::Reveal(_)))
        .map(|s| s.spans[0].0.clone())
        .collect();
    assert_eq!(words, ["tarae", "src", "ui", "card.rs"], "the project, each folder, the file");
    in_project(&mut ed);
    ed.workspace.viewer.at = At::Code;
    frame(&mut ed);
    let selected = |ed: &Editor| {
        let s = ed.workspace.viewer.sidebar.as_ref().unwrap();
        s.lines()[s.selected].target.clone()
    };
    press_word(&mut ed, &Press::Reveal(root.join("src/ui")));
    assert!(ed.workspace.is_at(At::Side), "the keys there");
    assert_eq!(selected(&ed), Target::Dir("src/ui".into()));
    assert!(ed.workspace.viewer.sidebar.as_ref().unwrap().is_open("src/ui"), "unfolded");
    ed.workspace.viewer.at = At::Code;
    frame(&mut ed);
    press_word(&mut ed, &Press::Reveal(path.clone()));
    assert_eq!(selected(&ed), Target::File(path));
    ed.workspace.viewer.at = At::Code;
    frame(&mut ed);
    press_word(&mut ed, &Press::Reveal(root));
    assert_eq!(ed.workspace.viewer.sidebar.as_ref().unwrap().selected, 0, "the project: its top");
}

/// A name in the path pressed while the files are listed already (the usual case): taken at once, and still there once
/// the listing the press asked for lands — never moved to the current file's row by it (the reviewer's, mouse X).
#[test]
fn a_revealed_folder_stays_selected_when_the_listing_it_asked_lands() {
    use crate::sidebar::Target;
    let dir = crate::scratch::Scratch::new("wordspot-reveal-lands");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let rels = ["src/main.rs", "src/ui/card.rs", "src/ui/list.rs"];
    for r in rels {
        std::fs::create_dir_all(root.join(r).parent().unwrap()).unwrap();
        std::fs::write(root.join(r), "\n").unwrap();
    }
    // (one file the given tree hasn't: there once the listing landed)
    std::fs::write(root.join("README.md"), "\n").unwrap();
    let mut ed = editor();
    ed.workspace.root = root.clone();
    let given = rels.map(String::from).to_vec();
    ed.workspace.viewer.sidebar = Some(crate::sidebar::Sidebar::with_files(root.clone(), given));
    ed.docs[0].path.set(Some(root.join("src/ui/card.rs")));
    frame(&mut ed);
    press_word(&mut ed, &Press::Reveal(root.join("src")));
    let selected = |ed: &Editor| {
        let s = ed.workspace.viewer.sidebar.as_ref().unwrap();
        s.lines()[s.selected].target.clone()
    };
    assert_eq!(selected(&ed), Target::Dir("src".into()), "taken at once");
    let landed = |ed: &Editor| {
        let s = ed.workspace.viewer.sidebar.as_ref().unwrap();
        s.lines().iter().any(|l| l.target == Target::File(root.join("README.md")))
    };
    while !landed(&ed) {
        let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the listing's result");
        ed.handle_event(ev);
    }
    assert_eq!(selected(&ed), Target::Dir("src".into()), "the listing landed: still the folder pressed");
    assert!(ed.workspace.viewer.sidebar.as_ref().unwrap().reveal.is_none(), "taken, then done");
}

fn said(k: &str) -> Option<Vec<String>> {
    keys(k).map(|v| v.iter().map(|k| k.to_string()).collect())
}

/// A card asking (a weighty one: the project's trust), short enough to be drawn whole — or, `long`, cut to fit.
fn trust_card(ed: &mut Editor, long: bool) {
    let body = match long {
        true => (0..80).map(|i| format!("line {i} of what it asks")).collect::<Vec<_>>().join("\n"),
        false => "x = 1".to_string(),
    };
    let what = crate::offer::What::ProjectTrust { hash: [0; 32] };
    ed.push_offer(crate::offer::Offer::new("Trust?".into(), &body, "", String::new(), what));
    frame(ed);
}

/// The front card's words on its bottom line, as a press finds them: its way in, then each answer (row, from, to, key —
/// `mouse::WAY` the way in's).
fn answers(ed: &Editor) -> Vec<(usize, usize, usize, char)> {
    let s = &ed.screen;
    let bottom = s.offer_card.map(|c| c.1 + c.3 - 1).expect("a card drawn");
    let way = s.offer_way.map(|w| (w.0, w.1, w.2, crate::mouse::WAY));
    way.into_iter().chain(s.offer_buttons.iter().copied().filter(|b| b.0 == bottom)).collect()
}

/// Where the front card's way in is drawn, read from the frame itself (#200's review: never `Screen.offer_way`, the
/// code's own reckoning) — its words on a card's bottom line (`╰`): `C-q answer` waiting, `esc back` focused.
fn way_drawn(ed: &Editor) -> (usize, usize, usize) {
    let words = match ed.card_has_keys() {
        true => "esc back".to_string(),
        false => format!("{} answer", ed.key_said("C-q")),
    };
    let shown = crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), 120, 40).unwrap();
    let found = shown.lines().enumerate().find_map(|(y, row)| {
        let i = row.find(&words).filter(|_| row.contains('╰'))?;
        let a = row[..i].chars().count();
        Some((y, a, a + words.chars().count()))
    });
    found.unwrap_or_else(|| panic!("`{words}` on a card's bottom line:\n{shown}"))
}

/// The cells from `a` to `b` on row `y`, rendered anew: what each holds, its text's colour, its ground.
fn cells(ed: &mut Editor, (y, a, b): (usize, usize, usize)) -> Vec<(String, vt100::Color, vt100::Color)> {
    ed.focus_sync();
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, 120, 40).unwrap();
    let mut p = vt100::Parser::new(40, 120, 0);
    p.process(&bytes);
    let cell = |x: usize| {
        p.screen().cell(y as u16, x as u16).map(|c| (c.contents().to_string(), c.fgcolor(), c.bgcolor()))
    };
    (a..b).map(|x| cell(x).unwrap()).collect()
}

/// A press and its release at (`x`, `y`), then a frame — a while after the last one (never a double).
fn press_at(ed: &mut Editor, at: (usize, usize)) {
    crate::clock::advance(std::time::Duration::from_secs(1));
    mouse(ed, MouseKind::Down, at);
    mouse(ed, MouseKind::Up, at);
    frame(ed);
}

/// frictions #200: a card's answers on its bottom line light under the mouse as every word that does its key — its
/// ground only, its text's colour as drawn (meok, hanji; the card's ground is the screen's, the lit one another) — each
/// lit where a press lands on it (`mouse_under`: that answer's key). Waiting, its way in `C-q answer` and its answers
/// light (a press focuses it: the way in pressed, it has the keys); focused, `esc back` in the way in's place lights,
/// and pressed gives the keys back.
#[test]
fn a_cards_answers_light_where_a_press_lands() {
    use crate::mouse::{Part, Surface as S, Under, WAY};
    // (colours drawn though tarae's test runner sets NO_COLOR)
    crossterm::style::force_color_output(true);
    crate::clock::hold();
    for theme in ["meok", "hanji"] {
        let mut ed = editor();
        ed.theme = crate::theme::load(theme).unwrap();
        trust_card(&mut ed, false);
        for focused in [false, true] {
            assert_eq!(ed.card_has_keys(), focused, "{theme}");
            let words = answers(&ed);
            let said = |k: char| if k == WAY { "its way in".to_string() } else { format!("{k}") };
            assert!(
                words.iter().any(|w| w.3 == 'y') && words.iter().any(|w| w.3 == WAY),
                "{theme}: {words:?}"
            );
            // (its way in where the frame has its words: what the press, the hover and the light go by)
            let way = words.iter().find(|w| w.3 == WAY).map(|w| (w.0, w.1, w.2));
            assert_eq!(way, Some(way_drawn(&ed)), "{theme}, focused {focused}: its way in as drawn");
            for &(y, a, b, k) in &words {
                let said = format!("{theme}, focused {focused}: {} at {y} {a}..{b}", said(k));
                assert_eq!(ed.mouse_at(a, y), Under { surface: S::Card, part: Part::Answer(k) }, "{said}");
                let cells = |ed: &mut Editor| {
                    ed.focus_sync();
                    let mut bytes = Vec::new();
                    crate::term::render(ed, &mut bytes, 120, 40).unwrap();
                    let mut p = vt100::Parser::new(40, 120, 0);
                    p.process(&bytes);
                    let cell =
                        |x: usize| p.screen().cell(y as u16, x as u16).map(|c| (c.fgcolor(), c.bgcolor()));
                    (a..b).map(|x| cell(x).unwrap()).collect::<Vec<_>>()
                };
                let before = cells(&mut ed);
                ed.handle_event(Event::Hover((b - 1) as u16, y as u16));
                let lit = cells(&mut ed);
                assert_eq!(ed.word_hover.get(), Some((y, a, b)), "{said}: lit, as a press finds it");
                let fg = |c: &[(vt100::Color, vt100::Color)]| c.iter().map(|c| c.0).collect::<Vec<_>>();
                assert_eq!(fg(&lit), fg(&before), "{said}: keeps its text's colour");
                assert!(lit.iter().zip(&before).all(|(l, b)| l.1 != b.1), "{said}: its ground lit");
                ed.handle_event(Event::Hover(5, 3));
                assert_eq!(cells(&mut ed), before, "{said}: let go");
            }
            // (its way in pressed: waiting, the card focused; focused, `esc back` — the keys back, the card waiting)
            let way = words.iter().find(|w| w.3 == WAY).unwrap();
            press_at(&mut ed, (way.1, way.0));
            assert_eq!(ed.card_has_keys(), !focused, "{theme}: its way in pressed");
            assert_eq!(ed.offers.len(), 1, "{theme}: unanswered");
            if focused {
                press_at(&mut ed, (way.1, way.0));
            }
        }
    }
    crate::clock::release();
}

/// #200's review: the card as a one-row bar over the status line, its text read in its buffer (`v`) — its answers light
/// where a press lands on them, as on a framed card: each one's words where the frame has them, its cells' text and
/// colour as drawn, its ground another (meok, hanji). Its `y`, held until the text is read to its end, lights nothing
/// until then.
#[test]
fn the_bars_answers_light_where_a_press_lands() {
    use crate::mouse::{Part, Surface as S, Under};
    crossterm::style::force_color_output(true);
    crate::clock::hold();
    for theme in ["meok", "hanji"] {
        let mut ed = editor();
        ed.theme = crate::theme::load(theme).unwrap();
        trust_card(&mut ed, true);
        ed.offer_view();
        frame(&mut ed);
        assert!(ed.offer_bar(), "{theme}: a bar over the status line");
        for whole in [false, true] {
            if whole {
                let id = ed.offers.first().map(|o| (o.id, o.rev)).unwrap();
                ed.offer_read_whole(id);
                frame(&mut ed);
                assert!(ed.offer_bar(), "{theme}: still a bar");
            }
            let bar = ed.screen.offer_card.map(|c| c.1).expect("drawn");
            let words: Vec<_> = (ed.screen.offer_buttons.iter())
                .filter(|b| b.0 == bar && b.3.is_ascii_lowercase())
                .copied()
                .collect();
            assert!(
                words.iter().any(|w| w.3 == 'y') && words.iter().any(|w| w.3 == 'n'),
                "{theme}: {words:?}"
            );
            for &(y, a, b, k) in &words {
                let said = format!("{theme}, read whole {whole}: {k} at {y} {a}..{b}");
                let before = cells(&mut ed, (y, a, b));
                assert_eq!(before[0].0, k.to_string(), "{said}: its key where the frame has it");
                assert_eq!(ed.mouse_at(a, y), Under { surface: S::Card, part: Part::Answer(k) }, "{said}");
                ed.handle_event(Event::Hover((b - 1) as u16, y as u16));
                let lit = cells(&mut ed, (y, a, b));
                if k == 'y' && !whole {
                    assert_eq!(ed.word_hover.get(), None, "{said}: held, it lights nothing");
                    assert_eq!(lit, before, "{said}: held, drawn as it was");
                } else {
                    assert_eq!(ed.word_hover.get(), Some((y, a, b)), "{said}: lit, as a press finds it");
                    let text = |c: &[(String, vt100::Color, vt100::Color)]| {
                        c.iter().map(|c| (c.0.clone(), c.1)).collect::<Vec<_>>()
                    };
                    assert_eq!(text(&lit), text(&before), "{said}: its text and its colour as drawn");
                    assert!(lit.iter().zip(&before).all(|(l, b)| l.2 != b.2), "{said}: its ground lit");
                }
                ed.handle_event(Event::Hover(5, 3));
                assert_eq!(cells(&mut ed, (y, a, b)), before, "{said}: let go");
            }
        }
    }
    crate::clock::release();
}

/// #200's review: `esc back` pressed is `esc` typed — the card's own key, all it does: after a window prefix left the
/// card (`C-w`, then `esc`: the card remembered as the pane it left) and a press focused it again, the keys back by
/// either way leave the editor the same — a press that only took the focus away kept that memory, which `esc` forgets.
#[test]
fn esc_back_pressed_is_esc_typed() {
    let key = |ed: &mut Editor, k: &str| {
        ed.handle_key(k.parse().unwrap());
        frame(ed);
    };
    crate::clock::hold();
    let ends = [true, false].map(|pressed| {
        let mut ed = editor();
        trust_card(&mut ed, false);
        key(&mut ed, "C-q");
        key(&mut ed, "C-w");
        key(&mut ed, "esc");
        assert!(!ed.card_has_keys(), "pressed {pressed}: the prefix took the keys back");
        assert!(ed.float_left.is_some(), "pressed {pressed}: the card the pane the prefix left");
        let (y, a, _) = way_drawn(&ed);
        press_at(&mut ed, (a, y));
        assert!(ed.card_has_keys(), "pressed {pressed}: its way in pressed, focused");
        match pressed {
            true => {
                let (y, a, _) = way_drawn(&ed);
                press_at(&mut ed, (a, y));
            }
            false => key(&mut ed, "esc"),
        }
        assert!(!ed.card_has_keys() && ed.offers.len() == 1, "pressed {pressed}: the keys back, unanswered");
        let at = format!("{:?}", ed.workspace.viewer.at);
        (ed.card_focus, ed.card_next, ed.pending.len(), ed.float_left.is_some(), at)
    });
    assert_eq!(ends[0], ends[1], "`esc back` pressed leaves the editor as `esc` typed");
    assert!(!ends[1].3, "typed: the pane the prefix left forgotten");
    crate::clock::release();
}

/// #200: an answer held until its card's text is read whole — a weighty card cut to fit, its `y` saying `once it's
/// read` — lights nothing under the mouse, waiting or focused: a press there answers nothing (it says so). Its `n` lights;
/// once the text was read whole, its `y` does.
#[test]
fn a_held_answer_lights_nothing() {
    use crate::mouse::{Part, Surface as S};
    crate::clock::hold();
    let mut ed = editor();
    trust_card(&mut ed, true);
    assert!(!ed.offer_seen_whole(), "cut to fit");
    let at = |ed: &Editor, k: char| answers(ed).into_iter().find(|w| w.3 == k).map(|w| (w.1, w.0));
    let lit = |ed: &mut Editor, (x, y): (usize, usize)| {
        ed.handle_event(Event::Hover(x as u16, y as u16));
        frame(ed);
        ed.word_hover.get().is_some()
    };
    for focused in [false, true] {
        let y = at(&ed, 'y').expect("its y drawn");
        assert_eq!(ed.mouse_at(y.0, y.1).part, Part::Answer('y'), "a press lands on it");
        assert!(!lit(&mut ed, y), "focused {focused}: its held y lights nothing");
        let n = at(&ed, 'n').expect("its n drawn");
        assert!(lit(&mut ed, n), "focused {focused}: its n lights");
        if !focused {
            press_at(&mut ed, n);
            assert!(ed.card_has_keys(), "its n pressed, waiting: focused");
        }
    }
    // (pressed, it says why and answers nothing)
    let y = at(&ed, 'y').unwrap();
    press_at(&mut ed, y);
    assert_eq!(ed.offers.len(), 1, "unanswered");
    assert!(ed.messages.iter().any(|m| m.2.contains("y waits until")), "said");
    // Read whole: its y lights
    let id = ed.offers.first().map(|o| (o.id, o.rev)).unwrap();
    ed.offer_read_whole(id);
    frame(&mut ed);
    let y = at(&ed, 'y').unwrap();
    assert!(lit(&mut ed, y), "read whole: its y lights");
    assert_eq!(ed.mouse_at(y.0, y.1).surface, S::Card);
    crate::clock::release();
}

/// frictions #211: a press on a waiting card's answer focuses it, and every answer stays in its cell — focused and then
/// waiting again (`esc`) — so a second press where you aimed lands on the same answer, and answers it. Claude's grant
/// card (five answers), the code in insert mode, inside zellij's hints (its way in `esc space I answer`; once the card
/// held the keys `C-q`'s twin was `esc esc space I`, the line 4 cells wider and its answers moved) and outside them.
/// Each answer pressed first in turn; `n`, which runs nothing, pressed again: answered.
#[test]
fn a_focusing_press_moves_no_answer() {
    use crate::mouse::{Part, Surface as S, Under};
    crate::clock::hold();
    let card = |zellij: bool| {
        let mut ed = editor();
        ed.zellij = zellij;
        ed.handle_key("i".parse().unwrap());
        let job =
            crate::tools::Job::Command("npm test".into(), std::path::PathBuf::from("/tmp/tarae-not-there"));
        let run = crate::blocked::BlockedRun {
            claude: Some(crate::tools::Call::test(1)),
            ..crate::blocked::BlockedRun::of(job)
        };
        let grant = crate::allow::Grant::Write(std::path::PathBuf::from("/tmp/tarae-not-there/npm-cache"));
        let reason = "npm keeps its cache there";
        ed.push_offer(crate::blocked::also_card(
            grant,
            reason,
            "npm test",
            Box::new(run),
            None,
            Vec::new(),
            false,
        ));
        frame(&mut ed);
        assert!(!ed.card_has_keys(), "zellij {zellij}: it waits");
        ed
    };
    // (its answers on its bottom line, the way in left out — `esc back` takes the place of `C-q answer` by design)
    let laid = |ed: &Editor| {
        (
            ed.screen.offer_card,
            answers(ed).into_iter().filter(|w| w.3 != crate::mouse::WAY).collect::<Vec<_>>(),
        )
    };
    for zellij in [true, false] {
        let keys: Vec<char> = laid(&card(zellij)).1.iter().map(|w| w.3).collect();
        assert_eq!(keys, ['y', 'a', 's', 'A', 'n'], "zellij {zellij}: five answers");
        let line = crate::term::card_line(&card(zellij)).unwrap();
        assert_eq!(
            line.contains("esc space I answer"),
            zellij,
            "zellij {zellij}: its way in as said there: {line}"
        );
        for k in keys {
            let mut ed = card(zellij);
            let waiting = laid(&ed);
            let w = waiting.1.iter().find(|w| w.3 == k).copied().unwrap();
            press_at(&mut ed, (w.1, w.0));
            let said = format!("zellij {zellij}, {k} pressed");
            assert!(ed.card_has_keys() && ed.offers.len() == 1, "{said}: focused, unanswered");
            assert_eq!(laid(&ed), waiting, "{said}: every answer in its cell, focused");
            assert_eq!(
                ed.mouse_at(w.1, w.0),
                Under { surface: S::Card, part: Part::Answer(k) },
                "{said}: again there"
            );
            ed.handle_key("esc".parse().unwrap());
            frame(&mut ed);
            assert!(!ed.card_has_keys(), "{said}: esc, waiting again");
            assert_eq!(laid(&ed), waiting, "{said}: every answer in its cell, waiting again");
            if k == 'n' {
                press_at(&mut ed, (w.1, w.0));
                press_at(&mut ed, (w.1, w.0));
                assert!(ed.offers.is_empty(), "{said}: pressed again where it was, answered");
            }
        }
    }
    crate::clock::release();
}

/// #200's review: a card that isn't weighty, cut to fit, answers at once as its keys do (`Offer::act` holds only a
/// weighty card's answers until its text is read — `Offer::read_first`): its `y` never says `once it's read`, its `y`
/// and `a` light waiting and focused, and `a` pressed answers it — as `a` typed does.
#[test]
fn a_light_cards_answers_are_never_held() {
    crate::clock::hold();
    let card = |ed: &mut Editor| {
        let body = (0..80).map(|i| format!("line {i} of what it asks")).collect::<Vec<_>>().join("\n");
        let what = crate::offer::What::Asks { call: crate::tools::Call::test(1) };
        ed.push_offer(crate::offer::Offer::new("Claude asks".into(), &body, "", String::new(), what));
        frame(ed);
        assert!(!ed.offers[0].weighty() && !ed.offer_seen_whole(), "light, cut to fit");
    };
    let (mut pressed, mut typed) = (editor(), editor());
    card(&mut pressed);
    card(&mut typed);
    let line = crate::term::card_line(&pressed).expect("drawn");
    assert!(!line.contains("once"), "nothing waits to be read: {line}");
    let at = |ed: &Editor, k: char| answers(ed).into_iter().find(|w| w.3 == k).map(|w| (w.1, w.0));
    let lit = |ed: &mut Editor, (x, y): (usize, usize)| {
        ed.handle_event(Event::Hover(x as u16, y as u16));
        frame(ed);
        ed.word_hover.get().is_some()
    };
    for focused in [false, true] {
        for k in ['y', 'a'] {
            assert!(!pressed.offer_press_held(k), "focused {focused}: its {k} isn't held");
            let spot = at(&pressed, k).unwrap_or_else(|| panic!("its {k} drawn: {line}"));
            assert!(lit(&mut pressed, spot), "focused {focused}: its {k} lights");
        }
        if !focused {
            let n = at(&pressed, 'n').unwrap();
            press_at(&mut pressed, n);
            assert!(pressed.card_has_keys(), "its n pressed, waiting: focused");
        }
    }
    let a = at(&pressed, 'a').unwrap();
    press_at(&mut pressed, a);
    typed.handle_key("C-q".parse().unwrap());
    typed.handle_key("a".parse().unwrap());
    assert!(typed.offers.is_empty(), "typed: answered");
    assert!(pressed.offers.is_empty(), "pressed: answered, as typed");
    crate::clock::release();
}

/// #200: a press on a card's answer does what its key does (`Offer::act`, the one table) — so every answer that lights
/// acts: a review's end card held for a high-risk change, its `h` (a press's own list of answers had none: it did
/// nothing) says what typing `h` says.
#[test]
fn a_pressed_answer_does_what_its_key_does() {
    let held = |ed: &mut Editor| {
        let what = crate::offer::What::ReviewEnd {
            session: crate::event::WorkspaceId::HOME,
            started: std::time::Instant::now(),
        };
        let mut o = crate::offer::Offer::new(
            "End the review?".into(),
            "1 high-risk change.",
            "",
            String::new(),
            what,
        );
        o.held = Some("a high-risk change no one has looked at".into());
        ed.push_offer(o);
        frame(ed);
        ed.handle_key("C-q".parse().unwrap());
        frame(ed);
        assert!(ed.card_has_keys());
    };
    crate::clock::hold();
    let (mut pressed, mut typed) = (editor(), editor());
    held(&mut pressed);
    held(&mut typed);
    let h = answers(&pressed).into_iter().find(|w| w.3 == 'h').expect("its h drawn");
    let said = pressed.said;
    press_at(&mut pressed, (h.1, h.0));
    assert!(pressed.said != said, "its h pressed: said");
    typed.handle_key("h".parse().unwrap());
    let last = |ed: &Editor| ed.messages.last().map(|m| m.2.clone());
    assert_eq!(last(&pressed), last(&typed), "as typed");
    assert_eq!(pressed.offer_said_now(), typed.offer_said_now(), "on the card, as typed");
    crate::clock::release();
}

/// A hint's keys as typed: a sequence after a prefix, a motion's pair, an arrow, a `:` command — never several keys
/// side by side that are each their own, nor a word.
#[test]
fn a_hint_names_its_keys_or_none() {
    let v = |s: &[&str]| Some(s.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(said("C-q"), v(&["C-q"]));
    assert_eq!(said("esc"), v(&["esc"]));
    assert_eq!(said("space l"), v(&["space", "l"]));
    assert_eq!(said("C-w j"), v(&["C-w", "j"]));
    assert_eq!(said("[r"), v(&["[", "r"]));
    assert_eq!(keys("↑"), Some(vec!["up".parse().unwrap()]));
    assert_eq!((said("↑"), said("enter"), said("C-w -")), (v(&["↑"]), v(&["enter"]), v(&["C-w", "-"])));
    assert_eq!(said(":procs").map(|k| k.len()), Some(7));
    for none in ["j k", "[r ]r", "type", "h j k l", "click", ""] {
        assert_eq!(said(none), None, "{none}");
    }
}
