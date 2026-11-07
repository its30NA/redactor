// Windows only: a version resource for scrub.exe, so its Properties show the product name, version
// and description (code-signing services require them). Elsewhere this does nothing.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            let mut res = tauri_winres::WindowsResource::new();
            res.set("ProductName", "Redactor")
                .set("FileDescription", "scrub - Redactor command-line tool")
                .set("InternalName", "scrub")
                .set("OriginalFilename", "scrub.exe")
                .set(
                    "LegalCopyright",
                    "Copyright (c) 2026 Sina Kashani. MIT License.",
                );
            res.compile_for(&["scrub"])
                .expect("version resource for scrub.exe");
        }
    }
}
