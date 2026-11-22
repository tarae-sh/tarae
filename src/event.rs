//! Event queue — key input, config changes and job results all arrive through one channel.
//!
//! Principle 4 (non-blocking): slow work runs on a thread via `Jobs::spawn`, and the result comes back as
//! "a closure to apply to the editor" that runs on the main loop. The main thread never blocks except
//! while waiting on the channel.
//! Zero dependencies — std threads + mpsc. (An async runtime only if LSP/LLM streaming proves it necessary.)

use std::panic::Location;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::editor::Editor;
use crate::key::Key;

pub type Apply = Box<dyn FnOnce(&mut Editor) + Send>;

pub enum Event {
    Key(Key),
    Mouse(Mouse),
    /// The mouse moved, no button down (column, row) — not input: it moves nothing but what's under it says
    /// (`presence::hovered`: a session's tag over its mark).
    Hover(u16, u16),
    /// Terminal focus (regained = true).
    Focus(bool),
    Resize,
    /// Text pasted into the terminal tarae runs in (bracketed paste) — one event, not a key per character.
    Paste(String),
    Job(Apply),
    /// A job's result for a workspace and the session at work there that it's of (`Tx` makes these): applied in that
    /// workspace (`Editor::with_workspace`).
    For(WorkspaceId, SessionId, Apply),
    /// Input from an interface that presents late (`interface::Facts::presents_late`): the last frame it presented
    /// as it sent `event` (the remote plan's `Input.seen`) — what the "only once drawn" rules judge it by
    /// (`Editor::frame_presented`, `Editor::saw`). The terminal's input comes bare: its frames are presented as
    /// they're written. `seq`: the interface's own count of its inputs, said back with each frame drawn after it
    /// (`Frame.input`; 0 for one that doesn't count them).
    Input {
        seen: crate::interface::Frame,
        seq: u64,
        event: Box<Event>,
    },
}

/// One mouse action (screen cell coordinates). Press, drag, release, wheel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mouse {
    pub kind: MouseKind,
    pub x: u16,
    pub y: u16,
    /// alt+press = add a cursor.
    pub alt: bool,
    /// ctrl+press = go there: a link followed, a name to its definition, an http(s) address opened as a page (`gf`)
    /// (where the terminal sends it — macOS's
    /// Terminal.app and iTerm2 can make it a right click, which tarae doesn't take).
    pub ctrl: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseKind {
    Down,
    Drag,
    Up,
    ScrollUp,
    ScrollDown,
    /// The wheel sideways (a trackpad, a tilt wheel).
    ScrollLeft,
    ScrollRight,
    /// The vertical wheel with Shift, as terminals send it (SGR 64/65 with Shift's bit, which crossterm reads as the
    /// vertical wheel): sideways over a code pane (`Editor::handle_mouse`), else the vertical wheel — a terminal's
    /// program that asked for the mouse gets it as it came.
    ShiftScrollUp,
    ShiftScrollDown,
    /// The middle button's press: the clipboard pasted at the pointer, on text that takes it (`Editor::middle_paste`).
    Middle,
}

impl MouseKind {
    /// The kind without Shift: the vertical wheel for Shift's, else itself.
    pub fn unshifted(self) -> MouseKind {
        match self {
            MouseKind::ShiftScrollUp => MouseKind::ScrollUp,
            MouseKind::ShiftScrollDown => MouseKind::ScrollDown,
            k => k,
        }
    }
}

/// Which workspace a result belongs to (`workspace.rs`).
pub type WorkspaceId = Id<OfWorkspace>;

/// Which session — a participant (`session.rs`).
pub type SessionId = Id<OfSession>;

/// What an `Id` names: a workspace or a session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OfWorkspace;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OfSession;

/// A workspace's or a session's id: the machine it was made on and its number there (collab §4: unique across
/// machines — a second machine's never reuses this one's). This machine's is 0, numbered in the order they're
/// made from 0, the checkout tarae started in; said by its number alone (`:workspace-switch 2`, `s2`), another
/// machine's as `2@3`.
#[derive(Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id<K> {
    pub machine: u32,
    pub n: u64,
    of: std::marker::PhantomData<K>,
}

impl<K> Clone for Id<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for Id<K> {}

impl<K> Id<K> {
    /// This machine's `n`th.
    pub const fn here(n: u64) -> Id<K> {
        Id { machine: 0, n, of: std::marker::PhantomData }
    }

    /// The checkout tarae started in (its Claude, for a session).
    pub const HOME: Id<K> = Id::here(0);

    /// The one made after it, on the same machine.
    pub fn next(self) -> Id<K> {
        Id { n: self.n + 1, ..self }
    }

    /// The same machine and number, naming the other kind — a workspace's Claude is born with it (`session::born_with`).
    pub fn twin<J>(self) -> Id<J> {
        Id { machine: self.machine, n: self.n, of: std::marker::PhantomData }
    }
}

impl<K> std::fmt::Display for Id<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.machine {
            0 => write!(f, "{}", self.n),
            m => write!(f, "{}@{m}", self.n),
        }
    }
}

impl<K> std::str::FromStr for Id<K> {
    type Err = std::num::ParseIntError;

    /// As `Display` says it: `2`, `2@3`.
    fn from_str(s: &str) -> Result<Id<K>, Self::Err> {
        let (n, machine) = s.split_once('@').map_or((s, None), |(n, m)| (n, Some(m)));
        let machine = machine.map(str::parse).transpose()?.unwrap_or(0);
        Ok(Id { machine, n: n.parse()?, of: std::marker::PhantomData })
    }
}

