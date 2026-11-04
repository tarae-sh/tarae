//! The interface tarae draws for and reads from — the person's side of the editor (the remote plan's §2, step 1).
//! The main loop draws each frame for one (`term::event_loop`, `term::present`): at its size, against its grid (only
//! what changed goes out — `Screen`), its resting redraw (`Resting`), written through its link (`Link`), whose thread
//! reads its input into the loop's events. What it says of itself — the `Facts` — is the editor's to read
//! (`Editor.interface`), never the environment's where it's asked: the theme's ground, undercurl, the key twins inside
//! zellij, the clipboard's way, sound, the URL opener, the zone times are shown in. The terminal tarae runs in
//! (`term::terminal`), or an interface across the attach socket (`attach.rs`), whose clipboard, speaker and browser are
//! its own machine's, asked across its wire.

use std::io;
use std::time::{Duration, Instant};

use crate::screen::Screen;

/// What an interface says of itself, read once as it's made (the terminal's: `term::terminal`; a wire's: its `Hello`,
/// its zone again by `Zone`). The default is a tests' editor: none of it — no ground said, plain underlines, no
/// twins, the clipboard's stand-in, no sound, no opener, this machine's zone.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// Its ground is light (the theme `default` is hanji, else meok) — None: it didn't say (meok).
    pub light: Option<bool>,
    /// It takes undercurl and an underline's colour (`CSI 4:3 m`, `CSI 58 …`) — else plain underlines (`term::apply`).
    pub undercurl: bool,
    /// It runs inside zellij: a hinted key zellij takes is said by its twin (`zellij.rs`).
    pub zellij: bool,
    /// The letters that zellij takes, as the interface's own machine names them (a wire's `Hello.taken`, S-G: the
    /// laptop's zellij config may differ from the host's) — None: this machine's `editor.zellij-keys`.
    pub taken: Option<Vec<char>>,
    /// Where a copy or paste goes: this machine's way (`clipboard::pick` — its programs looked for on PATH at each
    /// copy), or the interface's, asked across its wire (`attach::ask`).
    pub clipboard: crate::clipboard::Clip,
    /// Whose speaker plays tarae's sound (`sound.rs`: the output opened as a turn runs with `editor.sound` on).
    pub sound: Speaker,
    /// What `:web-open` hands a page's address to (`web/page.rs`) — none: said.
    pub opener: Opener,
    /// Where the times it shows are told (`log::Zone`): this machine's zone, or the interface's offset.
    pub zone: crate::log::Zone,
    /// A frame it's sent isn't presented yet: it says what it presented with each input (`Event::Input`'s `seen`)
    /// — an interface across a wire. False: a frame written is a frame presented (the terminal, a tests' editor).
    pub presents_late: bool,
}

/// Whose speaker plays tarae's sound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Speaker {
    #[default]
    None,
    /// This machine's (the terminal tarae runs in).
    Here,
    /// The interface's across its wire: what's decided goes there (`ToInterface::Sound`), its client plays it.
    Wire,
}

/// What opens a web address in a browser.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Opener {
    #[default]
    None,
    /// This machine's program (`open`, `xdg-open`).
    Here(&'static str),
    /// The interface's machine, asked across its wire (`Ask::Open`).
    Wire,
}

/// A frame's number (`Editor.frames`).
pub type Frame = u64;

