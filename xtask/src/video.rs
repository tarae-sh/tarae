//! A scene as a short video (`cargo xtask scenes --video`, CLAUDE.md's "Short videos"; collab's `.sb/videos-plan.md`):
//! the take `scenes --record` makes, drawn by the shots' renderer at a steady `FPS` — each frame the screen in force at
//! its time (`screen_at`), real time throughout, the scene's own pauses its holds — on a `W`×`H` picture: the screen at
//! the largest glyph size its cells fit (`fit`), the scene's `@beat:` captions in a band of their own under it (never
//! over tarae's cells), a title card before and an end card after, the seal stamped on both (`docs/brand/`, a PNG made
//! once from its SVG). ffmpeg makes the MP4 from the raw frames piped in (H.264, 4:2:0, BT.709); where there's none,
//! said — the sheet of the video's own frames is written first all the same.
//!
//! `--shorts` makes it a YouTube Short (`Form::Short`): `SHORT` upright, the screen `SHORT_COLS` wide and as tall as
//! the window holds (`short_size`: 80×43), and over it a camera (`camera.rs`) — the scene's `@zoom:<target>` lines
//! eased to and held, the window the safe area's shape with the target at the eye's height (`short_eye`), each frame
//! the screen drawn at the glyph size the camera's region fits the window at, only whole cells shown, every card the
//! window cuts left out, a row its edge cuts drawn cut there, fading (`Video::short_frame`); everything inside `SAFE`, the
//! margins YouTube's own buttons and title cover; the captions in a band above the window, wrapped; a `@punch` opening
//! it on `PUNCH_MS` of the take from there, then the title card and the take from its start; and a sound track
//! (`sound::track`: tarae's own strokes for the keys typed, a stamp for a card's answer, the settle where an edit
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
use crate::recording::{self, Picture};
use crate::scenes::docshots::{Bg, none_of_yours};
use crate::shot::{self, Fonts, PAD, Recorded, Rgb};
use crate::sound::{self, Heard};
use crate::{Result, root};

/// The picture: 1080p, 16:9 — what YouTube and a phone play full.
pub(crate) const W: usize = 1920;
pub(crate) const H: usize = 1080;
pub(crate) const FPS: u64 = 30;
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
/// A Short's captions: their glyphs (read on a phone held upright: ~3 mm) and their band at the safe area's top —
/// two lines and air.
const SHORT_CAPTION_PX: f32 = 50.0;
const SHORT_BAND: usize = 190;
/// A Short's screen: a laptop's narrow terminal's columns; its rows what the window holds at their width.
pub(crate) const SHORT_COLS: u16 = 80;
/// The largest glyph a Short's camera draws (a narrow card's: ~36 columns across).
const SHORT_PX_MAX: u32 = 40;
/// A row cut at the window's edge fades over its last cells there, to this much of its ground at the edge: the line
/// reads as going on, never as broken off.
const FADE_CELLS: usize = 2;
const FADE_MAX: u8 = 200;
/// A spotlight at full (a target floating over the code: a list, a card, a prompt): what's round it this much of the
/// way to the ground (of 255) — the code still read as code, faint, the card the one thing lit.
const SPOT_DIM: u8 = 179;
/// A Short's title card and end card: it's swiped past in a second, so its title stands shorter.
const SHORT_TITLE_MS: u64 = 1_200;
const SHORT_END_MS: u64 = 2_500;
/// A `@zoom`'s target may be drawn a frame after its key: looked for this long after it.
const FIND_MS: u64 = 2_000;
/// A `@zoom` this soon after a beat is the beat's own (else the beat takes the camera back to the whole screen).
const OWN_MS: u64 = 1_000;
/// A key this soon after a `@sound:stamp` is the stamp's: it sounds as the stamp alone.
const STAMP_KEY_MS: u64 = 150;
/// How wide a phone's screen is, held upright (mm) — what a glyph's size on it is said by — and the glyph a Short
/// aims at for what its caption is about (a camera never takes more in past it: `camera::framed`'s `legible`).
const PHONE_MM: f32 = 70.0;
const LEGIBLE_MM: f32 = 2.5;
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

    /// The whole screen after the title card, before the take: a Short's (its camera reads the parts after it).
    fn whole_ms(self) -> u64 {
        if self == Form::Short { camera::WHOLE_MS } else { 0 }
    }
}

