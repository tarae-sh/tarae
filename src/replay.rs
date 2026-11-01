//! Replay — every stop of Claude's turns, kept in order: what it read (the range), what it edited (the
//! changed lines and what they replaced), what a search or a lookup found (its first hit and the count), a
//! command's or a test run's verdict, a note. `]r`/`[r` step through them at any time — while it works or
//! after — the editor going to each stop's file and lines (an edit's changed lines on the review's wash, the
//! lines it replaced above them); a click on a chat row of a call, or `C-]` there, goes to that call's
//! stop. The last `TURNS` turns of the chat are kept.
//!
//! Live follow (`follow.rs`) shows the newest stop as it comes; replay is the record, stepped at your pace.

use std::collections::VecDeque;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::chat::Look;

/// Turns of the chat whose stops are kept.
pub const TURNS: usize = 8;

/// A change an edit made: the lines it left (0-based, in the file right after it), the lines they replaced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub lines: Range<usize>,
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

/// One stop: the call it came from, its row (the tool's name, what, range), where it was.
#[derive(Clone, Debug)]
pub struct Stop {
    pub call: String,
    pub look: Look,
    pub path: Option<PathBuf>,
    /// 0-based lines (a read's range, an edit's changed lines, a search's first hit).
    pub lines: Option<Range<usize>>,
    /// An edit's changes (empty for the rest).
    pub hunks: Vec<Hunk>,
    /// What Claude said right before the call (the card says it at this stop).
    pub why: String,
    /// A page's read (`read` of a URL, web-plan D14): shown from the copy Claude read, never fetched again.
    pub page: Option<PageAt>,
    /// What the turn's map reads of it (`turnmap.rs`) beside what replay shows.
    pub map: Mapped,
}

/// A stop as the turn's map reads it, beside its place.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mapped {
    /// A further place of its call: a batched read's next range or file, an edit's next file, a search's next file —
    /// a stop of its own for `]r` and the map, the follow card's strip keeping one mark a call.
    pub more: bool,
    /// The file itself changed hands — removed, moved away or moved here: changed, with no lines to say so.
    pub whole: bool,
    /// What a `run_tests` runs (`testing::Target::holds`) — None on any other stop.
    pub holds: Option<Holds>,
    /// Its call came back an error, failed or refused: what it set out to do didn't happen.
    pub errored: bool,
    /// What the call's answer showed Claude, file by file, on its first stop (`follow::shown`): a read's numbered lines, a
    /// search's or a lookup's hits — taken from the whole answer as it went out, never the stream's first 4 KB, nor what
    /// was asked (an outline, a read cut at `LINES_MAX`, a search's later hits). What a place's check reads (`turnmap::File::check`).
    pub shown: Vec<Shown>,
}

/// Lines of a file an answer showed Claude.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub path: PathBuf,
    pub lines: Range<usize>,
    /// The file was seen to change under them since, by no edit of Claude's (`Replay::went_under`).
    pub old: bool,
}

/// What a test run holds of the file it runs (`testing::find`): all of it, or a test or group's lines (0-based).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Holds {
    File,
    Lines(Range<usize>),
}

/// A page Claude read: the address asked, and — once its answer landed — the copy read: its address after redirects
/// and when it was fetched (the session's `Pages` key it by both).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageAt {
    pub url: String,
    pub copy: Option<(String, std::time::Instant)>,
}

#[derive(Default)]
pub struct Turn {
    pub stops: Vec<Stop>,
    /// Which message of the conversation asked it and what it said — None: a turn no message of yours began.
    pub asked: Option<(usize, String)>,
    /// `Replay.begun` as it began.
    begun: u64,
}

