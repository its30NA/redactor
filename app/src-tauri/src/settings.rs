//! App settings (`Settings`), validated and stored as `%APPDATA%\Redactor\settings.json`.
//!
//! The file holds settings only: never scanned text, never session secrets. The only free text is
//! `alwaysRedact`, the plain words the user chose to store.

use std::collections::HashSet;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

use redactor_core::{PlaceholderStyle, Sensitivity};
use serde::{Deserialize, Serialize};

use crate::hotkey::{Hotkey, DEFAULT_HOTKEY};

/// Always-redact words: at least 3 characters, at most 500 entries.
pub const MIN_ALWAYS_REDACT: usize = 3;
pub const MAX_ALWAYS_REDACT: usize = 500;
/// Longest single always-redact entry, in characters.
pub const MAX_ALWAYS_REDACT_LEN: usize = 200;
/// Room for spaces around an entry (trimmed away), in bytes.
const MAX_PADDING_BYTES: usize = 64;
/// The most bytes the whole always-redact list may take before it is looked at word by word.
pub const MAX_ALWAYS_REDACT_BYTES: usize =
    MAX_ALWAYS_REDACT * (MAX_ALWAYS_REDACT_LEN * 4 + MAX_PADDING_BYTES);
/// The settings file is small; anything bigger is not ours.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReducedMotion {
    #[default]
    System,
    On,
    Off,
}

/// Mirrors the UI's `Settings` (camelCase JSON).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: Theme,
    pub sensitivity: Sensitivity,
    pub pii: bool,
    pub strip_invisible: bool,
    pub hotkey: String,
    pub tray_mode: bool,
    /// 0 (off), 5, 10 or 30.
    pub auto_wipe_minutes: u32,
    pub wipe_on_lock: bool,
    pub content_protection: bool,
    pub placeholder_style: PlaceholderStyle,
    pub reduced_motion: ReducedMotion,
    pub always_redact: Vec<String>,
    pub first_run_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::System,
            sensitivity: Sensitivity::Balanced,
            pii: false,
            strip_invisible: true,
            hotkey: DEFAULT_HOTKEY.to_string(),
            tray_mode: true,
            auto_wipe_minutes: 10,
            wipe_on_lock: true,
            content_protection: true,
            placeholder_style: PlaceholderStyle::Labeled,
            reduced_motion: ReducedMotion::System,
            always_redact: Vec::new(),
            first_run_done: false,
        }
    }
}

/// Why settings were refused. Shown to the user as they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsError {
    Hotkey(String),
    AutoWipe,
    WordTooShort,
    WordTooLong,
    WordMultiline,
    TooManyWords,
    ListTooLarge,
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingsError::Hotkey(m) => f.write_str(m),
            SettingsError::AutoWipe => f.write_str("Auto-wipe must be off, 5, 10 or 30 minutes."),
            SettingsError::WordTooShort => write!(
                f,
                "Always-redact words need at least {MIN_ALWAYS_REDACT} characters."
            ),
            SettingsError::WordTooLong => write!(
                f,
                "Always-redact words are at most {MAX_ALWAYS_REDACT_LEN} characters."
            ),
            SettingsError::WordMultiline => f.write_str("One line per always-redact word."),
            SettingsError::TooManyWords => write!(
                f,
                "The always-redact list holds up to {MAX_ALWAYS_REDACT} entries."
            ),
            SettingsError::ListTooLarge => f.write_str("The always-redact list is too large."),
        }
    }
}

impl std::error::Error for SettingsError {}

const AUTO_WIPE: [u32; 4] = [0, 5, 10, 30];

