//! The session list, a floating pane (panes P3 step 7, `.sb/panes-P3-plan.md` §3.6; CLAUDE.md "The manager's
//! view"): `space W` opens it in a named frame (`╭─ sessions ─…─╮`) over the code and the chat — not a modal
//! picker: it stays while you work (`esc code` gives the keys back, it stays in sight; `space W`, a click or `C-w w`
//! gives it them again; `q` closes it), and across switching sessions — it's the viewer's, not a workspace's. Done
//! once used (`floatlist.rs`, user 2026-10-23): what takes you to a session from it — `enter`, a digit, a double
//! click, `o`, `f`, `v` — closes it.
//!
//! It's the manager's overview: one row a session, a child under its parent on an arm (`├─── `, the one list L1 —
//! `.sb/one-list-plan.md`), its name in its colour, then what a person watching several sessions work needs — `▲` in a
//! column of its own while it waits on you, its state with the step its turn is at, in the now line's words without
//! its time (`▸ asks to run cargo test` · `● editing src/a.rs 12-15` · `◦ stuck? symbols 42s` · `done` · `queued
//! 2nd`), `≈` whom it overlaps, `↓N` behind its base, its context, its model where the rows' differ, its last report
//! line (its Claude's last sentence), then the rest the list always said (its branch, what's changed, its reviewer,
//! who pays, its cache and cost, its build folder, its processes). Each a column as wide as its widest, row under row;
//! narrow, they give way in `LADDER`'s order (`lay`): the rest first, then the report (cut, then gone), the model, the
//! step cut to its floor, the context, the cost, behind, the overlaps, `▲`, the state last, the name never. Built from
//! the sessions every frame it's drawn (`rows`), nothing slow: git's part is read on a worker as it opens
//! (`Editor::session_list`).
//!
//! Its keys are a floating list's (`floatlist.rs`: the moves, its own search, `esc code`, `q`), `enter` switches to
//! the row's session, and its letters as the picker's were (`Editor::session_list_key`: `o` new, `d` end, `b` bring,
//! `v` woven, `f` follow, digits, …).

use crate::editor::Editor;
use crate::event::WorkspaceId;
use crate::pane::GroupKind;
use unicode_width::UnicodeWidthStr as _;

/// How a part of a row is inked (`term::draw_sessions_pane` maps it to the theme): the row's text, faint, what needs
/// you (warning), done (the accent), a session's own colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Text,
    Faint,
    Warn,
    Accent,
    Participant(u8),
}

/// A row's columns, in their order on the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Col {
    /// What waits on you: `▲`, in warning.
    Waits,
    State,
    Overlap,
    Behind,
    Context,
    /// Its model and effort (`opus · high`), only where the rows' differ (`models_where_they_differ`).
    Model,
    Report,
    Branch,
    Ahead,
    Changed,
    Reloaded,
    Compacted,
    Reviewer,
    Billing,
    Cache,
    Cost,
    Build,
    Usage,
}

/// Every column, in the row's order.
pub const COLS: [Col; 18] = [
    Col::Waits,
    Col::State,
    Col::Overlap,
    Col::Behind,
    Col::Context,
    Col::Model,
    Col::Report,
    Col::Branch,
    Col::Ahead,
    Col::Changed,
    Col::Reloaded,
    Col::Compacted,
    Col::Reviewer,
    Col::Billing,
    Col::Cache,
    Col::Cost,
    Col::Build,
    Col::Usage,
];

/// One step of what gives way when a row is too wide: a column gone, or cut to the room the others leave while
/// that's at least its floor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Give {
    Gone(Col),
    Cut(Col, usize),
}

/// What gives way when the pane is narrow, the first first: the rest from the last, then the report (cut to
/// `REPORT_MIN`, then gone), the model, the step cut to its floor before the context or the cost go (the one list,
/// 2026-11-15: "the step gives way to its floor before ctx or the cost go"), the context, the cost, behind, the
/// overlaps, `▲`, the state last — the name never.
pub const LADDER: [Give; 20] = {
    use Col::*;
    use Give::{Cut, Gone};
    [
        Gone(Usage),
        Gone(Build),
        Gone(Cache),
        Gone(Billing),
        Gone(Reviewer),
        Gone(Compacted),
        Gone(Reloaded),
        Gone(Changed),
        Gone(Ahead),
        Gone(Branch),
        Cut(Report, REPORT_MIN),
        Gone(Report),
        Gone(Model),
        Cut(State, STATE_MIN),
        Gone(Context),
        Gone(Cost),
        Gone(Behind),
        Gone(Overlap),
        Gone(Waits),
        Gone(State),
    ]
};

