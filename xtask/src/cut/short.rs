//! A Short's form (`form = "short"`, main's `.sb/cut-short-form-brief.md`): the same edit list drawn upright on
//! `video.rs`'s Short — its window (`video::short_window`), band (`video::band_px`, `video::SHORT_BAND`), eye
//! (`video::short_eye`), caption box (`video::caption_boxed`) and end card (`video::brand`) —, with no figure of its own
//! but where the band's three things stand in it (`Band`):
//!
//! - **the take** 80×34 (`video::short_size`), drawn whole at its own glyph in the window (`layout`) — any other size
//!   refused, nothing scaled or cropped;
//! - **the time row** on the band's top, over everything under it: the take's clock at its left, the jump's mark after
//!   it, the speed's mark (or `where it ends`) at its right — what makes the cut honest, in every frame as the wide
//!   band has them;
//! - **the character and its line** under it: the mascot at its keys at the band's left end, its ink on the safe
//!   area's corner and its foot on the band's foot (the user, 2026-11-18: "캐릭터를 다시 왼쪽에 있게 하자" — YouTube's
//!   buttons take the frame's right edge, so only at the left can it stand in the corner); and the caption
//!   `mascot::GAP` past its box, every line starting in one place, growing right up to the band's right edge, two
//!   lines at most at `video::CAPTION_SHORT_PX`, bold, in the Shorts' box. The
//!   box's air is `video::CAPTION_PAD` alone — the renderer's own margin (`shot::PAD`) left out of its lines' picture,
//!   as a scene's box has it too — so two lines leave the row its room: 220 px hold the row (`Band::row`, 40) and
//!   two lines (180), never one over another nor over a cell;
//! - **zooms** through the Short's camera (`window`): the target at the eye's height, never past
//!   `video::SHORT_PX_MAX`, eased as the wide cut's are (`camera::Path`);
//! - **the brand's end card** (`end_card`): the swing and `t a r a e`, the Short's title its caption in the box.

use super::draw::{Layout, MARK_PX, QUIET, Words, ZOOM_MAX, jump_ink};
use super::mascot::{self, Place};
use super::{Cut, Frame, Program, jumped, sped};
use crate::Result;
use crate::camera::{self, Region};
use crate::recording::Picture;
use crate::shot::{self, Fonts, PAD};
use crate::video::{self, FPS, Look, SHORT};

/// Where the band's three things stand (px): the band (x, y, w, h — `video::band_px`), the time row's height on its top
/// and its middle, the mascot's box's top left, the line's box's room (its left, its right at most, its middle), and the
/// widest line it holds (characters at `video::CAPTION_SHORT_PX`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Band {
    pub(crate) at: (usize, usize, usize, usize),
    pub(crate) row: usize,
    pub(crate) row_mid: usize,
    pub(crate) mascot: (usize, usize),
    pub(crate) line: (usize, usize, usize),
    pub(crate) cols: usize,
}

/// The band as `video.rs` lays it, across the safe area's width under the window.
fn band_at() -> (usize, usize, usize, usize) {
    let b = video::band_px();
    (b.x as usize, b.y as usize, b.w as usize, b.h as usize)
}

/// The room the mascot's box takes at the band's corner (its top left): its left edge at the band's left end (the
/// safe area's), its foot on the band's foot — the line starts `mascot::GAP` past it. The character itself stands in
/// it by its ink (`place`), only ever further left and down.
pub(crate) fn mascot_at() -> (usize, usize) {
    let (x, y, _, h) = band_at();
    (x, y + h - mascot::BOX.1)
}

/// A caption box's height for `n` lines (px): its lines' cells and `video::CAPTION_PAD`'s air.
fn box_h(n: usize, fonts: &Fonts) -> usize {
    n * shot::cell(fonts, video::CAPTION_SHORT_PX).1 + 2 * video::CAPTION_PAD.1
}

