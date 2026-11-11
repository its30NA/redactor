//! Finding keys in text and the value that belongs to each.
//!
//! One Aho-Corasick pass (ASCII case-insensitive) finds every lexicon word and every command
//! name. Around a word hit the key token is grown to its full extent (`DB_PASSWORD`,
//! `X-Api-Key`, `spring.datasource.password`), split into words and classified. Only then is
//! the text after it parsed. Every step looks at a bounded window, so the work stays linear.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::OnceLock;

use aho_corasick::{AhoCorasick, MatchKind};

use super::keyname::{classify, split_key, KeyClass};
use super::lexicon::{
    anchor_words, is_negative, Strength, CONNECTORS, OBJECT_ROOTS, PHRASE_ANCHORS,
};
use super::value::value_score;
use crate::engine::is_builtin_placeholder;

/// How a key and its value are written.
#[cfg_attr(not(test), allow(dead_code))] // Public interface of the context layer; the engine uses the detector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    /// `KEY=v`, `KEY: v`, `KEY := v`, `KEY => v` inside a line (query strings, arguments,
    /// connection strings).
    Assign,
    /// The same at the start of a line (`.env`, INI, YAML, properties, HTTP headers,
    /// `export KEY=v`, `set KEY=v`, `$env:KEY = "v"`).
    LineAssign,
    /// `"KEY": "v"`, `'KEY' => 'v'`, `ENV["KEY"] = "v"`.
    QuotedKey,
    /// `<KEY>v</KEY>`.
    Xml,
    /// `<add key="KEY" value="v"/>`.
    XmlAttr,
    /// `--KEY v`, `--KEY=v`, `-KEY v`, `/KEY:v`.
    Flag,
    /// `KEY v` (netrc, redis `requirepass v`, Dockerfile `ENV KEY v`, SQL `PASSWORD 'v'`).
    Spaced,
    /// A command's password argument (`curl -u user:v`, `mysql -pv`, `sshpass -p v`, …).
    Command,
}

/// A key and its raw value, before the value is judged.
#[cfg_attr(not(test), allow(dead_code))] // Public interface of the context layer; the engine uses the detector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    pub key: Range<usize>,
    pub value: Range<usize>,
    pub syntax: Syntax,
}

/// Every (key, value) pair the syntax layer finds in `text`, in order. Keys are lexicon keys
/// (and command names); values are not judged yet (code expressions, types and filler are
/// still in).
#[cfg_attr(not(test), allow(dead_code))] // Public interface of the context layer; the engine uses the detector.
pub fn extract_pairs(text: &str) -> Vec<Pair> {
    let mut out = Vec::new();
    for_each_event(text, |ev| match ev {
        Event::Key(key) => {
            let p = pair(text, &key)?;
            if let Some(v) = p.value {
                out.push(Pair {
                    key: key.start..key.end,
                    value: v,
                    syntax: p.syntax,
                });
            }
            Some(p.scanned)
        }
        Event::Cmd { start, end, cmd } => {
            let v = command(text, end, cmd)?;
            let e = v.end;
            out.push(Pair {
                key: start..end,
                value: v,
                syntax: Syntax::Command,
            });
            Some(e)
        }
    });
    out
}

// ---------------------------------------------------------------------------------------
// Anchors
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cmd {
    Curl,
    Mysql,
    Sshpass,
    Ldap,
    DockerLogin,
    Zip,
    SevenZip,
    Sqlplus,
    SecureString,
    IdentifiedBy,
    NetUse,
    Login,
    Htpasswd,
}

const COMMANDS: &[(&str, Cmd)] = &[
    ("curl", Cmd::Curl),
    ("mysql", Cmd::Mysql),
    ("mysqldump", Cmd::Mysql),
    ("mysqladmin", Cmd::Mysql),
    ("mariadb", Cmd::Mysql),
    ("sshpass", Cmd::Sshpass),
    ("ldapsearch", Cmd::Ldap),
    ("ldapmodify", Cmd::Ldap),
    ("ldapadd", Cmd::Ldap),
    ("ldapdelete", Cmd::Ldap),
    ("ldapwhoami", Cmd::Ldap),
    ("ldappasswd", Cmd::Ldap),
    ("docker", Cmd::DockerLogin),
    ("podman", Cmd::DockerLogin),
    ("nerdctl", Cmd::DockerLogin),
    ("helm", Cmd::DockerLogin),
    ("zip", Cmd::Zip),
    ("unzip", Cmd::Zip),
    ("7z", Cmd::SevenZip),
    ("7za", Cmd::SevenZip),
    ("sqlplus", Cmd::Sqlplus),
    ("convertto-securestring", Cmd::SecureString),
    ("identified by", Cmd::IdentifiedBy),
    ("net use", Cmd::NetUse),
    ("login", Cmd::Login),
    ("htpasswd", Cmd::Htpasswd),
];

enum AnchorKind {
    Word,
    Cmd(Cmd),
}

struct Anchors {
    ac: AhoCorasick,
    kinds: Vec<AnchorKind>,
}

fn anchors() -> &'static Anchors {
    static A: OnceLock<Anchors> = OnceLock::new();
    A.get_or_init(|| {
        let mut pats: Vec<String> = Vec::new();
        let mut kinds = Vec::new();
        for w in anchor_words() {
            // The automaton folds ASCII case only; add the capitalized and upper-case forms
            // of other scripts (`Пароль`, `ПАРОЛЬ`, `Hasło`).
            let mut forms = vec![w.to_string()];
            if !w.is_ascii() {
                let mut cs = w.chars();
                if let Some(f) = cs.next() {
                    forms.push(f.to_uppercase().chain(cs).collect());
                }
                forms.push(w.to_uppercase());
            }
            forms.sort();
            forms.dedup();
            for f in forms {
                pats.push(f);
                kinds.push(AnchorKind::Word);
            }
        }
        for (c, cmd) in COMMANDS {
            pats.push(c.to_string());
            kinds.push(AnchorKind::Cmd(*cmd));
        }
        let ac = AhoCorasick::builder()
            .match_kind(MatchKind::Standard)
            .ascii_case_insensitive(true)
            .build(&pats)
            .expect("context anchors build");
        Anchors { ac, kinds }
    })
}

/// A classified key in the text.
#[derive(Clone, Debug)]
pub(crate) struct Key {
    /// The key name (without leading dashes or a closing quote).
    #[cfg_attr(not(test), allow(dead_code))]
    pub start: usize,
    pub end: usize,
    /// Start including leading dashes.
    pub token_start: usize,
    pub dashes: usize,
    pub class: KeyClass,
    pub words: Vec<String>,
    /// A CJK key: no spaces around it, no expansion.
    pub cjk: bool,
    /// `DB_PASSWORD`: ASCII upper case, digits and `_` only (environment style).
    pub upper: bool,
    /// `self.password`, `config.token`: a member of a code object.
    pub member: bool,
}

pub(crate) enum Event {
    Key(Key),
    Cmd {
        #[cfg_attr(not(test), allow(dead_code))]
        start: usize,
        end: usize,
        cmd: Cmd,
    },
}

/// Longest key token we consider; longer runs of key characters are blobs, not names
/// (`spring.security.oauth2.client.registration.azure-active-directory.client-secret` is
/// 79 bytes).
const MAX_KEY: usize = 128;

/// Calls `f` for every key and command in text order. `f` returns how far the text is
/// consumed (the end of a value it took); anchors before that are skipped without building
/// a key, which keeps dense inputs (`password=password=…`) linear.
pub(crate) fn for_each_event(text: &str, mut f: impl FnMut(Event) -> Option<usize>) {
    let a = anchors();
    let mut hits: Vec<(usize, usize, usize)> =
        a.ac.find_overlapping_iter(text)
            .map(|m| (m.start(), m.end(), m.pattern().as_usize()))
            .collect();
    hits.sort_unstable();
    let mut last_token = usize::MAX;
    let mut blob_until = 0usize;
    let mut consumed = 0usize;
    // Key tokens repeat (long .env files, logs, adversarial input): classify each once.
    let mut cache: TokenCache = HashMap::new();
    for (s, e, pid) in hits {
        if s < blob_until || s < consumed {
            continue;
        }
        let ev = match a.kinds[pid] {
            AnchorKind::Cmd(cmd) => {
                let before = char_before(text, s);
                let after = text[e..].chars().next();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-')
                    || !after.is_some_and(|c| c == ' ' || c == '\t')
                {
                    continue;
                }
                Event::Cmd {
                    start: s,
                    end: e,
                    cmd,
                }
            }
            AnchorKind::Word => {
                let Some(key) = build_key(text, s, e, &mut last_token, &mut blob_until, &mut cache)
                else {
                    continue;
                };
                Event::Key(key)
            }
        };
        if let Some(c) = f(ev) {
            consumed = consumed.max(c);
        }
    }
}

