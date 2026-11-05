//! A scene as a short video (`cargo xtask scenes --video`, CLAUDE.md's "Short videos"; collab's `.sb/videos-plan.md`):
//! the take `scenes --record` makes, drawn by the shots' renderer at a steady `FPS` — each frame the screen in force at
//! its time (`screen_at`), real time throughout, the scene's own pauses its holds — on a `W`×`H` picture: the screen at
//! the largest glyph size its cells fit (`fit`), the scene's `@beat:` captions in a band of their own under it (never
//! over tarae's cells), a title card before and an end card after, the seal stamped on both (`docs/brand/`, a PNG made
//! once from its SVG). ffmpeg makes the MP4 from the raw frames piped in (H.264, 4:2:0, BT.709); where there's none,
//! said — the sheet of the video's own frames is written first all the same.
//!
//! `--shorts` makes it a YouTube Short (`Form::Short`): `SHORT` upright, the screen `SHORT_COLS` wide and as tall as
//! the window holds (`short_size`: 80×34, filling the frame's width), and over it a camera (`camera.rs`) that stands
//! still on the whole, but for an emphasis — a beat's one read (its first `@zoom:<target>`, `@zoom-deep:` where it must;
//! `camera::course`), moved in by `camera::way`, held and moved out —, the window the screen's shape with a read's target
//! at the eye's height (`short_eye`);
//! each frame the screen drawn through the camera's region at any scale (`Video::view`: rendered at the next glyph
//! size up and resampled, so a zoom never steps), two screens crossing where
//! most of the cells changed (`camera::crossing`); the screen across the frame's whole width (YouTube's buttons over its
//! right edge taken), everything else inside `SAFE`, the margins YouTube's own buttons and title cover: the window from
//! the safe area's top down to its captions' band (`SHORT_BAND`, under the screen's last row — in a zoom too, under the
//! window's) — its captions (`@note:<words> → <target>`, `captions.rs`) a phrase at a time centred in
//! the band, never over the picture, fading in and out; a `@punch` opening it on
//! `PUNCH_MS` of the take from there, then its intro in the title card's place (10-27's brush swing, its frames made
//! once — `intro_dir` —, `t a r a e` under it, the Short's own title its hook, said as the caption; `INTRO_MS`) and the
//! take from its start. Its take has tarae draw its fades and drying at the video's rate (`FRAME_MS`); and a sound track (the intro's brush — its swish and tap, `sound::brush` — then
//! `sound::track`: tarae's own strokes for the keys typed, a stamp for a card's answer, the settle where an edit
//! lands, Claude's writing as tarae's reveal plays it — the scene's `@sound:stamp`, `@sound:settle`, and its
//! `@sound:writing`/`@sound:landing` … `@sound:still` spans, `writing`) as AAC. The wide video ignores `@zoom`,
//! `@sound` and `@punch`, and stays silent.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::camera::{self, Region};
use crate::captions::{self, Px};
use crate::recording::{self, Picture};
use crate::scenes::docshots::{Bg, none_of_yours};
use crate::shot::{self, Fonts, PAD, Recorded, Rgb};
use crate::sound::{self, Heard};
use crate::{Result, root};

/// The picture: 1080p, 16:9 — what YouTube and a phone play full.
pub(crate) const W: usize = 1920;
pub(crate) const H: usize = 1080;
pub(crate) const FPS: u64 = 30;
/// A video's frame as the tarae it records draws its fades and drying (`TARAE_SHOT_FRAME_MS`, `editor::tick`).
pub(crate) const FRAME_MS: &str = "33";
const _: () = assert!(1000 / FPS == 33);
/// The captions' band under the screen, its height: a caption's row and air above and below it.
const BAND: usize = 150;
/// The air kept round the screen above the band: at its sides, and above and below it.
const SIDE: usize = 32;
const ABOVE: usize = 12;
/// A caption's glyphs, and the cards' (the spike's 34 px read well at 1080p; ≤ ~60 characters fill the band's width).
const CAPTION_PX: f32 = 34.0;
const NAME_PX: f32 = 72.0;
const LINE_PX: f32 = 40.0;
/// A caption comes in over this long (motion's short fade), then stands until the next beat.
const FADE_MS: u64 = 200;
/// The title card, the last screen held, the end card.
const TITLE_MS: u64 = 2_000;
const LAST_MS: u64 = 1_500;
const END_MS: u64 = 3_000;
/// Where ffmpeg is looked for past `PATH` (Homebrew's two homes).
const FFMPEG_AT: [&str; 2] = ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"];

/// A Short's picture: 1080×1920, 9:16 upright.
pub(crate) const SHORT: (usize, usize) = (1080, 1920);
/// What YouTube's Shorts player draws over the picture, kept clear (top, right, bottom, left, px): its bar at the top,
/// the buttons down the right edge, the title, channel and description over the bottom ~21 %. YouTube publishes no
/// pixel figures; these are the usual 1080×1920 template's, a little wider at the right and bottom.
const SAFE: (usize, usize, usize, usize) = (150, 150, 400, 48);
/// A Short's screen fills the frame's width (CLAUDE.md's Shorts bullet, the user 2026-11-05: "just fill the width" —
/// YouTube's buttons over its right edge taken): its window from the frame's left edge to its right, from the safe
/// area's top — YouTube's bar (its back arrow, search and menu) stands over the top `SAFE.0`, where the screen's first
/// row would say the path and the sessions — down as many rows as leave its captions' band, the rest of the safe area,
/// two caption lines (the user's choice, at `CAPTION_SHORT_PX`). Its height (px): 34 rows of 33 px (23 px glyphs, 13 px cells — 80 of them 1,040 px
/// of the 1,080, the whole drawn sharp at its own px) and the renderer's margin (`a_shorts_screen_fills_the_width`).
const SHORT_SCREEN_H: usize = 1_150;
/// A Short's captions' band (px): the safe area's foot under tarae's screen ("under tarae's screen", never over the code
/// or a card).
const SHORT_BAND: usize = SHORT.1 - SAFE.0 - SAFE.2 - SHORT_SCREEN_H;
/// A Short's captions (`captions.rs`, in its band): their glyphs, JetBrains Mono Bold (read on a phone held
/// upright: ~3.5 mm), the box's air round its lines across and down and its corners' radius (px), its ground and ink —
/// a quiet dark box under light words, the same on either theme, so it reads over anything.
const CAPTION_SHORT_PX: f32 = 54.0;
const CAPTION_PAD: (usize, usize) = (26, 12);
const CAPTION_R: f32 = 18.0;
const CAPTION_GROUND: Rgb = [0x0b, 0x0d, 0x11];
const CAPTION_INK: Rgb = [0xe6, 0xe1, 0xcf];
/// A Short's hook, its title said as a caption over the intro: its first word with the brush's tap.
const HOOK_FROM: u64 = 800;
/// The air between the intro's name and the caption's box (px).
const INTRO_ABOVE: usize = 48;
/// A Short's screen: a laptop's narrow terminal's columns; its rows what the window holds at their width.
pub(crate) const SHORT_COLS: u16 = 80;
/// The largest glyph a Short's camera draws (a narrow card's: ~36 columns across).
const SHORT_PX_MAX: u32 = 40;
/// A Short's title card and end card: it's swiped past in a second, so its title stands shorter.
const SHORT_TITLE_MS: u64 = 1_200;
/// A Short's intro in its title card's place (CLAUDE.md's Shorts bullet: 10-27's brush swing): its length — the swing,
/// the brush's tap on the cursor at 0.8 s, the name and the title risen under it —, then straight onto the whole. Its
/// frames are made once (`INTRO_FPS`, main's `.sb/youtube/shorts-intro/`, `intro_dir`), the last held.
const INTRO_MS: u64 = 2_200;
// (the hook is a phrase too: it stands long enough to read)
const _: () = assert!(INTRO_MS - HOOK_FROM >= captions::STAND_MIN_MS);
const INTRO_FPS: u64 = 30;
/// The name rising under the mascot (from, to — the intro's ms) by `INTRO_RISE_PX`, as 10-27's did.
const NAME_RISE: (u64, u64) = (1_100, 1_500);
const INTRO_RISE_PX: f32 = 20.0;
/// The intro's name, larger than a card's: the mascot is.
const INTRO_NAME_PX: f32 = 96.0;
const SHORT_END_MS: u64 = 2_500;
/// A `@zoom`'s target may be drawn a frame after its key: looked for this long after it.
const FIND_MS: u64 = 2_000;
/// A Short's longest: YouTube's.
const SHORT_MAX_MS: u64 = 60_000;
/// A key this soon after a `@sound:stamp` is the stamp's: it sounds as the stamp alone.
const STAMP_KEY_MS: u64 = 150;
/// How wide a phone's screen is, held upright (mm) — what a glyph's size on it is said by — and the glyph a Short
/// aims at for what its caption is about (a camera never takes more in past it: `camera::framed`'s `legible`).
const PHONE_MM: f32 = 70.0;
const LEGIBLE_MM: f32 = 2.5;
/// The least glyph a read stands at (the emphasis read, never only placed): one under it is said.
const READ_MM_MIN: f32 = 2.3;

/// A read's glyph through `window` where the whole screen is drawn at `whole` px: its px and its mm on a phone.
fn read_glyph(whole: f32, window: Region, screen: (u16, u16)) -> (f32, f32) {
    let px = whole * camera::nearer(window, screen) / camera::nearer(Region::all(screen), screen);
    (px, px / SHORT.0 as f32 * PHONE_MM)
}
/// A Short's opening on its punchline (the scene's `@punch`): this long of the take from there, before the title card.
const PUNCH_MS: u64 = 1_500;
/// A writing step this much past the last ends its burst; a burst of an edit that lands unseen (`@sound:landing`)
/// settles `LANDS_MS` after its last step.
const BURST_GAP_MS: u64 = 400;
const LANDS_MS: u64 = 150;
/// A step of the writing that changed more rows than this is the view moving (follow's jump, a scroll), not writing.
const WRITING_ROWS_MAX: usize = 2;

/// Which picture a video is: the wide one, or a Short.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Form {
    Wide,
    Short,
}

impl Form {
    fn dims(self) -> (usize, usize) {
        match self {
            Form::Wide => (W, H),
            Form::Short => SHORT,
        }
    }

    fn title_ms(self) -> u64 {
        if self == Form::Short { SHORT_TITLE_MS } else { TITLE_MS }
    }

    fn end_ms(self) -> u64 {
        if self == Form::Short { SHORT_END_MS } else { END_MS }
    }

    /// The take's last screen held before the end card: the wide video's `LAST_MS`; a Short's only its crossing's time
    /// (CLAUDE.md's Shorts bullet: no hold but the emphasis's), so a switch at the take's end is never cut mid-cross.
    fn last_ms(self) -> u64 {
        if self == Form::Short { camera::CROSS_MS } else { LAST_MS }
    }
}

/// Where a take's showing ends, its last screen at `last`: that screen held `Form::last_ms`; a Short's on to the take's
/// own end (`taken`: its run past its last step, `shot::TAIL_S`, or its cut — the screen as it stood till then), and
/// until the caption standing at its end (`spans`, to `u64::MAX`) has stood `captions::STAND_MIN_MS` — the end card
/// never cuts a phrase short, nothing trimmed for length (CLAUDE.md's Shorts bullet).
fn shown_to(last: u64, taken: u64, form: Form, spans: &[Option<captions::Span>]) -> u64 {
    let end = last + form.last_ms();
    match form {
        Form::Wide => end,
        Form::Short => (spans.iter().flatten())
            .filter(|s| s.to >= end)
            .map(|s| s.from + captions::STAND_MIN_MS)
            .fold(end.max(taken), u64::max),
    }
}

/// A Short's window, where the camera's picture goes (x, y, width, height): the frame's whole width, from the safe
/// area's top to its captions' band (`SHORT_SCREEN_H`) — the picture never under a caption, the whole or a zoom alike.
pub(crate) fn short_window() -> (usize, usize, usize, usize) {
    (0, SAFE.0, SHORT.0, SHORT_SCREEN_H)
}

/// The safe area's width: what YouTube's buttons at the right and the edge at the left leave — the captions' and the
/// intro's, never the screen's.
fn safe_w() -> usize {
    SHORT.0 - SAFE.1 - SAFE.3
}

/// A Short's captions' band in the video's px: under the window's last row to the safe area's bottom, across the safe
/// area (YouTube's buttons stand over the right edge there).
fn band_px() -> Px {
    let (_, y, _, h) = short_window();
    Px { x: SAFE.3 as f32, y: (y + h) as f32, w: safe_w() as f32, h: SHORT_BAND as f32 }
}

/// Where a Short's target sits: its centre a third down the safe area — the eye's height on a phone held upright —,
/// as a fraction of the window's height from its top.
pub(crate) fn short_eye() -> f32 {
    let (_, wy, _, wh) = short_window();
    let (top, _, bottom, _) = SAFE;
    let third = top + (SHORT.1 - top - bottom) / 3;
    third.saturating_sub(wy) as f32 / wh as f32
}

/// A Short's screen: `SHORT_COLS` columns and the rows the window holds at the glyph size those fit across it — the
/// whole screen in the window at `@zoom:all`. (Cells are 0.6 × 1.45 of their glyph, so 80 columns across a phone's
/// width fill its height at ~43 rows, never ~140.)
pub(crate) fn short_size() -> Result<(u16, u16)> {
    let fonts = Fonts::load(1)?;
    let (_, _, w, h) = short_window();
    let across = Region { x: 0.0, y: 0.0, w: f32::from(SHORT_COLS), h: 1.0 };
    let px = camera::fit(across, (w, h), SHORT_PX_MAX, |px| shot::cell(&fonts, px));
    let rows = h / shot::cell(&fonts, px).1;
    Ok((SHORT_COLS, rows as u16))
}

/// The theme's colours the video draws with outside tarae's cells: its ground, ink, faint ink (the band's rule) and
/// accent (`t a r a e` on the cards) — meok's and hanji's palettes (`src/themes/`).
struct Look {
    ground: Rgb,
    ink: Rgb,
    faint: Rgb,
    accent: Rgb,
}

fn look(bg: Bg) -> Look {
    let (ink, ground) = recording::ground(bg);
    match bg {
        Bg::Dark => Look { ground, ink, faint: [0x51, 0x54, 0x5c], accent: [0xf4, 0x80, 0x5a] },
        Bg::Light => Look { ground, ink, faint: [0x9c, 0x95, 0x86], accent: [0xb3, 0x3a, 0x22] },
    }
}

/// What `make` made: the video's length, its glyph size (the wide one's), its frames and the sheet's; a Short's camera
/// moves said (each: when, its target, its glyph and cell size) and its sound (`Sound`); what went wrong on the way that
/// left the video made all the same (a target never found).
pub(crate) struct Made {
    pub(crate) seconds: f64,
    pub(crate) px: f32,
    pub(crate) frames: u64,
    /// The frames that differ from the one before.
    pub(crate) differ: u64,
    pub(crate) sheet: usize,
    pub(crate) zooms: Vec<String>,
    pub(crate) sound: Option<Sound>,
    /// What a Short's track holds: keys typed, steps of Claude's writing shown, stamps, settles.
    pub(crate) heard: Option<(usize, usize, usize, usize)>,
    pub(crate) missed: Vec<String>,
}

