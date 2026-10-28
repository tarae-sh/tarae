//! orchestra Part I (`.sb/orchestra-plan.md`) — the session tools a parent's chat has: `sessions_start` and
//! `session_send` (P1a; `sessions` comes with P1b, reading back). A parent's Claude asks for a set of sessions on one
//! card (`What::SessionSet`); your `y` makes them behind — the front session never moves — each its own branch,
//! checkout and Claude, its brief its first message; `session_send` puts its words into a child's chat, after
//! anything of yours queued there. Nothing a child does comes back to the parent yet; cards, brings and ends stay
//! yours (no session tool reaches `offer_accept_by`).

use serde_json::{Value, json};
use unicode_width::UnicodeWidthStr as _;

use crate::editor::Editor;
use crate::event::{SessionId, WorkspaceId};
use crate::worktree::Making;

/// A set of sessions Claude asked for (`sessions_start`), on its card until answered — its call waits on it.
#[derive(Clone, Debug)]
pub struct Set {
    pub call: crate::tools::Call,
    /// The workspace whose Claude asked — the children's parent — and its name, as the card says it.
    pub parent: crate::event::WorkspaceId,
    pub parent_name: String,
    /// Each session's making and brief, by name (the call's object, sorted). One base and billing for all.
    pub sessions: Vec<(Making, String)>,
}

/// `sessions_start`: the set looked at on a worker (every name a new branch, its checkout's place free — one bad
/// name refuses the whole set, nothing made), then its card; the call is answered by the card's answer.
pub fn start(ed: &mut Editor, call: crate::tools::Call, args: &Value) -> Result<(), String> {
    let asked = args["sessions"]
        .as_object()
        .filter(|o| !o.is_empty())
        .ok_or("sessions: name → brief, one at least")?;
    let mut set = Vec::new();
    for (name, brief) in asked {
        let brief = brief.as_str().map(str::trim).unwrap_or_default();
        if brief.is_empty() {
            return Err(format!("{name}: its brief is empty — say its goal, where, and what done means"));
        }
        set.push((name.trim().to_string(), brief.to_string()));
    }
    let base = args["base"].as_str().map(str::trim).filter(|b| !b.is_empty()).map(str::to_string);
    let parent = ed.workspace.id;
    let waiting = ed.offers.iter().chain(&ed.workspace.parked);
    if let Some(o) = waiting
        .into_iter()
        .find(|o| matches!(&o.what, crate::offer::What::SessionSet(s) if s.parent == parent))
    {
        return Err(format!("a start card waits already — card#{}", o.id));
    }
    // (one set at a time, from its call to its answer — its card is pushed after a worker's look, so one asked beside
    // it would pass `llm.children-max` too; a past process's call counts no more: its answer goes nowhere)
    let generation = ed.workspace.claude.chat.as_ref().map(crate::chat::Chat::generation);
    ed.workspace.claude.starting.retain(|c| Some(c.generation()) == generation);
    if !ed.workspace.claude.starting.is_empty() {
        return Err("a start is under way — its card comes; one set at a time".into());
    }
    // (how many it may have open at once: yours to say, `llm.children-max` — asked before any card, and again at `y`)
    let (open, max) = (children(ed, ed.workspace.claude.id).count(), ed.config.llm.children_max);
    if open + set.len() > max {
        return Err(over_max(set.len(), open, max));
    }
    ed.workspace.claude.starting.push(call.clone());
    let root = ed.workspace.root.clone();
    let setting = ed.config.workspace_worktrees.clone();
    let builds = ed.workspace_store.builds.clone();
    let parent_name = ed.workspace.label().to_string();
    let work = move || makings(&root, &set, base.as_deref(), &setting, builds.as_ref());
    ed.events.jobs().spawn(move || {
        let found = work();
        move |ed: &mut Editor| match found {
            Err(e) => crate::tools::answer(ed, call, Err(e)),
            Ok(sessions) => {
                // (who pays: one answer for the set, as a new session's card asks it — orchestra decision 10)
                ed.api_check(ed.events.jobs(), move |ed, _| {
                    let (billing, ask, note) = ed.new_session_ask();
                    let sessions = sessions
                        .into_iter()
                        .map(|(mut m, brief)| {
                            (m.billing, m.billing_ask, m.billing_note) = (billing, ask.clone(), note.clone());
                            (m, brief)
                        })
                        .collect();
                    let o = card(ed, Set { call, parent, parent_name, sessions });
                    ed.push_offer(o);
                });
            }
        }
    });
    Ok(())
}

