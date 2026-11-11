//! Propagation: once a value is known to be secret, every further exact
//! occurrence of it is redacted too, with the same label (and so the same placeholder).
//!
//! `DB_PASSWORD=Hunter2-Fake-9x` … `login failed for Hunter2-Fake-9x`: the second occurrence
//! has no key next to it, but it is the same secret.
//!
//! Encoded copies count too: the value in base64 (inside a longer blob as well), hex,
//! percent-encoded or JSON-escaped, up to two encodings deep. Such a copy is redacted as the
//! value itself (one placeholder for both); see [`crate::encoded`].

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::encoded::{decode, Decoded};
use crate::finding::{Candidate, Source};
use crate::normalize::{normalize_value, Shadow};
use crate::occurrences::{Marks, Occurrences};
use crate::terms;

/// Shorter values are never propagated.
pub(crate) const MIN_CHARS: usize = 6;
/// Below this length an occurrence must stand alone (no letter or digit glued to it).
pub(crate) const BOUNDARY_BELOW: usize = 12;

/// An encoded copy found by [`propagate`]: its candidate's span, and the index of the
/// candidate whose value it encodes (whose value key it takes).
pub(crate) type Alias = ((usize, usize), usize);

/// Appends a `Propagated` candidate for every occurrence of a candidate's value that no
/// candidate covers yet. `cands` are in ORIGINAL offsets; the search runs on the shadow text
/// (so a value split by a hidden character elsewhere is still found). Values for which
/// `skip` is true (kept by the user) are not propagated. Values of the matcher and the term
/// list get only the encoded-copy search (they find their plain copies themselves). Returns
/// the encoded copies.
pub(crate) fn propagate(
    text: &str,
    shadow: &Shadow,
    cands: &mut Vec<Candidate>,
    skip: impl Fn(&Candidate) -> bool,
) -> Vec<Alias> {
    // value -> (pattern index); per pattern the strongest candidate speaks for it.
    let mut index: HashMap<Cow<'_, str>, usize> = HashMap::new();
    let mut values: Vec<Cow<'_, str>> = Vec::new();
    let mut reps: Vec<usize> = Vec::new();
    // The matcher and the term list find every plain occurrence themselves, so their values
    // get only the encoded-copy search. They come last: values `..plain` get both searches.
    let encoded_only = |c: &Candidate| c.source == Source::Watchlist || c.detector == terms::ID;
    for pass_encoded_only in [false, true] {
        for (i, c) in cands.iter().enumerate() {
            if encoded_only(c) != pass_encoded_only || c.len() < MIN_CHARS {
                continue;
            }
            let v = normalize_value(&text[c.start..c.end]);
            if v.chars().count() < MIN_CHARS {
                continue;
            }
            if let Some(&p) = index.get(&v) {
                if c.confidence > cands[reps[p]].confidence {
                    reps[p] = i;
                }
                continue;
            }
            if skip(c) {
                continue;
            }
            index.insert(v.clone(), values.len());
            values.push(v);
            reps.push(i);
        }
    }
    let plain = reps.iter().filter(|&&r| !encoded_only(&cands[r])).count();
    let bounded: Vec<bool> = values
        .iter()
        .map(|v| v.chars().count() < BOUNDARY_BELOW)
        .collect();
    // The search reports the longest value at a position, and every copy of a short value
    // (which counts only where it stands alone). The checks below drop a copy only when they
    // would drop every copy inside it too.
    let occ = Occurrences::new(
        values.iter().map(|v| v.as_bytes()).collect(),
        bounded.clone(),
    );
    // The plain search, over values `..plain` (the same indices).
    let plain_occ = (plain < values.len()).then(|| {
        Occurrences::new(
            values[..plain].iter().map(|v| v.as_bytes()).collect(),
            bounded[..plain].to_vec(),
        )
    });
    let conf: Vec<f32> = reps.iter().map(|&r| cands[r].confidence).collect();

    // What is already covered, as sorted, merged intervals.
    let mut covered: Vec<(usize, usize)> = cands.iter().map(|c| (c.start, c.end)).collect();
    covered.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(covered.len());
    for (s, e) in covered {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    let is_covered = |s: usize, e: usize| -> bool {
        let i = merged.partition_point(|&(ms, _)| ms <= s);
        i > 0 && merged[i - 1].1 >= e
    };

    let st: &str = &shadow.text;
    let mut copies = Copies::default();
    plain_occ
        .as_ref()
        .unwrap_or(&occ)
        .find(st.as_bytes(), |ms, me, p| {
            if bounded[p] {
                let glued = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
                if glued(st[..ms].chars().next_back()) || glued(st[me..].chars().next()) {
                    return;
                }
            }
            let r = shadow.to_original(ms..me);
            if is_covered(r.start, r.end) {
                return;
            }
            copies.add(ValueCopy::new(r.start, r.end, p, conf[p], None));
        });
    // Encoded copies.
    let min_len = values.iter().map(|v| v.len()).min().unwrap_or(MIN_CHARS);
    let glued = |b: Option<u8>| b.is_some_and(|b| b.is_ascii_alphanumeric());
    let levels = encoded_levels(st.as_bytes(), min_len);
    // The values written out in one level-1 piece (the one in `marked`), for the level-2
    // pieces decoded from it.
    let mut marks: Option<Marks> = None;
    let mut marked: Option<usize> = None;
    for level in &levels {
        for (at, bytes, outer) in level.pieces() {
            if let Some(o) = outer {
                if marked != Some(o) {
                    occ.mark(levels[0].bytes(o), marks.get_or_insert_with(|| occ.marks()));
                    marked = Some(o);
                }
            }
            occ.find(bytes, |s, e, p| {
                let (ds, de) = (at + s, at + e);
                if bounded[p] {
                    let (before, after) = level.neighbours(ds, de);
                    if glued(before) || glued(after) {
                        return;
                    }
                }
                let Some((src, exact)) = level.source(ds, de) else {
                    return;
                };
                // Written out without an escape: the plain search above has it.
                if exact && src.len() == de - ds {
                    return;
                }
                // Found one level deeper, but written out in the outer run: the search of the
                // outer run has it (`\\share` is a backslash and a word, not a form feed).
                if outer.is_some() && marks.as_ref().is_some_and(|m| m.has(p)) {
                    return;
                }
                let r = shadow.to_original(src);
                if is_covered(r.start, r.end) {
                    return;
                }
                copies.add(ValueCopy::new(r.start, r.end, p, conf[p], Some(reps[p])));
            });
        }
    }
    let mut found: Vec<Candidate> = Vec::new();
    let mut aliases: Vec<Alias> = Vec::new();
    for c in copies.finish() {
        let rep = &cands[reps[c.value]];
        found.push(Candidate {
            start: c.start,
            end: c.end,
            kind: rep.kind.clone(),
            label: rep.label.clone(),
            confidence: rep.confidence,
            detector: rep.detector.clone(),
            reason: Cow::Borrowed(if c.encoded {
                "Encoded copy of another finding"
            } else {
                "Same value as another finding"
            }),
            source: Source::Propagated,
        });
        if let Some(rep) = c.alias {
            aliases.push(((c.start, c.end), rep));
        }
    }
    cands.extend(found);
    aliases
}

/// The copies found so far. Copies that overlap are merged as they come in: the resolve step
/// merges them anyway, so a run of overlapping copies (a value repeated, values nested) costs
/// one candidate, named as resolve would name it (see [`ValueCopy::names_over`]).
#[derive(Default)]
pub(crate) struct Copies {
    list: Vec<ValueCopy>,
    /// `list` was sorted and merged when it held this many.
    settled: usize,
    /// Copies added so far.
    added: usize,
    /// The own spans of the first [`SEEN`] copies. A copy at the same place as an earlier one
    /// is the same candidate (the first one found), so it never names a span; it still gives
    /// an encoded copy's key.
    seen: HashSet<(usize, usize)>,
}

/// How many spans [`Copies`] remembers to tell a copy at the same place as an earlier one.
const SEEN: usize = 1 << 16;

/// A copy, or overlapping copies merged into one span.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ValueCopy {
    pub start: usize,
    pub end: usize,
    /// The copy that names the span: its value, the value's confidence, its own span, and
    /// whether it is an encoded copy.
    pub value: usize,
    conf: f32,
    own: (usize, usize),
    pub encoded: bool,
    /// The order in which it was found.
    seq: usize,
    /// For an encoded copy whose own span is the whole span: the candidate whose value it
    /// encodes (the lowest such index), so the span takes that value's key.
    pub alias: Option<usize>,
}

