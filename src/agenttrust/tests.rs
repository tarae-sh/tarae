//! Guard 4: a project's files the stand-in's row holds (`.stand-in/`), read on the gate's job and held until trusted —
//! the card, its answers, the store and its carry; the refusals no answer lifts.

use std::path::Path;

use serde_json::json;

use super::*;
use crate::agent::STAND_IN;
use crate::offer::{Offer, What};
use crate::scratch::Scratch;

/// What the stand-in says when it starts: its handshake, then a message's turn.
fn script() -> serde_json::Value {
    json!([
        {"expect": "initialize", "result": {"protocolVersion": 1, "agentCapabilities": {}}},
        {"expect": "session/new", "result": {"sessionId": "s-1", "modes": {"currentModeId": "default"}}},
        {"expect": "session/prompt"},
        {"end_turn": "end_turn"},
    ])
}

/// An editor in `root` whose chat is the stand-in's, its store in `dir`, its environment the stand-in's (`tag` names
/// its script and what it heard).
fn editor(dir: &Scratch, root: &Path, tag: &str) -> Editor {
    use crate::agent::acp::standin::{HEARD, SCRIPT};
    let script_at = dir.join(format!("{tag}.json"));
    std::fs::write(&script_at, script().to_string()).unwrap();
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let mut ed = Editor::with_root(crate::config::Config::default(), root.to_path_buf());
    ed.claude_env = Some(crate::llm::Env::from([
        (SCRIPT.into(), path(&script_at)),
        (HEARD.into(), path(&dir.join(format!("{tag}.heard")))),
        ("XDG_STATE_HOME".into(), path(&dir.join("state"))),
    ]));
    ed.trust_path = Some(dir.join("trust.json"));
    let mut c = crate::chat::Chat::new();
    (c.agent, c.agent_fixed) = (&STAND_IN, true);
    ed.workspace.claude.chat = Some(c);
    ed
}

/// What the stand-in tagged `tag` heard — nothing: it never started.
fn heard(dir: &Scratch, tag: &str) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(dir.join(format!("{tag}.heard"))).unwrap_or_default();
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn heard_method(dir: &Scratch, tag: &str, method: &str) -> bool {
    heard(dir, tag).iter().any(|v| v["method"] == method)
}

fn card(ed: &Editor) -> Option<&Offer> {
    ed.offers.iter().find(|o| matches!(o.what, What::AgentFiles { .. }))
}

fn chat(ed: &Editor) -> &crate::chat::Chat {
    ed.workspace.claude.chat.as_ref().unwrap()
}

fn says(ed: &Editor, words: &str) -> bool {
    chat(ed).msgs.iter().any(|m| m.text == words)
}

fn status_said(ed: &Editor, words: &str) -> bool {
    ed.messages.iter().any(|m| m.2 == words)
}

#[track_caller]
fn settle_until(ed: &mut Editor, done: impl Fn(&Editor) -> bool) {
    crate::test_results::settle_saying(ed, done, |ed| {
        let msgs: Vec<_> =
            ed.workspace.claude.chat.iter().flat_map(|c| c.msgs.iter().map(|m| &m.text)).collect();
        let said: Vec<_> = ed.messages.iter().map(|m| &m.2).collect();
        format!("the chat: {msgs:?}; said: {said:?}")
    });
}

