//! The platform layer: everything the shell needs from the operating system.
//!
//! The shell (`app`, `commands`, `state`, `tray`) talks only to [`Platform`], built once at
//! start-up by [`native`]. Each OS lives in its own submodule; only those may use `unsafe`.

use std::sync::Arc;

use crate::clipscrub::{ClipError, Clipboard};
use crate::hotkey::Hotkey;
use crate::wipe::WipeReason;

pub mod hotkey_plugin;
#[cfg(not(windows))]
pub mod unsupported;
#[cfg(windows)]
pub mod windows;

/// The system clipboard as the shell uses it.
pub trait SystemClipboard: Clipboard + Send + Sync {
    /// Copy safe text: ordinary text, it may enter the clipboard history.
    fn write_text(&self, text: &str) -> Result<(), ClipError>;
}

/// Session lock and sleep notifications.
pub trait LockWatcher: Send + Sync {
    /// Starts watching for session lock and sleep. `on_event` runs on the watcher's own
    /// thread. Called once per process.
    fn spawn(&self, on_event: Box<dyn Fn(WipeReason) + Send + Sync>);
}

/// Webview settings the Tauri builder does not expose.
pub trait WebviewHardening: Send + Sync {
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
    /// `app_info.platform`. "windows" today.
    pub name: &'static str,
    pub clipboard: Arc<dyn SystemClipboard>,
    pub lock: Box<dyn LockWatcher>,
    pub webview: Box<dyn WebviewHardening>,
    pub hotkey: Box<dyn GlobalHotkey>,
    /// False where the desktop shows no tray: the window then never hides to it.
    pub tray: bool,
}

/// The platform this build runs on.
#[cfg(windows)]
pub fn native() -> Platform {
    windows::platform()
}

/// The platform this build runs on: a placeholder until macOS and Linux get their own.
#[cfg(not(windows))]
pub fn native() -> Platform {
    unsupported::platform()
}
