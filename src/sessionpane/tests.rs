use super::*;
use crate::key::{Code, Key};
use crate::workspace::{Doing, Here};
use crate::worktree::{Facts, Shown};

/// A session as the list says it, every column filled.
fn full(id: WorkspaceId, name: &str, doing: Doing) -> Shown {
    Shown {
        id,
        name: name.into(),
        parent: None,
        branch: Some(format!("{name}-branch")),
        facts: Some(Facts { ahead: 2, behind: 3, changed: 4 }),
        doing,
        front: id == WorkspaceId::HOME,
        usage: Some((12.0, 300 << 20)),
        rebasing: None,
        reloaded: 1,
        compacted: true,
        claude: Some(("● warm until 14:32".into(), "$0.42".into())),
        billing: crate::billing::Billing::Api,
        build: Some((2 << 20, true)),
        queue: None,
        step: String::new(),
        model: "opus · high".into(),
        colour: id.n as u8,
        overlap: vec![("main".into(), Some(0))],
        report: "I will switch it to the entry API so a repeat keeps the first strand".into(),
        context: Some(84),
        children: None,
        wakes: None,
    }
}

/// The pattern the list lights (`floatlist::lit`).
fn lit(ed: &Editor) -> Option<regex::Regex> {
    crate::floatlist::lit(ed, GroupKind::Sessions)
}

fn cols(laid: &(usize, Vec<(Col, usize)>)) -> Vec<Col> {
    laid.1.iter().map(|c| c.0).collect()
}

/// The manager's row (P3 step 7, §3.6; the one list L1): `▲` in a column of its own while it waits on you — who waits,
/// never what (polish landing 1: no count of its cards, the stack's peeks say them) —, its state with the step its turn
/// is at in the strip's inks, whom it overlaps (each name its colour), `↓N` behind, its context (warning past 80 %), its
/// model, its last report line, then the rest the list always said — empty cells left out; no `At` (the step says it).
#[test]
fn a_row_says_what_the_manager_needs() {
    let r = row(&full(WorkspaceId::here(1), "fix", Doing::Waiting));
    assert_eq!(r.cell(Col::Waits), [("▲".to_string(), Ink::Warn)]);
    assert_eq!(r.cell(Col::State), [("waits".to_string(), Ink::Warn)]);
    assert_eq!(r.cell(Col::Model), [("opus · high".to_string(), Ink::Faint)]);
    assert!(!r.line().contains("src/a.rs"), "no At: {}", r.line());
    // (the step its turn is at, in the now line's words: in warning while it waits, faint while it works)
    let asks = |doing| Shown {
        step: "▸ asks to run cargo test".into(),
        ..full(WorkspaceId::here(1), "fix", doing)
    };
    assert_eq!(
        row(&asks(Doing::Waiting)).cell(Col::State),
        [("▸ asks to run cargo test".to_string(), Ink::Warn)]
    );
    let edits = Shown {
        step: "● editing src/a.rs 12-15".into(),
        ..full(WorkspaceId::here(1), "fix", Doing::Working)
    };
    let r = row(&edits);
    assert_eq!(r.cell(Col::State), [("● editing src/a.rs 12-15".to_string(), Ink::Faint)]);
    assert!(r.cell(Col::Waits).is_empty() && r.working, "no ▲ while it works");
    assert_eq!(row(&asks(Doing::Done)).text(Col::State), "done", "done says done");
    assert!(!r.cells.iter().any(|c| c.1.iter().any(|p| p.0.contains("card"))), "no count of cards");
    assert_eq!(r.cell(Col::Overlap), [("≈ ".into(), Ink::Warn), ("main".into(), Ink::Participant(0))]);
    assert_eq!((r.text(Col::Behind), r.text(Col::Ahead)), ("↓3".into(), "↑2".into()));
    assert_eq!(r.cell(Col::Context), [("ctx ".into(), Ink::Faint), ("84%".into(), Ink::Warn)]);
    assert_eq!(r.text(Col::Report), "I will switch it to the entry API so a repeat keeps the first strand");
    for (doing, said, ink) in [
        (Doing::Working, "working", Ink::Faint),
        (Doing::Done, "done", Ink::Accent),
        (Doing::Queued { pos: 2 }, "queued 2nd", Ink::Faint),
        (Doing::Stuck { tool: "symbols", secs: 42 }, "◦ stuck? symbols 42s", Ink::Warn),
    ] {
        assert_eq!(
            row(&full(WorkspaceId::here(1), "fix", doing)).cell(Col::State),
            [(said.to_string(), ink)]
        );
    }
    let idle = Shown { overlap: Vec::new(), context: None, ..full(WorkspaceId::HOME, "main", Doing::Idle) };
    let r = row(&idle);
    assert!(
        r.cell(Col::State).is_empty() && r.cell(Col::Overlap).is_empty() && r.cell(Col::Waits).is_empty()
    );
    assert!(r.front, "the front's name bold");
    // (a rebase and the bring queue before the state)
    let rebasing =
        Shown { rebasing: Some("rebasing 1/2".into()), ..full(WorkspaceId::here(1), "fix", Doing::Working) };
    assert_eq!(row(&rebasing).text(Col::State), "rebasing 1/2 · working");
    // (a parent's cost: its children's in all after its own, faint — orchestra §6)
    let parent =
        Shown { children: Some((3, "$4.12".into())), ..full(WorkspaceId::HOME, "main", Doing::Working) };
    assert_eq!(
        row(&parent).cell(Col::Cost),
        [("$0.42".to_string(), Ink::Text), (" · 3 children $4.12".into(), Ink::Faint)]
    );
    let one = Shown {
        children: Some((1, String::new())),
        claude: None,
        ..full(WorkspaceId::HOME, "main", Doing::Working)
    };
    assert_eq!(row(&one).cell(Col::Cost), [("1 child".to_string(), Ink::Faint)]);
    // (a child's turn that wakes its parent as it ends: in the state's column, faint — alone when the state says none)
    let waking = Shown { wakes: Some("main".into()), ..full(WorkspaceId::here(1), "fix", Doing::Working) };
    assert_eq!(
        row(&waking).cell(Col::State),
        [("working".to_string(), Ink::Faint), (" · wakes main".into(), Ink::Faint)]
    );
    let idle = Shown { wakes: Some("main".into()), ..full(WorkspaceId::here(1), "fix", Doing::Idle) };
    assert_eq!(row(&idle).text(Col::State), "wakes main");
}

/// The list's width at 160 (the frame 96 wide) and at 80 (48): the columns give way in `LADDER`'s order — the rest
/// first, then the report (cut to what's there before it goes), the model, the step cut to its floor, the context, the
/// cost, behind, the overlaps, `▲`; the state last, the name never. Every narrower room shows a subset of the wider's.
#[test]
fn the_row_gives_way_in_its_order() {
    let step = "● editing src/sessionpane.rs 120-180 and its tests";
    let rows = [
        row(&Shown { step: step.into(), ..full(WorkspaceId::HOME, "main", Doing::Working) }),
        row(&Shown { model: "haiku".into(), ..full(WorkspaceId::here(1), "fix-login", Doing::Waiting) }),
    ];
    // (the rows' room in a frame `w` wide: its sides, the bar's cells, a cell before the side)
    let room = |w: usize| w - 2 - 2 - 1;
    let wide = lay(&rows, 400);
    assert_eq!(cols(&wide), COLS, "every column with room for all");
    let width = |l: &(usize, Vec<(Col, usize)>), c: Col| l.1.iter().find(|x| x.0 == c).map(|x| x.1);
    assert_eq!(width(&wide, Col::State), Some(STATE_MAX), "the step to its most");
    let at160 = lay(&rows, room(96));
    let shown = cols(&at160);
    assert!(
        [Col::Waits, Col::State, Col::Overlap, Col::Behind, Col::Context, Col::Cost]
            .iter()
            .all(|c| shown.contains(c)),
        "at 160 the manager's columns all: {shown:?}"
    );
    let at80 = cols(&lay(&rows, room(48)));
    assert!(
        !at80.contains(&Col::Report) && !at80.contains(&Col::Model),
        "at 80 the report and the model first: {at80:?}"
    );
    assert!(at80.contains(&Col::State) && at80.contains(&Col::Waits), "the state stays, and ▲: {at80:?}");
    // (from wide to nothing, one room at a time: what goes, goes in `LADDER`'s order, and never comes back)
    let mut gone: Vec<Col> = Vec::new();
    let mut was = cols(&wide);
    for r in (0..400).rev() {
        let now = cols(&lay(&rows, r));
        assert!(now.iter().all(|c| was.contains(c)), "{r}: {now:?} ⊄ {was:?}");
        gone.extend(was.iter().filter(|c| !now.contains(c)));
        was = now;
    }
    let order: Vec<Col> = (LADDER.iter())
        .filter_map(|g| match *g {
            Give::Gone(c) => Some(c),
            Give::Cut(..) => None,
        })
        .collect();
    assert_eq!(gone, order, "the order they go in");
    // (a column cut only once everything before it in the ladder gave way)
    let before = |c: Col| {
        let at = LADDER
            .iter()
            .position(|g| *g == Give::Cut(c, if c == Col::Report { REPORT_MIN } else { STATE_MIN }));
        order.iter().filter(|g| LADDER[..at.unwrap()].contains(&Give::Gone(**g))).copied().collect::<Vec<_>>()
    };
    for (c, most) in [(Col::Report, REPORT_MAX), (Col::State, STATE_MAX)] {
        let cut = (0..400).rev().map(|r| lay(&rows, r)).find(|l| width(l, c).is_some_and(|w| w < most));
        let cut = cut.expect("cut once").1;
        assert!(cut.iter().all(|x| !before(c).contains(&x.0)), "{c:?} only once the rest went: {cut:?}");
    }
    // (the step at its floor, never shorter, while the context stays: the context goes only when the step can't have it)
    for r in 0..400 {
        let l = lay(&rows, r);
        if width(&l, Col::Context).is_some() {
            assert!(width(&l, Col::State).is_some_and(|w| w >= STATE_MIN), "{r}: {l:?}");
        }
        if width(&l, Col::Context).is_none() && width(&lay(&rows, r + 1), Col::Context).is_some() {
            assert_eq!(
                width(&lay(&rows, r + 1), Col::State),
                Some(STATE_MIN),
                "{r}: ctx goes with the step at 12"
            );
        }
    }
    let (name_w, _) = lay(&rows, 3);
    assert_eq!(name_w, 3, "the names cut to the room, never gone");
}

/// A row's state runs on into the columns right after it the row leaves empty (`room`, the one list 2026-11-16: at 52
/// docked `▸ asks to r…` was cut to nine cells beside an empty overlap): the row that needs you says it whole where
/// another row's overlap keeps the column; a row with its own overlap keeps its cut; the card then says nothing more.
#[test]
fn a_rows_state_runs_on_into_an_empty_overlap() {
    let asks = "▸ asks to run it";
    // (no report nor model: gone this narrow, they'd be what the card says more)
    let bare = |s: Shown| row(&Shown { report: String::new(), model: String::new(), ..s });
    let rows = [
        bare(Shown {
            step: asks.into(),
            overlap: Vec::new(),
            ..full(WorkspaceId::HOME, "main", Doing::Waiting)
        }),
        bare(Shown {
            step: "● editing src/sessionpane.rs 120-180".into(),
            ..full(WorkspaceId::here(1), "fix-login", Doing::Working)
        }),
    ];
    // (the widest room where the step is at its floor and the overlaps still shown)
    let (name_w, cols) = (0..400)
        .rev()
        .map(|r| lay(&rows, r))
        .find(|l| l.1.contains(&(Col::State, STATE_MIN)) && l.1.iter().any(|c| c.0 == Col::Overlap))
        .expect("the step at its floor beside the overlaps");
    let at = cols.iter().position(|c| c.0 == Col::State).unwrap();
    let overlap = cols[at + 1];
    assert_eq!(overlap.0, Col::Overlap, "{cols:?}");
    assert!(asks.width() > STATE_MIN && asks.width() <= STATE_MIN + GAP + overlap.1, "the case: {cols:?}");
    assert_eq!(
        room(&rows[0], &cols, at),
        (STATE_MIN + GAP + overlap.1, 1),
        "into the empty overlap, no further"
    );
    assert_eq!(room(&rows[1], &cols, at), (STATE_MIN, 0), "its own overlap: its cut");
    assert!(!cut_on_row(&rows[0], name_w, &cols), "whole on its row");
    assert!(cut_on_row(&rows[1], name_w, &cols), "cut on its row");
    // (the columns after the state are never given its room)
    let behind = cols.iter().position(|c| c.0 == Col::Behind).unwrap();
    assert_eq!(room(&rows[0], &cols, behind), (cols[behind].1, 0));
}

