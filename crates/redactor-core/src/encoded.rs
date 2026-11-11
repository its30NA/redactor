//! Encoded copies of a text: every run that may be base64 (either alphabet, padded or not),
//! hex, percent-encoded or a JSON string with escapes, decoded into one buffer, so that the
//! values propagation knows can be searched in their encoded forms too.
//!
//! Unlike the decode layer, nothing here has to be readable text: a run is decoded whenever
//! it is long enough to hold the shortest value, and only an exact copy of a known value in
//! the decoded bytes counts. A hit maps back to the characters that carry its bits: exactly
//! for percent and JSON escapes (every decoded byte knows the escape it came from), and for
//! base64 and hex to the characters holding any bit of it (a base64 character at the edge
//! also holds bits of a neighbour). Base64 wrapped across lines is decoded joined as well, and
//! a hit in it maps through the lines, so a value split by a line break maps to its
//! characters on both lines. One level deeper, a hit maps to the whole outer run.

use std::ops::Range;

use crate::decode::wrapped;

/// The decoded runs of one text, back to back in `buf` (a zero byte between runs).
#[derive(Default)]
pub(crate) struct Decoded {
    pub buf: Vec<u8>,
    pieces: Vec<Piece>,
    /// Source offsets of decoded bytes, for pieces that map exactly.
    map: Vec<usize>,
    /// The outer piece the next pieces are decoded from (one level deeper).
    outer: Option<usize>,
}

struct Piece {
    /// Where the decoded bytes start in `buf`.
    at: usize,
    len: usize,
    /// The encoded run in the source.
    src: Range<usize>,
    how: Map,
    /// The piece of the level above this one was decoded from.
    outer: Option<usize>,
}

/// How decoded bytes map back to the source.
#[derive(Clone, Copy)]
enum Map {
    /// Not at all: a hit stands for the whole run.
    Whole,
    /// `map[m + i]` is the source offset of the escape that produced decoded byte `i`, and
    /// `map[m + len]` the end of the run.
    Escapes(usize),
    /// Base64 starting at this source offset; the run's body ends at the second one.
    Base64(usize, usize),
    /// Base64 wrapped across `lines` lines of `width` characters (the last may hold fewer):
    /// `map[at + i]` is where line `i` starts, and `map[at + lines]` the number of characters.
    Wrapped {
        at: usize,
        lines: usize,
        width: usize,
    },
    /// Hex starting at this source offset.
    Hex(usize),
}

impl Decoded {
    /// The piece holding `buf[s..e]`, if one holds all of it.
    fn piece(&self, s: usize, e: usize) -> Option<&Piece> {
        Some(&self.pieces[self.index(s, e)?])
    }

    /// The source range a hit `buf[s..e]` (not empty) stands for, and whether it came from
    /// escapes (percent or JSON), which a value written out plainly maps through one to one.
    pub fn source(&self, s: usize, e: usize) -> Option<(Range<usize>, bool)> {
        let p = self.piece(s, e)?;
        let (a, b) = (s - p.at, e - p.at);
        Some(match p.how {
            Map::Whole => (p.src.clone(), false),
            Map::Escapes(m) => {
                // The end of the escape that produced the last byte: the next escape's start.
                let mut k = b;
                while k < p.len && self.map[m + k] == self.map[m + b - 1] {
                    k += 1;
                }
                (self.map[m + a]..self.map[m + k], true)
            }
            Map::Base64(c0, body_end) => {
                // Byte i sits in characters 4(i/3) + i%3 and the one after.
                let first = c0 + 4 * (a / 3) + a % 3;
                let last = c0 + 4 * ((b - 1) / 3) + (b - 1) % 3 + 1;
                // Padding goes with the last character.
                let end = if last + 1 >= body_end {
                    p.src.end
                } else {
                    last + 1
                };
                (first..end, false)
            }
            Map::Wrapped { at, lines, width } => {
                // As for base64 in one line, in the characters of all lines joined.
                let total = self.map[at + lines];
                let char_at = |j: usize| {
                    let line = (j / width).min(lines - 1);
                    self.map[at + line] + (j - line * width)
                };
                let first = 4 * (a / 3) + a % 3;
                let last = 4 * ((b - 1) / 3) + (b - 1) % 3 + 1;
                let end = if last + 1 >= total {
                    p.src.end
                } else {
                    char_at(last) + 1
                };
                (char_at(first)..end, false)
            }
            Map::Hex(c0) => (c0 + 2 * a..c0 + 2 * b, false),
        })
    }

