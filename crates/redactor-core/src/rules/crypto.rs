//! Keys and crypto material: PEM/PGP/PuTTY/SSH2/OpenVPN blocks, BIP-39 seed phrases, JWTs,
//! WIF keys, WireGuard keys and kubeconfig key data.

use std::borrow::Cow;
use std::sync::OnceLock;

use super::{validate, Bound, Family, Hit, Rule, RuleInfo, Verdict};
use crate::detector::Detector;
use crate::finding::{Candidate, Source};

fn r(
    id: &'static str,
    label: &'static str,
    shape: &'static str,
    anchors: &'static [&'static str],
    re: &'static str,
    conf: f32,
) -> Rule {
    Rule::new(Family::Crypto, id, label, shape, anchors, re, conf)
}

fn jwt(h: &Hit) -> Verdict {
    let header = h.secret().split('.').next().unwrap_or("");
    if validate::jwt_header_ok(header) {
        Verdict::Keep
    } else {
        Verdict::Reject
    }
}

/// Bitcoin WIF: base58check payload = version (0x80 mainnet / 0xEF testnet) + 32-byte key
/// [+ 0x01 when the public key is compressed].
fn wif(h: &Hit) -> Verdict {
    match validate::base58check(h.secret()) {
        Some(p)
            if matches!(p[0], 0x80 | 0xEF) && (p.len() == 33 || (p.len() == 34 && p[33] == 1)) =>
        {
            Verdict::Keep
        }
        _ => Verdict::Reject,
    }
}

pub(super) fn rules() -> Vec<Rule> {
    vec![
        r(
            "jwt",
            "JSON Web Token",
            "eyJ… . … . … (header decodes to JSON with alg/typ); JWE with 5 parts; wrapped after a dot",
            &["eyJ"],
            // JWS; JWE (`h.key.iv.ciphertext.tag`, the key empty for direct encryption);
            // a JWS wrapped after its dots (`h.⏎ payload.⏎ signature`).
            concat!(
                r"eyJ[A-Za-z0-9_-]{8,}\.(?:",
                r"[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]*(?:\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+)?",
                r"|\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+",
                r"|\r?\n[ \t]{0,8}eyJ[A-Za-z0-9_-]{8,}\.(?:\r?\n[ \t]{0,8})?[A-Za-z0-9_-]{16,})",
            ),
            0.95,
        )
        .fwd(16384)
        .check(jwt),
        r(
            "bitcoin_wif",
            "Bitcoin Private Key",
            "51-52 base58 chars starting 5/K/L/9/c (base58check valid)",
            &[],
            r"[5KL9c][1-9A-HJ-NP-Za-km-z]{50,51}",
            0.95,
        )
        .check(wif),
        r(
            "wireguard_private_key",
            "WireGuard Key",
            "PrivateKey/PresharedKey = 44 base64",
            &["PrivateKey", "PresharedKey"],
            r"(?:PrivateKey|PresharedKey)\s*=\s*(?P<s>[A-Za-z0-9+/]{42}[AEIMQUYcgkosw048]=)",
            0.95,
        )
        .fwd(80)
        .bounds(Bound::Word, Bound::Base64),
        r(
            "kubeconfig_client_key_data",
            "Private Key",
            "client-key-data: <base64 PEM>",
            &["client-key-data:"],
            r#"client-key-data:\s*["']?(?P<s>[A-Za-z0-9+/=]{20,})"#,
            0.97,
        )
        .fwd(65536)
        .bounds(Bound::Word, Bound::Base64),
    ]
}

// ---------------------------------------------------------------------------------------
// Structured detectors
// ---------------------------------------------------------------------------------------

const PEM_PRIVATE: &str = "private_key";
const PEM_PGP: &str = "pgp_private_key";
const PEM_OPENVPN: &str = "openvpn_static_key";
const PEM_SSH2: &str = "ssh2_private_key";
const PUTTY: &str = "putty_private_key";
const BIP39: &str = "bip39_seed_phrase";

