use crate::editor::Editor;
use crate::event::{Mouse, MouseKind};
use crate::key::{Code, Key};
use crate::offer::{Offer, What};
use crate::pane::GroupKind;
use crate::uistate::Region;
use crate::workspace::Here;

/// A toast's kind's setter (`set_status`, …).
type Say = fn(&mut Editor, String);

/// Every toast tarae's code says, from where it's made (#482): each `set_status`, `set_success`, `set_warning`,
/// `set_error` and `say` of a text written at the call (`written`: its literals, each `{…}` a stand-in path, or a
/// `const`'s) — its kind's setter, the text and its file; and how many make theirs elsewhere (a variable, a function's
/// result), not walked.
fn toasts_said() -> (Vec<(Say, String, String)>, usize) {
    use crate::editor::ToastKind;
    let setters: [(&str, Say); 4] = [
        (".set_status(", |ed, s| ed.set_status(s)),
        (".set_success(", |ed, s| ed.set_success(s)),
        (".set_warning(", |ed, s| ed.set_warning(s)),
        (".set_error(", |ed, s| ed.set_error(s)),
    ];
    let kinds: [(&str, Say); 4] = [
        ("ToastKind::Info,", |ed, s| ed.say(ToastKind::Info, s)),
        ("ToastKind::Success,", |ed, s| ed.say(ToastKind::Success, s)),
        ("ToastKind::Warning,", |ed, s| ed.say(ToastKind::Warning, s)),
        ("ToastKind::Error,", |ed, s| ed.say(ToastKind::Error, s)),
    ];
    let (mut said, mut elsewhere) = (Vec::new(), 0);
    for (file, code) in crate::sim::lint::sources() {
        let mut at = |rest: &str, set: fn(&mut Editor, String)| match written(rest, &code)[..] {
            [] => elsewhere += 1,
            ref texts => said.extend(texts.iter().map(|t| (set, t.clone(), file.clone()))),
        };
        for (call, set) in setters {
            for (i, _) in code.match_indices(call) {
                at(&code[i + call.len()..], set);
            }
        }
        for (i, _) in code.match_indices(".say(") {
            let rest = code[i + ".say(".len()..].trim_start();
            if let Some((kind, set)) = kinds.iter().find(|(k, _)| rest.starts_with(k)) {
                at(&rest[kind.len()..], *set);
            }
        }
    }
    (said, elsewhere)
}

/// The texts a call's argument writes (`rest`, from after its `(`, in `code`): each literal in it — one, a
/// `format!`'s, each arm's of a `match` or an `if` — or a `const`'s of this file it names, each `{…}` a stand-in; none
/// where it's made elsewhere.
fn written(rest: &str, code: &str) -> Vec<String> {
    // (the argument: up to its call's `)`, past what's in literals)
    let (mut depth, mut end, mut quoted, mut escaped) = (0usize, rest.len(), false, false);
    for (i, c) in rest.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '(' | '[' | '{' if !quoted => depth += 1,
            ')' | ']' | '}' if !quoted && depth == 0 => {
                end = i;
                break;
            }
            ')' | ']' | '}' if !quoted => depth -= 1,
            _ => {}
        }
    }
    let arg = &rest[..end];
    let mut texts = Vec::new();
    let mut from = 0;
    while let Some(q) = arg[from..].find('"') {
        let Some((text, len)) = literal(&arg[from + q + 1..]) else { break };
        texts.push(text);
        from += q + 1 + len;
    }
    // (a `const` of this file's, named alone)
    let name = arg.trim();
    if texts.is_empty() && !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
        let def = format!("const {name}: &str = \"");
        if let Some(i) = code.find(&def) {
            texts.extend(literal(&code[i + def.len()..]).map(|t| t.0));
        }
    }
    texts
}

