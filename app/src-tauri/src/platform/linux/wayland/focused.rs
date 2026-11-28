//! The clipboard while the app's window is in front, through GTK: for desktops without
//! data-control (GNOME, Weston, WSLg).
//!
//! There a Wayland client reaches the clipboard only while one of its windows has keyboard
//! focus. GTK is that client here. Every operation runs on GTK's main thread (the app's, where
//! Tauri runs the GTK main loop), handed over with `glib::MainContext::invoke`; the calling
//! thread waits up to 10 s for the answer, then gives up with an error. One operation at a
//! time.
//!
//! Each operation first catches up with the compositor: a round trip, then every GDK event it
//! brought. The sequence number and the window's focus are then those of this moment, as the
//! sync of `DataControlClipboard` makes them. Then, in the same main-thread step, a window of
//! ours must be active, or reading fails and writing does nothing.
//!
//! The sequence number counts GTK's `owner-change` signal, connected by the first operation.
//! The compositor tells a client about the selection only while it has focus: a copy made while
//! the app is in the background counts once its window is active again, before the first read.
//! `write_if_unchanged` compares and sets in one main-thread step, so no GTK event runs in
//! between. A write waits for the compositor's echo; a refused copy is an error.
//!
//! Text moves as bytes, as the sender offered them: GTK's own text calls would turn CRLF into LF
//! on the way in and LF into CRLF on the way out. Our text stays ours until GTK lets go of the
//! selection, and is zeroized then.
//!
//! Known limits: GTK reads a whole transfer before the size is known, and keeps its own copy of
//! the text it reads (and of what it sends to a paste), in buffers that are not zeroized. GTK's
//! waits have no deadline of their own: a sender that never answers keeps one open (the window
//! still works, its loop runs inside the wait) until the next copy ends it, and until then
//! every operation fails at once.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, glib};
use zeroize::Zeroizing;

use super::{hint_value_is_secret, lost, HINT_BYTES, PASSWORD_HINT, TEXT_MIMES};
use crate::clipscrub::{text_from_utf8, ClipError, ClipText, Clipboard};
use crate::platform::SystemClipboard;

/// How long the main thread gets to run an operation.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

/// What the calling threads and the main thread share.
struct Shared {
    /// `owner-change` signals seen so far.
    changes: AtomicU32,
    /// The signal is connected (on the main thread, by the first operation).
    watching: AtomicBool,
    /// An operation runs on the main thread. GTK's waits run the main loop, and with it work
    /// handed over later: an operation never runs inside another.
    busy: AtomicBool,
}

/// The clipboard of the app's GTK windows. Works only while one of them is active.
pub struct FocusedClipboard {
    shared: Arc<Shared>,
    /// One operation at a time.
    op: Mutex<()>,
}

impl FocusedClipboard {
    /// Makes no GTK call: GTK is not up yet when the platform is built.
    pub fn new() -> FocusedClipboard {
        FocusedClipboard {
            shared: Arc::new(Shared {
                changes: AtomicU32::new(0),
                watching: AtomicBool::new(false),
                busy: AtomicBool::new(false),
            }),
            op: Mutex::new(()),
        }
    }

    /// Runs `op` on GTK's main thread, after the catch-up, and waits for its answer.
    fn on_main<T, F>(&self, op: F) -> Result<T, ClipError>
    where
        T: Send + 'static,
        F: FnOnce(&gtk::Clipboard, &Shared) -> Result<T, ClipError> + Send + 'static,
    {
        let _op = self.op.lock().unwrap_or_else(|e| e.into_inner());
        let shared = Arc::clone(&self.shared);
        let (tx, rx) = mpsc::sync_channel(1);
        glib::MainContext::default().invoke(move || {
            // Nobody waits any more when the answer comes too late.
            let _ = tx.send(run(&shared, op));
        });
        rx.recv_timeout(ANSWER_TIMEOUT).map_err(lost)?
    }
}

impl Default for FocusedClipboard {
    fn default() -> FocusedClipboard {
        FocusedClipboard::new()
    }
}

/// One operation, where `invoke` runs it: on the main thread, or refused anywhere else.
fn run<T>(
    shared: &Arc<Shared>,
    op: impl FnOnce(&gtk::Clipboard, &Shared) -> Result<T, ClipError>,
) -> Result<T, ClipError> {
    // GTK is not up yet, or nobody runs the main loop and `invoke` ran this on the calling
    // thread.
    if !gtk::is_initialized_main_thread() {
        return Err(ClipError);
    }
    if shared.busy.swap(true, Ordering::SeqCst) {
        return Err(ClipError);
    }
    let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
    if !shared.watching.swap(true, Ordering::SeqCst) {
        let weak = Arc::downgrade(shared);
        // gtk-rs has no typed binding for this signal (its event type is not bound).
        clipboard.connect_local("owner-change", false, move |_| {
            if let Some(shared) = weak.upgrade() {
                shared.changes.fetch_add(1, Ordering::SeqCst);
            }
            None
        });
    }
    catch_up();
    let result = op(&clipboard, shared);
    shared.busy.store(false, Ordering::SeqCst);
    result
}