/// The report line is cut to this at most, and goes before it would be cut shorter than `REPORT_MIN`.
pub const REPORT_MAX: usize = 56;
pub const REPORT_MIN: usize = 16;

/// The state with its step is cut to this at most, and to no less than `STATE_MIN` before the context goes.
pub const STATE_MAX: usize = 32;
pub const STATE_MIN: usize = 12;

/// Two cells between columns, as between key hints.
pub const GAP: usize = 2;

/// One part of a cell: its text and ink.
pub type Part = (String, Ink);

/// A session's row: who it is and its cells, by column (empty ones left out).
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: WorkspaceId,
    pub name: String,
    pub colour: u8,
    /// The session in front — its name bold, as the strip's.
    pub front: bool,
    /// Before its name, faint: a child's arm (`arms`) — empty for a session nobody started.
    pub arm: String,
    /// Its Claude works (`Doing::Working`): the top line's count.
    pub working: bool,
    pub cells: Vec<(Col, Vec<Part>)>,
}

impl Row {
    /// Column `c`'s parts (none: empty).
    pub fn cell(&self, c: Col) -> &[Part] {
        self.cells.iter().find(|x| x.0 == c).map_or(&[], |x| &x.1)
    }

    /// Column `c`'s text.
    pub fn text(&self, c: Col) -> String {
        self.cell(c).iter().map(|p| p.0.as_str()).collect()
    }

    /// Every cell's text, two cells apart (the tests' and `session_items`' one line).
    #[cfg(test)]
    pub fn line(&self) -> String {
        COLS.iter().map(|&c| self.text(c)).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("  ")
    }
}

/// Session `l`'s row as `shown` says it.
pub fn row(l: &crate::worktree::Shown) -> Row {
    use crate::workspace::Doing;
    let mut cells: Vec<(Col, Vec<Part>)> = Vec::new();
    let mut put = |c: Col, parts: Vec<Part>| {
        if parts.iter().any(|p| !p.0.is_empty()) {
            cells.push((c, parts));
        }
    };
    let one = |s: String, ink: Ink| vec![(s, ink)];
    // What waits on you, in a column of its own: who waits, not what (what waits is the card stack's and its peeks',
    // polish landing 1)
    if l.doing == Doing::Waiting {
        put(Col::Waits, one("▲".into(), Ink::Warn));
    }
    // The state with the step its turn is at, as the now line says it without its time (the one list L1): what needs
    // you in warning, done in the accent, the calm states faint — a rebase under way and its place in the bring queue
    // before it
    let step = |or: String| if l.step.is_empty() { or } else { l.step.clone() };
    let (said, ink) = match l.doing {
        Doing::Waiting => (step("waits".into()), Ink::Warn),
        Doing::Done => ("done".into(), Ink::Accent),
        d @ Doing::Stuck { .. } => (format!("◦ stuck? {}", d.said()), Ink::Warn),
        d @ Doing::Queued { .. } => (step(d.said()), Ink::Faint),
        Doing::Working => (step("working".into()), Ink::Faint),
        Doing::Idle => (String::new(), Ink::Faint),
    };
    let mut state: Vec<Part> = Vec::new();
    for s in [&l.rebasing, &l.queue].into_iter().flatten() {
        state.push((
            format!("{s}{}", if said.is_empty() && state.is_empty() { "" } else { " · " }),
            Ink::Text,
        ));
    }
    state.push((said, ink));
    // (a child's turn that wakes its parent as it ends: in the column that folds last — never unseen)
    if let Some(parent) = &l.wakes {
        let sep = if state.iter().all(|p| p.0.is_empty()) { "" } else { " · " };
        state.push((format!("{sep}wakes {parent}"), Ink::Faint));
    }
    put(Col::State, state);
    // Whom its changes would conflict with: `≈` in warning, each name in its colour
    if !l.overlap.is_empty() {
        let mut parts = vec![("≈ ".to_string(), Ink::Warn)];
        for (i, (name, c)) in l.overlap.iter().enumerate() {
            if i > 0 {
                parts.push((", ".into(), Ink::Text));
            }
            parts.push((name.clone(), c.map_or(Ink::Text, Ink::Participant)));
        }
        put(Col::Overlap, parts);
    }
    let f = l.facts.clone().unwrap_or_default();
    put(Col::Behind, one(if f.behind > 0 { format!("↓{}", f.behind) } else { String::new() }, Ink::Text));
    if let Some(pct) = l.context {
        let ink = if pct >= 80 { Ink::Warn } else { Ink::Text };
        put(Col::Context, vec![("ctx ".into(), Ink::Faint), (format!("{pct}%"), ink)]);
    }
    put(Col::Model, one(l.model.clone(), Ink::Faint));
    put(Col::Report, one(l.report.clone(), Ink::Faint));
    // The rest the list always said
    put(Col::Branch, one(l.branch.clone().filter(|b| *b != l.name).unwrap_or_default(), Ink::Faint));
    put(Col::Ahead, one(if f.ahead > 0 { format!("↑{}", f.ahead) } else { String::new() }, Ink::Text));
    put(
        Col::Changed,
        one(if f.changed > 0 { format!("{} changed", f.changed) } else { String::new() }, Ink::Text),
    );
    put(
        Col::Reloaded,
        one(if l.reloaded > 0 { format!("• {} reloaded", l.reloaded) } else { String::new() }, Ink::Text),
    );
    put(Col::Compacted, one(if l.compacted { "≈ compacted".into() } else { String::new() }, Ink::Text));
    put(Col::Reviewer, one(l.reviewer.clone(), Ink::Text));
    // (who pays: `api` — the plan, the calm default, says nothing — D5)
    let api = if l.billing == crate::billing::Billing::Api { "api" } else { "" };
    put(Col::Billing, one(api.into(), Ink::Text));
    let (cache, cost) = l.claude.clone().unwrap_or_default();
    put(Col::Cache, one(cache, Ink::Faint));
    // (a parent's: its children's in all after its own — orchestra §6, `main · 3 children $4.12`)
    let mut costs = one(cost, Ink::Text);
    if let Some((n, total)) = &l.children {
        let sep = if costs[0].0.is_empty() { "" } else { " · " };
        let said = if *n == 1 { "1 child".to_string() } else { format!("{n} children") };
        let total = if total.is_empty() { String::new() } else { format!(" {total}") };
        costs.push((format!("{sep}{said}{total}"), Ink::Faint));
        costs.retain(|p| !p.0.is_empty());
    }
    put(Col::Cost, costs);
    put(
        Col::Build,
        one(
            l.build.map(|s| format!("build {}", crate::worktree::size_text(s))).unwrap_or_default(),
            Ink::Faint,
        ),
    );
    let usage = l.usage.map(|(cpu, bytes)| format!("{cpu:>3.0}%  {:>6}", crate::procs::mem(bytes)));
    put(Col::Usage, one(usage.unwrap_or_default(), Ink::Faint));
    Row {
        id: l.id,
        name: l.name.clone(),
        colour: l.colour,
        front: l.front,
        arm: String::new(),
        working: l.doing == Doing::Working,
        cells,
    }
}

