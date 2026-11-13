//! G13: a stand-in `rust-analyzer` (a `sh` that keeps what it's sent — named so, it has its pack's traits and runs
//! nothing) in a scratch cargo project, the trust store a scratch file; a stand-in `taplo` beside it for a server
//! that runs no code of the project's.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::*;
use crate::lsp_editor::LspDot;

/// An editor with the gate on and its own store, rust's server the stand-in `rust-analyzer` and TOML's `taplo`;
/// the project's folder (real), `main.rs` and `Cargo.toml` in it.
fn setup(s: &crate::scratch::Scratch) -> (Editor, PathBuf) {
    let dir = std::fs::canonicalize(s.path()).unwrap();
    std::fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
    let cfg = format!(
        "[lsp.rust-analyzer]\ncommand = \"sh\"\nargs = [\"-c\", \"cat > '{0}/ra.log'\"]\n\
         [lsp.taplo]\ncommand = \"sh\"\nargs = [\"-c\", \"cat > '{0}/taplo.log'\"]\n\
         [lang.rust]\nlsp = [\"rust-analyzer\"]\n[lang.toml]\nlsp = [\"taplo\"]\n",
        dir.display()
    );
    let (config, w) = crate::config::parse(&cfg);
    assert!(w.is_empty(), "{w:?}");
    let mut ed = Editor::new(config);
    ed.code_gate = true;
    ed.trust_path = Some(dir.join("trust.json"));
    (ed, dir)
}

/// Events until `done` (the trust store's read and write are jobs).
fn until(ed: &mut Editor, done: impl Fn(&Editor) -> bool) -> bool {
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    while !done(ed) && std::time::Instant::now() < end {
        if let Some(ev) = ed.events.recv_timeout(Duration::from_millis(20)) {
            ed.handle_event(ev);
        }
    }
    done(ed)
}

fn card(ed: &Editor) -> Option<&crate::offer::Offer> {
    ed.offers.iter().find(|o| matches!(o.what, crate::offer::What::CodeTrust { .. }))
}

fn has_server(ed: &Editor, file: &Path) -> bool {
    ed.docs.iter().any(|d| d.path.as_deref() == Some(file) && d.lsp.client.is_some())
}

/// A server that runs the project's code (rust-analyzer, its pack says so) doesn't start in a repository you
/// haven't trusted: its file waits, the status line says `untrusted`, and its card names the server and what of
/// the project it runs. One that runs nothing of it (taplo) starts beside as ever. `n` holds it back until you
/// quit — the card asks no more on its own, `:code-trust` asks again; `y` starts it, and the answer is kept: the
/// next tarae starts it with no card.
#[test]
fn a_server_that_runs_the_projects_code_waits_for_its_trust() {
    let s = crate::scratch::Scratch::new("code-trust");
    let (mut ed, dir) = setup(&s);
    let (rs, toml) = (dir.join("main.rs"), dir.join("Cargo.toml"));
    ed.open(&rs).unwrap();
    assert!(!has_server(&ed, &rs), "not started while its trust is read");
    assert!(until(&mut ed, |ed| card(ed).is_some()), "asked once read");
    assert!(!has_server(&ed, &rs) && !dir.join("ra.log").exists(), "nothing started");
    assert_eq!(ed.lsp_state(), Some((LspDot::Untrusted, "rust".into())));
    let c = card(&ed).unwrap();
    assert_eq!(c.title, FOLDER_TITLE, "a folder's, as #513's card beside it says");
    assert!(
        c.question.contains("rust-analyzer — its build scripts and proc macros")
            && !c.question.contains("checkout"),
        "no repository, no checkouts to name: {}",
        c.question
    );
    // (a server that runs no code of the project's: as ever)
    ed.open(&toml).unwrap();
    assert!(has_server(&ed, &toml), "taplo starts");
    ed.open(&rs).unwrap();
    // n: held back until you quit
    let id = card(&ed).unwrap().id;
    ed.code_trust_answer(id, &dir, false);
    assert!(card(&ed).is_none() && !has_server(&ed, &rs));
    ed.open(&toml).unwrap();
    ed.open(&rs).unwrap();
    assert!(card(&ed).is_none(), "not asked again on its own");
    assert_eq!(ed.lsp_state(), Some((LspDot::Untrusted, "rust".into())), "still said");
    // :code-trust, y: it starts, kept
    ed.code_trust_ask().unwrap();
    let id = card(&ed).expect("asked again").id;
    ed.code_trust_answer(id, &dir, true);
    assert!(has_server(&ed, &rs), "started on the yes");
    assert!(until(&mut ed, |ed| ed.trust_writing == 0), "kept");
    let (mut next, _) = setup(&s);
    next.open(&rs).unwrap();
    assert!(until(&mut next, |ed| has_server(ed, &rs)), "the next tarae: started once read");
    assert!(card(&next).is_none(), "no card");
}

/// #517: a folder's code trusted, then a repository made there (`git init`'s folder, made by hand): the next tarae
/// starts its servers with no second card, says the carry once, and a later one says nothing; `:code-untrust` then
/// takes it back at the repository's place, nothing left at the folder's.
#[test]
fn a_folders_trust_is_the_repository_made_there() {
    let s = crate::scratch::Scratch::new("code-trust-carry");
    let (mut ed, dir) = setup(&s);
    let rs = dir.join("main.rs");
    ed.open(&rs).unwrap();
    assert!(until(&mut ed, |ed| card(ed).is_some()));
    let id = card(&ed).unwrap().id;
    ed.code_trust_answer(id, &dir, true);
    assert!(until(&mut ed, |ed| ed.trust_writing == 0), "kept");
    std::fs::create_dir(dir.join(".git")).unwrap();
    let name = dir.file_name().unwrap().to_string_lossy().to_string();
    let words = format!("trust carried from {name} — answered before it was a repository");
    let said = |ed: &Editor| ed.messages.iter().filter(|m| m.2 == words).count();
    let (mut next, _) = setup(&s);
    next.open(&rs).unwrap();
    assert!(until(&mut next, |ed| has_server(ed, &rs)), "started once read");
    assert!(card(&next).is_none(), "no second card");
    assert_eq!(said(&next), 1, "{:?}", next.messages.iter().map(|m| &m.2).collect::<Vec<_>>());
    let (mut later, _) = setup(&s);
    later.open(&rs).unwrap();
    assert!(until(&mut later, |ed| has_server(ed, &rs)));
    assert_eq!(said(&later), 0, "said once, never again");
    later.code_untrust().unwrap();
    assert!(until(&mut later, |ed| ed.trust_writing == 0));
    let kept = std::fs::read_to_string(dir.join("trust.json")).unwrap();
    assert!(!kept.contains(&dir.join(FILE).display().to_string()), "nothing at the folder's place: {kept}");
    let (mut again, _) = setup(&s);
    again.open(&rs).unwrap();
    assert!(until(&mut again, |ed| card(ed).is_some()), "asked again");
    assert_eq!(card(&again).unwrap().title, TITLE, "a repository's now");
    for e in [&mut ed, &mut next, &mut later, &mut again] {
        e.writes_flush();
    }
}

