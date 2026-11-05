//! A Short's captions (`@note:<words> → <target>`; CLAUDE.md's Shorts bullet: "the way Shorts caption", and "it flows,
//! nothing pops"): in one place — the band under tarae's screen, centred in it (`at`): never over the code or a card —,
//! bold on a quiet backing —, a short phrase at
//! a time, whole as it comes (`phrase`: never a word at a time), its `{}` the target's text as it changes — the ask
//! line's question following the typing inside the phrase, laid out at its fullest so nothing moves (`shown`). A
//! caption stands from when its target is first on the screen at or after its mark until the target leaves it or the
//! next caption comes (`spans`), eased in and out over `FADE_MS` — the next easing in as the one before eases out, the
//! two crossing. Nothing is drawn round its target: the camera points (`camera.rs`). Pure: the drawing is `video.rs`'s.

/// A caption eases in and out this long (CLAUDE.md: ~0.2 s), and two cross this long.
pub(crate) const FADE_MS: u64 = 200;
const _: () = assert!(FADE_MS <= 300);
/// A caption's written words at most (its `{}` apart): past them it's said — a caption says a few.
pub(crate) const WORDS_MAX: usize = 4;
/// A phrase stands at least this long at the scene's own pace (about a phrase read at a glance): the phrases are
/// written for the time the scene gives them, never a hold added for them — one standing less is said (`short`).
pub(crate) const STAND_MIN_MS: u64 = 1_200;
/// A Short's last caption — its last beat, from when its target lands — stands at least this long before the end card
/// (the user, 2026-11-05: the last beat is the payoff, and gone ~1 s after it came it goes unread on a phone): the take
/// runs on at the scene's own pace and is cut later (`shot::TAIL_S`), nothing held. One shorter is said (`tail`).
pub(crate) const TAIL_MIN_MS: u64 = 2_000;
const _: () = assert!(TAIL_MIN_MS >= STAND_MIN_MS);

/// A rectangle in the video's px: its left, top, width and height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Px {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl Px {
    fn right(self) -> f32 {
        self.x + self.w
    }

    fn bottom(self) -> f32 {
        self.y + self.h
    }

    /// Whether `o` lies wholly inside this.
    pub(crate) fn holds(self, o: Px) -> bool {
        self.x <= o.x && self.y <= o.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }
}

/// A caption's mark as written: its words and its target (`<words> → <target>`, the last arrow: the words may hold one —
/// `enter → fix's workspace`), each trimmed; none where either is missing.
pub(crate) fn parse(note: &str) -> Option<(&str, &str)> {
    let (words, target) = note.rsplit_once(" → ")?;
    let (words, target) = (words.trim(), target.trim());
    (!words.is_empty() && !target.is_empty()).then_some((words, target))
}

/// How many words a caption says as written (its `{}` apart).
pub(crate) fn counted(written: &str) -> usize {
    written.split_whitespace().filter(|w| *w != "{}").count()
}

/// A caption's phrase as the take has it: what it says from when, in order — the written words, a `{}` the target's
/// text then (the ask line's question as it's typed).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Phrase {
    pub(crate) said: Vec<(u64, String)>,
}

impl Phrase {
    /// What it says at its fullest — its last text, the question typed whole —: what its place is laid out for.
    pub(crate) fn fullest(&self) -> &str {
        self.said.last().map_or("", |s| s.1.as_str())
    }
}

/// `written` as a phrase coming whole at `from`, its `{}` what the target says (`said`: each change, its ms and text, in
/// order) — nothing yet where it has said nothing by then; each later change a new text of the phrase.
pub(crate) fn phrase(written: &str, from: u64, said: &[(u64, String)]) -> Phrase {
    let fill = |s: &str| -> String {
        let words = written.split_whitespace().map(|w| if w == "{}" { s } else { w });
        words.filter(|w| !w.trim().is_empty()).collect::<Vec<_>>().join(" ")
    };
    if !written.split_whitespace().any(|w| w == "{}") {
        return Phrase { said: vec![(from, fill(""))] };
    }
    let now = said.iter().take_while(|s| s.0 <= from).last().map_or("", |s| s.1.as_str());
    let mut out = vec![(from, fill(now))];
    for (t, s) in said.iter().filter(|s| s.0 > from) {
        let text = fill(s);
        if out.last().is_some_and(|l| l.1 != text) {
            out.push((*t, text));
        }
    }
    Phrase { said: out }
}

