//! The clipboard through data-control: our own client on its own connection.
//!
//! One connection, two kinds of threads, as on X11. The worker is the only one that dispatches
//! events: it counts every `selection` event (another app's copy, a cleared clipboard, our own
//! write's echo), keeps the offer of the current selection with its mime types, and answers
//! pastes from our source. Calling threads send their requests on the same connection, one
//! clipboard operation at a time.
//!
//! A `wl_display.sync` sent by a calling thread is answered on the worker's queue after every
//! event the compositor sent before it, so the count read when the answer comes is exact, and
//! the current offer is the one at that moment.
//!
//! Writing means offering a source of ours and answering whoever pastes. There is no server
//! grab on Wayland: [`Clipboard::write_if_unchanged`] makes its source, then the worker compares
//! the count at a sync's answer and sets the selection right there. A copy that reaches the
//! compositor between that answer and our `set_selection` (one wake-up of the worker) is
//! overwritten: a Known limit, bounded by `tests/wayland_clipboard.rs`.

use std::io::pipe;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use zeroize::Zeroizing;

use super::pipe::{read_bounded, write_bounded};
use super::protocol::{Device, Events, Manager, Offer, Source, SourceData};
use super::{hint_value_is_secret, lost, HINT_BYTES, PASSWORD_HINT, TEXT_MIMES};
use crate::bridge::MAX_INPUT_BYTES;
use crate::clipscrub::{text_from_utf8, ClipError, ClipText, Clipboard};
use crate::platform::SystemClipboard;

/// How long the compositor gets to answer a round trip (a sync, the echo of our write).
const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a sender may take between two pieces of a read.
const STEP_TIMEOUT: Duration = Duration::from_secs(1);
/// How long a whole read may take.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the password-manager hint may take.
const HINT_TIMEOUT: Duration = Duration::from_secs(1);
/// How long a paste of our text may take before we stop writing.
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// What the worker and the calling threads share.
struct Shared {
    conn: Connection,
    qh: QueueHandle<Worker>,
    manager: Manager,
    device: Device,
    /// Selection events seen so far.
    changes: AtomicU32,
    /// The offer of the current selection; `None`: the clipboard is empty. Held while a read
    /// asks the offer for its data, so the worker does not destroy it in between.
    current: Mutex<Option<Offer>>,
    /// Our source while it is the selection: set by the worker as it asks for the selection,
    /// cleared when the compositor says another copy replaced it.
    ours: Mutex<Option<Source>>,
    /// The device ended or the connection broke: every operation fails from now on.
    gone: AtomicBool,
    stop: AtomicBool,
}

/// The state of the worker thread, the only one that dispatches events.
pub(super) struct Worker {
    shared: Arc<Shared>,
}

/// The Wayland clipboard of one seat, through `ext-data-control-v1` or
/// `zwlr-data-control-unstable-v1`.
pub struct DataControlClipboard {
    shared: Arc<Shared>,
    /// One clipboard operation at a time.
    op: Mutex<()>,
    worker: Option<JoinHandle<()>>,
}

impl DataControlClipboard {
    /// Connects to `$WAYLAND_DISPLAY`, binds data-control and the first seat, and starts the
    /// worker. Fails when the compositor cannot be reached or offers no data-control or no seat.
    pub fn connect() -> Result<DataControlClipboard, ClipError> {
        let conn = Connection::connect_to_env().map_err(lost)?;
        let (globals, queue) = registry_queue_init::<Worker>(&conn).map_err(lost)?;
        let qh = queue.handle();
        let manager = Manager::bind(&globals, &qh).ok_or(ClipError)?;
        let seat: WlSeat = globals.bind(&qh, 1..=1, ()).map_err(lost)?;
        let device = manager.get_device(&seat, &qh);
        let shared = Arc::new(Shared {
            conn,
            qh,
            manager,
            device,
            changes: AtomicU32::new(0),
            current: Mutex::new(None),
            ours: Mutex::new(None),
            gone: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        });
        let worker = std::thread::Builder::new()
            .name("wayland-clipboard".into())
            .spawn({
                let shared = Arc::clone(&shared);
                move || run(queue, shared)
            })
            .map_err(lost)?;
        let clipboard = DataControlClipboard {
            shared,
            op: Mutex::new(()),
            worker: Some(worker),
        };
        // The compositor names the current selection right after the device is made.
        clipboard.shared.sync()?;
        Ok(clipboard)
    }

