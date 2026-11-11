//! The hands' tests: made-up states from a stand-in UI socket of the test's own (never tarae), a harmless child (`sh`)
//! for the way in end to end, scratch folders by `Scratch::new`. Nothing waits on a fixed time: a key's pause after a
//! screen that ended returns at once, and the end-to-end test waits on what its child prints.

use std::sync::mpsc;
use std::thread::JoinHandle;

use tarae_testkit::scratch::Scratch;

use super::*;
use crate::record::{Outer, Spec, TERMINAL, read_log, record};

/// A made-up state: the keys in `region` (taking `takes`), `prompt` up where given, the cards given.
fn state(region: &str, takes: &str, prompt: Option<&str>, cards: Value) -> Value {
    json!({
        "keys": { "to": region, "region": region, "takes": takes },
        "prompt": prompt.map(|p| json!({ "kind": p, "text": "" })),
        "front": "w0",
        "workspaces": [
            { "id": "w0", "name": "main", "sessions": [{ "id": "s0", "doing": "idle" }] },
            { "id": "w1", "name": "dates", "sessions": [{ "id": "s1", "doing": "working" }] }
        ],
        "cards": cards,
    })
}

fn run_card(up: &str) -> Value {
    json!([{ "id": "card#3", "kind": "run", "title": "Claude wants to run a command", "asks": "cargo test",
             "answers": [{ "key": "y", "does": "run it" }, { "key": "n", "does": "tell Claude no" }],
             "up": up, "focused": false, "waits": "focus" }])
}

/// A stand-in for tarae's UI socket at `sock`: each of `states` the answer to one connection's request, in order.
fn stand_in_ui(sock: &Path, states: Vec<Value>) -> JoinHandle<()> {
    let listener = tarae_sock::at(sock, |p: &Path| UnixListener::bind(p)).unwrap();
    std::thread::spawn(move || {
        for s in states {
            let (conn, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&conn).read_line(&mut line).unwrap();
            writeln!(&mut &conn, "{s}").unwrap();
        }
    })
}

/// The hands' context on a terminal that's a file of the test's (what's typed lands there), the screen already ended
/// (a key's pause returns at once), the stand-in UI at `ui`.
fn ctx(dir: &Scratch, ui: &Path) -> (Ctx, mpsc::Receiver<Log>, PathBuf) {
    let typed = dir.join("typed");
    let (tx, rx) = mpsc::channel();
    let seen = Arc::new(Seen::default());
    seen.set(None, true);
    let ctx = Ctx {
        to: File::create(&typed).unwrap(),
        tx,
        seen,
        hub: Arc::new(Hub::default()),
        ui: Some(ui.to_path_buf()),
        started: Instant::now(),
        typing: Mutex::new(()),
    };
    (ctx, rx, typed)
}

#[test]
fn type_and_key_with_to_refuse_where_the_keys_are_elsewhere_and_send_nothing() {
    let dir = Scratch::new("xtask-hands-guard");
    let ui = dir.join("ui.sock");
    let code_ = state("code", "commands", None, json!([]));
    let chat = state("chat", "text", None, json!([]));
    let served = stand_in_ui(&ui, vec![code_.clone(), code_, chat]);
    let (ctx, rx, typed) = ctx(&dir, &ui);

    let (c, said) = act(&json!({ "do": "type", "to": "chat", "text": "run the tests", "send": true }), &ctx);
    assert_eq!(c, code::REFUSED, "{said}");
    assert!(said.starts_with("the keys are in code, not chat (keys → code (commands)"), "{said}");
    assert!(said.ends_with("— nothing typed"), "{said}");
    let (c, said) = act(&json!({ "do": "key", "to": "card", "keys": ["y"] }), &ctx);
    assert_eq!(c, code::REFUSED, "{said}");
    assert!(said.contains("the keys are in code, not card"), "{said}");
    assert_eq!(std::fs::read(&typed).unwrap(), b"", "nothing reached the terminal");
    assert!(rx.try_recv().is_err(), "nothing logged");

    // (where they are: sent, and logged as the hands' — the key script's)
    let (c, said) = act(&json!({ "do": "key", "to": "chat", "keys": ["C-q"] }), &ctx);
    assert_eq!((c, said.as_str()), (code::HELD, "sent C-q"));
    assert_eq!(std::fs::read(&typed).unwrap(), b"\x11");
    assert!(matches!(rx.try_recv(), Ok(Log::Key(_, SCRIPT, b)) if b == b"\x11"));
    // (words are never typed blind)
    let (c, said) = act(&json!({ "do": "type", "text": "hi" }), &ctx);
    assert_eq!(c, code::ERROR, "{said}");
    served.join().unwrap();
}

