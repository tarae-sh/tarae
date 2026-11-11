//! The mascot at the keys (main's `.sb/full-video-mascot-brief.md`, piece 2): the ball of thread at a small keyboard
//! at the band's right end, its hands tapping where the cut's sound hears a key — tarae's screen stays the focus.
//!
//! - **Its frames**: five pictures made at four times their size (main's `.sb/youtube/mascot-keys/out/`, found as the
//!   intro's swing is: `TARAE_MASCOT_KEYS`, else by the repository's common git folder), shrunk once to `BOX`. Not
//!   found, or hanji's ground (they're meok's only): the cut is made without it, and says so.
//! - **A tap** where a key is heard (`sound::sounds`' own list — picture and sound agree), on the key's side of a
//!   QWERTY (`side`): both hands on a return and where a card is answered. A hand stays down `HOLD_MS`; the same hand
//!   asked again before it's up comes up for one frame first (`Hands`), so fast typing reads as taps.
//! - **Watching** where no key has been heard for `QUIET_MS`, and on every span too fast for its keys to be heard: the
//!   eyes raised to the screen. Every change of pose snaps; nothing else moves.
//! - **Where**: on the take's frames and where it ends, never on a card; its box's right edge at the band's rule's end,
//!   its foot `FOOT` above the frame's, its head over the rule — the rule stopping short of its ink (`RULE_GAP`) — and
//!   never over a cell (`place`). The band's marks keep `GAP` from its box.

use std::path::PathBuf;

use super::sound::KEYS_MAX_SPEED;
use super::{Cut, Frame, Program};
use crate::Result;
use crate::scenes::docshots::Bg;
use crate::shot::PAD;
use crate::video::{self, BAND, FPS, H, Rgba};

/// The box at 1080p (px): the frames are made at four times it.
pub(crate) const BOX: (usize, usize) = (213, 165);
const MADE: usize = 4;
/// The box's foot over the frame's bottom (px).
const FOOT: usize = 14;
/// The band's marks (their letters) this far left of the box (px).
pub(crate) const GAP: usize = 22;
/// The band's rule stops this far short of the art's ink where the two meet (px).
const RULE_GAP: usize = 26;
/// A hand stays down this long — at `FPS`, whole frames (90 ms: three).
pub(crate) const HOLD_MS: u64 = 90;
/// No key heard this long: watching.
pub(crate) const QUIET_MS: u64 = 1_500;
/// The pictures, in `Pose`'s order (`frame-NAME.png`).
const NAMES: [&str; 5] = ["up", "left", "right", "both", "watch"];

/// What the mascot shows: both hands up, the left down (the viewer's left), the right, both, or watching the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Pose {
    Up,
    Left,
    Right,
    Both,
    Watch,
}

/// The hand a key asks for: the side of a QWERTY it's on, both, or (the space bar) the hand that didn't tap last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Left,
    Right,
    Both,
    Space,
}

/// A key's side, from its bytes as the take keeps them: letters and digits by a QWERTY's halves (shifted ones too);
/// escape and tab left; a return both; the space bar its own; backspace, arrows and the rest right. The keyboard
/// protocol's `ESC [ code … u` by its code, a control key by its letter, an alt key by its character, a line the key
/// script types at once by its first.
pub(crate) fn side(b: &[u8]) -> Side {
    if super::init::is_return(b) {
        return Side::Both;
    }
    if let Some(body) = b.strip_prefix(b"\x1b[").and_then(|r| r.strip_suffix(b"u")) {
        let code: String = body.iter().take_while(|c| c.is_ascii_digit()).map(|&c| c as char).collect();
        return match code.parse::<u32>() {
            Ok(27 | 9) => Side::Left,
            Ok(32) => Side::Space,
            Ok(c) => char::from_u32(c).map_or(Side::Right, of_char),
            Err(_) => Side::Right,
        };
    }
    match b {
        [] => Side::Right,
        [0x1b] | [0x09, ..] => Side::Left,
        [0x1b, b'[' | b'O', ..] => Side::Right,
        [0x1b, rest @ ..] => side(rest),
        [b' ', ..] => Side::Space,
        [c @ 1..=26, ..] => of_char(char::from(c + 96)),
        _ => String::from_utf8_lossy(b).chars().next().map_or(Side::Right, of_char),
    }
}

