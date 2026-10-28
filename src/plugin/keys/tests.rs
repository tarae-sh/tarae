//! What a plugin hears of the keys you press (`keys.rs`), through the demo plugin's build patched to ask for `keys`
//! (`abi::demo_asking`): the three gates, commands only on every surface, the offline rule, a busy plugin's keys in one
//! event.

use super::*;
use crate::event::Event;
use crate::offer::What;
use crate::scratch::Scratch;
use crate::uistate::{Handler, TOOK};

#[track_caller]
fn settle(ed: &mut Editor, done: impl Fn(&Editor) -> bool) {
    crate::test_results::settle_saying(ed, done, |ed| format!("{:?}", said(ed)));
}

/// Everything said since tarae started (`:messages`), oldest first.
fn said(ed: &Editor) -> Vec<String> {
    ed.messages.iter().map(|m| m.2.clone()).collect()
}

/// An editor with the demo built to ask for `set` (`abi::demo_asking`), loaded from a folder of its own; its card
/// answered `yes` when it asks — the card's text returned.
fn editor(name: &str, set: u8, yes: bool) -> (Editor, Scratch, String) {
    editor_with(name, set, yes, crate::config::Config::default())
}

fn editor_with(name: &str, set: u8, yes: bool, config: crate::config::Config) -> (Editor, Scratch, String) {
    let dir = Scratch::new(&format!("keys-{name}"));
    std::fs::write(dir.join("demo.wasm"), abi::demo_asking(set)).unwrap();
    let mut ed = Editor::new(config);
    ed.docs[0].text = ropey::Rope::from_str("one two\nthree four\n");
    ed.plugins_sync_in(&dir.join("state"), crate::plugin::scan(&dir));
    let loaded = |ed: &Editor| {
        ed.plugin_command("echo").is_some() || said(ed).iter().any(|s| s.starts_with("plugin demo:"))
    };
    settle(&mut ed, loaded);
    let card = ed
        .offers
        .first()
        .filter(|o| matches!(&o.what, What::PluginPermissions { plugin, .. } if plugin == "demo"));
    let text = card.map(|o| o.question.clone()).unwrap_or_default();
    if card.is_some() {
        if yes { ed.offer_accept() } else { ed.offer_dismiss() }
    }
    (ed, dir, text)
}

/// A key as the terminal sends it — through the event loop, where your keys come in.
fn press(ed: &mut Editor, k: &str) {
    ed.handle_event(Event::Key(k.parse().unwrap()));
}

/// Keys sent to the demo and not answered yet.
fn sent(ed: &Editor) -> bool {
    ed.plugins.as_ref().and_then(|p| p.outbox.get("demo")).is_some_and(|o| o.busy || !o.queued.is_empty())
}

/// `keys` pressed one by one, each answered before the next.
fn presses(ed: &mut Editor, keys: &[&str]) {
    for k in keys {
        press(ed, k);
        settle(ed, |ed| !sent(ed));
    }
}

/// What the demo showed in its overlay — where a plugin that hears keys says what it got (`refused`) — oldest first.
fn shown() -> Vec<String> {
    super::super::overlay::SHOWN
        .with(|s| s.borrow().iter().filter(|(p, _)| p == "demo").map(|s| s.1.clone()).collect())
}

/// What the demo heard, an entry a key event (`[space f] done normal`), and the calls they came in.
fn heard() -> (Vec<String>, usize) {
    let calls: Vec<String> = shown()
        .iter()
        .filter_map(|s| s.strip_prefix("keys "))
        .map(|s| s.split(" · ").next().unwrap_or_default().to_string())
        .collect();
    (calls.iter().flat_map(|c| c.split(", ").map(str::to_string)).collect(), calls.len())
}

/// The status line, drawn 120 × 30.
fn status_line(ed: &mut Editor) -> String {
    status_at(ed, 120)
}

/// The status line, drawn `w` wide.
fn status_at(ed: &mut Editor, w: u16) -> String {
    crate::term::render(ed, &mut Vec::new(), w, 30).unwrap();
    let (f, w, h) = ed.last_frame.as_ref().unwrap();
    crate::screen::text(f, *w, *h).unwrap_or_default().lines().last().unwrap_or_default().to_string()
}

