//! The shell never names an operating system: it talks to `platform::Platform`.

fn src(rel: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn the_shell_names_no_operating_system() {
    for file in [
        "src/app.rs",
        "src/commands.rs",
        "src/state.rs",
        "src/tray.rs",
    ] {
        let text = src(file);
        for banned in [
            "cfg(windows)",
            "cfg(target_os",
            "cfg(unix)",
            "platform::windows",
            "platform::unsupported",
            "crate::win",
            "WinClipboard",
        ] {
            assert!(!text.contains(banned), "{file} must not contain `{banned}`");
        }
    }
    assert!(!std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/win")
        .exists());
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
