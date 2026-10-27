//! Claude's edits from the chat: what its edit tools share (`edittool.rs`, `refactor.rs` — tarae makes every
//! change, in the in-buffer review or unseen under review at the end) — review at the end, which names ask
//! first and what they warn of, the review's title and reason, a turn's end — and the answer to claude's
//! permission requests.
//!
//! - claude runs with `--permission-prompt-tool stdio` and none of Claude Code's tools (`--tools ""`): a
//!   `control_request {subtype: "can_use_tool"}` it still sends is answered `{behavior: "deny", message}` — but
//!   a read's (`reads.rs`).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::document::DocId;
use crate::editor::Editor;
use crate::offer::{Offer, What};

#[derive(Default)]
pub struct Edits {
    /// The review showing one of ours (its `llm_generation`).
    pub review: Option<u64>,
    /// Review at the end (`llm.review-at-end`, every chat's — `space v a`, agreed on its card): edits under this
    /// folder go in as Claude makes them, and the turn is reviewed when it ends.
    pub at_end: Option<PathBuf>,
    /// …the setting's other way, waiting for this turn's end (`at_end_follow_config`): said in the chat once as it
    /// came, not again at every reload that says the same.
    pub at_end_pending: Option<bool>,
    /// Claude's renames, replaces and edits landing (`refactor.rs`): the front one's files reviewed one at a
    /// time.
    pub sets: VecDeque<crate::refactor::Landing>,
    /// Each file as Claude last saw it through tarae (`read`, or a change of its written) — its disk stamp; an
    /// `edit` of one that has changed since says so (`edittool.rs`).
    pub seen: std::collections::HashMap<PathBuf, crate::disk::Stamp>,
}

// ── Review at the end ───────────────────────────────────────────────────────

const AT_END_TITLE: &str = "Review Claude's edits at the end?";

/// What the card asks — what turning it on means.
const AT_END_ASK: &str = "For every chat — each session's, and new ones: Claude's edits go into your files as it \
makes them — before you see them — and the checks it runs (cargo test, cargo build) run that code.\n\nWhen its \
turn ends you review all of it; space u takes the turn back. Build scripts, settings and files outside the \
project still ask first.";

/// What Claude hears with its next message when it's turned on · off.
const TELL_ON: &str = "From now on in this conversation your edits are applied as you make them, \
without the user seeing each one: they review all of your changes when your turn ends. Edits outside the project, \
to build scripts and settings, and to files with unsaved edits, moves and deletes still go to the user first.";
const TELL_OFF: &str = "Your edits are shown to the user one at a time again, before they're written.";

/// Whether the chat's edits are reviewed at the end of its turn.
pub fn at_end(ed: &Editor) -> bool {
    ed.workspace.claude.chat.as_ref().is_some_and(|c| c.edits.at_end.is_some())
}

/// `space v a` · `:review-at-end [on|off]` (`want`; None flips it): on asks first, on its card.
pub fn at_end_toggle(ed: &mut Editor, want: Option<bool>) {
    // (the setting's: a chat mid-turn may still have the other until its turn ends)
    let on = ed.config.llm.review_at_end;
    let want = want.unwrap_or(!on);
    ed.offers.retain(|o| !matches!(o.what, What::ReviewAtEnd));
    if want == on {
        return ed.set_status(match on {
            true => "review at the end is on — space v a turns it off",
            false => "Claude's edits come one at a time",
        });
    }
    if at_end_held(ed, want) {
        return;
    }
    if !want {
        return at_end_set(ed, false);
    }
    let card = Offer::new(AT_END_TITLE.into(), AT_END_ASK, "", String::new(), What::ReviewAtEnd);
    ed.push_offer(card);
}

/// A new conversation (the chat opened, `C-l`, `:chat-new`): review at the end as `llm.review-at-end`
/// says — on, Claude hears it with its first message.
pub fn at_end_new_chat(ed: &mut Editor) {
    let on = ed.config.llm.review_at_end;
    let root = ed.workspace.root.clone();
    let Some(c) = &mut ed.workspace.claude.chat else { return };
    c.edits.at_end = on.then_some(root);
    let tell = &mut ed.workspace.claude.checkpoints.tell;
    tell.retain(|t| t != TELL_ON && t != TELL_OFF);
    if on {
        tell.push(TELL_ON.to_string());
    }
}