/// A child's arm (2026-11-04's, the one list L1): `├─── ` under its parent, `└─── ` for the last of its siblings; a
/// child's child one arm further in, `│` down the line of a parent that has a sibling after it, a blank where it has
/// none; a session nobody started none.
#[test]
fn a_child_hangs_on_its_arm() {
    // main → one → knot, main → two, fix → mend → cord
    let parents = [None, Some(0), Some(1), Some(0), None, Some(4), Some(5)];
    assert_eq!(arms(&parents), ["", "├─── ", "│    └─── ", "└─── ", "", "└─── ", "     └─── "]);
    let r = Row { arm: "├─── ".into(), ..row(&full(WorkspaceId::here(1), "fix", Doing::Idle)) };
    assert_eq!(r.name_width(), ARM + 3, "the arm in the name's column");
}

/// The model's column only where the rows' differ: alike, it says nothing and goes from every row; one switched, it
/// stays on all (an unknown one counts for neither).
#[test]
fn the_model_shows_only_where_the_rows_differ() {
    let of = |model: &str, n: u64| {
        row(&Shown { model: model.into(), ..full(WorkspaceId::here(n), "s", Doing::Idle) })
    };
    let mut alike = [of("opus · high", 0), of("opus · high", 1), of("", 2)];
    models_where_they_differ(&mut alike);
    assert!(alike.iter().all(|r| r.cell(Col::Model).is_empty()), "alike: none");
    let mut differ = [of("opus · high", 0), of("haiku", 1), of("", 2)];
    models_where_they_differ(&mut differ);
    let said: Vec<String> = differ.iter().map(|r| r.text(Col::Model)).collect();
    assert_eq!(said, ["opus · high", "haiku", ""]);
}

/// The step in the list's words: its mark, verb and what it acts on; the working sign (it moves) and the time (it
/// ticks) left out.
#[test]
fn the_step_is_said_without_its_time() {
    use crate::now::{Sign, Step, words};
    let step = |sign, verb, what: &str| Step {
        sign,
        verb,
        what: what.into(),
        warn: false,
        took: Some(std::time::Duration::from_secs(75)),
        key: String::new(),
        call: String::new(),
    };
    let run = step(Sign::Mark(crate::chat::Mark::Run), "asks to run", "cargo test");
    assert_eq!(words(&run), "▸ asks to run cargo test");
    assert_eq!(words(&step(Sign::Working, "thinking", "")), "thinking");
    assert!(!words(&run).contains("1m"), "no time");
}

/// Its Claude's last report line: the last sentence of its last answer, Markdown's marks out; nothing before one.
#[test]
fn the_report_line_is_its_claudes_last_sentence() {
    use crate::chat::{Chat, Msg, Role};
    assert_eq!(report(None), "");
    let mut c = Chat::new();
    c.msgs.push(Msg::new(Role::User, "why?", ""));
    assert_eq!(report(Some(&c)), "", "no answer yet");
    c.msgs.push(Msg::new(
        Role::Assistant,
        "First. The `name` is **cloned** here.\n\n```rust\nx\n```\n- So it keys by value. Done now.",
        "",
    ));
    assert_eq!(report(Some(&c)), "Done now.");
    c.msgs.push(Msg::new(Role::Assistant, "## Fixed the `insert`", ""));
    assert_eq!(report(Some(&c)), "Fixed the insert");
}

fn frame(ed: &mut Editor, w: u16, h: u16) -> String {
    ed.focus_sync();
    crate::term::render(ed, &mut Vec::new(), w, h).unwrap();
    crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), w, h).unwrap()
}

fn feed(ed: &mut Editor, keys: &str) {
    feed_at(ed, keys, 160, 40);
}

/// `keys`, a frame `w` × `h` drawn after each.
fn feed_at(ed: &mut Editor, keys: &str, w: u16, h: u16) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let k: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(k);
        frame(ed, w, h);
    }
}

/// `space W` floats the list in a named frame with the keys, its rows the sessions; `esc` gives the keys back (it
/// stays, its frame unlit); a press on it, `space W` again, `C-w w` give them to it; it moves and sizes as any floating
/// pane, its frame the viewer's (kept with the sizes); `C-w e`, `C-w H` say it floats; `q` closes it.
#[test]
fn the_session_list_floats_and_stays_while_you_work() {
    let dir = crate::scratch::Scratch::new("sessionpane");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    feed(&mut ed, " W");
    let p = ed.sessions_pane.clone().expect("up");
    assert_eq!(ed.workspace.here(), Here::List(p.list));
    let s = frame(&mut ed, 160, 40);
    let f = ed.screen.floating.borrow().iter().find(|f| f.0 == GroupKind::Sessions).unwrap().1;
    assert_eq!(
        f,
        crate::split::Rect { x: 32, y: 10, w: 96, h: 3 },
        "its top at 1/4, 3/5 wide, as tall as its row"
    );
    let top = s.lines().nth(f.y).unwrap();
    assert!(top.contains("╭─ sessions ── 1 session") && !top.contains("enter switch"), "{top}");
    let bottom = s.lines().nth(f.y + f.h - 1).unwrap();
    assert!(bottom.contains("╰") && bottom.contains("enter switch"), "its keys in its bottom line: {bottom}");
    assert!(s.lines().nth(f.y + 1).unwrap().contains("▎ main"), "its row, selected");
    feed(&mut ed, "<esc>");
    assert!(ed.sessions_pane.is_some() && ed.workspace.here() == Here::Code, "esc: the keys back, it stays");
    assert!(
        !frame(&mut ed, 160, 40).lines().nth(f.y + f.h - 1).unwrap().contains("enter switch"),
        "its keys only with them"
    );
    ed.handle_mouse(crate::event::Mouse {
        kind: crate::event::MouseKind::Down,
        x: (f.x + 4) as u16,
        y: (f.y + 1) as u16,
        alt: false,
        ctrl: false,
    });
    assert!(ed.sessions_list_keys(), "a press on its row");
    // `C-w w` goes round to it after the docked panes (its id among the leaves, reviewer 7a); `space W` again
    feed(&mut ed, "<esc><C-w>w");
    assert!(ed.sessions_list_keys(), "C-w w: the list");
    feed(&mut ed, "<C-w>w");
    assert_eq!(ed.workspace.here(), Here::Code, "C-w w on: round to the code");
    feed(&mut ed, " W");
    assert!(ed.sessions_list_keys(), "space W again");
    feed(&mut ed, "<C-w>e");
    assert!(ed.messages.last().unwrap().2.contains("the session list floats"));
    // (it docks — P3 step 7d — and `C-w e` floats it again)
    feed(&mut ed, "<C-w>H");
    assert!(ed.messages.last().unwrap().2.contains("sessions docked left"));
    feed(&mut ed, "<C-w>e");
    assert!(ed.sizes.sessions_dock.is_none() && ed.messages.last().unwrap().2.contains("sessions floats"));
    frame(&mut ed, 160, 40);
    feed(&mut ed, "<C-w>mj<ret><C-w>>");
    let moved = ed.screen.floating.borrow().iter().find(|f| f.0 == GroupKind::Sessions).unwrap().1;
    assert_eq!((moved.y, moved.w), (f.y + 2, f.w + crate::sizes::STEP_COLS));
    assert!(ed.sizes.sessions.is_some(), "kept with the sizes");
    feed(&mut ed, "q");
    assert!(ed.sessions_pane.is_none() && ed.screen.floating.borrow().is_empty());
    feed(&mut ed, " W");
    let again = ed.screen.floating.borrow().iter().find(|f| f.0 == GroupKind::Sessions).unwrap().1;
    assert_eq!(again, moved, "where you put it");
}

/// Docked (`C-w L`, P3 step 7d) the list is a leaf beside the code, its keys its own: `enter` on another row switches and
/// it stays — in the session now in front too; its rows take presses there; `C-w e` floats it again.
#[test]
fn the_session_list_docks_and_stays_after_enter() {
    let dir = crate::scratch::Scratch::new("sessiondock");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    let other = dir.join("fix");
    std::fs::create_dir_all(&other).unwrap();
    let fix = ed.workspace_open(&other).unwrap();
    ed.workspace.name = "fix".into();
    assert!(ed.workspace_switch(WorkspaceId::HOME).is_in());
    feed(&mut ed, " W<C-w>L");
    assert_eq!(ed.sizes.sessions_dock, Some(crate::pane::Side::Right));
    let s = frame(&mut ed, 160, 40);
    assert!(ed.screen.floating.borrow().iter().all(|f| f.0 != GroupKind::Sessions), "not floating");
    let (_, r) = ed.dock_rect(GroupKind::Sessions).expect("docked, drawn");
    assert!(s.lines().nth(r.y).unwrap().contains("sessions"), "its top line\n{s}");
    assert!(ed.sessions_list_keys(), "the keys stay with it");
    // (its keys at its foot, a side column's — the one list L2 — never in its top line)
    let foot: String = s.lines().nth(r.y + r.h - 1).unwrap().chars().skip(r.x).take(r.w).collect();
    assert!(foot.trim_start_matches(['│', ' ']).starts_with("enter switch"), "its foot: {foot:?}\n{s}");
    assert!(!s.lines().nth(r.y).unwrap().contains("enter switch"), "{s}");
    feed(&mut ed, "j<ret>");
    assert_eq!(ed.workspace.id, fix, "switched");
    assert!(
        ed.sessions_pane.is_some() && !ed.sessions_list_keys(),
        "docked, it stays — the keys to the code"
    );
    let s = frame(&mut ed, 160, 40);
    let (_, r) = ed.dock_rect(GroupKind::Sessions).expect("in fix's layout too");
    assert!(s.lines().nth(r.y + 2).unwrap().contains("fix"), "its rows\n{s}");
    // A press on main's row: the keys its, that row selected
    ed.handle_mouse(crate::event::Mouse {
        kind: crate::event::MouseKind::Down,
        x: (r.x + 4) as u16,
        y: (r.y + 1) as u16,
        alt: false,
        ctrl: false,
    });
    assert!(ed.sessions_list_keys() && ed.sessions_here() == Some(WorkspaceId::HOME), "a press on its row");
    feed(&mut ed, "<C-w>e");
    assert!(ed.sizes.sessions_dock.is_none(), "floating again");
    frame(&mut ed, 160, 40);
    assert!(ed.screen.floating.borrow().iter().any(|f| f.0 == GroupKind::Sessions));
}

/// #414: docked right in main, the list goes with you where `C-q` takes you for a card — where docs' conversation leaves
/// its column no room it floats for those frames, its dock kept (P3 step 7d) — and a switch made from there leaves it up:
/// a docked list stays. #302's close (a floating list is done once a session is picked elsewhere) read where it was that
/// frame, so it closed it, and back in main it was gone.
#[test]
fn a_docked_session_list_stays_up_across_a_switch_made_where_it_had_no_room() {
    let dir = crate::scratch::Scratch::new("sessiondockcq");
    let mut ed = Editor::new(crate::config::Config::default());
    ed.config.llm.command = "sh".into();
    ed.config.llm.args = vec!["-c".into(), "cat > /dev/null".into()];
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    let other = dir.join("docs");
    std::fs::create_dir_all(&other).unwrap();
    let docs = ed.workspace_open(&other).unwrap();
    ed.workspace.name = "docs".into();
    // (docs' conversation in sight, as the scene's: asked to run the tests)
    feed_at(&mut ed, " L<esc>", 120, 36);
    assert!(ed.workspace_switch(WorkspaceId::HOME).is_in());
    feed_at(&mut ed, " W<C-w>L<esc>", 120, 36);
    assert!(ed.dock_rect(GroupKind::Sessions).is_some(), "docked in main");
    // A card of docs' Claude, waiting behind
    let what = crate::offer::What::Asks { call: crate::tools::Call::test(1) };
    let mut o = crate::offer::Offer::new("Claude asks".into(), "to see the screen", "", String::new(), what);
    ed.offer_seq += 1;
    o.id = ed.offer_seq;
    ed.workspaces.iter_mut().find(|w| w.id == docs).unwrap().parked.push(o);
    frame(&mut ed, 120, 36);
    feed_at(&mut ed, "<C-q>", 120, 36);
    assert_eq!(ed.workspace.id, docs, "C-q took us to docs' card");
    let s = frame(&mut ed, 120, 36);
    let floating = |ed: &Editor| ed.screen.floating.borrow().iter().any(|f| f.0 == GroupKind::Sessions);
    assert!(ed.card_has_keys() && !ed.sessions_list_keys(), "the keys the card's, not the list's");
    // (no room beside docs' chat and the keys elsewhere: nothing drawn, never a box over the chat — #414's second part)
    assert!(!floating(&ed) && ed.dock_rect(GroupKind::Sessions).is_none(), "nothing drawn in docs\n{s}");
    assert!(ed.sessions_pane.is_some(), "still up");
    // Answered, back in main by `:workspace-switch`: the list still up, docked there
    feed_at(&mut ed, "n:workspace-switch 0<ret>", 120, 36);
    assert_eq!(ed.workspace.id, WorkspaceId::HOME);
    let s = frame(&mut ed, 120, 36);
    assert!(ed.sessions_pane.is_some(), "a docked list stays up\n{s}");
    let (_, r) = ed.dock_rect(GroupKind::Sessions).unwrap_or_else(|| panic!("docked in main again\n{s}"));
    assert!(!floating(&ed) && s.lines().nth(r.y).unwrap().contains("sessions"), "its top line\n{s}");
    // …and where there's room in docs, docked there too
    feed_at(&mut ed, ":workspace-switch 1<ret>", 200, 40);
    let s = frame(&mut ed, 200, 40);
    assert!(ed.dock_rect(GroupKind::Sessions).is_some() && !floating(&ed), "docked in docs at 200\n{s}");
}