#[test]
fn where_the_keys_are_is_said_in_the_drivers_words() {
    let ask = state("prompt", "text", Some("change:"), json!([]));
    assert_eq!(place(&ask), "ask");
    assert!(at("ask", &ask) && at("prompt", &ask) && !at("chat", &ask));
    assert_eq!(place(&state("prompt", "text", Some(":"), json!([]))), "command");
    assert_eq!(place(&state("prompt", "text", Some("new session:"), json!([]))), "session-name");
    let side = state("side:git", "commands", None, json!([]));
    assert!(at("side", &side) && at("side:git", &side) && !at("si", &side));
    assert!(!at("chat", &state("view:chat", "commands", None, json!([]))), "the chat's view isn't its input");
    let line = front_line(&state("chat", "text", None, run_card("front")));
    assert_eq!(
        line,
        "keys → chat (text) · s0 main (front) idle · s1 dates working · card#3 run · Claude wants to run a command · \
         \"cargo test\" · y run it · n tell Claude no · waits — C-q focuses it"
    );
}

#[test]
fn keys_in_the_scenes_words_are_the_bytes_a_terminal_sends() {
    let b = |k: &str| key_bytes(k).unwrap();
    assert_eq!(
        (b("space"), b("<ret>"), b("esc"), b("<tab>")),
        (b" ".to_vec(), b"\r".to_vec(), vec![0x1b], b"\t".to_vec())
    );
    assert_eq!((b("C-q"), b("C-w"), b("C-space")), (vec![0x11], vec![0x17], vec![0]));
    assert_eq!(
        (b("A-x"), b("<up>"), b("W"), b("\\x1b")),
        (b"\x1bx".to_vec(), b"\x1b[A".to_vec(), b"W".to_vec(), vec![0x1b])
    );
    assert!(key_bytes("<nope>").is_err() && key_bytes("C-12").is_err() && key_bytes("").is_err());
}

