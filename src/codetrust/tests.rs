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
