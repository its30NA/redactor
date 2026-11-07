//! Always-redact words: `EngineConfig::terms`.
//!
//! Plain words the user never wants to leave the machine (a name, a company, an internal host
//! name). Matched case-insensitively and as whole words: `acme` hits `ACME` and `Acme,` but
//! not `acmeville`.

use std::borrow::Cow;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};

use crate::detector::Detector;
use crate::finding::{Candidate, Source};
use crate::normalize::normalize_value;

/// Detector id and finding kind.
pub(crate) const ID: &str = "custom_term";
/// Placeholder label of every term.
pub(crate) const LABEL: &str = "Custom Term";
const CONFIDENCE: f32 = 0.95;
/// Shorter entries are ignored.
const MIN_CHARS: usize = 3;

pub(crate) struct TermsDetector {
    ac: AhoCorasick,
}

impl TermsDetector {
    /// `None` when no usable term is left.
    pub fn new(terms: &[String]) -> Option<TermsDetector> {
        let mut patterns: Vec<String> = Vec::new();
        for t in terms {
            // The detectors see the normalized shadow text, so the terms are normalized too.
            let t = normalize_value(t.trim());
            if t.chars().count() < MIN_CHARS {
                continue;
            }
            // ASCII letters are folded by the automaton; for other letters the lower, upper
            // and title-case spellings are added (`müller`, `MÜLLER`, `Müller`).
            for v in [
                t.to_string(),
                t.to_lowercase(),
                t.to_uppercase(),
                title_case(&t),
            ] {
                if !patterns.contains(&v) {
                    patterns.push(v);
                }
            }
        }
        if patterns.is_empty() {
            return None;
        }
        let ac = AhoCorasickBuilder::new()
            .ascii_case_insensitive(true)
            .match_kind(MatchKind::Standard)
            .build(&patterns)
            .ok()?;
        Some(TermsDetector { ac })
    }
}

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if start {
            out.extend(c.to_uppercase());
        } else {
            out.extend(c.to_lowercase());
        }
        start = !c.is_alphanumeric();
    }
    out
}

/// A letter, digit or `_` glued to the match makes it part of a longer word.
fn is_word_char(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

impl Detector for TermsDetector {
    fn id(&self) -> &'static str {
        ID
    }

    fn detect(&self, text: &str, out: &mut Vec<Candidate>) {
        // Overlapping search: a match rejected by the word check never hides another one.
        for m in self.ac.find_overlapping_iter(text) {
            let (s, e) = (m.start(), m.end());
            if !text.is_char_boundary(s) || !text.is_char_boundary(e) {
                continue;
            }
            if is_word_char(text[..s].chars().next_back()) || is_word_char(text[e..].chars().next())
            {
                continue;
            }
            out.push(Candidate {
                start: s,
                end: e,
                kind: Cow::Borrowed(ID),
                label: Cow::Borrowed(LABEL),
                confidence: CONFIDENCE,
                detector: Cow::Borrowed(ID),
                reason: Cow::Borrowed("On your always-redact list"),
                source: Source::Custom,
            });
        }
    }
}
