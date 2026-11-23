//! Session lock and sleep (`wipeOnLock`) on Linux.
//!
//! Two threads, each on its own D-Bus connection. On the system bus, logind:
//! `PrepareForSleep(true)` on the manager, and `Lock` and a `LockedHint` change on this
//! process's own session. On the session bus, a screen saver's `ActiveChanged(true)`. Only what
//! sends these is heard: a locker started on its own (swaylock, i3lock) that tells neither logind
//! nor a screen saver does not wipe. A bus that cannot be reached at the start ends its thread
//! quietly. Whether either bus has a lock source at all (this process's logind session, a screen
//! saver that owns its name) is known shortly after the start ([`LockWatcher::listens`]);
//! without one, a screen lock does not wipe here, and suspend, idle and the tray still do. A bus
//! that was watched and goes away (its connection breaks, the bus restarts) is subscribed to
//! again, after a pause that grows from [`RETRY`] to [`RETRY_MAX`] while it stays away, for as
//! long as the app runs. Signals carry no content and nothing here logs.
//! (Named `lock.rs`; `session.rs` is the desktop session type.)

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use zbus::blocking::{connection, Connection, MessageIterator};
use zbus::message::Type;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{Address, Message};

use crate::platform::LockWatcher;
use crate::wipe::WipeReason;

const LOGIND: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const SESSION: &str = "org.freedesktop.login1.Session";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// The screen-saver interfaces whose `ActiveChanged(true)` means the screen locked.
const SCREEN_SAVERS: [&str; 5] = [
    "org.freedesktop.ScreenSaver",
    "org.gnome.ScreenSaver",
    "org.cinnamon.ScreenSaver",
    "org.mate.ScreenSaver",
    "org.xfce.ScreenSaver",
];

/// The wipe a signal asks for. `flag` is the signal's boolean (`LockedHint` for its property
/// change), `None` where it has none.
pub fn reason(interface: &str, member: &str, flag: Option<bool>) -> Option<WipeReason> {
    match (interface, member, flag) {
        (MANAGER, "PrepareForSleep", Some(true)) => Some(WipeReason::Sleep),
        (SESSION, "Lock", _) | (SESSION, "LockedHint", Some(true)) => Some(WipeReason::Lock),
        (iface, "ActiveChanged", Some(true)) if SCREEN_SAVERS.contains(&iface) => {
            Some(WipeReason::Lock)
        }
        _ => None,
    }
}

/// The first pause before a watched bus that went away is subscribed to again.
const RETRY: Duration = Duration::from_secs(1);
/// The longest pause between two tries while that bus stays away.
const RETRY_MAX: Duration = Duration::from_secs(30);

type Callback = Arc<dyn Fn(WipeReason) + Send + Sync>;

/// Watches logind and the screen savers.
#[derive(Default)]
pub struct LogindLock {
    found: Mutex<Found>,
}

impl LockWatcher for LogindLock {
    fn spawn(&self, on_event: Box<dyn Fn(WipeReason) + Send + Sync>) {
        let buses = Self::watch(on_event);
        self.found.lock().unwrap_or_else(|e| e.into_inner()).buses = Some(buses);
    }

    fn listens(&self, wait: Duration) -> Option<bool> {
        self.found
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .within(wait)
    }
}

/// Whether a lock source was found, from each bus's first try.
#[derive(Default)]
struct Found {
    /// One message per bus: whether it has a lock source. Its sender ends with that bus's try.
    buses: Option<mpsc::Receiver<bool>>,
    /// Kept once known.
    verdict: Option<bool>,
}

impl Found {
    /// True as soon as one bus has a lock source; false once every bus said it has none (or its
    /// thread is gone); `None` while that is not known after `wait`, or before the watch starts.
    fn within(&mut self, wait: Duration) -> Option<bool> {
        if self.verdict.is_none() {
            let buses = self.buses.as_ref()?;
            let deadline = Instant::now() + wait;
            self.verdict = loop {
                match buses.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(true) => break Some(true),
                    Ok(false) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break Some(false),
                    Err(mpsc::RecvTimeoutError::Timeout) => return None,
                }
            };
        }
        self.verdict
    }
}