/// #302: floating, the list is done once a session is picked any other way too — its tag, `:workspace-switch` — as
/// zellij's session manager closes: kept, it floated over the next workspace, its keys the one it was opened in, and a
/// round there gave them back to it while you drove the code. Docked it stays, the overview kept in sight.
#[test]
fn a_floating_session_list_closes_on_a_switch_made_elsewhere() {
    let dir = crate::scratch::Scratch::new("sessionswitched");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    let other = dir.join("fix");
    std::fs::create_dir_all(&other).unwrap();
    let fix = ed.workspace_open(&other).unwrap();
    ed.workspace.name = "fix".into();
    assert!(ed.workspace_switch(WorkspaceId::HOME).is_in());
    feed(&mut ed, " W");
    frame(&mut ed, 160, 40);
    assert!(ed.sessions_list_keys());
    assert!(ed.workspace_switch(fix).is_in());
    ed.focus_sync();
    let s = frame(&mut ed, 160, 40);
    assert!(ed.sessions_pane.is_none(), "closed\n{s}");
    assert_eq!(ed.keys_to().region, crate::uistate::Region::Code, "the keys the code's");
    // (and back where it was opened: the code's there too, nothing left of it)
    assert!(ed.workspace_switch(WorkspaceId::HOME).is_in());
    ed.focus_sync();
    assert!(ed.workspace.is_at(Here::Code) && ed.float_list_in_hand().is_none());
    // Docked, it stays — its keys left where it was
    feed(&mut ed, " W<C-w>L");
    frame(&mut ed, 160, 40);
    assert!(ed.sessions_list_keys());
    assert!(ed.workspace_switch(fix).is_in());
    assert!(ed.sessions_pane.is_some(), "docked, it stays");
}

/// Docked where its column gets no room — a narrow screen — or with the code zoomed, `space W` gives the keys to a
/// list that's drawn: it floats for that frame, its dock kept (back docked once there's room); used, its keys go and it
/// stays up, docked by its place (#414).
#[test]
fn a_docked_list_with_no_room_floats_so_the_keys_go_to_a_list_in_sight() {
    let dir = crate::scratch::Scratch::new("sessiondocknarrow");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    let other = dir.join("fix");
    std::fs::create_dir_all(&other).unwrap();
    ed.workspace_open(&other).unwrap();
    ed.workspace.name = "fix".into();
    assert!(ed.workspace_switch(WorkspaceId::HOME).is_in());
    feed(&mut ed, " W<C-w>L");
    frame(&mut ed, 160, 40);
    assert!(ed.dock_rect(GroupKind::Sessions).is_some(), "docked at 160");
    let floating =
        |ed: &Editor| ed.screen.floating.borrow().iter().find(|f| f.0 == GroupKind::Sessions).map(|f| f.1);
    // (the state's pane for the list: where it is and the rect the frame drew it at)
    let listed = |ed: &Editor| {
        let state = ed.ui_state();
        let panes: Vec<_> =
            state.workspaces.iter().flat_map(|l| &l.panes).filter(|p| p.shows == "list:sessions").collect();
        assert_eq!(panes.len(), 1, "listed once: {panes:#?}");
        (panes[0].place, panes[0].rect)
    };
    let (_, d) = ed.dock_rect(GroupKind::Sessions).unwrap();
    assert_eq!(listed(&ed), (Some("docked"), Some((d.x, d.y, d.w, d.h))), "docked: its leaf");
    // Narrow: no room beside the code — it floats, drawn, with the keys
    let s = frame(&mut ed, 60, 30);
    let r = floating(&ed).expect("floats where its column has no room");
    assert_eq!(listed(&ed), (Some("floating"), Some((r.x, r.y, r.w, r.h))), "floating: its frame");
    assert!(ed.sessions_list_keys(), "the keys with it");
    assert!(s.lines().nth(r.y).unwrap().contains("sessions"), "its frame drawn\n{s}");
    assert!(s.lines().skip(r.y + 1).take(r.h).any(|l| l.contains("fix")), "its rows\n{s}");
    assert_eq!(ed.sizes.sessions_dock, Some(crate::pane::Side::Right), "its dock kept");
    // Room again: docked
    frame(&mut ed, 160, 40);
    assert!(floating(&ed).is_none() && ed.dock_rect(GroupKind::Sessions).is_some(), "docked again");
    // Narrow, `enter` on the front's row: used, the keys go — docked by its place, it stays up, drawn nothing there
    frame(&mut ed, 60, 30);
    assert!(ed.sessions_list_keys(), "the keys still the list's");
    feed_at(&mut ed, "<ret>", 60, 30);
    assert!(ed.sessions_pane.is_some() && !ed.sessions_list_keys(), "docked, it stays: {:?}", ed.status);
    assert!(floating(&ed).is_none() && ed.dock_rect(GroupKind::Sessions).is_none(), "drawn nothing");
    assert_eq!(ed.sizes.sessions_dock, Some(crate::pane::Side::Right), "its dock still kept");
    // The list docked and up, the code (split) zoomed: `space W` — the list drawn, the keys its
    feed(&mut ed, " W<esc><C-w>v<C-w>z");
    frame(&mut ed, 160, 40);
    assert!(ed.zoomed().is_some(), "the code zoomed");
    feed(&mut ed, " W");
    let s = frame(&mut ed, 160, 40);
    assert!(ed.sessions_list_keys(), "the keys with the list");
    let shown = ed.dock_rect(GroupKind::Sessions).map(|d| d.1).or_else(|| floating(&ed));
    let shown = shown.unwrap_or_else(|| panic!("drawn\n{s}"));
    assert!(s.lines().skip(shown.y + 1).take(shown.h).any(|l| l.contains("fix")), "its rows in sight\n{s}");
}

/// #414: docked where its column has no room — here a narrow screen — the list draws nothing while the keys aren't its
/// (never a box over the code or the chat you read; the state lists it docked, no rect), comes back docked as room does,
/// and given the keys floats for that time: `space W`, and `C-w w`'s round, whose stop after the docked panes it is —
/// never a stop on nothing. A list opened floating floats, keys or not, as it did.
#[test]
fn a_docked_list_with_no_room_draws_nothing_until_it_has_the_keys() {
    let dir = crate::scratch::Scratch::new("sessiondockunshown");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    let other = dir.join("fix");
    std::fs::create_dir_all(&other).unwrap();
    ed.workspace_open(&other).unwrap();
    ed.workspace.name = "fix".into();
    assert!(ed.workspace_switch(WorkspaceId::HOME).is_in());
    feed(&mut ed, " W<C-w>L<esc>");
    assert!(ed.dock_rect(GroupKind::Sessions).is_some(), "docked at 160");
    let floating =
        |ed: &Editor| ed.screen.floating.borrow().iter().find(|f| f.0 == GroupKind::Sessions).map(|f| f.1);
    let listed = |ed: &Editor| {
        let state = ed.ui_state();
        let p = state.workspaces.iter().flat_map(|l| &l.panes).find(|p| p.shows == "list:sessions").cloned();
        p.map(|p| (p.place, p.rect, p.at))
    };
    let unshown = |ed: &Editor, s: &str, why: &str| {
        assert!(floating(ed).is_none() && ed.dock_rect(GroupKind::Sessions).is_none(), "{why}: nothing\n{s}");
        assert!(!s.contains("─ sessions") && ed.sessions_pane.is_some(), "{why}: up, not drawn\n{s}");
        assert_eq!(listed(ed), Some((Some("docked"), None, false)), "{why}: listed docked, no rect");
    };
    // Narrow, the keys the code's: nothing drawn
    let s = frame(&mut ed, 60, 30);
    unshown(&ed, &s, "narrow");
    // `C-w w` from the code: its stop — it floats with the keys; the round on, back to the code and gone again
    feed_at(&mut ed, "<C-w>w", 60, 30);
    assert!(ed.sessions_list_keys(), "C-w w: the list's stop");
    let r = floating(&ed).expect("floats with the keys");
    let s = frame(&mut ed, 60, 30);
    assert!(s.lines().nth(r.y).unwrap().contains("sessions"), "its frame\n{s}");
    assert_eq!(listed(&ed).map(|l| l.0), Some(Some("floating")));
    feed_at(&mut ed, "<C-w>w", 60, 30);
    assert_eq!(ed.workspace.here(), Here::Code, "the round on: the code");
    let s = frame(&mut ed, 60, 30);
    unshown(&ed, &s, "the round on");
    // `space W`: the keys, floating; `esc`: gone again
    feed_at(&mut ed, " W", 60, 30);
    assert!(ed.sessions_list_keys() && floating(&ed).is_some(), "space W: floats with the keys");
    feed_at(&mut ed, "<esc>", 60, 30);
    let s = frame(&mut ed, 60, 30);
    unshown(&ed, &s, "esc");
    // Room back: docked, the keys elsewhere
    let s = frame(&mut ed, 160, 40);
    let (_, d) = ed.dock_rect(GroupKind::Sessions).unwrap_or_else(|| panic!("docked again\n{s}"));
    assert!(floating(&ed).is_none() && s.lines().nth(d.y).unwrap().contains("sessions"), "{s}");
    // A list opened floating floats where a docked one would have no room, keys or not (today's)
    feed(&mut ed, " W<C-w>e<esc>");
    assert!(ed.sizes.sessions_dock.is_none() && !ed.sessions_list_keys());
    frame(&mut ed, 60, 30);
    assert!(floating(&ed).is_some(), "floating by its place: drawn without the keys");
}

/// The chat closed and the list docked right, the asks sit over the list's foot as over the chat's (the user,
/// 2026-10-24): their own width (#177), a cell in from the screen's edge, a row of air over its leaf's end; the list
/// keeps its selected row in sight above the stack.
#[test]
fn the_asks_float_over_a_docked_lists_foot_its_selected_row_above_them() {
    use crate::offer::{Offer, What};
    let dir = crate::scratch::Scratch::new("sessiondockasks");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    // (more than the leaf has rows: the last one would be under the stack)
    for k in 0..30 {
        let other = dir.join(format!("s{k:02}"));
        std::fs::create_dir_all(&other).unwrap();
        ed.workspace_open(&other).unwrap();
        ed.workspace.name = format!("s{k:02}");
    }
    assert!(ed.workspace_switch(WorkspaceId::HOME).is_in());
    feed(&mut ed, " W<C-w>LG");
    let what = What::Asks { call: crate::tools::Call::test(1) };
    ed.push_offer(Offer::new("Claude asks".into(), "to see the screen", "", String::new(), what));
    let s = frame(&mut ed, 160, 30);
    let (_, r) = ed.dock_rect(GroupKind::Sessions).expect("docked, drawn");
    let (x, y, cw, ch) = ed.screen.offer_card.expect("drawn");
    assert_eq!(x + cw, r.x + r.w - 1, "a cell in from its right\n{s}");
    assert!(x > r.x + 2, "its own width, narrower than the list's\n{s}");
    assert_eq!(y + ch, r.y + r.h - 1, "a row of air over its leaf's end\n{s}");
    let rows: Vec<&str> = s.lines().collect();
    assert!(rows[y + ch].chars().skip(r.x + 1).all(|c| c == ' '), "air, no row of the list\n{s}");
    // (the list's own bar, at its edge — a toast standing above the card over its rows has one too, further in)
    let bar = (r.y + 1..r.y + r.h).find(|&k| rows[k].chars().nth(r.x + 1) == Some('▎'));
    assert!(
        bar.is_some_and(|k| k + 1 < y),
        "the selected row above the stack, a row of air: {bar:?} < {y}\n{s}"
    );
    assert!(rows[bar.unwrap()].contains("s29"), "the last one selected\n{s}");
    // (its keys over the stack, a row of air between — a side column's foot where the asks stand on it)
    assert!(rows[y - 2].contains("enter switch"), "its keys above the asks: {}\n{s}", rows[y - 2]);
    assert!(rows[y - 1].chars().skip(r.x + 1).take(r.w - 1).all(|c| c == ' '), "air\n{s}");
}

