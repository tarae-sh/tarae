//! A recording of tarae's screen, frame by frame, in the homepage player's format (`tarae.sh`'s `player.js`,
//! `.sb/home-plan.md`) — the hero's (`homepage.rs`) and any scene's (`scenes --record`): one encoder for both.
//! Beside it what a recording is judged on from stills (`sheet`: its frames every ~2 s and at each mark, the time
//! under each) and in motion (`page`: one HTML file that plays it, nothing fetched).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::{Value, json};

use crate::Result;
use crate::scenes::docshots::Bg;
use crate::shot::{self, Fonts, Rgb};

/// Keyframe interval in milliseconds (~10 s, so scrubbing never replays from `t = 0`).
pub(crate) const KEYFRAME_MS: u64 = 10_000;
/// Sampling bucket width in milliseconds (keeps the latest screen in each 200 ms window).
pub(crate) const FRAME_STEP_MS: u64 = 200;
/// A contact sheet's step: a frame every `SHEET_EVERY_MS` (and one at each mark).
pub(crate) const SHEET_EVERY_MS: u64 = 2_000;
/// A contact sheet's pages (`sheet-01.png` …) hold this many frames each — the hero's beats sheet's 2×3, which reads
/// at its size; `sheet.png` holds them all.
const PAGE: usize = 6;

/// The player's default foreground and background on `bg` (`[fg, bg]`): meok's ink and paper, hanji's.
pub(crate) fn ground(bg: Bg) -> (Rgb, Rgb) {
    match bg {
        Bg::Dark => ([232, 227, 216], [7, 8, 11]),
        Bg::Light => ([52, 50, 45], [245, 241, 232]),
    }
}

/// Samples raw frames by keeping the first frame (`t = 0`), every time in `keep` (the beats), and the latest frame in
/// each `FRAME_STEP_MS` (200 ms) bucket. If multiple raw frames share the same millisecond timestamp, keeps the last one.
pub(crate) fn sample(raw: &[(u64, vt100::Screen)], keep: &[u64]) -> Vec<(u64, vt100::Screen)> {
    let beat_times: HashSet<u64> = keep.iter().copied().collect();
    let mut dedup: Vec<(u64, vt100::Screen)> = Vec::with_capacity(raw.len());
    for (t, screen) in raw {
        if let Some((last_t, last_screen)) = dedup.last_mut()
            && *last_t == *t
        {
            *last_screen = screen.clone();
            continue;
        }
        dedup.push((*t, screen.clone()));
    }
    let mut out = Vec::new();
    for (i, (t, screen)) in dedup.iter().enumerate() {
        let is_first = i == 0;
        let is_beat = beat_times.contains(t);
        let is_bucket_end =
            dedup.get(i + 1).is_none_or(|(next_t, _)| next_t / FRAME_STEP_MS > t / FRAME_STEP_MS);
        if is_first || is_beat || is_bucket_end {
            out.push((*t, screen.clone()));
        }
    }
    out
}

/// Each cell's text, row by row (a wide character's second cell empty) — what `none_of_yours` reads.
pub(crate) fn screen_cells(screen: &vt100::Screen) -> Vec<Vec<String>> {
    let (rows, cols) = screen.size();
    let text = |r, c| screen.cell(r, c).map(|cell| cell.contents().to_string()).unwrap_or_default();
    (0..rows).map(|r| (0..cols).map(|c| text(r, c)).collect()).collect()
}

// ── Keyframed cell-run encoder ───────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Style {
    fg: u16,
    bg: u16,
    flags: u8,
}

#[derive(Clone, PartialEq, Eq)]
struct CellState {
    style: u16,
    ch: String,
    wide: u8, // 0 = normal (1 col), 1 = wide start (2 cols), 2 = wide continuation
}