impl Settings {
    /// Checks settings that came over IPC and returns their normalized form: the hotkey in
    /// canonical spelling, always-redact words trimmed and de-duplicated (case-insensitive).
    pub fn validated(mut self) -> Result<Settings, SettingsError> {
        let hk = Hotkey::parse(&self.hotkey).map_err(|e| SettingsError::Hotkey(e.to_string()))?;
        self.hotkey = hk.to_string();
        if !AUTO_WIPE.contains(&self.auto_wipe_minutes) {
            return Err(SettingsError::AutoWipe);
        }
        // Size first, so an oversized list costs nothing more than counting it.
        if self.always_redact.len() > MAX_ALWAYS_REDACT {
            return Err(SettingsError::TooManyWords);
        }
        if self.always_redact.iter().map(String::len).sum::<usize>() > MAX_ALWAYS_REDACT_BYTES {
            return Err(SettingsError::ListTooLarge);
        }
        let mut seen = HashSet::with_capacity(self.always_redact.len());
        let mut words: Vec<String> = Vec::with_capacity(self.always_redact.len());
        for raw in &self.always_redact {
            if raw.contains(['\r', '\n']) {
                return Err(SettingsError::WordMultiline);
            }
            let w = raw.trim();
            let n = w.chars().count();
            if n < MIN_ALWAYS_REDACT {
                return Err(SettingsError::WordTooShort);
            }
            if n > MAX_ALWAYS_REDACT_LEN {
                return Err(SettingsError::WordTooLong);
            }
            if seen.insert(w.to_lowercase()) {
                words.push(w.to_string());
            }
        }
        self.always_redact = words;
        Ok(self)
    }

    /// Repairs settings read from disk: every invalid field falls back to its default, so one
    /// bad value (an edited file, an older version) never resets the rest.
    pub fn sanitized(self) -> Settings {
        let d = Settings::default();
        let mut s = self;
        s.hotkey = Hotkey::parse(&s.hotkey)
            .map(|h| h.to_string())
            .unwrap_or(d.hotkey);
        if !AUTO_WIPE.contains(&s.auto_wipe_minutes) {
            s.auto_wipe_minutes = d.auto_wipe_minutes;
        }
        let mut seen = HashSet::new();
        let mut words: Vec<String> = Vec::new();
        for raw in &s.always_redact {
            if words.len() == MAX_ALWAYS_REDACT {
                break;
            }
            if raw.len() > MAX_ALWAYS_REDACT_LEN * 4 + MAX_PADDING_BYTES
                || raw.contains(['\r', '\n'])
            {
                continue;
            }
            let w = raw.trim();
            let n = w.chars().count();
            if (MIN_ALWAYS_REDACT..=MAX_ALWAYS_REDACT_LEN).contains(&n)
                && seen.insert(w.to_lowercase())
            {
                words.push(w.to_string());
            }
        }
        s.always_redact = words;
        s
    }
}

