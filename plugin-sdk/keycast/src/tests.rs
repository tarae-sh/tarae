use super::*;
use tarae::plugin::types::Buffer;

fn ev(keys: &[&str], done: bool, mode: Mode) -> KeyEvent {
    KeyEvent { keys: keys.iter().map(|k| k.to_string()).collect(), done, mode }
}

fn done(keys: &[&str]) -> KeyEvent {
    ev(keys, true, Mode::Normal)
}

fn under_way(keys: &[&str]) -> KeyEvent {
    ev(keys, false, Mode::Normal)
}

/// The row as text: a keycap `[k]`, a faint span `{f}`, the rest as it is. None: no overlay.
fn shown(actions: &[Action]) -> Option<String> {
    let [Action::Overlay(o)] = actions else { panic!("one overlay, nothing else: {actions:?}") };
    let o = o.as_ref()?;
    assert_eq!((o.rows.len(), o.hold), (1, None), "one row, for tarae's reading time");
    Some(
        o.rows[0]
            .iter()
            .map(|s| match s.role {
                Role::Key => format!("[{}]", s.text),
                Role::Faint => format!("{{{}}}", s.text),
                Role::Text => s.text.clone(),
            })
            .collect(),
    )
}

/// Keys heard one call each, 100 ms apart from `at`: what the last one shows.
fn after(s: &mut State, at: u64, keys: Vec<KeyEvent>) -> Option<String> {
    let mut last = None;
    for (i, k) in keys.into_iter().enumerate() {
        last = Some(s.heard(vec![k], at + 100 * i as u64));
    }
    shown(&last.expect("a key"))
}

/// The keys pressed and nothing else (overlays plan §7 8): keycaps, newest at the right, a binding's caps a cell apart
/// and bindings three; a sequence under way stands as the binding in progress and its end takes its place.
#[test]
fn a_run_of_commands_is_their_keys_newest_at_the_right() {
    let mut s = State::default();
    let row = after(
        &mut s,
        0,
        vec![
            done(&["j"]),
            done(&["j"]),
            done(&["j"]),
            done(&["j"]),
            under_way(&["C-w"]),
            done(&["C-w", "v"]),
            under_way(&["space"]),
        ],
    );
    assert_eq!(row.as_deref(), Some("[j] ×4   [C-w] [v]   [space]"));
    assert_eq!(
        after(&mut s, 1000, vec![done(&["space", "f"])]).as_deref(),
        Some("[j] ×4   [C-w] [v]   [space] [f]")
    );
}

/// A repeat folds as screenkey's (`j ×4`), a sequence's too once it's done; a count is its keys, and another count is
/// another binding.
#[test]
fn a_repeat_folds_and_a_count_is_its_keys() {
    let mut s = State::default();
    let keys =
        vec![done(&["3", "j"]), done(&["3", "j"]), done(&["2", "j"]), under_way(&["g"]), done(&["g", "g"])];
    assert_eq!(after(&mut s, 0, keys).as_deref(), Some("[3] [j] ×2   [2] [j]   [g] [g]"));
    let keys = vec![under_way(&["g"]), done(&["g", "g"])];
    assert_eq!(
        after(&mut s, 1000, keys).as_deref(),
        Some("[3] [j] ×2   [2] [j]   [g] [g] ×2"),
        "its end folds"
    );
}

/// Insert mode is the key that entered it and the one that left it, a faint `insert` after them while it lasts
/// (tarae hands over nothing typed); left by a binding of letters, none is named and only the `insert` goes.
#[test]
fn insert_mode_is_its_way_in_and_out() {
    let mut s = State::default();
    assert_eq!(after(&mut s, 0, vec![ev(&["i"], true, Mode::Insert)]).as_deref(), Some("[i]   {insert}"));
    assert_eq!(after(&mut s, 500, vec![done(&["esc"])]).as_deref(), Some("[i]   [esc]"));
    assert_eq!(
        after(&mut s, 900, vec![ev(&["o"], true, Mode::Insert)]).as_deref(),
        Some("[i]   [esc]   [o]   {insert}")
    );
    assert_eq!(after(&mut s, 1300, vec![done(&[])]).as_deref(), Some("[i]   [esc]   [o]"), "left by `j k`");
}

