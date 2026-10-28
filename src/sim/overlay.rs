//! A plugin's overlay on its own clock (`plugin/overlay.rs`), simulated: shown, replaced, taken away by its plugin, frames,
//! ticks, time passing and `editor.motion` changed, in any order. After every step: with none up nothing wakes; with one
//! up a wake is asked, a step away while a row dries or it fades (else at its fade's start); a tick served lets go of
//! every one past its time, the frame drawn; while it's up it's never drawn narrower.

use std::cell::Cell;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::{Editor, TICK};
use crate::plugin::{OverlayRole as Role, OverlaySpan as Span, PluginOverlay as Overlay};

const SEEDS: &[u64] = &[0, 1, 2, 7, 10];

const INPUTS: usize = 60;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn an_overlay_wakes_on_its_own_clock_and_goes() {
    super::seeds("an_overlay_wakes_on_its_own_clock_and_goes", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("an_overlay_wakes_on_its_own_clock_and_goes", choices, LIMIT);
    sim.frames = Some((80, 16));
    let mut ed = Editor::new(crate::config::Config::default());
    ed.docs[0].text = ropey::Rope::from_str("alpha beta\ngamma delta\n");
    ed.plugins_detached();
    // (what a step saw go wrong, for the check after it — and the widest drawn while it's up)
    let stuck = Cell::new(None::<String>);
    let widest = Cell::new(None::<usize>);
    let check = |ed: &Editor| held(ed, &stuck);
    sim.walk(&mut ed, INPUTS, |sim, ed| step(sim, ed, &stuck, &widest), check, print);
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn frame(ed: &mut Editor, stuck: &Cell<Option<String>>, widest: &Cell<Option<usize>>) {
    crate::term::render(ed, &mut Vec::new(), 80, 16).unwrap();
    let drawn = ed.screen.overlays.borrow().first().map(|o| o.1.2);
    if let (Some(w), Some(was)) = (drawn, widest.get())
        && w < was
    {
        stuck.set(Some(format!("drawn {w} wide, {was} before while it's up")));
    }
    if up(ed) == 0 {
        widest.set(None);
    } else if let Some(w) = drawn {
        widest.set(Some(widest.get().map_or(w, |was| was.max(w))));
    }
}

fn up(ed: &Editor) -> usize {
    ed.plugins.as_ref().map_or(0, |p| p.overlays.len())
}

fn step(
    sim: &mut Sim,
    ed: &mut Editor,
    stuck: &Cell<Option<String>>,
    widest: &Cell<Option<usize>>,
) -> String {
    let c = &mut sim.choices;
    // (by arm: shown 4, taken away 1, a frame 2, a tick 4, time passing 4, the motion setting 1)
    match c.weighted(&[4, 1, 2, 4, 4, 1]) {
        0 => {
            let text = *c.pick(&["j ×4", "C-w  v   space  f", "i  insert"]);
            let hold = *c.pick(&[None, Some(150), Some(400), Some(1000)]);
            let o = Overlay { rows: vec![vec![Span { text: text.into(), role: Role::Text }]], hold };
            // (over its time already, the one there is no longer up: this is a new one, its own width)
            let now = crate::clock::now();
            if ed.plugins.as_ref().is_some_and(|p| p.overlays.values().all(|o| o.gone(now, ed.moving()))) {
                widest.set(None);
            }
            ed.plugin_overlay("demo", Some(o));
            format!("shown: {text:?}, held {hold:?}")
        }
        1 => {
            ed.plugin_overlay("demo", None);
            widest.set(None);
            "taken away".into()
        }
        2 => {
            frame(ed, stuck, widest);
            "a frame".into()
        }
        3 => {
            // (as the main loop's wake: its tick, then a frame — one past its time lets it go, and is drawn)
            let over = ed
                .plugins
                .as_ref()
                .is_some_and(|p| p.overlays.values().any(|o| o.gone(crate::clock::now(), ed.moving())));
            ed.schedule_tick();
            if up(ed) > 0
                && ed
                    .plugins
                    .as_ref()
                    .is_some_and(|p| p.overlays.values().any(|o| o.gone(crate::clock::now(), ed.moving())))
            {
                stuck.set(Some("a tick left an overlay past its time".into()));
            } else if over && ed.unchanged {
                stuck.set(Some("an overlay let go, and the frame said nothing changed".into()));
            }
            frame(ed, stuck, widest);
            "a tick".into()
        }
        4 => {
            let ms = *c.pick(&[30, 100, 102, 150, 400, 1000, 3000]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => {
            let level = *c.pick(&["full", "calm", "none"]);
            ed.config.motion = level.into();
            format!("editor.motion = {level}")
        }
    }
}

/// None up: no wake. One up: a wake, within a step while a row dries or it fades, never past its time.
fn held(ed: &Editor, stuck: &Cell<Option<String>>) -> Result<(), String> {
    if let Some(s) = stuck.take() {
        return Err(s);
    }
    let wake = ed.overlays_wake();
    let Some(p) = ed.plugins.as_ref().filter(|p| !p.overlays.is_empty()) else {
        return match wake {
            Some(w) => Err(format!("no overlay up, a wake asked in {w:?}")),
            None => Ok(()),
        };
    };
    let Some(w) = wake else { return Err("an overlay up, and no wake asked".into()) };
    let (now, moving) = (crate::clock::now(), ed.moving());
    let slack = Duration::from_millis(2);
    for o in p.overlays.values() {
        let left = o.until(now, moving);
        if w > left + slack && !left.is_zero() {
            return Err(format!("a wake in {w:?}, past its time ({left:?} left)"));
        }
        let drying = (0..o.rows.len()).any(|i| o.wetness(i, now, moving) > 0.0);
        let fading = moving && o.alpha(now, true) < 1.0;
        if (drying || fading) && w > TICK + slack {
            return Err(format!("a wake in {w:?} while it dries or fades"));
        }
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    let (now, moving) = (crate::clock::now(), ed.moving());
    let left: Vec<u128> = ed
        .plugins
        .as_ref()
        .map_or(Vec::new(), |p| p.overlays.values().map(|o| o.until(now, moving).as_millis()).collect());
    format!("overlays {left:?} ms left · wake {:?}", ed.overlays_wake())
}