/// Where the settings live.
#[derive(Clone, Debug)]
pub struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    pub fn new(path: PathBuf) -> SettingsFile {
        SettingsFile { path }
    }

    /// `%APPDATA%\Redactor\settings.json`, or None when `APPDATA` is not set.
    pub fn in_appdata() -> Option<SettingsFile> {
        let base = std::env::var_os("APPDATA")?;
        Some(SettingsFile::new(
            Path::new(&base).join("Redactor").join("settings.json"),
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The stored settings, repaired; defaults when the file is missing or unreadable.
    pub fn load(&self) -> Settings {
        let Ok(meta) = std::fs::metadata(&self.path) else {
            return Settings::default();
        };
        if meta.len() > MAX_FILE_BYTES {
            return Settings::default();
        }
        std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Settings>(&b).ok())
            .map(Settings::sanitized)
            .unwrap_or_default()
    }

    /// Writes atomically: a temporary file next to the target, then a rename.
    pub fn save(&self, settings: &Settings) -> std::io::Result<()> {
        let dir = self
            .path
            .parent()
            .ok_or_else(|| std::io::Error::other("settings path has no parent"))?;
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_vec_pretty(settings).map_err(std::io::Error::other)?;
        let tmp = self.path.with_extension("json.tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&json)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> SettingsFile {
        let dir = std::env::temp_dir().join(format!(
            "redactor-settings-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        SettingsFile::new(dir.join("settings.json"))
    }

    fn cleanup(f: &SettingsFile) {
        if let Some(dir) = f.path().parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn settings_roundtrip_has_no_text_fields() {
        let s = Settings {
            always_redact: vec!["Nightjar".into(), "build-07.corp.example".into()],
            first_run_done: true,
            ..Settings::default()
        };
        let json = serde_json::to_value(&s).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        // Exactly the contract's fields: nothing that could carry scanned text or a secret.
        assert_eq!(
            keys,
            [
                "alwaysRedact",
                "autoWipeMinutes",
                "contentProtection",
                "firstRunDone",
                "hotkey",
                "pii",
                "placeholderStyle",
                "reducedMotion",
                "sensitivity",
                "stripInvisible",
                "theme",
                "trayMode",
                "wipeOnLock",
            ]
        );
        // Unknown fields from the UI are dropped, never stored.
        let mut extra = json.clone();
        extra["text"] = "pasted input".into();
        extra["secret"] = "x".into();
        let back: Settings = serde_json::from_value(extra).unwrap();
        assert_eq!(back, s);

        let f = temp_file("roundtrip");
        f.save(&s).unwrap();
        assert_eq!(f.load(), s);
        let raw = std::fs::read_to_string(f.path()).unwrap();
        assert!(!raw.contains("pasted input"));
        cleanup(&f);
    }

    #[test]
    fn defaults_match_the_ui() {
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(json["theme"], "system");
        assert_eq!(json["sensitivity"], "balanced");
        assert_eq!(json["placeholderStyle"], "labeled");
        assert_eq!(json["reducedMotion"], "system");
        assert_eq!(json["hotkey"], "Ctrl+Alt+R");
        assert_eq!(json["autoWipeMinutes"], 10);
        assert_eq!(json["trayMode"], true);
        assert_eq!(json["contentProtection"], true);
        assert_eq!(json["firstRunDone"], false);
    }

    #[test]
    fn missing_file_and_garbage_give_defaults() {
        let f = temp_file("garbage");
        assert_eq!(f.load(), Settings::default());
        std::fs::create_dir_all(f.path().parent().unwrap()).unwrap();
        std::fs::write(f.path(), b"{ not json").unwrap();
        assert_eq!(f.load(), Settings::default());
        cleanup(&f);
    }

    #[test]
    fn load_repairs_single_fields() {
        let f = temp_file("repair");
        std::fs::create_dir_all(f.path().parent().unwrap()).unwrap();
        std::fs::write(
            f.path(),
            br#"{"theme":"dark","hotkey":"Ctrl+Alt+A","autoWipeMinutes":7,"alwaysRedact":["ab","Acme","ACME"," Nightjar "]}"#,
        )
        .unwrap();
        let s = f.load();
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);
        assert_eq!(s.auto_wipe_minutes, 10);
        assert_eq!(s.always_redact, ["Acme", "Nightjar"]);
        cleanup(&f);
    }

    #[test]
    fn validated_normalizes_and_refuses() {
        let s = Settings {
            hotkey: "alt+ctrl+k".into(),
            always_redact: vec![
                " Müller GmbH & Co. KG ".into(),
                "müller gmbh & co. kg".into(),
            ],
            ..Settings::default()
        };
        let v = s.validated().unwrap();
        assert_eq!(v.hotkey, "Ctrl+Alt+K");
        assert_eq!(v.always_redact, ["Müller GmbH & Co. KG"]);

        let with = |f: fn(&mut Settings)| {
            let mut s = Settings::default();
            f(&mut s);
            s.validated()
        };
        assert!(matches!(
            with(|s| s.hotkey = "Ctrl+R".into()),
            Err(SettingsError::Hotkey(_))
        ));
        assert_eq!(
            with(|s| s.auto_wipe_minutes = 3),
            Err(SettingsError::AutoWipe)
        );
        assert_eq!(
            with(|s| s.always_redact = vec!["ab".into()]),
            Err(SettingsError::WordTooShort)
        );
        assert_eq!(
            with(|s| s.always_redact = vec!["two\nlines".into()]),
            Err(SettingsError::WordMultiline)
        );
        // Over the limit before de-duplication: refused by count, without comparing words.
        assert_eq!(
            with(|s| s.always_redact = vec!["same".into(); MAX_ALWAYS_REDACT + 1]),
            Err(SettingsError::TooManyWords)
        );
        assert_eq!(
            with(|s| s.always_redact = vec![" ".repeat(MAX_ALWAYS_REDACT_BYTES + 1)]),
            Err(SettingsError::ListTooLarge)
        );
        // A full list of distinct words, many differing only in case, is fine and fast.
        let full = with(|s| {
            s.always_redact = (0..MAX_ALWAYS_REDACT)
                .map(|i| {
                    if i % 2 == 0 {
                        format!("word{}", i / 2)
                    } else {
                        format!("WORD{}", i / 2)
                    }
                })
                .collect()
        })
        .unwrap();
        assert_eq!(full.always_redact.len(), MAX_ALWAYS_REDACT / 2);
        assert_eq!(
            with(|s| s.always_redact = (0..=MAX_ALWAYS_REDACT)
                .map(|i| format!("word{i}"))
                .collect()),
            Err(SettingsError::TooManyWords)
        );
    }
}
