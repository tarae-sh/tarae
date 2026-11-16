//! The cut drawn (`cargo xtask cut FILE`): one pass over the video's frames in order, the take's log replayed as it goes
//! (`record::Replay` — one screen held, however long the take), each frame composed as the wide video composes one —
//! the screen at `video::fit`'s glyph centred above the band — and piped to ffmpeg; the sheet's pictures taken on the
//! way.
//!
//! - The camera: the whole, but for a zoom — its target found on the screen at its time (`camera::target`, or within
//!   `FIND_MS` after), widened to the screen's shape (`wide_window`, never nearer than `ZOOM_MAX`), eased in and out
//!   as the Shorts' camera moves (`camera::Path`), drawn through `video::view` so a zoom never steps: the screen's
//!   picture drawn once at the read's glyph for the whole stretch off the whole (`Glyphs`), taken down and up to each
//!   frame, the whole's own laid over it near the whole (`FADE_ZOOM`) — the text moving with the camera, never shaking.
//!   Where it ends has its own path, on its own clock, from the opening's `zoom` (`paths`); a `[[zoom]]` is the take's.
//! - The masks (`mask.rs`): each frame's cells drawn blank in the screen's own picture, before the camera takes it.
//! - Crossings (`camera::CROSS_MS`, never a cut): where a part begins (the card of what's built, where it ends, a
//!   chapter's card, the take after them, the end card) through the ground — the part going out fades to it, then the
//!   next comes up (`cross_px`) —; where time is left out, and where most of the cells change at once
//!   (`camera::CROSS`) between two screens that each stand `camera::STAND_MS` of video (`changes`, else a cut), the two
//!   screens over each other. A crossing between two identical pictures draws nothing (`Crossings::over`); none into a
//!   chapter's end held or the take's last screen held, the picture they leave.
//! - The sheet (`sheet_picks`): each picture taken once its part has come up, never inside a crossing.
//! - The words over where it ends (`label.rs`), where the opening has them, over the board and under the band.
//! - The band (`band`): the clock, the jump's mark, the speed's mark or `where it ends` (none while the words say it),
//!   the caption; the mascot at the keys at its right end (`mascot.rs`), the marks `mascot::GAP` from its box, a
//!   caption that would reach them refused.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use super::mascot::motions::{self, Plan};
use super::mascot::pops::{self, Pops};
use super::mascot::{self, Art, Hands, Pic, Place};
use super::picture::{self, Boards};
use super::route;
use super::{
    Card, Cut, Frame, Marks, Pace, Program, Weight, What, clock, factor, jumped, label, opening, sped, told,
    video_time, weight,
};
use crate::Result;
use crate::camera::{self, Cells, Region};
use crate::record::{Replay, Take};
use crate::recording::Picture;
use crate::scenes::docshots::none_of_yours;
use crate::shot::{self, Blank, Fonts, PAD, Rgb};
use crate::video::{self, BAND, H, Scale, W};

/// The band's own words — the clock, the jump's mark, the speed's and `where it ends` — and how strongly the quiet ones (the
/// clock, the speed) are drawn over the ground, of 255: read, never louder than the caption.
pub(crate) const MARK_PX: f32 = 26.0;
pub(crate) const QUIET: u8 = 170;
/// The mark over the take's stretch the opening shows, and its part's name on the sheet.
pub(crate) const ENDS: &str = "where it ends";
/// A chapter card's title (and the words over where it ends, `label.rs`).
pub(crate) const CHAPTER_PX: f32 = 56.0;
/// The camera never goes nearer than this: its window at least 1/`ZOOM_MAX` of the screen across.
pub(crate) const ZOOM_MAX: f32 = 2.0;
/// A zoom's target is looked for this long after its time (tarae may draw it a frame or two later).
const FIND_MS: u64 = 2_000;
/// The sheet: a picture every `SHEET_EVERY_US` of the video (more apart past `SHEET_REGULAR` of them), and at each part,
/// caption, zoom, jump and change of speed; `PAGE` a page.
const SHEET_EVERY_US: u64 = 2_000_000;
const SHEET_REGULAR: u64 = 120;
const PAGE: usize = 6;

/// What a cut makes: the video — `cut.mp4` and its sound, the sheet, `picks.tsv` —, or (`--picks`) the sheet and
/// `picks.tsv` alone, a frame drawn only where the sheet picks it or a crossing begins from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Made {
    Video,
    Picks,
    /// Every frame drawn as the video's are, nothing encoded: what `Picks`' frames are held to in its test.
    #[cfg(test)]
    Drawn,
}

pub(super) fn run(
    file: &Path,
    scale: Scale,
    out: Option<&Path>,
    (made, stills): (Made, &[u64]),
) -> Result<()> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let dir = file.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let take = Take::read(dir)?;
    let marks = Marks::read(dir, take.ms())?;
    let cut = Cut::parse(&text, &file.display().to_string(), &marks)?;
    let yours = std::env::var_os("HOME");
    let words = (cut.chapters.iter().map(|c| c.title.as_str()))
        .chain(cut.captions.iter().map(|c| c.text.as_str()))
        .chain([cut.opening.name.as_str(), cut.opening.line.as_str()])
        .chain(cut.opening.code.iter().map(String::as_str))
        .chain(cut.opening.label.iter().chain(&cut.opening.label_line).map(String::as_str))
        .chain(cut.pictures.iter().flat_map(|p| &p.steps).flat_map(|s| &s.text).map(String::as_str));
    for w in words {
        none_of_yours(&[w.chars().map(String::from).collect()], yours.as_deref())?;
    }
    let stem = file.file_stem().map_or("cut".into(), |s| s.to_string_lossy().into_owned());
    // (2160p beside 1080p, never over it)
    let list = file.canonicalize().map_err(|e| format!("{}: {e}", file.display()))?;
    let out = match (out, scale) {
        (Some(o), _) => {
            out_refused(o, dir, &list)?;
            o.to_path_buf()
        }
        (None, Scale::HD) => dir.join(stem),
        (None, _) => dir.join(format!("{stem}-{}", scale.name())),
    };
    let mut said = make(&cut, &take, (dir, &marks), &out, (scale, made, stills))?;
    said.push_str(&format!("{MADE_FROM}{}\n", list.display()));
    let p = out.join("cut.txt");
    std::fs::write(&p, &said).map_err(|e| format!("{}: {e}", p.display()))?;
    print!("{said}");
    Ok(())
}

/// `cut.txt`'s last line: the list it was made from, as its real path — what `out_refused` tells a cut by.
const MADE_FROM: &str = "made from the list ";

/// `--out DIR` refused where it's the take's own folder (`take`) or a cut's in it (a `cut.txt` there): the take's
/// records and its cuts are only read; and where it holds a cut made from another list than `list` (its `cut.txt`
/// naming another, or none) — a folder of this list's own earlier run stays usable. Compared as real paths, the part
/// not made yet named without `..`.
pub(crate) fn out_refused(out: &Path, take: &Path, list: &Path) -> Result<()> {
    let said = |why: &str| format!("--out {}: {why} — name a new folder", out.display());
    let abs = std::path::absolute(out).map_err(|e| said(&e.to_string()))?;
    // (the nearest folder there, as it really is, and the rest under it)
    let (mut there, mut rest) = (abs.as_path(), Vec::new());
    let real = loop {
        if let Ok(r) = there.canonicalize() {
            break r;
        }
        match (there.file_name(), there.parent()) {
            (Some(n), Some(up)) => (rest.push(n), there = up),
            (None, Some(_)) => return Err(said("a `..` in the part not made yet")),
            (None, None) | (Some(_), None) => return Err(said("no part of it is there")),
        };
    };
    let real = rest.iter().rev().fold(real, |r, n| r.join(n));
    let take = take.canonicalize().map_err(|e| format!("{}: {e}", take.display()))?;
    match (real == take, real.starts_with(&take) && real.join("cut.txt").is_file()) {
        (true, _) => Err(said("the take's own folder, whose records are only read")),
        (_, true) => Err(said("a cut of the take's (its cut.txt there), only read")),
        _ => {
            let Ok(was) = std::fs::read_to_string(real.join("cut.txt")) else { return Ok(()) };
            let from = was.lines().rev().find_map(|l| l.strip_prefix(MADE_FROM));
            match from {
                Some(f) if Path::new(f) == list => Ok(()),
                Some(f) => Err(said(&format!("it holds a cut of another list ({f}), not written over"))),
                None => Err(said("it holds a cut whose cut.txt names no list, not written over")),
            }
        }
    }
}

/// The FNV-1a (64) of a frame's raw RGB: `picks.tsv`'s.
pub(crate) fn fnv1a(px: &[u8]) -> u64 {
    px.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}

/// A zoom's window for `target` on a screen of `cols`×`rows`: the target at its middle, widened to the screen's shape
/// (the picture it's drawn into is the screen's), at least 1/`ZOOM_MAX` of it, inside it.
pub(crate) fn wide_window(t: Region, (cols, rows): (u16, u16)) -> Region {
    let (c, r) = (f32::from(cols), f32::from(rows));
    let w = t.w.max(t.h * c / r).max(c / ZOOM_MAX).min(c);
    let h = w * r / c;
    let x = (t.x + t.w / 2.0 - w / 2.0).clamp(0.0, c - w);
    let y = (t.y + t.h / 2.0 - h / 2.0).clamp(0.0, r - h);
    Region { x, y, w, h }
}

/// The frame in force in the frames' loop: what it looks like, its picture (none where it wasn't needed yet), the frame
/// it was first met on, and the board's rows under its mascot, bare.
type Last = (Key, Option<Vec<u8>>, Frame, Option<Under>);

/// A frame's mascot as `Stage::finish` lays it (`key`'s pose and pops up, of `pops`): none on a card.
fn mascot_of<'a>(key: &'a Key, pops: &'a Pops) -> (Option<Pic>, &'a Pops, &'a [(usize, u64)]) {
    (key.pose.filter(|_| key.card.is_none()), pops, &key.pops)
}

/// What a frame looks like, to draw it only where it changed: its card, the clock's second, the band's marks and
/// caption, the screen (whose replay, how many screens in), the camera's region, a crossing's share.
#[derive(PartialEq)]
struct Key {
    card: Option<Card>,
    secs: Option<u64>,
    speed: Option<f64>,
    jump: Option<(u64, u8)>,
    ends: bool,
    caption: Option<(usize, u8)>,
    chapter: Option<(usize, u8)>,
    screen: (bool, u64),
    region: Region,
    cross: Option<u8>,
    pose: Option<Pic>,
    pops: Vec<(usize, u64)>,
    blank: Vec<Blank>,
    label: Option<super::label::Label>,
    picture: Option<picture::Shows>,
    route: Option<route::Shows>,
}

/// The screen's picture kept, by what it was drawn of: whose replay, how many screens in, the glyph (px) and the cells
/// drawn blank.
pub(crate) type Kept = (bool, u64, u32, Vec<Blank>);

/// `text` at `px` (1080p's) in `fg`, drawn at `scale`, kept: the band's words repeat.
pub(crate) struct Words<'f> {
    fonts: &'f Fonts,
    ground: Rgb,
    scale: Scale,
    kept: HashMap<(String, u32, Rgb, bool), Picture>,
}

