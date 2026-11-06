//! `editor.motion` — how much moves on screen: `full` (default) · `calm` · `none`. What moves on a clock (the
//! working sign, a toast's fade) moves only at `full`; at `calm` the sign holds `▄`, its time counted by the
//! second, and a toast comes and goes whole. Bleeding ink (번짐 — the head of what's being written, a function of
//! where the stream is, never of time) stays at `calm` and goes at `none`; the rows of an edit being written and
//! a decision's stamp stay at all three — shown at the stream's pace (`writing::Pace`: it takes the lumps' jumps out)
//! at `full` and `calm`, as they come at `none`. macOS's Reduce Motion (read once at start, on a worker) makes `full`
//! behave as `calm`; an explicit `none` stays `none`.
//!
//! Motions on a clock (`Moving`, `Editor.motions`): what an invisible act touched flashes (undo, redo, `.`, a
//! yank, a paste that leaves no selection) — made where the act is, drawn by `term::drying` on one ease-out
//! (`strength`), a frame a `TICK` step of its own clock (`motion_wake`) until it's gone, then none. At `calm` a
//! flash shows whole for its time and goes in one frame; at `none` none is made (nothing to say). Unfocused, or
//! from before a switch to `none`, one ends by its time: a frame as it goes. Made in tests too (drawn only where
//! `motion_drawn` — off in tests unless one turns it on).
//!
//! A stamp your key makes in sight (§5: an edit review's change head, the turn review rail's file, the chat's knot
//! row) is pressed (`press`, `What::Press` by its `Place`): it lands lit and dries to its dry ink over `PRESS` on the
//! flash's drying (`pressed`, `term::stamp_spans`); on the row still in hand it stays lit. Only at `full`: at `calm`
//! and `none` it's dry at once (nothing on a clock).
//!
//! Claude's fresh writing dries (§2: `wet`, `What::Wet` by document, its lines `Document.wet`): where a write you
//! didn't watch land is in an open buffer — review at the end's unasked edit, a change set's `y` all, a run of Claude's
//! that rewrote the file (`run_wrote`, as the buffer takes it: `wet_sync`) — the lines it left (`written`; a pure
//! deletion none) land on the change's green and dry over `WET` (`term::wet_bg`); on a line already on a change's wash
//! that wash lands deeper and dries to its own instead (`term::wet_deeper` — one sign). None after your `y` in its
//! review (the stamp's press says that act). Only at `full`.
//!
//! What came on its own dries once as it lands in sight (§6, #401/#402: `arrived`, `What::Arrived` — "a state that
//! just changed dries once"; the user, 2026-11-06: "the other places too, so it all behaves consistently"): a card's peek
//! the first frame it's drawn in any form; where the frame before drew them (`Seen`, told after each frame by `drawn`), a
//! session list row's state word changed (`Doing::word`), a session a Claude started come (its row whole, its name on the
//! strip), the sessions' strip's state word and its `•`, the status line's `◂ new`, its `▲N waiting` and `N runs quiet`
//! grown, a run's tab's badge turned to its end mark with the slot out of hand — the flash's ground (`term::flash_bg`,
//! `term::arrived_style`) under its words, drying over `ARRIVED`, the ink's time: news you didn't make. A session's
//! "waits for you" says it once: no toast where a frame drew its card's peek (#405, `peeks_said`). Each on its own clock:
//! what lands in one frame dries together, a newer landing elsewhere never cuts it short (a drying isn't a wave — two
//! may dry at once), a place landing again while it dries starts over. None for what your key opened or answered right there
//! (a card you asked for is focused whole; your key's own place has the flash or the press), none while you type (it's
//! taken in silence: nothing moves while you type) — at `calm` whole for its time, at `none` nothing, as the flash.
//!
//! What a flash says (`flash_saying`: a yank's "yanked …") is a note only where a frame drew that flash: kept in
//! `:messages` at once, its toast after the frame that didn't draw it (the chat's transcript draws no flash, a
//! buffer moved or out of sight, named colours) or as it goes undrawn (`words_drawn`, `words_unseen`).

use std::cell::Cell;
use std::ops::Range;
use std::time::{Duration, Instant};

use crate::document::DocId;
use crate::editor::{Editor, ToastKind, tick};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Full,
    Calm,
    None,
}

pub const VALUES: &[&str] = &["full", "calm", "none"];

impl Editor {
    /// How much moves now: the setting, `full` as `calm` under the system's Reduce Motion.
    pub fn motion(&self) -> Motion {
        match self.config.motion.as_str() {
            "none" => Motion::None,
            "calm" => Motion::Calm,
            _ if self.reduce_motion => Motion::Calm,
            _ => Motion::Full,
        }
    }

    /// Whether what moves on a clock moves (the working sign, a toast's fade).
    pub fn moving(&self) -> bool {
        self.motion() == Motion::Full
    }

    /// Whether the head of what's being written is drawn bleeding (off in tests unless one turns it on — `bleed_ink`).
    pub fn bleeds(&self) -> bool {
        self.bleed_ink && self.motion() != Motion::None
    }

    /// How old a motion is: its time on the clock, or `TARAE_MOTION_AT`'s pinned age (shots).
    pub fn motion_age(&self, m: &Moving) -> Duration {
        self.motion_at.unwrap_or_else(|| crate::clock::since(m.born))
    }

