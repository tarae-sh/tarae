//! The mascot at the keys (main's `.sb/full-video-mascot-brief.md`, piece 2): the ball of thread at a small keyboard
//! at the band's right end, its hands tapping where the cut's sound hears a key — tarae's screen stays the focus.
//!
//! - **Its frames**: five pictures made at four times their size (main's `.sb/youtube/mascot-keys/out/`, found as the
//!   intro's swing is: `TARAE_MASCOT_KEYS`, else by the repository's common git folder), shrunk once to `BOX`. Not
//!   found, or hanji's ground (they're meok's only): the cut is made without it, and says so.
//! - **A tap** where a key is heard (`sound::sounds`' own list — picture and sound agree), on the key's side of a
//!   QWERTY (`side`): both hands on a return. A hand stays down `HOLD_MS`; the same hand asked again before it's up
//!   comes up for one frame first (`Hands`), so fast typing reads as taps. **A stamp** (a card answered) is one press
//!   of the answering key's own paw, held `STAMP_HOLD_MS` — a seal pressed, not a letter typed —, the other paw up over
//!   the keys (the user, 2026-11-12: one hand on a lone `y`). A card that goes only once its work is done (the
//!   grammar's) is pressed where its answering key is heard (`sound::ANSWER_WAIT_MS`); a stamp with no key taps nothing.
//! - **Watching** where no key has been heard for `QUIET_MS`, and on every span too fast for its keys to be heard: the
//!   eyes raised to the screen. Every change of pose snaps; nothing else moves. (With `[mascot] fast`, a fast span long
//!   enough is pressing fast-forward instead: `motions::fast_holds`.)
//! - **Where**: on the take's frames and where it ends, never on a card; its box's right edge at the band's rule's end,
//!   its foot `FOOT` above the frame's, its head over the rule — the rule stopping short of its ink (`RULE_GAP`) — and
//!   never over a cell (`place`). The band's marks keep `GAP` from its box — from the key pops' lane where they're on.
//! - **Its motions beyond typing** (`motions.rs`, V5: blink, glance, surprise, stretch, brush swing — none broken off)
//!   and **the key pops** beside it (`pops.rs`), each switched in the edit list's `[mascot]` (`Switches`). Every picture
//!   it can show is a `Pic`: hands and a face, or a frame of a whole-body motion; only those its switches use are loaded.

use std::path::PathBuf;

use super::{Cut, Program};
use crate::Result;
use crate::scenes::docshots::Bg;
use crate::shot::PAD;
use crate::video::{self, BAND, FPS, H, Rgba, Scale};

pub(crate) mod motions;
pub(crate) mod pops;

/// The box at 1080p (px): the frames are made at four times it — at 2160p (twice it) still shrunk, by half.
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
/// A stamp's paw stays down this long — at `FPS`, whole frames (300 ms: nine): a seal pressed, not a letter typed (the
/// design session's range, 250–350 ms; the user: one paw on a lone key). The stamp's own sound — the big key's press,
/// the settle's tap about 220 ms on, quiet by about 340 ms — is all under the paw, which lifts as it dies away; at
/// 350 ms (eleven) the paw lay on past it.
pub(crate) const STAMP_HOLD_MS: u64 = 300;
/// No key heard this long: watching.
pub(crate) const QUIET_MS: u64 = 1_500;
/// The key pops' lane (`pops.rs`): this wide (px), its right edge at the box's left; the pops start at the paws' height,
/// `PAWS` under the box's top.
pub(crate) const LANE: usize = 150;
pub(crate) const PAWS: usize = 120;

/// What the mascot shows: both hands up, the left down (the viewer's left), the right, both, or watching the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Pose {
    Up,
    Left,
    Right,
    Both,
    Watch,
}

/// The arms in a picture: typing (both up over the keys, the viewer's left down, the right, both) or resting (watching).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Arms {
    Up,
    Left,
    Right,
    Both,
    Rest,
}

/// The face over the arms: open (on the keys), closed (a blink), half-raised (the turn), raised (on the screen), wide (a
/// surprise over typing hands, no hop).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Face {
    Open,
    Closed,
    Half,
    Raised,
    Wide,
}

