//! Another application on the throwaway Wayland compositor, for the clipboard contract on
//! Wayland.
//!
//! Independent code on purpose: it never uses `DataControlClipboard`. It is a client of its own,
//! on its own connection, with its own `zwlr_data_control` source, so it can offer any set of
//! mime types and answer a paste in odd ways (an endless stream, never, a few bytes and then the
//! end). Pastes go through `wl-paste`, a real third-party client.
//!
//! The including test file must name this module `wayland_other`.

use std::fs::File;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_device_v1::{
    self, ZwlrDataControlDeviceV1,
};
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_offer_v1::ZwlrDataControlOfferV1;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_source_v1::{
    self, ZwlrDataControlSourceV1,
};

use crate::contract::OtherApp;

/// The throwaway compositor's socket. Panics unless this process runs inside
/// `tools/ci/wayland-tests.sh`: these tests never touch a real desktop (on WSLg the inherited
/// `WAYLAND_DISPLAY` is the Windows clipboard).
pub fn compositor() -> String {
    let socket = std::env::var("REDACTOR_TEST_WAYLAND")
        .expect("REDACTOR_TEST_WAYLAND is not set: run tools/ci/wayland-tests.sh");
    assert!(
        !socket.is_empty() && !socket.contains('/'),
        "REDACTOR_TEST_WAYLAND must be a socket name in XDG_RUNTIME_DIR"
    );
    assert_eq!(
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        Some(socket.as_str()),
        "WAYLAND_DISPLAY is not the test compositor"
    );
    assert!(
        std::env::var_os("WAYLAND_SOCKET").is_none(),
        "WAYLAND_SOCKET is set and would win over WAYLAND_DISPLAY"
    );
    let runtime = std::env::var("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR is not set");
    let mark = std::fs::read_to_string(Path::new(&runtime).join("redactor-test-runtime"))
        .expect("XDG_RUNTIME_DIR is not the folder of tools/ci/wayland-tests.sh");
    assert_eq!(
        mark.trim_end(),
        socket,
        "XDG_RUNTIME_DIR belongs to another run of tools/ci/wayland-tests.sh"
    );
    socket
}

/// The text types a text copy offers, as GTK, Qt and `wl-copy` do.
pub const TEXT_MIMES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
    "STRING",
    "TEXT",
];

/// The password-manager hint (KeePassXC, Bitwarden and others set it to `secret`).
pub const HINT: &str = "x-kde-passwordManagerHint";

/// A 1x1 transparent PNG.
const PNG: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// How long the compositor gets to answer a round trip.
const ANSWER_DEADLINE: Duration = Duration::from_secs(2);

/// How a copy answers a paste.
enum Serve {
    /// The bytes of each offered mime type.
    Data(Vec<(String, Arc<[u8]>)>),
    /// Writes until the reader closes the pipe.
    Endless,
    /// Keeps the pipe open and never writes.
    Silent,
    /// Writes these bytes and closes the pipe, whatever was asked.
    ShortThenClose(Vec<u8>),
    /// Writes these bytes one at a time, with a pause after each.
    Trickle(Arc<[u8]>),
}

/// The user data of a source of ours.
struct SourceData {
    serve: Serve,
    /// The compositor said another copy replaced this one.
    cancelled: AtomicBool,
}

/// What the event thread and the test thread share.
#[derive(Default)]
struct Shared {
    /// Selection events seen so far.
    selections: AtomicU32,
    /// The offer of the current selection (destroyed when the next one comes).
    offer: Mutex<Option<ZwlrDataControlOfferV1>>,
    /// The pipes of `copy_silent`, open until this app is dropped.
    held: Mutex<Vec<OwnedFd>>,
}

/// The state of the event thread.
struct State {
    shared: Arc<Shared>,
}

pub struct WaylandOther {
    conn: Connection,
    qh: QueueHandle<State>,
    manager: ZwlrDataControlManagerV1,
    device: ZwlrDataControlDeviceV1,
    shared: Arc<Shared>,
    /// The source of the last copy.
    last: Mutex<Option<ZwlrDataControlSourceV1>>,
    stop: Arc<AtomicBool>,
    events: Option<JoinHandle<()>>,
}

