//! Selection model — the heart of helix. Editing is always "select → act"; a cursor is a 0–1 wide selection.
//!
//! All positions are **byte offsets** (always on char boundaries); ranges are half-open `[from, to)`.
//! tree-sitter, regex and LSP (UTF-8 negotiated) all speak bytes, so there is no conversion layer.
//! `anchor` is the fixed end, `head` the moving end. `head > anchor` means forward.

use ropey::Rope;

use crate::graphemes;
use crate::movement as mv;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub anchor: usize,
    pub head: usize,
}

impl Range {
    pub const fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub const fn point(pos: usize) -> Self {
        Self::new(pos, pos)
    }

    /// A mouse drag's selection: from the press's character `anchor` to the one under the pointer `pos`, both
    /// included (a range is half-open) — dragging back, the press's character still in it (`Held::dragged`, by
    /// characters).
    pub fn dragged(text: &Rope, anchor: usize, pos: usize) -> Range {
        Held::at(text, anchor, Unit::Char).dragged(text, pos)
    }

    pub fn from(&self) -> usize {
        self.anchor.min(self.head)
    }

    pub fn to(&self) -> usize {
        self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn is_forward(&self) -> bool {
        self.head >= self.anchor
    }

    /// Start of the char (cluster) under the block cursor. When forward, it's the char just before head.
    pub fn cursor(&self, text: &Rope) -> usize {
        if self.head > self.anchor { graphemes::prev_boundary(text, self.head) } else { self.head }
    }

    /// Normal mode: a 0-width selection covers the char (grapheme cluster) under the cursor (not at EOF).
    pub fn min_width_1(self, text: &Rope) -> Range {
        if self.is_empty() && self.head < text.len_bytes() {
            Range::new(self.head, graphemes::next_boundary(text, self.head))
        } else {
            self
        }
    }

    pub fn flip(self) -> Range {
        Range::new(self.head, self.anchor)
    }

    pub fn clamp(self, len: usize) -> Range {
        Range::new(self.anchor.min(len), self.head.min(len))
    }

    /// Move the cursor to char `pos`. `extend` grows, keeping the anchor-side char covered (select mode).
    pub fn put_cursor(self, text: &Rope, pos: usize, extend: bool) -> Range {
        if !extend {
            return Range::point(pos);
        }
        let anchor = if self.is_forward() && pos < self.anchor {
            graphemes::next_boundary(text, self.anchor)
        } else if !self.is_forward() && pos >= self.anchor {
            graphemes::prev_boundary(text, self.anchor)
        } else {
            self.anchor
        };
        if anchor <= pos {
            Range::new(anchor, graphemes::next_boundary(text, pos))
        } else {
            Range::new(anchor, pos)
        }
    }
}

/// What a mouse press takes by how many came at once on one character (`mouse::presses`): one its character, two
/// the word, three the line — a drag from it goes by the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Char,
    Word,
    Line,
}

impl Unit {
    /// The `n`th press at once: a fourth starts again at the character.
    pub fn of(n: usize) -> Unit {
        match n.saturating_sub(1) % 3 {
            0 => Unit::Char,
            1 => Unit::Word,
            _ => Unit::Line,
        }
    }
}

/// What a press holds for a drag from it (`Editor.mouse_anchor`): the text it took, `from..to` — its character, a
/// double's word, a triple's line — and the unit the drag goes by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    pub from: usize,
    pub to: usize,
    pub unit: Unit,
}

impl Held {
    /// The `unit` around `pos` (a word where there's none — a line's end, an empty text: its character).
    pub fn at(text: &Rope, pos: usize, unit: Unit) -> Held {
        let (from, to) = span(text, graphemes::snap(text, pos.min(text.len_bytes())), unit);
        Held { from, to, unit }
    }

    /// A press's character at `at` where the text isn't at hand (the prompt's line, a picker's query: they drag from
    /// `from` by their own rule, `prompt::drag`).
    pub fn char_at(at: usize) -> Held {
        Held { from: at, to: at, unit: Unit::Char }
    }

    /// What the press selects: a double's word, a triple's line — one press only puts the cursor there.
    pub fn range(self) -> Range {
        match self.unit {
            Unit::Char => Range::point(self.from),
            _ => Range::new(self.from, self.to),
        }
    }

    /// A drag from it to `pos`: what's between selected, the press's text and the unit under the pointer whole —
    /// dragging back, the press's own still in it, the head at the pointer's.
    pub fn dragged(self, text: &Rope, pos: usize) -> Range {
        let len = text.len_bytes();
        let (a, b) = (graphemes::snap(text, self.from.min(len)), graphemes::snap(text, self.to.min(len)));
        let (from, to) = span(text, graphemes::snap(text, pos.min(len)), self.unit);
        if from >= a { Range::new(a, to.max(b)) } else { Range::new(b, from) }
    }
}

/// The bytes of `unit` around `pos`.
fn span(text: &Rope, pos: usize, unit: Unit) -> (usize, usize) {
    let char = (pos, graphemes::next_boundary(text, pos));
    match unit {
        Unit::Char => char,
        Unit::Word => {
            let r = crate::textobject::word_object(
                text,
                Range::point(pos),
                crate::textobject::Kind::Inside,
                false,
            );
            if r.is_empty() { char } else { (r.from(), r.to()) }
        }
        Unit::Line => {
            let line = mv::line_of(text, pos);
            (text.line_to_byte(line), text.line_to_byte((line + 1).min(text.len_lines())))
        }
    }
}

