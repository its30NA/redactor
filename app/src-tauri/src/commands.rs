//! The IPC commands. Each one is whitelisted in `capabilities/main.json` and listed in
//! `ipc_names.rs`. Heavy or blocking work runs on a blocking worker, never on the UI thread.
//! Errors are short user-facing strings and never quote the text.

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroize;

use crate::app;
use crate::bridge::{self, ScanRequestIn, ScanResponseOut};
use crate::clipscrub::ClipboardScrub;
use crate::session::SessionSecret;
use crate::settings::Settings;
use crate::state::Shared;
use crate::textfile;
use crate::win::clipboard::{self, WinClipboard};

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
    blocking(clipboard::read_text_for_ui)
        .await?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn copy_text(state: State<'_, Shared>, mut text: String) -> Res<()> {
    state.wiper.touch(false);
    let r = blocking(move || {
        let r = WinClipboard.write_text(&text);
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
        let meta =
            std::fs::metadata(&path).map_err(|_| "The file could not be read.".to_string())?;
        if meta.len() > bridge::MAX_INPUT_BYTES as u64 {
            return Err(textfile::FileError::TooLarge.to_string());
        }
        let bytes = std::fs::read(&path).map_err(|_| "The file could not be read.".to_string())?;
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
}

#[tauri::command]
pub async fn app_info(state: State<'_, Shared>) -> Res<AppInfo> {
    let st = state.inner().clone();
    blocking(move || AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        engine_version: redactor_core::ENGINE_VERSION,
        rule_count: st.engines.rule_count(),
        platform: "windows",
    })
    .await
}

/// Emits the scrub result to the UI (it shows a toast).
pub fn emit_scrub(app: &AppHandle, r: &ClipboardScrub) {
    let _ = app.emit("clipboard-scrubbed", r);
}
