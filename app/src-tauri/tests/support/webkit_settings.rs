//! What `harden_settings` must have turned off, checked on a real `WebKitSettings`: shared by
//! the X11 (`linux_webview.rs`) and the Wayland (`wayland_webview.rs`) desktop tests.
//!
//! The including test file must name this module `webkit_settings`.

use webkit2gtk::glib::{self, prelude::ObjectExt};
use webkit2gtk::{Settings, SettingsExt};

/// True when this WebKitGTK marks the setting `name` deprecated (it then ignores it).
fn retired(s: &Settings, name: &str) -> bool {
    s.find_property(name)
        .is_none_or(|p| p.flags().contains(glib::ParamFlags::DEPRECATED))
}

/// Panics, naming the setting, unless `s` is hardened.
pub fn assert_hardened(s: &Settings) {
    // Newer WebKitGTK (2.52 here) marks these two deprecated: it no longer prefetches DNS, and it
    // ignores the auditing setting. Where they still work, they are off.
    if !retired(s, "enable-dns-prefetching") {
        assert!(!s.enables_dns_prefetching(), "DNS prefetching");
    }
    if !retired(s, "enable-hyperlink-auditing") {
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
