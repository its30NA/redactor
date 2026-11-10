//! Occurrences of many known values in a text, in linear time and memory.
//!
//! Values can nest (`abab…` of every length is a prefix of the next), so a text can hold
//! quadratically many occurrences, and listing them all took a minute for 649 KB. A search
//! reports instead, at each position, the longest value that starts there (or that ends
//! there, see below) and at most [`CHAIN`] shorter ones that start (end) there too, plus every
//! occurrence of the values marked `every` (short values whose copies must stand alone, so a
//! shorter one counts even inside a longer one; they have distinct lengths of 6 to 11
//! characters, so at most six end at one position). Unless more than [`CHAIN`] values nest at
//! one position, that is every occurrence; past it, every occurrence still lies inside a
//! reported one that starts or ends where it does, so a caller that drops a reported
//! occurrence (already covered, written out plainly) must be one that would drop what lies
//! inside it too.
//!
//! Values are indexed by their first [`ANCHOR`] bytes: one pass over the text reads each
//! 6-byte window, a small bit filter rejects almost all of them, and a window that passes is
//! looked up in the index. Its values are sorted, so one binary search finds the largest value
//! not above the text there; the longest value that is a prefix of the text is that value or
//! one of its prefixes (each value links to its longest proper prefix among the values).
//!
//! On periodic text and long values that costs up to the values' length per position, so the
//! work shares a budget linear in the text. Past it, an Aho-Corasick automaton over all values
//! takes over: it finds the longest value ending at each position in constant time (each
//! state knows it, and each value its longest proper suffix among the values).

use std::cell::{Cell, OnceCell};
use std::collections::HashMap;

/// Bytes of a value that key the index. Shorter values are never found.
pub(crate) const ANCHOR: usize = 6;
/// The work (bytes compared, plus a fixed cost per step) the index may do before the
/// automaton takes over: this much, plus [`WORK_PER_BYTE`] per byte searched.
const WORK_BASE: usize = 64 << 20;
const WORK_PER_BYTE: usize = 32;
/// The fixed cost of one probe or step, in bytes compared.
const STEP: usize = 16;
/// Shorter values reported at one position besides the longest (and the `every` ones).
const CHAIN: usize = 8;
/// No value, no state.
const NONE: u32 = u32::MAX;

/// The values to find (each at least [`ANCHOR`] bytes; shorter ones are ignored).
pub(crate) struct Occurrences<'v> {
    values: Vec<&'v [u8]>,
    /// Report every occurrence of these values, not only the longest at a position.
    every: Vec<bool>,
    /// Value indices without duplicates, sorted by (anchor key, value bytes).
    order: Vec<u32>,
    /// Anchor key -> range of `order`.
    groups: HashMap<u64, (u32, u32)>,
    /// One bit per hashed anchor key.
    filter: Vec<u64>,
    shift: u32,
    longest: usize,
    /// Per value: the longest value that is a proper prefix of it, and the longest `every`
    /// value that is a prefix of it or itself (`NONE` for none).
    parent: Vec<u32>,
    prefix_every: Vec<u32>,
    /// Work allowed and done so far by the index, over every search.
    allowed: Cell<usize>,
    spent: Cell<usize>,
    per_byte: usize,
    /// Built once the index has used up its work.
    automaton: OnceCell<Automaton>,
}

/// The longest value that starts at a position of the text (the index), or that ends at one
/// (the automaton, which is passed along).
enum Head<'a> {
    Start(usize, u32),
    End(usize, u32, &'a Automaton),
}

/// Which values occur in a text: [`Occurrences::mark`] sets them, [`Marks::has`] tells.
pub(crate) struct Marks {
    stamp: u32,
    /// Per value, the stamp of the last text it was found in, through the index (with its
    /// prefixes) or the automaton (with its suffixes).
    prefix: Vec<u32>,
    suffix: Vec<u32>,
}

impl Marks {
    pub fn has(&self, value: usize) -> bool {
        self.stamp != 0 && (self.prefix[value] == self.stamp || self.suffix[value] == self.stamp)
    }
}

