//! The context engine: secrets that have no recognisable shape of their own and
//! are only known by the name next to them (`DB_PASSWORD=…`, `--token …`, "the wifi pw is …").
//!
//! 1. [`syntax`] finds lexicon keys (whole words of the key name, never substrings) and
//!    command names, and parses the value after each.
//! 2. This module judges the value: literals count, code does not. `DB_PASSWORD=hunter2prod`
//!    in a `.env` file is a secret; `password = password_input` in Python is not. The call is
//!    made from the syntax: quoting, config-style line-start assignment versus code statement,
//!    key casing, and whether the value is used as an identifier elsewhere in the text.
//! 3. [`prose`] handles keys named in sentences.

mod keyname;
mod lexicon;
mod prose;
mod syntax;
mod value;

use std::borrow::Cow;
use std::collections::HashMap;

use aho_corasick::{AhoCorasick, MatchKind};

use crate::detector::Detector;
use crate::engine::is_builtin_placeholder;
use crate::finding::{Candidate, Source};
use crate::guard::{is_placeholder, mixed_in_digits};
use crate::rules::is_filler;

#[allow(unused_imports)]
pub use keyname::{classify_key, split_key};
#[allow(unused_imports)]
pub use syntax::{extract_pairs, Pair, Syntax};

use lexicon::{
    is_lexicon_word, is_negative, is_stopword, Label, Strength, CODE_WORDS, KEYWORDS, OBJECT_ROOTS,
    TYPE_NAMES,
};
use prose::{prose_value, ProseHit};
use syntax::{ceil, command, floor, for_each_event, line_end, pair, Event, Key, RawPair, Sep};
pub(crate) use value::{looks_like_password, value_score};

/// Detector id (`EngineConfig::disabled` takes it).
pub(crate) const ID: &str = "context";

/// The context engine as a [`Detector`].
pub(crate) struct ContextDetector;

impl Detector for ContextDetector {
    fn id(&self) -> &'static str {
        ID
    }

    fn detect(&self, text: &str, out: &mut Vec<Candidate>) {
        let mut found: Vec<Found> = Vec::new();
        for_each_event(text, |ev| match ev {
            Event::Key(key) => {
                if let Some(p) = pair(text, &key) {
                    if let Some(f) = judge_pair(text, &key, &p) {
                        found.push(f);
                    }
                    return Some(p.scanned);
                }
                let hit = prose_value(text, &key)?;
                let f = judge_prose(text, &key, &hit)?;
                let end = f.end;
                found.push(f);
                Some(end)
            }
            Event::Cmd { start, end, cmd } => {
                let v = command(text, start, end, cmd)?;
                let f = judge_command(text, v.start, v.end)?;
                let e = f.end;
                found.push(f);
                Some(e)
            }
        });
        terraform_defaults(text, &mut found);
        table_columns(text, &mut found);
        plan_changes(text, &mut found);
        sql_inserts(text, &mut found);
        drop_identifiers(text, &mut found);
        out.extend(found.into_iter().map(|f| Candidate {
            start: f.start,
            end: f.end,
            kind: Cow::Borrowed(f.label.kind()),
            label: Cow::Borrowed(f.label.as_str()),
            confidence: f.confidence,
            detector: Cow::Borrowed(ID),
            reason: Cow::Borrowed(f.reason),
            source: Source::Context,
        }));
    }
}

/// A judged value.
#[derive(Clone, Debug)]
struct Found {
    start: usize,
    end: usize,
    label: Label,
    confidence: f32,
    reason: &'static str,
    /// An identifier-shaped value outside `.env` style: dropped if the text uses it as an
    /// identifier elsewhere (`def login(user, x9k2):` … `password = x9k2`).
    ident_check: bool,
}

const R_STRONG_RANDOM: &str = "Random-looking value of a secret-named key";
const R_STRONG_PLAIN: &str = "Value of a secret-named key";
const R_WEAK: &str = "Random-looking value of a key, auth or signature field";
const R_FLAG: &str = "Value of a secret-named command-line option";
const R_COMMAND: &str = "Password argument of a command";
const R_PROSE: &str = "Secret named in a sentence";
const R_PIN: &str = "Digits of a PIN field";
const R_TF_DEFAULT: &str = "Default of a secret-named Terraform variable";
const R_WEBHOOK: &str = "Random path segment of a webhook URL";

/// The first path or query segment of a URL that reads as a generated secret (16+
/// characters, letters and digits, random-looking).
fn webhook_secret(v: &str) -> Option<std::ops::Range<usize>> {
    let scheme = v.find("://")?;
    let host_end = scheme + 3 + v[scheme + 3..].find('/')?;
    let mut i = host_end;
    for seg in v[host_end..].split(['/', '?', '&', '=', '#']) {
        let st = i;
        i += seg.len() + 1;
        let b = seg.as_bytes();
        let generated = b.len() >= 16
            && b.iter()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
            && b.iter().any(u8::is_ascii_digit)
            && b.iter().any(u8::is_ascii_alphabetic)
            && value_score(seg) >= 0.5;
        if generated {
            return Some(st..st + seg.len());
        }
    }
    None
}

// ---------------------------------------------------------------------------------------
// Value judges
// ---------------------------------------------------------------------------------------