/// Every picture the mascot can show: arms and a face (resting arms only raised, half or closed; open and wide over
/// typing ones), or the `n`th frame of the surprise (2), the stretch (5) or the swing (5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Pic {
    At(Arms, Face),
    Surprise(u8),
    Stretch(u8),
    Swing(u8),
}

const ARMS: [Arms; 5] = [Arms::Up, Arms::Left, Arms::Right, Arms::Both, Arms::Rest];
const TYPING: [Arms; 4] = [Arms::Up, Arms::Left, Arms::Right, Arms::Both];

impl Pic {
    /// How many there are (`index`'s range).
    pub(crate) const COUNT: usize = 5 + 5 + 5 + 4 + 4 + 2 + 5 + 5;
    pub(crate) const WATCH: Pic = Pic::At(Arms::Rest, Face::Raised);

    /// V4's five poses as pictures.
    #[cfg(test)]
    pub(crate) fn of(p: Pose) -> Pic {
        match p {
            Pose::Up => Pic::At(Arms::Up, Face::Open),
            Pose::Left => Pic::At(Arms::Left, Face::Open),
            Pose::Right => Pic::At(Arms::Right, Face::Open),
            Pose::Both => Pic::At(Arms::Both, Face::Open),
            Pose::Watch => Pic::WATCH,
        }
    }

    /// Its place in `Art`'s list: the five first (in `Pose`'s order), then the closed, half, raised and wide faces, the
    /// surprise, the stretch, the swing.
    pub(crate) fn index(self) -> usize {
        let arm = |a: Arms| ARMS.iter().position(|&x| x == a).unwrap_or(0);
        match self {
            Pic::At(Arms::Rest, Face::Raised) => 4,
            Pic::At(a, Face::Open) => arm(a),
            Pic::At(a, Face::Closed) => 5 + arm(a),
            Pic::At(a, Face::Half) => 10 + arm(a),
            Pic::At(a, Face::Raised) => 15 + arm(a),
            Pic::At(a, Face::Wide) => 19 + arm(a).min(3),
            Pic::Surprise(n) => 23 + usize::from(n.min(1)),
            Pic::Stretch(n) => 25 + usize::from(n.min(4)),
            Pic::Swing(n) => 30 + usize::from(n.min(4)),
        }
    }

    /// Its file's name (`frame-NAME.png`): round 1's, but the revised mouths (main's `decisions/showcase.md`, "The
    /// mascot's mouths, revised").
    pub(crate) fn name(self) -> String {
        let arm =
            |a: Arms| ["up", "left", "right", "both", "rest"][ARMS.iter().position(|&x| x == a).unwrap_or(0)];
        match self {
            Pic::At(Arms::Rest, Face::Raised) => "watch".into(),
            Pic::At(a, Face::Open) => arm(a).into(),
            Pic::At(a, Face::Closed) => format!("{}-closed", arm(a)),
            Pic::At(a, Face::Half) => format!("{}-half", arm(a)),
            Pic::At(a, Face::Raised) => format!("{}-raised", arm(a)),
            Pic::At(a, Face::Wide) => format!("{}-wide-r3", arm(a)),
            Pic::Surprise(0) => "surprise-1-r3".into(),
            Pic::Surprise(_) => "surprise-2-r2".into(),
            Pic::Stretch(n) => {
                ["stretch-1", "stretch-2-r3", "stretch-3-r3-square", "stretch-4-r3", "stretch-5"]
                    [usize::from(n.min(4))]
                .into()
            }
            Pic::Swing(n) => {
                ["swing-1", "swing-2", "swing-3-r3", "swing-4", "swing-5"][usize::from(n.min(4))].into()
            }
        }
    }

    /// Every picture, in `index`'s order.
    pub(crate) fn all() -> Vec<Pic> {
        let mut v: Vec<Pic> = TYPING.iter().map(|&a| Pic::At(a, Face::Open)).collect();
        v.push(Pic::WATCH);
        v.extend(ARMS.iter().map(|&a| Pic::At(a, Face::Closed)));
        v.extend(ARMS.iter().map(|&a| Pic::At(a, Face::Half)));
        v.extend(TYPING.iter().map(|&a| Pic::At(a, Face::Raised)));
        v.extend(TYPING.iter().map(|&a| Pic::At(a, Face::Wide)));
        v.extend((0..2).map(Pic::Surprise));
        v.extend((0..5).map(Pic::Stretch));
        v.extend((0..5).map(Pic::Swing));
        v
    }