/// A Short's window, where the camera's picture goes (x, y, width, height): inside `SAFE`, under the captions' band.
pub(crate) fn short_window() -> (usize, usize, usize, usize) {
    let (top, right, bottom, left) = SAFE;
    let y = top + SHORT_BAND;
    (left, y, SHORT.0 - left - right, SHORT.1 - bottom - y)
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
/// moves said (each: when, its target, its glyph and cell size) and its sound's peak and RMS (dBFS); what went wrong
/// on the way that left the video made all the same (a target never found).
pub(crate) struct Made {
    pub(crate) seconds: f64,
    pub(crate) px: f32,
    pub(crate) frames: u64,
    pub(crate) sheet: usize,
    pub(crate) zooms: Vec<String>,
    pub(crate) sound: Option<(f32, f32)>,
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
        Form::Short => wrap(title, short_window().2 / shot::cell(&fonts, LINE_PX).0),
    };
    let name = ("t a r a e", NAME_PX, look.accent, true);
    let mut first_lines = vec![name];
    first_lines.extend(titled.iter().map(|t| (t.as_str(), LINE_PX, look.ink, false)));
    let cards = (
        card(&first_lines, &seal, &look, &fonts, dims),
        card(&[name, ("tarae.sh", LINE_PX, look.ink, false)], &seal, &look, &fonts, dims),
    );
    let screen = (cols, rows);
    let (moves, mut missed) = match form {
        Form::Short => {
            // (the window's columns a row, at a mid zoom's cells)
            let (_, _, ww, wh) = short_window();
            let (cw, ch) = shot::cell(&fonts, 30.0);
            let legible = (LEGIBLE_MM / PHONE_MM * SHORT.0 as f32).ceil();
            let legible = (ww / shot::cell(&fonts, legible).0) as f32;
            moves(rec, (ww as f32 / cw as f32) / (wh as f32 / ch as f32), short_eye(), legible)
        }
        Form::Wide => (Vec::new(), Vec::new()),
    };
    let times: Vec<u64> = rec.frames.iter().map(|f| f.0).collect();
    let last_ms = times.last().copied().unwrap_or(0) + LAST_MS;
    // (a Short's whole screen laid in: the camera's course, each step a move or the whole)
    let all = Region::all(screen);
    let planned: Vec<camera::Planned> =
        (moves.iter()).map(|m| camera::Planned { at: m.0, window: m.2, target: m.3, beat: m.5 }).collect();
    // (where each screen is — its path bar's file and workspace —, as it changes)
    let mut places: Vec<(u64, String)> = Vec::new();
    for (t, s) in rec.frames.iter().filter(|_| form == Form::Short) {
        let here = camera::place(&camera::Cells::of(s));
        if places.last().is_none_or(|p| p.1 != here) {
            places.push((*t, here));
        }
    }
    let course = camera::wholes(&planned, &places, last_ms, screen);
    let place_at =
        |t: u64| places[..places.partition_point(|p| p.0 <= t)].last().map_or("", |p| p.1.as_str());
    let s = |t: u64| t as f64 / 1000.0;
    let zooms = match form {
        Form::Wide => Vec::new(),
        Form::Short => std::iter::once(format!("the whole: {:.1} s before the take", s(camera::WHOLE_MS)))
            .chain(course.steps.iter().map(|&(t, which)| {
                let Some(i) = which else {
                    return match t == course.out {
                        true => format!("{:.1} s the whole, {:.1} s before the end card", s(t), s(camera::WHOLE_MS)),
                        false => format!("{:.1} s the whole, between beats", s(t)),
                    };
                };
                let (_, spec, region, ..) = &moves[i];
                let (_, _, ww, wh) = short_window();
                let px = camera::fit(*region, (ww, wh), SHORT_PX_MAX, |p| shot::cell(&fonts, p));
                let (cw, ch) = shot::cell(&fonts, px);
                let mm = px / SHORT.0 as f32 * PHONE_MM;
                let way = match course.ways.iter().find(|w| w.0 == i).map(|w| w.1) {
                    Some(camera::Way::Pulled(0)) => " — in from the whole".to_string(),
                    Some(camera::Way::Pulled(late)) => format!(" — in from the whole, {:.1} s past its time", s(late)),
                    Some(camera::Way::Elsewhere(late)) => format!(
                        " — in from the whole, another place ({}), {:.1} s past its time",
                        place_at(moves[i].0),
                        s(late)
                    ),
                    Some(camera::Way::Pan) => " — a pan: side by side with the last target".to_string(),
                    Some(camera::Way::NoRoom(held)) => {
                        format!(" — a pan: no room for the whole (its target would stand {:.1} s)", s(held))
                    }
                    None => String::new(),
                };
                format!(
                    "{:.1} s {spec}: {px} px glyphs, cells {cw}×{ch} px ({:.0} columns across; ~{mm:.1} mm a glyph on a \
                     phone){way}",
                    s(t),
                    region.w
                )
            }))
            .collect(),
    };
    let moves: Vec<Move> = match form {
        Form::Wide => moves,
        Form::Short => (course.steps.iter())
            .map(|&(t, which)| match which {
                Some(i) => {
                    let (_, spec, window, target, spot, beat) = moves[i].clone();
                    (t, spec, window, target, spot, beat)
                }
                None => (t, "all".to_string(), all, all, (false, None), None),
            })
            .collect(),
    };
    let shown_ms = match form {
        Form::Wide => last_ms,
        Form::Short => course.out + camera::WHOLE_MS,
    };
    let named = moves.iter().map(|m| m.1.clone()).collect();
    let targets = moves.iter().map(|m| (m.0, m.3)).collect();
    let spots = moves.iter().map(|m| m.4).collect();
    let moves = moves.iter().map(|m| (m.0, m.2)).collect();
    // (a Short opens on its punchline where the scene marks one)
    let punch = rec.punch.filter(|_| form == Form::Short);
    let lead = punch.map_or(0, |_| PUNCH_MS) + form.title_ms() + form.whole_ms();
    let video = Video {
        rec,
        times,
        px,
        look,
        fonts,
        cards,
        shown_ms,
        form,
        moves,
        targets,
        named,
        spots,
        screen,
        punch,
        lead,
    };
    let frames = video.frames();
    let dir = mp4.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let sheet = video.sheet(&dir.join("sheet.png"))?;
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
            let (samples, peak, rms) = sound::track(&heard, length);
            let wav = mp4.with_extension("wav");
            sound::wav(&wav, &samples)?;
            (Some(wav), Some((peak, rms)), Some(counted))
        }
    };
    let ffmpeg = ffmpeg().ok_or_else(|| {
        format!(
            "no ffmpeg on PATH nor at {}: the MP4 is made by it (Homebrew's `ffmpeg`) — the sheet of its frames is \
             written, {}",
            FFMPEG_AT.join(" or "),
            dir.join("sheet.png").display()
        )
    })?;
    video.encode(&ffmpeg, mp4, audio.as_deref())?;
    Ok(Made { seconds: frames as f64 / FPS as f64, px, frames, sheet, zooms, sound, heard: counted, missed })
}