    fn begin(&self) -> MutexGuard<'_, ()> {
        self.op.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Offers `text` as the selection, if the count of selection events is still `seq` (`None`:
    /// whatever it is), and waits for the compositor's echo. False: the clipboard changed,
    /// nothing was written.
    ///
    /// The source is made first; the comparison and the `set_selection` happen on the worker,
    /// at the sync's answer, so the gap in which another copy can slip in is one wake-up of the
    /// worker, not a hand-over to this thread and back.
    fn set(&self, text: &str, seq: Option<u32>) -> Result<bool, ClipError> {
        let s = &self.shared;
        if s.gone.load(Ordering::SeqCst) {
            return Err(ClipError);
        }
        let data = SourceData {
            text: Arc::new(Zeroizing::new(text.to_owned())),
        };
        let source = s.manager.create_source(&s.qh, data);
        for mime in TEXT_MIMES {
            source.offer(mime);
        }
        let (tx, rx) = mpsc::channel();
        let answer = Answer::SetIfUnchanged {
            seq,
            source: Mutex::new(Some(source.clone())),
            reply: tx,
        };
        let callback = s.conn.display().sync(&s.qh, answer);
        let answered = s
            .conn
            .flush()
            .ok()
            .and_then(|()| rx.recv_timeout(ANSWER_TIMEOUT).ok());
        let count = match answered {
            Some(count) => count,
            None => {
                // Taken back, unless the worker has it already: then its answer is on the way.
                // (A sync that could not be sent now may still go out with a later request.)
                let pending = callback.data::<Answer>().and_then(Answer::take_source);
                if let Some(source) = pending {
                    source.destroy();
                    let _ = s.conn.flush();
                    return Err(ClipError);
                }
                rx.recv_timeout(ANSWER_TIMEOUT).map_err(lost)?
            }
        };
        if seq.is_some_and(|seq| seq != count) {
            return Ok(false);
        }
        // The compositor sends the echo while it handles `set_selection`, so it is counted once
        // the next sync is answered.
        if s.sync().is_ok_and(|now| now != count) {
            return Ok(true);
        }
        // No echo: whether or not the selection is ours, a read now asks the compositor.
        let mut ours = lock(&s.ours);
        if ours.as_ref() == Some(&source) {
            *ours = None;
        }
        Err(ClipError)
    }

    /// Reads the current selection; the caller has synced.
    fn read_current(&self) -> Result<ClipText, ClipError> {
        let s = &self.shared;
        if let Some(text) = lock(&s.ours).as_ref().and_then(Source::text) {
            return Ok(own_text(&text));
        }
        let Some(offer) = lock(&s.current).clone() else {
            return Ok(ClipText::Empty);
        };
        let mimes = offer.mimes();
        if mimes.iter().any(|m| m == PASSWORD_HINT)
            && hint_is_secret(s.receive(&offer, PASSWORD_HINT))
        {
            return Ok(ClipText::Protected);
        }
        let Some(mime) = TEXT_MIMES
            .into_iter()
            .find(|t| mimes.iter().any(|m| m == t))
        else {
            // An image, files: no text.
            return Ok(ClipText::Empty);
        };
        let reader = s.receive(&offer, mime)?;
        Ok(
            match read_bounded(reader, MAX_INPUT_BYTES, READ_TIMEOUT, STEP_TIMEOUT)? {
                Some(bytes) => text_from_utf8(bytes),
                None => ClipText::TooLarge,
            },
        )
    }
}

impl Shared {
    /// Asks the compositor "what happened up to now?": the number of selection events it had
    /// sent us when it handled this request. An error when no answer came in time.
    fn sync(&self) -> Result<u32, ClipError> {
        if self.gone.load(Ordering::SeqCst) {
            return Err(ClipError);
        }
        let (tx, rx) = mpsc::channel();
        self.conn.display().sync(&self.qh, Answer::Count(tx));
        self.conn.flush().map_err(lost)?;
        rx.recv_timeout(ANSWER_TIMEOUT).map_err(lost)
    }

