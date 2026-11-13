//! Session lock and sleep (`wipeOnLock`) on Linux.
//!
//! Two threads, each on its own D-Bus connection. On the system bus, logind:
//! `PrepareForSleep(true)` on the manager, and `Lock` and a `LockedHint` change on this
//! process's own session. On the session bus, a screen saver's `ActiveChanged(true)`. A bus that
//! cannot be reached ends its thread quietly; the app then wipes on idle and from the tray only.
//! Signals carry no content and nothing here logs.
//! (Named `lock.rs`; `session.rs` is the desktop session type.)

use std::collections::HashMap;
use std::sync::{mpsc, Arc};

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

type Callback = Arc<dyn Fn(WipeReason) + Send + Sync>;

/// Watches logind and the screen savers.
pub struct LogindLock;

impl LockWatcher for LogindLock {
    fn spawn(&self, on_event: Box<dyn Fn(WipeReason) + Send + Sync>) {
        drop(Self::watch(on_event));
    }
}

impl LogindLock {
    /// Starts watching, as `spawn` does. The receiver gets one message per bus once that bus is
    /// watched or given up on, so a test can emit signals only after both are subscribed.
    /// Both bus addresses and `XDG_SESSION_ID` are read before this returns.
    pub fn watch(on_event: Box<dyn Fn(WipeReason) + Send + Sync>) -> mpsc::Receiver<()> {
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
                    let watching = system.and_then(|a| watch_logind(a, session_id).ok());
                    let _ = ready.send(());
                    drop(ready);
                    let Some((messages, own)) = watching else {
                        return;
                    };
                    for msg in messages.map_while(Result::ok) {
                        if let Some(r) = logind_reason(&msg, own.as_ref()) {
                            on_event(r);
                        }
                    }
                });
        }
        let _ = std::thread::Builder::new()
            .name("lock-screensaver".into())
            .spawn(move || {
                let watching = session.and_then(|a| watch_screen_savers(a).ok());
                let _ = ready.send(());
                drop(ready);
                let Some(messages) = watching else {
                    return;
                };
                for msg in messages.map_while(Result::ok) {
                    if let Some(r) = screen_saver_reason(&msg) {
                        on_event(r);
                    }
                }
            });
        rx
    }
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

/// This process's logind session: by pid, else by `$XDG_SESSION_ID`.
fn own_session(conn: &Connection, session_id: Option<String>) -> Option<OwnedObjectPath> {
    let by_pid = conn.call_method(
        Some(LOGIND),
        MANAGER_PATH,
        Some(MANAGER),
        "GetSessionByPID",
        &(std::process::id(),),
    );
    let reply = match by_pid {
        Ok(reply) => reply,
        Err(_) => conn
            .call_method(
                Some(LOGIND),
                MANAGER_PATH,
                Some(MANAGER),
                "GetSession",
                &(session_id?,),
            )
            .ok()?,
    };
    reply.body().deserialize::<OwnedObjectPath>().ok()
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

/// Subscribes to the screen savers' `ActiveChanged`.
fn watch_screen_savers(address: Address) -> zbus::Result<MessageIterator> {
    let conn = connect(address)?;
    let messages = MessageIterator::from(&conn);
    for iface in SCREEN_SAVERS {
        add_match(
            &conn,
            &format!("type='signal',interface='{iface}',member='ActiveChanged'"),
        )?;
    }
    Ok(messages)
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
    use super::*;

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
