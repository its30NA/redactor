//! `redactor-app`: the Tauri 2 shell of Redactor.
//!
//! The platform-neutral parts (settings, session secrets, the engine bridge, hotkey and wipe
//! rules, file decoding) build and test on every platform. The shell itself (`app`, `commands`,
//! `tray`, `win`) is Windows-only, like the product.
//!
//! Nothing here logs, prints or stores scanned text.
// `unsafe` is allowed only in `win/`, which opts in with its own `allow`.
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

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod commands;
#[cfg(windows)]
mod state;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
pub mod win;

#[cfg(windows)]
pub use app::run;

/// A panic prints nothing (its message or location could quote input text) and the release
/// build aborts (`panic = "abort"` in the workspace profile).
pub fn install_quiet_panic_hook() {
    std::panic::set_hook(Box::new(|_| {}));
}