/// Claude hears the change with its next message — unless it never heard of the other.
fn at_end_told(ed: &mut Editor, on: bool) {
    let (say, unsay) = if on { (TELL_ON, TELL_OFF) } else { (TELL_OFF, TELL_ON) };
    let tell = &mut ed.workspace.claude.checkpoints.tell;
    match tell.iter().position(|t| t == unsay) {
        Some(i) => {
            tell.remove(i);
        }
        None => tell.push(say.to_string()),
    }
}

/// `llm.review-at-end` changed (`space v a` in any session, a reload of the file — another tarae's key too —, a
/// project's trust given or taken back, `:set`): every chat of this tarae — each session's — follows it, on and off
/// alike. A chat between turns at once, said in it; one mid-turn at its turn's end (`at_end_turn_over`), said in it
/// now — never mid-turn, where the edits it's making would change how they're seen under it. But your own key
/// (`space v a`, `:set`) switches the front chat at once, mid-turn too — in sight, yours: its callers
/// `at_end_follow_here` first.
pub fn at_end_follow_config(ed: &mut Editor) {
    let on = ed.config.llm.review_at_end;
    // (one that differs, or that waits for a change since taken back)
    let moves = |s: &crate::workspace::Workspace| {
        s.claude
            .chat
            .as_ref()
            .is_some_and(|c| c.edits.at_end.is_some() != on || c.edits.at_end_pending.is_some())
    };
    let ids: Vec<_> =
        std::iter::once(&ed.workspace).chain(&ed.workspaces).filter(|s| moves(s)).map(|s| s.id).collect();
    for id in ids {
        ed.with_workspace(id, |ed| {
            let Some(c) = &mut ed.workspace.claude.chat else { return };
            if !c.busy() {
                return at_end_follow_here(ed);
            }
            let differs = c.edits.at_end.is_some() != on;
            let want = differs.then_some(on);
            // (said once a change: a reload saying the same again — the watcher's after your own write too — is quiet)
            if c.edits.at_end_pending == want {
                return;
            }
            c.edits.at_end_pending = want;
            let row = match (differs, on) {
                (true, true) => "review at the end, every chat's · from this turn's end",
                (true, false) => {
                    "Claude's edits come one at a time again, every chat's · from this turn's end"
                }
                (false, true) => "review at the end stays on — the change was taken back",
                (false, false) => "Claude's edits stay one at a time — the change was taken back",
            };
            c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Note, row, ""));
        });
    }
}

/// The chat's turn ended: review at the end as the config says now — a change made while it ran takes effect here
/// (`at_end_follow_config` said so in it as it came).
pub fn at_end_turn_over(ed: &mut Editor) {
    at_end_follow_here(ed);
}

/// What a failed write of `llm.review-at-end` says — `space v a`'s and `:set!`'s alike: kept until you quit.
pub(crate) fn at_end_not_saved(on: bool, why: &str) -> String {
    let way = if on { "on" } else { "off" };
    format!("review at the end {way} until you quit — not saved: {why}")
}

/// A layer above your config file sets `llm.review-at-end` the other way (`EditorConfig.at_end_above`: the project's
/// trusted `.tarae.toml`, `.tarae.local.toml`): what `space v a` would write to yours wouldn't hold — the next
/// reload would take it back — so it's said where to change it, and nothing is written. One rule for every way you
/// set it: the card, `space v a`, `:set`, `:set!` (`typed::set_value`).
pub(crate) fn at_end_held(ed: &mut Editor, want: bool) -> bool {
    let Some((file, on)) = ed.config.at_end_above.clone().filter(|(_, on)| *on != want) else { return false };
    let way = if on { "on" } else { "off" };
    ed.set_warning(format!("this project's {file} sets review at the end {way} — change it there"));
    true
}

/// This workspace's chat as `llm.review-at-end` says, said in it and told to Claude with its next message.
pub fn at_end_follow_here(ed: &mut Editor) {
    let on = ed.config.llm.review_at_end;
    let root = ed.workspace.root.clone();
    let Some(c) = &mut ed.workspace.claude.chat else { return };
    c.edits.at_end_pending = None;
    if c.edits.at_end.is_some() == on {
        return;
    }
    c.edits.at_end = on.then_some(root);
    let row = match on {
        true => "review at the end, every chat's · space v a turns it off",
        false => "Claude's edits come one at a time again, every chat's",
    };
    c.msgs.push(crate::chat::Msg::new(crate::chat::Role::Note, row, ""));
    at_end_told(ed, on);
}

