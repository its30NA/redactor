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
//!
//! Writing means becoming the owner of the selection and answering whoever pastes (`serve`).
//! The compare-and-set of [`Clipboard::write_if_unchanged`] holds a server grab from the ping
//! to the ownership change: while it lasts the server handles no other client, so no copy can
//! land between the comparison and the write.

mod read;
mod serve;

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
use x11rb::wrapper::ConnectionExt as _;
use zeroize::Zeroizing;

use crate::clipscrub::{ClipError, ClipText, Clipboard};
use crate::platform::SystemClipboard;
use serve::Serving;

/// How long one answer may take: a `SelectionNotify`, a piece of a large transfer, a ping.
const STEP_TIMEOUT: Duration = Duration::from_secs(1);
/// How long a whole read may take.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the clipboard manager gets to take our text over when we exit.
const HANDOVER_TIMEOUT: Duration = Duration::from_secs(1);

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
        TIMESTAMP,
        // The desktop's clipboard manager, and the request "save what I hold, I am leaving".
        CLIPBOARD_MANAGER,
        SAVE_TARGETS,
        SAVE: b"_REDACTOR_SAVE",
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
    /// The text we serve while we own the selection. The lock is also held while ownership is
    /// taken or checked, so the text and "we are the owner" always belong together.
    text: Mutex<Option<Arc<Zeroizing<String>>>>,
    /// The server time at which we took the selection.
    owned_at: AtomicU32,
    /// The largest piece of text sent in one request.
    chunk: usize,
    /// The clipboard manager has been asked to keep our text (on exit).
    handed_over: AtomicBool,
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

        // Without the BIG-REQUESTS extension a request holds a little less than 256 KiB.
        let chunk = serve::INCR_CHUNK.min(conn.maximum_request_bytes().saturating_sub(1024));
        let shared = Arc::new(Shared {
            conn,
            root,
            window,
            atoms,
            changes: AtomicU32::new(0),
            listening: AtomicU32::new(x11rb::NONE),
            text: Mutex::new(None),
            owned_at: AtomicU32::new(x11rb::CURRENT_TIME),
            chunk,
            handed_over: AtomicBool::new(false),
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

impl X11Clipboard {
    /// Makes us the owner of the clipboard, serving `text`. `time` is a server time from a ping.
    fn own(&self, text: &str, time: Timestamp) -> Result<(), ClipError> {
        let s = &self.shared;
        let mut held = s.text.lock().unwrap_or_else(|e| e.into_inner());
        // In place before we own the selection: a request can come the moment we do.
        *held = Some(Arc::new(Zeroizing::new(text.to_owned())));
        s.owned_at.store(time, Ordering::SeqCst);
        // The server ignores a request with a time before the last change, so ask who owns it.
        let owner = s
            .conn
            .set_selection_owner(s.window, s.atoms.CLIPBOARD, time)
            .map_err(lost)
            .and_then(|_| s.owner());
        if owner != Ok(s.window) {
            *held = None;
            return Err(ClipError);
        }
        Ok(())
    }
}

/// The server handles no other client while this lives. Every way out ends it.
struct ServerGrab<'a>(&'a RustConnection);

impl<'a> ServerGrab<'a> {
    fn new(conn: &'a RustConnection) -> Result<Self, ClipError> {
        conn.grab_server().map_err(lost)?;
        Ok(ServerGrab(conn))
    }
}

impl Drop for ServerGrab<'_> {
    fn drop(&mut self) {
        let _ = self.0.ungrab_server();
        let _ = self.0.flush();
    }
}

impl Shared {
    /// The window that owns the clipboard now (0: nobody).
    fn owner(&self) -> Result<Window, ClipError> {
        Ok(self
            .conn
            .get_selection_owner(self.atoms.CLIPBOARD)
            .map_err(lost)?
            .reply()
            .map_err(lost)?
            .owner)
    }

