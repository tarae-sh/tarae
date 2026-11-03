//! `cargo xtask focus [scene …] [--layout L …] [--bin TARAE] [--compare]` — a turn's reading path, measured from its
//! frames (focus D1, `.sb/focus-plan.md` §1): where on screen something new to read appears, in order, and how far the
//! eye goes between those places. It says what was drawn, never what was meant to be.
//!
//! Each scene is one turn of the stand-in claude (`scenes --claude focus-…`: no network, no account; `a1b-two` the
//! second of two, the first before the recording) on a demo crate of its own, at each layout (`LAYOUTS`: 160×45 with the chat open, the same minimized, 80×24 open, 160×45 with a
//! second session). Every frame from the message's `enter` to the end is kept with its number among the frames tarae
//! wrote, and a watcher of tarae's UI state asking `regions` is told where each thing was drawn as that changes
//! (`src/regions.rs`): each frame's cells are read by the regions of the frame that drew them.
//!
//! The rules (`measure`), crude on purpose — the same on any machine, a comparison before/after, never a claim about
//! attention:
//! - **new to read**: a region shows a *word* (a run of ≥ 3 letters) it didn't show the frame before, compared as the
//!   region's word multiset — a timer (`41s`), the working sign, the drying ink and the caret change no word; a word that
//!   moved within its region (a scroll) isn't new. What you typed (the scene's own words) never counts, nor the chat's
//!   input, a prompt or the status line's rest.
//! - **one copy read**: the same new word in two regions in one frame (the follow card mirrors the chat) counts once, in
//!   the region nearest the last one read.
//! - **per turn**, over the regions with new words in order (a frame's several, nearest first; repeats in a row one):
//!   `switches` (moves between them), `travel` (their centres' distances, in cells, a row counted as 2 columns),
//!   `away` (the share of new words outside the code pane follow leads, else the focused one), `must` (the share that
//!   waits for your answer: an ask, the edit review), `typing` (new words while letters become text — insert mode, a
//!   prompt, the chat's input), and `other` (the most cells a frame drew in no region: the check, 0).
//!
//! Kept in `target/focus/history.tsv` (date, commit — `+` with changes —, scene, layout, the numbers); `--compare`
//! says each against the last row of its scene and layout before this run's are added.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::scenes::{config_toml, demo_crate_with, write};
use crate::shot::{self, Setup};
use crate::{Result, root};

const USAGE: &str =
    "usage: cargo xtask focus [scene …] [--layout L …] [--bin TARAE] [--compare] [--words] | focus --list";

const HISTORY_FILE: &str = "target/focus/history.tsv";

/// A word: at least this many letters in a row.
const WORD_MIN: usize = 3;

/// How a scene asks: in the chat, in the chat after a turn of its own (its first question's answer ending in `That is
/// all`: the turn measured is the second), or `space n` at a line.
#[derive(Clone, Copy, Debug)]
enum Ask {
    Chat(&'static str),
    Second(&'static str, &'static str),
    Line(&'static str),
}

/// One turn: the stand-in's script, its question, the keys after its `enter` (waits, answers, your typing) and what
/// it types into the code meanwhile (never counted as read).
struct Scene {
    name: &'static str,
    ask: Ask,
    after: &'static [&'static str],
    typed: &'static [&'static str],
}

const SCENES: &[Scene] = &[
    Scene { name: "focus-explain", ask: Ask::Chat("what does the loop print?"), after: &["@8"], typed: &[] },
    Scene {
        name: "focus-fix",
        ask: Ask::Chat("the length test fails, fix it"),
        after: &["@20?Change 1 of 1", "@1", "y", "@6"],
        typed: &[],
    },
    Scene {
        name: "focus-asks",
        ask: Ask::Chat("run the tests"),
        after: &["@20?Its reason", "@1", "\x11", "@0.5", "y", "@12"],
        typed: &[],
    },
    Scene { name: "focus-line", ask: Ask::Line("why a Vec here?"), after: &["@8"], typed: &[] },
    // (focus A1b step 3: a second answer, the first's placed paragraphs folding as it ends)
    Scene {
        name: "a1b-two",
        ask: Ask::Second("what does the loop print?", "why a hash map?"),
        after: &["@20?A Vec", "@8"],
        typed: &[],
    },
    // (you type in the code while Claude reads: the keys back to the code, a line opened under the loop)
    Scene {
        name: "focus-typing",
        ask: Ask::Chat("what does the loop print?"),
        after: &[
            "@0.6", "\x1b", "1", "1", "G", "o", "@0.3", TYPED[0], "@1.5", TYPED[1], "@1.5", "\x1b", "@6",
        ],
        typed: &TYPED,
    },
];

