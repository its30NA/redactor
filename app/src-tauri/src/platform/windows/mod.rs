//! Windows-only code: clipboard, clipboard history, session lock and sleep, WebView2 settings.
//! The only place in the app where `unsafe` is allowed; every block has a
//! `// SAFETY:` comment.
#![allow(unsafe_code)]

pub mod clipboard;
pub mod history;
pub mod lock;
pub mod webview;

use std::sync::Arc;

use super::hotkey_plugin::PluginHotkey;
use super::{LockWatcher, Platform, SystemClipboard, WebviewHardening};
use crate::clipscrub::ClipError;
use crate::os::Os;
use crate::wipe::WipeReason;
use clipboard::WinClipboard;

/// Windows as the shell sees it.
pub fn platform() -> Platform {
    Platform {
        os: Os::Windows,
        name: "windows",
        clipboard: Arc::new(WinClipboard),
        lock: Box::new(WinLock),
        webview: Box::new(WebView2Hardening),
        hotkey: Box::new(PluginHotkey),
        tray: true,
        shield: true,
        reopen: super::never_reopens,
        tray_click_opens_menu: false,
        tray_notice: super::TRAY_NOTICE,
    }
}

impl SystemClipboard for WinClipboard {
    fn write_text(&self, text: &str) -> Result<(), ClipError> {
        WinClipboard::write_text(self, text)
    }
}

/// `WTSRegisterSessionNotification` and power notifications, see [`lock`].
pub struct WinLock;

impl LockWatcher for WinLock {
    fn spawn(&self, on_event: Box<dyn Fn(WipeReason) + Send + Sync>) {
        lock::spawn(on_event);
    }
}

/// WebView2 settings, see [`webview`].
pub struct WebView2Hardening;

impl WebviewHardening for WebView2Hardening {
    fn harden(&self, window: &tauri::WebviewWindow) {
        webview::harden(window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tray_behaves_as_before() {
        let p = platform();
        assert!(!p.tray_click_opens_menu);
        assert_eq!(
            p.tray_notice,
            "Redactor is still running in the tray. Quit it from the tray menu."
        );
        assert!(!(p.reopen)(&tauri::RunEvent::Ready));
    }
}
