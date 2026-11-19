//! #640, the design session's rule (2026-11-19): turning to a card that's drawn never makes it vanish, nor shows less of
//! it than the frame before — a card whole while it waits is whole with the keys, its top where it was. Its floor is its
//! own area's (over the chat's foot the chat's last row, never the code's over a slot docked under the code alone);
//! past that it's cut, never dropped.

use crate::editor::Editor;
use crate::offer::{Offer, What};
use crate::sizes::Slot;
use crate::uistate::tests::{chat, editor, light_card, task_panel};

/// A frame at `w`×`h`, as the main loop draws one after an event.
fn frame(ed: &mut Editor, (w, h): (u16, u16)) {
    ed.focus_sync();
    super::render(ed, &mut Vec::new(), w, h).unwrap();
}

/// The review's end card as `space v q` puts it up (`codereview::close_said`'s words): `files` not viewed, the first
/// `high` of them high-risk and not looked at — its yes held for them.
fn end_card(files: usize, high: usize) -> Offer {
    let mut q = format!("main's review · last turn · 0 of {files} viewed\n\n{files} files not viewed\n");
    for i in 1..=files.min(6) {
        q.push_str(&format!("\n  src/high{i}.rs"));
    }
    if files > 6 {
        q.push_str(&format!("\n  … {} more", files - 6));
    }
    let from = q.split('\n').count() + 1;
    let changes = if high == 1 { "change" } else { "changes" };
    q.push_str(&format!(
        "\n\n{high} high-risk {changes} not looked at — it ends once each has had a look (h goes to them, ]h the next, \
         space v h on each):"
    ));
    for i in 1..=high.min(6) {
        q.push_str(&format!("\n  src/high{i}.rs:1"));
    }
    if high > 6 {
        q.push_str(&format!("\n  … {} more", high - 6));
    }
    let warning = from..q.split('\n').count();
    q.push_str("\n\nNo board has read them: space v C has one read them first.");
    q.push_str("\n\nDrafts stay. Opened again, its marks are kept: a file changed since asks only for its lines since.");
    let what =
        What::ReviewEnd { session: crate::event::WorkspaceId::HOME, started: std::time::Instant::now() };
    let mut o = Offer::new("End main's review?".into(), &q, "", String::new(), what);
    o.held = Some(crate::codereview::held_said(high));
    o.warning = Some(warning);
    o
}

/// A weighty card taller than any screen here: a change set of a chat process that isn't there, sixty lines of it.
fn huge_card() -> Offer {
    let mut q = String::from("Claude renames `a` to `b` in 60 files.\n");
    for i in 1..=60 {
        q.push_str(&format!("\n  src/strand{i}.rs"));
    }
    let what = What::ChangeSet { whose: crate::refactor::Whose::Claude(999), ops: false };
    Offer::new("Claude changes 60 files".into(), &q, "", String::new(), what)
}

/// The front card as this frame drew it: its rect and how many of its lines it left out.
fn drawn(ed: &Editor) -> Option<((usize, usize, usize, usize), usize)> {
    Some((ed.screen.offer_card?, ed.screen.offer_cut.unwrap_or(0)))
}

