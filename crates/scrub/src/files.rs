//! File walking and decoding for `scan` and `check`: which files are candidates, how they are
//! read, and which are skipped.
//!
//! Binary and oversized files are skipped, each with a line on stderr: they are not text anyone
//! pastes into an assistant, and rewriting them risks corruption. UTF-16 files (what Windows
//! PowerShell 5.1 writes with `>`) are text and are scanned. Symlinks found while walking are
//! not followed, so `--write` never reaches outside the tree.

use std::fs;
use std::path::{Path, PathBuf};

use redactor_wire::text::{self, NotText, TextEncoding};

/// Directories never worth scanning: noise, dependencies, VCS internals.
pub const SKIP_DIRS: [&str; 14] = [
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    ".mypy_cache",
    ".ruff_cache",
    ".pytest_cache",
    "dist",
    "build",
    ".idea",
    ".vscode",
];

/// Files above this size are skipped.
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// How much of a file decides "binary": a NUL byte in this first chunk.
const SNIFF_BYTES: usize = 1024;

/// File bytes, read as text or set aside with a reason.
#[derive(Debug, PartialEq, Eq)]
pub enum Decoded {
    Text {
        text: String,
        encoding: TextEncoding,
        /// Not valid UTF-8 and no BOM (a legacy 8-bit encoding): decoded with replacement
        /// characters, so the file must not be rewritten.
        lossy: bool,
    },
    Skip(Skip),
}

/// Why a file is not scanned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    TooLarge,
    Binary,
    /// A BOM announces a text encoding that the bytes do not follow.
    Undecodable(TextEncoding),
}

impl Skip {
    /// True when the file is (or claims to be) text: a commit check must not pass it unread.
    pub fn is_text(self) -> bool {
        !matches!(self, Skip::Binary)
    }
}

impl std::fmt::Display for Skip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Skip::TooLarge => write!(f, "larger than {} MiB", MAX_BYTES / (1024 * 1024)),
            Skip::Binary => f.write_str("binary"),
            Skip::Undecodable(e) => {
                write!(f, "starts with a {} BOM but does not decode", e.name())
            }
        }
    }
}

/// Reads file bytes as text: UTF-8 (BOM or not), UTF-16LE/BE (see [`redactor_wire::text`]),
/// or, failing those, a legacy 8-bit text decoded lossily. Binary means a NUL byte in the first
/// KiB that is not UTF-16 text.
pub fn decode(raw: &[u8]) -> Decoded {
    if raw.len() as u64 > MAX_BYTES {
        return Decoded::Skip(Skip::TooLarge);
    }
    decode_any_size(raw)
}

/// [`decode`] without the size limit.
pub fn decode_any_size(raw: &[u8]) -> Decoded {
    match text::decode(raw) {
        Ok((text, encoding)) => Decoded::Text {
            text,
            encoding,
            lossy: false,
        },
        Err(NotText::BadEncoding(e)) => Decoded::Skip(Skip::Undecodable(e)),
        Err(NotText::Binary) => {
            if raw.iter().take(SNIFF_BYTES).any(|&b| b == 0) {
                return Decoded::Skip(Skip::Binary);
            }
            // UTF-8 with a NUL further on, or a legacy encoding (Latin-1, Windows-1252).
            match std::str::from_utf8(raw) {
                Ok(t) => Decoded::Text {
                    text: t.to_owned(),
                    encoding: TextEncoding::Utf8 { bom: false },
                    lossy: false,
                },
                Err(_) => Decoded::Text {
                    text: String::from_utf8_lossy(raw).into_owned(),
                    encoding: TextEncoding::Utf8 { bom: false },
                    lossy: true,
                },
            }
        }
    }
}

/// Whether a file too large to scan is binary, judged from its first bytes.
pub fn head_is_binary(head: &[u8]) -> bool {
    let even = &head[..head.len().min(4096) & !1];
    decode_any_size(even) == Decoded::Skip(Skip::Binary)
}

/// The files to scan under `paths`, in a stable order, and the paths that do not exist.
pub fn collect(paths: &[PathBuf], recursive: bool) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files = Vec::new();
    let mut missing = Vec::new();
    for path in paths {
        if path.is_file() {
            files.push(path.clone());
        } else if path.is_dir() {
            walk(path, recursive, &mut files);
        } else {
            missing.push(path.clone());
        }
    }
    (files, missing)
}

fn walk(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = read.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_file() {
            out.push(path);
        } else if kind.is_dir() && recursive {
            let skip = entry
                .file_name()
                .to_str()
                .is_some_and(|n| SKIP_DIRS.contains(&n));
            if !skip {
                walk(&path, recursive, out);
            }
        }
    }
}

/// Reads a file and remembers which file it was, for [`replace`].
pub fn read(path: &Path) -> std::io::Result<(Vec<u8>, fs::Metadata)> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let meta = file.metadata()?;
    let mut raw = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    file.read_to_end(&mut raw)?;
    Ok((raw, meta))
}

/// Same file, same content as when it was read (as far as metadata tells).
fn unchanged(read: &fs::Metadata, now: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (read.dev(), read.ino()) != (now.dev(), now.ino()) {
            return false;
        }
    }
    read.len() == now.len() && read.modified().ok() == now.modified().ok()
}

