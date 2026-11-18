//! A Short's form (`short.rs`): its take's size, what its list has none of, its band's three things, its lines' width,
//! its honest marks on every sped frame, its length — on the made-up take, no tarae, no claude.

use super::*;
use crate::cut::draw::{Stage, Words};
use crate::cut::short::{self as form, Band};
use crate::shot::{Fonts, PAD};
use crate::video::{SHORT, short_window};

/// The made-up take as a Short: the hook 3 s from the second card, 0–10 s at the default 4×, 10–20 s at 1×, 20–50 s
/// left out, the rest 4×; a caption; the title in two lines.
const SHORT_CUT: &str = r#"form = "short"
default = 4
title = ["halil 0.2.0", "cargo install halil"]

[opening]
at = "card 2 shown"
for = "3s"
hook = "Four notes on halil. One minute."

[[span]]
from = "turn 1 start"
to = "20s"
speed = 1

[[span]]
from = "20s"
to = "50s"
out = true

[[caption]]
at = "card 1 shown"
for = "2s"
text = "A card."
"#;

fn fonts() -> Fonts {
    Fonts::load(1).unwrap()
}

/// The line (from 1) of the first of `text`'s lines that starts with `key`.
fn line_of(text: &str, key: &str) -> usize {
    text.lines().position(|l| l.trim_start().starts_with(key)).unwrap() + 1
}

/// A Short's take is `video::short_size`'s, 80×34; another is refused with both sizes in the words, nothing scaled.
#[test]
fn a_shorts_take_of_another_size_is_refused_saying_both() {
    let fonts = fonts();
    assert_eq!(video::short_size().unwrap(), (80, 34));
    let lay = form::layout(&fonts, (80, 34)).unwrap();
    let (wx, wy, ww, wh) = short_window();
    assert!(lay.short && lay.dims == SHORT);
    assert!(lay.at.0 >= wx && lay.at.0 + lay.pic.0 <= wx + ww, "across the window: {lay:?}");
    assert!(lay.at.1 >= wy && lay.at.1 + lay.pic.1 <= wy + wh, "down the window: {lay:?}");
    for size in [(166, 32), (80, 33), (81, 34), (120, 34)] {
        let e = form::layout(&fonts, size).unwrap_err();
        let (a, b) = (format!("{}×{}", size.0, size.1), "80×34");
        assert!(e.contains(&a) && e.contains(b), "{size:?}: {e}");
    }
}

/// What a Short has none of — the card's words and a picture where it ends, chapters, pictures, a route and its stations,
/// a bed's chapter, key pops — each refused by its line; a wide list's title too.
#[test]
fn each_key_a_short_has_none_of_is_refused_by_its_line() {
    assert!(cut(SHORT_CUT).is_ok(), "{:?}", cut(SHORT_CUT).err());
    let hook = "hook = \"Four";
    let refused = |text: String, key: &str| {
        let e = cut(&text).unwrap_err();
        let at = format!("cut.toml:{}:", line_of(&text, key));
        assert!(e.starts_with(&at), "{key}: {e} — not at {at}");
    };
    for (key, value) in [
        ("name", "\"halil\""),
        ("line", "\"A task manager.\""),
        ("code", "[\"halil board\"]"),
        ("label", "\"The result\""),
        ("label-line", "\"shown next\""),
        ("picture", "\"pictures/one.png\""),
    ] {
        refused(SHORT_CUT.replace(hook, &format!("{key} = {value}\n{hook}")), key);
    }
    for (head, table) in [
        ("[[chapter]]", "[[chapter]]\nat = \"start\"\ntitle = \"One\"\n"),
        ("[[picture]]", "[[picture]]\nat = \"start\"\n"),
        ("[route]", "[route]\nstations = [\"The room\"]\n"),
        ("[[station]]", "[[station]]\npicture = 1\n"),
    ] {
        refused(format!("{SHORT_CUT}\n{table}"), head);
    }
    refused(format!("{SHORT_CUT}\n[[music]]\nchapter = 1\nfile = \"bed.mp3\"\n"), "chapter = 1");
    let two = format!("{SHORT_CUT}\n[[music]]\nfile = \"a.mp3\"\n\n[[music]]\nfile = \"b.mp3\"\n");
    let e = cut(&two).unwrap_err();
    let second = two.lines().enumerate().filter(|(_, l)| *l == "[[music]]").nth(1).unwrap().0 + 1;
    assert!(e.starts_with(&format!("cut.toml:{second}:")), "{e}");
    refused(format!("{SHORT_CUT}\n[mascot]\nkeys = \"all\"\n"), "keys =");
    refused(format!("title = \"halil\"\n{CUT}"), "title");
    // (the Short's own: says on and no key pops if left out; a bed from the first frame)
    let c = cut(&format!("{SHORT_CUT}\n[[music]]\nfile = \"a.mp3\"\n")).unwrap();
    assert!(c.mascot.says && c.mascot.keys == mascot::Keys::Off);
    assert_eq!(c.beds[0].chapter, 0);
    assert_eq!(c.short.as_ref().unwrap().title, ["halil 0.2.0", "cargo install halil"]);
}

