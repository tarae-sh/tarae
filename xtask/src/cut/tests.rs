//! `cut`'s tests: made-up takes (landmarks written by hand, screens drawn by their own bytes) — no tarae, no claude,
//! nothing that waits.

use serde_json::json;

use super::draw::{QUIET, jump_ink, label, lost_zooms, wide_window};
use super::init::{Facts, rules, stills, ticking, written};
use super::sound::{
    BED_REF_LUFS, Laid, Made, Mix, NEAR_KEY_DB, Placed, Track, Where, bed_gains, heard_at, laid, lay, layout,
    made_said, mixed, realtime, sides, sounds, track_len, under,
};
use super::*;
use crate::camera::Region;
use crate::record::{Rec, Replay, SCREEN, SCRIPT, Take};
use crate::sound::{Heard, RATE};

/// A made-up take's landmarks, two minutes long: the front `main` at 1 s; a turn 10–40 s with a run card shown at 12 s
/// and answered at 15 s; `fix` in front at 50 s; a bring card 55–56 s.
fn marks() -> Marks {
    let at = |ms: u64, mut v: Value| {
        v["ms"] = json!(ms);
        v
    };
    Marks::new(
        vec![
            at(50_000, json!({ "what": "front", "to": "w1", "name": "fix" })),
            at(1_000, json!({ "what": "front", "to": "w0", "name": "main" })),
            at(10_000, json!({ "what": "turn-started", "session": "s0" })),
            at(12_000, json!({ "what": "card-shown", "id": "card#1", "kind": "run" })),
            at(15_000, json!({ "what": "card-answered", "id": "card#1", "kind": "run" })),
            at(40_000, json!({ "what": "turn-ended", "session": "s0" })),
            at(55_000, json!({ "what": "card-shown", "id": "card#2", "kind": "rebased" })),
            at(56_000, json!({ "what": "card-answered", "id": "card#2", "kind": "rebased" })),
        ],
        120_000,
    )
}

/// `text` against the made-up take; one with no `[opening]` gets `OPENING` at its end (its lines as they were).
fn cut(text: &str) -> Result<Cut> {
    match text.contains("[opening]") {
        true => Cut::parse(text, "cut.toml", &marks()),
        false => Cut::parse(&format!("{text}{OPENING}"), "cut.toml", &marks()),
    }
}

const OPENING: &str =
    "\n[opening]\nname = \"halil\"\nline = \"A task manager.\"\nat = \"end - 4s\"\nfor = \"3s\"\n";

/// The made-up cut: the opening's card and where it ends, 3 s from 55 s; 0–10 s at the default 4×, 10–20 s at 1×,
/// 20–50 s left out, 50–56 s fit in 3 s, the rest 4×; chapters at the start and at `fix`; a caption; a zoom.
const CUT: &str = r#"
default = 4

[opening]
name = "halil"
line = "A task manager for the terminal."
code = ['halil add "pay rent friday 9am +home !high"', "halil board"]
at = "card 2 shown"
for = "3s"

[[chapter]]
at = "start"
title = "One"

[[chapter]]
at = "front fix"
title = "Two"

[[span]]
from = "turn 1 start"
to = "20s"
speed = 1

[[span]]
from = "20s"
to = "50s"
out = true

[[span]]
from = "front fix"
to = "card 2 answered"
fit = "3s"

[[caption]]
at = "card 1 shown"
for = "2s"
text = "A card."

[[zoom]]
at = "card 1 shown"
for = "1s"
on = "card:run"
"#;

#[test]
fn the_time_map_is_exact_at_every_edge_and_never_goes_backward() {
    let c = cut(CUT).unwrap();
    let map = &c.map;
    let paces: Vec<(u64, u64, Pace)> = map.segs().iter().map(|s| (s.from, s.to, s.pace)).collect();
    assert_eq!(
        paces,
        [
            (0, 10_000, Pace::Speed(4.0)),
            (10_000, 20_000, Pace::Speed(1.0)),
            (20_000, 50_000, Pace::Out),
            (50_000, 56_000, Pace::Speed(2.0)),
            (56_000, 120_000, Pace::Speed(4.0)),
        ]
    );
    // (each edge both ways: a stretch's start in the video is where the one before ended)
    for (i, s) in map.segs().iter().enumerate() {
        assert_eq!(map.video(s.from), map.seg_at(i), "{s:?}");
        assert_eq!(map.video(s.to), map.seg_at(i) + map.seg_len(i), "{s:?}");
        if s.pace != Pace::Out {
            assert_eq!(map.take(map.seg_at(i)), (s.from, i), "{s:?}");
        }
    }
    assert_eq!(map.seg_at(3), 2_500_000 + 10_000_000, "10 s at 4× and 10 s at 1×");
    // (left out: no time in the video, the moment it sits at shows what comes after it)
    assert_eq!(map.seg_len(2), 0);
    assert_eq!(map.video(20_000), map.video(50_000));
    assert_eq!(map.video(35_000), map.video(50_000));
    assert_eq!(map.take(map.video(20_000)).0, 50_000);
    // (fit: its span in exactly that long)
    assert_eq!(map.seg_len(3), 3_000_000);
    assert_eq!(map.len(), 2_500_000 + 10_000_000 + 3_000_000 + 16_000_000);
    // (never backward, either way; and a shown moment comes back to itself within a frame of the take)
    let mut was = 0;
    for t in (0..=120_000).step_by(7) {
        let v = map.video(t);
        assert!(v >= was, "video({t}) = {v} < {was}");
        was = v;
        if !map.left_out(t) && !(20_000..50_000).contains(&t) {
            let back = map.take(v).0;
            assert!(back <= t && t - back <= 4, "take(video({t})) = {back}");
        }
    }
    let mut was = 0;
    for v in (0..=map.len() + 10_000).step_by(997) {
        let t = map.take(v).0;
        assert!(t >= was, "take({v}) = {t} < {was}");
        was = t;
    }
    assert_eq!(map.take(map.len() + 1).0, 120_000, "past the end: its end");
    // (the take's tail left out: the video ends where it begins, its last screen the last shown)
    let tail = cut("default = \"out\"\n[[span]]\nfrom = \"10s\"\nto = \"20s\"\nspeed = 2\n").unwrap();
    assert_eq!((tail.map.len(), tail.map.take(tail.map.len()).0), (5_000_000, 20_000));
    let p = Program::new(&tail);
    let last = p.parts.iter().find(|x| x.what == What::Last).unwrap();
    assert_eq!((p.frame(last.at).clock, p.frame(last.at).jump), (Some(20_000), None));
}

#[test]
fn a_time_resolves_by_the_takes_landmarks_and_one_it_lacks_is_refused_by_name() {
    let m = marks();
    for (said, ms) in [
        ("start", 0),
        ("end - 500ms", 119_500),
        ("1:02.5", 62_500),
        ("0:01:02", 62_000),
        ("72.5s", 72_500),
        ("1m 30s", 90_000),
        ("turn 1 start", 10_000),
        ("turn last end", 40_000),
        ("card 1 answered + 1s", 16_000),
        ("card 2 shown - 2.5s", 52_500),
        ("run card 1 shown", 12_000),
        ("rebased card 1 answered", 56_000),
        ("front fix", 50_000),
        ("front 2", 50_000),
        ("front main + 1s", 2_000),
    ] {
        assert_eq!(m.time(said), Ok(ms), "{said}");
    }
    for (said, why) in [
        ("turn 2 start", "the take has 1 turn"),
        ("rebased card 2 shown", "the take has 1 rebased card"),
        ("review 1", "the take has 0 reviews"),
        ("front nowhere", "no workspace nowhere"),
        ("run 1 end", "the take has 0 runs"),
        ("2:01", "past the take's end"),
        ("start - 1s", "before the take's start"),
        ("turn 0 start", "a count from 1"),
        ("after lunch", "not a time"),
        ("12", "a length"),
    ] {
        let e = m.time(said).unwrap_err();
        assert!(e.contains(&format!("\"{said}\"")) && e.contains(why), "{said}: {e}");
    }
    // (in a file: its line)
    let e = cut("default = 4\n\n[[span]]\nfrom = \"turn 1 start\"\nto = \"turn 2 end\"\nspeed = 1\n")
        .unwrap_err();
    assert!(e.starts_with("cut.toml:5: ") && e.contains("\"turn 2 end\": the take has 1 turn"), "{e}");
}

#[test]
fn a_file_out_of_the_takes_order_is_refused_with_its_line() {
    let refused = [
        (
            "default = 4\n[[span]]\nfrom = \"30s\"\nto = \"40s\"\nspeed = 1\n[[span]]\nfrom = \"10s\"\nto = \"20s\"\nspeed = 2\n",
            "cut.toml:7: span: out of the take's order",
        ),
        (
            "default = 4\n[[span]]\nfrom = \"10s\"\nto = \"40s\"\nspeed = 1\n[[span]]\nfrom = \"30s\"\nto = \"50s\"\nspeed = 2\n",
            "cut.toml:7: span: it starts (0:00:30) inside the span at line 2",
        ),
        (
            "default = 4\n[[chapter]]\nat = \"front fix\"\ntitle = \"B\"\n[[chapter]]\nat = \"start\"\ntitle = \"A\"\n",
            "cut.toml:6: chapter: out of the take's order",
        ),
        (
            "default = 4\n[[caption]]\nat = \"30s\"\nfor = \"1s\"\ntext = \"b\"\n[[caption]]\nat = \"10s\"\nfor = \"1s\"\ntext = \"a\"\n",
            "cut.toml:7: caption: out of the take's order",
        ),
        (
            "default = 4\n[[zoom]]\nat = \"30s\"\nfor = \"1s\"\non = \"all\"\n[[zoom]]\nat = \"10s\"\nfor = \"1s\"\non = \"all\"\n",
            "cut.toml:7: zoom: out of the take's order",
        ),
        // (the 11-10 rule: one caption at a time)
        (
            "default = 1\n[[caption]]\nat = \"10s\"\nfor = \"3s\"\ntext = \"a\"\n[[caption]]\nat = \"12s\"\nfor = \"1s\"\ntext = \"b\"\n",
            "cut.toml:7: caption: it shows while the one at line 2 still stands",
        ),
        (
            "default = 4\n[[span]]\nfrom = \"10s\"\nto = \"20s\"\nout = true\n[[caption]]\nat = \"15s\"\nfor = \"1s\"\ntext = \"a\"\n",
            "cut.toml:7: caption: at 0:00:15, inside a span left out",
        ),
        (
            "default = 4\n[[span]]\nfrom = \"10s\"\nto = \"20s\"\nfit = \"30s\"\n",
            "cut.toml:5: span: fit in 30.0 s what lasted 10.0 s",
        ),
        (
            "default = 4\n[[span]]\nfrom = \"10s\"\nto = \"20s\"\nspeed = 2\nout = true\n",
            "cut.toml:2: span: one of",
        ),
        ("default = 4\n[[span]]\nfrom = \"10s\"\nto = \"20s\"\nsped = 2\n", "cut.toml:5: span: no key sped"),
        ("bg = \"dark\"\n", "cut.toml:1: no default"),
        // (the title and the hook are gone: the opening says what the video is)
        ("title = \"x\"\ndefault = 4\n", "cut.toml:1: the file: no key title"),
        ("default = 4\n[hook]\nat = \"start\"\nfor = \"1s\"\n", "cut.toml:1: the file: no key hook"),
    ];
    for (text, want) in refused {
        let e = cut(text).unwrap_err();
        assert!(e.starts_with(want), "{want}\n  got: {e}");
    }
}

/// The frame `ms` after the take part's moment for the take's `t`.
fn at_take(p: &Program, t: u64, ms: u64) -> Frame {
    p.frame(p.video_of(p.cut.map.video(t)).unwrap() + ms * 1000)
}

/// A jump's mark is as loud as the jump is far, at each weight's edges: 9.9 s left out none (a cut), 10 s and 59.9 s the
/// clock's quiet ink (saying seconds, never minutes), a minute the accent; an unmarked one never cuts a mark short; the
/// clock steps at every jump, marked or not (a 5 s one too); `cut.txt` counts them by weight.
#[test]
fn a_jumps_mark_is_as_loud_as_the_jump_is_far() {
    assert_eq!(
        [weight(9_900), weight(10_000), weight(59_900), weight(60_000)],
        [Weight::Unmarked, Weight::Quiet, Weight::Quiet, Weight::Accent]
    );
    assert_eq!(
        (jumped(10_000), jumped(59_900), jumped(60_000)),
        ("+10 s".into(), "+59 s".into(), "+1 min".into())
    );
    let text = r#"
default = 1

[[span]]
from = "0:00:10.000"
to = "0:00:19.900"
out = true

[[span]]
from = "0:00:30.000"
to = "0:00:40.000"
out = true

[[span]]
from = "0:00:40.500"
to = "0:00:45.000"
out = true

[[span]]
from = "0:00:50.000"
to = "0:01:49.900"
out = true

[[span]]
from = "0:02:00.000"
to = "0:03:00.000"
out = true

[[span]]
from = "0:03:30.000"
to = "0:03:35.000"
out = true
"#;
    let c = Cut::parse(&format!("{text}{OPENING}"), "cut.toml", &Marks::new(Vec::new(), 240_000)).unwrap();
    let p = Program::new(&c);
    let standing = |t: u64| at_take(&p, t, FADE_MS + JUMP_MS / 2).jump;
    assert_eq!(standing(19_900), None, "9.9 s: a cut, no mark");
    assert_eq!(standing(40_000), Some((10_000, 255)), "10 s: marked, through the 4.5 s cut 0.5 s after it");
    assert_eq!(standing(109_900), Some((59_900, 255)), "59.9 s: marked");
    assert_eq!(standing(180_000), Some((60_000, 255)), "a minute: marked");
    // (the clock steps at every jump, whatever its mark: the take's moment before it, then the one after it, a
    // millisecond of video apart; a mark beside it only from 10 s)
    let jumps = [
        (10_000, 19_900, false),
        (30_000, 40_000, true),
        (50_000, 109_900, true),
        (120_000, 180_000, true),
        (210_000, 215_000, false),
    ];
    for (from, to, marked) in jumps {
        let v = p.video_of(c.map.video(to)).unwrap();
        let (before, at) = (p.frame(v - 1_000), p.frame(v));
        assert_eq!((before.clock, at.clock), (Some(from - 1), Some(to)), "the clock steps at {from}–{to}");
        // (0.3 s on: a mark up by then, and no jump since — the 10 s one's next cut comes at 0.5 s)
        let up = p.frame(v + 300_000);
        assert_eq!(up.clock, Some(to + 300), "and goes on from there");
        assert_eq!(up.jump.is_some(), marked, "{} s left out: marked {marked}", (to - from) / 1000);
    }
    // (how each is drawn: the clock's quiet ink under a minute, the accent from it; a fade scales both)
    let look = video::look(Bg::Dark);
    assert_eq!(jump_ink(10_000, 255, &look), (look.ink, QUIET));
    assert_eq!(jump_ink(59_900, 255, &look), (look.ink, QUIET));
    assert_eq!(jump_ink(60_000, 255, &look), (look.accent, 255));
    assert_eq!(jump_ink(59_900, 0, &look), (look.ink, 0));
    assert_eq!(
        p.jumps_said(),
        "jumps: 6 — 3 unmarked (under 10 s), 2 quiet (10 s to 1 min), 1 accent (1 min or more)"
    );
}

#[test]
fn the_band_says_the_clock_the_speed_and_the_jump_where_each_must_and_nowhere_else() {
    assert_eq!(clock(37 * 60_000 + 12_000), "0:37:12");
    assert_eq!(clock(3_723_400), "1:02:03");
    assert_eq!(
        (jumped(30_000), jumped(6 * 60_000 + 10_000), jumped(72 * 60_000)),
        ("+30 s".to_string(), "+6 min".to_string(), "+1 h 12 min".to_string())
    );
    assert_eq!((sped(16.0), sped(2.5), sped(1.0)), (Some("▸▸ ×16".into()), Some("▸▸ ×2.5".into()), None));
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    // (the intro and the card of what's built: no clock; then where it ends, the take from 55 s at 1×, marked)
    let intro = p.frame(1_000_000);
    assert_eq!((intro.card, intro.clock, intro.ends), (Some(Card::Intro(1_000)), None, false));
    let built = p.frame(4_000_000);
    assert_eq!((built.card, built.clock, built.speed), (Some(Card::Built), None, None));
    let ends = p.frame(7_000_000);
    assert_eq!((ends.clock, ends.speed, ends.ends, ends.jump), (Some(56_000), None, true, None));
    // (a sped stretch: its factor; at 1× none; the clock always the take's)
    let fast = at_take(&p, 5_000, 0);
    assert_eq!((fast.clock, fast.speed, fast.ends), (Some(5_000), Some(4.0), false));
    let one = at_take(&p, 15_000, 0);
    assert_eq!((one.clock, one.speed, one.jump), (Some(15_000), None, None));
    // (the jump: 30 s left out, said from the moment after it for a second, then gone; never before it)
    assert_eq!(at_take(&p, 19_900, 0).jump, None);
    let jump = at_take(&p, 50_000, 0);
    assert_eq!(
        (jump.clock, jump.speed, jump.jump),
        (Some(50_000), Some(2.0), None),
        "coming in from nothing"
    );
    assert_eq!(at_take(&p, 50_000, 50).jump.map(|j| j.0), Some(30_000));
    assert_eq!(at_take(&p, 50_000, FADE_MS + JUMP_MS / 2).jump, Some((30_000, 255)), "standing");
    assert_eq!(at_take(&p, 50_000, JUMP_MS + 2 * FADE_MS + 40).jump, None, "gone");
    // (a chapter's card: no clock, no marks; the last screen held: its clock, no speed)
    let second = p.parts.iter().find(|x| x.what == What::Chapter(1)).unwrap();
    let card = p.frame(second.at + 500_000);
    assert_eq!((card.card, card.clock, card.speed, card.jump), (Some(Card::Chapter(1)), None, None, None));
    let last = p.parts.iter().find(|x| x.what == What::Last).unwrap();
    let held = p.frame(last.at + 100_000);
    assert_eq!((held.clock, held.speed), (Some(120_000), None));
    // (the caption: in, standing, gone)
    assert_eq!(at_take(&p, 12_000, FADE_MS / 2).caption.map(|c| c.0), Some(0));
    assert_eq!(at_take(&p, 12_000, 1_000).caption, Some((0, 255)));
    assert_eq!(at_take(&p, 12_000, 2_000).caption, None);
    // (the sheet's words: the video's time, the take's, the speed)
    let v = p.video_of(c.map.video(5_000)).unwrap();
    let said = label(&p, v, &fast, false, &["now ×4".into()]);
    assert_eq!(said, format!("{}  take 0:00:05  ×4  now ×4", video_time(v)));
    assert!(label(&p, 7_000_000, &ends, false, &[]).contains("take 0:00:56  1× where it ends"));
}

#[test]
fn the_parts_cross_and_chapters_txt_says_each_card_s_time() {
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let whats: Vec<What> = p.parts.iter().map(|x| x.what).collect();
    assert_eq!(
        whats,
        [
            What::Intro,
            What::Built,
            What::Ends { from: 55_000 },
            What::Chapter(0),
            What::Take { from: 0 },
            What::Chapter(1),
            What::Take { from: 12_500_000 },
            What::Last,
            What::End
        ]
    );
    // (the opening 9 s, chapter 1's card 2 s; the take's 12.5 s to `fix`, its card; the rest 19 s)
    assert_eq!(p.parts[5].at, 9_000_000 + 2_000_000 + 12_500_000);
    let (txt, said) = p.chapters();
    assert_eq!(txt, "0:00 halil\n0:09 One\n0:23 Two\n");
    assert!(said.iter().any(|s| s.contains("“halil” runs 9.0 s")), "{said:?}");
    // (each part's start crosses, and the jump at 20 s does — where `fix`'s card stands, one crossing)
    let crossings = p.crossings();
    for x in &p.parts[1..] {
        assert!(crossings.iter().any(|c| c.0 == x.at), "{x:?} in {crossings:?}");
    }
}

/// A change of part goes through the ground — the part going out fades to it, then the next comes up, in the crossing's
/// time —: no frame of it has both parts' pixels. A crossing in the take (a jump, a screen mostly changed) stays the two
/// screens over each other.
#[test]
fn a_change_of_part_goes_through_the_ground_never_one_part_over_the_other() {
    use super::draw::{cross_px, cross_share};
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let crossings = p.crossings();
    for x in &p.parts[1..] {
        assert!(crossings.contains(&(x.at, true)), "{x:?} through the ground: {crossings:?}");
    }
    let (from, to, ground) = ([200u8, 0, 0].repeat(4), [0u8, 200, 0].repeat(4), [10u8, 10, 10]);
    let step = 1_000_000 / video::FPS;
    for &(at, through) in &crossings {
        let (mut v, mut seen) = (at, (0, 0));
        while v < at + camera::CROSS_MS * 1000 {
            let mut px = to.clone();
            cross_px(&from, &mut px, ground, cross_share(v, at), through);
            let (out, up) = (px[0] > ground[0], px[1] > ground[1]);
            assert!(!(out && up), "{} into the crossing at {}: both parts, {:?}", v - at, at, &px[..3]);
            seen = (seen.0 + usize::from(out), seen.1 + usize::from(up));
            v += step;
        }
        assert!(seen.0 > 0 && seen.1 > 0, "the crossing at {at}: one, then the other ({seen:?})");
    }
    // (in the take: halfway, the two over each other)
    let mut px = to.clone();
    cross_px(&from, &mut px, ground, 128, false);
    assert!(px[0] > 10 && px[1] > 10, "{:?}", &px[..3]);
}

/// The sheet's pictures are taken once their part has come up — never inside a crossing, where the label would name a
/// part the picture doesn't show yet (V1b's 0:06.0, the card under `where it ends`).
#[test]
fn the_sheet_takes_each_picture_once_its_part_has_come_up() {
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let picks = super::draw::sheet_picks(&p);
    let crossings = p.crossings();
    for (&k, marks) in &picks {
        let v = Program::time_of(k);
        assert!(
            !crossings.iter().any(|c| c.0 <= v && v < c.0 + camera::CROSS_MS * 1000),
            "{} {marks:?}: inside a crossing",
            video_time(v)
        );
    }
    let ends = p.parts.iter().find(|x| matches!(x.what, What::Ends { .. })).unwrap().at;
    assert_eq!(ends, 6_000_000, "a regular picture's time");
    assert!(!picks.contains_key(&Program::frame_at(ends)), "{:?}", picks.keys().collect::<Vec<_>>());
    assert!(picks.contains_key(&Program::frame_at(ends + camera::CROSS_MS * 1000)));
}

/// The video's first frame shows the swing, not the ground: its empty frames as made are left out (a file's first frame
/// is what a preview shows). Where main's swing frames aren't there, nothing to check.
#[test]
fn the_videos_first_frame_shows_the_swing() {
    let (bg, look) = (Bg::Dark, video::look(Bg::Dark));
    let fonts = crate::shot::Fonts::load(1).unwrap();
    let Ok(intro) = video::WideIntro::new(bg, &look, &fonts, video::Scale::HD) else { return };
    let px = intro.frame(0, &look);
    let inked = px.chunks(3).filter(|p| **p != look.ground).count();
    assert!(inked > 100, "the first frame: {inked} pixels off the ground ({} empty left out)", intro.skipped);
}

/// A mark's words resolve as a time, as the driver wrote them (`record mark WORDS`): whole, an offset after them, one
/// the take lacks or has twice refused in words; and so through the file.
#[test]
fn a_mark_s_words_resolve_as_a_time() {
    let mut list = marks().list;
    for (ms, said) in [
        (58_000, "the board, for the opening"),
        (70_000, "a friction - again"),
        (80_000, "twice"),
        (90_000, "twice"),
    ] {
        list.push(json!({ "what": "mark", "said": said, "ms": ms }));
    }
    let m = Marks::new(list, 120_000);
    assert_eq!(m.time("mark \"the board, for the opening\""), Ok(58_000));
    assert_eq!(m.time("mark \"the board, for the opening\" - 2s"), Ok(56_000));
    assert_eq!(m.time("mark \"a friction - again\""), Ok(70_000));
    assert_eq!(m.time("mark \"a friction - again\" + 500ms"), Ok(70_500));
    let e = m.time("mark \"the board\"").unwrap_err();
    assert!(e.contains("the take has no mark “the board”"), "{e}");
    let e = m.time("mark \"twice\"").unwrap_err();
    assert!(e.contains("the take has 2 marks “twice”"), "{e}");
    let file = CUT.replace("at = \"card 2 shown\"", "at = 'mark \"the board, for the opening\"'");
    assert_eq!(Cut::parse(&file, "cut.toml", &m).unwrap().opening.from, 58_000);
}

