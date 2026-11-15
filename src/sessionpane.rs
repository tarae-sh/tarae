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
//! (`Editor::session_list`). The row in hand's card (`card`, the one list L3) says its row whole — floating, just outside
//! the frame, below, while its row shows less or the wheel has taken it out of sight; docked, at its foot above its keys.
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
    Billing,
    Cache,
    Cost,
    Build,
    Usage,
}

/// Every column, in the row's order.
pub const COLS: [Col; 17] = [
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
pub const LADDER: [Give; 19] = {
    use Col::*;
    use Give::{Cut, Gone};
    [
        Gone(Usage),
        Gone(Build),
        Gone(Cache),
        Gone(Billing),
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

/// A session's row, or one of its helpers' (`helper`): who it is and its cells, by column (empty ones left out).
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Its session (a helper's: the session it works for).
    pub id: WorkspaceId,
    /// A helper's row (the one list L4): what helper — None, the session's own.
    pub helper: Option<Kind>,
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
    /// Its id in the list (`FloatList.selected`, `RowId::pack`).
    pub fn key(&self) -> u64 {
        match self.helper {
            None => self.id.n,
            Some(kind) => RowId::Helper { of: self.id, kind }.pack(),
        }
    }

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
        Doing::Waiting => (step("waits".into()), word("waits")),
        Doing::Done => ("done".into(), word("done")),
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
        helper: None,
        name: l.name.clone(),
        colour: l.colour,
        front: l.front,
        arm: String::new(),
        working: l.doing == Doing::Working,
        cells,
    }
}

/// A state word's ink, one rule for a session's row and a helper's (the design session, 2026-11-16): `done` the accent —
/// news, its work ended; `waits` and `failed` the warning — it needs you (a turn's error is a warning toast,
/// `workspace::turn_end_words`); `stopped` faint — your own key's, or its turn's end, never news (a turn you stopped is
/// an info toast there).
pub const WORDS: [(&str, Ink); 4] =
    [("done", Ink::Accent), ("waits", Ink::Warn), ("failed", Ink::Warn), ("stopped", Ink::Faint)];

/// The ink `WORDS` gives what a state cell says by its first word (`done · 2 findings`: `done`'s) — None: no word of
/// theirs.
pub fn word_ink(said: &str) -> Option<Ink> {
    let first = said.split([' ', '·']).next()?;
    WORDS.iter().find(|w| w.0 == first).map(|w| w.1)
}

/// `WORDS`' ink for one of its words.
fn word(w: &str) -> Ink {
    word_ink(w).unwrap_or(Ink::Text)
}

/// Before a child's name in the chat's top line, faint, when its parent went into `+N` (elsewhere there a child joins
/// its parent or a sibling by `›`, `term::MEMBERS_SEP`). The list draws a child's arm instead (`arms`).
pub const CHILD: &str = "› ";

/// A child's arm's cells in the list: `├───` and a blank.
pub const ARM: usize = 5;

/// A helper's arm's cells (the one list L4): `├─` and a blank — half a child's, so a helper reads as no session.
pub const HELPER_ARM: usize = 3;

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
#[cfg(test)]
pub fn arms(parents: &[Option<usize>]) -> Vec<String> {
    arms_of(parents, &vec![false; parents.len()])
}

/// `arms`, the rows `helper[i]` says are helpers' on a helper's arm (`├─ `, the one list L4) — right after their
/// session, before its children, so a child's `└` still knows it's the last.
pub fn arms_of(parents: &[Option<usize>], helper: &[bool]) -> Vec<String> {
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
            let arm = if helper.get(i).copied().unwrap_or(false) { HELPER_ARM } else { ARM };
            s.push_str(&"─".repeat(arm - 2));
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

/// What the row in hand's card says after its name (the one list L3, `.sb/one-list-plan.md`): the manager's columns,
/// whole — `▲` (the state says it waits) and the rest the list always said left out.
pub const CARD: [Col; 7] =
    [Col::State, Col::Overlap, Col::Behind, Col::Context, Col::Model, Col::Report, Col::Cost];

/// The most rows the row in hand's card takes docked (`term::draw_sessions_pane`): past them its last is cut.
pub const CARD_ROWS: usize = 3;

/// Between the card's cells.
pub const CARD_SEP: &str = " · ";

/// The row in hand's card (the one list L3): its name in its colour, then each of `CARD`'s cells it has, whole,
/// `CARD_SEP` apart — the step whole, whom it overlaps, behind, its context, model, report and cost, what a narrow list
/// gave up on its row said here.
pub fn card(row: &Row) -> Vec<Part> {
    let mut parts = vec![(row.name.clone(), Ink::Participant(row.colour))];
    for c in CARD {
        let cell = row.cell(c);
        if cell.is_empty() {
            continue;
        }
        parts.push((CARD_SEP.into(), Ink::Faint));
        parts.extend(cell.iter().cloned());
    }
    parts
}

/// The cells shown column `cols[i]` takes on `row`, and how many shown columns after it it took: its own width — the
/// state's runs on into the columns right after it this row leaves empty, `GAP` and their width each (the one list,
/// 2026-11-16: `▸ asks to r…` cut to nine cells at 52 docked beside an empty overlap).
pub fn room(row: &Row, cols: &[(Col, usize)], i: usize) -> (usize, usize) {
    let (c, mut w) = cols[i];
    let mut took = 0;
    while c == Col::State
        && let Some(&(next, nw)) = cols.get(i + 1 + took)
        && row.cell(next).is_empty()
    {
        w += GAP + nw;
        took += 1;
    }
    (w, took)
}

/// Whether `row`, laid out as `lay` lays it (the name's column `name_w`, the columns shown `cols`), shows less than its
/// card says: its name cut, or one of `CARD`'s cells it has cut (in its `room`) or gone.
pub fn cut_on_row(row: &Row, name_w: usize, cols: &[(Col, usize)]) -> bool {
    row.name_width() > name_w
        || CARD.iter().any(|&c| {
            let w = row.text(c).width();
            w > 0 && cols.iter().position(|x| x.0 == c).is_none_or(|i| w > room(row, cols, i).0)
        })
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

// ── Helper rows (the one list L4) ────────────────────────────────────────────────────

/// A session's helper, by what it is — a row of its own under its session (the one list L4, `.sb/one-list-plan.md`):
/// the code reviewer or its board, `space i`, its Claude's web searches (many in one row), the commit draft. The card
/// reviewer has none while its shadow week hides its verdict (`autocards.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Review,
    Ask,
    Search,
    Draft,
}

impl Kind {
    const ALL: [Kind; 4] = [Kind::Review, Kind::Ask, Kind::Search, Kind::Draft];

    /// Where `enter` on its row goes, as the bottom line says it (`Editor::helper_enter`).
    pub fn enter(self) -> &'static str {
        match self {
            Kind::Review => "its notes and verdict",
            Kind::Ask | Kind::Draft => "its edit review",
            Kind::Search => "its results",
        }
    }
}

/// A row of the list by its id there (`FloatList.selected`, a `u64`): a session's — its workspace's number — or a
/// helper's: the top bit set, its kind in the byte below it, its session's number under that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowId {
    Session(WorkspaceId),
    Helper { of: WorkspaceId, kind: Kind },
}

const HELPER_BIT: u64 = 1 << 63;
const KIND_SHIFT: u32 = 56;

impl RowId {
    pub fn pack(self) -> u64 {
        match self {
            RowId::Session(id) => id.n,
            RowId::Helper { of, kind } => HELPER_BIT | (kind as u64) << KIND_SHIFT | of.n,
        }
    }

    pub fn of(n: u64) -> RowId {
        if n & HELPER_BIT == 0 {
            return RowId::Session(WorkspaceId::here(n));
        }
        let kind = Kind::ALL.get(((n >> KIND_SHIFT) & 0x7f) as usize).copied().unwrap_or(Kind::Review);
        RowId::Helper { of: WorkspaceId::here(n & ((1 << KIND_SHIFT) - 1)), kind }
    }

    /// The session it is, or works for.
    pub fn session(self) -> WorkspaceId {
        match self {
            RowId::Session(id) | RowId::Helper { of: id, .. } => id,
        }
    }
}

/// A helper that ended (`Session.helpers_ended`, one a kind): its row stands as it ended until its session's next turn
/// begins (`Chat.started`) — the reviewer's is its own (`Workspace.reviewer` stays).
#[derive(Clone, Debug, PartialEq)]
pub struct Ended {
    pub kind: Kind,
    pub at: std::time::Instant,
    /// How it ended, in its ink: `done` · `4 pages` · `no changes` · `failed` · `stopped`.
    pub said: Part,
    /// What it was asked (`Helper.what`).
    pub what: String,
    /// How many of its kind ended since that turn began: a session's searches, `search ×3`.
    pub n: usize,
}

/// A helper of the session in front ended (a session behind's events are applied with it in front): its row stands so.
pub fn ended(ed: &mut Editor, kind: Kind, said: Part, what: String) {
    let s = &mut ed.workspace.claude;
    let began = s.chat.as_ref().map(|c| c.started);
    let mut n = 1;
    if let Some(i) = s.helpers_ended.iter().position(|e| e.kind == kind) {
        let was = s.helpers_ended.remove(i);
        if kind == Kind::Search && began.is_none_or(|b| was.at >= b) {
            n += was.n;
        }
    }
    s.helpers_ended.push(Ended { kind, at: crate::clock::now(), said, what, n });
}

/// One helper's row as `helpers` finds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Helper {
    pub kind: Kind,
    /// `reviewer` · `board` · `ask` · `search ×3` · `draft`.
    pub name: String,
    /// Its step while it runs (`● writing`), how it ended once over (`done · 2 findings`) — in its ink. No time in it:
    /// a row changes only as its helper does.
    pub said: Part,
    /// What it was asked: `"rename the field"`, a search's words, what a review is of.
    pub what: String,
    pub model: String,
    /// What it cost, where its result says it (the reviewer's) and money is on the screen (`llm.cost`).
    pub cost: String,
    pub running: bool,
}

