//! The web view on macOS: built from our own configuration, then hardened.
//!
//! Two things can only be decided before a `WKWebView` exists: where it keeps web data, and
//! whether Writing Tools may read the page. Tauri offers no setting for the second, so the
//! window gets a configuration made here. A view built from a configuration it is handed
//! keeps that configuration's data store (Tauri's `incognito` is then not looked at), so this
//! one names the store that keeps nothing on disk itself.
//!
//! Everything here runs on the main thread: WebKit's classes live there.

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{msg_send, sel, MainThreadMarker};
use objc2_web_kit::{WKPreferences, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore};
use tauri::{AppHandle, TitleBarStyle, WebviewWindow, WebviewWindowBuilder, Wry};

use crate::platform::WebviewHardening;

/// `NSWritingToolsBehaviorNone`: Writing Tools neither read nor rewrite the page's text.
const WRITING_TOOLS_NONE: isize = -1;

/// The configuration the app's web view is built from: no web data on disk, no Writing
/// Tools, no inline predictions, and the preferences of `harden_preferences`.
pub fn configuration(mtm: MainThreadMarker) -> Retained<WKWebViewConfiguration> {
    // SAFETY: `mtm` proves the main thread. The setters get live objects of the types they
    // name. The two selectors newer than macOS 13 (Writing Tools: 15, inline predictions:
    // 14) are sent only where the configuration answers them, and `setWritingToolsBehavior:`
    // takes an NSInteger.
    unsafe {
        let config = WKWebViewConfiguration::new(mtm);
        config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm));
        harden_preferences(&config.preferences());
        if config.respondsToSelector(sel!(setWritingToolsBehavior:)) {
            let _: () = msg_send![&*config, setWritingToolsBehavior: WRITING_TOOLS_NONE];
        }
        if config.respondsToSelector(sel!(setAllowsInlinePredictions:)) {
            config.setAllowsInlinePredictions(false);
        }
        config
    }
}

/// Scripts open no windows, no address is sent to a Safe Browsing look-up, and no element
/// goes fullscreen.
fn harden_preferences(preferences: &WKPreferences) {
    // SAFETY: plain setters, all older than macOS 13; `WKPreferences` is main-thread-only,
    // so holding one means this is the main thread.
    unsafe {
        preferences.setJavaScriptCanOpenWindowsAutomatically(false);
        preferences.setFraudulentWebsiteWarningEnabled(false);
        preferences.setElementFullscreenEnabled(false);
    }
}

/// What can be set on the built view: no back and forward swipes, no pinch zoom, no link
/// previews, and the preferences once more (the builder switches element fullscreen on).
pub fn harden_view(view: &WKWebView) {
    // SAFETY: plain setters and getters, all older than macOS 13; `WKWebView` is
    // main-thread-only, so holding one means this is the main thread.
    unsafe {
        view.setAllowsBackForwardNavigationGestures(false);
        view.setAllowsMagnification(false);
        view.setAllowsLinkPreview(false);
        harden_preferences(&view.configuration().preferences());
    }
}

/// The macOS web view: our configuration before it exists, `harden_view` after.
pub struct WkHardening;

impl WebviewHardening for WkHardening {
    /// The window gets the system's own title bar buttons over our title bar, and the web
    /// view our configuration. The window is built in `setup`, on the main thread; anywhere
    /// else the configuration cannot be made and the builder keeps Tauri's own.
    fn prepare<'a>(
        &self,
        builder: WebviewWindowBuilder<'a, Wry, AppHandle>,
    ) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
        let builder = builder
            .decorations(true)
            .title_bar_style(TitleBarStyle::Overlay)
            .hidden_title(true);
        match MainThreadMarker::new() {
            Some(mtm) => builder.with_webview_configuration(configuration(mtm)),
            None => builder,
        }
    }

    fn harden(&self, window: &WebviewWindow) {
        let _ = window.with_webview(|webview| {
            // SAFETY: Tauri runs this on the main thread and hands over the live
            // `WKWebView`, which it keeps alive for the length of the call.
            let view: &WKWebView = unsafe { &*webview.inner().cast() };
            harden_view(view);
        });
    }
}
