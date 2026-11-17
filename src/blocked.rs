//! Acting on what the sandbox blocked — the user's call, never Claude's: `space m s` on a command's panel,
//! `space x s` on a test run's, `:sandbox-blocked`. A list made of the run's denials: let Claude's commands
//! read a place in home (`allow::read_choices` — never keys or Claude's and tarae's own; Claude's Read reads
//! it too), write in a folder (each folder from the denied path's own up to just below home — how wide is the
//! user's call; `allow::write_choices` leaves out what must never open), connect to a port on this machine
//! (not the risky ones — `sandbox::RISKY_PORTS`, by hand only), or run that one command outside the
//! sandbox. Each is a weighty card. A write or a port is for once (the command runs again now with it),
//! for this conversation, this session, or always (`allow.rs`; no longer than `Caps::storable` lets it) — and
//! runs it again now too. That run is the user's: how it ended goes to Claude with the next message
//! (`tools::For::User`).

use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use crate::allow::{Grant, Scope};
use crate::editor::Editor;
use crate::sandbox::Denial;
use crate::tools::{Extra, For, Job};

/// A run of Claude's the sandbox blocked something in: what ran, and what was denied.
#[derive(Clone, Debug)]
pub struct BlockedRun {
    pub job: Job,
    pub denials: Vec<Denial>,
    /// Linux: where the denials were told from (`Policy::denials_from`) — the cards say it.
    pub from: Option<String>,
    /// Claude's call waiting on the card, when Claude asked for it (`also` — `tools::also_ask`): the run is its,
    /// not the user's run again, and a no is said to it.
    pub claude: Option<crate::tools::Call>,
    /// The variables left out of its environment that its output names (`RunEnv::named`) — names only.
    pub env: Vec<String>,
    /// Claude's run's report, held while the card about it waits (Gradle's loopback — `loopback_card`): a no
    /// gives Claude that.
    pub report: Option<String>,
    /// Which run it was (the panel's) — the user's run again of it goes on by itself when it was Claude's last
    /// turn's failed one (piece D3, `Chat.failed`).
    pub key: Option<RunKey>,
    /// Its policy's rule for the files named like keys (`Policy::key_rules`): what a refused one offers.
    pub keys: Option<crate::sandbox::KeyRules>,
    /// What its card's grant of a place leaves of the files named like keys in it (`sandbox::grant_keys`, worked
    /// out on the card's worker) — None: not a place's, or not worked out (its words say them open).
    pub granting: Option<crate::allow::Keys>,
    /// The kept places its command names, each with whose it is (`cardwarn::kept_named`, worked out on the list's
    /// worker): its outside card's `names-kept` — a run denied at a kept place is the one that leads there.
    pub kept: Vec<String>,
}

/// A run of the session's: its command's or its tests', and which of them (`generation`: each spawn a new one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunKey {
    pub tests: bool,
    pub generation: u64,
}

impl BlockedRun {
    /// `job`, nothing blocked in it known, nobody waiting.
    pub fn of(job: Job) -> BlockedRun {
        BlockedRun {
            job,
            denials: Vec::new(),
            from: None,
            claude: None,
            env: Vec::new(),
            report: None,
            key: None,
            keys: None,
            granting: None,
            kept: Vec::new(),
        }
    }

    /// What ran, as the cards and Claude say it.
    pub fn label(&self) -> String {
        match &self.job {
            Job::Command(c, _) => c.clone(),
            Job::Tests(t) => format!("tests: {}", t.label),
        }
    }
}

/// Which run's denials: the command's panel, the test run's, or whichever has them (newest first).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Task,
    Test,
    Either,
}

/// One thing that could open: what, its list row's hint (what was denied, short — the part below the
/// folder offered), and what its card says was denied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub grant: Grant,
    pub hint: String,
    pub why: String,
}

/// What could open for `denials` in a session that may do `caps`: the places in home a read may open
/// (`allow::read_choices` — never what's kept from every run, `Caps.kept`), the folders a write may open
/// (`allow::write_choices` — never where a write runs something later, `Caps.later`), a port on this machine
/// (not a risky one) — each once. A file named like a key goes by the run's own rule (`keys`, its policy's
/// `key_rules` — None: the project's only).
#[cfg(test)]
pub fn choices(denials: &[Denial], caps: &crate::caps::Caps) -> Vec<Choice> {
    choices_by(denials, caps, None)
}

/// `choices`, by the run's rule for the files named like keys.
pub fn choices_by(
    denials: &[Denial],
    caps: &crate::caps::Caps,
    keys: Option<&crate::sandbox::KeyRules>,
) -> Vec<Choice> {
    let mut out: Vec<Choice> = Vec::new();
    let home = caps.home.as_deref().unwrap_or(Path::new("/"));
    let (root, kept, later) = (caps.root.as_path(), &caps.kept, &caps.later);
    // (a key's name as its row and the run's report say it: from the project its rule is for)
    let named = |t: &Path| crate::allow::from_project(keys.map_or(root, |k| k.root()), t);
    for d in denials {
        let target = crate::sandbox::profile_path(Path::new(&d.target));
        let below = |dir: &Path| {
            let below = target.strip_prefix(dir).map(|r| format!("…/{}", r.display()));
            below.ok().filter(|b| b != "…/").unwrap_or_else(|| crate::allow::tilde(&target))
        };
        let file = d.target.starts_with('/');
        let mut found: Vec<Choice> = Vec::new();
        // A terminal (macOS, 3c): the terminals — no folder of `/dev` nor the file itself
        if d.terminal() {
            let why = format!("{} was opening {}", d.process, d.target);
            found.push(Choice {
                grant: Grant::Terminals,
                hint: format!("{} opening a terminal", d.process),
                why,
            });
        }
        // A host the proxy held and the user's answer would lift (`host:port: why`) — never what its other checks
        // refused (an address, the server name)
        if d.op == crate::sandbox::proxy::HOST_OP
            && let Some((host, port)) =
                d.target.split_once(": ").and_then(|(at, _)| crate::allow::host_port(at))
        {
            let why = format!("{} was reaching it", d.process);
            found.push(Choice {
                grant: Grant::Host(host, port),
                hint: format!("{} reaching", d.process),
                why,
            });
        }
        let read = d.op.starts_with("file-read-data") || d.op == crate::sandbox::EITHER;
        // A file the run's rule closes by its name (7a, 7b; outside the project, frictions29 — macOS: a place a
        // grant or a default opened, or one whose folders a grant would close it in): that one file — no folder
        // of it (the place is read anyway, or opening it would close the name again)
        let closes = |t: &Path| match keys {
            Some(k) => k.closes(t),
            None => crate::sandbox::key_named_in(&crate::sandbox::profile_path(root), t),
        };
        // (macOS closes its changes too — B1, `KEY_WRITES`: one refused where runs write is the name's, not the
        // place's; where they don't — home, a place read only — the place's, its folder offered, as before B1)
        let refuses = |t: &Path| match keys {
            Some(k) => k.refuses_change(t),
            None => crate::sandbox::key_named_in(&crate::sandbox::profile_path(root), t),
        };
        let changed = d.op.starts_with("file-write") && d.op != "file-write-create" && refuses(&target);
        // (macOS: a folder the walk before its run locked for the keys it holds — G33: the key, where it's the only
        // one — opened, the next run's walk leaves it out and the folder is free; holding more, none, one opened
        // alone wouldn't free it. No folder either way: a grant of one doesn't lift the lock)
        let held = match keys {
            Some(k) if file && crate::sandbox::keyfolders::lock_op(&d.op) => k.held_by(&target),
            _ => Vec::new(),
        };
        if let [k] = held.as_slice() {
            found.push(Choice {
                grant: Grant::Key(k.to_path_buf()),
                hint: format!("{} moving or removing its folder · named like a key", d.process),
                why: format!(
                    "{} was moving or removing {}, which holds {}, named like a key",
                    d.process,
                    crate::allow::tilde(&target),
                    crate::allow::tilde(k)
                ),
            });
        }
        let key = file
            && held.is_empty()
            && (read || changed)
            && closes(&target)
            && !kept.iter().any(|k| target.starts_with(k));
        if key {
            // (as the panel's row says it: `Denial::plain`)
            let doing = match d.op.as_str() {
                "file-write-unlink" => "deleting",
                "file-write-data" => "writing",
                o if o.starts_with("file-write") => "changing",
                crate::sandbox::EITHER => "reading or writing",
                _ => "reading",
            };
            // (its hint: who did what, then why the file is closed — what a narrow list cuts first; not its path,
            // the row's own)
            found.push(Choice {
                grant: Grant::Key(target.clone()),
                hint: format!("{} {doing} · named like a key", d.process),
                why: format!("{} was {doing} {}, named like a key", d.process, named(&target)),
            });
        } else if file && !d.terminal() && read {
            found.extend(crate::allow::read_choices(&target, home, kept).into_iter().map(|place| {
                let why = format!("{} was reading {} in it", d.process, below(&place));
                Choice { hint: below(&place), grant: Grant::Read(place), why }
            }));
        }
        // (a key's change refused: its name's, so no folder opens it)
        let writing = d.op.starts_with("file-write") && !(key && changed) && held.is_empty();
        if file && !d.terminal() && (writing || d.op == crate::sandbox::EITHER) {
            found.extend(crate::allow::write_choices(&target, home, root, later, kept).into_iter().map(
                |dir| {
                    let why = format!("{} was writing {} in it", d.process, below(&dir));
                    Choice { hint: below(&dir), grant: Grant::Write(dir), why }
                },
            ));
        } else if let Some(n) = d.port_here()
            && !crate::sandbox::RISKY_PORTS.iter().any(|(p, _)| *p == n)
        {
            let why = format!("{} was connecting to it", d.process);
            let hint = format!("{} connecting", d.process);
            // (every port here: what picks its ports as it starts — Gradle's client and daemon — needs them all;
            // the one it was refused, picked anew at its next start, would only fail again: not offered)
            let picked = crate::sandboxpack::get().refused_port.iter().any(|r| r.picks(d));
            if !picked {
                found.push(Choice { grant: Grant::Port(n), hint: hint.clone(), why: why.clone() });
            }
            found.push(Choice { grant: Grant::Localhost, hint, why });
        } else if d.op == "network-outbound" && file {
            // A unix socket (#119): that file, by its real path — one that's there and is a socket
            use std::os::unix::fs::FileTypeExt as _;
            let real = std::fs::canonicalize(&d.target).ok().filter(|p| {
                std::fs::metadata(p).is_ok_and(|m| m.file_type().is_socket()) && !p.starts_with(root)
            });
            let why = format!("{} was connecting to it", d.process);
            found.extend(real.map(|s| Choice {
                grant: Grant::Socket(s),
                hint: format!("{} connecting", d.process),
                why,
            }));
        }
        for c in found {
            if !out.iter().any(|o| o.grant == c.grant) {
                out.push(c);
            }
        }
    }
    out
}

