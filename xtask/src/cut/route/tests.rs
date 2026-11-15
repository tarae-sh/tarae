//! The route's tests: made-up lists over a made-up take, each picture's files names only (the list's checks read no
//! picture), the rule drawn on a strip of its own. No picture of the video's is read.

use std::path::Path;

use serde_json::{Value, json};
use tarae_testkit::scratch::Scratch;

use super::super::{Cut, Marks, Program, Result, What};
use super::*;

/// A made-up take's landmarks, two minutes long.
fn marks() -> Marks {
    Marks::new(vec![json!({ "what": "front", "to": "w1", "name": "fix", "ms": 50_000 })], 120_000)
}

/// A list at 1×: a chapter's card at the start, `rest`, the opening.
fn list(rest: &str) -> String {
    format!(
        "default = 1\n\n[[chapter]]\nat = \"start\"\ntitle = \"One\"\n\n{rest}\n[opening]\nname = \"halil\"\n\
         line = \"A task manager.\"\nat = \"end - 4s\"\nfor = \"3s\"\n"
    )
}

fn read(text: &str, dir: &Path) -> (Result<Cut>, String) {
    let file = dir.join("cut.toml").display().to_string();
    (Cut::parse(text, &file, &marks()), file)
}

/// The line (from 1) of `text`'s `nth` line (from 0) that starts with `start`.
fn line_of(text: &str, start: &str, nth: usize) -> usize {
    (text.lines().enumerate())
        .filter(|(_, l)| l.trim_start().starts_with(start))
        .nth(nth)
        .map(|(i, _)| i + 1)
        .unwrap_or_else(|| panic!("no line {nth} starting {start:?} in:\n{text}"))
}

/// A step's manifest: its drawing's `ms`, its strokes (at, ms) and a fade.
fn manifest(ms: u64, strokes: &[(u64, u64)]) -> Value {
    let strokes: Vec<Value> = (strokes.iter())
        .map(|&(at, ms)| json!({ "at": at, "ms": ms, "r": 30, "path": [[200, 200], [1200, 200]] }))
        .collect();
    json!({ "size": [3840, 1860], "ms": ms, "strokes": strokes, "fades": [{ "at": 0, "ms": 300, "rect": [100, 400, 600, 200] }] })
}

/// A step's four files in `dir`, names only: NAME.png and its layers empty, NAME.json `m`.
fn named(dir: &Path, name: &str, m: &Value) {
    for f in [format!("{name}.png"), format!("{name}.ink.png"), format!("{name}.wash.png")] {
        std::fs::write(dir.join(f), b"").unwrap();
    }
    std::fs::write(dir.join(format!("{name}.json")), m.to_string()).unwrap();
}

/// The made-up pictures: `a` (strokes to 0.4 s and 0.1–0.9 s, then `c`, fades alone), `b`, `d` (a stroke to 0.7 s,
/// drawn at 2×), `e` (fades alone).
fn pictures(d: &Path) {
    named(d, "a", &manifest(1_000, &[(0, 400), (100, 800)]));
    named(d, "b", &manifest(1_000, &[(0, 400)]));
    named(d, "c", &manifest(500, &[]));
    named(d, "d", &manifest(1_000, &[(0, 700)]));
    named(d, "e", &manifest(500, &[]));
}

/// Three pictures at 20 s, 40 s and 60 s.
fn three() -> String {
    let at = |when: &str, steps: &str| format!("[[picture]]\nat = \"{when}\"\nsteps = [\n{steps}]\n\n");
    at("20s", "  { file = \"a.png\", for = \"1s\" },\n  { file = \"c.png\", for = \"1s\" },\n")
        + &at("40s", "  { file = \"b.png\", for = \"1s\" },\n")
        + &at("60s", "  { file = \"d.png\", for = \"1s\", speed = 2 },\n")
}

const ROUTE: &str = "[route]\nstations = [\"The room\", \"The tunnel\"]\n\n[[station]]\npicture = 1\n\n[[station]]\npicture = 3\n\n";

