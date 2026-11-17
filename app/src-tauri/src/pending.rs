//! A clipboard scrub that waits for the window to come to the front.
//!
//! Where the clipboard can be read only while Redactor's window is in front, a hotkey press
//! shows the window and arms a pending scrub. The next time the window gains focus the scrub
//! runs, once; after [`PENDING_FOR`] it has expired instead, so it never runs on a clipboard
//! the user has moved on from. A timer expires it at [`PENDING_FOR`] even when the window never
//! comes to the front, so the user hears that it did not run; whichever comes first takes it,
//! so there is one notice and never a late scrub.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long an armed scrub waits for the window to come to the front.
pub const PENDING_FOR: Duration = Duration::from_secs(30);

/// What a focus-in does with the pending scrub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Take {
    /// A scrub was armed less than [`PENDING_FOR`] ago: run it now.
    Run,
    /// A scrub was armed, but too long ago: tell the user it did not run.
    Expired,
    /// No scrub was armed.
    Nothing,
}

/// One arming of the pending scrub: when, and which (a later arming is another one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Armed {
    at: Instant,
    id: u64,
}

/// The last hotkey press still waiting for focus, if any.
#[derive(Debug, Default)]
pub struct PendingScrub {
    armed: Mutex<Option<Armed>>,
    next_id: AtomicU64,
}

impl PendingScrub {
    pub fn new() -> PendingScrub {
        PendingScrub::default()
    }

    /// A press while the window is not in front. A second press restarts the wait; it never
    /// adds a second scrub. Returns this arming, for [`PendingScrub::expire`].
    pub fn arm(&self, now: Instant) -> Armed {
        let armed = Armed {
            at: now,
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
        };
        *self.lock() = Some(armed);
        armed
    }

    /// A scrub ran without waiting: the one still pending is dropped, so one press never
    /// scrubs twice.
    pub fn clear(&self) {
        *self.lock() = None;
    }

    /// The window came to the front: takes the pending scrub, if any.
    pub fn take_on_focus(&self, now: Instant) -> Take {
        match self.lock().take() {
            None => Take::Nothing,
            Some(armed) if now.saturating_duration_since(armed.at) < PENDING_FOR => Take::Run,
            Some(_) => Take::Expired,
        }
    }

    /// The timer of `armed` is up: takes the pending scrub if it is still that arming and has
    /// waited [`PENDING_FOR`]. True: it expired now, tell the user. False: a focus, a scrub or
    /// a later press dealt with it already.
    pub fn expire(&self, armed: Armed, now: Instant) -> bool {
        let mut slot = self.lock();
        let due = now.saturating_duration_since(armed.at) >= PENDING_FOR;
        if due && *slot == Some(armed) {
            *slot = None;
            true
        } else {
            false
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Armed>> {
        self.armed.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_scrub_runs_once_on_focus() {
        let p = PendingScrub::new();
        let t = Instant::now();
        p.arm(t);
        assert_eq!(p.take_on_focus(t + Duration::from_secs(1)), Take::Run);
        assert_eq!(p.take_on_focus(t + Duration::from_secs(2)), Take::Nothing);
    }

    #[test]
    fn pending_scrub_expires_after_30_s() {
        let t = Instant::now();
        let p = PendingScrub::new();
        p.arm(t);
        assert_eq!(p.take_on_focus(t + Duration::from_secs(30)), Take::Expired);
        assert_eq!(p.take_on_focus(t + Duration::from_secs(31)), Take::Nothing);

        p.arm(t);
        assert_eq!(
            p.take_on_focus(t + Duration::from_millis(29_900)),
            Take::Run
        );
    }

    #[test]
    fn second_press_rearms_not_doubles() {
        let p = PendingScrub::new();
        let t = Instant::now();
        p.arm(t);
        p.arm(t + Duration::from_secs(20));
        assert_eq!(p.take_on_focus(t + Duration::from_secs(40)), Take::Run);
        assert_eq!(p.take_on_focus(t + Duration::from_secs(41)), Take::Nothing);
    }

    #[test]
    fn clear_drops_a_pending_arm() {
        let p = PendingScrub::new();
        let t = Instant::now();
        p.arm(t);
        p.clear();
        assert_eq!(p.take_on_focus(t + Duration::from_secs(1)), Take::Nothing);
    }

    #[test]
    fn the_timer_expires_an_unanswered_press_once() {
        let p = PendingScrub::new();
        let t = Instant::now();
        let armed = p.arm(t);
        assert!(!p.expire(armed, t + Duration::from_secs(29)), "not due yet");
        assert!(p.expire(armed, t + PENDING_FOR));
        assert!(!p.expire(armed, t + PENDING_FOR), "one notice");
        // A focus after the timer: no late scrub, no second notice.
        assert_eq!(p.take_on_focus(t + Duration::from_secs(31)), Take::Nothing);
    }

    #[test]
    fn the_timer_of_an_older_press_stays_quiet() {
        let p = PendingScrub::new();
        let t = Instant::now();
        let first = p.arm(t);
        let second = p.arm(t + Duration::from_secs(20));
        assert!(
            !p.expire(first, t + PENDING_FOR),
            "the second press restarted the wait"
        );
        assert_eq!(p.take_on_focus(t + Duration::from_secs(40)), Take::Run);
        assert!(
            !p.expire(second, t + Duration::from_secs(50)),
            "the scrub ran"
        );
    }

    #[test]
    fn the_timer_stays_quiet_after_a_focus_or_a_scrub() {
        let p = PendingScrub::new();
        let t = Instant::now();
        let armed = p.arm(t);
        assert_eq!(p.take_on_focus(t + PENDING_FOR), Take::Expired);
        assert!(
            !p.expire(armed, t + PENDING_FOR),
            "the focus told the user already"
        );

        let armed = p.arm(t);
        p.clear();
        assert!(
            !p.expire(armed, t + PENDING_FOR),
            "a scrub ran without waiting"
        );
    }

    #[test]
    fn focus_without_press_is_nothing() {
        let p = PendingScrub::new();
        assert_eq!(p.take_on_focus(Instant::now()), Take::Nothing);
    }
}
