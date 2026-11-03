//! Inside zellij and out: the hints say a key zellij takes by its twin only inside it, and the twin does the
//! same — the keymap's (which-key, the palette, `:config-show`), the tutor's, the cards' and the chat's.

use super::*;
use crate::chat::{Chat, Msg, Role, State};
use crate::keymap::Keymaps;
use crate::offer::{Offer, What};

fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(key);
    }
}

fn drawn(ed: &mut Editor, w: u16, h: u16) -> String {
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, w, h).unwrap();
    let mut p = vt100::Parser::new(h, w, 0);
    p.process(&bytes);
    p.screen().rows(0, w).collect::<Vec<_>>().join("\n")
}

fn editor(zellij: bool) -> Editor {
    let mut ed = Editor::new(crate::config::Config::default());
    ed.interface.zellij = zellij;
    ed
}

fn seq(s: &str) -> Vec<Key> {
    s.split(' ').map(|k| k.parse().unwrap()).collect()
}

/// A card of Claude's (it asks to see the screen).
fn card(ed: &mut Editor) {
    let what = What::Asks { call: crate::tools::Call::test(1) };
    ed.push_offer(Offer::new("Claude asks".into(), "to see the screen", "", String::new(), what));
}

/// A chat with a conversation, its input holding the keys.
fn chatting(ed: &mut Editor) {
    let mut c = Chat::new();
    c.msgs.push(Msg::new(Role::User, "hi", ""));
    c.msgs.push(Msg::new(Role::Assistant, "hello", ""));
    ed.workspace.claude.chat = Some(c);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
}

/// Keys, a frame after each (the chat's view mode is laid out of the last frame).
fn keys(ed: &mut Editor, keys: &str) {
    for k in keys.split(' ') {
        drawn(ed, 160, 40);
        match k {
            "space" => feed(ed, "<space>"),
            "esc" | "ret" | "F2" => feed(ed, &format!("<{k}>")),
            k if k.starts_with('C') && k.len() > 1 && k.as_bytes()[1] == b'-' => feed(ed, &format!("<{k}>")),
            k => feed(ed, k),
        }
    }
    drawn(ed, 160, 40);
}

/// The chat's twins are its commands, typed in the code: from its input the way back first — and those keys run
/// the command, nothing typed to Claude (`:` in the input is a character). Nothing said yet: one `esc`.
#[test]
fn the_chats_command_twins_run_the_command() {
    for said in [false, true] {
        let mut ed = editor(true);
        match said {
            true => chatting(&mut ed),
            false => {
                let c = Chat::new();
                ed.workspace.claude.chat = Some(c);
                ed.workspace.viewer.at = crate::workspace::At::Chat;
            }
        }
        drawn(&mut ed, 160, 40);
        let twin = ed.key_said("C-s");
        assert_eq!(twin, if said { "esc esc :chat-screen" } else { "esc :chat-screen" });
        keys(&mut ed, &format!("{twin} ret"));
        let c = ed.workspace.claude.chat.as_ref().unwrap();
        assert!(
            c.screen.is_some(),
            "{said}: the screen goes with the next message: {:?}",
            ed.messages.last()
        );
        assert!(c.input.is_empty(), "nothing typed into the chat");
        assert_eq!(c.msgs.len(), if said { 2 } else { 0 }, "nothing sent");
    }
}

/// What a status line or toast says, its last one.
fn said(ed: &Editor) -> String {
    format!("{:?} {:?}", ed.status.as_ref().map(|s| &s.0), ed.messages.last())
}

/// The prompt's line for a key it doesn't take, a card up: its own `esc` is the way back — what reaches the card is
/// said from the code (inside zellij its twin, `F2`), never the prompt's `esc` again.
#[test]
fn the_prompts_line_says_the_card_from_the_code() {
    for zellij in [false, true] {
        let mut ed = editor(zellij);
        card(&mut ed);
        feed(&mut ed, ":<C-y>");
        let says = match zellij {
            true => "esc close — then F2 reaches the card",
            false => "esc close — then C-q reaches the card",
        };
        assert!(said(&ed).contains(says), "{says}: {}", said(&ed));
        feed(&mut ed, "<esc>");
        assert!(!ed.card_has_keys(), "back in the code: it waits to be focused");
        feed(&mut ed, if zellij { "<F2>" } else { "<C-q>" });
        assert!(ed.card_has_keys(), "as it says");
    }
}

