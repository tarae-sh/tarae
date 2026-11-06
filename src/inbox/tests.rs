//! The inbox's order, `C-q` across sessions (#94's case too), the status line's count, `enter`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::NOTHING;
use crate::editor::Editor;
use crate::event::WorkspaceId;
use crate::key::{Code, Key};
use crate::offer::{Offer, What};

/// An editor in `front`, with sessions for `back` and `third` behind it — folders in the test's own temp
/// folder.
fn three(name: &str) -> (Editor, PathBuf, WorkspaceId, WorkspaceId) {
    let dir = crate::scratch::Scratch::new(&format!("inbox-{name}")).into_path();
    for p in ["front", "back", "third"] {
        std::fs::create_dir_all(dir.join(p)).unwrap();
    }
    let dir = std::fs::canonicalize(dir).unwrap();
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.join("front"), "front".into());
    let back = ed.workspace_open(&dir.join("back")).unwrap();
    let third = ed.workspace_open(&dir.join("third")).unwrap();
    assert!(ed.workspace_switch(WorkspaceId::HOME).is_in());
    (ed, dir, back, third)
}

/// A card of Claude's (it asks to see the screen) that came `ago` seconds ago.
fn claudes(n: u64, ago: u64) -> Offer {
    let what = What::Asks { call: crate::tools::Call::test(n) };
    let mut o = Offer::new(format!("Claude asks {n}"), "to see the screen", "", String::new(), what);
    o.since = Instant::now() - Duration::from_secs(ago);
    o
}

/// A card of tarae's own (the project's settings ask for trust) that came `ago` seconds ago.
fn tarae(ago: u64) -> Offer {
    let what = What::ProjectTrust { hash: [7; 32] };
    let mut o =
        Offer::new("Trust this project's settings?".into(), "its .tarae.toml runs", "", String::new(), what);
    o.since = Instant::now() - Duration::from_secs(ago);
    o
}

fn session(ed: &mut Editor, id: WorkspaceId) -> &mut crate::workspace::Workspace {
    std::iter::once(&mut ed.workspace).chain(&mut ed.workspaces).find(|l| l.id == id).expect("the session")
}

/// Card `o` waiting in session `id` (behind), with an id of its own as `push_offer` gives one.
fn park(ed: &mut Editor, id: WorkspaceId, mut o: Offer) {
    ed.offer_seq += 1;
    o.id = ed.offer_seq;
    session(ed, id).parked.push(o);
}

/// Card `o` up in front, keeping when it came (`push_offer` says now).
fn up(ed: &mut Editor, o: Offer) {
    let since = o.since;
    ed.push_offer(o);
    ed.offers.last_mut().unwrap().since = since;
}

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

fn screen(ed: &mut Editor, w: u16, h: u16) -> Vec<String> {
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, w, h).unwrap();
    let mut vt = vt100::Parser::new(h, w, 0);
    vt.process(&bytes);
    vt.screen().rows(0, w).collect()
}

/// A read of Claude's waiting for its card since `since` (on the real clock, as the inbox reads it).
fn read_waiting(since: Instant) -> crate::reads::Pending {
    let look = crate::reads::Look {
        tool: "Read".into(),
        at: Some("/elsewhere/a.rs".into()),
        pattern: String::new(),
        filter: String::new(),
        given: "/elsewhere/a.rs".into(),
        file: true,
        keys: Vec::new(),
        cut: false,
        part: String::new(),
    };
    crate::reads::Pending {
        request_id: "r1".into(),
        generation: 1,
        input: serde_json::Value::Null,
        look,
        class: crate::cards::Class::Elsewhere,
        since,
        found: Default::default(),
        repository: None,
        not_whole: None,
        own: None,
        then: None,
        log: false,
    }
}