/// `rec` as a video at `mp4` on `bg`'s theme in `form`, `title` on its title card, `sheet.png` of its frames beside it
/// (a Short's sound track too, `<scene>.wav`). Every text it draws passes `none_of_yours` with `yours` (the screens
/// did, as they were taken).
pub(crate) fn make(
    rec: &Recorded,
    bg: Bg,
    title: &str,
    mp4: &Path,
    yours: Option<&OsStr>,
    form: Form,
) -> Result<Made> {
    let Some(first) = rec.frames.first() else { return Err("the take kept no frame".into()) };
    for text in rec.beats.iter().map(|b| b.1.as_str()).chain([title]) {
        none_of_yours(&[text.chars().map(String::from).collect()], yours)?;
    }
    let (rows, cols) = first.1.size();
    let fonts = Fonts::load(1)?;
    let px = match form {
        Form::Wide => fit(&fonts, usize::from(cols), usize::from(rows))
            .ok_or_else(|| format!("{cols}×{rows} cells don't fit {W}×{H} under the captions' band"))?,
        Form::Short => 0.0,
    };
    let look = look(bg);
    let seal = seal(bg)?;
    let dims = form.dims();
    // (a Short's title wrapped inside its safe area)
    let titled: Vec<String> = match form {
        Form::Wide => vec![title.to_string()],
        Form::Short => wrap(title, safe_w() / shot::cell(&fonts, LINE_PX).0),
    };
    let name = ("t a r a e", NAME_PX, look.accent, true);
    let mut first_lines = vec![name];
    first_lines.extend(titled.iter().map(|t| (t.as_str(), LINE_PX, look.ink, false)));
    let cards = (
        card(&first_lines, &seal, &look, &fonts, dims),
        card(&[name, ("tarae.sh", LINE_PX, look.ink, false)], &seal, &look, &fonts, dims),
    );
    let screen = (cols, rows);
    let (notes, noted_missed) = match form {
        Form::Short => noted(rec),
        Form::Wide => (Vec::new(), Vec::new()),
    };
    for n in &notes {
        none_of_yours(&[n.written.chars().map(String::from).collect()], yours)?;
    }
    let whole = match form {
        Form::Short => {
            let (_, _, ww, wh) = short_window();
            camera::fit(Region::all(screen), (ww, wh), SHORT_PX_MAX, |p| shot::cell(&fonts, p))
        }
        Form::Wide => 0.0,
    };
    // (the window's columns a row, at a mid zoom's cells; the frame's own, at the whole's — `view`'s)
    let (_, _, ww, wh) = short_window();
    let shape = |(cw, ch): (usize, usize)| (ww as f32 / cw as f32) / (wh as f32 / ch as f32);
    let (aspect, drawn) = (shape(shot::cell(&fonts, 30.0)), shape(shot::cell(&fonts, whole.max(8.0))));
    let (aims, mut missed) = match form {
        Form::Short => {
            let legible = (LEGIBLE_MM / PHONE_MM * SHORT.0 as f32).ceil();
            let legible = (ww / shot::cell(&fonts, legible).0) as f32;
            reads(rec, (aspect, drawn), short_eye(), legible)
        }
        Form::Wide => (Vec::new(), Vec::new()),
    };
    missed.extend(noted_missed);
    // (a Short's intro in its title card's place: the mascot's swing and the name above the caption's place, its own
    // title said there as the hook — the mascot made smaller where the two don't fit above it)
    let intro = match (form, intro_frames(bg)) {
        (Form::Wide, _) => None,
        (Form::Short, Ok(frames)) => {
            let name = text("t a r a e", INTRO_NAME_PX, look.accent, look.ground, true, &fonts);
            let room = intro_room(caption_h(&fonts));
            let k = frames.first().map_or(1.0, |f| intro_fit(f.1, name.1, room));
            let frames = if k < 1.0 { frames.iter().map(|f| shrunk(f, k)).collect() } else { frames };
            Some(Intro { frames, name, hook: captions::phrase(title, HOOK_FROM, &[]), room })
        }
        (Form::Short, Err(e)) => {
            missed.push(format!("no intro ({e}): the title card in its place"));
            None
        }
    };
    let times: Vec<u64> = rec.frames.iter().map(|f| f.0).collect();
    let spans: Vec<Option<captions::Span>> = notes.iter().map(|n| n.span).collect();
    let last_ms = shown_to(times.last().copied().unwrap_or(0), rec.end, form, &spans);
    // (a Short's camera: each beat's read laid — `camera::course` —, where it aims on each screen, its path)
    let planned: Vec<camera::Planned> = aims.iter().map(|a| a.planned).collect();
    let course = camera::course(&planned);
    let s = |t: u64| t as f64 / 1000.0;
    for &i in &course.early {
        missed.push(format!(
            "the beat at {:.1} s: its read @zoom:{} gone from the screen at {:.1} s, before a read could stand — none",
            s(aims[i].planned.beat),
            aims[i].spec,
            s(aims[i].planned.gone.unwrap_or(0))
        ));
    }
    for &i in &course.over {
        missed.push(format!(
            "the beat at {:.1} s: its read @zoom:{} past the Short's {} — the camera on the whole",
            s(aims[i].planned.beat),
            aims[i].spec,
            camera::READS_MAX
        ));
    }
    for &(i, by) in &course.short {
        let (a, laid) = (&aims[i], course.reads[i].map_or("none", |_| "cut"));
        missed.push(format!(
            "the beat at {:.1} s: its read @zoom:{} needs {:.1} s more before the next beat — {laid}",
            s(a.planned.beat),
            a.spec,
            s(by)
        ));
    }
    // (where the camera aims: the whole, a read's window while it holds — back on the whole before the end card)
    let cells: Vec<camera::Cells> = match form {
        Form::Short => rec.frames.iter().map(|f| camera::Cells::of(&f.1)).collect(),
        Form::Wide => Vec::new(),
    };
    let windows = course.windows(&planned);
    let shown_ms = match form {
        Form::Wide => last_ms,
        Form::Short => last_ms.max(windows.iter().map(|w| w.1 + camera::DRIFT_MS).max().unwrap_or(0)),
    };
    let path = camera::Path::of(&camera::aims(&windows, screen), screen, shown_ms);
    missed.extend(stood_short(&notes, shown_ms));
    let tail = (form == Form::Short).then(|| captions::tail(&spans, shown_ms)).flatten();
    missed.extend(tail_short(&notes, tail));
    // (where most of the cells change at once, the two screens cross)
    let changed: Vec<f32> = (0..cells.len())
        .map(|i| if i == 0 { 0.0 } else { camera::changed(&cells[i - 1], &cells[i]) })
        .collect();
    let fades = camera::crossfades(&changed);
    // (a news read's toast laid over the window's top while the camera is there)
    let news: Vec<(u64, u64, String)> = (course.reads.iter().zip(&aims))
        .filter_map(|(r, a)| {
            Some((r.as_ref()?.there(), r.as_ref()?.out, camera::news(&a.spec)?.0.to_string()))
        })
        .collect();
    let zooms = match form {
        Form::Wide => Vec::new(),
        Form::Short => {
            let all = camera::nearer(Region::all(screen), screen);
            let mut said: Vec<String> = (rec.beats.iter().enumerate())
                .map(|(b, (t, caption))| {
                    let read = aims.iter().position(|a| a.beat == b);
                    let Some((i, Some(r))) = read.map(|i| (i, course.reads[i])) else {
                        return format!("{:.1} s “{caption}”: on the whole", s(*t));
                    };
                    let a = &aims[i];
                    let (px, mm) = read_glyph(whole, a.planned.window, screen);
                    format!(
                        "{:.1} s “{caption}”: eases in {:.1}–{:.1} s, held to {:.1} s, back on the whole at {:.1} s: \
                         {} {}, {px:.0} px glyphs ({:.0} columns across, the frame showing {:.1}; ~{mm:.1} mm a glyph \
                         on a phone){}",
                        s(*t),
                        s(r.into),
                        s(r.there()),
                        s(r.out),
                        s(r.out + camera::DRIFT_MS),
                        a.spec,
                        if a.deep { "deep" } else { "shallow" },
                        a.planned.window.w,
                        camera::shown(a.planned.window, drawn).w,
                        match a.left_kept {
                            true => "",
                            false => "; its pane's left (the gutter) not kept — the target held whole",
                        },
                    )
                })
                .collect();
            for (a, _) in aims.iter().zip(&course.reads).filter(|(_, r)| r.is_some()) {
                let (px, mm) = read_glyph(whole, a.planned.window, screen);
                if mm < READ_MM_MIN {
                    missed.push(format!(
                        "@zoom:{}: {px:.0} px glyphs, ~{mm:.1} mm on a phone — under the {READ_MM_MIN} mm a read needs",
                        a.spec
                    ));
                }
            }
            for n in &notes {
                let Some(span) = n.span else { continue };
                let to = if span.to >= shown_ms { " (to the end)" } else { "" };
                said.push(format!(
                    "caption “{}” → {}: from {:.1} s, {:.1} s{to}",
                    n.phrase.fullest(),
                    n.target,
                    s(span.from),
                    s(captions::stands(span, shown_ms))
                ));
            }
            for &i in &fades {
                said.push(format!(
                    "crossed at {:.1} s: {:.0} % of the cells changed",
                    s(times[i]),
                    changed[i] * 100.0
                ));
            }
            // (every 10 ms: the camera exactly on the whole, or zoomed — held at a read's window, or easing in or out —;
            // its largest)
            let (mut on_whole, mut zoomed, mut held) = (0u64, 0u64, 0u64);
            let (mut most, mut most_at) = (1.0f32, 0);
            for ms in (0..shown_ms).step_by(10) {
                let at = path.at(ms);
                if at == Region::all(screen) {
                    on_whole += 10;
                    continue;
                }
                zoomed += 10;
                if windows.iter().any(|w| (w.0 + camera::DRIFT_MS..w.1).contains(&ms)) {
                    held += 10;
                }
                let k = camera::nearer(at, screen) / all;
                if k > most {
                    (most, most_at) = (k, ms);
                }
            }
            let share = |t: u64| t as f64 * 100.0 / (on_whole + zoomed) as f64;
            said.push(format!(
                "the take {:.1} s: on the whole at 1.0× {:.1} s ({:.0} %), zoomed {:.1} s ({:.0} %: held {:.1} s, \
                 easing in and out {:.1} s); the largest {most:.2}× at {:.1} s",
                s(shown_ms),
                s(on_whole),
                share(on_whole),
                s(zoomed),
                share(zoomed),
                s(held),
                s(zoomed - held),
                s(most_at)
            ));
            if let Some((k, ms)) = tail {
                said.push(format!(
                    "its last beat “{}” lands at {:.1} s: a tail of {:.1} s to the end card",
                    notes[k].phrase.fullest(),
                    s(notes[k].span.map_or(0, |s| s.from)),
                    s(ms)
                ));
            }
            said.push(eased());
            said.push(waved(rec));
            said
        }
    };
    // (a Short opens on its punchline where the scene marks one: the read held there)
    let punch = rec.punch.filter(|_| form == Form::Short).map(|m| course.punch(m));
    let lead = punch.map_or(0, |_| PUNCH_MS) + intro.as_ref().map_or(form.title_ms(), |_| INTRO_MS);
    let video = Video {
        rec,
        times,
        px,
        look,
        fonts,
        cards,
        shown_ms,
        form,
        path,
        reads: (course.reads.iter().zip(&aims))
            .filter_map(|(r, a)| Some((*r)?.there()).map(|t| (t, a.spec.clone())))
            .collect(),
        fades,
        news,
        screen,
        whole,
        punch,
        lead,
        intro,
        notes,
    };
    missed.extend(video.checked());
    let frames = video.frames();
    if form == Form::Short && frames * 1000 / FPS > SHORT_MAX_MS {
        missed.push(format!(
            "the Short runs {:.1} s, past {} s — its beats as they are (none cut)",
            s(frames * 1000 / FPS),
            SHORT_MAX_MS / 1000
        ));
    }
    let dir = mp4.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let sheet = video.sheet(&dir.join("sheet.png"))?;
    let ffmpeg = ffmpeg().ok_or_else(|| {
        format!(
            "no ffmpeg on PATH nor at {}: the MP4 is made by it (Homebrew's `ffmpeg`) — the sheet of its frames is \
             written, {}",
            FFMPEG_AT.join(" or "),
            dir.join("sheet.png").display()
        )
    })?;
    let (audio, sound, counted) = match form {
        Form::Wide => (None, None, None),
        Form::Short => {
            let (heard, said) = heard(rec, video.lead);
            missed.extend(said);
            let count = |f: fn(&Heard) -> bool| heard.iter().filter(|h| f(&h.1)).count();
            let counted = (
                count(|h| matches!(h, Heard::Key(_))),
                count(|h| matches!(h, Heard::Shown(..))),
                count(|h| matches!(h, Heard::Stamp)),
                count(|h| matches!(h, Heard::Settle)),
            );
            let length = Duration::from_millis(frames * 1000 / FPS);
            // (the intro's brush: its swish and tap, from the intro's start)
            let brush =
                video.intro.as_ref().map(|_| Duration::from_millis(video.punch.map_or(0, |_| PUNCH_MS)));
            // (the AAC's true peak at `AAC_MARGIN_DB` under `AAC_PEAK_DB` as encoded: the limiter's ceiling moved by what
            // the encoder put on top of it or left under, measured, until it is — within `AAC_NEAR_DB`, never over, the
            // ceiling itself never past that aim, `AAC_TRIES` at most; a lower ceiling also lowers the keys' gain, the
            // limiter taking no more from them)
            let wav = mp4.with_extension("wav");
            let (mut ceilings, mut tracks) = (Vec::new(), Vec::new());
            let mut ceiling = sound::SHORT_CEILING_DB;
            let measured = |ceilings: &[(f32, std::result::Result<f32, String>)]| -> Vec<(f32, f32)> {
                ceilings.iter().filter_map(|(c, p)| Some((*c, *p.as_ref().ok()?))).collect()
            };
            loop {
                let track = sound::track(&heard, length, brush, ceiling);
                sound::wav(&wav, &track.samples)?;
                let peak = aac_peak(&ffmpeg, &wav);
                let failed = peak.is_err();
                ceilings.push((ceiling, peak));
                tracks.push(track);
                match next_ceiling(&measured(&ceilings)) {
                    Some(next) if !failed && ceilings.len() < AAC_TRIES => ceiling = next,
                    _ => break,
                }
            }
            // (the track whose AAC came out best — the last tried where none was measured)
            let pick = best_ceiling(&measured(&ceilings))
                .and_then(|c| ceilings.iter().position(|t| t.0 == c))
                .unwrap_or(tracks.len() - 1);
            if pick != tracks.len() - 1 {
                sound::wav(&wav, &tracks[pick].samples)?;
            }
            let track = tracks.swap_remove(pick);
            (Some(wav), Some((track, ceilings)), Some(counted))
        }
    };
    let differ = video.encode(&ffmpeg, mp4, audio.as_deref())?;
    let sound = sound.map(|(t, ceilings)| Sound {
        peak: t.peak,
        rms: t.rms,
        true_peak: t.true_peak,
        limited: t.limited,
        lufs: t.lufs,
        gain: t.gain,
        brush: t.brush,
        ceilings,
        encoded: loudness(&ffmpeg, mp4),
    });
    Ok(Made {
        seconds: frames as f64 / FPS as f64,
        px,
        frames,
        differ,
        sheet,
        zooms,
        sound,
        heard: counted,
        missed,
    })
}