/// The hook first — where it ends alone, its line from the first frame — and the brand last: no intro, no card of what's
/// built, the last screen only its crossing.
#[test]
fn a_short_opens_on_its_hook_and_ends_on_the_brand() {
    let c = cut(SHORT_CUT).unwrap();
    let p = Program::new(&c);
    let whats: Vec<What> = p.parts.iter().map(|q| q.what).collect();
    assert!(matches!(whats[..], [What::Ends { .. }, What::Take { .. }, What::Last, What::End]), "{whats:?}");
    assert_eq!(p.parts[2].len, camera::CROSS_MS * 1000);
    assert_eq!(p.parts[3].len, video::BRAND_MS * 1000);
    let first = p.frame(0);
    assert!(first.ends && first.hook == Some(255), "{first:?}");
    assert_eq!(p.frame(p.parts[1].at).hook, None);
    assert_eq!(p.frame(p.total - 1).card, Some(Card::Brand((p.total - 1 - p.parts[3].at) / 1000)));
}

/// Texts of every length up to one past two of the band's lines: words of four letters, the last cut short.
fn every_length(cols: usize) -> Vec<String> {
    (1..=2 * cols + 2)
        .map(|n| "abcd ".repeat(n).chars().take(n).collect::<String>().trim().to_string())
        .collect()
}