impl<'f> Words<'f> {
    pub(crate) fn new(fonts: &'f Fonts, ground: Rgb, scale: Scale) -> Words<'f> {
        Words { fonts, ground, scale, kept: HashMap::new() }
    }

    fn get(&mut self, text: &str, px: f32, fg: Rgb) -> &Picture {
        self.word(text, px, fg, false)
    }

    /// `text` at `px` in `fg`, bold or not.
    pub(crate) fn word(&mut self, text: &str, px: f32, fg: Rgb, bold: bool) -> &Picture {
        if self.kept.len() > 512 {
            self.kept.clear();
        }
        let (fonts, ground, s) = (self.fonts, self.ground, self.scale);
        (self.kept.entry((text.to_string(), px as u32, fg, bold)))
            .or_insert_with(|| video::text_at(text, px, fg, ground, bold, fonts, s))
    }
}

/// Where a take's frame puts things at a scale (px): the picture; the screen's glyph (1080p's px, drawn at `scale`:
/// `video::fit`'s) and its picture's size and place, centred above the band; the band's rule's row and its middle; the
/// renderer's margin. Every figure 1080p's constants times the scale, never a second set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Layout {
    pub(crate) scale: Scale,
    pub(crate) dims: (usize, usize),
    pub(crate) glyph: f32,
    pub(crate) pic: (usize, usize),
    pub(crate) at: (usize, usize),
    pub(crate) rule: usize,
    pub(crate) mid: usize,
    pub(crate) pad: usize,
}

/// The layout of a take `size` cells at `s` — refused where its cells don't fit 1080p's picture over the band.
pub(crate) fn layout(fonts: &Fonts, size: (u16, u16), s: Scale) -> Result<Layout> {
    let (cols, rows) = (usize::from(size.0), usize::from(size.1));
    let glyph = video::fit(fonts, cols, rows)
        .ok_or_else(|| format!("{cols}×{rows} cells don't fit {W}×{H} over the band"))?;
    // (the renderer's own picture of them: `shot::render_scaled`'s, its cells and margin `s` times)
    let (cw, ch) = shot::cell(fonts, glyph);
    let pic = (s.px(cols * cw + 2 * PAD), s.px(rows * ch + 2 * PAD));
    let (w, h) = s.dims();
    let band = s.px(BAND);
    Ok(Layout {
        scale: s,
        dims: (w, h),
        glyph,
        pic,
        at: (s.centre(w, pic.0), s.centre(h - band, pic.1)),
        rule: h - band,
        mid: h - band / 2,
        pad: s.px(PAD),
    })
}

impl Layout {
    /// The camera's `region` of a `size` screen through the screen's place (`video::view_drawn`, its cells and margin
    /// the renderer's at this scale; the view's px 1080p's, drawn at the scale): its picture drawn at `glyph` where it
    /// says (`Glyphs`), else at the view's own.
    pub(crate) fn view(
        &self,
        region: Region,
        size: (u16, u16),
        fonts: &Fonts,
        glyph: Option<f32>,
    ) -> video::View {
        let k = self.scale.k();
        let cell = |p| {
            let (cw, ch) = shot::cell(fonts, p);
            (cw * k, ch * k)
        };
        video::view_drawn(region, size, self.pic, (self.glyph, glyph), (cell, self.pad))
    }

    /// How far the camera's `region` of a `size` screen is pushed in (`video::zoom`): 1 at the whole.
    pub(crate) fn zoom(&self, region: Region, size: (u16, u16), fonts: &Fonts) -> f32 {
        let (cw, ch) = shot::cell(fonts, self.glyph);
        let k = self.scale.k();
        video::zoom(region, size, self.pic, (cw * k, ch * k))
    }
}

/// The camera's zoom (`video::zoom`) under which the whole's own picture is laid over the zoom's, fading out as it goes
/// in: the two pictures' glyphs sit a fraction of a px apart in their cells (each glyph size rounds its baseline its own
/// way — 0.25 px at 1080p, 0.47 px at 2160p in take 3's zooms), so the camera leaving or reaching the whole slides
/// from one to the other, never steps — over four frames of the quick start going in (at 1.1, three: a 0.32 px step at
/// 2160p), over the slow end's twenty-odd coming out.
const FADE_ZOOM: f32 = 1.2;

/// The screen's pictures a take's frame draws from, kept: the zoom's (at `Glyphs`'), and the whole's (at the layout's
/// glyph — at rest, and over the zoom's near the whole).
#[derive(Default)]
pub(crate) struct Pictures {
    zoom: Option<(Kept, Picture)>,
    whole: Option<(Kept, Picture)>,
}

/// The screen as a take's frame lays it: the whole's picture as it is, or a picture through the camera's view — near
/// the whole, with the whole's own through its view over it, this much of it (of 255).
pub(crate) enum Shown<'p> {
    Whole(&'p Picture),
    Through(&'p Picture, video::View, Option<(&'p Picture, video::View, u8)>),
}

/// The glyph a take's frame draws the screen's picture at through each stretch a camera's path is off the whole (µs on
/// the path: from, to, glyph): the deepest of its reads' own (`video::view`'s glyph for it, as the camera rests there)
/// — one picture for the move in, the read and the move out, taken down or up to each frame's region, never a glyph
/// size a frame (take 3's zooms: 19 sizes in a 1.2 s move, each rounding its baseline and its glyphs' left edges in
/// its own way — the text shook up to 0.8 px a frame, back and forth).
pub(crate) struct Glyphs(Vec<(u64, u64, f32)>);

impl Glyphs {
    /// The glyphs of `path`'s stretches off the whole, for a `size` screen laid out as `lay`.
    pub(crate) fn of(path: &camera::Path, lay: &Layout, size: (u16, u16), fonts: &Fonts) -> Glyphs {
        Glyphs(
            (path.away().into_iter())
                .map(|(from, to, reads)| {
                    let rest = |r: &Region| lay.view(*r, size, fonts, None).px;
                    (from * 1000, to * 1000, reads.iter().map(rest).fold(lay.glyph, f32::max))
                })
                .collect(),
        )
    }

    /// The glyph `us` along the path, where it's off the whole.
    pub(crate) fn at(&self, us: u64) -> Option<f32> {
        self.0.iter().find(|s| (s.0..=s.1).contains(&us)).map(|s| s.2)
    }
}

/// What every take's frame is drawn on: its layout, the theme, the cut, the mascot and where the band's marks end.
pub(crate) struct Stage<'a> {
    pub(crate) lay: Layout,
    pub(crate) look: &'a video::Look,
    pub(crate) cut: &'a Cut,
    pub(crate) mascot: Option<(Art, Place)>,
    pub(crate) marks_right: usize,
    stamping: std::cell::Cell<std::time::Duration>,
}