/// Every session's cards — up in front, stacked, parked behind, Claude's and tarae's — and Claude's reads waiting
/// for their card, in one list, oldest first, whatever session each is in. A card downloading waits for nobody:
/// not in it. The UI state lists the same, in the same order, and every card it lists that waits is in it.
#[test]
fn the_inbox_is_every_sessions_waiting_things_oldest_first() {
    let (mut ed, dir, back, third) = three("order");
    // Up in front: two cards (the stacked one older); parked behind: one in each
    up(&mut ed, claudes(1, 30));
    up(&mut ed, claudes(2, 300));
    park(&mut ed, back, claudes(3, 120));
    park(&mut ed, third, tarae(600));
    // A read of Claude's waiting for its card, behind
    let mut chat = crate::chat::Chat::new();
    chat.reads.push_back(read_waiting(Instant::now() - Duration::from_secs(60)));
    session(&mut ed, back).claude.chat = Some(chat);
    // One downloading: waits for nobody
    let mut busy = tarae(900);
    busy.state = crate::offer::OfferState::Installing(Instant::now());
    park(&mut ed, back, busy);
    let ids: Vec<String> = ed.inbox().into_iter().map(|e| e.id).collect();
    let card = |o: &Offer| format!("card#{}", o.id);
    let (front_new, front_old) = (card(&ed.offers[0]), card(&ed.offers[1]));
    let parked = |ed: &mut Editor, id| card(&session(ed, id).parked[0]);
    let (b, t) = (parked(&mut ed, back), parked(&mut ed, third));
    let read = format!("read#s{back}:r1");
    assert_eq!(ids, [t, front_old, b, read.clone(), front_new], "oldest first, every session's");
    let e = ed.inbox().into_iter().find(|e| e.id == read).unwrap();
    assert_eq!((e.kind, e.asks.as_str(), e.up), ("read", "Claude's Read of /elsewhere/a.rs", "queued"));
    // The state: the same list in the same order — and it's built on the state's cards
    let state = ed.ui_state();
    assert_eq!(state.inbox.iter().map(|e| &e.id).collect::<Vec<_>>(), ids.iter().collect::<Vec<_>>());
    for c in state.cards.iter().filter(|c| c.state != "installing") {
        assert!(ids.contains(&c.id), "{} waits, so it's in the inbox", c.id);
    }
    assert!(!state.cards.iter().filter(|c| c.state == "installing").any(|c| ids.contains(&c.id)));
    let json = state.json();
    assert_eq!(json["inbox"][0]["id"], ids[0].as_str(), "{json}");
    assert_eq!(json["inbox"][3]["kind"], "read");
    assert!(
        state.text().contains(&format!("inbox, oldest first: {} (s{third})", ids[0])),
        "{}",
        state.text()
    );
    // The card's count says it, here and elsewhere
    assert_eq!(ed.cards_waiting(), (2, 3));
    std::fs::remove_dir_all(&dir).ok();
}

/// frictions15 #5, polish landing 1: what waits behind the card in front is never a silent queue, and its home is the
/// stack — a peek each under the card (this session's, then another session's, named) — so the card says no count and
/// its bottom line no key that lists them; the status line counts only what nothing draws: all in sight, none; a
/// fifth past the stack's room, `▲1 waiting` — and a click on that count lists them.
#[test]
fn the_stack_says_what_waits_and_the_status_line_counts_the_rest() {
    let (mut ed, dir, back, _) = three("behind");
    up(&mut ed, claudes(1, 60));
    up(&mut ed, claudes(2, 30));
    park(&mut ed, back, claudes(3, 10));
    let rows = screen(&mut ed, 120, 30);
    let all = rows.join("\n");
    assert!(!all.contains("more wait") && !all.contains("elsewhere"), "no count on the card: {all}");
    assert_eq!(ed.screen.asks_drawn.len(), 3, "the card and its two peeks: {all}");
    assert!(all.contains("back asks · Claude asks 3"), "another session's peek, named: {all}");
    assert!(!rows[29].contains("waiting"), "all in sight, none counted: {}", rows[29]);
    let line = "╰─ C-q answer · y Once · a This chat · n No ─╯";
    assert_eq!(crate::term::card_line(&ed).as_deref(), Some(line), "no key that lists them: {all}");
    // A fifth: past the stack's room (the card and three peeks), counted — the way in staying the card's
    up(&mut ed, claudes(4, 5));
    up(&mut ed, claudes(5, 4));
    let rows = screen(&mut ed, 120, 30);
    assert_eq!(ed.screen.asks_drawn.len(), 4, "the stack's room");
    assert!(rows[29].contains("▲1 waiting") && !rows[29].contains("C-q"), "{}", rows[29]);
    // …and a click on the count lists every one
    let x = rows[29].find("▲1 waiting").map(|b| rows[29][..b].chars().count()).expect("the count");
    let click = crate::event::Mouse {
        kind: crate::event::MouseKind::Down,
        x: x as u16,
        y: 29,
        alt: false,
        ctrl: false,
    };
    ed.handle_mouse(click);
    let p = ed.picker.as_ref().expect("the inbox");
    assert_eq!((p.title.as_str(), p.items().len()), (super::INBOX, 5));
    std::fs::remove_dir_all(&dir).ok();
}

/// A card of Claude's with a long title (`to run what it wrote — outside the sandbox`), `ago` seconds ago.
fn long(n: u64, ago: u64) -> Offer {
    let mut o = claudes(n, ago);
    o.title = format!("Claude wants to run what it wrote {n} — outside the sandbox");
    o
}

