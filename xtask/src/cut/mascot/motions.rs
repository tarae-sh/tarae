//! The mascot's motions beyond typing (V5; the design session's brief, `full-video-mascot-motions-brief.md` §2, §2b, §4;
//! main's `decisions/showcase.md`, "The mascot's motions beyond typing"): each what the person at the keys would do then,
//! laid on after the take, so the cut knows every key's time and uses it — nothing is ever broken off.
//!
//! - **Blink** (`BLINK_*`): on its own, every 4–9 s of video, the eyes closed two frames over whatever the arms do.
//! - **Glance** (`GLANCE_*`): inside a typing streak of 4 s or more, the face raised `GLANCE_MS` (0.7 s, the turns
//!   in it) while the hands keep tapping; at most once every 6 s of a streak.
//! - **Surprise** (`fails`): a run that failed comes on screen — `exit N` (N not 0) or `failed…`, Claude's runs and
//!   tests, the preflight, the user's test runs and tasks; never tarae's own git, a restart's build or an install. 0.5 s, at
//!   most once every 20 s, on any span (a failure flying by at ×8 still gets its half second). The one abrupt start, on
//!   purpose; with a key inside it, the wide face over the typing hands, no hop.
//! - **Stretch**: 6 s of video with no key heard (`STRETCH_QUIET_MS`) — after a chapter's card, 6 s of the new chapter:
//!   the quiet starts again at the card's end, never through the hold and the card; 1.2 s, at most once a minute.
//! - **Brush swing**: a chapter's last second before its card (the first's: where it ends), and a bring that landed;
//!   0.9 s. **Every chapter's end gets its swing** (the design session's, 2026-11-12): placed first, and one with no room
//!   is a `Hold` — the cut holds that chapter's last frame still before its card (`What::Hold`, the swing and its margin
//!   past whatever was still going on) and the swing plays there. A bring's stays as it was: placed, or left out.
//! - **Whole-body motions (stretch, swing) only where they fit whole** (`Room`): from the half-turn before them to
//!   `MARGIN_MS` of hands back over the keys after, no key heard, no card, no surprise; one every `BODY_EVERY_MS` within
//!   a chapter (a card is a full break; a chapter's end's swing only clear of the others). A swing
//!   with no room at its moment moves to the nearest within `SWING_MOVE_MS` either side, else it's left out (said); it
//!   also waits for the key pops to go, its trail beside their lane. After one the hands stay over the keys `QUIET_MS`,
//!   as after a key.
//! - **One at a time**, placed in this order: surprise, swing, stretch, glance, blink — each only where the ones before
//!   left room (a blink or a glance with none is skipped).
//! - **The face turns, it doesn't jump** (`turned`): between the face on the keys and the raised one, `TURN` frames of
//!   the half-raised; after a watch it starts down `TURN` frames before the key (the arms over the keys), so the eyes are
//!   on the keyboard when the hand taps. The surprise's start is the one change with no turn. A tap stays a snap.
//! - **The same cut is the same video**: the blinks and glances come from a sequence seeded by the edit list (`Cut::seed`).
//! - **Pressing fast-forward** (`fast_holds`, the user's, 2026-11-12: "빨리 감기에서도 누르긴 해야지"): through a stretch too fast
//!   for its keys (past `KEYS_MAX_SPEED`) the right paw is held down, the face raised to the screen, lifted where it
//!   ends (the hands over the keys `QUIET_MS` after, as after a key); a `▸▸` pop where it goes down (`pops::presses`).
//!   Each key shown in it taps the left paw a frame, a frame up after — keys closer share one. Fast stretches with only
//!   a cut between are one hold; one under `FAST_MIN_MS` of video none. A surprise or swing shows over it; no stretch
//!   begins in one, its quiet counted from the lift (the user's pick, 2026-11-12).
//! - **Winding the skein** (`Plan::wind`, the user's pick, 2026-11-12: "실타래 감기로 가보자"), where its pictures are found:
//!   each hold is the left paw winding the stamp's coil instead — `WIND_IN_FRAMES` of the in-between (the coil at half
//!   strength, the paw reaching), then the loop, a step every `WIND_EVERY` frames (`WIND_EVERY_FAST` past
//!   ×`WIND_FAST_PAST`), a key shown in it stepping it at once (keys closer than `WIND_KEY_GAP` share a step); back to
//!   the keys where the hold ends. Not found: held as above.
//! - **The character's lines** (`Lines`, `[mascot] says`, Z3's E1): every caption is the character's. As a line comes
//!   in, the face on the keys glances up to it — `GLANCE_MS` while the hands keep tapping, the turns as any glance's —
//!   once, and no seeded glance while that line stands. A face already up (watching, fast-forward) meets it as it is
//!   (the nod is the motions' to come); a whole-body motion goes on.
//! - **Painting on the tablet** (`Plan::paint`, Z3: the mascot brief's §6.2, the user's "붓으로 슥슥 … 너무 빠르게
//!   휙휙 할 필요는 없을 듯"), where its pictures are found: one calm rhythm of its own, never the board's strokes — while
//!   a step is drawn, from the frame its first stroke starts to the frame its last ends, the brush across the tablet
//!   (`sweep-1` … `-7`, `SWEEP`'s holds: a second), back over it in the air (`return-1` … `-3`, `RETURN`'s) and lifted
//!   (`draw-lift`, `PAINT_LIFT`), again; no picture of it held fewer frames than its own hold (`rhythm`). Then at rest (`draw-in`: the brush up, the eyes off the tablet) on the frame the
//!   last stroke ends, the one its station is inked on, through the stand. The keyboard becomes the tablet inside the
//!   board's crossing into the picture (`swing-1`, then `draw-in`), and back inside the crossing out. A surprise,
//!   stretch or swing over it stays. Not found: it watches, said.
//! - Every switch off: V4's mascot, picture for picture (no turn either).

use super::{Arms, Face, Hands, Pic, Pose, QUIET_MS, Switches};
use crate::camera::CROSS_MS;
use crate::cut::sound::KEYS_MAX_SPEED;
use crate::cut::{HOLD_STEP, Hold, Marks, Pace, Program, What, clock, ms_of};
use crate::rain::Rng;
use crate::video::FPS;