/// A Short's AAC: its true peak at most this (dBTP, as encoded — a player decodes the AAC, not the WAV), with this much
/// room below it aimed at, and this near the aim enough; the limiter's ceiling moved this many times at most.
const AAC_PEAK_DB: f32 = -1.0;
const AAC_MARGIN_DB: f32 = 0.3;
const AAC_NEAR_DB: f32 = 0.2;
const AAC_TRIES: usize = 4;

/// The limiter's next ceiling (dBTP) from the ceilings tried and the AAC's true peak each came out at, in order: where
/// the last came out over the aim (`AAC_MARGIN_DB` under `AAC_PEAK_DB`) or more than `AAC_NEAR_DB` under it, moved by
/// the difference, never past the aim — and inside what's known (the encoder isn't even: a ceiling raised 0.6 dB came
/// out 1.2 dB higher): under the lowest that came out over, over the highest that came out under, else half way
/// between them. None where it's near enough, or the next is one tried.
fn next_ceiling(tried: &[(f32, f32)]) -> Option<f32> {
    let aim = AAC_PEAK_DB - AAC_MARGIN_DB;
    let &(ceiling, tp) = tried.last()?;
    if tp <= aim && aim - tp <= AAC_NEAR_DB {
        return None;
    }
    let over = tried.iter().filter(|t| t.1 > aim).map(|t| t.0).fold(f32::INFINITY, f32::min);
    let under = tried.iter().filter(|t| t.1 <= aim).map(|t| t.0).fold(f32::NEG_INFINITY, f32::max);
    let mut next = (ceiling + aim - tp).min(aim);
    if !(under + SAME_DB < next && next < over - SAME_DB) {
        next = match (under.is_finite(), over.is_finite()) {
            (true, true) => (under + over) / 2.0,
            (false, true) => over - (tp - aim).abs().max(AAC_NEAR_DB),
            _ => aim,
        };
    }
    tried.iter().all(|t| (t.0 - next).abs() > SAME_DB).then_some(next)
}

/// Two ceilings this near (dB) are one.
const SAME_DB: f32 = 0.05;

/// Of the ceilings tried (with the AAC's true peak each came out at), the one to keep: the highest whose AAC is at most
/// `AAC_PEAK_DB`, else the one whose AAC came out lowest.
fn best_ceiling(tried: &[(f32, f32)]) -> Option<f32> {
    let under = tried.iter().filter(|t| t.1 <= AAC_PEAK_DB).max_by(|a, b| a.0.total_cmp(&b.0));
    under.or_else(|| tried.iter().min_by(|a, b| a.1.total_cmp(&b.1))).map(|t| t.0)
}

/// `wav`'s true peak as AAC (dBTP): encoded as the MP4's sound is (160 kb/s) into a file beside it, read by `ebur128`
/// (`loudness`), the file removed.
fn aac_peak(ffmpeg: &Path, wav: &Path) -> std::result::Result<f32, String> {
    let m4a = wav.with_extension("peak.m4a");
    let out = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(wav)
        .args(["-c:a", "aac", "-b:a", "160k"])
        .arg(&m4a)
        .output()
        .map_err(|e| format!("{}: {e}", ffmpeg.display()))?;
    let peak = match out.status.success() {
        true => loudness(ffmpeg, &m4a).map(|l| l.1),
        false => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
    };
    let _ = std::fs::remove_file(&m4a);
    peak
}

/// A Short's sound as made: the track's peak and RMS (dBFS, while sounding), true peak (dBTP), loudness (LUFS) and the
/// gain it took (dB), what the limiter took, the intro brush's loudness over the rest's before the gain and after (LU),
/// each limiter ceiling tried (dBTP) with the AAC's true peak it came to, and the MP4's AAC as ffmpeg's `ebur128`
/// measures it — integrated LUFS and true peak (dBTP), or why not.
pub(crate) struct Sound {
    pub(crate) peak: f32,
    pub(crate) rms: f32,
    pub(crate) true_peak: f32,
    pub(crate) limited: sound::Limited,
    pub(crate) lufs: f32,
    pub(crate) gain: f32,
    pub(crate) ceilings: Vec<(f32, std::result::Result<f32, String>)>,
    pub(crate) brush: Option<(f32, f32)>,
    pub(crate) encoded: std::result::Result<(f32, f32), String>,
}

/// The MP4's sound as a player hears it: ffmpeg's `ebur128` over its AAC — its summary's integrated loudness and true
/// peak.
fn loudness(ffmpeg: &Path, mp4: &Path) -> std::result::Result<(f32, f32), String> {
    let out = Command::new(ffmpeg)
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(mp4)
        .args(["-map", "0:a", "-af", "ebur128=peak=true", "-f", "null", "-"])
        .output()
        .map_err(|e| format!("{}: {e}", ffmpeg.display()))?;
    let said = String::from_utf8_lossy(&out.stderr);
    let summary = said.rsplit_once("Summary:").map_or("", |s| s.1);
    let value = |key: &str| {
        (summary.lines().map(str::trim))
            .find_map(|l| l.strip_prefix(key))
            .and_then(|v| v.split_whitespace().next()?.parse::<f32>().ok())
    };
    value("I:").zip(value("Peak:")).ok_or_else(|| "ebur128 said no summary".to_string())
}

/// A beat's one read, as the take has it: the beat (its index), its target as written and whether it goes deep, what
/// `camera::course` lays it by, and whether its window keeps its target's pane's left (`camera::keeps_panes_left`:
/// a deep one's and a news beat's always — neither is laid from there).
struct Aim {
    beat: usize,
    spec: String,
    deep: bool,
    planned: camera::Planned,
    left_kept: bool,
}

/// A Short's reads from the take, one a beat: its first `@zoom` (`@zoom-deep`) before the next beat, found on the
/// first screen at or after its time that draws its target (within `FIND_MS`), shaped to the window there
/// (`camera::framed`: `aspect` columns a row, its centre `eye` down; deep, `legible` columns at most taken in; shallow,
/// `legible` columns at least, from the target's pane's left) — as wide as it grows on the same rows before the next beat
/// (`camera::widened`: a line Claude is writing reaches its end, never past the window's edge); a beat with none stays
/// on the whole. A second `@zoom` in a beat, one before the first beat and a target never found are said, left out.
fn reads(rec: &Recorded, (aspect, drawn): (f32, f32), eye: f32, legible: f32) -> (Vec<Aim>, Vec<String>) {
    let (mut out, mut missed) = (Vec::new(), Vec::new());
    let s = |t: u64| t as f64 / 1000.0;
    let first = rec.beats.first().map_or(u64::MAX, |b| b.0);
    for (t, spec, _) in rec.zooms.iter().filter(|z| z.0 < first) {
        missed.push(format!("@zoom:{spec} at {:.1} s: before the first beat — left out", s(*t)));
    }
    for (b, &(beat, _)) in rec.beats.iter().enumerate() {
        let next = rec.beats.get(b + 1).map(|n| n.0);
        let until = next.unwrap_or(u64::MAX);
        let mut marks = rec.zooms.iter().filter(|z| (beat..until).contains(&z.0));
        let Some((t, spec, deep)) = marks.next() else { continue };
        for (t, extra, _) in marks {
            missed
                .push(format!("@zoom:{extra} at {:.1} s: a beat reads one thing ({spec}) — left out", s(*t)));
        }
        let (t, deep) = (*t, *deep);
        let from = rec.frames.partition_point(|f| f.0 <= t).saturating_sub(1);
        let found = rec.frames[from..].iter().take_while(|f| f.0 <= t + FIND_MS).find_map(|(at, s)| {
            let cells = camera::Cells::of(s);
            let region = camera::target(&cells, spec)?;
            // (the target on each screen until the next beat, as it grows)
            let held = (rec.frames.iter())
                .skip_while(|f| f.0 <= *at)
                .take_while(|f| f.0 < until)
                .filter_map(|f| camera::target(&camera::Cells::of(&f.1), spec));
            let region = camera::widened(region, held, (cells.text[0].len() as u16, cells.text.len() as u16));
            let (window, left_kept) = match camera::news(spec) {
                Some((text, _)) => {
                    (camera::news_framed(&cells, text, region, (aspect, eye, legible, deep)), true)
                }
                None => {
                    // (the cards drawn while it stands — the ease in and the hold — wholly in its window or out of it)
                    let stands = *at + camera::DRIFT_MS + camera::READ_MS;
                    let mut later: Vec<camera::Card> = Vec::new();
                    for f in
                        rec.frames.iter().skip_while(|f| f.0 <= *at).take_while(|f| f.0 < stands.min(until))
                    {
                        for c in camera::cards(&camera::Cells::of(&f.1)) {
                            if !later.contains(&c) {
                                later.push(c);
                            }
                        }
                    }
                    let window =
                        camera::framed_among(&cells, region, (aspect, eye, legible, deep), (&later, drawn));
                    let seen = camera::shown(window, drawn);
                    (window, deep || camera::keeps_panes_left(&cells, region, seen))
                }
            };
            Some((*at.max(&t), window, left_kept))
        });
        match found {
            Some((found, window, left_kept)) => out.push(Aim {
                beat: b,
                spec: spec.clone(),
                deep,
                planned: camera::Planned {
                    beat,
                    next,
                    found,
                    gone: card_gone(rec, spec, found, until),
                    window,
                },
                left_kept,
            }),
            None => missed.push(format!(
                "@zoom:{spec} at {:.1} s: not on screen within {} s — its beat goes with its captions",
                s(t),
                FIND_MS / 1000
            )),
        }
    }
    (out, missed)
}

/// When a read's target leaves the screen while it's read (from `at` until `until`): the first screen after the last one
/// that draws it (`camera::target`) — none while one to the read's end does; `at` where none does.
fn card_gone(rec: &Recorded, spec: &str, at: u64, until: u64) -> Option<u64> {
    let from = rec.frames.partition_point(|f| f.0 <= at).saturating_sub(1);
    let end = rec.frames.partition_point(|f| f.0 < until).max(from);
    let hold = &rec.frames[from..end];
    match hold.iter().rposition(|f| camera::target(&camera::Cells::of(&f.1), spec).is_some()) {
        Some(last) => hold.get(last + 1).map(|f| f.0),
        None => Some(at),
    }
}

/// A Short's caption as the take has it: its words as written (`{}` the target's text), its target, when it stands
/// (`captions::spans`; none where its target never came, or the next came before it stood) and its phrase as the take
/// has it (`captions::phrase`: a `{}` the target's text as it changed while the caption stood).
struct Noted {
    written: String,
    target: String,
    span: Option<captions::Span>,
    phrase: captions::Phrase,
}

/// A Short's captions from the take, each `@note:<words> → <target>` in mark order: in from the first screen at or after
/// its mark that draws its target (within `FIND_MS`), out at the first after that which doesn't — or as the next comes
/// (`captions::spans`: the next crossing it). A mark that isn't `<words> → <target>`, words past `captions::WORDS_MAX`,
/// a target never found and one the next came on before it could stand are said.
fn noted(rec: &Recorded) -> (Vec<Noted>, Vec<String>) {
    let s = |t: u64| t as f64 / 1000.0;
    let (mut notes, mut found, mut missed) = (Vec::new(), Vec::new(), Vec::new());
    for (t, mark) in &rec.notes {
        let Some((written, target)) = captions::parse(mark) else {
            missed.push(format!("@note:{mark} at {:.1} s: not `<words> → <target>` — left out", s(*t)));
            continue;
        };
        if captions::counted(written) > captions::WORDS_MAX {
            missed.push(format!("@note:{mark}: past {} words — a caption says a few", captions::WORDS_MAX));
        }
        let drawn = |f: &(u64, vt100::Screen)| camera::target(&camera::Cells::of(&f.1), target).is_some();
        let from = rec.frames.partition_point(|f| f.0 <= *t).saturating_sub(1);
        let first =
            (rec.frames[from..].iter().take_while(|f| f.0 <= t + FIND_MS)).position(drawn).map(|k| from + k);
        let Some(first) = first else {
            missed.push(format!(
                "@note:{mark} at {:.1} s: not on screen within {} s — left out",
                s(*t),
                FIND_MS / 1000
            ));
            continue;
        };
        let gone = rec.frames[first..].iter().find(|f| !drawn(f)).map(|f| f.0);
        found.push((rec.frames[first].0.max(*t), gone));
        notes.push(Noted {
            written: written.to_string(),
            target: target.to_string(),
            span: None,
            phrase: captions::Phrase { said: Vec::new() },
        });
    }
    for (n, span) in notes.iter_mut().zip(captions::spans(&found)) {
        let Some(span) = span else {
            missed.push(format!(
                "@note:{} → {}: the next caption came before it could stand — none",
                n.written, n.target
            ));
            continue;
        };
        // (what its target says, each change while it stands: the question as it's typed)
        let mut said: Vec<(u64, String)> = Vec::new();
        if n.written.split_whitespace().any(|w| w == "{}") {
            let from = rec.frames.partition_point(|f| f.0 < span.from).saturating_sub(1);
            for f in rec.frames[from..].iter().take_while(|f| f.0 < span.to) {
                let now = camera::said_in(&camera::Cells::of(&f.1), &n.target);
                if let Some(now) = now.filter(|now| said.last().is_none_or(|l| &l.1 != now)) {
                    said.push((f.0.max(span.from), now));
                }
            }
        }
        n.phrase = captions::phrase(&n.written, span.from, &said);
        n.span = Some(span);
    }
    (notes, missed)
}

/// The captions of a take of `end` ms standing less than `captions::STAND_MIN_MS`, each named with when and how long:
/// a phrase is written for the time the scene gives it, none held for it.
fn stood_short(notes: &[Noted], end: u64) -> Vec<String> {
    let s = |t: u64| t as f64 / 1000.0;
    let spans: Vec<Option<captions::Span>> = notes.iter().map(|n| n.span).collect();
    (captions::short(&spans, end).into_iter())
        .map(|(k, ms)| {
            format!(
                "caption “{}” stands {:.1} s from {:.1} s, under {:.1} s — its phrase for the time the scene gives it",
                notes[k].phrase.fullest(),
                s(ms),
                s(spans[k].map_or(0, |s| s.from)),
                s(captions::STAND_MIN_MS)
            )
        })
        .collect()
}

/// A Short's last caption (`tail`: which, how long it stands) said where it stands less than `captions::TAIL_MIN_MS`:
/// its last beat gone before it's read — the take is to run on past it, not hold.
fn tail_short(notes: &[Noted], tail: Option<(usize, u64)>) -> Option<String> {
    let (k, ms) = tail.filter(|t| t.1 < captions::TAIL_MIN_MS)?;
    let s = |t: u64| t as f64 / 1000.0;
    Some(format!(
        "its last caption “{}” stands {:.1} s from {:.1} s before the end card, under the {:.1} s tail — the take runs on \
         past its last beat (`shot::TAIL_S`), its target on the screen till the end",
        notes[k].phrase.fullest(),
        s(ms),
        s(notes[k].span.map_or(0, |s| s.from)),
        s(captions::TAIL_MIN_MS)
    ))
}