/// `y` on the card (on) · turned off: `llm.review-at-end`, kept in the config file — every chat's, each session's
/// (`at_end_follow_config`), and the chats after them.
pub fn at_end_set(ed: &mut Editor, on: bool) {
    // (a layer above yours read since its card came)
    if at_end_held(ed, on) {
        return;
    }
    // (opened as it was set — it's told below)
    if ed.workspace.claude.chat.is_none() {
        // (opened without the keys: they stay where they were)
        let at = ed.workspace.viewer.at;
        crate::chat::open(ed);
        ed.workspace.viewer.at = at;
        if let Some(c) = &mut ed.workspace.claude.chat {
            c.unfolded = false;
        }
    }
    ed.config.llm.review_at_end = on;
    ed.overrides.retain(|(p, _)| p != "llm.review-at-end");
    // (here, as `:set!` writes: on a worker each, a quick on-off could land off-on — and the config watch
    // would take that back — or cross a `:set!` of the same file)
    let kept = ed.save_setting("llm.review-at-end", &on.into());
    // (not written: kept until you quit, as `:set!`'s — a reload of the file would take it back)
    if let Some(Err(_)) = &kept {
        ed.overrides.push(("llm.review-at-end".into(), on.into()));
    }
    // (this chat at once, mid-turn too: your key, in sight — the others between turns)
    at_end_follow_here(ed);
    at_end_follow_config(ed);
    match (kept, on) {
        (Some(Err(e)), _) => ed.set_error(at_end_not_saved(on, &e)),
        (_, true) => {
            ed.set_success("Every chat's edits go in as Claude works — each turn reviewed when it ends")
        }
        (_, false) => ed.set_status("Every chat's edits come one at a time again"),
    }
}

/// A `can_use_tool` request: answered now (Some) or after its card (None). The chat has none of Claude Code's
/// tools, so only a read's is asked (`reads.rs`); any other is denied — tarae's `edit` makes every change.
pub fn request(ed: &mut Editor, request_id: &str, req: &Value) -> Option<Value> {
    let tool = req["tool_name"].as_str().unwrap_or_default();
    if crate::reads::TOOLS.contains(&tool) {
        return crate::reads::request(ed, request_id, req);
    }
    Some(json!({ "behavior": "deny", "message": format!("{tool} isn't available in tarae's chat") }))
}

/// Whether an edit to `path` (as Claude named it, from the root) changes a name Claude's runs can't write
/// (`sandbox::protected_at` — a build script, a manifest, `.git`, a hook) where it really leads: its `.` and `..`
/// taken lexically, then the parts that exist resolved — links followed (`src/x.rs -> ../build.rs`, a folder
/// linked into `.git/hooks`) — and as the system would walk it too (a link before a `..`). From the root (as
/// given, or real) when inside it; one that leads outside is judged as it leads, all its parts. A disk read: a
/// worker's (the review of 4b: the path as written missed `src/../build.rs`, a link, a root given otherwise).
pub fn protected(root: &Path, path: &Path) -> bool {
    use std::path::Component;
    let mut lexical = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                lexical.pop();
            }
            c => lexical.push(c),
        }
    }
    let real_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let judged = |p: &Path| {
        let rel = p.strip_prefix(&real_root).or_else(|_| p.strip_prefix(root));
        let parts: Vec<&std::ffi::OsStr> = match rel {
            Ok(rel) => rel.iter().collect(),
            Err(_) => p.iter().filter(|c| *c != "/").collect(),
        };
        !crate::sandbox::protected_at(&parts).is_empty()
    };
    let ways: Vec<PathBuf> =
        [crate::paths::real(&lexical), crate::paths::real(path)].into_iter().flatten().collect();
    // (neither resolves — no folder of it there: judged as written, `.` and `..` gone)
    match ways.is_empty() {
        true => judged(&lexical),
        false => ways.iter().any(|p| judged(p)),
    }
}

