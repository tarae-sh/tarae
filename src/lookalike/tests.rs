//! #610, landing 2: the first-trust card — asked once a version, its `y` a note that grants nothing, its cut said.

use std::path::{Path, PathBuf};

use super::*;
use crate::key::{Code, Key};
use crate::trust::{State, Store};

/// git in a test's own repository — no user or system config, no hooks, no signing, a name of its own.
fn git_here(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"])
        .args(["-c", "user.email=t@example.invalid", "-c", "user.name=t"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A repository with `n` folders git takes for a repository committed (`a/`, `b/`, …), and a trust store beside it:
/// (its scratch, the checkout, the store).
fn checkout(name: &str, n: usize) -> (crate::scratch::Scratch, PathBuf, PathBuf) {
    let s = crate::scratch::Scratch::new(name);
    let base = std::fs::canonicalize(s.path()).unwrap();
    let root = base.join("repo");
    std::fs::create_dir_all(&root).unwrap();
    for f in (b'a'..).take(n).map(|c| root.join((c as char).to_string())) {
        std::fs::create_dir_all(f.join("objects/info")).unwrap();
        std::fs::create_dir_all(f.join("refs/heads")).unwrap();
        std::fs::write(f.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(f.join("config"), "[core]\n\tbare = true\n").unwrap();
        std::fs::write(f.join("objects/info/keep"), "").unwrap();
        std::fs::write(f.join("refs/heads/keep"), "").unwrap();
    }
    std::fs::write(root.join("README.md"), "# here\n").unwrap();
    git_here(&root, &["init", "-q"]);
    git_here(&root, &["add", "-A"]);
    git_here(&root, &["commit", "-q", "-m", "base"]);
    (s, root, base.join("trust.json"))
}

/// An editor started in `root`, its answers kept in `store` — one start of tarae.
fn started(root: &Path, store: &Path) -> Editor {
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = root.to_path_buf();
    ed.trust_path = Some(store.to_path_buf());
    ed.workspace.viewer.keys_to_code();
    ed
}

/// The trust scan as the git watch makes it (its clock held: the line's limit never decides here), its result applied
/// and — something found — the store's word with it.
fn look(ed: &mut Editor) {
    let root = ed.workspace.root.clone();
    crate::clock::hold();
    let scan = crate::snapshot::scan(&root, None).unwrap();
    crate::clock::release();
    let asks = !scan.found.is_empty();
    ed.look_alikes_found(&root, scan);
    if asks {
        let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the store's word");
        ed.handle_event(ev);
    }
}

fn card(ed: &Editor) -> Option<&Offer> {
    ed.offers.iter().find(|o| o.kind() == "look-alike")
}

/// The front card drawn, given the keys and read, then `key` on it — as a person's hand does.
fn press(ed: &mut Editor, key: Key) {
    crate::term::render(ed, &mut Vec::new(), 140, 40).unwrap();
    let o = (ed.offers[0].id, ed.offers[0].rev);
    ed.offer_read_whole(o);
    ed.card_focus = Some(o);
    ed.handle_key(key);
}

fn letter(c: char) -> Key {
    Key::plain(Code::Char(c))
}

/// The answer written to the store.
fn written(ed: &mut Editor) {
    crate::test_results::settle_until(ed, |ed| ed.trust_writing == 0);
}

/// #610, landing 2: the first-trust card comes once a version — `y`, then not at the next start (its folder noted again
/// from the store); again once a folder's `config` changes; `esc` leaves it up unanswered, `n` asks again at the next
/// start (not on a rerun before it); with four folders three are named, a line each, and "and 1 more" said, `v` showing
/// each with its `HEAD`, its `config` under it (a tab as two spaces) and its hooks (not git's samples). Breaks if the store
/// isn't read.
#[test]
fn the_first_trust_card_comes_once() {
    let (_s, root, store) = checkout("look-once", 1);
    let mut ed = started(&root, &store);
    look(&mut ed);
    let o = card(&ed).expect("asked at the first start");
    assert_eq!(o.title, TITLE);
    assert!(
        o.question.starts_with(
            "a/ — here before Claude ran anything\n\ngit run from there or below — your terminal, a prompt's git \
             status — uses it instead of this project's repository and runs what its config and hooks name. tarae's \
             own git doesn't.\n\nNoted: asked again only when this changes. Not now: asked again when this project \
             is next opened.\n"
        ),
        "{}",
        o.question
    );
    assert!(o.decides(0) && !o.decides(2), "the folder at full strength, what git does faint");
    assert_eq!(o.answers(false), [('y', "Noted"), ('v', "Details"), ('n', "Not now")]);
    assert!(ed.workspace.made_protected.is_empty(), "it holds nothing");
    press(&mut ed, letter('y'));
    written(&mut ed);
    assert!(card(&ed).is_none());
    assert_eq!(ed.workspace.noted, [root.join("a")]);
    // The next start: not asked, noted again from the store
    let mut ed = started(&root, &store);
    look(&mut ed);
    assert!(card(&ed).is_none(), "noted: not asked again");
    assert_eq!(ed.workspace.noted, [root.join("a")], "noted again");
    // What decides it changed: asked again
    std::fs::write(root.join("a/config"), "[core]\n\thooksPath = /elsewhere\n").unwrap();
    let mut ed = started(&root, &store);
    look(&mut ed);
    assert!(card(&ed).is_some(), "its config changed: asked again");
    assert!(ed.workspace.noted.is_empty(), "this version isn't noted");
    // esc: the keys go back, the card stays unanswered; n: not again until the next start
    press(&mut ed, Key::plain(Code::Esc));
    assert!(card(&ed).is_some() && ed.card_focus.is_none(), "esc: up, unanswered");
    press(&mut ed, letter('n'));
    assert!(card(&ed).is_none());
    look(&mut ed);
    assert!(card(&ed).is_none(), "not now: a look before the next start doesn't ask");
    let mut ed = started(&root, &store);
    look(&mut ed);
    assert!(card(&ed).is_some(), "not now: the next start asks again");
    // Four: three named, then how many more; `v` shows each
    let (_s4, root4, store4) = checkout("look-four", 4);
    std::fs::create_dir_all(root4.join("d/hooks")).unwrap();
    for h in ["pre-commit.sample", "post-checkout", "pre-commit"] {
        std::fs::write(root4.join("d/hooks").join(h), "").unwrap();
    }
    // (b/: more hooks and config lines than `v` shows — each said cut)
    std::fs::create_dir_all(root4.join("b/hooks")).unwrap();
    for i in 0..13 {
        std::fs::write(root4.join(format!("b/hooks/h{i:02}")), "").unwrap();
    }
    std::fs::write(root4.join("b/config"), (0..8).map(|i| format!("[s{i}]\n")).collect::<String>()).unwrap();
    let mut ed = started(&root4, &store4);
    look(&mut ed);
    let o = card(&ed).expect("four");
    assert_eq!(o.title, TITLE_MANY);
    assert!(
        o.question.starts_with(
            "a/\nb/\nc/\nand 1 more\nAll here before Claude ran anything.\n\ngit run from there or below — your \
             terminal, a prompt's git status — uses them instead of this project's repository and runs what their \
             config and hooks name. tarae's own git doesn't.\n\nNoted: asked again only when this changes. Not now: \
             asked again when this project is next opened.\n"
        ),
        "{}",
        o.question
    );
    assert!(
        o.decides(4) && !o.decides(5),
        "every folder's line and \"All here\" at full strength, the blank after not"
    );
    press(&mut ed, letter('v'));
    let shown = ed.doc().text.to_string();
    for line in [
        "\nd/ — HEAD, objects/, refs/\n  HEAD: ref: refs/heads/main\n  config:\n    [core]\n      bare = true\n  \
         hooks:\n    post-checkout\n    pre-commit",
        "\n    [s5]\n    … (cut)\n  hooks:\n    h00\n",
        "\n    h11\n    … (cut)\n",
        "\nc/ — HEAD, objects/, refs/\n  HEAD: ref: refs/heads/main\n  config:\n    [core]\n      bare = true\n  \
         hooks: none\n",
    ] {
        assert!(shown.contains(line), "{line:?} in {shown}");
    }
    assert!(!shown.contains(".sample"), "git's own samples aren't named: {shown}");
    assert!(card(&ed).is_some(), "v: the card stays");
}

/// #610, landing 2 (the design session's condition): the card's `y` grants nothing. Kept as a no at its own place (the
/// repository's, file `.`), it leaves every other kind there unanswered, `:granted` lists nothing, and the file says
/// `look-alike` with a version of `false`. Breaks if it's kept as a yes.
#[test]
fn a_noted_look_alike_grants_nothing() {
    let (_s, root, store) = checkout("look-grants", 1);
    let mut ed = started(&root, &store);
    look(&mut ed);
    let ask = match &card(&ed).expect("its card").what {
        What::LookAlike(a) => (**a).clone(),
        w => panic!("{w:?}"),
    };
    press(&mut ed, letter('y'));
    written(&mut ed);
    let common = std::fs::canonicalize(root.join(".git")).unwrap();
    assert_eq!(ask.place, Place::Repo { common: common.clone(), file: PathBuf::from(".") });
    let s = Store::read(Some(&store));
    assert_eq!(s.state_at(&ask.place, ask.version), State::Ignored, "a note: never Trusted");
    assert_eq!(s.trusted_in(Some(&common), &root), [], "`:granted` lists nothing");
    // (every other kind's place in this repository: git's commands, tarae's settings, Claude's, a hook, the code)
    let others = [
        ".git/config",
        ".tarae.toml",
        ".claude/settings.json",
        ".claude/settings.local.json",
        ".git/hooks/x",
    ];
    for file in others {
        for hash in [ask.version, [0u8; 32]] {
            assert_eq!(s.state(&root.join(file), hash), State::Ask, "{file}");
        }
    }
    let code = Place::Repo { common: common.clone(), file: PathBuf::from("(its code)") };
    assert_eq!(s.state_at(&code, ask.version), State::Ask, "the code");
    assert!(!crate::gitrepo::answered_since(&root.join(".git/config"), ask.version), "git's commands");
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&store).unwrap()).unwrap();
    let answers = json["answers"].as_array().unwrap();
    assert_eq!(answers.len(), 1, "{json}");
    assert_eq!((answers[0]["kind"].as_str(), answers[0]["file"].as_str()), (Some("look-alike"), Some(".")));
    assert_eq!(answers[0]["versions"][0][1], serde_json::json!(false));
}

/// #610, landing 2: `n` (not now) on the card leaves things as they were and says so — the card's line says where it
/// leaves them; after it nothing is noted, nothing is written to the store (its place, every other kind's there `Ask`,
/// `:granted` empty) and the status line says when it's asked again. Breaks if `n` is taken as `y`, or the line goes.
#[test]
fn a_look_alike_not_now_notes_and_grants_nothing() {
    let (_s, root, store) = checkout("look-not-now", 1);
    let mut ed = started(&root, &store);
    look(&mut ed);
    let o = card(&ed).expect("its card");
    assert!(o.question.contains(&format!("\n\n{}\n", asked_again())), "{}", o.question);
    assert!(asked_again().ends_with(LATER), "{}", o.question);
    let ask = match &o.what {
        What::LookAlike(a) => (**a).clone(),
        w => panic!("{w:?}"),
    };
    press(&mut ed, letter('n'));
    assert!(card(&ed).is_none(), "n: down");
    assert_eq!(ed.workspace.noted, Vec::<PathBuf>::new(), "nothing noted");
    assert_eq!(ed.trust_writing, 0, "nothing queued for the store");
    assert!(!store.exists(), "nothing written");
    assert!(
        ed.toasts.iter().any(|t| t.text == "Not now — asked again when this project is next opened"),
        "{:?}",
        ed.toasts
    );
    let common = std::fs::canonicalize(root.join(".git")).unwrap();
    let s = Store::read(Some(&store));
    assert_eq!(s.state_at(&ask.place, ask.version), State::Ask, "unanswered");
    assert_eq!(s.trusted_in(Some(&common), &root), [], "`:granted` lists nothing");
    for file in [".git/config", ".tarae.toml", ".claude/settings.json", ".git/hooks/x"] {
        assert_eq!(s.state(&root.join(file), ask.version), State::Ask, "{file}");
    }
}

/// #610, landing 2: a cut scan with no card to say it is said in one toast — once since tarae started; one with a card
/// says it on the card. (The cut itself: `sim::trustscan::the_untracked_scan_is_cut_and_said`.)
#[test]
fn a_cut_with_no_card_is_said_once_in_a_toast() {
    let (_s, root, store) = checkout("look-cut-toast", 0);
    let mut ed = started(&root, &store);
    let cut = || Scan { slow: true, past: 12, ..Scan::default() };
    ed.look_alikes_found(&root, cut());
    let said = "Not looked through for folders git takes for a repository: the untracked files took over 2.0s and 12 \
                more folders holding a HEAD";
    assert_eq!(ed.toasts.iter().filter(|t| t.text == said).count(), 1, "{:?}", ed.toasts);
    // (a toast said again keeps its one place: cleared first, so a second saying would show)
    ed.toasts.clear();
    ed.look_alikes_found(&root, cut());
    assert!(!ed.toasts.iter().any(|t| t.text == said), "once since tarae started: {:?}", ed.toasts);
    let (_s1, root1, store1) = checkout("look-cut-card", 1);
    let mut ed = started(&root1, &store1);
    crate::clock::hold();
    let scan = Scan { slow: true, ..crate::snapshot::scan(&root1, None).unwrap() };
    crate::clock::release();
    ed.look_alikes_found(&root1, scan);
    let ev = ed.events.recv_timeout(crate::test_results::PATIENCE).expect("the store's word");
    ed.handle_event(ev);
    let o = card(&ed).expect("its card");
    // (right under the folder, at full strength, its lead short: the title says what was looked for)
    assert!(
        o.question.starts_with(
            "a/ — here before Claude ran anything\nNot looked at: the untracked files took over 2.0s.\n\ngit run "
        ),
        "{}",
        o.question
    );
    assert!(o.decides(1) && !o.decides(2), "the cut at full strength, the blank after not");
    assert!(!ed.toasts.iter().any(|t| t.text.starts_with("Not looked through")), "on the card, not a toast");
}

/// #610, landing 2: a cut of one folder past the scan's limit says "1 more folder", of several "folders" — on a card of
/// several folders under its "All here …" line, at full strength. Breaks if the count's plural is always said.
#[test]
fn a_cut_says_its_count_of_folders_right() {
    let (_s, root, _store) = checkout("look-cut-one", 2);
    crate::clock::hold();
    let scan = crate::snapshot::scan(&root, None).unwrap();
    crate::clock::release();
    let one = Scan { past: 1, ..scan.clone() };
    assert_eq!(one.cut_why().as_deref(), Some("1 more folder holding a HEAD"));
    let five = Scan { past: 5, ..scan };
    assert_eq!(five.cut_why().as_deref(), Some("5 more folders holding a HEAD"));
    let ask = Ask { place: place(&root), version: version(&one.found), root, scan: one };
    let o = ask.card();
    assert!(
        o.question.starts_with(
            "a/\nb/\nAll here before Claude ran anything.\nNot looked at: 1 more folder holding a HEAD.\n\ngit run "
        ),
        "{}",
        o.question
    );
    assert!(o.decides(3) && !o.decides(4), "the cut at full strength, the blank after not");
}
