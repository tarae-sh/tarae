//! The audio thread, `tarae-sound`: made the first time a turn runs with `editor.sound` on (never at start), it opens
//! the system's output (`mac.rs`), then waits on its channel. A sound starts the device and goes into the ring the
//! render callback reads; `QUIET` after the last one ends the device stops — measured: open and silent, the callback
//! wakes ~94 times a second and coreaudiod spends 6–10 % of a core (.sb/sound-plan.md), stopped both are at the
//! floor. Stopped, the thread waits in `recv` with no timeout: nothing wakes it until the next sound. The main loop
//! only sends; it never waits on this thread nor hears from it but the open's answer (on a worker).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use super::said::{Kind, Said};
#[cfg(target_os = "macos")]
use super::synth::Synth;

/// How long the device keeps running after the last sound ends: the stream's lumps come ~0.6 s apart (writing.rs),
/// a start costs ~40 ms (measured) — a burst is one run of the device.
pub const QUIET: Duration = Duration::from_millis(1500);

/// The longest a sound rings (the space bar held 155 ms, then its way up's decay): the device runs this past a
/// sound's delay, then `QUIET`.
const RINGS: Duration = Duration::from_millis(400);

pub enum Msg {
    /// A sound may come soon (an edit's text began streaming): the device starts, so its first key isn't late.
    Wake,
    Play(Said),
    Volume(u8),
}

/// The system's output as the thread drives it.
pub trait Output {
    /// The render callback runs from now (the device was stopped).
    fn start(&mut self) -> Result<(), String>;
    /// It stops (the device was running).
    fn stop(&mut self);
}

/// Sounds on their way from the thread to the render callback: one writer, one reader, no lock and no allocation
/// (the callback runs on the system's real-time thread). A sound past its room is dropped.
pub struct Ring {
    slots: [[AtomicU64; 2]; RING],
    head: AtomicUsize,
    tail: AtomicUsize,
    /// The master level (`said::master`, f32 bits), read by the callback.
    pub master: AtomicU32,
}

const RING: usize = 64;

const KINDS: [Kind; 5] = [Kind::Key, Kind::Space, Kind::Return, Kind::Erase, Kind::Settle];

impl Ring {
    pub fn new(volume: u8) -> Ring {
        Ring {
            slots: std::array::from_fn(|_| [AtomicU64::new(0), AtomicU64::new(0)]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            master: AtomicU32::new(super::said::master(volume).to_bits()),
        }
    }

    /// The writer's side (the thread).
    pub fn push(&self, s: Said) -> bool {
        let (h, t) = (self.head.load(Ordering::Relaxed), self.tail.load(Ordering::Acquire));
        if h - t >= RING {
            return false;
        }
        let kind = KINDS.iter().position(|k| *k == s.kind).unwrap_or(0) as u64;
        let slot = &self.slots[h % RING];
        slot[0].store(kind << 32 | s.seed as u64, Ordering::Relaxed);
        slot[1].store((s.gain.to_bits() as u64) << 32 | s.delay.to_bits() as u64, Ordering::Relaxed);
        self.head.store(h + 1, Ordering::Release);
        true
    }

    /// The reader's side (the render callback).
    #[cfg(any(target_os = "macos", test))]
    pub fn pop(&self) -> Option<Said> {
        let (t, h) = (self.tail.load(Ordering::Relaxed), self.head.load(Ordering::Acquire));
        if t == h {
            return None;
        }
        let slot = &self.slots[t % RING];
        let (a, b) = (slot[0].load(Ordering::Relaxed), slot[1].load(Ordering::Relaxed));
        self.tail.store(t + 1, Ordering::Release);
        Some(Said {
            kind: KINDS[((a >> 32) as usize).min(KINDS.len() - 1)],
            seed: a as u32,
            gain: f32::from_bits((b >> 32) as u32),
            delay: f32::from_bits(b as u32),
        })
    }
}

/// The render callback's side: what came, synthesized into the device's buffer. macOS's for now (Linux's ALSA is
/// sound-plan §5 step 3).
#[cfg(target_os = "macos")]
pub struct Player {
    ring: Arc<Ring>,
    synth: Synth,
}

#[cfg(target_os = "macos")]
impl Player {
    pub fn new(ring: Arc<Ring>, rate: f32) -> Player {
        Player { synth: Synth::new(rate), ring }
    }