#[derive(Default)]
pub struct Replay {
    /// Oldest first; the last is the turn under way (or the latest). Changed only by the methods here, each giving
    /// the stops a new `version`.
    turns: VecDeque<Turn>,
    /// The stops' version: a number no other stops anywhere had (`VERSIONS`), new at each change — 0: none yet.
    /// What's made from them is kept while it holds (`presence.rs`: #214, each busy session's places made anew every
    /// event).
    version: u64,
    /// The stop showing — (turn, stop) — while you step through them; None: live.
    pub at: Option<(usize, usize)>,
    /// Turns begun so far, an empty one begun again counted too.
    begun: u64,
    /// The files you changed, by path from the checkout's top, each with `begun` at your last change: lines the stops
    /// don't follow, so a turn begun by then leaves their signs out (`yours_in`, `turnmap.rs`). The session's, as its
    /// stops are; one goes once no turn kept began by then.
    pub yours: std::collections::HashMap<PathBuf, u64>,
}

/// The last stops' version given, every replay's (so one replay's version is never another's stops).
static VERSIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl Replay {
    /// The turns kept, oldest first.
    pub fn turns(&self) -> &VecDeque<Turn> {
        &self.turns
    }

    /// The stops' version (`version`): the same while they are.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The stops are about to change: a new version.
    fn changes(&mut self) {
        self.version = VERSIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    }

    /// The newest stop `pick` takes, to change — a new version only when there is one (#214: a lookup that found
    /// nothing made every busy session's presence again).
    pub fn find_mut(&mut self, pick: impl Fn(&Stop) -> bool) -> Option<&mut Stop> {
        let (t, s) = self
            .turns
            .iter()
            .enumerate()
            .rev()
            .find_map(|(t, turn)| turn.stops.iter().rposition(&pick).map(|s| (t, s)))?;
        self.changes();
        Some(&mut self.turns[t].stops[s])
    }

    /// A new question: a turn of its own (an empty last one is reused), the oldest beyond `TURNS` dropped.
    pub fn begin(&mut self) {
        self.begun += 1;
        // (begun anew: what you changed before this question isn't after it)
        if let Some(t) = self.turns.back_mut().filter(|t| t.stops.is_empty()) {
            t.begun = self.begun;
            return;
        }
        self.changes();
        self.turns.push_back(Turn { begun: self.begun, ..Turn::default() });
        while self.turns.len() > TURNS {
            self.turns.pop_front();
            self.at = self.at.and_then(|(t, s)| t.checked_sub(1).map(|t| (t, s)));
        }
        let oldest = self.turns.front().map_or(0, |t| t.begun);
        self.yours.retain(|_, at| *at >= oldest);
    }

    /// You changed `rel`: every turn begun so far holds lines of it that have moved under them, on the map in sight or
    /// not. Each key of yours: no stop is looked at.
    pub fn you_changed(&mut self, rel: &Path) {
        if !self.turns.is_empty() && self.yours.get(rel) != Some(&self.begun) {
            self.yours.insert(rel.to_path_buf(), self.begun);
        }
    }

    /// You changed `rel` since turn `t` began: its lines there aren't the stops'.
    pub fn yours_in(&self, t: usize, rel: &Path) -> bool {
        self.turns.get(t).zip(self.yours.get(rel)).is_some_and(|(t, at)| t.begun <= *at)
    }

    /// A read of `call` found `path` changed since Claude last saw it, by no edit of its: what this turn's earlier calls
    /// showed of it isn't what's there.
    pub fn went_under(&mut self, call: &str, path: &Path) {
        self.changes();
        let turn = self.turns.back_mut().into_iter().flat_map(|t| t.stops.iter_mut());
        let earlier = turn.filter(|s| s.call != call).flat_map(|s| s.map.shown.iter_mut());
        earlier.filter(|x| x.path == path).for_each(|x| x.old = true);
    }

    /// The turn just begun: message `n` of the conversation asked it (`Turn.asked`).
    pub fn asked(&mut self, n: usize, text: &str) {
        self.changes();
        if self.turns.is_empty() {
            self.turns.push_back(Turn::default());
        }
        self.turns.back_mut().expect("just made").asked = Some((n, text.to_string()));
    }

    pub fn add(&mut self, stop: Stop) {
        self.changes();
        if self.turns.is_empty() {
            self.turns.push_back(Turn::default());
        }
        self.turns.back_mut().expect("just made").stops.push(stop);
    }

    /// `stop`, a further place of `call`'s (`Mapped.more`), right after that call's last stop.
    pub fn after_call(&mut self, call: &str, mut stop: Stop) {
        let Some((t, s)) = self.turns.iter().enumerate().rev().find_map(|(t, turn)| {
            turn.stops.iter().rposition(|s| !call.is_empty() && s.call == call).map(|s| (t, s))
        }) else {
            return;
        };
        self.changes();
        stop.map.more = true;
        self.turns[t].stops.insert(s + 1, stop);
        self.at = self.at.map(|(at, k)| (at, k + usize::from(at == t && k > s)));
    }

    /// The stop of `call` at `path`: the one there, else the call's with no place yet, else one more of its own after
    /// the call's last (a call that wrote several files) — a copy of it, to fill in. None: no stop of that call.
    pub fn at_place(&mut self, call: &str, path: &Path) -> Option<&mut Stop> {
        let (t, _) = self.place_of(call)?;
        let of = |s: &Stop| s.call == call;
        let stops = &self.turns[t].stops;
        let there = stops.iter().position(|s| of(s) && s.path.as_deref() == Some(path));
        let at = there.or_else(|| stops.iter().position(|s| of(s) && s.path.is_none()));
        if let Some(s) = at {
            self.changes();
            return Some(&mut self.turns[t].stops[s]);
        }
        let last = stops.iter().rposition(of)?;
        let mut more = stops[last].clone();
        more.hunks.clear();
        more.map = Mapped::default();
        more.path = Some(path.to_path_buf());
        self.after_call(call, more);
        Some(&mut self.turns[t].stops[last + 1])
    }

    /// `call`'s stops, to change — a new version only when there's one.
    pub fn of_call(&mut self, call: &str) -> impl Iterator<Item = &mut Stop> {
        let turn = self.place_of(call).map(|p| p.0);
        if turn.is_some() {
            self.changes();
        }
        let stops = turn.and_then(|t| self.turns.get_mut(t)).map(|t| &mut t.stops);
        stops.into_iter().flatten().filter(move |s| s.call == call)
    }

    /// The stop of tool call `call`, to change — its first, a call of several places: the one its row is (`place_of`), a
    /// further place keeping its own words.
    pub fn by_call(&mut self, call: &str) -> Option<&mut Stop> {
        let (t, s) = self.place_of(call)?;
        self.changes();
        self.turns.get_mut(t)?.stops.get_mut(s)
    }

    /// Where the stop of `call` is — its first, a call of several places.
    pub fn place_of(&self, call: &str) -> Option<(usize, usize)> {
        self.turns.iter().enumerate().rev().find_map(|(t, turn)| {
            turn.stops.iter().position(|s| !call.is_empty() && s.call == call).map(|s| (t, s))
        })
    }

    pub fn get(&self, at: (usize, usize)) -> Option<&Stop> {
        self.turns.get(at.0)?.stops.get(at.1)
    }

    /// Stops of the latest turn that has any.
    pub fn latest(&self) -> usize {
        self.turns.iter().rev().map(|t| t.stops.len()).find(|&n| n > 0).unwrap_or(0)
    }

    /// An edit to `path` moved its lines (`changes`: old → new line ranges, in order): the stops recorded
    /// before it there follow (a read of lines below it, an earlier edit's lines).
    pub fn edited(&mut self, path: &Path, changes: &[(Range<usize>, Range<usize>)]) {
        // (a new version only when a stop is there — the edit of a file the turn never looked at moves none)
        let shown = |s: &Stop| s.map.shown.iter().any(|x| x.path == path);
        if !self.turns.iter().flat_map(|t| &t.stops).any(|s| s.path.as_deref() == Some(path) || shown(s)) {
            return;
        }
        self.changes();
        for stop in self.turns.iter_mut().flat_map(|t| t.stops.iter_mut()) {
            for x in stop.map.shown.iter_mut().filter(|x| x.path == path) {
                x.lines = moved_range(&x.lines, changes);
            }
            if stop.path.as_deref() != Some(path) {
                continue;
            }
            if let Some(l) = &mut stop.lines {
                *l = moved_range(l, changes);
                // (its row's `L12–13` says where it is now — a ` +2` after it stays)
                if let Some(rest) = stop.look.range.strip_prefix('L')
                    && rest.starts_with(|c: char| c.is_ascii_digit())
                {
                    let tail = rest.find(' ').map_or("", |i| &rest[i..]);
                    stop.look.range = format!("{}{tail}", l_range(l));
                }
            }
            if let Some(Holds::Lines(l)) = &mut stop.map.holds {
                *l = moved_range(l, changes);
            }
            for h in &mut stop.hunks {
                h.lines = if h.lines.is_empty() {
                    let at = moved(h.lines.start, changes, false);
                    at..at
                } else {
                    moved_range(&h.lines, changes)
                };
            }
        }
    }

    /// The next (`forward`) or previous stop from where it is — from live, back is the newest stop and
    /// forward the latest turn's first. None: past the newest (back to live) or before the oldest.
    pub fn step(&self, forward: bool) -> Option<(usize, usize)> {
        let all: Vec<(usize, usize)> = self
            .turns
            .iter()
            .enumerate()
            .flat_map(|(t, turn)| (0..turn.stops.len()).map(move |s| (t, s)))
            .collect();
        let last = *all.last()?;
        let Some(at) = self.at else {
            return Some(if forward { (last.0, 0) } else { last });
        };
        let i = all.iter().position(|&p| p == at)?;
        if forward { all.get(i + 1).copied() } else { i.checked_sub(1).map(|i| all[i]) }
    }

    /// Which stop of how many in its turn, and how many turns back (0: the latest).
    pub fn position(&self, at: (usize, usize)) -> (usize, usize, usize) {
        let n = self.turns.get(at.0).map_or(0, |t| t.stops.len());
        let latest = self.turns.iter().rposition(|t| !t.stops.is_empty()).unwrap_or(0);
        (at.1 + 1, n, latest.saturating_sub(at.0))
    }
}

