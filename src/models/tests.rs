use crate::editor::Editor;
use crate::event::WorkspaceId;
use crate::key::Key;
use crate::llm::ModelInfo;
use crate::scratch::Scratch;

/// What the chat's claude offers: two models with effort levels, one with none.
fn offered() -> Vec<ModelInfo> {
    let info = |value: &str, resolved: &str, efforts: &[&str]| ModelInfo {
        value: value.into(),
        name: value.into(),
        about: String::new(),
        resolved: resolved.into(),
        efforts: efforts.iter().map(|e| e.to_string()).collect(),
    };
    vec![
        info("opus", "claude-opus-5-5", &["low", "high"]),
        info("sonnet", "claude-sonnet-5-5", &["low", "high"]),
        info("haiku", "claude-haiku-5-5", &[]),
    ]
}

/// The session in front's chat open, its input with the keys, claude's list of models known.
fn chat(ed: &mut Editor) {
    crate::uistate::tests::chat(ed, "");
    ed.workspace.claude.chat.as_mut().unwrap().models = offered();
}

/// An editor whose config file is the test's own (`opus`, `high` in it, as read at start) — its chat open.
fn editor(s: &Scratch) -> Editor {
    let mut ed = crate::uistate::tests::editor();
    let file = s.join("config.toml");
    std::fs::write(&file, "[llm]\nmodel = \"opus\"\neffort = \"high\"\n").unwrap();
    (ed.config_file, ed.config.llm.model, ed.config.llm.effort) =
        (Some(file), Some("opus".into()), "high".into());
    chat(&mut ed);
    ed
}

/// Keys as typed — words split by spaces, `<space>` one — a frame after them.
fn keys(ed: &mut Editor, k: &str) {
    for w in k.split(' ') {
        let w = if w == "<space>" { "space" } else { w };
        ed.handle_key(w.parse::<Key>().unwrap_or_else(|_| panic!("a key: {w}")));
    }
    crate::uistate::tests::frame(ed);
}

/// `llm.model` and `llm.effort` as the config file holds them, read back.
fn saved(s: &Scratch) -> (String, String) {
    let v: toml::Table = toml::from_str(&std::fs::read_to_string(s.join("config.toml")).unwrap()).unwrap();
    let get = |k: &str| v["llm"].get(k).and_then(toml::Value::as_str).unwrap_or_default().to_string();
    (get("model"), get("effort"))
}

/// The picker's frame's top line as drawn.
fn picker_top(ed: &mut Editor) -> String {
    let mut bytes = Vec::new();
    crate::term::render(ed, &mut bytes, 120, 40).unwrap();
    let text = crate::screen::text(&bytes, 120, 40).unwrap();
    let kind = ed.picker.as_ref().expect("a picker").title.clone();
    text.lines().find(|l| l.contains('╭') && l.contains(&kind)).unwrap_or_default().to_string()
}

fn said(ed: &Editor) -> String {
    ed.messages.iter().map(|m| m.2.as_str()).collect::<Vec<_>>().join("\n")
}

/// #361: from the home session `C-t` and `:effort` save your default — the config file has it, and the pickers say so
/// beside their titles; a model that takes no effort clears the saved one.
#[test]
fn the_home_sessions_choice_is_your_default() {
    let s = Scratch::new("models-home");
    let mut ed = editor(&s);
    keys(&mut ed, "C-t");
    assert!(picker_top(&mut ed).contains("model your default"), "{}", picker_top(&mut ed));
    keys(&mut ed, "s o n n e t ret");
    assert_eq!(saved(&s), ("sonnet".into(), "high".into()), "{}", said(&ed));
    assert_eq!(ed.config.llm.model.as_deref(), Some("sonnet"));
    assert_eq!(ed.workspace.claude.choice, None, "nothing of the session's own");
    // The effort, from the code: `:effort` its picker
    keys(&mut ed, "esc esc : e f f o r t ret");
    assert!(picker_top(&mut ed).contains("effort your default"), "{}", picker_top(&mut ed));
    keys(&mut ed, "l o w ret");
    assert_eq!(saved(&s), ("sonnet".into(), "low".into()), "{}", said(&ed));
    // A model with no levels: the saved effort goes with the switch
    keys(&mut ed, ": m o d e l <space> h a i k u ret");
    assert_eq!(saved(&s), ("haiku".into(), String::new()), "{}", said(&ed));
    assert!(!said(&ed).contains("this session"), "{}", said(&ed));
}

/// #361: another session's `C-t` and `:effort` switch that session alone — the config file as it was, your default
/// the home session's still, the session's claude started on its own; its picker says `this session`. Kept with the
/// workspace, it comes back with it (`:restart`).
#[test]
fn another_sessions_choice_is_its_own() {
    let s = Scratch::new("models-other");
    let mut ed = editor(&s);
    let other = s.join("other");
    std::fs::create_dir_all(&other).unwrap();
    let id = ed.workspace_open(&other).unwrap();
    assert_eq!(ed.workspace.id, id);
    chat(&mut ed);
    let file = std::fs::read_to_string(s.join("config.toml")).unwrap();
    keys(&mut ed, "C-t");
    assert!(picker_top(&mut ed).contains("model this session"), "{}", picker_top(&mut ed));
    keys(&mut ed, "s o n n e t ret");
    assert_eq!(std::fs::read_to_string(s.join("config.toml")).unwrap(), file, "your default untouched");
    assert_eq!(ed.config.llm.model.as_deref(), Some("opus"));
    assert!(said(&ed).contains("Claude: sonnet · high for this session"), "{}", said(&ed));
    keys(&mut ed, "esc esc : e f f o r t ret");
    assert!(picker_top(&mut ed).contains("effort this session"), "{}", picker_top(&mut ed));
    keys(&mut ed, "l o w ret");
    assert_eq!(std::fs::read_to_string(s.join("config.toml")).unwrap(), file, "your default untouched");
    assert_eq!(ed.config.llm.effort, "high");
    let mine = super::Choice { model: Some("sonnet".into()), effort: "low".into() };
    assert_eq!(ed.workspace.claude.choice.as_ref(), Some(&mine));
    // What its Claude starts on: its own — the chat's and `space i`'s
    let cfg = super::config(&ed);
    assert_eq!((cfg.model.as_deref(), cfg.effort.as_str()), (Some("sonnet"), "low"));
    // Kept with the workspace: back after a restart, its own again
    let kept = crate::worktree::Kept::of(&ed.workspace);
    // The home session: your default, as it was
    assert!(ed.workspace_switch(WorkspaceId::HOME).is_in());
    assert_eq!(super::of(&ed), super::Choice { model: Some("opus".into()), effort: "high".into() });
    keys(&mut ed, "C-t");
    assert!(picker_top(&mut ed).contains("model your default"), "{}", picker_top(&mut ed));
    let mut again = crate::uistate::tests::editor();
    again.workspaces_back(vec![kept], Vec::new(), None, false);
    let back = again.workspaces.iter().find(|w| w.root == ed.workspaces[0].root).expect("back");
    assert_eq!(back.claude.choice.as_ref(), Some(&mine));
}
