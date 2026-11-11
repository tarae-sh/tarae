//! `cargo xtask cut init TAKE [--out FILE]` — a first edit list for a take, by rule, to be tuned by hand after (the
//! brief's "The first edit list, by rule"):
//!
//! - a card from shown to answered plus `CARD_AFTER` (one never answered: `CARD_OPEN` from shown): 1×;
//! - what was typed, complete before it's sent: from the last key before a return (or `ASK_STANDS` before the return,
//!   where it came later) to `TYPED_AFTER` past the return, 1× — as long as the take had it stand, never longer;
//! - something typed (an ask, a `:` command — keys from the terminal or the key script, a burst while each comes within
//!   `TYPED_GAP` of the last) to `TYPED_AFTER` past its last key: `TYPED_SPEED`×;
//! - a stretch where nothing on the screen changes but a timer or a spinner (`ticking`) for over `STILL_MS`: left out
//!   but for its first `STILL_KEPT`, shown at 1×;
//! - a turn: sped so what's left of it takes at most `TURN_MAX` on screen, never under `TURN_MIN`×;
//! - the rest: `DEFAULT`×, the file's `default`;
//! - a chapter where the take starts and wherever the workspace in front changes, titled with its name (to fill in).
//!   A bring landing has no landmark (tarae says it only in a toast's words), so none is placed for it.
//!
//! Where rules meet, the first in that list wins. Each span's edges are written as the landmark they came from where
//! there is one (`card 2 answered + 1s`), else as the take's clock to the millisecond.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{Cut, Marks, Pace, Seg, clock, factor, ms_of};
use crate::Result;
use crate::record::{Rec, Replay, SCREEN, Take, read_log};

pub(crate) const TYPED_GAP: u64 = 1_500;
pub(crate) const TYPED_AFTER: u64 = 500;
pub(crate) const TYPED_SPEED: f64 = 2.0;
pub(crate) const ASK_STANDS: u64 = 1_500;
pub(crate) const CARD_AFTER: u64 = 1_000;
pub(crate) const CARD_OPEN: u64 = 3_000;
pub(crate) const STILL_MS: u64 = 2_000;
pub(crate) const STILL_KEPT: u64 = 500;
pub(crate) const TURN_MAX: u64 = 20_000;
pub(crate) const TURN_MIN: f64 = 4.0;
pub(crate) const DEFAULT: f64 = 4.0;

const USAGE: &str = "usage: cargo xtask cut init TAKE [--out FILE]";

pub(super) fn run(args: &[String]) -> Result<()> {
    let (dir, out) = match args {
        [d] => (PathBuf::from(d), PathBuf::from(d).join("init.toml")),
        [d, flag, f] if flag == "--out" => (PathBuf::from(d), PathBuf::from(f)),
        _ => return Err(USAGE.into()),
    };
    if out.exists() {
        return Err(format!(
            "{} is there already — name another with --out, or move it away first",
            out.display()
        ));
    }
    let take = Take::read(&dir)?;
    let marks = Marks::read(&dir, take.ms())?;
    let keys = match dir.join("keys.bin").exists() {
        true => read_log(&dir.join("keys.bin"))?,
        false => Vec::new(),
    };
    let facts = Facts::of(&marks, &keys, stills(&take.screen, take.size, take.ms()));
    let (text, said) = written(&facts, &dir);
    // (what it wrote reads back: every edge resolves to the same ms)
    Cut::parse(&text, &out.display().to_string(), &marks)
        .map_err(|e| format!("cut init wrote a list it can't read back — {e}"))?;
    std::fs::write(&out, &text).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("{}\n{said}", out.display());
    Ok(())
}

/// A screen as the still rule reads it: its text with each run of digits one `0`, each spinner's glyph one `*` and each
/// run of blanks one space (a timer that ticks, a column that shifts as it grows, a spinner that turns — all the same
/// screen), and where its cursor is when it's shown.
pub(crate) fn ticking(screen: &vt100::Screen) -> String {
    let spinner = |c: char| matches!(c, '\u{2800}'..='\u{28ff}' | '\u{2581}'..='\u{2588}' | '◐' | '◑' | '◒' | '◓' | '◦' | '•' | '●');
    let mut out = String::new();
    let mut last = ' ';
    for c in screen.contents().chars() {
        let c = match c {
            c if c.is_ascii_digit() => '0',
            c if spinner(c) => '*',
            ' ' | '\t' => ' ',
            c => c,
        };
        if !(c == last && matches!(c, '0' | ' ')) {
            out.push(c);
        }
        last = c;
    }
    // (where a cursor stands that's shown: a hidden one moves with what's written)
    let (r, c) = screen.cursor_position();
    match screen.hide_cursor() {
        true => out,
        false => out + &format!("\n@{r},{c}"),
    }
}