const TYPED: [&str; 2] = ["// a note of mine while it reads", " and some more of it"];

/// Where a scene runs: its name, the terminal's size, and how the chat stands.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Layout {
    Open160,
    Min160,
    Open80,
    Two160,
    // (the flow plan's baseline widths, the chat open: asked by name, not in the default run)
    Open120,
    Open200,
}

const LAYOUTS: [Layout; 4] = [Layout::Open160, Layout::Min160, Layout::Open80, Layout::Two160];

/// Every layout `--layout` takes by name.
const NAMED: [Layout; 6] =
    [Layout::Open160, Layout::Min160, Layout::Open80, Layout::Two160, Layout::Open120, Layout::Open200];

impl Layout {
    fn name(self) -> &'static str {
        match self {
            Layout::Open160 => "160-open",
            Layout::Min160 => "160-min",
            Layout::Open80 => "80-open",
            Layout::Two160 => "160-two",
            Layout::Open120 => "120-open",
            Layout::Open200 => "200-open",
        }
    }

    fn size(self) -> (u16, u16) {
        match self {
            Layout::Open80 => (80, 24),
            // (160×45's shape)
            Layout::Open120 => (120, 34),
            Layout::Open200 => (200, 56),
            _ => (160, 45),
        }
    }
}

/// `scene`'s keys at `layout`: the layout made, the question, `@rec` before its `enter` (the turn's frames from
/// there), the chat minimized after it where the layout says, then the scene's own.
fn keys(scene: &Scene, layout: Layout) -> Vec<String> {
    let mut k: Vec<&str> = vec!["@1"];
    if layout == Layout::Two160 {
        // (a second session beside main — idle —, then back in main: the chat's top line names both)
        k.extend([
            ":",
            "sh git worktree add -q ../fix",
            "\r",
            "@3",
            ":",
            "workspace-open ../fix",
            "\r",
            "@3",
        ]);
        k.extend([" ", "W", "@1", "1", "@2"]);
    }
    match scene.ask {
        // (`@seen` before the question: the screen as you read it before asking — what the ask line covers was read
        // before it came, so it's nothing new as it goes)
        Ask::Chat(q) => k.extend(["@seen", " ", "l", "@1", q, "@rec", "\r"]),
        // (the ask line goes once its message is sent — the flow plan's piece 2: the answer at its lines, `space l` again for
        // the next question)
        Ask::Second(first, q) => {
            k.extend([" ", "l", "@1", first, "\r", "@20?¶", "@2", "@seen", " ", "l", "@1", q, "@rec", "\r"]);
        }
        // (the chat open beside the code, the keys back in the code — minimized: it opens as the question goes)
        Ask::Line(q) => {
            if layout != Layout::Min160 {
                k.extend([" ", "l", "@1", "\x1b", "@0.5"]);
            }
            k.extend(["9", "G", "x", "@seen", " ", "n", "@1", q, "@rec", "\r"]);
        }
    }
    if layout == Layout::Min160 && matches!(scene.ask, Ask::Chat(_) | Ask::Second(..)) {
        k.extend(["@0.3", "\x1b", "\x1b"]);
    }
    k.extend(scene.after);
    k.into_iter().map(String::from).collect()
}

// ── The measure ─────────────────────────────────────────────────────────────────────────────

/// A region as the state says it (`regions[]`): its kind, a code pane's id, whether follow leads it, whether it waits
/// for your answer, its cells.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Region {
    pub kind: String,
    pub pane: Option<String>,
    pub lead: bool,
    pub must: bool,
    pub rect: (usize, usize, usize, usize),
}

impl Region {
    fn of(v: &Value) -> Option<Region> {
        let r = v["rect"].as_array()?;
        let n = |i: usize| r.get(i).and_then(Value::as_u64).map(|n| n as usize);
        Some(Region {
            kind: v["kind"].as_str()?.to_string(),
            pane: v["pane"].as_str().map(String::from),
            lead: v["lead"].as_bool().unwrap_or(false),
            must: v["must"].as_bool().unwrap_or(false),
            rect: (n(0)?, n(1)?, n(2)?, n(3)?),
        })
    }

