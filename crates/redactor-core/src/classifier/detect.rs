//! Standalone detection of unlabeled tokens.
//!
//! Balanced: `p ≥ 0.92`, length ≥ 20, ≥ 3 character classes, not hex-only, not a UUID →
//! confidence 0.55 ("Possible"). Paranoid (threshold 0.30): `p ≥ 0.75`, length ≥ 16 → 0.45,
//! and hex runs of 32+ that no hash word explains (`commit`, `sha`, `digest`, …).

use std::borrow::Cow;

use crate::detector::Detector;
use crate::finding::{Candidate, Source};

/// Detector id.
pub(crate) const ID: &str = "classifier";
const LABEL: &str = "Random String";

const BALANCED_P: f32 = 0.92;
const BALANCED_LEN: usize = 20;
const BALANCED_CONF: f32 = 0.55;
const PARANOID_P: f32 = 0.75;
const PARANOID_LEN: usize = 16;
const PARANOID_CONF: f32 = 0.45;
const HEX_LEN: usize = 32;
const HEX_P: f32 = 0.5;
/// Very long runs are scored on their start (the features look at 256 characters anyway).
const MAX_RUN: usize = 4096;

/// Words before a hex or base64 run that say it is a digest, not a secret.
const HASH_WORDS: &[&str] = &[
    "commit",
    "commits",
    "sha",
    "hash",
    "hashes",
    "digest",
    "checksum",
    "integrity",
    "etag",
    "revision",
    "rev",
    "md",
    "blake",
];

pub(crate) struct ClassifierDetector;

fn token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'_' | b'-')
}

