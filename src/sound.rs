//! The ASMR mode (`editor.sound`, off by default, yours only): Claude's writing heard as it appears. A sound says
//! something or it's silent, as motion: keys as characters appear at the writing head, the space bar for a gap between
//! words, return as a line ends (`writing::reveal` — what each step newly shows, `typist.rs`'s pace), a soft thunk as
//! an edit lands unseen, backspace when it only took text out (`refactor.rs`'s review at the end, and a run of
//! Claude's that rewrote an open buffer as the buffer takes it — `motion::run_landed`: where the wet ink lands, the
//! settle sounds). Only the front workspace's Claude, only in a buffer of a pane in front; never your keys, nor your
//! `y` (the stamp says that), nor reading, a run's output or thinking; the chat's prose silent for now. The keys are
//! a recorded keyboard's (`sampled.rs`, `recorded.rs`), the settle synthesized (`synth.rs`).
//!
//! The decision is here, on the main loop's clock (`clock::now`: the simulation runs it); the sound is the audio
//! thread's (`engine.rs`, `mac.rs`), only told — the main loop never waits on it. It's made the first time a turn runs
//! with sound on, on a worker (opening the output takes ~100 ms), and lives until quit or `editor.sound` goes off.
//! Only for an interface whose speaker plays it (`Editor.interface.sound`: the terminal's is this machine's; an attached
//! interface's is its client's — what's decided goes across its wire, `Sink::Wire`, and its client plays it, `Played`)
//! — a tests' editor's has none, so no thread or device is made; a test that gives it one stands in its opener
//! (`State.open`). A test records what would sound (`Sink::Record`).

mod engine;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
mod recorded;
pub mod said;
#[cfg(target_os = "macos")]
pub mod sampled;
// (what plays a sound: only macOS's output does yet — Linux's ALSA is sound-plan §5 step 3)
#[cfg(target_os = "macos")]
pub mod synth;
pub mod typist;

use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::document::DocId;
use crate::editor::Editor;
use crate::interface::Speaker;
use engine::Msg;
use said::{Kind, Said};

#[cfg(target_os = "macos")]
use mac as device;

/// Elsewhere there's no output yet (Linux's ALSA is sound-plan §5 step 3): opening says so, and never gives one —
/// no output stands in for a device.
#[cfg(not(target_os = "macos"))]
mod device {
    use std::convert::Infallible;

    impl super::engine::Output for Infallible {
        fn start(&mut self) -> Result<(), String> {
            match *self {}
        }
        fn stop(&mut self) {
            match *self {}
        }
    }

    pub fn open(_: std::sync::Arc<super::engine::Ring>) -> Result<Infallible, String> {
        Err(NOT_HERE.into())
    }

    pub const NOT_HERE: &str = "no sound on this system yet (macOS only for now)";
}

/// Where what's decided goes.
pub enum Sink {
    Off,
    /// The audio thread's channel.
    Device(Sender<Msg>),
    /// An attached interface's speaker (by its number, `Interface.id`): what's decided goes across its wire
    /// (`ToInterface::Sound`), its client plays it (`Played`).
    Wire(u64),
    /// Kept, with when it was decided (tests, the simulation).
    #[cfg_attr(not(test), allow(dead_code))] // (the scenes' too with the track — sound-plan §5 step 4)
    Record(Vec<(Instant, Said)>),
}

pub struct State {
    pub sink: Sink,
    typist: typist::Typist,
    /// The output being opened on a worker.
    opening: bool,
    /// It couldn't be opened — said once; tried again once `editor.sound` goes off and on.
    failed: bool,
    /// The volume the thread was last told.
    told: u8,
    /// Opens the output and starts its thread at a volume, on a worker (`engine::spawn`).
    pub open: fn(u8) -> Result<Sender<Msg>, String>,
}

impl Default for State {
    fn default() -> State {
        State {
            sink: Sink::Off,
            typist: typist::Typist::new(0x5eed),
            opening: false,
            failed: false,
            told: 0,
            open: engine::spawn,
        }
    }
}

/// Whether Claude's writing is heard now: `editor.sound`, and not a session behind's result being applied.
pub fn on(ed: &Editor) -> bool {
    ed.config.sound && !ed.background()
}

