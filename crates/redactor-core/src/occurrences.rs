//! Every occurrence of many known values in a text, in linear time and memory.
//!
//! An Aho-Corasick automaton over all values found in a large input costs a lot to build:
//! hundreds of nanoseconds and tens of bytes per pattern byte, so 20 MiB of distinct values
//! took seconds and gigabytes. Values here are indexed by their first [`ANCHOR`] bytes
//! instead: one pass over the text reads each 6-byte window, a small bit filter rejects
//! almost all of them, and a window that passes is looked up in the index, whose values
//! (sorted) are matched by binary search.
//!
//! Verifying a match compares at most the value's length; on periodic text (`aaaa…`) and a
//! long periodic value that can add up, so the bytes compared beyond [`SHORT`] per probe
//! share a budget linear in the text. Past it, the values longer than [`SHORT`] are matched
//! by one Aho-Corasick pass of their own (a linear search; there are few such values).
//!
//! A few values (the common case) go into one small Aho-Corasick automaton: it builds in
//! no time and skips text with a prefilter, faster than reading every window.

use std::collections::HashMap;

use aho_corasick::{AhoCorasick, AhoCorasickKind, MatchKind};

/// Bytes of a value that key the index. Every value is at least this long.
pub(crate) const ANCHOR: usize = 6;
/// Values up to this length are always verified in place.
const SHORT: usize = 256;
/// Up to this many values, of at most [`SMALL_BYTES`] in all, use one automaton.
const SMALL_COUNT: usize = 1024;
const SMALL_BYTES: usize = 32 << 10;

/// The values to find (each at least [`ANCHOR`] bytes; shorter ones are ignored).
pub(crate) struct Occurrences<'v> {
    values: Vec<&'v [u8]>,
    /// Value indices, sorted by (anchor key, value bytes).
    order: Vec<u32>,
    /// Anchor key -> range of `order`.
    groups: HashMap<u64, (u32, u32)>,
    /// One bit per hashed anchor key.
    filter: Vec<u64>,
    shift: u32,
    longest: usize,
    /// For a few values: one automaton over them (pattern i is value `order[i]`).
    small: Option<AhoCorasick>,
}

fn key(b: &[u8]) -> u64 {
    let mut k = [0u8; 8];
    k[..ANCHOR].copy_from_slice(&b[..ANCHOR]);
    u64::from_le_bytes(k)
}

