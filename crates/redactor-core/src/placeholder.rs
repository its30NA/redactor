//! Placeholder wording and stable numbering.

use std::collections::HashMap;

use crate::config::PlaceholderStyle;
use crate::finding::ValueKey;

/// The label every placeholder uses in [`PlaceholderStyle::Generic`].
pub const GENERIC_LABEL: &str = "Secret";

/// How placeholders are built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceholderPolicy {
    pub style: PlaceholderStyle,
    /// Template with `{label}` and `{suffix}`.
    pub template: String,
}

impl Default for PlaceholderPolicy {
    fn default() -> Self {
        PlaceholderPolicy {
            style: PlaceholderStyle::Labeled,
            template: crate::config::DEFAULT_TEMPLATE.to_string(),
        }
    }
}

impl PlaceholderPolicy {
    /// The label as it appears in the placeholder.
    pub fn display_label<'a>(&self, label: &'a str) -> &'a str {
        match self.style {
            PlaceholderStyle::Labeled => label,
            PlaceholderStyle::Generic => GENERIC_LABEL,
        }
    }

    /// Fills the template.
    pub fn render(&self, label: &str, suffix: &str) -> String {
        self.template
            .replace("{label}", label)
            .replace("{suffix}", suffix)
    }

    /// Placeholders for spans given in output order as `(label, value key)`: same value, same
    /// number; numbering per label in order of first appearance; ` #N` only when a label has
    /// more than one distinct value.
    pub fn number(&self, items: &[(&str, ValueKey)]) -> Vec<String> {
        // label -> number of distinct values so far; (label, value) -> its number
        let mut distinct: HashMap<&str, usize> = HashMap::new();
        let mut index: HashMap<(&str, ValueKey), usize> = HashMap::new();
        let mut numbers = Vec::with_capacity(items.len());
        for &(label, key) in items {
            let label = self.display_label(label);
            let n = *index.entry((label, key)).or_insert_with(|| {
                let d = distinct.entry(label).or_insert(0);
                *d += 1;
                *d
            });
            numbers.push((label, n));
        }
        // Each distinct placeholder is rendered once; repeats are copies.
        let mut rendered: HashMap<(&str, usize), String> = HashMap::new();
        numbers
            .into_iter()
            .map(|(label, n)| {
                rendered
                    .entry((label, n))
                    .or_insert_with(|| {
                        let suffix = if distinct[label] > 1 {
                            format!(" #{n}")
                        } else {
                            String::new()
                        };
                        self.render(label, &suffix)
                    })
                    .clone()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(b: u8) -> ValueKey {
        ValueKey([b; 16])
    }

    #[test]
    fn numbering_same_value_same_number() {
        let p = PlaceholderPolicy::default();
        let out = p.number(&[
            ("Password", k(1)),
            ("Password", k(2)),
            ("Password", k(1)),
            ("GitHub Token", k(9)),
        ]);
        assert_eq!(
            out,
            [
                "[REDACTED: Password #1]",
                "[REDACTED: Password #2]",
                "[REDACTED: Password #1]",
                "[REDACTED: GitHub Token]",
            ]
        );
    }

    #[test]
    fn no_suffix_when_single() {
        let p = PlaceholderPolicy::default();
        let out = p.number(&[("API Key", k(3)), ("API Key", k(3))]);
        assert_eq!(out, ["[REDACTED: API Key]", "[REDACTED: API Key]"]);
    }

    #[test]
    fn generic_style_hides_labels() {
        let p = PlaceholderPolicy {
            style: PlaceholderStyle::Generic,
            ..PlaceholderPolicy::default()
        };
        let out = p.number(&[("Password", k(1)), ("GitHub Token", k(2))]);
        assert_eq!(out, ["[REDACTED: Secret #1]", "[REDACTED: Secret #2]"]);
    }

    #[test]
    fn custom_template() {
        let p = PlaceholderPolicy {
            template: "<{label}{suffix}>".into(),
            ..PlaceholderPolicy::default()
        };
        assert_eq!(p.number(&[("Token", k(1))]), ["<Token>"]);
    }
}