/// Replaces the file at `path` with `bytes`, atomically: a temporary file in the same folder,
/// with the file's permissions, renamed over it. Right before, the path is checked again
/// without following links: it must still be the regular file that was read (`read_meta`),
/// so a symlink swapped in after the walk is never followed.
pub fn replace(path: &Path, bytes: &[u8], read_meta: &fs::Metadata) -> Result<(), String> {
    use std::io::Write;
    let now = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !now.file_type().is_file() {
        return Err("it is not a regular file (a symbolic link?); left unchanged".into());
    }
    if !unchanged(read_meta, &now) {
        return Err("it changed while it was being scanned; left unchanged".into());
    }
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (tmp, mut file) = (0..1000)
        .find_map(|n| {
            let tmp = dir.join(format!(".{name}.scrub-{}-{n}.tmp", std::process::id()));
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
                .ok()
                .map(|f| (tmp, f))
        })
        .ok_or("cannot create a temporary file next to it")?;
    let written = file
        .write_all(bytes)
        .and_then(|()| file.set_permissions(now.permissions()))
        .and_then(|()| file.sync_all());
    drop(file);
    let done = written.and_then(|()| fs::rename(&tmp, path));
    if let Err(e) = done {
        let _ = fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_is_atomic_and_keeps_permissions() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.env");
        fs::write(&p, "old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let (_, meta) = read(&p).unwrap();
        replace(&p, b"new", &meta).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        // Nothing left behind.
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 1);
    }

    #[test]
    fn replace_refuses_a_changed_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.env");
        fs::write(&p, "old").unwrap();
        let (_, meta) = read(&p).unwrap();
        fs::write(&p, "someone else's longer edit").unwrap();
        assert!(replace(&p, b"new", &meta).unwrap_err().contains("changed"));
        assert_eq!(fs::read(&p).unwrap(), b"someone else's longer edit");
    }

    #[cfg(unix)]
    #[test]
    fn replace_never_follows_a_swapped_in_symlink() {
        let d = tempfile::tempdir().unwrap();
        let (p, outside) = (d.path().join("a.env"), d.path().join("outside.txt"));
        fs::write(&p, "old").unwrap();
        fs::write(&outside, "keep me").unwrap();
        let (_, meta) = read(&p).unwrap();
        fs::remove_file(&p).unwrap();
        std::os::unix::fs::symlink(&outside, &p).unwrap();
        assert!(replace(&p, b"new", &meta)
            .unwrap_err()
            .contains("not a regular file"));
        assert_eq!(fs::read(&outside).unwrap(), b"keep me");
    }

    fn skip(raw: &[u8]) -> Option<Skip> {
        match decode(raw) {
            Decoded::Skip(s) => Some(s),
            Decoded::Text { .. } => None,
        }
    }

    #[test]
    fn binary_and_large_are_skipped() {
        assert_eq!(skip(b"text\x00more"), Some(Skip::Binary));
        assert_eq!(skip(b"just text"), None);
        assert_eq!(
            skip(&vec![b'a'; MAX_BYTES as usize + 1]),
            Some(Skip::TooLarge)
        );
    }

    #[test]
    fn a_nul_after_the_first_chunk_does_not_count() {
        let mut raw = vec![b'a'; 1024];
        raw.push(0);
        assert_eq!(skip(&raw), None);
    }

    #[test]
    fn utf16_is_text_and_keeps_its_encoding() {
        let le = text::encode("PASS=x\r\n", TextEncoding::Utf16Le { bom: true });
        assert_eq!(
            decode(&le),
            Decoded::Text {
                text: "PASS=x\r\n".into(),
                encoding: TextEncoding::Utf16Le { bom: true },
                lossy: false
            }
        );
        assert_eq!(
            skip(&[0xFF, 0xFE, b'a']),
            Some(Skip::Undecodable(TextEncoding::Utf16Le { bom: true }))
        );
        assert!(Skip::Undecodable(TextEncoding::Utf8 { bom: true }).is_text());
        assert!(Skip::TooLarge.is_text());
        assert!(!Skip::Binary.is_text());
    }

    #[test]
    fn latin1_is_decoded_lossily() {
        let Decoded::Text { text, lossy, .. } = decode(b"caf\xe9 = 1") else {
            panic!("not text")
        };
        assert!(lossy && text.starts_with("caf"));
    }

    #[test]
    fn large_heads_are_judged() {
        assert!(head_is_binary(&[0x89, b'P', b'N', b'G', 0, 0, 1, 2]));
        assert!(!head_is_binary(b"plain text, cut in the midd"));
        let mut le = text::encode("a line\n", TextEncoding::Utf16Le { bom: true });
        le.push(b'x'); // cut mid-unit
        assert!(!head_is_binary(&le));
    }

    #[test]
    fn walk_is_sorted_and_prunes_noise() {
        let d = tempfile::tempdir().unwrap();
        for p in [
            "b.txt",
            "a.txt",
            "src/c.txt",
            "node_modules/x.js",
            ".git/config",
        ] {
            let f = d.path().join(p);
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, "x").unwrap();
        }
        let (files, missing) = collect(&[d.path().to_path_buf()], true);
        let names: Vec<_> = files
            .iter()
            .map(|f| {
                f.strip_prefix(d.path())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(names, ["a.txt", "b.txt", "src/c.txt"]);
        assert!(missing.is_empty());
        let (flat, _) = collect(&[d.path().to_path_buf()], false);
        assert_eq!(flat.len(), 2);
    }
}