/// What the user asked last — its first line: the title of the edit tools' reviews, whichever tool made the
/// change (frictions34 43: tarae's `edit` said `edit a.rs`); Claude's words go under it (`reason`).
pub fn asked(c: &crate::chat::Chat) -> Option<String> {
    let m = c.msgs.iter().rev().find(|m| m.role == crate::chat::Role::User)?;
    Some(m.text.lines().next().unwrap_or_default().to_string())
}

/// Why Claude makes the change it's calling for now (its review shows it): its words right before this call
/// (after the call's own row), not ones from before earlier calls — else the plan's step in hand.
pub fn reason(c: &crate::chat::Chat) -> String {
    use crate::chat::Role;
    let turn = c.msgs.iter().rposition(|m| m.role == Role::User).map_or(0, |i| i + 1);
    let before: Vec<_> = c.msgs[turn..].iter().rev().skip_while(|m| m.role == Role::Tool).collect();
    let words = before
        .iter()
        .take_while(|m| m.role != Role::Tool)
        .find(|m| matches!(m.role, Role::Thought | Role::Assistant))
        .map(|m| prose(&crate::chat::displayed(&m.text)));
    let step = c.plan.as_ref().and_then(|p| {
        let i = p.current()?;
        Some(format!("step {}/{}: {}", i + 1, p.steps.len(), p.steps[i].text))
    });
    words.filter(|w| !w.is_empty()).or(step).unwrap_or_default()
}

/// Claude's words without its code, on one line: a fenced block (```` ``` ```` or `~~~`, its language word and its
/// lines) left out — the review's reason is prose (`…replaces the first: rust let mut threads: …` ran the
/// code into it).
fn prose(text: &str) -> String {
    let mut fence: Option<&str> = None;
    let mut words = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        match fence {
            Some(f) if t.starts_with(f) => fence = None,
            Some(_) => {}
            None if t.starts_with("```") => fence = Some("```"),
            None if t.starts_with("~~~") => fence = Some("~~~"),
            None => words.extend(line.split_whitespace()),
        }
    }
    words.join(" ")
}

/// Whether an edit to `rel` (from the project's root) still asks first: a name Claude's runs can't write
/// (`sandbox::protected_names` — `.git`, `.tarae.local.toml`, every pack's build scripts and manifests,
/// whatever the project's languages; nothing configured takes one out), or an entry of `llm.review-ask`,
/// yours, on top — a name matches any part of the path in any case (APFS opens `Build.rs` for `build.rs`), a
/// path (with `/`) its start. (The report sends Claude here for the protected ones.)
pub fn listed(rel: &Path, ask: &[String]) -> bool {
    let parts: Vec<&std::ffi::OsStr> = rel.iter().collect();
    let is = |p: &std::ffi::OsStr, name: &str| p.eq_ignore_ascii_case(name);
    !crate::sandbox::protected_at(&parts).is_empty()
        || ask
            .iter()
            .map(|a| a.trim().trim_end_matches('/'))
            .filter(|a| !a.is_empty())
            .any(|a| if a.contains('/') { rel.starts_with(a) } else { parts.iter().any(|p| is(p, a)) })
}

/// Every event: a review whose buffer is gone ends.
pub fn lost_review(ed: &mut Editor) {
    // A review whose buffer is gone draws nothing: it doesn't keep the keys, it ends and says so (#149 — one
    // held them, invisible, for 80 minutes; `Editor::close_doc` ends them, this is what's left if it didn't)
    if ed
        .workspace
        .claude
        .review
        .as_ref()
        .is_some_and(|r| r.ready && !ed.docs.iter().any(|d| d.id == r.doc_id))
    {
        crate::llm::close_review(ed);
        ed.set_error(
            "a change under review lost its buffer and can't be shown — it was declined, Claude was told",
        );
    }
}

/// What an edit that changes a name running later outside the sandbox warns of (`cardwarn.rs`, cardburden
/// 4b — the fact, `edit-protected`, worked out where its path really leads: `protected`) — judged by the
/// packs' entries and yours as a command's are, so the review's header says it in their words.
pub(crate) fn warned(ed: &Editor) -> Vec<crate::cardwarn::Warned> {
    let packs = crate::sandboxpack::get();
    let no = |_: &str| false;
    let facts = crate::cardwarn::Facts {
        cmd: "",
        scripts: &[],
        env: &no,
        wrappers: &packs.wrappers,
        reads: &[],
        script_changed: false,
        edit_protected: true,
        outside: false,
        in_outputs: &no,
    };
    crate::cardwarn::judge(packs.warn.iter().chain(&ed.config.cards_warn), &facts)
}