impl<'a> Stage<'a> {
    pub(crate) fn new(
        lay: Layout,
        look: &'a video::Look,
        cut: &'a Cut,
        mascot: Option<(Art, Place)>,
    ) -> Stage<'a> {
        let marks_right = mascot.as_ref().map_or(lay.at.0 + lay.pic.0, |m| m.1.marks_right);
        Stage { lay, look, cut, mascot, marks_right, stamping: Default::default() }
    }

    /// The screen `s` (`which`: whose replay, how many screens in) drawn at glyph `at`, `blank`'s cells blank in it — so
    /// a crop or a zoom resamples them blank —; kept in `kept` while the same screen comes at the same glyph with the
    /// same cells blank. Whether it was drawn now.
    fn kept_pic<'k>(
        &self,
        kept: &'k mut Option<(Kept, Picture)>,
        (s, which): (&vt100::Screen, (bool, u64)),
        at: f32,
        blank: &[Blank],
        fonts: &Fonts,
    ) -> (&'k Picture, bool) {
        let want = (which.0, which.1, at as u32, blank.to_vec());
        let fresh = kept.as_ref().is_none_or(|k| k.0 != want);
        if fresh {
            *kept = Some((want, shot::render_masked(s, fonts, at, self.lay.scale.k(), blank)));
        }
        (&kept.as_ref().expect("drawn").1, fresh)
    }

    /// The screen `s` (`which`: whose replay, how many screens in) of `size` as a take's frame shows it through the
    /// camera's `region`: at the whole, its picture at the layout's glyph as it is; else through the camera's view, its
    /// picture at the stretch's glyph where `glyph` says (`Glyphs`), else the view's own — and under `FADE_ZOOM` the
    /// whole's picture through the view too, over it as much as the camera is near the whole. Both kept in `kept`
    /// (`kept_pic`); how many were drawn now.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn shown<'k>(
        &self,
        kept: &'k mut Pictures,
        screen: (&vt100::Screen, (bool, u64)),
        (region, glyph): (Region, Option<f32>),
        size: (u16, u16),
        blank: &[Blank],
        fonts: &Fonts,
    ) -> (Shown<'k>, u64) {
        let lay = &self.lay;
        let Pictures { zoom, whole } = kept;
        if region == Region::all(size) {
            let (pic, fresh) = self.kept_pic(whole, screen, lay.glyph, blank, fonts);
            return (Shown::Whole(pic), u64::from(fresh));
        }
        let view = lay.view(region, size, fonts, glyph);
        let (pic, fresh) = self.kept_pic(zoom, screen, view.px, blank, fonts);
        let near = (lay.zoom(region, size, fonts) - 1.0) / (FADE_ZOOM - 1.0);
        if view.px == lay.glyph || near >= 1.0 {
            return (Shown::Through(pic, view, None), u64::from(fresh));
        }
        let (under, fresh_whole) = self.kept_pic(whole, screen, lay.glyph, blank, fonts);
        let a = (255.0 * (1.0 - camera::ease_in_out(near))).round() as u8;
        let over = (under, lay.view(region, size, fonts, Some(lay.glyph)), a);
        (Shown::Through(pic, view, Some(over)), u64::from(fresh) + u64::from(fresh_whole))
    }

    /// The screen as `screen` shows it (`shown`) laid in its place on a frame `px` — through a view, its `rows` alone (of
    /// the picture's place, from its top; the whole picture is laid whole).
    pub(crate) fn lay_screen(&self, px: &mut [u8], screen: &Shown, rows: std::ops::Range<usize>) {
        let lay = &self.lay;
        let w = lay.dims.0;
        let (x0, y0) = lay.at;
        match screen {
            Shown::Whole(pic) => video::blit(px, lay.dims, pic, x0, y0, 255),
            Shown::Through(pic, view, over) => {
                let place = ((x0, y0), lay.pic);
                video::resample_rows(px, w, place, pic, view, rows.clone());
                if let Some((under, view, a)) = over {
                    // (the whole's picture through its view, laid over by `a`)
                    let mut whole = px.to_vec();
                    video::resample_rows(&mut whole, w, place, under, view, rows.clone());
                    let bytes = ((y0 + rows.start) * w * 3)..((y0 + rows.end) * w * 3);
                    let (a, b) = (u32::from(*a), 255 - u32::from(*a));
                    for (d, o) in px[bytes.clone()].iter_mut().zip(&whole[bytes]) {
                        *d = ((u32::from(*d) * b + u32::from(*o) * a + 127) / 255) as u8;
                    }
                }
            }
        }
    }

    /// The take's frame `f`: the screen as `screen` shows it (`shown`), the band's rule (a line `scale` px thick), the
    /// band, the mascot in `pose` and the key pops up (`up`: of `pops`, with their age in frames) in its lane — no
    /// crossing over it (the frames' loop: `take_bare`, then `finish`).
    #[cfg(test)]
    pub(crate) fn take_frame(
        &self,
        screen: &Shown,
        f: &Frame,
        mascot: (Option<Pic>, &Pops, &[(usize, u64)]),
        words: &mut Words,
    ) -> Vec<u8> {
        let mut px = self.take_bare(screen, f, words);
        self.lay_mascot(&mut px, mascot, words);
        px
    }

    /// The take's frame `f` without its mascot: what `finish` lays it on, after the crossing in force or before.
    pub(crate) fn take_bare(&self, screen: &Shown, f: &Frame, words: &mut Words) -> Vec<u8> {
        let (w, h) = self.lay.dims;
        let mut px = self.look.ground.repeat(w * h);
        self.lay_screen(&mut px, screen, 0..self.lay.pic.1);
        self.dress(px, f, words)
    }

    /// A picture's frame `f` without its mascot: the `board` (the frame above the band, `picture.rs`) in place of the
    /// screen, the rest as a take's frame has it.
    pub(crate) fn picture_bare(&self, board: &[u8], f: &Frame, words: &mut Words) -> Vec<u8> {
        let (w, h) = self.lay.dims;
        let mut px = self.look.ground.repeat(w * h);
        px[..board.len().min(self.lay.rule * w * 3)]
            .copy_from_slice(&board[..board.len().min(self.lay.rule * w * 3)]);
        self.dress(px, f, words)
    }

    /// A bare frame `px` finished: the crossing in force over it (`share` of the way through, `crossings`') and the
    /// mascot (`mascot`, as `take_frame` has it) — after the crossing where it crosses the board alone, into and out of a
    /// picture: the character isn't the board's, its head over the rule never dimmed by a straight edge; else before it,
    /// crossed with the frame whole (into a card, which has none). What the mascot covers of the board's rows, bare —
    /// where no crossing was laid under it — for a crossing of the board from this frame (`Crossings::bare`).
    pub(crate) fn finish(
        &self,
        px: &mut [u8],
        mascot: (Option<Pic>, &Pops, &[(usize, u64)]),
        share: Option<u8>,
        crossings: &mut Crossings,
        words: &mut Words,
    ) -> Option<Under> {
        let ground = self.look.ground;
        if crossings.confines(share) {
            crossings.over(px, ground, share);
            return self.lay_mascot(px, mascot, words);
        }
        let under = self.lay_mascot(px, mascot, words);
        crossings.over(px, ground, share);
        under.filter(|_| share.is_none())
    }

    /// What's laid over a frame's board `px`: the words over where it ends, the band's rule, the band.
    fn dress(&self, mut px: Vec<u8>, f: &Frame, words: &mut Words) -> Vec<u8> {
        let lay = &self.lay;
        let w = lay.dims.0;
        let x0 = lay.at.0;

        // (the words over where it ends, on the board as this frame drew it)
        if let (Some(l), Some(title)) = (f.label, &self.cut.opening.label) {
            let ink = self.look.ink;
            let title = words.word(title, label::TITLE_PX, ink, true).clone();
            let small = (self.cut.opening.label_line.as_ref())
                .map(|s| words.word(s, label::LINE_PX, ink, false).clone());
            let cell = shot::cell(words.fonts, label::TITLE_PX).0 * lay.scale.k();
            label::draw(&mut px, lay, (&title, small.as_ref()), self.look, cell, l);
        }
        let rule_end = self.mascot.as_ref().map_or(x0 + lay.pic.0 - lay.pad, |m| m.1.rule_end);
        for y in lay.rule..lay.rule + lay.scale.k() {
            for x in x0 + lay.pad..rule_end {
                px[(y * w + x) * 3..][..3].copy_from_slice(&self.look.faint);
            }
        }
        // (the route on it, where the list has one)
        if let (Some(r), Some(shows)) = (&self.cut.route, f.route) {
            let look = self.look;
            let at = (w, lay.rule, lay.scale.k());
            route::draw(
                &mut px,
                at,
                (x0 + lay.pad, rule_end),
                r.stations.len(),
                shows,
                (look.ink, look.faint, look.accent),
            );
        }
        band(&mut px, lay, f, self.cut, (x0, self.marks_right), self.look, words);
        px
    }

    /// The mascot in `pose` over a frame `px`, the swing's V and the key pops up (`up`: of `pops`, with their age in
    /// frames) in its lane over it — the pops over its paw, so laid with it. What it covers of the board's rows (from
    /// its box's top), as they were before; none where no mascot is laid.
    pub(crate) fn lay_mascot(
        &self,
        px: &mut [u8],
        (pose, pops, up): (Option<Pic>, &Pops, &[(usize, u64)]),
        words: &mut Words,
    ) -> Option<Under> {
        let lay = &self.lay;
        let w = lay.dims.0;
        let mut under = None;
        if let (Some((art, place)), Some(p)) = (&self.mascot, pose) {
            let t = Instant::now();
            let rows = (place.y.min(lay.rule) * w * 3)..(lay.rule * w * 3);
            under = Some(Under { at: rows.start, bytes: px[rows].to_vec() });
            video::stamp(px, lay.dims, art.picture(p), (place.x, place.y));
            // (the swing's big V over the lane, against the box, where the swing draws it: no pop is up then)
            if let (Some((left, _)), Some(v)) = (place.lane, art.mark(p)) {
                video::stamp(px, lay.dims, v, (left, place.y));
            }
            if let Some((left, right)) = place.lane {
                let s = lay.scale;
                for &(i, age) in up {
                    let pop = &pops.list[i];
                    let (rise, a) = pops::motion(age);
                    let (size, fg) = match pop.command {
                        true => (pops::COMMAND_PX, self.look.ink),
                        false => (pops::TYPED_PX, self.look.faint),
                    };
                    let word = words.word(&pop.text, size, fg, pop.command);
                    // (its ink right-aligned to the lane's right edge, a step aside; never past the lane's left —
                    // the mascot's own press centred over its right paw)
                    let x = match pop.paw {
                        true => (place.x + s.px(pops::PAW_X)).saturating_sub(word.0 / 2),
                        false => (right + lay.pad)
                            .saturating_sub(s.px(pop.aside) + word.0)
                            .max(left.saturating_sub(lay.pad)),
                    };
                    let mid =
                        place.y + s.px(mascot::PAWS) - (rise * s.px(pops::RISE) as f32).round() as usize;
                    pops::ink(
                        px,
                        lay.dims,
                        word,
                        (x, mid.saturating_sub(word.1 / 2)),
                        (fg, self.look.ground),
                        a,
                    );
                }
            }
            self.stamping.set(self.stamping.get() + t.elapsed());
        }
        under
    }
}

/// A frame's board rows under its mascot as they were before it was laid, from byte `at` to the board's end: what a
/// crossing of the board alone leaves from, so the head it had is never a ghost in the board's rows.
pub(crate) struct Under {
    at: usize,
    bytes: Vec<u8>,
}

/// The take part's whole-screen changes (video µs: each frame whose screen has more than `camera::CROSS` of its cells
/// unlike the one shown before it — as the frames will see the replay) and those of them that cross (`camera::standing`,
/// against the edit list's own crossings); the rest cut. One pass over the replay before the frames: the look ahead.
pub(crate) fn changes(prog: &Program, take: &Take) -> (Vec<u64>, Vec<u64>) {
    let mut replay = Replay::new(&take.screen, take.size);
    let mut seen: Option<Cells> = None;
    let mut whole = Vec::new();
    for k in 0..prog.frames() {
        let v = Program::time_of(k);
        let f = prog.frame(v);
        let Some(t) = f.screen.filter(|_| !f.ends) else { continue };
        if replay.to(t) {
            let cells = Cells::of(replay.screen());
            if seen.as_ref().is_some_and(|s| camera::changed(s, &cells) > camera::CROSS) {
                whole.push(v);
            }
            seen = Some(cells);
        }
    }
    let edges: Vec<u64> = prog.crossings().iter().map(|c| c.0).collect();
    let crossing = camera::standing(&whole, &edges);
    (whole, crossing)
}

/// What moves on the take's screen through each window (the take's ms, from–to: `Program::stand_window`): None where
/// it stands — no screen unlike the one before comes in it —, else how many came and where: the topmost row that
/// changed and its text then, how many more, or the cursor alone. One replay, the windows in the take's order (a new one where
/// one goes back).
pub(crate) fn moving(take: &Take, windows: &[(u64, u64)]) -> Vec<Option<String>> {
    const SAID: usize = 40;
    let mut order: Vec<usize> = (0..windows.len()).collect();
    order.sort_by_key(|&i| windows[i]);
    let mut out = vec![None; windows.len()];
    let mut replay = Replay::new(&take.screen, take.size);
    // (how far the replay has been fed, ms)
    let mut fed = 0;
    for i in order {
        let (a, b) = windows[i];
        if a < fed {
            replay = Replay::new(&take.screen, take.size);
        }
        replay.to(a);
        fed = fed.max(a);
        let mut seen = Cells::of(replay.screen());
        let (mut came, mut rows) = (0, Vec::<(usize, String)>::new());
        while let Some(ms) = replay.next() {
            fed = ms;
            if ms > b {
                break;
            }
            came += 1;
            let now = Cells::of(replay.screen());
            for (r, (x, y)) in now.text.iter().zip(&seen.text).enumerate() {
                if (x != y || now.ground[r] != seen.ground[r]) && !rows.iter().any(|q| q.0 == r) {
                    rows.push((r, x.concat().trim().to_string()));
                }
            }
            seen = now;
        }
        if came == 0 {
            continue;
        }
        let screens = format!(
            "{came} screen{} in {:.1} s of take",
            if came == 1 { "" } else { "s" },
            (b - a) as f64 / 1e3
        );
        rows.sort_by_key(|q| q.0);
        out[i] = Some(match rows.first() {
            None => format!("{screens}: the cursor"),
            Some((r, text)) => {
                let text: String = match text.chars().count() > SAID {
                    true => text.chars().take(SAID - 1).chain(['…']).collect(),
                    false => text.clone(),
                };
                let more = match rows.len() - 1 {
                    0 => String::new(),
                    n => format!(" and {n} more row{}", if n == 1 { "" } else { "s" }),
                };
                format!("{screens}: row {} “{text}”{more}", r + 1)
            }
        });
    }
    out
}