/// The refusal of a set of `n` past `llm.children-max` with `open` children open.
fn over_max(n: usize, open: usize, max: usize) -> String {
    format!(
        "{n} asked with {open} of yours open — llm.children-max is {max} (the user's setting): nothing was made"
    )
}

/// `sessions_start`'s call answered, whichever way: no longer a set under way (`Session.starting`).
pub fn answered(ed: &mut Editor, call: &crate::tools::Call) {
    for l in std::iter::once(&mut ed.workspace).chain(&mut ed.workspaces) {
        l.claude.starting.retain(|c| !c.same(call));
    }
}

/// Each name looked at as `:session-new` looks at one — the first measured whole, the rest beside it (one checkout,
/// one base); two names whose checkouts would be one folder refused. On a worker.
fn makings(
    root: &std::path::Path,
    set: &[(String, String)],
    base: Option<&str>,
    setting: &str,
    builds: Option<&crate::builds::Builds>,
) -> Result<Vec<(Making, String)>, String> {
    let mut out: Vec<(Making, String)> = Vec::new();
    for (name, brief) in set {
        let m = match out.first() {
            None => crate::worktree::making(root, name, base, setting, builds),
            Some((first, _)) => crate::worktree::making_beside(first, name, setting),
        }
        .map_err(|e| format!("{name}: {e} — nothing was made"))?;
        if let Some((other, _)) = out.iter().find(|(o, _)| o.path == m.path) {
            return Err(format!(
                "{name} and {} would share one checkout folder — nothing was made",
                other.branch
            ));
        }
        out.push((m, brief.clone()));
    }
    Ok(out)
}