/// The band's three things — the time row, the mascot, the line's box — never over one another at any length the form
/// accepts, all in the safe area under the window; and drawn, nothing changes past the band.
#[test]
fn the_bands_three_things_never_overlap_nor_leave_the_safe_area() {
    let fonts = fonts();
    let b = Band::new(&fonts);
    let (x0, y0, w, h) = b.at;
    let (_, wy, _, wh) = short_window();
    assert_eq!(y0, wy + wh, "the band under the window");
    type Rect = (usize, usize, usize, usize);
    let inside = |r: Rect| form::safe(r.0, r.1) && form::safe(r.0 + r.2 - 1, r.1 + r.3 - 1);
    let apart =
        |a: Rect, c: Rect| a.0 + a.2 <= c.0 || c.0 + c.2 <= a.0 || a.1 + a.3 <= c.1 || c.1 + c.3 <= a.1;
    let row: Rect = (x0, y0, w, b.row);
    let mascot: Rect = (b.mascot.0, b.mascot.1, mascot::BOX.0, mascot::BOX.1);
    assert!(inside(row) && inside(mascot) && apart(row, mascot), "{row:?} {mascot:?}");
    // (the character at the band's left end; every line `mascot::GAP` past its box, up to the band's right edge)
    assert_eq!(
        (mascot.0, b.line.0),
        (x0, x0 + mascot.2 + mascot::GAP),
        "the box's left edge, the line's left"
    );
    assert_eq!(b.line.1, x0 + w, "the line's right at most");
    assert_eq!((x0, x0 + w), (video::SAFE.3, SHORT.0 - video::SAFE.1), "the safe area's edges");
    // (the character placed by its ink: of made-up pictures with air round their ink, each its own — a typing paw
    // short of the box, a swing's brush further left, a stretch lower — no frame's ink past the band's corner, and
    // some frame's reaching it; clear of the row and of the line's room)
    let picture = |(x1, x2, y1, y2): (usize, usize, usize, usize)| {
        let (bw, bh) = mascot::BOX;
        let mut px = vec![0u8; bw * bh * 4];
        for y in y1..y2 {
            for x in x1..x2 {
                px[(y * bw + x) * 4 + 3] = 255;
            }
        }
        (bw, bh, px)
    };
    let mut pics = vec![None; mascot::Pic::COUNT];
    pics[0] = Some(picture((25, 188, 12, 150)));
    pics[1] = Some(picture((12, 196, 30, 146)));
    pics[2] = Some(picture((30, 170, 25, 158)));
    let art = mascot::Art { pics, dir: Default::default(), scale: video::Scale::HD, marks: Vec::new() };
    let at = form::place(&art).unwrap();
    let inked: Vec<(usize, usize)> = (art.pics.iter().flatten())
        .flat_map(|(pw, ph, px)| {
            (0..pw * ph).filter(move |i| px[i * 4 + 3] > 25).map(move |i| (at.x + i % pw, at.y + i / pw))
        })
        .collect();
    let (left, bottom) = inked.iter().fold((usize::MAX, 0), |m, &(x, y)| (m.0.min(x), m.1.max(y + 1)));
    assert_eq!((left, bottom), (x0, y0 + h), "the ink's corner on the band's, x 48 and y 1520");
    let (right, top) = inked.iter().fold((0, usize::MAX), |m, &(x, y)| (m.0.max(x + 1), m.1.min(y)));
    assert!(
        right + mascot::GAP <= b.line.0 && top >= y0 + b.row,
        "the ink to ({right}, {top}): over the line or row"
    );
    assert!(at.x <= mascot.0 && at.y >= mascot.1, "by its ink, only ever further left and down: {at:?}");
    // (the row's widest words, their ink rows, inside it and clear of each other)
    let ink = |t: &str| {
        let p = video::text(t, crate::cut::draw::MARK_PX, [255; 3], [0; 3], false, &fonts);
        (p.0, p.1 - 2 * PAD)
    };
    let (clock, jump, mark) = (ink("9:59:59"), ink("+9 h 59 min"), ink(crate::cut::draw::ENDS));
    assert!(x0 - PAD + clock.0 + jump.0 <= x0 + w + PAD - mark.0, "the row's words meet");
    assert!(clock.1 <= b.row && mark.1 <= b.row, "the row's words {clock:?} {mark:?} in {} px", b.row);
    let mut accepted = 0;
    let widest = format!("{} {}", "x".repeat(b.cols), "x".repeat(b.cols));
    for text in every_length(b.cols).into_iter().chain([widest]) {
        let Ok(lines) = form::lines(&text, b.cols) else { continue };
        accepted += 1;
        let (bw, bh) = video::caption_box(&form::caption_pic(&lines, &fonts));
        let boxed: Rect = (b.line.0, b.line.2 - bh / 2, bw, bh);
        assert!(inside(boxed), "“{text}”: {boxed:?} past the safe area");
        assert!(apart(boxed, row) && apart(boxed, mascot), "“{text}”: {boxed:?} over {row:?} or {mascot:?}");
        assert!(
            boxed.0 >= mascot.0 + mascot.2 + mascot::GAP,
            "“{text}”: {boxed:?} nearer the box than the gap"
        );
        assert!(boxed.0 + bw <= x0 + w, "“{text}”: {boxed:?} past the band's right edge");
        assert!(boxed.1 + bh <= y0 + h, "“{text}”: under the band");
    }
    // (every one-line length, and two lines' as the words break them)
    assert!(accepted > b.cols + 1, "{accepted} lengths accepted of {}", 2 * b.cols + 3);

    // (drawn: a full screen, the widest line, the clock, a jump and the speed — every px they change in the band)
    let c = cut(SHORT_CUT).unwrap();
    let lay = form::layout(&fonts, (80, 34)).unwrap();
    let mut s = vt100::Parser::new(34, 80, 0);
    for r in 0..34 {
        s.process(format!("\x1b[{};1H{}", r + 1, "█".repeat(80)).as_bytes());
    }
    let pic = crate::shot::render_scaled(s.screen(), &fonts, lay.glyph, 1);
    let look = video::look(Bg::Dark);
    let stage = Stage::new(lay, &look, &c, None);
    let none = (None, &mascot::pops::Pops::default(), &[][..]);
    let mut words = Words::new(&fonts, look.ground, video::Scale::HD);
    let bare = Frame { screen: Some(0), ..Frame::none() };
    let widest = format!("{} {}", "x".repeat(b.cols), "x".repeat(b.cols));
    let wide = SHORT_CUT.replace("A card.", &widest).replace("for = \"2s\"", "for = \"3s\"");
    let c_wide = cut(&wide).unwrap();
    let stage_c = Stage::new(lay, &look, &c_wide, None);
    let said = Frame {
        clock: Some(35_999_000),
        speed: Some(16.0),
        jump: Some((7_140_000, 255)),
        caption: Some((0, 255)),
        ..bare
    };
    let a = stage.take_frame(&Shown::Whole(&pic), &bare, none, &mut words);
    let d = stage_c.take_frame(&Shown::Whole(&pic), &said, none, &mut words);
    let changed: Vec<(usize, usize)> = (0..SHORT.0 * SHORT.1)
        .filter(|i| a[i * 3..][..3] != d[i * 3..][..3])
        .map(|i| (i % SHORT.0, i / SHORT.0))
        .collect();
    assert!(!changed.is_empty());
    for &(x, y) in &changed {
        assert!(form::safe(x, y) && (y0..y0 + h).contains(&y), "({x}, {y}) changed outside the band");
        assert!(
            x < b.mascot.0 || x >= b.mascot.0 + mascot::BOX.0 || y < b.mascot.1,
            "({x}, {y}) on the mascot's box"
        );
    }
}

