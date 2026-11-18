//! The memo's step (c): docked beside the code with `C-w e`, floating again with it — its group in `Workspace.groups` for
//! its placement alone (`.sb/memo-c-dev-note.md`).

use super::*;

/// A frame `w` × `h`, as the main loop draws one, as text.
fn frame_at(ed: &mut Editor, w: u16, h: u16) -> Vec<String> {
    ed.focus_sync();
    let mut buf = Vec::new();
    crate::term::render(ed, &mut buf, w, h).unwrap();
    crate::screen::text(&buf, w, h).unwrap().lines().map(str::to_string).collect()
}

/// Keys as `feed` types them, a frame `w` × `h` after each.
fn feed_at(ed: &mut Editor, keys: &str, w: u16, h: u16) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(key);
        frame_at(ed, w, h);
    }
}

/// A frame at `W` × `H` as a terminal shows it: its cells and their colours.
fn screen(ed: &mut Editor) -> vt100::Parser {
    ed.focus_sync();
    crossterm::style::force_color_output(true);
    let mut buf = Vec::new();
    crate::term::render(ed, &mut buf, W, H).unwrap();
    let mut p = vt100::Parser::new(H, W, 0);
    p.process(&buf);
    p
}

/// The memo's leaf as last drawn docked (its edge's cell in it), None while it isn't.
fn docked(ed: &Editor) -> Option<Rect> {
    ed.screen.docked.borrow().iter().find(|d| d.0 == GroupKind::Memo).map(|d| d.1)
}

/// Row `y` of `rows` from cell `x` on.
fn from(rows: &[String], y: usize, x: usize) -> String {
    rows[y].chars().skip(x).collect()
}

/// The last thing said.
fn said(ed: &Editor) -> String {
    ed.toasts.last().map(|t| t.text.clone()).unwrap_or_default()
}