/// What a Short hears, at its time in the video (the take's after `lead` ms: the punchline and the title card, both
/// silent): each key sent (a `@type:` character one by one; a key sent whole — a paste, an escape — one stroke), each
/// `@sound:stamp` (the key after it within `STAMP_KEY_MS` its own) and `@sound:settle`, and Claude's writing inside
/// the scene's `@sound:writing` and `@sound:landing` spans (`writing`); another `@sound:` said.
fn heard(rec: &Recorded, lead: u64) -> (Vec<(Duration, Heard)>, Vec<String>) {
    let at = |t: u64| Duration::from_millis(lead + t);
    let (mut out, mut said) = (Vec::new(), Vec::new());
    let stamps: Vec<u64> = rec.sounds.iter().filter(|s| s.1 == "stamp").map(|s| s.0).collect();
    for (t, k) in &rec.keys {
        if stamps.iter().any(|s| (*s..=s + STAMP_KEY_MS).contains(t)) {
            continue;
        }
        if let Some(c) = k.chars().next() {
            out.push((at(*t), Heard::Key(c)));
        }
    }
    for (t, kind) in &rec.sounds {
        match kind.as_str() {
            "stamp" => out.push((at(*t), Heard::Stamp)),
            "settle" => out.push((at(*t), Heard::Settle)),
            "writing" | "landing" | "still" => {}
            other => {
                said.push(format!("@sound:{other}: no such sound (stamp, settle, writing, landing, still)"))
            }
        }
    }
    out.extend(writing(rec).into_iter().map(|(t, h)| (at(t), h)));
    out.sort_by_key(|h| h.0);
    (out, said)
}

/// Claude's writing in a take, as tarae's reveal hears it: inside each span the scene opens with `@sound:writing` (or
/// `@sound:landing`: an edit that lands unseen) and closes with `@sound:still` (else the next span or the take's end),
/// each screen's newly shown code (`shown`) one step of the typist, as long as that screen stood (at most 100 ms, the
/// typist's `LATE_MAX`) — never a screen that changed more than `WRITING_ROWS_MAX` rows (the view moving, not
/// writing) —, and in a landing's span each burst (steps under `BURST_GAP_MS` apart) settling `LANDS_MS` after its
/// last step.
fn writing(rec: &Recorded) -> Vec<(u64, Heard)> {
    // (each span: from, to, and whether its edits land unseen)
    let mut spans: Vec<(u64, u64, bool)> = Vec::new();
    let mut open: Option<(u64, bool)> = None;
    for (t, kind) in &rec.sounds {
        let lands = match kind.as_str() {
            "writing" => Some(false),
            "landing" => Some(true),
            "still" => None,
            _ => continue,
        };
        if let Some((from, l)) = open.take() {
            spans.push((from, *t, l));
        }
        open = lands.map(|l| (*t, l));
    }
    spans.extend(open.map(|(from, l)| (from, u64::MAX, l)));
    let mut out = Vec::new();
    for (from, to, lands) in spans {
        let first = rec.frames.partition_point(|f| f.0 < from);
        let rows = |i: usize| camera::code_rows(&camera::Cells::of(&rec.frames[i].1));
        let mut before = first.checked_sub(1).map(rows);
        let mut last: Option<u64> = None;
        for i in first..rec.frames.len() {
            let t = rec.frames[i].0;
            if t > to {
                break;
            }
            let now = rows(i);
            if let Some((text, changed)) = before.as_ref().map(|b| shown(b, &now))
                && !text.is_empty()
                && changed <= WRITING_ROWS_MAX
            {
                if lands && let Some(l) = last.filter(|l| t - l > BURST_GAP_MS) {
                    out.push((l + LANDS_MS, Heard::Settle));
                }
                let next = rec.frames.get(i + 1).map_or(t + 25, |f| f.0);
                out.push((t, Heard::Shown(text, Duration::from_millis((next - t).clamp(1, 100)))));
                last = Some(t);
            }
            before = Some(now);
        }
        if lands && let Some(l) = last {
            out.push((l + LANDS_MS, Heard::Settle));
        }
    }
    out
}

/// The working wave's steps in the take, said: how far apart its cells changed (min, median, max ms) — tarae steps it
/// every 0.2 s (`shimmer::STEP`, the user's pick) whatever rate its fades are drawn at. The wave: the first three cells
/// in a row of `▂`…`▇` on a screen, a step a change of them where they stand (a wave somewhere else starts anew).
fn waved(rec: &Recorded) -> String {
    type Wave = Option<(usize, usize, String)>;
    let lit = |c: &String| c.chars().count() == 1 && ('▂'..='▇').contains(&c.chars().next().unwrap_or(' '));
    let wave = |s: &vt100::Screen| -> Wave {
        let cells = recording::screen_cells(s);
        cells.iter().enumerate().find_map(|(r, row)| {
            let c = row.windows(3).position(|w| w.iter().all(lit))?;
            Some((r, c, row[c..c + 3].concat()))
        })
    };
    let mut steps: Vec<(u64, Wave)> = Vec::new();
    for (t, s) in &rec.frames {
        let now = wave(s);
        if steps.last().is_none_or(|l| l.1 != now) {
            steps.push((*t, now));
        }
    }
    let same_place = |a: &Wave, b: &Wave| matches!((a, b), (Some(a), Some(b)) if (a.0, a.1) == (b.0, b.1));
    let pairs: Vec<&[(u64, Wave)]> = steps.windows(2).filter(|p| same_place(&p[0].1, &p[1].1)).collect();
    let mut apart: Vec<u64> = pairs.iter().map(|p| p[1].0 - p[0].0).collect();
    apart.sort_unstable();
    // (the steps off the 0.2 s: when, from what to what)
    let off: Vec<String> = (pairs.iter())
        .filter(|p| !(160..=240).contains(&(p[1].0 - p[0].0)))
        .map(|p| {
            let cells = |w: &Wave| w.as_ref().map_or(String::new(), |w| w.2.clone());
            format!(
                "{:.2} s +{} ms {}→{}",
                p[1].0 as f64 / 1000.0,
                p[1].0 - p[0].0,
                cells(&p[0].1),
                cells(&p[1].1)
            )
        })
        .collect();
    match (apart.first(), apart.last()) {
        (Some(lo), Some(hi)) => format!(
            "the working wave: {} steps at one place, {lo}/{}/{hi} ms apart (min/median/max), {} of them 0.2 s ± 40 ms; \
             the others: {}",
            apart.len(),
            apart[apart.len() / 2],
            apart.iter().filter(|d| (160..=240).contains(*d)).count(),
            off.join(", ")
        ),
        _ => "the working wave: never on screen".into(),
    }
}

/// What screen `b`'s code rows newly show against `a`'s (each `camera::code_rows`), and how many rows changed: a row
/// whose text no row of `a` has gives what it adds past the text it shares with `a`'s row there — or, sharing no more
/// than its indent, the whole of it as a new line (`\n` before it). A row moved down by a line written above it is a row
/// `a` has: unchanged.
fn shown(a: &[Option<String>], b: &[Option<String>]) -> (String, usize) {
    let had: std::collections::HashSet<&str> = a.iter().flatten().map(String::as_str).collect();
    let (mut out, mut changed) = (String::new(), 0);
    for (i, row) in b.iter().enumerate() {
        let Some(row) = row.as_deref() else { continue };
        if row.trim().is_empty() || had.contains(row) {
            continue;
        }
        changed += 1;
        let old = a.get(i).and_then(|o| o.as_deref()).unwrap_or("");
        let common = row.chars().zip(old.chars()).take_while(|(x, y)| x == y).count();
        let indent = row.chars().take_while(|c| c.is_whitespace()).count();
        if common <= indent {
            out.push('\n');
            out.push_str(row.trim_start());
        } else {
            out.extend(row.chars().skip(common));
        }
    }
    (out, changed)
}

/// A Short's move as numbers (`camera::way`'s, each way): its time, when it has come 90 % of the way, and how far past
/// its mark it goes.
fn eased() -> String {
    let way = |toward: bool| {
        let (ninety, past) = camera::way_numbers(toward);
        let past = match past > 0.0005 {
            true => format!(", {:.1} % past it", past * 100.0),
            false => ", never past it".to_string(),
        };
        format!("90 % of the way at {:.2} s{past}", ninety as f64 / 1000.0)
    };
    format!(
        "the move: {:.1} s each way — in {}; out {}",
        camera::DRIFT_MS as f64 / 1000.0,
        way(true),
        way(false)
    )
}

/// Which of the screens drawn at `times` (ms from the take's start, in order) the `k`th frame of a `FPS` video shows:
/// the last one drawn by its time, `k / FPS` s — the first one before any.
pub(crate) fn screen_at(times: &[u64], k: u64) -> usize {
    times.partition_point(|&t| t * FPS <= k * 1000).saturating_sub(1)
}

/// The largest glyph size (whole pixels) whose `cols`×`rows` cells, with the renderer's margin, fit the picture above
/// the captions' band with `SIDE` and `ABOVE` round them.
fn fit(fonts: &Fonts, cols: usize, rows: usize) -> Option<f32> {
    (8..=64).rev().map(|px| px as f32).find(|&px| {
        let (cw, ch) = shot::cell(fonts, px);
        cols * cw + 2 * PAD <= W - 2 * SIDE && rows * ch + 2 * PAD <= H - BAND - 2 * ABOVE
    })
}

/// `text` in as few lines of at most `cols` characters as it takes, broken between words and balanced (each line as
/// near the same length as the words let it — never one word left alone under a full line).
fn wrap(text: &str, cols: usize) -> Vec<String> {
    let lines = greedy(text, cols);
    let total = text.chars().count();
    (total.div_ceil(lines.len().max(1))..cols)
        .map(|w| greedy(text, w))
        .find(|l| l.len() == lines.len())
        .unwrap_or(lines)
}

/// `text` in lines of at most `cols` characters, each filled before the next (a word longer than a line on its own).
fn greedy(text: &str, cols: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(l) if l.chars().count() + 1 + word.chars().count() <= cols => {
                l.push(' ');
                l.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// RGBA pixels: width, height, four bytes a pixel.
type Rgba = (usize, usize, Vec<u8>);

/// The seal for `bg`'s theme, RGBA (`docs/brand/seal-<theme>-96.png`, made once from its SVG).
fn seal(bg: Bg) -> Result<Rgba> {
    rgba(&root().join(format!("docs/brand/seal-{}-96.png", bg.theme())))
}

/// Where a Short's intro frames are (made once, `.sb/shorts-intro/make.sh`): `TARAE_SHORTS_INTRO`, else main's
/// `.sb/youtube/shorts-intro` — the main checkout's, by the repository's common git folder: outside git, where `cargo
/// clean` can't take it, the same from every checkout.
fn intro_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("TARAE_SHORTS_INTRO") {
        return Some(PathBuf::from(dir));
    }
    Some(crate::linux::git_folder(&root())?.parent()?.join(".sb/youtube/shorts-intro"))
}

/// The intro's frames for `bg`'s theme, in order (`<theme>/NNN.png`).
fn intro_frames(bg: Bg) -> Result<Vec<Rgba>> {
    let dir = intro_dir().ok_or("no main checkout found")?.join(bg.theme());
    let listed = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> =
        listed.flatten().map(|e| e.path()).filter(|p| p.extension() == Some(OsStr::new("png"))).collect();
    paths.sort();
    if paths.is_empty() {
        return Err(format!("{}: no frames", dir.display()));
    }
    paths.iter().map(|p| rgba(p)).collect()
}

/// An 8-bit RGBA PNG.
fn rgba(path: &Path) -> Result<Rgba> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().map_err(|e| err(&e))?;
    let mut px = vec![0; reader.output_buffer_size().ok_or_else(|| err(&"too big"))?];
    let info = reader.next_frame(&mut px).map_err(|e| err(&e))?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(err(&"not 8-bit RGBA"));
    }
    px.truncate(info.buffer_size());
    Ok((info.width as usize, info.height as usize, px))
}

/// `text` in one row of cells at `px`, `fg` on `ground` (bold: `bold`), by the shots' renderer — its margin the ground.
fn text(text: &str, px: f32, fg: Rgb, ground: Rgb, bold: bool, fonts: &Fonts) -> Picture {
    let cols = text.chars().count().max(1) as u16;
    let mut p = vt100::Parser::new(1, cols, 0);
    let [r, g, b] = fg;
    let [gr, gg, gb] = ground;
    let weight = if bold { "1;" } else { "" };
    p.process(format!("\x1b[?25l\x1b[{weight}38;2;{r};{g};{b};48;2;{gr};{gg};{gb}m{text}").as_bytes());
    shot::render_px(p.screen(), fonts, px)
}

/// A caption's lines, a row of cells each at `CAPTION_SHORT_PX`, bold, in its ink on its ground, by the shots' renderer.
fn caption_text(lines: &[String], fonts: &Fonts) -> Picture {
    let cols = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1).max(1) as u16;
    let mut p = vt100::Parser::new(lines.len().max(1) as u16, cols, 0);
    let ([r, g, b], [gr, gg, gb]) = (CAPTION_INK, CAPTION_GROUND);
    let mut bytes = format!("\x1b[?25l\x1b[1;38;2;{r};{g};{b};48;2;{gr};{gg};{gb}m");
    for (k, line) in lines.iter().enumerate() {
        bytes.push_str(&format!("\x1b[{};1H{line}", k + 1));
    }
    p.process(bytes.as_bytes());
    shot::render_px(p.screen(), fonts, CAPTION_SHORT_PX)
}

/// A card's frame, `dims` px: `lines` (text, size, colour, bold) one under another, centred, the seal stamped under
/// them.
fn card(
    lines: &[(&str, f32, Rgb, bool)],
    seal: &(usize, usize, Vec<u8>),
    look: &Look,
    fonts: &Fonts,
    dims: (usize, usize),
) -> Vec<u8> {
    let (w, h) = dims;
    let mut px = look.ground.repeat(w * h);
    let pictures: Vec<Picture> = (lines.iter().filter(|l| !l.0.is_empty()))
        .map(|&(t, size, fg, bold)| text(t, size, fg, look.ground, bold, fonts))
        .collect();
    let gap = 48;
    let tall = pictures.iter().map(|p| p.1).sum::<usize>() + gap + seal.1;
    let mut y = h.saturating_sub(tall) / 2;
    for p in &pictures {
        blit(&mut px, dims, p, (w - p.0.min(w)) / 2, y, 255);
        y += p.1;
    }
    let (sw, sh, rgba) = seal;
    let (x0, y0) = ((w - sw) / 2, y + gap);
    for sy in 0..*sh {
        for sx in 0..*sw {
            let s = &rgba[(sy * sw + sx) * 4..][..4];
            let at = ((y0 + sy) * w + x0 + sx) * 3;
            mix(&mut px[at..at + 3], [s[0], s[1], s[2]], s[3]);
        }
    }
    px
}

/// `dst` (one pixel, RGB) moved `a`/255 of the way to `c`.
fn mix(dst: &mut [u8], c: Rgb, a: u8) {
    for (d, s) in dst.iter_mut().zip(c) {
        *d = ((*d as u32 * (255 - a as u32) + s as u32 * a as u32 + 127) / 255) as u8;
    }
}

