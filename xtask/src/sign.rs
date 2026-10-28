//! `cargo xtask sign [<out-dir>]` — the working sign as built and its other candidates, as animations (#159 again: the
//! user, 2026-10-28, "the way it moves feels awkward"; 2, the breath alone, chosen on these and built). One turn of the
//! stand-in claude (`shimmer-sign`, `scenes::SIGN_TURN`: start, a think, prose, a stall, a read's wait, prose) with a
//! task running beside it, recorded cell by cell in meok and hanji — the build's own cell in the chat's top line, the
//! status line's now line and the task tab's badge as candidate 2, each other candidate drawn into the same frames
//! where that cell stands — and the frames written as animated PNGs at their recorded times. How the next change to
//! the sign is judged: build it, run this, watch it beside the rest. Into `<out-dir>` (`target/sign` by default):
//! - `sign-<n>-<candidate>-<theme>.png`: one candidate in its three places;
//! - `sign-all-<theme>.png`: every candidate, one above the other, on one clock;
//! - `strip-<theme>.png` and `ticks.txt`: the now line's sign at every tick, a row per candidate and phase — still,
//!   for looking at what each draws;
//! - `index.html`: the animations side by side; `load.txt`: frames a second, tarae's and what each sign asks of them.
//!
//! The others are drawn by xtask, not by tarae: where the turn's cell is still (the stream stopped) comes from the
//! turn's script, `▲`/`◦` from the recorded cell, a candidate's clock is the frame's time in `TICK`s — as tarae's
//! `editor::anim_step` is the time it draws at. (The first round's 1, the stream's level, was recorded from the build
//! of its day; it can't be drawn, and is gone with it.)

use std::f32::consts::PI;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::scenes::{Fresh, SIGN_STREAM, SIGN_TURN, config_toml, demo_crate_with, run_files, write};
use crate::shot::{self, Fonts, Setup};
use crate::{Result, root};

const SIZE: (u16, u16) = (100, 30);
/// How long a recording runs from the question's `enter`: the turn and a little past its end.
const RECORD_MS: u64 = 12_400;
/// tarae's animation step (`editor::TICK`), and how long a stopped stream keeps the turn's cell moving
/// (`shimmer::STILL_AFTER`).
const TICK: u64 = 100;
const STILL_AFTER: u64 = 1000;
/// Cells shown of each place, and the labels' column before them.
const CROP: usize = 46;
const LABEL: usize = 17;
/// The shortest a frame is shown: browsers play an APNG's frame of 10 ms or less as 100 ms.
const FRAME_MIN: u64 = 20;
/// The last frame held this long before the animation starts again.
const HOLD_MS: u64 = 1500;

/// The build's heights (`shimmer::CELLS`).
const CELLS: [char; 4] = ['▃', '▄', '▅', '▆'];

/// A candidate: its name in files, its title, the cells it takes, what it draws at a clock step (None: the build's, the
/// recorded cell as it is) and what it holds while the stream stands still (None: where its clock stopped).
struct Cand {
    key: &'static str,
    title: &'static str,
    width: usize,
    at: Option<fn(u64) -> String>,
    rest: Option<&'static str>,
}

const CANDS: [Cand; 5] = [
    Cand { key: "2-breath", title: "2 the 2 s breath — as built", width: 1, at: None, rest: None },
    Cand {
        key: "3-dot", title: "3 a dot breathing — · • ●, 2 s", width: 1, at: Some(dot), rest: Some("·")
    },
    Cand {
        key: "4-two",
        title: "4 two cells out of phase — 1.2 s",
        width: 2,
        at: Some(two),
        rest: Some("▃▃"),
    },
    Cand {
        key: "5-wave",
        title: "5 three cells — the old wave, 1 s",
        width: 3,
        at: Some(wave),
        rest: Some("▂▂▂"),
    },
    Cand {
        key: "6-round", title: "6 a quadrant going round — 0.8 s", width: 1, at: Some(round), rest: None
    },
];

/// `0..=top` on a cosine of `period` steps: from the floor to the top and back, lingering at both ends.
fn cosine(step: u64, period: u64, top: usize) -> usize {
    let t = (step % period) as f32 / period as f32;
    (0.5 * (1.0 - (2.0 * PI * t).cos()) * top as f32).round() as usize
}