/// A literal's text from after its opening quote, each `{…}` a stand-in, and the bytes it took (its closing quote too).
fn literal(body: &str) -> Option<(String, usize)> {
    let (mut text, mut chars) = (String::new(), body.chars());
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next()? {
                'n' => text.push('\n'),
                't' => text.push('\t'),
                // (a line continued: its next line's indent left out)
                '\n' => {
                    let left: String = chars.clone().collect();
                    let trimmed = left.trim_start_matches([' ', '\t', '\n']);
                    chars = body[body.len() - trimmed.len()..].chars();
                }
                'u' => {
                    let hex: String = chars.by_ref().skip(1).take_while(|&c| c != '}').collect();
                    text.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                c => text.push(c),
            },
            c => text.push(c),
        }
    }
    let took = body.len() - chars.as_str().len();
    // (`{{` a brace; a `{…}` a stand-in — a path, as long as a common one; one right after a word's end none: a part
    // that goes on it, a plural's `s` or what follows when there's one — `freed{branch}{left}`)
    let (mut out, mut it) = (String::new(), text.chars().peekable());
    while let Some(c) = it.next() {
        match c {
            '{' if it.peek() == Some(&'{') => out.push(it.next()?),
            '}' if it.peek() == Some(&'}') => out.push(it.next()?),
            '{' => {
                it.by_ref().take_while(|&c| c != '}').for_each(drop);
                if out.chars().last().is_none_or(char::is_whitespace) {
                    out.push_str("src/lib.rs");
                }
            }
            c => out.push(c),
        }
    }
    Some((out, took))
}

/// The toasts the walk can't read, their text made away from the call — a count that only goes down. (177: step 4's
/// L2 lifted the agent gate, and with it `chat::send_from`'s hold said on the status line, `llm::held_for`'s words.)
const MADE_ELSEWHERE: usize = 177;

/// What's wrong with the notification card on the frame just drawn (80 columns): each toast on it read row by row — a
/// word not the text's (cut, or broken across rows) or its lead (`term::toast_lead`) not whole —, the newest `texts[0]`,
/// the one under it `texts[1]`. A routine message said on the status line instead: there whole.
fn card_wrong(ed: &Editor, texts: &[&str]) -> Option<String> {
    let (frame, fw, fh) = ed.last_frame.as_ref().unwrap();
    let screen = crate::screen::text(frame, *fw, *fh).unwrap_or_default();
    let words = |s: &str| s.split_whitespace().map(str::to_string).collect::<Vec<_>>();
    let Some((x, y, w, h)) = ed.screen.toast_card.get() else {
        let status = screen.lines().last().unwrap_or_default();
        let said = ed.screen.toast_said.get() && words(status).join(" ").contains(&words(texts[0]).join(" "));
        return (!said).then(|| format!("no card, not on the status line whole:\n{screen}"));
    };
    let rows: Vec<String> =
        screen.lines().skip(y).take(h).map(|r| r.chars().skip(x).take(w).collect()).collect();
    // (a toast's rows: from its bar to the next one's — the `+N earlier` row none's)
    let mut toasts: Vec<Vec<String>> = Vec::new();
    for r in &rows {
        let (bar, words_of) = (r.chars().nth(1) == Some('▎'), words(&r.replace('▎', " ")));
        match toasts.last_mut() {
            Some(t) if !bar => t.extend(words_of),
            _ => toasts.push(words_of),
        }
    }
    let card = rows.join("\n");
    for (text, shown) in texts.iter().zip(&toasts) {
        let own = words(text);
        if let Some(bad) = shown.iter().find(|w| *w != "…" && !own.contains(w)) {
            return Some(format!("`{bad}` isn't a word of {text:?} — cut or broken:\n{card}"));
        }
        // (its `·`s left out: one a break took isn't drawn)
        let lead: Vec<String> =
            crate::term::toast_words(crate::term::toast_lead(text)).into_iter().map(String::from).collect();
        let shown: Vec<String> = shown.iter().filter(|w| *w != "·").cloned().collect();
        if !shown.starts_with(&lead) {
            return Some(format!("its lead {:?} isn't whole:\n{card}", lead.join(" ")));
        }
    }
    None
}

