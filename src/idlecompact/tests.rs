use super::*;
use std::time::{Duration, Instant};

/// The last request's time in these facts.
const AT: u64 = 50_000_000;
/// 54 minutes after it.
const DUE: u64 = AT + 54 * 60_000;

/// The facts where every rule holds: the 1h cache, big enough, idle, at `DUE`.
fn holds() -> Facts {
    Facts {
        now_ms: DUE,
        chat: true,
        touched_ms: Some(AT),
        ttl: Some(Ttl::Hour1),
        tokens: Some(200_000),
        rule: Rule::default(),
        busy: None,
        pressing: None,
    }
}

/// `decide` at its edges: the size (199,999 · 200,000), the time (53:59 · 54:00, and the 60 s past it), the TTL,
/// the settings, what waits on you, a plan window near its limit.
#[test]
fn decide_at_its_edges() {
    use Decision::{Compact, LetGo, WakeAt};
    assert_eq!(due(AT, Ttl::Hour1), Some(DUE), "0.9 of the hour");
    assert_eq!(decide(&holds()), Compact);
    let with = |change: fn(&mut Facts)| {
        let mut f = holds();
        change(&mut f);
        decide(&f)
    };
    assert_eq!(with(|f| f.tokens = Some(199_999)), LetGo(Why::Small));
    assert_eq!(with(|f| f.tokens = None), LetGo(Why::Small), "not reported: not big");
    assert_eq!(with(|f| f.now_ms = AT + 53 * 60_000 + 59_000), WakeAt(DUE), "53:59");
    assert_eq!(with(|f| f.now_ms = DUE - 1), WakeAt(DUE));
    assert_eq!(with(|f| f.now_ms = AT), WakeAt(DUE), "armed as the request lands");
    assert_eq!(with(|f| f.now_ms = DUE + LATE_MS), Compact, "a minute late: still");
    assert_eq!(with(|f| f.now_ms = DUE + LATE_MS + 1), LetGo(Why::Late), "asleep past it");
    assert_eq!(with(|f| f.ttl = Some(Ttl::Min5)), LetGo(Why::NotOneHour), "the 5-minute TTL");
    assert_eq!(with(|f| f.ttl = None), LetGo(Why::NoCache));
    assert_eq!(with(|f| f.chat = false), LetGo(Why::NoCache), "no claude running to send it to");
    assert_eq!(
        with(|f| f.rule.off = Some("idleCompaction")),
        LetGo(Why::Off("idleCompaction")),
        "`idleCompaction: false`"
    );
    assert_eq!(with(|f| f.busy = Some(Busy::Card)), LetGo(Why::Busy(Busy::Card)), "a card waiting");
    assert_eq!(with(|f| f.busy = Some(Busy::Turn)), LetGo(Why::Busy(Busy::Turn)));
    assert_eq!(with(|f| f.pressing = Some("5-hour".into())), LetGo(Why::Pressing("5-hour".into())));
    // (the variable's minimum, its floor)
    assert_eq!(with(|f| f.rule.min_tokens = 200_001), LetGo(Why::Small));
    assert_eq!(with(|f| (f.rule.min_tokens, f.tokens) = (FLOOR, Some(FLOOR))), Compact);
    // (off is said before the size, the size before the time: a small one never wakes)
    assert_eq!(with(|f| (f.tokens, f.now_ms) = (Some(10), AT)), LetGo(Why::Small));
}

