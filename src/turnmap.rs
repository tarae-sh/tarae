//! focus E1 — a turn as a map, not a log (`.sb/focus-plan.md` §7 E): the places the last turn touched, on the code's own
//! structure, by role — `◦` read · `•` searched · `●` changed · `▸` tested · `▲` failed, motion's one vocabulary
//! (`chat::Mark`). The side panel's file rows say each file's roles by weight, a folded folder its strongest, the outline
//! a definition's, the sign column a faint mark on the lines (under every other sign, as presence's `▎`), and the panel's
//! top line names the turn. No token, no process, no timer: read only from the replay's stops (`replay.rs`), folded again
//! when they change (`sync`, a stop's work, never an event's), kept until the next turn starts — a single session too,
//! where presence draws nothing; with 2+ sessions the front session's turn only, the others stay presence's. The turn is
//! the one replay shows (`follow::strip`'s rule): `[r` into an older turn shows that turn's map, the stop in hand lit.
//!
//! **Tested** is a claim, so it has one rule: a `run_tests` that passed, after the last change to that file, whose
//! target holds a change there (`testing::Target::holds`: the file's tests, or a test's or group's lines). A plain `run`
//! never — tarae can't know what a command exercised — nor a `run_tests` of another file. `▲` claims less — a test failed
//! there — so any failure the editor went to says it, until that run passes or its file is changed.
//!
//! Your own edits move lines the stops don't follow: in every turn begun before your edit, on the map in sight or not, a
//! buffer you changed keeps its file row and loses its signs (`Replay.yours`) — a mark on the wrong line reads worse than
//! none.
//!
//! **What tarae knows without asking** (focus F0, `.sb/focus-plan.md` §7 F): the same fold, at a turn's end, judges each
//! place its answer names (`Judge`, `File::check`) — read this turn, not read, changed and no test seen, E1's rule. It says
//! what tarae saw, never whether the answer is true: `Read` only of lines an answer showed Claude (`Mapped.shown`), and
//! only while the file is as Claude last saw it (`Judged::word`, by `Edits.seen`'s stamps). A card says every verdict, a
//! line end only its doubts (`Judged::doubt`). A place is judged by its real path, as what was shown and seen are kept
//! (`places::Known::place`).

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::chat::Mark;
use crate::disk::Stamp;
use crate::document::{DocId, Document};
use crate::editor::Editor;
use crate::event::WorkspaceId;
use crate::replay::{Holds, Replay, Stop};

/// The roles by weight: what a row says first.
const WEIGHT: [Mark; 5] = [Mark::Failed, Mark::Edit, Mark::Run, Mark::Lookup, Mark::Read];

/// A count past this says `9+`.
const COUNT_MAX: usize = 9;

fn weight(m: Mark) -> usize {
    WEIGHT.iter().position(|w| *w == m).unwrap_or(WEIGHT.len())
}

/// One role on a row: its mark and the stops that gave it, oldest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Role {
    pub mark: Mark,
    pub stops: Vec<usize>,
}

impl Role {
    /// As a row says it: its mark, a read's count when read more than once (`◦ 3`, past `COUNT_MAX` `◦ 9+`).
    pub fn said(&self) -> String {
        match self.stops.len() {
            n if self.mark != Mark::Read || n < 2 => self.mark.glyph().to_string(),
            n if n > COUNT_MAX => format!("{} {COUNT_MAX}+", self.mark.glyph()),
            n => format!("{} {n}", self.mark.glyph()),
        }
    }

    /// In words, as the row in hand's foot says it.
    fn word(&self) -> String {
        match self.mark {
            Mark::Failed => "failed".into(),
            Mark::Edit => "changed".into(),
            Mark::Run => "tested".into(),
            Mark::Lookup => "searched".into(),
            Mark::Read => match self.stops.len() {
                n if n > 1 => format!("read {n}×"),
                _ => "read".into(),
            },
        }
    }