/// A station is inked on the frame its picture's last stroke ends — the first whose step clock has reached it, in
/// the picture's last step with strokes, at its speed —, at once; and the rule moves at nothing else: no route on a
/// card, a picture with no station inks none, and between inkings every frame's route is the same. With no
/// `[route]`, no frame has one.
#[test]
fn a_station_is_inked_on_its_pictures_last_stroke_and_the_rule_moves_at_nothing_else() {
    let dir = Scratch::new("xtask-route-inked");
    pictures(dir.path());
    let (c, _) = read(&list(&(three() + ROUTE)), dir.path());
    let c = c.unwrap();
    let p = Program::new(&c);
    assert_eq!(p.inks.len(), 2);
    // (where each is: the board's own clock, frame by frame, through the step with the last stroke)
    let expect = |n: usize, k: usize, end: u64| {
        let part = p.parts.iter().find(|x| x.what == What::Picture(n)).unwrap();
        let first = Program::frame_at(part.at);
        (first..first + 300)
            .map(Program::time_of)
            .find(|&v| p.frame(v).picture.is_some_and(|s| s.k == k && s.t.is_some_and(|t| t >= end)))
            .unwrap()
    };
    // (picture 1: step 1's second stroke ends at 0.9 s — step 2 only fades; picture 3: 0.7 s at 2×)
    let (v1, v3) = (expect(0, 0, 900_000), expect(2, 0, 700_000));
    assert_eq!(p.inks, vec![v1, v3]);
    let frame = Program::time_of(1);
    let shows = |v: u64| p.frame(v).route.unwrap();
    assert_eq!(shows(v1 - frame), Shows { inked: 0 });
    assert_eq!(shows(v1), Shows { inked: 1 });
    // (nothing on the rule moves from there to the next inking: through picture 2, which inks none, and the take)
    let steady = Program::frame_at(v1)..Program::frame_at(v3);
    assert!(steady.map(Program::time_of).all(|v| p.frame(v).route.is_none_or(|r| r == Shows { inked: 1 })));
    assert_eq!((shows(v3 - frame), shows(v3)), (Shows { inked: 1 }, Shows { inked: 2 }));
    // (the sheet takes each inking's frame)
    let marks = super::super::draw::sheet_marks(&p);
    assert!(marks.contains(&(v1, "station 1 inked: “The room”".to_string())), "{marks:?}");
    assert!(marks.contains(&(v3, "station 2 inked: “The tunnel”".to_string())));
    let last = p.parts.iter().find(|x| x.what == What::Last).unwrap();
    assert_eq!(shows(last.at + last.len - 1).inked, 2);
    // (no route on a card: the intro, the chapter's)
    for w in [What::Intro, What::Chapter(0), What::End] {
        let part = p.parts.iter().find(|x| x.what == w).unwrap();
        assert_eq!(p.frame(part.at + 1).route, None, "{w:?}");
    }
    // (with no route: none on any frame, and the parts as they were)
    let (c, _) = read(&list(&three()), dir.path());
    let c = c.unwrap();
    let plain = Program::new(&c);
    assert_eq!(plain.parts, p.parts);
    assert!((0..plain.frames()).step_by(7).all(|k| plain.frame(Program::time_of(k)).route.is_none()));
}

/// `route` and `station` are out of the cut's seed: laying the stations reshuffles no blink.
#[test]
fn the_route_stays_out_of_the_seed() {
    let dir = Scratch::new("xtask-route-seed");
    pictures(dir.path());
    let (with, _) = read(&list(&(three() + ROUTE)), dir.path());
    let (without, _) = read(&list(&three()), dir.path());
    let (with, without) = (with.unwrap(), without.unwrap());
    assert!(with.route.is_some() && without.route.is_none());
    assert_eq!(with.seed, without.seed);
}

const INK: Rgb = [232, 227, 216];
const FAINT: Rgb = [81, 84, 92];
const ACCENT: Rgb = [244, 128, 90];
const GROUND: Rgb = [10, 11, 14];

/// A strip `w` across with a rule `k` thick at row `10 k`, faint from `from` to `to`, and the route on it.
fn strip(k: usize, (from, to): (usize, usize), n: usize, shows: Shows) -> (Vec<u8>, usize) {
    let (w, h, rule) = (400 * k, 20 * k, 10 * k);
    let mut px = GROUND.repeat(w * h);
    for y in rule..rule + k {
        for x in from..to {
            px[(y * w + x) * 3..][..3].copy_from_slice(&FAINT);
        }
    }
    draw(&mut px, (w, rule, k), (from, to), n, shows, (INK, FAINT, ACCENT));
    (px, w)
}

