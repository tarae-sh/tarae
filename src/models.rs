//! Choosing Claude's model and effort — from the list claude itself gives (its answer to `initialize`,
//! measured: value · displayName · description · resolvedModel · supportedEffortLevels).
//!
//! - `C-t` in the chat · `:model` · the palette: the models, the one in use marked; `:effort`: the levels
//!   the model takes. From the home session a choice is saved as your default (`llm.model` · `llm.effort` —
//!   `space i` uses them too); from any other it's that session's alone until it ends (`Session.choice`, kept with
//!   its workspace — #361), the picker's title saying which. Either is applied to the conversation at once: `set_model` and `apply_flag_settings {effortLevel}` control requests
//!   (measured: the next answer comes from the new model; `low`/`max` changed the thinking 47 → 174 tokens).
//!   Back to the model's own effort: `effortLevel: null` (measured: thinking back at the default's).
//! - The chat's title says which (`Opus 5.5 · high`) while it's idle — the model that answered (`init`, then
//!   each answer's `message.model`); when that isn't what's asked for, both (`claude-sonnet-5-5 · asked opus`).

use crate::editor::Editor;

#[cfg(test)]
mod tests;
use crate::agent::ModelInfo;

/// Before the agent has said which it offers (the chat not open yet): the ones its row knows it takes (`Row.models_known`
/// — Claude's aliases; none for one that lists its own only once its session starts).
fn fallback(row: &crate::agent::Row) -> Vec<ModelInfo> {
    (row.models_known.iter())
        .map(|&(v, n, a)| ModelInfo {
            value: v.into(),
            name: n.into(),
            about: a.into(),
            resolved: String::new(),
            efforts: Vec::new(),
        })
        .collect()
}

/// A session's own model and effort, in `llm.model`'s and `llm.effort`'s forms (None: `default`; empty: the model's
/// own) — what a session other than the home one switched to (`Session.choice`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    pub model: Option<String>,
    pub effort: String,
}

/// Whether a choice here is your default: the home session's (the checkout tarae started in) — any other session's
/// `C-t` and `:effort` switch that session alone.
pub fn saves_default(ed: &Editor) -> bool {
    ed.workspace.id == crate::event::WorkspaceId::HOME
}

/// What the session in front's agent starts on: its own choice, else your default — its row's model setting
/// (`Row.model_setting`) and `llm.effort`.
pub fn of(ed: &Editor) -> Choice {
    let row = crate::llm::session_row(ed);
    ed.workspace.claude.choice.clone().unwrap_or_else(|| default_of(&ed.config.llm, row))
}

/// Your default for a session on `row`.
fn default_of(llm: &crate::llm::LlmConfig, row: &crate::agent::Row) -> Choice {
    let model = (row.model_setting.of)(llm).map(String::from);
    Choice { model, effort: llm.effort.clone() }
}

/// The setting a model chosen here goes in: the session's row's (`Row.model_setting`).
fn model_path(ed: &Editor) -> &'static str {
    crate::llm::session_row(ed).model_setting.path
}

/// The config a Claude of the session in front starts with — `llm`, its model and effort the session's (`of`).
pub fn config(ed: &Editor) -> crate::llm::LlmConfig {
    let Choice { model, effort } = of(ed);
    crate::llm::LlmConfig { model, effort, ..ed.config.llm.clone() }
}

fn offered(ed: &Editor) -> Vec<ModelInfo> {
    match ed.workspace.claude.chat.as_ref().map(|c| &c.models).filter(|m| !m.is_empty()) {
        Some(m) => m.clone(),
        // (its row's own list until its agent says — none for an agent that lists none)
        None if crate::llm::session_row(ed).caps.models => fallback(crate::llm::session_row(ed)),
        None => Vec::new(),
    }
}

/// The value chosen (`default` when none is).
fn chosen(ed: &Editor) -> String {
    of(ed).model.unwrap_or_else(|| "default".into())
}