/// The cut of `take` (in `dir`, its landmarks `marks`) as `cut` says, at `scale`, into `out`: `cut.mp4` with its sound
/// (`sound.rs`, `cut.wav` beside it), the sheet's pages, `picks.tsv`, `chapters.txt` — as `made` says: with
/// `Made::Picks` no MP4 and no sound, a frame drawn only where the sheet picks it or a crossing begins from it (the frame
/// before a crossing drawn as it begins, its screen still in the replays). What was made, said.
pub(crate) fn make(
    cut: &Cut,
    take: &Take,
    (dir, marks): (&Path, &Marks),
    out: &Path,
    (scale, made, stills): (Scale, Made, &[u64]),
) -> Result<String> {
    let started = Instant::now();
    let only_picks = made == Made::Picks;
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let size = take.size;
    let k = scale.k();
    let fonts = Fonts::load(k)?;
    let lay = layout(&fonts, size, scale)?;
    let (glyph, (pw, ph), (x0, y0)) = (lay.glyph, lay.pic, lay.at);
    let dims = lay.dims;
    let mut said = vec![format!(
        "the picture: {}×{} ({}), the screen's glyphs {} px",
        dims.0,
        dims.1,
        scale.name(),
        glyph * k as f32
    )];
    // (the mascot at the keys: its frames and its place, else the cut without it — said)
    let loaded = Instant::now();
    let mascot: Option<(Art, Place)> =
        match mascot::found(cut, mascot::dir(), (x0, pw), y0 + ph - lay.pad, scale) {
            Ok(m) => Some(m),
            Err(none) => {
                said.push(none);
                if cut.mascot.says {
                    said.push(motions::Lines::absent(cut));
                }
                None
            }
        };
    let loaded = loaded.elapsed();
    let look = video::look(cut.bg);
    let stage = Stage::new(lay, &look, cut, mascot);
    let mascot = &stage.mascot;
    let marks_right = stage.marks_right;
    // (measured at 1080p: every width doubles at 2160p)
    captions_clear(cut, &Fonts::load(1)?, (marks_right / k, mascot.is_some()))?;
    // (the pictures' files, checked at 2160p before any frame; their boards at the video's size, read as they come)
    let checking = Instant::now();
    let found = picture::check(cut, look.ground)?;
    let paints = mascot.as_ref().is_some_and(|(art, _)| art.paints());
    let painter = match (paints, mascot) {
        (true, _) => {
            "the character paints each step on the tablet (the mascot's motions say how)".to_string()
        }
        (false, Some((art, _))) => format!(
            "the character watches (painting: its pictures, frame-draw-1…3.png, frame-draw-lift.png, \
             frame-draw-in.png and frame-swing-1.png, not all in {})",
            art.dir.display()
        ),
        (false, None) => "no character to paint them".to_string(),
    };
    said.extend(pictures_said(cut, &found, checking.elapsed(), &painter));
    let mut boards = Boards::new(cut, scale, &look)?;
    let keys = match dir.join("keys.bin").exists() {
        true => crate::record::read_log(&dir.join("keys.bin"))?,
        false => Vec::new(),
    };
    // (the keys heard, the mascot's key pops and motions, laid on knowing every key: before the frames — and laid again
    // with each chapter's end held still where its swing had no room)
    let planned = Instant::now();
    let regions = match mascot {
        Some(_) => Some(pops::Regions::read(dir)?),
        None => None,
    };
    // (a text's width in the lane at 1080p: the renderer's cells, a command's and a letter's size)
    let cw = |px: f32| shot::cell(&fonts, px).0;
    let (cmd_w, typed_w) = (cw(pops::COMMAND_PX), cw(pops::TYPED_PX));
    let width = |t: &str, command: bool| t.chars().count() * if command { cmd_w } else { typed_w };
    let winds = mascot.as_ref().is_some_and(|(art, _)| art.winds());
    if cut.mascot.fast
        && let Some((art, _)) = mascot
        && !winds
    {
        said.push(format!(
            "the skein: its pictures (frame-wind-1…3.png, frame-wind-in.png) not all in {} — fast-forward held as before",
            art.dir.display()
        ));
    }
    let laid = |prog: &Program<'_>| {
        let heard = super::sound::sounds(prog, &keys, marks);
        let hands = Hands::new(&heard.taps).sped(&heard.sped);
        let keyless = heard.stamps_keyless;
        let Some(regions) = &regions else { return ((heard.taps, keyless, hands, Pops::default()), None) };
        let fast = match cut.mascot.fast {
            true => motions::fast_holds(prog).0,
            false => Vec::new(),
        };
        let pops = pops::pops(&heard.struck, regions, cut.mascot.keys, cut.seed, &width)
            .presses(&fast, cut.mascot.keys);
        let causes = motions::causes(prog, marks);
        let mut plan = Plan::new(prog, &hands, &causes, &pops.busy(), &cut.mascot, cut.seed);
        // (pressing fast-forward winds the skein where its pictures were found, else it's held as before)
        if cut.mascot.fast && winds {
            plan.wind(prog, &hands);
        }
        // (painting each step on the tablet as it's drawn, where its pictures were found, else watching)
        if paints {
            plan.paint(prog);
        }
        ((heard.taps, keyless, hands, pops), Some(plan))
    };
    let (prog, (taps, stamps_keyless, hands, pops), plan, held) = held_ends(cut, laid, |w| moving(take, w));
    said.extend(held);
    let planned = planned.elapsed();
    // (the seal made at the picture's size, else 1080p's — smaller, never stretched —, said)
    let seal = match video::seal_at(cut.bg, scale) {
        Ok(s) => s,
        Err(e) if scale != Scale::HD => {
            said.push(format!("the seal: none made at {} ({e}) — 1080p's, at its own size", scale.name()));
            video::seal(cut.bg)?
        }
        Err(e) => return Err(e),
    };
    let card = |lines: &[(&str, f32, Rgb, bool)]| video::card_at(lines, &seal, &look, &fonts, dims, scale);
    let name = ("t a r a e", video::NAME_PX, look.accent, true);
    let told = told(super::worked(cut), prog.total);
    let built_card = opening::card(&cut.opening, &told, cut.bg, &look, &fonts, scale);
    // (the intro: the swing's frames, else the name and the seal — said)
    let intro = match video::WideIntro::new(cut.bg, &look, &fonts, scale) {
        Ok(i) => {
            let (n, (w, h), _) = i.drawn();
            said.push(format!(
                "the intro: the brush's swing, {n} frames made {}×{} and drawn {w}×{h} (from its first that shows \
                 something: {} empty left out, its brush's head cut by {:.2} s), then `t a r a e` risen, held to {:.1} s",
                i.made.0,
                i.made.1,
                i.skipped,
                i.skipped_us() as f64 / 1e6,
                super::INTRO_MS as f64 / 1e3
            ));
            Ok(i)
        }
        Err(e) => {
            said.push(format!("the intro: no brush swing ({e}) — the name and the seal, still"));
            Err(card(&[name]))
        }
    };
    said.push(format!("the card of what's built: “{}” — “{told}”", cut.opening.name));
    if let Some(t) = &cut.opening.label {
        let small = cut.opening.label_line.as_ref().map_or(String::new(), |s| format!(" / “{s}”"));
        said.push(format!(
            "the words over where it ends: “{t}”{small} — the band's `{ENDS}` gone; a light from {:.1} s, a pass of \
             {:.1} s every {:.1} s",
            label::LIGHT_FROM_US as f64 / 1e6,
            label::PASS_US as f64 / 1e6,
            label::EVERY_US as f64 / 1e6
        ));
    }
    let end_card = card(&[name, ("tarae.sh", video::LINE_PX, look.ink, false)]);
    let chapter_cards: Vec<Vec<u8>> = (cut.chapters.iter().enumerate())
        .map(|(i, c)| {
            let n = format!("{}", i + 1);
            card(&[(&n, video::LINE_PX, look.faint, false), (&c.title, CHAPTER_PX, look.ink, true)])
        })
        .collect();

    // (the camera: each zoom's window from its target on the screen then)
    let mut windows = Vec::new();
    let mut found_at = Vec::new();
    let mut scan = Replay::new(&take.screen, size);
    for z in &cut.zooms {
        let (when, found) = find(&mut scan, z.at, z.at + FIND_MS, &z.on);
        found_at.push(when);
        let target = found.ok_or_else(|| {
            format!(
                "{}:{}: zoom on \"{}\": not on the screen at {} nor in the {} s after",
                cut.file,
                z.line,
                z.on,
                clock(z.at),
                FIND_MS / 1000
            )
        })?;
        let window = wide_window(target, size);
        windows.push(window);
        said.push(format!(
            "zoom at {} on \"{}\": {:.0}×{:.0} cells at ({:.0}, {:.0}), {:.2}× the whole",
            clock(z.at),
            z.on,
            window.w,
            window.h,
            window.x,
            window.y,
            f32::from(size.0) / window.w
        ));
    }
    drop(scan);
    // (where it ends: the opening's own zoom, its target looked for from that stretch's start to its end)
    let o = &cut.opening;
    let ends_zoom = match &o.zoom {
        None => None,
        Some((on, line)) => {
            let mut scan = Replay::new(&take.screen, size);
            let (when, found) = find(&mut scan, o.from, o.from + o.len, on);
            let target = found.ok_or_else(|| {
                format!(
                    "{}:{line}: opening: zoom on \"{on}\": not on the screen where it ends ({} to {})",
                    cut.file,
                    clock(o.from),
                    clock(o.from + o.len)
                )
            })?;
            let w = wide_window(target, size);
            said.push(format!(
                "where it ends: zoom on \"{on}\" from {}: {:.0}×{:.0} cells at ({:.0}, {:.0}), {:.2}× the whole",
                clock(when),
                w.w,
                w.h,
                w.x,
                w.y,
                f32::from(size.0) / w.w
            ));
            Some((when - o.from, w))
        }
    };
    let reads = take_reads(cut, &windows);
    let lost = lost_zooms(cut, take, &prog, &reads, &found_at);
    let paths = paths(cut, &windows, ends_zoom, size);
    let glyphs = (Glyphs::of(&paths.0, &lay, size, &fonts), Glyphs::of(&paths.1, &lay, size, &fonts));
    // (the masks against the take's check.txt, and the check's places, to tell which of them the masks hid)
    let (hits, source) = super::check::read(dir)?;
    let check_txt = std::fs::read_to_string(dir.join("check.txt")).ok();
    let masked = super::mask::resolve(&cut.masks, check_txt.as_deref(), &cut.file)?;
    let mut masks = super::mask::Tally::new(&masked, &hits);

    // (the sheet's pictures: which frames, and what each is marked with)
    let frames = prog.frames();
    let picks = sheet_picks(&prog);

    let mp4 = out.join("cut.mp4");
    let ffmpeg = match made {
        Made::Video => video::ffmpeg(),
        _ => None,
    };
    // (the sound first: ffmpeg reads it beside the frames)
    let wav = out.join("cut.wav");
    let heard = match &ffmpeg {
        Some(f) => {
            let skip = intro.as_ref().map_or(0, video::WideIntro::skipped_us);
            Some(super::sound::make(&prog, dir, &keys, marks, (f, skip), &wav)?)
        }
        None => None,
    };
    let mut child = match &ffmpeg {
        Some(f) => {
            Some(video::encoder(f, &mp4, dims, heard.as_ref().map(|_| (wav.as_path(), video::Form::Wide)))?)
        }
        None => None,
    };
    let mut stdin = child.as_mut().and_then(|c| c.stdin.take());
    // (the whole-screen changes found first, each crossing only between two screens that stand)
    let looked = Instant::now();
    let (whole, changed) = changes(&prog, take);
    said.push(format!(
        "whole-screen changes: {} in the take part, {} crossed (each screen standing {:.1} s of video before and after), \
         {} cut; found in {:.1} s",
        whole.len(),
        changed.len(),
        camera::STAND_MS as f64 / 1e3,
        whole.len() - changed.len(),
        looked.elapsed().as_secs_f64()
    ));
    drop(whole);
    let mut crossings =
        Crossings::new(prog.crossings(), changed).confined(prog.confined(), lay.rule * dims.0 * 3);
    let (mut ends, mut replay) = (Replay::new(&take.screen, size), Replay::new(&take.screen, size));
    let mut words = Words::new(&fonts, look.ground, scale);
    let sheet_fonts = Fonts::load(1)?;
    let mut pics = Pictures::default();
    // (the frame in force: what it looks like, its picture — with `--picks` none where it wasn't needed yet — and the
    // frame it was first met on, what it's drawn from if it's needed after; the board's rows under its mascot, bare)
    let mut last: Option<Last> = None;
    let (mut differ, mut drawn, mut painted, mut wrote) = (0u64, 0u64, 0u64, Ok(()));
    let mut hashes: Vec<(u64, u64, u64)> = Vec::new();
    let mut sheet: Vec<Picture> = Vec::new();
    let mut pages = 0;
    let all = Region::all(size);
    let mut posed: HashMap<Pic, u64> = HashMap::new();
    // (a frame's picture: the card it is, else the take's screen `s` as the frame shows it, without its mascot — the
    // crossing in force and the mascot laid after, `finish`; how many screens were drawn for it)
    let paint = |f: &Frame,
                 key: &Key,
                 s: &vt100::Screen,
                 (pics, boards): (&mut Pictures, &mut Boards),
                 words: &mut Words<'_>|
     -> (Vec<u8>, u64) {
        if let (None, Some(shows)) = (f.card, &f.picture) {
            let board = boards.board(shows);
            return (stage.picture_bare(&board, f, words), 0);
        }
        match f.card {
            Some(Card::Intro(ms)) => match &intro {
                Ok(i) => (i.frame(ms, &look), 0),
                Err(still) => (still.clone(), 0),
            },
            Some(Card::Built) => (built_card.clone(), 0),
            Some(Card::End) => (end_card.clone(), 0),
            Some(Card::Chapter(i)) => (chapter_cards[i].clone(), 0),
            None => {
                let camera = (key.region, drawn_at(f, &glyphs));
                let (shown, fresh) = stage.shown(pics, (s, key.screen), camera, size, &key.blank, &fonts);
                (stage.take_bare(&shown, f, words), fresh)
            }
        }
    };
    for k in 0..frames {
        let v = Program::time_of(k);
        let f = prog.frame(v);
        // (`--picks`: the frame before, not drawn, drawn now where a crossing begins from it — its screen still in the
        // replays, the crossings still at it)
        if only_picks
            && crossings.begins(v)
            && let Some((key, held @ None, first, under)) = last.as_mut()
        {
            let s = if first.ends { ends.screen() } else { replay.screen() };
            let (mut px, fresh) = paint(first, key, s, (&mut pics, &mut boards), &mut words);
            *under = stage.finish(&mut px, mascot_of(key, &pops), key.cross, &mut crossings, &mut words);
            (drawn, painted) = (drawn + fresh, painted + 1);
            *held = Some(px);
        }
        // (the screen in force; where most of it changed at once between two that stand, a crossing from the frame
        // before)
        if let Some(t) = f.screen {
            let r = if f.ends { &mut ends } else { &mut replay };
            r.to(t);
        }
        let share = crossings.at(v, last.as_ref().and_then(|l| l.1.as_deref()));
        if let Some(under) = last.as_ref().and_then(|l| l.3.as_ref()) {
            crossings.bare(under);
        }
        let region = region(&f, &paths, size);
        let screen = match f.ends {
            true => (true, ends.shown),
            false => (false, replay.shown),
        };
        // (the masks' cells in this frame, the take's screen at its time)
        let blank = match f.screen {
            Some(t) => masks.frame(t, if f.ends { ends.screen() } else { replay.screen() }),
            None => Vec::new(),
        };
        let pose = plan.as_ref().and_then(|p| p.pic(k));
        if let Some(p) = pose {
            *posed.entry(p).or_default() += 1;
        }
        let up = match pose {
            Some(_) => pops.at(k),
            None => Vec::new(),
        };
        let key = Key {
            card: f.card,
            secs: f.clock.map(|t| t / 1000),
            speed: f.speed,
            jump: f.jump,
            ends: f.ends,
            caption: f.caption,
            chapter: f.chapter,
            screen,
            region,
            cross: share,
            pose,
            pops: up,
            blank,
            label: f.label,
            picture: f.picture,
            route: f.route,
        };
        // (drawn where it differs from the frame before — with `--picks`, only where the sheet picks it or a crossing
        // begins on it, else kept to be drawn if a crossing begins from it: either way from the frame it was first met
        // on, as the whole pass draws it)
        let still = stills.binary_search(&k).is_ok();
        let needed = !only_picks || picks.contains_key(&k) || crossings.fresh() || still;
        let s = if f.ends { ends.screen() } else { replay.screen() };
        match &mut last {
            Some((was, held @ None, first, under)) if *was == key && needed => {
                let (mut px, fresh) = paint(first, was, s, (&mut pics, &mut boards), &mut words);
                *under = stage.finish(&mut px, mascot_of(was, &pops), share, &mut crossings, &mut words);
                (drawn, painted) = (drawn + fresh, painted + 1);
                *held = Some(px);
            }
            Some((was, ..)) if *was == key => {}
            _ if needed => {
                let (mut px, fresh) = paint(&f, &key, s, (&mut pics, &mut boards), &mut words);
                let under = stage.finish(&mut px, mascot_of(&key, &pops), share, &mut crossings, &mut words);
                (drawn, painted) = (drawn + fresh, painted + 1);
                differ += u64::from(last.as_ref().is_none_or(|l| l.1.as_ref() != Some(&px)));
                last = Some((key, Some(px), f, under));
            }
            _ => last = Some((key, None, f, None)),
        }
        let px = last.as_ref().and_then(|l| l.1.as_deref());
        if let (Some(w), Some(px)) = (stdin.as_mut(), px)
            && wrote.is_ok()
        {
            wrote = w.write_all(px);
        }
        if still && let Some(px) = px {
            shot::save_png(&out.join(format!("frame-{k:05}.png")), dims.0, dims.1, px)?;
        }
        if let Some(marks) = picks.get(&k) {
            let px = px.expect("a frame the sheet picks is drawn");
            hashes.push((k, v, fnv1a(px)));
            let label = label(&prog, v, &f, region != all, marks);
            let cols = W / 2 / shot::cell(&sheet_fonts, 15.0).0;
            sheet.push(crate::recording::labelled(sheet_picture(px, &lay), &label, cols, &sheet_fonts));
            if sheet.len() == PAGE {
                pages += 1;
                crate::recording::grid(&sheet, &out.join(format!("sheet-{pages:02}.png")))?;
                sheet.clear();
            }
        }
    }
    if let Some(e) = boards.failed.take() {
        return Err(format!("a picture's files, read for its frames: {e}"));
    }
    if !cut.pictures.is_empty() || cut.opening.picture.is_some() {
        said.push(format!(
            "the pictures' boards: {} steps read at {}, the frames waiting {:.1} s for them",
            boards.read,
            scale.name(),
            boards.waited.as_secs_f64()
        ));
        // (where each is in the video: its part's first frame, each step's)
        for p in &prog.parts {
            let What::Picture(n) = p.what else { continue };
            let first = Program::time_of(Program::frame_at(p.at));
            let steps: Vec<String> =
                (cut.pictures[n].starts().iter()).map(|s| Program::frame_at(first + s).to_string()).collect();
            said.push(format!(
                "  picture {} in the video: from {} (frame {}), its steps from frames {}",
                n + 1,
                video_time(p.at),
                Program::frame_at(p.at),
                steps.join(", ")
            ));
        }
    }
    said.extend(route_said(&prog));
    if let (Some((art, place)), Some(plan)) = (mascot, &plan) {
        let (bw, bh) = mascot::box_at(scale);
        let of = |s: mascot::Side| {
            let stamps = taps.iter().filter(|t| t.1 == s && t.2 == mascot::Press::Stamp).count();
            let all = taps.iter().filter(|t| t.1 == s).count();
            match stamps {
                0 => format!("{all}"),
                1 => format!("{all} (1 stamp)"),
                n => format!("{all} ({n} stamps)"),
            }
        };
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
        let n = |p: Pic| posed.get(&p).copied().unwrap_or(0);
        let face = |f: mascot::Face| {
            posed.iter().filter(|(p, _)| matches!(p, Pic::At(_, x) if *x == f)).map(|e| e.1).sum::<u64>()
        };
        let body = |k: fn(&Pic) -> bool| posed.iter().filter(|(p, _)| k(p)).map(|e| e.1).sum::<u64>();
        let open = |a: mascot::Arms| n(Pic::At(a, mascot::Face::Open));
        let cut_ink = match art.cut_ink() {
            v if v.is_empty() => "no picture's ink reaches its box's edge".to_string(),
            v => format!(
                "the box cuts ink of {}",
                v.iter().map(|(n, k)| format!("{n} ({k})")).collect::<Vec<_>>().join("; ")
            ),
        };
        said.push(format!(
            "the mascot at the keys ({}): {} taps — {} left, {} right (returns too), {} on the space bar; a stamp's paw \
             held {} ms{}; on {} \
             frames: up {}, left {}, right {}, both {}, watching {}, the face half-turned {}, closed {}, raised over typing {}, \
             wide {}, surprised {}, stretching {}, swinging {}, winding the skein {}; its box {}×{} at ({}, {}), the rule stopping at x {}, the \
             marks ending at x {}{}; {} of {} pictures loaded, {cut_ink}; it cost {:.0} ms to load and shrink, {:.0} ms to lay \
             out its motions and pops, {:.0} ms to stamp",
            art.dir.display(),
            hands.taps(),
            of(mascot::Side::Left),
            of(mascot::Side::Right),
            of(mascot::Side::Space),
            mascot::STAMP_HOLD_MS,
            match stamps_keyless {
                0 => String::new(),
                n => format!(", {n} with no key heard no tap"),
            },
            posed.values().sum::<u64>(),
            open(mascot::Arms::Up),
            open(mascot::Arms::Left),
            open(mascot::Arms::Right),
            open(mascot::Arms::Both),
            n(Pic::WATCH),
            face(mascot::Face::Half),
            face(mascot::Face::Closed),
            face(mascot::Face::Raised) - n(Pic::WATCH),
            face(mascot::Face::Wide),
            body(|p| matches!(p, Pic::Surprise(_))),
            body(|p| matches!(p, Pic::Stretch(_))),
            body(|p| matches!(p, Pic::Swing(_))),
            body(|p| matches!(p, Pic::Wind(_) | Pic::WindIn)),
            bw,
            bh,
            place.x,
            place.y,
            place.rule_end,
            marks_right - lay.pad,
            place.lane.map_or(String::new(), |(l, r)| format!(", the pops' lane x {l} to {r}")),
            art.pics.iter().flatten().count(),
            Pic::COUNT,
            ms(loaded),
            ms(planned),
            ms(stage.stamping.get()),
        ));
        if cut.mascot.swing {
            said.push(art.v_said(place.lane.is_some()));
        }
        said.extend(plan.said(&cut.mascot));
        said.push(match cut.mascot.keys {
            mascot::Keys::Off => "the key pops: none (keys = false)".to_string(),
            k => format!(
                "the key pops (keys = {}): {} — {} commands, {} typed, {} a held key's count, {} fast-forward presses (▸▸); \
                 {} spaces in typed text pop nothing, no key on a sped span; the names drawn: {}",
                if k == mascot::Keys::All { "\"all\"" } else { "\"commands\"" },
                pops.list.len(),
                pops.commands,
                pops.typed,
                pops.held,
                pops.presses,
                pops.spaces,
                pops.names.join(" ")
            ),
        });
    }
    if !sheet.is_empty() {
        pages += 1;
        crate::recording::grid(&sheet, &out.join(format!("sheet-{pages:02}.png")))?;
    }
    let tsv: String = std::iter::once("frame\tvideo_us\tfnv1a64\n".to_string())
        .chain(hashes.iter().map(|(k, v, h)| format!("{k}\t{v}\t{h:016x}\n")))
        .collect();
    let p = out.join("picks.tsv");
    std::fs::write(&p, tsv).map_err(|e| format!("{}: {e}", p.display()))?;
    // (pages an earlier, longer cut left: gone)
    for k in pages + 1.. {
        if std::fs::remove_file(out.join(format!("sheet-{k:02}.png"))).is_err() {
            break;
        }
    }
    drop(stdin);
    let encoded = match child {
        None if made != Made::Video => Ok(()),
        None => Err(format!(
            "no ffmpeg on PATH nor at {}: the sheet is written, the MP4 isn't",
            video::FFMPEG_AT.join(" or ")
        )),
        Some(child) => {
            let o = child.wait_with_output().map_err(|e| format!("ffmpeg: {e}"))?;
            let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
            match (o.status.success(), wrote) {
                (true, Ok(())) => Ok(()),
                (_, Err(e)) => Err(format!("ffmpeg stopped taking frames ({e}): {err}")),
                (false, _) => Err(format!("ffmpeg: {} — {err}", o.status)),
            }
        }
    };
    let (chapters, warned) = prog.chapters();
    let p = out.join("chapters.txt");
    std::fs::write(&p, &chapters).map_err(|e| format!("{}: {e}", p.display()))?;
    said.extend(warned);

    let s = |us: u64| us as f64 / 1e6;
    let mut head = vec![match only_picks {
        false => format!(
            "{}: {:.1} s of video from {} of take ({}), glyphs {glyph} px; {frames} frames ({differ} differ), {drawn} \
             screens drawn; the sheet {} pictures in {pages} pages",
            mp4.display(),
            s(prog.total),
            clock(cut.end),
            cut.file,
            picks.len(),
        ),
        true => format!(
            "{}: the sheet's frames only (--picks), no MP4 and no sound — {:.1} s of video from {} of take ({}), glyphs \
             {glyph} px; {frames} frames, {painted} drawn (those picked, and those a crossing begins from or on), \
             {drawn} screens drawn; the sheet {} pictures in {pages} pages",
            out.join("picks.tsv").display(),
            s(prog.total),
            clock(cut.end),
            cut.file,
            picks.len(),
        ),
    }];
    head.extend(spans_said(cut));
    head.push(prog.jumps_said());
    // (the masks, each with the frames it covered — one that matched none loudly —; then every place `record check`
    // found that the cut shows, strong or weak, but those the masks hid wherever a frame showed them: said, never hidden)
    head.extend(masks.said(&cut.file));
    for l in masks.loud(&cut.file) {
        eprintln!("cut: {l}");
    }
    head.extend(super::check::said(&prog, &hits, &masks.hidden(), source));
    head.push(format!("chapters.txt:\n{}", chapters.trim_end()));
    head.extend(said);
    if let (Some(mut heard), Some(f)) = (heard, &ffmpeg)
        && encoded.is_ok()
    {
        let enc = match video::loudness(f, &mp4) {
            Ok((l, tp)) => format!("{l:.1} LUFS integrated, {tp:.1} dBTP"),
            Err(e) => format!("not measured ({e})"),
        };
        heard.insert(1, format!("  as encoded (the MP4's AAC, ffmpeg's ebur128): {enc}"));
        head.extend(heard);
    }
    head.push(cost(started));
    if !lost.is_empty() {
        head.push(format!(
            "zooms whose target leaves the screen before the camera starts back ({}):",
            lost.len()
        ));
        head.extend(lost.iter().map(|l| format!("  {l}")));
    }
    encoded?;
    Ok(head.join("\n") + "\n")
}

