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
    /// A frame it's sent isn't presented yet: it says what it presented with each input (`Event::Input`'s `seen`)
    /// — an interface across a wire. False: a frame written is a frame presented (the terminal, a tests' editor).
    pub presents_late: bool,
}

/// A frame's number (`Editor.frames`).
pub type Frame = u64;

impl crate::editor::Editor {
    /// Whether `frame` is one the person could see: the interface presented it (`Editor.presented`), or it's the frame
    /// being drawn, asked for its own look. Every rule that says "only once drawn" asks this of the frame its fact was
    /// drawn in — a weighty card's answers, the keys an answer handed on, a card with no room, a card read whole, a
    /// review's high-risk lines on screen, a bring's change looked at.
    pub fn saw(&self, frame: Frame) -> bool {
        self.drawing || frame <= self.presented
    }

    /// The interface presented frame `n` (the terminal: as it was written — `term::present`; else as its input says):
    /// the rules that wait for it can count it, and — when it's the newest drawn — what it showed the person is
    /// marked: what of the chat was read, the toasts shown (an error's time runs), the reviews' time in sight, the
    /// front card's first sight. An older one marks nothing: the newest's marks go when it's presented.
    pub fn frame_presented(&mut self, n: Frame) {
        let n = n.min(self.frames);
        if n <= self.presented {
            return;
        }
        self.presented = n;
        if self.card_handed.is_some_and(|h| h.2 <= n) {
            self.card_handed = None;
        }
        if let Some(u) = self.offer_unshown.get()
            && self.offer_drawn.get().is_some_and(|d| (d.0, d.1) == (u.0, u.1) && u.2 < d.2 && d.2 <= n)
        {
            self.offer_unshown.set(None);
        }
        if self.presented_marks.is_none_or(|m| m > n) {
            return;
        }
        self.presented_marks = None;
        // (what the chat showed you: what `space L` counts as read)
        crate::chatdoc::had_in_sight(self);
        // (an error under a picker waits for the card — U7)
        let card = self.screen.toast_card.get().is_some();
        self.toasts_drawn(card);
        // (first in sight: its `card` line counts the answer's time from here — `cards.rs`)
        if let Some(o) = self.offers.first()
            && self.offer_drawn.get().is_some_and(|d| (d.0, d.1) == (o.id, o.rev))
            && o.drawn.get().is_none()
        {
            o.drawn.set(Some(crate::clock::now()));
        }
        // (the reviews under way: what of them this frame shows — the review record's time)
        crate::reviews::after_frame(self);
    }

    /// The front card is drawn as it is, but the frame that drew it so isn't presented yet: a weighty one (its answers
    /// count only in sight — `Stands.drawn`), or one the keys were handed on to in that frame (`card_handed`). Its
    /// answers are refused as typed before it was in sight (`card_key`).
    pub fn card_unpresented(&self) -> bool {
        self.offers.first().is_some_and(|o| {
            let drawn = self.offer_drawn.get().filter(|d| (d.0, d.1) == (o.id, o.rev));
            (o.weighty() && drawn.is_some_and(|d| !self.saw(d.2)))
                || self.card_handed.is_some_and(|h| (h.0, h.1) == (o.id, o.rev) && !self.saw(h.2))
        })
    }

    /// The front card no frame presented with room for it (id, `rev`): the last frame had none, or one that draws it
    /// again isn't presented yet — no key of yours answers it (`card_key`).
    pub fn offer_unshown_now(&self) -> Option<(u64, u64)> {
        let u = self.offer_unshown.get()?;
        let drawn = self.offer_drawn.get().filter(|d| (d.0, d.1) == (u.0, u.1) && u.2 < d.2);
        match drawn {
            Some(d) if self.saw(d.2) => None,
            _ => Some((u.0, u.1)),
        }
    }
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

    use crate::editor::Editor;
    use crate::event::Event;

    /// An editor whose interface presents late: a frame drawn isn't presented until an input says so.
    fn late() -> Editor {
        let mut ed = Editor::new(crate::config::Config::default());
        ed.interface.presents_late = true;
        ed
    }

    fn frame(ed: &mut Editor) {
        crate::term::render(ed, &mut Vec::new(), 100, 30).unwrap();
    }

    /// Key `k` as an interface that had presented frame `seen` sends it.
    fn key(ed: &mut Editor, seen: Frame, k: &str) {
        ed.handle_event(Event::Input { seen, event: Box::new(Event::Key(k.parse().unwrap())) });
    }

    /// A weighty card up (its `y` brings back no conversation: nothing starts) — its id.
    fn weighty(ed: &mut Editor) -> u64 {
        let what = crate::offer::What::RecoverChats(Vec::new());
        ed.push_offer(crate::offer::Offer::new(
            "Bring them back?".into(),
            "One line.",
            "",
            String::new(),
            what,
        ));
        ed.offers[0].id
    }

    fn up(ed: &Editor, id: u64) -> bool {
        ed.offers.iter().any(|o| o.id == id)
    }