/// The model in use as the title says it: its name (for `default`, the model it stands for), and the effort
/// when the model takes one — `Opus 5.5 · high`. When what answered isn't what's asked for (a chat started
/// before `llm.model` changed), both: `claude-sonnet-5-5 · asked opus`. None before anything's known.
pub fn label(ed: &Editor) -> Option<String> {
    let c = ed.workspace.claude.chat.as_ref()?;
    let models = &c.models;
    let value = chosen(ed);
    let m = models.iter().find(|m| m.value == value);
    // (one that lists its models by the values it runs — an ACP agent's — is titled by the name of the one it runs,
    // whatever was asked: a value it doesn't offer is said on the chat's line at each start, `acp::Wire`)
    if let Some(now) = running(models, &c.model) {
        return Some(now.name.clone());
    }
    if !c.model.is_empty() && !asks_for(m, &value, &c.model) {
        return Some(format!("{} · asked {value}", c.model));
    }
    let name = match m {
        Some(m) if m.value == "default" => models
            .iter()
            .find(|o| o.value != "default" && o.resolved == m.resolved)
            .map_or(m.name.clone(), |o| o.name.clone()),
        Some(m) => m.name.clone(),
        None if of(ed).model.is_some() => value.clone(),
        None => return Some(c.model.clone()).filter(|m| !m.is_empty()),
    };
    let effort = of(ed).effort;
    let takes = m.is_some_and(|m| m.efforts.contains(&effort));
    Some(if takes { format!("{name} · {effort}") } else { name })
}

/// Session `l`'s model as its row in the session list says it: the family of what answered (`opus`), else of what it
/// asks for, and the effort where that model takes one — `opus · high`. Empty before anything's known (no chat yet,
/// nothing said).
pub fn row_label(llm: &crate::llm::LlmConfig, l: &crate::workspace::Workspace) -> String {
    let Some(c) = l.claude.chat.as_ref() else { return String::new() };
    if let Some(now) = running(&c.models, &c.model) {
        return now.name.clone();
    }
    let Choice { model, effort } = (l.claude.choice.clone()).unwrap_or_else(|| default_of(llm, c.agent));
    let value = model.unwrap_or_else(|| "default".into());
    let m = c.models.iter().find(|m| m.value == value);
    let id = match (c.model.as_str(), m) {
        ("", Some(m)) if !m.resolved.is_empty() => m.resolved.clone(),
        ("", _) if value != "default" => value.clone(),
        ("", _) => return String::new(),
        (id, _) => id.to_string(),
    };
    let bare = id.split('[').next().unwrap_or_default();
    let short = crate::usage::model_short(bare);
    let family = short.split(' ').next().unwrap_or_default();
    match m.is_some_and(|m| m.efforts.contains(&effort)) {
        true => format!("{family} · {effort}"),
        false => family.to_string(),
    }
}

/// The option an agent runs, where it lists its models by the very values it runs (an ACP agent's `configOptions`: no
/// `resolved`) — None for Claude's, whose aliases stand for an id.
fn running<'a>(models: &'a [ModelInfo], answered: &str) -> Option<&'a ModelInfo> {
    models.iter().find(|m| m.resolved.is_empty() && !answered.is_empty() && m.value == answered)
}

/// Whether the model `answered` is what `value` asks for: the model it resolves to (an alias, `default`) —
/// ids compared without a `[1m]`-style suffix, a dated one counting as the same — else, before claude has
/// said what it offers, the value itself or a family alias in the id (`opus`).
fn asks_for(m: Option<&ModelInfo>, value: &str, answered: &str) -> bool {
    let bare = |s: &str| s.split('[').next().unwrap_or(s).to_string();
    let a = bare(answered);
    match m.map(|m| bare(&m.resolved)).filter(|r| !r.is_empty()) {
        Some(r) => a.starts_with(&r) || r.starts_with(&a),
        None => value == "default" || a.contains(value),
    }
}

