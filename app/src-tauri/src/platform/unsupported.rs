//! The placeholder platform for macOS and Linux until their own code lands (port phases 1-2).
//! It lets the shell compile there; `main.rs` still refuses to start outside Windows. The
//! clipboard only ever fails, so nothing is read or written by accident.

use std::sync::Arc;

use super::hotkey_plugin::PluginHotkey;
use super::{LockWatcher, Platform, SystemClipboard, WebviewHardening};
use crate::clipscrub::{ClipError, ClipText, Clipboard};
use crate::wipe::WipeReason;

pub fn platform() -> Platform {
    Platform {
        name: "unsupported",
        clipboard: Arc::new(NoClipboard),
        lock: Box::new(NoLockWatcher),
        webview: Box::new(NoHardening),
        hotkey: Box::new(PluginHotkey),
        tray: true,
    }
}

/// A clipboard that refuses every operation.
struct NoClipboard;

impl Clipboard for NoClipboard {
    fn sequence(&self) -> u32 {
        0
    }

    fn read(&self) -> Result<ClipText, ClipError> {
        Err(ClipError)
    }

    fn write_if_unchanged(&self, _text: &str, _seq: u32) -> Result<bool, ClipError> {
        Err(ClipError)
    }

    fn delete_from_history(&self, _raw: &str) -> bool {
        false
    }
}

impl SystemClipboard for NoClipboard {
    fn write_text(&self, _text: &str) -> Result<(), ClipError> {
        Err(ClipError)
    }
}

struct NoLockWatcher;

impl LockWatcher for NoLockWatcher {
    fn spawn(&self, _on_event: Box<dyn Fn(WipeReason) + Send + Sync>) {}
}

struct NoHardening;

impl WebviewHardening for NoHardening {
    fn harden(&self, _window: &tauri::WebviewWindow) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_placeholder_clipboard_only_fails() {
        let p = platform();
        assert_eq!(p.name, "unsupported");
        assert!(crate::clipscrub::read_text_for_ui(&*p.clipboard).is_err());
        assert!(p.clipboard.write_text("x").is_err());
        let engine = redactor_core::Engine::new(redactor_core::EngineConfig::default()).unwrap();
        let r = crate::clipscrub::scrub(
            &*p.clipboard,
            &engine,
            redactor_core::Sensitivity::Balanced,
            false,
            true,
        );
        assert_eq!(r.status, crate::clipscrub::ClipStatus::Error);
    }
}
