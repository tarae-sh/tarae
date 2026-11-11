//! The keys pop up beside the mascot (the brief's §6, the user: "그 게임에서의 데미지 표시처럼 나타났다 사라지는 이펙트"): a pop
//! where a key is heard, from the same list as the sound and the taps (`sound::sounds`' `struck`), so none on a sped span.
//!
//! - **Its words** (`name`): the key by tarae's own name for it, as the hint row writes it (`key.rs`'s `Display`): `j`,
//!   `space`, `esc`, `enter`, `tab`, `backspace`, `C-q`, `A-x`, `←`. A chord is one pop; a sequence its presses.
//! - **Two weights, by where the key went** (`Regions`, the take's `keys` events): a **command** (the code in normal
//!   mode, a card's or the edit review's answer, a list, a panel; `enter`, `esc` and a chord anywhere) in the band's ink,
//!   bold, `COMMAND_PX`; **typed text** (insert mode, the chat's input, a prompt, the terminal's program) the letter
//!   itself, `TYPED_PX`, at birth as strong as the band's clock (`draw.rs`'s quiet ink) — a space there pops nothing.
//!   `keys = "commands"` leaves the typed ones out.
//! - **Never two over each other** (`pops`, checked by `meets`): a command pressed while the last command's pop is up
//!   joins it as a sequence (`space l`, a held key counting `j ×12`), back at full strength; typed letters take free
//!   places left to right across the lane, the oldest in the way ending when there's none.
//! - **How it moves** (`motion`): at full strength on the key's frame, `RISE` px up over `POP_MS`, slowing, fading over
//!   its last `FADE_MS`.
//! - **Where**: the lane `mascot::LANE` wide left of the box, inside the band, from the paws' height: commands
//!   right-aligned to its right edge, letters from its left. Its letters' ink only, over the ground (`ink`).

use std::path::Path;

use super::Keys;
use crate::Result;
use crate::cut::Program;
use crate::cut::sound::Struck;
use crate::shot::Rgb;
use crate::video::FPS;

/// A command's life and its fade at the end of it; a typed letter's, shorter: the cut shows typing at 2×, about 20 keys
/// a second, and the lane holds about nine letters — at 0.4 s about eight are up at once. Each fades over the same
/// share of its life (`FADE_MS` of `POP_MS`).
pub(crate) const POP_MS: u64 = 600;
pub(crate) const FADE_MS: u64 = 350;
pub(crate) const TYPED_MS: u64 = 400;
pub(crate) const RISE: usize = 28;
pub(crate) const COMMAND_PX: f32 = 26.0;
pub(crate) const TYPED_PX: f32 = 20.0;
/// The air between two typed letters up at once, and between a command and a letter (px at 1080p).
const GAP_TYPED: usize = 4;
const CLEAR: usize = 8;