#[test]
fn the_variable_moves_the_minimum_never_below_its_floor() {
    assert_eq!(min_tokens(None), MIN_TOKENS);
    assert_eq!(min_tokens(Some("150000")), 150_000);
    assert_eq!(min_tokens(Some(" 300000 ")), 300_000);
    assert_eq!(min_tokens(Some("5000")), FLOOR, "the floor");
    assert_eq!(min_tokens(Some("-1")), FLOOR);
    assert_eq!(min_tokens(Some("1.5e5")), 150_000);
    assert_eq!(min_tokens(Some("lots")), MIN_TOKENS, "not a number");
    assert_eq!(min_tokens(Some("")), MIN_TOKENS, "held back to \"\": unset");
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

/// The settings as claude ranks them — a later layer's key wins — and the variables: the environment, each layer's
/// `env` block over it; what turns it off in claude's order.
#[test]
fn the_rule_is_read_as_claude_ranks_its_settings() {
    let none = |_: &str| None;
    assert_eq!(rule(&[], none), Rule::default());
    let user_off = obj(json!({ "idleCompaction": false }));
    let local_on = obj(json!({ "idleCompaction": true }));
    assert_eq!(rule(std::slice::from_ref(&user_off), none).off, Some("idleCompaction"));
    assert_eq!(rule(&[user_off.clone(), local_on.clone()], none).off, None, "the later one wins");
    assert_eq!(rule(&[local_on, user_off], none).off, Some("idleCompaction"));
    let auto_off = obj(json!({ "autoCompactEnabled": false }));
    assert_eq!(rule(&[auto_off], none).off, Some("autoCompactEnabled"), "compaction off: idle too");
    assert_eq!(rule(&[obj(json!({ "idleCompaction": "false" }))], none).off, None, "not a bool");
    // The variables: the environment's, a settings file's `env` over it
    let env = |n: &str| match n {
        MIN_VAR => Some("300000".to_string()),
        "DISABLE_AUTO_COMPACT" => Some("0".to_string()),
        _ => None,
    };
    assert_eq!(rule(&[], env), Rule { off: None, min_tokens: 300_000 });
    let set = obj(json!({ "env": { MIN_VAR: "120000", "DISABLE_COMPACT": "true" } }));
    assert_eq!(rule(&[set], env), Rule { off: Some("DISABLE_COMPACT"), min_tokens: 120_000 });
    // (a held-back variable reads as unset: tarae's `--settings` lays "" over it)
    let held = obj(json!({ "env": { MIN_VAR: "" } }));
    assert_eq!(rule(&[held], env).min_tokens, MIN_TOKENS);
}

/// Only the scopes the chat's claude loads (`--setting-sources`) are read, then its `--settings`.
#[test]
fn only_the_settings_the_chats_claude_loads_are_read() {
    let dir = crate::scratch::Scratch::new("idlecompact-places");
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    let user = root.join("user-settings.json");
    std::fs::write(&user, r#"{"autoCompactEnabled": false}"#).unwrap();
    std::fs::write(root.join(".claude/settings.json"), r#"{"idleCompaction": false}"#).unwrap();
    std::fs::write(root.join(".claude/settings.local.json"), r#"{"env": {"DISABLE_COMPACT": "1"}}"#).unwrap();
    let places = |sources: Option<&str>, settings: Option<&str>| Places {
        sources: sources.map(String::from),
        settings: settings.map(String::from),
        root: root.clone(),
        user: Some(user.clone()),
        managed: Vec::new(),
        env: Vec::new(),
    };
    assert_eq!(places(Some(""), None).rule().off, None, "`--setting-sources \"\"`: none");
    assert_eq!(places(Some("project"), None).rule().off, Some("idleCompaction"));
    assert_eq!(places(Some("user"), None).rule().off, Some("autoCompactEnabled"));
    assert_eq!(places(None, None).rule().off, Some("DISABLE_COMPACT"), "not given: all three");
    let over = r#"{"idleCompaction": true, "env": {"DISABLE_COMPACT": ""}}"#;
    assert_eq!(places(Some("project,local"), Some(over)).rule().off, None, "`--settings` over them");
    // (a file that isn't a settings object is no layer)
    std::fs::write(root.join(".claude/settings.json"), "[1, 2]").unwrap();
    assert_eq!(places(Some("project"), None).rule().off, None);
}

// ── With a stand-in claude ───────────────────────────────────────────────────────────────────────────

/// A stand-in claude: every line it's sent goes to `stdin`. A message is a turn that wrote 250k tokens to the 1h
/// cache; `/compact`, sent as the whole message, compacts it to 9k (`compact_boundary`, as claude writes it —
/// `compaction.rs`) in a turn of 0 tokens; anything else that names `/compact` is a plain turn.
const STAND_IN: &str = r#"while read -r line; do
  printf '%s\n' "$line" >> stdin
  case "$line" in
    *tarae-init*)
      printf '{"type":"control_response","response":{"subtype":"success","request_id":"tarae-init","response":{"models":[],"account":{"apiKeySource":"none"}}}}\n';;
    *'"content":"/compact",'*)
      printf '{"type":"system","subtype":"status","status":"compacting","session_id":"conv-1"}\n'
      printf '{"type":"system","subtype":"compact_boundary","session_id":"conv-1","compact_metadata":{"trigger":"manual","pre_tokens":250002,"post_tokens":9000}}\n'
      printf '{"type":"system","subtype":"status","status":null,"compact_result":"success","session_id":"conv-1"}\n'
      printf '{"type":"result","subtype":"success","is_error":false,"result":"","session_id":"conv-1","total_cost_usd":1.05,"usage":{"input_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":0}}\n';;
    *'"type":"user"'*)
      printf '{"type":"system","subtype":"init","session_id":"conv-1","apiKeySource":"none","model":"m"}\n'
      printf '{"type":"system","subtype":"status","status":"requesting"}\n'
      printf '{"type":"stream_event","event":{"type":"message_start","message":{"id":"c1","model":"m","usage":{"input_tokens":2,"cache_read_input_tokens":0,"cache_creation_input_tokens":250000,"cache_creation":{"ephemeral_1h_input_tokens":250000},"output_tokens":1}}}}\n'
      printf '{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"conv-1","total_cost_usd":1.0}\n';;
  esac
done"#;

fn until(ed: &mut Editor, what: &str, done: impl Fn(&Editor) -> bool) {
    let end = Instant::now() + crate::test_results::PATIENCE;
    while !done(ed) {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        if let Some(ev) = ed.events.recv_timeout(Duration::from_millis(20)) {
            ed.handle_event(ev);
        }
    }
}

/// A chat that had one turn — 250k tokens on the 1h cache — its request `ago_ms` back.
fn idle(root: &Path, ago_ms: u64) -> Editor {
    let mut ed = Editor::with_root(crate::config::Config::default(), root.to_path_buf());
    ed.config.llm.command = "sh".into();
    ed.config.llm.args = vec!["-c".into(), STAND_IN.into(), "stand-in".into()];
    ed.config.llm.chat_settings = false;
    crate::chat::open(&mut ed);
    ed.workspace.claude.chat.as_mut().unwrap().input = "hello".into();
    crate::chat::send(&mut ed);
    until(&mut ed, "the turn", |ed| {
        ed.workspace.claude.chat.as_ref().is_some_and(|c| !c.busy())
            && ed.workspace.claude.usage.conv.as_ref().is_some_and(|c| c.ttl.is_some())
    });
    // (the rule read after it, landed)
    until(&mut ed, "the rule read", |ed| !ed.workspace.claude.idle.reading);
    let at = crate::usage::now_ms() - ago_ms;
    ed.workspace.claude.usage.conv.as_mut().unwrap().touched_ms = Some(at);
    ed
}

/// The chat's last row, as text, at `w` cells.
fn row_text(ed: &Editor, w: usize) -> String {
    crate::term::usage_row_for_test(ed, w).into_iter().map(|s| s.text).collect()
}

fn lines(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join("stdin")).unwrap_or_default();
    text.lines().map(String::from).collect()
}