    /// Asks the sender of `offer` to write `mime` into a new pipe; returns its read end. Fails
    /// when `offer` is no longer the current selection: a newer copy came since the read
    /// began (and may be a password manager's), so its data is not what was asked for.
    fn receive(&self, offer: &Offer, mime: &str) -> Result<std::io::PipeReader, ClipError> {
        let (reader, writer) = pipe().map_err(lost)?;
        {
            // Held until the request is sent, so the worker does not destroy the offer first.
            let current = lock(&self.current);
            if current.as_ref() != Some(offer) {
                return Err(ClipError);
            }
            offer.receive(mime, writer.as_fd());
        }
        self.conn.flush().map_err(lost)?;
        // The request carries its own copy of the write end; ours must close, or the read would
        // never see the end.
        drop(writer);
        Ok(reader)
    }
}

/// True when the password-manager hint says `secret`, and also when it cannot be read in time
/// or is longer than a short word: in doubt the copy is left alone.
fn hint_is_secret(hint: Result<std::io::PipeReader, ClipError>) -> bool {
    let Ok(reader) = hint else {
        return true;
    };
    let Ok(Some(value)) = read_bounded(reader, HINT_BYTES, HINT_TIMEOUT, HINT_TIMEOUT) else {
        return true;
    };
    hint_value_is_secret(&value)
}

/// Our own text, read as another app would get it: up to the first NUL, within the limit.
fn own_text(text: &str) -> ClipText {
    let end = memchr::memchr(0, text.as_bytes()).unwrap_or(text.len());
    if end == 0 {
        ClipText::Empty
    } else if end > MAX_INPUT_BYTES {
        ClipText::TooLarge
    } else {
        ClipText::Text(Zeroizing::new(text[..end].to_owned()))
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The worker thread: dispatches events until the clipboard is dropped or the connection
/// breaks.
fn run(mut queue: EventQueue<Worker>, shared: Arc<Shared>) {
    let mut worker = Worker {
        shared: Arc::clone(&shared),
    };
    while !shared.stop.load(Ordering::SeqCst) {
        if queue.blocking_dispatch(&mut worker).is_err() {
            shared.gone.store(true, Ordering::SeqCst);
            return;
        }
    }
}

impl Events for Worker {
    fn selection(&mut self, offer: Option<Offer>) {
        let s = &self.shared;
        let old = std::mem::replace(&mut *lock(&s.current), offer);
        s.changes.fetch_add(1, Ordering::SeqCst);
        if let Some(old) = old {
            old.destroy();
        }
    }

    fn finished(&mut self) {
        self.shared.gone.store(true, Ordering::SeqCst);
        self.shared.device.destroy();
    }

    fn send(&mut self, source: &SourceData, mime: String, fd: OwnedFd) {
        // Only the types we offered; anything else gets an empty answer (the pipe closes).
        if !TEXT_MIMES.contains(&mime.as_str()) {
            return;
        }
        let text = Arc::clone(&source.text);
        // A paste that is slow to read must not hold up the worker.
        let _ = std::thread::Builder::new()
            .name("wayland-clipboard-send".into())
            .spawn(move || {
                let _ = write_bounded(fd, text.as_bytes(), WRITE_TIMEOUT);
            });
    }

    fn cancelled(&mut self, source: Source) {
        let mut ours = lock(&self.shared.ours);
        if ours.as_ref() == Some(&source) {
            *ours = None;
        }
        drop(ours);
        source.destroy();
    }
}

/// What the worker does when the compositor answers a sync. Every event it sent before has
/// been dispatched by then, so the count is exact at this moment.
enum Answer {
    /// Reports the count.
    Count(Sender<u32>),
    /// Takes the selection with `source` if the count is still `seq` (`None`: always), then
    /// reports the count. The source is gone from here once taken, by the worker or by a
    /// writer that stopped waiting.
    SetIfUnchanged {
        seq: Option<u32>,
        source: Mutex<Option<Source>>,
        reply: Sender<u32>,
    },
}

impl Answer {
    fn take_source(&self) -> Option<Source> {
        match self {
            Answer::Count(_) => None,
            Answer::SetIfUnchanged { source, .. } => lock(source).take(),
        }
    }
}

impl Dispatch<WlCallback, Answer> for Worker {
    fn event(
        worker: &mut Self,
        _: &WlCallback,
        event: wl_callback::Event,
        answer: &Answer,
        conn: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let wl_callback::Event::Done { .. } = event else {
            return;
        };
        let s = &worker.shared;
        let count = s.changes.load(Ordering::SeqCst);
        let reply = match answer {
            Answer::Count(reply) => reply,
            Answer::SetIfUnchanged { seq, reply, .. } => {
                if let Some(source) = answer.take_source() {
                    if seq.is_none_or(|seq| seq == count) {
                        // In place before the selection is ours: the next read asks right after
                        // the writer hears back, and a `cancelled` comes on this thread, later.
                        *lock(&s.ours) = Some(source.clone());
                        s.device.set_selection(&source);
                        let _ = conn.flush();
                    } else {
                        source.destroy();
                    }
                }
                reply
            }
        };
        // Nobody waits any more when the answer came too late.
        let _ = reply.send(count);
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Worker {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for Worker {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: <WlSeat as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Drop for DataControlClipboard {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // Any event wakes the worker, which then sees the flag. If the connection is gone the
        // worker has ended already.
        let (tx, _rx) = mpsc::channel();
        self.shared
            .conn
            .display()
            .sync(&self.shared.qh, Answer::Count(tx));
        let _ = self.shared.conn.flush();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Clipboard for DataControlClipboard {
    fn sequence(&self) -> u32 {
        let _op = self.begin();
        self.shared
            .sync()
            .unwrap_or_else(|_| self.shared.changes.load(Ordering::SeqCst))
    }

    fn read(&self) -> Result<ClipText, ClipError> {
        let _op = self.begin();
        self.shared.sync()?;
        self.read_current()
    }

    fn write_if_unchanged(&self, text: &str, seq: u32) -> Result<bool, ClipError> {
        let _op = self.begin();
        self.set(text, Some(seq))
    }

    /// Wayland has no clipboard history of its own; a clipboard manager's history is out of
    /// reach.
    fn delete_from_history(&self, _raw: &str) -> bool {
        false
    }
}

impl SystemClipboard for DataControlClipboard {
    fn write_text(&self, text: &str) -> Result<(), ClipError> {
        let _op = self.begin();
        self.set(text, None).map(|_| ())
    }

    // `keep_after_exit`: no protocol hands a selection over to a clipboard manager on Wayland.
    // A desktop with a manager (Klipper) takes the text over by itself; sway has none, and the
    // text is gone with the app.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_text_ends_at_nul_and_keeps_the_limit() {
        assert!(matches!(own_text("one\0two"), ClipText::Text(t) if t.as_str() == "one"));
        assert!(matches!(own_text("\0two"), ClipText::Empty));
        assert!(matches!(
            own_text(&"a".repeat(MAX_INPUT_BYTES + 1)),
            ClipText::TooLarge
        ));
    }

    fn hint(value: &[u8]) -> bool {
        use std::io::Write;
        let (reader, mut writer) = pipe().unwrap();
        writer.write_all(value).unwrap();
        drop(writer);
        hint_is_secret(Ok(reader))
    }

    #[test]
    fn only_the_word_secret_is_secret() {
        assert!(hint(b"secret"));
        assert!(hint(b" secret\n\0"));
        assert!(!hint(b"public"));
        assert!(!hint(b""));
    }

    #[test]
    fn an_unreadable_hint_counts_as_secret() {
        assert!(hint_is_secret(Err(ClipError)));
        assert!(hint(&[b' '; HINT_BYTES + 1]));
    }
}