/// Heard only through all three gates: your own `plugins.keys` (off by default, a project's file can't turn it on),
/// the plugin's `keys` approved on its card (which says what it is), and the status line naming who hears.
#[test]
fn a_plugin_hears_keys_only_through_all_three_gates() {
    let (mut ed, _dir, card) = editor("gates", b'k', true);
    assert!(card.contains("hear the keys you press as commands, never a character typed"), "{card}");
    // Your setting off, as it comes: nothing goes, nothing's said
    assert!(!ed.config.plugins_keys, "off by default");
    assert!(crate::trust::yours_only("plugins.keys"), "only your own config turns it on");
    press(&mut ed, "j");
    assert!(!sent(&ed), "the setting off: nothing to the plugin");
    assert!(!status_line(&mut ed).contains("keys →"), "{}", status_line(&mut ed));
    // On: heard, and the status line says who hears
    ed.config.plugins_keys = true;
    presses(&mut ed, &["j"]);
    assert_eq!(heard().0, ["[j] done normal"]);
    assert!(status_line(&mut ed).contains("keys → demo"), "{}", status_line(&mut ed));
    // Its permission taken back: nothing, and nothing said
    ed.plugin_revoke("demo").unwrap();
    press(&mut ed, "k");
    assert!(!sent(&ed), "not approved: nothing to the plugin");
    assert!(!status_line(&mut ed).contains("keys →"), "{}", status_line(&mut ed));
    // … nor while its card was answered not now
    let (mut ed, _dir, _) = editor("gates-not-now", b'k', false);
    ed.config.plugins_keys = true;
    press(&mut ed, "j");
    assert!(!sent(&ed) && ed.keys_heard_by().is_empty());
}

/// The sign is never dropped for room: on a narrow status line beside a plugin's badge, the inbox and keys under way,
/// only the mode and the keys under way outlast it; a prompt's keys, cut to fit, leave it its room.
#[test]
fn a_narrow_status_line_still_says_who_hears() {
    let (mut ed, _dir, _) = editor("narrow", b'k', true);
    ed.config.plugins_keys = true;
    ed.plugins.as_mut().unwrap().badge = Some(("demo".into(), "prod-eu-west".into(), abi::Tone::Warning));
    let what = What::Asks { call: crate::tools::Call::test(1) };
    ed.push_offer(crate::offer::Offer::new(
        "Claude asks".into(),
        "to see the screen",
        "",
        String::new(),
        what,
    ));
    presses(&mut ed, &["g"]);
    let wide = status_at(&mut ed, 120);
    for there in ["keys → demo", "prod-eu-west", "waiting", "g"] {
        assert!(wide.contains(there), "{there}: {wide}");
    }
    // (32: the mode, the sign and the keys under way — the badge and the inbox folded; 60: the inbox back)
    for w in [32, 40, 60] {
        let line = status_at(&mut ed, w);
        assert!(line.contains("keys → demo") && line.contains("normal"), "{w}: {line}");
    }
    // A line being typed: its keys cut to the room the sign leaves
    presses(&mut ed, &["esc", ":"]);
    assert!(ed.prompt.is_some());
    assert!(status_at(&mut ed, 120).contains("esc close"), "{}", status_at(&mut ed, 120));
    for w in [32, 40, 60] {
        let line = status_at(&mut ed, w);
        assert!(line.contains("keys → demo"), "{w}: {line}");
    }
    // An error waiting there under a picker (no card could show it): cut to the room the sign leaves too
    presses(&mut ed, &["esc", "space", "b"]);
    assert!(ed.picker.is_some());
    ed.set_error("the language server stopped: ".repeat(4));
    assert!(status_at(&mut ed, 120).contains("the language server stopped"), "{}", status_at(&mut ed, 120));
    for w in [40, 60, 80] {
        let line = status_at(&mut ed, w);
        assert!(line.contains("keys → demo"), "{w}: {line}");
        assert!(w < 80 || line.contains("the language"), "{w}: {line}");
    }
}

