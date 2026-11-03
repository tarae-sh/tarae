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
//! eased to and held, each frame the screen drawn at the glyph size the camera's region fits the window at, only whole
//! cells shown and a word the window's edge would cut hidden (`Video::short_frame`); everything inside `SAFE`, the
//! margins YouTube's own buttons and title cover; the captions in a band above the window, wrapped; and a sound track
//! (`sound::track`: tarae's own strokes for the keys typed, a stamp for a card's answer, the settle where an edit
//! lands — the scene's `@sound:stamp` and `@sound:settle`) as AAC. The wide video ignores `@zoom` and `@sound` and
//! stays silent.

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
/// A Short's title card and end card: it's swiped past in a second, so its title stands shorter.
const SHORT_TITLE_MS: u64 = 1_200;
const SHORT_END_MS: u64 = 2_500;
/// A `@zoom`'s target may be drawn a frame after its key: looked for this long after it.
const FIND_MS: u64 = 2_000;
/// A `@zoom` this soon after a beat is the beat's own (else the beat takes the camera back to the whole screen).
const OWN_MS: u64 = 1_000;
/// A key this soon after a `@sound:stamp` is the stamp's: it sounds as the stamp alone.
const STAMP_KEY_MS: u64 = 150;
/// How wide a phone's screen is, held upright (mm) — what a glyph's size on it is said by.
const PHONE_MM: f32 = 70.0;

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
}

/// A Short's window, where the camera's picture goes (x, y, width, height): inside `SAFE`, under the captions' band.
pub(crate) fn short_window() -> (usize, usize, usize, usize) {
    let (top, right, bottom, left) = SAFE;
    let y = top + SHORT_BAND;
    (left, y, SHORT.0 - left - right, SHORT.1 - bottom - y)
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
            moves(rec, (ww as f32 / cw as f32) / (wh as f32 / ch as f32))
        }
        Form::Wide => (Vec::new(), Vec::new()),
    };
    let zooms = moves
        .iter()
        .map(|(t, spec, region, _)| {
            let (_, _, ww, wh) = short_window();
            let px = camera::fit(*region, (ww, wh), SHORT_PX_MAX, |p| shot::cell(&fonts, p));
            let (cw, ch) = shot::cell(&fonts, px);
            let mm = px / SHORT.0 as f32 * PHONE_MM;
            format!(
                "{:.1} s {spec}: {px} px glyphs, cells {cw}×{ch} px ({:.0} columns across; ~{mm:.1} mm a glyph on a phone)",
                *t as f64 / 1000.0,
                region.w
            )
        })
        .collect();
    let times: Vec<u64> = rec.frames.iter().map(|f| f.0).collect();
    let shown_ms = times.last().copied().unwrap_or(0) + LAST_MS;
    let named = moves.iter().map(|m| m.1.clone()).collect();
    let clips = moves.iter().map(|m| (m.0, m.3)).collect();
    let moves = moves.iter().map(|m| (m.0, m.2)).collect();
    let video = Video { rec, times, px, look, fonts, cards, shown_ms, form, moves, clips, named, screen };
    let frames = video.frames();
    let dir = mp4.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let sheet = video.sheet(&dir.join("sheet.png"))?;
    let (audio, sound) = match form {
        Form::Wide => (None, None),
        Form::Short => {
            let (heard, said) = heard(rec, form.title_ms());
            missed.extend(said);
            let length = Duration::from_millis(frames * 1000 / FPS);
            let (samples, peak, rms) = sound::track(&heard, length);
            let wav = mp4.with_extension("wav");
            sound::wav(&wav, &samples)?;
            (Some(wav), Some((peak, rms)))
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
    Ok(Made { seconds: frames as f64 / FPS as f64, px, frames, sheet, zooms, sound, missed })
}

/// A Short's camera moves from the take (each: when, its target, its region, what of it is drawn): each `@zoom` found
/// on the first screen at or after its time that draws its target (within `FIND_MS`), shaped to the window there
/// (`camera::framed`: `aspect` columns a row), and each beat with no `@zoom` of its own (none within `OWN_MS` after it)
/// back to the whole screen. A target never found is said, the camera staying where it was.
type Move = (u64, String, Region, Region);

fn moves(rec: &Recorded, aspect: f32) -> (Vec<Move>, Vec<String>) {
    let (mut out, mut missed) = (Vec::new(), Vec::new());
    let mut marks: Vec<(u64, &str)> = rec.zooms.iter().map(|(t, z)| (*t, z.as_str())).collect();
    for (t, _) in &rec.beats {
        if !rec.zooms.iter().any(|(z, _)| (*t..=t + OWN_MS).contains(z)) {
            marks.push((*t, "all"));
        }
    }
    marks.sort_by_key(|m| m.0);
    for (t, spec) in marks {
        let from = rec.frames.partition_point(|f| f.0 <= t).saturating_sub(1);
        let found = rec.frames[from..].iter().take_while(|f| f.0 <= t + FIND_MS).find_map(|(at, s)| {
            let cells = camera::Cells::of(s);
            let region = camera::target(&cells, spec)?;
            Some((*at.max(&t), camera::framed(&cells, region, aspect)))
        });
        match found {
            Some((at, (region, clip))) => out.push((at, spec.to_string(), region, clip)),
            None => missed.push(format!(
                "@zoom:{spec} at {:.1} s: not on screen within {} s — the camera stayed",
                t as f64 / 1000.0,
                FIND_MS / 1000
            )),
        }
    }
    (out, missed)
}

/// What a Short hears, at its time in the video (the take's after `title` ms): each key sent (a `@type:` character one
/// by one; a key sent whole — a paste, an escape — one stroke), each `@sound:stamp` (the key after it within
/// `STAMP_KEY_MS` its own) and `@sound:settle`; another `@sound:` said.
fn heard(rec: &Recorded, title: u64) -> (Vec<(Duration, Heard)>, Vec<String>) {
    let at = |t: u64| Duration::from_millis(title + t);
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
            other => said.push(format!("@sound:{other}: no such sound (stamp, settle)")),
        }
    }
    out.sort_by_key(|h| h.0);
    (out, said)
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