/// `C-w e` from the memo docks it right of the code, at the screen's edge, its measure wide — the keys staying, its
/// keys at its foot; leaving it (`C-w w`, a press on the code) it stays where it is, to be read, no mark; `space M`, a
/// press on its text and `C-w w`'s round give it the keys back where it stands; `space M` from it puts it away — its leaf
/// gone, the mark said, its side kept for the next `space M`; `C-w e` again floats it, and the float's rule is back.
#[test]
fn c_w_e_docks_the_memo_beside_the_code_where_it_stays() {
    let dir = crate::scratch::Scratch::new("memo-dock");
    let mut ed = editor(&dir);
    feed(&mut ed, " Mone<ret>two<esc><C-w>e");
    assert_eq!(said(&ed), "memo docked right — C-w e floats it");
    let r = docked(&ed).expect("docked");
    assert!(drawn(&ed).is_none(), "nothing floats");
    assert_eq!((r.x + r.w, r.w), (W as usize, DOCK_W), "right of the code at the edge, its measure wide");
    assert!(ed.memo_has_keys(), "the keys stayed");
    let rows = frame(&mut ed);
    assert!(from(&rows, r.y, r.x).contains("memo"), "{}", rows[r.y]);
    assert!(from(&rows, r.y + 1, r.x + 2).starts_with("one"), "{}", rows[r.y + 1]);
    assert!(from(&rows, r.y + 2, r.x + 2).starts_with("two"), "{}", rows[r.y + 2]);
    let foot = from(&rows, r.y + r.h - 1, r.x);
    assert!(
        foot.contains("space M put away  C-w e float  C-w w code"),
        "its foot, all three in order: {foot}"
    );
    // (the code beside it, never under it)
    let code = ed.screen.leaves.iter().find(|l| ed.workspace.views.iter().any(|v| v.id == l.0)).unwrap().1;
    assert!(code.x + code.w <= r.x, "the code {code:?} left of it {r:?}");
    // Left by `C-w w`: there still, no mark, no keys at its foot
    feed(&mut ed, "<C-w>w");
    assert!(!ed.memo_has_keys() && ed.memo_up() && docked(&ed) == Some(r), "it stays");
    assert!(!ed.memo_marked(), "in sight: no mark");
    let rows = frame(&mut ed);
    assert!(!from(&rows, r.y + r.h - 1, r.x).contains("space M"), "its keys only with the keys");
    assert!(!rows[H as usize - 1].contains(crate::term::MEMO_MARK), "{}", rows[H as usize - 1]);
    // `space M` from the code: the keys, typing at the end of its words, where it stands
    feed(&mut ed, " M");
    let m = ed.workspace.memo.as_ref().unwrap();
    assert!(ed.memo_has_keys() && m.mode() == Mode::Insert && docked(&ed) == Some(r));
    assert_eq!(m.pane.doc_in().unwrap().selection().primary().head, "one\ntwo".len());
    // A press on the code: the code's keys, the memo there still
    press(&mut ed, MouseKind::Down, 10, 5);
    press(&mut ed, MouseKind::Up, 10, 5);
    assert!(!ed.memo_has_keys() && docked(&ed) == Some(r), "{:?}", ed.keys_to().region);
    // A press on its text: the keys, the cursor there
    press(&mut ed, MouseKind::Down, r.x + 2, r.y + 2);
    press(&mut ed, MouseKind::Up, r.x + 2, r.y + 2);
    assert!(ed.memo_has_keys(), "{:?}", ed.keys_to().region);
    let head = ed.workspace.memo.as_ref().unwrap().pane.doc_in().unwrap().selection().primary().head;
    assert_eq!(head, "one\n".len(), "on `two`'s first character");
    // `C-w w`'s round: the code, then the memo again
    feed(&mut ed, "<esc><C-w>w");
    assert_eq!(ed.keys_to().region, Region::Code);
    feed(&mut ed, "<C-w>w");
    assert!(ed.memo_has_keys(), "the round lands on it: {:?}", ed.keys_to().region);
    // `space M` from it: put away — its leaf gone, the mark, its side kept
    feed(&mut ed, "<esc> M");
    assert!(!ed.memo_up() && docked(&ed).is_none() && drawn(&ed).is_none(), "put away");
    assert!(ed.workspace.group(GroupKind::Memo).is_none(), "no group while put away");
    assert!(ed.memo_marked() && frame(&mut ed)[H as usize - 1].contains(crate::term::MEMO_MARK));
    feed(&mut ed, " M");
    assert!(ed.memo_has_keys() && docked(&ed) == Some(r), "back where it was");
    // `C-w q` from it: put away as `space M` puts it — never the code pane beside it closed
    let panes = ed.workspace.views.len();
    feed(&mut ed, "<esc><C-w>q");
    assert!(
        !ed.memo_up() && docked(&ed).is_none() && ed.workspace.views.len() == panes,
        "put away, the code kept"
    );
    feed(&mut ed, " M");
    assert!(ed.memo_has_keys() && docked(&ed) == Some(r));
    // `C-w e` again: floating, the keys its; left, put away
    feed(&mut ed, "<esc><C-w>e");
    assert_eq!(said(&ed), "memo floating — C-w e docks it, C-w m moves it");
    assert!(drawn(&ed).is_some() && docked(&ed).is_none() && ed.memo_has_keys());
    assert!(ed.workspace.group(GroupKind::Memo).is_none(), "floating, no group");
    feed(&mut ed, "<C-w>w");
    assert!(!ed.memo_up() && drawn(&ed).is_none(), "floating, left: put away");
}

/// Docked without the keys it's drawn as the chat and the code are without them: its words at full ink, to be read —
/// only its line goes unlit; no ground of its own (look B is the float's).
#[test]
fn docked_without_the_keys_its_words_keep_their_ink() {
    let dir = crate::scratch::Scratch::new("memo-dock-ink");
    for theme in ["meok", "hanji"] {
        let mut ed = editor(&dir);
        ed.theme = crate::theme::load(theme).unwrap();
        feed(&mut ed, " Mone<esc><C-w>e");
        let r = docked(&ed).expect("docked");
        let (word, line) = ((r.y + 1, r.x + 2), (r.y, r.x + 1));
        let cell =
            |p: &vt100::Parser, (y, x): (usize, usize)| p.screen().cell(y as u16, x as u16).unwrap().clone();
        let with = screen(&mut ed);
        assert_eq!(cell(&with, word).contents(), "o");
        feed(&mut ed, "<C-w>w");
        let without = screen(&mut ed);
        assert_eq!(cell(&without, word).contents(), "o");
        assert_eq!(cell(&without, word).fgcolor(), cell(&with, word).fgcolor(), "{theme}: its words' ink");
        assert_ne!(cell(&without, line).fgcolor(), cell(&with, line).fgcolor(), "{theme}: its line unlit");
        // (the code's ground past its file's end, off its cursor's line)
        let code_bg = cell(&without, (r.y + 10, 20)).bgcolor();
        assert_eq!(cell(&without, word).bgcolor(), code_bg, "{theme}: the pane's ground, none of its own");
    }
}