/// Encodes `frames` and `beats` (each one's time and label) into the homepage player's JSON format
/// (`.sb/home-plan.md`):
/// - `palette`: array of `"#rrggbb"` strings (index 0 = default background, index 1 = default foreground)
/// - `styles`: array of `[fgPaletteIdx, bgPaletteIdx, flags]` (`1 = bold`, `2 = italic`, `4 = underline`)
/// - `beats`: `[{"at": ms, "label": "..."}, ...]`
/// - `frames`: keyframes at `t = 0` and every `KEYFRAME_MS` (~10 s), with changed-row `[col, styleIndex, text]`
///   runs between them. Identical non-beat frames are deduplicated so idle moments add zero bytes, while beat
///   timestamps are always preserved so seeking to `beat.at` lands on that exact beat.
pub(crate) fn encode(
    frames: &[(u64, vt100::Screen)],
    beats: &[(u64, &str)],
    default_fg: Rgb,
    default_bg: Rgb,
) -> Value {
    let beat_times: HashSet<u64> = beats.iter().map(|(t, _)| *t).collect();
    let mut palette: Vec<String> = Vec::new();
    let mut color_idx: HashMap<Rgb, u16> = HashMap::new();
    let mut intern_color = |rgb: Rgb| -> u16 {
        if let Some(&i) = color_idx.get(&rgb) {
            return i;
        }
        let i = palette.len() as u16;
        palette.push(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]));
        color_idx.insert(rgb, i);
        i
    };
    intern_color(default_bg);
    intern_color(default_fg);

    let mut styles: Vec<[u16; 3]> = Vec::new();
    let mut style_idx: HashMap<Style, u16> = HashMap::new();
    let mut intern_style = |s: Style| -> u16 {
        if let Some(&i) = style_idx.get(&s) {
            return i;
        }
        let i = styles.len() as u16;
        styles.push([s.fg, s.bg, u16::from(s.flags)]);
        style_idx.insert(s, i);
        i
    };
    intern_style(Style { fg: 1, bg: 0, flags: 0 });

    let (rows, cols) = frames.first().map_or((0, 0), |(_, s)| s.size());

    let mut out_frames: Vec<Value> = Vec::new();
    let mut prev_grid: Option<Vec<Vec<CellState>>> = None;
    let mut prev_cursor: Option<Option<[u16; 2]>> = None;
    let mut last_key_t = 0u64;

    for (idx, (t, screen)) in frames.iter().enumerate() {
        let cur_grid =
            extract_grid(screen, rows, cols, default_fg, default_bg, &mut intern_color, &mut intern_style);
        let cursor = (!screen.hide_cursor()).then(|| {
            let (cy, cx) = screen.cursor_position();
            [cx, cy]
        });
        let is_key = idx == 0 || *t >= last_key_t + KEYFRAME_MS;
        let is_beat = beat_times.contains(t);

        let mut row_map = serde_json::Map::new();
        for r in 0..usize::from(rows) {
            let runs = match &prev_grid {
                Some(prev) if !is_key => diff_row_runs(&prev[r], &cur_grid[r]),
                _ => full_row_runs(&cur_grid[r]),
            };
            if !runs.is_empty() {
                let encoded: Vec<Value> = runs.into_iter().map(|(c, s, txt)| json!([c, s, txt])).collect();
                row_map.insert(r.to_string(), Value::Array(encoded));
            }
        }
        if !is_key && !is_beat && row_map.is_empty() && prev_cursor == Some(cursor) {
            continue;
        }
        if is_key {
            last_key_t = *t;
        }
        let mut obj = serde_json::Map::new();
        obj.insert("t".into(), json!(t));
        if is_key {
            obj.insert("key".into(), json!(true));
        }
        if let Some([cx, cy]) = cursor {
            obj.insert("cursor".into(), json!([cx, cy]));
        }
        obj.insert("rows".into(), Value::Object(row_map));
        out_frames.push(Value::Object(obj));
        prev_grid = Some(cur_grid);
        prev_cursor = Some(cursor);
    }

    let out_beats: Vec<Value> = beats.iter().map(|(at, label)| json!({"at": at, "label": label})).collect();

    json!({
        "cols": cols,
        "rows": rows,
        "palette": palette,
        "styles": styles,
        "beats": out_beats,
        "frames": out_frames,
    })
}