#[test]
fn a_wait_returns_on_its_condition_and_gives_up_at_its_patience_saying_whats_in_front() {
    let hub = Hub::default();
    let since = Instant::now();
    // (an earlier turn's end of the same session, before the hands' last key: it doesn't count)
    let early = since.checked_sub(Duration::from_millis(5)).unwrap();
    hub.told(
        early,
        &json!({ "event": "turn", "what": "ended", "session": "s1", "said": "s1 dates: an old turn" }),
    );
    let mut asked = 0;
    let (c, said) = wait(
        &Wait::Turn(Some("dates".into())),
        crate::PATIENCE,
        &mut || {
            asked += 1;
            // (round 1: news that ends nothing, so the wait looks again; round 2: the turn's end, of `dates`)
            // (nothing after: a wait that missed it waits out its patience, never spins on news of its own making)
            match asked {
                1 => hub.told(Instant::now(), &json!({ "event": "keys" })),
                2 => hub.told(
                    Instant::now(),
                    &json!({ "event": "turn", "what": "ended", "session": "s1", "said": "s1 dates: turn ended" }),
                ),
                _ => {}
            }
            Ok(state("chat", "text", None, json!([])))
        },
        &hub,
        since,
    );
    assert_eq!(asked, 2, "looked again at the news");
    assert_eq!(c, code::HELD, "{said}");
    assert!(said.starts_with("s1 dates: turn ended — keys → chat (text)"), "{said}");

    // (a card in front first: said, and the wait ends — answered at once, never waited past)
    let carded = state("code", "commands", None, run_card("front"));
    let (c, said) = wait(&Wait::Run, crate::PATIENCE, &mut || Ok(carded.clone()), &hub, Instant::now());
    assert_eq!(c, code::CARD, "{said}");
    assert!(said.starts_with("a card first, waiting for a run's end: card#3 run ·"), "{said}");
    // (… which is what a wait for a card waits for; one parked behind isn't in front)
    let (c, said) =
        wait(&Wait::Card(Some("run".into())), crate::PATIENCE, &mut || Ok(carded.clone()), &hub, since);
    assert_eq!(c, code::HELD, "{said}");
    let parked = state("chat", "text", None, run_card("parked"));
    let (c, said) = wait(&Wait::Card(None), Duration::ZERO, &mut || Ok(parked.clone()), &hub, since);
    assert_eq!(c, code::GAVE_UP, "{said}");
    assert!(said.starts_with("gave up after "), "{said}");
    assert!(
        said.ends_with("waiting for a card: keys → chat (text) · s0 main (front) idle · s1 dates working"),
        "{said}"
    );

    // (a bring's end, and the keys somewhere)
    hub.told(
        Instant::now(),
        &json!({ "event": "bring", "how": "landed", "said": "s1 dates: brought into main" }),
    );
    let (c, said) = wait(&Wait::Bring, crate::PATIENCE, &mut || Ok(parked.clone()), &hub, since);
    assert_eq!(c, code::HELD, "{said}");
    assert!(said.starts_with("s1 dates: brought into main — "), "{said}");
    let (c, _) = wait(&Wait::Keys("chat".into()), crate::PATIENCE, &mut || Ok(parked.clone()), &hub, since);
    assert_eq!(c, code::HELD);
    // (the take ended: no wait outlives it)
    hub.end();
    let (c, said) = wait(&Wait::Run, crate::PATIENCE, &mut || Ok(parked.clone()), &hub, Instant::now());
    assert_eq!(c, code::GAVE_UP, "{said}");
    assert!(said.starts_with("the take ended waiting for a run's end"), "{said}");
}

/// News that keeps coming while the condition never holds (a live tarae's events): the wait still gives up at its
/// patience, having looked again at each piece.
#[test]
fn a_wait_gives_up_at_its_patience_while_news_keeps_coming() {
    let hub = Hub::default();
    let mut asked = 0;
    let patience = Duration::from_millis(50);
    let (c, said) = wait(
        &Wait::Turn(None),
        patience,
        &mut || {
            asked += 1;
            hub.told(Instant::now(), &json!({ "event": "keys" }));
            Ok(state("chat", "text", None, json!([])))
        },
        &hub,
        Instant::now(),
    );
    assert_eq!(c, code::GAVE_UP, "{said}");
    assert!(said.starts_with("gave up after "), "{said}");
    assert!(asked > 1, "looked again at the news: {asked}");
}

#[test]
fn a_command_line_is_its_request() {
    let args = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
    let (r, via) = request("type", &args("--to chat --send run the tests --via /tmp/x")).unwrap();
    assert_eq!(r, json!({ "do": "type", "to": "chat", "text": "run the tests", "send": true }));
    assert_eq!(via, Some(PathBuf::from("/tmp/x")));
    let (r, _) = request("wait", &args("turn --session dates --for 1.5")).unwrap();
    assert_eq!(r, json!({ "do": "wait", "what": "turn", "arg": "dates", "for_ms": 1500 }));
    let (r, _) = request("key", &args("--to card y")).unwrap();
    assert_eq!(r, json!({ "do": "key", "to": "card", "keys": ["y"] }));
    assert!(request("wait", &args("keys")).is_err(), "keys wants a where");
    assert!(request("state", &args("now")).is_err());
}

