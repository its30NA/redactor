//! Text file encodings, shared by the desktop app (`openTextFile`) and the `scrub` CLI.
//!
//! Accepted: UTF-8 with or without a BOM, UTF-16LE and UTF-16BE with a BOM, and UTF-16 without
//! a BOM when the bytes clearly are mostly-ASCII UTF-16 text. Anything else, including any NUL
//! character, is not text. [`encode`] writes text back in the encoding it was read in, BOM
//! included, so a file rewritten in place keeps its encoding.
//!
//! Intermediate buffers that hold decoded text are zeroized before they are dropped.

use zeroize::Zeroizing;

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
const UTF16LE_BOM: [u8; 2] = [0xFF, 0xFE];
const UTF16BE_BOM: [u8; 2] = [0xFE, 0xFF];

/// How a text file was stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8 { bom: bool },
    Utf16Le { bom: bool },
    Utf16Be { bom: bool },
}

impl TextEncoding {
    /// A short name for messages: `UTF-8`, `UTF-16LE (BOM)`, ...
    pub fn name(self) -> &'static str {
        match self {
            TextEncoding::Utf8 { bom: false } => "UTF-8",
            TextEncoding::Utf8 { bom: true } => "UTF-8 (BOM)",
            TextEncoding::Utf16Le { bom: false } => "UTF-16LE",
            TextEncoding::Utf16Le { bom: true } => "UTF-16LE (BOM)",
            TextEncoding::Utf16Be { bom: false } => "UTF-16BE",
            TextEncoding::Utf16Be { bom: true } => "UTF-16BE (BOM)",
        }
    }
}

/// Why bytes are not text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotText {
    /// A BOM announces UTF-8 or UTF-16, but the bytes after it do not decode (or hold a NUL).
    BadEncoding(TextEncoding),
    /// No BOM, and the bytes are neither UTF-8 nor recognisable UTF-16 text.
    Binary,
}

/// Decodes text file bytes. The caller applies its own size limit first.
pub fn decode(bytes: &[u8]) -> Result<(String, TextEncoding), NotText> {
    if let Some(rest) = bytes.strip_prefix(&UTF8_BOM) {
        let enc = TextEncoding::Utf8 { bom: true };
        return utf8(rest)
            .map(|t| (t, enc))
            .ok_or(NotText::BadEncoding(enc));
    }
    if let Some(rest) = bytes.strip_prefix(&UTF16LE_BOM) {
        let enc = TextEncoding::Utf16Le { bom: true };
        return utf16(rest, u16::from_le_bytes)
            .map(|t| (t, enc))
            .ok_or(NotText::BadEncoding(enc));
    }
    if let Some(rest) = bytes.strip_prefix(&UTF16BE_BOM) {
        let enc = TextEncoding::Utf16Be { bom: true };
        return utf16(rest, u16::from_be_bytes)
            .map(|t| (t, enc))
            .ok_or(NotText::BadEncoding(enc));
    }
    if let Some(t) = utf8(bytes) {
        return Ok((t, TextEncoding::Utf8 { bom: false }));
    }
    if bytes.contains(&0) {
        if let Some(t) = bomless_utf16(bytes, u16::from_le_bytes) {
            return Ok((t, TextEncoding::Utf16Le { bom: false }));
        }
        if let Some(t) = bomless_utf16(bytes, u16::from_be_bytes) {
            return Ok((t, TextEncoding::Utf16Be { bom: false }));
        }
    }
    Err(NotText::Binary)
}

/// `text` as bytes in `encoding`, BOM included when the original had one.
pub fn encode(text: &str, encoding: TextEncoding) -> Vec<u8> {
    type Unit = fn(u16) -> [u8; 2];
    let (bom, unit): (&[u8], Option<Unit>) = match encoding {
        TextEncoding::Utf8 { bom } => (if bom { &UTF8_BOM } else { &[] }, None),
        TextEncoding::Utf16Le { bom } => {
            (if bom { &UTF16LE_BOM } else { &[] }, Some(u16::to_le_bytes))
        }
        TextEncoding::Utf16Be { bom } => {
            (if bom { &UTF16BE_BOM } else { &[] }, Some(u16::to_be_bytes))
        }
    };
    let mut out = Vec::with_capacity(bom.len() + text.len() * if unit.is_some() { 2 } else { 1 });
    out.extend_from_slice(bom);
    match unit {
        None => out.extend_from_slice(text.as_bytes()),
        Some(f) => {
            for u in text.encode_utf16() {
                out.extend_from_slice(&f(u));
            }
        }
    }
    out
}

fn utf8(b: &[u8]) -> Option<String> {
    if b.contains(&0) {
        return None;
    }
    std::str::from_utf8(b).ok().map(str::to_owned)
}

fn units_of(b: &[u8], unit: fn([u8; 2]) -> u16) -> Option<Zeroizing<Vec<u16>>> {
    if !b.len().is_multiple_of(2) {
        return None;
    }
    let mut units = Zeroizing::new(Vec::with_capacity(b.len() / 2));
    units.extend(b.as_chunks::<2>().0.iter().map(|c| unit(*c)));
    Some(units)
}

