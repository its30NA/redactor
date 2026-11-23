//! The platform layer: everything the shell needs from the operating system.
//!
//! The shell (`app`, `commands`, `state`, `tray`) talks only to [`Platform`], built once at
//! start-up by [`native`], or told by it why this desktop is not supported. Each OS lives in its
//! own submodule; only those may use `unsafe`.

use std::sync::Arc;

use serde::Serialize;

use crate::clipscrub::{ClipError, Clipboard};
use crate::hotkey::Hotkey;
use crate::os::Os;
use crate::wipe::WipeReason;

pub mod hotkey_plugin;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
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

    /// True where the clipboard can be read and written only while Redactor's window is in
    /// front. A hotkey press then shows the window and the scrub waits for focus.
    fn needs_focus(&self) -> bool {
        false
    }
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

/// How a hotkey press reaches the app (`app_info.hotkey`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyMode {
    /// The app registers the key combination from Settings itself.
    Keys,
    /// The desktop's shortcuts portal binds the key; the user confirms it there.
    Portal,
    /// The app holds no key: a desktop shortcut runs `redactor --scrub-clipboard`.
    Command,
}

/// The system-wide hotkey.
pub trait GlobalHotkey: Send + Sync {
    fn register(&self, app: &tauri::AppHandle, hk: &Hotkey) -> Result<(), HotkeyTaken>;
    fn unregister(&self, app: &tauri::AppHandle, hk: &Hotkey);

    /// How a press reaches the app.
    fn mode(&self) -> HotkeyMode {
        HotkeyMode::Keys
    }

    /// Hands over what a press does. Called once, in `setup`, before the first `register`.
    /// Where presses arrive some other way (the global-shortcut plugin's handler) this does
    /// nothing.
    fn listen(
        &self,
        _app: &tauri::AppHandle,
        _on_press: Arc<dyn Fn(&tauri::AppHandle) + Send + Sync>,
    ) {
    }
}

/// What this OS offers the shell.
pub struct Platform {
    /// `app_info.platform`: "windows", "macos" or "linux".
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

/// The platform this build runs on.
#[cfg(target_os = "macos")]
pub fn native() -> Result<Platform, Unavailable> {
    Ok(macos::platform())
}

/// Any other system: the shell compiles, and refuses to start.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn native() -> Result<Platform, Unavailable> {
    Err(unsupported::UNAVAILABLE)
}

/// The app's identifier, as `tauri.conf.json` has it (a test checks they agree).
pub const IDENTIFIER: &str = "io.github.its30na.redactor";

/// True when the single-instance plugin may run: a second launch then hands its arguments to
/// this one and exits. On macOS the plugin talks over a socket at a fixed path in `/tmp`
/// ([`single_instance_socket`]), and exits the new launch as soon as anything there accepts a
/// connection: another user of the same Mac could make every start of Redactor exit. There it
/// is used only when that path is missing or ours. Elsewhere it is always used.
#[cfg(target_os = "macos")]
pub fn single_instance_usable(identifier: &str) -> bool {
    macos::single_instance_usable(identifier)
}

/// True: only macOS puts the plugin's socket where another user can take it.
#[cfg(not(target_os = "macos"))]
pub fn single_instance_usable(_identifier: &str) -> bool {
    true
}

/// Where tauri-plugin-single-instance 2.5 keeps its socket on macOS (without its `semver`
/// feature): `/tmp/<identifier with . and - as _>_si.sock`.
pub fn single_instance_socket(identifier: &str) -> std::path::PathBuf {
    let name = identifier.replace(['.', '-'], "_");
    std::path::PathBuf::from(format!("/tmp/{name}_si.sock"))
}

/// The socket may be trusted: there is none yet (`owner` is `None`), or `own` user made it.
pub fn socket_is_ours(owner: Option<u32>, own: u32) -> bool {
    match owner {
        None => true,
        Some(uid) => uid == own,
    }
}

/// Switches core dumps off for this process and every process it starts (the web view's helper
/// processes inherit the limit). A crash aborts the process, and a core file would put its memory
/// on disk: clipboard text, both panes, the session secrets. The limit stops the kernel writing a
/// core file and systemd-coredump storing one; it is no guarantee against every crash reporter.
/// Called first thing in `main`. An error is ignored: the app runs either way.
#[cfg(target_os = "linux")]
pub fn no_core_dumps() {
    use rustix::process::{setrlimit, Resource, Rlimit};
    let none = Rlimit {
        current: Some(0),
        maximum: Some(0),
    };
    let _ = setrlimit(Resource::Core, none);
}

/// Elsewhere nothing to do here.
#[cfg(not(target_os = "linux"))]
pub fn no_core_dumps() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_single_instance_socket_is_trusted_only_when_missing_or_ours() {
        assert!(socket_is_ours(None, 501), "missing: the plugin makes it");
        assert!(socket_is_ours(Some(501), 501), "left from our own run");
        assert!(!socket_is_ours(Some(502), 501), "another user's");
        assert!(!socket_is_ours(Some(0), 501), "root's");
    }

    #[test]
    fn the_single_instance_socket_is_where_the_plugin_puts_it() {
        assert_eq!(
            single_instance_socket(IDENTIFIER),
            std::path::Path::new("/tmp/io_github_its30na_redactor_si.sock")
        );
        assert_eq!(
            single_instance_socket("a.b-c"),
            std::path::Path::new("/tmp/a_b_c_si.sock")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn no_core_dumps_sets_the_core_limit_to_zero() {
        use rustix::process::{getrlimit, Resource, Rlimit};
        no_core_dumps();
        assert_eq!(
            getrlimit(Resource::Core),
            Rlimit {
                current: Some(0),
                maximum: Some(0),
            }
        );
    }
}
