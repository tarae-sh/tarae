//! What Claude's writing sounds like as it appears: the reveal's steps in (`writing::reveal` — what each frame newly
//! shows), the sounds out — a key, the space bar, a return — as a person types, never the stream's even pace.
//!
//! The reveal shows ~300 characters a second in 40 steps (`writing::REVEAL_EVERY`); a person types a tenth of that.
//! So the hand keeps its own time and strikes for what's just been shown, never `LATE_MAX` after it (which
//! characters get a sound is its freedom): bursts inside a word (`WORD`, a gap spread over 65–140 ms, never a grid),
//! the space bar or a punctuation key as a word it's typing ends (`ENDING`), a longer, uneven gap before the next
//! word (`NEXT`) and the next line (`LINE`), now and then a hesitation there (`HESITATE`). A word is a few keys; it
//! ends at the line's end, space or punctuation shown when its last key is done (a return, the space bar, a key —
//! a line's end waited for a moment, `HOLD`), else it runs on. Returns come at most every `RETURN_GAP` (code's
//! short lines end ~10 times a second at that pace). The same gaps type a text a person types (`typing`). Pure — std only (`cargo xtask sound` includes it); its time is the
//! caller's (`clock::now` in tarae).

use std::time::{Duration, Instant};

use super::said::{Kind, Said};

/// The shortest gap between any two sounds (a landing after the last key).
pub const GAP_MIN: Duration = Duration::from_millis(40);

/// The shortest gap between two returns: four a second.
pub const RETURN_GAP: Duration = Duration::from_millis(250);

/// The farthest a sound is from the character it stands for.
pub const LATE_MAX: Duration = Duration::from_millis(100);

/// A gap before a stroke (ms, shortest and longest): a key after a key inside a word — 16 a second at most.
const WORD: (f32, f32) = (65.0, 140.0);
/// … the space bar, punctuation or return after a word's last key.
const ENDING: (f32, f32) = (90.0, 170.0);
/// … a word's first key after a space or punctuation.
const NEXT: (f32, f32) = (150.0, 350.0);
/// … a line's first key after return.
const LINE: (f32, f32) = (200.0, 400.0);
/// How often a word or line begins after a hesitation, and how much longer it is (ms).
const HESITATE: (f32, f32, f32) = (0.06, 250.0, 600.0);
/// How long a word's end waits for a line's end to be shown (before it takes a space or punctuation): code's short
/// lines end in returns, prose's long ones in space bars.
const HOLD: Duration = Duration::from_millis(60);
/// Reaching for shift (ms): a capital's gap is this much longer.
const SHIFT: (f32, f32) = (20.0, 45.0);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Gap {
    Word,
    Ending,
    Next,
    Line,
}

pub struct Typist {
    /// When the hand strikes next (on the first step after it that shows something).
    ready: Option<Instant>,
    /// When the last sound is heard.
    last: Option<Instant>,
    /// When the last return is heard.
    returned: Option<Instant>,
    /// Keys left in the word it's typing (at 0 it ends the word), and the keys struck in it.
    word: (u32, u32),
    /// The last character shown (a step's first space ends a word only after a character that isn't one).
    shown: char,
    rng: u32,
}

impl Typist {
    pub fn new(seed: u32) -> Typist {
        let mut t =
            Typist { ready: None, last: None, returned: None, word: (0, 0), shown: '\n', rng: seed | 1 };
        t.word.0 = t.word_keys();
        t
    }