/// The polish landing's peeks (the user, 2026-11-03, on f237-five at 200: `docs asks · … outside the …` cut with half
/// the screen free): a peek takes its title's width, up to the width its own card takes whole — never held to the
/// stair behind a narrower card while the screen has room. Alone (other sessions' only) and behind a short card alike
/// they read whole, their right ends on the stair; on a narrow screen a title wider than its own card is cut there.
#[test]
fn a_peek_is_as_wide_as_its_title_up_to_its_own_cards_width() {
    let (mut ed, dir, back, third) = three("peek-width");
    park(&mut ed, back, long(2, 60));
    park(&mut ed, third, long(3, 30));
    let peek =
        |rows: &[String], who: &str| rows.iter().find(|r| r.contains(who)).cloned().unwrap_or_default();
    let rows = screen(&mut ed, 200, 40);
    for (who, n) in [("back asks", 2), ("third asks", 3)] {
        let whole = format!("{who} · Claude wants to run what it wrote {n} — outside the sandbox");
        assert!(peek(&rows, who).contains(&whole), "alone, whole: {}", rows.join("\n"));
    }
    // Behind a card of this session's narrower than their titles: wider than the stair, the right ends on it
    up(&mut ed, claudes(1, 90));
    let rows = screen(&mut ed, 200, 40);
    let (b, t) = (peek(&rows, "back asks"), peek(&rows, "third asks"));
    assert!(b.contains("what it wrote 2 — outside the sandbox"), "behind, whole: {}", rows.join("\n"));
    assert!(t.contains("what it wrote 3 — outside the sandbox"), "behind, whole: {}", rows.join("\n"));
    let card = rows.iter().find(|r| r.contains("Claude asks 1")).expect("the card's frame");
    let card_left = card.chars().position(|c| c == '╭').expect("its corner");
    let back_left = b.chars().position(|c| c == '▎').expect("its bar");
    assert!(back_left < card_left, "wider than the card in front: {}", rows.join("\n"));
    // (its right end as the mouse finds it, `PEEK_INSET` in from the card's)
    let y = rows.iter().position(|r| r.contains("back asks")).unwrap();
    let to = ed.screen.elements.borrow().iter().find(|e| e.y == y).map(|e| e.to).expect("the peek, a button");
    let card_right = card.chars().position(|c| c == '╮').expect("its corner") + 1;
    assert_eq!(card_right - to, 2, "its right end on the stair: {}", rows.join("\n"));
    // Narrow: a title wider than its own card is cut at that card's width, never past the screen's left
    ed.offers.clear();
    session(&mut ed, back).parked[0].title = format!("Claude wants to run {}", "a long line ".repeat(10));
    let rows = screen(&mut ed, 80, 24);
    let b = peek(&rows, "back asks");
    assert!(b.trim_end().ends_with('…'), "cut: {b:?}");
    let y = rows.iter().position(|r| r.contains("back asks")).unwrap();
    let from = ed.screen.elements.borrow().iter().find(|e| e.y == y).map(|e| e.from).expect("the peek");
    assert!(from >= 1, "a cell of air at the screen's left: {b:?}");
    std::fs::remove_dir_all(&dir).ok();
}

/// `C-q` with nothing waiting in front goes to the one that has waited longest, in any session, and its card
/// takes the keys there (frictions29 19: it took a second `C-q`, and from a chat the key meant for the card was
/// typed into it). #94's case: a card of tarae's parked in a session behind whose turn
/// ended (`done`, its Claude idle) — `C-q` said "no card waits for you" while it waited there; now it goes to
/// it. Nothing anywhere: said.
#[test]
fn c_q_goes_to_the_oldest_in_any_session() {
    let (mut ed, dir, back, third) = three("c-q");
    park(&mut ed, back, claudes(2, 60));
    // (#94: tarae's own card, parked in a session whose turn ended behind)
    park(&mut ed, third, tarae(30));
    session(&mut ed, third).claude.turn_end = Some((Instant::now(), Ok(())));
    // The status line counts none: both in sight as peeks, its way in said once
    let rows = screen(&mut ed, 160, 30);
    assert!(rows[29].contains("C-q answer") && !rows[29].contains("waiting"), "{}", rows[29]);
    // C-q: to back's (the oldest), its card up
    feed(&mut ed, "<C-q>");
    // (front's comes meanwhile, the newest: it waits behind)
    park(&mut ed, WorkspaceId::HOME, claudes(1, 5));
    assert_eq!(ed.workspace.id, back, "the one that waited longest");
    assert_eq!(ed.offers.first().map(|o| o.title.as_str()), Some("Claude asks 2"));
    assert!(ed.card_has_keys(), "the keys with it");
    assert_eq!(
        ed.keys_to().route(Key::plain(Code::Char('n'))),
        crate::uistate::Region::Card(ed.offers[0].id)
    );
    // `n` answers it there
    feed(&mut ed, "n");
    assert!(ed.offers.is_empty() && !ed.card_has_keys());
    // C-q: third's — tarae's own card, its turn over (#94) — before front's newer one
    assert!(ed.status.as_ref().is_none_or(|s| s.0 != NOTHING));
    feed(&mut ed, "<C-q>");
    assert_eq!(ed.workspace.id, third, "#94: found");
    assert_eq!(ed.offers.first().map(|o| o.title.as_str()), Some("Trust this project's settings?"));
    ed.offers.clear();
    // …then front's
    feed(&mut ed, "<C-q>");
    assert_eq!(
        (ed.workspace.id, ed.offers.first().map(|o| o.title.as_str())),
        (crate::event::WorkspaceId::HOME, Some("Claude asks 1"))
    );
    assert!(ed.card_has_keys());
    // Nothing anywhere: said
    ed.offers.clear();
    feed(&mut ed, "<C-q>");
    assert!(ed.status.as_ref().is_some_and(|s| s.0 == NOTHING), "{:?}", ed.status);
    std::fs::remove_dir_all(&dir).ok();
}

