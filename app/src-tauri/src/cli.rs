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

/// The Flatpak app id this process runs under: `Some` only inside a Flatpak sandbox (Flatpak
/// puts `/.flatpak-info` there, `info_file_exists`) and only when `FLATPAK_ID` has an app id's
/// shape, at most 255 bytes of three or more dot-separated parts, each starting with a letter or
/// `_` and holding only ASCII letters, digits, `_` and `-`. A desktop shortcut then runs
/// `flatpak run <id>`: the sandbox's own path does not exist on the host, and an id of this
/// shape needs no quoting in a command line.
pub fn flatpak_app_id(flatpak_id: Option<OsString>, info_file_exists: bool) -> Option<String> {
    if !info_file_exists {
        return None;
    }
    let id = flatpak_id?.into_string().ok()?;
    let part_ok = |part: &str| {
        let mut chars = part.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    let well_formed = id.len() <= 255 && id.split('.').count() >= 3 && id.split('.').all(part_ok);
    well_formed.then_some(id)
}

/// [`flatpak_app_id`] for this process: always `None` outside Linux.
pub fn this_flatpak() -> Option<String> {
    #[cfg(target_os = "linux")]
    return flatpak_app_id(
        std::env::var_os("FLATPAK_ID"),
        Path::new("/.flatpak-info").exists(),
    );
    #[cfg(not(target_os = "linux"))]
    None
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

    #[test]
    fn flatpak_names_flatpak_run() {
        assert_eq!(
            flatpak_app_id(Some("io.github.its30na.redactor".into()), true).as_deref(),
            Some("io.github.its30na.redactor")
        );
        assert_eq!(
            flatpak_app_id(Some("org.Example_App.my-tool".into()), true).as_deref(),
            Some("org.Example_App.my-tool")
        );
    }

    #[test]
    fn flatpak_id_needs_the_info_file() {
        assert_eq!(
            flatpak_app_id(Some("io.github.its30na.redactor".into()), false),
            None
        );
        assert_eq!(flatpak_app_id(None, true), None);
    }

    #[test]
    fn flatpak_id_must_look_like_an_app_id() {
        let long = format!("io.github.{}", "a".repeat(246));
        assert_eq!(long.len(), 256);
        for odd in [
            "",
            "redactor",
            "a.b",
            "io.github.x;rm",
            "io..x.y",
            "1io.x.y",
            "io.1x.y",
            "io.x.",
            ".io.x.y",
            "io.x.y z",
            "io.x.\u{e9}",
            long.as_str(),
        ] {
            assert_eq!(flatpak_app_id(Some(odd.into()), true), None, "{odd:?}");
        }
        // 255 bytes is still an id.
        let longest = format!("io.github.{}", "a".repeat(245));
        assert_eq!(
            flatpak_app_id(Some(longest.clone().into()), true),
            Some(longest)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_flatpak_id_that_is_not_unicode_is_not_one() {
        use std::os::unix::ffi::OsStringExt;
        let odd = OsString::from_vec(b"io.github.red\xffactor".to_vec());
        assert_eq!(flatpak_app_id(Some(odd), true), None);
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