/// The video laid out — by `lay`: the keys heard, the key pops and the mascot's plan, for a program — and, where a
/// chapter's end had no room for its swing (`Plan::holds`), laid again with each such end held still before its card
/// (`Program::held`): the program, what `lay` gave for it, and `cut.txt`'s lines of the ends held — each one's screen
/// standing or what moves on it (`moves`: for each held frame's `Program::stand_window`, what moves there, `moving`),
/// and each that moves a place in the edit list, the line of the chapter whose card follows (where it ends: the
/// `[opening]`'s): held, it reads as a frozen video, and the chapter's end goes where the screen stands.
pub(crate) fn held_ends<'c, T>(
    cut: &'c Cut,
    lay: impl Fn(&Program<'c>) -> (T, Option<Plan>),
    moves: impl FnOnce(&[(u64, u64)]) -> Vec<Option<String>>,
) -> (Program<'c>, T, Option<Plan>, Vec<String>) {
    let prog = Program::new(cut);
    let (got, plan) = lay(&prog);
    let holds = plan.as_ref().map_or(Vec::new(), |p| p.holds.clone());
    if holds.is_empty() {
        return (prog, got, plan, Vec::new());
    }
    let prog = Program::held(cut, &holds);
    let (got, plan) = lay(&prog);
    let more: u64 = holds.iter().map(|h| h.us()).sum();
    // (each held end: its part, the chapter whose card follows, the frame held)
    let held: Vec<(&super::Part, usize, u64)> = (prog.parts.iter().enumerate())
        .filter_map(|(i, p)| match (p.what, prog.parts.get(i + 1).map(|q| q.what)) {
            (What::Hold { v }, Some(What::Chapter(n))) => Some((p, n, v)),
            _ => None,
        })
        .collect();
    let windows: Vec<(u64, u64)> = held.iter().map(|h| prog.stand_window(h.2).unwrap_or((0, 0))).collect();
    let moved = moves(&windows);
    let standing = moved.iter().filter(|m| m.is_none()).count();
    let mut said = vec![format!(
        "chapter ends held still for their swing (no room for it before the card): {}, {:.1} s more video; {standing} on \
         a screen that stands, {} on one that moves",
        holds.len(),
        more as f64 / 1e6,
        held.len() - standing
    )];
    let mut places = Vec::new();
    for (&(p, n, v), m) in held.iter().zip(&moved) {
        let f = prog.frame(v);
        let (what, line) = match (f.ends, n) {
            (true, _) => ("where it ends".to_string(), cut.opening.line_at),
            (false, 0) => ("the take before chapter 1".to_string(), cut.chapters[0].line),
            (false, n) => (format!("chapter {n}'s end"), cut.chapters[n].line),
        };
        let how = m.as_ref().map_or("stands".to_string(), |m| format!("moves — {m}"));
        let at = format!("held {:.1} s at {}", p.len as f64 / 1e6, video_time(p.at));
        said.push(format!(
            "  {what}, before chapter {}'s card: {at} (take {}) — {how}",
            n + 1,
            f.clock.map_or("?".into(), clock)
        ));
        if let Some(m) = m {
            places.push(format!(
                "{}:{line}: {what} is {at} on a screen that moves ({m}): held, it reads as a frozen video — end the \
                 chapter where its screen stands",
                cut.file
            ));
        }
    }
    said.extend(places);
    // (a hold leaves room by its making; were any still wanting one, said)
    for h in plan.as_ref().map_or(&[][..], |p| &p.holds[..]) {
        said.push(format!("  chapter {}'s end: still no room for its swing, held or not", h.chapter + 1));
    }
    (prog, got, plan, said)
}