/// #198: `C-q` in a session with a card of its own takes that one first — an older one in another session
/// waits (answering here meant `n` to the other first). Then the rest by age. The list keeps its order: the
/// oldest first, whatever session is in front.
#[test]
fn c_q_takes_this_sessions_own_card_first() {
    let (mut ed, dir, back, _) = three("own");
    park(&mut ed, back, claudes(1, 600));
    up(&mut ed, claudes(2, 5));
    assert_eq!(ed.inbox()[0].session_id(), back, "the list: oldest first");
    feed(&mut ed, "<C-q>");
    assert_eq!(ed.workspace.id, WorkspaceId::HOME, "stays here");
    assert!(ed.card_has_keys(), "this session's card has the keys");
    assert_eq!(ed.offers.first().map(|o| o.title.as_str()), Some("Claude asks 2"));
    feed(&mut ed, "n");
    assert!(ed.offers.is_empty() && !ed.card_has_keys());
    // Answered: then the rest, by age
    feed(&mut ed, "<C-q>");
    assert_eq!((ed.workspace.id, ed.offers.first().map(|o| o.title.as_str())), (back, Some("Claude asks 1")));
    std::fs::remove_dir_all(&dir).ok();
}

/// The order is when each came, however it's read: two cards come within a millisecond, their waits read a
/// moment apart in whole milliseconds (`keys_to`, then `handle_key` — each reads every card's at its own
/// instant) — the newer one's read later, rounded up past the older one's. Read so, the order swapped: the
/// routing test's "two cards stacked" now and then (`C-q`: `keys_to` said NextCard, `handle_key` took Card).
#[test]
fn the_order_is_when_each_came_however_it_is_read() {
    let (mut ed, dir, _, _) = three("instant");
    let t = Instant::now() - Duration::from_secs(5);
    let (mut older, mut newer) = (claudes(1, 0), claudes(2, 0));
    (older.since, newer.since) = (t, t + Duration::from_micros(400));
    up(&mut ed, older);
    up(&mut ed, newer);
    let cards = ed.ui_cards();
    let ids = |cards: &[crate::uistate::CardState]| -> Vec<String> {
        ed.inbox_from(cards).into_iter().map(|e| e.id).collect()
    };
    let first = ids(&cards);
    assert_eq!(first[0], format!("card#{}", ed.offers.iter().find(|o| o.since == t).unwrap().id));
    let mut later = cards.clone();
    let most = later.iter().map(|c| c.waited_ms).max().unwrap();
    later.iter_mut().find(|c| c.since > t).unwrap().waited_ms = most + 1;
    assert_eq!(ids(&later), first, "the same order, read a moment later");
    std::fs::remove_dir_all(&dir).ok();
}

/// Review (detsim P1, note 8): a card's wait is on logic's clock — a simulation's here, moved on ten minutes —
/// and an edit's on the real one, a minute: the two are ordered by how long each has waited on its own clock,
/// never as instants of two clocks. The card first.
#[test]
fn the_order_is_by_each_ones_wait_on_its_own_clock() {
    let mut sim =
        crate::sim::Sim::new("inbox_clocks", crate::sim::Choices::seeded(1), Duration::from_secs(30));
    let (mut ed, dir, back, _) = three("clocks");
    let mut card = claudes(1, 0);
    card.since = crate::clock::now();
    up(&mut ed, card);
    let mut chat = crate::chat::Chat::new();
    chat.reads.push_back(read_waiting(Instant::now() - Duration::from_secs(60)));
    session(&mut ed, back).claude.chat = Some(chat);
    sim.wait(Duration::from_secs(600));
    let ids: Vec<String> = ed.inbox().into_iter().map(|e| e.id).collect();
    assert_eq!(ids, [format!("card#{}", ed.offers[0].id), format!("read#s{back}:r1")]);
    drop(sim);
    std::fs::remove_dir_all(&dir).ok();
}

/// Inside zellij `C-q` is zellij's (uxaudit 7): its twin, `F2` (#291), reaches a card waiting in a session behind as
/// `C-q` does, and the card takes the keys.
#[test]
fn inside_zellij_f2_reaches_the_card_as_c_q_does() {
    let (mut ed, dir, back, _) = three("zellij");
    ed.interface.zellij = true;
    park(&mut ed, back, claudes(1, 20));
    assert_eq!(ed.key_said("C-q"), "F2");
    feed(&mut ed, "<F2>");
    assert_eq!(ed.workspace.id, back, "went to the card's session");
    assert_eq!(ed.offers.first().map(|o| o.title.as_str()), Some("Claude asks 1"));
    feed(&mut ed, "n");
    assert!(ed.offers.is_empty(), "its n answered it: {:?}", ed.messages.last());
    std::fs::remove_dir_all(&dir).ok();
}

