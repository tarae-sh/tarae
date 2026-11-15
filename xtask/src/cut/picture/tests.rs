//! Picture cards' tests: their pictures made here, in a scratch folder — small boards for the reveal and the layers'
//! arithmetic, 2160p's board where the cut reads files. No picture of the video's is read.

use std::path::Path;

use serde_json::{Value, json};
use tarae_testkit::scratch::Scratch;

use super::super::{Program, What};
use super::*;

/// A made-up take's landmarks, two minutes long: `fix` in front at 50 s.
fn marks() -> Marks {
    Marks::new(vec![json!({ "what": "front", "to": "w1", "name": "fix", "ms": 50_000 })], 120_000)
}

/// A list at 1×: a chapter's card at the start, `rest`, the opening (where it ends, the take's last 4 s).
fn list(rest: &str) -> String {
    format!(
        "default = 1\n\n[[chapter]]\nat = \"start\"\ntitle = \"One\"\n\n{rest}\n[opening]\nname = \"halil\"\n\
         line = \"A task manager.\"\nat = \"end - 4s\"\nfor = \"3s\"\n"
    )
}

/// `text` read as `dir`'s `cut.toml` (its pictures from there), and that name.
fn read(text: &str, dir: &Path) -> (Result<Cut>, String) {
    let file = dir.join("cut.toml").display().to_string();
    (Cut::parse(text, &file, &marks()), file)
}

/// The line (from 1) of `text`'s `nth` line (from 0) that starts with `start` (spaces before it aside).
fn line_of(text: &str, start: &str, nth: usize) -> usize {
    (text.lines().enumerate())
        .filter(|(_, l)| l.trim_start().starts_with(start))
        .nth(nth)
        .map(|(i, _)| i + 1)
        .unwrap_or_else(|| panic!("no line {nth} starting {start:?} in:\n{text}"))
}

/// A step's manifest: its drawing's `ms`, a straight stroke and a fade.
fn manifest(ms: u64) -> Value {
    json!({
        "size": [3840, 1860], "ms": ms,
        "strokes": [{ "at": 0, "ms": 400, "r": 30, "path": [[200, 200], [1200, 200]] }],
        "fades": [{ "at": 200, "ms": 300, "rect": [100, 400, 600, 200] }],
    })
}

/// A step's four files in `dir` as names only (the list's checks read no picture): NAME.png and its layers empty,
/// NAME.json `m`.
fn named(dir: &Path, name: &str, m: &Value) {
    for f in [format!("{name}.png"), format!("{name}.ink.png"), format!("{name}.wash.png")] {
        std::fs::write(dir.join(f), b"").unwrap();
    }
    std::fs::write(dir.join(format!("{name}.json")), m.to_string()).unwrap();
}

const PICTURE: &str = "[[picture]]\nat = \"20s\"\nsteps = [\n  { file = \"a.png\", for = \"1.5s\", text = \"One ztunnel on each node.\" },\n  \
                       { file = \"b.png\", for = \"1s\", speed = 2 },\n]\n";

// ── The reveal ──

/// A step of `strokes` and `fades` alone (its drawing `ms`), for the arithmetic.
fn step_of(ms: u64, strokes: Vec<Stroke>, fades: Vec<Fade>) -> Step {
    let file = PathBuf::from("/nowhere/s.png");
    Step { file, name: "s".into(), hold: 1_000, speed: 1.0, text: Vec::new(), line: 1, ms, strokes, fades }
}

/// An RGBA layer of `dims`, `alpha` everywhere in `rgb`.
fn layer_of(dims: (usize, usize), rgb: Rgb, alpha: u8) -> Vec<u8> {
    [rgb[0], rgb[1], rgb[2], alpha].repeat(dims.0 * dims.1)
}

/// Each ink pixel comes in as the brush first comes within `r` of it: the smaller root along a segment, at its share of
/// the stroke's length; within `r` of the start at once; past `r` of the whole path never (`NONE`); where two strokes
/// reach a pixel, the earlier.
#[test]
fn a_stroke_inks_each_pixel_as_the_brush_first_comes_within_r_of_it() {
    let dims = (130, 30);
    let line = Stroke { at: 0, ms: 100, r: 5.0, path: vec![(10.0, 10.0), (110.0, 10.0)] };
    let s = step_of(200, vec![line.clone()], Vec::new());
    let ink = layer_of(dims, [200; 3], 255);
    let t0 = reveal(&s, &ink, dims, 1.0);
    let at = |x: usize, y: usize| t0[y * dims.0 + x];
    // (pixel (60, 10)'s centre (60.5, 10.5): u = (5050 − √247 500) / 10 000 of the way, of 100 ms)
    assert_eq!(at(60, 10), ((5050.0 - 247_500f64.sqrt()) / 10_000.0 * 100_000.0).round() as u32);
    assert_eq!(at(10, 10), 0, "within r of the start: at once");
    let row: Vec<u32> = (15..108).map(|x| at(x, 10)).collect();
    assert!(row.windows(2).all(|w| w[0] < w[1]), "in the brush's order: {row:?}");
    assert!(at(60, 12) > at(60, 10), "off the path, a little later: the disc's edge reaches it after");
    assert_eq!((at(60, 16), at(120, 10), at(2, 10)), (NONE, NONE, NONE), "past r: no stroke reaches");
    // (a second stroke at 150 ms back over the first: the first's time stands where both reach)
    let back = Stroke { at: 150, ms: 50, r: 5.0, path: vec![(110.0, 10.0), (10.0, 10.0)] };
    let both = reveal(&step_of(200, vec![line, back], Vec::new()), &ink, dims, 1.0);
    assert_eq!(both, t0, "the earliest over every stroke");
    // (no ink, no time; at half the size, the same times at half the coordinates)
    assert!(reveal(&s, &layer_of(dims, [200; 3], 0), dims, 1.0).iter().all(|&t| t == NONE));
    let half = reveal(&s, &ink, (65, 15), 0.5);
    let (a, b) = (half[5 * 65 + 30], at(61, 11));
    assert!(a.abs_diff(b) <= 1_000, "1080p's pixel (30, 5) as 2160p's (61, 11): {a} vs {b}");
}