/// `C-t` · `:model` — the models, the one in use marked.
pub fn picker(ed: &mut Editor) {
    use crate::picker::{Action, Item};
    let models = offered(ed);
    // (the one in force: what an agent that lists its models by the values it runs says it runs — its setting empty,
    // or naming one it doesn't offer — else the value chosen)
    let answered = ed.workspace.claude.chat.as_ref().map_or("", |c| c.model.as_str());
    let now = running(&models, answered).map_or_else(|| chosen(ed), |m| m.value.clone());
    if models.is_empty() {
        let a = crate::llm::session_row(ed).shown;
        return ed.set_status(format!(
            "{a}'s models are listed once its session starts — :model <value> saves one"
        ));
    }
    let at = models.iter().position(|m| m.value == now).unwrap_or(0);
    let items: Vec<Item> = models
        .into_iter()
        .map(|m| Item {
            label: m.name.clone(),
            hint: m.about.clone(),
            glyph: Some(if m.value == now { ("●", "ui.accent") } else { ("◦", "ui.virtual") }),
            action: Action::Typed(format!("model {}", m.value)),
        })
        .collect();
    // A small card over the code, on the one in use (nothing to preview)
    let mut p = crate::picker::Picker::of("model", "model", items, false).without_preview();
    p.compact = true;
    p.selected = at;
    p.aside = Some(whose(ed).into());
    ed.open_picker(p, None);
}

/// `:effort` — the levels the model in use takes, and its own default.
pub fn effort_picker(ed: &mut Editor) {
    use crate::picker::{Action, Item};
    // (`no_effort`'s words, written at the call)
    let row = crate::llm::session_row(ed);
    if !row.caps.effort {
        let only = crate::agent::CLAUDES_ONLY;
        return ed.set_status(format!("not yet with {} — :effort sets {only}", row.name));
    }
    let value = chosen(ed);
    let levels = match offered(ed).into_iter().find(|m| m.value == value) {
        Some(m) if m.efforts.is_empty() && !m.resolved.is_empty() => {
            return ed.set_status(format!("{} has no effort levels", m.name));
        }
        Some(m) if !m.efforts.is_empty() => m.efforts,
        _ => ["low", "medium", "high", "xhigh", "max"].map(String::from).to_vec(),
    };
    let now = of(ed).effort;
    let about = |l: &str| match l {
        "low" => "Quick — thinks little",
        "medium" => "Balanced",
        "high" => "Thorough",
        "xhigh" => "More thorough",
        "max" => "As hard as it can",
        _ => "",
    };
    let mut items: Vec<Item> = vec![Item {
        label: "default".into(),
        hint: "The model's own".into(),
        glyph: Some(if now.is_empty() { ("●", "ui.accent") } else { ("◦", "ui.virtual") }),
        action: Action::Typed("effort default".into()),
    }];
    items.extend(levels.iter().map(|l| Item {
        label: l.clone(),
        hint: about(l).into(),
        glyph: Some(if *l == now { ("●", "ui.accent") } else { ("◦", "ui.virtual") }),
        action: Action::Typed(format!("effort {l}")),
    }));
    let mut p = crate::picker::Picker::of("effort", "effort", items, false).without_preview();
    p.compact = true;
    p.aside = Some(whose(ed).into());
    p.selected = if now.is_empty() { 0 } else { levels.iter().position(|l| *l == now).map_or(0, |i| i + 1) };
    ed.open_picker(p, None);
}

/// Why the session's row takes no effort from tarae (`Caps::effort`), in `agent::not_yet`'s words — None: it does.
fn no_effort(ed: &Editor) -> Option<String> {
    let row = crate::llm::session_row(ed);
    (!row.caps.effort)
        .then(|| crate::agent::not_yet(row, &format!(":effort sets {}", crate::agent::CLAUDES_ONLY)))
}

/// What a choice in the pickers will do, said faint beside their title.
fn whose(ed: &Editor) -> &'static str {
    if saves_default(ed) { "your default" } else { "this session" }
}

/// `llm.model` or `llm.effort` set to `v` where a choice goes (`saves_default`): saved as your default (`save`), or
/// this session's own, kept with its workspace — what isn't saved, why.
fn choose(ed: &mut Editor, path: &str, v: &str) -> Result<Option<String>, String> {
    if saves_default(ed) {
        return save(ed, path, v);
    }
    let mut c = of(ed);
    match path == model_path(ed) {
        true => c.model = Some(v.to_string()).filter(|v| !v.is_empty()),
        false => c.effort = v.to_string(),
    }
    ed.workspace.claude.choice = Some(c);
    ed.workspaces_keep();
    Ok(None)
}

