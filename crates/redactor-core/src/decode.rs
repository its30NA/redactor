//! The decode layer: encoded runs that decode to readable text.
//!
//! Four encodings are recognised, each as a maximal run of its own characters:
//! - base64 / base64url runs of 16+ characters (padding included),
//! - hex runs of 32+ digits,
//! - percent-encoded runs with at least one `%XX` (a secret with a single symbol is
//!   encoded with one escape),
//! - JSON string runs with at least one `\uXXXX` escape.
//!
//! A run becomes a [`Layer`] only when it decodes to valid UTF-8 of which at least 80 % is
//! printable. The engine scans the layers again (and their layers, up to depth 2) and maps
//! every hit back onto the whole encoded run. All decoded bytes of a scan share one
//! [`Budget`].

use std::ops::Range;

/// One decoded run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Layer {
    /// The encoded run in the text that was decoded.
    pub range_in_parent: Range<usize>,
    /// The decoded text.
    pub text: String,
}

/// How many decoded bytes a scan may still produce, and how many runs were skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Budget {
    pub remaining: usize,
    /// Runs left undecoded because the budget ran out.
    pub skipped: usize,
}

/// Decoded bytes are capped at 4x the input and 8 MiB.
pub(crate) const MAX_DECODED: usize = 8 << 20;

impl Budget {
    pub fn for_input(len: usize) -> Budget {
        Budget {
            remaining: len.saturating_mul(4).min(MAX_DECODED),
            skipped: 0,
        }
    }

    fn take(&mut self, n: usize) -> bool {
        if n <= self.remaining {
            self.remaining -= n;
            true
        } else {
            self.skipped += 1;
            false
        }
    }
}

const MIN_B64: usize = 16;
const MIN_HEX: usize = 32;

/// All layers of `text` (one level deep), sorted by start. Layers may overlap (a percent
/// run around a base64 run).
pub(crate) fn decode_layers(text: &str, budget: &mut Budget) -> Vec<Layer> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    base64_and_hex(bytes, budget, &mut out);
    if bytes.contains(&b'%') {
        percent(bytes, budget, &mut out);
    }
    if text.contains("\\u") {
        json_escapes(text, bytes, budget, &mut out);
    }
    out.sort_by_key(|l| (l.range_in_parent.start, l.range_in_parent.end));
    out
}

fn push(out: &mut Vec<Layer>, budget: &mut Budget, range: Range<usize>, decoded: Vec<u8>) {
    let Ok(text) = String::from_utf8(decoded) else {
        return;
    };
    if !printable(&text) || !budget.take(text.len()) {
        return;
    }
    out.push(Layer {
        range_in_parent: range,
        text,
    });
}

/// At least 4 characters, at least 80 % of them printable (tab and line breaks count).
fn printable(s: &str) -> bool {
    let mut total = 0usize;
    let mut good = 0usize;
    for c in s.chars() {
        total += 1;
        if !c.is_control() || matches!(c, '\t' | '\n' | '\r') {
            good += 1;
        }
    }
    total >= 4 && good * 5 >= total * 4
}

// ---------------------------------------------------------------------------------------
// base64, base64url, hex
// ---------------------------------------------------------------------------------------

fn b64_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'-' | b'_')
}

fn base64_and_hex(bytes: &[u8], budget: &mut Budget, out: &mut Vec<Layer>) {
    let mut i = 0;
    while i < bytes.len() {
        if !b64_char(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && b64_char(bytes[i]) {
            i += 1;
        }
        let body_end = i;
        while i < bytes.len() && i - body_end < 2 && bytes[i] == b'=' {
            i += 1;
        }
        let end = i;
        let body = &bytes[start..body_end];
        // 16+ characters, padding included.
        if end - start < MIN_B64 {
            continue;
        }
        if body.len() >= MIN_HEX
            && body.len().is_multiple_of(2)
            && body.iter().all(u8::is_ascii_hexdigit)
        {
            push(out, budget, start..body_end, hex_decode(body));
            continue;
        }
        let std = body.iter().any(|b| matches!(b, b'+' | b'/'));
        let url = body.iter().any(|b| matches!(b, b'-' | b'_'));
        if std && url {
            continue;
        }
        if let Some(decoded) = b64_decode(body, url) {
            push(out, budget, start..end, decoded);
        }
    }
}

fn hex_decode(h: &[u8]) -> Vec<u8> {
    let nib = |c: u8| -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => c - b'A' + 10,
        }
    };
    h.as_chunks::<2>()
        .0
        .iter()
        .map(|p| (nib(p[0]) << 4) | nib(p[1]))
        .collect()
}