/// Token -> its words and class, or `None` for a random blob.
type TokenCache<'t> = HashMap<&'t str, Option<(Option<KeyClass>, Vec<String>)>>;

fn build_key<'t>(
    text: &'t str,
    s: usize,
    e: usize,
    last_token: &mut usize,
    blob_until: &mut usize,
    cache: &mut TokenCache<'t>,
) -> Option<Key> {
    let first = text[s..].chars().next()?;
    if is_cjk(first) {
        if *last_token == s {
            return None;
        }
        *last_token = s;
        let words = vec![text[s..e].to_lowercase()];
        let class = classify(&words)?;
        return Some(Key {
            start: s,
            end: e,
            token_start: s,
            dashes: 0,
            class,
            words,
            cjk: true,
            upper: false,
            member: false,
        });
    }
    // Grow to the whole key token.
    let mut ks = s;
    while let Some(c) = char_before(text, ks) {
        if !is_key_char(c) {
            break;
        }
        ks -= c.len_utf8();
        if s - ks > MAX_KEY {
            // Skip the rest of this blob, and only this blob: the next line may hold a key.
            let rest = text[e..].find(|c: char| !is_key_char(c));
            *blob_until = rest.map_or(text.len(), |j| e + j);
            return None;
        }
    }
    let mut ke = e;
    while let Some(c) = text[ke..].chars().next() {
        if !is_key_char(c) {
            break;
        }
        ke += c.len_utf8();
        if ke - e > MAX_KEY {
            *blob_until = ke;
            return None;
        }
    }
    let token_start = ks;
    let dashes = text[ks..ke].bytes().take_while(|&b| b == b'-').count();
    ks += dashes;
    while ks < ke && text.as_bytes()[ks] == b'.' {
        ks += 1;
    }
    while ke > ks && matches!(text.as_bytes()[ke - 1], b'.' | b'-') {
        ke -= 1;
    }
    if ke <= ks || ke < e || ks > s {
        return None;
    }
    if *last_token == token_start {
        return None;
    }
    *last_token = token_start;
    let token = &text[ks..ke];
    if cache.len() > 4096 {
        cache.clear();
    }
    let entry = cache.entry(token).or_insert_with(|| {
        let w = split_key(token);
        let c = classify(&w);
        // A key name is a name: a random blob that happens to contain `Pass` is not. A
        // token whose words classify as a key (`OAUTH2_PROXY_CLIENT_SECRET`, `PG15_PASSWORD`,
        // `oauth2ProxyClientSecret`) is a name however random its letters and digits look.
        if c.is_none() && token.len() >= 12 && value_score(token) >= 0.6 {
            return None;
        }
        Some((c, w))
    });
    let (mut class, mut words) = match entry {
        Some((c, w)) => (*c, w.clone()),
        None => return None,
    };
    // Only a lexicon key, or a phrase part (`passe`, `url`), can become a key.
    if class.is_none() && !(words.len() == 1 && PHRASE_ANCHORS.contains(&words[0].as_str())) {
        return None;
    }
    // Phrases written with spaces (`API key`, `mot de passe`, `Key Content`) and adjacent
    // negative words (`public key`, `password reset`).
    let mut start = ks;
    let mut end = ke;
    if words.len() == 1 && dashes == 0 {
        let p1 = word_before(text, ks);
        let p2 = p1.and_then(|(a, _)| word_before(text, a));
        let n1 = word_after(text, ke);
        let n2 = n1.and_then(|(_, b)| word_after(text, b));
        for nb in [p1, n1].into_iter().flatten() {
            let w = text[nb.0..nb.1].to_lowercase();
            // `password has been reset to v`: after the key, `has` starts a sentence's verb;
            // before it (`has password`) it names a flag.
            let verb = nb.0 > ke && matches!(w.as_str(), "has" | "have" | "had");
            if is_negative(&w) && !CONNECTORS.contains(&w.as_str()) && !verb {
                // Unless the neighbour completes a phrase (`database url`).
                let mut both = words.clone();
                if nb.0 < ks {
                    both.insert(0, w);
                } else {
                    both.push(w);
                }
                if !classify(&both).is_some_and(|k| k.all_consumed) {
                    return None;
                }
            }
        }
        let mut best: Option<(usize, KeyClass, Vec<String>, usize, usize)> = None;
        for (l, r) in [(2, 0), (1, 0), (0, 1), (0, 2), (1, 1)] {
            let left: Vec<(usize, usize)> = match l {
                2 => match (p2, p1) {
                    (Some(a), Some(b)) => vec![a, b],
                    _ => continue,
                },
                1 => match p1 {
                    Some(a) => vec![a],
                    None => continue,
                },
                _ => vec![],
            };
            let right: Vec<(usize, usize)> = match r {
                2 => match (n1, n2) {
                    (Some(a), Some(b)) => vec![a, b],
                    _ => continue,
                },
                1 => match n1 {
                    Some(a) => vec![a],
                    None => continue,
                },
                _ => vec![],
            };
            let mut ws: Vec<String> = left
                .iter()
                .map(|&(a, b)| text[a..b].to_lowercase())
                .collect();
            ws.extend(words.iter().cloned());
            ws.extend(right.iter().map(|&(a, b)| text[a..b].to_lowercase()));
            if let Some(k) = classify(&ws) {
                if k.all_consumed && best.as_ref().is_none_or(|b| ws.len() > b.0) {
                    let st = left.first().map_or(ks, |x| x.0);
                    let en = right.last().map_or(ke, |x| x.1);
                    best = Some((ws.len(), k, ws, st, en));
                }
            }
        }
        if let Some((_, k, ws, st, en)) = best {
            class = Some(k);
            words = ws;
            start = st;
            end = en;
        }
    }
    let class = class?;
    let upper = token
        .bytes()
        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && token.bytes().any(|b| b.is_ascii_uppercase());
    let member = token.contains('.')
        && token
            .split('.')
            .next()
            .is_some_and(|r| OBJECT_ROOTS.contains(&r.to_ascii_lowercase().as_str()));
    Some(Key {
        start,
        end,
        token_start: token_start.min(start),
        dashes,
        class,
        words,
        cjk: false,
        upper,
        member,
    })
}

/// One alphabetic word separated from `pos` by a single space (same line).
fn word_before(text: &str, pos: usize) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    if pos == 0 || b[pos - 1] != b' ' {
        return None;
    }
    let end = pos - 1;
    let mut st = end;
    while let Some(c) = char_before(text, st) {
        if !c.is_alphabetic() || is_cjk(c) {
            if is_key_char(c) {
                return None;
            }
            break;
        }
        st -= c.len_utf8();
        if end - st > 24 {
            return None;
        }
    }
    (st < end).then_some((st, end))
}

fn word_after(text: &str, pos: usize) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    if pos >= b.len() || b[pos] != b' ' {
        return None;
    }
    let st = pos + 1;
    let mut end = st;
    while let Some(c) = text[end..].chars().next() {
        if !c.is_alphabetic() || is_cjk(c) {
            if is_key_char(c) {
                return None;
            }
            break;
        }
        end += c.len_utf8();
        if end - st > 24 {
            return None;
        }
    }
    (end > st).then_some((st, end))
}

pub(crate) fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF
        | 0x1100..=0x11FF | 0x3130..=0x318F | 0xF900..=0xFAFF)
}

fn is_key_char(c: char) -> bool {
    (c.is_alphanumeric() && !is_cjk(c)) || c == '_' || c == '-' || c == '.'
}

pub(crate) fn char_before(text: &str, i: usize) -> Option<char> {
    text[..i].chars().next_back()
}

/// Start of the line holding `i` (looks back at most 512 bytes: a key further into a line
/// is not at its start, and the prefix heuristics only need its tail).
pub(crate) fn line_start(text: &str, i: usize) -> usize {
    let i = floor(text, i);
    let lo = floor(text, i.saturating_sub(512));
    text[lo..i].rfind('\n').map_or(lo, |p| lo + p + 1)
}

/// End of the line holding `i` (looks ahead at most 2 KiB), before `\r\n` / `\n`.
pub(crate) fn line_end(text: &str, i: usize) -> usize {
    let i = ceil(text, i);
    line_end_within(text, i, ceil(text, i.saturating_add(2048)))
}