fn at(px: &[u8], w: usize, (x, y): (usize, usize)) -> Rgb {
    px[(y * w + x) * 3..][..3].try_into().unwrap()
}

/// The rule carries the route: a dot of `DOT_PX` (11) a station in the middle of its share, round; the part travelled
/// in ink to the last inked dot, `TRAVELLED_PX` (2) thick from the rule's row down, the rest faint and 1 px; the inked
/// dots in ink, the one being worked on in the accent, the rest faint; at 2160p 1080p's doubled.
#[test]
fn the_rule_carries_the_route_in_ink_faint_and_the_accent() {
    assert_eq!(dots(20, 380, 4, 1), vec![65, 155, 245, 335]);
    assert_eq!(dots(40, 760, 4, 2), vec![130, 310, 490, 670], "2160p's: 1080p's doubled");
    let none = Shows { inked: 0 };
    let (px, w) = strip(1, (20, 380), 4, none);
    assert_eq!((at(&px, w, (65, 10)), at(&px, w, (155, 10))), (ACCENT, FAINT));
    assert_eq!(at(&px, w, (100, 10)), FAINT, "the rule between: as it was");
    // (11 px across and down, round: its corner's pixels left as they were, its edge's soft)
    let col: Vec<Rgb> = (2..19).map(|y| at(&px, w, (65, y))).collect();
    assert_eq!(col, [vec![GROUND; 3], vec![ACCENT; 11], vec![GROUND; 3]].concat());
    assert_eq!(at(&px, w, (60, 5)), GROUND, "its corner");
    let edge = at(&px, w, (61, 6));
    assert!(edge != GROUND && edge != ACCENT, "its edge soft: {edge:?}");
    assert_eq!((at(&px, w, (100, 10)), at(&px, w, (100, 11))), (FAINT, GROUND), "none travelled: 1 px");
    // (two inked: the travelled part 2 px, the rule's row and the one under it)
    let (px, w) = strip(1, (20, 380), 4, Shows { inked: 2 });
    let row = |px: &[u8], x: usize| at(px, w, (x, 10));
    assert_eq!([row(&px, 20), row(&px, 100), row(&px, 149), row(&px, 200)], [INK, INK, INK, FAINT]);
    assert_eq!([row(&px, 65), row(&px, 155), row(&px, 245), row(&px, 335)], [INK, INK, ACCENT, FAINT]);
    let col = |px: &[u8], x: usize| [9, 10, 11, 12].map(|y| at(px, w, (x, y)));
    assert_eq!(col(&px, 100), [GROUND, INK, INK, GROUND], "travelled: 2 px, down from the rule");
    assert_eq!(col(&px, 200), [GROUND, FAINT, GROUND, GROUND], "the rest: 1 px");
    // (at 2160p: 22 px across, the rule's two rows, the travelled part's four)
    let (px, w) = strip(2, (40, 760), 4, none);
    let col: Vec<Rgb> = (9..33).map(|y| at(&px, w, (130, y))).collect();
    assert_eq!((col[0], col[23]), (GROUND, GROUND));
    assert!(col[1..23].iter().all(|&c| c != GROUND) && col[2..22].iter().all(|&c| c == ACCENT), "{col:?}");
    let (px, w) = strip(2, (40, 760), 4, Shows { inked: 2 });
    let col: Vec<Rgb> = (19..26).map(|y| at(&px, w, (200, y))).collect();
    assert_eq!(col, [vec![GROUND], vec![INK; 4], vec![GROUND; 2]].concat(), "travelled at 2160p: 4 px");
}

