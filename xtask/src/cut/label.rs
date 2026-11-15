//! The words over where it ends (`[opening]`'s `label` and `label-line`; the user's pick of 2026-11-12, after the
//! design session's mocks in `.sb/seal/`): a title in the chapter cards' title style and a small line under it in the
//! band's caption size and quiet ink, centred on the opening's board, in with it and out before chapter 1's card. While
//! it shows the band says nothing of where it ends: one label for one thing.
//!
//! - **The panel**: the ground at `PANEL` under the words, only where the board has something there (any pixel of the
//!   words' box off the ground, the frame's own picture) — on an empty stretch of board the words stand alone.
//! - **The light** (`Shine`): one slanted band, tall enough for both lines, crossing the wider line's width left to
//!   right in `PASS_US`, a pass every `EVERY_US` from `LIGHT_FROM_US` into the board — the cadence fixed from the first
//!   pass, so the pass that meets the fade-out fades with the words, never stretched to end neatly. Inside the glyphs
//!   only: the title rests at `TITLE_REST` of its paper (the opening only) and lights to pure white, the small line
//!   from its own faint strength to the paper; one band-width behind the light the letters dip by `TRAIL`. No glow, no
//!   sound.

use super::draw::Layout;
use crate::recording::Picture;
use crate::shot::Rgb;
use crate::video::{self, Look};

/// The title: a chapter card's (`draw::CHAPTER_PX`, bold, the text's colour). The small line: a caption's size, in the
/// band's quiet ink (`draw::QUIET` of 255).
pub(crate) const TITLE_PX: f32 = super::draw::CHAPTER_PX;
pub(crate) const LINE_PX: f32 = video::CAPTION_PX;
const LINE_STRENGTH: f32 = super::draw::QUIET as f32 / 255.0;
/// The words come in and go out over this long (µs), with the board.
pub(crate) const FADE_US: u64 = 300_000;
/// The light: its first pass from this far into the board (the words fully up), each pass crossing in `PASS_US`, a
/// rest of `REST_US` between (the user's "a slightly slower b", 2026-11-12: 1.2 s, not the mocks' 0.9 s).
pub(crate) const LIGHT_FROM_US: u64 = 500_000;
pub(crate) const PASS_US: u64 = 1_200_000;
pub(crate) const REST_US: u64 = 500_000;
pub(crate) const EVERY_US: u64 = PASS_US + REST_US;
const _: () = assert!(FADE_US <= LIGHT_FROM_US, "the words fully up before the light's first pass");
/// The title at rest, the opening only: this share of its paper, every channel alike (never grey) — so the light's
/// white core shows above it, at a phone's size too (`.sb/seal`'s 400 px check: at 0.90 it didn't).
pub(crate) const TITLE_REST: f32 = 0.87;
/// The band: this many of the title's letters wide at half its height, leaning this many px across for each px up; the
/// letters this much darker one band-width behind it.
pub(crate) const BAND_LETTERS: f32 = 2.0;
pub(crate) const LEAN: f32 = 0.3;
pub(crate) const TRAIL: f32 = 0.15;
/// The panel: the ground at this share over the board; its air round the words and its corners (px at 1080p); a pixel
/// this far off the ground (of 255, any channel) is the board's.
pub(crate) const PANEL: f32 = 0.85;
const PANEL_AIR: (usize, usize) = (48, 28);
const PANEL_RADIUS: f32 = 14.0;
const BOARD_INK: i32 = 16;
const WHITE: Rgb = [255, 255, 255];

/// The label in one frame: how far it's drawn (0..=1, its fades) and where the light is in its pass (0..=1 across),
/// none between passes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Label {
    pub(crate) shown: f32,
    pub(crate) light: Option<f32>,
}

fn smooth(p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    p * p * (3.0 - 2.0 * p)
}

/// The label `off` µs into a board shown `len` µs: in and out over `FADE_US`; the light on its cadence from
/// `LIGHT_FROM_US`, whatever the fade.
pub(crate) fn at(off: u64, len: u64) -> Label {
    let fade = |d: u64| (d as f32 / FADE_US as f32).clamp(0.0, 1.0);
    let shown = fade(off).min(fade(len.saturating_sub(off)));
    let light = off.checked_sub(LIGHT_FROM_US).and_then(|t| {
        let into = t % EVERY_US;
        (into <= PASS_US).then(|| smooth(into as f32 / PASS_US as f32))
    });
    Label { shown, light }
}

fn mix(dst: &mut [u8], c: Rgb, a: f32) {
    let a = a.clamp(0.0, 1.0);
    for (d, s) in dst.iter_mut().zip(c) {
        *d = (f32::from(*d) * (1.0 - a) + f32::from(s) * a).round() as u8;
    }
}

fn scaled(c: Rgb, k: f32) -> Rgb {
    c.map(|v| (f32::from(v) * k).round().clamp(0.0, 255.0) as u8)
}

/// The light's band across the words: its path (px, from just touching the first letter of the wider line to just
/// past its last), the words' middle row it leans about, its width (a Gaussian's σ) and where it is in its pass.
pub(crate) struct Shine {
    from: f32,
    to: f32,
    mid: f32,
    sigma: f32,
    p: f32,
}

impl Shine {
    /// The light at pixel (x, y): how far it lifts the letter there (0..=1) and how far it dips it (0..=1, behind it).
    fn at(&self, x: usize, y: usize) -> (f32, f32) {
        let c = self.from + self.p * (self.to - self.from) + (self.mid - y as f32) * LEAN;
        let d = x as f32 - c;
        let g = |d: f32| (-(d / self.sigma).powi(2) / 2.0).exp();
        (g(d), g(d + 2.0 * self.sigma))
    }
}