/// A character's side of a QWERTY.
fn of_char(c: char) -> Side {
    match "`12345qwertasdfgzxcvb~!@#$%".contains(c.to_ascii_lowercase()) {
        true => Side::Left,
        false => Side::Right,
    }
}

/// The hands through the video: each hand's down stretches (frames, from–to) and every tap's frame, in order.
#[derive(Debug)]
pub(crate) struct Hands {
    down: [Vec<(u64, u64)>; 2],
    taps: Vec<u64>,
}

impl Hands {
    /// From the taps (video µs, side): each at the first frame at or after it, down `HOLD_MS` or until one frame before
    /// its hand's next — a tap with no room for that one frame up is moved to the frame after it.
    pub(crate) fn new(taps: &[(u64, Side)]) -> Hands {
        let hold = ((HOLD_MS * FPS) as f64 / 1000.0).round() as u64;
        let mut sorted = taps.to_vec();
        sorted.sort_by_key(|t| t.0);
        let (mut at, mut all): ([Vec<u64>; 2], Vec<u64>) = ([Vec::new(), Vec::new()], Vec::new());
        let mut last = None;
        for (v, s) in sorted {
            let s = match (s, last) {
                (Side::Space, Some(Side::Left)) => Side::Right,
                (Side::Space, Some(Side::Right)) => Side::Left,
                (Side::Space, _) => Side::Right,
                (s, _) => s,
            };
            last = Some(s);
            let hands: &[usize] = match s {
                Side::Left => &[0],
                Side::Right => &[1],
                _ => &[0, 1],
            };
            let f = (hands.iter())
                .filter_map(|&h| at[h].last().map(|p| p + 2))
                .fold(Program::frame_at(v), u64::max);
            for &h in hands {
                at[h].push(f);
            }
            all.push(f);
        }
        all.sort_unstable();
        let down = at.map(|fs| {
            (fs.iter().enumerate())
                .map(|(i, &f)| (f, fs.get(i + 1).map_or(f + hold, |&n| (f + hold).min(n - 1))))
                .collect()
        });
        Hands { down, taps: all }
    }

    /// How many taps.
    pub(crate) fn taps(&self) -> usize {
        self.taps.len()
    }

    /// The pose at frame `k`; `fast` where its span is too fast for its keys to be heard.
    pub(crate) fn pose(&self, k: u64, fast: bool) -> Pose {
        let down = |h: usize| {
            let d = &self.down[h];
            let i = d.partition_point(|s| s.0 <= k);
            i > 0 && k < d[i - 1].1
        };
        match (down(0), down(1)) {
            (true, true) => Pose::Both,
            (true, false) => Pose::Left,
            (false, true) => Pose::Right,
            (false, false) => {
                let quiet = QUIET_MS * FPS / 1000;
                let i = self.taps.partition_point(|&f| f <= k);
                match fast || i == 0 || k - self.taps[i - 1] >= quiet {
                    true => Pose::Watch,
                    false => Pose::Up,
                }
            }
        }
    }
}

/// The five pictures at the box's size, in `Pose`'s order, and where they were found.
pub(crate) struct Art {
    pub(crate) poses: Vec<Rgba>,
    pub(crate) dir: PathBuf,
}

/// Where the frames are: `TARAE_MASCOT_KEYS`, else main's `.sb/youtube/mascot-keys/out` — the main checkout's, by the
/// repository's common git folder (outside git, where `cargo clean` can't take it).
pub(crate) fn dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("TARAE_MASCOT_KEYS") {
        return Some(PathBuf::from(d));
    }
    Some(crate::linux::git_folder(&crate::root())?.parent()?.join(".sb/youtube/mascot-keys/out"))
}