/// A mark resolves by its number too (`mark 3`, `mark last`), and a chapter the driver said by its words and its number,
/// apart from the marks: a mark's words aren't a chapter's.
#[test]
fn a_mark_resolves_by_its_number_and_a_chapter_by_its_words_and_number() {
    let mut list = marks().list;
    list.push(json!({ "what": "mark", "said": "a friction", "ms": 30_000 }));
    list.push(json!({ "what": "chapter", "said": "The empty folder", "ms": 5_000 }));
    list.push(json!({ "what": "mark", "said": "stuck", "ms": 45_000 }));
    list.push(json!({ "what": "chapter", "said": "Two at once", "ms": 60_000 }));
    list.push(json!({ "what": "mark", "said": "the board", "ms": 90_000 }));
    let m = Marks::new(list, 120_000);
    assert_eq!(m.time("mark 1"), Ok(30_000));
    assert_eq!(m.time("mark 3"), Ok(90_000));
    assert_eq!(m.time("mark last - 1s"), Ok(89_000));
    let e = m.time("mark 4").unwrap_err();
    assert!(e.contains("the take has 3 marks"), "{e}");
    assert_eq!(m.time("chapter 2"), Ok(60_000));
    assert_eq!(m.time("chapter \"The empty folder\" + 2s"), Ok(7_000));
    let e = m.time("chapter \"stuck\"").unwrap_err();
    assert!(e.contains("the take has no chapter “stuck”"), "{e}");
    let e = m.time("mark \"Two at once\"").unwrap_err();
    assert!(e.contains("the take has no mark “Two at once”"), "{e}");
}

/// `cut init`'s chapters: where the driver said them (`record chapter`), titled by their words, spans apart at each, and
/// the front's changes not one; the list reads back to them.
#[test]
fn cut_init_puts_its_chapters_where_the_driver_said_them() {
    let mut list = marks().list;
    list.push(json!({ "what": "chapter", "said": "The \"empty\" folder", "ms": 3_000 }));
    list.push(json!({ "what": "mark", "said": "a note", "ms": 20_000 }));
    list.push(json!({ "what": "chapter", "said": "The bring", "ms": 52_000 }));
    let m = Marks::new(list, 120_000);
    let facts = Facts::of(&m, &[], Vec::new());
    let at: Vec<(u64, &str, &str)> =
        facts.chapters.iter().map(|c| (c.0, c.1.as_str(), c.2.as_str())).collect();
    assert_eq!(at, [(3_000, "chapter 1", "The \"empty\" folder"), (52_000, "chapter 2", "The bring")]);
    let edges: Vec<u64> = rules(&facts).iter().map(|(s, _)| s.from).collect();
    assert!(edges.contains(&3_000) && edges.contains(&52_000) && !edges.contains(&50_000), "{edges:?}");
    let (text, said) = written(&facts, std::path::Path::new("take"));
    assert!(text.contains("at = \"chapter 2\"") && !text.contains("front 2"), "{text}");
    assert!(said.contains("2 chapters (the driver's)"), "{said}");
    let text = (text.replacen("name = \"\"", "name = \"halil\"", 1)).replacen(
        "line = \"\"",
        "line = \"A tool.\"",
        1,
    );
    let back = Cut::parse(&text, "init.toml", &m).unwrap();
    assert_eq!(
        back.chapters.iter().map(|c| (c.at, c.title.as_str())).collect::<Vec<_>>(),
        [(3_000, "The \"empty\" folder"), (52_000, "The bring")]
    );
}

/// `cargo xtask cut init` end to end on a made-up take with the driver's chapters and marks: it writes its list (the
/// opening's words empty, its read-back standing in for them); then `cargo xtask cut` on that list is refused only for
/// those words, each by its line and what to fill — the list's every time read back by the cut itself.
#[test]
fn cut_init_on_a_take_writes_a_list_the_cut_refuses_only_for_the_openings_words() {
    let s = tarae_testkit::scratch::Scratch::new("xtask-cut-init-e2e");
    let dir = s.join("take");
    std::fs::create_dir(&dir).unwrap();
    let about = json!({ "v": 1, "size": [80, 24], "ms": 120_000, "started_unix_ms": 0, "made_up": true });
    std::fs::write(dir.join("take.json"), about.to_string()).unwrap();
    let mut screen = Vec::new();
    for (ms, b) in [(0u64, "\x1b[H\x1b[2Jhalil"), (30_000, "\x1b[2;1Hcargo build"), (90_000, "\x1b[3;1Hdone")]
    {
        crate::record::put(&mut screen, ms * 1000, SCREEN, b.as_bytes()).unwrap();
    }
    std::fs::write(dir.join("screen.bin"), screen).unwrap();
    let mut list = marks().list;
    list.push(json!({ "what": "chapter", "said": "The empty folder", "ms": 2_000 }));
    list.push(json!({ "what": "mark", "said": "a friction", "ms": 20_000 }));
    list.push(json!({ "what": "chapter", "said": "The bring", "ms": 52_000 }));
    list.push(json!({ "what": "bring", "ms": 58_000 }));
    let lines: String = list.iter().map(|m| m.to_string() + "\n").collect();
    std::fs::write(dir.join("landmarks.jsonl"), lines).unwrap();
    let arg = |p: &std::path::Path| p.to_string_lossy().into_owned();

    run(&["init".into(), arg(&dir)]).unwrap();
    let init = dir.join("init.toml");
    let text = std::fs::read_to_string(&init).unwrap();
    assert!(text.contains("at = \"chapter 2\"") && text.contains("title = \"The bring\""), "{text}");
    let e = run(&["init".into(), arg(&dir)]).unwrap_err();
    assert!(e.contains("init.toml is there already"), "{e}");

    // (the cut on it: refused for the opening's name, then its line, each said with its line in the list)
    let line_of = |text: &str, key: &str| 1 + text.lines().position(|l| l.starts_with(key)).unwrap();
    let e = run(&[arg(&init)]).unwrap_err();
    let at = format!(
        "init.toml:{}: opening: its name, one line of 1 to {NAME_MAX} characters (0 here)",
        line_of(&text, "name = ")
    );
    assert!(e.ends_with(&at), "{e}");
    let named = text.replacen("name = \"\"", "name = \"halil\"", 1);
    std::fs::write(&init, &named).unwrap();
    let e = run(&[arg(&init)]).unwrap_err();
    let at = format!(
        "init.toml:{}: opening: its line, one line of 1 to {LINE_MAX} characters (0 here)",
        line_of(&named, "line = ")
    );
    assert!(e.ends_with(&at), "{e}");
    // (with them written, the list reads back whole — the cut's own read, before it draws)
    let filled = named.replacen("line = \"\"", "line = \"A tool.\"", 1);
    let marks = Marks::read(&dir, 120_000).unwrap();
    let back = Cut::parse(&filled, "init.toml", &marks).unwrap();
    assert_eq!(
        back.chapters.iter().map(|c| (c.at, c.title.as_str())).collect::<Vec<_>>(),
        [(2_000, "The empty folder"), (52_000, "The bring")]
    );
}

/// A take with no chapter said: by the front, counting only a workspace that stays ten seconds or more — the start
/// first, by the first such one's name, then each by another name: a switch away and back within ten seconds isn't one.
#[test]
fn cut_init_by_the_front_never_puts_two_chapters_under_ten_seconds_apart() {
    let front = |ms: u64, name: &str| json!({ "what": "front", "name": name, "ms": ms });
    let m = Marks::new(
        vec![
            front(1_000, "fix"),    // 3 s at the start: the start's title is the next one's
            front(4_000, "main"),   // the start
            front(30_000, "dates"), // 4 s: a look
            front(34_000, "main"),  // back: no chapter
            front(50_000, "dates"), // 5 s, then
            front(55_000, "board"), // stays: a chapter
            front(80_000, "main"),  // a chapter
            front(100_000, "main"), // the same name again: none
        ],
        120_000,
    );
    let facts = Facts::of(&m, &[], Vec::new());
    let at: Vec<(u64, &str, &str)> =
        facts.chapters.iter().map(|c| (c.0, c.1.as_str(), c.2.as_str())).collect();
    assert_eq!(at, [(0, "start", "main"), (55_000, "front 6", "board"), (80_000, "front 7", "main")]);
    let gaps = facts.chapters.windows(2).map(|w| w[1].0 - w[0].0);
    assert!(gaps.clone().all(|g| g >= 10_000), "{:?}", gaps.collect::<Vec<_>>());
    let (_, said) = written(&facts, std::path::Path::new("take"));
    assert!(said.contains("3 chapters (by the front: the driver said none)"), "{said}");
}

#[test]
fn the_opening_is_the_intro_the_card_then_where_it_ends_and_a_list_without_it_is_refused() {
    let c = cut(CUT).unwrap();
    let o = &c.opening;
    assert_eq!(
        (o.name.as_str(), o.line.as_str(), o.code.as_slice(), o.from, o.len),
        (
            "halil",
            "A task manager for the terminal.",
            &["halil add \"pay rent friday 9am +home !high\"".to_string(), "halil board".into()][..],
            55_000,
            3_000
        )
    );
    let p = Program::new(&c);
    let first: Vec<(What, u64, u64)> = p.parts[..4].iter().map(|x| (x.what, x.at, x.len)).collect();
    assert_eq!(
        first,
        [
            (What::Intro, 0, INTRO_MS * 1000),
            (What::Built, INTRO_MS * 1000, BUILT_MS * 1000),
            (What::Ends { from: 55_000 }, 6_000_000, 3_000_000),
            (What::Chapter(0), 9_000_000, CHAPTER_MS * 1000),
        ],
        "the intro, the card, where it ends, then the take"
    );
    // (the sheet says each part by its name)
    let sheet: Vec<String> = super::draw::sheet_marks(&p).into_iter().map(|m| m.1).collect();
    assert_eq!(&sheet[..3], ["intro", "what's built", "where it ends"]);
    // (a list with no card of what's built, or one missing a part of it: refused, naming what's missing)
    let bare = "default = 4\n[[chapter]]\nat = \"start\"\ntitle = \"One\"\n";
    let e = Cut::parse(bare, "cut.toml", &marks()).unwrap_err();
    assert!(e.starts_with("cut.toml:1: no [opening]: the card of what's built — name, line, code"), "{e}");
    let at = CUT.lines().position(|l| l == "[opening]").unwrap() + 1;
    for (gone, said) in [
        ("name = \"halil\"\n", "opening: no name ("),
        ("at = \"card 2 shown\"\nfor = \"3s\"\n", "opening: no at, for ("),
    ] {
        let e = cut(&CUT.replace(gone, "")).unwrap_err();
        assert!(e.starts_with(&format!("cut.toml:{at}: {said}")), "{e}");
    }
    for (was, now, said) in [
        ("name = \"halil\"", "name = \"\"", "opening: its name, one line of 1 to 24 characters (0 here)"),
        ("\"halil board\"]", "\"halil board\", \"halil done 1\"]", "opening: code = "),
        ("for = \"3s\"\n", "for = \"70s\"\n", "opening: where it ends runs past the take's end"),
        ("name = \"halil\"", "name = \"halil\"\ntitle = \"x\"", "opening: no key title"),
    ] {
        let e = cut(&CUT.replace(was, now)).unwrap_err();
        assert!(e.starts_with("cut.toml:") && e.contains(said), "{now}: {e}");
    }
}

#[test]
fn the_small_line_says_the_takes_length_and_the_videos_by_run_duration() {
    let d = Duration::from_millis;
    for (ms, said) in [
        (412, "412ms"),
        (999, "999ms"),
        (8_300, "8.3s"),
        (59_960, "1m 00s"),
        (7 * 60_000 + 12_000, "7m 12s"),
        (92 * 60_000, "1h 32m"),
        ((48 + 4) * 3_600_000, "2d 04h"),
    ] {
        assert_eq!(run_duration(d(ms)), said, "{ms} ms");
    }
    assert_eq!(
        told(87 * 60_000 + 12_000, 603_000_000),
        "A real session with Claude in tarae: 1h 27m, shown in 10m 03s"
    );
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    assert_eq!(
        told(worked(&c), p.total),
        format!(
            "A real session with Claude in tarae: 2m 00s, shown in {}",
            run_duration(Duration::from_micros(p.total))
        )
    );
    // (how long the work took: from the first chapter, 10 s in, to the last moment kept, 100 s — the tail left out)
    let later = CUT.replacen("at = \"start\"", "at = \"turn 1 start\"", 1)
        + "\n[[span]]\nfrom = \"100s\"\nto = \"end\"\nout = true\n";
    let c = cut(&later).unwrap();
    assert_eq!((c.chapters[0].at, worked(&c)), (10_000, 90_000));
}

/// By hand (the design session's look, before any commit): the card of what's built at 1080p on meok and on hanji, the
/// full video's words and made-up times (1h 27m of take, 10m 03s of video), and the intro's frame where the name has
/// risen (meok only: hanji has no swing), written to `$CUT_STILLS` (a folder made new).
#[test]
#[ignore = "by hand: writes the opening's stills where $CUT_STILLS says"]
fn by_hand_the_opening_s_stills() {
    let dir = std::path::PathBuf::from(std::env::var("CUT_STILLS").expect("CUT_STILLS: where the stills go"));
    std::fs::create_dir(&dir).unwrap();
    let c = cut(CUT).unwrap();
    let fonts = crate::shot::Fonts::load(1).unwrap();
    let said = told(87 * 60_000 + 12_000, 603_000_000);
    for bg in [Bg::Dark, Bg::Light] {
        let look = video::look(bg);
        let px = super::opening::card(&c.opening, &said, bg, &look, &fonts, video::Scale::HD);
        let p = dir.join(format!("built-{}.png", bg.theme()));
        crate::shot::save_png(&p, video::W, video::H, &px).unwrap();
        println!("{}", p.display());
        if let Ok(intro) = video::WideIntro::new(bg, &look, &fonts, video::Scale::HD) {
            for ms in [800, INTRO_MS - 100] {
                let p = dir.join(format!("intro-{}-{ms}.png", bg.theme()));
                crate::shot::save_png(&p, video::W, video::H, &intro.frame(ms, &look)).unwrap();
                println!("{} ({:?} as made, {:?} drawn)", p.display(), intro.made, intro.drawn());
            }
        }
    }
}

#[test]
fn where_it_ends_is_marked_and_zoomed_by_the_opening_alone() {
    // (no `from later` left anywhere the cut says a word)
    for (name, src) in [
        ("cut.rs", include_str!("../cut.rs")),
        ("draw.rs", include_str!("draw.rs")),
        ("sound.rs", include_str!("sound.rs")),
        ("init.rs", include_str!("init.rs")),
        ("opening.rs", include_str!("opening.rs")),
    ] {
        assert!(!src.contains(concat!("from ", "later")), "{name}");
    }
    assert_eq!(super::draw::ENDS, "where it ends");
    let size = (40, 10);
    let all = Region::all(size);
    let near = Region { x: 20.0, y: 5.0, w: 20.0, h: 5.0 };
    let close = |r: Region| (r.w - near.w).abs() < 1.0 && (r.x - near.x).abs() < 1.0;
    // (the opening's zoom: the camera there, on that stretch's own clock — eased in from its start, held to its end)
    let line = CUT.lines().position(|l| l == "for = \"3s\"").unwrap() + 2;
    let c = cut(&CUT.replace("for = \"3s\"\n", "for = \"3s\"\nzoom = \"card:rebased\"\n")).unwrap();
    assert_eq!(c.opening.zoom, Some(("card:rebased".to_string(), line)));
    let p = Program::new(&c);
    let ends = p.parts.iter().find(|x| matches!(x.what, What::Ends { .. })).unwrap();
    let paths = super::draw::paths(&c, &[all], Some((0, near)), size);
    let region = |v: u64| super::draw::region(&p.frame(v), &c, &paths, size);
    assert_eq!(region(ends.at), all, "at its start: the whole");
    let held = region(ends.at + (camera::DRIFT_MS + 300) * 1000);
    assert!(close(held), "eased in, held on its target: {held:?}");
    let last = region(ends.at + ends.len - 1_000_000 / video::FPS);
    assert!(close(last), "held to its end: {last:?}");
    assert_eq!(region(p.chapter_at(1) + CHAPTER_MS * 1000 + 100_000), all, "never in the take");
    let e = cut(&CUT.replace("for = \"3s\"\n", "for = \"3s\"\nzoom = \" \"\n")).unwrap_err();
    assert!(e.starts_with(&format!("cut.toml:{line}: opening: zoom = ")), "{e}");
    // (a [[zoom]] whose time falls where it ends: nothing there, the take's alone)
    let zoomed =
        CUT.to_string() + "\n[[zoom]]\nat = \"card 2 shown + 0.5s\"\nfor = \"1s\"\non = \"card:rebased\"\n";
    let c = cut(&zoomed).unwrap();
    let p = Program::new(&c);
    let paths = super::draw::paths(&c, &[all, near], None, size);
    let region = |v: u64| super::draw::region(&p.frame(v), &c, &paths, size);
    let ends = p.parts.iter().find(|x| matches!(x.what, What::Ends { .. })).unwrap();
    let step = 1_000_000 / video::FPS;
    for v in (ends.at..ends.at + ends.len).step_by(step as usize) {
        assert_eq!(region(v), all, "{} where it ends: the whole", video_time(v));
    }
    let m = p.video_of(c.map.video(55_500)).unwrap();
    let held = region(m + (camera::DRIFT_MS + 500) * 1000);
    assert!(close(held), "in the take: {held:?}");
    // (one in a span left out: refused, wherever it ends falls)
    let only = zoomed
        .replace("at = \"card 2 shown\"\nfor = \"3s\"", "at = \"25s\"\nfor = \"3s\"")
        .replace("card 2 shown + 0.5s", "25.5s");
    let e = cut(&only).unwrap_err();
    assert!(e.contains("zoom: at 0:00:25, inside a span left out"), "{e}");
}

#[test]
fn a_zoom_window_holds_its_target_in_the_screens_shape_never_nearer_than_its_most() {
    let size = (180, 36);
    let card = camera::Region { x: 100.0, y: 30.0, w: 50.0, h: 6.0 };
    let w = wide_window(card, size);
    assert!((w.w / w.h - 5.0).abs() < 1e-3, "the screen's shape: {w:?}");
    assert!(w.w >= 180.0 / super::draw::ZOOM_MAX - 1e-3);
    assert!(
        w.x <= card.x && card.x + card.w <= w.x + w.w && w.y <= card.y && card.y + card.h <= w.y + w.h,
        "{w:?}"
    );
    assert!(w.x + w.w <= 180.0 && w.y + w.h <= 36.0, "inside: {w:?}");
    let tiny = wide_window(camera::Region { x: 0.0, y: 0.0, w: 4.0, h: 1.0 }, size);
    assert_eq!((tiny.w, tiny.x, tiny.y), (90.0, 0.0, 0.0));
}

/// A screen record drawing `text` alone at `ms`.
#[test]
fn a_chapter_with_no_card_stands_in_the_band_and_a_caption_waits_for_it() {
    let plain = CUT.replace("title = \"One\"\n", "title = \"One\"\ncard = false\n");
    let c = cut(&plain).unwrap();
    let p = Program::new(&c);
    let whats: Vec<What> = p.parts.iter().map(|x| x.what).collect();
    assert_eq!(
        whats,
        [
            What::Intro,
            What::Built,
            What::Ends { from: 55_000 },
            What::Take { from: 0 },
            What::Chapter(1),
            What::Take { from: 12_500_000 },
            What::Last,
            What::End
        ],
        "no card for chapter 1: the opening, then the take"
    );
    // (its number and title in the band from the take's first frame, standing, then gone after 3 s)
    let at = p.chapter_at(0);
    assert_eq!(at, 9_000_000, "after the opening");
    assert_eq!(p.frame(at + 1_500_000).chapter, Some((0, 255)));
    assert_eq!(p.frame(at + FADE_MS * 500).chapter.map(|c| c.0), Some(0), "fading in");
    assert_eq!(p.frame(at + CHAPTER_BAND_MS * 1000 + 40_000).chapter, None, "gone");
    assert_eq!(p.frame(p.parts[4].at + 500_000).chapter, None, "a card is a card");
    assert_eq!(p.chapters().0, "0:00 halil\n0:09 One\n0:21 Two\n");
    // (a caption while it stands: refused, with its line)
    let crowded = plain.replace("at = \"card 1 shown\"\nfor = \"2s\"", "at = \"2s\"\nfor = \"2s\"");
    let e = cut(&crowded).unwrap_err();
    assert!(e.contains("stands in the band") && e.starts_with("cut.toml:"), "{e}");
    assert!(cut(&CUT.replace("title = \"One\"\n", "title = \"One\"\ncard = 0\n")).is_err());
}

#[test]
fn a_zoom_whose_target_leaves_the_screen_is_said_with_its_line_and_time() {
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let m = c.map.video(12_000) / 1000;
    let reads = [(m, m + camera::DRIFT_MS + 1_000, Region::all((40, 4)))];
    let take = |recs: Vec<Rec>| Take { about: Value::Null, size: (40, 4), screen: recs };
    let gone = take(vec![drawn(0, "nothing"), drawn(12_000, "wants to run"), drawn(13_000, "answered")]);
    let said = lost_zooms(&c, &gone, &p, &reads, &[12_000]);
    assert_eq!(said.len(), 1, "{said:?}");
    let line = CUT.lines().position(|l| l == "[[zoom]]").unwrap() + 1;
    let v = video_time(p.video_of(c.map.video(13_000)).unwrap());
    assert!(said[0].starts_with(&format!("cut.toml:{line}: zoom on \"card:run\": gone at {v}")), "{said:?}");
    assert!(said[0].contains("take 0:00:13.0"), "{said:?}");
    // (a target that stays: nothing said)
    let stays =
        take(vec![drawn(0, "nothing"), drawn(12_000, "wants to run"), drawn(13_000, "wants to run!")]);
    assert!(lost_zooms(&c, &stays, &p, &reads, &[12_000]).is_empty());
}

/// A key at a take's ms.
fn key(ms: u64, b: &[u8]) -> Rec {
    Rec { us: ms * 1000, whose: SCRIPT, bytes: b.to_vec() }
}

#[test]
fn a_key_in_a_real_time_span_sounds_at_its_video_time_and_one_in_a_fast_span_doesnt() {
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let keys = [
        key(5_000, b"a"),  // the default's 4×
        key(14_000, b"b"), // 1×
        key(14_500, b"\r"),
        key(14_600, b"\x1b[97;1:3u"), // a key let go: no stroke
        key(30_000, b"c"),            // left out
        key(31_000, b"\x1b[<0;3;4M"), // the mouse: no key
        key(52_000, b" "),            // fit in 3 s: 2×, heard
    ];
    let s = sounds(&p, &keys, &marks());
    // (the opening 9 s, chapter 1's card 2 s; the take's 10 s at 4× is 2.5 s, then 4 s at 1×)
    let ks: Vec<(u64, Heard)> = s
        .list
        .iter()
        .filter(|h| matches!(h.1, Heard::Key(_)))
        .map(|(t, h)| (t.as_micros() as u64, h.clone()))
        .collect();
    let fit = p.video_of(c.map.video(52_000)).unwrap();
    assert_eq!(ks, [(17_500_000, Heard::Key('k')), (18_000_000, Heard::Key('\r')), (fit, Heard::Key(' '))]);
    assert_eq!((s.keys, s.fast, s.out), (3, 1, 1));
    assert_eq!(heard_at(&p, 5_000), Where::Fast);
    assert_eq!(heard_at(&p, 30_000), Where::Out);
    // (where it ends: heard there, and again in the take where it's shown slowly enough — 55.5 s is the 2× fit's)
    assert_eq!(heard_at(&p, 55_500), Where::At(vec![6_500_000, p.video_of(c.map.video(55_500)).unwrap()]));
    assert_eq!(heard_at(&p, 57_000), Where::At(vec![8_000_000]), "there only: the take shows it at 4×");
}

#[test]
fn the_stamp_sounds_once_where_a_card_is_answered_its_key_its_own() {
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let s = sounds(&p, &[key(14_950, b"y"), key(14_000, b"x")], &marks());
    let at = |h: &Heard| -> Vec<u64> {
        s.list.iter().filter(|x| &x.1 == h).map(|x| x.0.as_micros() as u64).collect()
    };
    // (card 1 answered at 15 s, at 1×: 11 s + 2.5 s + 5 s; card 2 at 56 s: the take's 4× there, where it ends' 1× at 7 s)
    assert_eq!(at(&Heard::Stamp), [7_000_000, 18_500_000]);
    assert_eq!(
        at(&Heard::Key('k')),
        [17_500_000],
        "the y that answered is the stamp's; the x before it its own"
    );
    assert_eq!((s.stamps, s.stamps_missed), (2, 0));
    let end = p.parts.iter().find(|x| x.what == What::End).unwrap().at;
    assert_eq!((s.intro, s.brush), (Some(0), Some(end)), "the brush with the intro and on the end card");
    // (the intro's is the end card's as mastered, laid again)
    let mut track = vec![0.0f32; 100];
    track[80..90].copy_from_slice(&[0.5; 10]);
    super::sound::intro_brush(&mut track, 80, 3, 10);
    assert_eq!((&track[3..13], track[2], track[13]), (&[0.5f32; 10][..], 0.0, 0.0));
}