/// `C-w e` where docking has no room to draw it — 80 columns: the code would keep less than `CODE_MIN` — says so in the
/// act's own words, and it floats while you use it; left, it's put away and the mark says it. Docked in sight, then a
/// screen grown narrow: nothing drawn, the mark says it's there; wide again, it's back.
#[test]
fn c_w_e_with_no_room_says_so() {
    let dir = crate::scratch::Scratch::new("memo-dock-room");
    let mut ed = editor(&dir);
    let mark = |rows: &[String]| rows.last().is_some_and(|l| l.contains(crate::term::MEMO_MARK));
    frame_at(&mut ed, 80, 34);
    feed_at(&mut ed, " Mone<esc><C-w>e", 80, 34);
    assert_eq!(said(&ed), "the memo is docked, with no room to draw it here: it floats while you use it");
    assert!(ed.memo_docked() && docked(&ed).is_none() && drawn(&ed).is_some(), "floating while used");
    // (its bottom line names what `C-w e` changes now: docked, with no room — `undock`)
    let r = drawn(&ed).unwrap();
    let bottom = from(&frame_at(&mut ed, 80, 34), r.y + r.h - 1, r.x);
    assert!(bottom.contains("space M put away  C-w e undock"), "{bottom}");
    feed_at(&mut ed, "<C-w>w", 80, 34);
    assert!(!ed.memo_up() && drawn(&ed).is_none());
    assert!(mark(&frame_at(&mut ed, 80, 34)), "put away: the mark");
    feed_at(&mut ed, " M", 80, 34);
    assert!(ed.memo_has_keys() && drawn(&ed).is_some(), "up again, floating");
    // Wide: docked; left there, then the screen narrow — not drawn, the mark; wide again, there
    feed_at(&mut ed, "<esc><C-w>w", 120, 40);
    feed_at(&mut ed, " M<esc><C-w>w", 120, 40);
    assert!(docked(&ed).is_some() && !ed.memo_has_keys(), "docked, the code's keys");
    let rows = frame_at(&mut ed, 80, 34);
    assert!(docked(&ed).is_none() && drawn(&ed).is_none() && mark(&rows), "{:?}", rows.last());
}

/// Docked it's in sight during a chat, and still in no agent's context (the plan's point 7, held for the float as for
/// the dock): its words in neither the message Claude is sent (its `editor-context`, the `ui:` line), nor Claude's state,
/// nor the drivers' (`tarae state`), nor any element pointed at. The screen sent (`C-s`) is the one way they travel,
/// as decided: what's in sight goes with it.
#[test]
fn floating_or_docked_its_words_reach_no_agent() {
    const WORDS: &str = "zebra-quartz-memo";
    let dir = crate::scratch::Scratch::new("memo-dock-agent");
    let mut ed = editor(&dir);
    let mut c = crate::chat::Chat::new();
    let rx = crate::chat::stand_in(&mut c);
    ed.workspace.claude.chat = Some(c);
    // (wide: the chat and the memo docked beside the code, both with room)
    let (w, h) = (160, 44);
    feed_at(&mut ed, &format!(" M{WORDS}<esc>"), w, h);
    let heard = |s: &str| s.contains(WORDS);
    for (form, keys) in [("floating", ""), ("docked", "<esc><C-w>e<C-w>w")] {
        // (the message before took the keys to the chat: the memo up again with them)
        if !ed.memo_has_keys() {
            ed.memo_bring();
        }
        feed_at(&mut ed, keys, w, h);
        frame_at(&mut ed, w, h);
        assert!(ed.memo_in_sight(), "{form}: in sight");
        let ask = format!("what is here, {form}?");
        assert!(crate::chat::submit(&mut ed, &ask, None, None), "{form}: sent");
        let sent: Vec<String> = rx.try_iter().filter(|l| l.contains(&ask)).collect();
        crate::chat::stand_in_done(&mut ed);
        assert_eq!(sent.len(), 1, "{form}: one message");
        assert!(!heard(&sent[0]), "{form}: the message holds its words: {}", sent[0]);
        assert!(!heard(&ed.ui_state_claude().text_claude(ed.behind)), "{form}: Claude's state");
        assert!(!heard(&ed.ui_state().text()), "{form}: the drivers' state");
        for t in ed.point_targets() {
            let p = crate::chat::pointed_said(&ed, &t.id, &[]);
            assert!(!heard(&p), "{form}: pointed at {}: {p}", t.id);
        }
    }
    assert!(ed.memo_docked_shown());
    crate::chat::attach_screen(&mut ed).unwrap();
    let shot = ed.workspace.claude.chat.as_ref().and_then(|c| c.screen.as_ref()).map(|s| s.text.clone());
    assert!(shot.is_some_and(|s| heard(&s)), "the screen sent shows what's in sight");
}

