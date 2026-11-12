//! Everything the shell keeps while it runs. Memory only, apart from the settings file.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};

use tauri::{AppHandle, Emitter, Manager};

use crate::engines::{EngineSet, Profile};
use crate::platform::Platform;
use crate::session::SessionStore;
use crate::settings::{Settings, SettingsFile};
use crate::wipe::{WipeEvent, WipeReason, Wiper};

pub struct AppState {
    pub session: Arc<SessionStore>,
    pub engines: EngineSet,
    pub wiper: Wiper,
    settings: Mutex<Settings>,
    pub settings_file: Option<SettingsFile>,
    /// The "still running in the tray" notice was shown this run.
    pub tray_notice_shown: AtomicBool,
    /// Serializes clipboard scrubs (the hotkey can repeat).
    pub scrub_lock: Mutex<()>,
    /// Held across a whole settings change (read, apply, store), so two changes never interleave.
    pub settings_change: Mutex<()>,
    /// The operating system behind the shell.
    pub platform: Platform,
}

impl AppState {
    pub fn new(
        settings: Settings,
        settings_file: Option<SettingsFile>,
        platform: Platform,
    ) -> AppState {
        let session = Arc::new(SessionStore::new());
        AppState {
            engines: EngineSet::new(session.clone(), Profile::of(&settings)),
            wiper: Wiper::new(session.clone()),
            session,
            settings: Mutex::new(settings),
            settings_file,
            tray_notice_shown: AtomicBool::new(false),
            scrub_lock: Mutex::new(()),
            settings_change: Mutex::new(()),
            platform,
        }
    }

    pub fn settings(&self) -> Settings {
        self.lock_settings().clone()
    }

    pub fn lock_settings(&self) -> MutexGuard<'_, Settings> {
        self.settings.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Stores new settings in memory and on disk. Disk errors are ignored: the app keeps
    /// working with the settings in memory.
    pub fn store_settings(&self, s: Settings) {
        if let Some(f) = &self.settings_file {
            let _ = f.save(&s);
        }
        *self.lock_settings() = s;
    }
}

/// The shared state, as Tauri manages it.
pub type Shared = Arc<AppState>;

pub fn state(app: &AppHandle) -> Shared {
    app.state::<Shared>().inner().clone()
}

/// Forgets the session secrets and tells the UI to clear both panes.
pub fn wipe(app: &AppHandle, reason: WipeReason) {
    let st = state(app);
    let list = st.wiper.wipe();
    let _ = app.emit("wipe", WipeEvent { reason });
    let _ = app.emit("session-changed", list);
}
