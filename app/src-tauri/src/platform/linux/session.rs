//! Which kind of desktop session this process runs in, from its environment.

use std::ffi::OsString;

/// The display server the session offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Session {
    X11,
    Wayland,
    None,
}

/// `WAYLAND_DISPLAY` set → Wayland (even with XWayland's `DISPLAY` beside it); else `DISPLAY`
/// set → X11; else no session. An empty value counts as unset. `var` reads one variable.
pub fn session(var: impl Fn(&str) -> Option<OsString>) -> Session {
    let set = |name: &str| var(name).is_some_and(|v| !v.is_empty());
    if set("WAYLAND_DISPLAY") {
        Session::Wayland
    } else if set("DISPLAY") {
        Session::X11
    } else {
        Session::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn wayland_wins_over_x11() {
        let vars = [("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")];
        assert_eq!(session(env(&vars)), Session::Wayland);
    }

    #[test]
    fn x11_when_only_display() {
        assert_eq!(session(env(&[("DISPLAY", ":0")])), Session::X11);
    }

    #[test]
    fn empty_values_count_as_unset() {
        let vars = [("WAYLAND_DISPLAY", ""), ("DISPLAY", ":1")];
        assert_eq!(session(env(&vars)), Session::X11);
        assert_eq!(session(env(&[("DISPLAY", "")])), Session::None);
    }

    #[test]
    fn nothing_set_is_none() {
        assert_eq!(session(env(&[])), Session::None);
    }
}
