//! The global hotkey as the settings store it ("Ctrl+Alt+R"), parsed and checked.
//!
//! The UI checks the same rules (`app/ui/src/lib/keys.ts`); the shell checks again because the
//! settings file and IPC are inputs too.

use std::fmt;

use crate::os::Os;

/// The default hotkey.
pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+R";

/// The key part of a hotkey.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// `A`..`Z`.
    Letter(char),
    /// `0`..`9`.
    Digit(u8),
    /// `F1`..`F24`.
    F(u8),
    Space,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Enter,
    Tab,
}

const NAMED: &[(&str, Key)] = &[
    ("Space", Key::Space),
    ("Insert", Key::Insert),
    ("Delete", Key::Delete),
    ("Home", Key::Home),
    ("End", Key::End),
    ("PageUp", Key::PageUp),
    ("PageDown", Key::PageDown),
    ("Backspace", Key::Backspace),
    ("Enter", Key::Enter),
    ("Tab", Key::Tab),
];

/// A parsed, valid global hotkey. `meta` is Cmd on macOS and Super on Linux; on Windows it is
/// never set (the Windows key is the system's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
    pub key: Key,
}

/// Why a hotkey string cannot be used. The messages are shown to the user as they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyError {
    Unreadable,
    TooFewModifiers(Os),
    Reserved,
    ReservedBySystem(Os),
    UsedInApp,
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HotkeyError::Unreadable => {
                "That hotkey cannot be read. Use modifiers and one key, like Ctrl+Alt+R."
            }
            HotkeyError::TooFewModifiers(Os::Windows) => {
                "Use at least two modifier keys (Ctrl, Alt, Shift), so it never fires while typing."
            }
            HotkeyError::TooFewModifiers(Os::Mac) => {
                "Use at least two modifier keys (Cmd, Control, Option, Shift), so it never fires while typing."
            }
            HotkeyError::TooFewModifiers(Os::Linux) => {
                "Use at least two modifier keys (Ctrl, Alt, Shift, Super), so it never fires while typing."
            }
            HotkeyError::Reserved => "This shortcut is reserved.",
            HotkeyError::ReservedBySystem(Os::Windows) => "Windows keeps this one.",
            HotkeyError::ReservedBySystem(Os::Mac) => "macOS keeps this one.",
            HotkeyError::ReservedBySystem(Os::Linux) => "Your desktop keeps this one.",
            HotkeyError::UsedInApp => "Redactor already uses this inside its window.",
        })
    }
}

impl std::error::Error for HotkeyError {}

/// The app's in-window shortcuts; a global hotkey must not shadow them.
const IN_APP: &[&str] = &[
    "Ctrl+V",
    "Ctrl+Shift+C",
    "Ctrl+K",
    "Ctrl+O",
    "Ctrl+S",
    "Ctrl+Shift+X",
    "Ctrl+1",
    "Ctrl+2",
    "Ctrl+3",
];

/// The same shortcuts on a Mac, where they use Cmd.
const IN_APP_MAC: &[&str] = &[
    "Cmd+V",
    "Shift+Cmd+C",
    "Cmd+K",
    "Cmd+O",
    "Cmd+S",
    "Shift+Cmd+X",
    "Cmd+1",
    "Cmd+2",
    "Cmd+3",
];

/// Shortcuts kept free for other local tools, so the global hotkey never takes them.
const RESERVED: &[&str] = &["Ctrl+Alt+Space", "Ctrl+Alt+A"];

/// Windows' own secure attention and task manager keys.
const SYSTEM_WINDOWS: &[&str] = &["Ctrl+Alt+Delete", "Ctrl+Shift+Escape"];

/// macOS screenshots, quit and log out, lock, emoji, full screen, force quit.
const SYSTEM_MAC: &[&str] = &[
    "Shift+Cmd+3",
    "Shift+Cmd+4",
    "Shift+Cmd+5",
    "Shift+Cmd+Q",
    "Ctrl+Cmd+Q",
    "Ctrl+Cmd+Space",
    "Ctrl+Cmd+F",
    "Alt+Cmd+Escape",
];

/// Linux desktops: log out, terminal, X server kill, virtual consoles.
const SYSTEM_LINUX: &[&str] = &[
    "Ctrl+Alt+Delete",
    "Ctrl+Alt+T",
    "Ctrl+Alt+Backspace",
    "Ctrl+Alt+F1",
    "Ctrl+Alt+F2",
    "Ctrl+Alt+F3",
    "Ctrl+Alt+F4",
    "Ctrl+Alt+F5",
    "Ctrl+Alt+F6",
    "Ctrl+Alt+F7",
    "Ctrl+Alt+F8",
    "Ctrl+Alt+F9",
    "Ctrl+Alt+F10",
    "Ctrl+Alt+F11",
    "Ctrl+Alt+F12",
];

