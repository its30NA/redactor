//! WebView2 settings Tauri does not expose: general autofill and password autosave
//! off, browser accelerator keys off, the default context menu off, no status bar, no swipe
//! navigation, devtools only in debug builds.

use tauri::WebviewWindow;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Settings3, ICoreWebView2Settings4, ICoreWebView2Settings6,
};
use windows::core::Interface;

pub fn harden(window: &WebviewWindow) {
    let _ = window.with_webview(|wv| {
        let controller = wv.controller();
        // SAFETY: COM calls on the live controller, on the thread Tauri runs this closure on
        // (the WebView's UI thread). Every result is checked; a missing interface (older
        // runtime) leaves that setting at its default.
        unsafe {
            let Ok(core) = controller.CoreWebView2() else {
                return;
            };
            let Ok(settings) = core.Settings() else {
                return;
            };
            let _ = settings.SetAreDefaultContextMenusEnabled(false);
            let _ = settings.SetIsStatusBarEnabled(false);
            let _ = settings.SetAreDevToolsEnabled(cfg!(debug_assertions));
            if let Ok(s3) = settings.cast::<ICoreWebView2Settings3>() {
                let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
            }
            if let Ok(s4) = settings.cast::<ICoreWebView2Settings4>() {
                let _ = s4.SetIsGeneralAutofillEnabled(false);
                let _ = s4.SetIsPasswordAutosaveEnabled(false);
            }
            if let Ok(s6) = settings.cast::<ICoreWebView2Settings6>() {
                let _ = s6.SetIsSwipeNavigationEnabled(false);
            }
        }
    });
}