impl Detector for ClassifierDetector {
    fn id(&self) -> &'static str {
        ID
    }

    fn detect(&self, text: &str, out: &mut Vec<Candidate>) {
        let bytes = text.as_bytes();
        let public = public_blocks(text);
        let mut i = 0;
        while i < bytes.len() {
            if !token_byte(bytes[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && token_byte(bytes[i]) {
                i += 1;
            }
            let mut end = i;
            // Base64 padding belongs to the run when nothing glues on after it and it
            // completes a 4-character group: `KEY=` before a placeholder is a key and its `=`.
            let mut pad = end;
            while pad < bytes.len() && pad - end < 2 && bytes[pad] == b'=' {
                pad += 1;
            }
            if pad > end
                && (pad - start) % 4 == 0
                && (pad == bytes.len() || !token_byte(bytes[pad]) && bytes[pad] != b'=')
            {
                end = pad;
                i = pad;
            }
            if end - start < PARANOID_LEN {
                continue;
            }
            // Leading and trailing separators are not part of a token, nor is the letter of
            // an escape (`\nZq81…` in a JSON string) or the tail of an ANSI colour code
            // (`ESC[1mZq81…`) before it.
            let mut s = escape_tail(text, start, end);
            let mut e = end;
            while s < e && matches!(bytes[s], b'/' | b'-' | b'_' | b'+') {
                s += 1;
            }
            while e > s && matches!(bytes[e - 1], b'/' | b'-' | b'_' | b'+') {
                e -= 1;
            }
            if e - s < PARANOID_LEN || in_blocks(&public, s) {
                continue;
            }
            if let Some(c) = judge(text, s, e) {
                out.push(c);
            }
        }
    }
}

/// Where a run starting at `start` begins after an escape glued in front: `n` of `\n`, or
/// `33m` of `ESC[1;33m`.
fn escape_tail(text: &str, start: usize, end: usize) -> usize {
    let b = text.as_bytes();
    if start > 0 && b[start - 1] == b'\\' && matches!(b[start], b'n' | b'r' | b't') {
        return if crate::rules::escape_ends_at(text, start + 1) {
            start + 1
        } else {
            start
        };
    }
    let digits = b[start..end]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count();
    let m = start + digits;
    if m < end
        && b[m] == b'm'
        && matches!(b.get(start.wrapping_sub(1)), Some(b'[' | b';'))
        && crate::rules::escape_ends_at(text, m + 1)
    {
        return m + 1;
    }
    start
}

fn judge(text: &str, s: usize, e: usize) -> Option<Candidate> {
    let bytes = text.as_bytes();
    // Glued to something that makes it part of another syntax: `%41…` (URL escapes), `\u…`,
    // `.` (domains, file names, JWT parts are the catalog's business), `@`, `:`, `$`.
    let alnum_at = |i: usize| bytes.get(i).is_some_and(u8::is_ascii_alphanumeric);
    if s > 0 && matches!(bytes[s - 1], b'%' | b'\\' | b'@' | b'$' | b'&' | b'#')
        || s > 1 && bytes[s - 1] == b'.' && alnum_at(s - 2)
    {
        return None;
    }
    if e < bytes.len() && (matches!(bytes[e], b'@' | b'(') || bytes[e] == b'.' && alnum_at(e + 1)) {
        return None;
    }
    let tok = &text[s..e.min(s + MAX_RUN)];
    let lower = tok.to_ascii_lowercase();
    if ["sha1-", "sha256-", "sha384-", "sha512-", "md5-"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        return None;
    }
    // `;base64,` data URLs; `h1:` is the hash prefix of go.sum lines.
    if text[..s].ends_with(";base64,") || text[..s].ends_with("h1:") || hash_context(text, s) {
        return None;
    }
    let n = e - s;
    let tb = tok.as_bytes();
    if periodic(tb)
        || alphabet(tb)
        || is_identifier(tb)
        || ssh_public_key(text, s, tb)
        || pod_name(tb)
    {
        return None;
    }
    if tb.iter().all(u8::is_ascii_hexdigit) {
        // Paranoid only; digits and letters both (a long number is a number), and not a
        // pattern (`deadbeefdeadbeef…`).
        let mixed = tb.iter().any(u8::is_ascii_digit) && tb.iter().any(u8::is_ascii_alphabetic);
        let random = n >= HEX_LEN && mixed && super::secret_probability(tok) >= HEX_P;
        return random.then(|| candidate(s, e, PARANOID_CONF, "Long hex string"));
    }
    let mut classes = [false; 4];
    for &b in tb {
        let k = match b {
            b'a'..=b'z' => 0,
            b'A'..=b'Z' => 1,
            b'0'..=b'9' => 2,
            // Separators join words; they do not make a token more random.
            b'_' | b'-' => continue,
            _ => 3,
        };
        classes[k] = true;
    }
    let n_classes = classes.iter().filter(|c| **c).count();
    if n_classes < 2 || is_uuid_like(tb) {
        return None;
    }
    let p = super::secret_probability(tok);
    if p >= BALANCED_P && n >= BALANCED_LEN && n_classes >= 3 {
        Some(candidate(s, e, BALANCED_CONF, "Random-looking token"))
    } else if p >= PARANOID_P {
        Some(candidate(s, e, PARANOID_CONF, "Random-looking token"))
    } else {
        None
    }
}

fn candidate(s: usize, e: usize, confidence: f32, reason: &'static str) -> Candidate {
    Candidate {
        start: s,
        end: e,
        kind: Cow::Borrowed("random_string"),
        label: Cow::Borrowed(LABEL),
        confidence,
        detector: Cow::Borrowed(ID),
        reason: Cow::Borrowed(reason),
        source: Source::Classifier,
    }
}

/// The token repeats a block of at most 64 bytes (`deadbeefdeadbeef`, `abcabcabc`).
fn periodic(t: &[u8]) -> bool {
    (1..=(t.len() / 2).min(64)).any(|p| t[p..] == t[..t.len() - p])
}

/// An alphabet written out in order (`0123…XYZabc…xyz`, base58 and base32 tables).
fn alphabet(t: &[u8]) -> bool {
    let steps = t
        .windows(2)
        .filter(|w| w[1] == w[0].wrapping_add(1))
        .count();
    steps * 4 >= (t.len() - 1) * 3
}

/// A name built from words, acronyms and short numbers (`NR_semtimedop_time64`,
/// `kCFStringEncodingUTF8`, `Win32_System_DataExchange`, `TAG-windows-x64-portable`).
/// It splits at `_`, `-` and case changes; every segment is a word or an acronym with at
/// most four digits after it, and at most a third are one or two letters run into the next. A digit followed
/// by a lowercase letter (`K9wQe`) is random, not a name.
pub(crate) fn is_identifier(t: &[u8]) -> bool {
    // (segment, whether it is a whole `_`/`-` part)
    let mut segs: Vec<(&[u8], bool)> = Vec::new();
    for part in t.split(|&b| b == b'_' || b == b'-') {
        if !part.iter().all(u8::is_ascii_alphanumeric) {
            return false;
        }
        let mut st = 0;
        for i in 1..part.len() {
            let (p, c) = (part[i - 1], part[i]);
            if p.is_ascii_digit() && c.is_ascii_lowercase() {
                return false;
            }
            let next_lower = part.get(i + 1).is_some_and(u8::is_ascii_lowercase);
            if c.is_ascii_uppercase()
                && (p.is_ascii_lowercase()
                    || p.is_ascii_digit()
                    || p.is_ascii_uppercase() && next_lower)
            {
                segs.push((&part[st..i], false));
                st = i;
            }
        }
        if st == 0 {
            segs.push((part, true));
        } else if st < part.len() {
            segs.push((&part[st..], false));
        }
    }
    if segs.len() < 2 {
        return false;
    }
    let mut short = 0;
    let mut long = false;
    for &(seg, whole) in &segs {
        let n_letters = seg.iter().take_while(|b| b.is_ascii_alphabetic()).count();
        let (letters, digits) = seg.split_at(n_letters);
        if digits.len() > 4 {
            return false;
        }
        if letters.len() <= 2 {
            // `NR_rt_…`, `x64`: short parts written apart are deliberate.
            short += usize::from(!whole);
            continue;
        }
        long |= letters.len() >= 4;
        let acronym = letters.len() <= 4 && letters.iter().all(u8::is_ascii_uppercase);
        if !acronym && !word_like(letters) {
            return false;
        }
    }
    // A one-letter lowercase prefix (`kCFString…`, `bIsValid`) is a naming convention.
    let (first, whole) = segs[0];
    let lead = usize::from(!whole && first.len() == 1 && first[0].is_ascii_lowercase());
    long && (short - lead) * 3 <= segs.len() - lead
}

/// The key of an OpenSSH public key line (`ssh-ed25519 AAAAC3Nza… user@host`): public.
fn ssh_public_key(text: &str, s: usize, tok: &[u8]) -> bool {
    if !tok.starts_with(b"AAAA") {
        return false;
    }
    let mut from = s.saturating_sub(64);
    while !text.is_char_boundary(from) {
        from += 1;
    }
    let before = &text[from..s];
    let Some(head) = before.strip_suffix(' ') else {
        return false;
    };
    let kind = head.rsplit(char::is_whitespace).next().unwrap_or("");
    ["ssh-", "ecdsa-sha2-", "sk-ssh-", "sk-ecdsa-"]
        .iter()
        .any(|p| kind.starts_with(p))
}

/// A Kubernetes pod name (`payment-service-7d9f8c6b5d-x2k4q`, after an optional
/// `pod/`): a lowercase name, the pod-template hash and a five-character suffix, both from
/// the alphabet Kubernetes generates them with (no vowels, no `0`, `1`, `3`).
fn pod_name(tok: &[u8]) -> bool {
    const SAFE: &[u8] = b"bcdfghjklmnpqrstvwxz2456789";
    let name = tok.rsplit(|&b| b == b'/').next().unwrap_or(tok);
    let mut parts = name.rsplitn(3, |&b| b == b'-');
    let (Some(suffix), Some(hash), Some(base)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    suffix.len() == 5
        && (6..=10).contains(&hash.len())
        && suffix.iter().chain(hash).all(|b| SAFE.contains(b))
        && base.first().is_some_and(u8::is_ascii_lowercase)
        && base
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// Letters that read as a word: vowels in a fair share and no long consonant runs.
pub(crate) fn word_like(w: &[u8]) -> bool {
    let vowel = |b: &u8| b"aeiouy".contains(&b.to_ascii_lowercase());
    let vowels = w.iter().filter(|b| vowel(b)).count();
    let mut run = 0;
    let mut max_run = 0;
    for b in w {
        run = if vowel(b) { 0 } else { run + 1 };
        max_run = max_run.max(run);
    }
    vowels > 0 && vowels * 7 >= w.len() && max_run <= 5
}

/// Hex groups joined by dashes (UUIDs and their relatives).
fn is_uuid_like(t: &[u8]) -> bool {
    t.contains(&b'-')
        && t.split(|&b| b == b'-')
            .all(|g| !g.is_empty() && g.iter().all(u8::is_ascii_hexdigit))
}

/// A hash word among the few words before `s` on the same line.
fn hash_context(text: &str, s: usize) -> bool {
    // Only the last 40 bytes are read: a search back to the line start would make one long
    // line of tokens quadratic.
    let mut from = s.saturating_sub(40);
    while !text.is_char_boundary(from) {
        from += 1;
    }
    if let Some(i) = text[from..s].rfind('\n') {
        from += i + 1;
    }
    text[from..s]
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| !w.is_empty())
        .any(|w| HASH_WORDS.contains(&w.to_ascii_lowercase().as_str()))
}

/// Whether `s` lies in one of `blocks` (sorted, disjoint).
fn in_blocks(blocks: &[(usize, usize)], s: usize) -> bool {
    let i = blocks.partition_point(|r| r.0 <= s);
    i > 0 && s < blocks[i - 1].1
}

/// Byte ranges of PEM blocks that hold public material (certificates, public keys), sorted
/// and disjoint. A block ends at its END marker or at the next BEGIN, whichever comes first;
/// one forward pass.
fn public_blocks(text: &str) -> Vec<(usize, usize)> {
    let begins: Vec<usize> = text.match_indices("-----BEGIN ").map(|(i, _)| i).collect();
    let mut out = Vec::new();
    // The first END marker after some point, reused while it lies ahead (`Some(None)`: none
    // left), so every byte is searched for END once.
    let mut next_end: Option<Option<usize>> = None;
    for (k, &s) in begins.iter().enumerate() {
        let stop = begins.get(k + 1).copied().unwrap_or(text.len());
        let line_end = text[s..stop].find('\n').map_or(stop, |j| s + j);
        let end_at = match next_end {
            Some(Some(e)) if e >= line_end => Some(e),
            Some(None) => None,
            _ => {
                let e = text[line_end..].find("-----END ").map(|j| line_end + j);
                next_end = Some(e);
                e
            }
        };
        let header = &text[s..line_end];
        if header.contains("CERTIFICATE") || header.contains("PUBLIC KEY") {
            out.push((s, end_at.map_or(stop, |e| e.min(stop))));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<&str> {
        let mut out = Vec::new();
        ClassifierDetector.detect(text, &mut out);
        out.iter().map(|c| &text[c.start..c.end]).collect()
    }

    /// Identifiers built from words, acronyms and short numbers are names, not secrets.
    #[test]
    fn identifiers_are_not_random() {
        for t in [
            "pub const NR_semtimedop_time64: i64 = 420;",
            "pub const __NR_rt_sigtimedwait_time64: i64 = 421;",
            "pub const __NR_clock_nanosleep_time64: i64 = 407;",
            "pub const kCFStringEncodingUTF8: u32 = 0x08000100;",
            "pub const IPV6_RECVPATHMTU_NOTIFICATION: i32 = 61;",
            "    \"Win32_System_DataExchange\",",
            "    \"Win32_System_RemoteDesktop\",",
            "$name = \"Redactor-$env:TAG-windows-x64-portable\"",
            "pub fn VkPhysicalDeviceSubgroupSizeControlPropertiesEXT_init(p: *mut u8);",
            "struct Utf8SingleCodeUnitWriter;",
            "let env: ICoreWebView2Environment = todo!();",
        ] {
            assert!(found(t).is_empty(), "{t}: {:?}", found(t));
        }
    }

    /// Mixed-case tokens with digits inside a word stay random.
    #[test]
    fn random_tokens_still_found() {
        for t in [
            "x Zq81mXv0pLr3TfK9wQe2Lm y",
            "x aB3dE5fG7hI9jK1lM3nOpQ y",
            "x Rk7_Vt2pQ9_Lm4xZw8_Nb3Hs y",
        ] {
            assert_eq!(found(t).len(), 1, "{t}");
        }
    }

    /// Alphabets written out in order and go.sum hashes (`h1:`) are not secrets.
    #[test]
    fn alphabets_and_go_sum() {
        for t in [
            "const A: &[u8] = b\"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz\";",
            "B58 = \"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz\"",
            "golang.org/x/text v0.14.0 h1:z3R8QgsVabzxZ3+/cW3dRrB09zQmQcwPUHCmvk9g8o4=",
        ] {
            assert!(found(t).is_empty(), "{t}: {:?}", found(t));
        }
    }

    /// OpenSSH public keys and Kubernetes pod names are not secrets; the same random key
    /// without the `ssh-` type before it still is.
    #[test]
    fn ssh_public_keys_and_pod_names() {
        let key = "AAAAC3NzaC1lZDI1NTE5AAAAIHcY4Zq81mXv0pLr3TfK9wQe2Lm4xZq81mXv0pLr3TfK";
        for t in [
            format!("ssh-ed25519 {key} jdoe@laptop"),
            "pod/payment-service-7d9f8c6b5d-x2k4q   1/1   Running".to_string(),
        ] {
            assert!(found(&t).is_empty(), "{t}: {:?}", found(&t));
        }
        let bare = format!("token {key}");
        assert!(!found(&bare).is_empty(), "{bare}");
    }
}
