//! A password manager's copy on the throwaway compositor, for the Wayland smoke
//! (`tools/ci/wayland-smoke.sh`, check W3). A development tool: it is not part of the app.
//!
//! `wayland_offer <file>` copies the text of the file the way KeePassXC and Bitwarden do: under
//! every text type plus `x-kde-passwordManagerHint` = `secret` (`wl-copy` offers one type
//! only). It prints `ready` once the copy is the selection and keeps it until it is stopped.
//!
//! The copy comes from the other app of the Wayland desktop tests (`tests/support`), so it
//! runs only inside `tools/ci/wayland-tests.sh --run`: never on a real desktop (on WSLg the
//! inherited `WAYLAND_DISPLAY` is the Windows clipboard).

#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "../tests/support/clipboard_contract.rs"]
mod contract;
#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "../tests/support/wayland_other.rs"]
mod wayland_other;
#[cfg(target_os = "linux")]
#[path = "../tests/support/wayland_session.rs"]
mod wayland_session;

#[cfg(target_os = "linux")]
fn main() {
    use contract::OtherApp;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [file] = args.as_slice() else {
        eprintln!("usage: wayland_offer <file>");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(file).expect("read the text to copy");
    let other = wayland_other::WaylandOther::new();
    other.copy_protected(&text);
    println!("ready");
    // The copy lives as long as this process and its connection.
    loop {
        std::thread::park();
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