/// A card holding the keys (`C-q` gave them — it swallows what isn't its answer): a twin lets go of it first, its
/// `esc` back to the chat, then the chat's way back — and the chat's row says no `answer card` while it holds them.
/// Its `C-q` again: keys back to the chat, said with its twin, `F2` — one key from the chat as from anywhere.
#[test]
fn a_card_holding_the_keys_lets_go_first() {
    let mut ed = editor(true);
    chatting(&mut ed);
    // (Claude at work: the row where the chat says `answer card`)
    ed.workspace.claude.chat.as_mut().unwrap().state = State::Thinking;
    card(&mut ed);
    assert!(drawn(&mut ed, 160, 40).contains("F2 answer card"), "the card not holding them yet");
    keys(&mut ed, "C-q");
    assert!(ed.card_has_keys(), "the card holds them");
    assert!(!drawn(&mut ed, 160, 40).contains("answer card"));
    let twin = ed.key_said("C-s");
    assert_eq!(twin, "esc esc esc :chat-screen");
    keys(&mut ed, &format!("{twin} ret"));
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert!(c.screen.is_some(), "it ran: {}", said(&ed));
    assert!(c.input.is_empty() && c.msgs.len() == 2, "nothing typed, nothing sent");
    assert_eq!(ed.offers.len(), 1, "the card waits, unanswered");
    // Given the keys again, then back: said the chat's way
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    keys(&mut ed, "C-q C-q");
    assert!(!ed.card_has_keys());
    assert!(said(&ed).contains("keys back — the card waits, F2 answers it"), "{}", said(&ed));
}

/// An edit's review in the buffer, one change of line 1 (`llm::review_with`), the keys in it.
fn reviewing(ed: &mut Editor) {
    ed.docs[0].text = ropey::Rope::from_str("a\n");
    let doc = ed.doc().id;
    crate::llm::review_with(ed, doc, "change it", vec![(0, 1, "a".into(), "b".into())], None);
    drawn(ed, 160, 40);
    assert_eq!(ed.keys_to().region, Region::EditReview);
}

/// The edit review's `esc`, back to the code: its line and its header say `C-q`'s twin (`F2`) — and it brings the
/// change back with the keys, as `C-q` does.
#[test]
fn the_edit_review_left_for_the_code_says_the_inbox_and_it_brings_it_back() {
    let mut ed = editor(true);
    reviewing(&mut ed);
    keys(&mut ed, "esc");
    assert!(ed.workspace.claude.review.as_ref().is_some_and(|r| r.away), "left, unanswered");
    assert!(
        said(&ed).contains("back to the code — the change waits unanswered, F2 answers it"),
        "{}",
        said(&ed)
    );
    assert!(drawn(&mut ed, 160, 40).contains("F2 answer it"), "the header says it too");
    keys(&mut ed, "F2");
    assert!(ed.workspace.claude.review.as_ref().is_some_and(|r| !r.away), "back in it: {}", said(&ed));
    assert_eq!(ed.keys_to().region, Region::EditReview);
}

/// The edit review taken from the chat: its `esc` goes back to the chat, said with `C-q`'s twin — one key from the
/// chat too — and it brings it back, nothing typed.
#[test]
fn the_edit_review_left_for_the_chat_says_the_chats_way_and_it_brings_it_back() {
    let mut ed = editor(true);
    chatting(&mut ed);
    ed.workspace.leave(crate::workspace::At::Chat);
    reviewing(&mut ed);
    ed.workspace.claude.review.as_mut().unwrap().from_chat = true;
    keys(&mut ed, "esc");
    assert_eq!(ed.keys_to().region, Region::Chat);
    let twin = ed.key_said("C-q");
    assert_eq!(twin, "F2");
    assert!(
        said(&ed).contains(&format!("back to the chat — the change waits unanswered, {twin} answers it"))
    );
    keys(&mut ed, &twin);
    assert_eq!(ed.keys_to().region, Region::EditReview, "back in it: {}", said(&ed));
    assert!(ed.workspace.claude.chat.as_ref().unwrap().input.is_empty(), "nothing typed into the chat");
}

/// A card stepped aside (`Offer.aside`): `C-q`'s twin reaches it and gives it the keys again.
#[test]
fn an_aside_card_is_the_inboxs() {
    let mut ed = editor(true);
    card(&mut ed);
    ed.offers[0].aside = true;
    drawn(&mut ed, 160, 40);
    assert!(!ed.card_has_keys());
    assert_eq!(ed.key_said("C-q"), "F2");
    keys(&mut ed, "F2");
    assert!(ed.card_has_keys(), "{}", said(&ed));
}

