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
//! - **Stretch**: 6 s of video with no key heard (`STRETCH_QUIET_MS`); 1.2 s, at most once a minute.
//! - **Brush swing**: a chapter's last second before its card (the first's: where it ends), and a bring that landed;
//!   0.9 s.
//! - **Whole-body motions (stretch, swing) only where they fit whole** (`Room`): from the half-turn before them to
//!   `MARGIN_MS` of hands back over the keys after, no key heard, no card, no surprise; one every `BODY_EVERY_MS`. A swing
//!   with no room at its moment moves to the nearest within `SWING_MOVE_MS` either side, else it's left out (said); it
//!   also waits for the key pops to go, its trail beside their lane. After one the hands stay over the keys `QUIET_MS`,
//!   as after a key.
//! - **One at a time**, placed in this order: surprise, swing, stretch, glance, blink — each only where the ones before
//!   left room (a blink or a glance with none is skipped).
//! - **The face turns, it doesn't jump** (`turned`): between the face on the keys and the raised one, `TURN` frames of
//!   the half-raised; after a watch it starts down `TURN` frames before the key (the arms over the keys), so the eyes are
//!   on the keyboard when the hand taps. The surprise's start is the one change with no turn. A tap stays a snap.
//! - **The same cut is the same video**: the blinks and glances come from a sequence seeded by the edit list (`Cut::seed`).
//! - Every switch off: V4's mascot, picture for picture (no turn either).

use super::{Arms, Face, Hands, Pic, Pose, QUIET_MS, Switches};
use crate::cut::sound::KEYS_MAX_SPEED;
use crate::cut::{Marks, Pace, Program, What, clock, ms_of};
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

/// What makes a surprise or a swing: where in the video (µs — a swing's, where it would start), the take's ms, in words.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Cause {
    pub(crate) v: u64,
    pub(crate) t: u64,
    pub(crate) said: String,
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
            c.surprise.extend(shown_at(prog, t).into_iter().map(|v| Cause { v, t, said: said.clone() }));
        }
    }
    let swing = len_of(&SWING) * 1_000_000 / FPS + MARGIN_MS * 1000;
    for (i, p) in prog.parts.iter().enumerate() {
        let What::Chapter(n) = p.what else { continue };
        let before = i.checked_sub(1).map(|j| prog.parts[j].what);
        if !matches!(before, Some(What::Ends { .. } | What::Take { .. })) {
            continue;
        }
        let v = p.at.saturating_sub(swing);
        let f = prog.frame(v);
        let what = match (f.ends, n) {
            (true, _) => "where it ends".to_string(),
            (false, 0) => "the take before chapter 1".to_string(),
            (false, n) => format!("chapter {n}'s end"),
        };
        let t = f.clock.unwrap_or(0);
        c.swing.push(Cause { v, t, said: format!("{what}, before chapter {}'s card", n + 1) });
    }
    for v in marks.of("bring").filter(|v| v["how"] == "landed") {
        let t = ms_of(v);
        let said = format!("a bring landed: {}", v["name"].as_str().unwrap_or("?"));
        c.swing.extend(shown_at(prog, t).into_iter().map(|v| Cause { v, t, said: said.clone() }));
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
}