/// Inside zellij, `C-q`'s twin goes where it goes: a card behind, from the chat too (one key, nothing typed), and a
/// turn that ended, with nothing waiting, its session.
#[test]
fn inside_zellij_c_qs_twin_reaches_what_c_q_goes_to() {
    let (mut ed, dir, back, third) = three("zellij-twin");
    ed.interface.zellij = true;
    let mut c = crate::chat::Chat::new();
    c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, "hi", ""));
    c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Assistant, "hello", ""));
    ed.workspace.claude.chat = Some(c);
    ed.workspace.set_at(crate::workspace::Here::Chat);
    park(&mut ed, back, claudes(1, 20));
    assert_eq!(ed.key_said("C-q"), "F2");
    screen(&mut ed, 160, 30);
    feed(&mut ed, "<F2>");
    assert_eq!(ed.workspace.id, back, "went to the card's session");
    feed(&mut ed, "n");
    assert!(ed.offers.is_empty(), "its n answered it: {:?}", ed.messages.last());
    // Nothing waits, third's turn ended: there
    session(&mut ed, third).claude.turn_end = Some((Instant::now() - Duration::from_secs(30), Ok(())));
    assert_eq!(ed.next_up(), super::Next::Done(third));
    assert_eq!(ed.key_said("C-q"), "F2");
    feed(&mut ed, "<F2>");
    assert_eq!(ed.workspace.id, third, "went to the session whose turn ended");
    std::fs::remove_dir_all(&dir).ok();
}

/// Leaving a session whose card waits, its toast says how to get back to it: `C-q goes there` — inside zellij
/// `F2 goes there`, one key from wherever the toast is read.
#[test]
fn inside_zellij_or_not_the_toast_for_a_session_left_waiting_says_its_key() {
    // (inside zellij by its setting, auto and off — and outside, on)
    let cases = [(false, "auto", false), (true, "auto", true), (true, "off", false), (false, "on", true)];
    for (zellij, setting, twins) in cases {
        let (mut ed, dir, back, _) = three(&format!("zellij-left-{zellij}-{setting}"));
        ed.interface.zellij = zellij;
        ed.config.zellij_hints = setting.into();
        // (the session switched to has its chat holding the keys: the toast is read later, from anywhere)
        let mut c = crate::chat::Chat::new();
        c.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, "hi", ""));
        session(&mut ed, back).claude.chat = Some(c);
        session(&mut ed, back).set_at(crate::workspace::Here::Chat);
        up(&mut ed, claudes(1, 20));
        assert!(ed.workspace_switch(back).is_in());
        let says = if twins {
            "front · waits for you — F2 goes there"
        } else {
            "front · waits for you — C-q goes there"
        };
        // (kept at once; its toast waits on the frame, which may draw its card's peek — #405)
        let all: Vec<&str> = ed.messages.iter().map(|m| m.2.as_str()).collect();
        assert!(all.iter().any(|t| t.ends_with(says)), "{says}: {all:?}");
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// A panel holding the keys (the side panel): the twin is one key there too, never the panel's.
#[test]
fn inside_zellij_a_panels_twin_of_c_q_is_one_key() {
    let (mut ed, dir, back, _) = three("zellij-panel");
    ed.interface.zellij = true;
    // (`space e` shows the files panel and gives it the keys)
    feed(&mut ed, "<space>e");
    assert!(matches!(ed.keys_to().region, crate::uistate::Region::Side(_)), "{:?}", ed.keys_to().region);
    park(&mut ed, back, claudes(1, 20));
    assert_eq!(ed.key_said("C-q"), "F2");
    feed(&mut ed, "<F2>");
    assert_eq!(ed.workspace.id, back, "went to the card's session");
    std::fs::remove_dir_all(&dir).ok();
}

/// frictions29 19: `C-q` from a chat that holds the keys reaches the card — one parked in a session behind (the
/// chat open and focused), one up in front (the chat minimized while Claude works, its input not on screen) —
/// and the card it reaches takes the keys: its `n` answers it, nothing typed into the chat.
#[test]
fn c_q_from_a_chat_holding_the_keys_reaches_the_card() {
    let (mut ed, dir, back, _) = three("chat-cq");
    let chat = |busy: bool| {
        let mut c = crate::chat::Chat::new();
        if busy {
            (c.state, c.follow.on) = (crate::chat::State::Thinking, true);
        }
        c
    };
    // The chat open and focused here; the card parked behind
    ed.workspace.claude.chat = Some(chat(false));
    ed.workspace.set_at(crate::workspace::Here::Chat);
    park(&mut ed, back, claudes(1, 20));
    assert!(ed.ui_state().text().contains("keys → chat"), "{}", ed.ui_state().text());
    // (that session's own chat holding the keys too)
    session(&mut ed, back).claude.chat = Some(chat(false));
    session(&mut ed, back).set_at(crate::workspace::Here::Chat);
    feed(&mut ed, "<C-q>");
    assert_eq!(ed.workspace.id, back, "C-q went to the card's session");
    assert_eq!(ed.offers.first().map(|o| o.title.as_str()), Some("Claude asks 1"));
    // …and the card has the keys: one C-q, not one to get there and another to answer
    feed(&mut ed, "n");
    assert!(ed.offers.is_empty(), "its n answered it: {:?}", ed.messages.last());
    assert!(ed.workspace.claude.chat.as_ref().unwrap().input.is_empty(), "nothing typed into the chat there");
    // While Claude works, holding the keys: a card up here
    ed.workspace.claude.chat = Some(chat(true));
    ed.workspace.set_at(crate::workspace::Here::Chat);
    up(&mut ed, claudes(2, 5));
    feed(&mut ed, "<C-q>n");
    assert!(ed.offers.is_empty(), "its n answered it: {:?}", ed.messages.last());
    assert!(ed.workspace.claude.chat.as_ref().unwrap().input.is_empty(), "nothing typed into the chat");
    std::fs::remove_dir_all(&dir).ok();
}