/// The chat taking the whole screen (50 columns): the card waits over its foot (P3 step 7d), its bottom line the
/// twin of `C-q` its way, the status line counting nothing it draws — and `esc` from the chat leaves it drawn, with
/// its answers.
#[test]
fn the_chat_taking_the_screen_says_its_way_back_to_the_card() {
    let mut ed = editor(true);
    let c = Chat::new();
    ed.workspace.claude.chat = Some(c);
    ed.workspace.viewer.at = crate::workspace::At::Chat;
    let what = What::ChangeSet { whose: crate::refactor::Whose::Claude(7), ops: false };
    ed.push_offer(Offer::new(
        "Claude changes 3 files".into(),
        "One review for the set.",
        "",
        String::new(),
        what,
    ));
    let s = drawn(&mut ed, 50, 30);
    let status = s.lines().last().unwrap_or_default().to_string();
    assert!(!status.contains("waiting"), "the card is drawn: {status}\n{s}");
    assert!(s.contains("F2 answer"), "its bottom line says the way in:\n{s}");
    assert_eq!(ed.key_said("C-q"), "F2");
    feed(&mut ed, "<esc>");
    drawn(&mut ed, 50, 30);
    assert!(ed.screen.offer_buttons.iter().any(|b| b.3 == 'y'), "drawn, with its answers");
}

/// Rebound, a toast's place names the keys as bound: the inbox on `A-q`, the session list on `A-w` — and `C-q`'s twin
/// stays the one key (#291), from the chat's input too.
#[test]
fn the_twins_say_the_keys_as_bound() {
    let mut ed = editor(true);
    let keys: toml::Table = toml::from_str("[normal]\nA-q = \"inbox\"\nA-w = \"session_list\"\n").unwrap();
    ed.keymaps.merge_user(&keys, &mut Vec::new());
    assert_eq!(ed.key_said("C-q"), "F2");
    chatting(&mut ed);
    assert_eq!(ed.key_said("C-q"), "F2");
}

/// The setting holds on every surface: `off` inside zellij says no twin anywhere, `on` outside says them all —
/// the palette, which-key, the tutor, `:config-show`.
#[test]
fn the_setting_holds_on_every_surface() {
    for (inside, setting, twins) in [(true, "off", false), (false, "on", true)] {
        let mut ed = editor(inside);
        crate::typed::execute(&mut ed, &format!("set editor.zellij-hints {setting}")).unwrap();
        let at = format!("{inside} {setting}");
        feed(&mut ed, "<space>?");
        let p = ed.picker.as_ref().expect("the palette");
        let up = p.items().iter().find(|i| i.action == crate::picker::Action::Command("page_up"));
        assert_eq!(up.map(|i| i.hint.as_str()), Some(if twins { "pageup" } else { "C-b" }), "{at}: palette");
        feed(&mut ed, "<esc>z");
        let s = drawn(&mut ed, 140, 40);
        let row = s.lines().find(|l| l.contains("C-b ")).unwrap_or_default();
        assert_eq!(row.contains("pageup"), twins, "{at}: which-key {row}");
        feed(&mut ed, "<esc>");
        ed.open_tutor();
        assert_eq!(ed.doc().text.to_string().contains("C-g locks zellij"), twins, "{at}: tutor");
        crate::typed::execute(&mut ed, "config-show").unwrap();
        assert_eq!(ed.doc().text.to_string().contains("# Inside zellij"), twins, "{at}: :config-show");
    }
}

/// A command bound only to keys zellij takes has no untaken key (`save_selection`: `C-s` alone); one with a key
/// zellij leaves gets that one (`page_up`: `pageup`, not `C-b`).
#[test]
fn a_twin_never_names_a_key_zellij_takes() {
    let ed = editor(true);
    let untaken = |cmd| ed.keymaps.untaken(Mode::Normal, cmd, ed.zellij_letters());
    assert_eq!(untaken("save_selection"), None);
    assert_eq!(untaken("page_up").as_deref(), Some("pageup"));
    assert_eq!(untaken("inbox").as_deref(), Some("space I"));
}