/// Idle 54 minutes on the 1h cache, 250k tokens: the row said `compacts` when; tarae sends `/compact`, bare — the
/// whole message, no editor context — as a turn of its own with a note of why; claude compacts, said `idle`, and
/// the row's `compacts` goes with the context it dropped.
#[test]
fn idle_54_minutes_sends_compact_as_a_turn() {
    let dir = crate::scratch::Scratch::new("idlecompact-send");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut ed = idle(&root, 30 * 60_000);
    let touched = ed.workspace.claude.usage.conv.as_ref().unwrap().touched_ms.unwrap();
    let due = touched + 54 * 60_000;
    // Ahead: said on the row and in the state, nothing sent
    let row = row_text(&ed, 140);
    assert!(row.contains(&format!("compacts {}", crate::usage::until(due))), "{row}");
    assert!(row.contains("warm until"), "{row}");
    let state = crate::uistate::UsageState::of_session(&ed, &ed.workspace, 0).unwrap().json();
    assert_eq!(state["idle_compact"]["at_ms"], due, "{state}");
    assert_eq!(state["idle_compact"]["min_tokens"], MIN_TOKENS);
    woke(&mut ed);
    assert!(!ed.workspace.claude.idle.reading, "not yet: nothing read for it");
    assert!(!lines(&root).iter().any(|l| l.contains("/compact")), "not yet: {:?}", lines(&root));
    // Due: `/compact`
    ed.workspace.claude.usage.conv.as_mut().unwrap().touched_ms = Some(crate::usage::now_ms() - 54 * 60_000);
    woke(&mut ed);
    until(&mut ed, "/compact sent", |ed| ed.workspace.claude.idle.sent.is_some());
    until(&mut ed, "/compact got", |_| lines(&root).iter().any(|l| l.contains(COMMAND)));
    let sent = lines(&root);
    assert_eq!(sent.last(), Some(&crate::llm::user_message(COMMAND)), "the whole message: {sent:?}");
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    assert!(c.busy(), "a turn of its own");
    let note = &c.msgs.last().unwrap();
    assert_eq!(note.role, crate::chat::Role::Note);
    assert!(note.text.starts_with("Idle 54m "), "{}", note.text);
    assert!(note.text.contains(" — compacting before Claude's cache expires at "), "{}", note.text);
    assert!(!row_text(&ed, 140).contains("compacts"), "under way: not said again");
    until(&mut ed, "the compaction's turn", |ed| !ed.workspace.claude.chat.as_ref().unwrap().busy());
    let c = ed.workspace.claude.chat.as_ref().unwrap();
    let said = c.msgs.iter().find(|m| crate::compaction::is_note(&m.text));
    let all: Vec<&str> = c.msgs.iter().map(|m| m.text.as_str()).collect();
    let said = said.unwrap_or_else(|| panic!("the compaction said: {all:?} · {:?}", lines(&root)));
    assert!(said.text.contains("(idle, 250k tokens before)"), "{}", said.text);
    assert_eq!(ed.workspace.claude.compacted.count, 1);
    assert_eq!(ed.workspace.claude.idle.sent, None, "its turn's");
    assert_eq!(ed.workspace.claude.usage.context().map(|c| c.0), Some(9000));
    assert!(!row_text(&ed, 140).contains("compacts"), "9k: no more");
    // Once: woken again, nothing goes
    woke(&mut ed);
    assert_eq!(lines(&root).iter().filter(|l| l.contains("/compact")).count(), 1);
}