/// A path shorter than `DAB_PX` is a dab: a disc growing from its point over its ms.
#[test]
fn a_dab_grows_from_its_point_over_its_ms() {
    let dims = (40, 40);
    let dab = Stroke { at: 100, ms: 200, r: 10.0, path: vec![(20.0, 20.0), (20.4, 20.0), (20.0, 20.0)] };
    let t0 = reveal(&step_of(400, vec![dab], Vec::new()), &layer_of(dims, [9; 3], 255), dims, 1.0);
    let at = |x: usize, y: usize| t0[y * dims.0 + x];
    let d = (5.5f64 * 5.5 + 0.5 * 0.5).sqrt();
    assert_eq!(at(25, 20), (100_000.0 + 200_000.0 * d / 10.0).round() as u32);
    assert_eq!(at(20, 20), (100_000.0 + 200_000.0 * 0.5f64.sqrt() / 10.0).round() as u32);
    assert_eq!((at(31, 20), at(20, 31)), (NONE, NONE), "past r");
}

/// A held step of one pixel row: the board before it ground, its ink `INK` at `t0`, its wash `WASH` in `rects`.
fn held_row(t0: Vec<u32>, wash: Option<Vec<u8>>, held: Vec<bool>, rects: Vec<[usize; 4]>) -> Held {
    let n = t0.len();
    let ink = Some(layer_of((n, 1), INK, 255));
    let (before, whole) = ([10u8; 3].repeat(n), INK.repeat(n));
    Held { key: (0, 0), w: n, before, whole, ink, wash, t0, held, rects }
}

const INK: Rgb = [210, 200, 180];
const WASH: Rgb = [60, 90, 140];
const FULL: Rgb = [250, 250, 250];

/// The leading end is soft: a pixel's ink comes up over `SOFT_MS` from its time, eased, never at once; fresh ink is
/// lifted `FRESH` toward the look's ink and dries over `DRY_MS`; without fresh ink, laid as it is. What no stroke
/// reaches comes in from the step's ms over `FADE_MS`. Settled, the board is the step's own picture.
#[test]
fn the_leading_end_is_soft_and_fresh_ink_dries_over_a_second() {
    let s = step_of(1_000, Vec::new(), Vec::new());
    let h = held_row(vec![100_000, NONE], None, vec![false; 2], Vec::new());
    let px = |t: u64, fresh: bool| h.board(&s, Some(t), fresh, FULL)[..3].to_vec();
    let late = |t: u64| h.board(&s, Some(t), false, FULL)[3..6].to_vec();
    assert_eq!(px(100_000, false), [10; 3], "at its time: nothing yet");
    let mid = px(100_000 + SOFT_MS * 500, false);
    assert!(mid[0] > 10 && mid[0] < INK[0], "halfway up its soft end: {mid:?}");
    assert_eq!(mid[0], (10.0 + (210.0 - 10.0) * 0.5f32).round() as u8, "eased: half at half");
    assert_eq!(px(100_000 + SOFT_MS * 1000, false), INK, "whole past its soft end");
    // (fresh: lighter toward the look's ink at first, drying to the ink itself)
    let wet = px(100_000 + SOFT_MS * 1000, true);
    assert!(wet[0] > INK[0] && wet[2] > INK[2], "{wet:?}");
    let lift = FRESH * (1.0 - super::smooth(SOFT_MS as f64 * 1000.0, DRY_MS as f64 * 1000.0));
    assert_eq!(wet[2], (180.0 + (250.0 - 180.0) * lift).round() as u8);
    assert_eq!(px(100_000 + DRY_MS * 1000, true), INK, "dry after a second");
    // (no stroke reaches the second pixel: it comes at the step's ms over FADE_MS)
    assert_eq!(late(999_999), [10; 3]);
    assert!(late(1_000_000 + FADE_MS * 500)[0] > 10);
    assert_eq!(late(1_000_000 + FADE_MS * 1000), INK);
    assert_eq!(h.board(&s, None, true, FULL), h.whole, "settled: the step's own picture");
    assert_eq!((s.settles(true), s.settles(false)), (2_000_000, 1_200_000));
}

/// A wash pixel's share is the strongest of the running fades whose rectangle holds it; a pixel in none comes at the
/// step's ms.
#[test]
fn a_wash_pixel_takes_the_strongest_fade_whose_rectangle_holds_it() {
    let fades = vec![
        Fade { at: 0, ms: 100, rect: [0.0, 0.0, 2.0, 1.0] },
        Fade { at: 50, ms: 100, rect: [1.0, 0.0, 2.0, 1.0] },
    ];
    let s = step_of(400, Vec::new(), fades);
    let (held, rects) = cover(&s, (4, 1), 1.0);
    assert_eq!(
        (held.clone(), rects.clone()),
        (vec![true, true, true, false], vec![[0, 0, 2, 1], [1, 0, 3, 1]])
    );
    let wash = Some(layer_of((4, 1), WASH, 255));
    let mut h = held_row(vec![NONE; 4], wash, held, rects);
    h.ink = None;
    let at = |t: u64| h.board(&s, Some(t), true, FULL);
    let shade = |px: &[u8], i: usize| px[i * 3 + 2];
    let b = at(75_000);
    // (at 75 ms the first fade is 0.84 in, the second 0.16: pixel 1, in both, takes the first's)
    assert_eq!(shade(&b, 1), shade(&b, 0), "the stronger of the two");
    assert!(shade(&b, 2) < shade(&b, 1) && shade(&b, 2) > 10, "the second's alone: {b:?}");
    assert_eq!(shade(&b, 3), 10, "in no rectangle: not before the step's ms");
    assert_eq!(shade(&at(400_000 + FADE_MS * 1000), 3), WASH[2], "in by the step's ms and FADE_MS");
}