/// A project with the stand-in's held folder holding `files` (path in it, text).
fn project(dir: &Scratch, name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let root = dir.join(name);
    for (rel, text) in files {
        let path = root.join(".stand-in").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// A linked checkout of the repository at `main` made by hand at `at`, git's links both ways.
fn linked(main: &Path, at: &Path) {
    let dir = main.join(".git/worktrees/wt");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(at).unwrap();
    std::fs::write(at.join(".git"), format!("gitdir: {}\n", dir.display())).unwrap();
    std::fs::write(dir.join("gitdir"), format!("{}\n", at.join(".git").display())).unwrap();
    std::fs::write(dir.join("commondir"), "../..\n").unwrap();
}

/// The digest of `files` (path from the root, its bytes) as §5 says: over (path, its bytes' SHA-256), in order.
fn digest(files: &[(&str, &str)]) -> Digest {
    let items: Vec<(String, String)> =
        files.iter().map(|(rel, text)| (rel.to_string(), crate::sha256::hex(text.as_bytes()))).collect();
    crate::trust::hash(&items)
}

/// Guard 4: an untrusted project's held files hold the agent — its folder made and the files read on one job, then
/// the card, no process: the stand-in never heard a word. The card names each file, a link with the listed file it
/// names, and says why it asks; a message sent meanwhile waits.
#[test]
fn an_untrusted_projects_files_hold_the_agent_and_the_card_names_them() {
    let dir = Scratch::new("agenttrust-held");
    let root = project(&dir, "p", &[("hooks.json", r#"{"hooks": {}}"#), ("rules.md", "be brief")]);
    std::os::unix::fs::symlink("hooks.json", root.join(".stand-in/same.json")).unwrap();
    let mut ed = editor(&dir, &root, "a");
    crate::chat::gate_opened(&mut ed);
    assert!(chat(&ed).starting(), "it waits for its folder and its files");
    settle_until(&mut ed, |ed| card(ed).is_some());
    let o = card(&ed).unwrap();
    assert_eq!(o.title, "Trust this folder's stand-in files?");
    let lines: Vec<&str> = o.question.lines().collect();
    assert_eq!(lines[0], "stand-in isn't started until you answer.");
    let link = "  .stand-in/same.json → .stand-in/hooks.json (a link)";
    // (`rules.md` isn't in `rules/`: it runs, as every file whose kind can't be told)
    assert_eq!(lines[2..], ["  .stand-in/hooks.json", "  .stand-in/rules.md", link, AFTER_YES]);
    let warned: Vec<usize> = o.warnings.iter().flat_map(|r| r.clone()).collect();
    assert_eq!(warned, [5], "the ▲ line in the warning colour");
    assert_eq!(o.answers(false), [('y', "Trust"), ('v', "Details"), ('n', "Hold back")]);
    assert!(says(&ed, "stand-in waits for your answer on the card — your message goes once it starts"));
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.input.set("hello");
    crate::chat::send(&mut ed);
    let c = chat(&ed);
    assert!(!c.running() && c.starting(), "not started, waiting");
    assert_eq!(c.queued.as_deref(), Some("hello"), "the message waits");
    assert!(heard(&dir, "a").is_empty(), "the stand-in started");
    let shown = crate::agenttrust::detail("stand-in", c.agent_gate.files.as_ref().unwrap());
    assert!(shown.contains("  .stand-in/rules.md") && shown.contains("outside tarae's sandbox"), "{shown}");
}

/// `y` keeps the answer at the repository's place and starts the agent once it's kept, the message that waited sent;
/// a second checkout of the repository with the same files starts with no card.
#[test]
fn a_yes_starts_it_and_is_kept_for_the_repository() {
    let dir = Scratch::new("agenttrust-yes");
    let files = [("hooks.json", r#"{"hooks": {}}"#)];
    let main = project(&dir, "main", &files);
    std::fs::create_dir(main.join(".git")).unwrap();
    let wt = dir.join("wt");
    linked(&main, &wt);
    project(&dir, "wt", &files);
    let mut ed = editor(&dir, &main, "main");
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| card(ed).is_some());
    assert_eq!(card(&ed).unwrap().title, "Trust this project's stand-in files?");
    ed.workspace.claude.chat.as_mut().unwrap().input.set("hello");
    crate::chat::send(&mut ed);
    ed.offer_accept();
    assert!(status_said(&ed, "Trusted — stand-in starts with this project's 1 file"));
    settle_until(&mut ed, |_| heard_method(&dir, "main", "session/prompt"));
    let prompt = heard(&dir, "main").into_iter().find(|v| v["method"] == "session/prompt").unwrap();
    assert!(prompt.to_string().contains("hello"), "the message that waited: {prompt}");
    let store = dir.join("trust.json");
    let seen = look(&STAND_IN, &wt, Some(&store)).unwrap();
    assert_eq!(seen.state, State::Trusted, "kept for the repository");
    assert!(matches!(seen.place, Place::Repo { .. }), "{:?}", seen.place);
    let mut second = editor(&dir, &wt, "wt");
    crate::chat::gate_opened(&mut second);
    settle_until(&mut second, |_| heard_method(&dir, "wt", "session/new"));
    assert!(card(&second).is_none(), "the second checkout asks nothing");
    ed.workspace.claude.chat = None;
    second.workspace.claude.chat = None;
}

/// The watch (L5): a file added under the held folder while the agent runs (a `git pull`) stops it — said once — and
/// their card asks again as its end lands, naming the file; `y` starts it again, with no second stop.
#[test]
fn a_file_added_while_it_runs_stops_it_and_asks() {
    let dir = Scratch::new("agenttrust-added");
    let root = project(&dir, "p", &[("hooks.json", "{}")]);
    let seen = look(&STAND_IN, &root, None).unwrap();
    let store = dir.join("trust.json");
    crate::trust::record(store, crate::trust::Kind::Agent, seen.file.clone(), seen.hash, true).unwrap();
    let script = json!([
        {"expect": "initialize", "result": {"protocolVersion": 1, "agentCapabilities": {}}},
        {"expect": "session/new", "result": {"sessionId": "s-1", "modes": {"currentModeId": "default"}}},
        {"eof": true},
    ]);
    let mut ed = editor(&dir, &root, "a");
    std::fs::write(dir.join("a.json"), script.to_string()).unwrap();
    crate::chat::gate_opened(&mut ed);
    let d = &dir;
    let new = move || heard(d, "a").iter().filter(|v| v["method"] == "session/new").count();
    let started = |n: usize| move |ed: &Editor| chat(ed).running() && new() == n;
    settle_until(&mut ed, started(1));
    assert!(card(&ed).is_none(), "trusted: no card");
    std::fs::write(root.join(".stand-in/added.json"), "{\"pulled\": true}").unwrap();
    settle_until(&mut ed, |ed| card(ed).is_some());
    let words = "stand-in's files in this project changed while it ran — stopped: it and everything it started ended. \
                 Their card asks again.";
    let n = |ed: &Editor| chat(ed).msgs.iter().filter(|m| m.text == words).count();
    assert_eq!(n(&ed), 1, "said once");
    assert!(!chat(&ed).running(), "stopped");
    let lines: Vec<String> = card(&ed).unwrap().question.lines().map(String::from).collect();
    assert_eq!(lines[2..], ["  .stand-in/added.json", "  .stand-in/hooks.json", AFTER_YES]);
    ed.offer_accept();
    settle_until(&mut ed, started(2));
    assert_eq!(n(&ed), 1, "no second stop");
    ed.workspace.claude.chat = None;
}

/// L4 note 6: a folder's entry that can't be read refuses the folder, as any unreadable thing does — never dropped
/// from the listing (its file would go unhashed while the agent may still load it). A made listing, as the OS's
/// entry errors (EIO and the like) can't be made on demand; `gather` lists every folder through it.
#[test]
fn a_folder_entry_that_cant_be_read_is_never_trusted() {
    use std::path::PathBuf;
    let made = |e: Vec<std::io::Result<&str>>| listed(e.into_iter().map(|r| r.map(PathBuf::from)));
    let failed = made(vec![Ok("b.json"), Err(std::io::Error::other("an entry unreadable")), Ok("a.json")]);
    assert_eq!(failed.map_err(|e| e.to_string()), Err("an entry unreadable".to_string()));
    let read = made(vec![Ok("b.json"), Ok("a.json")]).unwrap();
    assert_eq!(read, [PathBuf::from("a.json"), PathBuf::from("b.json")], "sorted");
}

/// `n`: not started in this project until you quit — never kept, a new chat in this tarae not started either (said,
/// no card), and a new tarae asks again.
#[test]
fn a_no_holds_it_until_you_quit() {
    let dir = Scratch::new("agenttrust-no");
    let root = project(&dir, "p", &[("hooks.json", "{}")]);
    let mut ed = editor(&dir, &root, "a");
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| card(ed).is_some());
    ed.offer_dismiss();
    let held =
        "Held back — stand-in isn't started in this project until you quit · :agent-files to look again";
    assert!(status_said(&ed, held));
    assert!(card(&ed).is_none() && !chat(&ed).running() && !chat(&ed).starting());
    assert_eq!(
        look(&STAND_IN, &root, Some(&dir.join("trust.json"))).unwrap().state,
        State::Ask,
        "never kept"
    );
    let mut c = crate::chat::Chat::new();
    (c.agent, c.agent_fixed) = (&STAND_IN, true);
    ed.workspace.claude.chat = Some(c);
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| says(ed, held));
    assert!(card(&ed).is_none() && !chat(&ed).running());
    assert!(heard(&dir, "a").is_empty(), "the stand-in started");
    let mut again = editor(&dir, &root, "b");
    crate::chat::gate_opened(&mut again);
    settle_until(&mut again, |ed| card(ed).is_some());
}

/// The answer is for the files as they were: a byte changed or a file added asks again — the store's yes and the yes
/// in hand alike; put back, the store's holds.
#[test]
fn a_changed_file_asks_again() {
    let dir = Scratch::new("agenttrust-changed");
    let root = project(&dir, "p", &[("hooks.json", "{}")]);
    let store = dir.join("trust.json");
    let state = || look(&STAND_IN, &root, Some(&store)).unwrap().state;
    let seen = look(&STAND_IN, &root, Some(&store)).unwrap();
    assert_eq!(seen.hash, digest(&[(".stand-in/hooks.json", "{}")]), "§5's digest");
    crate::trust::record(store.clone(), crate::trust::Kind::Agent, seen.file.clone(), seen.hash, true)
        .unwrap();
    assert_eq!(state(), State::Trusted);
    let hooks = root.join(".stand-in/hooks.json");
    std::fs::write(&hooks, "{ }").unwrap();
    assert_eq!(state(), State::Ask, "a byte changed");
    std::fs::write(&hooks, "{}").unwrap();
    assert_eq!(state(), State::Trusted, "put back");
    std::fs::write(root.join(".stand-in/more.toml"), "").unwrap();
    assert_eq!(state(), State::Ask, "a file added");
    std::fs::remove_file(root.join(".stand-in/more.toml")).unwrap();
    assert_eq!(state(), State::Trusted, "put back");
    // (the yes in hand is for its files only: changed since, the card asks, nothing starts)
    let dir2 = Scratch::new("agenttrust-changed-hand");
    let mut ed = editor(&dir2, &root, "a");
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    c.agent_gate.files = Some(Seen { state: State::Trusted, ..look(&STAND_IN, &root, None).unwrap() });
    std::fs::write(&hooks, "{\"changed\": 1}").unwrap();
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| card(ed).is_some());
    let now = look(&STAND_IN, &root, None).unwrap().hash;
    assert!(matches!(card(&ed).unwrap().what, What::AgentFiles { hash, .. } if hash == now));
    assert!(heard(&dir2, "a").is_empty(), "the stand-in started");
}

/// `to` linked at `at`, whatever was there gone first.
fn link(to: impl AsRef<Path>, at: &Path) {
    let _ = std::fs::remove_file(at);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(to, at).unwrap();
}

/// A link out of the held paths — any hop, resolved from its own folder, never read through — isn't plain: never
/// trusted. The agent is held and the chat says the link's name and its target, with no card. (Relative, a link goes
/// out only by `..`: `a_dot_dot_in_a_links_target_is_never_trusted`.)
#[test]
fn a_link_out_of_the_held_paths_holds_the_agent_and_says_why() {
    let dir = Scratch::new("agenttrust-link-out");
    let root = project(&dir, "p", &[("hooks.json", "{}")]);
    let held = root.join(".stand-in");
    let (far, x, y) = (dir.join("far.json"), held.join("x.json"), held.join("y.json"));
    std::fs::write(&far, "{}").unwrap();
    let refused = |rel: &str, to: &str| {
        format!("{rel}: a link to {to}, outside .stand-in/ — never trusted; stand-in isn't started here")
    };
    let looked = || look(&STAND_IN, &root, None).err().unwrap_or_else(|| "read".into());
    let far_said = far.display().to_string();
    link(&far, &x);
    assert_eq!(looked(), refused(".stand-in/x.json", &far_said), "absolute");
    link("./hooks.json", &x);
    assert_eq!(looked(), "read", "relative, in");
    link(held.join("hooks.json"), &x);
    assert_eq!(looked(), "read", "absolute, in");
    // (a hop out: the link that goes out is named)
    link("y.json", &x);
    link(&far, &y);
    assert_eq!(looked(), refused(".stand-in/y.json", &far_said), "the second hop");
    // (the held folder itself a link)
    std::fs::remove_dir_all(&held).unwrap();
    std::fs::create_dir(dir.join("elsewhere")).unwrap();
    std::fs::write(dir.join("elsewhere/hooks.json"), "{}").unwrap();
    link(dir.join("elsewhere"), &held);
    assert_eq!(looked(), refused(".stand-in", &dir.join("elsewhere").display().to_string()), "the folder");
    // Through the chat: said where it would start, no card, nothing started
    std::fs::remove_file(&held).unwrap();
    project(&dir, "p", &[("hooks.json", "{}")]);
    link(&far, &x);
    let mut ed = editor(&dir, &root, "a");
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| says(ed, &refused(".stand-in/x.json", &far_said)));
    assert!(card(&ed).is_none() && !chat(&ed).running() && !chat(&ed).starting());
    assert!(heard(&dir, "a").is_empty(), "the stand-in started");
}

/// A link inside the held paths to a file the listing holds is trusted with it: keyed by that file's path from the
/// root — re-pointed, it asks again; named another way, it doesn't — and the file digested by its bytes, so a change
/// to it asks again. The card shows the file it names.
#[test]
fn a_link_inside_to_a_listed_file_is_trusted_with_it_and_its_change_asks_again() {
    let dir = Scratch::new("agenttrust-link-in");
    let root = project(&dir, "p", &[("hooks.json", "{}"), ("rules.md", "be brief")]);
    let held = root.join(".stand-in");
    let (to, hooks) = (held.join("to.json"), held.join("hooks.json"));
    link("hooks.json", &to);
    let store = dir.join("trust.json");
    let state = || look(&STAND_IN, &root, Some(&store)).unwrap().state;
    let seen = look(&STAND_IN, &root, Some(&store)).unwrap();
    let sha = |s: &str| crate::sha256::hex(s.as_bytes());
    let items = [
        (".stand-in/hooks.json".to_string(), sha("{}")),
        (".stand-in/rules.md".to_string(), sha("be brief")),
        (".stand-in/to.json".to_string(), "link\0.stand-in/hooks.json".to_string()),
    ];
    assert_eq!(seen.hash, crate::trust::hash(&items), "the link by the file it names");
    let (q, ..) = question(&STAND_IN, &seen);
    assert!(q.contains("\n  .stand-in/to.json → .stand-in/hooks.json (a link)"), "{q}");
    crate::trust::record(store.clone(), crate::trust::Kind::Agent, seen.file.clone(), seen.hash, true)
        .unwrap();
    assert_eq!(state(), State::Trusted);
    link(&hooks, &to);
    assert_eq!(state(), State::Trusted, "the same file named another way");
    std::fs::write(&hooks, "{\"hooks\": 1}").unwrap();
    assert_eq!(state(), State::Ask, "the file it names changed");
    std::fs::write(&hooks, "{}").unwrap();
    assert_eq!(state(), State::Trusted, "put back");
    link("rules.md", &to);
    assert_eq!(state(), State::Ask, "re-pointed");
    link("hooks.json", &to);
    assert_eq!(state(), State::Trusted, "pointed back");
}

/// A `..` in a link's target is never followed: the OS pops what the component before it resolves to — through a
/// link, somewhere else — so a lexical `sub/../x` isn't `x`. Refused with the link's name, whatever the rest resolves
/// to, relative or absolute; the agent held and said, with no card.
#[test]
fn a_dot_dot_in_a_links_target_is_never_trusted() {
    let dir = Scratch::new("agenttrust-link-up");
    let root = project(&dir, "p", &[("x", "{}")]);
    let (held, a) = (root.join(".stand-in"), root.join(".stand-in/a"));
    let refused = |rel: &str, to: &str| {
        format!("{rel}: a link to {to}, through .. — never trusted; stand-in isn't started here")
    };
    let looked = || look(&STAND_IN, &root, None).err().unwrap_or_else(|| "read".into());
    // (absolute, through a link outside the root: `out/..` is `elsewhere`, not the scratch folder — lexically a
    // listed file, nothing else refused)
    std::fs::create_dir_all(dir.join("elsewhere/deeper")).unwrap();
    link(dir.join("elsewhere/deeper"), &dir.join("out"));
    let through = dir.join("out/../p/.stand-in/x");
    link(&through, &a);
    assert_eq!(looked(), refused(".stand-in/a", &through.display().to_string()), "through a link outside");
    // (`sub` a link out, `a` through it to a listed file's name: the OS lands on `/x`; `/etc` is only a link's text,
    // never followed. `sub` is refused on its own too; `a` is named first, for its own reason)
    link("/etc", &held.join("sub"));
    link("sub/../x", &a);
    assert_eq!(looked(), refused(".stand-in/a", "sub/../x"), "through a link in the held folder");
    std::fs::remove_file(held.join("sub")).unwrap();
    // (up and back in, up and out, past the root: refused alike)
    for to in ["../.stand-in/x", "../near.json", &("../".repeat(64) + "far")] {
        link(to, &a);
        assert_eq!(looked(), refused(".stand-in/a", to), "{to}");
    }
    link("x", &a);
    assert_eq!(looked(), "read", "the same file named without ..");
    // Through the chat: said where it would start, no card, nothing started
    link("sub/../x", &a);
    let mut ed = editor(&dir, &root, "a");
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| says(ed, &refused(".stand-in/a", "sub/../x")));
    assert!(card(&ed).is_none() && !chat(&ed).running() && !chat(&ed).starting());
    assert!(heard(&dir, "a").is_empty(), "the stand-in started");
}

/// A link to nothing, to a folder, or a chain past `HOPS_MAX`: never trusted, said, no card.
#[test]
fn a_dangling_link_or_one_to_a_folder_is_never_trusted() {
    let dir = Scratch::new("agenttrust-link-bad");
    let root = project(&dir, "p", &[("hooks.json", "{}"), ("sub/x", "")]);
    let held = root.join(".stand-in");
    let a = held.join("a");
    let refused = |rel: &str, why: &str| format!("{rel}: {why} — never trusted; stand-in isn't started here");
    let looked = || look(&STAND_IN, &root, None).err().unwrap_or_else(|| "read".into());
    link("gone.json", &a);
    assert_eq!(looked(), refused(".stand-in/a", "a link to gone.json, which isn't there"));
    link("sub", &a);
    assert_eq!(looked(), refused(".stand-in/a", "a link to sub, a folder"));
    link(".", &a);
    assert_eq!(looked(), refused(".stand-in/a", "a link to ., a folder"), "the held folder");
    link("a", &a);
    assert_eq!(looked(), refused(".stand-in/a", "a chain of more than 4 links"), "itself");
    std::fs::remove_file(&a).unwrap();
    // (a chain of `HOPS_MAX` links reaches its file; one more doesn't)
    for i in 1..HOPS_MAX {
        link(format!("l{}", i + 1), &held.join(format!("l{i}")));
    }
    link("hooks.json", &held.join(format!("l{HOPS_MAX}")));
    assert_eq!(looked(), "read", "{HOPS_MAX} hops");
    link("l1", &held.join("l0"));
    assert_eq!(looked(), refused(".stand-in/l0", "a chain of more than 4 links"));
    // Through the chat: said where it would start, no card, nothing started
    link("gone.json", &held.join("l0"));
    let mut ed = editor(&dir, &root, "a");
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| says(ed, &refused(".stand-in/l0", "a link to gone.json, which isn't there")));
    assert!(card(&ed).is_none() && !chat(&ed).running() && !chat(&ed).starting());
    assert!(heard(&dir, "a").is_empty(), "the stand-in started");
}