    /// How strong a motion is drawn now, 0 ‥ 1 — at `full` its drying (`strength`), at `calm` whole while it shows,
    /// at `none` (and where nothing is drawn — tests) nothing.
    pub fn motion_strength(&self, m: &Moving) -> f32 {
        if !self.motion_drawn {
            return 0.0;
        }
        let age = self.motion_age(m);
        match self.motion() {
            Motion::Full => strength(age, m.life),
            Motion::Calm if age < shown_for(m.life) => 1.0,
            _ => 0.0,
        }
    }

    /// What an act touched in the current document flashes: `ranges` (byte ranges at its version now — sorted
    /// and merged here, an empty one widened to the character there, past `RANGES_MAX` none). None at `none`;
    /// a newer flash on that document takes the place of the one before.
    pub fn flash(&mut self, act: Act, ranges: Vec<Range<usize>>) {
        if let Some(what) = self.flash_of(act, ranges) {
            self.motion_start(what);
        }
    }

    /// A flash of the current document's `ranges`, not yet begun (`flash`; a yank to `+` carries it with its copy
    /// until the clipboard took it — `clipboard::copy_flashing`). None at `none`.
    pub fn flash_of(&self, act: Act, ranges: Vec<Range<usize>>) -> Option<What> {
        if self.motion() == Motion::None {
            return None;
        }
        let doc = self.doc();
        let ranges = flash_ranges(&doc.text, ranges)?;
        Some(What::Flash { doc: doc.id, version: doc.version(), ranges, act })
    }

    /// A motion begins now, in the place of one of its kind on the same thing (whose words, unsaid, are a toast).
    pub fn motion_start(&mut self, what: What) {
        let (gone, kept): (Vec<Moving>, Vec<Moving>) =
            std::mem::take(&mut self.motions).into_iter().partition(|m| m.what.same(&what));
        self.motions = kept;
        self.words_unseen(gone);
        let life = match what {
            What::Flash { .. } => FLASH,
            What::Press(_) => PRESS,
            What::Wet(_) => WET,
            What::Arrived(_) => ARRIVED,
        };
        self.motions.push(Moving {
            what,
            born: crate::clock::now(),
            life,
            seen: Cell::new(false),
            words: None,
        });
    }

    /// Your key stamped a decision at `place`, in sight (§5): the stamp lands lit and dries over `PRESS`. Only at
    /// `full` — at `calm` and `none` it's dry at once, as before.
    pub fn press(&mut self, place: Place) {
        if self.motion() == Motion::Full {
            self.motion_start(What::Press(place));
        }
    }

    /// Claude's write landed in `path` unwatched (§2: review at the end's unasked edit, a change set's `y` all, a
    /// run of Claude's that rewrote it): where an open buffer is the file as written (`after`), the lines it left
    /// are wet (`Document.wet`) and dry over `WET`, a newer write's in their place. Pure deletions leave none (no ink
    /// left). Only at `full` — at `calm` and `none` there's no wet ink.
    pub fn wet_landed(&mut self, path: &std::path::Path, before: &ropey::Rope, after: &ropey::Rope) {
        if !self.moving() {
            return;
        }
        let Some(id) = (self.docs.iter())
            .find(|d| d.path.as_deref() == Some(path) && !d.loading && d.text == *after)
            .map(|d| d.id)
        else {
            return;
        };
        self.wet(id, before);
    }

    /// An open buffer took a run's write (`run_wrote`, `wet_sync`): its lines wet (at `full`), and the page heard
    /// settling as an unseen landing is (`sound::settle`, whatever `editor.motion` says) — where the wet ink lands,
    /// the settle sounds.
    fn run_landed(&mut self, path: &std::path::Path, before: &ropey::Rope, after: &ropey::Rope) {
        self.wet_landed(path, before, after);
        crate::sound::settle(self, path, before, after);
    }

    /// Document `id` took Claude's write over `before` (its text now the write's): the lines it left are wet, a
    /// newer write's in their place; a pure deletion leaves what was wet as it is. Only at `full`.
    pub fn wet(&mut self, id: DocId, before: &ropey::Rope) {
        if !self.moving() {
            return;
        }
        let Some(doc) = self.docs.iter_mut().find(|d| d.id == id) else { return };
        let lines = written(before, &doc.text);
        if lines.is_empty() {
            return;
        }
        doc.wet = Some(lines);
        self.motion_start(What::Wet(id));
    }

    /// A run of Claude's changed `files` (`snapshot::Change`, `tools.rs`): each open buffer it rewrote is wet and
    /// heard as it takes the text — now if it has (`run_landed`), else once it does (the disk watch's reload, the
    /// turn's gather: `wet_sync`). A buffer with your unsaved edits takes nothing; none waits past its next version.
    pub fn run_wrote(&mut self, files: &[crate::snapshot::Change]) {
        if !self.moving() && !crate::sound::on(self) {
            return;
        }
        for c in files {
            let (Some(was), Some(now)) = (
                c.before.as_ref().and_then(|b| b.text.as_deref()),
                c.after.as_ref().and_then(|b| b.text.as_deref()),
            ) else {
                continue;
            };
            // (the open buffer first: a run that rewrote the tree builds ropes only for the files open)
            let Some(doc) = self.docs.iter().find(|d| d.path.as_deref() == Some(c.path.as_path())) else {
                continue;
            };
            let taken = doc.text == now;
            if !taken && doc.is_modified() {
                continue;
            }
            let (doc, version) = (doc.id, doc.version());
            let (before, after) = (ropey::Rope::from_str(was), ropey::Rope::from_str(now));
            if taken {
                self.run_landed(&c.path, &before, &after);
            } else {
                self.wet_waiting.retain(|w| w.doc != doc);
                self.wet_waiting.push(Waiting { doc, version, path: c.path.clone(), before, after });
            }
        }
    }

