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
//!
//! The wait counts on two clocks, [`When`]: where the monotonic clock stands still while the
//! computer sleeps (Linux and macOS), the wall clock still shows that a press from before a
//! suspend is old.
//!
//! The slot keeps the number of the press that waits (`AppState::presses`), and every outcome
//! hands it back, so the window can tell which press a notice is about.

use std::ops::Add;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// How long an armed scrub waits for the window to come to the front.
pub const PENDING_FOR: Duration = Duration::from_secs(30);

/// How long after the timer expired a scrub the window still reports it on the next focus.
pub const UNSEEN_FOR: Duration = Duration::from_secs(10 * 60);

/// A moment on both clocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct When {
    /// Never jumps, but may stand still while the computer sleeps.
    pub mono: Instant,
    /// Runs on during a sleep, but may be set back or forward.
    pub wall: SystemTime,
}

impl When {
    pub fn now() -> When {
        When {
            mono: Instant::now(),
            wall: SystemTime::now(),
        }
    }

    /// How long ago `earlier` was: the longer of the two clocks, so a sleep counts. A wall clock
    /// set back counts as no time.
    pub(crate) fn since(self, earlier: When) -> Duration {
        let mono = self.mono.saturating_duration_since(earlier.mono);
        let wall = self.wall.duration_since(earlier.wall).unwrap_or_default();
        mono.max(wall)
    }
}

impl Add<Duration> for When {
    type Output = When;

    fn add(self, d: Duration) -> When {
        When {
            mono: self.mono + d,
            wall: self.wall + d,
        }
    }
}

/// What a focus-in does with the pending scrub. Each carries the number of the press that waited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Take {
    /// A scrub was armed less than [`PENDING_FOR`] ago: run it now.
    Run(u64),
    /// The timer expired a scrub and told the user already; the window has not shown it yet:
    /// show it now, without a second desktop notification.
    Expired(u64),
    /// A scrub was armed too long ago, but the timer has not said so (it runs late, e.g. after
    /// a suspend): tell the user it did not run, on the desktop and in the window. The timer
    /// then finds nothing, so this is the only notice.
    Lapsed(u64),
    /// No scrub was armed.
    Nothing,
}

/// What the expiry timer does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timer {
    /// The pending scrub has some wait left: sleep this long, then ask again.
    Sleep(Duration),
    /// The pending scrub of this press just expired: tell the user. The timer is done.
    Expired(u64),
    /// Nothing waits any more (a focus or a scrub dealt with it): the timer is done.
    Done,
}

