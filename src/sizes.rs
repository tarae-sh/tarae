//! Sizes you set (U1): every pane's, a share in its workspace's tree (onetree S5b) — the chat's, the side panel's and
//! the review rail's widths and the slot's height beside or under the code their leaf's `Share::Cells`, which its rule
//! here clamps (`split::Room::share`), a code pane's its child's in its split. Kept per workspace with its layout
//! (`layouts.json`, the restart's file); `$state/sizes.json` keeps only what floats or docks as a group (the floating
//! frames, the follow card, the docked groups' widths). A screen too small for a kept size shows less of it — clamped
//! here, never written back — and a bigger one gets it again. Unset, each is automatic: the chat 2/5 of the screen
//! (36–96 columns — U3), the side panel 32 and more on a wide screen, the bottom slot what its panel wants (a task's
//! output — U2).
//!
//! `C-w < > + -` grow and shrink the pane in hand (D7, the user 2026-11-04 — Vim's, zellij's and tmux's one meaning):
//! `>` wider, `<` narrower, `+` taller, `-` shorter, its edge toward the side with a pane that way moving (the right
//! edge first, else the left; the bottom first, else the top), the pane across it giving or taking the room
//! (`Editor::size_pane`). A drag of a line sizes the pane whose line it is the same way. `C-w =` makes them all
//! automatic again (a split of the code's even).
//!
//! Beside each other (140 columns: side panel 32 + chat 56 left the code 52): an automatic chat narrows to
//! leave the code `CODE_ROOM` (down to `CHAT_MIN`); if the code would still have less than `CODE_MIN`, the side
//! panel gives way, whole (it comes back when there's room — its keys go back to the code meanwhile).
//! Beside the code review's rail an automatic chat narrows further, to leave the code `REVIEW_CODE_ROOM`. Who gives
//! way to whom is one table, `give_way`.
//!
//! `C-w z` fills the screen with what has the keys — the code (its pane alone), the bottom slot, the chat, the
//! side panel — and back (`Zoom`); it ends when the keys go elsewhere. The bottom slot takes the keys with
//! `C-w j` or a click: they're its panel's view mode (`view.rs` — `q` closes it, Esc or `C-w k` back to the
//! code).

use crate::pane::Shows;
use crate::workspace::Here;

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The chat's width bounds — automatic up to `CHAT_AUTO_MAX` (U3), set by you up to what leaves the code
/// `CHAT_FULL_CODE` (below that the chat takes the whole width, as on a narrow screen).
pub const CHAT_MIN: usize = 36;
pub const CHAT_AUTO_MAX: usize = 96;
pub const CHAT_FULL_CODE: usize = 40;
/// The side panel's: automatic `SIDE_AUTO` (+ up to `SIDE_MORE` on a wide screen, the code keeping 100).
pub const SIDE_MIN: usize = 20;
pub const SIDE_MAX: usize = 72;
pub const SIDE_AUTO: usize = 32;
pub const SIDE_MORE: usize = 12;
/// The code beside them: an automatic chat narrows to leave it this much; under `CODE_MIN` the side panel
/// gives way.
pub const CODE_ROOM: usize = 60;
pub const CODE_MIN: usize = 48;
/// The code beside the review's rail: an automatic chat narrows to leave it this much (`give_way`'s row 3).
pub const REVIEW_CODE_ROOM: usize = 100;
/// The code review's rail: what its rows need between `RAIL_MIN` and `RAIL_AUTO_MAX`, or yours up to `RAIL_MAX` (a drag
/// of its edge, `C-w < >` with its keys — mouse R, Q4); the code beside it keeps `RAIL_CODE` (from 90 columns on).
pub const RAIL_MIN: usize = 24;
pub const RAIL_AUTO_MAX: usize = 48;
pub const RAIL_MAX: usize = SIDE_MAX;
pub const RAIL_CODE: usize = 60;
/// The bottom slot: at least this many rows, and the code keeps `BOTTOM_CODE` above it.
pub const BOTTOM_MIN: usize = 3;
pub const BOTTOM_CODE: usize = 4;
/// The conversation stacked over or under the code is at least this tall (`across`).
pub const CHAT_ROWS_MIN: usize = 12;
/// The navigator stacked: the rows besides its list's (its top line, its foot's keys).
const NAV_ROWS_MORE: usize = 2;
/// How a size's toast ends (the next one replaces it — `say_size`).
const SIZE_TAIL: &str = " · C-w = automatic";
/// One `C-w <`/`>` · `C-w -`/`+`.
pub const STEP_COLS: usize = 4;
pub const STEP_ROWS: usize = 2;

/// What you set of what floats or docks as a group (None: automatic) — the tree's panes keep theirs in the tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sizes {
    /// A docked group's, per kind (`GroupKind::dock_index`): its width beside the code, its rows above it.
    pub docked: [Option<u16>; crate::pane::DOCKABLE],
    /// The session list's frame (`sessionpane.rs`) — where you put it, the viewer's (None: centred).
    pub sessions: Option<crate::pane::Frame>,
    /// `:messages`' frame, as the session list's.
    pub messages: Option<crate::pane::Frame>,
    /// `:folders`' frame, as the session list's.
    pub folders: Option<crate::pane::Frame>,
    /// The side the session list is docked on (P3 step 7d, `C-w H/K/L`; `C-w e` floats it again) — the viewer's place
    /// for it, kept while it closes and opens and across `:restart`: the session in front holds its leaf
    /// (`list_dock_sync`). None: it floats.
    pub sessions_dock: Option<crate::pane::Side>,
    /// The follow card's width and height, whole cells, frame and all (#267: its free edges drag, `C-w < > - +` while
    /// it has the keys) — each None: automatic, by the pane it leads. Shown within the pane and
    /// the gap it stands in, never written back.
    pub follow_w: Option<u16>,
    pub follow_h: Option<u16>,
}

/// The ask line's width (`Editor::chat_float_rect`): the chat's prose measure (`chrome::MEASURE`), the gutter's cell and
/// the text's padding either side (`term::chat_widths`), the frame's right line.
pub const CHAT_FLOAT_W: usize = crate::chrome::MEASURE + crate::chrome::GAP + 2 * crate::chrome::PAD + 1;

/// The follow card at its smallest: its line's names and a strip need this (a body of 28 — `term::draw_follow_card`);
/// three rows, one of them its own.
pub const FOLLOW_MIN_W: usize = 32;
pub const FOLLOW_MIN_H: usize = 3;

/// The dockable kinds by `GroupKind::dock_index`, as `sizes.json` names them.
const DOCKED: [crate::pane::GroupKind; crate::pane::DOCKABLE] = [
    crate::pane::GroupKind::Tests,
    crate::pane::GroupKind::Task,
    crate::pane::GroupKind::Preflight,
    crate::pane::GroupKind::Procs,
    crate::pane::GroupKind::Sessions,
];

impl Sizes {
    pub fn path() -> Option<PathBuf> {
        crate::config::state_dir().map(|d| d.join("sizes.json"))
    }

    /// Read from `path` (missing or unreadable: all automatic).
    pub fn read(path: Option<&Path>) -> Sizes {
        let Some(v) = path
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            return Sizes::default();
        };
        let n = |v: &Value| v.as_u64().and_then(|n| u16::try_from(n).ok());
        let docked = DOCKED.map(|k| n(&v["docked"][k.name()]));
        let (sessions, messages) =
            (crate::pane::Frame::of_json(&v["sessions"]), crate::pane::Frame::of_json(&v["messages"]));
        Sizes {
            docked,
            sessions,
            messages,
            folders: crate::pane::Frame::of_json(&v["folders"]),
            sessions_dock: v["sessions_dock"].as_str().and_then(crate::pane::Side::of_name),
            follow_w: n(&v["follow"]["w"]),
            follow_h: n(&v["follow"]["h"]),
        }
    }

    pub fn to_json(self) -> String {
        let docked: serde_json::Map<String, Value> = DOCKED
            .iter()
            .filter_map(|k| Some((k.name().to_string(), json!(self.docked[k.dock_index()]?))))
            .collect();
        let (sessions, messages) =
            (self.sessions.map(crate::pane::Frame::json), self.messages.map(crate::pane::Frame::json));
        let dock = self.sessions_dock.map(crate::pane::Side::name);
        let folders = self.folders.map(crate::pane::Frame::json);
        let follow = json!({ "w": self.follow_w, "h": self.follow_h });
        json!({ "docked": docked, "sessions": sessions, "messages": messages, "folders": folders, "sessions_dock": dock,
                "follow": follow })
        .to_string()
    }
}

/// Written on a worker: the last one asked for wins (an older write that finishes later is skipped) — counted
/// per file.
pub fn save(path: &Path, sizes: Sizes, seq: u64) {
    use std::sync::Mutex;
    static LAST: Mutex<Vec<(PathBuf, u64)>> = Mutex::new(Vec::new());
    let Ok(mut last) = LAST.lock() else { return };
    match last.iter_mut().find(|(p, _)| p == path) {
        Some((_, n)) if seq < *n => return,
        Some((_, n)) => *n = seq,
        None => last.push((path.to_path_buf(), seq)),
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, sizes.to_json()).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// The gives-way table (onetree §2.3): on a screen too narrow for every pane at its own width, which kinds give way and
/// in what order — by kind, never by edge. Kept sizes are clamped here, never written back.
/// 1. a group docked beside the code folds into the slot (`group_beside`: under `DOCK_MIN`, or the code keeping less
///    than `CODE_MIN`);
/// 2. the navigator goes, whole: the side panel where the code would keep less than `CODE_MIN`, the review's rail
///    where it would keep less than its room (`rail_code_min`);
/// 3. an automatic conversation narrows to leave the code `CODE_ROOM` beside the side panel, `REVIEW_CODE_ROOM` beside
///    the review's rail (frictions25 R6: reading the code is what a review is for), down to `CHAT_MIN`;
/// 4. below `CHAT_FULL_CODE` beside it the conversation takes the whole width (`Editor::chat_panel`: only while it has
///    the keys);
/// 5. the slot moved beside the code (onetree S5) takes 2/5 of what the columns leave, as much as leaves the code
///    `CODE_MIN` — yours too —, down to `SIDE_MIN`; its least counts as the code's in rows 2 and 3 (the navigator goes
///    before the code is squeezed for it).
///
/// Down the screen, the slot's height (`bottom`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wants {
    /// The screen's columns.
    pub total: usize,
    /// The conversation docked as a column (`Editor::chat_panel`) and the width you set (None inside: automatic);
    /// None: no column.
    pub chat: Option<Option<u16>>,
    pub nav: Nav,
    /// The slot beside the code, shown (`Editor::slot_beside`); None: not beside it.
    pub slot: Option<Beside>,
}