/// The half-raised face between the keys and the screen, each way (frames).
pub(crate) const TURN: u64 = 2;
/// A whole-body motion ends this long (hands back over the keys) before the next heard key, a card or a surprise.
pub(crate) const MARGIN_MS: u64 = 300;
pub(crate) const SURPRISE_EVERY_MS: u64 = 20_000;
/// 6 s, not the brief's 20 s (the design session, 2026-11-12: at 20 s and at 10 s take 1's cut had one stretch in twelve
/// minutes — the cut leaves stills out and shows a turn in at most 20 s; at 6 s, five, with the caps over them).
pub(crate) const STRETCH_QUIET_MS: u64 = 6_000;
pub(crate) const STRETCH_EVERY_MS: u64 = 60_000;
/// At most one whole-body motion (a stretch or a swing) this often.
pub(crate) const BODY_EVERY_MS: u64 = 30_000;
pub(crate) const SWING_MOVE_MS: u64 = 2_000;
/// The brief's 0.5 s didn't read at the band's size on take 1's clip (level and raised a few px apart): 0.7 s, as the
/// design session said to if so.
pub(crate) const GLANCE_MS: u64 = 700;
pub(crate) const GLANCE_STREAK_MS: u64 = 4_000;
pub(crate) const GLANCE_EVERY_MS: u64 = 6_000;
/// A glance comes this far into its streak at the soonest: seeded between the two.
const GLANCE_FIRST_MS: (u64, u64) = (1_500, 3_000);
/// Past `GLANCE_EVERY_MS`, a seeded wait up to this.
const GLANCE_MORE_MS: u64 = 2_000;
pub(crate) const BLINK_MS: (u64, u64) = (4_000, 9_000);
pub(crate) const BLINK_FRAMES: u64 = 2;
/// Each motion's frames and how long each is held (frames at `FPS`): the whole-body ones on twos, near enough — the
/// arms rising, the hold, coming down; the brush taken, raised, the stroke, its end held, put back.
pub(crate) const SURPRISE: [(u8, u64); 2] = [(0, 8), (1, 7)];
pub(crate) const STRETCH: [(u8, u64); 5] = [(0, 4), (1, 4), (2, 20), (3, 4), (4, 4)];
pub(crate) const SWING: [(u8, u64); 5] = [(0, 4), (1, 6), (2, 2), (3, 10), (4, 5)];
/// The glances `cut.txt` names by their time, at most.
const GLANCES_SAID: usize = 12;
/// A run's name in `cut.txt`, at most this many characters.
const RUN_SAID: usize = 72;
/// The sequences' streams of the edit list's seed.
const BLINKS: u64 = 1;
const GLANCES: u64 = 2;

/// A fast stretch shorter than this (video) gets no fast-forward hold: a hold outlasts the stamp's press
/// (`STAMP_HOLD_MS`, 300 ms) to read as held, not pressed and lifted; and take 3's rough cut has 36 sped bits at about
/// 0.3 s (a turn's ×4 between half-seconds of a still screen) that a 300 ms line would split a frame either way.
pub(crate) const FAST_MIN_MS: u64 = 400;
/// Winding the skein (`Plan::wind`): the in-between it cuts in with (frames) — straight from the keys to the coil read
/// as a pop of ink on the strip's flip row; a step of the loop every this many frames, every `WIND_EVERY_FAST` past
/// ×`WIND_FAST_PAST` (the design session's spec); keys closer than `WIND_KEY_GAP` frames share a step.
pub(crate) const WIND_IN_FRAMES: u64 = 2;
pub(crate) const WIND_EVERY: u64 = 4;
pub(crate) const WIND_EVERY_FAST: u64 = 2;
pub(crate) const WIND_FAST_PAST: f64 = 8.0;
pub(crate) const WIND_KEY_GAP: u64 = 2;
/// Painting's sweep (the in-between pictures, the user's 2026-11-16): the brush across the tablet, each picture
/// (`Pic::Sweep`) and the frames it's held — easing in and out, 30 frames, a second —…
pub(crate) const SWEEP: [(u8, u64); Pic::SWEEPS as usize] =
    [(0, 5), (1, 4), (2, 4), (3, 4), (4, 4), (5, 4), (6, 5)];
/// …back over it in the air (`Pic::Return`)…
pub(crate) const RETURN: [(u8, u64); Pic::RETURNS as usize] = [(0, 2), (1, 2), (2, 2)];
/// …lifted (`draw-lift`), and again: a sweep 38 frames (1.27 s).
pub(crate) const PAINT_LIFT: u64 = 2;

/// A sweep's pictures in order, each with its hold (frames): `SWEEP`, `RETURN`, `draw-lift`.
pub(crate) fn sweep() -> Vec<(Pic, u64)> {
    let mut v: Vec<(Pic, u64)> = SWEEP.iter().map(|&(n, f)| (Pic::Sweep(n), f)).collect();
    v.extend(RETURN.iter().map(|&(n, f)| (Pic::Return(n), f)));
    v.push((Pic::DrawLift, PAINT_LIFT));
    v
}

/// Painting's rhythm over a run of `n` frames from a step's first stroke: `sweep`, again. A last piece cut shorter than
/// its own picture's hold is taken by the one before it, which is whole; a run shorter than `sweep-1`'s hold is none
/// (the rest stays) — so no picture is held fewer frames than its own hold.
pub(crate) fn rhythm(n: u64) -> Vec<Pic> {
    let sweep = sweep();
    if n < sweep[0].1 {
        return Vec::new();
    }
    // (each piece: its picture, its frames, its hold)
    let mut pieces: Vec<(Pic, u64, u64)> = Vec::new();
    let mut left = n;
    for &(p, hold) in sweep.iter().cycle() {
        if left == 0 {
            break;
        }
        pieces.push((p, hold.min(left), hold));
        left -= hold.min(left);
    }
    let k = pieces.len();
    if k >= 2 && pieces[k - 1].1 < pieces[k - 1].2 {
        pieces[k - 2].1 += pieces[k - 1].1;
        pieces.pop();
    }
    pieces.iter().flat_map(|&(p, len, _)| std::iter::repeat_n(p, len as usize)).collect()
}

/// A painting picture's hold in a sweep (frames); none for the rest's and the rest.
pub(crate) fn hold(p: Pic) -> Option<u64> {
    sweep().into_iter().find(|s| s.0 == p).map(|s| s.1)
}

/// What painting laid (`Plan::paint`): the steps painted of the pictures' all, and each one's run (frames, from its first
/// stroke to its rest); the frames of each of the sweep's seven, of the three back, of the lift, of the rest (`draw-in`,
/// the turn's too) and of `swing-1` turning; how many times a picture of the rhythm stood fewer frames than its hold (a
/// surprise, stretch or swing over it can cut one); and each sweep across the tablet as shown (`Sweep`): the brush's
/// sound, a stroke a sweep (`cut::brushing`).
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Painted {
    pub(crate) steps: (usize, usize),
    pub(crate) runs: Vec<(u64, u64)>,
    pub(crate) sweeps: Vec<Sweep>,
    pub(crate) sweep: [usize; Pic::SWEEPS as usize],
    pub(crate) back: [usize; Pic::RETURNS as usize],
    pub(crate) lift: usize,
    pub(crate) rest: usize,
    pub(crate) turns: usize,
    pub(crate) under: usize,
}

/// A sweep across the tablet as shown: its first frame (a `sweep-1` the frame before isn't across from), how many frames
/// the brush is on the tablet (`sweep-1` … `-7`) — up to its way back in the air, its rest or whatever stands over
/// it —, and which it is: its picture, its step and its place among that step's sweeps (from 0) — the brush's sound's
/// seed, the same sweep the same sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sweep {
    pub(crate) at: u64,
    pub(crate) frames: u64,
    pub(crate) picture: usize,
    pub(crate) step: usize,
    pub(crate) nth: usize,
}

/// The fast-forward holds (frames, from–to, in order): each run of frames shown past `KEYS_MAX_SPEED` — runs with only
/// a cut between are one, the frames are next to each other — of `FAST_MIN_MS` or more; and how many runs were shorter.
pub(crate) fn fast_holds(prog: &Program) -> (Vec<(u64, u64)>, usize) {
    let n = prog.frames();
    let fast = |k: u64| {
        let f = prog.frame(Program::time_of(k));
        f.card.is_none() && f.speed.is_some_and(|s| s > KEYS_MAX_SPEED + 1e-6)
    };
    let (mut holds, mut short) = (Vec::new(), 0);
    let mut k = 0;
    while k < n {
        if !fast(k) {
            k += 1;
            continue;
        }
        let a = k;
        while k < n && fast(k) {
            k += 1;
        }
        match k - a >= frames(FAST_MIN_MS) {
            true => holds.push((a, k)),
            false => short += 1,
        }
    }
    (holds, short)
}