impl crate::editor::Editor {
    /// Whether `frame` is one the person could see: the interface presented it (`Editor.presented`), or it's the frame
    /// being drawn, asked for its own look. Every rule that says "only once drawn" asks this of the frame its fact was
    /// drawn in — a weighty card's answers, the keys an answer handed on, a card with no room, a card read whole, a
    /// review's high-risk lines on screen, a bring's change looked at.
    ///
    /// What an interface attached before the one there now presented doesn't count (`Editor.attached_at`, the remote
    /// plan's decision D): "in sight now" starts over at an attach — a fact drawn before it and drawn on since (each
    /// rule checks it's the same) counts from the first frame drawn for the interface there now; what was seen before
    /// is kept where it was marked.
    pub fn saw(&self, frame: Frame) -> bool {
        let from = match self.attached_at {
            0 => frame,
            at => frame.max(at + 1),
        };
        self.drawing || from <= self.presented
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
        // (the ring keeps the newest presented and what's newer: the ones before it can't be shown any more)
        while self.kept.get(1).is_some_and(|k| k.n <= n) {
            self.kept.pop_front();
        }
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

    /// A press (`m`) the interface made on frame `seen`, older than the newest drawn: it goes on only where the same
    /// element is under it now as on the frame it was made on (the remote plan's decision E) — the same surface and
    /// part (`mouse_under` over each frame's `Screen`), and what says which one it is: the code pane's document, the
    /// front card, else the row's text. Refused, it's said (on a card, as a key typed before it was in sight) and its
    /// drag and release go nowhere (`Editor.press_refused`). The wheel and hover go by the frame now: a misplaced
    /// scroll moves nothing of yours, hover is no input.
    pub fn press_holds(&mut self, seen: Frame, m: &crate::event::Mouse) -> bool {
        use crate::event::MouseKind as K;
        match m.kind {
            K::Down | K::Middle => {}
            K::Drag | K::Up if self.press_refused => {
                self.press_refused = m.kind == K::Drag;
                self.unchanged = true;
                return false;
            }
            _ => return true,
        }
        self.press_refused = false;
        if !self.interface.presents_late || seen >= self.frames {
            return true;
        }
        let (x, y) = (m.x as usize, m.y as usize);
        let now = self.mouse_under(K::Down, x, y);
        let same = match self.kept.iter().position(|k| k.n == seen) {
            Some(i) => {
                let card = self.kept[i].card;
                let (then, row) = self.as_kept(i, |ed, f| (ed.mouse_under(K::Down, x, y), row_text(f, y)));
                let doc = |s: &crate::editor::Screen, p| {
                    s.panes.iter().position(|q| q.0 == p).and_then(|i| s.pane_docs.get(i).copied())
                };
                use crate::mouse::{Part as P, Surface as S};
                then == now
                    && match (now.surface, now.part) {
                        (S::Code(p) | S::PaneTop(p), _) => {
                            let then = self.kept.get(i).and_then(|k| doc(&k.screen, p));
                            then.is_some() && then == doc(&self.screen, p)
                        }
                        (S::Card, _) => card == self.offer_drawn.get().map(|d| (d.0, d.1)),
                        (_, P::Text { .. } | P::Blank | P::Border(_) | P::Frame(_) | P::Outside) => true,
                        _ => row.is_some_and(|r| {
                            Some(r) == self.last_frame.as_ref().and_then(|f| row_text(f, y))
                        }),
                    }
            }
            // (a frame the ring let go — or none presented: what was under it isn't known)
            None => false,
        };
        if same {
            return true;
        }
        self.press_refused = true;
        let said = format!("frame {seen} pressed at {x},{y}, frame {} under it now: {now:?}", self.frames);
        crate::log::line("attach", "info", &format!("a press refused — {said}"));
        match now.surface {
            crate::mouse::Surface::Card => {
                self.offer_refused("a press came before this card was in sight — not taken")
            }
            _ => self.set_status("the screen moved under that press — not taken"),
        }
        false
    }

    /// The kept frame the interface shows now: the newest it presented (`Editor.kept`'s index) — None: none yet.
    pub fn presented_kept(&self) -> Option<usize> {
        self.kept.iter().rposition(|k| k.n <= self.presented)
    }

    /// `f` with kept frame `i`'s layout as `Editor.screen` and its bytes — the layout now put back after (the
    /// `docpane::swap` pattern).
    pub fn as_kept<R>(&mut self, i: usize, f: impl FnOnce(&mut Self, &(Vec<u8>, u16, u16)) -> R) -> R {
        let mut k = self.kept.remove(i).expect("a kept frame");
        std::mem::swap(&mut self.screen, &mut k.screen);
        let r = f(self, &k.frame);
        std::mem::swap(&mut self.screen, &mut k.screen);
        self.kept.insert(i, k);
        r
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

/// Row `y` of a frame as text (trailing blanks dropped) — None: a frame this doesn't read, or no such row.
fn row_text((frame, w, h): &(Vec<u8>, u16, u16), y: usize) -> Option<String> {
    let text = crate::screen::text_hiding(frame, *w, *h, &[])?;
    Some(text.lines().nth(y).unwrap_or_default().to_string())
}

/// How many frames drawn since the one presented the ring keeps (`Editor.kept`): with credits (`tarae_wire`'s
/// `IN_FLIGHT`) an interface is never further behind than that — one older was skipped.
pub const KEPT: usize = 4;

/// A frame drawn for an interface that presents late, kept while it may be what the person sees: its number, its
/// layout (what a press there landed on — `Editor::press_holds`), its bytes (what `C-s` sends) and the front card it
/// drew (id, `rev`).
pub struct Kept {
    pub n: Frame,
    pub screen: crate::editor::Screen,
    pub frame: (Vec<u8>, u16, u16),
    pub card: Option<(u64, u64)>,
}

/// The frame just drawn kept (`Editor.kept`), for an interface that presents late — past `KEPT` drawn since the one
/// presented, the oldest of those goes. The terminal keeps none.
pub(crate) fn keep(ed: &mut crate::editor::Editor) {
    if !ed.interface.presents_late {
        return;
    }
    let Some(frame) = ed.last_frame.clone() else { return };
    let card = ed.offer_drawn.get().map(|d| (d.0, d.1));
    ed.kept.push_back(Kept { n: ed.frames, screen: ed.screen.clone(), frame, card });
    let newer = ed.kept.iter().filter(|k| k.n > ed.presented).count();
    if newer > KEPT
        && let Some(i) = ed.kept.iter().position(|k| k.n > ed.presented)
    {
        ed.kept.remove(i);
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
    /// It takes each frame whole and finds what changed off the main loop (a wire: `attach::Wire`) — `hand`, never
    /// `write`, and no resting redraw: its interface redraws from the grid it holds.
    fn whole(&self) -> bool {
        false
    }
    /// A frame drawn whole handed over at once, never waited on; a buffer comes back for the next frame (one sent).
    fn hand(&mut self, f: Whole) -> io::Result<Option<Vec<u8>>> {
        drop(f);
        Err(io::Error::other("this interface takes what changed, not whole frames"))
    }
    /// It's let go, told why (`said`: what a quit didn't write, printed once its terminal is back) — a wire's words;
    /// the terminal has nobody to tell.
    fn bye(&mut self, _gone: tarae_wire::Gone, _said: String) {}
    /// A message for the interface's machine (a question for its clipboard or browser, a sound for its speaker), to go
    /// by `order` — a wire's; the terminal's machine is this one, nothing goes.
    fn say(&mut self, _m: tarae_wire::ToInterface, _order: tarae_wire::Order) {}
    /// What `bye` said written, waited for `within` at most: tarae is ending, its last words go before it does.
    fn flushed(&mut self, _within: Duration) {}
}

/// A frame drawn whole for a link that finds what changed itself (`Link::whole`).
pub struct Whole {
    pub n: Frame,
    /// The last input the interface sent that was handled before it was drawn (`Event::Input`'s `seq`).
    pub input: u64,
    pub w: u16,
    pub h: u16,
    pub bytes: Vec<u8>,
    /// Its first part read while it was drawn (`Editor.drawn`): read on from there.
    pub drawn: Option<crate::screen::Drawn>,
    /// What the interface shows isn't known: this one goes whole (a focus back, `:redraw`).
    pub unknown: bool,
}

/// An interface the main loop draws for: its facts, its grid and resting redraw, its link.
pub struct Interface {
    pub facts: Facts,
    /// The grid it shows: a frame's diff is made against it (`Screen::present_drawn`) — a whole-frame link's own.
    pub(crate) screen: Screen,
    /// None: the interface redraws from its own grid (a wire's client — `Link::whole`).
    pub(crate) resting: Option<Resting>,
    pub(crate) link: Box<dyn Link>,
    /// The attach that made it (`attach::Pending.link`) — 0: the terminal tarae runs in.
    pub(crate) id: u64,
    /// The next frame goes whole (`invalidate`) — a whole-frame link's, told with the frame.
    pub(crate) unknown: bool,
}

impl Interface {
    pub fn new(facts: Facts, link: Box<dyn Link>) -> Interface {
        let resting = (!link.whole()).then(Resting::default);
        Interface { facts, screen: Screen::new(), resting, link, id: 0, unknown: false }
    }

    /// What it shows may not be the last frame (a resize, focus back, `:redraw`, the resting redraw): the next goes
    /// whole.
    pub fn invalidate(&mut self) {
        self.screen.invalidate();
        self.unknown = true;
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
        ed.handle_event(Event::Input { seen, seq: 0, event: Box::new(Event::Key(k.parse().unwrap())) });
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

    /// At an attach "in sight now" starts over (the remote plan's decision D): a weighty card the interface before
    /// presented isn't in sight for the one that took over until a frame of its own that draws it is presented — its
    /// `y` on what it hasn't presented yet is refused, said; once it has, an answer. The one before is let go, told.
    #[test]
    fn an_attach_starts_in_sight_over() {
        let mut ed = late();
        let id = weighty(&mut ed);
        frame(&mut ed);
        key(&mut ed, 1, "C-q");
        assert!(ed.saw(1) && ed.card_has_keys(), "presented by the first interface");
        let fake = || Fake { size: (100, 30), wrote: Arc::default() };
        let mut there =
            Some(Interface::new(Facts { presents_late: true, ..Facts::default() }, Box::new(fake())));
        ed.host = true;
        let mut coming = Interface::new(Facts { presents_late: true, ..Facts::default() }, Box::new(fake()));
        coming.id = 7;
        ed.attach.coming = Some(coming);
        assert!(crate::attach::swap(&mut ed, &mut there), "a frame owed for the one attached");
        assert_eq!((there.as_ref().map(|i| i.id), ed.attach.current, ed.attached_at), (Some(7), 7, 1));
        assert!(!ed.saw(1), "what the interface before presented isn't in sight for this one");
        key(&mut ed, 1, "y");
        assert!(up(&ed, id), "y on a frame only the interface before presented answered it");
        let said = ed.offer_said_now().unwrap_or_default().to_string();
        assert!(said.contains("came before this card was in sight"), "{said}");
        frame(&mut ed);
        key(&mut ed, 2, "y");
        assert!(!up(&ed, id), "its own frame presented: answered");
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
            seq: 0,
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

    /// A press made on an older frame acts only where the same thing is under it now (the remote plan's decision E):
    /// the code scrolled since — another line under the press — it's refused and said, its drag goes nowhere and the
    /// cursor stays; on the frame drawn now, or one older that drew the same there, it's taken.
    #[test]
    fn a_press_on_an_older_frame_counts_only_where_the_same_thing_is_still_under_it() {
        let scratch = crate::scratch::Scratch::new("press-older");
        let path = scratch.join("a.txt");
        let text: String = (1..=200).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&path, text).unwrap();
        let (x, y) = (20u16, 5u16);
        let mouse = |seen, kind| Event::Input {
            seen,
            seq: 0,
            event: Box::new(Event::Mouse(crate::event::Mouse { kind, x, y, alt: false, ctrl: false })),
        };
        use crate::event::MouseKind as K;
        let mut ed = late();
        ed.open(&path).unwrap();
        frame(&mut ed);
        ed.frame_presented(1);
        // (a press taken on the frame drawn now: what a drag would select from)
        ed.handle_event(mouse(1, K::Down));
        ed.goto_line(150);
        frame(&mut ed);
        let line_now = match ed.mouse_at(x as usize, y as usize).part {
            crate::mouse::Part::Text { line, .. } => line,
            p => panic!("the code's text under it: {p:?}"),
        };
        assert_ne!(line_now, 150, "the press lands elsewhere than the cursor");
        ed.handle_event(mouse(1, K::Down));
        assert_eq!(ed.cursor_line(), 150, "pressed on frame 1's line, another under it now: not taken");
        let said = ed.messages.last().map(|m| m.2.clone()).unwrap_or_default();
        assert!(said.contains("the screen moved under that press"), "{said}");
        ed.handle_event(mouse(1, K::Drag));
        ed.handle_event(mouse(1, K::Up));
        assert_eq!(ed.cursor_line(), 150, "its drag went nowhere");
        // (frame 2 presented: the press is on what's there)
        ed.handle_event(mouse(2, K::Down));
        ed.handle_event(mouse(2, K::Up));
        assert_eq!(ed.cursor_line(), line_now, "pressed on the frame drawn now: taken");
        // (a frame drawn since with the same line there: the press on frame 2 still lands on it)
        frame(&mut ed);
        frame(&mut ed);
        let line_now = match ed.mouse_at(x as usize, y as usize).part {
            crate::mouse::Part::Text { line, .. } => line,
            p => panic!("the code's text under it: {p:?}"),
        };
        ed.goto_line(150);
        ed.handle_event(mouse(3, K::Down));
        assert!(ed.frames > 3);
        assert_eq!(ed.cursor_line(), line_now, "the same line under it on frame 3 and now: taken");
        assert!(ed.kept.len() <= KEPT + 1, "the ring kept {}", ed.kept.len());
    }

    /// A press on an older frame's list row acts only while that row says the same now (decision E: a row's index is
    /// no element's identity, its text stands in for one): a row of `:messages` that says another thing under the
    /// same index — refused, said, the selection stays; a frame drawn since with the same rows there, taken.
    #[test]
    fn a_press_on_an_older_frames_list_row_counts_only_while_the_row_says_the_same() {
        use crate::event::MouseKind as K;
        use crate::pane::GroupKind;
        let mut ed = late();
        for i in 0..60 {
            ed.recorded(crate::editor::ToastKind::Info, format!("message {i}"));
        }
        let ids = ed.message_ids();
        ed.messages_open(Some(ids[40]));
        frame(&mut ed);
        ed.frame_presented(1);
        let here = |ed: &Editor| ed.float_list_here(GroupKind::Messages);
        let x = 50;
        let rows = |ed: &Editor| -> Vec<(usize, usize)> {
            (0..30)
                .filter_map(|y| match ed.mouse_at(x, y) {
                    crate::mouse::Under {
                        surface: crate::mouse::Surface::Floatlist(_),
                        part: crate::mouse::Part::Row(i),
                        ..
                    } => Some((y, i)),
                    _ => None,
                })
                .collect()
        };
        let (y, i) =
            rows(&ed).into_iter().find(|&(_, i)| Some(ids[i]) != here(&ed)).expect("a row not selected");
        let press = |seen, kind| Event::Input {
            seen,
            seq: 0,
            event: Box::new(Event::Mouse(crate::event::Mouse {
                kind,
                x: x as u16,
                y: y as u16,
                alt: false,
                ctrl: false,
            })),
        };
        // (the row under the press says another thing, its index the same — a stand-in for any list's: `:messages`
        // holds its view by id, so the oldest let go past `MESSAGES_KEPT` moves the index, refused by the part itself)
        ed.messages[i].2 = "another message".into();
        frame(&mut ed);
        assert!(rows(&ed).contains(&(y, i)), "the same index under the press");
        let was = here(&ed);
        ed.handle_event(press(1, K::Down));
        ed.handle_event(press(1, K::Up));
        assert_eq!(here(&ed), was, "pressed on frame 1's row, another message there now: not taken");
        let said = ed.messages.last().map(|m| m.2.clone()).unwrap_or_default();
        assert!(said.contains("the screen moved under that press"), "{said}");
        // (drawn twice since, the rows the same: a press on the older of the two lands on what's there)
        frame(&mut ed);
        let seen = ed.frames;
        frame(&mut ed);
        let ids = ed.message_ids();
        let i = rows(&ed).into_iter().find(|&(at, _)| at == y).expect("a row under the press").1;
        assert_ne!(Some(ids[i]), here(&ed), "not the one selected");
        ed.handle_event(press(seen, K::Down));
        assert_eq!(here(&ed), Some(ids[i]), "the same row's text on frame {seen} and now: taken");
    }

    /// A press on an older frame's card acts only while the front card is the one it drew (decision E: its id and
    /// `rev`) — what it says changed since, a press on its way in is refused and said on it; the same card drawn again,
    /// taken.
    #[test]
    fn a_press_on_an_older_frames_card_counts_only_while_its_rev_stands() {
        let mut ed = late();
        weighty(&mut ed);
        frame(&mut ed);
        ed.frame_presented(1);
        frame(&mut ed);
        let (row, x, _) = ed.screen.offer_way.expect("its way in on the card");
        let press = |seen| Event::Input {
            seen,
            seq: 0,
            event: Box::new(Event::Mouse(crate::event::Mouse {
                kind: crate::event::MouseKind::Down,
                x: x as u16,
                y: row as u16,
                alt: false,
                ctrl: false,
            })),
        };
        ed.handle_event(press(1));
        assert!(ed.card_has_keys(), "the same card on frame 1 and frame 2: its way in taken");
        key(&mut ed, 2, "esc");
        assert!(!ed.card_has_keys(), "given back");
        // (what it says changed: drawn again at its next rev)
        ed.offers[0].rev += 1;
        frame(&mut ed);
        assert_eq!(ed.screen.offer_way.map(|w| (w.0, w.1)), Some((row, x)), "its way in where it was");
        ed.handle_event(press(2));
        assert!(!ed.card_has_keys(), "pressed on frame 2's card, another rev of it now: not taken");
        let said = ed.offer_said_now().unwrap_or_default().to_string();
        assert!(said.contains("a press came before this card was in sight"), "{said}");
        ed.handle_event(press(ed.frames));
        assert!(ed.card_has_keys(), "its frame presented: taken");
    }

    /// What the `kept` ring costs a frame (the remote plan's §8b): a 200×50 frame of code drawn 400 times with `j`/`k`
    /// between, for an interface that presents late (each frame kept: its layout cloned, its bytes copied — then
    /// presented, as an ack lets it go) and for the terminal (none kept); the difference of the medians, and what one
    /// kept frame holds. Said for `cargo perf`.
    #[test]
    fn perf_kept_ring() {
        if crate::test_results::unoptimized("interface::tests::perf_kept_ring") {
            return;
        }
        let dir = crate::scratch::Scratch::new("perf-kept");
        let path = dir.join("a.rs");
        let text: String =
            (0..2_000).map(|i| format!("fn f{i}() {{ let x = {i}; }} // line {i}\n")).collect();
        std::fs::write(&path, &text).unwrap();
        let (w, h) = (200u16, 50u16);
        let run = |late: bool| {
            let mut ed = Editor::new(crate::config::Config::default());
            ed.interface.presents_late = late;
            ed.open(&path).unwrap();
            ed.goto_line(10);
            let mut buf = Vec::with_capacity(1 << 16);
            let mut frames = Vec::new();
            for i in 0..400 {
                ed.handle_event(Event::Key((if i % 2 == 0 { "j" } else { "k" }).parse().unwrap()));
                buf.clear();
                let t = std::time::Instant::now();
                crate::term::render(&mut ed, &mut buf, w, h).unwrap();
                frames.push(t.elapsed());
                ed.frame_presented(ed.frames);
            }
            assert_eq!(ed.kept.is_empty(), !late, "kept only for an interface that presents late");
            let bytes = ed.kept.back().map_or(0, |k| k.frame.0.len());
            frames.sort();
            (frames[frames.len() / 2], bytes)
        };
        // (each its best of 3 — the first run in a process also pays the clock ramping up)
        let best = |late| (0..3).map(|_| run(late)).min().unwrap();
        let (terminal, _) = best(false);
        let (late, bytes) = best(true);
        let us = |d: std::time::Duration| d.as_secs_f64() * 1e6;
        let added = us(late) - us(terminal);
        let said = format!(
            "kept ring: a frame {:.1} µs kept, {:.1} µs not; one kept frame's bytes {bytes}\n",
            us(late),
            us(terminal)
        );
        let line = format!("PERF kept_ring_added_us {added:.1} budget=2000 noise=20\n");
        let _ = std::io::Write::write_all(&mut std::io::stderr(), format!("{said}{line}").as_bytes());
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