/// A key by tarae's own name for it, from its bytes as the take keeps them; None for a key let go (the keyboard
/// protocol's release). A line the key script types at once is its text.
pub(crate) fn name(b: &[u8]) -> Option<String> {
    if crate::cut::init::is_return(b) {
        return Some("enter".into());
    }
    if let Some(body) = b.strip_prefix(b"\x1b[").and_then(|r| r.strip_suffix(b"u")) {
        if body.ends_with(b":3") {
            return None;
        }
        let text = String::from_utf8_lossy(body);
        let mut parts = text.split(';');
        let code: u32 = parts.next()?.split(':').next()?.parse().ok()?;
        let mods: u32 = parts.next().and_then(|m| m.split(':').next()?.parse().ok()).unwrap_or(1);
        let m = mods.saturating_sub(1);
        let key = match code {
            27 => "esc".into(),
            9 => "tab".into(),
            13 => "enter".into(),
            32 => "space".into(),
            127 | 8 => "backspace".into(),
            c => {
                let c = char::from_u32(c)?;
                char_name(if m & 1 != 0 { c.to_ascii_uppercase() } else { c })
            }
        };
        let mut s = String::new();
        if m & 4 != 0 {
            s.push_str("C-");
        }
        if m & 2 != 0 {
            s.push_str("A-");
        }
        // (shift with a character is its capital, as the take sends it: only a named key says S-)
        if m & 1 != 0 && key.chars().count() > 1 {
            s.push_str("S-");
        }
        s.push_str(&key);
        return Some(s);
    }
    let named = match b {
        [] => return None,
        [0x1b] => "esc",
        [0x09] => "tab",
        [0x7f] | [0x08] => "backspace",
        [b' '] => "space",
        [0x00] => "C-space",
        b"\x1b[A" | b"\x1bOA" => "↑",
        b"\x1b[B" | b"\x1bOB" => "↓",
        b"\x1b[C" | b"\x1bOC" => "→",
        b"\x1b[D" | b"\x1bOD" => "←",
        b"\x1b[H" | b"\x1bOH" | b"\x1b[1~" => "home",
        b"\x1b[F" | b"\x1bOF" | b"\x1b[4~" => "end",
        b"\x1b[3~" => "del",
        b"\x1b[5~" => "pageup",
        b"\x1b[6~" => "pagedown",
        b"\x1b[Z" => "S-tab",
        [0x1c] => "C-\\",
        [0x1d] => "C-]",
        [0x1e] => "C-^",
        [0x1f] => "C-_",
        [c @ 1..=26] => return Some(format!("C-{}", char::from(c + 96))),
        [0x1b, rest @ ..] if !rest.starts_with(b"[") && !rest.starts_with(b"O") => {
            return name(rest).map(|n| format!("A-{n}"));
        }
        _ => {
            let s = String::from_utf8_lossy(b);
            let mut cs = s.chars();
            return match (cs.next(), cs.next()) {
                (Some(c), None) => Some(char_name(c)),
                (Some(_), Some(_)) if !s.chars().any(char::is_control) => Some(s.into_owned()),
                _ => None,
            };
        }
    };
    Some(named.into())
}

/// A character's name: itself, but `space` (and `-` stays `-`: tarae shows it so).
fn char_name(c: char) -> String {
    match c {
        ' ' => "space".into(),
        c => c.to_string(),
    }
}

/// Where the keys went through the take (`events.jsonl`): each `keys` event's place from its ms, and each key tarae said
/// does nothing where it went (`key_unused`'s "`{key}` does nothing in …", ms, the key's name).
#[derive(Debug, Default)]
pub(crate) struct Regions {
    pub(crate) places: Vec<(u64, Place)>,
    pub(crate) refused: Vec<(u64, String)>,
}

/// Where the keys went, as a pop weighs them: a place that takes commands; typed text (`takes` `text` — insert mode,
/// the chat's input, a prompt, a picker — or `program`, the terminal's); a card, whose answer is its `card-answered`
/// landmark's key, any other letter the typing it swallowed (it says its refusals on itself, in its own words); a
/// surface of answers with no such landmark (the edit review), whose key is its own unless tarae said it does nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    Commands,
    Text,
    Card,
    Review,
}

/// A refusal is said this soon after its key (ms).
const REFUSED_MS: u64 = 250;

