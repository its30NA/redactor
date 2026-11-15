//! The platform layer: everything the shell needs from the operating system.
//!
//! The shell (`app`, `commands`, `state`, `tray`) talks only to [`Platform`], built once at
//! start-up by [`native`], or told by it why this desktop is not supported. Each OS lives in its
//! own submodule; only those may use `unsafe`.

use std::sync::Arc;

use crate::clipscrub::{ClipError, Clipboard};
use crate::hotkey::Hotkey;
use crate::os::Os;
use crate::wipe::WipeReason;

pub mod hotkey_plugin;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(not(any(windows, target_os = "linux")))]
pub mod unsupported;
#[cfg(windows)]
pub mod windows;

/// The system clipboard as the shell uses it.
pub trait SystemClipboard: Clipboard + Send + Sync {
    /// Copy safe text: ordinary text, it may enter the clipboard history.
    fn write_text(&self, text: &str) -> Result<(), ClipError>;

    /// Called on the way out, before the wipe. Where copied text lives in the app that copied
    /// it and dies with it (X11), this hands it to the desktop so it can still be pasted.
    /// Elsewhere the text stays anyway and there is nothing to do.
    fn keep_after_exit(&self) {}
}

/// Session lock and sleep notifications.
pub trait LockWatcher: Send + Sync {
    /// Starts watching for session lock and sleep. `on_event` runs on the watcher's own
    /// thread. Called once per process.
    fn spawn(&self, on_event: Box<dyn Fn(WipeReason) + Send + Sync>);
}

/// Webview settings the Tauri builder does not expose.
pub trait WebviewHardening: Send + Sync {
    /// The last step before the main window is built: settings the OS only takes before the
    /// web view exists. Most OSes need none.
    fn prepare<'a>(
        &self,
        builder: tauri::WebviewWindowBuilder<'a, tauri::Wry, tauri::AppHandle>,
    ) -> tauri::WebviewWindowBuilder<'a, tauri::Wry, tauri::AppHandle> {
        builder
    }

    /// Settings the Tauri builder does not expose, applied to the built window.
    fn harden(&self, window: &tauri::WebviewWindow);
}

/// The OS or another app refused the hotkey.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyTaken;

/// The system-wide hotkey.
pub trait GlobalHotkey: Send + Sync {
    fn register(&self, app: &tauri::AppHandle, hk: &Hotkey) -> Result<(), HotkeyTaken>;
    fn unregister(&self, app: &tauri::AppHandle, hk: &Hotkey);
}

/// What this OS offers the shell.
pub struct Platform {
    /// `app_info.platform`: "windows" or "linux".
    pub name: &'static str,
    /// Which OS's rules apply (hotkeys, settings folder, the UI's shortcuts).
    pub os: Os,
    pub clipboard: Arc<dyn SystemClipboard>,
    pub lock: Box<dyn LockWatcher>,
    pub webview: Box<dyn WebviewHardening>,
    pub hotkey: Box<dyn GlobalHotkey>,
    /// False where the desktop shows no tray: the window then never hides to it.
    pub tray: bool,
    /// True where the OS can hide the window from screenshots and screen sharing. Without it
    /// the screen-share shield is always off and the tray menu does not offer it.
    pub shield: bool,
    /// True when this run-loop event is the OS asking for the window back (the window is then
    /// shown again).
    pub reopen: fn(&tauri::RunEvent) -> bool,
    /// True where a left click on the tray icon opens its menu; false where it opens the window.
    pub tray_click_opens_menu: bool,
    /// Shown once, when closing the window hides it to the tray.
    pub tray_notice: &'static str,
}

/// The tray notice where the icon sits in a tray.
pub const TRAY_NOTICE: &str = "Redactor is still running in the tray. Quit it from the tray menu.";

/// For OSes that never ask for the window back.
pub fn never_reopens(_event: &tauri::RunEvent) -> bool {
    false
}

/// Why the desktop app cannot start here: one sentence that points to the `scrub` CLI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unavailable(pub &'static str);

/// The platform this build runs on.
#[cfg(windows)]
pub fn native() -> Result<Platform, Unavailable> {
    Ok(windows::platform())
}

/// The platform this build runs on: the session decides.
#[cfg(target_os = "linux")]
pub fn native() -> Result<Platform, Unavailable> {
    linux::platform()
}

/// Not yet: macOS gets its own platform in port phase 1-2b.
#[cfg(not(any(windows, target_os = "linux")))]
pub fn native() -> Result<Platform, Unavailable> {
    Err(unsupported::UNAVAILABLE)
}