/// A card by name, and what puts it up.
type Card = (&'static str, fn(&mut Editor));

/// Every layout the routing scenes use for a card — the chat docked or not × a bottom panel or not — at halil's size
/// and two others, × a card short, tall (halil's end card), tallest (the nine-file one) and taller than the screen: a card
/// drawn waiting is drawn the frame after `C-q` gave it the keys, with at least the rows it showed, no more of it
/// left out and no narrower (#632, landing 3); whole while it waited, whole with the keys, its top where it was.
#[test]
fn turning_to_a_drawn_card_never_shows_less_of_it() {
    let cards: [Card; 4] = [
        ("short", light_card),
        ("tall", |ed| ed.push_offer(end_card(4, 1))),
        ("tallest", |ed| ed.push_offer(end_card(9, 9))),
        ("taller than the screen", |ed| ed.push_offer(huge_card())),
    ];
    let mut off = Vec::new();
    let mut turned = 0;
    for size in [(80, 34), (120, 40), (80, 24)] {
        for with_chat in [false, true] {
            for slot in [false, true] {
                for (card, put) in cards {
                    let name = format!("{}×{} chat {with_chat} slot {slot} card {card}", size.0, size.1);
                    let mut ed = editor();
                    // (a task's panel under the code alone, at halil's 13 rows — a short one moves no floor)
                    if slot {
                        task_panel(&mut ed);
                        ed.workspace.viewer.slot_front = Some(Slot::Task);
                        frame(&mut ed, size);
                        ed.workspace.set_kept(crate::pane::Shows::Slot, Some(13));
                    }
                    if with_chat {
                        chat(&mut ed, "");
                    }
                    frame(&mut ed, size);
                    if slot {
                        let rows = ed.viewport.0;
                        assert!(
                            rows + 15 <= usize::from(size.1),
                            "{name}: the code over the panel, {rows} rows"
                        );
                    }
                    put(&mut ed);
                    // (laid as it comes, then standing where it was: its top kept — #561)
                    frame(&mut ed, size);
                    frame(&mut ed, size);
                    assert!(!ed.card_has_keys(), "{name}: it came on its own, it waits");
                    let Some((was, was_cut)) = drawn(&ed) else { continue };
                    ed.handle_key("C-q".parse().unwrap());
                    frame(&mut ed, size);
                    let Some((now, now_cut)) = drawn(&ed) else {
                        off.push(format!("{name}: drawn waiting at {was:?}, gone with the keys"));
                        continue;
                    };
                    turned += 1;
                    if !ed.card_has_keys() {
                        off.push(format!("{name}: drawn, but the keys aren't its"));
                    }
                    // (#632, landing 3: nor narrower — the width it stood at kept)
                    if now.3 < was.3 || now_cut > was_cut || now.2 < was.2 {
                        off.push(format!(
                            "{name}: {was:?} cut {was_cut} waiting, {now:?} cut {now_cut} with the keys"
                        ));
                    }
                    if was_cut == 0 && (now_cut != 0 || now.1 != was.1) {
                        off.push(format!(
                            "{name}: whole at {was:?} waiting, {now:?} cut {now_cut} with the keys"
                        ));
                    }
                }
            }
        }
    }
    assert!(off.is_empty(), "{} of {turned}:\n{}", off.len(), off.join("\n"));
    // (most of the 48 have a card drawn waiting to turn to: the test can't pass by drawing none)
    assert!(turned >= 40, "turned to {turned}");
}

/// #640's `C-q` row, on what's left once a drawn card is never dropped: a card no frame has room for. The code sized
/// down over a task's panel by `C-w -` (#637's shots): the way out first, in the pane's terms — `C-w = makes room —
/// the pane has 4 rows, the card needs 8`, the keys staying; `C-w =` then draws it, and `C-q` gives it the keys.
#[test]
fn c_q_on_a_card_with_no_room_says_the_way_out_first() {
    let size = (120, 40);
    let mut ed = editor();
    task_panel(&mut ed);
    ed.workspace.viewer.slot_front = Some(Slot::Task);
    frame(&mut ed, size);
    for _ in 0..15 {
        ed.handle_key("C-w".parse().unwrap());
        ed.handle_key("-".parse().unwrap());
    }
    frame(&mut ed, size);
    let rows = ed.viewport.0;
    assert!(rows < 8, "the code pane sized down: {rows}");
    light_card(&mut ed);
    frame(&mut ed, size);
    assert!(ed.screen.offer_card.is_none(), "no room for it");
    ed.handle_key("C-q".parse().unwrap());
    assert!(!ed.card_has_keys(), "no keys to a card it can't draw");
    let said = ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
    assert!(said.starts_with("C-w = makes room — the pane has "), "{said}");
    assert!(said.ends_with(" rows, the card needs 8"), "{said}");
    ed.handle_key("C-w".parse().unwrap());
    ed.handle_key("=".parse().unwrap());
    frame(&mut ed, size);
    assert!(ed.screen.offer_card.is_some(), "C-w = made room");
    ed.handle_key("C-q".parse().unwrap());
    frame(&mut ed, size);
    assert!(ed.card_has_keys() && ed.screen.offer_card.is_some(), "drawn, the keys its own");
}

/// Halil's layout by name (#640: the halil take, `space v q`'s end card at 80×34): the chat docked, the terminal at 13
/// rows under the code alone, one high-risk change. The card stands over the chat's foot, whole, waiting; `C-q` changes
/// its frame's light and nothing else — the same rect, whole, the keys its own (they held it to the code's floor over
/// the terminal, and it vanished).
#[test]
fn halils_end_card_stays_put_given_the_keys() {
    if crate::sandbox::no_pty_here("term::card_tests::halils_end_card_stays_put_given_the_keys") {
        return;
    }
    let size = (80, 34);
    let mut ed = editor();
    crate::uistate::tests::terminal(&mut ed);
    frame(&mut ed, size);
    ed.workspace.set_kept(crate::pane::Shows::Slot, Some(13));
    chat(&mut ed, "");
    frame(&mut ed, size);
    ed.push_offer(end_card(4, 1));
    frame(&mut ed, size);
    frame(&mut ed, size);
    let (was, cut) = drawn(&ed).expect("drawn waiting over the chat's foot");
    assert_eq!(cut, 0, "whole while it waits: {was:?}");
    let slot = ed.workspace.terminal.as_ref().map(|_| ());
    assert!(slot.is_some(), "the terminal under the code");
    ed.handle_key("C-q".parse().unwrap());
    frame(&mut ed, size);
    assert!(ed.card_has_keys(), "the keys its own");
    assert_eq!(drawn(&ed), Some((was, 0)), "the same rect, whole");
}

/// #632, landing 3 (the design session's, on #640's shots): a card given the keys keeps the width it stood at — never
/// narrower, wider only if what it now shows needs it. The nine-file end card in halil's layout, cut while it waits
/// over the chat's foot, its cut row setting its width: given the keys it stands whole down to the chat's floor, and
/// went 5 columns narrower, its answers shortened with it (`h Go to them · n Keep reviewing` to `h Go there · n Keep`).
/// Its answers whole first (#632), their line is as wide as its cut row; the same card with no high-risk change says
/// `y End it · n Keep reviewing`, narrower than that row — the width it stood at is what holds it.
#[test]
fn the_nine_file_end_card_keeps_its_width_given_the_keys() {
    if crate::sandbox::no_pty_here("term::card_tests::the_nine_file_end_card_keeps_its_width_given_the_keys")
    {
        return;
    }
    let size = (80, 34);
    for (high, answers) in [(9, ["h Go to them", "n Keep reviewing"]), (0, ["y End it", "n Keep reviewing"])]
    {
        let mut ed = editor();
        crate::uistate::tests::terminal(&mut ed);
        frame(&mut ed, size);
        ed.workspace.set_kept(crate::pane::Shows::Slot, Some(13));
        chat(&mut ed, "");
        frame(&mut ed, size);
        // (with none high-risk, nothing holds its yes; as tall as the nine's: more files listed)
        let mut o = end_card(9, high);
        if high == 0 {
            o.held = None;
            for i in 1..=6 {
                o.question.push_str(&format!("\n  src/also{i}.rs"));
            }
        }
        ed.push_offer(o);
        frame(&mut ed, size);
        frame(&mut ed, size);
        let (was, cut) = drawn(&ed).expect("drawn waiting over the chat's foot");
        assert!(cut > 0, "{high}: cut while it waits: {was:?}");
        let whole = |line: &str| answers.iter().all(|a| line.contains(&format!(" {a} ")));
        let line = super::card_line(&ed).unwrap_or_default();
        assert!(whole(&line), "{high}: its answers whole, waiting: {line}");
        ed.handle_key("C-q".parse().unwrap());
        frame(&mut ed, size);
        assert!(ed.card_has_keys(), "{high}: the keys its own");
        let (now, cut) = drawn(&ed).expect("drawn with the keys");
        assert_eq!(cut, 0, "{high}: whole with the keys: {now:?}");
        assert_eq!(
            (now.0, now.1, now.2),
            (was.0, was.1, was.2),
            "{high}: its top and its sides where they were"
        );
        let line = super::card_line(&ed).unwrap_or_default();
        assert!(whole(&line), "{high}: its answers whole, with the keys: {line}");
    }
}