/// `L12` · `L12–30` — 0-based lines as a row says them.
pub fn l_range(lines: &Range<usize>) -> String {
    if lines.len() > 1 {
        format!("L{}–{}", lines.start + 1, lines.end)
    } else {
        format!("L{}", lines.start + 1)
    }
}

/// `old` → `new` as an edit's hunks, with the old lines each replaced (for `Replay::edited`).
pub fn hunks(old: &str, new: &str) -> Vec<(Range<usize>, Hunk)> {
    use imara_diff::intern::InternedInput;
    use imara_diff::{Algorithm, diff};
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let input = InternedInput::new(old, new);
    let mut out = Vec::new();
    diff(Algorithm::Histogram, &input, |before: Range<u32>, after: Range<u32>| {
        let (bs, be) = (before.start as usize, before.end as usize);
        let (as_, ae) = (after.start as usize, after.end as usize);
        let lines = |v: &[&str], r: Range<usize>| -> Vec<String> {
            v.get(r.start.min(v.len())..r.end.min(v.len()))
                .unwrap_or_default()
                .iter()
                .map(|l| l.to_string())
                .collect()
        };
        out.push((bs..be, Hunk { lines: as_..ae, removed: lines(&a, bs..be), added: lines(&b, as_..ae) }));
    });
    out
}