/// `:code-untrust` stops the servers that run the project's code (not the others) and its file waits for them
/// again — the card asks at the next look.
#[test]
fn code_untrust_stops_them() {
    let s = crate::scratch::Scratch::new("code-untrust");
    let (mut ed, dir) = setup(&s);
    let (rs, toml) = (dir.join("main.rs"), dir.join("Cargo.toml"));
    ed.open(&toml).unwrap();
    ed.open(&rs).unwrap();
    assert!(until(&mut ed, |ed| card(ed).is_some()));
    let id = card(&ed).unwrap().id;
    ed.code_trust_answer(id, &dir, true);
    assert!(has_server(&ed, &rs) && has_server(&ed, &toml));
    ed.code_untrust().unwrap();
    assert!(!has_server(&ed, &rs), "rust-analyzer stopped");
    assert!(has_server(&ed, &toml), "taplo runs nothing of it: it stays");
    assert!(until(&mut ed, |ed| card(ed).is_some()), "asked again, the store read anew");
    assert_eq!(ed.code_untrust().unwrap_err(), "this repository's code isn't trusted here");
    // (the store written before the folder goes — #441: one landing after it was left in the temp folder)
    ed.writes_flush();
}

/// In a repository the trust is the repository's, every server root in it alike: two cargo roots ask once, and
/// `:code-untrust` from either takes it back from both — every root's servers that run its code stopped, its files
/// waiting, not only the front file's root.
#[test]
fn code_untrust_takes_it_back_at_every_root_of_the_repository() {
    let s = crate::scratch::Scratch::new("code-untrust-roots");
    let (mut ed, dir) = setup(&s);
    // (a repository of its own beside setup's crate, a crate in each of two folders — no `Cargo.toml` above them)
    let repo = dir.join("repo");
    std::fs::create_dir(&repo).unwrap();
    let init = std::process::Command::new("git").args(["init", "-q"]).current_dir(&repo).status().unwrap();
    assert!(init.success());
    let (a, b) = (repo.join("a"), repo.join("b"));
    for root in [&a, &b] {
        std::fs::create_dir(root).unwrap();
        std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
    }
    let (in_a, in_b) = (a.join("main.rs"), b.join("main.rs"));
    ed.open(&in_a).unwrap();
    assert!(until(&mut ed, |ed| card(ed).is_some()));
    let crate::offer::What::CodeTrust { root } = card(&ed).unwrap().what.clone() else { unreachable!() };
    assert_eq!(root, a, "each crate its own server root");
    let id = card(&ed).unwrap().id;
    ed.code_trust_answer(id, &a, true);
    assert!(until(&mut ed, |ed| ed.trust_writing == 0), "kept");
    ed.open(&in_b).unwrap();
    assert!(until(&mut ed, |ed| has_server(ed, &in_b)), "the other root: the repository's answer, no card");
    assert!(card(&ed).is_none());
    ed.open(&in_a).unwrap();
    // (each root's share key is read on a worker: the one read second joins the first's server, parked until
    // its swap begins — `a`'s may be that one)
    assert!(until(&mut ed, |ed| has_server(ed, &in_a)), "the first root's server, its own or the shared one");
    ed.code_untrust().unwrap();
    assert!(!has_server(&ed, &in_a), "the front file's root's server stopped");
    assert!(!has_server(&ed, &in_b), "and the other root's: the trust it ran on is gone");
    assert!(until(&mut ed, |ed| card(ed).is_some() && ed.trust_writing == 0), "asked again");
    assert!(!has_server(&ed, &in_b), "still waiting, the store read anew");
}

/// In a git repository the answer is the repository's — the card says every checkout of it shares it.
#[test]
fn a_repositorys_card_says_its_checkouts_share_it() {
    let s = crate::scratch::Scratch::new("code-trust-repo");
    let (mut ed, dir) = setup(&s);
    let init = std::process::Command::new("git").args(["init", "-q"]).current_dir(&dir).status().unwrap();
    assert!(init.success());
    ed.open(&dir.join("main.rs")).unwrap();
    assert!(until(&mut ed, |ed| card(ed).is_some()));
    let q = &card(&ed).unwrap().question;
    assert!(q.contains("Every checkout of it shares the answer."), "{q}");
}

/// Every code trust card up or parked, any session's.
fn cards(ed: &Editor) -> Vec<&crate::offer::Offer> {
    let parked = std::iter::once(&ed.workspace).chain(&ed.workspaces).flat_map(|l| &l.parked);
    ed.offers
        .iter()
        .chain(parked)
        .filter(|o| matches!(o.what, crate::offer::What::CodeTrust { .. }))
        .collect()
}

