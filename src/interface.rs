//! The interface tarae draws for and reads from — the person's side of the editor (the remote plan's §2, step 1).
//! The main loop draws each frame for one (`term::event_loop`, `term::present`): at its size, against its grid (only
//! what changed goes out — `Screen`), its resting redraw (`Resting`), written through its link (`Link`), whose thread
//! reads its input into the loop's events. What it says of itself — the `Facts` — is the editor's to read
//! (`Editor.interface`), never the environment's where it's asked: the theme's ground, undercurl, the key twins inside
//! zellij, the clipboard's way, sound, the URL opener. The terminal tarae runs in is the only one yet
//! (`term::terminal`); an interface across a socket is step 3's.

use std::io;
use std::time::{Duration, Instant};

use crate::screen::Screen;

/// What an interface says of itself, read once as it's made (the terminal's: `term::terminal`). The default is a
/// tests' editor: none of it — no ground said, plain underlines, no twins, the clipboard's stand-in, no sound, no
/// opener.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// Its ground is light (the theme `default` is hanji, else meok) — None: it didn't say (meok).
    pub light: Option<bool>,
    /// It takes undercurl and an underline's colour (`CSI 4:3 m`, `CSI 58 …`) — else plain underlines (`term::apply`).
    pub undercurl: bool,
    /// It runs inside zellij: a hinted key zellij takes is said by its twin (`zellij.rs`).
    pub zellij: bool,
    /// Where a copy or paste goes (`clipboard::pick`) — its programs looked for on PATH at each copy.
    pub clipboard: crate::clipboard::Env,
    /// Its speaker plays tarae's sound (`sound.rs`: the output opened as a turn runs with `editor.sound` on).
    pub sound: bool,
    /// The program `:web-open` hands a page's address to (`web/page.rs`) — None: none, said.
    pub opener: Option<&'static str>,
}

/// How the main loop reaches an interface: made ready and left as found, its size, the thread reading its input,
/// where a frame goes.
pub trait Link {
    /// Ready to show frames and send input (the terminal: raw mode, the alternate screen, the mouse).
    fn begin(&mut self) -> io::Result<()>;
    /// Left as `begin` found it.
    fn end(&mut self) -> io::Result<()>;
    /// Its size in cells now — asked for each frame.
    fn size(&mut self) -> io::Result<(u16, u16)>;
    /// Its input read on a thread of its own into `tx`, as `Event`s — once, as the loop starts.
    fn listen(&mut self, tx: crate::event::Tx);
    /// What changed of a frame (`frame`), then what goes after it (`own`: OSC 52 — `Editor.term_out`), written and
    /// flushed. The main loop waits while it's written.
    fn write(&mut self, frame: &[u8], own: &[u8]) -> io::Result<()>;
}

/// An interface the main loop draws for: its facts, its grid and resting redraw, its link.
pub struct Interface {
    pub facts: Facts,
    /// The grid it shows: a frame's diff is made against it (`Screen::present_drawn`).
    pub(crate) screen: Screen,
    pub(crate) resting: Resting,
    pub(crate) link: Box<dyn Link>,
}

impl Interface {
    pub fn new(facts: Facts, link: Box<dyn Link>) -> Interface {
        Interface { facts, screen: Screen::new(), resting: Resting::default(), link }
    }
}

/// How long input rests before a screen sent in parts is drawn whole again (`term::event_loop`).
pub const RESTING_REDRAW: Duration = Duration::from_millis(500);

/// When the whole screen goes again (`term::event_loop`): `RESTING_REDRAW` after the last write that went out in
/// parts — a terminal's healing (what it left of its own, anything else that drifted). Its time runs from that write:
/// an event that writes nothing (a job's result that changed nothing, a state asked) doesn't put it off — else events
/// a little under `RESTING_REDRAW` apart would hold it back, to come in the quiet after them, a frame nothing asked
/// for then (`Editor.redraw_due` says it's due).
#[derive(Default)]
pub struct Resting {
    at: Option<Instant>,
    /// Put off once already for an animation's frame (`now_or_after`) — until the whole screen has gone.
    held: bool,
}