fn extract_grid(
    screen: &vt100::Screen,
    rows: u16,
    cols: u16,
    default_fg: Rgb,
    default_bg: Rgb,
    intern_color: &mut impl FnMut(Rgb) -> u16,
    intern_style: &mut impl FnMut(Style) -> u16,
) -> Vec<Vec<CellState>> {
    let mut out = Vec::with_capacity(usize::from(rows));
    for r in 0..rows {
        let mut row = Vec::with_capacity(usize::from(cols));
        for c in 0..cols {
            let Some(cell) = screen.cell(r, c) else {
                row.push(CellState { style: 0, ch: " ".into(), wide: 0 });
                continue;
            };
            if cell.is_wide_continuation() {
                let style = row.last().map_or(0, |prev: &CellState| prev.style);
                row.push(CellState { style, ch: String::new(), wide: 2 });
                continue;
            }
            let (fg_rgb, bg_rgb) = {
                let f = shot::color(cell.fgcolor(), default_fg);
                let b = shot::color(cell.bgcolor(), default_bg);
                if cell.inverse() { (b, f) } else { (f, b) }
            };
            let flags =
                u8::from(cell.bold()) | (u8::from(cell.italic()) << 1) | (u8::from(cell.underline()) << 2);
            let style = intern_style(Style { fg: intern_color(fg_rgb), bg: intern_color(bg_rgb), flags });
            let text = cell.contents();
            let ch = if text.is_empty() { " ".into() } else { text.to_string() };
            let wide = u8::from(cell.is_wide());
            row.push(CellState { style, ch, wide });
        }
        out.push(row);
    }
    out
}

fn full_row_runs(row: &[CellState]) -> Vec<(u16, u16, String)> {
    let mut runs: Vec<(u16, u16, String)> = Vec::new();
    let mut c = 0usize;
    while c < row.len() {
        if row[c].wide == 2 || (row[c].style == 0 && row[c].ch == " ") {
            c += 1;
            continue;
        }
        let start = c as u16;
        let style = row[c].style;
        let mut txt = String::new();
        while c < row.len() && row[c].style == style {
            if row[c].wide == 2 {
                c += 1;
                continue;
            }
            // Only break a default-style run when 8+ consecutive default spaces follow
            if style == 0 && row[c].ch == " " {
                let mut sp = 0;
                while c + sp < row.len() && row[c + sp].style == 0 && row[c + sp].ch == " " {
                    sp += 1;
                }
                if sp >= 8 {
                    break;
                }
            }
            txt.push_str(&row[c].ch);
            c += 1;
        }
        runs.push((start, style, txt));
    }
    runs
}

fn diff_row_runs(prev: &[CellState], cur: &[CellState]) -> Vec<(u16, u16, String)> {
    let n = cur.len().min(prev.len());
    let mut dirty = vec![false; n];
    for i in 0..n {
        if cur[i] != prev[i] {
            dirty[i] = true;
            if (cur[i].wide == 2 || prev[i].wide == 2) && i > 0 {
                dirty[i - 1] = true;
            }
            if (cur[i].wide == 1 || prev[i].wide == 1) && i + 1 < n {
                dirty[i + 1] = true;
            }
        }
    }
    // Bridge short (<= 4 cell) clean gaps of the same style to avoid JSON array framing overhead.
    let mut i = 0;
    while i < n {
        if dirty[i] {
            let s = cur[i].style;
            let mut gap = 1;
            while i + gap < n && !dirty[i + gap] && cur[i + gap].style == s && gap <= 4 {
                gap += 1;
            }
            if gap > 1 && i + gap < n && dirty[i + gap] && cur[i + gap].style == s {
                for g in 1..gap {
                    dirty[i + g] = true;
                }
            }
        }
        i += 1;
    }
    let mut runs: Vec<(u16, u16, String)> = Vec::new();
    let mut i = 0;
    while i < n {
        if !dirty[i] || cur[i].wide == 2 {
            i += 1;
            continue;
        }
        let start = i as u16;
        let style = cur[i].style;
        let mut txt = String::new();
        while i < n && dirty[i] && cur[i].style == style {
            if cur[i].wide != 2 {
                txt.push_str(&cur[i].ch);
            }
            i += 1;
        }
        runs.push((start, style, txt));
    }
    runs
}

// ── The contact sheet ────────────────────────────────────────────────────────