/// How much of the phrase's fullest text shows `ms` in (characters from its start): as far as what it says then agrees
/// with it — laid out once at its fullest, a typed part grows in its place and nothing else moves. 0 before it comes.
pub(crate) fn shown(p: &Phrase, ms: u64) -> usize {
    let Some(now) = p.said.iter().take_while(|s| s.0 <= ms).last() else { return 0 };
    now.1.chars().zip(p.fullest().chars()).take_while(|(a, b)| a == b).count()
}

/// When a caption stands: from `from` to `to` (ms of the take; `u64::MAX` to the end), eased in from its start and out
/// from its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) from: u64,
    pub(crate) to: u64,
}

/// When each caption stands, from when its target is first found and when it leaves the screen (`None`: it stays to
/// the end), in mark order: until its target goes or the next caption comes, whichever is first — the next easing in
/// as it eases out. One that would stand less than `FADE_MS` is none.
pub(crate) fn spans(found: &[(u64, Option<u64>)]) -> Vec<Option<Span>> {
    (found.iter().enumerate())
        .map(|(k, &(from, gone))| {
            let next = found[k + 1..].iter().map(|f| f.0).filter(|&t| t >= from).min();
            let to = gone.unwrap_or(u64::MAX).min(next.unwrap_or(u64::MAX));
            (to >= from + FADE_MS).then_some(Span { from, to })
        })
        .collect()
}

/// How long a caption stands in a take of `end` ms: from its start to its end, or the take's.
pub(crate) fn stands(s: Span, end: u64) -> u64 {
    s.to.min(end).saturating_sub(s.from)
}

/// The captions standing less than `STAND_MIN_MS` in a take of `end` ms, each with how long it stands.
pub(crate) fn short(spans: &[Option<Span>], end: u64) -> Vec<(usize, u64)> {
    (spans.iter().enumerate())
        .filter_map(|(k, s)| Some((k, stands((*s)?, end))))
        .filter(|(_, ms)| *ms < STAND_MIN_MS)
        .collect()
}

/// The last caption to come (the latest `from`) and how long it stands in a take of `end` ms: the take's tail.
pub(crate) fn tail(spans: &[Option<Span>], end: u64) -> Option<(usize, u64)> {
    let (k, s) =
        (spans.iter().enumerate()).filter_map(|(k, s)| Some((k, (*s)?))).max_by_key(|(k, s)| (s.from, *k))?;
    Some((k, stands(s, end)))
}

/// How drawn a caption is `ms` into the take (0 none, 1 whole): eased in and out (`camera::ease_in_out`) over `FADE_MS`
/// from its start, and out over as long from its end.
pub(crate) fn drawn(s: Span, ms: u64) -> f32 {
    if ms < s.from || ms >= s.to.saturating_add(FADE_MS) {
        return 0.0;
    }
    let ease = |t: u64| crate::camera::ease_in_out(t as f32 / FADE_MS as f32);
    let came = ease(ms - s.from);
    if ms < s.to { came } else { ease(s.to - s.from).min(came) * (1.0 - ease(ms - s.to)) }
}

/// The captions drawn `ms` in and how much (one standing, or two crossing — the one going first).
pub(crate) fn standing(spans: &[Option<Span>], ms: u64) -> Vec<(usize, f32)> {
    (spans.iter().enumerate())
        .filter_map(|(k, s)| Some((k, drawn((*s)?, ms))))
        .filter(|(_, a)| *a > 0.0)
        .collect()
}