impl WaylandOther {
    pub fn new() -> WaylandOther {
        compositor();
        let conn = Connection::connect_to_env().expect("connect to the test compositor");
        let (globals, mut queue) = registry_queue_init::<State>(&conn).unwrap();
        let qh = queue.handle();
        let seat: WlSeat = globals.bind(&qh, 1..=1, ()).expect("no seat");
        let manager: ZwlrDataControlManagerV1 = globals
            .bind(&qh, 1..=1, ())
            .expect("the compositor offers no zwlr data-control");
        let device = manager.get_data_device(&seat, &qh, ());
        let shared = Arc::new(Shared::default());
        let stop = Arc::new(AtomicBool::new(false));
        let events = std::thread::spawn({
            let mut state = State {
                shared: Arc::clone(&shared),
            };
            let stop = Arc::clone(&stop);
            move || {
                while !stop.load(Ordering::SeqCst) {
                    if queue.blocking_dispatch(&mut state).is_err() {
                        return;
                    }
                }
            }
        });
        let other = WaylandOther {
            conn,
            qh,
            manager,
            device,
            shared,
            last: Mutex::new(None),
            stop,
            events: Some(events),
        };
        other.sync();
        other
    }

    /// Waits until the compositor has handled everything this app sent so far and its answers
    /// are in.
    fn sync(&self) {
        let (tx, rx) = mpsc::channel();
        self.conn.display().sync(&self.qh, tx);
        self.conn.flush().unwrap();
        rx.recv_timeout(ANSWER_DEADLINE)
            .expect("the compositor did not answer in 2 s");
    }

    /// Offers `mimes` and makes the copy the selection, without waiting for the compositor.
    fn start(&self, serve: Serve, mimes: &[&str]) {
        let data = SourceData {
            serve,
            cancelled: AtomicBool::new(false),
        };
        let source = self.manager.create_data_source(&self.qh, data);
        for mime in mimes {
            source.offer((*mime).to_string());
        }
        self.device.set_selection(Some(&source));
        self.conn.flush().unwrap();
        *self.last.lock().unwrap() = Some(source);
    }

    /// A copy, then the wait for its own selection event.
    fn copy(&self, serve: Serve, mimes: &[&str]) {
        let before = self.shared.selections.load(Ordering::SeqCst);
        self.start(serve, mimes);
        self.sync();
        assert_ne!(
            self.shared.selections.load(Ordering::SeqCst),
            before,
            "the copy did not become the selection"
        );
    }

    /// Copies with exactly these mime types and contents.
    pub fn copy_with(&self, mimes: &[(&str, &[u8])]) {
        let names: Vec<&str> = mimes.iter().map(|(m, _)| *m).collect();
        let data = mimes
            .iter()
            .map(|(m, bytes)| ((*m).to_string(), Arc::from(*bytes)))
            .collect();
        self.copy(Serve::Data(data), &names);
    }

    /// `text` under every text type, and `extra` beside it. One buffer serves all of them.
    fn copy_text_and(&self, text: &str, extra: &[(&str, &[u8])]) {
        let bytes: Arc<[u8]> = Arc::from(text.as_bytes());
        let mut data: Vec<(String, Arc<[u8]>)> = TEXT_MIMES
            .iter()
            .map(|m| ((*m).to_string(), Arc::clone(&bytes)))
            .collect();
        data.extend(extra.iter().map(|(m, b)| ((*m).to_string(), Arc::from(*b))));
        let names: Vec<&str> = data.iter().map(|(m, _)| m.as_str()).collect();
        self.copy(Serve::Data(data.clone()), &names);
    }

    /// A text copy that answers every paste with an endless stream.
    pub fn copy_endless(&self) {
        self.copy(Serve::Endless, &TEXT_MIMES);
    }

    /// A text copy that never writes a byte and keeps the pipe open.
    pub fn copy_silent(&self) {
        self.copy(Serve::Silent, &TEXT_MIMES);
    }

    /// A text copy that writes `bytes` and closes the pipe.
    pub fn copy_short_then_close(&self, bytes: &[u8]) {
        self.copy(Serve::ShortThenClose(bytes.to_vec()), &TEXT_MIMES);
    }

    /// A text copy that answers every paste with `text`, one byte per write.
    pub fn copy_trickle(&self, text: &str) {
        self.copy(Serve::Trickle(Arc::from(text.as_bytes())), &TEXT_MIMES);
    }

    /// A text copy that is still on its way when this returns.
    pub fn start_copy(&self, text: &str) {
        let bytes: Arc<[u8]> = Arc::from(text.as_bytes());
        let data = TEXT_MIMES
            .iter()
            .map(|m| ((*m).to_string(), Arc::clone(&bytes)))
            .collect();
        self.start(Serve::Data(data), &TEXT_MIMES);
    }