/// A line of words (`p`, the renderer's `fg` on `ground`) laid at (`x0`, `y0`), drawn `shown` of the way: each pixel's
/// share of ink by its largest channel's distance from the ground; at rest `rest` at strength `own`, under the light
/// moved to `peak` at full strength and dipped behind it. Only where the words have ink.
#[allow(clippy::too_many_arguments)]
fn line(
    px: &mut [u8],
    (w, h): (usize, usize),
    p: &Picture,
    (x0, y0): (usize, usize),
    (fg, ground): (Rgb, Rgb),
    (rest, peak, own): (Rgb, Rgb, f32),
    shown: f32,
    shine: Option<&Shine>,
) {
    let (pw, ph, src) = p;
    let c = (0..3).max_by_key(|&c| (i32::from(fg[c]) - i32::from(ground[c])).abs()).unwrap_or(0);
    let span = (f32::from(fg[c]) - f32::from(ground[c])).abs().max(1.0);
    for y in 0..*ph {
        for x in 0..*pw {
            let cov = ((f32::from(src[(y * pw + x) * 3 + c]) - f32::from(ground[c])).abs() / span).min(1.0);
            let (gx, gy) = (x0 + x, y0 + y);
            if cov <= 0.0 || gx >= w || gy >= h {
                continue;
            }
            let (lit, dip) = shine.map_or((0.0, 0.0), |s| s.at(gx, gy));
            let mut col = scaled(rest, 1.0 - TRAIL * dip);
            mix(&mut col, peak, lit);
            let strength = (own + (1.0 - own) * lit) * (1.0 - TRAIL * dip * (1.0 - own));
            mix(&mut px[(gy * w + gx) * 3..][..3], col, cov * strength * shown);
        }
    }
}

/// How much of pixel (x, y) is inside the rounded box (l, t, r, b) of radius `rad`: 1 inside, 0 out, a px between.
fn inside(x: f32, y: f32, (l, t, r, b): (f32, f32, f32, f32), rad: f32) -> f32 {
    let (cx, cy) = (x.clamp(l + rad, r - rad), y.clamp(t + rad, b - rad));
    (0.5 - (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - rad)).clamp(0.0, 1.0)
}

/// The label `l` over the board's picture as `lay` places it: the title `title` and the small line `small` (the
/// renderer's pictures in the text's colour on the ground, at `lay`'s scale), centred on the picture, one under the
/// other as a chapter card stacks its lines; the panel where the board has something under them; the light (`cell`:
/// one of the title's letters, px). Whether the panel was drawn.
pub(crate) fn draw(
    px: &mut [u8],
    lay: &Layout,
    (title, small): (&Picture, Option<&Picture>),
    look: &Look,
    cell: usize,
    l: Label,
) -> bool {
    let (w, _) = lay.dims;
    let k = lay.scale.k();
    let small_w = small.map_or(0, |s| s.0);
    let (text_w, text_h) = (title.0.max(small_w), title.1 + small.map_or(0, |s| s.1));
    let (cx, cy) = (lay.at.0 + lay.pic.0 / 2, lay.at.1 + lay.pic.1 / 2);
    let (tx, ty) = (cx - text_w / 2, cy - text_h / 2);
    let at_title = (tx + (text_w - title.0) / 2, ty);
    let at_small = (tx + (text_w - small_w) / 2, ty + title.1);
    // (the panel's box: the words' ink and its air; the board under it, as this frame drew it)
    let (ax, ay) = (PANEL_AIR.0 * k, PANEL_AIR.1 * k);
    let (l0, t0) = ((tx + lay.pad).saturating_sub(ax), (ty + lay.pad).saturating_sub(ay));
    let (r0, b0) = (tx + text_w - lay.pad + ax, ty + text_h - lay.pad + ay);
    let under = (t0..b0).any(|y| {
        (l0..r0).any(|x| {
            let p = &px[(y * w + x) * 3..][..3];
            (0..3).any(|c| (i32::from(p[c]) - i32::from(look.ground[c])).abs() > BOARD_INK)
        })
    });
    if under {
        let rect = (l0 as f32, t0 as f32, r0 as f32, b0 as f32);
        let rad = PANEL_RADIUS * k as f32;
        for y in t0..b0 {
            for x in l0..r0 {
                let fill = inside(x as f32 + 0.5, y as f32 + 0.5, rect, rad);
                mix(&mut px[(y * w + x) * 3..][..3], look.ground, fill * PANEL * l.shown);
            }
        }
    }
    // (one light over both lines: across the wider line, its edge on the letters at both ends so a pass never pops in)
    let shine = l.light.map(|p| {
        let sigma = BAND_LETTERS * cell as f32 / 2.355;
        let reach = 1.5 * sigma + LEAN * text_h as f32 / 2.0;
        let (from, to) = ((tx + lay.pad) as f32 - reach, (tx + text_w - lay.pad) as f32 + reach);
        Shine { from, to, mid: (ty + text_h / 2) as f32, sigma, p }
    });
    let paper = look.ink;
    let title_line = (scaled(paper, TITLE_REST), WHITE, 1.0);
    line(px, lay.dims, title, at_title, (paper, look.ground), title_line, l.shown, shine.as_ref());
    if let Some(s) = small {
        let small_line = (paper, paper, LINE_STRENGTH);
        line(px, lay.dims, s, at_small, (paper, look.ground), small_line, l.shown, shine.as_ref());
    }
    under
}