/// The frames a sheet shows, by their index in `times` (each frame's ms, in order), with what's written under each: the
/// screen as seen every `every` ms (the last frame drawn by then), at each of `marks` (its time and what it was), and
/// the last frame. A frame picked twice is shown once, under the first time it was picked, its marks joined.
pub(crate) fn sheet_picks(times: &[u64], marks: &[(u64, String)], every: u64) -> Vec<(usize, String)> {
    let Some(&end) = times.last() else { return Vec::new() };
    let mut wanted: Vec<(u64, Option<&str>)> = (0..=end / every).map(|k| (k * every, None)).collect();
    wanted.extend(marks.iter().map(|(t, what)| (*t, Some(what.as_str()))));
    wanted.push((end, None));
    wanted.sort_by_key(|w| w.0);
    let mut picks: Vec<(usize, u64, Vec<&str>)> = Vec::new();
    for (t, what) in wanted {
        let at = times.partition_point(|&f| f <= t).saturating_sub(1);
        match picks.iter_mut().find(|p| p.0 == at) {
            Some(p) => p.2.extend(what),
            None => picks.push((at, t, what.into_iter().collect())),
        }
    }
    let said = |t: u64, what: &[&str]| {
        let time = format!("{:5.1} s", t as f64 / 1000.0);
        if what.is_empty() { time } else { format!("{time}  {}", what.join(" · ")) }
    };
    picks.into_iter().map(|(at, t, what)| (at, said(t, &what))).collect()
}

/// The contact sheet of `frames` at `out` (`sheet_picks`: every `SHEET_EVERY_MS` and at each of `marks`), two
/// across, each frame's time and marks written under it; past `PAGE` frames also in pages of `PAGE` beside it
/// (`sheet-01.png` …), which read at their size where the whole sheet is scaled down. What it returns: the frames shown.
pub(crate) fn sheet(
    frames: &[(u64, vt100::Screen)],
    marks: &[(u64, String)],
    fonts: &Fonts,
    out: &Path,
) -> Result<usize> {
    let times: Vec<u64> = frames.iter().map(|f| f.0).collect();
    let picks = sheet_picks(&times, marks, SHEET_EVERY_MS);
    let pictures: Vec<Picture> = picks
        .iter()
        .map(|(at, said)| {
            let screen = &frames[*at].1;
            let (_, cols) = screen.size();
            labelled(shot::render(screen, fonts, 1), said, usize::from(cols), fonts)
        })
        .collect();
    pages(&pictures, out)
}

/// RGB pixels: width, height, three bytes a pixel.
pub(crate) type Picture = (usize, usize, Vec<u8>);

/// `picture` with `said` written under it, `cols` cells wide (a screen of one row: drawn by the shots' own renderer,
/// in their font).
pub(crate) fn labelled(picture: Picture, said: &str, cols: usize, fonts: &Fonts) -> Picture {
    let cols = cols.min(usize::from(u16::MAX)) as u16;
    let mut caption = vt100::Parser::new(1, cols, 0);
    caption.process(b"\x1b[?25l");
    caption.process(said.chars().take(usize::from(cols)).collect::<String>().as_bytes());
    stacked(&[picture, shot::render(caption.screen(), fonts, 1)])
}

/// A contact sheet of `pictures` at `out`, two across; past `PAGE` also in pages of `PAGE` beside it (`sheet-01.png`
/// …). What it returns: the pictures shown.
pub(crate) fn pages(pictures: &[Picture], out: &Path) -> Result<usize> {
    let stem = out.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let dir = out.parent().unwrap_or(Path::new("."));
    // (a page an earlier, longer take left: gone, so every page there is this take's)
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(&format!("{stem}-")) && name.ends_with(".png") {
            let _ = std::fs::remove_file(e.path());
        }
    }
    grid(pictures, out)?;
    if pictures.len() > PAGE {
        for (k, page) in pictures.chunks(PAGE).enumerate() {
            grid(page, &dir.join(format!("{stem}-{:02}.png", k + 1)))?;
        }
    }
    Ok(pictures.len())
}

/// `pictures` (RGB, `(w, h, px)`) one under another, left-aligned, a mid grey where one is narrower.
fn stacked(pictures: &[(usize, usize, Vec<u8>)]) -> (usize, usize, Vec<u8>) {
    let w = pictures.iter().map(|p| p.0).max().unwrap_or(0);
    let h = pictures.iter().map(|p| p.1).sum();
    let mut px = vec![0x80u8; w * h * 3];
    let mut y0 = 0;
    for (pw, ph, p) in pictures {
        for y in 0..*ph {
            let at = ((y0 + y) * w) * 3;
            px[at..at + pw * 3].copy_from_slice(&p[y * pw * 3..(y + 1) * pw * 3]);
        }
        y0 += ph;
    }
    (w, h, px)
}

