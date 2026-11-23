//! Everything the shell keeps while it runs. Memory only, apart from the settings file.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tauri::{AppHandle, Emitter, Manager};

use crate::clipscrub::ClipboardScrub;
use crate::commands::Notice;
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
    /// The "settings could not be saved" notice was shown this run.
    pub save_failed_shown: AtomicBool,
    /// Serializes clipboard scrubs and lets at most one wait (the hotkey can repeat).
    pub scrubs: ScrubQueue,
    /// Scrub presses so far (hotkey, tray, flag, "Clean now"), and the latest one a scrub may
    /// cover: the events carry a press's number, so the window lets a result end only the wait
    /// of a press it covers.
    pub presses: Presses,
    /// `--scrub-clipboard` started the app where the clipboard needs the window, and the scrub
    /// waits for the page to listen.
    pub start_scrub: HeldScrub,
    /// Scrub results (`clipboard-scrubbed`) wait here until the page listens: a scrub can finish
    /// before the page has loaded (`--scrub-clipboard` starts the app).
    pub scrub_results: ForPage<(ClipboardScrub, u64)>,
    /// Notices for the window (`app-status`), kept in order until the page listens. `None` is a
    /// bare change of the hotkey's state: nothing to keep, the first `app_info` answers with it.
    pub notices: ForPage<Vec<Option<Notice>>>,
    /// The hotkey works now (`app_info.hotkeyActive`). True until a registration fails; where
    /// the app holds no key (a desktop shortcut runs the command) it stays true.
    pub hotkey_active: AtomicBool,
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
            save_failed_shown: AtomicBool::new(false),
            scrubs: ScrubQueue::default(),
            presses: Presses::default(),
            start_scrub: HeldScrub::default(),
            scrub_results: ForPage::default(),
            notices: ForPage::default(),
            hotkey_active: AtomicBool::new(true),
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

    /// Stores new settings in memory and on disk. They apply in memory even when the disk
    /// refuses them: the error says only that they will not outlive this run.
    pub fn store_settings(&self, s: Settings) -> std::io::Result<()> {
        store(self.settings_file.as_ref(), &self.settings, s)
    }
}

fn store(file: Option<&SettingsFile>, slot: &Mutex<Settings>, s: Settings) -> std::io::Result<()> {
    let saved = file.map_or(Ok(()), |f| f.save(&s));
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = s;
    saved
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

/// What became of a scrub asked for with [`ScrubQueue::start`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// A scrub thread started; it tells the user.
    Started,
    /// A scrub waits to run already and covers this call.
    Covered,
    /// The scrub thread could not start: no scrub runs for this call, nor for any call that
    /// counted on it meanwhile. The caller tells the user, for every press so far.
    Failed,
}

impl ScrubQueue {
    /// A scrub is asked for. Unless one waits already, `spawn` starts a thread that calls
    /// [`ScrubQueue::run`] and says whether it started.
    pub fn start(&self, spawn: impl FnOnce() -> bool) -> Start {
        if !self.enqueue() {
            return Start::Covered;
        }
        if spawn() {
            return Start::Started;
        }
        self.not_started();
        Start::Failed
    }

    /// True: none waits, the caller starts one. False: one waits already; it reads the clipboard
    /// after this call, so it covers this one too.
    fn enqueue(&self) -> bool {
        !self.queued.swap(true, Ordering::SeqCst)
    }