/// Asks the compositor "what happened up to now?" and handles every event that brought: a
/// copy, the focus going or coming.
fn catch_up() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    display.sync();
    while let Some(mut event) = gdk::Event::get() {
        gtk::main_do_event(&mut event);
    }
}

/// True when a window of ours has the keyboard focus.
fn window_in_front() -> bool {
    gtk::Window::list_toplevels().iter().any(|w| {
        w.downcast_ref::<gtk::Window>()
            .is_some_and(|w| w.is_active())
    })
}

/// Reads the selection; the window is in front.
fn read_now(clipboard: &gtk::Clipboard) -> Result<ClipText, ClipError> {
    let hint = gdk::Atom::intern(PASSWORD_HINT);
    let targets = clipboard.wait_for_targets().unwrap_or_default();
    if targets.contains(&hint) && hint_is_secret(clipboard.wait_for_contents(&hint)) {
        return Ok(ClipText::Protected);
    }
    let Some(mime) = TEXT_MIMES
        .into_iter()
        .map(gdk::Atom::intern)
        .find(|t| targets.contains(t))
    else {
        // An empty clipboard, an image, files: no text.
        return Ok(ClipText::Empty);
    };
    match clipboard.wait_for_contents(&mime) {
        Some(contents) if contents.length() >= 0 => {
            Ok(text_from_utf8(Zeroizing::new(contents.data())))
        }
        // The sender did not answer.
        _ => Err(ClipError),
    }
}

/// Makes `text` the selection and waits for the compositor's echo, which comes within the
/// round trip.
///
/// No echo: the compositor refused the copy. wlroots and Weston refuse one whose serial (that of
/// the window's last input) is older than the current selection's. GTK still takes its own
/// text for the clipboard then and would read it back: it lets go of it again (its request to
/// clear carries the same refused serial), and the write fails.
fn set_now(
    clipboard: &gtk::Clipboard,
    shared: &Shared,
    text: Zeroizing<String>,
) -> Result<(), ClipError> {
    let before = shared.changes.load(Ordering::SeqCst);
    let targets: Vec<gtk::TargetEntry> = TEXT_MIMES
        .iter()
        .map(|m| gtk::TargetEntry::new(m, gtk::TargetFlags::empty(), 0))
        .collect();
    // GTK keeps the closure, and with it the text, until another copy replaces ours.
    let offered = clipboard.set_with_data(&targets, move |_, data, _| {
        data.set(&data.target(), 8, text.as_bytes());
    });
    if !offered {
        return Err(ClipError);
    }
    catch_up();
    if shared.changes.load(Ordering::SeqCst) != before {
        return Ok(());
    }
    clipboard.clear();
    Err(ClipError)
}

/// True when the password-manager hint says `secret`, and also when it cannot be read or is
/// longer than a short word: in doubt the copy is left alone.
fn hint_is_secret(contents: Option<gtk::SelectionData>) -> bool {
    let Some(contents) = contents else {
        return true;
    };
    if contents.length() < 0 {
        return true;
    }
    let value = contents.data();
    value.len() > HINT_BYTES || hint_value_is_secret(&value)
}

impl Clipboard for FocusedClipboard {
    /// Where the main thread does not answer, the last number it saw.
    fn sequence(&self) -> u32 {
        self.on_main(|_, shared| Ok(shared.changes.load(Ordering::SeqCst)))
            .unwrap_or_else(|_| self.shared.changes.load(Ordering::SeqCst))
    }

    fn read(&self) -> Result<ClipText, ClipError> {
        self.on_main(|clipboard, _| {
            if !window_in_front() {
                return Err(ClipError);
            }
            read_now(clipboard)
        })
    }

    fn write_if_unchanged(&self, text: &str, seq: u32) -> Result<bool, ClipError> {
        let text = Zeroizing::new(text.to_owned());
        self.on_main(move |clipboard, shared| {
            if !window_in_front() || shared.changes.load(Ordering::SeqCst) != seq {
                return Ok(false);
            }
            set_now(clipboard, shared, text)?;
            Ok(true)
        })
    }

    /// Wayland has no clipboard history of its own; a clipboard manager's history is out of
    /// reach.
    fn delete_from_history(&self, _raw: &str) -> bool {
        false
    }
}

impl SystemClipboard for FocusedClipboard {
    fn write_text(&self, text: &str) -> Result<(), ClipError> {
        let text = Zeroizing::new(text.to_owned());
        self.on_main(move |clipboard, shared| {
            if !window_in_front() {
                return Err(ClipError);
            }
            set_now(clipboard, shared, text)
        })
    }

    fn needs_focus(&self) -> bool {
        true
    }
}

/// The calling side only; what runs on the main thread is tested on a compositor
/// (`tests/wayland_focused.rs`).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_gtk_every_operation_fails_and_nothing_hangs() {
        let cb = FocusedClipboard::new();
        assert!(cb.needs_focus());
        assert!(cb.read().is_err());
        assert_eq!(cb.write_if_unchanged("text", 0), Err(ClipError));
        assert!(cb.write_text("text").is_err());
        assert_eq!(cb.sequence(), 0);
    }

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn shareable_between_threads() {
        assert_send_sync::<FocusedClipboard>();
    }
}