/// Past a bound — a file over `FILE_MAX`, more than `FILES_MAX` files, folders past `DEPTH_MAX` — a file that isn't
/// plain or can't be read: never trusted, the agent not started there, said in the chat with no card.
#[test]
fn an_unreadable_or_huge_file_is_never_trusted() {
    let dir = Scratch::new("agenttrust-unreadable");
    let root = project(&dir, "p", &[]);
    let held = root.join(".stand-in");
    let refused = |rel: &str, why: &str| format!("{rel}: {why} — never trusted; stand-in isn't started here");
    let looked = || look(&STAND_IN, &root, None).err().unwrap_or_else(|| "read".into());
    let clear = || {
        let _ = std::fs::remove_dir_all(&held);
        std::fs::create_dir(&held).unwrap();
    };
    clear();
    std::fs::write(held.join("big"), vec![b'x'; FILE_MAX as usize + 1]).unwrap();
    assert_eq!(looked(), refused(".stand-in/big", "too big to read"));
    std::fs::write(held.join("big"), vec![b'x'; FILE_MAX as usize]).unwrap();
    assert_eq!(looked(), "read", "at the bound");
    clear();
    for i in 0..=FILES_MAX {
        std::fs::write(held.join(format!("f{i:03}")), "").unwrap();
    }
    assert_eq!(looked(), refused(&format!(".stand-in/f{FILES_MAX:03}"), "more files than tarae reads"));
    clear();
    let deep = held.join("d/".repeat(DEPTH_MAX));
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("x"), "").unwrap();
    let rel = format!(".stand-in/{}", ["d"; DEPTH_MAX].join("/"));
    assert_eq!(looked(), refused(&rel, "more files than tarae reads"));
    clear();
    std::fs::create_dir_all(held.join("d/".repeat(DEPTH_MAX - 1))).unwrap();
    std::fs::write(held.join("d/".repeat(DEPTH_MAX - 1)).join("x"), "").unwrap();
    assert_eq!(looked(), "read", "as deep as it reads");
    clear();
    let fifo = std::ffi::CString::new(held.join("fifo").into_os_string().into_encoded_bytes()).unwrap();
    // SAFETY: a path of ours, NUL-ended.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert_eq!(looked(), refused(".stand-in/fifo", "not a plain file"));
    clear();
    // (a file this user can't read: root reads any)
    // SAFETY: geteuid only reads.
    if unsafe { libc::geteuid() } != 0 {
        std::fs::write(held.join("shut"), "x").unwrap();
        std::fs::set_permissions(held.join("shut"), std::os::unix::fs::PermissionsExt::from_mode(0o000))
            .unwrap();
        assert_eq!(looked(), refused(".stand-in/shut", "Permission denied (os error 13)"));
        clear();
    }
    // Through the chat: said where it would start, no card, nothing started
    std::fs::write(held.join("big"), vec![b'x'; FILE_MAX as usize + 1]).unwrap();
    let mut ed = editor(&dir, &root, "a");
    crate::chat::gate_opened(&mut ed);
    let said = refused(".stand-in/big", "too big to read");
    settle_until(&mut ed, |ed| says(ed, &said));
    assert!(card(&ed).is_none() && !chat(&ed).running() && !chat(&ed).starting());
    assert!(heard(&dir, "a").is_empty(), "the stand-in started");
}

