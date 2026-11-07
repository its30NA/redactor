//! Byte offset <-> UTF-16 code unit mapping for the app shell (IPC offsets are UTF-16).

/// Maps byte offsets of one string to UTF-16 indices and back.
///
/// Stores one entry per non-ASCII character, so pure ASCII text costs nothing.
#[derive(Clone, Debug, Default)]
pub struct Utf16Map {
    /// Per non-ASCII character: (byte start, utf16 start, utf8 length, utf16 length).
    chars: Vec<(usize, usize, u8, u8)>,
    byte_len: usize,
    utf16_len: usize,
}

impl Utf16Map {
    pub fn new(text: &str) -> Utf16Map {
        let mut chars = Vec::new();
        let mut utf16 = 0usize;
        for (i, c) in text.char_indices() {
            if !c.is_ascii() {
                chars.push((i, utf16, c.len_utf8() as u8, c.len_utf16() as u8));
            }
            utf16 += c.len_utf16();
        }
        Utf16Map {
            chars,
            byte_len: text.len(),
            utf16_len: utf16,
        }
    }

    /// Length of the text in UTF-16 code units.
    pub fn utf16_len(&self) -> usize {
        self.utf16_len
    }

    /// UTF-16 index of a byte offset. Offsets inside a character map to the character's start.
    pub fn to_utf16(&self, byte: usize) -> usize {
        let byte = byte.min(self.byte_len);
        let idx = self.chars.partition_point(|&(b, ..)| b <= byte);
        if idx == 0 {
            return byte;
        }
        let (b, u, l8, l16) = self.chars[idx - 1];
        let char_end = b + l8 as usize;
        if byte < char_end {
            u
        } else {
            u + l16 as usize + (byte - char_end)
        }
    }

    /// Byte offset of a UTF-16 index. Indices inside a surrogate pair map to the character's start.
    pub fn to_byte(&self, utf16: usize) -> usize {
        let utf16 = utf16.min(self.utf16_len);
        let idx = self.chars.partition_point(|&(_, u, ..)| u <= utf16);
        if idx == 0 {
            return utf16;
        }
        let (b, u, l8, l16) = self.chars[idx - 1];
        let char_end_u = u + l16 as usize;
        if utf16 < char_end_u {
            b
        } else {
            b + l8 as usize + (utf16 - char_end_u)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_identity() {
        let m = Utf16Map::new("hello");
        assert_eq!(m.to_utf16(3), 3);
        assert_eq!(m.to_byte(3), 3);
    }

    #[test]
    fn roundtrip_with_emoji_and_umlaut() {
        let text = "a😀bä\r\nc";
        let m = Utf16Map::new(text);
        let mut u = 0;
        for (i, c) in text.char_indices() {
            assert_eq!(m.to_utf16(i), u, "byte {i}");
            assert_eq!(m.to_byte(u), i, "utf16 {u}");
            u += c.len_utf16();
        }
        assert_eq!(m.to_utf16(text.len()), u);
        assert_eq!(m.to_byte(u), text.len());
        assert_eq!(m.utf16_len(), text.encode_utf16().count());
        // Inside the surrogate pair / inside the UTF-8 sequence: snap to the char start.
        assert_eq!(m.to_byte(2), 1);
        assert_eq!(m.to_utf16(2), 1);
    }
}
