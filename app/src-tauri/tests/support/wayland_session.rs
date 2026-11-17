//! The guard of every Wayland desktop test: the throwaway compositor `tools/ci/wayland-tests.sh`
//! starts, never the desktop the tests are run from.
//!
//! The including test file must name this module `wayland_session`.

use std::path::Path;

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