impl<'v> Occurrences<'v> {
    pub fn new(values: Vec<&'v [u8]>) -> Occurrences<'v> {
        Occurrences::build(values, true)
    }

    fn build(values: Vec<&'v [u8]>, allow_small: bool) -> Occurrences<'v> {
        let mut order: Vec<u32> = (0..values.len() as u32)
            .filter(|&i| values[i as usize].len() >= ANCHOR)
            .collect();
        order.sort_unstable_by(|&a, &b| {
            let (va, vb) = (values[a as usize], values[b as usize]);
            (key(va), va).cmp(&(key(vb), vb))
        });
        let bytes: usize = order.iter().map(|&i| values[i as usize].len()).sum();
        if allow_small && order.len() <= SMALL_COUNT && bytes <= SMALL_BYTES {
            // A noncontiguous NFA builds in linear time even for long, repetitive values.
            if let Ok(ac) = AhoCorasick::builder()
                .kind(Some(AhoCorasickKind::NoncontiguousNFA))
                .match_kind(MatchKind::Standard)
                .build(order.iter().map(|&i| values[i as usize]))
            {
                return Occurrences {
                    values,
                    order,
                    groups: HashMap::new(),
                    filter: Vec::new(),
                    shift: 0,
                    longest: 0,
                    small: Some(ac),
                };
            }
        }
        // 16+ bits per key keeps false positives of the filter rare.
        let bits = (order.len() * 16)
            .next_power_of_two()
            .clamp(1 << 12, 1 << 27);
        let shift = 64 - bits.trailing_zeros();
        let mut filter = vec![0u64; bits / 64];
        let mut groups: HashMap<u64, (u32, u32)> = HashMap::new();
        let mut longest = 0;
        let mut i = 0;
        while i < order.len() {
            let k = key(values[order[i] as usize]);
            let mut j = i;
            while j < order.len() && key(values[order[j] as usize]) == k {
                longest = longest.max(values[order[j] as usize].len());
                j += 1;
            }
            groups.insert(k, (i as u32, j as u32));
            let h = hash(k, shift);
            filter[h / 64] |= 1 << (h % 64);
            i = j;
        }
        Occurrences {
            values,
            order,
            groups,
            filter,
            shift,
            longest,
            small: None,
        }
    }

    /// Calls `f(start, end, value index)` for every occurrence of every value in `hay`,
    /// overlapping ones included. An occurrence may be reported twice; callers dedupe.
    pub fn find(&self, hay: &[u8], mut f: impl FnMut(usize, usize, usize)) {
        if self.order.is_empty() || hay.len() < ANCHOR {
            return;
        }
        if let Some(ac) = &self.small {
            for m in ac.find_overlapping_iter(hay) {
                f(
                    m.start(),
                    m.end(),
                    self.order[m.pattern().as_usize()] as usize,
                );
            }
            return;
        }
        let budget = 16 * hay.len() + (64 << 20);
        let mut spent = 0usize;
        let mut window = self.longest;
        let mut deferred = false;
        for p in 0..=hay.len() - ANCHOR {
            let k = key(&hay[p..]);
            let h = hash(k, self.shift);
            if self.filter[h / 64] & (1 << (h % 64)) == 0 {
                continue;
            }
            let Some(&(lo, hi)) = self.groups.get(&k) else {
                continue;
            };
            let s = &hay[p..(p + window).min(hay.len())];
            spent += self.prefixes(&self.order[lo as usize..hi as usize], s, |v| {
                f(p, p + self.values[v].len(), v)
            });
            if !deferred && spent > budget {
                deferred = true;
                window = SHORT.min(self.longest);
            }
        }
        if deferred {
            self.find_long(hay, f);
        }
    }

    /// Values in `group` (sorted) that are prefixes of `s`, longest first. Returns the bytes
    /// compared beyond [`SHORT`] per comparison.
    fn prefixes(&self, group: &[u32], mut s: &[u8], mut f: impl FnMut(usize)) -> usize {
        let mut spent = 0;
        let mut end = group.len();
        while end > 0 {
            // The largest value <= s: any value that is a prefix of s is <= s.
            let i = group[..end].partition_point(|&v| self.values[v as usize] <= s);
            if i == 0 {
                break;
            }
            let v = group[i - 1] as usize;
            let val = self.values[v];
            let l = lcp(val, s);
            spent += l.saturating_sub(SHORT);
            if l == val.len() {
                f(v);
                if l == 0 {
                    break;
                }
                s = &s[..l - 1];
            } else {
                // A longer prefix of s would sort after `val`: only prefixes of s[..l] remain.
                s = &s[..l];
            }
            end = i - 1;
            if s.len() < ANCHOR {
                break;
            }
        }
        spent
    }

    /// The values longer than [`SHORT`], by one linear Aho-Corasick pass.
    fn find_long(&self, hay: &[u8], mut f: impl FnMut(usize, usize, usize)) {
        let long: Vec<usize> = (0..self.values.len())
            .filter(|&i| self.values[i].len() > SHORT)
            .collect();
        if long.is_empty() {
            return;
        }
        // A noncontiguous NFA builds in linear time even for long, repetitive values.
        let Ok(ac) = AhoCorasick::builder()
            .kind(Some(AhoCorasickKind::NoncontiguousNFA))
            .match_kind(MatchKind::Standard)
            .build(long.iter().map(|&i| self.values[i]))
        else {
            return;
        };
        for m in ac.find_overlapping_iter(hay) {
            f(m.start(), m.end(), long[m.pattern().as_usize()]);
        }
    }
}

fn hash(k: u64, shift: u32) -> usize {
    (k.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> shift) as usize
}

fn lcp(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0;
    // Eight bytes at a time.
    while i + 8 <= n {
        let x = u64::from_le_bytes(a[i..i + 8].try_into().unwrap_or([0; 8]));
        let y = u64::from_le_bytes(b[i..i + 8].try_into().unwrap_or([0; 8]));
        if x != y {
            return i + ((x ^ y).trailing_zeros() / 8) as usize;
        }
        i += 8;
    }
    while i < n && a[i] == b[i] {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both ways of searching, which must agree.
    fn all(values: &[&str], hay: &str) -> Vec<(usize, usize, usize)> {
        let mut answers = Vec::new();
        for small in [false, true] {
            let occ = Occurrences::build(values.iter().map(|v| v.as_bytes()).collect(), small);
            assert_eq!(occ.small.is_some(), small);
            let mut out = Vec::new();
            occ.find(hay.as_bytes(), |s, e, v| out.push((s, e, v)));
            out.sort_unstable();
            out.dedup();
            answers.push(out);
        }
        assert_eq!(answers[0], answers[1]);
        answers.pop().unwrap_or_default()
    }

    /// The same answer as a naive search, overlapping and nested values included.
    #[test]
    fn matches_naive_search() {
        let values = [
            "abcdef",
            "abcdefgh",
            "abcdefxyz",
            "bcdefg",
            "zzzzzz",
            "zzzzzzz",
            "sk-ant-one-1",
            "sk-ant-two-2",
            "short",
        ];
        let hay = "xxabcdefghij abcdefxyz zzzzzzzzz sk-ant-two-2 sk-ant-one-1sk-ant-two-2";
        let mut naive = Vec::new();
        for (i, v) in values.iter().enumerate() {
            if v.len() < ANCHOR {
                continue;
            }
            for (s, _) in hay.match_indices(v) {
                naive.push((s, s + v.len(), i));
            }
            // Overlapping occurrences of a periodic value.
            let mut from = 0;
            while let Some(j) = hay[from..].find(v) {
                naive.push((from + j, from + j + v.len(), i));
                from += j + 1;
            }
        }
        naive.sort_unstable();
        naive.dedup();
        assert_eq!(all(&values, hay), naive);
    }

    #[test]
    fn long_periodic_value_on_periodic_text_stays_linear() {
        let value = format!("{}b", "a".repeat(5000));
        let hay = format!("{}{value}", "a".repeat(1 << 20));
        let found = all(&[&value], &hay);
        assert_eq!(found, vec![(1 << 20, (1 << 20) + value.len(), 0)]);
    }
}