    /// The bytes of the piece around `buf[s..e]`: what is glued to a hit.
    pub fn neighbours(&self, s: usize, e: usize) -> (Option<u8>, Option<u8>) {
        match self.piece(s, e) {
            Some(p) => (
                (s > p.at).then(|| self.buf[s - 1]),
                (e < p.at + p.len).then(|| self.buf[e]),
            ),
            None => (None, None),
        }
    }

    /// The pieces as (decoded bytes, source range): the input of a deeper level.
    pub fn runs(&self) -> impl Iterator<Item = (&[u8], Range<usize>)> {
        self.pieces
            .iter()
            .map(|p| (&self.buf[p.at..p.at + p.len], p.src.clone()))
    }

    /// The pieces as (where they start in `buf`, their decoded bytes, the piece of the level
    /// above they were decoded from), in the order they were pushed.
    pub fn pieces(&self) -> impl Iterator<Item = (usize, &[u8], Option<usize>)> {
        self.pieces
            .iter()
            .map(|p| (p.at, &self.buf[p.at..p.at + p.len], p.outer))
    }

    /// The decoded bytes of piece `i`.
    pub fn bytes(&self, i: usize) -> &[u8] {
        let p = &self.pieces[i];
        &self.buf[p.at..p.at + p.len]
    }

    /// The index of the piece holding all of `buf[s..e]`.
    fn index(&self, s: usize, e: usize) -> Option<usize> {
        let i = self.pieces.partition_point(|p| p.at <= s).checked_sub(1)?;
        let p = &self.pieces[i];
        (e <= p.at + p.len).then_some(i)
    }

    /// Pieces pushed from now on were decoded from piece `outer` of the level above.
    pub fn set_outer(&mut self, outer: usize) {
        self.outer = Some(outer);
    }

    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    fn push(&mut self, bytes: &[u8], src: Range<usize>, how: Map, map: &[usize]) {
        let how = match how {
            Map::Escapes(_) => {
                let at = self.map.len();
                self.map.extend_from_slice(map);
                Map::Escapes(at)
            }
            Map::Wrapped { lines, width, .. } => {
                let at = self.map.len();
                self.map.extend_from_slice(map);
                Map::Wrapped { at, lines, width }
            }
            h => h,
        };
        self.pieces.push(Piece {
            at: self.buf.len(),
            len: bytes.len(),
            src,
            how,
            outer: self.outer,
        });
        self.buf.extend_from_slice(bytes);
        self.buf.push(0);
    }
}

/// Decodes every encoded run of `hay` that can hold `min_len` bytes. `src` maps a run of
/// `hay` to the source range a hit in it redacts; `exact` maps hits to the characters that
/// carry them instead (only meaningful when `src` is the identity). `budget` caps the
/// decoded bytes; returns false when it ran out.
pub(crate) fn decode(
    hay: &[u8],
    min_len: usize,
    exact: bool,
    src: &dyn Fn(Range<usize>) -> Range<usize>,
    budget: &mut usize,
    out: &mut Decoded,
) -> bool {
    let mut scratch = Vec::new();
    let mut map = Vec::new();
    base64(hay, min_len, exact, src, budget, &mut scratch, out)
        && wrapped_base64(
            hay,
            min_len,
            exact,
            src,
            budget,
            &mut scratch,
            &mut map,
            out,
        )
        && hex(hay, min_len, exact, src, budget, &mut scratch, out)
        && (!hay.contains(&b'%')
            || percent(
                hay,
                min_len,
                exact,
                src,
                budget,
                &mut scratch,
                &mut map,
                out,
            ))
        && (!hay.contains(&b'\\')
            || json(
                hay,
                min_len,
                exact,
                src,
                budget,
                &mut scratch,
                &mut map,
                out,
            ))
}

fn take(budget: &mut usize, n: usize) -> bool {
    if n <= *budget {
        *budget -= n;
        true
    } else {
        false
    }
}

pub(crate) fn b64_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'-' | b'_')
}

/// Both alphabets at once: `+` and `-` are 62, `/` and `_` are 63.
fn b64_val(c: u8) -> u32 {
    match c {
        b'A'..=b'Z' => (c - b'A') as u32,
        b'a'..=b'z' => (c - b'a' + 26) as u32,
        b'0'..=b'9' => (c - b'0' + 52) as u32,
        b'+' | b'-' => 62,
        _ => 63,
    }
}

