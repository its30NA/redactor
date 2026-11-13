//! The X11 clipboard: our own client on its own connection, instead of a clipboard plugin.
//!
//! One connection, two kinds of threads. The worker is the only one that waits for events: it
//! counts every change of the `CLIPBOARD` owner (XFIXES) and hands the answers of a clipboard
//! operation in progress to the thread that asked. Calling threads send their requests on the
//! same connection, one operation at a time.
//!
//! X11 has no clipboard sequence number, so the count of owner changes is ours. The X server
//! reports events in the order things happened; a [`X11Clipboard::ping`] (an empty append to a
//! property of our own window) therefore comes back after every owner change the server saw
//! before it, and the count read at that moment is exact.

mod read;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use x11rb::connection::{Connection, RequestConnection as _};
use x11rb::protocol::xfixes::{self, ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, Property,
    PropertyNotifyEvent, SelectionNotifyEvent, Timestamp, Window, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use crate::clipscrub::{ClipError, ClipText, Clipboard};
use crate::platform::SystemClipboard;

/// How long one answer may take: a `SelectionNotify`, a piece of a large transfer, a ping.
const STEP_TIMEOUT: Duration = Duration::from_secs(1);
/// How long a whole read may take.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        CLIPBOARD,
        TARGETS,
        INCR,
        UTF8_STRING,
        TEXT_PLAIN_UTF8: b"text/plain;charset=utf-8",
        // Set to `secret` by KeePassXC, Bitwarden and others.
        PASSWORD_HINT: b"x-kde-passwordManagerHint",
        PING: b"_REDACTOR_PING",
        // Where a selection owner puts its answer on a read's window.
        TRANSFER: b"_REDACTOR_TRANSFER",
    }
}

/// Any X error is "the clipboard is busy or unavailable"; none of them carries text.
fn lost<E>(_: E) -> ClipError {
    ClipError
}

/// What the worker hands to the thread whose clipboard operation is in progress.
enum Reply {
    /// The answer to the `number`th ping: the owner changes counted up to it, the server time.
    Ping {
        number: u32,
        changes: u32,
        time: Timestamp,
    },
    Selection(SelectionNotifyEvent),
    Property(PropertyNotifyEvent),
}

/// What the worker and the calling threads share.
struct Shared {
    conn: RustConnection,
    root: Window,
    /// Our unmapped window: the target of pings.
    window: Window,
    atoms: Atoms,
    /// Owner changes of `CLIPBOARD` seen so far.
    changes: AtomicU32,
    /// The window of the read in progress (0: none); its events go to the reading thread.
    listening: AtomicU32,
    stop: AtomicBool,
}

/// The state of the one clipboard operation that may run at a time.
struct Op {
    replies: Receiver<Reply>,
    pings: u32,
}

/// The `CLIPBOARD` selection of one X display.
pub struct X11Clipboard {
    shared: Arc<Shared>,
    op: Mutex<Op>,
    worker: Option<JoinHandle<()>>,
}

impl X11Clipboard {
    /// Connects to `display` (`None`: `$DISPLAY`), creates one unmapped window and starts the
    /// worker. Fails when the server cannot be reached or has no XFIXES extension.
    pub fn connect(display: Option<&str>) -> Result<X11Clipboard, ClipError> {
        let (conn, screen) = RustConnection::connect(display).map_err(lost)?;
        let root = conn.setup().roots.get(screen).ok_or(ClipError)?.root;
        conn.extension_information(xfixes::X11_EXTENSION_NAME)
            .map_err(lost)?
            .ok_or(ClipError)?;
        // XFIXES answers nothing until the client has said which version it speaks.
        conn.xfixes_query_version(5, 0)
            .map_err(lost)?
            .reply()
            .map_err(lost)?;
        let atoms = Atoms::new(&conn).map_err(lost)?.reply().map_err(lost)?;

        let window = conn.generate_id().map_err(lost)?;
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(lost)?
        .check()
        .map_err(lost)?;
        // A new owner, an owner whose window is destroyed, an owner that exits.
        conn.xfixes_select_selection_input(
            window,
            atoms.CLIPBOARD,
            SelectionEventMask::SET_SELECTION_OWNER
                | SelectionEventMask::SELECTION_WINDOW_DESTROY
                | SelectionEventMask::SELECTION_CLIENT_CLOSE,
        )
        .map_err(lost)?
        .check()
        .map_err(lost)?;

        let shared = Arc::new(Shared {
            conn,
            root,
            window,
            atoms,
            changes: AtomicU32::new(0),
            listening: AtomicU32::new(x11rb::NONE),
            stop: AtomicBool::new(false),
        });
        let (tx, replies) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("x11-clipboard".into())
            .spawn({
                let shared = Arc::clone(&shared);
                move || worker(&shared, &tx)
            })
            .map_err(lost)?;
        Ok(X11Clipboard {
            shared,
            op: Mutex::new(Op { replies, pings: 0 }),
            worker: Some(worker),
        })
    }