pub(super) fn structured_infos() -> Vec<RuleInfo> {
    let info = |id, label, shape, confidence| RuleInfo {
        id,
        label,
        family: Family::Crypto,
        example_shape: shape,
        confidence,
        default_enabled: true,
    };
    vec![
        info(
            PEM_PRIVATE,
            "Private Key",
            "-----BEGIN … PRIVATE KEY----- block (PEM, OpenSSH, PKCS#8, encrypted)",
            1.0,
        ),
        info(
            PEM_PGP,
            "PGP Private Key",
            "-----BEGIN PGP PRIVATE KEY BLOCK----- block",
            1.0,
        ),
        info(
            PEM_OPENVPN,
            "OpenVPN Static Key",
            "-----BEGIN OpenVPN Static key V1----- block",
            1.0,
        ),
        info(
            PEM_SSH2,
            "Private Key",
            "---- BEGIN SSH2 ENCRYPTED PRIVATE KEY ---- block",
            1.0,
        ),
        info(
            PUTTY,
            "PuTTY Private Key",
            "PuTTY-User-Key-File-N … Private-Lines … Private-MAC",
            1.0,
        ),
        info(
            BIP39,
            "Recovery Phrase",
            "12/15/18/21/24 BIP-39 words with a valid checksum",
            0.97,
        ),
    ]
}

pub(super) fn structured(active: &dyn Fn(&str, bool) -> bool) -> Vec<Box<dyn Detector>> {
    let mut out: Vec<Box<dyn Detector>> = Vec::new();
    let pem = PemDetector {
        private: active(PEM_PRIVATE, true),
        pgp: active(PEM_PGP, true),
        openvpn: active(PEM_OPENVPN, true),
        ssh2: active(PEM_SSH2, true),
    };
    if pem.private || pem.pgp || pem.openvpn || pem.ssh2 {
        out.push(Box::new(pem));
    }
    if active(PUTTY, true) {
        out.push(Box::new(PuttyDetector));
    }
    if active(BIP39, true) {
        out.push(Box::new(Bip39Detector));
    }
    out
}

fn cand(
    start: usize,
    end: usize,
    id: &'static str,
    label: &'static str,
    conf: f32,
    reason: &'static str,
) -> Candidate {
    Candidate {
        start,
        end,
        kind: Cow::Borrowed(id),
        label: Cow::Borrowed(label),
        confidence: conf,
        detector: Cow::Borrowed(id),
        reason: Cow::Borrowed(reason),
        source: Source::Pattern,
    }
}

/// Longest block we follow before giving up on an END marker.
const MAX_BLOCK: usize = 1 << 20;

struct PemDetector {
    private: bool,
    pgp: bool,
    openvpn: bool,
    ssh2: bool,
}

impl PemDetector {
    /// End of the block whose header ends at `body_from`, or `None` when there is neither an
    /// END marker nor a key-sized body (a header mentioned in prose). The block never reaches
    /// past `stop` (the next BEGIN): every byte is read for one block only.
    fn block(&self, text: &str, body_from: usize, stop: usize, end_marker: &str) -> Option<usize> {
        let limit = super::floor_boundary(text, (body_from + MAX_BLOCK).min(stop).max(body_from));
        if let Some(i) = text[body_from..limit].find(end_marker) {
            return Some(body_from + i + end_marker.len());
        }
        // No END marker (truncated paste): cover the base64 body that follows, if there is
        // enough of it to be key material. Header lines (`Proc-Type: …`) and escaped
        // newlines (`\n` inside JSON) may sit in between.
        let body = &text[body_from..limit];
        let len = body
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || b"+/=\\:-,. \t\r\n\"".contains(b))
            .count();
        let b64 = body[..len]
            .bytes()
            .filter(|b| b.is_ascii_alphanumeric() || b"+/".contains(b))
            .count();
        (b64 >= 64).then(|| body_from + body[..len].trim_end().len())
    }
}

/// Start of each match of `marker`, and where the next one starts (or the text ends).
fn starts_and_stops(text: &str, marker: &str) -> Vec<(usize, usize)> {
    let starts: Vec<usize> = text.match_indices(marker).map(|(i, _)| i).collect();
    starts
        .iter()
        .enumerate()
        .map(|(k, &p)| (p, starts.get(k + 1).copied().unwrap_or(text.len())))
        .collect()
}

