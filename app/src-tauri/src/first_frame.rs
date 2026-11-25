//! The colour the window shows before the page has painted its first frame.
//!
//! Without it the window flashes white at start-up, before the page's own background (the
//! `--bg` token in `app/ui/src/styles/tokens.css`) is drawn. `Dark` and `Light` start in their
//! own colour. `System` follows the desktop's light or dark preference as the window reports it
//! once built (Windows' app mode, macOS' appearance, GTK's dark-theme setting), the same
//! preference the page's `prefers-color-scheme` follows; where the desktop does not say, it starts
//! dark.

use crate::settings::Theme;

/// The desktop's light or dark preference, as the window reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopTheme {
    Light,
    Dark,
}

impl DesktopTheme {
    /// Tauri's report of the window's theme; `None` for a theme it may add later.
    pub fn from_tauri(theme: tauri::Theme) -> Option<Self> {
        match theme {
            tauri::Theme::Light => Some(Self::Light),
            tauri::Theme::Dark => Some(Self::Dark),
            _ => None,
        }
    }
}

/// The page background (`--bg`) of the theme the window starts in, as red, green, blue.
/// `desktop` is the desktop's preference where it is known.
pub fn first_frame_rgb(theme: Theme, desktop: Option<DesktopTheme>) -> (u8, u8, u8) {
    const LIGHT: (u8, u8, u8) = (0xf6, 0xf7, 0xfb);
    const DARK: (u8, u8, u8) = (0x07, 0x0a, 0x12);
    match (theme, desktop) {
        (Theme::Light, _) | (Theme::System, Some(DesktopTheme::Light)) => LIGHT,
        (Theme::Dark, _) | (Theme::System, Some(DesktopTheme::Dark) | None) => DARK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DARK: (u8, u8, u8) = (0x07, 0x0a, 0x12);
    const LIGHT: (u8, u8, u8) = (0xf6, 0xf7, 0xfb);

    #[test]
    fn dark_starts_dark_whatever_the_desktop() {
        for desktop in [None, Some(DesktopTheme::Dark), Some(DesktopTheme::Light)] {
            assert_eq!(first_frame_rgb(Theme::Dark, desktop), DARK, "{desktop:?}");
        }
    }

    #[test]
    fn light_starts_light_whatever_the_desktop() {
        for desktop in [None, Some(DesktopTheme::Dark), Some(DesktopTheme::Light)] {
            assert_eq!(first_frame_rgb(Theme::Light, desktop), LIGHT, "{desktop:?}");
        }
    }

    #[test]
    fn system_follows_the_desktop() {
        assert_eq!(
            first_frame_rgb(Theme::System, Some(DesktopTheme::Light)),
            LIGHT
        );
        assert_eq!(
            first_frame_rgb(Theme::System, Some(DesktopTheme::Dark)),
            DARK
        );
    }

    #[test]
    fn system_starts_dark_when_the_desktop_does_not_say() {
        assert_eq!(first_frame_rgb(Theme::System, None), DARK);
    }

    /// `#rrggbb` as three bytes.
    fn hex(colour: &str) -> (u8, u8, u8) {
        let colour = colour.trim().strip_prefix('#').expect("a #rrggbb colour");
        assert_eq!(colour.len(), 6, "{colour}");
        let byte = |i: usize| u8::from_str_radix(&colour[i..i + 2], 16).expect("hex digits");
        (byte(0), byte(2), byte(4))
    }

    #[test]
    fn colours_match_the_ui_tokens() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/styles/tokens.css");
        let css = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        // The page background: `--bg: light-dark(<light>, <dark>);`
        let line = css
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with("--bg:"))
            .expect("tokens.css defines --bg (renamed? update first_frame.rs with it)");
        let pair = line
            .strip_prefix("--bg:")
            .and_then(|v| v.trim().strip_prefix("light-dark("))
            .and_then(|v| v.trim_end_matches(';').trim().strip_suffix(')'))
            .unwrap_or_else(|| panic!("--bg is no longer light-dark(<light>, <dark>): {line}"));
        let (light, dark) = pair.split_once(',').expect("two colours");
        assert_eq!(
            first_frame_rgb(Theme::Light, None),
            hex(light),
            "light: {line}"
        );
        assert_eq!(
            first_frame_rgb(Theme::Dark, None),
            hex(dark),
            "dark: {line}"
        );
    }
}