/// `CUT` with two beds: the first from the start, the second from chapter 2 looping between 1 s and 3 s.
fn bedded() -> String {
    CUT.to_string()
        + "\n[[music]]\nchapter = 1\nfile = \"a.wav\"\n\n[[music]]\nchapter = 2\nfile = \"b.wav\"\nloop = [\"1s\", \"0:03\"]\n"
}

fn sample_at(us: u64) -> usize {
    (us as f64 * f64::from(RATE) / 1e6).round() as usize
}

#[test]
fn beds_cross_at_equal_power_loop_at_their_marks_and_fade_where_they_run_out() {
    let c = cut(&bedded()).unwrap();
    assert_eq!(
        c.beds.iter().map(|b| (b.chapter, b.looped)).collect::<Vec<_>>(),
        [(0, None), (1, Some((1_000, 3_000)))]
    );
    for (bad, said) in [
        ("chapter = 2\nfile = \"b.wav\"", "chapter = 3\nfile = \"b.wav\""),
        ("chapter = 2\nfile = \"b.wav\"", "chapter = 1\nfile = \"b.wav\""),
        ("[\"1s\", \"0:03\"]", "[\"1s\", \"2.5s\"]"),
        ("[\"1s\", \"0:03\"]", "\"1s\""),
    ] {
        let e = cut(&bedded().replace(bad, said)).unwrap_err();
        assert!(e.starts_with("cut.toml:") && e.contains("music:"), "{said}: {e}");
    }
    let p = Program::new(&c);
    let len = sample_at(p.total);
    let secs = |s: f64| (s * f64::from(RATE)) as usize;
    let tone = |hz: f32, n: usize| -> Vec<f32> {
        (0..n).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / RATE as f32).sin()).collect()
    };
    // (laid out: the first in under the intro's last half second; crossed into the second over chapter 2's card)
    let card = p.parts.iter().find(|x| x.what == What::Chapter(1)).unwrap();
    let (placed, said) = layout(&p, &[(secs(60.0), 0), (secs(5.0), 0)]);
    assert!(said.is_empty(), "{said:?}");
    let intro = INTRO_MS * 1000;
    assert_eq!(
        (placed[0].start, placed[0].fade_in, placed[0].fade_out),
        (intro - 500_000, (intro - 500_000, intro), (card.at, card.at + card.len))
    );
    assert_eq!((placed[1].start, placed[1].fade_in), (card.at, (card.at, card.at + card.len)));
    let end = p.parts.iter().find(|x| x.what == What::End).unwrap();
    assert_eq!(placed[1].fade_out, (end.at, end.at + end.len), "the last out with the end card");
    // (the crossing: never past either bed's own peak by more than equal power's 3 dB, and no dip)
    let (a, b) = (tone(440.0, secs(60.0)), tone(660.0, secs(60.0)));
    let mut y = vec![0.0f32; len];
    lay(&placed[0], &a, None, 1.0, &mut y);
    lay(&placed[1], &b, None, 1.0, &mut y);
    let (c0, c1) = (sample_at(card.at), sample_at(card.at + card.len));
    let peak = y[c0..c1].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(peak > 0.6 && peak <= 0.5 * 10f32.powf(3.02 / 20.0), "the crossing's peak {peak}");
    let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
    let w = secs(0.05);
    for k in (c0..c1 - w).step_by(w) {
        let db = 20.0 * (rms(&y[k..k + w]) / rms(&a[..w])).log10();
        assert!(
            db.abs() < 0.5,
            "at {:.2} s the crossing is {db:.2} dB off the beds' own",
            k as f32 / RATE as f32
        );
    }
    // (the loop: its seams where the marks say — 3 s into the file, then every 2 s —, the file's 1 s after each)
    assert_eq!(&placed[1].seams[..2], [card.at + 3_000_000, card.at + 5_000_000]);
    let ramp: Vec<f32> = (0..secs(5.0)).map(|i| i as f32 / secs(5.0) as f32).collect();
    let mut r = vec![0.0f32; len];
    lay(&placed[1], &ramp, Some((secs(1.0), secs(3.0))), 1.0, &mut r);
    let file = |s: f64| s / 5.0;
    let near = |got: f32, want: f64| (f64::from(got) - want).abs() < 1e-3;
    for (k, seam) in placed[1].seams[..2].iter().enumerate() {
        assert!(near(r[sample_at(seam - 100_000)], file(2.9)), "seam {k}: before it, the file's 2.9 s");
        assert!(near(r[sample_at(seam + 100_000)], file(1.1)), "seam {k}: after it, the file's 1.1 s");
    }
    // (a bed with no loop that runs out: it fades to nothing at its end, and the next begins under it)
    // (from 2 s, so it runs out at 12 s)
    let (placed, said) = layout(&p, &[(secs(10.0), 0), (secs(5.0), 0)]);
    assert_eq!((placed[0].ran_out, placed[0].fade_out), (Some(12_000_000), (10_000_000, 12_000_000)));
    assert_eq!((placed[1].start, placed[1].fade_in), (10_000_000, (10_000_000, 12_000_000)));
    assert!(
        said.len() == 1 && said[0].contains("runs out at 0:12.0") && said[0].contains("bed 2"),
        "{said:?}"
    );
    let mut f = vec![0.0f32; len];
    lay(&placed[0], &tone(440.0, secs(10.0)), None, 1.0, &mut f);
    let level = |from: f64| rms(&f[secs(from)..secs(from + 0.05)]);
    assert!((level(9.0) / 0.3536 - 1.0).abs() < 0.02, "before its fade, as it is: {}", level(9.0));
    assert!(level(11.0) < 0.3536 * 0.75 && level(11.0) > 0.3536 * 0.6, "half way: {}", level(11.0));
    assert!(level(11.95) < 0.02 && f[secs(12.0)..].iter().all(|x| *x == 0.0), "gone at its end");
}

#[test]
fn the_bed_drops_under_the_keys_at_a_span_s_edge_and_comes_back_after_by_the_samples() {
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    // (where it ends; the 1× span 10–20 s after chapter 1's card; the 2× fit after chapter 2's card)
    assert_eq!(realtime(&p), [(6_000_000, 9_000_000), (13_500_000, 23_500_000), (25_500_000, 28_500_000)]);
    let span = [(5_000_000, 10_000_000)];
    let len = sample_at(12_000_000);
    let d = under(&span, len);
    let ones = vec![1.0f32; len];
    let y = mixed(&vec![0.0; len], &ones, &d, -10.0, 8.0);
    let db = |us: u64| 20.0 * y[sample_at(us)].log10();
    let near = |got: f32, want: f32| (got - want).abs() < 0.05;
    assert!(near(db(4_000_000), -10.0) && near(db(4_690_000), -10.0), "leading before the ramp");
    assert!(near(db(4_850_000), -14.0), "half way down, half a ramp before the edge: {}", db(4_850_000));
    assert!(near(db(5_000_000), -18.0) && near(db(7_500_000), -18.0) && near(db(9_999_000), -18.0), "under");
    assert!(near(db(10_150_000), -14.0), "half way up: {}", db(10_150_000));
    assert!(near(db(10_310_000), -10.0) && near(db(11_000_000), -10.0), "leading again");
    // (two stretches nearer than two ramps: the bed never comes all the way up between)
    let d = under(&[(1_000_000, 2_000_000), (2_400_000, 3_000_000)], len);
    assert!(d[sample_at(2_200_000)] > 0.3, "{}", d[sample_at(2_200_000)]);
}

/// The bed's own level (LUFS) at each video time, a stretch where the keys may sound at 5–10 s, the keys at −23.7 LUFS
/// and the mix `mix`: a bed laid at `BED_REF_LUFS` (a constant one, so each sample is its gain), through `mixed`.
fn bed_level(mix: Mix) -> impl Fn(u64) -> f32 {
    let len = sample_at(12_000_000);
    let d = under(&[(5_000_000, 10_000_000)], len);
    let (lead, depth) = bed_gains(-23.7, mix);
    let y = mixed(&vec![0.0; len], &vec![1.0; len], &d, lead, depth);
    move |us| 20.0 * y[sample_at(us)].log10() + BED_REF_LUFS
}

/// Before, half way down, under, half way up, after: the bed's level at each, against what's wanted.
fn lies(level: impl Fn(u64) -> f32, under: f32, lead: f32) {
    let near = |us: u64, want: f32| {
        let got = level(us);
        assert!((got - want).abs() < 0.05, "at {:.2} s: {got:.2}, not {want:.2}", us as f64 / 1e6);
    };
    near(4_000_000, lead);
    near(4_850_000, (lead + under) / 2.0);
    near(5_000_000, under);
    near(9_999_000, under);
    near(10_150_000, (lead + under) / 2.0);
    near(11_000_000, lead);
}

#[test]
fn the_bed_lies_at_the_constants_distances_from_the_keys_by_the_samples() {
    let c = cut(CUT).unwrap();
    assert_eq!(c.mix, Mix::default(), "no [sound]: the constants");
    // (the keys at −23.7: the bed at their own under them, 6 over that where it leads)
    lies(bed_level(c.mix), -23.7, -17.7);
}

#[test]
fn the_bed_lies_at_the_lists_distances_from_the_keys_by_the_samples() {
    let c = cut(&(CUT.to_string() + "\n[sound]\nunder = 5\nover = 6\n")).unwrap();
    assert_eq!(c.mix, Mix { under: 5.0, over: 6.0 });
    lies(bed_level(c.mix), -28.7, -22.7);
    // (one left out: its constant; out of its range: refused, with its line)
    let half = cut(&(CUT.to_string() + "\n[sound]\nover = 13.6\n")).unwrap();
    assert_eq!(half.mix, Mix { under: Mix::default().under, over: 13.6 });
    let e = cut(&(CUT.to_string() + "\n[sound]\nover = -1\n")).unwrap_err();
    assert!(e.starts_with("cut.toml:") && e.contains("sound: over"), "{e}");
    assert!(cut(&(CUT.to_string() + "\n[sound]\nswing = 6\n")).is_err());
}

fn tone(hz: f32, secs: f64) -> Vec<f32> {
    let n = (secs * f64::from(RATE)) as usize;
    (0..n).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / RATE as f32).sin()).collect()
}

fn rms_at(y: &[f32], us: u64) -> f32 {
    let s = &y[sample_at(us)..sample_at(us + 50_000)];
    (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
}

/// V2b: the cut is stereo. Bed 1 sounds only on the left, bed 2 only on the right: each arrives on its own side and
/// nothing of it on the other — levelled or at its own gain —; a key, in the centre, is the same on both.
#[test]
fn a_beds_left_and_right_arrive_on_their_own_sides_and_a_key_on_both_alike() {
    let c = cut(&bedded()).unwrap();
    let p = Program::new(&c);
    let len = sample_at(p.total);
    let (placed, _) = layout(&p, &[(sample_at(60_000_000), 0), (sample_at(60_000_000), 0)]);
    let silent = vec![0.0f32; sample_at(60_000_000)];
    let tracks = [
        Track { sides: [tone(440.0, 60.0), silent.clone()], from: 0, looped: None },
        Track { sides: [silent, tone(660.0, 60.0)], from: 0, looped: None },
    ];
    let mut keys = vec![0.0f32; len];
    for k in (0..len).step_by(sample_at(130_000)) {
        keys[k..(k + 200).min(len)].iter_mut().for_each(|x| *x = 0.3);
    }
    let card = p.parts.iter().find(|x| x.what == What::Chapter(1)).unwrap();
    let d = under(&realtime(&p), len);
    for gains in [[Some(0.0), Some(-3.0)], [None, None]] {
        let (beds, _) = laid(&placed, &tracks, &gains, len);
        let ([l, r], mine) = (sides(&keys, &beds, &d, -10.0, 6.0), gains[0].is_some());
        let ([bl, br], other) = match mine {
            true => (&beds.made, &beds.levelled),
            false => (&beds.levelled, &beds.made),
        };
        assert!(other.iter().all(Vec::is_empty), "{gains:?}: all one kind");
        // (bed 1 alone before chapter 2's card: on the left only; bed 2 alone after it: on the right only)
        let (one, two) =
            (sample_at(5_000_000)..sample_at(card.at), sample_at(card.at + card.len)..sample_at(p.total));
        assert!(
            br[one.clone()].iter().all(|x| *x == 0.0) && bl[two.clone()].iter().all(|x| *x == 0.0),
            "{gains:?}"
        );
        assert!(
            rms_at(bl, 5_000_000) > 0.01 && rms_at(br, card.at + card.len) > 0.01,
            "{gains:?}: each heard"
        );
        // (what's left over each side's beds as mixed is the keys, the same on both)
        let [ml, mr] = sides(&vec![0.0; len], &beds, &d, -10.0, 6.0);
        assert!(
            mr[one].iter().all(|x| *x == 0.0) && ml[two].iter().all(|x| *x == 0.0),
            "{gains:?}: as mixed"
        );
        for n in (0..len).step_by(97) {
            let (kl, kr) = (l[n] - ml[n], r[n] - mr[n]);
            assert!(
                (kl - keys[n]).abs() < 1e-6 && (kr - keys[n]).abs() < 1e-6,
                "{gains:?} at {n}: {kl} {kr}"
            );
        }
    }
}

/// A bed with `gain` plays at it, its level its own: before, in and after a 1× span the same by the samples, and its
/// file's level times its gain (not brought to `BED_REF_LUFS`); without it, V2's fall under the keys and rise after.
#[test]
fn a_made_bed_keeps_its_level_through_a_1x_span_and_a_levelled_one_swings() {
    let made = bedded().replace("file = \"a.wav\"\n", "file = \"a.wav\"\ngain = -6\n");
    for (list, gain) in [(made, Some(-6.0f32)), (bedded(), None)] {
        let c = cut(&list).unwrap();
        assert_eq!((c.beds[0].gain, c.beds[1].gain), (gain, None));
        let p = Program::new(&c);
        let len = sample_at(p.total);
        let (placed, _) = layout(&p, &[(sample_at(60_000_000), 0), (sample_at(60_000_000), 0)]);
        let both = || [tone(440.0, 60.0), tone(440.0, 60.0)];
        let tracks =
            [Track { sides: both(), from: 0, looped: None }, Track { sides: both(), from: 0, looped: None }];
        let gains: Vec<Option<f32>> = c.beds.iter().map(|b| b.gain).collect();
        let (beds, _) = laid(&placed, &tracks, &gains, len);
        let real = realtime(&p);
        let (a, b) = real[0];
        assert!(placed[0].fade_in.1 < a - 800_000 && placed[0].fade_out.0 > b + 800_000, "{:?}", placed[0]);
        let (lead, depth) = bed_gains(-20.0, Mix::default());
        let [l, r] = sides(&vec![0.0; len], &beds, &under(&real, len), lead, depth);
        let db = |us: u64| 20.0 * rms_at(&l, us).log10();
        let (before, inside, after) = (db(a - 800_000), db((a + b) / 2), db(b + 800_000));
        assert_eq!(l, r, "the same file on both sides");
        match gain {
            Some(g) => {
                let file = 20.0 * (0.5f32 / 2f32.sqrt()).log10();
                for (at, got) in [("before", before), ("in", inside), ("after", after)] {
                    assert!(
                        (got - (file + g)).abs() < 0.01,
                        "{at} the span: {got:.3} dB, not {:.3}",
                        file + g
                    );
                }
            }
            None => {
                assert!((before - after).abs() < 0.01, "before {before:.2}, after {after:.2}");
                assert!((before - inside - depth).abs() < 0.05, "under the keys by {:.2}", before - inside);
            }
        }
    }
    for bad in ["gain = \"loud\"", "gain = 30", "gain = -41"] {
        let e =
            cut(&bedded().replace("file = \"a.wav\"\n", &format!("file = \"a.wav\"\n{bad}\n"))).unwrap_err();
        assert!(e.starts_with("cut.toml:") && e.contains("music: gain"), "{bad}: {e}");
    }
}

/// By hand (V2b): a ten-minute cut's sound at the build's opt-level, each step timed — once a cut: a stereo bed written
/// and read back as ffmpeg decodes one, laid at its gain, the swing's curve, the keys rendered (a key every 0.3 s over
/// half the video) and at the cap; each ceiling try: the keys at it, their peaks, the two sides, their true peaks and
/// limiter, the figures (the true peaks after bounded, then read whole to check — not a try's), the WAV and its AAC;
/// once after: the 3 s windows. Then the most memory held.
#[test]
#[ignore = "by hand: a ten-minute cut's sound, timed"]
fn ten_minutes_of_the_cuts_sound_timed() {
    use std::time::Instant;
    let total: u64 = 600_000_000;
    let len = sample_at(total);
    let (all, mut t) = (Instant::now(), Instant::now());
    let mut lap = |what: &str| {
        eprintln!("{what:>44}: {:6.1} s", t.elapsed().as_secs_f64());
        t = Instant::now();
    };
    let dir = tarae_testkit::scratch::Scratch::new("xtask-ten-minutes");
    let mut seed = 1u32;
    let mut noise = |n: usize| -> Vec<f32> {
        (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 * 0.2 - 0.1
            })
            .collect()
    };
    let n = sample_at(total + 10_000_000);
    let (bl, br) = (noise(n), noise(n));
    let bed = dir.join("bed.wav");
    crate::sound::wav_sides(&bed, &[&bl, &br]).unwrap();
    drop((bl, br));
    lap("(once) a stereo bed written, 610 s");
    let sides_read = match crate::video::ffmpeg() {
        Some(f) => crate::video::decoded_sides(&f, &bed, Duration::from_micros(total + 10_000_000)).unwrap(),
        None => [noise(n), noise(n)],
    };
    lap("(once) read back, both sides");
    let placed = Placed {
        bed: 0,
        start: 0,
        from: 0,
        fade_in: (0, 500_000),
        fade_out: (total - 3_000_000, total),
        seams: Vec::new(),
        ran_out: None,
    };
    let (beds, made) =
        laid(&[placed], &[Track { sides: sides_read, from: 0, looped: None }], &[Some(4.0)], len);
    assert_eq!(made.len(), 1);
    lap("(once) laid at its gain, its peak found");
    let d = under(&[(0, total / 2)], len);
    lap("(once) the swing's curve");
    let ms = Duration::from_millis;
    let heard: Vec<(Duration, Heard)> = (0..1_000).map(|i| (ms(i * 300), Heard::Key('a'))).collect();
    let rendered = crate::sound::Keys::new(&heard, Duration::from_micros(total), Some(ms(595_000)));
    lap("(once) the keys rendered, their true peaks");
    let unit = rendered.unit();
    lap("(once) the keys at the cap");
    let keys = rendered.mastered(&unit, -2.6);
    lap("(a try) the keys at its ceiling");
    let peaks = super::sound::key_peaks(&keys.samples, heard.iter().map(|h| h.0.as_micros() as u64));
    assert_eq!(peaks.len(), 1_000);
    lap("(a try) each key's peak");
    let [mut l, mut r] = sides(&keys.samples, &beds, &d, 0.0, 6.0);
    drop(keys);
    lap("(a try) the two sides");
    let tp = crate::sound::true_peaks_of(&[&l, &r]);
    lap("(a try) their true peaks");
    let gains = crate::sound::limit_sides(&mut [&mut l, &mut r], &tp, 10f32.powf(-2.0 / 20.0));
    lap("(a try) the limiter");
    let _ = crate::sound::stereo_lufs(&l, &r);
    lap("(a try) the whole's loudness");
    let tps =
        crate::sound::true_peak_after(&[&l, &r], &tp, &gains, &[(0, len), (0, len / 2), (len / 2, len)]);
    lap("(a try) true peaks after, bounded: whole, 2 chapters");
    drop((tp, gains));
    let exact = crate::sound::true_peak_db(&[&l, &r]);
    assert_eq!((20.0 * tps[0].log10()).to_bits(), exact.to_bits());
    lap("(not in a try) the whole's true peak, read whole");
    let _ = crate::sound::correlation(&l, &r);
    lap("(a try) the correlation");
    let wav = dir.join("cut.wav");
    crate::sound::wav_sides(&wav, &[&l, &r]).unwrap();
    lap("(a try) the WAV");
    if let Some(f) = crate::video::ffmpeg() {
        let _ = crate::video::aac_peak(&f, &wav, crate::video::Form::Wide);
        lap("(a try) its AAC's true peak");
    }
    let _ = crate::sound::stereo_short_term(&l, &r, 3.0, 1.0);
    lap("(after) the 3 s windows");
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: getrusage fills the struct it's given.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    let k = if cfg!(target_os = "macos") { 1 } else { 1024 };
    eprintln!(
        "{:>44}: {:6.1} s, at most {} MB held",
        "all",
        all.elapsed().as_secs_f64(),
        u.ru_maxrss as u64 * k / (1024 * 1024)
    );
}

/// A made bed's lines: its loudest moment against the quietest key's peak, the keys' quietest and loudest — and a line
/// of its own when that moment comes within `NEAR_KEY_DB` of the quietest key.
#[test]
fn a_made_beds_loudest_moment_near_a_key_is_said_in_a_line_of_its_own() {
    let m = |peak: f32| Made { bed: 0, lufs: -40.0, peak, at: 12_300_000 };
    let keys = [(1_000_000, -9.0), (2_000_000, -4.5), (3_400_000, -12.0), (5_000_000, -3.0)];
    let said = made_said(4.0, &m(-18.5), &keys, video_time);
    assert_eq!(said.len(), 2, "{said:#?}");
    assert!(said[0].contains("+4.0 dB on the file as made") && said[0].contains("-18.5 dBTP"), "{}", said[0]);
    assert!(
        said[1].contains("6.5 dB under the quietest key's peak (-12.0 dBTP, at 0:03.4)")
            && said[1].contains("-12.0 to -3.0 dBTP, their median -4.5 (4 keys")
            && said[1].contains("0:12.3"),
        "{}",
        said[1]
    );
    let near = made_said(4.0, &m(-12.0 - NEAR_KEY_DB + 0.5), &keys, video_time);
    assert!(near.len() == 3 && near[2].contains("bed 1's loudest moment comes within 6 dB"), "{near:#?}");
    let over = made_said(4.0, &m(-10.0), &keys, video_time);
    assert!(over[1].contains("2.0 dB over the quietest") && over.len() == 3, "{over:#?}");
    let none = made_said(0.0, &m(-20.0), &[], video_time);
    assert!(none.len() == 2 && none[1].contains("no key heard"), "{none:#?}");
}

fn drawn(ms: u64, text: &str) -> Rec {
    Rec { us: ms * 1000, whose: SCREEN, bytes: format!("\x1b[H\x1b[2J\x1b[?25l{text}").into_bytes() }
}

#[test]
fn a_screen_where_only_a_timer_or_a_spinner_moves_stands_still() {
    let screen = |text: &str| {
        let mut p = vt100::Parser::new(2, 40, 0);
        p.process(drawn(0, text).bytes.as_slice());
        ticking(p.screen())
    };
    assert_eq!(screen("running ⠋  9.9s"), screen("running ⠙ 10.0s"));
    assert_eq!(screen("writing ▂▅ 7m 59s"), screen("writing ▇▁ 8m 0s"));
    assert_ne!(screen("running 10.0s"), screen("runnint 10.0s"));
    let recs = [
        drawn(0, "waiting 1s"),
        drawn(1_000, "waiting 2s"),
        drawn(2_500, "waiting 3s"),
        drawn(3_000, "answered"),
        drawn(4_000, "answered."),
        drawn(4_400, "answered!"),
    ];
    assert_eq!(stills(&recs, (40, 2), 5_000), [(0, 3_000)], "1.0 s and 0.6 s still are no still stretch");
    assert_eq!(stills(&recs, (40, 2), 7_000), [(0, 3_000), (4_400, 7_000)], "to the take's end");
    // (the replay: the screen in force at a time, one held)
    let mut r = Replay::new(&recs, (40, 2));
    assert!(r.to(2_600));
    assert!(r.screen().contents().starts_with("waiting 3s"));
    assert!(!r.to(2_900), "nothing new by then");
}

/// `cut.txt` names every place `record check` found that a span of the cut shows — a weak one too (a two-letter login
/// as a bare word) —, with its video time, take time, row and words; one in a stretch left out isn't named; with none
/// inside, it says so. `check.txt` read back as `listed` wrote it.
#[test]
fn cut_txt_names_every_check_hit_inside_the_cut_and_none_outside() {
    use crate::record::check::{Hit, listed, parse};
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let hit = |from: u64, to: u64, found: &str, weak: bool| Hit {
        from,
        to,
        row: 3,
        kind: "name",
        found: found.into(),
        text: format!("drwxr-xr-x  4 {found} wheel  128 tasks"),
        weak,
    };
    // (12–13 s in the 1× span; 30–31 s left out)
    let hits = vec![hit(12_000, 13_000, "zq", true), hit(30_000, 31_000, "zz", false)];
    let read = parse(&listed(&hits)).unwrap();
    assert_eq!(read, [hits[1].clone(), hits[0].clone()], "the weak listed after the rest");
    let said = super::check::said(&p, &read, &[], "check.txt");
    let v = video_time(p.video_of(c.map.video(12_000)).unwrap());
    assert_eq!(
        said,
        [
            "check (check.txt): 0 strong, 1 weak inside the cut's spans, crossings included — of the take's 2 hits; for \
             the user to decide:"
                .to_string(),
            format!(
                "  {v} in the video (take 0:00:12.00–0:00:13.00), row 3: name zq (weak) │ drwxr-xr-x  4 zq wheel  128 \
                 tasks"
            ),
        ]
    );
    assert_eq!(
        super::check::said(&p, &read[..1], &[], "check.txt"),
        [
            "check (check.txt): 0 strong, 0 weak inside the cut's spans, crossings included — of the take's 1 hits; none \
          inside the cut"
        ]
    );
    // (a strong one inside is counted as strong: the rough cut goes to the user only with 0)
    let strong = super::check::said(&p, &[hit(13_000, 14_000, "zz", false)], &[], "check.txt");
    assert!(strong[0].starts_with("check (check.txt): 1 strong, 0 weak inside"), "{strong:?}");
    // (where it ends shows its stretch too: a hit there named at both its video times)
    assert_eq!(super::check::shown(&p, 56_000, 57_000).len(), 2);
}

