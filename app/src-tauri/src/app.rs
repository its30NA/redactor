//! The Tauri app: window, plugins, hotkey, idle timer, lock watcher and settings changes.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::webview::NewWindowResponse;
use tauri::{
    AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_notification::NotificationExt;

use crate::clipscrub::{self, ClipboardScrub};
use crate::commands;
use crate::engines::Profile;
use crate::hotkey::{Hotkey, Key};
use crate::settings::{Settings, SettingsFile};
use crate::state::{self, AppState, Shared};
use crate::tray;
use crate::win;
use crate::wipe::WipeReason;

pub const MAIN: &str = "main";

pub fn run() {
    let settings_file = SettingsFile::in_appdata();
    let settings = settings_file
        .as_ref()
        .map(SettingsFile::load)
        .unwrap_or_default();
    let shared: Shared = Arc::new(AppState::new(settings, settings_file));

    let app = tauri::Builder::default()
        // Must come first: a second launch focuses this window and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let app = app.clone();
                        std::thread::spawn(move || {
                            let r = scrub_clipboard_now(&app);
                            notify(&app, &r.notice());
                        });
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            commands::scan,
            commands::read_clipboard,
            commands::copy_text,
            commands::open_text_file,
            commands::save_text_file,
            commands::session_list,
            commands::session_remember,
            commands::session_add,
            commands::session_forget,
            commands::session_clear,
            commands::get_settings,
            commands::set_settings,
            commands::app_info,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = state::state(&handle).settings();
            create_main_window(&handle, &settings)?;
            tray::build(&handle, settings.content_protection)?;
            if let Ok(hk) = Hotkey::parse(&settings.hotkey) {
                // Another app may hold the key; the app still runs, and Settings can pick
                // another one.
                let _ = handle.global_shortcut().register(shortcut(&hk));
            }
            start_idle_timer(handle.clone());
            let lock_handle = handle.clone();
            win::lock::spawn(move |reason| {
                let st = state::state(&lock_handle);
                if st.settings().wipe_on_lock && st.wiper.has_content() {
                    state::wipe(&lock_handle, reason);
                }
            });
            // Build the engine now, off the UI thread, so the first scan is fast.
            let warm = state::state(&handle);
            std::thread::spawn(move || {
                warm.engines.get(true);
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != MAIN {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let st = state::state(app);
                if st.settings().tray_mode {
                    api.prevent_close();
                    let _ = window.hide();
                    if !st.tray_notice_shown.swap(true, Ordering::SeqCst) {
                        notify(
                            app,
                            "Redactor is still running in the tray. Quit it from the tray menu.",
                        );
                    }
                } else {
                    quit(app);
                }
            }
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|_| std::process::abort());

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            // Zeroize the session secrets on the way out.
            state::state(app).wiper.wipe();
        }
    });
}

fn create_main_window(app: &AppHandle, settings: &Settings) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("Redactor")
        .inner_size(1280.0, 800.0)
        .min_inner_size(960.0, 640.0)
        .center()
        .decorations(false)
        .content_protected(settings.content_protection)
        .incognito(true)
        .browser_extensions_enabled(false)
        .additional_browser_args(crate::browser_args::BROWSER_ARGS)
        .devtools(cfg!(debug_assertions))
        .on_navigation(allowed_origin)
        .on_new_window(|_url, _features| NewWindowResponse::Deny)
        .build()?;
    win::webview::harden(&window);
    Ok(window)
}

/// Only the app's own origin (and the dev server in debug builds) may load in the window.
fn allowed_origin(url: &tauri::Url) -> bool {
    match (url.scheme(), url.host_str()) {
        ("http" | "https", Some("tauri.localhost" | "isolation.localhost")) => true,
        ("tauri" | "isolation", _) => true,
        ("http", Some("localhost")) => cfg!(debug_assertions) && url.port() == Some(5173),
        _ => false,
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn quit(app: &AppHandle) {
    state::state(app).wiper.wipe();
    app.exit(0);
}

pub fn notify(app: &AppHandle, body: &str) {
    let _ = app
        .notification()
        .builder()
        .title("Redactor")
        .body(body)
        .show();
}

/// Scrubs the clipboard with the current settings and tells the UI.
pub fn scrub_clipboard_now(app: &AppHandle) -> ClipboardScrub {
    let st = state::state(app);
    let _one_at_a_time = st.scrub_lock.lock().unwrap_or_else(|e| e.into_inner());
    let s = st.settings();
    let engine = st.engines.get(s.strip_invisible);
    let r = clipscrub::scrub(
        &win::clipboard::WinClipboard,
        &engine,
        s.sensitivity,
        s.pii,
        s.strip_invisible,
    );
    commands::emit_scrub(app, &r);
    r
}

fn start_idle_timer(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(15));
        let st = state::state(&app);
        let minutes = st.settings().auto_wipe_minutes;
        if st.wiper.idle_due(minutes, Instant::now()) {
            state::wipe(&app, WipeReason::Idle);
        }
    });
}

