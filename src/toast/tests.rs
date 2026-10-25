use crate::editor::Editor;
use crate::event::{Mouse, MouseKind};
use crate::key::{Code, Key};
use crate::offer::{Offer, What};
use crate::pane::GroupKind;
use crate::session::At;
use crate::uistate::Region;

fn frame(ed: &mut Editor) -> String {
    ed.focus_sync();
    crate::term::render(ed, &mut Vec::new(), 160, 40).unwrap();
    crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), 160, 40).unwrap()
}

/// Keys, a frame after each (`<C-w>`, `<esc>`, `<ret>` as written).
fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let k: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(k);
        frame(ed);
    }
}

fn editor() -> Editor {
    let mut ed = Editor::new(crate::config::Config::default());
    ed.docs[0].text = ropey::Rope::from_str("one\ntwo\nthree\n");
    ed
}

/// The card's rows as drawn, top to bottom, with the row above and the row below it.
fn around(s: &str, ed: &Editor) -> Vec<String> {
    let (x, y, w, h) = ed.screen.toast_card.get().expect("the card drawn");
    s.lines().skip(y - 1).take(h + 2).map(|l| l.chars().skip(x).take(w).collect()).collect()
}

fn press(ed: &mut Editor, x: usize, y: usize) {
    ed.handle_mouse(Mouse { kind: MouseKind::Down, x: x as u16, y: y as u16, alt: false, ctrl: false });
    frame(ed);
}

/// A toast is news, never a pane with the keys (the user, 2026-10-25): no frame — its rect is its rows, a message a
/// row, its kind's `▎` —, no name, no keys of its own; `y` is the code's yank; the keys stay the code's, and the state
/// lists it as a toast, never a pane.
#[test]
fn a_toast_takes_no_key_and_has_no_frame() {
    let mut ed = editor();
    // (the flash drawn, so `y`'s words are a note, no toast of theirs on the card — `motion.rs`)
    ed.motion_drawn = true;
    ed.set_status("saved a.rs");
    ed.set_error("no such command: 'wat'");
    let s = frame(&mut ed);
    let (_, _, _, h) = ed.screen.toast_card.get().expect("a card");
    assert_eq!(h, 2, "its rect is its rows, a message a row");
    let rows = around(&s, &ed);
    assert!(rows[1].contains("▎ no such command: 'wat'") && rows[2].contains("▎ saved a.rs"), "{rows:#?}");
    let frame_glyphs = ['╭', '╮', '╰', '╯', '─', '│'];
    assert!(rows.iter().all(|r| !r.contains(frame_glyphs)), "no frame, focused or not: {rows:#?}");
    assert!(
        rows.iter().all(|r| !r.contains("messages") && !r.contains("esc close")),
        "no name, no keys: {rows:#?}"
    );
    assert_eq!(ed.keys_to().region, Region::Code);
    let state = ed.ui_state();
    assert!(state.sessions[0].panes.iter().all(|p| !p.id.starts_with("toast#")), "never a pane in the state");
    assert!(state.toasts.len() == 2, "listed as toasts");
    feed(&mut ed, "y");
    assert!(ed.messages.last().unwrap().2.starts_with("yanked"), "y: the code's yank");
    assert_eq!(ed.session.at, At::Code);
}

/// `C-w w`'s round never stops on a toast: the panes, then a card waiting, then round again — one pane on screen, the
/// toast up, it says so and the keys stay the code's.
#[test]
fn the_round_never_stops_on_a_toast() {
    let mut ed = editor();
    ed.set_status("saved a.rs");
    frame(&mut ed);
    feed(&mut ed, "<C-w>w");
    assert_eq!(
        (ed.session.at, ed.keys_to().region),
        (At::Code, Region::Code),
        "one pane: no stop on the toast"
    );
    assert!(ed.messages.last().unwrap().2.starts_with("one pane on screen"), "{:?}", ed.messages.last());
    let o = Offer::new(
        "End the review?".into(),
        "2 files not viewed.",
        "",
        String::new(),
        What::ReviewEnd { session: 0, started: std::time::Instant::now() },
    );
    ed.push_offer(o);
    // (the cursor off the top rows: the toast takes the top right, the card the bottom)
    ed.docs[0].text = ropey::Rope::from_str(&"line\n".repeat(30));
    feed(&mut ed, "jjjjjjjj");
    ed.set_status("saved b.rs");
    frame(&mut ed);
    assert!(ed.screen.offer_card.is_some() && ed.screen.toast_card.get().is_some(), "both in sight");
    feed(&mut ed, "<C-w>w");
    assert!(ed.card_has_keys(), "the card");
    feed(&mut ed, "<C-w>w");
    assert!(!ed.card_has_keys() && ed.session.at == At::Code, "then round to the code, past the toast");
    feed(&mut ed, "<C-w>w");
    assert!(ed.card_has_keys(), "the card again");
}