/// The list's side is kept as its width is (`sizes.json`, plan §3.4 "its place kept"): docked left, the next tarae —
/// `:restart`'s, reading the file — opens it docked there; `C-w =` makes sizes automatic, never its side.
#[test]
fn a_docked_session_lists_side_is_kept_across_restart() {
    let dir = crate::scratch::Scratch::new("sessiondockkept");
    let path = dir.join("sizes.json");
    let mut ed = Editor::new(crate::config::Config::default());
    ed.sizes_path = Some(path.clone());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    feed(&mut ed, " W<C-w>H");
    frame(&mut ed, 160, 40);
    feed(&mut ed, "<C-w>=");
    assert_eq!(ed.sizes.sessions_dock, Some(crate::pane::Side::Left), "C-w = keeps where it is");
    // (written on a worker)
    let kept = || crate::sizes::Sizes::read(Some(&path)).sessions_dock;
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    while kept().is_none() && std::time::Instant::now() < end {
        if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
            ed.handle_event(ev);
        }
    }
    assert_eq!(kept(), Some(crate::pane::Side::Left), "in the file");
    // The next tarae: its sizes read from the file, the list opened — docked left, not floating
    let mut ed = Editor::new(crate::config::Config::default());
    ed.sizes = crate::sizes::Sizes::read(Some(&path));
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    feed(&mut ed, " W");
    frame(&mut ed, 160, 40);
    assert!(ed.screen.floating.borrow().iter().all(|f| f.0 != GroupKind::Sessions), "not floating");
    let (side, r) = ed.dock_rect(GroupKind::Sessions).expect("docked, drawn");
    assert!(side == crate::pane::Side::Left && r.x < 80, "on the side it was: {side:?} at {}", r.x);
}

/// A card that comes while the list has the keys waits to be focused (P3 §3.8, the rule 7a's stand-in folded into):
/// its letters stay the list's — `n` its search, never the card's no; `v` the list's —; its bottom line says `C-q`
/// reaches it (the status line counting nothing it draws); there `n` answers it.
#[test]
fn a_card_waits_while_the_session_list_has_the_keys() {
    let dir = crate::scratch::Scratch::new("sessionpane");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    feed(&mut ed, " W");
    ed.push_offer(crate::offer::Offer::new(
        "End the review?".into(),
        "2 files not viewed.",
        "",
        String::new(),
        crate::offer::What::ReviewEnd { session: WorkspaceId::HOME, started: std::time::Instant::now() },
    ));
    let s = frame(&mut ed, 160, 40);
    assert!(!ed.card_has_keys(), "the card waits");
    assert_eq!(ed.keys_to().region, crate::uistate::Region::List(GroupKind::Sessions));
    assert_eq!(ed.keys_to().waiting, Some(format!("card#{}", ed.offer_seq)), "the state says it waits");
    let state = ed.ui_state().json();
    assert_eq!(state["cards"][0]["waits"], "focus", "…and the card");
    let status = s.lines().nth(39).unwrap_or_default();
    assert!(!status.contains("waiting"), "drawn, not counted: {status}");
    assert!(crate::term::card_line(&ed).is_some_and(|l| l.starts_with("╰─ C-q answer")), "its way in:\n{s}");
    feed(&mut ed, "n");
    assert_eq!(ed.offers.len(), 1, "n: not the card's");
    assert!(ed.messages.last().unwrap().2.contains("/ searches"), "the list's n");
    feed(&mut ed, "v");
    assert_eq!(ed.offers.len(), 1, "v: not the card's");
    feed(&mut ed, "<C-q>n");
    assert!(ed.offers.is_empty(), "C-q: the card's keys, n its no");
    // The one the list's own letter asked for (`b`'s, `B`'s — the user's own) takes the keys at once
    let was = ed.events.set_asked(Some(ed.input_seq));
    ed.push_offer(crate::offer::Offer::new(
        "End the review?".into(),
        "2 files not viewed.",
        "",
        String::new(),
        crate::offer::What::ReviewEnd { session: WorkspaceId::HOME, started: std::time::Instant::now() },
    ));
    ed.events.set_asked(was);
    frame(&mut ed, 160, 40);
    assert!(ed.sessions_list_keys() && ed.card_has_keys(), "yours: its keys");
    assert_eq!(ed.keys_to().waiting, None);
}

/// Polish landing 1: a card that came on its own never covers the session list you opened — at 80×24 its stack would, so
/// it folds to its peek at the asks' foot, clear of the list: the peek its title, counted as drawn (the status line
/// says no `▲1 waiting`, only its way in, `C-q answer`, the one place left that says it). A press on that word or `C-q`
/// stands it whole with the keys; `esc` folds it again. A card your own key asked for (`yours`) never folds.
#[test]
fn a_card_that_came_on_its_own_folds_off_the_session_list() {
    let dir = crate::scratch::Scratch::new("sessionpane");
    let dir = std::fs::canonicalize(&dir).unwrap();
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.join("main"), "main".into());
    // (sessions enough that the list reaches down to where the asks stand)
    for name in ["main", "fix", "docs", "web", "k8s", "sound", "motion", "carry"] {
        std::fs::create_dir_all(dir.join(name)).unwrap();
        if name != "main" {
            ed.workspace_open(&dir.join(name)).unwrap();
        }
    }
    assert!(ed.workspace_switch(WorkspaceId::HOME).is_in());
    feed(&mut ed, " W");
    let card = |ed: &mut Editor| {
        let what = crate::offer::What::Asks { call: crate::tools::Call::test(1) };
        let ask = "Claude asks to see the screen: the frame as text goes with the next request, the terminal's rows too.\n\
                   It reads what each pane shows as you see it, the chat's column and the cards as well.";
        ed.push_offer(crate::offer::Offer::new("Claude asks".into(), ask, "", String::new(), what));
    };
    // (drawn whole: its answers in its bottom line)
    let whole = |ed: &Editor| ed.screen.offer_buttons.iter().any(|b| b.3 == 'y');
    card(&mut ed);
    let s = frame(&mut ed, 80, 24);
    let list = ed.float_rect(GroupKind::Sessions).expect("the list floats");
    assert!(!ed.card_has_keys(), "it waits");
    assert!(!whole(&ed), "not drawn whole:\n{s}");
    let rows: Vec<&str> = s.lines().collect();
    let peek =
        rows.iter().position(|r| r.contains("Claude asks")).unwrap_or_else(|| panic!("its peek:\n{s}"));
    assert!(peek >= list.y + list.h, "under the list, never over it (row {peek}, list {list:?}):\n{s}");
    assert_eq!(ed.screen.asks_drawn, [ed.offers[0].id], "its peek is it in sight");
    // (its way in on its peek, its count at the row's end — #544: the status line says none, nor that it waits)
    let status = rows.last().copied().unwrap_or_default();
    assert!(rows[peek].trim_end().ends_with("C-q"), "its peek says its way in: {}", rows[peek]);
    assert!(!status.contains("C-q") && !status.contains("waiting"), "{status}");
    // A press on its peek, its way in: it stands whole, the keys its
    let spot = (ed.screen.offer_buttons.iter())
        .find(|b| b.0 == peek && b.3 == crate::mouse::WAY)
        .copied()
        .unwrap_or_else(|| panic!("its peek a button: {:?}", ed.screen.offer_buttons));
    for kind in [crate::event::MouseKind::Down, crate::event::MouseKind::Up] {
        let (x, y) = (((spot.1 + spot.2) / 2) as u16, spot.0 as u16);
        ed.handle_mouse(crate::event::Mouse { kind, x, y, alt: false, ctrl: false });
    }
    let s = frame(&mut ed, 80, 24);
    assert!(ed.card_has_keys(), "the press focused it");
    assert!(whole(&ed), "whole, over the list:\n{s}");
    // `esc`: the keys back to the list, the card folded again; `C-q` stands it whole once more
    ed.handle_key("esc".parse().unwrap());
    frame(&mut ed, 80, 24);
    assert!(!ed.card_has_keys() && !whole(&ed), "folded again");
    ed.handle_key("C-q".parse().unwrap());
    frame(&mut ed, 80, 24);
    assert!(ed.card_has_keys() && whole(&ed), "C-q: whole");
    ed.handle_key("n".parse().unwrap());
    assert!(ed.offers.is_empty(), "its n");
    // One your own key asked for (the list's `d`: end the review?) stands whole at once, the list or not
    let was = ed.events.set_asked(Some(ed.input_seq));
    let what =
        crate::offer::What::ReviewEnd { session: WorkspaceId::HOME, started: std::time::Instant::now() };
    let ask = "2 files not viewed.\nTheir notes stay with the session; the review's marks go with it.\nEnd it anyway?";
    ed.push_offer(crate::offer::Offer::new("End the review?".into(), ask, "", String::new(), what));
    ed.events.set_asked(was);
    assert!(ed.offers[0].yours);
    let s = frame(&mut ed, 80, 24);
    assert!(ed.card_has_keys() && whole(&ed), "yours: whole:\n{s}");
}

/// orchestra N6: a session another one's Claude started comes right after its parent — fix, yours and opened before them,
/// after main's children, its own child after it, and a child's child right under that child — on its arm (the one
/// list L1); `j`/`k` and the digits go by that order, the one drawn.
#[test]
fn a_child_follows_its_parent_in_the_list() {
    let dir = crate::scratch::Scratch::new("sessionchildren");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    let main = ed.workspace.claude.id;
    let add = |ed: &mut Editor, name: &str, parent| {
        let id = ed.workspace_add(dir.join(name));
        let l = ed.workspace_by_id(id).unwrap();
        (l.name, l.claude.parent) = (name.into(), parent);
        l.claude.id
    };
    let fix = add(&mut ed, "fix", None);
    let one = add(&mut ed, "one", Some(main));
    add(&mut ed, "two", Some(main));
    add(&mut ed, "mend", Some(fix));
    add(&mut ed, "knot", Some(one));
    let order = ["main", "one", "knot", "two", "fix", "mend"];
    let rows = ed.sessions_rows();
    let named: Vec<(&str, &str)> = rows.iter().map(|r| (r.name.as_str(), r.arm.as_str())).collect();
    assert_eq!(
        named,
        [
            ("main", ""),
            ("one", "├─── "),
            ("knot", "│    └─── "),
            ("two", "└─── "),
            ("fix", ""),
            ("mend", "└─── ")
        ]
    );
    feed(&mut ed, " W");
    let s = frame(&mut ed, 160, 40);
    let drawn: Vec<&str> = (s.lines())
        .filter_map(|l| order.iter().find(|n| l.contains(&format!(" {n} ")) && l.contains('│')).copied())
        .collect();
    assert_eq!(drawn, order, "drawn in that order:\n{s}");
    for line in ["├─── one ", "│    └─── knot ", "└─── two ", "└─── mend "]
    {
        assert!(s.lines().any(|l| l.contains(line)), "{line}:\n{s}");
    }
    assert!(!s.contains("─── fix"), "no arm for fix, a session of yours:\n{s}");
    let at = |ed: &Editor| {
        let id = ed.sessions_here().unwrap();
        ed.sessions_in_order().iter().find(|l| l.id == id).unwrap().name.clone()
    };
    let mut walked = vec![at(&ed)];
    for _ in 1..order.len() {
        feed(&mut ed, "j");
        walked.push(at(&ed));
    }
    assert_eq!(walked, order, "j goes by it");
    feed(&mut ed, "kk");
    assert_eq!(at(&ed), "two", "k too");
    feed(&mut ed, "3");
    assert_eq!(ed.workspace.name, "knot", "3: the third row drawn");
}