    /// The stop a press on it goes to: the next of its after the one in hand, else its first — pressed again, the one after.
    pub fn press(&self, hand: Option<usize>) -> usize {
        let next = self.stops.iter().find(|&&s| Some(s) > hand);
        *next.or(self.stops.first()).unwrap_or(&0)
    }
}

/// Lines a stop touched in its role.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Touched {
    pub lines: Range<usize>,
    pub mark: Mark,
    pub stop: usize,
}

/// A file on the map: its roles by weight, and what they touched line by line (a whole read: no lines).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct File {
    pub roles: Vec<Role>,
    pub lines: Vec<Touched>,
    /// The roles a stop of gave the file with no lines: a change of the file itself, a failure of a run of it.
    whole: Vec<Mark>,
    /// What answers showed Claude of it, with whether the file changed under them since (`replay::Shown`).
    shown: Vec<(Range<usize>, bool)>,
    /// A read's answer showed lines of it, by whatever name it was asked: a read through a link has its role on the
    /// link's own path, what it showed here.
    shown_read: bool,
}

/// What tarae saw of a place an answer names, in the turn that answered (focus F0). Never whether what's said is true.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    /// An answer showed Claude every line of it.
    Read,
    /// …but a later read found the file changed under them.
    ReadThenChanged,
    /// Not every line of it was shown: said from memory, an earlier turn or a guess.
    NotRead,
    /// The turn changed it, and no `run_tests` holding the change passed after (`▸`'s rule).
    NotRunOver,
    /// …and one did.
    RunOver,
    /// …and a test failed there since (`▲`'s).
    Failed,
}

impl File {
    /// What the turn folded here saw of `lines`: a change there first — what's changed was written, not read —, then
    /// whether they were shown.
    pub fn check(&self, lines: &Range<usize>) -> Check {
        let meets = |l: &Range<usize>| l.start < lines.end && lines.start < l.end;
        let at = |m: Mark| self.lines.iter().filter(move |t| t.mark == m && meets(&t.lines));
        let (whole, changed) = (self.whole.contains(&Mark::Edit), at(Mark::Edit).count() > 0);
        if whole || changed {
            let ran = |t: &Touched| at(Mark::Run).any(|r| r.lines == t.lines);
            return match () {
                _ if self.whole.contains(&Mark::Failed) || at(Mark::Failed).count() > 0 => Check::Failed,
                _ if whole && !self.whole.contains(&Mark::Run) => Check::NotRunOver,
                _ if at(Mark::Edit).all(ran) => Check::RunOver,
                _ => Check::NotRunOver,
            };
        }
        let covered = |old: bool| {
            let mut parts: Vec<&Range<usize>> =
                self.shown.iter().filter(|(_, was)| old || !was).map(|(l, _)| l).collect();
            crate::sorting::by_key(&mut parts, |l| l.start);
            let mut from = lines.start;
            for l in parts {
                if l.start > from {
                    break;
                }
                from = from.max(l.end);
            }
            from >= lines.end
        };
        match () {
            _ if covered(false) => Check::Read,
            _ if covered(true) => Check::ReadThenChanged,
            _ => Check::NotRead,
        }
    }
}

/// A place's verdict as a note keeps it (`Note.check`): judged once, as its turn ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Judged {
    pub check: Check,
    /// The file as tarae knew it then — what `Read` and `RunOver` hold only while it's still so.
    saw: Option<Stamp>,
}

impl Judged {
    /// As the note says it, `doc` the file's buffer: what the file is now, unsaved edits too (`read` reads the disk). Every
    /// frame it's drawn: two stamps compared, no disk.
    pub fn word(&self, doc: &Document) -> &'static str {
        let since = doc.is_modified() || doc.disk.is_some_and(|d| Some(d) != self.saw);
        match self.check {
            Check::Read if since => "read, changed since",
            Check::Read => "read this turn",
            Check::ReadThenChanged => "read, changed since",
            Check::NotRead => "not read this turn",
            Check::NotRunOver => "changed, no test seen",
            Check::RunOver if since => "tested, changed since",
            Check::RunOver => "changed, tested",
            Check::Failed => "changed, test failed",
        }
    }

    /// `word` where it's a doubt — what a line end says; what's as it should be stays on the card.
    pub fn doubt(&self, doc: &Document) -> Option<&'static str> {
        let word = self.word(doc);
        (!matches!(word, "read this turn" | "changed, tested")).then_some(word)
    }
}

