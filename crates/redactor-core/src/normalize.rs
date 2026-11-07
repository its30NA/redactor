//! Shadow text: the input with invisible, bidi and tag characters removed
//! and NFKC applied per character, plus a byte map back to the original.

use std::borrow::Cow;
use std::ops::Range;

/// Classes of characters that are removed from the shadow text (and from the output when
/// `strip_invisible` is on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hidden {
    Invisible,
    Bidi,
    Tag,
}

/// Counts of removed characters, for `warnings`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HiddenCounts {
    pub invisible: usize,
    pub bidi: usize,
    pub tag: usize,
}

impl HiddenCounts {
    fn add(&mut self, class: Hidden) {
        match class {
            Hidden::Invisible => self.invisible += 1,
            Hidden::Bidi => self.bidi += 1,
            Hidden::Tag => self.tag += 1,
        }
    }
}

/// The detection view of a text.
///
/// NFKC is applied character by character (so `ｐａｓｓ` becomes `pass` and `ﬁ` becomes `fi`),
/// not across character boundaries: a decomposed `n` + U+0303 stays decomposed. Every byte of
/// the shadow text maps back to exactly one original character.
#[derive(Debug)]
pub struct Shadow<'a> {
    original: &'a str,
    /// Normalized text the detectors run on.
    pub text: Cow<'a, str>,
    /// Sparse map from shadow bytes to original bytes, sorted by shadow offset. Empty when
    /// the shadow is identical to the original.
    segs: Vec<Seg>,
    pub counts: HiddenCounts,
}

/// From `shadow` on (up to the next segment), shadow bytes map to original bytes: one to
/// one from `original` when `linear`, else all to the one original character at `original`
/// (a character NFKC changed).
#[derive(Clone, Copy, Debug)]
struct Seg {
    shadow: usize,
    original: usize,
    linear: bool,
}

