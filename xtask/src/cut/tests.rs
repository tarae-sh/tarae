//! `cut`'s tests: made-up takes (landmarks written by hand, screens drawn by their own bytes) — no tarae, no claude,
//! nothing that waits.

use serde_json::json;

use super::draw::{label, lost_zooms, wide_window};
use super::init::{Facts, rules, stills, ticking, written};
use super::sound::{
    BED_REF_LUFS, Mix, Where, bed_gains, heard_at, lay, layout, mixed, realtime, sounds, under,
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

fn cut(text: &str) -> Result<Cut> {
    Cut::parse(text, "cut.toml", &marks())
}

/// The made-up cut: 0–10 s at the default 4×, 10–20 s at 1×, 20–50 s left out, 50–56 s fit in 3 s, the rest 4×; a
/// hook of 3 s from 55 s; chapters at the start and at `fix`; a caption; a zoom.
const CUT: &str = r#"
title = "made up"
default = 4

[hook]
at = "card 2 shown"
for = "3s"
title = "Where it ends"

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
        ("title = \"x\"\n", "cut.toml:1: no default"),
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
    // (the hook: the take from 55 s at 1×, from later; then the title card, no clock)
    let hook = p.frame(1_000_000);
    assert_eq!((hook.clock, hook.speed, hook.later, hook.jump), (Some(56_000), None, true, None));
    let title = p.frame(4_000_000);
    assert_eq!((title.card, title.clock, title.speed), (Some(Card::Title), None, None));
    // (a sped stretch: its factor; at 1× none; the clock always the take's)
    let fast = at_take(&p, 5_000, 0);
    assert_eq!((fast.clock, fast.speed, fast.later), (Some(5_000), Some(4.0), false));
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
    assert!(label(&p, 1_000_000, &hook, false, &[]).contains("take 0:00:56  1× from later"));
}

#[test]
fn the_parts_cross_and_chapters_txt_says_each_card_s_time() {
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let whats: Vec<What> = p.parts.iter().map(|x| x.what).collect();
    assert_eq!(
        whats,
        [
            What::Hook { from: 55_000 },
            What::Title,
            What::Chapter(0),
            What::Take { from: 0 },
            What::Chapter(1),
            What::Take { from: 12_500_000 },
            What::Last,
            What::End
        ]
    );
    // (hook 3 s, title 2 s, chapter 2 s; the take's 12.5 s to `fix`, its card; the rest 19 s)
    assert_eq!(p.parts[4].at, 3_000_000 + 2_000_000 + 2_000_000 + 12_500_000);
    let (txt, said) = p.chapters();
    assert_eq!(txt, "0:00 Where it ends\n0:05 One\n0:19 Two\n");
    assert!(said.iter().any(|s| s.contains("“Where it ends” runs 5.0 s")), "{said:?}");
    // (each part's start crosses, and the jump at 20 s does — where `fix`'s card stands, one crossing)
    let crossings = p.crossings();
    for x in &p.parts[1..] {
        assert!(crossings.contains(&x.at), "{x:?} in {crossings:?}");
    }
    // (no hook's title: the first chapter covers the hook and the title card)
    let plain = cut(&CUT.replace("title = \"Where it ends\"\n", "")).unwrap();
    assert_eq!(Program::new(&plain).chapters().0, "0:00 One\n0:19 Two\n");
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
            What::Hook { from: 55_000 },
            What::Title,
            What::Take { from: 0 },
            What::Chapter(1),
            What::Take { from: 12_500_000 },
            What::Last,
            What::End
        ],
        "no card for chapter 1: the title card, then the take"
    );
    // (its number and title in the band from the take's first frame, standing, then gone after 3 s)
    let at = p.chapter_at(0);
    assert_eq!(at, 5_000_000, "after the hook and the title card");
    assert_eq!(p.frame(at + 1_500_000).chapter, Some((0, 255)));
    assert_eq!(p.frame(at + FADE_MS * 500).chapter.map(|c| c.0), Some(0), "fading in");
    assert_eq!(p.frame(at + CHAPTER_BAND_MS * 1000 + 40_000).chapter, None, "gone");
    assert_eq!(p.frame(p.parts[3].at + 500_000).chapter, None, "a card is a card");
    assert_eq!(p.chapters().0, "0:00 Where it ends\n0:05 One\n0:17 Two\n");
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
    // (hook 3 s, title 2 s, chapter 1's card 2 s; the take's 10 s at 4× is 2.5 s, then 4 s at 1×)
    let ks: Vec<(u64, Heard)> = s
        .list
        .iter()
        .filter(|h| matches!(h.1, Heard::Key(_)))
        .map(|(t, h)| (t.as_micros() as u64, h.clone()))
        .collect();
    let fit = p.video_of(c.map.video(52_000)).unwrap();
    assert_eq!(ks, [(13_500_000, Heard::Key('k')), (14_000_000, Heard::Key('\r')), (fit, Heard::Key(' '))]);
    assert_eq!((s.keys, s.fast, s.out), (3, 1, 1));
    assert_eq!(heard_at(&p, 5_000), Where::Fast);
    assert_eq!(heard_at(&p, 30_000), Where::Out);
    // (in the hook: heard there, and again in the take where it's shown slowly enough — 55.5 s is the 2× fit's)
    assert_eq!(heard_at(&p, 55_500), Where::At(vec![500_000, p.video_of(c.map.video(55_500)).unwrap()]));
    assert_eq!(heard_at(&p, 57_000), Where::At(vec![2_000_000]), "the hook only: the take shows it at 4×");
}