fn judge_pair(text: &str, key: &Key, p: &RawPair) -> Option<Found> {
    let r = p.value.clone()?;
    let v = &text[r.clone()];
    let label = key.class.label;
    let strong = key.class.strength == Strength::Strong;
    let flag = p.syntax == Syntax::Flag;
    // `WEBHOOK_URL=https://hooks.example.com/in/<secret>`: the URL's random segment.
    if key.words.len() >= 2
        && key.words.last().is_some_and(|w| w == "url" || w == "uri")
        && key.words.iter().any(|w| w == "webhook" || w == "hook")
    {
        let seg = webhook_secret(v)?;
        return Some(Found {
            start: r.start + seg.start,
            end: r.start + seg.end,
            label,
            confidence: 0.8,
            reason: R_WEBHOOK,
            ident_check: false,
        });
    }
    if never_a_value(v, label) {
        return None;
    }
    let found = |confidence: f32, reason: &'static str, ident_check: bool| Found {
        start: r.start,
        end: r.end,
        label,
        confidence,
        reason,
        ident_check,
    };
    let digits = all_digits(v);
    let n = v.chars().count();
    if label == Label::Pin {
        // A card's CVV has 3 or 4 digits.
        let card = key
            .words
            .iter()
            .any(|w| matches!(w.as_str(), "cvv" | "cvc" | "cvv2" | "cvc2" | "csc"));
        let len = if card { 3..=4 } else { 4..=12 };
        return (digits && len.contains(&n)).then(|| found(0.75, R_PIN, false));
    }
    if digits {
        // Numbers count for PIN keys, for a passcode, and for a password on a config line or
        // in a JSON string (`DB_PASSWORD=12345678`); elsewhere a number is a number.
        let passcode = key
            .words
            .iter()
            .any(|w| matches!(w.as_str(), "passcode" | "zugangscode"));
        let config = !p.code
            && (p.line_start && (p.literal || p.quoted)
                || p.quoted && p.syntax == Syntax::QuotedKey);
        let ok = label == Label::Password
            && strong
            && (passcode && (4..=12).contains(&n) || config && (6..=64).contains(&n));
        return ok.then(|| found(0.75, R_STRONG_PLAIN, false));
    }
    let score = value_score(v);
    let random = score >= 0.5;
    // Vault `secret_id` is a UUID or a random string; AWS `SecretId` names a secret by path
    // or ARN.
    let secret_id = key.words.len() >= 2 && key.words[key.words.len() - 2..] == ["secret", "id"];
    if secret_id && !(uuid_like(v) || random && !v.contains([':', '/'])) {
        return None;
    }
    match label {
        Label::ConnectionString if !(has_credentials(v) || random) => return None,
        Label::RecoveryPhrase if !(random || phrase_like(v) || code_groups(v)) => return None,
        _ => {}
    }
    if p.quoted {
        // `" secret "` with blanks at an end, or `'openaiKey'` (an identifier naming a
        // secret, as in a fixture table) is not a secret.
        // `"Temp1234"`, `"KeyWest2024"`: words with digits are a password even when the
        // words are code words.
        if v.trim() != v || is_identifier(v) && score < 0.5 && code_words(v) && !digits_in_words(v)
        {
            return None;
        }
    } else {
        // In a code statement only a string literal is a value; `vec![…]`, `x.y()`, `[7; 16]`
        // and identifiers are code.
        // `password: x,` / `token: Token![fn],` at a line start: a struct or object literal.
        let literal_field = p.stmt_end && p.syntax == Syntax::LineAssign;
        // A config line, an option or a logfmt field holds a literal: brackets, braces and
        // backslashes there are characters of the password, not a truncated expression.
        // Only a whole value of code shape (`getPassword()`, `ENV['X']`) is code.
        let code_shape = if p.literal {
            expression(v, true, p.sep == Sep::Colon) || is_structure(v)
        } else {
            is_expression(v)
                || unbalanced(v)
                || v.contains('{')
                || v.starts_with('\\')
                || v.contains('\\') && score < 0.7
        };
        // `DB_PASSWORD=1234!5678#`: digits and symbols on a config line are a password.
        let letterless = !v.chars().any(char::is_alphabetic)
            && !(p.line_start && p.literal && label == Label::Password && digit_symbols(v));
        if p.code || literal_field || is_type_name(v) || code_shape || letterless {
            return None;
        }
        // A member path of plain words is code; a segment with digits mixed into its
        // letters (`config.Zq81mXv0pLr3`) is a generated value behind a dotted prefix.
        if let Some(segs) = member_path(v).filter(|s| !s[1..].iter().any(|g| mixed_in_digits(g))) {
            let shares = segs.iter().any(|s| {
                split_key(s)
                    .iter()
                    .any(|w| key.words.contains(w) || CODE_WORDS.contains(&w.as_str()))
            });
            let root = OBJECT_ROOTS.contains(&segs[0].to_ascii_lowercase().as_str());
            if root || shares || p.code || p.args || key.member {
                return None;
            }
        }
    }
    let mut ident_check = false;
    if !p.quoted && is_identifier(v) && score < 0.6 {
        let lower = v.to_lowercase();
        // `password1`, `secret123`: a lexicon word with digits is a classic weak password
        // in a config line, not a variable name.
        let weak_classic = p.line_start && {
            let w = lower.trim_end_matches(|c: char| c.is_ascii_digit());
            w.len() < lower.len() && is_lexicon_word(w)
        };
        let env_style = key.upper && p.sep == Sep::Equals && p.tight && p.line_start;
        // In an argument list (`f(user, password=pw1)`, `{password: secret}`) an identifier
        // is a variable; other shapes (`Tulip-Harbor-91`) are literals of a dump or a list.
        // Outside code, code words (`temp`, `new`, `user`) only make a variable of a value
        // without digits among its words: `password: Temp1234` is a password, and a `.env`
        // line (`SMTP_PASSWORD=new_pass`) holds configuration whatever it looks like.
        // `(user: qa1, pw: Temp1234)`: after `,` or `(` a strong password key with digits
        // mixed into the letters holds a password; `password=pw_value` is a variable.
        let mixed_password = label == Label::Password && strong && mixed_in_digits(v);
        if p.code
            || p.args && !mixed_password
            || p.stmt_end
            || key.member
            || is_stopword(&lower)
            || code_words(v) && !weak_classic && !env_style && !digits_in_words(v)
        {
            return None;
        }
        let ok = if env_style {
            true
        } else if flag || p.syntax == Syntax::Spaced {
            v.len() >= 4
        } else if p.line_start && p.sep == Sep::Colon {
            // YAML: `password: hunter`; `password: abc12` with digits among letters.
            n >= 6 || n >= 4 && letters_and_digits(v)
        } else if p.line_start && p.sep == Sep::Equals && p.tight {
            // properties / INI without spaces: `db.password=hunter`
            v.len() >= 4
        } else if p.line_start && p.sep == Sep::Equals && !v.contains(['_', '.']) {
            // INI: `db_password = dragonfly`, `password = kiwi9`. A variable in code mostly
            // has an `_` or is used as an identifier elsewhere in the text (`ident_check`).
            n >= 6 || n >= 4 && letters_and_digits(v)
        } else if p.syntax == Syntax::Assign && p.literal && p.sep == Sep::Equals && p.tight {
            // A logfmt field (`user=bob password=sunshine attempt=2`) holds a literal; a
            // state word (`password=redacted`, `password=present`) is about it.
            n >= 6 && !LOG_STATES.contains(&lower.as_str())
        } else {
            // `password = hunter2prod`: INI, or Python with a variable; `tokens: GitHub` in
            // prose. Only a value with digits among its letters, and not one the text uses as
            // an identifier.
            v.bytes().any(|b| b.is_ascii_digit()) && v.bytes().any(|b| b.is_ascii_alphabetic())
        };
        if !ok {
            return None;
        }
        ident_check = !env_style;
    }
    // A `DB_PASSWORD=…` line is configuration, not prose: its words are the value (`pass word
    // with spaces`, `'my dog Rex 2019'`), unless they read as a sentence.
    let env_line = key.upper && p.sep == Sep::Equals && p.tight && p.line_start;
    let dotenv = !p.quoted && env_line;
    let prose = if env_line {
        // One word is never a sentence (`'Winter2024!'`).
        let words = v.split_whitespace().count();
        words > 8 || words > 1 && v.trim_end().ends_with(['.', '?', '!', ':'])
    } else {
        prose_like(v)
    };
    if (p.quoted || v.contains(char::is_whitespace)) && prose {
        return None;
    }
    // nsswitch.conf: `passwd:  files systemd` names the passwd database, not a password.
    if !p.quoted
        && v.split_whitespace()
            .next()
            .is_some_and(|w| lexicon::NSS_SOURCES.contains(&w))
    {
        return None;
    }
    if !strong {
        let hash_field = key.words.last().is_some_and(|w| w == "hash")
            && v.bytes().all(|b| b.is_ascii_hexdigit());
        // A value that repeats its key's word is a label (`'sid_et': 'sid_ET.UTF-8'`).
        // One- and two-letter words (`b` of `B2_APPLICATION_KEY`) also occur in random
        // values split at case changes.
        let echoes = split_key(v)
            .iter()
            .any(|w| w.len() >= 3 && key.words.contains(w));
        if !random || v.chars().count() < 8 || hash_field || echoes {
            return None;
        }
        return Some(found(0.7, R_WEAK, ident_check));
    }
    if flag {
        return Some(found(if random { 0.9 } else { 0.8 }, R_FLAG, ident_check));
    }
    if random {
        return Some(found(0.9, R_STRONG_RANDOM, ident_check));
    }
    // Tokens, keys and secrets are generated: a plain word or identifier there (`token1:
    // "Hello"`, `'secret': 'ticketBlob'`) is a fixture name or a UI string. Paranoid only.
    // A `.env` line keeps it: there the value is configuration whatever it looks like.
    let machine = matches!(
        label,
        Label::Token | Label::ApiKey | Label::Secret | Label::PrivateKey
    );
    if machine && !dotenv && is_identifier(v) && score < WORD_SCORE {
        return Some(found(0.45, R_STRONG_PLAIN, ident_check));
    }
    Some(found(0.75, R_STRONG_PLAIN, ident_check))
}

/// Every character is a digit (ASCII, Persian or Arabic-Indic).
fn all_digits(v: &str) -> bool {
    !v.is_empty()
        && v.chars().all(|c| {
            c.is_ascii_digit()
                || ('\u{06F0}'..='\u{06F9}').contains(&c)
                || ('\u{0660}'..='\u{0669}').contains(&c)
        })
}

/// ASCII letters and digits both.
fn letters_and_digits(v: &str) -> bool {
    v.bytes().any(|b| b.is_ascii_digit()) && v.bytes().any(|b| b.is_ascii_alphabetic())
}

