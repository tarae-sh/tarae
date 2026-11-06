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

use std::path::{Path, PathBuf};

use crate::recording::{self, screen_cells};
use crate::scenes::docshots::{Bg, Named, fixed_clock, homes, none_of_yours, now};
use crate::scenes::{MAIN_RS, MAIN_TESTS, config_toml, demo_crate_with, demo_git, write};
use crate::shot::{self, Fonts, Setup};
use crate::{Result, root};

const COLS: u16 = 120;
const ROWS: u16 = 32;

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
        let status = demo_git(&cwd)
            .args(["worktree", "add", "-q", wt])
            .stdout(std::process::Stdio::null())
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
    let beat_times: Vec<u64> = beats.iter().map(|(t, _)| *t).collect();
    let sampled = recording::sample(&raw_frames, &beat_times);

    let yours = std::env::var_os("HOME");
    for (_, screen) in &sampled {
        none_of_yours(&screen_cells(screen), yours.as_deref())?;
    }
    for (_, screen) in &beats {
        none_of_yours(&screen_cells(screen), yours.as_deref())?;
    }

    let (default_fg, default_bg) = recording::ground(bg);
    // (each beat's caption, the last one's past the six)
    let labelled: Vec<(u64, &str)> = beats
        .iter()
        .enumerate()
        .map(|(i, (at, _))| (*at, BEATS.get(i).copied().unwrap_or(BEATS[BEATS.len() - 1])))
        .collect();
    let json_val = recording::encode(&sampled, &labelled, default_fg, default_bg);
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
        let status = demo_git(dir)
            .args(args)
            .stdout(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
        if !status.success() {
            return Err(format!("git {} exited with {status} in {}", args.join(" "), dir.display()));
        }
    }
    Ok(())
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
}