/// The queue the editor holds. The sending side is cloned and handed out to threads.
pub struct Queue {
    tx: Sender<Event>,
    rx: Receiver<Event>,
    /// The workspace in front and its session — what `sender` and `jobs` tag their results with (set on a switch, and
    /// while a result for a workspace behind is applied).
    at: std::cell::Cell<(WorkspaceId, SessionId)>,
    /// The user's input (its `input_seq`) whose handling is under way — or a job's result it started, through
    /// jobs it started in turn (`Jobs` carries it): what a card it makes was asked by (#205).
    asked: std::cell::Cell<Option<u64>>,
    /// Writes under way the process's end waits for (`Editor::writes_flush`): every `background` job, and what a
    /// spawned job writes that an answer is kept in (`Writes::start`).
    writes: Writes,
    /// What runs now is the start's (`Starts`): from the queue's making until the first frame (`started`), and while
    /// the result of a job of the start's is applied — what it spawns is the start's too. Off in tests: an editor
    /// made in-process has no start.
    starting: std::cell::Cell<bool>,
    /// While the result of a job of the start's is applied: since when its chain is out — what it spawns is out as
    /// long (a poll that respawns from its own result ages as one job, so it's said late once past `START_LATE`).
    chain: std::cell::Cell<Option<Instant>>,
    /// The start's jobs whose results haven't landed.
    starts: Starts,
    /// Every job whose work is under way or whose result hasn't landed, the start's or not (`owed_out`).
    owed: Starts,
    /// The simulation it was made in (`sim.rs`): what its senders send goes there.
    #[cfg(test)]
    sim: Option<crate::sim::Handle>,
}

impl Default for Queue {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            at: Default::default(),
            asked: Default::default(),
            writes: Writes::default(),
            starting: std::cell::Cell::new(!cfg!(test)),
            chain: Default::default(),
            starts: Starts::default(),
            owed: Starts::default(),
            #[cfg(test)]
            sim: crate::sim::current(),
        }
    }
}

impl Queue {
    /// A sender whose results are applied in the workspace in front now, wherever the user is when they land.
    pub fn sender(&self) -> Tx {
        self.tx_for(Some(self.at.get()))
    }

    fn tx_for(&self, to: Option<(WorkspaceId, SessionId)>) -> Tx {
        Tx {
            tx: self.tx.clone(),
            to,
            #[cfg(test)]
            sim: self.sim.clone(),
        }
    }

    /// A sender whose results are applied as they come, in whatever session is in front then — for what every
    /// session shares (language servers, the saver, input, the config watch, plugins).
    pub fn shared_sender(&self) -> Tx {
        self.tx_for(None)
    }

    pub fn jobs(&self) -> Jobs {
        Jobs {
            tx: self.sender(),
            asked: self.asked.get(),
            writes: self.writes.clone(),
            start: self.start(),
            owed: self.owed.clone(),
        }
    }

    /// The start's count, for a job made now, with the chain it's out in (`chain`) — None once the start is over.
    fn start(&self) -> Option<(Starts, Option<Instant>)> {
        self.starting.get().then(|| (self.starts.clone(), self.chain.get()))
    }

    /// The start's count for a watch's first look (`git::watch`, `config::watch` — a thread, not a job: `Jobs::spawn`
    /// counts those by itself): what it finds is sent unasked, so it's out until that result is applied (the count
    /// dropped with it) or the look found nothing to send (#210: the git watch's first round, its repository resolved
    /// first, landed in a settled tarae's rest on a loaded Mac; again, the trust scan after it — its `git ls-files`
    /// polled). None once the start is over.
    #[track_caller]
    pub fn start_out(&self) -> Option<StartOut> {
        let site = Location::caller();
        self.start().map(|(s, chain)| s.out(site, chain))
    }

    /// The start is over (its first frame drawn): what runs from now on is the start's only as a result of one of
    /// its jobs (`Queue.starting`).
    pub fn started(&self) {
        self.starting.set(false);
    }

    /// What runs from now on is the start's (`on`) or not — a test's editor, as the binary's is until its first frame.
    #[cfg(test)]
    pub fn set_starting(&self, on: bool) {
        self.starting.set(on);
    }

    /// The start's jobs whose results haven't landed, each by the line that spawned it and how long it's been out
    /// (the state's `starting`).
    pub fn starts_out(&self) -> Vec<(&'static Location<'static>, Duration)> {
        self.starts.listed()
    }

    /// Every job whose work is under way — or done, its result not landed yet — by the line that spawned it and how
    /// long it's been out (the state's `owed`): the start's or not, but never one whose work waits (`waits`: a timer,
    /// a watch's round). What an idle check waits out besides the start's (#398: on a loaded Mac a job the chat's
    /// opening spawned landed in a settled tarae's rest).
    pub fn owed_out(&self) -> Vec<(&'static Location<'static>, Duration)> {
        self.owed.listed()
    }

    /// The writes under way the process's end waits for.
    pub fn writes(&self) -> &Writes {
        &self.writes
    }

    /// Jobs whose results are applied in whatever session is in front when they land — work about sessions
    /// themselves (making, ending one: `worktree.rs`), never swapped into the session it's about.
    pub fn shared_jobs(&self) -> Jobs {
        Jobs {
            tx: self.shared_sender(),
            asked: self.asked.get(),
            writes: self.writes.clone(),
            start: self.start(),
            owed: self.owed.clone(),
        }
    }

    /// Results made from now on are for workspace `w` and its session `s`.
    pub fn set_workspace(&self, w: WorkspaceId, s: SessionId) {
        self.at.set((w, s));
    }

    /// The user's input that what runs now was asked by (`Queue.asked`) — None: nobody's just now (a task's
    /// output, a server, a timer).
    pub fn asked(&self) -> Option<u64> {
        self.asked.get()
    }

    /// What runs from now on was asked by input `n` (None: by nobody); what it was before.
    pub fn set_asked(&self, n: Option<u64>) -> Option<u64> {
        self.asked.replace(n)
    }

    pub fn recv(&self) -> Option<Event> {
        self.rx.recv().ok()
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }

    /// Like `recv`, but gives up after `d`: Ok(None) when it passed with nothing (Err: the queue is closed).
    pub fn recv_within(&self, d: std::time::Duration) -> Result<Option<Event>, ()> {
        match self.rx.recv_timeout(d) {
            Ok(ev) => Ok(Some(ev)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(()),
        }
    }

    #[cfg(test)]
    pub fn recv_timeout(&self, d: std::time::Duration) -> Option<Event> {
        self.rx.recv_timeout(d).ok()
    }
}

/// The sending side: a job's result sent through it is applied in the workspace it was made for, tagged with its
/// session too (`None`: the one in front when it lands).
#[derive(Clone)]
pub struct Tx {
    tx: Sender<Event>,
    to: Option<(WorkspaceId, SessionId)>,
    #[cfg(test)]
    sim: Option<crate::sim::Handle>,
}

impl Tx {
    #[track_caller]
    pub fn send(&self, ev: Event) -> Result<(), mpsc::SendError<Event>> {
        self.send_from(ev, Location::caller())
    }

    /// `send`, a job's result named by `site` — where it was sent from, or where the job that sent it was
    /// spawned: what a frame it draws was drawn for (`Editor.event_from`, the state's `frames_for`).
    fn send_from(&self, ev: Event, site: &'static Location<'static>) -> Result<(), mpsc::SendError<Event>> {
        let ev = match ev {
            Event::Job(apply) => Event::Job(Box::new(move |ed: &mut Editor| {
                ed.stall.refine(crate::stall::Label::Job(site));
                ed.event_from = Some(crate::editor::DrawnFor::Job(site));
                apply(ed)
            })),
            ev => ev,
        };
        let ev = match (ev, self.to) {
            (Event::Job(apply), Some((w, s))) => Event::For(w, s, apply),
            (ev, _) => ev,
        };
        // (in a simulation: its inbox, landed when the seed says — and a sender made outside it, used in it, is
        // refused: what it sends would never land)
        #[cfg(test)]
        if let Some(sim) = &self.sim {
            crate::sim::send(sim, ev);
            return Ok(());
        }
        #[cfg(test)]
        crate::sim::stray();
        self.tx.send(ev)
    }
}

#[derive(Clone)]
pub struct Jobs {
    tx: Tx,
    /// The input it was started under (`Queue.asked`): its result is applied as asked by it too — one-shot
    /// work only; a long-lived sender (a task's output, a server's) carries none.
    asked: Option<u64>,
    writes: Writes,
    /// Made while the start ran (`Queue.starting`): its jobs are the start's, counted until their results land —
    /// out since the chain they're in went out (`Queue.chain`), else since their spawn.
    start: Option<(Starts, Option<Instant>)>,
    /// Every job's count until its result lands (`Queue.owed`).
    owed: Starts,
}

impl Jobs {
    /// Runs `work` on a thread and sends its result (a closure to apply to the editor) to the main loop.
    #[track_caller]
    pub fn spawn<W, A>(&self, work: W)
    where
        W: FnOnce() -> A + Send + 'static,
        A: FnOnce(&mut Editor) + Send + 'static,
    {
        self.spawn_boxed(Location::caller(), Box::new(move || Box::new(work()) as Apply));
    }

    /// `spawn`'s work, one copy for every job: a job's closures are generic, so what's in `spawn` is copied at each
    /// of its call sites (~545) — there only its work and its result are boxed (#365: with the start's bookkeeping
    /// and the result's wrapper in the generic closures, two copies a site, the release binary was 0.26 MB larger).
    /// Never inlined into them, for the same reason; a call and two boxes more a job, beside the thread it starts.
    #[inline(never)]
    fn spawn_boxed(&self, site: &'static Location<'static>, work: Box<dyn FnOnce() -> Apply + Send>) {
        let (tx, asked) = (self.tx.clone(), self.asked);
        // (counted until its result is applied — or dropped unapplied, or its work panics, or waits: `waits`)
        let out = self.start.as_ref().map(|(s, since)| s.out(site, *since));
        let owed = self.owed.out(site, None);
        run(
            site,
            Box::new(move || {
                let working = Working::with(out, owed);
                let apply = work();
                let (out, owed) = working.done();
                let apply = move |ed: &mut Editor| {
                    let asked = asked.map(|_| ed.events.set_asked(asked));
                    // (what a result of the start's spawns is the start's too, out since this one's chain)
                    let starting = out
                        .as_ref()
                        .map(|o| (ed.events.starting.replace(true), ed.events.chain.replace(Some(o.2))));
                    apply(ed);
                    if let Some((was, chain)) = starting {
                        ed.events.starting.set(was);
                        ed.events.chain.set(chain);
                    }
                    if let Some(was) = asked {
                        ed.events.set_asked(was);
                    }
                    drop((out, owed));
                };
                let _ = tx.send_from(Event::Job(Box::new(apply)), site);
            }),
        );
    }

    /// `spawn` for work that writes what's kept (an answer, a store): the process's end waits for the work, as for a
    /// `background` job's — its result isn't waited for (the loop has ended).
    #[track_caller]
    pub fn spawn_kept<W, A>(&self, work: W)
    where
        W: FnOnce() -> A + Send + 'static,
        A: FnOnce(&mut Editor) + Send + 'static,
    {
        let writing = self.writes.start();
        self.spawn(move || {
            let apply = work();
            drop(writing);
            apply
        });
    }

    /// Runs `work` on a thread with nothing to bring back (a state file saved, history written): no result, so
    /// the main loop isn't woken for it — a result, even one that does nothing, is an event and a frame. The
    /// process's end waits for it (`Writes`, `EXIT_WAIT` at most): what it writes isn't cut off by a quit.
    #[track_caller]
    pub fn background(&self, work: impl FnOnce() + Send + 'static) {
        let writing = self.writes.start();
        self.unanswered(
            Location::caller(),
            Box::new(move || {
                let _writing = writing;
                work()
            }),
        );
    }

    /// `background` the process's end doesn't wait for: a timer, a clean-up a quit may cut short (the start's
    /// prunes, a commit's tree removed) — nothing kept is lost with it.
    #[track_caller]
    pub fn detached(&self, work: impl FnOnce() + Send + 'static) {
        self.unanswered(Location::caller(), Box::new(work));
    }