/// Six or more characters, digits and a symbol a number never holds (`1234!5678#`); dates,
/// times, versions, amounts and phone numbers use only `.,:-/+` and blanks.
fn digit_symbols(v: &str) -> bool {
    v.chars().count() >= 6
        && !v.contains(char::is_whitespace)
        && v.bytes().any(|b| b.is_ascii_digit())
        && v.bytes().any(|b| b"!@#$%^&*?~|;".contains(&b))
}

/// Words a log line writes in a secret's field to say what happened to it.
const LOG_STATES: &[&str] = &[
    "redacted",
    "masked",
    "hidden",
    "present",
    "missing",
    "provided",
    "invalid",
    "filtered",
    "removed",
    "omitted",
    "supplied",
    "required",
    "changed",
    "updated",
    "expired",
    "accepted",
    "rejected",
    "correct",
    "incorrect",
    "matched",
    "mismatch",
    "mismatched",
    "absent",
    "unset",
    "empty",
    "sanitized",
    "scrubbed",
    "censored",
    "elided",
    "verified",
];

/// `8-4-4-4-12` hex groups.
fn uuid_like(v: &str) -> bool {
    let groups: Vec<&str> = v.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Classifier score below which a value reads as a word or identifier.
const WORD_SCORE: f32 = 0.2;

fn judge_command(text: &str, s: usize, e: usize) -> Option<Found> {
    let v = &text[s..e];
    // `-pPASSWORD.` in a sentence: the full stop is the sentence's.
    if never_a_value(v, Label::Password)
        || never_a_value(v.trim_end_matches('.'), Label::Password)
        || v == "*"
    {
        return None;
    }
    let random = value_score(v) >= 0.5;
    Some(Found {
        start: s,
        end: e,
        label: Label::Password,
        confidence: if random { 0.9 } else { 0.8 },
        reason: R_COMMAND,
        ident_check: false,
    })
}

fn judge_prose(text: &str, key: &Key, hit: &ProseHit) -> Option<Found> {
    let v = &text[hit.value.clone()];
    let label = key.class.label;
    let strong = key.class.strength == Strength::Strong;
    // A strong key needs its own last word to be the lexicon word (`pw`, not `pw_prod`); an
    // option (`--password`) or a key glued to a quote or path is not prose.
    if !key.class.last_consumed || key.dashes > 0 {
        return None;
    }
    // Without a connector a value starts with a letter or digit and is not quoted at all:
    // `(Ctrl+1)` and `` `ops_eyJ` `` are Markdown. The key is then often an English verb
    // ("pass stdout=PIPE", "pass f.fileno()"): no `pass`, and nothing code-like.
    let raw_first = text[hit.raw.clone()].chars().next();
    let first = v.chars().next();
    let direct_ok = key.words.last().is_some_and(|w| w != "pass")
        && !v.contains(['=', '(', ')', '[', ']', '{', '}', '<', '>', '\\']);
    // After a connector ("the password is …") any characters are the value's. Markdown
    // emphasis and brackets around a plain word (`**generated**`, `*(contextual)*`) are
    // judged by the word inside.
    let core = v.trim_matches(['*', '_', '~', '(', ')', '[', ']', '{', '}', '<', '>']);
    // `<base64 user:password>`: a placeholder in angle brackets over several words.
    let angle_words = v.starts_with('<')
        && !v.contains('>')
        && text[hit.raw.end..line_end(text, hit.raw.end)].contains('>');
    // `NTLM\|Negotiate`, `read/write`: plain words as alternatives, not a password.
    let alternatives = core.contains(['|', '\\', '/'])
        && core
            .split(['|', '\\', '/'])
            .all(|w| w.chars().all(char::is_alphabetic));
    if !hit.connector
        && !(first.is_some_and(char::is_alphanumeric)
            && raw_first.is_some_and(char::is_alphanumeric)
            && direct_ok)
        || core.is_empty()
        || angle_words
        || alternatives
        || is_tech_term(v)
        || core != v && is_tech_term(core)
    {
        return None;
    }
    for b in &hit.between {
        let w = text[b.clone()].to_lowercase();
        if is_negative(w.trim_matches(|c: char| !c.is_alphanumeric())) {
            return None;
        }
    }
    let lower = v.to_lowercase();
    if never_a_value(v, label)
        || is_stopword(&lower)
        || is_lexicon_word(&lower)
        || v.chars().filter(|c| c.is_alphanumeric()).count() < 3
    {
        return None;
    }
    let found = |confidence: f32, reason: &'static str| Found {
        start: hit.value.start,
        end: hit.value.end,
        label,
        confidence,
        reason,
        ident_check: false,
    };
    let digits = v.bytes().all(|b| b.is_ascii_digit());
    if label == Label::Pin {
        return (digits && (4..=12).contains(&v.len())).then(|| found(0.7, R_PIN));
    }
    if digits {
        return None;
    }
    let score = value_score(v);
    if !strong {
        return (score >= 0.6 && v.chars().count() >= 12).then(|| found(0.7, R_PROSE));
    }
    // Directly after the key ("the wifi pw Kx9!mPq2vL") the bar is higher than after a
    // connector: digits among letters or a random look; a CamelCase word is a type name
    // ("unexpected token BasicParseError").
    let has_digit_letter =
        v.bytes().any(|b| b.is_ascii_digit()) && v.chars().any(char::is_alphabetic);
    let direct_bar = hit.connector || has_digit_letter || score >= 0.5;
    if direct_bar && looks_like_password(core) {
        return Some(found(0.65 + 0.15 * score, R_PROSE));
    }
    // Paranoid only (below the Balanced threshold): "the password is hunter". Not after a
    // plural key or an arrow ("secrets are showing", "Bearer eyJ… → Authorization"), and
    // not a participle or adverb ("is unmistakable", "are skipped").
    let singular = key
        .words
        .last()
        .is_some_and(|w| !w.ends_with('s') || w.ends_with("ss"));
    let copula = !matches!(hit.conn.as_str(), "are" | "→" | "->" | "=>");
    let english = [
        "ing", "ed", "en", "able", "ible", "ly", "tion", "ful", "less",
    ]
    .iter()
    .any(|suf| lower.ends_with(suf))
        && !lower.ends_with("fly");
    (hit.connector
        && singular
        && copula
        && !english
        && v.chars().count() >= 6
        && v.chars().all(char::is_alphanumeric))
    .then(|| found(0.35, R_PROSE))
}

/// `base64`, `SHA-256`, `AES-128`, `464/tcp`: technical words, not passwords.
fn is_tech_term(v: &str) -> bool {
    let lower = v.to_ascii_lowercase();
    if lexicon::TECH_TERMS.contains(&lower.as_str()) {
        return true;
    }
    let alg = lower.trim_end_matches(|c: char| c.is_ascii_digit());
    let alg = alg.trim_end_matches(['-', '_']);
    let digits = lower.len() > alg.len();
    if digits
        && matches!(
            alg,
            "sha"
                | "md"
                | "aes"
                | "rsa"
                | "hmac-sha"
                | "hkdf-sha"
                | "pbkdf"
                | "des"
                | "rc"
                | "ecdsa-p"
                | "p"
                | "ed"
                | "x"
                | "utf"
                | "base"
                | "ipv"
                | "http"
                | "tls"
                | "ssl"
                | "sha3"
                | "blake"
        )
    {
        return true;
    }
    // Service ports: `464/tcp`.
    if let Some((port, proto)) = lower.split_once('/') {
        if port.bytes().all(|b| b.is_ascii_digit()) && matches!(proto, "tcp" | "udp") {
            return true;
        }
    }
    false
}

/// More `(`/`[` than `)`/`]`: a truncated expression (`ValueKey([7`, `\s*[`).
fn unbalanced(v: &str) -> bool {
    v.matches('(').count() > v.matches(')').count()
        || v.matches('[').count() > v.matches(']').count()
}

