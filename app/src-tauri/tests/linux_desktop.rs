//! The Linux desktop services over D-Bus: logind and the screen savers (wipe on lock and sleep).
//!
//! These tests talk to a system bus and a session bus, so they are `#[ignore]`d and panic unless
//! `REDACTOR_TEST_BUS` says both buses are private ones. Only `tools/ci/x11-tests.sh` sets it: it
//! starts a private session bus and a private bus with a logind mock (python3-dbusmock) that
//! stands in for the system bus, and runs these tests one at a time.
#![cfg(target_os = "linux")]

use std::sync::mpsc;
use std::time::Duration;

use redactor_app::platform::linux::lock::LogindLock;
use redactor_app::platform::LockWatcher;
use redactor_app::wipe::WipeReason;
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, Value};

const LOGIND: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const SESSION: &str = "org.freedesktop.login1.Session";
const MOCK: &str = "org.freedesktop.DBus.Mock";
const WAIT: Duration = Duration::from_secs(2);

/// Panics unless the buses are the throwaway ones `tools/ci/x11-tests.sh` starts.
fn private_buses() {
    for var in [
        "REDACTOR_TEST_BUS",
        "DBUS_SESSION_BUS_ADDRESS",
        "DBUS_SYSTEM_BUS_ADDRESS",
    ] {
        assert!(
            std::env::var_os(var).is_some(),
            "{var} is not set: run tools/ci/x11-tests.sh"
        );
    }
}

/// A watcher whose events arrive on the returned channel, started once both buses are watched.
fn watcher() -> mpsc::Receiver<WipeReason> {
    let (tx, rx) = mpsc::channel();
    let ready = LogindLock::watch(Box::new(move |reason| {
        let _ = tx.send(reason);
    }));
    for bus in ["system", "session"] {
        ready
            .recv_timeout(WAIT)
            .unwrap_or_else(|e| panic!("the watcher did not get ready on a bus ({bus}): {e}"));
    }
    rx
}

fn expect(events: &mpsc::Receiver<WipeReason>, want: WipeReason, after: &str) {
    match events.recv_timeout(WAIT) {
        Ok(got) => assert_eq!(got, want, "after {after}"),
        Err(e) => panic!("no {want:?} within {WAIT:?} after {after}: {e}"),
    }
}

fn mock_call<B>(bus: &Connection, path: &str, iface: &str, method: &str, body: &B) -> zbus::Message
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    bus.call_method(Some(LOGIND), path, Some(iface), method, body)
        .unwrap_or_else(|e| panic!("{iface}.{method} on the logind mock: {e}"))
}

#[test]
#[ignore = "needs private D-Bus buses with a logind mock: tools/ci/x11-tests.sh"]
fn the_watcher_hears_logind() {
    private_buses();
    let system = Connection::system().expect("connect to the private system bus");
    let id = format!("redactor{}", std::process::id());
    let session: OwnedObjectPath = mock_call(
        &system,
        MANAGER_PATH,
        MOCK,
        "AddSession",
        &(id.as_str(), "seat0", 1000u32, "redactor", true),
    )
    .body()
    .deserialize::<String>()
    .expect("AddSession returns the session path")
    .try_into()
    .expect("an object path");
    // The template has GetSession but not GetSessionByPID: this process belongs to the session.
    mock_call(
        &system,
        MANAGER_PATH,
        MOCK,
        "AddMethod",
        &(
            MANAGER,
            "GetSessionByPID",
            "u",
            "o",
            format!("ret = '{}'", session.as_str()),
        ),
    );

    let events = watcher();
    mock_call(
        &system,
        MANAGER_PATH,
        MOCK,
        "EmitSignal",
        &(MANAGER, "PrepareForSleep", "b", vec![Value::from(true)]),
    );
    expect(&events, WipeReason::Sleep, "PrepareForSleep(true)");
    mock_call(&system, session.as_str(), SESSION, "Lock", &());
    expect(&events, WipeReason::Lock, "Lock");
    mock_call(
        &system,
        session.as_str(),
        SESSION,
        "SetLockedHint",
        &(true,),
    );
    expect(&events, WipeReason::Lock, "LockedHint = true");
}

#[test]
#[ignore = "needs private D-Bus buses with a logind mock: tools/ci/x11-tests.sh"]
fn the_watcher_hears_a_screen_saver() {
    private_buses();
    let events = watcher();
    // Another program on the session bus, as gnome-screensaver would be.
    let other = Connection::session().expect("connect to the private session bus");
    for active in [false, true] {
        other
            .emit_signal(
                None::<&str>,
                "/org/gnome/ScreenSaver",
                "org.gnome.ScreenSaver",
                "ActiveChanged",
                &(active,),
            )
            .expect("emit ActiveChanged");
    }
    expect(
        &events,
        WipeReason::Lock,
        "org.gnome.ScreenSaver.ActiveChanged(true)",
    );
    assert!(
        events.recv_timeout(Duration::from_millis(300)).is_err(),
        "ActiveChanged(false) must not wipe"
    );
}

#[test]
#[ignore = "needs private D-Bus buses with a logind mock: tools/ci/x11-tests.sh"]
fn no_bus_is_not_an_error() {
    private_buses();
    let nowhere = std::env::temp_dir().join(format!("redactor-no-bus-{}", std::process::id()));
    let nowhere = format!("unix:path={}", nowhere.display());
    let saved: Vec<_> = ["DBUS_SYSTEM_BUS_ADDRESS", "DBUS_SESSION_BUS_ADDRESS"]
        .into_iter()
        .map(|var| (var, std::env::var_os(var)))
        .collect();
    let (tx, rx) = mpsc::channel();
    // The tests run one at a time; `spawn` reads both addresses before it returns.
    for (var, _) in &saved {
        std::env::set_var(var, &nowhere);
    }
    LogindLock.spawn(Box::new(move |reason| {
        let _ = tx.send(reason);
    }));
    for (var, value) in saved {
        match value {
            Some(v) => std::env::set_var(var, v),
            None => std::env::remove_var(var),
        }
    }
    assert!(
        rx.recv_timeout(Duration::from_millis(500)).is_err(),
        "no bus, no event"
    );
}