/// An error said under a picker, its text cut away on a narrow status line by what outranks it (the sign, a waiting
/// card's count, the mode), isn't counted as shown: it waits (`Toast::waiting`, its time stopped, no key lets it go)
/// and comes up on its card once the picker closes — never only on the status line, let alone only as its bar.
#[test]
fn an_error_cut_under_a_picker_comes_back_on_its_card() {
    let (mut ed, _dir, _) = editor("cut-error", b'k', true);
    ed.config.plugins_keys = true;
    let what = What::Asks { call: crate::tools::Call::test(1) };
    ed.push_offer(crate::offer::Offer::new(
        "Claude asks".into(),
        "to see the screen",
        "",
        String::new(),
        what,
    ));
    presses(&mut ed, &["space", "b"]);
    assert!(ed.picker.is_some());
    let text = "the language server stopped: ".repeat(4);
    ed.set_error(text.clone());
    let line = status_at(&mut ed, 40);
    assert!(line.contains("keys → demo") && !line.contains("stopped: the language"), "cut: {line}");
    let toast = |ed: &Editor| ed.toasts.iter().find(|t| t.text == text).map(|t| (t.waiting(), t.shown));
    assert_eq!(toast(&ed), Some((true, false)), "the status line isn't its card");
    // (a key goes by under the picker: it still waits — its time hasn't started, so the key doesn't start its fade)
    let life = |ed: &Editor| ed.toasts.iter().find(|t| t.text == text).map(|t| t.life);
    let was = life(&ed);
    presses(&mut ed, &["j"]);
    status_at(&mut ed, 40);
    assert_eq!(toast(&ed), Some((true, false)), "still waiting");
    assert_eq!(life(&ed), was, "a key under the picker lets it go");
    presses(&mut ed, &["esc"]);
    assert!(ed.picker.is_none());
    status_at(&mut ed, 40);
    assert!(ed.screen.toast_card.get().is_some(), "its card, the picker gone");
    let (f, w, h) = ed.last_frame.as_ref().unwrap();
    let screen = crate::screen::text(f, *w, *h).unwrap_or_default();
    assert!(screen.contains("the language server"), "its text on the card:\n{screen}");
    assert_eq!(toast(&ed), Some((false, true)), "shown once its card was");
}