/// A frame as the sheet shows it: 1080p's halved, whatever the picture's size (the sheet stays small).
fn sheet_picture(px: &[u8], lay: &Layout) -> Picture {
    let mut p = video::half(px, lay.dims);
    while p.0 > W / 2 {
        p = video::half(&p.2, (p.0, p.1));
    }
    p
}

/// The width of `t`'s ink at `px` (1080p's), its picture's padding left out.
pub(crate) fn letters(t: &str, px: f32, fonts: &Fonts) -> usize {
    video::text(t, px, [255; 3], [0; 3], false, fonts).0.saturating_sub(2 * PAD)
}

/// The width a caption centred in the band has (px at 1080p) with the marks' pictures ending at `right`: to that end,
/// or short of `mark` (the widest speed mark shown over it) by `mascot::GAP` — and as much again on its left.
pub(crate) fn caption_room(fonts: &Fonts, right: usize, mark: Option<&str>) -> usize {
    let end = right - PAD;
    let limit = mark.map_or(end, |t| end.saturating_sub(letters(t, MARK_PX, fonts) + mascot::GAP));
    (2 * limit).saturating_sub(W)
}

/// Each caption clear of the band's right end (the marks' pictures ending at `right`; `boxed`: the mascot's box after
/// them): of the widest speed mark shown over its stretch by `mascot::GAP`, else of that end (`caption_room`). Checked
/// before a frame is drawn; every one that would reach it is refused at once, each with its line and the room there.
pub(crate) fn captions_clear(cut: &Cut, fonts: &Fonts, (right, boxed): (usize, bool)) -> Result<()> {
    let map = &cut.map;
    let letters = |t: &str, px: f32| letters(t, px, fonts);
    let mut refused = Vec::new();
    for c in &cut.captions {
        let (m0, m1) = (c.m, c.end());
        let widest = (0..map.segs().len())
            .filter(|&i| map.seg_at(i) < m1 && m0 < map.seg_at(i) + map.seg_len(i))
            .filter_map(|i| match map.segs()[i].pace {
                Pace::Speed(k) => sped(k),
                Pace::Out => None,
            })
            .map(|t| (letters(&t, MARK_PX), t))
            .max();
        let room = caption_room(fonts, right, widest.as_ref().map(|(_, t)| t.as_str()));
        let w = letters(&c.text, video::CAPTION_PX);
        if w > room {
            let what = match (&widest, boxed) {
                (Some((_, t)), _) => format!("the speed's mark `{t}`"),
                (None, true) => "the mascot's box".to_string(),
                (None, false) => "the screen's right edge".to_string(),
            };
            refused.push(format!(
                "{}:{}: caption: {w} px wide, centred it reaches {what} — {room} px left there (about {} characters)",
                cut.file,
                c.line,
                room / shot::cell(fonts, video::CAPTION_PX).0.max(1)
            ));
        }
    }
    // (a picture's lines: no speed's mark stands under a picture)
    let room = caption_room(fonts, right, None);
    for (p, s) in cut.pictures.iter().flat_map(|p| p.steps.iter().map(move |s| (p, s))) {
        let k = p.steps.iter().position(|x| std::ptr::eq(x, s)).unwrap_or(0);
        for (j, t) in s.text.iter().enumerate() {
            let w = letters(t, video::CAPTION_PX);
            if w > room {
                let what = if boxed { "the mascot's box" } else { "the screen's right edge" };
                refused.push(format!(
                    "{}:{}: picture, step {} ({}), line {}: {w} px wide, centred it reaches {what} — {room} px left there",
                    cut.file,
                    s.line,
                    k + 1,
                    s.name,
                    j + 1
                ));
            }
        }
    }
    match refused.len() {
        0 => Ok(()),
        1 => Err(refused.remove(0)),
        n => Err(format!("{n} captions don't fit the band:\n  {}", refused.join("\n  "))),
    }
}

/// `on` looked for on `scan`'s screen from the take's `at` until `until` (ms): when it's first there, and its region.
fn find(scan: &mut Replay, at: u64, until: u64, on: &str) -> (u64, Option<Region>) {
    scan.to(at);
    let mut found = camera::target(&Cells::of(scan.screen()), on);
    let mut when = at;
    while found.is_none() {
        match scan.next() {
            Some(ms) if ms <= until => {
                found = camera::target(&Cells::of(scan.screen()), on);
                when = ms;
            }
            _ => break,
        }
    }
    (when, found)
}

/// The take part's camera reads (the map's ms: from, to, window), one for each zoom (`windows`, the zooms' order).
pub(crate) fn take_reads(cut: &Cut, windows: &[Region]) -> Vec<(u64, u64, Region)> {
    (cut.zooms.iter().zip(windows))
        .map(|(z, &w)| {
            let m = cut.map.video(z.at) / 1000;
            (m, m + camera::DRIFT_MS + z.len, w)
        })
        .collect()
}

/// The camera's two paths: the take part's, on the map's ms, from each `[[zoom]]`'s window (`windows`); where it
/// ends', on its own ms from its start, from the opening's zoom (`ends`: where its target is first there, its window) —
/// eased in from then and held to that stretch's end, a crossing through the ground after it.
pub(crate) fn paths(
    cut: &Cut,
    windows: &[Region],
    ends: Option<(u64, Region)>,
    size: (u16, u16),
) -> (camera::Path, camera::Path) {
    let reads = take_reads(cut, windows);
    let take =
        camera::Path::of(&camera::aims(&reads, size), size, cut.map.len() / 1000 + 2 * camera::DRIFT_MS);
    let o = &cut.opening;
    let reads: Vec<_> = ends.into_iter().map(|(from, w)| (from, o.len.max(from + 1), w)).collect();
    let ends = camera::Path::of(&camera::aims(&reads, size), size, o.len + 2 * camera::DRIFT_MS);
    (take, ends)
}

