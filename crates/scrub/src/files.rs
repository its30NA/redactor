//! File walking and decoding for `scan` and `check`: which files are candidates, how they are
//! read, and which are skipped.
//!
//! Binary and oversized files are skipped, each with a line on stderr: they are not text anyone
//! pastes into an assistant, and rewriting them risks corruption. Binary is decided by one rule
//! for every command ([`decode`]): a share of control bytes, never a single NUL. UTF-16 files
//! (what Windows PowerShell 5.1 writes with `>`), with or without a BOM, are text and are
//! scanned. Symlinks found while walking are
//! not followed, so `--write` never reaches outside the tree.

use std::fs;
use std::path::{Path, PathBuf};

use redactor_wire::text::{self, NotText, TextEncoding};

/// Directories pruned while walking, with the reason printed in their `skipped` line: VCS
/// internals, dependencies, caches and build output. Editor settings (`.vscode`, `.idea`) are
/// scanned: they often hold credentials. A pruned directory named on the command line is
/// scanned.
pub const SKIP_DIRS: [(&str, &str); 12] = [
    (".git", "version control data"),
    (".hg", "version control data"),
    (".svn", "version control data"),
    ("node_modules", "dependencies"),
    (".venv", "dependencies"),
    ("venv", "dependencies"),
    ("__pycache__", "cache"),
    (".mypy_cache", "cache"),
    (".ruff_cache", "cache"),
    (".pytest_cache", "cache"),
    ("dist", "build output"),
    ("build", "build output"),
];