/// Commands only, in the code and the lines you type at: a count and a sequence as their keys, insert mode as its
/// entering key and the `esc` that left it, `f`/`r` without the character they took, a `:` line and a picker as
/// their opening keys and `esc`/`enter` (the shown name, #202) — nothing typed between (the letters typed are upper-case, in no key's name).
#[test]
fn a_plugin_hears_the_commands_never_what_is_typed() {
    let (mut ed, _dir, _) = editor("commands", b'k', true);
    ed.config.plugins_keys = true;
    let typed = ["Q", "Z", "X"];
    let with = |keys: &[&'static str]| [keys, &typed].concat();
    presses(&mut ed, &["3", "j", "g", "g", "v", "esc"]);
    presses(&mut ed, &with(&["i"]));
    presses(&mut ed, &["esc", "f", "W", "r", "K"]);
    presses(&mut ed, &with(&[":"]));
    presses(&mut ed, &["esc", ":", "Q", "ret"]);
    presses(&mut ed, &with(&["space", "b"]));
    presses(&mut ed, &["esc"]);
    let (heard, _) = heard();
    assert_eq!(
        heard,
        [
            "[3] more normal",
            "[3 j] done normal",
            "[g] more normal",
            "[g g] done normal",
            "[v] done select",
            "[esc] done normal",
            "[i] done insert",
            "[esc] done normal",
            "[f] done normal",
            "[r] done normal",
            "[:] done normal",
            "[esc] done normal",
            "[:] done normal",
            "[enter] done normal",
            "[space] more normal",
            "[space b] done normal",
            "[esc] done normal",
        ]
    );
    let all = heard.join(" ");
    assert!(!["Q", "Z", "X", "W", "K"].iter().any(|c| all.contains(c)), "{all}");
}

/// Only your keys: what tarae feeds itself — `.` replaying an insert, its `esc` too — is never heard; `.` is.
#[test]
fn only_your_keys_are_heard_never_what_tarae_replays() {
    let (mut ed, _dir, _) = editor("replay", b'k', true);
    ed.config.plugins_keys = true;
    presses(&mut ed, &["i", "Q", "esc", "."]);
    assert_eq!(heard().0, ["[i] done insert", "[esc] done normal", "[.] done normal"]);
    assert_eq!(ed.doc().text.to_string().matches('Q').count(), 2, "`.` typed it again");
}

/// Insert mode left by a binding of characters (`j k`, yours): the mode change heard, no key named.
#[test]
fn insert_left_by_letters_names_no_key() {
    let (config, w) = crate::config::parse("[keys.insert]\nj = { k = \"normal_mode\" }\n");
    assert!(w.is_empty(), "{w:?}");
    let (mut ed, _dir, _) = editor_with("jk", b'k', true, config);
    ed.config.plugins_keys = true;
    presses(&mut ed, &["i", "j", "k"]);
    assert_eq!(heard().0, ["[i] done insert", "[] done normal"]);
}

/// A plugin that hears keys asks for nothing else: one asking for `keys` beside the network, a program, a read pattern,
/// a variable or the project doesn't load — said, none of its commands — nor is it approved (a kept approval, its card,
/// `:plugin-approve`). A path it picks would be a lookup the filesystem makes — a server's on a network mount. (What
/// needs no permission is refused as it's answered: `a_plugin_that_hears_keys_shows_them_in_its_overlay_only`.)
#[test]
fn a_plugin_that_hears_keys_asks_for_nothing_else() {
    let sets = [
        (b'n', "network"),
        (b'r', "run true"),
        (b'f', "read /tarae-demo/**"),
        (b'e', "env TARAE_DEMO"),
        (b'p', "project"),
    ];
    for (set, what) in sets {
        let (ed, _dir, card) = editor(&format!("alone-{}", set as char), set, true);
        assert!(card.is_empty() && ed.plugin_command("echo").is_none(), "{what}: refused at load");
        let why = format!(
            "it asks to hear keys and for “{what}” — a plugin that hears keys asks for nothing else: it reads, \
             connects to and runs nothing"
        );
        assert!(
            said(&ed).iter().any(|s| s.starts_with("plugin demo:") && s.contains(&why)),
            "{what}: {:?}",
            said(&ed)
        );
    }
    // (the rule as the plugin thread and `tarae plugin list` take it, beside the one that holds no keys)
    use abi::Permission as P;
    let invalid = super::super::perm::invalid;
    assert!(invalid(&[P::Keys]).is_none());
    assert!(invalid(&[P::Network, P::Run("true".into()), P::Project]).is_none());
    assert!(invalid(&[P::Run("true".into()), P::Keys]).is_some());
    assert!(invalid(&[P::Keys, P::Read("/net/host/**".into())]).is_some());
    // Approval refuses it too: a set the load would refuse, with an approval kept for it, its card answered yes and
    // `:plugin-approve` — never approved, never heard
    let (mut ed, _dir, _) = editor("alone-approval", b'k', true);
    ed.config.plugins_keys = true;
    assert_eq!(ed.keys_heard_by(), ["demo"]);
    let more = vec![P::Keys, P::Read("/Volumes/**".into())];
    let p = ed.plugins.as_mut().unwrap();
    p.permissions.insert("demo".into(), more.clone());
    p.approvals.entry("demo".into()).or_default().granted =
        more.iter().map(super::super::perm::key).collect();
    assert!(!ed.plugin_approved("demo") && ed.keys_heard_by().is_empty(), "a kept approval");
    let e = ed.plugin_approve("demo").unwrap_err();
    assert!(e.contains("can't be approved") && e.contains("asks for nothing else"), "{e}");
    ed.plugin_permissions_answer("demo".into(), more, true);
    assert!(!ed.plugin_approved("demo") && ed.keys_heard_by().is_empty(), "its card's yes");
    assert!(said(&ed).last().is_some_and(|s| s.contains("can't be approved")), "{:?}", said(&ed));
}

/// What it heard stays in tarae: a plugin that hears keys shows it in its overlay and nowhere else — an edit, a move, a
/// message, status text, a panel, a side tab, a badge, hints, a buffer, a pick, a prompt, a path opened, a read (a
/// file, a variable, a find), a program, a connection, a file of its own and an install are refused (a request it
/// waits on failed back to it), said once in tarae's words, no card; and its overlay is never at Claude's level.
#[test]
fn a_plugin_that_hears_keys_shows_them_in_its_overlay_only() {
    let (mut ed, dir, _) = editor("overlay-only", b'k', true);
    ed.config.plugins_keys = true;
    // (a path in its own scratch: were a refusal broken, nothing outside it is opened or read)
    let x = dir.join("x.txt").display().to_string();
    let (visit, fetch) = (format!("visit {x}"), format!("fetch {x}"));
    ed.docs[0].text = ropey::Rope::from_str("one two  \nthree four\n");
    let (sel, docs, before) = (ed.doc().selection().clone(), ed.docs.len(), said(&ed).len());
    for cmd in [
        "shout",
        "trim-trailing",
        "grab one",
        "send move_line_down",
        "notify hi",
        &visit,
        "view",
        "side",
        "hint",
        "choose",
        "question",
        "install rg",
        "keep",
        "exec",
        "dial 1",
        "requests",
        "table",
        &fetch,
        "getenv TARAE_DEMO",
        "find **/*.rs",
    ] {
        let (name, args) = cmd.split_once(' ').unwrap_or((cmd, ""));
        ed.plugin_run(name, args).unwrap();
    }
    ed.plugin_run("overlay", "text over").unwrap();
    settle(&mut ed, |_| shown().iter().any(|s| s == "over"));
    settle(&mut ed, |ed| !sent(ed));
    assert_eq!(ed.doc().text.to_string(), "one two  \nthree four\n", "no edit");
    assert_eq!(ed.doc().selection(), &sel, "no move");
    assert_eq!(ed.docs.len(), docs, "no file opened, no buffer");
    let p = ed.plugins.as_ref().unwrap();
    assert!(p.panel.is_none() && p.side.is_none() && p.badge.is_none() && p.status.is_empty(), "no panel");
    assert!(p.views.is_empty() && p.hints.is_empty() && p.prompts.is_empty(), "no buffer, hints, prompt");
    assert!(ed.picker.is_none() && ed.offers.is_empty(), "no pick, no card (an install's, a read's)");
    let new = &said(&ed)[before..];
    assert_eq!(new.len(), 1, "said once, nothing of its own: {new:?}");
    assert!(
        new[0].starts_with("plugin demo hears keys: ") && new[0].ends_with("in its overlay only"),
        "{new:?}"
    );
    // (what it waits on, failed back to it — the demo says so in its overlay)
    let back = shown().join("\n");
    for why in [
        "a prompt: a plugin that hears keys shows what it hears in its overlay only",
        "a file of its own: a plugin that hears keys",
        "an install: a plugin that hears keys",
    ] {
        assert!(back.contains(why), "{why}:\n{back}");
    }
    // (by its id: the pick's, the file's, the variable's, the find's)
    for (id, what) in [(41, "a pick"), (10, "a read"), (11, "a read"), (50, "a read")] {
        let why =
            format!("failed {id} {what}: a plugin that hears keys shows what it hears in its overlay only");
        assert!(back.contains(&why), "{why}:\n{back}");
    }
    // Its overlay is the person's: never at Claude's level
    assert!(!ed.ui_state().overlays.is_empty());
    assert!(ed.ui_state_claude().overlays.is_empty(), "Claude's level holds no overlay");
}

/// What comes while its call is in flight goes in the next one, as one `keys` event — the newest `QUEUED_MAX` of a
/// burst; nothing waits for the plugin.
#[test]
fn keys_that_come_while_it_is_busy_go_as_one_event() {
    let (mut ed, _dir, _) = editor("busy", b'k', true);
    ed.config.plugins_keys = true;
    for k in ["j", "k", "k", "l"] {
        press(&mut ed, k);
    }
    settle(&mut ed, |ed| !sent(ed));
    assert_eq!(
        heard(),
        (
            vec![
                "[j] done normal".into(),
                "[k] done normal".into(),
                "[k] done normal".into(),
                "[l] done normal".into()
            ],
            2
        )
    );
    // A burst past what one event carries: the newest kept
    press(&mut ed, "j");
    for _ in 0..QUEUED_MAX + 5 {
        press(&mut ed, "w");
    }
    press(&mut ed, "b");
    settle(&mut ed, |ed| !sent(ed));
    let calls: Vec<String> = shown().into_iter().filter(|s| s.starts_with("keys ")).collect();
    let last = calls.last().unwrap().split(" · ").next().unwrap().to_string();
    let entries: Vec<&str> = last["keys ".len()..].split(", ").collect();
    assert_eq!(entries.len(), QUEUED_MAX, "{last}");
    assert_eq!(entries.last(), Some(&"[b] done normal"), "the newest kept");
}

/// The keys a walk sends: letters and digits that type where text goes, the editor's own, a card's answers, `esc`.
/// (No `ret`: in the chat it would start a claude.)
const PROBES: &[&str] = &["x", "Q", "y", "a", "i", "j", "3", "space", ":", "C-w", "C-q", "esc"];

/// A plugin that hears keys, approved, with no thread behind it (what it's sent goes nowhere).
fn hearer(ed: &mut Editor) {
    if ed.plugins.is_none() {
        ed.plugins_detached();
    }
    ed.config.plugins_keys = true;
    let p = ed.plugins.as_mut().unwrap();
    p.permissions.insert("hearer".into(), vec![abi::Permission::Keys]);
    let granted = vec!["keys".to_string()];
    p.approvals.insert("hearer".into(), super::super::perm::Approval { granted, ..Default::default() });
}

/// No character typed reaches a plugin, on any surface: in every scene of the routing test, each probe judged by the
/// handler that took it (`TOOK` — `handle_key`'s own word, not the chain `keys.rs` reads): a line being typed (a
/// picker, a prompt, the chat's input) hears only `enter`/`esc`; the code in insert mode only the key that left it,
/// naming no character; the terminal's program only tarae's own rows; pointing's labels and the minimized chat's
/// first key nothing.
#[test]
fn no_typed_character_reaches_a_plugin_on_any_surface() {
    if crate::sandbox::no_pty_here("plugin::keys::tests::no_typed_character_reaches_a_plugin_on_any_surface")
    {
        return;
    }
    let mut off = Vec::new();
    let mut commands = 0;
    for (name, setup) in crate::uistate::tests::SCENES {
        for probe in PROBES {
            let mut ed = crate::uistate::tests::editor();
            setup(&mut ed);
            crate::uistate::tests::frame(&mut ed);
            hearer(&mut ed);
            ed.focus_sync();
            let k: Key = probe.parse().unwrap();
            let row = ed.workspace.program_has_keys() && ed.keymaps.terminal_takes(k).is_some();
            let insert = ed.mode == Mode::Insert;
            let answers = ed.keys_to().answers;
            let Some(b) = ed.keys_before(k) else {
                off.push(format!("{name} · {probe}: no plugin hears"));
                continue;
            };
            TOOK.set(None);
            ed.handle_key(k);
            let took = TOOK.get();
            let ev = ed.key_heard(k, &b);
            let named = ev.as_ref().map(|e| e.keys.join(" "));
            let fine = match (took, &ev) {
                (_, None) => true,
                (Some(Handler::Point | Handler::Unfold), Some(_)) => false,
                (Some(Handler::Program), Some(_)) => row,
                (Some(Handler::Picker | Handler::Prompt | Handler::Chat), Some(_)) => act(k),
                (Some(Handler::Card | Handler::EditReview), Some(_)) => {
                    k.plain_char().is_none_or(|c| answers.contains(&c))
                }
                (Some(Handler::Code), Some(e)) if insert => {
                    ed.mode != Mode::Insert && e.keys.iter().all(|n| n.chars().count() > 1)
                }
                (_, Some(_)) => {
                    commands += 1;
                    true
                }
            };
            if !fine {
                off.push(format!("{name} · {probe}: {took:?} took it, the plugin heard {named:?}"));
            }
        }
    }
    assert!(
        off.is_empty(),
        "{} of {}:\n{}",
        off.len(),
        crate::uistate::tests::SCENES.len() * PROBES.len(),
        off.join("\n")
    );
    assert!(commands > crate::uistate::tests::SCENES.len(), "the walk heard commands: {commands}");
}
