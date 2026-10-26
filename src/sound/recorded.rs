//! The recorded strokes tarae plays (`sampled.rs`), made by `cargo xtask sound make` and `install trollarch2` —
//! never by hand. Cut from
//! "Keyboard Typing" by trollarch2, https://freesound.org/people/trollarch2/sounds/331656/ (CC0),
//! high-passed at 100 Hz, each faded and normalised: 10 strokes, 92566 bytes.

use super::sampled::{Bank, Stroke};

pub static BANK: Bank =
    Bank { rate: 32000.0, pcm: include_bytes!("recorded.pcm"), letters: &LETTERS, big: &BIG, level: 1.2472 };

const LETTERS: [Stroke; 10] = [
    Stroke { press: (0, 1967), release: (1967, 2623), hold: 145.7 }, // at 9.662 s
    Stroke { press: (4590, 1712), release: (6302, 2927), hold: 53.5 }, // at 13.616 s
    Stroke { press: (9229, 1967), release: (11196, 2927), hold: 146.1 }, // at 11.931 s
    Stroke { press: (14123, 1967), release: (16090, 2857), hold: 154.8 }, // at 6.166 s
    Stroke { press: (18947, 1829), release: (20776, 1153), hold: 123.8 }, // at 9.808 s
    Stroke { press: (21929, 1967), release: (23896, 2927), hold: 159.0 }, // at 9.205 s
    Stroke { press: (26823, 1967), release: (28790, 2927), hold: 128.1 }, // at 14.519 s
    Stroke { press: (31717, 1967), release: (33684, 2927), hold: 104.0 }, // at 2.834 s
    Stroke { press: (36611, 1967), release: (38578, 2811), hold: 140.9 }, // at 0.075 s
    Stroke { press: (41389, 1967), release: (43356, 2927), hold: 109.7 }, // at 12.612 s
];

const BIG: [Stroke; 0] = [];