/// Decodes base64 characters (either alphabet, no padding) onto `out`; a last single
/// character holds no whole byte.
pub(crate) fn b64_into(chars: &[u8], out: &mut Vec<u8>) {
    for chunk in chars.chunks(4) {
        if chunk.len() == 1 {
            break;
        }
        let mut n = 0u32;
        for (k, &c) in chunk.iter().enumerate() {
            n |= b64_val(c) << (18 - 6 * k);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..chunk.len() - 1]);
    }
}

fn base64(
    hay: &[u8],
    min_len: usize,
    exact: bool,
    src: &dyn Fn(Range<usize>) -> Range<usize>,
    budget: &mut usize,
    scratch: &mut Vec<u8>,
    out: &mut Decoded,
) -> bool {
    let min_chars = (min_len * 4).div_ceil(3);
    let mut i = 0;
    while i < hay.len() {
        if !b64_char(hay[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < hay.len() && b64_char(hay[i]) {
            i += 1;
        }
        let body_end = i;
        while i < hay.len() && i - body_end < 2 && hay[i] == b'=' {
            i += 1;
        }
        let body = &hay[start..body_end];
        if body.len() < min_chars {
            continue;
        }
        // A run of letters and digits starts where its encoding starts; one with `/`, `-`,
        // `_` or `+` may carry a prefix glued on (`/v1/<base64>`), so every alignment is tried.
        let shifts = if body.iter().all(u8::is_ascii_alphanumeric) {
            1
        } else {
            4
        };
        for shift in 0..shifts {
            let b = &body[shift..];
            if b.len() < min_chars {
                break;
            }
            scratch.clear();
            b64_into(b, scratch);
            if !take(budget, scratch.len()) {
                return false;
            }
            let how = if exact {
                Map::Base64(start + shift, body_end)
            } else {
                Map::Whole
            };
            out.push(scratch, src(start..i), how, &[]);
        }
    }
    true
}

/// Base64 wrapped across lines (see [`wrapped`]): the lines' characters joined and decoded.
#[allow(clippy::too_many_arguments)]
fn wrapped_base64(
    hay: &[u8],
    min_len: usize,
    exact: bool,
    src: &dyn Fn(Range<usize>) -> Range<usize>,
    budget: &mut usize,
    scratch: &mut Vec<u8>,
    map: &mut Vec<usize>,
    out: &mut Decoded,
) -> bool {
    let min_chars = (min_len * 4).div_ceil(3);
    for w in wrapped(hay) {
        let joined: Vec<u8> = w
            .lines
            .iter()
            .flat_map(|r| hay[r.clone()].iter().copied())
            .collect();
        if joined.len() < min_chars {
            continue;
        }
        scratch.clear();
        b64_into(&joined, scratch);
        if !take(budget, scratch.len()) {
            return false;
        }
        let whole = src(w.lines[0].start..w.end);
        if exact {
            map.clear();
            map.extend(w.lines.iter().map(|r| r.start));
            map.push(joined.len());
            let how = Map::Wrapped {
                at: 0,
                lines: w.lines.len(),
                width: w.lines[0].len(),
            };
            out.push(scratch, whole, how, map);
        } else {
            out.push(scratch, whole, Map::Whole, &[]);
        }
    }
    true
}

fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => c - b'A' + 10,
    }
}

fn hex(
    hay: &[u8],
    min_len: usize,
    exact: bool,
    src: &dyn Fn(Range<usize>) -> Range<usize>,
    budget: &mut usize,
    scratch: &mut Vec<u8>,
    out: &mut Decoded,
) -> bool {
    let mut i = 0;
    while i < hay.len() {
        if !hay[i].is_ascii_hexdigit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < hay.len() && hay[i].is_ascii_hexdigit() {
            i += 1;
        }
        // Both byte alignments: a hex run may have a digit glued on in front.
        for shift in 0..2 {
            let run = &hay[start + shift..i];
            if run.len() < 2 * min_len {
                break;
            }
            scratch.clear();
            scratch.extend(
                run.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| (nibble(p[0]) << 4) | nibble(p[1])),
            );
            if !take(budget, scratch.len()) {
                return false;
            }
            let how = if exact {
                Map::Hex(start + shift)
            } else {
                Map::Whole
            };
            out.push(scratch, src(start..i), how, &[]);
        }
    }
    true
}

/// Characters of one percent-encoded value (as the decode layer reads them).
fn pct_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'%' | b'-' | b'.' | b'_' | b'~' | b'+' | b'!' | b'*')
}

