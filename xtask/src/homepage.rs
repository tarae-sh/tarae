//! `cargo xtask homepage [<out-dir>]` — records the homepage hero scene ("Three Claudes, one repository") in both
//! themes (`meok` and `hanji`) at 120×32 cells and writes:
//! - `<out-dir>/hero-meok.json` and `<out-dir>/hero-hanji.json` — palette, styles, beats, and keyframed cell-run
//!   deltas for the homepage player (`tarae.sh`)
//! - `<out-dir>/hero-meok.png` and `<out-dir>/hero-hanji.png` — first-frame still PNGs (`<noscript>` fallback)
//! - `<out-dir>/beats-meok.png` and `<out-dir>/beats-hanji.png` — 6-beat contact sheets for visual review
//!
//! Uses `docshots`'s isolation (`Named`, `homes`, `fixed_clock`, `none_of_yours`): `HOME`, `XDG_DATA_HOME`,
//! `XDG_CACHE_HOME` and `XDG_STATE_HOME` are inside a fresh `/tmp/tarae-docs/homepage-*` directory, the clock is
//! pinned via `TZ`, and every recorded frame is checked with `none_of_yours` so no host path or username can leak.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use crate::scenes::docshots::{Bg, Named, fixed_clock, homes, none_of_yours, now};
use crate::scenes::{MAIN_RS, MAIN_TESTS, config_toml, demo_crate_with, write};
use crate::shot::{self, Fonts, Rgb, Setup};
use crate::{Result, root};

const COLS: u16 = 120;
const ROWS: u16 = 32;
/// Keyframe interval in milliseconds (~10 s, so scrubbing never replays from `t = 0`).
const KEYFRAME_MS: u64 = 10_000;
/// Sampling bucket width in milliseconds (keeps the latest screen in each 200 ms window).
const FRAME_STEP_MS: u64 = 200;

/// The six beats of "Three Claudes, one repository" (`.sb/home-plan.md` §2).
const BEATS: [&str; 6] = [
    "Three Claudes, each in a workspace of its own.",
    "You see where each one reads and writes.",
    "The moment two would conflict, tarae says so.",
    "Its commands run sandboxed; the rest asks you.",
    "Every workspace's thread woven into one view.",
    "Review it like a pull request — then bring it in.",
];

/// Keys driving the hero scene:
/// - Before `@rec`: opens the `../docs` session (its doc comment on line 6 already a commit, `record_theme`) and
///   triggers its sandbox host-grant card (`Open the sandbox a little?` for `index.crates.io`), opens `../fix` and
///   starts its Claude streaming its edit on line 12 (with `review-at-end` on), docks the session list over the code
///   (`space W`, `C-w K`) in `main`, and sends the prompt in `main`'s chat, which stays docked right.
/// - Beat 6: main's last edit reviewed in the buffer and accepted (`y`, its stamp on the chat's edit row), then
///   docs brought in (`:workspace-bring docs`, its card).
/// - After `@rec`: captures the 6 beats in one continuous take.
const HERO_KEYS: &[&str] = &[
    "@10?fn main()",
    // 1) docs: its committed doc comment on line 6, then ask to run tests in the sandbox -> host grant card
    ":",
    "workspace-open ../docs",
    "\r",
    "@10?docs",
    ":",
    "e src/main.rs",
    "\r",
    "@10?docs › src › main.rs",
    "@10?First strand kept",
    " ",
    "l",
    "@10?Ask about",
    "run the tests",
    "\r",
    "@20?Open the sandbox a little?",
    "\x1b",
    // 2) fix: open src/main.rs, start `home-fix` streaming its edit on line 12 (held until main writes `or_insert`)
    ":",
    "workspace-open ../fix",
    "\r",
    "@10?fix",
    ":",
    "e src/main.rs",
    "\r",
    "@10?fix › src › main.rs",
    " ",
    "l",
    "@10?Ask about",
    "simplify the summary line",
    "\r",
    "@20?knots\", t.k",
    "\x1b",
    // 3) back to main: dock the session list over the code (its rows read across), send the prompt in main's chat
    //    (docked right: the turn is read there, no follow card over the code), and start recording
    ":",
    "workspace-switch 0",
    "\r",
    "@10?tarae-demo › src › main.rs",
    " ",
    "W",
    "@10?sessions",
    "\x17",
    "K",
    "@10?sessions docked top",
    "\x1b",
    " ",
    "l",
    "@10?Ask about",
    "keep the first strand",
    "\r",
    "@10?Switching to",
    "\x1b",
    "\x1b",
    "@rec",
    // Beat 1: Three Claudes, each in a workspace of its own.
    "@beat",
    // Beat 2: You see where each one reads and writes.
    "@20?or_insert",
    "@beat",
    "@hold:2800",
    // Beat 3: The moment two would conflict, tarae says so.
    // (the overlap's toast: its words wrap in the card, so its file and line, which only it says)
    "@20?src/main.rs L10",
    "@beat",
    "@hold:3200",
    // Beat 4: Its commands run sandboxed; the rest asks you.
    "\x11",
    "@10?Open the sandbox a little?",
    "@10!turn ended",
    "@hold:400",
    "@beat",
    "@hold:1800",
    "y",
    "@10!Open the sandbox a little?",
    "@hold:1400",
    // Beat 5: Every workspace's thread woven into one view.
    // (the keys back to the code from wherever the card left them — docs' chat stays open)
    "\x1b",
    "\x1b",
    ":",
    "workspace-switch 0",
    "\r",
    "@10?tarae-demo › src › main.rs",
    ":",
    "weave",
    "\r",
    "@15?conflict 1 of 1",
    "@10!you took over",
    "@hold:400",
    "@beat",
    "@hold:4200",
    // Beat 6: Review it like a pull request — then bring it in.
    ":",
    "e src/main.rs",
    "\r",
    "@10?tarae-demo › src › main.rs",
    ":",
    "review-at-end off",
    "\r",
    "@20?Change 1 of 1",
    "@10!config reloaded",
    "@hold:2200",
    "y",
    "@20?Done —",
    "@hold:800",
    // docs' doc comment, a clean commit: its bring card beside the review's stamp
    "\x1b",
    "\x1b",
    ":",
    "workspace-bring docs",
    "\r",
    "@20?Bring docs into main",
    "@hold:400",
    "@beat",
    "@hold:2600",
];