/// `text` on one line, in quotes.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// `name`, `×n` after it where there are several.
fn times(name: &str, n: usize) -> String {
    if n > 1 { format!("{name} ×{n}") } else { name.to_string() }
}

/// `1 read` · `3 reads`.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What `ask`'s and the draft's waiting review says of its claude: thinking, or writing what it streams.
fn writing(r: &crate::llm::Review) -> Part {
    match r.thinking || r.streamed.is_empty() {
        true => ("◦ thinking".into(), Ink::Faint),
        false => ("● writing".into(), Ink::Faint),
    }
}

/// Session `l`'s helpers as their rows say them, in `Kind`'s order: each that runs, and each that ended since its
/// session's turn began (the reviewer also while its end is unseen, as its column said — `Reviewer.unseen`).
pub fn helpers(ed: &Editor, l: &crate::workspace::Workspace) -> Vec<Helper> {
    use crate::reviewer::{Role, State};
    let s = &l.claude;
    let began = s.chat.as_ref().map(|c| c.started);
    let since = |at: std::time::Instant| began.is_none_or(|b| at >= b);
    let last = |kind: Kind| s.helpers_ended.iter().find(|e| e.kind == kind && since(e.at));
    let model = crate::models::row_label(&ed.config.llm, l);
    let mut out = Vec::new();
    let mut put = |kind, name: String, said, what, model: &str, cost, running| {
        out.push(Helper { kind, name, said, what, model: model.to_string(), cost, running });
    };
    if let Some(r) = l.reviewer.as_ref() {
        let running = matches!(r.state, State::Waiting | State::Reading);
        if running || r.unseen || since(r.started + r.took.unwrap_or_default()) {
            let said = match &r.state {
                _ if running && crate::reviewer::waits(ed, l) => ("waits to review".into(), Ink::Faint),
                _ if running && r.board.is_some() => {
                    // (the angles, then the skeptics once every angle is over)
                    let of = |skeptic: bool| {
                        r.members.iter().filter(move |m| matches!(m.role, Role::Skeptic(_)) == skeptic)
                    };
                    let skeptics = of(true).next().is_some() && of(false).all(|m| m.over());
                    let reading = of(skeptics).filter(|m| m.state == State::Reading).count();
                    let who = if skeptics { "skeptics" } else { "angles" };
                    (format!("{reading} of {} {who} reading", of(skeptics).count()), Ink::Faint)
                }
                _ if running => (format!("reviewing · {}", count(r.reads, "read", "reads")), Ink::Faint),
                State::Done => {
                    (format!("done · {}", count(r.findings.len(), "finding", "findings")), word("done"))
                }
                State::Failed(_) => ("failed".into(), word("failed")),
                _ => ("stopped".into(), word("stopped")),
            };
            let name = if r.board.is_some() { "board" } else { "reviewer" };
            let cost = r.cost.filter(|_| ed.config.llm.cost).map(crate::usage::dollars).unwrap_or_default();
            put(Kind::Review, name.into(), said, r.label.clone(), &model, cost, running);
        }
    }
    match (s.ask.as_ref(), last(Kind::Ask)) {
        (Some(a), _) => {
            let said = match s.review.as_ref() {
                Some(r) if !r.ready => writing(r),
                Some(_) => ("▸ its change waits on you".into(), Ink::Warn),
                None => ("● working".into(), Ink::Faint),
            };
            put(Kind::Ask, "ask".into(), said, quoted(&a.instruction), &model, String::new(), true);
        }
        (None, Some(e)) => {
            put(Kind::Ask, "ask".into(), e.said.clone(), e.what.clone(), &model, String::new(), false)
        }
        _ => {}
    }
    // (the search's own model, `llm.web-search-model`: its family, as a row's)
    let wanted = ed.config.llm.web_search_model.trim();
    let short = crate::usage::model_short(if wanted.is_empty() { crate::web::search::MODEL } else { wanted });
    let searcher = short.split(' ').next().unwrap_or_default();
    let running = s.searches.iter().filter(|x| !x.answered).count() + s.searches_queued.len();
    let newest = s.searches.iter().rev().find(|x| !x.answered).map(|x| &x.query);
    match (newest.or(s.searches_queued.front().map(|a| &a.query)), last(Kind::Search)) {
        (Some(q), _) => {
            let said = ("• searching".to_string(), Ink::Faint);
            put(
                Kind::Search,
                times("search", running),
                said,
                quoted(&q.words),
                searcher,
                String::new(),
                true,
            );
        }
        (None, Some(e)) => {
            let (said, what) = (e.said.clone(), e.what.clone());
            put(Kind::Search, times("search", e.n), said, what, searcher, String::new(), false);
        }
        _ => {}
    }
    match (s.review.as_ref().filter(|r| !r.ready && s.ask.is_none()), last(Kind::Draft)) {
        (Some(r), _) => {
            put(Kind::Draft, "draft".into(), writing(r), r.instruction.clone(), &model, String::new(), true)
        }
        (None, Some(e)) => {
            put(Kind::Draft, "draft".into(), e.said.clone(), e.what.clone(), &model, String::new(), false)
        }
        _ => {}
    }
    out
}