/// `editor.zellij-hints`: `auto` says the twins inside zellij only, `on` always, `off` never (zellij's unlock-first
/// preset, keys of your own) — `:set` too.
#[test]
fn the_setting_turns_the_twins_on_and_off() {
    for (inside, setting, said) in
        [(true, "auto", "F2"), (false, "auto", "C-q"), (false, "on", "F2"), (true, "off", "C-q")]
    {
        let mut ed = editor(inside);
        crate::typed::execute(&mut ed, &format!("set editor.zellij-hints {setting}")).unwrap();
        assert_eq!(ed.key_said("C-q"), said, "{inside} {setting}");
        assert_eq!(ed.key_hint("C-s", "screen").1, if said == "C-q" { "screen" } else { "" });
    }
}

/// Nothing waits: `C-q`'s twin says so as `C-q` does.
#[test]
fn nothing_waiting_the_twin_is_the_inbox_saying_so() {
    let mut ed = editor(true);
    assert_eq!(ed.next_up(), crate::inbox::Next::None);
    assert_eq!(ed.key_hint("C-q", "answer card"), ("F2".to_string(), "answer card"));
    feed(&mut ed, "<F2>");
    assert!(said(&ed).contains("nothing waits for you"), "{}", said(&ed));
}

/// The chat's view mode: the twin is one key there too — no way back to the code first.
#[test]
fn from_the_chats_view_mode_the_twin_is_one_key() {
    let mut ed = editor(true);
    chatting(&mut ed);
    keys(&mut ed, "esc");
    assert!(crate::chatdoc::has_keys(&ed), "in its view mode");
    card(&mut ed);
    assert!(!ed.card_has_keys());
    assert_eq!(ed.key_said("C-q"), "F2");
    keys(&mut ed, "F2");
    assert!(ed.card_has_keys(), "{:?}", ed.messages.last());
}

/// The terminal's program holding the keys: `F2` focuses the card in front, never the program's — on a stand-in
/// terminal (no pty, no program).
#[test]
fn from_the_terminals_program_the_twin_is_one_key() {
    let mut ed = editor(true);
    let _sent = crate::terminal::stand_in(&mut ed, 10, 80, 100);
    drawn(&mut ed, 160, 40);
    assert_eq!(ed.keys_to().region, crate::uistate::Region::Program);
    card(&mut ed);
    assert!(!ed.card_has_keys());
    assert_eq!(ed.key_said("C-q"), "F2");
    keys(&mut ed, "F2");
    assert!(ed.card_has_keys(), "{:?} {:?}", ed.keys_to().region, ed.messages.last());
    feed(&mut ed, "n");
    assert!(ed.offers.is_empty(), "its n answered it");
}

/// `editor.zellij-keys` names the letters zellij takes: zellij's defaults (session on `C-o`) or a config of one's
/// own (session on `C-y`, `C-o` left alone) — every place reads it: the chat's `C-o point`, `C-q`'s twin (`F2` while
/// zellij takes `C-q`, whatever else is bound), `:config-show`'s list, the tutor's.
#[test]
fn the_keys_zellij_takes_are_the_settings() {
    let yours = ["g", "q", "p", "n", "s", "y", "t", "h", "b"];
    for mine in [false, true] {
        let mut ed = editor(true);
        if mine {
            let setting = crate::settings::find("editor.zellij-keys").expect("its row");
            (setting.set)(&mut ed.config, &toml::Value::Array(yours.iter().map(|&k| k.into()).collect()));
            assert_eq!(ed.zellij_letters(), ['g', 'q', 'p', 'n', 's', 'y', 't', 'h', 'b']);
        }
        let at = if mine { "session on C-y" } else { "zellij's defaults" };
        // The chat's `C-o point`: zellij's own default takes it, a `C-y` config leaves it
        let c = Chat::new();
        ed.workspace.claude.chat = Some(c);
        ed.workspace.viewer.at = crate::workspace::At::Chat;
        let s = drawn(&mut ed, 160, 40);
        let point = if mine { "C-o point" } else { "esc :chat-point" };
        assert!(s.contains(point), "{at}: {point}\n{s}");
        ed.workspace.claude.chat = None;
        // `C-q`'s twin: one key, the inbox bound to `C-y` or not (both lists take `C-q`)
        let keys: toml::Table = toml::from_str("[normal]\nC-y = \"inbox\"\n").unwrap();
        ed.keymaps.merge_user(&keys, &mut Vec::new());
        assert_eq!(ed.key_said("C-q"), "F2", "{at}");
        // The lists: `:config-show`'s and the tutor's
        crate::typed::execute(&mut ed, "config-show").unwrap();
        let shown = ed.doc().text.to_string();
        let list =
            if mine { "C-g C-q C-p C-n C-s C-y C-t C-h C-b" } else { "C-g C-q C-p C-n C-s C-t C-h C-o C-b" };
        assert!(shown.contains(&format!("takes {list} first (editor.zellij-keys)")), "{at}: {shown}");
        ed.open_tutor();
        assert!(ed.doc().text.to_string().contains(list), "{at}: the tutor's list");
    }
}