/// What it shows of a key is the name tarae hands over, whatever it is — none is spelled in the plugin (tarae's names
/// can change: `ret` or `enter`, `up` or `↑`).
#[test]
fn a_key_is_shown_by_the_name_it_came_with() {
    let mut s = State::default();
    let keys = vec![done(&[":"]), done(&["ret"]), done(&["enter"]), done(&["↑"]), done(&["S-tab"])];
    assert_eq!(after(&mut s, 0, keys).as_deref(), Some("[ret]   [enter]   [↑]   [S-tab]"));
}

/// The last four bindings; after a pause the row starts again — under tarae's shortest hold, so what faded never comes
/// back beside a new key.
#[test]
fn the_last_few_and_a_pause_starts_again() {
    let mut s = State::default();
    let keys = ["w", "b", "e", "x", "d"].iter().map(|k| done(&[k])).collect();
    assert_eq!(after(&mut s, 0, keys).as_deref(), Some("[b]   [e]   [x]   [d]"));
    assert_eq!(
        after(&mut s, 400 + QUIET_MS - 1, vec![done(&["u"])]).as_deref(),
        Some("[e]   [x]   [d]   [u]")
    );
    let quiet = 400 + 2 * QUIET_MS;
    assert_eq!(after(&mut s, quiet, vec![done(&["u"])]).as_deref(), Some("[u]"), "not `u ×2`");
}

/// The row fits in `CELLS` with the newest at its end, never past tarae's 80 (whose `…` would take the newest): older
/// bindings go whole; one alone too wide keeps its last keys after a faint `…`.
#[test]
fn the_row_is_cut_from_its_oldest() {
    let cells = |r: &[Span]| r.iter().map(width).sum::<usize>();
    let wide = |n: &str| done(&[&n.repeat(12), &n.repeat(12)]);
    let mut s = State::default();
    s.heard(vec![wide("a"), wide("b")], 0);
    let row = spans(&s.row, false);
    assert!(cells(&row) <= CELLS, "{}", cells(&row));
    assert_eq!(
        shown(&[s.overlay()]).as_deref(),
        Some("[bbbbbbbbbbbb] [bbbbbbbbbbbb]"),
        "the older one gone whole"
    );
    let count: Vec<String> = (0..40).map(|i| (i % 10).to_string()).collect();
    let mut keys: Vec<&str> = count.iter().map(String::as_str).collect();
    keys.push("j");
    let mut s = State::default();
    let row = after(&mut s, 0, vec![ev(&keys, true, Mode::Insert)]).unwrap();
    assert!(row.starts_with("{… }[") && row.ends_with("[9] [j]   {insert}"), "{row}");
    assert!(cells(&spans(&s.row, true)) <= CELLS);
}

fn cx() -> Context {
    Context {
        args: String::new(),
        buffer: Buffer {
            path: None,
            name: "a.rs".into(),
            language: None,
            text: None,
            version: 0,
            modified: false,
        },
        selections: Vec::new(),
        primary: 0,
        now: Now { millis: 0, entropy: Vec::new() },
    }
}

fn heard(keys: Vec<KeyEvent>, at: u64) -> Vec<Action> {
    Keycast::on_events(vec![HostEvent::Keys(keys)], Now { millis: at, entropy: Vec::new() })
}

/// It asks for `keys` alone and answers only with its overlay — what tarae lets a plugin that hears keys answer with;
/// `:keycast-off` puts it away and hears nothing until `:keycast`; a press on it puts it away until the next key.
#[test]
fn it_hears_keys_alone_and_answers_with_its_overlay_only() {
    assert_eq!(Keycast::permissions(), [Permission::Keys]);
    assert_eq!(shown(&heard(vec![done(&["w"])], 0)).as_deref(), Some("[w]"));
    assert_eq!(shown(&Keycast::run("keycast-off".into(), cx())), None);
    assert_eq!(heard(vec![done(&["ret"])], 100), []);
    assert_eq!(Keycast::run("keycast".into(), cx()), []);
    assert_eq!(shown(&heard(vec![done(&["ret"])], 200)).as_deref(), Some("[ret]"), "from a fresh row");
    let pressed = Keycast::on_events(vec![HostEvent::Pressed(0)], Now { millis: 300, entropy: Vec::new() });
    assert_eq!(shown(&pressed), None);
    assert_eq!(shown(&heard(vec![done(&["b"])], 400)).as_deref(), Some("[b]"));
}
