//! The Tauri app: window, plugins, hotkey, idle timer, lock watcher and settings changes.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::webview::NewWindowResponse;
use tauri::{
    AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};
use tauri_plugin_global_shortcut::ShortcutState;
use tauri_plugin_notification::NotificationExt;

use crate::cli;
use crate::clipscrub::{self, ClipboardScrub};
use crate::commands;
use crate::engines::Profile;
use crate::hotkey::{self, Hotkey};
use crate::pending::{Take, Timer, When};
use crate::platform::{self, HotkeyMode};
use crate::settings::{Settings, SettingsFile};
use crate::state::{self, AppState, Shared};
use crate::tray;
use crate::wipe::WipeReason;

pub const MAIN: &str = "main";

/// Starts the app. Returns only when this desktop is not supported, before any window or
/// Tauri call; otherwise the process ends inside the event loop.
pub fn run() -> Result<(), platform::Unavailable> {
    let platform = platform::native()?;
    let settings_file = SettingsFile::for_os(platform.os);
    let settings = settings_file
        .as_ref()
        .map(SettingsFile::load)
        .unwrap_or_default()
        .without_unsupported(platform.shield);
    let shared: Shared = Arc::new(AppState::new(settings, settings_file, platform));
    let reopen = shared.platform.reopen;

    let app = tauri::Builder::default()
        // Must come first: a second launch focuses this window, or cleans the clipboard when
        // it carries `--scrub-clipboard`, and exits.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if cli::scrub_requested(&args) {
                hotkey_pressed(app);
            } else {
                show_main(app);
            }
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        hotkey_pressed(app);
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
            commands::scrub_clipboard,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let st = state::state(&handle);
            let settings = st.settings();
            create_main_window(&handle, &settings)?;
            if st.platform.tray {
                let shield = st.platform.shield.then_some(settings.content_protection);
                tray::build(&handle, shield, st.platform.tray_click_opens_menu)?;
            }
            if st.platform.hotkey.mode() == HotkeyMode::Portal {
                start_hotkey_in_background(&handle);
            } else {
                st.platform.hotkey.listen(&handle, Arc::new(hotkey_pressed));
                if let Ok(hk) = Hotkey::parse(&settings.hotkey, st.platform.os) {
                    // Another app may hold the key; the app still runs, and Settings can pick
                    // another one.
                    let _ = st.platform.hotkey.register(&handle, &hk);
                }
            }
            start_idle_timer(handle.clone());
            let lock_handle = handle.clone();
            st.platform.lock.spawn(Box::new(move |reason| {
                let st = state::state(&lock_handle);
                if st.settings().wipe_on_lock && st.wiper.has_content() {
                    state::wipe(&lock_handle, reason);
                }
            }));
            // Build the engine now, off the UI thread, so the first scan is fast.
            let warm = state::state(&handle);
            std::thread::spawn(move || {
                warm.engines.get(true);
            });
            if cli::scrub_requested(&args()) {
                hold_start_scrub(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != MAIN {
                return;
            }
            if let WindowEvent::Focused(true) = event {
                scrub_if_pending(window.app_handle());
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let st = state::state(app);
                if st.settings().tray_mode && st.platform.tray {
                    api.prevent_close();
                    let _ = window.hide();
                    if !st.tray_notice_shown.swap(true, Ordering::SeqCst) {
                        notify(app, st.platform.tray_notice);
                    }
                } else {
                    quit(app);
                }
            }
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|_| std::process::abort());

    app.run(move |app, event| {
        if reopen(&event) {
            show_main(app);
        }
        if let RunEvent::Exit = event {
            let st = state::state(app);
            // Text the user copied from us stays pasteable after we are gone.
            st.platform.clipboard.keep_after_exit();
            // Zeroize the session secrets on the way out.
            st.wiper.wipe();
        }
    });
    Ok(())
}