/// `p` on the frame `dst` (`dims` px) at (`x0`, `y0`), `a`/255 of it over what's there; clipped.
fn blit(dst: &mut [u8], dims: (usize, usize), p: &Picture, x0: usize, y0: usize, a: u8) {
    let (fw, fh) = dims;
    let (pw, ph, src) = p;
    let w = (*pw).min(fw.saturating_sub(x0));
    for y in 0..(*ph).min(fh.saturating_sub(y0)) {
        let (from, to) = ((y * pw) * 3, ((y0 + y) * fw + x0) * 3);
        if a == 255 {
            dst[to..to + w * 3].copy_from_slice(&src[from..from + w * 3]);
            continue;
        }
        for x in 0..w {
            let s = &src[from + x * 3..][..3];
            mix(&mut dst[to + x * 3..][..3], [s[0], s[1], s[2]], a);
        }
    }
}

/// `p` (`dims` px) at half its size, each pixel the mean of four (a sheet's frame).
fn half(p: &[u8], dims: (usize, usize)) -> Picture {
    let (fw, _) = dims;
    let (w, h) = (dims.0 / 2, dims.1 / 2);
    let mut out = vec![0; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let at = |dx: usize, dy: usize| p[((2 * y + dy) * fw + 2 * x + dx) * 3 + c] as u32;
                out[(y * w + x) * 3 + c] = ((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8;
            }
        }
    }
    (w, h, out)
}

/// ffmpeg on `PATH`, else at `FFMPEG_AT`.
fn ffmpeg() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    (std::env::split_paths(&path).map(|d| d.join("ffmpeg")))
        .chain(FFMPEG_AT.iter().map(PathBuf::from))
        .find(|p| p.is_file())
}

/// Which part of the video its `t` ms is in: a Short's punchline (the take's ms from `punch`, `PUNCH_MS` long), the
/// title card or a Short's intro in its place (to `lead`), the take (its ms, `shown` long — a Short's first beat opens on the whole), then the end card.
#[derive(Debug, PartialEq)]
enum Part {
    Punch(u64),
    Title,
    Take(u64),
    End,
}

fn part(t: u64, punch: Option<u64>, lead: u64, shown: u64) -> Part {
    match punch {
        Some(from) if t < PUNCH_MS => Part::Punch(from + t),
        _ if t < lead => Part::Title,
        _ if t < lead + shown => Part::Take(t - lead),
        _ => Part::End,
    }
}

/// What one frame of the video shows.
#[derive(Clone, Copy, PartialEq)]
enum Shows {
    Title,
    /// A Short's intro, its ms.
    Intro(u64),
    Screen(Take),
    End,
}

/// The take as one frame shows it: the screen in force, and the one it crosses from with how far the screen in force is
/// drawn over it (of 255); the beat whose caption a wide video says and how far that has come in (of 255); the camera's
/// region; the news read whose toast is laid over the window; and a Short's captions standing — one, or two crossing,
/// the one going first: each which, how drawn (of 255) and how many characters of its phrase.
#[derive(Clone, Copy, PartialEq)]
struct Take {
    i: usize,
    cross: Option<(usize, u8)>,
    beat: Option<usize>,
    came: u8,
    region: Region,
    news: Option<usize>,
    captions: [Option<(usize, u8, usize)>; 2],
}

/// How the camera's region maps the screen onto a Short's window: the screen drawn at `px` (its cells `cell` px), and
/// for each of the window's px the picture's it samples, by axis `origin + k · scale` (px indices: a pixel's centre).
#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    px: f32,
    cell: (usize, usize),
    scale: (f32, f32),
    origin: (f32, f32),
}

impl View {
    /// A region of the screen's cells in the video's px.
    #[cfg(test)]
    fn px(&self, r: Region) -> Px {
        let (wx, wy, _, _) = short_window();
        let at = |o: f32, s: f32, cell: usize, c: f32| (PAD as f32 + c * cell as f32 - o - 0.5) / s + 0.5;
        let (x0, x1) = (
            at(self.origin.0, self.scale.0, self.cell.0, r.x),
            at(self.origin.0, self.scale.0, self.cell.0, r.x + r.w),
        );
        let (y0, y1) = (
            at(self.origin.1, self.scale.1, self.cell.1, r.y),
            at(self.origin.1, self.scale.1, self.cell.1, r.y + r.h),
        );
        Px { x: wx as f32 + x0, y: wy as f32 + y0, w: x1 - x0, h: y1 - y0 }
    }

    /// The whole cells a `window` (px) shows of a screen of `cols`×`rows`: columns `c0..c1`, rows `r0..r1`.
    fn cells(&self, (cols, rows): (u16, u16), window: (usize, usize)) -> (usize, usize, usize, usize) {
        let axis = |o: f32, s: f32, view: usize, cell: usize, n: u16| {
            let n = usize::from(n);
            let (from, to) = (o + 0.5 - s / 2.0 - PAD as f32, o + 0.5 + (view as f32 - 0.5) * s - PAD as f32);
            let first = ((from / cell as f32).ceil().max(0.0) as usize).min(n);
            (first, ((to / cell as f32).floor().max(0.0) as usize).clamp(first, n))
        };
        let (c0, c1) = axis(self.origin.0, self.scale.0, window.0, self.cell.0, cols);
        let (r0, r1) = axis(self.origin.1, self.scale.1, window.1, self.cell.1, rows);
        (c0, r0, c1, r1)
    }
}

/// How the camera's `region` of a `screen` maps onto a `window` (px): its cells as large on the window as the region
/// fills it, against the whole's (`whole`: the glyph size the whole screen fits at — there the picture's own px, the
/// whole drawn sharp); drawn at the smallest glyph size whose cells (`cell`'s) are at least that large (at most
/// `SHORT_PX_MAX`) and taken down to it — so a push is continuous, never a glyph size's step; the region's middle at
/// the window's.
fn view(
    region: Region,
    screen: (u16, u16),
    window: (usize, usize),
    whole: f32,
    cell: impl Fn(f32) -> (usize, usize),
) -> View {
    let (ww, wh) = (window.0 as f32, window.1 as f32);
    let (cw0, ch0) = cell(whole);
    let fill = |r: Region| (ww / (r.w * cw0 as f32)).min(wh / (r.h * ch0 as f32));
    let m = fill(region) / fill(Region::all(screen));
    let m = if (m - 1.0).abs() < 1e-3 { 1.0 } else { m };
    let (tw, th) = (cw0 as f32 * m, ch0 as f32 * m);
    let px = match m == 1.0 {
        true => whole,
        false => (8..=SHORT_PX_MAX)
            .map(|p| p as f32)
            .find(|&p| cell(p).0 as f32 >= tw && cell(p).1 as f32 >= th)
            .unwrap_or(SHORT_PX_MAX as f32),
    };
    let (cw, ch) = cell(px);
    let scale = (cw as f32 / tw, ch as f32 / th);
    let origin = |start: f32, len: f32, cell: usize, view: f32, s: f32| {
        PAD as f32 + (start + len / 2.0) * cell as f32 - (view / 2.0 - 0.5) * s - 0.5
    };
    View {
        px,
        cell: (cw, ch),
        scale,
        origin: (origin(region.x, region.w, cw, ww, scale.0), origin(region.y, region.h, ch, wh, scale.1)),
    }
}

/// `pic` drawn through `view` into the `size` px at `at` of a frame `w` px wide (RGB): each px the picture's bilinear
/// sample there, where that's inside the picture (else the frame's own, as it is); at a scale of 1 and a whole origin,
/// the picture's own px.
fn resample(out: &mut [u8], w: usize, at: (usize, usize), size: (usize, usize), pic: &Picture, view: &View) {
    let (pw, ph, src) = pic;
    let axis = |o: f32, s: f32, n: usize, len: usize| -> Vec<Option<(usize, usize, u32)>> {
        (0..n)
            .map(|k| {
                let p = o + k as f32 * s;
                if p < -0.5 || p > len as f32 - 0.5 {
                    return None;
                }
                let f = p.floor();
                let a = (f.max(0.0) as usize).min(len - 1);
                let b = ((f + 1.0).max(0.0) as usize).min(len - 1);
                Some((a, b, if p < 0.0 { 0 } else { ((p - f) * 256.0).round() as u32 }))
            })
            .collect()
    };
    let (xs, ys) =
        (axis(view.origin.0, view.scale.0, size.0, *pw), axis(view.origin.1, view.scale.1, size.1, *ph));
    for (y, row) in ys.iter().enumerate() {
        let Some((y0, y1, wy)) = *row else { continue };
        let (r0, r1) = (&src[y0 * pw * 3..][..pw * 3], &src[y1 * pw * 3..][..pw * 3]);
        let line = &mut out[((at.1 + y) * w + at.0) * 3..][..size.0 * 3];
        for (x, col) in xs.iter().enumerate() {
            let Some((x0, x1, wx)) = *col else { continue };
            let d = &mut line[x * 3..x * 3 + 3];
            if wx == 0 && wy == 0 {
                d.copy_from_slice(&r0[x0 * 3..x0 * 3 + 3]);
                continue;
            }
            for c in 0..3 {
                let top = u32::from(r0[x0 * 3 + c]) * (256 - wx) + u32::from(r0[x1 * 3 + c]) * wx;
                let bottom = u32::from(r1[x0 * 3 + c]) * (256 - wx) + u32::from(r1[x1 * 3 + c]) * wx;
                d[c] = ((top * (256 - wy) + bottom * wy + 32_768) >> 16) as u8;
            }
        }
    }
}

/// A caption's box, its height: a row of its glyphs and its air above and below.
fn caption_h(fonts: &Fonts) -> usize {
    shot::cell(fonts, CAPTION_SHORT_PX).1 + 2 * CAPTION_PAD.1
}

/// Where the intro's mascot and name stand above the caption's place: the safe zone's top to `INTRO_ABOVE` over the
/// caption's box (`caption_h` tall), px.
fn intro_room(caption_h: usize) -> (usize, usize) {
    let top = SAFE.0;
    let caption = captions::at(band_px(), (0.0, caption_h as f32));
    (top, (caption.y as usize).saturating_sub(INTRO_ABOVE).max(top))
}

/// How much smaller the intro's mascot (`mascot_h` px tall) is drawn so it and the name (`name_h`) stand in `room` with
/// their gap: 1 where they fit as made.
fn intro_fit(mascot_h: usize, name_h: usize, (top, bottom): (usize, usize)) -> f32 {
    let room = (bottom - top).saturating_sub(INTRO_GAP + name_h) as f32;
    (room / mascot_h.max(1) as f32).min(1.0)
}

/// The intro's gap between the mascot and its name (px).
const INTRO_GAP: usize = 40;

/// `p` made `k` (< 1) of its size, each pixel the mean of those it covers, by their alpha (no dark fringe round the
/// mascot's strokes).
fn shrunk(p: &Rgba, k: f32) -> Rgba {
    let (w, h, src) = p;
    let (nw, nh) = (((*w as f32) * k).round().max(1.0) as usize, ((*h as f32) * k).round().max(1.0) as usize);
    let mut out = vec![0u8; nw * nh * 4];
    let span = |i: usize, n: usize, of: usize| (i * of / n, ((i + 1) * of / n).max(i * of / n + 1).min(of));
    for y in 0..nh {
        let (y0, y1) = span(y, nh, *h);
        for x in 0..nw {
            let (x0, x1) = span(x, nw, *w);
            let (mut sum, mut alpha, mut n) = ([0u32; 3], 0u32, 0u32);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let s = &src[(sy * w + sx) * 4..][..4];
                    for c in 0..3 {
                        sum[c] += u32::from(s[c]) * u32::from(s[3]);
                    }
                    alpha += u32::from(s[3]);
                    n += 1;
                }
            }
            let d = &mut out[(y * nw + x) * 4..][..4];
            for c in 0..3 {
                d[c] = sum[c].checked_div(alpha).unwrap_or(0) as u8;
            }
            d[3] = (alpha / n.max(1)) as u8;
        }
    }
    (nw, nh, out)
}

/// Screens drawn, kept for the next frames: (its index, its glyph size) and its picture — the last `DRAWN_KEPT` (a
/// crossing draws two a frame).
type Drawn = Vec<((usize, u32), Picture)>;
const DRAWN_KEPT: usize = 3;

struct Video<'a> {
    rec: &'a Recorded,
    times: Vec<u64>,
    px: f32,
    look: Look,
    fonts: Fonts,
    /// The title card's frame and the end card's.
    cards: (Vec<u8>, Vec<u8>),
    /// How long the take is shown: to its last screen and `LAST_MS` of it (a Short's: back on the whole, too).
    shown_ms: u64,
    form: Form,
    /// A Short's camera: its path; each read's arrival and target as written (the sheet's marks); the screens that come
    /// by a crossing; each news read's toast (from, to, its text); the screen's size (columns, rows); and the glyph size
    /// the whole screen is drawn at.
    path: camera::Path,
    reads: Vec<(u64, String)>,
    fades: Vec<usize>,
    news: Vec<(u64, u64, String)>,
    screen: (u16, u16),
    whole: f32,
    /// A Short's punchline: the take's ms it opens on (`PUNCH_MS` of it, before the title card).
    punch: Option<u64>,
    /// The video's ms before the take starts: the punchline's and the title card's (or the intro's).
    lead: u64,
    intro: Option<Intro>,
    notes: Vec<Noted>,
}

/// A Short's intro: the swing's frames (RGBA, the mascot alone, made smaller where it must be — `intro_fit`), its name
/// drawn by the recorder, its hook (the Short's title, said as its caption) and the room above the caption's place
/// (`intro_room`).
struct Intro {
    frames: Vec<Rgba>,
    name: Picture,
    hook: captions::Phrase,
    room: (usize, usize),
}

/// Where the intro's parts stand on a Short (x, y — px): the swing's box, then the name under it, centred across the
/// safe area and down its room above the caption's place.
fn intro_laid(intro: &Intro) -> Vec<(usize, usize)> {
    let (_, right, _, left) = SAFE;
    let safe_w = SHORT.0 - left - right;
    let (top, bottom) = intro.room;
    let (bw, bh) = intro.frames.first().map_or((0, 0), |f| (f.0, f.1));
    let tall = bh + INTRO_GAP + intro.name.1;
    let y = top + (bottom - top).saturating_sub(tall) / 2;
    let across = |w: usize| left + safe_w.saturating_sub(w) / 2;
    vec![(across(bw), y), (across(intro.name.0), y + bh + INTRO_GAP)]
}

/// A phrase at its fullest as a caption's lines (`lines`, `wrap`'s), its first `shown` characters each where it stands
/// there — the rest blanks, every line as wide as the widest: nothing moves as a typed part grows.
fn caption_laid(fullest: &str, lines: &[String], shown: usize) -> Vec<String> {
    let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1);
    let mut from = 0;
    lines
        .iter()
        .map(|line| {
            let start = fullest[from..].find(line.as_str()).map_or(from, |b| from + b);
            from = start + line.len();
            let k = shown.saturating_sub(fullest[..start].chars().count()).min(line.chars().count());
            let part: String = line.chars().take(k).collect();
            format!("{part:<width$}")
        })
        .collect()
}

