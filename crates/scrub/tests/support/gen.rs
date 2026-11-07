//! Seeded generator for format-valid FAKE tokens (a trimmed copy of the engine's test support).
//!
//! No realistic token is ever written into the repository: tests build them at run time from
//! a prefix plus a seeded random body, so secret scanners and push protection stay quiet.
#![allow(dead_code)]

pub const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
pub const HEX: &[u8] = b"0123456789abcdef";
pub const DIGITS: &[u8] = b"0123456789";

/// SplitMix64: tiny, seeded, good enough for fake tokens.
pub struct Gen(u64);

impl Gen {
    pub fn new(seed: u64) -> Gen {
        Gen(seed)
    }

    /// A seed derived from a name (FNV-1a), so each test gets its own stable stream.
    pub fn named(name: &str) -> Gen {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in name.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        Gen(h)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next_u64() as u8).collect()
    }

    /// `n` characters from `alphabet`, regenerated until the classes in the alphabet that
    /// matter for "random-looking" checks (lower/upper/digit) are all present (n >= 8).
    pub fn from(&mut self, alphabet: &[u8], n: usize) -> String {
        let wanted = classes(alphabet);
        loop {
            let s: String = (0..n)
                .map(|_| alphabet[self.below(alphabet.len())] as char)
                .collect();
            if n < 8 || classes(s.as_bytes()) == wanted {
                return s;
            }
        }
    }

    pub fn alnum(&mut self, n: usize) -> String {
        self.from(BASE62, n)
    }

    pub fn hex(&mut self, n: usize) -> String {
        self.from(HEX, n)
    }

    pub fn digits(&mut self, n: usize) -> String {
        self.from(DIGITS, n)
    }

    /// GitHub-style token: prefix + 30 random base62 + 6-char base62 CRC32 of those 30.
    pub fn github(&mut self, prefix: &str) -> String {
        let random = self.alnum(30);
        format!(
            "{prefix}{random}{}",
            base62(crc32(random.as_bytes()) as u64, 6)
        )
    }

    /// A Luhn-valid card number with the given prefix and length.
    pub fn card(&mut self, prefix: &str, len: usize) -> String {
        let mut s = prefix.to_string();
        while s.len() < len - 1 {
            s.push((b'0' + self.below(10) as u8) as char);
        }
        for d in 0..10u8 {
            let candidate = format!("{s}{d}");
            if luhn(&candidate) {
                return candidate;
            }
        }
        unreachable!("one check digit always fits")
    }
}

fn classes(s: &[u8]) -> u8 {
    let mut c = 0;
    if s.iter().any(u8::is_ascii_lowercase) {
        c |= 1;
    }
    if s.iter().any(u8::is_ascii_uppercase) {
        c |= 2;
    }
    if s.iter().any(u8::is_ascii_digit) {
        c |= 4;
    }
    c
}

/// Bitwise CRC32 (IEEE), deliberately not table-driven like the engine's.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

pub fn base62(mut n: u64, width: usize) -> String {
    let mut out = Vec::new();
    while n > 0 {
        out.push(BASE62[(n % 62) as usize]);
        n /= 62;
    }
    while out.len() < width {
        out.push(b'0');
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

pub fn luhn(s: &str) -> bool {
    let sum: u32 = s
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, b)| {
            let d = (b - b'0') as u32;
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