/// frictions29 9: a run quiet only because a filter holds its output (`| tail -60` over a long build) is working,
/// not waiting on anyone — it stays out of the inbox while its processes use the CPU, comes in once they've been
/// idle for `SILENT` too (or half the time its own `timeout` gives it is gone), and says the filter holds it.
#[test]
fn a_quiet_run_still_working_stays_out_of_the_inbox() {
    use crate::task::{Run, State, Task};
    let (mut ed, dir, _, _) = three("filtered");
    let Some(then) = Instant::now().checked_sub(Duration::from_secs(400)) else { return };
    let run = |cmd: &str| {
        let mut r = Run::for_test(Task { name: cmd.into(), cmd: cmd.into(), cwd: dir.join("front") });
        (r.state, r.by_claude, r.last_output, r.started) = (State::Running, true, then, then);
        r
    };
    let said = |ed: &Editor| ed.inbox().into_iter().find(|e| e.kind == "silent-run").map(|e| e.asks);
    // Working: out
    let mut r = run("cargo test 2>&1 | grep -E 'result' | tail -60");
    r.wait.looked(240.0, Instant::now());
    ed.workspace.task_run = Some(r);
    assert_eq!(said(&ed), None, "a run using the CPU behind its filter");
    // Its own time limit half gone: in, working or not
    let r = ed.workspace.task_run.as_mut().unwrap();
    r.wait.limited(then, Some(Duration::from_secs(600)));
    assert!(said(&ed).is_some_and(|l| l.contains("its output goes through grep")), "{:?}", said(&ed));
    // Idle for minutes as well: in, the filter said
    let r = ed.workspace.task_run.as_mut().unwrap();
    r.wait.limited(then, None);
    r.wait.looked(0.0, then);
    let l = said(&ed).expect("idle and quiet: in the inbox");
    assert!(l.contains("no output for") && l.contains("its output goes through grep"), "{l}");
    // No filter: said as before
    let mut r = run("cargo test");
    r.wait.looked(0.0, then);
    ed.workspace.task_run = Some(r);
    assert!(said(&ed).is_some_and(|l| !l.contains("goes through")), "{:?}", said(&ed));
    // `C-q` takes the user to its panel, as for any quiet run
    feed(&mut ed, "<C-q>");
    assert_eq!(ed.slot(), Some(crate::sizes::Slot::Task));
    std::fs::remove_dir_all(&dir).ok();
}

/// #171: a run of Claude's quiet for minutes (a slow `apt-get` in docker) is in the inbox, but after a card
/// that waits for an answer — even one that came long after it went quiet, in another session. `C-q` goes to
/// the card first; the quiet run stays reachable after it.
#[test]
fn a_card_waiting_comes_before_a_run_only_quiet() {
    use crate::task::{Run, State, Task};
    let (mut ed, dir, back, _) = three("quiet");
    let cmd = "docker run --rm img sh -c 'apt-get install -y x'";
    let mut r = Run::for_test(Task { name: cmd.into(), cmd: cmd.into(), cwd: dir.join("front") });
    (r.state, r.by_claude) = (State::Running, true);
    let Some(then) = Instant::now().checked_sub(Duration::from_secs(228)) else { return };
    r.last_output = then;
    ed.workspace.task_run = Some(r);
    park(&mut ed, back, claudes(1, 20));
    let inbox = ed.inbox();
    let kinds: Vec<&str> = inbox.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [crate::offer::ASKS, "silent-run"], "{inbox:?}");
    assert!(inbox[1].waited_ms > inbox[0].waited_ms, "quiet for longer than the card has waited");
    assert_eq!(ed.ui_state().inbox.iter().map(|e| e.kind).collect::<Vec<_>>(), kinds);
    // C-q: to back's card, not the quiet run's panel here
    feed(&mut ed, "<C-q>");
    assert_eq!(ed.workspace.id, back);
    assert_eq!(ed.offers.first().map(|o| o.title.as_str()), Some("Claude asks 1"));
    assert!(ed.card_has_keys(), "focused there");
    feed(&mut ed, "n");
    assert!(ed.offers.is_empty());
    // Answered: C-q now goes to the quiet run, its panel with the keys
    feed(&mut ed, "<C-q>");
    assert_eq!(ed.workspace.id, WorkspaceId::HOME);
    assert_eq!(ed.slot(), Some(crate::sizes::Slot::Task));
    assert!(ed.bottom_has_keys());
    std::fs::remove_dir_all(&dir).ok();
}

