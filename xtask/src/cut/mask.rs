//! The cut's masks (`[[mask]]`; the full video's take 3, where Claude's `ls -la` shows the login as the `.` line's owner
//! in a stretch that holds chapter 1's heart): cells drawn blank, in the screen's own ground, in every frame whose take
//! time the mask's span holds. The renderer leaves them blank (`shot::render_masked`), so the camera's crops and zooms,
//! the crossings and the sheet show them blank too. A mask names a span (take seconds, or a time as a span's), a row
//! and its cells; or a place `record check` found (`check = N`, the take's `check.txt` line): its span, `CHECK_MARGIN_MS`
//! wider each side (the file's precision), its row, and wherever on that row what it found stands, the cells the check
//! reads as it. `cut.txt` says each with the frames it covered; one that covers no frame, or only cells with nothing in
//! them, is said loudly. The cut's own check (`check::said`) leaves out a place a mask's row and span hold whole and
//! that no frame showed unmasked, judged on the masked row by the check's own rule; never one it only might hide.

use super::clock;
use crate::Result;
use crate::record::check::{self, Hit, Yours};
use crate::shot::Blank;

/// A mask as the list says it, with its line.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    pub(crate) what: Spec,
    pub(crate) line: usize,
}

/// What a mask names: a take span (ms), a row and its first and last cells (from 0); or `check.txt`'s line N.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Spec {
    Cells { from: u64, to: u64, row: u16, cells: (u16, u16) },
    Check(usize),
}

/// `check.txt` writes its times to 10 ms: a mask made from one of its places is this much wider each side.
pub(crate) const CHECK_MARGIN_MS: u64 = 10;

/// Where on its row a mask blanks: these cells (the first and the last), or wherever what the check found stands.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Place {
    Cells(u16, u16),
    Found { kind: &'static str, found: String },
}

/// A mask resolved: its take span (ms, `to` not held), its row, its place, its line in the list and its name in
/// `cut.txt`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Mask {
    pub(crate) from: u64,
    pub(crate) to: u64,
    pub(crate) row: u16,
    pub(crate) place: Place,
    pub(crate) line: usize,
    pub(crate) named: String,
}

/// The list's masks (`file`'s) against the take's `check.txt` text, if it has one: a `check = N` that isn't a place's
/// line, or with no `check.txt`, refused with its line.
pub(crate) fn resolve(entries: &[Entry], check: Option<&str>, file: &str) -> Result<Vec<Mask>> {
    let hits = match check {
        Some(t) => Some(check::parse_lines(t).map_err(|e| format!("the take's {e}"))?),
        None => None,
    };
    let mut out = Vec::new();
    for e in entries {
        out.push(match e.what {
            Spec::Cells { from, to, row, cells: (a, b) } => Mask {
                from,
                to,
                row,
                place: Place::Cells(a, b),
                line: e.line,
                named: format!("row {row}, cells {a}–{b}"),
            },
            Spec::Check(n) => {
                let at = |said: String| format!("{file}:{}: mask: check = {n}: {said}", e.line);
                let hits = (hits.as_ref()).ok_or_else(|| {
                    at("the take has no check.txt (`cargo xtask record check` writes it)".into())
                })?;
                let Some((_, h)) = hits.iter().find(|(l, _)| *l == n) else {
                    let lines: Vec<String> = hits.iter().map(|(l, _)| l.to_string()).collect();
                    let on = match lines.is_empty() {
                        true => "it has none".to_string(),
                        false => format!("its places are on lines {}", lines.join(", ")),
                    };
                    return Err(at(format!("check.txt's line {n} isn't a place ({on})")));
                };
                Mask {
                    from: h.from.saturating_sub(CHECK_MARGIN_MS),
                    to: h.to + CHECK_MARGIN_MS,
                    row: h.row,
                    place: Place::Found { kind: h.kind, found: h.found.clone() },
                    line: e.line,
                    named: format!("check.txt:{n}, row {} {} {}", h.row, h.kind, h.found),
                }
            }
        });
    }
    Ok(out)
}