pub(crate) fn frames(ms: u64) -> u64 {
    ms * FPS / 1000
}

fn len_of(table: &[(u8, u64)]) -> u64 {
    table.iter().map(|t| t.1).sum()
}

/// The frame of `table` shown `at` frames into its motion.
fn nth(table: &[(u8, u64)], at: u64) -> u8 {
    let mut left = at;
    for &(n, len) in table {
        if left < len {
            return n;
        }
        left -= len;
    }
    table.last().map_or(0, |t| t.0)
}

/// A whole-body motion or a surprise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Surprise,
    Stretch,
    Swing,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Surprise => "surprise",
            Kind::Stretch => "stretch",
            Kind::Swing => "swing",
        }
    }
}

/// What makes a surprise or a swing: where in the video (µs — a swing's, where it would start), the take's ms, in words;
/// a chapter's end: whose card follows, and whether it's held still for the swing (`What::Hold`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Cause {
    pub(crate) v: u64,
    pub(crate) t: u64,
    pub(crate) said: String,
    pub(crate) chapter: Option<usize>,
    pub(crate) held: bool,
}

impl Cause {
    /// A cause that isn't a chapter's end.
    pub(crate) fn at(v: u64, t: u64, said: String) -> Cause {
        Cause { v, t, said, chapter: None, held: false }
    }
}

/// A swing and its margin (µs): a chapter's starts this long before its card.
pub(crate) fn swing_us() -> u64 {
    len_of(&SWING) * 1_000_000 / FPS + MARGIN_MS * 1000
}

/// The take's surprises and swings, each where the cut shows it.
#[derive(Debug, Default)]
pub(crate) struct Causes {
    pub(crate) surprise: Vec<Cause>,
    pub(crate) swing: Vec<Cause>,
}

/// Whether a run's end is a surprise: its name Claude's run or tests, the preflight, the user's test run or task (never
/// tarae's own git, a restart's build or an install); its step `exit N`, N not 0, or `failed…`. `passed`, `not run: …`
/// and `dropped before its end` make nothing.
pub(crate) fn fails(run: &str, step: &str) -> bool {
    let ours = ["claude's run: ", "claude's run again: ", "claude's tests: ", "tests: ", "task: "]
        .iter()
        .any(|p| run.starts_with(p))
        || run.starts_with("preflight");
    let exit = step.strip_prefix("exit ").and_then(|n| n.trim().parse::<i64>().ok());
    ours && (step.starts_with("failed") || exit.is_some_and(|n| n != 0))
}

/// Where the take's `t` (ms) is shown in the video (µs): where it ends, and the take part's frame — at any speed; none
/// where it's left out.
pub(crate) fn shown_at(prog: &Program, t: u64) -> Vec<u64> {
    let cut = prog.cut;
    let mut at = Vec::new();
    let o = &cut.opening;
    if o.ends(t)
        && let Some(p) = prog.parts.iter().find(|p| matches!(p.what, What::Ends { .. }))
    {
        at.push(p.at + (t - o.from) * 1000);
    }
    let map = &cut.map;
    if t < cut.end && map.segs()[map.seg_of(t)].pace != Pace::Out {
        at.extend(prog.video_of(map.video(t)));
    }
    at
}

/// The take's causes in the cut: each failed run's end where it's shown (surprise); the second before each chapter's card
/// and each landed bring where it's shown (swing — a chapter's ends `MARGIN_MS` before its card).
pub(crate) fn causes(prog: &Program, marks: &Marks) -> Causes {
    let mut c = Causes::default();
    for v in marks.of("run-ended") {
        let (run, step) = (v["run"].as_str().unwrap_or(""), v["step"].as_str().unwrap_or(""));
        if fails(run, step) {
            let t = ms_of(v);
            // (a long command cut short: the list is read by a person)
            let run = match run.chars().count() > RUN_SAID {
                true => format!("{}…", run.chars().take(RUN_SAID - 1).collect::<String>()),
                false => run.to_string(),
            };
            let said = format!("{run} — {step}");
            c.surprise.extend(shown_at(prog, t).into_iter().map(|v| Cause::at(v, t, said.clone())));
        }
    }
    let swing = swing_us();
    for (i, p) in prog.parts.iter().enumerate() {
        let What::Chapter(n) = p.what else { continue };
        let before = i.checked_sub(1).map(|j| prog.parts[j]);
        let held = match before.map(|b| b.what) {
            Some(What::Ends { .. } | What::Take { .. }) => None,
            Some(What::Hold { .. }) => before.map(|b| b.len),
            _ => continue,
        };
        let v = p.at.saturating_sub(swing);
        let f = prog.frame(v);
        let what = match (f.ends, n) {
            (true, _) => "where it ends".to_string(),
            (false, 0) => "the take before chapter 1".to_string(),
            (false, n) => format!("chapter {n}'s end"),
        };
        let t = f.clock.unwrap_or(0);
        let still = held.map_or(String::new(), |us| format!(", held still {:.1} s", us as f64 / 1e6));
        let said = format!("{what}, before chapter {}'s card{still}", n + 1);
        c.swing.push(Cause { v, t, said, chapter: Some(n), held: held.is_some() });
    }
    for v in marks.of("bring").filter(|v| v["how"] == "landed") {
        let t = ms_of(v);
        let said = format!("a bring landed: {}", v["name"].as_str().unwrap_or("?"));
        c.swing.extend(shown_at(prog, t).into_iter().map(|v| Cause::at(v, t, said.clone())));
    }
    c.surprise.sort_by_key(|c| c.v);
    c.swing.sort_by_key(|c| c.v);
    c
}

/// A motion placed: what, from which frame, how many, its cause, how far it was moved (frames), whether a key is
/// heard inside it (a surprise over typing).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Placed {
    pub(crate) kind: Kind,
    pub(crate) from: u64,
    pub(crate) len: u64,
    pub(crate) cause: String,
    pub(crate) t: u64,
    pub(crate) moved: i64,
    pub(crate) typing: bool,
}

/// Every frame's picture (None on a card), and what was placed, left out, blinked and glanced.
#[derive(Debug)]
pub(crate) struct Plan {
    pics: Vec<Option<Pic>>,
    pub(crate) placed: Vec<Placed>,
    pub(crate) left_out: Vec<String>,
    pub(crate) blinks: usize,
    pub(crate) glances: usize,
    /// Each glance's first frame.
    pub(crate) glanced: Vec<u64>,
    /// Each watch of `STRETCH_QUIET_MS` or more given no stretch: its first frame, its length, why (at its first chance).
    pub(crate) unstretched: Vec<(u64, u64, String)>,
    /// How many stretches each of `STRETCH_TRIED_MS` would give (ms, count).
    pub(crate) would_stretch: Vec<(u64, usize)>,
    /// Each chapter's end whose swing had no room, and how long it must be held still before its card for one
    /// (`cut::Program::held`; the cut lays it all again with them).
    pub(crate) holds: Vec<Hold>,
    /// Pressing fast-forward: each hold (frames, from–to), the left paw's taps in them, the keys shown in them, and the
    /// fast stretches too short for one.
    pub(crate) fast: Vec<(u64, u64)>,
    pub(crate) fast_taps: usize,
    pub(crate) fast_keys: usize,
    pub(crate) fast_short: usize,
    /// Whether the holds wind the skein (`wind`), and how many of its steps a key made.
    pub(crate) wound: bool,
    pub(crate) wind_key_steps: usize,
    /// The character's lines, met as each came in (`says`).
    pub(crate) lines: Lines,
    /// What painting laid, where it did (`paint`).
    pub(crate) painted: Option<Painted>,
}