/// Step (d): a memo longer than its room shows that it scrolls as the code's pane does, its bar in the blank column after
/// its text — floating once it's grown to half the screen, docked once its lines pass its height; none while it fits.
/// The view follows the cursor while it has the keys; docked without them it stays where it was left, a screen's height
/// changing under it too.
#[test]
fn a_memo_longer_than_its_room_shows_the_code_panes_bar() {
    let dir = crate::scratch::Scratch::new("memo-long");
    let mut ed = editor(&dir);
    let thumb = |rows: &[String], x: usize, ys: std::ops::Range<usize>| {
        ys.filter(|&y| rows[y].chars().nth(x).is_some_and(|c| c.to_string() == crate::term::THUMB)).count()
    };
    // Floating: three lines fit, no bar; forty grow it to half the screen, then it scrolls — the bar, the cursor's row
    feed(&mut ed, " Mone<ret>two<ret>three");
    let r = drawn(&ed).unwrap();
    let bar_x = r.x + PAD + text_w(r.w);
    assert_eq!(thumb(&frame(&mut ed), bar_x, r.y + 1..r.y + r.h - 1), 0, "it fits: no bar");
    let many: String = (4..=40).map(|i| format!("<ret>line {i}")).collect();
    feed(&mut ed, &many);
    let r = drawn(&ed).unwrap();
    let rows = frame(&mut ed);
    assert!(thumb(&rows, bar_x, r.y + 1..r.y + r.h - 1) > 0, "floating, longer than its room: the bar");
    assert!(rows[r.y + r.h - 2].contains("line 40"), "the cursor's row in sight: {}", rows[r.y + r.h - 2]);
    // Docked: its leaf's height passed — the bar in its last column; at its words' end, in sight
    let more: String = (41..=60).map(|i| format!("<ret>line {i}")).collect();
    feed(&mut ed, &format!("{more}<esc><C-w>e"));
    let d = docked(&ed).expect("docked");
    let rows = frame(&mut ed);
    let last = d.y + crate::memo::memo_docked_rows(d.h);
    assert!(thumb(&rows, d.x + d.w - 1, d.y + 1..last + 1) > 0, "docked, the bar");
    assert!(from(&rows, last, d.x).contains("line 60"), "{}", rows[last]);
    // Without the keys it stays where it was left — the screen shorter under it too
    feed(&mut ed, "<C-w>w");
    let was = ed.workspace.memo.as_ref().unwrap().scroll.get();
    let rows = frame_at(&mut ed, W, 30);
    assert_eq!(ed.workspace.memo.as_ref().unwrap().scroll.get(), was, "its view where it was");
    let d = docked(&ed).expect("docked");
    assert!(!rows.iter().any(|l| l.contains("line 60")), "the cursor's row not followed without the keys");
    // With them again (`space M`: at the end of its words), it follows
    feed_at(&mut ed, " M", W, 30);
    let rows = frame_at(&mut ed, W, 30);
    assert!(from(&rows, d.y + crate::memo::memo_docked_rows(d.h), d.x).contains("line 60"));
}

