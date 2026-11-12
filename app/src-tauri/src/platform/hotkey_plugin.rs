//! The global hotkey through `tauri-plugin-global-shortcut` (Windows, macOS, Linux X11).

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

use super::{GlobalHotkey, HotkeyTaken};
use crate::hotkey::{Hotkey, Key};

/// Registers hotkeys with Tauri's global-shortcut plugin.
pub struct PluginHotkey;

impl GlobalHotkey for PluginHotkey {
    fn register(&self, app: &AppHandle, hk: &Hotkey) -> Result<(), HotkeyTaken> {
        app.global_shortcut()
            .register(shortcut(hk))
            .map_err(|_| HotkeyTaken)
    }

    fn unregister(&self, app: &AppHandle, hk: &Hotkey) {
        let _ = app.global_shortcut().unregister(shortcut(hk));
    }
}

pub fn shortcut(hk: &Hotkey) -> Shortcut {
    let mut mods = Modifiers::empty();
    if hk.ctrl {
        mods |= Modifiers::CONTROL;
    }
    if hk.shift {
        mods |= Modifiers::SHIFT;
    }
    if hk.alt {
        mods |= Modifiers::ALT;
    }
    if hk.meta {
        mods |= Modifiers::SUPER;
    }
    Shortcut::new(Some(mods), code(hk.key))
}

fn code(k: Key) -> Code {
    const LETTERS: [Code; 26] = [
        Code::KeyA,
        Code::KeyB,
        Code::KeyC,
        Code::KeyD,
        Code::KeyE,
        Code::KeyF,
        Code::KeyG,
        Code::KeyH,
        Code::KeyI,
        Code::KeyJ,
        Code::KeyK,
        Code::KeyL,
        Code::KeyM,
        Code::KeyN,
        Code::KeyO,
        Code::KeyP,
        Code::KeyQ,
        Code::KeyR,
        Code::KeyS,
        Code::KeyT,
        Code::KeyU,
        Code::KeyV,
        Code::KeyW,
        Code::KeyX,
        Code::KeyY,
        Code::KeyZ,
    ];
    const DIGITS: [Code; 10] = [
        Code::Digit0,
        Code::Digit1,
        Code::Digit2,
        Code::Digit3,
        Code::Digit4,
        Code::Digit5,
        Code::Digit6,
        Code::Digit7,
        Code::Digit8,
        Code::Digit9,
    ];
    const FKEYS: [Code; 24] = [
        Code::F1,
        Code::F2,
        Code::F3,
        Code::F4,
        Code::F5,
        Code::F6,
        Code::F7,
        Code::F8,
        Code::F9,
        Code::F10,
        Code::F11,
        Code::F12,
        Code::F13,
        Code::F14,
        Code::F15,
        Code::F16,
        Code::F17,
        Code::F18,
        Code::F19,
        Code::F20,
        Code::F21,
        Code::F22,
        Code::F23,
        Code::F24,
    ];
    match k {
        Key::Letter(c) => LETTERS[(c as u8 - b'A') as usize],
        Key::Digit(d) => DIGITS[d as usize],
        Key::F(n) => FKEYS[(n - 1) as usize],
        Key::Space => Code::Space,
        Key::Insert => Code::Insert,
        Key::Delete => Code::Delete,
        Key::Home => Code::Home,
        Key::End => Code::End,
        Key::PageUp => Code::PageUp,
        Key::PageDown => Code::PageDown,
        Key::Backspace => Code::Backspace,
        Key::Enter => Code::Enter,
        Key::Tab => Code::Tab,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::Hotkey;
    use crate::os::Os;
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

    #[test]
    fn cmd_and_super_map_to_the_super_modifier() {
        for (combo, os) in [("Shift+Cmd+R", Os::Mac), ("Shift+Super+R", Os::Linux)] {
            let s = shortcut(&Hotkey::parse(combo, os).unwrap());
            assert_eq!(
                s,
                Shortcut::new(Some(Modifiers::SHIFT | Modifiers::SUPER), Code::KeyR),
                "{combo}"
            );
        }
    }

    #[test]
    fn modifiers_map_one_to_one() {
        let s = shortcut(&Hotkey::parse("Ctrl+Alt+R", Os::Windows).unwrap());
        assert_eq!(
            s,
            Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyR)
        );
        let s = shortcut(&Hotkey::parse("Ctrl+Shift+Alt+F9", Os::Windows).unwrap());
        assert_eq!(
            s,
            Shortcut::new(
                Some(Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::ALT),
                Code::F9
            )
        );
    }

    /// Every key the parser accepts maps to the W3C code of the same name.
    #[test]
    fn every_key_has_its_code() {
        let mut names: Vec<String> = ('A'..='Z').map(|c| c.to_string()).collect();
        names.extend((0..=9).map(|d| d.to_string()));
        names.extend((1..=24).map(|n| format!("F{n}")));
        names.extend(
            [
                "Space",
                "Insert",
                "Delete",
                "Home",
                "End",
                "PageUp",
                "PageDown",
                "Backspace",
                "Enter",
                "Tab",
            ]
            .map(String::from),
        );
        for name in names {
            let hk = Hotkey::parse(&format!("Ctrl+Alt+Shift+{name}"), Os::Windows).unwrap();
            let want = match name.len() {
                1 if name.as_bytes()[0].is_ascii_digit() => format!("Digit{name}"),
                1 => format!("Key{name}"),
                _ => name.clone(),
            };
            assert_eq!(shortcut(&hk).key.to_string(), want, "{name}");
        }
    }
}