/// The one refusal no answer lifts: a held file naming an MCP server as tarae's (any case, by key where it's JSON, by
/// the word in a `.json` that isn't) — refused untrusted, and with a yes kept for those very files.
#[test]
fn a_server_named_tarae_is_refused_trusted_or_not() {
    let dir = Scratch::new("agenttrust-tarae");
    let text = json!({"mcpServers": {"other": {}, "Tarae": {"command": "x"}}}).to_string();
    let root = project(&dir, "p", &[("mcp_config.json", &text)]);
    let store = dir.join("trust.json");
    let looked = || look(&STAND_IN, &root, Some(&store)).err().unwrap_or_else(|| "read".into());
    let refused = |file: &str, server: &str| {
        format!(
            "{file} names an MCP server \"{server}\" — its tools would pass for tarae's; stand-in isn't started here"
        )
    };
    assert_eq!(looked(), refused(".stand-in/mcp_config.json", "Tarae"));
    let hash = digest(&[(".stand-in/mcp_config.json", &text)]);
    let file = root.join(".stand-in");
    crate::trust::record(store.clone(), crate::trust::Kind::Agent, file, hash, true).unwrap();
    assert_eq!(looked(), refused(".stand-in/mcp_config.json", "Tarae"), "trusted: still refused");
    let config = root.join(".stand-in/mcp_config.json");
    std::fs::write(&config, json!({"mcpServers": {"taraexyz": {}, "o": {"args": ["tarae"]}}}).to_string())
        .unwrap();
    assert_eq!(looked(), "read", "another name, the word elsewhere");
    std::fs::write(&config, "{ // tarae").unwrap();
    assert_eq!(looked(), refused(".stand-in/mcp_config.json", "tarae"), "not JSON: by the word");
    std::fs::remove_file(&config).unwrap();
    std::fs::write(root.join(".stand-in/notes.md"), "tarae").unwrap();
    std::fs::write(root.join(".stand-in/plugin"), json!({"servers": [{"name": "tarae"}]}).to_string())
        .unwrap();
    assert_eq!(looked(), refused(".stand-in/plugin", "tarae"), "JSON by another name");
    std::fs::write(&config, &text).unwrap();
    std::fs::remove_file(root.join(".stand-in/plugin")).unwrap();
    let mut ed = editor(&dir, &root, "a");
    crate::chat::gate_opened(&mut ed);
    let said = refused(".stand-in/mcp_config.json", "Tarae");
    settle_until(&mut ed, |ed| says(ed, &said));
    assert!(card(&ed).is_none() && !chat(&ed).running());
    assert!(heard(&dir, "a").is_empty(), "the stand-in started");
}

