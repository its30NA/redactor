//! Wayland: what the compositor offers, and the clipboard through data-control.
//!
//! A Wayland client sees the clipboard only while one of its windows has keyboard focus, unless
//! the compositor offers data-control (`ext-data-control-v1` on Plasma 6, `zwlr-data-control`
//! on sway and KWin 5.27): a protocol made for clipboard managers that reads and writes the
//! selection of a seat at any time. [`globals`] asks which of the two is offered;
//! [`DataControlClipboard`] is the background clipboard through it, with its own connection.
//!
//! Everything another client sends is untrusted: mime lists are capped, every transfer has a
//! size limit and deadlines (`pipe`), and clipboard bytes are `Zeroizing`.

mod datacontrol;
mod pipe;
mod protocol;

pub use datacontrol::DataControlClipboard;

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection, Dispatch, QueueHandle};

use crate::clipscrub::ClipError;

/// Which data-control protocol a compositor offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataControlKind {
    /// `ext_data_control_manager_v1`, preferred where offered.
    Ext,
    /// `zwlr_data_control_manager_v1`.
    Wlr,
}

/// What the compositor offers that the app can use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Globals {
    pub data_control: Option<DataControlKind>,
    pub seat: bool,
}

/// Asks the compositor at `$WAYLAND_DISPLAY` for its globals: one round trip, then the
/// connection is closed.
pub fn globals() -> Result<Globals, ClipError> {
    let conn = Connection::connect_to_env().map_err(lost)?;
    let (list, _queue) = registry_queue_init::<Probe>(&conn).map_err(lost)?;
    let offered = list.contents().clone_list();
    let has = |name: &str| offered.iter().any(|g| g.interface == name);
    let (ext, wlr) = protocol::interface_names();
    let data_control = if has(ext) {
        Some(DataControlKind::Ext)
    } else if has(wlr) {
        Some(DataControlKind::Wlr)
    } else {
        None
    };
    Ok(Globals {
        data_control,
        seat: has("wl_seat"),
    })
}

/// The state of the probe's queue, which never dispatches.
struct Probe;

impl Dispatch<WlRegistry, GlobalListContents> for Probe {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Any Wayland error is "the clipboard is busy or unavailable"; none of them carries text.
fn lost<E>(_: E) -> ClipError {
    ClipError
}