/// The keys' track and the mix are one length, `track_len`'s, on a cut whose total in samples rounds up (V4's made-up
/// 90 minutes: a chapter's range one sample past the keys' track).
#[test]
fn the_keys_track_and_the_mix_are_one_length_however_the_total_rounds() {
    let c = cut(&CUT.replace("default = 4", "default = 3")).unwrap();
    let p = Program::new(&c);
    let exact = p.total as f64 * f64::from(RATE) / 1e6;
    assert_ne!(exact.round() as usize, exact as usize, "the total rounds up: {} µs", p.total);
    let len = track_len(&p);
    let heard = [(std::time::Duration::from_secs(1), Heard::Key('k'))];
    let keys = crate::sound::Keys::new(&heard, std::time::Duration::from_micros(p.total), None);
    let keys = keys.mastered(&keys.unit(), crate::sound::SHORT_CEILING_DB).samples;
    assert_eq!(keys.len(), len, "the keys' track");
    let beds = Laid { levelled: [vec![0.0; len], vec![0.0; len]], made: [vec![0.0; len], vec![0.0; len]] };
    let d = under(&realtime(&p), len);
    let [l, r] = sides(&keys, &beds, &d, 0.0, 6.0);
    assert_eq!((l.len(), r.len()), (len, len), "the mix");
}

/// A change of the whole screen crosses only between two screens that each stand `camera::STAND_MS` of video: a stretch
/// of whole-screen changes every 250 ms of video — at 4× (one a second of take) and at 1× — has no crossing frame, a
/// lone change still crosses (V4's sped 4K clip: two frames in four were two screens over each other).
#[test]
fn a_change_crosses_only_between_two_screens_that_each_stand() {
    // (the rule alone: before and after, against the edit list's edges too)
    let s = |ms: &[u64]| ms.iter().map(|m| m * 1000).collect::<Vec<u64>>();
    assert_eq!(camera::standing(&s(&[5_000]), &[]), s(&[5_000]), "a lone change");
    assert_eq!(camera::standing(&s(&[5_000, 5_250, 5_500]), &[]), s(&[]), "every 250 ms");
    assert_eq!(camera::standing(&s(&[5_000, 5_600]), &[]), s(&[5_000, 5_600]), "600 ms apart");
    assert_eq!(camera::standing(&s(&[5_000]), &s(&[4_700])), s(&[]), "a part began 300 ms before");
    assert_eq!(camera::standing(&s(&[5_000]), &s(&[5_300])), s(&[]), "time left out 300 ms after");
    // (through the replay, as the frames see it: the made-up cut's 0–10 s at 4×, 10–20 s at 1×)
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    // (three cells in four changed: more than `camera::CROSS`, not all)
    let full = |ms: u64, ch: char| {
        let row: String = std::iter::repeat_n(ch, 6).chain("xx".chars()).collect();
        Rec {
            us: ms * 1000,
            whose: SCREEN,
            bytes: format!("\x1b[H\x1b[2J\x1b[?25l{row}\r\n{row}").into_bytes(),
        }
    };
    let mut recs = vec![full(0, 'a')];
    let sped: Vec<u64> = (4..=7).map(|s| s * 1_000).collect();
    let real: Vec<u64> = (0..=12).map(|i| 10_500 + i * 250).collect();
    let lone = 16_000;
    for (i, &ms) in sped.iter().chain(&real).chain([&lone]).enumerate() {
        recs.push(full(ms, if i % 2 == 0 { 'b' } else { 'c' }));
    }
    let take = Take { about: Value::Null, size: (8, 2), screen: recs };
    let (whole, crossing) = super::draw::changes(&p, &take);
    // (a change is found on the first frame that shows it, or the next)
    let v = |t: u64| p.video_of(c.map.video(t)).unwrap();
    let near =
        |list: &[u64], t: u64| list.iter().any(|&x| x >= v(t) && x <= v(t) + 2 * 1_000_000 / video::FPS);
    for &t in sped.iter().chain(&real).chain([&lone]) {
        assert!(near(&whole, t), "the change at {t} ms found: {whole:?}");
    }
    assert_eq!(whole.len(), sped.len() + real.len() + 1, "{whole:?}");
    assert!(
        crossing.len() == 1 && near(&crossing, lone),
        "only the lone change crosses: {crossing:?} of {whole:?}"
    );
}

/// By hand (the report's cost): a made-up 90-minute take at 182×36 written to `$CUT_MADE_UP` (a folder not there yet) —
/// twenty cycles of an ask typed (10 s, a key every 150 ms), a turn streaming its answer for three minutes (a line every
/// 200 ms, the whole screen redrawn every 2 s, a run card shown 5 s at its middle), then 80 s where only a timer ticks;
/// the front changing every seventh cycle — about tarae's bytes a minute as V0 measured them. Then `cargo xtask cut init`
/// and `cargo xtask cut` on it, under `/usr/bin/time -l`.
#[test]
#[ignore = "by hand: writes a made-up 90-minute take where $CUT_MADE_UP says, for the cut's cost"]
fn by_hand_a_made_up_ninety_minute_take() {
    use std::io::Write;
    let dir =
        std::path::PathBuf::from(std::env::var("CUT_MADE_UP").expect("CUT_MADE_UP: where the take goes"));
    assert!(!dir.exists(), "{} is there already", dir.display());
    std::fs::create_dir_all(&dir).unwrap();
    let (cols, rows) = (182u16, 36u16);
    let mut screen = std::io::BufWriter::new(std::fs::File::create(dir.join("screen.bin")).unwrap());
    let mut keys = std::io::BufWriter::new(std::fs::File::create(dir.join("keys.bin")).unwrap());
    let mut marks: Vec<Value> = Vec::new();
    let mut seed = 7u64;
    let mut word = || {
        const WORDS: [&str; 12] = [
            "the", "task", "file", "date", "reads", "parse", "board", "tests", "column", "due", "tag", "list",
        ];
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        WORDS[(seed >> 33) as usize % WORDS.len()]
    };
    let put = |mut w: &mut dyn Write, ms: u64, whose: u8, b: &str| {
        crate::record::put(&mut w, ms * 1000, whose, b.as_bytes()).unwrap()
    };
    let frame = |inner: String| format!("\x1b[?2026h{inner}\x1b[?2026l");
    let (mut bytes, cycle, n) = (0usize, 270_000u64, 20u64);
    for k in 0..n {
        let t0 = k * cycle;
        if k % 7 == 0 {
            let name = if k % 14 == 0 { "main" } else { "fix" };
            marks.push(json!({ "ms": t0 + 1, "what": "front", "to": name, "name": name }));
        }
        let full = |word: &mut dyn FnMut() -> &'static str, timer: &str, card: bool| {
            let mut s = String::from("\x1b[H\x1b[2J");
            for r in 1..=rows {
                let line: Vec<&str> = (0..26).map(|_| word()).collect();
                s += &format!("\x1b[{r};1H\x1b[38;2;200;190;170m{:>4} {}\x1b[0m", r, line.join(" "));
            }
            if card {
                s += "\x1b[20;60H╭─ Claude wants to run ─ outside the sandbox ─╮\x1b[21;60H│ cargo test │";
            }
            frame(s + &format!("\x1b[{rows};160H{timer}"))
        };
        // (the ask typed)
        let mut typed = String::new();
        for i in 0..66 {
            let ms = t0 + 100 + i * 150;
            let c = (b'a' + (i % 26) as u8) as char;
            typed.push(c);
            put(&mut keys, ms, crate::record::SCRIPT, &c.to_string());
            let b = frame(format!("\x1b[34;100H{typed}"));
            bytes += b.len();
            put(&mut screen, ms + 5, SCREEN, &b);
        }
        // (the turn)
        let (start, end) = (t0 + 10_000, t0 + 190_000);
        marks.push(json!({ "ms": start, "what": "turn-started", "session": "s0" }));
        let card = (t0 + 100_000, t0 + 105_000);
        marks.push(json!({ "ms": card.0, "what": "card-shown", "id": format!("card#{k}"), "kind": "run" }));
        marks
            .push(json!({ "ms": card.1, "what": "card-answered", "id": format!("card#{k}"), "kind": "run" }));
        for ms in (start..end).step_by(200) {
            let timer = format!("writing {:.1}s", (ms - start) as f64 / 1000.0);
            let b = match (ms - start) % 2_000 {
                0 => full(&mut word, &timer, (card.0..card.1).contains(&ms)),
                _ => {
                    let line: Vec<&str> = (0..9).map(|_| word()).collect();
                    let row = 3 + (ms / 200) % 28;
                    frame(format!("\x1b[{row};120H{}\x1b[{rows};160H{timer}", line.join(" ")))
                }
            };
            bytes += b.len();
            put(&mut screen, ms, SCREEN, &b);
        }
        marks.push(json!({ "ms": end, "what": "turn-ended", "session": "s0" }));
        // (only a timer ticks)
        for s in 0..80 {
            let b = frame(format!("\x1b[{rows};160Hready {s}s"));
            bytes += b.len();
            put(&mut screen, end + 500 + s * 1000, SCREEN, &b);
        }
    }
    let ms = n * cycle;
    let about = json!({ "v": 1, "size": [cols, rows], "ms": ms, "started_unix_ms": 0, "made_up": true });
    std::fs::write(dir.join("take.json"), about.to_string()).unwrap();
    let lines: String = marks.iter().map(|m| m.to_string() + "\n").collect();
    std::fs::write(dir.join("landmarks.jsonl"), lines).unwrap();
    println!(
        "{}: {} min, {} KB of screen ({:.0} KB a minute)",
        dir.display(),
        ms / 60_000,
        bytes / 1024,
        bytes as f64 / 1024.0 / (ms as f64 / 60_000.0)
    );
}

#[test]
fn cut_init_gives_the_spans_its_rules_say_and_reads_them_back() {
    let keys: Vec<Rec> = [(5_000, &b"k"[..]), (6_000, b"k"), (7_400, b"k"), (8_800, b"\r")]
        .iter()
        .map(|&(ms, b)| Rec { us: ms * 1000, whose: SCRIPT, bytes: b.to_vec() })
        .chain([Rec {
            us: 30_000_000,
            whose: crate::record::TERMINAL,
            bytes: b"\x1b]11;rgb:0/0/0\x07".to_vec(),
        }])
        .collect();
    let facts = Facts::of(&marks(), &keys, vec![(25_000, 35_000)]);
    assert_eq!(facts.typed, [(5_000, 8_800)], "the terminal's answer is no key");
    assert_eq!(facts.sent, [(7_400, 8_800)], "the return, and the key before it");
    let got: Vec<(u64, u64, Pace)> = rules(&facts).iter().map(|(s, _)| (s.from, s.to, s.pace)).collect();
    let (one, two, four) = (Pace::Speed(1.0), Pace::Speed(2.0), Pace::Speed(4.0));
    assert_eq!(
        got,
        [
            (0, 5_000, four),
            (5_000, 7_400, two),   // typed at 2×
            (7_400, 9_300, one),   // complete from its last key, to half a second past its return
            (9_300, 12_000, four), // the rest, then the turn: 16 s left of it, so never under 4×
            (12_000, 16_000, one), // card 1, shown to answered + 1 s
            (16_000, 25_000, four),
            (25_000, 25_500, one), // a still screen's first half second
            (25_500, 35_000, Pace::Out),
            (35_000, 50_000, four),
            (50_000, 55_000, four), // apart at `fix`'s chapter
            (55_000, 57_000, one),  // card 2
            (57_000, 120_000, four),
        ]
    );
    // (a long turn: in at most 20 s on screen)
    let long = Facts {
        end: 200_000,
        turns: vec![((0, "turn 1 start".into()), (200_000, "turn 1 end".into()))],
        ..Facts::default()
    };
    assert_eq!(rules(&long).iter().map(|(s, _)| s.pace).collect::<Vec<_>>(), [Pace::Speed(10.0)]);
    // (a return 3 s after the last key: the ask stands its last 1.5 s at 1×, never more)
    let late = Facts {
        end: 30_000,
        typed: vec![(20_000, 20_000), (23_000, 23_000)],
        sent: vec![(20_000, 23_000)],
        ..Facts::default()
    };
    assert_eq!(
        rules(&late).iter().map(|(s, _)| (s.from, s.to, s.pace)).collect::<Vec<_>>(),
        [
            (0, 20_000, four),
            (20_000, 20_500, two),
            (20_500, 21_500, four),
            (21_500, 23_500, one),
            (23_500, 30_000, four)
        ]
    );
    // (a card stays 1× where something is typed in it)
    let in_card = Facts { typed: vec![(13_000, 13_000)], ..Facts::of(&marks(), &[], Vec::new()) };
    assert!(rules(&in_card).iter().any(|(s, _)| (s.from, s.to, s.pace) == (12_000, 16_000, one)));
    // (what it writes reads back to the same stretches, edges as landmarks where they are)
    let (text, said) = written(&facts, std::path::Path::new("take"));
    assert!(text.contains("to = \"card 1 answered + 1s\"") && text.contains("at = \"front 2\""), "{text}");
    assert!(said.contains("4 at 1×") && said.contains("1 left out (9.5 s)"), "{said}");
    // (the opening's words left to fill in: refused until they're written)
    let e = cut(&text).unwrap_err();
    assert!(e.contains("opening: its name, one line"), "{e}");
    let text =
        text.replacen("name = \"\"", "name = \"halil\"", 1).replacen("line = \"\"", "line = \"A tool.\"", 1);
    let back = cut(&text).unwrap();
    assert_eq!(
        (back.opening.from, back.opening.len),
        (114_000, 4_000),
        "where it ends: the take's last seconds"
    );
    let read: Vec<(u64, u64, Pace)> = back.map.segs().iter().map(|s| (s.from, s.to, s.pace)).collect();
    assert_eq!(read, got, "{text}");
    assert_eq!(
        back.chapters.iter().map(|c| (c.at, c.title.as_str())).collect::<Vec<_>>(),
        [(0, "main"), (50_000, "fix")]
    );
}

/// A card waited on (7 s or more) is 1× for its first 4 s and from 2 s before its answer to 1 s past it; between, the
/// other rules decide — a still left out, the rest at the default — so its wait isn't held (take 1: 601 of init's
/// 1,103 s were cards). One answered within 7 s stays one span. What's written reads back to the same stretches.
#[test]
fn cut_init_holds_a_waited_card_for_its_read_and_its_answer_only() {
    let at = |ms: u64, what: &str, id: &str| json!({ "ms": ms, "what": what, "id": id, "kind": "run" });
    let m = Marks::new(
        vec![
            at(10_000, "card-shown", "card#1"),
            at(40_000, "card-answered", "card#1"),
            at(50_000, "card-shown", "card#2"),
            at(56_900, "card-answered", "card#2"),
            at(60_000, "card-shown", "card#3"),
            at(67_000, "card-answered", "card#3"),
        ],
        90_000,
    );
    let f = Facts::of(&m, &[], vec![(20_000, 36_000)]);
    let held: Vec<(usize, u64, u64)> = f.cards.iter().map(|c| (c.0, c.1.0, c.2.0)).collect();
    assert_eq!(
        held,
        [
            (1, 10_000, 14_000),
            (1, 38_000, 41_000),
            (2, 50_000, 57_900),
            (3, 60_000, 64_000),
            (3, 65_000, 68_000)
        ],
        "a 30 s wait in two; 6.9 s whole; 7 s in two"
    );
    let (one, four) = (Pace::Speed(1.0), Pace::Speed(4.0));
    let got: Vec<(u64, u64, Pace)> = rules(&f).iter().map(|(s, _)| (s.from, s.to, s.pace)).collect();
    assert_eq!(
        got,
        [
            (0, 10_000, four),
            (10_000, 14_000, one),
            (14_000, 20_000, four),
            (20_000, 20_500, one),
            (20_500, 36_000, Pace::Out),
            (36_000, 38_000, four),
            (38_000, 41_000, one),
            (41_000, 50_000, four),
            (50_000, 57_900, one),
            (57_900, 60_000, four),
            (60_000, 64_000, one),
            (64_000, 65_000, four),
            (65_000, 68_000, one),
            (68_000, 90_000, four),
        ]
    );
    let (text, _) = written(&f, std::path::Path::new("take"));
    assert!(
        text.contains("to = \"card 1 shown + 4s\"") && text.contains("from = \"card 1 answered - 2s\""),
        "{text}"
    );
    let text =
        text.replacen("name = \"\"", "name = \"halil\"", 1).replacen("line = \"\"", "line = \"A tool.\"", 1);
    let back = Cut::parse(&text, "init.toml", &m).unwrap();
    let read: Vec<(u64, u64, Pace)> = back.map.segs().iter().map(|s| (s.from, s.to, s.pace)).collect();
    assert_eq!(read, got, "{text}");
}

// ── The mascot at the keys ──

use super::mascot::{self, Art, Hands, Pose, Press, Side};

#[test]
fn a_keys_side_is_its_half_of_a_qwerty() {
    let cases: [(&[u8], Side); 17] = [
        (b"q", Side::Left),
        (b"B", Side::Left),
        (b"5", Side::Left),
        (b"6", Side::Right),
        (b"p", Side::Right),
        (b":", Side::Right),
        (b"\x1b", Side::Left),
        (b"\t", Side::Left),
        (b"\x7f", Side::Right),
        (b"\x1b[A", Side::Right),
        (b"\x11", Side::Left), // C-q
        (b"\x1bj", Side::Right),
        (b"\x1b[27u", Side::Left),
        (b"\x1b[106;5u", Side::Right),
        (b" ", Side::Space),
        (b"\x1b[13u", Side::Both),
        (b"sh git worktree add", Side::Left), // the key script's line: its first
    ];
    for (b, s) in cases {
        assert_eq!(mascot::side(b), s, "{:?}", String::from_utf8_lossy(b));
    }
    assert_eq!(mascot::side(b"\r"), Side::Both);
}

#[test]
fn the_mascot_taps_where_a_key_is_heard_on_its_side_and_nowhere_else() {
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let keys = [
        key(5_000, b"f"),   // the default's 4×: not heard, no tap
        key(14_000, b"a"),  // 1×, left
        key(14_300, b"j"),  // right
        key(14_600, b"\r"), // a return: both
        key(14_950, b"y"),  // the key that answers card 1: the stamp, its own paw (right), one press
        key(30_000, b"d"),  // left out
    ];
    let s = sounds(&p, &keys, &marks());
    // (the opening's where it ends shows 55–58 s: card 2's answer at 56 s, its stamp there at 7 s — no key heard, no
    // paw)
    assert_eq!(
        s.taps,
        [
            (17_500_000, Side::Left, Press::Key),
            (17_800_000, Side::Right, Press::Key),
            (18_100_000, Side::Both, Press::Key),
            (18_500_000, Side::Right, Press::Stamp)
        ]
    );
    assert_eq!((s.stamps, s.stamps_keyless), (2, 1));
    // (a tap where every key and every stamp with its key sounds, and the sound's own times)
    let heard: Vec<u64> = s.list.iter().map(|h| h.0.as_micros() as u64).filter(|&v| v != 7_000_000).collect();
    assert_eq!(s.taps.iter().map(|t| t.0).collect::<Vec<_>>(), heard);
    let hands = Hands::new(&s.taps);
    let at = |us: u64| hands.pose(Program::frame_at(us), false);
    assert_eq!(
        [at(17_500_000), at(17_800_000), at(18_100_000), at(18_500_000), at(7_000_000)],
        [Pose::Left, Pose::Right, Pose::Both, Pose::Right, Pose::Watch]
    );
    // (between them the hands up; where the 4× key was, no tap: watching)
    assert_eq!(at(17_700_000), Pose::Up);
    let fast = p.video_of(c.map.video(5_000)).unwrap();
    assert_eq!(mascot::pose_at(&hands, Program::frame_at(fast), &p.frame(fast)), Some(Pose::Watch));
    assert_eq!(hands.taps(), 4);
}

#[test]
fn a_stamp_is_one_press_of_its_keys_paw_held_as_a_seal_the_other_up() {
    let stamp = Press::Stamp.hold();
    assert_eq!((Press::Key.hold(), stamp), (3, 9), "90 ms and 300 ms at 30 fps");
    // (a y's stamp: the right paw down its nine frames, the left up all through, then both up)
    let h = Hands::new(&[(1_000_000, Side::Right, Press::Stamp)]);
    let k = Program::frame_at(1_000_000);
    let poses: Vec<Pose> = (k..k + stamp + 1).map(|f| h.pose(f, false)).collect();
    let mut want = vec![Pose::Right; stamp as usize];
    want.push(Pose::Up);
    assert_eq!(poses, want);
    // (an a's, c's or s's: the left)
    for b in [&b"a"[..], b"c", b"s"] {
        assert_eq!(mascot::side(b), Side::Left, "{:?}", String::from_utf8_lossy(b));
    }
    assert_eq!(mascot::side(b"y"), Side::Right);
    // (the same paw asked again inside the seal: up one frame first, as for a key)
    let h = Hands::new(&[(0, Side::Left, Press::Stamp), (100_000, Side::Left, Press::Key)]);
    assert_eq!((h.pose(1, false), h.pose(2, false), h.pose(3, false)), (Pose::Left, Pose::Up, Pose::Left));
    // (from the sound: the answering key taps once, the stamp's own press — no tap of its own beside it)
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let s = sounds(&p, &[key(14_950, b"c")], &marks());
    assert_eq!(s.taps, [(18_500_000, Side::Left, Press::Stamp)]);
}

/// A card that goes once its work is done (the grammar's, 2.4 s after its y in take 3): the y's own press is the seal,
/// where the y is heard; the stamp sounds where it did, with no paw of its own.
#[test]
fn a_card_answered_before_it_goes_is_pressed_where_its_key_is() {
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let how = |how: &str| {
        Marks::new(
            vec![
                json!({ "what": "card-shown", "id": "card#1", "kind": "grammars", "ms": 12_000 }),
                json!({ "what": "card-answered", "id": "card#1", "kind": "grammars", "how": how, "ms": 15_000 }),
            ],
            120_000,
        )
    };
    let Where::At(at_y) = heard_at(&p, 13_000) else { panic!("13 s is shown at 1x") };
    let s = sounds(&p, &[key(13_000, b"y")], &how("answered y"));
    assert_eq!(s.taps, [(at_y[0], Side::Right, Press::Stamp)]);
    assert_eq!((s.stamps, s.stamps_keyless), (1, 0));
    assert_eq!(s.list.iter().filter(|h| h.1 == Heard::Stamp).count(), 1, "the stamp's sound where it was");
    assert_eq!(s.list.iter().filter(|h| matches!(h.1, Heard::Key(_))).count(), 1, "and the y's own");
    // (not its letter, another key after it, or further back than `ANSWER_WAIT_MS`: a typed key, the stamp no paw)
    for (keys, how_) in [
        (vec![key(13_000, b"y")], "answered n"),
        (vec![key(13_000, b"y"), key(14_000, b"x")], "answered y"),
        (vec![key(15_000 - super::sound::ANSWER_WAIT_MS - 1, b"y")], "answered y"),
    ] {
        let s = sounds(&p, &keys, &how(how_));
        assert!(s.taps.iter().all(|t| t.2 == Press::Key), "{:?} {how_}", s.taps);
        assert_eq!(s.stamps_keyless, 1);
    }
}

#[test]
fn a_hand_stays_down_its_hold_and_the_same_hand_again_comes_up_one_frame_first() {
    let frame = 1_000_000 / video::FPS;
    // (alone: down three frames, then up)
    let one = Hands::new(&[(1_000_000, Side::Left, Press::Key)]);
    let k = Program::frame_at(1_000_000);
    let poses: Vec<Pose> = (k..k + 4).map(|f| one.pose(f, false)).collect();
    assert_eq!(poses, [Pose::Left, Pose::Left, Pose::Left, Pose::Up]);
    // (the same hand twice in 60 ms, and in 20 ms: an up frame between)
    for gap in [60_000, 20_000] {
        let h = Hands::new(&[(0, Side::Right, Press::Key), (gap, Side::Right, Press::Key)]);
        let poses: Vec<Pose> = (0..4).map(|f| h.pose(f, false)).collect();
        assert_eq!(poses, [Pose::Right, Pose::Up, Pose::Right, Pose::Right], "{gap} µs apart");
    }
    // (the other hand is no wait: both down together)
    let h = Hands::new(&[(0, Side::Left, Press::Key), (frame, Side::Right, Press::Key)]);
    assert_eq!((h.pose(0, false), h.pose(1, false)), (Pose::Left, Pose::Both));
    // (the space bar: the hand that didn't tap last)
    let h = Hands::new(&[
        (0, Side::Left, Press::Key),
        (500_000, Side::Space, Press::Key),
        (1_000_000, Side::Space, Press::Key),
    ]);
    assert_eq!(h.pose(Program::frame_at(500_000), false), Pose::Right);
    assert_eq!(h.pose(Program::frame_at(1_000_000), false), Pose::Left);
}