/// A file named like a key as the list offers it (`Grant::Key`): used where runs write, since its writes reach
/// there (`KeyRules::written`); read elsewhere — home, a place read only. With no rule, the project's only. Named
/// as the run's report names it (`allow::from_project`): from the project its rule is for (`root` with none).
pub(crate) fn key_label(k: &Path, keys: Option<&crate::sandbox::KeyRules>, root: &Path) -> String {
    let root = keys.map_or(root, |r| r.root());
    let written = match keys {
        Some(rules) => rules.written(k),
        None => k.starts_with(root),
    };
    let verb = if written { "using" } else { "reading" };
    format!("allow {verb} {} — that file only", crate::allow::from_project(root, k))
}

/// What could open for the variables `names` (left out of a run's environment, named in its output): each one
/// passed whole — never a name `readonly::env_allowed` keeps off (what finds programs, runs or loads code, the
/// packs' own).
pub fn env_choices(names: &[String]) -> Vec<Choice> {
    names
        .iter()
        .filter(|n| crate::readonly::env_allowed(n))
        .map(|n| Choice {
            grant: Grant::Env(n.clone()),
            hint: "named in its output".into(),
            why: format!("Its output names {n}, left out of your commands' environment"),
        })
        .collect()
}

impl Editor {
    /// The run of Claude's whose denials `which` means, if the sandbox blocked anything in it — what the
    /// kernel reported, and what its output says (`Policy::said_in_output`: the kernel's report gets lost).
    fn blocked_run(&self, which: Which) -> Option<BlockedRun> {
        // (what it was refused, and the variables left out its output names — None: neither)
        let denials = |g: Option<&crate::sandbox::Guard>,
                       output: &[(bool, String)]|
         -> Option<(Vec<Denial>, Vec<String>)> {
            let g = g?;
            let mut seen = g.watch().map(|w| w.blocked().seen).unwrap_or_default();
            if let crate::sandbox::Guard::In(p) = g {
                let said = p.said_in_output(output, &seen);
                let rest = p.merge_made(&mut seen, said);
                seen.extend(rest);
            }
            let env = g.run_env().map(|e| e.named(output)).unwrap_or_default();
            (!seen.is_empty() || !env.is_empty()).then_some((seen, env))
        };
        let from = |g: Option<&crate::sandbox::Guard>| match g {
            Some(crate::sandbox::Guard::In(p)) => p.denials_from(),
            _ => None,
        };
        let keys = |g: Option<&crate::sandbox::Guard>| match g {
            Some(crate::sandbox::Guard::In(p)) => Some(p.key_rules()),
            _ => None,
        };
        // (a run on a commit's tree: not offered — its tree is gone, so neither a grant nor a run outside could
        // run it again where it ran — review of #212)
        let task = self.workspace.task_run.as_ref().filter(|r| r.base.is_none()).and_then(|r| {
            let (denials, env) = denials(r.guard.as_ref(), &r.output.to_vec())?;
            Some((
                r.started,
                BlockedRun {
                    denials,
                    from: from(r.guard.as_ref()),
                    env,
                    key: Some(RunKey { tests: false, generation: r.generation() }),
                    keys: keys(r.guard.as_ref()),
                    ..BlockedRun::of(Job::Command(r.task.cmd.clone(), r.task.cwd.clone()))
                },
            ))
        });
        let test = self.workspace.test_run.as_ref().and_then(|r| {
            let job = Job::Tests(Box::new(r.target.clone()));
            let from = from(r.guard.as_ref());
            let (denials, env) = denials(r.guard.as_ref(), &r.output.to_vec())?;
            let key = Some(RunKey { tests: true, generation: r.generation() });
            let keys = keys(r.guard.as_ref());
            Some((r.started, BlockedRun { denials, from, env, key, keys, ..BlockedRun::of(job) }))
        });
        match which {
            Which::Task => task.map(|t| t.1),
            Which::Test => test.map(|t| t.1),
            Which::Either => match (task, test) {
                (Some(a), Some(b)) => Some(if a.0 >= b.0 { a.1 } else { b.1 }),
                (a, b) => a.or(b).map(|t| t.1),
            },
        }
    }

    /// `space m s` · `space x s` · `:sandbox-blocked` — what could open for what the sandbox blocked in that
    /// run: a list; Enter puts one on its card.
    pub fn sandbox_blocked(&mut self, which: Which) {
        // (the run meant was on a commit's tree: said, before what it was refused — review of #212)
        let base = self.workspace.task_run.as_ref().filter(|r| r.base.is_some());
        let newer =
            |t: &crate::task::Run| self.workspace.test_run.as_ref().is_none_or(|x| t.started >= x.started);
        let on_base = match which {
            Which::Task => base.is_some(),
            Which::Test => false,
            Which::Either => base.is_some_and(newer),
        };
        if on_base {
            return self.set_status(
                "that run was on a commit's tree, gone since — nothing of it to open or run again",
            );
        }
        // (nothing blocked: the panel still opens — its state line, and why its list is empty)
        let Some(run) = self.blocked_run(which) else { return self.blocked_open(None, Vec::new()) };
        // (what could open, by what the session may do — made on a worker: it reads the kept answers, and the
        // places never opened are looked for)
        let basis = self.caps_basis();
        // (and the kept places its command names — its outside card's `names-kept`, as Claude's outside card's)
        let (home, here) = (crate::tools::kept_home(self), self.sandbox_here.clone());
        let mut run = run;
        self.events.jobs().spawn(move || {
            let mut list = choices_by(&run.denials, &basis.build(), run.keys.as_ref());
            list.extend(env_choices(&run.env));
            if let (Job::Command(cmd, cwd), Some(home)) = (&run.job, home) {
                let here = here.map_or_else(crate::sandbox::Here::now, |h| h());
                run.kept = crate::cardwarn::kept_named(cmd, cwd, &home, &here);
            }
            move |ed: &mut Editor| ed.blocked_open(Some(run), list)
        });
    }

    /// The list of what the sandbox blocked in `run`, `list` what could open.
    fn blocked_open(&mut self, run: Option<BlockedRun>, list: Vec<Choice>) {
        use crate::picker::{Action, Item};
        let keys = run.as_ref().and_then(|r| r.keys.as_ref());
        let mut items: Vec<Item> = list
            .iter()
            .enumerate()
            .map(|(i, c)| Item {
                label: match &c.grant {
                    Grant::Read(p) => format!("allow reading {}", crate::allow::tilde(p)),
                    Grant::Write(p) => format!("allow writing in {}", crate::allow::tilde(p)),
                    Grant::Port(n) => format!("allow port {n} on this machine"),
                    Grant::Socket(s) => format!("allow the socket {} only", crate::allow::tilde(s)),
                    Grant::Host(h, p) => format!("allow reaching {h}:{p}"),
                    Grant::Terminals => "allow opening terminals — yours too".into(),
                    Grant::Key(k) => key_label(k, keys, &self.workspace.root),
                    Grant::Localhost => "allow every port on this machine — the risky ones too".into(),
                    Grant::Env(n) => format!("allow passing your variable {n}"),
                    Grant::Masked(n, _) => format!("allow passing your variable {n} masked"),
                },
                action: Action::Grant(i.to_string()),
                hint: c.hint.clone(),
                glyph: Some(("◦", "ui.virtual")),
            })
            .collect();
        if let Some(run) = &run {
            items.push(Item {
                label: "run it outside the sandbox, once".into(),
                action: Action::Grant("outside".into()),
                hint: run.label(),
                glyph: Some(("▲", "warning")),
            });
        }
        // (9f: where tarae's AppArmor profile would raise the tier, its step to copy — after what was blocked)
        let step = self.workspace.claude.sandbox.as_ref().and_then(|g| g.step_row());
        let nothing = run.is_none();
        items.extend(step.map(|(label, hint)| Item {
            label,
            action: Action::Grant(PROFILE_STEP.into()),
            hint: hint.into(),
            glyph: Some(("◦", "ui.virtual")),
        }));
        self.workspace.blocked = run.map(|run| (run, list));
        let mut p =
            crate::picker::Picker::of("blocked", "what the sandbox blocked", items, false).without_preview();
        // (before any sandboxed run, one thing said: no header, and the list's row one says there's been none)
        p.head = self.workspace.claude.sandbox.as_ref().map(head);
        let empty = if self.workspace.claude.sandbox.is_some() { NOTHING_BLOCKED } else { NO_RUN_YET };
        // (nothing blocked but a step to copy: said where the list's first row would be, the step's row under it)
        let listed = !p.items().is_empty();
        match (&mut p.head, nothing && listed) {
            (Some(h), true) => h.note = Some(empty.into()),
            _ => p.empty = Some(empty.into()),
        }
        self.open_picker(p, None);
    }