/// How the character met its lines (`[mascot] says`): how many lines in how many rows; the first frame of each it
/// glanced up to from the keys; how many it met with its face already up (watching, fast-forward: no nod yet), and how
/// many in a whole-body motion or never shown.
#[derive(Debug, Default)]
pub(crate) struct Lines {
    pub(crate) n: usize,
    pub(crate) rows: usize,
    pub(crate) glanced: Vec<u64>,
    pub(crate) watching: usize,
    pub(crate) other: usize,
}

impl Lines {
    fn counted(cut: &crate::cut::Cut) -> Lines {
        Lines {
            n: cut.captions.len(),
            rows: cut.captions.iter().filter(|c| c.row.0 == 0).count(),
            ..Lines::default()
        }
    }

    /// `cut.txt`'s line where the list says but no mascot is drawn.
    pub(crate) fn absent(cut: &crate::cut::Cut) -> String {
        let l = Lines::counted(cut);
        format!("the character's lines (says): {} in {} rows — no mascot to meet them", l.n, l.rows)
    }

    fn said(&self) -> String {
        format!(
            "the character's lines (says): {} in {} rows — {} glanced, {} met while watching (no nod yet), {} in a \
             motion or not shown",
            self.n,
            self.rows,
            self.glanced.len(),
            self.watching,
            self.other
        )
    }
}

/// What a whole-body motion's frames must be clear of.
struct Room<'a> {
    n: u64,
    /// Frames not shown before each frame (a prefix sum: a stretch shown wholly when it adds none).
    unshown: Vec<u32>,
    taps: &'a [u64],
    /// Each tap's hold (frames, its hand up at the tap's frame and this), as `taps`, and the longest.
    held: &'a [u64],
    longest: u64,
    margin: u64,
    /// The key pops' frames alive (from, until), in order of their start.
    pops: &'a [(u64, u64)],
}

impl Room<'_> {
    fn shown(&self, a: u64, b: u64) -> bool {
        b <= self.n && a <= b && self.unshown[b as usize] == self.unshown[a as usize]
    }

    fn tap_in(&self, a: u64, b: u64) -> Option<u64> {
        let i = self.taps.partition_point(|&f| f < a);
        self.taps.get(i).copied().filter(|&f| f < b)
    }

    /// The first tap whose hand is still down at `k` (its hold not past) or that comes before `end`.
    fn pressed(&self, k: u64, end: u64) -> Option<u64> {
        let i = self.taps.partition_point(|&f| f < k.saturating_sub(self.longest));
        (i..self.taps.len())
            .take_while(|&j| self.taps[j] < end)
            .find(|&j| self.taps[j] + self.held[j] > k)
            .map(|j| self.taps[j])
    }

    /// Whether a `len`-frame whole-body motion fits from frame `k` among `placed`; else why not, in words. `spaced`: at
    /// least `BODY_EVERY_MS` from another whole-body motion in its chapter — a card between is a full break (the design
    /// session's, 2026-11-12) —; a chapter's end's swing isn't, only clear of it (each gets its swing).
    fn fits(
        &self,
        k: u64,
        len: u64,
        placed: &[Placed],
        pops: bool,
        spaced: bool,
    ) -> std::result::Result<(), String> {
        let end = k + len + self.margin;
        if k < TURN || !self.shown(k - TURN, end) {
            return Err("a card or the video's edge".into());
        }
        if let Some(f) = self.pressed(k, end) {
            return Err(format!("a key at {}", secs(f)));
        }
        for p in placed {
            let near = match p.kind {
                Kind::Surprise => k - TURN < p.from + p.len + TURN && p.from < end + TURN,
                _ if spaced => {
                    k.abs_diff(p.from) < frames(BODY_EVERY_MS) && self.shown(k.min(p.from), k.max(p.from))
                }
                _ => k - TURN < p.from + p.len + self.margin && p.from < end + TURN,
            };
            if near {
                return Err(format!("the {} at {}", p.kind.word(), secs(p.from)));
            }
        }
        if pops {
            let i = self.pops.partition_point(|p| p.0 <= k);
            if self.pops[..i].iter().rev().take(64).any(|p| p.1 > k) {
                return Err("the key pops still up".into());
            }
        }
        Ok(())
    }

    /// How long a chapter's end must be held still before its card (its first frame `card`) for a `len`-frame swing to
    /// fit there: past the last key's hand coming up, the last key pop gone and the last surprise's turn, then the swing
    /// and the margin — in whole `HOLD_STEP`s (frames). Nothing comes inside a hold: no key maps there, no surprise.
    fn hold_for(&self, card: u64, len: u64, placed: &[Placed]) -> u64 {
        let mut k = card;
        let before = self.taps.partition_point(|&f| f < card);
        let from = self.taps.partition_point(|&f| f < card.saturating_sub(self.longest));
        if let Some(up) = (from..before).map(|j| self.taps[j] + self.held[j]).max() {
            k = k.max(up);
        }
        let i = self.pops.partition_point(|p| p.0 < card);
        if let Some(until) = self.pops[..i].iter().map(|p| p.1).max() {
            k = k.max(until);
        }
        for p in placed.iter().filter(|p| p.kind == Kind::Surprise && p.from < card) {
            k = k.max(p.from + p.len + 2 * TURN);
        }
        (k - card + len + self.margin).div_ceil(HOLD_STEP) * HOLD_STEP
    }
}

/// The other quiets `cut.txt` says the stretches of, to tune `STRETCH_QUIET_MS` by.
const STRETCH_TRIED_MS: [u64; 5] = [5_000, 8_000, 10_000, 15_000, 20_000];