/// Shapes that are never a secret value, whatever the key.
fn never_a_value(v: &str, label: Label) -> bool {
    let t = v.trim();
    if t.chars().count() < 3 && label != Label::Pin || t.is_empty() {
        return true;
    }
    if is_placeholder(t) || is_filler(t) {
        return true;
    }
    // An earlier placeholder of the default template with a built-in label: the engine
    // protects it anyway. Any other text framed like one is judged as a value.
    if is_builtin_placeholder(t.trim_matches(|c| c == '"' || c == '\'' || c == '`')) {
        return true;
    }
    let lower = t.to_lowercase();
    let keyword = |w: &str| KEYWORDS.contains(&w) || matches!(w, "n/a" | "na" | "tbd");
    if keyword(&lower) {
        return true;
    }
    // `(none)`, `[null]`, `<empty>`: a keyword or placeholder in brackets.
    for (open, close) in [('(', ')'), ('[', ']'), ('<', '>')] {
        if let Some(inner) = t.strip_prefix(open).and_then(|r| r.strip_suffix(close)) {
            if keyword(&inner.to_lowercase()) || is_placeholder(inner) {
                return true;
            }
        }
    }
    // YAML tags that hold no value of their own: `!vault |`, `!secret db_pw`, `!Ref Pw`.
    if let Some((tag, rest)) = t.strip_prefix('!').and_then(|r| r.split_once([' ', '\t'])) {
        let rest = rest.trim_start();
        let reference = matches!(
            tag.trim_start_matches('!').to_ascii_lowercase().as_str(),
            "secret" | "env" | "include" | "vault" | "ref" | "sub" | "getatt" | "importvalue"
        );
        if reference || rest.starts_with(['|', '>']) {
            return true;
        }
    }
    // Integer constants: `0x08074b50`.
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        if hex.len() <= 16 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return true;
        }
    }
    if !t.chars().any(char::is_alphanumeric) {
        return true;
    }
    if t.contains("${")
        || t.contains("{{")
        || t.contains("%(")
        || t.starts_with("$(")
        || t.contains("---")
        || t.contains("***")
        || t.contains("===")
    {
        return true;
    }
    if brace_template(t) {
        return true;
    }
    // A URL belongs to the URL detectors (userinfo password, query tokens): the user, host
    // and path stay readable, and the password keeps one placeholder everywhere.
    if t.contains("://") {
        return true;
    }
    is_path(t)
}

/// `{db_password}`, `{user}_{env}`, `{0}`: format fields. Every letter and digit sits in a
/// `{…}` group; outside them only separators. `p{ss}w0rd` and `{CedarQuill37}` (capitals
/// with digits, as a password has them and a field name does not) are values.
fn brace_template(t: &str) -> bool {
    let mut groups = 0;
    let mut rest = t;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('{') {
            let Some(close) = r.find('}') else {
                return false;
            };
            let name = &r[..close];
            let upper = name.bytes().any(|c| c.is_ascii_uppercase());
            let digit = name.bytes().any(|c| c.is_ascii_digit());
            if name.contains('{') || upper && digit {
                return false;
            }
            groups += 1;
            rest = &r[close + 1..];
        } else {
            let c = rest.chars().next().unwrap_or(' ');
            if c.is_alphanumeric() {
                return false;
            }
            rest = &rest[c.len_utf8()..];
        }
    }
    groups > 0
}

fn is_path(v: &str) -> bool {
    const EXT: &[&str] = &[
        ".pem",
        ".key",
        ".crt",
        ".cer",
        ".p12",
        ".pfx",
        ".json",
        ".txt",
        ".yaml",
        ".yml",
        ".env",
        ".conf",
        ".cfg",
        ".ini",
        ".jks",
        ".keystore",
        ".gpg",
        ".asc",
        ".pub",
        ".der",
        ".kdbx",
    ];
    let lower = v.to_ascii_lowercase();
    let prefixed = ["/", "./", "../", "~/", "\\\\", "%"]
        .iter()
        .any(|p| lower.starts_with(p))
        || (lower.len() > 2 && lower.as_bytes()[1] == b':' && lower.as_bytes()[2] == b'\\');
    let ext = EXT.iter().any(|e| lower.ends_with(e)) && !lower.contains(' ');
    // Windows and registry paths: `SOFTWARE\Microsoft\…`, `C:\Users\…`.
    // JSON and C escapes (`\\`, `\"`) are one character, not a separator.
    // Segments hold the characters of file and key names; `Cedar\Quill\(x]^y` is a value.
    let unescaped = v.replace("\\\\", "\\").replace("\\\"", "\"");
    let segment = |s: &str| {
        s.chars()
            .all(|c| c.is_alphanumeric() || " ._-()$%~:{}".contains(c))
    };
    let backslashes = unescaped.split('\\').filter(|s| !s.is_empty()).count() >= 3
        && unescaped.split('\\').all(segment);
    backslashes || (prefixed || ext) && value_score(v) < 0.6
}

/// A value that only makes sense as code: a call, an index, `await x`, an operator.
fn is_expression(v: &str) -> bool {
    expression(v, false, false)
}

/// [`is_expression`] for a literal value (`literal`): a pointer or YAML alias prefix
/// (`*CedarQuill`, `&x`, `!x`) is a character of the password except in YAML (`yaml`), and a
/// trailing `(` or `[` only opens a call after a code name (`getpass(`, not `Cedar37(`).
fn expression(v: &str, literal: bool, yaml: bool) -> bool {
    let v = if literal {
        v.strip_suffix(';').unwrap_or(v)
    } else {
        v
    };
    if is_cmdlet(v) {
        return true;
    }
    let lower = v.to_ascii_lowercase();
    for kw in [
        "await ", "new ", "yield ", "typeof ", "keyof ", "lambda", "function", "async ", "not ",
    ] {
        if lower.starts_with(kw) {
            return true;
        }
    }
    if [
        "||", "&&", "??", " + ", " ? ", "=>", " | ", " & ", " * ", " / ", " % ",
    ]
    .iter()
    .any(|op| v.contains(op))
    {
        return true;
    }
    // A call chain: `process.current_process().authkey`, `a.b(c).d`.
    if v.contains('.')
        && v.contains('(')
        && v.chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
        && v.chars().all(|c| {
            c.is_alphanumeric() || matches!(c, '_' | '$' | '.' | '(' | ')' | '[' | ']' | '\'' | '"')
        })
    {
        return true;
    }
    // Paths and macros: `TokenStreamBuilder::new()`, `token::Paren`, `Token![fn]`,
    // `quote!(#X)`; ABNF: `1*tchar`.
    if v.contains("::")
        || v.contains("![")
        || v.contains("!(")
        || v.contains("!{")
        || v.contains(").")
        || v.ends_with('!') && is_macro_name(&v[..v.len() - 1])
    {
        return true;
    }
    // Objective-C selectors: `initWithSpace:credential:count:`.
    if v.matches(':').count() >= 2 && v.split(':').all(|s| s.is_empty() || is_identifier(s)) {
        return true;
    }
    // ABNF repetition `1*tchar`; in a literal only with its count (`*Cedar37` is a value).
    let digits_star = v.trim_start_matches(|c: char| c.is_ascii_digit());
    let counted = digits_star.len() < v.len();
    if digits_star.starts_with('*') && digits_star.len() > 1 && (counted || !literal) {
        return true;
    }
    // `&password`, `*ptr`, `!flag`; YAML `*alias`
    if let Some(rest) = v.strip_prefix(['&', '*', '!']).filter(|_| !literal || yaml) {
        if is_identifier(rest) || member_path(rest).is_some() {
            return true;
        }
    }
    // `getpass()`, `os.getenv("X")`, `request.form['password']`, `getpass(`
    let i = v
        .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '$' | '.')))
        .unwrap_or(v.len());
    if i > 0 && i < v.len() {
        let head = &v[..i];
        let opener = v[i..].starts_with(['(', '[']);
        let closes = v.ends_with([')', ']', '(', '[']);
        let head_ok = head
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$');
        let open_end = v.ends_with(['(', '[']);
        if opener && closes && head_ok && !(literal && open_end && !code_name(head)) {
            return true;
        }
    }
    false
}

/// A name as code writes it: `getpass`, `get_pw`, `os.getenv`, `$pw`, `ENV`. A capitalized
/// word with digits (`Cedar37`) is not one.
fn code_name(v: &str) -> bool {
    let first = v.bytes().next();
    v.contains(['.', '_'])
        || first.is_some_and(|c| c.is_ascii_lowercase() || c == b'$')
        || !v.bytes().any(|c| c.is_ascii_lowercase())
        || !v.bytes().any(|c| c.is_ascii_digit())
}