/// Where it doesn't: a card waiting on you, a message queued, `idleCompaction: false` as read — the row says
/// nothing of it, and nothing goes.
#[test]
fn what_waits_on_you_or_turns_it_off_sends_nothing() {
    let dir = crate::scratch::Scratch::new("idlecompact-held");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut ed = idle(&root, 54 * 60_000);
    let what = crate::offer::What::ReviewEnd {
        session: crate::event::WorkspaceId::HOME,
        started: crate::clock::now(),
    };
    ed.push_offer(crate::offer::Offer::new(
        "End review?".into(),
        "2 files not viewed.",
        "",
        String::new(),
        what,
    ));
    assert!(!row_text(&ed, 140).contains("compacts"));
    woke(&mut ed);
    ed.offers.clear();
    assert!(row_text(&ed, 140).contains("compacts"), "the card gone: due");
    ed.workspace.claude.idle.rule.off = Some("idleCompaction");
    assert!(!row_text(&ed, 140).contains("compacts"));
    let state = crate::uistate::UsageState::of_session(&ed, &ed.workspace, 0).unwrap().json();
    assert_eq!(state["idle_compact"]["off"], "idleCompaction");
    assert_eq!(state["idle_compact"]["at_ms"], Value::Null);
    woke(&mut ed);
    assert!(!ed.workspace.claude.idle.reading, "let go: nothing read for it");
    assert!(!lines(&root).iter().any(|l| l.contains("/compact")), "{:?}", lines(&root));
}