/// A Short's camera moves from the take (each: when, its target as written, the window's region, the target's, and the
/// spotlight's: whether the target floats over the code — `camera::spotlit` — and when its card goes while the camera
/// holds there, `card_gone`): each
/// `@zoom` found on the first screen at or after its time that draws its target (within `FIND_MS`), shaped to the
/// window there (`camera::framed`: `aspect` columns a row, its centre `eye` down, `legible` columns at most taken in) —
/// as wide as it grows on the same rows while the camera holds there (`camera::widened`: a line Claude is writing reaches
/// its end, never past the window's edge) —, and each beat with no `@zoom` of its own (none within `OWN_MS` after it)
/// back to the whole screen; each move a beat's first (its `@zoom`, or its whole) carries the beat's time (`wholes`
/// pulls out to the whole there). A target never found is said, the camera staying where it was.
type Move = (u64, String, Region, Region, (bool, Option<u64>), Option<u64>);

fn moves(rec: &Recorded, aspect: f32, eye: f32, legible: f32) -> (Vec<Move>, Vec<String>) {
    let (mut out, mut missed) = (Vec::new(), Vec::new());
    let mut marks: Vec<(u64, &str, Option<u64>)> =
        rec.zooms.iter().map(|(t, z)| (*t, z.as_str(), None)).collect();
    for (t, _) in &rec.beats {
        // (the beat's own: the first `@zoom` within `OWN_MS` of it)
        match marks.iter_mut().filter(|m| (*t..=t + OWN_MS).contains(&m.0)).min_by_key(|m| m.0) {
            Some(own) => own.2 = Some(*t),
            None => marks.push((*t, "all", Some(*t))),
        }
    }
    marks.sort_by_key(|m| m.0);
    for (i, &(t, spec, beat)) in marks.iter().enumerate() {
        let until = marks.get(i + 1).map_or(u64::MAX, |m| m.0);
        let from = rec.frames.partition_point(|f| f.0 <= t).saturating_sub(1);
        let found = rec.frames[from..].iter().take_while(|f| f.0 <= t + FIND_MS).find_map(|(at, s)| {
            let cells = camera::Cells::of(s);
            let region = camera::target(&cells, spec)?;
            // (the target on each screen of the hold, as it grows)
            let held = (rec.frames.iter())
                .skip_while(|f| f.0 <= *at)
                .take_while(|f| f.0 < until)
                .filter_map(|f| camera::target(&camera::Cells::of(&f.1), spec));
            let region = camera::widened(region, held, (cells.text[0].len() as u16, cells.text.len() as u16));
            let window = match camera::news(spec) {
                Some((text, _)) => camera::news_framed(&cells, text, region, aspect, eye, legible),
                None => camera::framed(&cells, region, aspect, eye, legible),
            };
            Some((*at.max(&t), window, region, !camera::spotlit(&cells, spec).is_empty()))
        });
        match found {
            Some((at, window, region, spot)) => {
                let gone = spot.then(|| card_gone(rec, spec, at, until)).flatten();
                out.push((at, spec.to_string(), window, region, (spot, gone), beat))
            }
            None => missed.push(format!(
                "@zoom:{spec} at {:.1} s: not on screen within {} s — the camera stayed",
                t as f64 / 1000.0,
                FIND_MS / 1000
            )),
        }
    }
    (out, missed)
}