/// The slot moved beside the code: the width you set there (None: automatic), and whether the conversation and the
/// navigator stand beside it — side by side, not stacked over or under everything (`C-w J` from the chat).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Beside {
    pub kept: Option<u16>,
    pub chat: bool,
    pub nav: bool,
}

/// The navigator beside the code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    None,
    /// The side panel, and the width you set (None: automatic).
    Side(Option<u16>),
    /// The code review's rail: its width with room (`term::review_rail_natural` — yours, or what its rows need), and
    /// whether it has the keys.
    Rail {
        natural: usize,
        focus: bool,
    },
}

/// What the table gives each: the conversation's width, the navigator's (0: none, or given way) and the slot's beside
/// the code (0: not beside it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Given {
    pub chat: usize,
    pub nav: usize,
    pub slot: usize,
}

/// The table's widths (above) for what `w` wants.
pub fn give_way(w: Wants) -> Given {
    let Given { chat, nav, .. } = columns(w);
    // (row 5: the slot in what the columns beside it leave, the code keeping `CODE_MIN` first)
    let slot = w.slot.map_or(0, |b| {
        let area = w.total.saturating_sub(if b.chat { chat } else { 0 } + if b.nav { nav } else { 0 });
        let want = b.kept.map_or(area * 2 / 5, |k| k as usize);
        want.min(area.saturating_sub(CODE_MIN)).max(SIDE_MIN).min(area)
    });
    Given { chat, nav, slot }
}

/// Rows 2–4: the conversation's and the navigator's widths, the slot's least beside the code counted as the code's.
fn columns(w: Wants) -> Given {
    let Wants { total, chat, nav, slot } = w;
    let least = if slot.is_some() { SIDE_MIN } else { 0 };
    let (code_min, code_room, review_room) = (CODE_MIN + least, CODE_ROOM + least, REVIEW_CODE_ROOM + least);
    let alone = chat.map_or(0, |kept| chat_alone(total, kept));
    let given = |chat, nav| Given { chat, nav, slot: 0 };
    match nav {
        Nav::None => given(alone, 0),
        Nav::Side(kept_side) => {
            // (row 4: the conversation takes it all, no navigator beside it)
            if chat.is_some() && alone >= total {
                return given(total, 0);
            }
            let auto_side =
                |edit: usize| SIDE_AUTO + (edit.saturating_sub(SIDE_AUTO + 100) / 2).min(SIDE_MORE);
            let side_of = |chat: usize| match kept_side {
                Some(k) => (k as usize).clamp(SIDE_MIN, SIDE_MAX),
                None => auto_side(total - chat),
            };
            let (mut c, mut side) = (alone, side_of(alone));
            // (row 3: an automatic conversation makes room for the code beside the panel)
            if chat == Some(None) && total < c + side + code_room {
                c = total.saturating_sub(side + code_room).max(CHAT_MIN).min(c);
                side = side_of(c);
            }
            // (row 2: at its width it would leave the code less than `CODE_MIN` — it gives way, not squeezed, the
            // conversation as it would be without it)
            match total < c + side + code_min {
                true => given(alone, 0),
                false => given(c, side),
            }
        }
        Nav::Rail { natural, focus } => {
            // (row 3 beside the rail, only where the rail is then drawn: one too narrow for it anyway keeps its
            // conversation as it was — frictions25: at 120 the chat lost 12 columns for a rail that didn't come)
            let c = match chat {
                Some(None) if alone < total => {
                    let beside = total.saturating_sub(natural + review_room).max(CHAT_MIN).min(alone);
                    match rail_code_min(total.saturating_sub(beside + least), focus) {
                        Some(_) => beside,
                        None => alone,
                    }
                }
                _ => alone,
            };
            // (row 2: the rail drawn only where the code keeps its room beside it)
            let area = (total - c).saturating_sub(least);
            let nav = rail_code_min(area, focus).map_or(0, |min| natural.min(area.saturating_sub(min)));
            given(c, nav)
        }
    }
}

/// The conversation's width with no navigator beside it: yours clamped, else 2/5 of the screen (`CHAT_MIN`–
/// `CHAT_AUTO_MAX`) — the whole width where the code would keep less than `CHAT_FULL_CODE` (row 4).
fn chat_alone(total: usize, kept: Option<u16>) -> usize {
    let chat = match kept {
        Some(k) => (k as usize).clamp(CHAT_MIN, total.saturating_sub(CHAT_FULL_CODE).max(CHAT_MIN)),
        None => (total * 2 / 5).clamp(CHAT_MIN, CHAT_AUTO_MAX),
    };
    if total < chat + CHAT_FULL_CODE { total } else { chat }
}

/// Whether the review's rail is drawn in an area `area` columns wide (the screen less the conversation), and what it
/// leaves the code — from 90 on, `RAIL_CODE`; narrower, while it has the keys (`space g` back from the git tab,
/// `space e`), in the room the side panel would have — they went to a rail that wasn't drawn (#146). None: not drawn.
fn rail_code_min(area: usize, focus: bool) -> Option<usize> {
    match area {
        w if w >= 90 => Some(RAIL_CODE),
        w if focus && w >= RAIL_MIN + CODE_MIN => Some(CODE_MIN),
        _ => None,
    }
}

/// Row 1: a group docked beside the code, in `room` columns (what the other columns leave, with its side's column
/// given back) — yours (`Sizes.docked`) as much as leaves the code `CODE_MIN`, automatic as much as leaves it
/// `CODE_ROOM` up to 2/5 of the screen, at least as wide as its side's `column` so far. None: it folds into the slot.
pub fn group_beside(total: usize, room: usize, kept: Option<u16>, column: usize) -> Option<usize> {
    let want = match kept {
        Some(k) => (k as usize).min(room.saturating_sub(CODE_MIN)),
        None => (total * 2 / 5).min(room.saturating_sub(CODE_ROOM)),
    }
    .max(column);
    (want >= crate::dock::DOCK_MIN && room.saturating_sub(want) >= CODE_MIN).then_some(want)
}

/// The table down the screen: the bottom slot's height in `total` rows (code and slot) — a kept size clamped, the
/// code keeping `BOTTOM_CODE` (none where not even `BOTTOM_MIN` fits), else what its panel wants.
pub fn bottom(total: usize, kept: Option<u16>, natural: usize) -> usize {
    let max = total.saturating_sub(BOTTOM_CODE);
    match kept {
        Some(k) if max >= BOTTOM_MIN => (k as usize).clamp(BOTTOM_MIN, max),
        Some(_) => 0,
        None => natural,
    }
}

/// A kind sitting in a split of the other way than its home's (onetree §2.3) — today's layout never does: reached once
/// panes move (S5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Across {
    /// The conversation over or under the code.
    Chat,
    /// The navigator over or under it, `rows` its list's.
    Nav { rows: usize },
}

/// Its size along that split, of `avail` cells: the one you set (`kept`, onetree S5b), clamped to `across_limits`; else
/// its natural one — the conversation a third of the rows, at least `CHAT_ROWS_MIN`; the navigator its rows, up to a
/// third. (The slot beside the code is the gives-way table's, row 5.)
pub fn across(kind: Across, avail: usize, kept: Option<u16>) -> usize {
    if let Some(k) = kept {
        let (min, max) = across_limits(kind, avail);
        return (k as usize).clamp(min.min(max), max);
    }
    match kind {
        Across::Chat => (avail / 3).max(CHAT_ROWS_MIN).min(avail),
        Across::Nav { rows } => (rows + NAV_ROWS_MORE).min(avail / 3),
    }
}

/// A size you set on a kind sitting the other way, in `avail` rows: at least the conversation's `CHAT_ROWS_MIN`, the
/// navigator's `BOTTOM_MIN`; at most what leaves the code beside it its least (`BOTTOM_CODE`).
pub fn across_limits(kind: Across, avail: usize) -> (usize, usize) {
    match kind {
        Across::Chat => (CHAT_ROWS_MIN, avail.saturating_sub(BOTTOM_CODE)),
        Across::Nav { .. } => (BOTTOM_MIN, avail.saturating_sub(BOTTOM_CODE)),
    }
}

/// A border the mouse drags: a line between two leaves (`edges.rs`), or the follow card's free edges. A press on one
/// without a drag gives the keys to what it heads (`border_heads`; mouse R, Q9); under the pointer it's lit as it is
/// while dragged (`border_lit`, Q7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Border {
    /// A line along `dir` between two leaves (onetree S3, `edges::Line`): `sized` the leaf a drag of it sizes, each kind
    /// by its own rule (`border_drag`), `heads` the one a press gives the keys. A surface's own line — the chat's left
    /// gutter, the side panel's and the rail's right one, the slot's top line, a docked group's edge — sizes and heads
    /// that surface; between two code panes — side by side the `│` left of `heads`, over and under `heads`' top line —
    /// `sized` is the pane before it (left or above), whose child in that split ends there, `heads` the pane after it.
    Divider { sized: crate::split::PaneId, heads: crate::split::PaneId, dir: crate::split::Dir },
    /// The follow card's free edges (#267): its left line sizes its width (`cols`), the line toward the gap it stands in
    /// — its top line at the gap's foot, its bottom line at its top — its height (`rows`); the corner both. Its right
    /// and its other line stay where the rule put them.
    Follow { cols: bool, rows: bool },
}

/// A code pane keeps at least this many columns and rows when its split is sized.
pub const PANE_MIN_COLS: usize = 12;
pub const PANE_MIN_ROWS: usize = 3;

/// What a size says a pane is (`say_size`).
fn size_name(shows: Option<Shows>) -> &'static str {
    match shows {
        Some(Shows::Chat) => "chat",
        Some(Shows::Side) => "side panel",
        Some(Shows::Rail) => "review rail",
        Some(Shows::Slot) => "bottom panel",
        _ => "pane",
    }
}

fn unit(dir: crate::split::Dir) -> &'static str {
    match dir {
        crate::split::Dir::Vertical => "columns",
        crate::split::Dir::Horizontal => "rows",
    }
}

/// What `C-w z` fills the screen with: a pane — a code pane alone, the bottom slot, the chat, the side panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zoom(pub crate::split::PaneId);

/// What the bottom slot shows — one at a time: `:procs` while it's open (asked for just now), then the
/// terminal (when shown), a plugin's panel, and the test run or task started last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Procs,
    Terminal,
    Plugin,
    Test,
    Task,
}