impl Detector for PemDetector {
    fn id(&self) -> &'static str {
        PEM_PRIVATE
    }

    fn rule_count(&self) -> usize {
        self.private as usize + self.pgp as usize + self.openvpn as usize + self.ssh2 as usize
    }

    fn detect(&self, text: &str, out: &mut Vec<Candidate>) {
        for (p, stop) in starts_and_stops(text, "-----BEGIN ") {
            let label_from = p + "-----BEGIN ".len();
            let head =
                &text[label_from..super::floor_boundary(text, (label_from + 64).min(text.len()))];
            let Some(close) = head.find("-----") else {
                continue;
            };
            let header = &head[..close];
            if !header
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b' ')
            {
                continue; // e.g. a regex like `-----BEGIN (?:[A-Z ]+ )?PRIVATE KEY-----`
            }
            let (id, label) = if header == "PGP PRIVATE KEY BLOCK" {
                (PEM_PGP, "PGP Private Key")
            } else if header.ends_with("PRIVATE KEY") {
                (PEM_PRIVATE, "Private Key")
            } else if header == "OpenVPN Static key V1" {
                (PEM_OPENVPN, "OpenVPN Static Key")
            } else {
                continue;
            };
            let enabled = match id {
                PEM_PGP => self.pgp,
                PEM_OPENVPN => self.openvpn,
                _ => self.private,
            };
            if !enabled {
                continue;
            }
            let body_from = label_from + close + 5;
            let end_marker = format!("-----END {header}-----");
            if body_from > stop {
                continue;
            }
            if let Some(end) = self.block(text, body_from, stop, &end_marker) {
                out.push(cand(p, end, id, label, 1.0, "Private key block"));
            }
        }
        if self.ssh2 {
            const BEGIN: &str = "---- BEGIN SSH2 ENCRYPTED PRIVATE KEY ----";
            const END: &str = "---- END SSH2 ENCRYPTED PRIVATE KEY ----";
            for (p, stop) in starts_and_stops(text, BEGIN) {
                if let Some(end) = self.block(text, p + BEGIN.len(), stop, END) {
                    out.push(cand(
                        p,
                        end,
                        PEM_SSH2,
                        "Private Key",
                        1.0,
                        "SSH2 private key block",
                    ));
                }
            }
        }
    }
}

struct PuttyDetector;

impl Detector for PuttyDetector {
    fn id(&self) -> &'static str {
        PUTTY
    }

    fn detect(&self, text: &str, out: &mut Vec<Candidate>) {
        // A file ends where the next one starts: every byte is read for one file only.
        for (p, stop) in starts_and_stops(text, "PuTTY-User-Key-File-") {
            let limit = super::floor_boundary(text, (p + 65536).min(stop));
            let region = &text[p..limit];
            let Some(pl) = region.find("Private-Lines:") else {
                continue;
            };
            // Private-Lines: N, then N body lines, then Private-MAC / Private-Hash.
            let after = &region[pl + "Private-Lines:".len()..];
            let count: usize = after
                .trim_start_matches([' ', '\t'])
                .bytes()
                .take_while(u8::is_ascii_digit)
                .fold(0usize, |n, d| {
                    n.saturating_mul(10).saturating_add((d - b'0') as usize)
                });
            let mut pos = p + pl;
            // End of the Private-Lines line.
            pos = line_end(text, pos, limit);
            for _ in 0..count.min(4096) {
                if pos >= limit {
                    break;
                }
                pos = line_end(text, pos + 1, limit);
            }
            let rest = &text[(pos + 1).min(limit)..limit];
            let rest = rest.trim_start_matches('\r');
            if rest.starts_with("Private-MAC:") || rest.starts_with("Private-Hash:") {
                let mac_start = limit - rest.len();
                pos = line_end(text, mac_start, limit);
            }
            let end = text[..pos].trim_end_matches('\r').len();
            out.push(cand(
                p,
                end,
                PUTTY,
                "PuTTY Private Key",
                1.0,
                "PuTTY private key file",
            ));
        }
    }
}

/// Byte offset of the `\n` ending the line that contains `from` (or `limit`).
fn line_end(text: &str, from: usize, limit: usize) -> usize {
    let from = from.min(limit);
    text[from..limit].find('\n').map_or(limit, |i| from + i)
}

/// The BIP-39 English words in an open-addressing table keyed by the packed word (a word is
/// looked up once per word of the text, so this has to be cheap): `(packed word, index)`,
/// `0` marks an empty slot.
fn wordlist() -> &'static [(u64, u16)] {
    static WORDS: OnceLock<Vec<(u64, u16)>> = OnceLock::new();
    WORDS.get_or_init(|| {
        let mut table = vec![(0u64, 0u16); WORD_SLOTS];
        for (i, w) in include_str!("bip39_english.txt").lines().enumerate() {
            let k = pack(w.trim().as_bytes());
            let mut slot = word_slot(k);
            while table[slot].0 != 0 {
                slot = (slot + 1) & (WORD_SLOTS - 1);
            }
            table[slot] = (k, i as u16);
        }
        table
    })
}

