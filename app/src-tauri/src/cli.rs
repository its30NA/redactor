//! The app's command-line arguments.
//!
//! `--scrub-clipboard` scrubs the clipboard as the hotkey does: on a second start the running
//! app gets it through the single-instance plugin, on a first start the app acts on it once it
//! is up. A desktop shortcut that runs the app with `--scrub-clipboard` is then a hotkey.
//!
//! `--version` prints the version and exits before any window, display or platform code runs,
//! so it also proves an installed package finds every library it links.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Asks the app to scrub the clipboard.
pub const SCRUB_FLAG: &str = "--scrub-clipboard";

/// True when an argument after the program name is exactly [`SCRUB_FLAG`].
pub fn scrub_requested(args: &[String]) -> bool {
    args.iter().skip(1).any(|a| a == SCRUB_FLAG)
}

/// Asks the app to print its version and exit.
pub const VERSION_FLAG: &str = "--version";

/// True when an argument after the program name is exactly [`VERSION_FLAG`].
pub fn version_requested(args: &[String]) -> bool {
    args.iter().skip(1).any(|a| a == VERSION_FLAG)
}

/// The line `--version` prints, e.g. `Redactor 2.1.1`.
pub fn version_line() -> String {
    format!("Redactor {}", env!("CARGO_PKG_VERSION"))
}

/// What the AppImage runtime tells the app it starts: the AppImage file (`$APPIMAGE`) and where
/// it is mounted (`$APPDIR`). Both are inherited by anything the app starts.
pub struct AppImage {
    pub file: OsString,
    pub mount: OsString,
}

/// The program a desktop shortcut runs to scrub the clipboard: the AppImage file when this
/// executable runs from that AppImage's mount (the executable then sits in a mount that changes
/// with each start), else this executable. The AppImage's paths count only when absolute: one
/// inherited from another program or set by hand names some other file. `None` when the path
/// chosen is not Unicode.
pub fn program_path(exe: Option<PathBuf>, appimage: Option<AppImage>) -> Option<String> {
    let exe = exe?;
    let file = appimage
        .filter(|a| {
            let mount = Path::new(&a.mount);
            mount.is_absolute() && exe.starts_with(mount)
        })
        .map(|a| PathBuf::from(a.file))
        .filter(|f| f.is_absolute())
        .and_then(|f| f.into_os_string().into_string().ok());
    file.or_else(|| exe.into_os_string().into_string().ok())
}

/// [`program_path`] for this process.
pub fn this_program() -> Option<String> {
    #[cfg(target_os = "linux")]
    let appimage = std::env::var_os("APPIMAGE").map(|file| AppImage {
        file,
        mount: std::env::var_os("APPDIR").unwrap_or_default(),
    });
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

    /// An AppImage and its mount, as the AppImage runtime sets them.
    #[cfg(unix)]
    fn appimage(file: &str, mount: &str) -> Option<AppImage> {
        Some(AppImage {
            file: file.into(),
            mount: mount.into(),
        })
    }

    // AppImages are Linux only: the paths in these tests are absolute only on Unix.
    #[cfg(unix)]
    #[test]
    fn an_appimage_names_its_own_file() {
        let exe = Some(PathBuf::from("/tmp/.mount_RedactXyz/usr/bin/Redactor"));
        assert_eq!(
            program_path(
                exe.clone(),
                appimage("/home/me/Apps/Redactor.AppImage", "/tmp/.mount_RedactXyz")
            )
            .as_deref(),
            Some("/home/me/Apps/Redactor.AppImage")
        );
        // Only an absolute path counts: anything else is not where the file is.
        for odd in ["", "Redactor.AppImage", "./Redactor.AppImage"] {
            assert_eq!(
                program_path(exe.clone(), appimage(odd, "/tmp/.mount_RedactXyz")).as_deref(),
                Some("/tmp/.mount_RedactXyz/usr/bin/Redactor"),
                "{odd:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_appimage_counts_only_when_this_executable_runs_from_its_mount() {
        let exe = Some(PathBuf::from("/opt/redactor/Redactor"));
        // Inherited from an AppImage that started this one, or set by hand.
        for mount in ["/tmp/.mount_RedactXyz", "", "opt", "/opt/redact"] {
            assert_eq!(
                program_path(
                    exe.clone(),
                    appimage("/home/me/Apps/Redactor.AppImage", mount)
                )
                .as_deref(),
                Some("/opt/redactor/Redactor"),
                "{mount:?}"
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
            program_path(
                Some(PathBuf::from("/opt/Redactor")),
                Some(AppImage {
                    file: odd,
                    mount: "/opt".into()
                })
            )
            .as_deref(),
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
    fn version_flag_after_program_name() {
        assert!(version_requested(&args(&["Redactor", "--version"])));
    }

    #[test]
    fn program_name_alone_is_not_a_request() {
        assert!(!version_requested(&args(&["--version"])));
        assert!(!version_requested(&args(&["Redactor"])));
        assert!(!version_requested(&args(&[])));
    }

    #[test]
    fn version_look_alikes_are_not() {
        for look_alike in ["--version=1", "-version", "--Version"] {
            assert!(
                !version_requested(&args(&["Redactor", look_alike])),
                "{look_alike}"
            );
        }
    }

    #[test]
    fn version_line_names_the_crate_version() {
        let line = version_line();
        assert_eq!(line, format!("Redactor {}", env!("CARGO_PKG_VERSION")));
        assert!(line.starts_with("Redactor 2."), "{line}");
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