/// The caption's place, its box `size` px, in the `band` under tarae's screen: centred across it and down it. One
/// taller than the band stands from the band's top — never up over the screen —, and is said (`band.holds`).
pub(crate) fn at(band: Px, (w, h): (f32, f32)) -> Px {
    let x = band.x + (band.w - w) / 2.0;
    let y = band.y + ((band.h - h) / 2.0).max(0.0);
    Px { x, y, w, h }
}

/// A point's signed distance from a rounded rectangle's edge (negative inside), its corners `r` round.
pub(crate) fn rounded(rect: Px, r: f32, (x, y): (f32, f32)) -> f32 {
    let r = r.min(rect.w / 2.0).min(rect.h / 2.0);
    let qx = (x - rect.x - rect.w / 2.0).abs() - (rect.w / 2.0 - r);
    let qy = (y - rect.y - rect.h / 2.0).abs() - (rect.h / 2.0 - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(x: f32, y: f32, w: f32, h: f32) -> Px {
        Px { x, y, w, h }
    }

    /// A caption's mark is its words, an arrow, its target — the last arrow, the words may hold one —; only the written
    /// words count against `WORDS_MAX`.
    #[test]
    fn a_caption_is_its_words_an_arrow_and_its_target() {
        assert_eq!(parse("you ask: {} → card:ask claude|44"), Some(("you ask: {}", "card:ask claude|44")));
        assert_eq!(
            parse("enter → fix's workspace → fix › src"),
            Some(("enter → fix's workspace", "fix › src"))
        );
        assert_eq!(parse("you ask"), None, "no target");
        assert_eq!(parse(" → card:x"), None, "no words");
        assert_eq!(counted("you ask: {}"), 2);
    }

    /// A phrase comes whole — every written word at its coming, never a word at a time —; its `{}` follows what the
    /// target says, typed letter by letter in its place: laid at its fullest, it shows as far as what's said agrees.
    #[test]
    fn a_caption_comes_as_a_whole_phrase_its_typed_part_as_typed() {
        let p = phrase("main's Claude writes", 1_000, &[]);
        assert_eq!(p.said, [(1_000, "main's Claude writes".to_string())]);
        assert_eq!(shown(&p, 999), 0);
        assert_eq!(shown(&p, 1_000), "main's Claude writes".len(), "whole at once");
        let said = [
            (900, "ask Claude…".to_string()),
            (1_100, "k".into()),
            (1_200, "k".into()),
            (1_600, "keep".into()),
            (1_900, "keep the".into()),
        ];
        let p = phrase("you ask: {}", 1_000, &said);
        assert_eq!(p.fullest(), "you ask: keep the");
        assert_eq!(
            p.said.iter().map(|s| s.0).collect::<Vec<_>>(),
            [1_000, 1_100, 1_600, 1_900],
            "each change its own text"
        );
        assert_eq!(
            shown(&p, 1_000),
            "you ask: ".len(),
            "what isn't the question yet (a placeholder) not shown"
        );
        assert_eq!(shown(&p, 1_100), "you ask: k".len());
        assert_eq!(shown(&p, 1_700), "you ask: keep".len());
        assert_eq!(shown(&p, 5_000), p.fullest().len());
        assert_eq!(
            phrase("you ask: {}", 1_000, &[]).said[0].1,
            "you ask:",
            "nothing said: the written words"
        );
    }

    /// A caption stands until its target goes or the next comes; the next eases in as it eases out, the two crossing
    /// over `FADE_MS` and never more than two drawn; one that would stand less than `FADE_MS` is none.
    #[test]
    fn the_next_caption_crosses_the_one_before() {
        let laid = spans(&[(1_000, Some(5_000)), (2_000, Some(2_500)), (4_000, None), (4_100, None)]);
        assert_eq!(
            laid,
            [
                Some(Span { from: 1_000, to: 2_000 }),
                Some(Span { from: 2_000, to: 2_500 }),
                None,
                Some(Span { from: 4_100, to: u64::MAX }),
            ]
        );
        for ms in (0..6_000).step_by(10) {
            let n = standing(&laid, ms).len();
            assert!(n <= 2, "{n} drawn at {ms} ms");
        }
        let crossing = standing(&laid, 2_000 + FADE_MS / 2);
        assert_eq!(crossing.iter().map(|s| s.0).collect::<Vec<_>>(), [0, 1], "the one going first");
        assert!((crossing[0].1 + crossing[1].1 - 1.0).abs() < 0.01, "crossing: {crossing:?}");
        assert_eq!(standing(&laid, 2_000 + FADE_MS), [(1, 1.0)], "crossed");
    }

    /// A phrase stands `STAND_MIN_MS` at least — till the next comes, its target goes or the take ends —; one shorter
    /// is named with how long it stands, one the next took out before it stood isn't counted twice.
    #[test]
    fn a_phrase_stands_long_enough_to_read() {
        let laid = spans(&[(1_000, None), (2_200, Some(3_000)), (3_000, None), (3_100, None), (9_000, None)]);
        assert_eq!(stands(laid[0].unwrap(), 20_000), STAND_MIN_MS, "just long enough");
        assert_eq!(laid[2], None, "the next came before it stood");
        assert_eq!(short(&laid, 10_000), [(1, 800), (4, 1_000)], "its target gone; the take's end");
        assert_eq!(short(&laid, 10_200), [(1, 800)]);
    }

    /// A take's tail is its last caption's stand, from its target's landing to the end card or its target's going: the
    /// latest to come, whatever its mark's order; none with no caption.
    #[test]
    fn a_takes_tail_is_its_last_captions_stand() {
        let laid = spans(&[(1_000, None), (4_000, None), (9_000, None)]);
        assert_eq!(tail(&laid, 11_000), Some((2, 2_000)), "just long enough");
        assert!(tail(&laid, 10_900).is_some_and(|t| t.1 < TAIL_MIN_MS), "0.1 s short");
        let gone = spans(&[(1_000, None), (9_000, Some(10_000))]);
        assert_eq!(tail(&gone, 12_000), Some((1, 1_000)), "its target gone before the end");
        assert_eq!(tail(&[None], 5_000), None);
    }

    /// A caption eases in and out over `FADE_MS`, gently at both ends, whole between, nothing outside.
    #[test]
    fn a_caption_eases_in_and_out() {
        let s = Span { from: 1_000, to: 3_000 };
        assert_eq!(drawn(s, 999), 0.0);
        assert!(drawn(s, 1_000 + FADE_MS / 10) < 0.05, "eased in, not popped: {}", drawn(s, 1_020));
        assert!((drawn(s, 1_000 + FADE_MS / 2) - 0.5).abs() < 0.01);
        assert_eq!(drawn(s, 2_000), 1.0);
        assert!((drawn(s, 3_000 + FADE_MS / 2) - 0.5).abs() < 0.01, "easing out");
        assert!(drawn(s, 3_000 + FADE_MS - FADE_MS / 10) < 0.05);
        assert_eq!(drawn(s, 3_000 + FADE_MS), 0.0);
        let short = Span { from: 1_000, to: 1_100 };
        assert!(drawn(short, 1_150) < drawn(short, 1_100), "out from where it had come to");
    }

    /// The caption stands in one place, the band: centred across it and down it whatever its width; one taller than the
    /// band starts at its top, never above it (over the screen), and the band doesn't hold it.
    #[test]
    fn a_caption_stands_in_the_middle_of_the_band() {
        let band = px(48.0, 1_300.0, 882.0, 220.0);
        let r = at(band, (500.0, 100.0));
        assert_eq!((r.x, r.y), (48.0 + 191.0, 1_300.0 + 60.0));
        assert!(band.holds(r));
        assert_eq!(at(band, (300.0, 100.0)).y, r.y, "one place, whatever its width");
        let tall = at(band, (500.0, 300.0));
        assert_eq!(tall.y, band.y, "never up over the screen");
        assert!(!band.holds(tall), "said");
        assert!(rounded(r, 18.0, (r.x, r.y)) > 0.0, "its corners round");
    }
}