    pub fn fill(&mut self, out: &mut [f32]) {
        while let Some(s) = self.ring.pop() {
            self.synth.play(&s);
        }
        self.synth.render(out, f32::from_bits(self.ring.master.load(Ordering::Relaxed)));
    }
}

/// The thread's life: each message as it comes, the device started by a sound and stopped `quiet` after the last
/// one rang out; it ends when the editor lets go of its channel. How many times it woke — for the tests: one a
/// message, one a stop, one at the end, never another.
pub fn run(rx: &Receiver<Msg>, ring: &Ring, out: &mut dyn Output, quiet: Duration) -> usize {
    let (mut until, mut woke): (Option<Instant>, usize) = (None, 0);
    loop {
        let msg = match until {
            // (stopped: nothing to wait for but the next message)
            None => rx.recv().ok(),
            Some(t) => match rx.recv_timeout(t.saturating_duration_since(crate::clock::now())) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => {
                    woke += 1;
                    out.stop();
                    until = None;
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => None,
            },
        };
        woke += 1;
        let Some(msg) = msg else { break };
        let rings = match msg {
            Msg::Volume(v) => {
                ring.master.store(super::said::master(v).to_bits(), Ordering::Relaxed);
                continue;
            }
            Msg::Wake => Duration::ZERO,
            Msg::Play(s) => {
                ring.push(s);
                Duration::from_secs_f32(s.delay.max(0.0)) + RINGS
            }
        };
        if until.is_none()
            && let Err(e) = out.start()
        {
            crate::log::line("sound", "warning", &format!("the output didn't start: {e}"));
            continue;
        }
        let end = crate::clock::now() + rings + quiet;
        until = Some(until.map_or(end, |u| u.max(end)));
    }
    if until.is_some() {
        out.stop();
    }
    woke
}

/// Opens the system's output and starts the thread on it: its channel, or why there's no sound. Waits for the open
/// (~100 ms the first time, measured) — a worker's, never the main loop's.
pub fn spawn(volume: u8) -> Result<Sender<Msg>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (said, opened) = std::sync::mpsc::sync_channel(1);
    let ring = Arc::new(Ring::new(volume));
    crate::event::thread("tarae-sound", move || {
        let mut out = match super::device::open(ring.clone()) {
            Ok(out) => out,
            Err(e) => {
                let _ = said.send(Err(e));
                return;
            }
        };
        let _ = said.send(Ok(()));
        run(&rx, &ring, &mut out, QUIET);
    });
    opened.recv().map_err(|_| "its thread ended".to_string())??;
    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A device that says what was done to it.
    struct Fake(Arc<Mutex<Vec<(&'static str, Instant)>>>, Sender<&'static str>);

    impl Output for Fake {
        fn start(&mut self) -> Result<(), String> {
            self.0.lock().unwrap().push(("start", Instant::now()));
            let _ = self.1.send("start");
            Ok(())
        }
        fn stop(&mut self) {
            self.0.lock().unwrap().push(("stop", Instant::now()));
            let _ = self.1.send("stop");
        }
    }

    fn said(kind: Kind) -> Said {
        Said { kind, gain: 1.0, seed: 3, delay: 0.0 }
    }

    #[test]
    fn a_sound_starts_the_device_and_it_stops_once_quiet_never_woken_while_stopped() {
        let quiet = Duration::from_millis(30);
        let (tx, rx) = std::sync::mpsc::channel();
        let (seen, done) = std::sync::mpsc::channel();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ring = Arc::new(Ring::new(30));
        let (c, r) = (calls.clone(), ring.clone());
        let t = std::thread::spawn(move || run(&rx, &r, &mut Fake(c, seen), quiet));
        tx.send(Msg::Play(said(Kind::Key))).unwrap();
        tx.send(Msg::Play(said(Kind::Return))).unwrap();
        let patience = crate::test_results::PATIENCE;
        assert_eq!(done.recv_timeout(patience), Ok("start"));
        assert_eq!(done.recv_timeout(patience), Ok("stop"));
        // (both went to the callback's ring, in order)
        assert_eq!(ring.pop().map(|s| s.kind), Some(Kind::Key));
        assert_eq!(ring.pop().map(|s| s.kind), Some(Kind::Return));
        drop(tx);
        let woke = t.join().unwrap();
        // two messages, the stop, the end: nothing woke it while it was stopped
        assert_eq!(woke, 4);
        let calls = calls.lock().unwrap();
        assert_eq!(calls.iter().map(|c| c.0).collect::<Vec<_>>(), ["start", "stop"], "started once");
        assert!(calls[1].1 - calls[0].1 >= RINGS + quiet, "stopped before the sound rang out and went quiet");
    }

    #[test]
    fn a_wake_starts_it_and_the_end_stops_it() {
        let (tx, rx) = std::sync::mpsc::channel();
        let (seen, done) = std::sync::mpsc::channel();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ring = Ring::new(30);
        tx.send(Msg::Wake).unwrap();
        tx.send(Msg::Volume(100)).unwrap();
        drop(tx);
        assert_eq!(run(&rx, &ring, &mut Fake(calls.clone(), seen), QUIET), 3);
        assert_eq!(done.try_iter().collect::<Vec<_>>(), ["start", "stop"]);
        assert_eq!(f32::from_bits(ring.master.load(Ordering::Relaxed)), 1.0);
    }

    #[test]
    fn the_ring_drops_what_has_no_room_and_keeps_its_order() {
        let ring = Ring::new(30);
        for i in 0..RING as u32 {
            assert!(ring.push(Said { seed: i, ..said(Kind::Space) }));
        }
        assert!(!ring.push(said(Kind::Key)), "past its room");
        let seeds: Vec<u32> = std::iter::from_fn(|| ring.pop()).map(|s| s.seed).collect();
        assert_eq!(seeds, (0..RING as u32).collect::<Vec<_>>());
        let s = Said { kind: Kind::Settle, gain: 0.7, seed: 9, delay: 0.012 };
        ring.push(s);
        assert_eq!(ring.pop(), Some(s));
    }
}