/// `pictures` two across, a gap between (the gaps a mid grey: apart from a dark and a light theme alike), at `out`.
fn grid(pictures: &[(usize, usize, Vec<u8>)], out: &Path) -> Result<()> {
    if pictures.is_empty() {
        return Ok(());
    }
    let (fw, fh) =
        (pictures.iter().map(|p| p.0).max().unwrap_or(0), pictures.iter().map(|p| p.1).max().unwrap_or(0));
    let gap = shot::PAD;
    let across = pictures.len().min(2);
    let down = pictures.len().div_ceil(across);
    let (w, h) = (across * fw + (across - 1) * gap, down * fh + (down - 1) * gap);
    let mut sheet = vec![0x80u8; w * h * 3];
    for (idx, (pw, ph, px)) in pictures.iter().enumerate() {
        let (x0, y0) = ((idx % across) * (fw + gap), (idx / across) * (fh + gap));
        for y in 0..*ph {
            let row = &px[y * pw * 3..(y + 1) * pw * 3];
            let at = ((y0 + y) * w + x0) * 3;
            sheet[at..at + row.len()].copy_from_slice(row);
        }
    }
    shot::save_png(out, w, h, &sheet)
}

// ── The page that plays it ───────────────────────────────────────────────────

/// One HTML file that plays `rec` (`encode`'s value): the player's script and the recording inlined, nothing fetched —
/// no font file either (a monospace stack) —, play and pause, a scrubber, the time; on a phone too (tap the screen to
/// play or pause, the screen fitted to the width, `zoom` its cells readable and panned).
pub(crate) fn page(rec: &Value, title: &str) -> Result<String> {
    let json = serde_json::to_string(rec).map_err(|e| e.to_string())?;
    // (inside `<script>`: a `<` only ever in a string, so `<` there reads the same and no `</script>` can end it)
    let json = json.replace('<', "\\u003c");
    let title = title.replace('&', "&amp;").replace('<', "&lt;");
    Ok(PAGE_HTML
        .replace("{{title}}", &title)
        .replace("{{script}}", include_str!("recording/play.js"))
        .replace("{{rec}}", &json))
}