    /// The lines of `doc` drying now, and how wet (0 ‥ 1) — none where nothing is drawn or it's dry.
    pub fn wet_on<'d>(&self, doc: &'d crate::document::Document) -> Option<(&'d [Range<usize>], f32)> {
        let lines = doc.wet.as_deref()?;
        let m = self.motions.iter().find(|m| m.what == What::Wet(doc.id))?;
        let k = self.motion_strength(m);
        (k > 0.0).then_some((lines, k))
    }

    /// The diff of `doc` at `version` landed (`git_schedule`): a hunk reset waiting on it (`press_on_diff`) presses
    /// its turn file's `u` now, the frame it first shows — where that diff is empty; a later version's lets it go.
    pub fn diff_landed(&mut self, doc: DocId, version: u64) {
        let Some((id, at)) = self.press_on_diff.filter(|&(id, at)| id == doc && version >= at) else {
            return;
        };
        self.press_on_diff = None;
        let d = self.docs.iter().find(|d| d.id == id);
        if version == at
            && let Some(path) = d.filter(|d| d.git_hunks.is_empty()).and_then(|d| d.path.clone())
        {
            crate::codereview::press_turn_file(self, &path);
        }
    }

    /// How lit the stamp at `place` is drawn now, 0 (dry) ‥ 1 (lit) — its press drying; 0 with none under way, or
    /// where nothing moves on a clock.
    pub fn pressed(&self, place: &Place) -> f32 {
        if !self.moving() {
            return 0.0;
        }
        (self.motions.iter())
            .find(|m| matches!(&m.what, What::Press(p) if p == place))
            .map_or(0.0, |m| self.motion_strength(m))
    }

    /// What came on its own landed in sight at `places`, in this frame (§6): it dries once, together, on its own clock —
    /// a landing still drying elsewhere dries on, never cut short; a place of it landing again starts over (it leaves
    /// the older landing, which goes once it holds none). What a row still drying says isn't news again: a row come
    /// whole covers its own word (a child's `working` a frame after it came keeps its row drying). None at `none` or
    /// while you type.
    pub fn arrived(&mut self, mut places: Vec<Place>) {
        if self.motion() == Motion::None || self.typing() {
            return;
        }
        places.retain(
            |p| !matches!(p, Place::State(id) | Place::Strip(id) if self.arrival(&Place::Row(*id)) > 0.0),
        );
        if places.is_empty() {
            return;
        }
        for m in &mut self.motions {
            if let What::Arrived(at) = &mut m.what {
                at.retain(|p| !places.contains(p));
            }
        }
        self.motions.retain(|m| !matches!(&m.what, What::Arrived(at) if at.is_empty()));
        self.motion_start(What::Arrived(places));
    }

    /// How fresh what landed at `place` is drawn now, 0 (dry) ‥ 1 (just come) — 0 with none under way.
    pub fn arrival(&self, place: &Place) -> f32 {
        (self.motions.iter())
            .find(|m| matches!(&m.what, What::Arrived(at) if at.contains(place)))
            .map_or(0.0, |m| self.motion_strength(m))
    }

    /// You're typing (focus §7 I, `watch::typing`'s rule): your letters become text — insert mode, a prompt, a picker's
    /// query, the chat's input once something's in it.
    pub fn typing(&self) -> bool {
        use crate::uistate::{Region, Takes};
        let k = self.keys_to();
        k.takes == Takes::Text
            && (k.region != Region::Chat
                || self.workspace.claude.chat.as_ref().is_some_and(|c| !c.input.text().is_empty()))
    }

    /// `flash` begins saying `msg` (a yank's "yanked …"): kept in `:messages` and a note now, a toast only if no
    /// frame draws it (`words_drawn`); no flash, a toast at once.
    pub fn flash_saying(&mut self, flash: Option<What>, kind: ToastKind, msg: String) {
        let Some(what) = flash else { return self.say(kind, msg) };
        self.motion_start(what);
        let msg = self.flashed_note(kind, msg);
        if let Some(m) = self.motions.last_mut() {
            m.words = Some((kind, msg));
        }
    }

    /// After a frame: the words of a flash it drew stay a note; of one it didn't, a toast — whether one was made
    /// (the frame is drawn again with it).
    pub fn words_drawn(&mut self) -> bool {
        let unseen: Vec<(ToastKind, String)> =
            (self.motions.iter_mut()).filter_map(|m| m.words.take().filter(|_| !m.seen.get())).collect();
        let said = !unseen.is_empty();
        unseen.into_iter().for_each(|(kind, msg)| self.flash_unseen(kind, msg));
        said
    }

    /// Motions gone before a frame drew them: their words a toast.
    fn words_unseen(&mut self, gone: Vec<Moving>) {
        for (kind, msg) in gone.into_iter().filter_map(|m| m.words) {
            self.flash_unseen(kind, msg);
        }
    }

    /// What the current document's transactions write is recorded from now on (`Document.touched`) — whether
    /// this call armed it (an act inside another's, a paste in a `.`, leaves it to the outer one).
    pub fn record_touched(&mut self) -> bool {
        let doc = self.doc_mut();
        let armed = doc.touched.is_none();
        if armed {
            doc.touched = Some(Vec::new());
        }
        armed
    }

    /// What was recorded since `record_touched` flashes as `act`'s, where that call armed it.
    pub fn flash_touched(&mut self, armed: bool, act: Act) {
        if !armed {
            return;
        }
        let ranges = self.doc_mut().touched.take().unwrap_or_default();
        self.flash(act, ranges);
    }

    /// The flash to draw on `doc` now, if one is — its ranges (at the version they were made at), strength, and
    /// where the drawer says it drew it (`Moving.seen`, for its words).
    pub fn flash_on(&self, doc: &crate::document::Document) -> Option<ToDraw<'_>> {
        self.motions.iter().find_map(|m| match &m.what {
            What::Flash { doc: d, version, ranges, .. } if *d == doc.id && *version == doc.version() => {
                let k = self.motion_strength(m);
                (k > 0.0).then_some((ranges.as_slice(), k, &m.seen))
            }
            _ => None,
        })
    }

    /// Whether a motion moves on screen now, step by step: moving at all, in sight.
    fn motions_tick(&self) -> bool {
        self.motion() != Motion::None && !self.unfocused
    }

    /// When the next frame a motion wants is due — on its own clock, `born + k·TICK` (2 ms past), the first step
    /// a full `TICK` after its act; at `calm`, unfocused or at `none` once, as it goes (let go by its time). None
    /// past the last — every wake ends — and while pinned (`TARAE_MOTION_AT`).
    pub fn motion_wake(&self) -> Option<Duration> {
        if self.motion_at.is_some() {
            return None;
        }
        let full = self.moving() && self.motions_tick();
        self.motions
            .iter()
            .map(|m| {
                let (age, over) = (crate::clock::since(m.born), shown_for(m.life));
                if age >= over {
                    // (over and not yet let go: the frame that draws it gone, now)
                    return Duration::ZERO;
                }
                let at = if full {
                    (age.as_nanos() / tick().as_nanos() + 1) * tick().as_nanos()
                } else {
                    over.as_nanos()
                };
                Duration::from_nanos((at - age.as_nanos()) as u64) + Duration::from_millis(2)
            })
            .min()
    }

    /// Motions over let go (`schedule_tick`) — whether any went (a frame draws them gone). Pinned ages
    /// (`TARAE_MOTION_AT`) stay.
    pub(crate) fn motions_prune(&mut self) -> bool {
        if self.motion_at.is_some() || self.motions.is_empty() {
            return false;
        }
        // (and a document's drying whose lines went — an undo put another text back: nothing left to draw)
        let docs = &self.docs;
        let dry = |id: DocId| docs.iter().find(|d| d.id == id).is_none_or(|d| d.wet.is_none());
        let (gone, kept): (Vec<Moving>, Vec<Moving>) =
            std::mem::take(&mut self.motions).into_iter().partition(|m| {
                crate::clock::since(m.born) >= shown_for(m.life) || matches!(m.what, What::Wet(id) if dry(id))
            });
        self.motions = kept;
        let any = !gone.is_empty();
        for m in &gone {
            if let What::Wet(id) = m.what
                && let Some(d) = self.docs.iter_mut().find(|d| d.id == id)
            {
                d.wet = None;
            }
        }
        self.words_unseen(gone);
        any
    }
}

