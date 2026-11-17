//! The hotkey through the desktop's GlobalShortcuts portal, against `tools/ci/mock_portal.py`:
//! a bind that the desktop accepts, refuses or never answers, a bind call that never returns, a
//! press, a rebind, and a bus with no portal at all.
//!
//! These tests own the portal name on the session bus they run on, so they are `#[ignore]`d and
//! panic unless the bus is the private one `tools/ci/wayland-tests.sh` started (it writes the
//! address into its runtime folder) inside its throwaway session. They never talk to the real
//! session bus or a real portal. The script runs `no_portal_means_command_mode` alone with
//! `MOCK_PORTAL=none` before it starts the mock, and the others after; each test refuses the
//! other pass.
#![cfg(target_os = "linux")]

#[path = "support/wayland_session.rs"]
mod wayland_session;

use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use redactor_app::hotkey::Hotkey;
use redactor_app::os::Os;
use redactor_app::platform::linux::command_hotkey::CommandHotkey;
use redactor_app::platform::linux::portal::{portal_version, session_bus_with, PortalHotkey};
use redactor_app::platform::{GlobalHotkey, HotkeyMode, HotkeyTaken};
use zbus::blocking::Connection;
use zbus::zvariant::OwnedObjectPath;

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const MOCK: &str = "org.redactor.Mock";
const SHORTCUT: &str = "scrub-clipboard";
/// How long a press may take to reach the callback.
const PRESS: Duration = Duration::from_secs(2);
/// How long the tests wait to be sure a second callback does not come.
const QUIET: Duration = Duration::from_millis(300);
/// The test timeouts: shorter than the shipped 10 s, 30 s and 120 s, long enough for the mock.
const CALL: Duration = Duration::from_secs(5);
const REQUEST: Duration = Duration::from_secs(5);
const BIND: Duration = Duration::from_secs(5);

/// The session bus. Panics unless it is the private bus of `tools/ci/wayland-tests.sh`.
fn private_bus() -> Connection {
    assert_private_bus();
    Connection::session().expect("connect to the private session bus")
}

/// Panics unless the session bus is the private one of `tools/ci/wayland-tests.sh`.
fn assert_private_bus() {
    wayland_session::compositor();
    assert!(
        std::env::var_os("REDACTOR_TEST_BUS").is_some(),
        "REDACTOR_TEST_BUS is not set: run tools/ci/wayland-tests.sh"
    );
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .expect("DBUS_SESSION_BUS_ADDRESS is not set: run tools/ci/wayland-tests.sh");
    let runtime = std::env::var("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR is not set");
    let mark = std::fs::read_to_string(Path::new(&runtime).join("redactor-test-bus"))
        .expect("tools/ci/wayland-tests.sh wrote no bus address into XDG_RUNTIME_DIR");
    assert_eq!(
        mark.trim_end(),
        address,
        "DBUS_SESSION_BUS_ADDRESS is not the private bus of tools/ci/wayland-tests.sh"
    );
}

/// The private bus with the mock portal on it, its next binds ending as `bind` says.
fn mock_bus(bind: &str) -> Connection {
    let bus = private_bus();
    assert_ne!(
        std::env::var("MOCK_PORTAL").ok().as_deref(),
        Some("none"),
        "MOCK_PORTAL=none: this pass has no mock portal, this test needs it"
    );
    bus.call_method(Some(PORTAL), PORTAL_PATH, Some(MOCK), "SetBind", &(bind,))
        .expect("the mock portal is not running: run tools/ci/wayland-tests.sh");
    bus
}

/// A portal hotkey on its own connection (method calls give up after `call`), started, whose
/// presses arrive on the receiver. Returns its unique bus name too, which names its sessions at
/// the mock.
fn started_hotkey(
    call: Duration,
    request: Duration,
    bind: Duration,
) -> (PortalHotkey, String, mpsc::Receiver<()>) {
    assert_private_bus();
    let conn = session_bus_with(call).expect("connect to the private session bus");
    let owner = conn.unique_name().expect("a unique name").to_string();
    let hotkey = PortalHotkey::with_timeouts(conn, request, bind).expect("the portal hotkey");
    let (tx, rx) = mpsc::channel();
    hotkey.start(Arc::new(move || {
        let _ = tx.send(());
    }));
    (hotkey, owner, rx)
}

/// The owner's open sessions at the mock, each with the trigger it asked for.
fn sessions(bus: &Connection, owner: &str) -> Vec<(OwnedObjectPath, String)> {
    bus.call_method(Some(PORTAL), PORTAL_PATH, Some(MOCK), "Sessions", &(owner,))
        .expect("ask the mock for the sessions")
        .body()
        .deserialize()
        .expect("a(os)")
}

fn press(bus: &Connection) {
    bus.call_method(Some(PORTAL), PORTAL_PATH, Some(MOCK), "Press", &(SHORTCUT,))
        .expect("press at the mock");
}

fn hotkey(s: &str) -> Hotkey {
    Hotkey::parse(s, Os::Linux).unwrap()
}

