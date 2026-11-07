//! `scrub clipboard`: sanitize the clipboard in place.

use crate::cli::ClipboardArgs;
use crate::clipboard::{scrub_clipboard, ClipOutcome, PowerShell};
use crate::CliError;

pub fn run(args: &ClipboardArgs) -> Result<i32, CliError> {
    if !cfg!(windows) {
        return Err(CliError::new(
            2,
            "the clipboard command is not supported on this platform (Windows only); \
             pipe the text through `scrub` instead",
        ));
    }
    let engine = super::load_engine(&args.engine)?;
    let mut clipboard = PowerShell::default();
    match scrub_clipboard(&mut clipboard, &engine).map_err(|e| CliError::new(2, e))? {
        ClipOutcome::Cleaned { summary, .. } => eprintln!("{summary}"),
        ClipOutcome::Clean => eprintln!("No sensitive data detected."),
        ClipOutcome::Empty => eprintln!("The clipboard has no text; nothing to do."),
        ClipOutcome::Protected => eprintln!(
            "The clipboard content is marked private by its source (for example a password \
             manager); left untouched."
        ),
        ClipOutcome::Changed => {
            return Err(CliError::new(
                2,
                "the clipboard changed while it was being scanned; nothing was written. \
                 Run the command again to scrub the new content.",
            ))
        }
    }
    Ok(0)
}