impl crate::editor::Editor {
    /// Whether `q` closes the test or task panel now: only once its run has ended — one under way (or a rebase
    /// stopped at its conflicts) is stopped by its own `space x c` / `space m c`, never by a `q` meant for a pager.
    pub fn q_closes(&self, slot: Slot) -> bool {
        use crate::task::State;
        match slot {
            Slot::Test => {
                self.workspace.test_run.as_ref().is_some_and(|r| r.state != crate::testing::RunState::Running)
            }
            Slot::Task => self
                .workspace
                .task_run
                .as_ref()
                .is_some_and(|r| matches!(r.state, State::Passed | State::Failed(_) | State::NotRun(_))),
            Slot::Procs => self.procs_up(),
            _ => false,
        }
    }

    /// `q` in a list or output panel with tarae's keys, and `C-w q` there: it closes — the terminal's hides as
    /// `space T` hides it (the shell runs on), a test's or task's once its run has ended (else said, the run
    /// untouched), `:procs`.
    pub fn slot_close(&mut self, slot: Slot) {
        match slot {
            Slot::Terminal => {
                if let Some(t) = self.workspace.terminal.as_mut() {
                    t.visible = false;
                }
                self.workspace.leave(Here::Program);
                self.workspace.leave(Here::Slot);
            }
            // A rebase stopped at its conflicts isn't running: it waits on them, and its panel with it — `space m c`
            // isn't the way out there (it drops the run the rebase is), `:rebase-abort` is
            Slot::Task
                if let Some(crate::task::State::Stopped(why)) =
                    self.workspace.task_run.as_ref().map(|r| &r.state) =>
            {
                let why = why.clone();
                self.set_status(format!(
                    "the rebase waits at {why} — its panel stays until it ends (:rebase-abort ends it)"
                ));
            }
            Slot::Test | Slot::Task if !self.q_closes(slot) => {
                let (what, stop) = match slot {
                    Slot::Test => ("the tests are", "space x c"),
                    _ => ("the task is", "space m c"),
                };
                self.set_status(format!("{what} still running — {stop} stops and closes"));
            }
            Slot::Test => {
                self.test_close();
                self.workspace.leave(Here::Slot);
            }
            Slot::Task => {
                self.task_close();
                self.workspace.leave(Here::Slot);
            }
            Slot::Procs => self.procs_close(),
            Slot::Plugin => {}
        }
    }

    /// `C-w q`: what has the keys closes, as its own `q` closes it — the side panel (a plugin's tab in it too:
    /// its `q` is tarae's, the side panel goes and the tab stays the plugin's), the review's rail (the review
    /// ends, asked first), `:procs`, the test, task and terminal panels. False: the code has them (its pane
    /// closes). The chat hides, as `space L` hides it (onetree S5c, `q` per kind; `chat::hide`): closing it ends its conversation's
    /// process, and a slip of two keys mustn't cut a turn off — `:chat-close` does that. Not a plugin's bottom panel
    /// (its `q` is the plugin's, and only the plugin closes it — tarae has no event to tell it so): said.
    pub fn close_focused(&mut self) -> bool {
        use crate::uistate::Region;
        match self.keys_to().region {
            Region::Side(_) => self.workspace.viewer.sidebar = None,
            Region::Rail => crate::codereview::close_asked(self),
            Region::Slot(Slot::Plugin) => self.set_status("a plugin's panel closes by its own keys"),
            Region::Slot(s) => self.slot_close(s),
            Region::List(k) => crate::grouplist::close(self, k),
            Region::View(crate::view::Surface::Terminal) => self.slot_close(Slot::Terminal),
            Region::Output(r) => self.slot_close(r.slot()),
            Region::Chat | Region::View(crate::view::Surface::Chat) => {
                // (nothing said, as `space L` says nothing — the user, 2026-11-03: it's gone from sight)
                crate::chat::hide(self);
            }
            _ => return false,
        }
        true
    }
}

impl crate::editor::Editor {
    /// How wide the chat can be on this screen: beside the side panel no wider than leaves the code
    /// `CODE_MIN` (else the panel would go), alone what leaves it `CHAT_FULL_CODE`.
    fn chat_max(&self, total: usize) -> usize {
        match self.screen.drawn_w(Shows::Side) {
            0 => total.saturating_sub(CHAT_FULL_CODE),
            side => total.saturating_sub(side + CODE_MIN),
        }
    }

    /// How wide the side panel can be: what leaves the code `CODE_MIN` beside the chat.
    fn side_max(&self, total: usize) -> usize {
        total.saturating_sub(self.screen.drawn_w(Shows::Chat) + CODE_MIN).min(SIDE_MAX)
    }

    /// How wide the review's rail can be: what leaves the code beside it `RAIL_CODE`, `RAIL_MAX` at most.
    fn rail_max(&self, total: usize) -> usize {
        total.saturating_sub(self.screen.drawn_w(Shows::Chat) + RAIL_CODE).min(RAIL_MAX)
    }

    /// How wide the slot beside the code can be: what leaves the code `CODE_MIN` beside the columns standing beside it
    /// (`give_way`'s row 5), `SIDE_MIN` at least.
    fn slot_max(&self, total: usize) -> usize {
        let s = &self.screen;
        let columns: usize = [Shows::Chat, Shows::Side, Shows::Rail]
            .into_iter()
            .filter(|&k| self.side_by_side(k))
            .map(|k| s.drawn_w(k))
            .sum();
        total.saturating_sub(columns + CODE_MIN).max(SIDE_MIN)
    }

    /// Whether surface `kind`'s leaf sits side by side (its home's way), not stacked over or under.
    fn side_by_side(&self, kind: Shows) -> bool {
        let ws = &self.workspace;
        ws.leaf(kind).is_some_and(|id| ws.split.parent_dir(id) == Some(crate::split::Dir::Vertical))
    }

    /// The slot moved beside the code (its column, `Workspace::slot_column` — a code pane over or under it in it too)
    /// with a panel to show: the width you set there and the columns beside it — `give_way`'s row 5; None: under the
    /// code, or nothing in it.
    pub fn slot_beside(&self) -> Option<Beside> {
        self.slot_beside_if(|| self.slot_shown().is_some())
    }

    /// `slot_beside` as the docked groups' widths ask it (`dock_widths`): a group docked right or left is out of the
    /// slot wherever its leaf is (`docked_beside`) — whether it gets a column is what they work out, and asking
    /// `docked_in_sight` there asked them again, without end. With the slot taking no room for it, it gets one where
    /// any is.
    pub fn slot_beside_unlaid(&self) -> Option<Beside> {
        let out = |s: &Slot| self.kind_of(*s).is_some_and(|k| self.docked_beside(k) || self.out_of_slot(k));
        self.slot_beside_if(|| self.slot_shown_by(|s| !out(s)).is_some())
    }

    /// `slot_beside`, `shows` saying whether the slot has a panel to show.
    fn slot_beside_if(&self, shows: impl FnOnce() -> bool) -> Option<Beside> {
        let (share, _) = self.workspace.slot_column()?;
        let kept = match share {
            crate::split::Share::Cells(n) => Some(n),
            _ => None,
        };
        let chat = self.side_by_side(Shows::Chat);
        let nav = self.side_by_side(Shows::Side) || self.side_by_side(Shows::Rail);
        shows().then_some(Beside { kept, chat, nav })
    }

    /// How tall the bottom slot can be: the code keeps `BOTTOM_CODE`.
    fn bottom_max(&self) -> usize {
        let s = &self.screen;
        s.body_end().saturating_sub(s.text_top + BOTTOM_CODE).max(BOTTOM_MIN)
    }

    /// A surface's own line (the chat's, the side panel's, the rail's, the slot's, a docked group's host's): the border
    /// that sizes and heads it, along its way — None where it has no leaf.
    pub fn line_of(&self, shows: crate::pane::Shows) -> Option<Border> {
        use crate::pane::{Place, Shows, Side};
        use crate::split::Dir;
        let id = self.workspace.leaf(shows)?;
        let dir = match shows {
            Shows::Slot => Dir::Horizontal,
            Shows::Group(k) if self.workspace.group(k)?.place == Place::Docked(Side::Top) => Dir::Horizontal,
            _ => Dir::Vertical,
        };
        Some(Border::Divider { sized: id, heads: id, dir })
    }

    /// What a drag of border `b` sizes: the kind of the leaf it sizes (the follow card's edges: None).
    pub fn border_sizes(&self, b: Border) -> Option<crate::pane::Shows> {
        match b {
            Border::Divider { sized, .. } => self.workspace.shows(sized),
            Border::Follow { .. } => None,
        }
    }

    /// The border at screen cell (`x`, `y`) — as last drawn: the follow card's free edges, else the frame's line
    /// there (`Screen.edges`) — but under a word on it that isn't the line: the bottom slot's tabs and its group's
    /// names (a click there brings that tab in front), a session's name on a code pane's line.
    pub fn border_at(&self, x: usize, y: usize) -> Option<Border> {
        // (one thing fills the screen: no borders)
        if self.zoomed().is_some() {
            return None;
        }
        let s = &self.screen;
        // (none before the first frame)
        s.width?;
        // (the follow card floats over what's under it: its edges first)
        if let Some((cx, cy, cw, ch)) = s.thought_card.get().filter(|_| self.workspace.claude.chat.is_some())
            && (cx..cx + cw).contains(&x)
            && (cy..cy + ch).contains(&y)
        {
            let free = if s.follow_at_foot.get() { cy } else { cy + ch - 1 };
            let (cols, rows) = (x == cx, y == free);
            // (a mark of the turn's strip on that line is its own button — its replay)
            let mark = s.stop_marks.borrow().iter().any(|m| m.0 == y && m.1 == x);
            if (cols || rows) && !mark {
                return Some(Border::Follow { cols, rows });
            }
        }
        let tab = || {
            s.slot_tabs.borrow().iter().any(|t| t.0 == y && (t.1..t.2).contains(&x))
                || s.member_names.borrow().iter().any(|t| t.0 == y && (t.1..t.2).contains(&x))
        };
        let word = || s.session_spots.borrow().iter().any(|l| l.0 == y && (l.1..l.2).contains(&x));
        let passed = |l: &crate::edges::Line| match self.workspace.shows(l.heads) {
            Some(Shows::Slot) => tab(),
            // (a code pane's or the stacked chat's top line: a name on it is its own)
            Some(Shows::Code | Shows::Chat) => l.dir == crate::split::Dir::Horizontal && word(),
            _ => false,
        };
        let l = s.edges.at(x, y, passed)?;
        Some(Border::Divider { sized: l.sized, heads: l.heads, dir: l.dir })
    }