/// Before a frame is drawn (`term::render_frame`), what it draws of what comes on its own, against what the frame before
/// drew (`Seen`): `peeks`, the cards it draws as a peek, of `drawn`, every card it draws — a peek whose card no frame drew
/// before has just come, not one your key made; where the frame before drew the session list, a row whose state changed to
/// another word, and a row a Claude started (a child: `Session.parent`) that wasn't there; where it drew the sessions'
/// strip, a session's state word changed there, and its `•` come; `◂ new` come (`unread`); `▲N waiting` and `N runs
/// quiet` grown; and where it drew the slot's tabs, a run's badge turned to its end mark — the slot not in hand. They dry
/// together, on their own clock (`arrived`); what's drawn is kept, for the cards still waiting.
pub fn arrivals(ed: &mut Editor, peeks: &[u64], drawn: &[u64], unread: bool) {
    use crate::event::WorkspaceId;
    let mut places = Vec::new();
    for &id in peeks.iter().filter(|id| !ed.seen.cards.contains(id)) {
        if !ed.offers.iter().any(|o| o.id == id && o.yours) {
            places.push(Place::Peek(id));
        }
    }
    if !(drawn.is_empty() && ed.seen.cards.is_empty()) {
        let waiting: Vec<u64> =
            (ed.offers.iter()).chain(ed.workspaces.iter().flat_map(|l| &l.parked)).map(|o| o.id).collect();
        ed.seen.cards.retain(|id| waiting.contains(id));
        for &id in drawn {
            if !ed.seen.cards.contains(&id) {
                ed.seen.cards.push(id);
            }
        }
    }
    // (against the list as the frame before drew it: a frame where it opens takes its states as they are; a word come in
    // the place of another, or of none — one gone to no word dries nothing)
    if let Some(before) = &ed.seen.list {
        let changed = |&(id, word): &(WorkspaceId, &str)| {
            !word.is_empty() && before.iter().any(|b| b.0 == id && b.1 != word)
        };
        places.extend(states(ed).iter().filter(|s| changed(s)).map(|s| Place::State(s.0)));
        let new = |l: &&crate::workspace::Workspace| {
            l.claude.parent.is_some() && !before.iter().any(|b| b.0 == l.id)
        };
        places.extend(
            (std::iter::once(&ed.workspace).chain(&ed.workspaces)).filter(new).map(|l| Place::Row(l.id)),
        );
    }
    if let Some(before) = &ed.seen.strip {
        let front = ed.workspace.id;
        let child = |id| {
            (std::iter::once(&ed.workspace).chain(&ed.workspaces))
                .any(|l| l.id == id && l.claude.parent.is_some())
        };
        for (id, (word, news)) in strip(ed) {
            match before.iter().find(|b| b.0 == id).map(|b| b.1) {
                Some((was, had)) => {
                    // (the front's `working` is said where its step is, never on the strip — `term::session_tabs_in`)
                    if !word.is_empty() && word != was && !(id == front && word == "working") {
                        places.push(Place::Strip(id));
                    }
                    if news && !had {
                        places.push(Place::News(id));
                    }
                }
                // (a child come on the strip, as in the list: its name)
                None if child(id) => places.push(Place::Row(id)),
                None => {}
            }
        }
    }
    if ed.seen.unread == Some(false) && unread {
        places.push(Place::Unread);
    }
    let (waiting, quiet) = waiting_counts(ed, drawn);
    if ed.seen.waiting.is_some_and(|w| waiting > w) {
        places.push(Place::Waiting);
    }
    if ed.seen.quiet.is_some_and(|q| quiet > q) {
        places.push(Place::Quiet);
    }
    // (a run's tab while its slot is in sight and your keys aren't in it: where they are, its panel says how it went)
    if let Some(before) =
        ed.seen.badges.as_ref().filter(|_| !ed.workspace.is_at(crate::workspace::Here::Slot))
    {
        use crate::chrome::Badge;
        let ended = |b: Badge| !matches!(b, Badge::None | Badge::Running);
        for t in ed.panel_tabs() {
            if ended(t.badge) && before.contains(&(t.slot, Badge::Running)) {
                places.push(Place::Badge(t.slot));
            }
        }
    }
    // (a child come in the list and on the strip: one place)
    let mut once = Vec::with_capacity(places.len());
    for p in places {
        if !once.contains(&p) {
            once.push(p);
        }
    }
    ed.arrived(once);
}

