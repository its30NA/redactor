//! Reading a text file the user opened (`openTextFile`): at most 20 MiB, UTF-8 (with or
//! without BOM) or UTF-16 (see `redactor_wire::text`, shared with the CLI). Anything else is
//! refused as binary.

use std::fmt;

use zeroize::Zeroize;

use crate::bridge::MAX_INPUT_BYTES;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileError {
    TooLarge,
    NotText,
    Unreadable,
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FileError::TooLarge => "The file is larger than 20 MiB.",
            FileError::NotText => "That is not a text file (UTF-8 or UTF-16).",
            FileError::Unreadable => "The file could not be read.",
        })
    }
}

impl std::error::Error for FileError {}

/// Decodes file bytes. The byte buffer is zeroized before returning.
pub fn decode(mut bytes: Vec<u8>) -> Result<String, FileError> {
    let r = decode_inner(&bytes);
    bytes.zeroize();
    r
}

fn decode_inner(bytes: &[u8]) -> Result<String, FileError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(FileError::TooLarge);
    }
    redactor_wire::text::decode(bytes)
        .map(|(text, _)| text)
        .map_err(|_| FileError::NotText)
}

/// A file name for display: the last path component only (no folders reach the UI).
pub fn display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file.txt".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_and_boms() {
        assert_eq!(decode(b"plain\r\ntext".to_vec()).unwrap(), "plain\r\ntext");
        assert_eq!(
            decode("\u{FEFF}Grüße".as_bytes().to_vec()).unwrap(),
            "Grüße"
        );
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("hé😀".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode(le).unwrap(), "hé😀");
        let be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain("hé😀".encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        assert_eq!(decode(be).unwrap(), "hé😀");
    }

    #[test]
    fn refuses_binary_and_huge() {
        assert_eq!(
            decode(vec![0x89, b'P', b'N', b'G', 0, 1]),
            Err(FileError::NotText)
        );
        assert_eq!(decode(vec![b'a', 0, b'b']), Err(FileError::NotText));
        assert_eq!(decode(vec![0xC3, 0x28]), Err(FileError::NotText));
        assert_eq!(decode(vec![0xFF, 0xFE, 0x41]), Err(FileError::NotText));
        assert_eq!(
            decode(vec![b'a'; MAX_INPUT_BYTES + 1]),
            Err(FileError::TooLarge)
        );
    }

    #[test]
    fn display_name_drops_folders() {
        let p = std::path::Path::new("some").join("dir").join("app.log");
        assert_eq!(display_name(&p), "app.log");
    }
}