/// The layers must make the step's picture: wash, then ink, over the board before, each pixel within `MISS_BY` of it;
/// what's further off is counted, the first said.
#[test]
fn the_layers_must_make_the_steps_picture() {
    let dims = (20, 10);
    let line = Stroke { at: 0, ms: 100, r: 3.0, path: vec![(2.0, 2.0), (18.0, 2.0)] };
    let s = step_of(200, vec![line], vec![Fade { at: 0, ms: 100, rect: [0.0, 0.0, 20.0, 10.0] }]);
    let before = [10u8; 3].repeat(200);
    let mut ink = vec![0u8; 800];
    let mut wash = vec![0u8; 800];
    for x in 2..18 {
        ink[(2 * 20 + x) * 4..][..4].copy_from_slice(&[INK[0], INK[1], INK[2], 200]);
        wash[(7 * 20 + x) * 4..][..4].copy_from_slice(&[WASH[0], WASH[1], WASH[2], 128]);
    }
    // (a wash under the stroke: wash then ink, not the other way)
    wash[(2 * 20 + 5) * 4..][..4].copy_from_slice(&[WASH[0], WASH[1], WASH[2], 128]);
    let whole = made(&before, Some(&wash), Some(&ink));
    // (at (5, 2): the wash at 128 over 10, 35; the ink at 200 over that, 172 — the ink first, then the wash: 113)
    assert_eq!(whole[(2 * 20 + 5) * 3], 172, "wash, then ink");
    let f = measure(&s, (&before, &whole), (Some(&ink), Some(&wash)), dims, 1.0);
    assert_eq!(f, Found { missed: 0, first: None, ink: (0, 16), wash: (0, 17) });
    let mut off = whole.clone();
    off[(7 * 20 + 4) * 3] += MISS_BY;
    assert_eq!(
        measure(&s, (&before, &off), (Some(&ink), Some(&wash)), dims, 1.0).missed,
        0,
        "within MISS_BY"
    );
    off[(7 * 20 + 4) * 3] += 1;
    off[(9 * 20 + 19) * 3 + 2] = 99;
    let f = measure(&s, (&before, &off), (Some(&ink), Some(&wash)), dims, 1.0);
    assert_eq!((f.missed, f.first), (2, Some((4, 7))));
    // (the ink under the wash: not what was made)
    let wrong = made(&before, Some(&ink), Some(&wash));
    assert!(measure(&s, (&before, &wrong), (Some(&ink), Some(&wash)), dims, 1.0).missed > 0);
}

/// What came at the end: ink no stroke reaches and wash no rectangle holds, counted only above `FLOOR` — a stroke's
/// faint skirt past a tight `r` is not.
#[test]
fn what_came_at_the_end_counts_only_pixels_above_the_floor() {
    let dims = (20, 10);
    let line = Stroke { at: 0, ms: 100, r: 2.0, path: vec![(2.0, 2.0), (18.0, 2.0)] };
    let s = step_of(200, vec![line], vec![Fade { at: 0, ms: 100, rect: [0.0, 0.0, 10.0, 10.0] }]);
    let before = [10u8; 3].repeat(200);
    let mut ink = vec![0u8; 800];
    let mut wash = vec![0u8; 800];
    let put = |l: &mut Vec<u8>, x: usize, y: usize, a: u8| {
        l[(y * 20 + x) * 4..][..4].copy_from_slice(&[90, 90, 90, a])
    };
    for x in 3..17 {
        put(&mut ink, x, 2, 255);
        put(&mut ink, x, 6, FLOOR);
        put(&mut wash, x, 8, 255);
    }
    put(&mut ink, 10, 8, FLOOR + 1);
    let whole = made(&before, Some(&wash), Some(&ink));
    let f = measure(&s, (&before, &whole), (Some(&ink), Some(&wash)), dims, 1.0);
    assert_eq!((f.ink, f.wash), ((1, 15), (7, 14)), "the skirt at FLOOR not counted, one above it is");
}

// ── The list ──