/// A turn's stops as its answer's places are judged, taken as it ended (`chatnotes::turn_over`): the places are placed
/// later, and another turn may have begun by then.
#[derive(Clone, Debug, Default)]
pub struct Judge {
    root: PathBuf,
    files: HashMap<PathBuf, File>,
    /// What Claude last saw of the files the turn read or changed (`Edits.seen`).
    seen: HashMap<PathBuf, Stamp>,
}

impl Judge {
    pub fn of(stops: &[Stop], root: &Path, seen: &HashMap<PathBuf, Stamp>) -> Judge {
        let files = fold_all(stops, root).0;
        let looked =
            |f: &File| f.shown_read || f.roles.iter().any(|r| matches!(r.mark, Mark::Read | Mark::Edit));
        let seen = seen.iter().filter(|(p, _)| files.get(rel_of(p, root)).is_some_and(looked));
        Judge { root: root.to_path_buf(), seen: seen.map(|(p, s)| (p.clone(), *s)).collect(), files }
    }

    /// The verdict on `lines` of `path`, `doc` its buffer if it has one. None where `Read` or `RunOver` has nothing to
    /// hold against later — a search's file not open —: nothing said, never a guess.
    pub fn judged(&self, path: &Path, lines: &Range<usize>, doc: Option<&Document>) -> Option<Judged> {
        let file = self.files.get(rel_of(path, &self.root));
        let check = file.map_or(Check::NotRead, |f| f.check(lines));
        let open = || doc.filter(|d| !d.is_modified()).and_then(|d| d.disk);
        let saw = self.seen.get(path).copied().or_else(open);
        (saw.is_some() || !matches!(check, Check::Read | Check::RunOver)).then_some(Judged { check, saw })
    }
}

/// `path` as the map keys it: from the checkout's top, a place outside the project whole.
fn rel_of<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

/// The sign column's mark on lines of a buffer (`Shows.map`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sign {
    pub lines: Range<usize>,
    pub mark: Mark,
    /// Its stop, in the turn replay shows.
    pub at: (usize, usize),
    pub in_hand: bool,
}

/// What a map was folded from: the front workspace, its stops' version, the stop replay shows.
type FoldedFrom = (WorkspaceId, u64, Option<(usize, usize)>);

/// The map of the turn replay shows (`Editor.turnmap`).
#[derive(Default)]
pub struct State {
    of: Option<FoldedFrom>,
    /// Changes each time it's folded anew — what's made from it is kept while this holds.
    pub made: u64,
    /// The turn, in the replay's order.
    pub turn: usize,
    /// Which message asked it and what it said (`replay::Turn.asked`).
    pub asked: Option<(usize, String)>,
    /// By path from the checkout's top.
    pub files: HashMap<PathBuf, File>,
    /// Stops outside the project: counted on the top line, never marked (follow's "what Claude reads elsewhere is never
    /// opened").
    pub outside: usize,
    /// The stop in hand, of this turn's.
    pub hand: Option<usize>,
    /// The front buffer's version last seen (a change of it is yours: `Replay.yours`).
    seen: HashMap<DocId, u64>,
}

/// The file a stop is at: from the checkout's top, a place outside the project whole (`rel_of`).
fn place<'a>(s: &'a Stop, root: &Path) -> Option<&'a Path> {
    Some(rel_of(s.path.as_deref().filter(|_| s.page.is_none())?, root))
}

