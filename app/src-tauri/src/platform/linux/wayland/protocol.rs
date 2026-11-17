//! The two data-control protocols behind one set of types: `ext-data-control-v1` (staging,
//! Plasma 6) and `zwlr-data-control-unstable-v1` (sway, KWin 5.27). They have the same requests
//! and events under different names, so the clipboard code exists once: it holds a [`Manager`],
//! a [`Device`], [`Source`]s and [`Offer`]s, and hears both protocols through [`Events`].
//!
//! Only version 1 of each is bound: the clipboard needs nothing newer, and zwlr's primary
//! selection (version 2) stays off. ext reports the primary selection in version 1; those
//! offers are destroyed unread.

use std::os::fd::{BorrowedFd, OwnedFd};
use std::sync::{Arc, Mutex};

use wayland_client::globals::GlobalList;
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
use zeroize::Zeroizing;

use super::datacontrol::Worker;

/// How many mime types of one offer are kept; a sender that lists more is not believed.
const MAX_MIMES: usize = 1024;

/// What the clipboard hears from either protocol, on the worker thread.
pub(super) trait Events {
    /// The selection changed: the offer of the new one, or `None` when it is empty now.
    fn selection(&mut self, offer: Option<Offer>);
    /// The compositor ended the device (its seat is gone).
    fn finished(&mut self);
    /// Somebody pastes from our `source`: write its text to `fd`.
    fn send(&mut self, source: &SourceData, mime: String, fd: OwnedFd);
    /// Another copy replaced our `source`.
    fn cancelled(&mut self, source: Source);
}

/// The text one of our sources serves.
pub(super) struct SourceData {
    pub text: Arc<Zeroizing<String>>,
}

/// The mime types another client offers, collected from `offer` events before the `selection`
/// event names the offer.
#[derive(Default)]
pub(super) struct OfferData {
    mimes: Mutex<Vec<String>>,
}

impl OfferData {
    fn add(&self, mime: String) {
        let mut mimes = self.mimes.lock().unwrap_or_else(|e| e.into_inner());
        if mimes.len() < MAX_MIMES {
            mimes.push(mime);
        }
    }
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

pub(super) enum Manager {
    Ext(ext_manager::ExtDataControlManagerV1),
    Wlr(wlr_manager::ZwlrDataControlManagerV1),
}

impl Manager {
    /// The ext manager when the compositor offers it, else the zwlr one.
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<Worker>) -> Option<Manager> {
        if let Ok(m) = globals.bind(qh, 1..=1, ()) {
            return Some(Manager::Ext(m));
        }
        globals.bind(qh, 1..=1, ()).ok().map(Manager::Wlr)
    }

    pub fn get_device(&self, seat: &WlSeat, qh: &QueueHandle<Worker>) -> Device {
        match self {
            Manager::Ext(m) => Device::Ext(m.get_data_device(seat, qh, ())),
            Manager::Wlr(m) => Device::Wlr(m.get_data_device(seat, qh, ())),
        }
    }

    pub fn create_source(&self, qh: &QueueHandle<Worker>, data: SourceData) -> Source {
        match self {
            Manager::Ext(m) => Source::Ext(m.create_data_source(qh, data)),
            Manager::Wlr(m) => Source::Wlr(m.create_data_source(qh, data)),
        }
    }
}

pub(super) enum Device {
    Ext(ext_device::ExtDataControlDeviceV1),
    Wlr(wlr_device::ZwlrDataControlDeviceV1),
}

impl Device {
    pub fn set_selection(&self, source: &Source) {
        match (self, source) {
            (Device::Ext(d), Source::Ext(s)) => d.set_selection(Some(s)),
            (Device::Wlr(d), Source::Wlr(s)) => d.set_selection(Some(s)),
            // Never mixed: every object comes from the one manager.
            _ => {}
        }
    }

    pub fn destroy(&self) {
        either!(self, Device, d => d.destroy());
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Source {
    Ext(ext_source::ExtDataControlSourceV1),
    Wlr(wlr_source::ZwlrDataControlSourceV1),
}

impl Source {
    pub fn offer(&self, mime: &str) {
        either!(self, Source, s => s.offer(mime.to_owned()));
    }

    pub fn destroy(&self) {
        either!(self, Source, s => s.destroy());
    }

    /// The text this source serves (`None` once the object is gone).
    pub fn text(&self) -> Option<Arc<Zeroizing<String>>> {
        either!(self, Source, s => s.data::<SourceData>().map(|d| Arc::clone(&d.text)))
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Offer {
    Ext(ext_offer::ExtDataControlOfferV1),
    Wlr(wlr_offer::ZwlrDataControlOfferV1),
}

impl Offer {
    /// The mime types offered.
    pub fn mimes(&self) -> Vec<String> {
        either!(self, Offer, o => o.data::<OfferData>().map_or_else(Vec::new, |d| {
            d.mimes.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }))
    }

    /// Asks the sender to write `mime` into `fd`.
    pub fn receive(&self, mime: &str, fd: BorrowedFd<'_>) {
        either!(self, Offer, o => o.receive(mime.to_owned(), fd));
    }

    pub fn destroy(&self) {
        either!(self, Offer, o => o.destroy());
    }
}

/// The `Dispatch` implementations of one protocol: each event becomes a call of [`Events`].
macro_rules! dispatch {
    (
        $kind:ident,
        $manager:ident :: $Manager:ident,
        $device:ident :: $Device:ident,
        $source:ident :: $Source:ident,
        $offer:ident :: $Offer:ident $(,)?
    ) => {
        impl Dispatch<$manager::$Manager, ()> for Worker {
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

        impl Dispatch<$device::$Device, ()> for Worker {
            fn event(
                worker: &mut Self,
                _: &$device::$Device,
                event: $device::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                match event {
                    $device::Event::Selection { id } => worker.selection(id.map(Offer::$kind)),
                    $device::Event::PrimarySelection { id: Some(offer) } => offer.destroy(),
                    $device::Event::Finished => worker.finished(),
                    // A new offer collects its mime types until a selection event names it.
                    _ => {}
                }
            }

            event_created_child!(Worker, $device::$Device, [
                $device::EVT_DATA_OFFER_OPCODE => ($offer::$Offer, OfferData::default()),
            ]);
        }

        impl Dispatch<$offer::$Offer, OfferData> for Worker {
            fn event(
                _: &mut Self,
                _: &$offer::$Offer,
                event: $offer::Event,
                data: &OfferData,
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                if let $offer::Event::Offer { mime_type } = event {
                    data.add(mime_type);
                }
            }
        }

        impl Dispatch<$source::$Source, SourceData> for Worker {
            fn event(
                worker: &mut Self,
                source: &$source::$Source,
                event: $source::Event,
                data: &SourceData,
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                match event {
                    $source::Event::Send { mime_type, fd } => worker.send(data, mime_type, fd),
                    $source::Event::Cancelled => worker.cancelled(Source::$kind(source.clone())),
                    _ => {}
                }
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

/// The interface names [`super::globals`] looks for.
pub(super) fn interface_names() -> (&'static str, &'static str) {
    (
        ext_manager::ExtDataControlManagerV1::interface().name,
        wlr_manager::ZwlrDataControlManagerV1::interface().name,
    )
}