impl Video<'_> {
    fn frames(&self) -> u64 {
        ((self.lead + self.shown_ms + self.form.end_ms()) * FPS).div_ceil(1000)
    }

    /// The take as it stands at its `ms`, screen `i` in force: the beat (and how far its caption has come in), and a
    /// Short's camera there, the screen it crosses from, the news read and the captions standing.
    fn take_at(&self, ms: u64, i: usize) -> Take {
        let beat =
            self.rec.beats.iter().rposition(|b| b.0 <= ms).filter(|&i| !self.rec.beats[i].1.is_empty());
        let came = beat.map_or(255, |i| ((ms - self.rec.beats[i].0) * 255 / FADE_MS).min(255) as u8);
        let mut take = Take {
            i,
            cross: None,
            beat,
            came,
            region: Region::all(self.screen),
            news: None,
            captions: [None; 2],
        };
        if self.form == Form::Short {
            take.region = self.path.at(ms);
            take.news = self.news.iter().position(|n| n.0 <= ms && ms < n.1);
            take.cross = camera::crossing(&self.times, &self.fades, ms)
                .map(|(from, a)| (from, (a * 255.0).round() as u8))
                .filter(|c| c.1 < 255 && c.0 != i);
            take.captions = self.standing(ms);
        }
        take
    }

    /// The captions standing `ms` into the take (`captions::standing`, the last two): which, how drawn (of 255) and how
    /// much of its phrase.
    fn standing(&self, ms: u64) -> [Option<(usize, u8, usize)>; 2] {
        let spans: Vec<Option<captions::Span>> = self.notes.iter().map(|n| n.span).collect();
        let on = captions::standing(&spans, ms);
        let mut out = [None; 2];
        for (slot, (k, a)) in out.iter_mut().zip(&on[on.len().saturating_sub(2)..]) {
            *slot =
                Some((*k, (a * 255.0).round().max(1.0) as u8, captions::shown(&self.notes[*k].phrase, ms)));
        }
        out
    }

    /// The camera's `region` as this video draws it (`view`).
    fn view(&self, region: Region) -> View {
        let (_, _, ww, wh) = short_window();
        view(region, self.screen, (ww, wh), self.whole, |p| shot::cell(&self.fonts, p))
    }

    /// Screen `i` rendered at `px`, kept in `drawn`.
    fn picture<'d>(&self, i: usize, px: f32, drawn: &'d mut Drawn) -> &'d Picture {
        let key = (i, px as u32);
        let k = match drawn.iter().position(|d| d.0 == key) {
            Some(k) => k,
            None => {
                if drawn.len() == DRAWN_KEPT {
                    drawn.remove(0);
                }
                drawn.push((key, shot::render_px(&self.rec.frames[i].1, &self.fonts, px)));
                drawn.len() - 1
            }
        };
        &drawn[k].1
    }

    /// A phrase at its fullest as a caption's lines: wrapped to the safe zone's width less its box's air.
    fn caption_lines(&self, fullest: &str) -> Vec<String> {
        let cols = (safe_w() - 2 * CAPTION_PAD.0 - 2 * PAD) / shot::cell(&self.fonts, CAPTION_SHORT_PX).0;
        wrap(fullest, cols.max(8))
    }

    /// A phrase's caption box at its fullest (px): its lines' picture and its air.
    fn caption_size(&self, fullest: &str) -> (f32, f32) {
        let pic = caption_text(&caption_laid(fullest, &self.caption_lines(fullest), 0), &self.fonts);
        ((pic.0 + 2 * CAPTION_PAD.0) as f32, (pic.1 + 2 * CAPTION_PAD.1) as f32)
    }

    /// What the captions' places say before the take is drawn: each caption — the hook too — whose box the band doesn't
    /// hold (it would run down into what YouTube's own title covers), with its lines and px against the band's.
    fn checked(&self) -> Vec<String> {
        let band = band_px();
        let hook = self.intro.as_ref().map(|i| ("the hook".to_string(), i.hook.fullest()));
        let notes =
            (self.notes.iter()).map(|n| (format!("@note:{} → {}", n.written, n.target), n.phrase.fullest()));
        let mut said = Vec::new();
        for (name, fullest) in hook.into_iter().chain(notes) {
            let size = self.caption_size(fullest);
            if !band.holds(captions::at(band, size)) {
                said.push(format!(
                    "{name}: its caption, {} line(s) {:.0} px tall, past the band's {:.0} px under the screen",
                    self.caption_lines(fullest).len(),
                    size.1,
                    band.h
                ));
            }
        }
        said
    }

    /// A caption on the frame, `alpha` of it: its phrase's first `shown` characters laid where its fullest stands
    /// (`caption_laid`), JetBrains Mono Bold on its quiet box at the caption's one place in the band (`captions::at`),
    /// its corners round. Its pictures kept in `pics`.
    fn caption_drawn(
        &self,
        out: &mut [u8],
        fullest: &str,
        shown: usize,
        alpha: f32,
        pics: &mut HashMap<String, Picture>,
    ) {
        let laid = caption_laid(fullest, &self.caption_lines(fullest), shown);
        let pic = pics.entry(format!("caption {laid:?}")).or_insert_with(|| caption_text(&laid, &self.fonts));
        let (pw, ph) = (pic.0, pic.1);
        let (w, h) = (pw + 2 * CAPTION_PAD.0, ph + 2 * CAPTION_PAD.1);
        let r = captions::at(band_px(), (w as f32, h as f32));
        let (x0, y0) = (r.x.round().max(0.0) as usize, r.y.round().max(0.0) as usize);
        let shape = Px { x: 0.0, y: 0.0, w: w as f32, h: h as f32 };
        for y in 0..h.min(SHORT.1.saturating_sub(y0)) {
            for x in 0..w.min(SHORT.0.saturating_sub(x0)) {
                let edge = captions::rounded(shape, CAPTION_R, (x as f32 + 0.5, y as f32 + 0.5));
                let cover = (0.5 - edge).clamp(0.0, 1.0) * alpha;
                if cover <= 0.0 {
                    continue;
                }
                let (tx, ty) = (x.wrapping_sub(CAPTION_PAD.0), y.wrapping_sub(CAPTION_PAD.1));
                let c = if tx < pw && ty < ph {
                    let s = &pic.2[(ty * pw + tx) * 3..][..3];
                    [s[0], s[1], s[2]]
                } else {
                    CAPTION_GROUND
                };
                mix(&mut out[((y0 + y) * SHORT.0 + x0 + x) * 3..][..3], c, (cover * 255.0).round() as u8);
            }
        }
    }

    /// The `k`th frame's ms in the video.
    fn ms(k: u64) -> u64 {
        k * 1000 / FPS
    }

    fn shows(&self, k: u64) -> Shows {
        match part(Self::ms(k), self.punch, self.lead, self.shown_ms) {
            // (the punchline: the take from its mark, its caption standing as it was there)
            Part::Punch(at) => {
                let i = self.times.partition_point(|&s| s <= at).saturating_sub(1);
                Shows::Screen(Take { came: 255, ..self.take_at(at, i) })
            }
            Part::Title if self.intro.is_some() => {
                Shows::Intro(Self::ms(k) - self.punch.map_or(0, |_| PUNCH_MS))
            }
            Part::Title => Shows::Title,
            Part::Take(at) => self.take_at(at, screen_at(&self.times, k - self.lead * FPS / 1000)).into(),
            Part::End => Shows::End,
        }
    }

    /// The frame `shows` says, its screens drawn into `drawn` when they aren't there and its words' pictures (a
    /// caption's) kept in `pics`.
    fn frame(&self, shows: Shows, drawn: &mut Drawn, pics: &mut HashMap<String, Picture>) -> Vec<u8> {
        let take = match shows {
            Shows::Title => return self.cards.0.clone(),
            Shows::Intro(ms) => return self.intro_frame(ms, pics),
            Shows::End => return self.cards.1.clone(),
            Shows::Screen(take) => take,
        };
        if self.form == Form::Short {
            return self.short_frame(take, drawn, pics);
        }
        let screen = self.picture(take.i, self.px, drawn);
        let mut px = self.look.ground.repeat(W * H);
        let (x0, y0) = ((W - screen.0) / 2, (H - BAND - screen.1) / 2);
        blit(&mut px, (W, H), screen, x0, y0, 255);
        // (the band's hairline, the screen's cells wide)
        let at = (H - BAND) * W * 3;
        for x in x0 + PAD..x0 + screen.0 - PAD {
            px[at + x * 3..][..3].copy_from_slice(&self.look.faint);
        }
        if let Some(b) = take.beat {
            let said = &self.rec.beats[b].1;
            let caption = pics.entry(said.clone()).or_insert_with(|| {
                text(said, CAPTION_PX, self.look.ink, self.look.ground, false, &self.fonts)
            });
            blit(
                &mut px,
                (W, H),
                caption,
                (W - caption.0.min(W)) / 2,
                H - BAND + (BAND - caption.1) / 2,
                take.came,
            );
        }
        px
    }

    /// A Short's intro `ms` in: the swing's frame then (the last held) over the ground, the name rising in under it
    /// (`NAME_RISE`: eased, `INTRO_RISE_PX` up), and the hook said as the caption, from the brush's tap (`HOOK_FROM`),
    /// eased in.
    fn intro_frame(&self, ms: u64, pics: &mut HashMap<String, Picture>) -> Vec<u8> {
        let Some(intro) = &self.intro else { return self.cards.0.clone() };
        let mut out = self.look.ground.repeat(SHORT.0 * SHORT.1);
        let laid = intro_laid(intro);
        let k = ((ms * INTRO_FPS / 1000) as usize).min(intro.frames.len() - 1);
        let (fw, fh, rgba) = &intro.frames[k];
        let (x0, y0) = laid[0];
        for y in 0..*fh {
            for x in 0..*fw {
                let s = &rgba[(y * fw + x) * 4..][..4];
                if s[3] > 0 && y0 + y < SHORT.1 && x0 + x < SHORT.0 {
                    let at = ((y0 + y) * SHORT.0 + x0 + x) * 3;
                    mix(&mut out[at..at + 3], [s[0], s[1], s[2]], s[3]);
                }
            }
        }
        let rise = |(from, to): (u64, u64)| {
            camera::ease_out(ms.saturating_sub(from) as f32 / (to - from) as f32) * f32::from(ms >= from)
        };
        let up = rise(NAME_RISE);
        if up > 0.0 {
            let (x, y) = laid[1];
            blit(
                &mut out,
                SHORT,
                &intro.name,
                x,
                y + ((1.0 - up) * INTRO_RISE_PX) as usize,
                (up * 255.0) as u8,
            );
        }
        let shown = captions::shown(&intro.hook, ms);
        if shown > 0 {
            let a = camera::ease_in_out(ms.saturating_sub(HOOK_FROM) as f32 / captions::FADE_MS as f32);
            self.caption_drawn(&mut out, intro.hook.fullest(), shown, a, pics);
        }
        out
    }

    /// A Short's frame: the screen in force drawn through the camera's region (`view`, `resample`) — over the one it
    /// crosses from, where it crosses (`camera::crossing`) —, a news read's toast laid over the window's top (`callout`),
    /// and the captions standing over it all (`caption_drawn`).
    fn short_frame(&self, take: Take, drawn: &mut Drawn, pics: &mut HashMap<String, Picture>) -> Vec<u8> {
        let view = self.view(take.region);
        let mut out = self.look.ground.repeat(SHORT.0 * SHORT.1);
        self.screen_drawn(&mut out, take.i, take.news, &view, drawn);
        if let Some((from, a)) = take.cross {
            let mut before = self.look.ground.repeat(SHORT.0 * SHORT.1);
            self.screen_drawn(&mut before, from, take.news, &view, drawn);
            for (b, n) in before.as_chunks_mut::<3>().0.iter_mut().zip(out.as_chunks::<3>().0) {
                mix(b, *n, a);
            }
            out = before;
        }
        for (k, a, shown) in take.captions.into_iter().flatten() {
            self.caption_drawn(&mut out, self.notes[k].phrase.fullest(), shown, f32::from(a) / 255.0, pics);
        }
        out
    }

    /// Screen `i` through `view` into a Short's window on `out`, its news read's toast (`news`) laid over the window's
    /// top-right as it stands on the screen — each cell's px the screen's own (`camera::callout`) — and left out where
    /// it stands.
    fn screen_drawn(&self, out: &mut [u8], i: usize, news: Option<usize>, view: &View, drawn: &mut Drawn) {
        let (wx, wy, ww, wh) = short_window();
        let pic = self.picture(i, view.px, drawn);
        let laid = news.and_then(|m| {
            let cells = camera::Cells::of(&self.rec.frames[i].1);
            let (c0, r0, c1, r1) = view.cells(self.screen, (ww, wh));
            let toast = camera::callout(&cells, &self.news[m].2, (c1 - c0).saturating_sub(2))?;
            let (pw, ph, src) = pic;
            let (cw, ch) = view.cell;
            let mut px = src.clone();
            let cell = |px: &mut Vec<u8>, (r, c): (usize, usize), from: Option<(usize, usize)>| {
                for dy in 0..ch.min(ph.saturating_sub(PAD + r * ch)) {
                    let y = PAD + r * ch + dy;
                    let n = cw.min(pw.saturating_sub(PAD + c * cw));
                    let at = (y * pw + PAD + c * cw) * 3;
                    match from {
                        Some((sr, sc)) => {
                            let s = ((PAD + sr * ch + dy) * pw + PAD + sc * cw) * 3;
                            px[at..at + n * 3].copy_from_slice(&src[s..s + n * 3]);
                        }
                        None => px[at..at + n * 3].copy_from_slice(&self.look.ground.repeat(n)),
                    }
                }
            };
            let (tr0, tc0, tr1, tc1) = toast.card;
            for r in tr0..=tr1 {
                for c in tc0..=tc1 {
                    cell(&mut px, (r, c), None);
                }
            }
            let (top, left) = camera::callout_at((c0, r0, c1, r1), toast.lines.first().map_or(0, Vec::len));
            for (k, line) in toast.lines.iter().enumerate() {
                for (j, from) in line.iter().enumerate() {
                    if top + k < r1 && left + j < c1 {
                        cell(&mut px, (top + k, left + j), Some(from.unwrap_or(toast.ground)));
                    }
                }
            }
            Some((*pw, *ph, px))
        });
        resample(out, SHORT.0, (wx, wy), (ww, wh), laid.as_ref().unwrap_or(pic), view);
    }

    /// Every frame through ffmpeg into `mp4`: raw RGB on its stdin, H.264 `slow` CRF 20 tuned for flat pictures, 4:2:0
    /// in BT.709 (what a phone's player assumes for HD), the index at the front; `audio` (a WAV) as AAC beside it. How
    /// many frames differ from the one before.
    fn encode(&self, ffmpeg: &Path, mp4: &Path, audio: Option<&Path>) -> Result<u64> {
        let (w, h) = self.form.dims();
        let size = format!("{w}x{h}");
        let fps = FPS.to_string();
        let mut cmd = Command::new(ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s"])
            .args([&size, "-r", &fps, "-i", "-"]);
        if let Some(wav) = audio {
            cmd.arg("-i").arg(wav).args(["-map", "0:v", "-map", "1:a", "-c:a", "aac", "-b:a", "160k"]);
        }
        let mut child = cmd
            .args(["-c:v", "libx264", "-preset", "slow", "-crf", "20"])
            .args(["-tune", "animation", "-vf", "scale=out_color_matrix=bt709:out_range=tv,format=yuv420p"])
            .args(["-colorspace", "bt709", "-color_range", "tv", "-movflags", "+faststart"])
            // (the primaries and transfer written into the stream itself: the filter's frames leave them unspecified,
            // whatever `-color_primaries` says)
            .args([
                "-bsf:v",
                "h264_metadata=colour_primaries=1:transfer_characteristics=1:matrix_coefficients=1",
            ])
            .arg(mp4)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{}: {e}", ffmpeg.display()))?;
        let mut stdin = child.stdin.take().ok_or("ffmpeg's stdin")?;
        let (mut drawn, mut captions) = (Vec::new(), HashMap::new());
        let mut last: Option<(Shows, Vec<u8>)> = None;
        let (mut wrote, mut differ) = (Ok(()), 0);
        for k in 0..self.frames() {
            let shows = self.shows(k);
            if last.as_ref().is_none_or(|l| l.0 != shows) {
                let px = self.frame(shows, &mut drawn, &mut captions);
                differ += u64::from(last.as_ref().is_none_or(|l| l.1 != px));
                last = Some((shows, px));
            }
            if let Some((_, px)) = &last
                && let Err(e) = stdin.write_all(px)
            {
                wrote = Err(e);
                break;
            }
        }
        drop(stdin);
        let out = child.wait_with_output().map_err(|e| format!("{}: {e}", ffmpeg.display()))?;
        let said = String::from_utf8_lossy(&out.stderr).trim().to_string();
        match (out.status.success(), wrote) {
            (true, Ok(())) => Ok(differ),
            (_, Err(e)) => Err(format!("ffmpeg stopped taking frames ({e}): {said}")),
            (false, _) => Err(format!("ffmpeg: {} — {said}", out.status)),
        }
    }

    /// The sheet of the video's own frames at `out` (`recording::sheet_picks`: one every 2 s and at each beat — a
    /// Short's at each read's arrival and each crossing too —, the cards' first frames too), each at half size with its
    /// time in the video and its beat under it.
    fn sheet(&self, out: &Path) -> Result<usize> {
        let lead = self.lead;
        let dims = self.form.dims();
        let times: Vec<u64> = (0..self.frames()).map(Self::ms).collect();
        let mut marks: Vec<(u64, String)> =
            self.rec.beats.iter().filter(|b| !b.1.is_empty()).map(|(t, c)| (lead + t, c.clone())).collect();
        for (t, spec) in &self.reads {
            marks.push((lead + t, format!("zoom:{spec}")));
        }
        for &i in &self.fades {
            marks.push((lead + self.times[i] + camera::CROSS_MS / 2, "crossing".into()));
        }
        if self.punch.is_some() {
            marks.push((PUNCH_MS / 2, "punchline".into()));
        }
        if self.intro.is_some() {
            let from = self.punch.map_or(0, |_| PUNCH_MS);
            marks.extend([(from + 800, "intro: the tap".into()), (lead - 100, "intro: the name".into())]);
        }
        marks.sort_by_key(|m| m.0);
        // (the end card's first frame: the first whose ms is past the take's)
        marks.push((Self::ms(((lead + self.shown_ms) * FPS).div_ceil(1000)), "end card".into()));
        let picks = recording::sheet_picks(&times, &marks, recording::SHEET_EVERY_MS);
        let (mut drawn, mut captions) = (Vec::new(), HashMap::new());
        let pictures: Vec<Picture> = picks
            .iter()
            .map(|(k, said)| {
                let frame = self.frame(self.shows(*k as u64), &mut drawn, &mut captions);
                recording::labelled(
                    half(&frame, dims),
                    said,
                    dims.0 / 2 / shot::cell(&self.fonts, 15.0).0,
                    &self.fonts,
                )
            })
            .collect();
        recording::pages(&pictures, out)
    }
}