impl Regions {
    /// The take's in `dir` (none where it has no `events.jsonl`: every key a command).
    pub(crate) fn read(dir: &Path) -> Result<Regions> {
        let p = dir.join("events.jsonl");
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Regions::default()),
            Err(e) => return Err(format!("{}: {e}", p.display())),
        };
        let mut r = Regions::default();
        let wanted = |l: &&str| l.contains("\"event\":\"keys\"") || l.contains(" does nothing in ");
        for (i, l) in text.lines().enumerate().filter(|(_, l)| wanted(l)) {
            let v: serde_json::Value =
                serde_json::from_str(l).map_err(|e| format!("{}:{}: {e}", p.display(), i + 1))?;
            let (Some(ms), e) = (v["ms"].as_u64(), &v["event"]) else { continue };
            match e["event"].as_str() {
                Some("keys") => {
                    let place = match (e["takes"].as_str(), e["region"].as_str()) {
                        (Some("text" | "program"), _) => Place::Text,
                        (Some("answers"), Some("card")) => Place::Card,
                        (Some("answers"), _) => Place::Review,
                        _ => Place::Commands,
                    };
                    r.places.push((ms, place));
                }
                Some("toast") => {
                    let text = e["text"].as_str().unwrap_or("");
                    if let Some((key, _)) = text.split_once(" does nothing in ") {
                        r.refused.push((ms, key.to_string()));
                    }
                }
                _ => {}
            }
        }
        r.places.sort_by_key(|p| p.0);
        r.refused.sort_by_key(|p| p.0);
        Ok(r)
    }

    /// These places (from their ms), nothing refused.
    #[cfg(test)]
    pub(crate) fn of(places: Vec<(u64, Place)>) -> Regions {
        Regions { places, refused: Vec::new() }
    }

    /// Where a key at the take's `t` (ms) went: the last event at or before it says (the key that moved the keys is said
    /// after it, so it's the place's before); commands before the first.
    pub(crate) fn at(&self, t: u64) -> Place {
        let i = self.places.partition_point(|r| r.0 <= t);
        if i == 0 { Place::Commands } else { self.places[i - 1].1 }
    }

    /// Whether tarae said the key `name` at `t` does nothing where it went (within `REFUSED_MS` after it).
    pub(crate) fn refused(&self, t: u64, name: &str) -> bool {
        let i = self.refused.partition_point(|r| r.0 < t);
        self.refused[i..].iter().take_while(|r| r.0 <= t + REFUSED_MS).any(|r| r.1 == name)
    }

    /// Whether a key named `name` at `t` pops as typed text — else a command, a key the surface took as its own: in a
    /// typed place its typing (not `enter`, `esc`, a chord); at a card, a single character that wasn't its `answer`; at
    /// the edit review, a single character it refused.
    pub(crate) fn typed(&self, t: u64, name: &str, answer: bool) -> bool {
        let letter = name.chars().count() == 1;
        match self.at(t) {
            _ if answer => false,
            Place::Text => typing(name),
            Place::Card => letter,
            Place::Review => letter && self.refused(t, name),
            Place::Commands => false,
        }
    }
}

/// A pop: from its frame until the frame it's gone (or restarted, or made room), its whole life (frames: `POP_MS` or
/// `TYPED_MS`), its words, whether a command, and its place in the lane — its ink's left edge from the lane's left and
/// its width (px at 1080p).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pop {
    pub(crate) from: u64,
    pub(crate) until: u64,
    pub(crate) life: u64,
    pub(crate) text: String,
    pub(crate) command: bool,
    pub(crate) x: usize,
    pub(crate) w: usize,
}

/// The pops and their counts: commands (pops begun), joined (a command added to the one up), held (a held key's
/// count), typed, typed letters ended early for room, spaces in typed text, and the names drawn.
#[derive(Debug, Default)]
pub(crate) struct Pops {
    pub(crate) list: Vec<Pop>,
    pub(crate) commands: usize,
    pub(crate) joined: usize,
    pub(crate) held: usize,
    pub(crate) typed: usize,
    pub(crate) made_room: usize,
    pub(crate) spaces: usize,
    pub(crate) names: Vec<String>,
}

/// A command pop's words as the hint row writes a sequence: each key by its name, a held one with its count (`space l`,
/// `j ×12`).
fn written(words: &[(String, u32)]) -> String {
    let one = |(w, n): &(String, u32)| match n {
        1 => w.clone(),
        n => format!("{w} ×{n}"),
    };
    words.iter().map(one).collect::<Vec<_>>().join(" ")
}

/// Whether two stretches of the lane (from, width) come within `gap` of each other.
fn near((a, aw): (usize, usize), (b, bw): (usize, usize), gap: usize) -> bool {
    a < b + bw + gap && b < a + aw + gap
}

/// Whether a key's name in a typed place is the typing (a character, a line, its editing keys) — `enter`, `esc` and a
/// chord stay commands there.
fn typing(name: &str) -> bool {
    let chord = name.len() > 2 && (name.starts_with("C-") || name.starts_with("A-"));
    !chord && !matches!(name, "enter" | "esc")
}