/// A picture stops the take at its point: a part of its own (the crossing in, each step drawn and standing, whole
/// tenths), the take after it shown as it would be without it, moved by its length; the band's clock standing at
/// `at`; each step from a whole frame, its clock its video time times its speed, settled after its ms and the drying
/// — one board while it stands —; its text a caption's line. At a chapter's card's point the card first; at the
/// take's end, after its last screen held.
#[test]
fn a_picture_stops_the_take_its_clock_standing_and_draws_each_step_from_a_whole_frame() {
    let dir = Scratch::new("xtask-picture-list");
    named(dir.path(), "a", &manifest(2_281));
    named(dir.path(), "b", &manifest(500));
    let (c, _) = read(&list(PICTURE), dir.path());
    let c = c.unwrap();
    let (plain, _) = read(&list(""), dir.path());
    let plain = plain.unwrap();
    let (p, p0) = (Program::new(&c), Program::new(&plain));
    let whats: Vec<What> = p.parts.iter().map(|x| x.what).collect();
    assert_eq!(
        &whats[3..7],
        [What::Chapter(0), What::Take { from: 0 }, What::Picture(0), What::Take { from: 20_000_000 }]
    );
    let pic = &c.pictures[0];
    // (in 0.2 s; a: 2.3 s drawn, 1.5 s standing; b: 0.25 s at 2× → 0.3 s, 1 s)
    assert_eq!((pic.len(), pic.starts()), (5_300_000, vec![200_000, 4_000_000]));
    let part = p.parts[5];
    assert_eq!(part.len, 5_300_000);
    assert_eq!(p.total, p0.total + part.len);
    // (the take after it: the same screens on the same frames, moved by its length)
    for k in [0, 1, 2, 37, 450] {
        let v = part.at + part.len + Program::time_of(k);
        let was = p0.frame(v - part.len);
        assert_eq!((p.frame(v).screen, p.frame(v).clock), (was.screen, was.clock), "frame {k} after it");
    }
    let first = Program::time_of(Program::frame_at(part.at));
    for s in pic.starts() {
        assert_eq!(Program::time_of(Program::frame_at(first + s)), first + s, "each step from a whole frame");
    }
    let f = |off: u64| p.frame(first + off);
    let shows = |off: u64| f(off).picture.unwrap();
    assert!(
        (0..200_000).step_by(33_333).all(|o| shows(o) == Shows { n: Some(0), k: 0, t: Some(0), line: None })
    );
    assert!((0..5_300_000).step_by(33_333).all(|o| f(o).clock == Some(20_000) && f(o).speed.is_none()));
    assert_eq!(shows(200_000 + 1_000_000).t, Some(1_000_000));
    assert_eq!(shows(200_000 + FADE_MS * 1000).line, Some((0, 255)), "its line a caption's");
    // (a settles at 2.281 s + 1 s: the frames standing after are one board)
    let (a, b) = (shows(200_000 + 3_300_000), shows(200_000 + 3_500_000));
    assert_eq!((a, a.t), (b, None));
    assert_eq!(shows(200_000 + 3_200_000).t, Some(3_200_000), "drying still");
    // (b at 2×: its clock twice its video time; no text, no line)
    assert_eq!(shows(4_000_000 + 100_000), Shows { n: Some(0), k: 1, t: Some(200_000), line: None });
    // (at a card's point, the card first; at the take's end, after its last screen held)
    let at = |text: &str, when: &str| list(&text.replace("\"20s\"", &format!("\"{when}\"")));
    let (c, _) = read(&at(PICTURE, "start"), dir.path());
    let c = c.unwrap();
    let whats: Vec<What> = Program::new(&c).parts.iter().map(|x| x.what).collect();
    assert_eq!(&whats[3..6], [What::Chapter(0), What::Picture(0), What::Take { from: 0 }]);
    let (c, _) = read(&at(PICTURE, "end"), dir.path());
    let c = c.unwrap();
    let whats: Vec<What> = Program::new(&c).parts.iter().map(|x| x.what).collect();
    assert_eq!(&whats[whats.len() - 3..], [What::Last, What::Picture(0), What::End]);
    // (a chapter in the band at the picture's point: its name as the take resumes)
    let banded = format!("[[chapter]]\nat = \"20s\"\ntitle = \"Two\"\ncard = false\n\n{PICTURE}");
    let (c, _) = read(&list(&banded), dir.path());
    let c = c.unwrap();
    let p = Program::new(&c);
    let after = p.parts[5].at + p.parts[5].len;
    assert_eq!(p.frame(after + FADE_MS * 1000).chapter, Some((1, 255)));
    assert!(p.frame(after - 1).chapter.is_none() && p.frame(after - 1).picture.is_some());
}

/// Into and out of a picture the board crosses through the ground, the band never dips: those crossings are the
/// confined ones, and `Crossings` lays them on the board's bytes alone; a card's still crosses whole.
#[test]
fn into_and_out_of_a_picture_the_board_crosses_and_the_band_stays() {
    use super::super::draw::Crossings;
    let dir = Scratch::new("xtask-picture-cross");
    named(dir.path(), "a", &manifest(2_281));
    named(dir.path(), "b", &manifest(500));
    let (c, _) = read(&list(PICTURE), dir.path());
    let c = c.unwrap();
    let p = Program::new(&c);
    let part = p.parts[5];
    assert_eq!(p.confined(), [part.at, part.at + part.len]);
    assert!(p.crossings().contains(&(part.at, true)) && p.crossings().contains(&(part.at + part.len, true)));
    // (from a chapter's card into a picture, and from a picture at the take's end into the end card: whole)
    for when in ["start", "end"] {
        let (c, _) = read(&list(&PICTURE.replace("\"20s\"", &format!("\"{when}\""))), dir.path());
        let c = c.unwrap();
        let p = Program::new(&c);
        let pic = *p.parts.iter().find(|x| x.what == What::Picture(0)).unwrap();
        let want: Vec<u64> = match when {
            "start" => vec![pic.at + pic.len],
            _ => vec![pic.at],
        };
        assert_eq!(p.confined(), want, "a picture at the {when}: none to or from a card");
    }
    // (two rows of two pixels: the first the board, the second the band)
    let ground = [0u8; 3];
    let from = [100u8; 12];
    let to = [200u8; 12];
    let mut x = Crossings::new(vec![(1_000_000, true)], Vec::new()).confined(vec![1_000_000], 6);
    x.at(1_000_000 - 33_333, None);
    let share = x.at(1_000_000, Some(&from));
    let mut px = to;
    x.over(&mut px, ground, share);
    assert_eq!(px[..6], [100; 6], "its first frame: the board still the picture it leaves");
    assert_eq!(px[6..], [200; 6], "the band its own, never dipped");
    let mut whole = Crossings::new(vec![(1_000_000, true)], Vec::new());
    whole.at(1_000_000 - 33_333, None);
    let share = whole.at(1_000_000, Some(&from));
    let mut px = to;
    whole.over(&mut px, ground, share);
    assert_eq!(px, [100; 12], "a crossing not confined: the whole frame");
}

