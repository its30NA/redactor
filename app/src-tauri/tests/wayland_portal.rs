//! The hotkey through the desktop's GlobalShortcuts portal, against `tools/ci/mock_portal.py`:
//! a bind that the desktop accepts, refuses or never answers, a bind call that never returns, a
//! press, a rebind, a session the desktop closes, a portal that restarts, signals another program
//! forges, and a bus with no portal at all.
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

use std::collections::HashMap;
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use redactor_app::hotkey::Hotkey;
use redactor_app::os::Os;
use redactor_app::platform::linux::command_hotkey::CommandHotkey;
use redactor_app::platform::linux::portal::{
    portal_version, request_path, session_bus_with, PortalHotkey,
};
use redactor_app::platform::{GlobalHotkey, HotkeyMode, HotkeyTaken};
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, Value};

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

/// A portal hotkey on its own connection (method calls give up after `call`), started.
struct Started {
    hotkey: PortalHotkey,
    /// Its unique bus name, which names its sessions at the mock.
    owner: String,
    /// One message per press.
    presses: mpsc::Receiver<()>,
    /// One message per time the hotkey is told it was lost.
    losses: mpsc::Receiver<()>,
}

fn started_hotkey(call: Duration, request: Duration, bind: Duration) -> Started {
    assert_private_bus();
    let conn = session_bus_with(call).expect("connect to the private session bus");
    let owner = conn.unique_name().expect("a unique name").to_string();
    let hotkey = PortalHotkey::with_timeouts(conn, request, bind).expect("the portal hotkey");
    let (press_tx, presses) = mpsc::channel();
    let (lost_tx, losses) = mpsc::channel();
    hotkey.start(
        Arc::new(move || {
            let _ = press_tx.send(());
        }),
        Arc::new(move || {
            let _ = lost_tx.send(());
        }),
    );
    Started {
        hotkey,
        owner,
        presses,
        losses,
    }
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

/// The desktop closes the owner's sessions.
fn close_sessions(bus: &Connection, owner: &str) {
    bus.call_method(
        Some(PORTAL),
        PORTAL_PATH,
        Some(MOCK),
        "CloseSessions",
        &(owner,),
    )
    .expect("close the sessions at the mock");
}

/// The portal drops its name and every session and takes the name again.
fn restart(bus: &Connection) {
    bus.call_method(Some(PORTAL), PORTAL_PATH, Some(MOCK), "Restart", &())
        .expect("restart the mock");
}

/// One press reaches the callback, and only one.
fn expect_one_press(presses: &mpsc::Receiver<()>) {
    presses
        .recv_timeout(PRESS)
        .expect("the press did not reach the callback within 2 s");
    assert!(
        presses.recv_timeout(QUIET).is_err(),
        "one press, one callback"
    );
}

/// The loss is told once, and only once.
fn expect_one_loss(losses: &mpsc::Receiver<()>, after: &str) {
    losses
        .recv_timeout(PRESS)
        .unwrap_or_else(|_| panic!("{after}: the loss was not told within 2 s"));
    assert!(
        losses.recv_timeout(QUIET).is_err(),
        "{after}: one loss, one notice"
    );
}

fn hotkey(s: &str) -> Hotkey {
    Hotkey::parse(s, Os::Linux).unwrap()
}

#[test]
#[ignore = "needs the mock portal on a private bus: tools/ci/wayland-tests.sh"]
fn bind_then_press_calls_back() {
    let bus = mock_bus("ok");
    assert_eq!(portal_version(&bus), Some(1));
    let Started {
        hotkey: hk,
        owner,
        presses,
        ..
    } = started_hotkey(CALL, REQUEST, BIND);
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
    let Started {
        hotkey: hk,
        owner,
        presses,
        ..
    } = started_hotkey(CALL, REQUEST, BIND);
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
    let hk = started_hotkey(CALL, REQUEST, wait).hotkey;
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
    let Started {
        hotkey: hk,
        presses,
        ..
    } = started_hotkey(call, REQUEST, BIND);
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
    let Started {
        hotkey: hk,
        owner,
        presses,
        ..
    } = started_hotkey(CALL, REQUEST, BIND);
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
#[ignore = "needs the mock portal on a private bus: tools/ci/wayland-tests.sh"]
fn a_closed_session_is_told_once_and_presses_stop() {
    let bus = mock_bus("ok");
    let s = started_hotkey(CALL, REQUEST, BIND);
    assert_eq!(s.hotkey.bind(&hotkey("Ctrl+Alt+R")), Ok(()));
    press(&bus);
    expect_one_press(&s.presses);

    close_sessions(&bus, &s.owner);
    expect_one_loss(&s.losses, "the desktop closed the session");
    press(&bus);
    assert!(
        s.presses.recv_timeout(QUIET).is_err(),
        "no press after the session closed"
    );
    // The same loss, seen again: not told a second time.
    restart(&bus);
    assert!(
        s.losses.recv_timeout(QUIET).is_err(),
        "a loss already told is not told again"
    );
}

#[test]
#[ignore = "needs the mock portal on a private bus: tools/ci/wayland-tests.sh"]
fn a_portal_restart_is_told_once() {
    let bus = mock_bus("ok");
    let s = started_hotkey(CALL, REQUEST, BIND);
    assert_eq!(s.hotkey.bind(&hotkey("Ctrl+Alt+R")), Ok(()));
    restart(&bus);
    expect_one_loss(&s.losses, "the portal restarted");
}

#[test]
#[ignore = "needs the mock portal on a private bus: tools/ci/wayland-tests.sh"]
fn nothing_bound_is_never_told_as_lost() {
    let bus = mock_bus("ok");
    let s = started_hotkey(CALL, REQUEST, BIND);
    // The session the start opened, never bound.
    close_sessions(&bus, &s.owner);
    restart(&bus);
    assert!(
        s.losses.recv_timeout(QUIET).is_err(),
        "no key was bound, so none was lost"
    );
    // Unbound by the app: not a loss either.
    assert_eq!(s.hotkey.bind(&hotkey("Ctrl+Alt+R")), Ok(()));
    s.hotkey.unbind();
    restart(&bus);
    assert!(
        s.losses.recv_timeout(QUIET).is_err(),
        "a key the app unbound is not lost"
    );
}

#[test]
#[ignore = "needs the mock portal on a private bus: tools/ci/wayland-tests.sh"]
fn binding_again_after_a_loss_works() {
    let bus = mock_bus("ok");
    let s = started_hotkey(CALL, REQUEST, BIND);
    assert_eq!(s.hotkey.bind(&hotkey("Ctrl+Alt+R")), Ok(()));
    close_sessions(&bus, &s.owner);
    expect_one_loss(&s.losses, "the desktop closed the session");

    // Settings sets the key again: a fresh session, and presses arrive.
    assert_eq!(s.hotkey.bind(&hotkey("Ctrl+Alt+R")), Ok(()));
    let open = sessions(&bus, &s.owner);
    assert_eq!(open.len(), 1, "one fresh session: {open:?}");
    assert_eq!(open[0].1, "CTRL+ALT+r");
    press(&bus);
    expect_one_press(&s.presses);

    // A second loss is told again, and the key binds again after it too.
    restart(&bus);
    expect_one_loss(&s.losses, "the portal restarted");
    assert_eq!(s.hotkey.bind(&hotkey("Ctrl+Alt+R")), Ok(()));
    press(&bus);
    expect_one_press(&s.presses);
}

/// Another program on the same bus sends the portal's signals straight to the hotkey's unique
/// name (match rules filter only broadcasts): a press, a closed session, a portal that left, and
/// the answer to a bind. None of them counts.
#[test]
#[ignore = "needs the mock portal on a private bus: tools/ci/wayland-tests.sh"]
fn forged_portal_signals_are_dropped() {
    let bus = mock_bus("ok");
    let s = started_hotkey(CALL, REQUEST, Duration::from_secs(1));
    assert_eq!(s.hotkey.bind(&hotkey("Ctrl+Alt+R")), Ok(()));
    let open = sessions(&bus, &s.owner);
    assert_eq!(open.len(), 1, "one session: {open:?}");
    let session = open[0].0.clone();
    let portal_owner: String = bus
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetNameOwner",
            &(PORTAL,),
        )
        .expect("the mock owns the portal name")
        .body()
        .deserialize()
        .expect("a unique name");

    let forger = Connection::session().expect("a second connection on the private bus");
    let to = Some(s.owner.as_str());
    let no_options = HashMap::<String, Value>::new();
    forger
        .emit_signal(
            to,
            PORTAL_PATH,
            "org.freedesktop.portal.GlobalShortcuts",
            "Activated",
            &(&session, SHORTCUT, 0u64, &no_options),
        )
        .expect("send a forged Activated");
    assert!(
        s.presses.recv_timeout(QUIET).is_err(),
        "a forged Activated scrubbed"
    );
    forger
        .emit_signal(
            to,
            session.as_str(),
            "org.freedesktop.portal.Session",
            "Closed",
            &(&no_options,),
        )
        .expect("send a forged Closed");
    forger
        .emit_signal(
            to,
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameOwnerChanged",
            &(PORTAL, portal_owner.as_str(), ""),
        )
        .expect("send a forged NameOwnerChanged");
    assert!(
        s.losses.recv_timeout(QUIET).is_err(),
        "a forged Closed or NameOwnerChanged was told as a loss"
    );
    press(&bus);
    expect_one_press(&s.presses);

    // A bind the desktop never answers, and a forged answer for every request path a token
    // could name.
    bus.call_method(
        Some(PORTAL),
        PORTAL_PATH,
        Some(MOCK),
        "SetBind",
        &("never",),
    )
    .expect("set the mock to never");
    s.hotkey.unbind();
    let (done_tx, done) = mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let _ = done_tx.send(s.hotkey.bind(&hotkey("Ctrl+Alt+R")));
        });
        let accepted = HashMap::<String, Value>::new();
        let deadline = Instant::now() + BIND + Duration::from_secs(5);
        let result = loop {
            for n in 0..500 {
                let path = request_path(&s.owner, &format!("redactor_{n}"));
                let _ = forger.emit_signal(
                    to,
                    path.as_str(),
                    "org.freedesktop.portal.Request",
                    "Response",
                    &(0u32, &accepted),
                );
            }
            match done.recv_timeout(Duration::from_millis(20)) {
                Ok(result) => break result,
                Err(_) => assert!(Instant::now() < deadline, "the bind never ended"),
            }
        };
        assert_eq!(result, Err(HotkeyTaken), "a forged Response bound the key");
    });
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