/// The keymap's twin: the shortest key of the mode bound to the same, none of it zellij's — none when it has
/// no other; the palette says it inside zellij only.
#[test]
fn the_keymaps_twin_is_the_same_command_without_zellijs_keys() {
    let km = Keymaps::default();
    let d = &DEFAULT_KEYS;
    assert_eq!(km.twin(Mode::Normal, &seq("z C-b"), d).as_deref(), Some("pageup"));
    assert_eq!(km.twin(Mode::Normal, &seq("C-b"), d).as_deref(), Some("pageup"));
    assert_eq!(km.twin(Mode::Insert, &seq("C-h"), d).as_deref(), Some("backspace"));
    assert_eq!(km.twin(Mode::Normal, &seq("C-s"), d), None, "save_selection has no other key");
    assert_eq!(km.twin(Mode::Normal, &seq("C-f"), d), None, "zellij doesn't take C-f");
    assert_eq!(km.twin(Mode::Normal, &seq("C-o"), d), None, "jump_backward: no other key");
    assert_eq!(km.bindings_in(&[])["page_up"], "C-b");
    assert_eq!(km.bindings_in(d)["page_up"], "pageup");
    assert_eq!(km.bindings_in(d)["save_selection"], "C-s", "no twin: said as it is");
    let taken = km.taken(d);
    let row = |keys: &str| taken.iter().find(|t| t.1 == keys).map(|t| t.3.clone());
    assert_eq!(row("z C-b"), Some(Some("pageup".into())));
    assert_eq!(row("C-s"), Some(None));
    assert_eq!(row("C-o"), Some(None), "zellij's default takes C-o");
    assert!(taken.iter().all(|t| t.1.split(' ').any(|k| takes(&k.parse().unwrap(), d))), "{taken:?}");
    for zellij in [false, true] {
        let mut ed = editor(zellij);
        feed(&mut ed, "<space>?");
        let p = ed.picker.as_ref().expect("the palette");
        let up = p.items().iter().find(|i| i.action == crate::picker::Action::Command("page_up"));
        assert_eq!(up.map(|i| i.hint.as_str()), Some(if zellij { "pageup" } else { "C-b" }));
    }
}

/// Which-key under `z`: inside zellij `C-b`'s row says `pageup` after what it does; outside, nothing more.
#[test]
fn which_key_says_the_twin_inside_zellij() {
    for zellij in [false, true] {
        let mut ed = editor(zellij);
        feed(&mut ed, "z");
        let s = drawn(&mut ed, 140, 40);
        let row = s.lines().find(|l| l.contains("C-b ")).unwrap_or_else(|| panic!("drawn:\n{s}"));
        assert_eq!(row.contains("pageup"), zellij, "{row}");
        assert!(!s.contains('…'), "nothing cut:\n{s}");
    }
}

/// On a narrow screen the twin goes before the description is cut under its floor — every width draws, none
/// panics (a cut `dw` under the twin's width underflowed); wide, it's there.
#[test]
fn which_key_drops_the_twin_where_it_doesnt_fit() {
    let mut seen = Vec::new();
    // (short screens too: the keys go into columns and the descriptions are cut to share the width)
    for h in [10, 12, 14, 40] {
        for w in (24..=140).step_by(4) {
            let mut ed = editor(true);
            feed(&mut ed, "z");
            let s = drawn(&mut ed, w, h);
            if let Some(row) = s.lines().find(|l| l.contains("C-b ")) {
                seen.push((w, h, row.contains("pageup")));
            }
        }
    }
    assert!(seen.iter().any(|&(.., twin)| !twin), "narrow: drawn without it {seen:?}");
    assert!(seen.contains(&(140, 40, true)), "{seen:?}");
}