/// The chat turn ended (or claude exited): a review still waiting for it closes, and the files outside the
/// project its reviews opened go (`Document::aside` — not ones with unsaved edits of yours, nor ones you
/// worked in since). Only this session's: buffers are shared, and another session's Claude may be showing one
/// in its review (#149).
pub fn turn_over(ed: &mut Editor) {
    at_end_turn_over(ed);
    let Some(c) = &mut ed.workspace.claude.chat else { return };
    if let Some(g) = c.edits.review.take()
        && ed.workspace.claude.review.as_ref().is_some_and(|r| r.generation == g)
    {
        crate::llm::close_review(ed);
    }
    let reviewed: Vec<DocId> = std::iter::once(&ed.workspace)
        .chain(&ed.workspaces)
        .filter_map(|l| l.claude.review.as_ref().map(|r| r.doc_id))
        .collect();
    let aside: Vec<_> = ed
        .docs
        .iter()
        .filter(|d| d.aside && !d.is_modified())
        .filter(|d| ed.workspace.docs.contains(&d.id) && !reviewed.contains(&d.id))
        .map(|d| d.id)
        .collect();
    for id in aside {
        ed.close_doc(id);
    }
}

/// The code pane before a key, click or paste: its session, document, version and selection — None while what
/// you press goes to a review (its answer moves and edits the file, but isn't you going there).
pub struct Before(Option<(crate::event::WorkspaceId, DocId, u64, crate::selection::Selection)>);

/// Before every key, click or paste (`taken_up` after it).
pub fn before_input(ed: &Editor) -> Before {
    let d = ed.doc();
    Before(
        (ed.workspace.claude.review.is_none())
            .then(|| (ed.workspace.id, d.id, d.version(), d.selection().clone())),
    )
}

/// After every key, click or paste: the file in the code pane, if Claude's turn opened it aside, is yours
/// now if you went there — switched to it (the file you were in still open, in the same session), moved in
/// it or changed it. Not a key that went elsewhere (a review's answer, the chat's input, `space l`, a
/// picker that closed again: #46 — such files stayed, each keeping a language server), nor one that only
/// brought it to the front by closing yours (`:bc`, `C-x` in the buffer list) or switching sessions. One only
/// Claude's turn opened (`claudes_only`) is yours the same way — and what was held back for it, a grammar or a
/// server to offer, is offered then (#208).
pub fn taken_up(ed: &mut Editor, before: Before) {
    let Before(Some((session, id, version, sel))) = before else { return };
    let d = ed.doc();
    if !(d.aside || d.claudes_only || d.review_only) || ed.workspace.claude.review.is_some() {
        return;
    }
    let switched = d.id != id && ed.workspace.id == session && ed.docs.iter().any(|d| d.id == id);
    let edited = d.id == id && d.version() != version;
    // (one only a code review opened, while it's up: reading it is going there and moving in it — an edit
    // takes it up; after the review, as any other — frictions25 R7)
    let reading = d.review_only && !d.aside && !d.claudes_only && ed.workspace.code_review.is_some();
    let took = match reading {
        true => edited,
        false => switched || edited || (d.id == id && *d.selection() != sel),
    };
    if took {
        let d = ed.doc_mut();
        d.aside = false;
        let review = std::mem::take(&mut d.review_only);
        if std::mem::take(&mut d.claudes_only) || review {
            // (a load still under way offers as for any file now — it isn't Claude's only any more)
            let (here, held) = (d.id, d.grammar_held.take());
            if let Some(spec) = held {
                ed.offer_grammar(spec);
            }
            ed.attach_lsp(here);
        }
    }
}