/// The hotkey's start where the desktop binds it: that can take long (a person may be reading
/// the desktop's dialog), so it runs on a thread of its own, never on the main thread. It holds
/// the settings lock throughout, taken before `setup` goes on, so a settings change waits for
/// it and the key it binds is the one the settings name.
fn start_hotkey_in_background(app: &AppHandle) {
    let app = app.clone();
    let (held, lock_taken) = std::sync::mpsc::channel::<()>();
    let started = std::thread::Builder::new()
        .name("hotkey-start".into())
        .spawn(move || {
            let st = state::state(&app);
            let _one_at_a_time = st.settings_change.lock().unwrap_or_else(|e| e.into_inner());
            let _ = held.send(());
            st.platform.hotkey.listen(&app, Arc::new(hotkey_pressed));
            if let Ok(hk) = Hotkey::parse(&st.settings().hotkey, st.platform.os) {
                // The desktop may refuse the key; the app still runs, and Settings can ask for
                // another one.
                let _ = st.platform.hotkey.register(&app, &hk);
            }
        });
    if started.is_ok() {
        // Nothing else holds the lock while `setup` runs: this waits only for the thread to start.
        let _ = lock_taken.recv();
    }
}

fn create_main_window(app: &AppHandle, settings: &Settings) -> tauri::Result<WebviewWindow> {
    let st = state::state(app);
    // Where the OS cannot hide the window, it is never marked as hidden.
    let protected = st.platform.shield && settings.content_protection;
    let builder = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("Redactor")
        .inner_size(1280.0, 800.0)
        .min_inner_size(960.0, 640.0)
        .center()
        .decorations(false)
        .content_protected(protected)
        .incognito(true)
        .browser_extensions_enabled(false)
        .additional_browser_args(crate::browser_args::BROWSER_ARGS)
        .devtools(cfg!(debug_assertions))
        .on_navigation(allowed_origin)
        .on_new_window(|_url, _features| NewWindowResponse::Deny);
    let window = st.platform.webview.prepare(builder).build()?;
    st.platform.webview.harden(&window);
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
    let st = state::state(app);
    st.platform.clipboard.keep_after_exit();
    st.wiper.wipe();
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

/// This process's arguments. One that is not valid Unicode is kept lossily (it is never the
/// flag), where `std::env::args` would panic.
fn args() -> Vec<String> {
    std::env::args_os()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

/// A hotkey press, the tray's scrub item, `--scrub-clipboard` or the window's "Clean now".
/// Where the clipboard needs the window in front and it is not, the window comes back and the
/// scrub waits for focus, up to [`crate::pending::PENDING_FOR`]. Otherwise the clipboard is
/// scrubbed now, off the calling thread.
pub fn hotkey_pressed(app: &AppHandle) {
    let st = state::state(app);
    // Numbered before any scrub can start for it, so the scrub that covers it reads this number.
    let press = st.presses.fetch_add(1, Ordering::SeqCst) + 1;
    if st.platform.clipboard.needs_focus() && !main_is_focused(app) {
        // The waiting notice first: a fast focus can run the scrub as soon as it is armed, and
        // its result must not come before the notice it replaces.
        commands::emit_scrub_waiting(app, press);
        // Armed before the window is asked for, so the focus-in can never come first.
        if st.pending.arm(When::now()) && !spawn_expiry_timer(app) {
            // No timer: the press expired at once and the user was told; nothing waits.
            return;
        }
        // A focus-in between the check above and the arm found nothing to run: run it now.
        if main_is_focused(app) {
            scrub_if_pending(app);
            return;
        }
        show_main(app);
        notify(app, "Bring Redactor to the front to clean the clipboard");
    } else {
        // A focus-in still queued from an earlier press must not scrub a second time.
        st.pending.clear();
        scrub_and_notify(app);
    }
}

/// How long a `--scrub-clipboard` the app started with waits for the page at most.
const START_SCRUB_WAIT: Duration = Duration::from_secs(10);

/// `--scrub-clipboard` started the app. The page cannot hear the scrub's events before it
/// listens, so the scrub waits for its first `app_info` (asked for once it listens), or
/// [`START_SCRUB_WAIT`] if the page never asks.
fn hold_start_scrub(app: &AppHandle) {
    state::state(app).start_scrub.store(true, Ordering::SeqCst);
    let handle = app.clone();
    let _ = std::thread::Builder::new()
        .name("start-scrub".into())
        .spawn(move || {
            std::thread::sleep(START_SCRUB_WAIT);
            run_start_scrub(&handle);
        });
}

/// Runs the held `--scrub-clipboard`, once: the page listens now, or it waited long enough.
pub fn run_start_scrub(app: &AppHandle) {
    if state::state(app).start_scrub.swap(false, Ordering::SeqCst) {
        hotkey_pressed(app);
    }
}

/// The one thread that tells the user a press waited for the window in vain. The window may
/// never come to the front, and a desktop may drop the notification, so it also tells the window
/// (it may be on screen, just not in front); the next focus does not notify a second time.
/// False: the thread could not start, so the press expired at once and the user was told.
fn spawn_expiry_timer(app: &AppHandle) -> bool {
    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("pending-expiry".into())
        .spawn(move || {
            let st = state::state(&handle);
            let mut timer = st.pending.timer();
            loop {
                match timer.step(When::now()) {
                    Timer::Sleep(left) => std::thread::sleep(left),
                    Timer::Expired => {
                        notify(&handle, NOT_CLEANED);
                        commands::emit_scrub_expired(&handle);
                        break;
                    }
                    Timer::Done => break,
                }
            }
        });
    if spawned.is_ok() {
        return true;
    }
    if state::state(app).pending.timer_failed(When::now()) {
        notify(app, NOT_CLEANED);
        commands::emit_scrub_expired(app);
    }
    false
}

/// A press waited for the window in vain.
const NOT_CLEANED: &str = "Not cleaned: Redactor's window did not come to the front";

/// The main window came to the front: runs the scrub a hotkey press left waiting, or says in
/// the window that it expired (the desktop may have dropped the notification).
fn scrub_if_pending(app: &AppHandle) {
    match state::state(app).pending.take_on_focus(When::now()) {
        Take::Run => scrub_and_notify(app),
        // The timer notified already; only the window still has to learn it.
        Take::Expired => commands::emit_scrub_expired(app),
        // The timer has not spoken (it runs late): this is the only notice.
        Take::Lapsed => {
            notify(app, NOT_CLEANED);
            commands::emit_scrub_expired(app);
        }
        Take::Nothing => {}
    }
}

fn main_is_focused(app: &AppHandle) -> bool {
    app.get_webview_window(MAIN)
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

/// Scrubs the clipboard on a new thread and shows the result as a notification. While a scrub
/// waits to run already, it covers this call too: nothing more starts.
fn scrub_and_notify(app: &AppHandle) {
    let st = state::state(app);
    if !st.scrubs.enqueue() {
        return;
    }
    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("clipboard-scrub".into())
        .spawn(move || {
            let r = state::state(&handle)
                .scrubs
                .run(|| scrub_clipboard_now(&handle));
            notify(&handle, &r.notice());
        });
    if spawned.is_err() {
        st.scrubs.not_started();
    }
}

/// Scrubs the clipboard with the current settings and tells the UI. Called one at a time.
fn scrub_clipboard_now(app: &AppHandle) -> ClipboardScrub {
    let st = state::state(app);
    // Every press so far came before the clipboard is read: this scrub covers them.
    let press = st.presses.load(Ordering::SeqCst);
    let s = st.settings();
    let engine = st.engines.get(s.strip_invisible);
    let r = clipscrub::scrub(
        &*st.platform.clipboard,
        &engine,
        s.sensitivity,
        s.pii,
        s.strip_invisible,
    );
    commands::emit_scrub(app, &r, press);
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

/// Validates, applies and stores new settings. The hotkey changes first: if another app holds
/// the new key, the old one stays registered and nothing else changes. Where the desktop binds
/// the key (the portal) and refuses the new one, nothing else changes either, and no hotkey is
/// set until another one is picked.
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
    let os = st.platform.os;
    let next = change(&prev)
        .validated(os)
        .map_err(|e| e.to_string())?
        .without_unsupported(st.platform.shield);

    if next.hotkey != prev.hotkey {
        let new_hk = Hotkey::parse(&next.hotkey, os).map_err(|e| e.to_string())?;
        let old = Hotkey::parse(&prev.hotkey, os).ok();
        let register = |h: &Hotkey| st.platform.hotkey.register(app, h).is_ok();
        let unregister = |h: &Hotkey| st.platform.hotkey.unregister(app, h);
        if st.platform.hotkey.mode() == HotkeyMode::Portal {
            // A refusal leaves no key: the old one ended with its session when it was unbound.
            if !hotkey::rebind(old.as_ref(), &new_hk, register, unregister) {
                return Err(format!(
                    "Your desktop did not accept {}. No hotkey is set now; pick another in Settings.",
                    next.hotkey
                ));
            }
        } else if !hotkey::replace_registration(old.as_ref(), &new_hk, register, unregister) {
            return Err(format!(
                "Another app already uses {}. Kept {}.",
                next.hotkey, prev.hotkey
            ));
        }
    }
    if st.platform.shield && next.content_protection != prev.content_protection {
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