/// The way in, end to end: a take of a harmless child in `--cwd`, its way in a folder of this user's alone, a key and a
/// mark through it — the key reaching the child as the hands' own, the mark in the landmarks at its time — and the way
/// in gone once the take ends.
#[test]
fn the_way_in_is_its_users_alone_its_keys_and_marks_land_and_it_goes_with_the_take() {
    let dir = Scratch::new("xtask-hands-way");
    let (cwd, take) = (dir.join("here"), dir.join("take"));
    std::fs::create_dir(&cwd).unwrap();
    let (master, slave) = crate::pty::open(24, 80).unwrap();
    let shown = File::from(master);
    let outer = Outer { input: slave.try_clone().unwrap(), output: slave };
    let rx = {
        let mut from = shown.try_clone().unwrap();
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = std::io::Read::read(&mut from, &mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        rx
    };
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.args(["-c", r#"printf 'in:%s\n' "$(basename "$PWD")"; read -r x; printf 'got:%s;' "$x"; read -r y"#]);
    let spec = Spec {
        size: (80, 24),
        out: take.clone(),
        outer: Some(outer),
        keys: Vec::new(),
        quit_after_keys: false,
        ui: None,
        cwd: Some(cwd.clone()),
        hands: Some(dir.path().to_path_buf()),
    };
    let taking = std::thread::spawn(move || record(spec, cmd));
    let mut seen = String::new();
    let until = |seen: &mut String, text: &str| {
        while !seen.contains(text) {
            let chunk =
                rx.recv_timeout(crate::PATIENCE).unwrap_or_else(|e| panic!("{e} before {text:?}: {seen:?}"));
            seen.push_str(&String::from_utf8_lossy(&chunk));
        }
    };
    until(&mut seen, "in:here");

    let sock = way_in(Some(&take)).unwrap();
    let folder = sock.parent().unwrap().to_path_buf();
    let m = std::fs::symlink_metadata(&folder).unwrap();
    // SAFETY: getuid can't fail.
    assert_eq!((m.mode() & 0o777, m.uid()), (0o700, unsafe { libc::getuid() }), "{}", folder.display());

    let (c, said) = ask(&sock, &json!({ "do": "key", "keys": ["hi", "<ret>"] })).unwrap();
    assert_eq!((c, said.as_str()), (code::HELD, "sent hi <ret>"));
    until(&mut seen, "got:hi;");
    let (c, said) = ask(&sock, &json!({ "do": "mark", "text": "a friction" })).unwrap();
    assert_eq!(c, code::HELD, "{said}");
    // (without tarae's socket a wait still ends where it can, and says why it can't look)
    let (c, said) = ask(&sock, &json!({ "do": "state" })).unwrap();
    assert_eq!((c, said.as_str()), (code::ERROR, "tarae's state: no UI socket watched (record --state)"));
    std::io::Write::write_all(&mut &shown, b"bye\r").unwrap();
    taking.join().unwrap().unwrap();

    assert!(!folder.exists() && !take.join(WAY_IN).exists(), "the way in went with the take");
    let keys = read_log(&take.join("keys.bin")).unwrap();
    let by = |whose: u8| {
        keys.iter().filter(|r| r.whose == whose).flat_map(|r| r.bytes.clone()).collect::<Vec<u8>>()
    };
    assert_eq!(
        (by(SCRIPT), by(TERMINAL)),
        (b"hi\r".to_vec(), b"bye\r".to_vec()),
        "the hands' keys and the pane's, apart"
    );
    let ret_ms = keys.iter().find(|r| r.bytes == b"\r").unwrap().us / 1000;
    let bye_ms = keys.iter().find(|r| r.whose == TERMINAL).unwrap().us / 1000;
    let marks = std::fs::read_to_string(take.join("landmarks.jsonl")).unwrap();
    let mark: Value = serde_json::from_str(marks.lines().next().unwrap()).unwrap();
    assert_eq!((&mark["what"], &mark["said"]), (&json!("mark"), &json!("a friction")));
    let ms = mark["ms"].as_u64().unwrap();
    assert!(ret_ms <= ms && ms <= bye_ms, "at its time: {ret_ms} ≤ {ms} ≤ {bye_ms}");
    let about: Value =
        serde_json::from_str(&std::fs::read_to_string(take.join("take.json")).unwrap()).unwrap();
    assert_eq!(about["cwd"], json!(cwd.display().to_string()));
}

/// The take's surroundings: your config copied with `llm.cost = false` and nothing else changed (yours untouched), the
/// rest of its folder linked beside it, git's stand-in name and mail — and the variables that give them.
#[test]
fn a_takes_surroundings_copy_your_config_with_cost_off_and_name_git_the_stand_ins() {
    let dir = Scratch::new("xtask-take-around");
    let (yours, home) = (dir.join("yours"), dir.join("home"));
    std::fs::create_dir_all(yours.join("themes")).unwrap();
    let text = "[llm]\ncommand = \"stand-in\"\ncost = true\n\n[editor]\nlsp = false\n";
    std::fs::write(yours.join("config.toml"), text).unwrap();
    std::fs::create_dir(&home).unwrap();
    let env = crate::record::take::surroundings(&home, Some(text), Some(&yours)).unwrap();

    let copy: toml::Table =
        std::fs::read_to_string(home.join("config/tarae/config.toml")).unwrap().parse().unwrap();
    assert_eq!(copy["llm"]["cost"].as_bool(), Some(false));
    assert_eq!(
        (copy["llm"]["command"].as_str(), copy["editor"]["lsp"].as_bool()),
        (Some("stand-in"), Some(false))
    );
    assert_eq!(std::fs::read_to_string(yours.join("config.toml")).unwrap(), text, "yours untouched");
    assert_eq!(std::fs::read_link(home.join("config/tarae/themes")).unwrap(), yours.join("themes"));
    let git = std::fs::read_to_string(home.join("gitconfig")).unwrap();
    assert!(git.contains("name = halil dev") && git.contains("email = dev@halil.invalid"), "{git}");
    let names: Vec<&str> = env.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        names,
        [
            "CARGO_BUILD_BUILD_DIR",
            "XDG_CONFIG_HOME",
            "GIT_CONFIG_GLOBAL",
            "GIT_AUTHOR_NAME",
            "GIT_AUTHOR_EMAIL",
            "GIT_COMMITTER_NAME",
            "GIT_COMMITTER_EMAIL"
        ]
    );
    let value = |k: &str| env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string_lossy().into_owned());
    for k in ["GIT_AUTHOR_NAME", "GIT_COMMITTER_NAME"] {
        assert_eq!(value(k).as_deref(), Some("halil dev"), "{k}");
    }
    for k in ["GIT_AUTHOR_EMAIL", "GIT_COMMITTER_EMAIL"] {
        assert_eq!(value(k).as_deref(), Some("dev@halil.invalid"), "{k}");
    }
    // (no config of yours: one of its own, cost off all the same)
    let bare = dir.join("bare");
    std::fs::create_dir(&bare).unwrap();
    crate::record::take::surroundings(&bare, None, None).unwrap();
    let copy: toml::Table =
        std::fs::read_to_string(bare.join("config/tarae/config.toml")).unwrap().parse().unwrap();
    assert_eq!(copy["llm"]["cost"].as_bool(), Some(false));
}

#[test]
fn a_bring_is_a_landmark() {
    let mut marks = crate::record::landmarks::Landmarks::default();
    let ev = json!({ "event": "bring", "session": "s1", "name": "dates", "base": "main", "how": "landed",
                     "tip": "abc1234", "commits": 2, "said": "…", "frame": 9 });
    let got = marks.told(4200, &ev);
    assert_eq!(
        got,
        [(
            4200,
            json!({ "what": "bring", "session": "s1", "name": "dates", "base": "main", "how": "landed",
                        "tip": "abc1234", "commits": 2 })
        )]
    );
}