    /// A press on border `b` without a drag: the keys to what it heads (zellij's frame press — mouse R, Q9) — the chat,
    /// the side panel, the rail, the bottom slot, a docked group, the code pane whose line it is.
    pub fn border_heads(&mut self, b: Border) {
        use crate::pane::Shows;
        match b {
            Border::Divider { heads, .. } => match self.workspace.shows(heads) {
                Some(Shows::Chat) => self.workspace.set_at(Here::Chat),
                Some(Shows::Side) if !self.workspace.is_at(Here::Side) => {
                    self.plugin_panel_unfocus();
                    self.procs_unfocus();
                    self.side_focus();
                }
                Some(Shows::Rail) if !self.workspace.is_at(Here::Rail) => crate::codereview::rail_focus(self),
                Some(Shows::Slot) if !self.bottom_has_keys() => self.bottom_take_keys(),
                Some(Shows::Group(kind)) => self.float_focus(kind),
                Some(Shows::Code) => self.pane_takes_keys(heads),
                _ => {}
            },
            // (as a press on the card: the chat it's Claude's turn of, brought back with the keys)
            Border::Follow { .. } => {
                if let Some(c) = self.workspace.claude.chat.as_mut() {
                    c.hidden = false;
                    self.workspace.set_at(Here::Chat);
                }
            }
        }
    }

    /// The line held, lit in the accent — the slot's line held by a name pressed on it only once it moves (a tab's
    /// press is the tab's).
    pub fn border_held(&self) -> Option<Border> {
        self.dragging.filter(|_| !(self.border_name && !self.border_moved))
    }

    /// A line lit: the one held, else the one under the pointer (Q7 — half lit: `term::grab_style`) — what the tests
    /// read where they draw no colours.
    #[cfg(test)]
    pub fn border_lit(&self) -> Option<Border> {
        match self.dragging {
            Some(_) => self.border_held(),
            None => self.border_hover,
        }
    }

    /// A split of yours around the code pane `id` along `dir`: its child holding the pane, that child's size along
    /// `dir` as last laid out, and the room it and its siblings that aren't a surface's own share (None: no other
    /// code pane beside it that way — the surfaces' sizes are sized instead).
    fn own_split(&self, id: crate::split::PaneId, dir: crate::split::Dir) -> Option<(usize, usize, usize)> {
        use crate::split::{Dir, Share, Split};
        let mut tree = self.workspace.split.clone();
        let (kids, i) = tree.parent_along(id, dir)?;
        let along = |s: &Split| {
            let rects: Vec<crate::split::Rect> =
                self.screen.leaves.iter().filter(|l| s.holds(l.0)).map(|l| l.1).collect();
            let (from, to) = match dir {
                Dir::Vertical => (rects.iter().map(|r| r.x).min()?, rects.iter().map(|r| r.x + r.w).max()?),
                Dir::Horizontal => (rects.iter().map(|r| r.y).min()?, rects.iter().map(|r| r.y + r.h).max()?),
            };
            Some(to - from)
        };
        // (a surface's leaf is no split of yours, a size you set on it neither — `Workspace::own_sized`)
        let own = |j: usize| kids[j].0 == Share::Auto || self.workspace.own_sized(&kids[j].1);
        let others: Vec<usize> = (0..kids.len()).filter(|&j| j != i && !own(j)).collect();
        if others.is_empty() {
            return None;
        }
        let now = along(&kids[i].1)?;
        let room = now + others.iter().filter_map(|&j| along(&kids[j].1)).sum::<usize>();
        Some((i, now, room.saturating_sub(others.len() * Self::pane_min(dir))))
    }

    fn pane_min(dir: crate::split::Dir) -> usize {
        match dir {
            crate::split::Dir::Vertical => PANE_MIN_COLS,
            crate::split::Dir::Horizontal => PANE_MIN_ROWS,
        }
    }

    /// The code pane `id`'s child in its split along `dir` set to `n` cells (within its room) — said, the tree kept
    /// with the session.
    fn pane_size(
        &mut self,
        id: crate::split::PaneId,
        dir: crate::split::Dir,
        n: usize,
        room: usize,
    ) -> usize {
        let n = n.clamp(Self::pane_min(dir), room.max(Self::pane_min(dir)));
        if let Some((kids, i)) = self.workspace.split.parent_along(id, dir) {
            kids[i].0 = crate::split::Share::Cells(n as u16);
        }
        n
    }

    /// The cells `kid` takes along `dir` as the last frame laid it out: from its first leaf with room to its last's end
    /// (0: none with room).
    fn laid_along(&self, kid: &crate::split::Split, dir: crate::split::Dir) -> usize {
        // (the slot's height is its panel's as drawn, the header's line with it — what a size of it sets, `sizes::bottom`)
        if let crate::split::Split::Leaf(id) = kid
            && dir == crate::split::Dir::Horizontal
            && self.workspace.shows(*id) == Some(Shows::Slot)
            && self.screen.slot().h > 0
        {
            return self.screen.slot().h;
        }
        // (a group's members are laid in its host's place: theirs are its)
        let held = |id: crate::split::PaneId| {
            kid.holds(id)
                || (self.workspace.groups.iter()).any(|g| g.member(id).is_some() && kid.holds(g.host))
        };
        let rects = || {
            (self.screen.leaves.iter()).filter(|l| l.1.w > 0 && l.1.h > 0 && held(l.0)).map(|l| match dir {
                crate::split::Dir::Vertical => (l.1.x, l.1.x + l.1.w),
                crate::split::Dir::Horizontal => (l.1.y, l.1.y + l.1.h),
            })
        };
        match (rects().map(|r| r.0).min(), rects().map(|r| r.1).max()) {
            (Some(from), Some(to)) => to - from,
            _ => 0,
        }
    }

    /// The least and the most `kid` can be along `dir` on this screen, `room` what it and the pane across its edge hold
    /// less that one's least: a surface by its rule (the chat 36 columns up to what leaves the code its room …; sitting
    /// the other way, `across_limits`'), a code pane or a node `PANE_MIN_*`.
    fn size_limits(&self, kid: &crate::split::Split, dir: crate::split::Dir, room: usize) -> (usize, usize) {
        use crate::split::{Dir, Split};
        let total = self.screen.width.unwrap_or(0);
        let shows = match kid {
            Split::Leaf(id) if self.workspace.own_sized(kid) => self.workspace.shows(*id),
            _ => None,
        };
        let (min, max) = match (shows, dir) {
            (Some(Shows::Chat), Dir::Vertical) => (CHAT_MIN, self.chat_max(total)),
            (Some(Shows::Side), Dir::Vertical) => (SIDE_MIN, self.side_max(total)),
            (Some(Shows::Rail), Dir::Vertical) => (RAIL_MIN, self.rail_max(total)),
            (Some(Shows::Slot), Dir::Horizontal) => (BOTTOM_MIN, self.bottom_max()),
            (Some(Shows::Chat), Dir::Horizontal) => (CHAT_ROWS_MIN, room),
            (Some(Shows::Slot), Dir::Vertical) => (SIDE_MIN, self.slot_max(total)),
            (Some(_), Dir::Horizontal) => (BOTTOM_MIN, room),
            _ => (Self::pane_min(dir), room),
        };
        (min, max.min(room).max(min))
    }

    /// `C-w < > + -` on the pane in hand (D7, onetree §2.4): it `STEP_COLS` wider or narrower, `STEP_ROWS` taller or
    /// shorter. The edge that moves is toward the side with a pane that way — the nearest split along `dir` with one
    /// with room after the child holding it, else before (the right or bottom edge first: from the right one of two code
    /// panes `C-w >` takes from the chat right of them, as Vim's does) — and the pane across it gives or takes the room.
    /// What's set is a `Share::Cells` in the workspace's tree: the pane across's where it's a surface or one you set (its
    /// own size), else the child holding this one's — and in each split along `dir` under that one, the child holding
    /// it, so the room is this pane's alone. Said; nothing that way is said too.
    fn size_pane(&mut self, dir: crate::split::Dir, more: bool) {
        use crate::split::{Dir, Share, Split};
        // (a group's member: its host's leaf — the slot's, or its own docked)
        let host = |p: crate::split::PaneId| {
            self.workspace.groups.iter().find(|g| g.member(p).is_some()).map_or(p, |g| g.host)
        };
        let Some(id) = self.at_pane().map(host) else { return };
        let way = match dir {
            Dir::Vertical => "beside it — C-w v splits it",
            Dir::Horizontal => "over or under it — C-w s splits it",
        };
        if self.zoomed().is_some() {
            return self.set_status("it fills the screen — C-w z gives the others back to size it against");
        }
        let tree = &self.workspace.split;
        let Some(path) = tree.path(id) else { return };
        let edge = |after: bool| {
            (0..path.len()).rev().find_map(|depth| {
                let Some(Split::Node(d, kids)) = tree.at(&path[..depth]) else { return None };
                let i = path[depth];
                let mut others: Vec<usize> =
                    if after { (i + 1..kids.len()).collect() } else { (0..i).rev().collect() };
                others.retain(|&j| self.laid_along(&kids[j].1, dir) > 0);
                (*d == dir).then(|| others.first().map(|&j| (depth, i, j))).flatten()
            })
        };
        let Some((depth, i, j)) = edge(true).or_else(|| edge(false)) else {
            return self.set_status(format!("nothing {way}"));
        };
        let Some(Split::Node(_, kids)) = tree.at(&path[..depth]) else { return };
        let ((mine, kid), (theirs, other)) = (&kids[i], &kids[j]);
        let (now_i, now_j) = (self.laid_along(kid, dir), self.laid_along(other, dir));
        let room = (now_i + now_j).saturating_sub(Self::pane_min(dir));
        let (lim_i, lim_j) = (self.size_limits(kid, dir, room), self.size_limits(other, dir, room));
        // (from the size you set, as much of it as this screen shows — two presses before a frame count twice —, else
        // from what's on screen)
        let from = |s: &Share, now: usize, (min, max): (usize, usize)| match s {
            Share::Cells(k) => (*k as usize).clamp(min, max),
            _ => now,
        };
        let (from_i, from_j) = (from(mine, now_i, lim_i), from(theirs, now_j, lim_j));
        let step = if dir == Dir::Vertical { STEP_COLS } else { STEP_ROWS };
        let grown = |n: usize, by_more: bool| if by_more { n + step } else { n.saturating_sub(step) };
        let own = |k: &Split| self.workspace.own_sized(k);
        let set = |s: &Share| own(other) || matches!(s, Share::Cells(_));
        // (which side's share holds it: this pane's own where it's a surface, else the one across where that's its own
        // size, else this side's child)
        let (n_i, n_j) = match (own(kid), set(theirs)) {
            (false, true) => (None, Some(grown(from_j, !more).clamp(lim_j.0, lim_j.1))),
            (true, _) | (false, false) => {
                let n = grown(from_i, more).clamp(lim_i.0, lim_i.1);
                let back = |m: usize| (from_j + from_i).saturating_sub(m).clamp(lim_j.0, lim_j.1);
                (Some(n), set(theirs).then(|| back(n)))
            }
        };
        // (what it changes from what's laid out — two presses before a frame, both)
        let delta = match (n_i, n_j) {
            (Some(n), _) => n as isize - now_i as isize,
            (None, Some(m)) => now_j as isize - m as isize,
            _ => 0,
        };
        // (each split along `dir` under that one: the child holding this pane its size and the room)
        let inner: Vec<(usize, usize)> = (depth + 1..path.len())
            .filter_map(|d| match tree.at(&path[..d]) {
                Some(Split::Node(nd, ks)) if *nd == dir => {
                    let now = self.laid_along(&ks[path[d]].1, dir) as isize;
                    Some((d, (now + delta).max(Self::pane_min(dir) as isize) as usize))
                }
                _ => None,
            })
            .collect();
        let said = match tree.at(&path).map(|leaf| self.laid_along(leaf, dir)) {
            _ if depth + 1 == path.len() && n_i.is_some() => n_i.unwrap_or(0),
            Some(now) => (now as isize + delta).max(0) as usize,
            None => 0,
        };
        let what = size_name(self.workspace.shows(id));
        let split = &mut self.workspace.split;
        if let Some(Split::Node(_, kids)) = split.at_mut(&path[..depth]) {
            if let Some(n) = n_i {
                kids[i].0 = Share::Cells(n as u16);
            }
            if let Some(m) = n_j {
                kids[j].0 = Share::Cells(m as u16);
            }
        }
        if delta != 0 {
            for (d, n) in inner {
                if let Some(Split::Node(_, ks)) = split.at_mut(&path[..d]) {
                    ks[path[d]].0 = Share::Cells(n as u16);
                }
            }
        }
        self.say_size(format!("{what} {said} {}", unit(dir)));
    }