/// A repository and three checkouts of it (`git worktree add`), a crate in each — made once, a next tarae finds
/// them there; the editor's front session at the first, each checkout's stand-in server its own (what's looked at
/// is the gate, not `share.rs`).
fn repository(ed: &mut Editor, dir: &Path) -> Vec<PathBuf> {
    ed.config.lsp.share = Some(Vec::new());
    let main = dir.join("main");
    let git = |args: &[&str]| {
        let mut c = std::process::Command::new("git");
        c.args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).current_dir(&main);
        assert!(c.status().unwrap().success(), "git {args:?}");
    };
    let mut roots = vec![main.clone()];
    if !main.exists() {
        std::fs::create_dir(&main).unwrap();
        git(&["init", "-q"]);
        git(&["commit", "-q", "--allow-empty", "-m", "first"]);
    }
    for n in 1..4 {
        let w = dir.join(format!("w{n}"));
        if !w.exists() {
            git(&["worktree", "add", "-q", w.to_str().unwrap()]);
        }
        roots.push(w);
    }
    for root in &roots {
        std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
    }
    ed.workspace.root = main;
    roots
}

/// A session behind for checkout `root`, its crate opened in it as a restore does — its trust read as that
/// session's job.
fn behind(ed: &mut Editor, root: &Path) -> crate::event::WorkspaceId {
    let id = ed.workspace_add(root.to_path_buf());
    let file = root.join("main.rs");
    ed.with_workspace(id, move |ed| {
        ed.open(&file).unwrap();
    });
    id
}

/// `repository`'s checkouts as sessions of one editor restored at once — the front one's crate opened in front,
/// each other's behind. #226: each had a card of its own (parked with its session), and an answer took its own
/// checkout only.
fn checkouts(ed: &mut Editor, dir: &Path) -> Vec<PathBuf> {
    let roots = repository(ed, dir);
    ed.open(&roots[0].join("main.rs")).unwrap();
    for root in &roots[1..] {
        behind(ed, root);
    }
    roots
}

/// #226: a repository's checkouts restored at once ask on one card, up in front — every checkout's server named
/// once. `n` holds every one back, said once; `:code-trust` asks again on one card, and `y` closes it and starts
/// every checkout's server. The next tarae starts them all with no card in any checkout.
#[test]
fn a_repositorys_checkouts_restored_at_once_ask_on_one_card() {
    let s = crate::scratch::Scratch::new("code-trust-checkouts");
    let (mut ed, dir) = setup(&s);
    let roots = checkouts(&mut ed, &dir);
    let files: Vec<PathBuf> = roots.iter().map(|r| r.join("main.rs")).collect();
    let read = |ed: &Editor| roots.iter().all(|r| ed.code_trust.get(r).is_some_and(|t| t.state.is_some()));
    assert!(until(&mut ed, read), "every checkout's trust read");
    let up = cards(&ed);
    assert_eq!(
        up.len(),
        1,
        "one card for the repository: {:?}",
        up.iter().map(|o| &o.what).collect::<Vec<_>>()
    );
    assert!(card(&ed).is_some(), "up in front, not parked with another session");
    let q = &card(&ed).unwrap().question;
    assert_eq!(q.matches("rust-analyzer —").count(), 1, "its server named once: {q}");
    assert!(files.iter().all(|f| !has_server(&ed, f)), "nothing started");
    // n: every checkout held back, said once
    let crate::offer::What::CodeTrust { root } = card(&ed).unwrap().what.clone() else { unreachable!() };
    ed.code_trust_answer(card(&ed).unwrap().id, &root, false);
    assert!(cards(&ed).is_empty(), "no card left in any session");
    assert!(files.iter().all(|f| !has_server(&ed, f)));
    assert_eq!(ed.messages.iter().filter(|m| m.2.contains("held back")).count(), 1, "said once");
    assert_eq!(ed.lsp_state(), Some((LspDot::Untrusted, "rust".into())));
    // :code-trust, y: one card, every checkout's server
    ed.code_trust_ask().unwrap();
    assert_eq!(cards(&ed).len(), 1, "asked again on one card");
    let crate::offer::What::CodeTrust { root } = card(&ed).expect("in front").what.clone() else {
        unreachable!()
    };
    ed.code_trust_answer(card(&ed).unwrap().id, &root, true);
    assert!(cards(&ed).is_empty(), "closed in every session");
    for f in &files {
        assert!(has_server(&ed, f), "{} started on the one yes", f.display());
    }
    assert!(until(&mut ed, |ed| ed.trust_writing == 0), "kept");
    // the next tarae: no card in any checkout
    let (mut next, _) = setup(&s);
    checkouts(&mut next, &dir);
    assert!(
        until(&mut next, |ed| files.iter().all(|f| has_server(ed, f))),
        "every checkout's started once read"
    );
    assert!(cards(&next).is_empty(), "no card");
}

/// #226: the card parked with a session behind (its checkout read first) comes up in front once a file there waits
/// for it — the same card, not a second: its id, when it came; not the user's own (it came on its own: unfocused,
/// it waits for `C-q`) and about no one session, as a parked card that comes up with its session (`workspace_in`);
/// its nudge from the session it was parked in finds nothing to say. The order fixed: the behind checkout's read
/// lands before the front's file is opened.
#[test]
fn a_card_parked_behind_comes_up_in_front_once_a_file_there_waits() {
    let s = crate::scratch::Scratch::new("code-trust-parked");
    let (mut ed, dir) = setup(&s);
    let roots = repository(&mut ed, &dir);
    let w1 = behind(&mut ed, &roots[1]);
    let parked = |ed: &Editor| {
        let l = ed.workspaces.iter().find(|l| l.id == w1)?;
        l.parked.iter().find(|o| matches!(o.what, crate::offer::What::CodeTrust { .. })).cloned()
    };
    assert!(until(&mut ed, |ed| parked(ed).is_some()), "parked with its session");
    assert!(card(&ed).is_none(), "not up in front: no file there waits for it");
    let was = parked(&ed).unwrap();
    ed.open(&roots[0].join("main.rs")).unwrap();
    assert!(until(&mut ed, |ed| card(ed).is_some()), "up in front once its file waits");
    assert_eq!(cards(&ed).len(), 1, "moved, not a second");
    let c = card(&ed).unwrap().clone();
    assert_eq!((c.id, c.came, c.since), (was.id, was.came, was.since), "the same card");
    assert!(!c.yours && c.about.is_none() && ed.card_focus.is_none(), "came on its own, about no session");
    assert_eq!(c.question.matches("rust-analyzer —").count(), 1, "its server named once: {}", c.question);
    // (its nudge, due: nothing — it's up)
    let long_ago = crate::clock::now().checked_sub(crate::workspace::NUDGE_AFTER * 2).unwrap();
    ed.offers.iter_mut().find(|o| o.id == c.id).unwrap().since = long_ago;
    let said = ed.messages.len();
    ed.card_nudge(w1, c.id);
    assert_eq!(ed.messages.len(), said, "no nudge: it's up in front");
    assert!(ed.workspaces.iter().all(|l| l.nudged.is_empty()));
    // answered in front: both checkouts' servers
    let crate::offer::What::CodeTrust { root } = c.what else { unreachable!() };
    ed.code_trust_answer(c.id, &root, true);
    assert!(cards(&ed).is_empty());
    for r in &roots[..2] {
        assert!(has_server(&ed, &r.join("main.rs")), "{} started", r.display());
    }
    ed.writes_flush();
}