pub fn run(args: &[String]) -> Result<()> {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.len() > 1 {
        return Err("usage: cargo xtask homepage [<out-dir>]".into());
    }
    let out = args.first().map_or_else(|| root().join("target/homepage"), PathBuf::from);
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let bin = shot::build(&root(), "shot-docs", &["bundled-grammars"])?;
    let me = std::env::current_exe().map_err(|e| format!("this xtask: {e}"))?;
    for bg in [Bg::Dark, Bg::Light] {
        record_theme(bg, &bin, &me, &out)?;
    }
    Ok(())
}

fn record_theme(bg: Bg, bin: &Path, me: &Path, out: &Path) -> Result<()> {
    let theme = bg.theme();
    let dir_name = format!("homepage-{theme}-{}", std::process::id());
    let dir = Named::new(&dir_name)?;
    let real = std::fs::canonicalize(&dir.0).map_err(|e| format!("{}: {e}", dir.0.display()))?;
    let mut env_homes = homes(&real, &|k| std::env::var_os(k))?;
    env_homes.push(("TZ", fixed_clock(now()).into()));
    let home = real.join("home");
    let cwd = home.join("tarae-demo");
    let passing_tests = MAIN_TESTS.replace("assert_eq!(t.length(), 8,", "assert_eq!(t.length(), 7,");
    let main_rs = english(&format!("{MAIN_RS}{passing_tests}"))?;
    let extra = [
        ("src/main.rs", main_rs.clone()),
        (".cargo/config.toml", "[build]\nbuild-dir = \"target/build\"\n".into()),
    ];
    demo_crate_with(&cwd, false, &extra)?;
    for wt in ["../fix", "../docs"] {
        let status = Command::new("git")
            .args(["worktree", "add", "-q", wt])
            .current_dir(&cwd)
            .status()
            .map_err(|e| format!("git worktree add {wt}: {e}"))?;
        if !status.success() {
            return Err(format!("git worktree add {wt} exited with {status}"));
        }
    }
    // (docs' doc comment, committed: the clean session beat 6 brings in)
    let docs = home.join("docs");
    write(&docs.join("src/main.rs"), &main_rs.replace(DOC_LINE, DOCS_DOC_LINE))?;
    commit_in(&docs, "Say which strand is kept")?;
    let state = real.join("state");
    // (the session list docked over the code as short as it goes — its three rows, kept as a size you set is: the
    // docked list's `C-w -` doesn't reach it)
    write(&state.join("tarae/sizes.json"), r#"{"docked":{"sessions":5}}"#)?;
    let config = real.join("xdg");
    let claude_spec = "main=home-main,fix=home-fix,docs=home-docs";
    // (no follow: main's chat stays docked right and tells its turn there — the follow card would float over the
    // code each beat is about)
    let cfg = format!("{}review-at-end = true\nfollow = false\n", config_toml(me, claude_spec));
    write(&config.join("tarae/config.toml"), &cfg)?;
    let setup = Setup {
        bin: bin.to_path_buf(),
        files: vec!["src/main.rs".into()],
        cwd,
        config,
        background: Some(bg.word()),
        env: vec![("ZELLIJ", None)],
    };
    let keys: Vec<String> = HERO_KEYS.iter().map(|k| k.to_string()).collect();
    let (raw_frames, beats) = shot::record_at(&setup, &keys, (COLS, ROWS), &state, &env_homes)?;
    let sampled = sample_frames(&raw_frames, &beats);

    let yours = std::env::var_os("HOME");
    for (_, screen) in &sampled {
        none_of_yours(&screen_cells(screen), yours.as_deref())?;
    }
    for (_, screen) in &beats {
        none_of_yours(&screen_cells(screen), yours.as_deref())?;
    }

    let default_bg = if bg == Bg::Dark { [7, 8, 11] } else { [245, 241, 232] };
    let default_fg = if bg == Bg::Dark { [232, 227, 216] } else { [52, 50, 45] };
    let json_val = encode_recording(&sampled, &beats, default_fg, default_bg);
    let json_text = serde_json::to_string(&json_val).map_err(|e| e.to_string())?;
    let json_path = out.join(format!("hero-{theme}.json"));
    write(&json_path, &json_text)?;
    println!(
        "saved {} ({} frames, {} beats, {:.1} KB)",
        json_path.display(),
        json_val["frames"].as_array().map_or(0, Vec::len),
        beats.len(),
        json_text.len() as f64 / 1024.0
    );

    let fonts = Fonts::load(2)?;
    if let Some((_, first)) = sampled.first() {
        let (w, h, px) = shot::render(first, &fonts, 2);
        let png_path = out.join(format!("hero-{theme}.png"));
        shot::save_png(&png_path, w, h, &px)?;
        println!("saved {} ({w}×{h})", png_path.display());
    }
    let fonts_1x = Fonts::load(1)?;
    save_beats_sheet(&beats, &fonts_1x, &out.join(format!("beats-{theme}.png")))?;
    Ok(())
}

/// `MAIN_RS`'s doc comment of `main`, once in English (`english`), and the one docs' session writes over it.
const DOC_LINE: &str = "/// A skein: one thread for each name.";
const DOCS_DOC_LINE: &str = "/// First strand kept for each name in the skein.";

/// The shared fixture's text in English — the site is English only; docshots' pictures keep `MAIN_RS` as it is.
/// Refused if any Hangul is left (the fixture changed under it).
fn english(text: &str) -> Result<String> {
    let text = text
        .replace("/// 실타래 — 이어진 실 한 가닥.", DOC_LINE)
        .replace("\"타래\"", "\"linen\"")
        .replace("\"실\"", "\"silk\"");
    match text.lines().find(|l| {
        l.chars().any(|c| ('\u{1100}'..='\u{11FF}').contains(&c) || ('\u{AC00}'..='\u{D7A3}').contains(&c))
    }) {
        Some(l) => Err(format!("the homepage's fixture still has Hangul: {l:?}")),
        None => Ok(text),
    }
}

/// Everything in `dir` committed as the demo's author (no hooks, none of your git config).
fn commit_in(dir: &Path, message: &str) -> Result<()> {
    for args in [&["add", "-A"][..], &["commit", "-q", "-m", message]] {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=demo", "-c", "user.email=demo@example.com", "-c", "commit.gpgsign=false"])
            .args(["-c", "core.hooksPath=/dev/null"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdout(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
        if !status.success() {
            return Err(format!("git {} exited with {status} in {}", args.join(" "), dir.display()));
        }
    }
    Ok(())
}

/// Samples raw frames by keeping the first frame (`t = 0`), every beat timestamp, and the latest frame in each
/// `FRAME_STEP_MS` (200 ms) bucket. If multiple raw frames share the same millisecond timestamp, keeps the last one.
fn sample_frames(raw: &[(u64, vt100::Screen)], beats: &[(u64, vt100::Screen)]) -> Vec<(u64, vt100::Screen)> {
    let beat_times: HashSet<u64> = beats.iter().map(|(t, _)| *t).collect();
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

fn screen_cells(screen: &vt100::Screen) -> Vec<Vec<String>> {
    let (rows, cols) = screen.size();
    let text = |r, c| screen.cell(r, c).map(|cell| cell.contents().to_string()).unwrap_or_default();
    (0..rows).map(|r| (0..cols).map(|c| text(r, c)).collect()).collect()
}

/// Saves a 2×3 grid contact sheet of the 6 beats to `out`.
fn save_beats_sheet(beats: &[(u64, vt100::Screen)], fonts: &Fonts, out: &Path) -> Result<()> {
    if beats.is_empty() {
        return Ok(());
    }
    let pictures: Vec<(usize, usize, Vec<u8>)> =
        beats.iter().map(|(_, screen)| shot::render(screen, fonts, 1)).collect();
    let (fw, fh) = (pictures[0].0, pictures[0].1);
    let gap = shot::PAD;
    let cols_grid = 2usize;
    let rows_grid = beats.len().div_ceil(cols_grid);
    let w = cols_grid * fw + (cols_grid - 1) * gap;
    let h = rows_grid * fh + (rows_grid - 1) * gap;
    let mut sheet = vec![0x80u8; w * h * 3];
    for (idx, (pw, ph, px)) in pictures.iter().enumerate() {
        let gx = idx % cols_grid;
        let gy = idx / cols_grid;
        let x0 = gx * (fw + gap);
        let y0 = gy * (fh + gap);
        for y in 0..*ph {
            let row = &px[y * pw * 3..(y + 1) * pw * 3];
            let at = ((y0 + y) * w + x0) * 3;
            sheet[at..at + row.len()].copy_from_slice(row);
        }
    }
    shot::save_png(out, w, h, &sheet)?;
    println!("saved {} ({} beats)", out.display(), beats.len());
    Ok(())
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

/// Encodes `frames` and `beats` into the homepage player's JSON format (`.sb/home-plan.md`):
/// - `palette`: array of `"#rrggbb"` strings (index 0 = default background, index 1 = default foreground)
/// - `styles`: array of `[fgPaletteIdx, bgPaletteIdx, flags]` (`1 = bold`, `2 = italic`, `4 = underline`)
/// - `beats`: `[{"at": ms, "label": "..."}, ...]`
/// - `frames`: keyframes at `t = 0` and every `KEYFRAME_MS` (~10 s), with changed-row `[col, styleIndex, text]`
///   runs between them. Identical non-beat frames are deduplicated so idle moments add zero bytes, while beat
///   timestamps are always preserved so seeking to `beat.at` lands on that exact beat.
fn encode_recording(
    frames: &[(u64, vt100::Screen)],
    beats: &[(u64, vt100::Screen)],
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

    let (rows, cols) = frames.first().map_or((ROWS, COLS), |(_, s)| s.size());

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

    let out_beats: Vec<Value> = beats
        .iter()
        .enumerate()
        .map(|(i, (at, _))| {
            let label = BEATS.get(i).copied().unwrap_or(BEATS[BEATS.len() - 1]);
            json!({"at": at, "label": label})
        })
        .collect();

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

#[cfg(test)]
mod tests {
    use super::*;

    /// The hero's code is English: the shared fixture's Hangul all said otherwise, docs' line there to replace.
    #[test]
    fn the_heros_fixture_is_english() {
        let text = english(&format!("{MAIN_RS}{MAIN_TESTS}")).unwrap();
        assert!(text.contains(DOC_LINE), "{text}");
        assert!(english("let t = \"실타래\";").is_err());
    }

    #[test]
    fn encodes_keyframes_and_changed_row_runs() {
        let mut p1 = vt100::Parser::new(2, 6, 0);
        p1.process(b"\x1b[38;2;244;128;90mhi\x1b[0m all\r\nrow2");
        let s1 = p1.screen().clone();

        let mut p2 = vt100::Parser::new(2, 6, 0);
        p2.process(b"\x1b[38;2;244;128;90mhi\x1b[0m all\r\nrow2!");
        let s2 = p2.screen().clone();

        let frames = vec![(0, s1.clone()), (100, s1.clone()), (200, s2.clone()), (KEYFRAME_MS + 500, s2)];
        let beats = vec![(0, s1)];
        let v = encode_recording(&frames, &beats, [232, 227, 216], [7, 8, 11]);

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
    fn sample_frames_keeps_latest_per_bucket_and_all_beats() {
        let mut p1 = vt100::Parser::new(2, 6, 0);
        p1.process(b"first");
        let s1 = p1.screen().clone();

        let mut p2 = vt100::Parser::new(2, 6, 0);
        p2.process(b"latest");
        let s2 = p2.screen().clone();

        // Burst in bucket 1 (200..399 ms): 210 ms has `s1`, 350 ms has `s2`, plus a beat at 240 ms (`s1`),
        // followed by a hold until 2000 ms (`s2`).
        let raw = vec![
            (0, s1.clone()),
            (210, s1.clone()),
            (240, s1.clone()),
            (350, s2.clone()),
            (2000, s2.clone()),
        ];
        let beats = vec![(0, s1.clone()), (240, s1.clone()), (2000, s2.clone())];
        let sampled = sample_frames(&raw, &beats);
        let times: Vec<u64> = sampled.iter().map(|(t, _)| *t).collect();
        assert_eq!(
            times,
            vec![0, 240, 350, 2000],
            "keeps t=0, beat at 240, latest in bucket 1 at 350 (drops 210), and 2000"
        );
        assert_eq!(sampled[2].1.contents(), "latest", "bucket 1 retains the latest screen before hold");

        let v = encode_recording(&sampled, &beats, [232, 227, 216], [7, 8, 11]);
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
}