/// The still stretches of a take (from, to ms): where its screens as `ticking` reads them stood unchanged over
/// `STILL_MS`, to the next that changed (or the take's `end`).
pub(crate) fn stills(recs: &[Rec], size: (u16, u16), end: u64) -> Vec<(u64, u64)> {
    let mut r = Replay::new(recs, size);
    let mut since: Option<(u64, String)> = None;
    let mut out = Vec::new();
    while let Some(ms) = r.next() {
        let now = ticking(r.screen());
        if let Some((t0, was)) = &since {
            if *was == now {
                continue;
            }
            if ms - t0 > STILL_MS {
                out.push((*t0, ms));
            }
        }
        since = Some((ms, now));
    }
    if let Some((t0, _)) = since
        && end.saturating_sub(t0) > STILL_MS
    {
        out.push((t0, end));
    }
    out
}

/// The stretches something was typed (from its first key to its last, ms): the terminal's and the key script's keys
/// — not the terminal's answers to tarae's questions nor the mouse's reports —, a burst while each comes within
/// `TYPED_GAP` of the one before.
pub(crate) fn typing(keys: &[Rec]) -> Vec<(u64, u64)> {
    let mut out: Vec<(u64, u64)> = Vec::new();
    for r in keys.iter().filter(|r| is_key(r)) {
        let ms = r.us / 1000;
        match out.last_mut() {
            Some(b) if ms <= b.1 + TYPED_GAP => b.1 = ms,
            _ => out.push((ms, ms)),
        }
    }
    out
}

/// Whether a record of the input log is a key someone pressed: the terminal's or the key script's — not the terminal's
/// answers to tarae's questions (colours, modes, the keyboard protocol), its focus reports nor the mouse's.
pub(crate) fn is_key(r: &Rec) -> bool {
    let b = &r.bytes[..];
    let answer = [&b"\x1b]"[..], b"\x1b[?", b"\x1b[<", b"\x1bP", b"\x1b[M"].iter().any(|p| b.starts_with(p))
        || b == b"\x1b[I"
        || b == b"\x1b[O";
    r.whose != SCREEN && !b.is_empty() && !answer
}

/// Whether a key is a return: `\r`, `\n`, or the keyboard protocol's `ESC [ 13 u` (pressed, not released).
pub(crate) fn is_return(b: &[u8]) -> bool {
    matches!(b, b"\r" | b"\n" | b"\r\n")
        || (b.starts_with(b"\x1b[13") && b.ends_with(b"u") && !b.ends_with(b":3u"))
}

/// The sends of a take (the last key before, the return, ms): each return, and the key before it — what was typed, from
/// when it stood complete.
pub(crate) fn sends(keys: &[Rec]) -> Vec<(u64, u64)> {
    let keys: Vec<&Rec> = keys.iter().filter(|r| is_key(r)).collect();
    (keys.iter().enumerate())
        .filter(|(_, r)| is_return(&r.bytes))
        .map(|(i, r)| {
            let at = r.us / 1000;
            (i.checked_sub(1).map_or(at, |j| keys[j].us / 1000), at)
        })
        .collect()
}

/// What the rules read from a take: its end; its turns, cards and front changes from the landmarks (each edge with the
/// landmark it is, as the file says one); what was typed, each send, and the still stretches.
#[derive(Debug, Default)]
pub(crate) struct Facts {
    pub(crate) end: u64,
    pub(crate) turns: Vec<((u64, String), (u64, String))>,
    pub(crate) cards: Vec<((u64, String), (u64, String))>,
    pub(crate) typed: Vec<(u64, u64)>,
    pub(crate) sent: Vec<(u64, u64)>,
    pub(crate) stills: Vec<(u64, u64)>,
    pub(crate) fronts: Vec<(u64, String, String)>,
}

