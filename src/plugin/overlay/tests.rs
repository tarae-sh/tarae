//! A plugin's overlay, through the demo plugin's `:overlay` (`plugin-sdk/demo`): where it floats and what it gives way
//! to.

use super::*;
use crate::key::{Code, Key};
use crate::scratch::Scratch;

const W: u16 = 100;
const H: u16 = 30;

#[track_caller]
fn settle(ed: &mut Editor, done: impl Fn(&Editor) -> bool) {
    crate::test_results::settle_saying(ed, done, |ed| format!("{:?}", ed.ui_overlays()));
}

/// An editor with the demo plugin loaded from a folder of its own (kept while the test runs), on `text`.
fn editor(name: &str, text: &str) -> (Editor, Scratch) {
    let dir = Scratch::new(&format!("overlay-{name}"));
    std::fs::write(dir.join("demo.wasm"), abi::DEMO).unwrap();
    let mut ed = Editor::new(crate::config::Config::default());
    ed.docs[0].text = ropey::Rope::from_str(text);
    ed.plugins_sync(super::super::host::scan(&dir));
    settle(&mut ed, |ed| ed.plugin_command("overlay").is_some());
    // (its permissions' card stands where the asks do: answered elsewhere, as far as these are concerned)
    ed.offers.clear();
    (ed, dir)
}

fn keys(ed: &mut Editor, text: &str) {
    for c in text.chars() {
        ed.handle_key(Key::plain(Code::Char(c)));
    }
}

/// `:overlay args` typed, then the demo's answer applied (`done` holds once it is).
fn overlay(ed: &mut Editor, args: &str, done: impl Fn(&Editor) -> bool) {
    keys(ed, &format!(":overlay {args}"));
    ed.handle_key("ret".parse().unwrap());
    settle(ed, done);
}

fn up(ed: &Editor) -> usize {
    ed.plugins.as_ref().map_or(0, |p| p.overlays.len())
}

/// The frame's rows, drawn `W` × `H`.
fn frame(ed: &mut Editor) -> Vec<String> {
    crate::term::render(ed, &mut Vec::new(), W, H).unwrap();
    let (f, fw, fh) = ed.last_frame.as_ref().unwrap();
    crate::screen::text(f, *fw, *fh).unwrap_or_default().lines().map(str::to_string).collect()
}

/// Where the last frame drew the overlay (x, y, width, height) — None: not drawn.
fn rect(ed: &Editor) -> Option<(usize, usize, usize, usize)> {
    ed.ui_state().overlays.first().and_then(|o| o.rect)
}

/// Shown at once and listed by the state (never on Claude's level), replaced by its plugin's next — its width never
/// narrower while it's up —, and gone at once with none.
#[test]
fn an_overlay_shows_at_once_is_replaced_and_goes() {
    let (mut ed, _dir) = editor("shows", "fn main() {}\n");
    overlay(&mut ed, "text 600000 the stress test passes", |ed| up(ed) == 1);
    let rows = frame(&mut ed);
    let (_, y, w, _) = rect(&ed).expect("drawn at once");
    assert!(rows[y].contains("the stress test passes"), "{rows:#?}");
    let s = &ed.ui_state().overlays[0];
    assert_eq!((s.plugin.as_str(), s.text.as_str()), ("demo", "the stress test passes"));
    assert!(ed.ui_state_claude().overlays.is_empty(), "chrome, never Claude's");
    assert!(ed.ui_state().json()["overlays"].is_array(), "in the JSON");
    // (replaced: shorter, as wide as it was — a row that slides is jitter)
    overlay(&mut ed, "text 600000 passes", |ed| ed.ui_overlays()[0].text == "passes");
    frame(&mut ed);
    assert_eq!(rect(&ed).map(|r| r.2), Some(w), "never narrower while it's up");
    overlay(&mut ed, "none", |ed| up(ed) == 0);
    assert!(!frame(&mut ed).iter().any(|r| r.contains("passes")), "gone at once");
}