#[test]
fn the_mascot_watches_after_the_quiet_and_on_a_sped_span_and_never_on_a_card() {
    let quiet = mascot::QUIET_MS * video::FPS / 1000;
    let h = Hands::new(&[(0, Side::Left, Press::Key)]);
    assert_eq!((h.pose(quiet - 1, false), h.pose(quiet, false)), (Pose::Up, Pose::Watch));
    assert_eq!(Hands::new(&[]).pose(10, false), Pose::Watch, "no key yet");
    // (typing again: the first key is a tap)
    let h = Hands::new(&[(0, Side::Left, Press::Key), (5_000_000, Side::Right, Press::Key)]);
    assert_eq!(h.pose(Program::frame_at(5_000_000) - 1, false), Pose::Watch);
    assert_eq!(h.pose(Program::frame_at(5_000_000), false), Pose::Right);
    // (on a sped span watching at once, a key a moment before; at 1× and at the 2× fit not)
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let near = |t: u64| {
        let v = p.video_of(c.map.video(t)).unwrap();
        let h = Hands::new(&[(v - 200_000, Side::Left, Press::Key)]);
        mascot::pose_at(&h, Program::frame_at(v), &p.frame(Program::time_of(Program::frame_at(v))))
    };
    assert_eq!(
        (near(5_000), near(15_000), near(53_000)),
        (Some(Pose::Watch), Some(Pose::Up), Some(Pose::Up))
    );
    // (a card: none)
    let h = Hands::new(&[(4_000_000, Side::Both, Press::Key)]);
    let built = Program::frame_at(4_000_000);
    assert_eq!(p.frame(Program::time_of(built)).card, Some(Card::Built));
    assert_eq!(mascot::pose_at(&h, built, &p.frame(Program::time_of(built))), None);
    let second = p.parts.iter().find(|x| x.what == What::Chapter(1)).unwrap();
    assert_eq!(mascot::pose_at(&h, Program::frame_at(second.at), &p.frame(second.at + 1)), None);
}

/// Five made-up poses at the box's size, their ink from column `ink` in rows 20 on.
fn art(ink: usize) -> Art {
    art_at(ink, video::Scale::HD)
}

/// `art` at `s`: its box, its ink's column and first row `s` times.
fn art_at(ink: usize, s: video::Scale) -> Art {
    let (w, h) = mascot::box_at(s);
    let mut px = vec![0u8; w * h * 4];
    for y in s.px(20)..h {
        for x in s.px(ink)..w {
            px[(y * w + x) * 4..][..4].copy_from_slice(&[200, 200, 200, 255]);
        }
    }
    Art {
        pics: vec![Some((w, h, px)); mascot::Pic::COUNT],
        dir: "made-up".into(),
        scale: s,
        marks: vec![None; 3],
    }
}

/// The layout's figures at 2160p are 1080p's doubled, by the same constants (`Scale`, never a second set): the picture,
/// the screen's picture and place at each size a take may be, the band's rule and middle, the renderer's margin — and
/// the renderer draws a screen at 2160p the size the layout says; the mascot's box and place; the intro's swing.
#[test]
fn the_layout_at_2160p_is_1080ps_doubled() {
    use super::draw::layout;
    use video::Scale;
    let fonts = crate::shot::Fonts::load(1).unwrap();
    let two = |(a, b): (usize, usize)| (2 * a, 2 * b);
    for size in [(166, 32), (182, 36), (182, 33), (80, 24)] {
        let (a, b) = (layout(&fonts, size, Scale::HD).unwrap(), layout(&fonts, size, Scale::UHD).unwrap());
        assert_eq!(a.dims, (1920, 1080));
        assert_eq!(
            (b.dims, b.pic, b.at, b.rule, b.mid, b.pad, b.glyph),
            (two(a.dims), two(a.pic), two(a.at), 2 * a.rule, 2 * a.mid, 2 * a.pad, a.glyph),
            "{size:?}"
        );
        let blank = vt100::Parser::new(size.1, size.0, 0);
        let drawn = |s: Scale| {
            let p = crate::shot::render_scaled(blank.screen(), &fonts, a.glyph, s.k());
            (p.0, p.1)
        };
        assert_eq!((drawn(Scale::HD), drawn(Scale::UHD)), (a.pic, b.pic), "{size:?}: the renderer's picture");
        // (the mascot under that screen)
        let bottom = |l: &super::draw::Layout| l.at.1 + l.pic.1 - l.pad;
        let p1 = mascot::place(&art_at(60, Scale::HD), (a.at.0, a.pic.0), bottom(&a), true);
        let p2 = mascot::place(&art_at(60, Scale::UHD), (b.at.0, b.pic.0), bottom(&b), true);
        match (p1, p2) {
            (Ok(p1), Ok(p2)) => assert_eq!(
                (p2.x, p2.y, p2.rule_end, p2.marks_right),
                (2 * p1.x, 2 * p1.y, 2 * p1.rule_end, 2 * p1.marks_right),
                "{size:?}: the mascot's place"
            ),
            (Err(e1), Err(e2)) => {
                let px = |e: &str| e.split_whitespace().find_map(|w| w.parse::<usize>().ok());
                assert_eq!(px(&e2), px(&e1).map(|n| 2 * n), "{size:?}: {e1} / {e2}");
            }
            (p1, p2) => panic!("{size:?}: the mascot placed at one size only: {p1:?} {p2:?}"),
        }
    }
    assert_eq!(mascot::box_at(Scale::UHD), two(mascot::BOX));
    // (the swing: made at each size from its source, drawn twice as tall — where main's frames are there)
    let (bg, look) = (Bg::Dark, video::look(Bg::Dark));
    if let (Ok(i1), Ok(i2)) = (
        video::WideIntro::new(bg, &look, &fonts, Scale::HD),
        video::WideIntro::new(bg, &look, &fonts, Scale::UHD),
    ) {
        let ((n1, d1, ink1), (n2, d2, ink2)) = (i1.drawn(), i2.drawn());
        assert_eq!((i2.made, n2, d2.1), (two(i1.made), n1, 2 * d1.1), "the swing as made and drawn");
        assert!(d2.0.abs_diff(2 * d1.0) <= 1, "its width: {d1:?} {d2:?}");
        assert!(ink2.0.abs_diff(2 * ink1.0) <= 2 && ink2.1.abs_diff(2 * ink1.1) <= 2, "{ink1:?} {ink2:?}");
        assert_eq!(i2.frame(INTRO_MS, &look).len(), 3840 * 2160 * 3);
    }
}

/// `big` (a 2160p frame) halved against `small` (1080p's) over 1080p's `rows`, where either has ink (off `ground` by
/// more than 24): the mean difference of a channel (of 255), the share of pixels a channel of which is more than `FAR`
/// apart, and the box each one's ink covers (columns, rows).
fn halved_against(
    big: &[u8],
    small: &[u8],
    rows: std::ops::Range<usize>,
    ground: crate::shot::Rgb,
) -> (f64, f64, [usize; 4], [usize; 4]) {
    const FAR: u8 = 96;
    let (w, h) = (video::W, video::H);
    let half = video::half(big, (2 * w, 2 * h)).2;
    let (mut sum, mut far, mut n) = (0u64, 0u64, 0u64);
    let mut boxes = [[usize::MAX, usize::MAX, 0, 0]; 2];
    for y in rows {
        for x in 0..w {
            let (a, b) = (&half[(y * w + x) * 3..][..3], &small[(y * w + x) * 3..][..3]);
            let inked = |p: &[u8]| p.iter().zip(ground).any(|(c, g)| c.abs_diff(g) > 24);
            for (k, _) in [a, b].into_iter().enumerate().filter(|(_, p)| inked(p)) {
                let bx = &mut boxes[k];
                *bx = [bx[0].min(x), bx[1].min(y), bx[2].max(x), bx[3].max(y)];
            }
            if !inked(a) && !inked(b) {
                continue;
            }
            let d: Vec<u8> = a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).collect();
            sum += d.iter().map(|&d| u64::from(d)).sum::<u64>();
            far += u64::from(d.iter().any(|&d| d > FAR));
            n += 1;
        }
    }
    (sum as f64 / (3 * n.max(1)) as f64, far as f64 / n.max(1) as f64, boxes[0], boxes[1])
}

/// A frame at 2160p halved is the 1080p frame, within what drawing glyphs at twice their size then averaging them
/// allows: at the cells (a screen of code, colours, a selected row, box lines), the band (the clock, the speed's mark, a
/// caption, the rule) and a card (what's built: all drawn text) — the same ink in the same place to a pixel, a small
/// mean difference where there's ink, few pixels far apart (a glyph's edge); the rule two rows thick. (Measured
/// 2026-11-11: means 0.35 at the cells, 0.49–0.77 at the band, 1.37 on the card; far 0.06 % at most.)
#[test]
fn a_frame_at_2160p_halved_is_the_1080p_frame() {
    use super::draw::{Stage, Words, layout};
    use video::Scale;
    let size = (166, 32);
    let mut screen = vt100::Parser::new(size.1, size.0, 0);
    screen.process(
        b"\x1b[?25l\x1b[48;2;11;14;20m\x1b[2J\x1b[1;1H\x1b[38;2;230;180;80mfn\x1b[38;2;191;189;182m main() {",
    );
    screen.process(b"\x1b[2;5H\x1b[38;2;170;217;76mlet\x1b[38;2;191;189;182m x = \x1b[38;2;149;230;203m\"a task\"\x1b[38;2;191;189;182m;");
    screen.process(b"\x1b[3;1H\x1b[48;2;34;40;52m\x1b[1m  selected row, bold  \x1b[0m\x1b[48;2;11;14;20m");
    screen.process(
        "\x1b[5;100H\x1b[38;2;81;84;92m╭────────╮\x1b[6;100H│ chat │\x1b[7;100H╰────────╯".as_bytes(),
    );
    screen.process(b"\x1b[31;1H\x1b[38;2;92;103;115mtest result: ok. 3 passed");
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let look = video::look(Bg::Dark);
    let frames =
        [("the clock and the speed", at_take(&p, 5_000, 0)), ("a caption", at_take(&p, 12_000, 600))];
    let drawn = |s: Scale, f: &Frame| {
        let fonts = crate::shot::Fonts::load(s.k()).unwrap();
        let lay = layout(&fonts, size, s).unwrap();
        let pic = crate::shot::render_scaled(screen.screen(), &fonts, lay.glyph, s.k());
        let stage = Stage::new(lay, &look, &c, None);
        let none = (None, &mascot::pops::Pops::default(), &[][..]);
        (stage.take_frame(&pic, None, f, none, &mut Words::new(&fonts, look.ground, s)), lay)
    };
    for (what, f) in &frames {
        assert!(f.clock.is_some(), "{what}: {f:?}");
        let ((small, lay), (big, _)) = (drawn(Scale::HD, f), drawn(Scale::UHD, f));
        for (part, rows) in [("the cells", 0..lay.rule - 1), ("the band", lay.rule..video::H)] {
            within(&format!("{what}, {part}"), halved_against(&big, &small, rows, look.ground));
        }
        let (x, w) = (lay.at.0 + 2 * lay.pad, 2 * video::W);
        for y in [2 * lay.rule, 2 * lay.rule + 1] {
            assert_eq!(&big[(y * w + 2 * x) * 3..][..3], &look.faint, "{what}: the rule's row {y} at 2160p");
        }
    }
    let card = |s: Scale| {
        let fonts = crate::shot::Fonts::load(s.k()).unwrap();
        let told = "a real session: 1h 27m of work, shown in 10m 03s";
        super::opening::card(&c.opening, told, Bg::Dark, &look, &fonts, s)
    };
    within("the card", halved_against(&card(Scale::UHD), &card(Scale::HD), 0..video::H, look.ground));
}

/// `halved_against`'s figures within what resampling allows: the ink in the same place to a pixel, a mean difference of
/// at most 3 of 255, at most 0.5 % of pixels far apart.
fn within(what: &str, (mean, far, big, small): (f64, f64, [usize; 4], [usize; 4])) {
    let said = format!("{what}: mean {mean:.2}, far {:.3} %, ink {big:?} against {small:?}", far * 100.0);
    assert!(big.iter().zip(small).all(|(a, b)| a.abs_diff(b) <= 1), "the ink's place — {said}");
    assert!(mean <= 3.0 && far <= 0.005, "{said}");
}

#[test]
fn the_band_s_marks_and_captions_stay_clear_of_the_mascot_and_it_never_covers_a_cell() {
    use crate::shot::PAD;
    let (x0, pw) = (33, video::W - 66);
    let place = mascot::place(&art(60), (x0, pw), 870, false).unwrap();
    let right = x0 + pw - PAD;
    let (y, bottom) = (video::H - 14 - mascot::BOX.1, video::H - 14);
    assert_eq!(
        (place.x + mascot::BOX.0, place.y),
        (right, y),
        "its right on the rule's end, its foot over the frame's"
    );
    assert_eq!(place.rule_end, place.x + 60 - 26, "the rule stops short of its ink");
    assert_eq!(place.marks_right - PAD + mascot::GAP, place.x);
    assert!(y < video::H - video::BAND && bottom < video::H, "its head over the rule");
    let e = mascot::place(&art(60), (x0, pw), y + 49, false).unwrap_err();
    assert!(e.contains("49 px over the screen's last row of cells"), "{e}");
    // (the marks drawn: ink left of the gap, none from it to the frame's edge — a sped span's, where it ends')
    let fonts = crate::shot::Fonts::load(1).unwrap();
    let look = video::look(Bg::Dark);
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let lay = super::draw::layout(&fonts, (166, 32), video::Scale::HD).unwrap();
    for f in [at_take(&p, 5_000, 0), p.frame(7_000_000)] {
        let mut px = look.ground.repeat(video::W * video::H);
        let mut words = super::draw::Words::new(&fonts, look.ground, video::Scale::HD);
        super::draw::band(&mut px, &lay, &f, &c, (x0, place.marks_right), &look, &mut words);
        let inked = |xs: std::ops::Range<usize>| {
            (video::H - video::BAND..video::H)
                .any(|y| xs.clone().any(|x| px[(y * video::W + x) * 3..][..3] != look.ground))
        };
        assert!(inked(place.x - mascot::GAP - 60..place.x - mascot::GAP), "the mark drawn: {f:?}");
        assert!(!inked(place.x - mascot::GAP..video::W), "clear of the box: {f:?}");
    }
    // (`CAPTION_MAX`'s reason: under the widest speed mark — seven characters, `▸▸ ×1.3` — with the pops' lane, a
    // centred caption has room for `CAPTION_MAX` characters and not one more)
    let widest = sped(1.3).unwrap();
    for i in 11..1000 {
        let k = f64::from(i) / 10.0;
        assert!(sped(k).unwrap().chars().count() <= widest.chars().count(), "{k}");
    }
    let laned = mascot::place(&art(60), (x0, pw), 870, true).unwrap();
    let room = super::draw::caption_room(&fonts, laned.marks_right, Some(&widest));
    let width = |n: usize| super::draw::letters(&"x".repeat(n), video::CAPTION_PX, &fonts);
    assert!(width(CAPTION_MAX) <= room && room < width(CAPTION_MAX + 1), "{room} px");
    // (a caption: clear of the widest speed mark over it, else of the box; in the band with the lane, one of
    // `CAPTION_MAX` anywhere)
    let boxed = (laned.marks_right, true);
    assert_eq!(super::draw::captions_clear(&c, &fonts, boxed), Ok(()));
    let caption =
        |at: &str| format!("[[caption]]\nat = \"{at}\"\nfor = \"1s\"\ntext = \"{}\"\n", "x".repeat(40));
    let text = (CUT.replacen("[[caption]]", &format!("{}\n[[caption]]", caption("2s")), 1)).replacen(
        "text = \"A card.\"\n",
        &format!("text = \"A card.\"\n\n{}", caption("52s")),
        1,
    );
    let two = cut(&text).unwrap();
    assert_eq!(super::draw::captions_clear(&two, &fonts, boxed), Ok(()));
    // (a band with less room — its marks ending 100 px nearer —: both that reach their marks named in one refusal,
    // each at its line, before any frame; the one that fits not)
    let e = super::draw::captions_clear(&two, &fonts, (laned.marks_right - 100, true)).unwrap_err();
    let lines: Vec<usize> = (text.lines().enumerate())
        .filter(|(_, l)| l.starts_with("[[caption]]"))
        .map(|(i, _)| i + 1)
        .collect();
    assert_eq!(lines.len(), 3);
    assert!(e.starts_with("2 captions don't fit the band:\n"), "{e}");
    for (line, mark) in [(lines[0], "▸▸ ×4"), (lines[2], "▸▸ ×2")] {
        let said = format!(
            "\n  cut.toml:{line}: caption: 800 px wide, centred it reaches the speed's mark `{mark}`"
        );
        assert!(e.contains(&said), "{said}\n  in: {e}");
    }
    assert!(!e.contains(&format!("cut.toml:{}:", lines[1])), "{e}");
}

/// A caption is one line of `CAPTION_MAX` (40) characters at most: a 41st is refused at its line as the file is read.
#[test]
fn a_caption_of_41_characters_is_refused_at_its_line() {
    let of = |n: usize| CUT.replacen("text = \"A card.\"", &format!("text = \"{}\"", "x".repeat(n)), 1);
    assert!(cut(&of(40)).is_ok());
    let text = of(41);
    let line = text.lines().position(|l| l.starts_with("text = \"xxx")).unwrap() + 1;
    let e = cut(&text).unwrap_err();
    assert!(
        e.starts_with(&format!("cut.toml:{line}: caption: one line of 1 to 40 characters (41 here)")),
        "{e}"
    );
    assert_eq!(CAPTION_MAX, 40);
}

#[test]
fn no_frames_no_mascot_and_it_says_so() {
    let c = cut(CUT).unwrap();
    let s = tarae_testkit::scratch::Scratch::new("xtask-cut-mascot");
    let hd = video::Scale::HD;
    let e = mascot::found(&c, Some(s.join("none")), (33, 1854), 870, hd).err().unwrap();
    assert!(e.starts_with("the mascot: none — ") && e.ends_with("; the cut is made without it"), "{e}");
    assert!(e.contains("frame-up.png"), "{e}");
    let e = mascot::found(&c, None, (33, 1854), 870, hd).err().unwrap();
    assert!(e.contains("no main checkout found"), "{e}");
    let off = cut(&format!("mascot = false\n{CUT}")).unwrap();
    assert_eq!(
        mascot::found(&off, Some(s.join("none")), (33, 1854), 870, hd).err().unwrap(),
        "the mascot: none (mascot = false)"
    );
    let light = cut(&format!("bg = \"light\"\n{CUT}")).unwrap();
    assert!(mascot::found(&light, None, (33, 1854), 870, hd).err().unwrap().contains("meok's only"));
    assert!(cut(&format!("mascot = 1\n{CUT}")).unwrap_err().contains("mascot: true or false"));
}

// ── The mascot's motions and its key pops ──

use super::mascot::motions::{self, Cause, Causes, Kind, Plan};
use super::mascot::pops::{self, Place, Pop, Pops, Regions};
use super::mascot::{Arms, Face, Keys, Pic, Switches};
use super::sound::Struck;

/// The made-up take at 1× all through: chapter 2's card at `front fix` (50 s); `rest` added to its list.
fn moving(rest: &str) -> String {
    format!(
        "default = 1\n{rest}\n[opening]\nname = \"halil\"\nline = \"A task manager.\"\nat = \"card 2 shown\"\nfor = \"3s\"\n\n\
         [[chapter]]\nat = \"start\"\ntitle = \"One\"\n\n[[chapter]]\nat = \"front fix\"\ntitle = \"Two\"\n"
    )
}

/// Where the take part shows the take's `ms` (µs).
fn take_at(p: &Program, ms: u64) -> u64 {
    p.video_of(p.cut.map.video(ms)).unwrap()
}

/// The motions with taps at `taps` (video µs, the left hand), seeded 7.
fn plan_of(p: &Program, taps: &[u64], causes: &Causes, busy: &[(u64, u64)], sw: Switches) -> (Plan, Hands) {
    let hands = Hands::new(&taps.iter().map(|&v| (v, Side::Left, Press::Key)).collect::<Vec<_>>());
    (Plan::new(p, &hands, causes, busy, &sw, 7), hands)
}

const STILL: Switches = Switches {
    on: true,
    blink: false,
    glance: false,
    surprise: false,
    stretch: false,
    swing: false,
    fast: false,
    keys: Keys::All,
};

/// Seconds as µs.
fn s(ms: u64) -> u64 {
    ms * 1000
}

/// Every whole-body motion clear of every key: none of its frames, from the turn before it to the hands back over the
/// keys, at a key's frame.
fn clear_of_keys(plan: &Plan, taps: &[u64]) {
    let margin = motions::frames(motions::MARGIN_MS);
    for m in plan.placed.iter().filter(|m| m.kind != Kind::Surprise) {
        for &t in taps {
            let k = Program::frame_at(t);
            assert!(
                !(m.from - motions::TURN <= k && k < m.from + m.len + margin),
                "{m:?} against a key at frame {k}"
            );
        }
    }
}

/// The motions for `p` with keys at the take's `keys` (ms) wherever `p` shows them, each with its pop up `POP_MS`: the
/// taps and the pops' frames, and the plan — as the cut lays them for a program (`draw::held_ends`' `lay`).
type Motions = ((Vec<u64>, Vec<(u64, u64)>), Option<Plan>);

fn motions_of(p: &Program, keys: &[u64], sw: Switches) -> Motions {
    let mut taps: Vec<u64> = keys.iter().flat_map(|&t| motions::shown_at(p, t)).collect();
    taps.sort_unstable();
    let life = motions::frames(mascot::pops::POP_MS);
    let busy: Vec<(u64, u64)> =
        taps.iter().map(|&v| (Program::frame_at(v), Program::frame_at(v) + life)).collect();
    let (plan, _) = plan_of(p, &taps, &motions::causes(p, &marks()), &busy, sw);
    ((taps, busy), Some(plan))
}

/// Every chapter with a card has its swing, whole and its margin before the card: a placed swing whose cause names it.
fn every_chapter_swings(p: &Program, plan: &Plan) -> usize {
    let margin = motions::frames(motions::MARGIN_MS);
    let mut n = 0;
    for (i, part) in p.parts.iter().enumerate() {
        let What::Chapter(c) = part.what else { continue };
        if !matches!(p.parts[i - 1].what, What::Take { .. } | What::Ends { .. } | What::Hold { .. }) {
            continue;
        }
        let words = format!("before chapter {}'s card", c + 1);
        let m = plan.placed.iter().find(|m| m.kind == Kind::Swing && m.cause.contains(&words));
        let m = m.unwrap_or_else(|| panic!("no swing {words}: {:#?}", plan.left_out));
        assert!(m.from + m.len + margin <= Program::frame_at(part.at), "{m:?}");
        n += 1;
    }
    n
}