impl Art {
    /// The frames for `bg` in `dir`: meok's only.
    pub(crate) fn load(bg: Bg, dir: Option<PathBuf>) -> Result<Art> {
        if bg == Bg::Light {
            return Err(
                "its frames are meok's only (pale ink on the dark ground), and this cut is hanji's".into()
            );
        }
        let dir = dir.ok_or("no main checkout found")?;
        let poses = (NAMES.iter())
            .map(|n| {
                let p = dir.join(format!("frame-{n}.png"));
                let f = video::rgba(&p)?;
                if (f.0, f.1) != (BOX.0 * MADE, BOX.1 * MADE) {
                    return Err(format!(
                        "{}: {}×{}, not {}×{}",
                        p.display(),
                        f.0,
                        f.1,
                        BOX.0 * MADE,
                        BOX.1 * MADE
                    ));
                }
                Ok(video::shrunk(&f, 1.0 / MADE as f32))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Art { poses, dir })
    }

    pub(crate) fn picture(&self, p: Pose) -> &Rgba {
        &self.poses[p as usize]
    }

    /// The leftmost ink of any pose within `rows` of the box's row `row` (alpha over a tenth), from its left.
    fn ink_from(&self, row: usize, rows: usize) -> Option<usize> {
        (self.poses.iter())
            .filter_map(|(w, h, px)| {
                (row.saturating_sub(rows)..(row + rows + 1).min(*h))
                    .filter_map(|y| (0..*w).find(|x| px[(y * w + x) * 4 + 3] > 25))
                    .min()
            })
            .min()
    }
}

/// The mascot for `cut` (its frames in `dir`), placed under a screen drawn at `x0`, `pw` wide, its cells ending at
/// `cells_bottom` — or why there's none, as `cut.txt` says it.
pub(crate) fn found(
    cut: &Cut,
    dir: Option<PathBuf>,
    screen: (usize, usize),
    cells_bottom: usize,
) -> std::result::Result<(Art, Place), String> {
    if !cut.mascot {
        return Err("the mascot: none (mascot = false)".into());
    }
    let got = Art::load(cut.bg, dir).and_then(|a| Ok((place(&a, screen, cells_bottom)?, a)));
    got.map(|(p, a)| (a, p)).map_err(|e| format!("the mascot: none — {e}; the cut is made without it"))
}

/// The pose on frame `k` showing `f`: none on a card; watching where its span is too fast for its keys to be heard.
pub(crate) fn pose_at(hands: &Hands, k: u64, f: &Frame) -> Option<Pose> {
    let fast = f.speed.is_some_and(|s| s > KEYS_MAX_SPEED + 1e-6);
    f.card.is_none().then(|| hands.pose(k, fast))
}

/// Where the mascot sits on a frame: its box's top left, where the rule stops, and where the band's right marks end
/// (their pictures', margin and all).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Place {
    pub(crate) x: usize,
    pub(crate) y: usize,
    pub(crate) rule_end: usize,
    pub(crate) marks_right: usize,
}

/// The box placed under a screen drawn at `x0`, `pw` wide, its cells ending at `cells_bottom` (px): right on the rule's
/// end, its foot `FOOT` over the frame's — refused (said) where it would rise over a cell.
pub(crate) fn place(art: &Art, (x0, pw): (usize, usize), cells_bottom: usize) -> Result<Place> {
    let (x, y) = (x0 + pw - PAD - BOX.0, H - FOOT - BOX.1);
    if y < cells_bottom {
        return Err(format!(
            "its box would rise {} px over the screen's last row of cells",
            cells_bottom - y
        ));
    }
    let rule = H - BAND;
    let rule_end = match (rule >= y).then(|| art.ink_from(rule - y, 3)).flatten() {
        Some(ink) => (x + ink).saturating_sub(RULE_GAP),
        None => x0 + pw - PAD,
    };
    Ok(Place { x, y, rule_end, marks_right: x - GAP + PAD })
}