/// A card of the session's own, as it waits behind: a session behind swapped in (`with_workspace`) goes by its own
/// `parked` cards, never the front's `offers` — its own card holds it, the front's alone doesn't.
#[test]
fn a_session_behind_goes_by_its_own_cards() {
    let dir = crate::scratch::Scratch::new("idlecompact-behind");
    let other = crate::scratch::Scratch::new("idlecompact-behind-front");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut ed = idle(&root, 54 * 60_000);
    let a = ed.workspace.id;
    let b = ed.workspace_open(other.path()).unwrap();
    assert_eq!(ed.workspace.id, b, "another session in front");
    let card = |session| {
        let what = crate::offer::What::ReviewEnd { session, started: crate::clock::now() };
        crate::offer::Offer::new("End review?".into(), "2 files not viewed.", "", String::new(), what)
    };
    fn idle_of(ed: &Editor, id: crate::event::WorkspaceId) -> &Idle {
        &ed.workspaces.iter().find(|s| s.id == id).unwrap().claude.idle
    }
    let compacts = |root: &Path| lines(root).iter().filter(|l| l.contains(COMMAND)).count();
    // Its own card parked: held
    ed.workspaces.iter_mut().find(|s| s.id == a).unwrap().parked.push(card(a));
    ed.with_workspace(a, |ed| {
        woke(ed);
    });
    assert_eq!(idle_of(&ed, a).held, Some(Why::Busy(Busy::Card)), "its own card holds it");
    assert!(!idle_of(&ed, a).reading && compacts(&root) == 0);
    // Only the front's card up: it goes
    ed.workspaces.iter_mut().find(|s| s.id == a).unwrap().parked.clear();
    ed.push_offer(card(b));
    ed.with_workspace(a, |ed| {
        woke(ed);
    });
    until(&mut ed, "/compact got", |_| compacts(&root) == 1);
    assert!(idle_of(&ed, a).sent.is_some(), "sent in its own session");
    assert!(ed.workspace.claude.idle.sent.is_none() && !ed.offers.is_empty(), "the front's untouched");
}