/// Every chapter's end gets its brush swing (the design session's, 2026-11-12): in place where there's room; where keys
/// leave none within 2 s, its last frame held still before the card — the clock standing, no speed, no key heard, no
/// crossing into it — for the swing past the last key and its pop, and its margin, in whole tenths; `cut.txt` says them.
#[test]
fn every_chapter_with_a_card_has_its_swing_in_place_or_held() {
    let c = cut(CUT).unwrap();
    let swing = STILL.with(|s| s.swing = true);
    let holds_of = |p: &Program| p.parts.iter().filter(|x| matches!(x.what, What::Hold { .. })).count();
    // (no keys: both in place — where it ends before chapter 1's card, chapter 1's end before chapter 2's —, none held)
    let (p, _, plan, said) = super::draw::held_ends(&c, |p| motions_of(p, &[], swing), standing);
    assert_eq!((holds_of(&p), said.len()), (0, 0));
    assert_eq!(every_chapter_swings(&p, &plan.unwrap()), 2);

    // (keys every 0.4 s through both ends' last seconds: no room for either within 2 s — each needs a hold)
    let keys = keys_through_both_ends();
    let p0 = Program::new(&c);
    let (_, plan0) = motions_of(&p0, &keys, swing);
    let plan0 = plan0.unwrap();
    assert_eq!(plan0.holds.iter().map(|h| h.chapter).collect::<Vec<_>>(), [0, 1], "{:#?}", plan0.left_out);

    let (p, (taps, busy), plan, said) = super::draw::held_ends(&c, |p| motions_of(p, &keys, swing), standing);
    let plan = plan.unwrap();
    assert_eq!(every_chapter_swings(&p, &plan), 2, "held or in place, every one");
    assert!(
        plan.holds.is_empty() && !plan.left_out.iter().any(|l| l.contains("'s card")),
        "{:#?}",
        plan.left_out
    );
    clear_of_keys(&plan, &taps);
    let holds: Vec<&Part> = p.parts.iter().filter(|x| matches!(x.what, What::Hold { .. })).collect();
    assert_eq!(holds.len(), 2);
    let more: u64 = holds.iter().map(|h| h.len).sum();
    assert_eq!(p.total, p0.total + more, "the video longer by its holds");
    let crossings: Vec<u64> = p.crossings().iter().map(|c| c.0).collect();
    for h in &holds {
        assert_eq!(h.len % 100_000, 0, "whole tenths: {h:?}");
        let (a, b) = (Program::frame_at(h.at), Program::frame_at(h.at + h.len));
        assert!(!crossings.contains(&h.at), "no crossing into a hold");
        // (the frame before it, still: the clock stands, no speed; no key heard in it)
        let before = p.frame(Program::time_of(a - 1));
        for k in a..b {
            let f = p.frame(Program::time_of(k));
            assert_eq!(
                (f.screen, f.clock, f.speed, f.card),
                (before.screen, before.clock, None, None),
                "{k}"
            );
        }
        assert!(!taps.iter().any(|&v| (a..b).contains(&Program::frame_at(v))), "a key in the hold");
        // (its swing in it, the last pop gone when it starts)
        let m = plan.placed.iter().find(|m| m.kind == Kind::Swing && (a..b).contains(&m.from)).unwrap();
        assert!(m.cause.contains("held still"), "{m:?}");
        assert!(!busy.iter().any(|q| q.0 <= m.from && m.from < q.1), "a pop up at {m:?}");
    }
    assert!(
        said[0].starts_with("chapter ends held still for their swing (no room for it before the card): 2, "),
        "{said:#?}"
    );
    assert!(said[1].starts_with("  where it ends, before chapter 1's card: held "), "{said:#?}");
    assert!(said[2].starts_with("  chapter 1's end, before chapter 2's card: held "), "{said:#?}");
}

/// Every held frame's screen stands (`draw::held_ends`' `moves`).
fn standing(w: &[(u64, u64)]) -> Vec<Option<String>> {
    vec![None; w.len()]
}

/// The keys of `every_chapter_with_a_card_has_its_swing_in_place_or_held`: every 0.4 s through both of the made-up
/// cut's chapter ends' last seconds — each held for its swing.
fn keys_through_both_ends() -> Vec<u64> {
    (13_000..=19_800).step_by(400).chain((55_000..58_000).step_by(400)).collect()
}

/// A held frame must be a screen that stands (the design session's, 2026-11-12): V4b's 600 ms of video leading into it
/// (`Program::stand_window`) — only time the video shows counts, never the take after the held frame —, the replay
/// asked what came in them (`draw::moving`) — a timer ticking named by its row; `cut.txt` says `stands` or what moves
/// for each held end, and one that moves is a place in the edit list, the line of the chapter whose card follows.
#[test]
fn a_held_end_stands_or_what_moves_is_named_and_its_chapter_is_a_place_to_fix() {
    // (the window: at 4× 600 ms of video before is 2.4 s of take; at 1×, 600 ms; it ends at the frame's own screen)
    let c = cut(CUT).unwrap();
    let p0 = Program::new(&c);
    assert_eq!(p0.stand_window(take_at(&p0, 8_000)), Some((5_600, 8_000)), "at 4×");
    assert_eq!(p0.stand_window(take_at(&p0, 15_000)), Some((14_400, 15_000)), "at 1×");

    // (the replay: a timer ticking on row 2 every 100 ms from 20 s; the windows out of order, one going back)
    let at = |ms: u64, bytes: &str| Rec { us: ms * 1000, whose: SCREEN, bytes: bytes.as_bytes().to_vec() };
    let mut recs = vec![at(0, "\x1b[H\x1b[2J\x1b[?25lhalil board\r\nidle")];
    recs.extend((0..30).map(|i| at(20_000 + i * 100, &format!("\x1b[2;1Hrun {}.{}s", i / 10, i % 10))));
    let take = Take { about: Value::Null, size: (20, 3), screen: recs };
    assert_eq!(
        super::draw::moving(&take, &[(19_500, 21_000), (5_000, 6_200), (20_500, 20_700), (22_900, 23_500)]),
        [
            Some("11 screens in 1.5 s of take: row 2 “run 0.0s”".to_string()),
            None,
            Some("2 screens in 0.2 s of take: row 2 “run 0.6s”".to_string()),
            None,
        ]
    );

    // (the made-up cut's two held ends: where it ends stands, chapter 1's end moves)
    let swing = STILL.with(|s| s.swing = true);
    let keys = keys_through_both_ends();
    let mut asked = Vec::new();
    let moves = |w: &[(u64, u64)]| {
        asked = w.to_vec();
        vec![None, Some("12 screens in 1.2 s of take: row 2 “run 0.3s”".to_string())]
    };
    let (p, _, _, said) = super::draw::held_ends(&c, |p| motions_of(p, &keys, swing), moves);
    let held: Vec<(u64, u64)> = (p.parts.iter())
        .filter_map(|x| match x.what {
            What::Hold { v } => p.stand_window(v),
            _ => None,
        })
        .collect();
    assert_eq!((asked.len(), &asked), (2, &held), "each held frame's window asked");
    assert!(said[0].ends_with("; 1 on a screen that stands, 1 on one that moves"), "{said:#?}");
    assert!(said[1].starts_with("  where it ends, before chapter 1's card: held "), "{said:#?}");
    assert!(said[1].ends_with(" — stands"), "{said:#?}");
    assert!(said[2].starts_with("  chapter 1's end, before chapter 2's card: held "), "{said:#?}");
    assert!(said[2].ends_with(" — moves — 12 screens in 1.2 s of take: row 2 “run 0.3s”"), "{said:#?}");
    let place = format!("cut.toml:{}: chapter 1's end is held ", c.chapters[1].line);
    assert_eq!(said.len(), 4, "{said:#?}");
    assert!(said[3].starts_with(&place), "{said:#?}");
    assert!(
        said[3].contains("on a screen that moves (12 screens")
            && said[3].ends_with("end the chapter where its screen stands"),
        "{said:#?}"
    );
}

/// For a hold, only time the video shows counts (the design session, 2026-11-12): the take's screen changing only
/// after each held frame — a cursor moving 0.1 s later, a line 0.5 s later, never seen — leaves both of the made-up
/// cut's held ends `stands`; the same change 0.1 s before one of them is named.
#[test]
fn a_change_after_a_held_frame_never_seen_leaves_it_standing() {
    let c = cut(CUT).unwrap();
    let swing = STILL.with(|s| s.swing = true);
    let keys = keys_through_both_ends();
    let lay = |p: &Program<'_>| motions_of(p, &keys, swing);
    let (p, _, _, _) = super::draw::held_ends(&c, lay, |w| vec![None; w.len()]);
    // (each held frame's own screen, the take's ms)
    let ends: Vec<u64> = (p.parts.iter())
        .filter_map(|x| match x.what {
            What::Hold { v } => p.frame(v).screen,
            _ => None,
        })
        .collect();
    assert_eq!(ends.len(), 2);
    let at = |ms: u64, bytes: &str| Rec { us: ms * 1000, whose: SCREEN, bytes: bytes.as_bytes().to_vec() };
    let take_with = |at_ms: &[u64]| {
        let mut recs = vec![at(0, "\x1b[H\x1b[2J\x1b[?25lhalil board\r\nidle")];
        recs.extend(at_ms.iter().enumerate().map(|(i, &ms)| at(ms, &format!("\x1b[3;1Hline {i}"))));
        recs.sort_by_key(|r| r.us);
        Take { about: Value::Null, size: (20, 3), screen: recs }
    };
    let after = take_with(&ends.iter().flat_map(|&t| [t + 100, t + 500]).collect::<Vec<_>>());
    let (_, _, _, said) = super::draw::held_ends(&c, lay, |w| super::draw::moving(&after, w));
    assert!(said[0].ends_with("; 2 on a screen that stands, 0 on one that moves"), "{said:#?}");
    assert!(said[1].ends_with(" — stands") && said[2].ends_with(" — stands") && said.len() == 3, "{said:#?}");
    let before = take_with(&[ends[1] - 100]);
    let (_, _, _, said) = super::draw::held_ends(&c, lay, |w| super::draw::moving(&before, w));
    assert!(said[0].ends_with("; 1 on a screen that stands, 1 on one that moves"), "{said:#?}");
    assert!(said[2].contains(" — moves — 1 screen in "), "{said:#?}");
}

/// The bed plays through a hold as through any of the video: each of its samples the file's next, no gap and none
/// again; and the sound is the video's length, its holds in it.
#[test]
fn a_bed_plays_through_a_hold_with_no_gap_or_repeat_and_the_sound_is_the_videos_length() {
    let c = cut(&bedded()).unwrap();
    let p0 = Program::new(&c);
    let hold = Hold { chapter: 1, frames: 4 * HOLD_STEP };
    let p = Program::held(&c, &[hold]);
    let h = *p.parts.iter().find(|x| matches!(x.what, What::Hold { .. })).unwrap();
    let card = *p.parts.iter().find(|x| x.what == What::Chapter(1)).unwrap();
    assert_eq!((h.at + h.len, p.total), (card.at, p0.total + hold.us()));
    let len = track_len(&p);
    assert_eq!(len, sample_at(p.total), "the sound the video's length");
    // (the first bed: in under the intro, out over chapter 2's card — after the hold)
    let n = sample_at(60_000_000);
    let (placed, _) = layout(&p, &[(n, 0), (n, 0)]);
    assert!(placed[0].fade_in.1 < h.at && placed[0].fade_out.0 == card.at, "{:?}", placed[0]);
    // (a file whose every sample is its own number: through the hold, each sample the one after the last)
    let track: Vec<f32> = (0..n).map(|i| i as f32).collect();
    let mut out = vec![0.0f32; len];
    lay(&placed[0], &track, None, 1.0, &mut out);
    let s0 = sample_at(placed[0].start);
    for k in sample_at(h.at) - RATE as usize..sample_at(card.at) {
        assert_eq!(out[k], track[placed[0].from + k - s0], "sample {k}");
    }
}

/// A chapter's card is a full break (the design session's, 2026-11-12): a stretch after it needs 6 s of the new chapter
/// with no key — the quiet never counted through the end of the chapter before and the card.
#[test]
fn a_stretch_after_a_chapter_card_needs_six_seconds_of_the_new_chapter() {
    let c = cut(&moving("")).unwrap();
    let p = Program::new(&c);
    let card = *p.parts.iter().find(|x| x.what == What::Chapter(1)).unwrap();
    // (keys every 0.4 s to 4 s before chapter 2's card: that quiet runs into the card, short of a stretch; none after)
    let keys: Vec<u64> = (0..=46_000).step_by(400).map(|t| take_at(&p, t)).collect();
    let stretch = STILL.with(|s| s.stretch = true);
    let (pl, _) = plan_of(&p, &keys, &Causes::default(), &[], stretch);
    let after = Program::frame_at(card.at + card.len);
    let first = pl.placed.iter().find(|m| m.kind == Kind::Stretch).map(|m| m.from);
    assert_eq!(first, Some(after + motions::frames(motions::STRETCH_QUIET_MS)), "{:#?}", pl.placed);
}

/// The 30 s between whole-body motions counts within a chapter: a stretch 10–20 s before its own chapter's end's swing
/// is refused (said), one 9 s after it, past the card, is placed.
#[test]
fn a_stretch_keeps_thirty_seconds_from_its_own_chapters_end_swing_and_none_across_the_card() {
    let c = cut(&moving("")).unwrap();
    let p = Program::new(&c);
    let card = *p.parts.iter().find(|x| x.what == What::Chapter(1)).unwrap();
    // (keys every 0.4 s but for 30–40 s and the end's last 3 s; after the card, none for 10 s)
    let keys: Vec<u64> = ((0..30_000).step_by(400))
        .chain((40_000..47_000).step_by(400))
        .chain((60_000..120_000).step_by(400))
        .map(|t| take_at(&p, t))
        .collect();
    let sw = STILL.with(|s| {
        s.stretch = true;
        s.swing = true
    });
    let (pl, _) = plan_of(&p, &keys, &motions::causes(&p, &marks()), &[], sw);
    let swing = pl.placed.iter().find(|m| m.kind == Kind::Swing && m.cause.starts_with("chapter 1's end"));
    let swing = swing.unwrap_or_else(|| panic!("chapter 1's end swings: {:#?}", pl.left_out));
    let stretches: Vec<u64> = pl.placed.iter().filter(|m| m.kind == Kind::Stretch).map(|m| m.from).collect();
    let at = |t: u64| Program::frame_at(take_at(&p, t));
    assert!(!stretches.iter().any(|&k| k < swing.from), "none in chapter 1: {:#?}", pl.placed);
    let refused = pl.unstretched.iter().find(|u| (at(29_000)..at(31_000)).contains(&u.0));
    assert!(
        refused.is_some_and(|u| u.2 == format!("the swing at {}", motions::secs(swing.from))),
        "{:#?}",
        pl.unstretched
    );
    let after = Program::frame_at(card.at + card.len);
    assert_eq!(
        stretches.first(),
        Some(&(after + motions::frames(motions::STRETCH_QUIET_MS))),
        "{:#?}",
        pl.placed
    );
}

#[test]
fn a_whole_body_motion_fits_whole_before_the_next_key_or_is_never_begun() {
    let c = cut(&moving("")).unwrap();
    let p = Program::new(&c);
    let v0 = take_at(&p, 0);
    let frame = Program::frame_at;
    let stretch = STILL.with(|s| s.stretch = true);
    // (a wait that turns out too short: the stretch 10 s in would end 0.1 s short of the margin before the key — never
    // begun; 10 s after the key it is, whole, and the hands come back over the keys)
    let quiet = motions::STRETCH_QUIET_MS;
    let key = v0 + s(quiet + 1_400);
    let (pl, _) = plan_of(&p, &[key], &Causes::default(), &[], stretch);
    // (and one more past chapter 2's card, in its own quiet)
    let froms: Vec<u64> = pl.placed.iter().map(|m| m.from).collect();
    let at = frame(key) + motions::frames(motions::STRETCH_QUIET_MS);
    assert_eq!(froms.first(), Some(&at), "{:?}", pl.placed);
    assert!(
        froms[1..]
            .iter()
            .all(|&k| Program::time_of(k) > p.parts.iter().find(|x| x.what == What::Chapter(1)).unwrap().at)
    );
    clear_of_keys(&pl, &[key]);
    let len = pl.placed[0].len;
    assert_eq!(
        (pl.pic(at), pl.pic(at + len - 1), pl.pic(at + len)),
        (Some(Pic::Stretch(0)), Some(Pic::Stretch(4)), Some(Pic::At(Arms::Up, Face::Open)))
    );
    // (a key 0.4 s further on: the first one fits)
    let (pl, _) = plan_of(&p, &[v0 + s(quiet + 1_800)], &Causes::default(), &[], stretch);
    assert_eq!(
        pl.placed.first().map(|m| m.from),
        Some(frame(v0) + motions::frames(motions::STRETCH_QUIET_MS))
    );

    // (the swing before chapter 2's card: alone, at its moment — its end the margin before the card)
    let swing = STILL.with(|s| s.swing = true);
    let cs = motions::causes(&p, &marks());
    let card2 = p.parts.iter().find(|x| x.what == What::Chapter(1)).unwrap().at;
    let ideal = cs.swing.iter().find(|c| c.said.starts_with("chapter 1's end")).unwrap().v;
    assert_eq!(ideal, card2 - s(1_200));
    let mine = |pl: &Plan| pl.placed.iter().find(|m| m.cause.starts_with("chapter 1's end")).cloned();
    let (pl, _) = plan_of(&p, &[], &cs, &[], swing);
    assert_eq!(mine(&pl).map(|m| (m.from, m.moved)), Some((frame(ideal), 0)));
    // (a key in its way: moved before it, whole)
    let key = card2 - s(900);
    let (pl, _) = plan_of(&p, &[key], &cs, &[], swing);
    let m = mine(&pl).unwrap();
    assert!(m.moved < 0 && m.moved >= -(motions::frames(motions::SWING_MOVE_MS) as i64), "{m:?}");
    clear_of_keys(&pl, &[key]);
    // (keys all round it: left out, and said)
    let keys: Vec<u64> = (0..12).map(|i| card2 - s(5_500) + s(500) * i).collect();
    let (pl, _) = plan_of(&p, &keys, &cs, &[], swing);
    assert_eq!(mine(&pl), None);
    assert!(
        pl.left_out.iter().any(|l| l.starts_with("swing left out") && l.contains("chapter 1's end")),
        "{:?}",
        pl.left_out
    );
    assert!(pl.said(&swing).iter().any(|l| l.contains("swing left out")));
    clear_of_keys(&pl, &keys);
    // (a bring's swing waits for the key pops to go, its trail beside their lane — later, the nearest; a second in 30 s
    // has no room)
    let bring = |v: u64| Cause::at(v, 0, "a bring landed: fix".into());
    let at = v0 + s(10_000);
    let two = Causes { surprise: Vec::new(), swing: vec![bring(at), bring(at + s(10_000))] };
    let tap = Program::time_of(frame(at) - 5);
    let (pl, _) = plan_of(&p, &[tap], &two, &[(frame(at) - 5, frame(at) + 10)], swing);
    assert_eq!(pl.placed.iter().map(|m| (m.from, m.moved)).collect::<Vec<_>>(), [(frame(at) + 10, 10)]);
    assert!(pl.left_out.iter().any(|l| l.contains("(the swing at ")), "{:?}", pl.left_out);
}

/// A whole-body motion waits for a stamp's paw to come up, held longer than a key's: in place and in a held end.
#[test]
fn a_whole_body_motion_waits_for_a_stamps_paw_to_come_up() {
    let c = cut(&moving("")).unwrap();
    let p = Program::new(&c);
    let swing = STILL.with(|s| s.swing = true);
    let cs = motions::causes(&p, &marks());
    let ideal = cs.swing.iter().find(|c| c.said.starts_with("chapter 1's end")).unwrap().v;
    let mine =
        |pl: &Plan| pl.placed.iter().find(|m| m.cause.starts_with("chapter 1's end")).cloned().unwrap();
    // (a tap three frames before the swing's moment: a key's hand is up by then, a stamp's still down)
    let f = Program::frame_at(ideal) - 3;
    let with = |press: Press| {
        let hands = Hands::new(&[(Program::time_of(f), Side::Right, press)]);
        Plan::new(&p, &hands, &cs, &[], &swing, 7)
    };
    assert_eq!(mine(&with(Press::Key)).moved, 0);
    let m = mine(&with(Press::Stamp));
    let margin = motions::frames(motions::MARGIN_MS);
    assert!(m.moved != 0, "{m:?}");
    assert!(m.from >= f + Press::Stamp.hold() || m.from + m.len + margin <= f, "{m:?} over the paw from {f}");

    // (a held end: past the last stamp's paw coming up, not a key's)
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let mut taps: Vec<u64> =
        keys_through_both_ends().iter().flat_map(|&t| motions::shown_at(&p, t)).collect();
    taps.sort_unstable();
    let cs = motions::causes(&p, &marks());
    let holds = |press: Press| {
        let hands = Hands::new(&taps.iter().map(|&v| (v, Side::Left, press)).collect::<Vec<_>>());
        Plan::new(&p, &hands, &cs, &[], &swing, 7).holds
    };
    // (the last key's hand is up a frame or more before each card; the stamp's paw past it — longer, by whole tenths)
    let (keys, stamps) = (holds(Press::Key), holds(Press::Stamp));
    assert_eq!(keys.len(), 2);
    for (k, st) in keys.iter().zip(&stamps) {
        assert_eq!(k.chapter, st.chapter);
        assert!(st.frames > k.frames, "{k:?} {st:?}");
    }
}

#[test]
fn a_failed_run_surprises_once_in_twenty_seconds_and_a_passed_one_never() {
    for (run, step, want) in [
        ("claude's run: cargo build", "exit 101", true),
        ("claude's tests: src/due.rs", "failed, exit 101", true),
        ("preflight: fmt, clippy, test", "exit 1", true),
        ("tests: src/task.rs", "failed", true),
        ("task: build", "exit 2", true),
        ("claude's run again: cargo test", "exit 1", true),
        ("claude's run: cargo build", "exit 0", false),
        ("claude's tests: src/task.rs", "passed", false),
        ("claude's run: cargo build --offline", "not run: ~/.cargo/registry is a place no card opens", false),
        ("claude's run: cargo fmt --check", "dropped before its end", false),
        ("tarae's git: git rebase main", "exit 1", false),
        ("restart's build: cargo build", "exit 101", false),
        ("install: rust-analyzer", "exit 1", false),
    ] {
        assert_eq!(motions::fails(run, step), want, "{run} — {step}");
    }
    let ended = |ms: u64, run: &str, step: &str| json!({ "what": "run-ended", "ms": ms, "id": format!("run#{ms}"), "run": run, "step": step });
    let mut list = marks().list;
    list.extend([
        ended(15_000, "claude's tests: src/due.rs", "failed, exit 101"),
        ended(16_000, "claude's run: cargo build", "exit 101"), // within 20 s of it: none
        ended(17_000, "claude's tests: src/task.rs", "passed"),
        ended(18_000, "tarae's git: git rebase main", "exit 1"),
        ended(40_000, "claude's run: cargo build", "exit 101"), // keys inside it: the wide face
        ended(65_000, "claude's run: cargo build", "exit 101"), // left out: nothing
        ended(100_000, "preflight: fmt, clippy, test", "exit 1"), // at 8×: still its half second
    ]);
    let m = Marks::new(list, 120_000);
    let text = moving(
        "\n[[span]]\nfrom = \"60s\"\nto = \"70s\"\nout = true\n\n[[span]]\nfrom = \"80s\"\nto = \"120s\"\nspeed = 8\n",
    );
    let c = Cut::parse(&text, "cut.toml", &m).unwrap();
    let p = Program::new(&c);
    let cs = motions::causes(&p, &m);
    assert_eq!(cs.surprise.iter().map(|c| c.t).collect::<Vec<_>>(), [15_000, 16_000, 40_000, 100_000]);
    let typing: Vec<u64> = (0..10).map(|i| take_at(&p, 39_800) + s(100) * i).collect();
    let (pl, _) = plan_of(&p, &typing, &cs, &[], STILL.with(|s| s.surprise = true));
    let placed: Vec<(u64, bool)> = pl.placed.iter().map(|m| (m.t, m.typing)).collect();
    assert_eq!(placed, [(15_000, false), (40_000, true), (100_000, false)]);
    assert!(
        pl.left_out.iter().any(|l| l.contains("within 20 s") && l.contains("cargo build")),
        "{:?}",
        pl.left_out
    );
    let k = |ms: u64| Program::frame_at(take_at(&p, ms));
    assert_eq!((pl.pic(k(15_000)), pl.pic(k(15_000) + 14)), (Some(Pic::Surprise(0)), Some(Pic::Surprise(1))));
    assert!(
        matches!(pl.pic(k(40_000)), Some(Pic::At(Arms::Left | Arms::Up, Face::Wide))),
        "{:?}",
        pl.pic(k(40_000))
    );
    assert_eq!(pl.pic(k(100_000)), Some(Pic::Surprise(0)));
    assert!(p.frame(take_at(&p, 100_000)).speed.is_some_and(|x| x > 2.0));
    let said = pl.said(&STILL.with(|s| s.surprise = true));
    assert!(said.iter().any(|l| l.contains("surprise at ") && l.contains("failed, exit 101")), "{said:?}");
}