/// After a frame (`term::render_drawing`): a session's "waits for you" held for its card's peek (#405,
/// `Editor::said_by_peek`) — dropped where this frame drew a card of that session's, whole or as a peek (it says it),
/// else its toast. Whether one was made (the frame is drawn again with it).
pub fn peeks_said(ed: &mut Editor) -> bool {
    if ed.peek_words.is_empty() {
        return false;
    }
    let mut said = false;
    for (id, kind, msg) in std::mem::take(&mut ed.peek_words) {
        let cards: Vec<u64> = match id == ed.workspace.id {
            true => ed.offers.iter().map(|o| o.id).collect(),
            false => {
                ed.workspaces.iter().filter(|l| l.id == id).flat_map(|l| &l.parked).map(|o| o.id).collect()
            }
        };
        if !cards.iter().any(|c| ed.screen.asks_drawn.contains(c)) {
            ed.peek_unseen(id, kind, msg);
            said = true;
        }
    }
    said
}

/// After a frame (`term::render_frame`): what it drew of what changes on its own (`Seen`) — the session list's states,
/// the strip's words, `◂ new`, the status line's counts, the slot's badges; each None where it drew none.
pub fn drawn(ed: &mut Editor) {
    let listed = ed.screen.lists.borrow().iter().any(|l| l.0 == crate::pane::GroupKind::Sessions);
    ed.seen.list = listed.then(|| states(ed));
    let d = ed.screen.news.take();
    ed.seen.strip =
        (!d.strip.is_empty()).then(|| strip(ed).into_iter().filter(|s| d.strip.contains(&s.0)).collect());
    ed.seen.unread = Some(d.unread);
    (ed.seen.waiting, ed.seen.quiet) = (Some(d.waiting), Some(d.quiet));
    ed.seen.badges = d.badges;
}

/// Each session's state word (`Doing::word`), as the session list says it now.
fn states(ed: &Editor) -> Vec<(crate::event::WorkspaceId, &'static str)> {
    (std::iter::once(&ed.workspace).chain(&ed.workspaces)).map(|l| (l.id, ed.doing(l).word())).collect()
}