/// Helper `h`'s row under its session `l`: its name in its session's colour, its step or end, its model, what it was
/// asked (in the report's column), its cost.
fn helper_row(l: &crate::worktree::Shown, h: Helper) -> Row {
    let mut cells: Vec<(Col, Vec<Part>)> = Vec::new();
    // (its state word in the ink a session's row gives the same word — `WORDS`, whoever wrote how it ended)
    let mut said = h.said;
    if let Some(ink) = word_ink(&said.0) {
        said.1 = ink;
    }
    for (c, part) in [
        (Col::State, said),
        (Col::Model, (h.model, Ink::Faint)),
        (Col::Report, (h.what, Ink::Faint)),
        (Col::Cost, (h.cost, Ink::Text)),
    ] {
        if !part.0.is_empty() {
            cells.push((c, vec![part]));
        }
    }
    let (name, working) = (h.name, h.running);
    let helper = Some(h.kind);
    Row { id: l.id, helper, name, colour: l.colour, front: false, arm: String::new(), working, cells }
}

/// The list's top line's counts, in their order — the line folds them from the last: its sessions, how many of them
/// work, then its helpers (design session, 2026-11-16: `8 sessions · 8 working · 3 helpers` read as every session at
/// work — a helper's work is its own row's, never the sessions' count).
pub fn counts(rows: &[Row]) -> Vec<String> {
    let sessions = rows.iter().filter(|r| r.helper.is_none());
    let working = sessions.clone().filter(|r| r.working).count();
    let helpers = rows.len() - sessions.count();
    let mut v = vec![crate::nav::count(rows.len() - helpers, "session")];
    v.extend((working > 0).then(|| format!("{working} working")));
    v.extend((helpers > 0).then(|| crate::nav::count(helpers, "helper")));
    v
}

