//! The app's command-line arguments.
//!
//! `--scrub-clipboard` cleans the clipboard as the hotkey does: on a second start the running
//! app gets it through the single-instance plugin, on a first start the app acts on it once it
//! is up. A desktop shortcut that runs `redactor --scrub-clipboard` is then a hotkey.

/// Asks the app to clean the clipboard.
pub const SCRUB_FLAG: &str = "--scrub-clipboard";

/// True when an argument after the program name is exactly [`SCRUB_FLAG`].
pub fn scrub_requested(args: &[String]) -> bool {
    args.iter().skip(1).any(|a| a == SCRUB_FLAG)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn flag_after_program_name_requests_a_scrub() {
        assert!(scrub_requested(&args(&["redactor", "--scrub-clipboard"])));
    }

    #[test]
    fn program_name_alone_does_not() {
        assert!(!scrub_requested(&args(&["--scrub-clipboard"])));
        assert!(!scrub_requested(&args(&["redactor"])));
        assert!(!scrub_requested(&args(&[])));
    }

    #[test]
    fn look_alikes_do_not() {
        for look_alike in ["--scrub-clipboard=1", "--scrub", "-scrub-clipboard"] {
            assert!(
                !scrub_requested(&args(&["redactor", look_alike])),
                "{look_alike}"
            );
        }
    }
}