/// A dot on the same breath, as Claude Code's one cell breathes.
fn dot(step: u64) -> String {
    ['·', '•', '●'][cosine(step, 20, 2)].to_string()
}

/// Two bars on one triangle half a period apart — `▃▆ ▄▅ ▅▄ ▆▃ ▅▄ ▄▅` — a step every two ticks.
fn two(step: u64) -> String {
    const TRI: [usize; 6] = [0, 1, 2, 3, 2, 1];
    let n = (step / 2) as usize;
    [CELLS[TRI[n % 6]], CELLS[TRI[(n + 3) % 6]]].iter().collect()
}

/// The wave as it was (6b23970a's `term::wave`): three cells three apart on a ten-step cycle, a step a tick.
fn wave(step: u64) -> String {
    const WAVE: [char; 10] = ['▂', '▃', '▄', '▅', '▆', '▇', '▆', '▅', '▄', '▃'];
    (0..3).map(|k| WAVE[(step as usize + k * 3) % WAVE.len()]).collect()
}

/// A quadrant going round the cell clockwise, a step every two ticks (`cargo xtask glyphs`: every upright face of the
/// four, as `─ │` are).
fn round(step: u64) -> String {
    ['▘', '▝', '▗', '▖'][(step / 2 % 4) as usize].to_string()
}

/// A recorded cell, as drawn.
#[derive(Clone)]
struct Cell {
    text: String,
    fg: vt100::Color,
    bg: vt100::Color,
    bold: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

impl Cell {
    fn of(screen: &vt100::Screen, r: u16, c: u16) -> Cell {
        match screen.cell(r, c) {
            Some(x) => Cell {
                text: match x.contents().to_string() {
                    t if t.is_empty() && !x.is_wide_continuation() => " ".into(),
                    t => t,
                },
                fg: x.fgcolor(),
                bg: x.bgcolor(),
                bold: x.bold(),
                italic: x.italic(),
                underline: x.underline(),
                inverse: x.inverse(),
            },
            None => Cell::plain(" ", vt100::Color::Default, vt100::Color::Default),
        }
    }

    fn plain(text: &str, fg: vt100::Color, bg: vt100::Color) -> Cell {
        Cell { text: text.into(), fg, bg, bold: false, italic: false, underline: false, inverse: false }
    }
}

fn row_text(screen: &vt100::Screen, r: u16) -> String {
    (0..screen.size().1).map(|c| Cell::of(screen, r, c).text).collect()
}

fn is_sign(text: &str) -> bool {
    let mut cs = text.chars();
    matches!((cs.next(), cs.next()), (Some(c), None) if CELLS.contains(&c) || c == '▲' || c == '◦')
}

/// Where a place's sign is and which of its cells are shown: its row, the sign's column, the first column shown, and
/// which way a wider sign takes its room (`left`: from the cells before it — the top line's rule).
struct Place {
    label: &'static str,
    row: u16,
    col: u16,
    from: u16,
    left: bool,
    /// A run's sign: the clock only, never the turn's stream.
    run: bool,
}

/// The three places, found on `screen` (a frame of the think).
fn places(screen: &vt100::Screen) -> Result<Vec<Place>> {
    let (rows, cols) = screen.size();
    let sign_then_word = |r: u16, c: u16| {
        is_sign(&Cell::of(screen, r, c).text)
            && Cell::of(screen, r, c + 1).text == " "
            && Cell::of(screen, r, c + 2).text.chars().all(char::is_alphabetic)
    };
    let said = || format!("the frame:\n{}", screen.contents());
    let top = (0..cols - 2)
        .rev()
        .find(|&c| sign_then_word(0, c))
        .ok_or_else(|| format!("no sign on the top line; {}", said()))?;
    let now = (0..cols - 2)
        .find(|&c| sign_then_word(rows - 1, c))
        .ok_or_else(|| format!("no sign on the now line; {}", said()))?;
    let badge = (0..rows)
        .find_map(|r| {
            let t: Vec<char> = row_text(screen, r).chars().collect();
            let at = t.windows(6).position(|w| w.iter().collect::<String>() == "watch ")?;
            is_sign(&Cell::of(screen, r, at as u16 + 6).text).then_some((r, at as u16 + 6))
        })
        .ok_or_else(|| format!("no task badge; {}", said()))?;
    Ok(vec![
        Place {
            label: "chat's top line",
            row: 0,
            col: top,
            from: cols - CROP as u16,
            left: true,
            run: false,
        },
        Place { label: "now line", row: rows - 1, col: now, from: 0, left: false, run: false },
        Place { label: "tab's badge", row: badge.0, col: badge.1, from: 0, left: false, run: true },
    ])
}

/// The turn as its script times it, in ms from the `enter`: its phases' ends, and when the stream stands still.
struct Script {
    ends: Vec<(&'static str, u64)>,
    still: (u64, u64),
}

impl Script {
    fn new() -> Script {
        let mut at = 0;
        let ends = SIGN_TURN
            .iter()
            .map(|&(name, ms)| {
                (name, {
                    at += ms;
                    at
                })
            })
            .collect::<Vec<_>>();
        // (the stall: still `STILL_AFTER` after the prose's last delta, until the read's call)
        let (mut spent, mut last) = (0, 0);
        for &(gap, _) in SIGN_STREAM.iter().cycle() {
            if spent + gap > SIGN_TURN[2].1 {
                break;
            }
            spent += gap;
            last = spent;
        }
        Script { still: (ends[1].1 + last + STILL_AFTER, ends[3].1), ends }
    }