/// Every refusal of `[route]` and `[[station]]`, each at its line.
#[test]
fn every_refusal_of_a_route_names_its_line() {
    let dir = Scratch::new("xtask-route-refused");
    let d = dir.path();
    pictures(d);
    let with = |extra: &str| {
        three() + "[[picture]]\nat = \"70s\"\nsteps = [\n  { file = \"e.png\", for = \"1s\" },\n]\n\n" + extra
    };
    let route = |head: &str, stations: &[&str]| {
        let mut s = format!("[route]\n{head}\n");
        for st in stations {
            s += &format!("\n[[station]]\n{st}\n");
        }
        s
    };
    let two = "stations = [\"A\", \"B\"]";
    // (a key at the file's top, before any table)
    let top = |key: &str| list(&with("")).replacen("default = 1", &format!("default = 1\n{key}"), 1);
    type Case = (String, (&'static str, usize), &'static str);
    let cases: Vec<Case> = vec![
        (top("route = 3"), ("route =", 0), "route: a table, as [route] with stations = ["),
        (
            with(&route(&format!("{two}\nnames = 1"), &["picture = 1", "picture = 2"])),
            ("names", 0),
            "route: no key names (it takes stations)",
        ),
        (with("[route]\n"), ("[route]", 0), "route: no stations (stations = ["),
        (
            with(&route("stations = []", &[])),
            ("stations", 0),
            "route: stations = [\"The room\", \"The key\", …] — a name a station",
        ),
        (
            with(&route("stations = [\"A\", 2]", &["picture = 1"])),
            ("stations", 0),
            "route: stations = [\"The room\", \"The key\", …] — a name a station",
        ),
        (
            with(&route("stations = \"A\"", &["picture = 1"])),
            ("stations", 0),
            "route: stations = [\"The room\", \"The key\", …] — a name a station",
        ),
        (
            with(&route("stations = [\"A\", \" \"]", &["picture = 1", "picture = 2"])),
            ("stations", 0),
            "route: station 2's name is empty",
        ),
        (
            with(&route(&format!("{two}\nboard = true"), &["picture = 1", "picture = 2"])),
            ("board", 0),
            "route: no key board (it takes stations)",
        ),
        (with("[[station]]\npicture = 1\n"), ("[[station]]", 0), "station: no [route] for it to be on"),
        (top("station = 1"), ("station =", 0), "station: tables, as [[station]]"),
        (
            with(&route(two, &["picture = 1", "picture = 2", "picture = 3"])),
            ("[[station]]", 2),
            "station 3: the route has 2 stations — one [[station]] each",
        ),
        (
            with(&route(two, &["picture = 1"])),
            ("stations", 0),
            "route: 2 stations, 1 [[station]] — one each, in the route's order",
        ),
        (
            with(&route(two, &["picture = 1", "picture = 2\nat = \"30s\""])),
            ("at =", 5),
            "station: no key at (it takes picture)",
        ),
        (with(&route(two, &["picture = 1", ""])), ("[[station]]", 1), "station 2: no picture (picture = N"),
        (
            with(&route(two, &["picture = 1", "picture = 0"])),
            ("picture =", 1),
            "station 2: picture = a whole number from 1",
        ),
        (
            with(&route(two, &["picture = 1", "picture = \"2\""])),
            ("picture =", 1),
            "station 2: picture = a whole number from 1",
        ),
        (
            with(&route(two, &["picture = 1", "picture = 5"])),
            ("picture =", 1),
            "station 2: picture = 5 — the list has 4 [[picture]]",
        ),
        (
            with(&route(two, &["picture = 2", "picture = 2"])),
            ("picture =", 1),
            "station 2: picture 2 inks station 1 (line {first}) already — one station a picture",
        ),
        (
            with(&route(two, &["picture = 3", "picture = 2"])),
            ("picture =", 1),
            "station 2: picture 2 comes before station 1's picture 3 — the stations' pictures in the route's order",
        ),
        (
            with(&route(two, &["picture = 1", "picture = 4"])),
            ("picture =", 1),
            "station 2: picture 4 (line {picture}) has no stroke to ink it by",
        ),
    ];
    for (text, (start, nth), want) in cases {
        let text = if text.contains("[opening]") { text } else { list(&text) };
        let (got, file) = read(&text, d);
        let mut want = want.to_string();
        for (mark, table, nth) in [("{first}", "[[station]]", 0), ("{picture}", "[[picture]]", 3)] {
            if want.contains(mark) {
                want = want.replace(mark, &line_of(&text, table, nth).to_string());
            }
        }
        let want = format!("{file}:{}: {want}", line_of(&text, start, nth));
        let e = got.unwrap_err();
        assert!(e.starts_with(&want), "{want}\n  got: {e}");
    }
    // (written right, it's read; picture 4 inks no station)
    let (got, _) = read(&list(&with(&route(two, &["picture = 1", "picture = 3"]))), d);
    let got = got.unwrap().route.unwrap();
    assert_eq!(got.pictures, vec![0, 2]);
}
