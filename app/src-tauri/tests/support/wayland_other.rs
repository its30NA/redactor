//! Another application on the throwaway Wayland compositor, for the clipboard contract on
//! Wayland.
//!
//! Independent code on purpose: it never uses `DataControlClipboard`. It is a client of its own,
//! on its own connection, with its own data-control source, so it can offer any set of mime
//! types and answer a paste in odd ways (an endless stream, never, a few bytes and then the end).
//! Pastes go through `wl-paste`, a real third-party client.
//!
//! It speaks `ext-data-control-v1` where the compositor offers it (KWin 6 offers nothing else),
//! else `zwlr-data-control-unstable-v1` (sway).
//!
//! The including test file must name this module `wayland_other`, and include
//! `support/wayland_session.rs` as `wayland_session`.

use std::fs::File;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use wayland_client::globals::{registry_queue_init, GlobalList, GlobalListContents};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1 as ext_device, ext_data_control_manager_v1 as ext_manager,
    ext_data_control_offer_v1 as ext_offer, ext_data_control_source_v1 as ext_source,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1 as wlr_device, zwlr_data_control_manager_v1 as wlr_manager,
    zwlr_data_control_offer_v1 as wlr_offer, zwlr_data_control_source_v1 as wlr_source,
};

use crate::contract::OtherApp;
use crate::wayland_session::compositor;

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
    offer: Mutex<Option<Offer>>,
    /// The pipes of `copy_silent`, open until this app is dropped.
    held: Mutex<Vec<OwnedFd>>,
}

/// The state of the event thread.
struct State {
    shared: Arc<Shared>,
}

/// `$body` with `$p` bound to the proxy inside `$value`, whichever protocol it belongs to.
macro_rules! either {
    ($value:expr, $enum:ident, $p:ident => $body:expr) => {
        match $value {
            $enum::Ext($p) => $body,
            $enum::Wlr($p) => $body,
        }
    };
}

/// The two protocols have the same requests and events under different names. Every object of
/// this app comes from the one manager it bound.
enum Manager {
    Ext(ext_manager::ExtDataControlManagerV1),
    Wlr(wlr_manager::ZwlrDataControlManagerV1),
}

enum Device {
    Ext(ext_device::ExtDataControlDeviceV1),
    Wlr(wlr_device::ZwlrDataControlDeviceV1),
}

enum Source {
    Ext(ext_source::ExtDataControlSourceV1),
    Wlr(wlr_source::ZwlrDataControlSourceV1),
}

enum Offer {
    Ext(ext_offer::ExtDataControlOfferV1),
    Wlr(wlr_offer::ZwlrDataControlOfferV1),
}

impl Manager {
    /// ext where the compositor offers it, else zwlr.
    fn bind(globals: &GlobalList, qh: &QueueHandle<State>) -> Manager {
        if let Ok(m) = globals.bind(qh, 1..=1, ()) {
            return Manager::Ext(m);
        }
        Manager::Wlr(
            globals
                .bind(qh, 1..=1, ())
                .expect("the compositor offers no data-control (ext or zwlr)"),
        )
    }

    fn get_device(&self, seat: &WlSeat, qh: &QueueHandle<State>) -> Device {
        match self {
            Manager::Ext(m) => Device::Ext(m.get_data_device(seat, qh, ())),
            Manager::Wlr(m) => Device::Wlr(m.get_data_device(seat, qh, ())),
        }
    }

    fn create_source(&self, qh: &QueueHandle<State>, data: SourceData) -> Source {
        match self {
            Manager::Ext(m) => Source::Ext(m.create_data_source(qh, data)),
            Manager::Wlr(m) => Source::Wlr(m.create_data_source(qh, data)),
        }
    }
}

impl Device {
    /// Makes `source` the selection, or clears it.
    fn set_selection(&self, source: Option<&Source>) {
        match (self, source) {
            (Device::Ext(d), Some(Source::Ext(s))) => d.set_selection(Some(s)),
            (Device::Ext(d), None) => d.set_selection(None),
            (Device::Wlr(d), Some(Source::Wlr(s))) => d.set_selection(Some(s)),
            (Device::Wlr(d), None) => d.set_selection(None),
            _ => unreachable!("a source of the other protocol"),
        }
    }
}