/// #517: the files answered in a folder, then a repository made there (`git init`'s folder, by hand): the agent starts
/// as answered with no card, the carry said once, the answer now the repository's.
#[test]
fn a_folders_answer_is_carried_to_the_repository_made_there() {
    let dir = Scratch::new("agenttrust-carry");
    let root = project(&dir, "p", &[("hooks.json", "{}")]);
    let store = dir.join("trust.json");
    let seen = look(&STAND_IN, &root, Some(&store)).unwrap();
    assert!(matches!(seen.place, Place::File(_)), "{:?}", seen.place);
    crate::trust::record(store.clone(), crate::trust::Kind::Agent, seen.file, seen.hash, true).unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();
    let mut ed = editor(&dir, &root, "a");
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |_| heard_method(&dir, "a", "session/new"));
    assert!(card(&ed).is_none(), "no card");
    let name = std::fs::canonicalize(&root).unwrap().file_name().unwrap().to_string_lossy().to_string();
    let words = format!("trust carried from {name} — answered before it was a repository");
    assert_eq!(ed.messages.iter().filter(|m| m.2 == words).count(), 1);
    let now = look(&STAND_IN, &root, Some(&store)).unwrap();
    assert!(matches!(now.place, Place::Repo { .. }) && now.state == State::Trusted, "{now:?}");
    ed.workspace.claude.chat = None;
}