/// The set's card: each session a row — its name and its brief's first line, cut (whole under the mouse, every
/// brief whole on `v`) —, where their checkouts go and build, what's copied in, who pays, and what their reports
/// carry back.
pub fn card(ed: &Editor, s: Set) -> crate::offer::Offer {
    let n = s.sessions.len();
    let Some((first, _)) = s.sessions.first() else {
        return crate::offer::Offer::new(
            String::new(),
            "",
            "",
            String::new(),
            crate::offer::What::SessionSet(Box::new(s)),
        );
    };
    let title = match n {
        1 => "Start a session?".to_string(),
        _ => format!("Start {n} sessions?"),
    };
    let mut q = match n {
        1 => format!(
            "A branch from {}, its own checkout and Claude — the brief its first message:",
            first.base_said()
        ),
        _ => format!(
            "Each a branch from {}, its own checkout and Claude — its brief its first message:",
            first.base_said()
        ),
    };
    let w = s.sessions.iter().map(|(m, _)| m.branch.width()).max().unwrap_or(0);
    // Each a row, its brief's first line cut at its end. A cut brief doesn't hold this weighty card's `y` as a card cut
    // in height holds it until `v` was read to its end (user, 2026-10-28): every brief is whole under the mouse and on
    // `v`, and each is its child's first message — in sight in that chat, its Claude's every act asking as any — so
    // what `y` decides (that sessions start, where, who pays) is all on the card. Cut in height, it waits as any
    let mut rows = Vec::new();
    for (k, (m, brief)) in s.sessions.iter().enumerate() {
        let head = brief.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
        let pad = " ".repeat(w - m.branch.width());
        // (more lines than its first: a `…` says so, whole under the mouse — every `…` tarae draws is)
        let more = brief.lines().filter(|l| !l.trim().is_empty()).count() > 1;
        q += &format!("\n  {}{pad}  {head}{}", m.branch, if more { " …" } else { "" });
        rows.push((k + 1, format!("{}{pad}  {brief}", m.branch)));
    }
    let under = first.path.parent().map(crate::allow::tilde).unwrap_or_default();
    q += &format!("\n\n{} in {under}/", if n == 1 { "Its checkout goes" } else { "Their checkouts go" });
    let top = &first.top;
    let shared = |d: &std::path::Path| !d.starts_with(top);
    match &first.builds {
        Some((tool, target, _)) if shared(target) => {
            q += &format!(
                "\n{tool} builds into {} (your config): shared, one build at a time.",
                crate::allow::tilde(target)
            )
        }
        Some((tool, _, build)) if shared(build) && first.build_own.is_some() => {
            let each = if n == 1 { "it" } else { "each" };
            q += &format!("\n{tool} builds {each} in a build-dir of its own, from cold");
            match first.build_own {
                Some(size) if size.0 > 0 => {
                    q += &format!(" — this checkout's is {}.", crate::worktree::size_text(size))
                }
                _ => q += ".",
            }
        }
        _ => {}
    }
    if !first.copies.is_empty() {
        let into = if n == 1 { "it" } else { "each" };
        q += &format!("\nCopied into {into} (not in git here): {}", first.copies.join("  "));
    }
    let api = first.billing == crate::billing::Billing::Api;
    if let Some(window) = &first.billing_ask {
        let (on, other) = if api { ("your API key", "your plan") } else { ("your plan", "your API key") };
        let their = if n == 1 { "Its Claude runs" } else { "Their Claudes run" };
        q += &format!("\n\nThe plan's {window}. {their} on {on} — $ puts them on {other}.");
    }
    if let Some(note) = &first.billing_note {
        q += &format!("\n\n{note}");
    }
    let warn = (api && ed.weakness_due()).then(|| {
        let at = q.split('\n').count() + 1;
        q += &format!("\n\n{}", crate::billing::WEAKNESS);
        at..at + 1
    });
    // (the threat model's §2.x: what a child read reaches its parent's Claude through its reports)
    q += &match n {
        1 => format!("\n\nWhat its Claude reads, its reports carry to {}'s Claude.", s.parent_name),
        _ => format!("\n\nWhat their Claudes read, their reports carry to {}'s Claude.", s.parent_name),
    };
    if let Some((b, sha)) = first.others.first() {
        let said = if b == sha { sha[..7].to_string() } else { format!("{b} ({})", &sha[..7]) };
        q += &format!("\n\nb — from {said} instead");
    }
    let briefs: Vec<String> = s
        .sessions
        .iter()
        .flat_map(|(m, brief)| {
            std::iter::once(format!("# {}", m.branch))
                .chain(brief.lines().map(str::to_string))
                .chain([String::new()])
        })
        .collect();
    let mut o = crate::offer::Offer::new(
        title,
        &q,
        "Making their checkouts…",
        "Couldn't make the sessions".into(),
        crate::offer::What::SessionSet(Box::new(s)),
    );
    o.rows = rows;
    o.output = Some(("briefs".into(), briefs));
    match warn {
        Some(lines) => o.warning(lines),
        None => o,
    }
}

