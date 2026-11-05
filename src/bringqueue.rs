//! The bring queue (frictions29 24 a, as frictions30's plan decided): several sessions ready at once, brought into
//! their base one after another on its tip — each rebased onto it when it moved (`rebase_check`, as the rebase card's
//! `y`), its quick preflight run there, then brought (fast-forward only, as the bring card's `y`); the next starts
//! once one lands. One `y` for the queue (`What::BringQueue`). A conflict, a failed or declined preflight, changes
//! not in a commit, a fast-forward refused twice, its HEAD moved while its preflight ran (what lands is the commit
//! tested, #373) — that one is out, said, and the rest go on; its own card (the rebase's, the held one's, the bring
//! card asking for the preflight again) stays as a single bring has it.
//!
//! `space W`: `m` marks the row in hand (again: unmarks) — the order marked is the order brought — and `B` asks to
//! bring the marked ones. The queue is the editor's, not a session's: one at a time, whatever's in front.
//!
//! **Every bring waits its turn in it** (#371, carry's `.sb/bringqueue-plan.md` (a)): a bring card's `y` while another
//! bring is under way — the queue's, or one going by itself — lines up behind it, its place said on the card before
//! (`queued_line`) and on the list's row after (`bring 2nd`); first asked, first brought. A bring going by itself goes
//! first: the line waits for it (`bring_alone`), but not for one stopped at its rebase's conflict, which waits on a
//! person. Each in turn is looked at on its base's tip as it is then — so a tip moved while it waited is rebased onto
//! and preflighted there by itself; a tip moved under its own preflight (by a bring outside the queue, a commit of
//! yours) gets one more try, then its card. Before its preflight its checkout is looked at (`bring_queue_check`):
//! changes in no commit stop it (the held card, as a rebase's), and so do high-risk changes no one looked at (#379:
//! its bring card up with `h`, never a preflight run to be thrown away) — the queue's `y` names those first.

use std::collections::VecDeque;

use crate::editor::Editor;
use crate::event::WorkspaceId;
use crate::offer::{Offer, What};

/// How a bring card's last paragraph starts while another bring is under way (`queued_line`).
const QUEUED_SAID: &str = "\n\nAnother bring is under way";

/// What `space W` marked and the queue going, if one is.
#[derive(Default)]
pub struct Queue {
    /// Marked to bring, in the order marked.
    pub marks: Vec<WorkspaceId>,
    pub run: Option<Run>,
    /// Tests: the rebase and the preflight it would start as tasks are left to a simulation (which models no
    /// task) — it says how each ended (`bring_queue_rebased`, `rebase_preflight_done`); a single bring's preflight too
    /// (`rebase_preflight_then_bring`).
    #[cfg(test)]
    pub stand_in: bool,
}

/// A queue going: those still to come, the one under way and where it is, and what became of the rest.
#[derive(Default)]
pub struct Run {
    pub left: VecDeque<WorkspaceId>,
    pub now: Option<(WorkspaceId, Stage)>,
    /// Brought: (name, what it said — `3 commits, now at 1a2b3c4`).
    pub brought: Vec<(String, String)>,
    /// Not brought: (name, why).
    pub out: Vec<(String, String)>,
    /// The one under way was refused its fast-forward once (its base moved under it): looked at again.
    pub again: bool,
    pub total: usize,
    /// Each one's name as the card said it — one whose session ended is still said by it.
    pub names: Vec<(WorkspaceId, String)>,
}

/// Where the one under way is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Its checkout looked at (on a worker): moved, ready, a rebase there.
    Looking,
    /// Rebased onto its base's tip, in its task panel.
    Rebasing,
    /// Its quick preflight runs (or waits on its card).
    Preflight,
    /// Brought in (on a worker).
    Bringing,
}

impl Stage {
    fn said(self) -> &'static str {
        match self {
            Stage::Looking => "looking",
            Stage::Rebasing => "rebasing",
            Stage::Preflight => "preflight",
            Stage::Bringing => "bringing",
        }
    }
}

/// What the queue's `y` found before any preflight (#379), and what its turn finds before its own
/// (`bring_queue_check`).
struct Check {
    /// Changed in no commit there, untracked too (`rebase::unclean`).
    unclean: Vec<String>,
    /// High-risk changes it brings that no one looked at.
    highs: usize,
    /// The commit its preflight tests.
    head: Option<String>,
}