/// Step (d), the mouse on the memo, each as its kind's: a drag on its floating frame's top line moves it; a drag on its
/// docked line sizes it (kept as a docked group's size); a drag in its docked words selects them, the keys its.
#[test]
fn the_mouse_moves_its_frame_sizes_its_dock_and_selects_its_words() {
    let dir = crate::scratch::Scratch::new("memo-mouse");
    let mut ed = editor(&dir);
    let drag = |ed: &mut Editor, (x, y): (usize, usize), (x2, y2): (usize, usize)| {
        press(ed, MouseKind::Down, x, y);
        press(ed, MouseKind::Drag, x2, y2);
        press(ed, MouseKind::Up, x2, y2);
    };
    // Floating: its top line, past its name, moves it by the drag
    feed(&mut ed, " Mone<ret>two<esc>");
    let r = drawn(&ed).unwrap();
    drag(&mut ed, (r.x + 20, r.y), (r.x + 10, r.y + 3));
    let moved = drawn(&ed).expect("floating still");
    assert_eq!((moved.x, moved.y, moved.w, moved.h), (r.x - 10, r.y + 3, r.w, r.h), "moved, its size kept");
    assert!(ed.memo_has_keys(), "the keys stay its");
    // Docked: a drag on its line toward the code widens it
    feed(&mut ed, "<C-w>e");
    let d = docked(&ed).expect("docked");
    drag(&mut ed, (d.x, d.y + 10), (d.x - 6, d.y + 10));
    let wider = docked(&ed).expect("docked still");
    assert_eq!((wider.x, wider.w), (d.x - 6, d.w + 6), "its line dragged six cells toward the code");
    assert_eq!(ed.sizes.docked[GroupKind::Memo.dock_index()], Some((d.w + 6) as u16), "kept as its size");
    // A drag in its words: selected
    feed(&mut ed, "<C-w>w");
    let d = wider;
    drag(&mut ed, (d.x + 2, d.y + 2), (d.x + 4, d.y + 2));
    assert!(ed.memo_has_keys(), "the keys its");
    let sel = ed.workspace.memo.as_ref().unwrap().pane.doc().selection().primary();
    assert_eq!((sel.from(), sel.to()), (4, 7), "`two` selected");
}

/// Every walk of `Workspace.groups` in the code, each with what the memo's group does there (`.sb/memo-c-dev-note.md`'s
/// table): its membership is for its placement alone — never a tab, a list, a run panel, a frame of the groups', nor
/// counted. A walk added without a line here fails, so it can't skip saying what the memo does in it.
const GROUP_WALKS: &[(&str, &str)] = &[
    ("dock.rs:dock_sync", "places its leaf while it's up and docked — the reason it's a member"),
    ("dock.rs:dock_own", "makes and drops its group (`memo_dock_sync`)"),
    ("dock.rs:dock_in", "a group docked right too stacks with it in one column, as two docked groups do"),
    (
        "dock.rs:group_in_hand",
        "by a list's keys: the memo's are `Here::Memo`, so `C-w H J K L`, `C-w t` never take it",
    ),
    ("dock.rs:group_set", "by kind: only its own placement"),
    (
        "editor.rs:split_view",
        "its pane maps to its leaf: a split from it lands beside its leaf, as from a member",
    ),
    ("editor.rs:show_here", "as `split_view`: `C-w p` from it beside its leaf"),
    (
        "float.rs:floats_up",
        "left out: floating, the memo is its own pane (`term::floats`), never a group's frame",
    ),
    ("float.rs:float_groups_sync", "floating places only: its group is never floating"),
    ("float.rs:float_top", "its `raised` is 0: nothing"),
    ("float.rs:float_raise", "the kind in hand: never the memo"),
    ("float.rs:float_set", "floating places only"),
    ("float.rs:float_auto", "floating places only — the memo's frame is `memo_auto`'s"),
    ("grouplist.rs:step_in_view", "the preflight's only"),
    ("grouplist.rs:group_made", "by kind: never asked for the memo, whose group `dock_own` alone makes"),
    ("grouplist.rs:groups_front", "a list's or the slot's keys: never the memo's"),
    ("grouplist.rs:stacked_other", "laid groups only: `group_laid` is false for it"),
    ("grouplist.rs:slot_group", "slot kinds only: it has no slot, so it's never a tab"),
    ("grouplist.rs:list_focused", "laid groups only"),
    ("grouplist.rs:hosted_detail", "it has no detail"),
    (
        "restart.rs:layout",
        "restored with the layout, then dropped: the memo starts put away (`memo_dock_sync`)",
    ),
    ("restart.rs:groups_json", "kept in the restart's file while docked and up — dropped on the way back in"),
    ("sizes.rs:laid_along", "its pane maps to its leaf, as a member's"),
    ("sizes.rs:size_pane", "its pane maps to its leaf — `size_dock` takes `C-w < >` from it first"),
    ("sizes.rs:sizes_auto", "its `laid` set automatic: nothing"),
    ("sizes.rs:focus_sync", "a list's keys: never the memo's"),
    ("term.rs:frame_edges", "docked in sight: its edge's line, as any docked leaf's"),
    (
        "term.rs:screen_leaves",
        "no members: its leaf stays, on `C-w w`'s round while docked in sight, gone put away",
    ),
    ("term.rs:slot_on", "slot kinds only"),
    ("term.rs:lay_group", "slot kinds only"),
    ("term.rs:group_shapes", "laid groups only"),
    ("term.rs:stacked_list_rows", "the task's and the preflight's only"),
    ("term.rs:new", "a zoomed group floating: never the memo's"),
    ("term.rs:draw_docked", "draws it docked (`draw_memo`, no frame) — its words never faded"),
    (
        "uistate.rs:panes_state",
        "its leaf listed (`group:memo`, `right`); its own pane no member — floating it's in none",
    ),
    ("workspace.rs:code_at_sync", "a member's pane isn't gone: its pane's keys are `Here::Memo` anyway"),
    ("workspace.rs:shows", "panes first: its pane shows the memo, its leaf `Group(Memo)`"),
    ("workspace.rs:group", "by kind"),
];