/// The band's widest two lines fit beside the mascot (`Band::cols` a line, about 18); one character more is refused,
/// with its text and count, by its line.
#[test]
fn a_two_line_caption_of_the_widest_length_fits_and_one_more_is_refused() {
    let fonts = fonts();
    let b = Band::new(&fonts);
    assert_eq!(b.cols, 18, "18 a line, the character on either side");
    let widest = format!("{} {}", "x".repeat(b.cols), "x".repeat(b.cols));
    let lines = form::lines(&widest, b.cols).unwrap();
    assert_eq!(lines.len(), 2);
    let (bw, _) = video::caption_box(&form::caption_pic(&lines, &fonts));
    assert!(b.line.0 + bw <= b.line.1, "{bw} px from {} past {}", b.line.0, b.line.1);
    let more = format!("{widest}x");
    let e = form::lines(&more, b.cols).unwrap_err();
    assert!(e.contains(&more) && e.contains(&format!("{} characters", more.chars().count())), "{e}");
    // (and a third line of words, none past the width, refused by the count)
    let three = vec!["x".repeat(b.cols); 3].join(" ");
    let e = form::lines(&three, b.cols).unwrap_err();
    assert!(e.contains("3 lines"), "{e}");
    // (and the cut refuses it before a frame, by its line — a hook as a caption)
    let text = SHORT_CUT.replace("text = \"A card.\"", &format!("text = \"{more}\""));
    let c = Cut::parse(&text.replace("for = \"2s\"", "for = \"3s\""), "cut.toml", &marks()).unwrap();
    let e = form::check(&c, &fonts).unwrap_err();
    assert!(e.starts_with(&format!("cut.toml:{}:", line_of(&text, "[[caption]]"))), "{e}");
    let ok = text.replace(&more, &widest).replace("for = \"2s\"", "for = \"3s\"");
    form::check(&Cut::parse(&ok, "cut.toml", &marks()).unwrap(), &fonts).unwrap();
    let hook = SHORT_CUT.replace("Four notes on halil. One minute.", &more);
    let e = form::check(&cut(&hook).unwrap(), &fonts).unwrap_err();
    assert!(e.starts_with(&format!("cut.toml:{}:", line_of(&hook, "hook"))), "{e}");
}

