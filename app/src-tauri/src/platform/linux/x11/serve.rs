//! Serving the clipboard: the owner side of the X11 selection protocol.
//!
//! On X11 the text stays with the app that copied it; another app that pastes asks us for it.
//! All of this runs on the worker thread. Text that fits in one request is answered in one
//! property; larger text goes in pieces (INCR), each sent when the requestor has deleted the
//! one before. A transfer works on a snapshot of the text taken when it was asked for.
//!
//! The requestor is another program and is not trusted either: X errors are ignored, the number
//! of transfers is capped, and a transfer whose requestor went away or stopped asking is
//! dropped.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, EventMask, PropMode,
    SelectionNotifyEvent, SelectionRequestEvent, Window, SELECTION_NOTIFY_EVENT,
};
use x11rb::wrapper::ConnectionExt as _;
use zeroize::Zeroizing;

use super::Shared;

/// Text up to this size is answered in one property; larger text goes in pieces of this size.
pub(super) const INCR_CHUNK: usize = 256 * 1024;
/// More transfers in pieces at once are refused.
const MAX_TRANSFERS: usize = 8;
/// More pairs in one MULTIPLE request are refused.
const MAX_PAIRS: u32 = 16;
/// A transfer whose requestor has not asked for the next piece for this long is dropped (at
/// the next event the worker sees).
const IDLE: Duration = Duration::from_secs(5);

/// A transfer in pieces to one requestor.
struct Transfer {
    requestor: Window,
    property: Atom,
    target: Atom,
    text: Arc<Zeroizing<String>>,
    sent: usize,
    touched: Instant,
}

/// The transfers in progress. Owned by the worker thread.
#[derive(Default)]
pub(super) struct Serving {
    transfers: Vec<Transfer>,
}