/// The walks of `Workspace.groups` in the code, as `file:fn` (test code left out): a line naming it through a workspace
/// (`workspace.`, `ws.`, `l.`, `s.` — and `self.` in `workspace.rs`), its function the nearest `fn` above.
fn group_walks() -> Vec<String> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut dirs = vec![src.clone()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_dir() && name != "tests" {
                dirs.push(p);
            } else if name.ends_with(".rs") && name != "tests.rs" {
                files.push(p);
            }
        }
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = Vec::new();
    for p in files {
        let rel = p.strip_prefix(&src).unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&p).unwrap();
        let lines: Vec<&str> = text.lines().take_while(|l| l.trim() != "mod tests {").collect();
        let mut needles = vec!["workspace.groups", "ws.groups", "l.groups", "s.groups"];
        if rel == "workspace.rs" {
            needles.push("self.groups");
        }
        for (i, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            let hit = needles.iter().any(|n| {
                line.match_indices(n).any(|(at, _)| {
                    !word(line[..at].chars().next_back()) && !word(line[at + n.len()..].chars().next())
                })
            });
            // (a chain broken over lines: `self.workspace` / `.groups`)
            let rest = line.trim_start().strip_prefix(".groups").filter(|r| !word(r.chars().next()));
            let before = lines[..i].iter().rev().map(|l| l.trim_end()).find(|l| !l.is_empty()).unwrap_or("");
            let hit = hit
                || rest.is_some()
                    && needles
                        .iter()
                        .map(|n| n.trim_end_matches(".groups"))
                        .any(|r| before.strip_suffix(r).is_some_and(|b| !word(b.chars().next_back())));
            if !hit {
                continue;
            }
            let f = lines[..=i].iter().rev().find_map(|l| {
                let at = l.find("fn ").filter(|&a| a == 0 || l.as_bytes()[a - 1] == b' ')?;
                let name: String =
                    l[at + 3..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                (!name.is_empty()).then_some(name)
            });
            out.push(format!("{rel}:{}", f.unwrap_or_default()));
        }
    }
    crate::sorting::sort(&mut out);
    out.dedup();
    out
}

#[test]
fn every_walk_of_the_groups_says_what_the_memo_does_there() {
    let found = group_walks();
    let listed: Vec<&str> = GROUP_WALKS.iter().map(|w| w.0).collect();
    let new: Vec<&String> = found.iter().filter(|f| !listed.contains(&f.as_str())).collect();
    assert!(
        new.is_empty(),
        "a walk of `Workspace.groups` not in `GROUP_WALKS` — say what the memo does there: {new:?}"
    );
    let gone: Vec<&&str> = listed.iter().filter(|l| !found.iter().any(|f| f == *l)).collect();
    assert!(gone.is_empty(), "listed in `GROUP_WALKS`, no walk there now: {gone:?}");
    // And what the walks say, met: docked, it's no slot tab, no list or run panel, no group's frame, not in the session's
    // panes but as its leaf
    let dir = crate::scratch::Scratch::new("memo-dock-walks");
    let mut ed = editor(&dir);
    feed(&mut ed, " Mone<esc><C-w>e<C-w>w");
    let g = ed.workspace.group(GroupKind::Memo).expect("its group, docked");
    assert!(!ed.group_laid(&g) && g.detail_pane().is_none() && g.kind.slot().is_none());
    assert!(ed.slot_group().is_none() && ed.list_focused().is_none() && ed.floats_up().is_empty());
    let s = ed.ui_state();
    let panes: Vec<String> = s.text().lines().filter(|l| l.contains("memo")).map(str::to_string).collect();
    assert!(panes.iter().all(|l| !l.contains("list:") && !l.contains("detail:")), "{panes:?}");
}