/// Nothing waits, but turns ended in sessions behind (their tabs say `done`): `C-q` goes to them, the one
/// that ended first first, and says when — after whatever waits: a card that comes meanwhile goes first, and
/// it isn't the inbox's (its count stays what waits for an answer). None left: "no card waits".
#[test]
fn c_q_goes_to_a_finished_session_when_nothing_waits() {
    let (mut ed, dir, back, third) = three("done");
    let ago = |s: u64| Some((Instant::now() - Duration::from_secs(s), Ok(())));
    session(&mut ed, back).claude.turn_end = ago(30);
    session(&mut ed, third).claude.turn_end = ago(200);
    assert!(ed.inbox().is_empty(), "a turn's end asks nothing");
    assert_eq!(ed.next_up(), super::Next::Done(third));
    // A card comes meanwhile: it goes first
    park(&mut ed, back, claudes(1, 1));
    feed(&mut ed, "<C-q>");
    assert_eq!((ed.workspace.id, ed.offers.first().map(|o| o.title.as_str())), (back, Some("Claude asks 1")));
    ed.offers.clear();
    // (back was looked at with it: its `done` went)
    assert!(session(&mut ed, back).claude.turn_end.is_none());
    // Then third's turn end, said with how long ago
    feed(&mut ed, "<C-q>");
    assert_eq!(ed.workspace.id, third);
    assert!(ed.status.as_ref().is_some_and(|s| s.0 == "third · its turn ended 3m ago"), "{:?}", ed.status);
    // Nothing left
    feed(&mut ed, "<C-q>");
    assert_eq!(ed.workspace.id, third);
    assert!(ed.status.as_ref().is_some_and(|s| s.0 == NOTHING), "{:?}", ed.status);
    std::fs::remove_dir_all(&dir).ok();
}

/// Each row names its session as every list names one (#165): the name in its participant colour, full — the row in
/// hand too —, what it asks after it in the row's own ink.
#[test]
fn each_row_names_its_session_in_its_colour() {
    let (mut ed, dir, back, third) = three("colours");
    up(&mut ed, claudes(1, 900));
    park(&mut ed, back, claudes(2, 60));
    park(&mut ed, third, tarae(400));
    feed(&mut ed, "<space>I");
    let vt = colour_frame(&mut ed);
    let mut seen = Vec::new();
    for (name, id, asks) in [
        ("front", WorkspaceId::HOME, "Claude asks 1"),
        ("third", third, "Trust this"),
        ("back", back, "Claude asks 2"),
    ] {
        let want = colour(&mut ed, id);
        let (row, name_fg, after) = named_row(&vt, name, asks);
        assert!(name_fg.iter().all(|c| *c == want), "{name} whole in its colour: {row}");
        assert_ne!(after[0], want, "the `·` after it isn't: {row}");
        assert_ne!(after[1], want, "nor what it asks: {row}");
        seen.push(want);
    }
    assert!(seen[0] != seen[1] && seen[1] != seen[2] && seen[0] != seen[2], "each its own");
    std::fs::remove_dir_all(&dir).ok();
}

/// #165's review: the list open, a session's colour or name changed shows on its row by the next event's sync — the
/// paint follows a colour alone (the rows the same), and a new name in its colour.
#[test]
fn a_sessions_new_colour_or_name_shows_while_the_list_is_open() {
    let (mut ed, dir, back, third) = three("repaint");
    park(&mut ed, back, claudes(2, 60));
    park(&mut ed, third, tarae(400));
    feed(&mut ed, "<space>I");
    let was = colour(&mut ed, back);
    // (a colour none of the three has)
    let used: Vec<u8> =
        [WorkspaceId::HOME, back, third].into_iter().map(|id| session(&mut ed, id).claude.colour).collect();
    let free = (0..crate::session::COLOURS as u8).find(|c| !used.contains(c)).expect("a free colour");
    session(&mut ed, back).claude.colour = free;
    session(&mut ed, third).name = "docs".into();
    ed.inbox_sync();
    let vt = colour_frame(&mut ed);
    let now = colour(&mut ed, back);
    assert_ne!(was, now);
    let (row, name_fg, _) = named_row(&vt, "back", "Claude asks 2");
    assert!(name_fg.iter().all(|c| *c == now), "back in its new colour: {row}");
    let (row, name_fg, _) = named_row(&vt, "docs", "Trust this");
    let want = colour(&mut ed, third);
    assert!(name_fg.iter().all(|c| *c == want), "docs, its new name, in its colour: {row}");
    assert!(!vt.screen().contents().contains("third ·"), "the old name gone");
    std::fs::remove_dir_all(&dir).ok();
}