/// A press on a toast — fading too — opens `:messages` at the message pressed, with the keys; opening it lets the toasts
/// go. With a prompt open the press is the prompt's, said.
#[test]
fn a_press_on_a_toast_opens_messages() {
    let mut ed = editor();
    ed.set_status("one");
    ed.set_status("two");
    frame(&mut ed);
    let two = ed.toasts.last().unwrap().id;
    let one = two - 1;
    let at_one = ed.screen.toast_rows.borrow().iter().find(|r| r.1 == one).map(|r| r.0).unwrap();
    let (x, ..) = ed.screen.toast_card.get().unwrap();
    press(&mut ed, x + 3, at_one);
    assert!(ed.float_list_keys(GroupKind::Messages), "its home, with the keys");
    assert_eq!(ed.float_list_here(GroupKind::Messages), Some(one), "at the message pressed");
    assert!(ed.toasts.is_empty() && ed.screen.toast_card.get().is_none(), "the toasts let go");
    feed(&mut ed, "q");
    // Fading: its time all but run out, a press still opens its home
    ed.set_status("three");
    frame(&mut ed);
    let t = ed.toasts.last_mut().unwrap();
    t.at = std::time::Instant::now() - t.life + std::time::Duration::from_millis(100);
    let s = frame(&mut ed);
    assert!(ed.toast_alpha() < 1.0, "fading");
    let (x, y, ..) = ed.screen.toast_card.get().unwrap_or_else(|| panic!("drawn as it fades\n{s}"));
    press(&mut ed, x + 3, y);
    assert!(ed.float_list_keys(GroupKind::Messages), "a fading one too");
    feed(&mut ed, "q");
    // A prompt open: the press is its, said
    ed.set_status("four");
    feed(&mut ed, ":");
    let (x, y, ..) = ed.screen.toast_card.get().expect("drawn");
    press(&mut ed, x + 3, y);
    assert!(ed.prompt.is_some() && ed.messages_pane.is_none(), "the prompt stays, no list");
    assert!(ed.messages.last().unwrap().2.starts_with("the prompt has the keys"), "said");
}

/// A press on a toast while a card has the keys: `:messages` takes them, the card lets go (it waits, `C-q` focuses it
/// again) — never a list that looks lit while `card_key` still takes every key.
#[test]
fn a_press_on_a_toast_takes_the_keys_from_a_focused_card() {
    let mut ed = editor();
    let what = What::ReviewEnd { session: 0, started: std::time::Instant::now() };
    ed.push_offer(Offer::new("End the review?".into(), "2 files not viewed.", "", String::new(), what));
    // (the cursor off the top rows: the toast takes the top right, the card the bottom)
    ed.docs[0].text = ropey::Rope::from_str(&"line\n".repeat(30));
    feed(&mut ed, "jjjjjjjj");
    ed.set_status("one");
    ed.set_status("two");
    frame(&mut ed);
    feed(&mut ed, "<C-q>");
    assert!(ed.card_has_keys(), "the card focused");
    let (x, y, ..) = ed.screen.toast_card.get().expect("a toast beside it");
    press(&mut ed, x + 3, y);
    assert!(!ed.card_has_keys() && !ed.offers.is_empty(), "the card lets go, still waiting");
    assert_eq!(ed.keys_to().region, Region::List(GroupKind::Messages));
    let at = ed.float_list_here(GroupKind::Messages);
    feed(&mut ed, "k");
    assert_ne!(ed.float_list_here(GroupKind::Messages), at, "k moves the list, never the card's");
    assert!(!ed.offers.is_empty(), "the card unanswered");
}

/// An error stays up until a key of yours goes by after `ERROR_MIN` — that key goes on to where it goes (here the code's
/// `j`), never taken by the toast; a key before `ERROR_MIN` leaves it up.
#[test]
fn an_error_goes_with_your_key_and_the_key_goes_on() {
    let mut ed = editor();
    ed.set_error("it broke");
    frame(&mut ed);
    feed(&mut ed, "j");
    let t = ed.toasts.iter().find(|t| t.text == "it broke").expect("up");
    assert!(t.life > t.age() + std::time::Duration::from_secs(30), "a key before ERROR_MIN: still up");
    assert_eq!(ed.cursor_line(), 1, "the key went to the code");
    ed.toasts[0].at = std::time::Instant::now() - std::time::Duration::from_secs(5);
    feed(&mut ed, "j");
    assert_eq!(ed.cursor_line(), 2, "the key went to the code");
    let t = ed.toasts.iter().find(|t| t.text == "it broke").expect("fading");
    assert!(t.life <= t.age() + std::time::Duration::from_millis(400), "your key lets it go: it fades");
    assert_eq!(ed.keys_to().region, Region::Code);
}