    /// Who it is from frame to frame: its kind, a code pane by its id.
    fn id(&self) -> String {
        match &self.pane {
            Some(p) => format!("{}:{p}", self.kind),
            None => self.kind.clone(),
        }
    }

    fn centre(&self) -> (f64, f64) {
        let (x, y, w, h) = self.rect;
        (x as f64 + w as f64 / 2.0, y as f64 + h as f64 / 2.0)
    }

    /// Read at all: your input, a prompt and the status line's rest aren't.
    fn read(&self) -> bool {
        !matches!(self.kind.as_str(), "chat.input" | "prompt" | "status.rest")
    }

    /// In the code pane follow leads (else the focused one).
    fn at_code(&self) -> bool {
        self.kind == "code" && self.lead
    }
}

/// The regions as of frame `written` (`frames_written`), and whether your letters were becoming text then.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Drawn {
    pub written: u64,
    pub typing: bool,
    pub regions: Vec<Region>,
}

impl Drawn {
    /// A watcher's `regions` event.
    fn of(v: &Value) -> Option<Drawn> {
        (v["event"] == "regions").then_some(())?;
        Some(Drawn {
            written: v["written"].as_u64()?,
            typing: v["typing"].as_bool().unwrap_or(false),
            regions: v["regions"].as_array()?.iter().filter_map(Region::of).collect(),
        })
    }
}

/// A turn's numbers.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Numbers {
    pub switches: usize,
    pub travel: f64,
    pub away: f64,
    pub must: f64,
    pub typing: usize,
    pub words: usize,
    pub other: usize,
    /// The regions read, in order (repeats in a row one), each with its new words — and those words, a step's
    /// (`--words`).
    pub path: Vec<(String, usize)>,
    pub said: Vec<Vec<String>>,
}

/// A frame's cells, row by row, each its text (a wide character's second cell empty).
pub(crate) type Grid = Vec<Vec<String>>;

/// The words — runs of `WORD_MIN` or more letters, lowercased — of a row's stretch of one region, `text` its cells: those cut by its ends — touching an end where another
/// region takes over (`cut`: at its start, at its end), a card over the rest of the word — left out.
fn run_words(text: &str, cut: (bool, bool), out: &mut HashMap<String, usize>) {
    let parts: Vec<&str> = text.split(|c: char| !c.is_alphabetic()).collect();
    let last = parts.len() - 1;
    for (i, w) in parts.iter().enumerate() {
        let torn = (i == 0 && cut.0) || (i == last && cut.1);
        if !torn && w.chars().count() >= WORD_MIN {
            *out.entry(w.to_lowercase()).or_default() += 1;
        }
    }
}

/// Each region's words on `grid` (by id), every cell read by the last region holding it — a word ends where its region
/// does, and one cut there isn't read — and the cells drawn in none.
fn read_frame(grid: &Grid, regions: &[Region]) -> (HashMap<String, HashMap<String, usize>>, usize) {
    let mut by: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let mut other = 0;
    let owner = |x: usize, y: usize| {
        regions.iter().rposition(|r| {
            let (rx, ry, w, h) = r.rect;
            (rx..rx + w).contains(&x) && (ry..ry + h).contains(&y)
        })
    };
    for (y, row) in grid.iter().enumerate() {
        // (each stretch: its region, its first cell, its text)
        let mut runs: Vec<(Option<usize>, usize, String)> = Vec::new();
        for (x, cell) in row.iter().enumerate() {
            let o = owner(x, y);
            if o.is_none() && !cell.trim().is_empty() {
                other += 1;
            }
            match runs.last_mut() {
                Some(r) if r.0 == o => r.2.push_str(cell),
                _ => runs.push((o, x, cell.clone())),
            }
        }
        let n = runs.len();
        for (k, (o, from, text)) in runs.iter().enumerate() {
            if let Some(i) = o.filter(|&i| regions[i].read()) {
                run_words(text, (*from > 0, k + 1 < n), by.entry(regions[i].id()).or_default());
            }
        }
    }
    (by, other)
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (2.0 * (a.1 - b.1)).powi(2)).sqrt()
}

