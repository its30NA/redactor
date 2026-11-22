//! A clipboard scrub that waits for the window to come to the front.
//!
//! Where the clipboard can be read only while Redactor's window is in front, a hotkey press
//! shows the window and arms a pending scrub. The next time the window gains focus the scrub
//! runs, once; after [`PENDING_FOR`] it has expired instead, so it never runs on a clipboard
//! the user has moved on from. One timer expires it at [`PENDING_FOR`] even when the window
//! never comes to the front, so the user hears that it did not run; whichever comes first takes
//! it, so there is never a late scrub. A desktop may drop that notice (WSLg shows none), so the
//! next focus after the timer still says once, in the window, that the scrub did not run, for
//! [`UNSEEN_FOR`] at most.
//!
//! Only one timer runs at a time, however often the press repeats (the window's "Clean now" is
//! an IPC command a script could loop): it follows the latest press.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long an armed scrub waits for the window to come to the front.
pub const PENDING_FOR: Duration = Duration::from_secs(30);

/// How long after the timer expired a scrub the window still reports it on the next focus.
pub const UNSEEN_FOR: Duration = Duration::from_secs(10 * 60);

/// What a focus-in does with the pending scrub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Take {
    /// A scrub was armed less than [`PENDING_FOR`] ago: run it now.
    Run,
    /// The timer expired a scrub and told the user already; the window has not shown it yet:
    /// show it now, without a second desktop notification.
    Expired,
    /// A scrub was armed too long ago, but the timer has not said so (it runs late, e.g. after
    /// a suspend): tell the user it did not run, on the desktop and in the window. The timer
    /// then finds nothing, so this is the only notice.
    Lapsed,
    /// No scrub was armed.
    Nothing,
}

/// What the expiry timer does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timer {
    /// The pending scrub has some wait left: sleep this long, then ask again.
    Sleep(Duration),
    /// The pending scrub just expired: tell the user. The timer is done.
    Expired,
    /// Nothing waits any more (a focus or a scrub dealt with it): the timer is done.
    Done,
}

/// What the pending scrub holds between a press and the next focus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Slot {
    #[default]
    Empty,
    /// A press at this time waits for the window.
    Armed(Instant),
    /// The timer expired a press at this time; the window has not said so yet.
    ExpiredUnseen(Instant),
}

#[derive(Debug, Default)]
struct State {
    slot: Slot,
    /// An expiry timer thread is running. Kept with the slot so that a press and a timer that
    /// stops can never miss each other.
    timer_running: bool,
}

/// The last hotkey press still waiting for focus, if any.
#[derive(Debug, Default)]
pub struct PendingScrub {
    state: Mutex<State>,
}

impl PendingScrub {
    pub fn new() -> PendingScrub {
        PendingScrub::default()
    }

    /// A press while the window is not in front. A second press restarts the wait; it never
    /// adds a second scrub, and an earlier expiry the window has not shown is dropped.
    /// True: no timer runs, the caller starts one ([`PendingScrub::timer_step`] in a loop).
    /// False: the running timer follows this press.
    pub fn arm(&self, now: Instant) -> bool {
        let mut st = self.lock();
        st.slot = Slot::Armed(now);
        !std::mem::replace(&mut st.timer_running, true)
    }

    /// The timer thread behind a true [`PendingScrub::arm`] could not start.
    pub fn timer_failed(&self) {
        self.lock().timer_running = false;
    }

    /// A scrub ran without waiting: the one still pending is dropped, so one press never
    /// scrubs twice, and an earlier expiry the window has not shown is stale.
    pub fn clear(&self) {
        self.lock().slot = Slot::Empty;
    }

    /// The window came to the front: takes the pending scrub, or the expiry the window has not
    /// shown yet, if any.
    pub fn take_on_focus(&self, now: Instant) -> Take {
        match std::mem::take(&mut self.lock().slot) {
            Slot::Empty => Take::Nothing,
            Slot::Armed(at) if now.saturating_duration_since(at) < PENDING_FOR => Take::Run,
            Slot::Armed(_) => Take::Lapsed,
            Slot::ExpiredUnseen(at) if now.saturating_duration_since(at) < UNSEEN_FOR => {
                Take::Expired
            }
            Slot::ExpiredUnseen(_) => Take::Nothing,
        }
    }