/// `:config-show` lists the taken keys and their twins inside zellij only; the tutor says its twins there.
#[test]
fn config_show_and_the_tutor_say_the_twins_inside_zellij() {
    for zellij in [false, true] {
        let mut ed = editor(zellij);
        crate::typed::execute(&mut ed, "config-show").unwrap();
        let text = ed.doc().text.to_string();
        assert_eq!(text.contains("# Inside zellij"), zellij);
        assert_eq!(text.contains("# normal z C-b (page_up): pageup"), zellij, "{text}");
        ed.open_tutor();
        let text = ed.doc().text.to_string();
        assert_eq!(text.contains("C-g locks zellij"), zellij);
        assert_eq!(text.contains("C-q hands it your keys"), !zellij);
    }
}

/// A busy chat holding the keys, a card up: its hint row says `C-q answer card` — inside zellij `F2`, and either
/// gives the card the keys (its `n` answers it, nothing typed).
#[test]
fn the_chats_twin_of_c_q_gives_the_card_the_keys() {
    for zellij in [false, true] {
        let mut ed = editor(zellij);
        let mut c = Chat::new();
        c.state = State::Thinking;
        c.msgs.push(Msg::new(Role::User, "hi", ""));
        c.msgs.push(Msg::new(Role::Assistant, "hello", ""));
        ed.workspace.claude.chat = Some(c);
        ed.workspace.viewer.at = crate::workspace::At::Chat;
        card(&mut ed);
        let s = drawn(&mut ed, 160, 40);
        let says = if zellij { "F2 answer card" } else { "C-q answer card" };
        assert!(s.contains(says), "{says}:\n{s}");
        assert!(!ed.card_has_keys());
        feed(&mut ed, if zellij { "<F2>" } else { "<C-q>" });
        assert!(ed.card_has_keys(), "the card has the keys: {:?}", ed.messages.last());
        // (the card holding them, the row no longer offers to give it them)
        assert!(!drawn(&mut ed, 160, 40).contains("answer card"), "held: no `answer card`");
        feed(&mut ed, "n");
        assert!(ed.offers.is_empty(), "its n answered it: {:?}", ed.messages.last());
        assert!(ed.workspace.claude.chat.as_ref().unwrap().input.is_empty(), "nothing typed into the chat");
    }
}

/// A card that came on its own waits to be focused (P3 §3.8): its bottom line's first pair says the twin — the
/// status line counting nothing it draws (polish landing 1) — and `F2` focuses it, as `C-q` does.
#[test]
fn a_card_waiting_to_be_focused_says_the_twin() {
    for zellij in [false, true] {
        let mut ed = editor(zellij);
        feed(&mut ed, "j");
        card(&mut ed);
        assert!(ed.card_waits(), "it waits to be focused");
        let s = drawn(&mut ed, 160, 40);
        let status = s.lines().last().unwrap_or_default().to_string();
        let says = if zellij { "╰─ F2 answer · " } else { "╰─ C-q answer · " };
        assert!(s.contains(says), "{says}:\n{s}");
        assert!(!status.contains("waiting"), "drawn, not counted: {status}");
        feed(&mut ed, if zellij { "<F2>" } else { "<C-q>" });
        assert!(ed.card_has_keys(), "{zellij}: {}", said(&ed));
    }
}

/// The chat's message waits for the card about a file named like a key it points at (`reads::ask_own`): `enter`
/// says so with `C-q` — inside zellij its twin, one key from the chat too, which gives the card the keys.
#[test]
fn the_chats_message_waiting_on_its_card_says_the_way_to_it() {
    for zellij in [false, true] {
        let mut ed = editor(zellij);
        let mut c = Chat::new();
        c.input = "look at this".into();
        ed.workspace.claude.chat = Some(c);
        ed.workspace.viewer.at = crate::workspace::At::Chat;
        let key_file = ed.workspace.root.join(".env");
        crate::reads::ask_own(&mut ed, vec![key_file], crate::reads::Own::Chip("pane#1".into()));
        assert!(crate::reads::pointing_asks(&ed), "its card");
        drawn(&mut ed, 160, 40);
        feed(&mut ed, "<ret>");
        let key = if zellij { "F2" } else { "C-q" };
        let says = format!("not sent yet — answer the card about what you pointed at first ({key})");
        assert!(said(&ed).contains(&says), "{says}: {}", said(&ed));
        feed(&mut ed, if zellij { "<F2>" } else { "<C-q>" });
        assert!(ed.card_has_keys(), "{zellij}: {}", said(&ed));
    }
}