/// Line `line` of the old text in the new one — a line inside a replaced range lands on the replacement's
/// first line (its last, for a range's `last`).
fn moved(line: usize, changes: &[(Range<usize>, Range<usize>)], last: bool) -> usize {
    let mut delta: isize = 0;
    for (old, new) in changes {
        if line < old.start {
            break;
        }
        if old.is_empty() || line >= old.end {
            delta += new.len() as isize - old.len() as isize;
        } else if last {
            return new.end.saturating_sub(1).max(new.start);
        } else {
            return new.start;
        }
    }
    (line as isize + delta).max(0) as usize
}

fn moved_range(r: &Range<usize>, changes: &[(Range<usize>, Range<usize>)]) -> Range<usize> {
    let start = moved(r.start, changes, false);
    let end = moved(r.end.saturating_sub(1).max(r.start), changes, true) + 1;
    start..end.max(start + 1)
}

/// The lines each section of a `read` answer gave (`== what …`, then `12: code`): its header after `== `, and the first
/// and last numbered line under it (0-based, past the last) — a read by name's lines come only from its answer.
pub fn read_sections(text: &str) -> Vec<(&str, Range<usize>)> {
    let mut out: Vec<(&str, Range<usize>)> = Vec::new();
    let mut open: Option<&str> = None;
    for l in text.lines() {
        if let Some(head) = l.strip_prefix("== ") {
            open = Some(head);
            continue;
        }
        let digits = l.bytes().take_while(u8::is_ascii_digit).count();
        let Some(n) = l[..digits].parse::<usize>().ok().filter(|_| l[digits..].starts_with(':')) else {
            continue;
        };
        let line = n.saturating_sub(1);
        match out.last_mut().filter(|s| Some(s.0) == open) {
            Some(s) => s.1.end = line + 1,
            None => out.extend(open.map(|head| (head, line..line + 1))),
        }
    }
    out
}