    /// The pictures a cut with `sw` can show: the five always; the half faces where anything moves (the turn); each
    /// motion's own.
    pub(crate) fn needed(sw: &Switches) -> Vec<Pic> {
        (Pic::all().into_iter())
            .filter(|p| match p {
                Pic::At(_, Face::Open) | Pic::At(Arms::Rest, Face::Raised) => true,
                Pic::At(_, Face::Half) => sw.moves(),
                Pic::At(_, Face::Closed) => sw.blink,
                Pic::At(Arms::Right | Arms::Both, Face::Raised) => sw.glance || sw.fast,
                Pic::At(_, Face::Raised) => sw.glance,
                Pic::At(_, Face::Wide) | Pic::Surprise(_) => sw.surprise,
                Pic::Stretch(_) => sw.stretch,
                Pic::Swing(_) => sw.swing,
            })
            .collect()
    }
}

/// The key pops (`[mascot] keys`): every key heard, the commands only, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Keys {
    All,
    Commands,
    Off,
}

/// The edit list's mascot: there or not (`mascot = false`), and its `[mascot]` switches — each motion and the key pops,
/// all on by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Switches {
    pub(crate) on: bool,
    pub(crate) blink: bool,
    pub(crate) glance: bool,
    pub(crate) surprise: bool,
    pub(crate) stretch: bool,
    pub(crate) swing: bool,
    /// Pressing fast-forward: the right paw held down through a stretch too fast for its keys (`motions::fast_holds`).
    pub(crate) fast: bool,
    pub(crate) keys: Keys,
}

impl Default for Switches {
    fn default() -> Switches {
        Switches {
            on: true,
            blink: true,
            glance: true,
            surprise: true,
            stretch: true,
            swing: true,
            fast: true,
            keys: Keys::All,
        }
    }
}

impl Switches {
    /// These switches changed by `f`.
    #[cfg(test)]
    pub(crate) fn with(mut self, f: fn(&mut Switches)) -> Switches {
        f(&mut self);
        self
    }

    /// Whether any motion is on — the face's turn through its half-raised picture with them: every one off is V4's
    /// mascot, picture for picture.
    pub(crate) fn moves(&self) -> bool {
        self.blink || self.glance || self.surprise || self.stretch || self.swing || self.fast
    }

    /// The switches in words, for `cut.txt`.
    pub(crate) fn said(&self) -> String {
        let on = |b: bool| if b { "on" } else { "off" };
        format!(
            "blink {}, glance {}, surprise {}, stretch {}, swing {}, fast-forward {}",
            on(self.blink),
            on(self.glance),
            on(self.surprise),
            on(self.stretch),
            on(self.swing),
            on(self.fast)
        )
    }
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

/// What a tap is: a key typed (down `HOLD_MS`) or a card's stamp (down `STAMP_HOLD_MS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Press {
    Key,
    Stamp,
}

impl Press {
    /// How long its hand stays down, in whole frames.
    pub(crate) fn hold(self) -> u64 {
        let ms = match self {
            Press::Key => HOLD_MS,
            Press::Stamp => STAMP_HOLD_MS,
        };
        ((ms * FPS) as f64 / 1000.0).round() as u64
    }
}

/// A character's side of a QWERTY.
fn of_char(c: char) -> Side {
    match "`12345qwertasdfgzxcvb~!@#$%".contains(c.to_ascii_lowercase()) {
        true => Side::Left,
        false => Side::Right,
    }
}

/// The hands through the video: each hand's down stretches (frames, from–to) and every tap's frame, in order, with how
/// long its press holds (frames).
#[derive(Debug)]
pub(crate) struct Hands {
    down: [Vec<(u64, u64)>; 2],
    taps: Vec<u64>,
    held: Vec<u64>,
    /// The keys on a stretch too fast for them to be heard: their frames, in order (`sped`) — the left paw's taps
    /// inside a fast-forward hold, nothing else.
    sped: Vec<u64>,
}