/// The sheet takes each step twice, half through its strokes and whole, each labelled with its file's name — never
/// inside a crossing.
#[test]
fn the_sheet_takes_each_step_half_drawn_and_whole() {
    let dir = Scratch::new("xtask-picture-sheet");
    named(dir.path(), "a", &manifest(2_281));
    named(dir.path(), "b", &manifest(500));
    let (c, _) = read(&list(PICTURE), dir.path());
    let c = c.unwrap();
    let p = Program::new(&c);
    let first = Program::time_of(Program::frame_at(p.parts[5].at));
    let marks: Vec<(u64, String)> =
        super::super::draw::sheet_marks(&p).into_iter().filter(|m| m.1.starts_with("picture")).collect();
    let said: Vec<&str> = marks.iter().map(|m| m.1.as_str()).collect();
    assert_eq!(
        said,
        [
            "picture 1, step 1 (a): drawing",
            "picture 1, step 1 (a): whole",
            "picture 1, step 2 (b): drawing",
            "picture 1, step 2 (b): whole"
        ]
    );
    // (a's strokes 0–400 ms: halfway at 200 ms; whole at its last frame, settled)
    assert_eq!(marks[0].0, first + 200_000 + 200_000);
    let picks = super::super::draw::sheet_picks(&p);
    for (v, what) in &marks {
        let k = Program::frame_at(*v);
        assert!(picks.get(&k).is_some_and(|m| m.contains(what)), "{what} picked at its frame");
        let f = p.frame(Program::time_of(k));
        let shows = f.picture.unwrap();
        assert_eq!(shows.t.is_none(), what.ends_with("whole"), "{what}: {shows:?}");
    }
}

/// The opening on a picture: where it ends shows it whole, the take not shown there (no take's time is "where it
/// ends"), its band `where it ends` with no clock; out of it into the take the board crosses alone.
#[test]
fn the_opening_on_a_picture_shows_it_whole() {
    let dir = Scratch::new("xtask-picture-opening");
    std::fs::write(dir.path().join("route.png"), b"").unwrap();
    let text = list("")
        .replace("at = \"end - 4s\"\n", "picture = \"route.png\"\n")
        .replace("at = \"start\"\ntitle = \"One\"\n", "at = \"start\"\ntitle = \"One\"\ncard = false\n");
    let (c, _) = read(&text, dir.path());
    let c = c.unwrap();
    assert_eq!(c.opening.picture.as_ref().map(|p| p.0.clone()), Some(dir.path().join("route.png")));
    assert!(!(0..120_000).step_by(500).any(|t| c.opening.ends(t)), "no take's time is where it ends");
    let p = Program::new(&c);
    let ends = p.parts[2];
    assert!(matches!(ends.what, What::Ends { .. }) && ends.len == 3_000_000);
    let f = p.frame(ends.at + 1_000_000);
    assert_eq!(
        (f.ends, f.clock, f.screen, f.picture),
        (true, None, None, Some(Shows { n: None, k: 0, t: None, line: None }))
    );
    assert_eq!(p.confined(), [ends.at + ends.len], "into the take: the board alone");
}

