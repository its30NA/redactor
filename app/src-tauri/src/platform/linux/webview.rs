//! WebKitGTK settings Tauri does not expose: no DNS prefetching, WebRTC, camera or microphone,
//! hyperlink auditing (ping), page cache, WebGL, WebAudio, back/forward swipe, or windows opened
//! by script; developer extras only in debug builds; no default context menu (as on Windows).

use tauri::WebviewWindow;
use webkit2gtk::glib::{self, prelude::ObjectExt};
use webkit2gtk::{Settings, SettingsExt, WebViewExt};

use crate::platform::WebviewHardening;

/// Turns off what the app does not use. Every setter exists in WebKitGTK 2.40 and later.
pub fn harden_settings(s: &Settings) {
    s.set_enable_dns_prefetching(false);
    s.set_enable_webrtc(false);
    s.set_enable_media_stream(false);
    // Newer WebKitGTK (2.52 here) ignores this setting, marks it deprecated, and warns on
    // every call; there the app's CSP is what keeps `<a ping>` from reaching anything.
    if !retired(s, "enable-hyperlink-auditing") {
        s.set_enable_hyperlink_auditing(false);
    }
    s.set_enable_page_cache(false);
    s.set_enable_webgl(false);
    s.set_enable_webaudio(false);
    s.set_enable_back_forward_navigation_gestures(false);
    s.set_javascript_can_open_windows_automatically(false);
    s.set_enable_developer_extras(cfg!(debug_assertions));
}

/// True when this WebKitGTK marks the setting `name` deprecated (it then ignores it).
fn retired(s: &Settings, name: &str) -> bool {
    s.find_property(name)
        .is_none_or(|p| p.flags().contains(glib::ParamFlags::DEPRECATED))
}

/// The WebKitGTK half of the Linux platform.
pub struct WebKitHardening;

impl WebviewHardening for WebKitHardening {
    fn harden(&self, window: &WebviewWindow) {
        // Tauri runs the closure on the GTK main thread, where the view lives.
        let _ = window.with_webview(|wv| {
            let view = wv.inner();
            if let Some(settings) = WebViewExt::settings(&view) {
                harden_settings(&settings);
            }
            // `true` means handled: WebKit shows no menu of its own.
            view.connect_context_menu(|_, _, _, _| true);
        });
    }
}