    /// A number in 0 ‥ 1.
    fn unit(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1 << 24) as f32
    }

    /// How many keys the next word is: 2 to 6.
    fn word_keys(&mut self) -> u32 {
        2 + (self.unit() * 5.0) as u32
    }

    /// A gap of its kind: spread about its middle, a word's or line's start sometimes a hesitation.
    fn gap(&mut self, gap: Gap) -> Duration {
        let (lo, hi) = match gap {
            Gap::Word => WORD,
            Gap::Ending => ENDING,
            Gap::Next => NEXT,
            Gap::Line => LINE,
        };
        let mut ms = lo + (hi - lo) * (self.unit() + self.unit()) / 2.0;
        if matches!(gap, Gap::Next | Gap::Line) && self.unit() < HESITATE.0 {
            ms += HESITATE.1 + (HESITATE.2 - HESITATE.1) * self.unit();
        }
        Duration::from_secs_f32(ms / 1000.0)
    }

    /// A step of the reveal at `now` showed `text` (what was newly shown; the next step `step` later): its sound, if
    /// the hand strikes within it. Its `delay` is from `now`, inside the step and under `LATE_MAX`.
    pub fn step(&mut self, now: Instant, text: &str, step: Duration) -> Option<Said> {
        let last = text.chars().last()?;
        let shown = std::mem::replace(&mut self.shown, last);
        let mut at = self.ready.map_or(Duration::ZERO, |r| r.saturating_duration_since(now));
        if at >= step.min(LATE_MAX) {
            return None;
        }
        if at.is_zero() {
            // (the hand was waiting for something to strike for: not on the step's beat)
            at = Duration::from_millis(12).mul_f32(self.unit());
        }
        // (what it shows that ends a word: a line's end, a space after a character, punctuation)
        let ending = self.word.0 == 0;
        let can_return = self.returned.is_none_or(|r| now + at >= r + RETURN_GAP);
        let line = ending && can_return && text.contains('\n');
        let waited = self.ready.map_or(HOLD, |r| (now + at).saturating_duration_since(r));
        if ending && can_return && !line && waited < HOLD {
            return None;
        }
        let mut before = shown;
        let space = text.chars().any(|c| {
            let gap = c == ' ' && !before.is_whitespace();
            before = c;
            gap
        });
        let punct = text.chars().any(|c| c.is_ascii_punctuation());
        let (kind, next, gain) = if line {
            self.returned = Some(now + at);
            (Kind::Return, Gap::Line, 1.0)
        } else if ending && space {
            (Kind::Space, Gap::Next, 1.0)
        } else if ending && punct {
            (Kind::Key, Gap::Next, 0.95)
        } else {
            // (a word's first key a little harder)
            let gain = if self.word.1 == 0 { 1.0 } else { 0.82 + 0.13 * self.unit() };
            self.word.0 = self.word.0.saturating_sub(1);
            self.word.1 += 1;
            (Kind::Key, if self.word.0 == 0 { Gap::Ending } else { Gap::Word }, gain)
        };
        if next != Gap::Word && next != Gap::Ending {
            self.word = (self.word_keys(), 0);
        }
        let gap = self.gap(next);
        self.ready = Some(now + at + gap);
        Some(self.said(now, at, kind, gain))
    }

    /// One sound at `now` (an edit landing): `GAP_MIN` after the last, and the hand's next key `GAP_MIN` after it (an
    /// edit still streaming types on past it).
    pub fn once(&mut self, now: Instant, kind: Kind) -> Said {
        let at = self.last.map_or(Duration::ZERO, |t| (t + GAP_MIN).saturating_duration_since(now));
        let next = now + at + GAP_MIN;
        self.ready = Some(self.ready.map_or(next, |r| r.max(next)));
        self.said(now, at, kind, 1.0)
    }

    fn said(&mut self, now: Instant, at: Duration, kind: Kind, gain: f32) -> Said {
        self.last = Some(now + at);
        Said { kind, gain, seed: self.seed(), delay: at.as_secs_f32() }
    }

    fn seed(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }
}

