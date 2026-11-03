//! A scene as a short video (`cargo xtask scenes --video`, CLAUDE.md's "Short videos"; collab's `.sb/videos-plan.md`):
//! the take `scenes --record` makes, drawn by the shots' renderer at a steady `FPS` — each frame the screen in force at
//! its time (`screen_at`), real time throughout, the scene's own pauses its holds — on a `W`×`H` picture: the screen at
//! the largest glyph size its cells fit (`fit`), the scene's `@beat:` captions in a band of their own under it (never
//! over tarae's cells), a title card before and an end card after, the seal stamped on both (`docs/brand/`, a PNG made
//! once from its SVG). ffmpeg makes the MP4 from the raw frames piped in (H.264, 4:2:0, BT.709); where there's none,
//! said — the sheet of the video's own frames is written first all the same.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::recording::{self, Picture};
use crate::scenes::docshots::{Bg, none_of_yours};
use crate::shot::{self, Fonts, PAD, Recorded, Rgb};
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

/// What `make` made: the video's length, its glyph size, its frames and the sheet's.
pub(crate) struct Made {
    pub(crate) seconds: f64,
    pub(crate) px: f32,
    pub(crate) frames: u64,
    pub(crate) sheet: usize,
}

/// `rec` as a video at `mp4` on `bg`'s theme, `title` on its title card, `sheet.png` of its frames beside it. Every text
/// it draws passes `none_of_yours` with `yours` (the screens did, as they were taken).
pub(crate) fn make(rec: &Recorded, bg: Bg, title: &str, mp4: &Path, yours: Option<&OsStr>) -> Result<Made> {
    let Some(first) = rec.frames.first() else { return Err("the take kept no frame".into()) };
    for text in rec.beats.iter().map(|b| b.1.as_str()).chain([title]) {
        none_of_yours(&[text.chars().map(String::from).collect()], yours)?;
    }
    let (rows, cols) = first.1.size();
    let fonts = Fonts::load(1)?;
    let px = fit(&fonts, usize::from(cols), usize::from(rows))
        .ok_or_else(|| format!("{cols}×{rows} cells don't fit {W}×{H} under the captions' band"))?;
    let look = look(bg);
    let seal = seal(bg)?;
    let cards = (
        card(
            &[("t a r a e", NAME_PX, look.accent, true), (title, LINE_PX, look.ink, false)],
            &seal,
            &look,
            &fonts,
        ),
        card(
            &[("t a r a e", NAME_PX, look.accent, true), ("tarae.sh", LINE_PX, look.ink, false)],
            &seal,
            &look,
            &fonts,
        ),
    );
    let times: Vec<u64> = rec.frames.iter().map(|f| f.0).collect();
    let shown_ms = times.last().copied().unwrap_or(0) + LAST_MS;
    let video = Video { rec, times, px, look, fonts, cards, shown_ms };
    let frames = video.frames();
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
    video.encode(&ffmpeg, mp4)?;
    Ok(Made { seconds: frames as f64 / FPS as f64, px, frames, sheet })
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

/// A card's frame: `lines` (text, size, colour, bold) one under another, centred, the seal stamped under them.
fn card(
    lines: &[(&str, f32, Rgb, bool)],
    seal: &(usize, usize, Vec<u8>),
    look: &Look,
    fonts: &Fonts,
) -> Vec<u8> {
    let mut px = look.ground.repeat(W * H);
    let pictures: Vec<Picture> = (lines.iter().filter(|l| !l.0.is_empty()))
        .map(|&(t, size, fg, bold)| text(t, size, fg, look.ground, bold, fonts))
        .collect();
    let gap = 48;
    let tall = pictures.iter().map(|p| p.1).sum::<usize>() + gap + seal.1;
    let mut y = H.saturating_sub(tall) / 2;
    for p in &pictures {
        blit(&mut px, p, (W - p.0.min(W)) / 2, y, 255);
        y += p.1;
    }
    let (sw, sh, rgba) = seal;
    let (x0, y0) = ((W - sw) / 2, y + gap);
    for sy in 0..*sh {
        for sx in 0..*sw {
            let s = &rgba[(sy * sw + sx) * 4..][..4];
            let at = ((y0 + sy) * W + x0 + sx) * 3;
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

/// `p` on the `W`×`H` frame `dst` at (`x0`, `y0`), `a`/255 of it over what's there; clipped.
fn blit(dst: &mut [u8], p: &Picture, x0: usize, y0: usize, a: u8) {
    let (pw, ph, src) = p;
    let w = (*pw).min(W.saturating_sub(x0));
    for y in 0..(*ph).min(H.saturating_sub(y0)) {
        let (from, to) = ((y * pw) * 3, ((y0 + y) * W + x0) * 3);
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

/// `p` at half its size, each pixel the mean of four (a sheet's frame).
fn half(p: &[u8]) -> Picture {
    let (w, h) = (W / 2, H / 2);
    let mut out = vec![0; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let at = |dx: usize, dy: usize| p[((2 * y + dy) * W + 2 * x + dx) * 3 + c] as u32;
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
    /// The take's screen, its caption (the beat's) and how far that has come in, of 255.
    Screen(usize, Option<usize>, u8),
    End,
}

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
}

impl Video<'_> {
    fn frames(&self) -> u64 {
        ((TITLE_MS + self.shown_ms + END_MS) * FPS).div_ceil(1000)
    }

    /// The `k`th frame's ms in the video.
    fn ms(k: u64) -> u64 {
        k * 1000 / FPS
    }

    fn shows(&self, k: u64) -> Shows {
        let title = TITLE_MS * FPS / 1000;
        let t = Self::ms(k);
        if t < TITLE_MS {
            return Shows::Title;
        }
        if t >= TITLE_MS + self.shown_ms {
            return Shows::End;
        }
        let local = t - TITLE_MS;
        let beat =
            self.rec.beats.iter().rposition(|b| b.0 <= local).filter(|&i| !self.rec.beats[i].1.is_empty());
        let came = beat.map_or(255, |i| ((local - self.rec.beats[i].0) * 255 / FADE_MS).min(255) as u8);
        Shows::Screen(screen_at(&self.times, k - title), beat, came)
    }

    /// The frame `shows` says, its screen drawn into `drawn` when it isn't the one there (`(index, picture)`) and its
    /// captions' pictures kept in `captions`.
    fn frame(
        &self,
        shows: Shows,
        drawn: &mut Option<(usize, Picture)>,
        captions: &mut HashMap<usize, Picture>,
    ) -> Vec<u8> {
        let (i, beat, came) = match shows {
            Shows::Title => return self.cards.0.clone(),
            Shows::End => return self.cards.1.clone(),
            Shows::Screen(i, beat, came) => (i, beat, came),
        };
        if drawn.as_ref().is_none_or(|d| d.0 != i) {
            *drawn = Some((i, shot::render_px(&self.rec.frames[i].1, &self.fonts, self.px)));
        }
        let Some((_, screen)) = drawn.as_ref() else { unreachable!("drawn above") };
        let mut px = self.look.ground.repeat(W * H);
        let (x0, y0) = ((W - screen.0) / 2, (H - BAND - screen.1) / 2);
        blit(&mut px, screen, x0, y0, 255);
        // (the band's hairline, the screen's cells wide)
        let at = (H - BAND) * W * 3;
        for x in x0 + PAD..x0 + screen.0 - PAD {
            px[at + x * 3..][..3].copy_from_slice(&self.look.faint);
        }
        if let Some(b) = beat {
            let caption = captions.entry(b).or_insert_with(|| {
                text(&self.rec.beats[b].1, CAPTION_PX, self.look.ink, self.look.ground, false, &self.fonts)
            });
            blit(&mut px, caption, (W - caption.0.min(W)) / 2, H - BAND + (BAND - caption.1) / 2, came);
        }
        px
    }

    /// Every frame through ffmpeg into `mp4`: raw RGB on its stdin, H.264 `slow` CRF 20 tuned for flat pictures, 4:2:0
    /// in BT.709 (what a phone's player assumes for HD), the index at the front.
    fn encode(&self, ffmpeg: &Path, mp4: &Path) -> Result<()> {
        let size = format!("{W}x{H}");
        let fps = FPS.to_string();
        let mut child = Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s"])
            .args([&size, "-r", &fps, "-i", "-", "-c:v", "libx264", "-preset", "slow", "-crf", "20"])
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

    /// The sheet of the video's own frames at `out` (`recording::sheet_picks`: one every 2 s and at each beat, the
    /// cards' first frames too), each at half size with its time in the video and its beat under it.
    fn sheet(&self, out: &Path) -> Result<usize> {
        let times: Vec<u64> = (0..self.frames()).map(Self::ms).collect();
        let mut marks: Vec<(u64, String)> = self
            .rec
            .beats
            .iter()
            .filter(|b| !b.1.is_empty())
            .map(|(t, c)| (TITLE_MS + t, c.clone()))
            .collect();
        // (the end card's first frame: the first whose ms is past the take's)
        marks.push((Self::ms(((TITLE_MS + self.shown_ms) * FPS).div_ceil(1000)), "end card".into()));
        let picks = recording::sheet_picks(&times, &marks, recording::SHEET_EVERY_MS);
        let (mut drawn, mut captions) = (None, HashMap::new());
        let pictures: Vec<Picture> = picks
            .iter()
            .map(|(k, said)| {
                let frame = self.frame(self.shows(*k as u64), &mut drawn, &mut captions);
                recording::labelled(half(&frame), said, W / 2 / shot::cell(&self.fonts, 15.0).0, &self.fonts)
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
