//! Key names: split into words, then classified against the lexicon.
//!
//! Matching is on whole words, never substrings: `dbPassword` is [db, password] and names a
//! password; `bypass`, `passport` and `keyboard` are one word each and name nothing.

use super::lexicon::{entries_starting, is_negative, Label, Strength, SUFFIX_WORDS, WEAK_VOID};

/// Splits a key name into lowercase words at non-alphanumeric characters (snake, kebab, dots,
/// spaces), at camelCase humps (`apiKey`, `APIKey`) and after a run of digits ends a word
/// (`db2Password` is [db, 2, password]; `2fa` stays one word).
pub fn split_key(key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = key.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            flush(&mut cur, &mut out);
            continue;
        }
        if let Some(&p) = i.checked_sub(1).and_then(|j| chars.get(j)) {
            let next = chars.get(i + 1).copied();
            let boundary = (p.is_lowercase() && c.is_uppercase())
                || (p.is_ascii_digit() && c.is_uppercase())
                || (p.is_alphabetic() && c.is_ascii_digit())
                || (p.is_uppercase() && c.is_uppercase() && next.is_some_and(|n| n.is_lowercase()));
            if boundary && p.is_alphanumeric() {
                flush(&mut cur, &mut out);
            }
        }
        cur.extend(c.to_lowercase());
    }
    flush(&mut cur, &mut out);
    out
}

fn flush(cur: &mut String, out: &mut Vec<String>) {
    if !cur.is_empty() {
        out.push(std::mem::take(cur));
    }
}

/// What the lexicon says about a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeyClass {
    pub label: Label,
    pub strength: Strength,
    /// Every word is part of a lexicon match (`api key`, `password`; not `db password`).
    pub all_consumed: bool,
    /// The last word is part of a lexicon match (`db_password`; not `password_prod`).
    pub last_consumed: bool,
    /// Index of the winning match's last word.
    pub word: usize,
}

/// The label and strength a key names, or `None` (no lexicon word, or a negative modifier
/// such as `min`, `url`, `type` outside a lexicon phrase).
#[cfg_attr(not(test), allow(dead_code))] // Public interface of the context layer; the engine uses the detector.
pub fn classify_key(words: &[String]) -> Option<(Label, Strength)> {
    classify(words).map(|k| (k.label, k.strength))
}

/// Negative words about an attempt or a change: they void a key only after its head
/// (`password_retry_count`); before it they say which secret it is (`retry_password`).
const AFTER_HEAD_ONLY: &[&str] = &[
    "retry", "retries", "attempts", "changed", "updated", "created", "rotation", "store", "stores",
    "helper", "helpers",
];

/// Words right before `token` that make it a pagination cursor (`next_page_token`).
const CURSOR_WORDS: &[&str] = &[
    "page",
    "next",
    "prev",
    "previous",
    "continuation",
    "pagination",
    "sync",
    "cursor",
    "resume",
];

