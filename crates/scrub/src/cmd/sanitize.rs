//! `scrub [FILE]`: sanitize a file or stdin.
//!
//! The sanitized text goes out in the input's encoding (UTF-8, or UTF-16 with its BOM);
//! `--json` and `--diff` are always UTF-8. Binary input is refused rather than passed through.

use std::io::Read;
use std::time::Instant;

use redactor_core::ScanRequest;
use redactor_wire::text::{self, TextEncoding};
use similar::TextDiff;

use crate::audit::{audit_json, rows_of, summary};
use crate::cli::SanitizeArgs;
use crate::files::{self, Decoded};
use crate::preview::preview;
use crate::{out, out_bytes, CliError};

fn read_input(path: Option<&str>) -> std::io::Result<Vec<u8>> {
    match path {
        None | Some("-") => {
            let mut buf = Vec::new();
            std::io::stdin().lock().read_to_end(&mut buf)?;
            Ok(buf)
        }
        Some(p) => std::fs::read(p),
    }
}

/// The input as text, and the encoding the sanitized text goes out in. UTF-16 input (Windows
/// PowerShell 5.1 writes it) is decoded, never passed through as "binary".
fn decode_input(bytes: &[u8]) -> Result<(String, TextEncoding), CliError> {
    match files::decode_any_size(bytes) {
        Decoded::Text {
            text,
            encoding,
            lossy,
        } => {
            if lossy {
                eprintln!(
                    "scrub: the input is not valid UTF-8 or UTF-16; invalid bytes were replaced \
                     with U+FFFD in the output"
                );
            }
            Ok((text, encoding))
        }
        Decoded::Skip(why) => Err(CliError::new(
            2,
            format!("cannot read input: it is {why}; nothing was written"),
        )),
    }
}

pub fn run(args: &SanitizeArgs) -> Result<i32, CliError> {
    let bytes = read_input(args.input.as_deref())
        .map_err(|e| CliError::new(2, format!("cannot read input: {e}")))?;
    let (text, encoding) = decode_input(&bytes)?;
    let engine = super::load_engine(&args.engine)?;
    let name = match args.input.as_deref() {
        None | Some("-") => "stdin",
        Some(p) => p,
    };

    let started = Instant::now();
    let outcome = engine.scan(&text, &ScanRequest::default());
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let rows = rows_of(&text, &outcome.findings);
    let verified = outcome.verified;

    if args.json {
        let mut resp = redactor_wire::response_from_outcome(&text, outcome);
        resp.stats.ms = (elapsed_ms * 10.0).round() / 10.0;
        let json = serde_json::to_string(&resp)
            .map_err(|e| CliError::new(2, format!("cannot encode JSON: {e}")))?;
        out(&format!("{json}\n"))?;
    } else if args.diff {
        let diff = TextDiff::from_lines(text.as_str(), outcome.output.as_str());
        out(&diff
            .unified_diff()
            .header(
                &format!("{name} (original)"),
                &format!("{name} (sanitized)"),
            )
            .to_string())?;
    } else {
        out_bytes(&text::encode(&outcome.output, encoding))?;
    }

    if args.preview {
        eprintln!("{}", preview(&text, &rows));
    }
    if let Some(target) = &args.audit {
        let doc = audit_json(name, &rows);
        if target == "-" {
            eprintln!("{doc}");
        } else {
            std::fs::write(target, format!("{doc}\n"))
                .map_err(|e| CliError::new(2, format!("cannot write audit {target}: {e}")))?;
        }
    }
    if args.summary {
        eprintln!("{}", summary(&rows));
    }
    Ok(if super::warn_unverified(name, verified) {
        2
    } else {
        0
    })
}