/// The asks' corner is the overlay's only while no ask stands there: a card, which-key, the prompt and a picker each
/// hide it, and it's back once they go.
#[test]
fn the_corner_overlay_gives_way_and_comes_back() {
    let (mut ed, _dir) = editor("corner", "fn main() {}\n");
    overlay(&mut ed, "keys 600000", |ed| up(ed) == 1);
    frame(&mut ed);
    let shown = rect(&ed).expect("in its corner");
    assert!(shown.0 + shown.2 > W as usize / 2 && shown.1 > H as usize / 2, "bottom-right: {shown:?}");
    let what = crate::offer::What::Asks { call: crate::tools::Call::test(1) };
    ed.push_offer(crate::offer::Offer::new("Claude asks".into(), "?", "", String::new(), what));
    frame(&mut ed);
    assert_eq!(rect(&ed), None, "an ask stands there");
    ed.offers.clear();
    frame(&mut ed);
    assert_eq!(rect(&ed), Some(shown), "back where it was");
    for (what, open) in [("which-key", " "), ("the prompt", ":")] {
        keys(&mut ed, open);
        frame(&mut ed);
        assert_eq!(rect(&ed), None, "{what} up");
        ed.handle_key("esc".parse().unwrap());
        frame(&mut ed);
        assert_eq!(rect(&ed), Some(shown), "{what} gone");
    }
    let item = crate::picker::Item {
        label: "one".into(),
        action: crate::picker::Action::Buffer(ed.doc().id),
        hint: String::new(),
        glyph: None,
    };
    ed.open_picker(crate::picker::Picker::of("buffers", "buffers", vec![item], false), None);
    frame(&mut ed);
    assert_eq!(rect(&ed), None, "under a picker");
}

/// A cut row is never wider than its cells, its `…` in them — a span that fills them exactly with more after it too.
#[test]
fn a_cut_row_keeps_to_its_cells() {
    let span = |text: &str, role| abi::Span { text: text.into(), role };
    let (t, k) = (abi::Role::Text, abi::Role::Key);
    let rows = [
        (vec![span("abcd", t), span("ef", t)], 4),
        // (a keycap filling them: its text and its two cells of ground)
        (vec![span("ab", t), span("cd", k), span("ef", t)], 6),
        (vec![span("abcdef", t)], 4),
        (vec![span("ab", t), span("cd", k)], 4),
    ];
    for (row, cells) in rows {
        let out = cut(&row, cells).expect("cut");
        let w: usize = out.iter().map(span_width).sum();
        assert!(w <= cells, "{w} cells of {cells}: {out:?}");
        assert!(out.last().is_some_and(|s| s.text.ends_with('…')), "{out:?}");
    }
    // (as it came: a span of `CELLS_MAX` cells, then more)
    let row = vec![span(&"x".repeat(CELLS_MAX), t), span("more", t)];
    let s = Shown::new(abi::Overlay { rows: vec![row], hold: None }, crate::clock::now(), None);
    assert!(s.width <= CELLS_MAX, "{} cells", s.width);
    assert_eq!(s.text(), format!("{}more", "x".repeat(CELLS_MAX)), "whole for its card");
}

/// Replaced, only the rows that changed land wet: one the same keeps drying from where it was.
#[test]
fn only_the_rows_that_changed_land_wet() {
    let (mut ed, _dir) = editor("wet", "fn main() {}\n");
    let shown = |ed: &Editor| ed.plugins.as_ref().unwrap().overlays["demo"].clone();
    overlay(&mut ed, "text 600000 kept | first", |ed| up(ed) == 1);
    let before = shown(&ed);
    overlay(&mut ed, "text 600000 kept | second", |ed| ed.ui_overlays()[0].text == "kept\nsecond");
    let after = shown(&ed);
    assert_eq!(after.wet[0], before.wet[0], "the same row: as dry as it was");
    assert!(after.wet[1] > before.wet[1], "the changed row: wet again");
}

/// Its plugin unloaded (its file gone from the folder), its overlay goes with it.
#[test]
fn an_overlay_goes_with_its_plugin() {
    let (mut ed, dir) = editor("unloaded", "fn main() {}\n");
    overlay(&mut ed, "text 600000 the stress test passes", |ed| up(ed) == 1);
    std::fs::remove_file(dir.join("demo.wasm")).unwrap();
    ed.plugins_sync(super::super::host::scan(&dir));
    settle(&mut ed, |ed| ed.plugin_command("overlay").is_none());
    assert_eq!(up(&ed), 0, "gone with it");
    assert!(!frame(&mut ed).iter().any(|r| r.contains("stress test")), "not drawn");
}