/// What a walk found.
#[derive(Debug, Default)]
pub struct Collected {
    pub files: Vec<PathBuf>,
    /// Named paths that do not exist.
    pub missing: Vec<PathBuf>,
    /// Directories not descended into, and why.
    pub pruned: Vec<(PathBuf, &'static str)>,
    /// Directories or entries that could not be read, and the error.
    pub unreadable: Vec<(PathBuf, String)>,
}

/// Files above this size are skipped.
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// How much of a file decides "binary": the first 8 KiB (git looks at about as much).
const SNIFF_BYTES: usize = 8 * 1024;

/// Binary means at least this share, in percent, of the sample is NUL or another control byte
/// that text does not use (tab, line feed, vertical tab, form feed, carriage return and escape
/// are allowed). Random, compressed or encrypted bytes have about 11% such bytes (28 of 256
/// values), and executables, images and archives far more; text has next to none, and a few
/// stray NULs in a file that holds a secret stay far below 5%. Half the floor of random data
/// keeps both sides well apart.
const BINARY_PERCENT: usize = 5;

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

/// Reads file bytes as text: UTF-8 (BOM or not), UTF-16LE/BE (with a BOM, or without one when
/// the bytes decode as UTF-16 text, see [`bomless_utf16`]), or, failing those, a legacy 8-bit
/// text decoded lossily. Binary means a sample with a meaningful share of control bytes (see
/// [`BINARY_PERCENT`]): one stray NUL does not make a text file binary. `scan`, `check` and
/// sanitizing all decide through here.
pub fn decode(raw: &[u8]) -> Decoded {
    if raw.len() as u64 > MAX_BYTES {
        return Decoded::Skip(Skip::TooLarge);
    }
    decode_any_size(raw)
}

/// [`decode`] without the size limit.
pub fn decode_any_size(raw: &[u8]) -> Decoded {
    classify(raw, false)
}

/// `partial`: `raw` is the head of a larger file, so it may end inside a character.
fn classify(raw: &[u8], partial: bool) -> Decoded {
    match text::decode(raw) {
        Ok((text, encoding)) => Decoded::Text {
            text,
            encoding,
            lossy: false,
        },
        Err(NotText::BadEncoding(e)) => Decoded::Skip(Skip::Undecodable(e)),
        Err(NotText::Binary) => {
            let sample = &raw[..raw.len().min(SNIFF_BYTES)];
            let binary_looking = mostly_control(sample);
            if raw.contains(&0) {
                for (unit, encoding) in [
                    (
                        u16::from_le_bytes as fn([u8; 2]) -> u16,
                        TextEncoding::Utf16Le { bom: false },
                    ),
                    (u16::from_be_bytes, TextEncoding::Utf16Be { bom: false }),
                ] {
                    if let Some(text) = bomless_utf16(raw, unit, partial) {
                        // ASCII text with a stray NUL can happen to decode as UTF-16 too (pairs
                        // of ASCII letters are valid CJK characters). In real UTF-16 text the
                        // line feed bytes are line feed units; in misread 8-bit text they are
                        // not. (A single line of mostly non-ASCII UTF-16 without a BOM is not
                        // recognised: too little to tell it from binary data.)
                        let lf_bytes = raw.iter().filter(|&&b| b == b'\n').count();
                        let lf_units = text.matches('\n').count();
                        if lf_units > 0 && lf_units * 2 >= lf_bytes {
                            return Decoded::Text {
                                text,
                                encoding,
                                lossy: false,
                            };
                        }
                    }
                }
            }
            if binary_looking {
                return Decoded::Skip(Skip::Binary);
            }
            // UTF-8 with a few stray NULs, or a legacy encoding (Latin-1, Windows-1252).
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

/// Whether at least [`BINARY_PERCENT`] of `sample` are control bytes that text does not use.
fn mostly_control(sample: &[u8]) -> bool {
    let control = sample
        .iter()
        .filter(|&&b| {
            (b < 0x20 && !matches!(b, b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | 0x1B)) || b == 0x7F
        })
        .count();
    !sample.is_empty() && control * 100 >= sample.len() * BINARY_PERCENT
}

/// UTF-16 without a BOM, in any script: an even length, valid surrogate pairs, no NUL unit and
/// no control unit other than tab, line feed, form feed and carriage return. Random bytes fail
/// on unpaired surrogates within a few hundred bytes; binary formats fail on NUL units. With
/// `partial`, a high surrogate cut off at the end is allowed.
fn bomless_utf16(raw: &[u8], unit: fn([u8; 2]) -> u16, partial: bool) -> Option<String> {
    if !raw.len().is_multiple_of(2) || raw.is_empty() {
        return None;
    }
    let mut units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| unit(*c)).collect();
    if partial && units.last().is_some_and(|u| (0xD800..0xDC00).contains(u)) {
        units.pop();
    }
    if units
        .iter()
        .any(|&u| u < 0x20 && !matches!(u, 0x09 | 0x0A | 0x0C | 0x0D) || u == 0x7F)
    {
        return None;
    }
    String::from_utf16(&units).ok()
}

/// Whether a file too large to scan is binary, judged from its first bytes by the same rule
/// as [`decode`].
pub fn head_is_binary(head: &[u8]) -> bool {
    let even = &head[..head.len().min(4096) & !1];
    classify(even, true) == Decoded::Skip(Skip::Binary)
}

/// The files to scan under `paths`, in a stable order, with what was left out. A path named
/// here is always walked, even when its name is in [`SKIP_DIRS`].
pub fn collect(paths: &[PathBuf], recursive: bool) -> Collected {
    let mut c = Collected::default();
    for path in paths {
        if path.is_file() {
            c.files.push(path.clone());
        } else if path.is_dir() {
            walk(path, recursive, &mut c);
        } else {
            c.missing.push(path.clone());
        }
    }
    c
}

fn walk(dir: &Path, recursive: bool, c: &mut Collected) {
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) => {
            c.unreadable.push((dir.to_path_buf(), e.to_string()));
            return;
        }
    };
    let mut entries = Vec::new();
    for entry in read {
        match entry {
            Ok(e) => entries.push(e),
            Err(e) => c.unreadable.push((dir.to_path_buf(), e.to_string())),
        }
    }
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let kind = match entry.file_type() {
            Ok(k) => k,
            Err(e) => {
                c.unreadable.push((path, e.to_string()));
                continue;
            }
        };
        if kind.is_file() {
            c.files.push(path);
        } else if kind.is_dir() && recursive {
            let pruned = entry.file_name().to_str().and_then(|n| {
                SKIP_DIRS
                    .iter()
                    .find(|(name, _)| *name == n)
                    .map(|(_, why)| *why)
            });
            match pruned {
                Some(why) => c.pruned.push((path, why)),
                None => walk(&path, recursive, c),
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
        assert_eq!(skip(b"text\x00\x01more"), Some(Skip::Binary));
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
    fn a_few_stray_nuls_do_not_make_text_binary() {
        let mut raw = b"x\0\n".to_vec();
        raw.extend_from_slice(b"DB_PASSWORD=Plumtree47-Kite\nTOKEN=abc\n");
        let Decoded::Text { text, lossy, .. } = decode(&raw) else {
            panic!("not text")
        };
        assert!(!lossy && text.contains("DB_PASSWORD=Plumtree47-Kite"));
    }

    /// Both sides of [`BINARY_PERCENT`]: 4% control bytes is text, 6% is binary.
    #[test]
    fn the_binary_threshold_is_a_share_of_control_bytes() {
        let with = |controls: usize| {
            let mut raw = vec![b'a'; 1000];
            for i in 0..controls {
                raw[i * 16 + 3] = if i % 2 == 0 { 0 } else { 1 };
            }
            raw
        };
        assert_eq!(skip(&with(40)), None);
        assert_eq!(skip(&with(60)), Some(Skip::Binary));
        // Random bytes (compressed or encrypted data) are about 11% control bytes.
        let mut state = 7u64;
        let random: Vec<u8> = (0..8192)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (state >> 56) as u8
            })
            .collect();
        assert_eq!(skip(&random), Some(Skip::Binary));
        assert!(head_is_binary(&random[..4096]));
        // A large text with a NUL in its head is text (blocked as too large, not passed).
        let mut head = vec![b'a'; 4096];
        head[10] = 0;
        assert!(!head_is_binary(&head));
    }

    #[test]
    fn bomless_utf16_in_any_script_is_text() {
        for (s, enc) in [
            (
                "Привет мир\nпароль: ключ\n",
                TextEncoding::Utf16Le { bom: false },
            ),
            (
                "日本語のテキスト\r\n次の行\r\n",
                TextEncoding::Utf16Le { bom: false },
            ),
            (
                "日本語のテキスト\n次の行\n",
                TextEncoding::Utf16Be { bom: false },
            ),
        ] {
            let raw = text::encode(s, enc);
            assert_eq!(
                decode(&raw),
                Decoded::Text {
                    text: s.into(),
                    encoding: enc,
                    lossy: false
                },
                "{s}"
            );
            assert!(!head_is_binary(&raw));
        }
        // ASCII text with a NUL after a line break does not turn into UTF-16.
        let mut raw = b"ab\n\0c".to_vec();
        raw.extend_from_slice(&b"d\nline of text\n".repeat(4));
        raw.push(b'.'); // even length: it is also valid UTF-16LE
        assert!(bomless_utf16(&raw, u16::from_le_bytes, false).is_some());
        let Decoded::Text { encoding, .. } = decode(&raw) else {
            panic!("not text")
        };
        assert_eq!(encoding, TextEncoding::Utf8 { bom: false });
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
            ".vscode/settings.json",
        ] {
            let f = d.path().join(p);
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, "x").unwrap();
        }
        let rel = |f: &PathBuf| {
            f.strip_prefix(d.path())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        };
        let c = collect(&[d.path().to_path_buf()], true);
        let names: Vec<_> = c.files.iter().map(rel).collect();
        assert_eq!(
            names,
            [".vscode/settings.json", "a.txt", "b.txt", "src/c.txt"]
        );
        assert!(c.missing.is_empty() && c.unreadable.is_empty());
        let pruned: Vec<_> = c.pruned.iter().map(|(p, why)| (rel(p), *why)).collect();
        assert_eq!(
            pruned,
            [
                (".git".to_string(), "version control data"),
                ("node_modules".to_string(), "dependencies")
            ]
        );
        assert_eq!(collect(&[d.path().to_path_buf()], false).files.len(), 2);
        // Named on the command line, a pruned directory is scanned.
        let named = collect(&[d.path().join("node_modules")], true);
        assert_eq!(named.files.len(), 1);
    }
}
