//! G13 (user, 2026-10-28; `.sb/g13-plan.md`): a language server that runs the project's code as it starts and
//! works — its pack's `runs-project-code`: rust-analyzer's build scripts and proc macros and the toolchain the
//! project names, jdtls's Gradle build — waits for the repository's trust. Asked once, every checkout of it sharing
//! the answer (`trust.rs`'s `Kind::Code`, kept at the repository's git common folder, else at the server's root), as
//! Claude Code keys its workspace trust on the repository's root and VS Code's rust-analyzer won't run in an
//! untrusted folder. srt has no notion of either: its way, the server in the sandbox, is a later item of its own.
//!
//! Until a root's answer is read (on a worker — the store is a file) and while it asks, its files wait for that
//! server (`Root.held`), nothing started; the other servers (a TOML one, a markdown one) start as ever. `y` keeps the
//! trust and starts them; `n` holds them back until you quit, the status line saying why. `:code-trust` asks again,
//! `:code-untrust` takes it back (the servers stopped).

use std::path::{Path, PathBuf};

use crate::document::DocId;
use crate::editor::Editor;
use crate::offer::What;
use crate::trust::{Digest, Kind, Place, State};

/// The place's name in `trust.json` and `:granted` — a repository's code, never a file of it.
pub const FILE: &str = "(its code)";

pub const TITLE: &str = "Trust this repository's code?";

/// What a yes is about: one version — every repository asks once.
fn hash() -> Digest {
    crate::trust::hash(&[("code".into(), "every server that runs it".into())])
}