/// Which of the camera's paths frame `f` is on and how far along it (µs, `Frame::camera`): where it ends, its own
/// (true); in the take part, the map's (false); None elsewhere (the whole).
fn on_path(f: &Frame) -> Option<(bool, u64)> {
    f.camera.map(|us| (f.ends, us))
}

/// The camera's region for frame `f`: where it ends, its own path's; in the take part, the map's; else the whole.
pub(crate) fn region(f: &Frame, paths: &(camera::Path, camera::Path), size: (u16, u16)) -> Region {
    match on_path(f) {
        Some((true, us)) => paths.1.at_us(us),
        Some((false, us)) => paths.0.at_us(us),
        None => Region::all(size),
    }
}

/// The glyph frame `f`'s screen picture is drawn at where its camera is off the whole (`Glyphs`, of each path).
pub(crate) fn drawn_at(f: &Frame, glyphs: &(Glyphs, Glyphs)) -> Option<f32> {
    match on_path(f)? {
        (true, us) => glyphs.1.at(us),
        (false, us) => glyphs.0.at(us),
    }
}

/// Each zoom whose target isn't on the screen for its whole length — from where it was found (`found_at`, the take's
/// ms) until the camera starts back (`reads`' end, the map's ms) —, checked at every frame: the file's line, the video
/// time it's gone and how long before the camera leaves. Said, never refused: the file's to tune.
pub(crate) fn lost_zooms(
    cut: &Cut,
    take: &Take,
    prog: &Program,
    reads: &[(u64, u64, Region)],
    found_at: &[u64],
) -> Vec<String> {
    let mut out = Vec::new();
    let mut chk = Replay::new(&take.screen, take.size);
    let step = 1_000_000 / video::FPS;
    for ((z, &(m0, m1, _)), &found) in cut.zooms.iter().zip(reads).zip(found_at) {
        let mut v = (m0 * 1000).max(cut.map.video(found));
        let mut first = true;
        while v < m1 * 1000 {
            let (t, _) = cut.map.take(v);
            let came = chk.to(t);
            if (came || first) && camera::target(&Cells::of(chk.screen()), &z.on).is_none() {
                out.push(format!(
                    "{}:{}: zoom on \"{}\": gone at {} in the video (take {}), {:.1} s before the camera starts back",
                    cut.file,
                    z.line,
                    z.on,
                    prog.video_of(v).map_or("?".into(), video_time),
                    super::clock_tenths(t),
                    (m1 * 1000 - v) as f64 / 1e6
                ));
                break;
            }
            first = false;
            v += step;
        }
    }
    out
}

/// `to` drawn `a` of 255 of the way over `from` (a crossing).
fn crossed(from: &[u8], to: &mut [u8], a: u8) {
    let a = u32::from(a);
    for (d, s) in to.iter_mut().zip(from) {
        *d = ((u32::from(*s) * (255 - a) + u32::from(*d) * a + 127) / 255) as u8;
    }
}

/// The crossings as the frames meet them, in order — the program's parts and jumps (`Program::crossings`) and the take's
/// whole-screen changes (`changes`) —: the one in force, from the picture it leaves.
pub(crate) struct Crossings {
    parts: Vec<(u64, bool)>,
    changes: Vec<u64>,
    next: (usize, usize),
    cross: Option<(Vec<u8>, u64, bool)>,
    /// Whether the one in force began at this frame.
    fresh: bool,
    /// The parts' crossings that cross the board alone (`Program::confined`), and the board's bytes; whether the one
    /// in force is one.
    board: (Vec<u64>, usize, bool),
}

impl Crossings {
    pub(crate) fn new(parts: Vec<(u64, bool)>, changes: Vec<u64>) -> Crossings {
        Crossings { parts, changes, next: (0, 0), cross: None, fresh: false, board: (Vec::new(), 0, false) }
    }

    /// The same, but those of the parts' at `at` crossing only a frame's first `bytes` (the board: into and out of a
    /// picture the band never dips).
    pub(crate) fn confined(self, at: Vec<u64>, bytes: usize) -> Crossings {
        Crossings { board: (at, bytes, false), ..self }
    }

    /// Whether a crossing begins by frame `v` that `at` hasn't met yet: `at(v, …)` will take the frame before.
    pub(crate) fn begins(&self, v: u64) -> bool {
        self.changes.get(self.next.1).is_some_and(|&c| c <= v)
            || self.parts.get(self.next.0).is_some_and(|p| p.0 <= v)
    }

    /// Whether the crossing in force began at the frame `at` last met.
    pub(crate) fn fresh(&self) -> bool {
        self.fresh
    }

    /// Frame `v` comes: each crossing begun by then, from the frame before as drawn (`last`; none before the first); how
    /// far through the one in force it is (of 255, `cross_share`), None where none is.
    pub(crate) fn at(&mut self, v: u64, last: Option<&[u8]>) -> Option<u8> {
        self.fresh = false;
        while self.next.1 < self.changes.len() && self.changes[self.next.1] <= v {
            if let Some(px) = last {
                (self.cross, self.fresh) = (Some((px.to_vec(), self.changes[self.next.1], false)), true);
                self.board.2 = false;
            }
            self.next.1 += 1;
        }
        while self.next.0 < self.parts.len() && self.parts[self.next.0].0 <= v {
            if let Some(px) = last {
                let (at, through) = self.parts[self.next.0];
                (self.cross, self.fresh) = (Some((px.to_vec(), at, through)), true);
                self.board.2 = self.board.0.contains(&at);
            }
            self.next.0 += 1;
        }
        let share = self.cross.as_ref().map(|(_, began, _)| cross_share(v, *began));
        if share == Some(255) {
            self.cross = None;
        }
        share.filter(|&a| a < 255)
    }

    /// Whether the crossing in force (`share` of the way through, `at`'s) crosses the board alone.
    pub(crate) fn confines(&self, share: Option<u8>) -> bool {
        share.is_some() && self.cross.is_some() && self.board.2
    }

    /// The crossing begun at the frame `at` last met, where it crosses the board alone, leaves the frame before with its
    /// board's rows under the mascot as they were before it was laid (`under`): the character, laid after, isn't the
    /// board's.
    pub(crate) fn bare(&mut self, under: &Under) {
        if let (true, true, Some((from, ..))) = (self.fresh, self.board.2, self.cross.as_mut()) {
            from[under.at..under.at + under.bytes.len()].copy_from_slice(&under.bytes);
        }
    }

    /// Frame `px` as drawn, the crossing in force over it, `share` of the way through (`at`'s) — but none where the
    /// frame it begins on is the picture it leaves (of the board, where it crosses the board alone): a crossing between
    /// two identical pictures draws nothing (take 3's final2c at 10:11.7, the take's last screen into itself held: down
    /// to the ground and back, the band with it).
    pub(crate) fn over(&mut self, px: &mut [u8], ground: Rgb, share: Option<u8>) {
        let (Some(p), Some((from, _, through))) = (share, &self.cross) else { return };
        let same = match self.board {
            (_, bytes, true) if bytes <= px.len() => from[..bytes] == px[..bytes],
            _ => from.as_slice() == px,
        };
        if self.fresh && same {
            self.cross = None;
            return;
        }
        match self.board {
            (_, bytes, true) if bytes <= px.len() => {
                cross_px(&from[..bytes], &mut px[..bytes], ground, p, *through)
            }
            _ => cross_px(from, px, ground, p, *through),
        }
    }
}

/// How far through a crossing begun at `began` the video's `v` is (µs), of 255, linear (`cross_px` eases it).
pub(crate) fn cross_share(v: u64, began: u64) -> u8 {
    ((v.saturating_sub(began) as f32 / (camera::CROSS_MS * 1000) as f32).min(1.0) * 255.0).round() as u8
}

/// A crossing's frame `p` of 255 of the way through it (linear), from the picture `from` to `to` (drawn in `to`): the
/// two eased over each other, or (`through`) through the `ground` — `from` fading to it over the first half, `to`
/// coming up from it over the second, each eased: never a pixel of both.
pub(crate) fn cross_px(from: &[u8], to: &mut [u8], ground: Rgb, p: u8, through: bool) {
    let p = f32::from(p) / 255.0;
    if !through {
        return crossed(from, to, (camera::ease_in_out(p) * 255.0).round() as u8);
    }
    let g: Vec<u8> = ground.repeat(to.len() / 3);
    match p < 0.5 {
        true => {
            to.copy_from_slice(&g);
            crossed(from, to, (camera::ease_in_out(p * 2.0) * 255.0).round() as u8);
        }
        false => crossed(&g, to, (camera::ease_in_out(p * 2.0 - 1.0) * 255.0).round() as u8),
    }
}

/// The sheet's pictures: which frames, and what each is marked with (`sheet_marks`) — one every `SHEET_EVERY_US`
/// besides; each taken once its picture has come up, never inside a crossing (its label would name a part its picture
/// doesn't show yet).
pub(crate) fn sheet_picks(prog: &Program) -> BTreeMap<u64, Vec<String>> {
    let frames = prog.frames();
    let crossings = prog.crossings();
    let up = |mut v: u64| {
        while let Some(c) = crossings.iter().find(|c| c.0 <= v && v < c.0 + camera::CROSS_MS * 1000) {
            v = c.0 + camera::CROSS_MS * 1000;
        }
        Program::frame_at(v).min(frames - 1)
    };
    let mut picks: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    let every = SHEET_EVERY_US.max(prog.total / SHEET_REGULAR);
    for v in (0..prog.total).step_by(every as usize) {
        picks.entry(up(v)).or_default();
    }
    for (v, mark) in sheet_marks(prog) {
        picks.entry(up(v)).or_default().push(mark);
    }
    picks
}

/// The band under the screen (its picture at `x0`), laid as `lay` says: the take's clock at the left with the jump's
/// mark after it, the speed's mark (or `where it ends`) at the right — its picture ending at `right`: the screen's, or
/// `mascot::GAP` short of the mascot's box —, the caption in the middle; each centred on the band's height.
pub(crate) fn band(
    px: &mut [u8],
    lay: &Layout,
    f: &Frame,
    cut: &Cut,
    (x0, right): (usize, usize),
    look: &video::Look,
    words: &mut Words,
) {
    let (mid, dims, w, s) = (lay.mid, lay.dims, lay.dims.0, lay.scale);
    let mut put = |p: &Picture, x: usize, a: u8| video::blit(px, dims, p, x, mid - s.half(p.1), a);
    let mut left = x0;
    if let Some(t) = f.clock {
        let p = words.get(&clock(t), MARK_PX, look.ink);
        left += p.0;
        put(p, x0, QUIET);
    }
    if let Some((ms, a)) = f.jump {
        let (fg, a) = jump_ink(ms, a, look);
        let p = words.get(&jumped(ms), MARK_PX, fg);
        put(p, left, a);
    }
    let mark = match (f.ends, f.speed.and_then(sped)) {
        // (the words over where it ends say it: one label for one thing)
        (true, _) if f.label.is_some() => None,
        (true, _) => Some((ENDS.to_string(), look.accent, 255)),
        (false, Some(s)) => Some((s, look.ink, QUIET)),
        (false, None) => None,
    };
    if let Some((t, fg, a)) = mark {
        let p = words.get(&t, MARK_PX, fg);
        put(p, right.saturating_sub(p.0), a);
    }
    if let Some((i, a)) = f.caption {
        let p = words.get(&cut.captions[i].text, video::CAPTION_PX, look.ink);
        put(p, s.centre(w, p.0.min(w)), a);
    }
    // (a picture's step's line, as a caption)
    if let Some(picture::Shows { n: Some(n), k, line: Some((j, a)), .. }) = f.picture {
        let p = words.get(&cut.pictures[n].steps[k].text[j], video::CAPTION_PX, look.ink);
        put(p, s.centre(w, p.0.min(w)), a);
    }
    // (a chapter with no card: its number, faint, then its title, centred as a caption is)
    if let Some((i, a)) = f.chapter {
        let n = words.get(&format!("{}   ", i + 1), video::CAPTION_PX, look.faint).clone();
        let p = words.get(&cut.chapters[i].title, video::CAPTION_PX, look.ink);
        let x = s.centre(w, (n.0 + p.0).min(w));
        put(&n, x, a);
        put(p, x + n.0, a);
    }
}