#[test]
fn the_blinks_and_glances_are_seeded_by_the_list_the_same_cut_the_same_video() {
    let c = cut(&moving("")).unwrap();
    let p = Program::new(&c);
    let v0 = take_at(&p, 0);
    // (a typing streak of 10 s)
    let taps: Vec<u64> = (0..67).map(|i| v0 + s(2_000) + s(150) * i).collect();
    let hands = Hands::new(&taps.iter().map(|&v| (v, Side::Left, Press::Key)).collect::<Vec<_>>());
    let all = Switches::default();
    let one = Plan::new(&p, &hands, &Causes::default(), &[], &all, c.seed);
    let again = Plan::new(&p, &hands, &Causes::default(), &[], &all, c.seed);
    let other = Plan::new(&p, &hands, &Causes::default(), &[], &all, c.seed ^ 1);
    let n = p.frames();
    assert!((0..n).all(|k| one.pic(k) == again.pic(k)), "the same list, the same poses");
    assert!((0..n).any(|k| one.pic(k) != other.pic(k)), "another seed, others");
    let other_list = moving("\n[[span]]\nfrom = \"60s\"\nto = \"70s\"\nspeed = 2\n");
    assert_ne!(cut(&other_list).unwrap().seed, c.seed);
    let pops_only = moving("# the pops: commands only\n[mascot]\nkeys = \"commands\"\nblink = true\n");
    assert_eq!(cut(&pops_only).unwrap().seed, c.seed, "its [mascot] and comments aside");
    // (the blinks: every 4 s at the soonest, two frames)
    let closed = |k: u64| matches!(one.pic(k), Some(Pic::At(_, Face::Closed)));
    let starts: Vec<u64> = (1..n).filter(|&k| closed(k) && !closed(k - 1)).collect();
    assert_eq!(starts.len(), one.blinks);
    assert!(one.blinks >= 10, "{} blinks in {} s", one.blinks, n / video::FPS);
    assert!(starts.windows(2).all(|w| w[1] - w[0] >= motions::frames(motions::BLINK_MS.0)), "{starts:?}");
    assert!(starts.iter().all(|&k| closed(k + 1) && !closed(k + 2)));
    // (the glance: inside the streak, the face raised while the hands tap, turning through the half each way)
    assert!(one.glances >= 1);
    let raised = |k: u64| matches!(one.pic(k), Some(Pic::At(a, Face::Raised)) if a != Arms::Rest);
    let first = (0..n).find(|&k| raised(k)).unwrap();
    assert!(first > Program::frame_at(taps[0]) && first < Program::frame_at(*taps.last().unwrap()));
    assert!(matches!(one.pic(first - 1), Some(Pic::At(_, Face::Half))), "{:?}", one.pic(first - 1));
    assert!((first..first + 13).any(|k| matches!(one.pic(k), Some(Pic::At(Arms::Left, Face::Raised)))));
}

#[test]
fn every_motion_off_is_v4s_mascot_picture_for_picture() {
    let mut list = marks().list;
    list.push(json!({ "what": "run-ended", "ms": 30_000, "run": "claude's run: x", "step": "exit 1" }));
    list.push(json!({ "what": "bring", "ms": 70_000, "how": "landed", "name": "fix" }));
    let m = Marks::new(list, 120_000);
    let c = Cut::parse(&moving(""), "cut.toml", &m).unwrap();
    let p = Program::new(&c);
    let v0 = take_at(&p, 0);
    let mut taps: Vec<u64> = (0..40).map(|i| v0 + s(1_000) + s(150) * i).collect();
    taps.push(v0 + s(12_000));
    let cs = motions::causes(&p, &m);
    let (pl, hands) = plan_of(&p, &taps, &cs, &[], STILL);
    for k in 0..p.frames() {
        let v4 = mascot::pose_at(&hands, k, &p.frame(Program::time_of(k))).map(Pic::of);
        assert_eq!(pl.pic(k), v4, "frame {k}");
    }
    assert!(pl.placed.is_empty() && pl.blinks == 0 && pl.glances == 0);
    let five = ["up", "left", "right", "both", "watch"];
    assert_eq!(Pic::needed(&STILL).iter().map(|p| p.name()).collect::<Vec<_>>(), five);
    // (each one alone changes the video)
    let each: [fn(&mut Switches); 5] = [
        |s| s.blink = true,
        |s| s.glance = true,
        |s| s.surprise = true,
        |s| s.stretch = true,
        |s| s.swing = true,
    ];
    for on in each {
        let sw = STILL.with(on);
        let (alone, _) = plan_of(&p, &taps, &cs, &[], sw);
        assert!((0..p.frames()).any(|k| alone.pic(k) != pl.pic(k)), "{}", sw.said());
    }
    // (keys = false: no lane, the marks where V4 had them)
    let a = art(60);
    let off = mascot::place(&a, (33, video::W - 66), 870, false).unwrap();
    assert_eq!((off.lane, off.marks_right + mascot::GAP - crate::shot::PAD), (None, off.x));
}

#[test]
fn the_face_turns_through_its_half_and_only_a_surprise_starts_abruptly() {
    let mut list = marks().list;
    list.push(json!({ "what": "run-ended", "ms": 30_000, "run": "claude's run: x", "step": "exit 1" }));
    let m = Marks::new(list, 120_000);
    let c = Cut::parse(&moving(""), "cut.toml", &m).unwrap();
    let p = Program::new(&c);
    let v0 = take_at(&p, 0);
    // (typing, a watch, a key; typing, a long watch — a stretch, a surprise —, typing)
    let mut taps: Vec<u64> = (0..30).map(|i| v0 + s(1_000) + s(150) * i).collect();
    let after = v0 + s(9_000);
    taps.push(after);
    taps.extend((0..30).map(|i| v0 + s(36_000) + s(150) * i));
    let (pl, _) = plan_of(&p, &taps, &motions::causes(&p, &m), &[], Switches::default());
    assert!(
        pl.placed.iter().any(|m| m.kind == Kind::Stretch)
            && pl.placed.iter().any(|m| m.kind == Kind::Surprise)
    );
    let up = |q: Pic| matches!(q, Pic::At(_, Face::Raised));
    let held = |q: Pic| matches!(q, Pic::At(_, Face::Raised | Face::Half | Face::Closed));
    for k in 1..p.frames() {
        let (Some(a), Some(b)) = (pl.pic(k - 1), pl.pic(k)) else { continue };
        assert!(!up(b) || held(a), "frame {k}: {a:?} straight to {b:?}");
        let abrupt = matches!(b, Pic::Surprise(_) | Pic::At(_, Face::Wide) | Pic::At(_, Face::Closed));
        assert!(!up(a) || held(b) || abrupt, "frame {k}: {a:?} straight to {b:?}");
    }
    // (after the watch, the face comes down two frames before the key, the arms over the keys)
    let k = Program::frame_at(after);
    assert_eq!(
        (pl.pic(k - 3), pl.pic(k - 2), pl.pic(k - 1), pl.pic(k)),
        (
            Some(Pic::WATCH),
            Some(Pic::At(Arms::Up, Face::Half)),
            Some(Pic::At(Arms::Up, Face::Half)),
            Some(Pic::At(Arms::Left, Face::Open))
        )
    );
    clear_of_keys(&pl, &taps);
}

#[test]
fn a_pop_for_every_heard_key_none_on_a_sped_span_a_chord_one_a_held_key_counted() {
    for (b, want) in [
        (&b"j"[..], Some("j")),
        (b"J", Some("J")),
        (b"\x11", Some("C-q")),
        (b"\x1b", Some("esc")),
        (b"\r", Some("enter")),
        (b" ", Some("space")),
        (b"\t", Some("tab")),
        (b"\x7f", Some("backspace")),
        (b"-", Some("-")),
        (b"\x1b[D", Some("←")),
        (b"\x1bx", Some("A-x")),
        (b"\x1c", Some("C-\\")),
        (b"\x1b[106;5u", Some("C-j")),
        (b"\x1b[27u", Some("esc")),
        (b"\x1b[106;1:3u", None),
        (b"up", Some("up")),
    ] {
        assert_eq!(pops::name(b).as_deref(), want, "{:?}", String::from_utf8_lossy(b));
    }
    // (from the sound's own list: none at the default's 4×, one each at 1×)
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let keys =
        [key(5_000, b"f"), key(14_000, b"a"), key(14_300, b"\x11"), key(14_600, b"\r"), key(30_000, b"d")];
    let heard = sounds(&p, &keys, &marks());
    // (commands 0.3 s apart: each its own pop)
    let ps = pops::pops(&heard.struck, &Regions::default(), Keys::All, 7, &cells);
    assert_eq!(ps.list.iter().map(|q| q.text.as_str()).collect::<Vec<_>>(), ["a", "C-q", "enter"]);
    assert_eq!(
        ps.list.iter().map(|q| q.from).collect::<Vec<_>>(),
        // (card 1's stamp has no key: answered with no key heard, it pops nothing)
        heard
            .taps
            .iter()
            .filter(|t| t.0 > 10_000_000)
            .take(3)
            .map(|t| Program::frame_at(t.0))
            .collect::<Vec<_>>()
    );
    assert_eq!(heard.taps.len(), heard.struck.len(), "the two stamps with no key: no tap");
    assert_eq!(heard.stamps_keyless, 2);
    assert!(ps.list.iter().all(|q| q.command));
    // (a held key: one pop, restarted with its count, never two up at once)
    let held: Vec<Struck> = (0..12)
        .map(|i| Struck { v: s(20_000) + s(80) * i, t: 0, bytes: b"j".to_vec(), answer: false })
        .collect();
    let ps = pops::pops(&held, &Regions::default(), Keys::All, 7, &cells);
    assert_eq!((ps.commands, ps.held), (1, 11));
    assert_eq!(ps.list.last().map(|q| q.text.as_str()), Some("j ×12"));
    let k0 = Program::frame_at(s(20_000));
    assert!((k0..k0 + 60).all(|k| ps.at(k).len() <= 1));
    assert!(ps.list.windows(2).all(|w| w[0].until == w[1].from && w[0].aside == w[1].aside));
    // (typed letters between the presses: still the held key's count)
    let typed_too = Regions::of(vec![(0, Place::Commands), (20_100, Place::Text), (20_200, Place::Commands)]);
    let mut between = held.clone();
    between.insert(2, Struck { v: s(20_150), t: 20_150, bytes: b"e".to_vec(), answer: false });
    let ps = pops::pops(&between, &typed_too, Keys::All, 7, &cells);
    assert_eq!((ps.commands, ps.held, ps.typed), (1, 11, 1));
    // (typed text: the letter, faint; a space nothing; enter and a chord commands; an answer a command)
    let typed = Regions::of(vec![(0, Place::Text)]);
    let at = |i: u64, b: &[u8], answer: bool| Struck {
        v: s(30_000) + s(200) * i,
        t: 1,
        bytes: b.to_vec(),
        answer,
    };
    let line = [
        at(0, b"a", false),
        at(1, b" ", false),
        at(2, b"b", false),
        at(3, b"\r", false),
        at(4, b"\x11", false),
        at(5, b"y", true),
    ];
    let ps = pops::pops(&line, &typed, Keys::All, 7, &cells);
    let got: Vec<(&str, bool)> = ps.list.iter().map(|q| (q.text.as_str(), q.command)).collect();
    assert_eq!(got, [("a", false), ("b", false), ("enter", true), ("C-q", true), ("y", true)]);
    assert_eq!(ps.spaces, 1);
    let ps = pops::pops(&line, &typed, Keys::Commands, 7, &cells);
    let texts: Vec<&str> = ps.list.iter().map(|q| q.text.as_str()).collect();
    assert_eq!(texts, ["enter", "C-q", "y"]);
    assert!(pops::pops(&line, &typed, Keys::Off, 7, &cells).list.is_empty());
    // (where the keys went: the last `keys` event at or before the key)
    let r = Regions::of(vec![(1_000, Place::Text), (2_000, Place::Commands)]);
    let t = |ms: u64| r.typed(ms, "x", false);
    assert_eq!((t(999), t(1_000), t(1_999), t(2_500)), (false, true, true, false));
    // (at a card: its answer a command, a letter it didn't take the typing it swallowed, a named key a command)
    let card = Regions::of(vec![(0, Place::Card)]);
    assert_eq!(
        (card.typed(5, "y", true), card.typed(5, "S", false), card.typed(5, "esc", false)),
        (false, true, false)
    );
    // (at the edit review, take 1's 34:15.8: `s / S h` each refused — "does nothing in the review", said a few ms
    // after — the swallowed typing; `a` taken ("a takes all": the change accepted), its own key, a command)
    let review = Regions {
        places: vec![(2_055_793, Place::Review), (2_056_293, Place::Text)],
        refused: [(2_055_977, "s"), (2_056_101, "/"), (2_056_130, "S"), (2_056_227, "h")]
            .map(|(ms, k)| (ms, k.to_string()))
            .to_vec(),
    };
    let at = [(2_055_960, "s"), (2_056_090, "/"), (2_056_130, "S"), (2_056_220, "h"), (2_056_290, "a")];
    let typed: Vec<bool> = at.iter().map(|&(ms, k)| review.typed(ms, k, false)).collect();
    assert_eq!(typed, [true, true, true, true, false]);
    assert!(!review.typed(2_055_960, "h", false), "a refusal is the key's own name's");
    // (it moves: full, rising and slowing, then fading over its last 0.35 s)
    let (r0, a0) = pops::motion(0);
    let (r7, a7) = pops::motion(7);
    let (r12, a12) = pops::motion(12);
    let (r17, a17) = pops::motion(17);
    assert_eq!((r0, a0, a7), (0.0, 255, 255));
    assert!(r7 > 0.4 && r12 - r7 < r7 && a12 < 255 && a17 < a12 && r17 > 0.9, "{r7} {r12} {a12} {a17}");
}

/// A made-up lane's widths: a command's character 16 px, a typed letter's 12.
fn cells(t: &str, command: bool) -> usize {
    t.chars().count() * if command { 16 } else { 12 }
}

/// What came after round 1 and stays with its pops: a line the key script typed at once wider than the lane, cut to it
/// with an `…` (its ink ran past the lane); the steps aside seeded by the list (the same seed the same, another others).
#[test]
fn a_line_wider_than_the_lane_is_cut_to_it_and_the_steps_aside_are_the_lists() {
    let st = |ms: u64, b: &[u8]| Struck { v: s(ms), t: ms, bytes: b.to_vec(), answer: false };
    let typed = Regions::of(vec![(0, Place::Text)]);
    let line = [st(0, b"a line the key script typed at once, far wider than the lane")];
    let ps = pops::pops(&line, &typed, Keys::All, 7, &cells);
    let text = &ps.list[0].text;
    assert!(text.ends_with('…') && cells(text, false) <= mascot::LANE && cells(text, false) > 130, "{text}");
    let word = b"read crate sources from cargo home";
    let keys: Vec<Struck> = word.iter().enumerate().map(|(i, c)| st(50 * i as u64, &[*c])).collect();
    let asides = |seed| -> Vec<usize> {
        pops::pops(&keys, &typed, Keys::All, seed, &cells).list.iter().map(|q| q.aside).collect()
    };
    assert!(asides(7) == asides(7) && asides(7) != asides(8));
    // (round 1's step aside of the last: drawn again, up to four times, while within a third of its range — so in a
    // burst few stay near the last: here 4 of 28, about 15 with no drawing again)
    let a = asides(7);
    let near = a.windows(2).filter(|w| w[0].abs_diff(w[1]) * 3 < 48).count();
    assert!(a.len() > 25 && near <= a.len() / 5, "{near} near: {a:?}");
}

#[test]
fn a_pop_stays_in_its_lane_clear_of_the_mascots_ink_the_cells_and_the_marks() {
    use super::draw::{Stage, Words, layout};
    let fonts = crate::shot::Fonts::load(1).unwrap();
    let look = video::look(Bg::Dark);
    let lay = layout(&fonts, (166, 32), video::Scale::HD).unwrap();
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let place = mascot::place(&art(60), (lay.at.0, lay.pic.0), lay.at.1 + lay.pic.1 - lay.pad, true).unwrap();
    let (left, right) = place.lane.unwrap();
    assert_eq!((right, place.marks_right - crate::shot::PAD + mascot::GAP), (place.x, left));
    let stage = Stage::new(lay, &look, &c, Some((art(60), place)));
    let screen = vt100::Parser::new(32, 166, 0);
    let pic = crate::shot::render_scaled(screen.screen(), &fonts, lay.glyph, 1);
    let pop = |text: &str, command: bool, aside: usize| Pop {
        from: 0,
        until: 18,
        text: text.into(),
        command,
        aside,
        paw: false,
    };
    let pops = Pops {
        list: vec![
            pop("C-e ×108", true, 16),
            pop("backspace", false, 48),
            pop("j", true, 0),
            pop("x", false, 48),
        ],
        ..Default::default()
    };
    let f = at_take(&p, 5_000, 0);
    let mut words = Words::new(&fonts, look.ground, video::Scale::HD);
    let without = stage.take_frame(&pic, None, &f, (Some(Pic::WATCH), &pops, &[]), &mut words);
    // (the marks: none of their ink from the gap before the lane to the box)
    for y in lay.rule + 1..video::H {
        for x in left - mascot::GAP..place.x {
            assert_eq!(&without[(y * video::W + x) * 3..][..3], &look.ground, "a mark at ({x}, {y})");
        }
    }
    for age in [0, 6, 12, 17] {
        let up: Vec<(usize, u64)> = (0..pops.list.len()).map(|i| (i, age)).collect();
        let with = stage.take_frame(&pic, None, &f, (Some(Pic::WATCH), &pops, &up), &mut words);
        let mut changed = 0;
        for y in 0..video::H {
            for x in 0..video::W {
                let at = (y * video::W + x) * 3;
                if with[at..at + 3] != without[at..at + 3] {
                    changed += 1;
                    assert!(
                        (left..right).contains(&x) && y > lay.rule && y < video::H,
                        "age {age}: a pop's ink at ({x}, {y}), the lane {left}..{right} under the rule {}",
                        lay.rule
                    );
                }
            }
        }
        assert!(changed > 0, "age {age}: drawn");
    }
    // (each alone: a command in the ink, a typed letter in the faint ink; its ink's right edge its step aside of the
    // lane's right, or, too wide for that, its left at the lane's left)
    for (i, q) in pops.list.iter().enumerate() {
        let with = stage.take_frame(&pic, None, &f, (Some(Pic::WATCH), &pops, &[(i, 0)]), &mut words);
        let (mut lo, mut hi, mut most) = (usize::MAX, 0, 0u8);
        for y in lay.rule + 1..video::H {
            for x in left..right {
                let at = (y * video::W + x) * 3;
                if with[at..at + 3] != without[at..at + 3] {
                    (lo, hi, most) = (lo.min(x), hi.max(x), most.max(with[at]));
                }
            }
        }
        let want = if q.command { look.ink[0] } else { look.faint[0] };
        assert!(most.abs_diff(want) <= 6, "{q:?}: its strongest red {most}, not {want}");
        match q.text == "backspace" {
            true => assert!(lo <= left + 3, "{q:?}: ink {lo}..={hi}"),
            false => assert!(hi < right - q.aside && hi + 6 >= right - q.aside, "{q:?}: ink {lo}..={hi}"),
        }
    }
    // (the box cutting a picture's ink is said: the made-up art's reaches its right and bottom edges)
    assert!(art(60).cut_ink().iter().any(|(n, _)| n == "up"));
}

/// A 166×32 screen on meok's ground with an `ls -la` line on row 2, a stand-in login `zq` at cells 15–16 on a
/// highlight (its own colour, to see it go), lines above and below it; `erased`: those two cells written as the
/// ground's spaces instead.
fn ls_screen(erased: bool) -> vt100::Parser {
    let mut p = vt100::Parser::new(32, 166, 0);
    p.process(b"\x1b[?25l\x1b[48;2;11;14;20m\x1b[2J\x1b[38;2;191;189;182m\x1b[2;1Htotal 8 drwxr-xr-x");
    p.process(b"\x1b[4;1Hdrwxr-xr-x@ 6 root  wheel\x1b[3;1Hdrwx------@  2 ");
    match erased {
        true => p.process(b"  "),
        false => p.process(b"\x1b[48;2;34;40;52mzq\x1b[48;2;11;14;20m"),
    }
    p.process(b"    wheel    64 Nov 12 16:51 .");
    p
}

/// A mask on what the check found, `zq` on row 2, over the take's 0–60 s.
fn zq_mask() -> mask::Mask {
    let place = mask::Place::Found { kind: "name", found: "zq".into() };
    mask::Mask { from: 0, to: 60_000, row: 2, place, line: 1, named: "check.txt:4, row 2 name zq".into() }
}

/// Rectangles of a picture, in px: left, top, right, bottom (the last two not held).
type Rects = Vec<(usize, usize, usize, usize)>;

/// The masked cells' pixels at 1080p (the renderer's cell at the layout's glyph): each cell's rectangle in the picture.
fn masked_rects(blank: &[crate::shot::Blank]) -> (Rects, crate::shot::Fonts, f32) {
    let fonts = crate::shot::Fonts::load(1).unwrap();
    let glyph = super::draw::layout(&fonts, (166, 32), video::Scale::HD).unwrap().glyph;
    let (cw, ch) = crate::shot::cell(&fonts, glyph);
    let pad = crate::shot::PAD;
    let rects = (blank.iter())
        .map(|&(r, a, b)| {
            let (r, a, b) = (usize::from(r), usize::from(a), usize::from(b));
            (pad + a * cw, pad + r * ch, pad + b * cw, pad + (r + 1) * ch)
        })
        .collect();
    (rects, fonts, glyph)
}

/// A mask on the check's place blanks the cells of what it found — the login's two, found on the row as the check reads
/// it — and draws them in the screen's ground: no letter, not the highlight they stood on.
#[test]
fn a_masks_cells_are_drawn_blank_in_the_screens_ground() {
    let screen = ls_screen(false);
    let (masks, hits) = (vec![zq_mask()], Vec::new());
    let mut tally = mask::Tally::new(&masks, &hits);
    let blank = tally.frame(12_000, screen.screen());
    assert_eq!(blank, [(2, 15, 17)], "the login's cells on row 2");
    let (rects, fonts, glyph) = masked_rects(&blank);
    let (w, _, bare) = crate::shot::render_scaled(screen.screen(), &fonts, glyph, 1);
    let (_, _, hid) = crate::shot::render_masked(screen.screen(), &fonts, glyph, 1, &blank);
    let ground = [11u8, 14, 20];
    let (x0, y0, x1, y1) = rects[0];
    let mut was = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) * 3;
            was += usize::from(bare[i..i + 3] != ground);
            assert_eq!(hid[i..i + 3], ground, "({x}, {y}) inside the masked cells");
        }
    }
    assert!(was > 0, "the unmasked picture had the login there");
}

/// Around a mask nothing changes: every pixel outside its cells is the unmasked picture's — a mask on `rw` of
/// `drwx------@`, its neighbours inked on every side: `d` and `x` beside it, the lines above and below.
#[test]
fn a_masks_neighbours_are_untouched() {
    let screen = ls_screen(false);
    let blank = [(2, 1, 3)];
    let (rects, fonts, glyph) = masked_rects(&blank);
    let (w, h, bare) = crate::shot::render_scaled(screen.screen(), &fonts, glyph, 1);
    let (_, _, hid) = crate::shot::render_masked(screen.screen(), &fonts, glyph, 1, &blank);
    let (x0, y0, x1, y1) = rects[0];
    let mut differ = 0;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 3;
            if !(x0..x1).contains(&x) || !(y0..y1).contains(&y) {
                assert_eq!(hid[i..i + 3], bare[i..i + 3], "({x}, {y}) outside the masked cells");
            } else {
                differ += usize::from(hid[i..i + 3] != bare[i..i + 3]);
            }
        }
    }
    assert!(differ > 0, "the mask changed its own cells");
}

/// The camera's zoom on the masked word still shows it blank: the frame through the zoom, its screen's picture made at
/// the zoom's glyph as the cut makes it (`Stage::screen_pic`), is the frame of the screen with those cells never
/// written; and the same screen first drawn unmasked is drawn again once masked, never the kept picture.
#[test]
fn a_zoom_on_a_mask_still_hides_its_cells() {
    use super::draw::{Stage, Words, layout};
    let size = (166, 32);
    let fonts = crate::shot::Fonts::load(1).unwrap();
    let lay = layout(&fonts, size, video::Scale::HD).unwrap();
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let look = video::look(Bg::Dark);
    let stage = Stage::new(lay, &look, &c, None);
    let region = wide_window(Region { x: 15.0, y: 2.0, w: 2.0, h: 1.0 }, size);
    assert!(region.w < 166.0, "a zoom, not the whole: {region:?}");
    let view = lay.view(region, size, &fonts);
    let f = at_take(&p, 12_000, 0);
    let pops = mascot::pops::Pops::default();
    let frame = |s: &vt100::Screen, blank: &[crate::shot::Blank], kept: &mut Option<_>| {
        let (pic, fresh) = stage.screen_pic(kept, s, (false, 1), Some(&view), blank, &fonts);
        let px = stage.take_frame(
            pic,
            Some(&view),
            &f,
            (None, &pops, &[][..]),
            &mut Words::new(&fonts, look.ground, lay.scale),
        );
        (px, fresh)
    };
    let (screen, mut kept) = (ls_screen(false), None);
    let blank = [(2, 15, 17)];
    let (bare, _) = frame(screen.screen(), &[], &mut kept);
    let (hid, fresh) = frame(screen.screen(), &blank, &mut kept);
    assert!(fresh, "the same screen, now masked, drawn again");
    let (erased, _) = frame(ls_screen(true).screen(), &[], &mut None);
    assert_ne!(bare, erased, "the zoom shows the login unmasked");
    let first = (0..hid.len()).find(|&i| hid[i] != erased[i]).map(|i| (i / 3 % video::W, i / 3 / video::W));
    assert_eq!(
        first, None,
        "the masked zoom against the erased screen's: the first pixel (x, y) that differs"
    );
}

