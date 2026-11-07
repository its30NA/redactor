//! The command-line switches the WebView2 browser process starts with.
//!
//! The content security policy limits what the page may fetch; these switches cover what the
//! policy does not govern. Setting them replaces the defaults wry (Tauri's WebView layer)
//! passes otherwise, so those are repeated here first.
//!
//! - wry's defaults (wry 0.57, `webview2/mod.rs`): `--disable-features=` the Edge mini menu
//!   (`msWebOOUI`, `msPdfOOUI`) and SmartScreen (`msSmartScreenProtection`). Autoplay and a
//!   proxy, which wry would add, are not configured by this app.
//! - `--force-webrtc-ip-handling-policy=disable_non_proxied_udp`: WebRTC may not open UDP
//!   connections of its own (the UI also removes the WebRTC constructors at start-up).
//! - `--dns-prefetch-disable`: no DNS lookups ahead of navigation.
//! - `--disable-background-networking`: no background requests by the browser itself
//!   (component and variations updates, network prediction).
//!
//! Chromium honours only the last `--disable-features`, so all features go in one switch.

/// Passed to `WebviewWindowBuilder::additional_browser_args`.
pub const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
--force-webrtc-ip-handling-policy=disable_non_proxied_udp \
--dns-prefetch-disable \
--disable-background-networking";
