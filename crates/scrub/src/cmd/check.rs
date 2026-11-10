//! `scrub check`: scan what is staged in git, for the pre-commit hook.
//!
//! What would be committed is what counts, whatever has been edited since `git add`:
//!
//! - The staged entries come from `git diff --cached --raw -z --no-renames`. Without rename
//!   detection a renamed or copied file is an added file, so its whole content is checked,
//!   however similar it is to the file it came from.
//! - Each entry is read by its blob id (`git cat-file --batch`), never by its name, so no file
//!   name can make git read something else (`:0:name` stage syntax and the like).
//! - Deletions are ignored, submodules (gitlinks) are skipped with a line, and a staged
//!   symlink is checked as what git stores for it: the link's target path, never the file it
//!   points to.
//! - Binary blobs are skipped with a line, by the same rule as `scan` ([`files::decode`]: a
//!   share of control bytes, not one NUL). A text blob that cannot be read (too large, a BOM
//!   that does not decode, a conflict) blocks the commit: the check never passes text unread.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use redactor_core::ScanRequest;

use crate::cli::CheckArgs;
use crate::files::{self, Decoded, Skip};
use crate::CliError;

const MODE_SYMLINK: &str = "120000";
const MODE_GITLINK: &str = "160000";

/// One staged entry: what the commit would record at `path`.
#[derive(Debug, PartialEq, Eq)]
pub struct Staged {
    pub mode: String,
    pub id: String,
    pub status: char,
    pub path: Vec<u8>,
}

impl Staged {
    fn name(&self) -> String {
        display_path(&self.path)
    }
}

/// A path for messages: lossy UTF-8, control characters (a newline in a name) escaped.
fn display_path(path: &[u8]) -> String {
    let s = String::from_utf8_lossy(path);
    if s.chars().any(char::is_control) {
        s.escape_debug().to_string()
    } else {
        s.into_owned()
    }
}

fn git_error(what: &str, stderr: &[u8]) -> CliError {
    let why = String::from_utf8_lossy(stderr);
    CliError::new(2, format!("git could not {what}: {}", why.trim()))
}

/// Parses `git diff --raw -z --no-renames` output.
pub fn parse_raw(out: &[u8]) -> Result<Vec<Staged>, String> {
    let mut fields = out.split(|&b| b == 0);
    let mut entries = Vec::new();
    while let Some(meta) = fields.next() {
        if meta.is_empty() {
            continue;
        }
        let meta = std::str::from_utf8(meta).map_err(|_| "unexpected git output".to_string())?;
        let parts: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
        let [_, new_mode, _, new_id, status] = parts[..] else {
            return Err(format!("unexpected git output: {meta}"));
        };
        let path = fields
            .next()
            .ok_or_else(|| "unexpected git output: no path".to_string())?;
        entries.push(Staged {
            mode: new_mode.to_string(),
            id: new_id.to_string(),
            status: status.chars().next().unwrap_or('?'),
            path: path.to_vec(),
        });
    }
    Ok(entries)
}

/// `git`, run at the repository top level, with the settings that could change what the
/// check reads turned off whatever the user or repository config says: replace refs (which
/// would show other content for a blob id), relative diffs (`diff.relative` limits the diff to
/// the current folder), prefixes, path quoting and colour.
fn git(top: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(top).args([
        "--no-replace-objects",
        "-c",
        "diff.relative=false",
        "-c",
        "diff.noprefix=false",
        "-c",
        "diff.mnemonicPrefix=false",
        "-c",
        "core.quotePath=false",
        "-c",
        "color.ui=never",
        "-c",
        "color.diff=never",
    ]);
    cmd
}

/// The repository's top-level folder, or one clear error outside a repository (instead of the
/// usage text `git diff` prints there).
fn top_level() -> Result<PathBuf, CliError> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| CliError::new(2, format!("cannot run git: {e}")))?;
    let top = String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string();
    if !out.status.success() || top.is_empty() {
        return Err(CliError::new(
            2,
            "not inside a git work tree: `scrub check` checks what is staged for a commit",
        ));
    }
    Ok(PathBuf::from(top))
}

/// Every staged entry, relative to the repository root.
fn staged_entries(top: &Path) -> Result<Vec<Staged>, CliError> {
    let out = git(top)
        .args([
            "diff",
            "--cached",
            "--raw",
            "-z",
            "--no-renames",
            "--no-abbrev",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--ignore-submodules=none",
        ])
        .output()
        .map_err(|e| CliError::new(2, format!("cannot run git: {e}")))?;
    if !out.status.success() {
        return Err(git_error("list the staged files", &out.stderr));
    }
    parse_raw(&out.stdout).map_err(|e| CliError::new(2, e))
}

/// What came back for one blob.
enum Blob {
    /// The whole blob, at most [`files::MAX_BYTES`].
    Full(Vec<u8>),
    /// Larger than the limit: the size and the first bytes only.
    Large {
        head: Vec<u8>,
    },
    Missing,
}