    /// The text we serve, if we own the clipboard.
    fn owned_text(&self) -> Option<Arc<Zeroizing<String>>> {
        self.text.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The server said we lost the clipboard. A write of ours may have taken it back since, so
    /// the server is asked, under the same lock a write holds while it takes ownership.
    fn forget_if_lost(&self) {
        let mut held = self.text.lock().unwrap_or_else(|e| e.into_inner());
        if self.owner() != Ok(self.window) {
            *held = None;
        }
    }

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
    let mut serving = Serving::default();
    loop {
        let Ok(event) = shared.conn.wait_for_event() else {
            return;
        };
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        serving.expire(shared);
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
            // To a read's window, or to our own (the clipboard manager's answer on exit).
            Event::SelectionNotify(e)
                if e.requestor == shared.listening.load(Ordering::SeqCst)
                    || e.requestor == shared.window =>
            {
                Reply::Selection(e)
            }
            Event::SelectionRequest(e) => {
                serving.request(shared, &e);
                continue;
            }
            Event::SelectionClear(e) if e.selection == a.CLIPBOARD => {
                shared.forget_if_lost();
                continue;
            }
            Event::PropertyNotify(e) if e.state == Property::DELETE => {
                serving.deleted(shared, e.window, e.atom);
                continue;
            }
            Event::DestroyNotify(e) => {
                serving.gone(e.window);
                continue;
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

    fn write_if_unchanged(&self, text: &str, seq: u32) -> Result<bool, ClipError> {
        let mut op = self.begin();
        let _grab = ServerGrab::new(&self.shared.conn)?;
        // Under the grab no other client can change the owner: what the ping counts is still
        // true when the selection is taken.
        let (changes, time) = self.ping(&mut op).ok_or(ClipError)?;
        if changes != seq {
            return Ok(false);
        }
        self.own(text, time)?;
        Ok(true)
    }

    /// X11 has no clipboard history of its own; a clipboard manager's history is out of reach.
    fn delete_from_history(&self, _raw: &str) -> bool {
        false
    }
}

impl SystemClipboard for X11Clipboard {
    fn write_text(&self, text: &str) -> Result<(), ClipError> {
        let mut op = self.begin();
        let (_, time) = self.ping(&mut op).ok_or(ClipError)?;
        self.own(text, time)
    }

    /// On X11 the clipboard is empty once its owner exits, unless a clipboard manager took the
    /// text over (GNOME, KDE and Xfce run one). If we own the clipboard and there is a manager,
    /// it is asked to save our text, and we keep serving until it says it is done or
    /// [`HANDOVER_TIMEOUT`] has passed. Without a manager this returns at once.
    fn keep_after_exit(&self) {
        let mut op = self.begin();
        let s = &self.shared;
        let a = &s.atoms;
        // Asked once: the shell calls this on every way out, and a manager that did not
        // answer the first time is not waited for again.
        if s.handed_over.swap(true, Ordering::SeqCst) {
            return;
        }
        let manager = s
            .conn
            .get_selection_owner(a.CLIPBOARD_MANAGER)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map_or(x11rb::NONE, |reply| reply.owner);
        if manager == x11rb::NONE || s.owned_text().is_none() || s.owner() != Ok(s.window) {
            return;
        }
        let Some((_, time)) = self.ping(&mut op) else {
            return;
        };
        // The targets worth saving, then the request itself.
        let asked = s
            .conn
            .change_property32(
                PropMode::REPLACE,
                s.window,
                a.SAVE,
                AtomEnum::ATOM,
                &[a.UTF8_STRING, a.TEXT_PLAIN_UTF8],
            )
            .and_then(|_| {
                s.conn.convert_selection(
                    s.window,
                    a.CLIPBOARD_MANAGER,
                    a.SAVE_TARGETS,
                    a.SAVE,
                    time,
                )
            })
            .and_then(|_| s.conn.flush());
        if asked.is_err() {
            return;
        }
        // The worker answers the manager's requests for the text meanwhile.
        let deadline = Instant::now() + HANDOVER_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match op.replies.recv_timeout(left) {
                Ok(Reply::Selection(e))
                    if e.requestor == s.window && e.selection == a.CLIPBOARD_MANAGER =>
                {
                    return;
                }
                Ok(_) => {}
                Err(_) => return,
            }
        }
    }
}