/// #441: these left the store in the temp folder every run — written as the folder went.
#[test]
fn the_trust_tests_leave_no_folder_behind() {
    let left = crate::scratch::left_by(|| {
        code_untrust_stops_them();
        a_card_parked_behind_comes_up_in_front_once_a_file_there_waits();
    });
    assert!(left.is_empty(), "left in the temp folder: {left:?}");
}

/// #226: a checkout read after the card was answered takes the answer — its read under way as `y` or `n` is given.
/// `y`: its server starts once read, no card. `n`: held back with the rest, no card — and nothing more said: the
/// `n` said it for the repository, every checkout's (a toast for each checkout read after would say it again), and
/// its file's status line says `untrusted` wherever it's looked at; `:code-trust` there asks again.
#[test]
fn a_checkout_read_after_the_answer_takes_it() {
    for yes in [true, false] {
        let s = crate::scratch::Scratch::new("code-trust-late");
        let (mut ed, dir) = setup(&s);
        let roots = repository(&mut ed, &dir);
        ed.open(&roots[0].join("main.rs")).unwrap();
        assert!(until(&mut ed, |ed| card(ed).is_some()));
        // (no event handled from here to the answer: its read's result waits)
        let w1 = behind(&mut ed, &roots[1]);
        let state = |ed: &Editor| ed.code_trust.get(&roots[1]).and_then(|r| r.state);
        assert_eq!(state(&ed), None, "still being read");
        ed.code_trust_answer(card(&ed).unwrap().id, &roots[0], yes);
        let late = roots[1].join("main.rs");
        assert!(until(&mut ed, |ed| state(ed).is_some() && ed.trust_writing == 0), "read, y: {yes}");
        assert!(cards(&ed).is_empty(), "no card for it, y: {yes}");
        if yes {
            assert_eq!(state(&ed), Some(State::Trusted));
            assert!(has_server(&ed, &late), "y: started once read");
            continue;
        }
        assert_eq!(state(&ed), Some(State::Ignored));
        assert!(!has_server(&ed, &late), "n: held back");
        assert_eq!(
            ed.messages.iter().filter(|m| m.2.contains("held back")).count(),
            1,
            "said once, by the n"
        );
        assert!(!matches!(ed.workspace_switch(w1), crate::workspace::Switched::Gone));
        assert_eq!(ed.lsp_state(), Some((LspDot::Untrusted, "rust".into())), "its file says why");
        ed.code_trust_ask().unwrap();
        assert_eq!(cards(&ed).len(), 1, "asked again there");
    }
}

/// #226's review: a yes kept in the store doesn't outrank this tarae's answer for a checkout read after it. Another
/// tarae's `y` lands in the store, then here `:code-trust` (asking) and maybe `n` (held back): a checkout whose read
/// began after the yes landed and lands after them takes the repository's answer here — joins the card, or held
/// back with the rest —, never trusted by the store's yes, its server never started.
#[test]
fn a_yes_in_the_store_never_outranks_this_answer() {
    for no in [false, true] {
        let s = crate::scratch::Scratch::new("code-trust-store-yes");
        let (mut ed, dir) = setup(&s);
        let roots = repository(&mut ed, &dir);
        ed.open(&roots[0].join("main.rs")).unwrap();
        assert!(until(&mut ed, |ed| card(ed).is_some()));
        ed.code_trust_answer(card(&ed).unwrap().id, &roots[0], false);
        // (another tarae's yes, kept)
        let (mut other, _) = setup(&s);
        repository(&mut other, &dir);
        other.open(&roots[0].join("main.rs")).unwrap();
        assert!(until(&mut other, |ed| card(ed).is_some()));
        other.code_trust_answer(card(&other).unwrap().id, &roots[0], true);
        assert!(until(&mut other, |ed| ed.trust_writing == 0), "kept");
        // here: :code-trust, a checkout's read begun (it finds the yes), maybe n — no event handled until then
        ed.code_trust_ask().unwrap();
        behind(&mut ed, &roots[1]);
        if no {
            ed.code_trust_answer(card(&ed).unwrap().id, &roots[0], false);
        }
        let state = |ed: &Editor| ed.code_trust.get(&roots[1]).and_then(|r| r.state);
        assert!(until(&mut ed, |ed| state(ed).is_some()), "read, n: {no}");
        let late = roots[1].join("main.rs");
        assert!(!has_server(&ed, &late), "not started on the store's yes, n: {no}");
        match no {
            false => {
                assert_eq!(state(&ed), Some(State::Ask), "asked with the rest");
                assert_eq!(cards(&ed).len(), 1, "joins the card up");
            }
            true => {
                assert_eq!(state(&ed), Some(State::Ignored), "held back with the rest");
                assert!(cards(&ed).is_empty(), "no card");
            }
        }
    }
}