/// What a whole-body motion's frames must be clear of.
struct Room<'a> {
    n: u64,
    /// Frames not shown before each frame (a prefix sum: a stretch shown wholly when it adds none).
    unshown: Vec<u32>,
    taps: &'a [u64],
    hold: u64,
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

    /// Whether a `len`-frame whole-body motion fits from frame `k` among `placed`; else why not, in words.
    fn fits(&self, k: u64, len: u64, placed: &[Placed], pops: bool) -> std::result::Result<(), String> {
        let end = k + len + self.margin;
        if k < TURN || !self.shown(k - TURN, end) {
            return Err("a card or the video's edge".into());
        }
        if let Some(f) = self.tap_in(k.saturating_sub(self.hold), end) {
            return Err(format!("a key at {}", secs(f)));
        }
        for p in placed {
            let near = match p.kind {
                Kind::Surprise => k - TURN < p.from + p.len + TURN && p.from < end + TURN,
                _ => k.abs_diff(p.from) < frames(BODY_EVERY_MS),
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
            why = match (k < next_ok, room.fits(k, len, &all, false)) {
                (true, _) => Some("within a minute of the last stretch".into()),
                (false, Err(e)) => Some(e),
                (false, Ok(())) => None,
            };
        }
        if k >= quiet_from + need && k >= next_ok && room.fits(k, len, &all, false).is_ok() {
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

/// A frame's video time, for what's said (`m:ss.s`).
fn secs(k: u64) -> String {
    let ms = Program::time_of(k) / 1000;
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
        for k in 0..n {
            let f = prog.frame(Program::time_of(k));
            shown[k as usize] = f.card.is_none();
            fast[k as usize] = f.speed.is_some_and(|s| s > KEYS_MAX_SPEED + 1e-6);
            unshown[k as usize + 1] = unshown[k as usize] + u32::from(f.card.is_some());
        }
        let room = Room {
            n,
            unshown,
            taps: hands.tap_frames(),
            hold: frames(super::HOLD_MS),
            margin: frames(MARGIN_MS),
            pops,
        };
        let mut placed: Vec<Placed> = Vec::new();
        let mut left_out = Vec::new();
        let at_take = |k: u64| prog.frame(Program::time_of(k)).clock.unwrap_or(0);

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
        // (the swings: at their moment, else the nearest room within `SWING_MOVE_MS`, else left out)
        if sw.swing {
            let len = len_of(&SWING);
            let most = frames(SWING_MOVE_MS) as i64;
            for c in &causes.swing {
                let ideal = Program::frame_at(c.v) as i64;
                let tries = (0..=most).flat_map(|d| if d == 0 { vec![0] } else { vec![-d, d] });
                let found = tries
                    .filter(|d| ideal + d >= 0)
                    .find(|d| room.fits((ideal + d) as u64, len, &placed, true).is_ok());
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
                        let why =
                            room.fits(ideal.max(0) as u64, len, &placed, true).err().unwrap_or_default();
                        left_out.push(format!(
                            "swing left out at {}: {} — no room within {} s either side ({why} at its moment)",
                            secs(ideal.max(0) as u64),
                            c.said,
                            SWING_MOVE_MS / 1000
                        ));
                    }
                }
            }
        }
        // (the stretches; and, for the list's tuning, how many other quiets would give)
        let (mut unstretched, mut would_stretch) = (Vec::new(), Vec::new());
        if sw.stretch {
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
        let body_ends: Vec<u64> =
            (placed.iter()).filter(|p| p.kind != Kind::Surprise).map(|p| p.from + p.len).collect();
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
            let a = arms(base(k));
            let pic = match placed.get(at).filter(|p| p.from <= k) {
                Some(p) => match p.kind {
                    Kind::Surprise if p.typing => {
                        Pic::At(if a == Arms::Rest { Arms::Up } else { a }, Face::Wide)
                    }
                    Kind::Surprise => Pic::Surprise(nth(&SURPRISE, k - p.from)),
                    Kind::Stretch => Pic::Stretch(nth(&STRETCH, k - p.from)),
                    Kind::Swing => Pic::Swing(nth(&SWING, k - p.from)),
                },
                None if a == Arms::Rest => Pic::WATCH,
                None => Pic::At(a, Face::Open),
            };
            pics[k as usize] = Some(pic);
        }

        // (the glances: seeded in each typing streak of `GLANCE_STREAK_MS` or more, inside it, clear of a surprise)
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
                    if (g..g + len).all(&typing) {
                        glanced.push((g, g + len));
                    }
                    g += frames(GLANCE_EVERY_MS) + ms(&mut rng, 0, GLANCE_MORE_MS);
                }
            }
            for &(a, b) in &glanced {
                for j in a..b {
                    if let Some(Pic::At(arms, Face::Open)) = pics[j as usize] {
                        pics[j as usize] = Some(Pic::At(arms, Face::Raised));
                    }
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
                        && !glanced.iter().any(|g| g.0 <= j && j < g.1)
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
        }
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
