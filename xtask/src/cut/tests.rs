//! `cut`'s tests: made-up takes (landmarks written by hand, screens drawn by their own bytes) — no tarae, no claude,
//! nothing that waits.

use serde_json::json;

use super::draw::{label, lost_zooms, wide_window};
use super::init::{Facts, rules, stills, ticking, written};
use super::sound::{
    BED_REF_LUFS, Made, Mix, NEAR_KEY_DB, Placed, Track, Where, bed_gains, heard_at, laid, lay, layout,
    made_said, mixed, realtime, sides, sounds, under,
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
    let Ok(intro) = video::WideIntro::new(bg, &look, &fonts) else { return };
    let px = intro.frame(0, &look, (video::W, video::H));
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
        told(c.end, p.total),
        format!(
            "A real session with Claude in tarae: 2m 00s, shown in {}",
            run_duration(Duration::from_micros(p.total))
        )
    );
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
        let px = super::opening::card(&c.opening, &said, bg, &look, &fonts, (video::W, video::H));
        let p = dir.join(format!("built-{}.png", bg.theme()));
        crate::shot::save_png(&p, video::W, video::H, &px).unwrap();
        println!("{}", p.display());
        if let Ok(intro) = video::WideIntro::new(bg, &look, &fonts) {
            for ms in [800, INTRO_MS - 100] {
                let p = dir.join(format!("intro-{}-{ms}.png", bg.theme()));
                crate::shot::save_png(&p, video::W, video::H, &intro.frame(ms, &look, (video::W, video::H)))
                    .unwrap();
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
/// and read back as ffmpeg decodes one, laid at its gain, the swing's curve; each ceiling try: the keys' track (a key
/// every 0.3 s over half the video), their peaks, the two sides, their true peaks and limiter, the figures, the WAV and
/// its AAC; once after: the 3 s windows. Then the most memory held.
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
    let keys = crate::sound::track(&heard, Duration::from_micros(total), Some(ms(595_000)), -2.0, None);
    lap("(a try) the keys' track");
    let peaks = super::sound::key_peaks(&keys.samples, heard.iter().map(|h| h.0.as_micros() as u64));
    assert_eq!(peaks.len(), 1_000);
    lap("(a try) each key's peak");
    let [mut l, mut r] = sides(&keys.samples, &beds, &d, 0.0, 6.0);
    drop(keys);
    lap("(a try) the two sides");
    let tp = crate::sound::true_peaks_of(&[&l, &r]);
    lap("(a try) their true peaks");
    let gains = crate::sound::limit_sides(&mut [&mut l, &mut r], &tp, 10f32.powf(-2.0 / 20.0));
    drop((tp, gains));
    lap("(a try) the limiter");
    let _ = crate::sound::stereo_lufs(&l, &r);
    lap("(a try) the whole's loudness");
    let _ = crate::sound::true_peak_db(&[&l, &r]);
    lap("(a try) the whole's true peak");
    let _ = crate::sound::correlation(&l, &r);
    lap("(a try) the correlation");
    let wav = dir.join("cut.wav");
    crate::sound::wav_sides(&wav, &[&l, &r]).unwrap();
    lap("(a try) the WAV");
    if let Some(f) = crate::video::ffmpeg() {
        let _ = crate::video::aac_peak(&f, &wav);
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

// ── The mascot at the keys ──

use super::mascot::{self, Art, Hands, Pose, Side};

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
        key(14_950, b"y"),  // the key that answers card 1: the stamp's, both
        key(30_000, b"d"),  // left out
    ];
    let s = sounds(&p, &keys, &marks());
    // (the opening's where it ends shows 55–58 s: card 2's answer at 56 s, its stamp, there at 7 s)
    assert_eq!(
        s.taps,
        [
            (7_000_000, Side::Both),
            (17_500_000, Side::Left),
            (17_800_000, Side::Right),
            (18_100_000, Side::Both),
            (18_500_000, Side::Both)
        ]
    );
    // (every tap where a key or a stamp sounds, and the sound's own times)
    let heard: Vec<u64> = s.list.iter().map(|h| h.0.as_micros() as u64).collect();
    assert_eq!(s.taps.iter().map(|t| t.0).collect::<Vec<_>>(), heard);
    let hands = Hands::new(&s.taps);
    let at = |us: u64| hands.pose(Program::frame_at(us), false);
    assert_eq!(
        [at(17_500_000), at(17_800_000), at(18_100_000), at(18_500_000), at(7_000_000)],
        [Pose::Left, Pose::Right, Pose::Both, Pose::Both, Pose::Both]
    );
    // (between them the hands up; where the 4× key was, no tap: watching)
    assert_eq!(at(17_700_000), Pose::Up);
    let fast = p.video_of(c.map.video(5_000)).unwrap();
    assert_eq!(mascot::pose_at(&hands, Program::frame_at(fast), &p.frame(fast)), Some(Pose::Watch));
    assert_eq!(hands.taps(), 5);
}

#[test]
fn a_hand_stays_down_its_hold_and_the_same_hand_again_comes_up_one_frame_first() {
    let frame = 1_000_000 / video::FPS;
    // (alone: down three frames, then up)
    let one = Hands::new(&[(1_000_000, Side::Left)]);
    let k = Program::frame_at(1_000_000);
    let poses: Vec<Pose> = (k..k + 4).map(|f| one.pose(f, false)).collect();
    assert_eq!(poses, [Pose::Left, Pose::Left, Pose::Left, Pose::Up]);
    // (the same hand twice in 60 ms, and in 20 ms: an up frame between)
    for gap in [60_000, 20_000] {
        let h = Hands::new(&[(0, Side::Right), (gap, Side::Right)]);
        let poses: Vec<Pose> = (0..4).map(|f| h.pose(f, false)).collect();
        assert_eq!(poses, [Pose::Right, Pose::Up, Pose::Right, Pose::Right], "{gap} µs apart");
    }
    // (the other hand is no wait: both down together)
    let h = Hands::new(&[(0, Side::Left), (frame, Side::Right)]);
    assert_eq!((h.pose(0, false), h.pose(1, false)), (Pose::Left, Pose::Both));
    // (the space bar: the hand that didn't tap last)
    let h = Hands::new(&[(0, Side::Left), (500_000, Side::Space), (1_000_000, Side::Space)]);
    assert_eq!(h.pose(Program::frame_at(500_000), false), Pose::Right);
    assert_eq!(h.pose(Program::frame_at(1_000_000), false), Pose::Left);
}

#[test]
fn the_mascot_watches_after_the_quiet_and_on_a_sped_span_and_never_on_a_card() {
    let quiet = mascot::QUIET_MS * video::FPS / 1000;
    let h = Hands::new(&[(0, Side::Left)]);
    assert_eq!((h.pose(quiet - 1, false), h.pose(quiet, false)), (Pose::Up, Pose::Watch));
    assert_eq!(Hands::new(&[]).pose(10, false), Pose::Watch, "no key yet");
    // (typing again: the first key is a tap)
    let h = Hands::new(&[(0, Side::Left), (5_000_000, Side::Right)]);
    assert_eq!(h.pose(Program::frame_at(5_000_000) - 1, false), Pose::Watch);
    assert_eq!(h.pose(Program::frame_at(5_000_000), false), Pose::Right);
    // (on a sped span watching at once, a key a moment before; at 1× and at the 2× fit not)
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let near = |t: u64| {
        let v = p.video_of(c.map.video(t)).unwrap();
        let h = Hands::new(&[(v - 200_000, Side::Left)]);
        mascot::pose_at(&h, Program::frame_at(v), &p.frame(Program::time_of(Program::frame_at(v))))
    };
    assert_eq!(
        (near(5_000), near(15_000), near(53_000)),
        (Some(Pose::Watch), Some(Pose::Up), Some(Pose::Up))
    );
    // (a card: none)
    let h = Hands::new(&[(4_000_000, Side::Both)]);
    let built = Program::frame_at(4_000_000);
    assert_eq!(p.frame(Program::time_of(built)).card, Some(Card::Built));
    assert_eq!(mascot::pose_at(&h, built, &p.frame(Program::time_of(built))), None);
    let second = p.parts.iter().find(|x| x.what == What::Chapter(1)).unwrap();
    assert_eq!(mascot::pose_at(&h, Program::frame_at(second.at), &p.frame(second.at + 1)), None);
}

/// Five made-up poses at the box's size, their ink from column `ink` in rows 20 on.
fn art(ink: usize) -> Art {
    let (w, h) = mascot::BOX;
    let mut px = vec![0u8; w * h * 4];
    for y in 20..h {
        for x in ink..w {
            px[(y * w + x) * 4..][..4].copy_from_slice(&[200, 200, 200, 255]);
        }
    }
    Art { poses: vec![(w, h, px); 5], dir: "made-up".into() }
}

#[test]
fn the_band_s_marks_and_captions_stay_clear_of_the_mascot_and_it_never_covers_a_cell() {
    use crate::shot::PAD;
    let (x0, pw) = (33, video::W - 66);
    let place = mascot::place(&art(60), (x0, pw), 870).unwrap();
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
    let e = mascot::place(&art(60), (x0, pw), y + 49).unwrap_err();
    assert!(e.contains("49 px over the screen's last row of cells"), "{e}");
    // (the marks drawn: ink left of the gap, none from it to the frame's edge — a sped span's, where it ends')
    let fonts = crate::shot::Fonts::load(1).unwrap();
    let look = video::look(Bg::Dark);
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    for f in [at_take(&p, 5_000, 0), p.frame(7_000_000)] {
        let mut px = look.ground.repeat(video::W * video::H);
        let mut words = super::draw::Words::new(&fonts, look.ground);
        super::draw::band(&mut px, &f, &c, (x0, place.marks_right), &look, &mut words);
        let inked = |xs: std::ops::Range<usize>| {
            (video::H - video::BAND..video::H)
                .any(|y| xs.clone().any(|x| px[(y * video::W + x) * 3..][..3] != look.ground))
        };
        assert!(inked(place.x - mascot::GAP - 60..place.x - mascot::GAP), "the mark drawn: {f:?}");
        assert!(!inked(place.x - mascot::GAP..video::W), "clear of the box: {f:?}");
    }
    // (a caption: clear of the widest speed mark over it, else of the box; one that would reach it refused)
    let boxed = (place.marks_right, true);
    assert_eq!(super::draw::captions_clear(&c, &fonts, boxed), Ok(()));
    let long = format!("[[caption]]\nat = \"5s\"\nfor = \"1s\"\ntext = \"{}\"\n", "x".repeat(60));
    let text = CUT.replacen("[[caption]]", &format!("{long}\n[[caption]]"), 1);
    let e = super::draw::captions_clear(&cut(&text).unwrap(), &fonts, boxed).unwrap_err();
    let line = text.lines().position(|l| l.starts_with("[[caption]]")).unwrap() + 1;
    assert!(e.starts_with(&format!("cut.toml:{line}: caption:")), "{e}");
    assert!(e.contains("reaches the speed's mark `▸▸ ×4`") && e.contains("px left there"), "{e}");
    let fits = text.replacen(&"x".repeat(60), &"x".repeat(40), 1);
    assert_eq!(super::draw::captions_clear(&cut(&fits).unwrap(), &fonts, boxed), Ok(()));
}

#[test]
fn no_frames_no_mascot_and_it_says_so() {
    let c = cut(CUT).unwrap();
    let s = tarae_testkit::scratch::Scratch::new("xtask-cut-mascot");
    let e = mascot::found(&c, Some(s.join("none")), (33, 1854), 870).err().unwrap();
    assert!(e.starts_with("the mascot: none — ") && e.ends_with("; the cut is made without it"), "{e}");
    assert!(e.contains("frame-up.png"), "{e}");
    let e = mascot::found(&c, None, (33, 1854), 870).err().unwrap();
    assert!(e.contains("no main checkout found"), "{e}");
    let off = cut(&format!("mascot = false\n{CUT}")).unwrap();
    assert_eq!(
        mascot::found(&off, Some(s.join("none")), (33, 1854), 870).err().unwrap(),
        "the mascot: none (mascot = false)"
    );
    let light = cut(&format!("bg = \"light\"\n{CUT}")).unwrap();
    assert!(mascot::found(&light, None, (33, 1854), 870).err().unwrap().contains("meok's only"));
    assert!(cut(&format!("mascot = 1\n{CUT}")).unwrap_err().contains("mascot: true or false"));
}