/// Each session's words on the strip that change on their own: its state word, and its `•` (news while it was behind).
fn strip(ed: &Editor) -> Vec<(crate::event::WorkspaceId, (&'static str, bool))> {
    (std::iter::once(&ed.workspace).chain(&ed.workspaces))
        .map(|l| (l.id, (ed.doing(l).word(), l.news())))
        .collect()
}

/// The status line's `▲N waiting` (what waits for your answer and no card of `drawn` draws) and `N runs quiet`, as it
/// counts them (`term::draw_statusline`).
pub fn waiting_counts(ed: &Editor, drawn: &[u64]) -> (usize, usize) {
    let inbox = ed.inbox();
    let here = crate::uistate::session_id(ed.workspace.claude.id);
    let in_sight = |e: &crate::inbox::Entry| match e.card() {
        Some(id) => drawn.contains(&id),
        None => {
            e.kind == "edit" && e.up == "review" && e.session == here && ed.screen.review_area.get().is_some()
        }
    };
    let waiting = inbox.iter().filter(|e| e.asks_answer() && !in_sight(e)).count();
    (waiting, inbox.iter().filter(|e| e.kind == "silent-run").count())
}

/// What the frame before drew of what changes on its own (`arrivals` tells this frame's landings against it; `drawn`
/// takes it after each frame). Each None before a frame drew that place.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    /// Every card a frame drew so far, whole or as a peek, while it waits — a peek not among them has just come.
    pub cards: Vec<u64>,
    /// The session list's rows' state words — None where the frame drew no list (opened: nothing on it has just changed).
    pub list: Option<Vec<(crate::event::WorkspaceId, &'static str)>>,
    /// The strip's sessions it drew, each its state word and its `•` — None where it drew no strip.
    pub strip: Option<Vec<(crate::event::WorkspaceId, (&'static str, bool))>>,
    /// `◂ new` on the status line.
    pub unread: Option<bool>,
    /// `▲N waiting`'s N and `N runs quiet`'s.
    pub waiting: Option<usize>,
    pub quiet: Option<usize>,
    /// The slot's tabs' badges — None where the frame drew no slot's tabs.
    pub badges: Option<Vec<(crate::sizes::Slot, crate::chrome::Badge)>>,
}

/// What a frame draws of `Seen`'s places, recorded where each is drawn (`Screen.news`).
#[derive(Clone, Debug, Default)]
pub struct Drawn {
    /// The sessions the strip named.
    pub strip: Vec<crate::event::WorkspaceId>,
    pub unread: bool,
    pub waiting: usize,
    pub quiet: usize,
    pub badges: Option<Vec<(crate::sizes::Slot, crate::chrome::Badge)>>,
}

/// Every event (`handle_event`): a buffer a run of Claude's rewrote (`run_wrote`) whose version moved — wet and heard
/// if it's now the file as the run left it, else let go (your edit, another write).
pub fn wet_sync(ed: &mut Editor) {
    if ed.wet_waiting.is_empty() {
        return;
    }
    let mut landed = Vec::new();
    ed.wet_waiting.retain(|w| match ed.docs.iter().find(|d| d.id == w.doc) {
        Some(d) if d.version() == w.version => true,
        Some(d) => {
            if d.text == w.after {
                landed.push((w.path.clone(), w.before.clone(), w.after.clone()));
            }
            false
        }
        None => false,
    });
    for (path, before, after) in landed {
        ed.run_landed(&path, &before, &after);
    }
}

#[cfg(test)]
mod tests;

/// How long a flash lives (§1): its peak, near half at the next `TICK`, a trace, gone — a blink with a soft edge
/// (Neovim's yank highlight is a 150 ms hard cut; at 200 ms the curve left the step after the peak at a quarter,
/// unseen on the sheets).
pub const FLASH: Duration = Duration::from_millis(300);
/// How long a stamp's press dries (§5): lit on its key's frame, dry at its end — the flash's drying, its time.
pub const PRESS: Duration = Duration::from_millis(300);
/// How long Claude's fresh writing dries (§2): drying ink is slow — ten `TICK` steps on the ease-out.
pub const WET: Duration = Duration::from_secs(1);
/// How long what came on its own dries (§6): the ink's time — news you didn't make, seen at the eye's edge, where the
/// flash's 0.3 s blink is for the place your key just acted on.
pub const ARRIVED: Duration = WET;
/// The most ranges a flash keeps (a `.` over thousands of cursors flashes none).
pub const RANGES_MAX: usize = 256;
/// The most lines an undo's span may cover and still flash (past it, its line diff would be the cost).
pub const LINES_MAX: usize = 2000;
/// Under this strength a motion is gone: no frame spent on a change nobody can see.
const GONE: f32 = 0.05;

/// `TARAE_MOTION_AT=<ms>`: every motion drawn at that age and none ending — a shot's picture of a motion at a given
/// step (docshots take the peak, `0`). Changes when a picture is taken, never what one shows.
pub fn pinned_age() -> Option<Duration> {
    std::env::var("TARAE_MOTION_AT").ok()?.trim().parse().ok().map(Duration::from_millis)
}

/// A flash to draw (`Editor::flash_on`): its ranges, strength, and where the drawer says it drew it.
pub type ToDraw<'a> = (&'a [Range<usize>], f32, &'a Cell<bool>);

/// A motion under way: what moves, since when, for how long — and what it says, until a frame drew it or not.
#[derive(Clone, Debug)]
pub struct Moving {
    pub what: What,
    pub born: Instant,
    pub life: Duration,
    /// A frame drew it (`term::draw_text`, through `flash_on`).
    pub seen: Cell<bool>,
    /// Its words (`flash_saying`), kept and a note, waiting on the frame that draws it or not.
    pub words: Option<(ToastKind, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum What {
    /// What an invisible act touched (§1): byte ranges of a document at its version — sorted, apart.
    Flash { doc: DocId, version: u64, ranges: Vec<Range<usize>>, act: Act },
    /// A stamp your key pressed (§5), where it's drawn.
    Press(Place),
    /// Claude's fresh writing in a document (§2) — its lines are the document's own (`Document.wet`).
    Wet(DocId),
    /// What came on its own and landed in sight in one frame (§6): a peek, a session list row's state.
    Arrived(Vec<Place>),
}

/// A buffer a run of Claude's rewrote on disk, waiting to take that text (`Editor::run_wrote`, `wet_sync`): its
/// version when the run's result came, the file before and after the run.
#[derive(Clone, Debug)]
pub struct Waiting {
    pub(crate) doc: DocId,
    pub(crate) version: u64,
    path: std::path::PathBuf,
    before: ropey::Rope,
    after: ropey::Rope,
}

impl What {
    /// Whether `other` is a motion of the same thing — the one it takes the place of: a flash on the same document,
    /// a press at the same place.
    fn same(&self, other: &What) -> bool {
        match (self, other) {
            (What::Flash { doc: a, .. }, What::Flash { doc: b, .. }) => a == b,
            (What::Press(a), What::Press(b)) => a == b,
            (What::Wet(a), What::Wet(b)) => a == b,
            // (each landing dries on its own clock: a place landing again leaves the older one — `Editor::arrived`)
            (What::Arrived(_), What::Arrived(_)) => false,
            _ => false,
        }
    }

    /// What the state names its act: a flash's act, a press `stamp`.
    pub fn act(&self) -> &'static str {
        match self {
            What::Flash { act, .. } => act.name(),
            What::Press(_) => "stamp",
            What::Wet(_) => "write",
            What::Arrived(_) => "news",
        }
    }
}

/// Where a stamp is pressed — the place it's drawn, its key (§5: the press time is kept by it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// An edit review's change head: the review's document, the change's index.
    Head { doc: DocId, change: usize },
    /// The turn review rail's file.
    File(std::path::PathBuf),
    /// The chat's knot row of call `id` (and the follow card's, the same call).
    Knot(String),
    /// A card's peek (§6), by the card's id.
    Peek(u64),
    /// A session list row's state word (§6), by its workspace.
    State(crate::event::WorkspaceId),
    /// A session a Claude started, come in the list (its row whole) or on the strip (its name) (§6).
    Row(crate::event::WorkspaceId),
    /// A session's state word on the sessions' strip (§6).
    Strip(crate::event::WorkspaceId),
    /// A session's `•` on the strip: news while it was behind (§6).
    News(crate::event::WorkspaceId),
    /// The status line's `◂ new` (§6).
    Unread,
    /// The status line's `▲N waiting`, its count grown (§6).
    Waiting,
    /// The status line's `N runs quiet`, its count grown (§6).
    Quiet,
    /// A run's tab's badge, turned to its end mark (§6).
    Badge(crate::sizes::Slot),
}

/// The act a flash shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Undo,
    Redo,
    Repeat,
    Yank,
    Paste,
}

impl Act {
    pub fn name(self) -> &'static str {
        match self {
            Act::Undo => "undo",
            Act::Redo => "redo",
            Act::Repeat => "repeat",
            Act::Yank => "yank",
            Act::Paste => "paste",
        }
    }
}

impl Moving {
    /// Its kind, as the state names it: `flash`, `press`.
    pub fn kind(&self) -> &'static str {
        match self.what {
            What::Flash { .. } => "flash",
            What::Press(_) => "press",
            What::Wet(_) => "wet",
            What::Arrived(_) => "arrived",
        }
    }
}

