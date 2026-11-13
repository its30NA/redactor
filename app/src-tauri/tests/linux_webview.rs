//! The WebKitGTK settings `harden_settings` turns off, read back from a real `WebKitSettings`.
//!
//! WebKitGTK builds no `Settings` before GTK is initialised, and GTK needs a display. So this
//! test is `#[ignore]`d and panics unless `REDACTOR_TEST_DISPLAY` names a throwaway display and
//! `DISPLAY` points at it too. Only `tools/ci/x11-tests.sh` sets both, on its own Xvfb.
#![cfg(target_os = "linux")]

use redactor_app::platform::linux::webview::harden_settings;
use webkit2gtk::glib::{self, prelude::ObjectExt};
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

/// True when this WebKitGTK marks the setting `name` deprecated (it then ignores it).
fn retired(s: &Settings, name: &str) -> bool {
    s.find_property(name)
        .is_none_or(|p| p.flags().contains(glib::ParamFlags::DEPRECATED))
}

#[test]
#[ignore = "needs a throwaway X display: run tools/ci/x11-tests.sh"]
fn settings_are_hardened() {
    init_gtk();
    let s = Settings::new();
    // A default this test relies on: without hardening, the page cache is on.
    assert!(s.enables_page_cache());

    harden_settings(&s);

    // Newer WebKitGTK (2.52 here) marks these two deprecated: it no longer prefetches DNS, and it
    // ignores the auditing setting. Where they still work, they are off.
    if !retired(&s, "enable-dns-prefetching") {
        assert!(!s.enables_dns_prefetching(), "DNS prefetching");
    }
    if !retired(&s, "enable-hyperlink-auditing") {
        assert!(!s.enables_hyperlink_auditing(), "hyperlink auditing");
    }
    assert!(!s.enables_webrtc(), "WebRTC");
    assert!(!s.enables_media_stream(), "media stream");
    assert!(!s.enables_page_cache(), "page cache");
    assert!(!s.enables_webgl(), "WebGL");
    assert!(!s.enables_webaudio(), "WebAudio");
    assert!(
        !s.enables_back_forward_navigation_gestures(),
        "back/forward gestures"
    );
    assert!(
        !s.is_javascript_can_open_windows_automatically(),
        "windows opened by script"
    );
    assert_eq!(
        s.enables_developer_extras(),
        cfg!(debug_assertions),
        "developer extras only in debug builds"
    );
}