    /// The thread behind a true [`ScrubQueue::enqueue`] could not start: the next call starts one.
    fn not_started(&self) {
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

/// The numbers of the scrub presses.
#[derive(Debug, Default)]
pub struct Presses {
    numbered: AtomicU64,
    covered: AtomicU64,
}

impl Presses {
    /// A new press: its number, before any scrub can start for it.
    pub fn next(&self) -> u64 {
        self.numbered.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// `press` is scrubbed without waiting, or its wait for the window ends in a scrub. Called
    /// before that scrub is asked for, so the scrub reads at least this number.
    pub fn cover(&self, press: u64) {
        self.covered.fetch_max(press, Ordering::SeqCst);
    }

    /// The latest press a scrub that starts now covers. A press still waiting for the window is
    /// never counted in.
    pub fn covered(&self) -> u64 {
        self.covered.load(Ordering::SeqCst)
    }
}

/// Results for the page, held back until it listens for the shell's events. Before that an
/// event would be lost: only the latest result is kept, and the page's first `app_info` (asked for
/// once it listens) hands it over. From then on results go straight out. One lock covers keeping,
/// handing over and sending, so a result racing the first `app_info` is shown exactly once.
#[derive(Debug)]
pub struct ForPage<T> {
    state: Mutex<Listening<T>>,
}

#[derive(Debug)]
enum Listening<T> {
    /// The page has not asked for `app_info` yet; the latest result waits for it.
    NotYet(Option<T>),
    Yes,
}

impl<T> Default for ForPage<T> {
    fn default() -> ForPage<T> {
        ForPage {
            state: Mutex::new(Listening::NotYet(None)),
        }
    }
}

impl<T> ForPage<T> {
    /// A result for the page: `emit` runs now if the page listens; otherwise the result is kept,
    /// in place of any kept before.
    pub fn send(&self, result: T, emit: impl FnOnce(T)) {
        match &mut *self.lock() {
            Listening::Yes => emit(result),
            Listening::NotYet(kept) => *kept = Some(result),
        }
    }

    /// The page listens now: `emit` runs for the kept result, if any, once.
    pub fn page_listens(&self, emit: impl FnOnce(T)) {
        let mut st = self.lock();
        if let Listening::NotYet(Some(kept)) = std::mem::replace(&mut *st, Listening::Yes) {
            emit(kept);
        }
    }

    fn lock(&self) -> MutexGuard<'_, Listening<T>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl<T> ForPage<Vec<T>> {
    /// One more for the page: `emit` runs now if the page listens; otherwise it is kept after
    /// the ones kept before, so none is lost (unlike [`ForPage::send`]).
    pub fn add(&self, item: T, emit: impl FnOnce(T)) {
        match &mut *self.lock() {
            Listening::Yes => emit(item),
            Listening::NotYet(kept) => kept.get_or_insert_with(Vec::new).push(item),
        }
    }
}

/// A `--scrub-clipboard` held until the page listens. The page's first `app_info` and a fallback
/// timer both release it; only the first runs it.
#[derive(Debug, Default)]
pub struct HeldScrub(AtomicBool);

impl HeldScrub {
    pub fn hold(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// True once after [`HeldScrub::hold`]: the caller runs the scrub.
    pub fn release(&self) -> bool {
        self.0.swap(false, Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_result_before_the_page_listens_is_handed_over_once() {
        let page = ForPage::default();
        let sent = Mutex::new(Vec::new());
        page.send((1, "clean"), |r| sent.lock().unwrap().push(r));
        assert!(sent.lock().unwrap().is_empty(), "kept: nobody listens yet");
        page.page_listens(|r| sent.lock().unwrap().push(r));
        page.page_listens(|r| sent.lock().unwrap().push(r));
        assert_eq!(*sent.lock().unwrap(), [(1, "clean")], "once");
    }

    #[test]
    fn the_latest_result_before_the_page_listens_wins() {
        let page = ForPage::default();
        let sent = Mutex::new(Vec::new());
        page.send((1, "clean"), |r| sent.lock().unwrap().push(r));
        page.send((2, "cleaned"), |r| sent.lock().unwrap().push(r));
        page.page_listens(|r| sent.lock().unwrap().push(r));
        assert_eq!(*sent.lock().unwrap(), [(2, "cleaned")]);
    }

    #[test]
    fn results_after_the_page_listens_go_straight_out() {
        let page = ForPage::default();
        let sent = Mutex::new(Vec::new());
        page.page_listens(|r| sent.lock().unwrap().push(r));
        assert!(sent.lock().unwrap().is_empty(), "nothing was kept");
        page.send((3, "clean"), |r| sent.lock().unwrap().push(r));
        page.send((4, "empty"), |r| sent.lock().unwrap().push(r));
        assert_eq!(*sent.lock().unwrap(), [(3, "clean"), (4, "empty")]);
    }

    #[test]
    fn added_items_before_the_page_listens_are_all_handed_over_in_order() {
        let page = ForPage::<Vec<&str>>::default();
        let sent = Mutex::new(Vec::new());
        page.add("unreadable", |n| sent.lock().unwrap().push(vec![n]));
        page.add("not active", |n| sent.lock().unwrap().push(vec![n]));
        assert!(sent.lock().unwrap().is_empty(), "kept: nobody listens yet");
        page.page_listens(|kept| sent.lock().unwrap().push(kept));
        page.page_listens(|kept| sent.lock().unwrap().push(kept));
        assert_eq!(*sent.lock().unwrap(), [vec!["unreadable", "not active"]]);
        page.add("lost", |n| sent.lock().unwrap().push(vec![n]));
        assert_eq!(sent.lock().unwrap().last().unwrap(), &vec!["lost"]);
    }

    #[test]
    fn a_result_racing_the_first_app_info_is_shown_exactly_once() {
        for _ in 0..200 {
            let page = ForPage::default();
            let sent = Mutex::new(Vec::new());
            std::thread::scope(|s| {
                s.spawn(|| page.send(7, |r| sent.lock().unwrap().push(r)));
                s.spawn(|| page.page_listens(|r| sent.lock().unwrap().push(r)));
            });
            assert_eq!(*sent.lock().unwrap(), [7]);
        }
    }

    #[test]
    fn a_scrub_covers_only_presses_that_do_not_wait_for_the_window() {
        let p = Presses::default();
        let first = p.next();
        p.cover(first);
        let waiting = p.next();
        assert_eq!((first, waiting), (1, 2));
        assert_eq!(p.covered(), 1, "press 2 still waits for the window");
        p.cover(waiting);
        assert_eq!(p.covered(), 2, "its wait ended in a scrub");
        p.cover(first);
        assert_eq!(p.covered(), 2, "never goes down");
    }

    #[test]
    fn settings_that_cannot_be_saved_still_apply_and_say_so() {
        let dir = std::env::temp_dir().join(format!("redactor-state-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // The settings folder would be below a regular file: it cannot be made.
        let blocker = dir.join("not-a-folder");
        std::fs::write(&blocker, b"").unwrap();
        let file = SettingsFile::new(
            blocker.join("Redactor").join("settings.json"),
            crate::os::Os::Linux,
        );
        let slot = Mutex::new(Settings::default());
        let next = Settings {
            auto_wipe_minutes: 42,
            ..Settings::default()
        };

        let saved = store(Some(&file), &slot, next.clone());
        let _ = std::fs::remove_dir_all(&dir);

        assert!(saved.is_err(), "the caller hears it");
        assert_eq!(*slot.lock().unwrap(), next, "in memory all the same");
        assert!(
            store(None, &slot, Settings::default()).is_ok(),
            "no file, nothing to save"
        );
    }

    #[test]
    fn the_held_start_scrub_runs_once() {
        let held = HeldScrub::default();
        assert!(!held.release(), "nothing held");
        held.hold();
        let runs = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..8 {
                // The page's first app_info and the fallback timer race.
                s.spawn(|| {
                    if held.release() {
                        runs.fetch_add(1, Ordering::SeqCst);
                    }
                });
            }
        });
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert!(!held.release());
    }

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
    fn a_scrub_thread_that_cannot_start_is_reported_and_frees_the_next_call() {
        let q = ScrubQueue::default();
        let mut covered = Start::Started;
        let first = q.start(|| {
            // A call between the enqueue and the failed spawn counts on the scrub that failed.
            covered = q.start(|| panic!("a scrub waits already"));
            false
        });
        assert_eq!(covered, Start::Covered);
        assert_eq!(first, Start::Failed, "the caller tells every press so far");
        assert_eq!(
            q.start(|| true),
            Start::Started,
            "the next call tries again"
        );
        assert_eq!(q.start(|| panic!("one waits")), Start::Covered);
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
