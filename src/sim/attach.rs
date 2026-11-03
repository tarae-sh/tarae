//! A slow interface across the attach socket (the remote plan's §8b), simulated: the host draws at its own pace and
//! puts each frame whole in the writer's mailbox (`tarae_wire::Slot`, as `attach::Wire::hand` does), the writer
//! (`attach::Writer::step`, driven here with no thread) sends what may go into a byte queue standing for the socket,
//! and the interface reads it in pieces, presents each frame (`screen::Held`, as `tarae attach`), acks late and at
//! random, types keys saying the last frame it presented, and resizes.
//!
//! Checked every step: every frame the interface presents is exactly a frame the host drew (its text, whole, from
//! only what was sent), in increasing order; never more than `IN_FLIGHT` sent and unacked; the editor counts no frame
//! presented that the interface didn't say it presented (its ack, or a key's `seen`). Once the host stops drawing the interface comes to its newest frame — no
//! frame lost for good — and over the seeds a slow interface made the mailbox skip some.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tarae_wire::{Decoder, IN_FLIGHT, Next, Slot, ToInterface};

use super::{Choices, Sim};
use crate::attach::Writer;
use crate::editor::Editor;
use crate::event::Event;
use crate::interface::Whole;

const SEEDS: &[u64] = &[0, 1, 2, 7, 10];

const STEPS: usize = 80;

const LIMIT: Duration = crate::test_results::PATIENCE;

/// Frames the mailbox skipped, over the seeds.
static SKIPPED: AtomicU64 = AtomicU64::new(0);

#[test]
fn a_slow_interface_skips_frames_and_shows_each_one_whole() {
    super::seeds("a_slow_interface_skips_frames_and_shows_each_one_whole", SEEDS, run);
    assert!(SKIPPED.load(Ordering::Relaxed) > 0, "no seed's interface was slow enough to skip a frame");
}

/// The host's mailbox and writer, the socket, and the interface.
struct Link {
    slot: Slot<Whole, ToInterface>,
    writer: Writer,
    /// Written and not read yet.
    socket: Vec<u8>,
    /// The frames the host drew, by number: their size and text.
    drawn: BTreeMap<u64, (u16, u16, String)>,
    dec: Decoder<ToInterface>,
    held: crate::screen::Held,
    /// The last frame the interface presented, and the last it acked.
    presented: u64,
    acked: u64,
    /// The interface's size.
    size: (u16, u16),
    seq: u64,
    wrong: Option<String>,
}

impl Link {
    fn new() -> Link {
        Link {
            slot: Slot::default(),
            writer: Writer::new(),
            socket: Vec::new(),
            drawn: BTreeMap::new(),
            dec: Decoder::new(),
            held: crate::screen::Held::new(),
            presented: 0,
            acked: 0,
            size: (60, 12),
            seq: 0,
            wrong: None,
        }
    }

    /// The host draws a frame and hands it over whole (the loop's `write` never waits: a slot, a replace).
    fn draw(&mut self, ed: &mut Editor) -> String {
        let (w, h) = self.size;
        let mut bytes = Vec::new();
        crate::term::render(ed, &mut bytes, w, h).unwrap();
        let n = ed.frames;
        let text = crate::screen::text(&bytes, w, h).unwrap_or_default();
        self.drawn.insert(n, (w, h, text));
        let f = Whole { n, input: self.seq, w, h, bytes, drawn: None, unknown: false };
        let skipped = self.slot.put_frame(n, f).is_some();
        SKIPPED.fetch_add(skipped as u64, Ordering::Relaxed);
        format!("frame {n} drawn at {w}×{h}{}", if skipped { ", the unsent one replaced" } else { "" })
    }

    /// The writer sends what may go next — None: nothing may.
    fn send(&mut self) -> Option<String> {
        let next = self.slot.take();
        let said = match &next {
            Next::Frame(n, _) => format!("frame {n} sent"),
            Next::Msg(_) => "a message sent".into(),
            Next::Wait | Next::Closed => return None,
        };
        if let Some(back) = self.writer.step(next, &mut self.socket) {
            self.slot.give_back(back);
        }
        Some(said)
    }

