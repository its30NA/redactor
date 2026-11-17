//! A keyboard for the throwaway compositor.
//!
//! Headless sway has no input devices, so its seat has no keyboard, and without one no window
//! ever gets keyboard focus: GTK never calls a window active, and the compositor neither sends
//! a client the selection nor takes its copy. A virtual keyboard (`zwp_virtual_keyboard_v1`,
//! on a connection of its own) gives the seat one.
//!
//! It presses one key, and only on request: [`VirtualKeyboard::tap`], a Shift press and
//! release, the user's next input in the window that has the focus.
//!
//! The including test file must name this module `virtual_keyboard`.

use std::io::Write;
use std::os::fd::AsFd;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};

use protocol::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use protocol::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;

/// The client side of `virtual-keyboard-unstable-v1` (the XML next to this file, as the
/// wayland-protocols-misc crate ships it).
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    unused_imports
)]
#[allow(missing_docs, clippy::all)]
mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::backend as wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("tests/support/virtual-keyboard-unstable-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("tests/support/virtual-keyboard-unstable-v1.xml");
}

/// `wl_keyboard.keymap_format.xkb_v1`.
const XKB_V1: u32 = 1;
/// The evdev code of the left Shift key: a key that types nothing.
const KEY_LEFTSHIFT: u32 = 42;
/// A US layout, compiled by the compositor from the system's xkeyboard-config.
const KEYMAP: &str = "xkb_keymap {
    xkb_keycodes { include \"evdev+aliases(qwerty)\" };
    xkb_types { include \"complete\" };
    xkb_compat { include \"complete\" };
    xkb_symbols { include \"pc+us+inet(evdev)\" };
};";

/// The keyboard; the seat loses it when this is dropped.
pub struct VirtualKeyboard {
    _conn: Connection,
    queue: Mutex<EventQueue<Quiet>>,
    keyboard: ZwpVirtualKeyboardV1,
}

impl VirtualKeyboard {
    /// Adds a keyboard to the compositor's seat and waits until the compositor has it.
    pub fn new() -> VirtualKeyboard {
        crate::wayland_other::compositor();
        let conn = Connection::connect_to_env().expect("connect to the test compositor");
        let (globals, mut queue) = registry_queue_init::<Quiet>(&conn).unwrap();
        let qh = queue.handle();
        let seat: WlSeat = globals.bind(&qh, 1..=1, ()).expect("no seat");
        let manager: ZwpVirtualKeyboardManagerV1 = globals
            .bind(&qh, 1..=1, ())
            .expect("the compositor offers no virtual keyboard");
        let keyboard = manager.create_virtual_keyboard(&seat, &qh, ());
        // A key needs a keymap first. The compositor maps the file and reads it up to the NUL.
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap();
        let path = Path::new(&runtime).join("redactor-test-keymap");
        let mut text = KEYMAP.as_bytes().to_vec();
        text.push(0);
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(&text))
            .expect("write the keymap");
        let file = std::fs::File::open(&path).expect("open the keymap");
        keyboard.keymap(XKB_V1, file.as_fd(), text.len() as u32);
        queue
            .roundtrip(&mut Quiet)
            .expect("the compositor did not take the keyboard");
        VirtualKeyboard {
            _conn: conn,
            queue: Mutex::new(queue),
            keyboard,
        }
    }

    /// Presses and releases Shift, and waits until the compositor has passed both on to the
    /// window with the focus. Each comes with a serial newer than anything before.
    pub fn tap(&self) {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u32;
        self.keyboard.key(ms, KEY_LEFTSHIFT, 1);
        self.keyboard.key(ms.wrapping_add(1), KEY_LEFTSHIFT, 0);
        self.queue
            .lock()
            .unwrap()
            .roundtrip(&mut Quiet)
            .expect("the compositor did not take the key");
    }
}

/// None of these objects sends an event the keyboard needs.
struct Quiet;

impl Dispatch<WlRegistry, GlobalListContents> for Quiet {
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

impl Dispatch<WlSeat, ()> for Quiet {
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

impl Dispatch<ZwpVirtualKeyboardManagerV1, ()> for Quiet {
    fn event(
        _: &mut Self,
        _: &ZwpVirtualKeyboardManagerV1,
        _: <ZwpVirtualKeyboardManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardV1, ()> for Quiet {
    fn event(
        _: &mut Self,
        _: &ZwpVirtualKeyboardV1,
        _: <ZwpVirtualKeyboardV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