/// A jump's mark of `ms` left out, `a`/255 into its fade: its ink and strength, as loud as the jump is far — the accent
/// at full from `JUMP_ACCENT_MS`, under it the clock's quiet ink (`QUIET`; under `JUMP_QUIET_MS` the frame has none).
pub(crate) fn jump_ink(ms: u64, a: u8, look: &video::Look) -> (Rgb, u8) {
    match weight(ms) {
        Weight::Accent => (look.accent, a),
        _ => (look.ink, (u32::from(a) * u32::from(QUIET) / 255) as u8),
    }
}

/// Where the sheet looks besides its regular pictures (video µs, what's there): each part, caption, zoom, jump and
/// change of speed.
pub(crate) fn sheet_marks(prog: &Program) -> Vec<(u64, String)> {
    let cut = prog.cut;
    let map = &cut.map;
    let mut out = Vec::new();
    for p in &prog.parts {
        match p.what {
            What::Intro => out.push((p.at + p.len * 3 / 4, "intro".to_string())),
            What::Built => out.push((p.at + p.len / 2, "what's built".into())),
            What::Ends { .. } => out.push((p.at + p.len / 2, ENDS.into())),
            What::Chapter(i) => {
                out.push((p.at + p.len / 2, format!("chapter {}: “{}”", i + 1, cut.chapters[i].title)))
            }
            What::Hold { .. } => out.push((p.at + p.len / 2, "held for the swing".into())),
            // (each step twice: half through its strokes, and whole — its last frame)
            What::Picture(n) => {
                let pic = &cut.pictures[n];
                let first = Program::time_of(Program::frame_at(p.at));
                for (k, (s, start)) in pic.steps.iter().zip(pic.starts()).enumerate() {
                    let what = format!("picture {}, step {} ({})", n + 1, k + 1, s.name);
                    let half = (s.halfway() as f64 / s.speed).round() as u64;
                    out.push((first + start + half, format!("{what}: drawing")));
                    let last = (first + start + s.len()).saturating_sub(1_000_000 / video::FPS + 1);
                    out.push((last, format!("{what}: whole")));
                }
            }
            What::Last => out.push((p.at, "the last screen held".into())),
            What::End => out.push((p.at + p.len / 2, "end card".into())),
            What::Take { .. } => {}
        }
    }
    if let Some(r) = &cut.route {
        for (k, (name, &v)) in r.stations.iter().zip(&prog.inks).enumerate() {
            out.push((v, format!("station {} inked: “{name}”", k + 1)));
        }
    }
    for c in &cut.captions {
        if let Some(v) = prog.video_of(c.m) {
            out.push((v + super::FADE_MS * 1000, format!("caption “{}”", c.text)));
        }
    }
    for (i, c) in cut.chapters.iter().enumerate().filter(|(_, c)| !c.card) {
        let v = prog.chapter_at(i);
        out.push((v + super::FADE_MS * 1000, format!("chapter {} in the band: “{}”", i + 1, c.title)));
    }
    for z in &cut.zooms {
        if let Some(v) = prog.video_of(map.video(z.at)) {
            out.push((v + camera::DRIFT_MS * 1000, format!("zoom on {}", z.on)));
        }
    }
    for &(m, ms) in &prog.jumps {
        if let Some(v) = prog.video_of(m) {
            let w = match weight(ms) {
                Weight::Unmarked => ", unmarked",
                Weight::Quiet => ", quiet",
                Weight::Accent => "",
            };
            out.push((v + camera::CROSS_MS * 1000, format!("jump {}{w}", jumped(ms))));
        }
    }
    let mut was = None;
    for (i, s) in map.segs().iter().enumerate() {
        if s.pace == Pace::Out || was == Some(s.pace) {
            continue;
        }
        if was.is_some()
            && let Some(v) = prog.video_of(map.seg_at(i))
        {
            out.push((v, format!("now {}", pace_said(s.pace))));
        }
        was = Some(s.pace);
    }
    out.sort_by_key(|m| m.0);
    out
}

fn pace_said(p: Pace) -> String {
    match p {
        Pace::Speed(k) if k <= 1.0 + 1e-6 => "1×".into(),
        Pace::Speed(k) => format!("×{}", factor(k)),
        Pace::Out => "left out".into(),
    }
}

/// A sheet picture's words: its time in the video; the take's time and the speed shown (or the card); a zoom; its marks.
pub(crate) fn label(prog: &Program, v: u64, f: &Frame, zoomed: bool, marks: &[String]) -> String {
    let mut said = vec![video_time(v)];
    match (f.card, f.clock) {
        (Some(_), _) => {}
        (None, Some(t)) => {
            said.push(format!("take {}", clock(t)));
            said.push(match (f.ends, f.speed) {
                (true, _) => format!("1× {ENDS}"),
                (false, Some(k)) => format!("×{}", factor(k)),
                (false, None) => match prog.part(v).what {
                    What::Last => "held".into(),
                    What::Hold { .. } => "held for the swing".into(),
                    What::Picture(_) => "stopped".into(),
                    _ => "1×".into(),
                },
            });
        }
        (None, None) => {}
    }
    if zoomed {
        said.push("zoomed".into());
    }
    said.extend(marks.iter().cloned());
    said.join("  ")
}

/// The cut's stretches said: each with its take time, its pace and its video length; then the totals by pace.
fn spans_said(cut: &Cut) -> Vec<String> {
    let map = &cut.map;
    let s = |ms: u64| ms as f64 / 1e3;
    let mut out = Vec::new();
    let (mut one, mut fast, mut gone) = ((0u64, 0u64), (0u64, 0u64), 0u64);
    for (i, seg) in map.segs().iter().enumerate() {
        let v = map.seg_len(i);
        let len = seg.to - seg.from;
        match seg.pace {
            Pace::Out => gone += len,
            Pace::Speed(k) if k <= 1.0 + 1e-6 => one = (one.0 + len, one.1 + v),
            Pace::Speed(_) => fast = (fast.0 + len, fast.1 + v),
        }
        out.push(format!(
            "  {}–{}  {:<9} {:>7.1} s → {:>6.1} s{}",
            super::clock_tenths(seg.from),
            super::clock_tenths(seg.to),
            pace_said(seg.pace),
            s(len),
            v as f64 / 1e6,
            seg.line.map_or(" (the default)".to_string(), |l| format!(" (line {l})"))
        ));
    }
    out.insert(
        0,
        format!(
            "spans: at 1× {:.1} s of take → {:.1} s; sped {:.1} s → {:.1} s; left out {:.1} s",
            s(one.0),
            one.1 as f64 / 1e6,
            s(fast.0),
            fast.1 as f64 / 1e6,
            s(gone)
        ),
    );
    out
}

/// `cut.txt`'s route: its stations on the rule, each inked where (the picture, its step, the video's time and frame).
fn route_said(prog: &Program) -> Vec<String> {
    let cut = prog.cut;
    let Some(r) = &cut.route else { return Vec::new() };
    let mut out = vec![format!(
        "the route: {} stations on the band's rule, a {} px dot each (at 1080p) — the part travelled in ink, {} px \
         thick, the rest faint, 1 px; the station being worked on in the accent; each inked on its picture's last stroke",
        r.stations.len(),
        route::DOT_PX,
        route::TRAVELLED_PX
    )];
    for (k, ((name, &n), &v)) in r.stations.iter().zip(&r.pictures).zip(&prog.inks).enumerate() {
        let step = route::last_stroke(&cut.pictures[n]).map_or(0, |s| s.0);
        out.push(format!(
            "  station {} “{name}”: picture {}, step {} ({}), at {} (frame {})",
            k + 1,
            n + 1,
            step + 1,
            cut.pictures[n].steps[step].name,
            video_time(v),
            Program::frame_at(v),
        ));
    }
    if let Some(n) = cut.pictures.len().checked_sub(r.pictures.len()).filter(|&n| n > 0) {
        out.push(format!("  {n} picture{} ink no station", if n == 1 { "" } else { "s" }));
    }
    out
}

/// `cut.txt`'s lines of the pictures: how many, their steps' drawing and standing, that `record check` doesn't scan
/// them, the character painting or watching (`painter`); each step with what its files said (`found`,
/// `picture::check`'s, which took `took`); the opening's picture.
fn pictures_said(
    cut: &Cut,
    found: &[Vec<picture::Found>],
    took: std::time::Duration,
    painter: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    let steps = || cut.pictures.iter().flat_map(|p| &p.steps);
    if !cut.pictures.is_empty() {
        let s = |us: u64| us as f64 / 1e6;
        out.push(format!(
            "pictures: {}, {} steps — {:.1} s drawn, {:.1} s standing, {:.1} s of video with the crossings in; not \
             scanned by `record check` (no cells); {} strokes, {} fades; {painter}; checked at 2160p in {:.1} s",
            cut.pictures.len(),
            steps().count(),
            s(steps().map(|x| x.drawing()).sum()),
            s(steps().map(|x| x.standing()).sum()),
            s(cut.pictures.iter().map(|p| p.len()).sum()),
            steps().map(|x| x.strokes.len()).sum::<usize>(),
            steps().map(|x| x.fades.len()).sum::<usize>(),
            took.as_secs_f64()
        ));
    }
    let pct = |(a, b): (usize, usize)| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
    for (n, (p, found)) in cut.pictures.iter().zip(found).enumerate() {
        for (k, (s, f)) in p.steps.iter().zip(found).enumerate() {
            out.push(format!(
                "  picture {} at {} (line {}), step {} ({}): drawn {:.1} s{}, standing {:.1} s; its layers miss its \
                 picture on {} px; what came at the end: ink {} of {} px ({:.2} %), wash {} of {} px ({:.2} %)",
                n + 1,
                clock(p.at),
                p.line,
                k + 1,
                s.name,
                s.drawing() as f64 / 1e6,
                if s.speed == 1.0 { String::new() } else { format!(" (×{})", s.speed) },
                s.standing() as f64 / 1e6,
                f.missed,
                f.ink.0,
                f.ink.1,
                pct(f.ink),
                f.wash.0,
                f.wash.1,
                pct(f.wash),
            ));
        }
    }
    if let Some((path, _)) = &cut.opening.picture {
        out.push(format!("where it ends: a picture, whole — {}", path.display()));
    }
    out
}

/// What the cut cost: its time, and the most memory it and ffmpeg held at once.
fn cost(started: Instant) -> String {
    let max_rss = |who: i32| {
        // SAFETY: getrusage fills the struct it's given; zeroed is a valid start.
        let mut u: libc::rusage = unsafe { std::mem::zeroed() };
        unsafe { libc::getrusage(who, &mut u) };
        // (macOS says bytes, Linux kilobytes)
        let k = if cfg!(target_os = "macos") { 1 } else { 1024 };
        u.ru_maxrss as u64 * k / (1024 * 1024)
    };
    format!(
        "took {:.1} s; at most {} MB held here, {} MB by ffmpeg",
        started.elapsed().as_secs_f64(),
        max_rss(libc::RUSAGE_SELF),
        max_rss(libc::RUSAGE_CHILDREN)
    )
}
