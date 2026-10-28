//! Key representation — read in helix config notation ("C-y", "A-x", "space", "ret"), shown by one name per key
//! (`Display`: `enter`, `-`, `↑` — #202).

use std::fmt;
use std::str::FromStr;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Code {
    Char(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
}

/// Shift isn't carried separately — char keys become uppercase, Tab becomes BackTab (same as helix) — but on
/// the page keys: `S-pageup`/`S-pagedown` scroll the terminal's past while its program has the keys, as every
/// terminal emulator's do (`shift`; elsewhere they're the page keys — `Keymaps::lookup`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    pub code: Code,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Key {
    pub const fn plain(code: Code) -> Self {
        Key { code, ctrl: false, alt: false, shift: false }
    }

    /// The key without its shift (the keymap's, for a shifted page key it has no binding for).
    pub const fn unshifted(self) -> Self {
        Key { shift: false, ..self }
    }

    pub fn from_event(ev: KeyEvent) -> Option<Key> {
        let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
        let code = match ev.code {
            // A terminal sends C-\ C-] C-^ C-_ as bytes 0x1c–0x1f, which read back as C-4 … C-7
            KeyCode::Char('4') if ctrl => Code::Char('\\'),
            KeyCode::Char('5') if ctrl => Code::Char(']'),
            KeyCode::Char('6') if ctrl => Code::Char('^'),
            KeyCode::Char('7') if ctrl => Code::Char('_'),
            KeyCode::Char(c) => Code::Char(c),
            KeyCode::Enter => Code::Enter,
            KeyCode::Esc => Code::Esc,
            KeyCode::Tab => Code::Tab,
            KeyCode::BackTab => Code::BackTab,
            KeyCode::Backspace => Code::Backspace,
            KeyCode::Delete => Code::Delete,
            KeyCode::Left => Code::Left,
            KeyCode::Right => Code::Right,
            KeyCode::Up => Code::Up,
            KeyCode::Down => Code::Down,
            KeyCode::Home => Code::Home,
            KeyCode::End => Code::End,
            KeyCode::PageUp => Code::PageUp,
            KeyCode::PageDown => Code::PageDown,
            KeyCode::F(n) => Code::F(n),
            _ => return None,
        };
        let shift =
            matches!(code, Code::PageUp | Code::PageDown) && ev.modifiers.contains(KeyModifiers::SHIFT);
        Some(Key { code, ctrl, alt: ev.modifiers.contains(KeyModifiers::ALT), shift })
    }

    /// The char, if this is a char key without modifiers.
    pub fn plain_char(&self) -> Option<char> {
        match self.code {
            Code::Char(c) if !self.ctrl && !self.alt => Some(c),
            _ => None,
        }
    }
}

/// A move through a panel's rows — a list's selection (the side panel, the review's rail, `:procs`, a plugin's
/// panel, `:granted`) or an output's top (the test, task and terminal panels): all take the same keys for it
/// (`list_move`), the code's for the same moves (uxaudit D7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListMove {
    By(isize),
    /// The view `by` rows (`C-e`/`C-y`), the selection staying unless it would leave the rows in sight — the code's
    /// act, Helix's (`in_sight`); an output's top moves as `By`.
    Scroll(isize),
    First,
    Last,
}

/// The rows a panel not drawn yet is taken to show (a page key's measure before the first frame).
pub const UNDRAWN_ROWS: usize = 20;

/// Half of `rows` in sight, a row at least — what `C-d`/`C-u` move everywhere (the code's, Helix's).
pub fn half(rows: usize) -> isize {
    (rows / 2).max(1) as isize
}