    /// The interface reads `k` bytes of the socket and presents every whole frame in them, as `tarae attach`.
    fn read(&mut self, k: usize) -> String {
        let k = k.min(self.socket.len());
        self.dec.push(&self.socket[..k]);
        self.socket.drain(..k);
        let mut shown = Vec::new();
        while let Ok(Some(m)) = self.dec.message() {
            let ToInterface::Frame { n, w, h, bytes, .. } = m else { continue };
            self.held.apply(&bytes, w, h);
            if n <= self.presented {
                self.wrong = Some(format!("frame {n} presented after frame {}", self.presented));
            }
            let mut whole = Vec::new();
            self.held.whole(&mut whole);
            let now = crate::screen::text(&whole, w, h).unwrap_or_default();
            match self.drawn.get(&n) {
                Some((dw, dh, text)) if (*dw, *dh) == (w, h) && *text == now => {}
                Some((dw, dh, text)) => {
                    self.wrong = Some(format!(
                        "frame {n} presented at {w}×{h} isn't the one drawn at {dw}×{dh}:\n{now}\n— drawn —\n{text}"
                    ))
                }
                None => self.wrong = Some(format!("frame {n} presented, never drawn")),
            }
            self.presented = n;
            shown.push(n);
        }
        format!("the interface reads {k} bytes, presents {shown:?}")
    }

    /// The interface acks what it presented: credit for the writer, presented for the editor (`attach::presented`).
    fn ack(&mut self, ed: &mut Editor) -> String {
        self.acked = self.presented;
        self.slot.ack(self.acked);
        ed.frame_presented(self.acked);
        format!("the interface acks frame {}", self.acked)
    }
}

fn run(choices: Choices) -> Vec<String> {
    let mut sim = Sim::new("a_slow_interface_skips_frames_and_shows_each_one_whole", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.interface.presents_late = true;
    ed.handle_event(Event::Key("i".parse().unwrap()));
    let link = RefCell::new(Link::new());
    let check = |ed: &Editor| held(ed, &link.borrow());
    sim.walk(&mut ed, STEPS, |sim, ed| step(sim, ed, &link), check, print);
    // The host stops drawing: the writer and the interface go on until nothing moves — it comes to the newest frame
    let mut l = link.borrow_mut();
    for _ in 0..64 {
        while l.send().is_some() {}
        let n = l.socket.len();
        l.read(n);
        l.ack(&mut ed);
    }
    let newest = l.drawn.keys().next_back().copied().unwrap_or(0);
    let mut trace = sim.trace().to_vec();
    if l.presented != newest || l.wrong.is_some() {
        trace.push(format!(
            "FAILED: quiet, the interface presented frame {} of {newest} drawn ({:?})",
            l.presented, l.wrong
        ));
        panic!("{}", trace.join("\n"));
    }
    drop(l);
    drop(sim);
    drop(ed);
    trace
}

fn step(sim: &mut Sim, ed: &mut Editor, link: &RefCell<Link>) -> String {
    let c = &mut sim.choices;
    let mut l = link.borrow_mut();
    // (by arm: a key typed 3, a frame drawn 4, the writer 3, the interface reading 3, acking 2, a resize 1, time 1)
    match c.weighted(&[3, 4, 3, 3, 2, 1, 1]) {
        0 => {
            l.seq += 1;
            let k = *c.pick(&["a", "b", "ret", "backspace", "x"]);
            let event = Box::new(Event::Key(k.parse().unwrap()));
            ed.handle_event(Event::Input { seen: l.presented, seq: l.seq, event });
            format!("key {k}, the interface having presented frame {}", l.presented)
        }
        1 => l.draw(ed),
        2 => l.send().unwrap_or_else(|| "the writer waits (credit, or nothing new)".into()),
        3 => {
            let k = *c.pick(&[1, 7, 64, 900, 1 << 16]);
            l.read(k)
        }
        4 => l.ack(ed),
        5 => {
            l.size = *c.pick(&[(60, 12), (40, 10), (72, 14)]);
            format!("the interface resized to {}×{}", l.size.0, l.size.1)
        }
        _ => {
            let ms = *c.pick(&[16, 300]);
            sim.wait(Duration::from_millis(ms));
            "time passes".into()
        }
    }
}

/// Nothing presented that wasn't drawn as it is, no more than `IN_FLIGHT` unacked, and the editor counts no frame the
/// interface didn't present (an ack, or a key's `seen`).
fn held(ed: &Editor, l: &Link) -> Result<(), String> {
    if let Some(why) = &l.wrong {
        return Err(why.clone());
    }
    if l.slot.in_flight() > IN_FLIGHT {
        return Err(format!("{} frames sent and unacked, past {IN_FLIGHT}", l.slot.in_flight()));
    }
    if ed.presented > l.presented {
        return Err(format!(
            "frame {} counted as presented, the interface presented {}",
            ed.presented, l.presented
        ));
    }
    Ok(())
}

fn print(ed: &Editor) -> String {
    format!("frame {} · presented {}", ed.frames, ed.presented)
}
