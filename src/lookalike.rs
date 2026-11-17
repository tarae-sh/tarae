//! #610, landing 2: the folders a checkout already holds that git takes for a repository, found when it's first opened
//! since tarae started — `git::watch`'s worker, after its first round, whether a trust card shows or not; the git tab's
//! pull and switch rerun the cached half (`look_alikes_again`) — by the trust scan (`snapshot::scan`), and named on a
//! card of their own (S3). The card holds nothing: no run made them. `y` notes them: they join `Workspace.noted`, so a
//! later run's change to what decides one names it again on the protected-made card (landing 1's check), and the note
//! is kept in `trust.json` (`trust::Kind::LookAlike`, one entry a repository its checkouts share, its version a SHA-256
//! over each folder's path and look-hash) — **a note, never a trust**: kept as an answer of no, it grants nothing. The
//! card comes again only when that version changes; `n` (not now) leaves it unanswered — asked again when this project
//! is next opened (the next start, or a workspace for this checkout opened again: `look_answered` starts empty).

use std::path::{Path, PathBuf};

use crate::editor::Editor;
use crate::offer::{Offer, What};
use crate::snapshot::{Peeked, Scan, Taken};
use crate::trust::{Digest, Place};

/// The title, one folder or several.
pub const TITLE: &str = "A folder here git takes for a repository";
pub const TITLE_MANY: &str = "Folders here git takes for a repository";

/// Where `n` leaves the card's folders, said on it: "not now" is no silent state. The next start asks again, and so does
/// a workspace for this checkout opened again (`Workspace::new`: `look_answered` empty, its watch's first look).
pub const LATER: &str = "Not now: asked again when this project is next opened.";

/// The card's last line: each answer by its own word and when the card asks again after it — "this": a folder added
/// as well as one changed.
pub fn asked_again() -> String {
    format!("Noted: asked again only when this changes. {LATER}")
}

/// Folders the card names, at most — then how many more.
const NAMED: usize = 3;

/// A line of a found folder's `HEAD`, `config` or hooks on `v`, at most (characters).
const SHOWN_W: usize = 100;

/// What the card is about: the checkout, what the scan found there, and where the note is kept, its version.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub root: PathBuf,
    pub scan: Scan,
    pub place: Place,
    pub version: Digest,
}

/// Where a checkout's note is kept: its repository's (git's common folder, file `.` — its checkouts share it), else the
/// folder's own real path. Reads the disk: on a worker.
pub fn place(root: &Path) -> Place {
    let real = crate::paths::real(root).unwrap_or_else(|| root.to_path_buf());
    match crate::allow::common_dir(&real) {
        Some(common) => Place::Repo { common, file: PathBuf::from(".") },
        None => Place::File(real),
    }
}

/// The note's version: each found folder's path and look-hash, in order (sorted by the scan).
pub fn version(found: &[Taken]) -> Digest {
    crate::trust::hash(&found.iter().map(|f| (f.rel.clone(), f.hash.clone())).collect::<Vec<_>>())
}