/// The root's answer's name as a file, for what's said (`trust::Write`'s words).
fn file(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// Where a root's answer is kept (it reads the disk: a worker's): its repository's — one for every checkout and
/// every server root in it, a second cargo or gradle root too —, else the root's own.
fn place(root: &Path) -> Place {
    match crate::trust::place_of(&file(root)) {
        Place::Repo { common, .. } => Place::Repo { common, file: PathBuf::from(FILE) },
        own => own,
    }
}

/// A server root's trust, as read and answered.
#[derive(Clone, Debug, Default)]
pub struct Root {
    /// None while it's read.
    pub state: Option<State>,
    reading: bool,
    /// A take-back came while it was read: what the read found may be from before it (a yes the store had then) —
    /// read again as it lands, the take-back marked by then (`trust::taking`: read as never answered).
    again: bool,
    /// Where its answer is kept, read with it (found on the disk) — a repository's, every checkout's and root's,
    /// or its own.
    place: Option<Place>,
    /// The files waiting for its servers.
    pub held: Vec<DocId>,
    /// The servers waiting (name, what of the project each runs — its pack's words).
    pub servers: Vec<(String, String)>,
    /// Its card went up since its answer was read (asked once; `:code-trust` again).
    asked: bool,
    /// Each server's look for what it loads (`lspagent.rs`), by its name — trusted only.
    pub agents: std::collections::HashMap<String, Look>,
}

/// A server's agents at a trusted root: looked for each time it starts, never kept past the start they were found
/// for — a jar Gradle's cleanup took or something else put in its place since is never passed on.
#[derive(Clone, Debug, Default)]
pub struct Look {
    /// The latest look begun (`Looks.count` then): a result of an older one — its trust taken back and given again
    /// meanwhile, the places runs write worked out then — is dropped.
    pub begun: u64,
    /// None while it's looked for.
    pub got: Option<Got>,
    /// The files waiting for it, only while it's looked for.
    pub held: Vec<DocId>,
}

#[derive(Clone, Debug)]
pub enum Got {
    /// Found by look `from`, for the server's next start (`lsp_start` takes it).
    Ready { from: u64, found: crate::lspagent::Found },
    /// Server `cid` started without agents its project uses (their names): its own import of the build may download
    /// them (a fresh clone), so they're looked for again — once — when it says the import is done (`asked`), with
    /// the arguments it's configured with (`args`, as the first look — never the ones it started with: its pack's
    /// `args-add` names the root, `-data …/lombok-demo-…`, which `unless` would take for an agent of yours).
    Waits { cid: crate::lsp::ClientId, missing: Vec<String>, args: Vec<String>, asked: bool },
}

/// The editor's count of looks begun (a root's `Root` goes with its trust, the count stays), and the home they look
/// in — None: this process's (`lspagent::found`'s stand-in otherwise).
#[derive(Debug, Default)]
pub struct Looks {
    pub count: u64,
    pub home: Option<PathBuf>,
}

impl Editor {
    /// Whether `server` may start for files under `root` now — false: file `doc` waits for it, the root's trust
    /// read, or asked about, or — trusted — its agents looked for. Every server that runs no code of the project's
    /// starts.
    pub(crate) fn code_trusted(&mut self, server: &crate::lsp::ServerSpec, root: &Path, doc: DocId) -> bool {
        if !self.code_gate {
            return true;
        }
        let Some(words) = crate::lsp::traits(server).runs_project_code.clone() else { return true };
        let r = self.code_trust.entry(root.to_path_buf()).or_default();
        if r.state == Some(State::Trusted) {
            return self.code_agents_found(server, root, doc);
        }
        if !r.held.contains(&doc) {
            r.held.push(doc);
        }
        if !r.servers.iter().any(|(n, _)| *n == server.name) {
            r.servers.push((server.name.clone(), words));
        }
        match r.state {
            None if !r.reading => self.code_trust_reading(root),
            Some(State::Ask) if !r.asked => self.code_trust_card(root),
            _ => {}
        }
        false
    }

    /// A root's answer read from the store, on a worker.
    fn code_trust_reading(&mut self, root: &Path) {
        if let Some(r) = self.code_trust.get_mut(root) {
            (r.reading, r.again) = (true, false);
        }
        let (path, root) = (self.trust_path.clone(), root.to_path_buf());
        self.events.jobs().spawn(move || {
            let place = place(&root);
            let state = crate::trust::Store::read(path.as_deref()).state_at(&place, hash());
            move |ed: &mut Editor| ed.code_trust_read(&root, state, place)
        });
    }

    /// A take-back given: every root still being read reads again as its read lands — the store may have said yes
    /// to it before the take-back was marked (no root's place is known before its read: all of them).
    fn code_trust_reread(&mut self) {
        for r in self.code_trust.values_mut().filter(|r| r.reading) {
            r.again = true;
        }
    }

    /// Whether `server`'s agents at the trusted `root` are known for its start (`lspagent.rs`) — false: file `doc`
    /// waits while they're looked for, on a worker, each time it starts and only once its code is trusted. A server
    /// with none, or one running there already: at once.
    fn code_agents_found(&mut self, server: &crate::lsp::ServerSpec, root: &Path, doc: DocId) -> bool {
        if crate::lsp::traits(server).agents.is_empty()
            || self.lsp.clients.iter().any(|c| c.name == server.name && c.root == root)
        {
            return true;
        }
        let Some(r) = self.code_trust.get_mut(root) else { return true };
        match r.agents.get_mut(&server.name) {
            // (found by its latest look: a result of an older one never starts it)
            Some(l) if matches!(l.got, Some(Got::Ready { from, .. }) if from == l.begun) => return true,
            Some(look @ Look { got: None, .. }) => {
                if !look.held.contains(&doc) {
                    look.held.push(doc);
                }
                return false;
            }
            // (none yet, or the server it was for is gone: a look of its own for this start)
            _ => {}
        }
        let agents = &crate::lsp::traits(server).agents;
        let begun = self.code_agents_look(&server.name, agents, &server.args, root);
        if let Some(r) = self.code_trust.get_mut(root) {
            r.agents.insert(server.name.clone(), Look { begun, got: None, held: vec![doc] });
        }
        false
    }

    /// A look for server `name`'s `agents` at `root` (its arguments `args`) begun on a worker — its number.
    fn code_agents_look(
        &mut self,
        name: &str,
        agents: &'static [crate::lspagent::Agent],
        args: &[String],
        root: &Path,
    ) -> u64 {
        self.agent_looks.count += 1;
        let begun = self.agent_looks.count;
        // (where Claude's runs may write, worked out there: a jar one of them put in place is never loaded)
        let writes = self.workspaces_writes();
        let home = self.agent_looks.home.clone();
        let (name, args, root) = (name.to_string(), args.to_vec(), root.to_path_buf());
        self.events.jobs().spawn(move || {
            let found = crate::lspagent::found(agents, &args, &root, writes(), home.as_deref());
            move |ed: &mut Editor| ed.code_agents_read(&root, name, begun, found)
        });
        begun
    }

    /// Look `from` for `server`'s agents at `root` done: its files get it, started with them (`lsp_start`) — or,
    /// looked for again after its import, it starts again with what's there now. A look begun since, or its trust
    /// taken back meanwhile: nothing (the root's answer read again).
    fn code_agents_read(&mut self, root: &Path, server: String, from: u64, found: crate::lspagent::Found) {
        let Some(r) = self.code_trust.get_mut(root).filter(|r| r.state == Some(State::Trusted)) else {
            return;
        };
        let Some(look) = r.agents.get_mut(&server).filter(|l| l.begun == from) else { return };
        let Some(Got::Waits { cid, missing, .. }) = look.got.take() else {
            look.got = Some(Got::Ready { from, found });
            for d in std::mem::take(&mut look.held) {
                self.attach_lsp(d);
            }
            return;
        };
        let now: Vec<String> =
            found.loaded.iter().map(|(n, _)| n.clone()).filter(|n| missing.contains(n)).collect();
        let running = self.lsp.clients.iter().any(|c| c.id == cid);
        if now.is_empty() || !running {
            r.agents.remove(&server);
            return;
        }
        // (once: what it starts with now has nothing missing to look for again)
        let found = crate::lspagent::Found { missing: Vec::new(), ..found };
        look.got = Some(Got::Ready { from, found });
        let served: Vec<DocId> =
            self.docs.iter().filter(|d| d.lsp.clients().any(|c| c == cid)).map(|d| d.id).collect();
        self.lsp_stopped(cid);
        for d in served {
            self.attach_lsp(d);
        }
        self.set_status(format!("{} found — {server} restarts with it", now.join(", ")));
    }

    /// What `server` starts with at `root` besides its arguments: its agents found for this start
    /// (`code_agents_found`), taken — the next start looks again.
    pub(crate) fn code_agents_take(&mut self, server: &str, root: &Path) -> crate::lspagent::Found {
        let Some(r) = self.code_trust.get_mut(root) else { return Default::default() };
        match r.agents.get(server) {
            Some(Look { got: Some(Got::Ready { .. }), .. }) => match r.agents.remove(server) {
                Some(Look { got: Some(Got::Ready { found, .. }), .. }) => found,
                _ => Default::default(),
            },
            _ => Default::default(),
        }
    }

    /// Server `cid` started at `root` without agents its project uses (`missing`), its configured arguments `args`:
    /// looked for again once it says its import is done (`code_agents_imported`).
    pub(crate) fn code_agents_started(
        &mut self,
        server: &str,
        root: &Path,
        cid: crate::lsp::ClientId,
        missing: Vec<String>,
        args: Vec<String>,
    ) {
        let Some(r) = self.code_trust.get_mut(root) else { return };
        let look = r.agents.entry(server.to_string()).or_default();
        look.got = Some(Got::Waits { cid, missing, args, asked: false });
    }

    /// Server `cid` says its import of the build is done (its pack's `progress` done, or `quiescent`): what its
    /// project uses and wasn't there as it started is looked for again, once.
    pub(crate) fn code_agents_imported(&mut self, cid: crate::lsp::ClientId) {
        let Some(c) = self.lsp.clients.iter().find(|c| c.id == cid) else { return };
        let (name, root, agents) = (c.name.clone(), c.root.clone(), &c.traits.agents);
        let Some(look) = self.code_trust.get_mut(&root).and_then(|r| r.agents.get_mut(&name)) else { return };
        let Some(Got::Waits { cid: c, asked, args, .. }) = &mut look.got else { return };
        if *c != cid || *asked {
            return;
        }
        *asked = true;
        let args = args.clone();
        let begun = self.code_agents_look(&name, agents, &args, &root);
        if let Some(look) = self.code_trust.get_mut(&root).and_then(|r| r.agents.get_mut(&name)) {
            look.begun = begun;
        }
    }

    /// A root's answer read: trusted, its files get their servers; else asked — a no kept from before asks again
    /// too (`n` holds back until you quit only). Another root of its repository answered since tarae started (a yes
    /// maybe not in the store yet, a no never kept), its answer is this one's; one asking, this one joins its card —
    /// either before the store's yes (#226's review: a yes kept, then `:code-trust` and `n`, and a checkout read
    /// meanwhile came back trusted, its servers started while the repository was held back).
    fn code_trust_read(&mut self, root: &Path, state: State, place: Place) {
        let Some(r) = self.code_trust.get_mut(root) else { return };
        if r.again {
            return self.code_trust_reading(root);
        }
        (r.reading, r.place) = (false, Some(place));
        let kin = self.code_trust_kin(root);
        let kin_is = |s| kin[1..].iter().any(|p| self.code_trust.get(p).and_then(|r| r.state) == Some(s));
        let state = match state {
            _ if kin_is(State::Ignored) => State::Ignored,
            _ if kin_is(State::Ask) => State::Ask,
            _ if kin_is(State::Trusted) => State::Trusted,
            State::Trusted => State::Trusted,
            _ => State::Ask,
        };
        if let Some(r) = self.code_trust.get_mut(root) {
            r.state = Some(state);
        }
        match state {
            State::Trusted => self.code_trust_release(root),
            // (held back with the others until you quit — said once, as `n` was answered)
            State::Ignored => {}
            _ => self.code_trust_card(root),
        }
    }

    /// The roots that share `root`'s answer, `root` first: every root read with its place — its repository's, each
    /// checkout's and a second cargo or gradle root's —, `root` alone until its place is read.
    fn code_trust_kin(&self, root: &Path) -> Vec<PathBuf> {
        let place = self.code_trust.get(root).and_then(|r| r.place.clone());
        let mut kin: Vec<PathBuf> = self
            .code_trust
            .iter()
            .filter(|(p, r)| place.is_some() && r.place == place && p.as_path() != root)
            .map(|(p, _)| p.clone())
            .collect();
        crate::sorting::sort(&mut kin);
        kin.insert(0, root.to_path_buf());
        kin
    }

    /// Its card: which servers wait, and what of the repository each would run as you. One card a repository
    /// (#226: each checkout of ten had its own): one up already — in front, or parked with any session — says every
    /// root's servers, and a parked one comes up in front once a file there waits for it.
    fn code_trust_card(&mut self, root: &Path) {
        let kin = self.code_trust_kin(root);
        let (mut servers, mut repo) = (Vec::<(String, String)>::new(), false);
        for p in &kin {
            let Some(r) = self.code_trust.get_mut(p) else { continue };
            r.asked = true;
            repo |= matches!(r.place, Some(Place::Repo { .. }));
            for s in &r.servers {
                if !servers.iter().any(|(n, _)| *n == s.0) {
                    servers.push(s.clone());
                }
            }
        }
        let up = std::iter::once(&self.workspace)
            .chain(&self.workspaces)
            .flat_map(|l| &l.parked)
            .chain(&self.offers)
            .find_map(|o| match &o.what {
                What::CodeTrust { root } if kin.contains(root) => Some(root.clone()),
                _ => None,
            });
        let at = up.clone().unwrap_or_else(|| root.to_path_buf());
        let name = at.file_name().map_or_else(|| at.display().to_string(), |n| n.to_string_lossy().into());
        let mut q =
            format!("Its language servers run {name}'s code as you, outside the sandbox, as they start:\n");
        for (s, words) in &servers {
            q.push_str(&format!("\n  {s} — {words}"));
        }
        q.push_str("\n\nUntil you trust it they don't start.");
        if repo {
            q.push_str(" Every checkout of it shares the answer.");
        }
        let o = crate::offer::Offer::new(
            TITLE.into(),
            &q,
            "",
            "Code trust".into(),
            What::CodeTrust { root: at.clone() },
        )
        .deciding(2..2 + servers.len());
        if up.is_none() {
            return self.push_offer(o);
        }
        let same = |x: &crate::offer::Offer| matches!(&x.what, What::CodeTrust { root } if *root == at);
        let fresh = |old: &mut crate::offer::Offer| {
            if old.question != o.question {
                (old.question, old.decided) = (o.question.clone(), o.decided.clone());
                old.rev += 1;
            }
        };
        if let Some(old) = self.offers.iter_mut().find(|x| same(x)) {
            return fresh(old);
        }
        let front = !self.background();
        let mut taken = None;
        for l in std::iter::once(&mut self.workspace).chain(&mut self.workspaces) {
            let Some(i) = l.parked.iter().position(same) else { continue };
            match front {
                true => taken = Some(l.parked.remove(i)),
                false => fresh(&mut l.parked[i]),
            }
            break;
        }
        if let Some(mut old) = taken {
            fresh(&mut old);
            let i = self.offer_place(&old);
            self.offers.insert(i, old);
        }
    }

    /// The answer on its card: `y` kept and the servers start; `n` they wait until you quit — at every root of its
    /// repository read since tarae started (one that's still read takes it as it lands: `code_trust_read`).
    pub(crate) fn code_trust_answer(&mut self, id: u64, root: &Path, trusted: bool) {
        self.offer_finished(id, Ok(()));
        let Some(place) = self.code_trust.get(root).map(|r| r.place.clone()) else { return };
        let kin = self.code_trust_kin(root);
        let mut names: Vec<String> = Vec::new();
        for p in &kin {
            let Some(r) = self.code_trust.get_mut(p) else { continue };
            r.state = Some(if trusted { State::Trusted } else { State::Ignored });
            for (n, _) in &r.servers {
                if !names.contains(n) {
                    names.push(n.clone());
                }
            }
        }
        if !trusted {
            return self.set_status(format!("{} held back — :code-trust to look again", names.join(", ")));
        }
        if let Some(path) = self.trust_path.clone() {
            let seen = crate::storewrite::seen_now(&path);
            let w = crate::trust::Write::Answer {
                kind: Kind::Code,
                place,
                file: file(root),
                hash: hash(),
                trusted: true,
                given: crate::trust::given(),
                seen,
            };
            crate::trust::queue(self, path, vec![w], |ed, r| {
                if let Err(e) = r {
                    ed.set_error(format!("couldn't keep the answer: {e}"));
                }
            });
        }
        for p in &kin {
            self.code_trust_release(p);
        }
        self.set_success(format!("Trusted — {} start", names.join(", ")));
    }

    /// Another tarae's `y` for `root`'s repository, written to the store this one reads (the simulation's) — nothing
    /// changes here until a root's read finds it.
    #[cfg(test)]
    pub(crate) fn code_trust_given_elsewhere(&mut self, root: &Path) {
        let Some(path) = self.trust_path.clone() else { return };
        let w = crate::trust::Write::Answer {
            kind: Kind::Code,
            place: Some(place(root)),
            file: file(root),
            hash: hash(),
            trusted: true,
            given: crate::trust::given(),
            seen: crate::storewrite::seen_now(&path),
        };
        crate::trust::queue(self, path, vec![w], |_, _| {});
    }

    /// A trusted root's waiting files get their servers.
    fn code_trust_release(&mut self, root: &Path) {
        let held = self.code_trust.get_mut(root).map(|r| std::mem::take(&mut r.held)).unwrap_or_default();
        for d in held {
            self.attach_lsp(d);
        }
    }

    /// The servers waiting for trust at the front file's root, for the status line — None: none waits.
    pub(crate) fn code_trust_waits(&self) -> Option<String> {
        let id = self.doc().id;
        let r = self.code_trust.values().find(|r| r.held.contains(&id) && r.state != Some(State::Trusted))?;
        let names: Vec<&str> = r.servers.iter().map(|(n, _)| n.as_str()).collect();
        Some(names.join(", "))
    }

    /// `:code-trust` — the card again for the front file's root.
    pub fn code_trust_ask(&mut self) -> Result<(), String> {
        let id = self.doc().id;
        let root = self
            .code_trust
            .iter()
            .filter(|(_, r)| r.held.contains(&id) && r.state != Some(State::Trusted))
            // (a root read first — its repository known —, then by path: never the map's order)
            .min_by_key(|(p, r)| (r.state.is_none(), p.to_path_buf()))
            .map(|(p, _)| p.clone())
            .ok_or("no language server here waits for this repository's trust")?;
        // (its repository's roots known once its answer is read — asked then by itself)
        if self.code_trust.get(&root).is_some_and(|r| r.state.is_none()) {
            return Err("this repository's trust is still being read — its card comes once it is".into());
        }
        let kin = self.code_trust_kin(&root);
        for p in &kin {
            if let Some(r) = self.code_trust.get_mut(p) {
                r.state = Some(State::Ask);
            }
        }
        // (the one up comes again as yours — one parked with another session comes up here: `code_trust_card`)
        self.drop_cards(|w| matches!(w, What::CodeTrust { root } if kin.contains(root)));
        self.code_trust_card(&root);
        Ok(())
    }

    /// `:code-untrust` — the front file's repository's trust taken back: the servers that run its code stop at every
    /// root it holds (each checkout's, a second cargo or gradle root's), and their files wait for them again (asked
    /// at the next one).
    pub fn code_untrust(&mut self) -> Result<(), String> {
        let path = self.doc().path.clone().ok_or("this buffer has no file")?;
        let (root, place) = self
            .code_trust
            .iter()
            .find(|(p, r)| path.starts_with(p) && r.state == Some(State::Trusted))
            .map(|(p, r)| (p.clone(), r.place.clone()))
            .ok_or("this repository's code isn't trusted here")?;
        // (read with its trust, so always there once trusted; worked out here only if it ever weren't)
        let place = place.unwrap_or_else(|| self::place(&root));
        let given = self.trust_taking_given(place.clone());
        match self.trust_path.clone() {
            Some(store) => {
                let w = crate::trust::Write::Forget { kind: Kind::Code, place: place.clone(), given };
                crate::trust::queue_said(self, store, vec![w], " — held back until you quit");
            }
            None => crate::trust::taken(&place, given),
        }
        self.code_trust_reread();
        let mut roots: Vec<PathBuf> = self
            .code_trust
            .iter()
            .filter(|(_, r)| r.state == Some(State::Trusted) && r.place.as_ref() == Some(&place))
            .map(|(p, _)| p.clone())
            .collect();
        if !roots.contains(&root) {
            roots.push(root);
        }
        let mut stopped = Vec::new();
        for root in roots {
            for name in self.code_servers_stop(&root) {
                if !stopped.contains(&name) {
                    stopped.push(name);
                }
            }
        }
        match stopped.is_empty() {
            true => self.set_status("Trust taken back — its servers wait for it at the next file"),
            false => self.set_status(format!("Trust taken back — {} stopped", stopped.join(", "))),
        }
        Ok(())
    }

    /// `:granted`'s `d` on a code trust answer (forgotten in the store by then): every trusted root's servers
    /// that run its code stop, and its files wait for them, the store read again.
    pub(crate) fn code_trust_forgotten(&mut self) {
        self.code_trust_reread();
        let roots: Vec<PathBuf> = self
            .code_trust
            .iter()
            .filter(|(_, r)| r.state == Some(State::Trusted))
            .map(|(p, _)| p.clone())
            .collect();
        for root in roots {
            self.code_servers_stop(&root);
        }
    }

    /// Root `root`'s running servers that run its code stopped, its trust read again — their files waiting for
    /// them; which stopped.
    fn code_servers_stop(&mut self, root: &Path) -> Vec<String> {
        // (a file waiting for a look waits for the trust again)
        let looking: Vec<DocId> = self
            .code_trust
            .remove(root)
            .map(|r| r.agents.into_values().flat_map(|l| l.held).collect())
            .unwrap_or_default();
        let gone: Vec<crate::lsp::ClientId> = self
            .lsp
            .clients
            .iter()
            .filter(|c| c.roots.iter().any(|r| r.starts_with(root)))
            .filter(|c| self.client_runs_code(c.id))
            .map(|c| c.id)
            .collect();
        let mut stopped = Vec::new();
        for cid in gone {
            let served: Vec<DocId> =
                self.docs.iter().filter(|d| d.lsp.clients().any(|c| c == cid)).map(|d| d.id).collect();
            if let Some(name) = self.lsp_stopped(cid) {
                stopped.push(name);
            }
            for d in served {
                self.attach_lsp(d);
            }
        }
        for d in looking {
            self.attach_lsp(d);
        }
        stopped
    }

    /// Whether the running server `cid` is one that runs the project's code (its pack says so).
    fn client_runs_code(&self, cid: crate::lsp::ClientId) -> bool {
        self.client(cid).is_some_and(|c| c.traits.runs_project_code.is_some())
    }
}

#[cfg(test)]
mod tests;