/// End of the line holding `i`, however far away.
pub(crate) fn line_end_full(text: &str, i: usize) -> usize {
    let i = ceil(text, i);
    line_end_within(text, i, text.len())
}

fn line_end_within(text: &str, i: usize, hi: usize) -> usize {
    let mut e = text[i..hi].find('\n').map_or(hi, |p| i + p);
    if e > i && text.as_bytes()[e - 1] == b'\r' {
        e -= 1;
    }
    e
}

/// The nearest char boundary at or before `i` (clamped to the text).
pub(crate) fn floor(text: &str, i: usize) -> usize {
    let mut i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// The nearest char boundary at or after `i` (clamped to the text).
pub(crate) fn ceil(text: &str, i: usize) -> usize {
    let mut i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

// ---------------------------------------------------------------------------------------
// Pairs
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sep {
    Colon,
    Equals,
    /// `:=`, `=>`, `→`.
    Other,
    None,
}

/// What [`pair`] found after a key.
#[derive(Clone, Debug)]
pub(crate) struct RawPair {
    /// `None`: the syntax is a pair, but there is no usable value (empty, a block, too long).
    pub value: Option<Range<usize>>,
    pub syntax: Syntax,
    pub quoted: bool,
    pub sep: Sep,
    /// No whitespace around the separator (`KEY=v`).
    pub tight: bool,
    /// The key starts its line (after indentation and `export`/`set`/`ENV`/`- `/`$env:`).
    pub line_start: bool,
    /// The line is a code statement (declaration keyword, call argument, member, `;` end).
    pub code: bool,
    /// The key follows a `(`, `,` or `{` and no code keyword (`User(id=42, password=v)`,
    /// `Username: admin, Password: v`): an argument list or a list in prose. Only an
    /// identifier-shaped value is code there.
    pub args: bool,
    /// The statement ends right after the value (`;`, or `,` in an object literal): code,
    /// unless the value is a literal (`Password=x;` in a connection string is fine).
    pub stmt_end: bool,
    /// The syntax makes the value a literal, not code: a config line (`.env`, INI, YAML,
    /// properties), a command-line option or a logfmt field. It runs as far as that syntax
    /// lets it, whatever characters it holds.
    pub literal: bool,
    /// How far the parser looked; anchors before this belong to this pair.
    pub scanned: usize,
}

/// Longest `<KEY>v</KEY>` element value.
const MAX_XML: usize = 1024;

/// Parses the value after `key`, or `None` when the key is not followed by pair syntax (the
/// caller then tries prose).
pub(crate) fn pair(text: &str, key: &Key) -> Option<RawPair> {
    let b = text.as_bytes();
    let ts = key.token_start;
    let before = char_before(text, ts);
    let ls = line_start(text, ts);
    let prefix = &text[ls..ts];
    let line_start = is_line_start_prefix(prefix);
    let mut code = is_code_prefix(prefix) || key.member;
    let mut args = !code && is_args_prefix(prefix);
    // The label of an earlier placeholder (`[REDACTED: Password #1]`, a built-in label) is not
    // a key. A made-up label (`[REDACTED: password=…]`) is read as text.
    const OPEN: &str = "[REDACTED: ";
    if prefix.ends_with(OPEN) {
        let open = ts - OPEN.len();
        let close = text[ts..].find([']', '\n']).filter(|&i| b[ts + i] == b']');
        if close.is_some_and(|i| is_builtin_placeholder(&text[open..=ts + i])) {
            return Some(RawPair {
                value: None,
                syntax: Syntax::Assign,
                quoted: false,
                sep: Sep::None,
                tight: false,
                line_start,
                code,
                args: false,
                stmt_end: false,
                literal: false,
                scanned: key.end,
            });
        }
    }
    // A key inside a string literal (`"aws_secret_access_key = "`): the next quote of that
    // kind closes the string, it does not open a value.
    let closing = open_quote(prefix);

    // <KEY>v</KEY>
    // Strong keys only: `<code>…</code>` and `<key>…</key>` are HTML and plist markup.
    if before == Some('<')
        && b.get(key.end) == Some(&b'>')
        && key.class.strength == Strength::Strong
    {
        let vs = key.end + 1;
        let hi = ceil(text, vs + MAX_XML);
        let close = text[vs..hi].find('<').map(|p| vs + p);
        let value = close.and_then(|c| {
            if !text[c..].starts_with("</") {
                // `<AdministratorPassword>⏎  <Value>v</Value>`: the value in a child.
                return text[vs..c]
                    .trim()
                    .is_empty()
                    .then(|| value_element(text, c))
                    .flatten();
            }
            let raw = &text[vs..c];
            let s = vs + (raw.len() - raw.trim_start().len());
            let e = vs + raw.trim_end().len();
            (e > s).then_some(s..e)
        });
        return Some(RawPair {
            scanned: value.as_ref().map_or(vs, |v| v.end),
            value,
            syntax: Syntax::Xml,
            quoted: true,
            sep: Sep::None,
            tight: true,
            line_start,
            code: false,
            args: false,
            stmt_end: false,
            literal: false,
        });
    }

    // "KEY": v  /  'KEY' => v  /  ENV["KEY"] = v  /  <add key="KEY" value="v"/>
    if let Some(q @ ('"' | '\'' | '`')) = before {
        if key.dashes == 0 && text[key.end..].starts_with(q) {
            let open = ts - 1;
            let pre = text[ls..open].trim_end().to_ascii_lowercase();
            if pre.ends_with("key=") || pre.ends_with("name=") {
                // XML attribute pair: look for value="…" in the same tag.
                let tag_end = text[key.end..ceil(text, key.end + 256)]
                    .find('>')
                    .map_or(key.end, |p| key.end + p);
                let seg = &text[key.end..tag_end];
                let Some(at) = seg.to_ascii_lowercase().find("value=") else {
                    // `<setting name="SmtpPassword">⏎  <value>v</value>`: a child element.
                    if b.get(tag_end) != Some(&b'>') {
                        return None;
                    }
                    let hi = ceil(text, tag_end + 1 + MAX_CELL_GAP);
                    let gap = &text[tag_end + 1..hi];
                    let c = tag_end + 1 + (gap.len() - gap.trim_start().len());
                    let value = value_element(text, c)?;
                    return Some(RawPair {
                        scanned: value.end,
                        value: Some(value),
                        syntax: Syntax::Xml,
                        quoted: true,
                        sep: Sep::None,
                        tight: true,
                        line_start,
                        code: false,
                        args: false,
                        stmt_end: false,
                        literal: false,
                    });
                };
                let vq = key.end + at + "value=".len();
                let (value, scanned, _) = quoted_value(text, vq)?;
                return Some(RawPair {
                    value,
                    syntax: Syntax::XmlAttr,
                    quoted: true,
                    sep: Sep::Equals,
                    tight: true,
                    line_start,
                    code: false,
                    args: false,
                    stmt_end: false,
                    literal: false,
                    scanned,
                });
            }
            // `{"name": "DB_PASSWORD", "value": "v"}`, `- name: "DB_PASSWORD"⏎  value: v`.
            if let Some(rp) = sibling_pair(text, key, &text[ls..open], key.end + 1) {
                return Some(rp);
            }
            code |= is_code_prefix(&text[ls..open]);
            args = !code && (args || is_args_prefix(&text[ls..open]));
            let mut p = key.end + 1;
            if b.get(p) == Some(&b']') {
                p += 1;
            }
            let (sep, tight, after) = separator(text, p)?;
            // `"db_password": { "value": "v", "type": "string" }` (Terraform outputs, Spring
            // actuator): the value of the nested `value` field.
            if sep == Sep::Colon && b.get(after) == Some(&b'{') {
                if let Some(v) = json_sibling(text, after + 1) {
                    let cx = Ctx::new(Mode::Token, sep, false, false, None, key.upper);
                    let mut rp = value_after(text, v, cx)?;
                    rp.syntax = Syntax::QuotedKey;
                    rp.sep = sep;
                    return Some(rp);
                }
            }
            let cx = Ctx::new(Mode::Token, sep, line_start, code, None, key.upper);
            let mut rp = value_after(text, after, cx)?;
            rp.syntax = Syntax::QuotedKey;
            rp.sep = sep;
            rp.tight = tight;
            rp.args = args;
            // `{"key": "X-Api-Key", "value": "v"}` (Postman): the value of a `key` field may
            // be a key name itself, so the scan goes on inside it.
            if key.words.len() == 1 && key.words[0] == "key" && rp.quoted {
                if let Some(v) = &rp.value {
                    rp.scanned = v.start;
                }
            }
            return Some(rp);
        }
    }

    // --KEY v, --KEY=v, -KEY v, /KEY:v
    let flag_lead = |c: Option<char>| {
        c.is_none_or(|c| c.is_whitespace() || matches!(c, '(' | '[' | '"' | '\'' | '`'))
    };
    let slash = before == Some('/') && flag_lead(char_before(text, ts - 1));
    if (key.dashes > 0 && flag_lead(before)) || slash {
        let p = key.end;
        let c = b.get(p).copied();
        let after = match c {
            Some(b'=') => p + 1,
            Some(b':') if slash => p + 1,
            Some(b' ' | b'\t') => {
                let q = skip_blank(text, p);
                if matches!(b.get(q), Some(b'-') | None | Some(b'\n' | b'\r')) {
                    return None;
                }
                q
            }
            _ => return None,
        };
        // An option's value is one shell word, whatever characters it holds.
        let sep = if c == Some(b'=') {
            Sep::Equals
        } else {
            Sep::None
        };
        let cx = Ctx::new(Mode::Word, sep, false, false, closing, key.upper);
        let mut rp = value_after(text, after, cx)?;
        rp.syntax = Syntax::Flag;
        rp.sep = sep;
        // `(--password v)`, `[--password=v]`: the bracket closes around the option.
        let opener = if slash {
            char_before(text, ts - 1)
        } else {
            before
        };
        if let (Some(open), Some(v)) = (opener, rp.value.clone()) {
            let close = match open {
                '(' => ')',
                '[' => ']',
                _ => '\0',
            };
            let s = &text[v.clone()];
            if close != '\0'
                && s.ends_with(close)
                && s.matches(open).count() < s.matches(close).count()
            {
                rp.value = Some(v.start..v.end - 1).filter(|r| r.end > r.start);
            }
        }
        // `-passin pass:secret` (OpenSSL): only the `pass:` form carries the password.
        if key.words.len() == 1 && matches!(key.words[0].as_str(), "passin" | "passout") {
            if let Some(v) = rp.value.clone() {
                rp.value = text[v.clone()]
                    .starts_with("pass:")
                    .then(|| v.start + 5..v.end)
                    .filter(|r| r.end > r.start);
            }
        }
        return Some(rp);
    }

    // The attribute *name* `key` in `<add key="DbPassword" …/>` is not a key.
    if key.words.len() == 1
        && matches!(key.words[0].as_str(), "key" | "name")
        && text[key.end..].starts_with("=\"")
        && prefix.contains('<')
    {
        return None;
    }

    // `- name: DB_PASSWORD⏎  value: v`
    if text[key.end..line_end(text, key.end)].trim().is_empty() {
        if let Some(rp) = sibling_pair(text, key, prefix, key.end) {
            return Some(rp);
        }
    }

    // <td>Password</td><td>v</td>, <dt>Password</dt><dd>v</dd>, | Password | v |
    if key.class.strength == Strength::Strong {
        if let Some(rp) = cell_pair(text, key, prefix, line_start) {
            return Some(rp);
        }
    }

    // KEY = v, KEY: v, KEY := v, KEY => v
    // `DB_PASSWORD==v`, `DB_PASSWORD=~v`, `DB_PASSWORD=>v` in a `.env` file or a logfmt
    // field: the value starts with `=`, `~` or `>`; there is no comparison or arrow there.
    let glued = ["==", "=~", "=>"]
        .iter()
        .any(|s| text[key.end..].starts_with(s))
        && !matches!(
            b.get(key.end + 2),
            None | Some(b' ' | b'\t' | b'\n' | b'\r')
        );
    let env_eq = glued
        && !code
        && key.dashes == 0
        && (line_start && key.upper || logfmt_field(text, prefix, key.end + 1));
    let sep_at = if env_eq {
        Some((Sep::Equals, true, key.end + 1))
    } else if inline_open(prefix) {
        // `**Password:** v`, `*Password*: v`, `<strong>Password:</strong> v`: emphasis and
        // inline tags around the key and separator are markup, not the value.
        separator(text, skip_closers(text, key.end))
            .map(|(sep, _, after)| (sep, false, skip_blank(text, skip_closers(text, after))))
    } else {
        separator(text, key.end)
    };
    if let Some((sep, tight, after)) = sep_at {
        // A line-start key with a strong name takes the rest of the line, as dotenv, YAML
        // plain scalars, INI and properties read it; elsewhere the value ends at a delimiter.
        // `KEY=v` without spaces may also be a shell prefix (`PGPASSWORD=v psql -h db`):
        // there the value ends at the first blank when a command follows.
        // A `name=v` field among others (`level=warn password=v user=app`) is one word.
        let mode = if line_start && key.class.strength == Strength::Strong {
            if sep == Sep::Equals && tight {
                Mode::Dotenv
            } else {
                Mode::Line
            }
        } else if sep == Sep::Equals
            && tight
            && !code
            && !args
            && !key.member
            && key.dashes == 0
            && logfmt_field(text, prefix, after)
        {
            Mode::Word
        } else {
            Mode::Token
        };
        let cx = Ctx::new(
            mode,
            sep,
            line_start,
            code || key.member,
            closing,
            key.upper,
        );
        let mut rp = value_after(text, after, cx)?;
        rp.syntax = if line_start {
            Syntax::LineAssign
        } else {
            Syntax::Assign
        };
        rp.sep = sep;
        rp.tight = tight;
        rp.args = args;
        // `auth.password=v` (helm `--set`, properties under an `app.` prefix) reads to the
        // first blank as code does, but only an identifier, member path or expression is
        // code there: `self.password = pw` is, `auth.password=Tulip-Harbor-91` is not.
        if key.member && !is_code_prefix(prefix) {
            rp.code = false;
            rp.args = true;
        }
        return Some(rp);
    }

    // KEY v
    if matches!(b.get(key.end), Some(b' ' | b'\t')) && !key.cjk {
        let after = skip_blank(text, key.end);
        let line = &text[ls..ts];
        // .netrc lines start with `machine` (or `default`).
        let netrc = ["machine ", "default "]
            .iter()
            .any(|w| starts_with_ci(line.trim_start(), w));
        let dockerfile = prefix.trim() == "ENV";
        let redis = line_start
            && key.words.len() == 1
            && matches!(key.words[0].as_str(), "requirepass" | "masterauth");
        // SQL `CREATE USER x WITH PASSWORD 'v'`: only `password`, only on a SQL-ish line
        // ("the token \"NUMBER\"" and "pass 'ignore'" are prose).
        let quoted_next = matches!(b.get(after), Some(b'"' | b'\''))
            && key
                .words
                .last()
                .is_some_and(|w| matches!(w.as_str(), "password" | "passwd"))
            && ["user", "role", "login", "with"]
                .iter()
                .any(|w| contains_ci(line, w));
        if netrc || redis || quoted_next {
            let cx = Ctx::new(
                Mode::Token,
                Sep::None,
                line_start,
                false,
                closing,
                key.upper,
            );
            let mut rp = value_after(text, after, cx)?;
            rp.syntax = Syntax::Spaced;
            return Some(rp);
        }
        if dockerfile {
            let cx = Ctx::new(Mode::Line, Sep::None, true, false, closing, key.upper);
            let mut rp = value_after(text, after, cx)?;
            rp.syntax = Syntax::Spaced;
            return Some(rp);
        }
    }
    None
}

/// `<value>v</value>` (any case) at `p`: the range of `v`.
fn value_element(text: &str, p: usize) -> Option<Range<usize>> {
    const OPEN: &str = "<value>";
    if !text[p..].get(..OPEN.len())?.eq_ignore_ascii_case(OPEN) {
        return None;
    }
    let vs = p + OPEN.len();
    let c = vs + text[vs..ceil(text, vs + MAX_XML)].find('<')?;
    if !text[c..].get(..8)?.eq_ignore_ascii_case("</value>") {
        return None;
    }
    let raw = &text[vs..c];
    let s = vs + (raw.len() - raw.trim_start().len());
    let e = vs + raw.trim_end().len();
    (e > s).then_some(s..e)
}

/// How far a sibling `value` field may sit from its `name` field.
const MAX_SIBLING: usize = 512;

/// The key is the value of a `name` or `key` field (`"name": "DB_PASSWORD"`, `- name:
/// DB_PASSWORD`, Kubernetes env, ECS, Azure app settings, HAR, Postman): the value is in the
/// sibling `value` field of the same object or list item. `pre` is the line up to the key
/// (or its opening quote); `from` is just past the key (and its closing quote).
fn sibling_pair(text: &str, key: &Key, pre: &str, from: usize) -> Option<RawPair> {
    let t = pre.trim_end();
    let field = t.strip_suffix(':')?.trim_end();
    let json = ["\"name\"", "\"key\"", "'name'", "'key'"].iter().any(|f| {
        let fb = field.as_bytes();
        fb.len() >= f.len() && fb[fb.len() - f.len()..].eq_ignore_ascii_case(f.as_bytes())
    });
    let after = if json {
        json_sibling(text, from)?
    } else {
        // YAML: `name:` alone (or after `- `) on its line; `value:` at the same column.
        let lead = t.trim_start();
        let item = lead.strip_prefix("- ").map_or(lead, str::trim_start);
        if !(item.eq_ignore_ascii_case("name:") || item.eq_ignore_ascii_case("key:")) {
            return None;
        }
        let col = t.len() - item.len();
        yaml_sibling(text, line_end_full(text, from), col)?
    };
    let mode = if !json && key.class.strength == Strength::Strong {
        Mode::Line
    } else {
        Mode::Token
    };
    let cx = Ctx::new(mode, Sep::Colon, !json, false, None, key.upper);
    let mut rp = value_after(text, after, cx)?;
    rp.syntax = if json {
        Syntax::QuotedKey
    } else {
        Syntax::LineAssign
    };
    rp.sep = Sep::Colon;
    Some(rp)
}

/// In a JSON object, from `from` on: the position after `"value":` of the same object, or
/// `None` at the object's end, at another `name`/`key` field or past [`MAX_SIBLING`].
fn json_sibling(text: &str, from: usize) -> Option<usize> {
    let b = text.as_bytes();
    let hi = ceil(text, from + MAX_SIBLING);
    let mut depth = 0i32;
    let mut i = from;
    while i < hi {
        match b[i] {
            b'"' => {
                let s = i + 1;
                let mut j = s;
                while j < hi && b[j] != b'"' {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                if j >= hi {
                    return None;
                }
                let k = skip_blank(text, j + 1);
                if depth == 0 && b.get(k) == Some(&b':') {
                    let name = &text[s..j];
                    if name.eq_ignore_ascii_case("value") {
                        return Some(skip_blank(text, k + 1));
                    }
                    if name.eq_ignore_ascii_case("name") || name.eq_ignore_ascii_case("key") {
                        return None;
                    }
                }
                i = j + 1;
                continue;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The YAML lines after a `name:` field (from `from`, a line start): the position after
/// `value:` at column `col`, or `None` at the next list item, a line left of `col`, another
/// `name`/`key` field or past [`MAX_SIBLING`].
fn yaml_sibling(text: &str, from: usize, col: usize) -> Option<usize> {
    let hi = ceil(text, from + MAX_SIBLING);
    let mut ls = from;
    for line in text[from..hi].split_inclusive('\n') {
        let start = ls;
        ls += line.len();
        let body = line.trim_end();
        let item = body.trim_start_matches(' ');
        if item.is_empty() {
            continue;
        }
        let indent = body.len() - item.len();
        if indent < col || indent == col && item.starts_with('-') {
            return None;
        }
        if indent > col {
            continue;
        }
        let lower = item.get(..6).unwrap_or("").to_ascii_lowercase();
        if lower == "value:" {
            return Some(skip_blank(text, start + indent + 6));
        }
        if lower.starts_with("name:") || lower.starts_with("key:") {
            return None;
        }
    }
    None
}

/// Inline tags that wrap a label (`<b>Password</b>: v`).
const INLINE_TAGS: &[&str] = &["b", "strong", "em", "i", "u", "span", "label"];

/// The key is opened by Markdown emphasis or an inline tag: `**`, `*`, `_`, `<b>`.
fn inline_open(prefix: &str) -> bool {
    if prefix.ends_with(['*', '_']) {
        return true;
    }
    let Some(open) = prefix.strip_suffix('>') else {
        return false;
    };
    let Some(lt) = open.rfind('<') else {
        return false;
    };
    let name = open[lt + 1..]
        .split(|c: char| c.is_whitespace())
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    INLINE_TAGS.contains(&name.as_str())
}

/// Skips closing emphasis (`**`, `*`, `__`) and closing inline tags (`</strong>`) at `p`.
fn skip_closers(text: &str, mut p: usize) -> usize {
    let b = text.as_bytes();
    loop {
        let run = b[p..]
            .iter()
            .take(4)
            .take_while(|&&c| matches!(c, b'*' | b'_'))
            .count();
        if (1..=3).contains(&run) {
            p += run;
            continue;
        }
        if text[p..].starts_with("</") {
            let close = text[p..ceil(text, p + 12)].find('>');
            let name = close.map(|c| text[p + 2..p + c].to_ascii_lowercase());
            if let (Some(c), Some(n)) = (close, name) {
                if INLINE_TAGS.contains(&n.as_str()) {
                    p += c + 1;
                    continue;
                }
            }
        }
        return p;
    }
}

/// Longest gap between a key cell and its value cell (`</td>⏎    <td>`).
const MAX_CELL_GAP: usize = 128;

/// A key alone in a table cell and the value in the next one: `<td>Password</td><td>v</td>`,
/// `<th>Password</th><td>v</td>`, `<dt>Password</dt><dd>v</dd>`, `| Password | v |`. The
/// value is one word (a cell of several words is a description in a docs table).
fn cell_pair(text: &str, key: &Key, prefix: &str, line_start: bool) -> Option<RawPair> {
    let b = text.as_bytes();
    let mut p = skip_blank(text, key.end);
    if b.get(p) == Some(&b':') {
        p = skip_blank(text, p + 1);
    }
    let (vs, ve) = if let Some(open) = prefix.strip_suffix('>') {
        let lt = open.rfind('<')?;
        let name = open[lt + 1..]
            .split(|c: char| c.is_whitespace())
            .next()?
            .to_ascii_lowercase();
        if !matches!(name.as_str(), "td" | "th" | "dt") {
            return None;
        }
        let close = format!("</{name}>");
        if !text[p..].get(..close.len())?.eq_ignore_ascii_case(&close) {
            return None;
        }
        p += close.len();
        let hi = ceil(text, p + MAX_CELL_GAP);
        p += text[p..hi].len() - text[p..hi].trim_start().len();
        let next = text[p..].get(..3)?.to_ascii_lowercase();
        if !matches!(next.as_str(), "<td" | "<dd") || !matches!(b.get(p + 3), Some(b'>' | b' ')) {
            return None;
        }
        let gt = text[p..ceil(text, p + MAX_CELL_GAP)].find('>')?;
        let vs = p + gt + 1;
        let lt = vs + text[vs..ceil(text, vs + MAX_XML)].find('<')?;
        (vs, lt)
    } else if prefix.trim_end().ends_with('|') && prefix.trim_start().starts_with('|') {
        if b.get(p) != Some(&b'|') {
            return None;
        }
        let vs = p + 1;
        let le = line_end(text, vs);
        // `\|` is a pipe inside the cell.
        let bar = text[vs..le]
            .bytes()
            .enumerate()
            .position(|(i, c)| c == b'|' && (i == 0 || b[vs + i - 1] != b'\\'))?;
        (vs, vs + bar)
    } else {
        return None;
    };
    let cell = &text[vs..ve];
    let t = cell.trim();
    if t.is_empty() || t.contains(char::is_whitespace) {
        return Some(RawPair {
            value: None,
            syntax: Syntax::Xml,
            quoted: false,
            sep: Sep::None,
            tight: false,
            line_start,
            code: false,
            args: false,
            stmt_end: false,
            literal: true,
            scanned: ve,
        });
    }
    let s = vs + (cell.len() - cell.trim_start().len());
    Some(RawPair {
        value: Some(s..s + t.len()),
        syntax: Syntax::Xml,
        quoted: false,
        sep: Sep::None,
        tight: false,
        line_start: false,
        code: false,
        args: false,
        stmt_end: false,
        literal: true,
        scanned: ve,
    })
}

fn starts_with_ci(hay: &str, needle: &str) -> bool {
    hay.len() >= needle.len()
        && hay.as_bytes()[..needle.len()].eq_ignore_ascii_case(needle.as_bytes())
}

/// ASCII case-insensitive substring test without allocating.
pub(crate) fn contains_ci(hay: &str, needle: &str) -> bool {
    let n = needle.as_bytes();
    hay.as_bytes()
        .windows(n.len())
        .any(|w| w.eq_ignore_ascii_case(n))
}

fn is_escape(c: Option<u8>) -> bool {
    matches!(
        c,
        Some(b'n' | b'r' | b't' | b'"' | b'\'' | b'\\' | b'0' | b'u' | b'x')
    )
}

/// The quote (`"` or `` ` ``) left open by the last 256 bytes of `prefix`, if any: the key
/// then sits inside a string literal or a Markdown code span. Single quotes are not counted
/// (apostrophes in prose would flip them).
fn open_quote(prefix: &str) -> Option<u8> {
    let b = prefix.as_bytes();
    let tail = &b[b.len().saturating_sub(256)..];
    for q in *b"\"`" {
        if !tail.contains(&q) {
            continue;
        }
        let mut n = 0;
        let mut i = 0;
        while i < tail.len() {
            if tail[i] == b'\\' {
                i += 2;
                continue;
            }
            if tail[i] == q {
                n += 1;
            }
            i += 1;
        }
        if n % 2 == 1 {
            return Some(q);
        }
    }
    None
}

/// `export `, `set `, `ENV `, `- `, `$env:`, indentation only.
fn is_line_start_prefix(prefix: &str) -> bool {
    let mut p = prefix.trim_start();
    for lead in [
        "export ",
        "set ",
        "setx ",
        "ENV ",
        "- ",
        "$env:",
        "declare -x ",
        "readonly ",
        "env ",
    ] {
        if let Some(r) = p.strip_prefix(lead) {
            p = r.trim_start();
            break;
        }
    }
    p.is_empty()
}

/// The prefix without a comment marker: code in a comment or doc comment is still code
/// (`///     let tokens = quote! { … }`).
fn uncommented(prefix: &str) -> &str {
    let p = prefix.trim();
    for c in ["///", "//!", "//", "#", "*", "--", ">"] {
        if let Some(r) = p.strip_prefix(c) {
            return r.trim_start();
        }
    }
    p
}

/// The key sits after `(`, `,`, `{` or `->`: a call, an object literal, a struct dump
/// (`User(id=42, password=v)`) or a list in prose (`user: admin, password: v`). This alone
/// does not make the value code; see [`RawPair::args`].
fn is_args_prefix(prefix: &str) -> bool {
    let p = uncommented(prefix);
    p.ends_with('(') || p.ends_with(',') || p.ends_with('{') || p.ends_with("->")
}

/// Declaration keywords at the start of the line (`let`, `const`, `def`, `self.`): a code
/// statement.
fn is_code_prefix(prefix: &str) -> bool {
    let p = uncommented(prefix);
    let first = p.split(|c: char| c.is_whitespace()).next().unwrap_or("");
    matches!(
        first,
        "let"
            | "const"
            | "var"
            | "val"
            | "my"
            | "our"
            | "auto"
            | "final"
            | "private"
            | "public"
            | "protected"
            | "internal"
            | "static"
            | "def"
            | "fn"
            | "func"
            | "function"
            | "pub"
            | "return"
            | "if"
            | "elif"
            | "else"
            | "while"
            | "for"
            | "await"
            | "yield"
            | "assert"
            | "String"
            | "string"
            | "char*"
            | "char"
            | "byte[]"
            | "dim"
            | "Dim"
    ) || p.starts_with("self.")
        || p.starts_with("this.")
}

/// A separator after `p` (blanks allowed around it): `=`, `:`, `:=`, `=>`, `→`.
/// `==`, `=~`, `::`, `?:` (optional field type) are not separators.
fn separator(text: &str, p: usize) -> Option<(Sep, bool, usize)> {
    let b = text.as_bytes();
    let q = skip_blank(text, p);
    let pre_blank = q > p;
    let rest = &text[q..];
    let (sep, len) = if rest.starts_with(":=") || rest.starts_with("=>") {
        (Sep::Other, 2)
    } else if rest.starts_with("==") || rest.starts_with("=~") || rest.starts_with("::") {
        return None;
    } else if rest.starts_with('=') {
        (Sep::Equals, 1)
    } else if rest.starts_with(':') {
        (Sep::Colon, 1)
    } else if rest.starts_with('→') {
        (Sep::Other, '→'.len_utf8())
    } else {
        return None;
    };
    let after = q + len;
    let post_blank = matches!(b.get(after), Some(b' ' | b'\t'));
    Some((sep, !pre_blank && !post_blank, skip_blank(text, after)))
}

fn skip_blank(text: &str, mut p: usize) -> usize {
    let b = text.as_bytes();
    while matches!(b.get(p), Some(b' ' | b'\t')) {
        p += 1;
    }
    p
}

/// How far an unquoted value may run.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// To the first blank or delimiter (code, prose, arguments, mid-line pairs).
    Token,
    /// To the end of the line, without a trailing comment (YAML, INI, `KEY = v`).
    Line,
    /// `KEY=v` alone on a line: the end of the line (dotenv, properties), unless a command
    /// or another `name=` field follows the first word (shell prefix, logfmt).
    Dotenv,
    /// One word, to the next blank: a command-line option or a logfmt field.
    Word,
}

/// How the value after a key is read.
#[derive(Clone, Copy)]
struct Ctx {
    mode: Mode,
    sep: Sep,
    line_start: bool,
    code: bool,
    /// The quote that closes a string literal around the key, if any.
    closing: Option<u8>,
    /// `DB_PASSWORD`: an environment-style key, never a keyword argument in code.
    upper: bool,
}

impl Ctx {
    fn new(
        mode: Mode,
        sep: Sep,
        line_start: bool,
        code: bool,
        closing: Option<u8>,
        upper: bool,
    ) -> Ctx {
        Ctx {
            mode,
            sep,
            line_start,
            code,
            closing,
            upper,
        }
    }

    /// A config line, an option or a logfmt field: the value is a literal.
    fn literal(&self) -> bool {
        self.mode != Mode::Token && !self.code
    }
}

/// Parses a value starting at `p`.
fn value_after(text: &str, p: usize, cx: Ctx) -> Option<RawPair> {
    let b = text.as_bytes();
    let mut p = p;
    let literal = cx.literal();
    let raw = RawPair {
        value: None,
        syntax: Syntax::Assign,
        quoted: false,
        sep: Sep::None,
        tight: false,
        line_start: cx.line_start,
        code: cx.code,
        args: false,
        stmt_end: false,
        literal,
        scanned: p,
    };
    if cx.closing.is_some() && b.get(p).copied() == cx.closing {
        return Some(raw);
    }
    // `secret = # comment`: after a blank, `#` starts a comment, not a value.
    if b.get(p) == Some(&b'#') && p > 0 && matches!(b[p - 1], b' ' | b'\t') {
        return Some(raw);
    }
    match b.get(p) {
        None | Some(b'\n' | b'\r') => return Some(raw),
        // Python string prefixes: r"…", b'…', f"…".
        Some(b'r' | b'b' | b'f' | b'u' | b'R' | b'B' | b'F' | b'U')
            if matches!(b.get(p + 1), Some(b'"' | b'\'')) =>
        {
            p += 1;
        }
        _ => {}
    }
    if let Some(b'"' | b'\'' | b'`') = b.get(p) {
        let (value, scanned, closed) = quoted_value(text, p)?;
        // In a literal, a quote that does not close on its line is a character of the
        // value (`password='Cedar` in a logfmt line).
        if closed || !literal {
            let le = line_end(text, scanned);
            let tail = text[scanned..le.max(scanned)].trim();
            let stmt_end = tail.ends_with(';') || tail.ends_with(',') && tail.len() > 1;
            return Some(RawPair {
                value,
                quoted: true,
                scanned,
                stmt_end,
                ..raw
            });
        }
    }
    // Structures and YAML block scalars are not values. In a literal only YAML has them
    // (`password: [a, b]`, `password: |`); `DB_PASSWORD=(Orchid)x` and `--password=[x]y`
    // are values.
    let structure = match b.get(p) {
        Some(b'{' | b'[' | b'|' | b'>') => !literal || cx.sep == Sep::Colon,
        Some(b'(' | b'<') => !literal,
        _ => false,
    };
    if structure {
        return Some(raw);
    }
    match cx.mode {
        Mode::Line | Mode::Dotenv if literal => Some(line_value(text, p, cx, raw)),
        Mode::Word => {
            let e = word_end(text, p, cx.closing);
            let (vs, ve) = trim_value(text, p, e, literal);
            Some(RawPair {
                value: (ve > vs).then_some(vs..ve),
                scanned: e,
                ..raw
            })
        }
        _ => {
            let ve = unquoted_end(text, p);
            let le = line_end(text, ve);
            let tail = text[ve..le].trim();
            let stmt_end = tail == ";" || tail.ends_with(';') && tail.len() <= 2;
            let (vs, ve) = trim_value(text, p, ve, false);
            Some(RawPair {
                value: (ve > vs).then_some(vs..ve),
                scanned: ve.max(p),
                stmt_end,
                ..raw
            })
        }
    }
}

/// The value of a config line: the rest of the line without a trailing comment, or the
/// first word of a shell or logfmt line. Reads the whole line however long it is, and
/// consumes it, so the work stays linear.
fn line_value(text: &str, p: usize, cx: Ctx, raw: RawPair) -> RawPair {
    let b = text.as_bytes();
    if cx.mode == Mode::Dotenv {
        // A shell or logfmt line: the scan resumes after the first word, so the fields
        // that follow are read as pairs of their own.
        let i = word_end(text, p, None);
        if matches!(b.get(i), Some(b' ' | b'\t')) && shell_tail(&text[i..line_end(text, i)]) {
            let mut s = &text[p..i];
            if let Some(c) = fields_tail(s) {
                s = &s[..c];
            }
            let (vs, ve) = trim_value(text, p, p + s.len(), true);
            return RawPair {
                value: (ve > vs).then_some(vs..ve),
                scanned: i,
                ..raw
            };
        }
    }
    let le = line_end_full(text, p);
    let mut s = &text[p..le];
    for c in [" #", "\t#", " //", "\t//", " ;", "\t;"] {
        if let Some(i) = s.find(c) {
            s = &s[..i];
        }
    }
    if let Some(i) = fields_tail(s) {
        s = &s[..i];
    }
    let mut st = s.trim_end();
    // `password = x;` ends a statement and `password: x,` an object field when what stands
    // before is code (`db_pass`, `getPass()`). An environment key is never code, and
    // `password = Cedar,9` keeps its comma.
    let mut stmt_end = false;
    let end = match cx.sep {
        Sep::Colon | Sep::Other => ',',
        _ => ';',
    };
    if !cx.upper {
        if let Some(rest) = st.strip_suffix(end) {
            if code_like(rest.trim_end()) {
                stmt_end = true;
                st = rest.trim_end();
            }
        }
    }
    let (vs, ve) = trim_value(text, p, p + st.len(), true);
    RawPair {
        value: (ve > vs).then_some(vs..ve),
        scanned: le,
        stmt_end,
        ..raw
    }
}

/// `db_pass`, `config.pw`, `getPass()`, `PW`: code before a `;` or `,`, not a password.
fn code_like(v: &str) -> bool {
    if v.is_empty() || v.ends_with([')', ']']) {
        return true;
    }
    let ident = |s: &str| {
        let mut cs = s.chars();
        cs.next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
            && cs.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    };
    if !v.split('.').all(ident) {
        return false;
    }
    let first = v.as_bytes()[0];
    v.contains(['.', '_'])
        || first.is_ascii_lowercase()
        || first == b'$'
        || !v.bytes().any(|c| c.is_ascii_lowercase())
}

/// The end of a word at `p`: the next blank, line end or `closing` quote.
fn word_end(text: &str, p: usize, closing: Option<u8>) -> usize {
    let b = text.as_bytes();
    p + b[p..]
        .iter()
        .position(|&c| matches!(c, b' ' | b'\t' | b'\n' | b'\r') || Some(c) == closing)
        .unwrap_or(b.len() - p)
}

/// `level=warn password=v user=app`: the key is one of several `name=value` fields, so its
/// value is the word up to the next blank.
fn logfmt_field(text: &str, prefix: &str, after: usize) -> bool {
    if !prefix.ends_with([' ', '\t']) {
        return false;
    }
    let prev = prefix.split_whitespace().next_back();
    let e = word_end(text, after, None);
    let n = skip_blank(text, e);
    let next = &text[n..word_end(text, n, None)];
    prev.is_some_and(is_field) || is_field(next)
}

/// `name=…` with a plain name.
fn is_field(w: &str) -> bool {
    let name = w
        .bytes()
        .take(128)
        .take_while(|c| c.is_ascii_alphanumeric() || b"_.-".contains(c))
        .count();
    name > 0 && w.as_bytes().get(name) == Some(&b'=')
}

/// A quoted string at `p` (`"…"` with backslash escapes, `'…'`, `` `…` ``) on one line.
/// Returns the inner range (or `None` for an empty / unterminated string), the end, and
/// whether the closing quote was found.
fn quoted_value(text: &str, p: usize) -> Option<(Option<Range<usize>>, usize, bool)> {
    let b = text.as_bytes();
    let q = *b.get(p)?;
    if !matches!(q, b'"' | b'\'' | b'`') {
        return None;
    }
    let mut i = p + 1;
    while i < b.len() {
        match b[i] {
            // A quoted value never runs into the next line, not even escaped.
            b'\\' if q != b'`' && matches!(b.get(i + 1), Some(b'\n' | b'\r')) => {
                return Some((None, i + 1, false));
            }
            b'\\' if q != b'`' => i += 2,
            b'\n' => return Some((None, i, false)),
            c if c == q => {
                let v = (i > p + 1).then_some(p + 1..i);
                return Some((v, i + 1, true));
            }
            _ => i += 1,
        }
    }
    Some((None, b.len(), false))
}

/// Whether the `&` or `;` at `i` ends an unquoted value: it does when the next pair starts
/// after it (`&next=`, `&amp;next=`, `;Database=`: a query or connection string), and a `;`
/// also at the end of the line or before a blank. Otherwise it is part of the value
/// (`p@ss&word!`, `Tr0ub4dor&3`).
fn ends_value(b: &[u8], i: usize) -> bool {
    let mut j = i + 1;
    match b[i] {
        b';' if b.get(j).is_none_or(|c| c.is_ascii_whitespace()) => return true,
        b';' => {}
        b'&' if b[j..].starts_with(b"amp;") => j += 4,
        b'&' => {}
        _ => return false,
    }
    let name = |c: &&u8| c.is_ascii_alphanumeric() || b"_.-[]%".contains(c);
    let n = b[j..].iter().take(128).take_while(name).count();
    n > 0 && b.get(j + n) == Some(&b'=')
}

/// Where a config value ends because a query or connection string goes on after it
/// (`Abc&123;Database=y`, `hunter2&next=/home`): at the `;` or `&` after which every
/// segment is a `name=value` field. A `;` or `&` inside a password (`Fake;9xQ2`,
/// `a&b=c+d(e`) stays: the rest is not all fields. One pass from the end.
fn fields_tail(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let field = |seg: &[u8]| {
        let n = seg
            .iter()
            .take(128)
            .take_while(|c| c.is_ascii_alphanumeric() || b"_.-[]%".contains(c))
            .count();
        n > 0 && seg.get(n) == Some(&b'=')
    };
    let mut cut = None;
    let mut end = b.len();
    let mut i = b.len();
    while i > 0 {
        i -= 1;
        if !matches!(b[i], b';' | b'&') {
            continue;
        }
        // The `;` of an `&amp;` entity belongs to it.
        if b[i] == b';' && i >= 4 && &b[i - 4..i] == b"&amp" {
            continue;
        }
        let start = if b[i..].starts_with(b"&amp;") {
            i + 5
        } else {
            i + 1
        };
        let seg = &b[start.min(end)..end];
        if field(seg) {
            cut = Some(i);
        } else if !(seg.is_empty() && end == b.len()) {
            break;
        }
        end = i;
    }
    cut.filter(|&c| c > 0)
}

/// After the first word of a `KEY=v …` line: a command (an option, a shell operator, a
/// path) or another `name=` field. Then the line is shell or logfmt, not dotenv.
fn shell_tail(tail: &str) -> bool {
    tail.split_whitespace().any(|w| {
        let b = w.as_bytes();
        (b[0] == b'-' && b.len() > 1)
            || matches!(b[0], b'&' | b'|' | b';' | b'>' | b'<' | b'/')
            || w.starts_with("./")
            || w.starts_with("2>")
            || w.find('=').is_some_and(|k| {
                k > 0
                    && w[..k]
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
            })
    })
}

/// End of an unquoted mid-line value: whitespace or one of `,|>}"'<`, a `&` or `;` that
/// [`ends_value`], or an unbalanced `)` / `]`.
fn unquoted_end(text: &str, p: usize) -> usize {
    let b = text.as_bytes();
    let mut depth_paren = 0i32;
    let mut depth_brack = 0i32;
    let mut i = p;
    while i < b.len() {
        match b[i] {
            // `\n`, `\"` inside a string literal that holds this text.
            b'\\' if is_escape(b.get(i + 1).copied()) => break,
            b';' | b'&' if ends_value(b, i) => break,
            b' ' | b'\t' | b'\n' | b'\r' | b',' | b'|' | b'>' | b'}' | b'"' | b'\'' | b'<'
            | b'`' => break,
            b'(' => depth_paren += 1,
            b'[' => depth_brack += 1,
            b')' => {
                if depth_paren == 0 {
                    break;
                }
                depth_paren -= 1;
            }
            b']' => {
                if depth_brack == 0 {
                    break;
                }
                depth_brack -= 1;
            }
            _ => {}
        }
        i += 1;
    }
    i
}

/// Trims blanks, and outside a literal trailing prose punctuation. An elided example
/// (`sk-...`, `abc…`) is no value at all.
fn trim_value(text: &str, s: usize, e: usize, literal: bool) -> (usize, usize) {
    let v = &text[s..e];
    if v.trim_end().ends_with("...") || v.trim_end().ends_with('…') {
        return (s, s);
    }
    let t = v.trim_start();
    let s2 = s + (v.len() - t.len());
    let mut t = t.trim_end();
    if !literal {
        t = t.trim_end_matches(['.', ',', ';', ':', '?']);
    }
    (s2, s2 + t.len())
}

// ---------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Tok {
    start: usize,
    end: usize,
}

/// Whitespace-separated shell tokens from `from` to the end of the line (at most 64, 1 KiB);
/// a token that starts with a quote runs to the closing quote.
fn shell_tokens(text: &str, from: usize) -> Vec<Tok> {
    let b = text.as_bytes();
    let le = floor(text, line_end(text, from).min(from + 1024));
    let mut out = Vec::new();
    let mut i = from;
    while i < le && out.len() < 64 {
        while i < le && matches!(b[i], b' ' | b'\t') {
            i += 1;
        }
        if i >= le {
            break;
        }
        let st = i;
        if matches!(b[i], b'"' | b'\'') {
            let q = b[i];
            i += 1;
            while i < le && b[i] != q {
                i += 1;
            }
            i = (i + 1).min(le);
        } else {
            while i < le && !matches!(b[i], b' ' | b'\t' | b'`') {
                i += 1;
            }
        }
        if i == st {
            // A lone backtick (end of a Markdown code span) ends the command.
            break;
        }
        out.push(Tok { start: st, end: i });
    }
    out
}

/// Strips one pair of surrounding quotes.
fn unquote(text: &str, s: usize, e: usize) -> Range<usize> {
    let b = text.as_bytes();
    if e >= s + 2 && matches!(b[s], b'"' | b'\'') && b[e - 1] == b[s] {
        s + 1..e - 1
    } else {
        s..e
    }
}

/// The password argument of a command whose name ends at `end`.
pub(crate) fn command(text: &str, end: usize, cmd: Cmd) -> Option<Range<usize>> {
    let toks = shell_tokens(text, end);
    let t = |i: usize| -> &str { &text[toks[i].start..toks[i].end] };
    let next = |i: usize| -> Option<Range<usize>> {
        toks.get(i + 1).map(|n| unquote(text, n.start, n.end))
    };
    let r = match cmd {
        Cmd::Curl => (0..toks.len()).find_map(|i| {
            let tok = t(i);
            let cred = if matches!(tok, "-u" | "--user" | "-U" | "--proxy-user") {
                next(i)?
            } else if let Some(rest) = tok.strip_prefix("--user=") {
                let s = toks[i].end - rest.len();
                unquote(text, s, toks[i].end)
            } else if tok.len() > 2 && tok.starts_with("-u") && !tok.starts_with("--") {
                unquote(text, toks[i].start + 2, toks[i].end)
            } else {
                return None;
            };
            let colon = text[cred.clone()].find(':')?;
            Some(cred.start + colon + 1..cred.end)
        }),
        Cmd::Mysql | Cmd::SevenZip => (0..toks.len()).find_map(|i| {
            let tok = t(i);
            (tok.len() > 2 && tok.starts_with("-p") && !tok.starts_with("--"))
                .then(|| unquote(text, toks[i].start + 2, toks[i].end))
        }),
        Cmd::Sshpass => (0..toks.len()).find_map(|i| (t(i) == "-p").then(|| next(i)).flatten()),
        Cmd::Ldap => (0..toks.len()).find_map(|i| (t(i) == "-w").then(|| next(i)).flatten()),
        Cmd::Zip => (0..toks.len()).find_map(|i| (t(i) == "-P").then(|| next(i)).flatten()),
        Cmd::DockerLogin => {
            if !(0..toks.len()).any(|i| t(i) == "login") {
                return None;
            }
            (0..toks.len()).find_map(|i| (t(i) == "-p").then(|| next(i)).flatten())
        }
        Cmd::Sqlplus => toks.iter().find_map(|tk| {
            let s = &text[tk.start..tk.end];
            if s.starts_with('-') || s.starts_with('/') {
                return None;
            }
            let slash = s.find('/')?;
            let rest = &s[slash + 1..];
            let pe = rest.find('@').unwrap_or(rest.len());
            (slash > 0 && pe > 0).then(|| tk.start + slash + 1..tk.start + slash + 1 + pe)
        }),
        Cmd::SecureString => {
            let lower = text[end..line_end(text, end)].to_ascii_lowercase();
            if !lower.contains("-asplaintext") {
                return None;
            }
            let i = usize::from(
                toks.first()
                    .is_some_and(|_| t(0).eq_ignore_ascii_case("-string")),
            );
            let tk = toks.get(i)?;
            if text[tk.start..tk.end].starts_with('-') {
                return None;
            }
            Some(unquote(text, tk.start, tk.end))
        }
        Cmd::IdentifiedBy => {
            // SQL quotes the password; unquoted only on a `… USER …` line ("identified by
            // four values" is English).
            let tk = toks.first()?;
            let s = &text[tk.start..tk.end];
            let e = tk.start + s.trim_end_matches(';').len();
            let quoted = s.starts_with(['\'', '"']);
            let ls = line_start(text, end);
            let line = text[ls..end].to_ascii_lowercase();
            if !quoted && !(line.contains("user ") || line.contains("role ")) {
                return None;
            }
            Some(unquote(text, tk.start, e))
        }
        // `tool login -u alice -p v`: `-p` after a user option. In a sentence, `the login is
        // alice / v` names a user and its password.
        Cmd::Login => {
            if toks.len() >= 4 && matches!(t(0), "is" | "ist" | "=" | ":") && t(2) == "/" {
                let r = unquote(text, toks[3].start, toks[3].end);
                let v = text[r.clone()].trim_end_matches(['.', ',', ';']);
                // `login is demo / Password: v`: a key follows, its own rule takes the value.
                (!v.ends_with(':') && super::looks_like_password(v))
                    .then_some(r.start..r.start + v.len())
            } else {
                (0..toks.len()).find_map(|i| {
                    let user =
                        (0..i).any(|j| matches!(t(j), "-u" | "-U" | "--user" | "--username"));
                    (user && t(i) == "-p").then(|| next(i)).flatten()
                })
            }
        }
        // `htpasswd -b file user v`, `htpasswd -nb user v`: the last positional argument
        // when `-b` takes the password from the command line.
        Cmd::Htpasswd => {
            let flags: String = (0..toks.len())
                .map(t)
                .filter(|s| s.starts_with('-') && !s.starts_with("--"))
                .collect();
            if !flags.contains('b') {
                return None;
            }
            let pos: Vec<&Tok> = toks
                .iter()
                .filter(|tk| !text[tk.start..tk.end].starts_with('-'))
                .collect();
            let tk = pos.get(if flags.contains('n') { 1 } else { 2 })?;
            Some(unquote(text, tk.start, tk.end))
        }
        Cmd::NetUse => toks.iter().find_map(|tk| {
            let s = &text[tk.start..tk.end];
            let skip = s.starts_with('/')
                || s.starts_with("\\\\")
                || s == "*"
                || (s.len() == 2 && s.ends_with(':'));
            (!skip).then(|| unquote(text, tk.start, tk.end))
        }),
    }?;
    // Trailing punctuation of the surrounding text (`…:pw',`, `…pw;`).
    let mut r = r;
    // A `)` closes the surrounding text only when the value has no `(` to match it.
    while r.end > r.start {
        let v = &text[r.clone()];
        let unmatched = v.ends_with(')') && v.matches('(').count() < v.matches(')').count();
        if !(v.ends_with([',', ';']) || unmatched) {
            break;
        }
        r.end -= 1;
    }
    let b = text.as_bytes();
    if r.end > r.start && matches!(b[r.end - 1], b'\'' | b'"') && b[r.start] != b[r.end - 1] {
        r.end -= 1;
    }
    (r.end > r.start).then_some(r)
}
