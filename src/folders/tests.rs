use super::*;
use crate::key::{Code, Key};
use crate::pane::GroupKind;
use crate::test_results::settle_until;

fn frame(ed: &mut Editor) -> String {
    ed.focus_sync();
    crate::term::render(ed, &mut Vec::new(), 160, 40).unwrap();
    crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), 160, 40).unwrap()
}

fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let k: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(k);
        frame(ed);
    }
}

/// What was said last.
fn said(ed: &Editor) -> String {
    ed.messages.last().map(|m| m.2.clone()).unwrap_or_default()
}

/// `:folder-add <at>`, until it's said — what was said.
fn add(ed: &mut Editor, at: &Path) -> String {
    let before = ed.said;
    feed(ed, &format!(":folder-add {}<ret>", at.display()));
    settle_until(ed, |ed| ed.said > before);
    said(ed)
}

/// A scratch with a project, a folder beside it (with a CLAUDE.md) and a stand-in home holding neither; an editor
/// for the project, its `Caps` made for that home.
fn place(name: &str) -> (crate::scratch::Scratch, PathBuf, PathBuf, Editor) {
    let s = crate::scratch::Scratch::new(&format!("folders-{name}"));
    let (project, infra, home) = (s.join("project"), s.join("infra"), s.join("home"));
    for d in [&project, &infra, &home] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(project.join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(infra.join("main.tf"), "resource {}\n").unwrap();
    std::fs::write(infra.join("CLAUDE.md"), "# infra\n").unwrap();
    let (project, infra) = (project.canonicalize().unwrap(), infra.canonicalize().unwrap());
    let mut ed = Editor::with_root(crate::config::Config::default(), project.clone());
    ed.caps_home = Some(home.canonicalize().unwrap());
    (s, project, infra, ed)
}

/// `:folder-add` makes a folder the front workspace's — its name, its instruction files looked for, said; `:folders`
/// floats it as a list with the keys (`╭─ folders`, its place and files on its row, its rows in the state), `d`
/// removes it (the list going with the last); Claude's context names it only while it's there; the state lists it.
#[test]
fn a_folder_is_added_listed_and_removed() {
    let (_s, _project, infra, mut ed) = place("add");
    assert_eq!(crate::folders::context_line(&ed.workspace.folders), None, "no folder, no line");
    feed(&mut ed, ":folders<ret>");
    assert!(said(&ed).contains("no folder is added here"), "{}", said(&ed));
    let s = add(&mut ed, &infra);
    assert!(s.contains("folder infra added"), "{s}");
    let f = &ed.workspace.folders[0];
    assert_eq!(
        (f.at.as_path(), f.name.as_str(), f.instructions.as_slice()),
        (infra.as_path(), "infra", &["CLAUDE.md".to_string()][..])
    );
    assert_eq!(
        crate::folders::context_line(&ed.workspace.folders).unwrap(),
        format!(
            "folders (read and edit there as in the project; paths absolute; search them by path): {} (has CLAUDE.md)",
            infra.display()
        )
    );
    assert_eq!(ed.ui_state().workspaces[0].folders, [infra.display().to_string()]);
    // Its Claude reads there as in the project: a default of its `Caps`
    let caps = ed.caps_basis().build();
    assert_eq!(caps.default_for(&infra.join("main.tf")), Some(Why::Folder));
    // The same again: said, not twice
    assert!(add(&mut ed, &infra).contains("it's added already"), "{}", said(&ed));
    assert_eq!(ed.workspace.folders.len(), 1);
    feed(&mut ed, ":folders<ret>");
    assert!(ed.float_list_keys(GroupKind::Folders), "focused at once");
    let s = frame(&mut ed);
    assert!(s.contains("╭─ folders ── 1 folder") && s.contains("d remove"), "{s}");
    let row = s.lines().find(|l| l.contains("▎ infra")).unwrap_or_else(|| panic!("its row: {s}"));
    assert!(row.contains("has CLAUDE.md"), "{row}");
    let listed = ed.ui_state().panels.lists;
    assert!(listed.iter().any(|(k, rows)| k == "folders" && rows.iter().any(|r| r.selected)), "{listed:?}");
    feed(&mut ed, "d");
    assert!(ed.workspace.folders.is_empty() && ed.folders_pane.is_none(), "removed, the list gone with it");
    assert!(said(&ed).contains("folder infra removed"), "{}", said(&ed));
    assert_eq!(crate::folders::context_line(&ed.workspace.folders), None);
}

/// What can't be a folder is refused and said why: not a folder, the project, in it, holding it, home or holding it,
/// what runs never read, in a folder added or holding one, another checkout of the project's repository.
#[test]
fn what_cant_be_a_folder_is_said() {
    let (s, project, infra, mut ed) = place("refused");
    let home = ed.caps_home.clone().unwrap();
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::create_dir_all(infra.join("modules")).unwrap();
    let refused = |ed: &mut Editor, at: &Path, why: &str| {
        let s = add(ed, at);
        assert!(s.contains("isn't added") && s.contains(why), "{}: {s}", at.display());
    };
    refused(&mut ed, &project.join("a.rs"), "it isn't a folder");
    refused(&mut ed, &s.join("nowhere"), "No such file");
    refused(&mut ed, &project, "that's the project itself");
    refused(&mut ed, &project.join("src"), "it's in the project");
    refused(&mut ed, s.path(), "it holds the project");
    refused(&mut ed, &home, "your home");
    refused(&mut ed, &home.join(".ssh"), "what runs never read");
    assert!(add(&mut ed, &infra).contains("added"));
    refused(&mut ed, &infra.join("modules"), "it's in folder infra");
    assert_eq!(ed.workspace.folders.len(), 1, "only the one");
    // Another checkout of the project's repository: that's a workspace
    git(&project, &["init", "-q"]);
    git(&project, &["add", "-A"]);
    git(&project, &["commit", "-q", "-m", "one"]);
    let other = s.join("other");
    git(&project, &["worktree", "add", "-q", other.to_str().unwrap()]);
    refused(&mut ed, &other, "another checkout of this repository");
}

/// A folder's file gets the folder's repository as a session's file gets its checkout's: the folder first among
/// `git_places`' roots, found even where a sandboxed run may write the folder (C opens it to runs — discovery from
/// a file's folder refuses such a place), its config that runs commands held until trusted (the card asks) and
/// not run meanwhile (a stand-in fsmonitor in the test's own scratch).
#[test]
fn a_folders_file_gets_the_folders_repository() {
    use std::os::unix::fs::PermissionsExt as _;
    let (s, _project, infra, mut ed) = place("git");
    git(&infra, &["init", "-q"]);
    git(&infra, &["add", "-A"]);
    git(&infra, &["commit", "-q", "-m", "one"]);
    let (monitor, ran) = (s.join("monitor.sh"), s.join("ran-fsmonitor"));
    std::fs::write(&monitor, format!("#!/bin/sh\ntouch '{}'\n", ran.display())).unwrap();
    std::fs::set_permissions(&monitor, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(&infra, &["config", "core.fsmonitor", monitor.to_str().unwrap()]);
    assert!(add(&mut ed, &infra).contains("added"), "{}", said(&ed));
    let tf = infra.join("main.tf");
    let places = ed.git_places(&tf);
    let roots = places.roots.clone();
    // (as C will have it: the folder one of the places a sandboxed run writes)
    let w = infra.clone();
    let places =
        crate::gitrepo::Places { writes: std::sync::Arc::new(move || Some(vec![w.clone()])), ..places };
    let (repo, rel) =
        crate::gitrepo::for_file_in(&places, &tf).map_err(|e| e.said()).expect("its repository");
    assert_eq!(rel, "main.tf");
    assert_eq!(roots.first(), Some(&infra), "the folder holding it first: {roots:?}");
    assert!(repo.held.is_some() && repo.asks, "its fsmonitor held, the card asks");
    std::fs::write(&tf, "resource { changed }\n").unwrap();
    let _ = repo.output(&["status", "--porcelain"], None);
    assert!(!ran.exists(), "held: the repository's fsmonitor never ran");
}

/// A folder swapped for a link after it was added (`infra` → a stand-in `~/.aws` under the scratch) is no root:
/// Claude's tools refuse what's through it and say why, never taking the link's target as the folder; the next
/// turn's look drops it and says so.
#[test]
fn a_folder_swapped_for_a_link_is_no_root() {
    let (s, _project, infra, mut ed) = place("swapped");
    let aws = ed.caps_home.clone().unwrap().join(".aws");
    std::fs::create_dir_all(&aws).unwrap();
    std::fs::write(aws.join("config"), "[default]\n").unwrap();
    assert!(add(&mut ed, &infra).contains("added"));
    std::fs::rename(&infra, s.join("infra.was")).unwrap();
    std::os::unix::fs::symlink(&aws, &infra).unwrap();
    let (roots, keys) = (ed.workspace.roots(), crate::reads::Keys::of(&ed));
    let config = infra.join("config");
    assert_eq!(roots.inside(&config), None, "no root");
    let e = crate::nav::file_in(&roots, &config, &keys).unwrap_err();
    assert!(e.contains("folder") && e.contains("isn't the folder added any more"), "{e}");
    let e = crate::edittool::place_in(&roots, &keys, config.to_str().unwrap()).unwrap_err();
    assert!(e.contains("isn't the folder added any more"), "{e}");
    let before = ed.said;
    ed.folders_look();
    settle_until(&mut ed, |ed| ed.said > before);
    assert!(said(&ed).starts_with("folder dropped: infra ("), "{}", said(&ed));
    assert!(ed.workspace.folders.is_empty(), "dropped");
}

/// A folder in a place sandboxed runs write, or holding one, is refused — a run could put a link in its place; with
/// runs not sandboxed (no places), it isn't for that.
#[test]
fn a_folder_where_runs_write_is_refused() {
    let s = crate::scratch::Scratch::new("folders-writes");
    for d in ["project", "tmp/infra", "work/runs"] {
        std::fs::create_dir_all(s.join(d)).unwrap();
    }
    let real = |p: &str| s.join(p).canonicalize().unwrap();
    let roots = Roots::new(real("project"), Vec::new());
    let caps = crate::caps::Caps::default();
    let why = judge(&real("tmp/infra"), &roots, &caps, &[real("tmp")]).unwrap_or_default();
    assert!(why.contains("where Claude's runs write — a run could put a link in its place"), "{why}");
    let why = judge(&real("work"), &roots, &caps, &[real("work/runs")]).unwrap_or_default();
    assert!(why.starts_with("it holds") && why.contains("where Claude's runs write"), "{why}");
    assert_eq!(judge(&real("tmp/infra"), &roots, &caps, &[]), None, "runs not sandboxed");
}

/// A folder is in a workspace once, whichever way it came: `:folder-add` of one kept and still being judged as the
/// workspace came back is said added already, and the judged one lands once; `:folder-remove` then leaves none.
#[test]
fn a_folder_is_in_its_workspace_once_while_kept_ones_are_judged() {
    let (_s, _project, infra, mut ed) = place("once");
    assert!(add(&mut ed, &infra).contains("added"));
    // (as `workspaces_back` leaves it: kept, being judged again)
    ed.workspace.folders_judging = std::mem::take(&mut ed.workspace.folders);
    let s = add(&mut ed, &infra);
    assert!(s.contains("added already"), "{s}");
    assert!(ed.workspace.folders.is_empty(), "not added beside the one being judged");
    ed.folders_back(ed.workspace.id);
    settle_until(&mut ed, |ed| ed.workspace.folders_judging.is_empty());
    assert_eq!(ed.workspace.folders.len(), 1, "once");
    feed(&mut ed, ":folder-remove infra<ret>");
    assert!(ed.workspace.folders.is_empty() && ed.workspace.roots().folders().is_empty(), "none left");
}

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Claude's tools take a folder as the project: `file_in` lets its file through and refuses one beside it; `edit`'s
/// place shows it whole and judges its protected names from it; a file named like a key there is shut, as in the
/// project; the reads' gate lets it through with no card, a sibling still asks; removed, it asks again.
#[test]
fn claudes_tools_work_in_a_folder_as_in_the_project() {
    let (s, project, infra, mut ed) = place("tools");
    std::fs::write(infra.join("Makefile"), "all:\n").unwrap();
    std::fs::write(infra.join(".env"), "TOKEN=tarae-fake-token\n").unwrap();
    let beside = s.join("beside");
    std::fs::create_dir_all(&beside).unwrap();
    std::fs::write(beside.join("b.txt"), "b\n").unwrap();
    assert!(add(&mut ed, &infra).contains("added"));
    let (roots, keys) = (ed.workspace.roots(), crate::reads::Keys::of(&ed));
    let tf = infra.join("main.tf");
    assert_eq!(crate::nav::file_in(&roots, &tf, &keys), Ok(tf.clone()));
    let e = crate::nav::file_in(&roots, &beside.join("b.txt"), &keys).unwrap_err();
    assert!(e.contains("outside the project — these tools look inside it and its folders only"), "{e}");
    assert!(
        crate::nav::file_in(&roots, &infra.join(".env"), &keys).unwrap_err().contains("named like a key")
    );
    // `edit`: a folder's path whole, the project's from it; its protected names its own
    let at = |p: &str| crate::edittool::place_in(&roots, &keys, p);
    assert_eq!(at("a.rs").map(|(_, rel, protected)| (rel, protected)), Ok(("a.rs".into(), false)));
    let shown = tf.display().to_string();
    assert_eq!(at(&shown).map(|(_, rel, protected)| (rel, protected)), Ok((shown.clone(), false)));
    assert_eq!(at(infra.join("Makefile").to_str().unwrap()).map(|(.., protected)| protected), Ok(true));
    let e = at(beside.join("b.txt").to_str().unwrap()).unwrap_err();
    assert!(e.contains("edit changes files inside it and its folders only"), "{e}");
    assert!(at(infra.to_str().unwrap()).unwrap_err().contains("an added folder itself"));
    let _ = project;
    // The reads' gate: the folder read unasked, beside it asked
    let gate = crate::reads::Gate::of(&ed, Vec::new());
    assert!(gate.judge(&tf, true).is_ok(), "a folder's file: no card");
    assert!(gate.judge(&beside.join("b.txt"), true).is_err(), "beside it: the card");
    feed(&mut ed, ":folder-remove infra<ret>");
    assert!(ed.workspace.folders.is_empty());
    let gate = crate::reads::Gate::of(&ed, Vec::new());
    assert!(gate.judge(&tf, true).is_err(), "removed: the card again");
}

/// A turn's file in a folder is taken back (`space u`'s step) through no link from the folder, as the project's.
#[test]
fn a_turns_file_in_a_folder_is_taken_back_from_the_folder() {
    let (_s, _project, infra, mut ed) = place("undo");
    assert!(add(&mut ed, &infra).contains("added"));
    let tf = infra.join("main.tf");
    let (before, after) = (ropey::Rope::from_str("resource {}\n"), ropey::Rope::from_str("resource { x }\n"));
    std::fs::write(&tf, after.to_string()).unwrap(); // (Claude's write)
    // (`space u`'s step: from what the turn left to what was there before it)
    let r = crate::checkpoint::file(&mut ed, &tf, Some(&after), Some(&before));
    assert_eq!(r, Ok((true, 0)));
    assert_eq!(std::fs::read_to_string(&tf).unwrap(), "resource {}\n");
}

/// Kept with the workspace (the sessions' file and `:restart`'s state, each by its own writer): a folder comes back
/// with it, but only once judged again on a worker — meanwhile kept, never read as its; one gone, or a link now to
/// home (a stand-in under the scratch), is dropped and said.
#[test]
fn a_folder_comes_back_with_its_workspace_judged_again() {
    let (s, _project, infra, mut ed) = place("kept");
    let (gone, tools) = (s.join("gone"), s.join("tools"));
    for d in [&gone, &tools] {
        std::fs::create_dir_all(d).unwrap();
    }
    for d in [&infra, &gone, &tools] {
        assert!(add(&mut ed, d).contains("added"), "{}", said(&ed));
    }
    let kept = crate::worktree::Kept::of(&ed.workspace);
    // (in the sessions' file, by where and name)
    let (dir, common) = (s.join("sessions"), s.join("repo.git"));
    let stored = crate::worktree::Stored { sessions: vec![kept.clone()], ..Default::default() };
    crate::worktree::save(&dir, &common, stored, &[], Default::default()).unwrap();
    let back = &crate::worktree::load(&dir, &common).sessions[0].folders;
    let by = |f: &[Folder]| f.iter().map(|f| (f.at.clone(), f.name.clone())).collect::<Vec<_>>();
    assert_eq!(by(back), by(&kept.folders));
    // `:restart`'s state, as it writes it; then one gone, one a link to home
    let state = crate::worktree::snapshot(&ed);
    std::fs::remove_dir(&gone).unwrap();
    std::fs::remove_dir(&tools).unwrap();
    std::os::unix::fs::symlink(ed.caps_home.clone().unwrap(), &tools).unwrap();
    ed.workspace.folders.clear();
    crate::worktree::restore(&mut ed, &state);
    assert!(ed.workspace.folders.is_empty(), "not its folders until judged");
    assert_eq!(ed.workspace.folders_judging.len(), 3, "kept meanwhile");
    assert_eq!(crate::worktree::Kept::of(&ed.workspace).folders.len(), 3);
    settle_until(&mut ed, |ed| ed.workspace.folders_judging.is_empty());
    let names: Vec<&str> = ed.workspace.folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["infra"]);
    assert_eq!(ed.workspace.folders[0].instructions, ["CLAUDE.md"], "its files looked for again");
    let s = said(&ed);
    assert!(s.starts_with("folder dropped as it came back:"), "{s}");
    assert!(s.contains("gone (") && s.contains("No such file"), "{s}");
    assert!(s.contains("tools (") && s.contains("your home"), "{s}");
}

/// The folder joins the workspace it was asked in and judged against — behind by the time its check lands, it's that
/// one's, never the front's; one ended meanwhile takes none, and the front gets none either.
#[test]
fn a_folder_joins_the_workspace_it_was_asked_in() {
    let (s, _project, infra, mut ed) = place("which");
    let home = ed.workspace.id;
    let (other, ended) = (s.join("other"), s.join("ended"));
    for d in [&other, &ended] {
        std::fs::create_dir_all(d).unwrap();
    }
    let b = ed.workspace_add(other.canonicalize().unwrap());
    ed.workspace_switch(b);
    feed(&mut ed, &format!(":folder-add {}<ret>", infra.display()));
    ed.workspace_switch(home);
    settle_until(&mut ed, |ed| ed.workspaces.iter().any(|w| w.id == b && !w.folders.is_empty()));
    assert!(ed.workspace.folders.is_empty(), "the front's: none");
    // Asked in one that ends before its check lands
    let c = ed.workspace_add(ended.canonicalize().unwrap());
    ed.workspace_switch(c);
    feed(&mut ed, &format!(":folder-add {}<ret>", infra.display()));
    ed.workspace_switch(home);
    ed.workspace_close(&ended.canonicalize().unwrap());
    let before = ed.said;
    settle_until(&mut ed, |ed| ed.said > before);
    assert!(said(&ed).contains("the workspace it was for has ended"), "{}", said(&ed));
    assert!(ed.workspace.folders.is_empty(), "never the front's");
}

/// On a narrow screen the place folds first, from its start, and the instruction files stay whole.
#[test]
fn the_place_folds_before_the_files() {
    let (s, _project, _infra, mut ed) = place("narrow");
    // (a name this long leaves the place a few cells in a 60-column screen's list)
    let tf = s.join("infra-terraform");
    std::fs::create_dir_all(&tf).unwrap();
    std::fs::write(tf.join("CLAUDE.md"), "# tf\n").unwrap();
    assert!(add(&mut ed, &tf).contains("added"));
    feed(&mut ed, ":folders<ret>");
    ed.focus_sync();
    crate::term::render(&mut ed, &mut Vec::new(), 60, 30).unwrap();
    let s = crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), 60, 30).unwrap();
    let row = s.lines().find(|l| l.contains("▎ infra-terraform")).unwrap_or_else(|| panic!("its row: {s}"));
    assert!(row.contains("has CLAUDE.md") && row.contains("…"), "{row}");
}

/// `:folders` only floats: `C-w L` from it says so (and how it moves); its frame, once you've set one, is kept with
/// the sizes (`sizes.json`, read again as tarae starts) and is the list's.
#[test]
fn the_folders_list_floats_and_keeps_its_frame() {
    let (s, _project, infra, mut ed) = place("frame");
    assert!(add(&mut ed, &infra).contains("added"));
    feed(&mut ed, ":folders<ret>");
    feed(&mut ed, "<C-w>L");
    assert!(said(&ed).contains("the folders list floats — C-w m moves it"), "{}", said(&ed));
    assert!(ed.float_list_keys(GroupKind::Folders), "still floating, with the keys");
    let frame = crate::pane::Frame { x: 100, y: 300, w: 500, h: 0 };
    let path = s.join("sizes.json");
    let sizes = crate::sizes::Sizes { folders: Some(frame), ..Default::default() };
    std::fs::write(&path, sizes.to_json()).unwrap();
    ed.sizes = crate::sizes::Sizes::read(Some(&path));
    assert_eq!(ed.float_list_frame(GroupKind::Folders), Some(frame));
}