// ── L-B2: every held folder up to the git top ─────────────────────────────────────────────────

/// `text` at `rel` in the stand-in's held folder of `at`.
fn held_at(at: &Path, rel: &str, text: &str) {
    let path = at.join(".stand-in").join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The files `look` read, as the card says them.
fn listed_lines(root: &Path) -> Vec<String> {
    look(&STAND_IN, root, None).unwrap().files.iter().map(line).collect()
}

/// L-B2 (#16, measured on 1.5.9: `proj`, `mid` and `top` loaded, `outer` above the top not): every held folder from
/// the root up to the git top is one listing, keyed from the top. A link in the root's to a parent's file is trusted
/// with it (the links rule's roots are the chain's folders), one to the folder above the top isn't; the card names the
/// parents' files with the root's.
#[test]
fn every_held_folder_up_to_the_git_top_is_one_listing() {
    let dir = Scratch::new("agenttrust-chain");
    let top = dir.join("outer/top");
    repository_at(&top);
    held_at(&dir.join("outer"), "o.json", "{}");
    held_at(&top, "t.json", "1");
    held_at(&top.join("mid"), "m.json", "2");
    let root = top.join("mid/proj");
    held_at(&root, "p.json", "3");
    let to = root.join(".stand-in/to.json");
    link(top.join("mid/.stand-in/m.json"), &to);
    let files = ["  .stand-in/t.json", "  mid/.stand-in/m.json", "  mid/proj/.stand-in/p.json"];
    let with_link = [&files[..], &["  mid/proj/.stand-in/to.json → mid/.stand-in/m.json (a link)"]].concat();
    assert_eq!(listed_lines(&root), with_link, "absolute, to a parent's file");
    let outer = dir.join("outer/.stand-in/o.json");
    link(&outer, &to);
    let refused = format!(
        "mid/proj/.stand-in/to.json: a link to {}, outside .stand-in/ — never trusted; stand-in isn't started here",
        outer.display()
    );
    assert_eq!(look(&STAND_IN, &root, None).err(), Some(refused), "to the folder above the top");
    std::fs::remove_file(&to).unwrap();
    let mut ed = editor(&dir, &root, "a");
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| card(ed).is_some());
    let o = card(&ed).unwrap();
    assert_eq!(o.title, "Trust this project's stand-in files?");
    let mut want = files.to_vec();
    want.push(AFTER_YES);
    assert_eq!(o.question.lines().skip(2).collect::<Vec<_>>(), want);
    assert!(heard(&dir, "a").is_empty(), "the stand-in started");
}

