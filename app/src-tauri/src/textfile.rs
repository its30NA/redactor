//! Reading a text file the user opened (`openTextFile`): at most 20 MiB, UTF-8 (with or
//! without BOM) or UTF-16 (see `redactor_wire::text`, shared with the CLI). Anything else is
//! refused as binary.

use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;

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

/// Reads the file the user picked, at most [`MAX_INPUT_BYTES`]. Only a regular file is read: a
/// folder, a pipe or a device is refused before it is opened (opening a pipe can block, and a
/// device such as `/dev/zero` reports no length and never ends), and checked again once it is
/// open. A file that grows while it is read stops one byte past the limit and is refused. The
/// buffer is allocated once, so no copy of the text is left behind by a reallocation, and it is
/// zeroized on every error.
pub fn read_bounded(path: &Path) -> Result<Vec<u8>, FileError> {
    let meta = std::fs::metadata(path).map_err(|_| FileError::Unreadable)?;
    check(&meta)?;
    let file = File::open(path).map_err(|_| FileError::Unreadable)?;
    // The path may name something else by now.
    check(&file.metadata().map_err(|_| FileError::Unreadable)?)?;
    let mut buf = Vec::with_capacity(MAX_INPUT_BYTES + 1);
    let read = file.take(MAX_INPUT_BYTES as u64 + 1).read_to_end(&mut buf);
    let refused = match read {
        Err(_) => FileError::Unreadable,
        Ok(_) if buf.len() > MAX_INPUT_BYTES => FileError::TooLarge,
        Ok(_) => return Ok(buf),
    };
    buf.zeroize();
    Err(refused)
}

/// A regular file within the limit.
fn check(meta: &std::fs::Metadata) -> Result<(), FileError> {
    if !meta.is_file() {
        return Err(FileError::Unreadable);
    }
    if meta.len() > MAX_INPUT_BYTES as u64 {
        return Err(FileError::TooLarge);
    }
    Ok(())
}

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
pub fn display_name(path: &Path) -> String {
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

    /// A folder of its own for one test, removed when dropped.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir()
                .join(format!("redactor-textfile-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn read_bounded_reads_a_text_file() {
        let dir = Scratch::new("small");
        let path = dir.0.join("a.txt");
        std::fs::write(&path, b"plain text").unwrap();
        assert_eq!(read_bounded(&path).unwrap(), b"plain text");
    }

    #[test]
    fn read_bounded_refuses_a_file_over_the_limit() {
        let dir = Scratch::new("large");
        let path = dir.0.join("big.log");
        let f = std::fs::File::create(&path).unwrap();
        // Sparse: no 20 MiB written to disk.
        f.set_len(MAX_INPUT_BYTES as u64 + 1).unwrap();
        assert_eq!(read_bounded(&path), Err(FileError::TooLarge));

        f.set_len(MAX_INPUT_BYTES as u64).unwrap();
        assert_eq!(read_bounded(&path).unwrap().len(), MAX_INPUT_BYTES);
    }

    #[test]
    fn read_bounded_refuses_a_folder() {
        let dir = Scratch::new("folder");
        assert_eq!(read_bounded(&dir.0), Err(FileError::Unreadable));
    }

    #[cfg(unix)]
    #[test]
    fn read_bounded_refuses_a_device_at_once() {
        // Its length is 0 and it never ends: it must not be read at all.
        assert_eq!(
            read_bounded(std::path::Path::new("/dev/zero")),
            Err(FileError::Unreadable)
        );
    }

    #[test]
    fn display_name_drops_folders() {
        let p = std::path::Path::new("some").join("dir").join("app.log");
        assert_eq!(display_name(&p), "app.log");
    }
}