impl LogindLock {
    /// Starts watching, as `spawn` does. The receiver gets one message per bus once that bus is
    /// watched or given up on at the first try (whether it has a lock source), so a test can
    /// emit signals only after both are subscribed. Both bus addresses and `XDG_SESSION_ID` are
    /// read before this returns.
    pub fn watch(on_event: Box<dyn Fn(WipeReason) + Send + Sync>) -> mpsc::Receiver<bool> {
        let on_event: Callback = Arc::from(on_event);
        let (ready, rx) = mpsc::channel();
        let system = Address::system().ok();
        let session = Address::session().ok();
        let session_id = std::env::var("XDG_SESSION_ID").ok();
        {
            let (on_event, ready) = (on_event.clone(), ready.clone());
            let _ = std::thread::Builder::new()
                .name("lock-logind".into())
                .spawn(move || {
                    let Some(system) = system else {
                        let _ = ready.send(false);
                        return;
                    };
                    // Lock signals come only for this process's own session.
                    let found = &Cell::new(false);
                    keep_watching(
                        || {
                            let (messages, own) = watch_logind(system.clone(), session_id.clone())?;
                            found.set(own.is_some());
                            Ok(messages
                                .map_while(Result::ok)
                                .filter_map(move |msg| logind_reason(&msg, own.as_ref())))
                        },
                        // Moves the sender, so it ends with the first try.
                        move || {
                            let _ = ready.send(found.get());
                        },
                        |r| on_event(r),
                        pause,
                    );
                });
        }
        let _ = std::thread::Builder::new()
            .name("lock-screensaver".into())
            .spawn(move || {
                let Some(session) = session else {
                    let _ = ready.send(false);
                    return;
                };
                let found = &Cell::new(false);
                keep_watching(
                    || {
                        let (messages, any) = watch_screen_savers(session.clone())?;
                        found.set(any);
                        Ok(messages
                            .map_while(Result::ok)
                            .filter_map(|msg| screen_saver_reason(&msg)))
                    },
                    move || {
                        let _ = ready.send(found.get());
                    },
                    |r| on_event(r),
                    pause,
                );
            });
        rx
    }
}

/// Watches one bus for as long as the app runs: `subscribe` connects and subscribes, each event
/// of its stream goes to `on_event`, and when the stream ends (the connection broke, the bus went
/// away) it subscribes again. `ready` runs once, after the first try; a first try that fails
/// gives up (there is no such bus here). Before each new try it waits through `pause`: [`RETRY`]
/// after a stream that worked, then twice as long after each try that fails, up to
/// [`RETRY_MAX`]. `pause` returning `false` stops the watch (the tests).
fn keep_watching<E, I: Iterator<Item = E>>(
    mut subscribe: impl FnMut() -> zbus::Result<I>,
    ready: impl FnOnce(),
    mut on_event: impl FnMut(E),
    mut pause: impl FnMut(Duration) -> bool,
) {
    let first = subscribe();
    ready();
    let Ok(mut events) = first else {
        return;
    };
    loop {
        events.for_each(&mut on_event);
        let mut wait = RETRY;
        events = loop {
            if !pause(wait) {
                return;
            }
            match subscribe() {
                Ok(events) => break events,
                Err(_) => wait = (wait * 2).min(RETRY_MAX),
            }
        };
    }
}

/// The pause between two tries in the app: a sleep, never stopping.
fn pause(wait: Duration) -> bool {
    std::thread::sleep(wait);
    true
}

fn connect(address: Address) -> zbus::Result<Connection> {
    connection::Builder::address(address)?.build()
}

/// Asks the bus to route the signals matching `rule` to this connection.
fn add_match(conn: &Connection, rule: &str) -> zbus::Result<()> {
    conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "AddMatch",
        &(rule,),
    )
    .map(drop)
}

/// This process's logind session: by pid, else by `$XDG_SESSION_ID`, else the one logind names
/// for `auto` (the caller's session or, for an app the user's service manager started, which is
/// in no session of its own, the user's display session).
fn own_session(conn: &Connection, session_id: Option<String>) -> Option<OwnedObjectPath> {
    let path = |reply: zbus::Result<zbus::Message>| {
        reply.ok()?.body().deserialize::<OwnedObjectPath>().ok()
    };
    let by_id = |id: &str| {
        path(conn.call_method(
            Some(LOGIND),
            MANAGER_PATH,
            Some(MANAGER),
            "GetSession",
            &(id,),
        ))
    };
    path(conn.call_method(
        Some(LOGIND),
        MANAGER_PATH,
        Some(MANAGER),
        "GetSessionByPID",
        &(std::process::id(),),
    ))
    .or_else(|| by_id(&session_id?))
    .or_else(|| by_id("auto"))
}

/// Subscribes to logind's sleep signal and, where this process has a session, its lock signals.
fn watch_logind(
    address: Address,
    session_id: Option<String>,
) -> zbus::Result<(MessageIterator, Option<OwnedObjectPath>)> {
    let conn = connect(address)?;
    let own = own_session(&conn, session_id);
    // Before the match rules, so no signal that arrives after them is missed.
    let messages = MessageIterator::from(&conn);
    let from_logind = format!("type='signal',sender='{LOGIND}'");
    add_match(
        &conn,
        &format!(
            "{from_logind},path='{MANAGER_PATH}',interface='{MANAGER}',member='PrepareForSleep'"
        ),
    )?;
    if let Some(path) = &own {
        let path = path.as_str();
        add_match(
            &conn,
            &format!("{from_logind},path='{path}',interface='{SESSION}',member='Lock'"),
        )?;
        add_match(
            &conn,
            &format!(
                "{from_logind},path='{path}',interface='{PROPERTIES}',\
                 member='PropertiesChanged',arg0='{SESSION}'"
            ),
        )?;
    }
    Ok((messages, own))
}

