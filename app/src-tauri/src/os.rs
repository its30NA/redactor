//! Which operating system the rules are for: hotkey names and lists, the settings folder, the
//! UI's shortcuts. A plain value, so every rule is testable for every OS on any OS.

/// The desktop operating system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

impl Os {
    /// From the platform name the UI sees (`app_info.platform`): "windows", "macos", "linux".
    pub fn from_name(s: &str) -> Option<Os> {
        match s {
            "windows" => Some(Os::Windows),
            "macos" => Some(Os::Mac),
            "linux" => Some(Os::Linux),
            _ => None,
        }
    }

    /// True where the web view serves the app from `http://tauri.localhost` rather than the
    /// `tauri://` scheme: WebView2 on Windows. macOS and Linux use `tauri://localhost`.
    pub fn serves_app_over_http(self) -> bool {
        self == Os::Windows
    }
}