/// Whether a sound can be heard now: on, and an output opened — never off macOS (none opens there yet), nor after it
/// failed to open, nor in a test's record — or an attached interface's speaker playing it.
pub fn playing(ed: &Editor) -> bool {
    on(ed) && matches!(ed.sound.sink, Sink::Device(_) | Sink::Wire(_))
}

/// The buffers whose writing is heard: those the last frame drew in a pane (none while it's off) — a pane zoomed
/// away, or behind another tab, draws nothing and says nothing.
pub fn sight(ed: &Editor) -> Vec<DocId> {
    match on(ed) {
        true => ed.screen.pane_docs.clone(),
        false => Vec::new(),
    }
}

/// A step of the reveal showed `text` in sight (`writing::reveal`): a key, the space bar or a return — or nothing,
/// at a typist's pace.
pub fn wrote(ed: &mut Editor, text: &str) {
    if text.is_empty() || !on(ed) {
        return;
    }
    let said = ed.sound.typist.step(crate::clock::now(), text, crate::writing::REVEAL_EVERY);
    if let Some(s) = said {
        play(ed, s);
    }
}

/// An edit's text began streaming (`writing::on_delta`): the device starts now, so its first key isn't ~40 ms late.
pub fn wake(ed: &mut Editor) {
    if on(ed) {
        told(ed, Msg::Wake);
    }
}

/// Claude's edit landed unseen in `f.path` (review at the end — `refactor::write`), its buffer in sight: the page
/// settling, or backspace when it only took text out. Your `y` lands through another path, and says nothing.
pub fn landed(ed: &mut Editor, f: &crate::refactor::FileChange) {
    use crate::refactor::Op;
    if matches!(f.op, Op::Text | Op::Create) {
        settle(ed, &f.path, &f.before, &f.after);
    }
}

/// Claude's write took `path` from `before` to `after` unseen — an unseen landing (`landed`) or a run of its that
/// rewrote an open buffer (`motion::run_landed`), its buffer in sight: the page settling, or backspace when it only
/// took text out.
pub fn settle(ed: &mut Editor, path: &std::path::Path, before: &ropey::Rope, after: &ropey::Rope) {
    if !on(ed) {
        return;
    }
    let sight = sight(ed);
    let shown = ed.docs.iter().any(|d| d.path.as_deref() == Some(path) && sight.contains(&d.id));
    if !shown {
        return;
    }
    let kind = match crate::disk::minimal_change(before, after) {
        None => return,
        Some(c) if c.insert.is_empty() => Kind::Erase,
        Some(_) => Kind::Settle,
    };
    let s = ed.sound.typist.once(crate::clock::now(), kind);
    play(ed, s);
}

fn play(ed: &mut Editor, s: Said) {
    match &mut ed.sound.sink {
        Sink::Record(kept) => kept.push((crate::clock::now(), s)),
        _ => told(ed, Msg::Play(s)),
    }
}

/// `m` to what plays the sound: the audio thread here, or the interface's speaker across its wire (at once, behind
/// no frame — `attach::flush`).
fn told(ed: &mut Editor, m: Msg) {
    match &ed.sound.sink {
        Sink::Device(tx) => {
            let _ = tx.send(m);
        }
        &Sink::Wire(iface) => {
            let m = tarae_wire::ToInterface::Sound(wire_of(m));
            ed.attach.out.push((iface, m, tarae_wire::Order::AtOnce));
        }
        Sink::Record(_) | Sink::Off => {}
    }
}

/// The audio thread's message as the wire says it (`ToInterface::Sound`).
fn wire_of(m: Msg) -> tarae_wire::Sound {
    match m {
        Msg::Wake => tarae_wire::Sound::Wake,
        Msg::Play(s) => tarae_wire::Sound::Play {
            kind: engine::KINDS.iter().position(|k| *k == s.kind).unwrap_or(0) as u8,
            gain: s.gain,
            seed: s.seed,
            delay: s.delay,
        },
        Msg::Volume(v) => tarae_wire::Sound::Volume(v),
    }
}