impl Serving {
    /// Another app asks for the clipboard. It always gets an answer: the property that holds
    /// the data, or a refusal.
    pub(super) fn request(&mut self, shared: &Shared, req: &SelectionRequestEvent) {
        let answered = self.answer(shared, req);
        let event = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: req.time,
            requestor: req.requestor,
            selection: req.selection,
            target: req.target,
            property: if answered { req.property } else { x11rb::NONE },
        };
        let _ = shared
            .conn
            .send_event(false, req.requestor, EventMask::NO_EVENT, event);
        let _ = shared.conn.flush();
    }

    /// Writes the answer to the requestor's property. False: refused.
    fn answer(&mut self, shared: &Shared, req: &SelectionRequestEvent) -> bool {
        let a = &shared.atoms;
        if req.selection != a.CLIPBOARD || req.owner != shared.window || req.property == x11rb::NONE
        {
            return false;
        }
        // Never onto a window of our own. Any client can send a request that names one, and the
        // answer would overwrite the property our pings use or switch off the events we wait for.
        if req.requestor == shared.window
            || req.requestor == shared.listening.load(Ordering::SeqCst)
        {
            return false;
        }
        let Some(text) = shared.owned_text() else {
            return false;
        };
        if req.target == a.MULTIPLE {
            return self.multiple(shared, req, &text);
        }
        self.convert(shared, req.requestor, req.target, req.property, &text)
    }

    /// Several targets in one request (clipboard managers save this way): the requestor's
    /// property lists pairs of target and property. Each pair is answered like a request of its
    /// own; the property of a pair we refuse is set to None in the list.
    fn multiple(
        &mut self,
        shared: &Shared,
        req: &SelectionRequestEvent,
        text: &Arc<Zeroizing<String>>,
    ) -> bool {
        let conn = &shared.conn;
        let listed = conn
            .get_property(
                false,
                req.requestor,
                req.property,
                AtomEnum::ANY,
                0,
                2 * MAX_PAIRS,
            )
            .ok()
            .and_then(|cookie| cookie.reply().ok());
        let Some(listed) = listed.filter(|l| l.bytes_after == 0) else {
            return false;
        };
        let Some(mut pairs) = listed.value32().map(|v| v.collect::<Vec<Atom>>()) else {
            return false;
        };
        if pairs.is_empty() || pairs.len() % 2 != 0 {
            return false;
        }
        for pair in pairs.as_chunks_mut::<2>().0 {
            let (target, property) = (pair[0], pair[1]);
            let answered = property != x11rb::NONE
                && property != req.property
                && target != shared.atoms.MULTIPLE
                && self.convert(shared, req.requestor, target, property, text);
            if !answered {
                pair[1] = x11rb::NONE;
            }
        }
        conn.change_property32(
            PropMode::REPLACE,
            req.requestor,
            req.property,
            listed.type_,
            &pairs,
        )
        .is_ok()
    }

    /// Writes one target of our text to `property` on the requestor's `window`. False: refused.
    fn convert(
        &mut self,
        shared: &Shared,
        window: Window,
        target: Atom,
        property: Atom,
        text: &Arc<Zeroizing<String>>,
    ) -> bool {
        let a = &shared.atoms;
        let conn = &shared.conn;
        let sent = if target == a.TARGETS {
            let targets = [
                a.TARGETS,
                a.MULTIPLE,
                a.TIMESTAMP,
                a.UTF8_STRING,
                a.TEXT_PLAIN_UTF8,
            ];
            conn.change_property32(
                PropMode::REPLACE,
                window,
                property,
                AtomEnum::ATOM,
                &targets,
            )
        } else if target == a.TIMESTAMP {
            let since = [shared.owned_at.load(Ordering::SeqCst)];
            conn.change_property32(
                PropMode::REPLACE,
                window,
                property,
                AtomEnum::INTEGER,
                &since,
            )
        } else if target == a.UTF8_STRING || target == a.TEXT_PLAIN_UTF8 {
            if text.len() <= shared.chunk {
                conn.change_property8(PropMode::REPLACE, window, property, target, text.as_bytes())
            } else {
                return self.announce(shared, window, target, property, Arc::clone(text));
            }
        } else {
            return false;
        };
        sent.is_ok()
    }

    /// Starts a transfer in pieces: the property says INCR and the size; the pieces follow as
    /// the requestor deletes the property.
    fn announce(
        &mut self,
        shared: &Shared,
        requestor: Window,
        target: Atom,
        property: Atom,
        text: Arc<Zeroizing<String>>,
    ) -> bool {
        let conn = &shared.conn;
        // A requestor that asks again on the same property has given the earlier transfer up.
        self.transfers
            .retain(|t| !(t.requestor == requestor && t.property == property));
        let Ok(size) = u32::try_from(text.len()) else {
            return false;
        };
        if self.transfers.len() >= MAX_TRANSFERS {
            return false;
        }
        // Its deletes of the property, and its window going away. Asked for before the
        // announcement is written, so that no delete is missed.
        let watch = ChangeWindowAttributesAux::new()
            .event_mask(EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY);
        if conn.change_window_attributes(requestor, &watch).is_err() {
            return false;
        }
        self.transfers.push(Transfer {
            requestor,
            property,
            target,
            text,
            sent: 0,
            touched: Instant::now(),
        });
        conn.change_property32(
            PropMode::REPLACE,
            requestor,
            property,
            shared.atoms.INCR,
            &[size],
        )
        .is_ok()
    }

    /// A property was deleted on some window. If that is a requestor asking for its next
    /// piece, sends it (an empty piece ends the transfer) and returns true.
    pub(super) fn deleted(&mut self, shared: &Shared, window: Window, property: Atom) -> bool {
        let Some(i) = self
            .transfers
            .iter()
            .position(|t| t.requestor == window && t.property == property)
        else {
            return false;
        };
        let t = &mut self.transfers[i];
        let bytes = t.text.as_bytes();
        let end = bytes.len().min(t.sent + shared.chunk);
        let piece = &bytes[t.sent..end];
        let _ = shared
            .conn
            .change_property8(PropMode::REPLACE, window, property, t.target, piece);
        let done = piece.is_empty();
        t.sent = end;
        t.touched = Instant::now();
        if done {
            self.transfers.swap_remove(i);
            self.unwatch(shared, window);
        }
        let _ = shared.conn.flush();
        true
    }

    /// A window was destroyed: transfers to it are over.
    pub(super) fn gone(&mut self, window: Window) {
        self.transfers.retain(|t| t.requestor != window);
    }

    /// Drops the transfers whose requestor stopped asking.
    pub(super) fn expire(&mut self, shared: &Shared) {
        let now = Instant::now();
        let mut stale = Vec::new();
        self.transfers.retain(|t| {
            let keep = now.duration_since(t.touched) < IDLE;
            if !keep {
                stale.push(t.requestor);
            }
            keep
        });
        if !stale.is_empty() {
            for window in stale {
                self.unwatch(shared, window);
            }
            let _ = shared.conn.flush();
        }
    }

    /// Stops listening to a requestor's window once no transfer to it is left.
    fn unwatch(&self, shared: &Shared, window: Window) {
        if self.transfers.iter().all(|t| t.requestor != window) {
            let quiet = ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT);
            let _ = shared.conn.change_window_attributes(window, &quiet);
        }
    }
}