/// The places a search's or a lookup's answer names, in order (its first 4 KB): `path:line` (Grep's
/// content mode, a definition), a path line followed by indented `  12: code` hits (tarae's search and
/// references), or bare paths (files only). Relative paths are from `root`.
pub fn hits(root: &Path, text: &str) -> Vec<(PathBuf, usize)> {
    places_in(root, text).into_iter().map(|(p, l, _)| (p, l)).collect()
}

/// The lines `text`, a search's or a lookup's whole answer, showed: a hit each, never a path alone (`output: files`).
pub fn hit_lines(root: &Path, text: &str) -> Vec<Shown> {
    let hit =
        |(path, l, line): (PathBuf, usize, bool)| line.then(|| Shown { path, lines: l..l + 1, old: false });
    places_in(root, text).into_iter().filter_map(hit).collect()
}

/// `hits`, each with whether the answer shows that line — false: a path alone, line 0 only standing in.
fn places_in(root: &Path, text: &str) -> Vec<(PathBuf, usize, bool)> {
    let mut out: Vec<(PathBuf, usize, bool)> = Vec::new();
    // A path line not yet followed by its hits — a hit of its own (line 1) if none come
    let mut file: Option<(PathBuf, bool)> = None;
    let close = |file: &mut Option<(PathBuf, bool)>, out: &mut Vec<(PathBuf, usize, bool)>| {
        if let Some((p, false)) = file.take() {
            out.push((p, 0, false));
        }
    };
    for l in text.lines() {
        if l.trim().is_empty() {
            continue;
        }
        if l.starts_with(char::is_whitespace) {
            let t = l.trim_start();
            let digits = t.bytes().take_while(u8::is_ascii_digit).count();
            if let Some((p, seen)) = file.as_mut()
                && digits > 0
                && t[digits..].starts_with(':')
                && let Ok(n) = t[..digits].parse::<usize>()
            {
                out.push((p.clone(), n.saturating_sub(1), true));
                *seen = true;
            }
            continue;
        }
        if let Some((_, link)) =
            crate::chat::refs_in(l, |n| Some(crate::chat::resolve(root, n))).into_iter().next()
        {
            close(&mut file, &mut out);
            out.push((link.path, link.line, true));
            continue;
        }
        // A bare path: one word that looks like a file
        let t = l.trim();
        let file_like = !t.contains(char::is_whitespace)
            && (t.contains('/') || t.rsplit_once('.').is_some_and(|(a, e)| !a.is_empty() && !e.is_empty()));
        if file_like {
            close(&mut file, &mut out);
            file = Some((crate::chat::resolve(root, t), false));
        }
    }
    close(&mut file, &mut out);
    out
}