/// Every session's helpers as the list says them, while the list is in sight (else None) — a helper's event that
/// moved nothing else compares them before and after (`redraw_if_moved`).
pub fn helpers_drawn(ed: &Editor) -> Option<Vec<Helper>> {
    let kind = GroupKind::Sessions;
    (ed.float_list(kind).is_some() && !ed.list_unshown(kind))
        .then(|| ed.sessions_by_parent().into_iter().flat_map(|l| helpers(ed, l)).collect())
}

/// A helper's event said it changed nothing on screen (`ed.unchanged`): but where it moved a helper's row in sight
/// (`before`, `helpers_drawn`'s), it did — the one reason the list adds for a frame.
pub fn redraw_if_moved(ed: &mut Editor, before: Option<Vec<Helper>>) {
    if ed.unchanged && before.is_some_and(|b| Some(b) != helpers_drawn(ed)) {
        ed.unchanged = false;
    }
}

impl Editor {
    /// `space W` (§3.6): the session list floats in sight with the keys, the session in front selected — up already,
    /// the keys to it. Git's part of each row is read again on a worker (the last read shown meanwhile); the
    /// processes looked at once.
    pub fn session_list(&mut self) {
        self.float_list_open(GroupKind::Sessions, self.workspace.id.n, None);
        self.session_facts();
    }