impl Band {
    pub(crate) fn new(fonts: &Fonts) -> Band {
        let at @ (x, y, w, h) = band_at();
        let two = box_h(2, fonts);
        let row = h.saturating_sub(two);
        let (mx, my) = mascot_at();
        // (the line from `mascot::GAP` past the box to the band's right edge)
        let (left, right) = (mx + mascot::BOX.0 + mascot::GAP, x + w);
        let cw = shot::cell(fonts, video::CAPTION_SHORT_PX).0;
        let cols = right.saturating_sub(left + 2 * video::CAPTION_PAD.0) / cw.max(1);
        Band { at, row, row_mid: y + row / 2, mascot: (mx, my), line: (left, right, y + row + two / 2), cols }
    }
}

/// The mascot's place on a Short's frames (`mascot::found`'s): by its ink, not its box — the furthest ink of every
/// picture the cut can draw (`Art::ink_reach`) on the band's corner, the safe area's left edge and the band's foot (the
/// user, 2026-11-18: "캐릭터를 다시 왼쪽에 있게 하자" — at the right YouTube's buttons keep it 150 px off the frame's
/// edge, at the left 48), so no frame's ink crosses it and one reaches it; no rule, no key pops' lane.
pub(crate) fn place(art: &mascot::Art) -> Result<Place> {
    let (bx, by, bw, bh) = band_at();
    let reach = art.ink_reach().ok_or("its pictures hold no ink")?;
    let x = bx.checked_sub(reach.left.0).ok_or("its ink starts past the band's left edge")?;
    Ok(Place { x, y: by + bh - reach.bottom.0, rule_end: x, marks_right: bx + bw + PAD, lane: None })
}

/// A Short's layout of a take `size` cells: `video::short_size`'s alone — refused, both sizes said, where it isn't —,
/// drawn whole at the glyph whose cells fit the window (`camera::fit`, as a scene's Short), centred in it; the band's
/// top its rule's row (none is drawn).
pub(crate) fn layout(fonts: &Fonts, size: (u16, u16)) -> Result<Layout> {
    let want = video::short_size()?;
    if size != want {
        return Err(format!(
            "the take is {}×{} cells, and a Short's is {}×{} (video::short_size): record it at that size — nothing is \
             scaled or cropped to fit",
            size.0, size.1, want.0, want.1
        ));
    }
    let (wx, wy, ww, wh) = video::short_window();
    let glyph = camera::fit(Region::all(size), (ww, wh), video::SHORT_PX_MAX, |p| shot::cell(fonts, p));
    let (cw, ch) = shot::cell(fonts, glyph);
    let pic = (usize::from(size.0) * cw + 2 * PAD, usize::from(size.1) * ch + 2 * PAD);
    let (_, by, _, bh) = band_at();
    Ok(Layout {
        scale: video::Scale::HD,
        dims: SHORT,
        glyph,
        pic,
        at: (wx + ww.saturating_sub(pic.0) / 2, wy + wh.saturating_sub(pic.1) / 2),
        rule: by,
        mid: by + bh / 2,
        pad: PAD,
        short: true,
    })
}

/// A zoom's window for `target` on a `cols`×`rows` screen drawn whole at `glyph`, through the Short's camera: the
/// screen's shape, never nearer than `video::SHORT_PX_MAX` draws it (nor `ZOOM_MAX`), the target's middle across at
/// the window's and down at the eye's height (`video::short_eye`), inside the screen — then held on whole rows as the
/// frame shows it, the scenes' Short camera's own snap (`camera::on_rows` through `video::frame_of`, of the screen's
/// picture and its margin as `Layout::view` draws it): no row cut across at its top or bottom, the target still shown,
/// never nearer than the most. Its columns aren't snapped: the scenes' camera doesn't either.
pub(crate) fn window(t: Region, (cols, rows): (u16, u16), glyph: f32, fonts: &Fonts) -> Region {
    let (c, r) = (f32::from(cols), f32::from(rows));
    let most = shot::cell(fonts, video::SHORT_PX_MAX as f32).0 as f32;
    let least = c * shot::cell(fonts, glyph).0 as f32 / most;
    let nearest = least.max(c / ZOOM_MAX).min(c);
    let w = t.w.max(t.h * c / r).max(nearest);
    let h = w * r / c;
    let x = (t.x + t.w / 2.0 - w / 2.0).clamp(0.0, c - w);
    let y = (t.y + t.h / 2.0 - video::short_eye() * h).clamp(0.0, r - h);
    let eyed = Region { x, y, w, h };
    let cell = |p| shot::cell(fonts, p);
    let (cw, ch) = cell(glyph);
    let pic = (usize::from(cols) * cw + 2 * PAD, usize::from(rows) * ch + 2 * PAD);
    let (_, frame) = video::frame_of((cols, rows), pic, glyph, cell);
    let shows = |k: &Region| {
        let s = camera::shown(*k, frame);
        let e = 1e-3;
        s.x <= t.x + e && t.x + t.w <= s.x + s.w + e && s.y <= t.y + e && t.y + t.h <= s.y + s.h + e
    };
    (camera::on_rows(eyed, frame, (cols, rows)).into_iter())
        .find(|k| k.w >= nearest - 1e-3 && shows(k))
        .unwrap_or(eyed)
}