/// The list is searched with `/` as every pane is (user, 2026-10-23), never type-to-narrow: its own prompt, the
/// selection moving to the next match as the pattern is typed, `esc` back to where it was; `enter` keeps it as the
/// list's search (never the code's), its matches lit, `n`/`N` the next and the one before, round the ends (said); a
/// pattern matching nothing said; `esc` unlights it. `o` makes a new session (`n` is the search's); another letter
/// says `/` searches.
#[test]
fn the_session_list_is_searched_with_slash() {
    let dir = crate::scratch::Scratch::new("sessionpane");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    for name in ["fix-login", "docs"] {
        ed.workspace_add(dir.join(name));
        ed.workspaces.last_mut().unwrap().name = name.into();
    }
    let at = |ed: &Editor| {
        let id = ed.sessions_here().unwrap();
        ed.sessions_in_order().iter().find(|l| l.id == id).unwrap().name.clone()
    };
    let said = |ed: &Editor| ed.messages.last().map(|m| m.2.clone()).unwrap_or_default();
    feed(&mut ed, " W");
    assert!(super::hints(&ed).iter().any(|h| h.0 == "/" && h.1 == "search"), "its keys say it");
    // Typing: the selection goes to the next match; esc back where it was
    feed(&mut ed, "/do");
    assert_eq!(at(&ed), "docs", "as it's typed");
    assert!(lit(&ed).is_some(), "its matches lit as it's typed");
    assert!(ed.search_hits.is_none(), "the code's matches untouched");
    feed(&mut ed, "<esc>");
    assert_eq!((at(&ed), ed.prompt.is_none()), ("main".into(), true), "esc: back where it was");
    assert!(ed.sessions_list_keys(), "the keys still the list's");
    // Entered: the list's search, lit, `n`/`N` round it
    feed(&mut ed, "/login|docs<ret>");
    assert_eq!(at(&ed), "fix-login");
    let p = ed.sessions_pane.as_ref().unwrap();
    assert_eq!((p.search.as_deref(), p.hl), (Some("login|docs"), true));
    assert_eq!(ed.search, None, "never the code's search");
    feed(&mut ed, "n");
    assert_eq!(at(&ed), "docs");
    feed(&mut ed, "n");
    assert_eq!((at(&ed), said(&ed)), ("fix-login".into(), "search wrapped around".into()));
    feed(&mut ed, "N");
    assert_eq!(at(&ed), "docs", "N: the one before, round the start");
    // Lit: the rows' cells drawn otherwise than unlit
    let shown = |ed: &mut Editor| {
        frame(ed, 160, 40);
        ed.last_frame.as_ref().unwrap().0.clone()
    };
    let with = shown(&mut ed);
    ed.sessions_pane.as_mut().unwrap().hl = false;
    assert_ne!(with, shown(&mut ed), "its matches lit");
    ed.sessions_pane.as_mut().unwrap().hl = true;
    feed(&mut ed, "/zzz<ret>");
    assert_eq!((at(&ed), said(&ed)), ("docs".into(), "no session matches /zzz/".into()), "nothing: said");
    // esc: unlit, the keys back to the code
    feed(&mut ed, "<esc>");
    assert!(!ed.sessions_pane.as_ref().unwrap().hl && lit(&ed).is_none());
    feed(&mut ed, " Wz");
    assert!(said(&ed).contains("/ searches"), "another letter: {}", said(&ed));
    feed(&mut ed, "o");
    assert_eq!(ed.prompt.as_ref().map(|p| p.kind), Some(crate::editor::PromptKind::Session), "o: new");
}

/// The floating list's rect at `w` × `h`, a frame drawn first.
fn list_rect(ed: &mut Editor, w: u16, h: u16) -> crate::split::Rect {
    frame(ed, w, h);
    ed.screen.floating.borrow().iter().find(|f| f.0 == GroupKind::Sessions).unwrap().1
}

/// The last thing said.
fn last_said(ed: &Editor) -> String {
    ed.messages.last().map(|m| m.2.clone()).unwrap_or_default()
}

/// The session list floats at a list's 3/5, centred (the user, 2026-11-16: the decided measure stands), whatever its
/// rows need. Where the rule places it, `C-w >` and `C-w <` widen and narrow it on both sides, two cells each, so it
/// stays centred — as a share of the screen, on another screen too; kept with the sizes and opened again so; never past
/// its least, said. `C-w =` puts it back to 3/5 and centred. Its left placed by the rule is `null` in `sizes.json`.
#[test]
fn the_list_is_three_fifths_and_widens_on_both_sides_while_centred() {
    use crate::pane::Frame;
    use crate::split::Rect;
    let dir = crate::scratch::Scratch::new("sessionwide");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    feed_at(&mut ed, " W", 80, 40);
    assert_eq!(list_rect(&mut ed, 80, 40), Rect { x: 16, y: 10, w: 48, h: 3 }, "a list's 3/5, centred");
    // (a name 60 cells long: the width stays — the rows give way, as `lay` says)
    ed.workspace_add(dir.join("n".repeat(60)));
    assert_eq!(list_rect(&mut ed, 80, 40), Rect { x: 16, y: 10, w: 48, h: 4 }, "3/5 whatever the rows need");
    assert_eq!(list_rect(&mut ed, 160, 40), Rect { x: 32, y: 10, w: 96, h: 4 }, "3/5 at 160 too");
    // `C-w >`: two cells on each side
    feed_at(&mut ed, "<C-w>>", 80, 40);
    assert_eq!((list_rect(&mut ed, 80, 40).x, list_rect(&mut ed, 80, 40).w), (14, 52), "wider on both sides");
    assert_eq!(last_said(&ed), "sessions 52 × 4, in the middle · C-w = automatic");
    feed_at(&mut ed, "<C-w>>", 80, 40);
    assert_eq!((list_rect(&mut ed, 80, 40).x, list_rect(&mut ed, 80, 40).w), (12, 56), "and again");
    assert!(
        ed.sizes.sessions.is_some_and(|f| f.x == Frame::PLACED && f.h == 0),
        "kept, centred, as tall as its rows"
    );
    // (a share of the screen: 56 of 80 is 112 of 160, centred there)
    assert_eq!(
        (list_rect(&mut ed, 160, 40).x, list_rect(&mut ed, 160, 40).w),
        (24, 112),
        "on another screen"
    );
    // `C-w <`: two cells off each side
    feed_at(&mut ed, "<C-w><lt>", 80, 40);
    assert_eq!(
        (list_rect(&mut ed, 80, 40).x, list_rect(&mut ed, 80, 40).w),
        (14, 52),
        "narrower on both sides"
    );
    // Opened again: the width you set, centred
    feed_at(&mut ed, "q W", 80, 40);
    assert_eq!((list_rect(&mut ed, 80, 40).x, list_rect(&mut ed, 80, 40).w), (14, 52), "kept");
    // `C-w =`: 3/5 and centred again
    feed_at(&mut ed, "<C-w>=", 80, 40);
    assert_eq!(
        (ed.sizes.sessions, list_rect(&mut ed, 80, 40).x, list_rect(&mut ed, 80, 40).w),
        (None, 16, 48)
    );
    assert!(last_said(&ed).starts_with("sessions in the middle"), "{}", last_said(&ed));
    // Never narrower than a floating pane's least, said
    feed_at(&mut ed, "<C-w><lt><C-w><lt>", 80, 40);
    assert_eq!((list_rect(&mut ed, 80, 40).x, list_rect(&mut ed, 80, 40).w), (20, 40), "its least");
    feed_at(&mut ed, "<C-w><lt>", 80, 40);
    assert_eq!(list_rect(&mut ed, 80, 40).w, 40);
    assert_eq!(last_said(&ed), "sessions as narrow as it goes · C-w = automatic");
    // (as short as its rows: no shorter, said)
    feed_at(&mut ed, "<C-w>-", 80, 40);
    assert_eq!(last_said(&ed), "sessions as short as it goes · C-w = automatic");
    // One word for `C-w =` in everything the list said, as every pane's size line (`say_size`)
    let ours = |m: &str| m.match_indices("C-w = ").all(|(i, _)| m[i..].starts_with("C-w = automatic"));
    assert!(
        ed.messages.iter().all(|m| ours(&m.2)),
        "{:?}",
        ed.messages.iter().map(|m| &m.2).collect::<Vec<_>>()
    );
    // (as `sizes.json` keeps it)
    assert_eq!(Frame::SESSIONS.json(), serde_json::json!([null, null, 600, 0]));
    assert_eq!(Frame::of_json(&Frame::SESSIONS.json()), Some(Frame::SESSIONS));
    // `:messages` stays a list's 3/5 with its left set
    assert_eq!(crate::floatlist::list_frame(GroupKind::Messages), Frame::LIST);
}

/// Once moved across (`C-w m`) the list's left edge stays as `C-w >` and `C-w <` size it, its right moving; `C-w =` puts
/// it back to 3/5 and centred. A move down only keeps it centred across: `C-w >` still widens it on both sides.
#[test]
fn once_moved_the_lists_left_edge_stays_as_keys_size_it() {
    let dir = crate::scratch::Scratch::new("sessionmoved");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    feed_at(&mut ed, " W<C-w>mj<ret><C-w>>", 80, 40);
    let r = list_rect(&mut ed, 80, 40);
    assert_eq!((r.x, r.y, r.w), (14, 12, 52), "moved down: still centred across, wider on both sides");
    feed_at(&mut ed, "<C-w>=<C-w>ml<ret>", 80, 40);
    assert_eq!((list_rect(&mut ed, 80, 40).x, list_rect(&mut ed, 80, 40).w), (20, 48), "moved right");
    assert_eq!(last_said(&ed), "moved — C-w m moves it again, C-w = automatic", "the size line's word");
    feed_at(&mut ed, "<C-w>>", 80, 40);
    assert_eq!((list_rect(&mut ed, 80, 40).x, list_rect(&mut ed, 80, 40).w), (20, 52), "its left edge stays");
    assert_eq!(last_said(&ed), "sessions 52 × 3 · C-w = automatic");
    feed_at(&mut ed, "<C-w><lt><C-w><lt>", 80, 40);
    assert_eq!(
        (list_rect(&mut ed, 80, 40).x, list_rect(&mut ed, 80, 40).w),
        (20, 44),
        "narrower, its left staying"
    );
    feed_at(&mut ed, "<C-w>=", 80, 40);
    assert_eq!((list_rect(&mut ed, 80, 40).x, list_rect(&mut ed, 80, 40).w), (16, 48), "back in the middle");
}

/// A press at (`x`, `y`) dragged to (`to`, `y`) and let go there, a frame `w` × `h` drawn after each.
fn drag_at(ed: &mut Editor, (x, y): (usize, usize), to: usize, (w, h): (u16, u16)) {
    use crate::event::{Mouse, MouseKind as K};
    for (kind, x) in [(K::Down, x), (K::Drag, to), (K::Up, to)] {
        ed.handle_mouse(Mouse { kind, x: x as u16, y: y as u16, alt: false, ctrl: false });
        frame(ed, w, h);
    }
}

/// The mouse sizes the floating list by both side borders, to the cell: the right one dragged moves its right edge, the
/// left one its left, the other side staying — kept and said; dragged, it's placed, so `C-w >` moves its right edge
/// only. Docked right, its border drags its width, the far side staying, and `C-w >` / `C-w <` move that edge as any
/// docked column's (#601: they skipped it — it has no run panel).
#[test]
fn the_lists_side_borders_drag_floating_and_docked_and_keys_size_it_docked() {
    use crate::pane::Shows;
    let dir = crate::scratch::Scratch::new("sessiondrag");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    let sz = (160, 40);
    feed(&mut ed, " W");
    let r = list_rect(&mut ed, 160, 40);
    assert_eq!((r.x, r.w), (32, 96));
    // Its right border, 6 cells right: 6 wider, its left staying
    drag_at(&mut ed, (r.x + r.w - 1, r.y + 1), r.x + r.w - 1 + 6, sz);
    let d = list_rect(&mut ed, 160, 40);
    assert_eq!((d.x, d.w), (32, 102), "the right edge to the cell");
    assert_eq!(last_said(&ed), "sessions 102 × 3 · C-w = automatic");
    assert!(ed.sizes.sessions.is_some_and(|f| f.x != crate::pane::Frame::PLACED), "kept, placed");
    // Its left border, 5 cells left: 5 wider, its right staying
    drag_at(&mut ed, (d.x, d.y + 1), d.x - 5, sz);
    let d = list_rect(&mut ed, 160, 40);
    assert_eq!((d.x, d.x + d.w), (27, 134), "the left edge to the cell, the right staying");
    // Dragged, `C-w >` moves its right edge only
    feed(&mut ed, "<C-w>>");
    assert_eq!((list_rect(&mut ed, 160, 40).x, list_rect(&mut ed, 160, 40).w), (27, 111));
    feed(&mut ed, "<C-w>=");
    assert_eq!((list_rect(&mut ed, 160, 40).x, list_rect(&mut ed, 160, 40).w), (32, 96), "C-w =: centred");
    // Docked right: `C-w >` / `C-w <` move its free edge, said
    feed(&mut ed, "<C-w>L");
    frame(&mut ed, 160, 40);
    let (_, r) = ed.dock_rect(GroupKind::Sessions).expect("docked, drawn");
    assert!(ed.sessions_list_keys());
    feed(&mut ed, "<C-w>>");
    let (_, w) = ed.dock_rect(GroupKind::Sessions).unwrap();
    assert_eq!((w.w, w.x + w.w), (r.w + crate::sizes::STEP_COLS, r.x + r.w), "wider, its left edge moving");
    assert!(last_said(&ed).starts_with(&format!("sessions {} columns", w.w)), "{}", last_said(&ed));
    feed(&mut ed, "<C-w><lt><C-w><lt>");
    let (_, n) = ed.dock_rect(GroupKind::Sessions).unwrap();
    assert_eq!((n.w, n.x + n.w), (r.w - crate::sizes::STEP_COLS, r.x + r.w), "narrower");
    assert!(ed.sizes.docked[GroupKind::Sessions.dock_index()].is_some(), "kept with the sizes");
    // Its border, dragged 6 cells left: 6 wider, the screen's edge side staying
    let line = ed.line_of(Shows::Group(GroupKind::Sessions)).expect("its line");
    let y = n.y + 3;
    let x = (n.x.saturating_sub(1)..=n.x).find(|&x| ed.border_at(x, y) == Some(line)).expect("its border");
    assert_eq!(ed.mouse_at(x, y).part, crate::mouse::Part::Border(line), "a press there takes its border");
    drag_at(&mut ed, (x, y), x - 6, sz);
    assert!(last_said(&ed).starts_with(&format!("sessions {} columns", n.w + 6)), "{}", last_said(&ed));
    let (_, g) = ed.dock_rect(GroupKind::Sessions).unwrap();
    assert_eq!((g.w, g.x + g.w), (n.w + 6, n.x + n.w), "dragged to the cell, the far side staying");
    assert!(ed.border_at(x - 6, y) == Some(line), "its border where it was let go");
}

