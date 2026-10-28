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
use crate::trust::{Digest, Kind, State};

/// The place's name in `trust.json` and `:granted` — a repository's code, never a file of it.
pub const FILE: &str = "(its code)";

pub const TITLE: &str = "Trust this repository's code?";

/// What a yes is about: one version — every repository asks once.
fn hash() -> Digest {
    crate::trust::hash(&[("code".into(), "every server that runs it".into())])
}

/// Where a root's answer is kept: its repository's (every checkout's), else the root's own.
fn file(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// A server root's trust, as read and answered.
#[derive(Clone, Debug, Default)]
pub struct Root {
    /// None while it's read.
    pub state: Option<State>,
    reading: bool,
    /// Its answer is a repository's, every checkout's (read with it: where it's kept is found on the disk).
    repo: bool,
    /// The files waiting for its servers.
    pub held: Vec<DocId>,
    /// The servers waiting (name, what of the project each runs — its pack's words).
    pub servers: Vec<(String, String)>,
    /// Its card went up since its answer was read (asked once; `:code-trust` again).
    asked: bool,
}

impl Editor {
    /// Whether `server` may start for files under `root` now — false: file `doc` waits for it, the root's trust
    /// read, or asked about. Every server that runs no code of the project's starts.
    pub(crate) fn code_trusted(&mut self, server: &crate::lsp::ServerSpec, root: &Path, doc: DocId) -> bool {
        if !self.code_gate {
            return true;
        }
        let Some(words) = crate::lsp::traits(server).runs_project_code.clone() else { return true };
        let r = self.code_trust.entry(root.to_path_buf()).or_default();
        if r.state == Some(State::Trusted) {
            return true;
        }
        if !r.held.contains(&doc) {
            r.held.push(doc);
        }
        if !r.servers.iter().any(|(n, _)| *n == server.name) {
            r.servers.push((server.name.clone(), words));
        }
        match r.state {
            None if !r.reading => {
                r.reading = true;
                let (path, root) = (self.trust_path.clone(), root.to_path_buf());
                self.events.jobs().spawn(move || {
                    let state = crate::trust::Store::read(path.as_deref()).state(&file(&root), hash());
                    let repo =
                        matches!(crate::trust::place_of(&file(&root)), crate::trust::Place::Repo { .. });
                    move |ed: &mut Editor| ed.code_trust_read(&root, state, repo)
                });
            }
            Some(State::Ask) if !r.asked => self.code_trust_card(root),
            _ => {}
        }
        false
    }

    /// A root's answer read: trusted, its files get their servers; else asked — a no kept from before asks again
    /// too (`n` holds back until you quit only).
    fn code_trust_read(&mut self, root: &Path, state: State, repo: bool) {
        let Some(r) = self.code_trust.get_mut(root) else { return };
        (r.reading, r.repo) = (false, repo);
        match state {
            State::Trusted => {
                r.state = Some(State::Trusted);
                self.code_trust_release(root);
            }
            _ => {
                r.state = Some(State::Ask);
                self.code_trust_card(root);
            }
        }
    }

    /// Its card: which servers wait, and what of the repository each would run as you.
    fn code_trust_card(&mut self, root: &Path) {
        let Some(r) = self.code_trust.get_mut(root) else { return };
        r.asked = true;
        let (servers, repo) = (r.servers.clone(), r.repo);
        let name =
            root.file_name().map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().into());
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
            crate::offer::What::CodeTrust { root: root.to_path_buf() },
        )
        .deciding(2..2 + servers.len());
        self.drop_cards(|w| matches!(w, crate::offer::What::CodeTrust { root: r } if r == root));
        self.push_offer(o);
    }

    /// The answer on its card: `y` kept and the servers start; `n` they wait until you quit.
    pub(crate) fn code_trust_answer(&mut self, id: u64, root: &Path, trusted: bool) {
        self.offer_finished(id, Ok(()));
        let Some(r) = self.code_trust.get_mut(root) else { return };
        let names: Vec<String> = r.servers.iter().map(|(n, _)| n.clone()).collect();
        if !trusted {
            r.state = Some(State::Ignored);
            return self.set_status(format!("{} held back — :code-trust to look again", names.join(", ")));
        }
        r.state = Some(State::Trusted);
        if let Some(path) = self.trust_path.clone() {
            let seen = crate::storewrite::seen_now(&path);
            let w = crate::trust::Write::Answer {
                kind: Kind::Code,
                place: None,
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
        self.code_trust_release(root);
        self.set_success(format!("Trusted — {} start", names.join(", ")));
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
            .find(|(_, r)| r.held.contains(&id) && r.state != Some(State::Trusted))
            .map(|(p, _)| p.clone())
            .ok_or("no language server here waits for this repository's trust")?;
        if let Some(r) = self.code_trust.get_mut(&root) {
            r.state = Some(State::Ask);
        }
        self.code_trust_card(&root);
        Ok(())
    }

    /// `:code-untrust` — the front file's root's trust taken back: its servers that run its code stop, and its
    /// files wait for them again (asked at the next one).
    pub fn code_untrust(&mut self) -> Result<(), String> {
        let path = self.doc().path.clone().ok_or("this buffer has no file")?;
        let root = self
            .code_trust
            .iter()
            .find(|(p, r)| path.starts_with(p) && r.state == Some(State::Trusted))
            .map(|(p, _)| p.clone())
            .ok_or("this repository's code isn't trusted here")?;
        let place = crate::trust::place_of(&file(&root));
        let given = self.trust_taking_given(place.clone());
        match self.trust_path.clone() {
            Some(store) => {
                let w = crate::trust::Write::Forget { kind: Kind::Code, place, given };
                crate::trust::queue_said(self, store, vec![w], " — held back until you quit");
            }
            None => crate::trust::taken(&place, given),
        }
        let stopped = self.code_servers_stop(&root);
        match stopped.is_empty() {
            true => self.set_status("Trust taken back — its servers wait for it at the next file"),
            false => self.set_status(format!("Trust taken back — {} stopped", stopped.join(", "))),
        }
        Ok(())
    }

    /// `:granted`'s `d` on a code trust answer (forgotten in the store by then): every trusted root's servers
    /// that run its code stop, and its files wait for them, the store read again.
    pub(crate) fn code_trust_forgotten(&mut self) {
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
        self.code_trust.remove(root);
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
        stopped
    }

    /// Whether the running server `cid` is one that runs the project's code (its pack says so).
    fn client_runs_code(&self, cid: crate::lsp::ClientId) -> bool {
        self.client(cid).is_some_and(|c| c.traits.runs_project_code.is_some())
    }
}

#[cfg(test)]
mod tests;