/// Reads blobs by id through one `git cat-file --batch`.
fn read_blobs(top: &Path, ids: &[&str]) -> Result<Vec<Blob>, CliError> {
    let mut child = git(top)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CliError::new(2, format!("cannot run git: {e}")))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let request: String = ids.iter().map(|id| format!("{id}\n")).collect();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(request.as_bytes());
    });
    let mut reader = BufReader::new(child.stdout.take().expect("piped stdout"));
    let fail = |what: &str| CliError::new(2, format!("git could not read a staged file: {what}"));
    let mut blobs = Vec::with_capacity(ids.len());
    for _ in ids {
        let mut header = String::new();
        reader
            .read_line(&mut header)
            .map_err(|e| fail(&e.to_string()))?;
        let fields: Vec<&str> = header.trim_end().split(' ').collect();
        match fields[..] {
            [_, "missing"] => blobs.push(Blob::Missing),
            [_, _, size] => {
                let size: u64 = size.parse().map_err(|_| fail("unexpected output"))?;
                let blob = if size > files::MAX_BYTES {
                    let mut head = vec![0u8; 4096];
                    reader
                        .read_exact(&mut head)
                        .map_err(|e| fail(&e.to_string()))?;
                    std::io::copy(&mut (&mut reader).take(size - 4096), &mut std::io::sink())
                        .map_err(|e| fail(&e.to_string()))?;
                    Blob::Large { head }
                } else {
                    let mut raw = vec![0u8; usize::try_from(size).unwrap_or(0)];
                    reader
                        .read_exact(&mut raw)
                        .map_err(|e| fail(&e.to_string()))?;
                    Blob::Full(raw)
                };
                let mut lf = [0u8; 1];
                reader
                    .read_exact(&mut lf)
                    .map_err(|e| fail(&e.to_string()))?;
                blobs.push(blob);
            }
            _ => return Err(fail("unexpected output")),
        }
    }
    let _ = writer.join();
    drop(reader);
    let out = child.wait_with_output().map_err(|e| fail(&e.to_string()))?;
    if !out.status.success() {
        return Err(git_error("read the staged files", &out.stderr));
    }
    Ok(blobs)
}

pub fn run(args: &CheckArgs) -> Result<i32, CliError> {
    let top = top_level()?;
    let loaded = super::load(&args.engine)?;
    super::warn_weakening(&loaded.config);
    super::require_detection(&loaded.engine, &loaded.config)?;
    let engine = loaded.engine;
    let (mut offending, mut unreadable, mut unverified) = (0usize, 0usize, 0usize);
    let mut to_read = Vec::new();
    for entry in staged_entries(&top)? {
        if entry.status == 'D' {
            continue;
        }
        if entry.status == 'U' {
            eprintln!(
                "scrub: cannot check {}: it has unresolved conflicts",
                entry.name()
            );
            unreadable += 1;
            continue;
        }
        if entry.mode == MODE_GITLINK {
            eprintln!("skipped  {}  (submodule)", entry.name());
            continue;
        }
        if entry.id.bytes().all(|b| b == b'0') {
            // Nothing staged under this name (an intent-to-add entry).
            continue;
        }
        to_read.push(entry);
    }
    let ids: Vec<&str> = to_read.iter().map(|e| e.id.as_str()).collect();
    let blobs = if ids.is_empty() {
        Vec::new()
    } else {
        read_blobs(&top, &ids)?
    };
    for (entry, blob) in to_read.iter().zip(blobs) {
        let name = entry.name();
        let decoded = match blob {
            Blob::Missing => {
                eprintln!("scrub: cannot check {name}: git has no object {}", entry.id);
                unreadable += 1;
                continue;
            }
            Blob::Large { head } if files::head_is_binary(&head) => Decoded::Skip(Skip::Binary),
            Blob::Large { .. } => Decoded::Skip(Skip::TooLarge),
            Blob::Full(raw) => files::decode(&raw),
        };
        let text = match decoded {
            Decoded::Text { text, .. } => text,
            Decoded::Skip(why) if why.is_text() => {
                eprintln!("scrub: cannot check {name}: it is {why}");
                unreadable += 1;
                continue;
            }
            Decoded::Skip(why) => {
                eprintln!("skipped  {name}  ({why})");
                continue;
            }
        };
        let what = if entry.mode == MODE_SYMLINK {
            " (symlink target)"
        } else {
            ""
        };
        let outcome = engine.scan(&text, &ScanRequest::default());
        if super::warn_unverified(&format!("{name}{what}"), outcome.verified) {
            unverified += 1;
        }
        if !outcome.findings.is_empty() {
            offending += 1;
            let kinds = super::kinds(outcome.findings.iter().map(|f| f.label.clone()));
            eprintln!(
                "{name}{what}: {} finding(s) — {kinds}",
                outcome.findings.len()
            );
        }
    }
    if unreadable > 0 {
        eprintln!(
            "scrub: {unreadable} staged file(s) could not be checked; the commit is blocked \
             until they can be (or bypass with `git commit --no-verify` after reviewing them)"
        );
    }
    Ok(if offending > 0 {
        1
    } else if unreadable + unverified > 0 {
        2
    } else {
        0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_output_is_parsed() {
        let z = "0".repeat(40);
        let (a, b) = ("a".repeat(40), "b".repeat(40));
        let out = format!(
            ":000000 100644 {z} {a} A\0new file.env\0:100644 100644 {a} {b} M\0dir/x\ny\0\
             :100644 000000 {a} {z} D\0gone\0"
        );
        let e = parse_raw(out.as_bytes()).unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!(
            e[0],
            Staged {
                mode: "100644".into(),
                id: a.clone(),
                status: 'A',
                path: b"new file.env".to_vec()
            }
        );
        assert_eq!((e[1].status, e[1].path.as_slice()), ('M', &b"dir/x\ny"[..]));
        assert_eq!(e[1].name(), "dir/x\\ny");
        assert_eq!(e[2].status, 'D');
        assert!(parse_raw(b":bad\0").is_err());
    }
}