/// `j`/`k` (↓/↑, `C-n`/`C-p`) a row, `C-e`/`C-y` the view a row (`Scroll`), `C-d`/`C-u` half the `rows` in sight,
/// `C-f`/`C-b` (the page keys) all of them, `g`/`G` (Home/End) the ends — as the code's `C-d` `C-f` `C-e` (0 rows:
/// not drawn yet, `UNDRAWN_ROWS`).
pub fn list_move(key: Key, rows: usize) -> Option<ListMove> {
    use ListMove::*;
    let rows = if rows == 0 { UNDRAWN_ROWS } else { rows };
    let page = rows as isize;
    Some(match (key.code, key.ctrl, key.alt) {
        (Code::Char('j') | Code::Down, false, false) | (Code::Char('n'), true, false) => By(1),
        (Code::Char('k') | Code::Up, false, false) | (Code::Char('p'), true, false) => By(-1),
        (Code::Char('e'), true, false) => Scroll(1),
        (Code::Char('y'), true, false) => Scroll(-1),
        (Code::Char('d'), true, false) => By(half(rows)),
        (Code::Char('u'), true, false) => By(-half(rows)),
        (Code::Char('f'), true, false) | (Code::PageDown, ..) => By(page),
        (Code::Char('b'), true, false) | (Code::PageUp, ..) => By(-page),
        (Code::Char('g') | Code::Home, false, false) => First,
        (Code::Char('G') | Code::End, false, false) => Last,
        _ => return None,
    })
}

impl ListMove {
    /// The row it lands on from `at`, in a list of `n` (kept in it) — a `Scroll` leaves it (`in_sight` moves it).
    pub fn from(self, at: usize, n: usize) -> usize {
        let last = n.saturating_sub(1);
        match self {
            ListMove::By(by) => at.saturating_add_signed(by).min(last),
            ListMove::Scroll(_) => at.min(last),
            ListMove::First => 0,
            ListMove::Last => last,
        }
    }
}

/// A list's view scrolled `by` rows from `top`, `rows` in sight of `n` (kept in the list): the new top, and the
/// selection `at` where it was — or, where it would leave the rows in sight, the nearest one in them. `hidden`: rows
/// at the top a panel covers (the side panel's pinned folders).
pub fn in_sight(by: isize, top: usize, at: usize, n: usize, rows: usize, hidden: usize) -> (usize, usize) {
    let rows = if rows == 0 { UNDRAWN_ROWS } else { rows };
    let top = top.saturating_add_signed(by).min(n.saturating_sub(rows));
    let first = (top + hidden).min(top + rows - 1);
    (top, at.clamp(first, top + rows - 1).min(n.saturating_sub(1)))
}

impl FromStr for Key {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let (mut ctrl, mut alt, mut shift) = (false, false, false);
        let mut rest = s;
        // "C-", "A-", "S-" prefixes. Also works when the key itself is '-', as in "C--".
        while rest.len() > 2 && rest.as_bytes()[1] == b'-' {
            match rest.as_bytes()[0] {
                b'C' => ctrl = true,
                b'A' => alt = true,
                b'S' => shift = true,
                _ => break,
            }
            rest = &rest[2..];
        }
        let code = match rest {
            "space" => Code::Char(' '),
            "ret" | "enter" => Code::Enter,
            "esc" => Code::Esc,
            "tab" => Code::Tab,
            "backspace" => Code::Backspace,
            "del" => Code::Delete,
            "left" => Code::Left,
            "right" => Code::Right,
            "up" => Code::Up,
            "down" => Code::Down,
            "home" => Code::Home,
            "end" => Code::End,
            "pageup" => Code::PageUp,
            "pagedown" => Code::PageDown,
            "minus" => Code::Char('-'),
            "lt" => Code::Char('<'),
            "gt" => Code::Char('>'),
            f if f.len() > 1 && f.starts_with('F') && f[1..].parse::<u8>().is_ok() => {
                Code::F(f[1..].parse().unwrap())
            }
            _ => {
                let mut it = rest.chars();
                match (it.next(), it.next()) {
                    (Some(c), None) => {
                        // A name only shown (`↑`, copied from the reference) is a character no key sends: refused,
                        // its spelling said (#202) — `enter`, `-`, `<`, `>` read as the keys they show
                        let only_shown =
                            SHOWN.iter().find(|(code, _, said)| *said == rest && *code != Code::Char(c));
                        if let Some((_, notation, _)) =
                            only_shown.filter(|_| !broken(Broken::ShownReadAsChar))
                        {
                            return Err(format!("{s:?} — write {}{notation}", &s[..s.len() - rest.len()]));
                        }
                        Code::Char(c)
                    }
                    _ => return Err(format!("unknown key: {s:?}")),
                }
            }
        };
        let code = match (code, shift) {
            (Code::Char(c), true) => Code::Char(c.to_ascii_uppercase()),
            (Code::Tab, true) => Code::BackTab,
            (c, _) => c,
        };
        let shift = shift && matches!(code, Code::PageUp | Code::PageDown);
        Ok(Key { code, ctrl, alt, shift })
    }
}