impl Hands {
    /// From the taps (video µs, side, press): each at the first frame at or after it, down its press's hold or until one
    /// frame before its hand's next — a tap with no room for that one frame up is moved to the frame after it.
    pub(crate) fn new(taps: &[(u64, Side, Press)]) -> Hands {
        let mut sorted = taps.to_vec();
        sorted.sort_by_key(|t| t.0);
        // (each hand's taps: frame and hold; every tap's)
        let mut at: [Vec<(u64, u64)>; 2] = [Vec::new(), Vec::new()];
        let mut all: Vec<(u64, u64)> = Vec::new();
        let mut last = None;
        for (v, s, press) in sorted {
            let hold = press.hold();
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
                .filter_map(|&h| at[h].last().map(|p| p.0 + 2))
                .fold(Program::frame_at(v), u64::max);
            for &h in hands {
                at[h].push((f, hold));
            }
            all.push((f, hold));
        }
        all.sort_unstable();
        let down = at.map(|fs| {
            (fs.iter().enumerate())
                .map(|(i, &(f, hold))| (f, fs.get(i + 1).map_or(f + hold, |n| (f + hold).min(n.0 - 1))))
                .collect()
        });
        let (taps, held) = all.into_iter().unzip();
        Hands { down, taps, held, sped: Vec::new() }
    }

    /// These hands with the keys on the too-fast stretches (video µs), each at the first frame at or after it.
    pub(crate) fn sped(mut self, keys: &[u64]) -> Hands {
        self.sped = keys.iter().map(|&v| Program::frame_at(v)).collect();
        self.sped.sort_unstable();
        self
    }

    /// The sped keys' frames, in order.
    pub(crate) fn sped_frames(&self) -> &[u64] {
        &self.sped
    }

    /// How many taps.
    pub(crate) fn taps(&self) -> usize {
        self.taps.len()
    }

    /// Every tap's frame, in order.
    pub(crate) fn tap_frames(&self) -> &[u64] {
        &self.taps
    }