/// The stretches among `placed`: `need` frames into a watch with no key heard (counted from a key's own frame, or from
/// the mascot's coming back after a card), the first frame from there where one fits whole, at most one a minute;
/// and each watch that long given none — its first frame, its length, why at its first chance.
#[allow(clippy::type_complexity)]
fn stretches(
    room: &Room,
    shown: &[bool],
    placed: &[Placed],
    need: u64,
    at_take: &dyn Fn(u64) -> u64,
) -> (Vec<Placed>, Vec<(u64, u64, String)>) {
    let (n, len) = (room.n, len_of(&STRETCH));
    let mut all = placed.to_vec();
    let (mut out, mut unstretched) = (Vec::new(), Vec::new());
    let (mut next_ok, mut quiet_from, mut why, mut k) = (0, 0, None::<String>, 0);
    while k < n {
        let ends = !shown[k as usize] || room.tap_in(k, k + 1).is_some();
        if (ends || k + 1 == n)
            && let Some(w) = why.take()
        {
            unstretched.push((quiet_from, k - quiet_from, w));
        }
        if ends {
            quiet_from = if shown[k as usize] { k } else { k + 1 };
            k += 1;
            continue;
        }
        if k == quiet_from + need {
            why = match (k < next_ok, room.fits(k, len, &all, false, true)) {
                (true, _) => Some("within a minute of the last stretch".into()),
                (false, Err(e)) => Some(e),
                (false, Ok(())) => None,
            };
        }
        if k >= quiet_from + need && k >= next_ok && room.fits(k, len, &all, false, true).is_ok() {
            why = None;
            let p = Placed {
                kind: Kind::Stretch,
                from: k,
                len,
                cause: format!("{:.1} s of video with no key heard", (k - quiet_from) as f64 / FPS as f64),
                t: at_take(k),
                moved: 0,
                typing: false,
            };
            all.push(p.clone());
            out.push(p);
            next_ok = k + frames(STRETCH_EVERY_MS);
            k += len;
            continue;
        }
        k += 1;
    }
    (out, unstretched)
}

/// A frame's video time, for what's said (`m:ss.s`): its start rounded up to the tenth, so a reader who goes to that
/// time lands on the frame or the two after it — inside what's named (a hold, a motion: 0.4 s and more), never on the
/// frame before it (truncated, a hold from frame 4633, 2:34.433, read 2:34.4: frame 4632, still at 1×).
pub(crate) fn secs(k: u64) -> String {
    let ms = Program::time_of(k).div_ceil(100_000) * 100;
    format!("{}.{}", clock(ms), ms % 1000 / 100)
}

/// The face's level, for the turn: on the keys, half-way, on the screen.
#[derive(PartialEq)]
enum Level {
    Down,
    Half,
    Up,
}

fn level(p: Pic) -> Level {
    match p {
        Pic::At(_, Face::Raised) => Level::Up,
        Pic::At(_, Face::Half) => Level::Half,
        _ => Level::Down,
    }
}

/// The arms of V4's pose.
fn arms(p: Pose) -> Arms {
    match p {
        Pose::Up => Arms::Up,
        Pose::Left => Arms::Left,
        Pose::Right => Arms::Right,
        Pose::Both => Arms::Both,
        Pose::Watch => Arms::Rest,
    }
}