    /// "Drawn" means presented (the remote plan's §4): a weighty card's `y` typed on a frame before the one that drew
    /// it is refused as typed before it was in sight, said on it; sent once that frame was presented, it answers. The
    /// terminal's frame is presented as it's written: its `y` answers at once.
    #[test]
    fn a_weighty_cards_key_counts_once_the_frame_that_drew_it_was_presented() {
        let mut ed = late();
        let id = weighty(&mut ed);
        frame(&mut ed);
        assert_eq!((ed.frames, ed.presented), (1, 0), "drawn, not presented");
        key(&mut ed, 0, "C-q");
        assert!(ed.card_has_keys(), "focused: no answer");
        key(&mut ed, 0, "y");
        assert!(up(&ed, id), "y typed on frame 0 answered a card frame 1 drew");
        let said = ed.offer_said_now().unwrap_or_default().to_string();
        assert!(said.contains("y came before this card was in sight — not taken"), "{said}");
        key(&mut ed, 1, "y");
        assert!(!up(&ed, id), "its frame presented: answered");
        // (the terminal: written is presented)
        let mut ed = Editor::new(crate::config::Config::default());
        let id = weighty(&mut ed);
        frame(&mut ed);
        assert_eq!(ed.presented, ed.frames, "the terminal's frame presented as it's written");
        for k in ["C-q", "y"] {
            ed.handle_event(Event::Key(k.parse().unwrap()));
        }
        assert!(!up(&ed, id), "answered");
    }

    /// A press on a weighty card's `y` is judged as its key: on a frame not presented yet, refused and said; once it
    /// was, an answer.
    #[test]
    fn a_press_on_a_cards_answer_counts_once_its_frame_was_presented() {
        let mut ed = late();
        let id = weighty(&mut ed);
        frame(&mut ed);
        key(&mut ed, 0, "C-q");
        frame(&mut ed);
        let &(row, x, _, _) = ed.screen.offer_buttons.iter().find(|b| b.3 == 'y').expect("its y on the card");
        let press = |seen| Event::Input {
            seen,
            event: Box::new(Event::Mouse(crate::event::Mouse {
                kind: crate::event::MouseKind::Down,
                x: x as u16,
                y: row as u16,
                alt: false,
                ctrl: false,
            })),
        };
        ed.handle_event(press(0));
        assert!(up(&ed, id), "a press on frame 0 answered");
        let said = ed.offer_said_now().unwrap_or_default().to_string();
        assert!(said.contains("a press came before this card was in sight"), "{said}");
        ed.handle_event(press(ed.frames));
        assert!(!up(&ed, id), "its frame presented: the press answered");
    }

    /// A card read whole counts for a key once the frame it was whole in was presented — and for the frame being
    /// drawn, its own look, at once.
    #[test]
    fn a_card_is_read_whole_once_the_frame_it_was_whole_in_was_presented() {
        let mut ed = late();
        weighty(&mut ed);
        frame(&mut ed);
        assert!(!ed.offer_seen_whole(), "whole in frame 1, frame 0 presented");
        ed.drawing = true;
        assert!(ed.offer_seen_whole(), "the frame being drawn: as it will show");
        ed.drawing = false;
        ed.frame_presented(1);
        assert!(ed.offer_seen_whole(), "presented");
    }

    /// A card no frame had room for stays unanswerable until a frame with room for it was presented — drawn again,
    /// not presented yet, still not.
    #[test]
    fn a_card_with_no_room_is_shown_once_a_frame_with_room_was_presented() {
        let mut ed = late();
        let id = weighty(&mut ed);
        let card = (id, ed.offers[0].rev);
        crate::term::render(&mut ed, &mut Vec::new(), 30, 4).unwrap();
        assert_eq!(ed.offer_unshown_now(), Some(card), "no room");
        frame(&mut ed);
        assert_eq!(ed.offer_unshown_now(), Some(card), "room in frame 2, frame 0 presented");
        ed.frame_presented(1);
        assert_eq!(ed.offer_unshown_now(), Some(card), "the frame with no room presented");
        ed.frame_presented(2);
        assert_eq!(ed.offer_unshown_now(), None, "the one with room presented");
        assert_eq!(ed.offer_unshown.get(), None, "and let go");
    }

    /// What a frame showed the person is marked once it's presented, not as it's drawn: the chat's messages read,
    /// the toasts shown (an error's time runs from there), the front card's first sight. An older frame presented
    /// marks nothing — the newest's go when it is.
    #[test]
    fn what_a_frame_showed_is_marked_once_its_presented() {
        let mut ed = crate::uistate::tests::editor();
        ed.interface.presents_late = true;
        crate::uistate::tests::chat(&mut ed, "");
        let chat = ed.workspace.claude.chat.as_mut().unwrap();
        chat.msgs.push(crate::chat::Msg::new(crate::chat::Role::User, "first?", ""));
        chat.msgs.push(crate::chat::Msg::new(crate::chat::Role::Assistant, "An answer.", ""));
        weighty(&mut ed);
        ed.set_error("an error to read");
        let seen = |ed: &Editor| ed.workspace.claude.chat.as_ref().unwrap().seen;
        let marked =
            |ed: &Editor| (seen(ed), ed.toasts.iter().all(|t| t.shown), ed.offers[0].drawn.get().is_some());
        crate::uistate::tests::frame(&mut ed);
        crate::uistate::tests::frame(&mut ed);
        assert_eq!(marked(&ed), (0, false, false), "drawn twice, nothing presented");
        ed.frame_presented(ed.frames - 1);
        assert_eq!(marked(&ed), (0, false, false), "an older frame presented: the newest's marks wait");
        ed.frame_presented(ed.frames);
        assert_eq!(marked(&ed), (2, true, true), "the newest presented");
    }

    /// An error shown in a frame has its time restarted as that frame is presented — after the frame's own time —
    /// and is owed no frame for it: shown is drawn (an idle tarae draws nothing more).
    #[test]
    fn a_toast_shown_as_its_frame_is_presented_is_owed_no_frame() {
        let mut ed = Editor::new(crate::config::Config::default());
        ed.set_error("an error to read");
        // (its time restarted past the frame's own)
        crate::clock::advance(std::time::Duration::from_millis(5));
        frame(&mut ed);
        assert!(ed.toasts.iter().all(|t| t.shown), "shown, presented");
        assert_eq!(ed.frame_owed(), None, "nothing owed");
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