    fn phase(&self, t: i64) -> &'static str {
        self.ends.iter().find(|(_, end)| t < *end as i64).map_or("done", |(name, _)| name)
    }

    fn still(&self, t: i64) -> bool {
        (self.still.0 as i64..self.still.1 as i64).contains(&t)
    }
}

/// What `cand` draws in a place this frame: `recorded` the cell the build drew, `t` the script's time, `step` the clock's,
/// `still_step` the clock's when the stream stood still.
fn drawn(
    cand: &Cand,
    place: &Place,
    recorded: &str,
    script: &Script,
    t: i64,
    step: u64,
    still_step: u64,
) -> String {
    let Some(at) = cand.at else { return recorded.to_string() };
    if recorded == "▲" || recorded == "◦" {
        return format!("{recorded}{}", " ".repeat(cand.width - 1));
    }
    match (!place.run && script.still(t), cand.rest) {
        (true, Some(rest)) => rest.to_string(),
        (true, None) => at(still_step),
        (false, _) => at(step),
    }
}

/// A place's `CROP` cells this frame, `sign` drawn where the recorded one stands (None: as recorded). A sign wider
/// than the recorded cell takes its room as a line would make it: from the longest run of `─` or blanks on its side
/// (`Place::left`: before it, the top line's rule; else after it), the rest of the line where it was.
fn place_cells(screen: &vt100::Screen, p: &Place, sign: Option<&str>) -> Vec<Cell> {
    let cols = screen.size().1 as usize;
    let mut row: Vec<Cell> = (0..cols as u16).map(|c| Cell::of(screen, p.row, c)).collect();
    let (from, col) = (p.from as usize, p.col as usize);
    let Some(sign) = sign.filter(|_| is_sign(&row[col].text)) else { return row[from..from + CROP].to_vec() };
    let cells: Vec<Cell> = sign.chars().map(|ch| Cell { text: ch.to_string(), ..row[col].clone() }).collect();
    let more = cells.len() - 1;
    row.splice(col..col + 1, cells);
    let (a, b) = match p.left {
        true => (from, col),
        false => (col + more + 1, (from + CROP + more).min(row.len())),
    };
    let mut best: Option<(usize, usize)> = None;
    let mut i = a;
    while i < b {
        let j = (i..b).find(|&j| row[j].text != row[i].text || row[j].bg != row[i].bg).unwrap_or(b);
        if (row[i].text == "─" || row[i].text == " ") && best.is_none_or(|(s, e)| j - i > e - s) {
            best = Some((i, j));
        }
        i = j;
    }
    match best {
        Some((s, e)) if e - s > more => drop(row.drain(s..s + more)),
        _ if p.left => return row[from + more..from + more + CROP].to_vec(),
        _ => {}
    }
    row[from..from + CROP].to_vec()
}

/// The colours a picture's own words take: the ground, faint ink, the accent.
struct Inks {
    ground: vt100::Color,
    faint: vt100::Color,
    accent: vt100::Color,
}

fn sgr(c: &Cell) -> String {
    let colour = |c: vt100::Color, fg: bool| match c {
        vt100::Color::Default => (if fg { "39" } else { "49" }).to_string(),
        vt100::Color::Idx(i) => format!("{};5;{i}", if fg { 38 } else { 48 }),
        vt100::Color::Rgb(r, g, b) => format!("{};2;{r};{g};{b}", if fg { 38 } else { 48 }),
    };
    let mut s = format!("\x1b[0;{};{}", colour(c.fg, true), colour(c.bg, false));
    for (on, n) in [(c.bold, "1"), (c.italic, "3"), (c.underline, "4"), (c.inverse, "7")] {
        if on {
            let _ = write!(s, ";{n}");
        }
    }
    s + "m"
}

/// One picture's screen: `cands` one above the other, each a title and its three places, this frame.
struct Picture<'a> {
    cands: Vec<&'a Cand>,
    places: &'a [Place],
    inks: &'a Inks,
    script: &'a Script,
}

