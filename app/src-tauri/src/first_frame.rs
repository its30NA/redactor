//! The colour the window shows before the page has painted its first frame.
//!
//! Without it the window flashes white at start-up, before the page's own background (the
//! `--bg` token in `app/ui/src/styles/tokens.css`) is drawn. The UI is dark unless the user
//! picked the light theme, and `System` starts dark too: the page follows the desktop's theme
//! a moment later, and a dark flash on a light desktop is the smaller jump.

use crate::settings::Theme;

/// The page background (`--bg`) of the theme the window starts in, as red, green, blue.
pub fn first_frame_rgb(theme: Theme) -> (u8, u8, u8) {
    match theme {
        Theme::Light => (0xf6, 0xf7, 0xfb),
        Theme::Dark | Theme::System => (0x07, 0x0a, 0x12),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_and_dark_start_dark() {
        assert_eq!(first_frame_rgb(Theme::Dark), (0x07, 0x0a, 0x12));
        assert_eq!(first_frame_rgb(Theme::System), first_frame_rgb(Theme::Dark));
    }

    #[test]
    fn light_starts_light() {
        assert_eq!(first_frame_rgb(Theme::Light), (0xf6, 0xf7, 0xfb));
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
        assert_eq!(first_frame_rgb(Theme::Light), hex(light), "light: {line}");
        assert_eq!(first_frame_rgb(Theme::Dark), hex(dark), "dark: {line}");
    }
}