    /// `background`'s and `detached`'s work: the start's while it runs (`Working`) — no result lands to end its count,
    /// so its work's end does, or its first wait (`waits`: a timer's). #398: the start's prune of the temp folder ran
    /// on into a settled tarae's rest on a loaded Mac, in no `starting`. One copy for both, as `spawn_boxed` (#365).
    #[inline(never)]
    fn unanswered(&self, site: &'static Location<'static>, work: Box<dyn FnOnce() + Send>) {
        let out = self.start.as_ref().map(|(s, since)| s.out(site, *since));
        let owed = self.owed.out(site, None);
        run(
            site,
            Box::new(move || {
                let _working = Working::with(out, owed);
                work()
            }),
        );
    }
}

/// A wait: `thread::sleep` for a time that has to come — a timer (an idle stop, a nudge, a frame's tick, a draft's
/// limit, idle compaction), a deadline or a cap (a server's answer, a format), a quiet given to someone else (the
/// servers, a tree before its overlap round, the shared server's gathering), a look again later (the shared
/// server's watch). A job of the start's that waits lets go of its count (`waits`): what a wait ends in isn't the
/// start's work, however soon. Its kind is the call's, never its length (#201's fifth round: the shared server's
/// timers were 1 s in release and 20 ms in tests, a wait in one and a pause in the other). The one sleep with
/// `pause` — nothing else in tarae calls `thread::sleep`.
pub fn sleep(d: Duration) {
    waits();
    nap(d);
}

/// A timer that stands down with what it guards: `sleep` waits a time, as `event::sleep` does (a simulation moves
/// its clock instead), unless a `ring` comes first — a code program's watchdog ends the moment its step does, so no
/// sleeping thread outlives the step it watched.
#[derive(Default)]
pub struct Wake {
    /// Rings so far · sleepers no ring has woken yet.
    state: std::sync::Mutex<(u64, usize)>,
    cv: std::sync::Condvar,
}

impl Wake {
    fn state(&self) -> std::sync::MutexGuard<'_, (u64, usize)> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Every sleep under way (and one about to start, given an older count) ends now.
    pub fn ring(&self) {
        let mut s = self.state();
        s.0 += 1;
        s.1 = 0;
        drop(s);
        self.cv.notify_all();
        #[cfg(test)]
        crate::sim::ring(self as *const Wake as usize);
    }

    /// Rings so far: what `sleep` is given, read before the look that decided to sleep.
    pub fn rings(&self) -> u64 {
        self.state().0
    }

    /// Sleepers no ring has woken (tests: a timer left behind).
    #[cfg(test)]
    pub fn asleep(&self) -> usize {
        self.state().1
    }

    /// Waits `d`, or until a ring past `seen` (`rings`) — at once if one came already. A spurious end is possible:
    /// the caller looks again.
    pub fn sleep(&self, d: Duration, seen: u64) {
        waits();
        let mut s = self.state();
        if s.0 != seen {
            return;
        }
        s.1 += 1;
        #[cfg(test)]
        {
            drop(s);
            if crate::sim::doze(d, self as *const Wake as usize) {
                let mut s = self.state();
                if s.0 == seen {
                    s.1 -= 1;
                }
                return;
            }
            s = self.state();
            if s.0 != seen {
                return;
            }
        }
        let (mut s, _) = self.cv.wait_timeout_while(s, d, |s| s.0 == seen).unwrap_or_else(|p| p.into_inner());
        if s.0 == seen {
            s.1 -= 1;
        }
    }
}

/// A pause in the work: holds back tarae's own work under way — a debounce before it (the gutter's diff, blame,
/// inlay hints, a live search, a plugin's changed text), a poll's step while it finishes (a process, a lock, a file
/// settling, a busy server asked again). Its job stays the start's, so a settle waits for what it then does
/// (#201's fourth round: the gutter's hunks at the start); a poll that goes on is said late once past
/// `START_LATE`, its respawns too (`Queue.chain`).
pub fn pause(d: Duration) {
    nap(d);
}

/// In a simulation (test builds) a job yields until the simulated clock reaches its wake, and other threads run
/// meanwhile.
fn nap(d: Duration) {
    #[cfg(test)]
    if crate::sim::sleep(d) {
        return;
    }
    thread::sleep(d);
}

/// How long the process's end waits for the writes under way (a hung disk isn't waited for forever — what it
/// was writing is lost).
pub const EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Writes under way on workers, counted.
#[derive(Clone, Default)]
pub struct Writes(std::sync::Arc<(std::sync::Mutex<usize>, std::sync::Condvar)>);

/// One write under way — counted until dropped (a worker that panics, too).
pub struct Writing(Writes);

impl Writes {
    /// One more under way, until the returned `Writing` goes.
    pub fn start(&self) -> Writing {
        *self.0.0.lock().unwrap_or_else(|p| p.into_inner()) += 1;
        Writing(self.clone())
    }

    /// Until none is under way, or `within` passes — whether they all finished.
    pub fn wait(&self, within: std::time::Duration) -> bool {
        let (lock, cv) = &*self.0;
        let n = lock.lock().unwrap_or_else(|p| p.into_inner());
        let (n, _) = cv.wait_timeout_while(n, within, |n| *n > 0).unwrap_or_else(|p| p.into_inner());
        *n == 0
    }
}

impl Drop for Writing {
    fn drop(&mut self) {
        let (lock, cv) = &*self.0.0;
        *lock.lock().unwrap_or_else(|p| p.into_inner()) -= 1;
        cv.notify_all();
    }
}

/// The start's jobs whose results haven't landed, each by the line that spawned it and since when: what tarae
/// spawned before its first frame (the sessions' restore, recovery's look, the languages' loads, Reduce Motion's read
/// …) and what their results spawned in turn, and its work with no result while that runs (the prunes —
/// `background`, `detached`) — but never one whose work waits (`waits`) — the state's `starting`, which an idle check
/// waits out (#191: on a loaded machine the restore's result landed in a settled tarae's rest; #398: the temp
/// folder's prune ran on into one).
#[derive(Clone, Default)]
pub struct Starts(std::sync::Arc<std::sync::Mutex<Vec<(&'static Location<'static>, Instant)>>>);

/// One of the start's jobs out — counted until dropped.
pub struct StartOut(Starts, &'static Location<'static>, Instant);

/// A job of the start's out this long is said with its time in the state's `starting` (`uistate.rs`): one that
/// should have landed by now — stuck, or waiting on something that isn't the start's work.
pub const START_LATE: Duration = Duration::from_secs(5);

impl Starts {
    /// One more out from `site`, since `chain` (what spawned it is out since then) or now.
    fn out(&self, site: &'static Location<'static>, chain: Option<Instant>) -> StartOut {
        let since = chain.unwrap_or_else(crate::clock::now);
        self.0.lock().unwrap_or_else(|p| p.into_inner()).push((site, since));
        StartOut(self.clone(), site, since)
    }