    /// A border dragged to (`x`, `y`): the size it makes, within what this screen allows (kept when the
    /// press is let go — `border_done`).
    pub fn border_drag(&mut self, b: Border, x: usize, y: usize) {
        if self.screen.width.is_none() {
            return;
        }
        let s = &self.screen;
        // (nothing said while it moves — the edge, in the accent, is there to see; letting go says it — each kind of leaf
        // sized by its own rule)
        use crate::pane::Shows;
        match (b, self.border_sizes(b)) {
            // (a surface's own line, on whichever side of it it is — the far side of it stays; along its split's way, its
            // size there: the stacked chat's top line its height, the slot's line beside the code its width)
            (
                Border::Divider { sized, dir, .. },
                Some(Shows::Chat | Shows::Side | Shows::Rail | Shows::Slot),
            ) => {
                // (the slot's members laid in its place: its rect as drawn)
                let r = match s.leaves.iter().find(|l| l.0 == sized) {
                    Some(l) => l.1,
                    None if self.workspace.shows(sized) == Some(Shows::Slot) => s.slot(),
                    None => return,
                };
                let (at, from, len) = match dir {
                    crate::split::Dir::Vertical => (x, r.x, r.w),
                    crate::split::Dir::Horizontal => (y, r.y, r.h),
                };
                // (its line nearest the pointer: the one held follows it)
                let line = (s.edges.lines.iter())
                    .filter(|l| l.dir == dir && l.sized == sized && l.heads == sized)
                    .min_by_key(|l| l.at.abs_diff(at));
                let first =
                    line.is_none_or(|l| l.at.abs_diff(from) <= l.at.abs_diff(from + len.saturating_sub(1)));
                let n = if first { (from + len).saturating_sub(at) } else { (at + 1).saturating_sub(from) };
                let leaf = crate::split::Split::Leaf(sized);
                let (min, max) = self.size_limits(&leaf, dir, self.size_room(sized, dir).unwrap_or(n));
                let n = crate::split::Share::Cells(n.clamp(min, max) as u16);
                // (the slot's width beside the code: its column's — a code pane over or under it there too)
                match (self.workspace.shows(sized), self.workspace.split.parent_along(sized, dir)) {
                    (Some(Shows::Slot), Some((kids, i))) if dir == crate::split::Dir::Vertical => {
                        kids[i].0 = n
                    }
                    _ => {
                        self.workspace.split.set_share(sized, n);
                    }
                }
            }
            // (a docked group's edge: the far side of it stays)
            (_, Some(Shows::Group(kind))) => {
                if let Some((side, r)) = self.dock_rect(kind) {
                    let n = match side {
                        crate::pane::Side::Right => (r.x + r.w).saturating_sub(x),
                        crate::pane::Side::Left => (x + 1).saturating_sub(r.x),
                        // (its edge the code's line under its last row)
                        crate::pane::Side::Top => y.saturating_sub(r.y),
                    };
                    self.dock_size(kind, n);
                }
            }
            // (the divider at column `x` — over and under, row `y`: the pane before it's child ends there)
            (Border::Divider { sized, dir, .. }, Some(Shows::Code)) => {
                let Some((_, now, room)) = self.own_split(sized, dir) else { return };
                let Some(r) = s.leaves.iter().find(|l| l.0 == sized).map(|l| l.1) else { return };
                let (end, at) = match dir {
                    crate::split::Dir::Vertical => (r.x + r.w, x),
                    crate::split::Dir::Horizontal => (r.y + r.h, y),
                };
                // (the child's first cell: its size now back from the pane's far edge)
                let start = end.saturating_sub(now);
                self.pane_size(sized, dir, at.saturating_sub(start), room);
            }
            // (its right edge and its other line stay: the width from its right, the height from its far line)
            (Border::Follow { cols, rows }, _) => {
                let Some((cx, cy, cw, ch)) = s.thought_card.get() else { return };
                let (foot, (max_w, max_h)) = (s.follow_at_foot.get(), s.follow_room.get());
                if cols {
                    let w = (cx + cw).saturating_sub(x).clamp(FOLLOW_MIN_W, max_w.max(FOLLOW_MIN_W));
                    self.sizes.follow_w = Some(w as u16);
                }
                if rows {
                    // (at the foot, from where its whole height ends: it grows down to there, #310)
                    let h = match foot {
                        true => s.follow_foot.get().max(cy + ch).saturating_sub(y),
                        false => (y + 1).saturating_sub(cy),
                    };
                    self.sizes.follow_h = Some(h.clamp(FOLLOW_MIN_H, max_h.max(FOLLOW_MIN_H)) as u16);
                }
            }
            // (a leaf no rule sizes: the line stays)
            (Border::Divider { .. }, _) => {}
        }
    }

    /// What surface leaf `id`'s split along `dir` holds as last laid out, less a code pane's least: the most a drag of
    /// its line gives it — the nearest split that way around it (the slot's column beside the code: the one it sits in
    /// side by side, not the code pane's over it).
    fn size_room(&self, id: crate::split::PaneId, dir: crate::split::Dir) -> Option<usize> {
        use crate::split::Split;
        let tree = &self.workspace.split;
        let path = tree.path(id)?;
        let along = (0..path.len()).rev().find_map(|d| match tree.at(&path[..d]) {
            Some(node @ Split::Node(nd, _)) if *nd == dir => Some(node),
            _ => None,
        });
        let parent = along.or_else(|| tree.at(&path[..path.len().checked_sub(1)?]))?;
        Some(self.laid_along(parent, dir).saturating_sub(Self::pane_min(dir)))
    }

    /// The press let go: what it set is kept, and said.
    pub fn border_done(&mut self, b: Border) {
        self.sizes_keep();
        use crate::pane::Shows;
        let (what, n, unit) = match (b, self.border_sizes(b)) {
            (
                Border::Divider { sized, dir, .. },
                Some(s @ (Shows::Chat | Shows::Side | Shows::Rail | Shows::Slot)),
            ) => {
                let n = match self.workspace.split.share(sized) {
                    Some(crate::split::Share::Cells(n)) => Some(n),
                    _ => None,
                };
                (size_name(Some(s)), n, unit(dir))
            }
            (_, Some(Shows::Group(kind))) => {
                let unit = match self.workspace.group(kind).map(|g| g.place) {
                    Some(crate::pane::Place::Docked(crate::pane::Side::Top)) => "rows",
                    _ => "columns",
                };
                (kind.name(), self.sizes.docked[kind.dock_index()], unit)
            }
            (Border::Divider { sized, dir, .. }, Some(Shows::Code)) => {
                let mut tree = self.workspace.split.clone();
                let n = match tree.parent_along(sized, dir) {
                    Some((kids, i)) => match kids[i].0 {
                        crate::split::Share::Cells(n) => Some(n),
                        _ => None,
                    },
                    None => None,
                };
                let unit = if dir == crate::split::Dir::Vertical { "columns" } else { "rows" };
                ("pane", n, unit)
            }
            (Border::Follow { .. }, _) => return self.follow_size_said(),
            (Border::Divider { .. }, _) => return,
        };
        if let Some(n) = n {
            self.say_size(format!("{what} {n} {unit}"));
        }
    }

    /// The follow card has the keys (`Editor.follow_focus`) and is drawn: `C-w < > - +` and `C-w =` are its.
    pub fn follow_in_hand(&self) -> bool {
        self.follow_focus && self.screen.thought_card.get().is_some()
    }