    /// A row of the list (`Action::Grant`: a grant's number, or `outside`) — its card; the profile's step
    /// (`PROFILE_STEP`) copied, and said so (`clipboard::copy`: the system clipboard, as `space y`).
    pub fn sandbox_grant(&mut self, arg: &str) -> Result<(), String> {
        if arg == PROFILE_STEP {
            let hint = self.workspace.claude.sandbox.as_ref().and_then(|g| g.hint.as_ref());
            let step = hint.and_then(|h| h.step.clone()).ok_or("no step to copy")?;
            let what = "the step for own namespaces — run it in a terminal, then restart tarae";
            crate::clipboard::copy(self, step, what.to_string());
            return Ok(());
        }
        let (run, list) =
            self.workspace.blocked.clone().ok_or("nothing blocked to act on — space m s on a run's panel")?;
        use crate::cards::{Asked, Past, Request, Why};
        let cmd = run.label();
        let then = then_said(self, &run);
        let run = Box::new(run);
        let card = match arg.trim() {
            "outside" => {
                // (the kept places it names, under the command — the tools pack's `names-kept`, as on Claude's)
                let warned = crate::cardwarn::names_kept(&crate::sandboxpack::get().warn, &cmd, &run.kept);
                let (mut question, mut warnings, mut details) = (cmd.clone(), Vec::new(), Vec::new());
                crate::cardwarn::put_on_card(&mut question, &warned, &mut warnings, &mut details);
                question.push_str(&format!(
                    "\n\nOutside the sandbox it runs with all your permissions — your files, your network — this \
                     once. {then}"
                ));
                let request = Request::Run {
                    past: Past::Outside,
                    sandboxed: crate::cards::sandboxed(self.config.llm.sandbox, &self.sandbox),
                    reads: Vec::new(),
                    outward: false,
                };
                let ids = warned.iter().map(|w| w.id.clone()).collect();
                let asked = Asked::new(Why::Blocked, request).about(&cmd).warned(ids);
                let what = crate::offer::What::Outside { run };
                let mut o =
                    crate::offer::Offer::new(OUTSIDE_TITLE.into(), &question, "", String::new(), what);
                o = o.asked(asked).with_details(details);
                o.warnings = warnings;
                o
            }
            n => {
                let Choice { grant, why, .. } =
                    n.parse::<usize>().ok().and_then(|i| list.get(i).cloned()).ok_or("no such row")?;
                // A read: its card says first what looks like a token there — looked through on a worker
                let closed = !self.config.llm.sandbox_network;
                // A variable: what its card says of passing it masked, its value read on a worker — only the match
                // comes back (P2b: a habitual `y` here never hands out the real value either)
                if let Grant::Env(n) = &grant {
                    let (n, here) = (n.clone(), self.sandbox_here.clone());
                    self.events.jobs().spawn(move || {
                        let here = here.map_or_else(crate::sandbox::Here::now, |h| h());
                        let masking = here
                            .env
                            .get(&n)
                            .map(|v| Masking::of(v))
                            .filter(|m| m.hosts().is_some() || crate::tokens::secret_name(&n));
                        move |ed: &mut Editor| {
                            let then = then_said(ed, &run);
                            let masking = masking.map(|m| crate::tools::masking_still(ed, m));
                            let card = grant_card_with(grant, &why, &cmd, run, None, (then, closed, masking));
                            ed.push_offer(card)
                        }
                    });
                    return Ok(());
                }
                if !matches!(grant, Grant::Read(_) | Grant::Write(_)) {
                    self.push_offer(grant_card(grant, &why, &cmd, run, None, (then, closed)));
                    return Ok(());
                }
                // A place: what its grant leaves of the files named like keys in it, by the rule its run will have
                let (basis, llm, here) =
                    (self.caps_basis(), self.config.llm.clone(), self.sandbox_here.clone());
                let mut run = run;
                self.events.jobs().spawn(move || {
                    let said = match &grant {
                        Grant::Read(place) => crate::tokens::found_in(place).said(place),
                        _ => None,
                    };
                    let here = here.map_or_else(crate::sandbox::Here::now, |h| h());
                    run.granting = crate::sandbox::grant_keys(&llm, &basis.build(), &grant, &here);
                    move |ed: &mut Editor| {
                        let then = then_said(ed, &run);
                        ed.push_offer(grant_card(grant, &why, &cmd, run, said, (then, closed)))
                    }
                });
                return Ok(());
            }
        };
        self.push_offer(card);
        Ok(())
    }
}

/// The card that opens `grant` for what was blocked in `run` (`why`, in `cmd`) — warning what it opens
/// (Docker's socket; a place holding a token-like line: `tokens`); `then`: how Claude hears its end (`then_said`);
/// `closed`: the network is (a host's card says what a yes opens of its pack's caches).
fn grant_card(
    grant: Grant,
    why: &str,
    cmd: &str,
    run: Box<BlockedRun>,
    tokens: Option<String>,
    (then, closed): (&str, bool),
) -> crate::offer::Offer {
    grant_card_with(grant, why, cmd, run, tokens, (then, closed, None))
}

/// `grant_card`, a variable's with what `masking` says of passing it masked (P2b — offered: `y masked · r real
/// value · n no`, as Claude's `also` card).
fn grant_card_with(
    grant: Grant,
    why: &str,
    cmd: &str,
    run: Box<BlockedRun>,
    tokens: Option<String>,
    (then, closed, masking): (&str, bool, Option<Masking>),
) -> crate::offer::Offer {
    use crate::cards::{Asked, Request, Why};
    // (every port for what picks its ports: the pack's words, as on the card Claude's turn held — `loopback_card`)
    let said = match (&grant, crate::tools::loopback_of(&run.denials)) {
        (Grant::Localhost, Some(card)) => loopback_said(&card, cmd, Some(then)),
        _ => format!("{why}, in `{cmd}` — which runs again now with it open, whichever you choose. {then}"),
    };
    let mut asked = Asked::new(Why::Blocked, Request::Grant(grant.clone())).about(cmd);
    if let Some(said) = masked_said(&grant, masking.as_ref()) {
        asked = asked.masked(said);
    }
    grant_offer(grant, said, asked, run, tokens.into_iter().collect(), (masking, closed))
}

/// The record's `masked` for a card offering `grant` masked: its name and hosts, never the value — None: not offered.
fn masked_said(grant: &Grant, masking: Option<&Masking>) -> Option<String> {
    match (grant, masking) {
        (Grant::Env(n), Some(Masking::Offered { hosts, .. })) => Some(format!("{n} → {}", hosts.join(", "))),
        _ => None,
    }
}

/// The card for what Claude's `also` asks (cardauto P3): `grant` for `cmd`, Claude's `reason` — its call waits on
/// it (`run.claude`); `y` runs the command with it open. `asks`: the command's warnings that ask where the run
/// could reach out (D4) — on this card, since any answer but `n` runs it, as the run card's `y` would. `closed`:
/// the network is (a host's card says what a yes opens of its pack's caches).
pub fn also_card(
    grant: Grant,
    reason: &str,
    (cmd, masking): (&str, Option<Masking>),
    run: Box<BlockedRun>,
    tokens: Option<String>,
    asks: Vec<crate::cardwarn::Warned>,
    closed: bool,
) -> crate::offer::Offer {
    use crate::cards::{Asked, Request, Why};
    // (offered masked, its reason leads the card: it names the variable — #474c)
    let what = match (&grant, &masking) {
        (Grant::Env(n), Some(Masking::Offered { .. })) => format!(" for {n}"),
        _ => String::new(),
    };
    let said =
        format!("Claude asks{what}, for `{cmd}`: {reason}\n\nIt runs with it open, whichever you choose.");
    let ids = asks.iter().map(|w| w.id.clone()).collect();
    let mut asked = Asked::new(Why::Also, Request::Grant(grant.clone())).about(cmd).warned(ids);
    // (the record: the name and the hosts offered, never the value)
    if let Some(said) = masked_said(&grant, masking.as_ref()) {
        asked = asked.masked(said);
    }
    let lines = asks.iter().map(|w| format!("▲ {}", w.line()));
    let warned = tokens.into_iter().chain(lines).collect();
    grant_offer(grant, said, asked, run, warned, (masking, closed))
}

