//! `scrub`: the command-line front end of redactor.
//!
//! ```text
//! scrub [FILE]              sanitize a file or stdin -> stdout (the default)
//! scrub scan PATH...        scan files and directories, exit 1 on findings
//! scrub check               scan what is staged in git (reads the staged blobs)
//! scrub install-hook        install the pre-commit hook
//! scrub clipboard           sanitize the clipboard in place (Windows)
//! scrub detectors           list every rule
//! ```
//!
//! Sanitized text goes to stdout; summaries, previews, notices and the audit (with `--audit -`)
//! go to stderr, so pipes stay clean. Exit codes are the v1 ones: 0 success, 1 findings (`scan`,
//! `check`) or an existing hook, 2 for usage, input and environment errors.
#![forbid(unsafe_code)]

pub mod audit;
pub mod cli;
pub mod clipboard;
pub mod cmd;
pub mod config;
pub mod files;
pub mod preview;

pub use cmd::hook;

use std::ffi::OsString;
use std::io::Write;

/// An error that ends the run with a message and an exit code.
#[derive(Debug)]
pub struct CliError {
    pub code: i32,
    pub message: String,
}

impl CliError {
    pub fn new(code: i32, message: impl Into<String>) -> CliError {
        CliError {
            code,
            message: message.into(),
        }
    }
}

/// Runs the CLI with the given arguments (including the program name) and returns the exit code.
pub fn run<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    use clap::Parser;
    let cli = match cli::Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return e.exit_code();
        }
    };
    match cmd::dispatch(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("scrub: {}", e.message);
            e.code
        }
    }
}

/// Exit code after a panic (the code Rust itself uses for one).
pub const PANIC_EXIT: i32 = 101;

/// Replaces the default panic output, which quotes the panic message, and a message can quote
/// input text (a string-slicing panic shows the characters around the cut). This hook prints a
/// fixed line with the source location only, then exits with [`PANIC_EXIT`] (before the
/// release build's abort).
pub fn install_quiet_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("{}", panic_line(info.location()));
        std::process::exit(PANIC_EXIT);
    }));
}

/// The one line printed for a panic: never its message.
pub fn panic_line(location: Option<&std::panic::Location<'_>>) -> String {
    match location {
        Some(l) => format!(
            "scrub: internal error at {}:{} (the details are withheld: they could quote your \
             text)",
            l.file(),
            l.line()
        ),
        None => "scrub: internal error (the details are withheld: they could quote your text)"
            .to_string(),
    }
}

/// Writes to stdout, ignoring a closed pipe (`scrub file | head`). Any other write error (a
/// full disk) is an error with exit code 2: the output is incomplete.
pub fn out(text: &str) -> Result<(), CliError> {
    out_bytes(text.as_bytes())
}

/// [`out`] for text already encoded (UTF-16 output of a UTF-16 input).
pub fn out_bytes(bytes: &[u8]) -> Result<(), CliError> {
    let mut stdout = std::io::stdout().lock();
    write_ignoring_closed_pipe(&mut stdout, bytes)
}

fn write_ignoring_closed_pipe(w: &mut impl Write, bytes: &[u8]) -> Result<(), CliError> {
    match w.write_all(bytes).and_then(|()| w.flush()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => {
            Err(CliError::new(2, format!("cannot write the output: {e}")))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    struct Failing(std::io::ErrorKind);

    impl std::io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn only_a_closed_pipe_is_ignored_on_output() {
        use std::io::ErrorKind;
        assert!(
            super::write_ignoring_closed_pipe(&mut Failing(ErrorKind::BrokenPipe), b"x").is_ok()
        );
        let e = super::write_ignoring_closed_pipe(&mut Failing(ErrorKind::StorageFull), b"x")
            .unwrap_err();
        assert_eq!(e.code, 2);
        assert!(
            e.message.contains("cannot write the output"),
            "{}",
            e.message
        );
    }

    #[test]
    fn the_panic_line_carries_the_location_and_nothing_else() {
        let here = std::panic::Location::caller();
        let line = super::panic_line(Some(here));
        assert!(line.starts_with("scrub: internal error at "), "{line}");
        assert!(line.contains(&format!("{}:{}", here.file(), here.line())));
        let payload = std::panic::catch_unwind(|| {
            let s = "pässword";
            let _ = &s[..2];
        })
        .unwrap_err();
        // The default message would have quoted the text; the line does not.
        let msg = payload.downcast_ref::<String>().unwrap();
        assert!(msg.contains('ä'), "{msg}");
        assert!(!line.contains('ä') && !super::panic_line(None).contains('ä'));
    }
}