/// `["a", "b"]`, `{ user = "x" }`, `[a, b]`: an array or table, not one value.
fn is_structure(v: &str) -> bool {
    let inner = v
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .or_else(|| v.strip_prefix('{').and_then(|r| r.strip_suffix('}')));
    inner.is_some_and(|i| {
        let i = i.trim();
        i.is_empty()
            || i.contains(['"', '\''])
            || i.contains(", ")
            || i.contains(": ")
            || i.contains(" = ")
    })
}

/// `quote`, `vec`, `include_str`, `Token` (as in `quote!`): a macro name has no digits and is
/// snake case, or one capitalized word. `Winter2024!` and `Sunshine!` are passwords.
/// A PowerShell cmdlet name (`New-Object`, `Get-Content`, `ConvertTo-SecureString`).
fn is_cmdlet(v: &str) -> bool {
    const VERBS: &[&str] = &[
        "Get",
        "Set",
        "New",
        "Read",
        "Write",
        "ConvertTo",
        "ConvertFrom",
        "Import",
        "Export",
        "Invoke",
        "Add",
        "Remove",
        "Start",
        "Stop",
        "Test",
        "Select",
        "Out",
        "Join",
        "Split",
    ];
    v.split_once('-').is_some_and(|(verb, noun)| {
        VERBS.contains(&verb)
            && noun.starts_with(|c: char| c.is_ascii_uppercase())
            && noun.chars().all(|c| c.is_ascii_alphabetic())
    })
}

fn is_macro_name(v: &str) -> bool {
    if !is_identifier(v) || v.bytes().any(|b| b.is_ascii_digit()) {
        return false;
    }
    v.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') || v == "Token"
}

/// `string`, `String`, `&str`, `Option<String>`, `string[]`, `str | None`.
fn is_type_name(v: &str) -> bool {
    let t = v.trim().trim_end_matches([';', ',']).trim();
    let lower = t.to_ascii_lowercase();
    if TYPE_NAMES.contains(&lower.as_str()) {
        return true;
    }
    if let Some((head, _)) = lower.split_once(" |") {
        if TYPE_NAMES.contains(&head.trim()) {
            return true;
        }
    }
    if ["*mut ", "*const ", "&mut ", "&'", "&self", "dyn ", "impl "]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        return true;
    }
    let ident_head = |s: &str| {
        !s.is_empty()
            && s.chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
            && s.chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | ':' | '.'))
    };
    // Generics: Option<String>, Vec<u8>, Promise<string>
    if let Some(open) = t.find('<') {
        if t.ends_with('>') && ident_head(&t[..open]) {
            return true;
        }
    }
    // Arrays and optionals: string[], String?, Optional[str]
    if let Some(head) = t.strip_suffix("[]").or_else(|| t.strip_suffix('?')) {
        if TYPE_NAMES.contains(&head.to_ascii_lowercase().as_str()) {
            return true;
        }
    }
    false
}

/// `a.b.c` where every segment is an identifier.
fn member_path(v: &str) -> Option<Vec<&str>> {
    if !v.contains('.') {
        return None;
    }
    let segs: Vec<&str> = v.split('.').collect();
    segs.iter().all(|s| is_identifier(s)).then_some(segs)
}