/// Every frame of a sped span says the take's real time and the speed's mark — by the frame, and in its pixels: ink at
/// the row's left and at its right.
#[test]
fn the_real_time_and_the_speed_mark_are_in_every_frame_of_a_sped_span() {
    let fonts = fonts();
    let b = Band::new(&fonts);
    let c = cut(SHORT_CUT).unwrap();
    let p = Program::new(&c);
    let look = video::look(Bg::Dark);
    let mut words = Words::new(&fonts, look.ground, video::Scale::HD);
    let (x0, _, w, _) = b.at;
    let inked = |px: &[u8], xs: std::ops::Range<usize>| {
        (b.row_mid - 12..b.row_mid + 12)
            .any(|y| xs.clone().any(|x| px[(y * SHORT.0 + x) * 3..][..3] != look.ground))
    };
    let mut sped = 0;
    for k in 0..p.frames() {
        let v = Program::time_of(k);
        let What::Take { from } = p.part(v).what else { continue };
        let (_, i) = c.map.take(from + v - p.part(v).at);
        let Pace::Speed(s) = c.map.segs()[i].pace else { continue };
        if s <= 1.0 {
            continue;
        }
        sped += 1;
        let f = p.frame(v);
        assert!(f.clock.is_some() && f.speed == Some(s), "frame {k}: {f:?}");
        let mut px = look.ground.repeat(SHORT.0 * SHORT.1);
        form::band(&mut px, &f, &c, &look, &mut words);
        assert!(inked(&px, x0..x0 + 120), "frame {k}: no clock");
        assert!(inked(&px, x0 + w - 120..x0 + w), "frame {k}: no speed's mark");
    }
    assert!(sped > 30, "{sped} sped frames");
}

/// A Short runs up to `video::SHORT_MAX_MS` (180 s, YouTube's): exactly that passes, a frame more is refused.
#[test]
fn a_short_runs_up_to_180_s_and_a_frame_more_is_refused() {
    assert_eq!(video::SHORT_MAX_MS, 180_000);
    // (the hook, the take's 120 s at 1×, the crossing and the brand: the hook makes it up)
    let list = |hook_ms: u64| {
        format!(
            "form = \"short\"\ndefault = 1\ntitle = \"halil\"\n\n[opening]\nat = \"start\"\nfor = \"{hook_ms}ms\"\nhook = \"Four notes.\"\n"
        )
    };
    let rest = 120_000 + camera::CROSS_MS + video::BRAND_MS;
    let at_most = video::SHORT_MAX_MS - rest;
    let c = cut(&list(at_most)).unwrap();
    let p = Program::new(&c);
    assert_eq!(p.frames(), video::SHORT_MAX_MS * FPS / 1000);
    form::length(&p).unwrap();
    let c = cut(&list(at_most + 1000 / FPS)).unwrap();
    let p = Program::new(&c);
    assert_eq!(p.frames(), video::SHORT_MAX_MS * FPS / 1000 + 1);
    let e = form::length(&p).unwrap_err();
    assert!(e.contains("180 s"), "{e}");
}

/// A Short's zoom: the target's middle at the eye's height where the screen lets it (within the row its hold moves it
/// to stand on whole rows), never nearer than `video::SHORT_PX_MAX` draws it.
#[test]
fn a_shorts_zoom_puts_its_target_at_the_eye_never_past_its_glyph() {
    let fonts = fonts();
    let lay = form::layout(&fonts, (80, 34)).unwrap();
    let t = Region { x: 30.0, y: 16.0, w: 6.0, h: 2.0 };
    let r = form::window(t, (80, 34), lay.glyph, &fonts);
    let eye = (t.y + t.h / 2.0 - r.y) / r.h;
    assert!((eye - video::short_eye()).abs() <= 1.0 / r.h, "{eye} against {}", video::short_eye());
    let view = lay.view(r, (80, 34), &fonts, None);
    assert!(view.px <= video::SHORT_PX_MAX as f32, "drawn at {} px", view.px);
    assert!((r.w / r.h - 80.0 / 34.0).abs() < 1e-3, "the screen's shape: {r:?}");
}