    /// Each tap's hold (frames), as `tap_frames`.
    pub(crate) fn holds(&self) -> &[u64] {
        &self.held
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

/// The box at `s` (px).
pub(crate) fn box_at(s: Scale) -> (usize, usize) {
    (s.px(BOX.0), s.px(BOX.1))
}

/// The pictures at the box's size at `scale`, by `Pic::index` (those the cut's switches use; None the rest), and where
/// they were found.
pub(crate) struct Art {
    pub(crate) pics: Vec<Option<Rgba>>,
    pub(crate) dir: PathBuf,
    pub(crate) scale: Scale,
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
    /// The frames `want` for `bg` in `dir`, at `scale`'s box: meok's only.
    pub(crate) fn load(bg: Bg, dir: Option<PathBuf>, scale: Scale, want: &[Pic]) -> Result<Art> {
        if bg == Bg::Light {
            return Err(
                "its frames are meok's only (pale ink on the dark ground), and this cut is hanji's".into()
            );
        }
        let dir = dir.ok_or("no main checkout found")?;
        let mut pics = vec![None; Pic::COUNT];
        for &pic in want {
            pics[pic.index()] = Some({
                let n = pic.name();
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
                video::shrunk(&f, scale.k() as f32 / MADE as f32)
            });
        }
        Ok(Art { pics, dir, scale })
    }

    /// A loaded picture (the plan shows only those its switches loaded).
    pub(crate) fn picture(&self, p: Pic) -> &Rgba {
        self.pics[p.index()].as_ref().unwrap_or_else(|| panic!("frame-{}.png not loaded", p.name()))
    }

    /// The loaded pictures whose ink reaches their box's edge (alpha over a tenth on its outer row or column) — the box
    /// would cut it: each with where, in words (each edge's pixels and its strongest alpha, of 255).
    pub(crate) fn cut_ink(&self) -> Vec<(String, String)> {
        (Pic::all().into_iter())
            .filter_map(|p| {
                let (w, h, px) = self.pics[p.index()].as_ref()?;
                let a = |x: usize, y: usize| px[(y * w + x) * 4 + 3];
                let edges: [(&str, Vec<u8>); 4] = [
                    ("top", (0..*w).map(|x| a(x, 0)).collect()),
                    ("bottom", (0..*w).map(|x| a(x, h - 1)).collect()),
                    ("left", (0..*h).map(|y| a(0, y)).collect()),
                    ("right", (0..*h).map(|y| a(w - 1, y)).collect()),
                ];
                let said: Vec<String> = (edges.iter())
                    .filter_map(|(name, line)| {
                        let inked: Vec<u8> = line.iter().copied().filter(|&v| v > 25).collect();
                        let most = inked.iter().max()?;
                        Some(format!("{name} {} px, alpha at most {most}", inked.len()))
                    })
                    .collect();
                (!said.is_empty()).then(|| (p.name(), said.join(", ")))
            })
            .collect()
    }

    /// The leftmost ink of any loaded picture within `rows` of the box's row `row` (alpha over a tenth), from its left.
    fn ink_from(&self, row: usize, rows: usize) -> Option<usize> {
        (self.pics.iter().flatten())
            .filter_map(|(w, h, px)| {
                (row.saturating_sub(rows)..(row + rows + 1).min(*h))
                    .filter_map(|y| (0..*w).find(|x| px[(y * w + x) * 4 + 3] > 25))
                    .min()
            })
            .min()
    }
}

/// The mascot for `cut` (its frames in `dir`) at `s`, placed under a screen drawn at `x0`, `pw` wide, its cells ending
/// at `cells_bottom` — or why there's none, as `cut.txt` says it.
pub(crate) fn found(
    cut: &Cut,
    dir: Option<PathBuf>,
    screen: (usize, usize),
    cells_bottom: usize,
    s: Scale,
) -> std::result::Result<(Art, Place), String> {
    if !cut.mascot.on {
        return Err("the mascot: none (mascot = false)".into());
    }
    let lane = cut.mascot.keys != Keys::Off;
    let got = (Art::load(cut.bg, dir, s, &Pic::needed(&cut.mascot)))
        .and_then(|a| Ok((place(&a, screen, cells_bottom, lane)?, a)));
    got.map(|(p, a)| (a, p)).map_err(|e| format!("the mascot: none — {e}; the cut is made without it"))
}

/// The pose on frame `k` showing `f` (V4's, before the motions): none on a card; watching where its span is too fast
/// for its keys to be heard.
#[cfg(test)]
pub(crate) fn pose_at(hands: &Hands, k: u64, f: &super::Frame) -> Option<Pose> {
    let fast = f.speed.is_some_and(|s| s > super::sound::KEYS_MAX_SPEED + 1e-6);
    f.card.is_none().then(|| hands.pose(k, fast))
}

/// Where the mascot sits on a frame: its box's top left, where the rule stops, where the band's right marks end (their
/// pictures', margin and all), and the key pops' lane (left and right edges, px) where they're on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Place {
    pub(crate) x: usize,
    pub(crate) y: usize,
    pub(crate) rule_end: usize,
    pub(crate) marks_right: usize,
    pub(crate) lane: Option<(usize, usize)>,
}

/// The box placed under a screen drawn at `x0`, `pw` wide, its cells ending at `cells_bottom` (px, at the art's scale):
/// right on the rule's end, its foot `FOOT` over the frame's — refused (said) where it would rise over a cell; with
/// `lane`, the pops' lane `LANE` wide just left of it, the marks `GAP` from the lane.
pub(crate) fn place(art: &Art, (x0, pw): (usize, usize), cells_bottom: usize, lane: bool) -> Result<Place> {
    let s = art.scale;
    let (bw, bh) = box_at(s);
    let (pad, h) = (s.px(PAD), s.px(H));
    let (x, y) = (x0 + pw - pad - bw, h - s.px(FOOT) - bh);
    if y < cells_bottom {
        return Err(format!(
            "its box would rise {} px over the screen's last row of cells",
            cells_bottom - y
        ));
    }
    let rule = h - s.px(BAND);
    let rule_end = match (rule >= y).then(|| art.ink_from(rule - y, s.px(3))).flatten() {
        Some(ink) => (x + ink).saturating_sub(s.px(RULE_GAP)),
        None => x0 + pw - pad,
    };
    let lane = lane.then(|| (x - s.px(LANE), x));
    let left = lane.map_or(x, |l| l.0);
    Ok(Place { x, y, rule_end, marks_right: left - s.px(GAP) + pad, lane })
}
