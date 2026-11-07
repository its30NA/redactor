//! Splicing placeholders into the text.

use crate::finding::{OutputSpan, ValueKey};
use crate::normalize::kept_runs;
use crate::placeholder::PlaceholderPolicy;

/// A non-overlapping span of the ORIGINAL text that gets replaced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved<'a> {
    pub start: usize,
    pub end: usize,
    pub label: &'a str,
    pub value_key: ValueKey,
    /// Index of the finding this span belongs to.
    pub finding: usize,
}

/// The redacted text, where each placeholder landed, and each span's placeholder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Redaction {
    pub output: String,
    pub spans: Vec<OutputSpan>,
    /// Placeholder per input span, same order as the `spans` argument of [`apply`].
    pub placeholders: Vec<String>,
    /// How the output was assembled, in output order: maps output offsets back to the input.
    pub pieces: Vec<Piece>,
}

/// A piece of the output: either input bytes copied verbatim, or a placeholder that replaced
/// `input`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub output: usize,
    pub input: std::ops::Range<usize>,
    pub placeholder: bool,
}

impl Redaction {
    /// Maps a range of the output back to the input. A bound inside a placeholder widens to
    /// the whole span it replaced.
    pub fn to_input(&self, s: usize, e: usize) -> (usize, usize) {
        let locate = |pos: usize, is_end: bool| -> usize {
            // The piece holding output byte `pos` (for an end: byte `pos - 1`).
            let key = if is_end { pos.saturating_sub(1) } else { pos };
            let k = self.pieces.partition_point(|p| p.output <= key);
            let Some(p) = self.pieces.get(k.wrapping_sub(1)) else {
                return 0;
            };
            if p.placeholder {
                if is_end {
                    p.input.end
                } else {
                    p.input.start
                }
            } else {
                (p.input.start + (pos - p.output)).min(p.input.end)
            }
        };
        if self.pieces.is_empty() {
            return (s, e);
        }
        let start = locate(s, false);
        let end = if e > s { locate(e, true) } else { start };
        (start, end.max(start))
    }
}

/// Replaces every span (sorted by start, non-overlapping) with its placeholder. Hidden
/// characters outside the spans are stripped when `strip_invisible` is set.
pub fn apply(
    text: &str,
    spans: &[Resolved<'_>],
    policy: &PlaceholderPolicy,
    strip_invisible: bool,
) -> Redaction {
    let items: Vec<(&str, ValueKey)> = spans.iter().map(|s| (s.label, s.value_key)).collect();
    let placeholders = policy.number(&items);
    let mut output = String::with_capacity(text.len());
    let mut out_spans = Vec::with_capacity(spans.len());
    let mut pieces: Vec<Piece> = Vec::with_capacity(2 * spans.len() + 1);
    let mut pos = 0usize;
    let copy = |out: &mut String, pieces: &mut Vec<Piece>, range: std::ops::Range<usize>| {
        let mut push = |r: std::ops::Range<usize>| {
            pieces.push(Piece {
                output: out.len(),
                input: r.clone(),
                placeholder: false,
            });
            out.push_str(&text[r]);
        };
        if strip_invisible {
            kept_runs(text, range, push);
        } else if !range.is_empty() {
            push(range);
        }
    };
    for (span, placeholder) in spans.iter().zip(&placeholders) {
        debug_assert!(
            span.start >= pos && span.end >= span.start,
            "spans must be sorted"
        );
        let start = span.start.max(pos);
        copy(&mut output, &mut pieces, pos..start);
        let out_start = output.len();
        output.push_str(placeholder);
        pos = span.end.max(start);
        pieces.push(Piece {
            output: out_start,
            input: start..pos,
            placeholder: true,
        });
        out_spans.push(OutputSpan {
            start: out_start,
            end: output.len(),
            finding: span.finding,
        });
    }
    copy(&mut output, &mut pieces, pos..text.len());
    Redaction {
        output,
        spans: out_spans,
        placeholders,
        pieces,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: usize, end: usize, label: &str, key: u8, finding: usize) -> Resolved<'_> {
        Resolved {
            start,
            end,
            label,
            value_key: ValueKey([key; 16]),
            finding,
        }
    }

    #[test]
    fn splices_and_reports_output_spans() {
        let text = "a=SECRET1 b=SECRET2 c=SECRET1";
        let spans = [
            span(2, 9, "Password", 1, 0),
            span(12, 19, "Password", 2, 1),
            span(22, 29, "Password", 1, 2),
        ];
        let r = apply(text, &spans, &PlaceholderPolicy::default(), true);
        assert_eq!(
            r.output,
            "a=[REDACTED: Password #1] b=[REDACTED: Password #2] c=[REDACTED: Password #1]"
        );
        assert_eq!(r.spans.len(), 3);
        for (s, p) in r.spans.iter().zip(&r.placeholders) {
            assert_eq!(&r.output[s.start..s.end], p);
        }
        assert_eq!(r.spans[1].finding, 1);
    }

    #[test]
    fn strip_keeps_emoji_zwj() {
        let text = "👨\u{200D}👩\u{200D}👧 key=TOPSECRET\u{E0041}\u{200B}!";
        let start = text.find("TOPSECRET").unwrap();
        let spans = [span(start, start + 9, "Password", 1, 0)];
        let r = apply(text, &spans, &PlaceholderPolicy::default(), true);
        assert_eq!(r.output, "👨\u{200D}👩\u{200D}👧 key=[REDACTED: Password]!");
        let kept = apply(text, &spans, &PlaceholderPolicy::default(), false);
        assert!(kept.output.contains('\u{E0041}'));
    }
}