    /// The expiry timer asks what to do: sleep on, tell the user (the pending scrub expired now;
    /// the next focus says so in the window too), or stop (a focus, a scrub or a later press
    /// dealt with it already). After `Expired` and `Done` no timer runs.
    pub fn timer_step(&self, now: Instant) -> Timer {
        let mut st = self.lock();
        if let Slot::Armed(at) = st.slot {
            let waited = now.saturating_duration_since(at);
            if let Some(left) = PENDING_FOR.checked_sub(waited).filter(|d| !d.is_zero()) {
                return Timer::Sleep(left);
            }
            st.slot = Slot::ExpiredUnseen(now);
            st.timer_running = false;
            return Timer::Expired;
        }
        st.timer_running = false;
        Timer::Done
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn pending_scrub_runs_once_on_focus() {
        let p = PendingScrub::new();
        let t = Instant::now();
        p.arm(t);
        assert_eq!(p.take_on_focus(t + S(1)), Take::Run);
        assert_eq!(p.take_on_focus(t + S(2)), Take::Nothing);
    }

    #[test]
    fn pending_scrub_lapses_after_30_s() {
        let t = Instant::now();
        let p = PendingScrub::new();
        p.arm(t);
        assert_eq!(p.take_on_focus(t + S(30)), Take::Lapsed);
        assert_eq!(p.take_on_focus(t + S(31)), Take::Nothing);

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
        p.arm(t + S(20));
        assert_eq!(p.take_on_focus(t + S(40)), Take::Run);
        assert_eq!(p.take_on_focus(t + S(41)), Take::Nothing);
    }

    #[test]
    fn clear_drops_a_pending_arm() {
        let p = PendingScrub::new();
        let t = Instant::now();
        p.arm(t);
        p.clear();
        assert_eq!(p.take_on_focus(t + S(1)), Take::Nothing);
    }

    #[test]
    fn the_timer_expires_an_unanswered_press_once() {
        let p = PendingScrub::new();
        let t = Instant::now();
        p.arm(t);
        assert_eq!(p.timer_step(t + S(29)), Timer::Sleep(S(1)));
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Done, "one notice");
        // A focus after the timer: no late scrub; the window says once that it did not run.
        assert_eq!(p.take_on_focus(t + S(31)), Take::Expired);
        assert_eq!(p.take_on_focus(t + S(32)), Take::Nothing);
    }

    #[test]
    fn focus_after_the_timer_expired_reports_once_in_the_window() {
        let p = PendingScrub::new();
        let t = Instant::now();
        p.arm(t);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired);
        assert_eq!(p.take_on_focus(t + S(45)), Take::Expired);
        assert_eq!(p.take_on_focus(t + S(46)), Take::Nothing);
    }

    #[test]
    fn an_expiry_the_window_never_saw_ages_out() {
        let p = PendingScrub::new();
        let t = Instant::now();
        p.arm(t);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired);
        let expired_at = t + PENDING_FOR;
        // A focus long after the press is no news any more.
        assert_eq!(
            p.take_on_focus(expired_at + UNSEEN_FOR),
            Take::Nothing,
            "dropped"
        );

        let p = PendingScrub::new();
        p.arm(t);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired);
        assert_eq!(
            p.take_on_focus(expired_at + UNSEEN_FOR - Duration::from_millis(1)),
            Take::Expired
        );
    }

    #[test]
    fn a_new_press_clears_an_unseen_expiry() {
        let p = PendingScrub::new();
        let t = Instant::now();
        p.arm(t);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired);
        p.arm(t + S(40));
        assert_eq!(p.take_on_focus(t + S(41)), Take::Run);
        assert_eq!(p.take_on_focus(t + S(42)), Take::Nothing);
    }

    #[test]
    fn a_scrub_without_waiting_clears_an_unseen_expiry() {
        let p = PendingScrub::new();
        let t = Instant::now();
        p.arm(t);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired);
        p.clear();
        assert_eq!(p.take_on_focus(t + S(41)), Take::Nothing);
    }

    #[test]
    fn a_second_press_restarts_the_timer_s_wait() {
        let p = PendingScrub::new();
        let t = Instant::now();
        p.arm(t);
        p.arm(t + S(20));
        assert_eq!(
            p.timer_step(t + PENDING_FOR),
            Timer::Sleep(S(20)),
            "the second press restarted the wait"
        );
        assert_eq!(p.timer_step(t + S(20) + PENDING_FOR), Timer::Expired);
    }

    #[test]
    fn the_timer_stays_quiet_after_a_focus_or_a_scrub() {
        let p = PendingScrub::new();
        let t = Instant::now();
        p.arm(t);
        assert_eq!(p.take_on_focus(t + PENDING_FOR), Take::Lapsed);
        assert_eq!(
            p.timer_step(t + PENDING_FOR),
            Timer::Done,
            "the focus told the user already"
        );

        p.arm(t);
        p.clear();
        assert_eq!(
            p.timer_step(t + PENDING_FOR),
            Timer::Done,
            "a scrub ran without waiting"
        );
    }

    #[test]
    fn a_looping_caller_gets_one_timer_not_one_per_press() {
        let p = PendingScrub::new();
        let t = Instant::now();
        assert!(p.arm(t), "the first press needs a timer");
        for i in 1..50 {
            assert!(!p.arm(t + Duration::from_millis(i)), "one timer runs");
        }
        // The one timer follows the last press and then stops.
        let last = t + Duration::from_millis(49);
        assert_eq!(
            p.timer_step(t + PENDING_FOR),
            Timer::Sleep(S(0) + Duration::from_millis(49))
        );
        assert_eq!(p.timer_step(last + PENDING_FOR), Timer::Expired);
        assert!(
            p.arm(last + S(40)),
            "the timer is gone: a new press needs one"
        );
    }

    #[test]
    fn a_timer_that_finds_nothing_stops_and_frees_the_next_press() {
        let p = PendingScrub::new();
        let t = Instant::now();
        assert!(p.arm(t));
        assert_eq!(p.take_on_focus(t + S(1)), Take::Run);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Done);
        assert!(p.arm(t + S(40)));
    }

    #[test]
    fn a_timer_that_could_not_start_frees_the_next_press() {
        let p = PendingScrub::new();
        let t = Instant::now();
        assert!(p.arm(t));
        p.timer_failed();
        assert!(p.arm(t + S(1)));
    }

    #[test]
    fn focus_without_press_is_nothing() {
        let p = PendingScrub::new();
        assert_eq!(p.take_on_focus(Instant::now()), Take::Nothing);
    }
}