/// What the card passing a token-like variable says of passing it masked (P2b, `.sb/masking-p2b-step1.md`) —
/// worked out on the also's worker from its value (`of`), which never leaves it: only the match comes to the card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Masking {
    /// A pack knows where a token of its form goes (`Packs::secret_form`): `y` passes it masked toward `hosts`,
    /// until you quit — `still`: those of them the proxy asks on their own cards (Q3: not opened by it), `closed`
    /// why: the network is, else they're not on your host list (`tools::masking_still`).
    Offered { pack: String, prefix: &'static str, hosts: Vec<String>, still: Vec<String>, closed: bool },
    /// `masked::maskable` refused it: why, in P2a's words.
    Refused(&'static str),
    /// Maskable, but no pack knows its form: no host is guessed (D2) — the card names the setting.
    Unknown,
}

impl Masking {
    /// For a variable whose value is `value` — refused first (`maskable`), then a pack's form. A worker's.
    pub fn of(value: &std::ffi::OsStr) -> Masking {
        let bytes = value.as_encoded_bytes();
        if let Err(why) = crate::sandbox::proxy::masked::maskable(bytes) {
            return Masking::Refused(why);
        }
        // (maskable: visible ASCII — text)
        let value = std::str::from_utf8(bytes).unwrap_or_default();
        match crate::sandboxpack::get().secret_form(value) {
            Some((f, prefix)) => Masking::Offered {
                pack: f.pack.clone(),
                prefix,
                hosts: f.hosts.iter().map(ToString::to_string).collect(),
                still: Vec::new(),
                closed: false,
            },
            None => Masking::Unknown,
        }
    }

    /// The hosts a `y` passes it masked toward — None: no `y masked` on the card.
    pub fn hosts(&self) -> Option<&[String]> {
        match self {
            Masking::Offered { hosts, .. } => Some(hosts),
            _ => None,
        }
    }
}

/// `[llm.sandbox-secrets] N = ["h", …]`, as a config writes it.
fn secrets_line(n: &str, hosts: &[&str]) -> String {
    let hosts: Vec<String> = hosts.iter().map(|h| format!("\"{h}\"")).collect();
    format!("[llm.sandbox-secrets] {n} = [{}]", hosts.join(", "))
}

/// The masked card's lines for variable `n` (P2b; #474c, shorter under the card-burden rules): what's on the card, and
/// its details — folded off it, whole on `v` with the rest (`Offer::with_details`). Offered: on the card what `y` and
/// `r` do — what masked means, so whoever's tool needs the real value on this machine knows to press `r` — and the
/// hosts once, on one line, with whether they still ask; its details where the real value goes, why they still ask,
/// whose suggestion the hosts are, the setting that would keep it. A refused or unknown form: the line that says why
/// there's no `y masked`, or how to have one — no details.
fn masking_lines(n: &str, m: &Masking) -> (Vec<String>, Vec<String>) {
    let Masking::Offered { pack, prefix, hosts, still, closed } = m else {
        let line = match m {
            Masking::Refused(why) => format!("No masked answer: {why}."),
            _ => format!(
                "To pass it masked, name the hosts it may go to in your config: {}",
                secrets_line(n, &["<host>"])
            ),
        };
        return (vec![line], Vec::new());
    };
    let asks = match still.len() {
        0 => String::new(),
        k if k == hosts.len() => " — each still asks its own yes".to_string(),
        _ => format!(" — {} still ask their own yes", still.join(", ")),
    };
    // (each a row of the card at 80 columns)
    let lines = vec![
        "y masked: commands see a stand-in until you quit".to_string(),
        format!("→ {}", hosts.join(" · ")),
        format!("  get the real value{asks}"),
        "r real value: this run, for a tool that needs it".to_string(),
    ];
    let mut details = vec![
        "Masked, tarae puts the real value in only in the sign-in header toward those hosts.".to_string(),
    ];
    if !still.is_empty() {
        let why = if *closed { "the network is closed" } else { "they're not on your host list" };
        details.push(format!("They still ask: {why}."));
    }
    details.push(format!("The hosts are the {pack} pack's, for a token starting {prefix}."));
    let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
    details.push(format!("To keep it masked, in your config: {}", secrets_line(n, &hosts)));
    (lines, details)
}

/// Gradle's loopback (piece D2): Claude's `cmd` failed because a program the packs say picks its ports as it
/// starts was refused one (`refused-port`) — every port's card at once, in the pack's words (`card`), Claude's
/// call waiting on it (`run.claude`) with the run's report (`run.report`): any answer but `n` runs it again with
/// them open, as Claude's run; `n` gives Claude the report.
pub fn loopback_card(card: &str, cmd: &str, run: Box<BlockedRun>) -> crate::offer::Offer {
    use crate::cards::{Asked, Request, Why};
    let said = loopback_said(card, cmd, None);
    let asked = Asked::new(Why::Blocked, Request::Grant(Grant::Localhost)).about(cmd);
    // (no host: the network's state says nothing on it)
    grant_offer(Grant::Localhost, said, asked, run, Vec::new(), (None, false))
}

/// Gradle's loopback card's words, Claude's turn holding it or not: the pack's `card`, then what failed and runs
/// again — Claude waiting on it (`then` None), or how it hears the end (`then_said`; the panel's card).
fn loopback_said(card: &str, cmd: &str, then: Option<&str>) -> String {
    let (waits, then) = match then {
        None => (" and waits", String::new()),
        Some(t) => ("", format!(" {t}")),
    };
    format!(
        "{card}\n\nClaude's `{cmd}` failed for it{waits}: it runs again now with them open, whichever you choose.{then}"
    )
}

/// A grant's card: what it lets, `said` (why, and what runs), how long it lasts, what it warns of (the grant's
/// risk, then `warned`: what looks like a token where it reads, the command's own). A host's: its pack's registry
/// and what a yes lets in with it, and what's read can go there (the caches too, the network `closed` —
/// `hosts::grant_said`). A token-like variable's: what `masking` says of passing it masked — offered, its
/// answers are `y masked · r real value · n no`, its risk `r`'s.
fn grant_offer(
    grant: Grant,
    said: String,
    asked: crate::cards::Asked,
    run: Box<BlockedRun>,
    warned: Vec<String>,
    (masking, closed): (Option<Masking>, bool),
) -> crate::offer::Offer {
    let (registry, host_risk) = match &grant {
        Grant::Host(h, p) => {
            let (line, warn) = crate::hosts::grant_said(h, *p, closed);
            (line, Some(warn))
        }
        _ => (None, None),
    };
    // (as far as it may be kept: a socket this conversation's at most, a key and Docker's socket this run's —
    // everything else stays in the sandbox)
    let lasts = match (&grant, crate::caps::Caps::storable(&grant)) {
        (Grant::Host(..), _) => "This chat lasts until the conversation ends, this session until you quit.",
        (_, Scope::Always) => "s lasts until this session ends, A until :granted takes it back.",
        (_, Scope::Session) => {
            "This chat lasts until the conversation ends, this session until you quit — a name for a secret is never \
             written."
        }
        (_, Scope::ThisChat) => {
            "Everything else stays in the sandbox; it's never kept past this conversation."
        }
        (_, Scope::Once) => "Everything else stays in the sandbox; it's never kept past this run.",
    };
    let registry = registry.map(|r| format!("\n{r}")).unwrap_or_default();
    let masked = masking.as_ref().and_then(|m| m.hosts().map(<[String]>::to_vec));
    // (the masked card's details — its asides, after its warning: never on the card, whole on `v`)
    let mut details = Vec::new();
    let mut question = match (&grant, &masking) {
        // (offered: Claude's reason first — it names the variable —, then its answers, each saying how long it lasts;
        // what's left of what it said, what it lets, and the rest are its details)
        (Grant::Env(n), Some(m)) if masked.is_some() => {
            let (lead, rest) = said.split_once("\n\n").unwrap_or((&said, ""));
            let (lines, more) = masking_lines(n, m);
            let mut aside =
                vec![format!("Claude's commands {}. {}", grant.shown(), rest.trim()).trim_end().to_string()];
            aside.extend(more);
            details = aside;
            format!("{lead}\n\n{}", lines.join("\n"))
        }
        (Grant::Env(n), Some(m)) => {
            let line = masking_lines(n, m).0.join("\n");
            format!("Let Claude's commands {}\n\n{said} {lasts}\n{line}", grant.shown())
        }
        _ => format!("Let Claude's commands {}{registry}\n\n{said} {lasts}", card_grant(&grant, &run)),
    };
    if let Some(from) = &run.from {
        question.push_str(&format!("\n\nWhat it refused was read {from}."));
    }
    let risk = match masked {
        Some(_) => Some("With r its commands read its value, and what they print reaches Claude.".into()),
        None => grant.risk(),
    };
    let warn: Vec<String> = risk.into_iter().chain(host_risk).chain(warned).collect();
    let warning = (!warn.is_empty()).then(|| {
        question.push_str("\n\n");
        let at = question.split('\n').count() - 1;
        question.push_str(&warn.join("\n"));
        at..at + warn.len()
    });
    // (the blank before them folded with them)
    let details: Vec<std::ops::Range<usize>> = (!details.is_empty())
        .then(|| {
            question.push_str("\n\n");
            let at = question.split('\n').count() - 2;
            question.push_str(&details.join("\n"));
            at..at + 1 + details.len()
        })
        .into_iter()
        .collect();
    let what = crate::offer::What::Grant { grant, run, masked };
    let o = crate::offer::Offer::new(GRANT_TITLE.into(), &question, "", String::new(), what)
        .with_details(details);
    match warning {
        Some(lines) => o.warning(lines).asked(asked),
        None => o.asked(asked),
    }
}

/// The panel's row for the AppArmor profile's step (`Action::Grant`'s argument).
const PROFILE_STEP: &str = "profile";

/// The panel's list when the run meant had nothing blocked (or there's none).
const NOTHING_BLOCKED: &str = "the sandbox blocked nothing in that run";

/// The panel's list before the session's first sandboxed run — the one thing it says then (no header).
const NO_RUN_YET: &str = "no sandboxed run yet in this session";

/// The panel's state line (9e; the user's decision 14: here and `:procs`, never the status line): what the session's
/// last sandboxed run was kept apart from and shared (`Guarantees::panel_pairs`, folded by `Guarantees::FOLD` when
/// narrow), why not the top tier in a few words faint under it (`Guarantees::brief`; the whole `why` is the report's
/// and `tarae state`'s), and under that how the tier would go up (9f, `Guarantees::hint_brief`: the profile's).
fn head(g: &crate::sandbox::guarantees::Guarantees) -> crate::picker::Head {
    use crate::sandbox::guarantees::Guarantees;
    crate::picker::Head {
        pairs: g.panel_pairs(),
        fold: Guarantees::FOLD,
        faint: g.brief.clone(),
        then: g.hint_brief(),
        note: None,
    }
}

pub const GRANT_TITLE: &str = "Open the sandbox a little?";
pub const OUTSIDE_TITLE: &str = "Run it outside the sandbox?";

/// The answer on a grant's card: None = no; else `grant` is allowed for `scope` and the run goes again now
/// with it — the user's run, its end told to Claude.
pub fn grant_answer(ed: &mut Editor, id: u64, grant: Grant, mut run: BlockedRun, scope: Option<Scope>) {
    ed.offer_finished(id, Ok(()));
    // Claude's `also`: its call runs with it open — or hears no, which sticks for the chat
    if let Some(call) = run.claude.take() {
        // (its turn over: nobody waits — nothing kept, no refusal recorded, nothing runs)
        let waits = |c: &crate::chat::Chat| c.generation() == call.generation() && c.busy();
        if !ed.workspace.claude.chat.as_ref().is_some_and(waits) {
            return;
        }
        let Some(scope) = scope else {
            if let Some(c) = ed.workspace.claude.chat.as_mut() {
                c.also_refused.push(grant.clone());
            }
            let no = refused_said(&grant, &run);
            // (a card tarae raised on the run's refusal: Claude hears how the run ended, then the no)
            return match run.report.take() {
                Some(report) => crate::tools::answer(ed, call, Ok(format!("{report}\n{no}\n"))),
                None => crate::tools::answer(ed, call, Err(no)),
            };
        };
        match &grant {
            // (C2: a host is the proxy's to let through — as the user's own choice below)
            Grant::Host(h, p) => crate::hosts::granted(ed, h, *p, scope),
            // (the masked card's `y`: its stamp the key that gave it)
            Grant::Masked(..) => crate::allow::grant_by(ed, &grant, scope, 'y', None),
            // (told as the card said it: what the place's run leaves of the keys in it)
            _ => crate::allow::grant_by(ed, &grant, scope, scope.key(), run.granting),
        }
        // (a yes on the terminals: asked again this turn, it takes no more of the turn's `also` cards — frictions29
        // 23, the user's interim decision 2026-10-19)
        if let Some(c) = ed.workspace.claude.chat.as_mut().filter(|_| grant == Grant::Terminals) {
            c.also_yes = true;
        }
        // (the answer rides along whatever its scope, as the user's own run below: a failed take-back of it hides it
        // from the store's answers until the `A`'s write lands)
        let extra = Extra { grant: Some(grant), outside: false, base: None };
        return crate::tools::guard_and_go(ed, For::Claude(call), run.job, false, extra);
    }
    let Some(scope) = scope else {
        return ed.set_status("nothing opened — the sandbox stays as it was");
    };
    match &grant {
        // (a host is the proxy's to let through — and the session's, not the conversation's)
        Grant::Host(h, p) => crate::hosts::granted(ed, h, *p, scope),
        // (the panel's masked `y`, as Claude's card's)
        Grant::Masked(..) => crate::allow::grant_by(ed, &grant, scope, 'y', None),
        _ => crate::allow::grant_by(ed, &grant, scope, scope.key(), run.granting),
    }
    let head = rerun_head(&grant, &run, scope);
    rerun_of(ed, &run, &head);
    // (the answer rides along whatever its scope: an `A` with no chat open is the store's only once written, and
    // a failed take-back of it hides it until then — the run again is of the answer just given)
    let extra = Extra { grant: Some(grant), outside: false, base: None };
    crate::tools::guard_and_go(ed, For::User(head), run.job, true, extra);
}

/// What a grant's card offered, as Claude is told the user's answer — in the card's words (`card_grant`).
pub(crate) fn refused_said(grant: &Grant, run: &BlockedRun) -> String {
    format!(
        "The user said no to letting your commands {} — for the rest of this chat: don't ask for it again. If the \
         command can't work without it, tell the user what it needs.",
        card_grant(grant, run)
    )
}

/// What Claude hears first of the user's run again from a grant's card (`scope` its answer's).
pub(crate) fn rerun_head(grant: &Grant, run: &BlockedRun, scope: Scope) -> String {
    format!(
        "The user ran `{}` again, letting it {}{}. How it ended:",
        run.label(),
        card_grant(grant, run),
        if scope == Scope::Once { " this once" } else { "" }
    )
}

/// A grant as its card says it — the card's line, Claude's answer, the reviewer's record: a key in the project named
/// from the root of the run's key rule (`Grant::shown_from`), a place with what that rule leaves of the keys in it.
pub(crate) fn card_grant(grant: &Grant, run: &BlockedRun) -> String {
    grant.shown_from(run.keys.as_ref().map(|k| k.root()), run.granting.unwrap_or_default())
}

/// The user's run again of `run` is told with `head` as it ends (`tools::told`): which run it was, for whether it
/// goes on to Claude by itself.
fn rerun_of(ed: &mut Editor, run: &BlockedRun, head: &str) {
    if let (Some(key), Some(c)) = (run.key, ed.workspace.claude.chat.as_mut()) {
        c.reruns.retain(|(h, _)| h != head);
        c.reruns.push((head.to_string(), key));
    }
}

/// What a card that runs Claude's command again says of how Claude hears its end (piece D3): by itself now, or
/// with the next message — by the state as the card is made; the end looks again (`tools::told`).
pub(crate) fn then_said(ed: &Editor, run: &BlockedRun) -> &'static str {
    match run.key.is_some_and(|k| crate::tools::goes_on(ed, k)) {
        true => GOES_ON,
        false => NEXT_MESSAGE,
    }
}