/// The names that differ between the keymap file (Helix's notation, `FromStr`) and what's shown (`Display`) —
/// every other key is written the same in both.
const SHOWN: &[(Code, &str, &str)] = &[
    (Code::Enter, "ret", "enter"),
    (Code::Char('-'), "minus", "-"),
    (Code::Up, "up", "↑"),
    (Code::Down, "down", "↓"),
    (Code::Left, "left", "←"),
    (Code::Right, "right", "→"),
];

/// A break a trap makes, to see its test fail (test builds only — `broken` is false in others; one at a time,
/// `Broken::set`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Broken {
    #[cfg(test)]
    No,
    /// `Display` writes the keymap's notation (`every_key_is_shown_by_its_one_name`).
    ShownAsNotation,
    /// `notation` writes the shown name: a plugin hears `enter` (`a_plugin_hears_keys_in_the_keymaps_notation`).
    NotationAsShown,
    /// `said` leaves a plugin's keys as it wrote them, `ret` (`every_key_is_shown_by_its_one_name`, its plugin's).
    SaidAsWritten,
    /// A config reads a shown arrow as the character it is (`every_shown_name_in_a_config_is_its_key_or_refused`).
    ShownReadAsChar,
}

#[cfg(test)]
thread_local! {
    static BROKEN: std::cell::Cell<Broken> = const { std::cell::Cell::new(Broken::No) };
}

/// Broken this way until it goes (dropped — a failed assertion too).
#[cfg(test)]
pub(crate) struct Back;

#[cfg(test)]
impl Drop for Back {
    fn drop(&mut self) {
        BROKEN.set(Broken::No);
    }
}

#[cfg(test)]
impl Broken {
    pub(crate) fn set(self) -> Back {
        BROKEN.set(self);
        Back
    }
}

#[cfg(test)]
fn broken(b: Broken) -> bool {
    BROKEN.get() == b
}

/// The shown names a config refuses, with their spelling there (`↑`, `up`) — the keymap reference says them.
#[cfg(test)]
pub(crate) fn spelled_apart() -> Vec<(&'static str, &'static str)> {
    let refused = |shown: &str| shown.parse::<Key>().is_err();
    SHOWN
        .iter()
        .filter(|(_, _, shown)| refused(shown))
        .map(|(_, notation, shown)| (*shown, *notation))
        .collect()
}

#[cfg(not(test))]
fn broken(_: Broken) -> bool {
    false
}

impl Key {
    /// The key in the keymap file's notation, Helix's (`ret`, `minus`, `up`) — what a plugin's `key` event names
    /// (the plugin API's word for it); everything shown is `Display`'s.
    pub fn notation(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, broken(Broken::NotationAsShown)).expect("a String takes it");
        s
    }

    /// A key as shown (`Display`) read back — a hint's word pressed (`wordspot`); the keymap's notation reads too.
    pub fn from_shown(s: &str) -> Option<Key> {
        // (the prefixes as `FromStr` takes them — `C--` is ctrl and `-`)
        let mut rest = s;
        while rest.len() > 2 && rest.as_bytes()[1] == b'-' && matches!(rest.as_bytes()[0], b'C' | b'A' | b'S')
        {
            rest = &rest[2..];
        }
        let mods = &s[..s.len() - rest.len()];
        let name =
            SHOWN.iter().find(|(_, _, shown)| *shown == rest).map_or(rest, |(_, notation, _)| notation);
        format!("{mods}{name}").parse().ok()
    }

    fn write(&self, f: &mut impl fmt::Write, shown: bool) -> fmt::Result {
        if self.ctrl {
            f.write_str("C-")?;
        }
        if self.alt {
            f.write_str("A-")?;
        }
        if self.shift {
            f.write_str("S-")?;
        }
        if let Some((_, notation, said)) = SHOWN.iter().find(|(c, ..)| *c == self.code) {
            return f.write_str(if shown { said } else { notation });
        }
        match self.code {
            Code::Char(' ') => f.write_str("space"),
            Code::Char(c) => write!(f, "{c}"),
            Code::Esc => f.write_str("esc"),
            Code::Tab => f.write_str("tab"),
            // (shift + tab: the keymap's name for it, Helix's and the one people say — #199)
            Code::BackTab => f.write_str("S-tab"),
            Code::Backspace => f.write_str("backspace"),
            Code::Delete => f.write_str("del"),
            Code::Home => f.write_str("home"),
            Code::End => f.write_str("end"),
            Code::PageUp => f.write_str("pageup"),
            Code::PageDown => f.write_str("pagedown"),
            Code::F(n) => write!(f, "F{n}"),
            Code::Enter | Code::Up | Code::Down | Code::Left | Code::Right => unreachable!("in SHOWN"),
        }
    }
}

