//! macOS: the pasteboard, the lock and sleep signals, the web view, and the platform the
//! shell gets.
#![allow(unsafe_code)]

use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSUserDefaults};

use super::hotkey_plugin::PluginHotkey;
use super::Platform;
use crate::os::Os;

pub mod clipboard;
pub mod lock;
pub mod webview;

/// Items AppKit adds to an Edit menu by itself, "Start Dictation" and "Emoji & Symbols",
/// unless these defaults say no. Both would put text into the editor from outside the page.
const DISABLED_MENU_ITEMS: [&str; 2] = [
    "NSDisabledDictationMenuItem",
    "NSDisabledCharacterPaletteMenuItem",
];

/// The macOS platform. Call it before Tauri builds the menu: it registers the defaults that
/// keep the OS's own items out of the Edit menu.
pub fn platform() -> Platform {
    disable_os_menu_items();
    Platform {
        name: "macos",
        os: Os::Mac,
        clipboard: Arc::new(clipboard::MacClipboard),
        lock: Box::new(lock::MacLock),
        webview: Box::new(webview::WkHardening),
        hotkey: Box::new(PluginHotkey),
        tray: true,
        // Since macOS 15.4 screen capture ignores the window flag Tauri's content protection
        // sets, and no public API prevents capture.
        shield: false,
        reopen: asks_for_the_window,
        tray_click_opens_menu: true,
        tray_notice:
            "Redactor is still running in the menu bar. Quit it from its menu there, or with Cmd+Q.",
    }
}

/// [`super::single_instance_usable`]: the plugin's socket path is free, or a file of ours (left
/// by our own run). A path we cannot even look at is not trusted.
pub fn single_instance_usable(identifier: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    let owner = match std::fs::symlink_metadata(super::single_instance_socket(identifier)) {
        Ok(meta) => Some(meta.uid()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return false,
    };
    super::socket_is_ours(owner, rustix::process::getuid().as_raw())
}

/// A click on the Dock icon, or opening the app again while it runs.
fn asks_for_the_window(event: &tauri::RunEvent) -> bool {
    matches!(event, tauri::RunEvent::Reopen { .. })
}

/// Registered as defaults, not written: a setting the user made for the app still wins.
fn disable_os_menu_items() {
    let yes = NSNumber::new_bool(true);
    let yes: &AnyObject = &yes;
    let keys = DISABLED_MENU_ITEMS.map(NSString::from_str);
    let defaults: Retained<NSDictionary<NSString, AnyObject>> =
        NSDictionary::from_slices(&[&*keys[0], &*keys[1]], &[yes, yes]);
    // SAFETY: both values are numbers, which a defaults dictionary may hold.
    unsafe { NSUserDefaults::standardUserDefaults().registerDefaults(&defaults) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mac_platform_says_what_it_offers() {
        let p = platform();
        assert_eq!(p.name, "macos");
        assert_eq!(p.os, Os::Mac);
        assert!(p.tray);
        // Since macOS 15.4 screen capture ignores the window flag the shield would set.
        assert!(!p.shield);
        assert!(p.tray_click_opens_menu);
        assert_eq!(
            p.tray_notice,
            "Redactor is still running in the menu bar. Quit it from its menu there, or with Cmd+Q."
        );
    }

    #[test]
    fn the_os_menu_items_are_switched_off() {
        let _ = platform();
        let defaults = NSUserDefaults::standardUserDefaults();
        for key in DISABLED_MENU_ITEMS {
            assert!(defaults.boolForKey(&NSString::from_str(key)), "{key}");
        }
    }
}