/// The drying's strength at `age` of `life` — one ease-out, `(1 − t)²`, read at whole `TICK` steps (one colour a
/// step, the frames come on them): full on the act's own frame, a quick fall, a long last step, as ink dries.
/// Over 1 s: 1 · .81 · .64 · .49 · .36 · .25 · .16 · .09 · gone; over 0.2 s: 1 · .25 · gone. 0 under `GONE`.
pub fn strength(age: Duration, life: Duration) -> f32 {
    let step = age.as_nanos() / tick().as_nanos() * tick().as_nanos();
    let t = step as f64 / life.as_nanos().max(1) as f64;
    let k = (1.0 - t).max(0.0).powi(2) as f32;
    if k < GONE { 0.0 } else { k }
}

/// How long a motion of `life` shows: to its first step under `GONE`.
pub fn shown_for(life: Duration) -> Duration {
    let mut at = Duration::ZERO;
    while at < life && strength(at, life) > 0.0 {
        at += tick();
    }
    at.min(life)
}

/// A flash's ranges in `text`: sorted, merged where they meet, each on character boundaries; an empty one (what
/// was taken out) the character there — the join —, or the one before at the end. None when there's nothing to
/// show or more than `RANGES_MAX`.
pub fn flash_ranges(text: &ropey::Rope, mut ranges: Vec<Range<usize>>) -> Option<Vec<Range<usize>>> {
    let n = text.len_bytes();
    if ranges.len() > RANGES_MAX || n == 0 {
        return None;
    }
    for r in &mut ranges {
        let start = crate::graphemes::snap(text, r.start.min(n));
        let end = crate::graphemes::snap(text, r.end.min(n)).max(start);
        *r = match start == end {
            true if start < n => start..crate::graphemes::next_boundary(text, start),
            true => crate::graphemes::prev_boundary(text, start)..start,
            false => start..end,
        };
    }
    crate::sorting::by_key(&mut ranges, |r| r.start);
    let mut out: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for r in ranges {
        match out.last_mut() {
            Some(l) if r.start <= l.end => l.end = l.end.max(r.end),
            _ => out.push(r),
        }
    }
    (!out.is_empty()).then_some(out)
}