impl Plan {
    /// The motions through `prog`'s frames: the taps `hands`, the take's `causes`, the key pops' frames alive `pops` (in
    /// order), as the switches `sw` say, seeded by `seed`.
    pub(crate) fn new(
        prog: &Program,
        hands: &Hands,
        causes: &Causes,
        pops: &[(u64, u64)],
        sw: &Switches,
        seed: u64,
    ) -> Plan {
        let n = prog.frames();
        let (mut shown, mut fast) = (vec![false; n as usize], vec![false; n as usize]);
        let mut unshown = vec![0u32; n as usize + 1];
        // (a picture's frames: shown, but no stretch there — the character watches it drawn)
        let mut pictured = vec![false; n as usize];
        for k in 0..n {
            let f = prog.frame(Program::time_of(k));
            pictured[k as usize] = f.picture.is_some();
            shown[k as usize] = f.card.is_none();
            fast[k as usize] = f.speed.is_some_and(|s| s > KEYS_MAX_SPEED + 1e-6);
            unshown[k as usize + 1] = unshown[k as usize] + u32::from(f.card.is_some());
        }
        let room = Room {
            n,
            unshown,
            taps: hands.tap_frames(),
            held: hands.holds(),
            longest: hands.holds().iter().copied().max().unwrap_or(0),
            margin: frames(MARGIN_MS),
            pops,
        };
        let mut placed: Vec<Placed> = Vec::new();
        let mut left_out = Vec::new();
        let at_take = |k: u64| prog.frame(Program::time_of(k)).clock.unwrap_or(0);
        // (pressing fast-forward's holds: a surprise or swing placed by its own rules shows over one; a stretch waits)
        let (fast_held, fast_short) = if sw.fast { fast_holds(prog) } else { (Vec::new(), 0) };

        // (the surprises: where each failure is shown, the caps between)
        if sw.surprise {
            let len = len_of(&SURPRISE);
            let mut last: Option<u64> = None;
            for c in &causes.surprise {
                let k = Program::frame_at(c.v);
                if let Some(l) = last.filter(|&l| k < l + frames(SURPRISE_EVERY_MS)) {
                    left_out.push(format!(
                        "surprise left out at {}: {} — within {} s of the one at {}",
                        secs(k),
                        c.said,
                        SURPRISE_EVERY_MS / 1000,
                        secs(l)
                    ));
                    continue;
                }
                if !room.shown(k, k + len) {
                    left_out.push(format!(
                        "surprise left out at {}: {} — a card within its half second",
                        secs(k),
                        c.said
                    ));
                    continue;
                }
                let typing = room.tap_in(k, k + len).is_some();
                placed.push(Placed {
                    kind: Kind::Surprise,
                    from: k,
                    len,
                    cause: c.said.clone(),
                    t: c.t,
                    moved: 0,
                    typing,
                });
                last = Some(k);
            }
        }
        // (the swings: the chapters' ends first — each gets its swing —, then the brings'; at their moment, else the
        // nearest room within `SWING_MOVE_MS`, else left out — a chapter's end then held still for it, `holds`; one in
        // its hold plays there)
        let mut holds = Vec::new();
        if sw.swing {
            let len = len_of(&SWING);
            let most = frames(SWING_MOVE_MS) as i64;
            let ends = causes.swing.iter().filter(|c| c.chapter.is_some());
            for c in ends.chain(causes.swing.iter().filter(|c| c.chapter.is_none())) {
                let ideal = Program::frame_at(c.v) as i64;
                let spaced = c.chapter.is_none();
                if c.held {
                    placed.push(Placed {
                        kind: Kind::Swing,
                        from: ideal as u64,
                        len,
                        cause: c.said.clone(),
                        t: c.t,
                        moved: 0,
                        typing: false,
                    });
                    continue;
                }
                let tries = (0..=most).flat_map(|d| if d == 0 { vec![0] } else { vec![-d, d] });
                let found = tries
                    .filter(|d| ideal + d >= 0)
                    .find(|d| room.fits((ideal + d) as u64, len, &placed, true, spaced).is_ok());
                match found {
                    Some(d) => placed.push(Placed {
                        kind: Kind::Swing,
                        from: (ideal + d) as u64,
                        len,
                        cause: c.said.clone(),
                        t: c.t,
                        moved: d,
                        typing: false,
                    }),
                    None => {
                        let why = room
                            .fits(ideal.max(0) as u64, len, &placed, true, spaced)
                            .err()
                            .unwrap_or_default();
                        left_out.push(format!(
                            "swing left out at {}: {} — no room within {} s either side ({why} at its moment)",
                            secs(ideal.max(0) as u64),
                            c.said,
                            SWING_MOVE_MS / 1000
                        ));
                        if let Some(chapter) = c.chapter {
                            let card = Program::frame_at(c.v + swing_us());
                            holds.push(Hold { chapter, frames: room.hold_for(card, len, &placed) });
                        }
                    }
                }
            }
        }
        // (the stretches; and, for the list's tuning, how many other quiets would give)
        let (mut unstretched, mut would_stretch) = (Vec::new(), Vec::new());
        if sw.stretch {
            // (holding fast-forward isn't idle: no stretch begins in a hold, its quiet counts from the lift — the
            // user's pick, 2026-11-12)
            let shown: Vec<bool> = (0..n)
                .map(|k| {
                    shown[k as usize]
                        && !pictured[k as usize]
                        && !fast_held.iter().any(|h| h.0 <= k && k < h.1)
                })
                .collect();
            let (mine, not) = stretches(&room, &shown, &placed, frames(STRETCH_QUIET_MS), &at_take);
            would_stretch = (STRETCH_TRIED_MS.iter())
                .map(|&ms| (ms, stretches(&room, &shown, &placed, frames(ms), &at_take).0.len()))
                .collect();
            placed.extend(mine);
            unstretched = not;
        }
        placed.sort_by_key(|p| p.from);

        // (the arms and face before the face's own motions: V4's pose — the hands kept over the keys `QUIET_MS` after a
        // whole-body motion, as after a key —, a placed motion's frames over it)
        let quiet = frames(QUIET_MS);
        // (pressing fast-forward: the left paw's taps in its holds — a frame down, a frame up)
        let hold_of = |k: u64| {
            let i = fast_held.partition_point(|h: &(u64, u64)| h.0 <= k);
            (i > 0 && k < fast_held[i - 1].1).then(|| i - 1)
        };
        let (mut left, mut fast_keys) = (Vec::<u64>::new(), 0);
        for &f in hands.sped_frames() {
            if hold_of(f).is_some() {
                fast_keys += 1;
                if left.last().is_none_or(|&l| f >= l + 2) {
                    left.push(f);
                }
            }
        }
        let mut body_ends: Vec<u64> = (placed.iter())
            .filter(|p| p.kind != Kind::Surprise)
            .map(|p| p.from + p.len)
            .chain(fast_held.iter().map(|h| h.1))
            .collect();
        body_ends.sort_unstable();
        let base = |k: u64| -> Pose {
            let p = hands.pose(k, fast[k as usize]);
            let i = body_ends.partition_point(|&e| e <= k);
            match p == Pose::Watch && !fast[k as usize] && i > 0 && k - body_ends[i - 1] < quiet {
                true => Pose::Up,
                false => p,
            }
        };
        let mut pics: Vec<Option<Pic>> = vec![None; n as usize];
        let mut at = 0;
        for k in 0..n {
            if !shown[k as usize] {
                continue;
            }
            while at < placed.len() && placed[at].from + placed[at].len <= k {
                at += 1;
            }
            let held = hold_of(k).is_some();
            let a = match held {
                true if left.binary_search(&k).is_ok() => Arms::Both,
                true => Arms::Right,
                false => arms(base(k)),
            };
            let pic = match placed.get(at).filter(|p| p.from <= k) {
                Some(p) => match p.kind {
                    Kind::Surprise if p.typing => {
                        Pic::At(if a == Arms::Rest { Arms::Up } else { a }, Face::Wide)
                    }
                    Kind::Surprise => Pic::Surprise(nth(&SURPRISE, k - p.from)),
                    Kind::Stretch => Pic::Stretch(nth(&STRETCH, k - p.from)),
                    Kind::Swing => Pic::Swing(nth(&SWING, k - p.from)),
                },
                None if held => Pic::At(a, Face::Raised),
                None if a == Arms::Rest => Pic::WATCH,
                None => Pic::At(a, Face::Open),
            };
            pics[k as usize] = Some(pic);
        }

        // (the character's lines: as each comes in, the face on the keys raised `GLANCE_MS` — once; the seeded glances
        // kept off while it stands. A face up already meets it as it is; a motion goes on)
        let mut lines = Lines::default();
        let mut met: Vec<(u64, u64)> = Vec::new();
        let mut standing: Vec<(u64, u64)> = Vec::new();
        if sw.says {
            lines = Lines::counted(prog.cut);
            let typing = |k: u64| matches!(pics[k as usize], Some(Pic::At(a, Face::Open)) if a != Arms::Rest);
            for c in &prog.cut.captions {
                let Some(k) = prog.video_of(c.m).map(Program::frame_at).filter(|&k| k < n) else {
                    lines.other += 1;
                    continue;
                };
                let end = prog.video_of(c.end()).map_or(n, Program::frame_at).min(n);
                standing.push((k, end));
                match pics[k as usize] {
                    _ if typing(k) => {
                        let to = (k + frames(GLANCE_MS)).min(end);
                        let j = (k..to).find(|&j| !typing(j)).unwrap_or(to);
                        met.push((k, j));
                        lines.glanced.push(k);
                    }
                    Some(Pic::At(_, Face::Raised | Face::Half)) => lines.watching += 1,
                    _ => lines.other += 1,
                }
            }
        }

        // (the glances: seeded in each typing streak of `GLANCE_STREAK_MS` or more, inside it, clear of a surprise —
        // and of a line standing)
        let mut glanced: Vec<(u64, u64)> = Vec::new();
        if sw.glance {
            let mut rng = Rng::new(seed, GLANCES);
            let len = frames(GLANCE_MS);
            let typing = |k: u64| matches!(pics[k as usize], Some(Pic::At(a, Face::Open)) if a != Arms::Rest);
            let mut k = 0;
            while k < n {
                if !typing(k) {
                    k += 1;
                    continue;
                }
                let a = k;
                while k < n && typing(k) {
                    k += 1;
                }
                // (the streak's typing: its first tap to its last)
                let first = room.tap_in(a, k);
                let last = room.taps[..room.taps.partition_point(|&f| f < k)].last().copied();
                let (Some(first), Some(last)) = (first, last.filter(|&l| l >= a)) else { continue };
                if last - first < frames(GLANCE_STREAK_MS) {
                    continue;
                }
                let ms = |r: &mut Rng, lo: u64, hi: u64| frames(r.range(lo as f64, hi as f64) as u64);
                let mut g = first + ms(&mut rng, GLANCE_FIRST_MS.0, GLANCE_FIRST_MS.1);
                while g + len <= last {
                    if (g..g + len).all(&typing) && !standing.iter().any(|s| g < s.1 && s.0 < g + len) {
                        glanced.push((g, g + len));
                    }
                    g += frames(GLANCE_EVERY_MS) + ms(&mut rng, 0, GLANCE_MORE_MS);
                }
            }
        }
        let raised: Vec<(u64, u64)> = glanced.iter().chain(&met).copied().collect();
        for &(a, b) in &raised {
            for j in a..b {
                if let Some(Pic::At(arms, Face::Open)) = pics[j as usize] {
                    pics[j as usize] = Some(Pic::At(arms, Face::Raised));
                }
            }
        }

        if sw.moves() {
            pics = turned(&pics);
        }

        // (the blinks: seeded, two frames of the closed face wherever the face is plainly open or raised — not in a
        // glance, a turn or a motion: skipped there)
        let mut blinks = 0;
        if sw.blink {
            let mut rng = Rng::new(seed, BLINKS);
            let next = |r: &mut Rng| frames(r.range(BLINK_MS.0 as f64, BLINK_MS.1 as f64) as u64);
            let mut k = next(&mut rng);
            while k + BLINK_FRAMES <= n {
                let plain = (k..k + BLINK_FRAMES).all(|j| {
                    matches!(pics[j as usize], Some(Pic::At(_, Face::Open)) | Some(Pic::WATCH))
                        && !raised.iter().any(|g| g.0 <= j && j < g.1)
                });
                if plain {
                    for j in k..k + BLINK_FRAMES {
                        if let Some(Pic::At(arms, _)) = pics[j as usize] {
                            pics[j as usize] = Some(Pic::At(arms, Face::Closed));
                        }
                    }
                    blinks += 1;
                }
                k += next(&mut rng);
            }
        }
        Plan {
            pics,
            placed,
            left_out,
            blinks,
            glances: glanced.len(),
            glanced: glanced.iter().map(|g| g.0).collect(),
            unstretched,
            would_stretch,
            holds,
            fast_taps: left.len(),
            fast: fast_held,
            fast_keys,
            fast_short,
            wound: false,
            wind_key_steps: 0,
            lines,
            painted: None,
        }
    }