fn is_identifier(v: &str) -> bool {
    let mut cs = v.chars();
    cs.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// An identifier whose words mark it as code (`password_input`, `userValue`, `pw`).
/// `Temp1234`, `KeyWest2024`, `user2024`: letters with digits and no `_`, the shape of a
/// human password; a variable named with code words mostly has an `_` or no digits.
fn digits_in_words(v: &str) -> bool {
    v.bytes().any(|b| b.is_ascii_digit())
        && v.bytes().any(|b| b.is_ascii_alphabetic())
        && !v.contains('_')
}

fn code_words(v: &str) -> bool {
    split_key(v)
        .iter()
        .any(|w| CODE_WORDS.contains(&w.as_str()) || is_lexicon_word(w))
}

/// A sentence rather than a value: `"Enter your password"`, `"Passwort"`, `"required"`.
fn prose_like(v: &str) -> bool {
    // More than 8 words is a sentence; only the first 9 are needed to tell.
    let words: Vec<String> = v
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .take(9)
        .collect();
    if words.is_empty() {
        return true;
    }
    if words.len() == 1 {
        let w = &words[0];
        return is_lexicon_word(w) || is_stopword(w) || lexicon_phrase_word(w);
    }
    // Several words: a passphrase (`correct horse battery staple`) has no function words.
    words.len() > 8
        || v.trim_end().ends_with(['.', '?', '!', ':'])
        || words.iter().any(|w| {
            is_lexicon_word(w)
                || lexicon::FUNCTION_WORDS.contains(&w.as_str())
                || lexicon_phrase_word(w)
        })
}

/// Translations of "password" that are phrase parts (`passe`, `عبور`).
fn lexicon_phrase_word(w: &str) -> bool {
    lexicon::PHRASE_ANCHORS.contains(&w) && !matches!(w, "url" | "uri" | "codes" | "phrase")
}

/// `postgres://user:pw@host`, `Server=…;Password=…`.
fn has_credentials(v: &str) -> bool {
    let lower = v.to_ascii_lowercase();
    if let Some(i) = lower.find("://") {
        let rest = &lower[i + 3..];
        let auth_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let auth = &rest[..auth_end];
        if let Some(at) = auth.rfind('@') {
            if auth[..at].contains(':') {
                return true;
            }
        }
    }
    lower.contains("password=") || lower.contains("pwd=")
}

/// Six or more alphabetic words: a mnemonic.
fn phrase_like(v: &str) -> bool {
    let words: Vec<&str> = v.split_whitespace().collect();
    words.len() >= 6 && words.iter().all(|w| w.chars().all(char::is_alphabetic))
}

/// Backup codes: `1234-5678 9012-3456`.
fn code_groups(v: &str) -> bool {
    v.split_whitespace().all(|g| {
        g.split('-')
            .all(|p| p.len() >= 3 && p.chars().all(|c| c.is_ascii_alphanumeric()))
    }) && v.chars().any(|c| c.is_ascii_digit())
}

// ---------------------------------------------------------------------------------------
// Identifier use elsewhere
// ---------------------------------------------------------------------------------------

/// `variable "db_password" { default = "v" }`: the name is in the block header, the value on
/// a `default` line of the block. Only quoted literals, and only for strong secret names.
fn terraform_defaults(text: &str, found: &mut Vec<Found>) {
    const HEAD: &str = "variable \"";
    let mut from = 0;
    while let Some(i) = text[from..].find(HEAD) {
        let ns = from + i + HEAD.len();
        from = ns;
        let Some(ne) = text[ns..].find('"').map(|j| ns + j) else {
            break;
        };
        let name = &text[ns..ne];
        if name.len() > 64
            || !text[ne + 1..]
                .trim_start_matches([' ', '\t'])
                .starts_with('{')
        {
            continue;
        }
        let Some(class) = keyname::classify(&split_key(name)) else {
            continue;
        };
        if class.strength != Strength::Strong {
            continue;
        }
        // The block: up to a `}` that starts a line, at most 2 KiB, and never into the next
        // block, so blocks left open do not share (and repeat) their `default` lines.
        let mut be = floor(text, ne + 2048);
        be = text[ne..be].find(HEAD).map_or(be, |j| ne + j);
        let body_end = text[ne..be].find("\n}").map_or(be, |j| ne + j);
        let mut ls = ne;
        for line in text[ne..body_end].split_inclusive('\n') {
            let start = ls;
            ls += line.len();
            let Some(rest) = line.trim_start().strip_prefix("default") else {
                continue;
            };
            let Some(rest) = rest.trim_start().strip_prefix('=') else {
                continue;
            };
            let Some(q) = rest.trim_start().strip_prefix('"') else {
                continue;
            };
            let Some(qe) = q.find('"') else {
                continue;
            };
            let v = &q[..qe];
            let vs = start + (line.len() - q.len());
            let lower = v.to_lowercase();
            if v.trim() != v
                || v.chars().filter(|c| c.is_alphanumeric()).count() < 3
                || v.contains("${")
                || never_a_value(v, class.label)
                || is_stopword(&lower)
                || is_lexicon_word(&lower)
            {
                continue;
            }
            found.push(Found {
                start: vs,
                end: vs + v.len(),
                label: class.label,
                confidence: 0.75,
                reason: R_TF_DEFAULT,
                ident_check: false,
            });
        }
    }
}

/// Terraform plan: `~ password = "old" -> "new"`: the new value of a changed secret too.
fn plan_changes(text: &str, found: &mut Vec<Found>) {
    let b = text.as_bytes();
    let mut extra = Vec::new();
    for f in found.iter() {
        let Some(&q) = b.get(f.end) else { continue };
        if !matches!(q, b'"' | b'\'') || !text[f.end + 1..].starts_with(" -> ") {
            continue;
        }
        let vs = f.end + 1 + " -> ".len();
        if b.get(vs) != Some(&q) {
            continue;
        }
        let le = line_end(text, vs + 1);
        let Some(close) = text[vs + 1..le].find(q as char) else {
            continue;
        };
        let (s, e) = (vs + 1, vs + 1 + close);
        let v = &text[s..e];
        if v.trim().is_empty() || never_a_value(v, f.label) {
            continue;
        }
        extra.push(Found {
            start: s,
            end: e,
            ..f.clone()
        });
    }
    found.extend(extra);
}

/// Most bytes of one `INSERT INTO t (cols) VALUES (…)` statement read.
const MAX_INSERT: usize = 4096;

/// SQL `INSERT INTO users (username, password) VALUES ('admin', 'v'), (…)`: the string
/// literal in a secret-named column of each row.
fn sql_inserts(text: &str, found: &mut Vec<Found>) {
    const HEAD: &str = "insert into ";
    let lower_head = |i: usize| {
        text.get(i..i + HEAD.len())
            .is_some_and(|h| h.eq_ignore_ascii_case(HEAD))
    };
    let b = text.as_bytes();
    let mut i = 0;
    while let Some(j) = memchr_ci(text, i, b'i') {
        i = j + 1;
        if !lower_head(j) || j > 0 && b[j - 1].is_ascii_alphanumeric() {
            continue;
        }
        let end = ceil(text, j + MAX_INSERT);
        let stmt = &text[j..end];
        // The table name, then the column list right after it.
        let name = stmt[HEAD.len()..]
            .bytes()
            .take_while(|c| c.is_ascii_alphanumeric() || b"_.\"`[]".contains(c))
            .count();
        let open = HEAD.len() + name;
        let open = open
            + stmt[open..]
                .bytes()
                .take_while(|c| *c == b' ' || *c == b'\t')
                .count();
        if name == 0 || stmt.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let cols_end = ceil(stmt, open + 512).min(stmt.len());
        let Some(close) = stmt[open..cols_end].find(')').map(|c| open + c) else {
            continue;
        };
        let cols: Vec<&str> = stmt[open + 1..close]
            .split(',')
            .map(|c| c.trim().trim_matches(['"', '`', '[', ']']))
            .collect();
        let secret: Vec<(usize, Key)> = cols
            .iter()
            .enumerate()
            .filter_map(|(k, c)| {
                column_class(c)
                    .filter(|(st, _)| *st == Strength::Strong)
                    .map(|(_, key)| (k, key))
            })
            .collect();
        if secret.is_empty() {
            continue;
        }
        let rest = &stmt[close + 1..];
        let Some(v) = rest
            .char_indices()
            .find(|&(_, c)| !c.is_whitespace())
            .filter(|&(k, _)| {
                rest[k..]
                    .get(..6)
                    .is_some_and(|w| w.eq_ignore_ascii_case("values"))
            })
            .map(|(k, _)| close + 1 + k + 6)
        else {
            continue;
        };
        // Each row: `(` literal, literal, … `)`, rows joined by `,`, up to `;`.
        let sb = stmt.as_bytes();
        let mut p = v;
        loop {
            while p < sb.len() && sb[p].is_ascii_whitespace() {
                p += 1;
            }
            if sb.get(p) != Some(&b'(') {
                break;
            }
            p += 1;
            let mut k = 0;
            let mut ok = true;
            while ok && p < sb.len() {
                while p < sb.len() && sb[p].is_ascii_whitespace() {
                    p += 1;
                }
                let (vs, ve);
                if sb.get(p) == Some(&b'\'') {
                    let s0 = p + 1;
                    let mut e0 = s0;
                    while e0 < sb.len() && !(sb[e0] == b'\'' && sb.get(e0 + 1) != Some(&b'\'')) {
                        e0 += if sb[e0] == b'\'' { 2 } else { 1 };
                    }
                    if e0 >= sb.len() {
                        ok = false;
                        break;
                    }
                    (vs, ve) = (s0, e0);
                    p = e0 + 1;
                    if let Some((_, key)) = secret.iter().find(|(c, _)| *c == k) {
                        let raw = RawPair {
                            value: Some(j + vs..j + ve),
                            syntax: Syntax::Assign,
                            quoted: true,
                            sep: Sep::None,
                            tight: false,
                            line_start: false,
                            code: false,
                            args: false,
                            stmt_end: false,
                            literal: false,
                            scanned: j + ve,
                        };
                        if let Some(f) = judge_pair(text, key, &raw) {
                            found.push(f);
                        }
                    }
                } else {
                    while p < sb.len() && !matches!(sb[p], b',' | b')' | b'\'') {
                        p += 1;
                    }
                }
                while p < sb.len() && sb[p].is_ascii_whitespace() {
                    p += 1;
                }
                match sb.get(p) {
                    Some(b',') => {
                        p += 1;
                        k += 1;
                    }
                    Some(b')') => {
                        p += 1;
                        break;
                    }
                    _ => ok = false,
                }
            }
            if !ok {
                break;
            }
            while p < sb.len() && sb[p].is_ascii_whitespace() {
                p += 1;
            }
            if sb.get(p) != Some(&b',') {
                break;
            }
            p += 1;
        }
        i = j + p.max(1);
    }
}

/// The next `c` or its upper case from `from` on.
fn memchr_ci(text: &str, from: usize, c: u8) -> Option<usize> {
    let up = c.to_ascii_uppercase();
    text.as_bytes()[from..]
        .iter()
        .position(|&x| x == c || x == up)
        .map(|k| from + k)
}

/// Most columns a header row may have.
const MAX_COLUMNS: usize = 64;

/// The cells of a row: a Markdown row split at `|` (not `\|`), or a CSV/TSV row split at
/// `delim`. Ranges are relative to the line.
fn cells(line: &str, delim: Option<u8>) -> Vec<std::ops::Range<usize>> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    match delim {
        None => {
            let t = line.trim_end();
            let Some(lead) = t.find('|') else {
                return out;
            };
            let mut st = lead + 1;
            for i in lead + 1..t.len() {
                if b[i] == b'|' && b[i - 1] != b'\\' {
                    out.push(st..i);
                    st = i + 1;
                }
            }
            if st < t.len() && !t[st..].trim().is_empty() {
                out.push(st..t.len());
            }
        }
        Some(d) => {
            let t = line.trim_end_matches(['\r', '\n']);
            let mut st = 0;
            for (i, &c) in t.as_bytes().iter().enumerate() {
                if c == d {
                    out.push(st..i);
                    st = i + 1;
                }
            }
            out.push(st..t.len());
        }
    }
    out
}

/// A header cell's key class, when it names a secret on its own (`password`, `api_key`).
fn column_class(cell: &str) -> Option<(lexicon::Strength, Key)> {
    let name = cell.trim().trim_matches(['"', '`', '*']);
    if name.is_empty()
        || name.len() > 40
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ' '))
    {
        return None;
    }
    let words = split_key(name);
    let class = keyname::classify(&words)?;
    class.last_consumed.then_some(())?;
    let upper = name
        .bytes()
        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    Some((
        class.strength,
        Key {
            start: 0,
            end: 0,
            token_start: 0,
            dashes: 0,
            class,
            words,
            cjk: false,
            upper,
            member: false,
        },
    ))
}

