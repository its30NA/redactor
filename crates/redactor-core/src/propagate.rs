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
use crate::occurrences::Occurrences;
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
/// `skip` is true (kept by the user) are not propagated. Returns the encoded copies.
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
    for (i, c) in cands.iter().enumerate() {
        // The matcher and the term list find every occurrence themselves.
        if c.source == Source::Watchlist || c.detector == terms::ID || c.len() < MIN_CHARS {
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
    if values.is_empty() {
        return Vec::new();
    }
    let occ = Occurrences::new(values.iter().map(|v| v.as_bytes()).collect());
    let bounded: Vec<bool> = values
        .iter()
        .map(|v| v.chars().count() < BOUNDARY_BELOW)
        .collect();

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
    let mut found: Vec<Candidate> = Vec::new();
    occ.find(st.as_bytes(), |ms, me, p| {
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
        let rep = &cands[reps[p]];
        found.push(Candidate {
            start: r.start,
            end: r.end,
            kind: rep.kind.clone(),
            label: rep.label.clone(),
            confidence: rep.confidence,
            detector: rep.detector.clone(),
            reason: Cow::Borrowed("Same value as another finding"),
            source: Source::Propagated,
        });
    });
    // Encoded copies.
    let mut aliases: Vec<Alias> = Vec::new();
    let min_len = values.iter().map(|v| v.len()).min().unwrap_or(MIN_CHARS);
    let glued = |b: Option<u8>| b.is_some_and(|b| b.is_ascii_alphanumeric());
    let levels = encoded_levels(st.as_bytes(), min_len);
    // (level-1 piece, value) for every value written out in a level-1 piece.
    let mut plain_in_outer: HashSet<(usize, usize)> = HashSet::new();
    if levels.get(1).is_some_and(|l| !l.is_empty()) {
        occ.find(&levels[0].buf, |ds, de, p| {
            if let Some(o) = levels[0].index(ds, de) {
                plain_in_outer.insert((o, p));
            }
        });
    }
    for level in &levels {
        occ.find(&level.buf, |ds, de, p| {
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
            if level
                .outer(ds, de)
                .is_some_and(|o| plain_in_outer.contains(&(o, p)))
            {
                return;
            }
            let r = shadow.to_original(src);
            if is_covered(r.start, r.end) {
                return;
            }
            let rep = &cands[reps[p]];
            aliases.push(((r.start, r.end), reps[p]));
            found.push(Candidate {
                start: r.start,
                end: r.end,
                kind: rep.kind.clone(),
                label: rep.label.clone(),
                confidence: rep.confidence,
                detector: rep.detector.clone(),
                reason: Cow::Borrowed("Encoded copy of another finding"),
                source: Source::Propagated,
            });
        });
    }
    // A value can be reported twice at one place.
    found.sort_unstable_by_key(|c| (c.start, c.end));
    found.dedup_by_key(|c| (c.start, c.end));
    cands.extend(found);
    aliases.sort_unstable();
    aliases.dedup_by_key(|a| a.0);
    aliases
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
