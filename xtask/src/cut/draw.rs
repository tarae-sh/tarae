//! The cut drawn (`cargo xtask cut FILE`): one pass over the video's frames in order, the take's log replayed as it goes
//! (`record::Replay` — one screen held, however long the take), each frame composed as the wide video composes one —
//! the screen at `video::fit`'s glyph centred above the band — and piped to ffmpeg; the sheet's pictures taken on the
//! way.
//!
//! - The camera: the whole, but for a zoom — its target found on the screen at its time (`camera::target`, or within
//!   `FIND_MS` after), widened to the screen's shape (`wide_window`, never nearer than `ZOOM_MAX`), eased in and out
//!   as the Shorts' camera moves (`camera::Path`), drawn through `video::view` so a zoom never steps.
//!   Where it ends has its own path, on its own clock, from the zooms whose time falls in it (`paths`).
//! - Crossings (`camera::CROSS_MS`, never a cut): where a part begins (the card of what's built, where it ends, a
//!   chapter's card, the take after them, the end card), where time is left out, and where most of the cells change at
//!   once (`camera::CROSS`).
//! - The band (`band`): the clock, the jump's mark, the speed's mark or `where it ends`, the caption.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use super::{
    Card, Cut, Frame, Marks, Pace, Program, What, clock, factor, jumped, opening, sped, told, video_time,
};
use crate::Result;
use crate::camera::{self, Cells, Region};
use crate::record::{Replay, Take};
use crate::recording::Picture;
use crate::scenes::docshots::none_of_yours;
use crate::shot::{self, Fonts, PAD, Rgb};
use crate::video::{self, BAND, H, W};

/// The band's own words — the clock, the jump's mark, the speed's and `where it ends` — and how strongly the quiet ones (the
/// clock, the speed) are drawn over the ground, of 255: read, never louder than the caption.
pub(crate) const MARK_PX: f32 = 26.0;
const QUIET: u8 = 170;
/// The mark over the take's stretch the opening shows, and its part's name on the sheet.
pub(crate) const ENDS: &str = "where it ends";
/// A chapter card's title.
const CHAPTER_PX: f32 = 56.0;
/// The camera never goes nearer than this: its window at least 1/`ZOOM_MAX` of the screen across.
pub(crate) const ZOOM_MAX: f32 = 2.0;
/// A zoom's target is looked for this long after its time (tarae may draw it a frame or two later).
const FIND_MS: u64 = 2_000;
/// The sheet: a picture every `SHEET_EVERY_US` of the video (more apart past `SHEET_REGULAR` of them), and at each part,
/// caption, zoom, jump and change of speed; `PAGE` a page.
const SHEET_EVERY_US: u64 = 2_000_000;
const SHEET_REGULAR: u64 = 120;
const PAGE: usize = 6;

pub(super) fn run(file: &Path) -> Result<()> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let dir = file.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let take = Take::read(dir)?;
    let marks = Marks::read(dir, take.ms())?;
    let cut = Cut::parse(&text, &file.display().to_string(), &marks)?;
    let yours = std::env::var_os("HOME");
    let words = (cut.chapters.iter().map(|c| c.title.as_str()))
        .chain(cut.captions.iter().map(|c| c.text.as_str()))
        .chain([cut.opening.name.as_str(), cut.opening.line.as_str()])
        .chain(cut.opening.code.iter().map(String::as_str));
    for w in words {
        none_of_yours(&[w.chars().map(String::from).collect()], yours.as_deref())?;
    }
    let stem = file.file_stem().map_or("cut".into(), |s| s.to_string_lossy().into_owned());
    let out = dir.join(stem);
    let said = make(&cut, &take, (dir, &marks), &out)?;
    let p = out.join("cut.txt");
    std::fs::write(&p, &said).map_err(|e| format!("{}: {e}", p.display()))?;
    print!("{said}");
    Ok(())
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
}