impl Facts {
    pub(crate) fn of(marks: &Marks, keys: &[Rec], stills: Vec<(u64, u64)>) -> Facts {
        let end = marks.end;
        let turns = (marks.of("turn-started").enumerate())
            .map(|(i, v)| {
                let ended = (marks.of("turn-ended"))
                    .find(|e| e["session"] == v["session"] && ms_of(e) >= ms_of(v))
                    .map_or((end, "end".to_string()), |e| (ms_of(e), format!("turn {} end", i + 1)));
                ((ms_of(v), format!("turn {} start", i + 1)), ended)
            })
            .collect();
        let cards = (marks.of("card-shown").enumerate())
            .map(|(i, v)| {
                let n = i + 1;
                let gone = (marks.of("card-answered")).find(|e| e["id"] == v["id"] && ms_of(e) >= ms_of(v));
                let to = match gone {
                    Some(e) => (ms_of(e) + CARD_AFTER, format!("card {n} answered + {}s", CARD_AFTER / 1000)),
                    None => (ms_of(v) + CARD_OPEN, format!("card {n} shown + {}s", CARD_OPEN / 1000)),
                };
                ((ms_of(v), format!("card {n} shown")), (to.0.min(end), to.1))
            })
            .collect();
        let fronts = (marks.of("front").enumerate())
            .map(|(i, v)| {
                let name = v["name"].as_str().or(v["to"].as_str()).unwrap_or("?").to_string();
                match i {
                    0 => (0, "start".to_string(), name),
                    _ => (ms_of(v), format!("front {}", i + 1), name),
                }
            })
            .collect();
        Facts { end, turns, cards, typed: typing(keys), sent: sends(keys), stills, fronts }
    }
}

/// Which rule a moment of the take falls to, the first that holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Rule {
    Card(usize),
    Sent,
    Typed,
    Kept,
    Still,
    Turn(usize),
    Rest,
}

/// The spans the rules give (end to end over the take, apart at each chapter), each with what made it.
pub(crate) fn rules(f: &Facts) -> Vec<(Seg, String)> {
    let end = f.end;
    let mut edges: Vec<u64> = vec![0, end];
    let mut put = |a: u64, b: u64| edges.extend([a.min(end), b.min(end)]);
    f.typed.iter().for_each(|&(a, b)| put(a, b + TYPED_AFTER));
    let stands = |&(last, ret): &(u64, u64)| (last.max(ret.saturating_sub(ASK_STANDS)), ret + TYPED_AFTER);
    f.sent.iter().map(stands).for_each(|(a, b)| put(a, b));
    f.cards.iter().for_each(|c| put(c.0.0, c.1.0));
    f.turns.iter().for_each(|t| put(t.0.0, t.1.0));
    for &(a, b) in &f.stills {
        put(a, (a + STILL_KEPT).min(b));
        put(a, b);
    }
    let chapters: Vec<u64> = f.fronts.iter().map(|c| c.0).collect();
    edges.extend(&chapters);
    edges.sort_unstable();
    edges.dedup();
    let inside = |a: u64, b: u64, t: u64| a <= t && t < b;
    let rule_of = |t: u64| -> Rule {
        if let Some(i) = f.cards.iter().position(|c| inside(c.0.0, c.1.0, t)) {
            return Rule::Card(i);
        }
        if f.sent.iter().map(stands).any(|(a, b)| inside(a, b, t)) {
            return Rule::Sent;
        }
        if f.typed.iter().any(|&(a, b)| inside(a, b + TYPED_AFTER, t)) {
            return Rule::Typed;
        }
        if f.stills.iter().any(|&(a, b)| inside(a, (a + STILL_KEPT).min(b), t)) {
            return Rule::Kept;
        }
        if f.stills.iter().any(|&(a, b)| inside(a, b, t)) {
            return Rule::Still;
        }
        match f.turns.iter().position(|x| inside(x.0.0, x.1.0, t)) {
            Some(i) => Rule::Turn(i),
            None => Rule::Rest,
        }
    };
    let pieces: Vec<(u64, u64, Rule)> = edges.windows(2).map(|w| (w[0], w[1], rule_of(w[0]))).collect();
    // (each turn's speed: what's left of it in at most `TURN_MAX`, never under `TURN_MIN`×)
    let speed = |i: usize| -> f64 {
        let left: u64 = pieces.iter().filter(|p| p.2 == Rule::Turn(i)).map(|p| p.1 - p.0).sum();
        (left as f64 / TURN_MAX as f64).max(TURN_MIN)
    };
    let pace_of = |r: Rule| match r {
        Rule::Sent | Rule::Card(_) | Rule::Kept => Pace::Speed(1.0),
        Rule::Typed => Pace::Speed(TYPED_SPEED),
        Rule::Still => Pace::Out,
        Rule::Turn(i) => Pace::Speed(speed(i)),
        Rule::Rest => Pace::Speed(DEFAULT),
    };
    let why = |r: Rule| match r {
        Rule::Sent => "the ask complete before its return".to_string(),
        Rule::Typed => "typed".to_string(),
        Rule::Card(i) => format!("card {}", i + 1),
        Rule::Kept => "a still screen's first half second".into(),
        Rule::Still => "a still screen".into(),
        Rule::Turn(i) => format!("turn {}", i + 1),
        Rule::Rest => "the rest".into(),
    };
    let mut out: Vec<(Seg, String)> = Vec::new();
    for &(a, b, r) in &pieces {
        let pace = pace_of(r);
        match out.last_mut() {
            Some((s, w)) if s.pace == pace && !chapters.contains(&a) => {
                s.to = b;
                let w2 = why(r);
                if !w.split(", ").any(|x| x == w2) {
                    *w += &format!(", {w2}");
                }
            }
            _ => out.push((Seg { from: a, to: b, pace, line: None }, why(r))),
        }
    }
    out
}

