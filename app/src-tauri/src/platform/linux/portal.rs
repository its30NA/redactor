//! The hotkey through the desktop's GlobalShortcuts portal (`org.freedesktop.portal.Desktop`,
//! KDE 6, GNOME 48 and later): the key from Settings goes as a wish, the desktop binds it (it may
//! ask the user, pick another key or refuse) and sends `Activated` on each press.
//!
//! Own blocking `zbus` code, on a connection of its own ([`session_bus`]) whose method calls give
//! up after [`CALL_TIMEOUT`]: a portal that never replies cannot hold up a settings change for
//! good. Every portal call answers through a `Request` object whose `Response` signal comes
//! later; its path is known before the call (the sender's unique name and our token), so the
//! match rule for it is in place before the call is made. One thread reads the connection: it
//! hands each `Response` to the call waiting for it and each `Activated` on the current session
//! to a second thread, which runs the press callback. Signals carry no clipboard content and
//! nothing here logs.
//!
//! Where there is no portal, the app uses `CommandHotkey` instead (`portal_version` decides).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use tauri::AppHandle;
use zbus::blocking::connection::Builder;
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::Message;

use crate::hotkey::{Hotkey, Key};
use crate::platform::{GlobalHotkey, HotkeyMode, HotkeyTaken};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST: &str = "org.freedesktop.portal.Request";
const SESSION: &str = "org.freedesktop.portal.Session";
const REGISTRY: &str = "org.freedesktop.host.portal.Registry";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const APP_ID: &str = "io.github.its30na.redactor";
/// The one shortcut the app binds.
const SHORTCUT_ID: &str = "scrub-clipboard";
const DESCRIPTION: &str = "Clean the clipboard";
/// How long a method call on the bus may take to reply (the portal replies at once; its answer
/// comes later, as a `Response`).
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a portal call may take to answer.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a bind may take: a person may be reading GNOME's dialog.
const BIND_TIMEOUT: Duration = Duration::from_secs(120);

/// What a press does, without the app around it.
pub type Press = Arc<dyn Fn() + Send + Sync>;

/// The portal's `Response`: its code (0 done, 1 cancelled by the user, 2 other) and results.
type Response = (u32, HashMap<String, OwnedValue>);

/// A connection to the session bus whose method calls give up after [`CALL_TIMEOUT`].
pub fn session_bus() -> zbus::Result<Connection> {
    session_bus_with(CALL_TIMEOUT)
}

/// As [`session_bus`], with another call deadline: for tests only.
#[doc(hidden)]
pub fn session_bus_with(call_timeout: Duration) -> zbus::Result<Connection> {
    Builder::session()?.method_timeout(call_timeout).build()
}

/// The GlobalShortcuts version the session bus offers; `None` where there is no portal or it
/// has no GlobalShortcuts.
pub fn portal_version(bus: &Connection) -> Option<u32> {
    let reply = bus
        .call_method(
            Some(PORTAL),
            PORTAL_PATH,
            Some(PROPERTIES),
            "Get",
            &(SHORTCUTS, "version"),
        )
        .ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    u32::try_from(value).ok()
}

/// The hotkey as the portal's `preferred_trigger`: modifiers in the order `CTRL`, `ALT`,
/// `SHIFT`, `LOGO`, then the key's XKB keysym name, joined by `+` (`CTRL+ALT+r`).
pub fn preferred_trigger(hk: &Hotkey) -> String {
    let mut parts: Vec<String> = [
        (hk.ctrl, "CTRL"),
        (hk.alt, "ALT"),
        (hk.shift, "SHIFT"),
        (hk.meta, "LOGO"),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, name)| name.to_string())
    .collect();
    parts.push(keysym(hk.key));
    parts.join("+")
}

fn keysym(key: Key) -> String {
    match key {
        Key::Letter(c) => c.to_ascii_lowercase().to_string(),
        Key::Digit(d) => d.to_string(),
        Key::F(n) => format!("F{n}"),
        Key::Space => "space".into(),
        Key::Insert => "Insert".into(),
        Key::Delete => "Delete".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
        Key::PageUp => "Page_Up".into(),
        Key::PageDown => "Page_Down".into(),
        Key::Backspace => "BackSpace".into(),
        Key::Enter => "Return".into(),
        Key::Tab => "Tab".into(),
    }
}