/// The frame with its colours, 160 × 30.
fn colour_frame(ed: &mut Editor) -> vt100::Parser {
    crossterm::style::force_color_output(true);
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, 160, 30).unwrap();
    let mut vt = vt100::Parser::new(30, 160, 0);
    vt.process(&bytes);
    vt
}

/// Session `id`'s participant colour at full strength, as the frame paints it.
fn colour(ed: &mut Editor, id: WorkspaceId) -> vt100::Color {
    let k = crate::session::PARTICIPANTS[session(ed, id).claude.colour as usize % crate::session::COLOURS];
    match ed.theme.try_get(k).and_then(|s| s.fg) {
        Some(crossterm::style::Color::Rgb { r, g, b }) => vt100::Color::Rgb(r, g, b),
        c => panic!("{k}: {c:?}"),
    }
}

/// The inbox's row `name · asks…`: its text, its name's cells' colours, and those of the `·` and what it asks.
fn named_row(vt: &vt100::Parser, name: &str, asks: &str) -> (String, Vec<vt100::Color>, [vt100::Color; 2]) {
    let rows: Vec<String> = vt.screen().rows(0, 160).collect();
    let lead = format!("{name} · {asks}");
    let y = rows.iter().position(|r| r.contains(&lead)).unwrap_or_else(|| panic!("{lead}: {rows:#?}"));
    let x = rows[y][..rows[y].find(&lead).unwrap()].chars().count();
    let at = |dx: usize| vt.screen().cell(y as u16, (x + dx) as u16).unwrap().fgcolor();
    (rows[y].clone(), (0..name.len()).map(at).collect(), [at(name.len() + 1), at(name.len() + 3)])
}

/// Its ages are a column of times: a long row is cut in what it asks, never in how long it has waited (counters and
/// timers keep their width — the docs' `inbox.png` at 120: api's `read · 16.3s` was cut to `read · …` by its read's
/// `· the whole file`).
#[test]
fn the_lists_ages_are_never_cut() {
    let (mut ed, dir, back, third) = three("ages");
    park(&mut ed, back, claudes(1, 60));
    park(&mut ed, third, tarae(400));
    screen(&mut ed, 64, 30);
    feed(&mut ed, "<space>I");
    let rows = screen(&mut ed, 64, 30);
    let row = rows.iter().find(|r| r.contains("third ·")).unwrap_or_else(|| panic!("{rows:#?}"));
    assert!(row.contains("…") && row.contains("6m 40s"), "what it asks cut, its age whole: {row}");
    std::fs::remove_dir_all(&dir).ok();
}

/// The list: `space I` (or a click on the status line's count, where it counts any) — a row each, its session named,
/// what it asks, its kind and how long it has waited, the oldest in hand. `enter` goes to it — its session in front,
/// its card up with the keys — and it's answered there. Nothing waiting: said, no list.
#[test]
fn the_list_goes_to_the_one_picked_and_its_card_takes_the_keys() {
    let (mut ed, dir, back, third) = three("list");
    park(&mut ed, back, claudes(1, 60));
    park(&mut ed, third, tarae(400));
    screen(&mut ed, 160, 30);
    feed(&mut ed, "<space>I");
    let p = ed.picker.as_ref().expect("the inbox");
    assert_eq!(p.title, super::INBOX);
    let labels: Vec<&str> = p.items().iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels.len(), 2);
    assert!(labels[0].starts_with("third · Trust this project's settings?"), "{labels:?}");
    assert!(labels[1].starts_with("back · Claude asks 1"), "{labels:?}");
    // (its time a column, `RUN_TIME_W` wide)
    assert!(p.items()[0].hint.starts_with("project-trust ·  6m 40s"), "{}", p.items()[0].hint);
    let rows = screen(&mut ed, 160, 30);
    assert!(rows.iter().any(|r| r.contains("enter go to it")), "its keys: {rows:#?}");
    // The second row, enter: back in front, its card up with the keys — `y` answers it there
    feed(&mut ed, "<down><ret>");
    assert!(ed.picker.is_none());
    assert_eq!(ed.workspace.id, back);
    assert_eq!(ed.offers.first().map(|o| o.title.as_str()), Some("Claude asks 1"));
    assert!(ed.card_has_keys(), "answered there");
    // Answered meanwhile: the list follows; empty, it closes and says so
    ed.inbox_open();
    session(&mut ed, third).parked.clear();
    ed.inbox_sync();
    assert_eq!(ed.picker.as_ref().map(|p| p.items().len()), Some(1));
    ed.offers.clear();
    ed.inbox_sync();
    assert!(ed.picker.is_none());
    feed(&mut ed, "<space>I");
    assert!(ed.picker.is_none());
    assert!(ed.status.as_ref().is_some_and(|s| s.0.contains("nothing waits for you")), "{:?}", ed.status);
    std::fs::remove_dir_all(&dir).ok();
}