/// A role's stops at a file as they're folded, each with its lines where it has them.
type Stops = Vec<(usize, Option<Range<usize>>)>;

/// A file's roles as they're being folded.
#[derive(Default)]
struct Work {
    changed: Stops,
    over: Stops,
    searched: Stops,
    read: Stops,
    shown: Vec<(Range<usize>, bool)>,
    shown_read: bool,
}

/// A failure the editor went to, or a failed test run at its target: until that run passes or its file is changed.
struct Failed<'a> {
    run: Option<&'a str>,
    rel: &'a Path,
    lines: Option<Range<usize>>,
    stop: usize,
}

/// A hunk's lines, a pure deletion's the line it went from.
fn at_least_a_line(l: &Range<usize>) -> Range<usize> {
    l.start..l.end.max(l.start + 1)
}

/// The stops of one turn as a map: each file's roles by weight, and how many stops were outside the project — what's
/// marked is the project's ("what Claude reads elsewhere is never opened").
fn fold(stops: &[Stop], root: &Path) -> (HashMap<PathBuf, File>, usize) {
    let (mut files, outside) = fold_all(stops, root);
    files.retain(|rel, f| rel.is_relative() && !f.roles.is_empty());
    (files, outside)
}

/// `fold`, every place kept: those of another folder (`:folder-add`) are judged the same (`Judge`), a file only hits
/// past `follow::HIT_FILES` showed too.
fn fold_all(stops: &[Stop], root: &Path) -> (HashMap<PathBuf, File>, usize) {
    let mut work: HashMap<&Path, Work> = HashMap::new();
    let mut failed: Vec<Failed> = Vec::new();
    let mut outside = 0;
    // (the run a failure the editor went to belongs to: the one just before it)
    let mut ran: Option<&str> = None;
    for (i, s) in stops.iter().enumerate() {
        for x in &s.map.shown {
            let w = work.entry(rel_of(&x.path, root)).or_default();
            w.shown.push((x.lines.clone(), x.old));
            w.shown_read |= s.look.mark() == Mark::Read;
        }
        let rel = place(s, root);
        outside += usize::from(rel.is_some_and(Path::is_absolute));
        match s.look.verb {
            "failed" => {
                failed.extend(rel.map(|rel| Failed { run: ran, rel, lines: s.lines.clone(), stop: i }))
            }
            "run" | "test" => {
                // (the same run again, ended: what it said before is over — one that ran nothing says nothing of it)
                if s.look.ended() {
                    failed.retain(|f| f.run != Some(s.look.what.as_str()));
                }
                ran = Some(s.look.what.as_str());
                if s.look.failed() {
                    failed.extend(rel.map(|rel| Failed { run: ran, rel, lines: None, stop: i }));
                    if let Some(w) = rel.and_then(|r| work.get_mut(r)) {
                        w.over.clear();
                    }
                }
                if let (true, Some(rel), Some(holds)) = (s.look.passed(), rel, &s.map.holds)
                    && let Some(w) = work.get_mut(rel)
                {
                    let over: Stops = held(&w.changed, holds).map(|l| (i, l)).collect();
                    w.over.extend(over);
                }
            }
            // (a call that failed or was refused: a read that read nothing is no read)
            _ if !s.look.marked() || (s.map.errored && s.hunks.is_empty()) => {}
            _ => {
                let Some(rel) = rel else { continue };
                let w = work.entry(rel).or_default();
                if !s.hunks.is_empty() || s.map.whole {
                    w.changed.extend(s.hunks.iter().map(|h| (i, Some(at_least_a_line(&h.lines)))));
                    if s.hunks.is_empty() {
                        w.changed.push((i, None));
                    }
                    // (what ran before this change didn't run over it; what failed there may be what it fixes)
                    w.over.clear();
                    failed.retain(|f| f.rel != rel);
                } else if s.look.mark() == Mark::Read {
                    w.read.push((i, s.lines.clone()));
                } else if s.look.mark() == Mark::Lookup {
                    w.searched.push((i, s.lines.clone()));
                }
            }
        }
    }
    let mut files: HashMap<PathBuf, File> = HashMap::new();
    for (rel, w) in work {
        let f = files.entry(rel.to_path_buf()).or_default();
        add(f, Mark::Edit, &w.changed);
        add(f, Mark::Run, &w.over);
        add(f, Mark::Lookup, &w.searched);
        add(f, Mark::Read, &w.read);
        (f.shown, f.shown_read) = (w.shown, w.shown_read);
    }
    for x in failed {
        add(files.entry(x.rel.to_path_buf()).or_default(), Mark::Failed, &[(x.stop, x.lines)]);
    }
    files.values_mut().for_each(|f| crate::sorting::by_key(&mut f.roles, |r| weight(r.mark)));
    (files, outside)
}