pub(crate) fn classify(words: &[String]) -> Option<KeyClass> {
    if words.is_empty() || words.len() > 12 {
        return None;
    }
    let mut consumed = vec![false; words.len()];
    // (label, strength, index of the last word)
    let mut matches: Vec<(Label, Strength, usize)> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let mut hit = None;
        for e in entries_starting(&words[i]) {
            let n = e.words.len();
            if i + n <= words.len() && e.words.iter().zip(&words[i..i + n]).all(|(a, b)| *a == b) {
                hit = Some((e.label, e.strength, n));
                break;
            }
        }
        if hit.is_none() {
            let wd = words[i].as_str();
            if let Some((_, label)) = SUFFIX_WORDS
                .iter()
                .find(|(sw, _)| wd.len() > sw.len() && wd.ends_with(sw))
            {
                hit = Some((*label, Strength::Strong, 1));
            }
        }
        match hit {
            Some((label, strength, n)) => {
                for c in &mut consumed[i..i + n] {
                    *c = true;
                }
                matches.push((label, strength, i + n - 1));
                i += n;
            }
            None => i += 1,
        }
    }
    if matches.is_empty() {
        return None;
    }
    let aws = words.iter().any(|w| w == "aws");
    let head = matches.iter().map(|m| m.2).max().unwrap_or(0);
    for (i, (wd, c)) in words.iter().zip(&consumed).enumerate() {
        if !c
            && is_negative(wd)
            && !(aws && wd == "id")
            && !(i < head && AFTER_HEAD_ONLY.contains(&wd.as_str()))
        {
            return None;
        }
    }
    // `page_token`, `next_token`, `continuation_token`: an opaque pagination cursor.
    let cursor = matches.last().and_then(|m| {
        let w = words.get(m.2)?;
        let prev = m.2.checked_sub(1).and_then(|i| words.get(i))?;
        Some(
            matches!(w.as_str(), "token" | "tokens")
                && !consumed[m.2 - 1]
                && CURSOR_WORDS.contains(&prev.as_str()),
        )
    });
    if cursor == Some(true) {
        return None;
    }
    // The head of an English compound is its last word: `private_key_password` is a password.
    let best = matches
        .iter()
        .rev()
        .find(|m| m.1 == Strength::Strong)
        .or_else(|| matches.last())
        .copied()?;
    if best.1 == Strength::Weak
        && words
            .iter()
            .zip(&consumed)
            .any(|(wd, c)| !c && WEAK_VOID.contains(&wd.as_str()))
    {
        return None;
    }
    Some(KeyClass {
        label: best.0,
        strength: best.1,
        all_consumed: consumed.iter().all(|c| *c),
        last_consumed: *consumed.last().unwrap_or(&false),
        word: best.2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        split_key(s)
    }

    #[test]
    fn splits_every_style() {
        assert_eq!(words("dbPassword"), ["db", "password"]);
        assert_eq!(words("X-Api-Key"), ["x", "api", "key"]);
        assert_eq!(words("APIKey"), ["api", "key"]);
        assert_eq!(words("DB_PASSWORD"), ["db", "password"]);
        assert_eq!(
            words("spring.datasource.password"),
            ["spring", "datasource", "password"]
        );
        assert_eq!(words("db2Password"), ["db", "2", "password"]);
        assert_eq!(words("2faSecret"), ["2fa", "secret"]);
        assert_eq!(words("mot de passe"), ["mot", "de", "passe"]);
        assert_eq!(words("Passwort"), ["passwort"]);
        assert_eq!(words("ПАРОЛЬ"), ["пароль"]);
    }

    #[test]
    fn classifies_whole_words_only() {
        let c = |s: &str| classify_key(&words(s)).map(|x| x.0);
        assert_eq!(c("dbPassword"), Some(Label::Password));
        assert_eq!(c("X-Api-Key"), Some(Label::ApiKey));
        assert_eq!(c("PGPASSWORD"), Some(Label::Password));
        assert_eq!(c("client_secret"), Some(Label::Secret));
        assert_eq!(c("aws_secret_access_key"), Some(Label::ApiKey));
        assert_eq!(c("private_key_password"), Some(Label::Password));
        assert_eq!(c("id_token"), Some(Label::Token));
        assert_eq!(c("DATABASE_URL"), Some(Label::ConnectionString));
        for k in [
            "bypass",
            "passport",
            "compass",
            "keyboard",
            "monkey",
            "turkey",
            "tokenizer",
            "spinner",
            "pinned",
            "ping",
        ] {
            assert_eq!(c(k), None, "{k}");
        }
    }

    #[test]
    fn negative_modifiers_void() {
        for k in [
            "max_tokens",
            "token_count",
            "token_type",
            "password_min_length",
            "PASSWORD_RESET_URL",
            "secret_name",
            "public_key",
            "key_size",
            "client_id",
            "passwordHint",
            "next_page_token",
            "continuationToken",
        ] {
            assert_eq!(classify_key(&words(k)), None, "{k}");
        }
    }

    /// Words about the attempt or the change of a secret void it only after the head
    /// (`password_retry_count`); before it they name which secret (`retry_password`).
    #[test]
    fn modifier_before_head_keeps_key() {
        for k in ["retry_password", "rotated_api_key", "updated_secret"] {
            assert!(classify_key(&words(k)).is_some(), "{k}");
        }
        for k in [
            "password_retry_count",
            "password_retries",
            "token_updated",
            "secret_rotation",
        ] {
            assert_eq!(classify_key(&words(k)), None, "{k}");
        }
    }

    #[test]
    fn weak_keys_are_weak() {
        assert_eq!(
            classify_key(&words("signature")),
            Some((Label::Secret, Strength::Weak))
        );
        assert_eq!(
            classify_key(&words("secret_key")),
            Some((Label::Secret, Strength::Strong))
        );
    }
}