    /// The session list's rows as things are now: each session's, then its helpers' (the one list L4), a child's arm
    /// and a helper's before its name, the model's column where the rows' differ.
    pub fn sessions_rows(&self) -> Vec<Row> {
        let shown = self.session_shown();
        let (mut rows, mut parents, mut helper) = (Vec::new(), Vec::new(), Vec::new());
        let mut at: Vec<(WorkspaceId, usize)> = Vec::new();
        for (l, w) in shown.iter().zip(self.sessions_by_parent()) {
            let me = rows.len();
            parents.push(l.parent.and_then(|p| at.iter().find(|a| a.0 == p).map(|a| a.1)));
            at.push((l.id, me));
            rows.push(row(l));
            helper.push(false);
            for h in helpers(self, w) {
                rows.push(helper_row(l, h));
                parents.push(Some(me));
                helper.push(true);
            }
        }
        for (r, arm) in rows.iter_mut().zip(arms_of(&parents, &helper)) {
            r.arm = arm;
        }
        models_where_they_differ(&mut rows);
        rows
    }

    /// The list's rows' ids, in its order (`sessions_rows`' without building them): each session's, then its helpers'.
    pub fn sessions_row_ids(&self) -> Vec<u64> {
        let mut v = Vec::new();
        for l in self.sessions_by_parent() {
            v.push(l.id.n);
            v.extend(helpers(self, l).iter().map(|h| RowId::Helper { of: l.id, kind: h.kind }.pack()));
        }
        v
    }

    /// The session list has the keys.
    #[cfg(test)]
    pub fn sessions_list_keys(&self) -> bool {
        self.float_list_keys(GroupKind::Sessions)
    }

    /// The session the list's selection is on (by its id, else the row it was on) — None on a helper's row.
    pub fn sessions_here(&self) -> Option<WorkspaceId> {
        match self.sessions_hand()? {
            RowId::Session(id) => Some(id),
            RowId::Helper { .. } => None,
        }
    }

    /// The row the list's selection is on: a session's or a helper's.
    pub fn sessions_hand(&self) -> Option<RowId> {
        self.float_list_here(GroupKind::Sessions).map(RowId::of)
    }

    /// `enter` on the list (a digit, a double click): to the selected session — the list done once used, so it
    /// closes (user, 2026-10-23: floating in the middle, it would cover the code); on the session in front too.
    pub(crate) fn sessions_enter(&mut self) {
        // (a pane's key has its document lent: the session to the front, the list used, once it's back)
        if crate::docpane::later(self, |ed| ed.sessions_enter()) {
            return;
        }
        let id = match self.sessions_hand() {
            Some(RowId::Session(id)) => id,
            Some(RowId::Helper { of, kind }) => return self.helper_enter(of, kind),
            None => return,
        };
        self.float_list_used(GroupKind::Sessions);
        if id != self.workspace.id {
            self.workspace_switch(id);
        } else if self.docked_in_sight(GroupKind::Sessions) {
            // (docked it stays: nothing to switch to, said — no silent key)
            self.set_status(format!("{} is in front — q closes the list", self.workspace.label()));
        }
    }