/// The pops of the keys heard (`struck`, in order), each typed or a command by `regions`, as `keys` says, laid in the
/// lane by `width` (a text's ink width at 1080p, px; a command's or a typed letter's) so no two up at once ever share
/// a column:
///
/// - **A command** pressed while the last command's pop is up joins it — the words grow as a sequence (`space l`), the
///   same key again counts (`j ×12`) —, back at full strength on that press; right-aligned to the lane's right edge.
///   Words that would outgrow the lane end that pop on this press and begin a new one. A typed letter in its way ends.
/// - **A typed letter** takes the next free place to the right of the last one (a burst reads left to right; the first
///   of a burst at the lane's left), left of a command up; none free, the oldest letters in the way end for it.
pub(crate) fn pops(
    struck: &[Struck],
    regions: &Regions,
    keys: Keys,
    width: &dyn Fn(&str, bool) -> usize,
) -> Pops {
    let mut p = Pops::default();
    if keys == Keys::Off {
        return p;
    }
    let life = POP_MS * FPS / 1000;
    let lane = super::LANE;
    // (the command pop up and its words; the typed letters' pops; where the next letter goes)
    let mut cmd: Option<(usize, Vec<(String, u32)>)> = None;
    let mut letters: Vec<usize> = Vec::new();
    let mut cursor = 0usize;
    let mut sorted: Vec<&Struck> = struck.iter().collect();
    sorted.sort_by_key(|s| s.v);
    for s in sorted {
        let Some(name) = name(&s.bytes) else { continue };
        let typed = regions.typed(s.t, &name, s.answer);
        if typed && name == "space" {
            p.spaces += 1;
            continue;
        }
        if typed && keys == Keys::Commands {
            continue;
        }
        let k = Program::frame_at(s.v);
        letters.retain(|&i| p.list[i].until > k);
        if !p.names.contains(&name) {
            p.names.push(name.clone());
        }
        if !typed {
            let up = cmd.take().filter(|(i, _)| p.list[*i].until > k);
            let mut words = vec![(name.clone(), 1)];
            if let Some((i, mut was)) = up {
                p.list[i].until = k;
                match was.last_mut() {
                    Some((w, n)) if *w == name => *n += 1,
                    _ => was.push((name.clone(), 1)),
                }
                if width(&written(&was), true) <= lane {
                    match was.last() {
                        Some((_, n)) if *n > 1 => p.held += 1,
                        _ => p.joined += 1,
                    }
                    words = was;
                }
            }
            if words.len() == 1 && words[0].1 == 1 {
                p.commands += 1;
            }
            let text = written(&words);
            let w = width(&text, true).min(lane);
            let x = lane - w;
            for &i in &letters {
                if near((x, w), (p.list[i].x, p.list[i].w), CLEAR) {
                    p.list[i].until = k;
                    p.made_room += 1;
                }
            }
            letters.retain(|&i| p.list[i].until > k);
            p.list.push(Pop { from: k, until: k + life, life, text, command: true, x, w });
            cmd = Some((p.list.len() - 1, words));
            continue;
        }
        // (a typed letter: left of a command up — which ends, if it leaves no room)
        let w = width(&name, false).min(lane);
        let mut limit = match &cmd {
            Some((i, _)) if p.list[*i].until > k => p.list[*i].x.saturating_sub(CLEAR),
            _ => lane,
        };
        if limit < w {
            if let Some((i, _)) = cmd.take() {
                p.list[i].until = k;
            }
            limit = lane;
        }
        if letters.is_empty() {
            cursor = 0;
        }
        let start = if cursor + w <= limit { cursor } else { 0 };
        let free = |x: usize| letters.iter().all(|&i| !near((x, w), (p.list[i].x, p.list[i].w), GAP_TYPED));
        let x = match (start..=limit - w).chain(0..start).find(|&x| free(x)) {
            Some(x) => x,
            None => {
                for &i in &letters {
                    if near((start, w), (p.list[i].x, p.list[i].w), GAP_TYPED) {
                        p.list[i].until = k;
                        p.made_room += 1;
                    }
                }
                start
            }
        };
        let life = TYPED_MS * FPS / 1000;
        p.list.push(Pop { from: k, until: k + life, life, text: name, command: false, x, w });
        letters.push(p.list.len() - 1);
        p.typed += 1;
        cursor = x + w + GAP_TYPED;
    }
    p.list.sort_by_key(|q| q.from);
    p
}

