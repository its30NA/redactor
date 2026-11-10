// The app's IPC commands, in the snake_case form the UI invokes.
//
// One list for three places: `build.rs` registers them in the app manifest (each one then needs
// an explicit `allow-*` permission), `capabilities/main.json` grants exactly these, and the
// capability test checks that nothing else is granted. Included by `build.rs`, so plain Rust only.

/// Every command the UI may call.
pub const COMMANDS: &[&str] = &[
    "scan",
    "read_clipboard",
    "copy_text",
    "open_text_file",
    "save_text_file",
    "session_list",
    "session_remember",
    "session_add",
    "session_forget",
    "session_clear",
    "get_settings",
    "set_settings",
    "app_info",
];