    /// `C-w < > - +` with the follow card in hand: it `STEP_COLS` narrower or wider, `STEP_ROWS` shorter or taller — from
    /// the size you set (as much of it as it had room for), else the size it's drawn, within its room. False: not in
    /// hand.
    fn follow_size(&mut self, dir: crate::split::Dir, more: bool) -> bool {
        if !self.follow_in_hand() {
            return false;
        }
        let Some((_, y, w, h)) = self.screen.thought_card.get() else { return false };
        // (at the foot, its whole height: it grows down to there, #310)
        let h = match self.screen.follow_at_foot.get() {
            true => self.screen.follow_foot.get().max(y + h) - y,
            false => h,
        };
        let (max_w, max_h) = self.screen.follow_room.get();
        let step = |kept: Option<u16>, shown: usize, n: usize, (min, max): (usize, usize)| {
            let from = kept.map_or(shown, |k| (k as usize).min(max));
            let to = if more { from + n } else { from.saturating_sub(n) };
            Some(to.clamp(min, max.max(min)) as u16)
        };
        match dir {
            crate::split::Dir::Vertical => {
                self.sizes.follow_w = step(self.sizes.follow_w, w, STEP_COLS, (FOLLOW_MIN_W, max_w));
            }
            crate::split::Dir::Horizontal => {
                self.sizes.follow_h = step(self.sizes.follow_h, h, STEP_ROWS, (FOLLOW_MIN_H, max_h));
            }
        }
        self.sizes_keep();
        self.follow_size_said();
        true
    }

    /// The follow card's size, said (`say_size`): what you set, else what it's drawn.
    fn follow_size_said(&mut self) {
        let (_, _, w, h) = self.screen.thought_card.get().unwrap_or_default();
        let w = self.sizes.follow_w.map_or(w, usize::from);
        let h = self.sizes.follow_h.map_or(h, usize::from);
        self.say_size(format!("follow card {w} × {h}"));
    }

    /// A size said in a toast — one at a time: the one before it goes (a few presses of `C-w >` are one
    /// message that changes, not a stack of them).
    pub(crate) fn say_size(&mut self, msg: String) {
        self.toasts.retain(|t| !t.text.ends_with(SIZE_TAIL));
        self.set_status(format!("{msg}{SIZE_TAIL}"));
    }

    /// `C-w >` / `C-w <` — the pane in hand wider / narrower (D7): the follow card in hand, a floating pane, a docked
    /// group, else the pane in the tree (`size_pane`). Kept with the workspace's layout — `C-w =` automatic again.
    pub fn size_width(&mut self, wider: bool) {
        self.size_key(crate::split::Dir::Vertical, wider);
    }

    /// `C-w +` / `C-w -` — the pane in hand taller / shorter, as `size_width`.
    pub fn size_height(&mut self, taller: bool) {
        self.size_key(crate::split::Dir::Horizontal, taller);
    }

    fn size_key(&mut self, dir: crate::split::Dir, more: bool) {
        if self.follow_size(dir, more) || self.float_size(dir, more) || self.size_dock(dir, more) {
            return;
        }
        self.size_pane(dir, more);
    }

    /// The side panel is open but folded away: on this screen (as last drawn) it would leave the code too
    /// little (`widths`). Not before the first frame, not while a review's rail holds its place.
    pub fn side_folded(&self) -> bool {
        let Some(total) = self.screen.width else { return false };
        let chat_open = self.chat_panel(total);
        let chat = chat_open.then_some(self.workspace.kept(Shows::Chat));
        self.workspace.viewer.sidebar.is_some()
            && !self.review_rail_up()
            && give_way(Wants {
                total,
                chat,
                nav: Nav::Side(self.workspace.kept(Shows::Side)),
                slot: self.slot_beside(),
            })
            .nav == 0
    }

    /// The chat is a panel on a screen `total` wide: open, not minimized — and where it would take the whole
    /// width (too narrow for it beside the code), only while it has the keys. Out of the chat (a card
    /// answered, Esc, an edit's review) they went to a code pane with no room on screen: `keys → pane#29`,
    /// typing landing unseen. What has the keys fills a narrow screen, the code too; `space l` brings the chat.
    /// A card that took them from the chat (`C-q`) leaves it there: the asks float over its foot (P3 step 7d), so
    /// the card stays where it waited — swapping in the code under it left a 57-column card, drawn while it waited,
    /// with no room once focused, every key refused (the review of cardkeys).
    pub fn chat_panel(&self, total: usize) -> bool {
        if !self.workspace.claude.chat.as_ref().is_some_and(|c| !c.hidden) || self.chat_floating() {
            return false;
        }
        self.workspace.is_at(Here::Chat)
            || give_way(Wants {
                total,
                chat: Some(self.workspace.kept(Shows::Chat)),
                nav: Nav::None,
                slot: None,
            })
            .chat
                < total
    }

    /// The chat floats over the code this frame: shown as the ask line (`Chat.asking`, the flow plan's piece 2) — the one
    /// form it floats in; the conversation is the docked column (the user, 2026-11-03: no floating reader). The layout
    /// lays its frame by it.
    pub fn chat_floating(&self) -> bool {
        self.workspace.claude.chat.as_ref().is_some_and(|c| !c.hidden && c.asking)
    }

    /// The chat is the ask line this frame, its input with the keys — a prompt, never drawn without them (`focus_sync`
    /// closes it once they leave).
    pub fn chat_asking(&self) -> bool {
        self.chat_floating() && self.workspace.is_at(Here::Chat)
    }

    /// The ask line's columns on a screen `w` wide with `rows` above the status line: its frame's top and height
    /// (`Frame::rect`), as wide as the chat's prose measure needs (`CHAT_FLOAT_W`) and centred across. A screen narrower
    /// than that: its whole width.
    pub fn chat_float_rect(&self, w: usize, rows: usize) -> crate::split::Rect {
        let r = crate::pane::Frame::CENTRED.rect(w, rows);
        let fw = if w >= CHAT_FLOAT_W + 2 { CHAT_FLOAT_W } else { w };
        crate::split::Rect { x: (w - fw) / 2, w: fw, ..r }
    }

    /// Whether `C-w =` changes a size the screen is laid out by — a share you set in this workspace's tree, a docked
    /// group's size (#640: `C-q`'s row on a card with no room offers it only where it makes room, `term::card_no_room`).
    pub fn sizes_set(&self) -> bool {
        let s = self.sizes;
        let auto = Sizes {
            sessions: s.sessions,
            messages: s.messages,
            folders: s.folders,
            sessions_dock: s.sessions_dock,
            ..Sizes::default()
        };
        let mut split = self.workspace.split.clone();
        split.even(&|k| self.workspace.own_sized(k));
        s != auto || split != self.workspace.split
    }

    /// `C-w =` — every size automatic again, and how each group lays its members (`C-w t`'s).
    pub fn sizes_auto(&mut self) {
        // (from a floating pane: it alone, back in the middle)
        if self.float_auto() {
            return;
        }
        // (the follow card in hand: it alone, automatic again — #267)
        if self.follow_in_hand() {
            (self.sizes.follow_w, self.sizes.follow_h) = (None, None);
            self.sizes_keep();
            self.toasts.retain(|t| !t.text.ends_with(SIZE_TAIL));
            return self.set_status("follow card automatic again");
        }
        // (where the lists are is no size: their frames and the session list's side stay)
        let s = self.sizes;
        self.sizes = Sizes {
            sessions: s.sessions,
            messages: s.messages,
            folders: s.folders,
            sessions_dock: s.sessions_dock,
            ..Sizes::default()
        };
        // (the shares you set: a code pane's even again, a surface's its own — `Split::even`)
        let mut split = self.workspace.split.clone();
        split.even(&|k| self.workspace.own_sized(k));
        self.workspace.split = split;
        for g in &mut self.workspace.groups {
            g.laid = crate::pane::Laid::Auto;
        }
        self.sizes_keep();
        self.toasts.retain(|t| !t.text.ends_with(SIZE_TAIL));
        self.set_status("sizes automatic again");
    }

    /// The run panel in hand: a docked group's while one of its members has the keys (`slot_in_hand`), else what
    /// the bottom slot shows (`slot_shown`) — what `At::Slot`'s keys act on.
    pub fn slot(&self) -> Option<Slot> {
        self.slot_in_hand().or_else(|| self.slot_shown())
    }

    /// A docked group's panel while its list or its detail has the keys (P3 §3.3: where a group is doesn't change
    /// what its keys do — `At::List`, `At::Slot` as in the slot).
    pub fn slot_in_hand(&self) -> Option<Slot> {
        let kind = self.workspace.viewer.hand?;
        let g = self.workspace.group(kind)?;
        let keys = self.workspace.is_at(Here::Slot) || self.workspace.is_at(Here::List(g.list));
        keys.then_some(()).filter(|_| self.out_of_slot(kind)).and(kind.slot())
    }

    /// What the bottom slot shows, if anything: the panel put in front (`chrome.rs` — a tab, one just
    /// opened), else the first there of `:procs`, the terminal, a plugin's panel, the test run
    /// or task started last — never one whose group is docked in sight (P3 §3.1: it's a pane of its own).
    pub fn slot_shown(&self) -> Option<Slot> {
        self.slot_shown_by(|s| !self.slot_docked(*s))
    }

    /// `slot_shown`, `free` saying which panels the slot keeps (not out of it).
    fn slot_shown_by(&self, free: impl Fn(&Slot) -> bool) -> Option<Slot> {
        if let Some(s) = self.slot_front().filter(&free) {
            return Some(s);
        }
        if self.procs_up() && free(&Slot::Procs) {
            return Some(Slot::Procs);
        }
        if self.workspace.terminal.as_ref().is_some_and(|t| t.visible) {
            return Some(Slot::Terminal);
        }
        if self.plugins.as_ref().is_some_and(|p| p.panel.is_some()) {
            return Some(Slot::Plugin);
        }
        let test = self.workspace.test_run.as_ref().filter(|_| free(&Slot::Test));
        let task = self.workspace.task_run.as_ref().filter(|_| free(&Slot::Task));
        match (test, task) {
            (Some(t), Some(k)) if k.started > t.started => Some(Slot::Task),
            (Some(_), _) => Some(Slot::Test),
            (None, Some(_)) => Some(Slot::Task),
            (None, None) => None,
        }
    }

    /// Whether the bottom slot has the keys — tarae's (`At::Slot`), or the terminal's program, or a
    /// plugin panel's.
    pub fn bottom_has_keys(&self) -> bool {
        // (a docked group's are its own pane's, beside the code)
        if self.slot_in_hand().is_some() {
            return false;
        }
        // (a group's list in the slot is the slot's: the tests', a task's places, a preflight's steps, `:procs`')
        let listing = || self.slot_group().is_some_and(|g| self.workspace.is_at(Here::List(g.list)));
        match self.slot() {
            Some(Slot::Terminal) => self.workspace.is_at(Here::Slot) || self.workspace.is_at(Here::Program),
            Some(Slot::Plugin) => self.plugin_panel_focused(),
            Some(Slot::Procs | Slot::Test | Slot::Task) => self.workspace.is_at(Here::Slot) || listing(),
            None => false,
        }
    }

