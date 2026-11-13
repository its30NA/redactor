//! Linux: the session decides what the app can use (X11 now, Wayland in phase 1-2d).

pub mod session;
pub mod webview;
pub mod x11;

use super::{Platform, Unavailable};
use session::Session;

const WAYLAND: Unavailable =
    Unavailable("The Redactor desktop app does not run on Wayland yet. Use the `scrub` CLI.");
const NO_DISPLAY: Unavailable =
    Unavailable("No desktop session found (DISPLAY is not set). Use the `scrub` CLI.");
const NO_X_SERVER: Unavailable = Unavailable("Could not reach the X server. Use the `scrub` CLI.");

/// The Linux platform for the session this process runs in.
pub fn platform() -> Result<Platform, Unavailable> {
    match session::session(|name| std::env::var_os(name)) {
        Session::Wayland => Err(WAYLAND),
        Session::None => Err(NO_DISPLAY),
        // The X11 backends are wired in once they exist (phase 1-2c, Task 8).
        Session::X11 => Err(NO_X_SERVER),
    }
}