/// A yes is for the whole chain's bytes: a parent's held file changed, or one added in the top's, asks again.
#[test]
fn a_change_in_a_parents_held_file_asks_again() {
    let dir = Scratch::new("agenttrust-chain-changed");
    let top = dir.join("top");
    repository_at(&top);
    held_at(&top.join("mid"), "m.json", "{}");
    let root = top.join("mid/proj");
    held_at(&root, "p.json", "{}");
    let store = dir.join("trust.json");
    let state = || look(&STAND_IN, &root, Some(&store)).unwrap().state;
    let seen = look(&STAND_IN, &root, Some(&store)).unwrap();
    assert_eq!(seen.hash, digest(&[("mid/.stand-in/m.json", "{}"), ("mid/proj/.stand-in/p.json", "{}")]));
    crate::trust::record(store.clone(), crate::trust::Kind::Agent, seen.file.clone(), seen.hash, true)
        .unwrap();
    assert_eq!(state(), State::Trusted);
    held_at(&top.join("mid"), "m.json", "{ }");
    assert_eq!(state(), State::Ask, "a parent's byte changed");
    held_at(&top.join("mid"), "m.json", "{}");
    assert_eq!(state(), State::Trusted, "put back");
    held_at(&top, "added.json", "");
    assert_eq!(state(), State::Ask, "a file added in the top's");
}

/// With no git top only the root's held folder is held: all 1.5.9 loads there (measured, probe 2 run D: `nproj`
/// alone, not `na`, `nogit` nor the scratch root's) — so the card says nothing more. A `.git` made above, and the
/// folders up to it join.
#[test]
fn with_no_git_top_only_the_roots_is_held() {
    let dir = Scratch::new("agenttrust-nogit");
    let nogit = dir.join("nogit");
    held_at(&nogit, "n.json", "1");
    held_at(&nogit.join("a"), "a.json", "2");
    let root = nogit.join("a/proj");
    held_at(&root, "p.json", "3");
    let real = std::fs::canonicalize(&root).unwrap();
    assert!(real.ancestors().all(|d| !d.join(".git").exists()), "the scratch folder is in a repository");
    assert_eq!(listed_lines(&root), ["  .stand-in/p.json"]);
    let mut ed = editor(&dir, &root, "a");
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| card(ed).is_some());
    let o = card(&ed).unwrap();
    assert_eq!(o.title, "Trust this folder's stand-in files?");
    let head = "stand-in isn't started until you answer.";
    let lines = o.question.lines().collect::<Vec<_>>();
    assert_eq!(lines, [head, "", "  .stand-in/p.json", AFTER_YES], "nothing more said");
    ed.workspace.claude.chat = None;
    repository_at(&nogit);
    assert_eq!(
        listed_lines(&root),
        ["  .stand-in/n.json", "  a/.stand-in/a.json", "  a/proj/.stand-in/p.json"]
    );
}

/// A held file anywhere in the chain naming an MCP server as tarae's refuses the start, trusted or not: a parent's
/// too, not the root's alone.
#[test]
fn a_parents_held_file_naming_a_server_tarae_refuses_the_start() {
    let dir = Scratch::new("agenttrust-chain-tarae");
    let top = dir.join("top");
    repository_at(&top);
    let text = json!({"mcpServers": {"tarae": {"command": "x"}}}).to_string();
    held_at(&top.join("mid"), "mcp_config.json", &text);
    let root = top.join("mid/proj");
    held_at(&root, "hooks.json", "{}");
    let store = dir.join("trust.json");
    let said = "mid/.stand-in/mcp_config.json names an MCP server \"tarae\" — its tools would pass for tarae's; \
                stand-in isn't started here";
    assert_eq!(look(&STAND_IN, &root, Some(&store)).err().as_deref(), Some(said));
    let hash = digest(&[("mid/.stand-in/mcp_config.json", &text), ("mid/proj/.stand-in/hooks.json", "{}")]);
    crate::trust::record(store.clone(), crate::trust::Kind::Agent, root.join(".stand-in"), hash, true)
        .unwrap();
    assert_eq!(look(&STAND_IN, &root, Some(&store)).err().as_deref(), Some(said), "trusted: still refused");
    let mut ed = editor(&dir, &root, "a");
    crate::chat::gate_opened(&mut ed);
    settle_until(&mut ed, |ed| says(ed, said));
    assert!(card(&ed).is_none() && !chat(&ed).running());
    assert!(heard(&dir, "a").is_empty(), "the stand-in started");
}

/// A linked checkout's `.git` file is its top — measured, 1.5.9 (probe 2 run B: `wt` and `sub` loaded; `wtop`, a
/// plain folder above, and `flat`, its main checkout, not).
#[test]
fn a_worktrees_git_file_is_its_top() {
    let dir = Scratch::new("agenttrust-worktree");
    let (flat, wtop) = (dir.join("flat"), dir.join("wtop"));
    let wt = wtop.join("wt");
    repository_at(&flat);
    linked(&flat, &wt);
    held_at(&flat, "f.json", "1");
    held_at(&wtop, "x.json", "2");
    held_at(&wt, "w.json", "3");
    let root = wt.join("sub");
    held_at(&root, "s.json", "4");
    assert!(std::fs::symlink_metadata(wt.join(".git")).unwrap().is_file(), "its .git a file");
    assert_eq!(listed_lines(&root), ["  .stand-in/w.json", "  sub/.stand-in/s.json"]);
}