    /// Each picture's steps painted on the tablet as they're drawn (its pictures found, `Art::paints`; module doc,
    /// "Painting on the tablet"). Over the picture's part and the crossing out after it: the crossing in's first half
    /// `swing-1`, then `draw-in` — the rest — but where a step is drawn, from the frame its clock reaches its first
    /// stroke's start to the frame it reaches its last's end (`Step::reaches`, as the station's inking), `rhythm`; the
    /// crossing out's first half `draw-in` still, its second `swing-1`, then the plan's own. Only the plain pictures
    /// (arms and a face) are painted over: a surprise, stretch or swing stays.
    pub(crate) fn paint(&mut self, prog: &Program) {
        let mut painted = Painted::default();
        let cross = CROSS_MS * 1000;
        for p in &prog.parts {
            let What::Picture(i) = p.what else { continue };
            let pic = &prog.cut.pictures[i];
            let a = Program::frame_at(p.at);
            let (crossed, b) = (Program::frame_at(p.at + cross), Program::frame_at(p.at + p.len));
            let out = Program::frame_at(p.at + p.len + cross);
            let at = |k: u64| (k - a) as usize;
            let mut lay = vec![Pic::DrawIn; at(out)];
            lay[..at(a + (crossed - a) / 2)].fill(Pic::Swing(0));
            lay[at(b + (out - b) / 2)..].fill(Pic::Swing(0));
            let first = Program::time_of(a);
            // (each step's run, by its step)
            let mut stepped: Vec<(usize, u64, u64)> = Vec::new();
            for (si, (s, &start)) in pic.steps.iter().zip(&pic.starts()).enumerate() {
                painted.steps.1 += 1;
                let from = s.strokes.iter().map(|x| x.at).min();
                let to = s.strokes.iter().map(|x| x.at + x.ms).max();
                let (Some(from), Some(to)) = (from, to) else { continue };
                let (s0, e) = (s.reaches(first + start, from * 1000), s.reaches(first + start, to * 1000));
                let run = rhythm(e - s0);
                if run.is_empty() {
                    continue;
                }
                lay[at(s0)..at(e)].copy_from_slice(&run);
                painted.steps.0 += 1;
                painted.runs.push((s0, e));
                stepped.push((si, s0, e));
            }
            for (j, &l) in lay.iter().enumerate() {
                let k = a as usize + j;
                if !matches!(self.pics.get(k), Some(Some(Pic::At(..)))) {
                    continue;
                }
                self.pics[k] = Some(l);
                match l {
                    Pic::Sweep(m) => painted.sweep[usize::from(m)] += 1,
                    Pic::Return(m) => painted.back[usize::from(m)] += 1,
                    Pic::DrawLift => painted.lift += 1,
                    Pic::DrawIn => painted.rest += 1,
                    _ => painted.turns += 1,
                }
            }
            // (each sweep as shown: from a `sweep-1` the frame before isn't across, as long as the brush is on the tablet)
            let across = |k: u64| matches!(self.pics.get(k as usize), Some(Some(Pic::Sweep(_))));
            for &(step, s0, e) in &stepped {
                let (mut k, mut nth) = (s0, 0);
                while k < e {
                    let starts = self.pics.get(k as usize) == Some(&Some(Pic::Sweep(0)))
                        && !k.checked_sub(1).is_some_and(across);
                    let n = match starts {
                        true => (k..e).take_while(|&j| across(j)).count() as u64,
                        false => 0,
                    };
                    if n > 0 {
                        painted.sweeps.push(Sweep { at: k, frames: n, picture: i, step, nth });
                        nth += 1;
                    }
                    k += n.max(1);
                }
            }
        }
        // (the stands of the rhythm's pictures shorter than their holds, all through)
        let mut run: Option<(Pic, u64)> = None;
        for k in 0..=self.pics.len() {
            let now = self.pics.get(k).copied().flatten().filter(|&p| hold(p).is_some());
            match (run, now) {
                (Some((p, n)), Some(q)) if p == q => run = Some((p, n + 1)),
                _ => {
                    if let Some((p, n)) = run
                        && hold(p).is_some_and(|h| n < h)
                    {
                        painted.under += 1;
                    }
                    run = now.map(|q| (q, 1));
                }
            }
        }
        self.painted = Some(painted);
    }

    /// Each fast-forward hold wound on the skein instead of held (its pictures found, `Art::winds`): the hold's own
    /// frames — the held paw, its face's turns; a surprise or swing over it stays — become `WIND_IN_FRAMES` of the
    /// in-between, then the loop, a step every `WIND_EVERY` frames (`WIND_EVERY_FAST` on a frame past
    /// ×`WIND_FAST_PAST`), a key shown in the hold (`hands`' sped keys) stepping it at once — those closer than
    /// `WIND_KEY_GAP` share one. The frame after the hold is the plan's own: the hands over the keys.
    pub(crate) fn wind(&mut self, prog: &Program, hands: &Hands) {
        let own =
            |p: Option<Pic>| matches!(p, Some(Pic::At(Arms::Right | Arms::Both, Face::Raised | Face::Half)));
        let keys = hands.sped_frames();
        let mut key_steps = 0;
        for &(a, b) in &self.fast {
            let (mut seen, mut since, mut step) = (0, 0, 0u8);
            let mut ki = keys.partition_point(|&f| f < a);
            for k in a..b {
                // (a key on this frame: one closer than `WIND_KEY_GAP` to the last step shares it)
                let mut keyed = false;
                while ki < keys.len() && keys[ki] <= k {
                    keyed = true;
                    ki += 1;
                }
                if !own(self.pic(k)) {
                    continue;
                }
                seen += 1;
                let pic = match seen {
                    s if s <= WIND_IN_FRAMES => Pic::WindIn,
                    s if s == WIND_IN_FRAMES + 1 => Pic::Wind(0),
                    _ => {
                        since += 1;
                        let fast = prog.frame(Program::time_of(k)).speed.is_some_and(|s| s > WIND_FAST_PAST);
                        let every = if fast { WIND_EVERY_FAST } else { WIND_EVERY };
                        let by_key = keyed && since >= WIND_KEY_GAP && since < every;
                        if by_key || since >= every {
                            key_steps += usize::from(by_key);
                            (step, since) = ((step + 1) % Pic::WIND_STEPS, 0);
                        }
                        Pic::Wind(step)
                    }
                };
                self.pics[k as usize] = Some(pic);
            }
        }
        self.wound = true;
        self.wind_key_steps = key_steps;
    }

