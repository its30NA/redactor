//! Checksums and format validators. Pure functions on borrowed slices.

use sha2::{Digest, Sha256};

/// Number of character classes present among lowercase, uppercase and digits.
pub fn char_classes(s: &str) -> usize {
    let (mut l, mut u, mut d) = (false, false, false);
    for b in s.bytes() {
        l |= b.is_ascii_lowercase();
        u |= b.is_ascii_uppercase();
        d |= b.is_ascii_digit();
    }
    l as usize + u as usize + d as usize
}

// ---- CRC32 (IEEE 802.3, reflected, as in zlib) --------------------------------------------

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static CRC_TABLE: [u32; 256] = crc_table();

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c = CRC_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// `n` in base62, left-padded with `0` to `width` characters.
pub fn base62(mut n: u64, width: usize) -> String {
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(BASE62[(n % 62) as usize]);
        n /= 62;
    }
    while digits.len() < width {
        digits.push(b'0');
    }
    digits.reverse();
    String::from_utf8(digits).expect("ascii")
}

/// GitHub token checksum: the last 6 characters of the 36-character body are the base62
/// CRC32 of the first 30 (the published format of `ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`).
pub fn github_checksum_ok(body: &str) -> bool {
    if body.len() != 36 || !body.is_ascii() {
        return false;
    }
    let (random, check) = body.split_at(30);
    base62(crc32(random.as_bytes()) as u64, 6) == check
}

// ---- Luhn, IBAN ------------------------------------------------------------------------

/// Luhn check over the digits of `s` (other characters ignored).
pub fn luhn_ok(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() < 2 {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 {
                    x - 9
                } else {
                    x
                }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// IBAN mod-97 check (spaces ignored).
pub fn iban_ok(s: &str) -> bool {
    let compact: Vec<u8> = s.bytes().filter(|b| *b != b' ').collect();
    if compact.len() < 15 || compact.len() > 34 {
        return false;
    }
    let (head, tail) = compact.split_at(4);
    let mut rem: u32 = 0;
    for &b in tail.iter().chain(head) {
        let v = match b {
            b'0'..=b'9' => (b - b'0') as u32,
            b'A'..=b'Z' => (b - b'A') as u32 + 10,
            _ => return false,
        };
        rem = if v >= 10 {
            (rem * 100 + v) % 97
        } else {
            (rem * 10 + v) % 97
        };
    }
    rem == 1
}

// ---- BIP-39 ----------------------------------------------------------------------------

/// Validates the checksum of a BIP-39 mnemonic given as word indices (0..2048).
/// Valid lengths: 12, 15, 18, 21, 24 words.
pub fn bip39_checksum_ok(indices: &[u16]) -> bool {
    let n = indices.len();
    if !matches!(n, 12 | 15 | 18 | 21 | 24) {
        return false;
    }
    let total_bits = n * 11;
    let cs_bits = total_bits / 33;
    let ent_bits = total_bits - cs_bits;
    let mut bits = Vec::with_capacity(total_bits);
    for &i in indices {
        for k in (0..11).rev() {
            bits.push((i >> k) & 1 == 1);
        }
    }
    let mut entropy = vec![0u8; ent_bits / 8];
    for (j, bit) in bits[..ent_bits].iter().enumerate() {
        if *bit {
            entropy[j / 8] |= 0x80 >> (j % 8);
        }
    }
    let hash = Sha256::digest(&entropy);
    (0..cs_bits).all(|k| {
        let expected = (hash[k / 8] >> (7 - (k % 8))) & 1 == 1;
        bits[ent_bits + k] == expected
    })
}

// ---- Base64, JWT -----------------------------------------------------------------------

/// Decodes standard or URL-safe base64, padding optional. `None` on any other character.
pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut nbits = 0;
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((acc >> nbits) as u8);
            acc &= (1 << nbits) - 1;
        }
    }
    if nbits >= 6 {
        return None; // a single dangling character is not valid base64
    }
    Some(out)
}

/// A JWT header: base64url that decodes to a JSON object with an `alg` or `typ` key.
pub fn jwt_header_ok(segment: &str) -> bool {
    let Some(bytes) = b64_decode(segment) else {
        return false;
    };
    match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(serde_json::Value::Object(map)) => map.contains_key("alg") || map.contains_key("typ"),
        _ => false,
    }
}

// ---- Base58check -----------------------------------------------------------------------

const BASE58: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Decodes base58 (Bitcoin alphabet).
pub fn b58_decode(s: &str) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    for c in s.bytes() {
        let mut carry = BASE58.iter().position(|&x| x == c)? as u32;
        for b in out.iter_mut().rev() {
            carry += (*b as u32) * 58;
            *b = (carry & 0xFF) as u8;
            carry >>= 8;
        }
        while carry > 0 {
            out.insert(0, (carry & 0xFF) as u8);
            carry >>= 8;
        }
    }
    let zeros = s.bytes().take_while(|&c| c == b'1').count();
    let mut full = vec![0u8; zeros];
    full.extend(out);
    Some(full)
}

/// Base58check: returns the payload when the 4-byte double-SHA-256 checksum matches.
pub fn base58check(s: &str) -> Option<Vec<u8>> {
    let data = b58_decode(s)?;
    if data.len() < 5 {
        return None;
    }
    let (payload, check) = data.split_at(data.len() - 4);
    let hash = Sha256::digest(Sha256::digest(payload));
    (hash[..4] == *check).then(|| payload.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn base62_pads() {
        assert_eq!(base62(0, 6), "000000");
        assert_eq!(base62(61, 2), "0z");
        assert_eq!(base62(62, 1), "10");
    }

    #[test]
    fn luhn_vectors() {
        assert!(luhn_ok("4539 1488 0343 6467"));
        assert!(!luhn_ok("4539 1488 0343 6468"));
    }

    #[test]
    fn iban_vectors() {
        // The well-known documentation IBANs.
        assert!(iban_ok("DE89 3704 0044 0532 0130 00"));
        assert!(iban_ok("GB82WEST12345698765432"));
        assert!(!iban_ok("DE89 3704 0044 0532 0130 01"));
    }

    #[test]
    fn bip39_all_abandon_about() {
        // The standard test vector: "abandon" x11 + "about" (indices 0 and 3).
        let mut idx = vec![0u16; 11];
        idx.push(3);
        assert!(bip39_checksum_ok(&idx));
        idx[11] = 4;
        assert!(!bip39_checksum_ok(&idx));
    }

    #[test]
    fn b64_both_alphabets() {
        assert_eq!(b64_decode("aGk").unwrap(), b"hi");
        assert_eq!(b64_decode("aGk=").unwrap(), b"hi");
        assert_eq!(b64_decode("-_8").unwrap(), b64_decode("+/8").unwrap());
        assert!(b64_decode("a").is_none());
        assert!(b64_decode("a*b").is_none());
    }

    #[test]
    fn jwt_header_vectors() {
        assert!(jwt_header_ok("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
        assert!(!jwt_header_ok("eyJmb28iOiJiYXIifQ")); // {"foo":"bar"}
        assert!(!jwt_header_ok("eyJub3Rqc29u"));
    }

    #[test]
    fn base58check_vectors() {
        // Genesis block address (public, not a secret).
        let p = base58check("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa").unwrap();
        assert_eq!(p.len(), 21);
        assert_eq!(p[0], 0);
        assert!(base58check("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNb").is_none());
    }
}
