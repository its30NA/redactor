//! Linux: the session decides what the app can use (X11 now, Wayland in phase 1-2d).

pub mod lock;
pub mod session;
pub mod tray;
pub mod wayland;
pub mod webview;
pub mod x11;

use std::sync::Arc;

use super::hotkey_plugin::PluginHotkey;
use super::{Platform, Unavailable};
use crate::os::Os;
use session::Session;
use x11::X11Clipboard;

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
        Session::X11 => x11_platform(),
    }
}

/// X11: the clipboard connects to `DISPLAY` now, so a server that is not there is refused
/// before any window exists.
fn x11_platform() -> Result<Platform, Unavailable> {
    let clipboard = X11Clipboard::connect(None).map_err(|_| NO_X_SERVER)?;
    // The window must live on the same X server as the clipboard and the hotkey. Left alone,
    // GTK prefers Wayland whenever a compositor socket exists at the default place, also when
    // `WAYLAND_DISPLAY` is not set (WSLg). Said before Tauri opens the display.
    gtk::gdk::set_allowed_backends("x11");
    Ok(Platform {
        name: "linux",
        os: Os::Linux,
        clipboard: Arc::new(clipboard),
        lock: Box::new(lock::LogindLock),
        webview: Box::new(webview::WebKitHardening),
        hotkey: Box::new(PluginHotkey),
        // Asked once, at start-up: a tray host that appears later is picked up at the next start.
        tray: tray::available(),
        // Tauri's content protection does nothing on Linux.
        shield: false,
        reopen: super::never_reopens,
        tray_click_opens_menu: false,
        tray_notice: super::TRAY_NOTICE,
    })
}