/// The wire's sound as the audio thread's message — None: a kind this build doesn't know.
fn msg_of(s: tarae_wire::Sound) -> Option<Msg> {
    Some(match s {
        tarae_wire::Sound::Wake => Msg::Wake,
        tarae_wire::Sound::Play { kind, gain, seed, delay } => {
            let kind = *engine::KINDS.get(kind as usize)?;
            Msg::Play(Said { kind, gain: gain.clamp(0.0, 1.0), seed, delay: delay.clamp(0.0, 1.0) })
        }
        tarae_wire::Sound::Volume(v) => Msg::Volume(v.min(100)),
    })
}

/// An attached client's speaker (`client.rs`): what its host decided, played here. The audio thread is made with the
/// first sound heard, on a thread that opens the output (~100 ms) while what comes meanwhile waits in its channel;
/// one that can't open is logged once and what follows dropped.
#[derive(Default)]
pub struct Played {
    tx: Option<Sender<Msg>>,
    /// The host's volume, as last told.
    volume: Option<u8>,
}

impl Played {
    pub fn heard(&mut self, s: tarae_wire::Sound) {
        let Some(m) = msg_of(s) else { return };
        if let Msg::Volume(v) = m {
            self.volume = Some(v);
        }
        let volume = self.volume.unwrap_or(crate::config::EditorConfig::default().sound_volume);
        let tx = self.tx.get_or_insert_with(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Msg>();
            crate::event::thread("tarae-sound-in", move || match engine::spawn(volume) {
                Ok(out) => {
                    for m in rx {
                        if out.send(m).is_err() {
                            return;
                        }
                    }
                }
                Err(e) => {
                    crate::log::line("sound", "warn", &format!("the host's sound not played here: {e}"))
                }
            });
            tx
        });
        let _ = tx.send(m);
    }
}

/// After every event: the thread let go once `editor.sound` is off; made (on a worker) the first time a turn runs
/// with it on; told a volume that changed.
pub fn sync(ed: &mut Editor) {
    let volume = ed.config.sound_volume;
    let st = &mut ed.sound;
    if !ed.config.sound {
        st.failed = false;
        if let Sink::Device(_) | Sink::Wire(_) = st.sink {
            // (its channel let go: the thread stops the device and ends; an interface's is told nothing more)
            st.sink = Sink::Off;
        }
        return;
    }
    if let Sink::Device(tx) = &st.sink {
        if st.told != volume {
            let _ = tx.send(Msg::Volume(volume));
        }
        st.told = volume;
        return;
    }
    // (an interface's speaker: told the volume as it attaches and as it changes; another attached, it's told anew;
    // none that plays, let go)
    let iface = ed.attach.current;
    if let Sink::Wire(was) = st.sink {
        if ed.interface.sound != Speaker::Wire {
            st.sink = Sink::Off;
        } else if was != iface || st.told != volume {
            st.sink = Sink::Wire(iface);
            st.told = volume;
            let m = tarae_wire::ToInterface::Sound(tarae_wire::Sound::Volume(volume));
            ed.attach.out.push((iface, m, tarae_wire::Order::AtOnce));
        }
        return;
    }
    let turn = ed.workspace.claude.chat.as_ref().is_some_and(|c| c.state != crate::chat::State::Idle);
    if ed.interface.sound == Speaker::Wire && turn && matches!(st.sink, Sink::Off) {
        st.sink = Sink::Wire(iface);
        st.told = volume;
        let m = tarae_wire::ToInterface::Sound(tarae_wire::Sound::Volume(volume));
        ed.attach.out.push((iface, m, tarae_wire::Order::AtOnce));
        return;
    }
    if ed.interface.sound != Speaker::Here
        || st.opening
        || st.failed
        || !matches!(st.sink, Sink::Off)
        || !turn
    {
        return;
    }
    st.opening = true;
    let open = st.open;
    ed.events.jobs().spawn(move || {
        let opened = open(volume);
        move |ed: &mut Editor| {
            ed.sound.opening = false;
            match opened {
                Ok(tx) if ed.config.sound => {
                    ed.sound.sink = Sink::Device(tx);
                    ed.sound.told = volume;
                }
                // (turned off while it opened: let go)
                Ok(_) => {}
                Err(e) => {
                    ed.sound.failed = true;
                    ed.set_warning(format!("sound: {e}"));
                }
            }
        }
    });
}