/// The turn's numbers from its frames (each its number among tarae's frames and its cells, the first `before` the screen
/// before the turn — as you read it before you asked, then as you sent it), the regions as told (`drawn`, in the order
/// told) and the words you typed.
pub(crate) fn measure(
    frames: &[(u64, Grid)],
    before: usize,
    drawn: &[Drawn],
    typed: &HashSet<String>,
) -> Numbers {
    let mut n = Numbers::default();
    // (what each region has shown so far this turn: each word as many times as it showed it at once — the frames before
    // it, as read)
    let mut seen: Option<HashMap<String, HashMap<String, usize>>> = None;
    let mut before = before.max(1);
    // (the last region read: its id and centre)
    let mut last: Option<(String, (f64, f64))> = None;
    let (mut at_code, mut must) = (0usize, 0usize);
    for (written, grid) in frames {
        let Some(d) = drawn.iter().rev().find(|d| d.written <= *written).or(drawn.first()) else { continue };
        let (now, other) = read_frame(grid, &d.regions);
        n.other = n.other.max(other);
        if before > 0 {
            before -= 1;
            let had = seen.get_or_insert_with(HashMap::new);
            for (id, ws) in now {
                let had = had.entry(id).or_default();
                for (w, c) in ws {
                    let at = had.entry(w).or_default();
                    *at = (*at).max(c);
                }
            }
            continue;
        }
        let Some(before) = seen.as_mut() else { continue };
        // What each region shows that it hadn't (a word's count past the most it showed), your own words left out —
        // the code a card uncovers again is what was read before it came
        let mut new: BTreeMap<String, HashMap<String, usize>> = BTreeMap::new();
        for (id, ws) in &now {
            let had = before.entry(id.clone()).or_default();
            for (w, &c) in ws {
                let was = had.get(w).copied().unwrap_or(0);
                if c > was {
                    had.insert(w.clone(), c);
                    if !typed.contains(w) {
                        new.entry(id.clone()).or_default().insert(w.clone(), c - was);
                    }
                }
            }
        }
        let region = |id: &str| d.regions.iter().find(|r| r.id() == id);
        let centre = |id: &str| region(id).map_or((0.0, 0.0), Region::centre);
        // One copy read: a word new in several regions stays in the one nearest the last read
        let mut seen: HashMap<String, Vec<String>> = HashMap::new();
        for (id, ws) in &new {
            for w in ws.keys() {
                seen.entry(w.clone()).or_default().push(id.clone());
            }
        }
        for (w, ids) in seen.into_iter().filter(|(_, ids)| ids.len() > 1) {
            let near = |id: &String| last.as_ref().map_or(0.0, |(_, c)| distance(*c, centre(id)));
            let keep = ids.iter().min_by(|a, b| near(a).total_cmp(&near(b))).cloned();
            for id in ids.iter().filter(|id| Some(*id) != keep.as_ref()) {
                if let Some(ws) = new.get_mut(id) {
                    ws.remove(&w);
                }
            }
        }
        new.retain(|_, ws| !ws.is_empty());
        // This frame's regions read nearest first, from the last
        let mut left: Vec<(String, usize)> =
            new.iter().map(|(id, ws)| (id.clone(), ws.values().sum::<usize>())).collect();
        while !left.is_empty() {
            let near = |id: &String| last.as_ref().map_or(0.0, |(_, c)| distance(*c, centre(id)));
            let i =
                (0..left.len()).min_by(|&a, &b| near(&left[a].0).total_cmp(&near(&left[b].0))).unwrap_or(0);
            let (id, count) = left.remove(i);
            let mut ws: Vec<String> = new[&id].keys().cloned().collect();
            ws.sort();
            let c = centre(&id);
            n.words += count;
            if d.typing {
                n.typing += count;
            }
            if region(&id).is_some_and(Region::at_code) {
                at_code += count;
            }
            if region(&id).is_some_and(|r| r.must) {
                must += count;
            }
            match (&last, n.path.last_mut(), n.said.last_mut()) {
                (Some((was, _)), Some(p), Some(s)) if *was == id => {
                    p.1 += count;
                    s.extend(ws);
                }
                _ => {
                    if let Some((_, from)) = &last {
                        n.switches += 1;
                        n.travel += distance(*from, c);
                    }
                    n.path.push((id.clone(), count));
                    n.said.push(ws);
                }
            }
            last = Some((id, c));
        }
    }
    if n.words > 0 {
        n.away = (n.words - at_code) as f64 / n.words as f64;
        n.must = must as f64 / n.words as f64;
    }
    n
}

