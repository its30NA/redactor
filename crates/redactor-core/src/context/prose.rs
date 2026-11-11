//! Keys named in prose: "the wifi pw is Kx9!mPq2vL", "Mein Passwort ist …",
//! "contraseña es …", "密码是…".

use std::ops::Range;

use super::lexicon::{CONNECTORS, TRAILING_CONNECTORS};
use super::syntax::{is_cjk, line_end, line_start, Key};

/// A value found after a key in prose.
#[derive(Clone, Debug)]
pub(crate) struct ProseHit {
    pub value: Range<usize>,
    /// The whole token the value was cut from (quotes and brackets included).
    pub raw: Range<usize>,
    /// The key and value are joined by a connector (`is`, `ist`, `es`, `:`), not adjacent.
    pub connector: bool,
    /// The connector word (`is`, `:`, `→`), lowercase; empty when there is none.
    pub conn: String,
    /// Words between the key and the connector (`the password *for the db* is …`).
    pub between: Vec<Range<usize>>,
}

/// CJK connectors, written without spaces: `密码是…`, `パスワードは…`.
const CJK_CONNECTORS: &[&str] = &["是", "为", "為", "は", ":", "="];

/// The value a prose key points at, if any. Looks at most 8 words / 256 bytes ahead: the
/// connector may follow a short description (`le mot de passe de la base de donnees est …`).
pub(crate) fn prose_value(text: &str, key: &Key) -> Option<ProseHit> {
    let b = text.as_bytes();
    if key.cjk {
        let mut p = skip_ws(text, key.end);
        let rest = &text[p..];
        let conn = CJK_CONNECTORS.iter().find(|c| rest.starts_with(*c))?;
        p = skip_ws(text, p + conn.len());
        let mut e = p;
        while let Some(c) = text[e..].chars().next() {
            if c.is_whitespace() || is_cjk(c) || matches!(c, '、' | '。' | '，' | ',' | '"') {
                break;
            }
            e += c.len_utf8();
            if e - p > 256 {
                return None;
            }
        }
        let value = clean(text, p, e)?;
        return Some(ProseHit {
            value,
            raw: p..e,
            connector: true,
            conn: conn.to_string(),
            between: Vec::new(),
        });
    }
    // `"v" is the password.`: the key ends the sentence.
    if matches!(
        b.get(key.end),
        None | Some(b'\n' | b'\r' | b'.' | b'!' | b',' | b';')
    ) {
        return value_before(text, key);
    }
    if !matches!(b.get(key.end), Some(b' ' | b'\t')) {
        return None;
    }
    let mut le = line_end(text, key.end).min(key.end + 256);
    while !text.is_char_boundary(le) {
        le -= 1;
    }
    let mut toks: Vec<Range<usize>> = Vec::new();
    let mut i = key.end;
    while i < le && toks.len() < 8 {
        while i < le && matches!(b[i], b' ' | b'\t') {
            i += 1;
        }
        let st = i;
        while i < le && !matches!(b[i], b' ' | b'\t') {
            i += 1;
        }
        if i > st {
            toks.push(st..i);
        }
    }
    if toks.is_empty() || text[toks[0].clone()].starts_with(['.', ',', '!', ';', ')']) {
        return value_before(text, key);
    }
    // The look-ahead window only finds the connector: a value token it cuts runs on to its
    // blank, however long.
    if let Some(last) = toks.last_mut() {
        if last.end == le {
            last.end = le
                + b[le..]
                    .iter()
                    .position(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r'))
                    .unwrap_or(b.len() - le);
        }
    }
    let lower = |r: &Range<usize>| text[r.clone()].to_lowercase();
    // Words between the key and the connector that only carry tense or a change verb
    // (`has been reset to`): not a description of the key, so never a negative word.
    let between = |toks: &[Range<usize>]| -> Vec<Range<usize>> {
        toks.iter()
            .filter(|r| {
                let w = lower(r);
                let w = w.trim_end_matches([',', '.', ':']);
                !(AUXILIARIES.contains(&w) || CHANGE_VERBS.contains(&w))
            })
            .cloned()
            .collect()
    };
    let line = &text[line_start(text, key.start)..le];
    for j in 0..toks.len().min(7) {
        // `Das Passwort fuer das Backup: v`, `Password (prod): v`, `پسورد سرور: v`: a colon
        // ends the description.
        let colon = text[toks[j].clone()].ends_with(':') && toks[j].len() > 1;
        // Connectors are short; skip lowercasing longer tokens.
        if toks[j].len() > 16 && !colon {
            continue;
        }
        let w = lower(&toks[j]);
        let w = w.trim_end_matches([',', '.']);
        // `is:`, `to:`: the connector with a colon after it.
        let bare = w.strip_suffix(':').filter(|b| !b.is_empty()).unwrap_or(w);
        let prev = j.checked_sub(1).map(|i| lower(&toks[i]));
        let prev = prev.as_deref();
        // `set/reset/changed … to v`, `will/should be v`, `auf v … geändert`, `را به v`.
        let conditional = match bare {
            "to" => CHANGE_VERBS.iter().any(|v| contains_word(line, v)),
            "be" => prev.is_some_and(|p| MODALS.contains(&p)),
            "auf" => GERMAN_CHANGE
                .iter()
                .any(|v| contains_word(&line.to_lowercase(), v)),
            "به" => prev == Some("را"),
            _ => false,
        };
        // `was changed: yesterday` reports a change, it does not give the value.
        let verb = AUXILIARIES.contains(&bare) || CHANGE_VERBS.contains(&bare);
        let w = if CONNECTORS.contains(&bare) || conditional {
            bare
        } else if colon && !verb {
            ":"
        } else {
            w
        };
        // Persian puts the verb last: `رمز عبور X است`, so a trailing verb after a word
        // points back at it.
        if CONNECTORS.contains(&w) && !(j >= 1 && TRAILING_CONNECTORS.contains(&w)) || conditional {
            let v = toks.get(j + 1)?;
            // `was reset to v`, `is now v`: the connector starts a longer verb.
            let next = lower(v);
            let next = next.trim_end_matches([',', '.', ':']);
            if AUXILIARIES.contains(&next) || CHANGE_VERBS.contains(&next) {
                continue;
            }
            return Some(ProseHit {
                value: clean(text, v.start, v.end)?,
                raw: v.clone(),
                connector: true,
                conn: w.to_string(),
                between: between(&toks[..j]),
            });
        }
        if j >= 1 && TRAILING_CONNECTORS.contains(&w) {
            let v = &toks[j - 1];
            // `گذرواژه X برای مدیر سیستم است`: a Persian word before the verb ends the
            // sentence, the value is the word right after the key.
            if text[v.clone()]
                .chars()
                .all(|c| c.is_alphabetic() && !c.is_ascii())
            {
                break;
            }
            return Some(ProseHit {
                value: clean(text, v.start, v.end)?,
                raw: v.clone(),
                connector: true,
                conn: w.to_string(),
                between: toks[..j - 1].to_vec(),
            });
        }
    }
    let v = &toks[0];
    Some(ProseHit {
        value: clean(text, v.start, v.end)?,
        raw: v.clone(),
        connector: false,
        conn: String::new(),
        between: Vec::new(),
    })
}

/// Tense and modal words between a key and its connector (`has been`, `will`).
const AUXILIARIES: &[&str] = &[
    "has",
    "have",
    "had",
    "been",
    "was",
    "were",
    "is",
    "will",
    "be",
    "being",
    "should",
    "must",
    "now",
    "just",
    "also",
    "then",
    "temporarily",
];

/// Verbs that make `to` a connector (`set to`, `reset to`, `changed it to`).
const CHANGE_VERBS: &[&str] = &[
    "set", "sets", "reset", "resets", "change", "changes", "changed", "changing", "update",
    "updated", "rotate", "rotated", "switched",
];

/// Words that make `be` a connector (`will be v`, `should be v`).
const MODALS: &[&str] = &[
    "will", "should", "must", "shall", "would", "to", "can", "could",
];

/// German change verbs that make `auf` a connector (`das Passwort auf v geändert`).
const GERMAN_CHANGE: &[&str] = &[
    "geändert",
    "gesetzt",
    "zurückgesetzt",
    "setze",
    "setzen",
    "ändere",
    "ändern",
    "umgestellt",
];

/// Determiners between `is` and a key named after its value (`v is the password`).
const DETERMINERS: &[&str] = &["the", "my", "your", "our", "his", "her", "their", "its"];

/// `v` as a whole word in `hay` (lowercase).
fn contains_word(hay: &str, w: &str) -> bool {
    hay.match_indices(w).any(|(i, _)| {
        let before = hay[..i].chars().next_back();
        let after = hay[i + w.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// `"v" is the password`, `v is the new admin password`: the key ends its sentence and the
/// value stands before `is the`, at most two words before the key.
fn value_before(text: &str, key: &Key) -> Option<ProseHit> {
    let ls = line_start(text, key.start);
    let lo = floor_char(text, ls.max(key.start.saturating_sub(256)));
    let toks: Vec<Range<usize>> = {
        let mut v = Vec::new();
        let b = text.as_bytes();
        let mut i = lo;
        while i < key.start {
            while i < key.start && matches!(b[i], b' ' | b'\t') {
                i += 1;
            }
            let st = i;
            while i < key.start && !matches!(b[i], b' ' | b'\t') {
                i += 1;
            }
            if i > st {
                v.push(st..i);
            }
        }
        v
    };
    let n = toks.len();
    for gap in 0..=2 {
        // value, `is`/`was`, determiner, `gap` words, key
        let Some(vi) = n.checked_sub(3 + gap) else {
            break;
        };
        let conn = text[toks[vi + 1].clone()].to_lowercase();
        let det = text[toks[vi + 2].clone()].to_lowercase();
        if matches!(conn.as_str(), "is" | "was") && DETERMINERS.contains(&det.as_str()) {
            let v = &toks[vi];
            return Some(ProseHit {
                value: clean(text, v.start, v.end)?,
                raw: v.clone(),
                connector: true,
                conn: conn.clone(),
                between: toks[vi + 3..].to_vec(),
            });
        }
    }
    None
}

/// The char boundary at or before `i`.
fn floor_char(text: &str, mut i: usize) -> usize {
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn skip_ws(text: &str, mut p: usize) -> usize {
    while let Some(c) = text[p..].chars().next() {
        if c != ' ' && c != '\t' {
            break;
        }
        p += 1;
    }
    p
}

/// Strips quotes, brackets and trailing prose punctuation from a prose token.
fn clean(text: &str, s: usize, e: usize) -> Option<Range<usize>> {
    let v = &text[s..e];
    // A `[` is the value's, as a closing `]` is (`[Orchid]Lantern42`).
    let t = v.trim_start_matches(['"', '\'', '`', '(', '“', '«', '„']);
    let s2 = s + (v.len() - t.len());
    let mut t = t.trim_end_matches(['"', '\'', '`', '”', '»', '“']);
    loop {
        let before = t.len();
        t = t.trim_end_matches(['.', ',', ';', ':', '?', '"', '\'', '`', '”', '»']);
        // An unbalanced `)` is punctuation, a balanced one is part of the value. A `]` is
        // the value's: prose does not close brackets that way.
        if t.ends_with(')') && t.matches('(').count() < t.matches(')').count() {
            t = &t[..t.len() - 1];
        }
        if t.len() == before {
            break;
        }
    }
    (!t.is_empty()).then(|| s2..s2 + t.len())
}
