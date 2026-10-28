//! A word that names a state does its key on a press (motion's now-plan §B; the mouse plan's landing W, frictions
//! #147): `follow` · `paused` on a top line follows again or stops, the turn's word follows Claude's place, `▲ waits`
//! is `C-q`'s, the model's name `C-t`'s, a key hint's pair (`C-q answer`, `esc code`, `space l focus`) its key — and
//! the mouse over one lights it, as a list's row in hand (the selected ground, the accent), so it reads as a thing to
//! press before it's pressed. Each is recorded where it's drawn (`Screen.word_spots`, its spans with it); what a press
//! lands on is `mouse_under`'s (`Word::Does`), and the one lit is the same answer for the pointer's cell, so a word
//! under a card or a picker is neither lit nor pressed. Never a press that costs on a stray one: the mode, a save, a
//! weighty answer have no spot.

use crate::editor::Editor;
use crate::key::{Code, Key};
use crate::mouse::Surface;
use crate::theme::Style;
use crate::workspace::At;

/// What a press on a word does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Press {
    /// A key hint's pair: its keys as written (`keys`), typed where the keys are — a hint says the keys of where they
    /// are — only in normal mode (`refused`: in INSERT they'd be typed into the file); a sequence under way there (a
    /// `d`, a count) ends first, as any press ends it (`Editor::sequence_end` — the mouse's one rule), never joined.
    Keys(String),
    /// `▲ waits`, Claude's change waiting, the inbox's `· C-q`: `C-q`'s act — the card or the review waiting gets the
    /// keys, from wherever they are, in any mode (the card's key comes first in `handle_key`).
    Card,
    /// `follow` · `paused`: `C-f`'s act — paused or off, following again; following, off (`follow::toggle`).
    Follow,
    /// The turn's word and time: Claude's place followed (`now::press`, the status line's word's).
    Now,
    /// The model's name: another model (`C-t`'s picker).
    Model,
    /// A word on the status line: its command by name wherever the keys are (from the chat's input `space g` would be
    /// typed) — a `:` one as typed (`:procs`).
    Command(&'static str),
    /// A tab's name on the side panel's top line (`files · outline · git`): that tab, the keys there — `space e`'s,
    /// `space o`'s … act, never its close (a press never closes what it names).
    Side(crate::sidebar::Mode),
    /// A buffer's name at a code pane's top line's right end: that buffer, in that pane.
    Buffer(crate::document::DocId),
    /// A folder's or the file's name in a code pane's path (`tarae › src › main.rs`): its row in the side panel's files,
    /// opened and unfolded down to it, the keys there (`space e`'s).
    Reveal(std::path::PathBuf),
}

/// A word where it's drawn: its row, its columns, the surface it's on as `mouse_under` names it, what a press does,
/// and what's drawn there (lit span by span while the mouse is over it).
#[derive(Clone, Debug)]
pub struct Spot {
    pub y: usize,
    pub from: usize,
    pub to: usize,
    pub on: Surface,
    pub press: Press,
    pub spans: Vec<(String, Style)>,
    /// A key hint's: where the keys it says are typed — the surface it was drawn for (the chat its foot's while the chat
    /// had them, the code the way in to it; the slot its line's; the code its pane's line's). Pressed, the keys go there
    /// first: a hint never types into another surface.
    pub home: Option<At>,
}

impl Spot {
    /// The spot of `spans` drawn from (`x`, `y`) on `on`.
    pub fn new(on: Surface, press: Press, (x, y): (usize, usize), spans: Vec<(String, Style)>) -> Spot {
        use unicode_width::UnicodeWidthStr;
        let w: usize = spans.iter().map(|s| s.0.width()).sum();
        Spot { y, from: x, to: x + w, on, press, spans, home: None }
    }

    /// A key hint's spot, its keys typed at `home`.
    pub fn home(self, home: At) -> Spot {
        Spot { home: Some(home), ..self }
    }
}