/// What of `changed` a passing run holds: every change, or those meeting its lines.
fn held<'a>(changed: &'a Stops, holds: &'a Holds) -> impl Iterator<Item = Option<Range<usize>>> + 'a {
    changed.iter().filter_map(move |(_, l)| match (holds, l) {
        (Holds::File, l) => Some(l.clone()),
        (Holds::Lines(h), Some(l)) => (l.start < h.end && h.start < l.end).then(|| Some(l.clone())),
        (Holds::Lines(_), None) => None,
    })
}

/// A role's stops into a file: each stop once in its role, its lines where it has them.
fn add(f: &mut File, mark: Mark, stops: &[(usize, Option<Range<usize>>)]) {
    if stops.is_empty() {
        return;
    }
    let at = match f.roles.iter().position(|r| r.mark == mark) {
        Some(at) => at,
        None => {
            f.roles.push(Role { mark, stops: Vec::new() });
            f.roles.len() - 1
        }
    };
    for (stop, lines) in stops {
        if !f.roles[at].stops.contains(stop) {
            f.roles[at].stops.push(*stop);
        }
        f.lines.extend(lines.clone().map(|lines| Touched { lines, mark, stop: *stop }));
        if lines.is_none() && !f.whole.contains(&mark) {
            f.whole.push(mark);
        }
    }
}

/// The front session's stops as they are now: the map folded anew when they changed, the front buffer's edits of yours
/// noted. Every event (`Editor::handle_event`), so nothing here but a comparison while the stops hold.
pub fn sync(ed: &mut Editor) {
    let replay = stops(ed);
    let of = replay.map(|r| (ed.workspace.id, r.version(), r.at));
    if of != ed.turnmap.of {
        let (mut files, mut outside, mut asked, mut turn, mut hand) = (HashMap::new(), 0, None, 0, None);
        if let Some(r) = replay {
            turn = r.at.map_or(r.turns().len().saturating_sub(1), |at| at.0);
            hand = r.at.map(|at| at.1);
            if let Some(t) = r.turns().get(turn) {
                (files, outside) = fold(&t.stops, &ed.workspace.root);
                asked = t.asked.clone();
            }
        }
        let m = &mut ed.turnmap;
        (m.of, m.files, m.outside, m.asked, m.turn, m.hand) = (of, files, outside, asked, turn, hand);
        m.made += 1;
    }
    let (id, version) = (ed.doc().id, ed.doc().version());
    if ed.turnmap.seen.insert(id, version).is_some_and(|v| v != version)
        && let Some(rel) = ed.doc().path.as_deref().and_then(|p| p.strip_prefix(&ed.workspace.root).ok())
    {
        let rel = rel.to_path_buf();
        let session = &mut ed.workspace.claude;
        if let Some(r) = session.chat.as_mut().map(|c| &mut c.follow.replay).or(session.stops_left.as_mut()) {
            r.you_changed(&rel);
        }
    }
}

/// You changed `rel` since the turn on the map began: its lines aren't the stops' (`Replay.yours`).
pub fn yours(ed: &Editor, rel: &Path) -> bool {
    stops(ed).is_some_and(|r| r.yours_in(ed.turnmap.turn, rel))
}