/// The masks over the cut's frames, in order: the cells each frame blanks, how many frames each covered and how many had
/// something under it; and which of the take's check places a mask hid wherever a frame showed them.
pub(crate) struct Tally<'a> {
    masks: &'a [Mask],
    hits: &'a [Hit],
    covered: Vec<u64>,
    under: Vec<u64>,
    /// (a place some mask's row and span hold whole)
    watched: Vec<bool>,
    /// (a watched place some frame showed despite the masks)
    shown: Vec<bool>,
}

impl<'a> Tally<'a> {
    pub(crate) fn new(masks: &'a [Mask], hits: &'a [Hit]) -> Tally<'a> {
        let watched = (hits.iter())
            .map(|h| masks.iter().any(|m| m.row == h.row && m.from <= h.from && h.to <= m.to))
            .collect();
        Tally {
            masks,
            hits,
            covered: vec![0; masks.len()],
            under: vec![0; masks.len()],
            watched,
            shown: vec![false; hits.len()],
        }
    }

    /// A frame of the video showing `screen`, the take's at `t` ms: the cells it draws blank (counted).
    pub(crate) fn frame(&mut self, t: u64, screen: &vt100::Screen) -> Vec<Blank> {
        let (rows, cols) = screen.size();
        let mut blank = Vec::new();
        for (i, m) in self.masks.iter().enumerate() {
            if !(m.from <= t && t < m.to) {
                continue;
            }
            self.covered[i] += 1;
            if m.row >= rows {
                continue;
            }
            let cells = row_cells(screen, m.row);
            let spans = match &m.place {
                &Place::Cells(a, b) => {
                    let b = b.saturating_add(1).min(cols);
                    if a < b { vec![(a, b)] } else { Vec::new() }
                }
                Place::Found { kind, found } => places(&cells, kind, found),
            };
            let inked = (spans.iter())
                .any(|&(a, b)| cells.iter().any(|c| a <= c.col && c.col < b && !c.text.trim().is_empty()));
            self.under[i] += u64::from(inked);
            blank.extend(spans.into_iter().map(|(a, b)| (m.row, a, b)));
        }
        blank.sort_unstable();
        blank.dedup();
        // (the check's places a mask holds: still there on the masked row? Looked for a little either side of their
        // times as written, for the file's rounding)
        for (j, h) in self.hits.iter().enumerate() {
            let near = h.from.saturating_sub(CHECK_MARGIN_MS) <= t && t < h.to + CHECK_MARGIN_MS;
            if self.watched[j]
                && !self.shown[j]
                && near
                && h.row < rows
                && still(&masked_row(screen, h.row, &blank), h)
            {
                self.shown[j] = true;
            }
        }
        blank
    }

    /// For each of the take's check places, whether the masks hid it: held whole by a mask's row and span, and shown
    /// by no frame despite them.
    pub(crate) fn hidden(&self) -> Vec<bool> {
        self.watched.iter().zip(&self.shown).map(|(w, s)| *w && !s).collect()
    }

    /// `cut.txt`'s lines (`file` the list's name): each mask, its span, the frames it covered; nothing with no mask.
    pub(crate) fn said(&self, file: &str) -> Vec<String> {
        if self.masks.is_empty() {
            return Vec::new();
        }
        let take = |ms: u64| format!("{}.{:02}", clock(ms), ms % 1000 / 10);
        let frames = |n: u64| format!("{n} frame{}", if n == 1 { "" } else { "s" });
        let mut out = vec![format!(
            "masks ({}): cells drawn blank in the screen's ground in every frame their span holds, the camera's crops and \
             zooms too",
            self.masks.len()
        )];
        for (i, m) in self.masks.iter().enumerate() {
            let (covered, under) = (self.covered[i], self.under[i]);
            let what = match (covered, under) {
                (0, _) => {
                    "MATCHES NO FRAME: no frame of the video shows the take then (left out, or past its end)"
                        .into()
                }
                (n, 0) => {
                    let none = match &m.place {
                        Place::Cells(..) => "its cells empty".to_string(),
                        Place::Found { found, .. } => format!("no {found} on row {}", m.row),
                    };
                    format!("MATCHES NO FRAME: {} covered, {none} in every one", frames(n))
                }
                (n, k) => format!("{} covered, something under it in {k}", frames(n)),
            };
            out.push(format!(
                "  {file}:{} — {}, take {}–{}: {what}",
                m.line,
                m.named,
                take(m.from),
                take(m.to)
            ));
        }
        out
    }

    /// The masks that matched no frame, said loudly (`said`'s lines for them).
    pub(crate) fn loud(&self, file: &str) -> Vec<String> {
        self.said(file)
            .into_iter()
            .filter(|l| l.contains("MATCHES NO FRAME"))
            .map(|l| l.trim().to_string())
            .collect()
    }
}