/// #482: every toast tarae says leads with what happens, wraps at words and folds its details first — walked from
/// where each is made (`toasts_said`), at 80 columns: alone, beside the chat, and stacked two (#484c) beside it, over
/// the host's toast and under the masked one. A toast whose text the walk can't read is counted (`MADE_ELSEWHERE`).
#[test]
fn every_toast_leads_with_what_happens_at_80() {
    let (said, elsewhere) = toasts_said();
    assert!(said.len() > 500, "the walk finds the toasts: {}", said.len());
    let host = "uploads.skein.invalid refused for this session — space m s opens it";
    let masked = "GH_TOKEN masked → api.github.com · github.com · uploads.github.com only, until you quit — \
                  :granted lists it";
    let mut ed = editor();
    let mut wrong = Vec::new();
    for (set, text, file) in &said {
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        // (alone, then beside the chat; stacked over the host's, then under the masked one)
        let ways: [(bool, Option<&str>, Option<&str>); 4] =
            [(false, None, None), (true, None, None), (true, Some(host), None), (true, None, Some(masked))];
        for (chat, under, over) in ways {
            ed.toasts.clear();
            ed.screen.toast_narrow.set(None);
            ed.workspace.claude.chat = chat.then(crate::chat::Chat::new);
            if let Some(u) = under {
                ed.set_status(u);
            }
            set(&mut ed, text.to_string());
            if let Some(o) = over {
                ed.set_success(o);
            }
            crate::term::render(&mut ed, &mut Vec::new(), 80, 43).unwrap();
            let texts: Vec<&str> = match (under, over) {
                (Some(u), _) => vec![text, u],
                (_, Some(o)) => vec![o, text],
                _ => vec![text],
            };
            if let Some(why) = card_wrong(&ed, &texts) {
                let way = match (chat, under, over) {
                    (false, ..) => "alone",
                    (_, Some(_), _) => "over the host's, beside the chat",
                    (_, _, Some(_)) => "under the masked one, beside the chat",
                    _ => "beside the chat",
                };
                wrong.push(format!("{file}: {text:?} ({way}): {why}"));
                break;
            }
        }
    }
    assert!(wrong.is_empty(), "{} toasts:\n\n{}", wrong.len(), wrong.join("\n\n"));
    assert!(
        elsewhere <= MADE_ELSEWHERE,
        "{elsewhere} toasts made away from their call, more than {MADE_ELSEWHERE}: write its text at the call"
    );
    assert!(
        elsewhere == MADE_ELSEWHERE,
        "{elsewhere} toasts made away from their call: lower MADE_ELSEWHERE to it"
    );
}

fn frame(ed: &mut Editor) -> String {
    ed.focus_sync();
    crate::term::render(ed, &mut Vec::new(), 160, 40).unwrap();
    crate::screen::text(ed.last_frame.as_ref().unwrap().0.as_slice(), 160, 40).unwrap()
}

/// #610: the status line carries the newest toast by when it was said (`term::status_toast`) — one said again after a
/// newer one keeps its place on the card's list, and is still the newest. Both on the card already: none waits.
#[test]
fn a_toast_said_again_after_a_newer_one_is_the_newest() {
    let mut ed = editor();
    ed.set_warning("disk full — nothing saved");
    ed.set_success("server ready");
    ed.set_warning("disk full — nothing saved");
    for t in &mut ed.toasts {
        t.shown = true;
    }
    assert_eq!(
        ed.toasts.last().map(|t| t.text.as_str()),
        Some("server ready"),
        "said again, it kept its place"
    );
    let carried = crate::term::status_toast(&ed).map(|t| t.text.clone());
    assert_eq!(carried.as_deref(), Some("disk full — nothing saved"));
}

/// Keys, a frame after each (`<C-w>`, `<esc>`, `<ret>` as written).
fn feed(ed: &mut Editor, keys: &str) {
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        let k: Key = match c {
            '<' => chars.by_ref().take_while(|&c| c != '>').collect::<String>().parse().unwrap(),
            c => Key::plain(Code::Char(c)),
        };
        ed.handle_key(k);
        frame(ed);
    }
}

fn editor() -> Editor {
    let mut ed = Editor::new(crate::config::Config::default());
    ed.docs[0].text = ropey::Rope::from_str("one\ntwo\nthree\n");
    ed
}

/// The card's rows as drawn, top to bottom, with the row above and the row below it.
fn around(s: &str, ed: &Editor) -> Vec<String> {
    let (x, y, w, h) = ed.screen.toast_card.get().expect("the card drawn");
    s.lines().skip(y - 1).take(h + 2).map(|l| l.chars().skip(x).take(w).collect()).collect()
}