/// How the meta modifier is written: Cmd on macOS, Super on Linux. Windows never reads one.
fn meta_name(os: Os) -> &'static str {
    match os {
        Os::Mac => "Cmd",
        Os::Linux | Os::Windows => "Super",
    }
}

impl Hotkey {
    /// Parses and checks a hotkey for `os`. Tokens are case-insensitive; `Control` is accepted
    /// for `Ctrl`; on macOS `Command` for `Cmd` and `Option`/`Opt` for `Alt`; on Linux `Meta`
    /// for `Super`.
    pub fn parse(s: &str, os: Os) -> Result<Hotkey, HotkeyError> {
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let (key_part, mods) = parts.split_last().ok_or(HotkeyError::Unreadable)?;
        let (mut ctrl, mut shift, mut alt, mut meta) = (false, false, false, false);
        for m in mods {
            let flag = match (m.to_ascii_lowercase().as_str(), os) {
                ("ctrl" | "control", _) => &mut ctrl,
                ("shift", _) => &mut shift,
                ("alt", _) | ("option" | "opt", Os::Mac) => &mut alt,
                ("cmd" | "command", Os::Mac) | ("super" | "meta", Os::Linux) => &mut meta,
                _ => return Err(HotkeyError::Unreadable),
            };
            if *flag {
                return Err(HotkeyError::Unreadable);
            }
            *flag = true;
        }
        // Check the system's combinations by name first, before the key is restricted to the
        // supported set (Escape is not a supported key, but Ctrl+Shift+Escape is still Windows').
        let canonical = canonical_mods(ctrl, shift, alt, meta.then(|| meta_name(os)), key_part);
        let (system, in_app) = match os {
            Os::Windows => (SYSTEM_WINDOWS, IN_APP),
            Os::Mac => (SYSTEM_MAC, IN_APP_MAC),
            Os::Linux => (SYSTEM_LINUX, IN_APP),
        };
        if system.iter().any(|w| w.eq_ignore_ascii_case(&canonical)) {
            return Err(HotkeyError::ReservedBySystem(os));
        }
        let key = parse_key(key_part).ok_or(HotkeyError::Unreadable)?;
        let hk = Hotkey {
            ctrl,
            shift,
            alt,
            meta,
            key,
        };
        let name = hk.name(os);
        if RESERVED.iter().any(|v| *v == name) {
            return Err(HotkeyError::Reserved);
        }
        if in_app.iter().any(|v| *v == name) {
            return Err(HotkeyError::UsedInApp);
        }
        if [ctrl, shift, alt, meta].iter().filter(|b| **b).count() < 2 {
            return Err(HotkeyError::TooFewModifiers(os));
        }
        Ok(hk)
    }

    /// The canonical form, in the UI's order: `Ctrl+Shift+Alt+<Cmd|Super>+Key`.
    pub fn name(&self, os: Os) -> String {
        canonical_mods(
            self.ctrl,
            self.shift,
            self.alt,
            self.meta.then(|| meta_name(os)),
            &self.key.to_string(),
        )
    }
}

/// Swaps the registered global hotkey from `old` to `new`: unregisters `old` (if any), then
/// registers `new`. When the OS or another app refuses `new`, `old` is registered again and the
/// result is false, so the user keeps a working hotkey.
pub fn replace_registration(
    old: Option<&Hotkey>,
    new: &Hotkey,
    mut register: impl FnMut(&Hotkey) -> bool,
    mut unregister: impl FnMut(&Hotkey),
) -> bool {
    if let Some(old) = old {
        unregister(old);
    }
    if register(new) {
        return true;
    }
    if let Some(old) = old {
        let _ = register(old);
    }
    false
}

fn canonical_mods(ctrl: bool, shift: bool, alt: bool, meta: Option<&str>, key: &str) -> String {
    let mut s = String::new();
    for (on, name) in [(ctrl, "Ctrl"), (shift, "Shift"), (alt, "Alt")] {
        if on {
            s.push_str(name);
            s.push('+');
        }
    }
    if let Some(m) = meta {
        s.push_str(m);
        s.push('+');
    }
    s.push_str(key);
    s
}