impl From<Take> for Shows {
    fn from(take: Take) -> Shows {
        Shows::Screen(take)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The limiter's ceiling follows what the encoder did: lowered by an AAC over the aim (−1.3 dBTP), raised by one
    /// well under it — never past the aim —, left where the AAC came near enough.
    #[test]
    fn the_ceiling_moves_until_the_aacs_peak_is_near_its_aim() {
        let near = |a: Option<f32>, b: f32| a.is_some_and(|a| (a - b).abs() < 1e-4);
        assert!(near(next_ceiling(&[(-2.0, 1.2)]), -4.5), "over: lowered");
        assert!(near(next_ceiling(&[(-2.0, -1.9)]), -1.4), "well under: raised");
        assert!(near(next_ceiling(&[(-1.4, -2.6)]), -1.3), "raised no further than the aim");
        assert_eq!(next_ceiling(&[(-2.0, -1.4)]), None, "near enough");
        assert_eq!(next_ceiling(&[(-1.3, -2.0)]), None, "at the aim already");
        // (V1's take of 2026-11-05: -2.0 came out -1.9, -1.4 out -0.7 — lowered back to -2.0 it went round)
        assert!(near(next_ceiling(&[(-2.0, -1.9), (-1.4, -0.7)]), -1.7), "between the two");
        assert!(near(next_ceiling(&[(-1.4, -0.7), (-1.7, -1.0)]), -2.0), "under the lowest over");
        assert_eq!(next_ceiling(&[(-1.7, -1.0), (-1.75, -1.9)]), None, "no room left between");
        // (kept: the highest under the AAC's limit, else the lowest AAC)
        assert_eq!(best_ceiling(&[(-2.0, -1.9), (-1.4, -0.7), (-1.7, -1.0)]), Some(-1.7));
        assert_eq!(best_ceiling(&[(-1.7, -0.9), (-1.4, -0.7)]), Some(-1.7), "none under: the lowest");
        assert_eq!(best_ceiling(&[(-1.4, -1.2), (-1.7, -0.9)]), Some(-1.4), "not the last");
        assert_eq!(best_ceiling(&[]), None);
    }

    /// A take whose screens came at 0, 40, 41 and 1000 ms: frame 0 (0 ms) shows the first; frame 1 (33.3 ms) still the
    /// first; frame 2 (66.7 ms) the third — the second, 1 ms before it, is never shown whole; frame 30 (1 s exactly)
    /// the fourth, drawn at that ms; frame 29 (966.7 ms) the third.
    #[test]
    fn each_frame_shows_the_screen_in_force_at_its_time() {
        let times = [0, 40, 41, 1000];
        let shown: Vec<usize> = (0..=31).map(|k| screen_at(&times, k)).collect();
        assert_eq!(&shown[..3], &[0, 0, 2]);
        assert_eq!(shown[29], 2);
        assert_eq!(&shown[30..], &[3, 3]);
        // (a screen drawn on a frame's own ms is in force on that frame: 100 ms is frame 3's)
        assert_eq!(screen_at(&[0, 100], 2), 0);
        assert_eq!(screen_at(&[0, 100], 3), 1);
        // (a take starting late: its first screen stands in from the first frame)
        assert_eq!(screen_at(&[50], 0), 0);
    }

    /// A Short with a punchline opens on the take from its mark for `PUNCH_MS`, then the title card, then the take from
    /// its start (its first beat on the whole), then the end card — a Short's take holding its last screen only for its
    /// crossing, the wide one's `LAST_MS`; without one, the title card first.
    #[test]
    fn a_punchline_opens_the_video_then_the_title_then_the_take_from_its_start() {
        assert_eq!((Form::Short.last_ms(), Form::Wide.last_ms()), (camera::CROSS_MS, LAST_MS));
        let (title, shown) = (1_200, 10_000);
        let lead = PUNCH_MS + title;
        assert_eq!(part(0, Some(7_000), lead, shown), Part::Punch(7_000));
        assert_eq!(part(PUNCH_MS - 1, Some(7_000), lead, shown), Part::Punch(7_000 + PUNCH_MS - 1));
        assert_eq!(part(PUNCH_MS, Some(7_000), lead, shown), Part::Title);
        assert_eq!(part(lead - 1, Some(7_000), lead, shown), Part::Title);
        assert_eq!(part(lead, Some(7_000), lead, shown), Part::Take(0));
        assert_eq!(part(lead + shown, Some(7_000), lead, shown), Part::End);
        assert_eq!(part(0, None, title, shown), Part::Title);
        assert_eq!(part(title + 5, None, title, shown), Part::Take(5));
    }

    fn rows(text: &[&str]) -> Vec<Option<String>> {
        text.iter().map(|r| Some(r.to_string())).collect()
    }

    /// What Claude's writing newly shows from one screen to the next: the characters a row gained; a line written
    /// below another, the rows under it moved down and unchanged, as a new line; a view that moved, many rows.
    #[test]
    fn the_writing_shows_what_each_screen_adds() {
        let a = rows(&["fn main() {", "    let x", "}"]);
        let b = rows(&["fn main() {", "    let x = 1;", "}"]);
        assert_eq!(shown(&a, &b), (" = 1;".into(), 1));
        let c = rows(&["fn main() {", "    let x = 1;", "    let y", "}"]);
        assert_eq!(shown(&b, &c), ("\nlet y".into(), 1));
        assert_eq!(shown(&c, &c), (String::new(), 0));
        let jumped = rows(&["pub struct Thread {", "    pub name: String,", "}", "impl Thread {"]);
        let (_, changed) = shown(&c, &jumped);
        assert!(changed == 3 && changed > WRITING_ROWS_MAX, "a view moved: every row but the one both have");
    }

    /// A screen of a code pane: its top line, `code` numbered from `first`, the status line.
    fn screen(first: u32, code: &[&str]) -> vt100::Screen {
        let mut p = vt100::Parser::new(6, 40, 0);
        let mut text = String::from("\x1b[H─ main.rs ─");
        for (i, line) in code.iter().enumerate() {
            text.push_str(&format!("\r\n {} {line}", first + i as u32));
        }
        text.push_str("\x1b[6;1H● normal");
        p.process(text.as_bytes());
        p.screen().clone()
    }

    /// Claude's writing in a take, as the reveal hears it: inside a landing's span each screen's new code a step (what
    /// it showed, how long it stood), the view's jump between them none, and each burst settling as it lands — never a
    /// step outside the span.
    #[test]
    fn the_writing_is_heard_step_by_step_and_settles_where_it_lands() {
        let frames = vec![
            (0, screen(1, &["fn main() {", "    let x", "}"])),
            (100, screen(1, &["fn main() {", "    let x = 1", "}"])),
            (150, screen(1, &["fn main() {", "    let x = 1;", "}"])),
            // (the view jumps: every row another)
            (1_000, screen(7, &["pub fn b() {", "    y", "    z"])),
            (1_100, screen(7, &["pub fn b() {", "    y = 2", "    z"])),
            // (past the span's end)
            (2_000, screen(7, &["pub fn b() {", "    y = 2 + 3", "    z"])),
        ];
        let sounds = vec![(50, "landing".to_string()), (1_500, "still".to_string())];
        let rec = Recorded {
            frames,
            beats: Vec::new(),
            waits: Vec::new(),
            zooms: Vec::new(),
            notes: Vec::new(),
            keys: Vec::new(),
            sounds,
            punch: None,
            missed: Vec::new(),
            wire: Vec::new(),
            end: 0,
        };
        let ms = Duration::from_millis;
        assert_eq!(
            writing(&rec),
            vec![
                (100, Heard::Shown(" = 1".into(), ms(50))),
                (150, Heard::Shown(";".into(), ms(100))),
                (150 + LANDS_MS, Heard::Settle),
                (1_100, Heard::Shown(" = 2".into(), ms(100))),
                (1_100 + LANDS_MS, Heard::Settle),
            ]
        );
    }

    /// A Short's intro stands inside the safe area above the caption's place, where its hook is said: the swing's box,
    /// then the name under it, each centred across; the mascot made smaller where the two don't fit there as made.
    #[test]
    fn a_shorts_intro_stands_above_its_hook_name_under_the_mascot() {
        let room = intro_room(100);
        let caption = captions::at(band_px(), (500.0, 100.0));
        assert_eq!(room, (SAFE.0, caption.y as usize - INTRO_ABOVE));
        let k = intro_fit(1_300, 140, room);
        assert!(k < 1.0, "1300 + 140 don't fit {room:?}");
        let h = (1_300.0 * k).round() as usize;
        assert!(room.0 + h + INTRO_GAP + 140 <= room.1 + 1, "made to fit: {h}");
        assert_eq!(intro_fit(400, 140, room), 1.0, "fits as made");
        let small = shrunk(&(4, 2, [[255, 0, 0, 255], [0, 0, 255, 0]].repeat(4).concat()), 0.5);
        assert_eq!(small, (2, 1, vec![255, 0, 0, 127, 255, 0, 0, 127]), "by alpha: no fringe");
        let pic = |w: usize, h: usize| (w, h, vec![0; w * h * 3]);
        let w = (880.0 * k).round() as usize;
        let hook = || captions::Phrase { said: Vec::new() };
        let intro =
            Intro { frames: vec![(w, h, vec![0; w * h * 4])], name: pic(520, 140), hook: hook(), room };
        let laid = intro_laid(&intro);
        let (_, right, _, left) = SAFE;
        let sizes = [(w, h), (520, 140)];
        for (&(x, y), &(w, h)) in laid.iter().zip(&sizes) {
            assert!(left <= x && x + w <= SHORT.0 - right, "across: {x} {w}");
            assert!(room.0 <= y && y + h <= room.1 + 1, "down, above the caption: {y} {h}");
        }
        // (a mascot smaller than its room: in the middle of it)
        let fits = Intro {
            frames: vec![(400, 400, vec![0; 400 * 400 * 4])],
            name: pic(520, 140),
            hook: hook(),
            room,
        };
        let at = intro_laid(&fits);
        let (above, below) = (at[0].1 - room.0, room.1 - (at[1].1 + 140));
        assert!(above.abs_diff(below) <= 1 && above > 100, "in the middle of its room: {above} {below}");
        for (&(x, _), &(w, _)) in laid.iter().zip(&sizes) {
            let middle = (left + SHORT.0 - right) / 2;
            assert!((x + w / 2).abs_diff(middle) <= 1, "centred: {x} {w}");
        }
        assert!(
            laid.windows(2).zip(&sizes).all(|(p, s)| p[0].1 + s.1 <= p[1].1),
            "one under another: {laid:?}"
        );
    }

    /// A caption stands from the first screen at or after its mark that draws its target to the first after that which
    /// doesn't, or to the next caption's coming; one not `<words> → <target>`, one past `WORDS_MAX` words, one never
    /// found and one found only past `FIND_MS` are said, and so is one standing under `captions::STAND_MIN_MS` (with when
    /// and how long, the last to the take's end). Its phrase whole from its coming.
    #[test]
    fn a_caption_stands_while_its_target_is_on_the_screen() {
        let tagged = |tag: &str| screen(1, &[&format!("fn main() {{ {tag}"), "    let x = 1;"]);
        let plain = || screen(1, &["fn main() {", "    let x = 1;"]);
        let notes = [
            (0, "fix's Claude → ‹fix"),
            (600, "the code → code:2-2"),
            (150, "no arrow here"),
            (800, "one two three four five six seven → code:1-1"),
            (300, "nowhere → card:nowhere"),
            (300, "late → ‹late"),
        ];
        let late = 300 + FIND_MS + 100;
        let rec = Recorded {
            frames: vec![(0, plain()), (50, tagged("‹fix ●›")), (500, plain()), (late, tagged("‹late"))],
            beats: vec![(0, "one".into())],
            waits: Vec::new(),
            zooms: Vec::new(),
            notes: notes.iter().map(|(t, n)| (*t, n.to_string())).collect(),
            keys: Vec::new(),
            sounds: Vec::new(),
            punch: None,
            missed: Vec::new(),
            wire: Vec::new(),
            end: 0,
        };
        let (noted, missed) = noted(&rec);
        let spans: Vec<_> =
            noted.iter().map(|n| (n.target.as_str(), n.span.map(|s| (s.from, s.to)))).collect();
        assert_eq!(
            spans,
            [("‹fix", Some((50, 500))), ("code:2-2", Some((600, 800))), ("code:1-1", Some((800, u64::MAX)))],
            "in as its target is drawn, out as it goes or the next comes"
        );
        assert_eq!(noted[0].phrase.said, [(50, "fix's Claude".to_string())]);
        assert_eq!(missed.len(), 4, "{missed:?}");
        assert!(missed[0].contains("no arrow here") && missed[0].contains("not `<words> → <target>`"));
        assert!(missed[1].contains("past 4 words"));
        assert!(missed[2].contains("card:nowhere") && missed[2].contains("not on screen"));
        assert!(missed[3].contains("‹late") && missed[3].contains("not on screen within"), "past `FIND_MS`");
        // (each phrase for the time the scene gives it: fix's 0.45 s and the code's 0.2 s named; the last to the end)
        let short = stood_short(&noted, 2_000);
        assert_eq!(short.len(), 2, "{short:?}");
        assert!(short[0].contains("“fix's Claude” stands 0.5 s from 0.1 s, under 1.2 s"), "{short:?}");
        assert!(short[1].contains("“the code” stands 0.2 s"), "{short:?}");
        assert_eq!(stood_short(&noted, 1_900).len(), 3, "the last stands to the take's end");
    }

    /// A phrase is laid out at its fullest, its shown characters each where they stand there, the rest blanks and every
    /// line as wide as the widest — so its box never changes size and nothing moves as a typed part grows.
    #[test]
    fn a_caption_is_laid_at_its_fullest_its_typed_part_growing_in_place() {
        let fullest = "you ask: keep the first";
        let lines = vec!["you ask: keep".to_string(), "the first".to_string()];
        assert_eq!(caption_laid(fullest, &lines, 0), [" ".repeat(13), " ".repeat(13)]);
        assert_eq!(caption_laid(fullest, &lines, 11), ["you ask: ke  ".to_string(), " ".repeat(13)]);
        assert_eq!(caption_laid(fullest, &lines, 17), ["you ask: keep", "the          "]);
        assert_eq!(caption_laid(fullest, &lines, 99), ["you ask: keep", "the first    "]);
    }

    /// The camera draws its region at any scale: at the whole the picture's own px (a scale of 1, a whole origin —
    /// sharp); nearer, the region as large on the window as the whole screen is and its middle at the window's,
    /// whatever glyph size it's drawn at, a cell on the window the whole's times how near — so a zoom never steps.
    #[test]
    fn the_camera_draws_its_region_at_any_scale() {
        let cell = |px: f32| ((px * 0.6) as usize, (px * 1.45) as usize);
        let (_, _, ww, wh) = short_window();
        let (screen, window) = ((80, 43), (ww, wh));
        let whole = camera::fit(Region::all(screen), window, SHORT_PX_MAX, cell);
        let v = view(Region::all(screen), screen, window, whole, cell);
        assert_eq!((v.px, v.scale), (whole, (1.0, 1.0)));
        assert_eq!((v.origin.0.fract(), v.origin.1.fract()), (0.0, 0.0), "{v:?}");
        let (wx, wy, ww, wh) = short_window();
        let (cw0, ch0) = cell(whole);
        let mut sizes = Vec::new();
        for k in 0..=60 {
            let push = 1.0 + k as f32 / 100.0;
            let (w, h) = (80.0 / push, 43.0 / push);
            let r = Region { x: 40.0 - w / 2.0, y: 21.5 - h / 2.0, w, h };
            let v = view(r, screen, window, whole, cell);
            let on = v.px(r);
            let fills = (on.w - 80.0 * cw0 as f32).abs() < 0.5 && (on.h - 43.0 * ch0 as f32).abs() < 0.5;
            assert!(fills, "{push}: {on:?}");
            assert!((on.x + on.w / 2.0 - (wx as f32 + ww as f32 / 2.0)).abs() < 0.5, "{push}: {on:?}");
            assert!((on.y + on.h / 2.0 - (wy as f32 + wh as f32 / 2.0)).abs() < 0.5, "{push}: {on:?}");
            assert!((v.cell.0 as f32 / v.scale.0 - cw0 as f32 * push).abs() < 1e-2, "{push}: {v:?}");
            sizes.push(v.px);
        }
        sizes.dedup();
        assert!(sizes.len() > 2, "drawn at several glyph sizes, taken down to each push: {sizes:?}");
    }

    /// A Short's screen fills the frame's width (CLAUDE.md's Shorts bullet): its window the frame's 1,080 px across from
    /// the safe area's top, its 80 columns at the largest glyph whose cells (and the renderer's margin) fit them, drawn at
    /// its own px; as many rows as leave the band under it two caption lines' box and no more, the whole's last row at
    /// the band's top. The captions stand in that band, from the window's last row to the safe area's bottom — where
    /// YouTube's own title starts —, across the safe area (its buttons over the right edge); a zoom's picture drawn only
    /// above the band, so it stays the band.
    #[test]
    fn a_shorts_screen_fills_the_width() {
        let fonts = Fonts::load(1).unwrap();
        let cell = |p: f32| shot::cell(&fonts, p);
        let (wx, wy, ww, wh) = short_window();
        assert_eq!((wx, wy, ww), (0, SAFE.0, SHORT.0), "the frame's width, under YouTube's bar");
        let band = band_px();
        assert_eq!(band.y as usize, wy + wh, "under the window");
        assert_eq!((band.y + band.h) as usize, SHORT.1 - SAFE.2, "inside the safe area, to its bottom");
        assert_eq!((band.x as usize, band.w as usize), (SAFE.3, safe_w()));
        let screen = short_size().unwrap();
        let whole = camera::fit(Region::all(screen), (ww, wh), SHORT_PX_MAX, cell);
        let across = |p: f32| 80 * cell(p).0 + 2 * PAD;
        assert!(
            across(whole) <= SHORT.0 && across(whole + 1.0) > SHORT.0,
            "the largest glyph across: {whole}"
        );
        let lines = vec!["x".to_string(); 2];
        let two = caption_text(&lines, &fonts).1 + 2 * CAPTION_PAD.1;
        let (rows, ch) = (usize::from(screen.1), cell(whole).1);
        assert_eq!(rows * ch + 2 * PAD, wh, "the window the screen's rows");
        assert!(
            two <= SHORT_BAND && SHORT_BAND < two + ch,
            "the band two lines, not a row more: {SHORT_BAND} {two}"
        );
        let on = view(Region::all(screen), screen, (ww, wh), whole, cell).px(Region::all(screen));
        let gap = band.y - (on.y + on.h);
        assert!(
            (0.0..cell(whole).1 as f32).contains(&gap),
            "the last row at the band's top: {on:?} {band:?}"
        );
        // (a deep zoom on the screen's last rows: drawn into the window, the band left as it was)
        let region = Region { x: 35.0, y: f32::from(screen.1) - 20.0, w: 10.0, h: 20.0 };
        let v = view(region, screen, (ww, wh), whole, cell);
        let (pw, ph) = (80 * v.cell.0 + 2 * PAD, rows * v.cell.1 + 2 * PAD);
        let pic: Picture = (pw, ph, vec![255; pw * ph * 3]);
        let mut out = vec![0u8; SHORT.0 * SHORT.1 * 3];
        resample(&mut out, SHORT.0, (wx, wy), (ww, wh), &pic, &v);
        let row = |y: usize| &out[(y * SHORT.0 + wx) * 3..][..ww * 3];
        assert!(row(wy + wh - 1).iter().any(|&b| b > 0), "the window drawn to its last row");
        assert!((wy + wh..SHORT.1).all(|y| row(y).iter().all(|&b| b == 0)), "the band left to the captions");
    }

    /// A take's last screen is held its crossing (a wide video's `LAST_MS`), and a Short's on to the take's own end (its
    /// run past its last step) and until the caption standing to its end has stood `STAND_MIN_MS` — one that went before
    /// the end, or stood its time already, holds nothing.
    #[test]
    fn a_shorts_last_screen_holds_till_its_last_caption_has_stood() {
        let span = |from, to| Some(captions::Span { from, to });
        let end = [span(2_000, 5_000), span(9_700, u64::MAX)];
        assert_eq!(shown_to(10_000, 0, Form::Short, &end), 9_700 + captions::STAND_MIN_MS);
        assert_eq!(shown_to(10_000, 12_000, Form::Wide, &end), 10_000 + LAST_MS);
        assert_eq!(shown_to(10_000, 0, Form::Short, &[span(8_000, u64::MAX)]), 10_000 + camera::CROSS_MS);
        assert_eq!(shown_to(10_000, 0, Form::Short, &[span(9_900, 10_100), None]), 10_000 + camera::CROSS_MS);
        assert_eq!(
            shown_to(10_000, 12_400, Form::Short, &end),
            12_400,
            "the take ran on past its last frame"
        );
    }

    /// A Short's last beat stands `TAIL_MIN_MS` before the end card: a take cut ~1 s after it (V3's bring card at 22.0 s,
    /// the end card at 22.7 s) is said with its caption, how long and from when; one run on `shot::TAIL_S` past its last
    /// step, the step that waited for the beat, isn't.
    #[test]
    fn a_shorts_last_beat_stands_its_tail_before_the_end_card() {
        let noted = |from: u64| Noted {
            written: "fix: rebase, then bring".into(),
            target: "card:Bring fix".into(),
            span: Some(captions::Span { from, to: u64::MAX }),
            phrase: captions::phrase("fix: rebase, then bring", from, &[]),
        };
        let notes = [noted(18_300)];
        let spans = [notes[0].span];
        let short = tail_short(&notes, captions::tail(&spans, 19_000)).expect("cut 0.7 s after it");
        assert!(short.contains("“fix: rebase, then bring” stands 0.7 s from 18.3 s"), "{short}");
        assert!(
            tail_short(&notes, captions::tail(&spans, 20_100)).is_some(),
            "1.8 s: a phrase's time, not a tail's"
        );
        let taken = 18_300 + (shot::TAIL_S * 1000.0) as u64;
        let shown = shown_to(18_300, taken, Form::Short, &spans);
        assert_eq!(tail_short(&notes, captions::tail(&spans, shown)), None, "run on past it");
    }

    /// A read's glyph is the whole's through its window: at 80×43's 18 px, ~38 columns across reads (~2.5 mm on a
    /// phone), 41 is under `READ_MM_MIN`, the old shallow 56 far under it.
    #[test]
    fn a_reads_glyph_is_the_wholes_through_its_window() {
        let at = |w: f32| read_glyph(18.0, Region { x: 0.0, y: 0.0, w, h: w * 43.0 / 80.0 }, (80, 43));
        let (px, mm) = at(38.0);
        assert!((px - 37.9).abs() < 0.1 && (mm - 2.46).abs() < 0.01, "{px} px, {mm} mm");
        assert!(mm >= READ_MM_MIN && at(41.0).1 < READ_MM_MIN && at(56.0).1 < 1.7);
        assert_eq!(at(80.0).0, 18.0);
    }

    /// A picture through a view: at a scale of 1 and a whole origin its own px; half a px over, the mean of two; past
    /// its edge, the frame's own px as they were.
    #[test]
    fn a_picture_is_its_own_px_at_scale_one_and_between_its_px_otherwise() {
        let pic: Picture = (4, 1, vec![0, 0, 0, 100, 100, 100, 200, 200, 200, 250, 250, 250]);
        let through = |origin: (f32, f32)| {
            let mut out = vec![7u8; 4 * 3];
            resample(
                &mut out,
                4,
                (0, 0),
                (4, 1),
                &pic,
                &View { px: 10.0, cell: (1, 1), scale: (1.0, 1.0), origin },
            );
            out.chunks(3).map(|c| c[0]).collect::<Vec<_>>()
        };
        assert_eq!(through((0.0, 0.0)), [0, 100, 200, 250]);
        assert_eq!(through((0.5, 0.0)), [50, 150, 225, 250]);
        assert_eq!(through((-2.0, 0.0)), [7, 7, 0, 100]);
    }

    /// A read's target goes as the screen stops drawing it: the first screen after the last that draws it — none while
    /// it's drawn to the read's end, the read's own time where it's drawn nowhere. A beat reads one thing: its first
    /// `@zoom` (deep where it says so), a second one in it and one before the first beat said and left out.
    #[test]
    fn a_reads_target_goes_as_the_screen_stops_drawing_it_and_a_beat_reads_one_thing() {
        let asks = || screen(1, &["fn main() {", "╭─ asks ──╮", "│ y move  │", "╰─────────╯"]);
        let plain = || screen(1, &["fn main() {", "    let x = 1;", "}"]);
        let rec = |frames: Vec<(u64, vt100::Screen)>, zooms: &[(u64, &str, bool)]| Recorded {
            frames,
            beats: vec![(0, "one".into()), (300, "two".into()), (450, "three".into())],
            waits: Vec::new(),
            zooms: zooms.iter().map(|(t, z, deep)| (*t, z.to_string(), *deep)).collect(),
            notes: Vec::new(),
            keys: Vec::new(),
            sounds: Vec::new(),
            punch: None,
            missed: Vec::new(),
            wire: Vec::new(),
            end: 0,
        };
        let zooms = [(0, "card:asks", false), (100, "code:1-1", false), (310, "code:1-1", true)];
        let gone = rec(vec![(0, asks()), (100, asks()), (200, plain()), (400, plain())], &zooms);
        assert_eq!(card_gone(&gone, "card:asks", 0, 1_000), Some(200));
        assert_eq!(card_gone(&gone, "card:asks", 0, 150), None, "drawn to the read's end");
        assert_eq!(card_gone(&gone, "code:1-1", 200, 1_000), None, "the code's lines: there to the end");
        assert_eq!(card_gone(&gone, "card:nowhere", 50, 1_000), Some(50));
        let back = rec(vec![(0, asks()), (200, plain()), (400, asks())], &zooms);
        assert_eq!(card_gone(&back, "card:asks", 0, 1_000), None, "back before the read's end");
        let (aims, missed) = reads(&gone, (0.6, 0.6), 0.33, 40.0);
        let read: Vec<_> = aims.iter().map(|a| (a.beat, a.spec.as_str(), a.deep, a.planned.next)).collect();
        assert_eq!(read, [(0, "card:asks", false, Some(300)), (1, "code:1-1", true, Some(450))]);
        assert_eq!(missed, ["@zoom:code:1-1 at 0.1 s: a beat reads one thing (card:asks) — left out"]);
    }
}
