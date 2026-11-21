//! `record`'s tests: a harmless stand-in child (`sh` and `printf`, never tarae or its config) on a pty of the test's
//! own as the terminal the take is shown in; made-up takes for the replay and `check` (nothing of yours in them).

use std::fs::File;
use std::io::{Read, Write};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};

use serde_json::json;
use tarae_testkit::scratch::Scratch;

use super::check::{Yours, hits, mails, money};
use super::keys::{self, Step};
use super::landmarks::Landmarks;
use super::*;

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// The test's end of a terminal: what it's sent, chunk by chunk, until it closes.
fn reader(master: &File) -> Receiver<Vec<u8>> {
    let mut from = master.try_clone().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = from.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    rx
}

/// What `rx` gives into `seen` until `text` is in it (the shared patience).
fn until(rx: &Receiver<Vec<u8>>, seen: &mut Vec<u8>, text: &[u8]) {
    while !contains(seen, text) {
        match rx.recv_timeout(crate::PATIENCE) {
            Ok(chunk) => seen.extend(chunk),
            Err(e) => panic!(
                "{e} before {:?} came: {:?}",
                String::from_utf8_lossy(text),
                String::from_utf8_lossy(seen)
            ),
        }
    }
}

/// The rest of what `rx` gives, until its terminal closes.
fn rest(rx: &Receiver<Vec<u8>>, seen: &mut Vec<u8>) {
    while let Ok(chunk) = rx.recv_timeout(crate::PATIENCE) {
        seen.extend(chunk);
    }
}

/// `record` on a thread of its own: what it returns, for `ended`.
pub(super) fn taking(spec: Spec, cmd: Command) -> Receiver<Result<Taken>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || tx.send(record(spec, cmd)));
    rx
}

/// What `taking`'s take returned, within the shared patience — else a failure naming `waits`, the test's own wait,
/// first. Given up, its thread is left behind: its stand-in ends with the pty when the test binary does.
pub(super) fn ended(rx: Receiver<Result<Taken>>, waits: &str) -> Result<Taken> {
    rx.recv_timeout(crate::PATIENCE).unwrap_or_else(|e| {
        panic!(
            "{waits}: not within {} s ({e}) — a take ends once its command did and its screen was read to the pty's \
             end",
            crate::PATIENCE.as_secs()
        )
    })
}

fn sh(script: &str) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", script]);
    cmd
}

fn spec(out: PathBuf, size: (u16, u16), outer: Option<Outer>, keys: Vec<Step>) -> Spec {
    Spec { size, out, outer, keys, after_keys: AfterKeys::Stay, ui: None, cwd: None, hands: None }
}

/// A terminal of `cols`×`rows` for a take to be shown in: the test's end, and the take's.
fn terminal(cols: u16, rows: u16) -> (File, Outer) {
    let (master, slave) = crate::pty::open(rows, cols).unwrap();
    (File::from(master), Outer { input: slave.try_clone().unwrap(), output: slave })
}

fn bytes_of(recs: &[Rec], whose: u8) -> Vec<u8> {
    recs.iter().filter(|r| r.whose == whose).flat_map(|r| r.bytes.clone()).collect()
}