    /// `C-w j` below the lowest pane, or a click on the panel: the keys to the bottom slot (on the terminal,
    /// tarae's — `i` then types in it).
    pub fn bottom_take_keys(&mut self) {
        self.workspace.viewer.hand = self.hand_of(self.slot_shown());
        match self.slot_shown() {
            None => self.set_status("nothing at the bottom — tests, tasks and the terminal show there"),
            Some(Slot::Plugin) => self.plugin_panel_click(None),
            Some(Slot::Procs) => self.procs_click(None),
            Some(_) => {
                self.workspace.set_at(Here::Slot);
                // (the terminal's: its view mode, at once — what has the keys is said before the next key; a run
                // panel's output, its cursor landed in sight)
                crate::view::sync(self);
                crate::rundoc::sync(self);
            }
        }
    }

    /// What has the keys, as `C-w z` would fill the screen with it — None: the review's file list (it keeps
    /// its width).
    pub fn focused_area(&self) -> Option<Zoom> {
        let s = &self.workspace;
        let shows = if s.is_at(Here::Chat) && s.claude.chat.as_ref().is_some_and(|c| !c.hidden) {
            Shows::Chat
        } else if self.review_rail_up() && s.is_at(Here::Rail) && s.code_review.is_some() {
            return None;
        } else if !self.review_rail_up() && s.is_at(Here::Side) && s.viewer.sidebar.is_some() {
            Shows::Side
        } else if let Some(g) = self.slot_in_hand().and(s.viewer.hand).and_then(|k| s.group(k)) {
            return Some(Zoom(g.host));
        } else if let Some(p) = self.float_list_in_hand().and_then(|k| self.float_list(k)) {
            return Some(Zoom(p.list));
        } else if self.bottom_has_keys() {
            Shows::Slot
        } else {
            Shows::Code
        };
        s.leaf(shows).map(Zoom)
    }

    /// What fills the screen now: `C-w z`'s, while it still has the keys.
    pub fn zoomed(&self) -> Option<Zoom> {
        self.workspace.viewer.zoom.filter(|z| Some(*z) == self.focused_area())
    }

    /// What the pane filling the screen shows (`zoomed`'s).
    pub fn zoomed_shows(&self) -> Option<crate::pane::Shows> {
        self.zoomed().and_then(|z| self.workspace.shows(z.0))
    }

    /// `C-w z` — what has the keys fills the screen; again, back as it was.
    pub fn zoom_toggle(&mut self) {
        if self.zoomed().is_some() {
            self.workspace.viewer.zoom = None;
            return;
        }
        let Some(z) = self.focused_area() else {
            return self.set_status("the review's file list keeps its width — C-w z from the code");
        };
        let s = &self.screen;
        let alone = self.workspace.views.len() == 1
            && s.drawn_w(Shows::Chat) == 0
            && s.drawn_w(Shows::Side) == 0
            && s.slot().h == 0
            && !(self.workspace.code_review.is_some() && self.review_rail_up());
        if self.workspace.shows(z.0) == Some(crate::pane::Shows::Code) && alone {
            return self.set_status("the code has the whole screen already");
        }
        self.workspace.viewer.zoom = Some(z);
    }

    /// Every key and event: the keys are still where `Viewer.at` says — what it names is still there (the
    /// chat open, the terminal shown and running, the slot showing a panel that takes them), else they're the
    /// code's; `C-w z` ends once what it filled the screen with doesn't have them.
    pub fn focus_sync(&mut self) {
        self.slot_sync();
        self.groups_sync();
        self.dock_sync();
        // (a floating group's own pane, out of the tree; the one in hand on top; `C-w m` ends with its pane)
        self.float_groups_sync();
        self.float_raise();
        // (a docked group folded into the slot puts its panel in front first, while the keys are still in it — judged
        // against an older front, its list or output wasn't there and they fell to the code)
        self.hand_sync();
        // (the keys on a code pane name the one focused — a pane gone, the code's)
        self.workspace.code_at_sync();
        let s = &self.workspace;
        let there = match s.docked() {
            Here::Code | Here::Float(_) => true,
            // (hidden, it holds no keys — `space L`, #270: they're the code's, wherever they were lent from)
            Here::Chat => s.claude.chat.as_ref().is_some_and(|c| !c.hidden),
            Here::Program => s.terminal.as_ref().is_some_and(|t| t.visible && t.exited.is_none()),
            Here::Slot => matches!(self.slot(), Some(Slot::Terminal | Slot::Test | Slot::Task | Slot::Procs)),
            Here::List(id) => {
                s.groups.iter().any(|g| g.list == id && self.group_laid(g))
                    || self.float_list_of(id).is_some()
            }
            Here::Plugin => self.plugins.as_ref().is_some_and(|p| p.panel.is_some()),
            Here::Side => s.viewer.sidebar.is_some(),
            Here::Rail => s.code_review.is_some(),
        };
        if !there {
            self.workspace.leave(self.workspace.docked());
            // (the keys gone from its group: the hand let go)
            self.hand_sync();
        }
        // (the ask line is a prompt: once the keys left it — a hop, a press elsewhere, a picker's pick — it goes, its draft
        // kept for `space l`)
        let away = !self.workspace.is_at(Here::Chat) && self.chat_floating();
        if let Some(c) = self.workspace.claude.chat.as_mut().filter(|c| c.asking && away) {
            c.hidden = true;
        }
        // (`C-w m` ends with its pane, or once the keys leave it — a hop, a click elsewhere)
        self.float_move_sync();
        self.groups_front();
        // `:procs`' panel gives the slot to the terminal given the keys (`C-\`), which it covered
        if self.procs_up() && self.workspace.program_has_keys() {
            self.procs_close();
        }
        // (a view whose surface lost the keys ends; the terminal given tarae's keys is in its view mode; the run
        // panels' documents follow their runs, the keys coming or going taken in)
        crate::view::sync(self);
        crate::rundoc::sync(self);
        if self.workspace.viewer.zoom.is_some() && self.zoomed().is_none() {
            self.workspace.viewer.zoom = None;
        }
        // (a card, the picker, a prompt, which-key: the keys' while up, the docked pane's again once gone)
        self.float_sync();
        // (the edit review's focus follows the keys: lost as they leave the code, folded as they come onto it — last,
        // once a floating pane gone has given them back)
        crate::llm::review_keys_sync(self);
    }