/// A held zoom's window, as the frame draws it (`Layout::view`, the screen's picture and its margin), starts and ends
/// on whole rows — no glyph cut across at its top or bottom (the proof's card read held with a sliver of the row above
/// it) —, its target shown, for targets all over the screen: the scenes' Short camera's snap (`camera::on_rows`).
#[test]
fn a_held_zooms_window_starts_and_ends_on_whole_rows() {
    let fonts = fonts();
    let lay = form::layout(&fonts, (80, 34)).unwrap();
    let mut checked = 0;
    for (w, h) in [(6.0, 2.0), (17.0, 1.0), (40.0, 7.0), (12.0, 3.4)] {
        for y in (0..34).map(|y| y as f32 * 0.97).filter(|y| y + h <= 34.0) {
            for x in [0.0, 5.0, 31.3, 80.0 - w] {
                let t = Region { x, y, w, h };
                let r = form::window(t, (80, 34), lay.glyph, &fonts);
                for glyph in [None, Some(video::SHORT_PX_MAX as f32)] {
                    let v = lay.view(r, (80, 34), &fonts, glyph);
                    assert!(v.whole_rows(lay.pic.1, 34), "{t:?}: {r:?} shows rows {:?}", v.rows(lay.pic.1));
                }
                let s = crate::camera::shown(r, lay_frame(&lay, &fonts));
                assert!(s.y <= t.y + 1e-3 && t.y + t.h <= s.y + s.h + 1e-3, "{t:?} not shown in {r:?}");
                checked += 1;
            }
        }
    }
    assert!(checked > 300, "{checked} targets");
}

/// A held zoom's window, as the frame draws it, starts on a whole column — no glyph cut down at its left (the halil
/// Short's first read: “Nothing here” a cell in from a board column's left, the cell a column cut in half would hide) —,
/// its target shown, for targets all over the screen: the scenes' Short camera's snap (`camera::on_cells`). Its right
/// edge is where the frame's shape puts it (whole rows and the frame's shape leave one edge across to choose).
#[test]
fn a_held_zooms_window_starts_on_a_whole_column() {
    let fonts = fonts();
    let lay = form::layout(&fonts, (80, 34)).unwrap();
    let mut checked = 0;
    for (w, h) in [(6.0, 2.0), (17.0, 1.0), (40.0, 7.0), (12.0, 3.4)] {
        for x in (0..80).map(|x| x as f32 * 0.97).filter(|x| x + w <= 80.0) {
            for y in [0.0, 5.0, 16.3, 34.0 - h] {
                let t = Region { x, y, w, h };
                let r = form::window(t, (80, 34), lay.glyph, &fonts);
                for glyph in [None, Some(video::SHORT_PX_MAX as f32)] {
                    let v = lay.view(r, (80, 34), &fonts, glyph);
                    assert!(v.whole_left(lay.pic.0), "{t:?}: {r:?} shows columns {:?}", v.cols(lay.pic.0));
                }
                let s = crate::camera::shown(r, lay_frame(&lay, &fonts));
                assert!(s.x <= t.x + 1e-3 && t.x + t.w <= s.x + s.w + 1e-3, "{t:?} not shown in {r:?}");
                checked += 1;
            }
        }
    }
    assert!(checked > 300, "{checked} targets");
}

/// How the frame shows a window on a Short's layout (`video::frame_of` of the screen's picture).
fn lay_frame(lay: &crate::cut::draw::Layout, fonts: &Fonts) -> crate::camera::Frame {
    video::frame_of((80, 34), lay.pic, lay.glyph, |p| crate::shot::cell(fonts, p)).1
}