#[test]
fn a_take_logs_what_its_terminal_was_sent_and_what_it_sent_back_in_order_at_its_own_size() {
    let dir = Scratch::new("xtask-record-take");
    // (the outer terminal bigger than the take: inside, the take's own size)
    let (mut shown, outer) = terminal(100, 30);
    let rx = reader(&shown);
    let cmd = sh(r#"stty size; printf 'ready\033[1mbold\033[0m\001'; read -r x; printf 'got:%s' "$x""#);
    let take = dir.join("take");
    let taking = taking(spec(take.clone(), (80, 24), Some(outer), Vec::new()), cmd);
    let mut seen = Vec::new();
    until(&rx, &mut seen, b"ready");
    // (what the terminal sends back — a key, an answer to a question — reaches the command)
    shown.write_all(b"hi\r").unwrap();
    until(&rx, &mut seen, b"got:hi");
    let taken = ended(taking, "the take's end once its command printed got:hi and exited").unwrap();
    rest(&rx, &mut seen);

    let screen = read_log(&take.join("screen.bin")).unwrap();
    let logged = bytes_of(&screen, SCREEN);
    assert_eq!(
        String::from_utf8_lossy(&logged),
        String::from_utf8_lossy(&seen),
        "the log holds the bytes the terminal was sent, untouched"
    );
    assert!(contains(&logged, b"ready\x1b[1mbold\x1b[0m\x01"), "{:?}", String::from_utf8_lossy(&logged));
    assert!(
        contains(&logged, b"24 80"),
        "inside, the take's own size: {:?}",
        String::from_utf8_lossy(&logged)
    );
    let keys = read_log(&take.join("keys.bin")).unwrap();
    assert_eq!(bytes_of(&keys, TERMINAL), b"hi\r", "the keys' log holds what the terminal sent");
    for log in [&screen, &keys] {
        assert!(log.windows(2).all(|w| w[0].us <= w[1].us), "times in order: {log:?}");
    }
    assert_eq!((taken.exit, taken.screen_bytes, taken.key_bytes), (Some(0), logged.len() as u64, 3));
    let about: Value =
        serde_json::from_str(&std::fs::read_to_string(take.join("take.json")).unwrap()).unwrap();
    assert_eq!(about["size"], json!([80, 24]));

    // (the log replayed: the screen the bytes make, parsed at once)
    let mut at_once = vt100::Parser::new(24, 80, 0);
    at_once.process(&seen);
    let replayed = screens(&screen, (80, 24));
    assert_eq!(replayed.last().unwrap().1.contents(), at_once.screen().contents());
}

#[test]
fn a_terminal_smaller_than_the_take_is_refused_saying_the_size_it_needs_and_nothing_starts() {
    let dir = Scratch::new("xtask-record-small");
    let (_shown, outer) = terminal(60, 20);
    let marker = dir.join("started");
    let mut cmd = sh(r#"touch "$1""#);
    cmd.arg("sh").arg(&marker);
    let err = record(spec(dir.join("take"), (80, 24), Some(outer), Vec::new()), cmd).unwrap_err();
    assert!(err.contains("60×20") && err.contains("80×24"), "{err}");
    assert!(!marker.exists(), "the command never started");
    assert!(!dir.join("take").exists(), "no take kept");
    assert!(fits((80, 24), (80, 24)).is_ok() && fits((79, 30), (80, 24)).is_err());
}

#[test]
fn a_folder_holding_a_take_is_never_written_over() {
    let dir = Scratch::new("xtask-record-twice");
    let take = dir.join("take");
    std::fs::create_dir_all(&take).unwrap();
    std::fs::write(take.join("take.json"), "{}").unwrap();
    let err = record(spec(take.clone(), (80, 24), None, Vec::new()), sh("printf x")).unwrap_err();
    assert!(err.contains("already holds a take"), "{err}");
    assert_eq!(std::fs::read_to_string(take.join("take.json")).unwrap(), "{}");
}

/// A made-up stream as tarae writes it: a line before any frame, then frames each in a synchronized update that clears
/// and draws `frame N` — cut into chunks at awkward places (inside an update's opening and its end).
fn made_up_stream() -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut all = b"before\r\n".to_vec();
    for n in 0..6 {
        all.extend(format!("\x1b[?2026h\x1b[H\x1b[2Jframe {n}\x1b[?2026l").as_bytes());
    }
    // (the line before the frames whole; then inside an opening, inside an end, two frames in one chunk)
    let cuts = [8, 11, 14, 25, 33, 40, 52, 67, 70, 88, 101, 160];
    let mut chunks = Vec::new();
    let mut from = 0;
    for c in cuts.into_iter().filter(|&c| c < all.len()).chain([all.len()]) {
        chunks.push(all[from..c].to_vec());
        from = c;
    }
    (all, chunks)
}

#[test]
fn the_log_read_back_and_replayed_is_the_screens_the_bytes_make() {
    let dir = Scratch::new("xtask-record-replay");
    let (all, chunks) = made_up_stream();
    let path = dir.join("screen.bin");
    let mut f = std::io::BufWriter::new(File::create(&path).unwrap());
    for (i, c) in chunks.iter().enumerate() {
        put(&mut f, 1000 * i as u64, SCREEN, c).unwrap();
    }
    drop(f);
    let recs = read_log(&path).unwrap();
    assert_eq!(recs.iter().map(|r| r.bytes.clone()).collect::<Vec<_>>(), chunks, "the records read back");

    let replayed = screens(&recs, (40, 5));
    // (what the bytes make, whole frame by whole frame, parsed straight)
    let mut straight = vt100::Parser::new(5, 40, 0);
    let mut want = Vec::new();
    let mut rest = &all[..];
    let first = rest.windows(FRAME_START.len()).position(|w| w == FRAME_START).unwrap();
    straight.process(&rest[..first]);
    want.push(straight.screen().contents());
    rest = &rest[first..];
    while let Some(e) = rest.windows(FRAME_END.len()).position(|w| w == FRAME_END) {
        straight.process(&rest[..e + FRAME_END.len()]);
        want.push(straight.screen().contents());
        rest = &rest[e + FRAME_END.len()..];
    }
    let got: Vec<String> = replayed.iter().map(|s| s.1.contents()).collect();
    // (a chunk that ended two frames at once shows the last of them: every screen replayed is one of the frames, in
    // order, never a half-drawn one, and the last is the last)
    assert!(got.iter().all(|g| want.contains(g)), "only whole frames: {got:?} of {want:?}");
    assert!(
        got.windows(2).all(|w| want.iter().position(|x| *x == w[0]) < want.iter().position(|x| *x == w[1]))
    );
    assert_eq!(got.last(), want.last());
    assert!(replayed.windows(2).all(|w| w[0].0 <= w[1].0), "times in order");
}

#[test]
fn an_updates_opening_cut_between_chunks_is_held_for_the_next() {
    let mut w = Whole::default();
    assert_eq!(w.take(b"abc\x1b[?20"), b"abc");
    assert_eq!(w.take(b"26hX"), b"");
    assert_eq!(w.take(b"Y\x1b[?2026lZ"), b"\x1b[?2026hXY\x1b[?2026lZ");
    // (with no update open, everything at once)
    assert_eq!(w.take(b"plain"), b"plain");
}

fn yours() -> Yours {
    // (made up: never this machine's)
    let mut y = Yours {
        names: vec!["planted-person".into(), "Jo Plantedname".into()],
        homes: vec!["/Users/planted-person".into()],
        mails: vec![concat!("jo", "@planted.example").into()],
    };
    y.tidy();
    y
}

/// A made-up take in `dir`: `rows` drawn as screens, one a second.
fn made_up_take(dir: &Path, screens: &[&[&str]]) {
    std::fs::create_dir_all(dir).unwrap();
    let about = json!({ "v": 1, "size": [60, 6], "ms": 1000 * screens.len() as u64 });
    std::fs::write(dir.join("take.json"), about.to_string()).unwrap();
    let mut f = File::create(dir.join("screen.bin")).unwrap();
    for (i, rows) in screens.iter().enumerate() {
        let mut b = b"\x1b[H\x1b[2J".to_vec();
        for (r, row) in rows.iter().enumerate() {
            b.extend(format!("\x1b[{};1H{row}", r + 1).as_bytes());
        }
        put(&mut f, 1_000_000 * i as u64, SCREEN, &b).unwrap();
    }
}

/// Each busy size fills the wide video's width at its glyph, and the video draws it at that glyph.
#[test]
fn the_busy_sizes_fill_the_wide_video_at_their_glyphs() {
    let fonts = crate::shot::Fonts::load(1).unwrap();
    for px in super::spike::BUSY_PX {
        let (c, r) = super::spike::filling_at(px).unwrap();
        println!("{px} px: {c}×{r}");
        assert_eq!(video::fit(&fonts, usize::from(c), usize::from(r)), Some(px), "{c}×{r}");
        assert_ne!(
            video::fit(&fonts, usize::from(c) + 1, usize::from(r)),
            Some(px),
            "one column more doesn't fit"
        );
    }
}

/// A login under four letters (made up: `qz`) is yours only after a home's folder or before `@`; as a bare word — a
/// command's alias in the `:` list — it's weak, listed apart, counted, not failing.
#[test]
fn a_short_login_is_yours_only_where_unmistakable_and_weak_as_a_bare_word() {
    let dir = Scratch::new("xtask-record-check-short");
    let mut y = Yours { names: vec!["qz".into()], homes: Vec::new(), mails: Vec::new() };
    y.tidy();
    let rows: &[&str] = &[
        "write-quit   Save and quit · qz x",
        "$ cd /Users/qz/work",
        "~qz/notes · /home/qz",
        concat!("from qz", "@mail.example"),
        "/Users/qzx/work · aqz@ · ~qza",
    ];
    made_up_take(&dir, &[rows]);
    let found = super::check::scan(&dir, &y).unwrap();
    let said: Vec<(u16, &str, bool)> = found.iter().map(|h| (h.row, h.kind, h.weak)).collect();
    assert_eq!(
        said,
        [(0, "name", true), (1, "name", false), (2, "name", false), (3, "name", false), (3, "mail", false)],
        "{found:?}"
    );
    let list = super::check::listed(&found);
    assert!(list.contains("4 places\n") && list.contains("1 weak\n"), "{list}");
    assert!(list.find("write-quit") > list.find("4 places"), "the weak listed apart, after: {list}");
    // (four letters or more: a bare word is yours, as ever)
    let long = Yours { names: vec!["qzqz".into()], ..Yours::default() };
    assert_eq!(super::check::found("alias · qzqz x", &long), [("name", "qzqz".to_string(), false)]);
}

#[test]
fn check_finds_a_planted_name_home_mail_and_amount_and_nothing_in_a_clean_take() {
    let dir = Scratch::new("xtask-record-check");
    let planted: &[&str] = &[
        "  src/main.rs   by Jo Plantedname",
        "$ cd /Users/planted-person/work",
        concat!("author: jo", "@planted.example"),
        "cost $1,234.50 · ₩26,000 left · 3 USD",
    ];
    made_up_take(&dir.join("planted"), &[planted, planted]);
    let found = super::check::scan(&dir.join("planted"), &yours()).unwrap();
    let said: Vec<(u16, &str, &str)> = found.iter().map(|h| (h.row, h.kind, h.found.as_str())).collect();
    for want in [
        (0, "name", "Jo Plantedname"),
        (1, "home", "/Users/planted-person"),
        (1, "name", "planted-person"),
        (2, "mail", concat!("jo", "@planted.example (yours)")),
        (3, "money", "$1,234.50"),
        (3, "money", "₩26,000"),
        (3, "money", "3 USD"),
    ] {
        assert!(said.contains(&want), "{want:?} in {said:?}");
    }
    // (one place for as long as it stands: both screens, one hit)
    assert_eq!(found.len(), 7, "{said:?}");
    assert!(found.iter().all(|h| (h.from, h.to) == (0, 2000)), "{found:?}");

    let clean: &[&str] = &[
        "$ echo $1 and $HOME",
        "let ratio = 0.31; // planted-personal? no",
        "ssh user@host · @type:hi · v1.2",
        "12 tests, 3 files, 2.5 s",
    ];
    made_up_take(&dir.join("clean"), &[clean]);
    let none = super::check::scan(&dir.join("clean"), &yours()).unwrap();
    assert!(none.is_empty(), "{none:?}");
}

#[test]
fn mail_and_money_are_read_as_people_write_them() {
    assert_eq!(mails("to a.b+c@mail.example.org, then x@y"), vec!["a.b+c@mail.example.org"]);
    assert_eq!(money("$0.31 · € 12 · 1,200원 · $5 · v2 usd"), vec!["$0.31", "€ 12", "1,200원"]);
    // (a place shown twice, apart, is two places)
    let screen = |t: &str| {
        let mut p = vt100::Parser::new(2, 30, 0);
        p.process(t.as_bytes());
        p.screen().clone()
    };
    // (…and one standing over two screens that differ elsewhere, one place from its first)
    let frames =
        vec![(0, screen("$9.99")), (300, screen("$9.99 paid")), (500, screen("")), (900, screen("$9.99"))];
    let h = hits(&frames, 1000, &Yours::default());
    assert_eq!(h.iter().map(|h| (h.from, h.to)).collect::<Vec<_>>(), vec![(0, 500), (900, 1000)]);
}

#[test]
fn a_key_script_reads_the_shots_syntax_and_refuses_a_scenes_marks() {
    let steps = keys::parse("# a note\n@type:hi there\n\\r\n@2.5\n@?ready\n@30!gone\n \n\\x1b\n").unwrap();
    assert_eq!(
        steps,
        vec![
            Step::Type("hi there".into()),
            Step::Send(b"\r".to_vec()),
            Step::Pause(Duration::from_millis(2500)),
            Step::Wait { text: "ready".into(), max: Duration::from_secs(120), gone: false },
            Step::Wait { text: "gone".into(), max: Duration::from_secs(30), gone: true },
            Step::Send(b" ".to_vec()),
            Step::Send(b"\x1b".to_vec()),
        ]
    );
    for mark in ["@beat:a caption", "@zoom:x", "@press?thread.rs", "@=done", "@rec"] {
        let err = keys::parse(&format!("a\n{mark}\n")).unwrap_err();
        assert!(err.starts_with("line 2:") && err.contains("isn't a live take's step"), "{err}");
    }
}

#[test]
fn a_key_script_is_played_in_order_waiting_on_the_screen() {
    let dir = Scratch::new("xtask-record-keys");
    let take = dir.join("take");
    let cmd = sh(r#"printf ready; read -r x; printf 'got:%s' "$x"; read -r y; printf 'then:%s' "$y""#);
    let steps = keys::of_keys(&["@?ready", "hi", "\\r", "@?got:hi", "@type:bye", "\\r"]).unwrap();
    let taken = ended(
        taking(spec(take.clone(), (80, 24), None, steps), cmd),
        "the take's end once its script's last key was read and its command exited",
    )
    .unwrap();
    assert!(taken.missed.is_empty(), "{:?}", taken.missed);
    let keys = read_log(&take.join("keys.bin")).unwrap();
    assert_eq!(bytes_of(&keys, SCRIPT), b"hi\rbye\r");
    // (typed one character a record, `bye` three)
    assert_eq!(keys.len(), 6, "{keys:?}");
    let screen = bytes_of(&read_log(&take.join("screen.bin")).unwrap(), SCREEN);
    assert!(
        contains(&screen, b"got:hi") && contains(&screen, b"then:bye"),
        "{:?}",
        String::from_utf8_lossy(&screen)
    );
}

/// #650: a headless take's key script ran out with no quit and its command stays up — a stand-in that never ends on
/// its own, only on a shot's `:q!` (as tarae does). Past the settle (passed in: no wall time) the recorder ends it as
/// a shot ends tarae and marks the take so; never the take that waits for good.
#[test]
fn a_headless_take_whose_keys_ran_out_with_it_still_up_is_ended_and_said() {
    let dir = Scratch::new("xtask-record-ran-out");
    let take = dir.join("take");
    let cmd = sh(r#"printf up; while read -r l; do case "$l" in *:q!*) printf bye; exit 0;; esac; done"#);
    let mut s = spec(take.clone(), (80, 24), None, keys::of_keys(&["@?up", "hi"]).unwrap());
    s.after_keys = AfterKeys::End(Duration::from_millis(50));
    let taken =
        ended(taking(s, cmd), "the take ended by the recorder, its keys run out with it still up").unwrap();
    assert!(taken.ended_after_keys.is_some_and(|ms| ms >= 50), "{taken:?}");
    assert_eq!(taken.exit, Some(0), "ended by the quit, not a kill: {taken:?}");
    assert!(taken.said(&take).contains("ended by the recorder — its keys ran out"), "{}", taken.said(&take));
    let keys = bytes_of(&read_log(&take.join("keys.bin")).unwrap(), SCRIPT);
    assert_eq!(keys, b"hi\x1b:q!\r");
    let about: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(take.join("take.json")).unwrap()).unwrap();
    assert!(about["ended_after_keys_ms"].as_u64().is_some_and(|ms| ms >= 50), "{about}");
}

/// #650: where the hands reached the take, its key script's end is said and the take left to them — nothing sent.
#[test]
fn a_take_the_hands_reached_isnt_ended_when_its_keys_run_out() {
    let (seen, hub) = (Seen::default(), hands::Hub::default());
    hub.reach();
    // (the take's child, a stand-in of the test's own in a session of its own: broken, a quit's kill lands on it only)
    let mut child = Command::new("sleep");
    child.arg("600").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        child.pre_exec(|| shot::cvt(libc::setsid()).map(|_| ()));
    }
    let mut child = child.spawn().unwrap();
    let mut sent = Vec::new();
    // (anything sent ends the screen at once: broken, the quit goes no further than its words)
    let mut send = |b: &[u8]| {
        sent.extend_from_slice(b);
        seen.set(None, true);
    };
    let ended = ran_out(Duration::from_millis(10), &seen, &hub, &mut send, child.id());
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!((ended, sent), (None, Vec::new()));
}

#[test]
fn a_wait_that_never_comes_is_said_and_ends_with_the_screen() {
    let dir = Scratch::new("xtask-record-miss");
    let steps = keys::of_keys(&["@600?never", "x"]).unwrap();
    let taken = ended(
        taking(spec(dir.join("take"), (80, 24), None, steps), sh("printf done")),
        "the take's end with its wait missed, its command gone after printf done",
    )
    .unwrap();
    // (said as a wait missed, or — the command gone before the script began — as the screen ended before it; the
    // key after it never sent either way)
    assert!(taken.missed.first().is_some_and(|m| m.contains("never")), "{:?}", taken.missed);
    assert!(
        taken.missed.last().is_some_and(|m| m.starts_with("the screen ended before")),
        "{:?}",
        taken.missed
    );
    let keys = read_log(&dir.join("take/keys.bin")).unwrap();
    assert!(keys.is_empty(), "{keys:?}");
}

#[test]
fn landmarks_are_drawn_from_tarae_s_events_and_the_snapshot_after_them() {
    let mut l = Landmarks::default();
    let mut all = Vec::new();
    let state = |front: &str, cards: serde_json::Value| {
        json!({ "front": front, "workspaces": [{"id": "w0", "name": "main"}, {"id": "w1", "name": "fix"}],
                "cards": cards })
    };
    all.extend(l.state(10, &state("w0", json!([]))));
    all.extend(l.told(100, &json!({ "event": "turn", "session": "s0", "what": "started", "calls": 0 })));
    all.extend(
        l.told(200, &json!({ "event": "card-up", "id": "card#3", "session": "s0", "asks": "cargo test" })),
    );
    all.extend(l.state(210, &state("w0", json!([{ "id": "card#3", "kind": "run" }]))));
    // (the same card again, moved: no second landmark)
    all.extend(l.told(250, &json!({ "event": "card-up", "id": "card#3", "up": "stacked" })));
    all.extend(l.told(300, &json!({ "event": "card-gone", "id": "card#3", "how": "answered y" })));
    // (an asked run's steps as tarae says them: its card's, then the command's)
    let steps = [
        (305, "card up", false),
        (306, "answered yes", false),
        (310, "spawned", false),
        (320, "first output", false),
        (900, "exit 0", true),
    ];
    for (ms, step, ended) in steps {
        all.extend(l.told(
            ms,
            &json!({ "event": "run", "id": "run#1", "what": "cargo test", "step": step, "ended": ended }),
        ));
    }
    all.extend(l.told(950, &json!({ "event": "review", "session": "s0", "what": "began", "kind": "edit" })));
    all.extend(l.told(990, &json!({ "event": "turn", "session": "s0", "what": "ended", "calls": 3 })));
    all.extend(l.state(1500, &state("w1", json!([]))));
    // (a card gone before any snapshot: shown all the same, its kind not known)
    all.extend(l.told(1600, &json!({ "event": "card-up", "id": "card#4" })));
    all.extend(l.told(1610, &json!({ "event": "card-gone", "id": "card#4", "how": "declined" })));
    let said: Vec<(u64, String)> = (all.iter())
        .map(|(ms, v)| {
            let more =
                ["kind", "how", "name", "step"].iter().filter_map(|k| v[*k].as_str()).collect::<Vec<_>>();
            (*ms, format!("{} {}", v["what"].as_str().unwrap(), more.join(" ")).trim().to_string())
        })
        .collect();
    let want: Vec<(u64, &str)> = vec![
        (10, "front main"),
        (100, "turn-started"),
        (200, "card-shown run"),
        (300, "card-answered run answered y"),
        (310, "run-started spawned"),
        (900, "run-ended exit 0"),
        (950, "review-opened edit"),
        (990, "turn-ended"),
        (1500, "front fix"),
        (1600, "card-shown"),
        (1610, "card-answered declined"),
    ];
    assert_eq!(said, want.into_iter().map(|(m, s)| (m, s.to_string())).collect::<Vec<_>>());
}

#[test]
fn a_start_is_its_first_screen_that_stood_still() {
    let s = vt100::Parser::new(2, 20, 0).screen().clone();
    let frames: Vec<(u64, vt100::Screen)> = [0, 30, 80, 1500, 1600].iter().map(|&t| (t, s.clone())).collect();
    assert_eq!(settled(&frames, 1000), 2);
    assert_eq!(settled(&frames[..2], 1000), 1, "the last where none stood still");
}

#[test]
fn a_size_is_columns_by_rows() {
    assert_eq!(parse_size("128x36").unwrap(), (128, 36));
    assert_eq!(parse_size("140×40").unwrap(), (140, 40));
    assert!(parse_size("wide").is_err() && parse_size("10x2").is_err());
}