/// A setting changed and saved, as `:set!` saves it (`Editor::save_setting`: the editor's own config file — none
/// in tests but the one they give). A write that fails, or no file to write in, keeps it until you quit: why, for the
/// answer to say.
fn save(ed: &mut Editor, path: &str, v: &str) -> Result<Option<String>, String> {
    let v = toml::Value::from(v);
    crate::settings::apply(&mut ed.config, path, &v)?;
    ed.overrides.retain(|(p, _)| p != path);
    let Err(e) = ed.save_setting(path, &v).unwrap_or(Err("no config directory".into())) else {
        return Ok(None);
    };
    ed.overrides.push((path.to_string(), v));
    Ok(Some(e))
}

/// What a switch says: done, or done until you quit — not saved, why.
fn said(ed: &mut Editor, what: String, unsaved: Option<String>) {
    match unsaved {
        Some(e) => ed.set_error(format!("{what} until you quit — not saved: {e}")),
        None => ed.set_success(what),
    }
}

/// After what a switch chose, outside the home session: that it's this session's alone.
fn alone(ed: &Editor) -> &'static str {
    if saves_default(ed) { "" } else { " for this session" }
}

/// `:model <value>` — your default from the home session, else this session's (`choose`), and the conversation goes
/// on with it.
pub fn set(ed: &mut Editor, value: &str) -> Result<(), String> {
    let known = offered(ed);
    let m = known.iter().find(|m| m.value == value || m.name.eq_ignore_ascii_case(value));
    let value = m.map_or(value.to_string(), |m| m.value.clone());
    // (what the conversation ran on — the line over the input says what the next message costs)
    ed.switching();
    let path = model_path(ed);
    let mut unsaved = choose(ed, path, if value == "default" { "" } else { &value })?;
    let told = crate::chat::agent(ed).filter(|a| a.caps().models).is_some_and(|a| a.set_model(&value));
    // (one whose models come only from its session, that session not started yet: saved, and nothing said of it
    // running — its session's start sets it, or says it isn't offered)
    if !told && known.is_empty() {
        let a = crate::llm::session_row(ed).shown;
        let what = format!("Model {value} saved{} — checked when {a}'s session starts", alone(ed));
        said(ed, what, unsaved);
        return Ok(());
    }
    // (what answers is known again from the next answer — until then the title says what was asked for; what it
    // runs on now is this chat's)
    if let Some(c) = ed.workspace.claude.chat.as_mut() {
        c.model.clear();
        c.runs_on.0 = Some(value.clone()).filter(|v| v != "default");
    }
    // An effort the new model doesn't take goes (it'd be refused)
    let effort = of(ed).effort;
    if m.is_some_and(|m| !m.resolved.is_empty() && !effort.is_empty() && !m.efforts.contains(&effort)) {
        unsaved = unsaved.or(choose(ed, "llm.effort", "")?);
        if let Some(c) = ed.workspace.claude.chat.as_mut() {
            c.runs_on.1.clear();
        }
    }
    let name = label(ed).or_else(|| m.map(|m| m.name.clone())).unwrap_or(value);
    let cost = ed.switched_said();
    let a = crate::llm::session_row(ed).shown;
    said(ed, format!("{a}: {name}{}{cost}", alone(ed)), unsaved);
    Ok(())
}

/// `:effort <level>` — your default from the home session, else this session's (`choose`), and applied to the
/// conversation.
pub fn set_effort(ed: &mut Editor, level: &str) -> Result<(), String> {
    if let Some(why) = no_effort(ed) {
        return Err(why);
    }
    let level = if level == "default" { "" } else { level };
    ed.switching();
    let unsaved = choose(ed, "llm.effort", level)?;
    if let Some(a) = crate::chat::agent(ed).filter(|a| a.caps().effort) {
        a.set_effort(Some(level).filter(|l| !l.is_empty()));
    }
    if let Some(c) = ed.workspace.claude.chat.as_mut() {
        c.runs_on.1 = level.to_string();
    }
    let what = if level.is_empty() { "the model's default".to_string() } else { level.to_string() };
    let cost = ed.switched_said();
    said(ed, format!("Effort: {what}{}{cost}", alone(ed)), unsaved);
    Ok(())
}