/// A mask that matches no frame is said loudly in `cut.txt`: one past the take's end (take seconds, not refused), one
/// whose cells are empty in every frame, one on a check place not on its row; the one that hid something says its
/// frames. A `check = N` that isn't a place's line is refused with the list's line.
#[test]
fn a_mask_that_matches_no_frame_is_said() {
    let text = format!(
        "{CUT}\n[[mask]]\ncheck = 4\n\n[[mask]]\nfrom = 200.5\nto = 201\nrow = 2\ncells = [15, 16]\n\n[[mask]]\nfrom = 10\n\
         to = \"turn 1 end\"\nrow = 9\ncells = [0, 40]\n\n[[mask]]\ncheck = 5\n"
    );
    let c = cut(&text).unwrap();
    let base = cut(CUT).unwrap();
    assert_eq!(c.seed, base.seed, "a mask reshuffles no blink");
    let check = "0 places\n\nweak — a login under 4 letters as a bare word, not failing:\n   10.00s–30.00s   row  2  name   zq  │ \
                 drwx------@  2 zq    wheel\n   11.00s–12.00s   row  2  name   zz  │ drwx------@  2 zz    wheel\n2 weak\n";
    let masks = mask::resolve(&c.masks, Some(check), "cut.toml").unwrap();
    assert_eq!(masks[0].from, 9_990, "a check place's mask 10 ms wider each side");
    let hits = Vec::new();
    let mut tally = mask::Tally::new(&masks, &hits);
    let screen = ls_screen(false);
    for t in [10_000, 11_500, 20_000, 39_000] {
        tally.frame(t, screen.screen());
    }
    let said = tally.said("cut.toml");
    let line = |k: usize| c.masks[k].line;
    assert_eq!(
        said,
        [
            "masks (4): cells drawn blank in the screen's ground in every frame their span holds, the camera's crops and \
             zooms too"
                .to_string(),
            format!(
                "  cut.toml:{} — check.txt:4, row 2 name zq, take 0:00:09.99–0:00:30.01: 3 frames covered, something under \
                 it in 3",
                line(0)
            ),
            format!(
                "  cut.toml:{} — row 2, cells 15–16, take 0:03:20.50–0:03:21.00: MATCHES NO FRAME: no frame of the video \
                 shows the take then (left out, or past its end)",
                line(1)
            ),
            format!(
                "  cut.toml:{} — row 9, cells 0–40, take 0:00:10.00–0:00:40.00: MATCHES NO FRAME: 4 frames covered, its \
                 cells empty in every one",
                line(2)
            ),
            format!(
                "  cut.toml:{} — check.txt:5, row 2 name zz, take 0:00:10.99–0:00:12.01: MATCHES NO FRAME: 1 frame \
                 covered, no zz on row 2 in every one",
                line(3)
            ),
        ]
    );
    assert_eq!(tally.loud("cut.toml").len(), 3);
    let e = mask::resolve(&c.masks, Some("0 places\n"), "cut.toml").unwrap_err();
    assert_eq!(
        e,
        format!("cut.toml:{}: mask: check = 4: check.txt's line 4 isn't a place (it has none)", line(0))
    );
    let e = mask::resolve(&c.masks, None, "cut.toml").unwrap_err();
    assert!(e.ends_with("the take has no check.txt (`cargo xtask record check` writes it)"), "{e}");
}

/// The cut's own check leaves out a place a mask hid wherever a frame showed it, counted apart — never one a mask
/// held in its span but missed on the row (its cells elsewhere), nor one no mask holds.
#[test]
fn the_cuts_check_leaves_out_only_what_a_mask_hid() {
    use crate::record::check::Hit;
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let text = "drwx------@  2 zq    wheel    64 Nov 12 16:51 .".to_string();
    let hits =
        vec![Hit { from: 12_000, to: 13_000, row: 2, kind: "name", found: "zq".into(), text, weak: false }];
    let screen = ls_screen(false);
    let missed = mask::Mask { place: mask::Place::Cells(30, 40), ..zq_mask() };
    let cases = [
        ("the mask on what it found", vec![zq_mask()], true),
        ("a mask on other cells", vec![missed], false),
    ];
    for (what, masks, hid) in cases {
        let mut tally = mask::Tally::new(&masks, &hits);
        for t in [12_000, 12_500] {
            tally.frame(t, screen.screen());
        }
        assert_eq!(tally.hidden(), [hid], "{what}");
        let said = super::check::said(&p, &hits, &tally.hidden(), "check.txt");
        let want = match hid {
            true => {
                "check (check.txt): 0 strong, 0 weak inside the cut's spans, crossings included — of the take's 1 \
                     hits, 1 hidden by the masks in every frame that shows them; none inside the cut"
            }
            false => {
                "check (check.txt): 1 strong, 0 weak inside the cut's spans, crossings included — of the take's 1 \
                      hits; for the user to decide:"
            }
        };
        assert_eq!(said[0], want, "{what}");
    }
    // (no mask: nothing hidden, whatever no frame showed)
    assert_eq!(mask::Tally::new(&[], &hits).hidden(), [false]);
}

// ── Pressing fast-forward ──

/// `moving`'s take with these spans (take seconds from, to, and its pace: `speed = N` or `out = true`).
fn spanned(spans: &[(u64, u64, &str)]) -> Cut {
    let rest: String = (spans.iter())
        .map(|(a, b, pace)| format!("[[span]]\nfrom = \"{a}s\"\nto = \"{b}s\"\n{pace}\n"))
        .collect();
    cut(&moving(&rest)).unwrap()
}

const FAST: Switches = Switches { fast: true, ..STILL };

/// A stretch past 2×: the right paw held down through it, the face on the screen, and one `▸▸` from that paw where it
/// goes down; lifted where it ends, the hands over the keys after. Off: watching, as before.
#[test]
fn a_fast_stretch_holds_the_right_paw_and_pops_once() {
    let c = spanned(&[(10, 20, "speed = 8")]);
    let p = Program::new(&c);
    let (holds, short) = motions::fast_holds(&p);
    let (a, b) = holds[0];
    assert_eq!((holds.len(), short), (1, 0));
    assert_eq!(a, Program::frame_at(take_at(&p, 10_000)), "down where the stretch begins");
    assert_eq!(b, Program::frame_at(take_at(&p, 20_000)), "lifted where it ends");
    let plan = Plan::new(&p, &Hands::new(&[]), &Causes::default(), &[], &FAST, 7);
    let held = Pic::At(Arms::Right, Face::Raised);
    assert_eq!(plan.pic(a - 1), Some(Pic::WATCH));
    assert!((a..b - motions::TURN).all(|k| plan.pic(k) == Some(held)), "{:?}", plan.pic(a));
    assert_eq!(
        (plan.pic(b - 1), plan.pic(b)),
        (Some(Pic::At(Arms::Right, Face::Half)), Some(Pic::At(Arms::Up, Face::Open))),
        "lifted, the face turned back to the keys"
    );
    assert_eq!(plan.fast, holds);
    let pops = Pops::default().presses(&holds, Keys::All);
    assert_eq!(
        pops.list.iter().map(|q| (q.from, q.text.as_str(), q.command, q.paw)).collect::<Vec<_>>(),
        [(a, pops::PRESS, true, true)]
    );
    assert!(Pops::default().presses(&holds, Keys::Off).list.is_empty(), "the pops off: none");
    // (off: watching through it, as before)
    let off = Plan::new(&p, &Hands::new(&[]), &Causes::default(), &[], &STILL, 7);
    assert!((a..b).all(|k| off.pic(k) == Some(Pic::WATCH)));
}

/// A key shown inside the hold taps the left paw one frame, a frame up after; keys closer than that share the tap; none
/// is heard or pops.
#[test]
fn a_key_inside_a_fast_stretch_taps_the_left_paw() {
    let c = spanned(&[(10, 20, "speed = 8")]);
    let p = Program::new(&c);
    let frame = |t: u64| Program::frame_at(take_at(&p, t));
    // (the first take ms past 16 s whose frame is the next one: a key a frame after another)
    let next = (16_001..17_000).find(|&t| frame(t) == frame(16_000) + 1).unwrap();
    let keys = [key(12_000, b"j"), key(12_010, b"k"), key(16_000, b"l"), key(next, b"h")];
    let s = sounds(&p, &keys, &marks());
    assert_eq!(s.sped.len(), 4);
    let heard = s.list.iter().any(|h| matches!(h.1, Heard::Key(_)));
    assert!(s.taps.is_empty() && s.struck.is_empty() && !heard, "no tap of its own, no pop, no sound");
    let hands = Hands::new(&s.taps).sped(&s.sped);
    let plan = Plan::new(&p, &hands, &Causes::default(), &[], &FAST, 7);
    let (both, right) = (Pic::At(Arms::Both, Face::Raised), Pic::At(Arms::Right, Face::Raised));
    let at = |k: u64| plan.pic(k).unwrap();
    assert_eq!((at(frame(12_000)), at(frame(12_000) + 1)), (both, right));
    assert_eq!((at(frame(16_000)), at(frame(16_000) + 1)), (both, right), "the next frame's key shares it");
    assert_eq!((plan.fast_taps, plan.fast_keys), (2, 4));
}

/// A fast stretch too short to read as a press gets nothing; one just long enough holds.
#[test]
fn a_short_fast_stretch_does_nothing() {
    let c = spanned(&[(10, 12, "speed = 8")]);
    let p = Program::new(&c);
    let (holds, short) = motions::fast_holds(&p);
    assert_eq!((holds.len(), short), (0, 1), "0.25 s of video");
    let plan = Plan::new(&p, &Hands::new(&[]), &Causes::default(), &[], &FAST, 7);
    assert!((0..p.frames()).all(|k| !matches!(plan.pic(k), Some(Pic::At(Arms::Right, _)))));
    assert!(Pops::default().presses(&holds, Keys::All).list.is_empty());
    let c = spanned(&[(10, 13, "speed = 10")]);
    let p = Program::new(&c);
    assert_eq!(motions::fast_holds(&p).1, 1, "0.3 s: a stamp's press, still short");
    let c = spanned(&[(10, 14, "speed = 8")]);
    let p = Program::new(&c);
    let (holds, short) = motions::fast_holds(&p);
    assert_eq!((holds.len(), short), (1, 0), "0.5 s of video");
}

/// Two fast stretches with only a cut between are one hold, one press; a stretch at 1× between makes two.
#[test]
fn two_fast_stretches_joined_by_a_cut_are_one_hold() {
    let c = spanned(&[(10, 14, "speed = 8"), (14, 30, "out = true"), (30, 40, "speed = 16")]);
    let p = Program::new(&c);
    let (holds, _) = motions::fast_holds(&p);
    assert_eq!(holds.len(), 1, "{holds:?}");
    assert_eq!(Pops::default().presses(&holds, Keys::All).presses, 1);
    let c = spanned(&[(10, 14, "speed = 8"), (14, 16, "speed = 1"), (16, 24, "speed = 16")]);
    let p = Program::new(&c);
    assert_eq!(motions::fast_holds(&p).0.len(), 2);
}

/// A stretch due inside a fast-forward hold doesn't begin there: holding fast-forward isn't idle, and the quiet counts
/// from the lift (the user's pick, 2026-11-12).
#[test]
fn no_stretch_begins_while_fast_forward_is_held() {
    let c = spanned(&[(5, 45, "speed = 8")]);
    let p = Program::new(&c);
    let (holds, _) = motions::fast_holds(&p);
    let (a, b) = holds[0];
    let quiet = motions::frames(motions::STRETCH_QUIET_MS);
    let stretches = |sw: Switches| {
        let plan = Plan::new(&p, &Hands::new(&[]), &Causes::default(), &[], &sw, 7);
        plan.placed.iter().filter(|m| m.kind == Kind::Stretch).map(|m| m.from).collect::<Vec<_>>()
    };
    // (with no hold, the stretch comes 6 s into the take: inside where the hold is)
    let due = stretches(STILL.with(|s| s.stretch = true));
    assert!(due.first().is_some_and(|&k| a <= k && k < b), "{due:?} against the hold {a}–{b}");
    // (held: none begins in it, the first a whole quiet after the lift)
    let held = stretches(FAST.with(|s| s.stretch = true));
    assert!(held.iter().all(|&k| k >= b + quiet), "{held:?} against the hold {a}–{b}");
}

/// At 2× the keys are heard: nothing of this changes there — every picture the same with it on or off.
#[test]
fn at_two_times_nothing_changes() {
    let all = |speed: &str| {
        let c = spanned(&[(10, 20, speed)]);
        let p = Program::new(&c);
        let s = sounds(&p, &[key(12_000, b"j"), key(16_000, b"f")], &marks());
        let hands = Hands::new(&s.taps).sped(&s.sped);
        let pics = |sw: Switches| {
            let plan = Plan::new(&p, &hands, &Causes::default(), &[], &sw, 7);
            (0..p.frames()).map(|k| plan.pic(k)).collect::<Vec<_>>()
        };
        let on = Switches::default();
        (s.sped.len(), s.taps.len(), pics(on) == pics(on.with(|s| s.fast = false)))
    };
    assert_eq!(all("speed = 2"), (0, 2, true));
    assert_eq!(all("speed = 8"), (2, 0, false), "past 2× it does");
}

// ── Winding the skein ──

/// A frame file `frame-NAME.png` in `s` at the made size (4× `size`), 8-bit RGBA as the frames are: clear, but where
/// `ink` says (a pixel at 4×: x, y, alpha).
fn made(
    s: &tarae_testkit::scratch::Scratch,
    name: &str,
    size: (usize, usize),
    ink: Option<(usize, usize, u8)>,
) {
    let (w, h) = (size.0 * 4, size.1 * 4);
    let mut px = vec![0u8; w * h * 4];
    if let Some((x, y, a)) = ink {
        px[(y * w + x) * 4..][..4].copy_from_slice(&[200, 200, 200, a]);
    }
    let file = std::fs::File::create(s.join(format!("frame-{name}.png"))).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&px).unwrap();
}

/// The plan over `c`'s take with the keys `keys`, its holds wound on the skein; and the first hold.
fn wound(c: &Cut, keys: &[Rec]) -> (Plan, (u64, u64)) {
    let p = Program::new(c);
    let s = sounds(&p, keys, &marks());
    let hands = Hands::new(&s.taps).sped(&s.sped);
    let mut plan = Plan::new(&p, &hands, &Causes::default(), &[], &FAST, 7);
    plan.wind(&p, &hands);
    let hold = plan.fast[0];
    (plan, hold)
}

/// The loop's pictures from frame `a` on, `n` of them, by name: `in` the in-between, `0`–`2` the loop's steps.
fn winding(plan: &Plan, a: u64, n: u64) -> String {
    (a..a + n)
        .map(|k| match plan.pic(k) {
            Some(Pic::WindIn) => "in".to_string(),
            Some(Pic::Wind(s)) => s.to_string(),
            p => format!("{p:?}"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A hold up to ×8 winds the skein instead: the in-between two frames, then the loop a step every 4 frames, round and
/// round, back to the keys where it ends; `cut.txt` says it was wound.
#[test]
fn a_fast_stretch_winds_the_skein_a_step_every_four_frames() {
    let (plan, (a, b)) = wound(&spanned(&[(10, 20, "speed = 8")]), &[]);
    assert_eq!(plan.pic(a - 1), Some(Pic::WATCH), "from the screen's watch");
    assert_eq!(winding(&plan, a, 15), "in in 0 0 0 0 1 1 1 1 2 2 2 2 0");
    assert!(matches!(plan.pic(b - 1), Some(Pic::Wind(_))), "wound to its last frame: {:?}", plan.pic(b - 1));
    assert_eq!(plan.pic(b), Some(Pic::At(Arms::Up, Face::Open)), "then the keys");
    let said = plan.said(&FAST).join("\n");
    assert!(said.contains("fast-forward wound on the skein (a ▸▸ pop): 1, at "), "{said}");
    assert!(!said.contains("the right paw"), "{said}");
}

/// A key shown in the hold steps the coil at once, not waiting its 4 frames; a key in the frame after a step shares it.
#[test]
fn a_key_inside_a_winding_steps_the_coil_at_once() {
    let c = spanned(&[(10, 20, "speed = 8")]);
    let p = Program::new(&c);
    let (_, (a, _)) = wound(&c, &[]);
    let take = |f: u64| (10_000..20_000).find(|&t| Program::frame_at(take_at(&p, t)) == f).unwrap();
    // (the loop's first step from frame a + 2: a key at a + 4, two frames in, steps it there; one at a + 9, a frame
    // after the step it made at a + 8, shares that one)
    let keys = [key(take(a + 4), b"j"), key(take(a + 9), b"k")];
    let (plan, _) = wound(&c, &keys);
    assert_eq!(winding(&plan, a, 14), "in in 0 0 1 1 1 1 2 2 2 2 0 0");
    assert_eq!((plan.wind_key_steps, plan.fast_keys), (1, 2));
}

/// Past ×8 the coil steps every 2 frames.
#[test]
fn past_eight_times_the_skein_winds_every_two_frames() {
    let (plan, (a, _)) = wound(&spanned(&[(10, 40, "speed = 16")]), &[]);
    assert_eq!(winding(&plan, a, 9), "in in 0 0 1 1 2 2 0");
}

/// Without the skein's pictures — all four, or the cut doesn't wind — the hold is as before: `Art` loads what's there
/// of them and never fails for one missing, and says whether they all came.
#[test]
fn without_the_skeins_pictures_the_hold_is_as_before() {
    let s = tarae_testkit::scratch::Scratch::new("xtask-cut-skein");
    let hd = video::Scale::HD;
    let wind = Pic::winding(&FAST);
    assert_eq!(wind.len(), 4);
    assert!(Pic::winding(&STILL).is_empty(), "fast-forward off: none looked for");
    let load = || Art::load(Bg::Dark, Some(s.path().to_path_buf()), hd, (&[], &wind), true).unwrap();
    assert!(!load().winds(), "none there: held");
    let blank = |name: &str| made(&s, name, mascot::BOX, None);
    for p in &wind[..3] {
        blank(&p.name());
    }
    assert!(!load().winds(), "three of four: held");
    blank("wind-in");
    assert!(load().winds(), "all four: wound");
    // (held: today's hold, the right paw down — no plan.wind)
    let c = spanned(&[(10, 20, "speed = 8")]);
    let p = Program::new(&c);
    let plan = Plan::new(&p, &Hands::new(&[]), &Causes::default(), &[], &FAST, 7);
    let (a, b) = plan.fast[0];
    assert!((a..b - motions::TURN).all(|k| plan.pic(k) == Some(Pic::At(Arms::Right, Face::Raised))));
    assert!(!plan.wound && plan.said(&FAST).join("\n").contains("fast-forward held (the right paw"));
}

// ── The swing's big V over the lane ──

/// `art(60)` with the big V's three pictures over the lane: each the lane's size at 1080p, inked from column 80 to
/// 147 in rows 5 to 144 (across the rule's row), each a different grey so one can be told from another.
fn art_with_v() -> Art {
    let mut a = art(60);
    let (w, h) = (mascot::LANE, mascot::BOX.1);
    a.marks = (0..3u8)
        .map(|i| {
            let mut px = vec![0u8; w * h * 4];
            for y in 5..145 {
                for x in 80..148 {
                    px[(y * w + x) * 4..][..4].copy_from_slice(&[150 + 30 * i, 100, 80, 255]);
                }
            }
            Some((w, h, px))
        })
        .collect();
    a
}

/// The big V is drawn over the lane at the swing's marked frames — its down-stroke at swing-3, whole at swing-4, drying
/// at swing-5 — and at no other picture; and nowhere but the lane beside the box.
#[test]
fn the_big_v_is_drawn_in_the_lane_at_the_swings_frames_and_nowhere_else() {
    use super::draw::{Stage, Words, layout};
    let fonts = crate::shot::Fonts::load(1).unwrap();
    let look = video::look(Bg::Dark);
    let lay = layout(&fonts, (166, 32), video::Scale::HD).unwrap();
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let art = art_with_v();
    assert!(art.big_v());
    let place = mascot::place(&art, (lay.at.0, lay.pic.0), lay.at.1 + lay.pic.1 - lay.pad, true).unwrap();
    let (left, right) = place.lane.unwrap();
    let stage = Stage::new(lay, &look, &c, Some((art, place)));
    let screen = vt100::Parser::new(32, 166, 0);
    let pic = crate::shot::render_scaled(screen.screen(), &fonts, lay.glyph, 1);
    let f = at_take(&p, 5_000, 0);
    let mut words = Words::new(&fonts, look.ground, video::Scale::HD);
    let mut frame =
        |pose: Pic| stage.take_frame(&pic, None, &f, (Some(pose), &Pops::default(), &[]), &mut words);
    let without = frame(Pic::WATCH);
    let mut reds = Vec::new();
    for pose in Pic::all() {
        let with = frame(pose);
        let (mut changed, mut red) = (0, 0u8);
        let (mut lo, mut hi) = ((usize::MAX, usize::MAX), (0, 0));
        for y in 0..video::H {
            for x in 0..video::W {
                let at = (y * video::W + x) * 3;
                if with[at..at + 3] != without[at..at + 3] {
                    changed += 1;
                    red = red.max(with[at]);
                    (lo, hi) = ((lo.0.min(x), lo.1.min(y)), (hi.0.max(x), hi.1.max(y)));
                    assert!(
                        (left..right).contains(&x) && (place.y..place.y + mascot::BOX.1).contains(&y),
                        "{pose:?}: ink at ({x}, {y}), the lane {left}..{right} from {}",
                        place.y
                    );
                }
            }
        }
        let marked = matches!(pose, Pic::Swing(n) if mascot::MARKED.contains(&n));
        assert_eq!(changed > 0, marked, "{pose:?}: {changed} pixels changed");
        if marked {
            reds.push(red);
            // (against the box, as its picture lays it: the lane's columns 80 to 147, rows 5 to 144 from the box's top)
            assert_eq!((lo, hi), ((left + 80, place.y + 5), (left + 147, place.y + 144)), "{pose:?}");
        }
    }
    assert_eq!(reds, [150, 180, 210], "swing-3, -4, -5 each its own picture");
}

/// The lane off, or the big pictures not all there: the swing's marked frames are the box's small-V ones — where those
/// are there, else its usual frames; never an error.
#[test]
fn the_v_falls_back_to_the_small_one_in_the_box_with_the_lane_off() {
    let s = tarae_testkit::scratch::Scratch::new("xtask-cut-big-v");
    let hd = video::Scale::HD;
    let want = [Pic::Swing(1), Pic::Swing(2)];
    let load =
        |lane: bool| Art::load(Bg::Dark, Some(s.path().to_path_buf()), hd, (&want, &[]), lane).unwrap();
    // (the usual frames clear; the small V's swing-3 inked at its first pixel, so it can be told apart)
    made(&s, "swing-2", mascot::BOX, None);
    made(&s, "swing-3-r3", mascot::BOX, None);
    let small = |a: &Art| a.picture(Pic::Swing(2)).2.chunks(4).any(|p| p[3] > 0);
    let a = load(true);
    assert!(!a.big_v() && !small(&a), "nothing else there: the usual frames");
    made(&s, "swing-3-v", mascot::BOX, Some((0, 0, 255)));
    for n in 3..=5 {
        made(&s, &format!("swing-mark-{n}"), (mascot::LANE, mascot::BOX.1), None);
    }
    let a = load(true);
    assert!(
        a.big_v() && !small(&a),
        "the lane on, the big V's three there: big, the box's frame without a V"
    );
    assert!(a.mark(Pic::Swing(3)).is_some() && a.mark(Pic::Swing(1)).is_none());
    let a = load(false);
    assert!(
        !a.big_v() && small(&a) && a.mark(Pic::Swing(3)).is_none(),
        "the lane off: the small V in the box"
    );
    assert!(a.v_said(false).contains("the lane is off"), "{}", a.v_said(false));
    std::fs::remove_file(s.join("frame-swing-mark-5.png")).unwrap();
    let a = load(true);
    assert!(!a.big_v() && small(&a), "one big picture missing: the small V");
    assert!(a.v_said(true).contains("not all found"), "{}", a.v_said(true));
}

/// The big V's place: over the lane, the box's height — so never over a cell (the box never is) —, the band's marks
/// ending `GAP` before it; the rule stopping `RULE_GAP` short of its ink on the rule's row, the whole video the same.
#[test]
fn the_big_v_stays_clear_of_the_marks_the_cells_and_the_rule() {
    use crate::shot::PAD;
    let (x0, pw) = (33, video::W - 66);
    let cells = 870;
    let without = mascot::place(&art(60), (x0, pw), cells, true).unwrap();
    let place = mascot::place(&art_with_v(), (x0, pw), cells, true).unwrap();
    let (left, right) = place.lane.unwrap();
    assert_eq!((right, right - left), (place.x, mascot::LANE));
    assert!(place.y >= cells, "the V's top (the box's) under the cells' last row");
    assert_eq!(place.marks_right - PAD + mascot::GAP, left, "the marks end GAP before the lane");
    assert_eq!(place.marks_right, without.marks_right, "the marks don't move for it");
    // (the made-up V's ink starts at the lane's column 80 on the rule's row)
    assert_eq!(place.rule_end, left + 80 - 26, "the rule stops short of the V's ink");
    assert_eq!(without.rule_end, without.x + 60 - 26, "without it, short of the box's");
}
