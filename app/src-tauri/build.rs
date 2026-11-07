// The Tauri build step runs only for the Windows shell; elsewhere the crate is the
// platform-neutral library (see Cargo.toml).

#[cfg(windows)]
include!("src/ipc_names.rs");

fn main() {
    #[cfg(windows)]
    {
        let attrs = tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
        tauri_build::try_build(attrs).expect("tauri build step");
    }
}