/// What a search found, as the card and the row say it: the tool's own count (`12 references in 3
/// files`) — else counted from its hits (`12 hits in 4 files`, `+` when its answer was cut).
pub fn count(text: &str, hits: &[(PathBuf, usize)], cut: bool) -> String {
    if let Some(v) = crate::nav::verdict(text) {
        return v;
    }
    if hits.is_empty() {
        return String::new();
    }
    let mut files: Vec<&PathBuf> = hits.iter().map(|h| &h.0).collect();
    crate::sorting::sort(&mut files);
    files.dedup();
    let plus = if cut { "+" } else { "" };
    let n = hits.len();
    let s = |n: usize, w: &str| if n == 1 { format!("{n}{plus} {w}") } else { format!("{n}{plus} {w}s") };
    if hits.iter().all(|h| h.1 == 0) && files.len() == n {
        return s(n, "file");
    }
    format!(
        "{} in {}",
        s(n, "hit"),
        if files.len() == 1 { "1 file".into() } else { format!("{} files", files.len()) }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(path: &str, lines: Range<usize>) -> Stop {
        let look = Look { verb: "read", what: path.into(), pattern: false, range: String::new(), link: None };
        let path = Some(PathBuf::from(path));
        let why = String::new();
        let (hunks, map) = (Vec::new(), Mapped::default());
        Stop { call: String::new(), look, path, lines: Some(lines), hunks, why, page: None, map }
    }

    /// What a test run holds moves as a stop's lines do ("tested" compares it with the changes as they are now), and
    /// a further place put after its call keeps the stop in hand the one it was.
    #[test]
    fn holds_follow_edits_and_a_further_place_keeps_the_hand() {
        let mut r = Replay::default();
        let mut held = stop("a.rs", 5..6);
        held.map.holds = Some(Holds::Lines(5..9));
        r.add(held);
        r.edited(Path::new("a.rs"), &[(1..1, 1..4)]);
        assert_eq!(r.get((0, 0)).unwrap().map.holds, Some(Holds::Lines(8..12)));

        let mut r = Replay::default();
        let of = |call: &str| Stop { call: call.into(), ..stop("a.rs", 0..1) };
        [of("one"), of("two")].into_iter().for_each(|s| r.add(s));
        r.at = Some((0, 1));
        r.after_call("one", of("one"));
        assert_eq!(r.at, Some((0, 2)), "still `two`, one further on");
        assert!(r.get((0, 1)).unwrap().map.more);
        r.at = Some((0, 0));
        r.after_call("two", of("two"));
        assert_eq!(r.at, Some((0, 0)), "before it: where it was");
    }

    /// An edit's hunks: the lines it left, the ones they replaced — and the old ranges, for moving others.
    #[test]
    fn an_edit_becomes_hunks() {
        let h = hunks("a\nb\nc\nd\n", "a\nB\nB2\nc\n");
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].0, 1..2);
        assert_eq!(
            h[0].1,
            Hunk { lines: 1..3, removed: vec!["b".into()], added: vec!["B".into(), "B2".into()] }
        );
        assert_eq!(h[1].1.removed, vec!["d".to_string()]);
        assert!(h[1].1.added.is_empty() && h[1].1.lines == (4..4));
    }

    /// A later edit in the same file moves what was recorded there before it: lines below go with it, a
    /// range inside what it replaced lands on the replacement, lines above stay.
    #[test]
    fn later_edits_move_earlier_stops() {
        let mut r = Replay::default();
        r.begin();
        r.add(stop("a.rs", 0..2));
        r.add(stop("a.rs", 10..15));
        r.add(stop("b.rs", 10..15));
        r.add(stop("a.rs", 4..6));
        // Lines 3..5 (3 lines) became 2 new lines, and 2 lines were inserted at old line 8
        r.edited(Path::new("a.rs"), &[(3..6, 3..5), (8..8, 7..9)]);
        let lines: Vec<_> = r.turns[0].stops.iter().map(|s| s.lines.clone().unwrap()).collect();
        assert_eq!(lines, [0..2, 11..16, 10..15, 3..5]);
    }

    /// The stops' version is new only when a stop is found or moved (#214): a lookup that finds nothing — an id no stop
    /// has, an empty one, an edit of a file no stop is in — keeps it; one that finds a stop, or an edit that moves one,
    /// doesn't.
    #[test]
    fn a_lookup_finding_nothing_keeps_the_version() {
        let mut r = Replay::default();
        r.begin();
        r.add(Stop { call: "c1".into(), ..stop("a.rs", 0..2) });
        let v = r.version();
        assert!(r.by_call("c2").is_none());
        assert!(r.by_call("").is_none());
        assert!(r.find_mut(|s| s.page.is_some()).is_none());
        r.edited(Path::new("b.rs"), &[(0..1, 0..2)]);
        assert_eq!(r.version(), v, "nothing found, nothing moved");
        assert!(r.by_call("c1").is_some());
        let found = r.version();
        assert_ne!(found, v, "a stop found, to change");
        r.edited(Path::new("a.rs"), &[(0..1, 0..2)]);
        assert_ne!(r.version(), found, "a stop moved");
    }

    /// Your edit is under every turn begun by then and none begun after it, and goes once no such turn is kept.
    #[test]
    fn your_edit_holds_for_the_turns_before_it() {
        let a = Path::new("a.rs");
        let mut r = Replay::default();
        r.you_changed(a);
        assert!(r.yours.is_empty(), "no turn to be under");
        r.begin();
        r.add(stop("a.rs", 0..1));
        r.you_changed(a);
        r.begin();
        r.add(stop("a.rs", 0..1));
        assert!(r.yours_in(0, a) && !r.yours_in(1, a));
        assert!(!r.yours_in(0, Path::new("b.rs")));
        for _ in 0..TURNS {
            r.begin();
            r.add(stop("a.rs", 0..1));
        }
        assert!(r.yours.is_empty(), "no turn kept began by then");
    }

    /// A call of several places: what's said of its row goes to its first stop — the row's words on its last place's would
    /// overwrite that place's own.
    #[test]
    fn a_calls_row_is_its_first_stop() {
        let mut r = Replay::default();
        r.begin();
        r.add(Stop { call: "c1".into(), ..stop("a.rs", 0..2) });
        r.after_call("c1", Stop { call: "c1".into(), ..stop("b.rs", 4..5) });
        r.by_call("c1").unwrap().look.range = "L1–2 +1 more".into();
        let said: Vec<&str> = r.turns[0].stops.iter().map(|s| s.look.range.as_str()).collect();
        assert_eq!(said, ["L1–2 +1 more", ""]);
    }

    /// Stepping: from live, back is the newest stop and forward the latest turn's first; across turns; past
    /// the newest is live again (None).
    #[test]
    fn stepping_through_turns() {
        let mut r = Replay::default();
        r.begin();
        r.add(stop("a", 0..1));
        r.add(stop("b", 0..1));
        r.begin();
        r.begin(); // (an empty turn is reused)
        r.add(stop("c", 0..1));
        r.add(stop("d", 0..1));
        assert_eq!(r.turns.len(), 2);
        assert_eq!(r.step(false), Some((1, 1)));
        assert_eq!(r.step(true), Some((1, 0)));
        r.at = Some((1, 0));
        assert_eq!(r.step(false), Some((0, 1)), "into the turn before");
        r.at = Some((1, 1));
        assert_eq!(r.step(true), None, "past the newest: live");
        r.at = Some((0, 0));
        assert_eq!(r.step(false), None);
        assert_eq!(r.position((0, 1)), (2, 2, 1));
        for _ in 0..TURNS + 2 {
            r.begin();
            r.add(stop("e", 0..1));
        }
        assert_eq!(r.turns.len(), TURNS, "only the last few turns");
    }

    /// Focus F0: a search's or a lookup's answer shows its hits' lines, whole or by its path lines, never a path alone.
    #[test]
    fn the_lines_an_answer_showed_are_its_hits_alone() {
        let root = Path::new("/p");
        let shown = |text: &str| -> Vec<(String, usize)> {
            let hits = hit_lines(root, text);
            hits.iter().map(|x| (x.path.display().to_string(), x.lines.start + 1)).collect()
        };
        let at = |p: &str, l| (p.to_string(), l);
        // (a search with its path lines, a place written whole; never a path alone — `output: files`)
        assert_eq!(
            shown("src/a.rs\n  in fn x (1–9)\n  12: code\n  --\n  30: more\nb.rs:7: code\n"),
            [at("/p/src/a.rs", 12), at("/p/src/a.rs", 30), at("/p/b.rs", 7)]
        );
        assert_eq!(shown("src/a.rs\nsrc/b.rs\n"), []);
        assert_eq!(hits(root, "src/a.rs\n"), [(PathBuf::from("/p/src/a.rs"), 0)], "as a place it stays");
    }

    /// What an answer showed follows Claude's edits wherever the stop is, a search's other files too; a read that finds the
    /// file changed under it marks what this turn's earlier calls showed of it — no other file's, no other turn's, nor its own.
    #[test]
    fn what_was_shown_follows_edits_and_goes_old() {
        let shown = |path: &str, lines| Shown { path: path.into(), lines, old: false };
        let with = |call: &str, x: Vec<Shown>| Stop {
            call: call.to_string(),
            map: Mapped { shown: x, ..Mapped::default() },
            ..stop("other", 0..1)
        };
        let mut r = Replay::default();
        r.begin();
        r.add(with("before", vec![shown("a", 0..2)]));
        r.begin();
        r.add(with("s", vec![shown("a", 10..11), shown("b", 10..11)]));
        r.add(with("r", vec![shown("a", 20..21)]));
        r.edited(Path::new("a"), &[(2..2, 2..5)]);
        let at = |r: &Replay, t: usize, s: usize| r.turns()[t].stops[s].map.shown.clone();
        assert_eq!(at(&r, 1, 0)[0].lines, 13..14);
        assert_eq!(at(&r, 1, 0)[1].lines, 10..11, "another file's stay");
        r.went_under("r", Path::new("a"));
        let old = |x: Vec<Shown>| x.iter().map(|x| x.old).collect::<Vec<_>>();
        assert_eq!(old(at(&r, 1, 0)), [true, false], "the earlier call's, that file's alone");
        assert_eq!(old(at(&r, 1, 1)), [false], "the read that found it");
        assert_eq!(old(at(&r, 0, 0)), [false], "an earlier turn was judged at its end");
    }

    /// The places a search names, in its tools' shapes.
    #[test]
    fn hits_in_search_answers() {
        let root = Path::new("/p");
        let tarae = "3 matching lines in 2 files:\nsrc/a.rs\n   12:     fn a()\n  --\n   40: a()\nsrc/b.rs\n    7: a()\n";
        assert_eq!(
            hits(root, tarae),
            [(root.join("src/a.rs"), 11), (root.join("src/a.rs"), 39), (root.join("src/b.rs"), 6)]
        );
        assert_eq!(count(tarae, &hits(root, tarae), false), "3 matching lines in 2 files");
        let grep = "/p/src/a.rs:3:fn a\n/p/src/c.rs:9:a()\n";
        let h = hits(root, grep);
        assert_eq!(h, [(root.join("src/a.rs"), 2), (root.join("src/c.rs"), 8)]);
        assert_eq!(count(grep, &h, false), "2 hits in 2 files");
        let files = "Found 2 files\n/p/src/a.rs\n/p/src/b.rs\n";
        let h = hits(root, files);
        assert_eq!(h, [(root.join("src/a.rs"), 0), (root.join("src/b.rs"), 0)]);
        assert_eq!(count(files, &h, true), "2+ files");
        assert!(hits(root, "No matches found").is_empty());
    }
}