impl Picture<'_> {
    fn size(&self) -> (u16, u16) {
        let n = self.cands.len() as u16;
        (n * 5 - 1 + 2, (1 + LABEL + CROP + 1) as u16)
    }

    fn screen(&self, frame: &vt100::Screen, t: i64, step: u64, still_step: u64) -> vt100::Parser {
        let (rows, cols) = self.size();
        let mut out = String::from("\x1b[?25l"); // (no cursor)
        let ground = Cell::plain(" ", self.inks.faint, self.inks.ground);
        for r in 0..rows {
            let _ = write!(out, "\x1b[{};1H{}{}", r + 1, sgr(&ground), " ".repeat(cols as usize));
        }
        let mut put = |r: u16, c: usize, cells: &[Cell]| {
            let _ = write!(out, "\x1b[{};{}H", r + 1, c + 1);
            for cell in cells.iter().filter(|c| !c.text.is_empty()) {
                let _ = write!(out, "{}{}", sgr(cell), cell.text);
            }
        };
        let words = |s: &str, fg: vt100::Color, bold: bool| -> Vec<Cell> {
            s.chars().map(|ch| Cell { bold, ..Cell::plain(&ch.to_string(), fg, self.inks.ground) }).collect()
        };
        let phase = match self.script.phase(t) {
            _ if t < 0 => "".to_string(),
            p => format!("{p} {:>5.1}s", t as f64 / 1000.0),
        };
        for (i, cand) in self.cands.iter().enumerate() {
            let r = 1 + i as u16 * 5;
            put(r, 1, &words(cand.title, self.inks.accent, true));
            let caption = words(&phase, self.inks.faint, false);
            put(r, cols as usize - 1 - caption.len(), &caption);
            for (k, p) in self.places.iter().enumerate() {
                let recorded = Cell::of(frame, p.row, p.col).text;
                let sign = drawn(cand, p, &recorded, self.script, t, step, still_step);
                put(r + 1 + k as u16, 1, &words(p.label, self.inks.faint, false));
                put(r + 1 + k as u16, 1 + LABEL, &place_cells(frame, p, Some(&sign)));
            }
        }
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(out.as_bytes());
        parser
    }
}

/// Frames as an animated PNG, looping: `(ms shown, RGB pixels)` — each after the first only the rectangle that changed,
/// its unchanged pixels clear over the last frame (they compress to nothing).
fn save_apng(path: &Path, (w, h): (usize, usize), frames: &[(u64, Vec<u8>)]) -> Result<()> {
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let file = std::fs::File::create(path).map_err(|e| err(&e))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::High);
    enc.set_animated(frames.len() as u32, 0).map_err(|e| err(&e))?;
    let mut wr = enc.write_header().map_err(|e| err(&e))?;
    wr.set_dispose_op(png::DisposeOp::None).map_err(|e| err(&e))?;
    let mut last: Option<&Vec<u8>> = None;
    for (ms, px) in frames {
        wr.set_frame_delay((*ms).min(u16::MAX as u64) as u16, 1000).map_err(|e| err(&e))?;
        let (x0, y0, x1, y1) = match last {
            None => (0, 0, w, h),
            Some(prev) => changed(prev, px, w, h).unwrap_or((0, 0, 1, 1)),
        };
        let blend = if last.is_some() { png::BlendOp::Over } else { png::BlendOp::Source };
        wr.set_blend_op(blend).map_err(|e| err(&e))?;
        wr.reset_frame_position().map_err(|e| err(&e))?;
        wr.set_frame_dimension((x1 - x0) as u32, (y1 - y0) as u32).map_err(|e| err(&e))?;
        wr.set_frame_position(x0 as u32, y0 as u32).map_err(|e| err(&e))?;
        let mut part = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * w + x) * 3;
                match last.is_some_and(|prev| prev[i..i + 3] == px[i..i + 3]) {
                    true => part.extend_from_slice(&[0, 0, 0, 0]),
                    false => part.extend_from_slice(&[px[i], px[i + 1], px[i + 2], 255]),
                }
            }
        }
        wr.write_image_data(&part).map_err(|e| err(&e))?;
        last = Some(px);
    }
    wr.finish().map_err(|e| err(&e))
}

