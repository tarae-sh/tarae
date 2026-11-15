//! The route (Z3's E2, `.sb/ztunnel-edit-plan-2.md` §4 and §5; the brief's §2 E2, §7 and §10): the video's stations
//! on the band's rule, inked one by one by the brush that draws each station's picture.
//!
//! - **Where**: on the band's rule, the line between tarae's cells and the band — a dot of `DOT_PX` (1080p's) a
//!   station, each in the middle of its share of the rule; the part travelled (from the rule's start to the last inked
//!   dot) in ink and `TRAVELLED_PX` thick, the rest faint and 1 px as the rule is, the station being worked on (the
//!   first not inked) in the accent. It costs the band no room and touches no cell of tarae's. Names aren't drawn on
//!   the rule: a station's name is its chapter's, in the band only (`card = false`; the user's way (a), 2026-11-16).
//! - **The inking is the brush's**: station k is inked on the frame its picture's last stroke ends (the last step with
//!   strokes, at the first frame whose step clock has reached the stroke's end) — the travelled ink reaches its dot and
//!   the next turns to the accent on that frame; laid at once, as the strokes are (fresh ink drying was tried twice in
//!   Z3 and didn't read at 1×).
//! - **Idle**: nothing on the rule moves but at an inking. With no `[route]` the rule is today's, pixel for pixel.
//! - `route` and `station` are out of the cut's seed: the route reshuffles no blink.

#[cfg(test)]
mod tests;

use super::picture::Picture;
use super::{Entry, Src};
use crate::Result;
use crate::shot::Rgb;

/// A station's dot across, at 1080p (the design session's, judged in the stills: 7 didn't read; the user asked that
/// the progress be seen).
pub(crate) const DOT_PX: usize = 11;
/// The travelled part of the rule, px thick at 1080p, against the rule's 1: from the rule's row down into the band's
/// side, never up into tarae's cells.
pub(crate) const TRAVELLED_PX: usize = 2;

/// The route as the list says: its stations' names, each station's picture (of the list's, from 0), `[route]`'s
/// line, each `[[station]]`'s.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Route {
    pub(crate) stations: Vec<String>,
    pub(crate) pictures: Vec<usize>,
    pub(crate) line: usize,
    pub(crate) lines: Vec<usize>,
}

/// What the rule shows in a frame: how many stations are inked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Shows {
    pub(crate) inked: usize,
}

const STATIONS: &str = "stations = [\"The room\", \"The key\", …]";

/// `[route]` and its `[[station]]`s in `top` (`src` its text by lines), against the list's `pictures` — refused, each
/// with its line: `[route]` not a table, a key it doesn't take, no stations or not a list of names, a name empty; a
/// `[[station]]` with no `[route]`, not a table, a key it doesn't take, no picture or not
/// a whole number from 1, a picture the list hasn't, one another station has, before the station before's, or with no
/// stroke to ink it by; more or fewer `[[station]]`s than the route's stations.
pub(super) fn parse(top: &toml::Table, src: &Src, pictures: &[Picture]) -> Result<Option<Route>> {
    let stations: Vec<Entry> = match top.get("station") {
        None => Vec::new(),
        Some(toml::Value::Array(a)) => (a.iter().enumerate())
            .map(|(k, v)| match v.as_table() {
                Some(t) => Ok(Entry { t, line: src.header("station", Some(k)), src, name: "station" }),
                None => Err(src.at(src.key(0, "station"), "station: tables, as [[station]]")),
            })
            .collect::<Result<_>>()?,
        Some(_) => return Err(src.at(src.key(0, "station"), "station: tables, as [[station]]")),
    };
    let t = match top.get("route") {
        None => {
            return match stations.first() {
                None => Ok(None),
                Some(s) => {
                    Err(src.at(s.line, "station: no [route] for it to be on ([route] with stations = […])"))
                }
            };
        }
        Some(toml::Value::Table(t)) => t,
        Some(_) => {
            return Err(src.at(src.key(0, "route"), format!("route: a table, as [route] with {STATIONS}")));
        }
    };
    let e = Entry { t, line: src.header("route", None), src, name: "route" };
    e.keys(&["stations"])?;
    let names: Vec<String> = match t.get("stations") {
        None => return Err(src.at(e.line, format!("route: no stations ({STATIONS})"))),
        Some(toml::Value::Array(a)) if !a.is_empty() => (a.iter())
            .map(|v| v.as_str().map(str::to_string))
            .collect::<Option<Vec<String>>>()
            .ok_or_else(|| e.err("stations", format!("route: {STATIONS} — a name a station")))?,
        Some(_) => return Err(e.err("stations", format!("route: {STATIONS} — a name a station"))),
    };
    if let Some(k) = names.iter().position(|n| n.trim().is_empty()) {
        return Err(e.err("stations", format!("route: station {}'s name is empty", k + 1)));
    }
    let mut chosen: Vec<usize> = Vec::new();
    for (k, s) in stations.iter().enumerate() {
        let who = format!("station {}", k + 1);
        if k >= names.len() {
            return Err(src.at(
                s.line,
                format!("{who}: the route has {} stations — one [[station]] each, in its order", names.len()),
            ));
        }
        s.keys(&["picture"])?;
        let n = match s.t.get("picture") {
            None => {
                return Err(
                    src.at(s.line, format!("{who}: no picture (picture = N: the list's Nth [[picture]])"))
                );
            }
            Some(toml::Value::Integer(n)) if *n >= 1 => *n as usize,
            Some(_) => {
                return Err(s.err(
                    "picture",
                    format!("{who}: picture = a whole number from 1 (the list's [[picture]]s in order)"),
                ));
            }
        };
        if n > pictures.len() {
            return Err(s.err(
                "picture",
                format!("{who}: picture = {n} — the list has {} [[picture]]", pictures.len()),
            ));
        }
        if let Some(j) = chosen.iter().position(|&c| c == n - 1) {
            return Err(s.err(
                "picture",
                format!(
                    "{who}: picture {n} inks station {} (line {}) already — one station a picture",
                    j + 1,
                    stations[j].line
                ),
            ));
        }
        if let Some(&before) = chosen.last()
            && n - 1 < before
        {
            return Err(s.err(
                "picture",
                format!(
                    "{who}: picture {n} comes before station {k}'s picture {} — the stations' pictures in the route's order",
                    before + 1
                ),
            ));
        }
        if last_stroke(&pictures[n - 1]).is_none() {
            return Err(s.err(
                "picture",
                format!(
                    "{who}: picture {n} (line {}) has no stroke to ink it by — its steps only fade",
                    pictures[n - 1].line
                ),
            ));
        }
        chosen.push(n - 1);
    }
    if stations.len() < names.len() {
        return Err(e.err(
            "stations",
            format!(
                "route: {} stations, {} [[station]] — one each, in the route's order",
                names.len(),
                stations.len()
            ),
        ));
    }
    let lines = stations.iter().map(|s| s.line).collect();
    Ok(Some(Route { stations: names, pictures: chosen, line: e.line, lines }))
}