/// Subscribes to the screen savers' `ActiveChanged`, and says whether any screen saver owns its
/// name on the bus now (each one's name is its interface's).
fn watch_screen_savers(address: Address) -> zbus::Result<(MessageIterator, bool)> {
    let conn = connect(address)?;
    let messages = MessageIterator::from(&conn);
    for iface in SCREEN_SAVERS {
        add_match(
            &conn,
            &format!("type='signal',interface='{iface}',member='ActiveChanged'"),
        )?;
    }
    let any = SCREEN_SAVERS.iter().any(|name| has_owner(&conn, name));
    Ok((messages, any))
}

/// True when a program owns `name` on the bus.
fn has_owner(conn: &Connection, name: &str) -> bool {
    conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "NameHasOwner",
        &(name,),
    )
    .ok()
    .and_then(|reply| reply.body().deserialize::<bool>().ok())
    .unwrap_or(false)
}

/// The interface and member of a signal, `None` for any other message.
fn signal_name(msg: &Message) -> Option<(String, String)> {
    if msg.message_type() != Type::Signal {
        return None;
    }
    let header = msg.header();
    Some((
        header.interface()?.to_string(),
        header.member()?.to_string(),
    ))
}

fn logind_reason(msg: &Message, own: Option<&OwnedObjectPath>) -> Option<WipeReason> {
    let (iface, member) = signal_name(msg)?;
    let header = msg.header();
    let path = header.path()?.as_str();
    let body = msg.body();
    if (iface.as_str(), member.as_str()) == (MANAGER, "PrepareForSleep") {
        if path != MANAGER_PATH {
            return None;
        }
        return reason(&iface, &member, body.deserialize::<bool>().ok());
    }
    if Some(path) != own.map(|p| p.as_str()) {
        return None;
    }
    if (iface.as_str(), member.as_str()) == (PROPERTIES, "PropertiesChanged") {
        type Changed = (String, HashMap<String, OwnedValue>, Vec<String>);
        let (changed_iface, changed, _): Changed = body.deserialize().ok()?;
        if changed_iface != SESSION {
            return None;
        }
        let hint = changed
            .get("LockedHint")
            .and_then(|v| bool::try_from(v).ok());
        return reason(SESSION, "LockedHint", hint);
    }
    reason(&iface, &member, None)
}