/// Decodes unpadded base64 (`url` selects the `-_` alphabet). `None` for an impossible length.
fn b64_decode(body: &[u8], url: bool) -> Option<Vec<u8>> {
    if body.len() % 4 == 1 {
        return None;
    }
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' if !url => 62,
            b'/' if !url => 63,
            b'-' if url => 62,
            b'_' if url => 63,
            _ => return None,
        })
    };
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    for chunk in body.chunks(4) {
        let mut n = 0u32;
        for (k, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * k);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..chunk.len() - 1]);
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------
// Percent encoding
// ---------------------------------------------------------------------------------------

/// Characters of one percent-encoded value: unreserved characters, `%`, and `+` (form
/// encoding). `&`, `=`, `?`, `/`, `:` and `#` separate values.
fn pct_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'%' | b'-' | b'.' | b'_' | b'~' | b'+' | b'!' | b'*')
}

fn percent(bytes: &[u8], budget: &mut Budget, out: &mut Vec<Layer>) {
    let mut i = 0;
    while i < bytes.len() {
        if !pct_char(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && pct_char(bytes[i]) {
            i += 1;
        }
        let run = &bytes[start..i];
        if !run.contains(&b'%') {
            continue;
        }
        let mut decoded = Vec::with_capacity(run.len());
        let mut escapes = 0;
        let mut k = 0;
        while k < run.len() {
            if run[k] == b'%'
                && k + 2 < run.len()
                && run[k + 1].is_ascii_hexdigit()
                && run[k + 2].is_ascii_hexdigit()
            {
                decoded.push(hex_decode(&run[k + 1..k + 3])[0]);
                escapes += 1;
                k += 3;
            } else {
                decoded.push(run[k]);
                k += 1;
            }
        }
        if escapes == 0 {
            continue;
        }
        // Form encoding writes a blank as `+`: both readings are scanned.
        if decoded.contains(&b'+') {
            let spaced: Vec<u8> = decoded
                .iter()
                .map(|&b| if b == b'+' { b' ' } else { b })
                .collect();
            push(out, budget, start..i, spaced);
        }
        push(out, budget, start..i, decoded);
    }
}

// ---------------------------------------------------------------------------------------
// JSON escapes
// ---------------------------------------------------------------------------------------

fn hex4(b: &[u8]) -> Option<u32> {
    if b.len() < 4 || !b[..4].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    Some(
        b[..4]
            .iter()
            .fold(0u32, |n, &c| n * 16 + (c as char).to_digit(16).unwrap_or(0)),
    )
}

/// Runs of JSON string content (no quote, no line break) holding at least one `\uXXXX`.
fn json_escapes(text: &str, bytes: &[u8], budget: &mut Budget, out: &mut Vec<Layer>) {
    let mut from = 0;
    while let Some(p) = text[from..].find("\\u") {
        let first = from + p;
        if hex4(&bytes[first + 2..]).is_none() {
            from = first + 2;
            continue;
        }
        // Extend back and forward over string content.
        // Never back over text an earlier run already covered (keeps this linear).
        let mut start = first;
        while start > from && !matches!(bytes[start - 1], b'"' | b'\n' | b'\r' | b'\'') {
            start -= 1;
        }
        while !text.is_char_boundary(start) {
            start += 1;
        }
        let mut decoded = String::new();
        let mut k = start;
        let mut escapes = 0;
        let mut pending_high: Option<u32> = None;
        while k < bytes.len() && !matches!(bytes[k], b'"' | b'\n' | b'\r' | b'\'') {
            if bytes[k] == b'\\' {
                let Some(&next) = bytes.get(k + 1) else {
                    break;
                };
                if next == b'u' {
                    let Some(cp) = hex4(&bytes[k + 2..]) else {
                        break;
                    };
                    escapes += 1;
                    k += 6;
                    match (pending_high.take(), cp) {
                        (Some(hi), 0xDC00..=0xDFFF) => {
                            let c = 0x10000 + ((hi - 0xD800) << 10) + (cp - 0xDC00);
                            decoded.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
                        }
                        (hi, 0xD800..=0xDBFF) => {
                            if hi.is_some() {
                                decoded.push('\u{FFFD}');
                            }
                            pending_high = Some(cp);
                        }
                        (hi, cp) => {
                            if hi.is_some() {
                                decoded.push('\u{FFFD}');
                            }
                            decoded.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                        }
                    }
                    continue;
                }
                let c = match next {
                    b'n' => '\n',
                    b't' => '\t',
                    b'r' => '\r',
                    b'b' => '\u{8}',
                    b'f' => '\u{c}',
                    b'"' | b'\\' | b'/' => next as char,
                    _ => break,
                };
                if pending_high.take().is_some() {
                    decoded.push('\u{FFFD}');
                }
                decoded.push(c);
                k += 2;
                continue;
            }
            if pending_high.take().is_some() {
                decoded.push('\u{FFFD}');
            }
            let c = text[k..].chars().next().unwrap_or('\u{FFFD}');
            decoded.push(c);
            k += c.len_utf8();
        }
        if pending_high.is_some() {
            decoded.push('\u{FFFD}');
        }
        if escapes > 0 {
            push(out, budget, start..k, decoded.into_bytes());
        }
        from = k.max(first + 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layers(text: &str) -> Vec<(String, String)> {
        let mut b = Budget::for_input(text.len());
        decode_layers(text, &mut b)
            .into_iter()
            .map(|l| (text[l.range_in_parent].to_string(), l.text))
            .collect()
    }

    #[test]
    fn base64_variants() {
        // "user=demo&mode=full" in standard and url-safe base64, padded and not.
        assert_eq!(
            layers("x dXNlcj1kZW1vJm1vZGU9ZnVsbA== y"),
            vec![(
                "dXNlcj1kZW1vJm1vZGU9ZnVsbA==".into(),
                "user=demo&mode=full".into()
            )]
        );
        assert_eq!(
            layers("dXNlcj1kZW1vJm1vZGU9ZnVsbA")[0].1,
            "user=demo&mode=full"
        );
        // 0xfb 0xff -> `-_` alphabet
        assert_eq!(layers("PDw_Pz4-aGVsbG8gd29ybGQ")[0].1, "<<??>>hello world");
        // Not text: random identifiers and words do not decode to anything printable.
        assert!(layers("getUserAccountSettings AbstractSingletonProxyFactory").is_empty());
    }

    #[test]
    fn hex_runs() {
        let hex: String = "hello there, plain text"
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(
            layers(&hex),
            vec![(hex.clone(), "hello there, plain text".into())]
        );
        // A digest does not decode to text.
        assert!(layers("da39a3ee5e6b4b0d3255bfef95601890afd80709").is_empty());
    }

    #[test]
    fn percent_runs() {
        assert_eq!(
            layers("a?k=pa%20ss%2Fw0rd&x=1"),
            vec![("pa%20ss%2Fw0rd".into(), "pa ss/w0rd".into())]
        );
        let both = layers("q=two+words%21");
        assert_eq!(both.len(), 2);
        assert!(both.iter().any(|l| l.1 == "two words!"));
        assert!(both.iter().any(|l| l.1 == "two+words!"));
        assert!(layers("100% sure").is_empty());
    }

    #[test]
    fn json_escape_runs() {
        assert_eq!(
            layers(r#"{"a": "pw \u0070\u0077\u0021 ok"}"#),
            vec![(r"pw \u0070\u0077\u0021 ok".into(), "pw pw! ok".into())]
        );
        assert_eq!(layers(r#""\ud83d\ude00 smile""#)[0].1, "😀 smile");
    }

    #[test]
    fn budget_caps_decoded_bytes() {
        let unit = "aGVsbG8gd29ybGQsIHBsYWluIHRleHQ= ";
        let text = unit.repeat(10);
        let mut b = Budget {
            remaining: 70,
            skipped: 0,
        };
        let got = decode_layers(&text, &mut b);
        assert_eq!(got.len(), 3);
        assert_eq!(b.skipped, 7);
        assert_eq!(Budget::for_input(10).remaining, 40);
        assert_eq!(Budget::for_input(100 << 20).remaining, MAX_DECODED);
    }
}
