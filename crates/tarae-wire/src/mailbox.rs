//! The host's writer's mailbox (the remote plan's §8b, "The main loop never waits"): the main loop puts each frame
//! in one slot — an unsent one there is replaced, never queued, so a slow interface skips frames and nothing is lost
//! (a frame not sent was never diffed) — and the other messages in a queue; the writer thread takes what may go next.
//! At most `IN_FLIGHT` frames are sent and not acked: past that the writer waits for an `Ack` while newer frames
//! replace each other here, so a slow link never holds seconds of frames in the kernel's and SSH's buffers.
//!
//! `Slot` is the rule, pure (the simulation drives it with no thread); `Mailbox` is it behind a lock, the writer
//! waiting on it.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard};

/// Frames sent and not acked yet, at most.
pub const IN_FLIGHT: usize = 2;

/// When a queued message may go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// At once, behind no frame nor credit (a sound: late, it would ring late).
    AtOnce,
    /// Once the frame last put (or a newer one that replaced it) is sent — an `Ask` after the frame it came with, as
    /// OSC 52 follows the frame's synchronized update.
    AfterFrame,
}

/// What the writer sends next.
#[derive(Debug, PartialEq)]
pub enum Next<F, M> {
    Frame(u64, F),
    Msg(M),
    /// Nothing may go yet.
    Wait,
    /// Closed, and everything queued went.
    Closed,
}

struct Queued<M> {
    m: M,
    /// The frame it goes after (`Order::AfterFrame`), else none.
    after: Option<u64>,
    bytes: usize,
}

pub struct Slot<F, M> {
    frame: Option<(u64, F)>,
    rest: VecDeque<Queued<M>>,
    /// Frames sent and not acked, oldest first.
    unacked: VecDeque<u64>,
    last_put: u64,
    last_sent: u64,
    pending: usize,
    spare: Option<F>,
    skipped: u64,
    closed: bool,
}

impl<F, M> Default for Slot<F, M> {
    fn default() -> Self {
        Slot {
            frame: None,
            rest: VecDeque::new(),
            unacked: VecDeque::new(),
            last_put: 0,
            last_sent: 0,
            pending: 0,
            spare: None,
            skipped: 0,
            closed: false,
        }
    }
}

impl<F, M> Slot<F, M> {
    /// Frame `n` (numbers only grow) in the slot. What it replaced, or the frame itself once closed, comes back: its
    /// buffer is the loop's again.
    pub fn put_frame(&mut self, n: u64, f: F) -> Option<F> {
        if self.closed {
            return Some(f);
        }
        self.last_put = n;
        let old = self.frame.replace((n, f)).map(|(_, f)| f);
        self.skipped += old.is_some() as u64;
        old
    }

    /// A message (`bytes` of it, for `pending`) to go by `order`. Nothing is queued once closed.
    pub fn push(&mut self, m: M, order: Order, bytes: usize) {
        if self.closed {
            return;
        }
        let after = match order {
            Order::AtOnce => None,
            Order::AfterFrame => Some(self.last_put),
        };
        self.pending += bytes;
        self.rest.push_back(Queued { m, after, bytes });
    }

    /// The interface presented frame `n`: every frame up to it is credit again.
    pub fn ack(&mut self, n: u64) {
        while self.unacked.front().is_some_and(|&f| f <= n) {
            self.unacked.pop_front();
        }
    }

    /// No more frames: what's queued still goes (a `Gone` last), the frame in the slot doesn't.
    pub fn close(&mut self) {
        self.closed = true;
    }

    pub fn in_flight(&self) -> usize {
        self.unacked.len()
    }

    /// Frames replaced in the slot before they went.
    pub fn skipped(&self) -> u64 {
        self.skipped
    }

    /// The bytes of messages queued and not sent.
    pub fn pending(&self) -> usize {
        self.pending
    }

    pub fn last_sent(&self) -> u64 {
        self.last_sent
    }

    /// A sent frame's buffer, for the loop's next frame.
    pub fn give_back(&mut self, f: F) {
        self.spare.get_or_insert(f);
    }

    pub fn take_spare(&mut self) -> Option<F> {
        self.spare.take()
    }

    /// What goes next: a queued message that may go, else the frame if credit allows.
    pub fn take(&mut self) -> Next<F, M> {
        let (closed, sent) = (self.closed, self.last_sent);
        if let Some(i) = self.rest.iter().position(|q| closed || q.after.is_none_or(|a| a <= sent)) {
            let q = self.rest.remove(i).expect("a position found");
            self.pending -= q.bytes;
            return Next::Msg(q.m);
        }
        if closed {
            if let Some((_, f)) = self.frame.take() {
                self.give_back(f);
            }
            return Next::Closed;
        }
        if self.unacked.len() < IN_FLIGHT
            && let Some((n, f)) = self.frame.take()
        {
            self.last_sent = n;
            self.unacked.push_back(n);
            return Next::Frame(n, f);
        }
        Next::Wait
    }
}

/// A `Slot` shared by the main loop, the reader (acks) and the writer, which waits on it.
pub struct Mailbox<F, M> {
    slot: Mutex<Slot<F, M>>,
    moved: Condvar,
}

impl<F, M> Default for Mailbox<F, M> {
    fn default() -> Self {
        Mailbox { slot: Mutex::new(Slot::default()), moved: Condvar::new() }
    }
}

impl<F, M> Mailbox<F, M> {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Slot<F, M>> {
        // (nothing holding it panics but a bug, and release aborts on one: what's there is still whole)
        self.slot.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// `f(slot)` under the lock, the writer woken after.
    pub fn with<T>(&self, f: impl FnOnce(&mut Slot<F, M>) -> T) -> T {
        let r = f(&mut self.lock());
        self.moved.notify_all();
        r
    }

    /// The writer's wait: what goes next, once something may.
    pub fn wait(&self) -> Next<F, M> {
        let mut slot = self.lock();
        loop {
            match slot.take() {
                Next::Wait => slot = self.moved.wait(slot).unwrap_or_else(|p| p.into_inner()),
                n => return n,
            }
        }
    }
}