impl Pops {
    /// Each pop's frames up (from, until), in order of their start — what a swing waits for.
    pub(crate) fn busy(&self) -> Vec<(u64, u64)> {
        self.list.iter().map(|q| (q.from, q.until)).collect()
    }

    /// Where two pops up at once would meet, `height` px tall (a cell's, at 1080p): each pair's first frame where their
    /// boxes touch (risen as `motion` says) — none, ever, is the rule.
    pub(crate) fn meets(&self, height: usize) -> Vec<(u64, usize, usize)> {
        let mut out = Vec::new();
        for (i, a) in self.list.iter().enumerate() {
            for (j, b) in self.list.iter().enumerate().skip(i + 1).take_while(|(_, b)| b.from < a.until) {
                if !near((a.x, a.w), (b.x, b.w), 0) {
                    continue;
                }
                let y = |q: &Pop, k: u64| (motion(k - q.from, q.life).0 * RISE as f32).round() as i64;
                let hit = (b.from..a.until.min(b.until)).find(|&k| y(a, k).abs_diff(y(b, k)) < height as u64);
                if let Some(k) = hit {
                    out.push((k, i, j));
                }
            }
        }
        out
    }

    /// The pops up at frame `k` with their age (frames), oldest first.
    pub(crate) fn at(&self, k: u64) -> Vec<(usize, u64)> {
        let life = POP_MS * FPS / 1000;
        let i = self.list.partition_point(|q| q.from <= k);
        let mut v: Vec<(usize, u64)> = (0..i)
            .rev()
            .take_while(|&j| k - self.list[j].from < life)
            .filter(|&j| self.list[j].until > k)
            .map(|j| (j, k - self.list[j].from))
            .collect();
        v.reverse();
        v
    }
}

/// A pop `age` frames into its `life` (frames): how far it has risen (of `RISE`, slowing — ease-out) and how strongly
/// it's drawn (of 255: full, then fading over its last `FADE_MS` of `POP_MS`'s share).
pub(crate) fn motion(age: u64, life: u64) -> (f32, u8) {
    let x = (age as f32 / life.max(1) as f32).clamp(0.0, 1.0);
    let rise = 1.0 - (1.0 - x) * (1.0 - x);
    let fade = FADE_MS as f32 / POP_MS as f32;
    let a = match x < 1.0 - fade {
        true => 1.0,
        false => ((1.0 - x) / fade).clamp(0.0, 1.0),
    };
    (rise, (a * 255.0).round() as u8)
}

/// The letters of `p` (drawn `fg` on `ground`) over the frame `dst` (`dims` px) at (`x0`, `y0`), `a`/255 strong: only
/// their ink, so the pops over each other and the ground stay as they were where a letter isn't.
pub(crate) fn ink(
    dst: &mut [u8],
    (w, h): (usize, usize),
    p: &crate::recording::Picture,
    (x0, y0): (usize, usize),
    (fg, ground): (Rgb, Rgb),
    a: u8,
) {
    let (pw, ph, src) = p;
    let c = (0..3).max_by_key(|&c| fg[c].abs_diff(ground[c])).unwrap_or(0);
    let span = f32::from(fg[c].abs_diff(ground[c]).max(1));
    for y in 0..*ph {
        for x in 0..*pw {
            if y0 + y >= h || x0 + x >= w {
                continue;
            }
            let s = src[(y * pw + x) * 3 + c];
            let cover = (f32::from(s.abs_diff(ground[c])) / span).min(1.0) * f32::from(a) / 255.0;
            if cover <= 0.0 {
                continue;
            }
            let at = ((y0 + y) * w + x0 + x) * 3;
            for k in 0..3 {
                let d = f32::from(dst[at + k]);
                dst[at + k] = (d + (f32::from(fg[k]) - d) * cover).round() as u8;
            }
        }
    }
}