fn key(b: &[u8]) -> u64 {
    let mut k = [0u8; 8];
    k[..ANCHOR].copy_from_slice(&b[..ANCHOR]);
    u64::from_le_bytes(k)
}

impl<'v> Occurrences<'v> {
    /// `every[i]` asks for every occurrence of value `i`.
    pub fn new(values: Vec<&'v [u8]>, every: Vec<bool>) -> Occurrences<'v> {
        debug_assert_eq!(values.len(), every.len());
        let mut order: Vec<u32> = (0..values.len() as u32)
            .filter(|&i| values[i as usize].len() >= ANCHOR)
            .collect();
        order.sort_unstable_by(|&a, &b| {
            let (va, vb) = (values[a as usize], values[b as usize]);
            (key(va), va, a).cmp(&(key(vb), vb, b))
        });
        // A value given twice is found once, as the first of its copies.
        order.dedup_by(|later, first| values[*later as usize] == values[*first as usize]);

        // 16+ bits per key keeps false positives of the filter rare.
        let bits = (order.len() * 16)
            .next_power_of_two()
            .clamp(1 << 12, 1 << 27);
        let shift = 64 - bits.trailing_zeros();
        let mut filter = vec![0u64; bits / 64];
        let mut groups: HashMap<u64, (u32, u32)> = HashMap::new();
        let mut parent = vec![NONE; values.len()];
        let mut prefix_every = vec![NONE; values.len()];
        let mut longest = 0;
        // The values that are prefixes of the current one, shortest first.
        let mut chain: Vec<u32> = Vec::new();
        let mut i = 0;
        while i < order.len() {
            let k = key(values[order[i] as usize]);
            let mut j = i;
            chain.clear();
            while j < order.len() && key(values[order[j] as usize]) == k {
                let v = order[j] as usize;
                // In sorted order a value's prefixes come before it, and every value between
                // a prefix and the value extends that prefix.
                while chain
                    .last()
                    .is_some_and(|&c| !values[v].starts_with(values[c as usize]))
                {
                    chain.pop();
                }
                let up = chain.last().copied().unwrap_or(NONE);
                parent[v] = up;
                prefix_every[v] = if every[v] {
                    v as u32
                } else if up != NONE {
                    prefix_every[up as usize]
                } else {
                    NONE
                };
                chain.push(v as u32);
                longest = longest.max(values[v].len());
                j += 1;
            }
            groups.insert(k, (i as u32, j as u32));
            let h = hash(k, shift);
            filter[h / 64] |= 1 << (h % 64);
            i = j;
        }
        Occurrences {
            values,
            every,
            order,
            groups,
            filter,
            shift,
            longest,
            parent,
            prefix_every,
            allowed: Cell::new(WORK_BASE),
            spent: Cell::new(0),
            per_byte: WORK_PER_BYTE,
            automaton: OnceCell::new(),
        }
    }

    fn len(&self, v: u32) -> usize {
        self.values[v as usize].len()
    }

    /// Calls `f(start, end, value index)` for the longest value that starts (or ends) at each
    /// position of `hay` and up to [`CHAIN`] shorter ones starting (ending) there, and for
    /// every occurrence of an `every` value: each occurrence lies inside a reported one that
    /// starts or ends where it does.
    pub fn find(&self, hay: &[u8], mut f: impl FnMut(usize, usize, usize)) {
        self.heads(hay, |head| match head {
            Head::Start(p, v) => {
                f(p, p + self.len(v), v as usize);
                let mut x = self.parent[v as usize];
                for _ in 0..CHAIN {
                    if x == NONE {
                        return;
                    }
                    f(p, p + self.len(x), x as usize);
                    x = self.parent[x as usize];
                }
                x = if x == NONE {
                    NONE
                } else {
                    self.prefix_every[x as usize]
                };
                while x != NONE {
                    f(p, p + self.len(x), x as usize);
                    x = self.shorter_every(x);
                }
            }
            Head::End(e, v, a) => {
                f(e - self.len(v), e, v as usize);
                let mut x = a.suffix[v as usize];
                for _ in 0..CHAIN {
                    if x == NONE {
                        return;
                    }
                    f(e - self.len(x), e, x as usize);
                    x = a.suffix[x as usize];
                }
                x = if x == NONE {
                    NONE
                } else {
                    a.suffix_every[x as usize]
                };
                while x != NONE {
                    f(e - self.len(x), e, x as usize);
                    x = a.shorter_every(x);
                }
            }
        });
    }

    /// The longest `every` value that is a proper prefix of value `v`.
    fn shorter_every(&self, v: u32) -> u32 {
        match self.parent[v as usize] {
            NONE => NONE,
            p => self.prefix_every[p as usize],
        }
    }

    /// Marks for [`Occurrences::mark`].
    pub fn marks(&self) -> Marks {
        Marks {
            stamp: 0,
            prefix: vec![0; self.values.len()],
            suffix: vec![0; self.values.len()],
        }
    }

    /// Forgets what `marks` held and marks every value that occurs in `hay`.
    pub fn mark(&self, hay: &[u8], marks: &mut Marks) {
        marks.stamp += 1;
        let stamp = marks.stamp;
        // A marked value has its prefixes (or suffixes) marked too: a walk stops at one.
        self.heads(hay, |head| match head {
            Head::Start(_, mut v) => {
                while v != NONE && marks.prefix[v as usize] != stamp {
                    marks.prefix[v as usize] = stamp;
                    v = self.parent[v as usize];
                }
            }
            Head::End(_, mut v, a) => {
                while v != NONE && marks.suffix[v as usize] != stamp {
                    marks.suffix[v as usize] = stamp;
                    v = a.suffix[v as usize];
                }
            }
        });
    }

    /// The longest value at each position of `hay`: from the index while its work lasts,
    /// then from the automaton.
    fn heads(&self, hay: &[u8], mut head: impl FnMut(Head<'_>)) {
        if self.order.is_empty() || hay.len() < ANCHOR {
            return;
        }
        let allowed = self
            .allowed
            .get()
            .saturating_add(self.per_byte.saturating_mul(hay.len()));
        self.allowed.set(allowed);
        if let Some(a) = self.automaton.get() {
            a.ends(hay, |e, v| head(Head::End(e, v, a)));
            return;
        }
        for p in 0..=hay.len() - ANCHOR {
            let k = key(&hay[p..]);
            let h = hash(k, self.shift);
            if self.filter[h / 64] & (1 << (h % 64)) == 0 {
                continue;
            }
            let Some(&(lo, hi)) = self.groups.get(&k) else {
                continue;
            };
            let s = &hay[p..(p + self.longest).min(hay.len())];
            let (v, work) = self.longest_prefix(&self.order[lo as usize..hi as usize], s);
            if v != NONE {
                head(Head::Start(p, v));
            }
            let spent = self.spent.get() + work;
            self.spent.set(spent);
            if spent > allowed {
                // The positions after `p` go to the automaton.
                let a = self
                    .automaton
                    .get_or_init(|| Automaton::new(&self.values, &self.every, &self.order));
                a.ends(&hay[p + 1..], |e, v| head(Head::End(p + 1 + e, v, a)));
                return;
            }
        }
    }

    /// The longest value in `group` (sorted, one anchor key) that is a prefix of `s`, and the
    /// work it took.
    fn longest_prefix(&self, group: &[u32], s: &[u8]) -> (u32, usize) {
        // The largest value not above `s`, and how many bytes it shares with `s`.
        let (mut lo, mut hi) = (0, group.len());
        let mut shared = 0;
        let mut work = 0;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let v = self.values[group[mid] as usize];
            let l = lcp(v, s);
            work += l + STEP;
            if l == v.len() || (l < s.len() && v[l] < s[l]) {
                lo = mid + 1;
                shared = l;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return (NONE, work);
        }
        // A value that is a prefix of `s` is not above it, so not above the value found, and
        // so a prefix of that one too (everything between a string and its extension extends
        // it): the longest prefix of the value found that is no longer than `shared`.
        let mut v = group[lo - 1];
        while v != NONE && self.len(v) > shared {
            v = self.parent[v as usize];
            work += STEP;
        }
        (v, work)
    }

    /// Makes the automaton do every search from now on (tests).
    #[cfg(test)]
    fn automaton_now(self) -> Self {
        self.automaton
            .get_or_init(|| Automaton::new(&self.values, &self.every, &self.order));
        self
    }

    /// Lets the index do `work` and no more (tests).
    #[cfg(test)]
    fn work(mut self, work: usize) -> Self {
        self.allowed.set(work);
        self.per_byte = 0;
        self
    }
}

/// An Aho-Corasick automaton over the values. A state is a prefix of a value; its failure
/// link the longest proper suffix of it that is one too; and each state knows the longest
/// value that ends there, so a search reports it without listing the shorter ones.
struct Automaton {
    /// The children of state `s` are the states `first[s]..first[s + 1]` (breadth-first
    /// order), the bytes leading to them in `label`, sorted.
    first: Vec<u32>,
    label: Vec<u8>,
    fail: Vec<u32>,
    /// The longest value that is a suffix of the state's text.
    out: Vec<u32>,
    /// The children of the root by byte.
    root: Box<[u32; 256]>,
    /// Per value: the longest value that is a proper suffix of it, and the longest `every`
    /// value that is a suffix of it or itself.
    suffix: Vec<u32>,
    suffix_every: Vec<u32>,
}

impl Automaton {
    /// From `order`: value indices, no two values equal, each at least [`ANCHOR`] bytes.
    fn new(values: &[&[u8]], every: &[bool], order: &[u32]) -> Automaton {
        let mut sorted = order.to_vec();
        sorted.sort_unstable_by(|&a, &b| values[a as usize].cmp(values[b as usize]));
        let val = |i: usize| values[sorted[i] as usize];
        // The trie, a level at a time: a state at depth `d` is the run of sorted values that
        // share its first `d` bytes, so its children are runs too, in order.
        let mut first: Vec<u32> = Vec::new();
        let mut label: Vec<u8> = vec![0];
        let mut term: Vec<u32> = vec![NONE];
        let mut level: Vec<(usize, usize)> = vec![(0, sorted.len())];
        let mut depth = 0;
        while !level.is_empty() {
            let mut next = Vec::new();
            for &(lo, hi) in &level {
                let state = first.len();
                first.push(label.len() as u32);
                let mut i = lo;
                // The value that ends here sorts first.
                if i < hi && val(i).len() == depth {
                    term[state] = sorted[i];
                    i += 1;
                }
                while i < hi {
                    let c = val(i)[depth];
                    let mut j = i + 1;
                    while j < hi && val(j)[depth] == c {
                        j += 1;
                    }
                    label.push(c);
                    term.push(NONE);
                    next.push((i, j));
                    i = j;
                }
            }
            level = next;
            depth += 1;
        }
        first.push(label.len() as u32);
        let states = label.len();
        let mut root = Box::new([NONE; 256]);
        for t in first[0]..first[1] {
            root[label[t as usize] as usize] = t;
        }
        let mut a = Automaton {
            first,
            label,
            fail: vec![0; states],
            out: vec![NONE; states],
            root,
            suffix: vec![NONE; values.len()],
            suffix_every: vec![NONE; values.len()],
        };
        // Breadth-first: a failure link points to a shallower state, which is done.
        for s in 0..states {
            let children = a.first[s] as usize..a.first[s + 1] as usize;
            for (t, &v) in children.clone().zip(&term[children]) {
                let f = if s == 0 {
                    0
                } else {
                    a.step(a.fail[s], a.label[t])
                };
                a.fail[t] = f;
                let below = a.out[f as usize];
                if v == NONE {
                    a.out[t] = below;
                    continue;
                }
                a.out[t] = v;
                a.suffix[v as usize] = below;
                a.suffix_every[v as usize] = if every[v as usize] {
                    v
                } else if below != NONE {
                    a.suffix_every[below as usize]
                } else {
                    NONE
                };
            }
        }
        a
    }

    fn child(&self, s: u32, c: u8) -> u32 {
        if s == 0 {
            return self.root[c as usize];
        }
        let (lo, hi) = (
            self.first[s as usize] as usize,
            self.first[s as usize + 1] as usize,
        );
        match self.label[lo..hi].binary_search(&c) {
            Ok(k) => (lo + k) as u32,
            Err(_) => NONE,
        }
    }

    fn step(&self, mut s: u32, c: u8) -> u32 {
        loop {
            let t = self.child(s, c);
            if t != NONE {
                return t;
            }
            if s == 0 {
                return 0;
            }
            s = self.fail[s as usize];
        }
    }

    /// Calls `f(end, value)` for the longest value ending at each position of `hay`.
    fn ends(&self, hay: &[u8], mut f: impl FnMut(usize, u32)) {
        let mut s = 0;
        let mut i = 0;
        while i < hay.len() {
            if s == 0 {
                // Skip what starts no value.
                while i < hay.len() && self.root[hay[i] as usize] == NONE {
                    i += 1;
                }
                if i == hay.len() {
                    break;
                }
            }
            s = self.step(s, hay[i]);
            i += 1;
            let v = self.out[s as usize];
            if v != NONE {
                f(i, v);
            }
        }
    }

    /// The longest `every` value that is a proper suffix of value `v`.
    fn shorter_every(&self, v: u32) -> u32 {
        match self.suffix[v as usize] {
            NONE => NONE,
            s => self.suffix_every[s as usize],
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

    type Hit = (usize, usize, usize);

    /// The ways of searching: the index alone, the automaton alone, and the index handing
    /// over to the automaton after a little work.
    fn modes<'v>(values: &[&'v [u8]], every: &[bool]) -> Vec<Occurrences<'v>> {
        let make = || Occurrences::new(values.to_vec(), every.to_vec());
        vec![
            make(),
            make().automaton_now(),
            make().work(0),
            make().work(200),
            make().work(2_000),
        ]
    }

    fn naive(values: &[&[u8]], hay: &[u8]) -> Vec<Hit> {
        let mut out = Vec::new();
        for (i, v) in values.iter().enumerate() {
            if v.len() < ANCHOR || v.len() > hay.len() {
                continue;
            }
            // A value given twice is found as its first copy.
            if values[..i].contains(v) {
                continue;
            }
            for s in 0..=hay.len() - v.len() {
                if hay[s..].starts_with(v) {
                    out.push((s, s + v.len(), i));
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// Checks one search against the naive one: every hit is an occurrence, every occurrence
    /// lies in a hit that starts or ends where it does, every occurrence of an `every` value
    /// is a hit, and `mark` marks exactly the values that occur. Returns the hits.
    fn check(occ: &Occurrences, values: &[&[u8]], every: &[bool], hay: &[u8]) -> Vec<Hit> {
        let all = naive(values, hay);
        let mut got = Vec::new();
        occ.find(hay, |s, e, v| got.push((s, e, v)));
        got.sort_unstable();
        let n = got.len();
        got.dedup();
        assert_eq!(got.len(), n, "reported twice");
        for h in &got {
            assert!(all.binary_search(h).is_ok(), "not an occurrence: {h:?}");
        }
        for &(s, e, v) in &all {
            assert!(
                got.iter()
                    .any(|&(gs, ge, _)| gs <= s && e <= ge && (gs == s || ge == e)),
                "not inside a hit: {:?}",
                (s, e, v)
            );
            if every[v] {
                assert!(got.contains(&(s, e, v)), "every: {:?}", (s, e, v));
            }
        }
        // With no more values than one position can report, every occurrence is reported.
        if values.len() <= CHAIN + 1 {
            assert_eq!(got, all, "with {} values", values.len());
        }
        let mut marks = occ.marks();
        occ.mark(b"unrelated text", &mut marks);
        occ.mark(hay, &mut marks);
        for (v, value) in values.iter().enumerate() {
            assert_eq!(
                marks.has(v),
                all.iter().any(|h| h.2 == v),
                "mark {:?}",
                String::from_utf8_lossy(value)
            );
        }
        got
    }

    #[test]
    fn matches_naive_search() {
        let values: Vec<&[u8]> = vec![
            b"abcdef",
            b"abcdefgh",
            b"abcdefxyz",
            b"bcdefg",
            b"zzzzzz",
            b"zzzzzzz",
            b"sk-ant-one-1",
            b"sk-ant-two-2",
            b"short",
            b"zzzzzz",
        ];
        let hay = b"xxabcdefghij abcdefxyz zzzzzzzzz sk-ant-two-2 sk-ant-one-1sk-ant-two-2";
        for every in [vec![false; values.len()], vec![true; values.len()]] {
            for occ in modes(&values, &every) {
                check(&occ, &values, &every, hay);
            }
        }
    }

    /// Random values and texts over two and three letters, so values nest and overlap.
    #[test]
    fn matches_naive_search_on_random_text() {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = |n: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % n as u64) as usize
        };
        for round in 0..300 {
            let letters: &[u8] = if round % 2 == 0 { b"ab" } else { b"abc" };
            let owned: Vec<Vec<u8>> = (0..1 + next(12))
                .map(|_| {
                    (0..4 + next(12))
                        .map(|_| letters[next(letters.len())])
                        .collect()
                })
                .collect();
            let values: Vec<&[u8]> = owned.iter().map(Vec::as_slice).collect();
            let every: Vec<bool> = values.iter().map(|_| next(3) == 0).collect();
            let hay: Vec<u8> = (0..next(300))
                .map(|_| letters[next(letters.len())])
                .collect();
            for occ in modes(&values, &every) {
                check(&occ, &values, &every, &hay);
            }
        }
    }

    /// Nested values (`ab` repeated, each a prefix of the next) on the text they were found
    /// in: about one report per position, not one per value per position (audit 2026-11-10,
    /// finding 3: `password=abab…` lines took cubic time).
    #[test]
    fn nested_values_are_reported_about_once_per_position() {
        let values: Vec<String> = (6..=200).map(|k| "ab".repeat(k)).collect();
        let text: String = values.iter().map(|v| format!("password={v}\n")).collect();
        let bytes: Vec<&[u8]> = values.iter().map(|v| v.as_bytes()).collect();
        for occ in modes(&bytes, &vec![false; bytes.len()]) {
            let mut reports = 0usize;
            occ.find(text.as_bytes(), |_, _, _| reports += 1);
            assert!(
                reports <= (CHAIN + 1) * text.len(),
                "{reports} reports for {} bytes",
                text.len()
            );
        }
    }

    /// The index stops after its work and hands over: nested values on 2 MB of periodic text
    /// cost a bounded amount of index work.
    #[test]
    fn index_work_is_bounded() {
        let values: Vec<String> = (6..=400).map(|k| "ab".repeat(k)).collect();
        let bytes: Vec<&[u8]> = values.iter().map(|v| v.as_bytes()).collect();
        let occ = Occurrences::new(bytes, vec![false; values.len()]);
        let hay = "ab".repeat(1 << 20);
        let mut reports = 0usize;
        occ.find(hay.as_bytes(), |_, _, _| reports += 1);
        assert!(occ.automaton.get().is_some());
        assert!(occ.spent.get() <= WORK_BASE + WORK_PER_BYTE * hay.len() + (64 << 10));
        assert!(reports <= (CHAIN + 1) * hay.len());
    }

    #[test]
    fn long_periodic_value_on_periodic_text_stays_linear() {
        let value = format!("{}b", "a".repeat(5000));
        let hay = format!("{}{value}", "a".repeat(1 << 20));
        let values: Vec<&[u8]> = vec![value.as_bytes()];
        for occ in modes(&values, &[false]) {
            let mut got = Vec::new();
            occ.find(hay.as_bytes(), |s, e, v| got.push((s, e, v)));
            assert_eq!(got, vec![(1 << 20, (1 << 20) + value.len(), 0)]);
        }
    }
}