/// A stand-in `jdtls` (a `sh` that writes the arguments it got after its script to `args.log`, then keeps what it's
/// sent, both beside the project) for a Java project `folder/` in a scratch folder, Lombok's jars looked for in a stand-in home `home/` beside
/// it (never the real one); the project's folder (real) and `Main.java`.
fn java_project(s: &crate::scratch::Scratch, folder: &str, build: &str) -> (Editor, PathBuf, PathBuf) {
    let top = std::fs::canonicalize(s.path()).unwrap();
    let (dir, home) = (top.join(folder), top.join("home"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(dir.join("build.gradle"), build).unwrap();
    std::fs::write(dir.join("Main.java"), "class Main {}\n").unwrap();
    // (its logs beside the project, not in it: its own arguments never name the project's folder)
    let script = format!("echo \\\"$@\\\" > '{0}/args.log'; cat > '{0}/jdtls.log'", top.display());
    let cfg = format!("[lsp.jdtls]\ncommand = \"sh\"\nargs = [\"-c\", \"{script}\", \"jdtls\"]\n");
    let (config, w) = crate::config::parse(&cfg);
    assert!(w.is_empty(), "{w:?}");
    let mut ed = Editor::new(config);
    ed.code_gate = true;
    ed.trust_path = Some(dir.join("trust.json"));
    ed.agent_looks.home = Some(home.clone());
    (ed, dir, home)
}

const LOMBOK: &str = "dependencies { compileOnly 'org.projectlombok:lombok:1.18.30' }\n";

/// Lombok 1.18.30 in the stand-in home's Gradle cache — the jar's path.
fn lombok_jar(home: &Path) -> PathBuf {
    let at = home.join(".gradle/caches/modules-2/files-2.1/org.projectlombok/lombok/1.18.30/ab12");
    std::fs::create_dir_all(&at).unwrap();
    let jar = at.join("lombok-1.18.30.jar");
    std::fs::write(&jar, "").unwrap();
    jar
}

fn jdtls(ed: &Editor) -> Option<&crate::lsp::Client> {
    ed.lsp.clients.iter().find(|c| c.name == "jdtls")
}

/// Its project trusted: the file's jdtls is asked for (the card answered).
fn trusted(ed: &mut Editor, dir: &Path, java: &Path) {
    ed.open(java).unwrap();
    assert!(until(ed, |ed| card(ed).is_some()), "asked once read");
    let id = card(ed).unwrap().id;
    ed.code_trust_answer(id, dir, true);
}

/// What the stand-in jdtls for the project at `dir` was started with after its script (`args.log` beside it: its
/// pack's `-data` and what's loaded), once it has said it.
fn started_with(dir: &Path) -> Vec<String> {
    let dir = dir.parent().unwrap();
    let log = dir.join("args.log");
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    loop {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        if text.ends_with('\n') || std::time::Instant::now() > end {
            let _ = std::fs::remove_file(&log);
            return text.split_whitespace().map(str::to_string).collect();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn no_agent(args: &[String]) -> bool {
    !args.iter().any(|a| a.contains("javaagent"))
}

/// A server with agents of its pack's (jdtls's Lombok, `lspagent.rs`) has them looked for only once its code is
/// trusted: its file waits for that worker after the yes, then it starts with the agent — the stand-in jdtls's own
/// arguments say so. Each start looks again: stopped, the jar gone (Gradle's cleanup), it starts without it.
#[test]
fn a_servers_agents_are_looked_for_once_its_code_is_trusted() {
    let s = crate::scratch::Scratch::new("code-trust-agents");
    let (mut ed, dir, home) = java_project(&s, "app", LOMBOK);
    let jar = lombok_jar(&home);
    let java = dir.join("Main.java");
    trusted(&mut ed, &dir, &java);
    assert!(!has_server(&ed, &java), "its agents are looked for first");
    assert!(until(&mut ed, |ed| has_server(ed, &java)), "then it starts");
    let agent = format!("--jvm-arg=-javaagent:{}", jar.display());
    let args = started_with(&dir);
    assert!(args.contains(&agent), "the process got it: {args:?}");
    let cid = jdtls(&ed).unwrap().id;
    assert!(jdtls(&ed).unwrap().spec.args.contains(&agent));
    std::fs::remove_file(&jar).unwrap();
    ed.lsp_stopped(cid);
    let id = ed.doc().id;
    ed.attach_lsp(id);
    assert!(!has_server(&ed, &java), "looked for again as it starts again, never the path found before");
    assert!(until(&mut ed, |ed| has_server(ed, &java)));
    let args = started_with(&dir);
    assert!(no_agent(&args) && args.contains(&"-data".to_string()), "its jar gone: none — {args:?}");
}

/// jdtls's `language/status` saying its import is done.
fn imported(ed: &mut Editor) {
    let cid = jdtls(ed).unwrap().id;
    let done = serde_json::json!({"jsonrpc": "2.0", "method": "language/status",
        "params": {"type": "ServiceReady", "message": "ServiceReady"}});
    ed.on_lsp_message(cid, done);
}

/// A fresh clone: its build names Lombok, but jdtls's own import is what downloads it, so the look as it starts finds
/// none. Once jdtls says its import is done it's looked for again: there now, jdtls restarts with it — once, said;
/// another "done" restarts nothing. Not there after the import either: it runs on as it is, looked for once. The
/// project's folder is `lombok-demo`, so the `-data` its pack adds names lombok: the look after the import goes by
/// the arguments jdtls is configured with, as the first did, never taking that folder for an agent of yours.
#[test]
fn lombok_its_import_downloaded_restarts_jdtls_with_it_once() {
    let s = crate::scratch::Scratch::new("code-trust-agents-late");
    let (mut ed, dir, home) = java_project(&s, "lombok-demo", LOMBOK);
    let java = dir.join("Main.java");
    trusted(&mut ed, &dir, &java);
    assert!(until(&mut ed, |ed| has_server(ed, &java)), "it starts without");
    let args = started_with(&dir);
    assert!(no_agent(&args), "{args:?}");
    let data = args.iter().skip_while(|a| *a != "-data").nth(1);
    assert!(data.is_some_and(|d| d.contains("lombok-demo")), "its -data names the folder: {args:?}");
    let first = jdtls(&ed).unwrap().id;
    let jar = lombok_jar(&home);
    imported(&mut ed);
    let said = "Lombok found — jdtls restarts with it";
    assert!(
        until(&mut ed, |ed| jdtls(ed).is_some_and(|c| c.id != first) && has_server(ed, &java)),
        "restarted"
    );
    assert!(ed.toasts.iter().any(|t| t.text == said), "said");
    let args = started_with(&dir);
    assert!(args.contains(&format!("--jvm-arg=-javaagent:{}", jar.display())), "{args:?}");
    let looks = ed.agent_looks.count;
    imported(&mut ed);
    assert_eq!(ed.agent_looks.count, looks, "once: another done looks for nothing");
    assert!(ed.code_trust[&dir].agents.is_empty());

    let s = crate::scratch::Scratch::new("code-trust-agents-never");
    let (mut ed, dir, _) = java_project(&s, "app", LOMBOK);
    let java = dir.join("Main.java");
    trusted(&mut ed, &dir, &java);
    assert!(until(&mut ed, |ed| has_server(ed, &java)));
    let cid = jdtls(&ed).unwrap().id;
    let looks = ed.agent_looks.count;
    imported(&mut ed);
    imported(&mut ed);
    assert_eq!(ed.agent_looks.count, looks + 1, "looked for again once, whatever it says after");
    assert!(
        until(&mut ed, |ed| ed.code_trust.get(&dir).is_some_and(|r| r.agents.is_empty())),
        "looked, none"
    );
    assert_eq!(jdtls(&ed).map(|c| c.id), Some(cid), "runs on");
}

/// Keys to the editor, as typed (`<esc>`), the servers told at the end.
fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let key: crate::key::Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            _ => c.to_string().parse().unwrap(),
        };
        ed.handle_key(key);
    }
    ed.lsp_flush();
}

/// #533: the project's folder (the one tarae was opened on), its code trusted and its server — the stand-in — up
/// and ready; and a library outside it, a cargo root of its own (std's `library/`, a crate in the registry), with
/// `src/vec.rs`, `src/boxed.rs` and `src/rc.rs`. The project, the library and the server.
fn ready_project(
    s: &crate::scratch::Scratch,
    lib: &crate::scratch::Scratch,
) -> (Editor, PathBuf, PathBuf, crate::lsp::ClientId) {
    let (mut ed, dir) = setup(s);
    ed.workspace.root = dir.clone();
    ed.open(&dir.join("main.rs")).unwrap();
    assert!(until(&mut ed, |ed| card(ed).is_some()));
    let id = card(&ed).unwrap().id;
    ed.code_trust_answer(id, &dir, true);
    let cid = ed.doc().lsp.client.expect("started on the yes");
    let caps = serde_json::json!({ "positionEncoding": "utf-8", "definitionProvider": true });
    ed.on_lsp_message(cid, serde_json::json!({ "id": 0, "result": { "capabilities": caps } }));
    (ed, dir, library(lib, "Cargo.toml", "rs"), cid)
}

/// A library's folder (real) with `marker` in it and `src/{vec,boxed,rc}.{ext}`.
fn library(lib: &crate::scratch::Scratch, marker: &str, ext: &str) -> PathBuf {
    let l = std::fs::canonicalize(lib.path()).unwrap();
    std::fs::create_dir(l.join("src")).unwrap();
    std::fs::write(l.join(marker), "[package]\nname = \"alloc\"\n").unwrap();
    for f in ["vec", "boxed", "rc"] {
        std::fs::write(l.join("src").join(format!("{f}.{ext}")), "pub fn new() {}\n").unwrap();
    }
    l
}

/// `gd` in the front file, server `cid` answering with a place in `to`: its buffer in front once the read lands.
fn jump(ed: &mut Editor, cid: crate::lsp::ClientId, to: &Path) {
    use crate::lsp_editor::{Goto, Kind};
    feed(ed, "gd");
    let asked = ed.lsp.pending.iter().filter(|(_, p)| matches!(p.kind, Kind::Goto(Goto::Definition)));
    let id = asked.map(|((_, id), _)| *id).max().expect("asked");
    let at = serde_json::json!({ "uri": crate::lsp::uri(to), "range": {
        "start": { "line": 0, "character": 7 }, "end": { "line": 0, "character": 10 } } });
    ed.on_lsp_message(cid, serde_json::json!({ "id": id, "result": [at] }));
    assert!(until(ed, |ed| ed.doc().path.as_deref() == Some(to)), "jumped to {}", to.display());
}

/// What the stand-in `log` was sent, once it holds `what` (it writes as it reads).
fn sent(log: &Path, what: &str) -> String {
    let end = std::time::Instant::now() + crate::test_results::PATIENCE;
    loop {
        let got = std::fs::read_to_string(log).unwrap_or_default();
        if got.contains(what) || std::time::Instant::now() > end {
            return got;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// #533: from a trusted project, `gd` into a library outside it (std's `vec/mod.rs`, a registry crate) is served by
/// the project's rust-analyzer — its pack says it may: no card, no second server; the library's folder never gets a
/// trust entry, a server root or a workspace folder; the server is told the file; the pane says the server's ready, as
/// for the project's own files. A jump on from there keeps the project's root.
#[test]
fn a_jump_into_a_library_is_served_by_the_projects_server_and_asks_nothing() {
    let (s, lib) =
        (crate::scratch::Scratch::new("code-trust-reach"), crate::scratch::Scratch::new("code-trust-lib"));
    let (mut ed, dir, l, cid) = ready_project(&s, &lib);
    let vec = l.join("src/vec.rs");
    jump(&mut ed, cid, &vec);
    assert_eq!(ed.doc().lsp.client, Some(cid), "the project's server");
    let origin = Reached { server: "rust-analyzer".into(), root: dir.clone() };
    assert_eq!(ed.doc().lsp.reached.as_ref(), Some(&origin));
    assert!(card(&ed).is_none(), "no card");
    assert_eq!(ed.lsp_state(), Some((LspDot::Ready, "rust-analyzer".into())));
    let ras: Vec<&crate::lsp::Client> = ed.lsp.clients.iter().filter(|c| c.name == "rust-analyzer").collect();
    assert_eq!(ras.len(), 1, "no second server");
    assert!(!ras[0].root.starts_with(&l) && !ras[0].roots.iter().any(|r| r.starts_with(&l)), "never a root");
    let log = sent(&dir.join("ra.log"), "vec.rs");
    assert!(log.contains("textDocument/didOpen") && log.contains("vec.rs"), "told the file: {log}");
    assert!(!log.contains("didChangeWorkspaceFolders"), "never a workspace folder: {log}");
    // (on from there: still the project's)
    feed(&mut ed, "<esc>");
    let caps = serde_json::json!({ "positionEncoding": "utf-8", "definitionProvider": true });
    ed.on_lsp_message(cid, serde_json::json!({ "id": 0, "result": { "capabilities": caps } }));
    let boxed = l.join("src/boxed.rs");
    jump(&mut ed, cid, &boxed);
    assert_eq!((ed.doc().lsp.client, ed.doc().lsp.reached.as_ref()), (Some(cid), Some(&origin)));
    assert!(!ed.code_trust.contains_key(&l) && card(&ed).is_none(), "no trust entry for the library");
    ed.writes_flush();
}

/// #533: a server whose pack doesn't say `serves-outside-roots` (zls: it would run every `build.zig` from the file's
/// folder up) leaves a jump's file to its own root and its own card — which names that root by its path, kept at its
/// end, says it's outside this project and leaves #517's sentence to the folder tarae was opened on.
#[test]
fn a_server_without_the_key_leaves_the_library_its_own_card() {
    let (s, lib) =
        (crate::scratch::Scratch::new("code-trust-zls"), crate::scratch::Scratch::new("code-trust-zlib"));
    let dir = std::fs::canonicalize(s.path()).unwrap();
    std::fs::write(dir.join("main.zig"), "pub fn main() void {}\n").unwrap();
    let cfg = format!(
        "[lsp.zls]\ncommand = \"sh\"\nargs = [\"-c\", \"cat > '{}/zls.log'\"]\n[lang.zig]\nlsp = [\"zls\"]\n",
        dir.display()
    );
    let (config, w) = crate::config::parse(&cfg);
    assert!(w.is_empty(), "{w:?}");
    let mut ed = Editor::new(config);
    (ed.code_gate, ed.trust_path, ed.workspace.root) = (true, Some(dir.join("trust.json")), dir.clone());
    ed.open(&dir.join("main.zig")).unwrap();
    assert!(until(&mut ed, |ed| card(ed).is_some()));
    let c = card(&ed).unwrap();
    assert!(
        c.question.ends_with("A repository made here keeps the answer.")
            && !c.question.contains("outside this"),
        "the folder tarae was opened on: as ever, #517's sentence kept: {}",
        c.question
    );
    let id = c.id;
    ed.code_trust_answer(id, &dir, true);
    let cid = ed.doc().lsp.client.expect("started on the yes");
    let caps = serde_json::json!({ "positionEncoding": "utf-8", "definitionProvider": true });
    ed.on_lsp_message(cid, serde_json::json!({ "id": 0, "result": { "capabilities": caps } }));
    let l = library(&lib, "build.zig", "zig");
    let std = l.join("src/vec.zig");
    jump(&mut ed, cid, &std);
    assert!(ed.doc().lsp.reached.is_none() && ed.doc().lsp.client.is_none(), "not the project's");
    assert!(until(&mut ed, |ed| card(ed).is_some()), "its own card");
    let c = card(&ed).unwrap();
    let root = crate::lsp::find_root_for("zig", &std);
    assert!(matches!(&c.what, crate::offer::What::CodeTrust { root: r } if *r == root));
    let path = crate::allow::tilde(&root);
    assert!(
        c.question
            .starts_with(&format!("{path}\nIt's outside this project. Its language servers run its code"))
            && !c.question.contains("keeps the answer"),
        "{}",
        c.question
    );
    assert!(c.paths == [0] && c.ends.is_empty(), "the path: whole, never cut");
    assert_eq!(ed.lsp_state(), Some((LspDot::Untrusted, "zig".into())));
    ed.writes_flush();
}

/// #533: the path a card trusts is shown whole: broken only after a `/`, each row after the first two cells in, no
/// row wider than the card — at the 80 columns' card (56 cells) and narrower ones; a name wider than its row breaks
/// inside, nothing left out.
#[test]
fn a_trusted_path_is_shown_whole_broken_only_at_a_slash() {
    let p = "~/.rustup/toolchains/1.99.0-aarch64-apple-darwin/lib/rustlib/src/rust/library";
    let rows = crate::term::path_rows(p, 56);
    assert_eq!(rows, ["~/.rustup/toolchains/1.99.0-aarch64-apple-darwin/lib/", "  rustlib/src/rust/library"]);
    // (30: its widest name, 28 cells, just fits a row two cells in; 20: it doesn't — broken inside, still whole)
    for w in [56, 40, 30, 20] {
        let rows = crate::term::path_rows(p, w);
        assert_eq!(rows.iter().map(|r| r.trim_start()).collect::<String>(), p, "whole at {w}");
        assert!(rows.iter().all(|r| unicode_width::UnicodeWidthStr::width(r.as_str()) <= w), "{rows:?}");
        assert!(rows[1..].iter().all(|r| r.starts_with("  ")), "two cells in: {rows:?}");
        if w >= 30 {
            assert!(
                rows[..rows.len() - 1].iter().all(|r| r.ends_with('/')),
                "broken at a `/` at {w}: {rows:?}"
            );
        }
    }
    let long = crate::term::path_rows("/tmp/a-name-much-wider-than-its-row", 12);
    assert_eq!(
        long.iter().map(|r| r.trim_start()).collect::<String>(),
        "/tmp/a-name-much-wider-than-its-row"
    );
    assert_eq!(crate::term::path_rows("/tmp/itoa", 56), ["/tmp/itoa"]);
    // (a name wider than its row breaks at a grapheme cluster, never inside one: macOS gives names decomposed — NFD
    // Hangul as jamo, `é` as e + ◌́ — and a family emoji is people joined by U+200D)
    let nfd = "\u{1112}\u{1161}\u{11ab}\u{1100}\u{1173}\u{11af}\u{1111}\u{1169}\u{11af}\u{1103}\u{1165}";
    let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
    for name in [nfd.repeat(3), "cafe\u{301}".repeat(4), format!("{family}{family}{family}x")] {
        let p = format!("/tmp/{name}");
        let boundaries: Vec<usize> =
            unicode_segmentation::UnicodeSegmentation::grapheme_indices(p.as_str(), true)
                .map(|(i, _)| i)
                .chain([p.len()])
                .collect();
        for w in [7, 8, 9, 10] {
            let rows = crate::term::path_rows(&p, w);
            let parts: Vec<&str> =
                rows.iter().enumerate().map(|(i, r)| if i == 0 { &r[..] } else { &r[2..] }).collect();
            assert_eq!(parts.concat(), p, "whole at {w}: {rows:?}");
            let mut at = 0;
            for part in &parts {
                at += part.len();
                assert!(boundaries.contains(&at), "a cluster split at {w}: {rows:?}");
            }
        }
    }
}

/// #533: a path dependency whose own card you answered `n`: a jump into it from the trusted project is the project's
/// server's, and leaves its answer and its waiting files as they were; a file of it already open stays its own, held
/// (a jump never moves an open buffer); one opened by hand is held as ever.
#[test]
fn a_jump_into_a_refused_folder_leaves_its_answer_and_its_own_open_held() {
    let (s, lib) =
        (crate::scratch::Scratch::new("code-trust-q"), crate::scratch::Scratch::new("code-trust-qlib"));
    let (mut ed, dir, q, cid) = ready_project(&s, &lib);
    let (vec, boxed, rc) =
        (q.join("src/vec.rs"), q.join("src/vec.rs").with_file_name("boxed.rs"), q.join("src/rc.rs"));
    ed.open(&vec).unwrap();
    assert!(until(&mut ed, |ed| card(ed).is_some_and(|c| c.question.contains("outside this project"))));
    let id = card(&ed).unwrap().id;
    ed.code_trust_answer(id, &q, false);
    let entry = |ed: &Editor| ed.code_trust.get(&q).map(|r| (r.state, r.held.clone()));
    let before = entry(&ed);
    assert_eq!(before.as_ref().map(|b| b.0), Some(Some(State::Ignored)));
    ed.open(&dir.join("main.rs")).unwrap();
    jump(&mut ed, cid, &boxed);
    assert_eq!(ed.doc().lsp.client, Some(cid), "the project's server: its graph builds it already");
    assert_eq!(entry(&ed), before, "its answer and its waiting files untouched");
    ed.open(&dir.join("main.rs")).unwrap();
    jump(&mut ed, cid, &vec);
    assert!(ed.doc().lsp.reached.is_none() && ed.doc().lsp.client.is_none(), "open already: never moved");
    assert_eq!(ed.lsp_state(), Some((LspDot::Untrusted, "rust".into())));
    assert_eq!(entry(&ed), before);
    ed.open(&rc).unwrap();
    assert!(ed.doc().lsp.client.is_none() && card(&ed).is_none(), "by hand: held, not asked again");
    ed.writes_flush();
}

/// #533: `:code-untrust` from a file a jump reached takes back the project's trust — the file's server stops with the
/// project's, it waits on the project's card, and the project's yes serves it again.
#[test]
fn code_untrust_takes_a_reached_file_with_its_project() {
    let (s, lib) =
        (crate::scratch::Scratch::new("code-trust-ru"), crate::scratch::Scratch::new("code-trust-rulib"));
    let (mut ed, dir, l, cid) = ready_project(&s, &lib);
    let vec = l.join("src/vec.rs");
    jump(&mut ed, cid, &vec);
    assert!(until(&mut ed, |ed| ed.trust_writing == 0));
    ed.code_untrust().unwrap();
    assert!(!has_server(&ed, &vec), "stopped with the project's");
    assert!(until(&mut ed, |ed| card(ed).is_some()), "asked again");
    let c = card(&ed).unwrap();
    assert!(matches!(&c.what, crate::offer::What::CodeTrust { root } if *root == dir), "the project's card");
    assert_eq!(ed.lsp_state(), Some((LspDot::Untrusted, "rust".into())));
    let id = c.id;
    ed.code_trust_answer(id, &dir, true);
    assert!(has_server(&ed, &vec) && !ed.code_trust.contains_key(&l), "the project's again");
    ed.writes_flush();
}

/// #533: a file a jump reached is kept with its origin among the files to restore; the next tarae reopens it served
/// by the project's server again, with no card for the library.
#[test]
fn a_restart_keeps_a_reached_files_origin() {
    let (s, lib) =
        (crate::scratch::Scratch::new("code-trust-rs"), crate::scratch::Scratch::new("code-trust-rslib"));
    let (mut ed, dir, l, cid) = ready_project(&s, &lib);
    let vec = l.join("src/vec.rs");
    jump(&mut ed, cid, &vec);
    assert!(until(&mut ed, |ed| ed.trust_writing == 0), "the yes kept");
    ed.restore_record();
    let origin = Reached { server: "rust-analyzer".into(), root: dir.clone() };
    assert_eq!(ed.restore.reached(&dir, &vec), Some(origin.clone()), "kept with the files");
    let (mut next, _) = setup(&s);
    next.workspace.root = dir.clone();
    next.restore = std::mem::take(&mut ed.restore);
    assert_eq!(next.restore_files(), 2);
    let at = next.docs.iter().find(|d| d.path.as_deref() == Some(vec.as_path())).expect("reopened");
    assert_eq!(at.lsp.reached.as_ref(), Some(&origin));
    // (outside every checkout: its server once it's shown — #141)
    let now = crate::clock::now();
    next.servers_follow_sight(now);
    assert!(until(&mut next, |ed| has_server(ed, &vec)), "served again");
    assert!(card(&next).is_none() && !next.code_trust.contains_key(&l), "no card for the library");
    for e in [&mut ed, &mut next] {
        e.writes_flush();
    }
}

/// With the gate off (tests, or a server no pack marks), servers start as ever.
#[test]
fn a_server_no_pack_marks_starts() {
    let s = crate::scratch::Scratch::new("code-trust-off");
    let (mut ed, dir) = setup(&s);
    ed.code_gate = false;
    let rs = dir.join("main.rs");
    ed.open(&rs).unwrap();
    assert!(has_server(&ed, &rs) && card(&ed).is_none());
}