/// The set card answered: `yes` makes each on a worker, then each behind (`made`), else none — the call told either
/// way.
pub fn answer(ed: &mut Editor, id: u64, s: Set, yes: bool) {
    if !yes {
        return crate::tools::answer(ed, s.call, Ok("the user said no — nothing was made".into()));
    }
    let parent = ed.claude_of(s.parent);
    // (the cap again: what's open now — the setting lowered, a child linked back since the card came)
    let (open, max) = (children(ed, parent).count(), ed.config.llm.children_max);
    if open + s.sessions.len() > max {
        let why = over_max(s.sessions.len(), open, max);
        ed.offer_finished(id, Err(why.clone()));
        return crate::tools::answer(ed, s.call, Err(why));
    }
    let Set { call, parent_name, sessions, .. } = s;
    ed.events.shared_jobs().spawn(move || {
        let made: Vec<Made> = sessions
            .into_iter()
            .map(|(m, brief)| {
                let r = crate::worktree::make(&m);
                (m, brief, r)
            })
            .collect();
        move |ed: &mut Editor| self::made(ed, id, call, parent, &parent_name, made)
    });
}

/// A session as its checkout's making came out: its making, its brief, its checkout and what wasn't copied into it.
type Made = (Making, String, Result<(std::path::PathBuf, Vec<String>), String>);

/// The checkouts made: each a session behind, `parent`'s child, its chat opened there and its brief its first
/// message — nothing in front moves. One that failed is said, the rest go on; the call told what was made and not.
fn made(
    ed: &mut Editor,
    id: u64,
    call: crate::tools::Call,
    parent: SessionId,
    parent_name: &str,
    made: Vec<Made>,
) {
    let (mut names, mut failed, mut uncopied, mut base) = (Vec::new(), Vec::new(), Vec::new(), String::new());
    for (m, brief, r) in made {
        base = m.base_said();
        let (root, not_copied) = match r {
            Ok(r) => r,
            Err(e) => {
                failed.push(format!("{}: {e}", m.branch));
                continue;
            }
        };
        let child = ed.workspace_add(root);
        if let Some(l) = ed.workspace_by_id(child) {
            (l.name, l.branch, l.base) = (m.branch.clone(), Some(m.branch.clone()), Some(m.base.clone()));
            // (who pays, as its card said)
            l.claude.billing = m.billing;
            l.claude.parent = Some(parent);
        }
        if m.billing == crate::billing::Billing::Api {
            ed.weakness_shown();
        }
        if !not_copied.is_empty() {
            uncopied.push(format!("{} ({})", m.branch, not_copied.join(", ")));
        }
        let from = parent_name.to_string();
        in_workspace(ed, child, move |ed| {
            crate::chat::open(ed);
            to_child(ed, &from, &brief);
        });
        names.push(m.branch);
    }
    ed.workspaces_keep();
    if names.is_empty() {
        let why = failed.join(" · ");
        ed.offer_finished(id, Err(why.clone()));
        return crate::tools::answer(ed, call, Err(format!("nothing was made — {why}")));
    }
    ed.offer_finished(id, Ok(()));
    let n = names.len();
    let sessions = if n == 1 { "1 session".to_string() } else { format!("{n} sessions") };
    ed.set_success(format!("{sessions} from {parent_name} — {} · space W lists them", names.join(", ")));
    if !uncopied.is_empty() {
        ed.set_warning(format!("not copied into {}", uncopied.join(", ")));
    }
    if !failed.is_empty() {
        ed.set_error(format!("not made: {}", failed.join(" · ")));
    }
    let mut said = format!("made {} from {base} — each brief went as its first message", names.join(", "));
    if !failed.is_empty() {
        said += &format!("\nnot made: {}", failed.join(" · "));
    }
    crate::tools::answer(ed, call, Ok(said));
}

/// `f` in workspace `id` — swapped in for it now, or, while another one behind is (never a swap inside a swap), as
/// that workspace's own event next (`tools::deliver`'s way).
fn in_workspace(ed: &mut Editor, id: WorkspaceId, f: impl FnOnce(&mut Editor) + Send + 'static) {
    if !ed.background() {
        ed.with_workspace(id, f);
        return;
    }
    let tag = ed.claude_of(id);
    let _ = ed.events.shared_sender().send(crate::event::Event::For(id, tag, Box::new(f)));
}