    /// True when the last copy of this app is the selection now.
    pub fn copy_is_in_place(&self) -> bool {
        self.sync();
        self.last.lock().unwrap().as_ref().is_some_and(|source| {
            !source
                .data::<SourceData>()
                .unwrap()
                .cancelled
                .load(Ordering::SeqCst)
        })
    }

    /// True when `wl-paste` finds a selection at all.
    pub fn has_selection(&self) -> bool {
        Command::new("timeout")
            .args(["10", "wl-paste", "--list-types"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("start wl-paste")
            .success()
    }
}

impl Drop for WaylandOther {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Any event wakes the event thread, which then sees the flag.
        let (tx, _rx) = mpsc::channel();
        self.conn.display().sync(&self.qh, tx);
        let _ = self.conn.flush();
        if let Some(events) = self.events.take() {
            let _ = events.join();
        }
    }
}

impl OtherApp for WaylandOther {
    fn copy_text(&self, text: &str) {
        self.copy_text_and(text, &[]);
    }

    fn copy_protected(&self, text: &str) {
        self.copy_text_and(text, &[(HINT, b"secret")]);
    }

    fn copy_image(&self) {
        self.copy_with(&[("image/png", &PNG)]);
    }

    fn clear(&self) {
        self.sync();
        *self.last.lock().unwrap() = None;
        // The compositor sends nothing for a clipboard that is empty already.
        if self.shared.offer.lock().unwrap().is_none() {
            return;
        }
        let before = self.shared.selections.load(Ordering::SeqCst);
        self.device.set_selection(None);
        self.sync();
        assert_ne!(
            self.shared.selections.load(Ordering::SeqCst),
            before,
            "the selection was not cleared"
        );
        assert!(
            self.shared.offer.lock().unwrap().is_none(),
            "the clipboard still holds something"
        );
    }

    fn paste(&self) -> Option<String> {
        let out = Command::new("timeout")
            .args(["10", "wl-paste", "--no-newline"])
            .stderr(Stdio::null())
            .output()
            .expect("start wl-paste");
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).expect("wl-paste returned invalid UTF-8"))
    }
}

/// Writes `bytes` to a paste's pipe on a thread of its own; a reader that goes away ends it.
fn write_out(fd: OwnedFd, bytes: Arc<[u8]>) {
    std::thread::spawn(move || {
        let _ = File::from(fd).write_all(&bytes);
    });
}

impl Dispatch<ZwlrDataControlSourceV1, SourceData> for State {
    fn event(
        state: &mut Self,
        source: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        data: &SourceData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_source_v1::Event::Send { mime_type, fd } => match &data.serve {
                Serve::Data(offered) => {
                    if let Some((_, bytes)) = offered.iter().find(|(m, _)| *m == mime_type) {
                        write_out(fd, Arc::clone(bytes));
                    }
                }
                Serve::ShortThenClose(bytes) => write_out(fd, Arc::from(bytes.as_slice())),
                Serve::Silent => state.shared.held.lock().unwrap().push(fd),
                Serve::Trickle(bytes) => {
                    let bytes = Arc::clone(bytes);
                    std::thread::spawn(move || {
                        let mut pipe = File::from(fd);
                        for byte in bytes.iter() {
                            if pipe.write_all(std::slice::from_ref(byte)).is_err() {
                                return;
                            }
                            std::thread::sleep(Duration::from_micros(100));
                        }
                    });
                }
                Serve::Endless => {
                    std::thread::spawn(move || {
                        let mut pipe = File::from(fd);
                        let chunk = vec![b'a'; 64 * 1024];
                        while pipe.write_all(&chunk).is_ok() {}
                    });
                }
            },
            zwlr_data_control_source_v1::Event::Cancelled => {
                data.cancelled.store(true, Ordering::SeqCst);
                source.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_device_v1::Event::Selection { id } = event {
            state.shared.selections.fetch_add(1, Ordering::SeqCst);
            let old = std::mem::replace(&mut *state.shared.offer.lock().unwrap(), id);
            if let Some(old) = old {
                old.destroy();
            }
        }
    }

    event_created_child!(State, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl Dispatch<WlCallback, Sender<()>> for State {
    fn event(
        _: &mut Self,
        _: &WlCallback,
        event: wl_callback::Event,
        done: &Sender<()>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            let _ = done.send(());
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
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

impl Dispatch<WlSeat, ()> for State {
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

impl Dispatch<ZwlrDataControlManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _: <ZwlrDataControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlOfferV1,
        _: <ZwlrDataControlOfferV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