/// What the pending scrub holds between a press and the next focus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Slot {
    #[default]
    Empty,
    /// The press with this number, at this time, waits for the window.
    Armed(When, u64),
    /// The timer expired this press at this time; the window has not said so yet.
    ExpiredUnseen(When, u64),
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

    /// Press number `press` while the window is not in front. A second press restarts the wait
    /// and the slot keeps the later press; it never adds a second scrub, and an earlier expiry
    /// the window has not shown is dropped.
    /// True: no timer runs, the caller starts one ([`PendingScrub::timer_step`] in a loop).
    /// False: the running timer follows this press.
    pub fn arm(&self, now: When, press: u64) -> bool {
        let mut st = self.lock();
        let press = match st.slot {
            Slot::Armed(_, earlier) => press.max(earlier),
            _ => press,
        };
        st.slot = Slot::Armed(now, press);
        !std::mem::replace(&mut st.timer_running, true)
    }

    /// The timer thread behind a true [`PendingScrub::arm`] could not start. No timer will tell
    /// the user, so the press expires now, as if the timer had run out; the next press tries a
    /// timer again. Some: this press was still waiting, so tell the user now, as on
    /// [`Timer::Expired`].
    pub fn timer_failed(&self, now: When) -> Option<u64> {
        let mut st = self.lock();
        st.timer_running = false;
        if let Slot::Armed(_, press) = st.slot {
            st.slot = Slot::ExpiredUnseen(now, press);
            return Some(press);
        }
        None
    }

    /// The timer thread's handle on the pending scrub, made first thing in the thread. If the
    /// thread ends before a step said `Expired` or `Done` (it panicked), dropping it frees the
    /// next press to start a timer.
    pub fn timer(&self) -> ExpiryTimer<'_> {
        ExpiryTimer {
            pending: self,
            ended: false,
        }
    }

    /// A scrub ran without waiting: the one still pending is dropped, so one press never
    /// scrubs twice, and an earlier expiry the window has not shown is stale.
    pub fn clear(&self) {
        self.lock().slot = Slot::Empty;
    }

    /// The window came to the front: takes the pending scrub, or the expiry the window has not
    /// shown yet, if any.
    pub fn take_on_focus(&self, now: When) -> Take {
        match std::mem::take(&mut self.lock().slot) {
            Slot::Empty => Take::Nothing,
            Slot::Armed(at, press) if now.since(at) < PENDING_FOR => Take::Run(press),
            Slot::Armed(_, press) => Take::Lapsed(press),
            Slot::ExpiredUnseen(at, press) if now.since(at) < UNSEEN_FOR => Take::Expired(press),
            Slot::ExpiredUnseen(..) => Take::Nothing,
        }
    }

    /// The expiry timer asks what to do: sleep on, tell the user (the pending scrub expired now;
    /// the next focus says so in the window too), or stop (a focus, a scrub or a later press
    /// dealt with it already). After `Expired` and `Done` no timer runs.
    pub fn timer_step(&self, now: When) -> Timer {
        let mut st = self.lock();
        if let Slot::Armed(at, press) = st.slot {
            let waited = now.since(at);
            if let Some(left) = PENDING_FOR.checked_sub(waited).filter(|d| !d.is_zero()) {
                return Timer::Sleep(left);
            }
            st.slot = Slot::ExpiredUnseen(now, press);
            st.timer_running = false;
            return Timer::Expired(press);
        }
        st.timer_running = false;
        Timer::Done
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The running expiry timer, from [`PendingScrub::timer`].
#[derive(Debug)]
pub struct ExpiryTimer<'a> {
    pending: &'a PendingScrub,
    /// A step said `Expired` or `Done`: the flag is reset, and a later press may own it now.
    ended: bool,
}

impl ExpiryTimer<'_> {
    /// [`PendingScrub::timer_step`], remembering when the timer is done.
    pub fn step(&mut self, now: When) -> Timer {
        let next = self.pending.timer_step(now);
        self.ended = !matches!(next, Timer::Sleep(_));
        next
    }
}

