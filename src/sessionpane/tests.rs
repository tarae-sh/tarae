use super::*;
use crate::key::{Code, Key};
use crate::workspace::{Doing, Here};
use crate::worktree::{Facts, Shown};

/// A session as the list says it, every column filled.
fn full(id: WorkspaceId, name: &str, doing: Doing) -> Shown {
    Shown {
        id,
        name: name.into(),
        child: false,
        branch: Some(format!("{name}-branch")),
        facts: Some(Facts { ahead: 2, behind: 3, changed: 4 }),
        doing,
        front: id == WorkspaceId::HOME,
        usage: Some((12.0, 300 << 20)),
        rebasing: None,
        reloaded: 1,
        compacted: true,
        reviewer: "¶ reviewing".into(),
        claude: Some(("● warm until 14:32".into(), "$0.42".into())),
        billing: crate::billing::Billing::Api,
        build: Some((2 << 20, true)),
        queue: None,
        at: "src/a.rs:12 · editing".into(),
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

/// The manager's row (P3 step 7, §3.6): its state in the strip's words and inks — who waits, never what (polish landing
/// 1: no count of its cards, the stack's peeks say them) —, whom it overlaps (each name its colour), `↓N` behind, its
/// context (warning past 80 %), its last report line, then the rest the list always said — empty cells left out.
#[test]
fn a_row_says_what_the_manager_needs() {
    let r = row(&full(WorkspaceId::here(1), "fix", Doing::Waiting));
    assert_eq!(r.cell(Col::State), [("▲ waits".to_string(), Ink::Warn)]);
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
    assert!(r.cell(Col::State).is_empty() && r.cell(Col::Overlap).is_empty());
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

/// The list's width at 160 (the frame 96 wide) and at 80 (48): whole columns fold in `FOLDS`' order — the rest first,
/// then the report (cut to what's there before it goes), the context, behind, the overlaps; the state last, the name
/// never. Every narrower room shows a subset of the wider's.
#[test]
fn the_row_folds_at_80_in_its_order() {
    let rows = [
        row(&full(WorkspaceId::HOME, "main", Doing::Working)),
        row(&full(WorkspaceId::here(1), "fix-login", Doing::Waiting)),
    ];
    // (the rows' room in a frame `w` wide: its sides, the bar's cells, a cell before the side)
    let room = |w: usize| w - 2 - 2 - 1;
    let wide = lay(&rows, 400);
    assert_eq!(cols(&wide), COLS, "every column with room for all");
    let at160 = lay(&rows, room(96));
    let shown = cols(&at160);
    assert!(
        [Col::State, Col::Overlap, Col::Behind, Col::Context, Col::Report].iter().all(|c| shown.contains(c)),
        "at 160 the manager's columns all: {shown:?}"
    );
    // (the cost after the report: a parent's children's total stays in sight while the report is cut — user, 2026-10-29)
    assert!(shown.contains(&Col::Cost), "at 160 the cost stays: {shown:?}");
    let report_gone = (0..400).rev().map(|r| cols(&lay(&rows, r))).find(|c| !c.contains(&Col::Report));
    assert!(report_gone.is_some_and(|c| c.contains(&Col::Cost)), "the report goes before the cost");
    let at80 = cols(&lay(&rows, room(48)));
    assert!(!at80.contains(&Col::Report), "at 80 the report first: {at80:?}");
    assert!(at80.contains(&Col::State), "the state stays: {at80:?}");
    // (from wide to nothing, one room at a time: what goes, goes in `FOLDS`' order, and never comes back)
    let mut gone: Vec<Col> = Vec::new();
    let mut was = cols(&wide);
    for r in (0..400).rev() {
        let now = cols(&lay(&rows, r));
        assert!(now.iter().all(|c| was.contains(c)), "{r}: {now:?} ⊄ {was:?}");
        gone.extend(was.iter().filter(|c| !now.contains(c)));
        was = now;
    }
    assert_eq!(gone, FOLDS, "the order they fold in");
    // (the report cut before it goes, once the rest went: to its least)
    // (from wide to narrow: the first room it's cut in)
    let cut = (0..400)
        .rev()
        .map(|r| lay(&rows, r))
        .find(|l| l.1.iter().any(|c| c.0 == Col::Report && c.1 < REPORT_MAX));
    let cut = cut.expect("cut once").1;
    let report_at = FOLDS.iter().position(|&c| c == Col::Report).unwrap();
    assert!(cut.iter().all(|c| !FOLDS[..report_at].contains(&c.0)), "only once the rest went: {cut:?}");
    let (name_w, _) = lay(&rows, 3);
    assert_eq!(name_w, 3, "the names cut to the room, never gone");
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
    assert!(top.contains("╭─ sessions ── 1 session") && top.contains("enter switch"), "{top}");
    assert!(s.lines().nth(f.y + 1).unwrap().contains("▎ main"), "its row, selected");
    feed(&mut ed, "<esc>");
    assert!(ed.sessions_pane.is_some() && ed.workspace.here() == Here::Code, "esc: the keys back, it stays");
    assert!(
        !frame(&mut ed, 160, 40).lines().nth(f.y).unwrap().contains("enter switch"),
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
    let status = rows.last().copied().unwrap_or_default();
    assert!(status.contains("C-q answer") && !status.contains("waiting"), "{status}");
    // A press on the status line's way in: it stands whole, the keys its
    let spot = (ed.screen.word_spots.borrow().iter())
        .find(|w| w.on == crate::mouse::Surface::Status && w.press == crate::wordspot::Press::Card)
        .cloned()
        .unwrap_or_else(|| panic!("`C-q answer` a button: {status}"));
    for kind in [crate::event::MouseKind::Down, crate::event::MouseKind::Up] {
        let (x, y) = (((spot.from + spot.to) / 2) as u16, spot.y as u16);
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
/// after main's children, its own child after it — a faint `›` before its name; `j`/`k` and the digits go by that
/// order, the one drawn.
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
    add(&mut ed, "one", Some(main));
    add(&mut ed, "two", Some(main));
    add(&mut ed, "mend", Some(fix));
    let order = ["main", "one", "two", "fix", "mend"];
    let rows = ed.sessions_rows();
    let named: Vec<(&str, bool)> = rows.iter().map(|r| (r.name.as_str(), r.child)).collect();
    assert_eq!(named, [("main", false), ("one", true), ("two", true), ("fix", false), ("mend", true)]);
    feed(&mut ed, " W");
    let s = frame(&mut ed, 160, 40);
    let drawn: Vec<&str> = (s.lines())
        .filter_map(|l| order.iter().find(|n| l.contains(&format!(" {n} ")) && l.contains('│')).copied())
        .collect();
    assert_eq!(drawn, order, "drawn in that order:\n{s}");
    assert!(s.lines().any(|l| l.contains("› one ")) && !s.contains("› fix"), "{s}");
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
    feed(&mut ed, "4");
    assert_eq!(ed.workspace.name, "fix", "4: the fourth row drawn");
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

/// A list floats 3/5 wide and as tall as its rows, up to half the screen (user, 2026-10-23), its top on the line every
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
    assert_eq!(rect(&mut ed), crate::split::Rect { x: 32, y: 12, w: 100, h: 6 }, "moved 2, wider 4, grown");
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