/// What undo or redo changed, from the text before it to the text after (the one span between their common ends):
/// within one line that span; across lines only the hunks of a line diff of the span's lines — a multi-cursor undo
/// doesn't wash the text it left alone between its cursors. Past `LINES_MAX` lines, none.
pub fn undone(old: &ropey::Rope, new: &ropey::Rope) -> Vec<Range<usize>> {
    let Some(c) = crate::disk::minimal_change(old, new) else { return Vec::new() };
    let (from, to) = (c.from, c.from + c.insert.len());
    let (l0, l1) = (new.byte_to_line(from), new.byte_to_line(to));
    if l0 == l1 {
        return std::iter::once(from..to).collect();
    }
    let (o0, o1) = (old.byte_to_line(c.from), old.byte_to_line(c.to));
    if l1 - l0 > LINES_MAX || o1 - o0 > LINES_MAX {
        return Vec::new();
    }
    // (the span's lines whole, both sides: a line diff of them; each hunk's new lines, or the join where it only took out)
    let line_end = |t: &ropey::Rope, l: usize| t.line_to_byte((l + 1).min(t.len_lines()));
    let (s_old, s_new) = (old.line_to_byte(o0), new.line_to_byte(l0));
    let a = old.byte_slice(s_old..line_end(old, o1)).to_string();
    let b = new.byte_slice(s_new..line_end(new, l1)).to_string();
    let at = |rel: usize| new.line_to_byte((l0 + rel).min(new.len_lines()));
    let mut out = Vec::new();
    for (_, h) in crate::replay::hunks(&a, &b) {
        // (as many lines each way: each line's own span, between what it kept at its ends — else its lines whole)
        if h.removed.len() != h.added.len() {
            out.push(at(h.lines.start)..at(h.lines.end));
            continue;
        }
        for (k, (was, now)) in h.removed.iter().zip(&h.added).enumerate() {
            let pre: usize =
                was.chars().zip(now.chars()).take_while(|(x, y)| x == y).map(|(x, _)| x.len_utf8()).sum();
            let room = was.len().min(now.len()) - pre;
            let suf: usize = was
                .chars()
                .rev()
                .zip(now.chars().rev())
                .take_while(|(x, y)| x == y)
                .map(|(x, _)| x.len_utf8())
                .scan(0, |n, l| {
                    *n += l;
                    (*n <= room).then_some(l)
                })
                .sum();
            let start = at(h.lines.start + k);
            out.push(start + pre..start + now.len() - suf);
        }
    }
    out
}

/// The lines a write left, from the text before it to the text after (the one span between their common ends): whole
/// lines as byte ranges of `new` — within one line that line; across lines each hunk's new lines of a line diff of the
/// span's lines, what it only took out none (no ink left). Past `LINES_MAX` lines, or `RANGES_MAX` hunks, none.
pub fn written(old: &ropey::Rope, new: &ropey::Rope) -> Vec<Range<usize>> {
    let Some(c) = crate::disk::minimal_change(old, new).filter(|c| !c.insert.is_empty()) else {
        return Vec::new();
    };
    let line = |l: usize| new.line_to_byte(l.min(new.len_lines()));
    let (from, to) = (c.from, c.from + c.insert.len());
    let (l0, l1) = (new.byte_to_line(from), new.byte_to_line(to));
    if l0 == l1 {
        return std::iter::once(line(l0)..line(l0 + 1)).filter(|r| !r.is_empty()).collect();
    }
    let (o0, o1) = (old.byte_to_line(c.from), old.byte_to_line(c.to));
    if l1 - l0 > LINES_MAX || o1 - o0 > LINES_MAX {
        return Vec::new();
    }
    let line_end = |t: &ropey::Rope, l: usize| t.line_to_byte((l + 1).min(t.len_lines()));
    let a = old.byte_slice(old.line_to_byte(o0)..line_end(old, o1)).to_string();
    let b = new.byte_slice(new.line_to_byte(l0)..line_end(new, l1)).to_string();
    let mut out: Vec<Range<usize>> = Vec::new();
    for (_, h) in crate::replay::hunks(&a, &b).into_iter().filter(|(_, h)| !h.lines.is_empty()) {
        let r = line(l0 + h.lines.start)..line(l0 + h.lines.end);
        match out.last_mut() {
            Some(l) if r.start <= l.end => l.end = l.end.max(r.end),
            _ if r.is_empty() => {}
            _ => out.push(r),
        }
    }
    if out.len() > RANGES_MAX { Vec::new() } else { out }
}

/// Whether macOS's Reduce Motion is on: `defaults read com.apple.universalaccess reduceMotion` says `1` (the
/// key it sets — measured on macOS 27, 2026-10-21: `0` with it off). A worker's: it runs `defaults`. Never in
/// tests; off macOS, no.
pub fn reduce_motion() -> bool {
    if cfg!(test) || !cfg!(target_os = "macos") {
        return false;
    }
    std::process::Command::new("/usr/bin/defaults")
        .args(["read", "com.apple.universalaccess", "reduceMotion"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "1")
}
