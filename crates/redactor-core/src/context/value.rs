//! How secret a value looks on its own.
//!
//! [`value_score`] is the single entry point: it delegates to the trained classifier, so every
//! caller goes through it.

/// A score in `[0, 1]`: how much `v` looks like a random secret rather than a word, an
/// identifier, a date or a version. Values with
/// whitespace or shorter than 4 characters are never random-looking.
pub(crate) fn value_score(v: &str) -> f32 {
    if v.chars().nth(3).is_none() || v.chars().any(char::is_whitespace) {
        return 0.05;
    }
    crate::classifier::secret_probability(v)
}

/// The prose test: a digit next to letters, a symbol, a case change inside the
/// word, or a high score at length 12+.
pub(crate) fn looks_like_password(v: &str) -> bool {
    let letters = v.chars().any(|c| c.is_alphabetic());
    let digits = v.chars().any(|c| c.is_ascii_digit());
    let symbol = v
        .chars()
        .any(|c| !c.is_alphanumeric() && !matches!(c, '-' | '_' | '\'' | '.' | '’'));
    let mut case_change = false;
    let mut prev: Option<char> = None;
    for c in v.chars() {
        if let Some(p) = prev {
            if p.is_lowercase() && c.is_uppercase() {
                case_change = true;
            }
        }
        prev = Some(c);
    }
    (letters && (digits || symbol || case_change))
        || (v.chars().count() >= 12 && value_score(v) >= 0.6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_tokens_score_high() {
        for v in [
            "Zq81mXv0pLr3TfK9wQe2",
            "SHVudGVyMi1GYWtlLTl4",
            "9f8e7d6c5b4a39281706f5e4d3c2b1a0",
            "Kx9!mPq2vL",
            "aB3dE5fG7hI9jK1lM3nO",
        ] {
            assert!(value_score(v) >= 0.5, "{v}: {}", value_score(v));
        }
    }

    #[test]
    fn words_and_identifiers_score_low() {
        for v in [
            "getUserAccountSettings",
            "node_modules",
            "password_input",
            "2024-11-30",
            "v1.2.3",
            "hunter",
            "correct horse battery staple",
            "PasswordInput",
            "required",
        ] {
            assert!(value_score(v) < 0.5, "{v}: {}", value_score(v));
        }
    }

    #[test]
    fn password_test() {
        for v in ["Tr0ub4dor&3", "Kx9!mPq2vL", "hunter2", "myPassPhrase"] {
            assert!(looks_like_password(v), "{v}");
        }
        for v in [
            "required",
            "falsch",
            "expired",
            "well-known",
            "1234",
            "link",
        ] {
            assert!(!looks_like_password(v), "{v}");
        }
    }
}
