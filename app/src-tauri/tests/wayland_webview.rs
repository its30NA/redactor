//! The Wayland variant of `linux_webview.rs`: a web view in a native Wayland window, its
//! WebKitGTK settings hardened by `harden_settings` and read back.
//!
//! GTK needs a compositor, and this one must be the throwaway compositor that
//! `tools/ci/wayland-tests.sh` started: the test is `#[ignore]`d and panics unless
//! `REDACTOR_TEST_WAYLAND` names it (`support/wayland_session.rs`).
#![cfg(target_os = "linux")]

#[path = "support/wayland_session.rs"]
mod wayland_session;
#[path = "support/webkit_settings.rs"]
mod webkit_settings;

use std::time::{Duration, Instant};

use gtk::prelude::*;
use redactor_app::platform::linux::webview::harden_settings;
use webkit2gtk::{SettingsExt, WebView, WebViewExt};

/// How long the window gets to appear.
const MAP_DEADLINE: Duration = Duration::from_secs(5);

#[test]
#[ignore = "needs the throwaway compositor: run tools/ci/wayland-tests.sh"]
fn wayland_webview_hardening() {
    wayland_session::compositor();
    // As the app does on Wayland: GDK may use nothing else.
    gtk::gdk::set_allowed_backends("wayland");
    gtk::init().expect("GTK on the test compositor");
    let display = gtk::gdk::Display::default().expect("GTK opened no display");
    assert_eq!(display.type_().name(), "GdkWaylandDisplay");

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(200, 200);
    let view = WebView::new();
    window.add(&view);
    window.show_all();
    let deadline = Instant::now() + MAP_DEADLINE;
    while !window.is_mapped() {
        assert!(
            Instant::now() < deadline,
            "the window did not appear within 5 s"
        );
        gtk::main_iteration_do(false);
        std::thread::sleep(Duration::from_millis(10));
    }
    let surface = window.window().expect("a mapped window has a GDK window");
    assert_eq!(surface.type_().name(), "GdkWaylandWindow");

    let s = WebViewExt::settings(&view).expect("the view has settings");
    // A default this test relies on: without hardening, the page cache is on.
    assert!(s.enables_page_cache());

    harden_settings(&s);

    webkit_settings::assert_hardened(&s);
}