/// More sessions than the list has rows: the `▎` thumb in the cell before its side, at the top, the middle and the end
/// of twenty as the selection goes down; none while every row is in sight.
#[test]
fn the_list_shows_its_thumb_when_it_scrolls() {
    let dir = crate::scratch::Scratch::new("sessionthumb");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    let thumb = |ed: &mut Editor| -> Vec<usize> {
        let s = frame(ed, 160, 20);
        let f = ed.screen.floating.borrow().iter().find(|f| f.0 == GroupKind::Sessions).unwrap().1;
        let rows: Vec<&str> = s.lines().collect();
        (f.y + 1..f.y + f.h - 1)
            .filter(|&y| rows[y].chars().nth(f.x + f.w - 2) == Some('▎'))
            .map(|y| y - f.y - 1)
            .collect()
    };
    feed_at(&mut ed, " W", 160, 20);
    assert!(thumb(&mut ed).is_empty(), "one row: no thumb");
    for i in 0..19 {
        ed.workspace_add(dir.join(format!("s{i:02}")));
    }
    // (20 rows in 7: a thumb of 3)
    feed_at(&mut ed, "gg", 160, 20);
    assert_eq!(thumb(&mut ed), vec![0, 1, 2], "at the top");
    feed_at(&mut ed, "jjjjjjjjjj", 160, 20);
    let middle = thumb(&mut ed);
    assert!(middle.len() == 3 && middle[0] > 0 && middle[2] < 6, "in the middle: {middle:?}");
    feed_at(&mut ed, "G", 160, 20);
    assert_eq!(thumb(&mut ed), vec![4, 5, 6], "at the end");
}

/// A list floats as tall as its rows, up to half the screen (user, 2026-10-23), its top on the line every
/// floating pane's is — 1/4 of the screen's height, a full 3/5 × 1/2 one's (centred) — for 1, 3 or many rows: a
/// session added grows it downward, its top never moving, opened again too; a move or a width keeps it as tall as its
/// rows, a height you set (`C-w + -`) is kept, `C-w =` gives it its rows again on that line.
#[test]
fn the_list_is_as_tall_as_its_rows() {
    let dir = crate::scratch::Scratch::new("sessionpane");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    let rect = |ed: &mut Editor| {
        frame(ed, 160, 40);
        ed.screen.floating.borrow().iter().find(|f| f.0 == GroupKind::Sessions).unwrap().1
    };
    // (a full floating pane, 3/5 × 1/2 — the tests' — is centred: its top at 1/4, row 10 of 39)
    let full = crate::pane::Frame::CENTRED.rect(160, 39);
    assert_eq!((full.y, full.h), (10, 20), "a full one: 10 rows above, 9 below");
    let rect_of = |y, h| crate::split::Rect { x: 32, y, w: 96, h };
    feed(&mut ed, " W");
    assert_eq!(rect(&mut ed), rect_of(full.y, 3), "one row: its top a full one's");
    ed.workspace_add(dir.join("a"));
    ed.workspace_add(dir.join("b"));
    assert_eq!(rect(&mut ed), rect_of(full.y, 5), "three: grown downward, its top line staying");
    feed(&mut ed, "q W");
    assert_eq!(rect(&mut ed), rect_of(full.y, 5), "opened again: the same line");
    for i in 0..30 {
        ed.workspace_add(dir.join(format!("s{i}")));
    }
    assert_eq!(rect(&mut ed), rect_of(full.y, 19), "many: up to half the screen (39 rows), the same top");
    for _ in 0..30 {
        let id = ed.workspaces.last().unwrap().id;
        ed.workspaces.retain(|l| l.id != id);
    }
    // A move and a width: still as tall as its rows
    feed(&mut ed, "<C-w>mj<ret><C-w>>");
    assert_eq!(ed.sizes.sessions.map(|f| f.h), Some(0), "kept, its height its rows'");
    ed.workspace_add(dir.join("c"));
    // (moved down only, it's centred across still: wider on both sides)
    assert_eq!(rect(&mut ed), crate::split::Rect { x: 30, y: 12, w: 100, h: 6 }, "moved 2, wider 4, grown");
    // As short as it goes: `C-w -` says so, never taller
    feed(&mut ed, "<C-w>-");
    assert_eq!(rect(&mut ed).h, 6, "no shorter");
    assert!(ed.messages.last().unwrap().2.contains("as short as it goes"));
    // A height you set is kept: a session added doesn't grow it
    feed(&mut ed, "<C-w>+");
    let set = rect(&mut ed);
    assert_eq!(set.h, 8);
    ed.workspace_add(dir.join("d"));
    assert_eq!(rect(&mut ed), set, "the height you set");
    feed(&mut ed, "<C-w>=");
    assert_eq!(
        (ed.sizes.sessions, rect(&mut ed)),
        (None, rect_of(full.y, 7)),
        "its rows again, its top at 1/4"
    );
}

/// The row in hand's card says the whole row (the one list L3): its name, then its state with the step whole, whom it
/// overlaps, behind, ctx, the model, the report whole and the cost, ` · ` apart — never `▲` nor the rest the list
/// always said; a row shows less than it where its name is cut or one of those cells is cut or gone.
#[test]
fn the_row_in_hands_card_says_the_whole_row() {
    let step = "▸ asks to run cargo test --workspace";
    let r = row(&Shown { step: step.into(), ..full(WorkspaceId::HOME, "main", Doing::Waiting) });
    let text: String = card(&r).iter().map(|p| p.0.as_str()).collect();
    assert_eq!(
        text,
        format!(
            "main · {step} · ≈ main · ↓3 · ctx 84% · opus · high · I will switch it to the entry API so a repeat \
             keeps the first strand · $0.42"
        )
    );
    assert_eq!(card(&r)[0].1, Ink::Participant(0), "its name in its colour");
    let rows = [r];
    let (name_w, cols) = lay(&rows, 400);
    assert!(cut_on_row(&rows[0], name_w, &cols), "a report past REPORT_MAX: cut, however wide");
    let short = "I will switch it to the entry API.";
    let rows = [row(&Shown { report: short.into(), ..full(WorkspaceId::HOME, "main", Doing::Waiting) })];
    let (name_w, cols) = lay(&rows, 400);
    assert!(!cut_on_row(&rows[0], name_w, &cols), "wide: every cell whole");
    let (name_w, cols) = lay(&rows, 60);
    assert!(cut_on_row(&rows[0], name_w, &cols), "the report gone, the step cut: {cols:?}");
    let (name_w, cols) = lay(&rows, 2);
    assert!(cut_on_row(&rows[0], name_w, &cols), "its name cut");
    // (what the card doesn't say gone says nothing)
    let few = [row(&Shown { report: String::new(), ..full(WorkspaceId::HOME, "main", Doing::Idle) })];
    let (name_w, mut cols) = lay(&few, 400);
    cols.retain(|c| !matches!(c.0, Col::Branch | Col::Usage | Col::Waits));
    assert!(!cut_on_row(&few[0], name_w, &cols), "the rest gone: nothing the card says");
}

/// The cut card's text and its frame's rect, the list's floating frame — the card outside it, below.
fn hand_card(ed: &mut Editor, w: u16, h: u16) -> (Option<String>, String) {
    let s = frame(ed, w, h);
    let f = ed.screen.floating.borrow().iter().find(|f| f.0 == GroupKind::Sessions).map(|f| f.1);
    let c = ed.screen.cut_card.borrow().clone();
    let text = c.map(|c| {
        let f = f.unwrap_or_else(|| panic!("floating\n{s}"));
        assert!(c.outside && c.rect.1 >= f.y + f.h, "outside its frame, below: {c:?} {f:?}\n{s}");
        let crate::whole::Cut::Text(w) = c.cut else { panic!("a text's card") };
        w.text
    });
    (text, s)
}

/// The name of the row in hand.
fn here_name(ed: &Editor) -> String {
    let here = ed.float_list_here(GroupKind::Sessions);
    ed.sessions_rows().into_iter().find(|r| Some(r.key()) == here).expect("a row in hand").name
}

/// An editor in `dir` with a chat (a stand-in's) and a file open, `main` in front, and `fix` behind — helpers start here.
fn helped(dir: &crate::scratch::Scratch) -> Editor {
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    let mut c = crate::chat::Chat::new();
    (c.web_search, c.model) = (true, "claude-opus-5-5".into());
    std::mem::forget(crate::chat::stand_in(&mut c));
    ed.workspace.claude.chat = Some(c);
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    ed.open(&dir.join("a.txt")).unwrap();
    let fix = ed.workspace_add(dir.join("fix"));
    ed.workspace_by_id(fix).unwrap().name = "fix".into();
    ed
}

/// A web search of `words` started in the session in front, its helper a stand-in.
fn searching(ed: &mut Editor, n: u64, words: &str) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::mem::forget(rx);
    (ed.search_lines, ed.workspace.claude.searches_ok) = (Some(tx), true);
    let generation = ed.workspace.claude.chat.as_ref().unwrap().generation();
    let call = crate::tools::Call::of(generation, "", serde_json::json!(n));
    crate::web::search::call(ed, call, &serde_json::json!({ "query": words })).unwrap();
}

/// `space i` started in the session in front, its claude a stand-in.
fn asking(ed: &mut Editor, instruction: &str) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::mem::forget(rx);
    ed.ask_lines = Some(tx);
    crate::ask::start(ed, instruction).unwrap();
}

/// Each row's arm and name, a helper's state after it.
fn named(ed: &Editor) -> Vec<String> {
    let rows = ed.sessions_rows();
    let said = |r: &Row| match r.helper {
        Some(_) => format!("{}{} · {}", r.arm, r.name, r.text(Col::State)),
        None => format!("{}{}", r.arm, r.name),
    };
    rows.iter().map(said).collect()
}

/// The one list L4: a session's helpers each a row under it on a helper's arm (`├─ `, half a child's) — the reviewer,
/// `space i`, its searches in one row (`search ×2`) — their step without its time, what each was asked in the report's
/// column; the top line counts the sessions and the helpers apart; the digits count sessions only; on a helper's row
/// the bottom line says where `enter` goes, and a session's key is refused in words.
#[test]
fn helper_rows_hang_under_their_session_and_digits_skip_them() {
    let dir = crate::scratch::Scratch::new("sessionhelpers");
    let mut ed = helped(&dir);
    let reviewer =
        crate::reviewer::Reviewer::for_test(dir.to_path_buf(), crate::reviewer::State::Reading, vec![], "");
    ed.workspace.reviewer = Some(reviewer);
    asking(&mut ed, "upper it");
    searching(&mut ed, 1, "tokio release");
    searching(&mut ed, 2, "node  lts");
    assert_eq!(
        named(&ed),
        [
            "main",
            "├─ reviewer · reviewing · 0 reads",
            "├─ ask · ◦ thinking",
            "└─ search ×2 · • searching",
            "fix"
        ]
    );
    let rows = ed.sessions_rows();
    assert_eq!(rows[2].text(Col::Report), "\"upper it\"");
    assert_eq!(rows[3].text(Col::Report), "\"node lts\"", "the newest's words, on one line");
    assert_eq!(rows[3].text(Col::Model), "haiku", "the search's own model, the one that differs");
    assert!(rows.iter().all(|r| r.colour == rows[0].colour || r.id != rows[0].id), "its session's colour");
    feed(&mut ed, " W");
    let s = frame(&mut ed, 160, 40);
    assert!(
        s.contains("2 sessions · 3 helpers") && !s.contains("working"),
        "helpers at work, no session: {s}"
    );
    assert!(s.contains("├─ reviewer ") && s.contains("└─ search ×2 "), "{s}");
    // (a helper's row in hand: no session there, its keys where its work is)
    feed(&mut ed, "j");
    assert_eq!(ed.sessions_here(), None);
    let main = ed.workspace.id;
    assert_eq!(ed.sessions_hand(), Some(RowId::Helper { of: main, kind: Kind::Review }));
    let s = frame(&mut ed, 160, 40);
    assert!(s.contains("enter its notes and verdict  / search  esc code  q close"), "{s}");
    assert!(!s.contains("o new"), "none of a session's keys:\n{s}");
    for c in ["d", "b", "m", "f", "$", "O"] {
        feed(&mut ed, c);
        let said = ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
        assert_eq!(
            said,
            format!(
                "{c} is a session's key — this row is main's helper, no session: enter goes to its notes and verdict"
            )
        );
    }
    assert_eq!(ed.workspaces.len(), 1, "nothing ended");
    feed(&mut ed, "jj");
    let s = frame(&mut ed, 160, 40);
    assert!(s.contains("enter its results  / search"), "{s}");
    feed(&mut ed, "2");
    assert_eq!(ed.workspace.name, "fix", "2: the second session, not the second row");
}