/// `text` as the character's line: in two lines at most of `cols` (`video::wrap`, balanced) — refused, with its text
/// and count, where it takes more or a word is longer than a line.
pub(crate) fn lines(text: &str, cols: usize) -> std::result::Result<Vec<String>, String> {
    let l = video::wrap(text, cols);
    let n = text.chars().count();
    match l.len() > 2 || l.iter().any(|x| x.chars().count() > cols) {
        true => Err(format!(
            "“{text}”, {n} characters: {} lines of {cols} at most beside the mascot — two at most",
            l.len()
        )),
        false => Ok(l),
    }
}

/// The widest line of the end card's title (characters): the safe area's width, the box's air left out — no mascot on
/// the card.
pub(crate) fn title_cols(fonts: &Fonts) -> usize {
    (video::safe_w() - 2 * video::CAPTION_PAD.0) / shot::cell(fonts, video::CAPTION_SHORT_PX).0.max(1)
}

/// Every caption and the hook line in two lines beside the mascot, each line of the title within the card's width —
/// checked before a frame; each that doesn't is refused at once, with its line.
pub(crate) fn check(cut: &Cut, fonts: &Fonts) -> Result<()> {
    let Some(short) = &cut.short else { return Ok(()) };
    let cols = Band::new(fonts).cols;
    let mut refused = Vec::new();
    let mut put =
        |line: usize, what: &str, e: String| refused.push(format!("{}:{line}: {what}: {e}", cut.file));
    if let Err(e) = lines(&short.hook, cols) {
        put(short.hook_at, "opening: hook", e);
    }
    for c in &cut.captions {
        if let Err(e) = lines(&c.text, cols) {
            put(c.line, "caption", e);
        }
    }
    let most = title_cols(fonts);
    for t in short.title.iter().filter(|t| t.chars().count() > most) {
        put(
            short.title_at,
            "title",
            format!("“{t}”, {} characters: {most} a line at most", t.chars().count()),
        );
    }
    match refused.len() {
        0 => Ok(()),
        1 => Err(refused.remove(0)),
        n => Err(format!("{n} lines don't fit a Short's band:\n  {}", refused.join("\n  "))),
    }
}

/// The video's length within a Short's (`video::SHORT_MAX_MS`) — refused past it, by as much.
pub(crate) fn length(prog: &Program) -> Result<()> {
    let ms = prog.frames() * 1000 / FPS;
    match ms > video::SHORT_MAX_MS {
        true => Err(format!(
            "{}: the Short runs {:.2} s ({} frames), past its {} s — leave out more of the take",
            prog.cut.file,
            ms as f64 / 1e3,
            prog.frames(),
            video::SHORT_MAX_MS / 1000
        )),
        false => Ok(()),
    }
}

/// `p` with `(dx, dy)` px left out at each side.
fn crop(p: &Picture, (dx, dy): (usize, usize)) -> Picture {
    let (w, h) = (p.0.saturating_sub(2 * dx), p.1.saturating_sub(2 * dy));
    let mut out = Vec::with_capacity(w * h * 3);
    for y in dy..dy + h {
        out.extend_from_slice(&p.2[(y * p.0 + dx) * 3..(y * p.0 + dx + w) * 3]);
    }
    (w, h, out)
}