/// What the card asks about: the sessions, in the order they come.
#[derive(Clone, Debug)]
pub struct Ask {
    pub ids: Vec<WorkspaceId>,
    pub names: Vec<String>,
    /// Their bases, each once (`main`).
    pub bases: Vec<String>,
}

/// `1st`, `2nd`, `3rd`, `4th` …
fn nth(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// `a`, `a and b`, `a, b and c`.
fn and_list(names: &[String]) -> String {
    match names.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// The card for `ask`.
pub fn card(ask: Ask) -> crate::offer::Offer {
    let n = ask.names.len();
    let base = match ask.bases.as_slice() {
        [one] => one.clone(),
        _ => "their bases".into(),
    };
    let tip = match ask.bases.as_slice() {
        [one] => format!("{one}'s tip"),
        _ => "its base's tip".into(),
    };
    // (in the order they come: `a, b, then c`)
    let order = match ask.names.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{}, then {last}", rest.join(", ")),
        None => String::new(),
    };
    let q = format!(
        "Bring {n} sessions into {base}, one after another? {order}.\n\nEach in turn: rebased onto {tip} if it \
         moved, its quick preflight run there, then brought — the next starts once it lands. A conflict or a \
         failed preflight stops only that one; the rest go on.",
    );
    crate::offer::Offer::new(
        format!("Bring {n} into {base}"),
        &q,
        "Bringing them in turn…",
        "Couldn't start the bring queue".into(),
        crate::offer::What::BringQueue(Box::new(ask)),
    )
}

impl Editor {
    /// `m` in the list: session `id` marked to bring (again: unmarked) — said, with what `B` does.
    pub(crate) fn bring_mark(&mut self, id: WorkspaceId) {
        if id == WorkspaceId::HOME {
            return self.set_status("that's the checkout tarae started in — sessions are brought into it");
        }
        let name = self.bring_name(id);
        let marks = &mut self.bring_queue.marks;
        match marks.iter().position(|m| *m == id) {
            Some(i) => {
                marks.remove(i);
            }
            None => marks.push(id),
        }
        let n = marks.len();
        let said = match (marks.contains(&id), n) {
            (false, 0) => format!("{name} unmarked — nothing marked to bring"),
            (false, _) => format!("{name} unmarked — {n} marked: B brings them in turn"),
            (true, 1) => format!("{name} marked to bring — m on another, then B brings them in turn"),
            (true, _) => format!("{name} marked to bring {} — B brings the {n} in turn", nth(n)),
        };
        self.set_status(said);
    }

    /// `B` in the list: the marked ones asked about on one card — one marked is a bring of its own.
    /// A queue going: they line up behind it (#371).
    pub(crate) fn bring_queue_ask(&mut self) -> Result<(), String> {
        let live = |ed: &Editor, id: WorkspaceId| {
            std::iter::once(&ed.workspace).chain(&ed.workspaces).any(|l| l.id == id) && !ed.bring_queued(id)
        };
        let marks: Vec<WorkspaceId> =
            self.bring_queue.marks.iter().copied().filter(|id| live(self, *id)).collect();
        self.bring_queue.marks.clone_from(&marks);
        match marks.as_slice() {
            [] => Err("nothing marked — m marks the sessions to bring in turn, then B".into()),
            [one] => {
                self.bring_queue.marks.clear();
                self.workspace_bring(&one.to_string())
            }
            _ => {
                let start = self.start_root();
                let all = std::iter::once(&self.workspace).chain(&self.workspaces);
                let mut bases: Vec<String> = Vec::new();
                let mut names = Vec::new();
                for id in &marks {
                    let Some(l) = all.clone().find(|l| l.id == *id) else { continue };
                    names.push(l.label().to_string());
                    let base = l.base.clone().unwrap_or_else(|| start_branch(self, &start));
                    if !bases.contains(&base) {
                        bases.push(base);
                    }
                }
                self.push_offer(card(Ask { ids: marks, names, bases }));
                Ok(())
            }
        }
    }

    /// `y` on the card: they line up at once (behind a queue going; first asked, first brought), the marks they took
    /// cleared — and what each brings is graded on a worker: those holding high-risk changes no one looked at are
    /// named (#379), each waiting at its turn on its card before any preflight of its own (`bring_queue_check`).
    pub(crate) fn bring_queue_start(&mut self, ask: Ask) {
        self.bring_queue.marks.retain(|m| !ask.ids.contains(m));
        crate::log::line("session", "bring-queue", &format!("started: {}", ask.names.join(", ")));
        for (id, name) in ask.ids.iter().copied().zip(ask.names.iter().cloned()) {
            self.bring_line_up(id, name, false);
        }
        self.bring_queue_next();
        let (rules, start) = (crate::risk::Rules::of(&self.config), self.start_root());
        let all = std::iter::once(&self.workspace).chain(&self.workspaces);
        let graded: Vec<_> = ask
            .ids
            .iter()
            .filter_map(|id| all.clone().find(|l| l.id == *id))
            .map(|l| {
                let base = l.base.clone().unwrap_or_else(|| start_branch(self, &start));
                (l.label().to_string(), l.root.clone(), base, l.looked.clone())
            })
            .collect();
        self.events.shared_jobs().spawn(move || {
            let held: Vec<(String, usize)> = graded
                .iter()
                .filter_map(|(name, root, base, looked)| {
                    let highs = crate::codereview::highs_between(root, base, "HEAD", &rules).ok()?;
                    let n = crate::codereview::unseen(&highs, looked).len();
                    (n > 0).then(|| (name.clone(), n))
                })
                .collect();
            move |ed: &mut Editor| ed.bring_queue_graded(held)
        });
    }

    /// The queue's `y` graded (`held`: who holds high-risk changes no one looked at, and how many): said.
    fn bring_queue_graded(&mut self, held: Vec<(String, usize)>) {
        if held.is_empty() {
            return;
        }
        let names: Vec<String> = held
            .iter()
            .map(|(name, n)| format!("{name} ({})", crate::nav::count(*n, "high-risk change")))
            .collect();
        self.set_warning(format!(
            "Not looked at yet: {} — each waits at its turn on its bring card (h goes to them), no preflight run; \
             the rest go on",
            and_list(&names)
        ));
    }

    /// Session `id` (named `name`) put in line — at its end, or `first` at its front (a bring that went by itself
    /// before those waiting asked): its bring of its own let go, the queue's from here. False: it was in line.
    fn bring_line_up(&mut self, id: WorkspaceId, name: String, first: bool) -> bool {
        if self.bring_queued(id) {
            return false;
        }
        let run = self.bring_queue.run.get_or_insert_with(Run::default);
        match first {
            true => run.left.push_front(id),
            false => run.left.push_back(id),
        }
        run.total += 1;
        run.names.retain(|(n, _)| *n != id);
        run.names.push((id, name));
        self.bring_queue.marks.retain(|m| *m != id);
        // (its own bring card goes: the queue asks nothing more of it until it leaves the line — then its card)
        let root = std::iter::once(&self.workspace)
            .chain(&self.workspaces)
            .find(|l| l.id == id)
            .map(|l| l.root.clone());
        if let Some(root) = root {
            let its = |o: &Offer| o.bring_root() == Some(root.as_path());
            self.offers.retain(|o| !its(o));
            for l in std::iter::once(&mut self.workspace).chain(&mut self.workspaces) {
                l.parked.retain(|o| !its(o));
            }
        }
        self.with_workspace(id, |ed| {
            let l = &mut ed.workspace;
            (l.bring_go, l.bring_rebased, l.bring_after_preflight, l.bring_landing) =
                (false, false, false, false);
        });
        true
    }

    /// Whether a bring card's `y` on session `id` waits its turn: a queue going, or another session's bring going by
    /// itself (`bring_alone`).
    pub(crate) fn bring_waits(&self, id: WorkspaceId) -> bool {
        self.bring_queue.run.is_some() || self.bring_alone(Some(id)).is_some()
    }

    /// A bring card's `y` (or `p`) on session `id` while another bring is under way (`bring_waits`): it lines up
    /// behind those in line, its place said — the queue looks at it in its turn, on its base's tip then.
    pub(crate) fn bring_join(&mut self, id: WorkspaceId) {
        let name = self.bring_name(id);
        if !self.bring_line_up(id, name.clone(), false) {
            return self.set_status(format!("{name} is in line to be brought already — space W says where"));
        }
        let after = self.bring_ahead(&[id]).pop().map(|a| format!(", after {a}")).unwrap_or_default();
        self.set_status(format!(
            "{name} lines up to be brought — {} in line{after}",
            nth(self.bring_place(id))
        ));
        crate::log::line("session", "bring-queue", &format!("{name} joined, {}", nth(self.bring_place(id))));
        self.bring_queue_next();
    }

    /// A bring that went by itself (session `id`'s, its rebase just over) going on to its preflight while the queue's
    /// one is under way, or another going by itself: it lines up first instead — it asked before those waiting —
    /// and is looked at again in its turn. True when it did.
    pub(crate) fn bring_goes_first_in_line(&mut self, id: WorkspaceId) -> bool {
        let going = self.bring_queue.run.as_ref().is_some_and(|r| r.now.is_some());
        if !going && self.bring_alone(Some(id)).is_none() {
            return false;
        }
        let name = self.bring_name(id);
        self.bring_line_up(id, name.clone(), true);
        self.recorded(
            crate::editor::ToastKind::Info,
            format!("{name} waits its turn to be brought — {} in line", nth(self.bring_place(id))),
        );
        true
    }

    /// A bring going by itself (its card's `y`, not the queue's), other than session `but`'s, that the line waits for
    /// — not one stopped at its rebase's conflict or resolved there: that waits on a person, the line goes on.
    fn bring_alone(&self, but: Option<WorkspaceId>) -> Option<&crate::workspace::Workspace> {
        use crate::rebase::Stage as R;
        let stopped = |l: &crate::workspace::Workspace| {
            l.rebase.as_ref().is_some_and(|r| match r.stage {
                R::Running { .. } => false,
                R::Resolved => true,
                // (stopped before git ran: about to)
                R::Stopped => r.stop != crate::rebase::Stop::default(),
            })
        };
        std::iter::once(&self.workspace)
            .chain(&self.workspaces)
            .find(|l| Some(l.id) != but && self.bring_going(l) && !stopped(l) && !self.bring_queued(l.id))
    }

    /// The names ahead of the first of `ids` in line — the one under way (the queue's, or one going by itself), then
    /// those waiting before it (all of them when it isn't in line).
    fn bring_ahead(&self, ids: &[WorkspaceId]) -> Vec<String> {
        let run = self.bring_queue.run.as_ref();
        let mut names = Vec::new();
        match run.and_then(|r| r.now) {
            Some((n, _)) if !ids.contains(&n) => names.push(self.bring_name(n)),
            Some(_) => {}
            None => names.extend(self.bring_alone(None).map(|l| l.label().to_string())),
        }
        for n in run.iter().flat_map(|r| &r.left) {
            if ids.contains(n) {
                break;
            }
            names.push(self.bring_name(*n));
        }
        names
    }

    /// Session `id`'s place in line, from 1 (the one under way) — 0 when it isn't in it.
    fn bring_place(&self, id: WorkspaceId) -> usize {
        let Some(run) = self.bring_queue.run.as_ref() else { return 0 };
        if run.now.is_some_and(|(n, _)| n == id) {
            return 1;
        }
        let going = run.now.is_some() || self.bring_alone(None).is_some();
        run.left.iter().position(|n| *n == id).map_or(0, |i| i + 1 + usize::from(going))
    }

    /// How many are in line: the one under way (the queue's, or one going by itself) and those waiting.
    fn bring_in_line(&self) -> usize {
        let run = self.bring_queue.run.as_ref();
        let going = run.is_some_and(|r| r.now.is_some()) || self.bring_alone(None).is_some();
        usize::from(going) + run.map_or(0, |r| r.left.len())
    }

    /// What a bring card about `ids` (one session's, or the queue's) says while another bring is under way: who's
    /// ahead, and the place its `y` takes — None while none is.
    fn queued_line(&self, ids: &[WorkspaceId]) -> Option<String> {
        let ahead = self.bring_ahead(ids);
        let (first, rest) = ahead.split_first()?;
        let n = ahead.len();
        let places = match ids.len() {
            1 => nth(n + 1),
            2 => format!("{} and {}", nth(n + 1), nth(n + 2)),
            k => format!("{} to {}", nth(n + 1), nth(n + k)),
        };
        let who = match rest {
            [] => first.clone(),
            rest => format!("{first}, then {}", and_list(rest)),
        };
        let it = if ids.len() == 1 { "it" } else { "them" };
        let behind = if n == 1 { "it" } else { "them" };
        Some(format!("{} ({who}) — y lines {it} up behind {behind}: {places} in line.", &QUEUED_SAID[2..]))
    }

    /// Every bring card up says the line it would join as it changes (`queued_line`, its last paragraph) — the
    /// plain and the rebase card, the queue's; not the failed one (its `y` brings at once).
    fn bring_cards_sync(&mut self) {
        let line = |ed: &Editor, o: &Offer| -> Option<Option<String>> {
            let ids = match &o.what {
                What::Rebase(m) | What::Rebased(m) if o.warnings.is_empty() => vec![ed.session_at(&m.root)?],
                What::BringQueue(a) => a.ids.clone(),
                _ => return None,
            };
            let want = ed.queued_line(&ids);
            let has = o.question.rfind(QUEUED_SAID).map(|i| &o.question[i + 2..]);
            (has != want.as_deref()).then_some(want)
        };
        // (where: 0 the stack, 1 the front session's parked ones, 2 + n a session's behind)
        let mut fixes: Vec<(usize, usize, Option<String>)> = Vec::new();
        let stacks = [&self.offers, &self.workspace.parked]
            .into_iter()
            .chain(self.workspaces.iter().map(|l| &l.parked));
        for (s, cards) in stacks.enumerate() {
            for (i, o) in cards.iter().enumerate() {
                if let Some(want) = line(self, o) {
                    fixes.push((s, i, want));
                }
            }
        }
        for (s, i, want) in fixes {
            let cards = match s {
                0 => &mut self.offers,
                1 => &mut self.workspace.parked,
                n => &mut self.workspaces[n - 2].parked,
            };
            let o = &mut cards[i];
            if let Some(at) = o.question.rfind(QUEUED_SAID) {
                o.question.truncate(at);
            }
            if let Some(line) = want {
                o.question.push_str("\n\n");
                o.question.push_str(&line);
            }
            o.rev += 1;
        }
    }

    /// The next one looked at — or, none left, the queue over and said. A bring going by itself goes first: the
    /// line waits for it (`bring_queue_sync` goes on once it's over).
    fn bring_queue_next(&mut self) {
        loop {
            let Some(run) = self.bring_queue.run.as_ref() else { return };
            if run.now.is_some() {
                return;
            }
            if run.left.is_empty() {
                return self.bring_queue_end();
            }
            if self.bring_alone(None).is_some() {
                return;
            }
            let Some(run) = self.bring_queue.run.as_mut() else { return };
            let Some(id) = run.left.pop_front() else { return };
            run.now = Some((id, Stage::Looking));
            run.again = false;
            match self.bringing_queued(id) {
                Ok(()) => return,
                Err(e) => {
                    let name = self.bring_name(id);
                    if let Some(run) = self.bring_queue.run.as_mut() {
                        run.out.push((name, e));
                        run.now = None;
                    }
                }
            }
        }
    }

    /// Every one gone through: said — a success when each was brought, else a warning naming who wasn't.
    fn bring_queue_end(&mut self) {
        let Some(run) = self.bring_queue.run.take() else { return };
        let brought: Vec<&str> = run.brought.iter().map(|(n, _)| n.as_str()).collect();
        let out: Vec<String> = run.out.iter().map(|(n, why)| format!("{n} ({why})")).collect();
        crate::log::line("session", "bring-queue", &format!("over: brought {brought:?}, not {out:?}"));
        let brought_said = match brought.as_slice() {
            [] => "none brought".to_string(),
            b => format!("brought {}", b.join(", ")),
        };
        match out.is_empty() {
            true => self.set_success(format!("Bring queue done — {brought_said}")),
            false => self
                .set_warning(format!("Bring queue done — {brought_said} · not brought: {}", out.join("; "))),
        }
    }

    /// The one under way (`id`) is out of the queue — `why` said — and the next goes on.
    pub(crate) fn bring_queue_out(&mut self, id: WorkspaceId, why: String) {
        self.bring_queue_leave(id, why, true);
    }

    /// The one under way (`id`) leaves the line — `why` said, a warning when it's `out`, else calm (it waits on its
    /// card, which says what for) — and the next goes on.
    fn bring_queue_leave(&mut self, id: WorkspaceId, why: String, out: bool) {
        let Some(run) = self.bring_queue.run.as_mut().filter(|r| r.now.is_some_and(|(n, _)| n == id)) else {
            return;
        };
        run.now = None;
        let name = self.bring_name(id);
        let left = self.bring_queue.run.as_ref().map_or(0, |r| r.left.len());
        let next = if left == 0 { String::new() } else { " — the next goes on".into() };
        match out {
            true => self.set_warning(format!("Bring queue: {name} not brought — {why}{next}")),
            false => self.set_status(format!("Bring queue: {name} {why}{next}")),
        }
        if let Some(run) = self.bring_queue.run.as_mut() {
            run.out.push((name, why));
        }
        self.bring_queue_next();
    }

    /// The stage session `id` is at in the queue, if it's the one under way.
    pub(crate) fn bring_queue_at(&self, id: WorkspaceId) -> Option<Stage> {
        self.bring_queue.run.as_ref()?.now.filter(|(n, _)| *n == id).map(|(_, s)| s)
    }

    fn bring_queue_stage(&mut self, id: WorkspaceId, stage: Stage) {
        if let Some(run) = self.bring_queue.run.as_mut()
            && let Some(now) = run.now.as_mut().filter(|(n, _)| *n == id)
        {
            now.1 = stage;
        }
    }

    /// Whether session `id` is in the queue going (under way or still to come).
    pub(crate) fn bring_queued(&self, id: WorkspaceId) -> bool {
        self.bring_queue
            .run
            .as_ref()
            .is_some_and(|r| r.left.contains(&id) || r.now.is_some_and(|(n, _)| n == id))
    }

    /// What the list says of session `id`: where it is in the queue going, its place in line waiting (`bring 2nd`, the
    /// one under way 1st), or marked (`bring 3rd`: brought after those in line, in the order marked).
    pub(crate) fn bring_said(&self, id: WorkspaceId) -> Option<String> {
        if let Some(run) = self.bring_queue.run.as_ref() {
            if let Some((_, stage)) = run.now.filter(|(n, _)| *n == id) {
                let at = run.brought.len() + run.out.len() + 1;
                return Some(format!("bring queue {at}/{} · {}", run.total, stage.said()));
            }
            if run.left.contains(&id) {
                return Some(format!("bring {}", nth(self.bring_place(id))));
            }
        }
        let i = self.bring_queue.marks.iter().position(|m| *m == id)?;
        Some(format!("bring {}", nth(self.bring_in_line() + i + 1)))
    }

    pub(crate) fn bring_queue_stands_in(&self) -> bool {
        #[cfg(test)]
        return self.bring_queue.stand_in;
        #[cfg(not(test))]
        false
    }

    fn bring_name(&self, id: WorkspaceId) -> String {
        let all = std::iter::once(&self.workspace).chain(&self.workspaces);
        let queued = || {
            let run = self.bring_queue.run.as_ref()?;
            run.names.iter().find(|(n, _)| *n == id).map(|(_, name)| name.clone())
        };
        match all.into_iter().find(|l| l.id == id) {
            Some(l) => l.label().to_string(),
            None => queued().unwrap_or_else(|| format!("session {id}")),
        }
    }

    // ── What the bring's steps say back ──

    /// The checkout of the one under way looked at (`bringing_queued`'s worker): what comes next for it.
    pub(crate) fn bring_queue_looked(&mut self, id: WorkspaceId, found: crate::worktree::Bringing) {
        use crate::worktree::Bringing;
        if self.bring_queue_at(id) != Some(Stage::Looking) {
            return;
        }
        match found {
            // (its base moved: rebased onto its tip in its own task panel — the rebase card's `y`, unasked)
            Bringing::Moved(m) => {
                self.bring_queue_stage(id, Stage::Rebasing);
                if !self.bring_queue_stands_in() {
                    self.with_workspace(id, |ed| ed.rebase_check(m, false));
                }
            }
            Bringing::Ready(m) if m.ahead == 0 => {
                self.bring_queue_out(id, format!("nothing to bring — no commits past {}", m.base));
            }
            // (looked at first, the commit its preflight tests read then, as a single bring's `p` has it: that one
            // lands, or none — #373)
            Bringing::Ready(m) => self.bring_queue_check(id, m),
            // (one tarae didn't start: taken up, its card up — as a single bring has it)
            Bringing::Rebasing(stop) => {
                self.with_workspace(id, |ed| {
                    if ed.workspace.rebase.is_none() {
                        ed.rebase_adopt(stop);
                    }
                });
                let _ = self.rebase_offer(id);
                self.bring_queue_out(id, "a rebase is under way there — its card".into());
            }
            Bringing::Held(h) => self.bring_queue_unclean(id, *h),
            Bringing::Refused(e) => self.bring_queue_out(id, e),
            // (a look brings nothing)
            Bringing::Brought(_) | Bringing::Untested { .. } => {}
        }
    }

    /// The one under way (`id`) on its base's tip (`m`): its checkout looked at on a worker before its preflight —
    /// changes in no commit, high-risk changes no one looked at (#379), the commit it would test.
    fn bring_queue_check(&mut self, id: WorkspaceId, m: crate::rebase::Moved) {
        let rules = crate::risk::Rules::of(&self.config);
        let all = std::iter::once(&self.workspace).chain(&self.workspaces);
        let looked = all.into_iter().find(|l| l.id == id).map(|l| l.looked.clone()).unwrap_or_default();
        let (root, base) = (m.root.clone(), m.base.clone());
        self.events.shared_jobs().spawn(move || {
            // (git can't say what's high-risk: the bring itself will — and holds then)
            let highs = crate::codereview::highs_between(&root, &base, "HEAD", &rules);
            let check = Check {
                unclean: crate::rebase::unclean(&root).unwrap_or_default(),
                highs: highs.map(|h| crate::codereview::unseen(&h, &looked).len()).unwrap_or(0),
                head: crate::worktree::sha(&root, "HEAD").ok(),
            };
            move |ed: &mut Editor| ed.bring_queue_checked(id, m, check)
        });
    }

    /// What `bring_queue_check` found: its card up and the next going on — the held card for changes in no commit,
    /// the bring card with `h` while high-risk changes wait for a look (no preflight run to be thrown away) — else
    /// its quick preflight on the commit read.
    fn bring_queue_checked(&mut self, id: WorkspaceId, m: crate::rebase::Moved, check: Check) {
        if self.bring_queue_at(id) != Some(Stage::Looking) {
            return;
        }
        if !check.unclean.is_empty() {
            let h =
                crate::rebase::Held { moved: m, changed: check.unclean, unsaved: Vec::new(), bring: true };
            return self.bring_queue_unclean(id, h);
        }
        if check.highs > 0 {
            let mut quick = None;
            self.with_workspace(id, |ed| quick = ed.preflight_quick());
            let (root, base) = (m.root.clone(), m.base.clone());
            let why = format!(
                "waits on its bring card — {} not looked at, h goes to them",
                crate::nav::count(check.highs, "high-risk change")
            );
            self.bring_queue_leave(id, why, false);
            return crate::codereview::push_bring_card(self, id, m.bring_card(quick, false), root, base);
        }
        self.bring_queue_stage(id, Stage::Preflight);
        let mut failed = None;
        let stands_in = self.bring_queue_stands_in();
        self.with_workspace(id, |ed| {
            let l = &mut ed.workspace;
            (l.bring_after_preflight, l.bring_tested) = (true, check.head);
            // (a failed preflight puts its card back, saying what failed — as a single bring's)
            l.bring_card_after = Some(Box::new(m));
            if stands_in {
                return;
            }
            if let Err(e) = ed.preflight_start(crate::preflight::Set::Quick) {
                ed.workspace.bring_after_preflight = false;
                failed = Some(e);
            }
        });
        if let Some(e) = failed {
            self.bring_queue_out(id, format!("its preflight didn't start: {e}"));
        }
    }

    /// The one under way (`id`) has changes in no commit (`h`): its held card up, and out — a preflight would test
    /// them, the bring land its commits without them.
    fn bring_queue_unclean(&mut self, id: WorkspaceId, h: crate::rebase::Held) {
        self.push_offer(h.card());
        self.bring_queue_out(id, "changes not in a commit in its checkout — its card asks".into());
    }

    /// Session `id` brought in (`said`: its commits and tip): the next goes on.
    pub(crate) fn bring_queue_landed(&mut self, id: WorkspaceId, said: String) {
        if self.bring_queue_at(id).is_none() {
            return;
        }
        let name = self.bring_name(id);
        if let Some(run) = self.bring_queue.run.as_mut() {
            run.now = None;
            run.brought.push((name, said));
        }
        self.bring_queue_next();
    }

    /// Session `id`'s bring after its preflight didn't land (`found`): its base moved during the preflight
    /// (`Moved` — the check before the fast-forward) or under the fast-forward itself (`Refused`) — a move from
    /// outside the queue, which moves its base only by its one under way: looked at once more, rebased and
    /// preflighted again by itself; moved again, out with its card. A rebase found there (its card up), or changes in
    /// no commit there now (the held card), out.
    pub(crate) fn bring_queue_not_brought(&mut self, id: WorkspaceId, found: crate::worktree::Bringing) {
        use crate::worktree::Bringing;
        if self.bring_queue_at(id) != Some(Stage::Bringing) {
            return;
        }
        let again = |ed: &mut Editor| {
            ed.bring_queue.run.as_mut().is_some_and(|r| !std::mem::replace(&mut r.again, true))
        };
        let why = match found {
            Bringing::Brought(_) => return,
            // (its HEAD moved while its preflight ran: out, its card asking for the preflight again — #373)
            Bringing::Untested { m, tested, head } => {
                let why =
                    format!("{} moved while its preflight ran — its card asks to run it again", m.branch);
                self.bring_untested_card(id, m, &tested, &head);
                return self.bring_queue_out(id, why);
            }
            Bringing::Moved(_) | Bringing::Refused(_) if again(self) => {
                self.bring_queue_stage(id, Stage::Looking);
                if let Err(e) = self.bringing_queued(id) {
                    self.bring_queue_out(id, e);
                }
                return;
            }
            Bringing::Moved(m) => {
                let mut quick = None;
                self.with_workspace(id, |ed| quick = ed.preflight_quick());
                let why =
                    format!("{} moved again while it was checked — its card asks to rebase it again", m.base);
                self.push_offer(m.card(quick));
                why
            }
            Bringing::Refused(e) => e,
            Bringing::Ready(m) => format!("not brought into {}", m.base),
            Bringing::Held(h) => return self.bring_queue_unclean(id, *h),
            Bringing::Rebasing(stop) => {
                self.with_workspace(id, |ed| {
                    if ed.workspace.rebase.is_none() {
                        ed.rebase_adopt(stop);
                    }
                });
                let _ = self.rebase_offer(id);
                return self.bring_queue_out(id, "a rebase is under way there — its card".into());
            }
        };
        self.bring_queue_out(id, why);
    }

    /// The front session's rebase is over (`done`: rebased onto its base's tip; else why not): the one under way
    /// looked at again (its preflight next), or out. True when the queue took it — no bring card then.
    pub(crate) fn bring_queue_rebased(&mut self, done: Result<(), String>) -> bool {
        let id = self.workspace.id;
        if self.bring_queue_at(id) != Some(Stage::Rebasing) {
            return false;
        }
        match done {
            Ok(()) => {
                self.bring_queue_stage(id, Stage::Looking);
                if let Err(e) = self.bringing_queued(id) {
                    self.bring_queue_out(id, e);
                }
            }
            Err(why) => self.bring_queue_out(id, why),
        }
        true
    }

    /// The front session's preflight for a bring ended (`passed`): brought next, or out.
    pub(crate) fn bring_queue_preflight(&mut self, passed: bool, title: &str) {
        let id = self.workspace.id;
        if self.bring_queue_at(id) != Some(Stage::Preflight) {
            return;
        }
        match passed {
            true => self.bring_queue_stage(id, Stage::Bringing),
            false => self.bring_queue_out(id, format!("its {title} didn't pass")),
        }
    }

    /// Every event: the one under way whose session ended, or whose preflight was dropped on the way (its card
    /// declined, it didn't start — `bring_after_preflight` cleared with no end said), is out; none under way, the
    /// next goes once no bring going by itself holds the line; the bring cards up say the line they'd join.
    pub(crate) fn bring_queue_sync(&mut self) {
        self.bring_queue_go_on();
        self.bring_cards_sync();
    }

    fn bring_queue_go_on(&mut self) {
        let Some(run) = self.bring_queue.run.as_ref() else { return };
        let Some((id, stage)) = run.now else { return self.bring_queue_next() };
        let session = std::iter::once(&self.workspace).chain(&self.workspaces).find(|l| l.id == id);
        match session {
            None => self.bring_queue_out(id, "its session ended".into()),
            Some(l) if stage == Stage::Preflight && !l.bring_after_preflight => {
                self.bring_queue_out(id, "its preflight didn't run".into())
            }
            Some(_) => {}
        }
    }
}

/// The branch of the checkout tarae started in (a session's base when it names none).
fn start_branch(ed: &Editor, start: &std::path::Path) -> String {
    let main = std::iter::once(&ed.workspace).chain(&ed.workspaces).find(|l| l.root == start);
    main.and_then(|l| l.git_branch.clone()).unwrap_or_else(|| "its base".into())
}