/// The front session's stops: its chat's, or what a closed chat left.
fn stops(ed: &Editor) -> Option<&Replay> {
    let session = &ed.workspace.claude;
    session.chat.as_ref().map(|c| &c.follow.replay).or(session.stops_left.as_ref())
}

/// Claude's edit landed in `path`: its buffer's version is the turn's, not one of yours.
pub fn landed(ed: &mut Editor, path: &Path) {
    if let Some((id, version)) = crate::docpaths::find(ed, path).map(|d| (d.id, d.version())) {
        ed.turnmap.seen.insert(id, version);
    }
}

impl State {
    /// Nothing on the map: every row and frame reads this first.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// What the panel's top line says, as its details: `turn 14 ` and `· 2 outside`, a part of it that folds first
    /// (`term::fold_details`) — nothing with no stop on it.
    pub fn said(&self) -> Vec<String> {
        let n = self.asked.as_ref().map(|a| format!("turn {}", a.0));
        match (n, self.outside) {
            (_, 0) if self.files.is_empty() => Vec::new(),
            (None, 0) => vec!["last turn".into()],
            (None, n) => vec![format!("{n} outside")],
            (Some(t), 0) => vec![t],
            (Some(t), n) => vec![format!("{t} "), format!("· {n} outside")],
        }
    }

    /// The stop a role of this turn gives to a press.
    pub fn at(&self, r: &Role) -> (usize, usize) {
        (self.turn, r.press(self.hand))
    }

    /// Whether the stop in hand is one of `r`'s.
    pub fn lit(&self, r: &Role) -> bool {
        self.hand.is_some_and(|h| r.stops.contains(&h))
    }

    /// A folded folder's mark: the strongest role under it, its stops, and how many files say it — out of sight, a count
    /// on the tree.
    pub fn folded(&self, dir: &Path) -> Option<(Role, usize)> {
        let under = || self.files.iter().filter(|(rel, _)| rel.starts_with(dir));
        let strongest =
            under().filter_map(|(_, f)| f.roles.first()).map(|r| r.mark).min_by_key(|m| weight(*m))?;
        let roles = under().flat_map(|(_, f)| &f.roles).filter(|r| r.mark == strongest);
        let mut stops: Vec<usize> = roles.flat_map(|r| r.stops.iter().copied()).collect();
        crate::sorting::by_key(&mut stops, |s| *s);
        stops.dedup();
        Some((Role { mark: strongest, stops }, under().count()))
    }

    /// The roles in words, for the row in hand's foot (`changed · tested · read 3×`).
    pub fn words(roles: &[Role]) -> Vec<String> {
        roles.iter().map(Role::word).collect()
    }

    /// The outline's roles: each definition's in `defs`, by what touched its own lines — a line is its innermost
    /// definition's (a read of a method marks the method, not its `impl`).
    pub fn outline(&self, rel: &Path, defs: &[crate::syntax::Def], text: &ropey::Rope) -> Vec<Vec<Role>> {
        let Some(f) = self.files.get(rel) else { return Vec::new() };
        let last = crate::movement::last_line(text);
        let line = |b: usize| text.byte_to_line(b.min(text.len_bytes()));
        let spans: Vec<Range<usize>> =
            defs.iter().map(|d| line(d.range.start)..line(d.range.end) + 1).collect();
        let mut roles: Vec<File> = vec![File::default(); defs.len()];
        for t in &f.lines {
            for l in t.lines.start.min(last)..t.lines.end.min(last + 1) {
                let after = spans.partition_point(|s| s.start <= l);
                if let Some(i) = spans[..after].iter().rposition(|s| s.contains(&l)) {
                    add(&mut roles[i], t.mark, &[(t.stop, None)]);
                }
            }
        }
        for r in &mut roles {
            crate::sorting::by_key(&mut r.roles, |r| weight(r.mark));
        }
        roles.into_iter().map(|r| r.roles).collect()
    }
}