fn press(ed: &mut Editor, x: usize, y: usize) {
    ed.handle_mouse(Mouse { kind: MouseKind::Down, x: x as u16, y: y as u16, alt: false, ctrl: false });
    frame(ed);
}

/// A toast is news, never a pane with the keys (the user, 2026-10-25): no frame — its rect is its rows, a message a
/// row, its kind's `▎` —, no name, no keys of its own; `y` is the code's yank; the keys stay the code's, and the state
/// lists it as a toast, never a pane.
#[test]
fn a_toast_takes_no_key_and_has_no_frame() {
    let mut ed = editor();
    // (the flash drawn, so `y`'s words are a note, no toast of theirs on the card — `motion.rs`)
    ed.motion_drawn = true;
    ed.set_status("saved a.rs");
    ed.set_error("no such command: 'wat'");
    let s = frame(&mut ed);
    let (_, _, _, h) = ed.screen.toast_card.get().expect("a card");
    assert_eq!(h, 2, "its rect is its rows, a message a row");
    let rows = around(&s, &ed);
    assert!(rows[1].contains("▎ no such command: 'wat'") && rows[2].contains("▎ saved a.rs"), "{rows:#?}");
    let frame_glyphs = ['╭', '╮', '╰', '╯', '─', '│'];
    assert!(rows.iter().all(|r| !r.contains(frame_glyphs)), "no frame, focused or not: {rows:#?}");
    assert!(
        rows.iter().all(|r| !r.contains("messages") && !r.contains("esc close")),
        "no name, no keys: {rows:#?}"
    );
    assert_eq!(ed.keys_to().region, Region::Code);
    let state = ed.ui_state();
    assert!(
        state.workspaces[0].panes.iter().all(|p| !p.id.starts_with("toast#")),
        "never a pane in the state"
    );
    assert!(state.toasts.len() == 2, "listed as toasts");
    feed(&mut ed, "y");
    assert!(ed.messages.last().unwrap().2.starts_with("yanked"), "y: the code's yank");
    assert_eq!(ed.workspace.here(), Here::Code);
}

/// `C-w w`'s round never stops on a toast: the panes, then a card waiting, then round again — one pane on screen, the
/// toast up, it says so and the keys stay the code's.
#[test]
fn the_round_never_stops_on_a_toast() {
    let mut ed = editor();
    ed.set_status("saved a.rs");
    frame(&mut ed);
    feed(&mut ed, "<C-w>w");
    assert_eq!(
        (ed.workspace.here(), ed.keys_to().region),
        (Here::Code, Region::Code),
        "one pane: no stop on the toast"
    );
    assert!(ed.messages.last().unwrap().2.starts_with("one pane on screen"), "{:?}", ed.messages.last());
    let o = Offer::new(
        "End the review?".into(),
        "2 files not viewed.",
        "",
        String::new(),
        What::ReviewEnd { session: crate::event::WorkspaceId::HOME, started: std::time::Instant::now() },
    );
    ed.push_offer(o);
    // (the cursor off the top rows: the toast takes the top right, the card the bottom)
    ed.docs[0].text = ropey::Rope::from_str(&"line\n".repeat(30));
    feed(&mut ed, "jjjjjjjj");
    ed.set_status("saved b.rs");
    frame(&mut ed);
    assert!(ed.screen.offer_card.is_some() && ed.screen.toast_card.get().is_some(), "both in sight");
    feed(&mut ed, "<C-w>w");
    assert!(ed.card_has_keys(), "the card");
    feed(&mut ed, "<C-w>w");
    assert!(
        !ed.card_has_keys() && ed.workspace.here() == Here::Code,
        "then round to the code, past the toast"
    );
    feed(&mut ed, "<C-w>w");
    assert!(ed.card_has_keys(), "the card again");
}

