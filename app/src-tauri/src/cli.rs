//! The app's command-line arguments.
//!
//! `--scrub-clipboard` cleans the clipboard as the hotkey does: on a second start the running
//! app gets it through the single-instance plugin, on a first start the app acts on it once it
//! is up. A desktop shortcut that runs the app with `--scrub-clipboard` is then a hotkey.

use std::ffi::OsString;
use std::path::PathBuf;

/// Asks the app to clean the clipboard.
pub const SCRUB_FLAG: &str = "--scrub-clipboard";

/// True when an argument after the program name is exactly [`SCRUB_FLAG`].
pub fn scrub_requested(args: &[String]) -> bool {
    args.iter().skip(1).any(|a| a == SCRUB_FLAG)
}

/// The program a desktop shortcut runs to clean the clipboard: the AppImage file when the app
/// runs from one (`$APPIMAGE`, an absolute path; the executable itself then sits in a mount that
/// changes with each start), else this executable. `None` when neither is a Unicode path.
pub fn program_path(exe: Option<PathBuf>, appimage: Option<OsString>) -> Option<String> {
    let appimage = appimage
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .and_then(|p| p.into_os_string().into_string().ok());
    appimage.or_else(|| exe?.into_os_string().into_string().ok())
}

/// [`program_path`] for this process.
pub fn this_program() -> Option<String> {
    #[cfg(target_os = "linux")]
    let appimage = std::env::var_os("APPIMAGE");
    #[cfg(not(target_os = "linux"))]
    let appimage = None;
    program_path(std::env::current_exe().ok(), appimage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_is_this_executable() {
        let exe = Some(PathBuf::from(
            "/home/me/src/redactor/target/release/Redactor",
        ));
        assert_eq!(
            program_path(exe, None).as_deref(),
            Some("/home/me/src/redactor/target/release/Redactor")
        );
        assert_eq!(program_path(None, None), None);
    }

    #[test]
    fn an_appimage_names_its_own_file() {
        let exe = Some(PathBuf::from("/tmp/.mount_RedactXyz/usr/bin/Redactor"));
        assert_eq!(
            program_path(exe.clone(), Some("/home/me/Apps/Redactor.AppImage".into())).as_deref(),
            Some("/home/me/Apps/Redactor.AppImage")
        );
        // Only an absolute path counts: anything else is not where the file is.
        for odd in ["", "Redactor.AppImage", "./Redactor.AppImage"] {
            assert_eq!(
                program_path(exe.clone(), Some(odd.into())).as_deref(),
                Some("/tmp/.mount_RedactXyz/usr/bin/Redactor"),
                "{odd:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_unicode_is_not_offered() {
        use std::os::unix::ffi::OsStringExt;
        let odd = OsString::from_vec(b"/opt/Red\xffactor".to_vec());
        assert_eq!(program_path(Some(PathBuf::from(odd.clone())), None), None);
        assert_eq!(
            program_path(Some(PathBuf::from("/opt/Redactor")), Some(odd)).as_deref(),
            Some("/opt/Redactor"),
            "an AppImage path that is not Unicode falls back to the executable"
        );
    }

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