/// A toast at its bottom spot stands where the asks do: the overlay isn't drawn while it's there, and is again at once
/// once it's gone.
#[test]
fn the_corner_overlay_gives_way_to_a_toast_at_its_bottom_spot() {
    let lines = 3 * H as usize;
    let text: String = (0..lines).map(|i| format!("let line_{i} = {i};\n")).collect();
    let (mut ed, _dir) = editor("toast", &text);
    ed.config.scrolloff = 0;
    overlay(&mut ed, "text 600000 the stress test passes", |ed| up(ed) == 1);
    ed.toasts.clear();
    ed.set_status("saved");
    // (the cursor's row the code's top, line by line upward: the toast's top spot is taken, it stands at its bottom one)
    let mut low = false;
    for line in (0..2 * H as usize).rev() {
        ed.goto_line(line);
        frame(&mut ed);
        let Some((_, y, _, h)) = ed.screen.toast_card.get() else { continue };
        if 2 * (y + h) > H as usize {
            low = true;
            assert_eq!(rect(&ed), None, "line {line}: drawn beside the toast at {y}");
        }
    }
    assert!(low, "the toast never stood at its bottom spot");
    ed.toasts.clear();
    frame(&mut ed);
    assert!(rect(&ed).is_some(), "back once it's gone");
}

/// The screen row line `n` (0-based) is drawn on.
fn row_of(rows: &[String], n: usize) -> usize {
    let text = format!("let line_{n} = {n};");
    rows.iter().position(|r| r.contains(&text)).unwrap_or_else(|| panic!("line {n} in sight: {rows:#?}"))
}

/// The cursor's row is never under it: where it comes into the corner the overlay moves up out of its way, by
/// `OVERLAY_REACH` rows at most — never a far jump.
#[test]
fn the_corner_overlay_gives_way_to_the_cursors_row() {
    let lines = H as usize;
    let text: String = (0..lines).map(|i| format!("let line_{i} = {i};\n")).collect();
    let (mut ed, _dir) = editor("cursor", &text);
    // (the cursor's row down to the code's foot: no lines kept below it)
    ed.config.scrolloff = 0;
    overlay(&mut ed, "text 600000 the stress test passes", |ed| up(ed) == 1);
    // (`Plugin demo: approved` stands at the top; out of these rows' way)
    ed.toasts.clear();
    frame(&mut ed);
    let (_, home, ..) = rect(&ed).expect("in its corner");
    let mut moved = false;
    for line in 0..lines {
        ed.goto_line(line);
        let rows = frame(&mut ed);
        let c = row_of(&rows, line);
        if let Some((_, y, _, h)) = rect(&ed) {
            assert!(!(y..y + h).contains(&c), "line {line}: over the cursor's row {c}: {:?}", (y, h));
            assert!(y <= home && home - y <= 3, "line {line}: {y}, a far jump from {home}");
            moved |= y != home;
        }
    }
    assert!(moved, "the cursor's row never came into its corner");
}

/// A press on it is its plugin's (`pressed`, its row from 0) — the demo says it — and the keys stay where they were; a
/// row cut to its room is whole on a card under the mouse.
#[test]
fn a_press_is_its_plugins_and_a_cut_row_is_whole_under_the_mouse() {
    let (mut ed, _dir) = editor("press", "fn main() {}\n");
    let say = "text 600000 passes | The offsets stay on character boundaries now — checking the stress test.";
    overlay(&mut ed, say, |ed| up(ed) == 1);
    crate::term::render(&mut ed, &mut Vec::new(), 60, H).unwrap();
    let (x, y, ..) = rect(&ed).expect("drawn");
    let under = ed.mouse_at(x + 5, y + 1);
    assert_eq!((under.surface, under.part), (crate::mouse::Surface::Overlay, crate::mouse::Part::Row(1)));
    let keys_before = ed.keys_to();
    let m = crate::event::Mouse {
        kind: crate::event::MouseKind::Down,
        x: (x + 5) as u16,
        y: (y + 1) as u16,
        alt: false,
        ctrl: false,
    };
    ed.handle_event(crate::event::Event::Mouse(m));
    settle(&mut ed, |ed| ed.toasts.iter().any(|t| t.text.contains("pressed 1")));
    assert_eq!(ed.keys_to(), keys_before, "news: the keys stay");
    // (at 60 cells its sentence is cut: recorded where it's printed, whole)
    let spots = ed.screen.cut_spots.borrow();
    let whole = spots.iter().find_map(|s| match &s.3 {
        crate::whole::Cut::Text(w) if w.on == crate::whole::On::Overlay => Some(w.text.clone()),
        _ => None,
    });
    assert_eq!(
        whole.as_deref(),
        Some("The offsets stay on character boundaries now — checking the stress test.")
    );
}