#[test]
#[ignore = "needs the mock portal on a private bus: tools/ci/wayland-tests.sh"]
fn bind_then_press_calls_back() {
    let bus = mock_bus("ok");
    assert_eq!(portal_version(&bus), Some(1));
    let (hk, owner, presses) = started_hotkey(CALL, REQUEST, BIND);
    assert_eq!(hk.mode(), HotkeyMode::Portal);
    assert_eq!(hk.bind(&hotkey("Ctrl+Alt+R")), Ok(()));
    let open = sessions(&bus, &owner);
    assert_eq!(open.len(), 1, "one session: {open:?}");
    assert_eq!(open[0].1, "CTRL+ALT+r");

    press(&bus);
    presses
        .recv_timeout(PRESS)
        .expect("the press did not reach the callback within 2 s");
    assert!(
        presses.recv_timeout(QUIET).is_err(),
        "one press, one callback"
    );
}

#[test]
#[ignore = "needs the mock portal on a private bus: tools/ci/wayland-tests.sh"]
fn refused_bind_is_taken() {
    let bus = mock_bus("refuse");
    let (hk, owner, presses) = started_hotkey(CALL, REQUEST, BIND);
    assert_eq!(hk.bind(&hotkey("Ctrl+Alt+R")), Err(HotkeyTaken));
    assert!(
        sessions(&bus, &owner)
            .iter()
            .all(|(_, trigger)| trigger.is_empty()),
        "nothing is bound"
    );
    press(&bus);
    assert!(
        presses.recv_timeout(QUIET).is_err(),
        "a refused key never calls back"
    );
}

#[test]
#[ignore = "needs the mock portal on a private bus: tools/ci/wayland-tests.sh"]
fn bind_cancelled_mid_wait_times_out() {
    let _bus = mock_bus("never");
    let wait = Duration::from_millis(500);
    let (hk, _owner, _presses) = started_hotkey(CALL, REQUEST, wait);
    let started = Instant::now();
    assert_eq!(hk.bind(&hotkey("Ctrl+Alt+R")), Err(HotkeyTaken));
    let took = started.elapsed();
    assert!(
        took >= wait,
        "gave up after {took:?}, before the {wait:?} wait"
    );
    assert!(
        took < wait + Duration::from_secs(2),
        "gave up after {took:?}, long after the {wait:?} wait"
    );
}

#[test]
#[ignore = "needs the mock portal on a private bus: tools/ci/wayland-tests.sh"]
fn bind_call_without_reply_gives_up_at_the_call_deadline() {
    let bus = mock_bus("silent");
    assert!(
        PortalHotkey::with_timeouts(private_bus(), REQUEST, BIND).is_err(),
        "a bus without a call deadline is refused"
    );
    let call = Duration::from_secs(1);
    let (hk, _owner, presses) = started_hotkey(call, REQUEST, BIND);
    let started = Instant::now();
    assert_eq!(hk.bind(&hotkey("Ctrl+Alt+R")), Err(HotkeyTaken));
    let took = started.elapsed();
    assert!(
        took >= call,
        "gave up after {took:?}, before the {call:?} call deadline"
    );
    assert!(
        took < call + Duration::from_secs(2),
        "gave up after {took:?}, long after the {call:?} call deadline (the {BIND:?} bind wait?)"
    );

    // The portal replies again: the hotkey still works.
    bus.call_method(Some(PORTAL), PORTAL_PATH, Some(MOCK), "SetBind", &("ok",))
        .expect("set the mock back to ok");
    hk.unbind();
    assert_eq!(hk.bind(&hotkey("Ctrl+Alt+R")), Ok(()));
    press(&bus);
    presses
        .recv_timeout(PRESS)
        .expect("the press did not reach the callback within 2 s");
}

#[test]
#[ignore = "needs the mock portal on a private bus: tools/ci/wayland-tests.sh"]
fn rebind_closes_the_old_session() {
    let bus = mock_bus("ok");
    let (hk, owner, presses) = started_hotkey(CALL, REQUEST, BIND);
    assert_eq!(hk.bind(&hotkey("Ctrl+Alt+R")), Ok(()));
    let first = sessions(&bus, &owner);
    assert_eq!(first.len(), 1, "one session: {first:?}");

    hk.unbind();
    assert_eq!(hk.bind(&hotkey("Ctrl+Shift+PageUp")), Ok(()));
    let second = sessions(&bus, &owner);
    assert_eq!(second.len(), 1, "the old session is closed: {second:?}");
    assert_ne!(second[0].0, first[0].0, "a fresh session");
    assert_eq!(second[0].1, "CTRL+SHIFT+Page_Up");

    press(&bus);
    presses
        .recv_timeout(PRESS)
        .expect("the press did not reach the callback within 2 s");
    assert!(
        presses.recv_timeout(QUIET).is_err(),
        "one press, one callback"
    );
}

#[test]
#[ignore = "needs a private bus without a portal: tools/ci/wayland-tests.sh"]
fn no_portal_means_command_mode() {
    let bus = private_bus();
    assert_eq!(
        std::env::var("MOCK_PORTAL").ok().as_deref(),
        Some("none"),
        "this test needs a bus without a portal: tools/ci/wayland-tests.sh runs it with MOCK_PORTAL=none"
    );
    assert_eq!(portal_version(&bus), None);
    assert_eq!(CommandHotkey.mode(), HotkeyMode::Command);
}
