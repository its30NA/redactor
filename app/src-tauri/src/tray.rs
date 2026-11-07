//! The tray icon: Open, Scrub clipboard, Screen-share shield, Wipe, Quit.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::app;
use crate::state;
use crate::wipe::WipeReason;

/// Held in app state so the shield check mark can follow Settings.
pub struct ShieldItem(pub CheckMenuItem<Wry>);

pub fn build(handle: &AppHandle, shield_on: bool) -> tauri::Result<()> {
    let open = MenuItem::with_id(handle, "open", "Open Redactor", true, None::<&str>)?;
    let scrub = MenuItem::with_id(handle, "scrub", "Scrub clipboard", true, None::<&str>)?;
    let shield = CheckMenuItem::with_id(
        handle,
        "shield",
        "Screen-share shield",
        true,
        shield_on,
        None::<&str>,
    )?;
    let wipe = MenuItem::with_id(handle, "wipe", "Wipe now", true, None::<&str>)?;
    let quit = MenuItem::with_id(handle, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(handle)?;
    let sep2 = PredefinedMenuItem::separator(handle)?;
    let menu = Menu::with_items(handle, &[&open, &scrub, &shield, &sep, &wipe, &sep2, &quit])?;
    handle.manage(ShieldItem(shield));

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Redactor")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => app::show_main(app),
            "scrub" => {
                let app = app.clone();
                std::thread::spawn(move || {
                    let r = app::scrub_clipboard_now(&app);
                    app::notify(&app, &r.notice());
                });
            }
            "shield" => app::toggle_shield(app),
            "wipe" => state::wipe(app, WipeReason::Tray),
            "quit" => app::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                app::show_main(tray.app_handle());
            }
        });
    if let Some(icon) = handle.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(handle)?;
    Ok(())
}

pub fn set_shield_checked(app: &AppHandle, on: bool) {
    if let Some(item) = app.try_state::<ShieldItem>() {
        let _ = item.0.set_checked(on);
    }
}