impl ValueCopy {
    /// A copy of value `value` at `start..end`; `alias` (an encoded copy) is the candidate
    /// whose value it encodes.
    pub fn new(
        start: usize,
        end: usize,
        value: usize,
        conf: f32,
        alias: Option<usize>,
    ) -> ValueCopy {
        ValueCopy {
            start,
            end,
            value,
            conf,
            own: (start, end),
            encoded: alias.is_some(),
            seq: 0,
            alias,
        }
    }

    /// Whether this copy names a merged span over `other`, as in [`crate::resolve::resolve`]:
    /// the higher confidence, then the longer own span, then the earlier one, then the one
    /// found first. One order for all copies, so the name does not depend on the order in
    /// which copies are merged.
    fn names_over(&self, other: &ValueCopy) -> bool {
        use std::cmp::{Ordering, Reverse};
        let rest = |c: &ValueCopy| (c.own.1 - c.own.0, Reverse(c.own), Reverse(c.seq));
        match self.conf.partial_cmp(&other.conf) {
            Some(Ordering::Greater) => true,
            Some(Ordering::Less) => false,
            _ => rest(self) > rest(other),
        }
    }

    /// Merges an overlapping copy into this one.
    fn absorb(&mut self, other: ValueCopy) {
        let span = (self.start.min(other.start), self.end.max(other.end));
        let alias = |c: &ValueCopy| c.alias.filter(|_| (c.start, c.end) == span);
        let alias = match (alias(self), alias(&other)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if other.names_over(self) {
            self.value = other.value;
            self.conf = other.conf;
            self.own = other.own;
            self.encoded = other.encoded;
            self.seq = other.seq;
        }
        (self.start, self.end) = span;
        self.alias = alias;
    }
}

impl Copies {
    pub fn add(&mut self, mut c: ValueCopy) {
        c.seq = self.added;
        self.added += 1;
        // Plain copies come first, one per place and value; an encoded copy can land on the
        // place of another copy.
        if c.encoded && self.seen.contains(&c.own) {
            c.conf = f32::NEG_INFINITY;
        } else if self.seen.len() < SEEN {
            self.seen.insert(c.own);
        }
        // Most copies arrive in order: one that overlaps the last merges at once.
        if let Some(last) = self.list.last_mut() {
            if c.start < last.end && last.start < c.end {
                last.absorb(c);
                return;
            }
        }
        self.list.push(c);
        if self.list.len() > 2 * self.settled + 1024 {
            self.settle();
        }
    }