/// `:sound [on|off]` — what it says, and whether that's all well (else a warning).
pub fn toggled(on: bool) -> (String, bool) {
    #[cfg(not(target_os = "macos"))]
    if on {
        return (format!("sound on — but {}", device::NOT_HERE), false);
    }
    match on {
        // (short: the status line's, beside the mode — `:set! editor.sound true` keeps it, as any `:set`)
        true => ("sound on · Claude's writing heard until you quit".into(), true),
        false => ("sound off".into(), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refactor::{FileChange, Op};

    /// An editor on `a.rs` (`text`) with sound on, what it would play kept.
    fn heard(text: &str) -> (Editor, crate::scratch::Scratch) {
        let dir = crate::scratch::Scratch::new("sound");
        std::fs::write(dir.join("a.rs"), text).unwrap();
        std::fs::write(dir.join("b.rs"), "b\n").unwrap();
        let mut ed = Editor::new(crate::config::Config::default());
        ed.workspace.root = dir.to_path_buf();
        ed.open(&dir.join("a.rs")).unwrap();
        ed.config.sound = true;
        ed.sound.sink = Sink::Record(Vec::new());
        drawn(&mut ed);
        (ed, dir)
    }

    /// A frame, as the main loop draws after events: what's in sight is what it drew.
    fn drawn(ed: &mut Editor) {
        crate::term::render(ed, &mut Vec::new(), 100, 30).unwrap();
    }

    fn keys(ed: &mut Editor, keys: &[&str]) {
        for k in keys {
            ed.handle_event(crate::event::Event::Key(k.parse::<crate::key::Key>().unwrap()));
        }
        drawn(ed);
    }

    fn land(ed: &mut Editor, before: &str, after: &str) -> Vec<Kind> {
        let path = ed.docs.iter().find_map(|d| d.path.clone().filter(|p| p.ends_with("a.rs"))).unwrap();
        let f = FileChange {
            path,
            rel: "a.rs".into(),
            before: before.into(),
            after: after.into(),
            places: 1,
            op: Op::Text,
            protected: false,
        };
        landed(ed, &f);
        match &ed.sound.sink {
            Sink::Record(k) => k.iter().map(|(_, s)| s.kind).collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn an_unseen_landing_settles_and_one_that_only_took_text_out_is_a_backspace() {
        let (mut ed, _dir) = heard("a\nb\n");
        assert_eq!(land(&mut ed, "a\nb\n", "a\nx\nb\n"), [Kind::Settle]);
        let (mut ed, _dir) = heard("a\nb\n");
        assert_eq!(land(&mut ed, "a\nb\n", "a\n"), [Kind::Erase]);
    }

    /// A run of Claude's that rewrote `a.rs` (`motion::run_wrote`): heard as the buffer takes the run's text — the
    /// disk watch's reload a version later, or at once where it has — whatever `editor.motion` says (no wet ink at
    /// `none`, the sound all the same); a pure deletion a backspace; your unsaved edits, another text or out of sight
    /// nothing.
    #[test]
    fn a_run_that_rewrote_an_open_buffer_settles_as_the_buffer_takes_it() {
        let blob = |t: &str| Some(crate::snapshot::Blob { id: String::new(), text: Some(t.into()) });
        let change = |ed: &Editor, was: &str, now: &str| crate::snapshot::Change {
            path: ed.doc().path.clone().unwrap(),
            before: blob(was),
            after: blob(now),
        };
        let kinds = |ed: &Editor| match &ed.sound.sink {
            Sink::Record(k) => k.iter().map(|(_, s)| s.kind).collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        let calm = |ed: &mut Editor| crate::typed::execute(ed, "set editor.motion none").unwrap();
        let (mut ed, _dir) = heard("a\n");
        calm(&mut ed);
        let c = change(&ed, "a\n", "a\nb\n");
        ed.run_wrote(std::slice::from_ref(&c));
        crate::motion::wet_sync(&mut ed);
        assert_eq!(kinds(&ed), [], "the buffer hasn't taken it yet");
        crate::disk::reload_text(ed.doc_mut(), &ropey::Rope::from_str("a\nb\n"));
        crate::motion::wet_sync(&mut ed);
        assert_eq!(kinds(&ed), [Kind::Settle], "taken by the reload: heard");
        assert!(ed.doc().wet.is_none(), "motion none: no wet ink, the sound all the same");
        let (mut ed, _dir) = heard("a\n");
        let c = change(&ed, "a\nb\n", "a\n");
        ed.run_wrote(std::slice::from_ref(&c));
        assert_eq!(kinds(&ed), [Kind::Erase], "taken already, only text out: a backspace at once");
        let (mut ed, _dir) = heard("a\n");
        keys(&mut ed, &["i", "x", "esc"]);
        ed.run_wrote(&[change(&ed, "a\n", "a\nb\n")]);
        crate::motion::wet_sync(&mut ed);
        assert!(
            kinds(&ed).is_empty() && ed.wet_waiting.is_empty(),
            "your unsaved edits: nothing taken, nothing heard"
        );
        let (mut ed, _dir) = heard("a\n");
        ed.run_wrote(&[change(&ed, "a\n", "a\nb\n")]);
        crate::disk::reload_text(ed.doc_mut(), &ropey::Rope::from_str("other\n"));
        crate::motion::wet_sync(&mut ed);
        assert_eq!(kinds(&ed), [], "another text: let go, silent");
        let (mut ed, dir) = heard("a\n");
        let c = change(&ed, "a\n", "a\nb\n");
        crate::disk::reload_text(ed.doc_mut(), &ropey::Rope::from_str("a\nb\n"));
        ed.open(&dir.join("b.rs")).unwrap();
        ed.handle_event(crate::event::Event::Job(Box::new(|_: &mut Editor| {})));
        drawn(&mut ed);
        ed.run_wrote(&[c]);
        assert_eq!(kinds(&ed), [], "out of sight: silent");
    }

    #[test]
    fn a_landing_out_of_sight_or_with_sound_off_or_a_move_is_silent() {
        let (mut ed, dir) = heard("a\n");
        ed.open(&dir.join("b.rs")).unwrap();
        // (the pane's view follows at the event's end)
        ed.handle_event(crate::event::Event::Job(Box::new(|_: &mut Editor| {})));
        drawn(&mut ed);
        assert_eq!(land(&mut ed, "a\n", "a\nb\n"), []);
        let (mut ed, _dir) = heard("a\n");
        ed.config.sound = false;
        assert_eq!(land(&mut ed, "a\n", "a\nb\n"), []);
        let (mut ed, _dir) = heard("a\n");
        let f = FileChange {
            path: ed.doc().path.clone().unwrap(),
            rel: "a.rs".into(),
            before: "a\n".into(),
            after: "a\n".into(),
            places: 1,
            op: Op::Delete,
            protected: false,
        };
        landed(&mut ed, &f);
        assert!(matches!(&ed.sound.sink, Sink::Record(k) if k.is_empty()));
    }

    /// What's in sight is what the last frame drew: `a.rs` beside `b.rs` is heard; with `b.rs`'s pane zoomed
    /// (`C-w z`) `a.rs` isn't drawn, so its landing is silent; zoomed back, heard again.
    #[test]
    fn a_pane_zoomed_away_is_silent() {
        let (mut ed, dir) = heard("a\n");
        keys(&mut ed, &["C-w", "v"]);
        ed.open(&dir.join("b.rs")).unwrap();
        ed.handle_event(crate::event::Event::Job(Box::new(|_: &mut Editor| {})));
        drawn(&mut ed);
        assert_eq!(ed.workspace.views.len(), 2, "two panes");
        assert_eq!(land(&mut ed, "a\n", "a\nb\n"), [Kind::Settle], "a.rs beside b.rs: heard");
        keys(&mut ed, &["C-w", "z"]);
        assert!(ed.doc().path.as_ref().is_some_and(|p| p.ends_with("b.rs")), "b.rs's pane zoomed");
        assert_eq!(land(&mut ed, "a\n", "a\nb\n"), [Kind::Settle], "zoomed away: silent");
        keys(&mut ed, &["C-w", "z"]);
        assert_eq!(land(&mut ed, "a\n", "a\nb\n"), [Kind::Settle, Kind::Settle], "back in sight: heard");
    }

    /// An output is opened only for an interface whose speaker plays tarae's sound (`Editor.interface.sound`) — a
    /// tests' editor's has none; given one, it's asked for (its opener a stand-in that refuses: no device made in a
    /// test). Off lets it go.
    #[test]
    fn a_device_is_asked_only_for_an_interface_with_sound_and_off_lets_it_go() {
        let (mut ed, _dir) = heard("a\n");
        ed.sound.sink = Sink::Off;
        let mut c = crate::chat::Chat::new();
        c.state = crate::chat::State::Thinking;
        ed.workspace.claude.chat = Some(c);
        sync(&mut ed);
        assert!(
            !ed.sound.opening && matches!(ed.sound.sink, Sink::Off),
            "opened for an interface with no sound"
        );
        ed.sound.open = |_| Err("no device in tests".into());
        ed.interface.sound = Speaker::Here;
        sync(&mut ed);
        assert!(ed.sound.opening, "its speaker's output asked for");
        let end = std::time::Instant::now() + crate::test_results::PATIENCE;
        while ed.sound.opening {
            let left = end.saturating_duration_since(std::time::Instant::now());
            let ev = ed.events.recv_timeout(left).expect("the opening's answer");
            ed.handle_event(ev);
        }
        assert!(ed.sound.failed && matches!(ed.sound.sink, Sink::Off), "no device made in a test");
        let (tx, _rx) = std::sync::mpsc::channel();
        ed.sound.sink = Sink::Device(tx);
        ed.config.sound = false;
        sync(&mut ed);
        assert!(matches!(ed.sound.sink, Sink::Off), "its channel kept with sound off");
    }

    /// An attached interface's speaker opens nothing here: what's decided goes to it across its wire, behind no frame —
    /// its volume first, and again for the next interface attached; none that plays, and it's let go.
    #[test]
    fn an_interfaces_speaker_is_told_across_its_wire() {
        use tarae_wire::{Order, Sound, ToInterface};
        let mut ed = Editor::new(crate::config::Config::default());
        ed.config.sound = true;
        ed.config.sound_volume = 40;
        ed.sound.sink = Sink::Off;
        ed.sound.open = |_| panic!("no output opened here for an interface's speaker");
        let mut c = crate::chat::Chat::new();
        c.state = crate::chat::State::Thinking;
        ed.workspace.claude.chat = Some(c);
        ed.interface.sound = Speaker::Wire;
        ed.attach.current = 3;
        let sounds = |ed: &mut Editor| -> Vec<(u64, String)> {
            let out = std::mem::take(&mut ed.attach.out).into_iter();
            out.map(|(to, m, order)| {
                assert_eq!(order, Order::AtOnce, "a sound waits for no frame");
                let ToInterface::Sound(s) = m else { panic!("{m:?}") };
                (to, format!("{s:?}"))
            })
            .collect()
        };
        sync(&mut ed);
        assert!(matches!(ed.sound.sink, Sink::Wire(3)) && !ed.sound.opening);
        assert_eq!(sounds(&mut ed), [(3, format!("{:?}", Sound::Volume(40)))]);
        settle(&mut ed, std::path::Path::new("/nowhere"), &"a".into(), &"b".into());
        wake(&mut ed);
        assert_eq!(sounds(&mut ed), [(3, format!("{:?}", Sound::Wake))], "nothing in sight settles");
        ed.attach.current = 4;
        sync(&mut ed);
        assert_eq!(sounds(&mut ed), [(4, format!("{:?}", Sound::Volume(40)))], "the next one told anew");
        ed.interface.sound = Speaker::None;
        sync(&mut ed);
        assert!(matches!(ed.sound.sink, Sink::Off));
        // (the wire's words for each kind come back as they went)
        for kind in engine::KINDS {
            let s = Said { kind, gain: 0.5, seed: 7, delay: 0.25 };
            let Some(Msg::Play(back)) = msg_of(wire_of(Msg::Play(s))) else { panic!() };
            assert_eq!(back, s);
        }
        assert!(msg_of(Sound::Play { kind: 9, gain: 1.0, seed: 0, delay: 0.0 }).is_none(), "an unknown kind");
    }
}
