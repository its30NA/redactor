//! The shell never names an operating system: it talks to `platform::Platform`.

fn src(rel: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The OS-specific spellings found in `text`.
fn os_mentions(text: &str) -> Vec<&'static str> {
    [
        // cfg(windows), cfg!(windows), cfg(not(windows)), cfg(all(windows, ..)), cfg_attr(windows, ..)
        "(windows",
        "(unix",
        "target_os",
        // The windows crate, std::os::windows / std::os::unix, raw libc calls.
        "windows::",
        "windows_sys",
        "std::os::",
        "libc::",
        "platform::windows",
        "platform::unsupported",
        "crate::win",
        "WinClipboard",
    ]
    .into_iter()
    .filter(|banned| text.contains(banned))
    .collect()
}

#[test]
fn the_shell_names_no_operating_system() {
    for file in [
        "src/app.rs",
        "src/commands.rs",
        "src/state.rs",
        "src/tray.rs",
    ] {
        let found = os_mentions(&src(file));
        assert!(found.is_empty(), "{file} must not contain {found:?}");
    }
    assert!(!std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/win")
        .exists());
}

#[test]
fn the_checker_catches_every_os_spelling() {
    for line in [
        "#[cfg(windows)]",
        "#[cfg(not(windows))]",
        "#[cfg(all(windows, debug_assertions))]",
        "#[cfg_attr(windows, allow(dead_code))]",
        "#[cfg(target_os = \"macos\")]",
        "#[cfg(unix)]",
        "if cfg!(windows) {",
        "if cfg!(target_os = \"linux\") {",
        "if cfg!(unix) {",
        "use windows::Win32::System::DataExchange::OpenClipboard;",
        "use windows_sys::Win32::Foundation::HWND;",
        "use std::os::windows::ffi::OsStrExt;",
        "use std::os::unix::fs::PermissionsExt;",
        "let pid = unsafe { libc::getpid() };",
        "crate::platform::windows::lock::spawn(f);",
        "use crate::platform::unsupported;",
        "let cb = WinClipboard;",
    ] {
        assert!(!os_mentions(line).is_empty(), "not caught: {line}");
    }
    for line in [
        ".devtools(cfg!(debug_assertions))",
        "st.platform.clipboard.write_text(&text)",
        "create_main_window(&handle, &settings)?;",
    ] {
        assert!(os_mentions(line).is_empty(), "false alarm: {line}");
    }
}

#[test]
fn unsafe_is_allowed_in_the_os_modules_only() {
    fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if std::fs::read_to_string(&p)
                .unwrap()
                .contains("allow(unsafe_code)")
            {
                out.push(
                    p.strip_prefix(env!("CARGO_MANIFEST_DIR"))
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut found = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    found.sort();
    assert_eq!(found, ["src/platform/windows/mod.rs"]);
    assert!(src("src/lib.rs").contains("#![deny(unsafe_code)]"));
}