// ── Running it ─────────────────────────────────────────────────────────────────────────────

/// A folder of one run's own under `target/f/` (0700, the next number when taken; removed on drop) — inside the
/// project, short: the scene's tarae binds its UI socket in its state there, and a run's sandbox reaches a unix socket
/// only in the project, within `sun_path`'s 104 bytes.
struct Own(PathBuf);

impl Own {
    fn new() -> Result<Own> {
        let base = root().join("target/f");
        std::fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
        for n in 0..1000 {
            let p = base.join(n.to_string());
            match std::fs::DirBuilder::new().mode(0o700).create(&p) {
                Ok(()) => return Ok(Own(p)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(format!("{}: {e}", p.display())),
            }
        }
        Err(format!("no free folder in {}", base.display()))
    }
}

impl Drop for Own {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One scene at one layout: its frames and regions recorded, measured. Its demo crate, config and homes in a temp
/// folder of their own (never under this repository: its `.tarae.toml` would ask for trust), its state — the UI
/// socket — in the project (`Own`).
fn one(scene: &Scene, layout: Layout, bin: &Path, me: &Path) -> Result<Numbers> {
    let (own, dir) = (Own::new()?, crate::scenes::Fresh::new("tarae-focus")?);
    let demo = dir.0.join("demo");
    demo_crate_with(&demo, false, &[])?;
    let config = dir.0.join("xdg");
    write(&config.join("tarae/config.toml"), &config_toml(me, scene.name))?;
    let setup = Setup {
        bin: bin.to_path_buf(),
        files: vec!["src/main.rs".into()],
        cwd: demo,
        config,
        background: Some("dark"),
        env: vec![("ZELLIJ", None)],
    };
    let state = own.0.join("state");
    // (a home of the run's own, as the docs' shots: none of your settings, Claude's or tarae's, in the scene)
    let homes = crate::scenes::docshots::homes(&dir.0, &|k| std::env::var_os(k))?;
    let (frames, told) =
        shot::record_watched(&setup, &keys(scene, layout), layout.size(), &state, &homes, &["regions"])?;
    let drawn: Vec<Drawn> = told.iter().filter_map(Drawn::of).collect();
    if drawn.is_empty() {
        return Err(format!("{} at {}: no regions told (an older tarae?)", scene.name, layout.name()));
    }
    let frames: Vec<(u64, Grid)> =
        frames.iter().map(|(n, s)| (*n, crate::homepage::screen_cells(s))).collect();
    let asked = match scene.ask {
        Ask::Chat(q) | Ask::Line(q) => vec![q],
        Ask::Second(first, q) => vec![first, q],
    };
    let mut typed = HashMap::new();
    for t in scene.typed.iter().chain(&asked) {
        run_words(t, (false, false), &mut typed);
    }
    let typed: HashSet<String> = typed.into_keys().collect();
    // (the screen before the turn: `@seen`'s too, where the keys kept one — `shot::drive_with`)
    let before = 1 + usize::from(keys(scene, layout).iter().any(|k| k == "@seen"));
    Ok(measure(&frames, before, &drawn, &typed))
}

/// A kept row: (scene, layout) → its numbers as written.
fn read_history(tsv: &str) -> BTreeMap<(String, String), Vec<String>> {
    let mut last = BTreeMap::new();
    for line in tsv.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() >= 10 {
            last.insert(
                (f[2].to_string(), f[3].to_string()),
                f[4..10].iter().map(|s| s.to_string()).collect(),
            );
        }
    }
    last
}

const HEADER: &str = "date\tcommit\tscene\tlayout\tswitches\ttravel\taway\tmust\ttyping\tother\twords\tpath";

fn row(n: &Numbers) -> [String; 6] {
    [
        n.switches.to_string(),
        format!("{:.0}", n.travel),
        format!("{:.2}", n.away),
        format!("{:.2}", n.must),
        n.typing.to_string(),
        n.other.to_string(),
    ]
}

pub fn run(args: &[String]) -> Result<()> {
    let (mut names, mut layouts, mut bin, mut compare, mut said) =
        (Vec::new(), Vec::new(), None, false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--list" => {
                SCENES.iter().for_each(|s| println!("{}", s.name));
                return Ok(());
            }
            "--layout" => {
                let l = it.next().ok_or(USAGE)?;
                let found = NAMED
                    .iter()
                    .find(|x| x.name() == l)
                    .ok_or_else(|| format!("no layout `{l}` — {}", NAMED.map(Layout::name).join(" ")))?;
                layouts.push(*found);
            }
            "--bin" => bin = Some(PathBuf::from(it.next().ok_or(USAGE)?)),
            "--compare" => compare = true,
            // (each step's new words under the row: what the path read)
            "--words" => said = true,
            n if !n.starts_with('-') => names.push(n.to_string()),
            _ => return Err(USAGE.into()),
        }
    }
    let scenes: Vec<&Scene> = match names.is_empty() {
        true => SCENES.iter().collect(),
        false => names
            .iter()
            .map(|n| {
                SCENES.iter().find(|s| s.name == n).ok_or_else(|| format!("no scene `{n}` — focus --list"))
            })
            .collect::<Result<_>>()?,
    };
    if layouts.is_empty() {
        layouts = LAYOUTS.to_vec();
    }
    let bin = match bin {
        Some(b) if b.is_absolute() && b.is_file() => b,
        Some(b) => return Err(format!("--bin {}: an absolute path to a tarae", b.display())),
        // (the grammars bundled, as the homepage's: a scene with no Rust grammar asks to download it — a card no one
        // with it sees)
        None => shot::build(&root(), "shot-docs", &["bundled-grammars"])?,
    };
    let me = std::env::current_exe().map_err(|e| format!("this xtask: {e}"))?;
    let history = root().join(HISTORY_FILE);
    let kept = std::fs::read_to_string(&history).unwrap_or_default();
    let before = read_history(&kept);
    let commit = crate::perf::commit(&root());
    let date = std::process::Command::new("date")
        .arg("+%Y-%m-%dT%H:%M")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let mut rows = Vec::new();
    let mut failed = Vec::new();
    println!(
        "{:<14} {:<9} {:>8} {:>6} {:>5} {:>5} {:>6} {:>5} {:>5}  path",
        "scene", "layout", "switches", "travel", "away", "must", "typing", "other", "words"
    );
    for scene in &scenes {
        for &layout in &layouts {
            let n = match one(scene, layout, &bin, &me) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("{} at {}: {e}", scene.name, layout.name());
                    failed.push(format!("{}@{}", scene.name, layout.name()));
                    continue;
                }
            };
            let r = row(&n);
            let path: Vec<String> = n.path.iter().map(|(id, c)| format!("{id}({c})")).collect();
            println!(
                "{:<14} {:<9} {:>8} {:>6} {:>5} {:>5} {:>6} {:>5} {:>5}  {}",
                scene.name,
                layout.name(),
                r[0],
                r[1],
                r[2],
                r[3],
                r[4],
                r[5],
                n.words,
                path.join(" → ")
            );
            if said {
                for ((id, c), ws) in n.path.iter().zip(&n.said) {
                    let shown: Vec<&str> = ws.iter().take(12).map(String::as_str).collect();
                    println!("{:>18} {id}({c}): {}", "", shown.join(" "));
                }
            }
            if compare && let Some(was) = before.get(&(scene.name.to_string(), layout.name().to_string())) {
                let names = ["switches", "travel", "away", "must", "typing", "other"];
                let moved: Vec<String> = (names.iter().zip(was.iter().zip(&r)))
                    .filter(|(_, (a, b))| a != b)
                    .map(|(name, (a, b))| format!("{name} {a} → {b}"))
                    .collect();
                match moved.is_empty() {
                    true => println!("{:>24} as before", ""),
                    false => println!("{:>24} {}", "", moved.join(" · ")),
                }
            }
            let mut fields = vec![date.clone(), commit.clone(), scene.name.into(), layout.name().into()];
            fields.extend(r);
            fields.push(n.words.to_string());
            fields.push(path.join(" "));
            rows.push(fields.join("\t"));
        }
    }
    if !rows.is_empty() {
        let mut text = if kept.is_empty() { format!("{HEADER}\n") } else { kept };
        for r in &rows {
            text += &format!("{r}\n");
        }
        write(&history, &text)?;
        println!("kept in {HISTORY_FILE}");
    }
    match failed.len() {
        0 => Ok(()),
        n => Err(format!("{n} failed: {}", failed.join(", "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A screen of `w`×`h` blanks with `text` written at each (x, y).
    fn grid(w: usize, h: usize, texts: &[(usize, usize, &str)]) -> Grid {
        let mut g = vec![vec![" ".to_string(); w]; h];
        for &(x, y, t) in texts {
            for (i, c) in t.chars().enumerate() {
                g[y][x + i] = c.to_string();
            }
        }
        g
    }

    fn region(kind: &str, rect: (usize, usize, usize, usize), lead: bool) -> Region {
        let pane = (kind == "code").then(|| "pane#1".to_string());
        Region { kind: kind.into(), pane, lead, must: kind == "ask", rect }
    }

    /// The code at the left (0–39), the chat's transcript at the right (40–79), the follow card over the code's foot,
    /// the status line's now under them all.
    fn layout() -> Vec<Drawn> {
        let regions = vec![
            region("code", (0, 0, 40, 10), true),
            region("chat.transcript", (40, 0, 40, 10), false),
            region("follow", (10, 6, 30, 4), false),
            region("status.now", (0, 10, 40, 1), false),
        ];
        vec![Drawn { written: 0, typing: false, regions }]
    }

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    /// A scripted frame pair: the transcript gains a sentence — its words, one place, the eyes away from the code.
    #[test]
    fn a_sentence_in_the_chat_is_its_words_away_from_the_code() {
        let a = grid(80, 11, &[(0, 0, "fn main() {")]);
        let b = grid(80, 11, &[(0, 0, "fn main() {"), (42, 2, "The loop prints names.")]);
        let n = measure(&[(1, a), (2, b)], 1, &layout(), &none());
        // ("The", "loop", "prints", "names")
        assert_eq!((n.words, n.switches, n.away), (4, 0, 1.0), "{n:?}");
        assert_eq!(n.path, [("chat.transcript".to_string(), 4)]);
        // Then the code: one switch, its distance the centres' (40 across, none down)
        let c =
            grid(80, 11, &[(0, 0, "fn main() {"), (42, 2, "The loop prints names."), (0, 3, "let first;")]);
        let n = measure(&[(1, grid(80, 11, &[])), (2, c.clone()), (3, c)], 1, &layout(), &none());
        // ("main", "let", "first" in the code)
        assert_eq!(n.words, 4 + 3, "{n:?}");
        assert!(n.switches == 1 && (n.travel - 40.0).abs() < 1e-9, "{n:?}");
    }

    /// A timer and the working sign change no word: a step's own time counting on is nothing new to read. (Counted as
    /// words — digits with the letters — it would be every frame's.)
    #[test]
    fn a_timer_is_no_word() {
        let at = |t: &str| grid(80, 11, &[(0, 10, t)]);
        let frames = [
            (1, at("▃ running cargo test 41s")),
            (2, at("▅ running cargo test 42s")),
            (3, at("▆ running cargo test 1m 2s")),
        ];
        let n = measure(&frames, 1, &layout(), &none());
        assert_eq!((n.words, n.switches), (0, 0), "{n:?}");
    }

    /// A scroll moves words within their region: none of them new. (Compared by place, every word would be.)
    #[test]
    fn a_scroll_is_nothing_new() {
        let a = grid(80, 11, &[(0, 1, "let first = Thread"), (0, 2, "threads insert first")]);
        let b = grid(80, 11, &[(0, 0, "let first = Thread"), (0, 1, "threads insert first")]);
        let n = measure(&[(1, a), (2, b)], 1, &layout(), &none());
        assert_eq!(n.words, 0, "{n:?}");
    }

    /// The same words in the follow card and the transcript in one frame are read once, where the eyes were: the
    /// follow card (over the code) after a word in the code. (Counted twice, 6 words and a switch across.)
    #[test]
    fn two_copies_are_read_once_nearest_the_last() {
        let a = grid(80, 11, &[(0, 0, "fn main")]);
        let b = grid(80, 11, &[(0, 0, "fn main"), (0, 1, "let")]);
        let c = grid(
            80,
            11,
            &[(0, 0, "fn main"), (0, 1, "let"), (12, 7, "walks blocks first"), (42, 4, "walks blocks first")],
        );
        let n = measure(&[(1, a), (2, b), (3, c)], 1, &layout(), &none());
        assert_eq!(n.words, 1 + 3, "{n:?}");
        assert_eq!(n.path, [("code:pane#1".to_string(), 1), ("follow".to_string(), 3)], "{n:?}");
        assert_eq!(n.switches, 1);
    }

    /// A card over the code cuts a word at its edge: the piece left in sight (`leng`) isn't read; the code it uncovers
    /// as it goes was read before it came — nothing new. (Read, the piece is a word; compared with the frame before
    /// only, `length` comes back new.)
    #[test]
    fn a_card_over_the_code_cuts_no_word_and_uncovers_nothing_new() {
        let code = || region("code", (0, 0, 40, 10), true);
        let bare = Drawn { written: 1, typing: false, regions: vec![code()] };
        let carded = Drawn {
            written: 2,
            typing: false,
            regions: vec![code(), region("follow", (10, 6, 30, 4), false)],
        };
        let back = Drawn { written: 3, ..bare.clone() };
        let line = "      length = knots";
        let a = grid(80, 11, &[(0, 7, line)]);
        let b = grid(80, 11, &[(0, 7, "      leng"), (10, 7, "│ reading")]);
        let n = measure(&[(1, a.clone()), (2, b), (3, a)], 1, &[bare, carded, back], &none());
        assert_eq!(n.path, [("follow".to_string(), 1)], "{n:?}");
    }

    /// The screen before the turn is every frame before it: the code as you read it before asking (`@seen`), then the ask
    /// line over it as you sent — the code it uncovers as it goes was read, its own words too. (From the second alone,
    /// `length` comes back new.)
    #[test]
    fn what_an_ask_covered_was_read_before_it() {
        let code = || region("code", (0, 0, 40, 10), true);
        let bare = Drawn { written: 1, typing: false, regions: vec![code()] };
        let asking = Drawn {
            written: 2,
            typing: false,
            regions: vec![code(), region("chat.frame", (4, 6, 30, 3), false)],
        };
        let gone = Drawn { written: 3, ..bare.clone() };
        let a = grid(80, 11, &[(0, 7, "      length = knots")]);
        let b = grid(80, 11, &[(4, 6, "╭─ ask claude ─╮"), (4, 8, "╰─ enter send ─╯")]);
        let frames = [(1, a.clone()), (2, b), (3, a)];
        let drawn = [bare, asking, gone];
        assert_eq!(measure(&frames, 2, &drawn, &none()).path, [], "nothing new");
        let n = measure(&frames[1..], 1, &drawn, &none());
        assert_eq!(n.path, [("code:pane#1".to_string(), 2)], "{n:?}");
    }

    /// What you typed is never read; what arrives while you type is counted apart; a cell drawn in no region is `other`.
    #[test]
    fn your_words_are_not_read_and_arrivals_while_typing_are_counted() {
        let mut drawn = layout();
        drawn.push(Drawn { written: 2, typing: true, regions: drawn[0].regions.clone() });
        let typed: HashSet<String> = ["note".to_string(), "mine".into()].into();
        let a = grid(80, 11, &[]);
        let b = grid(80, 11, &[(0, 4, "// a note of mine"), (42, 2, "Reading the loop.")]);
        let n = measure(&[(1, a), (2, b)], 1, &drawn, &typed);
        assert_eq!((n.words, n.typing), (3, 3), "{n:?}");
        let stray = grid(80, 12, &[(0, 11, "x")]);
        assert_eq!(measure(&[(1, stray)], 1, &layout(), &none()).other, 1);
    }

    #[test]
    fn the_history_gives_each_scenes_last_row() {
        let tsv = format!(
            "{HEADER}\nd\tc1\tfocus-explain\t160-open\t7\t300\t0.50\t0.00\t0\t0\t40\tp\nd\tc2\tfocus-explain\t160-open\t5\t200\t0.40\t0.00\t0\t0\t40\tp\n"
        );
        let h = read_history(&tsv);
        assert_eq!(h[&("focus-explain".to_string(), "160-open".to_string())][0], "5");
    }

    /// Every scene's keys record from its question's `enter`, at every layout — the screen before it kept (`@seen`).
    #[test]
    fn every_scene_records_from_its_enter() {
        for s in SCENES {
            for l in LAYOUTS {
                let k = keys(s, l);
                let rec = k.iter().position(|k| k == "@rec").expect("@rec");
                assert_eq!(k[rec + 1], "\r", "{} at {}", s.name, l.name());
                assert!(k[..rec].contains(&"@seen".to_string()), "{} at {}: @seen", s.name, l.name());
            }
        }
    }
}