/// The one name a key is shown by, wherever tarae shows it — which-key, the palette, hints, what a key refused says,
/// the keymap reference (#202): `enter`, `-`, `↑`, as people say them; the keymap file keeps Helix's notation
/// (`notation`), which `FromStr` reads.
impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f, !broken(Broken::ShownAsNotation))
    }
}

/// A sequence written in either notation (`ret`, `C-w minus`), each word that's a key as shown — what a plugin
/// names in its keys' hints; a word that isn't a key (`click`) stays.
pub fn said(seq: &str) -> String {
    if broken(Broken::SaidAsWritten) {
        return seq.to_string();
    }
    let word = |w: &str| w.parse::<Key>().map_or(w.to_string(), |k| k.to_string());
    seq.split(' ').map(word).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(s: &str) -> Key {
        s.parse().unwrap()
    }

    /// Every list panel moves by the same keys — the code's measures: half the rows in sight, a page all of them —
    /// and stays in its list.
    #[test]
    fn list_panels_move_by_the_same_keys() {
        // (8 rows in sight)
        let to = |key: &str, at: usize| list_move(k(key), 8).map(|m| m.from(at, 30));
        assert_eq!((to("j", 3), to("down", 3), to("C-n", 3)), (Some(4), Some(4), Some(4)));
        assert_eq!((to("k", 3), to("up", 3), to("C-p", 3)), (Some(2), Some(2), Some(2)));
        // (`C-e`/`C-y` scroll the view: the selection stays — `in_sight` keeps it there)
        let ce = |key: &str| list_move(k(key), 8);
        assert_eq!((ce("C-e"), ce("C-y")), (Some(ListMove::Scroll(1)), Some(ListMove::Scroll(-1))));
        assert_eq!(to("C-e", 3), Some(3));
        assert_eq!((to("C-d", 3), to("C-u", 10)), (Some(7), Some(6)), "half");
        assert_eq!(
            (to("C-f", 3), to("pagedown", 3), to("C-b", 10), to("pageup", 10)),
            (Some(11), Some(11), Some(2), Some(2)),
            "a page"
        );
        assert_eq!(
            (to("g", 12), to("home", 12), to("G", 2), to("end", 2)),
            (Some(0), Some(0), Some(29), Some(29))
        );
        assert_eq!((to("j", 29), to("C-d", 27)), (Some(29), Some(29)), "kept in the list");
        assert_eq!(list_move(k("j"), 8).map(|m| m.from(0, 0)), Some(0), "an empty list");
        assert_eq!((to("A-j", 3), to("x", 3), to("space", 3)), (None, None, None), "not a move");
        let one = |key: &str, rows| list_move(k(key), rows);
        assert_eq!(
            (one("C-d", 1), one("C-d", 0)),
            (Some(ListMove::By(1)), Some(ListMove::By(10))),
            "one row; undrawn"
        );
    }

    /// A scrolled view keeps the selection where it was, unless it would leave the rows in sight — then the nearest
    /// one (under what covers the top, too); the view stays in the list.
    #[test]
    fn a_scroll_keeps_the_selection_in_sight() {
        // (30 rows, 8 in sight)
        assert_eq!(in_sight(1, 0, 4, 30, 8, 0), (1, 4), "stays");
        assert_eq!(in_sight(1, 0, 0, 30, 8, 0), (1, 1), "would leave at the top: the nearest");
        assert_eq!(in_sight(-1, 5, 12, 30, 8, 0), (4, 11), "would leave at the bottom: the nearest");
        assert_eq!(in_sight(1, 22, 25, 30, 8, 0), (22, 25), "at the end the view stays");
        assert_eq!(in_sight(-1, 0, 3, 30, 8, 0), (0, 3), "at the top too");
        assert_eq!(in_sight(1, 0, 1, 30, 8, 2), (1, 3), "under two pinned rows: below them");
        assert_eq!(in_sight(1, 0, 2, 3, 8, 0), (0, 2), "a list shorter than its rows");
    }

    /// A shifted page key carries its shift (the terminal's past, `S-pageup`) — parsed, shown, from a terminal;
    /// every other key's shift is folded in as before.
    #[test]
    fn shift_is_carried_on_the_page_keys_only() {
        assert!(k("S-pageup").shift && k("S-pagedown").shift);
        assert_eq!(k("S-pageup").to_string(), "S-pageup");
        assert_eq!(k("S-pageup").unshifted(), k("pageup"));
        assert!(!k("S-up").shift && !k("S-x").shift);
        let ev = |code, m| Key::from_event(KeyEvent::new(code, m)).unwrap();
        assert_eq!(ev(KeyCode::PageDown, KeyModifiers::SHIFT), k("S-pagedown"));
        assert_eq!(ev(KeyCode::Home, KeyModifiers::SHIFT), k("home"));
    }

    #[test]
    fn parses_helix_notation() {
        assert_eq!(k("x"), Key::plain(Code::Char('x')));
        assert_eq!(k("A-x"), Key { code: Code::Char('x'), ctrl: false, alt: true, shift: false });
        assert_eq!(k("C-y"), Key { code: Code::Char('y'), ctrl: true, alt: false, shift: false });
        assert_eq!(k("A-,"), Key { code: Code::Char(','), ctrl: false, alt: true, shift: false });
        assert_eq!(k("C--"), Key { code: Code::Char('-'), ctrl: true, alt: false, shift: false });
        assert_eq!(k("-"), Key::plain(Code::Char('-')));
        assert_eq!(k("space"), Key::plain(Code::Char(' ')));
        assert_eq!(k("S-x"), Key::plain(Code::Char('X')));
        assert_eq!(k("S-tab"), Key::plain(Code::BackTab));
        assert_eq!(k("F12"), Key::plain(Code::F(12)));
        assert_eq!(k("ret"), Key::plain(Code::Enter));
        assert!("nope".parse::<Key>().is_err());
    }

    #[test]
    fn notation_roundtrips() {
        for s in
            ["x", "A-x", "C-y", "space", "minus", "ret", "C-A-backspace", "F3", "S-pagedown", "C-S-tab", "up"]
        {
            assert_eq!(k(s).notation(), s);
        }
    }

    /// #202: a key is shown by one name — `enter`, `-`, the arrows as `↑ ↓ ← →` — the rest as the keymap writes them;
    /// what's shown reads back as the same key (a hint's word pressed), and the keymap's notation reads as before.
    #[test]
    fn every_key_is_shown_by_one_name_that_reads_back() {
        let shown = [
            ("ret", "enter"),
            ("A-ret", "A-enter"),
            ("C-minus", "C--"),
            ("A-minus", "A--"),
            ("minus", "-"),
            ("up", "↑"),
            ("C-left", "C-←"),
            ("lt", "<"),
            ("gt", ">"),
            ("del", "del"),
            ("backspace", "backspace"),
            ("S-pageup", "S-pageup"),
            ("space", "space"),
            ("esc", "esc"),
            ("S-tab", "S-tab"),
        ];
        for (notation, said) in shown {
            assert_eq!(k(notation).to_string(), said, "{notation}");
            assert_eq!(Key::from_shown(said), Some(k(notation)), "{said} reads back");
        }
        for key in every_kind() {
            assert_eq!(Key::from_shown(&key.to_string()), Some(key), "{key}");
            assert_eq!(key.notation().parse::<Key>(), Ok(key), "{}", key.notation());
        }
        assert_eq!(said("C-w minus"), "C-w -");
        assert_eq!(said("ret"), "enter");
        assert_eq!(said("click"), "click", "a word that isn't a key stays");
    }

    /// A key of every kind, with and without ctrl and alt (and the shifted page key).
    fn every_kind() -> Vec<Key> {
        let codes =
            [Code::Enter, Code::Esc, Code::Tab, Code::BackTab, Code::Backspace, Code::Delete, Code::Left]
                .into_iter()
                .chain([
                    Code::Right,
                    Code::Up,
                    Code::Down,
                    Code::Home,
                    Code::End,
                    Code::PageUp,
                    Code::PageDown,
                ])
                .chain([Code::F(5), Code::Char(' '), Code::Char('-'), Code::Char('x'), Code::Char('<')])
                .chain([Code::Char('>')]);
        let mods = [(false, false), (true, false), (false, true), (true, true)];
        let keys = codes.flat_map(|code| mods.map(|(ctrl, alt)| Key { code, ctrl, alt, shift: false }));
        keys.chain([k("S-pageup")]).collect()
    }

    /// Where a key's shown name (`Display`), written in a config, isn't that key nor refused with the key's spelling
    /// (which reads as it) — a dead binding, unsaid (#202's review: `↑` copied from the reference read as the
    /// character). Each a line naming it.
    fn shown_in_a_config() -> Vec<String> {
        let mut out = Vec::new();
        for key in every_kind() {
            let shown = key.to_string();
            match shown.parse::<Key>() {
                Ok(read) if read == key => {}
                Ok(read) => out.push(format!("{shown:?} reads as another key: {read:?}")),
                Err(e) => match e.rsplit_once(" — write ").map(|(_, w)| w.parse::<Key>()) {
                    Some(Ok(spelled)) if spelled == key => {}
                    _ => out.push(format!("{shown:?} refused without its spelling: {e}")),
                },
            }
        }
        out
    }

    /// #202's review: a name only shown (the arrows) is refused in a config with the key's spelling, `"↑" — write up`;
    /// every other shown name is the key it shows (`enter`, `-`, `C--`, `<`).
    #[test]
    fn every_shown_name_in_a_config_is_its_key_or_refused() {
        let off = shown_in_a_config();
        assert!(off.is_empty(), "{off:#?}");
        assert_eq!("↑".parse::<Key>(), Err("\"↑\" — write up".to_string()));
        assert_eq!("C-←".parse::<Key>(), Err("\"C-←\" — write C-left".to_string()));
        assert_eq!((k("enter"), k("-"), k("C--")), (k("ret"), k("minus"), k("C-minus")));
        // (a config's warning names where: `keys.normal.↑: "↑" — write up`)
        let table: toml::Table = toml::from_str("\"↑\" = \"move_line_up\"").unwrap();
        let mut warnings = Vec::new();
        crate::keymap::Node::from_toml(&table, "keys.normal", &mut warnings);
        assert_eq!(warnings, ["keys.normal.↑: \"↑\" — write up"]);
    }

    /// …and it would see a shown arrow read as the character it is.
    #[test]
    fn the_config_test_sees_a_shown_arrow_read_as_a_character() {
        let _back = Broken::ShownReadAsChar.set();
        let off = shown_in_a_config();
        assert!(off.iter().any(|l| l.starts_with("\"↑\" reads as another key")), "{off:#?}");
    }

    /// #199: shift + tab is `S-tab` wherever a key is shown — which-key, the palette, a card's refusal, `:config-show`,
    /// the keymap reference and a plugin's key all write `Display`'s — as the keymap writes it, Helix too, and people
    /// say it; one name for it (`backtab` was only the code's).
    #[test]
    fn shift_tab_is_named_s_tab_wherever_a_key_is_shown() {
        let ev = Key::from_event(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)).unwrap();
        assert_eq!((ev.to_string(), ev.notation()), ("S-tab".to_string(), "S-tab".to_string()));
        assert_eq!(k("S-tab"), ev);
        assert_eq!(
            (k("A-S-tab").to_string(), k("C-S-tab").to_string()),
            ("A-S-tab".into(), "C-S-tab".into())
        );
        assert!("backtab".parse::<Key>().is_err(), "one name");
    }

    /// C-\\ comes from a terminal as 0x1c, which reads back as C-4 — it's C-\\ (the terminal's way out).
    #[test]
    fn control_backslash_from_a_terminal() {
        let ev = KeyEvent::new(KeyCode::Char('4'), KeyModifiers::CONTROL);
        assert_eq!(
            Key::from_event(ev),
            Some(Key { code: Code::Char('\\'), ctrl: true, alt: false, shift: false })
        );
        assert_eq!("C-\\".parse::<Key>().ok(), Key::from_event(ev));
    }
}
