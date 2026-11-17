//! The WebKitGTK settings `harden_settings` turns off, read back from a real `WebKitSettings`.
//!
//! WebKitGTK builds no `Settings` before GTK is initialised, and GTK needs a display. So this
//! test is `#[ignore]`d and panics unless `REDACTOR_TEST_DISPLAY` names a throwaway display and
//! `DISPLAY` points at it too. Only `tools/ci/x11-tests.sh` sets both, on its own Xvfb.
#![cfg(target_os = "linux")]

#[path = "support/webkit_settings.rs"]
mod webkit_settings;

use redactor_app::platform::linux::webview::harden_settings;
use webkit2gtk::{Settings, SettingsExt};

/// Initialises GTK on the test display, never on the desktop this runs from.
fn init_gtk() {
    let display = std::env::var("REDACTOR_TEST_DISPLAY")
        .expect("REDACTOR_TEST_DISPLAY is not set: run tools/ci/x11-tests.sh");
    assert_eq!(
        std::env::var("DISPLAY").ok().as_deref(),
        Some(display.as_str()),
        "DISPLAY must name the test display"
    );
    gtk::init().expect("GTK on the test display");
}

#[test]
#[ignore = "needs a throwaway X display: run tools/ci/x11-tests.sh"]
fn settings_are_hardened() {
    init_gtk();
    let s = Settings::new();
    // A default this test relies on: without hardening, the page cache is on.
    assert!(s.enables_page_cache());

    harden_settings(&s);

    webkit_settings::assert_hardened(&s);
}
