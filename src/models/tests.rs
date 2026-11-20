use crate::agent::ModelInfo;
use crate::editor::Editor;
use crate::event::WorkspaceId;
use crate::key::Key;
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

/// The chat on the stand-in's row (agy's shape: its models only once its session lists them), none listed yet.
fn on_stand_in(ed: &mut Editor) {
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    (c.agent, c.agent_fixed) = (&crate::agent::STAND_IN, true);
    c.models.clear();
}

/// An ACP agent's list: values that run as they are (no `resolved`).
fn acp_offered() -> Vec<ModelInfo> {
    let info = |value: &str, name: &str| ModelInfo {
        value: value.into(),
        name: name.into(),
        about: String::new(),
        resolved: String::new(),
        efforts: Vec::new(),
    };
    vec![info("m-one", "Model One"), info("m-two", "Model Two")]
}

/// A setting as the config file holds it, read back.
fn saved_key(s: &Scratch, k: &str) -> String {
    let v: toml::Table = toml::from_str(&std::fs::read_to_string(s.join("config.toml")).unwrap()).unwrap();
    v["llm"].get(k).and_then(toml::Value::as_str).unwrap_or_default().to_string()
}

/// Pinned (3): `:model x` on a row whose models come from its session, that session not started: saved in its row's
/// setting (`llm.agy-model`, Claude's `llm.model` untouched), said as saved and checked at its start — never as running.
/// `C-t` there says when its list comes, opening nothing.
#[test]
fn a_model_chosen_before_the_session_starts_is_saved_and_checked_at_its_start() {
    let s = Scratch::new("models-before");
    let mut ed = editor(&s);
    on_stand_in(&mut ed);
    keys(&mut ed, "C-t");
    assert!(ed.picker.is_none(), "nothing to list");
    assert_eq!(said(&ed), "stand-in's models are listed once its session starts — :model <value> saves one");
    keys(&mut ed, "esc esc : m o d e l <space> m - t w o ret");
    assert!(
        said(&ed).ends_with("\nModel m-two saved — checked when stand-in's session starts"),
        "{}",
        said(&ed)
    );
    assert_eq!(ed.config.llm.agy_model.as_deref(), Some("m-two"));
    assert_eq!((saved_key(&s, "agy-model"), saved_key(&s, "model")), ("m-two".into(), "opus".into()));
    assert_eq!(ed.config.llm.model.as_deref(), Some("opus"), "Claude's setting untouched");
    assert_eq!(super::of(&ed).model.as_deref(), Some("m-two"), "the session starts on it");
}

/// `C-t` on a row whose session listed its models: its own list; the title the name of the one it runs.
#[test]
fn the_stand_ins_own_models_are_listed_and_its_running_one_titled() {
    let s = Scratch::new("models-acp");
    let mut ed = editor(&s);
    on_stand_in(&mut ed);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    (c.models, c.model) = (acp_offered(), "m-one".into());
    keys(&mut ed, "C-t");
    let labels: Vec<String> = ed.picker.as_ref().unwrap().items().iter().map(|i| i.label.clone()).collect();
    assert_eq!(labels, ["Model One", "Model Two"]);
    keys(&mut ed, "esc");
    // (the setting names one it doesn't offer: the title is still what runs — its line said it at the start)
    ed.config.llm.agy_model = Some("m-gone".into());
    assert_eq!(super::label(&ed).as_deref(), Some("Model One"));
}

/// `C-t` with the row's setting empty (the design session's fix 4 to L4) marks the model its session reports running,
/// with the mark Claude's picker gives the one in force (`●`, the accent); the rest `◦`. The same with the setting
/// naming one it doesn't offer: what runs is marked.
#[test]
fn c_t_marks_the_model_the_session_runs() {
    let s = Scratch::new("models-mark");
    let mut ed = editor(&s);
    on_stand_in(&mut ed);
    let c = ed.workspace.claude.chat.as_mut().unwrap();
    (c.models, c.model) = (acp_offered(), "m-two".into());
    for setting in [None, Some("m-gone")] {
        ed.config.llm.agy_model = setting.map(String::from);
        keys(&mut ed, "C-t");
        let items = ed.picker.as_ref().expect("the list").items();
        let marks: Vec<(String, Option<(&str, &str)>)> =
            items.iter().map(|i| (i.label.clone(), i.glyph.map(|g| (g.0, g.1)))).collect();
        assert_eq!(
            marks,
            [("Model One".into(), Some(("◦", "ui.virtual"))), ("Model Two".into(), Some(("●", "ui.accent")))],
            "setting {setting:?}"
        );
        assert_eq!(ed.picker.as_ref().unwrap().selected, 1, "on the one in force");
        keys(&mut ed, "esc");
    }
}

/// `:effort` on a row tarae sets no effort for (`Caps::effort`): refused in `not_for`'s words, nothing changed — the
/// picker too.
#[test]
fn effort_on_a_row_without_it_is_refused_and_changes_nothing() {
    let s = Scratch::new("models-effort");
    let mut ed = editor(&s);
    on_stand_in(&mut ed);
    keys(&mut ed, "esc esc : e f f o r t <space> l o w ret");
    assert!(
        said(&ed).ends_with("not yet with stand-in — :effort sets Claude's sessions only"),
        "{}",
        said(&ed)
    );
    keys(&mut ed, ": e f f o r t ret");
    assert!(ed.picker.is_none());
    assert!(
        said(&ed).ends_with("only\nnot yet with stand-in — :effort sets Claude's sessions only"),
        "{}",
        said(&ed)
    );
    assert_eq!(ed.config.llm.effort, "high");
    assert_eq!(saved(&s), ("opus".into(), "high".into()));
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
