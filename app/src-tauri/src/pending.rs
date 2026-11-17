//! A clipboard scrub that waits for the window to come to the front.
//!
//! Where the clipboard can be read only while Redactor's window is in front, a hotkey press
//! shows the window and arms a pending scrub. The next time the window gains focus the scrub
//! runs, once; after [`PENDING_FOR`] it has expired instead, so it never runs on a clipboard
//! the user has moved on from.

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

/// The time of the last hotkey press still waiting for focus, if any.
#[derive(Debug, Default)]
pub struct PendingScrub {
    armed_at: Mutex<Option<Instant>>,
}

impl PendingScrub {
    pub fn new() -> PendingScrub {
        PendingScrub::default()
    }

    /// A press while the window is not in front. A second press restarts the wait; it never
    /// adds a second scrub.
    pub fn arm(&self, now: Instant) {
        *self.lock() = Some(now);
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
            Some(armed) if now.saturating_duration_since(armed) < PENDING_FOR => Take::Run,
            Some(_) => Take::Expired,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Instant>> {
        self.armed_at.lock().unwrap_or_else(|e| e.into_inner())
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
    fn focus_without_press_is_nothing() {
        let p = PendingScrub::new();
        assert_eq!(p.take_on_focus(Instant::now()), Take::Nothing);
    }
}
