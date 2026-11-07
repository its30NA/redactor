//! Token features. Mirrors `features()` in `tools/classifier/train.py` exactly;
//! the parity test in `mod.rs` checks both against each other.

use super::model::{N_FEATURES, TRI_NONE, TRI_SCALE};

/// Longer tokens are scored on their first characters.
pub(crate) const MAX_CHARS: usize = 256;

const VOWELS: &[u8] = b"aeiou";
const WALK_ROWS: &[&[u8]] = &[
    b"1234567890",
    b"qwertyuiop",
    b"asdfghjkl",
    b"zxcvbnm",
    b"abcdefghijklmnopqrstuvwxyz",
];

/// 0 lower, 1 upper, 2 digit, 3 anything else (ASCII classes only).
fn class(c: char) -> usize {
    match c {
        'a'..='z' => 0,
        'A'..='Z' => 1,
        '0'..='9' => 2,
        _ => 3,
    }
}

/// a-z (either case) to 0..26, everything else to the boundary symbol 26.
fn tri_index(c: char) -> usize {
    match c {
        'a'..='z' => c as usize - 'a' as usize,
        'A'..='Z' => c as usize - 'A' as usize,
        _ => 26,
    }
}

fn is_b64(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '-')
}

fn is_b58(c: char) -> bool {
    c.is_ascii_alphanumeric() && !matches!(c, '0' | 'O' | 'I' | 'l')
}

fn is_uuid(chars: &[char]) -> bool {
    chars.len() == 36
        && chars.iter().enumerate().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                *c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

/// The feature vector of `token` (non-empty), in `model.rs` weight order.
pub(crate) fn features(token: &str, tri: &[u8]) -> [f64; N_FEATURES] {
    let chars: Vec<char> = token.chars().take(MAX_CHARS).collect();
    let n = chars.len();
    let nf = n as f64;
    let cls: Vec<usize> = chars.iter().map(|&c| class(c)).collect();
    let mut counts = [0usize; 4];
    for &k in &cls {
        counts[k] += 1;
    }
    let present = counts.map(|c| c > 0);
    let classes = present.iter().filter(|p| **p).count() as f64;

    // Shannon entropy, summed in code-point order (as the training script does).
    let mut sorted = chars.clone();
    sorted.sort_unstable();
    let mut entropy = 0.0f64;
    let mut i = 0;
    while i < sorted.len() {
        let mut j = i;
        while j < sorted.len() && sorted[j] == sorted[i] {
            j += 1;
        }
        let p = (j - i) as f64 / nf;
        entropy -= p * p.log2();
        i = j;
    }
    let alphabet = 26 * present[0] as usize
        + 26 * present[1] as usize
        + 10 * present[2] as usize
        + 32 * present[3] as usize;
    let cap = n.min(alphabet);
    let entropy_rel = if cap > 1 {
        entropy / (cap as f64).log2()
    } else {
        0.0
    };

    let transitions = cls.windows(2).filter(|w| w[0] != w[1]).count();
    let trans = if n > 1 {
        transitions as f64 / (nf - 1.0)
    } else {
        0.0
    };
    let mut longest = 1usize;
    let mut run = 1usize;
    for w in cls.windows(2) {
        run = if w[0] == w[1] { run + 1 } else { 1 };
        longest = longest.max(run);
    }
    let longest_run = longest as f64 / nf;

    let letters = counts[0] + counts[1];
    let vowels = chars
        .iter()
        .zip(&cls)
        .filter(|(c, k)| **k < 2 && VOWELS.contains(&(c.to_ascii_lowercase() as u8)))
        .count();
    let vowel_ratio = if letters > 0 {
        vowels as f64 / letters as f64
    } else {
        0.0
    };

    // Trigram log-likelihood over letters; the context starts as two boundaries.
    let mut logps: Vec<f64> = Vec::with_capacity(n);
    let (mut a, mut b) = (26usize, 26usize);
    for &c in &chars {
        let c = tri_index(c);
        if c < 26 {
            logps.push(tri[(a * 27 + b) * 27 + c] as i8 as f64 / TRI_SCALE);
        }
        a = b;
        b = c;
    }
    let (tri_avg, tri_min3) = if logps.is_empty() {
        (TRI_NONE, TRI_NONE)
    } else {
        let avg = logps.iter().sum::<f64>() / logps.len() as f64;
        let min3 = if logps.len() >= 3 {
            logps
                .windows(3)
                .map(|w| (w[0] + w[1] + w[2]) / 3.0)
                .fold(f64::INFINITY, f64::min)
        } else {
            avg
        };
        (avg, min3)
    };

    let repeat_ratio = if n > 1 {
        let mut bigrams: Vec<(char, char)> = chars.windows(2).map(|w| (w[0], w[1])).collect();
        let total = bigrams.len();
        bigrams.sort_unstable();
        bigrams.dedup();
        1.0 - bigrams.len() as f64 / total as f64
    } else {
        0.0
    };

    // ASCII lowercase, as bytes; non-ASCII characters never take part in a walk.
    let low: Vec<u8> = chars
        .iter()
        .map(|c| {
            if c.is_ascii() {
                c.to_ascii_lowercase() as u8
            } else {
                0
            }
        })
        .collect();
    let walk = keyboard_walk(&low);

    let all = |f: fn(char) -> bool| chars.iter().all(|&c| f(c));
    let mixed = present[0] && present[1] && present[2];
    let hex_shape = n >= 8 && all(|c| c.is_ascii_hexdigit());
    let b64_shape = n >= 16 && mixed && all(is_b64);
    let b58_shape = n >= 16 && mixed && all(is_b58);
    let uuid_shape = is_uuid(&chars);

    let ln_len = nf.ln();
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    [
        ln_len,
        counts[0] as f64 / nf,
        counts[1] as f64 / nf,
        counts[2] as f64 / nf,
        counts[3] as f64 / nf,
        classes,
        entropy,
        entropy_rel,
        trans,
        longest_run,
        vowel_ratio,
        tri_avg,
        tri_min3,
        repeat_ratio,
        flag(walk),
        flag(hex_shape),
        flag(b64_shape),
        flag(b58_shape),
        flag(uuid_shape),
        ln_len * entropy_rel,
        ln_len * tri_avg,
        classes * trans,
    ]
}

/// Four consecutive keys of a keyboard row or the alphabet, either direction.
fn keyboard_walk(low: &[u8]) -> bool {
    if low.len() < 4 {
        return false;
    }
    let next = next_table();
    // `next[a][b]` has bit r set when `b` follows `a` in row r.
    let step = |a: u8, b: u8| -> u8 {
        if a < 128 && b < 128 {
            next[a as usize][b as usize]
        } else {
            0
        }
    };
    low.windows(4).any(|w| {
        step(w[0], w[1]) & step(w[1], w[2]) & step(w[2], w[3]) != 0
            || step(w[1], w[0]) & step(w[2], w[1]) & step(w[3], w[2]) != 0
    })
}

fn next_table() -> &'static [[u8; 128]; 128] {
    static TABLE: std::sync::OnceLock<Box<[[u8; 128]; 128]>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = Box::new([[0u8; 128]; 128]);
        for (r, row) in WALK_ROWS.iter().enumerate() {
            for w in row.windows(2) {
                t[w[0] as usize][w[1] as usize] |= 1 << r;
            }
        }
        t
    })
}