#[test]
fn the_stamp_sounds_once_where_a_card_is_answered_its_key_its_own() {
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    let s = sounds(&p, &[key(14_950, b"y"), key(14_000, b"x")], &marks());
    let at = |h: &Heard| -> Vec<u64> {
        s.list.iter().filter(|x| &x.1 == h).map(|x| x.0.as_micros() as u64).collect()
    };
    // (card 1 answered at 15 s, at 1×: 7 s + 2.5 s + 5 s; card 2 at 56 s: the take's 4× there, the hook's 1× at 1 s)
    assert_eq!(at(&Heard::Stamp), [1_000_000, 14_500_000]);
    assert_eq!(
        at(&Heard::Key('k')),
        [13_500_000],
        "the y that answered is the stamp's; the x before it its own"
    );
    assert_eq!((s.stamps, s.stamps_missed), (2, 0));
    let end = p.parts.iter().find(|x| x.what == What::End).unwrap().at;
    assert_eq!(s.brush, Some(end), "the brush on the end card");
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
    // (laid out: the first in over half a second from 0:00; crossed into the second over chapter 2's card)
    let card = p.parts.iter().find(|x| x.what == What::Chapter(1)).unwrap();
    let (placed, said) = layout(&p, &[(secs(60.0), 0), (secs(5.0), 0)]);
    assert!(said.is_empty(), "{said:?}");
    assert_eq!(
        (placed[0].start, placed[0].fade_in, placed[0].fade_out),
        (0, (0, 500_000), (card.at, card.at + card.len))
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
    let (placed, said) = layout(&p, &[(secs(10.0), 0), (secs(5.0), 0)]);
    assert_eq!((placed[0].ran_out, placed[0].fade_out), (Some(10_000_000), (8_000_000, 10_000_000)));
    assert_eq!((placed[1].start, placed[1].fade_in), (8_000_000, (8_000_000, 10_000_000)));
    assert!(
        said.len() == 1 && said[0].contains("runs out at 0:10.0") && said[0].contains("bed 2"),
        "{said:?}"
    );
    let mut f = vec![0.0f32; len];
    lay(&placed[0], &tone(440.0, secs(10.0)), None, 1.0, &mut f);
    let level = |from: f64| rms(&f[secs(from)..secs(from + 0.05)]);
    assert!((level(7.0) / 0.3536 - 1.0).abs() < 0.02, "before its fade, as it is: {}", level(7.0));
    assert!(level(9.0) < 0.3536 * 0.75 && level(9.0) > 0.3536 * 0.6, "half way: {}", level(9.0));
    assert!(level(9.95) < 0.02 && f[secs(10.0)..].iter().all(|x| *x == 0.0), "gone at its end");
}

#[test]
fn the_bed_drops_under_the_keys_at_a_span_s_edge_and_comes_back_after_by_the_samples() {
    let c = cut(CUT).unwrap();
    let p = Program::new(&c);
    // (the hook; the 1× span 10–20 s after the title and chapter 1's card; the 2× fit after chapter 2's card)
    assert_eq!(realtime(&p), [(0, 3_000_000), (9_500_000, 19_500_000), (21_500_000, 24_500_000)]);
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
    let back = cut(&text).unwrap();
    let read: Vec<(u64, u64, Pace)> = back.map.segs().iter().map(|s| (s.from, s.to, s.pace)).collect();
    assert_eq!(read, got, "{text}");
    assert_eq!(
        back.chapters.iter().map(|c| (c.at, c.title.as_str())).collect::<Vec<_>>(),
        [(0, "main"), (50_000, "fix")]
    );
}