/// A row's hint with `roles` before `rest`: `● ▸ ◦ 3  M` — the marks in their scopes, the one in hand in the accent, each
/// a button (`Paint.presses`). `count`: a folded folder's files on the map, over the count of its own that `rest` is
/// (`● 3/12`). `short`: only the strongest (a narrow panel folds to it first).
pub fn hint(
    map: &State,
    roles: &[Role],
    count: Option<usize>,
    short: bool,
    rest: (String, Option<crate::picker::Paint>),
) -> (String, Option<crate::picker::Paint>) {
    let (rest, was) = rest;
    let mut paint = crate::picker::Paint::default();
    let mut text = String::new();
    for r in roles.iter().take(if short { 1 } else { roles.len() }) {
        if !text.is_empty() {
            text.push(' ');
        }
        let said = r.said();
        let at = text.len()..text.len() + r.mark.glyph().len();
        let scope = match r.mark {
            _ if map.lit(r) => "ui.accent",
            Mark::Edit => "diff.plus",
            Mark::Failed => "warning",
            _ => "ui.virtual",
        };
        paint.hint.push((at.clone(), scope));
        if map.lit(r) {
            paint.bold.push(at.clone());
        }
        paint.presses.push((text.len()..text.len() + said.len(), map.at(r)));
        text.push_str(&said);
    }
    if let Some(n) = count {
        text.push_str(&format!(" {n}/{rest}"));
        return (text, Some(paint));
    }
    if rest.is_empty() {
        return (text, Some(paint));
    }
    let from = text.len() + 2;
    text = format!("{text}  {rest}");
    if let Some(was) = was {
        paint.hint.extend(was.hint.into_iter().map(|(r, s)| (r.start + from..r.end + from, s)));
        paint.bold.extend(was.bold.into_iter().map(|r| r.start + from..r.end + from));
    }
    (text, Some(paint))
}

/// What's left of a mark's button (`press`, `glyph` its first part: ranges of the `whole` hint) in the hint as drawn, its
/// `fit`: the whole where the two agree, its glyph where a count was cut after it, none where the glyph itself is cut —
/// the byte offsets may land inside the `…`, or on one standing where the glyph was.
pub fn press_kept(
    whole: &str,
    fit: &str,
    glyph: &Range<usize>,
    press: &Range<usize>,
) -> Option<Range<usize>> {
    let agree = |r: &Range<usize>| fit.get(r.clone()).is_some() && fit.get(r.clone()) == whole.get(r.clone());
    match (agree(glyph), agree(press)) {
        (true, true) => Some(press.clone()),
        (true, false) => Some(glyph.clone()),
        (false, _) => None,
    }
}

/// The signs in buffer `doc`, none for a file you changed since (`Replay.yours`).
pub fn signs(ed: &Editor, doc: &Document) -> Vec<Sign> {
    let m = &ed.turnmap;
    if m.is_empty() {
        return Vec::new();
    }
    let rel = doc.path.as_deref().and_then(|p| p.strip_prefix(&ed.workspace.root).ok());
    let Some(f) = rel.filter(|r| !yours(ed, r)).and_then(|r| m.files.get(r)) else {
        return Vec::new();
    };
    let sign = |t: &Touched| Sign {
        lines: t.lines.clone(),
        mark: t.mark,
        at: (m.turn, t.stop),
        in_hand: m.hand == Some(t.stop),
    };
    f.lines.iter().map(sign).collect()
}

/// The sign on line `l`: the one in hand first, then the heaviest, then the newest.
pub fn sign_at(signs: &[Sign], l: usize) -> Option<&Sign> {
    let on = signs.iter().filter(|s| s.lines.contains(&l));
    on.min_by_key(|s| (!s.in_hand, weight(s.mark), usize::MAX - s.at.1))
}

#[cfg(test)]
mod tests;
