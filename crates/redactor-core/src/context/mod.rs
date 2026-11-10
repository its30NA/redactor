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
            Event::Cmd { end, cmd, .. } => {
                let v = command(text, end, cmd)?;
                let f = judge_command(text, v.start, v.end)?;
                let e = f.end;
                found.push(f);
                Some(e)
            }
        });
        terraform_defaults(text, &mut found);
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

// ---------------------------------------------------------------------------------------
// Value judges
// ---------------------------------------------------------------------------------------

fn judge_pair(text: &str, key: &Key, p: &RawPair) -> Option<Found> {
    let r = p.value.clone()?;
    let v = &text[r.clone()];
    let label = key.class.label;
    let strong = key.class.strength == Strength::Strong;
    let flag = p.syntax == Syntax::Flag;
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
    let digits = v.bytes().all(|b| b.is_ascii_digit());
    if label == Label::Pin {
        return (digits && (4..=12).contains(&v.len())).then(|| found(0.75, R_PIN, false));
    }
    if digits {
        // Numbers only count for PIN keys.
        return None;
    }
    let score = value_score(v);
    let random = score >= 0.5;
    match label {
        Label::ConnectionString if !(has_credentials(v) || random) => return None,
        Label::RecoveryPhrase if !(random || phrase_like(v) || code_groups(v)) => return None,
        _ => {}
    }
    if p.quoted {
        // `" secret "` with blanks at an end, or `'openaiKey'` (an identifier naming a
        // secret, as in a fixture table) is not a secret.
        if v.trim() != v || is_identifier(v) && score < 0.5 && code_words(v) {
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
        if p.code
            || literal_field
            || is_type_name(v)
            || code_shape
            || !v.chars().any(char::is_alphabetic)
        {
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
            if root || shares || p.code || key.member {
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
        if p.code
            || p.stmt_end
            || key.member
            || is_stopword(&lower)
            || code_words(v) && !weak_classic
        {
            return None;
        }
        let env_style = key.upper && p.sep == Sep::Equals && p.tight && p.line_start;
        let ok = if env_style {
            true
        } else if flag || p.syntax == Syntax::Spaced {
            v.len() >= 4
        } else if p.line_start && p.sep == Sep::Colon {
            // YAML: `password: hunter`
            v.chars().count() >= 6
        } else if p.line_start && p.sep == Sep::Equals && p.tight {
            // properties / INI without spaces: `db.password=hunter`
            v.len() >= 4
        } else if p.line_start && p.sep == Sep::Equals && !v.contains(['_', '.']) {
            // INI: `db_password = dragonfly`. A variable in code mostly has an `_` or is
            // used as an identifier elsewhere in the text (`ident_check`).
            v.chars().count() >= 6
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
        let echoes = split_key(v).iter().any(|w| key.words.contains(w));
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

/// Classifier score below which a value reads as a word or identifier.
const WORD_SCORE: f32 = 0.2;

fn judge_command(text: &str, s: usize, e: usize) -> Option<Found> {
    let v = &text[s..e];
    if never_a_value(v, Label::Password) || v == "*" {
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
    // Already redacted.
    if t.contains("[REDACTED") {
        return true;
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