    /// Starts a clipboard operation: waits for the one in progress, drops answers nobody took.
    fn begin(&self) -> MutexGuard<'_, Op> {
        let op = self.op.lock().unwrap_or_else(|e| e.into_inner());
        while op.replies.try_recv().is_ok() {}
        op
    }

    /// Asks the server "what happened up to now?": the number of owner changes it had seen when
    /// it handled this request, and its time. `None` when no answer came in time.
    fn ping(&self, op: &mut Op) -> Option<(u32, Timestamp)> {
        self.shared.touch().ok()?;
        op.pings = op.pings.wrapping_add(1);
        let deadline = Instant::now() + STEP_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match op.replies.recv_timeout(left).ok()? {
                Reply::Ping {
                    number,
                    changes,
                    time,
                } if number == op.pings => return Some((changes, time)),
                // The answer to an earlier ping that came too late, or a finished read's event.
                _ => {}
            }
        }
    }
}

impl Shared {
    /// An empty append to a property of our own window. It changes nothing, and the server
    /// answers with a `PropertyNotify` that carries its time.
    fn touch(&self) -> Result<(), ClipError> {
        self.conn
            .change_property(
                PropMode::APPEND,
                self.window,
                self.atoms.PING,
                AtomEnum::STRING,
                8,
                0,
                &[],
            )
            .map_err(lost)?;
        self.conn.flush().map_err(lost)
    }
}

/// The only thread that waits for events. X errors and events nobody asked for are dropped.
fn worker(shared: &Shared, tx: &Sender<Reply>) {
    let a = &shared.atoms;
    let mut pings = 0u32;
    loop {
        let Ok(event) = shared.conn.wait_for_event() else {
            return;
        };
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let reply = match event {
            Event::XfixesSelectionNotify(e) if e.selection == a.CLIPBOARD => {
                shared.changes.fetch_add(1, Ordering::SeqCst);
                continue;
            }
            Event::PropertyNotify(e)
                if e.window == shared.window
                    && e.atom == a.PING
                    && e.state == Property::NEW_VALUE =>
            {
                pings = pings.wrapping_add(1);
                Reply::Ping {
                    number: pings,
                    changes: shared.changes.load(Ordering::SeqCst),
                    time: e.time,
                }
            }
            Event::PropertyNotify(e) if e.window == shared.listening.load(Ordering::SeqCst) => {
                Reply::Property(e)
            }
            Event::SelectionNotify(e) if e.requestor == shared.listening.load(Ordering::SeqCst) => {
                Reply::Selection(e)
            }
            _ => continue,
        };
        // The receiver lives as long as the clipboard; without it nobody waits.
        let _ = tx.send(reply);
    }
}

impl Drop for X11Clipboard {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // Any event wakes the worker, which then sees the flag. If the connection is gone the
        // worker has ended already.
        let _ = self.shared.touch();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Clipboard for X11Clipboard {
    fn sequence(&self) -> u32 {
        let mut op = self.begin();
        match self.ping(&mut op) {
            Some((changes, _)) => changes,
            None => self.shared.changes.load(Ordering::SeqCst),
        }
    }

    fn read(&self) -> Result<ClipText, ClipError> {
        let mut op = self.begin();
        let (_, time) = self.ping(&mut op).ok_or(ClipError)?;
        read::read(&self.shared, &op, time)
    }

    fn write_if_unchanged(&self, _text: &str, _seq: u32) -> Result<bool, ClipError> {
        Err(ClipError)
    }

    /// X11 has no clipboard history of its own; a clipboard manager's history is out of reach.
    fn delete_from_history(&self, _raw: &str) -> bool {
        false
    }
}

impl SystemClipboard for X11Clipboard {
    fn write_text(&self, _text: &str) -> Result<(), ClipError> {
        Err(ClipError)
    }
}