/// What one frame of the video shows.
#[derive(Clone, Copy, PartialEq)]
enum Shows {
    Title,
    /// The take's screen, its caption (the beat's), how far that has come in (of 255), the camera's region and what
    /// of it is drawn.
    Screen(usize, Option<usize>, u8, Region, Region),
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
    /// A Short's camera moves (when, where to; what of it is drawn), each one's target as written, and the screen's
    /// size (columns, rows).
    moves: Vec<(u64, Region)>,
    clips: Vec<(u64, Region)>,
    named: Vec<String>,
    screen: (u16, u16),
}

impl Video<'_> {
    fn frames(&self) -> u64 {
        ((self.form.title_ms() + self.shown_ms + self.form.end_ms()) * FPS).div_ceil(1000)
    }

    /// The `k`th frame's ms in the video.
    fn ms(k: u64) -> u64 {
        k * 1000 / FPS
    }

    fn shows(&self, k: u64) -> Shows {
        let title_ms = self.form.title_ms();
        let title = title_ms * FPS / 1000;
        let t = Self::ms(k);
        if t < title_ms {
            return Shows::Title;
        }
        if t >= title_ms + self.shown_ms {
            return Shows::End;
        }
        let local = t - title_ms;
        let beat =
            self.rec.beats.iter().rposition(|b| b.0 <= local).filter(|&i| !self.rec.beats[i].1.is_empty());
        let came = beat.map_or(255, |i| ((local - self.rec.beats[i].0) * 255 / FADE_MS).min(255) as u8);
        let (region, clip) = match self.form {
            Form::Wide => (Region::all(self.screen), Region::all(self.screen)),
            Form::Short => {
                (camera::at(&self.moves, self.screen, local), camera::at(&self.clips, self.screen, local))
            }
        };
        Shows::Screen(screen_at(&self.times, k - title), beat, came, region, clip)
    }

    /// The frame `shows` says, its screen drawn into `drawn` when it isn't the one there and its captions' pictures kept
    /// in `captions`.
    fn frame(&self, shows: Shows, drawn: &mut Drawn, captions: &mut HashMap<usize, Picture>) -> Vec<u8> {
        let (i, beat, came, region, clip) = match shows {
            Shows::Title => return self.cards.0.clone(),
            Shows::End => return self.cards.1.clone(),
            Shows::Screen(i, beat, came, region, clip) => (i, beat, came, region, clip),
        };
        if self.form == Form::Short {
            return self.short_frame(i, (beat, came), (region, clip), drawn, captions);
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

    /// A Short's frame: screen `i` drawn at the glyph size `region` fits the window at, cropped there — only whole cells
    /// inside the clip, a word its left or right edge would cut hidden on its own ground —, the beat's caption (and how
    /// far it has come in) wrapped in the band above.
    fn short_frame(
        &self,
        i: usize,
        (beat, came): (Option<usize>, u8),
        (region, clip): (Region, Region),
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
        let (c0, c1) = (c0.max(clip.x.ceil() as usize), c1.min((clip.x + clip.w).floor() as usize));
        let (r0, r1) = (r0.max(clip.y.ceil() as usize), r1.min((clip.y + clip.h).floor() as usize));
        let (c1, r1) = (c1.max(c0), r1.max(r0));
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
        // (a word cut at the window's left or right edge: its cells laid with their own ground — the colour at each
        // one's top-left)
        let word = |r: usize, c: usize| {
            screen.cell(r as u16, c as u16).is_some_and(|cell| {
                cell.contents().chars().next().is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
            })
        };
        let mut hide = |r: usize, c: usize| {
            let (x, y) = (PAD + c * cw, PAD + r * ch);
            let ground = &pic[(y * pw + x) * 3..][..3];
            let ground = [ground[0], ground[1], ground[2]];
            for yy in y..(y + ch).min(ph) {
                for xx in x..(x + cw).min(pw) {
                    out[to(xx, yy)..][..3].copy_from_slice(&ground);
                }
            }
        };
        for r in r0..r1 {
            if c0 > 0 && c0 < c1 && word(r, c0 - 1) && word(r, c0) {
                (c0..c1).take_while(|&c| word(r, c)).for_each(|c| hide(r, c));
            }
            if c1 < cols && c1 > c0 && word(r, c1) && word(r, c1 - 1) {
                (c0..c1).rev().take_while(|&c| word(r, c)).for_each(|c| hide(r, c));
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
        let title = self.form.title_ms();
        let dims = self.form.dims();
        let times: Vec<u64> = (0..self.frames()).map(Self::ms).collect();
        let mut marks: Vec<(u64, String)> =
            self.rec.beats.iter().filter(|b| !b.1.is_empty()).map(|(t, c)| (title + t, c.clone())).collect();
        for ((t, _), name) in self.moves.iter().zip(&self.named) {
            marks.push((title + t + camera::EASE_MS, format!("zoom:{name}")));
        }
        marks.sort_by_key(|m| m.0);
        // (the end card's first frame: the first whose ms is past the take's)
        marks.push((Self::ms(((title + self.shown_ms) * FPS).div_ceil(1000)), "end card".into()));
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
}