/// Non-empty, sorted and merged list of ranges + primary selection index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    ranges: Vec<Range>,
    primary: usize,
}

impl Selection {
    pub fn new(ranges: Vec<Range>, primary: usize) -> Self {
        assert!(!ranges.is_empty(), "selection needs at least one range");
        let primary = primary.min(ranges.len() - 1);
        Self { ranges, primary }.normalized()
    }

    pub fn single(range: Range) -> Self {
        Self { ranges: vec![range], primary: 0 }
    }

    pub fn point(pos: usize) -> Self {
        Self::single(Range::point(pos))
    }

    pub fn ranges(&self) -> &[Range] {
        &self.ranges
    }

    pub fn primary(&self) -> Range {
        self.ranges[self.primary]
    }

    pub fn primary_index(&self) -> usize {
        self.primary
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn transform(&self, mut f: impl FnMut(Range) -> Range) -> Self {
        Self::new(self.ranges.iter().map(|&r| f(r)).collect(), self.primary)
    }

    /// Add a range and make it the primary selection.
    pub fn push(mut self, range: Range) -> Self {
        self.ranges.push(range);
        let primary = self.ranges.len() - 1;
        Self::new(self.ranges, primary)
    }

    pub fn keep_primary(&self) -> Self {
        Self::single(self.primary())
    }

    pub fn clamp(&self, len: usize) -> Self {
        self.transform(|r| r.clamp(len))
    }

    /// Sort, then merge overlapping ranges. Ranges that merely touch aren't merged (same as helix).
    fn normalized(self) -> Self {
        if self.ranges.len() == 1 {
            return self; // nothing to sort or merge — most selections, on every key
        }
        let primary = self.primary;
        let mut tagged: Vec<(Range, bool)> =
            self.ranges.into_iter().enumerate().map(|(i, r)| (r, i == primary)).collect();
        #[expect(
            clippy::disallowed_methods,
            reason = "the cursors, on every key that moves them (thousands)"
        )]
        tagged.sort_by_key(|(r, _)| (r.from(), r.to()));
        let mut out: Vec<(Range, bool)> = Vec::with_capacity(tagged.len());
        for (r, is_primary) in tagged {
            if let Some((last, last_primary)) = out.last_mut()
                && (r.from() < last.to() || r.from() == last.from())
            {
                let (from, to) = (last.from(), last.to().max(r.to()));
                *last = if last.is_forward() { Range::new(from, to) } else { Range::new(to, from) };
                *last_primary |= is_primary;
                continue;
            }
            out.push((r, is_primary));
        }
        let primary = out.iter().position(|(_, p)| *p).unwrap_or(0);
        Self { ranges: out.into_iter().map(|(r, _)| r).collect(), primary }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_is_char_before_head_when_forward() {
        let t = Rope::from_str("0123456789");
        assert_eq!(Range::new(2, 5).cursor(&t), 4);
        assert_eq!(Range::new(5, 2).cursor(&t), 2);
        assert_eq!(Range::point(3).cursor(&t), 3);
        // Byte positions: selecting through '한' (3 B) puts the cursor at the start of '한'
        let t = Rope::from_str("a한b");
        assert_eq!(Range::new(0, 4).cursor(&t), 1);
    }

    #[test]
    fn put_cursor_extend_keeps_anchor_char() {
        let t = Rope::from_str("0123456789");
        // Extending right from point (3) gives 3..=5
        assert_eq!(Range::point(3).put_cursor(&t, 5, true), Range::new(3, 6));
        // Extending left from point (3) gives 1..=3 (backward)
        assert_eq!(Range::point(3).put_cursor(&t, 1, true), Range::new(4, 1));
        // From forward 3..=5, going left past anchor — char 3 stays covered
        assert_eq!(Range::new(3, 6).put_cursor(&t, 1, true), Range::new(4, 1));
        // From backward 1..=3, going right past it
        assert_eq!(Range::new(4, 1).put_cursor(&t, 5, true), Range::new(3, 6));
        // Multibyte: extending while covering '한' (1..4), the bound is the char end
        let t = Rope::from_str("a한b");
        assert_eq!(Range::point(1).put_cursor(&t, 4, true), Range::new(1, 5));
    }

    #[test]
    fn normalize_merges_overlaps_and_tracks_primary() {
        let s = Selection::new(vec![Range::new(5, 8), Range::new(0, 2), Range::new(6, 10)], 2);
        assert_eq!(s.ranges(), &[Range::new(0, 2), Range::new(5, 10)]);
        assert_eq!(s.primary_index(), 1);
        // Touching ranges stay separate
        let s = Selection::new(vec![Range::new(0, 2), Range::new(2, 4)], 0);
        assert_eq!(s.len(), 2);
        // Two identical points become one
        let s = Selection::new(vec![Range::point(3), Range::point(3)], 1);
        assert_eq!(s.ranges(), &[Range::point(3)]);
    }
}