/// A header row that names its columns: every cell a short plain name.
fn header_names(line: &str, cs: &[std::ops::Range<usize>]) -> bool {
    (2..=MAX_COLUMNS).contains(&cs.len())
        && cs.iter().all(|r| {
            let n = line[r.clone()].trim().trim_matches(['"', '`', '*']);
            !n.is_empty()
                && n.len() <= 40
                && n.chars().next().is_some_and(char::is_alphabetic)
                && n.chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ' '))
        })
}

/// `|---|:--:|`: the row under a Markdown table header.
fn delimiter_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.contains('-') && t.bytes().all(|c| b"|-: ".contains(&c))
}

/// A table being read: its delimiter (`None`: Markdown), column count and secret columns.
type Table = (Option<u8>, usize, Vec<(usize, Key)>);

/// Tables with a header row: in a Markdown table (`| user | password |` over `|---|---|`)
/// or a CSV/TSV block (`username,password,role`), the cells under a secret-named column
/// are values. Rows run to the first line of another shape. One pass over the lines.
fn table_columns(text: &str, found: &mut Vec<Found>) {
    if !text.contains(['|', ',', '\t']) {
        return;
    }
    // (delimiter, column count, secret columns)
    let mut table: Option<Table> = None;
    // A Markdown header waiting for its delimiter row.
    let mut header: Option<(usize, Vec<(usize, Key)>)> = None;
    let mut ls = 0;
    for line in text.split_inclusive('\n') {
        let start = ls;
        ls += line.len();
        if let Some((delim, n, cols)) = &table {
            let cs = cells(line, *delim);
            let row = match delim {
                None => line.trim_start().starts_with('|') && !delimiter_row(line),
                Some(_) => cs.len() == *n,
            };
            if row {
                for (i, key) in cols {
                    let Some(r) = cs.get(*i) else { continue };
                    let cell = &line[r.clone()];
                    let mut t = cell.trim();
                    for q in ['"', '`'] {
                        if t.len() >= 2 && t.starts_with(q) && t.ends_with(q) {
                            t = &t[1..t.len() - 1];
                        }
                    }
                    if t.is_empty() || t.contains(char::is_whitespace) {
                        continue;
                    }
                    let vs = start + r.start + cell.find(t).unwrap_or(0);
                    let raw = RawPair {
                        value: Some(vs..vs + t.len()),
                        syntax: Syntax::Assign,
                        quoted: false,
                        sep: Sep::None,
                        tight: false,
                        line_start: false,
                        code: false,
                        args: false,
                        stmt_end: false,
                        literal: true,
                        scanned: vs + t.len(),
                    };
                    if let Some(f) = judge_pair(text, key, &raw) {
                        found.push(f);
                    }
                }
                continue;
            }
            table = None;
        }
        if let Some((n, cols)) = header.take() {
            if delimiter_row(line) {
                table = Some((None, n, cols));
                continue;
            }
        }
        let t = line.trim();
        let (delim, cs) = if t.starts_with('|') {
            (None, cells(line, None))
        } else if let Some(d) = b"\t,;".iter().copied().find(|d| t.as_bytes().contains(d)) {
            (Some(d), cells(line, Some(d)))
        } else {
            continue;
        };
        if !header_names(line, &cs) {
            continue;
        }
        let cols: Vec<(usize, Key)> = cs
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                column_class(&line[r.clone()])
                    .filter(|(st, _)| *st == Strength::Strong)
                    .map(|(_, k)| (i, k))
            })
            .collect();
        if cols.is_empty() {
            continue;
        }
        match delim {
            None => header = Some((cs.len(), cols)),
            Some(_) => table = Some((delim, cs.len(), cols)),
        }
    }
}

/// Drops identifier-shaped values that the text also uses as code identifiers: an argument
/// (`login(user, x9k2)`), a call (`x9k2()`), a member (`x9k2.len`), an assignment target.
/// One automaton pass over the text for all such values together; around each match it looks
/// at a few bytes only, so long lines stay linear.
fn drop_identifiers(text: &str, found: &mut Vec<Found>) {
    /// How far around a match the code-use test looks.
    const NEAR: usize = 64;
    let mut values: Vec<&str> = found
        .iter()
        .filter(|f| f.ident_check)
        .map(|f| &text[f.start..f.end])
        .collect();
    if values.is_empty() {
        return;
    }
    values.sort_unstable();
    values.dedup();
    let Ok(ac) = AhoCorasick::builder()
        .match_kind(MatchKind::LeftmostLongest)
        .build(&values)
    else {
        return;
    };
    let mut own: Vec<usize> = found.iter().map(|f| f.start).collect();
    own.sort_unstable();
    let mut used: HashMap<usize, bool> = HashMap::new();
    let b = text.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$';
    for m in ac.find_iter(text) {
        let (s, e) = (m.start(), m.end());
        if own.binary_search(&s).is_ok() {
            continue;
        }
        if s > 0 && ident(b[s - 1]) || e < b.len() && ident(b[e]) {
            continue;
        }
        let lo = floor(text, s.saturating_sub(NEAR));
        let prev = text[lo..s].trim_end_matches([' ', '\t']);
        let next = text[e..ceil(text, e + NEAR)].trim_start_matches([' ', '\t']);
        let prev_c = prev.bytes().last();
        let next_c = next.bytes().next();
        // A word that runs past the window is longer than any keyword.
        let word_start = prev
            .rfind(|c: char| !c.is_alphanumeric())
            .map(|j| j + prev[j..].chars().next().map_or(1, char::len_utf8));
        let prev_word = match word_start {
            Some(j) => &prev[j..],
            None if lo == 0 => prev,
            None => "",
        };
        let code_use = matches!(prev_c, Some(b'(' | b',' | b'.' | b'[' | b'!' | b'{'))
            || matches!(
                next_c,
                Some(b'(' | b')' | b'.' | b'=' | b'[' | b']' | b',' | b';')
            )
            || matches!(
                prev_word,
                "def" | "let" | "const" | "var" | "fn" | "return" | "if" | "not" | "and" | "or"
            );
        if code_use {
            used.insert(m.pattern().as_usize(), true);
        }
    }
    if used.is_empty() {
        return;
    }
    found.retain(|f| {
        if !f.ident_check {
            return true;
        }
        let v = &text[f.start..f.end];
        let idx = values.binary_search(&v).ok();
        !idx.is_some_and(|i| used.contains_key(&i))
    });
}