/// The path of the `Request` a call with `token` gets: the sender's unique name without the
/// `:` and with `_` for `.`.
pub fn request_path(sender: &str, token: &str) -> String {
    format!("{PORTAL_PATH}/request/{}/{token}", sender_part(sender))
}

fn sender_part(sender: &str) -> String {
    sender.trim_start_matches(':').replace('.', "_")
}

/// A token for a request or a session, unique in this process.
fn next_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("redactor_{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// What the reading thread shares with the calls.
#[derive(Default)]
struct Shared {
    /// The calls waiting for their `Response`, by request path.
    waiting: Mutex<HashMap<String, mpsc::Sender<Response>>>,
    /// The session whose `Activated` counts.
    active: Mutex<Option<OwnedObjectPath>>,
    /// To the thread that runs the press callback: a slow press never holds up a `Response`.
    presses: OnceLock<mpsc::Sender<()>>,
}

/// A portal session and whether a bind has used it.
struct Session {
    path: OwnedObjectPath,
    used: bool,
}

/// The hotkey through the portal (`mode()` → `Portal`).
pub struct PortalHotkey {
    bus: Connection,
    sender: String,
    shared: Arc<Shared>,
    /// The current session. Held through each portal operation, so they run one at a time.
    session: Mutex<Option<Session>>,
    request_timeout: Duration,
    bind_timeout: Duration,
}

impl PortalHotkey {
    /// The portal hotkey on `bus` (from [`session_bus`]), with the shipped timeouts.
    pub fn new(bus: Connection) -> zbus::Result<PortalHotkey> {
        Self::with_timeouts(bus, REQUEST_TIMEOUT, BIND_TIMEOUT)
    }

    /// As [`PortalHotkey::new`], with shorter waits: for tests only. Both refuse a bus without a
    /// call deadline.
    #[doc(hidden)]
    pub fn with_timeouts(
        bus: Connection,
        request: Duration,
        bind: Duration,
    ) -> zbus::Result<PortalHotkey> {
        if bus.method_timeout().is_none() {
            return Err(zbus::Error::Failure("a bus without a call deadline".into()));
        }
        let sender = bus
            .unique_name()
            .ok_or_else(|| zbus::Error::Failure("not a bus connection".into()))?
            .to_string();
        let shared = Arc::new(Shared::default());
        // Before the match rules, so no signal that arrives after them is missed.
        let messages = MessageIterator::from(&bus);
        add_match(
            &bus,
            &format!(
                "type='signal',sender='{PORTAL}',path='{PORTAL_PATH}',\
                 interface='{SHORTCUTS}',member='Activated'"
            ),
        )?;
        {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("portal-hotkey".into())
                .spawn(move || {
                    for msg in messages.map_while(Result::ok) {
                        dispatch(&shared, &msg);
                    }
                })
                .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        }
        Ok(PortalHotkey {
            bus,
            sender,
            shared,
            session: Mutex::new(None),
            request_timeout: request,
            bind_timeout: bind,
        })
    }

    /// Hands over what a press does, tells the portal who we are and opens the first session.
    /// Called once, before the first `bind`. A failure here is retried by the next `bind`.
    /// Presses run one at a time, on a thread of their own.
    pub fn start(&self, on_press: Press) {
        let (tx, rx) = mpsc::channel::<()>();
        if self.shared.presses.set(tx).is_ok() {
            let _ = std::thread::Builder::new()
                .name("portal-press".into())
                .spawn(move || {
                    for () in rx {
                        on_press();
                    }
                });
        }
        // Older portals have no registry; it only names the app in the desktop's dialog.
        let _ = self.bus.call_method(
            Some(PORTAL),
            PORTAL_PATH,
            Some(REGISTRY),
            "Register",
            &(APP_ID, HashMap::<&str, Value>::new()),
        );
        let mut session = lock(&self.session);
        self.open_session(&mut session);
    }

    /// Asks the portal to bind `hk`. `HotkeyTaken` when the desktop refuses it, fails, does not
    /// reply within the call deadline or does not answer within the bind wait.
    pub fn bind(&self, hk: &Hotkey) -> Result<(), HotkeyTaken> {
        let mut session = lock(&self.session);
        let path = self.open_session(&mut session).ok_or(HotkeyTaken)?;
        if let Some(s) = session.as_mut() {
            s.used = true;
        }
        let shortcut: HashMap<&str, Value> = HashMap::from([
            ("description", Value::from(DESCRIPTION)),
            ("preferred_trigger", Value::from(preferred_trigger(hk))),
        ]);
        let response = self.request(self.bind_timeout, |token| {
            let options = HashMap::from([("handle_token", Value::from(token))]);
            self.bus.call_method(
                Some(PORTAL),
                PORTAL_PATH,
                Some(SHORTCUTS),
                "BindShortcuts",
                &(&path, vec![(SHORTCUT_ID, shortcut)], "", options),
            )
        });
        match response {
            Some((0, _)) => Ok(()),
            _ => Err(HotkeyTaken),
        }
    }

    /// Closes the session, so the bound key is free again, and opens a fresh one for the next
    /// bind.
    pub fn unbind(&self) {
        let mut session = lock(&self.session);
        self.close_session(&mut session);
        self.open_session(&mut session);
    }

    /// The session for the next bind: the current one if no bind used it yet, else a fresh
    /// one. `None` when the portal does not open one.
    fn open_session(&self, slot: &mut Option<Session>) -> Option<OwnedObjectPath> {
        match slot {
            Some(s) if !s.used => return Some(s.path.clone()),
            Some(_) => self.close_session(slot),
            None => {}
        }
        let session_token = next_token();
        let (code, results) = self.request(self.request_timeout, |token| {
            let options = HashMap::from([
                ("handle_token", Value::from(token)),
                ("session_handle_token", Value::from(session_token.as_str())),
            ]);
            self.bus.call_method(
                Some(PORTAL),
                PORTAL_PATH,
                Some(SHORTCUTS),
                "CreateSession",
                &(options,),
            )
        })?;
        if code != 0 {
            return None;
        }
        // A string by the portal's documentation; an object path is taken too.
        let path = match &**results.get("session_handle")? {
            Value::Str(s) => OwnedObjectPath::try_from(s.as_str()).ok()?,
            Value::ObjectPath(p) => OwnedObjectPath::from(p.clone()),
            _ => return None,
        };
        *lock(&self.shared.active) = Some(path.clone());
        *slot = Some(Session {
            path: path.clone(),
            used: false,
        });
        Some(path)
    }

    fn close_session(&self, slot: &mut Option<Session>) {
        let Some(s) = slot.take() else {
            return;
        };
        // From here on a press for it no longer counts.
        *lock(&self.shared.active) = None;
        let _ = self
            .bus
            .call_method(Some(PORTAL), &s.path, Some(SESSION), "Close", &());
    }

    /// Makes one portal call and waits for its `Response`. The match rule for the predicted
    /// request path is in place before the call. `None` when the call fails (also when it gets
    /// no reply within the call deadline), names another path, or no answer comes within
    /// `timeout` (the request is then closed).
    fn request(
        &self,
        timeout: Duration,
        call: impl FnOnce(&str) -> zbus::Result<Message>,
    ) -> Option<Response> {
        let token = next_token();
        let path = request_path(&self.sender, &token);
        let rule = format!(
            "type='signal',sender='{PORTAL}',path='{path}',interface='{REQUEST}',member='Response'"
        );
        let (tx, rx) = mpsc::channel();
        lock(&self.shared.waiting).insert(path.clone(), tx);
        let answer = (|| {
            add_match(&self.bus, &rule).ok()?;
            let handle: OwnedObjectPath = call(&token).ok()?.body().deserialize().ok()?;
            if handle.as_str() != path {
                return None;
            }
            let answer = rx.recv_timeout(timeout).ok();
            if answer.is_none() {
                // Ends the desktop's dialog, if one is still open.
                let _ =
                    self.bus
                        .call_method(Some(PORTAL), path.as_str(), Some(REQUEST), "Close", &());
            }
            answer
        })();
        lock(&self.shared.waiting).remove(&path);
        let _ = remove_match(&self.bus, &rule);
        answer
    }
}

impl GlobalHotkey for PortalHotkey {
    fn register(&self, _app: &AppHandle, hk: &Hotkey) -> Result<(), HotkeyTaken> {
        self.bind(hk)
    }

    fn unregister(&self, _app: &AppHandle, _hk: &Hotkey) {
        self.unbind();
    }

    fn mode(&self) -> HotkeyMode {
        HotkeyMode::Portal
    }

    fn listen(&self, app: &AppHandle, on_press: Arc<dyn Fn(&AppHandle) + Send + Sync>) {
        let app = app.clone();
        self.start(Arc::new(move || on_press(&app)));
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Asks the bus to route the signals matching `rule` to this connection.
fn add_match(bus: &Connection, rule: &str) -> zbus::Result<()> {
    bus_call(bus, "AddMatch", rule)
}

fn remove_match(bus: &Connection, rule: &str) -> zbus::Result<()> {
    bus_call(bus, "RemoveMatch", rule)
}

fn bus_call(bus: &Connection, method: &str, rule: &str) -> zbus::Result<()> {
    bus.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        method,
        &(rule,),
    )
    .map(drop)
}

/// One message read from the connection: a `Response` goes to the call waiting for it, an
/// `Activated` of our shortcut on the current session goes to the press thread.
fn dispatch(shared: &Shared, msg: &Message) {
    if msg.message_type() != Type::Signal {
        return;
    }
    let header = msg.header();
    let (Some(iface), Some(member), Some(path)) =
        (header.interface(), header.member(), header.path())
    else {
        return;
    };
    match (iface.as_str(), member.as_str()) {
        (REQUEST, "Response") => {
            let Some(waiter) = lock(&shared.waiting).remove(path.as_str()) else {
                return;
            };
            if let Ok(response) = msg.body().deserialize::<Response>() {
                let _ = waiter.send(response);
            }
        }
        (SHORTCUTS, "Activated") if path.as_str() == PORTAL_PATH => {
            type Activated<'a> = (ObjectPath<'a>, String, u64, HashMap<String, OwnedValue>);
            let body = msg.body();
            let Ok((session, id, _, _)) = body.deserialize::<Activated>() else {
                return;
            };
            let current = lock(&shared.active)
                .as_ref()
                .is_some_and(|a| a.as_str() == session.as_str());
            if id == SHORTCUT_ID && current {
                if let Some(presses) = shared.presses.get() {
                    let _ = presses.send(());
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hk(ctrl: bool, alt: bool, shift: bool, meta: bool, key: Key) -> Hotkey {
        Hotkey {
            ctrl,
            shift,
            alt,
            meta,
            key,
        }
    }

    #[test]
    fn ctrl_shift_s_is_ctrl_shift_s() {
        let k = hk(true, false, true, false, Key::Letter('S'));
        assert_eq!(preferred_trigger(&k), "CTRL+SHIFT+s");
    }

    #[test]
    fn all_modifiers_in_order() {
        let k = hk(true, true, true, true, Key::F(5));
        assert_eq!(preferred_trigger(&k), "CTRL+ALT+SHIFT+LOGO+F5");
    }

    #[test]
    fn page_keys_use_xkb_names() {
        let k = hk(true, false, false, false, Key::PageUp);
        assert_eq!(preferred_trigger(&k), "CTRL+Page_Up");
        let k = hk(true, false, false, false, Key::PageDown);
        assert_eq!(preferred_trigger(&k), "CTRL+Page_Down");
    }

    #[test]
    fn request_path_from_sender() {
        assert_eq!(
            request_path(":1.42", "redactor_3"),
            "/org/freedesktop/portal/desktop/request/1_42/redactor_3"
        );
    }
}