/// Whether a file Claude's turn opens is aside — it goes when the turn ends unless you take it up: one
/// outside the project, or in its build output (`target/` — test fixtures, sandboxes; #46).
pub fn aside_path(root: &Path, path: &Path) -> bool {
    !path.starts_with(root) || path.starts_with(root.join("target"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review at the end lets in only edits that land inside the project (`paths::inside` — where the path
    /// really leads) and not to `.git` or a listed name (anywhere) or path (from the root).
    #[test]
    fn which_edits_go_in_unasked() {
        let ask: Vec<String> =
            ["build.rs", ".github", "src/secret/", "Cargo.toml"].iter().map(|s| s.to_string()).collect();
        let listed = |p: &str| listed(Path::new(p), &ask);
        assert!(!listed("src/main.rs"));
        assert!(listed(".git/hooks/pre-commit"), ".git always asks");
        assert!(listed("crates/a/build.rs"), "a name, anywhere");
        assert!(listed(".github/workflows/ci.yml"));
        assert!(listed("src/secret/key.rs"), "a path, from the root");
        assert!(!listed("lib/src/secret/key.rs"));
        assert!(!listed("src/build.rs.bak"));
        assert!(listed("Cargo.toml") && listed("crates/a/Cargo.toml"));
        // #20: a cargo config always asks, as `.git` does (the rust pack protects `.cargo`)
        for p in [".cargo/config.toml", ".cargo/config", "crates/a/.cargo/config.toml"] {
            assert!(listed(p), "{p}");
        }
        assert!(!listed("src/config.toml"));
        // Your own config always asks too (read with no trust card); a name in any case (APFS opens it)
        assert!(listed(".tarae.local.toml") && listed("BUILD.RS") && listed(".Cargo/Config.toml"));
        // Every name Claude's runs can't write is one its Edit asks about (the report sends it there) — with
        // `llm.review-ask` empty, as a project could set it: it only adds
        for name in crate::sandbox::protected_names() {
            assert!(super::listed(&Path::new("a").join(name).join("x"), &[]), "{name}");
        }
        // Every pack's, whatever the project: a Java build file asks in any project; the wrapper's folder is
        // its names in a row, not `gradle` alone
        for p in ["build.gradle.kts", "app/pom.xml", "gradle/wrapper/gradle-wrapper.properties", "Gradlew"] {
            assert!(super::listed(Path::new(p), &[]), "{p}");
        }
        assert!(!super::listed(Path::new("src/main/java/org/gradle/Api.java"), &[]));
    }

    /// The review of 4b: an edit is judged where its path really leads, not as Claude wrote it — `..` and `.`
    /// taken, links followed (a file linked to `build.rs`, a folder linked into `.git/hooks`), the root as given
    /// or real; one that leads outside the project judged as it leads. (Links and files in the test's own temp
    /// folder; nothing is written.)
    #[test]
    fn an_edit_is_judged_where_its_path_really_leads() {
        let scratch = crate::scratch::Scratch::new("edit-judged");
        // (the root as given: the temp folder unresolved — `/var` for `/private/var` on macOS)
        let base = std::env::temp_dir().join(scratch.file_name().unwrap());
        let (root, out) = (base.join("p"), base.join("out"));
        for d in [root.join("src"), root.join(".git/hooks"), out.clone()] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(root.join("build.rs"), "").unwrap();
        std::fs::write(root.join("src/a.rs"), "").unwrap();
        std::os::unix::fs::symlink("../build.rs", root.join("src/x.rs")).unwrap();
        std::os::unix::fs::symlink(".git/hooks", root.join("hooks")).unwrap();
        let real_root = std::fs::canonicalize(&root).unwrap();
        for (path, judged) in [
            (root.join("src/a.rs"), false),
            (root.join("src/new.rs"), false),
            (root.join("build.rs"), true),
            (root.join("src/../build.rs"), true),
            (root.join("./src/new/../../build.rs"), true),
            // (`..` taken: out of `.git` again — as written its parts name `.git`)
            (root.join(".git/new/../../src/new.rs"), false),
            (root.join("src/x.rs"), true),
            (root.join("hooks/pre-commit"), true),
            (real_root.join("src/../build.rs"), true),
            (out.join("Makefile"), true),
            (out.join("notes.txt"), false),
        ] {
            assert_eq!(protected(&root, &path), judged, "{}", path.display());
        }
        std::fs::remove_dir_all(&base).ok();
    }

    /// Edits the tests ask through `request`, as claude does (`file`, its old text, its new), to a stand-in
    /// claude that keeps what it's sent in `log`.
    fn chat_for(dir: &Path, log: &Path) -> Editor {
        let mut ed = Editor::new(crate::config::Config::default());
        ed.workspace.root = dir.to_path_buf();
        ed.config.llm.command = "sh".into();
        ed.config.llm.args = vec!["-c".into(), format!("cat > '{}'", log.display())];
        crate::chat::open(&mut ed);
        ed
    }

    /// The reason an edit's review shows is Claude's words without its code: a fenced block — its language
    /// word and its lines — left out, the words around it kept, on one line.
    #[test]
    fn an_edits_reason_is_its_words_without_code() {
        use crate::chat::{Msg, Role};
        let dir = crate::scratch::Scratch::new("edits-reason");
        let mut ed = chat_for(&dir, &dir.join("log"));
        let c = ed.workspace.claude.chat.as_mut().unwrap();
        c.msgs.push(Msg::new(Role::User, "why?", ""));
        let said = "Keying by the name replaces the first:\n\n```rust\nlet x = 1;\n```\n\nI will switch it to `entry`.\n\n~~~\nmore\n~~~";
        c.msgs.push(Msg::new(Role::Assistant, said, ""));
        assert_eq!(reason(c), "Keying by the name replaces the first: I will switch it to `entry`.");
    }

    /// Claude's `edit` of `file`, call `n`, as `tools::mcp` makes it.
    fn ask(ed: &mut Editor, n: u64, file: PathBuf, old: &str, new: &str) {
        let args = json!({ "edits": [{ "file_path": file, "old_string": old, "new_string": new }] });
        crate::edittool::call(ed, crate::tools::Call::test(n), &args).unwrap();
    }

    /// What call `n` was answered (`tools::ANSWERED`).
    fn answered(n: u64) -> Option<Result<String, String>> {
        crate::tools::ANSWERED
            .with(|a| a.borrow().iter().find(|(i, _)| *i == json!(n)).map(|(_, r)| r.clone()))
    }

    /// Claude asks through Claude Code's permission prompt for a tool the chat hasn't: denied, said why.
    #[test]
    fn a_permission_ask_for_another_tool_is_denied() {
        let dir = crate::scratch::Scratch::new("edits-denied");
        let mut ed = chat_for(&dir, &dir.join("log"));
        let input = json!({ "file_path": dir.join("a.rs"), "old_string": "a", "new_string": "b" });
        let r =
            request(&mut ed, "p1", &json!({ "tool_name": "Edit", "input": input })).expect("answered now");
        assert_eq!(r["behavior"], "deny");
        assert_eq!(r["message"], "Edit isn't available in tarae's chat");
        assert!(ed.workspace.claude.review.is_none());
    }

    /// Events until `done` (the edit is worked out and written on workers) — and, as the main loop does after
    /// every event, the next file's review up or its answer sent (`refactor::next`: a review closed here by hand
    /// sends none).
    fn pump(ed: &mut Editor, what: &str, done: impl Fn(&Editor) -> bool) {
        let end = std::time::Instant::now() + crate::test_results::PATIENCE;
        while !done(ed) {
            assert!(std::time::Instant::now() < end, "{what}");
            crate::refactor::next(ed);
            if let Some(ev) = ed.events.recv_timeout(std::time::Duration::from_millis(20)) {
                ed.handle_event(ev);
            }
        }
    }

    /// cardburden 4b: an edit of Claude's to a file that runs later outside the sandbox (`build.rs`, among
    /// every pack's protected names) says so on its review's header — in the tools pack's words, under the
    /// title, whole (its end says what to do) — and in `tarae state`; through a link to it too (the review of
    /// 4b); an edit to another file says nothing. (The files are the test's own, in its temp folder; nothing is
    /// written: each review is declined.)
    #[test]
    fn an_edit_to_a_protected_file_warns_on_its_review() {
        let scratch = crate::scratch::Scratch::new("edit-protected");
        let dir = scratch.to_path_buf();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("build.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.join("src/a.txt"), "one\n").unwrap();
        std::os::unix::fs::symlink("../build.rs", dir.join("src/x.rs")).unwrap();
        let mut ed = chat_for(&dir, &dir.join("stdin.log"));
        let reviewed = |ed: &Editor| ed.workspace.claude.review.is_some();
        ask(&mut ed, 1, dir.join("build.rs"), "fn main() {}", "fn main() { }");
        pump(&mut ed, "its review", reviewed);
        let r = ed.workspace.claude.review.as_ref().expect("its review");
        let ids: Vec<&str> = r.warnings.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["edit-protected"]);
        let mut bytes = Vec::new();
        crate::term::render(&mut ed, &mut bytes, 100, 30).unwrap();
        let mut parser = vt100::Parser::new(30, 100, 0);
        parser.process(&bytes);
        let rows: Vec<String> = parser.screen().rows(0, 100).collect();
        let title =
            rows.iter().position(|r| r.contains("build.rs") && r.contains("decided")).expect("the title");
        // (its rows under the title — wrapped at the code's width beside the chat)
        // (the code's side of each row: the chat beside it past its line)
        let code = |r: &String| r.split(" │ ").next().unwrap_or_default().trim().to_string();
        let under: String = rows[title + 1..title + 5].iter().map(code).collect::<Vec<_>>().join(" ");
        assert!(under.starts_with("▲ It changes a file that runs later outside the sandbox"), "{rows:#?}");
        assert!(under.contains("read all of it. (tools pack)"), "whole, not cut: {under}");
        let state = ed.ui_state().json();
        assert_eq!(state["panels"]["edit_review"]["warnings"], json!(["edit-protected"]), "{state}");
        crate::llm::close_review(&mut ed);
        // Through a link to it: the same
        pump(&mut ed, "the first answered", |_| answered(1).is_some());
        ask(&mut ed, 2, dir.join("src/x.rs"), "fn main() {}", "fn main() { }");
        pump(&mut ed, "its review", reviewed);
        assert_eq!(ed.workspace.claude.review.as_ref().unwrap().warnings.len(), 1, "src/x.rs -> ../build.rs");
        crate::llm::close_review(&mut ed);
        // Another file: no warning
        pump(&mut ed, "the second answered", |_| answered(2).is_some());
        ask(&mut ed, 3, dir.join("src/a.txt"), "one", "ONE");
        pump(&mut ed, "its review", reviewed);
        assert!(ed.workspace.claude.review.as_ref().expect("its review").warnings.is_empty());
        crate::llm::close_review(&mut ed);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The review of 4b, as this tarae runs: with review at the end on, an edit to a name that runs later
    /// outside the sandbox — `build.rs` here, nothing in `llm.review-ask` — isn't let in unasked: it's
    /// reviewed as it's made, with its warning. Another file goes in as Claude makes it (written, its answer
    /// says so). (Files and the stand-in claude's log in the test's own temp folder; the review is declined.)
    #[test]
    fn review_at_the_end_still_asks_for_a_protected_file_with_its_warning() {
        let scratch = crate::scratch::Scratch::new("edit-at-end");
        let dir = scratch.to_path_buf();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("build.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.join("src/a.txt"), "one\n").unwrap();
        let log = dir.join("stdin.log");
        let mut ed = chat_for(&dir, &log);
        assert!(ed.config.llm.review_ask.is_empty());
        ed.workspace.claude.chat.as_mut().unwrap().edits.at_end = Some(dir.clone());
        ask(&mut ed, 11, dir.join("build.rs"), "fn main() {}", "fn main() { }");
        pump(&mut ed, "its review", |ed| ed.workspace.claude.review.is_some());
        let ids: Vec<String> =
            ed.workspace.claude.review.as_ref().unwrap().warnings.iter().map(|w| w.id.clone()).collect();
        assert_eq!(ids, ["edit-protected"]);
        crate::llm::close_review(&mut ed);
        pump(&mut ed, "the first answered", |_| answered(11).is_some());
        // Another file: in, unasked
        ask(&mut ed, 12, dir.join("src/a.txt"), "one", "ONE");
        pump(&mut ed, "written unasked", |_| answered(12).is_some());
        assert!(ed.workspace.claude.review.is_none(), "no review for it");
        assert_eq!(std::fs::read_to_string(dir.join("src/a.txt")).unwrap(), "ONE\n");
        std::fs::remove_dir_all(&dir).ok();
    }
}