    /// Each one out, by its site and how long.
    fn listed(&self) -> Vec<(&'static Location<'static>, Duration)> {
        let out = self.0.lock().unwrap_or_else(|p| p.into_inner());
        out.iter().map(|(site, since)| (*site, crate::clock::since(*since))).collect()
    }
}

impl Drop for StartOut {
    fn drop(&mut self) {
        let mut out = self.0.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(i) = out.iter().position(|s| *s == (self.1, self.2)) {
            out.remove(i);
        }
    }
}

thread_local! {
    /// The job whose work this thread runs now (`Jobs::spawn`): its count as the start's, and as owed (`Queue.owed`)
    /// — until it waits (`waits`).
    static WORKING: std::cell::RefCell<(Option<StartOut>, Option<StartOut>)> =
        const { std::cell::RefCell::new((None, None)) };
}

/// A job's work under way on this thread, holding its counts (`WORKING`) — let go if it panics.
struct Working;

impl Working {
    fn with(out: Option<StartOut>, owed: StartOut) -> Working {
        WORKING.with(|w| *w.borrow_mut() = (out, Some(owed)));
        Working
    }

    /// The work done: still the start's, and owed, unless it waited.
    fn done(self) -> (Option<StartOut>, Option<StartOut>) {
        WORKING.with(|w| std::mem::take(&mut *w.borrow_mut()))
    }
}

impl Drop for Working {
    fn drop(&mut self) {
        let _ = WORKING.try_with(|w| std::mem::take(&mut *w.borrow_mut()));
    }
}

/// The job whose work runs on this thread waits — on time (`sleep`: a timer) or for something to happen (a watch's
/// next round, `fswatch::Watcher::wait`, however soon it comes — a watch's wait has no end of its own): from here
/// it's never the start's, nor is what its result spawns, whoever spawned it (#201: the usage watch, armed by the
/// restore's result, held the start open up to an hour). One rule at the wait itself, so no timer or watch has to
/// remember to leave: every sleep in the code is `sleep` or `pause`
/// (`every_sleep_in_the_code_goes_through_event_sleep`) — a wait that's neither time nor a watch (a channel, a
/// process), or a poll of `pause`s that goes on (respawning from its own result too: `Queue.chain`), shows in the
/// state's `starting` once past `START_LATE`.
pub fn waits() {
    let _ = WORKING.try_with(|w| drop(std::mem::take(&mut *w.borrow_mut())));
}

impl Editor {
    /// Before the process ends (quitting, `:restart`'s exec): the writes still under way land (`Writes`) —
    /// false: some didn't within `EXIT_WAIT`.
    pub fn writes_flush(&self) -> bool {
        self.events.writes().wait(EXIT_WAIT)
    }

    /// What a quit says it didn't write (`flushed`: `writes_flush`'s answer) — every store's writes not landed,
    /// cut off past the wait or failed (trust's, what projects keep, the sessions'), and a take-back of a session's
    /// given before the kept sessions were back.
    pub fn unwritten(&self, flushed: bool) -> Vec<String> {
        let stores: Vec<std::path::PathBuf> = [self.trust_path.clone(), self.allow_path.clone()]
            .into_iter()
            .flatten()
            .chain(self.workspaces_path())
            .collect();
        let stores: Vec<&std::path::Path> = stores.iter().map(std::path::PathBuf::as_path).collect();
        let before = self.workspace_store.taken.iter().map(|(_, w)| format!("the sessions kept without {w}"));
        let said = crate::storewrite::pending_said(&stores).into_iter().chain(before);
        said.map(|said| match flushed {
            true => format!("tarae: not written: {said}"),
            false => format!(
                "tarae: not written before quitting ({} waited): {said}",
                crate::term::run_duration(EXIT_WAIT)
            ),
        })
        .collect()
    }
}

/// Lets `x` go on a worker — a big file's parse trees take milliseconds to free (`editor.rs`'s, perf
/// profile: 4.6 ms the main tree, 1.1 ms its layers — and those go at every parse). No result comes back.
#[track_caller]
pub fn drop_later<T: Send + 'static>(x: T) {
    #[cfg(test)]
    DROPPED_LATER.with(|n| n.set(n.get() + 1));
    run(Location::caller(), Box::new(move || drop(x)));
}