impl Ask {
    /// The card (S3), laid out as S2's: the folders at full strength — one with when it came, several each bare on a
    /// line of its own (three, then how many more) and when they came said once under them — and the scan's cut under
    /// them when it was (a fact about this project: full strength too), then what git does with them
    /// (`snapshot::git_uses`, faint) and when each answer asks again; `v` shows each with what makes it one, its `HEAD`, `config` and hooks (its details, never on
    /// the card). Text from the checkout is spelled out (`trust::visible_line`): a folder's name can't make a line of the
    /// card's own.
    pub fn card(&self) -> Offer {
        let found = &self.scan.found;
        let line = crate::trust::visible_line;
        let title = if found.len() == 1 { TITLE } else { TITLE_MANY };
        let mut named: Vec<String> = match found.len() {
            1 => vec![format!("{}/ — here before Claude ran anything", line(&found[0].rel))],
            _ => found.iter().take(NAMED).map(|f| format!("{}/", line(&f.rel))).collect(),
        };
        if found.len() > NAMED {
            named.push(format!("and {} more", found.len() - NAMED));
        }
        if found.len() > 1 {
            named.push("All here before Claude ran anything.".into());
        }
        // (the title says what was looked for: the toast, alone, says it — `look_alikes_cut`)
        if let Some(why) = self.scan.cut_why() {
            named.push(format!("Not looked at: {why}."));
        }
        let decided = named.len();
        let mut q = named.join("\n");
        q += "\n\n";
        q += &crate::snapshot::git_uses(found.len() > 1);
        q += "\n\n";
        q += &asked_again();
        let from = q.split('\n').count();
        q.push('\n');
        for f in found {
            q += &format!("\n{}/ — {}", line(&f.rel), f.takes.said());
            q += &shown("HEAD", &f.head, true);
            q += &shown("config", &f.config, false);
            q += &shown("hooks", &f.hooks, false);
        }
        let to = q.split('\n').count();
        let what = What::LookAlike(Box::new(self.clone()));
        Offer::new(title.into(), &q, "", title.into(), what)
            .deciding(0..decided)
            .with_details(std::iter::once(from..to).collect())
    }
}

/// A found folder's part as `v` shows it: `  name: none`, or its lines — the first beside the name (`beside`, one line
/// as `HEAD` is) or all under it, a tab as two spaces — each cut at `SHOWN_W` (an `…`), and `… (cut)` under them when
/// the scan read only some. Spelled out (`trust::visible_line`), a remote's URL's token masked (as a card's words for
/// the reviewer are).
fn shown(name: &str, p: &Peeked, beside: bool) -> String {
    let one = |l: &str| {
        let l = crate::tokens::redacted(&crate::trust::visible_line(l.replace('\t', "  ").trim_end()));
        match l.chars().count() > SHOWN_W {
            true => format!("{}…", l.chars().take(SHOWN_W - 1).collect::<String>()),
            false => l,
        }
    };
    if p.lines.is_empty() && !p.cut {
        return format!("\n  {name}: none");
    }
    let mut lines = p.lines.iter();
    let mut s = match (beside, lines.next()) {
        (true, Some(l)) => format!("\n  {name}: {}", one(l)),
        (_, l) => format!("\n  {name}:") + &l.map(|l| format!("\n    {}", one(l))).unwrap_or_default(),
    };
    for l in lines {
        s += &format!("\n    {}", one(l));
    }
    if p.cut {
        s += "\n    … (cut)";
    }
    s
}

impl Editor {
    /// The trust scan's result for checkout `dir` (`git::watch`'s first look, or a rerun): its folders holding a `HEAD`
    /// remembered (`Workspace.heads`, as a run's are), then — something found — the store asked on a worker whether this
    /// version was noted. Only the session it's for.
    pub(crate) fn look_alikes_found(&mut self, dir: &Path, scan: Scan) {
        if self.workspace.root != dir {
            return self.unchanged = true;
        }
        for (p, parts) in &scan.heads {
            let heads = &mut self.workspace.heads;
            match () {
                _ if heads.iter().any(|h| &h.0 == p) => {}
                _ if heads.len() < crate::snapshot::HEADS_MAX => heads.push((p.clone(), *parts)),
                _ => self.workspace.heads_past += 1,
            }
        }
        self.workspace.look_folders = scan.folders.clone();
        if scan.found.is_empty() {
            return self.look_alikes_cut(&scan);
        }
        self.unchanged = true;
        let (root, store) = (dir.to_path_buf(), self.trust_path.clone());
        self.events.jobs().spawn(move || {
            let (place, version) = (place(&root), version(&scan.found));
            let state = crate::trust::Store::read(store.as_deref()).state_at(&place, version);
            let noted = state != crate::trust::State::Ask;
            move |ed: &mut Editor| ed.look_alikes_known(Ask { root, scan, place, version }, noted)
        });
    }

