//! The Linux desktop services over D-Bus: logind and the screen savers (wipe on lock and sleep),
//! and the StatusNotifier watcher (whether the desktop shows a tray).
//!
//! These tests talk to a system bus and a session bus, so they are `#[ignore]`d and panic unless
//! `REDACTOR_TEST_BUS` says both buses are private ones. Only `tools/ci/x11-tests.sh` sets it: it
//! starts a private session bus and a private bus with a logind mock (python3-dbusmock) that
//! stands in for the system bus, and runs these tests one at a time.
#![cfg(target_os = "linux")]

use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use redactor_app::platform::linux::lock::LogindLock;
use redactor_app::platform::linux::tray;
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

/// Adds a session to the logind mock and returns its object path.
fn add_session(system: &Connection, id: &str) -> OwnedObjectPath {
    mock_call(
        system,
        MANAGER_PATH,
        MOCK,
        "AddSession",
        &(id, "seat0", 1000u32, "redactor", true),
    )
    .body()
    .deserialize::<String>()
    .expect("AddSession returns the session path")
    .try_into()
    .expect("an object path")
}

/// Started by the user's service manager, the app is in no session of its own. logind then
/// names the user's display session (`auto`), and that session's lock wipes.
#[test]
#[ignore = "needs private D-Bus buses with a logind mock: tools/ci/x11-tests.sh"]
fn the_watcher_finds_the_display_session() {
    private_buses();
    let system = Connection::system().expect("connect to the private system bus");
    let session = add_session(&system, &format!("display{}", std::process::id()));
    // No session for this pid, and no session id that logind knows: only `auto` answers.
    let no_session = "raise dbus.exceptions.DBusException(\
        'no session', name='org.freedesktop.login1.NoSuchSession')";
    mock_call(
        &system,
        MANAGER_PATH,
        MOCK,
        "AddMethod",
        &(MANAGER, "GetSessionByPID", "u", "o", no_session),
    );
    mock_call(
        &system,
        MANAGER_PATH,
        MOCK,
        "AddMethod",
        &(
            MANAGER,
            "GetSession",
            "s",
            "o",
            format!(
                "if args[0] != 'auto':\n    {no_session}\nret = '{}'",
                session.as_str()
            ),
        ),
    );

    let events = watcher();
    mock_call(&system, session.as_str(), SESSION, "Lock", &());
    expect(&events, WipeReason::Lock, "Lock on the display session");
}

#[test]
#[ignore = "needs private D-Bus buses with a logind mock: tools/ci/x11-tests.sh"]
fn the_watcher_hears_logind() {
    private_buses();
    let system = Connection::system().expect("connect to the private system bus");
    let session = add_session(&system, &format!("redactor{}", std::process::id()));
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

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";

fn name_has_owner(bus: &Connection, name: &str) -> bool {
    bus.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "NameHasOwner",
        &(name,),
    )
    .expect("NameHasOwner")
    .body()
    .deserialize::<bool>()
    .expect("a boolean")
}

/// Polls `done` every 20 ms until it holds or `WAIT` passes.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "{what} within {WAIT:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A StatusNotifier watcher (python3-dbusmock) on the private session bus; gone when dropped.
struct MockWatcher {
    child: Child,
    bus: Connection,
}

impl MockWatcher {
    fn start(bus: &Connection) -> Self {
        // A pyenv or toolcache python3 does not see the system's python3-dbusmock.
        let python = ["python3", "/usr/bin/python3"]
            .into_iter()
            .find(|p| {
                Command::new(p)
                    .args(["-c", "import dbusmock"])
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success())
            })
            .expect("python3-dbusmock is installed");
        let child = Command::new(python)
            .args([
                "-m",
                "dbusmock",
                "--session",
                WATCHER,
                WATCHER_PATH,
                WATCHER,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the watcher mock");
        let mock = MockWatcher {
            child,
            bus: bus.clone(),
        };
        wait_until("the watcher mock owns its name", || {
            name_has_owner(bus, WATCHER)
        });
        mock
    }

    fn call<B>(&self, iface: &str, method: &str, body: &B)
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        self.bus
            .call_method(Some(WATCHER), WATCHER_PATH, Some(iface), method, body)
            .unwrap_or_else(|e| panic!("{iface}.{method} on the watcher mock: {e}"));
    }
}

impl Drop for MockWatcher {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let bus = self.bus.clone();
        wait_until("the watcher mock's name is released", || {
            !name_has_owner(&bus, WATCHER)
        });
    }
}

#[test]
#[ignore = "needs private D-Bus buses with a logind mock: tools/ci/x11-tests.sh"]
fn no_watcher_means_no_tray() {
    private_buses();
    let bus = Connection::session().expect("connect to the private session bus");
    assert!(!name_has_owner(&bus, WATCHER));
    assert!(!tray::host_registered(&bus));
}

#[test]
#[ignore = "needs private D-Bus buses with a logind mock: tools/ci/x11-tests.sh"]
fn host_registered_reads_the_watcher() {
    private_buses();
    let bus = Connection::session().expect("connect to the private session bus");
    let mock = MockWatcher::start(&bus);
    let property = "IsStatusNotifierHostRegistered";
    mock.call(
        "org.freedesktop.DBus.Mock",
        "AddProperty",
        &(WATCHER, property, Value::from(true)),
    );
    assert!(tray::host_registered(&bus), "a host is registered");
    mock.call(
        "org.freedesktop.DBus.Properties",
        "Set",
        &(WATCHER, property, Value::from(false)),
    );
    assert!(!tray::host_registered(&bus), "no host is registered");
}

/// A watcher that owns its name but never answers (a hung panel): every start, a second one that
/// hands `--scrub-clipboard` over included, asks it before anything else. The answer is "no
/// tray", within the probe's deadline.
#[test]
#[ignore = "needs private D-Bus buses with a logind mock: tools/ci/x11-tests.sh"]
fn a_watcher_that_never_answers_means_no_tray_in_time() {
    private_buses();
    let bus = Connection::session().expect("connect to the private session bus");
    // A connection without an object server answers no method call.
    let silent = Connection::session().expect("connect the silent watcher");
    silent
        .request_name(WATCHER)
        .expect("the silent watcher owns its name");
    assert!(name_has_owner(&bus, WATCHER));

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(tray::available());
    });
    let limit = Duration::from_secs(5);
    match rx.recv_timeout(limit) {
        Ok(shown) => assert!(!shown, "a watcher that never answers is no tray"),
        Err(_) => panic!("the tray probe still waited for the watcher after {limit:?}"),
    }
    drop(silent);
    wait_until("the silent watcher's name is released", || {
        !name_has_owner(&bus, WATCHER)
    });
}
