//! `scrub scan PATH...`: report (or fix) secrets in files and directories.
//!
//! Every file that is not scanned gets a `skipped` line on stderr. `--write` rewrites a file
//! atomically and in the encoding it was read in.

use std::path::PathBuf;

use redactor_core::{Engine, ScanRequest};
use redactor_wire::text;

use crate::cli::ScanArgs;
use crate::files::{self, collect, Decoded, Skip};
use crate::{out, CliError};

pub fn run(args: &ScanArgs) -> Result<i32, CliError> {
    let engine = super::load_engine(&args.engine)?;
    let paths = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths.clone()
    };
    let (files, missing) = collect(&paths, !args.no_recursive);

    let mut errors = false;
    for path in &missing {
        eprintln!(
            "scrub: cannot read {}: no such file or directory",
            path.display()
        );
        errors = true;
    }

    let (mut total_files, mut total_findings, mut unfixed) = (0usize, 0usize, 0usize);
    for path in &files {
        let (raw, meta) = match files::read(path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("scrub: cannot read {}: {e}", path.display());
                errors = true;
                continue;
            }
        };
        let (text, encoding, lossy) = match files::decode(&raw) {
            Decoded::Text {
                text,
                encoding,
                lossy,
            } => (text, encoding, lossy),
            Decoded::Skip(why) => {
                eprintln!("skipped  {}  ({why})", path.display());
                if matches!(why, Skip::Undecodable(_)) {
                    errors = true;
                }
                continue;
            }
        };
        let outcome = scan_text(&engine, &text);
        if outcome.findings.is_empty() {
            continue;
        }
        let n = outcome.findings.len();
        let kinds = super::kinds(outcome.findings.iter().map(|f| f.label.clone()));
        if args.write {
            if lossy {
                eprintln!(
                    "skipped  {}  (not valid UTF-8 or UTF-16, left unchanged)",
                    path.display()
                );
                unfixed += 1;
                continue;
            }
            if text.contains('\0') {
                // Text with stray NUL bytes may be a binary format after all: never rewritten.
                eprintln!(
                    "skipped  {}  (contains NUL bytes, left unchanged; {n} finding(s) — {kinds})",
                    path.display()
                );
                unfixed += 1;
                continue;
            }
            let bytes = text::encode(&outcome.output, encoding);
            if let Err(e) = files::replace(path, &bytes, &meta) {
                eprintln!("scrub: cannot write {}: {e}", path.display());
                errors = true;
                continue;
            }
            out(&format!("fixed  {}  ({n}: {kinds})\n", path.display()));
        } else {
            out(&format!("{}: {n} finding(s) — {kinds}\n", path.display()));
        }
        total_files += 1;
        total_findings += n;
    }

    let verb = if args.write { "sanitized" } else { "found in" };
    eprintln!("\n{total_findings} finding(s) {verb} {total_files} file(s).");
    if errors {
        return Ok(2);
    }
    // Report mode signals findings through the exit code (handy for CI); fix mode succeeds
    // unless a file had to be left alone.
    let findings_left = if args.write { unfixed } else { total_findings };
    Ok(i32::from(findings_left > 0))
}

fn scan_text(engine: &Engine, text: &str) -> redactor_core::ScanOutcome {
    engine.scan(text, &ScanRequest::default())
}