/// Before a child's name in the chat's top line, faint, when its parent went into `+N` (elsewhere there a child joins
/// its parent or a sibling by `›`, `term::MEMBERS_SEP`). The list draws a child's arm instead (`arms`).
pub const CHILD: &str = "› ";

/// A child's arm's cells in the list: `├───` and a blank.
pub const ARM: usize = 5;

impl Row {
    /// Its name's cells, its arm's with it.
    pub fn name_width(&self) -> usize {
        self.arm.width() + self.name.width()
    }
}

/// Each row's arm (2026-11-04's, the one list L1), the rows in their tree's order (a parent before its own) and
/// `parents[i]` the row row `i` hangs from: for each ancestor below a root, `│` where that ancestor has a sibling after
/// it, else a blank, as wide as an arm; then its own — `├`, or `└` for the last of its siblings, its line and a blank
/// (`├─── `). A root's is empty.
pub fn arms(parents: &[Option<usize>]) -> Vec<String> {
    let last = |k: usize| !parents[k + 1..].iter().any(|&p| p == parents[k]);
    (0..parents.len())
        .map(|i| {
            if parents[i].is_none() {
                return String::new();
            }
            // (its ancestors below a root, the nearest first — each before it in the list, so the walk ends)
            let mut chain = Vec::new();
            let mut at = parents[i];
            while let Some(a) = at.filter(|&a| a < i && parents[a].is_some() && chain.len() < parents.len()) {
                chain.push(a);
                at = parents[a];
            }
            let mut s = String::new();
            for &a in chain.iter().rev() {
                s.push(if last(a) { ' ' } else { '│' });
                s.push_str(&" ".repeat(ARM - 1));
            }
            s.push(if last(i) { '└' } else { '├' });
            s.push_str(&"─".repeat(ARM - 2));
            s.push(' ');
            s
        })
        .collect()
}