#[cfg(test)]
thread_local! {
    /// How many `drop_later`s this thread asked for (tests: what a test's own editor handed off).
    pub static DROPPED_LATER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The bytes of a thread's name Linux keeps (the rest is cut: `top -H` and a profiler see only these).
pub const THREAD_NAME_MAX: usize = 15;

/// A thread name of tarae's fits `THREAD_NAME_MAX` (checked in debug builds, where every thread started says so).
pub fn check_thread_name(name: &str) {
    debug_assert!(
        name.len() <= THREAD_NAME_MAX,
        "{name}: Linux keeps {THREAD_NAME_MAX} bytes of a thread's name"
    );
}

/// A thread of tarae's, by name — `tarae-<what>`, at most `THREAD_NAME_MAX` bytes — so `top -H`, a profiler
/// or `tarae-trace` tells tarae's threads apart (unnamed, every one is called `tarae`).
/// In a simulation it's the step's own while it lives (`sim::helper`): refused once it outlives that step.
#[track_caller]
pub fn thread<T: Send + 'static>(
    name: &str,
    f: impl FnOnce() -> T + Send + 'static,
) -> thread::JoinHandle<T> {
    check_thread_name(name);
    #[cfg(test)]
    let f = {
        let helper = crate::sim::helper(Location::caller());
        move || {
            if let Some(h) = &helper {
                h.enter();
            }
            f()
        }
    };
    thread::Builder::new().name(name.into()).spawn(f).expect("failed to spawn thread")
}

/// A scoped thread of tarae's, by name, as `thread`. A scope's own `spawn` starts it unnamed: it shows its
/// creator's name — `tarae` when that's the main thread or a thread still naming itself — and the e2e test
/// (`every_thread_tarae_starts_has_a_name`) caught such ones now and then (1 run in 5 on Debian).
pub fn scoped<'scope, T: Send + 'scope>(
    s: &'scope thread::Scope<'scope, '_>,
    name: &str,
    f: impl FnOnce() -> T + Send + 'scope,
) -> thread::ScopedJoinHandle<'scope, T> {
    check_thread_name(name);
    thread::Builder::new().name(name.into()).spawn_scoped(s, f).expect("failed to spawn thread")
}

/// One copy of the thread-spawn machinery for every job, not one per job type. `site`: where it was spawned —
/// what a simulation's schedule names it by.
fn run(site: &'static Location<'static>, job: Box<dyn FnOnce() + Send>) {
    #[cfg(not(test))]
    {
        let _ = site;
        thread("tarae-job", job);
    }
    // Tests: the worker carries the test's name (libtest names its threads after them), so a failure on
    // it says which test it ran for
    #[cfg(test)]
    if let Some(job) = crate::sim::spawn_job(site, job) {
        // (what the test asks for of the sandbox ends with it, its workers' asks too — #643)
        #[cfg(target_os = "linux")]
        crate::sandbox::keeper::arm_test_end();
        let name = thread::current().name().map(str::to_string);
        let worker = name.map_or_else(thread::Builder::new, |n| thread::Builder::new().name(n));
        let job = move || {
            WORKER.set(true);
            job()
        };
        worker.spawn(job).expect("failed to spawn thread");
    }
}