impl Source {
    fn offer(&self, mime: &str) {
        either!(self, Source, s => s.offer(mime.to_string()));
    }

    /// The compositor said another copy replaced this one.
    fn cancelled(&self) -> bool {
        either!(self, Source, s => s.data::<SourceData>().unwrap().cancelled.load(Ordering::SeqCst))
    }
}

impl Offer {
    fn destroy(&self) {
        either!(self, Offer, o => o.destroy());
    }
}

pub struct WaylandOther {
    conn: Connection,
    qh: QueueHandle<State>,
    manager: Manager,
    device: Device,
    shared: Arc<Shared>,
    /// The source of the last copy.
    last: Mutex<Option<Source>>,
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
        let manager = Manager::bind(&globals, &qh);
        let device = manager.get_device(&seat, &qh);
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
        let source = self.manager.create_source(&self.qh, data);
        for mime in mimes {
            source.offer(mime);
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
        self.last
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|source| !source.cancelled())
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

impl State {
    /// Answers a paste of `mime_type` from a copy of ours, as the copy says.
    fn send(&self, data: &SourceData, mime_type: String, fd: OwnedFd) {
        match &data.serve {
            Serve::Data(offered) => {
                if let Some((_, bytes)) = offered.iter().find(|(m, _)| *m == mime_type) {
                    write_out(fd, Arc::clone(bytes));
                }
            }
            Serve::ShortThenClose(bytes) => write_out(fd, Arc::from(bytes.as_slice())),
            Serve::Silent => self.shared.held.lock().unwrap().push(fd),
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
        }
    }

    /// A selection event: its offer (or none) replaces the one before.
    fn selection(&self, offer: Option<Offer>) {
        self.shared.selections.fetch_add(1, Ordering::SeqCst);
        let old = std::mem::replace(&mut *self.shared.offer.lock().unwrap(), offer);
        if let Some(old) = old {
            old.destroy();
        }
    }
}

/// The `Dispatch` implementations of one protocol.
macro_rules! dispatch {
    (
        $kind:ident,
        $manager:ident :: $Manager:ident,
        $device:ident :: $Device:ident,
        $source:ident :: $Source:ident,
        $offer:ident :: $Offer:ident $(,)?
    ) => {
        impl Dispatch<$source::$Source, SourceData> for State {
            fn event(
                state: &mut Self,
                source: &$source::$Source,
                event: $source::Event,
                data: &SourceData,
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                match event {
                    $source::Event::Send { mime_type, fd } => state.send(data, mime_type, fd),
                    $source::Event::Cancelled => {
                        data.cancelled.store(true, Ordering::SeqCst);
                        source.destroy();
                    }
                    _ => {}
                }
            }
        }

        impl Dispatch<$device::$Device, ()> for State {
            fn event(
                state: &mut Self,
                _: &$device::$Device,
                event: $device::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                match event {
                    $device::Event::Selection { id } => state.selection(id.map(Offer::$kind)),
                    // ext reports the primary selection in version 1; nothing here reads it.
                    $device::Event::PrimarySelection { id: Some(offer) } => offer.destroy(),
                    _ => {}
                }
            }

            event_created_child!(State, $device::$Device, [
                $device::EVT_DATA_OFFER_OPCODE => ($offer::$Offer, ()),
            ]);
        }

        impl Dispatch<$manager::$Manager, ()> for State {
            fn event(
                _: &mut Self,
                _: &$manager::$Manager,
                _: $manager::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }

        impl Dispatch<$offer::$Offer, ()> for State {
            fn event(
                _: &mut Self,
                _: &$offer::$Offer,
                _: $offer::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    };
}

dispatch!(
    Ext,
    ext_manager::ExtDataControlManagerV1,
    ext_device::ExtDataControlDeviceV1,
    ext_source::ExtDataControlSourceV1,
    ext_offer::ExtDataControlOfferV1,
);
dispatch!(
    Wlr,
    wlr_manager::ZwlrDataControlManagerV1,
    wlr_device::ZwlrDataControlDeviceV1,
    wlr_source::ZwlrDataControlSourceV1,
    wlr_offer::ZwlrDataControlOfferV1,
);

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
