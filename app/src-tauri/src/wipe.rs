//! Wiping: after `autoWipeMinutes` idle, on session lock or sleep
//! (`wipeOnLock`), and from the tray. The shell clears the session secrets; the `wipe` event
//! tells the UI to clear both panes and their undo history.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use crate::pending::When;
use crate::session::{SessionSecret, SessionStore};

/// Why a wipe happened (the `wipe` event's `reason`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WipeReason {
    Idle,
    Lock,
    Sleep,
    Tray,
}

/// The `wipe` event payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct WipeEvent {
    pub reason: WipeReason,
}

/// Clears the session secrets and tracks whether the app holds anything worth wiping.
pub struct Wiper {
    session: Arc<SessionStore>,
    /// On both clocks: where the monotonic clock stands still while the computer sleeps (Linux
    /// and macOS), the wall clock still counts the sleep as idle time.
    last_activity: Mutex<When>,
    /// Text reached the shell since the last wipe (the panes may hold it).
    dirty: AtomicBool,
}

impl Wiper {
    pub fn new(session: Arc<SessionStore>) -> Wiper {
        Wiper {
            session,
            last_activity: Mutex::new(When::now()),
            dirty: AtomicBool::new(false),
        }
    }

    /// Any IPC call from the UI counts as activity; `with_text` marks that text is on screen.
    pub fn touch(&self, with_text: bool) {
        *self.last_activity.lock().unwrap_or_else(|e| e.into_inner()) = When::now();
        if with_text {
            self.dirty.store(true, Ordering::SeqCst);
        }
    }

    /// True when there is something to wipe: text since the last wipe, or session secrets.
    pub fn has_content(&self) -> bool {
        self.dirty.load(Ordering::SeqCst) || !self.session.is_empty()
    }

    /// True when the idle timer has run out (`minutes == 0` means never) and there is content.
    /// Idle time is the longer of the two clocks, so time asleep counts; a wall clock set back
    /// counts as no time, and one set forward counts as idle time.
    pub fn idle_due(&self, minutes: u32, now: When) -> bool {
        if minutes == 0 || !self.has_content() {
            return false;
        }
        let last = *self.last_activity.lock().unwrap_or_else(|e| e.into_inner());
        now.since(last) >= Duration::from_secs(u64::from(minutes) * 60)
    }

    /// Forgets the session secrets. Returns the new (empty) list for `session-changed`.
    pub fn wipe(&self) -> Vec<SessionSecret> {
        self.dirty.store(false, Ordering::SeqCst);
        self.session.clear()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_needs_content_and_time() {
        let session = Arc::new(SessionStore::new());
        let w = Wiper::new(session.clone());
        let later = When::now() + Duration::from_secs(11 * 60);
        assert!(!w.idle_due(10, later), "nothing to wipe");
        w.touch(true);
        assert!(!w.idle_due(10, When::now()));
        assert!(w.idle_due(10, later + Duration::from_secs(60)));
        assert!(!w.idle_due(0, later + Duration::from_secs(3600)), "0 = off");
        w.wipe();
        assert!(!w.idle_due(10, later + Duration::from_secs(60)));
        session.add("abcdefgh", "x").unwrap();
        assert!(w.idle_due(10, later + Duration::from_secs(60)));
    }

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn the_idle_wipe_is_due_after_a_suspend() {
        let w = Wiper::new(Arc::new(SessionStore::new()));
        w.touch(true);
        let t = When::now();
        // The monotonic clock stood still while the computer slept; the wall clock moved on.
        let resumed = When {
            mono: t.mono + Duration::from_secs(1),
            wall: t.wall + 11 * MIN,
        };
        assert!(w.idle_due(10, resumed));
    }

    #[test]
    fn a_wall_clock_set_back_does_not_delay_the_idle_wipe() {
        let w = Wiper::new(Arc::new(SessionStore::new()));
        w.touch(true);
        let t = When::now();
        let set_back = |mono: Duration| When {
            mono: t.mono + mono,
            wall: t.wall - 60 * MIN,
        };
        assert!(!w.idle_due(10, set_back(Duration::from_secs(1))));
        assert!(
            w.idle_due(10, set_back(11 * MIN)),
            "the monotonic time counts"
        );
    }

    #[test]
    fn wipe_event_shape() {
        let json = serde_json::to_string(&WipeEvent {
            reason: WipeReason::Sleep,
        })
        .unwrap();
        assert_eq!(json, r#"{"reason":"sleep"}"#);
    }
}