/// A card stepped aside: its own line says `aside while you look — C-q answers it`, inside zellij its twin.
#[test]
fn an_aside_cards_line_says_the_twin() {
    for zellij in [false, true] {
        let mut ed = editor(zellij);
        card(&mut ed);
        ed.offers[0].aside = true;
        let s = drawn(&mut ed, 160, 40);
        let says = if zellij {
            "aside while you look — F2 answers it"
        } else {
            "aside while you look — C-q answers it"
        };
        assert!(s.contains(says), "{says}:\n{s}");
    }
}

/// In insert mode a card waits, its bottom line's first pair says `C-q answer` — inside zellij `F2 answer`, one key
/// that gives it the keys, nothing typed; from a prompt too (it closes it, as `C-q` does); a picker's `C-q` is its
/// list's, so there its `esc` first.
#[test]
fn insert_modes_twin_of_c_q_is_one_key() {
    for zellij in [false, true] {
        let mut ed = editor(zellij);
        feed(&mut ed, "i");
        card(&mut ed);
        let twin = if zellij { "F2" } else { "C-q" };
        assert_eq!(ed.key_said("C-q"), twin);
        let s = drawn(&mut ed, 160, 40);
        let says = format!("╰─ {twin} answer · ");
        assert!(s.contains(&says), "{says}:\n{s}");
        let text = ed.doc().text.to_string();
        feed(&mut ed, if zellij { "<F2>" } else { "<C-q>" });
        assert!(ed.card_has_keys());
        assert_eq!(ed.doc().text.to_string(), text, "nothing typed");
        feed(&mut ed, "n");
        assert!(ed.offers.is_empty(), "its n answered it");
        // A prompt open: the one key closes it
        feed(&mut ed, ":");
        card(&mut ed);
        assert_eq!(ed.key_said("C-q"), twin);
        feed(&mut ed, if zellij { "<F2>" } else { "<C-q>" });
        assert!(ed.prompt.is_none(), "the prompt closed, as C-q closes it");
        assert!(ed.card_has_keys(), "{zellij}: {:?} {:?}", ed.offers.len(), ed.messages.last());
        feed(&mut ed, "n");
        // A picker open: its `esc` first
        feed(&mut ed, "<esc><space>b");
        assert!(ed.picker.is_some(), "the buffer picker");
        card(&mut ed);
        assert_eq!(ed.key_said("C-q"), if zellij { "esc F2" } else { "C-q" });
        if zellij {
            feed(&mut ed, "<esc><F2>");
            assert!(ed.picker.is_none() && ed.card_has_keys(), "{:?}", ed.messages.last());
        }
    }
}

/// The empty chat's first keys: `C-t` another model, `C-p` an earlier conversation — inside zellij their
/// commands, which do the same.
#[test]
fn the_empty_chat_says_its_commands_inside_zellij() {
    for zellij in [false, true] {
        let mut ed = editor(zellij);
        let c = Chat::new();
        ed.workspace.claude.chat = Some(c);
        ed.workspace.viewer.at = crate::workspace::At::Chat;
        let s = drawn(&mut ed, 160, 40);
        // (a `:` command says what it does: alone, never `:model for another model`; from the chat's input the
        // way back to the code first — nothing said yet, one `esc`)
        let model = if zellij { "esc :model" } else { "C-t for another model" };
        assert!(s.contains(model), "{model}:\n{s}");
        let screen = if zellij { "esc :chat-screen  esc :chat-point" } else { "C-s screen  C-o point" };
        assert!(s.contains(screen), "{screen}:\n{s}");
        assert_eq!(s.contains("for another model") || s.contains(":chat-screen screen"), !zellij, "{s}");
    }
    let pair = |zellij: bool, k, w| editor(zellij).key_hint(k, w);
    assert_eq!(pair(true, "C-s", "screen"), (":chat-screen".to_string(), ""));
    assert_eq!(pair(true, "C-b", "page up"), ("C-b".to_string(), "page up"), "not a `:` twin: kept whole");
    assert_eq!(pair(false, "C-s", "screen"), ("C-s".to_string(), "screen"));
    for (key, twin) in [("C-t", ":model"), ("C-p", ":chat-resume"), ("C-s", ":chat-screen")] {
        assert_eq!(editor(true).key_said(key), twin);
        assert!(crate::cmdline::COMMANDS.iter().any(|c| c.names[0] == &twin[1..]), "{twin} is a command");
    }
}