/// A line's picture in the box: its lines, each as wide as the widest (the box's ground behind every cell), bold at
/// `video::CAPTION_SHORT_PX` (`video::caption_text`), the renderer's margin left out.
pub(crate) fn caption_pic(lines: &[String], fonts: &Fonts) -> Picture {
    let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let laid: Vec<String> = lines.iter().map(|l| format!("{l:<w$}")).collect();
    crop(&video::caption_text(&laid, fonts), (PAD, PAD))
}

/// The band of a Short's take frame `f` on `px`: the time row (the clock, the jump's mark, the speed's or `where it
/// ends`), then the line beside where the mascot stands — the caption in force, or over where it ends the hook line.
pub(crate) fn band(px: &mut [u8], f: &Frame, cut: &Cut, look: &Look, words: &mut Words) {
    let b = Band::new(words.fonts);
    let (x0, _, w, _) = b.at;
    // (the row's words, their ink rows alone: nothing of their ground over the line's box)
    let mut put = |p: &Picture, x: usize, a: u8| {
        let p = crop(p, (0, PAD));
        video::blit(px, SHORT, &p, x, b.row_mid - p.1 / 2, a);
    };
    let mut left = x0 - PAD;
    if let Some(t) = f.clock {
        let p = words.word(&super::clock(t), MARK_PX, look.ink, false).clone();
        put(&p, left, QUIET);
        left += p.0;
    }
    if let Some((ms, a)) = f.jump {
        let (fg, a) = jump_ink(ms, a, look);
        let p = words.word(&jumped(ms), MARK_PX, fg, false).clone();
        put(&p, left, a);
    }
    let mark = match (f.ends, f.speed.and_then(sped)) {
        (true, _) => Some((super::draw::ENDS.to_string(), look.accent, 255)),
        (false, Some(s)) => Some((s, look.ink, QUIET)),
        (false, None) => None,
    };
    if let Some((t, fg, a)) = mark {
        let p = words.word(&t, MARK_PX, fg, false).clone();
        put(&p, (x0 + w + PAD).saturating_sub(p.0), a);
    }
    let said = match (f.caption, f.hook, &cut.short) {
        (Some((i, a)), _, _) => Some((cut.captions[i].text.as_str(), a)),
        (None, Some(a), Some(s)) => Some((s.hook.as_str(), a)),
        _ => None,
    };
    if let Some((text, a)) = said {
        // (refused before a frame where it doesn't fit: `check`)
        let l = lines(text, b.cols).unwrap_or_else(|_| vec![text.to_string()]);
        let pic = words.caption(&l);
        let (_, h) = video::caption_box(pic);
        video::caption_boxed(px, pic, (b.line.0, b.line.2 - h / 2), f32::from(a) / 255.0);
    }
}

/// The end card `ms` in: the brand's (`video::brand_frame`; else `still`, the name and the seal), the title in the box
/// at the caption's place under it (`video::band_px`, centred), coming in at `video::TITLE_FROM` over the fade.
pub(crate) fn end_card(
    brand: std::result::Result<&video::Brand, &Vec<u8>>,
    title: &Picture,
    look: &Look,
    ms: u64,
) -> Vec<u8> {
    let mut out = match brand {
        Ok(b) => video::brand_frame(b, look, ms),
        Err(still) => still.clone(),
    };
    let a = (ms.saturating_sub(video::TITLE_FROM) as f32 / super::FADE_MS as f32).clamp(0.0, 1.0);
    if ms >= video::TITLE_FROM && a > 0.0 {
        let (w, h) = video::caption_box(title);
        let r = crate::captions::at(video::band_px(), (w as f32, h as f32));
        video::caption_boxed(
            &mut out,
            title,
            (r.x.round().max(0.0) as usize, r.y.round().max(0.0) as usize),
            a,
        );
    }
    out
}

/// Whether a px of a Short's frame is inside the safe area (`video::SAFE`) — the band's things' test.
#[cfg(test)]
pub(crate) fn safe(x: usize, y: usize) -> bool {
    let (top, right, bottom, left) = video::SAFE;
    (left..SHORT.0 - right).contains(&x) && (top..SHORT.1 - bottom).contains(&y)
}