/// `text` at `px` in `fg`, kept: the band's words repeat.
struct Words<'f> {
    fonts: &'f Fonts,
    ground: Rgb,
    kept: HashMap<(String, u32, Rgb), Picture>,
}

impl Words<'_> {
    fn get(&mut self, text: &str, px: f32, fg: Rgb) -> &Picture {
        if self.kept.len() > 512 {
            self.kept.clear();
        }
        let (fonts, ground) = (self.fonts, self.ground);
        (self.kept.entry((text.to_string(), px as u32, fg)))
            .or_insert_with(|| video::text(text, px, fg, ground, false, fonts))
    }
}

/// The cut of `take` (in `dir`, its landmarks `marks`) as `cut` says, into `out`: `cut.mp4` with its sound
/// (`sound.rs`, `cut.wav` beside it), the sheet's pages, `chapters.txt`. What was made, said.
pub(crate) fn make(cut: &Cut, take: &Take, (dir, marks): (&Path, &Marks), out: &Path) -> Result<String> {
    let started = Instant::now();
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let prog = Program::new(cut);
    let size = take.size;
    let fonts = Fonts::load(1)?;
    let glyph = video::fit(&fonts, usize::from(size.0), usize::from(size.1))
        .ok_or_else(|| format!("{}×{} cells don't fit {W}×{H} over the band", size.0, size.1))?;
    let blank = vt100::Parser::new(size.1, size.0, 0);
    let (pw, ph, _) = shot::render_px(blank.screen(), &fonts, glyph, None);
    let (x0, y0) = ((W - pw) / 2, (H - BAND - ph) / 2);
    let look = video::look(cut.bg);
    let seal = video::seal(cut.bg)?;
    let name = ("t a r a e", video::NAME_PX, look.accent, true);
    let told = told(cut.end, prog.total);
    let built_card = opening::card(&cut.opening, &told, cut.bg, &look, &fonts, (W, H));
    let mut said = Vec::new();
    // (the intro: the swing's frames, else the name and the seal — said)
    let intro = match video::WideIntro::new(cut.bg, &look, &fonts) {
        Ok(i) => {
            let (n, (w, h), _) = i.drawn();
            said.push(format!(
                "the intro: the brush's swing, {n} frames made {}×{} and drawn {w}×{h}, then `t a r a e` risen, held to {:.1} s",
                i.made.0,
                i.made.1,
                super::INTRO_MS as f64 / 1e3
            ));
            Ok(i)
        }
        Err(e) => {
            said.push(format!("the intro: no brush swing ({e}) — the name and the seal, still"));
            Err(video::card(&[name], &seal, &look, &fonts, (W, H)))
        }
    };
    said.push(format!("the card of what's built: “{}” — “{told}”", cut.opening.name));
    let end_card =
        video::card(&[name, ("tarae.sh", video::LINE_PX, look.ink, false)], &seal, &look, &fonts, (W, H));
    let chapter_cards: Vec<Vec<u8>> = (cut.chapters.iter().enumerate())
        .map(|(i, c)| {
            let n = format!("{}", i + 1);
            video::card(
                &[(&n, video::LINE_PX, look.faint, false), (&c.title, CHAPTER_PX, look.ink, true)],
                &seal,
                &look,
                &fonts,
                (W, H),
            )
        })
        .collect();

    // (the camera: each zoom's window from its target on the screen then)
    let mut windows = Vec::new();
    let mut found_at = Vec::new();
    let mut scan = Replay::new(&take.screen, size);
    for z in &cut.zooms {
        scan.to(z.at);
        let mut found = camera::target(&Cells::of(scan.screen()), &z.on);
        let mut when = z.at;
        while found.is_none() {
            match scan.next() {
                Some(ms) if ms <= z.at + FIND_MS => {
                    found = camera::target(&Cells::of(scan.screen()), &z.on);
                    when = ms;
                }
                _ => break,
            }
        }
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
            "zoom at {}{} on \"{}\": {:.0}×{:.0} cells at ({:.0}, {:.0}), {:.2}× the whole",
            clock(z.at),
            match (z.ends, z.shown) {
                (true, true) => " (where it ends, and in its place)",
                (true, false) => " (where it ends only)",
                _ => "",
            },
            z.on,
            window.w,
            window.h,
            window.x,
            window.y,
            f32::from(size.0) / window.w
        ));
    }
    drop(scan);
    let reads = take_reads(cut, &windows);
    let lost = lost_zooms(cut, take, &prog, &reads, &found_at);
    let paths = paths(cut, &windows, size);

    // (the sheet's pictures: which frames, and what each is marked with)
    let frames = prog.frames();
    let mut picks: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    let every = SHEET_EVERY_US.max(prog.total / SHEET_REGULAR);
    for v in (0..prog.total).step_by(every as usize) {
        picks.entry(Program::frame_at(v).min(frames - 1)).or_default();
    }
    for (v, mark) in sheet_marks(&prog) {
        picks.entry(Program::frame_at(v).min(frames - 1)).or_default().push(mark);
    }

    let mp4 = out.join("cut.mp4");
    let ffmpeg = video::ffmpeg();
    // (the sound first: ffmpeg reads it beside the frames)
    let wav = out.join("cut.wav");
    let heard = match &ffmpeg {
        Some(f) => {
            let keys = match dir.join("keys.bin").exists() {
                true => crate::record::read_log(&dir.join("keys.bin"))?,
                false => Vec::new(),
            };
            Some(super::sound::make(&prog, dir, &keys, marks, f, &wav)?)
        }
        None => None,
    };
    let mut child = match &ffmpeg {
        Some(f) => Some(video::encoder(f, &mp4, (W, H), heard.as_ref().map(|_| wav.as_path()))?),
        None => None,
    };
    let mut stdin = child.as_mut().and_then(|c| c.stdin.take());
    let crossings = prog.crossings();
    let mut next_cross = 0;
    let (mut ends, mut replay) = (Replay::new(&take.screen, size), Replay::new(&take.screen, size));
    let mut words = Words { fonts: &fonts, ground: look.ground, kept: HashMap::new() };
    let mut pic: Option<((bool, u64, u32), Picture)> = None;
    let mut last: Option<(Key, Vec<u8>)> = None;
    let mut cross: Option<(Vec<u8>, u64)> = None;
    let mut seen: Option<Cells> = None;
    let (mut differ, mut drawn, mut wrote) = (0u64, 0u64, Ok(()));
    let mut sheet: Vec<Picture> = Vec::new();
    let mut pages = 0;
    let all = Region::all(size);
    for k in 0..frames {
        let v = Program::time_of(k);
        let f = prog.frame(v);
        // (the screen in force; where most of it changed at once, a crossing from the frame before)
        if let Some(t) = f.screen {
            let r = if f.ends { &mut ends } else { &mut replay };
            let came = r.to(t);
            if came && !f.ends {
                let cells = Cells::of(replay.screen());
                if seen.as_ref().is_some_and(|s| camera::changed(s, &cells) > camera::CROSS)
                    && let Some((_, px)) = &last
                {
                    cross = Some((px.clone(), v));
                }
                seen = Some(cells);
            }
        }
        while next_cross < crossings.len() && crossings[next_cross] <= v {
            if let Some((_, px)) = &last {
                cross = Some((px.clone(), crossings[next_cross]));
            }
            next_cross += 1;
        }
        let share = cross.as_ref().map(|(_, began)| {
            let a = camera::ease_in_out((v - began) as f32 / (camera::CROSS_MS * 1000) as f32);
            (a * 255.0).round() as u8
        });
        if share == Some(255) {
            cross = None;
        }
        let share = share.filter(|&a| a < 255);
        let region = region(&f, cut, &paths, size);
        let screen = match f.ends {
            true => (true, ends.shown),
            false => (false, replay.shown),
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
        };
        if last.as_ref().is_none_or(|l| l.0 != key) {
            let mut px = match f.card {
                Some(Card::Intro(ms)) => match &intro {
                    Ok(i) => i.frame(ms, &look, (W, H)),
                    Err(still) => still.clone(),
                },
                Some(Card::Built) => built_card.clone(),
                Some(Card::End) => end_card.clone(),
                Some(Card::Chapter(i)) => chapter_cards[i].clone(),
                None => {
                    let s = if f.ends { ends.screen() } else { replay.screen() };
                    let mut px = look.ground.repeat(W * H);
                    let view = (region != all)
                        .then(|| video::view(region, size, (pw, ph), glyph, |p| shot::cell(&fonts, p)));
                    let at = view.as_ref().map_or(glyph, |v| v.px);
                    let want = (screen.0, screen.1, at as u32);
                    if pic.as_ref().is_none_or(|p| p.0 != want) {
                        pic = Some((want, shot::render_px(s, &fonts, at, None)));
                        drawn += 1;
                    }
                    let p = &pic.as_ref().expect("drawn").1;
                    match &view {
                        None => video::blit(&mut px, (W, H), p, x0, y0, 255),
                        Some(view) => video::resample(&mut px, W, (x0, y0), (pw, ph), p, view),
                    }
                    let at = (H - BAND) * W * 3;
                    for x in x0 + PAD..x0 + pw - PAD {
                        px[at + x * 3..][..3].copy_from_slice(&look.faint);
                    }
                    band(&mut px, &f, cut, (x0, pw), &look, &mut words);
                    px
                }
            };
            if let (Some(a), Some((from, _))) = (share, &cross) {
                crossed(from, &mut px, a);
            }
            differ += u64::from(last.as_ref().is_none_or(|l| l.1 != px));
            last = Some((key, px));
        }
        let px = &last.as_ref().expect("a frame").1;
        if let Some(w) = stdin.as_mut()
            && wrote.is_ok()
        {
            wrote = w.write_all(px);
        }
        if let Some(marks) = picks.get(&k) {
            let label = label(&prog, v, &f, region != all, marks);
            let cols = W / 2 / shot::cell(&fonts, 15.0).0;
            sheet.push(crate::recording::labelled(video::half(px, (W, H)), &label, cols, &fonts));
            if sheet.len() == PAGE {
                pages += 1;
                crate::recording::grid(&sheet, &out.join(format!("sheet-{pages:02}.png")))?;
                sheet.clear();
            }
        }
    }
    if !sheet.is_empty() {
        pages += 1;
        crate::recording::grid(&sheet, &out.join(format!("sheet-{pages:02}.png")))?;
    }
    // (pages an earlier, longer cut left: gone)
    for k in pages + 1.. {
        if std::fs::remove_file(out.join(format!("sheet-{k:02}.png"))).is_err() {
            break;
        }
    }
    drop(stdin);
    let encoded = match child {
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
    let mut head = vec![format!(
        "{}: {:.1} s of video from {} of take ({}), glyphs {glyph} px; {frames} frames ({differ} differ), {drawn} screens \
         drawn; the sheet {} pictures in {pages} pages",
        mp4.display(),
        s(prog.total),
        clock(cut.end),
        cut.file,
        picks.len(),
    )];
    head.extend(spans_said(cut));
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

/// The take part's camera reads (the map's ms: from, to, window), one for each zoom (`windows`, the zooms' order) — a
/// zoom the take part leaves out (shown only where it ends) at no length.
pub(crate) fn take_reads(cut: &Cut, windows: &[Region]) -> Vec<(u64, u64, Region)> {
    (cut.zooms.iter().zip(windows))
        .map(|(z, &w)| {
            let m = cut.map.video(z.at) / 1000;
            (m, if z.shown { m + camera::DRIFT_MS + z.len } else { m }, w)
        })
        .collect()
}

/// The camera's two paths from each zoom's window (`windows`): the take part's, on the map's ms, from the zooms it
/// shows; where it ends', on its own ms from its start, from the zooms whose time falls in it.
pub(crate) fn paths(cut: &Cut, windows: &[Region], size: (u16, u16)) -> (camera::Path, camera::Path) {
    let reads: Vec<_> = (take_reads(cut, windows).into_iter().zip(&cut.zooms))
        .filter(|(_, z)| z.shown)
        .map(|(r, _)| r)
        .collect();
    let take =
        camera::Path::of(&camera::aims(&reads, size), size, cut.map.len() / 1000 + 2 * camera::DRIFT_MS);
    let o = &cut.opening;
    let reads: Vec<_> = (cut.zooms.iter().zip(windows))
        .filter(|(z, _)| z.ends)
        .map(|(z, &w)| (z.at - o.from, z.at - o.from + camera::DRIFT_MS + z.len, w))
        .collect();
    let ends = camera::Path::of(&camera::aims(&reads, size), size, o.len + 2 * camera::DRIFT_MS);
    (take, ends)
}

/// The camera's region for frame `f`: where it ends, its own path's; in the take part, the map's; else the whole.
pub(crate) fn region(f: &Frame, cut: &Cut, paths: &(camera::Path, camera::Path), size: (u16, u16)) -> Region {
    match (f.ends, f.screen, f.map) {
        (true, Some(t), _) => paths.1.at(t.saturating_sub(cut.opening.from)),
        (false, _, Some(m)) => paths.0.at(m / 1000),
        _ => Region::all(size),
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
        if !z.shown {
            continue;
        }
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

/// The band under the screen (its picture at `x0`, `pw` wide): the take's clock at the left with the jump's mark after
/// it, the speed's mark (or `where it ends`) at the right, the caption in the middle — each centred on the band's height.
fn band(
    px: &mut [u8],
    f: &Frame,
    cut: &Cut,
    (x0, pw): (usize, usize),
    look: &video::Look,
    words: &mut Words,
) {
    let mid = H - BAND / 2;
    let mut put = |p: &Picture, x: usize, a: u8| video::blit(px, (W, H), p, x, mid - p.1 / 2, a);
    let mut left = x0;
    if let Some(t) = f.clock {
        let p = words.get(&clock(t), MARK_PX, look.ink);
        left += p.0;
        put(p, x0, QUIET);
    }
    if let Some((ms, a)) = f.jump {
        let p = words.get(&jumped(ms), MARK_PX, look.accent);
        put(p, left, a);
    }
    let right = match (f.ends, f.speed.and_then(sped)) {
        (true, _) => Some((ENDS.to_string(), look.accent, 255)),
        (false, Some(s)) => Some((s, look.ink, QUIET)),
        (false, None) => None,
    };
    if let Some((t, fg, a)) = right {
        let p = words.get(&t, MARK_PX, fg);
        put(p, (x0 + pw).saturating_sub(p.0), a);
    }
    if let Some((i, a)) = f.caption {
        let p = words.get(&cut.captions[i].text, video::CAPTION_PX, look.ink);
        put(p, (W - p.0.min(W)) / 2, a);
    }
    // (a chapter with no card: its number, faint, then its title, centred as a caption is)
    if let Some((i, a)) = f.chapter {
        let n = words.get(&format!("{}   ", i + 1), video::CAPTION_PX, look.faint).clone();
        let p = words.get(&cut.chapters[i].title, video::CAPTION_PX, look.ink);
        let x = (W - (n.0 + p.0).min(W)) / 2;
        put(&n, x, a);
        put(p, x + n.0, a);
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
            What::Last => out.push((p.at, "the last screen held".into())),
            What::End => out.push((p.at + p.len / 2, "end card".into())),
            What::Take { .. } => {}
        }
    }
    for c in &cut.captions {
        if let Some(v) = prog.video_of(map.video(c.at)) {
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
            out.push((v + camera::CROSS_MS * 1000, format!("jump {}", jumped(ms))));
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