/// The keys a hint writes as `said`, as typed — None where it names no one sequence: several keys side by side
/// (`j k`, `[r ]r`: each its own), a word (`type`). A sequence after a prefix (`space l`, `C-w j`), a motion's pair
/// (`[r`), an arrow (`↑`), a `:` command (typed, then `enter`).
pub fn keys(said: &str) -> Option<Vec<Key>> {
    let plain = |c: char| Key::plain(Code::Char(c));
    if let Some(cmd) = said.strip_prefix(':').filter(|c| !c.is_empty() && !c.contains(char::is_whitespace)) {
        let enter: Key = "enter".parse().ok()?;
        return Some(std::iter::once(plain(':')).chain(cmd.chars().map(plain)).chain([enter]).collect());
    }
    let words: Vec<&str> = said.split_whitespace().collect();
    let lead = words.first()?.parse::<Key>().ok();
    if words.len() > 1 && !lead.is_some_and(|k| k.code == Code::Char(' ') || k.ctrl || k.alt) {
        return None;
    }
    let one = |w: &str| -> Option<Vec<Key>> {
        if let Some(k) = Key::from_shown(w) {
            return Some(vec![k]);
        }
        let mut c = w.chars();
        match (c.next(), c.next(), c.next()) {
            (Some(a @ ('[' | ']')), Some(b), None) => Some(vec![plain(a), plain(b)]),
            _ => None,
        }
    };
    let mut out = Vec::new();
    for w in words {
        out.extend(one(w)?);
    }
    Some(out)
}

/// A press on word `i` of the frame last drawn: what it does.
pub fn press(ed: &mut Editor, i: usize) {
    let spot = ed.screen.word_spots.borrow().get(i).map(|s| (s.on, s.press.clone(), s.home));
    let Some((on, p, home)) = spot else { return };
    match p {
        Press::Keys(said) => {
            // (a sequence under way: ended on the press's Down, before this — `handle_mouse`, `Editor::sequence_end`)
            if let Some(why) = refused(ed, home) {
                return ed.set_status(format!("{why}: esc first — a word presses {said} in normal mode"));
            }
            // (the chat's input types what it's given: there only a chord, `enter` or `esc` — never letters into the draft)
            let typed = |k: &Key| !(k.ctrl || k.alt || matches!(k.code, Code::Enter | Code::Esc));
            if home == Some(At::Chat)
                && !crate::chatdoc::keyed(ed)
                && keys(&said).unwrap_or_default().iter().any(typed)
            {
                return ed.set_status(format!(
                    "{said} would be typed into the chat's draft — a word presses a chord, enter or esc there"
                ));
            }
            // (the keys to the surface the hint was drawn for first — the pane whose line it's on, focused or not; the
            // chat its foot's; the slot its line's: a pane's `enter` never the chat's send, the follow card's `C-c` never
            // the code's comment)
            let (was_at, was_focus) = (ed.workspace.viewer.at, ed.workspace.viewer.focus);
            if let Surface::PaneTop(id) = on
                && id != was_focus
            {
                ed.focus_pane(id);
            }
            if let Some(h) = home.filter(|&h| !ed.workspace.is_at(h)) {
                ed.workspace.viewer.at = h;
            }
            let there = (ed.workspace.viewer.at, ed.workspace.viewer.focus);
            for k in keys(&said).unwrap_or_default() {
                ed.handle_key(k);
            }
            // A hint that means "go there" (`space l focus`, `esc code`, `C-w j scroll`) moved the keys itself: they stay
            // where it took them. One that acts (`C-c stop`, `enter send`) acted where they were lent: they go back
            // where they were. A pane's line is F's rule, a press gives the keys to what it lands on: that pane focused,
            // the keys there, they stay (`enter go there` goes there)
            // (one that closed where it was lent — the slot's `close` — gives them back too: nothing's there to keep them)
            let lent = there != (was_at, was_focus) && !matches!(on, Surface::PaneTop(_));
            let gone = home.is_some_and(|h| !standing(ed, h));
            if lent && ((ed.workspace.viewer.at, ed.workspace.viewer.focus) == there || gone) {
                if ed.workspace.viewer.focus != was_focus {
                    ed.focus_pane(was_focus);
                }
                ed.workspace.viewer.at = was_at;
            }
        }
        Press::Card => {
            if let Ok(k) = "C-q".parse() {
                ed.handle_key(k);
            }
        }
        Press::Follow => crate::follow::toggle(ed),
        Press::Now => crate::now::press(ed),
        Press::Model => crate::models::picker(ed),
        Press::Command(name) => match name.strip_prefix(':') {
            Some(typed) => {
                if let Err(e) = crate::typed::execute(ed, typed) {
                    ed.set_error(e);
                }
            }
            None => ed.run_command_by_name(name),
        },
        Press::Side(mode) => ed.sidebar_tab(mode),
        Press::Buffer(id) => {
            if let Surface::PaneTop(p) = on
                && p != ed.workspace.viewer.focus
            {
                ed.focus_pane(p);
            }
            ed.workspace.viewer.at = At::Code;
            if let Some(i) = ed.docs.iter().position(|d| d.id == id) {
                ed.current = i;
            }
        }
        Press::Reveal(path) => ed.sidebar_reveal(&path),
    }
}

