//! What a sound says — the words between the decision (`typist.rs`, `sound.rs`) and what plays it (`synth.rs`, the
//! audio thread): its kind, how hard, its seed, when; and how loud the volume makes it. Everywhere — the playback is
//! macOS's for now. Pure: `cargo xtask sound` includes it.

/// What a sound says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Characters appeared at the writing head.
    Key,
    /// … a gap between words among them: the space bar.
    Space,
    /// A line was finished: the big stabilized key.
    Return,
    /// Claude's edit landed unseen and only took text out: backspace.
    Erase,
    /// Claude's edit landed unseen: a soft low thunk, the page settling.
    Settle,
}

/// One sound to play: `delay` seconds after it's handed over, at `gain` (0 ‥ 1), varied by `seed`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Said {
    pub kind: Kind,
    pub gain: f32,
    pub seed: u32,
    pub delay: f32,
}

/// The master level of `editor.sound-volume` (0 ‥ 100): its square, as a volume slider's taper.
pub fn master(volume: u8) -> f32 {
    let v = volume.min(100) as f32 / 100.0;
    v * v
}