/// Every label the context engine gives its findings.
pub(crate) fn labels() -> impl Iterator<Item = &'static str> {
    lexicon::Label::ALL.into_iter().map(lexicon::Label::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(text: &str) -> Vec<(String, String, Syntax)> {
        extract_pairs(text)
            .into_iter()
            .map(|p| (text[p.key].to_string(), text[p.value].to_string(), p.syntax))
            .collect()
    }

    #[test]
    fn syntaxes_yield_pairs() {
        let cases: &[(&str, &str, Syntax)] = &[
            (
                "DB_PASSWORD=hunter2prod\n",
                "hunter2prod",
                Syntax::LineAssign,
            ),
            ("x password := \"pw-Fake-1\"", "pw-Fake-1", Syntax::Assign),
            (
                "{'password' => 'pw-Fake-1'}",
                "pw-Fake-1",
                Syntax::QuotedKey,
            ),
            ("<password>pw-Fake-1</password>", "pw-Fake-1", Syntax::Xml),
            (
                "<add key=\"DbPassword\" value=\"pw-Fake-1\"/>",
                "pw-Fake-1",
                Syntax::XmlAttr,
            ),
            ("run --password pw-Fake-1 now", "pw-Fake-1", Syntax::Flag),
            ("run --password=pw-Fake-1 now", "pw-Fake-1", Syntax::Flag),
            ("cmdkey /pass:pw-Fake-1", "pw-Fake-1", Syntax::Flag),
            ("requirepass pw-Fake-1\n", "pw-Fake-1", Syntax::Spaced),
            (
                "$env:API_KEY = \"pw-Fake-1\"",
                "pw-Fake-1",
                Syntax::LineAssign,
            ),
            ("GET /x?token=pw-Fake-1&a=1", "pw-Fake-1", Syntax::Assign),
            (
                "openssl rsa -passin pass:pw-Fake-1",
                "pw-Fake-1",
                Syntax::Flag,
            ),
            (
                "CREATE USER a WITH PASSWORD 'pw-Fake-1';",
                "pw-Fake-1",
                Syntax::Spaced,
            ),
        ];
        for (text, value, syntax) in cases {
            let got = pairs(text);
            assert!(
                got.iter().any(|(_, v, s)| v == value && s == syntax),
                "{text:?}: {got:?}"
            );
        }
    }

    #[test]
    fn commands_yield_pairs() {
        for (text, value) in [
            ("curl -u admin:pw-Fake-1 https://x", "pw-Fake-1"),
            ("mysql -u root -ppw-Fake-1 db", "pw-Fake-1"),
            ("sshpass -p pw-Fake-1 ssh host", "pw-Fake-1"),
            ("ldapsearch -x -w pw-Fake-1 -b dc=x", "pw-Fake-1"),
            ("docker login -u me -p pw-Fake-1 reg", "pw-Fake-1"),
            ("zip -P pw-Fake-1 out.zip a", "pw-Fake-1"),
            ("7z a -ppw-Fake-1 out.7z", "pw-Fake-1"),
            ("sqlplus scott/pw-Fake-1@orcl", "pw-Fake-1"),
            ("ALTER USER x IDENTIFIED BY 'pw-Fake-1';", "pw-Fake-1"),
            (
                "net use Z: \\\\srv\\share pw-Fake-1 /user:dom\\me",
                "pw-Fake-1",
            ),
        ] {
            let got = pairs(text);
            assert!(
                got.iter()
                    .any(|(_, v, s)| v == value && *s == Syntax::Command),
                "{text:?}: {got:?}"
            );
        }
    }

    /// `mysql -u root -p shop` in a string literal of source code: the escape and quote
    /// glued to the database name do not make it password-shaped.
    #[test]
    fn mysql_database_after_p_in_source() {
        let text = "    \"mysql -u root -p shop\\n\",\n";
        assert!(
            pairs(text).iter().all(|(_, v, _)| !v.contains("shop")),
            "{:?}",
            pairs(text)
        );
    }

    #[test]
    fn expressions_and_types() {
        for v in [
            "getpass()",
            "os.getenv(",
            "request.form['password']",
            "await",
        ] {
            assert!(is_expression(v) || KEYWORDS.contains(&v), "{v}");
        }
        assert!(!is_expression("Pa(ss)w0rd"));
        for v in [
            "string",
            "String",
            "&str",
            "Option<String>",
            "string[]",
            "str | None",
        ] {
            assert!(is_type_name(v), "{v}");
        }
    }

    /// A key-character blob longer than a key name hides nothing on the next line.
    #[test]
    fn long_blob_does_not_hide_next_line() {
        let text =
            "client_secretsecretX-Api-KeyX-Api-Keyhellohellohellohelloapi_keyclient_secret\n\
                    DB_PASSWORD=Hunter2-Fake-9x\n";
        let mut out = Vec::new();
        ContextDetector.detect(text, &mut out);
        let got: Vec<&str> = out.iter().map(|c| &text[c.start..c.end]).collect();
        assert_eq!(got, ["Hunter2-Fake-9x"]);
    }

    /// Every pair on a logfmt line is judged in one pass: the first value ending at the
    /// next `name=` field does not skip that field.
    #[test]
    fn logfmt_line_judges_every_pair() {
        let text = "primary_password=Tulip-Harbor-91 backup_password=Cedar-Quill-37 \
                    retry_password=Tulip-Harbor-91\n";
        let keys: Vec<String> = pairs(text).into_iter().map(|p| p.0).collect();
        assert_eq!(
            keys,
            ["primary_password", "backup_password", "retry_password"],
            "{text:?}"
        );
        let mut out = Vec::new();
        ContextDetector.detect(text, &mut out);
        let got: Vec<&str> = out.iter().map(|c| &text[c.start..c.end]).collect();
        assert_eq!(
            got,
            ["Tulip-Harbor-91", "Cedar-Quill-37", "Tulip-Harbor-91"],
            "{text:?}"
        );
    }

    /// Every key and command the context engine knows, followed by runs of 2-, 3- and 4-byte
    /// characters and combining marks whose length sweeps across every byte limit the engine
    /// looks through (key, word, prose, line, value and command windows), at every alignment
    /// of a character to that limit. No slice may split a character.
    #[test]
    fn non_ascii_runs_never_split_a_character() {
        const PREFIXES: &[&str] = &[
            "curl ",
            "curl -u admin:",
            "curl --user=a:",
            "zip -P ",
            "7z a -p",
            "docker login -u me -p ",
            "docker run ",
            "mysql -u root -p",
            "sshpass -p ",
            "ldapsearch -w ",
            "sqlplus scott/",
            "net use Z: ",
            "ConvertTo-SecureString -AsPlainText ",
            "CREATE USER a IDENTIFIED BY '",
            "htpasswd -b f u ",
            "tool login -u a -p ",
            "the login is a / ",
            "password=",
            "DB_PASSWORD=",
            "DB_PASSWORD=\"",
            "DB_PASSWORD==",
            "PGPASSWORD=x ",
            "password: ",
            "password = \"a",
            "password = 'a",
            "\"password\": \"",
            "'password' => '",
            "--password ",
            "--password=",
            "(--password=",
            "cmdkey /pass:",
            "-passin pass:",
            "level=warn password=",
            "a=1 password=",
            "<password>",
            "<add key=\"DbPassword\" value=\"",
            "requirepass ",
            "machine h login u password ",
            "ENV DB_PASSWORD ",
            "WITH PASSWORD '",
            "the password is ",
            "the wifi pw ",
            "Mein Passwort ist ",
            "密码是",
            "パスワードは",
            "variable \"db_password\" {\n  default = \"",
            "token=",
            "x?token=a&",
            "Server=x;Password=",
            "Authorization: Bearer ",
            "api_key: `",
            "\"x\": \"export DB_PASSWORD=",
        ];
        const RUNS: &[&str] = &["é", "日", "😀", "e\u{301}", "é日😀\u{200B}"];
        // The byte windows: prose connector words, neighbour words, key names and identifier
        // look-around, field names, prose and quote look-back, line start, command line and
        // XML element, line end and Terraform block.
        const CAPS: &[usize] = &[16, 24, 64, 128, 256, 512, 1024, 2048, 4096];
        let check = |text: &str, what: &str| {
            let mut out = Vec::new();
            ContextDetector.detect(text, &mut out);
            for c in &out {
                assert!(
                    text.is_char_boundary(c.start) && text.is_char_boundary(c.end),
                    "{what}"
                );
            }
            extract_pairs(text);
        };
        for prefix in PREFIXES {
            for run in RUNS {
                let width = run.chars().map(char::len_utf8).max().unwrap_or(1);
                // Every alignment of a character to every window, with the run ending just
                // before, on and after it, and on the next line too.
                for pad in 0..width {
                    for &cap in CAPS {
                        let lo = (cap - 8).div_ceil(run.len());
                        for n in lo..=(cap + 8).div_ceil(run.len()) {
                            let body = format!("{prefix}{}{}", "a".repeat(pad), run.repeat(n));
                            let what = format!("{prefix:?} {run:?} x{n} pad {pad}");
                            check(&body, &what);
                            check(&format!("{body}\n{prefix}x\n"), &what);
                        }
                    }
                }
                // Lengths from 500 to 1100 bytes in steps of 5 (prime to every character
                // width, so the run ends at every alignment), and 4090 to 4100.
                for len in (500..=1100).step_by(5).chain(4090..=4100) {
                    let mut body = format!("{prefix}{}", run.repeat(len / run.len()));
                    body.push_str(&"a".repeat(len % run.len()));
                    check(&body, &format!("{prefix:?} {run:?} {len} bytes"));
                }
            }
        }
    }
}
