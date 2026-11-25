//! The IPC commands. Each one is whitelisted in `capabilities/main.json` and listed in
//! `ipc_names.rs`. Heavy or blocking work runs on a blocking worker, never on the UI thread.
//! Errors are short user-facing strings and never quote the text.

use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroize;

use crate::app;
use crate::bridge::{self, ScanRequestIn, ScanResponseOut};
use crate::clipscrub::{self, ClipboardScrub};
use crate::platform::HotkeyMode;
use crate::session::SessionSecret;
use crate::settings::Settings;
use crate::state::{self, Shared};
use crate::textfile;

type Res<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|_| "Internal error.".to_string())
}

#[tauri::command]
pub async fn scan(state: State<'_, Shared>, req: ScanRequestIn) -> Res<ScanResponseOut> {
    let st = state.inner().clone();
    st.wiper.touch(!req.text.is_empty());
    blocking(move || {
        let engine = st.engines.get(req.strip_invisible);
        bridge::scan(&engine, &req)
        // `req` drops here and zeroizes the text.
    })
    .await
}

#[tauri::command]
pub async fn read_clipboard(state: State<'_, Shared>) -> Res<Option<String>> {
    state.wiper.touch(true);
    let st = state.inner().clone();
    blocking(move || clipscrub::read_text_for_ui(&*st.platform.clipboard))
        .await?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn copy_text(state: State<'_, Shared>, mut text: String) -> Res<()> {
    state.wiper.touch(false);
    let st = state.inner().clone();
    let r = blocking(move || {
        let r = st.platform.clipboard.write_text(&text);
        text.zeroize();
        r
    })
    .await?;
    r.map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct OpenedFile {
    name: String,
    text: String,
}

#[tauri::command]
pub async fn open_text_file(app: AppHandle, state: State<'_, Shared>) -> Res<Option<OpenedFile>> {
    state.wiper.touch(true);
    blocking(move || {
        let Some(picked) = app
            .dialog()
            .file()
            .set_title("Open a text file")
            .add_filter("Text", &TEXT_EXTENSIONS)
            .add_filter("All files", &["*"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = picked
            .into_path()
            .map_err(|_| "The file could not be read.".to_string())?;
        let bytes = textfile::read_bounded(&path).map_err(|e| e.to_string())?;
        let text = textfile::decode(bytes).map_err(|e| e.to_string())?;
        Ok(Some(OpenedFile {
            name: textfile::display_name(&path),
            text,
        }))
    })
    .await?
}

const TEXT_EXTENSIONS: [&str; 22] = [
    "txt",
    "log",
    "md",
    "env",
    "json",
    "yaml",
    "yml",
    "toml",
    "ini",
    "conf",
    "cfg",
    "xml",
    "csv",
    "sh",
    "ps1",
    "py",
    "js",
    "ts",
    "sql",
    "properties",
    "tf",
    "pem",
];

#[tauri::command]
pub async fn save_text_file(
    app: AppHandle,
    state: State<'_, Shared>,
    mut text: String,
    suggested_name: String,
) -> Res<bool> {
    state.wiper.touch(false);
    blocking(move || {
        let name = safe_file_name(&suggested_name);
        let picked = app
            .dialog()
            .file()
            .set_title("Save a safe copy")
            .set_file_name(name)
            .add_filter("Text", &["txt"])
            .blocking_save_file();
        let r = match picked {
            None => Ok(false),
            Some(p) => match p.into_path() {
                Ok(path) => std::fs::write(&path, text.as_bytes())
                    .map(|_| true)
                    .map_err(|_| "The file could not be saved.".to_string()),
                Err(_) => Err("The file could not be saved.".to_string()),
            },
        };
        text.zeroize();
        r
    })
    .await?
}

/// A plain file name: no folders, no characters Windows refuses.
fn safe_file_name(s: &str) -> String {
    let name: String = s
        .chars()
        .filter(|c| !c.is_control() && !r#"<>:"/\|?*"#.contains(*c))
        .take(120)
        .collect();
    let name = name.trim().trim_matches('.').to_string();
    if name.is_empty() {
        "redacted.safe.txt".to_string()
    } else {
        name
    }
}

#[tauri::command]
pub fn session_list(state: State<'_, Shared>) -> Vec<SessionSecret> {
    state.session.list()
}

#[tauri::command]
pub async fn session_remember(
    state: State<'_, Shared>,
    mut text: String,
    start: usize,
    end: usize,
    label: String,
) -> Res<Vec<SessionSecret>> {
    state.wiper.touch(true);
    let st = state.inner().clone();
    // A wipe (lock, sleep, idle, tray) that runs while this call waits for a worker wins.
    let since = st.session.generation();
    blocking(move || {
        let r = st
            .session
            .remember_unless_cleared(since, &text, start, end, &label);
        text.zeroize();
        r.map_err(|e| e.to_string())
    })
    .await?
}

#[tauri::command]
pub fn session_add(
    state: State<'_, Shared>,
    mut secret: String,
    label: String,
) -> Res<Vec<SessionSecret>> {
    state.wiper.touch(false);
    let r = state.session.add(&secret, &label);
    secret.zeroize();
    r.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn session_forget(state: State<'_, Shared>, id: String) -> Vec<SessionSecret> {
    state.wiper.touch(false);
    state.session.forget(&id)
}

#[tauri::command]
pub fn session_clear(state: State<'_, Shared>) -> Vec<SessionSecret> {
    state.wiper.touch(false);
    state.session.clear()
}

#[tauri::command]
pub fn get_settings(state: State<'_, Shared>) -> Settings {
    state.settings()
}

#[tauri::command]
pub async fn set_settings(app: AppHandle, settings: Settings) -> Res<Settings> {
    let s = blocking(move || app::apply_settings(&app, settings)).await??;
    Ok(s)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    engine_version: &'static str,
    rule_count: usize,
    platform: &'static str,
    /// The desktop shows the tray icon, so closing the window can keep the app running there.
    tray: bool,
    /// The OS can hide the window from screenshots: the screen-share shield exists here.
    shield: bool,
    /// How a hotkey press reaches the app.
    hotkey: HotkeyMode,
    /// The clipboard works only while the window is in front.
    clipboard_needs_focus: bool,
    /// The hotkey works now: false when it could not be registered at start (another app holds
    /// it, the desktop refused it), the desktop dropped it, or a new one was refused and none is
    /// held. Changes come as the `app-status` event.
    hotkey_active: bool,
    /// Something reports a screen lock here: false where no lock source could be subscribed at
    /// all (Linux: no logind session for this process, no screen saver on the session bus), so a
    /// lock does not wipe and suspend and idle still do. Left out while that is not known.
    #[serde(skip_serializing_if = "Option::is_none")]
    lock_watched: Option<bool>,
    /// The program a desktop shortcut runs with `--scrub-clipboard`; left out when its path is
    /// not Unicode, and inside a Flatpak (its path there does not exist on the host).
    #[serde(skip_serializing_if = "Option::is_none")]
    executable: Option<String>,
    /// The Flatpak app id when the app runs inside a Flatpak: the shortcut runs
    /// `flatpak run <id> --scrub-clipboard`. Left out elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    flatpak: Option<String>,
}

/// How long `app_info` waits at most for the lock watcher to know whether it has a lock source
/// (it asks the buses right at start; a bus that does not answer leaves the field out).
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

/// The page asks for this once it listens for the shell's events, so it also hands over a scrub
/// result and the notices that came before, and runs a `--scrub-clipboard` held for the page.
#[tauri::command]
pub async fn app_info(app: AppHandle, state: State<'_, Shared>) -> Res<AppInfo> {
    let st = state.inner().clone();
    blocking(move || {
        st.scrub_results
            .page_listens(|(r, press)| send_scrub(&app, &r, press));
        st.notices.page_listens(|kept| {
            for notice in kept.into_iter().flatten() {
                send_status(&app, Some(notice));
            }
        });
        app::run_start_scrub(&app);
        let flatpak = crate::cli::this_flatpak();
        AppInfo {
            version: env!("CARGO_PKG_VERSION"),
            engine_version: redactor_core::ENGINE_VERSION,
            rule_count: st.engines.rule_count(),
            platform: st.platform.name,
            tray: st.platform.tray,
            shield: st.platform.shield,
            hotkey: st.platform.hotkey.mode(),
            clipboard_needs_focus: st.platform.clipboard.needs_focus(),
            hotkey_active: st.hotkey_active.load(Ordering::SeqCst),
            lock_watched: st.platform.lock.listens(LOCK_WAIT),
            executable: flatpak.is_none().then(crate::cli::this_program).flatten(),
            flatpak,
        }
    })
    .await
}

/// The window's "Scrub now": scrubs the clipboard the way a hotkey press does. The result
/// comes as the `clipboard-scrubbed` event, never the clipboard's text. Calls in a loop start
/// no more than one scrub waiting behind the one that runs.
#[tauri::command]
pub async fn scrub_clipboard(app: AppHandle, state: State<'_, Shared>) -> Res<()> {
    state.wiper.touch(false);
    blocking(move || app::hotkey_pressed(&app)).await
}

/// The `clipboard-scrubbed` event: the result and the latest press it covers (the scrub read the
/// clipboard after it). The window ends only the wait of a press the result covers.
#[derive(Clone, Serialize)]
struct Scrubbed<'a> {
    #[serde(flatten)]
    result: &'a ClipboardScrub,
    press: u64,
}

/// The `clipboard-scrub-waiting` event: the press that waits for the window.
#[derive(Clone, Serialize)]
struct Waiting {
    press: u64,
}

/// Emits the scrub result to the UI (it shows a toast). Before the page listens, the latest
/// result waits for its first `app_info`.
pub fn emit_scrub(app: &AppHandle, r: &ClipboardScrub, press: u64) {
    state::state(app)
        .scrub_results
        .send((r.clone(), press), |(r, press)| send_scrub(app, &r, press));
}

fn send_scrub(app: &AppHandle, r: &ClipboardScrub, press: u64) {
    let _ = app.emit("clipboard-scrubbed", Scrubbed { result: r, press });
}

/// Tells the UI that a scrub waits for the window to come to the front.
pub fn emit_scrub_waiting(app: &AppHandle, press: u64) {
    let _ = app.emit("clipboard-scrub-waiting", Waiting { press });
}

/// The `clipboard-scrub-expired` event: the press that waited for the window in vain.
#[derive(Clone, Serialize)]
struct Expired {
    press: u64,
}

/// Tells the UI that the scrub of `press` waited for the window in vain and did not run.
pub fn emit_scrub_expired(app: &AppHandle, press: u64) {
    let _ = app.emit("clipboard-scrub-expired", Expired { press });
}

/// Something the window shows once, as a notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Notice {
    /// The hotkey from Settings could not be registered at start (another app holds it, the
    /// desktop refused it).
    HotkeyNotActive,
    /// The desktop dropped the hotkey it had bound (the shortcuts portal).
    HotkeyLost,
    /// Settings could not be written to disk; they hold until the app closes.
    SettingsNotSaved,
    /// The settings file could not be used at start and is kept as `settings.json.bad`.
    SettingsUnreadable,
}

/// The `app-status` event: whether the hotkey works now, and a notice for the window, if any.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStatus {
    hotkey_active: bool,
    notice: Option<Notice>,
}

/// Tells the window that the hotkey's state changed, or `notice`. Before the page listens a
/// notice waits for its first `app_info` (which also answers with the hotkey's state then).
pub fn post_status(app: &AppHandle, notice: Option<Notice>) {
    state::state(app)
        .notices
        .add(notice, |notice| send_status(app, notice));
}

fn send_status(app: &AppHandle, notice: Option<Notice>) {
    let hotkey_active = state::state(app).hotkey_active.load(Ordering::SeqCst);
    let _ = app.emit(
        "app-status",
        AppStatus {
            hotkey_active,
            notice,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipscrub::ClipStatus;

    #[test]
    fn the_status_event_carries_the_hotkey_state_and_the_notice() {
        let v = |hotkey_active, notice| {
            serde_json::to_value(AppStatus {
                hotkey_active,
                notice,
            })
            .unwrap()
        };
        assert_eq!(
            v(true, None),
            serde_json::json!({ "hotkeyActive": true, "notice": null })
        );
        for (notice, name) in [
            (Notice::HotkeyNotActive, "hotkeyNotActive"),
            (Notice::HotkeyLost, "hotkeyLost"),
            (Notice::SettingsNotSaved, "settingsNotSaved"),
            (Notice::SettingsUnreadable, "settingsUnreadable"),
        ] {
            assert_eq!(
                v(false, Some(notice)),
                serde_json::json!({ "hotkeyActive": false, "notice": name })
            );
        }
    }

    #[test]
    fn the_scrub_events_carry_the_press_beside_the_result() {
        let r = ClipboardScrub {
            status: ClipStatus::Clean,
            removed: 0,
            by_label: Vec::new(),
            history_cleaned: false,
        };
        let v = serde_json::to_value(Scrubbed {
            result: &r,
            press: 7,
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "status": "clean",
                "removed": 0,
                "byLabel": [],
                "historyCleaned": false,
                "press": 7,
            })
        );
        assert_eq!(
            serde_json::to_value(Waiting { press: 7 }).unwrap(),
            serde_json::json!({ "press": 7 })
        );
        assert_eq!(
            serde_json::to_value(Expired { press: 7 }).unwrap(),
            serde_json::json!({ "press": 7 })
        );
    }

    /// Every status the `clipboard-scrubbed` event can carry, as the UI's schema lists them.
    #[test]
    fn the_scrub_statuses_are_pinned() {
        for (status, name) in [
            (ClipStatus::Cleaned, "cleaned"),
            (ClipStatus::Unverified, "unverified"),
            (ClipStatus::Stripped, "stripped"),
            (ClipStatus::Clean, "clean"),
            (ClipStatus::Empty, "empty"),
            (ClipStatus::Protected, "protected"),
            (ClipStatus::TooLarge, "tooLarge"),
            (ClipStatus::Error, "error"),
        ] {
            assert_eq!(serde_json::to_value(status).unwrap(), name);
        }
    }
}
