//! Everything the shell keeps while it runs. Memory only, apart from the settings file.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tauri::{AppHandle, Emitter, Manager};

use crate::engines::{EngineSet, Profile};
use crate::pending::PendingScrub;
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
    /// Serializes clipboard scrubs and lets at most one wait (the hotkey can repeat).
    pub scrubs: ScrubQueue,
    /// Scrub presses so far (hotkey, tray, flag, "Clean now"): the events carry a press's number,
    /// so the window lets a result end only the wait of a press it covers.
    pub presses: AtomicU64,
    /// Held across a whole settings change (read, apply, store), so two changes never interleave.
    pub settings_change: Mutex<()>,
    /// A hotkey press waiting for the window to come to the front.
    pub pending: PendingScrub,
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
            scrubs: ScrubQueue::default(),
            presses: AtomicU64::new(0),
            settings_change: Mutex::new(()),
            pending: PendingScrub::new(),
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

/// Clipboard scrubs, one at a time, and at most one waiting behind the one that runs, however
/// often they are asked for (the window's "Clean now" is an IPC command a script could loop).
#[derive(Debug, Default)]
pub struct ScrubQueue {
    /// A scrub was asked for and has not started yet.
    queued: AtomicBool,
    running: Mutex<()>,
}

impl ScrubQueue {
    /// A scrub is asked for. True: none waits, the caller starts one ([`ScrubQueue::run`] on a
    /// thread of its own). False: one waits already; it reads the clipboard after this call, so
    /// it covers this one too.
    pub fn enqueue(&self) -> bool {
        !self.queued.swap(true, Ordering::SeqCst)
    }

    /// The thread behind a true [`ScrubQueue::enqueue`] could not start: the next call starts one.
    pub fn not_started(&self) {
        self.queued.store(false, Ordering::SeqCst);
    }

    /// Runs the waiting scrub once the one running is done. From then on a new call queues the
    /// next scrub: this one may have read the clipboard already.
    pub fn run<T>(&self, scrub: impl FnOnce() -> T) -> T {
        let _one_at_a_time = self.running.lock().unwrap_or_else(|e| e.into_inner());
        self.queued.store(false, Ordering::SeqCst);
        scrub()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_while_a_scrub_waits_adds_none() {
        let q = ScrubQueue::default();
        assert!(q.enqueue(), "the first call starts a scrub");
        for _ in 0..50 {
            assert!(!q.enqueue(), "the waiting scrub covers it");
        }
    }

    #[test]
    fn a_call_while_the_scrub_runs_queues_one_more() {
        let q = ScrubQueue::default();
        assert!(q.enqueue());
        q.run(|| {
            assert!(
                q.enqueue(),
                "the running scrub may have read the clipboard already"
            );
            assert!(!q.enqueue());
        });
        q.run(|| {});
        assert!(q.enqueue(), "nothing waits any more");
    }

    #[test]
    fn a_scrub_that_could_not_start_frees_the_next_call() {
        let q = ScrubQueue::default();
        assert!(q.enqueue());
        q.not_started();
        assert!(q.enqueue());
    }

    #[test]
    fn scrubs_run_one_at_a_time() {
        let q = ScrubQueue::default();
        let inside = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    q.run(|| {
                        assert_eq!(inside.fetch_add(1, Ordering::SeqCst), 0, "one at a time");
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        inside.fetch_sub(1, Ordering::SeqCst);
                    })
                });
            }
        });
    }
}