    /// What the store says of the scan's version, applied: noted before — its folders noted again, nothing asked;
    /// else its card (one a repository: one up already takes its place), unless this version was answered since tarae
    /// started.
    fn look_alikes_known(&mut self, ask: Ask, noted: bool) {
        if self.workspace.root != ask.root {
            return self.unchanged = true;
        }
        let place = ask.place.clone();
        let same = |o: &Offer| matches!(&o.what, What::LookAlike(a) if a.place == place);
        self.offers.retain(|o| !same(o) || o.installing());
        self.workspace.parked.retain(|o| !same(o));
        if noted || self.workspace.look_answered == Some(ask.version) {
            if noted {
                self.look_alikes_note(&ask);
            }
            return self.look_alikes_cut(&ask.scan);
        }
        // (its cut said on it)
        self.workspace.look_cut_told |= ask.scan.cut_why().is_some();
        let o = ask.card();
        self.push_offer(o);
    }

    /// The scan's cut, said once since tarae started in a toast — when no card says it. Nothing: nothing changed on screen.
    fn look_alikes_cut(&mut self, scan: &Scan) {
        match scan.cut_why() {
            Some(why) if !self.workspace.look_cut_told => {
                self.workspace.look_cut_told = true;
                self.set_warning(format!("Not looked through for folders git takes for a repository: {why}"));
            }
            _ => self.unchanged = true,
        }
    }

    /// The found folders noted (`Workspace.noted`, whole, sorted): a run's change to what decides one names it again.
    fn look_alikes_note(&mut self, ask: &Ask) {
        for f in &ask.scan.found {
            let p = ask.root.join(&f.rel);
            if let Err(i) = self.workspace.noted.binary_search(&p) {
                self.workspace.noted.insert(i, p);
            }
        }
    }

    /// The git tab's pull or switch landed: the cached half run again on a worker, the folders found before judged
    /// with it — a new or changed look-alike asks, one already noted doesn't.
    pub(crate) fn look_alikes_again(&mut self) {
        let (root, before) = (self.workspace.root.clone(), self.workspace.look_folders.clone());
        self.events.jobs().spawn(move || {
            let scan = crate::snapshot::scan(&root, Some(before.as_slice()));
            move |ed: &mut Editor| match scan {
                Ok(scan) => ed.look_alikes_found(&root, scan),
                Err(e) => {
                    crate::log::line("git", "warn", &format!("the look for folders git takes: {e}"));
                    ed.unchanged = true;
                }
            }
        });
    }
}

/// `y` (noted): its folders noted, the version answered since tarae started, and the note kept in the store — an answer
/// of no, at the repository's place: it grants nothing. One past what `look_hash` reads isn't kept (the next start asks
/// again), as a protected-made card's `y` keeps none.
pub(crate) fn answer(ed: &mut Editor, id: u64, ask: Ask) {
    ed.offer_finished(id, Ok(()));
    ed.look_alikes_note(&ask);
    ed.workspace.look_answered = Some(ask.version);
    ed.set_success(match ask.scan.found.len() {
        1 => "Noted — a run's change to what decides it is named",
        _ => "Noted — a run's change to what decides one is named",
    });
    let Some(path) = ed.trust_path.clone() else { return };
    if !ask.scan.found.iter().all(Taken::keepable) {
        return;
    }
    let (given, seen) = (crate::trust::given(), crate::storewrite::seen_now(&path));
    let w = crate::trust::Write::Answer {
        kind: crate::trust::Kind::LookAlike,
        place: Some(ask.place),
        file: ask.root,
        hash: ask.version,
        trusted: false,
        given,
        seen,
    };
    crate::trust::queue_said(ed, path, vec![w], "");
}

/// `n` (not now): unanswered — not asked again until this project is next opened (`LATER`), unless what it found
/// changes.
pub(crate) fn later(ed: &mut Editor, ask: &Ask) {
    ed.workspace.look_answered = Some(ask.version);
    ed.set_status(LATER_SAID);
}

/// The status line after `n`, as the card's line says it.
pub const LATER_SAID: &str = "Not now — asked again when this project is next opened";

#[cfg(test)]
mod tests;