fn parse_key(k: &str) -> Option<Key> {
    let b = k.as_bytes();
    if b.len() == 1 {
        let c = b[0] as char;
        return match c {
            'a'..='z' | 'A'..='Z' => Some(Key::Letter(c.to_ascii_uppercase())),
            '0'..='9' => Some(Key::Digit(b[0] - b'0')),
            _ => None,
        };
    }
    if let Some(digits) = k.strip_prefix(['F', 'f']) {
        // Plain digits only: no "F01", no sign.
        if !digits.starts_with('0') && digits.bytes().all(|d| d.is_ascii_digit()) {
            if let Ok(n) = digits.parse::<u8>() {
                if (1..=24).contains(&n) {
                    return Some(Key::F(n));
                }
            }
        }
    }
    NAMED
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(k))
        .map(|(_, key)| *key)
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Letter(c) => write!(f, "{c}"),
            Key::Digit(d) => write!(f, "{d}"),
            Key::F(n) => write!(f, "F{n}"),
            other => {
                let name = NAMED
                    .iter()
                    .find(|(_, k)| k == other)
                    .map(|(n, _)| *n)
                    .unwrap_or("?");
                f.write_str(name)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_parses_and_round_trips() {
        let hk = Hotkey::parse(DEFAULT_HOTKEY, Os::Windows).unwrap();
        assert_eq!(
            hk,
            Hotkey {
                ctrl: true,
                shift: false,
                alt: true,
                meta: false,
                key: Key::Letter('R')
            }
        );
        assert_eq!(hk.name(Os::Windows), DEFAULT_HOTKEY);
    }

    #[test]
    fn canonical_order_and_case() {
        let hk = Hotkey::parse("alt+control+shift+f9", Os::Windows).unwrap();
        assert_eq!(hk.name(Os::Windows), "Ctrl+Shift+Alt+F9");
        assert_eq!(
            Hotkey::parse("Ctrl+Alt+pageup", Os::Windows)
                .unwrap()
                .name(Os::Windows),
            "Ctrl+Alt+PageUp"
        );
    }

    #[test]
    fn hotkey_needs_two_modifiers() {
        for s in ["R", "Ctrl+R", "Alt+F4", "Shift+Space"] {
            assert_eq!(
                Hotkey::parse(s, Os::Windows),
                Err(HotkeyError::TooFewModifiers(Os::Windows)),
                "{s}"
            );
        }
    }

    #[test]
    fn hotkey_refuses_reserved_and_windows_keys() {
        assert_eq!(
            Hotkey::parse("Ctrl+Alt+Space", Os::Windows),
            Err(HotkeyError::Reserved)
        );
        assert_eq!(
            Hotkey::parse("Alt+Ctrl+a", Os::Windows),
            Err(HotkeyError::Reserved)
        );
        assert_eq!(
            Hotkey::parse("Ctrl+Alt+Delete", Os::Windows),
            Err(HotkeyError::ReservedBySystem(Os::Windows))
        );
        assert_eq!(
            Hotkey::parse("Ctrl+Shift+Escape", Os::Windows),
            Err(HotkeyError::ReservedBySystem(Os::Windows))
        );
    }

    #[test]
    fn hotkey_refuses_in_app_shortcuts() {
        for s in ["Ctrl+Shift+C", "Ctrl+Shift+X", "ctrl+shift+x"] {
            assert_eq!(
                Hotkey::parse(s, Os::Windows),
                Err(HotkeyError::UsedInApp),
                "{s}"
            );
        }
    }

    #[test]
    fn hotkey_refuses_garbage() {
        for s in [
            "",
            "Ctrl+Alt+",
            "Ctrl+Ctrl+R",
            "Ctrl+Alt+Win",
            "Ctrl+Alt+F0",
            "Ctrl+Alt+F25",
            "Ctrl+Alt+F01",
            "Meta+Alt+R",
            "Ctrl+Alt+RR",
            "Ctrl+Alt+ä",
        ] {
            assert_eq!(
                Hotkey::parse(s, Os::Windows),
                Err(HotkeyError::Unreadable),
                "{s:?}"
            );
        }
    }
    fn hk(s: &str) -> Hotkey {
        Hotkey::parse(s, Os::Windows).unwrap()
    }

    /// A fake OS: `held` is what we registered, `taken` belongs to another app.
    fn swap(held: &mut Vec<Hotkey>, taken: &[Hotkey], old: Option<&Hotkey>, new: &Hotkey) -> bool {
        let held = std::cell::RefCell::new(held);
        replace_registration(
            old,
            new,
            |h| {
                if taken.contains(h) {
                    false
                } else {
                    held.borrow_mut().push(*h);
                    true
                }
            },
            |h| held.borrow_mut().retain(|x| x != h),
        )
    }

    #[test]
    fn replacing_registers_the_new_key_and_drops_the_old() {
        let (old, new) = (hk("Ctrl+Alt+R"), hk("Ctrl+Alt+K"));
        let mut held = vec![old];
        assert!(swap(&mut held, &[], Some(&old), &new));
        assert_eq!(held, [new]);
        let mut none = vec![];
        assert!(swap(&mut none, &[], None, &new));
        assert_eq!(none, [new]);
    }

    #[test]
    fn a_refused_key_keeps_the_old_one_registered() {
        let (old, new) = (hk("Ctrl+Alt+R"), hk("Ctrl+Alt+K"));
        let mut held = vec![old];
        assert!(!swap(&mut held, &[new], Some(&old), &new));
        assert_eq!(held, [old]);
    }
}