/// The list's text, its spans those `rules` gives but the default's (the file's `default`), its chapters the fronts; and
/// what the rules gave, said: the take's length, the video's, the spans by kind.
pub(crate) fn written(f: &Facts, dir: &Path) -> (String, String) {
    let spans = rules(f);
    let mut labels: BTreeMap<u64, String> = BTreeMap::new();
    let mut label = |(ms, l): &(u64, String)| {
        labels.entry(*ms).or_insert_with(|| l.clone());
    };
    f.turns.iter().chain(&f.cards).for_each(|(a, b)| {
        label(a);
        label(b);
    });
    for (ms, edge, _) in &f.fronts {
        labels.entry(*ms).or_insert_with(|| edge.clone());
    }
    labels.insert(0, "start".into());
    labels.insert(f.end, "end".into());
    let at = |ms: u64| labels.get(&ms).cloned().unwrap_or_else(|| exact(ms));
    let mut t = format!(
        "# cut init's list for {}, by rule (cargo xtask cut init): tune it by hand, then `cargo xtask cut` it.\n\
         # The rules: a card shown to answered + 1s at 1×; something typed at 2×, then 1× from its last key (at most\n\
         # 1.5s) to half a second past its return; a still screen (but a timer or a spinner) over 2s left out but its\n\
         # first half second; a turn in at most 20s on screen, never under 4×.\n\n\
         title = \"A live take\"\ndefault = {}   # the rest\n",
        dir.display(),
        factor(DEFAULT)
    );
    for (ms, edge, name) in &f.fronts {
        let edge = if *edge == "start" || *ms == 0 { "start".to_string() } else { edge.clone() };
        t += &format!(
            "\n[[chapter]]\nat = \"{edge}\"\ntitle = \"{name}\"   # to fill in: {name} came to the front\n"
        );
    }
    let (mut one, mut fast, mut gone) = ((0, 0u64), (0, 0u64), (0, 0u64));
    let mut video = 0.0;
    for (s, why) in &spans {
        let len = s.to - s.from;
        match s.pace {
            Pace::Out => gone = (gone.0 + 1, gone.1 + len),
            Pace::Speed(k) => {
                video += len as f64 / k;
                match k <= 1.0 + 1e-6 {
                    true => one = (one.0 + 1, one.1 + len),
                    false => fast = (fast.0 + 1, fast.1 + len),
                }
            }
        }
        if s.pace == Pace::Speed(DEFAULT) && why == "the rest" {
            continue;
        }
        let pace = match s.pace {
            Pace::Out => "out = true".to_string(),
            Pace::Speed(k) => format!("speed = {}", round(k)),
        };
        t += &format!("\n[[span]]   # {why}\nfrom = \"{}\"\nto = \"{}\"\n{pace}\n", at(s.from), at(s.to));
    }
    let s = |ms: u64| ms as f64 / 1e3;
    // (a send whose text stood complete under `ASK_STANDS` in the take: shown as it stood, said)
    let short: Vec<String> = (f.sent.iter())
        .filter(|(last, ret)| ret - last < ASK_STANDS)
        .map(|(last, ret)| format!("{} ({:.1} s)", clock(*ret), (ret - last) as f64 / 1e3))
        .collect();
    let short = match short.len() {
        0 => String::new(),
        n => format!(
            "; {n} of {} returns came under {:.1} s after the last key, shown as they stood: {}",
            f.sent.len(),
            ASK_STANDS as f64 / 1e3,
            short.join(", ")
        ),
    };
    let said = format!(
        "the take {} → about {:.1} s of the take part (cards not counted); spans: {} at 1× ({:.1} s), {} sped ({:.1} s), \
         {} left out ({:.1} s); {} chapters{short}",
        clock(f.end),
        video / 1e3,
        one.0,
        s(one.1),
        fast.0,
        s(fast.1),
        gone.0,
        s(gone.1),
        f.fronts.len()
    );
    (t, said)
}

/// A speed as the file writes it: to a hundredth, whole where it is.
fn round(k: f64) -> String {
    let r = (k * 100.0).round() / 100.0;
    if r.fract() == 0.0 { format!("{}", r as u64) } else { format!("{r}") }
}

/// The take's time to the millisecond, as the file's clock: `0:01:05.250`.
fn exact(ms: u64) -> String {
    format!("{}.{:03}", clock(ms), ms % 1000)
}