    /// `enter` on a helper's row (the one list L4): its session to the front, then where its work is — the review's
    /// notes and verdict (`space v r`), `space i`'s or the draft's edit review, a search's stop (its list, as `]r`
    /// shows it). The list done once used, as a session's `enter`.
    pub(crate) fn helper_enter(&mut self, of: WorkspaceId, kind: Kind) {
        self.float_list_used(GroupKind::Sessions);
        if !self.workspace_switch(of).is_in() {
            return;
        }
        let ended = self.workspace.claude.helpers_ended.iter().find(|e| e.kind == kind).cloned();
        match kind {
            Kind::Review => crate::reviewer::verdict(self),
            Kind::Ask | Kind::Draft => {
                let Some(r) = self.workspace.claude.review.as_mut() else {
                    let how = ended.map_or_else(String::new, |e| format!(" — it ended {}", e.said.0));
                    let what = if kind == Kind::Ask { "space i" } else { "the draft" };
                    return self.set_status(format!("{what} has no change waiting for you{how}"));
                };
                // (its buffer in front, the keys the code's: waiting, its card's peek there — a waiting review never
                // takes the place of what your keys are on; ready, focused, as `C-q` does)
                let doc = r.doc_id;
                if let Some(i) = self.docs.iter().position(|d| d.id == doc) {
                    self.current = i;
                }
                self.workspace.viewer.keys_to_code();
                crate::llm::review_focus(self);
            }
            Kind::Search => {
                let s = &self.workspace.claude;
                let newest = s.searches.iter().rev().find(|x| !x.answered).map(|x| x.query.words.clone());
                let words = newest.or_else(|| ended.map(|e| e.what.trim_matches('"').to_string()));
                let url = words.map(|w| crate::web::search::page_url(&w));
                if !url.is_some_and(|u| crate::follow::replay_page(self, &u)) {
                    crate::chat::ask(self);
                    self.set_status("its search's stop is gone — the chat has its call");
                }
            }
        }
    }
}

/// Row `row`'s text `re` matches: its name, else a column's (its parts as one).
pub(crate) fn matches(row: &Row, re: &regex::Regex) -> bool {
    re.is_match(&row.name) || COLS.iter().any(|&c| re.is_match(&row.text(c)))
}

/// The letters the session list takes beyond a floating list's own (`floatlist::key`: the moves, `/` `?` `n` `N`, `q`,
/// `enter`, `esc`) and the digits (`Editor::session_list_key`): each, what it does, and whether it acts on the session
/// in hand — a helper's row has none, so those are refused there in words (the one list L4).
pub const KEYS: [(char, &str, bool); 10] = [
    ('o', "new", false),
    ('O', "fork", true),
    ('d', "end", true),
    ('b', "bring into its base", true),
    ('m', "mark to bring", true),
    ('B', "bring the marked in turn", false),
    ('x', "drop its build", true),
    ('v', "woven", true),
    ('f', "follow", true),
    ('$', "billing", true),
];

/// Key `c` acts on the session in hand (`KEYS`).
pub fn takes_the_session(c: char) -> bool {
    KEYS.iter().any(|k| k.0 == c && k.2)
}

/// The list's keys for its frame's bottom line (docked, its foot), as they apply to the row in hand: `enter` switches, `/` searches, then what the row's session
/// takes — `v` its changes woven where it overlaps, `f` follow while its Claude works, `B` the marked ones in turn —,
/// `o` new, `O` fork, `d` end and `b` bring (not the checkout tarae started in), `m` mark, `x` its build folder once measured,
/// `$` who pays; `esc code`, `q close`. The line folds them from the last.
pub fn hints(ed: &Editor) -> Vec<(&'static str, String)> {
    use crate::workspace::Doing;
    // (a helper's row: where its work is — none of a session's keys are its)
    if let Some(RowId::Helper { kind, .. }) = ed.sessions_hand() {
        let v = [("enter", kind.enter()), ("/", "search"), ("esc", "code"), ("q", "close")];
        return v.into_iter().map(|(k, d)| (k, d.to_string())).collect();
    }
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