fn screen_saver_reason(msg: &Message) -> Option<WipeReason> {
    let (iface, member) = signal_name(msg)?;
    reason(&iface, &member, msg.body().deserialize::<bool>().ok())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// `keep_watching` with scripted subscriptions (`Ok` with the events it delivers before its
    /// stream ends, or `Err`), stopped at the pause after the last one. Returns the events in
    /// order, how many times it subscribed, how often ready was sent, and each pause.
    fn run(script: Vec<zbus::Result<Vec<u32>>>) -> (Vec<u32>, usize, usize, Vec<Duration>) {
        let script = RefCell::new(script.into_iter());
        let subscribed = RefCell::new(0);
        let mut events = Vec::new();
        let mut ready = 0;
        let mut pauses = Vec::new();
        keep_watching(
            || {
                *subscribed.borrow_mut() += 1;
                let next = script.borrow_mut().next();
                next.unwrap_or(Ok(Vec::new())).map(Vec::into_iter)
            },
            || ready += 1,
            |e| events.push(e),
            |d| {
                pauses.push(d);
                script.borrow().len() > 0
            },
        );
        let subscribed = *subscribed.borrow();
        (events, subscribed, ready, pauses)
    }

    fn gone() -> zbus::Error {
        zbus::Error::Failure("no bus".into())
    }

    #[test]
    fn an_ended_stream_is_subscribed_again() {
        let (events, subscribed, ready, pauses) =
            run(vec![Ok(vec![1, 2]), Ok(vec![3]), Ok(vec![4])]);
        assert_eq!(events, [1, 2, 3, 4], "every stream is heard");
        assert_eq!(subscribed, 3, "one subscription per ended stream");
        assert_eq!(ready, 1, "ready once, after the first subscription");
        assert_eq!(
            pauses, [RETRY; 3],
            "a working subscription starts the backoff over"
        );
    }

    #[test]
    fn a_bus_that_stays_away_is_tried_with_a_growing_capped_pause() {
        let mut script = vec![Ok(vec![1])];
        script.extend((0..8).map(|_| Err(gone())));
        script.push(Ok(vec![2]));
        let (events, subscribed, ready, pauses) = run(script);
        assert_eq!(events, [1, 2], "heard again once the bus is back");
        assert_eq!(subscribed, 10);
        assert_eq!(ready, 1);
        let secs: Vec<u64> = pauses.iter().map(Duration::as_secs).collect();
        assert_eq!(secs, [1, 2, 4, 8, 16, 30, 30, 30, 30, 1]);
        assert!(pauses.iter().all(|p| *p <= RETRY_MAX));
    }

    #[test]
    fn a_bus_missing_at_the_start_is_given_up_on() {
        let (events, subscribed, ready, pauses) = run(vec![Err(gone()), Ok(vec![1])]);
        assert!(events.is_empty(), "never subscribed");
        assert_eq!(subscribed, 1, "one try");
        assert_eq!(ready, 1, "ready all the same");
        assert!(pauses.is_empty(), "no retry");
    }

    /// `Found` over a channel the test feeds as the two buses would.
    fn found() -> (Found, mpsc::Sender<bool>, mpsc::Sender<bool>) {
        let (logind, rx) = mpsc::channel();
        let screen_saver = logind.clone();
        let f = Found {
            buses: Some(rx),
            verdict: None,
        };
        (f, logind, screen_saver)
    }

    const NOW: Duration = Duration::from_millis(20);

    #[test]
    fn one_bus_with_a_lock_source_is_enough() {
        let (mut f, logind, screen_saver) = found();
        logind.send(false).unwrap();
        screen_saver.send(true).unwrap();
        assert_eq!(f.within(NOW), Some(true));
        drop((logind, screen_saver));
        assert_eq!(f.within(NOW), Some(true), "kept");
    }

    #[test]
    fn no_lock_source_on_either_bus_is_known_once_both_have_spoken() {
        let (mut f, logind, screen_saver) = found();
        logind.send(false).unwrap();
        drop(logind);
        assert_eq!(f.within(NOW), None, "the session bus has not answered yet");
        screen_saver.send(false).unwrap();
        drop(screen_saver);
        assert_eq!(f.within(NOW), Some(false));
    }

    #[test]
    fn a_bus_thread_that_never_ran_counts_as_none() {
        let (mut f, logind, screen_saver) = found();
        drop(logind);
        drop(screen_saver);
        assert_eq!(f.within(NOW), Some(false));
    }

    #[test]
    fn before_the_watch_starts_nothing_is_known() {
        assert_eq!(Found::default().within(NOW), None);
        assert_eq!(LogindLock::default().listens(NOW), None);
    }

    #[test]
    fn sleep_only_when_going_down() {
        assert_eq!(
            reason(MANAGER, "PrepareForSleep", Some(true)),
            Some(WipeReason::Sleep)
        );
        assert_eq!(reason(MANAGER, "PrepareForSleep", Some(false)), None);
        assert_eq!(reason(MANAGER, "PrepareForSleep", None), None);
    }

    #[test]
    fn lock_signal_is_a_lock() {
        assert_eq!(reason(SESSION, "Lock", None), Some(WipeReason::Lock));
        assert_eq!(reason(SESSION, "Lock", Some(false)), Some(WipeReason::Lock));
    }

    #[test]
    fn locked_hint_true_is_a_lock() {
        assert_eq!(
            reason(SESSION, "LockedHint", Some(true)),
            Some(WipeReason::Lock)
        );
        assert_eq!(reason(SESSION, "LockedHint", Some(false)), None);
        assert_eq!(reason(SESSION, "LockedHint", None), None);
    }

    #[test]
    fn screen_saver_active_is_a_lock() {
        for iface in SCREEN_SAVERS {
            assert_eq!(
                reason(iface, "ActiveChanged", Some(true)),
                Some(WipeReason::Lock),
                "{iface}"
            );
            assert_eq!(reason(iface, "ActiveChanged", Some(false)), None, "{iface}");
            assert_eq!(reason(iface, "ActiveChanged", None), None, "{iface}");
        }
    }

    #[test]
    fn other_signals_are_ignored() {
        for (iface, member, flag) in [
            (MANAGER, "Lock", None),
            (MANAGER, "PrepareForShutdown", Some(true)),
            (SESSION, "Unlock", None),
            (SESSION, "PrepareForSleep", Some(true)),
            (SESSION, "IdleHint", Some(true)),
            ("org.kde.screensaver", "ActiveChanged", Some(true)),
            ("org.gnome.ScreenSaver", "WakeUpScreen", None),
            ("org.freedesktop.DBus", "NameAcquired", None),
            ("", "", Some(true)),
        ] {
            assert_eq!(reason(iface, member, flag), None, "{iface}.{member}");
        }
    }
}