/// When a floating target's card leaves the screen while the camera holds on it (from `at` until `until`): the first
/// screen after the last one that draws it (`camera::spotlit`) — none while one to the hold's end does; `at` where none
/// does.
fn card_gone(rec: &Recorded, spec: &str, at: u64, until: u64) -> Option<u64> {
    let from = rec.frames.partition_point(|f| f.0 <= at).saturating_sub(1);
    let end = rec.frames.partition_point(|f| f.0 < until).max(from);
    let hold = &rec.frames[from..end];
    match hold.iter().rposition(|f| !camera::spotlit(&camera::Cells::of(&f.1), spec).is_empty()) {
        Some(last) => hold.get(last + 1).map(|f| f.0),
        None => Some(at),
    }
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

/// The seal for `bg`'s theme, RGBA (`docs/brand/seal-<theme>-96.png`, made once from its SVG).
fn seal(bg: Bg) -> Result<(usize, usize, Vec<u8>)> {
    let path = root().join(format!("docs/brand/seal-{}-96.png", bg.theme()));
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
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

/// Which part of the video its `t` ms is in: a Short's punchline (the take's ms from its mark `punch`, `PUNCH_MS`
/// long), the title card, a Short's whole screen (the take's first, its last `whole` ms before `lead`), the take (its
/// ms, `shown` long), then the end card.
#[derive(Debug, PartialEq)]
enum Part {
    Punch(u64),
    Title,
    Whole,
    Take(u64),
    End,
}

fn part(t: u64, punch: Option<u64>, whole: u64, lead: u64, shown: u64) -> Part {
    match punch {
        Some(from) if t < PUNCH_MS => Part::Punch(from + t),
        _ if t + whole < lead => Part::Title,
        _ if t < lead => Part::Whole,
        _ if t < lead + shown => Part::Take(t - lead),
        _ => Part::End,
    }
}

/// What one frame of the video shows.
#[derive(Clone, Copy, PartialEq)]
enum Shows {
    Title,
    /// The take's screen, its caption (the beat's), how far that has come in (of 255), the camera's region, its
    /// target's (a card meeting it never left out), the move whose target is a news beat's (its toast laid over) and
    /// the spotlight: the move whose card it lights and how lit it is (of 255).
    Screen(usize, Option<usize>, u8, Region, Region, Option<usize>, Option<(usize, u8)>),
    End,
}

/// A drawn screen kept for the next frame: (its index, its glyph size) and its picture.
type Drawn = Option<((usize, u32), Picture)>;

struct Video<'a> {
    rec: &'a Recorded,
    times: Vec<u64>,
    px: f32,
    look: Look,
    fonts: Fonts,
    /// The title card's frame and the end card's.
    cards: (Vec<u8>, Vec<u8>),
    /// How long the take is shown: to its last screen and `LAST_MS` of it.
    shown_ms: u64,
    form: Form,
    /// A Short's camera moves (when, where to; each one's target's region), each one's target as written, and the
    /// screen's size (columns, rows).
    moves: Vec<(u64, Region)>,
    targets: Vec<(u64, Region)>,
    named: Vec<String>,
    /// Each move's spotlight: whether its target floats over the code (`camera::spotlit`), and when its card went.
    spots: Vec<(bool, Option<u64>)>,
    screen: (u16, u16),
    /// A Short's punchline: the take's ms it opens on (`PUNCH_MS` of it, before the title card).
    punch: Option<u64>,
    /// The video's ms before the take starts: the punchline's and the title card's.
    lead: u64,
}

impl Video<'_> {
    fn frames(&self) -> u64 {
        ((self.lead + self.shown_ms + self.form.end_ms()) * FPS).div_ceil(1000)
    }

    /// The take as it stands at its `ms`: the screen in force, the beat (and how far its caption has come in), the
    /// camera's region and its target's, and the spotlight (`camera::lit`: on the last move whose target floats, while
    /// it's lit at all).
    fn take_at(&self, ms: u64, screen: usize) -> Shows {
        let beat =
            self.rec.beats.iter().rposition(|b| b.0 <= ms).filter(|&i| !self.rec.beats[i].1.is_empty());
        let came = beat.map_or(255, |i| ((ms - self.rec.beats[i].0) * 255 / FADE_MS).min(255) as u8);
        let (region, target, news, spot) = match self.form {
            Form::Wide => (Region::all(self.screen), Region::all(self.screen), None, None),
            Form::Short => {
                let i = self.targets.partition_point(|t| t.0 <= ms).checked_sub(1);
                let target = i.map_or(Region::all(self.screen), |i| self.targets[i].1);
                let news = i.filter(|&i| camera::news(&self.named[i]).is_some());
                let changes = (self.moves.iter().zip(&self.spots))
                    .flat_map(|(m, &(on, gone))| [Some((m.0, on)), gone.map(|g| (g, false))])
                    .flatten();
                let level = (camera::lit(changes, ms) * 255.0).round() as u8;
                let spot = i
                    .and_then(|i| self.spots[..=i].iter().rposition(|s| s.0))
                    .filter(|_| level > 0)
                    .map(|m| (m, level));
                (camera::at(&self.moves, self.screen, ms), target, news, spot)
            }
        };
        Shows::Screen(screen, beat, came, region, target, news, spot)
    }

    /// The `k`th frame's ms in the video.
    fn ms(k: u64) -> u64 {
        k * 1000 / FPS
    }

    fn shows(&self, k: u64) -> Shows {
        match part(Self::ms(k), self.punch, self.form.whole_ms(), self.lead, self.shown_ms) {
            // (the punchline: the take from its mark, its caption standing as it was there)
            Part::Punch(at) => {
                let Shows::Screen(i, beat, _, region, target, news, spot) =
                    self.take_at(at, self.times.partition_point(|&s| s <= at).saturating_sub(1))
                else {
                    unreachable!("take_at is a screen")
                };
                Shows::Screen(i, beat, 255, region, target, news, spot)
            }
            Part::Title => Shows::Title,
            // (the take's first screen, whole, no caption yet: the camera eases in from it as the take starts)
            Part::Whole => {
                let all = Region::all(self.screen);
                Shows::Screen(0, None, 255, all, all, None, None)
            }
            Part::Take(at) => self.take_at(at, screen_at(&self.times, k - self.lead * FPS / 1000)),
            Part::End => Shows::End,
        }
    }

    /// The frame `shows` says, its screen drawn into `drawn` when it isn't the one there and its captions' pictures kept
    /// in `captions`.
    fn frame(&self, shows: Shows, drawn: &mut Drawn, captions: &mut HashMap<usize, Picture>) -> Vec<u8> {
        let (i, beat, came, region, target, news, spot) = match shows {
            Shows::Title => return self.cards.0.clone(),
            Shows::End => return self.cards.1.clone(),
            Shows::Screen(i, beat, came, region, target, news, spot) => {
                (i, beat, came, region, target, news, spot)
            }
        };
        if self.form == Form::Short {
            let news = news.and_then(|m| camera::news(&self.named[m])).map(|(text, _)| text);
            return self.short_frame(i, (beat, came), (region, target, news, spot), drawn, captions);
        }
        let key = (i, self.px as u32);
        if drawn.as_ref().is_none_or(|d| d.0 != key) {
            *drawn = Some((key, shot::render_px(&self.rec.frames[i].1, &self.fonts, self.px)));
        }
        let Some((_, screen)) = drawn.as_ref() else { unreachable!("drawn above") };
        let mut px = self.look.ground.repeat(W * H);
        let (x0, y0) = ((W - screen.0) / 2, (H - BAND - screen.1) / 2);
        blit(&mut px, (W, H), screen, x0, y0, 255);
        // (the band's hairline, the screen's cells wide)
        let at = (H - BAND) * W * 3;
        for x in x0 + PAD..x0 + screen.0 - PAD {
            px[at + x * 3..][..3].copy_from_slice(&self.look.faint);
        }
        if let Some(b) = beat {
            let caption = captions.entry(b).or_insert_with(|| {
                text(&self.rec.beats[b].1, CAPTION_PX, self.look.ink, self.look.ground, false, &self.fonts)
            });
            blit(
                &mut px,
                (W, H),
                caption,
                (W - caption.0.min(W)) / 2,
                H - BAND + (BAND - caption.1) / 2,
                came,
            );
        }
        px
    }

    /// A Short's frame: screen `i` drawn at the glyph size `region` fits the window at, cropped there — only whole
    /// cells, every card the window cuts (but the one `target` is in) left out on the window's ground, and a row whose
    /// text its left or right edge cuts drawn cut there, fading into its ground, a sliver the left edge leaves erased
    /// (`camera::Window`) —, every cell round the card a floating target is in dimmed toward the ground by the spotlight
    /// (`spot`: the move whose card it is and how lit, of 255; `SPOT_DIM` at full — every cell, as it eases out, where
    /// this screen no longer draws the card), a news beat's toast (`news`: its text) laid over the window's top as it stands on the
    /// screen, bright, each of its cells copied from the screen's picture (`camera::callout`), the beat's caption (and how
    /// far it has come in) wrapped in the band above.
    fn short_frame(
        &self,
        i: usize,
        (beat, came): (Option<usize>, u8),
        (region, target, news, spot): (Region, Region, Option<&str>, Option<(usize, u8)>),
        drawn: &mut Drawn,
        captions: &mut HashMap<usize, Picture>,
    ) -> Vec<u8> {
        let (wx, wy, ww, wh) = short_window();
        let px = camera::fit(region, (ww, wh), SHORT_PX_MAX, |p| shot::cell(&self.fonts, p));
        let key = (i, px as u32);
        if drawn.as_ref().is_none_or(|d| d.0 != key) {
            *drawn = Some((key, shot::render_px(&self.rec.frames[i].1, &self.fonts, px)));
        }
        let Some((_, (pw, ph, pic))) = drawn.as_ref() else { unreachable!("drawn above") };
        let (pw, ph) = (*pw, *ph);
        let screen = &self.rec.frames[i].1;
        let (rows, cols) = screen.size();
        let (rows, cols) = (usize::from(rows), usize::from(cols));
        let (cw, ch) = shot::cell(&self.fonts, px);
        let (ox, oy) = camera::crop(region, (cw, ch), PAD, (pw, ph), (ww, wh));
        // (the whole cells inside the window: columns c0..c1, rows r0..r1)
        let whole = |o: i64, view: usize, cell: usize, n: usize| {
            let first = ((o - PAD as i64).max(0) as usize).div_ceil(cell).min(n);
            let last = (((o + view as i64 - PAD as i64).max(0) as usize) / cell).min(n);
            (first, last.max(first))
        };
        let (c0, c1) = whole(ox, ww, cw, cols);
        let (r0, r1) = whole(oy, wh, ch, rows);
        let mut out = self.look.ground.repeat(SHORT.0 * SHORT.1);
        let to = |x: usize, y: usize| {
            let (dx, dy) = (wx as i64 + x as i64 - ox, wy as i64 + y as i64 - oy);
            (dy as usize * SHORT.0 + dx as usize) * 3
        };
        let (x_from, x_to) = (PAD + c0 * cw, (PAD + c1 * cw).min(pw));
        for y in (PAD + r0 * ch)..(PAD + r1 * ch).min(ph) {
            let at = to(x_from, y);
            out[at..at + (x_to - x_from) * 3]
                .copy_from_slice(&pic[(y * pw + x_from) * 3..(y * pw + x_to) * 3]);
        }
        // (the cards the window cuts: left out, the window's ground in their cells; every other cell drawn)
        let cells = camera::Cells::of(screen);
        let callout = news.and_then(|text| camera::callout(&cells, text, (c1 - c0).saturating_sub(2)));
        let window = camera::Window::of(&cells, (c0, r0, c1, r1), target, callout.as_ref().map(|c| c.card));
        let fill = |out: &mut Vec<u8>, r: usize, c: usize, ground: Rgb| {
            for y in (PAD + r * ch)..(PAD + (r + 1) * ch).min(ph) {
                for x in (PAD + c * cw)..(PAD + (c + 1) * cw).min(pw) {
                    out[to(x, y)..][..3].copy_from_slice(&ground);
                }
            }
        };
        for r in r0..r1 {
            for c in (c0..c1).filter(|&c| !window.drawn(r, c)) {
                fill(&mut out, r, c, self.look.ground);
            }
        }
        // (a sliver: each cell its own ground — its top-left pixel's, where no glyph reaches)
        for &(r, from, end) in &window.slivers {
            for c in from..end {
                let at = to(PAD + c * cw, PAD + r * ch);
                let ground: Rgb = [out[at], out[at + 1], out[at + 2]];
                fill(&mut out, r, c, ground);
            }
        }
        // (a row whose text runs past the window's edge: cut there, its last `FADE_CELLS` easing into each cell's own
        // ground — its top-left pixel's —, to `FADE_MAX` at the edge)
        let span = (FADE_CELLS * cw) as f32;
        for &(r, edge) in &window.fade {
            let (y0, y1) = (PAD + r * ch, (PAD + (r + 1) * ch).min(ph));
            for k in 0..FADE_CELLS.min(c1 - c0) {
                let c = match edge {
                    camera::Edge::Left => c0 + k,
                    camera::Edge::Right => c1 - 1 - k,
                };
                let x0 = PAD + c * cw;
                let at = to(x0, y0);
                let ground: Rgb = [out[at], out[at + 1], out[at + 2]];
                for x in x0..(x0 + cw).min(pw) {
                    // (how far in from the edge, in px)
                    let d = match edge {
                        camera::Edge::Left => x - PAD - c0 * cw,
                        camera::Edge::Right => PAD + c1 * cw - 1 - x,
                    };
                    let a = (f32::from(FADE_MAX) * (1.0 - d as f32 / span)).max(0.0) as u8;
                    for y in y0..y1 {
                        mix(&mut out[to(x, y)..][..3], ground, a);
                    }
                }
            }
        }
        // (the spotlight: every cell round the card dimmed toward the window's ground)
        if let Some((m, level)) = spot {
            let lit = camera::spotlit(&cells, &self.named[m]);
            let a = (u32::from(SPOT_DIM) * u32::from(level) / 255) as u8;
            for r in r0..r1 {
                for c in
                    (c0..c1).filter(|&c| !lit.iter().any(|b| b.0 <= r && r <= b.2 && b.1 <= c && c <= b.3))
                {
                    for y in (PAD + r * ch)..(PAD + (r + 1) * ch).min(ph) {
                        for x in (PAD + c * cw)..(PAD + (c + 1) * cw).min(pw) {
                            mix(&mut out[to(x, y)..][..3], self.look.ground, a);
                        }
                    }
                }
            }
        }
        // (the news laid over the window's top: each cell's pixels the screen's own, where it stands on the screen)
        if let Some(news) = &callout {
            let w = news.lines.first().map_or(0, Vec::len);
            let (top, left) = camera::callout_at((c0, r0, c1, r1), w);
            for (k, line) in news.lines.iter().enumerate() {
                for (j, cell) in line.iter().enumerate() {
                    let (r, c) = (top + k, left + j);
                    if r >= r1 || c >= c1 {
                        continue;
                    }
                    let (sr, sc) = cell.unwrap_or(news.ground);
                    for dy in 0..ch.min(ph.saturating_sub(PAD + sr * ch)) {
                        let (from, y) = ((PAD + sr * ch + dy) * pw + PAD + sc * cw, PAD + r * ch + dy);
                        let at = to(PAD + c * cw, y);
                        let n = cw.min(pw.saturating_sub(PAD + sc * cw));
                        out[at..at + n * 3].copy_from_slice(&pic[from * 3..(from + n) * 3]);
                    }
                }
            }
        }
        if let Some(b) = beat {
            let caption = captions.entry(b).or_insert_with(|| self.short_caption(&self.rec.beats[b].1));
            let (top, right, _, left) = SAFE;
            let safe_w = SHORT.0 - left - right;
            let x = left + safe_w.saturating_sub(caption.0) / 2;
            blit(&mut out, SHORT, caption, x, top + SHORT_BAND.saturating_sub(caption.1) / 2, came);
        }
        out
    }

    /// A Short's caption: wrapped to the safe area's width, its lines centred one under another on the ground.
    fn short_caption(&self, said: &str) -> Picture {
        let (_, right, _, left) = SAFE;
        let w = SHORT.0 - left - right;
        let lines = wrap(said, w / shot::cell(&self.fonts, SHORT_CAPTION_PX).0);
        let pictures: Vec<Picture> = (lines.iter())
            .map(|l| text(l, SHORT_CAPTION_PX, self.look.ink, self.look.ground, false, &self.fonts))
            .collect();
        let h = pictures.iter().map(|p| p.1).sum();
        let mut px = self.look.ground.repeat(w * h);
        let mut y = 0;
        for p in &pictures {
            blit(&mut px, (w, h), p, w.saturating_sub(p.0) / 2, y, 255);
            y += p.1;
        }
        (w, h, px)
    }

    /// Every frame through ffmpeg into `mp4`: raw RGB on its stdin, H.264 `slow` CRF 20 tuned for flat pictures, 4:2:0
    /// in BT.709 (what a phone's player assumes for HD), the index at the front; `audio` (a WAV) as AAC beside it.
    fn encode(&self, ffmpeg: &Path, mp4: &Path, audio: Option<&Path>) -> Result<()> {
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
        let (mut drawn, mut captions) = (None, HashMap::new());
        let mut last: Option<(Shows, Vec<u8>)> = None;
        let mut wrote = Ok(());
        for k in 0..self.frames() {
            let shows = self.shows(k);
            if last.as_ref().is_none_or(|l| l.0 != shows) {
                last = Some((shows, self.frame(shows, &mut drawn, &mut captions)));
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
            (true, Ok(())) => Ok(()),
            (_, Err(e)) => Err(format!("ffmpeg stopped taking frames ({e}): {said}")),
            (false, _) => Err(format!("ffmpeg: {} — {said}", out.status)),
        }
    }

    /// The sheet of the video's own frames at `out` (`recording::sheet_picks`: one every 2 s and at each beat — a
    /// Short's at each camera move's end too —, the cards' first frames too), each at half size with its time in the
    /// video and its beat under it.
    fn sheet(&self, out: &Path) -> Result<usize> {
        let lead = self.lead;
        let dims = self.form.dims();
        let times: Vec<u64> = (0..self.frames()).map(Self::ms).collect();
        let mut marks: Vec<(u64, String)> =
            self.rec.beats.iter().filter(|b| !b.1.is_empty()).map(|(t, c)| (lead + t, c.clone())).collect();
        for ((t, _), name) in self.moves.iter().zip(&self.named) {
            let said = if name == "all" { "the whole".to_string() } else { format!("zoom:{name}") };
            marks.push((lead + t + camera::EASE_MS, said));
        }
        if self.form.whole_ms() > 0 {
            marks.push((lead - self.form.whole_ms() / 2, "the whole".into()));
        }
        if self.punch.is_some() {
            marks.push((PUNCH_MS / 2, "punchline".into()));
        }
        marks.sort_by_key(|m| m.0);
        // (the end card's first frame: the first whose ms is past the take's)
        marks.push((Self::ms(((lead + self.shown_ms) * FPS).div_ceil(1000)), "end card".into()));
        let picks = recording::sheet_picks(&times, &marks, recording::SHEET_EVERY_MS);
        let (mut drawn, mut captions) = (None, HashMap::new());
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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A Short with a punchline opens on the take from its mark for `PUNCH_MS`, then the title card, then the whole
    /// screen, then the take from its start, then the end card; without one, the title card first; the wide video has
    /// no whole of its own.
    #[test]
    fn a_punchline_opens_the_video_then_the_whole_then_the_take_from_its_start() {
        let (title, whole, shown) = (1_200, camera::WHOLE_MS, 10_000);
        let lead = PUNCH_MS + title + whole;
        assert_eq!(part(0, Some(7_000), whole, lead, shown), Part::Punch(7_000));
        assert_eq!(part(PUNCH_MS - 1, Some(7_000), whole, lead, shown), Part::Punch(7_000 + PUNCH_MS - 1));
        assert_eq!(part(PUNCH_MS, Some(7_000), whole, lead, shown), Part::Title);
        assert_eq!(part(PUNCH_MS + title - 1, Some(7_000), whole, lead, shown), Part::Title);
        assert_eq!(part(PUNCH_MS + title, Some(7_000), whole, lead, shown), Part::Whole);
        assert_eq!(part(lead - 1, Some(7_000), whole, lead, shown), Part::Whole);
        assert_eq!(part(lead, Some(7_000), whole, lead, shown), Part::Take(0));
        assert_eq!(part(lead + shown, Some(7_000), whole, lead, shown), Part::End);
        assert_eq!(part(0, None, whole, title + whole, shown), Part::Title);
        assert_eq!(part(title, None, whole, title + whole, shown), Part::Whole);
        assert_eq!(part(title + whole + 5, None, whole, title + whole, shown), Part::Take(5));
        assert_eq!(part(title + 5, None, 0, title, shown), Part::Take(5));
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
            keys: Vec::new(),
            sounds,
            punch: None,
            missed: Vec::new(),
            wire: Vec::new(),
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

    /// A spotlight goes as its card does: a move to a floating card is lit, one to the code isn't; the card's going while
    /// the camera holds there is the first screen after the last that draws it — none while it's drawn to the hold's end,
    /// the move's own time where it's drawn nowhere.
    #[test]
    fn a_spotlight_goes_as_its_card_does() {
        let asks = || screen(1, &["fn main() {", "╭─ asks ──╮", "│ y move  │", "╰─────────╯"]);
        let plain = || screen(1, &["fn main() {", "    let x = 1;", "}"]);
        let rec = |frames: Vec<(u64, vt100::Screen)>, zooms: &[(u64, &str)]| Recorded {
            frames,
            beats: Vec::new(),
            waits: Vec::new(),
            zooms: zooms.iter().map(|(t, z)| (*t, z.to_string())).collect(),
            keys: Vec::new(),
            sounds: Vec::new(),
            punch: None,
            missed: Vec::new(),
            wire: Vec::new(),
        };
        let zooms = [(0, "card:asks"), (500, "code:1-1")];
        let gone = rec(vec![(0, asks()), (100, asks()), (200, plain()), (400, plain())], &zooms);
        assert_eq!(card_gone(&gone, "card:asks", 0, 1_000), Some(200));
        assert_eq!(card_gone(&gone, "card:asks", 0, 150), None, "drawn to the hold's end");
        assert_eq!(card_gone(&gone, "card:nowhere", 50, 1_000), Some(50));
        let back = rec(vec![(0, asks()), (200, plain()), (400, asks())], &zooms);
        assert_eq!(card_gone(&back, "card:asks", 0, 1_000), None, "back before the hold's end");
        let (moved, missed) = moves(&gone, 0.6, 0.33, 40.0);
        assert!(missed.is_empty(), "{missed:?}");
        let spots: Vec<_> = moved.iter().map(|m| m.4).collect();
        assert_eq!(spots, [(true, Some(200)), (false, None)]);
    }
}