    /// Sorts the copies and merges the ones that overlap (spans that only touch stay apart).
    fn settle(&mut self) {
        self.list.sort_unstable_by_key(|c| (c.start, c.end));
        let mut out: Vec<ValueCopy> = Vec::with_capacity(self.list.len());
        for c in self.list.drain(..) {
            match out.last_mut() {
                Some(last) if c.start < last.end => last.absorb(c),
                _ => out.push(c),
            }
        }
        self.list = out;
        self.settled = self.list.len();
    }

    /// The merged copies, sorted by start, none overlapping another.
    pub fn finish(mut self) -> Vec<ValueCopy> {
        self.settle();
        self.list
    }
}

/// The encoded runs of `text` decoded (level 1), and the encoded runs inside those (level 2,
/// mapped to the whole level-1 run). Decoded bytes are capped at 8x the text plus 1 MiB.
fn encoded_levels(text: &[u8], min_len: usize) -> Vec<Decoded> {
    let mut budget = text.len().saturating_mul(8) + (1 << 20);
    let mut first = Decoded::default();
    if !decode(text, min_len, true, &|r| r, &mut budget, &mut first) || first.is_empty() {
        return vec![first];
    }
    let mut second = Decoded::default();
    for (i, (bytes, src)) in first.runs().enumerate() {
        second.set_outer(i);
        let outer = |_: std::ops::Range<usize>| src.clone();
        if !decode(bytes, min_len, false, &outer, &mut budget, &mut second) {
            break;
        }
    }
    vec![first, second]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(start: usize, end: usize) -> Candidate {
        Candidate {
            start,
            end,
            kind: Cow::Borrowed("password"),
            label: Cow::Borrowed("Password"),
            confidence: 0.8,
            detector: Cow::Borrowed("context"),
            reason: Cow::Borrowed("test"),
            source: Source::Context,
        }
    }

    /// `password=abab…` lines (each value a prefix of the next), then one long `abab…` run:
    /// the copies in the run cost a few candidates, not one per value per position, and
    /// still cover all of it (audit 2026-11-10, finding 3: 98 KB of this took 1.33 GB).
    #[test]
    fn copies_of_nested_values_cost_few_candidates() {
        let mut text = String::new();
        let mut cands = Vec::new();
        for k in 6..=60 {
            text.push_str("password=");
            let s = text.len();
            text.push_str(&"ab".repeat(k));
            cands.push(found(s, text.len()));
            text.push('\n');
        }
        text.push_str("note ");
        let run = (text.len(), text.len() + 16_000);
        text.push_str(&"ab".repeat(8_000));
        text.push('\n');
        let shadow = Shadow::new(&text);
        let before = cands.len();
        propagate(&text, &shadow, &mut cands, |_| false);
        let added = &cands[before..];
        assert!(added.len() <= 4, "{} candidates", added.len());
        let mut spans: Vec<(usize, usize)> = added.iter().map(|c| (c.start, c.end)).collect();
        spans.sort_unstable();
        let mut reach = run.0;
        for (s, e) in spans {
            if s <= reach {
                reach = reach.max(e);
            }
        }
        assert!(reach >= run.1, "covered up to {reach} of {run:?}");
    }
}