    /// Writes the sizes (worker — the newest write wins).
    pub fn sizes_keep(&mut self) {
        let Some(path) = self.sizes_path.clone() else { return };
        self.sizes_seq += 1;
        let (sizes, seq) = (self.sizes, self.sizes_seq);
        self.events.jobs().background(move || save(&path, sizes, seq));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Panes P2a: where the keys are is one value (`Viewer.at`) — once what it names is gone (the chat closed,
    /// the side panel closed, the terminal hidden, the slot showing nothing that takes them) the next event
    /// gives them to the code; a pane still there keeps them; `C-w z` ends with them.
    #[test]
    fn the_keys_go_to_the_code_once_what_had_them_is_gone() {
        use crate::workspace::Here;
        let dir = crate::scratch::Scratch::new("keys-gone");
        let mut ed = crate::editor::Editor::new(crate::config::Config::default());
        ed.workspace.root = dir.path().to_path_buf();
        use crate::key::{Code, Key};
        let key = |ed: &mut crate::editor::Editor| ed.handle_key(Key::plain(Code::Char('l')));
        ed.workspace.claude.chat = Some(crate::chat::Chat::new());
        ed.workspace.set_at(Here::Chat);
        ed.focus_sync();
        assert_eq!(ed.workspace.here(), Here::Chat);
        ed.workspace.claude.chat = None;
        ed.focus_sync();
        assert_eq!(ed.workspace.here(), Here::Code, "the chat closed");
        ed.sidebar_show(crate::sidebar::Mode::Files);
        assert_eq!(ed.workspace.here(), Here::Side);
        key(&mut ed);
        assert_eq!(ed.workspace.here(), Here::Side, "a pane still there keeps them");
        ed.workspace.viewer.sidebar = None;
        key(&mut ed);
        assert_eq!(ed.workspace.here(), Here::Code, "the side panel closed");
        let _sent = crate::terminal::stand_in(&mut ed, 10, 80, 100);
        ed.workspace.viewer.zoom = ed.focused_area();
        assert_eq!((ed.workspace.here(), ed.zoomed_shows()), (Here::Program, Some(crate::pane::Shows::Slot)));
        ed.workspace.terminal.as_mut().unwrap().visible = false;
        ed.focus_sync();
        assert_eq!(
            (ed.workspace.here(), ed.workspace.viewer.zoom),
            (Here::Code, None),
            "hidden: the code's, the zoom over"
        );
        ed.workspace.set_at(Here::Slot);
        ed.focus_sync();
        assert_eq!(ed.workspace.here(), Here::Code, "nothing in the slot takes them");
    }

    /// cardburden 4b: `K` in the task panel's view mode says where Claude's command in it ran and why — its
    /// run's record, as a click on its chat row's mark does; its hint on the panel's header. A run that isn't
    /// Claude's says so. (The run is the test's own `sleep`, stopped at the end.)
    #[test]
    fn k_in_the_task_panel_says_where_claudes_command_ran() {
        let scratch = crate::scratch::Scratch::new("k-ran");
        let dir = std::fs::canonicalize(scratch.path()).unwrap();
        let mut ed = crate::editor::Editor::new(crate::config::Config::default());
        ed.workspace.root = dir.clone();
        let task = crate::task::Task { name: "t".into(), cmd: "sleep 30".into(), cwd: dir.clone() };
        assert!(
            ed.task_start_for(task, crate::tools::For::User("t".into()), None, Default::default()).is_ok()
        );
        ed.bottom_take_keys();
        let k: crate::key::Key = "K".parse().unwrap();
        let said = |ed: &crate::editor::Editor| ed.messages.last().map(|m| m.2.clone()).unwrap_or_default();
        ed.handle_key(k);
        assert_eq!(crate::uistate::TOOK.get(), Some(crate::uistate::Handler::Output));
        assert!(said(&ed).contains("this run isn't one"), "{}", said(&ed));
        let r = ed.workspace.task_run.as_mut().unwrap();
        r.by_claude = true;
        r.ran = Some(crate::chat::Ran::of(&crate::sandbox::Guard::Outside, Some(7), None, Vec::new()));
        let mut bytes = Vec::new();
        crate::term::render(&mut ed, &mut bytes, 140, 30).unwrap();
        let mut parser = vt100::Parser::new(30, 140, 0);
        parser.process(&bytes);
        assert!(parser.screen().contents().contains("K where it ran"), "its hint");
        ed.handle_key(k);
        assert_eq!(
            said(&ed),
            "sleep 30 — ran outside\nOutside the sandbox, with all your permissions — said yes to. From card#7."
        );
        ed.workspace.task_run = None;
    }

    /// Automatic: the chat 2/5 up to 96 (U3); at 140 with the side panel, the chat narrows so the code keeps
    /// 60; narrower still, the panel gives way; a narrow screen is all chat.
    #[test]
    fn automatic_widths() {
        let given = |total, chat, nav| give_way(Wants { total, chat, nav, slot: None });
        let (chat, side) = (Some(None), Nav::Side(None));
        assert_eq!(given(140, chat, Nav::None), Given { chat: 56, nav: 0, slot: 0 });
        assert_eq!(given(200, chat, Nav::None), Given { chat: 80, nav: 0, slot: 0 });
        assert_eq!(given(300, chat, Nav::None), Given { chat: 96, nav: 0, slot: 0 }, "the cap: 96");
        // 140, both: the chat gives 4 columns so the code keeps 60 (it had 52)
        let w = given(140, chat, side);
        assert_eq!(w, Given { chat: 48, nav: 32, slot: 0 });
        assert_eq!(140 - w.chat - w.nav, CODE_ROOM);
        // 120: the chat at its least, the code 52 — the panel stays (48 is its line)
        assert_eq!(given(120, chat, side), Given { chat: 36, nav: 32, slot: 0 });
        // 110: the code would have 42 — the panel gives way
        assert_eq!(given(110, chat, side), Given { chat: 44, nav: 0, slot: 0 });
        assert_eq!(
            given(70, chat, side),
            Given { chat: 70, nav: 0, slot: 0 },
            "narrow: the chat takes it all"
        );
        assert_eq!(given(200, None, side), Given { chat: 0, nav: 32 + 12, slot: 0 });
    }

    /// frictions25 R6: beside the review's rail (36 columns here) an automatic chat narrows to leave the code a
    /// review's room (100), down to its least — at 160 the code had 60 beside a 64-column chat; on a wide
    /// screen it's the chat's 2/5 again; a width you set, or a chat taking the whole width, stays.
    #[test]
    fn beside_the_review_rail_the_chat_leaves_the_code_its_room() {
        let rail = Nav::Rail { natural: 36, focus: false };
        let chat = |total, kept| give_way(Wants { total, chat: Some(kept), nav: rail, slot: None }).chat;
        let alone = give_way(Wants { total: 160, chat: Some(None), nav: Nav::None, slot: None }).chat;
        assert_eq!(alone, 64, "as it was beside the rail");
        assert_eq!(chat(160, None), CHAT_MIN, "160: the code 88");
        assert_eq!(chat(140, None), CHAT_MIN);
        assert_eq!(chat(200, None), 64, "200: the code keeps 100");
        assert_eq!(chat(260, None), CHAT_AUTO_MAX, "wide: no wider than without the rail");
        assert_eq!(chat(160, Some(70)), 70, "yours stays");
        assert_eq!(chat(70, None), 70, "narrow: the chat takes it all, as without the rail");
        // (the rail itself: what its rows need beside the code's 60 — none where the code can't keep its room)
        assert_eq!(give_way(Wants { total: 160, chat: Some(None), nav: rail, slot: None }).nav, 36);
        assert_eq!(give_way(Wants { total: 70, chat: Some(None), nav: rail, slot: None }).nav, 0);
        let rail = |focus| Nav::Rail { natural: 36, focus };
        let narrow = |focus| give_way(Wants { total: 80, chat: None, nav: rail(focus), slot: None });
        assert_eq!(narrow(false).nav, 0, "80: under 90, not drawn");
        assert_eq!(narrow(true).nav, 32, "with the keys: in the side panel's room, the code 48");
    }

    /// A kept size is shown as it is where it fits, clamped where it doesn't — the kept value itself isn't
    /// touched (it's the caller's, unchanged), so a bigger screen gets it whole again.
    #[test]
    fn kept_widths_are_clamped_not_changed() {
        let kept = Wants { total: 200, chat: Some(Some(90)), nav: Nav::Side(Some(40)), slot: None };
        assert_eq!(give_way(kept), Given { chat: 90, nav: 40, slot: 0 });
        // 150: the chat keeps its 90; the panel's 40 would leave the code 20 — it gives way
        assert_eq!(give_way(Wants { total: 150, ..kept }), Given { chat: 90, nav: 0, slot: 0 });
        // 120: the chat as wide as leaves the code 40
        assert_eq!(
            give_way(Wants { total: 120, nav: Nav::None, ..kept }),
            Given { chat: 80, nav: 0, slot: 0 }
        );
        assert_eq!(kept.chat, Some(Some(90)), "kept as it was");
        assert_eq!(give_way(kept), Given { chat: 90, nav: 40, slot: 0 }, "the bigger screen: whole again");
        // Out of bounds kept values (an edited file): within them
        let odd = Wants { total: 240, chat: Some(Some(5)), nav: Nav::Side(Some(500)), slot: None };
        assert_eq!(give_way(odd), Given { chat: CHAT_MIN, nav: SIDE_MAX, slot: 0 });
    }

    /// Row 1: a group beside the code — automatic 2/5 leaving the code its room, yours leaving it `CODE_MIN`, at least
    /// its side's column; folded where it'd be under `DOCK_MIN` or squeeze the code.
    #[test]
    fn a_group_beside_the_code_gives_way_first() {
        assert_eq!(group_beside(200, 200, None, 0), Some(80));
        assert_eq!(group_beside(200, 100, None, 0), Some(40), "the code keeps 60");
        assert_eq!(group_beside(200, 100, Some(70), 0), Some(52), "yours: the code keeps 48");
        assert_eq!(group_beside(200, 120, None, 70), Some(70), "as wide as its column");
        assert_eq!(group_beside(200, 80, None, 0), None, "under DOCK_MIN: folded");
        assert_eq!(group_beside(200, 100, None, 60), None, "its column would squeeze the code");
    }

    #[test]
    fn bottom_rows() {
        assert_eq!(bottom(36, None, 9), 9, "automatic: what the panel wants");
        assert_eq!(bottom(36, Some(20), 9), 20, "yours wins");
        assert_eq!(bottom(20, Some(30), 9), 16, "clamped: the code keeps 4");
        assert_eq!(bottom(36, Some(1), 9), BOTTOM_MIN);
        assert_eq!(bottom(5, Some(10), 9), 0, "no room at all");
    }

    #[test]
    fn across_its_homes_direction() {
        assert_eq!(across(Across::Chat, 48, None), 16, "a third of the rows");
        assert_eq!(across(Across::Chat, 30, None), CHAT_ROWS_MIN, "at least 12");
        assert_eq!(across(Across::Chat, 8, None), 8, "never past the room");
        assert_eq!(across(Across::Nav { rows: 3 }, 48, None), 5, "its rows, its top line and foot");
        assert_eq!(across(Across::Nav { rows: 40 }, 48, None), 16, "up to a third");
        // (a size you set: clamped, never written back)
        assert_eq!(across(Across::Chat, 48, Some(20)), 20, "yours");
        assert_eq!(across(Across::Chat, 48, Some(4)), CHAT_ROWS_MIN, "at least its least");
        assert_eq!(across(Across::Chat, 48, Some(60)), 48 - BOTTOM_CODE, "the code keeps its rows");
    }

    /// Row 5 (onetree S5, stack right): the slot beside the code takes 2/5 of what the columns leave, as much as leaves
    /// the code `CODE_MIN`, down to `SIDE_MIN`; its least counts as the code's, so the side panel goes before the code
    /// is squeezed for it (at 120 the code had 17 columns beside a 35-column slot).
    #[test]
    fn the_slot_beside_the_code_leaves_it_its_room() {
        let to = |kept| Some(Beside { kept, chat: true, nav: true });
        let beside = |total, slot| give_way(Wants { total, chat: Some(None), nav: Nav::Side(None), slot });
        let code = |total, g: Given| total - g.chat - g.nav - g.slot;
        // 200: the columns as without it, the slot its 2/5 of the rest
        let g = beside(200, to(None));
        assert_eq!(g, Given { chat: 80, nav: 32, slot: 35 });
        assert_eq!(code(200, g), 53);
        // 120: the side panel gives way (beside the chat and the slot's least the code would keep 32), the chat as
        // without it, the slot as much as leaves the code its least
        let g = beside(120, to(None));
        assert_eq!(g, Given { chat: 48, nav: 0, slot: 24 });
        assert_eq!(code(120, g), CODE_MIN);
        assert_eq!(beside(120, None).nav, 32, "under the code, the panel stays");
        // 80: no room for the code's least beside the chat at all — the slot at its own, the code the rest
        let g = beside(80, to(None));
        assert_eq!(g, Given { chat: 36, nav: 0, slot: SIDE_MIN });
        assert_eq!(code(80, g), 24);
        // yours: as much as leaves the code its least, never under the slot's
        assert_eq!(beside(200, to(Some(70))).slot, 40);
        assert_eq!(beside(200, to(Some(30))).slot, 30);
        assert_eq!(beside(200, to(Some(5))).slot, SIDE_MIN);
        // the conversation stacked under everything (`C-w J`): its width isn't taken off the slot's room
        let under = Some(Beside { kept: Some(150), chat: false, nav: true });
        assert_eq!(beside(200, under).slot, 200 - 32 - CODE_MIN);
    }

    /// Kept across runs: read back as written; missing or odd files are automatic.
    #[test]
    fn kept_in_a_file() {
        let dir = crate::scratch::Scratch::new("sizes");
        let path = dir.join("sizes.json");
        assert_eq!(Sizes::read(Some(&path)), Sizes::default(), "missing: automatic");
        let s = Sizes {
            docked: [Some(30), None, None, None, None],
            sessions: None,
            messages: None,
            folders: None,
            sessions_dock: Some(crate::pane::Side::Right),
            follow_w: Some(60),
            follow_h: None,
        };
        save(&path, s, 1);
        assert_eq!(Sizes::read(Some(&path)), s);
        save(&path, Sizes::default(), 0);
        assert_eq!(Sizes::read(Some(&path)), s, "an older write that comes late is skipped");
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Sizes::read(Some(&path)), Sizes::default());
        std::fs::remove_dir_all(&dir).ok();
    }
}