/// A model every row says alike says nothing: the model's column stays only where the rows' differ (a session switched
/// with `C-t`), else it's taken off every row.
pub fn models_where_they_differ(rows: &mut [Row]) {
    let mut models: Vec<String> = rows.iter().map(|r| r.text(Col::Model)).filter(|m| !m.is_empty()).collect();
    crate::sorting::sort(&mut models);
    models.dedup();
    if models.len() < 2 {
        for r in rows {
            r.cells.retain(|c| c.0 != Col::Model);
        }
    }
}

/// The columns that widen the floating list's frame (`need`): what a person watching several sessions work reads —
/// the rest only in room left over.
const WIDENS: [Col; 8] =
    [Col::Waits, Col::State, Col::Overlap, Col::Behind, Col::Context, Col::Model, Col::Report, Col::Cost];

/// What a floating frame takes beside its rows' text: its two sides, the bar's cell and a blank, a cell before the side.
pub const FRAMED: usize = 5;

/// The cells the floating list's frame needs for `rows` (the one list L2, `.sb/one-list-plan.md` §1): the name's
/// column, then each of `WIDENS` as wide as its widest (the state to `STATE_MAX`, the report to `REPORT_MAX`) with its
/// `GAP`, then the frame's own (`FRAMED`) — taken over every row, in sight or not, so scrolling never widens it.
pub fn need(rows: &[Row]) -> usize {
    let width = |c: Col| rows.iter().map(|r| r.text(c).width()).max().unwrap_or(0);
    let cap = |c: Col, w: usize| match c {
        Col::Report => w.min(REPORT_MAX),
        Col::State => w.min(STATE_MAX),
        _ => w,
    };
    let name = rows.iter().map(Row::name_width).max().unwrap_or(0);
    let cols: usize = WIDENS.iter().map(|&c| cap(c, width(c))).filter(|&w| w > 0).map(|w| GAP + w).sum();
    name + cols + FRAMED
}

/// The columns shown in a pane whose rows are `room` cells wide (the name's column first, then each shown column
/// `GAP` after the one before), and each one's width: every column as wide as its widest (the report to `REPORT_MAX`,
/// the state to `STATE_MAX`); too wide, they give way in `LADDER`'s order, the first step that fits kept — a cut
/// column takes the room the others leave, while that's at least its floor.
pub fn lay(rows: &[Row], room: usize) -> (usize, Vec<(Col, usize)>) {
    let width = |c: Col| rows.iter().map(|r| r.text(c).width()).max().unwrap_or(0);
    let name_w = rows.iter().map(Row::name_width).max().unwrap_or(0).min(room);
    let cap = |c: Col, w: usize| match c {
        Col::Report => w.min(REPORT_MAX),
        Col::State => w.min(STATE_MAX),
        _ => w,
    };
    let all: Vec<(Col, usize)> = COLS.iter().map(|&c| (c, cap(c, width(c)))).filter(|c| c.1 > 0).collect();
    for k in 0..=LADDER.len() {
        let steps = &LADDER[..k];
        let gone = |c: Col| steps.contains(&Give::Gone(c));
        let shown: Vec<(Col, usize)> = all.iter().filter(|c| !gone(c.0)).copied().collect();
        let total = name_w + shown.iter().map(|c| GAP + c.1).sum::<usize>();
        if total <= room {
            return (name_w, shown);
        }
        // (a column cut and not gone since: the room the others leave, while that's its floor — one at a time, as the
        // report goes before the state is cut)
        let cut = steps.iter().find_map(|g| match *g {
            Give::Cut(c, floor) if !gone(c) => shown.iter().find(|x| x.0 == c).map(|x| (c, x.1, floor)),
            _ => None,
        });
        if let Some((c, w, floor)) = cut
            && total - w + floor.min(w) <= room
        {
            let left = room - (total - w);
            return (name_w, shown.iter().map(|&(x, v)| (x, if x == c { left.min(v) } else { v })).collect());
        }
    }
    (name_w, Vec::new())
}

/// Its Claude's last report line: the last sentence of its last answer, the Markdown's marks taken out — empty before
/// one.
pub fn report(chat: Option<&crate::chat::Chat>) -> String {
    let Some(text) = chat.and_then(|c| c.msgs.iter().rev().find(|m| m.role == crate::chat::Role::Assistant))
    else {
        return String::new();
    };
    let line = text.text.lines().rev().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("```"));
    let line = line.unwrap_or_default().trim_start_matches(['#', '-', '*', '>', ' ']);
    let plain: String = line.replace("**", "").replace('`', "");
    // (its last sentence: past the last `. ` that ends one)
    let last = plain.trim_end_matches(['.', ' ']).rfind(". ").map_or(plain.as_str(), |i| &plain[i + 2..]);
    last.trim().to_string()
}

