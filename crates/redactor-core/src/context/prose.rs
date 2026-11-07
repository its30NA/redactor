//! Keys named in prose: "the wifi pw is Kx9!mPq2vL", "Mein Passwort ist …",
//! "contraseña es …", "密码是…".

use std::ops::Range;

use super::lexicon::{CONNECTORS, TRAILING_CONNECTORS};
use super::syntax::{is_cjk, line_end, Key};

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
    if toks.is_empty() {
        return None;
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
    for j in 0..toks.len().min(7) {
        // `Das Passwort fuer das Backup: v`: a colon ends the description.
        let colon = j > 0 && text[toks[j].clone()].ends_with(':') && toks[j].len() > 1;
        // Connectors are short; skip lowercasing longer tokens.
        if toks[j].len() > 16 && !colon {
            continue;
        }
        let w = lower(&toks[j]);
        let w = w.trim_end_matches([',', '.']);
        let w = if colon { ":" } else { w };
        // Persian puts the verb last: `رمز عبور X است`, so a trailing verb after a word
        // points back at it.
        if CONNECTORS.contains(&w) && !(j >= 1 && TRAILING_CONNECTORS.contains(&w)) {
            let v = toks.get(j + 1)?;
            return Some(ProseHit {
                value: clean(text, v.start, v.end)?,
                raw: v.clone(),
                connector: true,
                conn: w.to_string(),
                between: toks[..j].to_vec(),
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