/// No environment given is an empty one in a test build — never the test process's own (`llm::env_var`); a given
/// one is the whole of what the rule reads.
#[test]
fn the_rule_reads_only_the_environment_given() {
    assert_eq!(crate::llm::env_var(None, "PATH"), None, "the real environment, read in a test");
    let mut ed = Editor::new(crate::config::Config::default());
    ed.config.llm.command = "sh".into();
    ed.config.llm.args = vec!["-c".into(), "cat > /dev/null".into()];
    crate::chat::open(&mut ed);
    ed.claude_env = None;
    let places = Places::of(&ed).expect("a chat");
    assert!(places.env.is_empty(), "{:?}", places.env);
    assert_eq!(places.rule(), Rule::default());
    let given = [("DISABLE_COMPACT", "1"), (MIN_VAR, "150000")];
    ed.claude_env = Some(given.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
    let places = Places::of(&ed).expect("a chat");
    assert_eq!(places.rule(), Rule { off: Some("DISABLE_COMPACT"), min_tokens: 150_000 });
}

/// Let go once woken, where the row said `compacts` until then — the rule read off as it fired, or a late wake —
/// the event draws: the row loses `compacts`. One the row never said draws nothing.
#[test]
fn a_let_go_at_fire_time_takes_compacts_off_the_row() {
    let dir = crate::scratch::Scratch::new("idlecompact-letgo");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut ed = idle(&root, 54 * 60_000 + 1000);
    assert!(row_text(&ed, 140).contains("compacts"), "{}", row_text(&ed, 140));
    // Read off as it fires
    ed.claude_env = Some([("DISABLE_COMPACT".to_string(), "1".to_string())].into());
    assert!(woke(&mut ed), "the rule read again first");
    let end = Instant::now() + crate::test_results::PATIENCE;
    let unchanged = loop {
        assert!(Instant::now() < end, "timed out waiting for the read");
        let Some(ev) = ed.events.recv_timeout(Duration::from_millis(20)) else { continue };
        let unchanged = crate::term::handle_noting(&mut ed, ev);
        if !ed.workspace.claude.idle.reading {
            break unchanged;
        }
    };
    assert_eq!(ed.workspace.claude.idle.held, Some(Why::Off("DISABLE_COMPACT")));
    assert!(!unchanged, "the row said compacts: drawn");
    assert!(!row_text(&ed, 140).contains("compacts"), "{}", row_text(&ed, 140));
    // A late wake (the machine slept), as the usage watch's wake leaves it: drawn too
    ed.claude_env = Some(Default::default());
    ed.workspace.claude.idle.rule = Rule::default();
    let late = crate::usage::now_ms() - 54 * 60_000 - LATE_MS - 1000;
    ed.workspace.claude.usage.conv.as_mut().unwrap().touched_ms = Some(late);
    ed.unchanged = true;
    assert!(!woke(&mut ed));
    assert_eq!(ed.workspace.claude.idle.held, Some(Why::Late));
    assert!(!ed.unchanged, "late: drawn");
    // What the row never said: nothing to draw
    ed.workspace.claude.usage.conv.as_mut().unwrap().touched_ms = Some(crate::usage::now_ms() - 54 * 60_000);
    ed.push_offer(crate::offer::Offer::new(
        "End review?".into(),
        "2 files not viewed.",
        "",
        String::new(),
        crate::offer::What::ReviewEnd { session: ed.workspace.id, started: crate::clock::now() },
    ));
    ed.unchanged = true;
    assert!(!woke(&mut ed));
    assert_eq!(ed.workspace.claude.idle.held, Some(Why::Busy(Busy::Card)));
    assert!(ed.unchanged, "a card up: the row never said it");
}

/// The row: `compacts 14:26` beside `warm until`, folded last but the cache; a narrower row never shows a group a
/// wider one doesn't.
#[test]
fn the_row_folds_compacts_last_but_the_cache() {
    let dir = crate::scratch::Scratch::new("idlecompact-row");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let ed = idle(&root, 10 * 60_000);
    let groups = ["warm until", "compacts", "ctx", "$1.00"];
    for w in 18..160 {
        let (narrow, wide) = (row_text(&ed, w), row_text(&ed, w + 1));
        for g in groups.iter().filter(|g| narrow.contains(*g)) {
            assert!(wide.contains(g), "`{g}` at {w} but not at {}: {narrow:?} → {wide:?}", w + 1);
        }
    }
    let fold = (18..160).find(|&w| row_text(&ed, w).contains("compacts")).expect("said somewhere");
    let at = row_text(&ed, fold);
    assert!(
        at.starts_with("● warm until ") && !at.contains("ctx") && !at.contains('$'),
        "last but the cache: {at}"
    );
}