#[cfg(test)]
thread_local! {
    /// Tests: this thread is a job's worker (`run`) — named after its test, as the test's own thread is.
    pub static WORKER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Work with nothing to bring back (a state file saved) sends nothing: the main loop isn't woken, and no
    /// frame is drawn for it — a spawned job's result, even an empty one, is an event.
    #[test]
    fn background_work_sends_nothing_back() {
        let q = Queue::default();
        let (done, ran) = mpsc::channel();
        q.jobs().background(move || {
            let _ = done.send(());
        });
        ran.recv_timeout(crate::test_results::PATIENCE).expect("it ran");
        assert!(q.recv_timeout(Duration::from_millis(100)).is_none(), "nothing came back");
        q.jobs().spawn(|| |_: &mut Editor| {});
        assert!(q.recv_timeout(crate::test_results::PATIENCE).is_some(), "a spawned job's result does");
    }

    /// Waiting ends when the last write under way does — on another thread, or panicking.
    #[test]
    fn waits_for_the_writes_under_way() {
        let writes = Writes::default();
        assert!(writes.wait(Duration::ZERO), "nothing under way");
        let (a, b) = (writes.start(), writes.start());
        assert!(!writes.wait(Duration::from_millis(10)));
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            drop(a);
        });
        let _ = std::thread::spawn(move || {
            let _b = b;
            panic!("a worker that panics still counts down");
        })
        .join();
        assert!(writes.wait(Duration::from_secs(5)));
    }

    /// A `background` job holds the process's end until it's done; a `detached` one doesn't.
    #[test]
    fn the_end_waits_for_background_work_not_detached() {
        let q = Queue::default();
        let (go, wait) = mpsc::channel::<()>();
        q.jobs().detached(move || {
            let _ = wait.recv_timeout(crate::test_results::PATIENCE);
        });
        assert!(q.writes().wait(Duration::ZERO), "a detached job held it");
        let (go2, wait2) = mpsc::channel::<()>();
        q.jobs().background(move || {
            let _ = wait2.recv_timeout(crate::test_results::PATIENCE);
        });
        assert!(!q.writes().wait(Duration::from_millis(20)), "a background job didn't hold it");
        let _ = (go.send(()), go2.send(()));
        assert!(q.writes().wait(Duration::from_secs(5)));
    }

    /// Every thread tarae starts has a name: `thread(…)`, or a `Builder` given one — the e2e test
    /// (`every_thread_tarae_starts_has_a_name`, Linux) sees only the threads its scenario starts. A thread
    /// started from the main thread without one is called `tarae` like it; from another, that one's name —
    /// but only once that one has named itself, so a scope's own `spawn` counts as unnamed too (`scoped`
    /// names one: unnamed, they made the e2e test flake). And every name given as `thread("…")` or
    /// `scoped(s, "…")` fits `THREAD_NAME_MAX` — the assert only sees threads that start.
    #[test]
    fn every_thread_in_the_code_is_named() {
        let files = crate::sim::lint::code_files().into_iter().filter(|(_, p)| !p.ends_with("tests.rs"));
        let (mut unnamed, mut long) = (Vec::new(), Vec::new());
        for (key, f) in files {
            let text = std::fs::read_to_string(&f).unwrap();
            // (a file's own tests start at its top-level `#[cfg(test)] mod`)
            let code = text.split("\n#[cfg(test)]\nmod ").next().unwrap_or_default();
            // (the names `thread::scope(|s| …)` gives its scope in this file: `s.spawn(` is an unnamed thread)
            let scopes: Vec<String> = code
                .match_indices("thread::scope(|")
                .filter_map(|(j, m)| {
                    code[j + m.len()..].split('|').next().map(|s| format!("{}.spawn(", s.trim()))
                })
                .collect();
            let lines: Vec<&str> = code.lines().collect();
            for (i, &line) in lines.iter().enumerate() {
                let at = || format!("{key}:{}", i + 1);
                let builder = line.contains("thread::Builder::new()")
                    && !lines[i..(i + 4).min(lines.len())].concat().contains(".name(");
                let scope_spawn = scopes.iter().any(|s| line.contains(s.as_str()));
                if line.contains("thread::spawn(") || builder || scope_spawn {
                    unnamed.push(at());
                }
                let given = line.match_indices("thread(\"").chain(line.match_indices("scoped(s, \""));
                for (_, rest) in given.map(|(j, m)| line.split_at(j + m.len())) {
                    let name = rest.split('"').next().unwrap_or_default();
                    if name.len() > THREAD_NAME_MAX {
                        long.push(format!("{}: {name} ({} bytes)", at(), name.len()));
                    }
                }
            }
        }
        assert!(
            unnamed.is_empty(),
            "threads started without a name (event::thread names one):\n{unnamed:#?}"
        );
        assert!(long.is_empty(), "names Linux cuts at {THREAD_NAME_MAX} bytes:\n{long:#?}");
    }

    /// A workspace's and a session's ids (collab §4: unique across machines): this machine's said by their number
    /// alone, as they were before the split — `:workspace-switch 2`, `s2` — another machine's with it; made in order,
    /// a workspace's Claude born with its number.
    #[test]
    fn ids_say_their_number_and_their_machine() {
        let (home, next) = (WorkspaceId::HOME, WorkspaceId::HOME.next());
        assert_eq!((home.to_string(), next.to_string()), ("0".to_string(), "1".to_string()));
        assert!(home < next && next < WorkspaceId::here(2));
        let theirs = WorkspaceId { machine: 3, n: 2, of: std::marker::PhantomData };
        assert_eq!(theirs.to_string(), "2@3");
        assert_eq!(
            ["2", "2@3"].map(|s| s.parse::<WorkspaceId>().ok()),
            [Some(WorkspaceId::here(2)), Some(theirs)]
        );
        assert!("x".parse::<WorkspaceId>().is_err() && "2@".parse::<WorkspaceId>().is_err());
        let claude: SessionId = theirs.twin();
        assert_eq!((claude.machine, claude.n), (3, 2));
        assert_eq!(crate::session::born_with(next), SessionId::here(1));
    }

    /// A result made for a workspace is tagged with it and its session (`Event::For`): one made while a workspace
    /// behind is swapped in is that one's and its Claude's, then the front's again.
    #[test]
    fn a_result_carries_its_workspace_and_its_session() {
        let root = crate::scratch::Scratch::new("c0-tags");
        let mut ed = Editor::new(crate::config::Config::default());
        let behind = ed.workspace_add(std::fs::canonicalize(&root).unwrap());
        // (the tag read off a channel of its own: the editor's queue has the setup's results too — `workspace_add`'s
        // `workspace_kept_back` reads the behind one's notes and turns on a worker — and on a loaded machine one lands
        // before the tag's: #433)
        let tag = |ed: &Editor| {
            let (tx, rx) = mpsc::channel();
            Tx { tx, ..ed.events.sender() }.send(Event::Job(Box::new(|_| {}))).unwrap();
            match rx.recv_timeout(crate::test_results::PATIENCE) {
                Ok(Event::For(w, s, _)) => (w, s),
                _ => panic!("a tagged result"),
            }
        };
        // (one of those landed already, as it may)
        let late = ed.events.tx_for(Some((behind, ed.claude_of(behind))));
        late.send(Event::Job(Box::new(|_| {}))).unwrap();
        let front = (ed.workspace.id, ed.workspace.claude.id);
        assert_eq!(tag(&ed), front);
        let made = std::cell::Cell::new(None);
        ed.with_workspace(behind, |ed| made.set(Some(tag(ed))));
        assert_eq!(made.get(), Some((behind, ed.claude_of(behind))));
        assert_eq!(ed.claude_of(behind), crate::session::born_with(behind));
        assert_eq!(tag(&ed), front, "the front's again");
    }

    /// The start's jobs (`Starts`, the state's `starting`) are counted from their spawn until their results are
    /// applied, with what those results spawn — one that pauses too (a debounce), never one once its work waits, on
    /// time however short (a timer: the shared server's are 20 ms in tests) or on a watch (the disk's rounds), nor what
    /// its result spawns, nor what's spawned after the start; one whose result is dropped unapplied isn't left counted.
    /// (Long timers let go as well: the servers' sight timer, `lsp_editor`'s test, and the usage watch, `sim::starts`;
    /// a poll that respawns ages as one job, `sim::starts` too.)
    #[test]
    fn the_start_s_jobs_are_counted_until_they_land() {
        let mut ed = Editor::new(crate::config::Config::default());
        let land = |ed: &mut Editor| match ed.events.recv_timeout(crate::test_results::PATIENCE) {
            Some(Event::Job(apply) | Event::For(_, _, apply)) => apply(ed),
            _ => panic!("a result"),
        };
        assert!(ed.events.starts_out().is_empty(), "an editor made in a test has no start");
        // (a pause before its work: still the start's until its result lands)
        ed.events.set_starting(true);
        ed.events.jobs().spawn(move || {
            pause(Duration::from_millis(10));
            |_: &mut Editor| {}
        });
        ed.events.started();
        let Some(Event::Job(apply) | Event::For(_, _, apply)) =
            ed.events.recv_timeout(crate::test_results::PATIENCE)
        else {
            panic!("a result")
        };
        assert_eq!(ed.events.starts_out().len(), 1, "a job that paused is the start's until it lands");
        apply(&mut ed);
        assert!(ed.events.starts_out().is_empty(), "landed");
        // (a short wait on time — a timer, as the shared server's in a test build: let go, whatever its length)
        ed.events.set_starting(true);
        ed.events.jobs().spawn(move || {
            sleep(Duration::from_millis(10));
            |ed: &mut Editor| ed.events.jobs().spawn(|| |_: &mut Editor| {})
        });
        ed.events.started();
        land(&mut ed);
        assert!(
            ed.events.starts_out().is_empty(),
            "a timer isn't the start's, however short, nor what it spawned"
        );
        land(&mut ed);
        // (a job waiting on a watch — the disk's rounds, each result starting the next: its wait over at once)
        ed.events.set_starting(true);
        let waker = crate::fswatch::Waker::new();
        waker.wake();
        ed.events.jobs().spawn(move || {
            let _ = crate::fswatch::Watcher::with_waker(waker).wait();
            |ed: &mut Editor| ed.events.jobs().spawn(|| |_: &mut Editor| {})
        });
        ed.events.started();
        land(&mut ed);
        assert!(
            ed.events.starts_out().is_empty(),
            "a watch's round isn't the start's, nor what its result spawned"
        );
        land(&mut ed);
        ed.events.set_starting(true);
        ed.events.shared_jobs().spawn(|| |ed: &mut Editor| ed.events.jobs().spawn(|| |_: &mut Editor| {}));
        ed.events.started();
        let first = ed.events.starts_out();
        assert_eq!(first.len(), 1, "the start's job, out");
        land(&mut ed);
        let then = ed.events.starts_out();
        assert!(
            then.len() == 1 && then[0].0 != first[0].0,
            "landed; what its result spawned is the start's: {then:?}"
        );
        land(&mut ed);
        assert!(ed.events.starts_out().is_empty(), "both landed");
        ed.events.jobs().spawn(|| |_: &mut Editor| {});
        assert!(ed.events.starts_out().is_empty(), "after the start: not the start's");
        land(&mut ed);
        ed.events.set_starting(true);
        ed.events.jobs().spawn(|| |_: &mut Editor| {});
        drop(ed.events.recv_timeout(crate::test_results::PATIENCE).expect("a result"));
        assert!(ed.events.starts_out().is_empty(), "dropped unapplied: not left counted");
    }

    /// The start's work with no result (`background`, `detached` — the prunes) is the start's while it runs, and over
    /// with its work (#398: the temp folder's prune ran on into a settled tarae's rest); one that waits on time first
    /// (a timer) lets go at once; after the start, none is.
    #[test]
    fn the_start_s_work_with_no_result_is_counted_while_it_runs() {
        let ed = Editor::new(crate::config::Config::default());
        for detached in [false, true] {
            let (go, held) = mpsc::channel::<()>();
            let (done, ended) = mpsc::channel::<()>();
            ed.events.set_starting(true);
            let work = move || {
                let _ = held.recv_timeout(crate::test_results::PATIENCE);
                let _ = done.send(());
            };
            match detached {
                true => ed.events.jobs().detached(work),
                false => ed.events.jobs().background(work),
            }
            ed.events.started();
            assert_eq!(ed.events.starts_out().len(), 1, "out while its work runs (detached {detached})");
            assert_eq!(ed.events.owed_out().len(), 1, "owed while its work runs (detached {detached})");
            go.send(()).unwrap();
            ended.recv_timeout(crate::test_results::PATIENCE).expect("its work ended");
            let end = std::time::Instant::now() + crate::test_results::PATIENCE;
            while !ed.events.owed_out().is_empty() && std::time::Instant::now() < end {
                std::thread::yield_now();
            }
            assert!(ed.events.starts_out().is_empty(), "over with its work (detached {detached})");
            assert!(ed.events.owed_out().is_empty(), "owed no more (detached {detached})");
        }
        // (any job, the start's or not: owed until its result is applied — #398's chat jobs)
        let mut ed = ed;
        ed.events.jobs().spawn(|| |_: &mut Editor| {});
        assert_eq!(ed.events.owed_out().len(), 1, "a job after the start is owed");
        let Some(Event::Job(apply) | Event::For(_, _, apply)) =
            ed.events.recv_timeout(crate::test_results::PATIENCE)
        else {
            panic!("a result")
        };
        assert_eq!(ed.events.owed_out().len(), 1, "its result not applied yet");
        apply(&mut ed);
        assert!(ed.events.owed_out().is_empty(), "applied: owed no more");
        // (a timer: its wait comes first — let go there, its end however far)
        let (go, held) = mpsc::channel::<()>();
        let (slept, asleep) = mpsc::channel::<()>();
        ed.events.set_starting(true);
        ed.events.jobs().detached(move || {
            sleep(Duration::from_millis(1));
            let _ = slept.send(());
            let _ = held.recv_timeout(crate::test_results::PATIENCE);
        });
        ed.events.started();
        asleep.recv_timeout(crate::test_results::PATIENCE).expect("it slept");
        assert!(ed.events.starts_out().is_empty(), "a timer isn't the start's");
        go.send(()).unwrap();
        let (go, held) = mpsc::channel::<()>();
        ed.events.jobs().background(move || {
            let _ = held.recv_timeout(crate::test_results::PATIENCE);
        });
        assert!(ed.events.starts_out().is_empty(), "after the start: not the start's");
        go.send(()).unwrap();
    }

    /// Every sleep in the code goes through `event::sleep`: in a simulation the clock moves instead of the thread
    /// sleeping (`sim.rs`), and a job of the start's that sleeps lets go of its count (`waits`, #201) — so no timer
    /// has to remember to leave. None calls `thread::sleep` but `event::sleep` and `event::pause` themselves.
    #[test]
    fn every_sleep_in_the_code_goes_through_event_sleep() {
        // (called, or passed as a value)
        let now = crate::sim::lint::counts(&["thread::sleep"], &["event.rs"]);
        crate::sim::lint::only_shrinks("thread::sleep", &now, &[]);
    }
}