/// The open sessions whose parent is `parent`.
fn children(ed: &Editor, parent: SessionId) -> impl Iterator<Item = &crate::workspace::Workspace> {
    std::iter::once(&ed.workspace).chain(&ed.workspaces).filter(move |l| l.claude.parent == Some(parent))
}

/// The one answer to a name that isn't a child of the asking session's: whether there's a session by that name at
/// all isn't told.
pub const NOT_YOURS: &str = "no session of yours is called that";

/// `session_send`: `message` into the chat of the asking session's child `session` — a turn now if it's idle, else
/// after its turn, after anything of the user's queued there (`chat::Later::Parent`); the call told which.
pub fn send(ed: &mut Editor, call: crate::tools::Call, args: &Value) -> Result<(), String> {
    let name = args["session"].as_str().map(str::trim).unwrap_or_default();
    let text = args["message"]
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or("message: what to tell it")?;
    let mine = |l: &&crate::workspace::Workspace| l.label() == name || l.branch.as_deref() == Some(name);
    let child = children(ed, ed.workspace.claude.id).find(mine).map(|l| l.id).ok_or(NOT_YOURS)?;
    let (from, text) = (ed.workspace.label().to_string(), text.to_string());
    // (always as the child's own event, never at once: one send while the parent is behind goes so, and a later one
    // made at once would land before it — the loop's order is the order they were sent in)
    let tag = ed.claude_of(child);
    let deliver = move |ed: &mut Editor| {
        let said = to_child(ed, &from, &text);
        crate::tools::answer(ed, call, Ok(said));
    };
    let _ = ed.events.shared_sender().send(crate::event::Event::For(child, tag, Box::new(deliver)));
    Ok(())
}

/// `text` from `from`'s Claude into the chat of the session in `ed.workspace` (its child): a turn now when its Claude
/// is idle and nothing of the user's waits, else queued after it — what the parent's answer says.
fn to_child(ed: &mut Editor, from: &str, text: &str) -> String {
    if ed.workspace.claude.chat.is_none() {
        // (opened as a note's reply opens one: where the keys are there stays)
        let at = ed.workspace.viewer.at;
        crate::chat::open(ed);
        ed.workspace.viewer.at = at;
    }
    let name = ed.workspace.label().to_string();
    let moving = ed.workspace.moving.is_some();
    let Some(c) = ed.workspace.claude.chat.as_ref() else {
        return format!("not sent — {name} has no chat");
    };
    let (busy, starting) = (c.busy(), c.settings.waiting() || !c.running());
    let yours = c.sends.iter().filter(|l| matches!(l, crate::chat::Later::Parent { .. })).count();
    // (at once only with nothing waiting before it — the user's queued message, or one sent earlier still held: a
    // turn refused for money, or put back as its claude ended)
    if !moving && !busy && c.queued.is_none() && yours == 0 && crate::chat::submit_from(ed, from, text) {
        return "sent — its turn started".into();
    }
    let Some(c) = ed.workspace.claude.chat.as_mut() else {
        return format!("not sent — {name} has no chat");
    };
    let users = c.queued.as_ref().map_or(0, |_| c.queued_n.max(1));
    c.sends.push(crate::chat::Later::Parent { from: from.to_string(), text: text.to_string() });
    // (a card it waits on is said as working, never named: a parent hears only what its own acts came to — the
    // user's decision, orchestra's R2)
    let why = match () {
        _ if moving => format!("{name}'s checkout is moving onto its base; it goes once that lands"),
        _ if busy => format!("{name} is working; it goes when this turn ends"),
        _ if starting => format!("{name}'s claude is starting; it goes once it's up"),
        _ if users > 0 => format!("a message of the user's waits in {name}; it goes after it"),
        _ => format!("what you sent {name} before still waits; it goes after it"),
    };
    let after = match (users, yours) {
        (0, 0) => String::new(),
        (0, y) => format!(" (after {y} of yours)"),
        (u, 0) => format!(" (after {u} of the user's)"),
        (u, y) => format!(" (after {u} of the user's and {y} of yours)"),
    };
    format!("queued — {why}{after}")
}