/// The rectangle where `a` and `b` differ, its end exclusive — None: they don't.
fn changed(a: &[u8], b: &[u8], w: usize, h: usize) -> Option<(usize, usize, usize, usize)> {
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 3;
            if a[i..i + 3] != b[i..i + 3] {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    (x1 > 0).then_some((x0, y0, x1, y1))
}

/// One recording: the turn from its `enter`, a task running beside it when `task`.
fn record(bin: &Path, me: &Path, bg: &'static str, task: bool) -> Result<Vec<(u64, vt100::Screen)>> {
    let dir = Fresh::new("tarae-sign")?;
    let demo = dir.0.join("tarae-demo");
    demo_crate_with(&demo, false, &if task { run_files("sign") } else { Vec::new() })?;
    let config = dir.0.join("xdg");
    write(&config.join("tarae/config.toml"), &config_toml(me, "shimmer-sign"))?;
    let setup = Setup {
        bin: bin.to_path_buf(),
        files: vec!["src/main.rs".into()],
        cwd: demo,
        config,
        background: Some(bg),
        env: vec![("ZELLIJ", None)],
    };
    let mut keys: Vec<String> = Vec::new();
    if task {
        // (the task's `.tarae.toml` trusted on its card, the task started: its tab's badge breathes the whole turn)
        keys.extend(
            ["@?Trust this", "@1", "\x11", "y", "@1", ":", "task watch", "\r", "@20?knot 3 tied"]
                .map(String::from),
        );
    }
    // (the chat, follow off — its top line in sight —, the question, every frame from its `enter`)
    keys.extend(["@1", " ", "l", "@1", "\x06", "how is the map filled?", "@rec", "\r"].map(String::from));
    keys.push(format!("@{}", RECORD_MS as f64 / 1000.0));
    let (frames, _) = shot::record_at(&setup, &keys, SIZE, &dir.0.join("state"), &[])?;
    let mut out: Vec<(u64, vt100::Screen)> = Vec::new();
    for (t, s) in frames.into_iter().filter(|(t, _)| *t <= RECORD_MS) {
        match out.last_mut() {
            Some(last) if last.0 == t => last.1 = s,
            _ => out.push((t, s)),
        }
    }
    Ok(out)
}

/// The script's time 0 in the recording's: where the now line first says `writing`, the prose's start.
fn offset(frames: &[(u64, vt100::Screen)]) -> Result<i64> {
    let prose = SIGN_TURN[0].1 + SIGN_TURN[1].1;
    let (t, _) = frames
        .iter()
        .find(|(_, s)| row_text(s, s.size().0 - 1).contains("writing"))
        .ok_or("the now line never says `writing`")?;
    Ok(*t as i64 - prose as i64)
}

pub fn run(args: &[String]) -> Result<()> {
    if args.len() > 1 || args.iter().any(|a| a.starts_with('-')) {
        return Err("usage: cargo xtask sign [<out-dir>]".into());
    }
    let out = args.first().map_or_else(|| root().join("target/sign"), PathBuf::from);
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let bin = shot::build_shot(&root())?;
    let me = std::env::current_exe().map_err(|e| format!("this xtask: {e}"))?;
    let script = Script::new();
    let fonts = Fonts::load(2)?;
    let fonts_1x = Fonts::load(1)?;
    let mut ticks = String::new();
    for (bg, theme) in [("dark", "meok"), ("light", "hanji")] {
        let frames = record(&bin, &me, bg, true)?;
        let zero = offset(&frames)?;
        println!(
            "{theme}: {} frames over {} ms, the script's 0 at {zero} ms",
            frames.len(),
            frames.last().map_or(0, |f| f.0)
        );
        let think = frames.iter().find(|(t, _)| *t as i64 - zero >= 1800).ok_or("no frame in the think")?;
        let places = places(&think.1)?;
        let sign = Cell::of(&think.1, places[1].row, places[1].col);
        let rule = Cell::of(&think.1, 0, places[0].from);
        let corner = Cell::of(&think.1, SIZE.1 - 1, SIZE.0 - 1);
        let inks = Inks { ground: corner.bg, faint: rule.fg, accent: sign.fg };
        let still_step = (script.still.0 as i64 + zero).max(0) as u64 / TICK;
        let pictures: Vec<(String, Vec<&Cand>)> = CANDS
            .iter()
            .map(|c| (format!("sign-{}-{theme}.png", c.key), vec![c]))
            .chain([(format!("sign-all-{theme}.png"), CANDS.iter().collect())])
            .collect();
        for (name, cands) in pictures {
            let pic = Picture { cands, places: &places, inks: &inks, script: &script };
            let mut shown: Vec<(u64, Vec<u8>)> = Vec::new();
            let mut size = (0, 0);
            for (i, (t, frame)) in frames.iter().enumerate() {
                let screen = pic.screen(frame, *t as i64 - zero, t / TICK, still_step);
                let (w, h, px) = shot::render(screen.screen(), &fonts, 2);
                size = (w, h);
                let ms = frames.get(i + 1).map_or(HOLD_MS, |next| next.0 - t);
                // (a frame shown under `FRAME_MIN` goes to the one before: players stretch one that short to 100 ms)
                match shown.last_mut() {
                    Some(last) if last.1 == px || ms < FRAME_MIN => last.0 += ms,
                    _ => shown.push((ms, px)),
                }
            }
            save_apng(&out.join(&name), size, &shown)?;
            // (every candidate as the think draws it: a still, for looking at the layout)
            if pic.cands.len() > 1 {
                let (t, frame) = think;
                let screen = pic.screen(frame, *t as i64 - zero, t / TICK, still_step);
                let (w, h, px) = shot::render(screen.screen(), &fonts, 2);
                shot::save_png(&out.join(format!("still-{theme}.png")), w, h, &px)?;
            }
            println!("saved {} ({} frames)", out.join(&name).display(), shown.len());
        }
        strip(
            &frames,
            zero,
            &places[1],
            &script,
            still_step,
            &inks,
            &fonts_1x,
            &out.join(format!("strip-{theme}.png")),
            &mut ticks,
        )?;
    }
    write(&out.join("ticks.txt"), &ticks)?;
    write(&out.join("load.txt"), &load(&bin, &me, &script)?)?;
    write(&out.join("index.html"), &index())?;
    println!("saved {}", out.join("index.html").display());
    Ok(())
}

/// The now line's sign at every tick of each phase, a row per candidate and phase — into `path` as a picture and
/// `ticks` as text.
#[allow(clippy::too_many_arguments, reason = "a dev task's one sheet")]
fn strip(
    frames: &[(u64, vt100::Screen)],
    zero: i64,
    p: &Place,
    script: &Script,
    still_step: u64,
    inks: &Inks,
    fonts: &Fonts,
    path: &Path,
    ticks: &mut String,
) -> Result<()> {
    const TILE: usize = 4;
    let mut lines: Vec<Vec<Cell>> = Vec::new();
    let words = |s: &str| -> Vec<Cell> {
        s.chars().map(|ch| Cell::plain(&ch.to_string(), inks.faint, inks.ground)).collect()
    };
    let _ =
        writeln!(ticks, "{}", path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned()));
    for cand in &CANDS {
        let _ = writeln!(ticks, "  {}", cand.title);
        let mut start = 0;
        for &(phase, end) in &script.ends {
            let mut row = words(&format!(" {:<5.5} {:<12.12}", &cand.key[2..], phase));
            let mut text = String::new();
            for tick in (start..end).step_by(TICK as usize) {
                let at = tick as i64 + zero;
                let Some((t, frame)) = frames.iter().rev().find(|(t, _)| *t as i64 <= at) else { continue };
                let recorded = Cell::of(frame, p.row, p.col).text;
                let sign = drawn(cand, p, &recorded, script, *t as i64 - zero, t / TICK, still_step);
                let mut cells = place_cells(frame, p, Some(&sign));
                cells.truncate(p.col as usize + TILE);
                row.extend(cells.drain(p.col as usize..));
                row.push(Cell::plain(" ", inks.faint, inks.ground));
                let _ = write!(text, "{sign:<3} ");
            }
            let _ = writeln!(ticks, "    {phase:<12} {text}");
            lines.push(row);
            start = end;
        }
        lines.push(Vec::new());
    }
    let (rows, cols) = (lines.len() as u16, lines.iter().map(Vec::len).max().unwrap_or(1) as u16);
    let mut out = String::from("\x1b[?25l"); // (no cursor)
    for (r, line) in lines.iter().enumerate() {
        let _ = write!(
            out,
            "\x1b[{};1H{}{}\x1b[{};1H",
            r + 1,
            sgr(&Cell::plain(" ", inks.faint, inks.ground)),
            " ".repeat(cols as usize),
            r + 1
        );
        for c in line.iter().filter(|c| !c.text.is_empty()) {
            let _ = write!(out, "{}{}", sgr(c), c.text);
        }
    }
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(out.as_bytes());
    let (w, h, px) = shot::render(parser.screen(), fonts, 1);
    shot::save_png(path, w, h, &px)
}