impl Resting {
    /// A frame went out at `now`: in parts (`patched`) — the whole screen due `RESTING_REDRAW` from now — or whole,
    /// none due (and a hold spent with it).
    pub fn wrote(&mut self, patched: bool, now: Instant) {
        self.at = patched.then(|| now + RESTING_REDRAW);
        self.held &= patched;
    }

    /// How long the main loop waits for an event at `now` before the whole screen goes — None: none is due.
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        self.at.map(|at| at.saturating_duration_since(now))
    }

    pub fn due(&self) -> bool {
        self.at.is_some()
    }

    /// Its time come at `now`: whether the whole screen goes now. Once per due it waits for an animation's frame
    /// due before another `RESTING_REDRAW` (`tick`: `Editor::tick_due`) — that frame first, the whole screen
    /// `RESTING_REDRAW` after (else a late fade tick lost the race: the card drawn whole by the redraw, ci round
    /// 5). Only once: the next deadline draws it whatever ticks — an animation stepping each second (nothing on a
    /// clock: the working sign, the reviewer's time) always has its next frame that close, and would hold it the whole turn.
    pub fn now_or_after(&mut self, tick: Option<Instant>, now: Instant) -> bool {
        if !self.held && tick.is_some_and(|t| t <= now + RESTING_REDRAW) {
            self.wrote(true, now);
            self.held = true;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// An interface in a test: a size of its own, what's written to it kept, no input.
    struct Fake {
        size: (u16, u16),
        wrote: Arc<Mutex<Vec<u8>>>,
    }

    impl Link for Fake {
        fn begin(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn end(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn size(&mut self) -> io::Result<(u16, u16)> {
            Ok(self.size)
        }
        fn listen(&mut self, _: crate::event::Tx) {}
        fn write(&mut self, frame: &[u8], own: &[u8]) -> io::Result<()> {
            let mut w = self.wrote.lock().unwrap();
            w.extend_from_slice(frame);
            w.extend_from_slice(own);
            Ok(())
        }
    }

    /// A frame is drawn at the interface's size and written through its link, the clipboard's OSC 52 after it — never
    /// the terminal the tests run in (which has none, or another size): the screen as `vt100` reads what went out,
    /// its width and height the link's; the editor's geometry the same; a second frame with nothing changed writes
    /// nothing; `:redraw` sends it whole again.
    #[test]
    fn a_frame_is_drawn_for_its_interface_at_its_size_and_written_through_its_link() {
        let dir = crate::scratch::Scratch::new("interface-frame");
        std::fs::write(dir.join("a.txt"), "alpha beta\n").unwrap();
        let mut ed = crate::editor::Editor::new(crate::config::Config::default());
        ed.open(&dir.join("a.txt")).unwrap();
        let wrote = Arc::new(Mutex::new(Vec::new()));
        let link = Fake { size: (37, 9), wrote: wrote.clone() };
        let mut iface = Interface::new(Facts::default(), Box::new(link));
        let mut frame = Vec::new();
        ed.term_out.extend_from_slice(b"\x1b]52;c;eA==\x1b\\");
        crate::term::present(&mut ed, &mut iface, &mut frame).unwrap();
        let first = std::mem::take(&mut *wrote.lock().unwrap());
        assert!(first.ends_with(b"\x1b[?2026l\x1b]52;c;eA==\x1b\\"), "the frame, then the clipboard's own");
        let mut vt = vt100::Parser::new(9, 37, 0);
        vt.process(&first);
        assert!(vt.screen().contents().contains("alpha beta"), "{}", vt.screen().contents());
        let drawn = (ed.screen.width, ed.screen.height);
        assert_eq!(
            drawn,
            (Some(37), Some(8)),
            "the editor drew at the interface's size (the rows above the status line)"
        );
        assert!(ed.term_out.is_empty(), "what was the terminal's is sent");
        crate::term::present(&mut ed, &mut iface, &mut frame).unwrap();
        assert!(wrote.lock().unwrap().is_empty(), "nothing changed, nothing written");
        ed.redraw = true;
        crate::term::present(&mut ed, &mut iface, &mut frame).unwrap();
        let whole = std::mem::take(&mut *wrote.lock().unwrap());
        assert!(whole.len() >= first.len() - 16, "whole again: {} of {} bytes", whole.len(), first.len());
    }

    /// ci round 5: the resting redraw waits once per due for an animation's frame due before long — that frame
    /// first, the whole screen `RESTING_REDRAW` after (a late fade tick lost the race: the card drawn whole by the
    /// redraw) — and only once: with nothing on a clock the wave steps each second, its next frame always that
    /// close, and each step writes in parts (the review of round 5: held the whole turn). A whole frame spends the
    /// hold; a tick far off, or none, doesn't hold it.
    #[test]
    fn the_resting_redraw_waits_once_for_an_animations_frame_due_before_long() {
        let (t0, ms) = (Instant::now(), Duration::from_millis);
        for tick in [ms(100), ms(500)] {
            let mut r = Resting::default();
            r.wrote(true, t0);
            let at = t0 + RESTING_REDRAW;
            // (a tick late — its time passed — or due soon: put off, the whole wait again)
            assert!(!r.now_or_after(Some(t0 + tick), at), "a tick due at {tick:?}");
            assert_eq!(r.wait(at), Some(RESTING_REDRAW));
            // (that tick wrote in parts, the next is as close: the redraw goes at the next deadline all the same)
            let next = at + ms(300);
            r.wrote(true, next);
            assert!(
                r.now_or_after(Some(next + RESTING_REDRAW + ms(200)), next + RESTING_REDRAW),
                "held once only"
            );
            // (the whole screen went: the next due may wait once again)
            r.wrote(false, next + RESTING_REDRAW);
            r.wrote(true, next + ms(1000));
            assert!(!r.now_or_after(Some(next + ms(1600)), next + ms(1500)), "a new due, a new hold");
        }
        let mut r = Resting::default();
        r.wrote(true, t0);
        let at = t0 + RESTING_REDRAW;
        assert!(r.now_or_after(Some(at + ms(59_000)), at), "a tick a minute off doesn't hold it");
        assert!(r.now_or_after(None, at), "no tick: now");
    }

    /// The whole screen goes again `RESTING_REDRAW` after the last write that went out in parts (`Resting`):
    /// events that write nothing, however close together, don't put it off — the wait shrinks to the deadline;
    /// a write in parts starts it again; a whole frame leaves none due. `Term::settle` and the idle e2e count on
    /// it (`redraw_due`).
    #[test]
    fn the_resting_redraw_is_timed_from_the_last_write() {
        let (t0, ms) = (Instant::now(), Duration::from_millis);
        let mut r = Resting::default();
        assert_eq!((r.wait(t0), r.due()), (None, false), "nothing written yet");
        r.wrote(true, t0);
        assert_eq!(r.wait(t0), Some(RESTING_REDRAW));
        // (events 400 ms apart that write nothing: each wait what's left, never the whole again)
        for (at, left) in [(400, 100), (499, 1), (800, 0)] {
            assert_eq!(r.wait(t0 + ms(at)), Some(ms(left)), "an event at {at} ms");
        }
        assert!(r.due());
        r.wrote(true, t0 + ms(450));
        assert_eq!(r.wait(t0 + ms(800)), Some(ms(150)), "a write in parts at 450 ms: due at 950");
        r.wrote(false, t0 + ms(950));
        assert_eq!((r.wait(t0 + ms(950)), r.due()), (None, false), "the whole screen went");
    }
}