impl Drop for ExpiryTimer<'_> {
    fn drop(&mut self) {
        if !self.ended {
            self.pending.lock().timer_running = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn pending_scrub_runs_once_on_focus() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        assert_eq!(p.take_on_focus(t + S(1)), Take::Run(1));
        assert_eq!(p.take_on_focus(t + S(2)), Take::Nothing);
    }

    #[test]
    fn pending_scrub_lapses_after_30_s() {
        let t = When::now();
        let p = PendingScrub::new();
        p.arm(t, 1);
        assert_eq!(p.take_on_focus(t + S(30)), Take::Lapsed(1));
        assert_eq!(p.take_on_focus(t + S(31)), Take::Nothing);

        p.arm(t, 1);
        assert_eq!(
            p.take_on_focus(t + Duration::from_millis(29_900)),
            Take::Run(1)
        );
    }

    #[test]
    fn second_press_rearms_not_doubles() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        p.arm(t + S(20), 1);
        assert_eq!(p.take_on_focus(t + S(40)), Take::Run(1));
        assert_eq!(p.take_on_focus(t + S(41)), Take::Nothing);
    }

    #[test]
    fn clear_drops_a_pending_arm() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        p.clear();
        assert_eq!(p.take_on_focus(t + S(1)), Take::Nothing);
    }

    #[test]
    fn the_timer_expires_an_unanswered_press_once() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        assert_eq!(p.timer_step(t + S(29)), Timer::Sleep(S(1)));
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired(1));
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Done, "one notice");
        // A focus after the timer: no late scrub; the window says once that it did not run.
        assert_eq!(p.take_on_focus(t + S(31)), Take::Expired(1));
        assert_eq!(p.take_on_focus(t + S(32)), Take::Nothing);
    }

    #[test]
    fn focus_after_the_timer_expired_reports_once_in_the_window() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired(1));
        assert_eq!(p.take_on_focus(t + S(45)), Take::Expired(1));
        assert_eq!(p.take_on_focus(t + S(46)), Take::Nothing);
    }

    #[test]
    fn an_expiry_the_window_never_saw_ages_out() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired(1));
        let expired_at = t + PENDING_FOR;
        // A focus long after the press is no news any more.
        assert_eq!(
            p.take_on_focus(expired_at + UNSEEN_FOR),
            Take::Nothing,
            "dropped"
        );

        let p = PendingScrub::new();
        p.arm(t, 1);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired(1));
        assert_eq!(
            p.take_on_focus(expired_at + (UNSEEN_FOR - Duration::from_millis(1))),
            Take::Expired(1)
        );
    }

    #[test]
    fn a_new_press_clears_an_unseen_expiry() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired(1));
        p.arm(t + S(40), 1);
        assert_eq!(p.take_on_focus(t + S(41)), Take::Run(1));
        assert_eq!(p.take_on_focus(t + S(42)), Take::Nothing);
    }

    #[test]
    fn a_scrub_without_waiting_clears_an_unseen_expiry() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired(1));
        p.clear();
        assert_eq!(p.take_on_focus(t + S(41)), Take::Nothing);
    }

    #[test]
    fn a_second_press_restarts_the_timer_s_wait() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        p.arm(t + S(20), 1);
        assert_eq!(
            p.timer_step(t + PENDING_FOR),
            Timer::Sleep(S(20)),
            "the second press restarted the wait"
        );
        assert_eq!(p.timer_step(t + S(20) + PENDING_FOR), Timer::Expired(1));
    }

    #[test]
    fn the_timer_stays_quiet_after_a_focus_or_a_scrub() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        assert_eq!(p.take_on_focus(t + PENDING_FOR), Take::Lapsed(1));
        assert_eq!(
            p.timer_step(t + PENDING_FOR),
            Timer::Done,
            "the focus told the user already"
        );

        p.arm(t, 1);
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
        let t = When::now();
        assert!(p.arm(t, 1), "the first press needs a timer");
        for i in 1..50 {
            assert!(!p.arm(t + Duration::from_millis(i), 1), "one timer runs");
        }
        // The one timer follows the last press and then stops.
        let last = t + Duration::from_millis(49);
        assert_eq!(
            p.timer_step(t + PENDING_FOR),
            Timer::Sleep(S(0) + Duration::from_millis(49))
        );
        assert_eq!(p.timer_step(last + PENDING_FOR), Timer::Expired(1));
        assert!(
            p.arm(last + S(40), 1),
            "the timer is gone: a new press needs one"
        );
    }

    #[test]
    fn a_timer_that_finds_nothing_stops_and_frees_the_next_press() {
        let p = PendingScrub::new();
        let t = When::now();
        assert!(p.arm(t, 1));
        assert_eq!(p.take_on_focus(t + S(1)), Take::Run(1));
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Done);
        assert!(p.arm(t + S(40), 1));
    }

    #[test]
    fn a_timer_that_could_not_start_frees_the_next_press() {
        let p = PendingScrub::new();
        let t = When::now();
        assert!(p.arm(t, 1));
        p.timer_failed(t);
        assert!(p.arm(t + S(1), 1));
    }

    #[test]
    fn a_timer_that_could_not_start_expires_the_press_at_once() {
        let p = PendingScrub::new();
        let t = When::now();
        assert!(p.arm(t, 1));
        assert_eq!(
            p.timer_failed(t),
            Some(1),
            "no timer will say it: tell the user now"
        );
        // No late scrub: the window says once that it did not run.
        assert_eq!(p.take_on_focus(t + S(1)), Take::Expired(1));
        assert_eq!(p.take_on_focus(t + S(2)), Take::Nothing);
    }

    #[test]
    fn a_timer_that_could_not_start_after_a_focus_says_nothing() {
        let p = PendingScrub::new();
        let t = When::now();
        assert!(p.arm(t, 1));
        assert_eq!(p.take_on_focus(t), Take::Run(1));
        assert_eq!(p.timer_failed(t), None, "the scrub ran");
        assert_eq!(p.take_on_focus(t + S(1)), Take::Nothing);
    }

    #[test]
    fn a_timer_that_panics_frees_the_next_press() {
        let p = PendingScrub::new();
        let t = When::now();
        assert!(p.arm(t, 1));
        let died = std::thread::scope(|s| {
            s.spawn(|| {
                let mut timer = p.timer();
                assert_eq!(timer.step(t + S(1)), Timer::Sleep(S(29)));
                panic!("the timer thread dies");
            })
            .join()
        });
        assert!(died.is_err());
        assert!(
            p.arm(t + S(2), 1),
            "the timer is gone: a new press needs one"
        );
    }

    #[test]
    fn a_timer_that_ends_normally_leaves_the_next_timer_alone() {
        let p = PendingScrub::new();
        let t = When::now();
        assert!(p.arm(t, 1));
        let mut first = p.timer();
        assert_eq!(first.step(t + PENDING_FOR), Timer::Expired(1));
        assert!(
            p.arm(t + S(31), 1),
            "a new press after the notice starts a timer"
        );
        // The first thread ends after its notice, while the second timer runs.
        drop(first);
        assert!(!p.arm(t + S(32), 1), "still one timer");
    }

    /// A suspend: the monotonic clock stood still, the wall clock moved on.
    fn after_a_suspend(t: When, asleep: Duration) -> When {
        When {
            mono: t.mono + S(1),
            wall: t.wall + asleep,
        }
    }

    #[test]
    fn a_press_from_before_a_suspend_lapses_on_focus() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        assert_eq!(
            p.take_on_focus(after_a_suspend(t, S(3600))),
            Take::Lapsed(1)
        );
    }

    #[test]
    fn the_timer_expires_a_press_from_before_a_suspend() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        assert_eq!(
            p.timer_step(after_a_suspend(t, PENDING_FOR)),
            Timer::Expired(1)
        );
    }

    #[test]
    fn an_unseen_expiry_from_before_a_long_suspend_ages_out() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired(1));
        let resumed = after_a_suspend(t + PENDING_FOR, UNSEEN_FOR);
        assert_eq!(p.take_on_focus(resumed), Take::Nothing);
    }

    #[test]
    fn a_wall_clock_set_back_does_not_end_the_wait() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        let set_back = When {
            mono: t.mono + S(1),
            wall: t.wall - S(3600),
        };
        assert_eq!(p.take_on_focus(set_back), Take::Run(1));
    }

    /// By design: the wall clock cannot tell a clock set forward from a suspend, so the wait
    /// ends early. The press is then reported as not cleaned; it never scrubs late.
    #[test]
    fn a_wall_clock_set_forward_expires_early() {
        let p = PendingScrub::new();
        let t = When::now();
        p.arm(t, 1);
        let set_forward = When {
            mono: t.mono + S(1),
            wall: t.wall + S(3600),
        };
        assert_eq!(p.timer_step(set_forward), Timer::Expired(1));

        let p = PendingScrub::new();
        p.arm(t, 1);
        assert_eq!(p.take_on_focus(set_forward), Take::Lapsed(1));
    }

    #[test]
    fn the_expiry_and_the_take_carry_the_armed_press() {
        let t = When::now();
        let p = PendingScrub::new();
        p.arm(t, 3);
        assert_eq!(p.timer_step(t + PENDING_FOR), Timer::Expired(3));
        assert_eq!(p.take_on_focus(t + S(31)), Take::Expired(3));

        p.arm(t, 4);
        assert_eq!(p.take_on_focus(t + S(1)), Take::Run(4));

        p.arm(t, 5);
        assert_eq!(p.take_on_focus(t + PENDING_FOR), Take::Lapsed(5));

        p.arm(t, 6);
        assert_eq!(p.timer_failed(t), Some(6));
        assert_eq!(p.timer_failed(t), None, "told once");
    }

    #[test]
    fn a_re_arm_carries_the_later_press() {
        let t = When::now();
        let p = PendingScrub::new();
        p.arm(t, 1);
        p.arm(t + S(20), 2);
        assert_eq!(p.take_on_focus(t + S(21)), Take::Run(2));

        // Two presses that arm out of order: the later press still wins.
        p.arm(t, 5);
        p.arm(t + S(1), 4);
        assert_eq!(p.timer_step(t + S(1) + PENDING_FOR), Timer::Expired(5));
    }

    #[test]
    fn focus_without_press_is_nothing() {
        let p = PendingScrub::new();
        assert_eq!(p.take_on_focus(When::now()), Take::Nothing);
    }
}
