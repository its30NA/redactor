// The Tauri build step: the generated context and the IPC command list.

include!("src/ipc_names.rs");

fn main() {
    let attrs = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attrs).expect("tauri build step");
}