impl<'a> Shadow<'a> {
    pub fn new(original: &'a str) -> Shadow<'a> {
        if original.is_ascii() {
            return Shadow::identity(original, HiddenCounts::default());
        }
        let bytes = original.as_bytes();
        let mut text = String::with_capacity(original.len());
        let mut segs: Vec<Seg> = Vec::new();
        let mut counts = HiddenCounts::default();
        let mut prev: Option<char> = None;
        // While `in_run`, shadow and original advance in step (the last segment is linear).
        let mut in_run = false;
        let mut i = 0;
        while i < bytes.len() {
            // ASCII runs are copied whole.
            let ascii = bytes[i..]
                .iter()
                .position(|&b| b >= 0x80)
                .unwrap_or(bytes.len() - i);
            if ascii > 0 {
                if !in_run {
                    segs.push(Seg {
                        shadow: text.len(),
                        original: i,
                        linear: true,
                    });
                    in_run = true;
                }
                text.push_str(&original[i..i + ascii]);
                prev = Some(bytes[i + ascii - 1] as char);
                i += ascii;
                continue;
            }
            let Some(c) = original[i..].chars().next() else {
                break;
            };
            let len = c.len_utf8();
            let next = original[i + len..].chars().next();
            if let Some(class) = hidden_at(prev, c, next) {
                counts.add(class);
                in_run = false;
                i += len;
                continue;
            }
            if is_nfkc_stable(c) {
                if !in_run {
                    segs.push(Seg {
                        shadow: text.len(),
                        original: i,
                        linear: true,
                    });
                    in_run = true;
                }
                text.push(c);
            } else {
                let at = text.len();
                push_nfkc(c, &mut text);
                if text.len() > at {
                    segs.push(Seg {
                        shadow: at,
                        original: i,
                        linear: false,
                    });
                }
                in_run = false;
            }
            prev = Some(c);
            i += len;
        }
        if text == original {
            return Shadow::identity(original, counts);
        }
        Shadow {
            original,
            text: Cow::Owned(text),
            segs,
            counts,
        }
    }

    fn identity(original: &'a str, counts: HiddenCounts) -> Shadow<'a> {
        Shadow {
            original,
            text: Cow::Borrowed(original),
            segs: Vec::new(),
            counts,
        }
    }

    /// Original byte offset of the character that produced shadow byte `i` (`i < len`).
    fn map(&self, i: usize) -> usize {
        // Byte 0 of a non-empty shadow always starts a segment.
        let k = self.segs.partition_point(|s| s.shadow <= i);
        let seg = self.segs[k.saturating_sub(1)];
        let mut o = if seg.linear {
            seg.original + (i - seg.shadow)
        } else {
            seg.original
        };
        while !self.original.is_char_boundary(o) {
            o -= 1;
        }
        o
    }

    /// Maps a byte range of the shadow text to the original text. The result covers every
    /// original character that produced a byte in the range, including removed characters
    /// between them (a zero-width space inside a token).
    pub fn to_original(&self, r: Range<usize>) -> Range<usize> {
        if self.segs.is_empty() {
            return r;
        }
        let len = self.text.len();
        let s = r.start.min(len);
        let e = r.end.min(len).max(s);
        if s == len {
            return self.original.len()..self.original.len();
        }
        let start = self.map(s);
        if e == s {
            return start..start;
        }
        let last = self.map(e - 1);
        let last_len = self.original[last..]
            .chars()
            .next()
            .map_or(0, char::len_utf8);
        start..last + last_len
    }
}

/// True when NFKC leaves `c` as it is (the common case for letters of most scripts).
fn is_nfkc_stable(c: char) -> bool {
    use unicode_normalization::{is_nfkc_quick, IsNormalized};
    matches!(is_nfkc_quick(std::iter::once(c)), IsNormalized::Yes)
}

/// The class of a character that is hidden, regardless of its neighbours.
pub(crate) fn hidden_class(c: char) -> Option<Hidden> {
    match c {
        '\u{200B}'..='\u{200F}'
        | '\u{2060}'..='\u{2064}'
        | '\u{FEFF}'
        | '\u{00AD}'
        | '\u{034F}'
        // Arabic letter mark (a direction mark like U+200E/U+200F).
        | '\u{061C}'
        // Mongolian free variation selectors and vowel separator.
        | '\u{180B}'..='\u{180F}'
        // Variation selectors (U+FE0F, emoji presentation, stays) and their supplement.
        | '\u{FE00}'..='\u{FE0E}'
        | '\u{E0100}'..='\u{E01EF}'
        // Hangul fillers: they render as nothing.
        | '\u{115F}'
        | '\u{1160}'
        | '\u{3164}'
        | '\u{FFA0}' => Some(Hidden::Invisible),
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' => Some(Hidden::Bidi),
        '\u{E0000}'..='\u{E007F}' => Some(Hidden::Tag),
        _ => None,
    }
}

/// Like [`hidden_class`], but keeps a zero-width joiner between two emoji (family, profession
/// and similar sequences) and the text-presentation selector U+FE0E right after an emoji.
pub(crate) fn hidden_at(prev: Option<char>, c: char, next: Option<char>) -> Option<Hidden> {
    let class = hidden_class(c)?;
    if c == '\u{200D}' && prev.is_some_and(is_emoji_part) && next.is_some_and(is_emoji_part) {
        return None;
    }
    if c == '\u{FE0E}' && prev.is_some_and(is_emoji_part) {
        return None;
    }
    Some(class)
}

/// Rough "part of an emoji sequence" test: pictographs, dingbats, symbols, skin tones,
/// variation selector 16 and the keycap mark.
fn is_emoji_part(c: char) -> bool {
    matches!(c,
        '\u{1F000}'..='\u{1FAFF}'
        | '\u{2600}'..='\u{27BF}'
        | '\u{2300}'..='\u{23FF}'
        | '\u{2B00}'..='\u{2BFF}'
        | '\u{2190}'..='\u{21FF}'
        | '\u{FE0F}'
        | '\u{20E3}'
        | '\u{00A9}'
        | '\u{00AE}'
        | '\u{203C}'
        | '\u{2049}'
        | '\u{2122}'
        | '\u{2139}'
        | '\u{3030}'
        | '\u{303D}'
        | '\u{3297}'
        | '\u{3299}')
}

fn push_nfkc(c: char, out: &mut String) {
    use unicode_normalization::UnicodeNormalization;
    out.extend(std::iter::once(c).nfkc());
}

/// The normalized form of a value (hidden characters removed, NFKC per character), used to
/// key values so a zero-width-split copy and a clean copy of a secret get the same number.
pub(crate) fn normalize_value(value: &str) -> Cow<'_, str> {
    if value.is_ascii() {
        return Cow::Borrowed(value);
    }
    let mut out = String::with_capacity(value.len());
    let mut prev = None;
    let mut iter = value.chars().peekable();
    while let Some(c) = iter.next() {
        if hidden_at(prev, c, iter.peek().copied()).is_some() {
            continue;
        }
        if c.is_ascii() {
            out.push(c);
        } else {
            push_nfkc(c, &mut out);
        }
        prev = Some(c);
    }
    Cow::Owned(out)
}

/// Strips hidden characters from `original[range]` into `out`, keeping a ZWJ between emoji.
/// Neighbours outside the range are looked up in `original`.
#[cfg(test)]
pub(crate) fn strip_into(original: &str, range: Range<usize>, out: &mut String) {
    kept_runs(original, range, |r| out.push_str(&original[r]));
}

/// Calls `f` with each run of `original[range]` that survives stripping hidden characters
/// (a ZWJ between emoji survives), in order. Neighbours outside the range are looked up in
/// `original`.
pub(crate) fn kept_runs(original: &str, range: Range<usize>, mut f: impl FnMut(Range<usize>)) {
    let bytes = original.as_bytes();
    let mut prev = original[..range.start].chars().next_back();
    let mut run = range.start;
    let mut i = range.start;
    while i < range.end {
        let ascii = bytes[i..range.end]
            .iter()
            .position(|&b| b >= 0x80)
            .unwrap_or(range.end - i);
        if ascii > 0 {
            i += ascii;
            prev = Some(bytes[i - 1] as char);
            continue;
        }
        let Some(c) = original[i..].chars().next() else {
            break;
        };
        let len = c.len_utf8();
        if hidden_at(prev, c, original[i + len..].chars().next()).is_some() {
            if i > run {
                f(run..i);
            }
            run = i + len;
        } else {
            prev = Some(c);
        }
        i += len;
    }
    if range.end > run {
        f(run..range.end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_borrowed_identity() {
        let s = Shadow::new("token=abc");
        assert!(matches!(s.text, Cow::Borrowed(_)));
        assert_eq!(s.to_original(6..9), 6..9);
    }

    #[test]
    fn zero_width_split_token_maps_back() {
        let original = "x=ab\u{200B}cd y";
        let s = Shadow::new(original);
        assert_eq!(s.text, "x=abcd y");
        let start = s.text.find("abcd").unwrap();
        let r = s.to_original(start..start + 4);
        assert_eq!(&original[r], "ab\u{200B}cd");
        assert_eq!(s.counts.invisible, 1);
    }

    #[test]
    fn fullwidth_folds() {
        let original = "ｐａｓｓｗｏｒｄ=１２３";
        let s = Shadow::new(original);
        assert_eq!(s.text, "password=123");
        assert_eq!(s.to_original(0..8), 0..original.find('=').unwrap());
        assert_eq!(
            s.to_original(9..12),
            original.find('１').unwrap()..original.len()
        );
    }

    #[test]
    fn bidi_and_tags_counted_and_removed() {
        let s = Shadow::new("a\u{202E}b\u{2066}c\u{E0041}d");
        assert_eq!(s.text, "abcd");
        assert_eq!(
            s.counts,
            HiddenCounts {
                invisible: 0,
                bidi: 2,
                tag: 1
            }
        );
        assert_eq!(s.to_original(0..4), 0..s.original.len());
    }

    #[test]
    fn expansion_maps_to_whole_char() {
        // U+FB01 (ﬁ ligature) folds to "fi": either byte maps back to the whole ligature.
        let original = "a\u{FB01}b";
        let s = Shadow::new(original);
        assert_eq!(s.text, "afib");
        assert_eq!(s.to_original(1..2), 1..4);
        assert_eq!(s.to_original(2..3), 1..4);
        assert_eq!(s.to_original(4..4), 5..5);
    }

    #[test]
    fn strip_keeps_emoji_zwj() {
        let original = "👨\u{200D}👩\u{200D}👧 hi\u{E0041}\u{200B}!";
        let mut out = String::new();
        strip_into(original, 0..original.len(), &mut out);
        assert_eq!(out, "👨\u{200D}👩\u{200D}👧 hi!");
        let s = Shadow::new(original);
        assert_eq!(
            s.counts,
            HiddenCounts {
                invisible: 1,
                bidi: 0,
                tag: 1
            }
        );
    }

    #[test]
    fn selectors_and_fillers_are_invisible() {
        for ch in [
            '\u{061C}',
            '\u{180B}',
            '\u{180F}',
            '\u{FE00}',
            '\u{FE0E}',
            '\u{E0100}',
            '\u{E01EF}',
            '\u{115F}',
            '\u{1160}',
            '\u{3164}',
            '\u{FFA0}',
        ] {
            let original = format!("x=ab{ch}cd y");
            let s = Shadow::new(&original);
            assert_eq!(s.text, "x=abcd y", "U+{:04X}", ch as u32);
            assert_eq!(s.counts.invisible, 1);
            let start = s.text.find("abcd").unwrap();
            let r = s.to_original(start..start + 4);
            assert_eq!(original[r].chars().count(), 5);
            assert_eq!(normalize_value(&format!("ab{ch}cd")), "abcd");
        }
        // Text presentation after an emoji and emoji presentation stay.
        assert_eq!(
            Shadow::new("\u{2764}\u{FE0E} ok").text,
            "\u{2764}\u{FE0E} ok"
        );
        assert_eq!(Shadow::new("\u{2764}\u{FE0F} ok").counts.invisible, 0);
    }

    #[test]
    fn normalized_value_matches_shadow() {
        assert_eq!(normalize_value("ab\u{200B}cd"), "abcd");
        assert_eq!(normalize_value("ｐｗ"), "pw");
        assert_eq!(normalize_value("plain"), "plain");
    }
}