/// Whether the surface `at` names is still there to hold the keys (a slot closed, a chat closed: not).
fn standing(ed: &Editor, at: At) -> bool {
    match at {
        At::Slot => ed.slot().is_some(),
        At::Chat => ed.workspace.claude.chat.is_some(),
        _ => true,
    }
}

/// The key hints a press does, by what they say they do (a remapped key moves only the key): the ways somewhere — the
/// chat, the code, a view, the slot's way in, a run's output, its failure, its latest, a place —, `enter send` and
/// `queue`, stop, a run's own (again, close), follow, the card waiting, full screen and back, replay. Never one that
/// costs on a stray click: a new conversation (`C-l new`), a turn undone, a change applied, the screen or a point sent,
/// what goes with a message taken off.
const PRESSABLE: &[&str] = &[
    "focus",
    "open the chat",
    "close",
    "code",
    "normal",
    "view",
    "type",
    "scroll",
    "send",
    "queue",
    "stop",
    "stop it",
    "again",
    "output",
    "failure",
    "latest",
    "go to it",
    "go there",
    "next",
    "list",
    "replay",
    "replay what it did",
    "follow",
    "unfollow",
    "answer card",
    "full screen",
    "back",
];

/// Whether a key hint — its key as written and what it says it does — is a word that does its key: one sequence
/// (`keys`), its meaning one a stray press can't cost (`PRESSABLE`).
pub fn pressable(key: &str, what: &str) -> bool {
    keys(key).is_some() && PRESSABLE.contains(&what.trim())
}

/// Why a key hint's keys can't go as typed now, said: the code where they go (`home`, else where the keys are) in a
/// mode other than normal (they'd be typed into the file). None: they go (a sequence under way ended first — `press`).
fn refused(ed: &Editor, home: Option<At>) -> Option<String> {
    use crate::uistate::Region;
    let code = match home {
        Some(h) => h == At::Code,
        None => matches!(ed.keys_to().region, Region::Code | Region::Weave | Region::Page),
    };
    let mode = format!("{:?}", ed.mode).to_lowercase();
    (code && ed.mode != crate::editor::Mode::Normal).then(|| format!("{mode} mode"))
}

/// A word lit under the pointer: its row, its columns, what's drawn there.
pub type Lit = (usize, usize, usize, Vec<(String, Style)>);

/// What lights with the mouse at (`x`, `y`) on the frame last drawn: the word a press there lands on — `mouse_under`'s
/// answer, so one under a card, a picker or a floating pane is none — as it was drawn: one of these spots, or a word
/// with a record of its own (a session's tab, the status line's chip, a slot's tab, a preflight's step chip —
/// `Screen.word_lit`). Not the three
/// whose hover says more: what Claude is doing now (its card), a mark of the strip (its words) and a chip over the
/// chat's input (what goes).
pub fn lit(ed: &Editor, x: usize, y: usize) -> Option<Lit> {
    use crate::mouse::{Part, Word};
    match ed.mouse_under(crate::event::MouseKind::Down, x, y).part {
        Part::Word(Word::Does(i)) => {
            ed.screen.word_spots.borrow().get(i).map(|s| (s.y, s.from, s.to, s.spans.clone()))
        }
        Part::Word(Word::Now | Word::Stop(_) | Word::With(_)) => None,
        Part::Word(_) => {
            let lit = ed.screen.word_lit.borrow();
            lit.iter().rev().find(|l| l.0 == y && (l.1..l.2).contains(&x)).cloned()
        }
        _ => None,
    }
}

/// The mouse at (`x`, `y`) (`Event::Hover`): over a word, it's lit. Whether that changed — a move that changes nothing
/// draws nothing.
pub fn hovered(ed: &mut Editor, x: usize, y: usize) -> bool {
    let over = lit(ed, x, y).map(|l| (l.0, l.1, l.2));
    ed.word_hover.replace(over) != over
}

#[cfg(test)]
mod tests;