const PAGE_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{{title}}</title>
<style>
  html, body { margin: 0; background: var(--bg, #07080b); color: var(--fg, #e8e3d8); }
  body { font: 15px/1.4 -apple-system, system-ui, sans-serif; display: flex; flex-direction: column; min-height: 100vh; }
  #screen { overflow-x: auto; padding: 8px; -webkit-overflow-scrolling: touch; }
  #grid {
    position: relative; white-space: pre; cursor: pointer;
    font-family: ui-monospace, "SF Mono", Menlo, "JetBrains Mono", "DejaVu Sans Mono", Consolas, monospace;
    font-size: calc(var(--cell-w) / 0.6); line-height: 1.45; width: calc(var(--cell-w) * var(--cols));
  }
  .r { display: flex; height: 1.45em; }
  .r > span { display: flex; flex: none; }
  .c { display: inline-block; flex: none; width: calc(var(--cell-w) * var(--n, 1)); text-align: center; overflow: visible; }
  .c.w { width: calc(var(--cell-w) * 2); }
  .cur { position: absolute; width: var(--cell-w); height: 1.45em; outline: 1px solid currentColor; opacity: .6; pointer-events: none; }
  #caption { min-height: 1.4em; padding: 4px 12px; opacity: .85; }
  #bar { position: sticky; bottom: 0; display: flex; gap: 10px; align-items: center; padding: 8px 12px calc(8px + env(safe-area-inset-bottom)); background: var(--bg, #07080b); }
  #bar button { font: inherit; color: inherit; background: none; border: 1px solid currentColor; border-radius: 6px; min-width: 64px; min-height: 40px; opacity: .85; }
  #seek { flex: 1; min-height: 40px; accent-color: currentColor; }
  #time { font-variant-numeric: tabular-nums; white-space: nowrap; opacity: .85; }
</style>
</head>
<body>
<div id="screen"><div id="grid"></div></div>
<div id="caption"></div>
<div id="bar">
  <button id="play" type="button">play</button>
  <input id="seek" type="range" min="0" value="0" step="100" aria-label="time">
  <span id="time"></span>
  <button id="zoom" type="button">zoom</button>
</div>
<script id="rec" type="application/json">{{rec}}</script>
<script>
{{script}}
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(rows: u16, cols: u16, bytes: &[u8]) -> vt100::Screen {
        let mut p = vt100::Parser::new(rows, cols, 0);
        p.process(bytes);
        p.screen().clone()
    }

    /// The player's decoding (`player.js`'s `applyFrame`/`patchRow`, narrow cells): each frame's text, row by row.
    fn played(v: &Value) -> Vec<(u64, Vec<String>)> {
        let (rows, cols) = (v["rows"].as_u64().unwrap() as usize, v["cols"].as_u64().unwrap() as usize);
        let mut grid = vec![vec![' '; cols]; rows];
        let mut out = Vec::new();
        for f in v["frames"].as_array().unwrap() {
            if f["key"] == true {
                grid = vec![vec![' '; cols]; rows];
            }
            for (r, runs) in f["rows"].as_object().unwrap() {
                let row = &mut grid[r.parse::<usize>().unwrap()];
                for run in runs.as_array().unwrap() {
                    let start = run[0].as_u64().unwrap() as usize;
                    for (k, ch) in run[2].as_str().unwrap().chars().enumerate() {
                        row[start + k] = ch;
                    }
                }
            }
            let text = grid.iter().map(|r| r.iter().collect::<String>().trim_end().to_string()).collect();
            out.push((f["t"].as_u64().unwrap(), text));
        }
        out
    }

    /// A frame sequence in, the player's format out: a frame a second for 25 s, one row's count changing each time —
    /// what the player draws from it is each screen as it was, and only the frames at 0, 10 and 20 s are keyframes
    /// (whole), the rest that row's change alone.
    #[test]
    fn a_recording_plays_back_each_screen_with_a_keyframe_every_ten_seconds() {
        let frames: Vec<(u64, vt100::Screen)> = (0..=25u64)
            .map(|s| {
                let text = format!(
                    "\x1b[1;38;2;244;128;90mtarae\x1b[0m waits\r\n{s:>2} s\r\n\x1b[48;2;40;44;52m{}",
                    "ok"
                );
                (s * 1000, screen(3, 20, text.as_bytes()))
            })
            .collect();
        let v = encode(&frames, &[(12_000, "the card arrives")], [232, 227, 216], [7, 8, 11]);
        assert_eq!((v["cols"].as_u64(), v["rows"].as_u64()), (Some(20), Some(3)));
        assert_eq!(v["beats"], json!([{"at": 12_000, "label": "the card arrives"}]));
        let fs = v["frames"].as_array().unwrap();
        let keys: Vec<u64> =
            fs.iter().filter(|f| f["key"] == true).map(|f| f["t"].as_u64().unwrap()).collect();
        assert_eq!(keys, [0, 10_000, 20_000], "a keyframe at the start and every 10 s");
        for f in fs.iter().filter(|f| f["key"] != true) {
            let rows: Vec<&String> = f["rows"].as_object().unwrap().keys().collect();
            assert_eq!(rows, ["1"], "at {}: only the row that changed", f["t"]);
        }
        let played = played(&v);
        assert_eq!(played.len(), frames.len(), "every frame changed something: none dropped");
        for ((t, screen), (pt, text)) in frames.iter().zip(&played) {
            let want: Vec<String> = screen.contents().lines().map(|l| l.trim_end().to_string()).collect();
            assert_eq!(
                (t, &text[..want.len()]),
                (pt, &want[..]),
                "the screen at {t} ms as the player draws it"
            );
        }
        // (the styles: tarae's word bold in its colour, `ok` on its ground — by the palette's index)
        let (pal, styles) = (v["palette"].as_array().unwrap(), v["styles"].as_array().unwrap());
        let style = |r: &str, col: u64| {
            let run = fs[0]["rows"][r].as_array().unwrap().iter().find(|run| run[0] == col).unwrap();
            styles[run[1].as_u64().unwrap() as usize].clone()
        };
        let tarae = style("0", 0);
        assert_eq!((&pal[tarae[0].as_u64().unwrap() as usize], &tarae[2]), (&json!("#f4805a"), &json!(1)));
        let ok = style("2", 0);
        assert_eq!(pal[ok[1].as_u64().unwrap() as usize], "#282c34");
    }

    #[test]
    fn encodes_keyframes_and_changed_row_runs() {
        let s1 = screen(2, 6, b"\x1b[38;2;244;128;90mhi\x1b[0m all\r\nrow2");
        let s2 = screen(2, 6, b"\x1b[38;2;244;128;90mhi\x1b[0m all\r\nrow2!");
        let frames = vec![(0, s1.clone()), (100, s1.clone()), (200, s2.clone()), (KEYFRAME_MS + 500, s2)];
        let v = encode(&frames, &[(0, "")], [232, 227, 216], [7, 8, 11]);

        assert_eq!(v["cols"], 6);
        assert_eq!(v["rows"], 2);
        assert_eq!(v["palette"][0], "#07080b");
        assert_eq!(v["palette"][1], "#e8e3d8");
        assert!(v["palette"].as_array().unwrap().contains(&json!("#f4805a")));

        let fs = v["frames"].as_array().unwrap();
        assert_eq!(fs.len(), 3, "duplicate idle frame at t=100 is skipped");
        assert_eq!(fs[0]["key"], true);
        assert!(fs[0]["rows"]["0"].is_array() && fs[0]["rows"]["1"].is_array());
        assert!(fs[1].get("key").is_none(), "t=200 is a delta frame");
        assert!(fs[1]["rows"].get("0").is_none(), "unchanged row 0 omitted in delta");
        assert_eq!(fs[1]["rows"]["1"], json!([[4, 0, "!"]]), "only changed cell at col 4 is emitted");
        assert_eq!(fs[2]["key"], true, "t=10500 >= KEYFRAME_MS is a keyframe");
    }

    #[test]
    fn sample_keeps_latest_per_bucket_and_all_beats() {
        let s1 = screen(2, 6, b"first");
        let s2 = screen(2, 6, b"latest");
        // Burst in bucket 1 (200..399 ms): 210 ms has `s1`, 350 ms has `s2`, plus a beat at 240 ms (`s1`),
        // followed by a hold until 2000 ms (`s2`).
        let raw = vec![
            (0, s1.clone()),
            (210, s1.clone()),
            (240, s1.clone()),
            (350, s2.clone()),
            (2000, s2.clone()),
        ];
        let sampled = sample(&raw, &[0, 240, 2000]);
        let times: Vec<u64> = sampled.iter().map(|(t, _)| *t).collect();
        assert_eq!(
            times,
            vec![0, 240, 350, 2000],
            "keeps t=0, beat at 240, latest in bucket 1 at 350 (drops 210), and 2000"
        );
        assert_eq!(sampled[2].1.contents(), "latest", "bucket 1 retains the latest screen before hold");

        let beats = [(0, ""), (240, ""), (2000, "")];
        let v = encode(&sampled, &beats, [232, 227, 216], [7, 8, 11]);
        let emitted_times: Vec<u64> =
            v["frames"].as_array().unwrap().iter().map(|f| f["t"].as_u64().unwrap()).collect();
        for b in v["beats"].as_array().unwrap() {
            let at = b["at"].as_u64().unwrap();
            assert!(
                emitted_times.contains(&at),
                "every beat.at ({at}) has an emitted frame in frames ({emitted_times:?})"
            );
        }
    }

    /// A sheet shows the screen every 2 s (the last frame drawn by then), each mark's, and the last — a frame once.
    #[test]
    fn a_sheet_picks_a_frame_every_two_seconds_and_at_each_mark() {
        let times = [0, 300, 1900, 2100, 5200, 5900];
        let marks = [(2100, "?fix asks".to_string()), (5300, "!Its reason".to_string())];
        let picks = sheet_picks(&times, &marks, 2000);
        let said: Vec<(usize, &str)> = picks.iter().map(|(i, s)| (*i, s.as_str())).collect();
        // (4 s shows the frame of 2.1 s again: it's that picture, once)
        assert_eq!(
            said,
            [
                (0, "  0.0 s"),
                (2, "  2.0 s"),
                (3, "  2.1 s  ?fix asks"),
                (4, "  5.3 s  !Its reason"),
                (5, "  5.9 s")
            ]
        );
    }
}
