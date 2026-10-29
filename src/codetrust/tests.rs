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
    assert_eq!(c.title, TITLE);
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
    assert!(has_server(&ed, &in_a));
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
