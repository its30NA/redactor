//! The hotkey where the desktop has no shortcuts portal: the app holds no key. The user binds a
//! key in the desktop's own keyboard settings to the command Settings shows (this program with
//! `--scrub-clipboard`), and that press reaches the running app through the single-instance
//! plugin.

use tauri::AppHandle;

use crate::hotkey::Hotkey;
use crate::platform::{GlobalHotkey, HotkeyMode, HotkeyTaken};

/// No key of its own; presses come as `--scrub-clipboard`.
pub struct CommandHotkey;

impl GlobalHotkey for CommandHotkey {
    fn register(&self, _app: &AppHandle, _hk: &Hotkey) -> Result<(), HotkeyTaken> {
        Ok(())
    }

    fn unregister(&self, _app: &AppHandle, _hk: &Hotkey) {}

    fn mode(&self) -> HotkeyMode {
        HotkeyMode::Command
    }
}