impl Editor {
    /// `space W` (§3.6): the session list floats in sight with the keys, the session in front selected — up already,
    /// the keys to it. Git's part of each row is read again on a worker (the last read shown meanwhile); the
    /// processes looked at once.
    pub fn session_list(&mut self) {
        self.float_list_open(GroupKind::Sessions, self.workspace.id.n, None);
        self.session_facts();
    }

    /// The session list's rows as things are now: each session's, a child's arm before its name, the model's column
    /// where the rows' differ.
    pub fn sessions_rows(&self) -> Vec<Row> {
        let shown = self.session_shown();
        let parents: Vec<Option<usize>> =
            shown.iter().map(|l| l.parent.and_then(|p| shown.iter().position(|s| s.id == p))).collect();
        let mut rows: Vec<Row> = shown.iter().map(row).collect();
        for (r, arm) in rows.iter_mut().zip(arms(&parents)) {
            r.arm = arm;
        }
        models_where_they_differ(&mut rows);
        rows
    }

    /// The session list has the keys.
    #[cfg(test)]
    pub fn sessions_list_keys(&self) -> bool {
        self.float_list_keys(GroupKind::Sessions)
    }

    /// The session the list's selection is on (by its id, else the row it was on).
    pub fn sessions_here(&self) -> Option<WorkspaceId> {
        self.float_list_here(GroupKind::Sessions).map(WorkspaceId::here)
    }

    /// `enter` on the list (a digit, a double click): to the selected session — the list done once used, so it
    /// closes (user, 2026-10-23: floating in the middle, it would cover the code); on the session in front too.
    pub(crate) fn sessions_enter(&mut self) {
        // (a pane's key has its document lent: the session to the front, the list used, once it's back)
        if crate::docpane::later(self, |ed| ed.sessions_enter()) {
            return;
        }
        let Some(id) = self.sessions_here() else { return };
        self.float_list_used(GroupKind::Sessions);
        if id != self.workspace.id {
            self.workspace_switch(id);
        } else if self.docked_in_sight(GroupKind::Sessions) {
            // (docked it stays: nothing to switch to, said — no silent key)
            self.set_status(format!("{} is in front — q closes the list", self.workspace.label()));
        }
    }
}

/// Row `row`'s text `re` matches: its name, else a column's (its parts as one).
pub(crate) fn matches(row: &Row, re: &regex::Regex) -> bool {
    re.is_match(&row.name) || COLS.iter().any(|&c| re.is_match(&row.text(c)))
}

/// The list's keys for its frame's bottom line (docked, its foot), as they apply to the row in hand: `enter` switches, `/` searches, then what the row's session
/// takes — `v` its changes woven where it overlaps, `f` follow while its Claude works, `B` the marked ones in turn —,
/// `o` new, `O` fork, `d` end and `b` bring (not the checkout tarae started in), `m` mark, `x` its build folder once measured,
/// `$` who pays; `esc code`, `q close`. The line folds them from the last.
pub fn hints(ed: &Editor) -> Vec<(&'static str, String)> {
    use crate::workspace::Doing;
    let here = ed.sessions_here();
    let order = ed.sessions_in_order();
    let l = here.and_then(|id| order.iter().find(|l| l.id == id));
    let working = l.is_some_and(|l| matches!(ed.doing(l), Doing::Working));
    let woven = here.is_some_and(|id| !ed.workspaces.is_empty() && crate::overlap::file_of(ed, id).is_some());
    let marked = ed.bring_queue.marks.len();
    let started = here == Some(WorkspaceId::HOME);
    let build =
        here.is_some_and(|id| id != ed.workspace.id && ed.workspace_store.build_dirs.contains_key(&id));
    // (`/` second: how the list is searched stays in sight as the line folds; `O fork` last, the rarest — it folds first)
    let mut v: Vec<(&'static str, String)> = vec![("enter", "switch".into()), ("/", "search".into())];
    v.extend(woven.then(|| ("v", "woven".into())));
    v.extend(working.then(|| ("f", "follow".into())));
    v.extend((marked > 0).then(|| ("B", format!("bring the {marked} in turn"))));
    v.push(("o", "new".into()));
    if !started {
        v.extend([("d", "end".into()), ("b", "bring into its base".into()), ("m", "mark to bring".into())]);
    }
    v.extend(build.then(|| ("x", "drop its build".into())));
    v.extend([("$", "billing".into()), ("esc", "code".into()), ("q", "close".into())]);
    v.push(("O", "fork".into()));
    v
}

#[cfg(test)]
mod tests;