#[allow(clippy::too_many_arguments)]
fn percent(
    hay: &[u8],
    min_len: usize,
    exact: bool,
    src: &dyn Fn(Range<usize>) -> Range<usize>,
    budget: &mut usize,
    scratch: &mut Vec<u8>,
    map: &mut Vec<usize>,
    out: &mut Decoded,
) -> bool {
    let mut i = 0;
    while i < hay.len() {
        if !pct_char(hay[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < hay.len() && pct_char(hay[i]) {
            i += 1;
        }
        let run = &hay[start..i];
        if !run.contains(&b'%') {
            continue;
        }
        scratch.clear();
        map.clear();
        let mut escapes = 0;
        let mut k = 0;
        while k < run.len() {
            map.push(start + k);
            if run[k] == b'%'
                && k + 2 < run.len()
                && run[k + 1].is_ascii_hexdigit()
                && run[k + 2].is_ascii_hexdigit()
            {
                scratch.push((nibble(run[k + 1]) << 4) | nibble(run[k + 2]));
                escapes += 1;
                k += 3;
            } else {
                scratch.push(run[k]);
                k += 1;
            }
        }
        map.push(i);
        if escapes == 0 || scratch.len() < min_len {
            continue;
        }
        if !take(budget, scratch.len()) {
            return false;
        }
        let how = if exact { Map::Escapes(0) } else { Map::Whole };
        out.push(scratch, src(start..i), how, map);
    }
    true
}

fn hex4(b: &[u8]) -> Option<u32> {
    if b.len() < 4 || !b[..4].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    Some(b[..4].iter().fold(0, |n, &c| n * 16 + nibble(c) as u32))
}

/// JSON string content between quotes or line breaks, with its escapes decoded. Escapes
/// the format does not know are kept as they are.
#[allow(clippy::too_many_arguments)]
fn json(
    hay: &[u8],
    min_len: usize,
    exact: bool,
    src: &dyn Fn(Range<usize>) -> Range<usize>,
    budget: &mut usize,
    scratch: &mut Vec<u8>,
    map: &mut Vec<usize>,
    out: &mut Decoded,
) -> bool {
    let delim = |b: u8| matches!(b, b'"' | b'\n' | b'\r' | b'\'');
    let mut i = 0;
    while i < hay.len() {
        let start = i;
        while i < hay.len() && !delim(hay[i]) {
            i += 1;
        }
        let seg = &hay[start..i];
        i += 1;
        if seg.len() < min_len || !seg.contains(&b'\\') {
            continue;
        }
        scratch.clear();
        map.clear();
        let mut escapes = 0;
        let mut k = 0;
        while k < seg.len() {
            let at = start + k;
            if seg[k] == b'\\' && k + 1 < seg.len() {
                let simple = match seg[k + 1] {
                    b'n' => Some(b'\n'),
                    b't' => Some(b'\t'),
                    b'r' => Some(b'\r'),
                    b'b' => Some(8),
                    b'f' => Some(12),
                    b'"' | b'\\' | b'/' | b'\'' => Some(seg[k + 1]),
                    _ => None,
                };
                if let Some(c) = simple {
                    map.push(at);
                    scratch.push(c);
                    escapes += 1;
                    k += 2;
                    continue;
                }
                if seg[k + 1] == b'u' {
                    if let Some(hi) = hex4(&seg[k + 2..]) {
                        let mut cp = hi;
                        let mut len = 6;
                        if (0xD800..0xDC00).contains(&hi) && seg[k + 6..].starts_with(b"\\u") {
                            if let Some(lo @ 0xDC00..=0xDFFF) = hex4(&seg[k + 8..]) {
                                cp = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                                len = 12;
                            }
                        }
                        let c = char::from_u32(cp).unwrap_or('\u{FFFD}');
                        let mut tmp = [0u8; 4];
                        for &b in c.encode_utf8(&mut tmp).as_bytes() {
                            map.push(at);
                            scratch.push(b);
                        }
                        escapes += 1;
                        k += len;
                        continue;
                    }
                }
            }
            map.push(at);
            scratch.push(seg[k]);
            k += 1;
        }
        map.push(start + seg.len());
        if escapes == 0 || scratch.len() < min_len {
            continue;
        }
        if !take(budget, scratch.len()) {
            return false;
        }
        let how = if exact { Map::Escapes(0) } else { Map::Whole };
        out.push(scratch, src(start..start + seg.len()), how, map);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(hay: &str) -> Decoded {
        let mut out = Decoded::default();
        let mut budget = 1 << 20;
        assert!(decode(
            hay.as_bytes(),
            6,
            true,
            &|r| r,
            &mut budget,
            &mut out
        ));
        out
    }

    /// Where `value` was found in the decoded copies of `hay`, mapped back.
    fn hits(hay: &str, value: &str) -> Vec<(String, bool)> {
        let d = decoded(hay);
        let mut out = Vec::new();
        let v = value.as_bytes();
        for s in 0..d.buf.len().saturating_sub(v.len() - 1) {
            if d.buf[s..].starts_with(v) {
                if let Some((r, exact)) = d.source(s, s + v.len()) {
                    out.push((hay[r].to_string(), exact));
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn b64(s: &str, url: bool) -> String {
        let table: &[u8] = if url {
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
        } else {
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        };
        let mut out = String::new();
        for c in s.as_bytes().chunks(3) {
            let n = c
                .iter()
                .enumerate()
                .fold(0u32, |n, (k, &b)| n | (b as u32) << (16 - 8 * k));
            for k in 0..=c.len() {
                out.push(table[(n >> (18 - 6 * k) & 63) as usize] as char);
            }
            if !url {
                out.push_str(&"=".repeat(3 - c.len()));
            }
        }
        out
    }

    fn hexed(s: &str, upper: bool) -> String {
        s.bytes()
            .map(|b| {
                if upper {
                    format!("{b:02X}")
                } else {
                    format!("{b:02x}")
                }
            })
            .collect()
    }

    #[test]
    fn every_encoding_maps_back() {
        let v = "Plumtree47-Kite";
        for url in [false, true] {
            let e = b64(v, url);
            assert_eq!(hits(&format!("x {e} y"), v), [(e.clone(), false)]);
            // Behind a path (another alignment): only the encoded value.
            assert_eq!(hits(&format!("/api/{e}"), v), [(e.clone(), false)]);
            // Inside longer blobs: the characters that hold its bits.
            let e = b64(&format!("user:{v}"), url);
            let got = hits(&format!("a={e}"), v);
            assert!(e.ends_with(&got[0].0) && got[0].0.len() >= 20, "{got:?}");
            let e = b64(&format!("{{\"db\": {{\"password\": \"{v}\"}}}}"), url);
            let got = hits(&format!("blob {e}\n"), v);
            assert!(e.contains(&got[0].0) && got[0].0.len() >= 20, "{got:?}");
        }
        for upper in [false, true] {
            let e = hexed(v, upper);
            assert_eq!(hits(&format!("hex: {e}\n"), v), [(e, false)]);
        }
        // Percent and JSON escapes map exactly.
        assert_eq!(
            hits("cb?q=Plumtree47%2DKite&x=1", v),
            [("Plumtree47%2DKite".into(), true)]
        );
        assert_eq!(
            hits(r#"{"p": "pw Plumtree47\u002dKite ok"}"#, v),
            [(r"Plumtree47\u002dKite".into(), true)]
        );
        assert_eq!(
            hits(r#""Plum\/tree""#, "Plum/tree"),
            [(r"Plum\/tree".into(), true)]
        );
    }

    /// Base64 wrapped across lines (audit 2026-11-10, finding 1): a value split by a line
    /// break is found in the joined body and maps to the characters that carry it on both
    /// lines, the break and the next line's indentation between them.
    #[test]
    fn wrapped_base64_maps_across_lines() {
        let v = "Plumtree47-Kite";
        for (indent, eol) in [("", "\n"), ("    ", "\r\n")] {
            for pad in 0..24 {
                let doc = format!("{} {v} and some more words after it", "x".repeat(pad));
                let e = b64(&doc, false);
                let hay: String = e
                    .as_bytes()
                    .chunks(32)
                    .map(|c| format!("{indent}{}{eol}", String::from_utf8_lossy(c)))
                    .collect();
                let got = hits(&hay, v);
                assert!(!got.is_empty(), "pad {pad}: {hay}");
                for (src, exact) in &got {
                    assert!(!exact);
                    let chars: String = src.split(eol).map(|l| l.trim_start()).collect();
                    assert!(e.contains(&chars), "pad {pad}: {src:?}");
                    assert!(chars.len() >= 20 && chars.len() <= 22, "pad {pad}: {src:?}");
                }
            }
        }
    }

    #[test]
    fn budget_stops_decoding() {
        let mut out = Decoded::default();
        let mut budget = 10;
        let hay = format!(
            "{} {}",
            b64("Plumtree47-Kite", false),
            b64("Plumtree47-Kite", true)
        );
        assert!(!decode(
            hay.as_bytes(),
            6,
            true,
            &|r| r,
            &mut budget,
            &mut out
        ));
    }
}
