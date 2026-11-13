//! The tray on Linux: only where the desktop shows one.
//!
//! A tray icon here is a StatusNotifierItem, shown by a StatusNotifierHost (KDE Plasma, most
//! panels, GNOME only with an extension) and drawn through libappindicator. Without a host the
//! icon is invisible and a window hidden to it cannot be brought back; without the library the
//! tray crate panics, and with `panic = "abort"` the app would end without a word. So the app
//! asks for both before it builds a tray.

use zbus::blocking::Connection;
use zbus::zvariant::OwnedValue;

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";

/// The libraries the tray crate (`libappindicator-sys` 0.9.0) tries, in its order.
const LIBRARIES: [&str; 4] = [
    "libayatana-appindicator3.so.1",
    "libappindicator3.so.1",
    "libayatana-appindicator3.so",
    "libappindicator3.so",
];

/// Whether a StatusNotifierHost is registered with the watcher on `conn`. Any error is `false`.
pub fn host_registered(conn: &Connection) -> bool {
    conn.call_method(
        Some(WATCHER),
        WATCHER_PATH,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &(WATCHER, "IsStatusNotifierHostRegistered"),
    )
    .ok()
    .and_then(|reply| reply.body().deserialize::<OwnedValue>().ok())
    .and_then(|value| bool::try_from(value).ok())
    .unwrap_or(false)
}

/// Whether one of the tray crate's libraries loads.
#[allow(unsafe_code)]
fn library_loads() -> bool {
    LIBRARIES.iter().any(|name| {
        // SAFETY: loading a library runs its initialisers. These are the system's appindicator
        // libraries, which the tray crate loads by the same names in the same order as soon as
        // a tray is built; nothing in them is called here. The handle is kept (never unloaded),
        // as the tray crate keeps its own: unloading a GTK-linked library is not safe.
        match unsafe { libloading::Library::new(name) } {
            Ok(library) => {
                std::mem::forget(library);
                true
            }
            Err(_) => false,
        }
    })
}

/// Whether this desktop shows a tray: the session bus answers, a host is registered, and the
/// tray library loads.
pub fn available() -> bool {
    Connection::session().is_ok_and(|conn| host_registered(&conn)) && library_loads()
}