/// A picture's last stroke: its last step with strokes (which), and where on that step's clock the stroke ends (µs).
pub(crate) fn last_stroke(p: &Picture) -> Option<(usize, u64)> {
    let k = p.steps.iter().rposition(|s| !s.strokes.is_empty())?;
    let end = p.steps[k].strokes.iter().map(|s| s.at + s.ms).max()?;
    Some((k, end * 1000))
}

/// Each station's dot's middle on a rule from `from` to `to` (px at the scale `k`): the middle of its share of the
/// rule, worked out at 1080p's px so 2160p's are 1080p's doubled.
pub(crate) fn dots(from: usize, to: usize, n: usize, k: usize) -> Vec<usize> {
    let (a, len) = (from / k, to.saturating_sub(from) / k);
    (0..n).map(|i| (a + len * (2 * i + 1) / (2 * n)) * k).collect()
}

/// The route laid on a frame `px` (`w` px across) whose rule is the row `rule`, `k` px thick, from `from` to `to`:
/// the travelled part in ink, `TRAVELLED_PX` × `k` thick from the rule's row down, each dot inked, in the accent (the one being worked on) or faint — round, its edge a px
/// soft.
pub(crate) fn draw(
    px: &mut [u8],
    (w, rule, k): (usize, usize, usize),
    (from, to): (usize, usize),
    n: usize,
    shows: Shows,
    (ink, faint, accent): (Rgb, Rgb, Rgb),
) {
    let xs = dots(from, to, n, k);
    if let Some(last) = shows.inked.checked_sub(1) {
        for y in rule..(rule + TRAVELLED_PX * k).min(px.len() / (w * 3)) {
            for x in from..xs[last] {
                px[(y * w + x) * 3..][..3].copy_from_slice(&ink);
            }
        }
    }
    let r = (DOT_PX * k) as f32 / 2.0;
    let cy = rule as f32 + k as f32 / 2.0;
    for (i, &x) in xs.iter().enumerate() {
        let c = match i.cmp(&shows.inked) {
            std::cmp::Ordering::Less => ink,
            std::cmp::Ordering::Equal => accent,
            std::cmp::Ordering::Greater => faint,
        };
        let cx = x as f32 + k as f32 / 2.0;
        let reach = r.ceil() as usize + 1;
        for py in (cy as usize).saturating_sub(reach)..cy as usize + reach {
            for qx in x.saturating_sub(reach)..x + reach {
                let d = ((qx as f32 + 0.5 - cx).powi(2) + (py as f32 + 0.5 - cy).powi(2)).sqrt();
                let a = (r + 0.5 - d).clamp(0.0, 1.0);
                if a <= 0.0 || qx >= w || (py + 1) * w * 3 > px.len() {
                    continue;
                }
                let d = &mut px[(py * w + qx) * 3..][..3];
                for (d, s) in d.iter_mut().zip(c) {
                    *d = (f32::from(*d) * (1.0 - a) + f32::from(s) * a).round() as u8;
                }
            }
        }
    }
}