/// The top line counts the sessions, how many of them work, then the helpers (the design session, 2026-11-16:
/// `8 sessions · 3 helpers · 8 working` read as every session at work) — a helper at work is never in the sessions'
/// count; too narrow, `· 3 helpers` folds first.
#[test]
fn the_top_line_counts_the_sessions_at_work_then_the_helpers_which_fold_first() {
    let session = |n: u64, doing| row(&full(WorkspaceId::here(n), "s", doing));
    let helper =
        |n: u64, running| Row { helper: Some(Kind::Search), working: running, ..session(n, Doing::Idle) };
    let rows = [session(1, Doing::Working), helper(1, true), helper(1, true), session(2, Doing::Idle)];
    assert_eq!(counts(&rows), ["2 sessions", "1 working", "2 helpers"], "working: the sessions'");
    assert_eq!(counts(&rows[1..]), ["1 session", "2 helpers"], "none at work: no count of it");
    assert_eq!(counts(&rows[3..]), ["1 session"]);
    // Drawn: the words; narrow, the helpers' count goes first
    let dir = crate::scratch::Scratch::new("sessioncounts");
    let mut ed = helped(&dir);
    ed.workspace.claude.chat.as_mut().unwrap().state = crate::chat::State::Thinking;
    asking(&mut ed, "upper it");
    searching(&mut ed, 1, "tokio release");
    feed(&mut ed, " W");
    let top = |ed: &mut Editor, w: u16| {
        let s = frame(ed, w, 40);
        let r = list_rect(ed, w, 40);
        s.lines().nth(r.y).unwrap().to_string()
    };
    let wide = top(&mut ed, 160);
    assert!(wide.contains("─ sessions ── 2 sessions · 1 working · 2 helpers ─"), "{wide}");
    // (at its least, 40: the name and two counts, not the third)
    feed_at(&mut ed, "<C-w><lt><C-w><lt>", 80, 40);
    let narrow = top(&mut ed, 60);
    assert!(
        narrow.contains("─ sessions ── 2 sessions · 1 working ─") && !narrow.contains("helper"),
        "{narrow}"
    );
}

/// A helper's state word takes the ink a session's row gives the same word (`WORDS`, the design session 2026-11-16):
/// `done` the accent as a session's `done`, `failed` the warning as a session's `waits`, `stopped` faint — your key's or
/// its turn's end, never news — whatever ink the code that ended it wrote.
#[test]
fn a_helpers_state_word_takes_a_session_rows_ink_for_it() {
    let session = |doing| row(&full(WorkspaceId::here(1), "s", doing)).cell(Col::State)[0].1;
    assert_eq!(word_ink("done · 2 findings"), Some(session(Doing::Done)), "done: a session's done");
    assert_eq!(word_ink("waits"), Some(session(Doing::Waiting)));
    let l = full(WorkspaceId::here(1), "s", Doing::Idle);
    let ink = |said: &str, wrote: Ink| {
        let h = Helper {
            kind: Kind::Ask,
            name: "ask".into(),
            said: (said.into(), wrote),
            what: String::new(),
            model: String::new(),
            cost: String::new(),
            running: false,
        };
        helper_row(&l, h).cell(Col::State)[0].1
    };
    assert_eq!(ink("done · 2 findings", Ink::Text), Ink::Accent);
    assert_eq!(ink("failed", Ink::Faint), Ink::Warn);
    assert_eq!(ink("stopped", Ink::Warn), Ink::Faint, "stopped: faint, not news");
    assert_eq!(ink("• searching", Ink::Faint), Ink::Faint, "a step: as written");
    assert_eq!(ink("found nothing", Ink::Faint), Ink::Faint);
    // (and live: `space i` stopped by your key, a reviewer done)
    let dir = crate::scratch::Scratch::new("sessionwords");
    let mut ed = helped(&dir);
    let done =
        crate::reviewer::Reviewer::for_test(dir.to_path_buf(), crate::reviewer::State::Done, vec![1], "");
    ed.workspace.reviewer = Some(done);
    asking(&mut ed, "upper it");
    crate::llm::cancel(&mut ed);
    let rows = ed.sessions_rows();
    assert_eq!(rows[1].cell(Col::State), [("done · 1 finding".to_string(), Ink::Accent)]);
    assert_eq!(rows[2].cell(Col::State), [("stopped".to_string(), Ink::Faint)]);
}

/// A helper's row says its model where the rows' models differ (`models_where_they_differ`) and its cost where its
/// result reported one (the reviewer's) while money is on the screen (`llm.cost`) — on its row and in its card.
#[test]
fn a_helpers_row_says_its_model_where_rows_differ_and_a_reported_cost() {
    let dir = crate::scratch::Scratch::new("sessionhelpercost");
    let mut ed = helped(&dir);
    let mut done =
        crate::reviewer::Reviewer::for_test(dir.to_path_buf(), crate::reviewer::State::Done, vec![1], "");
    done.cost = Some(0.08);
    ed.workspace.reviewer = Some(done);
    let reviewer =
        |ed: &Editor| ed.sessions_rows().into_iter().find(|r| r.helper == Some(Kind::Review)).unwrap();
    let r = reviewer(&ed);
    assert_eq!(r.text(Col::Cost), crate::usage::dollars(0.08), "its reported cost");
    assert!(card(&r).iter().any(|p| p.0 == crate::usage::dollars(0.08)), "in its card: {:?}", card(&r));
    // (every row's model alike — main's and its reviewer's `opus` —: said nowhere)
    assert_eq!(r.text(Col::Model), "", "alike: none");
    // (a search's own model beside it: each row its own)
    searching(&mut ed, 1, "tokio release");
    let r = reviewer(&ed);
    assert_eq!(r.text(Col::Model), "opus", "its session's model, the rows differing");
    assert!(card(&r).iter().any(|p| p.0 == "opus"), "in its card");
    // (money off the screen: no cost)
    ed.config.llm.cost = false;
    assert_eq!(reviewer(&ed).text(Col::Cost), "");
}

/// Every key the session list takes, on a helper's row, acts or is refused in words — never nothing (the design
/// session, 2026-11-16): each printable key, so a session's key added later can't fall through in silence where no
/// session is in hand; those `KEYS` says act on the session are refused as a session's keys.
#[test]
fn every_key_on_a_helper_row_acts_or_is_refused_in_words() {
    let dir = crate::scratch::Scratch::new("sessionhelperkeys");
    let mut ed = helped(&dir);
    let reviewer =
        crate::reviewer::Reviewer::for_test(dir.to_path_buf(), crate::reviewer::State::Reading, vec![], "");
    ed.workspace.reviewer = Some(reviewer);
    let main = ed.workspace.id;
    let helper_row = RowId::Helper { of: main, kind: Kind::Review };
    // (the editor's own keys — the space menu, the command line — are never the list's)
    for c in ('!'..='~').filter(|&c| c != ':') {
        if ed.workspace.id != main {
            assert!(ed.workspace_switch(main).is_in());
        }
        ed.prompt = None;
        ed.session_list();
        ed.sessions_pane.as_mut().unwrap().selected = helper_row.pack();
        frame(&mut ed, 160, 40);
        assert_eq!(ed.sessions_hand(), Some(helper_row));
        ed.status = None;
        let said_before = ed.messages.len();
        let state = |ed: &Editor| {
            let at = ed.float_list_here(GroupKind::Sessions);
            (ed.workspace.id, ed.prompt.is_some(), ed.sessions_pane.is_some(), at, ed.offers.len(), ed.mode)
        };
        let before = state(&ed);
        ed.handle_key(Key::plain(Code::Char(c)));
        frame(&mut ed, 160, 40);
        let said = ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
        let acted = state(&ed) != before;
        assert!(
            acted || !said.is_empty() || ed.messages.len() > said_before,
            "{c:?} on a helper's row: nothing"
        );
        if takes_the_session(c) {
            assert!(said.starts_with(&format!("{c} is a session's key")), "{c:?}: {said}");
        }
        ed.mode = crate::editor::Mode::Normal;
    }
}

/// An ended helper's row stands as it ended until its session's next turn begins (the one list L4) — `space i`
/// stopped beside a search that runs; the reviewer's done too, and past that turn while its end is unseen. A row in hand
/// that goes leaves its session's in hand.
#[test]
fn an_ended_helpers_row_stands_until_its_sessions_next_turn() {
    let dir = crate::scratch::Scratch::new("sessionended");
    let mut ed = helped(&dir);
    let done =
        crate::reviewer::Reviewer::for_test(dir.to_path_buf(), crate::reviewer::State::Done, vec![1, 2], "");
    ed.workspace.reviewer = Some(done);
    asking(&mut ed, "upper it");
    crate::llm::cancel(&mut ed);
    searching(&mut ed, 1, "tokio release");
    assert_eq!(
        named(&ed),
        ["main", "├─ reviewer · done · 2 findings", "├─ ask · stopped", "└─ search · • searching", "fix"]
    );
    let rows = ed.sessions_rows();
    assert_eq!(rows[2].cell(Col::State), [("stopped".to_string(), Ink::Faint)], "your key's: faint");
    assert_eq!(rows[1].cell(Col::State)[0].1, Ink::Accent);
    // (the search ends without its list: its row says so, in place of the one running)
    let id = ed.workspace.claude.searches[0].id;
    crate::web::search::on_event(&mut ed, id, crate::agent::Event::Exit(crate::agent::Exit::exited("")));
    assert_eq!(named(&ed)[3], "└─ search · failed");
    feed(&mut ed, " Wj");
    assert_eq!(ed.sessions_hand(), Some(RowId::Helper { of: ed.workspace.id, kind: Kind::Review }));
    // The next turn: they go — the reviewer's stays while its end is unseen
    assert!(crate::chat::submit(&mut ed, "and the tests", None, None), "a turn began");
    assert_eq!(named(&ed), ["main", "fix"]);
    assert_eq!(ed.sessions_here(), Some(ed.workspace.id), "its session's row in hand");
    ed.workspace.reviewer.as_mut().unwrap().unseen = true;
    assert_eq!(named(&ed), ["main", "└─ reviewer · done · 2 findings", "fix"]);
    // (fix's row in hand as its reviewer's goes — not the first row)
    let fix = ed.workspaces[0].id;
    let r = crate::reviewer::Reviewer::for_test(dir.join("fix"), crate::reviewer::State::Done, vec![], "");
    ed.workspaces[0].reviewer = Some(r);
    ed.session_list();
    feed(&mut ed, "G");
    assert_eq!(ed.sessions_hand(), Some(RowId::Helper { of: fix, kind: Kind::Review }));
    ed.workspaces[0].reviewer = None;
    assert_eq!(ed.sessions_here(), Some(fix));
}