const WORD_SLOTS: usize = 8192;

fn word_slot(k: u64) -> usize {
    (k.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 51) as usize
}

/// The index of a BIP-39 word (any case), if it is one.
fn word_index(table: &[(u64, u16)], word: &[u8]) -> Option<u16> {
    let k = pack(word);
    let mut slot = word_slot(k);
    loop {
        let (key, idx) = table[slot];
        if key == k {
            return Some(idx);
        }
        if key == 0 {
            return None;
        }
        slot = (slot + 1) & (WORD_SLOTS - 1);
    }
}

/// Lowercase letters packed big-endian, zero padded (order-preserving for up to 8 bytes).
fn pack(word: &[u8]) -> u64 {
    let mut k = 0u64;
    for i in 0..8 {
        let b = word.get(i).map_or(0, u8::to_ascii_lowercase);
        k = (k << 8) | b as u64;
    }
    k
}

struct Bip39Detector;

/// Characters allowed between two seed words (spaces, commas, list numbering).
fn seed_gap_ok(gap: &str) -> bool {
    gap.len() <= 12
        && !gap.is_empty()
        && gap
            .bytes()
            .all(|b| b.is_ascii_whitespace() || b.is_ascii_digit() || b".,;:)(-\"'".contains(&b))
}

/// Seed words are drawn at random: a run in word-list order (the list itself) or one that
/// repeats words more than a few times (`allow-session-list`, `allow-session-add`, …) is
/// a list of words, not a phrase. Twelve random words come out sorted with odds of 1 in
/// 12!, and three repeats among 24 words have odds below 1 in 1000.
fn plausible_seed(idx: &[u16]) -> bool {
    if idx.windows(2).all(|w| w[0] < w[1]) {
        return false;
    }
    let mut d = idx.to_vec();
    d.sort_unstable();
    d.dedup();
    idx.len() - d.len() <= 2 + idx.len() / 24
}

impl Bip39Detector {
    fn flush(run: &[(u16, usize, usize)], out: &mut Vec<Candidate>) {
        let n = run.len();
        if n < 12 {
            return;
        }
        let mut found = false;
        let mut i = 0;
        'outer: while i + 12 <= n {
            for len in [24usize, 21, 18, 15, 12] {
                if i + len <= n {
                    let idx: Vec<u16> = run[i..i + len].iter().map(|w| w.0).collect();
                    if plausible_seed(&idx) && validate::bip39_checksum_ok(&idx) {
                        out.push(cand(
                            run[i].1,
                            run[i + len - 1].2,
                            BIP39,
                            "Recovery Phrase",
                            0.97,
                            "BIP-39 words with a valid checksum",
                        ));
                        found = true;
                        i += len;
                        continue 'outer;
                    }
                }
            }
            i += 1;
        }
        let all: Vec<u16> = run.iter().map(|w| w.0).collect();
        if !found && plausible_seed(&all) {
            // Paranoid only: a run of seed words whose checksum does not match.
            out.push(cand(
                run[0].1,
                run[n - 1].2,
                BIP39,
                "Recovery Phrase",
                0.4,
                "12+ BIP-39 words, checksum not valid",
            ));
        }
    }
}

impl Detector for Bip39Detector {
    fn id(&self) -> &'static str {
        BIP39
    }

    fn detect(&self, text: &str, out: &mut Vec<Candidate>) {
        let words = wordlist();
        let bytes = text.as_bytes();
        let mut run: Vec<(u16, usize, usize)> = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if !bytes[i].is_ascii_alphabetic() {
                i += 1;
                continue;
            }
            let s = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            let len = i - s;
            let idx = if (3..=8).contains(&len) {
                word_index(words, &bytes[s..i])
            } else {
                None
            };
            match idx {
                Some(ix) => {
                    if let Some(&(_, _, prev_end)) = run.last() {
                        if !seed_gap_ok(&text[prev_end..s]) {
                            Self::flush(&run, out);
                            run.clear();
                        }
                    }
                    run.push((ix, s, i));
                }
                None => {
                    Self::flush(&run, out);
                    run.clear();
                }
            }
        }
        Self::flush(&run, out);
    }
}