/// A person typing `text`, every character a stroke: when each is struck (the first at 0) and its sound — the same
/// gaps as `Typist`'s, by what's typed: inside a word, the word's end, the next word, the next line, a capital.
#[allow(dead_code, reason = "the tests' and `cargo xtask sound`'s")]
pub fn typing(text: &str, seed: u32) -> Vec<(Duration, Said)> {
    let mut t = Typist::new(seed);
    let (mut at, mut before, mut out) = (Duration::ZERO, '\n', Vec::new());
    for c in text.chars() {
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let (kind, gap) = match c {
            '\n' => (Kind::Return, Gap::Ending),
            ' ' => (Kind::Space, if word(before) { Gap::Ending } else { Gap::Word }),
            _ if before == '\n' => (Kind::Key, Gap::Line),
            _ if word(c) && !word(before) => (Kind::Key, Gap::Next),
            _ if !word(c) && word(before) => (Kind::Key, Gap::Ending),
            _ => (Kind::Key, Gap::Word),
        };
        if !out.is_empty() {
            at += t.gap(gap);
            if c.is_uppercase() {
                at += Duration::from_secs_f32((SHIFT.0 + (SHIFT.1 - SHIFT.0) * t.unit()) / 1000.0);
            }
        }
        let gain = if gap == Gap::Word { 0.82 + 0.13 * t.unit() } else { 1.0 };
        out.push((at, Said { kind, gain, seed: t.seed(), delay: 0.0 }));
        before = c;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: Duration = Duration::from_millis(25);

    /// A sound's delay is an f32 of seconds: its time is that precise.
    const EPS: Duration = Duration::from_micros(10);

    /// A step: its time and text, its sound and when that's heard.
    type Step = (Duration, String, Option<(Duration, Said)>);

    /// 300 characters a second of `text`, 8 a step.
    fn stepped(text: &str) -> Vec<Step> {
        let mut t = Typist::new(7);
        let start = Instant::now();
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        for (k, step) in chars.chunks(8).enumerate() {
            let now = start + STEP * k as u32;
            let s: String = step.iter().collect();
            let said =
                t.step(now, &s, STEP).map(|said| (now - start + Duration::from_secs_f32(said.delay), said));
            out.push((now - start, s, said));
        }
        out
    }

    fn typed(text: &str) -> Vec<(Duration, Said)> {
        stepped(text).into_iter().filter_map(|(_, _, s)| s).collect()
    }

    #[test]
    fn keys_come_as_a_person_types_and_stay_with_the_text() {
        let text = "/// Every name is wound into the skein, one after another, as it comes in from the list.\nfn main() {\n    let skein = Skein::new();\n    for name in names {\n        skein.wind(name);\n    }\n}\n"
            .repeat(12);
        let steps = stepped(&text);
        let said = typed(&text);
        // Each sound within its step, for what that step showed: a return only for a line's end
        for (at, s, said) in &steps {
            if let Some((heard, said)) = said {
                assert!(*heard < *at + STEP.min(LATE_MAX), "{said:?} at {heard:?} for {s:?} shown at {at:?}");
                assert!(said.kind != Kind::Return || s.contains('\n'), "a return for {s:?}");
            }
        }
        let returns: Vec<Duration> =
            said.iter().filter(|(_, s)| s.kind == Kind::Return).map(|(at, _)| *at).collect();
        assert!(
            returns.windows(2).all(|w| w[1] - w[0] + EPS >= RETURN_GAP),
            "returns too close: {returns:?}"
        );
        assert!(returns.len() >= 3, "only {} returns in {} sounds", returns.len(), said.len());
        let keys: Vec<Duration> =
            said.iter().filter(|(_, s)| s.kind != Kind::Return).map(|(at, _)| *at).collect();
        // (at most 16 keys in any second)
        for (i, at) in keys.iter().enumerate() {
            let within = keys[i..].iter().take_while(|k| **k - *at < Duration::from_secs(1)).count();
            assert!(within <= 16, "{within} keys in a second from {at:?}");
        }
        assert!(said.iter().any(|(_, s)| s.kind == Kind::Space), "no space bar in {} sounds", said.len());
        assert!(keys.len() > 10, "only {} keys", keys.len());
        // A person's rhythm, not a grid: bursts inside a word, a longer wait after a word or a line
        let gaps: Vec<(Kind, f32)> =
            said.windows(2).map(|w| (w[0].1.kind, (w[1].0 - w[0].0).as_secs_f32() * 1000.0)).collect();
        assert!(gaps.iter().all(|(_, ms)| *ms + 0.01 >= WORD.0), "a gap under {}: {gaps:?}", WORD.0);
        for (kind, ms) in &gaps {
            let least = match kind {
                Kind::Space => NEXT.0,
                Kind::Return => LINE.0,
                _ => continue,
            };
            assert!(*ms + 0.01 >= least, "{ms} ms after {kind:?}");
        }
        let inside: Vec<f32> = gaps.iter().filter(|(_, ms)| *ms <= WORD.1).map(|(_, ms)| *ms).collect();
        assert!(inside.len() * 2 > gaps.len(), "few bursts: {gaps:?}");
        let mean = inside.iter().sum::<f32>() / inside.len() as f32;
        let spread = (inside.iter().map(|ms| (ms - mean).powi(2)).sum::<f32>() / inside.len() as f32).sqrt();
        assert!(spread > 10.0, "inside a word every {mean} ± {spread} ms: a grid");
    }

    #[test]
    fn a_person_types_in_bursts_and_waits_at_a_words_end() {
        let text = "The quick brown fox jumps over the lazy dog. Then it rests.\n";
        let typed = typing(text, 3);
        assert_eq!(typed.len(), text.chars().count());
        let chars: Vec<char> = text.chars().collect();
        let word = |c: char| c.is_alphanumeric();
        for (i, w) in typed.windows(2).enumerate() {
            let (a, b) = (chars[i], chars[i + 1]);
            let ms = (w[1].0 - w[0].0).as_secs_f32() * 1000.0;
            if word(a) && word(b) && !b.is_uppercase() {
                assert!((WORD.0..=WORD.1 + 0.01).contains(&ms), "{a}{b}: {ms} ms");
            }
            if !word(a) && word(b) {
                assert!(ms + 0.01 >= NEXT.0, "{a:?} then {b}: {ms} ms");
            }
        }
        let kinds: Vec<Kind> = typed.iter().map(|(_, s)| s.kind).collect();
        assert_eq!(kinds.iter().filter(|k| **k == Kind::Space).count(), 11);
        assert_eq!(kinds.last(), Some(&Kind::Return));
    }

    /// An edit lands right after a key while the next one streams on: the settle, then that edit's keys, every one
    /// `GAP_MIN` after the sound before it — the landing moves the hand too.
    #[test]
    fn a_landing_mid_stream_keeps_its_gap_from_the_next_key() {
        let text = "let skein = Skein::new(names); for name in names { skein.wind(name); }\n".repeat(4);
        for seed in 0..64 {
            let mut t = Typist::new(seed * 2 + 1);
            let start = Instant::now();
            let mut heard: Vec<(Duration, Kind)> = Vec::new();
            let mut landed = false;
            for (k, step) in text.chars().collect::<Vec<_>>().chunks(8).enumerate() {
                let now = start + STEP * k as u32;
                let s: String = step.iter().collect();
                if let Some(said) = t.step(now, &s, STEP) {
                    heard.push((now - start + Duration::from_secs_f32(said.delay), said.kind));
                    // (the first edit lands at the step after the first key — the next a word's next key)
                    if heard.len() == 1 && !landed {
                        landed = true;
                        let then = now + STEP;
                        let s = t.once(then, Kind::Settle);
                        heard.push((then - start + Duration::from_secs_f32(s.delay), s.kind));
                    }
                }
            }
            heard.sort_by_key(|h| h.0);
            let close = heard.windows(2).find(|w| w[1].0 + EPS < w[0].0 + GAP_MIN);
            assert!(close.is_none(), "seed {seed}: {close:?} closer than GAP_MIN");
        }
    }

    #[test]
    fn the_same_seed_types_the_same() {
        let text = "let a = 1;\nlet b = 2;\n".repeat(4);
        let a: Vec<_> = typed(&text).into_iter().map(|(t, s)| (t, s.kind, s.seed)).collect();
        let b: Vec<_> = typed(&text).into_iter().map(|(t, s)| (t, s.kind, s.seed)).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn nothing_shown_is_silent() {
        assert!(Typist::new(1).step(Instant::now(), "", STEP).is_none());
    }
}