fn utf16(b: &[u8], unit: fn([u8; 2]) -> u16) -> Option<String> {
    let units = units_of(b, unit)?;
    if units.contains(&0) {
        return None;
    }
    String::from_utf16(&units).ok()
}

/// UTF-16 without a BOM, accepted only when it plainly is text: it decodes, holds no NUL and
/// no control character other than tab, line feed, form feed and carriage return, and at least
/// half of its code units are ASCII. Binary data practically never passes all of that.
fn bomless_utf16(b: &[u8], unit: fn([u8; 2]) -> u16) -> Option<String> {
    let units = units_of(b, unit)?;
    if units.is_empty() {
        return None;
    }
    let mut ascii = 0usize;
    for &u in units.iter() {
        if u < 0x20 && !matches!(u, 0x09 | 0x0A | 0x0C | 0x0D) || u == 0x7F {
            return None;
        }
        if u < 0x80 {
            ascii += 1;
        }
    }
    if ascii * 2 < units.len() {
        return None;
    }
    String::from_utf16(&units).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(s: &str, bom: bool) -> Vec<u8> {
        encode(s, TextEncoding::Utf16Le { bom })
    }

    #[test]
    fn utf8_with_and_without_bom() {
        assert_eq!(
            decode(b"plain\r\ntext").unwrap(),
            ("plain\r\ntext".into(), TextEncoding::Utf8 { bom: false })
        );
        assert_eq!(
            decode("\u{FEFF}Grüße".as_bytes()).unwrap(),
            ("Grüße".into(), TextEncoding::Utf8 { bom: true })
        );
    }

    #[test]
    fn utf16_with_a_bom_both_orders() {
        let (t, e) = decode(&le("hé😀\r\n", true)).unwrap();
        assert_eq!(
            (t.as_str(), e),
            ("hé😀\r\n", TextEncoding::Utf16Le { bom: true })
        );
        let be = encode("hé😀", TextEncoding::Utf16Be { bom: true });
        assert_eq!(be[..2], UTF16BE_BOM);
        let (t, e) = decode(&be).unwrap();
        assert_eq!(
            (t.as_str(), e),
            ("hé😀", TextEncoding::Utf16Be { bom: true })
        );
    }

    #[test]
    fn bomless_utf16_text_is_recognised() {
        let (t, e) = decode(&le("key = value\nnext line\n", false)).unwrap();
        assert_eq!(t, "key = value\nnext line\n");
        assert_eq!(e, TextEncoding::Utf16Le { bom: false });
        let be = encode("key = value\n", TextEncoding::Utf16Be { bom: false });
        assert_eq!(decode(&be).unwrap().1, TextEncoding::Utf16Be { bom: false });
    }

    #[test]
    fn round_trips_keep_the_encoding() {
        for enc in [
            TextEncoding::Utf8 { bom: false },
            TextEncoding::Utf8 { bom: true },
            TextEncoding::Utf16Le { bom: true },
            TextEncoding::Utf16Be { bom: true },
            TextEncoding::Utf16Le { bom: false },
            TextEncoding::Utf16Be { bom: false },
        ] {
            let text = "a = 1\r\nb = ü\n";
            let bytes = encode(text, enc);
            assert_eq!(decode(&bytes).unwrap(), (text.to_string(), enc), "{enc:?}");
        }
    }

    #[test]
    fn binary_and_broken_text_are_refused() {
        assert_eq!(
            decode(&[0x89, b'P', b'N', b'G', 0, 1]),
            Err(NotText::Binary)
        );
        assert_eq!(decode(&[b'a', 0, b'b']), Err(NotText::Binary));
        assert_eq!(decode(&[0xC3, 0x28]), Err(NotText::Binary));
        assert_eq!(decode(&[0, 0, 0, 0]), Err(NotText::Binary));
        assert_eq!(
            decode(&[0xFF, 0xFE, 0x41]),
            Err(NotText::BadEncoding(TextEncoding::Utf16Le { bom: true }))
        );
        // A lone surrogate after a BOM.
        assert_eq!(
            decode(&[0xFF, 0xFE, 0x00, 0xD8]),
            Err(NotText::BadEncoding(TextEncoding::Utf16Le { bom: true }))
        );
        // A NUL character inside BOM'd UTF-16.
        assert_eq!(
            decode(&[0xFF, 0xFE, 0x41, 0, 0, 0]),
            Err(NotText::BadEncoding(TextEncoding::Utf16Le { bom: true }))
        );
        assert_eq!(
            decode(&[0xEF, 0xBB, 0xBF, 0xC3]),
            Err(NotText::BadEncoding(TextEncoding::Utf8 { bom: true }))
        );
    }

    #[test]
    fn mostly_non_ascii_bomless_utf16_is_not_guessed() {
        // CJK text without a BOM: too few ASCII units to tell it from binary data.
        assert_eq!(decode(&le("日本語のテキスト", false)), Err(NotText::Binary));
    }
}
