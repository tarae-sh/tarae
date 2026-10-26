//! The ASMR mode (`editor.sound`, off by default, yours only): Claude's writing heard as it appears. A sound says
//! something or it's silent, as motion: keys as characters appear at the writing head, the space bar for a gap between
//! words, return as a line ends (`writing::reveal` — what each step newly shows, `typist.rs`'s pace), a soft thunk as
//! an edit lands unseen, backspace when it only took text out (`refactor.rs`'s review at the end). Only the front
//! workspace's Claude, only in a buffer of a pane in front; never your keys, nor your `y` (the stamp says that), nor
//! reading, runs or thinking; the chat's prose silent for now.
//!
//! The decision is here, on the main loop's clock (`clock::now`: the simulation runs it); the sound is the audio
//! thread's (`engine.rs`, `mac.rs`), only told — the main loop never waits on it. It's made the first time a turn runs
//! with sound on, on a worker (opening the output takes ~100 ms), and lives until quit or `editor.sound` goes off.
//! In tests no thread or device is made (`State.devices`); a test records what would sound (`Sink::Record`).

mod engine;
#[cfg(target_os = "macos")]
mod mac;
pub mod said;
// (what plays a sound: only macOS's output does yet — Linux's ALSA is sound-plan §5 step 3)
#[cfg(target_os = "macos")]
pub mod synth;
pub mod typist;

use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::document::DocId;
use crate::editor::Editor;
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
    /// Whether a device may be opened — off in tests, as the animation is.
    pub devices: bool,
}

impl Default for State {
    fn default() -> State {
        State {
            sink: Sink::Off,
            typist: typist::Typist::new(0x5eed),
            opening: false,
            failed: false,
            told: 0,
            devices: !cfg!(test),
        }
    }
}

/// Whether Claude's writing is heard now: `editor.sound`, and not a session behind's result being applied.
pub fn on(ed: &Editor) -> bool {
    ed.config.sound && !ed.background()
}

/// Whether a sound can be heard now: on, and an output opened — never off macOS (none opens there yet), nor after it
/// failed to open, nor in a test's record.
pub fn playing(ed: &Editor) -> bool {
    on(ed) && matches!(ed.sound.sink, Sink::Device(_))
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
    if let (true, Sink::Device(tx)) = (on(ed), &ed.sound.sink) {
        let _ = tx.send(Msg::Wake);
    }
}

/// Claude's edit landed unseen in `f.path` (review at the end — `refactor::write`), its buffer in sight: the page
/// settling, or backspace when it only took text out. Your `y` lands through another path, and says nothing.
pub fn landed(ed: &mut Editor, f: &crate::refactor::FileChange) {
    use crate::refactor::Op;
    if !on(ed) || !matches!(f.op, Op::Text | Op::Create) {
        return;
    }
    let sight = sight(ed);
    let shown = ed.docs.iter().any(|d| d.path.as_deref() == Some(f.path.as_path()) && sight.contains(&d.id));
    if !shown {
        return;
    }
    let kind = match crate::disk::minimal_change(&f.before, &f.after) {
        None => return,
        Some(c) if c.insert.is_empty() => Kind::Erase,
        Some(_) => Kind::Settle,
    };
    let s = ed.sound.typist.once(crate::clock::now(), kind);
    play(ed, s);
}

fn play(ed: &mut Editor, s: Said) {
    match &mut ed.sound.sink {
        Sink::Device(tx) => {
            let _ = tx.send(Msg::Play(s));
        }
        Sink::Record(kept) => kept.push((crate::clock::now(), s)),
        Sink::Off => {}
    }
}

/// After every event: the thread let go once `editor.sound` is off; made (on a worker) the first time a turn runs
/// with it on; told a volume that changed.
pub fn sync(ed: &mut Editor) {
    let volume = ed.config.sound_volume;
    let st = &mut ed.sound;
    if !ed.config.sound {
        st.failed = false;
        if let Sink::Device(_) = st.sink {
            // (its channel let go: the thread stops the device and ends)
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
    let turn = ed.workspace.claude.chat.as_ref().is_some_and(|c| c.state != crate::chat::State::Idle);
    if !st.devices || st.opening || st.failed || !matches!(st.sink, Sink::Off) || !turn {
        return;
    }
    st.opening = true;
    ed.events.jobs().spawn(move || {
        let opened = engine::spawn(volume);
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

    #[test]
    fn no_device_is_made_in_tests_and_off_lets_it_go() {
        let (mut ed, _dir) = heard("a\n");
        ed.sound.sink = Sink::Off;
        let mut c = crate::chat::Chat::new();
        c.state = crate::chat::State::Thinking;
        ed.workspace.claude.chat = Some(c);
        sync(&mut ed);
        assert!(!ed.sound.opening && matches!(ed.sound.sink, Sink::Off), "a device opened in a test");
        let (tx, _rx) = std::sync::mpsc::channel();
        ed.sound.sink = Sink::Device(tx);
        ed.config.sound = false;
        sync(&mut ed);
        assert!(matches!(ed.sound.sink, Sink::Off), "its channel kept with sound off");
    }
}