/// Every refusal of a `[[picture]]` (and the opening's picture) when the list is read, each at its line: a step's at
/// its own line, saying which.
#[test]
fn every_refusal_of_a_picture_names_its_line() {
    let dir = Scratch::new("xtask-picture-refused");
    let d = dir.path();
    named(d, "a", &manifest(1_000));
    named(d, "b", &manifest(1_000));
    let with = |f: &dyn Fn(&mut Value)| {
        let mut m = manifest(1_000);
        f(&mut m);
        m
    };
    let cases_m: Vec<(&str, Value)> = vec![
        ("size", with(&|m| m["size"] = json!([1920, 930]))),
        ("ms0", with(&|m| m["ms"] = json!(0))),
        ("past", with(&|m| m["strokes"][0]["ms"] = json!(1_200))),
        ("fpast", with(&|m| m["fades"][0]["at"] = json!(800))),
        ("r0", with(&|m| m["strokes"][0]["r"] = json!(0))),
        ("sms0", with(&|m| m["strokes"][0]["ms"] = json!(0))),
        ("nopath", with(&|m| m["strokes"][0]["path"] = json!([]))),
        ("out", with(&|m| m["strokes"][0]["path"] = json!([[3900, 10]]))),
        ("rect", with(&|m| m["fades"][0]["rect"] = json!([3800, 0, 100, 10]))),
    ];
    for (name, m) in &cases_m {
        named(d, name, m);
    }
    named(d, "noink", &manifest(1_000));
    std::fs::remove_file(d.join("noink.ink.png")).unwrap();
    named(d, "nowash", &manifest(1_000));
    std::fs::remove_file(d.join("nowash.wash.png")).unwrap();
    std::fs::write(d.join("nojson.png"), b"").unwrap();
    let one = |step: &str| format!("[[picture]]\nat = \"20s\"\nsteps = [\n  {step},\n]\n");
    let at = |when: &str| {
        format!("[[picture]]\nat = \"{when}\"\nsteps = [\n  {{ file = \"a.png\", for = \"1s\" }},\n]\n")
    };
    let m = |name: &str| one(&format!("{{ file = \"{name}.png\", for = \"1s\" }}"));
    let long = "x".repeat(41);
    let in_repo = crate::root().join("docs/brand/seal-meok-96.png");
    let opening = |what: &str| list("").replace("at = \"end - 4s\"\n", what);
    type Case = (String, (&'static str, usize), String);
    let cases: Vec<Case> = vec![
        (list("[[picture]]\nat = \"20s\"\n"), ("[[picture]]", 0), "picture: no steps".into()),
        (list("[[picture]]\nat = \"20s\"\nsteps = []\n"), ("steps", 0), "picture: steps = [] — a step at least".into()),
        (list("[[picture]]\nat = \"20s\"\nsteps = 3\n"), ("steps", 0), "picture: steps = [{ file".into()),
        (list("[[picture]]\nat = \"20s\"\nfresh = 1\nsteps = [\n  { file = \"a.png\", for = \"1s\" },\n]\n"), ("fresh", 0), "picture: fresh = true or false".into()),
        (list("[[picture]]\nat = \"20s\"\nsize = 1\nsteps = [\n  { file = \"a.png\", for = \"1s\" },\n]\n"), ("size", 0), "picture: no key size (it takes at, steps, fresh)".into()),
        (list(&one("\"a.png\"")), ("steps", 0), "picture, step 1: a table".into()),
        (list(&one("{ file = \"a.png\", for = \"1s\", size = 2 }")), ("{", 0), "picture, step 1: no key size (it takes file, for, speed, text)".into()),
        (list(&one("{ for = \"1s\" }")), ("{", 0), "picture, step 1: no file".into()),
        (list(&one("{ file = \"a.jpg\", for = \"1s\" }")), ("{", 0), "picture, step 1: file = \"a.jpg\" — the step's whole picture, NAME.png".into()),
        (list(&one("{ file = \"a.png\" }")), ("{", 0), "picture, step 1 (a): no for".into()),
        (list(&one("{ file = \"a.png\", for = \"1s\", speed = 4 }")), ("{", 0), "picture, step 1 (a): speed = 0.5 to 3".into()),
        (list(&one("{ file = \"a.png\", for = \"0.5s\" }")), ("{", 0), "picture, step 1 (a): stands 0.50 s after it's drawn — fresh ink dries over 1.0 s after it: for = \"1000ms\" at the least".into()),
        (list("[[picture]]\nat = \"20s\"\nfresh = false\nsteps = [\n  { file = \"a.png\", for = \"0.1s\" },\n]\n"), ("{", 0), "picture, step 1 (a): stands 0.10 s after it's drawn — what came at the end comes in over 200 ms after it: for = \"200ms\" at the least".into()),
        (list(&one("{ file = \"a.png\", for = \"1s\", text = 3 }")), ("{", 0), "picture, step 1 (a): text = a line, or a row of lines".into()),
        (list(&one(&format!("{{ file = \"a.png\", for = \"3s\", text = \"{long}\" }}"))), ("{", 0), "picture, step 1 (a): one line of 1 to 40 characters (41 here)".into()),
        (list(&one("{ file = \"a.png\", for = \"1s\", text = [\"From inside, you can't see who.\", \"Two.\"] }")), ("{", 0), "picture, step 1 (a), line 1 of 2: stands 1.00 s (its share of the step's 2.00 s, drawn and standing) — 31 characters need 2.17 s to be read".into()),
        (list(&m("gone")), ("{", 0), format!("picture, step 1 (gone): {} not there", d.join("gone.png").display())),
        (list(&one(&format!("{{ file = \"{}\", for = \"1s\" }}", in_repo.display()))), ("{", 0), format!("picture, step 1 (seal-meok-96): {} is in the repository", in_repo.display())),
        (list(&m("nojson")), ("{", 0), "picture, step 1 (nojson): nojson.json: ".into()),
        (list(&m("size")), ("{", 0), "picture, step 1 (size): size.json: its size is 1920×930 — the board is 3840×1860 (made at 2160p)".into()),
        (list(&m("ms0")), ("{", 0), "picture, step 1 (ms0): ms0.json: no ms above 0".into()),
        (list(&m("past")), ("{", 0), "picture, step 1 (past): past.json: stroke 1 runs to 1200 ms, past the step's ms (1000)".into()),
        (list(&m("fpast")), ("{", 0), "picture, step 1 (fpast): fpast.json: fade 1 runs to 1100 ms, past the step's ms (1000)".into()),
        (list(&m("r0")), ("{", 0), "picture, step 1 (r0): r0.json: stroke 1: r = 0 — its half width and edge, above 0".into()),
        (list(&m("sms0")), ("{", 0), "picture, step 1 (sms0): sms0.json: stroke 1: ms = 0 — it's drawn over its ms".into()),
        (list(&m("nopath")), ("{", 0), "picture, step 1 (nopath): nopath.json: stroke 1: no path".into()),
        (list(&m("out")), ("{", 0), "picture, step 1 (out): out.json: stroke 1: its point (3900, 10) is outside the board (3840×1860)".into()),
        (list(&m("rect")), ("{", 0), "picture, step 1 (rect): rect.json: fade 1: its rectangle [3800, 0, 100, 10] is outside the board (3840×1860)".into()),
        (list(&m("noink")), ("{", 0), format!("picture, step 1 (noink): it has strokes and no ink layer — {} not there", d.join("noink.ink.png").display())),
        (list(&m("nowash")), ("{", 0), format!("picture, step 1 (nowash): it has fades and no wash layer — {} not there", d.join("nowash.wash.png").display())),
        // (a step's line is its own: the second step refused at the second's)
        (list("[[picture]]\nat = \"20s\"\nsteps = [\n  { file = \"a.png\", for = \"1s\" },\n  { file = \"b.png\", for = \"9\" },\n]\n"), ("{", 1), "picture, step 2 (b): for \"9\": a length".into()),
        (list(&format!("[[span]]\nfrom = \"30s\"\nto = \"40s\"\nout = true\n{}", at("35s"))), ("at = \"35s\"", 0), "picture: at 0:00:35, inside a span left out".into()),
        (list(&(at("30s") + &at("20s"))), ("at = \"20s\"", 0), "picture: out of the take's order — at 0:00:20, not after the picture at line {first} (0:00:30)".into()),
        (list(&(at("20s") + &at("20s"))), ("at = \"20s\"", 1), "picture: out of the take's order — at 0:00:20, not after the picture at line {first} (0:00:20)".into()),
        (list(&format!("[[caption]]\nat = \"19s\"\nfor = \"2s\"\ntext = \"Over it.\"\n{}", at("20s"))), ("at = \"20s\"", 0), "picture: at 0:00:20, while the caption at line {caption} stands — shown before the picture and again after".into()),
        (list(&format!("[[chapter]]\nat = \"19s\"\ntitle = \"Two\"\ncard = false\n{}", at("20s"))), ("at = \"20s\"", 0), "picture: at 0:00:20, while the chapter at line {chapter} stands in the band — one at a time".into()),
        (opening("at = \"end - 4s\"\npicture = \"a.png\"\n"), ("at = \"end", 0), "opening: at and picture — where it ends is the take (at, for, zoom) or a picture (picture, for)".into()),
        (opening("picture = \"none.png\"\n"), ("picture =", 0), format!("opening: picture: {} not there", d.join("none.png").display())),
        (opening(&format!("picture = \"{}\"\n", in_repo.display())), ("picture =", 0), format!("opening: picture: {} is in the repository", in_repo.display())),
    ];
    for (text, (start, nth), want) in cases {
        let (got, file) = read(&text, d);
        let last =
            |table: &str| line_of(&text, table, text.matches(table).count().saturating_sub(1)).to_string();
        let mut want = want;
        for (mark, table) in
            [("{first}", "[[picture]]"), ("{caption}", "[[caption]]"), ("{chapter}", "[[chapter]]")]
        {
            if want.contains(mark) {
                let at = if mark == "{first}" { line_of(&text, table, 0).to_string() } else { last(table) };
                want = want.replace(mark, &at);
            }
        }
        let want = format!("{file}:{}: {want}", line_of(&text, start, nth));
        let e = got.unwrap_err();
        assert!(e.starts_with(&want), "{want}\n  got: {e}");
    }
    // (each of those, written right, is read)
    for text in [
        list(PICTURE),
        list(&at("35s")),
        list(&(at("20s") + &at("30s"))),
        list(&format!("[[caption]]\nat = \"18s\"\nfor = \"2s\"\ntext = \"Before it.\"\n{}", at("20s"))),
        opening("picture = \"a.png\"\n"),
    ] {
        let (got, _) = read(&text.replace("b.png", "a.png"), d);
        assert!(got.is_ok(), "{text}\n{:?}", got.err());
    }
}

// ── The files, at 2160p ──

/// `px` (RGBA, `dims`) as a PNG at `path`.
fn save_rgba(path: &Path, dims: (usize, usize), px: &[u8]) {
    let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut e = png::Encoder::new(f, dims.0 as u32, dims.1 as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.set_compression(png::Compression::Fast);
    e.write_header().unwrap().write_image_data(px).unwrap();
}

/// `px` (RGB, `dims`) as a PNG at `path`.
fn save_rgb(path: &Path, dims: (usize, usize), px: &[u8]) {
    let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut e = png::Encoder::new(f, dims.0 as u32, dims.1 as u32);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.set_compression(png::Compression::Fast);
    e.write_header().unwrap().write_image_data(px).unwrap();
}

const GROUND: Rgb = [7, 8, 11];

/// A step's layers at 2160p as its manifest (`manifest(1000)`'s) draws them: its stroke's ink within `r` − 2 of the
/// path, its fade's wash in its rectangle; `more_ink` and `more_wash` blocks (x, y, side) where nothing reaches.
fn layers(
    more_ink: Option<(usize, usize, usize)>,
    more_wash: Option<(usize, usize, usize)>,
) -> (Vec<u8>, Vec<u8>) {
    let (w, _) = BOARD;
    let mut ink = vec![0u8; BOARD.0 * BOARD.1 * 4];
    let mut wash = ink.clone();
    let fill = |l: &mut Vec<u8>, (x0, x1, y0, y1): (usize, usize, usize, usize), c: Rgb, a: u8| {
        for y in y0..y1 {
            for x in x0..x1 {
                l[(y * w + x) * 4..][..4].copy_from_slice(&[c[0], c[1], c[2], a]);
            }
        }
    };
    fill(&mut ink, (200, 1200, 172, 228), INK, 255);
    fill(&mut wash, (100, 700, 400, 600), WASH, 160);
    if let Some((x, y, s)) = more_ink {
        fill(&mut ink, (x, x + s, y, y + s), INK, 255);
    }
    if let Some((x, y, s)) = more_wash {
        fill(&mut wash, (x, x + s, y, y + s), WASH, 160);
    }
    (ink, wash)
}

/// A step `name` in `dir` at 2160p: its layers, its whole made of them over `before`, its manifest. Its whole.
fn files(dir: &Path, name: &str, before: &[u8], (ink, wash): &(Vec<u8>, Vec<u8>)) -> Vec<u8> {
    let whole = made(before, Some(wash), Some(ink));
    save_rgb(&dir.join(format!("{name}.png")), BOARD, &whole);
    save_rgba(&dir.join(format!("{name}.ink.png")), BOARD, ink);
    save_rgba(&dir.join(format!("{name}.wash.png")), BOARD, wash);
    std::fs::write(dir.join(format!("{name}.json")), manifest(1_000).to_string()).unwrap();
    whole
}

/// Every refusal of a picture's files, checked at 2160p before a frame is drawn, each at its step's line: the whole's
/// size and corner, a layer's kind, size and corner, the layers not making the picture, what came at the end; the
/// opening's picture's corner. The files written right pass, and their boards at 1080p are the wholes halved — the
/// next step's read on the worker.
#[test]
fn every_refusal_of_a_pictures_files_names_its_line_and_right_ones_draw() {
    let dir = Scratch::new("xtask-picture-files");
    let d = dir.path();
    let ground = GROUND.repeat(BOARD.0 * BOARD.1);
    let base = layers(None, None);
    let w1 = files(d, "w1", &ground, &base);
    let w2 = files(d, "w2", &w1, &layers(Some((1500, 900, 20)), None));
    let text = list(
        "[[picture]]\nat = \"20s\"\nsteps = [\n  { file = \"w1.png\", for = \"1s\" },\n  { file = \"w2.png\", for = \"1s\" },\n]\n",
    );
    let (c, file) = read(&text, d);
    let c = c.unwrap();
    let found = check(&c, GROUND).unwrap();
    assert_eq!((found[0][0].missed, found[0][0].ink.0, found[0][0].wash.0), (0, 0, 0));
    assert_eq!(found[0][1].ink, (400, 56_400), "w2's block no stroke reaches: counted, under 1 %");
    // (the boards at 1080p: before its first stroke the ground, settled the whole halved; the next read on the worker)
    let look = video::look(Bg::Dark);
    let mut boards = Boards::new(&c, Scale::HD, &video::Look { ground: GROUND, ..look }).unwrap();
    let half = |px: &[u8]| video::half(px, BOARD).2;
    let shows = |k: usize, t: Option<u64>| Shows { n: Some(0), k, t, line: None };
    assert_eq!(boards.board(&shows(0, Some(0))), half(&ground));
    assert_eq!(boards.board(&shows(0, None)), half(&w1));
    assert!(boards.next.is_some(), "the next step read on a worker while this one draws");
    assert_eq!(
        boards.board(&shows(1, Some(0))),
        half(&w1),
        "the next step's board before it: this one's whole"
    );
    assert_eq!(boards.board(&shows(1, None)), half(&w2));
    let drawn = boards.board(&shows(0, Some(200_000)));
    assert!(drawn != half(&ground) && drawn != half(&w1), "half drawn: neither");
    assert_eq!((boards.read, boards.failed.as_deref()), (3, None));

    // (each case: one file of w1's written another way, the list refused at w1's line)
    let line = line_of(&text, "{", 0);
    let corner = {
        let mut px = w1.clone();
        px[..3].copy_from_slice(&[244, 239, 230]);
        px
    };
    let mut missing = w1.clone();
    for y in 1000..1100 {
        missing[(y * BOARD.0 + 2000) * 3..(y * BOARD.0 + 2100) * 3].fill(200);
    }
    let mut cloudy = base.0.clone();
    cloudy[3] = 255;
    type Write = Box<dyn Fn(&Path)>;
    let cases: Vec<(Write, String)> = vec![
        (Box::new(|d: &Path| save_rgb(&d.join("w1.png"), (100, 50), &[0; 15_000])), "w1.png: it is 100×50, not 3840×1860".into()),
        (Box::new(move |d: &Path| save_rgb(&d.join("w1.png"), BOARD, &corner)), "w1.png: its corner is #f4efe6 — not this cut's ground, meok's #07080b".into()),
        (Box::new(|d: &Path| save_rgb(&d.join("w1.ink.png"), BOARD, &vec![0; BOARD.0 * BOARD.1 * 3])), "w1.ink.png: not 8-bit RGBA".into()),
        (Box::new(|d: &Path| save_rgba(&d.join("w1.wash.png"), (100, 50), &[0; 20_000])), "w1.wash.png: it is 100×50, not 3840×1860".into()),
        (Box::new(move |d: &Path| save_rgba(&d.join("w1.ink.png"), BOARD, &cloudy)), "w1.ink.png: its corner isn't clear (alpha 255)".into()),
        (Box::new(move |d: &Path| save_rgb(&d.join("w1.png"), BOARD, &missing)), format!("its layers miss w1.png on 10000 px (more than 2/255 off), the first at (2000, 1000) — {MISS_MOST} at the most")),
        (Box::new(|d: &Path| _ = files(d, "w1", &GROUND.repeat(BOARD.0 * BOARD.1), &layers(Some((2000, 1000, 300)), None))), "what came at the end: 90000 of its 146000 ink px above alpha 8 (61.6 %) — no stroke within its r; 1 % at the most".into()),
        (Box::new(|d: &Path| _ = files(d, "w1", &GROUND.repeat(BOARD.0 * BOARD.1), &layers(None, Some((2000, 1000, 100))))), "what came at the end: 10000 of its 130000 wash px above alpha 8 (7.7 %) — in no fade's rectangle".into()),
    ];
    for (write, want) in cases {
        let case = Scratch::new("xtask-picture-file");
        for n in [
            "w1.png",
            "w1.ink.png",
            "w1.wash.png",
            "w1.json",
            "w2.png",
            "w2.ink.png",
            "w2.wash.png",
            "w2.json",
        ] {
            std::fs::copy(d.join(n), case.path().join(n)).unwrap();
        }
        write(case.path());
        let (c, file) = read(&text, case.path());
        let e = check(&c.unwrap(), GROUND).unwrap_err();
        let want = format!("{file}:{line}: picture, step 1 (w1): {want}");
        assert!(e.starts_with(&want), "{want}\n  got: {e}");
    }
    // (the opening's picture: its corner the cut's ground)
    save_rgb(&d.join("hanji.png"), BOARD, &[244, 239, 230].repeat(BOARD.0 * BOARD.1));
    let text = list("").replace("at = \"end - 4s\"\n", "picture = \"hanji.png\"\n");
    let (c, _) = read(&text, d);
    let e = check(&c.unwrap(), GROUND).unwrap_err();
    let want = format!(
        "{file}:{}: opening: picture: hanji.png: its corner is #f4efe6",
        line_of(&text, "picture =", 0)
    );
    assert!(e.starts_with(&want), "{want}\n  got: {e}");
    assert_eq!(w2.len(), BOARD.0 * BOARD.1 * 3);
}