/// The design session's note 1 on L-B2: a `.git` that's no repository's — an empty folder, one holding `HEAD` alone, a
/// file that doesn't start `gitdir:`, a FIFO (opened without waiting) — isn't the top: git walks past it (or refuses
/// it), and whether agy's top is git's answer is unmeasured, so the chain walks on to the repository above and holds
/// every folder up to it. The FIFO through the whole look too, bounded by the shared patience (#607: `trust::place_of`
/// waited on it for good): the answer's place is then the folder's own, no repository's.
#[test]
fn a_git_folder_that_is_no_repository_is_not_the_top() {
    let dir = Scratch::new("agenttrust-not-a-top");
    let top = dir.join("top");
    repository_at(&top);
    let (mid, odd) = (top.join("mid"), top.join("mid/odd"));
    let root = odd.join("proj");
    for (at, name) in [(&top, "t"), (&mid, "m"), (&odd, "o"), (&root, "p")] {
        held_at(at, &format!("{name}.json"), "{}");
    }
    std::fs::create_dir(mid.join(".git")).unwrap();
    std::fs::create_dir(odd.join(".git")).unwrap();
    std::fs::write(odd.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(root.join(".git"), "not a gitdir\n").unwrap();
    let all = [
        "  .stand-in/t.json",
        "  mid/.stand-in/m.json",
        "  mid/odd/.stand-in/o.json",
        "  mid/odd/proj/.stand-in/p.json",
    ];
    assert_eq!(listed_lines(&root), all, "an empty folder, HEAD alone, a junk file");
    std::fs::remove_file(root.join(".git")).unwrap();
    let fifo = std::ffi::CString::new(root.join(".git").to_str().unwrap()).unwrap();
    // SAFETY: a plain call on a path of the test's own scratch folder.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert_eq!(chain(&root).top, std::fs::canonicalize(&top).unwrap(), "a FIFO, not waited on");
    // (#607: the whole look too — `trust::place_of` reads the root's `.git` through `allow::common_dir`; bounded by the
    // shared patience, so a read that waits on the FIFO fails here rather than hangs)
    let (tx, rx) = std::sync::mpsc::channel();
    let at = root.clone();
    std::thread::spawn(move || tx.send(listed_lines(&at)));
    let listed = rx.recv_timeout(crate::test_results::PATIENCE).expect("a look waited on a FIFO named .git");
    assert_eq!(listed, all, "a FIFO, through the look");
    let place = crate::trust::place_of(&root.join(".stand-in"));
    let real = std::fs::canonicalize(&root).unwrap().join(".stand-in");
    assert_eq!(place, crate::trust::Place::File(real), "no repository's: the folder's own place");
    std::fs::remove_file(root.join(".git")).unwrap();
    std::fs::write(root.join(".git"), "gitdir: /nowhere\n").unwrap();
    assert_eq!(listed_lines(&root), ["  .stand-in/p.json"], "a gitdir file is a top");
}

/// The design session's fix on L-B3: with no repository above the root, a `.git` that's no repository's two levels up
/// (`make`) is the top — a walk of agy's own may stop there (unmeasured, plan §1c) and load every `.agents/` from it
/// down to the root — so all three folders are held, and none above it.
fn no_repository_and_a_git_of_another_kind_above(name: &str, make: impl Fn(&Path)) {
    let dir = Scratch::new(name);
    let above = dir.join("above");
    let x = above.join("x");
    held_at(&above, "n.json", "0");
    held_at(&x, "x.json", "1");
    held_at(&x.join("a"), "a.json", "2");
    let root = x.join("a/proj");
    held_at(&root, "p.json", "3");
    let real = std::fs::canonicalize(&above).unwrap();
    assert!(real.ancestors().all(|d| !d.join(".git").exists()), "the scratch folder is in a repository");
    make(&x.join(".git"));
    assert_eq!(chain(&root).top, std::fs::canonicalize(&x).unwrap(), "its top");
    assert_eq!(
        listed_lines(&root),
        ["  .stand-in/x.json", "  a/.stand-in/a.json", "  a/proj/.stand-in/p.json"]
    );
}

/// `no_repository_and_a_git_of_another_kind_above` with an empty `.git` folder.
#[test]
fn with_no_repository_an_empty_git_folder_above_is_the_top() {
    no_repository_and_a_git_of_another_kind_above("agenttrust-empty-git", |g| {
        std::fs::create_dir(g).unwrap()
    });
}

/// `no_repository_and_a_git_of_another_kind_above` with a junk `.git` file.
#[test]
fn with_no_repository_a_junk_git_file_above_is_the_top() {
    no_repository_and_a_git_of_another_kind_above("agenttrust-junk-git", |g| {
        std::fs::write(g, "not a gitdir\n").unwrap();
    });
}

/// The variable that has `git_open_in_its_own_process` run (else it returns at once: `--ignored` alone doesn't).
#[cfg(target_os = "linux")]
const NOCTTY_CHILD: &str = "TARAE_TEST_NOCTTY_CHILD";

/// The design session's item on L-B3: a `.git` file's open (`open_git_file`) never takes a terminal — a tty swapped in
/// for `.git` after the stat said plain file, opened by a session leader with none, isn't its terminal after. Linux
/// only: macOS takes a terminal only by `TIOCSCTTY`, never on an open, and closes every tty to a sandboxed run. In a
/// process of its own, which `setsid`s (`git_open_in_its_own_process`).
#[cfg(target_os = "linux")]
#[test]
fn a_tty_named_git_is_never_taken_as_a_terminal() {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "agenttrust::tests::git_open_in_its_own_process", "--ignored", "--nocapture"])
        .env(NOCTTY_CHILD, "1")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && said.contains("1 passed"), "the child ran it: {said}");
}

/// `a_tty_named_git_is_never_taken_as_a_terminal`'s child: a session of its own, with no terminal, opens a pty's
/// slave (one of its own making, nobody's terminal: `pty::open`, both ends `O_NOCTTY`) again by its path, as
/// `open_git_file` opens a `.git`, and still has no terminal.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "run by a_tty_named_git_is_never_taken_as_a_terminal in a process of its own"]
fn git_open_in_its_own_process() {
    use std::os::fd::AsRawFd;
    if std::env::var_os(NOCTTY_CHILD).is_none() {
        return;
    }
    let no_terminal = || std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").is_err();
    // SAFETY: a plain call in this process of its own.
    assert!(unsafe { libc::setsid() } > 0, "a session of its own");
    assert!(no_terminal(), "a terminal after setsid");
    let (_master, slave) = crate::pty::open(24, 80).expect("a pty");
    assert!(no_terminal(), "pty::open took its slave as the terminal");
    let path = format!("/proc/self/fd/{}", slave.as_raw_fd());
    let opened = open_git_file(Path::new(&path)).expect("the pty's slave opened again");
    assert!(no_terminal(), "the open took the pty as its terminal");
    drop(opened);
}