/// A press on a toast — fading too — opens `:messages` at the message pressed, with the keys; opening it lets the toasts
/// go. With a prompt open the press closes it, as `esc` — nothing more.
#[test]
fn a_press_on_a_toast_opens_messages() {
    let mut ed = editor();
    ed.set_status("one");
    ed.set_status("two");
    frame(&mut ed);
    let two = ed.toasts.last().unwrap().id;
    let one = two - 1;
    let at_one = ed.screen.toast_rows.borrow().iter().find(|r| r.1 == one).map(|r| r.0).unwrap();
    let (x, ..) = ed.screen.toast_card.get().unwrap();
    press(&mut ed, x + 3, at_one);
    assert!(ed.float_list_keys(GroupKind::Messages), "its home, with the keys");
    assert_eq!(ed.float_list_here(GroupKind::Messages), Some(one), "at the message pressed");
    assert!(ed.toasts.is_empty() && ed.screen.toast_card.get().is_none(), "the toasts let go");
    feed(&mut ed, "q");
    // Fading: its time all but run out, a press still opens its home
    ed.set_status("three");
    frame(&mut ed);
    let t = ed.toasts.last_mut().unwrap();
    t.at = std::time::Instant::now() - t.life + std::time::Duration::from_millis(100);
    let s = frame(&mut ed);
    assert!(ed.toast_alpha() < 1.0, "fading");
    let (x, y, ..) = ed.screen.toast_card.get().unwrap_or_else(|| panic!("drawn as it fades\n{s}"));
    press(&mut ed, x + 3, y);
    assert!(ed.float_list_keys(GroupKind::Messages), "a fading one too");
    feed(&mut ed, "q");
    // A prompt open: a press outside it closes it, as `esc` — nothing under it pressed (the mouse audit's Q2)
    ed.set_status("four");
    feed(&mut ed, ":");
    let (x, y, ..) = ed.screen.toast_card.get().expect("drawn");
    press(&mut ed, x + 3, y);
    assert!(ed.prompt.is_none() && ed.messages_pane.is_none(), "the prompt closed, no list");
}

/// A press on a toast while a card has the keys: `:messages` takes them, the card lets go (it waits, `C-q` focuses it
/// again) — never a list that looks lit while `card_key` still takes every key.
#[test]
fn a_press_on_a_toast_takes_the_keys_from_a_focused_card() {
    let mut ed = editor();
    let what =
        What::ReviewEnd { session: crate::event::WorkspaceId::HOME, started: std::time::Instant::now() };
    ed.push_offer(Offer::new("End the review?".into(), "2 files not viewed.", "", String::new(), what));
    // (the cursor off the top rows: the toast takes the top right, the card the bottom)
    ed.docs[0].text = ropey::Rope::from_str(&"line\n".repeat(30));
    feed(&mut ed, "jjjjjjjj");
    ed.set_status("one");
    ed.set_status("two");
    frame(&mut ed);
    feed(&mut ed, "<C-q>");
    assert!(ed.card_has_keys(), "the card focused");
    let (x, y, ..) = ed.screen.toast_card.get().expect("a toast beside it");
    press(&mut ed, x + 3, y);
    assert!(!ed.card_has_keys() && !ed.offers.is_empty(), "the card lets go, still waiting");
    assert_eq!(ed.keys_to().region, Region::List(GroupKind::Messages));
    let at = ed.float_list_here(GroupKind::Messages);
    feed(&mut ed, "k");
    assert_ne!(ed.float_list_here(GroupKind::Messages), at, "k moves the list, never the card's");
    assert!(!ed.offers.is_empty(), "the card unanswered");
}

/// An error stays up until a key of yours goes by after `ERROR_MIN` — that key goes on to where it goes (here the code's
/// `j`), never taken by the toast; a key before `ERROR_MIN` leaves it up.
#[test]
fn an_error_goes_with_your_key_and_the_key_goes_on() {
    let mut ed = editor();
    ed.set_error("it broke");
    frame(&mut ed);
    feed(&mut ed, "j");
    let t = ed.toasts.iter().find(|t| t.text == "it broke").expect("up");
    assert!(t.life > t.age() + std::time::Duration::from_secs(30), "a key before ERROR_MIN: still up");
    assert_eq!(ed.cursor_line(), 1, "the key went to the code");
    ed.toasts[0].at = std::time::Instant::now() - std::time::Duration::from_secs(5);
    feed(&mut ed, "j");
    assert_eq!(ed.cursor_line(), 2, "the key went to the code");
    let t = ed.toasts.iter().find(|t| t.text == "it broke").expect("fading");
    assert!(t.life <= t.age() + std::time::Duration::from_millis(400), "your key lets it go: it fades");
    assert_eq!(ed.keys_to().region, Region::Code);
}
