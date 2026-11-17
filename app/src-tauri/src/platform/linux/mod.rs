//! Linux: the session decides what the app can use, X11 or Wayland.

pub mod command_hotkey;
pub mod lock;
pub mod portal;
pub mod session;
pub mod tray;
pub mod wayland;
pub mod webview;
pub mod x11;

use std::sync::{mpsc, Arc};
use std::time::Duration;

use super::hotkey_plugin::PluginHotkey;
use super::{GlobalHotkey, Platform, SystemClipboard, Unavailable};
use crate::os::Os;
use command_hotkey::CommandHotkey;
use portal::{portal_version, PortalHotkey};
use session::Session;
use wayland::{DataControlClipboard, FocusedClipboard, Globals};
use x11::X11Clipboard;

const NO_DISPLAY: Unavailable =
    Unavailable("No desktop session found (DISPLAY is not set). Use the `scrub` CLI.");
const NO_X_SERVER: Unavailable = Unavailable("Could not reach the X server. Use the `scrub` CLI.");
const NO_COMPOSITOR: Unavailable =
    Unavailable("Could not reach the Wayland compositor. Use the `scrub` CLI.");

/// How long start-up waits for the session bus to say whether it has a shortcuts portal.
const PORTAL_PROBE: Duration = Duration::from_secs(5);

/// A test switch: set to `1`, the app uses the focused-window clipboard even where the
/// compositor offers data-control, so that clipboard can be tried on a compositor that has
/// both. It can only take a capability away. Read only by [`wayland_platform`].
const FOCUSED_ONLY: &str = "REDACTOR_WAYLAND_FOCUSED";

/// The Linux platform for the session this process runs in.
pub fn platform() -> Result<Platform, Unavailable> {
    match session::session(|name| std::env::var_os(name)) {
        Session::Wayland => wayland_platform(),
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

/// Wayland: a compositor that does not answer is refused before any window exists. The
/// clipboard works in the background where the compositor offers data-control, else only while
/// the window is in front. The hotkey goes through the desktop's shortcuts portal where the
/// session bus has one, else through a desktop shortcut that runs `--scrub-clipboard`.
fn wayland_platform() -> Result<Platform, Unavailable> {
    let globals = wayland::globals().map_err(|_| NO_COMPOSITOR)?;
    let focused_only = std::env::var_os(FOCUSED_ONLY).is_some_and(|v| v == "1");
    let clipboard: Arc<dyn SystemClipboard> = match clipboard_kind(&globals, focused_only) {
        // Offered a moment ago, yet refused now (no seat, a broken connection): the window's
        // clipboard still works.
        ClipboardKind::DataControl => match DataControlClipboard::connect() {
            Ok(c) => Arc::new(c),
            Err(_) => Arc::new(FocusedClipboard::new()),
        },
        ClipboardKind::Focused => Arc::new(FocusedClipboard::new()),
    };
    // The window must live on the compositor the clipboard talks to, never on XWayland. Said
    // before Tauri opens the display.
    gtk::gdk::set_allowed_backends("wayland");
    Ok(Platform {
        name: "linux",
        os: Os::Linux,
        clipboard,
        lock: Box::new(lock::LogindLock),
        webview: Box::new(webview::WebKitHardening),
        hotkey: wayland_hotkey(),
        // Asked once, at start-up: a tray host that appears later is picked up at the next start.
        tray: tray::available(),
        // Tauri's content protection does nothing on Linux.
        shield: false,
        reopen: super::never_reopens,
        tray_click_opens_menu: false,
        tray_notice: super::TRAY_NOTICE,
    })
}

/// Which clipboard the Wayland platform uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipboardKind {
    /// In the background, through data-control.
    DataControl,
    /// Only while the app's window is in front, through GTK.
    Focused,
}

/// Data-control where the compositor offers it, unless `focused_only` (the test switch).
fn clipboard_kind(globals: &Globals, focused_only: bool) -> ClipboardKind {
    if globals.data_control.is_some() && !focused_only {
        ClipboardKind::DataControl
    } else {
        ClipboardKind::Focused
    }
}

/// The portal hotkey where the session bus offers GlobalShortcuts, else the desktop-shortcut
/// command. The probe is bounded: a bus or portal that does not answer within
/// [`PORTAL_PROBE`] means the command.
fn wayland_hotkey() -> Box<dyn GlobalHotkey> {
    let portal = within(PORTAL_PROBE, || {
        let bus = zbus::blocking::Connection::session().ok()?;
        if !uses_portal(portal_version(&bus)) {
            return None;
        }
        PortalHotkey::new(bus).ok()
    });
    match portal {
        Some(p) => Box::new(p),
        None => Box::new(CommandHotkey),
    }
}

/// GlobalShortcuts version 1 is the first.
fn uses_portal(version: Option<u32>) -> bool {
    version.is_some_and(|v| v >= 1)
}

/// Runs `f` on a thread of its own and waits up to `deadline` for its answer. `None` when it
/// answers `None` or too late; a late thread finishes on its own and its answer is dropped.
fn within<T: Send + 'static>(
    deadline: Duration,
    f: impl FnOnce() -> Option<T> + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("startup-probe".into())
        .spawn(move || {
            let _ = tx.send(f());
        })
        .ok()?;
    rx.recv_timeout(deadline).ok().flatten()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::wayland::{DataControlKind, Globals};
    use super::*;

    fn offered(data_control: Option<DataControlKind>) -> Globals {
        Globals {
            data_control,
            seat: true,
        }
    }

    #[test]
    fn data_control_is_used_where_offered() {
        let g = offered(Some(DataControlKind::Wlr));
        assert_eq!(clipboard_kind(&g, false), ClipboardKind::DataControl);
        let g = offered(Some(DataControlKind::Ext));
        assert_eq!(clipboard_kind(&g, false), ClipboardKind::DataControl);
    }

    #[test]
    fn no_data_control_means_the_focused_clipboard() {
        assert_eq!(
            clipboard_kind(&offered(None), false),
            ClipboardKind::Focused
        );
    }

    #[test]
    fn the_test_switch_only_takes_data_control_away() {
        let g = offered(Some(DataControlKind::Wlr));
        assert_eq!(clipboard_kind(&g, true), ClipboardKind::Focused);
        assert_eq!(clipboard_kind(&offered(None), true), ClipboardKind::Focused);
    }

    #[test]
    fn the_portal_is_used_from_version_1() {
        assert!(!uses_portal(None));
        assert!(!uses_portal(Some(0)));
        assert!(uses_portal(Some(1)));
        assert!(uses_portal(Some(2)));
    }

    #[test]
    fn within_hands_back_the_answer() {
        assert_eq!(within(Duration::from_secs(5), || Some(3)), Some(3));
        assert_eq!(within(Duration::from_secs(5), || None::<u32>), None);
    }

    #[test]
    fn within_gives_up_at_the_deadline() {
        let start = Instant::now();
        let late = within(Duration::from_millis(50), || {
            std::thread::sleep(Duration::from_secs(10));
            Some(1)
        });
        assert_eq!(late, None);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "waited for the late answer"
        );
    }
}
