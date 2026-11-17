//! A keyboard for the throwaway compositor, for the Wayland smoke (`tools/ci/wayland-smoke.sh`,
//! checks W4 and W5). A development tool: it is not part of the app.
//!
//! Headless sway has no input devices, and without a keyboard no window ever gets focus, so
//! the clipboard that works only while the window is in front never works there.
//! `wayland_keyboard` gives the seat the virtual keyboard of the Wayland desktop tests
//! (`tests/support/virtual_keyboard.rs`), prints `ready`, and keeps it until it is stopped.
//!
//! It runs only inside `tools/ci/wayland-tests.sh --run`: never on a real desktop.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "../tests/support/virtual_keyboard.rs"]
mod virtual_keyboard;
#[cfg(target_os = "linux")]
#[path = "../tests/support/wayland_session.rs"]
mod wayland_session;

#[cfg(target_os = "linux")]
fn main() {
    if std::env::args().len() != 1 {
        eprintln!("usage: wayland_keyboard");
        std::process::exit(2);
    }
    let _keyboard = virtual_keyboard::VirtualKeyboard::new();
    println!("ready");
    // The seat keeps the keyboard as long as this process and its connection.
    loop {
        std::thread::park();
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