/// What a parent's message is to its child's Claude: in `<from-session>`, and the first time in a conversation the
/// line saying whose it is and where its final reply goes (orchestra §1, ~45 tokens once). Its closing tag carries an
/// id of its own, one the text doesn't hold (as a quote's — `quote::id_for`): a brief or message printing
/// `</from-session>` can't end the frame and have what follows read as the user's own words.
pub fn framed(from: &str, text: &str, first: bool) -> String {
    let from = from.replace(['"', '<', '>'], "");
    let id = crate::quote::id_for(text);
    let head = match first {
        true => format!(
            "This message is from Claude in session {from}, which started this session. The user sees this chat and \
             answers its cards; your final reply goes to {from}'s Claude.\n"
        ),
        false => String::new(),
    };
    format!("{head}<from-session name=\"{from}\" id=\"{id}\">\n{text}\n</from-session id=\"{id}\">")
}

/// `llm.session-tools`' values: whose chat gets the session tools.
pub const MODES: &[&str] = &["home", "any", "off"];

/// Whether the chat of `ed.workspace` may start sessions of its own (`llm.session-tools`): `home` — the checkout
/// tarae started in (no branch of a session's own); `any` — any session that isn't a child; `off` — none. A child
/// never: no children of children.
pub fn may_parent(ed: &Editor) -> bool {
    if ed.workspace.claude.parent.is_some() {
        return false;
    }
    match ed.config.llm.session_tools.as_str() {
        "home" => ed.workspace.branch.is_none(),
        "any" => true,
        _ => false,
    }
}

/// Whether the chat of `ed.workspace` lists the session tools: as its process started (`Chat.session_tools`) —
/// the list is the cache's prefix, fixed for the process's life; a `:set` since applies to the next one.
pub fn lists(ed: &Editor) -> bool {
    ed.workspace.claude.chat.as_ref().is_some_and(|c| c.session_tools)
}

pub fn start_schema() -> Value {
    json!({
        "name": "sessions_start",
        "description": "Start sessions, each a new branch (from base, default main) with its own checkout and \
    Claude; its brief is its first message. One card asks the user for all.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "sessions": {
                    "type": "object",
                    "description": "Branch name → brief: goal, where, what done means; it knows nothing of this chat",
                    "additionalProperties": { "type": "string" }
                },
                "base": { "type": "string" }
            },
            "required": ["sessions"]
        }
    })
}

pub fn send_schema() -> Value {
    json!({
        "name": "session_send",
        "description": "Message a session you started: now if idle, else after its turn.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "session": { "type": "string" },
                "message": { "type": "string" }
            },
            "required": ["session", "message"]
        }
    })
}

/// A set card of `names` (each brief its name's), asked by the front workspace's Claude, as `start` would put it up —
/// for a test of the card alone. Its `y` makes nothing: each checkout's place is the root's folder, there already, so
/// `worktree::make` stops before any write or git.
#[cfg(test)]
pub(crate) fn stand_in_card(ed: &Editor, names: &[&str]) -> crate::offer::Offer {
    let making = |b: &str| Making {
        top: ed.workspace.root.clone(),
        branch: b.into(),
        base: "main".into(),
        base_sha: "a".repeat(40),
        path: ed.workspace.root.clone(),
        files: (0, true),
        big: Vec::new(),
        builds: None,
        build_own: None,
        copies: Vec::new(),
        others: Vec::new(),
        billing: Default::default(),
        billing_ask: None,
        billing_note: None,
    };
    let sessions = names.iter().map(|n| (making(n), format!("Brief of {n}.\nIts second line."))).collect();
    let call = crate::tools::Call::of(u64::MAX, "stand-in", json!("set"));
    card(ed, Set { call, parent: ed.workspace.id, parent_name: ed.workspace.label().to_string(), sessions })
}

#[cfg(test)]
mod tests;