pub fn shortcut(hk: &Hotkey) -> Shortcut {
    let mut mods = Modifiers::empty();
    if hk.ctrl {
        mods |= Modifiers::CONTROL;
    }
    if hk.shift {
        mods |= Modifiers::SHIFT;
    }
    if hk.alt {
        mods |= Modifiers::ALT;
    }
    Shortcut::new(Some(mods), code(hk.key))
}

fn code(k: Key) -> Code {
    const LETTERS: [Code; 26] = [
        Code::KeyA,
        Code::KeyB,
        Code::KeyC,
        Code::KeyD,
        Code::KeyE,
        Code::KeyF,
        Code::KeyG,
        Code::KeyH,
        Code::KeyI,
        Code::KeyJ,
        Code::KeyK,
        Code::KeyL,
        Code::KeyM,
        Code::KeyN,
        Code::KeyO,
        Code::KeyP,
        Code::KeyQ,
        Code::KeyR,
        Code::KeyS,
        Code::KeyT,
        Code::KeyU,
        Code::KeyV,
        Code::KeyW,
        Code::KeyX,
        Code::KeyY,
        Code::KeyZ,
    ];
    const DIGITS: [Code; 10] = [
        Code::Digit0,
        Code::Digit1,
        Code::Digit2,
        Code::Digit3,
        Code::Digit4,
        Code::Digit5,
        Code::Digit6,
        Code::Digit7,
        Code::Digit8,
        Code::Digit9,
    ];
    const FKEYS: [Code; 24] = [
        Code::F1,
        Code::F2,
        Code::F3,
        Code::F4,
        Code::F5,
        Code::F6,
        Code::F7,
        Code::F8,
        Code::F9,
        Code::F10,
        Code::F11,
        Code::F12,
        Code::F13,
        Code::F14,
        Code::F15,
        Code::F16,
        Code::F17,
        Code::F18,
        Code::F19,
        Code::F20,
        Code::F21,
        Code::F22,
        Code::F23,
        Code::F24,
    ];
    match k {
        Key::Letter(c) => LETTERS[(c as u8 - b'A') as usize],
        Key::Digit(d) => DIGITS[d as usize],
        Key::F(n) => FKEYS[(n - 1) as usize],
        Key::Space => Code::Space,
        Key::Insert => Code::Insert,
        Key::Delete => Code::Delete,
        Key::Home => Code::Home,
        Key::End => Code::End,
        Key::PageUp => Code::PageUp,
        Key::PageDown => Code::PageDown,
        Key::Backspace => Code::Backspace,
        Key::Enter => Code::Enter,
        Key::Tab => Code::Tab,
    }
}

/// Validates, applies and stores new settings. The hotkey changes first: if another app holds
/// the new key, the old one stays registered and nothing else changes.
pub fn apply_settings(app: &AppHandle, incoming: Settings) -> Result<Settings, String> {
    change_settings(app, |_| incoming)
}

/// Every settings change goes through here: `change` gets the current settings and returns
/// the new ones. One lock is held from reading the current settings to storing the new ones,
/// so changes from the UI and the tray never interleave. Never call this on the main thread:
/// registering a hotkey waits for the main thread while holding the lock.
fn change_settings(
    app: &AppHandle,
    change: impl FnOnce(&Settings) -> Settings,
) -> Result<Settings, String> {
    let st = state::state(app);
    let _one_at_a_time = st.settings_change.lock().unwrap_or_else(|e| e.into_inner());
    let prev = st.settings();
    let next = change(&prev).validated().map_err(|e| e.to_string())?;

    if next.hotkey != prev.hotkey {
        let new_hk = Hotkey::parse(&next.hotkey).map_err(|e| e.to_string())?;
        let gs = app.global_shortcut();
        let old = Hotkey::parse(&prev.hotkey).ok().map(|h| shortcut(&h));
        if let Some(old) = old {
            let _ = gs.unregister(old);
        }
        if gs.register(shortcut(&new_hk)).is_err() {
            if let Some(old) = old {
                let _ = gs.register(old);
            }
            return Err(format!(
                "Another app already uses {}. Kept {}.",
                next.hotkey, prev.hotkey
            ));
        }
    }
    if next.content_protection != prev.content_protection {
        if let Some(w) = app.get_webview_window(MAIN) {
            let _ = w.set_content_protected(next.content_protection);
        }
        tray::set_shield_checked(app, next.content_protection);
    }
    if st.engines.set_profile(Profile::of(&next)) {
        // Build the new engine in the background. The old engines are dropped now, so a scan
        // that comes first builds the new one itself.
        let warm = st.clone();
        let strip = next.strip_invisible;
        std::thread::spawn(move || {
            warm.engines.get(strip);
        });
    }
    st.store_settings(next.clone());
    Ok(next)
}

/// The tray's shield toggle: same path as Settings, then tell the UI. The toggle reads the
/// current value under the same lock that stores the new one, so quick clicks never get lost.
pub fn toggle_shield(app: &AppHandle) {
    let toggled = change_settings(app, |prev| {
        let mut s = prev.clone();
        s.content_protection = !s.content_protection;
        s
    });
    if let Ok(applied) = toggled {
        let _ = app.emit("settings-changed", applied);
    }
}