/// `enter` on a helper's row goes where its work is (the one list L4), its session to the front first: a reviewer's
/// to its notes and verdict (`space v r`'s buffer), `space i`'s to its edit review (its buffer in front, the keys the
/// code's — waiting, its card's peek there), a search's to its stop — its list, as `]r` shows it; a stop gone, the chat and words.
#[test]
fn enter_on_a_helper_row_goes_where_its_work_is() {
    let dir = crate::scratch::Scratch::new("sessionenter");
    let mut ed = helped(&dir);
    let fix = ed.workspaces[0].id;
    let r = crate::reviewer::Reviewer::for_test(dir.join("fix"), crate::reviewer::State::Done, vec![], "");
    ed.workspaces[0].reviewer = Some(r);
    // (fix's reviewer: fix to the front, its verdict)
    feed(&mut ed, " W");
    let at = ed
        .sessions_row_ids()
        .iter()
        .position(|&n| RowId::of(n) == RowId::Helper { of: fix, kind: Kind::Review });
    assert_eq!(at, Some(2), "under fix");
    feed(&mut ed, "jj<ret>");
    assert_eq!(ed.workspace.id, fix);
    assert!(ed.sessions_pane.is_none(), "the list used");
    assert_eq!(ed.doc().title.as_deref(), Some("reviewer"), "its verdict's buffer");
    // (main's `space i`: its buffer in front, the code with the keys)
    assert!(ed.workspace_switch(WorkspaceId::HOME).is_in());
    let file = ed.doc().id;
    asking(&mut ed, "upper it");
    ed.open_virtual("elsewhere", "x\n");
    ed.workspace.set_at(Here::Chat);
    ed.session_list();
    feed(&mut ed, "j<ret>");
    assert_eq!((ed.doc().id, ed.keys_to().region), (file, crate::uistate::Region::Code));
    assert!(ed.workspace.claude.review.is_some(), "its review there");
    // (a search: its stop)
    crate::llm::cancel(&mut ed);
    searching(&mut ed, 1, "tokio release");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.follow.new_turn();
    let url = crate::web::search::page_url("tokio release");
    let page = Some(crate::replay::PageAt { url, copy: None });
    let look = crate::chat::Look {
        verb: "web search",
        what: "tokio release".into(),
        pattern: false,
        range: String::new(),
        link: None,
    };
    let (path, lines, hunks, why, map) = (None, None, Vec::new(), String::new(), Default::default());
    c.follow.replay.add(crate::replay::Stop { call: "s1".into(), look, path, lines, hunks, why, page, map });
    let search = |ed: &Editor| {
        let ids = ed.sessions_row_ids();
        ids.iter().position(|&n| RowId::of(n) == RowId::Helper { of: WorkspaceId::HOME, kind: Kind::Search })
    };
    feed(&mut ed, " Wgg");
    for _ in 0..search(&ed).unwrap() {
        feed(&mut ed, "j");
    }
    feed(&mut ed, "<ret>");
    let at = ed.workspace.claude.chat.as_ref().unwrap().follow.replay.at;
    assert!(at.is_some(), "its stop shown");
    // (its stop gone: the chat, said)
    ed.workspace.claude.chat.as_mut().unwrap().follow.new_turn();
    ed.workspace.claude.chat.as_mut().unwrap().follow.replay = Default::default();
    feed(&mut ed, " Wgg");
    for _ in 0..search(&ed).unwrap() {
        feed(&mut ed, "j");
    }
    feed(&mut ed, "<ret>");
    let said = ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
    assert_eq!(said, "its search's stop is gone — the chat has its call");
}

/// Floating, the row in hand's card stands just outside the frame, below (the one list L3): while its row shows less
/// than it (a name cut) — none for a row whole and in sight — and, the wheel having taken the row out of sight, still
/// naming it there (no pinned row); `j`/`k` take it to the next row's; without the keys, none.
#[test]
fn the_row_in_hands_card_names_it_under_the_frame_in_sight_or_not() {
    let dir = crate::scratch::Scratch::new("sessioncard");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    for i in 0..19 {
        ed.workspace_add(dir.join(format!("s{i:02}")));
    }
    let long = "l".repeat(90);
    ed.workspace_add(dir.join(&long));
    feed_at(&mut ed, " Wgg", 80, 24);
    let (c, s) = hand_card(&mut ed, 80, 24);
    assert_eq!(c, None, "a row whole and in sight: no card\n{s}");
    // The long one: its name cut on its row, whole on the card
    feed_at(&mut ed, "G", 80, 24);
    let (c, s) = hand_card(&mut ed, 80, 24);
    assert_eq!(c.as_deref(), Some(long.as_str()), "its name whole\n{s}");
    assert!(s.contains("lll…"), "cut on its row\n{s}");
    // The wheel takes it out of sight: the card still names it
    crate::floatlist::wheel(&mut ed, GroupKind::Sessions, true);
    hand_card(&mut ed, 80, 24);
    crate::floatlist::wheel(&mut ed, GroupKind::Sessions, true);
    let (c, s) = hand_card(&mut ed, 80, 24);
    assert!(!s.contains("lll…"), "out of sight\n{s}");
    assert_eq!(c.as_deref(), Some(long.as_str()), "named out of sight\n{s}");
    // A row whole, out of sight: named too
    feed_at(&mut ed, "gg", 80, 24);
    assert_eq!(hand_card(&mut ed, 80, 24).0, None, "back in sight, whole");
    crate::floatlist::wheel(&mut ed, GroupKind::Sessions, false);
    hand_card(&mut ed, 80, 24);
    crate::floatlist::wheel(&mut ed, GroupKind::Sessions, false);
    let (c, s) = hand_card(&mut ed, 80, 24);
    assert!(!s.contains("▎ main"), "main out of sight\n{s}");
    assert_eq!(c.as_deref(), Some("main"), "named out of sight\n{s}");
    // `j` brings the view back to the next row — in sight, whole: no card
    feed_at(&mut ed, "j", 80, 24);
    let (c, s) = hand_card(&mut ed, 80, 24);
    assert_eq!((here_name(&ed).as_str(), c), ("s00", None), "{s}");
    // Without the keys: none
    feed_at(&mut ed, "G<esc>", 80, 24);
    let (c, s) = hand_card(&mut ed, 80, 24);
    assert!(c.is_none() && s.contains("─ sessions"), "no card without the keys\n{s}");
}

/// Docked, the row in hand's card sits at the foot above its keys while the list has them (the one list L3): as tall as
/// the most rows any row's card takes, up to `CARD_ROWS`, so `j`/`k` never changes the rows in sight; it goes above
/// the asks with the keys when they stand over the foot; the keys elsewhere, the rows have the room back.
#[test]
fn a_docked_lists_row_in_hand_has_its_card_at_the_foot_above_the_keys() {
    use crate::offer::{Offer, What};
    let dir = crate::scratch::Scratch::new("sessiondockcard");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    for i in 0..5 {
        ed.workspace_add(dir.join(format!("s{i:02}")));
    }
    let long = "l".repeat(200);
    ed.workspace_add(dir.join(&long));
    feed(&mut ed, " W<C-w>Lgg");
    let rows_in_sight = |ed: &Editor| {
        let lists = ed.screen.lists.borrow();
        lists.iter().find(|l| l.0 == GroupKind::Sessions).map(|l| l.1.1).expect("drawn")
    };
    // (the leaf's rows: its text from the bar's column, and the row its keys are on)
    let foot = |ed: &mut Editor| -> (Vec<String>, usize, String) {
        let s = frame(ed, 160, 30);
        let (_, r) = ed.dock_rect(GroupKind::Sessions).unwrap_or_else(|| panic!("docked\n{s}"));
        // (from the bar's column: the line between the code and the leaf left out)
        let leaf: Vec<String> = (s.lines())
            .map(|l| l.chars().skip(r.x).take(r.w).collect::<String>().trim_start_matches('│').to_string())
            .collect();
        let keys = (r.y..r.y + r.h)
            .find(|&y| leaf[y].contains("enter switch"))
            .unwrap_or_else(|| panic!("keys\n{s}"));
        (leaf, keys, s)
    };
    let (leaf, keys, s) = foot(&mut ed);
    let n = rows_in_sight(&ed);
    let y0 = ed.screen.lists.borrow().iter().find(|l| l.0 == GroupKind::Sessions).map(|l| l.1.0).unwrap();
    assert_eq!(y0 + n, keys - 3, "its rows end over its card\n{s}");
    // (the long name would take four of its card's rows: three over the keys kept; main's card one line, at their foot)
    let blank = |l: &str| l.trim().is_empty();
    assert!(
        blank(&leaf[keys - 3]) && blank(&leaf[keys - 2]) && leaf[keys - 1].starts_with("  main"),
        "main's\n{s}"
    );
    feed(&mut ed, "j");
    let (leaf, keys, s) = foot(&mut ed);
    assert!(leaf[keys - 1].starts_with(&format!("  {}", here_name(&ed))), "the next row's\n{s}");
    assert_eq!(rows_in_sight(&ed), n, "the rows in sight the same");
    feed(&mut ed, "G");
    let (leaf, keys, s) = foot(&mut ed);
    assert!((keys - 3..keys).all(|y| leaf[y].starts_with("  lll")), "wrapped\n{s}");
    assert!(leaf[keys - 1].trim_end().ends_with("l…"), "its last row cut\n{s}");
    let spot = ed.screen.cut_spots.borrow().iter().any(|c| c.0 == keys - 1);
    assert!(spot, "its rest whole under the mouse");
    assert_eq!(rows_in_sight(&ed), n, "the rows in sight the same");
    // An ask over its foot: the asks, then the keys, then the card, then the rows
    let what = What::Asks { call: crate::tools::Call::test(1) };
    ed.push_offer(Offer::new("Claude asks".into(), "to see the screen", "", String::new(), what));
    let (leaf, keys, s) = foot(&mut ed);
    let (_, y, _, _) = ed.screen.offer_card.expect("drawn");
    assert_eq!(keys, y - 2, "its keys over the asks, a row of air\n{s}");
    assert!((keys - 3..keys).all(|y| leaf[y].starts_with("  lll")), "the card over them\n{s}");
    // The keys elsewhere: no card, no keys — the rows have the room
    ed.offers.clear();
    feed(&mut ed, "<esc>");
    let s = frame(&mut ed, 160, 30);
    assert!(rows_in_sight(&ed) > n && !s.contains("enter switch"), "the rows have its room\n{s}");
}

/// Docked, the row in hand's card is as tall as its own lines (the design session, 2026-11-16: two lines of text in
/// three rows of wash): its room stays the tallest card's, so `j`/`k` never changes the rows in sight, but only the
/// card's lines are washed, at the room's foot over the keys — the rest is the pane's ground.
#[test]
fn a_docked_lists_card_is_as_tall_as_its_lines() {
    crossterm::style::force_color_output(true);
    let dir = crate::scratch::Scratch::new("sessiondockcardh");
    let mut ed = Editor::new(crate::config::Config::default());
    (ed.workspace.root, ed.workspace.name) = (dir.to_path_buf(), "main".into());
    // (a card of two lines and one of three — more, cut to `CARD_ROWS`)
    ed.workspace_add(dir.join("m".repeat(70)));
    ed.workspace_add(dir.join("l".repeat(200)));
    feed(&mut ed, " W<C-w>L");
    let (w, h) = (160u16, 30u16);
    // (the band over the keys: each of its rows' text, and whether it's washed — the row in hand's ground, not the
    // keys' line's)
    let band = |ed: &mut Editor| -> (Vec<(String, bool)>, usize, String) {
        // (no toast over it)
        ed.toasts.clear();
        let mut bytes = Vec::new();
        ed.focus_sync();
        crate::term::render(ed, &mut bytes, w, h).unwrap();
        let mut vt = vt100::Parser::new(h, w, 0);
        vt.process(&bytes);
        let s = vt.screen().contents();
        let (_, r) = ed.dock_rect(GroupKind::Sessions).unwrap_or_else(|| panic!("docked\n{s}"));
        let text =
            |y: usize| vt.screen().contents_between(y as u16, r.x as u16, y as u16, (r.x + r.w) as u16);
        let keys = (r.y..r.y + r.h).find(|&y| text(y).contains("enter switch")).expect("its keys");
        let bg = |y: usize| vt.screen().cell(y as u16, (r.x + 1) as u16).unwrap().bgcolor();
        let rows =
            (keys - crate::sessionpane::CARD_ROWS..keys).map(|y| (text(y), bg(y) != bg(keys))).collect();
        let n = ed.screen.lists.borrow().iter().find(|l| l.0 == GroupKind::Sessions).map(|l| l.1.1).unwrap();
        (rows, n, s)
    };
    let mut seen = Vec::new();
    for k in ["", "j", "j"] {
        feed(&mut ed, k);
        let (rows, n, s) = band(&mut ed);
        // (the line between the code and the leaf left out)
        let lines = rows.iter().filter(|r| !r.0.trim_start_matches('│').trim().is_empty()).count();
        let washed: Vec<bool> = rows.iter().map(|r| r.1).collect();
        let want: Vec<bool> = (0..rows.len()).map(|i| i >= rows.len() - lines).collect();
        assert_eq!(
            washed,
            want,
            "{} — washed as its {lines} lines, at the foot: {rows:?}\n{s}",
            here_name(&ed)
        );
        seen.push((lines, n));
    }
    assert_eq!(seen.iter().map(|x| x.0).collect::<Vec<_>>(), [1, 2, 3], "main's, the 70's, the 200's");
    assert!(seen.iter().all(|x| x.1 == seen[0].1), "the rows in sight the same: {seen:?}");
}