/// Frames a second, by phase: tarae's (a recording with no task beside the turn, so only the turn draws), and how often
/// each candidate's sign changes and how many of its cells — what it would ask of frames if nothing else moved.
fn load(bin: &Path, me: &Path, script: &Script) -> Result<String> {
    let frames = record(bin, me, "dark", false)?;
    let zero = offset(&frames)?;
    let p = Place { label: "", row: SIZE.1 - 1, col: 0, from: 0, left: false, run: false };
    let think = frames.iter().find(|(t, _)| *t as i64 - zero >= 1800).ok_or("no frame in the think")?;
    let p = Place { col: places_now(&think.1)?, ..p };
    let still_step = (script.still.0 as i64 + zero).max(0) as u64 / TICK;
    let mut out = String::from(
        "Frames a second by phase (meok, 100×30, no task beside the turn; perf profile `shot`).\n\
         tarae: the frames it drew. Each candidate: frames where its sign changed / cells it rewrote, a second —\n\
         what the sign alone would ask if nothing else moved (every candidate steps on the 100 ms TICK tarae\n\
         already draws while a turn runs, so none adds a frame of its own).\n\n",
    );
    let _ = write!(out, "{:<16}", "");
    for &(phase, _) in &script.ends {
        let _ = write!(out, "{phase:>13}");
    }
    out.push('\n');
    let mut start = 0i64;
    let mut row = format!("{:<16}", "tarae");
    for &(_, end) in &script.ends {
        let n = frames.iter().filter(|(t, _)| (start..end as i64).contains(&(*t as i64 - zero))).count();
        let _ = write!(row, "{:>13.1}", n as f64 * 1000.0 / (end as i64 - start) as f64);
        start = end as i64;
    }
    out += &row;
    out.push('\n');
    for cand in &CANDS {
        let mut row = format!("{:<16}", cand.key);
        let mut start = 0i64;
        for &(_, end) in &script.ends {
            let (mut changes, mut cells, mut last): (u32, u32, Option<String>) = (0, 0, None);
            for (t, frame) in frames.iter().filter(|(t, _)| (start..end as i64).contains(&(*t as i64 - zero)))
            {
                let recorded = Cell::of(frame, p.row, p.col).text;
                let sign = drawn(cand, &p, &recorded, script, *t as i64 - zero, t / TICK, still_step);
                if let Some(prev) = &last
                    && *prev != sign
                {
                    changes += 1;
                    cells += prev.chars().zip(sign.chars()).filter(|(a, b)| a != b).count() as u32;
                }
                last = Some(sign);
            }
            let secs = (end as i64 - start) as f64 / 1000.0;
            let _ =
                write!(row, "{:>13}", format!("{:.1} / {:.1}", changes as f64 / secs, cells as f64 / secs));
            start = end as i64;
        }
        out += &row;
        out.push('\n');
    }
    Ok(out)
}