/// One cell of a row as the check reads it: its column, its text (a space where empty), and whether it's wide.
struct RowCell {
    col: u16,
    text: String,
    wide: bool,
}

/// A row's cells, a wide character's second cell left out.
fn row_cells(screen: &vt100::Screen, row: u16) -> Vec<RowCell> {
    let (_, cols) = screen.size();
    (0..cols)
        .filter_map(|c| screen.cell(row, c).map(|cell| (c, cell)))
        .filter(|(_, c)| !c.is_wide_continuation())
        .map(|(col, c)| RowCell {
            col,
            text: if c.contents().is_empty() { " ".into() } else { c.contents().to_string() },
            wide: c.is_wide(),
        })
        .collect()
}

/// The row's text as the check reads it, `blank`'s cells spaces.
fn masked_row(screen: &vt100::Screen, row: u16, blank: &[Blank]) -> String {
    let gone = |c: u16| blank.iter().any(|&(r, a, b)| r == row && a <= c && c < b);
    row_cells(screen, row).into_iter().map(|c| if gone(c.col) { " ".to_string() } else { c.text }).collect()
}

/// Whether the check still finds its place `h` in a row's `text`: its kind and what it found, by `check::found`.
fn still(text: &str, h: &Hit) -> bool {
    let mut y = Yours::default();
    match h.kind {
        "name" => y.names.push(h.found.clone()),
        "home" => y.homes.push(h.found.clone()),
        "mail" => y.mails.extend(h.found.strip_suffix(" (yours)").map(String::from)),
        _ => {}
    }
    check::found(text, &y).iter().any(|(k, f, _)| *k == h.kind && *f == h.found)
}

/// Where on a row (`cells`) the check's `found` (of `kind`) stands, each as its columns `from..to`: a name as a whole
/// word (the check's: a run of letters, digits, `_`, `-` and `.`, its end dots left off), a full name, a home, a mail
/// or an amount wherever its letters are (a mail's case aside).
fn places(cells: &[RowCell], kind: &str, found: &str) -> Vec<(u16, u16)> {
    let lower = |s: &str| s.chars().flat_map(char::to_lowercase).collect::<String>();
    let ch = |c: &RowCell| c.text.chars().next().unwrap_or(' ');
    let end = |c: &RowCell| c.col + if c.wide { 2 } else { 1 };
    let want = lower(found.strip_suffix(" (yours)").unwrap_or(found));
    let mut out = Vec::new();
    if kind == "name" && !want.contains(' ') {
        let word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
        let mut i = 0;
        while i < cells.len() {
            if !word(ch(&cells[i])) {
                i += 1;
                continue;
            }
            let mut j = i;
            while j < cells.len() && word(ch(&cells[j])) {
                j += 1;
            }
            let (mut a, mut b) = (i, j);
            while a < b && ch(&cells[a]) == '.' {
                a += 1;
            }
            while b > a && ch(&cells[b - 1]) == '.' {
                b -= 1;
            }
            let run: String = cells[a..b].iter().map(|c| c.text.as_str()).collect();
            if a < b && lower(&run) == want {
                out.push((cells[a].col, end(&cells[b - 1])));
            }
            i = j;
        }
        return out;
    }
    let n = want.chars().count();
    let hay: Vec<char> = cells.iter().map(|c| ch(c).to_lowercase().next().unwrap_or(' ')).collect();
    let needle: Vec<char> = want.chars().collect();
    if n == 0 {
        return out;
    }
    let mut i = 0;
    while i + n <= hay.len() {
        if hay[i..i + n] == needle[..] {
            out.push((cells[i].col, end(&cells[i + n - 1])));
            i += n;
        } else {
            i += 1;
        }
    }
    out
}