    /// Frame `k`'s picture: None on a card.
    pub(crate) fn pic(&self, k: u64) -> Option<Pic> {
        self.pics.get(k as usize).copied().flatten()
    }

    /// What `cut.txt` says: the counts, then each surprise, stretch and swing with its time in the video and its cause,
    /// and each one left out.
    pub(crate) fn said(&self, sw: &Switches) -> Vec<String> {
        let of = |k: Kind| self.placed.iter().filter(|p| p.kind == k).count();
        let mut out = vec![format!(
            "the mascot's motions ({}): {} surprises, {} stretches, {} swings, {} left out; {} blinks, {} glances",
            sw.said(),
            of(Kind::Surprise),
            of(Kind::Stretch),
            of(Kind::Swing),
            self.left_out.len(),
            self.blinks,
            self.glances
        )];
        for p in &self.placed {
            let moved = match p.moved {
                0 => String::new(),
                d => format!(", moved {:+.1} s", d as f64 / FPS as f64),
            };
            let typing = if p.typing { ", over typing (the wide face)" } else { "" };
            out.push(format!(
                "  {} at {} (take {}): {}{moved}{typing}",
                p.kind.word(),
                secs(p.from),
                clock(p.t),
                p.cause
            ));
        }
        out.extend(self.left_out.iter().map(|l| format!("  {l}")));
        if sw.fast {
            let at: Vec<String> = (self.fast.iter())
                .map(|&(a, b)| format!("{} ({:.1} s)", secs(a), (b - a) as f64 / FPS as f64))
                .collect();
            out.push(match self.wound {
                true => format!(
                    "  fast-forward wound on the skein (a ▸▸ pop): {}{}; {} frames of the in-between, then a step every {} \
                     frames, every {} past ×{}; {} steps made at once by the {} keys shown in them; {} fast stretches \
                     under {} ms of video: none",
                    self.fast.len(),
                    if at.is_empty() { String::new() } else { format!(", at {}", at.join(", ")) },
                    WIND_IN_FRAMES,
                    WIND_EVERY,
                    WIND_EVERY_FAST,
                    WIND_FAST_PAST,
                    self.wind_key_steps,
                    self.fast_keys,
                    self.fast_short,
                    FAST_MIN_MS
                ),
                false => format!(
                "  fast-forward held (the right paw, a ▸▸ pop): {}{}; the left paw tapped {} times for the {} keys shown \
                 in them; {} fast stretches under {} ms of video: none",
                self.fast.len(),
                if at.is_empty() { String::new() } else { format!(", at {}", at.join(", ")) },
                    self.fast_taps,
                    self.fast_keys,
                    self.fast_short,
                    FAST_MIN_MS
                ),
            });
        }
        if let Some(p) = &self.painted {
            let dots = |v: &mut dyn Iterator<Item = String>| v.collect::<Vec<_>>().join("·");
            let held = |h: &[(u8, u64)]| dots(&mut h.iter().map(|x| x.1.to_string()));
            out.push(format!(
                "  painting on the tablet: {} steps of {}; a sweep of sweep-1 … -7 held {} frames, return-1 … -3 {} in \
                 the air, draw-lift {PAINT_LIFT} ({}), again — {}; from each step's first \
                 stroke to its last's end, then at rest on draw-in; the keyboard a tablet inside the board's crossings \
                 (swing-1, draw-in); frames: sweep-1 … -7 {}, return-1 … -3 {}, lifted {}, draw-in {}, swing-1 {}",
                p.steps.0,
                p.steps.1,
                held(&SWEEP),
                held(&RETURN),
                sweep().iter().map(|s| s.1).sum::<u64>(),
                match p.under {
                    0 => "no picture held under its own hold".to_string(),
                    n => format!("{n} stands under their own holds"),
                },
                dots(&mut p.sweep.iter().map(|n| n.to_string())),
                dots(&mut p.back.iter().map(|n| n.to_string())),
                p.lift,
                p.rest,
                p.turns
            ));
            for &(s0, e) in &p.runs {
                out.push(format!(
                    "    painted {} to {} ({:.1} s)",
                    secs(s0),
                    secs(e),
                    (e - s0) as f64 / FPS as f64
                ));
            }
        }
        if sw.says {
            out.push(format!("  {}", self.lines.said()));
            if !self.lines.glanced.is_empty() {
                let at: Vec<String> =
                    self.lines.glanced.iter().take(GLANCES_SAID).map(|&k| secs(k)).collect();
                let more = self.lines.glanced.len().saturating_sub(GLANCES_SAID);
                let more = if more > 0 { format!(" and {more} more") } else { String::new() };
                out.push(format!("    glanced at {}{more}", at.join(", ")));
            }
        }
        if !self.unstretched.is_empty() {
            out.push(format!(
                "  watches of {} s or more with no stretch: {}",
                STRETCH_QUIET_MS / 1000,
                self.unstretched.len()
            ));
            for (from, len, why) in &self.unstretched {
                out.push(format!("    at {}, {:.1} s: {why}", secs(*from), *len as f64 / FPS as f64));
            }
        }
        if !self.would_stretch.is_empty() {
            let tried: Vec<String> =
                self.would_stretch.iter().map(|(ms, n)| format!("{} s: {n}", ms / 1000)).collect();
            out.push(format!("  stretches had the quiet been {} (the caps the same)", tried.join(", ")));
        }
        if !self.glanced.is_empty() {
            let at: Vec<String> = self.glanced.iter().take(GLANCES_SAID).map(|&k| secs(k)).collect();
            let more = self.glanced.len().saturating_sub(GLANCES_SAID);
            let more = if more > 0 { format!(" and {more} more") } else { String::new() };
            out.push(format!("  glances at {}{more}", at.join(", ")));
        }
        out
    }
}

/// `pics` with the face turned through its half-raised picture wherever it goes between the keys and the screen:
/// `TURN` frames into the raised face after the keys; `TURN` frames before the keys after it (the arms over the keys if
/// a key comes; resting if a whole-body motion does) — but into a surprise, the one abrupt start.
pub(crate) fn turned(pics: &[Option<Pic>]) -> Vec<Option<Pic>> {
    let mut out = pics.to_vec();
    for k in 1..pics.len() {
        let (Some(a), Some(b)) = (pics[k - 1], pics[k]) else { continue };
        match (level(a), level(b)) {
            (Level::Down, Level::Up) => {
                for j in k..(k + TURN as usize).min(pics.len()) {
                    match pics[j] {
                        Some(Pic::At(arms, Face::Raised)) => out[j] = Some(Pic::At(arms, Face::Half)),
                        _ => break,
                    }
                }
            }
            (Level::Up, Level::Down) if !matches!(b, Pic::Surprise(_) | Pic::At(_, Face::Wide)) => {
                let keys = matches!(b, Pic::At(x, _) if x != Arms::Rest);
                for j in (k.saturating_sub(TURN as usize)..k).rev() {
                    match pics[j] {
                        Some(Pic::At(arms, Face::Raised)) => {
                            let arms = if arms == Arms::Rest && keys { Arms::Up } else { arms };
                            out[j] = Some(Pic::At(arms, Face::Half));
                        }
                        _ => break,
                    }
                }
            }
            _ => {}
        }
    }
    out
}