fn places_now(screen: &vt100::Screen) -> Result<u16> {
    let (rows, cols) = screen.size();
    (0..cols - 2)
        .find(|&c| {
            is_sign(&Cell::of(screen, rows - 1, c).text) && Cell::of(screen, rows - 1, c + 1).text == " "
        })
        .ok_or_else(|| format!("no sign on the now line:\n{}", screen.contents()))
}

/// The page that shows them side by side.
fn index() -> String {
    let mut rows = String::new();
    for key in std::iter::once("all").chain(CANDS.iter().map(|c| c.key)) {
        let _ = writeln!(
            rows,
            "<div class=row><img src=\"sign-{key}-meok.png\"><img src=\"sign-{key}-hanji.png\"></div>"
        );
    }
    format!(
        "<!doctype html><meta charset=utf-8><title>tarae — the working sign's candidates</title>\n\
         <style>body{{background:#808080;margin:16px;font:14px Menlo,monospace}}.row{{display:flex;gap:16px;\
         margin-bottom:16px;align-items:flex-start}}img{{width:50%;max-width:{}px;image-rendering:auto}}</style>\n{rows}",
        (1 + LABEL + CROP + 1) * 9 * 2 + 56
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the APNG writer wrote comes back frame by frame as it was given: each part laid where it says, at its delay.
    #[test]
    fn an_animation_reads_back_as_its_frames() {
        let dir = crate::scenes::Fresh::new("tarae-sign-test").unwrap();
        let (w, h) = (5, 4);
        let mut a = vec![10u8; w * h * 3];
        let frames: Vec<(u64, Vec<u8>)> = (0..3u8)
            .map(|k| {
                // (two pixels apart: the rectangle between them unchanged, clear in the frame)
                for i in [((1 + k as usize) * w + 2) * 3, k as usize * 3] {
                    a[i..i + 3].copy_from_slice(&[200, k, 7]);
                }
                (100 + k as u64, a.clone())
            })
            .collect();
        let path = dir.0.join("a.png");
        save_apng(&path, (w, h), &frames).unwrap();
        let mut reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()))
            .read_info()
            .unwrap();
        let mut canvas = vec![0u8; w * h * 3];
        let mut buf = vec![0u8; reader.output_buffer_size().unwrap()];
        for (ms, want) in &frames {
            reader.next_frame(&mut buf).unwrap();
            let fc = *reader.info().frame_control().unwrap();
            assert_eq!((fc.delay_num, fc.delay_den), (*ms as u16, 1000));
            for y in 0..fc.height as usize {
                for x in 0..fc.width as usize {
                    let (cx, cy) = (fc.x_offset as usize + x, fc.y_offset as usize + y);
                    let (s, d) = ((y * fc.width as usize + x) * 4, (cy * w + cx) * 3);
                    if buf[s + 3] == 255 {
                        canvas[d..d + 3].copy_from_slice(&buf[s..s + 3]);
                    }
                }
            }
            assert_eq!(&canvas, want);
        }
    }

    /// A wider sign takes its room from the line's rule beside it, the words after it where they were.
    #[test]
    fn a_wider_sign_takes_its_room_from_the_rule() {
        let mut parser = vt100::Parser::new(2, CROP as u16 + 4, 0);
        let rule = "─".repeat(30);
        parser.process(format!("{rule} ▃ thinking 1.9s ─\r\n● chat  ▃ thinking   main").as_bytes());
        let text = |cells: Vec<Cell>| cells.iter().map(|c| c.text.as_str()).collect::<String>();
        let top = Place { label: "", row: 0, col: 31, from: 4, left: true, run: false };
        let shown = text(place_cells(parser.screen(), &top, Some("▂▅▇")));
        assert!(shown.trim_end().ends_with("─ ▂▅▇ thinking 1.9s ─"), "{shown}");
        assert_eq!(shown.chars().count(), CROP);
        let now = Place { label: "", row: 1, col: 8, from: 0, left: false, run: false };
        let shown = text(place_cells(parser.screen(), &now, Some("▃▆")));
        // (the status line's gap before its right side gives way, not the room between the now line's words)
        assert!(shown.starts_with("● chat  ▃▆ thinking   main "), "{shown}");
        assert_eq!(shown.chars().count(), CROP);
    }
}
