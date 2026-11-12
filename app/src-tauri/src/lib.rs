//! `redactor-app`: the Tauri 2 shell of Redactor.
//!
//! The platform-neutral parts (settings, session secrets, the engine bridge, hotkey and wipe
//! rules, file decoding) need no desktop. The shell (`app`, `commands`, `state`, `tray`) reaches
//! the operating system only through `platform`, and builds on Windows, macOS and Linux. It
//! starts on Windows only for now: elsewhere the binary points to the `scrub` CLI.
//!
//! Nothing here logs, prints or stores scanned text.
// `unsafe` is allowed only in `platform/windows/`, which opts in with its own `allow`.
#![deny(unsafe_code)]

pub mod bridge;
pub mod browser_args;
pub mod clipscrub;
pub mod engines;
pub mod hotkey;
pub mod ipc_names;
pub mod session;
pub mod settings;
pub mod textfile;
pub mod wipe;

pub mod platform;

mod app;
mod commands;
mod state;
mod tray;

pub use app::run;

/// A panic prints nothing (its message or location could quote input text) and the release
/// build aborts (`panic = "abort"` in the workspace profile).
pub fn install_quiet_panic_hook() {
    std::panic::set_hook(Box::new(|_| {}));
}