pub const GOES_ON: &str = "Claude goes on when it ends.";
pub const NEXT_MESSAGE: &str = "Claude hears how it ended with your next message.";

/// The answer on the run-outside card: `y` runs it now, outside the sandbox — the user's run.
pub fn outside_answer(ed: &mut Editor, id: u64, run: BlockedRun, yes: bool) {
    ed.offer_finished(id, Ok(()));
    if !yes {
        return ed.set_status("not run — the sandbox stays as it was");
    }
    let head = format!("The user ran `{}` again outside the sandbox, this once. How it ended:", run.label());
    rerun_of(ed, &run, &head);
    crate::tools::guard_and_go(
        ed,
        For::User(head),
        run.job,
        true,
        Extra { grant: None, outside: true, base: None },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keys as the main loop takes them (`<ret>` by name).
    fn feed(ed: &mut Editor, keys: &str) {
        let mut chars = keys.chars();
        while let Some(c) = chars.next() {
            let key: crate::key::Key = match c {
                '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
                c => crate::key::Key::plain(crate::key::Code::Char(c)),
            };
            ed.handle_key(key);
        }
    }

    /// 9f: where tarae's AppArmor profile would raise the tier, its step is a row of the panel — the list's one row
    /// when the run blocked nothing, which is still said where the first row would be — and Enter on it copies the
    /// whole step to the system clipboard, said with what to do next. Without a hint (Seatbelt) no such row: the
    /// list is empty and says why, as before.
    #[test]
    fn the_profiles_step_is_a_row_enter_copies() {
        use crate::sandbox::guarantees::{Found, Guarantees, Hint, Ns, Tier};
        let place = crate::sandbox::apparmor::Place {
            path: "/usr/local/bin/tarae".into(),
            root_owned: Ok(()),
            packaged: false,
        };
        let hint = Hint::of(Tier::Host, &place);
        let step = hint.step.clone().expect("a root-owned plain path: a step");
        let mut ed = Editor::new(crate::config::Config::default());
        ed.workspace.claude.sandbox = Some(Guarantees::landlock(Found {
            ns: Ns::NONE,
            served: Ok(()),
            abi: 4,
            ptys: Some(4),
            lo_missing: Vec::new(),
            short: None,
            brief: Some("AppArmor blocks user namespaces".into()),
            hint: Some(hint),
            network: false,
            bridged: false,
        }));
        ed.sandbox_blocked(Which::Either);
        let p = ed.picker.as_ref().expect("the panel");
        let rows: Vec<(&str, &str)> = p.items().iter().map(|i| (i.label.as_str(), i.hint.as_str())).collect();
        assert_eq!(rows, [("copy the step for own namespaces", "needs sudo, then restart tarae")]);
        let head = p.head.as_ref().unwrap();
        assert_eq!(head.then.as_deref(), Some("tarae's AppArmor profile would give own namespaces"));
        assert_eq!((head.note.as_deref(), &p.empty), (Some(NOTHING_BLOCKED), &None), "said above the row");
        feed(&mut ed, "<ret>");
        let end = std::time::Instant::now() + crate::test_results::PATIENCE;
        let said = "copied the step for own namespaces — run it in a terminal, then restart tarae";
        while !ed.toasts.iter().any(|t| t.text == said) {
            let left = end.saturating_duration_since(std::time::Instant::now());
            let ev = ed.events.recv_timeout(left).unwrap_or_else(|| panic!("{said:?} before the timeout"));
            ed.handle_event(ev);
        }
        assert!(crate::clipboard::written(&step), "the whole step copied: {step}");
        // (Seatbelt's: no tiers, no step — the list says the run blocked nothing in its first row)
        ed.picker = None;
        ed.workspace.claude.sandbox = Some(Guarantees::seatbelt(false, false));
        ed.sandbox_blocked(Which::Either);
        let p = ed.picker.as_ref().expect("the panel");
        assert!(p.items().is_empty() && p.head.as_ref().is_some_and(|h| h.note.is_none()));
        assert_eq!(p.empty.as_deref(), Some(NOTHING_BLOCKED));
    }

    /// Review of #212: a run on a commit's tree isn't offered to grant or run again outside — its tree is gone
    /// (or another run's now), so either would run it where it no longer is; said instead.
    #[test]
    fn a_run_on_a_commits_tree_is_not_offered_again() {
        let mut ed = Editor::new(crate::config::Config::default());
        let tree = std::env::temp_dir().join(format!("tarae-blocked-base-{}", std::process::id()));
        let task =
            crate::task::Task { name: "cargo test".into(), cmd: "cargo test".into(), cwd: tree.clone() };
        let mut r = crate::task::Run::for_test(task);
        r.base = Some(crate::basetree::Base { rev: "HEAD".into(), tree, ..Default::default() });
        ed.workspace.task_run = Some(r);
        ed.sandbox_blocked(Which::Task);
        assert!(ed.picker.is_none());
        let said = ed.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
        assert!(said.starts_with("that run was on a commit's tree"), "{said}");
    }

    /// What a run's denials offer: the folders a write may open, a port that isn't a risky one — once each. A
    /// connection past the proxy (`network-direct`: ssh, a database client) offers no grant: the list's last row, a
    /// run outside once, is its one way (`the_user_opens_what_the_sandbox_blocked`).
    #[test]
    fn what_a_runs_denials_offer() {
        let base = crate::scratch::Scratch::new("blocked");
        std::fs::create_dir_all(base.join("home/data/cache")).unwrap();
        std::fs::create_dir_all(base.join("home/proj")).unwrap();
        let home = std::fs::canonicalize(base.join("home")).unwrap();
        let root = home.join("proj");
        let d = |op: &str, target: String| Denial { process: "npm".into(), op: op.into(), target };
        let denials = [
            d("file-write-create", home.join("data/cache/x").display().to_string()),
            d("file-write-data", home.join("data/cache/y").display().to_string()),
            d("network-outbound", "remote:*:5432".into()),
            d("network-outbound", "remote:*:2375".into()),
            d("file-write-create", root.join(".git/index.lock").display().to_string()),
            d("mach-lookup", "com.apple.lsd.mapdb".into()),
            // (Linux's supervisor names the address: a port on this machine is offered, one beyond isn't)
            d("network-outbound", "remote:[::1]:8080".into()),
            d("network-outbound", "remote:192.0.2.1:9".into()),
            d(crate::sandbox::DIRECT_OP, "remote:192.0.2.7:22".into()),
        ];
        let all = choices(&denials, &crate::caps::Caps::at(&home, &root));
        assert_eq!((all[0].hint.as_str(), all[1].hint.as_str()), ("…/x", "…/cache/x"), "below the folder");
        assert_eq!(all[2].why, "npm was connecting to it");
        let got: Vec<Grant> = all.into_iter().map(|c| c.grant).collect();
        assert_eq!(
            got,
            [
                Grant::Write(home.join("data/cache")),
                Grant::Write(home.join("data")),
                Grant::Port(5432),
                Grant::Localhost,
                Grant::Port(8080)
            ],
            "Docker's port, an address beyond this machine, a connection past the proxy and the project's .git \
             aren't offered; each folder once and every port here once, after the first port"
        );
    }

    /// A read or a write refused at a folder (a listing, a folder removed) offers that folder first — never
    /// only the one above it, which may hold other projects — then the wider ones, as a file's do.
    #[test]
    fn a_folder_refused_offers_itself_first() {
        let base = crate::scratch::Scratch::new("blocked-folder");
        std::fs::create_dir_all(base.join("home/Workspace/sandbox-runtime")).unwrap();
        std::fs::create_dir_all(base.join("home/proj")).unwrap();
        let home = std::fs::canonicalize(base.join("home")).unwrap();
        let srt = home.join("Workspace/sandbox-runtime");
        let d = |op: &str| Denial { process: "ls".into(), op: op.into(), target: srt.display().to_string() };
        let caps = crate::caps::Caps::at(&home, &home.join("proj"));
        let all = choices(&[d("file-read-data"), d("file-write-unlink")], &caps);
        let got: Vec<Grant> = all.into_iter().map(|c| c.grant).collect();
        let workspace = home.join("Workspace");
        assert_eq!(
            got,
            [
                Grant::Read(srt.clone()),
                Grant::Read(workspace.clone()),
                Grant::Write(srt.clone()),
                Grant::Write(workspace)
            ]
        );
    }

    /// A terminal refused (macOS, 3c — reading or writing `/dev/ttysN`) offers the terminals, once, whichever
    /// and however many: never `/dev` nor the file itself for writing. Its card says what yes risks — though
    /// not as much as outside, so a chat's `a` and the project's `A` take it — and the project keeps it
    /// (`sandbox.json`'s `terminals`) until `:sandbox-disallow terminals` takes it back.
    #[test]
    fn a_terminal_refused_offers_the_terminals_once() {
        let d = |op: &str, target: &str| Denial {
            process: "script".into(),
            op: op.into(),
            target: target.into(),
        };
        let denials = [
            d("file-write-data", "/dev/ttys012"),
            d("file-read-data", "/dev/ttys012"),
            d("file-read-data", "/dev/ttys003"),
            d("file-write-create", "/dev/ttysq"),
            // (not an open: no grant offered, nor `/dev` to write in)
            d("file-write-unlink", "/dev/ttys005"),
            d("file-write-mode", "/dev/ttys005"),
        ];
        let all = choices(&denials, &crate::caps::Caps::at(Path::new("/Users/u"), Path::new("/Users/u/p")));
        let got: Vec<(&Grant, &str)> = all.iter().map(|c| (&c.grant, c.why.as_str())).collect();
        assert_eq!(got, [(&Grant::Terminals, "script was opening /dev/ttys012")]);
        let g = Grant::Terminals;
        assert!(g.keepable() && !g.as_outside());
        assert!(g.risk().is_some_and(|r| r.contains("read what you type")), "{:?}", g.risk());
        let base = crate::scratch::Scratch::new("blocked-tty");
        let store = base.join("sandbox.json");
        crate::allow::keep(&store, Path::new("/p"), |a| a.add(&g)).unwrap();
        let kept = crate::allow::Store::read(Some(&store)).get(Path::new("/p"));
        assert!(kept.terminals && !kept.is_empty());
        let w = crate::tools::with_allowed(
            &crate::llm::LlmConfig::default(),
            &crate::caps::Caps::answered(&[&kept]),
            None,
        );
        assert!(w.terminals && w.granted == [g.shown()], "{:?}", w.granted);
        crate::allow::keep(&store, Path::new("/p"), |a| a.remove(crate::allow::TERMINALS_LISTED)).unwrap();
        assert_eq!(crate::allow::Store::read(Some(&store)).get(Path::new("/p")), Default::default());
    }

    /// A port refused here offers that port, then every port; one a pack's `refused-port` program picks as it
    /// starts (Gradle's `java`) only every port — the one it was refused is picked anew next time — and that card
    /// says the pack's words, as the card Claude's turn held does, ending with how Claude hears the end. Every
    /// port's card names the risky ones it opens too. A variable the output names offers it by name — never one no
    /// card passes, and its value is on no card.
    #[test]
    fn a_port_offers_every_port_too_and_a_variable_its_name_only() {
        let caps = crate::caps::Caps::at(Path::new("/Users/u"), Path::new("/Users/u/p"));
        let port = |process: &str, n: u16| Denial {
            process: process.into(),
            op: "network-outbound".into(),
            target: format!("remote:*:{n}"),
        };
        let grants = |d: Denial| choices(&[d], &caps).into_iter().map(|c| c.grant).collect::<Vec<_>>();
        assert_eq!(grants(port("psql", 5432)), [Grant::Port(5432), Grant::Localhost]);
        assert_eq!(grants(port("java", 52113)), [Grant::Localhost], "Gradle's: one port won't do");
        let gradle = BlockedRun {
            denials: vec![port("java", 52113)],
            ..BlockedRun::of(Job::Command("bin/gradle build".into(), PathBuf::from("/Users/u/p")))
        };
        let c = choices(&gradle.denials, &caps).remove(0);
        let card =
            grant_card(c.grant, &c.why, "bin/gradle build", Box::new(gradle.clone()), None, (GOES_ON, true));
        let held = loopback_card(&crate::tools::loopback_of(&gradle.denials).unwrap(), "bin/gradle build", {
            Box::new(gradle)
        });
        let (q, h) = (&card.question, &held.question);
        assert!(q.contains("one port won't do") && !q.contains("connecting to it"), "{q}");
        let tail = "Claude's `bin/gradle build` failed for it: it runs again now with them open, whichever you choose. \
                    Claude goes on when it ends.";
        assert!(q.contains(tail), "{q}");
        assert_eq!(q.split("failed for it").next(), h.split("failed for it").next(), "the held card's words");
        let risky = Grant::Localhost.risk().unwrap();
        for (n, _) in crate::sandbox::RISKY_PORTS {
            assert!(risky.contains(&n.to_string()), "{n}: {risky}");
        }
        assert!(Grant::Localhost.keepable() && !Grant::Localhost.as_outside());
        let docker =
            Denial { process: "curl".into(), op: "network-outbound".into(), target: "remote:*:2375".into() };
        assert!(choices(&[docker], &caps).is_empty(), "a risky port: by hand only");
        // (a value tarae has, as a run's environment holds it — a fake token)
        let value = "tarae-fake-token-4f9a1c";
        let env = crate::sandbox::runenv::RunEnv {
            vars: Vec::new(),
            tokens: vec!["MY_API_TOKEN".into()],
            others: vec!["RUSTFLAGS".into(), "DATABASE_URL".into()],
            ..Default::default()
        };
        let said = format!("MY_API_TOKEN={value} refused; RUSTFLAGS and DATABASE_URL unset");
        let named = env.named(&[(true, said)]);
        let list = env_choices(&named);
        let got: Vec<&Grant> = list.iter().map(|c| &c.grant).collect();
        assert_eq!(got, [&Grant::Env("MY_API_TOKEN".into()), &Grant::Env("DATABASE_URL".into())]);
        let run = BlockedRun {
            env: named,
            ..BlockedRun::of(Job::Command("psql".into(), PathBuf::from("/Users/u/p")))
        };
        let c = list[0].clone();
        let card = grant_card(c.grant, &c.why, "psql", Box::new(run), None, (NEXT_MESSAGE, true));
        let text = card.question.clone();
        assert!(text.contains("MY_API_TOKEN") && !text.contains(value), "{text}");
        assert!(text.contains("this session until you quit") && !text.contains("A until"), "{text}");
    }

    /// A mount refused (Linux, Landlock: `MOUNT_OP`) offers nothing — no grant opens a mount, so the panel offers no
    /// write where one was refused, though a write refused at that same folder would offer it; the run outside is
    /// the way, as for what no grant opens. `choices_by` offered nothing for it before too: this keeps a mount out of
    /// the write choices; that Landlock's record of one reaches here as a mount is pinned on Linux
    /// (`an_audit_records_topology_change_is_a_mount`).
    #[test]
    fn a_mount_refused_offers_no_grant() {
        let base = crate::scratch::Scratch::new("blocked-mount");
        std::fs::create_dir_all(base.join("home/mnt")).unwrap();
        std::fs::create_dir_all(base.join("home/proj")).unwrap();
        let home = std::fs::canonicalize(base.join("home")).unwrap();
        let at = home.join("mnt");
        let d =
            |op: &str| Denial { process: "bwrap".into(), op: op.into(), target: at.display().to_string() };
        let caps = crate::caps::Caps::at(&home, &home.join("proj"));
        assert!(!choices(&[d("file-write*")], &caps).is_empty(), "a write there would offer the folder");
        assert!(choices(&[d(crate::sandbox::MOUNT_OP)], &caps).is_empty());
        let empty = Denial { target: String::new(), ..d(crate::sandbox::MOUNT_OP) };
        assert!(choices(&[empty], &caps).is_empty(), "nor one at no place");
        let userns = Denial { target: String::new(), ..d(crate::sandbox::USERNS_OP) };
        assert!(choices(&[d(crate::sandbox::USERNS_OP), userns], &caps).is_empty(), "nor a user namespace");
    }

    /// A file named like a key refused in the project (macOS, 7a) offers that one file, once — never a folder of
    /// it (the project is read anyway), never kept past this run (`Caps::storable`, cardauto 6); kept by an
    /// earlier build's `A`, it reaches the run (`with_allowed`) and `:sandbox-disallow` takes it back. One outside
    /// the project goes by the run's own rule (`choices_by`); with none, home's rules (its folders).
    #[test]
    fn a_key_named_file_refused_offers_that_file() {
        let d = |target: &str| Denial {
            process: "node".into(),
            op: "file-read-data".into(),
            target: target.into(),
        };
        let (home, root) = (Path::new("/Users/u"), Path::new("/Users/u/p"));
        let env = root.join("app/.env");
        let all = choices(
            &[d("/Users/u/p/app/.env"), d("/Users/u/p/app/.env")],
            &crate::caps::Caps::at(home, root),
        );
        let got: Vec<(&Grant, &str, &str)> =
            all.iter().map(|c| (&c.grant, c.hint.as_str(), c.why.as_str())).collect();
        let g = Grant::Key(env.clone());
        // (named from the project, as the run's report names it)
        let why = "node was reading app/.env, named like a key";
        assert_eq!(got, [(&g, "node reading · named like a key", why)]);
        // (outside the project, by the run's own rule: `sandbox::tests::every_user_of_the_key_rule_gives_one_answer`;
        // with none, the project's only)
        let outside = choices(&[d("/Users/u/q/.env")], &crate::caps::Caps::at(home, root));
        assert!(outside.iter().all(|c| matches!(c.grant, Grant::Read(_))), "{outside:?}");
        assert!(!g.keepable() && !g.as_outside() && g.risk().is_some_and(|r| r.contains("reaches Claude")));
        let base = crate::scratch::Scratch::new("blocked-key");
        let store = base.join("sandbox.json");
        crate::allow::keep(&store, root, |a| a.add(&g)).unwrap();
        let kept = crate::allow::Store::read(Some(&store)).get(root);
        assert_eq!(kept.keys, std::slice::from_ref(&env));
        let w = crate::tools::with_allowed(
            &crate::llm::LlmConfig::default(),
            &crate::caps::Caps::answered(&[&kept]),
            None,
        );
        assert!(w.keys == [env.clone()] && w.granted == [g.shown()] && w.reads.is_empty(), "{:?}", w.granted);
        crate::allow::keep(&store, root, |a| a.remove(&env.display().to_string())).unwrap();
        assert_eq!(crate::allow::Store::read(Some(&store)).get(root), Default::default());
    }

    /// A file named like a key the run was refused changing (macOS, B1 — `policy::KEY_WRITES`: written, removed,
    /// its mode, flags, ACL or owner) offers that one file, said as what it was doing — never a folder of it: the name
    /// closed it, not the place. Its creation refused is the place's (B1 leaves `file-write-create` open), and a
    /// write of a file not named like one stays the folder's.
    #[test]
    fn a_key_named_files_change_refused_offers_that_file() {
        let d =
            |op: &str, target: &str| Denial { process: "sh".into(), op: op.into(), target: target.into() };
        let (home, root) = (Path::new("/Users/u"), Path::new("/Users/u/p"));
        let caps = crate::caps::Caps::at(home, root);
        let env = root.join("app/.env");
        let g = Grant::Key(env.clone());
        for (op, doing) in [
            ("file-write-data", "writing"),
            ("file-write-unlink", "deleting"),
            ("file-write-mode", "changing"),
            ("file-write-acl", "changing"),
            ("file-write-owner", "changing"),
        ] {
            let all = choices(&[d(op, "/Users/u/p/app/.env")], &caps);
            let got: Vec<(&Grant, &str, &str)> =
                all.iter().map(|c| (&c.grant, c.hint.as_str(), c.why.as_str())).collect();
            let why = format!("sh was {doing} app/.env, named like a key");
            let hint = format!("sh {doing} · named like a key");
            assert_eq!(got, [(&g, hint.as_str(), why.as_str())], "{op}");
        }
        let made = choices(&[d("file-write-create", "/Users/u/p/app/.env")], &caps);
        assert!(made.iter().all(|c| !matches!(c.grant, Grant::Key(_))), "{made:?}");
        let plain = choices(&[d("file-write-data", "/Users/u/p/app/notes.txt")], &caps);
        assert!(plain.iter().all(|c| !matches!(c.grant, Grant::Key(_))), "{plain:?}");
    }

    /// A host the proxy held and the user's answer would lift (`network-host`: not among the hosts, no answer,
    /// refused) is offered — that host at that port, once; what the proxy's other checks refused (an address
    /// never reached, a server name not the host's, `network-proxy`) never is: no grant makes those safe.
    #[test]
    fn a_host_refusal_offers_that_host_only() {
        let d =
            |op: &str, target: &str| Denial { process: "curl".into(), op: op.into(), target: target.into() };
        let host = crate::sandbox::proxy::HOST_OP;
        let denials = [
            d(host, "static.example.invalid:443: the user refused it for this session"),
            d(host, "static.example.invalid:443: 8 hosts already wait for the user's answer"),
            d(
                host,
                "api.example.invalid:8443: not among the hosts it may reach (llm.sandbox-hosts); the user was asked and didn't answer within 20 s",
            ),
            d(
                crate::sandbox::proxy::OP,
                "meta.example.invalid:443: it resolves only to addresses never reached: 169.254.169.254",
            ),
            d(
                crate::sandbox::proxy::OP,
                "sni.example.invalid:443: its ClientHello names other.example.invalid — the tunnel closed",
            ),
        ];
        let all = choices(&denials, &crate::caps::Caps::at(Path::new("/Users/u"), Path::new("/Users/u/p")));
        assert_eq!((all[0].hint.as_str(), all[0].why.as_str()), ("curl reaching", "curl was reaching it"));
        let got: Vec<Grant> = all.into_iter().map(|c| c.grant).collect();
        assert_eq!(
            got,
            [
                Grant::Host("static.example.invalid".into(), 443),
                Grant::Host("api.example.invalid".into(), 8443)
            ]
        );
    }

    /// A toolchain's shared cache is never offered for writing, in a project that isn't the tool's either (no
    /// `go.mod` here, so nothing asked Go where its module cache is): not Go's module cache at its default, nor
    /// cargo's registry, nor what holds them — and no folder the packs' `ask` could name there opens. Written
    /// only through the caches rule, with the network open. Nor npm's home (`npx` runs what's there). (A
    /// stand-in home in the test's temp folder.)
    #[test]
    fn a_shared_cache_is_never_offered_in_a_project_not_the_tools() {
        let base = crate::scratch::Scratch::new("blocked-cache");
        let modcache = "go/pkg/mod/golang.org/x/text@v0.1.0";
        for d in [
            modcache,
            "go/pkg/mod/cache/download",
            ".cargo/registry/src/i/serde-1.0.0",
            ".npm/_cacache",
            ".npm/_npx/0a1b",
            "data/cache",
            "proj",
        ] {
            std::fs::create_dir_all(base.join("home").join(d)).unwrap();
        }
        let home = std::fs::canonicalize(base.join("home")).unwrap();
        let root = home.join("proj");
        assert!(!root.join("go.mod").exists());
        let d = |target: PathBuf| Denial {
            process: "x".into(),
            op: "file-write-create".into(),
            target: target.display().to_string(),
        };
        let denials = [
            d(home.join(modcache).join("new.go")),
            d(home.join("go/pkg/mod/cache/download/x.zip")),
            d(home.join(".cargo/registry/src/i/serde-1.0.0/build.rs")),
            d(home.join(".npm/_cacache/x")),
            d(home.join(".npm/_npx/0a1b/x.js")),
            d(home.join("data/cache/x")),
        ];
        let got: Vec<Grant> =
            choices(&denials, &crate::caps::Caps::at(&home, &root)).into_iter().map(|c| c.grant).collect();
        assert_eq!(
            got,
            [Grant::Write(home.join("data/cache")), Grant::Write(home.join("data"))],
            "only the folder no toolchain keeps"
        );
        // …nor as a folder the toolchain names, this project's policy holding no write there
        let here = crate::sandbox::Here {
            env: [("HOME".to_string(), home.clone().into())].into(),
            ..Default::default()
        };
        let llm = crate::llm::LlmConfig::default();
        crate::sandbox::toolchain_found_in(&root, Default::default(), &here);
        let env = crate::sandbox::env_in(&root, &llm, &[], &here).0;
        for d in [home.join(modcache), home.join("go/pkg/mod"), home.join(".cargo/registry")] {
            let why = crate::sandbox::ask_dir_verdict(&d, &root, Some(&home), &env.runs_later).unwrap_err();
            assert!(why.contains("a toolchain's shared cache"), "{}: {why}", d.display());
        }
        let why =
            crate::sandbox::ask_dir_verdict(&home.join(".npm/_npx"), &root, Some(&home), &env.runs_later);
        assert!(why.as_ref().is_err_and(|w| w.starts_with("it's in a toolchain's home")), "{why:?}");
        let p = crate::sandbox::Policy::for_project(&env);
        assert!(!p.writes_allow(&home.join(modcache).join("new.go")), "{:?}", p.write);
    }

    /// Card model P2: a read the sandbox denied in home offers the places it may open, nearest first — never
    /// a key's; one only the output said (reading or writing — it doesn't say which) offers both.
    #[test]
    fn what_a_denied_read_offers() {
        let base = crate::scratch::Scratch::new("blocked-read");
        for d in ["home/src/lib", "home/.ssh", "home/notes", "home/proj"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        let home = std::fs::canonicalize(base.join("home")).unwrap();
        let d = |op: &str, target: PathBuf| Denial {
            process: "cat".into(),
            op: op.into(),
            target: target.display().to_string(),
        };
        let denials = [
            d("file-read-data", home.join("src/lib/a.rs")),
            d("file-read-data", home.join(".ssh/id_ed25519")),
            d(crate::sandbox::EITHER, home.join("notes/x")),
        ];
        let all = choices(&denials, &crate::caps::Caps::at(&home, &home.join("proj")));
        assert_eq!((all[0].hint.as_str(), all[1].hint.as_str()), ("…/a.rs", "…/lib/a.rs"));
        assert_eq!(all[0].why, "cat was reading …/a.rs in it");
        let got: Vec<Grant> = all.into_iter().map(|c| c.grant).collect();
        assert_eq!(
            got,
            [
                Grant::Read(home.join("src/lib")),
                Grant::Read(home.join("src")),
                Grant::Read(home.join("notes")),
                Grant::Write(home.join("notes")),
            ],
            "nothing for the key"
        );
    }

    /// A read's grant card says first when the place holds a token-like line — by the file's name, never the
    /// value, in the warning colour; it comes once the place's been looked through (a worker's). A place
    /// without one says nothing of tokens. (Fake values in the test's own temp folder.)
    #[test]
    fn a_grants_card_says_when_a_token_is_there() {
        let scratch = crate::scratch::Scratch::new("blocked-tok");
        let base = scratch.to_path_buf();
        for (f, text) in [("conf/hosts.yml", "oauth_token: tarae-fake-token\n"), ("plain/x.txt", "marker\n")]
        {
            std::fs::create_dir_all(base.join(f).parent().unwrap()).unwrap();
            std::fs::write(base.join(f), text).unwrap();
        }
        let mut ed = Editor::new(crate::config::Config::default());
        let run = BlockedRun::of(Job::Command("cat x".into(), base.clone()));
        let choice = |dir: &str| Choice {
            grant: Grant::Read(base.join(dir)),
            hint: String::new(),
            why: "cat was reading …/x in it".into(),
        };
        ed.workspace.blocked = Some((run, vec![choice("conf"), choice("plain")]));
        let card = |ed: &mut Editor, row: &str| {
            ed.offers.clear();
            ed.sandbox_grant(row).unwrap();
            let end = std::time::Instant::now() + crate::test_results::PATIENCE;
            while ed.offers.is_empty() {
                let left = end.saturating_duration_since(std::time::Instant::now());
                let ev = ed.events.recv_timeout(left).expect("the card before the timeout");
                ed.handle_event(ev);
            }
            let o = &ed.offers[0];
            let warned: Vec<&str> = match &o.warning {
                Some(r) => o.question.split('\n').skip(r.start).take(r.len()).collect(),
                None => Vec::new(),
            };
            (o.question.clone(), warned.join("\n"))
        };
        let (q, warned) = card(&mut ed, "0");
        // (as the card shows it, `~` for home: the test's temp folder is in home in a sandboxed run)
        let said =
            format!("In {}, hosts.yml holds a token-like line.", crate::allow::tilde(&base.join("conf")));
        assert_eq!(warned, said, "{q}");
        assert!(!q.contains("tarae-fake-token"), "{q}");
        let (q, warned) = card(&mut ed, "1");
        assert!(warned.is_empty() && !q.contains("token"), "{q}");
    }

    /// cardauto 6: a grant's card offers each scope up to what its grant may be kept for (`Caps::storable`) —
    /// `s` (this session) on every one kept past the conversation; a socket the conversation's at most; Docker's
    /// socket, a file named like a key, home and a place holding keys this run's only; a host as its own card.
    #[test]
    fn a_grants_card_offers_each_scope_up_to_what_may_be_kept() {
        let scratch = crate::scratch::Scratch::new("blocked-scopes");
        let base = scratch.to_path_buf();
        let home = crate::allow::home().expect("a home");
        let card = |g: Grant| {
            let run = BlockedRun::of(Job::Command("true".into(), base.clone()));
            grant_card(g, "it was refused", "true", Box::new(run), None, (NEXT_MESSAGE, true))
        };
        let all = "yasAn";
        for (g, keys) in [
            (Grant::Read(base.join("r")), all),
            (Grant::Write(base.join("w")), all),
            (Grant::Port(5432), all),
            (Grant::Terminals, all),
            (Grant::Host("example.invalid".into(), 443), "yasn"),
            (Grant::Socket(base.join("s.sock")), "yan"),
            (Grant::Socket("/run/docker.sock".into()), "yn"),
            (Grant::Key(base.join(".env")), "yn"),
            (Grant::Read(home.clone()), "yn"),
            (Grant::Read(home.join(".ssh")), "yn"),
        ] {
            let o = card(g.clone());
            let answers: String = o.answers(false).iter().map(|a| a.0).collect();
            assert_eq!(answers, keys, "{g:?}");
            // (what the keys take: each letter shown is taken, `s` among them)
            assert_eq!(o.lasts(), keys.contains('a'), "{g:?}");
            assert_eq!(o.for_workspace(), keys.contains('s'), "{g:?}");
            assert_eq!(o.keeps(), keys.contains('A'), "{g:?}");
        }
    }

    /// #119: a denied connect to a unix socket offers that socket — its real path, the file only — and not
    /// a path that isn't a socket, nor one in the project (that's open already); Docker's says what it is,
    /// and none is kept for good.
    #[test]
    fn a_denied_socket_offers_that_socket_only() {
        // (a socket's path has 104 bytes at most: short names — the temp folder is the home)
        let scratch = crate::scratch::Scratch::new("k");
        let base = scratch.to_path_buf();
        std::fs::create_dir_all(base.join(".colima/t")).unwrap();
        std::fs::create_dir_all(base.join("p")).unwrap();
        let home = base.clone();
        let (root, sock) = (home.join("p"), home.join(".colima/t/docker.sock"));
        let _l = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let _in = std::os::unix::net::UnixListener::bind(root.join("own.sock")).unwrap();
        std::os::unix::fs::symlink(&sock, home.join("docker.sock")).unwrap();
        std::fs::write(home.join("plain.txt"), "marker").unwrap();
        let d = |target: PathBuf| Denial {
            process: "docker".into(),
            op: "network-outbound".into(),
            target: target.display().to_string(),
        };
        let denials = [
            d(home.join("docker.sock")),
            d(sock.clone()),
            d(home.join("plain.txt")),
            d(root.join("own.sock")),
            d(home.join("gone.sock")),
        ];
        let got: Vec<Grant> =
            choices(&denials, &crate::caps::Caps::at(&home, &root)).into_iter().map(|c| c.grant).collect();
        assert_eq!(got, [Grant::Socket(sock.clone())], "the link's socket, once — nothing else");
        let g = Grant::Socket(sock);
        assert!(!g.keepable() && g.warning().is_some_and(|w| w.contains("mounts any folder")));
        assert!(Grant::Socket("/tmp/x/app.sock".into()).warning().is_none());
    }

    /// The kept-rows landing, review 4: the run panel's outside card says the kept places its command names — the tools
    /// pack's `names-kept`, the words Claude's outside card says, worked out on the list's worker (`BlockedRun.kept`)
    /// — right under the command, in the warning colour, and the card's record names it; a command naming none says
    /// nothing of it. A scratch home (`Editor.caps_home`); every card dismissed: nothing runs. Breaks if the panel's
    /// card stops saying it.
    #[test]
    fn the_panels_outside_card_says_a_kept_place_it_names() {
        let scratch = crate::scratch::Scratch::new("blocked-kept");
        let home = std::fs::canonicalize(&scratch).unwrap();
        for d in [".claude", "p"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        let mut ed = Editor::new(crate::config::Config::default());
        ed.workspace.root = home.join("p");
        ed.caps_home = Some(home.clone());
        let said = "▲ It names ~/.claude (Claude's own place): kept from every sandboxed run and every read — this run \
                    can read or change what's there. (tools pack)";
        for (cmd, names) in [("cat ~/.claude/settings.json", true), ("cat notes.txt", false)] {
            let watch = crate::sandbox::Watch::quiet("t.");
            let target = if names { home.join(".claude/settings.json") } else { home.join("p/notes.txt") };
            watch.record(Denial {
                process: "cat".into(),
                op: "file-read-data".into(),
                target: target.display().to_string(),
            });
            let policy = crate::sandbox::Policy {
                watch: Some(watch),
                ..crate::sandbox::Policy::for_project(&crate::sandbox::Env::default())
            };
            let task = crate::task::Task { name: cmd.into(), cmd: cmd.into(), cwd: home.join("p") };
            let mut run = crate::task::Run::for_test(task);
            run.guard = Some(crate::sandbox::Guard::In(Box::new(policy)));
            run.by_claude = true;
            ed.workspace.task_run = Some(run);
            ed.workspace.blocked = None;
            ed.sandbox_blocked(Which::Task);
            crate::test_results::settle_until(&mut ed, |ed| {
                ed.workspace.blocked.as_ref().is_some_and(|(r, _)| r.label() == cmd)
            });
            ed.sandbox_grant("outside").unwrap();
            let o = ed.offers.first().expect("its card");
            assert_eq!(o.title, OUTSIDE_TITLE);
            let lines: Vec<&str> = o.question.lines().collect();
            match names {
                true => {
                    assert_eq!(lines[..3], [cmd, "", said], "{}", o.question);
                    assert_eq!(o.warnings, vec![2..3]);
                    assert!(o.asked.as_ref().is_some_and(|a| a.warn == ["names-kept"]), "{:?}", o.asked);
                }
                false => assert!(!o.question.contains("It names") && o.warnings.is_empty(), "{}", o.question),
            }
            assert!(o.question.contains("Outside the sandbox it runs with all your permissions"));
            ed.offer_dismiss();
            ed.picker = None;
        }
    }

    /// R1's review, must 1: a yes on a place's card tells Claude, and toasts, what the card said of the files named
    /// like keys there (`BlockedRun.granting`) — never the plain words, which would let Claude think a key readable.
    #[test]
    fn a_places_yes_is_told_and_toasted_in_the_cards_words() {
        use crate::allow::Keys;
        let scratch = crate::scratch::Scratch::new("grant-told");
        let dir = std::fs::canonicalize(&scratch).unwrap();
        let mut ed = Editor::new(crate::config::Config::default());
        ed.workspace.root = dir.clone();
        ed.config.llm.command = "sh".into();
        ed.config.llm.args = vec!["-c".into(), "cat > /dev/null".into()];
        crate::chat::open(&mut ed);
        let cases = [
            (Grant::Read(dir.join("closed")), Keys::Closed, "but files named like keys"),
            (Grant::Write(dir.join("closed-w")), Keys::Closed, "in it but files named like keys"),
            (Grant::Read(dir.join("open")), Keys::Open, ", files named like keys too"),
            (Grant::Write(dir.join("open-w")), Keys::Open, ", files named like keys too"),
        ];
        for (n, (grant, keys, words)) in cases.into_iter().enumerate() {
            let c = ed.workspace.claude.chat.as_mut().unwrap();
            c.state = crate::chat::State::Thinking;
            let call = crate::tools::Call::of(c.generation(), &format!("t{n}"), serde_json::json!(n));
            let run = BlockedRun {
                claude: Some(call),
                granting: Some(keys),
                ..BlockedRun::of(crate::tools::Job::Command("true".into(), dir.clone()))
            };
            let said = grant.shown_with(keys);
            assert!(said.contains(words), "{said}");
            ed.workspace.claude.checkpoints.tell.clear();
            grant_answer(&mut ed, 0, grant, run, Some(Scope::ThisChat));
            let told = ed.workspace.claude.checkpoints.tell.join("\n");
            assert!(told.contains(&format!("The user let your commands {said} — ")), "{told}");
            let toast = ed.toasts.last().map(|t| t.text.clone()).unwrap_or_default();
            assert!(toast.starts_with(&format!("Claude's commands may {said} ")), "{toast}");
        }
    }
}
